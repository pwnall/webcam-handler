//! What a verb answers with.
//!
//! `--json` emits schema types verbatim (design §2.7), and "verbatim" is only checkable if
//! the shape has a name in an emitted document — so each verb's answer is a type here
//! rather than an object assembled in a renderer. The same types are what the T5 wire
//! surface returns (D10): one home for the shape, two transports over it.
//!
//! **Which document names which.** An answer a `--json` verb prints is a root in
//! `schemas/webcam-handler-schema.json`. An answer only the wire carries —
//! [`DiscoveryReport`], which `webcam-handler-cli` splits between its `ControlReport` and two
//! lines on standard error, and [`TerminationReport`], whose verb has no command-line spelling
//! yet — is named in `schemas/webcam-handler-openrpc.json` under `components/schemas` instead,
//! because that is the document its consumer is reading. Neither type is in the bundle,
//! deliberately.
//!
//! Most of these answer a read. Three answer something that changed the world —
//! [`WriteReport`], [`DiscoveryReport`] and [`TerminationReport`] — and each of them says
//! what it did rather than only what it found, because requested is not applied (E4) and
//! a report is the only place a caller learns the difference.
//!
//! These carry no rendering decisions. A table is one view of [`ControlReport`] and
//! `--json` is another, and neither is allowed to know something the other does not.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::camera::{CameraId, CameraInfo, FormatInfo};
use crate::control::{Applied, ControlDesc, ControlSlug};
use crate::error::Holder;
use crate::pairing::{AutomationPair, ProbeSkip};
use crate::snapshot::RestoreReport;
use crate::vocabulary::closed_vocabulary;

closed_vocabulary! {
    /// Why a listing might not say what the user expected.
    ///
    /// D1: "an empty enumeration is diagnosed, not shrugged at". Hints are *data*, not
    /// prose the CLI prints and the daemon forgets — an agent reading `--json` gets the
    /// same diagnosis a human reading the table does.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum HintKind {
        /// A USB device presents a video-class interface and no driver has bound a V4L2
        /// node to it — the skill's `lsusb` triage, built in \[PF:14\].
        DriverlessUsbVideoDevice,
        /// A device node exists and could not be interrogated, so the camera it belongs
        /// to is not in the listing.
        ///
        /// The alternative — listing the camera with only the nodes that answered —
        /// would let a busy capture node read as "this camera cannot capture", which is
        /// the availability-as-capability conversion E3 exists to prevent.
        NodeUnreadable,
    }
}

/// One diagnosis attached to a listing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ListHint {
    /// What kind of problem this is.
    pub kind: HintKind,
    /// What it is about — a USB device path such as `1-2`.
    pub subject: String,
}

impl ListHint {
    /// The sentence a human reads. Lives here rather than in a renderer so the CLI and
    /// the daemon cannot describe the same finding differently.
    #[must_use]
    pub fn message(&self) -> String {
        match self.kind {
            HintKind::DriverlessUsbVideoDevice => format!(
                "USB device {} presents a video-class interface with no V4L2 driver bound; \
                 the camera is plugged in and nothing is driving it",
                self.subject
            ),
            HintKind::NodeUnreadable => format!(
                "{} could not be read, so the camera it belongs to is not listed; this is \
                 a fact about access to the node, not about what the camera can do",
                self.subject
            ),
        }
    }
}

/// What `list` answers.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
pub struct CameraList {
    /// The cameras, in enumeration order.
    pub cameras: Vec<CameraInfo>,
    /// Anything worth saying about what is *not* in the list.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hints: Vec<ListHint>,
}

/// What `info` answers: one camera and the formats its capture node offers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CameraDetail {
    /// Identity, nodes, grouping.
    pub info: CameraInfo,
    /// Formats, with sizes and the intervals available at each size nested under them
    /// \[PF:9\]. Empty for a camera with no capture node, which is a real shape.
    pub formats: Vec<FormatInfo>,
}

/// What `controls` answers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ControlReport {
    /// Which camera these came from — so a saved `--json` document is self-describing.
    pub camera: CameraId,
    /// Every control the device enumerated, including the ones this build cannot
    /// interpret \[PF:1\].
    pub controls: Vec<ControlDesc>,
    /// The auto/manual pairs in effect for this camera (design D3): the declared table
    /// filtered to controls this device has, merged with anything a probe measured on it.
    ///
    /// Each pair carries its own [`crate::pairing::Provenance`], because "the UVC spec
    /// says so" and "this camera did it while we watched" are different claims and E1
    /// makes the second one win.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pairs: Vec<AutomationPair>,
}

