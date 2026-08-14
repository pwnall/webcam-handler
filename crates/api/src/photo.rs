//! The photo answer, with its bytes (design D6, D10).
//!
//! This module is the one home of D10's base64-in-JSON encoding. `webcam-handler-schema`'s
//! [`PhotoDelivery`] doc comment legislates the split from the other side — "**The bytes
//! themselves are not in this document**: `webcam-handler-cli` streams them to standard
//! output, and D10's base64-in-JSON encoding lives with the wire surface that needs it" —
//! because a `--json` document and an on-disk session file never carry a frame, and an
//! encoding nobody reads would be a dependency nobody reads.
//!
//! `webcam-handler-cli-core`'s `Photograph` is the same pair of facts for the in-process
//! caller and stays where it is: `webcam-handler-cli` already has the bytes in memory and
//! needs no encoding at all. `webcam-handler-client` decodes a [`PhotoResponse`] into a
//! `Photograph` and hands that to the renderer both binaries share, which is what makes
//! `webcam-handler-cli photo` and `webcam-handler-client photo` write the same file (P4f's
//! parity gate).

use std::fmt;

use base64::prelude::{BASE64_STANDARD, Engine as _};
use schema::capture::{PhotoDelivery, PhotoReport};
use schemars::JsonSchema;
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Bytes on a JSON wire: base64, the standard alphabet, padded (design D10).
///
/// A newtype rather than a bare `String` field, for two reasons, and the second one is a
/// privacy rule.
///
/// First, the encoding is a *transport* decision, and this type is where it happens once.
/// `webcam-handler-api` is the only crate in the workspace that declares the `base64`
/// dependency at all, which is what keeps `webcam-handler-schema` free of it.
///
/// Second: **a frame may contain a person** (AGENTS.md; rubric A12). A derived `Debug` on
/// anything holding photo bytes would print them into a log line or a panic message, and
/// base64 of a face is still a face — so `Debug` here prints the byte count and nothing
/// else, exactly as `schema::capture::Frame` does, and every document that embeds this
/// inherits that property rather than having to remember it.
///
/// The alphabet is standard and padded rather than URL-safe: these bytes ride in a JSON
/// string, never in a URL, and padded standard base64 is what a JSON-RPC consumer's own
/// library decodes by default.
#[derive(Clone, PartialEq, Eq, JsonSchema)]
#[schemars(transparent)]
pub struct Base64Bytes(#[schemars(with = "String")] Vec<u8>);

impl Base64Bytes {
    /// Carry `bytes` across the wire.
    #[must_use]
    pub const fn new(bytes: Vec<u8>) -> Base64Bytes {
        Base64Bytes(bytes)
    }

    /// The bytes, borrowed.
    #[must_use]
    pub fn as_slice(&self) -> &[u8] {
        &self.0
    }

    /// How many bytes there are.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether there are none.
    ///
    /// Zero bytes is a representable answer, not an absent one: a device that produced an
    /// empty buffer is a fact about the device, and collapsing it into "no bytes at all"
    /// would be the availability-as-capability conversion E3 exists to prevent.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The bytes, owned.
    #[must_use]
    pub fn into_inner(self) -> Vec<u8> {
        self.0
    }
}

impl fmt::Debug for Base64Bytes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // A frame may contain a person. Never the bytes, and never the base64 of them.
        write!(f, "Base64Bytes(<{} bytes>)", self.0.len())
    }
}

impl Serialize for Base64Bytes {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&BASE64_STANDARD.encode(&self.0))
    }
}

impl<'de> Deserialize<'de> for Base64Bytes {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        // The refusal carries base64's own complaint — an alphabet symbol and an offset,
        // never a decoded byte, so a malformed payload cannot leak a frame into a log line
        // by way of an error message (rubric A12).
        BASE64_STANDARD
            .decode(text.as_bytes())
            .map(Base64Bytes)
            .map_err(D::Error::custom)
    }
}

