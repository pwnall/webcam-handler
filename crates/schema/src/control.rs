//! The control model (design D2): represent, don't reject.
//!
//! Everything the device says about a control is carried, including the parts that
//! contradict the device's own declarations. Four of the twelve probe findings are a
//! camera disagreeing with itself:
//!
//! - a control type the `v4l` crate has never heard of \[PF:1\] → [`ControlType::Unknown`]
//! - menu indices with holes in them \[PF:2\] → the menu is a sparse map, not a `Vec`
//! - a current value outside the declared range \[PF:4\] → [`ControlDesc::current_out_of_range`]
//! - a default outside the declared range \[PF:5\] → [`ControlDesc::default_out_of_range`]
//!
//! None of these is corrected on the way through. They are reported.

use std::collections::BTreeMap;
use std::fmt;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::limits;
use crate::slug::{Separator, slugify};
use crate::vocabulary::{bit_vocabulary, closed_vocabulary};

/// A V4L2 control id (`V4L2_CID_*`), as the kernel numbers it.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(transparent)]
pub struct ControlId(pub u32);

impl fmt::Display for ControlId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "0x{:08x}", self.0)
    }
}

/// A control's slug: its kernel name through the D2 transform, the spelling agents and
/// `v4l2-ctl` users already know.
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(transparent)]
pub struct ControlSlug(String);

impl ControlSlug {
    /// Derive the slug of a kernel control name.
    ///
    /// Returns `None` when the name slugs to nothing — a control named entirely in
    /// punctuation has no usable handle, and inventing one would collide silently.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        let s = slugify(name, Separator::Underscore);
        if s.is_empty() {
            None
        } else {
            Some(ControlSlug(s))
        }
    }

    /// Accept a slug from a caller (CLI argument, session file, RPC request) verbatim.
    ///
    /// No validation beyond non-emptiness: a slug that matches no control is a
    /// [`crate::Error::ControlUnknown`] at lookup time, which is a better error than a
    /// parse failure because it can list what *does* exist.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        if s.is_empty() {
            None
        } else {
            Some(ControlSlug(s.to_owned()))
        }
    }

    /// The slug's text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ControlSlug {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A control's type.
///
/// Closed for the types we can interpret, open-ended for the rest: `Unknown` carries the
/// raw discriminant so a control we cannot read still enumerates, displays, round-trips,
/// and is reported by name. The Chicony's `Region of Interest Rectangle` (type `0x0107`)
/// is the seed case — the crate that panicked on it is why we own this layer \[PF:1\].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ControlType {
    /// `V4L2_CTRL_TYPE_INTEGER`
    Integer,
    /// `V4L2_CTRL_TYPE_BOOLEAN`
    Boolean,
    /// `V4L2_CTRL_TYPE_MENU` — items are named strings, sparsely indexed \[PF:2\].
    Menu,
    /// `V4L2_CTRL_TYPE_BUTTON` — write-only trigger, no value.
    Button,
    /// `V4L2_CTRL_TYPE_INTEGER64`
    Integer64,
    /// `V4L2_CTRL_TYPE_CTRL_CLASS` — a class header, not a real control.
    ControlClass,
    /// `V4L2_CTRL_TYPE_STRING`
    String,
    /// `V4L2_CTRL_TYPE_BITMASK`
    Bitmask,
    /// `V4L2_CTRL_TYPE_INTEGER_MENU` — items are integers, sparsely indexed.
    IntegerMenu,
    /// `V4L2_CTRL_TYPE_U8` — compound array of bytes.
    U8,
    /// `V4L2_CTRL_TYPE_U16` — compound array.
    U16,
    /// `V4L2_CTRL_TYPE_U32` — compound array.
    U32,
    /// `V4L2_CTRL_TYPE_AREA` — `{width, height}`.
    Area,
    /// `V4L2_CTRL_TYPE_RECT` — `{left, top, width, height}`; the PF:1 control.
    Rect,
    /// Anything else the kernel emits: carried as an opaque payload.
    ///
    /// The payload's size is on the descriptor (`elem_size` × `elems`), not here — one
    /// home per fact.
    Unknown {
        /// The kernel's discriminant, preserved exactly.
        raw: u32,
    },
}

impl ControlType {
    /// Decode a `v4l2_query_ext_ctrl::type` value. Total by construction.
    #[must_use]
    pub const fn from_raw(raw: u32) -> Self {
        match raw {
            1 => ControlType::Integer,
            2 => ControlType::Boolean,
            3 => ControlType::Menu,
            4 => ControlType::Button,
            5 => ControlType::Integer64,
            6 => ControlType::ControlClass,
            7 => ControlType::String,
            8 => ControlType::Bitmask,
            9 => ControlType::IntegerMenu,
            0x0100 => ControlType::U8,
            0x0101 => ControlType::U16,
            0x0102 => ControlType::U32,
            0x0106 => ControlType::Area,
            0x0107 => ControlType::Rect,
            other => ControlType::Unknown { raw: other },
        }
    }

    /// The kernel discriminant this type came from. `from_raw` and `to_raw` are inverses
    /// over every `u32` — that property is the whole point of `Unknown`.
    #[must_use]
    pub const fn to_raw(self) -> u32 {
        match self {
            ControlType::Integer => 1,
            ControlType::Boolean => 2,
            ControlType::Menu => 3,
            ControlType::Button => 4,
            ControlType::Integer64 => 5,
            ControlType::ControlClass => 6,
            ControlType::String => 7,
            ControlType::Bitmask => 8,
            ControlType::IntegerMenu => 9,
            ControlType::U8 => 0x0100,
            ControlType::U16 => 0x0101,
            ControlType::U32 => 0x0102,
            ControlType::Area => 0x0106,
            ControlType::Rect => 0x0107,
            ControlType::Unknown { raw } => raw,
        }
    }

    /// Whether this type carries menu items.
    #[must_use]
    pub const fn is_menu(self) -> bool {
        matches!(self, ControlType::Menu | ControlType::IntegerMenu)
    }

    /// Whether values of this type are scalar integers we can plan sweeps over.
    ///
    /// Compound and opaque types round-trip but cannot be swept: a sweep needs an
    /// ordered range, and "opaque bytes" has none.
    #[must_use]
    pub const fn is_scalar(self) -> bool {
        matches!(
            self,
            ControlType::Integer
                | ControlType::Boolean
                | ControlType::Menu
                | ControlType::Integer64
                | ControlType::Bitmask
                | ControlType::IntegerMenu
        )
    }
}

