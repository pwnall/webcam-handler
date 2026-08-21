//! The device profile (design T3).
//!
//! A JSON capture of everything a backend can enumerate about a camera, in **two
//! sections whose comparison semantics differ**:
//!
//! - the **invariant** description — identity, nodes, formats, the control set with its
//!   menus, ranges and non-volatile flags, and the automation pairs measured on the
//!   device. This is what "the corpus still resembles the device" means. Formats,
//!   controls and pairs compare exactly; the `info` half compares by
//!   [`crate::camera::CameraInfo::differing_fields`], because the one field in here that
//!   is not invariant is the `/dev/videoN` path the kernel hands out in probe order
//!   \[PF:22, note **N63**\].
//! - the **state** block — current control values and the INACTIVE-class flags, which
//!   change with use \[PF:3, PF:4\]. Re-capturing a profile after a sweep must not read as
//!   corpus drift, so this compares loosely or not at all.
//!
//! Provenance rides outside both.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::backend::BackendKind;
use crate::camera::{CameraInfo, FormatInfo};
use crate::control::{ControlDesc, ControlSlug, ControlValue, KnownFlag};
use crate::limits;
use crate::pairing::AutomationPair;
use crate::time::Stamp;

/// Where a profile came from. Never compared — a re-capture has a new timestamp by
/// definition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ProfileProvenance {
    /// When it was captured.
    pub captured_at: Stamp,
    /// `uname -r` of the capturing host.
    pub kernel: String,
    /// The tool version that captured it.
    pub tool_version: String,
    /// Who or what ran the capture.
    pub capturer: String,
    /// Which backend produced it. A profile captured from the fake backend would be
    /// circular corpus, so this field is what makes that visible.
    pub backend: BackendKind,
}

/// The part of a profile that should not change unless the device or the kernel does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ProfileInvariant {
    /// Identity, nodes, grouping.
    pub info: CameraInfo,
    /// Formats, with sizes and intervals nested \[PF:9\].
    pub formats: Vec<FormatInfo>,
    /// The full control set, with `current` cleared and the volatile flag bits masked
    /// out — see [`invariant_control`].
    pub controls: Vec<ControlDesc>,
    /// Automation pairs discovered by probing this device \[PF:3\].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub measured_pairs: Vec<AutomationPair>,
}

/// The part that changes with use.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
pub struct ProfileState {
    /// Each control's value at capture time — including the ones outside their declared
    /// range \[PF:4\], which is exactly why this is recorded.
    #[serde(default)]
    pub values: BTreeMap<ControlSlug, ControlValue>,
    /// Each control's raw flag word at capture time. The INACTIVE bit here tracks which
    /// automation was on when the profile was taken \[PF:3\].
    #[serde(default)]
    pub flags: BTreeMap<ControlSlug, u32>,
}

/// A captured device profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DeviceProfile {
    /// The document version.
    pub schema_version: u32,
    /// Where this came from.
    pub provenance: ProfileProvenance,
    /// What does not change.
    pub invariant: ProfileInvariant,
    /// What does.
    #[serde(default)]
    pub state: ProfileState,
}

/// The slugs whose invariant descriptors disagree, in sorted order.
///
/// Keyed by slug rather than compared positionally, so the answer names *which* control
/// moved: a control present on one side only differs, and so does one both sides describe
/// differently. The sort is on the slug, so the same two profiles always produce the same
/// list — a diff whose order depended on which side was passed first would be a document two
/// runs could disagree about.
fn differing_control_slugs(mine: &[ControlDesc], theirs: &[ControlDesc]) -> Vec<ControlSlug> {
    let ours: BTreeMap<&ControlSlug, &ControlDesc> =
        mine.iter().map(|desc| (&desc.slug, desc)).collect();
    let others: BTreeMap<&ControlSlug, &ControlDesc> =
        theirs.iter().map(|desc| (&desc.slug, desc)).collect();

    // Through a set rather than by chaining the two key iterators: a slug both sides carry
    // appears in both, and the two runs are each sorted without being sorted *together*, so
    // the duplicates a `dedup` would remove are not adjacent. The set is also what makes the
    // order a property of the slugs instead of a property of the argument order.
    let every_slug: BTreeSet<&ControlSlug> = ours.keys().chain(others.keys()).copied().collect();
    every_slug
        .into_iter()
        .filter(|slug| ours.get(slug) != others.get(slug))
        .cloned()
        .collect()
}

/// The flag bits that belong to the state block rather than the invariant one.
///
/// `INACTIVE` flips when an automation partner is toggled and `GRABBED` flips while
/// something streams — both are facts about *right now*, not about the device.
pub const VOLATILE_FLAG_BITS: u32 = KnownFlag::Inactive.bit() | KnownFlag::Grabbed.bit();

/// Strip a control descriptor down to its invariant part.
///
/// Clears the current value and masks the flag bits that change with use, so two
/// captures of the same untouched device compare equal even if one was taken mid-stream.
#[must_use]
pub fn invariant_control(desc: &ControlDesc) -> ControlDesc {
    use crate::control::ControlFlags;

    let mut out = desc.clone();
    out.current = None;
    out.flags = ControlFlags::from_raw(desc.flags.raw & !VOLATILE_FLAG_BITS);
    out
}

/// Which sections of two profiles' invariant halves disagree.
///
/// [`DeviceProfile::invariant_matches`] used to be the whole answer: one bool over four
/// sections, and every caller that got `false` had the same recourse, which was to go and
/// read a `{:#?}` dump of both sides. That stopped being enough on **2026-08-13**, when the
/// owner ruled that a camera's advertised support **may change each time the camera is
/// plugged in**, and that we need not worry about it changing while the camera stays
/// connected. Under that ruling a disagreement is no longer one kind of event: a fresh
/// capture that offers different modes than the corpus is the device exercising a licence
/// the owner has granted it, and a fresh capture whose *identity*, control set or measured
/// pairs moved is still the corpus being wrong about the device or the device being wrong
/// about itself.
///
/// **The split is measured, not assumed.** \[PF:23\] is unusually specific about where the
/// variation lives. When the OBSBOT Tiny 3 stopped advertising 3840×2160 and 120 fps its
/// `CameraInfo` half was identical — `differing_fields` answered `[]` — and its control set
/// was identical, "all 24 controls, byte for byte"; only the format tree moved. When the
/// whole tree came back two days later, on an enumeration the journal can point at, it came
/// back the same way and by the same amount. Two observations in opposite directions, and in
/// both of them exactly one of the four sections is the one that moved. So `formats` gets a
/// predicate of its own and the other three do not, and the day a control set is measured
/// moving across a replug is the day that stops being the right shape — which is why
/// [`Self::is_only_the_format_tree`] is written as a question about *this* value rather than
/// as a general "is this difference tolerable".
///
/// **A `Vec<&'static str>` of section names was the smaller change and it loses the thing
/// that matters.** `info` disagrees by *field* — [`CameraInfo::differing_fields`] is the one
/// home for that rule \[PF:22, note **N63**\] — so a flat list of section names either
/// throws those field names away or smuggles them into a string nobody can match on. This
/// value carries both: the sections as flags, and the `info` fields as the names that
/// function produced, so a caller can print the useful sentence and still branch on the
/// cheap question.
///
/// **Not a wire type, and deliberately not.** It derives neither `Serialize` nor
/// `JsonSchema`. It is the answer to a comparison between two documents, computed fresh on
/// whichever side is asking; putting it in `schemas/` would turn an internal verdict into
/// something this project promises to keep answering the same way, and the verdict is
/// exactly the thing PF:23 says may have to change when the next device teaches us
/// something.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvariantDifference {
    /// The `info` fields that describe different devices, in the order and the spelling
    /// [`CameraInfo::differing_fields`] produced. Empty when the two halves describe the
    /// same camera in the same shape — which is not the same as being equal, because
    /// `/dev/videoN` is excluded there \[PF:22\].
    info: Vec<String>,
    /// Everything that is not identity: the format tree, the control set and the measured
    /// pairs. Held as [`DeviceDifference`] rather than as three flags so that the
    /// section-membership rule has one home and this type is a *view* of the comparison
    /// D15 made rather than a second comparison beside it (design §2.10).
    device: DeviceDifference,
}

/// What two profiles say about the **device** — the description half of D15's partition.
///
/// The identity half is where the device *is* (`info`: id, fingerprint, bus strings, node
/// table) and legitimately differs across a forwarded bus, another port, another machine.
/// This half is what the device *is*, and across all of those it must not differ. Answering
/// them separately is FR-W2's whole request: "is the forwarded camera the same device?" and
/// "what identity moved?" are two questions with one comparison behind them.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DeviceDifference {
    /// Whether the format tree differs, at any of its three depths: a pixel format, a size
    /// under a pixel format, or a frame interval under a size. One flag rather than three,
    /// because the ruling this type serves is about the tree and not about a level of it.
    pub formats: bool,
    /// The control slugs whose invariant descriptor differs — described differently on the
    /// two sides, or present on only one of them. Named rather than counted, because
    /// "seventeen controls differ" sends a reader to diff two documents by hand and
    /// `["pan_absolute"]` sends them to the control.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub controls: Vec<ControlSlug>,
    /// Whether the automation pairs D3's probe measured differ \[PF:3\].
    pub measured_pairs: bool,
}

impl DeviceDifference {
    /// The sections that disagree, named, in the order [`ProfileInvariant`] declares them.
    ///
    /// The one walk of these fields: [`InvariantDifference::sections`] extends this list
    /// with `info`, [`ProfileComparison`] renders it, and the format-tree predicates on both
    /// types compare against it — so "which of these counts as a disagreement, and in what
    /// order" is settled here and nowhere else.
    #[must_use]
    pub fn sections(&self) -> Vec<&'static str> {
        // Destructured, not field-accessed, so a fourth description section cannot be added
        // without being named here: a section missing from this list is a section that
        // silently never disagrees *and* one the format-tree predicates would keep waving
        // through, because both are written over this list precisely so they cannot be told
        // about a section separately.
        let Self {
            formats,
            controls,
            measured_pairs,
        } = self;

