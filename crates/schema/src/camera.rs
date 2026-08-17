//! Camera identity, device nodes, and the format tree (design D1).
//!
//! A *camera* is a group of device nodes sharing a USB interface, not a `/dev/videoN`
//! path: one USB device can host several logical cameras, and node numbering says
//! nothing about which nodes belong together \[PF:7\]. Capture nodes are told from
//! metadata nodes by `device_caps`, never by "the lowest-numbered one".

use std::collections::BTreeSet;
use std::fmt;

use camino::Utf8PathBuf;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::slug::{Separator, slugify};
use crate::vocabulary::closed_vocabulary;

/// The name RPC calls and CLI arguments use: `cam:<card-slug>[-<n>]`.
///
/// Reproducible across runs while the attached topology is unchanged, and stable for one
/// engine's lifetime even across replug. Never persisted as identity — that is
/// [`CameraFingerprint`]'s job.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, JsonSchema)]
#[serde(transparent)]
pub struct CameraId(String);

/// Hand-written rather than derived, because a derived one is a wildcard (note **N50**).
///
/// A derived `Deserialize` over a `#[serde(transparent)]` newtype accepts **any** string,
/// `""` included, while [`CameraId::parse`] refuses an empty body — and resolution is a
/// *prefix* match by design ([`resolve_prefix`]: `cam:obsbot` finds `cam:obsbot-tiny-3`),
/// so every id starts with the empty string. On a command line `cli_core::CameraArg::id()`
/// catches it; off a socket nothing did, so `{"camera": ""}` resolved silently to **the
/// only camera** on a one-camera host — on every method that names a camera, `wch_set` and
/// `wch_photo` and the calibrate verbs included. That is a wildcard nobody designed, and the
/// one home for "what is a camera id" is this type's own constructor, so this is where the
/// wire has to go through it.
///
/// The rationale lives on the `impl` rather than on the type, and that is not a style
/// choice: `schemars` publishes a type's doc comment as the `description` of its node in
/// `schemas/webcam-handler-schema.json`, so a paragraph up there would move a committed
/// bundle. This one moves nothing — `JsonSchema` is a separate derive over unchanged
/// attributes and still emits `{"type": "string"}` — which is the claim
/// `scripts/gates/schema-artifacts-current.sh` checks and the reason this half of the debt
/// could land without its own commit.
impl<'de> Deserialize<'de> for CameraId {
    fn deserialize<D>(deserializer: D) -> Result<CameraId, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        // Through the constructor, which is the whole point: a value that reaches a
        // handler is one `CameraId::parse` would have made, so `cam:` stays optional on
        // input and always present on output and an empty body is refused here rather than
        // becoming a prefix of everything two crates later.
        CameraId::parse(&raw).ok_or_else(|| {
            serde::de::Error::invalid_value(
                serde::de::Unexpected::Str(&raw),
                &"a camera id with a body, like `cam:integrated-camera` or `integrated-camera`",
            )
        })
    }
}

/// The prefix every camera id carries, so an id is recognizable in isolation.
pub const CAMERA_ID_PREFIX: &str = "cam:";

impl CameraId {
    /// Build an id from an already-derived slug body (no `cam:` prefix).
    ///
    /// Returns `None` for an empty body — a card name that slugs to nothing gets an
    /// ordinal-only id from [`assign_ids`] instead of a nameless one.
    #[must_use]
    pub fn from_slug(slug: &str) -> Option<Self> {
        if slug.is_empty() {
            None
        } else {
            Some(CameraId(format!("{CAMERA_ID_PREFIX}{slug}")))
        }
    }

    /// Parse a user-supplied id. The `cam:` prefix is optional on input and always
    /// present on output.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        let body = s.strip_prefix(CAMERA_ID_PREFIX).unwrap_or(s);
        Self::from_slug(body)
    }

    /// The full id, prefix included.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The id without its `cam:` prefix.
    #[must_use]
    pub fn body(&self) -> &str {
        self.0.strip_prefix(CAMERA_ID_PREFIX).unwrap_or(&self.0)
    }
}

impl fmt::Display for CameraId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Assign ids to cameras in enumeration order, resolving collisions by D1's rule.
///
/// A natural slug always wins its own name, and a collision ordinal increments until it
/// collides with nothing — natural slugs included. The seed hardware is why the second
/// clause exists: "OBSBOT Tiny 3" slugs to `obsbot-tiny-3`, which looks exactly like a
/// collision form of `obsbot-tiny`, so a second "OBSBOT Tiny" must skip past it.
#[must_use]
pub fn assign_ids(cards: &[String]) -> Vec<CameraId> {
    let naturals: Vec<String> = cards
        .iter()
        .map(|c| slugify(c, Separator::Hyphen))
        .collect();
    let reserved: BTreeSet<&str> = naturals
        .iter()
        .filter(|s| !s.is_empty())
        .map(String::as_str)
        .collect();

    let mut taken: BTreeSet<String> = BTreeSet::new();
    let mut out = Vec::with_capacity(cards.len());
    for (index, natural) in naturals.iter().enumerate() {
        // A card name that slugs to nothing still needs a handle, and `camera-<index>` is
        // derived from something stable (enumeration position). It is **not** out of a
        // natural slug's reach, which this comment used to claim: "Camera 1" slugs to
        // exactly `camera-1`, so the fallback is tested against the reserved naturals as
        // well as against what is already taken. D1 says a natural slug always wins its
        // own name, and between a card that has one and a card that has none, the one
        // that must move is the one with nothing to lose (note **N226**).
        //
        // A natural base is deliberately *not* tested against `reserved`: it is in there,
        // because it put itself there.
        let (base, contest_reserved) = if natural.is_empty() {
            (format!("camera-{index}"), true)
        } else {
            (natural.clone(), false)
        };

        let chosen =
            if taken.contains(&base) || (contest_reserved && reserved.contains(base.as_str())) {
                let mut n = 2u32;
                loop {
                    let candidate = format!("{base}-{n}");
                    if !taken.contains(&candidate) && !reserved.contains(candidate.as_str()) {
                        break candidate;
                    }
                    n += 1;
                }
            } else {
                base
            };
        taken.insert(chosen.clone());
        // The precondition is three branches up and in view: `chosen` is either a natural slug
        // this function has already tested for emptiness, or `camera-<index>`, or one of those
        // with `-<n>` appended. `from_slug` refuses exactly the empty string, so this states a
        // precondition rather than risking a device-derived value — docs/9's `cfg(test)`
        // carve-out, applied to the one site in this file that is that shape. Restructuring it
        // is a real option and a different change: the answer this function owes its caller is
        // one id per card, positionally, so there is no honest total form that does not first
        // decide what a card with no representable handle *is*.
        //
        // Bare, for the reason `pairing.rs` states beside its own (note **N167**).
        #[expect(
            clippy::expect_used,
            reason = "`chosen` is non-empty by the three branches above"
        )]
        out.push(CameraId::from_slug(&chosen).expect("chosen slug is non-empty by construction"));
    }
    out
}

/// How a caller-supplied prefix resolved against the known ids.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrefixMatch {
    /// Exactly one id starts with the prefix.
    Unique(CameraId),
    /// Nothing starts with it.
    None,
    /// Several do — the candidates, so the error can name them.
    Ambiguous(Vec<CameraId>),
}

/// Resolve an unambiguous id prefix (`cam:obsbot` for `cam:obsbot-tiny-3`).
///
/// Derived slugs get long; agents should not have to type them in full. An exact match
/// always wins, so a full id is never ambiguous against a longer one.
#[must_use]
pub fn resolve_prefix(ids: &[CameraId], input: &str) -> PrefixMatch {
    let body = input.strip_prefix(CAMERA_ID_PREFIX).unwrap_or(input);
    if let Some(exact) = ids.iter().find(|id| id.body() == body) {
        return PrefixMatch::Unique(exact.clone());
    }
    let mut matches: Vec<CameraId> = ids
        .iter()
        .filter(|id| id.body().starts_with(body))
        .cloned()
        .collect();
    // `pop` rather than a length check and an `expect`: the two questions "how many matched"
    // and "which one" are then answered by the same value, so the single-match arm has no
    // precondition to state. A `None` from `pop` means the list was empty, which is exactly
    // what `PrefixMatch::None` says — a total match with no unreachable arm to argue about.
    match matches.len() {
        0 => PrefixMatch::None,
        1 => matches.pop().map_or(PrefixMatch::None, PrefixMatch::Unique),
        _ => PrefixMatch::Ambiguous(matches),
    }
}

/// A USB vendor:product pair.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
pub struct UsbId {
    /// Vendor id.
    pub vendor: u16,
    /// Product id.
    pub product: u16,
}

impl fmt::Display for UsbId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04x}:{:04x}", self.vendor, self.product)
    }
}

/// Best-effort identity across replug and reboot.
///
/// Serial numbers are in here but not trusted: the Chicony reports `"0001"` and the
/// OBSBOT reports none at all \[PF:8\]. Matching is conservative — a mismatch on
/// `calibrate apply` is a refusal naming the differing fields, not a warning.
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
pub struct CameraFingerprint {
    /// The USB interface path (`3-1:1.0`) or, for non-USB devices, the driver's bus info.
    pub bus_path: String,
    /// `VID:PID` when the device is on USB.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usb_id: Option<UsbId>,
    /// The `QUERYCAP` card name.
    pub card: String,
    /// The driver name.
    pub driver: String,
    /// The serial, only when the device reports a distinguishing one \[PF:8\].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub serial: Option<String>,
}

impl CameraFingerprint {
    /// A filesystem-safe slug for the session directory layout (design D9).
    #[must_use]
    pub fn slug(&self) -> String {
        let card = slugify(&self.card, Separator::Hyphen);
        let bus = slugify(&self.bus_path, Separator::Hyphen);
        match (card.is_empty(), bus.is_empty()) {
            (false, false) => format!("{card}-{bus}"),
            (false, true) => card,
            (true, false) => bus,
            (true, true) => "unknown-camera".to_owned(),
        }
    }

