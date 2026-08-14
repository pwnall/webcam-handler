//! The guarded-write planner (design D3, §2.4).
//!
//! Writing `exposure_time_absolute` while `auto_exposure` is on does nothing: the
//! automation overwrites it on the next frame, the read-back reports a value the caller
//! never asked for, and nothing in the system is wrong except the order of two writes.
//! `--guarded` is the promise that the order is right, and this module is where the
//! promise is kept: `(controls, pairs, targets) → ordered write plan`, with every
//! automation partner switched off *before* the manual write that needs it.
//!
//! The vocabulary — [`AutomationPair`], `AutomationOff`, `Provenance` and the declared
//! table — lives in `schema::pairing`. One home per law: the table is data, the planning
//! is here.
//!
//! ## Why there is a validator as well as a planner
//!
//! "The plan never writes a manual control while its automation partner is enabled" is a
//! property, and a property test is only as good as the thing it asserts with. A test
//! that re-implements the planner's own reasoning to check the planner proves nothing.
//! So [`validate`] is written as an independent walk — simulate the device, replay the
//! plan, refuse the first write made under live automation — and the inverse fixtures in
//! this module's tests hand it plans that *do* get it wrong, so its ability to say no is
//! demonstrated rather than assumed (rubric B3, docs/9 "Guarded-set inverse").

use std::collections::BTreeMap;
use std::fmt;

use schema::control::{ControlDesc, ControlId, ControlSlug, ControlValue};
use schema::pairing::{AutomationOff, AutomationPair, Provenance};
use schema::{Error, Result};

use crate::refusal::illegal;

/// How deep a chain of automation controls the planner will unwind.
///
/// Bounded everything (rubric A14). The declared table's depth is one — a manual control
/// governed by an automation control that is governed by nothing — and no measured pair
/// has ever gone deeper, so eight is generous. What the bound really buys is termination
/// on a *cyclic* pair set, which no amount of care in the table can rule out once pairs
/// can be discovered empirically.
const MAX_GUARD_DEPTH: usize = 8;

/// How many "did you mean" slugs an unknown-control refusal offers.
///
/// Three is enough to be useful and short enough to read; a list of every control on the
/// device is not a suggestion.
const MAX_SUGGESTIONS: usize = 3;

/// Why the plan contains a particular write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WritePurpose {
    /// The caller asked for this one.
    Target,
    /// The planner added it: an automation control being switched off so the manual
    /// write named here can stick.
    DisableAutomation {
        /// The manual control this clears the way for.
        manual: ControlSlug,
    },
}

/// One write, in the order it must happen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedWrite {
    /// Which control.
    pub control: ControlSlug,
    /// Its kernel id, so the executor does not have to look the slug up again.
    pub id: ControlId,
    /// The value to write.
    pub value: ControlValue,
    /// Why this write is in the plan.
    pub purpose: WritePurpose,
}

/// An ordered set of writes that satisfies the guarded-write property.
///
/// Ordered, not a set: the whole point is the sequence. Executing it out of order is
/// exactly the defect [`validate`] exists to catch.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WritePlan {
    /// The writes, in execution order.
    pub writes: Vec<PlannedWrite>,
}

impl WritePlan {
    /// Whether the plan does nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.writes.is_empty()
    }

    /// The automation controls this plan switches off, in order and without repeats.
    ///
    /// The caller records these on the session ([`schema::session::ControlStatus::AutoDisabled`])
    /// and restores them afterwards (D4), so it needs them as a list rather than as a
    /// filter over the writes.
    #[must_use]
    pub fn disabled_automation(&self) -> Vec<ControlSlug> {
        let mut out: Vec<ControlSlug> = Vec::new();
        for write in &self.writes {
            if matches!(write.purpose, WritePurpose::DisableAutomation { .. })
                && !out.contains(&write.control)
            {
                out.push(write.control.clone());
            }
        }
        out
    }
}

/// A plan that would write a manual control while its automation partner is live.
///
/// Not a [`schema::Error`]: this is an internal consistency failure, not something a
/// device did, and D13 is the registry of the latter. It carries the index so a failing
/// property test names the offending write rather than dumping the whole plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuardViolation {
    /// Where in the plan the offending write sits.
    pub index: usize,
    /// The manual control the plan writes too early.
    pub manual: ControlSlug,
    /// The automation control that is still enabled at that point.
    pub automation: ControlSlug,
}

impl fmt::Display for GuardViolation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "write {} sets {} while {} is still enabled",
            self.index, self.manual, self.automation
        )
    }
}

/// Combine the declared table with what a probe measured on this device (E1).
///
/// Pairs are identified by their `(manual, automation)` slugs: a measured pair for the
/// same two controls replaces the declared one, keeping the measured `off` recipe and
/// the measured provenance, so nothing downstream has to ask twice where a claim came
/// from. Two entries of the same rank resolve to the later one — a re-probe supersedes
/// an earlier probe, and the declared table does not contain duplicates.
///
/// The result is sorted by `(manual, automation)`, so a plan does not depend on the order
/// a profile happened to list its pairs in.
#[must_use]
pub fn merge(declared: Vec<AutomationPair>, measured: Vec<AutomationPair>) -> Vec<AutomationPair> {
    let mut by_pair: BTreeMap<(ControlSlug, ControlSlug), AutomationPair> = BTreeMap::new();
    for pair in declared.into_iter().chain(measured) {
        let key = (pair.manual.clone(), pair.automation.clone());
        let keep_existing = by_pair
            .get(&key)
            .is_some_and(|existing| existing.provenance.beats(pair.provenance));
        if !keep_existing {
            by_pair.insert(key, pair);
        }
    }
    by_pair.into_values().collect()
}

/// Plan the writes that set `targets`, switching automation off first where it is on.
///
/// Targets are honored in the order given, and the plan is what it takes to make each
/// one stick at the point it happens. Pairs naming controls this device does not have
/// are dropped rather than refused: the declared table lists both spellings of the
/// white-balance automation control precisely because a device exposes one or the other.
///
/// # Errors
///
/// - [`Error::ControlUnknown`] — no control by that slug, with the closest ones named.
/// - [`Error::ControlReadOnly`] — the device will not accept a write to it. `DISABLED`
///   controls and control-class pseudo-controls report the same variant: D13 has one
///   "you cannot write this" refusal, and all three are the device stating a *capability*
///   limit, so collapsing them does not cross E3's availability/capability line.
/// - [`Error::ControlInactive`] — an automation partner owns the control and the planner
///   cannot release it: `automation: None` when no pair for it survived the device's
///   control list, `Some(partner)` when the partner is known but its "off" value cannot
///   be resolved (a menu with no item this recognizes — never guessed \[PF:2\]).
/// - [`Error::IllegalTransition`] — the pair set has a cycle, or a chain of automation
///   controls deeper than this module's guard-depth bound.
pub fn plan(
    controls: &[ControlDesc],
    pairs: &[AutomationPair],
    targets: &[(ControlSlug, ControlValue)],
) -> Result<WritePlan> {
    let mut planner = Planner {
        board: Board::new(controls, pairs),
        all_controls: controls,
        writes: Vec::new(),
        guarded: true,
    };
    for (slug, value) in targets {
        planner.emit(slug, value.clone(), WritePurpose::Target, 0)?;
    }
    Ok(WritePlan {
        writes: planner.writes,
    })
}

