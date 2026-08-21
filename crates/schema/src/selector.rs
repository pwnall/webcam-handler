//! The spellings a caller may use to say which camera they mean (design D14).
//!
//! A caller on a multi-camera machine usually already holds something that identifies the
//! camera — a `/dev` node it just watched appear, a bus position, a VID:PID, a serial — and
//! before D14 this tool made it re-derive the card-name slug from an enumeration first. The
//! knowledge is a spelling now, and this module is the only place a spelling becomes one.
//!
//! **One parser.** Both command-line positionals, the wire's `camera` parameter and the
//! embedding facade go through [`parse`]; [`CameraSelector`]'s `Deserialize` goes through it
//! too, so a value that reaches a handler is one this function made — the same reason
//! [`crate::camera::CameraId`] hand-writes its own (note **N50**), one grammar wider.
//!
//! **The scheme table is closed, and that is what makes a mistake safe.** A card slug's
//! alphabet is `[a-z0-9-]`, so it can never contain a colon: a colon-delimited scheme this
//! build does not know is unambiguously a *selector*, not an id that happens to look odd,
//! and it is refused as [`crate::error::Error::CameraUnknown`] naming the vocabulary. A
//! spelling mistake is a request no camera can ever match, which is precisely what that kind
//! means — which is why D14 adds no error kind.
//!
//! **Address versus identity.** [`CameraSelector::NodePath`] is an *address*: node numbers
//! are probe-order bookkeeping that one driver reload rotates \[PF:22\], so it is resolved
//! against the live listing at every call and is the session-scoped choice for a caller that
//! just materialized a node. `BusPath`, `UsbId` and `Serial` are fingerprint-tier and survive
//! a replug. [`SelectorScheme::stability`] carries that split as data, because the guide's
//! selector table is generated from it rather than written twice.

use std::fmt;

use camino::Utf8PathBuf;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::camera::{CAMERA_ID_PREFIX, CameraId, UsbId};
use crate::error::{Error, Result};
use crate::vocabulary::closed_vocabulary;

closed_vocabulary! {
    /// Which spelling a selector is written in.
    ///
    /// `ALL` is generated from this definition, so a sixth spelling cannot be added without
    /// joining the vocabulary sentence a refusal prints, the guide's selector table, and
    /// every walk that resolves one.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum SelectorScheme {
        /// `cam:<slug>`, or a bare slug, or any unambiguous prefix of one — D1's grammar,
        /// unchanged.
        Id,
        /// A `/dev` node path, matched against any of a camera's nodes.
        NodePath,
        /// `bus:<interface-path>`, matched against the fingerprint's bus path.
        BusPath,
        /// `usb:<vid>:<pid>`, hex, matched against the fingerprint's USB id.
        UsbId,
        /// `serial:<text>`, matched against the fingerprint's serial.
        Serial,
    }
}

/// Whether a spelling survives a replug, or only names where the device is right now.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Stability {
    /// Resolved against the live listing every call, and rotated by a driver reload
    /// \[PF:22\]: correct for a caller that just watched the node appear, wrong to persist.
    Address,
    /// Part of the identity that survives a replug and a reboot — the fingerprint tier.
    Fingerprint,
}

impl SelectorScheme {
    /// The literal a spelling in this scheme begins with, when it has one.
    ///
    /// `None` for the two schemes recognized by shape rather than by prefix: an id may be
    /// written bare, and a node path is recognized by its leading `/`.
    #[must_use]
    pub const fn prefix(self) -> Option<&'static str> {
        match self {
            SelectorScheme::Id => Some(CAMERA_ID_PREFIX),
            SelectorScheme::NodePath => None,
            SelectorScheme::BusPath => Some("bus:"),
            SelectorScheme::UsbId => Some("usb:"),
            SelectorScheme::Serial => Some("serial:"),
        }
    }

    /// How a caller writes one, terse enough to sit in a list.
    ///
    /// The spelling only, because both readers put it in a table or a comma-separated
    /// sentence: what each one *matches* is prose beside it, not inside it. The `cam:`
    /// prefix being optional is a convenience of the id grammar rather than a sixth
    /// spelling, so it is taught where there is room — the guide's selector table — and not
    /// in a refusal that has to stay one line.
    #[must_use]
    pub const fn example(self) -> &'static str {
        match self {
            SelectorScheme::Id => "cam:<id>",
            SelectorScheme::NodePath => "/dev/videoN",
            SelectorScheme::BusPath => "bus:<interface-path>",
            SelectorScheme::UsbId => "usb:<vid>:<pid>",
            SelectorScheme::Serial => "serial:<text>",
        }
    }

    /// Whether this spelling is an address or part of the device's identity.
    #[must_use]
    pub const fn stability(self) -> Stability {
        match self {
            // An id is derived from the card name and is stable for one engine's lifetime,
            // but D1 assigns collision ordinals over the whole machine — so it moves when
            // the machine's population does, which is what an address does.
            SelectorScheme::Id | SelectorScheme::NodePath => Stability::Address,
            SelectorScheme::BusPath | SelectorScheme::UsbId | SelectorScheme::Serial => {
                Stability::Fingerprint
            }
        }
    }
}