    /// The fields where `self` and `other` disagree, in a stable order.
    ///
    /// Empty means "these are the same camera as far as we can tell". A `None` serial on
    /// either side is not a disagreement — it is an absence, and PF:8 says absence is
    /// the common case.
    #[must_use]
    pub fn differing_fields(&self, other: &CameraFingerprint) -> Vec<&'static str> {
        let mut out = Vec::new();
        if self.bus_path != other.bus_path {
            out.push("bus_path");
        }
        if self.usb_id != other.usb_id && self.usb_id.is_some() && other.usb_id.is_some() {
            out.push("usb_id");
        }
        if self.card != other.card {
            out.push("card");
        }
        if self.driver != other.driver {
            out.push("driver");
        }
        match (&self.serial, &other.serial) {
            (Some(a), Some(b)) if a != b => out.push("serial"),
            _ => {}
        }
        out
    }

    /// Whether these fingerprints identify the same camera.
    #[must_use]
    pub fn matches(&self, other: &CameraFingerprint) -> bool {
        self.differing_fields(other).is_empty()
    }
}

/// What a device node is for, decided by `device_caps` \[PF:7\].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NodeKind {
    /// `V4L2_CAP_VIDEO_CAPTURE` — frames come from here.
    VideoCapture,
    /// `V4L2_CAP_META_CAPTURE` — recorded, never streamed.
    MetaCapture,
    /// `V4L2_CAP_VIDEO_OUTPUT` — an output node on the same interface.
    VideoOutput,
    /// Something else. Represented, not discarded.
    Other {
        /// The `device_caps` word that produced this classification.
        device_caps: u32,
    },
}

/// `V4L2_CAP_VIDEO_CAPTURE`.
pub const CAP_VIDEO_CAPTURE: u32 = 0x0000_0001;
/// `V4L2_CAP_VIDEO_OUTPUT`.
pub const CAP_VIDEO_OUTPUT: u32 = 0x0000_0002;
/// `V4L2_CAP_META_CAPTURE`.
pub const CAP_META_CAPTURE: u32 = 0x0080_0000;

impl NodeKind {
    /// Classify a node from its `device_caps`.
    #[must_use]
    pub const fn from_device_caps(device_caps: u32) -> Self {
        if device_caps & CAP_VIDEO_CAPTURE != 0 {
            NodeKind::VideoCapture
        } else if device_caps & CAP_META_CAPTURE != 0 {
            NodeKind::MetaCapture
        } else if device_caps & CAP_VIDEO_OUTPUT != 0 {
            NodeKind::VideoOutput
        } else {
            NodeKind::Other { device_caps }
        }
    }
}

/// One `/dev/videoN` node belonging to a camera.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DeviceNode {
    /// The device path.
    #[schemars(with = "String")]
    pub path: Utf8PathBuf,
    /// What it is for.
    pub kind: NodeKind,
    /// The node's own `device_caps`.
    pub device_caps: u32,
    /// The whole-device `capabilities` word, kept because it differs from `device_caps`
    /// on multi-node devices and the difference is the point.
    pub capabilities: u32,
}

/// A camera: a group of nodes, an identity, and the driver's own description of itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CameraInfo {
    /// The id callers use.
    pub id: CameraId,
    /// The identity that survives a replug.
    pub fingerprint: CameraFingerprint,
    /// The `QUERYCAP` card name, verbatim.
    pub card: String,
    /// The driver name.
    pub driver: String,
    /// The driver's bus info string.
    pub bus_info: String,
    /// Every node in the group, capture and metadata alike.
    pub nodes: Vec<DeviceNode>,
    /// Which backend produced this. Recorded so `--json` output is self-describing and
    /// a fake-backend run can never be mistaken for a hardware one.
    pub backend: crate::backend::BackendKind,
}

impl CameraInfo {
    /// The node frames come from: the **first** group member with `VIDEO_CAPTURE`
    /// capabilities, in the node order enumeration produced (numeric, `sysfs.rs`).
    ///
    /// `None` for a group of metadata-only nodes, which is a real shape — the camera is
    /// listed, and streaming it is a typed refusal rather than a surprise.
    ///
    /// **Why "first" rather than "the"** \[PF:19\]: a group may hold more than one capture
    /// node. The Dell U3224KB/A drives two USB Streaming output terminals off one camera
    /// sensor, so `uvcvideo` registers two capture nodes (plus two metadata nodes) against
    /// its single VideoControl interface — one camera, four nodes, two of them streamable.
    /// V4L2 offers nothing that ranks them: `device_caps` is identical, and only opening
    /// each and comparing format trees tells the full-resolution stream from the 640×480
    /// secondary. So the rule is positional and stated rather than implied, and every node
    /// stays listed in [`CameraInfo::nodes`] — nothing is hidden, one is chosen.
    #[must_use]
    pub fn capture_node(&self) -> Option<&DeviceNode> {
        self.nodes.iter().find(|n| n.kind == NodeKind::VideoCapture)
    }

    /// The fields where `self` and `other` describe **different devices**, in a stable
    /// order.
    ///
    /// Empty means "these two descriptions are of the same camera in the same shape" —
    /// the same idiom, and the same sentence, as [`CameraFingerprint::differing_fields`]
    /// one type up. This is the one home (design §2.10) for "does the attached camera
    /// still match its committed profile": both R3 arms that ask it — the enumeration arm
    /// directly, [`crate::profile::DeviceProfile::invariant_matches`] underneath the
    /// capture arm — go through here rather than spelling a comparison twice.
    ///
    /// # `/dev/videoN` is deliberately not compared \[PF:22\]
    ///
    /// Node numbering is probe-order bookkeeping, not a property of the device. Design
    /// §1.2 already says it "is never load-bearing", and PF:7 says a camera is a group of
    /// nodes sharing a USB interface rather than a path — but a `==` over
    /// [`CameraInfo::nodes`] compares [`DeviceNode::path`] along with everything else, so
    /// the rule and the code disagreed.
    ///
    /// Measured 2026-08-11 on this host, after one `uvcvideo` unload/reload of exactly the
    /// kind this project's own R3 hotplug arm performs (evidence E9): three of the four
    /// attached cameras changed every path they own — `chicony-rgb` `video0,1` →
    /// `video2,3`, `chicony-ir` `video2,3` → `video4,5`, `obsbot-tiny3` `video4,5` →
    /// `video0,1`, the Dell unchanged only because it hangs off a different PCI root —
    /// while `card`, `bus_info` and every node's `kind`, `device_caps` and `capabilities`
    /// were identical on both sides. Nothing about any device had changed. The kernel had
    /// renamed three of them.
    ///
    /// **The rejected fix is re-capturing the profiles**, which is the obvious one and is
    /// wrong: it bakes today's arbitrary numbering into the corpus, and the next reload —
    /// which this project performs *deliberately*, as R3 evidence — breaks it again. The
    /// corpus is not stale. The comparison was. Note **N63**.
    ///
    /// The path stays in the schema regardless: it is capture-time provenance, and the
    /// fake backend replays it, which is a statement about the machine the profile came
    /// from and not a claim about this one.
    ///
    /// # What still has to match, because a device really can change shape
    ///
    /// The node **count**, and per node the `kind`, `device_caps` and `capabilities` —
    /// everything that says what the node *is*. A camera that grew a node, lost one, or
    /// reclassified one goes red here, which is what makes PF:19's four-node Dell and
    /// PF:7's two-logical-cameras-per-USB-device checkable at all. Dropping the node set
    /// rather than the paths would have made the R3 arm green and worthless.
    ///
    /// A count mismatch reports `nodes.len` and stops there rather than also walking the
    /// pairs: one inserted node shifts every later index, and a screenful of consequent
    /// mismatches buries the one fact that explains them.
    ///
    /// # [`CameraId`] is excluded, and for a neighbouring reason
    ///
    /// An id is derived from the card names of **every** camera attached at enumeration
    /// time: [`assign_ids`] hands out collision ordinals, so a second identically-named
    /// camera decides whether this one is `cam:webcam` or `cam:webcam-2`, and which of the
    /// two gets the ordinal follows enumeration order — which is node order, which is the
    /// thing that just moved. `CameraId`'s own documentation says it is "never persisted
    /// as identity". Comparing it against a corpus entry captured on some other day's
    /// topology is therefore the same defect in a second costume, and it costs nothing to
    /// drop, because the only device-derived input to an id is `card`, which is compared
    /// right here. Unlike the path finding this one is *unmeasured* — no two attached
    /// cameras collide on this host — so it is named rather than claimed.
    ///
    /// # Why this is a test defect and not a data-loss bug
    ///
    /// [`CameraFingerprint`] — which keys D9's session directories through
    /// [`CameraFingerprint::slug`] — is `bus_path`/`usb_id`/`card`/`driver`/`serial` and
    /// holds no node path at all. A calibration session recorded before a reload still
    /// finds its camera afterwards. Nothing persisted was ever keyed on the number.
    #[must_use]
    pub fn differing_fields(&self, other: &CameraInfo) -> Vec<String> {
        // Destructured rather than field-accessed so that this function stops compiling
        // the day `CameraInfo` grows a field: a new field is a decision about which half
        // of this split it belongs to, and the compiler is the only thing that reliably
        // asks for it. `id` here and `DeviceNode::path` in the node loop are bound to `_`
        // rather than omitted, precisely so the two exclusions are visible in the code
        // instead of being inferred from what is missing from it.
        let CameraInfo {
            id: _,
            fingerprint,
            card,
            driver,
            bus_info,
            nodes,
            backend,
        } = self;

        let mut out = Vec::new();
        // Reused rather than re-derived, so PF:8's rule that an absent serial is not a
        // disagreement holds in both comparisons at once.
        out.extend(
            fingerprint
                .differing_fields(&other.fingerprint)
                .into_iter()
                .map(|field| format!("fingerprint.{field}")),
        );
        if *card != other.card {
            out.push("card".to_owned());
        }
        if *driver != other.driver {
            out.push("driver".to_owned());
        }
        if *bus_info != other.bus_info {
            out.push("bus_info".to_owned());
        }
        if nodes.len() == other.nodes.len() {
            for (index, (mine, theirs)) in nodes.iter().zip(other.nodes.iter()).enumerate() {
                let DeviceNode {
                    path: _,
                    kind,
                    device_caps,
                    capabilities,
                } = mine;
                if *kind != theirs.kind {
                    out.push(format!("nodes[{index}].kind"));
                }
                if *device_caps != theirs.device_caps {
                    out.push(format!("nodes[{index}].device_caps"));
                }
                if *capabilities != theirs.capabilities {
                    out.push(format!("nodes[{index}].capabilities"));
                }
            }
        } else {
            out.push("nodes.len".to_owned());
        }
        if *backend != other.backend {
            out.push("backend".to_owned());
        }
        out
    }