/// Plan the writes that set `targets`, **without** switching any automation off.
///
/// `--no-guard`'s planner. It exists as a sibling of [`plan`] rather than as a flag inside
/// it, and it shares everything that is not the guard: slug resolution, the did-you-mean
/// list, and the read-only refusal all come from the same private walk with its partner
/// search skipped. Guarded and unguarded must disagree about *guarding* and about
/// nothing else — two verbs that resolve a control name differently are two verbs wearing
/// one name, and a caller who reaches for `--no-guard` to work around an unrelated refusal
/// would find it did not help.
///
/// A target the device reports INACTIVE is *written anyway*, which is the whole meaning of
/// the flag: the caller has been told there is an automation partner and has said to write
/// regardless. Real hardware accepts such a write and lets the automation overwrite it on
/// the next frame \[PF:3\], and the read-back reports what actually stuck — so the futility
/// is visible in the answer rather than hidden by a refusal.
///
/// # Errors
///
/// [`Error::ControlUnknown`] and [`Error::ControlReadOnly`], identically to [`plan`].
/// Never [`Error::ControlInactive`]: refusing for an inactive partner is exactly what this
/// function is for not doing.
pub fn plan_unguarded(
    controls: &[ControlDesc],
    targets: &[(ControlSlug, ControlValue)],
) -> Result<WritePlan> {
    // An empty pair set is not a shortcut: with no pairs, `Board` finds no partners, so
    // `plan`'s guard loop has nothing to do and its INACTIVE refusal — which fires only
    // when a control is inactive *and* has no known partner — would still trigger. That
    // refusal is the one thing this path must not make, so the planner is told to skip it
    // rather than starved of the data that reaches it.
    let mut planner = Planner {
        board: Board::new(controls, &[]),
        all_controls: controls,
        writes: Vec::new(),
        guarded: false,
    };
    for (slug, value) in targets {
        planner.emit(slug, value.clone(), WritePurpose::Target, 0)?;
    }
    Ok(WritePlan {
        writes: planner.writes,
    })
}

/// The controls whose INACTIVE bit differs between two readings of one device.
///
/// The comparator behind D3's empirical pass \[PF:3\]: toggle an automation-shaped control,
/// read the control set again, and whatever changed is what that control owns. Pure, so the
/// probe's one judgement is testable without a camera.
///
/// A control present in only one of the two readings is **not** reported. A control that
/// appeared or vanished between the reads did not have its flag flipped by the write — it
/// is a different device answer — and calling it a discovered partner would put a
/// hallucinated pair into the corpus with `Provenance::Measured` on it.
#[must_use]
pub fn inactive_diff(before: &[ControlDesc], after: &[ControlDesc]) -> Vec<ControlSlug> {
    let was: BTreeMap<&str, bool> = before
        .iter()
        .map(|d| (d.slug.as_str(), d.is_inactive()))
        .collect();
    after
        .iter()
        .filter(|desc| {
            was.get(desc.slug.as_str())
                .is_some_and(|before| *before != desc.is_inactive())
        })
        .map(|desc| desc.slug.clone())
        .collect()
}

/// Turn one observation into a pair a profile can carry (design D3 layer 2, E1).
///
/// `off_index` is the value the probe wrote to `automation` when `manual` came free.
///
/// A menu-valued automation control records its "off" position **by name** when the item
/// at that index has one. Menu indices are per-device \[PF:2\] — `Manual Mode` is index 1
/// on both seed cameras and that is a coincidence — so a name is the portable claim and an
/// index is a claim about this unit. Only a nameless index falls back to a bare value, and
/// a pair recorded that way is honest about being device-specific rather than pretending
/// to a portability it does not have.
///
/// `None` when the automation control is not writable, because a pair whose switch-off
/// cannot be performed is not a pair anybody can use.
#[must_use]
pub fn measured_pair(
    automation: &ControlDesc,
    off_index: i64,
    manual: &ControlSlug,
) -> Option<AutomationPair> {
    if !automation.is_writable() {
        return None;
    }
    let by_name = u32::try_from(off_index)
        .ok()
        .and_then(|index| automation.menu.get(&index))
        .and_then(schema::control::MenuItem::name)
        .map(|name| AutomationOff::MenuItemNamed {
            patterns: vec![name.to_ascii_lowercase()],
        });
    Some(AutomationPair {
        manual: manual.clone(),
        automation: automation.slug.clone(),
        off: by_name.unwrap_or(AutomationOff::Value { value: off_index }),
        provenance: Provenance::Measured,
    })
}

/// Replay `plan` against the device and refuse the first write made under live
/// automation.
///
/// This is the property, spelled as a check. It walks the plan from the start with the
/// device's *reported* state, applies each write to its model of that state, and reports
/// the first write to a control whose automation partner has not been switched off yet.
///
/// A hand-built plan is as valid an input as one from [`plan`] — that is the point.
///
/// # Errors
///
/// [`GuardViolation`] naming the offending write, the manual control, and the automation
/// control that was still live.
pub fn validate(
    controls: &[ControlDesc],
    pairs: &[AutomationPair],
    plan: &WritePlan,
) -> std::result::Result<(), GuardViolation> {
    let mut board = Board::new(controls, pairs);
    for (index, write) in plan.writes.iter().enumerate() {
        if let Some(pair) = board.live_partners_of(write.control.as_str()).first() {
            return Err(GuardViolation {
                index,
                manual: write.control.clone(),
                automation: pair.automation.clone(),
            });
        }
        board.note_write(write.control.as_str(), &write.value);
    }
    Ok(())
}

/// The pairs this device can actually exhibit: both halves name controls it has.
///
/// The declared table nominates relationships for hardware in general — it lists both
/// spellings of the white-balance automation control because a device exposes one or the
/// other — so "the pairs in effect for *this* camera" is a different list, and it is the
/// one a `controls` report shows and a snapshot's roles are decided from.
///
/// Stricter than the planner's own filter, and deliberately: [`plan`] keeps a pair whose
/// *automation* control exists because it is answering "what must I switch off before
/// this target", a question that only comes up once the target has been resolved. This
/// function answers "what does this device do", and a pair whose manual half is missing
/// is not something this device does.
#[must_use]
pub fn applicable(controls: &[ControlDesc], pairs: &[AutomationPair]) -> Vec<AutomationPair> {
    let present: BTreeMap<&str, &ControlDesc> =
        controls.iter().map(|c| (c.slug.as_str(), c)).collect();
    pairs
        .iter()
        .filter(|pair| {
            present.contains_key(pair.manual.as_str())
                && present.contains_key(pair.automation.as_str())
        })
        .cloned()
        .collect()
}

/// The pair set every verb plans against, for one device.
///
/// The declared table (D3) merged with whatever a probe measured on this camera (E1), narrowed
/// by [`applicable`] to the relationships the device can actually exhibit. One function
/// because it is one law with several readers: a `set` that guarded against a different pair
/// set than the `snapshot` that decided roles would order its restore against a device the two
/// halves disagreed about — and now that both composition roots answer a `controls` report
/// (`webcam-handler-cli` through T4, `webcam-handler-daemon` through T5), two copies of this
/// composition would be two opinions about what a camera's automation looks like.
///
/// `measured` is empty for every caller that did not run the probe, which is every read
/// verb: measuring pairs writes to the camera, and that is its own operation (note N30).
#[must_use]
pub fn in_effect(controls: &[ControlDesc], measured: Vec<AutomationPair>) -> Vec<AutomationPair> {
    applicable(
        controls,
        &merge(schema::pairing::declared_pairs(), measured),
    )
}