        let mut out = Vec::new();
        if *formats {
            out.push(InvariantDifference::FORMATS);
        }
        if !controls.is_empty() {
            out.push("controls");
        }
        if *measured_pairs {
            out.push("measured_pairs");
        }
        out
    }

    /// Whether the two profiles describe the same device.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.sections().is_empty()
    }

    /// The one computation of D15's verdict, from the one walk of these fields.
    ///
    /// Both of [`ProfileComparison`]'s bools are readings of this, and nothing else computes
    /// it, so the three answers a consumer can get out of a comparison — "same device",
    /// "only the format tree", "differs" — cannot come to disagree with each other or with
    /// [`Self::sections`] (design §2.10).
    ///
    /// **A `match` on the section list, and never a conjunction over the fields.** The
    /// obvious spelling of the middle arm is `formats && controls.is_empty() &&
    /// !measured_pairs`, and note **N89** rules it out by name: it "keeps answering `true`
    /// when a *fifth* section is added later, which would silently extend the owner's ruling
    /// to something nobody was asked about". Written over the list, an unrecognised
    /// combination falls to the wildcard and lands in [`DeviceVerdict::Differs`] — the
    /// direction a permission has to fail in.
    #[must_use]
    pub fn verdict(&self) -> DeviceVerdict {
        match self.sections().as_slice() {
            [] => DeviceVerdict::SameDevice,
            [InvariantDifference::FORMATS] => DeviceVerdict::OnlyTheFormatTree,
            _ => DeviceVerdict::Differs,
        }
    }
}

/// What one comparison says about the device half, as a closed vocabulary on the document
/// (design D15; owner's 2026-08-13 ruling; note **N89**).
///
/// **Why this is on the document and not only a pair of accessors.** `ProfileComparison`
/// exists for the consumer design §1.3 names — the sibling HIL harness, which pins this
/// library *and* shells out to `--json`, comparing answers across machines. That consumer
/// reading the document got `device` and `identity` and had to rebuild the verdict itself,
/// and the only spelling available to it was the conjunction over the three device fields
/// that N89 says fails **open** the day a fourth section lands. A verdict a Rust caller can
/// read and a `--json` caller cannot is a verdict published in the one form its stated reader
/// cannot safely use, so the serialized document carries it — computed at write time by
/// [`DeviceDifference::verdict`], which is where it is computed for every other reader too
/// (note **N289**: the value stores no copy, because the half it is derived from is public
/// and a stored summary is one a caller can leave behind).
///
/// **Three arms and no fourth.** They are the three answers the owner's ruling distinguishes:
/// nothing moved; the format tree moved and nothing else, which a camera is licensed to do
/// each time it is plugged in \[PF:23\]; and anything else, which no measurement licenses.
/// A section added to [`ProfileInvariant`] later joins [`Self::Differs`] by falling through
/// the wildcard rather than by being waved through a permission, which is the whole reason
/// the computation is an equality against a list.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DeviceVerdict {
    /// The device half is empty: the two profiles describe the same device, whatever their
    /// identity halves say.
    ///
    /// The default, because an empty [`DeviceDifference`] *is* the same device and a
    /// `ProfileComparison::default()` that said otherwise would be a document disagreeing
    /// with its own contents.
    #[default]
    SameDevice,
    /// The advertised format tree is the **only** section that differs.
    ///
    /// The owner's 2026-08-13 ruling in one word: a camera's format tree is invariant within
    /// a connection and nowhere else \[PF:23, note **N89**\], so a consumer may treat this as
    /// a fact about a replug rather than about a different device. This tool reports the
    /// shape and declines to guess what it means for somebody else's rig.
    OnlyTheFormatTree,
    /// Something the ruling does not license moved — including a format-tree difference with
    /// a second finding beside it, which is two observations at once and the second one is
    /// unexplained.
    Differs,
}

impl fmt::Display for DeviceDifference {
    /// Every disagreeing section, with the control slugs named underneath theirs.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let named: Vec<String> = self
            .sections()
            .into_iter()
            .map(|section| {
                if section == "controls" {
                    let slugs: Vec<&str> = self.controls.iter().map(ControlSlug::as_str).collect();
                    format!("controls ({})", slugs.join(", "))
                } else {
                    section.to_owned()
                }
            })
            .collect();
        f.write_str(&named.join(", "))
    }
}

/// Two profiles compared with identity held to one side (design D15; FR-W2).
///
/// The document `profile compare` answers, and a schema DTO because the consumer that asked
/// for it reads `--json` out of a subprocess. Both halves come back from one comparison
/// because the consumer needs both: [`Self::device`] is the fidelity assertion — the
/// forwarded camera is the same device — and [`Self::identity`] is the expected-delta
/// report, since a different bus path is exactly what forwarding *means*.
///
/// **The verdict is on the document, and it is not a field of this value** (notes **N286**,
/// **N289**). §1.3's sibling harness pins this library *and* shells out, and reading the
/// document it got `device` and `identity` and had to rebuild the format-tree permission
/// from three fields by the conjunction note **N89** rules out — so the answer has to be in
/// the bytes. It gets there by being *computed at write time*, in
/// [`DeviceDifference::verdict`], rather than by being stored here: [`Self::device`] is a
/// public field of a public type on a value that derives `Default`, so a stored summary of
/// it is a summary any caller can leave behind, and note **N289** measured that hole open —
/// `ProfileComparison::default()` plus `device.formats = true` answered "same device" and
/// serialized bytes this type's own `Deserialize` refused. Every reading below is a reading
/// of `device` as it stands at the moment it is taken, so there is nothing to go stale.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProfileComparison {
    /// What the device *is*: formats, controls, measured pairs.
    pub device: DeviceDifference,
    /// Where the device *is*: the `info` fields that differ, in the order and the spelling
    /// [`CameraInfo::differing_fields`] produced. Expected to be non-empty across a
    /// forwarded bus, a different port, a reboot or another machine.
    pub identity: Vec<String>,
}

/// The document a [`ProfileComparison`] is written to and read from — D15's two halves with
/// D15's verdict beside them.
///
/// **`verdict` is on the document and not on the value**, because the two have different
/// hazards. A `--json` consumer holds bytes it cannot recompute anything from without
/// reimplementing note **N89**'s rule, so the bytes must carry the answer; a Rust caller
/// holds a value whose `device` half it can edit, so the value must not carry a copy that
/// could be left behind (note **N289**). This struct is where those meet: it is the one
/// shape [`ProfileComparison`]'s `Serialize`, `Deserialize` and `JsonSchema` all go through,
/// so the bytes, the refusal and the published schema cannot come to describe three
/// different documents.
///
/// A shadow struct rather than a `serialize_with`/`deserialize_with` on the field, because
/// the verdict is a function of a *sibling* field and a field (de)serializer sees only its
/// own — the one way this differs from [`crate::error::Failure`]'s marker, whose
/// contradiction is visible in the field itself.
#[derive(Serialize, Deserialize, JsonSchema)]
struct ProfileComparisonDocument {
    /// What the device *is*: formats, controls, measured pairs. The fidelity assertion.
    device: DeviceDifference,
    /// Where the device *is*: the `info` fields that differ. Expected to be non-empty across
    /// a forwarded bus, a different port, a reboot or another machine, and omitted when
    /// nothing about the address moved.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    identity: Vec<String>,
    /// D15's answer about the device half, in one closed word: `same_device`,
    /// `only_the_format_tree` or `differs`.
    ///
    /// A reading of `device` and never a second opinion beside it — computed by
    /// [`DeviceDifference::verdict`] on the way out, and refused on the way *in* when the
    /// bytes state one the sections they carry do not support, because bytes arrive from
    /// somewhere else and nothing this crate does can constrain how they were written.
    verdict: DeviceVerdict,
}

/// Refuse a comparison whose stated verdict is not the one its device half supports.
///
/// The verdict is a reading of `device`, so a document that disagrees with itself is one of
/// two things and both are worse than a refusal: a hand-written comparison claiming a
/// permission its sections do not license — `same_device` over a non-empty device half, or
/// `only_the_format_tree` over a difference with a control beside it — or a document written
/// by a version of this tool whose section vocabulary is not this one's. Refusing rather than
/// recomputing is AGENTS rule 6's direction: a value that silently replaced the stated verdict
/// with the computed one would parse the second case into an answer nobody wrote.
fn refuse_a_verdict_the_sections_do_not_support(
    document: ProfileComparisonDocument,
) -> std::result::Result<ProfileComparison, String> {
    let ProfileComparisonDocument {
        device,
        identity,
        verdict,
    } = document;
    let computed = device.verdict();
    if verdict == computed {
        Ok(ProfileComparison { device, identity })
    } else {
        Err(format!(
            "a comparison's `verdict` is a reading of its `device` half; this document says \
             {verdict:?} where the sections it carries ({}) say {computed:?}",
            if device.sections().is_empty() {
                "none".to_owned()
            } else {
                device.sections().join(", ")
            }
        ))
    }
}