impl ControlReport {
    /// The controls whose declared default or current value sits outside their declared
    /// range \[PF:4, PF:5\].
    ///
    /// Reported rather than corrected, and computed here so the table and `--json` agree
    /// about which rows deserve a mark.
    #[must_use]
    pub fn self_contradicting(&self) -> Vec<&ControlDesc> {
        self.controls
            .iter()
            .filter(|desc| desc.default_out_of_range() || desc.current_out_of_range())
            .collect()
    }
}

/// What `set` answers (design D3/E4).
///
/// A list rather than a single [`Applied`], because a guarded write is more than one
/// write: switching an automation partner off is a change to the camera the caller is
/// entitled to see, and hiding it would make `--guarded` a verb whose side effects are
/// undocumented at the moment they happen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct WriteReport {
    /// Which camera took them.
    pub camera: CameraId,
    /// Every write the plan made, in the order it made them — the automation switch-offs
    /// included, each with its own `{requested, applied}` pair.
    pub writes: Vec<Applied>,
    /// The automation controls that were switched off to make the rest stick.
    ///
    /// Derived from the plan rather than from the writes, so it stays a statement about
    /// intent: a caller restoring the camera afterwards needs the list even when one of
    /// the switch-offs was itself adjusted.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub disabled_automation: Vec<ControlSlug>,
}

impl WriteReport {
    /// The writes the device did not take exactly (E4).
    ///
    /// Computed here so a table and `--json` agree about which rows deserve a mark — and
    /// so no renderer has to re-derive "did this land" from two fields.
    #[must_use]
    pub fn inexact(&self) -> Vec<&Applied> {
        self.writes.iter().filter(|a| !a.is_exact()).collect()
    }

    /// Whether every write landed exactly as asked.
    #[must_use]
    pub fn is_exact(&self) -> bool {
        self.writes.iter().all(Applied::is_exact)
    }
}

/// What `discover_pairs` answers (design D3, D4, note N9).
///
/// More than a [`ControlReport`], because D3's second layer *writes to the camera*: it toggles
/// automation-shaped controls and puts them back. `webcam-handler-cli` prints what the probe
/// declined and what it could not restore on standard error, and a caller on the other end of
/// a socket that could not see those two facts would be running a write with its restoration
/// report withheld — which is AGENTS rule 8 ("tests assert restoration") turned into a wire
/// property.
///
/// **Two of the three fields come straight from `engine::discover::Discovery`; the third is
/// assembled.** `skipped` and `restored` are that type's own. `controls` is not: `Discovery`
/// carries `pairs`, the measured relationships, and turning those into a [`ControlReport`]
/// means re-reading the control set *after* the probe put the camera back and merging declared
/// with measured so measured wins (E1) — which is `engine::pairing::applicable(&controls,
/// &merge(declared_pairs(), measured))`, exactly what `webcam-handler-cli controls
/// --discover-pairs` does today in `crates/cli`'s `InProcess::controls`. Whoever routes this
/// method next assembles it the same way or the two surfaces disagree about the provenance on
/// their pairs; note N34 records that the assembly wants one home before it has two callers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DiscoveryReport {
    /// The control set as it stands after the probe put the camera back, carrying the
    /// declared and measured pairs merged with measured winning (E1).
    pub controls: ControlReport,
    /// The automation-shaped controls the probe did not toggle, and why.
    ///
    /// A probe silent about what it passed over reads as a probe that found nothing
    /// there, and "this camera has no pairs" and "this probe did not look at half of
    /// them" are answers a caller acts on very differently.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skipped: Vec<ProbeSkip>,
    /// What putting the camera back achieved. Reported rather than assumed — and
    /// `OwnedByAutomation` is a success, not a failure (note N9).
    pub restored: RestoreReport,
}

closed_vocabulary! {
    /// The signals this tool will send to a process holding a camera.
    ///
    /// Closed, and with one member, because "which signal" is a decision the product
    /// makes rather than one a caller supplies: design §5 makes killing a holder "an
    /// explicit command naming its target, never a fallback", and a wire field that
    /// accepted an arbitrary signal number would make `terminate_holder` a general-purpose
    /// `kill` with a camera argument.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum TerminationSignal {
        /// `SIGTERM`. Asks; does not compel.
        Term,
    }
}

