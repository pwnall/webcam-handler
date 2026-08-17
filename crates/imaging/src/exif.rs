//! Capture metadata stamped onto JPEG bytes (design D6).
//!
//! A calibration sample photo is self-describing without its session file: who took it,
//! when, in what format, at what size, and with which control values in effect. That is
//! the whole requirement, and it is why this module exists rather than a sidecar `.json`
//! — a photo an agent hands to a human keeps its provenance across a copy, a rename and
//! a chat attachment.
//!
//! ## Stamping must not touch the image
//!
//! EXIF lives in an APP1 segment before the entropy-coded scan. Writing one rewrites the
//! file's *header*, never its pixels — which is what makes stamping compatible with E6:
//! a verbatim camera JPEG stays the camera's bitstream after it is stamped. The test
//! `the_entropy_coded_scan_survives_byte_identical` is the assertion that keeps that
//! true, because "the writer says it only touched the header" is not evidence.
//!
//! ## Read back, never assumed
//!
//! docs/9 names write-only EXIF as a defect class: a writer that returns `Ok` having
//! produced a segment no reader accepts passes every test that trusts it. Every field
//! this module writes is read back in tests with `kamadak-exif`, an independent
//! implementation that shares no code with the writer.

use std::collections::BTreeMap;

use little_exif::exif_tag::ExifTag;
use little_exif::filetype::FileExtension;
use little_exif::metadata::Metadata;
use schema::TOOL_VERSION;
use schema::camera::CameraFingerprint;
use schema::capture::{NegotiatedStream, Transform};
use schema::control::{ControlSlug, ControlValue};
use schema::error::Result;
use schema::limits;
use schema::time::Stamp;

use crate::fault::imaging_failure;

const OP: &str = "stamp EXIF onto JPEG";

/// What a shortened description says about itself, and where the count goes.
///
/// **A decline is data first and a line second** (note N121's rule, one surface along): a
/// reader holding a photo whose control list stops early must be able to tell that from a
/// camera that reported nothing more. So the sentence names both numbers — what was written
/// and what there was — rather than trailing off in an ellipsis a reader has to interpret.
const ELIDED: &str = "(truncated here:";

/// The EXIF character-code prefix that declares a `UserComment` to be plain ASCII.
///
/// The tag is `UNDEF`, and the first eight bytes are the encoding declaration — a reader
/// handed the text without it sees an eight-byte-shifted string.
const USER_COMMENT_ASCII: [u8; 8] = *b"ASCII\0\0\0";

/// Everything a photo carries about its own capture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureMetadata {
    /// When the frame was taken.
    pub captured_at: Stamp,
    /// Which camera took it — the fingerprint, so a photo can be matched back to a
    /// session recorded against the same device.
    pub camera: CameraFingerprint,
    /// What the device agreed to deliver (D5). Recorded rather than what was requested,
    /// because the two differ and only one of them describes the pixels.
    pub negotiated: NegotiatedStream,
    /// The orientation the photo was taken with; becomes the Orientation tag.
    pub transform: Transform,
    /// The control values in effect at capture time.
    pub controls: BTreeMap<ControlSlug, ControlValue>,
}

/// Stamp capture metadata onto a JPEG, returning the new bytes.
///
/// Any EXIF the camera already wrote is replaced: two EXIF segments in one file is
/// undefined, and the camera's is describing the same capture less accurately than we
/// can.
///
/// # Errors
///
/// [`schema::Error::DeviceIo`] naming the step when the input is not a
/// JPEG, or when the EXIF writer refuses.
pub fn stamp_jpeg(jpeg: &[u8], metadata: &CaptureMetadata) -> Result<Vec<u8>> {
    if !jpeg.starts_with(&[0xff, 0xd8]) {
        return Err(imaging_failure(
            OP,
            format!(
                "buffer of {} bytes does not begin with the JPEG SOI marker",
                jpeg.len()
            ),
        ));
    }

    let captured_at = exif_datetime(metadata.captured_at)?;
    let mut tags = Metadata::new();
    tags.set_tag(ExifTag::Orientation(vec![
        metadata.transform.exif_orientation(),
    ]));
    // The kernel gives us a driver, not a manufacturer. `Make` is the closest honest
    // home for it, and the USB vendor:product pair — the field a reader actually wants
    // when identifying hardware — is in the description alongside the rest of the
    // fingerprint.
    tags.set_tag(ExifTag::Make(metadata.camera.driver.clone()));
    tags.set_tag(ExifTag::Model(metadata.camera.card.clone()));
    tags.set_tag(ExifTag::Software(format!("webcam-handler {TOOL_VERSION}")));
    tags.set_tag(ExifTag::ImageDescription(describe_capture(metadata)));
    tags.set_tag(ExifTag::UserComment(describe_controls(metadata)));
    tags.set_tag(ExifTag::DateTimeOriginal(captured_at.clone()));
    tags.set_tag(ExifTag::CreateDate(captured_at.clone()));
    tags.set_tag(ExifTag::ModifyDate(captured_at));

    // The APP1 segment, built without the writer ever seeing our file — see
    // [`splice_app1`] for why that separation is load-bearing.
    let app1 = tags
        .as_u8_vec(FileExtension::JPEG)
        .map_err(|err| imaging_failure(OP, err.to_string()))?;
    check_segment_length(&app1)?;
    splice_app1(jpeg, &app1)
}