impl Serialize for ProfileComparison {
    /// Through `ProfileComparisonDocument` — a private type, so this is a plain reference and
    /// not an intra-doc link — so the word in the bytes is computed from the device half being
    /// written rather than read out of a field that was filled earlier.
    ///
    /// The halves are cloned to build it. That is a bool, a slug list and a short string list
    /// copied once per `profile compare` answer, and the alternative — a second, borrowing
    /// shadow struct — would be a second home for the document's field list, free to drift
    /// from the one the deserializer and the published schema use (design §2.10).
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        ProfileComparisonDocument {
            device: self.device.clone(),
            identity: self.identity.clone(),
            verdict: self.device.verdict(),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ProfileComparison {
    fn deserialize<D>(deserializer: D) -> std::result::Result<ProfileComparison, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        refuse_a_verdict_the_sections_do_not_support(ProfileComparisonDocument::deserialize(
            deserializer,
        )?)
        .map_err(serde::de::Error::custom)
    }
}

/// The published schema is the *document*'s, under this type's name.
///
/// Delegated rather than derived, because what a `--json` consumer validates against has to
/// be the shape [`Serialize`] emits and [`Deserialize`] accepts — which carries `verdict`,
/// and which this Rust value deliberately does not (note **N289**). Deriving it here would
/// publish a schema with no `verdict` property while every document this crate writes has
/// one, and `scripts/gates/json-validates.sh` validates real `--json` output against the
/// committed bundle, which is where that would surface.
impl JsonSchema for ProfileComparison {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "ProfileComparison".into()
    }

    fn schema_id() -> std::borrow::Cow<'static, str> {
        concat!(module_path!(), "::ProfileComparison").into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        ProfileComparisonDocument::json_schema(generator)
    }
}

impl ProfileComparison {
    /// D15's verdict about the device half — the one word the document carries.
    ///
    /// Computed from [`Self::device`] on every call, by [`DeviceDifference::verdict`], and the
    /// two bools below are readings of it. A consumer holding the deserialized document reads
    /// the `verdict` field; a consumer holding the value calls this; they are the same answer
    /// because both are the same computation over the same half, and
    /// `refuse_a_verdict_the_sections_do_not_support` is what keeps a *foreign* document from
    /// claiming otherwise.
    #[must_use]
    pub fn verdict(&self) -> DeviceVerdict {
        self.device.verdict()
    }

    /// Whether the two profiles describe the same device, whatever their identity says.
    ///
    /// The derived bool, for callers that only branch. Everything that decides it is on
    /// [`Self::device`], so a caller that wants to know *what* moved never has to compare
    /// twice to find out.
    #[must_use]
    pub fn device_matches(&self) -> bool {
        self.verdict() == DeviceVerdict::SameDevice
    }

    /// Whether the advertised format tree is the **only** thing the device half disagrees
    /// about.
    ///
    /// The distinction the owner's 2026-08-13 ruling turns on, carried on the answer so a
    /// consumer can apply the policy its own situation warrants: a camera's format tree is
    /// invariant within a connection and nowhere else \[PF:23, note **N89**\], so two
    /// captures across a replug may legitimately differ here and nowhere else. This tool
    /// reports the shape and declines to guess what it means for somebody else's rig.
    ///
    /// A reading of [`Self::verdict`], which is written as an equality against
    /// [`DeviceDifference::sections`] rather than as a conjunction, for the reason
    /// [`InvariantDifference::is_only_the_format_tree`] gives: a conjunction fails *open* the
    /// day a fourth section is added, and a permission should fail closed.
    #[must_use]
    pub fn device_differs_only_in_the_format_tree(&self) -> bool {
        self.verdict() == DeviceVerdict::OnlyTheFormatTree
    }
}

impl fmt::Display for ProfileComparison {
    /// The device half, then the identity half, each named — and "the same device" said in
    /// words when the device half is empty, because an empty string is the one answer a
    /// reader cannot tell from a failure to print.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.device.is_empty() {
            f.write_str("same device")?;
        } else {
            write!(f, "device differs: {}", self.device)?;
        }
        if !self.identity.is_empty() {
            write!(f, "; identity differs: {}", self.identity.join(", "))?;
        }
        Ok(())
    }
}

impl InvariantDifference {
    /// How the two sections anybody asks about by name are spelled, in one place, because
    /// [`fmt::Display`] and [`Self::is_only_the_format_tree`] both have to recognise one of
    /// them in the list [`Self::sections`] returns.
    const INFO: &'static str = "info";
    const FORMATS: &'static str = "formats";

    /// The sections that disagree, named, in the order [`ProfileInvariant`] declares them.
    ///
    /// The value-shaped answer, so a test can assert *which* sections a comparison found
    /// rather than pattern-matching on prose — the same reason
    /// [`crate::pairing::AutomationPair`]'s consumers get values and not sentences. It is
    /// also the **one** walk of these fields: [`fmt::Display`] renders this list and
    /// [`Self::is_only_the_format_tree`] compares against it, so "which of these counts as a
    /// disagreement, and in what order" is settled here and nowhere else (design §2.10).
    #[must_use]
    pub fn sections(&self) -> Vec<&'static str> {
        // Destructured for the same reason `DeviceProfile::compare` destructures
        // `ProfileInvariant`: a field added to this struct and not named here would be a
        // section that silently never disagrees *and* one the format-tree predicate would
        // keep waving through. The compiler is the only thing that reliably asks. The three
        // description sections are `DeviceDifference`'s to name — one list, extended here
        // with the identity section this type adds, rather than two lists that could come
        // to disagree about the order.
        let Self { info, device } = self;

        let mut out = Vec::new();
        if !info.is_empty() {
            out.push(Self::INFO);
        }
        out.extend(device.sections());
        out
    }

    /// Whether the format tree is the **only** thing that disagrees.
    ///
    /// The predicate the owner's 2026-08-13 ruling turns on, and the only reason this type
    /// exists rather than a bool. A caller answering `true` here is looking at the shape
    /// \[PF:23\] measured twice — once shrinking, once returning whole — and may treat it as
    /// a fact about the device. A caller answering `false` is looking at something no
    /// measurement licenses, **including a formats difference with anything else beside
    /// it**: the ruling is about a device re-deciding what it advertises when its rail comes
    /// up, and a run in which the control set moved *as well* is not that observation, it is
    /// two findings at once and the second one is unexplained.
    ///
    /// **Written as an equality against [`Self::sections`] rather than as a conjunction over
    /// the fields**, and the difference is not style. `formats && info.is_empty() &&
    /// !controls && !measured_pairs` is the obvious spelling and it fails *open*: the day
    /// somebody adds a fifth section to [`ProfileInvariant`], that conjunction keeps
    /// answering `true` for a difference in it, and a hardware arm silently extends the
    /// owner's ruling to a section the owner has never been asked about. The equality
    /// answers `false` for anything it does not recognise, which is the direction a
    /// permission should fail in, and it costs one `Vec` on a path that already allocated
    /// one for the `info` field names.
    #[must_use]
    pub fn is_only_the_format_tree(&self) -> bool {
        self.sections() == [Self::FORMATS]
    }
}

impl fmt::Display for InvariantDifference {
    /// Every disagreeing section, and underneath `info` the fields that disagreed.
    ///
    /// The fields are named for the reason [`crate::pairing::AutomationPair`]'s consumers
    /// name their partner control: a section name on its own tells a reader that something
    /// moved and gives them nowhere to look, and this string's whole job is to be the line
    /// somebody reads in a transcript a week later, on a desk that no longer has the device
    /// on it.
    ///
    /// Built over [`Self::sections`] rather than by walking the fields a second time, so the
    /// membership rule and the order have one home (design §2.10); `info` is the only arm
    /// with anything to add, and it has to recognise itself by name to add it.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let named: Vec<String> = self
            .sections()
            .into_iter()
            .map(|section| {
                if section == Self::INFO {
                    format!("{section} ({})", self.info.join(", "))
                } else {
                    section.to_owned()
                }
            })
            .collect();
        f.write_str(&named.join(", "))
    }
}

impl DeviceProfile {
    /// Which sections of the invariant half disagree with `other`'s, or `None` when none of
    /// them do.
    ///
    /// Compares the invariant section only. Provenance and state are excluded by
    /// construction, which is what lets the G1 criterion — "`profile capture` reproduces
    /// the committed profile" — be true of a camera someone has been using.
    ///
    /// The `info` half goes through [`CameraInfo::differing_fields`] rather than through
    /// `==`, because one field inside this "invariant" section is not invariant:
    /// `/dev/videoN` is probe order, and a `uvcvideo` reload renumbered three of four
    /// attached cameras without any of them changing \[PF:22, note **N63**\]. That
    /// comparison is the one home for the rule; this method is its second consumer, and
    /// spelling the exclusion again here would be the second copy AGENTS forbids.
    ///
    /// This answers with a value rather than a bool because its two hardware consumers no
    /// longer want the same thing from it — see [`InvariantDifference`] for the owner's
    /// 2026-08-13 ruling and for the two PF:23 measurements that say the format tree is the
    /// section a replug moves. [`Self::invariant_matches`] is still here, and still the
    /// right call for anybody who only wants the bool.
    #[must_use]
    pub fn invariant_difference(&self, other: &DeviceProfile) -> Option<InvariantDifference> {
        let comparison = self.compare(other);
        let difference = InvariantDifference {
            info: comparison.identity,
            device: comparison.device,
        };
        // Asked through `sections()` rather than re-spelling "is any of the four set",
        // because "which of these four count as a disagreement" is one rule and this is the
        // place it would quietly grow a second copy.
        (!difference.sections().is_empty()).then_some(difference)
    }