/// Every spelling this build understands, as one sentence for a refusal to carry.
///
/// Derived from [`SelectorScheme::ALL`], so a sixth scheme joins the message by existing.
#[must_use]
pub fn vocabulary() -> String {
    let spellings: Vec<&'static str> = SelectorScheme::ALL
        .iter()
        .map(|scheme| scheme.example())
        .collect();
    spellings.join("; ")
}

/// The clause [`Error::CameraUnknown`]'s message adds when the request was a spelling
/// mistake rather than a camera that is not here.
///
/// Empty for everything else, and that emptiness is the point: `no camera matches
/// "cam:obsbot"` is a fact about the machine, and appending a grammar lesson to it would
/// teach an unattended reader that its own spelling was wrong when the camera was simply
/// unplugged. The clause appears exactly when [`parse`] would refuse the string — a colon
/// scheme this build does not know, a scheme with an empty body, a malformed `usb:` pair, or
/// nothing at all — because those are the requests whose repair is a different spelling.
#[must_use]
pub fn scheme_hint(requested: &str) -> String {
    if parse(requested).is_ok() {
        return String::new();
    }
    format!(" — a camera selector is one of: {}", vocabulary())
}

/// One camera, named in any of D14's five spellings.
///
/// Built only by [`parse`] — including on the way in off a socket, where the derived
/// `Deserialize` a `#[serde(transparent)]` newtype would have given us is exactly the
/// wildcard note **N50** measured.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CameraSelector {
    /// D1's id grammar: `cam:` optional, unambiguous-prefix matching unchanged.
    Id(CameraId),
    /// A `/dev` node path, matched against **any** of a camera's nodes — you address by
    /// what you can see in `/dev`, not only by the capture node.
    NodePath(Utf8PathBuf),
    /// A USB interface path, matched exactly against the fingerprint's `bus_path`.
    BusPath(String),
    /// A vendor:product pair, matched against the fingerprint's `usb_id`.
    UsbId(UsbId),
    /// A serial, matched against the fingerprint's `serial` — a device that reports none
    /// matches nothing \[PF:8\].
    Serial(String),
}

impl CameraSelector {
    /// Which spelling this is.
    ///
    /// The match is exhaustive over a vocabulary with an `ALL`, so a sixth spelling cannot
    /// be added without giving it a scheme.
    #[must_use]
    pub const fn scheme(&self) -> SelectorScheme {
        match self {
            CameraSelector::Id(_) => SelectorScheme::Id,
            CameraSelector::NodePath(_) => SelectorScheme::NodePath,
            CameraSelector::BusPath(_) => SelectorScheme::BusPath,
            CameraSelector::UsbId(_) => SelectorScheme::UsbId,
            CameraSelector::Serial(_) => SelectorScheme::Serial,
        }
    }

    /// The id this selector names, when it names one directly.
    ///
    /// `None` for the four spellings that need the enumeration to answer — which is every
    /// one of them except an id, and the reason resolution lives in the engine rather than
    /// on this type.
    #[must_use]
    pub const fn as_id(&self) -> Option<&CameraId> {
        match self {
            CameraSelector::Id(id) => Some(id),
            CameraSelector::NodePath(_)
            | CameraSelector::BusPath(_)
            | CameraSelector::UsbId(_)
            | CameraSelector::Serial(_) => None,
        }
    }
}

impl fmt::Display for CameraSelector {
    /// The canonical spelling — the one [`parse`] round-trips.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CameraSelector::Id(id) => f.write_str(id.as_str()),
            CameraSelector::NodePath(path) => f.write_str(path.as_str()),
            CameraSelector::BusPath(bus) => write!(f, "bus:{bus}"),
            CameraSelector::UsbId(usb) => write!(f, "usb:{usb}"),
            CameraSelector::Serial(serial) => write!(f, "serial:{serial}"),
        }
    }
}

