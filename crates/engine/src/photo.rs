//! Taking a photo, from the request to the bytes on disk (design D6).
//!
//! The assembly, and only the assembly: [`crate::capture`] gets the frame,
//! `imaging::photo` decides pass-through versus re-encode (E6), `imaging::exif` writes the
//! metadata, and this module owns the one thing neither of them may — the *file*.
//!
//! ## Why the sink lives here and not in the renderer
//!
//! `-o out.jpg` has to mean the same thing in `webcam-handler-cli` and in
//! `webcam-handler-client` (D10), and the way that is guaranteed is that both turn the flag
//! into a [`Sink`] and hand it to this function. A CLI that wrote the bytes itself would be a
//! second answer to "where do photos go", and the two would diverge the first time the daemon
//! needed a different rule.
//!
//! ## Stamping is not re-encoding
//!
//! EXIF lives in an APP1 segment ahead of the entropy-coded scan, so writing one rewrites
//! the file's header and leaves the camera's bitstream alone. That is what makes a stamped
//! photo still a *verbatim* photo under E6, and `imaging::exif`'s own tests assert the scan
//! survives byte-identical rather than taking the writer's word for it.
//!
//! ## Why *how* the file is opened is a seam and *where* it goes is not
//!
//! [`Destination`] is the one thing about the write that differs between this module's two
//! callers, and note **N51** is why it has to. `webcam-handler-cli` resolves `-o` on a command
//! line somebody typed and opens it when the bytes are ready — a fifo or `/dev/stdout` is a
//! feature there, and a person who typed one has Ctrl-C. The daemon has none of that: an
//! `open(2)` that blocks runs inside a camera's actor thread, so a client that named a fifo
//! would park that camera for the life of the process. So the daemon supplies its own
//! [`Destination`], which resolves the name to a *descriptor* before it touches a camera at
//! all — and *where* photos go is still answered exactly once, here, by this module's own
//! `deliver`.

use std::collections::BTreeMap;
use std::io::Write as _;

use camino::Utf8Path;
use schema::backend::Camera;
use schema::capture::{PhotoDelivery, PhotoFormat, PhotoReport, PhotoRequest, Sink};
use schema::control::{ControlSlug, ControlValue};
use schema::error::{Error, Result};
use schema::time::Stamp;

use crate::actor::OpenCamera;
use crate::capture;
use crate::preview::Gap;
use crate::settle::Clock;

/// A photo, and — when the sink asked for them — its bytes.
///
/// The bytes are *beside* the report rather than in it, and that is D10's split showing
/// through: [`PhotoReport`] crosses the wire and gets serialized, and a `Vec<u8>` in a
/// JSON document needs an encoding that only the wire surface needs (P4). Here the caller
/// already has the bytes in memory, so handing them over costs nothing and commits to
/// nothing.
///
/// `Debug` is hand-written for the reason [`schema::capture::Frame`]'s is: **a frame may
/// contain a person** (AGENTS.md; rubric A12). A derived one prints
/// `Some([255, 216, 255, …])` — a whole JPEG of whoever was in front of the camera — into
/// whatever `tracing::debug!(?photograph)` or `.expect(&format!("{photograph:?}"))` a
/// later sub-milestone adds, and no lint or gate can go red on a line like that. The rule
/// has four subjects now (`Frame`, `api::photo::Base64Bytes`, and both `Photograph`s) and
/// four tests; note N36 records why it does not yet have a walkable population.
pub struct Photograph {
    /// What was taken, where it went, and what was done to it.
    pub report: PhotoReport,
    /// The bytes, for a [`Sink::ReturnBytes`] request. `None` when they were written to a
    /// path — a caller who asked for a file gets a file, not a file and a copy.
    pub returned: Option<Vec<u8>>,
}

impl std::fmt::Debug for Photograph {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        /// The byte count, wearing the only `Debug` a payload may have.
        ///
        /// A shim rather than `format_args!` because the bytes are behind an `Option` and
        /// a `format_args!` built in a closure cannot outlive it — and going through
        /// `Option`'s own `Debug` is what keeps `Some(…)`/`None` distinguishable, which is
        /// the difference between "a file was written" and "an empty payload came back".
        struct ByteCount(usize);
        impl std::fmt::Debug for ByteCount {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "<{} bytes>", self.0)
            }
        }

        // A frame may contain a person. The count, and never the bytes.
        f.debug_struct("Photograph")
            .field("report", &self.report)
            .field(
                "returned",
                &self.returned.as_ref().map(|bytes| ByteCount(bytes.len())),
            )
            .finish()
    }
}