    /// Compare two profiles with identity held to one side (design D15; FR-W2 — "the single
    /// highest-value request in this file").
    ///
    /// **The partition is a destructuring, not a list.** `ProfileInvariant`'s four fields are
    /// bound by name on both sides, so a field added later stops this function compiling
    /// until somebody assigns it a side — closed in both directions by construction, which
    /// is the only mechanical defence this design trusts for a partition. `info` is identity
    /// (where the device is); `formats`, `controls` and `measured_pairs` are description
    /// (what the device is).
    ///
    /// **Identity goes through [`CameraInfo::differing_fields`]**, which is the one home for
    /// "is this the same camera": `/dev/videoN` is probe order and a `uvcvideo` reload
    /// renumbered three of four attached cameras without any of them changing \[PF:22, note
    /// **N63**\], and spelling that exclusion again here would be the second copy AGENTS
    /// forbids.
    ///
    /// **Controls are compared by slug, and order is deliberately not a difference.** The
    /// control walk is `QUERY_EXT_CTRL` in id order, so two captures of one device produce
    /// one order and a *set* that differs is what a device changing shape looks like; naming
    /// the slugs is what makes the answer usable, and a reordering with identical members is
    /// not a fact about the device this comparison will invent. Provenance and state are
    /// excluded by construction — they are outside the invariant section, which is what lets
    /// "the capture reproduces the committed profile" be true of a camera someone has been
    /// using.
    #[must_use]
    pub fn compare(&self, other: &DeviceProfile) -> ProfileComparison {
        // Destructured on both sides so a new field on `ProfileInvariant` stops compiling
        // here until somebody says which half of D15's partition it belongs to — and, since
        // 2026-08-13, until somebody also says whether a run may *decline* over it the way
        // it may decline over `formats`. A field added to the struct and forgotten here
        // would be a section that silently never disagrees.
        let ProfileInvariant {
            info,
            formats,
            controls,
            measured_pairs,
        } = &self.invariant;
        let ProfileInvariant {
            info: other_info,
            formats: other_formats,
            controls: other_controls,
            measured_pairs: other_pairs,
        } = &other.invariant;

        ProfileComparison {
            device: DeviceDifference {
                formats: formats != other_formats,
                controls: differing_control_slugs(controls, other_controls),
                measured_pairs: measured_pairs != other_pairs,
            },
            identity: info.differing_fields(other_info),
        }
    }

    /// Whether two profiles describe the same device, whatever their identity says.
    ///
    /// The bool for callers that only branch; [`Self::compare`] is what they call when the
    /// answer is "no" and they need to say why.
    #[must_use]
    pub fn device_matches(&self, other: &DeviceProfile) -> bool {
        self.compare(other).device_matches()
    }

    /// Whether two profiles describe the same device in the same way.
    ///
    /// The bool [`Self::invariant_difference`] used to be, kept because most of this
    /// method's callers — the engine's own round-trip tests, and every unit arm below — want
    /// nothing more than that and reading a `None` as "matches" at each of them would be the
    /// rule stated over and over.
    #[must_use]
    pub fn invariant_matches(&self, other: &DeviceProfile) -> bool {
        self.invariant_difference(other).is_none()
    }

    /// The control descriptor for a slug, from the invariant section.
    #[must_use]
    pub fn control(&self, slug: &ControlSlug) -> Option<&ControlDesc> {
        self.invariant.controls.iter().find(|c| &c.slug == slug)
    }

    /// A descriptor with the state block's value and flags folded back in — what a
    /// replaying backend hands out as "the control right now".
    #[must_use]
    pub fn live_control(&self, slug: &ControlSlug) -> Option<ControlDesc> {
        use crate::control::ControlFlags;

        let mut desc = self.control(slug)?.clone();
        if let Some(value) = self.state.values.get(slug) {
            desc.current = Some(value.clone());
        }
        if let Some(raw) = self.state.flags.get(slug) {
            desc.flags = ControlFlags::from_raw(*raw);
        }
        Some(desc)
    }

    /// A profile with its state block replaced by the invariant defaults — used when a
    /// replay wants to start from a clean device rather than from whatever the capture
    /// caught.
    #[must_use]
    pub fn with_reset_state(&self) -> DeviceProfile {
        let mut out = self.clone();
        out.state = ProfileState {
            values: out
                .invariant
                .controls
                .iter()
                .filter(|c| c.control_type.is_scalar())
                .map(|c| (c.slug.clone(), ControlValue::Int(c.default)))
                .collect(),
            flags: out
                .invariant
                .controls
                .iter()
                .map(|c| (c.slug.clone(), c.flags.raw))
                .collect(),
        };
        out
    }