bit_vocabulary! {
    /// The control flag bits this version names.
    ///
    /// Flags are carried as raw bits *plus* this decoded set, because the set grows:
    /// `HasWhichMinMax` (0x1000) arrived with the same kernel work as the RECT support
    /// behind PF:1 and older references do not list it \[PF:12\]. Bits outside this
    /// vocabulary survive in [`ControlFlags::unknown_bits`].
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum KnownFlag {
        /// `V4L2_CTRL_FLAG_DISABLED` — permanently unsupported on this device.
        Disabled = 0x0001,
        /// `V4L2_CTRL_FLAG_GRABBED` — busy because streaming is in progress.
        Grabbed = 0x0002,
        /// `V4L2_CTRL_FLAG_READ_ONLY` — the Chicony's `Privacy` control \[PF:12\].
        ReadOnly = 0x0004,
        /// `V4L2_CTRL_FLAG_UPDATE` — writing it changes other controls' properties.
        Update = 0x0008,
        /// `V4L2_CTRL_FLAG_INACTIVE` — an automation partner owns it right now \[PF:3\].
        Inactive = 0x0010,
        /// `V4L2_CTRL_FLAG_SLIDER` — a UI hint.
        Slider = 0x0020,
        /// `V4L2_CTRL_FLAG_WRITE_ONLY` — reading it is meaningless.
        WriteOnly = 0x0040,
        /// `V4L2_CTRL_FLAG_VOLATILE` — the value changes without us writing it.
        Volatile = 0x0080,
        /// `V4L2_CTRL_FLAG_HAS_PAYLOAD` — the value is a compound payload.
        HasPayload = 0x0100,
        /// `V4L2_CTRL_FLAG_EXECUTE_ON_WRITE` — writing has an effect beyond the value.
        ExecuteOnWrite = 0x0200,
        /// `V4L2_CTRL_FLAG_MODIFY_LAYOUT` — writing it reshapes the format.
        ModifyLayout = 0x0400,
        /// `V4L2_CTRL_FLAG_DYNAMIC_ARRAY` — element count varies at runtime.
        DynamicArray = 0x0800,
        /// `V4L2_CTRL_FLAG_HAS_WHICH_MIN_MAX` — widely set on recent kernels \[PF:12\].
        HasWhichMinMax = 0x1000,
    }
}

/// A control's flags: the raw word and the decoded subset, both preserved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ControlFlags {
    /// Exactly what the kernel reported.
    pub raw: u32,
    /// The bits this version can name, in declaration order.
    pub known: Vec<KnownFlag>,
    /// The bits it cannot. Not an error, not dropped — next year's flag is data.
    pub unknown_bits: u32,
}

impl ControlFlags {
    /// Decode a raw flag word.
    #[must_use]
    pub fn from_raw(raw: u32) -> Self {
        ControlFlags {
            raw,
            known: KnownFlag::decode(raw),
            unknown_bits: KnownFlag::unknown_bits(raw),
        }
    }

    /// Whether a named flag is set. Reads the raw word, so a decoding bug cannot make
    /// a flag silently disappear from a policy decision.
    #[must_use]
    pub const fn has(&self, flag: KnownFlag) -> bool {
        self.raw & flag.bit() != 0
    }
}

impl Default for ControlFlags {
    fn default() -> Self {
        ControlFlags::from_raw(0)
    }
}

/// One menu item. Menus are sparse: `VIDIOC_QUERYMENU` returns `EINVAL` on the holes,
/// and the holes are real \[PF:2\].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MenuItem {
    /// A named item (`V4L2_CTRL_TYPE_MENU`), e.g. `"Manual Mode"`.
    Name {
        /// The kernel's name for this index.
        name: String,
    },
    /// An integer item (`V4L2_CTRL_TYPE_INTEGER_MENU`).
    Value {
        /// The kernel's value for this index.
        value: i64,
    },
}

impl MenuItem {
    /// The item's name, when it has one.
    #[must_use]
    pub fn name(&self) -> Option<&str> {
        match self {
            MenuItem::Name { name } => Some(name),
            MenuItem::Value { .. } => None,
        }
    }
}

/// A control's declared range. Declared: the current value and the default are both
/// free to sit outside it, and on real hardware they do \[PF:4, PF:5\].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ControlRange {
    /// Declared minimum.
    pub min: i64,
    /// Declared maximum.
    pub max: i64,
    /// Declared step. Zero and negative steps occur; treat them as 1 when planning
    /// (see [`ControlRange::effective_step`]) rather than dividing by them.
    pub step: i64,
}

impl ControlRange {
    /// The step to plan with: the declared step when it is usable, else 1.
    #[must_use]
    pub const fn effective_step(&self) -> i64 {
        if self.step > 0 { self.step } else { 1 }
    }

    /// Whether `value` lies inside the declared range.
    #[must_use]
    pub const fn contains(&self, value: i64) -> bool {
        value >= self.min && value <= self.max
    }

    /// `value` pulled into the declared range, the way a driver pulls it \[PF:6\].
    ///
    /// A descriptor declaring its minimum above its maximum is returned unchanged rather
    /// than panicking: `i64::clamp` panics on an inverted range, and a range is device
    /// data (D2), so a lying descriptor must not be able to abort the process.
    #[must_use]
    pub const fn clamp_value(&self, value: i64) -> i64 {
        if self.min > self.max {
            value
        } else if value < self.min {
            self.min
        } else if value > self.max {
            self.max
        } else {
            value
        }
    }

    /// `value` rounded **down** to a whole [`ControlRange::effective_step`] counted from
    /// [`ControlRange::min`], then pulled back into range.
    ///
    /// Down is the direction the UVC drivers on the seed hardware round, and rounding is
    /// only ever applied to a value already inside the range — an inverted or unusable
    /// range leaves the value alone rather than inventing a grid for it.
    ///
    /// **Every operation here saturates**, and that is a rule about this method rather
    /// than a defensive habit: it is a `pub` method of the shared vocabulary crate taking
    /// two device-supplied numbers and one caller-supplied one, so any `i64` it is handed
    /// is inside its contract. The subtraction was the one that did not, and note
    /// **N224** records what it answered — for `{min: 2000, max: 10000, step: 100}`, a
    /// range `corpus/profiles/obsbot-tiny3.json` carries, `align_down(i64::MIN)` wrapped
    /// to the range's *maximum*. Saturation gives the near end instead, which is what
    /// [`ControlRange::clamp_value`] would have said about the same value.
    #[must_use]
    pub const fn align_down(&self, value: i64) -> i64 {
        let step = self.effective_step();
        if step <= 1 || self.min > self.max {
            return value;
        }
        let offset = value.saturating_sub(self.min);
        // `rem_euclid` is non-negative, so this leans on `saturating_sub` only when the
        // offset itself saturated — i.e. when the value is further below `min` than an
        // `i64` reaches. `i64::MIN` stays `i64::MIN` and clamps to `min` below.
        let aligned = self
            .min
            .saturating_add(offset.saturating_sub(offset.rem_euclid(step)));
        self.clamp_value(aligned)
    }
}