/// Refuse a segment whose declared length is not the length it has.
///
/// **The dependency's arithmetic, read back rather than trusted** — the posture this
/// module's header already takes towards every field it writes, applied to the one number
/// `little_exif` computes for us. `encode_metadata_jpg` builds the length as
/// `2 + EXIF_HEADER.len() + exif_vec.len() as u16`, and that cast **truncates**: a payload
/// past [`limits::MAX_EXIF_APP1_BYTES`] produces a segment declaring a length modulo 65 536,
/// which every reader then uses to find the next marker. The file that results is not a JPEG
/// with bad metadata; it is a JPEG whose header walk lands in the middle of an EXIF payload
/// and reads it as structure (the G6 review's L5; note **N203**).
///
/// The two free-text tags are already bounded by [`limits::MAX_EXIF_TEXT_BYTES`], so this
/// cannot fire on anything a device can say through the fields above. It exists because that
/// bound is a *fact about two tags* and this is a *property of the segment*, and a ninth tag
/// added later would be covered by the second and not the first. A refusal here is therefore
/// a defect in this module rather than a fact about the camera, which is why it is
/// `DeviceIo` naming both numbers rather than a shortened description: there is nothing
/// left to shorten by the time the writer has run.
///
/// # Errors
///
/// [`schema::Error::DeviceIo`] naming the declared length and the real one.
fn check_segment_length(app1: &[u8]) -> Result<()> {
    // `FF E1` then the big-endian length, which counts its own two bytes and everything
    // after them.
    let declared = match (app1.get(2), app1.get(3)) {
        (Some(high), Some(low)) => usize::from(u16::from_be_bytes([*high, *low])),
        _ => {
            return Err(imaging_failure(
                OP,
                format!(
                    "the EXIF writer produced {} bytes, which is not even a segment header",
                    app1.len()
                ),
            ));
        }
    };
    // The marker is the two bytes the length does not cover.
    let actual = app1.len().saturating_sub(2);
    if declared != actual {
        return Err(imaging_failure(
            OP,
            format!(
                "the EXIF segment declares {declared} bytes and carries {actual}; a JPEG \
                 segment length is 16 bits and the largest payload one can name is \
                 {} bytes",
                limits::MAX_EXIF_APP1_BYTES
            ),
        ));
    }
    Ok(())
}

/// `text`, shortened to `budget` bytes with a sentence saying it was.
///
/// **The photograph is the product and its description is not**, so a device with more to
/// say than an APP1 segment holds gets a shorter description rather than a refused photo —
/// and the description says so, with both numbers, so a reader can tell a truncated list
/// from a camera that reported nothing more (AGENTS rule 6: represent what happened, never
/// silently correct it).
///
/// The cut lands on a `char` boundary because a `String` sliced anywhere else is not one, and
/// it lands on the last `; ` before the boundary when there is one — both renderings above
/// are semicolon-delimited `key=value`, and a list ending mid-key reads as a control whose
/// name this build got wrong.
fn fit(text: String, budget: usize) -> String {
    if text.len() <= budget {
        return text;
    }
    // The note is sized against the budget and then written against what was actually kept.
    // Those are two different numbers and the first attempt printed the first one twice: the
    // budget is what there was *room* for, and the text that survives is shorter than that by
    // the note itself and by however far back the last `; ` sits. A reader told "200 of 522
    // bytes" over a description holding rather less than 200 has been given a number that
    // describes nothing it can see. Sizing against the budget and printing against the result
    // is safe in the one direction that matters: `kept <= budget`, so the printed note is never
    // longer than the one the room was reserved for, and the total stays inside the bound.
    let note_for = |kept: usize| format!(" {ELIDED} {kept} of {} bytes)", text.len());
    // The note has to fit inside the budget too, or shortening would be how the bound is
    // exceeded. A budget below the note is not one this build sets — `MAX_EXIF_TEXT_BYTES` is
    // sixteen kibibytes — and the honest answer for one that was is the note alone, which is
    // why this arm is the one place `fit` answers longer than it was asked for.
    let Some(room) = budget.checked_sub(note_for(budget).len()) else {
        return note_for(0);
    };
    let mut cut = room;
    while cut > 0 && !text.is_char_boundary(cut) {
        cut -= 1;
    }
    let head = text.get(..cut).unwrap_or_default();
    let tidy = head.rfind("; ").map_or(head, |at| &head[..at]);
    format!("{tidy}{}", note_for(tidy.len()))
}

