//! Auto/manual control pairing — the data half (design D3).
//!
//! Writing `exposure_time_absolute` while `auto_exposure` is on does nothing: the
//! automation overwrites it on the next frame. Finding the automation partner and turning
//! it off first is what `--guarded` means.
//!
//! Resolution is layered, and the layers have different standing (E1):
//!
//! 1. **The declared table** below — transcribed from the UVC/V4L2 vocabulary. Data
//!    marked `declared`, which is to say *nominated*.
//! 2. **Empirical discovery** — toggle an automation-shaped control and diff the INACTIVE
//!    flags across the control set \[PF:3\]. Marked `measured`, and **measured wins**.
//!
//! The planner that consumes both lives in `webcam-handler-engine::pairing`; this module
//! is the vocabulary and the table, so there is one home for each.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::control::{ControlDesc, ControlSlug};

/// Where a claim about this device came from.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Provenance {
    /// Transcribed from documentation. Nominated, not legislated.
    Declared,
    /// Observed on this device, by probe. Beats `Declared` on conflict (E1).
    Measured,
}

impl Provenance {
    /// Whether `self` outranks `other`.
    #[must_use]
    pub fn beats(self, other: Provenance) -> bool {
        self == Provenance::Measured && other == Provenance::Declared
    }
}

/// How to switch an automation control off.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AutomationOff {
    /// Write this exact value (booleans: `0`).
    Value {
        /// The value that means "off".
        value: i64,
    },
    /// Select the menu item whose *name* matches — never the index.
    ///
    /// `Manual Mode` happens to be index 1 on both seed cameras, and that is a
    /// coincidence rather than a contract: menus are sparse and per-device \[PF:2\].
    MenuItemNamed {
        /// Lowercase substrings, tried in order; the first match wins.
        patterns: Vec<String>,
    },
}

impl AutomationOff {
    /// Resolve this to a concrete value for a specific control on a specific device.
    ///
    /// `None` when the control's menu has no item this recognizes — which is a real
    /// outcome on unfamiliar hardware and becomes a
    /// [`crate::Error::ControlInactive`] with the partner named, not a guess.
    #[must_use]
    pub fn resolve(&self, automation: &ControlDesc) -> Option<i64> {
        match self {
            AutomationOff::Value { value } => Some(*value),
            AutomationOff::MenuItemNamed { patterns } => patterns.iter().find_map(|pattern| {
                automation
                    .menu_index_by_name(|name| name.to_ascii_lowercase().contains(pattern))
                    .map(i64::from)
            }),
        }
    }
}

/// One manual control and the automation that overrides it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AutomationPair {
    /// The control a caller wants to drive by hand.
    pub manual: ControlSlug,
    /// The control that must be switched off first.
    pub automation: ControlSlug,
    /// How to switch it off.
    pub off: AutomationOff,
    /// Whether we read this in a spec or saw it on this device.
    pub provenance: Provenance,
}

/// One control the empirical probe declined to toggle, and why.
///
/// A named pair rather than a tuple for the reason
/// [`crate::control::ControlWrite`] is one — it crosses the T5 wire inside
/// [`crate::report::DiscoveryReport`] (D10), where an array of two strings is a shape a
/// consumer has to be told about out of band.
///
/// The `reason` is free text on purpose. A closed vocabulary here would turn a reason
/// nobody anticipated into the nearest one that was, and the probe's list of what it
/// passed over is exactly the place where "we did not look" must not be readable as "we
/// looked and found nothing" — §5's motor rule is one reason and a device that refused
/// the toggle is another, and both are discovered rather than designed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ProbeSkip {
    /// Which control.
    pub control: ControlSlug,
    /// Why, in the probe's own words.
    pub reason: String,
}

/// The declared pairing table.
///
/// Deliberately short. Anything that is not a well-known UVC relationship belongs in
/// empirical discovery, where it arrives with evidence attached.
#[must_use]
pub fn declared_pairs() -> Vec<AutomationPair> {
    fn slug(s: &str) -> ControlSlug {
        ControlSlug::parse(s).expect("declared table slugs are non-empty literals")
    }
    fn boolean_off() -> AutomationOff {
        AutomationOff::Value { value: 0 }
    }
    fn manual_menu() -> AutomationOff {
        AutomationOff::MenuItemNamed {
            patterns: vec!["manual".to_owned()],
        }
    }

    vec![
        AutomationPair {
            manual: slug("focus_absolute"),
            automation: slug("focus_automatic_continuous"),
            off: boolean_off(),
            provenance: Provenance::Declared,
        },
        AutomationPair {
            manual: slug("exposure_time_absolute"),
            automation: slug("auto_exposure"),
            off: manual_menu(),
            provenance: Provenance::Declared,
        },
        AutomationPair {
            manual: slug("exposure_time_absolute"),
            automation: slug("exposure_dynamic_framerate"),
            off: boolean_off(),
            provenance: Provenance::Declared,
        },
        AutomationPair {
            manual: slug("white_balance_temperature"),
            automation: slug("white_balance_automatic"),
            off: boolean_off(),
            provenance: Provenance::Declared,
        },
        // The older spelling the vendored skill records. Both are listed because a
        // device exposes one or the other, never both, and resolution filters against
        // the controls the device actually has.
        AutomationPair {
            manual: slug("white_balance_temperature"),
            automation: slug("white_balance_temperature_auto"),
            off: boolean_off(),
            provenance: Provenance::Declared,
        },
    ]
}

