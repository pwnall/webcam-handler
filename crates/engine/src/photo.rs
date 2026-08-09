//! Taking a photo, from the request to the bytes on disk (design D6).
//!
//! The assembly, and only the assembly: [`crate::capture`] gets the frame,
//! `imaging::photo` decides pass-through versus re-encode (E6), `imaging::exif` writes the
//! metadata, and this module owns the one thing neither of them may — the *file*.
//!
//! ## Why the sink lives here and not in the renderer
//!
//! `-o out.jpg` has to mean the same thing in `wch` and in `wchc` (D10), and the way that
//! is guaranteed is that both turn the flag into a [`Sink`] and hand it to this function.
//! A CLI that wrote the bytes itself would be a second answer to "where do photos go", and
//! the two would diverge the first time the daemon needed a different rule.
//!
//! ## Stamping is not re-encoding
//!
//! EXIF lives in an APP1 segment ahead of the entropy-coded scan, so writing one rewrites
//! the file's header and leaves the camera's bitstream alone. That is what makes a stamped
//! photo still a *verbatim* photo under E6, and `imaging::exif`'s own tests assert the scan
//! survives byte-identical rather than taking the writer's word for it.

use std::collections::BTreeMap;

use camino::Utf8Path;
use schema::backend::Camera;
use schema::capture::{PhotoDelivery, PhotoFormat, PhotoReport, PhotoRequest, Sink};
use schema::control::{ControlSlug, ControlValue};
use schema::error::{Error, Result};
use schema::time::Stamp;

use crate::capture;
use crate::settle::Clock;

/// A photo, and — when the sink asked for them — its bytes.
///
/// The bytes are *beside* the report rather than in it, and that is D10's split showing
/// through: [`PhotoReport`] crosses the wire and gets serialized, and a `Vec<u8>` in a
/// JSON document needs an encoding that only the wire surface needs (P4). Here the caller
/// already has the bytes in memory, so handing them over costs nothing and commits to
/// nothing.
#[derive(Debug)]
pub struct Photograph {
    /// What was taken, where it went, and what was done to it.
    pub report: PhotoReport,
    /// The bytes, for a [`Sink::ReturnBytes`] request. `None` when they were written to a
    /// path — a caller who asked for a file gets a file, not a file and a copy.
    pub returned: Option<Vec<u8>>,
}

/// Take one photo (design D5, D6, D10).
///
/// `now` and `clock` are both arguments because the engine reads no clock: `now` is the
/// wall time that goes in the EXIF, and `clock` is the monotonic one the settle policy
/// runs on. They are different things and conflating them is how an NTP step becomes a
/// settle failure.
///
/// # Errors
///
/// The device's, from starting the stream or waiting for a frame;
/// [`schema::Error::SettleTimeout`] when the sensor did not settle in time;
/// [`schema::Error::StorageIo`] when the sink's path could not be written.
pub fn take(
    camera: &mut dyn Camera,
    request: &PhotoRequest,
    clock: &dyn Clock,
    now: Stamp,
) -> Result<Photograph> {
    let camera_id = camera.info().id.clone();
    let fingerprint = camera.info().fingerprint.clone();
    // Read *before* the stream starts. A control read is an ioctl on the same fd, and the
    // values that describe a photo are the ones in effect when it was taken — asking
    // after the frame would report values a caller could have changed in between.
    let controls = controls_in_effect(camera);

    let captured = capture::grab(camera, &request.stream, request.settle, clock)?;
    let format = sink_format(&request.sink);
    let photo = imaging::photo::render(&captured.frame, format, request.transform)?;

    let bytes = if format == PhotoFormat::Jpeg {
        // Both JPEG paths are stamped, the verbatim one included: the orientation *is*
        // the transform on that path (E6), so a pass-through photo with no EXIF would be
        // a photo that silently dropped what the caller asked for.
        imaging::exif::stamp_jpeg(
            &photo.bytes,
            &imaging::exif::CaptureMetadata {
                captured_at: now,
                camera: fingerprint,
                negotiated: captured.negotiated.clone(),
                transform: request.transform,
                controls,
            },
        )?
    } else {
        // PNG and PPM carry no EXIF. The transform is already in the pixels on those
        // paths, so nothing is lost — which `TransformApplication` records, so a reader
        // does not have to know that rule to interpret the answer.
        photo.bytes.clone()
    };

    let delivery = deliver(&request.sink, format, &bytes)?;
    let returned = matches!(delivery, PhotoDelivery::Bytes { .. }).then_some(bytes);

    Ok(Photograph {
        report: PhotoReport {
            camera: camera_id,
            taken_at: now,
            negotiated: captured.negotiated,
            rendering: photo.rendering,
            transform: photo.transform,
            width: photo.width,
            height: photo.height,
            frames_settled: captured.frames_settled,
            delivery,
        },
        returned,
    })
}

