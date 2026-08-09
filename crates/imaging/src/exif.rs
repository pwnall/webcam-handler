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
use schema::time::Stamp;

use crate::fault::imaging_failure;

const OP: &str = "stamp EXIF onto JPEG";

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
    splice_app1(jpeg, &app1)
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
    parts.join("; ")
}

/// The control values in effect, as an EXIF `UserComment` payload.
///
/// Returns the eight-byte ASCII character-code prefix followed by the text, which is
/// what the tag's `UNDEF` type means.
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
    out.extend_from_slice(body.as_bytes());
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