    /// Whether these two descriptions are of the same camera in the same shape.
    ///
    /// [`CameraInfo::differing_fields`] carries the argument for what "the same" means
    /// here and what it deliberately ignores.
    #[must_use]
    pub fn describes_same_device(&self, other: &CameraInfo) -> bool {
        self.differing_fields(other).is_empty()
    }
}

/// A pixel format, as its FourCC — and on every wire this project has, **as the four
/// characters rather than as four numbers** (owner ruling, 2026-08-14; note **N109**).
///
/// The four bytes are the kernel's, so the Rust value is `[u8; 4]` and stays that way: it is
/// what `v4l2_fmtdesc::pixelformat` decodes to and what `VIDIOC_S_FMT` takes back. What the
/// ruling settles is the *spelling* every consumer outside this process reads. The type used
/// to be `#[serde(transparent)]` over the array, so an agent asking why its photo was refused
/// was answered `{"available": [[77,74,80,71],[89,85,89,86]]}` while the CLI's table and the
/// D13 message beside it said `MJPG, YUYV` — three renderings of one fact, and the one the
/// primary consumer reads was the unreadable one. AGENTS' opening section makes that a defect
/// rather than a cosmetic complaint: the agent has no hands, and `FormatUnsupported
/// { requested, available }` is a refusal it is supposed to *act* on.
///
/// # The spelling, and the ambiguity it had to fix first
///
/// A FourCC is ASCII by kernel convention and a driver is input, so a byte that is not an
/// ASCII graphic is carried rather than corrected (AGENTS rule 6) — it is escaped `\xNN`, as
/// [`fmt::Display`] has always done. **That escape alone is not a wire encoding**, because it
/// is not injective: a device reporting the four literal bytes `\`, `x`, `4`, `d` spelled
/// `\x4d`, which is byte for byte what the same function emits for the single byte `0x4d`
/// (`M`). Two values, one spelling, and a wire form that cannot round-trip is worse than the
/// array it replaces. So the escape escapes itself:
///
/// | Byte | Spelling |
/// |---|---|
/// | `0x5c` (`\`) | `\\` |
/// | ASCII graphic (`0x21`–`0x7e`), other than `\` | itself |
/// | anything else — `0x00`, `0x20`, `0x7f`, `0xff` | `\xNN`, two lowercase hex digits |
///
/// This *changes* `Display`'s output for a backslash-bearing FourCC, from `\x4d` to `\\x4d`.
/// That is the repair, not a regression: the old rendering named a value it did not mean.
///
/// # One home for it
///
/// The encoder is [`fmt::Display`] and the decoder is [`PixelFormat::parse`], and they are the
/// only two. `Serialize` is `collect_str`, `Deserialize` is `parse`, the `JsonSchema`
/// description below is written from the same table, and `cli_core`'s `--pixel-format` flag
/// parses with [`PixelFormat::parse`] rather than spelling a FourCC itself — because a flag
/// and a wire that disagreed about what a format is called is the same defect as the tables
/// and the JSON disagreeing, one layer down. [`fmt::Debug`] goes through `Display` too, so a
/// panic message, a `tracing` field and a test failure name the format the way a human reads
/// it; the derived one printed the array, which is how that spelling reached a transcript in
/// the notes.
///
/// The two are a **bijection over all 2^32 values**, and that is asserted rather than
/// believed: `every_four_byte_value_survives_the_wire_spelling_that_names_it` round-trips a
/// generated population including `0x00`, `0x5c`, `0x7f` and `0xff`, and
/// `two_different_fourccs_cannot_share_one_spelling` is the injectivity half — it is red
/// against exactly the pre-repair encoder.
///
/// # What it refuses
///
/// [`PixelFormat::parse`] answers `None` for anything that does not spell exactly four bytes:
/// a three-character name, a fifth character, a `\` that begins no escape, `\x` without two
/// hex digits, and any non-ASCII character (a `\xNN` escape is how a byte above `0x7f`
/// travels — the raw byte is not valid UTF-8 to begin with). A raw space decodes to `0x20`
/// even though the encoder always writes `\x20` for it, so a FourCC copied out of `v4l2-ctl`
/// with its kernel padding (`Y16 `) still parses; the canonical spelling is still the escaped
/// one, because a trailing space survives JSON but not a shell word, a table column or a
/// reader's eye.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PixelFormat(
    /// The four characters, e.g. `MJPG`.
    pub [u8; 4],
);

impl PixelFormat {
    /// Motion JPEG — the format photos pass through verbatim (E6).
    pub const MJPG: PixelFormat = PixelFormat(*b"MJPG");
    /// Packed YUV 4:2:2.
    pub const YUYV: PixelFormat = PixelFormat(*b"YUYV");
    /// 8-bit greyscale — the Chicony IR camera's only format.
    pub const GREY: PixelFormat = PixelFormat(*b"GREY");
    /// Planar YUV 4:2:0 with interleaved chroma.
    pub const NV12: PixelFormat = PixelFormat(*b"NV12");
    /// JPEG, as distinct from MJPG in the kernel's vocabulary.
    pub const JPEG: PixelFormat = PixelFormat(*b"JPEG");

    /// Decode a `v4l2_fmtdesc::pixelformat` word (little-endian FourCC).
    #[must_use]
    pub const fn from_fourcc(raw: u32) -> Self {
        PixelFormat(raw.to_le_bytes())
    }

    /// The kernel's word for this format.
    #[must_use]
    pub const fn to_fourcc(self) -> u32 {
        u32::from_le_bytes(self.0)
    }

    /// Decode a format name — **the one decoder**, and the inverse of [`fmt::Display`].
    ///
    /// Every caller that turns text into a format goes through here: `Deserialize` off the
    /// JSON-RPC wire and out of a committed device profile, and `cli_core`'s
    /// `--pixel-format` flag. The type's doc carries the grammar, the argument for the
    /// escapes and the list of what this refuses; the short version is that it accepts what
    /// `Display` writes, plus a raw space where `Display` writes `\x20`, and nothing else.
    ///
    /// `None` rather than a typed error, deliberately and for [`CameraId`]'s reason: the two
    /// callers that must explain a refusal already have a vocabulary for it — serde says
    /// `invalid_value` with the expectation this crate writes, clap says it with the usage
    /// line — and a third error enum in between would be a spelling of "not a FourCC" that
    /// neither of them prints.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        let mut bytes = [0u8; 4];
        // Filled through an iterator rather than an index: the fifth byte of a too-long
        // spelling is refused by `next()` returning `None`, which is the same answer as
        // every other refusal here rather than a bounds check somebody has to remember.
        let mut sink = bytes.iter_mut();
        let mut rest = s.as_bytes();
        while let [first, tail @ ..] = rest {
            let (byte, width) = match (*first, tail) {
                (b'\\', [b'\\', ..]) => (b'\\', 2),
                (b'\\', [b'x', high, low, ..]) => (hex_byte(*high, *low)?, 4),
                // A backslash that begins no escape. Refused rather than taken literally,
                // because taking it literally is the ambiguity this grammar exists to close.
                (b'\\', _) => return None,
                // A raw byte stands for itself only where `Display` would have written it
                // raw — an ASCII graphic — plus the space, which is the one alias the doc
                // above argues for. Everything else is refused rather than taken, and this
                // walks *bytes*: a `é` is two of them, so a decoder that took each raw byte
                // it met accepted `éé` as a four-byte FourCC no device can have and no
                // encoder here would ever write (the G6 review's L4, note **N227**). The
                // same arm is what refuses a raw `\x7f` or a raw NUL, both of which have a
                // `\xNN` spelling and neither of which has a literal one.
                (0x20..=0x7e, _) => (*first, 1),
                (_, _) => return None,
            };
            *sink.next()? = byte;
            rest = rest.get(width..)?;
        }
        // Exactly four, so a three-character name is refused here rather than becoming a
        // format with a zero byte on the end.
        sink.next().is_none().then_some(PixelFormat(bytes))
    }

    /// Whether frames in this format are a compressed bitstream we pass through rather
    /// than pixels we interpret.
    #[must_use]
    pub fn is_compressed(self) -> bool {
        self == PixelFormat::MJPG || self == PixelFormat::JPEG
    }
}

/// One `\xNN` payload, from its two hex digits.
///
/// Both cases accepted on input while [`fmt::Display`] only ever writes lowercase: a
/// document written by hand is still a document this build must read, and accepting `\xFF`
/// costs nothing that the round trip proves — the canonical spelling is what comes *out*.
fn hex_byte(high: u8, low: u8) -> Option<u8> {
    let value = char::from(high).to_digit(16)? * 16 + char::from(low).to_digit(16)?;
    u8::try_from(value).ok()
}

impl fmt::Display for PixelFormat {
    /// **The one encoder** — the type's doc carries the table and the argument.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for &b in &self.0 {
            // Formats are ASCII by kernel convention, but a driver is input: print the
            // byte rather than assume it. The backslash goes first because it is the
            // escape's own character, and an escape that does not escape itself spells two
            // different formats the same way (note **N109**).
            match b {
                b'\\' => f.write_str("\\\\")?,
                graphic if graphic.is_ascii_graphic() => write!(f, "{}", char::from(graphic))?,
                other => write!(f, "\\x{other:02x}")?,
            }
        }
        Ok(())
    }
}

/// Through [`fmt::Display`], so a panic message and a `tracing` field name the format the
/// way the wire and the CLI's tables do.
///
/// The derived one printed `PixelFormat([77, 74, 80, 71])`, which is the spelling the
/// 2026-08-14 ruling removed from the wire — and it had already leaked out of a `Debug` into
/// a mutation transcript in the notes, which is the evidence that a second spelling does not
/// stay where it was put.
impl fmt::Debug for PixelFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PixelFormat(\"{self}\")")
    }
}