/// Put `app1` into `jpeg` immediately after the SOI, dropping any EXIF already there.
///
/// ## Why this is ours and not the writer's
///
/// `little_exif 0.6.23`'s own JPEG path walks the **whole file** byte by byte looking for
/// `0xFF <marker>` pairs — including inside the entropy-coded scan. That is not a place
/// markers live: a literal `0xFF` in compressed data is byte-stuffed as `FF 00`, and a
/// file using restart intervals contains `FF D0`–`FF D7` throughout its scan. The walker
/// reads the two bytes after each as a segment length, gets a number out of the image
/// data, and seeks past the end — `failed to fill whole buffer`.
///
/// Measured on the Chicony, which sets a restart interval (`FFDD`): **nine of forty**
/// frames failed, varying with the scene, because whether a garbage length happens to land
/// inside the buffer depends on what the sensor was looking at. A photo verb that works
/// two times in three is worse than one that never works, because the failures look like
/// the camera's fault. Recorded as PF:16.
///
/// ## Why the fix is small
///
/// Every JPEG segment *before* the start-of-scan is a well-formed `FF <marker> <u16 len>`,
/// and `SOS` is where that stops being true. So the walk stops at `SOS`, which is also the
/// only region an APP1 can legally occupy — and the entropy-coded data, the part this
/// function must not misread, is copied verbatim and never parsed. That is E6's promise
/// restated as an implementation: stamping rewrites the header and cannot touch a pixel.
fn splice_app1(jpeg: &[u8], app1: &[u8]) -> Result<Vec<u8>> {
    let mut out = Vec::with_capacity(jpeg.len() + app1.len());
    out.extend_from_slice(jpeg.get(..2).unwrap_or_default());
    out.extend_from_slice(app1);

    // Walk the header segments, copying everything except an existing EXIF APP1. Two
    // EXIF segments in one file is undefined, and the camera's describes the same
    // capture less accurately than ours.
    let mut at = 2usize;
    while let Some(segment) = next_segment(jpeg, at) {
        match segment {
            Segment::Standalone { end } => {
                out.extend_from_slice(jpeg.get(at..end).unwrap_or_default());
                at = end;
            }
            Segment::Sized { marker, body, end } => {
                let is_exif = marker == APP1
                    && jpeg
                        .get(body..body.saturating_add(EXIF_SIGNATURE.len()))
                        .is_some_and(|prefix| prefix == EXIF_SIGNATURE);
                if !is_exif {
                    out.extend_from_slice(jpeg.get(at..end).unwrap_or_default());
                }
                at = end;
            }
            // The scan, and everything after it — copied without a single byte being
            // interpreted, which is the whole point.
            Segment::ScanBegins => break,
        }
    }
    out.extend_from_slice(jpeg.get(at..).unwrap_or_default());
    Ok(out)
}

/// `APP1`, the marker EXIF lives in.
const APP1: u8 = 0xe1;
/// `SOS` — after this, the bytes are compressed image data and not markers.
const SOS: u8 = 0xda;
/// The six bytes that make an APP1 segment an *EXIF* one rather than, say, XMP.
const EXIF_SIGNATURE: &[u8] = b"Exif\0\0";

/// One step of the header walk.
enum Segment {
    /// A marker with no length field (`RSTn`, `TEM`).
    Standalone { end: usize },
    /// A marker with a `u16` length.
    Sized {
        marker: u8,
        /// Where the segment's payload starts, after the marker and the length.
        body: usize,
        /// Where the next segment starts.
        end: usize,
    },
    /// The start of scan. The walk stops here.
    ScanBegins,
}

/// The segment at `at`, or `None` when the buffer does not hold one.
///
/// Total by construction: every arithmetic step is checked and every slice is `get`. The
/// input is a camera's bitstream, which is device data and therefore attacker-shaped
/// (rubric B10) — a segment claiming a length past the end of the buffer must end the
/// walk, not index past it.
fn next_segment(jpeg: &[u8], at: usize) -> Option<Segment> {
    if *jpeg.get(at)? != 0xff {
        return None;
    }
    let marker = *jpeg.get(at.checked_add(1)?)?;
    match marker {
        SOS => Some(Segment::ScanBegins),
        // `RSTn` and `TEM` carry no length. They do not appear in a header, but a walk
        // that assumed so would misread one rather than stopping.
        0xd0..=0xd9 | 0x01 => Some(Segment::Standalone {
            end: at.checked_add(2)?,
        }),
        _ => {
            let high = u16::from(*jpeg.get(at.checked_add(2)?)?);
            let low = u16::from(*jpeg.get(at.checked_add(3)?)?);
            let length = usize::from((high << 8) | low);
            // A length below 2 does not include its own field, which is a segment that
            // cannot exist; both that and a length past the end end the walk.
            let end = at.checked_add(2)?.checked_add(length.max(2))?;
            if length < 2 || end > jpeg.len() {
                return None;
            }
            Some(Segment::Sized {
                marker,
                body: at.checked_add(4)?,
                end,
            })
        }
    }
}

/// The camera identity and negotiated format, as one line.
///
/// Semicolon-delimited `key=value`, because `ImageDescription` is a free-text field and
/// something has to impose a shape on it. Absent optional fields are omitted rather than
/// written empty — PF:8 says a missing serial is the common case, and `serial=` reads as
/// a serial of the empty string.
///
/// Bounded by [`limits::MAX_EXIF_TEXT_BYTES`] like its sibling, and for the sibling's
/// reason even though every part of it comes out of a fixed-width kernel field today: this
/// tag and [`describe_controls`] are the segment's two free-text halves, and a bound that
/// covered one of them would be a bound the arithmetic in `limits` could not state.
#[must_use]
pub fn describe_capture(metadata: &CaptureMetadata) -> String {
    let camera = &metadata.camera;
    let stream = &metadata.negotiated;
    let mut parts = vec![
        "webcam-handler capture".to_owned(),
        format!("card={}", camera.card),
        format!("driver={}", camera.driver),
        format!("bus={}", camera.bus_path),
    ];
    if let Some(usb) = camera.usb_id {
        parts.push(format!("usb={usb}"));
    }
    if let Some(serial) = &camera.serial {
        parts.push(format!("serial={serial}"));
    }
    parts.push(format!("format={}", stream.pixel_format));
    parts.push(format!("size={}x{}", stream.width, stream.height));
    if let Some(fps) = stream.interval.fps() {
        parts.push(format!("fps={fps}"));
    }
    fit(parts.join("; "), limits::MAX_EXIF_TEXT_BYTES)
}