/// How a photo's bytes reach the path a [`Sink::ServerPath`] names.
///
/// A seam and not a constant, because the two callers want different answers and both are
/// right — see this module's header, and note **N51** for the measurement. The trait is the
/// *whole* delivery rather than only the `open` so that a caller which resolved the
/// destination in advance can also decide when to truncate it: truncating a file before the
/// bytes exist destroys an operator's photo on the way to reporting that the capture failed.
///
/// `Send` because the daemon moves one into a camera actor's closure; deliberately not
/// `Sync`, because a destination is one request's and sharing one between two photos is a
/// question nothing here needs to answer.
pub trait Destination: std::fmt::Debug + Send {
    /// Put `bytes` at `path`.
    ///
    /// # Errors
    ///
    /// [`Error::StorageIo`] naming the path, when the bytes could not be written.
    fn write(&mut self, path: &Utf8Path, bytes: &[u8]) -> Result<()>;
}

/// Open the path when the bytes are ready, and write them — what this module has always
/// done.
///
/// `std::fs::write`'s semantics exactly, and not `write_json_atomic`'s: that is the session
/// store's protocol for documents the tool re-reads and must never find half-written (D9). A
/// photo is written once for a human or an agent, at a path they named, and a
/// temp-file-plus-rename would silently break the case where that path is a fifo or
/// `/dev/stdout` — which for `webcam-handler-cli` is a feature rather than a hazard.
#[derive(Debug, Default, Clone, Copy)]
pub struct WhereverTheCallerSaid;

impl Destination for WhereverTheCallerSaid {
    fn write(&mut self, path: &Utf8Path, bytes: &[u8]) -> Result<()> {
        std::fs::write(path, bytes).map_err(|error| Error::StorageIo {
            path: path.to_owned(),
            errno: error.raw_os_error(),
            message: error.to_string(),
        })
    }
}

/// Put `bytes` on an already-open descriptor, and cut whatever was there beyond them.
///
/// The other half of note **N51**'s answer, and the only piece of it that belongs to the
/// engine: a caller that resolved a destination *before* it opened a camera has a
/// `std::fs::File` rather than a name, and the write has to be expressible against one.
/// Public because the daemon's [`Destination`] is where the resolution lives — that part is
/// the transport's, for the reason `daemon::server::addressable` gives — and this is the
/// half that must not be written twice.
///
/// **The truncation happens here rather than at the open**, and that ordering is the fix's
/// quiet half: `O_TRUNC` on a descriptor opened before the capture would empty an operator's
/// existing photo and then report that the camera failed. `set_len` after `write_all` leaves
/// a file exactly `bytes` long whether it was longer, shorter or absent.
///
/// # Errors
///
/// [`Error::StorageIo`] naming `path`, which is used for the message only — the bytes go to
/// the descriptor, which is the whole point.
pub fn write_to_open_file(file: &mut std::fs::File, path: &Utf8Path, bytes: &[u8]) -> Result<()> {
    let storage_io = |error: &std::io::Error| Error::StorageIo {
        path: path.to_owned(),
        errno: error.raw_os_error(),
        message: error.to_string(),
    };
    file.write_all(bytes).map_err(|error| storage_io(&error))?;
    let length = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    file.set_len(length).map_err(|error| storage_io(&error))?;
    Ok(())
}

/// A photo attempt, and what it cost a preview that was running.
///
/// **Two facts, and neither may hide the other.** The photo is the caller's answer; the
/// [`Gap`] is what the *camera's viewers* saw, and it exists on the path where the caller has
/// nothing else — a capture that failed still stopped a preview and still started it again,
/// and a gap nobody counted is exactly the silence rubric rule 3 is about. A
/// `Result<Photograph>` cannot carry the second on that path, which is why this type exists
/// rather than a field on [`Photograph`].
///
/// `outcome` first, because it is what nearly every caller wants: `take(…).outcome?` is the
/// whole of the migration for a caller that runs no previews (`webcam-handler-cli`, and every
/// test here).
#[derive(Debug)]
pub struct Taken {
    /// The photo, or the refusal that stood in for it.
    pub outcome: Result<Photograph>,
    /// What this photo did to a preview, when there was one to interrupt. `None` when the
    /// camera was not streaming for anybody — which is every `webcam-handler-cli` invocation
    /// and every daemon request that arrived with no tab open.
    pub gap: Option<Gap>,
}