/// What `terminate_holder` answers when it did signal something (design §5, D10, D13).
///
/// Killing a process that holds the camera names its target, so the answer names the
/// target back. Every field is here because signalling is a *request* and the doctrine
/// that requested is not applied (E4) does not stop at the device: the process may ignore
/// `SIGTERM`, may take longer than the re-check, or may have exited between the diagnosis
/// and the signal.
///
/// [`crate::Error::HolderGone`] is the refusal for a pid that was not holding the node
/// when we looked. This is the answer for every case where a signal was actually sent —
/// including the one where it changed nothing, which `still_held` says out loud rather
/// than leaving the caller to re-run `info` and guess.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TerminationReport {
    /// The camera whose node the pid was holding.
    pub camera: CameraId,
    /// Who was signalled, as diagnosed from `/proc/*/fd` *before* the signal — the same
    /// [`Holder`] a [`crate::Error::Busy`] refusal carries, so both verbs name a process
    /// the same way.
    pub holder: Holder,
    /// What was sent.
    pub signal: TerminationSignal,
    /// Whether the node was still held when we looked again, after a bounded wait.
    ///
    /// `true` is not an error and must not be rendered as one: it is the honest answer
    /// for a process that ignored `SIGTERM`, and E3's "availability is not capability"
    /// means the caller has to be able to tell "signalled, device now free" from
    /// "signalled, still held" without guessing.
    pub still_held: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_hint_kind_renders_a_sentence_naming_its_subject() {
        // The population is the generated `ALL`, so a new hint cannot be added without
        // getting a message.
        for &kind in HintKind::ALL {
            let hint = ListHint {
                kind,
                subject: "1-2".to_owned(),
            };
            let message = hint.message();
            assert!(message.contains("1-2"), "{kind:?} renders {message:?}");
            assert!(message.len() > 20, "{kind:?} renders too thin: {message}");
        }
    }

    #[test]
    fn an_empty_listing_still_round_trips_and_omits_an_empty_hint_list() {
        let empty = CameraList::default();
        let json = serde_json::to_string(&empty).expect("serialize");
        assert_eq!(json, r#"{"cameras":[]}"#);
        assert_eq!(
            serde_json::from_str::<CameraList>(&json).expect("deserialize"),
            empty
        );
    }

    #[test]
    fn a_listing_with_a_hint_carries_it_across_the_wire() {
        let list = CameraList {
            cameras: Vec::new(),
            hints: vec![ListHint {
                kind: HintKind::DriverlessUsbVideoDevice,
                subject: "1-2".to_owned(),
            }],
        };
        let json = serde_json::to_string(&list).expect("serialize");
        let back: CameraList = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, list);
        assert_eq!(back.hints[0].subject, "1-2");
    }

    #[test]
    fn a_control_report_marks_the_rows_that_contradict_themselves() {
        use std::collections::BTreeMap;

        use crate::control::{
            ControlFlags, ControlId, ControlRange, ControlSlug, ControlType, ControlValue,
        };

        let control = |name: &str, default: i64, current: i64| ControlDesc {
            id: ControlId(1),
            name: name.to_owned(),
            slug: ControlSlug::from_name(name).expect("literal name"),
            control_type: ControlType::Integer,
            range: ControlRange {
                min: 0,
                max: 100,
                step: 1,
            },
            default,
            flags: ControlFlags::from_raw(0),
            menu: BTreeMap::new(),
            elems: 1,
            elem_size: 4,
            dims: Vec::new(),
            current: Some(ControlValue::Int(current)),
        };

        let report = ControlReport {
            camera: CameraId::parse("cam:test").expect("literal id"),
            controls: vec![
                control("Ordinary", 50, 50),
                // PF:5's shape: a default outside the declared range.
                control("Odd Default", 300, 50),
                // PF:4's shape: a current outside it.
                control("Odd Current", 50, 245),
            ],
            pairs: Vec::new(),
        };
        let flagged: Vec<&str> = report
            .self_contradicting()
            .into_iter()
            .map(|d| d.name.as_str())
            .collect();
        assert_eq!(flagged, vec!["Odd Default", "Odd Current"]);
    }

    #[test]
    fn a_write_report_names_the_writes_the_device_did_not_take_exactly() {
        use crate::control::{ControlId, ControlRange, ControlValue, WriteWarning};

        let applied = |slug: &str, requested: i64, took: i64| Applied {
            control: ControlId(1),
            slug: ControlSlug::parse(slug).expect("literal slug"),
            requested: ControlValue::Int(requested),
            applied: ControlValue::Int(took),
            warnings: if requested == took {
                Vec::new()
            } else {
                vec![WriteWarning::Clamped {
                    requested,
                    applied: took,
                    range: ControlRange {
                        min: 0,
                        max: took,
                        step: 1,
                    },
                }]
            },
        };

        let report = WriteReport {
            camera: CameraId::parse("cam:test").expect("literal id"),
            writes: vec![
                applied("white_balance_automatic", 0, 0),
                applied("white_balance_temperature", 9_000, 6_500),
            ],
            disabled_automation: vec![
                ControlSlug::parse("white_balance_automatic").expect("literal slug"),
            ],
        };
        assert!(!report.is_exact());
        assert_eq!(
            report
                .inexact()
                .iter()
                .map(|a| a.slug.as_str())
                .collect::<Vec<_>>(),
            vec!["white_balance_temperature"]
        );

        let json = serde_json::to_string(&report).expect("serialize");
        assert_eq!(
            serde_json::from_str::<WriteReport>(&json).expect("deserialize"),
            report
        );

        // The inverse: a report where everything landed has nothing to mark, and its
        // empty automation list stays out of the document.
        let clean = WriteReport {
            camera: report.camera.clone(),
            writes: vec![applied("brightness", 50, 50)],
            disabled_automation: Vec::new(),
        };
        assert!(clean.is_exact());
        assert!(clean.inexact().is_empty());
        assert!(
            !serde_json::to_string(&clean)
                .expect("serialize")
                .contains("disabled_automation")
        );
    }

    #[test]
    fn a_discovery_answer_carries_what_the_probe_declined_and_what_it_put_back() {
        use crate::snapshot::{RestoreOutcome, UnrestorableReason};

        let report = DiscoveryReport {
            controls: ControlReport {
                camera: CameraId::parse("cam:test").expect("literal id"),
                controls: Vec::new(),
                pairs: Vec::new(),
            },
            // §5: a motorized candidate is named rather than silently passed over.
            skipped: vec![ProbeSkip {
                control: ControlSlug::parse("focus_automatic_continuous").expect("literal slug"),
                reason: "motorized (design §5): the probe does not move motors".to_owned(),
            }],
            restored: RestoreReport {
                outcomes: vec![RestoreOutcome::Unrestorable {
                    control: ControlSlug::parse("white_balance_temperature").expect("literal slug"),
                    reason: UnrestorableReason::NoLongerWritable,
                }],
            },
        };
        let json = serde_json::to_string(&report).expect("serialize");
        let back: DiscoveryReport = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, report);
        // The two facts a caller on the far end of a socket would otherwise lose: what was
        // skipped, and that the camera did not come all the way back (AGENTS rule 8).
        assert!(json.contains("focus_automatic_continuous"), "{json}");
        assert!(!back.restored.is_complete());

        // The other direction: a probe that touched everything and restored everything
        // leaves the skip list out of the document entirely, so an empty list and a
        // missing one are the same answer rather than two.
        let clean = DiscoveryReport {
            controls: report.controls.clone(),
            skipped: Vec::new(),
            restored: RestoreReport {
                outcomes: Vec::new(),
            },
        };
        let json = serde_json::to_string(&clean).expect("serialize");
        assert!(!json.contains("skipped"), "{json}");
        assert_eq!(
            serde_json::from_str::<DiscoveryReport>(&json).expect("deserialize"),
            clean
        );
    }

    #[test]
    fn a_termination_answer_names_who_was_signalled_and_whether_it_worked() {
        let report = TerminationReport {
            camera: CameraId::parse("cam:test").expect("literal id"),
            holder: Holder {
                pid: 4242,
                comm: Some("cheese".to_owned()),
            },
            signal: TerminationSignal::Term,
            still_held: false,
        };
        let json = serde_json::to_string(&report).expect("serialize");
        assert_eq!(
            serde_json::from_str::<TerminationReport>(&json).expect("deserialize"),
            report
        );

        // The half that makes the field non-decorative: a process that ignored the signal
        // is an answer this document can express, not a failure it converts into an error.
        // E3 — "we asked and it is still there" is not "the camera cannot".
        let stubborn = TerminationReport {
            still_held: true,
            ..report
        };
        let json = serde_json::to_string(&stubborn).expect("serialize");
        assert!(json.contains(r#""still_held":true"#), "{json}");
        assert_eq!(
            serde_json::from_str::<TerminationReport>(&json).expect("deserialize"),
            stubborn
        );
    }

    #[test]
    fn the_only_signal_this_tool_sends_is_the_one_that_asks() {
        // Design §5 makes killing a holder an explicit command rather than a general
        // `kill`; the vocabulary is what stops a caller supplying `SIGKILL`. The walk is
        // over the generated `ALL`, so widening it is a decision somebody has to make here
        // rather than a field that silently accepts more.
        assert_eq!(TerminationSignal::ALL, &[TerminationSignal::Term]);
        assert_eq!(
            serde_json::to_string(&TerminationSignal::Term).expect("serialize"),
            r#""term""#
        );
    }
}