/// A control value, as read or as written.
///
/// Three shapes, not fifteen: scalars are integers whatever their declared type, and
/// interpretation is the descriptor's job. Payload types are opaque bytes, which is
/// exactly enough to round-trip a control we cannot read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum ControlValue {
    /// Integer, boolean, menu index, bitmask — anything scalar.
    Int(i64),
    /// `V4L2_CTRL_TYPE_STRING`.
    Text(String),
    /// A compound or unrecognized payload, byte-exact.
    Bytes(Vec<u8>),
}

impl ControlValue {
    /// The scalar value, when this is one.
    #[must_use]
    pub fn as_int(&self) -> Option<i64> {
        match self {
            ControlValue::Int(v) => Some(*v),
            _ => None,
        }
    }
}

impl fmt::Display for ControlValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ControlValue::Int(v) => write!(f, "{v}"),
            ControlValue::Text(s) => f.write_str(s),
            ControlValue::Bytes(b) => write!(f, "<{} bytes>", b.len()),
        }
    }
}

/// Everything the device says about one control.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ControlDesc {
    /// The kernel's control id.
    pub id: ControlId,
    /// The kernel's name, verbatim.
    pub name: String,
    /// The name through the D2 transform.
    pub slug: ControlSlug,
    /// The control's type, `Unknown` included.
    #[serde(rename = "type")]
    pub control_type: ControlType,
    /// The declared range.
    pub range: ControlRange,
    /// The declared default — which may sit outside `range` \[PF:5\].
    pub default: i64,
    /// Flags, raw and decoded.
    pub flags: ControlFlags,
    /// Menu items by index. Sparse \[PF:2\]; empty for non-menu controls.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub menu: BTreeMap<u32, MenuItem>,
    /// Element count for arrays and compound controls (1 for scalars).
    pub elems: u32,
    /// Bytes per element.
    pub elem_size: u32,
    /// Array dimensions, when the control is one.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dims: Vec<u32>,
    /// The current value **as read, unvalidated** — outside the range is a fact about
    /// the device, not an error to correct \[PF:4\].
    ///
    /// Absent for two different reasons, and telling them apart is `type` and `flags`
    /// rather than this field. A control that **declares** it has no value — a button, a
    /// control class, a write-only or a disabled control, or a compound whose declared
    /// payload size is one no reader will fetch — was never going to have one. Anything
    /// else with no value here was *asked and did not answer*: the device declined the
    /// read. The two are one predicate apart on this same document, so a consumer with no
    /// Rust toolchain can make the distinction from the fields beside this one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current: Option<ControlValue>,
}

impl ControlDesc {
    /// Whether the declared default sits outside the declared range \[PF:5\].
    ///
    /// The OBSBOT's `Power Line Frequency` says range `[0..2]`, default `3`.
    #[must_use]
    pub const fn default_out_of_range(&self) -> bool {
        !self.range.contains(self.default)
    }

    /// Whether the current value sits outside the declared range \[PF:4\].
    ///
    /// The OBSBOT's `Zoom, Continuous` says range `[-100..100]`, current `245`.
    #[must_use]
    pub fn current_out_of_range(&self) -> bool {
        match &self.current {
            Some(ControlValue::Int(v)) => !self.range.contains(*v),
            _ => false,
        }
    }

    /// Whether this control can be written right now, ignoring automation.
    #[must_use]
    pub fn is_writable(&self) -> bool {
        !self.flags.has(KnownFlag::ReadOnly)
            && !self.flags.has(KnownFlag::Disabled)
            && self.control_type != ControlType::ControlClass
    }

    /// Whether an automation partner currently owns this control \[PF:3\].
    #[must_use]
    pub fn is_inactive(&self) -> bool {
        self.flags.has(KnownFlag::Inactive)
    }

    /// Whether the value changes without us writing it — such a control cannot be
    /// meaningfully snapshotted or restored, and restore says so rather than pretending.
    #[must_use]
    pub fn is_volatile(&self) -> bool {
        self.flags.has(KnownFlag::Volatile)
    }

    /// Why a write to this control cannot be read back, when it cannot (design D3).
    ///
    /// Asked by every backend's `set` through [`WriteWarning::classify`], so "which
    /// controls cannot be verified" is one question with one answer rather than a
    /// condition each backend re-derives.
    #[must_use]
    pub fn unverifiable(&self) -> Option<Unverifiable> {
        if self.control_type == ControlType::Button {
            Some(Unverifiable::TypeHasNoValue)
        } else if self.flags.has(KnownFlag::WriteOnly) {
            Some(Unverifiable::WriteOnly)
        } else {
            None
        }
    }

    /// Whether the descriptor itself says there is no current value to read.
    ///
    /// Four declarations, and they are the device's rather than ours: a `BUTTON` has no
    /// value, a `CONTROL_CLASS` is a header rather than a control, a `WRITE_ONLY`
    /// control's value means nothing, and a `DISABLED` one is permanently unsupported. A
    /// fifth is this build's own limit rather than the device's, and it belongs here for
    /// the same reason: a compound control whose declared `elem_size × elems` is zero or
    /// larger than [`limits::MAX_CONTROL_PAYLOAD_BYTES`] is one no reader will fetch
    /// (rubric B10 bounds it before it can become an allocation), so its value was never
    /// going to be present either.
    ///
    /// **This is the list a backend's read path must be reading**, not a second copy of
    /// it: control semantics live here (AGENTS "One home per law"), and the whole worth
    /// of the predicate below is that the two lists cannot drift apart.
    #[must_use]
    pub fn declares_no_value(&self) -> bool {
        matches!(
            self.control_type,
            ControlType::Button | ControlType::ControlClass
        ) || self.flags.has(KnownFlag::WriteOnly)
            || self.flags.has(KnownFlag::Disabled)
            || (self.flags.has(KnownFlag::HasPayload) && !self.payload_is_readable())
    }