/// Whether a control looks like automation — the candidate set empirical discovery
/// toggles (design D3 layer 2).
///
/// Shape, not identity: a boolean or menu control whose name suggests automation. Being
/// wrong here costs one extra toggle during discovery, and being *narrow* here costs a
/// pair nobody finds, so the predicate leans inclusive.
#[must_use]
pub fn looks_like_automation(desc: &ControlDesc) -> bool {
    use crate::control::ControlType;

    if !matches!(desc.control_type, ControlType::Boolean | ControlType::Menu) {
        return false;
    }
    if !desc.is_writable() {
        return false;
    }
    let slug = desc.slug.as_str();
    slug.contains("auto") || slug.contains("automatic") || slug.contains("continuous")
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::control::{
        ControlFlags, ControlId, ControlRange, ControlType, ControlValue, MenuItem,
    };

    fn menu_control(slug: &str, items: &[(u32, &str)]) -> ControlDesc {
        ControlDesc {
            id: ControlId(0x0098_0901),
            name: slug.to_owned(),
            slug: ControlSlug::parse(slug).expect("literal slug"),
            control_type: ControlType::Menu,
            range: ControlRange {
                min: 0,
                max: 3,
                step: 1,
            },
            default: 3,
            flags: ControlFlags::from_raw(0),
            menu: items
                .iter()
                .map(|(i, n)| {
                    (
                        *i,
                        MenuItem::Name {
                            name: (*n).to_owned(),
                        },
                    )
                })
                .collect::<BTreeMap<_, _>>(),
            elems: 1,
            elem_size: 4,
            dims: Vec::new(),
            current: Some(ControlValue::Int(3)),
        }
    }

    #[test]
    fn manual_mode_is_found_by_name_on_both_seed_cameras() {
        // PF:2 — the two devices number their menus differently, and one of them has no
        // index 2 at all. Resolution must read names.
        let chicony = menu_control(
            "auto_exposure",
            &[(1, "Manual Mode"), (3, "Aperture Priority Mode")],
        );
        let obsbot = menu_control(
            "auto_exposure",
            &[
                (0, "Auto Mode"),
                (1, "Manual Mode"),
                (3, "Aperture Priority Mode"),
            ],
        );
        let off = AutomationOff::MenuItemNamed {
            patterns: vec!["manual".to_owned()],
        };
        assert_eq!(off.resolve(&chicony), Some(1));
        assert_eq!(off.resolve(&obsbot), Some(1));
    }

    #[test]
    fn a_menu_with_no_manual_item_resolves_to_nothing_rather_than_guessing() {
        let odd = menu_control(
            "auto_exposure",
            &[(0, "Auto Mode"), (3, "Aperture Priority Mode")],
        );
        let off = AutomationOff::MenuItemNamed {
            patterns: vec!["manual".to_owned()],
        };
        assert_eq!(off.resolve(&odd), None);
    }

    #[test]
    fn measured_beats_declared_and_nothing_beats_measured() {
        assert!(Provenance::Measured.beats(Provenance::Declared));
        assert!(!Provenance::Declared.beats(Provenance::Measured));
        assert!(!Provenance::Measured.beats(Provenance::Measured));
        assert!(!Provenance::Declared.beats(Provenance::Declared));
    }

    #[test]
    fn the_declared_table_is_all_declared() {
        // A `measured` entry in a hand-written table would be a lie about provenance,
        // and provenance is what decides conflicts.
        for pair in declared_pairs() {
            assert_eq!(pair.provenance, Provenance::Declared, "{pair:?}");
        }
        assert!(!declared_pairs().is_empty());
    }

    #[test]
    fn automation_candidates_are_writable_boolean_or_menu_controls() {
        let auto = menu_control("auto_exposure", &[(1, "Manual Mode")]);
        assert!(looks_like_automation(&auto));

        let mut read_only = auto.clone();
        read_only.flags = ControlFlags::from_raw(0x0004);
        assert!(
            !looks_like_automation(&read_only),
            "a read-only control cannot be toggled"
        );

        let mut integer = auto.clone();
        integer.control_type = ControlType::Integer;
        assert!(!looks_like_automation(&integer));

        let mut unrelated = auto;
        unrelated.slug = ControlSlug::parse("brightness").expect("literal slug");
        assert!(!looks_like_automation(&unrelated));
    }
}