/// Take one photo (design D5, D6, D10).
///
/// `now` and `clock` are both arguments because the engine reads no clock: `now` is the
/// wall time that goes in the EXIF, and `clock` is the monotonic one the settle policy
/// runs on. They are different things and conflating them is how an NTP step becomes a
/// settle failure.
///
/// `destination` is an argument for the reason this module's header gives: a CLI and a daemon
/// open a caller-named path differently and both are right (note **N51**).
/// [`WhereverTheCallerSaid`] is what `webcam-handler-cli` hands in and what this pipeline did
/// before the seam existed.
///
/// ## A live preview is suspended for the capture and started again after it
///
/// V4L2 allows one streamer per node, so a camera somebody is watching used to answer
/// [`schema::Error::Busy`] here. Since the owner's ruling of 2026-08-12 it does not:
/// [`crate::preview::while_suspended`] stops that stream, takes the frame and starts it again,
/// and this is the one place that happens — **inside the photo pipeline rather than beside
/// it**, so `webcam-handler-cli`, `webcam-handler-daemon` and anything that reaches this
/// function get the same behaviour without arranging anything, and a second copy of the
/// sequence cannot drift from this one (§2.10). Note **N83** carries the ruling.
///
/// **Only the capture is inside the suspension.** The sink check, the control read, the
/// render, the EXIF stamp and the write are all outside it — none of them touches the stream,
/// and a preview held down for the length of a 4K PNG re-encode and a write to a slow disk
/// would be a pause bounded by the filesystem. What is inside is `capture::grab`, whose length
/// is the settle deadline the request carries, which is the number
/// [`schema::limits::PREVIEW_SUSPEND_MAX_MS`] is checked against.
///
/// # Errors
///
/// In [`Taken::outcome`]: the device's, from starting the stream or waiting for a frame;
/// [`schema::Error::SettleTimeout`] when the sensor did not settle in time;
/// [`schema::Error::StorageIo`] when the sink's path could not be written;
/// [`schema::Error::IllegalTransition`] from [`Sink::writable_format`] when the sink names
/// an encoding this build does not write, and from [`crate::preview::while_suspended`] when
/// the request's settle deadline is longer than a photo may hold a preview down.
pub fn take(
    camera: OpenCamera<'_>,
    request: &PhotoRequest,
    destination: &mut dyn Destination,
    clock: &dyn Clock,
    now: Stamp,
) -> Taken {
    // First, before anything is asked of the device. The refusal is about the *request* —
    // a sink naming an encoding this build does not write — so paying a `STREAMON`, the
    // whole settle budget and a `DQBUF` before making it would mean a frame of whoever is
    // in front of the lens was captured for a request that was never going to be honoured
    // (debt D-1, note N46). `from_capture` asks again, because it is also reached directly
    // by the sweep; asking twice is one rule asked twice and not two rules.
    //
    // It is also before the *suspension*, which matters for the same reason one layer up: a
    // request nobody was ever going to honour must not interrupt somebody's preview.
    if let Err(refused) = request.sink.writable_format() {
        return Taken {
            outcome: Err(refused),
            gap: None,
        };
    }
    // The same sink, asked the *other* question it can answer: whether it can carry the
    // camera's own bitstream. D5's 2026-08-13 amendment ranks two formats tied on
    // resolution by how much of the sensor's output survives the whole path to the file,
    // and that path ends here — `PhotoRequest::stream_for_sink` is the one place the two
    // halves of this request meet, and it is deliberately below the refusal above so a
    // request nobody was going to honour never gets as far as choosing a mode for it.
    let stream = request.stream_for_sink();
    // Read *before* the stream starts. A control read is an ioctl on the same fd, and the
    // values that describe a photo are the ones in effect when it was taken — asking
    // after the frame would report values a caller could have changed in between. It is
    // also legal *during* a preview — `VIDIOC_G_EXT_CTRLS` on a streaming node is ordinary
    // (D12) — so it stays outside the suspension.
    let controls = controls_in_effect(camera);
    let (captured, gap) =
        crate::preview::while_suspended(&mut *camera, request.settle.deadline_ms, |camera| {
            capture::grab(camera, &stream, request.settle, clock)
        });
    let outcome = captured
        .and_then(|captured| from_capture(camera, &captured, request, destination, controls, now));
    Taken { outcome, gap }
}