impl From<CameraId> for CameraSelector {
    fn from(id: CameraId) -> Self {
        CameraSelector::Id(id)
    }
}

/// Turn a caller's spelling into the camera they mean.
///
/// The dispatch is by shape and then by scheme, in that order, and both halves are decided
/// by the same fact: a card slug's alphabet is `[a-z0-9-]`, so neither a leading `/` nor a
/// colon can occur in an id. A leading `/` is therefore a node path; a colon is therefore a
/// scheme; anything else is D1's id grammar, which is what every caller wrote before v3 and
/// still writes.
///
/// # Errors
///
/// [`Error::CameraUnknown`] for a spelling no camera could ever match: the empty string, a
/// scheme this build does not know, a scheme with an empty body, or a `usb:` pair that is
/// not two hex numbers. Its message names the vocabulary — the refusal is where an
/// unattended caller learns the grammar, since there is nobody to read a manual.
pub fn parse(input: &str) -> Result<CameraSelector> {
    let unknown = || Error::CameraUnknown {
        requested: input.to_owned(),
    };

    if input.is_empty() {
        return Err(unknown());
    }

    if input.starts_with('/') {
        return Ok(CameraSelector::NodePath(Utf8PathBuf::from(input)));
    }

    // Split on the *first* colon only: a bus path carries colons of its own
    // (`bus:3-4:1.2`), and a usb pair is two halves of one body.
    let Some((scheme, body)) = input.split_once(':') else {
        return CameraId::parse(input)
            .map(CameraSelector::Id)
            .ok_or_else(unknown);
    };

    if body.is_empty() {
        return Err(unknown());
    }

    match scheme {
        "cam" => CameraId::parse(body)
            .map(CameraSelector::Id)
            .ok_or_else(unknown),
        "bus" => Ok(CameraSelector::BusPath(body.to_owned())),
        "usb" => parse_usb(body)
            .map(CameraSelector::UsbId)
            .ok_or_else(unknown),
        "serial" => Ok(CameraSelector::Serial(body.to_owned())),
        _ => Err(unknown()),
    }
}

/// `vvvv:pppp`, hex, both halves required.
///
/// Deliberately stricter than `u16::from_str_radix` alone: that function accepts a leading
/// `+`, so `usb:+4f2:+b83c` would parse to the Chicony's id and print back as something the
/// caller never wrote. A spelling that does not round-trip is not the spelling.
fn parse_usb(body: &str) -> Option<UsbId> {
    let (vendor, product) = body.split_once(':')?;
    Some(UsbId {
        vendor: parse_hex16(vendor)?,
        product: parse_hex16(product)?,
    })
}

fn parse_hex16(text: &str) -> Option<u16> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    u16::from_str_radix(text, 16).ok()
}

impl Serialize for CameraSelector {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for CameraSelector {
    /// Through [`parse`], for note **N50**'s reason one grammar wider: a derived
    /// `Deserialize` would accept any string at all, and `""` is a prefix of every id.
    fn deserialize<D>(deserializer: D) -> std::result::Result<CameraSelector, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        parse(&raw).map_err(|_| {
            // Through [`vocabulary`], not transcribed beside it. This sentence used to spell
            // the five schemes out a hundred and ninety lines below the function that renders
            // them, and the only test on this door asserted `is_err()` without reading the
            // message — so a sixth scheme would have joined every other reader of `ALL` and
            // not this one, and the wire would have taught a grammar the parser no longer had.
            let expected = format!("a camera selector: {}", vocabulary());
            serde::de::Error::invalid_value(serde::de::Unexpected::Str(&raw), &&*expected)
        })
    }
}

impl JsonSchema for CameraSelector {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "CameraSelector".into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        // A string on the wire, exactly as `CameraId` was: the grammar is the parser's, and
        // a JSON Schema that tried to spell it as a pattern would be a second copy of the
        // rule that could disagree with the one that runs.
        let mut schema = String::json_schema(generator);
        schema.insert(
            "description".to_owned(),
            serde_json::Value::String(format!(
                "Which camera, in any of the spellings webcam-handler understands: {}.",
                vocabulary()
            )),
        );
        schema
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One well-formed spelling per scheme, in one place, so the walks below quantify over
    /// [`SelectorScheme::ALL`] and a sixth scheme fails them by having no sample rather than
    /// by being quietly untested.
    fn samples() -> Vec<(SelectorScheme, &'static str)> {
        vec![
            (SelectorScheme::Id, "cam:integrated-camera"),
            (SelectorScheme::NodePath, "/dev/video0"),
            (SelectorScheme::BusPath, "bus:3-4:1.2"),
            (SelectorScheme::UsbId, "usb:04f2:b83c"),
            (SelectorScheme::Serial, "serial:0001"),
        ]
    }