/// The control values in effect, as an EXIF `UserComment` payload.
///
/// Returns the eight-byte ASCII character-code prefix followed by the text, which is
/// what the tag's `UNDEF` type means.
///
/// **This is the unbounded half.** Every control the device reported is rendered, and
/// neither the number of controls nor the length of a `V4L2_CTRL_TYPE_STRING` value is
/// anything this side of the cable decides — `vivid` enumerates 77 and one of them is a
/// string. So the text is fitted to [`limits::MAX_EXIF_TEXT_BYTES`] and says so when it did
/// not all fit; see `fit` for why shortening rather than refusing is the answer.
#[must_use]
pub fn describe_controls(metadata: &CaptureMetadata) -> Vec<u8> {
    let body = if metadata.controls.is_empty() {
        "controls: (none recorded)".to_owned()
    } else {
        let rendered: Vec<String> = metadata
            .controls
            .iter()
            .map(|(slug, value)| format!("{slug}={value}"))
            .collect();
        format!("controls: {}", rendered.join("; "))
    };
    let mut out = USER_COMMENT_ASCII.to_vec();
    out.extend_from_slice(fit(body, limits::MAX_EXIF_TEXT_BYTES).as_bytes());
    out
}

/// Render an instant the way EXIF spells one: `YYYY:MM:DD HH:MM:SS`.
///
/// Derived from the RFC 3339 string `Stamp` is defined to produce, by reshaping the
/// separators. Deliberately *not* by doing calendar arithmetic: the timestamp library
/// has one home (`webcam-handler-schema::time`) and a second copy of civil-date maths
/// here is a second copy of a law, with the added charm of being wrong about leap years
/// on some future Tuesday.
///
/// # Errors
///
/// [`schema::Error::DeviceIo`] if the rendering is not RFC 3339 — which
/// means the schema's contract broke, and silently omitting the capture time from a
/// calibration sample is the worse answer.
fn exif_datetime(stamp: Stamp) -> Result<String> {
    let rendered = stamp.to_string();
    let Some((date, rest)) = rendered.split_once('T') else {
        return Err(imaging_failure(
            OP,
            format!("capture time {rendered:?} is not an RFC 3339 timestamp"),
        ));
    };
    // Fractional seconds and the zone suffix are not part of the EXIF spelling. The
    // separators listed here cover `Z`, `+hh:mm` and `-hh:mm`.
    let time = rest.split(['.', 'Z', '+', '-']).next().unwrap_or(rest);
    let plausible = date.len() == 10
        && time.len() == 8
        && date.chars().filter(|c| *c == '-').count() == 2
        && time.chars().filter(|c| *c == ':').count() == 2;
    if !plausible {
        return Err(imaging_failure(
            OP,
            format!("capture time {rendered:?} is not an RFC 3339 timestamp"),
        ));
    }
    Ok(format!("{} {}", date.replace('-', ":"), time))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode::{self, Decoded};
    use crate::encode;
    use crate::fixtures;
    use exif::{In, Tag, Value};
    use schema::camera::{FrameInterval, PixelFormat, UsbId};

    fn fingerprint() -> CameraFingerprint {
        CameraFingerprint {
            bus_path: "3-4:1.0".to_owned(),
            usb_id: Some(UsbId {
                vendor: 0x04f2,
                product: 0xb83c,
            }),
            card: "Integrated Camera: Integrated C".to_owned(),
            driver: "uvcvideo".to_owned(),
            serial: Some("0001".to_owned()),
        }
    }

    fn negotiated() -> NegotiatedStream {
        NegotiatedStream {
            pixel_format: PixelFormat::MJPG,
            width: 1280,
            height: 720,
            bytes_per_line: 0,
            size_image: 1 << 19,
            interval: FrameInterval::Discrete {
                numerator: 1,
                denominator: 30,
            },
            adjustments: Vec::new(),
        }
    }

    fn metadata(transform: Transform) -> CaptureMetadata {
        CaptureMetadata {
            captured_at: Stamp::epoch(),
            camera: fingerprint(),
            negotiated: negotiated(),
            transform,
            controls: BTreeMap::from([
                (
                    ControlSlug::parse("brightness").expect("literal slug"),
                    ControlValue::Int(128),
                ),
                (
                    ControlSlug::parse("focus_absolute").expect("literal slug"),
                    ControlValue::Int(45),
                ),
            ]),
        }
    }

    fn sample_jpeg() -> Vec<u8> {
        encode::jpeg(&Decoded::Gray(fixtures::text_like(64, 48)), 88).expect("encode fixture")
    }

    fn read_back(bytes: &[u8]) -> exif::Exif {
        exif::Reader::new()
            .read_from_container(&mut std::io::Cursor::new(bytes))
            .expect("the stamped file must be readable by an independent EXIF reader")
    }

    fn ascii(data: &exif::Exif, tag: Tag) -> Option<String> {
        match &data.get_field(tag, In::PRIMARY)?.value {
            Value::Ascii(entries) => entries
                .first()
                .map(|bytes| String::from_utf8_lossy(bytes).into_owned()),
            _ => None,
        }
    }

    /// The offset of the SOS marker, walking the JPEG's segment headers.
    ///
    /// Everything from here to the end of the file is the entropy-coded scan plus the
    /// EOI — the part stamping must not touch.
    fn scan_offset(jpeg: &[u8]) -> Option<usize> {
        let mut cursor = 2usize;
        loop {
            if jpeg.get(cursor)? != &0xff {
                return None;
            }
            let marker = *jpeg.get(cursor + 1)?;
            if marker == 0xda {
                return Some(cursor);
            }
            // Restart markers, TEM and a second SOI carry no length field.
            if (0xd0..=0xd9).contains(&marker) || marker == 0x01 {
                cursor += 2;
                continue;
            }
            let length = usize::from(u16::from_be_bytes([
                *jpeg.get(cursor + 2)?,
                *jpeg.get(cursor + 3)?,
            ]));
            cursor = cursor.checked_add(2)?.checked_add(length)?;
        }
    }

    #[test]
    fn every_stamped_field_reads_back_through_an_independent_reader() {
        // docs/9's "EXIF read-back" gate as a test: the writer returning `Ok` proves
        // nothing about whether any reader can find what it claims to have written.
        let stamped = stamp_jpeg(&sample_jpeg(), &metadata(Transform::Rot90)).expect("stamp");
        let data = read_back(&stamped);

        assert_eq!(
            data.get_field(Tag::Orientation, In::PRIMARY)
                .and_then(|field| field.value.get_uint(0)),
            Some(6),
            "the Orientation tag did not survive"
        );
        assert_eq!(ascii(&data, Tag::Make).as_deref(), Some("uvcvideo"));
        assert_eq!(
            ascii(&data, Tag::Model).as_deref(),
            Some("Integrated Camera: Integrated C")
        );
        assert_eq!(
            ascii(&data, Tag::DateTimeOriginal).as_deref(),
            Some("1970:01:01 00:00:00")
        );
        assert_eq!(
            ascii(&data, Tag::DateTimeDigitized).as_deref(),
            Some("1970:01:01 00:00:00")
        );
        let software = ascii(&data, Tag::Software).unwrap_or_default();
        assert!(software.starts_with("webcam-handler "), "{software}");

        let description = ascii(&data, Tag::ImageDescription).unwrap_or_default();
        for expected in [
            "card=Integrated Camera: Integrated C",
            "driver=uvcvideo",
            "bus=3-4:1.0",
            "usb=04f2:b83c",
            "serial=0001",
            "format=MJPG",
            "size=1280x720",
            "fps=30",
        ] {
            assert!(
                description.contains(expected),
                "{expected:?} missing from {description:?}"
            );
        }

        let comment = match &data
            .get_field(Tag::UserComment, In::PRIMARY)
            .expect("UserComment")
            .value
        {
            Value::Undefined(bytes, _) => bytes.clone(),
            other => panic!("UserComment is {other:?}"),
        };
        assert!(
            comment.starts_with(&USER_COMMENT_ASCII),
            "no character code"
        );
        let text = String::from_utf8_lossy(&comment);
        assert!(text.contains("brightness=128"), "{text}");
        assert!(text.contains("focus_absolute=45"), "{text}");
    }

    /// A control whose value is `bytes` characters of device-supplied text.
    ///
    /// `V4L2_CTRL_TYPE_STRING` is the shape: its length is the *device's* to choose, and
    /// nothing between the driver and this module bounds it. `vivid` enumerates 77 controls
    /// and one of them is a string, which is the combination that reaches the ceiling
    /// fastest.
    fn a_talkative_control(slug: &str, bytes: usize) -> (ControlSlug, ControlValue) {
        (
            ControlSlug::parse(slug).expect("literal slug"),
            ControlValue::Text("v".repeat(bytes)),
        )
    }

    #[test]
    fn a_device_with_more_to_say_than_the_segment_holds_is_described_shorter_not_corrupted() {
        // **An APP1 segment's length is a `u16`, and `little_exif` computes it with
        // `exif_vec.len() as u16`** — a truncation, not a refusal. So a device verbose enough
        // to push the payload past 65 535 bytes produced a segment declaring a length modulo
        // 65 536, a header walk that landed in the middle of the EXIF and read its bytes as
        // markers, and a `stamp_jpeg` that returned `Ok` over it. The photograph is the
        // product and its metadata is not, so the answer is a *shorter description* naming
        // its own omission — never a refused photo and never a corrupt one.
        //
        // The bound is `schema::limits::MAX_EXIF_TEXT_BYTES` and the ceiling it is priced
        // against is `schema::limits::MAX_EXIF_APP1_BYTES`; both are read here and the
        // read-back below is what proves the arithmetic rather than restating it.
        let controls: BTreeMap<ControlSlug, ControlValue> = (0..12)
            .map(|index| a_talkative_control(&format!("chatty_{index}"), 8_192))
            .collect();
        let verbose = CaptureMetadata {
            controls,
            ..metadata(Transform::None)
        };

        let stamped =
            stamp_jpeg(&sample_jpeg(), &verbose).expect("a talkative device is stampable");
        // The independent reader is the assertion: a truncated length field puts it in the
        // middle of the EXIF payload, where it finds no tags this module wrote.
        let data = read_back(&stamped);
        assert_eq!(ascii(&data, Tag::Make).as_deref(), Some("uvcvideo"));
        let comment = match &data
            .get_field(Tag::UserComment, In::PRIMARY)
            .expect("UserComment survived")
            .value
        {
            Value::Undefined(bytes, _) => bytes.clone(),
            other => panic!("UserComment is {other:?}"),
        };
        assert!(
            comment.len() <= USER_COMMENT_ASCII.len() + limits::MAX_EXIF_TEXT_BYTES,
            "the comment is {} bytes and the bound is {}",
            comment.len(),
            limits::MAX_EXIF_TEXT_BYTES
        );
        let text = String::from_utf8_lossy(&comment);
        assert!(
            text.contains(ELIDED),
            "a shortened description must say so: {:?}",
            &text[..text.len().min(200)]
        );
        // What did fit is still the device's own text rather than a placeholder.
        assert!(
            text.contains("chatty_0=vvv"),
            "the first control is missing"
        );

        // And the segment the file carries declares its own true length — the property the
        // dependency's `as u16` cannot be trusted with.
        assert_eq!(
            stamped.get(..4),
            Some([0xff, 0xd8, 0xff, 0xe1].as_slice()),
            "the APP1 goes immediately after the SOI"
        );
        let declared = usize::from(u16::from_be_bytes([
            *stamped.get(4).expect("length high byte"),
            *stamped.get(5).expect("length low byte"),
        ]));
        assert!(
            declared <= limits::MAX_EXIF_APP1_BYTES,
            "the segment declares {declared} bytes, past what a u16 length field can mean"
        );
        // The scan is where the walk says it is, which a truncated length would move.
        let original = sample_jpeg();
        let offset = scan_offset(&original).expect("the fixture has a scan");
        assert!(
            stamped.ends_with(original.get(offset..).expect("in range")),
            "the entropy-coded scan moved under a long description"
        );
        assert!(
            scan_offset(&stamped).is_some(),
            "the stamped file's header walk no longer reaches SOS"
        );
    }

    #[test]
    fn a_segment_whose_declared_length_is_not_its_own_is_refused_rather_than_spliced() {
        // The backstop, in both directions, driven directly — the bound above means no
        // metadata this build can assemble reaches it, and a check nothing can turn red is
        // the shape rubric rule 2 refuses. Two hand-built segments, one honest and one with
        // `little_exif`'s truncation applied by hand.
        let mut honest = vec![0xff, 0xe1, 0x00, 0x00];
        honest.extend_from_slice(EXIF_SIGNATURE);
        honest.extend(std::iter::repeat_n(0u8, 40));
        let length = u16::try_from(honest.len() - 2).expect("small");
        honest[2..4].copy_from_slice(&length.to_be_bytes());
        assert!(
            check_segment_length(&honest).is_ok(),
            "a segment that declares its own length is fine"
        );

        // The same segment with the length wrapped, which is exactly what
        // `2 + EXIF_HEADER.len() + exif_vec.len() as u16` produces for a payload past the
        // ceiling: the bytes are all still there and the header says there are far fewer.
        let mut truncated = honest.clone();
        truncated[2..4].copy_from_slice(&4u16.to_be_bytes());
        let err =
            check_segment_length(&truncated).expect_err("a lying length must not reach a file");
        let rendered = err.to_string();
        assert!(rendered.contains("declares 4 bytes"), "{rendered}");
        assert!(
            rendered.contains(&limits::MAX_EXIF_APP1_BYTES.to_string()),
            "the refusal must name the ceiling it is about: {rendered}"
        );

        // And a buffer too short to hold a length field at all is refused rather than read.
        assert!(check_segment_length(&[0xff, 0xe1]).is_err());
    }

    #[test]
    fn a_description_shortened_to_its_budget_stops_on_a_whole_control_and_says_so() {
        // `fit` is where the bound becomes a *sentence*, and three properties make that
        // sentence worth anything: it never exceeds the budget, it never cuts a `key=value`
        // in half, and it names both numbers so a reader can tell a shortened list from a
        // quiet camera.
        let long = (0..40)
            .map(|index| format!("control_{index:02}=0123456789"))
            .collect::<Vec<_>>()
            .join("; ");
        let budget = 200;
        let fitted = fit(long.clone(), budget);
        assert!(
            fitted.len() <= budget,
            "{} bytes exceeds the {budget}-byte budget",
            fitted.len()
        );
        assert!(fitted.contains(ELIDED), "{fitted}");
        assert!(
            fitted.contains(&long.len().to_string()),
            "the note must name what there was: {fitted}"
        );
        let kept = fitted.split(ELIDED).next().unwrap_or_default().trim_end();
        assert!(
            kept.split("; ").all(|part| part.contains('=')),
            "a control was cut in half: {kept:?}"
        );
        assert!(kept.starts_with("control_00=0123456789"));

        // The first number is the one that was wrong, and asserting the bound could not see
        // it: the note used to print the *budget*, which is what there was room for, while
        // the text that survives is shorter by the note itself and by however far back the
        // last `; ` sits. Both numbers are now read back off the sentence and checked against
        // the two strings they describe, so a note that names a length nothing in the
        // description has goes red.
        let counted = fitted
            .split(ELIDED)
            .nth(1)
            .and_then(|tail| tail.trim().strip_suffix(" bytes)"))
            .and_then(|pair| pair.split_once(" of "))
            .map(|(kept, whole)| (kept.trim().to_owned(), whole.trim().to_owned()))
            .unwrap_or_default();
        assert_eq!(
            counted,
            (kept.len().to_string(), long.len().to_string()),
            "the note names two numbers and they are what was written and what there was: \
             {fitted}"
        );

        // Under the budget nothing is touched at all, which is the arm that keeps every
        // other test in this module reading the device's own words.
        assert_eq!(fit(long.clone(), long.len()), long);
        assert_eq!(fit(long.clone(), long.len() + 1), long);

        // A budget too small even for the note gets the note, because a shortening that
        // exceeded the bound would be no bound.
        let squeezed = fit(long, 4);
        assert!(squeezed.contains(ELIDED), "{squeezed}");
    }

    #[test]
    fn the_entropy_coded_scan_survives_byte_identical() {
        // Stamping rewrites the header. If it ever rewrote the scan, a verbatim camera
        // JPEG would stop being verbatim the moment it was described (E6).
        let original = sample_jpeg();
        let stamped = stamp_jpeg(&original, &metadata(Transform::None)).expect("stamp");
        let offset = scan_offset(&original).expect("the fixture has a scan");
        let scan = original.get(offset..).expect("offset is within the file");
        assert!(
            stamped.ends_with(scan),
            "the entropy-coded scan changed under stamping"
        );
        assert!(stamped.len() > original.len(), "nothing was added");

        // Said a second way, independently of the file layout: the pixels are the same.
        let before = decode::decode_jpeg(&original, 64, 48).expect("decode");
        let after = decode::decode_jpeg(&stamped, 64, 48).expect("decode");
        assert_eq!(before, after);
    }

    /// A JPEG whose entropy-coded scan contains byte sequences that *look* like markers.
    ///
    /// Hand-built, because our own encoder does not emit restart intervals and the frames
    /// that exposed the defect are camera frames, which never enter the repository (rubric
    /// A12). Every byte after `SOS` here is scan data by construction — including
    /// `FF D0` (a restart marker) and `FF 00 FF FF` (a stuffed literal followed by two
    /// bytes that read as a 65535-byte length).
    ///
    /// It is not a decodable image and does not need to be: this fixture's subject is the
    /// *parser*, and the parser's contract is that it never looks at these bytes.
    fn jpeg_with_marker_shaped_scan_bytes() -> Vec<u8> {
        let mut out = vec![0xff, 0xd8];
        // A `DRI` segment, which is what tells a reader restart markers are coming — the
        // Chicony sets one, which is why its frames contained them.
        out.extend_from_slice(&[0xff, 0xdd, 0x00, 0x04, 0x00, 0x10]);
        // A minimal `SOF0`, so the header is not merely a DRI.
        out.extend_from_slice(&[
            0xff, 0xc0, 0x00, 0x0b, 0x08, 0x00, 0x10, 0x00, 0x10, 0x01, 0x01, 0x11, 0x00,
        ]);
        // `SOS`, after which nothing is a marker.
        out.extend_from_slice(&[0xff, 0xda, 0x00, 0x08, 0x01, 0x01, 0x00, 0x00, 0x3f, 0x00]);
        out.extend_from_slice(&[0x12, 0x34, 0x56]);
        out.extend_from_slice(&[0xff, 0xd0]); // a restart marker
        out.extend_from_slice(&[0x78, 0x9a]);
        out.extend_from_slice(&[0xff, 0x00, 0xff, 0xff]); // a stuffed 0xFF, then "length 65535"
        out.extend_from_slice(&[0xbc, 0xde]);
        out.extend_from_slice(&[0xff, 0xd9]);
        out
    }

    #[test]
    fn a_scan_full_of_marker_shaped_bytes_is_stamped_without_being_parsed() {
        // The PF:16 regression, forever. A stamper that walks past `SOS` reads a length
        // out of the image data and runs off the end of the buffer — which is what
        // `little_exif`'s own JPEG path does, and why nine of forty frames from the
        // Chicony failed to stamp before this.
        let original = jpeg_with_marker_shaped_scan_bytes();
        let stamped = stamp_jpeg(&original, &metadata(Transform::None))
            .expect("a scan is not a place markers live");

        let offset = scan_offset(&original).expect("the fixture has a scan");
        let scan = original.get(offset..).expect("in range");
        assert!(
            stamped.ends_with(scan),
            "the bytes after SOS were touched; they are image data, not structure"
        );
        assert!(
            stamped.starts_with(&[0xff, 0xd8, 0xff, 0xe1]),
            "the APP1 goes immediately after the SOI"
        );

        // And it is real EXIF, read back by the independent implementation — a splice
        // that produced an unreadable segment would satisfy every assertion above.
        let mut cursor = std::io::Cursor::new(stamped);
        let exif = exif::Reader::new()
            .read_from_container(&mut cursor)
            .expect("the spliced segment is readable EXIF");
        assert!(
            exif.get_field(Tag::Make, In::PRIMARY).is_some(),
            "the tags survived the splice"
        );
    }

    #[test]
    fn an_exif_segment_the_camera_wrote_is_replaced_rather_than_joined() {
        // Two EXIF segments in one file is undefined, and a reader picks one arbitrarily.
        // Stamping a photo that already carries EXIF must leave exactly one — ours.
        let once = stamp_jpeg(&sample_jpeg(), &metadata(Transform::None)).expect("stamp");
        let twice = stamp_jpeg(&once, &metadata(Transform::Rot90)).expect("re-stamp");

        assert_eq!(
            count_app1_exif(&twice),
            1,
            "re-stamping left more than one EXIF segment"
        );
        // …and the *second* stamp is the one that survived.
        let mut cursor = std::io::Cursor::new(twice);
        let exif = exif::Reader::new()
            .read_from_container(&mut cursor)
            .expect("readable");
        assert_eq!(
            exif.get_field(Tag::Orientation, In::PRIMARY)
                .expect("orientation")
                .value
                .get_uint(0),
            Some(6),
            "the newer stamp must win"
        );
    }

    /// How many EXIF APP1 segments a file's *header* holds.
    fn count_app1_exif(jpeg: &[u8]) -> usize {
        let mut at = 2usize;
        let mut found = 0usize;
        while let Some(segment) = next_segment(jpeg, at) {
            match segment {
                Segment::ScanBegins => break,
                Segment::Standalone { end } => at = end,
                Segment::Sized { marker, body, end } => {
                    if marker == APP1
                        && jpeg
                            .get(body..body + EXIF_SIGNATURE.len())
                            .is_some_and(|p| p == EXIF_SIGNATURE)
                    {
                        found += 1;
                    }
                    at = end;
                }
            }
        }
        found
    }

    #[test]
    fn a_segment_claiming_more_bytes_than_the_file_holds_ends_the_walk() {
        // A camera's bitstream is device data (rubric B10). A header segment whose length
        // runs past the end must stop the walk rather than index past it — and the file
        // still gets its stamp, because the bytes after the bad segment are copied
        // verbatim rather than interpreted.
        let mut truncated = vec![0xff, 0xd8];
        truncated.extend_from_slice(&[0xff, 0xe0, 0xff, 0xff]); // APP0, "length 65535"
        truncated.extend_from_slice(&[0x00, 0x01, 0x02]);
        assert!(next_segment(&truncated, 2).is_none());

        let stamped = stamp_jpeg(&truncated, &metadata(Transform::None))
            .expect("a malformed header is not a reason to lose the photo");
        assert!(stamped.starts_with(&[0xff, 0xd8, 0xff, 0xe1]));
        assert!(
            stamped.ends_with(&[0xff, 0xe0, 0xff, 0xff, 0x00, 0x01, 0x02]),
            "everything the walk could not interpret is copied through"
        );

        // A zero length is the same class of lie and gets the same answer.
        let zero = [0xff, 0xd8, 0xff, 0xe0, 0x00, 0x00];
        assert!(next_segment(&zero, 2).is_none());
    }

    #[test]
    fn a_different_transform_stamps_a_different_orientation() {
        // The inverse of the read-back test: if Orientation were hard-coded, the test
        // above would still pass.
        for (transform, expected) in [
            (Transform::None, 1),
            (Transform::HFlip, 2),
            (Transform::Rot180, 3),
            (Transform::VFlip, 4),
            (Transform::Rot90, 6),
            (Transform::Rot270, 8),
        ] {
            let stamped = stamp_jpeg(&sample_jpeg(), &metadata(transform)).expect("stamp");
            let data = read_back(&stamped);
            assert_eq!(
                data.get_field(Tag::Orientation, In::PRIMARY)
                    .and_then(|field| field.value.get_uint(0)),
                Some(expected),
                "{transform:?} stamped the wrong orientation"
            );
        }
    }

    #[test]
    fn stamping_twice_leaves_one_exif_segment_with_the_second_stamp() {
        // Cameras ship their own APP1. Two EXIF segments in one file is undefined, and
        // whichever a reader picks, it must not be the stale one.
        let once = stamp_jpeg(&sample_jpeg(), &metadata(Transform::Rot90)).expect("stamp");
        let twice = stamp_jpeg(&once, &metadata(Transform::VFlip)).expect("restamp");
        let data = read_back(&twice);
        assert_eq!(
            data.get_field(Tag::Orientation, In::PRIMARY)
                .and_then(|field| field.value.get_uint(0)),
            Some(4)
        );
        // And the file did not grow by a whole second segment.
        assert!(
            twice.len() <= once.len() + 16,
            "restamping accumulated segments: {} then {}",
            once.len(),
            twice.len()
        );
    }

    #[test]
    fn a_buffer_that_is_not_a_jpeg_is_refused() {
        let err = stamp_jpeg(&[0u8; 64], &metadata(Transform::None)).expect_err("not a JPEG");
        assert!(err.to_string().contains("SOI"), "{err}");
    }

    #[test]
    fn a_capture_with_no_controls_says_so_rather_than_writing_an_empty_tag() {
        let mut meta = metadata(Transform::None);
        meta.controls.clear();
        let stamped = stamp_jpeg(&sample_jpeg(), &meta).expect("stamp");
        let data = read_back(&stamped);
        let comment = match &data
            .get_field(Tag::UserComment, In::PRIMARY)
            .expect("UserComment")
            .value
        {
            Value::Undefined(bytes, _) => String::from_utf8_lossy(bytes).into_owned(),
            other => panic!("UserComment is {other:?}"),
        };
        assert!(comment.contains("(none recorded)"), "{comment}");
    }

    #[test]
    fn an_absent_serial_is_omitted_rather_than_written_empty() {
        // PF:8: the OBSBOT reports none. `serial=` would read as a serial that is the
        // empty string, which is a different claim from "the device reports none".
        let mut meta = metadata(Transform::None);
        meta.camera.serial = None;
        meta.camera.usb_id = None;
        let described = describe_capture(&meta);
        assert!(!described.contains("serial="), "{described}");
        assert!(!described.contains("usb="), "{described}");
        assert!(describe_capture(&metadata(Transform::None)).contains("serial=0001"));
    }

    #[test]
    fn the_exif_datetime_spelling_is_the_standards_one() {
        assert_eq!(
            exif_datetime(Stamp::epoch()).expect("epoch renders"),
            "1970:01:01 00:00:00"
        );
        let later = Stamp::from_millis(1_754_654_400_123).expect("in range");
        let rendered = exif_datetime(later).expect("renders");
        assert_eq!(rendered.len(), 19, "{rendered}");
        assert!(rendered.starts_with("2025:08:"), "{rendered}");
        // Fractional seconds are not part of the EXIF spelling and must not leak in.
        assert!(!rendered.contains('.'), "{rendered}");
        assert!(!rendered.contains('T'), "{rendered}");
    }
}
