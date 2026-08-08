//! The control model (design D2): represent, don't reject.
//!
//! Everything the device says about a control is carried, including the parts that
//! contradict the device's own declarations. Four of the twelve probe findings are a
//! camera disagreeing with itself:
//!
//! - a control type the `v4l` crate has never heard of [PF:1] → [`ControlType::Unknown`]
//! - menu indices with holes in them [PF:2] → the menu is a sparse map, not a `Vec`
//! - a current value outside the declared range [PF:4] → [`ControlDesc::current_out_of_range`]
//! - a default outside the declared range [PF:5] → [`ControlDesc::default_out_of_range`]
//!
//! None of these is corrected on the way through. They are reported.

use std::collections::BTreeMap;
use std::fmt;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::slug::{Separator, slugify};
use crate::vocabulary::bit_vocabulary;

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
/// is the seed case — the crate that panicked on it is why we own this layer [PF:1].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ControlType {
    /// `V4L2_CTRL_TYPE_INTEGER`
    Integer,
    /// `V4L2_CTRL_TYPE_BOOLEAN`
    Boolean,
    /// `V4L2_CTRL_TYPE_MENU` — items are named strings, sparsely indexed [PF:2].
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
    /// behind PF:1 and older references do not list it [PF:12]. Bits outside this
    /// vocabulary survive in [`ControlFlags::unknown_bits`].
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum KnownFlag {
        /// `V4L2_CTRL_FLAG_DISABLED` — permanently unsupported on this device.
        Disabled = 0x0001,
        /// `V4L2_CTRL_FLAG_GRABBED` — busy because streaming is in progress.
        Grabbed = 0x0002,
        /// `V4L2_CTRL_FLAG_READ_ONLY` — the Chicony's `Privacy` control [PF:12].
        ReadOnly = 0x0004,
        /// `V4L2_CTRL_FLAG_UPDATE` — writing it changes other controls' properties.
        Update = 0x0008,
        /// `V4L2_CTRL_FLAG_INACTIVE` — an automation partner owns it right now [PF:3].
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
        /// `V4L2_CTRL_FLAG_HAS_WHICH_MIN_MAX` — widely set on recent kernels [PF:12].
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
/// and the holes are real [PF:2].
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
/// free to sit outside it, and on real hardware they do [PF:4, PF:5].
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
    /// The declared default — which may sit outside `range` [PF:5].
    pub default: i64,
    /// Flags, raw and decoded.
    pub flags: ControlFlags,
    /// Menu items by index. Sparse [PF:2]; empty for non-menu controls.
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
    /// the device, not an error to correct [PF:4]. `None` when it was not read (a
    /// write-only control, or an enumeration that did not fetch values).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current: Option<ControlValue>,
}

impl ControlDesc {
    /// Whether the declared default sits outside the declared range [PF:5].
    ///
    /// The OBSBOT's `Power Line Frequency` says range `[0..2]`, default `3`.
    #[must_use]
    pub const fn default_out_of_range(&self) -> bool {
        !self.range.contains(self.default)
    }

    /// Whether the current value sits outside the declared range [PF:4].
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

    /// Whether an automation partner currently owns this control [PF:3].
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

    /// The menu index whose item name matches `predicate`.
    ///
    /// Menu semantics are discovered by *name*, never by index: `Manual Mode` is index 1
    /// on both seed cameras, and that is a coincidence, not a contract [PF:2].
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
    /// The driver clamped the value into range and said "success" [PF:6].
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
}