    /// Whether this document's version is one this build reads.
    #[must_use]
    pub fn version_is_supported(&self) -> bool {
        self.schema_version == limits::PROFILE_SCHEMA_VERSION
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::{CameraFingerprint, CameraId};
    use crate::control::{ControlFlags, ControlId, ControlRange, ControlType};

    fn control(slug: &str, raw_flags: u32, current: i64) -> ControlDesc {
        ControlDesc {
            id: ControlId(1),
            name: slug.to_owned(),
            slug: ControlSlug::parse(slug).expect("literal slug"),
            control_type: ControlType::Integer,
            range: ControlRange {
                min: 0,
                max: 100,
                step: 1,
            },
            default: 50,
            flags: ControlFlags::from_raw(raw_flags),
            menu: BTreeMap::new(),
            elems: 1,
            elem_size: 4,
            dims: Vec::new(),
            current: Some(ControlValue::Int(current)),
        }
    }

    fn profile(controls: Vec<ControlDesc>) -> DeviceProfile {
        let state = ProfileState {
            values: controls
                .iter()
                .filter_map(|c| c.current.clone().map(|v| (c.slug.clone(), v)))
                .collect(),
            flags: controls
                .iter()
                .map(|c| (c.slug.clone(), c.flags.raw))
                .collect(),
        };
        DeviceProfile {
            schema_version: limits::PROFILE_SCHEMA_VERSION,
            provenance: ProfileProvenance {
                captured_at: Stamp::epoch(),
                kernel: "7.0.0-29-generic".to_owned(),
                tool_version: "0.1.0".to_owned(),
                capturer: "test".to_owned(),
                backend: BackendKind::V4l2,
            },
            invariant: ProfileInvariant {
                info: CameraInfo {
                    id: CameraId::parse("cam:test").expect("literal id"),
                    fingerprint: CameraFingerprint {
                        bus_path: "3-1:1.0".to_owned(),
                        usb_id: None,
                        card: "Test".to_owned(),
                        driver: "uvcvideo".to_owned(),
                        serial: None,
                    },
                    card: "Test".to_owned(),
                    driver: "uvcvideo".to_owned(),
                    bus_info: "usb-3-1".to_owned(),
                    nodes: Vec::new(),
                    backend: BackendKind::V4l2,
                },
                formats: Vec::new(),
                controls: controls.iter().map(invariant_control).collect(),
                measured_pairs: Vec::new(),
            },
            state,
        }
    }

    /// A profile of a two-node camera sitting at the given paths.
    ///
    /// The caps words are the OBSBOT's, measured 2026-08-11: a capture node and a
    /// metadata node, unchanged across the reload that moved them.
    fn profile_with_nodes(paths: &[&str; 2]) -> DeviceProfile {
        use crate::camera::{DeviceNode, NodeKind};

        let mut out = profile(Vec::new());
        out.invariant.info.nodes = vec![
            DeviceNode {
                path: paths[0].into(),
                kind: NodeKind::VideoCapture,
                device_caps: 0x0420_0001,
                capabilities: 0x84a0_0001,
            },
            DeviceNode {
                path: paths[1].into(),
                kind: NodeKind::MetaCapture,
                device_caps: 0x0480_0000,
                capabilities: 0x84a0_0001,
            },
        ];
        out
    }

    #[test]
    fn using_the_camera_does_not_read_as_corpus_drift() {
        // The T3 split, as the property it exists for: the same device, sampled before
        // and after somebody used it, must compare equal on the invariant section.
        let fresh = profile(vec![control("white_balance_temperature", 0x1000, 4600)]);
        // Later: automation was switched on, so INACTIVE is set and the value moved.
        let used = profile(vec![control("white_balance_temperature", 0x1010, 6500)]);

        assert!(
            fresh.invariant_matches(&used),
            "state changes must not count as drift"
        );
        assert_ne!(fresh.state, used.state, "…but the state block did change");
    }

    #[test]
    fn renumbering_the_nodes_does_not_read_as_corpus_drift_either() {
        // The second consumer of `CameraInfo::differing_fields`, and the reason it has to
        // be the *same* function: this is the arm that never ran on 2026-08-11, because
        // the enumeration arm failed first and the suite stopped. `capture` copies
        // `camera.info()` verbatim, so a `self.invariant == other.invariant` compared the
        // kernel's node names as well [PF:22, note N63].
        let committed = profile_with_nodes(&["/dev/video4", "/dev/video5"]);
        let after_reload = profile_with_nodes(&["/dev/video0", "/dev/video1"]);
        assert_ne!(
            committed.invariant.info.nodes, after_reload.invariant.info.nodes,
            "the fixture has to differ in the field under test"
        );
        assert!(
            committed.invariant_matches(&after_reload),
            "a uvcvideo reload renamed the nodes; nothing about the device moved"
        );

        // The inverse, at this layer rather than at the schema's: a node that vanished is
        // a device that changed, and this method has to carry that through.
        let mut lost = after_reload.clone();
        lost.invariant.info.nodes.pop();
        assert!(!committed.invariant_matches(&lost));

        // …and the halves that still compare exactly are still compared exactly, so
        // routing `info` through a rule cannot be mistaken for loosening the section.
        let mut refprofile = committed.clone();
        refprofile
            .invariant
            .formats
            .push(crate::camera::FormatInfo {
                pixel_format: crate::camera::PixelFormat::MJPG,
                description: "Motion-JPEG".to_owned(),
                flags: 0,
                sizes: Vec::new(),
            });
        assert!(!committed.invariant_matches(&refprofile));
    }

    /// A profile of a camera offering one MJPG format with the given modes, each mode a
    /// discrete size and the whole-number frame rates available *at that size*.
    ///
    /// The shape is the OBSBOT's, because that is the device PF:23 measured: sizes nested
    /// under a pixel format and intervals nested under a size, so a mode can vanish at
    /// either of the two inner levels.
    fn profile_offering(modes: &[(u32, u32, &[u32])]) -> DeviceProfile {
        use crate::camera::{FormatInfo, FrameInterval, FrameSize, FrameSizeInfo, PixelFormat};

        let mut out = profile(Vec::new());
        out.invariant.formats = vec![FormatInfo {
            pixel_format: PixelFormat::MJPG,
            description: "Motion-JPEG".to_owned(),
            // V4L2_FMT_FLAG_COMPRESSED.
            flags: 0x0001,
            sizes: modes
                .iter()
                .map(|&(width, height, rates)| FrameSizeInfo {
                    size: FrameSize::Discrete { width, height },
                    intervals: rates
                        .iter()
                        .map(|&fps| FrameInterval::Discrete {
                            numerator: 1,
                            denominator: fps,
                        })
                        .collect(),
                })
                .collect(),
        }];
        out
    }

    #[test]
    fn a_mode_the_device_stopped_advertising_reads_as_drift() {
        // PF:23 as a property rather than as a transcript. Between 2026-08-08 and
        // 2026-08-11 the OBSBOT Tiny 3 stopped advertising 3840×2160 at all and stopped
        // offering 120 fps at the two sizes it kept — one loss at the *size* level and
        // one at the *interval* level, from a device whose `CameraInfo` half was
        // byte-identical across the two captures. Only the format tree moved.
        //
        // Both arms pass today, because `invariant_matches` compares `formats` with `==`.
        // They are written down anyway, for the reason N63 gives for writing down its
        // over-correction: the obvious next change to this method is a `formats` diff
        // that *names* what moved, the way the `info` half got one, and such a diff has a
        // characteristic wrong shape — it compares the size list and forgets that the
        // intervals hang off it. The second arm is the one that catches that.
        //
        // 2026-08-13 took half of that step and not this half: `invariant_difference` names
        // the *section*, so a reader is told the format tree moved, and it still compares
        // the tree with `==`, so nothing here says which size or which interval. The warning
        // above therefore still stands as written, against the diff that has not been built.
        let modes: &[(u32, u32, &[u32])] = &[(1920, 1080, &[120, 60, 30]), (3840, 2160, &[30])];
        let committed = profile_offering(modes);

        // Non-vacuity first: two captures of an unchanged device still match. Without
        // this, a builder that made every profile differ would satisfy both refusals.
        assert!(
            committed.invariant_matches(&profile_offering(modes)),
            "the same modes twice are not drift"
        );

        // The size that vanished.
        let no_4k = profile_offering(&[(1920, 1080, &[120, 60, 30])]);
        assert!(
            !committed.invariant_matches(&no_4k),
            "a size the device stopped offering is the device changing shape"
        );

        // The interval that vanished, at a size the device kept. The size *list* is
        // identical on both sides here, which is what makes this the arm a comparison
        // written one level too shallow would let through.
        let no_120 = profile_offering(&[(1920, 1080, &[60, 30]), (3840, 2160, &[30])]);
        let sizes = |p: &DeviceProfile| -> Vec<crate::camera::FrameSize> {
            p.invariant
                .formats
                .iter()
                .flat_map(|f| f.sizes.iter().map(|entry| entry.size))
                .collect()
        };
        assert_eq!(
            sizes(&committed),
            sizes(&no_120),
            "this fixture has to differ from the committed one *only* in its intervals"
        );
        assert!(
            !committed.invariant_matches(&no_120),
            "a frame rate the device stopped offering is drift too, and it hides one \
             level deeper than the size does"
        );
    }

    // ------------------- which section moved, under the owner's 2026-08-13 ruling
    //
    // The ruling is that a camera's advertised support may change each time it is plugged
    // in, and PF:23 is what says *where* that shows up: twice now — once shrinking, once
    // returning whole — the OBSBOT Tiny 3's format tree moved while its identity and its 24
    // controls did not. A hardware arm may therefore decline over a formats-only difference
    // and must not decline over any other, so the difference between "only the format tree"
    // and "the format tree and something else" is a load-bearing distinction and gets tests
    // over values. These are the arms that survive the corpus being re-captured to today's
    // tree: after that the hardware arm goes green and stops exercising the decline at all,
    // and the next plug event that reaches it will be somebody else's session.

    #[test]
    fn a_formats_only_difference_is_named_as_one_and_not_as_a_match() {
        // The device fact, as a value. The two sides differ in the tree and in nothing
        // else, which is exactly PF:23's two observations, and all three of the questions a
        // caller can ask have to agree about that.
        let modes: &[(u32, u32, &[u32])] = &[(1920, 1080, &[120, 60, 30]), (3840, 2160, &[30])];
        let committed = profile_offering(&[(1920, 1080, &[60, 30])]);
        let fresh = profile_offering(modes);

        // Non-vacuity, first and in the same shape the arm above uses: the builder has to be
        // capable of producing two profiles that agree, or every refusal below is free.
        assert_eq!(
            committed.invariant_difference(&profile_offering(&[(1920, 1080, &[60, 30])])),
            None,
            "two captures of an unchanged device disagree in no section at all"
        );

        let difference = committed
            .invariant_difference(&fresh)
            .expect("a tree that gained a size and a rate is a difference");
        assert_eq!(difference.sections(), ["formats"]);
        assert!(difference.is_only_the_format_tree());
        assert_eq!(difference.to_string(), "formats");
        // And the bool the old callers read is still the old bool: a difference is a
        // difference, whatever a hardware arm then decides to do about it. `is_none()` over
        // a value is only a refactor if this stays true.
        assert!(!committed.invariant_matches(&fresh));
    }

    #[test]
    fn a_control_set_that_moved_is_not_the_ruling_the_format_tree_gets() {
        // The inverse, and the one that matters most: the ruling is about a device
        // re-deciding what it *advertises*, and PF:23 measured the control set holding still
        // through both of those events — "all 24 controls, byte for byte". A difference that
        // reaches the controls is therefore outside what anybody has licensed, and a
        // predicate that answered `true` here would convert an unexplained finding into a
        // green run with a skip line on it.
        let before = profile(vec![control("brightness", 0, 50)]);
        let mut moved = vec![control("brightness", 0, 50)];
        moved[0].range = ControlRange {
            min: 0,
            max: 255,
            step: 1,
        };
        let after = profile(moved.clone());

        let difference = before
            .invariant_difference(&after)
            .expect("a control whose range moved is a difference");
        assert_eq!(difference.sections(), ["controls"]);
        assert!(
            !difference.is_only_the_format_tree(),
            "a control set that moved is not a format tree that moved"
        );

        // …and the same refusal when the format tree moved *as well*, which is the shape a
        // predicate written as "no controls difference unless" would still wave through in
        // one direction. Two findings in one capture is not the observation PF:23 recorded;
        // it is that observation with an unexplained one sitting beside it.
        let mut both = profile_offering(&[(1920, 1080, &[60, 30])]);
        both.invariant.controls = moved.iter().map(invariant_control).collect();
        let mut only_formats = profile_offering(&[(1920, 1080, &[120, 60, 30])]);
        only_formats.invariant.controls = before.invariant.controls.clone();

        let difference = only_formats
            .invariant_difference(&both)
            .expect("a tree and a control set that both moved is a difference");
        assert_eq!(difference.sections(), ["formats", "controls"]);
        assert!(
            !difference.is_only_the_format_tree(),
            "the format tree moving does not license whatever moved with it"
        );
        assert_eq!(difference.to_string(), "formats, controls");
    }

    #[test]
    fn an_identity_difference_names_the_fields_it_found_and_is_not_the_format_tree_either() {
        // The third section, and the reason this value is not a list of section names: the
        // `info` half disagrees by *field*, `CameraInfo::differing_fields` is the one home
        // for which fields count \[PF:22, note N63\], and a caller printing "info" alone
        // would be telling a reader that a camera changed and giving them nowhere to look.
        let committed = profile_with_nodes(&["/dev/video4", "/dev/video5"]);
        let mut lost = profile_with_nodes(&["/dev/video0", "/dev/video1"]);
        lost.invariant.info.nodes.pop();

        let difference = committed
            .invariant_difference(&lost)
            .expect("a camera that lost a node is a difference");
        assert_eq!(difference.sections(), ["info"]);
        assert!(!difference.is_only_the_format_tree());
        assert_eq!(
            difference.to_string(),
            "info (nodes.len)",
            "the section on its own is not an answer a reader can act on"
        );

        // The renumbering that started all of this is still not a difference at all, so
        // routing the `info` half through a rule survived being given a richer answer type.
        assert_eq!(
            committed.invariant_difference(&profile_with_nodes(&["/dev/video0", "/dev/video1"])),
            None
        );

        // And identity beside a moved tree is still not the ruling, which needs saying
        // separately: the two arms above both leave `formats` false, so neither of them can
        // tell `formats && info.is_empty() && …` from `formats && …`. This is the only place
        // the `info` conjunct is load-bearing, and a camera whose identity moved is the one
        // difference nobody may wave through — the corpus is then not describing this device
        // at all, and whatever its format tree says is an answer to a different question.
        let mut renamed = profile_offering(&[(1920, 1080, &[60, 30])]);
        renamed.invariant.info.card = "Some Other Camera".to_owned();
        let difference = profile_offering(&[(1920, 1080, &[120, 60, 30])])
            .invariant_difference(&renamed)
            .expect("a different camera offering a different tree is a difference");
        assert_eq!(difference.sections(), ["info", "formats"]);
        assert!(
            !difference.is_only_the_format_tree(),
            "a format tree read off a camera that is not the committed one is not evidence \
             about the committed one"
        );
        assert_eq!(difference.to_string(), "info (card), formats");
    }

    #[test]
    fn a_changed_range_does_read_as_drift() {
        // The inverse direction: if the *device* changes, the corpus must notice.
        let before = profile(vec![control("brightness", 0, 50)]);
        let mut after_controls = vec![control("brightness", 0, 50)];
        after_controls[0].range = ControlRange {
            min: 0,
            max: 255,
            step: 1,
        };
        let after = profile(after_controls);
        assert!(!before.invariant_matches(&after));
    }

    #[test]
    fn live_controls_fold_the_state_block_back_in() {
        let p = profile(vec![control("zoom_continuous", 0x1000, 245)]);
        let slug = ControlSlug::parse("zoom_continuous").expect("literal slug");

        // The invariant copy has no current value and no volatile flags.
        let inv = p.control(&slug).expect("control present");
        assert_eq!(inv.current, None);

        // The live copy has both — including the out-of-range current [PF:4].
        let live = p.live_control(&slug).expect("control present");
        assert_eq!(live.current, Some(ControlValue::Int(245)));
        assert!(live.current_out_of_range());
    }

    #[test]
    fn resetting_state_returns_every_scalar_control_to_its_default() {
        let p = profile(vec![control("brightness", 0, 17)]).with_reset_state();
        let slug = ControlSlug::parse("brightness").expect("literal slug");
        assert_eq!(
            p.live_control(&slug).expect("control").current,
            Some(ControlValue::Int(50))
        );
    }

    #[test]
    fn a_foreign_version_is_recognizable_before_anything_else_is_read() {
        let mut p = profile(Vec::new());
        assert!(p.version_is_supported());
        p.schema_version = 99;
        assert!(!p.version_is_supported());
    }

    #[test]
    fn a_profile_round_trips_through_json() {
        let p = profile(vec![control("brightness", 0x1000, 50)]);
        let json = serde_json::to_string_pretty(&p).expect("serialize");
        let back: DeviceProfile = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, p);
    }