    #[test]
    fn every_spelling_parses_to_its_own_scheme() {
        let samples = samples();
        for scheme in SelectorScheme::ALL {
            let (_, spelling) = samples
                .iter()
                .find(|(candidate, _)| candidate == scheme)
                .expect("every scheme in the vocabulary has a sample spelling here");
            let parsed = parse(spelling).expect("the sample spelling parses");
            assert_eq!(
                parsed.scheme(),
                *scheme,
                "{spelling} named the wrong scheme"
            );
        }
        assert_eq!(samples.len(), SelectorScheme::ALL.len());
    }

    #[test]
    fn a_bare_slug_and_a_prefixed_one_are_the_same_selector() {
        assert_eq!(
            parse("obsbot-tiny-3").expect("a bare slug is D1's grammar"),
            parse("cam:obsbot-tiny-3").expect("and so is the prefixed form")
        );
    }

    #[test]
    fn every_spelling_round_trips_through_its_own_display() {
        for spelling in [
            "cam:integrated-camera",
            "/dev/video0",
            "/dev/v4l/by-id/usb-video-index0",
            "bus:3-4:1.2",
            "usb:04f2:b83c",
            "serial:ABC-123",
        ] {
            let parsed = parse(spelling).expect("the spelling parses");
            assert_eq!(
                parsed.to_string(),
                spelling,
                "{spelling} did not round-trip"
            );
            assert_eq!(
                parse(&parsed.to_string()).expect("the canonical spelling parses"),
                parsed
            );
        }
    }

    #[test]
    fn a_bare_slug_round_trips_as_the_prefixed_form_it_means() {
        // The one spelling that is *not* byte-identical on the way out, stated rather than
        // discovered: `cam:` is optional on input and always present on output, which is
        // `CameraId`'s rule and not this module's to change.
        let parsed = parse("obsbot-tiny-3").expect("a bare slug parses");
        assert_eq!(parsed.to_string(), "cam:obsbot-tiny-3");
    }

    #[test]
    fn a_scheme_this_build_does_not_know_is_refused_naming_the_vocabulary() {
        let refusal = parse("bus_path:3-4:1.2").expect_err("an unknown scheme is refused");
        assert!(
            matches!(&refusal, Error::CameraUnknown { requested } if requested == "bus_path:3-4:1.2")
        );
        let message = refusal.to_string();
        for scheme in SelectorScheme::ALL {
            assert!(
                message.contains(scheme.example()),
                "the refusal does not teach {scheme:?}: {message}"
            );
        }
    }

    #[test]
    fn an_empty_spelling_is_refused_rather_than_matching_everything() {
        // Note **N50**'s wildcard, in the wider grammar: `""` is a prefix of every id.
        assert!(matches!(
            parse(""),
            Err(Error::CameraUnknown { requested }) if requested.is_empty()
        ));
        for empty_body in ["cam:", "bus:", "usb:", "serial:"] {
            assert!(
                parse(empty_body).is_err(),
                "{empty_body} is a scheme with nothing in it and must be refused"
            );
        }
    }

    #[test]
    fn a_usb_pair_is_two_hex_numbers_and_nothing_else() {
        let parsed = parse("usb:04f2:b83c").expect("a well-formed pair parses");
        assert_eq!(
            parsed,
            CameraSelector::UsbId(UsbId {
                vendor: 0x04f2,
                product: 0xb83c
            })
        );
        // Upper case is the same pair; the spelling it prints back is the lower-case one
        // `UsbId::Display` writes, which is the canonical form and the reason this is
        // asserted rather than assumed.
        assert_eq!(
            parse("usb:04F2:B83C").expect("hex is case-insensitive"),
            parsed
        );
        assert_eq!(parsed.to_string(), "usb:04f2:b83c");

        for malformed in [
            "usb:04f2",
            "usb:04f2:",
            "usb::b83c",
            "usb:zzzz:b83c",
            "usb:+4f2:b83c",
            "usb:104f2:b83c",
            "usb:04f2:b83c:extra",
        ] {
            assert!(
                parse(malformed).is_err(),
                "{malformed} is not a vendor:product pair and must be refused"
            );
        }
    }