/// `collect_str`, so the wire form is literally [`fmt::Display`] rather than a copy of it.
impl Serialize for PixelFormat {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.collect_str(self)
    }
}

/// Through [`PixelFormat::parse`], for [`CameraId`]'s reason: a value that reaches a handler
/// is one the decoder made, so the wire cannot carry a format the CLI would refuse to name.
impl<'de> Deserialize<'de> for PixelFormat {
    fn deserialize<D>(deserializer: D) -> std::result::Result<PixelFormat, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        PixelFormat::parse(&raw).ok_or_else(|| {
            serde::de::Error::invalid_value(
                serde::de::Unexpected::Str(&raw),
                &"a four-character FourCC such as `MJPG`, with `\\\\` for a backslash and \
                  `\\xNN` for a byte that is not an ASCII graphic",
            )
        })
    }
}

/// Hand-written because the derived one publishes the *Rust* shape — an array of four
/// integers with `minItems`/`maxItems` — and that shape is exactly what the 2026-08-14 ruling
/// took off the wire.
///
/// The `description` is not decoration here. `schemas/webcam-handler-openrpc.json` is what an
/// agent reads to learn this API without reading its source, and a bare `{"type": "string"}`
/// would leave the escapes to be discovered from a value that happened to carry one. The
/// bounds are derived from the grammar rather than guessed: four bytes at one character each
/// is the shortest spelling, four `\xNN` escapes at four characters each is the longest. A
/// `pattern` is deliberately **not** emitted — it would be a second home for the grammar
/// (design §2.10), free to drift from [`PixelFormat::parse`], and a schema that accepted a
/// string this build refuses is worse than one that says less.
impl JsonSchema for PixelFormat {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "PixelFormat".into()
    }

    fn schema_id() -> std::borrow::Cow<'static, str> {
        concat!(module_path!(), "::PixelFormat").into()
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "string",
            "description": "A pixel format, as the four-character FourCC the driver \
                            enumerated: `MJPG`, `YUYV`, `GREY`, `NV12`, `JPEG`, or one this \
                            build has never heard of, which is carried rather than \
                            rejected. A byte that is not an ASCII graphic character is \
                            escaped as `\\xNN` with two lowercase hex digits, and a literal \
                            backslash as `\\\\`, so a FourCC that is not printable crosses \
                            the wire exactly and decodes back to the same four bytes.",
            "minLength": 4,
            "maxLength": 16,
        })
    }
}

/// How much of what the sensor produced a pixel format keeps (design D5, amended
/// 2026-08-13).
///
/// **A fact about a format, not about a device**, which is why it lives here beside
/// [`PixelFormat`] rather than in a backend: the same FourCC means the same thing on every
/// camera, and a second copy of this table would let the fake and the hardware disagree
/// about which of two modes is the better photograph.
///
/// The owner's ruling of 2026-08-13 asks for "less lossy encodings are preferred to more
/// lossy encodings", and that sentence needs a scale to be a rule.
/// [`StreamRequest::choose`](crate::capture::StreamRequest::choose) is the one consumer.
///
/// [`Lossiness::Unknown`] is AGENTS rule 6's fallback arm and it **carries its payload**:
/// a FourCC this build has never heard of is data, not a surprise, and it is ranked rather
/// than dropped — see [`Lossiness::rank_for`] for where it lands and why.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Lossiness {
    /// Every sample the sensor handed the driver, at full rate.
    ///
    /// `GREY` is this project's instance: an IR sensor's luma has no chroma to subsample
    /// and no quantiser in front of it.
    Lossless,
    /// Luma at full rate, chroma at a fraction of it.
    ///
    /// The fraction is carried rather than named, because the two members of this class
    /// this project has met differ *only* in it and the difference is the whole tiebreak:
    /// `YUYV` is 4:2:2 and keeps half the chroma samples, `NV12` is 4:2:0 and keeps a
    /// quarter [PF:26's Dell].
    ChromaSubsampled {
        /// Chroma samples kept, as a percentage of the luma rate: 50 for 4:2:2, 25 for
        /// 4:2:0.
        chroma_percent: u8,
    },
    /// A quantised bitstream — `MJPG` and `JPEG`, the two spellings of the same thing.
    ///
    /// Where this ranks depends on where the frames are going, which is the only place in
    /// this vocabulary the destination gets a vote: see [`Lossiness::rank_for`].
    Compressed,
    /// A FourCC this build has never heard of, carried whole.
    Unknown {
        /// The four characters the driver enumerated, preserved exactly.
        fourcc: PixelFormat,
    },
}

closed_vocabulary! {
    /// What the destination these frames are going to can do with them (design D5, amended
    /// 2026-08-13).
    ///
    /// One bit, and it is the one bit the re-ranking's tiebreak cannot be written without.
    /// Two formats tied on resolution are not tied on *fidelity to the device under test*,
    /// and which of them wins inverts with the destination:
    ///
    /// - a JPEG destination takes an `MJPG` frame **byte for byte** (E6), so a compressed
    ///   format is the only one that arrives with nothing this program did in it — and the
    ///   uncompressed candidate would have to be run through our encoder, inserting exactly
    ///   the artefacts Expected usage item 3 says fabricate evidence in a test;
    /// - a PNG or PPM destination encodes losslessly whatever arrives, so an uncompressed
    ///   format arrives with the sensor's own samples intact and the compressed candidate
    ///   arrives having already been through the camera's quantiser.
    ///
    /// So "less lossy" is measured over the whole path from sensor to file rather than over
    /// the driver's buffer alone, and that is what makes one rule out of two answers. The
    /// two answers are a closed vocabulary with an `ALL`, so a test walks the destinations
    /// rather than naming the ones it remembers. The map from a
    /// [`PhotoFormat`](crate::capture::PhotoFormat) to one of these lives beside the
    /// encodings it reads rather than here.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    pub enum SinkFidelity {
        /// A JPEG file, an MJPEG preview, an MJPEG-in-AVI recording: a camera bitstream
        /// reaches it unmodified.
        ///
        /// The default, because it is what every destination in this build that can carry
        /// a camera's own bytes is, and because it is the answer that never adds an encode
        /// the camera did not already perform.
        #[default]
        PassesCompressedThrough,
        /// A PNG or PPM file: whatever pixels arrive are written without further loss, and
        /// a compressed frame has to be decoded to get there.
        EncodesLosslessly,
    }
}

impl Lossiness {
    /// Classify a FourCC.
    ///
    /// The five arms are the five formats [`PixelFormat`] names, which are also exactly
    /// the five `imaging::decode::SourceFormat` can turn into pixels (design D6 closes that
    /// set). That coincidence is load-bearing rather than lucky — the ranking below sorts
    /// what it cannot classify to the back precisely because it cannot decode it either —
    /// so `imaging` asserts it from its own side rather than leaving it to be noticed.
    #[must_use]
    pub fn of(format: PixelFormat) -> Self {
        match format {
            PixelFormat::MJPG | PixelFormat::JPEG => Lossiness::Compressed,
            PixelFormat::YUYV => Lossiness::ChromaSubsampled { chroma_percent: 50 },
            PixelFormat::NV12 => Lossiness::ChromaSubsampled { chroma_percent: 25 },
            PixelFormat::GREY => Lossiness::Lossless,
            // AGENTS rule 6: a `match` on device vocabulary has a payload-carrying
            // fallback arm. The FourCC comes back out, so a caller — or a panic message
            // that never happens — can say *which* format it was.
            other => Lossiness::Unknown { fourcc: other },
        }
    }

    /// Where this sits on the preference scale for a given destination: **higher keeps
    /// more of what the sensor produced**, once the destination has had its say.
    ///
    /// The pair is `(class, chroma)`. The second half only ever discriminates *within*
    /// [`Lossiness::ChromaSubsampled`] — two members of any other class have the same
    /// class rank and the same filler — which is what lets one comparable value order both
    /// "4:2:2 beats 4:2:0" and "compressed beats subsampled, or does not, depending on
    /// where these frames are going".
    ///
    /// [`Lossiness::Unknown`] ranks **last in both columns**, and not because an
    /// unrecognised format is bad: it is because nothing here can say it is good. A format
    /// this build cannot classify is a format `imaging::decode` cannot decode, so
    /// preferring it would trade a photograph for a
    /// [`FormatUnsupported`](crate::Error::FormatUnsupported) — and the ruling is about
    /// which photograph is better, not which mode number is biggest. It is ranked rather
    /// than filtered so that a device offering nothing else still resolves to something
    /// deterministic instead of to `None`.
    ///
    /// The match is over both vocabularies at once so the compiler owns the table: a sixth
    /// format or a third destination cannot be added without an arm here.
    #[must_use]
    pub const fn rank_for(self, sink: SinkFidelity) -> (u8, u8) {
        /// The filler for the classes where chroma does not discriminate.
        const WHOLE: u8 = 100;
        match (self, sink) {
            (Lossiness::Unknown { .. }, _) => (0, 0),
            // The verbatim path (E6): nothing this program did is in the answer.
            (Lossiness::Compressed, SinkFidelity::PassesCompressedThrough) => (3, WHOLE),
            (Lossiness::Lossless, SinkFidelity::PassesCompressedThrough) => (2, WHOLE),
            (
                Lossiness::ChromaSubsampled { chroma_percent },
                SinkFidelity::PassesCompressedThrough,
            ) => (1, chroma_percent),
            // The lossless-encode path: the camera's quantiser is the only loss that
            // cannot be undone, so the format that never met one wins.
            (Lossiness::Lossless, SinkFidelity::EncodesLosslessly) => (3, WHOLE),
            (Lossiness::ChromaSubsampled { chroma_percent }, SinkFidelity::EncodesLosslessly) => {
                (2, chroma_percent)
            }
            (Lossiness::Compressed, SinkFidelity::EncodesLosslessly) => (1, WHOLE),
        }
    }

    /// Whether this build could name the format at all.
    ///
    /// The ranking's first question, ahead of resolution — see [`Lossiness::rank_for`] for
    /// the argument.
    #[must_use]
    pub const fn is_named(self) -> bool {
        !matches!(self, Lossiness::Unknown { .. })
    }
}