/// Which encoding a sink wants.
fn sink_format(sink: &Sink) -> PhotoFormat {
    match sink {
        Sink::ReturnBytes { format } => *format,
        // The extension is the request. An unknown one never reaches here — the CLI
        // refuses it while building the sink, where the message can name the three
        // formats this build writes — so a path that got this far is one of them, and
        // JPEG is the answer for the only other case there is: a path with no extension
        // at all, which is a filename the caller chose and we should not rename.
        Sink::ServerPath { path } => path
            .extension()
            .and_then(PhotoFormat::from_extension)
            .unwrap_or(PhotoFormat::Jpeg),
    }
}

/// The control values in effect, for the photo's own record.
///
/// Best effort by construction: a camera that cannot enumerate its controls can still take
/// a photo, and refusing the photo over the *metadata* would be letting the record decide
/// whether the picture happens. An empty map renders as "(none recorded)", which is the
/// honest thing to say.
fn controls_in_effect(camera: &mut dyn Camera) -> BTreeMap<ControlSlug, ControlValue> {
    camera.controls().map_or_else(
        |_| BTreeMap::new(),
        |controls| {
            controls
                .into_iter()
                .filter_map(|desc| Some((desc.slug, desc.current?)))
                .collect()
        },
    )
}

/// Put the bytes where the sink says (design D10).
///
/// # Errors
///
/// [`Error::StorageIo`] naming the path. A photo that could not be written is a failure
/// even though the capture succeeded — reporting success with the bytes discarded would
/// be the worst of both.
fn deliver(sink: &Sink, format: PhotoFormat, bytes: &[u8]) -> Result<PhotoDelivery> {
    let byte_count = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    match sink {
        Sink::ReturnBytes { .. } => Ok(PhotoDelivery::Bytes { format, byte_count }),
        Sink::ServerPath { path } => {
            write_photo(path, bytes)?;
            Ok(PhotoDelivery::Path {
                path: path.clone(),
                byte_count,
            })
        }
    }
}