    /// Whether this build would fetch this control's declared payload at all.
    fn payload_is_readable(&self) -> bool {
        let Ok(elem_size) = usize::try_from(self.elem_size) else {
            return false;
        };
        let Ok(elems) = usize::try_from(self.elems) else {
            return false;
        };
        let Some(len) = elem_size.checked_mul(elems) else {
            return false;
        };
        len != 0 && len <= limits::MAX_CONTROL_PAYLOAD_BYTES
    }

    /// Whether the device was asked for this control's value and would not give it.
    ///
    /// The predicate AGENTS rule 7 needs a reader to be able to make. A `None`
    /// [`ControlDesc::current`] has two populations under it: the one the descriptor
    /// predicted ([`ControlDesc::declares_no_value`]) and the one it did not. The second
    /// is an **availability** fact — the driver answered `EBUSY`, or answered nothing this
    /// build could use — and reading it as "this control has no value" is availability
    /// converted into capability, which is the conversion E3 exists to stop.
    ///
    /// It is a derived predicate rather than a field on purpose. Every input is already on
    /// this document and on the wire, so a JSON consumer computes the same answer from the
    /// same bytes; a field would be a second place for the fact to live and a migration for
    /// every stored profile. What it deliberately does **not** carry is *which* refusal:
    /// `EBUSY` and a short reply are the same value here, and note **N192** records that
    /// trade and the evidence that would reverse it.
    ///
    /// The callers that must not ignore it are the ones that promise to put a camera back:
    /// `engine::snapshot::take` records it, `engine::lifecycle::arm_pre_snapshot` refuses
    /// on it, and `engine::profile::capture` refuses on it (note **N195**).
    #[must_use]
    pub fn value_was_declined(&self) -> bool {
        self.current.is_none() && !self.declares_no_value()
    }

    /// The menu index whose item name matches `predicate`.
    ///
    /// Menu semantics are discovered by *name*, never by index: `Manual Mode` is index 1
    /// on both seed cameras, and that is a coincidence, not a contract \[PF:2\].
    pub fn menu_index_by_name<F: Fn(&str) -> bool>(&self, predicate: F) -> Option<u32> {
        self.menu
            .iter()
            .find(|(_, item)| item.name().is_some_and(&predicate))
            .map(|(index, _)| *index)
    }
}

/// A warning that rides a successful write.
///
/// Not an error: the driver accepted the write and reported success. But the caller
/// asked for one thing and got another, and D13 keeps warnings out of the error registry
/// precisely so a warning is never mistaken for a failure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WriteWarning {
    /// The driver clamped the value into range and said "success" \[PF:6\].
    Clamped {
        /// What we asked for.
        requested: i64,
        /// What the device now holds.
        applied: i64,
        /// The range it was clamped into, as declared at write time.
        range: ControlRange,
    },
    /// The driver aligned the value to the control's step.
    StepAligned {
        /// What we asked for.
        requested: i64,
        /// What the device now holds.
        applied: i64,
        /// The step it was aligned to.
        step: i64,
    },
    /// The value came back different for a reason we cannot attribute. Still reported:
    /// an unexplained difference is the most interesting kind.
    Adjusted {
        /// What we asked for.
        requested: ControlValue,
        /// What the device now holds.
        applied: ControlValue,
    },
    /// The write was accepted and there was nothing to read back, so
    /// [`Applied::applied`] is what was *asked for* rather than what was observed.
    ///
    /// D3 says every write reads back. Some controls cannot: a `BUTTON` is a trigger with
    /// no value, and a `WRITE_ONLY` control's value is the device's to keep. Returning
    /// `{requested, applied}` with the two equal and no warning would make those writes
    /// indistinguishable from verified ones, which is the collapse E4 exists to prevent —
    /// so the gap is carried as data instead.
    Unverified {
        /// What makes the value unreadable.
        because: Unverifiable,
    },
}

closed_vocabulary! {
    /// Why a write could not be read back.
    ///
    /// Both members are read off the descriptor the device supplied, so neither is a
    /// guess about the device's intentions.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum Unverifiable {
        /// The control's type has no value: `V4L2_CTRL_TYPE_BUTTON` is a trigger, and
        /// writing it *is* the effect.
        TypeHasNoValue,
        /// The device flags the control `WRITE_ONLY`, which is the device saying that
        /// reading it would not mean anything.
        WriteOnly,
        /// The write was accepted and the read-back that should have followed it was
        /// refused by the device.
        ///
        /// Distinct from the two above because it is not a property of the descriptor:
        /// nothing said this control was unreadable, and then it was. Several drivers
        /// answer `EINVAL` to `G_EXT_CTRLS` for controls they happily enumerate, so this
        /// is a real outcome rather than a defensive branch — and it is the one case
        /// where a caller might sensibly re-read later.
        DeviceDeclinedToRead,
    }
}

impl WriteWarning {
    /// Explain one write, given the control that took it (design D3, §2.10).
    ///
    /// **The single home for "what kind of adjustment was that".** Every backend's
    /// [`crate::Camera::set`] calls this and the engine never re-derives it: a second copy
    /// would let the fake and the real driver describe the same adjustment differently,
    /// and E5's whole claim is that they do not.
    ///
    /// The method is a *diagnosis*, not a model. It replays what a driver would have done
    /// — clamp into range \[PF:6\], then round down to the step — and reports that chain
    /// only when the chain actually predicts what came back. When it does not, the honest
    /// answer is [`WriteWarning::Adjusted`]: the device changed the value for a reason
    /// this build cannot attribute, and an unexplained difference is the most interesting
    /// kind. Claiming "clamped" for an adjustment that was not a clamp would be inventing
    /// a cause, which is the one thing D2 forbids.
    ///
    /// A control the device will not let us read back gets
    /// [`WriteWarning::Unverified`] instead of a diagnosis, whatever the two values look
    /// like — there is nothing to compare, and a caller who cannot tell that apart from a
    /// verified write has lost the fact D3 is about. Otherwise an exact write produces no
    /// warnings at all.
    #[must_use]
    pub fn classify(
        desc: &ControlDesc,
        requested: &ControlValue,
        applied: &ControlValue,
    ) -> Vec<WriteWarning> {
        if let Some(because) = desc.unverifiable() {
            return vec![WriteWarning::Unverified { because }];
        }
        if requested == applied {
            return Vec::new();
        }
        let unattributed = || {
            vec![WriteWarning::Adjusted {
                requested: requested.clone(),
                applied: applied.clone(),
            }]
        };
        let (ControlValue::Int(asked), ControlValue::Int(took)) = (requested, applied) else {
            // A string or a payload that came back different has no range to explain it.
            return unattributed();
        };

        let clamped = desc.range.clamp_value(*asked);
        let aligned = desc.range.align_down(clamped);
        if aligned != *took {
            return unattributed();
        }

        // The chain, in the order the driver walked it, so a value that was both out of
        // range and off the grid reports both steps rather than one summarised jump.
        //
        // At least one of the two fires, and that is arithmetic rather than optimism:
        // `aligned == took` and `asked != took`, so `clamped == asked && aligned ==
        // clamped` would make `asked == took`. A silent empty list is therefore
        // unreachable here rather than merely unlikely — which is why there is no
        // defensive third branch nobody could turn red.
        let mut warnings = Vec::new();
        if clamped != *asked {
            warnings.push(WriteWarning::Clamped {
                requested: *asked,
                applied: clamped,
                range: desc.range,
            });
        }
        if aligned != clamped {
            warnings.push(WriteWarning::StepAligned {
                requested: clamped,
                applied: aligned,
                step: desc.range.effective_step(),
            });
        }
        warnings
    }
}