    // ------------------------------------------------------- D15: the masked device compare

    #[test]
    fn a_profile_is_the_same_device_as_its_identity_rewritten_self() {
        // FR-W2's question, in its simplest true form: the same camera reached over a
        // forwarded bus reports a different bus path, a different card ordinal and a
        // different serial, and every one of those is identity. The description must not
        // move, and `device_matches` is the sentence a consumer branches on.
        let mine = profile(vec![control("brightness", 0, 50)]);
        let mut forwarded = mine.clone();
        forwarded.invariant.info.fingerprint.bus_path = "1-2:1.0".to_owned();
        forwarded.invariant.info.bus_info = "usb-1-2".to_owned();
        forwarded.invariant.info.id = CameraId::parse("cam:test-2").expect("literal id");

        let comparison = mine.compare(&forwarded);
        assert!(comparison.device_matches(), "{comparison}");
        assert!(mine.device_matches(&forwarded));
        assert_eq!(
            comparison.identity,
            vec!["fingerprint.bus_path".to_owned(), "bus_info".to_owned()],
            "the identity half must name what moved rather than merely disagreeing"
        );
        // And the whole-invariant comparison still calls that a difference, which is the
        // point of having two answers: `invariant_difference` is about the corpus, `compare`
        // is about the device.
        assert!(mine.invariant_difference(&forwarded).is_some());
    }

    #[test]
    fn a_control_the_other_side_describes_differently_is_named_by_its_slug() {
        let mine = profile(vec![
            control("brightness", 0, 50),
            control("contrast", 0, 50),
        ]);
        let mut theirs = mine.clone();
        theirs.invariant.controls[1].range.max = 200;

        let comparison = mine.compare(&theirs);
        assert!(!comparison.device_matches());
        assert_eq!(
            comparison.device.controls,
            vec![ControlSlug::parse("contrast").expect("literal slug")]
        );
        assert_eq!(comparison.device.sections(), vec!["controls"]);
        assert!(comparison.to_string().contains("controls (contrast)"));
    }

    #[test]
    fn a_control_only_one_side_has_is_a_difference_in_both_directions() {
        // Both directions, because a comparison that only noticed additions would call a
        // camera that lost a control the same device.
        let fewer = profile(vec![control("brightness", 0, 50)]);
        let more = profile(vec![control("brightness", 0, 50), control("gain", 0, 50)]);
        let gain = ControlSlug::parse("gain").expect("literal slug");

        assert_eq!(fewer.compare(&more).device.controls, vec![gain.clone()]);
        assert_eq!(more.compare(&fewer).device.controls, vec![gain]);
    }

    #[test]
    fn the_same_controls_in_another_order_are_the_same_device() {
        // Stated rather than discovered, because it is a deliberate narrowing of the
        // whole-invariant `==` this comparison replaced: the control walk is id-ordered, so
        // one device produces one order, and a set that differs is what a device changing
        // shape looks like. Order is not a fact about the device this answer will invent.
        let mine = profile(vec![
            control("brightness", 0, 50),
            control("contrast", 0, 50),
        ]);
        let mut reordered = mine.clone();
        reordered.invariant.controls.reverse();

        assert!(mine.device_matches(&reordered));
        assert!(mine.compare(&reordered).device.controls.is_empty());
    }

    #[test]
    fn a_format_tree_difference_is_reported_as_the_only_one_it_is() {
        // The owner's 2026-08-13 ruling, carried on the D15 answer: a consumer applies its
        // own policy to a formats-only delta, and this predicate is what it branches on.
        let mine = profile(vec![control("brightness", 0, 50)]);
        let mut theirs = mine.clone();
        theirs.invariant.formats.push(crate::camera::FormatInfo {
            pixel_format: crate::camera::PixelFormat(*b"MJPG"),
            description: "Motion-JPEG".to_owned(),
            flags: 0,
            sizes: Vec::new(),
        });

        let comparison = mine.compare(&theirs);
        assert!(!comparison.device_matches());
        assert!(comparison.device_differs_only_in_the_format_tree());
        assert_eq!(comparison.device.sections(), vec!["formats"]);

        // And with anything beside it, the permission goes away — the ruling is about a
        // device re-deciding what it advertises, not about two findings at once.
        theirs.invariant.controls[0].range.max = 200;
        assert!(
            !mine
                .compare(&theirs)
                .device_differs_only_in_the_format_tree()
        );
    }

    #[test]
    fn an_identity_only_difference_is_no_device_difference_at_all() {
        // The inverse of the format-tree arm and the reason the two halves are separate: an
        // identity delta must never make `device_differs_only_in_the_format_tree` false,
        // because it is not a section of the device half.
        let mine = profile(vec![control("brightness", 0, 50)]);
        let mut theirs = mine.clone();
        theirs.invariant.info.fingerprint.bus_path = "9-9:1.0".to_owned();

        let comparison = mine.compare(&theirs);
        assert!(comparison.device_matches());
        assert!(comparison.device.is_empty());
        assert_eq!(comparison.device.sections(), Vec::<&str>::new());
        assert_eq!(
            comparison.to_string(),
            "same device; identity differs: fingerprint.bus_path"
        );
    }

    #[test]
    fn an_identity_delta_beside_a_format_tree_delta_is_still_only_the_format_tree() {
        // The clause neither arm above can reach on its own: one of them moves identity with
        // an empty device half, the other moves the format tree with an empty identity half,
        // and "an identity delta never makes the format-tree permission go away" is a claim
        // about the pair. It is also the pair the ruling is *for* — a replug is where a bus
        // path moves, so a `compare` that let the identity half into `verdict()` would answer
        // `Differs` about exactly the two captures the owner's 2026-08-13 ruling licenses, and
        // it would do it on the everyday shape rather than on a contrived one.
        let mine = profile(vec![control("brightness", 0, 50)]);
        let mut theirs = mine.clone();
        theirs.invariant.info.fingerprint.bus_path = "9-9:1.0".to_owned();
        theirs.invariant.formats.push(crate::camera::FormatInfo {
            pixel_format: crate::camera::PixelFormat(*b"MJPG"),
            description: "Motion-JPEG".to_owned(),
            flags: 0,
            sizes: Vec::new(),
        });

        let comparison = mine.compare(&theirs);
        assert!(
            comparison.device_differs_only_in_the_format_tree(),
            "a camera whose bus path moved lost the owner's format-tree ruling: {comparison}"
        );
        assert_eq!(comparison.verdict(), DeviceVerdict::OnlyTheFormatTree);
        assert_eq!(comparison.device.sections(), vec!["formats"]);
        // The identity half beside it, because an arm whose identity delta went missing would
        // be the formats-only arm above wearing another name and would hold nothing.
        assert_eq!(
            comparison.identity,
            vec!["fingerprint.bus_path".to_owned()],
            "the identity half stopped naming the field that moved: {comparison}"
        );
    }

    /// A profile whose invariant carries one measured automation pair.
    ///
    /// The shape D3's probe produces on the seed hardware — a manual control, the automation
    /// that has to be switched off first, how to switch it off, and `Measured` because a probe
    /// saw it — so the section this exercises is compared in the form it is actually captured
    /// in rather than in a placeholder.
    fn profile_pairing(manual: &str, automation: &str) -> DeviceProfile {
        use crate::pairing::{AutomationOff, Provenance};

        let mut out = profile(vec![control(manual, 0, 50)]);
        out.invariant.measured_pairs = vec![AutomationPair {
            manual: ControlSlug::parse(manual).expect("literal slug"),
            automation: ControlSlug::parse(automation).expect("literal slug"),
            off: AutomationOff::Value { value: 0 },
            provenance: Provenance::Measured,
        }];
        out
    }