/// Write a photo to a path, reporting a filesystem failure as one.
///
/// Not `write_json_atomic`: that is the session store's protocol for documents the tool
/// re-reads and must never find half-written (D9). A photo is written once for a human or
/// an agent, at a path they named, and a temp-file-plus-rename would silently break the
/// case where that path is a fifo or `/dev/stdout`.
fn write_photo(path: &Utf8Path, bytes: &[u8]) -> Result<()> {
    std::fs::write(path, bytes).map_err(|error| Error::StorageIo {
        path: path.to_owned(),
        errno: error.raw_os_error(),
        message: error.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use exif::{In, Tag};
    use fake::FakeBackend;
    use schema::ErrorKind;
    use schema::backend::CameraBackend;
    use schema::camera::PixelFormat;
    use schema::capture::{
        SettlePolicy, SettleSpec, StreamRequest, Transform, TransformApplication,
    };

    use super::*;
    use crate::settle::SteppedClock;

    /// A photo request that settles immediately, so the tests are about D6 and not D5.
    fn request(sink: Sink, transform: Transform) -> PhotoRequest {
        PhotoRequest {
            stream: StreamRequest::default(),
            settle: SettlePolicy {
                spec: SettleSpec::SkipFrames { frames: 0 },
                deadline_ms: 5_000,
            },
            transform,
            sink,
        }
    }

    /// The fake replaying a committed profile — the modules whose subject is realistic
    /// device behaviour use it, and a photo is exactly that.
    fn camera_from(profile: &str) -> Box<dyn Camera> {
        let profile = testkit::corpus::load(profile).expect("a committed profile");
        let backend = FakeBackend::from_profile(profile).expect("replays");
        let id = backend
            .enumerate()
            .expect("enumerate")
            .into_iter()
            .next()
            .expect("one camera")
            .id;
        backend.open(&id).expect("opens")
    }

    #[test]
    fn a_jpeg_photo_carries_exif_an_independent_reader_can_read_back() {
        // docs/9 names write-only EXIF as a defect class: a writer that returns `Ok`
        // having produced a segment no reader accepts passes every test that trusts it.
        // `kamadak-exif` shares no code with `little_exif`, which is the whole point.
        let mut camera = camera_from("chicony-rgb");
        let taken = take(
            camera.as_mut(),
            &request(
                Sink::ReturnBytes {
                    format: PhotoFormat::Jpeg,
                },
                Transform::None,
            ),
            &SteppedClock::new(0),
            Stamp::epoch(),
        )
        .expect("takes a photo");

        assert!(matches!(taken.report.delivery, PhotoDelivery::Bytes { .. }));
        assert_eq!(taken.report.transform, TransformApplication::Identity);

        // The bytes, read by an implementation that shares no code with the writer. The
        // first version of this test asserted only that the *report* looked right, which
        // is precisely the write-only-EXIF defect its own name promises to catch: a
        // writer that returns `Ok` having produced a segment no reader accepts passes
        // every test that trusts it.
        let bytes = taken
            .returned
            .expect("a ReturnBytes sink hands the bytes back");
        assert_eq!(
            u64::try_from(bytes.len()).expect("fits"),
            taken.report.delivery.byte_count()
        );
        let mut cursor = std::io::Cursor::new(bytes);
        let exif = exif::Reader::new()
            .read_from_container(&mut cursor)
            .expect("the stamped bytes carry readable EXIF");
        assert!(
            exif.get_field(Tag::Make, In::PRIMARY)
                .expect("the driver is recorded as Make")
                .display_value()
                .to_string()
                .contains("uvcvideo")
        );
        assert_eq!(
            exif.get_field(Tag::Orientation, In::PRIMARY)
                .expect("orientation")
                .value
                .get_uint(0),
            Some(1),
            "an untransformed photo is orientation 1"
        );
    }

    #[test]
    fn a_photo_written_to_a_path_reports_the_path_and_the_bytes_it_holds() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = camino::Utf8PathBuf::from_path_buf(dir.path().join("shot.jpg"))
            .expect("utf-8 temp dir");
        let mut camera = camera_from("chicony-rgb");
        let report = take(
            camera.as_mut(),
            &request(Sink::ServerPath { path: path.clone() }, Transform::None),
            &SteppedClock::new(0),
            Stamp::epoch(),
        )
        .expect("takes a photo")
        .report;

        let PhotoDelivery::Path {
            path: reported,
            byte_count,
        } = &report.delivery
        else {
            panic!("a path sink must report a path");
        };
        assert_eq!(reported, &path);
        let written = std::fs::read(&path).expect("the file exists");
        assert_eq!(u64::try_from(written.len()).expect("fits"), *byte_count);

        // And the EXIF reads back through an independent implementation, off the file
        // rather than off the in-memory buffer — the bytes that reached the disk are the
        // ones a user will open.
        let mut cursor = std::io::Cursor::new(written);
        let exif = exif::Reader::new()
            .read_from_container(&mut cursor)
            .expect("the stamped file carries readable EXIF");
        let make = exif
            .get_field(Tag::Make, In::PRIMARY)
            .expect("the driver is recorded as Make");
        assert!(
            make.display_value().to_string().contains("uvcvideo"),
            "{make:?}"
        );
    }

    #[test]
    fn a_transform_on_the_verbatim_path_becomes_an_orientation_tag_and_moves_no_pixels() {
        // E6: rotating a `.jpg` must not cost a re-encode. The report says which happened,
        // because "the photo is rotated" and "the photo says it is rotated" are different
        // facts and a viewer that ignores EXIF distinguishes them.
        let mut camera = camera_from("chicony-rgb");
        let report = take(
            camera.as_mut(),
            &request(
                Sink::ReturnBytes {
                    format: PhotoFormat::Jpeg,
                },
                Transform::Rot90,
            ),
            &SteppedClock::new(0),
            Stamp::epoch(),
        )
        .expect("takes a photo")
        .report;

        assert!(report.rendering.is_verbatim(), "{:?}", report.rendering);
        assert_eq!(
            report.transform,
            TransformApplication::ExifOrientation { orientation: 6 }
        );
        assert_eq!(
            (report.width, report.height),
            (report.negotiated.width, report.negotiated.height),
            "nothing was rotated, so the dimensions are the frame's"
        );
    }

    #[test]
    fn a_png_sink_moves_the_pixels_instead_and_swaps_the_axes() {
        // The other direction of the same claim, so `is_verbatim` and
        // `TransformApplication` are both measuring something.
        let dir = tempfile::tempdir().expect("temp dir");
        let path = camino::Utf8PathBuf::from_path_buf(dir.path().join("shot.png"))
            .expect("utf-8 temp dir");
        let mut camera = camera_from("chicony-rgb");
        let report = take(
            camera.as_mut(),
            &request(Sink::ServerPath { path }, Transform::Rot90),
            &SteppedClock::new(0),
            Stamp::epoch(),
        )
        .expect("takes a photo")
        .report;

        assert!(!report.rendering.is_verbatim());
        assert_eq!(report.transform, TransformApplication::Pixels);
        assert_eq!(
            (report.width, report.height),
            (report.negotiated.height, report.negotiated.width),
            "a quarter turn swaps the axes"
        );
    }

    #[test]
    fn the_grey_camera_produces_a_decodable_jpeg_because_grayscale_is_not_optional() {
        // D6's own phrasing, made a test: the Chicony IR camera's only format is GREY,
        // which is not a bitstream, so a `.jpg` from it is necessarily a re-encode — and
        // the record must say so rather than claiming a byte fidelity it does not have.
        let mut camera = camera_from("chicony-ir");
        let report = take(
            camera.as_mut(),
            &request(
                Sink::ReturnBytes {
                    format: PhotoFormat::Jpeg,
                },
                Transform::None,
            ),
            &SteppedClock::new(0),
            Stamp::epoch(),
        )
        .expect("a grayscale camera takes photos")
        .report;

        assert_eq!(report.negotiated.pixel_format, PixelFormat::GREY);
        assert!(
            !report.rendering.is_verbatim(),
            "GREY is not a bitstream; there is nothing to pass through"
        );
        assert!(report.delivery.byte_count() > 0);
    }

    #[test]
    fn the_control_values_in_effect_reach_the_photos_own_record() {
        // A calibration sample is self-describing without its session file (D6). The
        // fixture's controls are the profile's, so the assertion is that *something the
        // device reported* made it into the comment rather than that a specific value did.
        let mut camera = camera_from("chicony-rgb");
        let controls = controls_in_effect(camera.as_mut());
        assert!(!controls.is_empty(), "the replayed profile has controls");

        let metadata = imaging::exif::CaptureMetadata {
            captured_at: Stamp::epoch(),
            camera: camera.info().fingerprint.clone(),
            negotiated: schema::capture::NegotiatedStream {
                pixel_format: PixelFormat::MJPG,
                width: 1280,
                height: 720,
                bytes_per_line: 0,
                size_image: 1 << 20,
                interval: schema::camera::FrameInterval::Discrete {
                    numerator: 1,
                    denominator: 30,
                },
                adjustments: Vec::new(),
            },
            transform: Transform::None,
            controls: controls.clone(),
        };
        let comment =
            String::from_utf8_lossy(&imaging::exif::describe_controls(&metadata)).into_owned();
        let (slug, value) = controls.iter().next().expect("at least one control");
        assert!(comment.contains(&format!("{slug}={value}")), "{comment}");
    }

    #[test]
    fn a_photo_to_a_path_that_cannot_be_written_is_a_storage_failure_naming_it() {
        // The capture succeeded and the photo did not land. Reporting success would be
        // the worst of both: a caller would believe there is a file.
        let mut camera = camera_from("chicony-rgb");
        let error = take(
            camera.as_mut(),
            &request(
                Sink::ServerPath {
                    path: "/nonexistent-directory-for-a-test/shot.jpg".into(),
                },
                Transform::None,
            ),
            &SteppedClock::new(0),
            Stamp::epoch(),
        )
        .expect_err("there is no such directory");
        assert_eq!(error.kind(), ErrorKind::StorageIo);
        assert!(error.to_string().contains("shot.jpg"), "{error}");
    }

    #[test]
    fn a_camera_with_no_capture_node_refuses_the_photo_rather_than_producing_one() {
        // D1's metadata-only shape. The refusal is a capability answer and it arrives
        // from the backend unchanged — the photo pipeline adds nothing to it.
        let mut profile = testkit::fixtures::synthetic_basic();
        profile
            .invariant
            .info
            .nodes
            .retain(|node| node.kind != schema::camera::NodeKind::VideoCapture);
        let backend = FakeBackend::from_profile(profile).expect("replays");
        let id = backend
            .enumerate()
            .expect("enumerate")
            .into_iter()
            .next()
            .expect("one camera")
            .id;
        let mut camera = backend.open(&id).expect("opens");

        let error = take(
            camera.as_mut(),
            &request(
                Sink::ReturnBytes {
                    format: PhotoFormat::Jpeg,
                },
                Transform::None,
            ),
            &SteppedClock::new(0),
            Stamp::epoch(),
        )
        .expect_err("a camera with no capture node cannot be streamed");
        assert_eq!(error.kind(), ErrorKind::FormatUnsupported);
    }

    #[test]
    fn a_sinks_format_comes_from_its_extension_and_a_bare_path_stays_a_jpeg() {
        for (path, expected) in [
            ("/tmp/a.jpg", PhotoFormat::Jpeg),
            ("/tmp/a.jpeg", PhotoFormat::Jpeg),
            ("/tmp/a.png", PhotoFormat::Png),
            ("/tmp/a.ppm", PhotoFormat::Ppm),
            ("/tmp/a.pgm", PhotoFormat::Ppm),
            // No extension: the caller named this file and we do not get to rename it.
            ("/tmp/photo", PhotoFormat::Jpeg),
        ] {
            assert_eq!(
                sink_format(&Sink::ServerPath { path: path.into() }),
                expected,
                "{path}"
            );
        }
        assert_eq!(
            sink_format(&Sink::ReturnBytes {
                format: PhotoFormat::Ppm
            }),
            PhotoFormat::Ppm
        );
    }
}