/// One requested write: which control, and the value to put in it.
///
/// A named pair rather than a `(ControlSlug, ControlValue)` tuple because this is what
/// `set` takes on the T5 wire (D10), and a tuple crosses JSON as
/// `["brightness", {"kind":"int","value":50}]` — positional, unnameable in the emitted
/// bundle, and a shape a hand-written client has to be told about out of band.
///
/// The answer to one of these is an [`Applied`], which is where E4's `{requested,
/// applied}` pair lives; nothing here records what the device took, because at this point
/// nothing has asked it.
///
/// **How far this spelling reaches, and why it stops there.** It is the shape of a requested
/// write from the command line inward: `cli_core::Assignment` is a clap newtype over one of
/// these, and `cli_core::Executor::set` takes a slice of them — so `webcam-handler-cli` and
/// `webcam-handler-client` hand their shared command surface the *same* value, and P4f's
/// parity gate compares two paths whose input has one shape. `engine::pairing` and
/// `engine::write` still take `(ControlSlug, ControlValue)` pairs, and that is a deliberate
/// stop rather than an oversight: the planner is a pure core on the mutation floor, its
/// callers build targets inline in dozens of places, and nothing downstream of the executor
/// serializes anything — a named pair buys a wire document nothing there emits. The conversion
/// to the planner's spelling is [`ControlWrite::target`], which is on the type rather than at
/// its call sites because P4c gave it a second caller: `webcam-handler-cli set` and
/// `webcam-handler-daemon`'s `wch_set` both cross this boundary, and two spellings of "which
/// fields of a requested write are the target" is how the two surfaces P4f compares start to
/// differ. Note N35 carries the rest. Contrast [`crate::pairing::ProbeSkip`], which *was*
/// pushed all the way into `engine::discover`: there the tuple had one producer and one
/// consumer, so one spelling cost four lines.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ControlWrite {
    /// Which control.
    pub control: ControlSlug,
    /// The value to write. Requested, not applied (E4) — what the device took comes back
    /// in [`Applied::applied`].
    pub value: ControlValue,
}

impl ControlWrite {
    /// The same write in the spelling `engine::pairing` and `engine::write` take.
    ///
    /// One home for the boundary described above. It clones rather than consuming because
    /// both callers hold a borrowed slice — a `set` refused by the planner still has to be
    /// able to name what was asked for.
    #[must_use]
    pub fn target(&self) -> (ControlSlug, ControlValue) {
        (self.control.clone(), self.value.clone())
    }
}

/// The result of a write: what was asked, what the device actually holds, and why they
/// differ if they do (design D3/E4).
///
/// Every layer above preserves both fields. A layer that collapses them to one value is
/// dropping the fact the whole doctrine exists to keep (rubric A10).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Applied {
    /// Which control.
    pub control: ControlId,
    /// Its slug, so output is readable without a second lookup.
    pub slug: ControlSlug,
    /// The value we asked for.
    pub requested: ControlValue,
    /// The value read back afterwards.
    pub applied: ControlValue,
    /// Why they differ, when they do.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<WriteWarning>,
}