/// A frame size the device offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FrameSize {
    /// One exact size.
    Discrete {
        /// Width in pixels.
        width: u32,
        /// Height in pixels.
        height: u32,
    },
    /// A range with a step.
    Stepwise {
        /// Smallest width.
        min_width: u32,
        /// Largest width.
        max_width: u32,
        /// Width step.
        step_width: u32,
        /// Smallest height.
        min_height: u32,
        /// Largest height.
        max_height: u32,
        /// Height step.
        step_height: u32,
    },
    /// A size described with a `v4l2_frmsizetypes` value this build cannot interpret.
    ///
    /// D2's rule, applied to sizes: the entry exists, so it is carried. Dropping it would
    /// make a *short* size list look like a complete one, and "this camera does not offer
    /// 4K" is exactly the capability claim we would be inventing.
    Unknown {
        /// The kernel's `type` discriminant, preserved exactly.
        raw: u32,
    },
}

impl FrameSize {
    /// The largest size this entry offers, as `(width, height)`.
    ///
    /// `None` for a size whose shape this build cannot read: guessing dimensions for it
    /// would be inventing the very fact we are admitting we do not have.
    #[must_use]
    pub const fn max_dimensions(&self) -> Option<(u32, u32)> {
        match *self {
            FrameSize::Discrete { width, height } => Some((width, height)),
            FrameSize::Stepwise {
                max_width,
                max_height,
                ..
            } => Some((max_width, max_height)),
            FrameSize::Unknown { .. } => None,
        }
    }

    /// Whether this build can interpret the shape the driver described.
    #[must_use]
    pub const fn is_interpretable(&self) -> bool {
        !matches!(*self, FrameSize::Unknown { .. })
    }

    /// The size this entry offers nearest to `(width, height)` without exceeding it, or
    /// `None` when the entry offers nothing that small.
    ///
    /// A discrete entry offers one size and either fits or does not. A **stepwise** entry
    /// offers a whole range, and treating it as its maximum alone — which
    /// [`FrameSize::max_dimensions`] does, correctly, for the question *it* answers — makes
    /// a device that can deliver 640×480 exactly report 1920×1080 and call it an
    /// adjustment. No camera this project has met is stepwise, which is why nothing
    /// noticed: the two Chicony nodes, the OBSBOT and the Dell U3224KB/A enumerate
    /// discrete sizes and nothing else — the 34 size entries in `corpus/profiles/`, plus
    /// the Dell's second capture node checked directly (2026-08-09), every one
    /// `V4L2_FRMSIZE_TYPE_DISCRETE`. The branch below is still exercised only by fixtures.
    ///
    /// The result is rounded *down* to the entry's step and clamped to its minimum, because
    /// a driver takes the grid it declared and reporting an off-grid size as agreed would
    /// be a claim the next `S_FMT` contradicts.
    #[must_use]
    pub fn largest_within(&self, width: u32, height: u32) -> Option<(u32, u32)> {
        match *self {
            FrameSize::Discrete {
                width: w,
                height: h,
            } => (w <= width && h <= height).then_some((w, h)),
            FrameSize::Stepwise {
                min_width,
                max_width,
                step_width,
                min_height,
                max_height,
                step_height,
            } => {
                if min_width > width || min_height > height {
                    return None;
                }
                Some((
                    align_down_within(width.min(max_width), min_width, step_width),
                    align_down_within(height.min(max_height), min_height, step_height),
                ))
            }
            FrameSize::Unknown { .. } => None,
        }
    }
}

/// `value` rounded down to a whole `step` counted from `min`, never below `min`.
///
/// A zero or absent step means the driver declared no grid, so the value stands.
fn align_down_within(value: u32, min: u32, step: u32) -> u32 {
    if step <= 1 || value <= min {
        return value.max(min);
    }
    min.saturating_add((value - min) / step * step)
}

/// A frame interval (the reciprocal of a frame rate — the kernel's own convention).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FrameInterval {
    /// One exact interval, as a fraction of a second.
    Discrete {
        /// Numerator.
        numerator: u32,
        /// Denominator.
        denominator: u32,
    },
    /// A range with a step.
    Stepwise {
        /// Shortest interval numerator.
        min_numerator: u32,
        /// Shortest interval denominator.
        min_denominator: u32,
        /// Longest interval numerator.
        max_numerator: u32,
        /// Longest interval denominator.
        max_denominator: u32,
    },
    /// An interval described with a `v4l2_frmivaltypes` value this build cannot interpret.
    /// Carried for [`FrameSize::Unknown`]'s reason.
    Unknown {
        /// The kernel's `type` discriminant, preserved exactly.
        raw: u32,
    },
    /// The device names no interval at all: it cleared `V4L2_CAP_TIMEPERFRAME` in its
    /// `G_PARM` reply, which is the driver saying frame-interval negotiation is not
    /// something this node does.
    ///
    /// **The fourth answer, and it exists because the third was being used as one.** The
    /// `unknown` kind carries *the kernel's own `type` discriminant*, and a device that
    /// named no shape has no discriminant to carry — the `{"kind": "unknown", "raw": 0}`
    /// the V4L2 backend used to produce here was a number this build invented, and `0` is
    /// not even a spare value: it is what a driver that filled in nothing would write.
    /// D2 represents what it cannot interpret; manufacturing the evidence for it is the
    /// one thing D2 forbids.
    ///
    /// **This means the bit and nothing else.** A driver that *sets* the bit and then
    /// writes a fraction with a zero in it has said something, and what it said is carried
    /// as a `discrete` interval holding that fraction — a rate nobody can compute, which
    /// `fps` answers `null` for. Reporting that as `unstated` would be a second invented
    /// claim about the device where the first one was removed.
    ///
    /// Distinct from an *absent* interval, which this vocabulary has no spelling for and
    /// does not need: a negotiated stream always reports one, and this is what it reports
    /// when the device would not say. There is no rate here — the same as `unknown` — so a
    /// consumer that wants frames per second has to handle both.
    ///
    /// **As a request** it means the caller stated no interval, exactly as omitting the
    /// field does; the negotiation reads back whatever the device is running at. It is not
    /// a request for "no interval", because there is no such thing to ask a device for.
    Unstated,
}

impl FrameInterval {
    /// Frames per second, for display. `None` when the interval is degenerate.
    #[must_use]
    pub fn fps(&self) -> Option<f64> {
        match *self {
            FrameInterval::Discrete {
                numerator,
                denominator,
            } => {
                // **Both** halves, and the second one matters more than it looks. A
                // driver that sets `V4L2_CAP_TIMEPERFRAME` and writes `1/0` is carried
                // here verbatim (D2), and `denominator / numerator` for it is `0.0` — a
                // rate nothing measured, arriving as a number a caller would print. The
                // fraction is the device's; the *rate* is an interpretation, and there
                // isn't one (note **N199**).
                if numerator == 0 || denominator == 0 {
                    None
                } else {
                    Some(f64::from(denominator) / f64::from(numerator))
                }
            }
            FrameInterval::Stepwise { .. }
            | FrameInterval::Unknown { .. }
            | FrameInterval::Unstated => None,
        }
    }
}

/// A frame size together with the intervals available *at that size*.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct FrameSizeInfo {
    /// The size.
    pub size: FrameSize,
    /// The intervals offered at it.
    pub intervals: Vec<FrameInterval>,
}