/// What `wch_photo` answers: the report every caller gets, and the bytes a `ReturnBytes`
/// sink asked for (design D6, D10).
///
/// The bytes ride *beside* the report rather than inside
/// [`PhotoDelivery::Bytes`] because `PhotoDelivery` is a schema type that a session file
/// and a `--json` document also carry, and putting an encoding there would put base64 in
/// all three. One JSON-RPC response is one document, so the two travel together here.
///
/// The whole [`PhotoReport`] survives the wire, untrimmed: `negotiated`, `rendering`,
/// `transform` and `frames_settled` are how a caller learns that the device did not agree
/// to what was asked, and a wire answer that dropped them would be a layer collapsing
/// `{requested, applied}` into one value (rubric A10).
///
/// **Size.** A 4K MJPG frame is roughly 8 MB, which is about 10.7 MB of base64 — past
/// jsonrpsee's default 10 MB response ceiling. Configuring that ceiling is the daemon's
/// job (P4b) and `Sink::ServerPath` is the sink that does not pay the encoding at all;
/// this is recorded here so the limit is met in a design note rather than in the field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PhotoResponse {
    /// What was taken, where it went, and what was done to it.
    pub report: PhotoReport,
    /// The bytes, for a `Sink::ReturnBytes` request; absent for `Sink::ServerPath`.
    ///
    /// `Option` rather than an empty vector, because "the caller asked for a file" and
    /// "the caller asked for bytes and got none" are different answers, and
    /// [`PhotoResponse::bytes_match_the_delivery`] is the predicate that tells them apart,
    /// called at both ends of the socket since P4f (see that method's own note).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bytes: Option<Base64Bytes>,
}