impl Applied {
    /// Whether the device holds what we asked for.
    #[must_use]
    pub fn is_exact(&self) -> bool {
        self.requested == self.applied
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_type_round_trips_every_u32() {
        // The property `Unknown` exists to provide. Sampling the whole space is
        // wasteful; sample the named values, their neighbours, and the compound range
        // where the kernel keeps adding types.
        let mut cases: Vec<u32> = (0..=20).collect();
        cases.extend(0x00f0..=0x0120);
        cases.extend([0x0200, 0x0283, 0x8000, u32::MAX]);
        for raw in cases {
            assert_eq!(ControlType::from_raw(raw).to_raw(), raw, "raw={raw:#x}");
        }
    }

    #[test]
    fn the_pf1_control_type_survives_instead_of_panicking() {
        // 0x0107 is the type that panics `v4l::query_controls` on this kernel. Here it
        // is a named variant; the point is that the *decoder* is total either way.
        assert_eq!(ControlType::from_raw(0x0107), ControlType::Rect);
        // And a type nobody has invented yet decodes rather than exploding.
        assert_eq!(
            ControlType::from_raw(0x0999),
            ControlType::Unknown { raw: 0x0999 }
        );
    }

    #[test]
    fn unknown_flag_bits_are_carried_not_dropped() {
        // 0x1000 is known (PF:12). 0x2000 is not — yet.
        let flags = ControlFlags::from_raw(0x1010 | 0x2000);
        assert!(flags.has(KnownFlag::Inactive));
        assert!(flags.has(KnownFlag::HasWhichMinMax));
        assert_eq!(flags.unknown_bits, 0x2000);
        assert_eq!(flags.raw, 0x3010);
    }

    #[test]
    fn known_flags_partition_the_raw_word() {
        // Both directions: every raw word splits into known bits plus unknown bits with
        // nothing lost and nothing invented.
        for raw in [0u32, 0x1, 0xffff, 0xdead_beef, u32::MAX] {
            let f = ControlFlags::from_raw(raw);
            let recomposed = f.known.iter().fold(0u32, |a, k| a | k.bit()) | f.unknown_bits;
            assert_eq!(recomposed, raw, "raw={raw:#x}");
        }
    }

    fn obsbot_zoom_continuous() -> ControlDesc {
        // PF:4 as data: range [-100..100], current 245.
        ControlDesc {
            id: ControlId(0x009a_090d),
            name: "Zoom, Continuous".to_owned(),
            slug: ControlSlug::from_name("Zoom, Continuous").expect("slug"),
            control_type: ControlType::Integer,
            range: ControlRange {
                min: -100,
                max: 100,
                step: 1,
            },
            default: 0,
            flags: ControlFlags::from_raw(0x1000),
            menu: BTreeMap::new(),
            elems: 1,
            elem_size: 4,
            dims: Vec::new(),
            current: Some(ControlValue::Int(245)),
        }
    }

    #[test]
    fn out_of_range_current_is_reported_not_corrected() {
        let desc = obsbot_zoom_continuous();
        assert!(desc.current_out_of_range());
        assert_eq!(desc.current, Some(ControlValue::Int(245)));
        // The inverse: an in-range value must not be flagged.
        let mut ok = obsbot_zoom_continuous();
        ok.current = Some(ControlValue::Int(50));
        assert!(!ok.current_out_of_range());
    }

    #[test]
    fn out_of_range_default_is_reported_not_corrected() {
        // PF:5 as data: OBSBOT Power Line Frequency, menu range [0..2], default 3.
        let desc = ControlDesc {
            id: ControlId(0x0098_0918),
            name: "Power Line Frequency".to_owned(),
            slug: ControlSlug::from_name("Power Line Frequency").expect("slug"),
            control_type: ControlType::Menu,
            range: ControlRange {
                min: 0,
                max: 2,
                step: 1,
            },
            default: 3,
            flags: ControlFlags::from_raw(0),
            menu: BTreeMap::from([
                (
                    0,
                    MenuItem::Name {
                        name: "Disabled".to_owned(),
                    },
                ),
                (
                    1,
                    MenuItem::Name {
                        name: "50 Hz".to_owned(),
                    },
                ),
                (
                    2,
                    MenuItem::Name {
                        name: "60 Hz".to_owned(),
                    },
                ),
            ]),
            elems: 1,
            elem_size: 4,
            dims: Vec::new(),
            current: Some(ControlValue::Int(2)),
        };
        assert!(desc.default_out_of_range());
        assert!(!desc.current_out_of_range());
    }

    #[test]
    fn sparse_menus_keep_their_holes() {
        // PF:2: the Chicony's Auto Exposure has items {1, 3}. Index 2 does not exist,
        // and a Vec would have invented it.
        let menu = BTreeMap::from([
            (
                1,
                MenuItem::Name {
                    name: "Manual Mode".to_owned(),
                },
            ),
            (
                3,
                MenuItem::Name {
                    name: "Aperture Priority Mode".to_owned(),
                },
            ),
        ]);
        let desc = ControlDesc {
            id: ControlId(0x0098_0901),
            name: "Auto Exposure".to_owned(),
            slug: ControlSlug::from_name("Auto Exposure").expect("slug"),
            control_type: ControlType::Menu,
            range: ControlRange {
                min: 0,
                max: 3,
                step: 1,
            },
            default: 3,
            flags: ControlFlags::from_raw(0x1000),
            menu,
            elems: 1,
            elem_size: 4,
            dims: Vec::new(),
            current: Some(ControlValue::Int(3)),
        };
        assert!(!desc.menu.contains_key(&2));
        assert_eq!(desc.menu_index_by_name(|n| n.contains("Manual")), Some(1));
        assert_eq!(desc.menu_index_by_name(|n| n.contains("Nonexistent")), None);

        let json = serde_json::to_string(&desc).expect("serialize");
        let back: ControlDesc = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, desc, "the hole must survive a round trip");
    }