    #[test]
    fn a_pair_set_the_two_sides_measured_differently_is_a_device_difference() {
        // The third device section, which was implemented, reached production and was
        // asserted by nothing: `sections()`'s `measured_pairs` arm could be deleted, or the
        // field hardwired to `false` in `compare`, and the whole workspace stayed green
        // (note **N287**).
        //
        // It belongs to the description half and not to identity for D3's reason: which
        // control has to be switched off before another can be driven by hand is a fact about
        // what the device *is*, measured on the device itself \[PF:3\], and a forwarded camera
        // whose pairs moved is not the same camera however its bus path reads.
        let mine = profile_pairing("exposure_time_absolute", "auto_exposure");
        let mut theirs = mine.clone();
        theirs.invariant.measured_pairs[0].automation =
            ControlSlug::parse("exposure_auto_priority").expect("literal slug");

        // Non-vacuity first: the two profiles differ in this section and in nothing else, so
        // an assertion below that named another section would be naming a fixture defect.
        assert_ne!(
            mine.invariant.measured_pairs, theirs.invariant.measured_pairs,
            "the fixture has to differ in the section under test"
        );
        assert_eq!(
            mine.invariant.formats, theirs.invariant.formats,
            "the fixture has to differ in the section under test and in no other"
        );
        assert_eq!(
            mine.invariant.controls, theirs.invariant.controls,
            "the fixture has to differ in the section under test and in no other"
        );

        let comparison = mine.compare(&theirs);
        assert!(
            !comparison.device_matches(),
            "a pair set the two sides measured differently is a device difference, so this is \
             not the same device — {comparison}"
        );
        assert!(
            comparison.device.measured_pairs,
            "the pair section did not notice a pair set that moved — {comparison}"
        );
        assert_eq!(
            comparison.device.sections(),
            vec!["measured_pairs"],
            "a measured-pair difference has to be the only section named — {comparison}"
        );
        assert!(
            !comparison.device_differs_only_in_the_format_tree(),
            "the owner's 2026-08-13 format-tree permission was granted to a pair-set \
             difference — {comparison}"
        );
        assert_eq!(
            comparison.verdict(),
            DeviceVerdict::Differs,
            "a pair set that moved is not a verdict the ruling licenses — {comparison}"
        );
        assert_eq!(
            comparison.to_string(),
            "device differs: measured_pairs",
            "the human line does not name the section the comparison found"
        );

        // Both directions, because a diff that noticed a pair one way round and not the other
        // would be an answer that depended on which profile was passed first.
        assert_eq!(
            theirs.compare(&mine).device.sections(),
            vec!["measured_pairs"],
            "the pair-set difference is reported one way round and not the other"
        );

        // And a pair set only one side measured at all, which is what a capture taken without
        // `--discover-pairs` looks like beside one taken with it.
        let unprobed = profile(vec![control("exposure_time_absolute", 0, 50)]);
        assert!(
            unprobed.invariant.measured_pairs.is_empty(),
            "the unprobed fixture has to carry no pairs, or it is not the capture under test"
        );
        assert_eq!(
            unprobed.compare(&mine).device.sections(),
            vec!["measured_pairs"],
            "a capture taken without `--discover-pairs` beside one taken with it is a \
             pair-set difference and nothing else"
        );
        assert_eq!(
            mine.compare(&unprobed).device.sections(),
            vec!["measured_pairs"],
            "a capture taken without `--discover-pairs` beside one taken with it is a \
             pair-set difference the other way round too"
        );
    }

    #[test]
    fn a_pair_set_beside_the_format_tree_is_not_the_plug_event_permission() {
        // The permission the owner's 2026-08-13 ruling grants is about a device re-deciding
        // what it advertises, and a run in which the measured pairs moved *as well* is two
        // findings at once. Written over `measured_pairs` and not only over `controls`,
        // because a `verdict` rewritten as a conjunction that forgot this field would grant
        // the permission here and nowhere a controls-shaped arm could see it.
        let mine = profile_pairing("exposure_time_absolute", "auto_exposure");
        let mut theirs = mine.clone();
        theirs.invariant.measured_pairs.clear();
        theirs.invariant.formats.push(crate::camera::FormatInfo {
            pixel_format: crate::camera::PixelFormat::MJPG,
            description: "Motion-JPEG".to_owned(),
            flags: 0,
            sizes: Vec::new(),
        });

        let comparison = mine.compare(&theirs);
        assert_eq!(
            comparison.device.sections(),
            vec!["formats", "measured_pairs"],
            "the fixture moved the format tree and the pair set, and the comparison names \
             something else — {comparison}"
        );
        assert!(
            !comparison.device_differs_only_in_the_format_tree(),
            "a pair set that moved beside the format tree was given the owner's plug-event \
             permission — {comparison}"
        );
        assert_eq!(
            comparison.verdict(),
            DeviceVerdict::Differs,
            "two findings at once were given a verdict that licenses one — {comparison}"
        );
    }

    /// The three verdicts, the profile pair that produces each, and the word the document
    /// spells it with.
    ///
    /// Written down rather than derived from [`DeviceDifference::verdict`], because an
    /// expectation computed by the function under test is red-able in one direction only
    /// (note **N252**): each row's verdict is read off the *design's* three cases — nothing
    /// moved, the format tree alone moved, something the ruling does not license moved — and
    /// each row's word is the serde spelling a `--json` consumer matches on.
    fn every_verdict_with_the_pair_that_produces_it() -> Vec<(
        &'static str,
        DeviceProfile,
        DeviceProfile,
        DeviceVerdict,
        &'static str,
    )> {
        let base = profile(vec![control("brightness", 0, 50)]);

        let mut moved = base.clone();
        moved.invariant.info.fingerprint.bus_path = "9-9:1.0".to_owned();

        let mut fewer_modes = base.clone();
        fewer_modes
            .invariant
            .formats
            .push(crate::camera::FormatInfo {
                pixel_format: crate::camera::PixelFormat::MJPG,
                description: "Motion-JPEG".to_owned(),
                flags: 0,
                sizes: Vec::new(),
            });

        let mut reshaped = base.clone();
        reshaped.invariant.controls[0].range.max = 200;

        vec![
            (
                "the same capture reached over a forwarded bus",
                base.clone(),
                moved,
                DeviceVerdict::SameDevice,
                "same_device",
            ),
            (
                "one pixel format more and nothing else",
                base.clone(),
                fewer_modes,
                DeviceVerdict::OnlyTheFormatTree,
                "only_the_format_tree",
            ),
            (
                "a control the two sides describe differently",
                base,
                reshaped,
                DeviceVerdict::Differs,
                "differs",
            ),
        ]
    }

    #[test]
    fn the_json_document_carries_the_verdict_and_not_only_the_halves_it_reads() {
        // D15's answer for the reader D15 names. `ProfileComparison` is a schema DTO because
        // §1.3's sibling harness pins this library *and* shells out to `--json`; before note
        // **N286** that reader got `device` and `identity` and had to rebuild the format-tree
        // permission itself, and the only spelling available to it was the conjunction over
        // the three device fields that note **N89** says keeps answering `true` the day a
        // fifth section lands. So the verdict has to be *in the bytes*, and this arm reads it
        // out of the bytes rather than off the Rust value.
        for (what, mine, theirs, expected, word) in every_verdict_with_the_pair_that_produces_it() {
            let comparison = mine.compare(&theirs);
            assert_eq!(comparison.verdict(), expected, "{what}");

            let document: serde_json::Value =
                serde_json::from_str(&serde_json::to_string(&comparison).expect("serialize"))
                    .expect("the comparison is an object");
            assert_eq!(
                document.get("verdict").and_then(serde_json::Value::as_str),
                Some(word),
                "{what}: the verdict a Rust caller reads is absent from the document a \
                 subprocess consumer reads — {document}"
            );

            // …and the two bools are readings of that one field rather than two more
            // computations beside it, which is what stops the document and the table coming
            // to disagree about one comparison.
            assert_eq!(
                comparison.device_matches(),
                expected == DeviceVerdict::SameDevice,
                "{what}"
            );
            assert_eq!(
                comparison.device_differs_only_in_the_format_tree(),
                expected == DeviceVerdict::OnlyTheFormatTree,
                "{what}"
            );
        }
    }

    #[test]
    fn a_document_whose_verdict_disagrees_with_its_sections_is_refused() {
        // The half computing the word at write time cannot give, and the `Failure`/`failed`
        // shape one document along: the bytes arrive from somewhere else, so the verdict is
        // checked against the sections beside it on the way *in* as well. Both directions,
        // because a deserializer that refused everything would satisfy the refusing half and
        // destroy the type.
        let (_, mine, theirs, _, _) = every_verdict_with_the_pair_that_produces_it()
            .into_iter()
            .find(|(_, _, _, verdict, _)| *verdict == DeviceVerdict::Differs)
            .expect("the table carries a differing pair");
        let honest = serde_json::to_string(&mine.compare(&theirs)).expect("serialize");
        assert!(honest.contains("\"differs\""), "{honest}");

        // The accepting direction first.
        let back: ProfileComparison = serde_json::from_str(&honest).expect("the honest bytes");
        assert_eq!(back.verdict(), DeviceVerdict::Differs);

        // Each of the two permissions a hand-written document could help itself to, refused by
        // name. `same_device` over a non-empty device half is a comparison denying its own
        // contents; `only_the_format_tree` over a control difference is the owner's ruling
        // extended to an observation nobody made.
        for claimed in ["same_device", "only_the_format_tree"] {
            let doctored = honest.replace("\"differs\"", &format!("\"{claimed}\""));
            assert_ne!(doctored, honest, "the doctoring has to change the bytes");
            let refusal = serde_json::from_str::<ProfileComparison>(&doctored)
                .expect_err("a comparison that says a verdict its sections do not support");
            let said = refusal.to_string();
            assert!(
                said.contains("controls"),
                "the refusal has to name the sections that contradict the verdict: {said}"
            );
        }

        // And a document with no verdict at all is refused rather than defaulted, because a
        // missing field silently read as `same_device` is the fail-open direction wearing an
        // absence.
        let stripped = r#"{"device":{"formats":false,"measured_pairs":false}}"#;
        serde_json::from_str::<ProfileComparison>(stripped)
            .expect_err("a comparison with no verdict at all");
    }

    #[test]
    fn a_comparison_round_trips_through_json_because_a_subprocess_consumer_reads_it() {
        let mine = profile(vec![control("brightness", 0, 50)]);
        let mut theirs = mine.clone();
        theirs.invariant.controls[0].range.max = 200;
        theirs.invariant.info.fingerprint.bus_path = "9-9:1.0".to_owned();

        let comparison = mine.compare(&theirs);
        let json = serde_json::to_string_pretty(&comparison).expect("serialize");
        let back: ProfileComparison = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, comparison);
        // The verdict is part of what round-trips, and it is a field of the *document* and
        // not of the value: a consumer that deserialized one has to be able to read the answer
        // it branched on, which is why the value answers it with a method rather than by
        // carrying a copy of the word it was handed.
        assert_eq!(
            back.verdict(),
            comparison.verdict(),
            "the comparison that came back answers a different verdict from the one that went \
             out"
        );
        assert_eq!(
            back.verdict(),
            DeviceVerdict::Differs,
            "a control described differently on the two sides is not a verdict the owner's \
             ruling licenses"
        );