/// One control's descriptor, refused with the same candidates a write to it would name.
///
/// The whole descriptor rather than the bare value, because a value with no range, no flags
/// and no menu is not renderable — but the reason this lives here rather than at a call site
/// is the *refusal*: `webcam-handler-cli get brightnes` and `webcam-handler-cli set
/// brightnes=1` have to name the same near misses, or a caller corrects a typo twice. The
/// suggestion list is the planner's, so asking the planner is how the two stay one answer, and
/// both composition roots ask.
///
/// # Errors
///
/// [`Error::ControlUnknown`] naming the closest slugs this camera does have. The planner
/// produces it; the fallback below is the unreachable arm where a planner that accepted the
/// slug still did not hand back a descriptor, and it refuses rather than panicking because
/// this is a request-driven path.
pub fn describe(controls: &[ControlDesc], slug: &ControlSlug) -> Result<ControlDesc> {
    controls
        .iter()
        .find(|desc| &desc.slug == slug)
        .cloned()
        .ok_or_else(|| {
            plan_unguarded(controls, &[(slug.clone(), ControlValue::Int(0))])
                .err()
                .unwrap_or(Error::ControlUnknown {
                    requested: slug.to_string(),
                    did_you_mean: Vec::new(),
                })
        })
}

/// Whether a control is the automation half of any pair the device can honor.
///
/// The snapshot's restore ordering (D4) needs the same question answered, and answering
/// it twice in two places is how the two orderings drift.
#[must_use]
pub fn is_automation(
    controls: &[ControlDesc],
    pairs: &[AutomationPair],
    slug: &ControlSlug,
) -> bool {
    Board::new(controls, pairs)
        .values
        .contains_key(slug.as_str())
}

/// The device as the planner and the validator both read it.
///
/// Shared on purpose: if the two disagreed about what "enabled" means, the property test
/// would be asserting against a mirror. Sharing one reading makes the *inverse fixtures*
/// the thing that proves the validator can still say no — which is the right place for
/// that proof, because a fixture is data a reviewer can check by eye.
///
/// The model is deliberately per-*pair* rather than per-control. Two pairs can name the
/// same automation control with different "off" recipes, and a single "is it enabled"
/// flag on the control cannot represent that: switching the control off for one pair
/// would then read as switching it off for the other. So what is tracked is the *value*
/// each automation control holds, and enabled-ness is computed against the pair asking.
#[derive(Debug)]
struct Board<'a> {
    controls: BTreeMap<&'a str, &'a ControlDesc>,
    /// Manual slug → the pairs whose automation control this device actually has.
    partners: BTreeMap<&'a str, Vec<&'a AutomationPair>>,
    /// Automation slug → the scalar value the model believes it holds. `None` means the
    /// value is unknown or not a scalar, which reads as "on" everywhere below.
    values: BTreeMap<&'a str, Option<i64>>,
}

impl<'a> Board<'a> {
    fn new(controls: &'a [ControlDesc], pairs: &'a [AutomationPair]) -> Self {
        let by_slug: BTreeMap<&str, &ControlDesc> =
            controls.iter().map(|c| (c.slug.as_str(), c)).collect();

        let mut partners: BTreeMap<&str, Vec<&AutomationPair>> = BTreeMap::new();
        let mut values: BTreeMap<&str, Option<i64>> = BTreeMap::new();
        for pair in pairs {
            // A pair naming a control the device does not have is not an error and not a
            // warning: the declared table lists `white_balance_temperature_auto` beside
            // `white_balance_automatic` because devices expose one spelling or the other.
            let Some(auto_desc) = by_slug.get(pair.automation.as_str()).copied() else {
                continue;
            };
            partners.entry(pair.manual.as_str()).or_default().push(pair);
            values
                .entry(pair.automation.as_str())
                .or_insert_with(|| auto_desc.current.as_ref().and_then(ControlValue::as_int));
        }

        Board {
            controls: by_slug,
            partners,
            values,
        }
    }

    /// The pairs of `slug` whose automation control is still live, in table order.
    fn live_partners_of(&self, slug: &str) -> Vec<&'a AutomationPair> {
        let Some(pairs) = self.partners.get(slug) else {
            return Vec::new();
        };
        pairs
            .iter()
            .copied()
            .filter(|pair| self.is_live(pair))
            .collect()
    }

    /// Whether this pair's automation control is overriding its manual partner.
    ///
    /// Unknowable reads as *on*, in all its shapes: no resolvable "off" value (a menu
    /// with no item we recognize \[PF:2\]), no current value (an enumeration that did not
    /// fetch one, or a WRITE_ONLY control), a non-scalar value. Guessing "off" here
    /// produces a plan that skips the switch-off and looks correct; guessing "on"
    /// produces one redundant write.
    fn is_live(&self, pair: &AutomationPair) -> bool {
        let Some(auto_desc) = self.controls.get(pair.automation.as_str()).copied() else {
            return false;
        };
        let held = self.values.get(pair.automation.as_str()).copied().flatten();
        match (pair.off.resolve(auto_desc), held) {
            (Some(off), Some(current)) => current != off,
            _ => true,
        }
    }

    /// Fold a write into the model.
    fn note_write(&mut self, slug: &str, value: &ControlValue) {
        if let Some(held) = self.values.get_mut(slug) {
            *held = value.as_int();
        }
    }
}

struct Planner<'a> {
    board: Board<'a>,
    all_controls: &'a [ControlDesc],
    writes: Vec<PlannedWrite>,
    /// Whether to switch automation partners off, and — inseparably — whether an INACTIVE
    /// control with no partner is a refusal. Both are "the guard"; splitting them would
    /// let `--no-guard` refuse a write it is supposed to make.
    guarded: bool,
}

impl Planner<'_> {
    fn emit(
        &mut self,
        slug: &ControlSlug,
        value: ControlValue,
        purpose: WritePurpose,
        depth: usize,
    ) -> Result<()> {
        if depth > MAX_GUARD_DEPTH {
            return Err(illegal("guard_depth_exceeded", format!("set {slug}")));
        }

        let Some(desc) = self.board.controls.get(slug.as_str()).copied() else {
            return Err(Error::ControlUnknown {
                requested: slug.to_string(),
                did_you_mean: suggestions(self.all_controls, slug.as_str()),
            });
        };
        if !desc.is_writable() {
            return Err(Error::ControlReadOnly {
                control: slug.clone(),
            });
        }

        if !self.guarded {
            self.board.note_write(slug.as_str(), &value);
            self.writes.push(PlannedWrite {
                control: slug.clone(),
                id: desc.id,
                value,
                purpose,
            });
            return Ok(());
        }

        let partner_count = self.board.partners.get(slug.as_str()).map_or(0, Vec::len);

        if desc.is_inactive() && partner_count == 0 {
            // PF:3 with nothing to do about it. The variant says so rather than implying
            // a partner exists, because the caller's next move differs: discover pairs,
            // not "disable something".
            return Err(Error::ControlInactive {
                control: slug.clone(),
                automation: None,
            });
        }

        // Re-read after every switch-off rather than walking the partner list once. Two
        // pairs can name the same automation control with different "off" values, so
        // clearing one partner can put another back; the loop converges when nothing is
        // live, and the bound catches the case where the pair set contradicts itself and
        // it never can. Consistent data needs at most one round per partner.
        for round in 0.. {
            let Some(pair) = self.board.live_partners_of(slug.as_str()).first().copied() else {
                break;
            };
            if round > partner_count {
                return Err(Error::ControlInactive {
                    control: slug.clone(),
                    automation: Some(pair.automation.clone()),
                });
            }
            let Some(auto_desc) = self.board.controls.get(pair.automation.as_str()).copied() else {
                break;
            };
            let Some(off) = pair.off.resolve(auto_desc) else {
                // An automation menu with no item we recognize as manual [PF:2]. There
                // is no index to guess at, and guessing is how a sweep ends up recorded
                // against a value the camera never held.
                return Err(Error::ControlInactive {
                    control: slug.clone(),
                    automation: Some(pair.automation.clone()),
                });
            };
            self.emit(
                &pair.automation,
                ControlValue::Int(off),
                WritePurpose::DisableAutomation {
                    manual: slug.clone(),
                },
                depth + 1,
            )?;
        }

        self.board.note_write(slug.as_str(), &value);
        self.writes.push(PlannedWrite {
            control: slug.clone(),
            id: desc.id,
            value,
            purpose,
        });
        Ok(())
    }
}