/// One pixel format and everything available in it.
///
/// Nesting is load-bearing: the OBSBOT offers MJPG well past where YUYV stops on the same
/// cable — 3840×2160 against 640×480 when PF:9 measured it, 1920×1440 against 640×480
/// today — so a flat size list would be a lie \[PF:9\]. The numbers moved because the
/// device stopped advertising its 4K mode and the corpus was re-captured \[PF:23\]; the
/// gap they illustrate is what the nesting is for, and that did not.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct FormatInfo {
    /// The format.
    pub pixel_format: PixelFormat,
    /// The driver's description string.
    pub description: String,
    /// The `v4l2_fmtdesc` flags word.
    pub flags: u32,
    /// Sizes, each with its own intervals.
    pub sizes: Vec<FrameSizeInfo>,
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    fn cards(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| (*s).to_owned()).collect()
    }

    fn bodies(ids: &[CameraId]) -> Vec<&str> {
        ids.iter().map(CameraId::body).collect()
    }

    #[test]
    fn distinct_cards_get_their_natural_slugs() {
        let ids = assign_ids(&cards(&[
            "Integrated Camera: Integrated C",
            "OBSBOT Tiny 3",
        ]));
        assert_eq!(
            bodies(&ids),
            vec!["integrated-camera-integrated-c", "obsbot-tiny-3"]
        );
    }

    #[test]
    fn collisions_take_ordinals_starting_at_two() {
        let ids = assign_ids(&cards(&["Webcam", "Webcam", "Webcam"]));
        assert_eq!(bodies(&ids), vec!["webcam", "webcam-2", "webcam-3"]);
    }

    #[test]
    fn a_natural_slug_wins_its_own_name_even_when_a_collision_wants_it() {
        // The seed-hardware ambiguity D1 closes by rule: `obsbot-tiny-3` is somebody's
        // real name, so the second "OBSBOT Tiny" must skip over it.
        let ids = assign_ids(&cards(&["OBSBOT Tiny", "OBSBOT Tiny", "OBSBOT Tiny 3"]));
        assert_eq!(
            bodies(&ids),
            vec!["obsbot-tiny", "obsbot-tiny-2", "obsbot-tiny-3"]
        );

        // And with the natural-slug camera enumerated first, the ordinal must still skip
        // past it rather than steal it.
        let ids = assign_ids(&cards(&[
            "OBSBOT Tiny 3",
            "OBSBOT Tiny",
            "OBSBOT Tiny",
            "OBSBOT Tiny",
        ]));
        assert_eq!(
            bodies(&ids),
            vec![
                "obsbot-tiny-3",
                "obsbot-tiny",
                "obsbot-tiny-2",
                "obsbot-tiny-4"
            ]
        );
    }

    #[test]
    fn a_nameless_card_still_gets_a_handle() {
        let ids = assign_ids(&cards(&["???", "Webcam"]));
        assert_eq!(bodies(&ids), vec!["camera-0", "webcam"]);
    }

    #[test]
    fn a_card_that_slugs_to_nothing_does_not_take_the_slug_another_card_earned() {
        // D1's rule, in the direction `a_natural_slug_wins_its_own_name_even_when_a
        // _collision_wants_it` does not reach: the handle invented for a nameless card is
        // `camera-<index>`, and that is a *natural* slug too — "Camera 1" slugs to exactly
        // it. Enumerate the nameless one at index 1 and it arrives first, so before this
        // was checked the camera that has the name got `camera-1-2` and the one with no
        // name got `camera-1`, which is D1 inverted.
        //
        // Declared, not measured: no device this project has seen reports a card name that
        // slugs to nothing, and none reports "Camera 1" either. The rule is what is under
        // test, and a fixture built only from cards we have seen cannot state it.
        let ids = assign_ids(&cards(&["Webcam", "???", "Camera 1"]));
        assert_eq!(bodies(&ids), vec!["webcam", "camera-1-2", "camera-1"]);

        // The other order, where the natural slug is taken before the fallback is invented,
        // has always worked — it goes through `taken` — and is here so the two halves of
        // the check are visible together.
        let ids = assign_ids(&cards(&["Camera 1", "???"]));
        assert_eq!(bodies(&ids), vec!["camera-1", "camera-1-2"]);
    }

    #[test]
    fn prefixes_resolve_when_unambiguous_and_report_candidates_when_not() {
        let ids = assign_ids(&cards(&[
            "OBSBOT Tiny 3",
            "Integrated Camera: Integrated C",
        ]));
        assert_eq!(
            resolve_prefix(&ids, "cam:obsbot"),
            PrefixMatch::Unique(ids[0].clone())
        );
        assert_eq!(
            resolve_prefix(&ids, "obsbot"),
            PrefixMatch::Unique(ids[0].clone())
        );
        assert_eq!(resolve_prefix(&ids, "cam:nope"), PrefixMatch::None);

        let ids = assign_ids(&cards(&["Webcam", "Webcam"]));
        match resolve_prefix(&ids, "cam:web") {
            PrefixMatch::Ambiguous(c) => assert_eq!(c.len(), 2),
            other => panic!("expected ambiguity, got {other:?}"),
        }
        // An exact match beats being a prefix of a longer id.
        assert_eq!(
            resolve_prefix(&ids, "cam:webcam"),
            PrefixMatch::Unique(ids[0].clone())
        );
    }

    #[test]
    fn an_empty_camera_id_is_refused_off_the_wire_rather_than_matching_every_camera() {
        // Note **N50**. The two directions have to be one test, because what makes the
        // refusal worth having is *why* an empty id is dangerous rather than merely
        // meaningless: resolution is a prefix match, so the empty string is a prefix of
        // every id and a derived `Deserialize` turned `{"camera": ""}` into "the only
        // camera on this host" — silently, on every method that names one.
        let ids = assign_ids(&cards(&["Integrated Camera: Integrated C"]));
        assert_eq!(
            resolve_prefix(&ids, ""),
            PrefixMatch::Unique(ids[0].clone()),
            "the wildcard this refusal exists to keep off the wire"
        );

        let refused = serde_json::from_str::<CameraId>(r#""""#)
            .expect_err("an empty body is not a camera id");
        assert!(
            refused.to_string().contains("camera id"),
            "the refusal has to name what it wanted: {refused}"
        );
        // `cam:` on its own is the same absence wearing the prefix.
        assert!(serde_json::from_str::<CameraId>(r#""cam:""#).is_err());

        // And the directions a real client sends, including the one the CLI's own
        // `CameraArg` accepts: the prefix is optional on input and always present on
        // output, which is `CameraId::parse`'s rule and now the wire's. Both spellings are
        // derived from the id the assignment produced rather than written down, so this
        // stays true the day the slug rule changes.
        for spelling in [ids[0].as_str(), ids[0].body()] {
            assert_eq!(
                serde_json::from_str::<CameraId>(&format!("\"{spelling}\""))
                    .unwrap_or_else(|err| panic!("{spelling}: {err}")),
                ids[0]
            );
        }
        // Round-trip, because a document this build wrote has to be one it can read back.
        let rendered = serde_json::to_string(&ids[0]).expect("a string");
        assert_eq!(
            serde_json::from_str::<CameraId>(&rendered).expect("what we just wrote"),
            ids[0]
        );
    }

    #[test]
    fn capture_and_metadata_nodes_are_told_apart_by_caps_not_numbering() {
        // PF:7 — the Chicony RGB camera's nodes, in kernel order.
        assert_eq!(
            NodeKind::from_device_caps(CAP_VIDEO_CAPTURE),
            NodeKind::VideoCapture
        );
        assert_eq!(
            NodeKind::from_device_caps(CAP_META_CAPTURE),
            NodeKind::MetaCapture
        );
        // An unrecognized node keeps its caps word rather than becoming "capture".
        assert_eq!(
            NodeKind::from_device_caps(0x0400_0000),
            NodeKind::Other {
                device_caps: 0x0400_0000
            }
        );
    }

    /// A camera whose nodes are described by `(path, device_caps)`, in the given order.
    fn grouped(nodes: &[(&str, u32)]) -> CameraInfo {
        CameraInfo {
            id: CameraId::parse("cam:test").expect("literal id"),
            fingerprint: CameraFingerprint {
                bus_path: "2-3.4.1.1:1.0".to_owned(),
                usb_id: None,
                card: "Test".to_owned(),
                driver: "uvcvideo".to_owned(),
                serial: None,
            },
            card: "Test".to_owned(),
            driver: "uvcvideo".to_owned(),
            bus_info: "usb-test".to_owned(),
            nodes: nodes
                .iter()
                .map(|(path, device_caps)| DeviceNode {
                    path: Utf8PathBuf::from(*path),
                    kind: NodeKind::from_device_caps(*device_caps),
                    device_caps: *device_caps,
                    capabilities: 0x84a0_0001,
                })
                .collect(),
            backend: crate::backend::BackendKind::V4l2,
        }
    }

    #[test]
    fn a_group_with_two_capture_nodes_picks_the_first_and_keeps_the_other_listed() {
        // PF:19 — the Dell U3224KB/A's measured topology: one camera sensor, two USB
        // Streaming output terminals, four nodes on one VideoControl interface, two of
        // them streamable. `capture_node()` has to answer *something*, and the rule is
        // positional; what must never happen is the second node disappearing, because
        // `nodes` is where the shape of the device is recorded.
        let dell = grouped(&[
            ("/dev/video6", CAP_VIDEO_CAPTURE),
            ("/dev/video7", CAP_META_CAPTURE),
            ("/dev/video8", CAP_VIDEO_CAPTURE),
            ("/dev/video9", CAP_META_CAPTURE),
        ]);
        assert_eq!(
            dell.capture_node().expect("it has a capture node").path,
            "/dev/video6"
        );
        assert_eq!(
            dell.nodes
                .iter()
                .filter(|n| n.kind == NodeKind::VideoCapture)
                .count(),
            2,
            "the second capture node must stay in the node list"
        );

        // The inverse direction: the choice is the node order's, so reversing it moves
        // the answer. A `capture_node()` that had quietly grown a preference — the
        // largest `device_caps`, the lowest path, the last match — fails here.
        let reversed = grouped(&[
            ("/dev/video8", CAP_VIDEO_CAPTURE),
            ("/dev/video9", CAP_META_CAPTURE),
            ("/dev/video6", CAP_VIDEO_CAPTURE),
            ("/dev/video7", CAP_META_CAPTURE),
        ]);
        assert_eq!(
            reversed.capture_node().expect("a capture node").path,
            "/dev/video8"
        );
    }

    /// The OBSBOT's measured node shape, at whatever paths the caller names.
    ///
    /// The caps words are the ones `webcam-handler-cli list --json` reported on 2026-08-11 — a
    /// capture node and a metadata node, byte-identical before and after the reload that moved
    /// them from `video4,5` to `video0,1`.
    fn obsbot_at(paths: [&str; 2]) -> CameraInfo {
        let mut info = grouped(&[(paths[0], CAP_VIDEO_CAPTURE), (paths[1], CAP_META_CAPTURE)]);
        info.card = "OBSBOT Tiny 3: OBSBOT Tiny 3 St".to_owned();
        info.bus_info = "usb-0000:00:14.0-1".to_owned();
        info.fingerprint.card.clone_from(&info.card);
        info.nodes[0].device_caps = 0x0420_0001;
        info.nodes[1].device_caps = 0x0480_0000;
        for node in &mut info.nodes {
            node.capabilities = 0x84a0_0001;
        }
        info
    }

    #[test]
    fn a_renumbered_node_is_not_drift_because_dev_video_n_is_probe_order() {
        // PF:22, note N63, and the measurement itself: one `uvcvideo` unload/reload — the
        // cycle this project's own R3 hotplug arm performs (E9) — moved the OBSBOT from
        // /dev/video4,5 to /dev/video0,1 with `card`, `bus_info` and every node's kind and
        // caps identical on both sides. Nothing about the device changed, so the
        // comparison must say nothing changed.
        let committed = obsbot_at(["/dev/video4", "/dev/video5"]);
        let live = obsbot_at(["/dev/video0", "/dev/video1"]);
        assert_ne!(
            committed.nodes[0].path, live.nodes[0].path,
            "the fixture has to actually differ in the field under test"
        );
        assert_eq!(
            committed.differing_fields(&live),
            Vec::<String>::new(),
            "a renumbering is not a device change"
        );
        assert!(committed.describes_same_device(&live));
    }

    #[test]
    fn a_camera_that_changed_shape_still_goes_red_at_every_field_that_describes_it() {
        // The inverse of the test above, and the reason this comparison is not simply
        // "ignore the node list": the count and the per-node kind and caps *are* the
        // claim. Each direction is arranged from the same measured baseline, so a
        // comparison that had quietly stopped looking at nodes fails all four.
        let committed = obsbot_at(["/dev/video4", "/dev/video5"]);

        // A camera that grew a node. Paths chosen to overlap the committed ones, so the
        // only thing that can produce the mismatch is the count.
        let mut grew = committed.clone();
        grew.nodes.push(DeviceNode {
            path: Utf8PathBuf::from("/dev/video6"),
            kind: NodeKind::VideoCapture,
            device_caps: 0x0420_0001,
            capabilities: 0x84a0_0001,
        });
        assert_eq!(committed.differing_fields(&grew), vec!["nodes.len"]);

        // …and one that lost one. Both directions, because a comparison written as
        // "every committed node is present" would pass this half.
        let mut lost = committed.clone();
        lost.nodes.pop();
        assert_eq!(committed.differing_fields(&lost), vec!["nodes.len"]);

        // A node that changed what it is for. PF:7: kind is decided by device_caps, so a
        // node whose caps moved reports both — the classification and the word behind it.
        let mut reclassified = committed.clone();
        reclassified.nodes[1].device_caps = CAP_VIDEO_CAPTURE;
        reclassified.nodes[1].kind = NodeKind::from_device_caps(CAP_VIDEO_CAPTURE);
        assert_eq!(
            committed.differing_fields(&reclassified),
            vec!["nodes[1].kind", "nodes[1].device_caps"]
        );

        // And the whole-device capabilities word, which differs from device_caps on a
        // multi-node device and is kept for exactly that reason.
        let mut recapped = committed.clone();
        recapped.nodes[0].capabilities = 0x8420_0001;
        assert_eq!(
            committed.differing_fields(&recapped),
            vec!["nodes[0].capabilities"]
        );
    }

    #[test]
    fn identity_still_has_to_match_and_the_report_names_which_half_moved() {
        // The half the R3 arm was right to assert all along: `card` and `bus_info` are
        // what survived the reload, so they are what a mismatch means something by.
        let committed = obsbot_at(["/dev/video4", "/dev/video5"]);

        let mut renamed = committed.clone();
        renamed.card = "OBSBOT Tiny 2".to_owned();
        assert_eq!(committed.differing_fields(&renamed), vec!["card"]);

        let mut moved = committed.clone();
        moved.bus_info = "usb-0000:00:14.0-4".to_owned();
        assert_eq!(committed.differing_fields(&moved), vec!["bus_info"]);

        // The driver too, which is the field that would move if this camera were bound by
        // something other than `uvcvideo` — a different driver is a different set of
        // measured behaviours, and every PF entry in the registry is about one of them.
        let mut rebound = committed.clone();
        rebound.driver = "uvcvideo_next".to_owned();
        assert_eq!(committed.differing_fields(&rebound), vec!["driver"]);

        // The fingerprint is reported through its own comparison rather than a second
        // copy of it, so its fields arrive prefixed and PF:8's absent-serial rule holds
        // here too.
        let mut replugged = committed.clone();
        replugged.fingerprint.bus_path = "3-4:1.0".to_owned();
        replugged.fingerprint.serial = Some("0001".to_owned());
        assert_eq!(
            committed.differing_fields(&replugged),
            vec!["fingerprint.bus_path"],
            "an appearing serial is an absence filled in [PF:8], not a disagreement"
        );

        // Nothing is compared "for completeness": the backend that produced a description
        // is part of it, because a fake-backend capture must never read as a hardware one.
        let mut faked = committed.clone();
        faked.backend = crate::backend::BackendKind::Fake;
        assert_eq!(committed.differing_fields(&faked), vec!["backend"]);
    }

    #[test]
    fn a_camera_id_is_not_compared_because_it_is_derived_from_the_whole_topology() {
        // `assign_ids` hands out collision ordinals over every attached card, so which of
        // two identical cameras is `-2` follows enumeration order — the same node order
        // that PF:22 says the kernel reassigns. The id carries no fact about *this*
        // device that `card` does not, and `card` is compared, so dropping it weakens
        // nothing. The second line is what makes that claim checkable rather than
        // asserted: the ordinal really does move with the order.
        let committed = obsbot_at(["/dev/video4", "/dev/video5"]);
        let mut reordered = committed.clone();
        reordered.id = CameraId::parse("cam:obsbot-tiny-3-obsbot-tiny-3-st-2").expect("literal");
        assert_eq!(committed.differing_fields(&reordered), Vec::<String>::new());

        let cards = cards(&["OBSBOT Tiny", "OBSBOT Tiny"]);
        assert_eq!(
            bodies(&assign_ids(&cards)),
            vec!["obsbot-tiny", "obsbot-tiny-2"]
        );
    }

    #[test]
    fn fingerprint_mismatch_names_the_fields() {
        let a = CameraFingerprint {
            bus_path: "3-1:1.0".to_owned(),
            usb_id: Some(UsbId {
                vendor: 0x04f2,
                product: 0xb83c,
            }),
            card: "OBSBOT Tiny 3".to_owned(),
            driver: "uvcvideo".to_owned(),
            serial: None,
        };
        let mut b = a.clone();
        assert!(a.matches(&b));
        b.card = "Something Else".to_owned();
        b.bus_path = "3-4:1.0".to_owned();
        assert_eq!(a.differing_fields(&b), vec!["bus_path", "card"]);
    }

    #[test]
    fn an_absent_serial_is_not_a_mismatch() {
        // PF:8: the OBSBOT reports no serial. If absence counted as disagreement,
        // `calibrate apply` would refuse to run against the camera that recorded it.
        let a = CameraFingerprint {
            bus_path: "3-1:1.0".to_owned(),
            usb_id: None,
            card: "OBSBOT Tiny 3".to_owned(),
            driver: "uvcvideo".to_owned(),
            serial: None,
        };
        let mut b = a.clone();
        b.serial = Some("0001".to_owned());
        assert!(a.matches(&b));
        // Two *different* declared serials do disagree.
        let mut c = a.clone();
        c.serial = Some("0002".to_owned());
        assert_eq!(b.differing_fields(&c), vec!["serial"]);
    }

    #[test]
    fn fourcc_round_trips_and_prints() {
        for f in [
            PixelFormat::MJPG,
            PixelFormat::YUYV,
            PixelFormat::GREY,
            PixelFormat::NV12,
        ] {
            assert_eq!(PixelFormat::from_fourcc(f.to_fourcc()), f);
        }
        assert_eq!(PixelFormat::MJPG.to_string(), "MJPG");
        assert_eq!(PixelFormat::parse("YUYV"), Some(PixelFormat::YUYV));
        assert_eq!(PixelFormat::parse("YUY"), None);
        // A driver emitting a non-ASCII fourcc prints rather than panics.
        assert_eq!(PixelFormat([0, 1, b'A', b'B']).to_string(), "\\x00\\x01AB");
    }

    /// The population the bijection below is asserted over, and why it is this one.
    ///
    /// A list of the five formats this build names would prove nothing: they are four
    /// ASCII letters each, which is the one case that was never in doubt. What has to be
    /// covered is every byte whose *spelling* can interact with another byte's — the
    /// escape's own two characters (`\` and `x`), every hex digit in both cases, and
    /// non-graphic bytes spanning the escape space — so the population is the exhaustive
    /// cross product of that alphabet (four positions, one value per combination), plus a
    /// deterministic pseudo-random sweep of the whole `[u8; 4]` space so that no
    /// interaction outside the alphabet goes unlooked-at.
    ///
    /// It is generated rather than typed for the reason note **N108** charges for: a
    /// hand-written list is a list of the cases somebody already thought of, and the
    /// spelling defect this repairs was in exactly the byte nobody thought of.
    fn fourcc_population() -> Vec<PixelFormat> {
        let mut alphabet: Vec<u8> = vec![b'\\', b'x', b'A', 0x00, 0x1f, 0x20, 0x7f, 0xff];
        alphabet.extend(b"0123456789abcdefABCDEF");
        alphabet.sort_unstable();
        alphabet.dedup();

        let mut out = Vec::new();
        for &a in &alphabet {
            for &b in &alphabet {
                for &c in &alphabet {
                    for &d in &alphabet {
                        out.push(PixelFormat([a, b, c, d]));
                    }
                }
            }
        }

        // xorshift64, seeded from a constant so a failure is reproducible: a population
        // that differed run to run would make a red run unfixable and a green one a
        // different claim each time.
        let mut state: u64 = 0x243F_6A88_85A3_08D3;
        for _ in 0..20_000 {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let low = u32::try_from(state & 0xFFFF_FFFF).expect("masked to 32 bits");
            out.push(PixelFormat(low.to_le_bytes()));
        }
        out
    }

    #[test]
    fn every_four_byte_value_survives_the_wire_spelling_that_names_it() {
        // The round-trip law the 2026-08-14 ruling rests on: what a consumer reads back is
        // the four bytes the driver enumerated, for *every* four bytes — not for the five
        // this build happens to name. `PixelFormat::parse` is the decoder and
        // `fmt::Display` the encoder, and this is the only place their inverse relationship
        // is stated.
        let population = fourcc_population();
        assert!(
            population.len() > 700_000,
            "the population collapsed to {} values; a bijection asserted over a handful of \
             ASCII names is the claim that was already true",
            population.len()
        );
        for format in &population {
            let spelled = format.to_string();
            assert_eq!(
                PixelFormat::parse(&spelled),
                Some(*format),
                "{format:?} spells {spelled:?}, which does not decode back to it"
            );
        }
    }

    #[test]
    fn two_different_fourccs_cannot_share_one_spelling() {
        // Injectivity, which is the half a round trip does not prove on its own: a decoder
        // that returned the right answer for every string it was handed would still be
        // useless if two formats produced the same string, because the ambiguity would be
        // in what the *wire* carries rather than in what this build does with it.
        let mut spellings: BTreeMap<String, PixelFormat> = BTreeMap::new();
        for format in fourcc_population() {
            if let Some(other) = spellings.insert(format.to_string(), format) {
                assert_eq!(
                    other,
                    format,
                    "{other:?} and {format:?} both spell {:?}",
                    format.to_string()
                );
            }
        }
    }

    #[test]
    fn a_backslash_bearing_fourcc_doubles_its_backslashes_so_one_pass_decoding_is_possible() {
        // The repair itself, stated as the two values it separates (note **N109**). Before
        // it, `\` printed as itself, and a decoder reading left to right could not tell a
        // literal backslash from the start of an escape without backtracking over the rest
        // of the string — 13% of the alphabet population above could not be read back at
        // all. This assertion is red against that encoder in both directions.
        assert_eq!(PixelFormat(*b"\\x4d").to_string(), "\\\\x4d");
        assert_eq!(PixelFormat::parse("\\\\x4d"), Some(PixelFormat(*b"\\x4d")));
        assert_eq!(PixelFormat([b'\\'; 4]).to_string(), "\\\\\\\\\\\\\\\\");
        // …and the four literal characters `\x4d` name no format at all now: they spell one
        // byte, and a FourCC is four.
        assert_eq!(PixelFormat::parse("\\x4d"), None);
    }

    #[test]
    fn a_spelling_that_names_no_four_bytes_is_refused_rather_than_padded() {
        // Every refusal the grammar has, named. A format short of four bytes must not
        // become one with a zero on the end, and a fifth must not be dropped: both would
        // hand a device a format nobody asked for.
        for refused in [
            "MJP",                // three
            "MJPGX",              // five
            "\\",                 // a backslash beginning no escape
            "\\q",                // an escape this grammar does not have
            "\\x",                // no hex digits
            "\\x4",               // one hex digit
            "\\xzz",              // two characters that are not hex digits
            "MJP\u{00e9}",        // non-ASCII: a byte above 0x7f travels as `\xNN`
            "",                   // nothing at all
            "\\x00\\x00\\x00",    // three escapes
            "\\x00\\x00\\x00\\x", // three escapes and a fragment
        ] {
            assert_eq!(
                PixelFormat::parse(refused),
                None,
                "{refused:?} was accepted as a fourcc"
            );
        }
        // The same refusal for the reason it names, which the row above does not reach: a
        // `\u{e9}` is two UTF-8 bytes, so `MJP\u{e9}` spells *five* and is refused for its
        // length. These four are exactly four bytes each, so length says nothing about
        // them and only the grammar can (the G6 review's L4, note **N227**).
        for refused in [
            "\u{00e9}\u{00e9}", // four bytes, no ASCII in them at all
            "MJ\u{00e9}",       // four bytes, two of them a character's halves
            "MJP\u{007f}",      // DEL — not a graphic, and `Display` escapes it
            "MJP\u{0000}",      // a raw NUL, which is what a padded C string leaks
        ] {
            assert_eq!(
                PixelFormat::parse(refused),
                None,
                "{refused:?} was accepted as a fourcc"
            );
        }
        // A raw space is the one alias, because `v4l2-ctl` prints the kernel's padded names
        // that way and a flag that refused `Y16 ` would be refusing a format the device has.
        assert_eq!(PixelFormat::parse("Y16 "), Some(PixelFormat(*b"Y16 ")));
        // …and it is an alias on input only: the canonical spelling escapes it, so a
        // trailing space cannot be lost to a shell word or a table column.
        assert_eq!(PixelFormat(*b"Y16 ").to_string(), "Y16\\x20");
        // Both cases on the way in, one on the way out.
        assert_eq!(
            PixelFormat::parse("\\xFF\\xff\\xFF\\xff"),
            Some(PixelFormat([0xFF; 4]))
        );
        assert_eq!(PixelFormat([0xFF; 4]).to_string(), "\\xff\\xff\\xff\\xff");
    }

    #[test]
    fn a_pixel_format_crosses_json_as_its_fourcc_rather_than_as_four_numbers() {
        // The owner's ruling of 2026-08-14, as the shape on the wire. This is the assertion
        // that is red against the `#[serde(transparent)]` newtype the type used to be.
        assert_eq!(
            serde_json::to_string(&PixelFormat::MJPG).expect("serialize"),
            "\"MJPG\""
        );
        assert_eq!(
            serde_json::to_string(&[PixelFormat::MJPG, PixelFormat::YUYV]).expect("serialize"),
            "[\"MJPG\",\"YUYV\"]"
        );
        assert_eq!(
            serde_json::from_str::<PixelFormat>("\"YUYV\"").expect("deserialize"),
            PixelFormat::YUYV
        );
        // A driver's unprintable fourcc crosses too, and comes back whole (AGENTS rule 6).
        let odd = PixelFormat([0x00, 0x5c, 0x7f, 0xff]);
        let json = serde_json::to_string(&odd).expect("serialize");
        assert_eq!(json, "\"\\\\x00\\\\\\\\\\\\x7f\\\\xff\"");
        assert_eq!(
            serde_json::from_str::<PixelFormat>(&json).expect("deserialize"),
            odd
        );
        // The old shape is refused rather than accepted alongside the new one: the owner
        // chose one wire form over "accept both", and a build that quietly took the array
        // back would make that choice unobservable.
        let refused = serde_json::from_str::<PixelFormat>("[77,74,80,71]");
        assert!(
            refused.is_err(),
            "the array form is still accepted: {refused:?}"
        );
        // And so is a string that names no four bytes, with the expectation spelled out
        // rather than left as "invalid value".
        let message = serde_json::from_str::<PixelFormat>("\"MJP\"")
            .expect_err("three characters are not a fourcc")
            .to_string();
        assert!(message.contains("four-character FourCC"), "{message}");
    }

    #[test]
    fn a_pixel_format_debugs_as_its_fourcc_so_a_panic_message_reads() {
        // One home for the spelling (design §2.10): the derived `Debug` printed the array,
        // which is how `[77, 74, 80, 71]` reached a mutation transcript in the notes long
        // before it was noticed on the wire.
        assert_eq!(format!("{:?}", PixelFormat::MJPG), "PixelFormat(\"MJPG\")");
        assert_eq!(
            format!("{:?}", PixelFormat([0x00, 0x5c, b'A', b'B'])),
            "PixelFormat(\"\\x00\\\\AB\")"
        );
    }

    #[test]
    fn the_published_schema_for_a_fourcc_is_a_string_that_explains_its_escapes() {
        // `schemas/webcam-handler-openrpc.json` is what an agent reads instead of this
        // source, so the shape *and* the sentence that makes it usable are both the
        // artifact's content — this is the claim `schema-artifacts-current.sh` backstops
        // from the other side.
        let schema = schemars::schema_for!(PixelFormat);
        let value = schema.as_value();
        assert_eq!(
            value.get("type").and_then(serde_json::Value::as_str),
            Some("string")
        );
        let description = value
            .get("description")
            .and_then(serde_json::Value::as_str)
            .expect("the description ships in the emitted document");
        assert!(description.contains("\\xNN"), "{description}");
        assert!(description.contains("\\\\"), "{description}");
    }

    #[test]
    fn a_size_or_interval_shape_this_build_cannot_read_is_carried_not_dropped() {
        // D2 applied to the format tree. The kernel defines three `type` values for each
        // and this build reads all three, so these variants are unreachable today — which
        // is the point: when a fourth arrives, a short list must not pass for a complete
        // one.
        let size = FrameSize::Unknown { raw: 9 };
        assert_eq!(size.max_dimensions(), None);
        assert!(!size.is_interpretable());
        assert!(
            FrameSize::Discrete {
                width: 640,
                height: 480
            }
            .is_interpretable()
        );

        // The interval half has no `is_interpretable` beside it, and that is deliberate:
        // `FrameSize::is_interpretable` has two readers in `capture.rs` that decide which
        // sizes a request may be resolved against, and the interval one had none in the
        // whole workspace — a `#[must_use]` public predicate nothing asked, with a
        // documented usage (`if is_interpretable() { compute_rate() }`) that would have
        // been wrong for `Unstated` (rubric A8, note **N199**). `fps` is the question
        // callers actually ask, and it answers for every variant.
        let interval = FrameInterval::Unknown { raw: 9 };
        assert_eq!(interval.fps(), None);

        // And both survive the wire with their discriminant.
        for value in [
            serde_json::to_string(&size).expect("serialize"),
            serde_json::to_string(&interval).expect("serialize"),
        ] {
            assert!(value.contains("\"raw\":9"), "{value}");
        }
        assert_eq!(
            serde_json::from_str::<FrameSize>(&serde_json::to_string(&size).expect("serialize"))
                .expect("deserialize"),
            size
        );
    }

    #[test]
    fn a_device_that_named_no_interval_and_one_that_named_an_unreadable_shape_are_two_answers() {
        // The fourth `FrameInterval`, and why it is a variant rather than a spelling of
        // the third (note **N194**). "The driver said `type = 0`" and "the driver said it
        // does not negotiate an interval on this node" are different facts, and until
        // 2026-08-16 the V4L2 backend spelled the second one as the first — a `raw` the
        // doc comment promises is *the kernel's discriminant, preserved exactly*, with a
        // number the kernel never sent.
        let unstated = FrameInterval::Unstated;
        let invented = FrameInterval::Unknown { raw: 0 };
        assert_ne!(unstated, invented);

        // Neither has a rate, and a caller asking "how fast" cannot tell them apart —
        // which is right, because neither answers that question.
        assert_eq!(unstated.fps(), None);
        assert_eq!(invented.fps(), None);

        // A degenerate fraction the device really did send is a third thing again, and it
        // is neither of these: the driver said something, it is carried verbatim, and the
        // rate it implies does not exist (note **N199**).
        let degenerate = FrameInterval::Discrete {
            numerator: 1,
            denominator: 0,
        };
        assert_ne!(degenerate, unstated);
        assert_eq!(
            degenerate.fps(),
            None,
            "a zero denominator is not zero frames per second"
        );

        // And on the wire they are two documents, with no `raw` on the one that has no
        // discriminant to carry.
        let rendered = serde_json::to_string(&unstated).expect("serialize");
        assert_eq!(rendered, r#"{"kind":"unstated"}"#);
        assert!(!rendered.contains("raw"), "{rendered}");
        assert_eq!(
            serde_json::from_str::<FrameInterval>(&rendered).expect("deserialize"),
            unstated
        );
    }

    #[test]
    fn frame_intervals_report_fps() {
        let i = FrameInterval::Discrete {
            numerator: 1,
            denominator: 30,
        };
        assert_eq!(i.fps(), Some(30.0));
        let degenerate = FrameInterval::Discrete {
            numerator: 0,
            denominator: 30,
        };
        assert_eq!(degenerate.fps(), None);
    }
}