    #[test]
    fn an_unknown_type_round_trips_with_its_payload() {
        let desc = ControlDesc {
            id: ControlId(0x0098_1ae1),
            name: "Region of Interest Rectangle".to_owned(),
            slug: ControlSlug::from_name("Region of Interest Rectangle").expect("slug"),
            control_type: ControlType::Unknown { raw: 0x0fff },
            range: ControlRange {
                min: 0,
                max: 0,
                step: 0,
            },
            default: 0,
            flags: ControlFlags::from_raw(0x0100),
            menu: BTreeMap::new(),
            elems: 1,
            elem_size: 16,
            dims: Vec::new(),
            current: Some(ControlValue::Bytes(vec![1, 2, 3, 4, 5, 6, 7, 8])),
        };
        let json = serde_json::to_string(&desc).expect("serialize");
        let back: ControlDesc = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, desc);
    }

    // ------------------------------------------- the write-warning classifier

    /// A plain integer control with the given range, for the classifier tests.
    fn scalar(min: i64, max: i64, step: i64) -> ControlDesc {
        ControlDesc {
            id: ControlId(0x0098_0900),
            name: "Brightness".to_owned(),
            slug: ControlSlug::from_name("Brightness").expect("literal name"),
            control_type: ControlType::Integer,
            range: ControlRange { min, max, step },
            default: 0,
            flags: ControlFlags::from_raw(0),
            menu: BTreeMap::new(),
            elems: 1,
            elem_size: 4,
            dims: Vec::new(),
            current: Some(ControlValue::Int(0)),
        }
    }

    #[test]
    fn a_value_the_device_declined_is_a_different_absence_from_one_it_never_had() {
        // AGENTS rule 7 as a predicate a reader can reach. `current: None` has two
        // populations under it and the descriptor says which: the four declarations a
        // device makes about a control with no value, plus this build's own payload
        // bound, against everything else — where the absence means the driver was asked
        // and did not answer (note **N195**).
        let declared: Vec<ControlDesc> = vec![
            ControlDesc {
                control_type: ControlType::Button,
                current: None,
                ..scalar(0, 100, 1)
            },
            ControlDesc {
                control_type: ControlType::ControlClass,
                current: None,
                ..scalar(0, 100, 1)
            },
            ControlDesc {
                flags: ControlFlags::from_raw(KnownFlag::WriteOnly.bit()),
                current: None,
                ..scalar(0, 100, 1)
            },
            ControlDesc {
                flags: ControlFlags::from_raw(KnownFlag::Disabled.bit()),
                current: None,
                ..scalar(0, 100, 1)
            },
            // The payload this build will not fetch: `elem_size × elems` past
            // `MAX_CONTROL_PAYLOAD_BYTES` is bounded before it becomes an allocation
            // (rubric B10), so its value was never going to arrive either.
            ControlDesc {
                control_type: ControlType::U8,
                flags: ControlFlags::from_raw(KnownFlag::HasPayload.bit()),
                elem_size: 1,
                elems: u32::MAX,
                current: None,
                ..scalar(0, 100, 1)
            },
        ];
        for desc in &declared {
            assert!(
                desc.declares_no_value(),
                "{:?} declares it has no value",
                desc.control_type
            );
            assert!(
                !desc.value_was_declined(),
                "{:?} was never asked, so nothing declined",
                desc.control_type
            );
        }

        // The population this predicate exists for: an ordinary writable integer with no
        // value on it. Nothing about the descriptor predicts the absence, so the device
        // declined the read — which is what a snapshot must refuse to write against.
        let declined = ControlDesc {
            current: None,
            ..scalar(0, 100, 1)
        };
        assert!(!declined.declares_no_value());
        assert!(declined.value_was_declined());
        assert!(
            declined.is_writable(),
            "the sharp case is a control restore could have put back"
        );

        // And a control that answered is neither.
        let answered = scalar(0, 100, 1);
        assert!(!answered.declares_no_value());
        assert!(!answered.value_was_declined());

        // A payload this build *would* fetch is not a declaration: an absent value there
        // is a declined read like any other.
        let readable_payload = ControlDesc {
            control_type: ControlType::U8,
            flags: ControlFlags::from_raw(KnownFlag::HasPayload.bit()),
            elem_size: 1,
            elems: 16,
            current: None,
            ..scalar(0, 100, 1)
        };
        assert!(!readable_payload.declares_no_value());
        assert!(readable_payload.value_was_declined());
    }

    #[test]
    fn an_exact_write_classifies_as_no_warning_at_all() {
        let desc = scalar(0, 100, 1);
        assert!(
            WriteWarning::classify(&desc, &ControlValue::Int(50), &ControlValue::Int(50))
                .is_empty()
        );
        // Non-scalar values too: a payload that came back byte-identical is exact.
        let bytes = ControlValue::Bytes(vec![1, 2, 3]);
        assert!(WriteWarning::classify(&desc, &bytes, &bytes).is_empty());
    }

    #[test]
    fn a_control_the_device_will_not_read_back_classifies_as_unverified() {
        // D3 says every write reads back; a BUTTON and a WRITE_ONLY control cannot, and
        // reporting `requested == applied` with no warning would make those writes
        // indistinguishable from the ones we actually checked.
        let mut button = scalar(0, 100, 1);
        button.control_type = ControlType::Button;
        assert_eq!(
            button.unverifiable(),
            Some(Unverifiable::TypeHasNoValue),
            "a button is a trigger, not a value"
        );
        assert_eq!(
            WriteWarning::classify(&button, &ControlValue::Int(1), &ControlValue::Int(1)),
            vec![WriteWarning::Unverified {
                because: Unverifiable::TypeHasNoValue
            }],
            "the two values matching proves nothing when nothing was read"
        );

        let mut write_only = scalar(0, 100, 1);
        write_only.flags = ControlFlags::from_raw(KnownFlag::WriteOnly.bit());
        assert_eq!(
            WriteWarning::classify(&write_only, &ControlValue::Int(7), &ControlValue::Int(7)),
            vec![WriteWarning::Unverified {
                because: Unverifiable::WriteOnly
            }]
        );

        // The inverse: an ordinary control is verifiable, so an exact write is silent.
        let ordinary = scalar(0, 100, 1);
        assert_eq!(ordinary.unverifiable(), None);
        assert!(
            WriteWarning::classify(&ordinary, &ControlValue::Int(7), &ControlValue::Int(7))
                .is_empty()
        );
    }

    #[test]
    fn a_write_the_driver_pulled_into_range_classifies_as_clamped_and_says_by_how_much() {
        // PF:6, as a diagnosis: the driver took 100 for a request of 5000 and reported
        // success. The pair is the fact E4 exists to keep.
        let desc = scalar(0, 100, 1);
        let warnings =
            WriteWarning::classify(&desc, &ControlValue::Int(5_000), &ControlValue::Int(100));
        assert_eq!(
            warnings,
            vec![WriteWarning::Clamped {
                requested: 5_000,
                applied: 100,
                range: desc.range,
            }]
        );
    }

    #[test]
    fn a_write_off_the_step_classifies_as_step_aligned_and_names_the_step() {
        let desc = scalar(0, 100, 5);
        let warnings =
            WriteWarning::classify(&desc, &ControlValue::Int(37), &ControlValue::Int(35));
        assert_eq!(
            warnings,
            vec![WriteWarning::StepAligned {
                requested: 37,
                applied: 35,
                step: 5,
            }]
        );
    }

    #[test]
    fn a_clamped_and_realigned_write_reports_both_warnings_in_the_order_they_happened() {
        // The maximum is not itself on the grid, so the driver clamps to 102 and then
        // rounds to 100. Reporting one warning would drop half the story.
        let desc = scalar(0, 102, 5);
        let warnings =
            WriteWarning::classify(&desc, &ControlValue::Int(500), &ControlValue::Int(100));
        assert_eq!(
            warnings,
            vec![
                WriteWarning::Clamped {
                    requested: 500,
                    applied: 102,
                    range: desc.range,
                },
                WriteWarning::StepAligned {
                    requested: 102,
                    applied: 100,
                    step: 5,
                },
            ]
        );
    }

    #[test]
    fn an_adjustment_the_range_cannot_explain_is_reported_as_unattributed() {
        // The value was in range and on the grid, and the device took something else
        // anyway. Calling that a clamp would invent a cause; `Adjusted` says what
        // happened and admits it does not know why.
        let desc = scalar(0, 100, 1);
        let warnings =
            WriteWarning::classify(&desc, &ControlValue::Int(50), &ControlValue::Int(42));
        assert_eq!(
            warnings,
            vec![WriteWarning::Adjusted {
                requested: ControlValue::Int(50),
                applied: ControlValue::Int(42),
            }]
        );

        // The same when the clamp *would* have predicted something else: a request past
        // the maximum that came back as neither the request nor the maximum.
        let odd = WriteWarning::classify(&desc, &ControlValue::Int(5_000), &ControlValue::Int(7));
        assert_eq!(
            odd,
            vec![WriteWarning::Adjusted {
                requested: ControlValue::Int(5_000),
                applied: ControlValue::Int(7),
            }]
        );
    }

    #[test]
    fn a_text_or_payload_write_the_device_changed_classifies_as_adjusted_not_as_clamped() {
        // A range says nothing about a string, so there is no chain to replay.
        let mut desc = scalar(0, 100, 1);
        desc.control_type = ControlType::String;
        let warnings = WriteWarning::classify(
            &desc,
            &ControlValue::Text("hello".to_owned()),
            &ControlValue::Text("hell".to_owned()),
        );
        assert!(matches!(
            warnings.as_slice(),
            [WriteWarning::Adjusted { .. }]
        ));

        // And a scalar that came back as bytes — a kind change is never a clamp.
        let mixed = WriteWarning::classify(
            &scalar(0, 100, 1),
            &ControlValue::Int(5),
            &ControlValue::Bytes(vec![5]),
        );
        assert!(matches!(mixed.as_slice(), [WriteWarning::Adjusted { .. }]));
    }

    #[test]
    fn a_descriptor_that_declares_its_minimum_above_its_maximum_classifies_without_panicking() {
        // Device data (D2), and `i64::clamp` panics on it. The range explains nothing, so
        // the difference is reported as unattributed rather than as a clamp into a range
        // that has no inside.
        let desc = scalar(100, 0, 1);
        assert_eq!(desc.range.clamp_value(5_000), 5_000);
        assert_eq!(desc.range.align_down(37), 37);
        let warnings =
            WriteWarning::classify(&desc, &ControlValue::Int(5_000), &ControlValue::Int(3));
        assert!(matches!(
            warnings.as_slice(),
            [WriteWarning::Adjusted { .. }]
        ));
    }

    #[test]
    fn alignment_rounds_down_the_way_a_driver_does_and_never_leaves_the_range() {
        let range = ControlRange {
            min: 3,
            max: 27,
            step: 4,
        };
        // Counted from the minimum, not from zero: 3, 7, 11, …
        assert_eq!(range.align_down(3), 3);
        assert_eq!(range.align_down(6), 3);
        assert_eq!(range.align_down(7), 7);
        assert_eq!(range.align_down(26), 23);
        assert_eq!(range.align_down(27), 27);
        // A step of one or zero has no grid to snap to.
        let unstepped = ControlRange {
            min: 0,
            max: 10,
            step: 0,
        };
        assert_eq!(unstepped.align_down(7), 7);
        // Saturating at the extremes rather than wrapping into a different answer. The
        // range is deliberately the *widest* one, which pins the two saturating calls and
        // nothing else: with `min == i64::MIN` the offset is whatever the value is, and
        // the interior subtraction is exercised by the test below instead, on a range a
        // camera actually declares.
        let wide = ControlRange {
            min: i64::MIN,
            max: i64::MAX,
            step: 1_000,
        };
        assert!(wide.contains(wide.align_down(i64::MAX)));
        assert!(wide.contains(wide.align_down(i64::MIN)));
    }

    #[test]
    fn a_value_far_below_a_real_range_aligns_to_its_minimum_and_not_to_its_maximum() {
        // The range is `White Balance Temperature` as `corpus/profiles/obsbot-tiny3.json`
        // committed it, and it is here because *this* is the shape that reaches the one
        // operation in `align_down` that does not saturate: a positive `min` makes the
        // offset saturate to `i64::MIN`, and `offset - offset.rem_euclid(step)` then has
        // nowhere to go. `min: i64::MIN` — the range the extremes above use — is the one
        // range where the subtraction is unreachable, because the offset is the value.
        //
        // The wrong answer is not a panic. Compiled without overflow checks, which is what
        // `cargo install` produces, the subtraction wraps and the aligned value comes back
        // *above* the maximum, so `clamp_value` returns the range's top. On a
        // `pan_absolute` that is a motor sent to its far limit for the most negative input
        // a caller could hand it, which is D2's "a lying number must not become a
        // movement".
        let white_balance = ControlRange {
            min: 2_000,
            max: 10_000,
            step: 100,
        };
        assert_eq!(white_balance.align_down(i64::MIN), 2_000);
        assert_eq!(white_balance.align_down(i64::MAX), 10_000);

        // The same shape on a control that moves a motor, from the same profile, and on
        // the negative side of it: a range straddling zero has headroom below its minimum,
        // so the saturation is on the other end and the answer is still the near limit.
        let pan = ControlRange {
            min: -468_000,
            max: 468_000,
            step: 3_600,
        };
        assert_eq!(pan.align_down(i64::MIN), -468_000);
        assert_eq!(pan.align_down(i64::MAX), 468_000);
    }

    #[test]
    fn clamping_puts_a_value_inside_the_range_from_either_side() {
        let range = ControlRange {
            min: -100,
            max: 100,
            step: 1,
        };
        assert_eq!(range.clamp_value(245), 100);
        assert_eq!(range.clamp_value(-245), -100);
        assert_eq!(range.clamp_value(0), 0);
    }

    #[test]
    fn a_zero_step_does_not_become_a_division_by_zero() {
        let r = ControlRange {
            min: 0,
            max: 10,
            step: 0,
        };
        assert_eq!(r.effective_step(), 1);
        let r = ControlRange {
            min: 0,
            max: 10,
            step: -4,
        };
        assert_eq!(r.effective_step(), 1);
        let r = ControlRange {
            min: 0,
            max: 10,
            step: 2,
        };
        assert_eq!(r.effective_step(), 2);
    }

    #[test]
    fn a_requested_write_names_its_control_and_its_value_rather_than_positioning_them() {
        // The whole reason this is a struct and not a tuple: the two fields have names on
        // the wire, so a hand-written client (P5c) reads the document rather than an
        // agreed ordering. A tuple would serialize as an array and this assertion is what
        // would go red if somebody "simplified" it back into one.
        let write = ControlWrite {
            control: ControlSlug::parse("brightness").expect("literal slug"),
            value: ControlValue::Int(50),
        };
        let json = serde_json::to_string(&write).expect("serialize");
        assert_eq!(
            json,
            r#"{"control":"brightness","value":{"kind":"int","value":50}}"#
        );
        assert_eq!(
            serde_json::from_str::<ControlWrite>(&json).expect("deserialize"),
            write
        );

        // A payload value survives the same trip: the descriptor decides whether a
        // control takes one (design §2.3), so the wire must be able to carry it.
        let payload = ControlWrite {
            control: ControlSlug::parse("region_of_interest").expect("literal slug"),
            value: ControlValue::Bytes(vec![0x00, 0xff]),
        };
        let json = serde_json::to_string(&payload).expect("serialize");
        assert_eq!(
            serde_json::from_str::<ControlWrite>(&json).expect("deserialize"),
            payload
        );
    }
}