        // …and the default is the one verdict an empty device half supports, asserted rather
        // than assumed: `ProfileComparison::default()` is public API, and a default that said
        // anything else would be a document contradicting its own contents from the moment it
        // was built.
        let empty = ProfileComparison::default();
        assert!(
            empty.device.is_empty(),
            "a fresh comparison has to carry an empty device half"
        );
        assert_eq!(
            empty.verdict(),
            DeviceVerdict::SameDevice,
            "the default comparison says something its own empty device half does not support"
        );
        assert!(
            empty.device_matches(),
            "the default comparison is not the same device as itself"
        );
    }

    #[test]
    fn the_invariant_projection_keeps_a_payloads_shape_because_pf17_is_not_absorbed() {
        // PF:17's other half, and the one no arm held until 2026-08-20. `corpus_replay`'s
        // `a_compound_controls_element_count_is_a_device_difference_because_pf17_is_not_absorbed`
        // holds the *comparison* side — a committed profile is compared as it was loaded, so
        // masking the shape in `differing_control_slugs` reddens it. This function runs at
        // *capture*, so stripping `elems`, `elem_size` and `dims` here leaves that arm green
        // while making every profile already in `corpus/` disagree with its own re-capture —
        // which is the symptom PF:17 opened with.
        //
        // Measured at workspace scope on 2026-08-20 by making that edit: the `corpus_replay`
        // arm passed, and the four arms that did go red said `the_committed_document_matches_
        // the_constructor`, `an_unknown_control_round_trips_its_opaque_payload` and two
        // downstream of them. Every one of those reads as *a fixture to re-bless*, which is
        // the repair a reader takes from them and the wrong one — so the shape needs a
        // sentence that names the rule rather than the fixture.
        //
        // So the capture side gets its own sentence to go red on. PF:17's **Retires when:**
        // clause conditions the split on a device where the reshape does not happen *and* a
        // per-control statement of which descriptor fields may move; until both land, the
        // shape stays in the invariant section, docs/12's T3 says so, and this is what makes
        // the day somebody changes it a day the design document has to move too.
        let mut compound = control("u16_8x16_matrix", KnownFlag::Inactive.bit(), 7);
        compound.control_type = ControlType::Unknown { raw: 0x0101 };
        compound.elems = 128;
        compound.elem_size = 2;
        compound.dims = vec![8, 16];

        let projected = invariant_control(&compound);

        assert_eq!(
            (projected.elems, projected.elem_size, projected.dims.clone()),
            (128, 2, vec![8, 16]),
            "a payload's shape is compared as description until PF:17 retires, and the \
             invariant projection dropped it — T3, note **N288**"
        );

        // Non-vacuity, and the other direction of the same claim: the projection is not the
        // identity function, so "it kept the shape" is a decision rather than an accident.
        assert_eq!(
            projected.current, None,
            "the invariant projection has to clear the current value, or this arm is passing \
             because nothing happened"
        );
        assert_eq!(
            projected.flags.raw,
            compound.flags.raw & !VOLATILE_FLAG_BITS,
            "the invariant projection has to mask the volatile flag bits, or this arm is \
             passing because nothing happened"
        );
        assert_ne!(
            projected, compound,
            "the invariant projection has to change something, or every claim above is vacuous"
        );

        // And what that costs, said where a reader of the projection meets it: two captures of
        // one vivid-class device taken across an `S_FMT` differ in the *description* half.
        let reshaped = {
            let mut out = compound.clone();
            out.dims[0] -= 1;
            out.elems -= 16;
            invariant_control(&out)
        };
        assert_ne!(
            projected, reshaped,
            "a reshaped payload has to survive the projection as a difference, because that \
             is what the comparison arm in `corpus_replay` is asserting on the other side"
        );
    }

    /// Every device half a caller can put on a comparison, with the four answers it supports.
    ///
    /// Written down rather than derived from [`DeviceDifference::verdict`], for note
    /// **N252**'s reason: each row's verdict is read off the *design's* three cases — nothing
    /// moved, the format tree alone moved, anything the owner's ruling does not license — and
    /// each row's word is the serde spelling a `--json` consumer matches on, so a `verdict`
    /// that started answering something else has an expectation outside itself to fail.
    ///
    /// The fourth row is the pair set beside the format tree, because that is the combination
    /// a conjunction-shaped verdict would wave through and no other row separates it.
    fn every_device_half_with_the_answers_it_supports() -> Vec<(
        &'static str,
        DeviceDifference,
        DeviceVerdict,
        &'static str,
        &'static str,
    )> {
        vec![
            (
                "nothing moved",
                DeviceDifference::default(),
                DeviceVerdict::SameDevice,
                "same device",
                "same_device",
            ),
            (
                "the format tree alone moved",
                DeviceDifference {
                    formats: true,
                    ..DeviceDifference::default()
                },
                DeviceVerdict::OnlyTheFormatTree,
                "device differs: formats",
                "only_the_format_tree",
            ),
            (
                "one control is described differently",
                DeviceDifference {
                    controls: vec![ControlSlug::parse("brightness").expect("literal slug")],
                    ..DeviceDifference::default()
                },
                DeviceVerdict::Differs,
                "device differs: controls (brightness)",
                "differs",
            ),
            (
                "the measured pairs moved beside the format tree",
                DeviceDifference {
                    formats: true,
                    measured_pairs: true,
                    ..DeviceDifference::default()
                },
                DeviceVerdict::Differs,
                "device differs: formats, measured_pairs",
                "differs",
            ),
        ]
    }

    #[test]
    fn every_reading_of_a_comparison_answers_from_the_device_half_it_currently_carries() {
        // The hole note **N286** left open, closed and now held here (note **N289**). The
        // verdict was a *cached* field beside a `pub device` on a type that derives `Default`,
        // so `ProfileComparison::default()` plus one field write — public API, no constructor
        // involved — produced a value whose verdict summarised a device half that was no
        // longer there. Measured at workspace scope on 2026-08-20 before the repair:
        // `device.formats = true` on a default gave `verdict() == SameDevice`,
        // `device_matches() == true`, `device_differs_only_in_the_format_tree() == false`, a
        // `Display` of "device differs: formats", and bytes carrying `"same_device"` that this
        // type's own `Deserialize` refused.
        //
        // So the claim is not "the constructors agree with themselves" — they always did — but
        // that the five readings a consumer can take are all readings of `device` **as it
        // stands when they are taken**, whatever route the value arrived by.
        for (what, device, verdict, line, word) in every_device_half_with_the_answers_it_supports()
        {
            // Two routes to the same value, because the ones that could disagree are the ones
            // no constructor was involved in: a `Default` a caller filled in, and a `compare`
            // result a caller edited afterwards.
            let mut from_default = ProfileComparison::default();
            assert!(
                from_default.device.is_empty(),
                "{what}: a fresh `ProfileComparison` has to start with an empty device half, \
                 or the edit below is not an edit"
            );
            from_default.device = device.clone();

            let mine = profile(vec![control("brightness", 0, 50)]);
            let mut theirs = mine.clone();
            theirs.invariant.controls[0].range.max = 200;
            let mut from_compare = mine.compare(&theirs);
            assert_eq!(
                from_compare.verdict(),
                DeviceVerdict::Differs,
                "{what}: the edited-afterwards route has to start somewhere other than the row \
                 it is edited into, or it proves nothing"
            );
            from_compare.device = device.clone();

            for (route, comparison) in [
                ("a default a caller filled in", &from_default),
                ("a compare result a caller edited", &from_compare),
            ] {
                assert_eq!(
                    comparison.verdict(),
                    verdict,
                    "{what}, via {route}: the verdict is not the one this device half \
                     supports — {comparison}"
                );
                assert_eq!(
                    comparison.device_matches(),
                    verdict == DeviceVerdict::SameDevice,
                    "{what}, via {route}: the fidelity assertion §1.3's harness branches on \
                     disagrees with the device half beside it — {comparison}"
                );
                assert_eq!(
                    comparison.device_differs_only_in_the_format_tree(),
                    verdict == DeviceVerdict::OnlyTheFormatTree,
                    "{what}, via {route}: the owner's 2026-08-13 format-tree permission \
                     disagrees with the device half beside it — {comparison}"
                );
                assert_eq!(
                    comparison.to_string(),
                    line,
                    "{what}, via {route}: the human line and the device half describe \
                     different comparisons"
                );

                // And the bytes, because the table `render::comparison` prints and the
                // document a subprocess consumer parses are the two halves that must not come
                // apart.
                let bytes = serde_json::to_string(comparison).expect("serialize");
                let document: serde_json::Value =
                    serde_json::from_str(&bytes).expect("the comparison is an object");
                assert_eq!(
                    document.get("verdict").and_then(serde_json::Value::as_str),
                    Some(word),
                    "{what}, via {route}: the word in the bytes is not the verdict the value \
                     answers — {bytes}"
                );

                // The round trip is the same claim from the other side: a document this type
                // emitted and its own `Deserialize` refuses is a library that cannot read what
                // it writes.
                let back: ProfileComparison = serde_json::from_str(&bytes).unwrap_or_else(|e| {
                    panic!(
                        "{what}, via {route}: this type serialized a document its own \
                         deserializer refuses — {e}: {bytes}"
                    )
                });
                assert_eq!(
                    &back, comparison,
                    "{what}, via {route}: the comparison that came back is not the one that \
                     went out"
                );
            }
        }
    }
}