    #[test]
    fn a_bus_path_keeps_the_colons_that_are_part_of_it() {
        assert_eq!(
            parse("bus:3-4:1.2").expect("a bus path parses"),
            CameraSelector::BusPath("3-4:1.2".to_owned())
        );
    }

    #[test]
    fn a_serial_keeps_whatever_the_device_reported() {
        // Serials are device text, not a slug: colons, spaces and case all survive, because
        // the comparison downstream is against the string the device gave us.
        assert_eq!(
            parse("serial:A:B c").expect("a serial parses"),
            CameraSelector::Serial("A:B c".to_owned())
        );
    }

    #[test]
    fn a_selector_crosses_json_as_the_string_it_was_written_as() {
        for spelling in ["cam:integrated-camera", "/dev/video0", "usb:04f2:b83c"] {
            let parsed = parse(spelling).expect("the spelling parses");
            let json = serde_json::to_string(&parsed).expect("a selector serializes");
            assert_eq!(json, format!("\"{spelling}\""));
            let back: CameraSelector = serde_json::from_str(&json).expect("and deserializes");
            assert_eq!(back, parsed);
        }
    }

    #[test]
    fn the_wire_refuses_a_selector_the_parser_would_refuse() {
        // The half note **N50** was written about: the grammar has to be the same on a
        // command line and off a socket, or the socket is the wider one.
        for raw in ["\"\"", "\"nope:0\"", "\"usb:zzzz:0001\""] {
            assert!(
                serde_json::from_str::<CameraSelector>(raw).is_err(),
                "{raw} reached a handler as a selector"
            );
        }
    }

    #[test]
    fn the_wires_refusal_teaches_the_same_vocabulary_the_parsers_does() {
        // The walk `a_scheme_this_build_does_not_know_is_refused_naming_the_vocabulary` runs
        // over the *parser*'s refusal, on the door off a socket — which is where the sentence
        // used to be a transcription rather than a rendering, and where the only arm asserted
        // `is_err()` and never read what it said (note **N303**). Driven from
        // `SelectorScheme::ALL`, so a sixth scheme is a red test here rather than a wire that
        // quietly teaches five spellings out of six.
        let message = serde_json::from_str::<CameraSelector>("\"nope:0\"")
            .expect_err("an unknown scheme is refused")
            .to_string();
        for scheme in SelectorScheme::ALL {
            assert!(
                message.contains(scheme.example()),
                "the wire's refusal does not teach {scheme:?}: {message}"
            );
        }
    }

    #[test]
    fn each_scheme_says_whether_it_is_an_address_or_an_identity() {
        // The split the guide's table is generated from. Asserted here because the sentence
        // "`NodePath` is an address" is a claim about behaviour under a driver reload
        // \[PF:22\], and a table that drifted from it would teach the opposite.
        assert_eq!(SelectorScheme::NodePath.stability(), Stability::Address);
        assert_eq!(SelectorScheme::Id.stability(), Stability::Address);
        for fingerprint_tier in [
            SelectorScheme::BusPath,
            SelectorScheme::UsbId,
            SelectorScheme::Serial,
        ] {
            assert_eq!(fingerprint_tier.stability(), Stability::Fingerprint);
        }
    }

    #[test]
    fn every_prefixed_scheme_is_recognized_by_the_prefix_it_declares() {
        // The table and the parser are one fact: a scheme whose `prefix()` disagreed with
        // what `parse` dispatches on would be a vocabulary sentence that teaches a spelling
        // the parser refuses. Checked against the samples rather than against an invented
        // body, because three of the five bodies have a grammar of their own.
        let mut prefixed = 0;
        for (scheme, spelling) in samples() {
            let Some(prefix) = scheme.prefix() else {
                assert!(
                    spelling.starts_with('/'),
                    "{spelling} declares no prefix, so it can only be recognized by shape"
                );
                continue;
            };
            prefixed += 1;
            assert!(
                spelling.starts_with(prefix),
                "{spelling} is a {scheme:?} and does not start with the prefix it declares"
            );
        }
        assert_eq!(prefixed, 4, "only a node path is recognized by shape");
    }
}