impl PhotoResponse {
    /// Whether the bytes and the report agree about what was delivered.
    ///
    /// Two claims in one predicate, because they fail together: bytes are present exactly
    /// when the delivery says `Bytes`, and there are exactly as many of them as the
    /// delivery's `byte_count` says. The second half is what stops `byte_count` from being
    /// decorative — a count nobody checks is a field that drifts silently, and this is the
    /// one place both facts are visible at once.
    ///
    /// A predicate rather than a constructor invariant: this document arrives off a
    /// socket, so it can be built by somebody else's code, and a type that could not
    /// *represent* the disagreement could not refuse it either.
    ///
    /// **Both of its consumers have landed**, one per end of the socket, which is what
    /// makes the refusal mutual rather than a sender's courtesy.
    /// `daemon::server::photo_response` calls this as the last statement before an answer
    /// leaves the process (P4c), so there is exactly one place a daemon can skip it, and a
    /// document that disagrees with itself is refused as `Error::DeviceIo` — ours, not the
    /// device's. `client::remote::Remote::photo` calls it on a response it *received*,
    /// before turning it into a `cli_core::Photograph` (P4f), so a truncated payload is now
    /// refused at both ends instead of by the sender and by nobody.
    ///
    /// Checked off at the G4 close, and the check-off is the point: note **N34** recorded
    /// this as one of four declarations with no consumer precisely so that the review
    /// closing G4 would count them rather than rediscover them. It is also the one that
    /// showed the mechanism can point the wrong way — the receiving call site landed at
    /// P4f and this paragraph went on claiming the absence for two sub-milestones, which
    /// is a declaration outliving the thing it declared. E13's photo arm drives the
    /// received half against three real cameras.
    #[must_use]
    pub fn bytes_match_the_delivery(&self) -> bool {
        match (&self.report.delivery, &self.bytes) {
            (PhotoDelivery::Bytes { byte_count, .. }, Some(bytes)) => {
                u64::try_from(bytes.len()).is_ok_and(|len| len == *byte_count)
            }
            (PhotoDelivery::Path { .. }, None) => true,
            (PhotoDelivery::Bytes { .. }, None) | (PhotoDelivery::Path { .. }, Some(_)) => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use schema::camera::{CameraId, FrameInterval, PixelFormat};
    use schema::capture::{NegotiatedStream, PhotoFormat, PhotoRendering, TransformApplication};
    use schema::time::Stamp;

    use super::*;

    fn negotiated() -> NegotiatedStream {
        NegotiatedStream {
            pixel_format: PixelFormat::MJPG,
            width: 1920,
            height: 1080,
            bytes_per_line: 0,
            size_image: 1_048_576,
            interval: FrameInterval::Discrete {
                numerator: 1,
                denominator: 30,
            },
            adjustments: Vec::new(),
        }
    }

    fn report(delivery: PhotoDelivery) -> PhotoReport {
        PhotoReport {
            camera: CameraId::parse("cam:test").expect("literal id"),
            taken_at: Stamp::epoch(),
            negotiated: negotiated(),
            rendering: PhotoRendering::Verbatim {
                source: PixelFormat::MJPG,
            },
            transform: TransformApplication::Identity,
            width: 1920,
            height: 1080,
            frames_settled: 3,
            delivery,
        }
    }

    fn bytes_response(bytes: Vec<u8>) -> PhotoResponse {
        let byte_count = u64::try_from(bytes.len()).expect("a fixture that fits");
        PhotoResponse {
            report: report(PhotoDelivery::Bytes {
                format: PhotoFormat::Jpeg,
                byte_count,
            }),
            bytes: Some(Base64Bytes::new(bytes)),
        }
    }

    #[test]
    fn a_returned_photo_arrives_byte_for_byte_after_base64_and_back() {
        // E6 is the product: a verbatim MJPG frame that goes out as `ReturnBytes` is the
        // camera's own bytes, so base64 has to be a transport encoding and nothing else.
        // The payload is deliberately not text — high bytes, an interior NUL, and the JPEG
        // SOI/EOI markers — because a `String` round-trip would pass a UTF-8 fixture and
        // lose this one.
        let original: Vec<u8> = vec![
            0xff, 0xd8, 0xff, 0xe0, 0x00, 0x10, 0x4a, 0x46, 0x49, 0x46, 0x00, 0x80, 0xfe, 0xff,
            0xd9,
        ];
        let response = bytes_response(original.clone());
        let json = serde_json::to_string(&response).expect("serialize");
        let back: PhotoResponse = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, response);
        assert_eq!(
            back.bytes.as_ref().expect("bytes came back").as_slice(),
            original.as_slice(),
            "the bytes changed on the way through base64"
        );
        // The encoding is standard and padded, which is what a consumer's own base64
        // library decodes by default. Pinned, because switching alphabets is a wire break
        // that nothing else would notice.
        assert!(json.contains(&BASE64_STANDARD.encode(&original)), "{json}");
    }

    #[test]
    fn the_payload_reports_its_own_size_and_hands_the_same_bytes_back() {
        // The accessors a consumer decodes through: `webcam-handler-client` turns a
        // `PhotoResponse` into a `cli_core::Photograph` by taking the bytes out (P4f), and
        // `bytes_match_the_delivery` compares the count. Both directions of `is_empty`,
        // because a payload that always reported "empty" would let a `ReturnBytes` answer read
        // as a photo the device never produced.
        let payload = Base64Bytes::new(vec![0x01, 0x02, 0x03]);
        assert_eq!(payload.len(), 3);
        assert!(!payload.is_empty());
        assert_eq!(payload.as_slice(), &[0x01, 0x02, 0x03]);
        assert_eq!(payload.clone().into_inner(), vec![0x01, 0x02, 0x03]);

        let nothing = Base64Bytes::new(Vec::new());
        assert_eq!(nothing.len(), 0);
        assert!(nothing.is_empty());
        assert!(nothing.into_inner().is_empty());
    }

    #[test]
    fn an_empty_photo_is_an_answer_rather_than_an_absent_one() {
        // Zero bytes and "no bytes field at all" are different claims: the first is a
        // device that produced an empty buffer, the second is a photo written to a file.
        // A round-trip that folded one into the other would let a `ServerPath` answer read
        // as an empty `ReturnBytes` one.
        let empty = bytes_response(Vec::new());
        let json = serde_json::to_string(&empty).expect("serialize");
        assert!(json.contains(r#""bytes":"""#), "{json}");
        let back: PhotoResponse = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, empty);
        assert!(back.bytes.as_ref().expect("an empty payload").is_empty());
        assert!(back.bytes_match_the_delivery());

        let to_a_file = PhotoResponse {
            report: report(PhotoDelivery::Path {
                path: "/tmp/out.jpg".into(),
                byte_count: 0,
            }),
            bytes: None,
        };
        // The `ServerPath` half of the predicate — half of D10's two-variant sink
        // semantics, and the arm that survived a mutation run when it was missing:
        // flipping this answer to `false` left the whole workspace green, which would
        // have shipped a daemon refusing every legitimate `--out file.jpg` answer as
        // malformed.
        assert!(to_a_file.bytes_match_the_delivery());

        let json = serde_json::to_string(&to_a_file).expect("serialize");
        assert!(!json.contains(r#""bytes":"#), "{json}");
        assert_eq!(
            serde_json::from_str::<PhotoResponse>(&json).expect("deserialize"),
            to_a_file
        );
        assert_ne!(to_a_file, empty);
    }

    #[test]
    fn the_bytes_and_the_delivery_have_to_agree_and_the_predicate_notices_when_they_do_not() {
        let good = bytes_response(vec![0x01, 0x02, 0x03]);
        assert!(good.bytes_match_the_delivery());

        // A count that does not match the payload. This is the assertion that makes
        // `byte_count` load-bearing rather than decorative — without it a truncated
        // payload would arrive looking exactly like a whole one.
        let miscounted = PhotoResponse {
            report: report(PhotoDelivery::Bytes {
                format: PhotoFormat::Jpeg,
                byte_count: 4,
            }),
            bytes: Some(Base64Bytes::new(vec![0x01, 0x02, 0x03])),
        };
        assert!(!miscounted.bytes_match_the_delivery());

        // Both mismatched shapes, because a predicate that only catches one of them is
        // half a predicate: a `Bytes` delivery with nothing attached, and a `Path`
        // delivery with a payload the caller never asked for.
        let promised_but_absent = PhotoResponse {
            bytes: None,
            ..good.clone()
        };
        assert!(!promised_but_absent.bytes_match_the_delivery());
        let unasked_for = PhotoResponse {
            report: report(PhotoDelivery::Path {
                path: "/tmp/out.jpg".into(),
                byte_count: 3,
            }),
            bytes: Some(Base64Bytes::new(vec![0x01, 0x02, 0x03])),
        };
        assert!(!unasked_for.bytes_match_the_delivery());
    }

    #[test]
    fn photo_bytes_never_reach_a_debug_line() {
        // Rubric A12 as a test, and the reason `Base64Bytes` hand-writes `Debug`: a frame
        // may contain a person, so formatting any document that holds one must be
        // incapable of printing it — base64 of a face is still a face.
        let response = bytes_response(vec![0xde, 0xad, 0xbe, 0xef]);
        let rendered = format!("{response:?}");
        assert!(rendered.contains("<4 bytes>"), "{rendered}");
        assert!(!rendered.contains("222"), "byte values leaked: {rendered}");
        assert!(
            !rendered.contains(&BASE64_STANDARD.encode([0xde, 0xad, 0xbe, 0xef])),
            "the base64 leaked: {rendered}"
        );
    }

    #[test]
    fn a_payload_that_is_not_base64_is_refused_rather_than_defaulted() {
        // The inverse arm: an encoding this build cannot read must be a refusal, not an
        // empty photo. Silently yielding no bytes would turn a corrupted response into a
        // camera that produced nothing, which is E3's conversion at the transport layer.
        //
        // The document is built from a *valid* response and only its payload corrupted,
        // because that is the only way this asserts what it claims: a hand-written
        // `{"report":null,…}` is refused at `report`, before serde ever reaches `bytes`,
        // and would keep passing if `Base64Bytes` silently decoded to nothing.
        let valid = serde_json::to_string(&bytes_response(vec![1, 2, 3])).expect("serialize");
        let json = valid.replace(&BASE64_STANDARD.encode([1, 2, 3]), "not base64!!");
        assert_ne!(json, valid, "the payload was not where the corruption went");
        assert!(
            serde_json::from_str::<PhotoResponse>(&json).is_err(),
            "{json}"
        );
        // And the twin: the same document, uncorrupted, is accepted — so the refusal above
        // is a refusal of the payload rather than of the shape it travelled in.
        assert!(serde_json::from_str::<PhotoResponse>(&valid).is_ok());

        let broken = serde_json::from_str::<Base64Bytes>(r#""not base64!!""#)
            .expect_err("an unreadable payload is refused");
        assert!(
            !format!("{broken}").is_empty(),
            "the refusal has to say something"
        );

        // And the healthy twin, so the refusal is a refusal of *this* input rather than of
        // everything: the same three bytes, correctly encoded, decode.
        let good: Base64Bytes =
            serde_json::from_str(&format!("\"{}\"", BASE64_STANDARD.encode([1, 2, 3])))
                .expect("well-formed base64 decodes");
        assert_eq!(good.as_slice(), &[1, 2, 3]);
    }
}