/// The same assembly, over a frame the caller already holds (design D6).
///
/// [`take`] is this function plus the capture. The split exists for exactly one caller —
/// a calibration sweep, which has to **score the frame it stores**: metrics computed from
/// a second capture would describe a second moment, and a sample whose photo and whose
/// numbers came from different frames is a comparison with nothing underneath it. So the
/// sweep grabs once ([`crate::capture::grab`]), measures the frame, and hands the same
/// one here rather than growing a second photo pipeline beside this one (§2.10).
///
/// `controls` is a parameter rather than a read, because by the time a caller has a frame
/// the stream is already up; [`controls_in_effect`] is what a caller calls *before*
/// starting it.
///
/// # Errors
///
/// As [`take`], minus the capture's: [`schema::Error::FormatUnsupported`] for a source
/// format outside D6's set, [`schema::Error::StorageIo`] when the sink's path could not be
/// written, and [`schema::Error::IllegalTransition`] when the sink names an encoding this
/// build does not write.
pub fn from_capture(
    camera: &dyn Camera,
    captured: &capture::Capture,
    request: &PhotoRequest,
    destination: &mut dyn Destination,
    controls: BTreeMap<ControlSlug, ControlValue>,
    now: Stamp,
) -> Result<Photograph> {
    let camera_id = camera.info().id.clone();
    let fingerprint = camera.info().fingerprint.clone();
    // The sink decides the encoding, and the sink is also what refuses one this build cannot
    // write — `Sink::writable_format` is that one home, beside the variants, because the rule
    // has to hold for a request a socket built as much as for one a command line did (note
    // N46, debt D-1). A caller that can refuse earlier should: `webcam-handler-daemon`
    // validates the sink before it opens a camera, and `webcam-handler-cli` refuses while
    // parsing. This is the backstop for whoever does neither.
    let format = request.sink.writable_format()?;
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

    let delivery = deliver(&request.sink, destination, format, &bytes)?;
    let returned = matches!(delivery, PhotoDelivery::Bytes { .. }).then_some(bytes);

    Ok(Photograph {
        report: PhotoReport {
            camera: camera_id,
            taken_at: now,
            negotiated: captured.negotiated.clone(),
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

/// The control values in effect, for the photo's own record.
///
/// Best effort by construction: a camera that cannot enumerate its controls can still take
/// a photo, and refusing the photo over the *metadata* would be letting the record decide
/// whether the picture happens. An empty map renders as "(none recorded)", which is the
/// honest thing to say.
///
/// Public because [`from_capture`] takes the answer as a parameter and its callers have to
/// be able to produce one — and because *when* it is read is the load-bearing part: before
/// the stream starts, never after.
#[must_use]
pub fn controls_in_effect(camera: &mut dyn Camera) -> BTreeMap<ControlSlug, ControlValue> {
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
fn deliver(
    sink: &Sink,
    destination: &mut dyn Destination,
    format: PhotoFormat,
    bytes: &[u8],
) -> Result<PhotoDelivery> {
    let byte_count = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    match sink {
        // The destination is never consulted, and that is what keeps a caller which
        // resolved one in advance from having resolved anything at all for a request that
        // asked for its bytes back.
        Sink::ReturnBytes { .. } => Ok(PhotoDelivery::Bytes { format, byte_count }),
        Sink::ServerPath { path } => {
            destination.write(path, bytes)?;
            Ok(PhotoDelivery::Path {
                path: path.clone(),
                byte_count,
            })
        }
    }
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
            // Nothing in this module queues behind anything: the tests here hold their own
            // camera. D12's flag is `daemon::server`'s to exercise.
            wait: false,
        }
    }

    /// The fake replaying a committed profile — the modules whose subject is realistic
    /// device behaviour use it, and a photo is exactly that.
    fn camera_from(profile: &str) -> Box<dyn Camera> {
        watched_camera_from(profile).1
    }

    /// The same, with the backend kept so its counters can be read.
    ///
    /// `FakeBackend::streams_started()` is the only thing that can tell "refused before the
    /// stream started" from "refused after a frame was captured and thrown away", and a
    /// test named for the first must be able to see it — the file it exists in is otherwise
    /// free to assert the weaker claim forever.
    fn watched_camera_from(profile: &str) -> (std::sync::Arc<FakeBackend>, Box<dyn Camera>) {
        let profile = testkit::corpus::load(profile).expect("a committed profile");
        let backend = std::sync::Arc::new(FakeBackend::from_profile(profile).expect("replays"));
        let id = backend
            .enumerate()
            .expect("enumerate")
            .into_iter()
            .next()
            .expect("one camera")
            .id;
        let camera = backend.open(&id).expect("opens");
        (backend, camera)
    }

    /// The Chicony with its MJPG list trimmed to the one size its YUYV list also offers.
    ///
    /// A rewrite of a committed profile into a shape a device really has — most webcams
    /// offer both formats at VGA — and it is the only way to reach the case D5's amendment
    /// of 2026-08-13 had to decide: **a compressed and an uncompressed format tied at one
    /// resolution.** No camera in `corpus/` has it, which is exactly why it is built here
    /// rather than waited for; the ruling's primary key cannot separate the two and the
    /// destination is what breaks the tie.
    fn tied_formats_camera() -> Box<dyn Camera> {
        use schema::camera::{FrameSize, PixelFormat};

        let mut profile = testkit::corpus::load("chicony-rgb").expect("a committed profile");
        for format in &mut profile.invariant.formats {
            if format.pixel_format == PixelFormat::MJPG {
                format.sizes.retain(|entry| {
                    entry.size
                        == FrameSize::Discrete {
                            width: 640,
                            height: 480,
                        }
                });
            }
        }
        let maxima: Vec<(PixelFormat, Option<(u32, u32)>)> = profile
            .invariant
            .formats
            .iter()
            .map(|format| {
                (
                    format.pixel_format,
                    format
                        .sizes
                        .iter()
                        .filter_map(|entry| entry.size.max_dimensions())
                        .max_by_key(|(w, h)| u64::from(*w) * u64::from(*h)),
                )
            })
            .collect();
        assert_eq!(
            maxima,
            vec![
                (PixelFormat::MJPG, Some((640, 480))),
                (PixelFormat::YUYV, Some((640, 480))),
            ],
            "the fixture stopped being a tie, so the tests below stopped being about one"
        );

        let backend = std::sync::Arc::new(FakeBackend::from_profile(profile).expect("replays"));
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
    fn a_tie_between_a_compressed_and_an_uncompressed_format_is_decided_by_the_sink() {
        // **Question 2 of the owner's ruling, end to end** (note N85). "Less lossy" taken
        // over the driver's buffer alone picks YUYV both times, which on a `.jpg` sink
        // throws away the camera's own bitstream to hand our encoder a frame it had no
        // reason to touch — Expected usage item 3's "a pipeline that silently re-encodes is
        // a pipeline that fabricates evidence in a test". Taken over the whole path from
        // sensor to file it picks the format that arrives least altered, which is a
        // different format for each destination.
        //
        // The assertions are on `rendering`, so what is being checked is the bytes that
        // landed rather than a field the chooser filled in.
        let mut camera = tied_formats_camera();
        let jpeg = take(
            camera.as_mut(),
            &request(
                Sink::ReturnBytes {
                    format: PhotoFormat::Jpeg,
                },
                Transform::None,
            ),
            &mut WhereverTheCallerSaid,
            &SteppedClock::new(0),
            Stamp::epoch(),
        )
        .outcome
        .expect("takes a photo");
        assert_eq!(
            jpeg.report.rendering,
            schema::capture::PhotoRendering::Verbatim {
                source: PixelFormat::MJPG
            },
            "a JPEG sink that chose the uncompressed half of a tie would re-encode a frame \
             the camera had already encoded"
        );
        assert!(jpeg.report.rendering.is_verbatim());

        let mut camera = tied_formats_camera();
        let png = take(
            camera.as_mut(),
            &request(
                Sink::ReturnBytes {
                    format: PhotoFormat::Png,
                },
                Transform::None,
            ),
            &mut WhereverTheCallerSaid,
            &SteppedClock::new(0),
            Stamp::epoch(),
        )
        .outcome
        .expect("takes a photo");
        assert_eq!(
            png.report.rendering,
            schema::capture::PhotoRendering::ConvertedAndEncoded {
                source: PixelFormat::YUYV,
                target: PhotoFormat::Png,
            },
            "a PNG sink encodes losslessly, so the source that never met the camera's \
             quantiser is the one that reaches the file intact — choosing MJPG here would \
             decode a lossy bitstream and write the loss into a lossless container"
        );

        // Same camera, same request but for the encoding, and the device was asked for two
        // different modes. That difference is the whole of question 2's answer, and it is
        // asserted rather than left to the two claims above to imply.
        assert_ne!(
            png.report.negotiated.pixel_format,
            jpeg.report.negotiated.pixel_format
        );
        // Neither answer reports an adjustment: nothing was requested for either of them to
        // differ from, and a default photo that read as negotiated-away would be a lie in
        // every `--json` document (D5's reporting rule, untouched by the amendment).
        assert!(jpeg.report.negotiated.is_exact());
        assert!(png.report.negotiated.is_exact());
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
            &mut WhereverTheCallerSaid,
            &SteppedClock::new(0),
            Stamp::epoch(),
        )
        .outcome
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
        let dir = crate::paths::scratch_dir().expect("a scratch directory");
        let path = camino::Utf8PathBuf::from_path_buf(dir.path().join("shot.jpg"))
            .expect("utf-8 temp dir");
        let mut camera = camera_from("chicony-rgb");
        let report = take(
            camera.as_mut(),
            &request(Sink::ServerPath { path: path.clone() }, Transform::None),
            &mut WhereverTheCallerSaid,
            &SteppedClock::new(0),
            Stamp::epoch(),
        )
        .outcome
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
            &mut WhereverTheCallerSaid,
            &SteppedClock::new(0),
            Stamp::epoch(),
        )
        .outcome
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
        let dir = crate::paths::scratch_dir().expect("a scratch directory");
        let path = camino::Utf8PathBuf::from_path_buf(dir.path().join("shot.png"))
            .expect("utf-8 temp dir");
        let mut camera = camera_from("chicony-rgb");
        let report = take(
            camera.as_mut(),
            &request(Sink::ServerPath { path }, Transform::Rot90),
            &mut WhereverTheCallerSaid,
            &SteppedClock::new(0),
            Stamp::epoch(),
        )
        .outcome
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
            &mut WhereverTheCallerSaid,
            &SteppedClock::new(0),
            Stamp::epoch(),
        )
        .outcome
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
            &mut WhereverTheCallerSaid,
            &SteppedClock::new(0),
            Stamp::epoch(),
        )
        .outcome
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
            &mut WhereverTheCallerSaid,
            &SteppedClock::new(0),
            Stamp::epoch(),
        )
        .outcome
        .expect_err("a camera with no capture node cannot be streamed");
        assert_eq!(error.kind(), ErrorKind::FormatUnsupported);
    }

    #[test]
    fn photo_bytes_never_reach_a_debug_line() {
        // Rubric A12 as a test, and the reason this struct hand-writes `Debug`: a frame may
        // contain a person, so formatting a document that holds one has to be incapable of
        // printing it. The photo is a real one off the fake, because the bytes that must
        // not appear have to be bytes something actually produced.
        let mut camera = camera_from("chicony-rgb");
        let taken = take(
            camera.as_mut(),
            &request(
                Sink::ReturnBytes {
                    format: PhotoFormat::Jpeg,
                },
                Transform::None,
            ),
            &mut WhereverTheCallerSaid,
            &SteppedClock::new(0),
            Stamp::epoch(),
        )
        .outcome
        .expect("a photo");
        let bytes = taken.returned.clone().expect("a ReturnBytes sink");
        assert!(bytes.len() > 100, "the fixture has to be worth hiding");

        let rendered = format!("{taken:?}");
        assert!(
            rendered.contains(&format!("<{} bytes>", bytes.len())),
            "{rendered}"
        );
        // A JPEG opens `255, 216, 255` in a derived `Debug`'s decimal rendering. Naming
        // the actual first bytes rather than a constant, so this notices whatever the
        // fixture happens to hold.
        let leak = bytes
            .iter()
            .take(3)
            .map(u8::to_string)
            .collect::<Vec<_>>()
            .join(", ");
        assert!(!rendered.contains(&leak), "frame bytes leaked: {rendered}");

        // And the other variant: a photo written to a file has no bytes to hide, and must
        // still render something a reader can tell apart from "an empty payload".
        let to_a_file = Photograph {
            report: taken.report.clone(),
            returned: None,
        };
        assert!(format!("{to_a_file:?}").contains("None"));

        // The wrapper this function now answers with is a fifth subject of the same rule, and
        // it is the one a `tracing::debug!(?taken)` would reach first — it derives `Debug`,
        // so the promise holds only because what it derives *through* keeps it.
        let wrapped = Taken {
            outcome: Ok(taken),
            gap: None,
        };
        let rendered = format!("{wrapped:?}");
        assert!(
            rendered.contains(&format!("<{} bytes>", bytes.len())),
            "{rendered}"
        );
        assert!(!rendered.contains(&leak), "frame bytes leaked: {rendered}");
    }

    #[test]
    fn a_photo_taken_while_the_camera_is_previewing_suspends_the_stream_and_stays_verbatim() {
        // The owner's ruling (2026-08-12) at the layer that owns the sequence. The preview is
        // a stream this test starts and does not stop, which is exactly the state an actor's
        // device is in between two `engine::preview::turn` commands — and before the ruling
        // this photo was `Error::Busy` from the device.
        //
        // Three claims, and the third is the one that could quietly be lost: the photo
        // happens, the preview is streaming again afterwards, and the bytes are still the
        // camera's own. A suspend/resume that re-negotiated into a format needing a re-encode
        // would pass the first two and cost the product what AGENTS calls it ("verbatim
        // camera JPEG when the sink allows — byte fidelity is the product").
        let (backend, mut camera) = watched_camera_from("chicony-rgb");
        let previewing = camera
            .start_stream(&crate::preview::request())
            .expect("this camera has an MJPEG mode");
        assert_eq!(backend.streams_started(), 1);
        // Three frames off it, so the sequence numbers a viewer has seen are not zero. What
        // this buys is asserted at the end: `STREAMON` resets the driver's own counter, so
        // the frame after the photo starts over — which is the one thing a client can read
        // the pause off, and is why `daemon::preview`'s header tells it to.
        let before = (0..3)
            .map(|_| {
                camera
                    .next_frame(std::time::Instant::now() + std::time::Duration::from_secs(1))
                    .expect("the preview is running")
                    .sequence
            })
            .collect::<Vec<u32>>();
        assert_eq!(before, vec![0, 1, 2]);

        let taken = take(
            camera.as_mut(),
            &request(
                Sink::ReturnBytes {
                    format: PhotoFormat::Jpeg,
                },
                Transform::None,
            ),
            &mut WhereverTheCallerSaid,
            &SteppedClock::new(0),
            Stamp::epoch(),
        );

        let gap = taken.gap.expect("a stream was running, so there is a gap");
        assert_eq!(gap.stream, previewing, "the gap names a stream nobody had");
        gap.resumed.expect("the preview came back");
        let report = taken.outcome.expect("a photo during a preview").report;
        assert!(report.rendering.is_verbatim(), "{:?}", report.rendering);

        // Three streams for one photo during one preview: the preview's, the capture's own,
        // and the resume. A build that skipped the suspend would have failed above with
        // `Busy`; one that skipped the resume answers two here.
        assert_eq!(backend.streams_started(), 3);
        assert!(
            camera.streaming().is_some(),
            "the photo left the preview stopped"
        );

        // What a viewer sees across the gap, made a fact rather than a paragraph: the resumed
        // stream numbers its frames from zero again, because `STREAMON` resets the driver's
        // counter. It is left as the device reports it — that field's whole contract is that
        // it is the *device's* number (D5) — and it is the only signal on the wire that says
        // "this stream restarted" rather than "frames were dropped", which move it forward.
        assert_eq!(
            camera
                .next_frame(std::time::Instant::now() + std::time::Duration::from_secs(1))
                .expect("the resumed stream delivers")
                .sequence,
            0,
            "the resumed stream continued the suspended one's numbering"
        );
    }

    #[test]
    fn a_photo_with_no_preview_running_starts_one_stream_and_reports_no_gap() {
        // The other direction of the test above, and the one that says the mechanism is
        // conditional rather than always-on: with nothing streaming, this is the photo path
        // exactly as it was before the ruling — one `STREAMON`, one `STREAMOFF`, no gap.
        let (backend, mut camera) = watched_camera_from("chicony-rgb");
        let taken = take(
            camera.as_mut(),
            &request(
                Sink::ReturnBytes {
                    format: PhotoFormat::Jpeg,
                },
                Transform::None,
            ),
            &mut WhereverTheCallerSaid,
            &SteppedClock::new(0),
            Stamp::epoch(),
        );

        assert!(
            taken.gap.is_none(),
            "a photo with nobody watching reported an interruption"
        );
        taken.outcome.expect("a photo");
        assert_eq!(
            backend.streams_started(),
            1,
            "the photo's own, and no other"
        );
        assert!(
            camera.streaming().is_none(),
            "the photo left the camera streaming"
        );
    }

    #[test]
    fn a_photo_whose_settle_budget_outlasts_the_bound_is_refused_and_the_preview_keeps_running() {
        // `limits::PREVIEW_SUSPEND_MAX_MS` reaching the verb a client actually calls: the
        // budget the bound is checked against is the settle deadline the *request* carries,
        // so this is the one place the two meet. The refusal arrives before the stream is
        // touched, which is what the counter says.
        let (backend, mut camera) = watched_camera_from("chicony-rgb");
        camera
            .start_stream(&crate::preview::request())
            .expect("this camera has an MJPEG mode");

        let mut asking = request(
            Sink::ReturnBytes {
                format: PhotoFormat::Jpeg,
            },
            Transform::None,
        );
        asking.settle.deadline_ms = schema::limits::PREVIEW_SUSPEND_MAX_MS + 1;
        let taken = take(
            camera.as_mut(),
            &asking,
            &mut WhereverTheCallerSaid,
            &SteppedClock::new(0),
            Stamp::epoch(),
        );

        assert_eq!(
            taken
                .outcome
                .expect_err("the budget is longer than a photo may hold a preview down")
                .kind(),
            ErrorKind::IllegalTransition
        );
        assert!(taken.gap.is_none());
        assert_eq!(backend.streams_started(), 1, "the preview was interrupted");
        assert!(camera.streaming().is_some());

        // And the same request inside the bound is served, so the refusal is about the number
        // rather than about a camera that refuses photos while streaming.
        asking.settle.deadline_ms = schema::limits::PREVIEW_SUSPEND_MAX_MS;
        take(
            camera.as_mut(),
            &asking,
            &mut WhereverTheCallerSaid,
            &SteppedClock::new(0),
            Stamp::epoch(),
        )
        .outcome
        .expect("a budget at the bound is within it");
        assert_eq!(backend.streams_started(), 3);
    }

    #[test]
    fn a_sink_naming_an_encoding_this_build_cannot_write_is_refused_before_a_stream_starts() {
        // Debt D-1, from the engine's side. The rule and its two directions live on
        // `schema::capture::Sink::writable_format`, which is where both surfaces call it;
        // what is asserted here is that this pipeline *asks*, and *when* — the refusal has
        // to reach a caller who came in through the engine rather than through a command
        // line, and it has to arrive before the camera is streamed.
        //
        // `streams_started()` is what makes the name of this test checkable. `!path.exists()`
        // alone is true of a build that opened the device, negotiated a format, spent the
        // whole settle budget and dequeued a frame of whoever was in front of the lens
        // before refusing — which is what this pipeline did until the check was hoisted to
        // `take`'s first statement.
        let dir = crate::paths::scratch_dir().expect("a scratch directory");
        let path = camino::Utf8PathBuf::from_path_buf(dir.path().join("shot.webp"))
            .expect("utf-8 temp dir");
        let (backend, mut camera) = watched_camera_from("chicony-rgb");
        assert_eq!(backend.streams_started(), 0, "opening is not streaming");
        let error = take(
            camera.as_mut(),
            &request(Sink::ServerPath { path: path.clone() }, Transform::None),
            &mut WhereverTheCallerSaid,
            &SteppedClock::new(0),
            Stamp::epoch(),
        )
        .outcome
        .expect_err("webp is not one of the three this build writes");

        assert_eq!(error.kind(), ErrorKind::IllegalTransition);
        assert!(!path.exists(), "a refused photo was written anyway");
        assert_eq!(
            backend.streams_started(),
            0,
            "the refusal arrived after a frame had been captured"
        );

        // The twin, over the same directory: the extensions this build does write still
        // land, so the arm above refuses `.webp` rather than refusing paths.
        for format in PhotoFormat::ALL {
            let written = camino::Utf8PathBuf::from_path_buf(
                dir.path().join(format!("shot.{}", format.extension())),
            )
            .expect("utf-8 temp dir");
            take(
                camera.as_mut(),
                &request(
                    Sink::ServerPath {
                        path: written.clone(),
                    },
                    Transform::None,
                ),
                &mut WhereverTheCallerSaid,
                &SteppedClock::new(0),
                Stamp::epoch(),
            )
            .outcome
            .unwrap_or_else(|err| panic!("{written}: {err}"));
            assert!(written.exists(), "{written}");
        }
        // And the counter moves for the requests this build does honour, so the zero above
        // is a zero this fake would otherwise have left behind.
        assert_eq!(
            backend.streams_started(),
            u64::try_from(PhotoFormat::ALL.len()).expect("three fits"),
            "one stream per honoured request"
        );
    }
}