/// The slugs closest to `requested`, for [`Error::ControlUnknown`].
///
/// Substring containment either way, then a shared prefix of four characters or more.
/// Deliberately not an edit distance: a wrong suggestion costs the caller one glance,
/// and the error is already actionable without any.
pub(crate) fn suggestions(controls: &[ControlDesc], requested: &str) -> Vec<ControlSlug> {
    let mut scored: Vec<(usize, &ControlSlug)> = controls
        .iter()
        .filter_map(|c| {
            let slug = c.slug.as_str();
            let shared = common_prefix_len(slug, requested);
            if slug.contains(requested) || requested.contains(slug) {
                Some((usize::MAX, &c.slug))
            } else if shared >= 4 {
                Some((shared, &c.slug))
            } else {
                None
            }
        })
        .collect();
    // Best match first, then alphabetically so the list is reproducible.
    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(b.1)));
    scored
        .into_iter()
        .take(MAX_SUGGESTIONS)
        .map(|(_, slug)| slug.clone())
        .collect()
}

/// Characters shared at the front of two strings. Character-wise, not byte-wise: a slug
/// arrives from a caller verbatim, so it is not guaranteed ASCII.
fn common_prefix_len(a: &str, b: &str) -> usize {
    a.chars().zip(b.chars()).take_while(|(x, y)| x == y).count()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use schema::ErrorKind;
    use schema::control::{ControlFlags, ControlRange, ControlType, MenuItem};
    use schema::pairing::{AutomationOff, Provenance, declared_pairs};

    use super::*;

    fn slug(s: &str) -> ControlSlug {
        ControlSlug::parse(s).expect("literal slug")
    }

    /// A scalar control with the given current value.
    fn integer(name: &str, current: i64, flags: u32) -> ControlDesc {
        ControlDesc {
            id: ControlId(0x0098_0900),
            name: name.to_owned(),
            slug: slug(name),
            control_type: ControlType::Integer,
            range: ControlRange {
                min: 0,
                max: 10_000,
                step: 1,
            },
            default: 0,
            flags: ControlFlags::from_raw(flags),
            menu: BTreeMap::new(),
            elems: 1,
            elem_size: 4,
            dims: Vec::new(),
            current: Some(ControlValue::Int(current)),
        }
    }

    fn boolean(name: &str, current: i64) -> ControlDesc {
        ControlDesc {
            control_type: ControlType::Boolean,
            range: ControlRange {
                min: 0,
                max: 1,
                step: 1,
            },
            ..integer(name, current, 0)
        }
    }

    /// The Chicony's `Auto Exposure`: a sparse menu with `Manual Mode` at index 1 \[PF:2\].
    fn auto_exposure(current: i64) -> ControlDesc {
        ControlDesc {
            control_type: ControlType::Menu,
            range: ControlRange {
                min: 0,
                max: 3,
                step: 1,
            },
            menu: BTreeMap::from([
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
            ]),
            ..integer("auto_exposure", current, 0)
        }
    }

    fn exposure_pair() -> AutomationPair {
        AutomationPair {
            manual: slug("exposure_time_absolute"),
            automation: slug("auto_exposure"),
            off: AutomationOff::MenuItemNamed {
                patterns: vec!["manual".to_owned()],
            },
            provenance: Provenance::Declared,
        }
    }

    fn white_balance_pair() -> AutomationPair {
        AutomationPair {
            manual: slug("white_balance_temperature"),
            automation: slug("white_balance_automatic"),
            off: AutomationOff::Value { value: 0 },
            provenance: Provenance::Declared,
        }
    }

    // ------------------------------------------------------------------ merge

    #[test]
    fn a_measured_pair_replaces_the_declared_one_and_keeps_its_provenance() {
        // E1: the device is the only authority on itself. The declared recipe for
        // `auto_exposure` is a menu lookup; this device was probed and answers to a
        // plain value, and that is what must survive.
        let measured = AutomationPair {
            off: AutomationOff::Value { value: 1 },
            provenance: Provenance::Measured,
            ..exposure_pair()
        };
        let merged = merge(vec![exposure_pair()], vec![measured.clone()]);
        assert_eq!(merged, vec![measured]);
    }

    #[test]
    fn a_declared_pair_does_not_replace_a_measured_one() {
        // The inverse direction, which is the one that silently loses probe evidence if
        // the comparison is written backwards.
        let measured = AutomationPair {
            off: AutomationOff::Value { value: 1 },
            provenance: Provenance::Measured,
            ..exposure_pair()
        };
        let merged = merge(vec![measured.clone()], vec![exposure_pair()]);
        assert_eq!(merged, vec![measured]);
    }

    #[test]
    fn merge_keeps_pairs_that_do_not_collide_and_sorts_them() {
        let merged = merge(
            vec![white_balance_pair(), exposure_pair()],
            vec![AutomationPair {
                manual: slug("focus_absolute"),
                automation: slug("focus_automatic_continuous"),
                off: AutomationOff::Value { value: 0 },
                provenance: Provenance::Measured,
            }],
        );
        let manuals: Vec<&str> = merged.iter().map(|p| p.manual.as_str()).collect();
        assert_eq!(
            manuals,
            vec![
                "exposure_time_absolute",
                "focus_absolute",
                "white_balance_temperature"
            ],
            "the output order must not depend on the input order"
        );
    }

    #[test]
    fn merging_the_declared_table_with_nothing_changes_nothing_but_the_order() {
        let merged = merge(declared_pairs(), Vec::new());
        assert_eq!(merged.len(), declared_pairs().len());
        assert!(
            merged.iter().all(|p| p.provenance == Provenance::Declared),
            "provenance must survive a merge that changes nothing"
        );
    }

    // ------------------------------------------------------------------- plan

    #[test]
    fn a_live_automation_partner_is_switched_off_before_the_manual_write() {
        let controls = vec![
            auto_exposure(3),
            integer("exposure_time_absolute", 156, 0x0010),
        ];
        let pairs = vec![exposure_pair()];
        let targets = vec![(slug("exposure_time_absolute"), ControlValue::Int(500))];

        let plan = plan(&controls, &pairs, &targets).expect("a plannable write");
        assert_eq!(
            plan.writes
                .iter()
                .map(|w| (w.control.as_str(), w.value.clone()))
                .collect::<Vec<_>>(),
            vec![
                ("auto_exposure", ControlValue::Int(1)),
                ("exposure_time_absolute", ControlValue::Int(500)),
            ]
        );
        assert_eq!(
            plan.writes[0].purpose,
            WritePurpose::DisableAutomation {
                manual: slug("exposure_time_absolute")
            }
        );
        assert_eq!(plan.writes[1].purpose, WritePurpose::Target);
        assert_eq!(plan.disabled_automation(), vec![slug("auto_exposure")]);
        validate(&controls, &pairs, &plan).expect("the planner's own plan must validate");
    }

    #[test]
    fn automation_that_is_already_off_is_not_written_again() {
        // `auto_exposure` already sits on Manual Mode. A redundant write is not a
        // correctness bug, but it is a state change we would then have to restore.
        let controls = vec![auto_exposure(1), integer("exposure_time_absolute", 156, 0)];
        let pairs = vec![exposure_pair()];
        let plan = plan(
            &controls,
            &pairs,
            &[(slug("exposure_time_absolute"), ControlValue::Int(500))],
        )
        .expect("a plannable write");
        assert_eq!(plan.writes.len(), 1);
        assert_eq!(plan.writes[0].purpose, WritePurpose::Target);
        assert!(plan.disabled_automation().is_empty());
    }

    #[test]
    fn a_pair_naming_a_control_this_device_lacks_is_dropped_not_refused() {
        // The declared table carries both spellings of the white-balance automation
        // control because a device has one or the other. Refusing on the absent spelling
        // would make the table unusable on every device.
        let controls = vec![
            boolean("white_balance_automatic", 1),
            integer("white_balance_temperature", 4600, 0x0010),
        ];
        let pairs = vec![
            white_balance_pair(),
            AutomationPair {
                manual: slug("white_balance_temperature"),
                automation: slug("white_balance_temperature_auto"),
                off: AutomationOff::Value { value: 0 },
                provenance: Provenance::Declared,
            },
        ];
        let plan = plan(
            &controls,
            &pairs,
            &[(slug("white_balance_temperature"), ControlValue::Int(3200))],
        )
        .expect("the absent spelling must not break the present one");
        assert_eq!(
            plan.writes
                .iter()
                .map(|w| w.control.as_str())
                .collect::<Vec<_>>(),
            vec!["white_balance_automatic", "white_balance_temperature"]
        );
    }

    #[test]
    fn an_inactive_control_with_no_discoverable_partner_is_refused_by_name() {
        let controls = vec![integer("exposure_time_absolute", 156, 0x0010)];
        let err = plan(
            &controls,
            &[],
            &[(slug("exposure_time_absolute"), ControlValue::Int(500))],
        )
        .expect_err("inactive with nothing to disable");
        assert_eq!(
            err,
            Error::ControlInactive {
                control: slug("exposure_time_absolute"),
                automation: None,
            }
        );
    }

    #[test]
    fn an_automation_menu_with_no_manual_item_refuses_rather_than_guessing_an_index() {
        // PF:2 taken to its conclusion: menu indices are per-device, so an unrecognized
        // menu means we do not know how to switch this automation off. Index 1 happens
        // to be Manual Mode on both seed cameras and that is a coincidence.
        let odd_menu = ControlDesc {
            menu: BTreeMap::from([
                (
                    0,
                    MenuItem::Name {
                        name: "Auto Mode".to_owned(),
                    },
                ),
                (
                    3,
                    MenuItem::Name {
                        name: "Aperture Priority Mode".to_owned(),
                    },
                ),
            ]),
            ..auto_exposure(3)
        };
        let controls = vec![odd_menu, integer("exposure_time_absolute", 156, 0x0010)];
        let err = plan(
            &controls,
            &[exposure_pair()],
            &[(slug("exposure_time_absolute"), ControlValue::Int(500))],
        )
        .expect_err("no manual item to select");
        assert_eq!(
            err,
            Error::ControlInactive {
                control: slug("exposure_time_absolute"),
                automation: Some(slug("auto_exposure")),
            }
        );
    }

    #[test]
    fn a_read_only_target_is_refused_as_read_only() {
        // PF:12 — the Chicony's `Privacy` control. READ_ONLY is 0x0004.
        let controls = vec![integer("privacy", 0, 0x0004)];
        let err = plan(&controls, &[], &[(slug("privacy"), ControlValue::Int(1))])
            .expect_err("read-only");
        assert_eq!(
            err,
            Error::ControlReadOnly {
                control: slug("privacy")
            }
        );

        // DISABLED (0x0001) reaches the same refusal — one "you cannot write this"
        // variant in D13, and both are capability statements from the device.
        let disabled = vec![integer("zoom_absolute", 0, 0x0001)];
        assert_eq!(
            plan(
                &disabled,
                &[],
                &[(slug("zoom_absolute"), ControlValue::Int(1))]
            )
            .expect_err("disabled")
            .kind(),
            ErrorKind::ControlReadOnly
        );
    }

    #[test]
    fn an_unknown_target_names_the_controls_it_could_have_meant() {
        let controls = vec![integer("brightness", 0, 0), integer("contrast", 0, 0)];
        let err = plan(&controls, &[], &[(slug("brightnes"), ControlValue::Int(1))])
            .expect_err("no such control");
        assert_eq!(
            err,
            Error::ControlUnknown {
                requested: "brightnes".to_owned(),
                did_you_mean: vec![slug("brightness")],
            }
        );

        // The inverse: nothing close means no suggestions rather than a random list.
        let far = plan(&controls, &[], &[(slug("qqqq"), ControlValue::Int(1))])
            .expect_err("no such control");
        assert_eq!(
            far,
            Error::ControlUnknown {
                requested: "qqqq".to_owned(),
                did_you_mean: Vec::new(),
            }
        );
    }

    #[test]
    fn each_of_the_two_ways_to_be_a_suggestion_is_the_only_way_for_one_fixture() {
        // `suggestions` has two rules — substring containment either way, and a shared
        // prefix of four characters or more — and the test above reaches both with one
        // fixture: "brightnes" is a substring of "brightness" *and* shares nine
        // characters with it. So either rule could be deleted with the workspace green,
        // and P3f's mutation run deleted each in turn and watched nothing happen.
        //
        // These two fixtures separate them. Each is reachable by exactly one rule, so
        // each rule is now pinned by a case the other cannot answer.
        let controls = vec![
            integer("white_balance_temperature", 4_600, 0),
            integer("exposure_time_absolute", 156, 0),
            integer("gain", 8, 0),
        ];

        // Containment only: "white_balance_temperature" contains "balance", and the two
        // share no first character at all.
        let by_containment = plan(&controls, &[], &[(slug("balance"), ControlValue::Int(1))])
            .expect_err("no such control");
        assert_eq!(
            by_containment,
            Error::ControlUnknown {
                requested: "balance".to_owned(),
                did_you_mean: vec![slug("white_balance_temperature")],
            }
        );

        // Shared prefix only: "expose_me" and "exposure_time_absolute" share "expos" and
        // neither contains the other.
        let by_prefix = plan(&controls, &[], &[(slug("expose_me"), ControlValue::Int(1))])
            .expect_err("no such control");
        assert_eq!(
            by_prefix,
            Error::ControlUnknown {
                requested: "expose_me".to_owned(),
                did_you_mean: vec![slug("exposure_time_absolute")],
            }
        );
    }

    #[test]
    fn one_automation_control_shared_by_two_targets_is_switched_off_once() {
        let controls = vec![
            auto_exposure(3),
            integer("exposure_time_absolute", 156, 0x0010),
            integer("iso_sensitivity", 100, 0x0010),
        ];
        let pairs = vec![
            exposure_pair(),
            AutomationPair {
                manual: slug("iso_sensitivity"),
                automation: slug("auto_exposure"),
                off: AutomationOff::MenuItemNamed {
                    patterns: vec!["manual".to_owned()],
                },
                provenance: Provenance::Measured,
            },
        ];
        let plan = plan(
            &controls,
            &pairs,
            &[
                (slug("exposure_time_absolute"), ControlValue::Int(500)),
                (slug("iso_sensitivity"), ControlValue::Int(400)),
            ],
        )
        .expect("a plannable pair of writes");
        assert_eq!(
            plan.writes
                .iter()
                .map(|w| w.control.as_str())
                .collect::<Vec<_>>(),
            vec!["auto_exposure", "exposure_time_absolute", "iso_sensitivity"]
        );
        validate(&controls, &pairs, &plan).expect("valid");
    }

    #[test]
    fn a_target_that_turns_automation_back_on_forces_a_second_switch_off() {
        // The caller asked for both, in this order. Honoring the second target means
        // undoing part of the first, and the plan says so rather than quietly producing
        // a manual write that the automation will overwrite on the next frame.
        let controls = vec![auto_exposure(1), integer("exposure_time_absolute", 156, 0)];
        let pairs = vec![exposure_pair()];
        let plan = plan(
            &controls,
            &pairs,
            &[
                (slug("auto_exposure"), ControlValue::Int(3)),
                (slug("exposure_time_absolute"), ControlValue::Int(500)),
            ],
        )
        .expect("a plannable sequence");
        assert_eq!(
            plan.writes
                .iter()
                .map(|w| (w.control.as_str(), w.value.clone()))
                .collect::<Vec<_>>(),
            vec![
                ("auto_exposure", ControlValue::Int(3)),
                ("auto_exposure", ControlValue::Int(1)),
                ("exposure_time_absolute", ControlValue::Int(500)),
            ]
        );
        validate(&controls, &pairs, &plan).expect("valid");
    }

    #[test]
    fn a_partner_a_nested_guard_puts_back_is_cleared_again_rather_than_refused() {
        // The switch-off loop re-reads after every write instead of walking the partner
        // list once, and its bound is `round > partner_count` — one round of slack past
        // "one round per partner". Nothing exercised the slack, so tightening the bound to
        // `round == partner_count` left the whole workspace green (P3f's mutation run).
        //
        // This is the shape that needs it, and it is the shape the loop's own comment
        // describes: `manual`'s two partners are `auto_a` and `auto_b`, and clearing
        // `auto_b` requires parking `auto_a` at 7 first — which puts `auto_a` back on for
        // `manual`. Two partners, three rounds of work, and the third round is the one the
        // tightened bound refuses.
        let controls = vec![
            integer("manual", 100, 0),
            integer("auto_a", 5, 0),
            integer("auto_b", 5, 0),
        ];
        let pairs = vec![
            AutomationPair {
                manual: slug("manual"),
                automation: slug("auto_a"),
                off: AutomationOff::Value { value: 0 },
                provenance: Provenance::Measured,
            },
            AutomationPair {
                manual: slug("manual"),
                automation: slug("auto_b"),
                off: AutomationOff::Value { value: 0 },
                provenance: Provenance::Measured,
            },
            AutomationPair {
                manual: slug("auto_b"),
                automation: slug("auto_a"),
                off: AutomationOff::Value { value: 7 },
                provenance: Provenance::Measured,
            },
        ];
        let plan = plan(
            &controls,
            &pairs,
            &[(slug("manual"), ControlValue::Int(42))],
        )
        .expect("a converging pair set has a plan");
        assert_eq!(
            plan.writes
                .iter()
                .map(|w| (w.control.as_str(), w.value.clone()))
                .collect::<Vec<_>>(),
            vec![
                ("auto_a", ControlValue::Int(0)),
                ("auto_a", ControlValue::Int(7)),
                ("auto_b", ControlValue::Int(0)),
                ("auto_a", ControlValue::Int(0)),
                ("manual", ControlValue::Int(42)),
            ],
            "the fourth write is `auto_a` going off for the second time, which is the \
             round a bound of `round == partner_count` would refuse"
        );
        // And the plan is still a guarded one: the validator reads the same board.
        validate(&controls, &pairs, &plan).expect("valid");
        assert_eq!(
            plan.disabled_automation(),
            vec![slug("auto_a"), slug("auto_b")],
            "each partner is recorded once, however many times it was written"
        );
    }

    #[test]
    fn a_guard_chain_exactly_at_the_depth_bound_plans_and_one_link_deeper_is_refused() {
        // `MAX_GUARD_DEPTH` is a number, and until this test the only thing asserting
        // anything about it was the cycle below — which refuses at *any* bound, so
        // narrowing `depth > MAX_GUARD_DEPTH` to `depth >= MAX_GUARD_DEPTH` left the
        // workspace green (P3f's mutation run). Both directions here, driven by chain
        // length: the last link is emitted at depth == chain length.
        fn chain(links: usize) -> (Vec<ControlDesc>, Vec<AutomationPair>) {
            let names: Vec<String> = (0..=links).map(|i| format!("g{i}")).collect();
            let controls = names.iter().map(|n| boolean(n, 1)).collect();
            let pairs = names
                .windows(2)
                .map(|w| AutomationPair {
                    manual: slug(&w[0]),
                    automation: slug(&w[1]),
                    off: AutomationOff::Value { value: 0 },
                    provenance: Provenance::Measured,
                })
                .collect();
            (controls, pairs)
        }

        let (controls, pairs) = chain(MAX_GUARD_DEPTH);
        let at_the_bound = plan(&controls, &pairs, &[(slug("g0"), ControlValue::Int(1))])
            .expect("a chain exactly at the bound is plannable");
        assert_eq!(
            at_the_bound
                .writes
                .iter()
                .map(|w| w.control.as_str())
                .collect::<Vec<_>>(),
            (0..=MAX_GUARD_DEPTH)
                .rev()
                .map(|i| format!("g{i}"))
                .collect::<Vec<_>>(),
            "the deepest link is switched off first and the target is written last"
        );
        validate(&controls, &pairs, &at_the_bound).expect("valid");

        let (controls, pairs) = chain(MAX_GUARD_DEPTH + 1);
        let err = plan(&controls, &pairs, &[(slug("g0"), ControlValue::Int(1))])
            .expect_err("one link past the bound has no plan");
        assert_eq!(err.kind(), ErrorKind::IllegalTransition);
        assert!(err.to_string().contains("guard_depth_exceeded"), "{err}");
    }

    #[test]
    fn an_empty_plan_says_it_is_empty_and_a_plan_with_a_write_does_not() {
        // `WritePlan::is_empty` has no caller in this workspace — P3f's mutation run found
        // it by replacing the body with `true` and watching nothing go red. It is exercised
        // rather than deleted, which is the ruling E6 made for `calibrate::applied_value`'s
        // producerless refusals: a public predicate on a public type is contract, and the
        // cheap way to keep contract honest is to assert it.
        assert!(WritePlan::default().is_empty());
        let controls = vec![integer("brightness", 128, 0)];
        let plan = plan(
            &controls,
            &[],
            &[(slug("brightness"), ControlValue::Int(200))],
        )
        .expect("a plannable write");
        assert_eq!(plan.writes.len(), 1);
        assert!(!plan.is_empty());
    }

    #[test]
    fn a_cyclic_pair_set_terminates_with_a_typed_refusal() {
        // Empirical discovery can produce anything; a cycle must not become a stack
        // overflow.
        let controls = vec![boolean("a", 1), boolean("b", 1)];
        let pairs = vec![
            AutomationPair {
                manual: slug("a"),
                automation: slug("b"),
                off: AutomationOff::Value { value: 0 },
                provenance: Provenance::Measured,
            },
            AutomationPair {
                manual: slug("b"),
                automation: slug("a"),
                off: AutomationOff::Value { value: 0 },
                provenance: Provenance::Measured,
            },
        ];
        let err = plan(&controls, &pairs, &[(slug("a"), ControlValue::Int(1))])
            .expect_err("a cycle has no plan");
        assert_eq!(err.kind(), ErrorKind::IllegalTransition);
        assert!(err.to_string().contains("guard_depth_exceeded"), "{err}");
    }

    #[test]
    fn an_automation_control_whose_value_was_never_read_is_assumed_to_be_on() {
        // An enumeration that did not fetch values, or a WRITE_ONLY automation control.
        // Assuming "off" would produce a plan that skips the switch-off and looks right.
        let mut auto = auto_exposure(1);
        auto.current = None;
        let controls = vec![auto, integer("exposure_time_absolute", 156, 0)];
        let pairs = vec![exposure_pair()];
        let plan = plan(
            &controls,
            &pairs,
            &[(slug("exposure_time_absolute"), ControlValue::Int(500))],
        )
        .expect("a plannable write");
        assert_eq!(plan.disabled_automation(), vec![slug("auto_exposure")]);
    }

    // -------------------------------------------------- the inverse fixtures

    #[test]
    fn the_validator_rejects_a_plan_that_omits_the_switch_off() {
        // docs/9's "Guarded-set inverse", built by hand: this is exactly the plan a
        // planner with the guard removed would emit, and the validator must refuse it.
        // Without this test, the property test below asserts with a function nobody has
        // seen say no.
        let controls = vec![
            auto_exposure(3),
            integer("exposure_time_absolute", 156, 0x0010),
        ];
        let pairs = vec![exposure_pair()];
        let unguarded = WritePlan {
            writes: vec![PlannedWrite {
                control: slug("exposure_time_absolute"),
                id: ControlId(0x0098_0900),
                value: ControlValue::Int(500),
                purpose: WritePurpose::Target,
            }],
        };
        let violation = validate(&controls, &pairs, &unguarded).expect_err("must be refused");
        assert_eq!(
            violation,
            GuardViolation {
                index: 0,
                manual: slug("exposure_time_absolute"),
                automation: slug("auto_exposure"),
            }
        );
        assert!(
            violation
                .to_string()
                .contains("while auto_exposure is still enabled"),
            "{violation}"
        );
    }

    #[test]
    fn the_validator_rejects_a_plan_whose_switch_off_comes_too_late() {
        // The subtler inverse: every write the guarded plan needs is present, in the
        // wrong order. A validator that only checked set membership would pass this.
        let controls = vec![
            auto_exposure(3),
            integer("exposure_time_absolute", 156, 0x0010),
        ];
        let pairs = vec![exposure_pair()];
        let reordered = WritePlan {
            writes: vec![
                PlannedWrite {
                    control: slug("exposure_time_absolute"),
                    id: ControlId(0x0098_0900),
                    value: ControlValue::Int(500),
                    purpose: WritePurpose::Target,
                },
                PlannedWrite {
                    control: slug("auto_exposure"),
                    id: ControlId(0x0098_0900),
                    value: ControlValue::Int(1),
                    purpose: WritePurpose::DisableAutomation {
                        manual: slug("exposure_time_absolute"),
                    },
                },
            ],
        };
        assert_eq!(
            validate(&controls, &pairs, &reordered)
                .expect_err("order is the whole point")
                .index,
            0
        );
    }

    #[test]
    fn the_validator_accepts_the_guarded_plan_for_the_same_fixture() {
        // The pass direction of the two tests above: the validator is not simply
        // refusing everything.
        let controls = vec![
            auto_exposure(3),
            integer("exposure_time_absolute", 156, 0x0010),
        ];
        let pairs = vec![exposure_pair()];
        let guarded = WritePlan {
            writes: vec![
                PlannedWrite {
                    control: slug("auto_exposure"),
                    id: ControlId(0x0098_0900),
                    value: ControlValue::Int(1),
                    purpose: WritePurpose::DisableAutomation {
                        manual: slug("exposure_time_absolute"),
                    },
                },
                PlannedWrite {
                    control: slug("exposure_time_absolute"),
                    id: ControlId(0x0098_0900),
                    value: ControlValue::Int(500),
                    purpose: WritePurpose::Target,
                },
            ],
        };
        validate(&controls, &pairs, &guarded).expect("the guarded order is accepted");
        assert!(validate(&controls, &pairs, &WritePlan::default()).is_ok());
    }

    // ------------------------------------------------------ the property test

    /// A deterministic generator. No `rand` in the dependency set (§2.8 is a closed
    /// registry), and a seeded LCG is what a property test needs anyway: a failure
    /// reproduces from its seed.
    struct Lcg(u64);

    impl Lcg {
        fn next(&mut self) -> u64 {
            // Numerical Recipes' constants; quality does not matter here, reproducibility
            // does.
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            self.0 >> 33
        }

        fn below(&mut self, n: u64) -> u64 {
            if n == 0 { 0 } else { self.next() % n }
        }

        fn chance(&mut self, one_in: u64) -> bool {
            self.below(one_in) == 0
        }
    }

    /// A device and a pair set with every shape the planner has to survive: menus and
    /// booleans, INACTIVE flags, read-only controls, pairs naming absent controls,
    /// several pairs sharing one automation control, and self-referential pairs.
    fn generate(
        rng: &mut Lcg,
    ) -> (
        Vec<ControlDesc>,
        Vec<AutomationPair>,
        Vec<(ControlSlug, ControlValue)>,
    ) {
        let count = 2 + rng.below(6);
        let mut controls = Vec::new();
        for index in 0..count {
            let name = format!("control_{index}");
            let flags =
                if rng.chance(4) { 0x0010 } else { 0 } | if rng.chance(8) { 0x0004 } else { 0 };
            let desc = if rng.chance(2) {
                ControlDesc {
                    flags: ControlFlags::from_raw(flags),
                    slug: slug(&name),
                    name: name.clone(),
                    ..auto_exposure(i64::try_from(rng.below(4)).unwrap_or(0))
                }
            } else {
                integer(&name, i64::try_from(rng.below(100)).unwrap_or(0), flags)
            };
            controls.push(desc);
        }

        let pair_count = rng.below(6);
        let mut pairs = Vec::new();
        for _ in 0..pair_count {
            let manual = format!("control_{}", rng.below(count));
            // Sometimes name a control the device does not have; sometimes name the
            // manual control itself, which is a cycle of length one.
            let automation = if rng.chance(5) {
                "control_absent".to_owned()
            } else {
                format!("control_{}", rng.below(count))
            };
            let off = if rng.chance(2) {
                AutomationOff::Value { value: 0 }
            } else {
                AutomationOff::MenuItemNamed {
                    patterns: vec!["manual".to_owned()],
                }
            };
            pairs.push(AutomationPair {
                manual: slug(&manual),
                automation: slug(&automation),
                off,
                provenance: if rng.chance(2) {
                    Provenance::Measured
                } else {
                    Provenance::Declared
                },
            });
        }

        let target_count = 1 + rng.below(3);
        let targets = (0..target_count)
            .map(|_| {
                (
                    slug(&format!("control_{}", rng.below(count))),
                    ControlValue::Int(i64::try_from(rng.below(1_000)).unwrap_or(0)),
                )
            })
            .collect();

        (controls, merge(pairs, Vec::new()), targets)
    }

    #[test]
    fn no_generated_plan_ever_writes_a_manual_control_under_live_automation() {
        let mut rng = Lcg(0x5eed_1234_5678_9abc);
        let mut planned = 0u32;
        let mut refused = 0u32;
        for case in 0..2_000 {
            let (controls, pairs, targets) = generate(&mut rng);
            match plan(&controls, &pairs, &targets) {
                Ok(plan) => {
                    planned += 1;
                    validate(&controls, &pairs, &plan)
                        .unwrap_or_else(|violation| panic!("case {case}: {violation}"));
                    // A plan that honors nothing satisfies the property vacuously, so
                    // pin the other half of the contract too: every target is written.
                    for (slug, _) in &targets {
                        assert!(
                            plan.writes
                                .iter()
                                .any(|w| &w.control == slug && w.purpose == WritePurpose::Target),
                            "case {case}: target {slug} is missing from the plan"
                        );
                    }
                }
                Err(err) => {
                    refused += 1;
                    // Every refusal is typed, and none of them is a stringly one.
                    assert!(
                        matches!(
                            err.kind(),
                            ErrorKind::ControlInactive
                                | ErrorKind::ControlReadOnly
                                | ErrorKind::ControlUnknown
                                | ErrorKind::IllegalTransition
                        ),
                        "case {case}: unexpected refusal {err:?}"
                    );
                }
            }
        }
        // A property test where every case refuses proves nothing, and one where none
        // does never exercises the refusals. Both arms have to be populated.
        assert!(planned > 100, "only {planned} cases produced a plan");
        assert!(refused > 100, "only {refused} cases were refused");
    }

    #[test]
    fn applicable_keeps_only_the_pairs_whose_two_halves_this_device_both_has() {
        // The declared table's white-balance rows are the case: two spellings of the
        // automation control, one of which this device has. And a pair whose *manual*
        // half is missing is not a relationship this camera exhibits, even though the
        // planner would happily carry it — the two are answering different questions.
        let controls = vec![
            boolean("white_balance_automatic", 1),
            integer("white_balance_temperature", 4_600, 0x0010),
            auto_exposure(3),
        ];
        let kept = applicable(&controls, &declared_pairs());
        assert_eq!(
            kept.iter()
                .map(|p| (p.manual.as_str(), p.automation.as_str()))
                .collect::<Vec<_>>(),
            vec![("white_balance_temperature", "white_balance_automatic")],
            "the absent spelling and the pair with no manual half both drop out"
        );

        // The inverse: a device with both halves of the exposure pair keeps it.
        let with_exposure = vec![auto_exposure(3), integer("exposure_time_absolute", 156, 0)];
        assert_eq!(
            applicable(&with_exposure, &[exposure_pair()]),
            vec![exposure_pair()]
        );
        assert!(applicable(&[], &declared_pairs()).is_empty());
    }

    #[test]
    fn the_pair_set_in_effect_is_the_declared_table_this_device_can_honor_and_measurement_wins() {
        // The composition both surfaces answer a `controls` report from, in one call.
        // Without the merge a probe's finding would be dropped; without the narrowing a
        // report would name relationships the camera cannot exhibit; and a caller that
        // measured nothing must still get the declared table, which is what makes this
        // the *only* pair-set assembly either root needs.
        let controls = vec![
            boolean("white_balance_automatic", 1),
            integer("white_balance_temperature", 4_600, 0x0010),
            auto_exposure(3),
            integer("exposure_time_absolute", 156, 0),
        ];

        let declared = in_effect(&controls, Vec::new());
        assert_eq!(declared, applicable(&controls, &declared_pairs()));
        assert!(
            declared
                .iter()
                .all(|pair| pair.provenance == Provenance::Declared),
            "{declared:?}"
        );

        // A measurement of a pair the table already nominates replaces it (E1), and one
        // the table does not name is added — both of which a caller passing the declared
        // table straight through would lose.
        let measured = AutomationPair {
            provenance: Provenance::Measured,
            ..exposure_pair()
        };
        let with_probe = in_effect(&controls, vec![measured.clone()]);
        assert!(with_probe.contains(&measured), "{with_probe:?}");
        assert_eq!(
            with_probe.len(),
            declared.len(),
            "a measured pair the table already had must replace it, not join it"
        );

        // And a device with neither half of a measured pair still does not exhibit it.
        assert!(in_effect(&[], vec![measured]).is_empty());
    }

    #[test]
    fn describing_a_control_that_is_not_there_names_what_a_write_to_it_would_name() {
        // The law both `get` surfaces rest on: a caller who typed `brightnes` is told the
        // same thing whether they were reading or writing. Asserted by comparing the two
        // refusals rather than by matching a string, so a change to the planner's
        // suggestions moves both or fails here.
        let controls = vec![integer("brightness", 50, 0), auto_exposure(3)];
        assert_eq!(
            describe(&controls, &slug("brightness")).expect("the device has it"),
            controls[0]
        );

        let typo = slug("brightnes");
        let refused = describe(&controls, &typo).expect_err("no such control");
        assert_eq!(refused.kind(), ErrorKind::ControlUnknown);
        let writing = plan_unguarded(&controls, &[(typo, ControlValue::Int(1))])
            .expect_err("no such control");
        assert_eq!(refused, writing);

        // And over a device with nothing on it, which is the arm where the planner has no
        // candidates to offer and still has to refuse.
        assert_eq!(
            describe(&[], &slug("brightness"))
                .expect_err("an empty device has no controls")
                .kind(),
            ErrorKind::ControlUnknown
        );
    }

    #[test]
    fn is_automation_answers_for_the_pairs_the_device_can_honor() {
        let controls = vec![
            auto_exposure(3),
            integer("exposure_time_absolute", 156, 0x0010),
        ];
        let pairs = vec![exposure_pair()];
        assert!(is_automation(&controls, &pairs, &slug("auto_exposure")));
        assert!(!is_automation(
            &controls,
            &pairs,
            &slug("exposure_time_absolute")
        ));
        // A pair whose automation control is absent makes nothing an automation control.
        assert!(!is_automation(
            &controls,
            &[white_balance_pair()],
            &slug("white_balance_automatic")
        ));
    }
}
