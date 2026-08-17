//! The backend conformance battery (design §2.11 step 4; docs/9 "The battery").
//!
//! Every T1/T2 implementation runs this suite, and passing it is what "the backend is
//! done" means. The arms are the doctrine made executable: enumeration groups nodes by
//! capability rather than numbering \[PF:7\], the control model represents what it cannot
//! interpret (D2) \[PF:1, PF:2, PF:4, PF:5\], writes read back (D3/E4), an out-of-range
//! write is a clamped *success* \[PF:6\], snapshot/restore is an inverse (D4), and a
//! hotplug poll that times out is an answer rather than an error (E3).
//!
//! ## Skip accounting is the design, not a convenience
//!
//! A backend may legitimately be unable to run an arm — no camera is attached, the device
//! has no writable control, the backend cannot script faults. That is expressed as a
//! *declared* skip with a written reason, and the accounting is checked **both
//! directions**: an arm that ran while it was declared skipped is a failure (the
//! declaration is stale, and stale declarations are how a suite quietly stops testing
//! anything), an arm that did not run without a declaration is a failure (docs/8 Part C:
//! "skip == pass, in any costume"), and a declared skip with an empty reason is a failure
//! (a skip nobody can read is a skip nobody can audit).
//!
//! Each arm decides for *itself* whether it ran. That is what makes the first direction
//! reachable: if [`run`] consulted the declarations before dispatching, "ran while
//! declared skipped" would be unrepresentable and the check would be theatre.
//!
//! ## What the battery deliberately does not do
//!
//! It never drives a control to its limits when the control's name says a motor moves
//! (design §5 — motors wear). Perturbations are one step wide; the PF:6 clamp probe picks
//! a non-motorized control or the arm skips with that reason.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::time::{Duration, Instant};

use schema::backend::{BackendKind, Camera, CameraBackend, HotplugEvent};
use schema::camera::{
    CAP_META_CAPTURE, CAP_VIDEO_CAPTURE, CameraFingerprint, CameraInfo, FormatInfo, NodeKind,
    PixelFormat,
};
use schema::capture::{Frame, NegotiatedStream, StreamRequest};
use schema::control::{
    ControlDesc, ControlId, ControlRange, ControlSlug, ControlType, ControlValue, KnownFlag,
    WriteWarning,
};
use schema::error::Error;
use schema::limits;
use schema::pairing::looks_like_automation;
use schema::session::SweepSpec;
use schema::snapshot::{ControlRole, RestoreOutcome, RestoreReport, Snapshot, SnapshotEntry};
use schema::time::Stamp;

use crate::vocabulary::closed_vocabulary;

closed_vocabulary! {
    /// One arm of the battery.
    ///
    /// `ALL` is generated from this definition (rubric rule 6): an arm cannot be added
    /// without joining the walk in [`run`], and the private `execute` dispatcher's
    /// exhaustive match cannot be satisfied without giving it an implementation.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub enum BatteryArm {
        /// Cameras enumerate with unique ids, serializable descriptions, and nodes
        /// classified by `device_caps` \[PF:7\].
        Enumeration,
        /// The control model carries what it cannot interpret (D2) \[PF:1, PF:2,
        /// PF:4, PF:5\].
        ControlModel,
        /// Every write returns `{requested, applied}`, and an out-of-range write is a
        /// clamped success \[PF:6\].
        WriteReadBack,
        /// Snapshot, perturb, restore, compare — the D4 inverse, asserted rather than
        /// assumed.
        SnapshotRestoreInverse,
        /// Start, take frames, stop, start again; frame bytes agree with the negotiated
        /// format and size.
        StreamLifecycle,
        /// D5's "an explicit request still wins": a named format or size the device does
        /// not offer is a typed refusal, never a substitution.
        ExplicitRequest,
        /// A hotplug watch can be created and polls to a timeout without erroring (E3).
        HotplugWatch,
        /// The backend's scripted fault menu (design §2.9).
        FaultMenu,
    }
}

impl BatteryArm {
    /// The arm's name, as it appears in reports.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            BatteryArm::Enumeration => "enumeration",
            BatteryArm::ControlModel => "control_model",
            BatteryArm::WriteReadBack => "write_read_back",
            BatteryArm::SnapshotRestoreInverse => "snapshot_restore_inverse",
            BatteryArm::StreamLifecycle => "stream_lifecycle",
            BatteryArm::ExplicitRequest => "explicit_request",
            BatteryArm::HotplugWatch => "hotplug_watch",
            BatteryArm::FaultMenu => "fault_menu",
        }
    }

    /// Run this arm against `backend`, appending anything it finds to `log`.
    ///
    /// The match is exhaustive, so a new variant stops the build until it has a body —
    /// an arm nobody dispatches is an arm nobody runs.
    fn execute(self, backend: &dyn CameraBackend, log: &mut ArmLog<'_>) -> ArmOutcome {
        match self {
            BatteryArm::Enumeration => arm_enumeration(backend, log),
            BatteryArm::ControlModel => arm_control_model(backend, log),
            BatteryArm::WriteReadBack => arm_write_read_back(backend, log),
            BatteryArm::SnapshotRestoreInverse => arm_snapshot_restore_inverse(backend, log),
            BatteryArm::StreamLifecycle => arm_stream_lifecycle(backend, log),
            BatteryArm::ExplicitRequest => arm_explicit_request(backend, log),
            BatteryArm::HotplugWatch => arm_hotplug_watch(backend, log),
            BatteryArm::FaultMenu => arm_fault_menu(backend, log),
        }
    }
}

impl fmt::Display for BatteryArm {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Whether an arm executed, or why it did not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArmOutcome {
    /// The arm executed. Whether it found anything is [`BatteryReport::failures`]'s
    /// business.
    Ran,
    /// The arm could not execute, with the reason a human reads in the report.
    Skipped {
        /// Why it could not run.
        reason: String,
    },
}

impl ArmOutcome {
    /// A skip carrying `reason`.
    ///
    /// Public since P6d, because [`crate::oracle`] reports in this vocabulary rather than
    /// inventing a second one: "the arm ran" and "the arm could not, and here is why" are the
    /// same two answers whether the subject is a backend or a container oracle, and a suite
    /// with two words for them is a suite whose skips have to be counted twice.
    #[must_use]
    pub fn skipped(reason: impl Into<String>) -> ArmOutcome {
        ArmOutcome::Skipped {
            reason: reason.into(),
        }
    }

    /// Whether the arm executed.
    #[must_use]
    pub fn ran(&self) -> bool {
        matches!(self, ArmOutcome::Ran)
    }
}

/// What one battery run found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatteryReport {
    /// Which backend was tested.
    pub backend: BackendKind,
    /// Every arm's outcome — the map is total over [`BatteryArm::ALL`], so a missing arm
    /// is not expressible.
    pub outcomes: BTreeMap<BatteryArm, ArmOutcome>,
    /// Everything wrong, arm-prefixed. Empty means the run is green.
    pub failures: Vec<String>,
    /// Every claim an arm that **ran** could not put to a camera, arm-prefixed.
    ///
    /// **The gap between `Ran` and `Skipped` that AGENTS rule 3 does not allow.** An arm
    /// reports one outcome for the whole backend, and until 2026-08-16 a claim it could not
    /// ask of *one* camera became a note that was rendered only if the arm ended `Skipped` —
    /// so a backend with two cameras, one of which could not be asked, reported `ran` and
    /// said nothing about the half that never happened (note **N138**). `Claim` exists
    /// precisely so "not asked" and "passed" do not collapse; this is where that distinction
    /// survives the arm boundary.
    ///
    /// Not a failure: an unaskable camera is E3's *availability*, and turning it red would
    /// make a busy device a conformance verdict. Named and counted is what rule 3 asks for,
    /// and what a reader of a green run needs in order to know what it did not cover.
    pub notes: Vec<String>,
}

impl BatteryReport {
    /// Whether the run found nothing wrong.
    #[must_use]
    pub fn is_green(&self) -> bool {
        self.failures.is_empty()
    }

    /// This arm's outcome. `None` cannot happen for an arm in [`BatteryArm::ALL`]; the
    /// signature is `Option` only because `BTreeMap` says so.
    #[must_use]
    pub fn outcome(&self, arm: BatteryArm) -> Option<&ArmOutcome> {
        self.outcomes.get(&arm)
    }

    /// The failures mentioning `arm`.
    #[must_use]
    pub fn failures_for(&self, arm: BatteryArm) -> Vec<&str> {
        let prefix = format!("{arm}: ");
        self.failures
            .iter()
            .filter(|f| f.starts_with(&prefix))
            .map(String::as_str)
            .collect()
    }

    /// The unasked claims mentioning `arm` — see [`BatteryReport::notes`].
    #[must_use]
    pub fn notes_for(&self, arm: BatteryArm) -> Vec<&str> {
        let prefix = format!("{arm}: ");
        self.notes
            .iter()
            .filter(|note| note.starts_with(&prefix))
            .map(String::as_str)
            .collect()
    }
}

impl fmt::Display for BatteryReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "battery report for backend {}", self.backend)?;
        for (arm, outcome) in &self.outcomes {
            match outcome {
                ArmOutcome::Ran => writeln!(f, "  {arm}: ran")?,
                ArmOutcome::Skipped { reason } => writeln!(f, "  {arm}: skipped — {reason}")?,
            }
        }
        // Counted, and before the verdict: a run whose arms all say "ran" while three
        // claims went unasked is not the run a reader would otherwise take it for.
        if !self.notes.is_empty() {
            writeln!(f, "  {} unasked claim(s):", self.notes.len())?;
            for note in &self.notes {
                writeln!(f, "    - {note}")?;
            }
        }
        if self.failures.is_empty() {
            writeln!(f, "  no failures")
        } else {
            writeln!(f, "  {} failure(s):", self.failures.len())?;
            for failure in &self.failures {
                writeln!(f, "    - {failure}")?;
            }
            Ok(())
        }
    }
}

/// Run every arm against `backend`, checking `declared_skips` in both directions.
///
/// `declared_skips` maps an arm the backend knows it cannot run to the written reason.
/// The reasons are not advisory: an unjustified one, a stale one, and a missing one are
/// each a failure in the returned report.
#[must_use]
pub fn run(
    backend: &dyn CameraBackend,
    declared_skips: &BTreeMap<BatteryArm, String>,
) -> BatteryReport {
    let mut report = BatteryReport {
        backend: backend.kind(),
        outcomes: BTreeMap::new(),
        failures: Vec::new(),
        notes: Vec::new(),
    };

    for &arm in BatteryArm::ALL {
        let mut failures = Vec::new();
        let mut notes = Vec::new();
        let outcome = {
            let mut log = ArmLog {
                arm,
                failures: &mut failures,
                notes: &mut notes,
            };
            arm.execute(backend, &mut log)
        };
        report.failures.extend(failures);
        report.notes.extend(notes);

        let declared = declared_skips.get(&arm);
        if let Some(reason) = declared
            && reason.trim().is_empty()
        {
            report.failures.push(format!(
                "{arm}: the declared skip carries an empty reason; a skip nobody can read \
                 is a skip nobody can audit"
            ));
        }
        match (declared, &outcome) {
            (Some(reason), ArmOutcome::Ran) => report.failures.push(format!(
                "{arm}: ran, but the backend declared it skipped ({reason:?}) — the \
                 declaration is stale"
            )),
            (None, ArmOutcome::Skipped { reason }) => report.failures.push(format!(
                "{arm}: did not run ({reason}) and no skip was declared for it"
            )),
            (Some(_), ArmOutcome::Skipped { .. }) | (None, ArmOutcome::Ran) => {}
        }

        report.outcomes.insert(arm, outcome);
    }

    report
}

/// Where an arm records what it found. Prefixing with the arm's name here means no arm
/// can forget to say which one it is.
struct ArmLog<'a> {
    arm: BatteryArm,
    failures: &'a mut Vec<String>,
    notes: &'a mut Vec<String>,
}

impl ArmLog<'_> {
    fn fail(&mut self, message: impl AsRef<str>) {
        let arm = self.arm;
        self.failures
            .push(format!("{arm}: {}", message.as_ref().trim()));
    }

    /// Record a claim this arm could not put to a camera — see [`BatteryReport::notes`].
    ///
    /// Separate from [`ArmLog::fail`] because the two are different verdicts and the whole
    /// point of `Claim` is that they stay apart (AGENTS rule 7, one layer in from where it
    /// usually applies). Separate from `CameraVisit`'s own notes because those become a
    /// *skip reason*, which is only rendered when the arm skipped; this channel is what
    /// carries them out of an arm that ran.
    fn note(&mut self, message: impl AsRef<str>) {
        let arm = self.arm;
        self.notes
            .push(format!("{arm}: {}", message.as_ref().trim()));
    }

    /// Record `message` unless `ok`. The closure keeps the formatting off the happy path.
    fn require(&mut self, ok: bool, message: impl FnOnce() -> String) {
        if !ok {
            self.fail(message());
        }
    }
}

// ---------------------------------------------------------------------------- arms

fn arm_enumeration(backend: &dyn CameraBackend, log: &mut ArmLog<'_>) -> ArmOutcome {
    let cameras = match backend.enumerate() {
        Ok(cameras) => cameras,
        Err(error) => {
            log.fail(format!("enumerate() failed: {error}"));
            return ArmOutcome::Ran;
        }
    };
    if cameras.is_empty() {
        return ArmOutcome::skipped("the backend enumerated no cameras");
    }

    let mut seen_ids = BTreeSet::new();
    for info in &cameras {
        log.require(seen_ids.insert(info.id.clone()), || {
            format!("camera id {} is not unique", info.id)
        });
        log.require(info.backend == backend.kind(), || {
            format!(
                "{}: reports backend {} but came from {}; a fake run must never be \
                 mistakable for a hardware one",
                info.id,
                info.backend,
                backend.kind()
            )
        });
        log.require(!info.nodes.is_empty(), || {
            format!("{}: enumerated with no device nodes", info.id)
        });
        log.require(info.nodes.len() <= limits::MAX_NODES_PER_CAMERA, || {
            format!(
                "{}: {} nodes exceeds limits::MAX_NODES_PER_CAMERA ({})",
                info.id,
                info.nodes.len(),
                limits::MAX_NODES_PER_CAMERA
            )
        });

        round_trip_camera_info(info, log);

        for node in &info.nodes {
            let expected = NodeKind::from_device_caps(node.device_caps);
            log.require(node.kind == expected, || {
                format!(
                    "{}: node {} is classified {:?} but its device_caps {:#010x} say {:?} \
                     — classification reads device_caps, never node numbering [PF:7]",
                    info.id, node.path, node.kind, node.device_caps, expected
                )
            });
            let metadata_only = node.device_caps & CAP_META_CAPTURE != 0
                && node.device_caps & CAP_VIDEO_CAPTURE == 0;
            log.require(!metadata_only || node.kind == NodeKind::MetaCapture, || {
                format!(
                    "{}: metadata node {} is not classified as one",
                    info.id, node.path
                )
            });
        }

        if let Some(capture) = info.capture_node() {
            log.require(capture.device_caps & CAP_VIDEO_CAPTURE != 0, || {
                format!(
                    "{}: capture node {} does not carry VIDEO_CAPTURE in device_caps \
                     {:#010x}",
                    info.id, capture.path, capture.device_caps
                )
            });
            log.require(capture.kind == NodeKind::VideoCapture, || {
                format!(
                    "{}: capture node {} is classified {:?}",
                    info.id, capture.path, capture.kind
                )
            });
        }
    }

    ArmOutcome::Ran
}

/// Every `CameraInfo` crosses the wire and the `--json` surface; a field that does not
/// survive serde is a field the daemon silently drops.
fn round_trip_camera_info(info: &CameraInfo, log: &mut ArmLog<'_>) {
    let json = match serde_json::to_string(info) {
        Ok(json) => json,
        Err(error) => {
            log.fail(format!("{}: does not serialize: {error}", info.id));
            return;
        }
    };
    match serde_json::from_str::<CameraInfo>(&json) {
        Ok(back) => log.require(&back == info, || {
            format!("{}: changed on a JSON round trip", info.id)
        }),
        Err(error) => log.fail(format!("{}: does not deserialize: {error}", info.id)),
    }
}

fn arm_control_model(backend: &dyn CameraBackend, log: &mut ArmLog<'_>) -> ArmOutcome {
    let mut visit = CameraVisit::new(backend, log);
    let Some(cameras) = visit.cameras() else {
        return visit.into_skip();
    };

    for info in cameras {
        let Some(mut camera) = visit.open(&info) else {
            continue;
        };
        let controls = match camera.controls() {
            Ok(controls) => controls,
            Err(error) => {
                visit
                    .log
                    .fail(format!("{}: controls() failed: {error}", info.id));
                continue;
            }
        };
        if controls.is_empty() {
            visit.note("a camera reported no controls");
            continue;
        }
        visit.examined += 1;

        let mut seen = BTreeSet::new();
        for desc in &controls {
            visit.log.require(seen.insert(desc.id), || {
                format!("{}: control id {} appears twice", info.id, desc.id)
            });
            check_control_descriptor(&info.id.to_string(), desc, visit.log);
            check_current_is_replayed_not_corrected(camera.as_mut(), desc, visit.log);
        }
    }

    visit.finish("no camera exposed a control set")
}

/// The D2 invariants that hold for one descriptor whatever the device is.
fn check_control_descriptor(camera: &str, desc: &ControlDesc, log: &mut ArmLog<'_>) {
    let json = match serde_json::to_string(desc) {
        Ok(json) => json,
        Err(error) => {
            log.fail(format!(
                "{camera}: {} does not serialize: {error}",
                desc.slug
            ));
            return;
        }
    };
    match serde_json::from_str::<ControlDesc>(&json) {
        Ok(back) => {
            log.require(&back == desc, || {
                format!(
                    "{camera}: {} changed on a JSON round trip — an unknown type, a sparse \
                     menu, or an out-of-range value was lost [PF:1, PF:2, PF:4, PF:5]",
                    desc.slug
                )
            });
            log.require(back.menu.keys().eq(desc.menu.keys()), || {
                format!(
                    "{camera}: {}'s menu indices changed on a round trip; the holes are \
                     real [PF:2]",
                    desc.slug
                )
            });
        }
        Err(error) => log.fail(format!(
            "{camera}: {} does not deserialize: {error}",
            desc.slug
        )),
    }

    // The unknown-type property, named where it lives: the decoder is total, so a type
    // this build cannot interpret still survives the trip to the kernel and back [PF:1].
    if let ControlType::Unknown { raw } = desc.control_type {
        log.require(ControlType::from_raw(raw) == desc.control_type, || {
            format!(
                "{camera}: {} carries type {raw:#x} as Unknown but the decoder names it \
                 {:?}",
                desc.slug, desc.control_type
            )
        });
    }

    match ControlSlug::from_name(&desc.name) {
        Some(derived) => log.require(derived == desc.slug, || {
            format!(
                "{camera}: control {:?} has slug {} but the D2 transform derives {derived} \
                 — agents type the derived spelling",
                desc.name, desc.slug
            )
        }),
        None => log.fail(format!(
            "{camera}: control {:?} slugs to nothing, yet the backend supplied {} — an \
             invented handle collides silently",
            desc.name, desc.slug
        )),
    }
}

/// PF:4 and PF:5: a value outside its declared range is a fact about the device. Reading
/// it back must produce the same fact, not a corrected one.
fn check_current_is_replayed_not_corrected(
    camera: &mut dyn Camera,
    desc: &ControlDesc,
    log: &mut ArmLog<'_>,
) {
    let Some(current) = &desc.current else {
        // A control with no value has nothing to replay, and the two reasons it can have
        // none are not the same fact. A declared absence is ordinary and silent; a
        // *declined* read is the device having refused this pass, and dropping it out of
        // the PF:4 claim with no line makes the arm's own count a number that quietly
        // means less than it says (AGENTS rule 3, note **N199**).
        if desc.value_was_declined() {
            log.note(format!(
                "{}: not compared — the device declined to read its current value, which \
                 is availability rather than a property of the control [PF:4's \
                 population, minus one]",
                desc.slug
            ));
        }
        return;
    };
    if desc.flags.has(KnownFlag::WriteOnly)
        || desc.is_volatile()
        || matches!(
            desc.control_type,
            ControlType::Button | ControlType::ControlClass
        )
    {
        return;
    }
    match camera.get(desc.id) {
        Ok(read) => log.require(&read == current, || {
            // The hint fires whichever side is the out-of-range one: a backend that
            // tidies the *enumerated* value and a backend that tidies the *read* value
            // are the same defect seen from opposite ends.
            let read_out_of_range = read
                .as_int()
                .is_some_and(|value| !desc.range.contains(value));
            let hint = if desc.current_out_of_range() || read_out_of_range {
                " — an out-of-range value is reported, never corrected [PF:4]"
            } else {
                ""
            };
            format!(
                "{}: enumerated as {current} but get() returned {read}{hint}",
                desc.slug
            )
        }),
        Err(error) => log.fail(format!("{}: get() failed: {error}", desc.slug)),
    }
}

fn arm_write_read_back(backend: &dyn CameraBackend, log: &mut ArmLog<'_>) -> ArmOutcome {
    let mut visit = CameraVisit::new(backend, log);
    let Some(cameras) = visit.cameras() else {
        return visit.into_skip();
    };

    for info in cameras {
        let Some(mut camera) = visit.open(&info) else {
            continue;
        };
        let Some(controls) = read_controls(camera.as_mut(), &info.id.to_string(), visit.log) else {
            continue;
        };

        let writable: Vec<&ControlDesc> = controls.iter().filter(|d| is_perturbable(d)).collect();
        if writable.is_empty() {
            visit.note("a camera exposed no writable scalar control to write back");
            continue;
        }
        // The PF:6 probe is the point of this arm, so the decision to run is made before
        // anything is written — otherwise "ran while declared skipped" would depend on
        // how far the arm got.
        let Some(clamp_probe) = writable
            .iter()
            .copied()
            .find(|d| is_clamp_probe_candidate(d))
        else {
            visit.note(
                "a camera exposed no non-motorized integer control to probe clamping on \
                 (design §5 keeps motors off their limits)",
            );
            continue;
        };
        visit.examined += 1;

        for desc in writable.iter().copied() {
            write_back_identity(camera.as_mut(), desc, visit.log);
        }
        probe_clamp(camera.as_mut(), clamp_probe, visit.log);
    }

    visit.finish("no camera offered a control this arm could write")
}

/// Writing a control's own current value back must be an exact, reported no-op.
fn write_back_identity(camera: &mut dyn Camera, desc: &ControlDesc, log: &mut ArmLog<'_>) {
    let Some(ControlValue::Int(current)) = desc.current.clone() else {
        return;
    };
    let requested = ControlValue::Int(current);
    match camera.set(desc.id, requested.clone()) {
        Ok(applied) => {
            log.require(applied.control == desc.id, || {
                format!(
                    "{}: set() reported control {} instead of {}",
                    desc.slug, applied.control, desc.id
                )
            });
            log.require(applied.slug == desc.slug, || {
                format!(
                    "{}: set() reported slug {} instead",
                    desc.slug, applied.slug
                )
            });
            log.require(applied.requested == requested, || {
                format!(
                    "{}: set() reported requested {} for a write of {requested} — the pair \
                     is the doctrine (E4)",
                    desc.slug, applied.requested
                )
            });
            log.require(applied.is_exact(), || {
                format!(
                    "{}: writing its own current value {requested} applied {} instead",
                    desc.slug, applied.applied
                )
            });
        }
        Err(error) => log.fail(format!(
            "{}: writing its own current value {requested} failed: {error}",
            desc.slug
        )),
    }
}

/// PF:6, as an assertion: drivers clamp out-of-range writes and report success. A backend
/// that turns the clamp into an error has converted "the device adjusted it" into "the
/// device refused", which is exactly the confusion E3 and D13 exist to prevent.
fn probe_clamp(camera: &mut dyn Camera, desc: &ControlDesc, log: &mut ArmLog<'_>) {
    let Some(ControlValue::Int(original)) = desc.current.clone() else {
        return;
    };
    let Some(beyond) = desc.range.max.checked_add(CLAMP_PROBE_OVERSHOOT) else {
        return;
    };
    let requested = ControlValue::Int(beyond);

    match camera.set(desc.id, requested.clone()) {
        Err(error) => log.fail(format!(
            "{}: writing {beyond} past its maximum {} failed with {error}; an \
             out-of-range write is a clamped success, never an error [PF:6]",
            desc.slug, desc.range.max
        )),
        Ok(applied) => {
            log.require(applied.requested == requested, || {
                format!(
                    "{}: a clamped write must still report what was requested, got {}",
                    desc.slug, applied.requested
                )
            });
            match applied.applied.as_int() {
                Some(value) => {
                    log.require(desc.range.contains(value), || {
                        format!(
                            "{}: a write of {beyond} applied {value}, outside the declared \
                             range [{}..={}] it should have been clamped into",
                            desc.slug, desc.range.min, desc.range.max
                        )
                    });
                    log.require(value != beyond, || {
                        format!(
                            "{}: a write of {beyond} past maximum {} reported itself \
                             applied verbatim",
                            desc.slug, desc.range.max
                        )
                    });
                }
                None => log.fail(format!(
                    "{}: a scalar write read back as {}",
                    desc.slug, applied.applied
                )),
            }
            log.require(!applied.warnings.is_empty(), || {
                format!(
                    "{}: a write of {beyond} was adjusted to {} without a warning; a silent \
                     adjustment is the fact E4 exists to keep",
                    desc.slug, applied.applied
                )
            });
            log.require(
                applied.warnings.iter().any(|w| {
                    matches!(
                        w,
                        WriteWarning::Clamped { .. }
                            | WriteWarning::StepAligned { .. }
                            | WriteWarning::Adjusted { .. }
                    )
                }),
                || format!("{}: adjustment warning list is empty of reasons", desc.slug),
            );
        }
    }

    // Leave the camera as we found it, and assert that we did (docs/8 Part C:
    // "restoration by assumption").
    match camera.set(desc.id, ControlValue::Int(original)) {
        Ok(applied) => log.require(applied.applied == ControlValue::Int(original), || {
            format!(
                "{}: restoring {original} after the clamp probe applied {} instead",
                desc.slug, applied.applied
            )
        }),
        Err(error) => log.fail(format!(
            "{}: restoring {original} after the clamp probe failed: {error}",
            desc.slug
        )),
    }
}

fn arm_snapshot_restore_inverse(backend: &dyn CameraBackend, log: &mut ArmLog<'_>) -> ArmOutcome {
    let mut visit = CameraVisit::new(backend, log);
    let Some(cameras) = visit.cameras() else {
        return visit.into_skip();
    };

    for info in cameras {
        let Some(mut camera) = visit.open(&info) else {
            continue;
        };
        let Some(before) = read_controls(camera.as_mut(), &info.id.to_string(), visit.log) else {
            continue;
        };

        let perturbations: Vec<(ControlId, ControlValue)> = before
            .iter()
            .filter(|d| is_perturbable(d) && !d.is_inactive())
            .filter_map(|d| perturbation(d).map(|v| (d.id, v)))
            .collect();
        if perturbations.is_empty() {
            visit.note("a camera exposed no control this arm could perturb");
            continue;
        }
        visit.examined += 1;

        let snapshot = snapshot_of(&info.fingerprint, &before);
        let flags_before: BTreeMap<ControlId, u32> =
            before.iter().map(|d| (d.id, d.flags.raw)).collect();

        // Everything between the first write and the restore happens inside the guard's
        // scope, so that **no path out of it can skip putting the camera back** — see
        // [`RestoreGuard`]. The guard's own complaints land here rather than in the log
        // directly, because `Drop` cannot borrow a log the block is already using.
        let mut restore_complaints = Vec::new();
        let perturbed_readable = {
            let guard = RestoreGuard {
                camera: camera.as_mut(),
                snapshot,
                complaints: &mut restore_complaints,
            };

            for (id, value) in &perturbations {
                if let Err(error) = guard.camera.set(*id, value.clone()) {
                    visit.log.fail(format!(
                        "perturbing control {id} to {value} failed: {error}"
                    ));
                }
            }

            // Non-vacuity: if nothing actually moved, "restore put it back" proves nothing.
            match read_controls(&mut *guard.camera, &info.id.to_string(), visit.log) {
                Some(perturbed) => {
                    let moved = perturbed
                        .iter()
                        .filter(|after| {
                            before
                                .iter()
                                .any(|b| b.id == after.id && b.current != after.current)
                        })
                        .count();
                    visit.log.require(moved > 0, || {
                        format!(
                            "{}: {} perturbations left every control where it was, so the \
                             restore this arm is named for would prove nothing",
                            info.id,
                            perturbations.len()
                        )
                    });
                    true
                }
                // The early exit this arm used to take *before* the restore, leaving a
                // real camera perturbed (note **N137**). It still exits — the read failed
                // and there is nothing left to compare — but the guard has put the camera
                // back by the time the block ends.
                None => false,
            }
        };
        for complaint in restore_complaints {
            visit.log.fail(complaint);
        }
        if !perturbed_readable {
            continue;
        }

        let Some(after) = read_controls(camera.as_mut(), &info.id.to_string(), visit.log) else {
            continue;
        };
        compare_control_state(&before, &after, &flags_before, visit.log);
    }

    visit.finish("no camera offered a control this arm could perturb")
}

/// Puts the snapshot back when it goes out of scope, however it goes out of scope.
///
/// **AGENTS rule 8 is not conditional on the rest of the arm succeeding.** This arm writes
/// every perturbation it planned and then re-reads the device to prove something moved; the
/// re-read can fail — a camera unplugged mid-arm, a driver that stopped answering — and
/// until 2026-08-16 that failure `continue`d to the next camera with the restore still
/// ahead of it, leaving a real device holding this suite's perturbations. §2.11 step 4
/// tells the author of every new backend to run this battery **against their device**, so
/// the population that finding lands on is "somebody else's camera" (note **N137**).
///
/// The shape is the tree's own answer to the same question: `capture::grab`'s `StreamGuard`
/// stops the stream its scope started and `actor::Liveness` marks the actor dead, both on
/// `Drop`, both for paths a reader cannot enumerate. A `Drop` cannot be skipped by an early
/// return, a `?`, or a panic, which is exactly the set of exits an arm grows over time.
///
/// **What it costs, since a reader meets it here and the G6 re-read asked.** A `Drop` that
/// runs while the block is already panicking, and itself panics, aborts the process. The
/// write below can panic — a backend that panics is a *measured* mode in this tree
/// (\[PF:1\]: the `v4l` crate's `query_controls` panics on modern kernels), and the old
/// explicit loop would simply have been skipped on that path. It is kept anyway, and
/// deliberately: `capture::grab`'s `StreamGuard` and `preview::Resuming` carry the identical
/// exposure for the identical reason, so this is house-consistent rather than novel, and the
/// trade is a real device left perturbed on *every* early exit against a worse panic message
/// on one. Catching the unwind here would swallow the finding the arm exists to report; the
/// answer if it ever bites is a backend that does not panic, which AGENTS' "no
/// `unwrap`/`expect`/`panic` on device-driven paths" already requires.
struct RestoreGuard<'a, 'c> {
    camera: &'a mut dyn Camera,
    snapshot: Snapshot,
    /// Where a failed restore is recorded. Not the [`ArmLog`] itself: the block this guard
    /// wraps is using the log, and a `Drop` that also held it could not compile — so the
    /// complaints are drained into the log by the caller a line after the guard falls.
    complaints: &'c mut Vec<String>,
}

impl Drop for RestoreGuard<'_, '_> {
    fn drop(&mut self) {
        for entry in self.snapshot.restore_order() {
            if let Err(error) = self.camera.set(entry.id, entry.value.clone()) {
                self.complaints.push(format!(
                    "restoring {} to {} failed: {error}",
                    entry.control, entry.value
                ));
            }
        }
    }
}

/// The D4 assertion: after snapshot → perturb → restore, the control state is what it
/// was. Values *and* raw flags, because PF:3's INACTIVE bit is control state too: an
/// automation control restored without its partner's flag coming back is a camera left
/// in a third state.
fn compare_control_state(
    before: &[ControlDesc],
    after: &[ControlDesc],
    flags_before: &BTreeMap<ControlId, u32>,
    log: &mut ArmLog<'_>,
) {
    for original in before {
        let Some(now) = after.iter().find(|d| d.id == original.id) else {
            log.fail(format!(
                "{} vanished from the control set across a snapshot/restore cycle",
                original.slug
            ));
            continue;
        };
        log.require(now.current == original.current, || {
            format!(
                "{}: was {:?} before the cycle and is {:?} after it",
                original.slug, original.current, now.current
            )
        });
        if let Some(&raw) = flags_before.get(&original.id) {
            log.require(now.flags.raw == raw, || {
                format!(
                    "{}: flags were {raw:#06x} before the cycle and are {:#06x} after it \
                     [PF:3]",
                    original.slug, now.flags.raw
                )
            });
        }
    }
}

/// The snapshot the D4 ordering is derived from. Roles come from the shape predicate in
/// `webcam-handler-schema::pairing`, so the battery does not hold a second opinion about
/// what automation looks like.
fn snapshot_of(fingerprint: &CameraFingerprint, controls: &[ControlDesc]) -> Snapshot {
    Snapshot {
        taken_at: Stamp::epoch(),
        camera: fingerprint.clone(),
        entries: controls
            .iter()
            .filter(|d| is_perturbable(d) && !d.is_inactive())
            .filter_map(|d| {
                d.current.clone().map(|value| SnapshotEntry {
                    control: d.slug.clone(),
                    id: d.id,
                    value,
                    role: if looks_like_automation(d) {
                        ControlRole::Automation
                    } else {
                        ControlRole::Manual
                    },
                    was_inactive: d.is_inactive(),
                    was_volatile: d.is_volatile(),
                })
            })
            .collect(),
        declined: Vec::new(),
    }
}

fn arm_stream_lifecycle(backend: &dyn CameraBackend, log: &mut ArmLog<'_>) -> ArmOutcome {
    let mut visit = CameraVisit::new(backend, log);
    let Some(cameras) = visit.cameras() else {
        return visit.into_skip();
    };

    for info in cameras {
        if info.capture_node().is_none() {
            visit.note("a camera has no capture node, so it is listed but not streamable");
            continue;
        }
        let Some(mut camera) = visit.open(&info) else {
            continue;
        };
        // Twice, because "stop released everything" is only observable from the second
        // start.
        let mut cycles_run = 0u32;
        for cycle in 1..=2u32 {
            match stream_once(camera.as_mut(), cycle, visit.log) {
                StreamAttempt::Ran => cycles_run += 1,
                StreamAttempt::Unavailable(reason) => {
                    visit.note(reason);
                    break;
                }
            }
        }
        if cycles_run == 2 {
            visit.examined += 1;
        }
    }

    visit.finish("no camera could be streamed")
}

/// D5's explicit-request contract, on whichever backend is in front of us.
///
/// **The arm §9.1 of the G6 review says would have caught H1 the day the fake grew its
/// guard.** Every other streaming arm here constructs `StreamRequest::default()`, so no arm
/// of this battery could express *any* explicit-request contract — and the two tests that
/// did pin it both ran over the fake, which honoured it, while the V4L2 backend ranked a
/// named-but-absent format into another one and photographed that (note **N134**). A rubric
/// row names a class; only a walked population finds an instance of it, and for this class
/// the population is every backend that implements T2.
///
/// Both halves are asserted per camera because they fail differently: the format half is
/// answered before a device is touched, and the size half is answered after a format has
/// been chosen and its size list walked.
fn arm_explicit_request(backend: &dyn CameraBackend, log: &mut ArmLog<'_>) -> ArmOutcome {
    let mut visit = CameraVisit::new(backend, log);
    let Some(cameras) = visit.cameras() else {
        return visit.into_skip();
    };

    for info in cameras {
        if info.capture_node().is_none() {
            visit.note(
                "a camera has no capture node, so it enumerates no format to name one absent from",
            );
            continue;
        }
        let Some(mut camera) = visit.open(&info) else {
            continue;
        };
        let formats = match camera.formats() {
            Ok(formats) => formats,
            // **Availability is not capability** (AGENTS rule 7, doctrine E3), and this arm
            // answered that correctly for `start_stream` and not for the enumeration that
            // precedes it until 2026-08-16: a camera grabbed by another process between
            // `open` and `formats` turned a whole battery run red for a fact about who
            // holds the device (note **N138**). The two halves say the same thing now.
            Err(error @ (Error::Busy { .. } | Error::PermissionDenied { .. })) => {
                visit.note(format!(
                    "{}: could not be asked what it offers ({error})",
                    info.id
                ));
                continue;
            }
            Err(error) => {
                visit
                    .log
                    .fail(format!("{}: formats() failed: {error}", info.id));
                continue;
            }
        };
        if formats.is_empty() {
            visit.note(format!(
                "{}: the capture node enumerated no formats, so there is nothing for a \
                 request to be refused *against*",
                info.id
            ));
            continue;
        }

        let mut exercised = 0u32;
        for claim in [
            refuses_an_absent_format(camera.as_mut(), &info, &formats, visit.log),
            refuses_an_unfittable_size(camera.as_mut(), &info, &formats, visit.log),
        ] {
            match claim {
                Claim::Asked => exercised += 1,
                Claim::NotAsked(why) => visit.note(why),
            }
        }
        if exercised == 2 {
            visit.examined += 1;
        }
    }

    visit.finish("no camera enumerated a format list to build an unanswerable request from")
}

/// A FourCC no device enumerates, because nobody has ever issued it.
///
/// Invented rather than borrowed from the D6 set: a real fourcc would make this arm depend
/// on which formats the camera in front of it happens to lack, and a camera that offered
/// all of them would turn the arm into a silent pass. The check below still asserts the
/// enumeration lacks it, because "the fixture cannot exercise the rule it pins" is the
/// smell this whole battery exists on the other side of.
const NEVER_ENUMERATED: &str = "WCHX";

/// Whether a claim reached the device at all.
///
/// [`StreamAttempt`]'s shape and for its reason: "the contract does not hold" and "the
/// contract could not be put to this camera" are different facts, and a helper answering a
/// bare `false` has already collapsed them — which is the conversion AGENTS rule 7 forbids,
/// one layer in from where it usually happens.
enum Claim {
    /// The device was asked and answered; whether the answer was right is in the log.
    Asked,
    /// It was not asked, and this is why — a named skip rather than a failure.
    NotAsked(String),
}

/// Half one: a named format the device does not enumerate is refused.
fn refuses_an_absent_format(
    camera: &mut dyn Camera,
    info: &CameraInfo,
    formats: &[FormatInfo],
    log: &mut ArmLog<'_>,
) -> Claim {
    let Some(absent) = PixelFormat::parse(NEVER_ENUMERATED) else {
        log.fail(format!(
            "{NEVER_ENUMERATED} is not four characters, so this arm has nothing to ask for"
        ));
        return Claim::NotAsked(format!("{NEVER_ENUMERATED} is not a pixel format"));
    };
    if formats.iter().any(|f| f.pixel_format == absent) {
        // A failure and not a skip: the arm's own fixture has stopped being absent, and a
        // request for a format the camera *has* would pass while proving nothing.
        log.fail(format!(
            "{}: enumerates {absent}, which this arm uses precisely because no device \
             issues it — the request below would prove nothing",
            info.id
        ));
        return Claim::NotAsked(format!("{}: enumerates {absent}", info.id));
    }

    let request = StreamRequest {
        pixel_format: Some(absent),
        ..StreamRequest::default()
    };
    match camera.start_stream(&request) {
        Ok(negotiated) => {
            // Stopped before complaining: the arm started this stream, and a camera left
            // streaming for an assertion is a camera the next arm finds busy.
            let _ = camera.stop_stream();
            log.fail(format!(
                "{}: asked for {absent}, which this camera does not enumerate, and got a \
                 stream in {} at {}x{} — D5's \"an explicit request still wins … or a \
                 typed refusal\" allows neither substitution nor silence",
                info.id, negotiated.pixel_format, negotiated.width, negotiated.height
            ));
            Claim::Asked
        }
        Err(Error::FormatUnsupported {
            requested,
            available,
            size,
            container,
        }) => {
            log.require(size.is_none() && container.is_none(), || {
                format!(
                    "{}: refused an absent format with a payload that also names a size or a \
                     container — the causes are exclusive, and a refusal naming two levers is \
                     one an unattended caller has to guess at (notes **N138**, **N211**)",
                    info.id
                )
            });
            log.require(requested == Some(absent), || {
                format!(
                    "{}: refused a request for {absent} while naming {requested:?} as what \
                     was asked for",
                    info.id
                )
            });
            log.require(!available.is_empty(), || {
                format!(
                    "{}: refused {absent} without naming one format it does have, which is \
                     the whole remedy an unattended caller has",
                    info.id
                )
            });
            Claim::Asked
        }
        // Availability is not capability (E3): a device somebody else holds, or one this
        // process may not open, has not told us anything about what it can offer — so this
        // is a named skip, and the arm's own accounting is what stops a run of them
        // reading as a pass.
        Err(error @ (Error::Busy { .. } | Error::PermissionDenied { .. })) => Claim::NotAsked(
            format!("{}: could not be asked for a format ({error})", info.id),
        ),
        Err(error) => {
            log.fail(format!(
                "{}: a format this camera does not enumerate was refused with {error} \
                 rather than FormatUnsupported — collapsing the two makes an unattended \
                 caller guess which one it is",
                info.id
            ));
            Claim::Asked
        }
    }
}

/// Half two: a named size no enumerated mode can deliver is refused (owner ruling,
/// 2026-08-16).
fn refuses_an_unfittable_size(
    camera: &mut dyn Camera,
    info: &CameraInfo,
    formats: &[FormatInfo],
    log: &mut ArmLog<'_>,
) -> Claim {
    // A device that can deliver a 1×1 frame would make the request below answerable, and
    // an arm that cannot tell "refused" from "there was nothing to refuse" is the vacuity
    // this file's skip accounting exists to prevent. No camera this project has met is
    // stepwise, let alone down to one pixel — but the arm asks rather than assuming.
    //
    // **The premise is the resolver's own rule**, and saying so is worth a line because for
    // two days it was not: `StreamRequest::choose` refused when the *chosen* format could
    // not deliver while this arm required a refusal only when **no** format could, so the
    // arm passed on the OBSBOT while `--size 640x480` was being refused there — the review's
    // finding one level in, a premise narrower than the rule it pins (note **N138**). The
    // two are the same question now: device-wide, across every format.
    let deliverable = formats
        .iter()
        .flat_map(|format| format.sizes.iter())
        .any(|entry| entry.size.largest_within(1, 1).is_some());
    if deliverable {
        return Claim::NotAsked(format!(
            "{}: offers a mode that fits inside 1x1, so this arm cannot name a size \
             nothing fits",
            info.id
        ));
    }

    let request = StreamRequest {
        width: Some(1),
        height: Some(1),
        ..StreamRequest::default()
    };
    match camera.start_stream(&request) {
        Ok(negotiated) => {
            let _ = camera.stop_stream();
            log.fail(format!(
                "{}: asked for 1x1, which no mode this camera enumerates can deliver, and \
                 got {}x{} — the largest thing on offer is not an adjustment of the \
                 smallest thing asked for",
                info.id, negotiated.width, negotiated.height
            ));
            Claim::Asked
        }
        Err(Error::FormatUnsupported {
            available,
            size,
            requested,
            container,
        }) => {
            log.require(!available.is_empty(), || {
                format!(
                    "{}: refused a size while claiming to enumerate no format, which \
                     contradicts the list this arm read a moment ago",
                    info.id
                )
            });
            // A `start_stream` names no file, so the container cause cannot be what refused
            // this — and a payload carrying it would be a second lever beside the size
            // (note **N211**).
            log.require(container.is_none(), || {
                format!(
                    "{}: refused a size with a payload that also names a container, which is \
                     two levers for a caller that can pull one",
                    info.id
                )
            });
            // The half that stops the refusal being a sentence about the wrong thing: a
            // size refusal must name the size (note **N138**). It rendered "format
            // (unspecified) is unavailable; MJPG, YUYV would be accepted" until 2026-08-16,
            // which sends an unattended caller to change its `--pixel-format` and meet the
            // identical refusal — N129's class, at this variant, one phase later.
            match size {
                Some(size) => log.require(
                    (size.requested_width, size.requested_height) == (1, 1),
                    || {
                        format!(
                            "{}: refused 1x1 while reporting {}x{} as the size that was \
                             asked for",
                            info.id, size.requested_width, size.requested_height
                        )
                    },
                ),
                None => log.fail(format!(
                    "{}: refused a size with a refusal that names only formats \
                     ({requested:?}, {available:?}) — a caller repairing its request from \
                     this payload changes the half that was answerable and loops",
                    info.id
                )),
            }
            Claim::Asked
        }
        Err(error @ (Error::Busy { .. } | Error::PermissionDenied { .. })) => Claim::NotAsked(
            format!("{}: could not be asked for a size ({error})", info.id),
        ),
        Err(error) => {
            log.fail(format!(
                "{}: a size nothing fits was refused with {error} rather than \
                 FormatUnsupported",
                info.id
            ));
            Claim::Asked
        }
    }
}

/// What one start→frames→stop cycle did.
enum StreamAttempt {
    Ran,
    Unavailable(String),
}

fn stream_once(camera: &mut dyn Camera, cycle: u32, log: &mut ArmLog<'_>) -> StreamAttempt {
    let request = StreamRequest::default();
    let negotiated = match camera.start_stream(&request) {
        Ok(negotiated) => negotiated,
        // Availability is not capability (E3): a device somebody else holds has not told
        // us it cannot stream.
        Err(error @ (Error::Busy { .. } | Error::PermissionDenied { .. })) => {
            return StreamAttempt::Unavailable(format!("a camera could not be streamed: {error}"));
        }
        Err(Error::FormatUnsupported { available, .. }) if available.is_empty() => {
            return StreamAttempt::Unavailable(
                "a camera's capture node enumerated no formats".to_owned(),
            );
        }
        Err(error) => {
            log.fail(format!("start_stream() on cycle {cycle} failed: {error}"));
            return StreamAttempt::Unavailable("a camera refused to start streaming".to_owned());
        }
    };

    for index in 0..FRAMES_PER_CYCLE {
        let deadline = Instant::now() + Duration::from_millis(limits::FRAME_DEADLINE_MS);
        match camera.next_frame(deadline) {
            Ok(frame) => check_frame(&frame, &negotiated, index, log),
            Err(error) => {
                log.fail(format!(
                    "next_frame() failed on cycle {cycle} frame {index}: {error}"
                ));
                break;
            }
        }
    }

    if let Err(error) = camera.stop_stream() {
        log.fail(format!("stop_stream() on cycle {cycle} failed: {error}"));
    }
    StreamAttempt::Ran
}

/// A frame's byte count is a claim about its format and size; a frame that disagrees with
/// its own header is how a decoder learns to read past a buffer.
fn check_frame(frame: &Frame, negotiated: &NegotiatedStream, index: u32, log: &mut ArmLog<'_>) {
    log.require(frame.pixel_format == negotiated.pixel_format, || {
        format!(
            "frame {index} arrived as {} on a stream negotiated as {}",
            frame.pixel_format, negotiated.pixel_format
        )
    });
    log.require(
        frame.width == negotiated.width && frame.height == negotiated.height,
        || {
            format!(
                "frame {index} is {}x{} on a stream negotiated at {}x{}",
                frame.width, frame.height, negotiated.width, negotiated.height
            )
        },
    );
    log.require(!frame.bytes.is_empty(), || {
        format!("frame {index} carries no bytes")
    });

    if frame.pixel_format.is_compressed() {
        // The JPEG start-of-image marker. PF:9 checked for it by hand on real hardware;
        // here it is the standing assertion.
        log.require(frame.bytes.starts_with(&[0xff, 0xd8]), || {
            format!(
                "frame {index} is declared {} but does not start with a JPEG SOI marker",
                frame.pixel_format
            )
        });
        if negotiated.size_image > 0 {
            let cap = usize::try_from(negotiated.size_image).unwrap_or(usize::MAX);
            log.require(frame.bytes.len() <= cap, || {
                format!(
                    "frame {index} is {} bytes, past the driver's declared maximum {cap}",
                    frame.bytes.len()
                )
            });
        }
    } else if let Some(expected) = uncompressed_frame_len(frame) {
        log.require(frame.bytes.len() == expected, || {
            format!(
                "frame {index} is {} bytes; {} at {}x{} (stride {}) is {expected}",
                frame.bytes.len(),
                frame.pixel_format,
                frame.width,
                frame.height,
                frame.bytes_per_line
            )
        });
    }
}

/// The exact byte count an uncompressed frame must have, or `None` for a format whose
/// layout this build does not know — an unknown format is represented, not guessed at
/// (D2).
fn uncompressed_frame_len(frame: &Frame) -> Option<usize> {
    let width = usize::try_from(frame.width).ok()?;
    let height = usize::try_from(frame.height).ok()?;
    let stride = usize::try_from(frame.bytes_per_line).ok()?;
    let row = |packed: usize| if stride > 0 { stride } else { packed };
    match frame.pixel_format {
        PixelFormat::YUYV => row(width.checked_mul(2)?).checked_mul(height),
        PixelFormat::GREY => row(width).checked_mul(height),
        // NV12 is a full-size luma plane followed by a half-height interleaved chroma
        // plane.
        PixelFormat::NV12 => {
            let luma = row(width).checked_mul(height)?;
            luma.checked_add(luma / 2)
        }
        _ => None,
    }
}

fn arm_hotplug_watch(backend: &dyn CameraBackend, log: &mut ArmLog<'_>) -> ArmOutcome {
    let mut watch = match backend.watch() {
        Ok(watch) => watch,
        Err(error) => {
            log.fail(format!("watch() failed: {error}"));
            return ArmOutcome::Ran;
        }
    };

    let started = Instant::now();
    let deadline = started + Duration::from_millis(HOTPLUG_POLL_MS);
    match watch.next_event(deadline) {
        // E3: the deadline arriving first is an answer, not a failure. A backend that
        // reports a timeout as an error teaches every caller to treat "quiet" as "broken".
        Ok(None) => {}
        Ok(Some(event)) => {
            let path = match &event {
                HotplugEvent::Added { path } | HotplugEvent::Removed { path } => path,
            };
            log.require(!path.as_str().is_empty(), || {
                format!("a hotplug event named an empty path: {event:?}")
            });
        }
        Err(error) => log.fail(format!(
            "next_event() at a deadline returned {error}; a timeout is Ok(None) (E3)"
        )),
    }

    // Bounded everything (rubric A14): a watch that ignores its deadline is a hang that
    // has not happened yet.
    let waited = started.elapsed();
    log.require(
        waited <= Duration::from_millis(HOTPLUG_POLL_MS + HOTPLUG_DEADLINE_SLACK_MS),
        || {
            format!(
                "next_event() honored a {HOTPLUG_POLL_MS} ms deadline only after {} ms",
                waited.as_millis()
            )
        },
    );

    ArmOutcome::Ran
}

fn arm_fault_menu(_backend: &dyn CameraBackend, _log: &mut ArmLog<'_>) -> ArmOutcome {
    // Stated rather than pretended: T1/T2 expose no fault-scripting seam, by design — a
    // backend that could be told to fail over the same trait the engine uses would be a
    // backend that can fail by accident. Each backend's fault menu is therefore walked by
    // that backend's own suite (design §2.9), and every backend declares this skip.
    ArmOutcome::skipped(
        "the T1/T2 surface exposes no fault-scripting seam; a backend's fault menu is \
         walked exhaustively by its own suite (design §2.9)",
    )
}

// --------------------------------------------------------------------- shared helpers

/// How many frames each stream cycle takes. Two would prove the sequence advances; three
/// leaves room for the first to be the odd one out \[PF:11\].
const FRAMES_PER_CYCLE: u32 = 3;

/// How far past a control's maximum the PF:6 probe writes. Far enough that no step
/// alignment could land on it by accident.
const CLAMP_PROBE_OVERSHOOT: i64 = 1_000;

/// How long the hotplug arm waits for an event before expecting `Ok(None)`.
const HOTPLUG_POLL_MS: u64 = 50;

/// How far past its deadline a watch may return before the arm calls it a hang. Generous:
/// this is a scheduling allowance, not a performance assertion.
const HOTPLUG_DEADLINE_SLACK_MS: u64 = 2_000;

/// Control name fragments that mean a motor moves (design §5: motors wear, and a
/// conformance run is not a reason to drive one to its limit).
const MOTORIZED_FRAGMENTS: &[&str] = &["pan", "tilt", "zoom", "focus", "roll"];

/// Whether writing this control turns a motor, judged by name because that is all a
/// backend-agnostic suite has.
///
/// Public because §5's motor rule is a law rather than this module's private taste, and
/// the hardware rung has to obey the same one: a second list of fragments is a second
/// answer to "may this test move the camera somebody is pointing at a person".
#[must_use]
pub fn is_motorized(slug: &ControlSlug) -> bool {
    let slug = slug.as_str();
    MOTORIZED_FRAGMENTS.iter().any(|f| slug.contains(f))
}

/// Whether this arm may write to the control at all.
///
/// Volatile controls are excluded because their value is the device's to choose, and
/// INACTIVE-partner semantics are the engine's business (D3), not a backend conformance
/// question. A control sitting off its own step is excluded too: writing such a value
/// back aligns it, so the arm would report a restore failure for a state the device put
/// itself in \[PF:4\].
///
/// Public for the same reason [`is_motorized`] is: the hardware rung perturbs controls
/// too, and it must exclude exactly what this arm excludes.
/// It is [`why_not_perturbable`] answering nothing, and *defined* that way rather than
/// written twice: a bool and a reason kept side by side are two rules that agree until
/// somebody edits one of them (note **N72**).
#[must_use]
pub fn is_perturbable(desc: &ControlDesc) -> bool {
    why_not_perturbable(desc).is_none()
}

/// Whether `value` sits on one of the control's steps, counting from its minimum.
fn is_step_aligned(range: &ControlRange, value: i64) -> bool {
    value
        .checked_sub(range.min)
        .is_some_and(|offset| offset % range.effective_step() == 0)
}

// ------------------------------------------------- which control a hardware arm may sweep

/// One reason a control is not one a test may write to or sweep.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Disqualifier {
    /// The READ_ONLY flag is set.
    ReadOnly,
    /// The DISABLED flag is set.
    Disabled,
    /// A control-class header rather than a control.
    ControlClass,
    /// Not writable, by a term this enum does not name yet.
    NotWritable,
    /// The type carries a payload rather than a scalar.
    NotScalar {
        /// The type, so the message names it.
        control_type: ControlType,
    },
    /// The VOLATILE flag is set.
    Volatile,
    /// The WRITE_ONLY flag is set.
    WriteOnly,
    /// The descriptor carries no current value.
    CurrentUnknown,
    /// The current value is not an integer.
    CurrentNotAnInteger {
        /// What it is instead.
        current: ControlValue,
    },
    /// The current value sits outside the control's own declared range \[PF:4\].
    CurrentOutsideRange {
        /// As read.
        current: i64,
        /// Declared minimum.
        min: i64,
        /// Declared maximum.
        max: i64,
    },
    /// The current value is not a whole number of steps above the minimum \[PF:4\].
    CurrentOffStep {
        /// As read.
        current: i64,
        /// Declared minimum.
        min: i64,
        /// The step it is counted against.
        step: i64,
    },
    /// A motor turns when this control is written (design §5).
    Motorized,
    /// An automation partner owns the control right now \[PF:3\].
    Inactive,
    /// Scalar, but not the plain integer a uniform sweep walks.
    NotAnInteger {
        /// The type, so the message names it.
        control_type: ControlType,
    },
    /// The declared range holds at most one value, so there is nothing to sweep.
    OneValueRange {
        /// Declared minimum.
        min: i64,
        /// Declared maximum.
        max: i64,
    },
}

impl fmt::Display for Disqualifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Disqualifier::ReadOnly => f.write_str("read-only"),
            Disqualifier::Disabled => f.write_str("DISABLED"),
            Disqualifier::ControlClass => {
                f.write_str("a control-class header rather than a control")
            }
            Disqualifier::NotWritable => {
                f.write_str("not writable, by a term this suite does not name yet")
            }
            Disqualifier::NotScalar { control_type } => {
                write!(f, "type is {control_type:?}, which carries a payload")
            }
            Disqualifier::Volatile => f.write_str("VOLATILE, so its value is the device's"),
            Disqualifier::WriteOnly => f.write_str("WRITE_ONLY, so there is nothing to read back"),
            Disqualifier::CurrentUnknown => f.write_str("it reports no current value"),
            Disqualifier::CurrentNotAnInteger { current } => {
                write!(f, "its current value is {current}, not an integer")
            }
            Disqualifier::CurrentOutsideRange { current, min, max } => {
                write!(f, "current {current} outside {min}..={max} [PF:4]")
            }
            Disqualifier::CurrentOffStep { current, min, step } => write!(
                f,
                "current {current} is not a whole number of steps of {step} above {min} [PF:4]"
            ),
            Disqualifier::Motorized => f.write_str("a motor turns when it is written"),
            Disqualifier::Inactive => {
                f.write_str("INACTIVE — an automation partner owns it [PF:3]")
            }
            Disqualifier::NotAnInteger { control_type } => write!(f, "type is {control_type:?}"),
            Disqualifier::OneValueRange { min, max } => {
                write!(f, "range {min}..={max} holds one value")
            }
        }
    }
}

/// Why a camera is not taking part in a brightness-class sweep.
///
/// **Two answers rather than one, because they are facts about different things.** AGENTS
/// rule 7 is "availability is not capability … no code or test converts one into the
/// other", and a predicate that answers a bare `false` has already done the converting: the
/// caller is left holding a bool where the interesting half was *which term said no*.
///
/// [`Decline::NoneNamed`] is a fact about the sensor's **control set**. The attached Chicony
/// IR sensor enumerates three controls and not one of them is brightness-class; no amount of
/// clearing automation would give it one, and the honest sentence is "this camera does not
/// have that control".
///
/// [`Decline::NoneUsable`] is a fact about a control the sensor **has**, and every instance
/// of it is a different story: a `gain` that is INACTIVE because `auto_exposure` is engaged
/// is a *state*, and D3's pairing planner exists precisely to clear it; a `brightness` whose
/// current sits outside its own declared range is the represented-unknown class \[PF:4\] and
/// a device finding worth writing down; a read-only or menu-typed one is a *capability*. The
/// shipped predicate printed the control-set sentence for all four (note **N72**).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decline {
    /// The device enumerates none of [`BRIGHTNESS_CLASS`].
    NoneNamed {
        /// How many controls it does enumerate — so a reader can tell a three-control IR
        /// sensor from a twenty-four-control PTZ camera that simply spells things
        /// differently.
        examined: usize,
    },
    /// The device enumerates one or more of them and every one is disqualified, each by the
    /// term recorded beside it.
    NoneUsable(Vec<(ControlSlug, Disqualifier)>),
}

impl Decline {
    /// What this decline is a fact **about**, as the clause a `SKIP` line uses.
    ///
    /// The distinction AGENTS rule 7 keeps, held in a value a unit test can read rather than
    /// in a sentence a `println!` asserts by being typed. That is the whole of N72's F5: the
    /// old message claimed "a fact about this sensor's control set" over a predicate that
    /// could equally have been refusing a state.
    #[must_use]
    pub const fn is_a_fact_about(&self) -> &'static str {
        match self {
            Decline::NoneNamed { .. } => "this sensor's control set",
            Decline::NoneUsable(_) => "the state of a control this sensor has",
        }
    }
}

impl fmt::Display for Decline {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Decline::NoneNamed { examined } => write!(
                f,
                "exposes none of {} among its {examined} control(s)",
                BRIGHTNESS_CLASS.join(", ")
            ),
            Decline::NoneUsable(refused) => {
                let named: Vec<String> = refused
                    .iter()
                    .map(|(slug, why)| format!("{slug} ({why})"))
                    .collect();
                write!(
                    f,
                    "exposes {}, disqualified by the term in parentheses",
                    named.join(", ")
                )
            }
        }
    }
}

/// What a hardware arm found when it went looking for something to sweep.
#[derive(Debug)]
pub enum SweepTarget<'a> {
    /// A control the arm may run a whole session over.
    Found(&'a ControlDesc),
    /// Nothing it may run one over, and why — named, so the `SKIP` line is a reason rather
    /// than a shrug.
    Declined(Decline),
}

/// The three UVC controls that move luma directly, in the order an arm should prefer them.
///
/// "A brightness-class control" is what docs/7 asks of both calibration rungs, and these
/// three are the population. The list lives here rather than in a suite because it was in
/// *two* suites: `crates/backends/v4l2/tests/hardware.rs` and `crates/client/tests/hardware.rs`
/// each carried a private copy of it and of the predicate below. A selection only a test
/// ever makes still has one home, and this module is the one the workspace already keeps
/// for that kind of answer — [`is_motorized`] and [`is_perturbable`] are here for the same
/// reason, and a private copy inside an ignored hardware binary is a rule nothing can
/// unit-test.
pub const BRIGHTNESS_CLASS: [&str; 3] = ["brightness", "gamma", "gain"];

/// Why a test may not perturb this control, when it may not.
///
/// The reason-answering sibling of [`is_perturbable`], and the **authority** of the pair:
/// that predicate is defined as this one answering nothing, so the two cannot drift into two
/// rules the way a `bool` and a message beside it can.
///
/// The terms are checked in the order [`is_perturbable`]'s conjunction reads, and the first
/// one that fires is the answer. A control disqualified twice over names the first term; a
/// reader who clears it and re-runs meets the second, which is the same conversation an
/// operator has with a device anyway.
#[must_use]
pub fn why_not_perturbable(desc: &ControlDesc) -> Option<Disqualifier> {
    if !desc.is_writable() {
        // The **verdict** is [`ControlDesc::is_writable`]'s and stays there; only the
        // *diagnosis* is here, because that predicate folds three device facts into one
        // `false` and they are not the same news — DISABLED is the device saying "not on
        // this model", READ_ONLY is "not by you", and a class header is not a control at
        // all. The last arm is the payload-carrying fallback AGENTS rule 6 asks of every
        // match on device vocabulary: if `is_writable` grows a fourth term, this reports
        // an honest "by a term this suite does not name yet" instead of guessing one of
        // the three, and the day somebody reads that sentence in a transcript is the day
        // this arm gets its fourth variant.
        return Some(if desc.flags.has(KnownFlag::ReadOnly) {
            Disqualifier::ReadOnly
        } else if desc.flags.has(KnownFlag::Disabled) {
            Disqualifier::Disabled
        } else if desc.control_type == ControlType::ControlClass {
            Disqualifier::ControlClass
        } else {
            Disqualifier::NotWritable
        });
    }
    if !desc.control_type.is_scalar() {
        return Some(Disqualifier::NotScalar {
            control_type: desc.control_type,
        });
    }
    if desc.is_volatile() {
        return Some(Disqualifier::Volatile);
    }
    if desc.flags.has(KnownFlag::WriteOnly) {
        return Some(Disqualifier::WriteOnly);
    }
    match &desc.current {
        None => Some(Disqualifier::CurrentUnknown),
        Some(ControlValue::Int(v)) if !desc.range.contains(*v) => {
            Some(Disqualifier::CurrentOutsideRange {
                current: *v,
                min: desc.range.min,
                max: desc.range.max,
            })
        }
        Some(ControlValue::Int(v)) if !is_step_aligned(&desc.range, *v) => {
            Some(Disqualifier::CurrentOffStep {
                current: *v,
                min: desc.range.min,
                step: desc.range.effective_step(),
            })
        }
        Some(ControlValue::Int(_)) => None,
        Some(other) => Some(Disqualifier::CurrentNotAnInteger {
            current: other.clone(),
        }),
    }
}

/// Why this control cannot carry a brightness-class sweep, when it cannot.
///
/// [`why_not_perturbable`] plus the three terms a *sweep* adds to a *write*: an automation
/// partner may not own the control (D3's business, and this arm is not the one to take it
/// on), the type has to be the plain integer a uniform plan walks, and the declared range
/// has to hold more than one value.
///
/// The motor question is asked first and deliberately so: design §5 is a law about hardware
/// wear, so it is the term that must fire even if some later one would have refused anyway.
#[must_use]
pub fn why_not_sweepable(desc: &ControlDesc) -> Option<Disqualifier> {
    if is_motorized(&desc.slug) {
        return Some(Disqualifier::Motorized);
    }
    if let Some(why) = why_not_perturbable(desc) {
        return Some(why);
    }
    if desc.is_inactive() {
        return Some(Disqualifier::Inactive);
    }
    if desc.control_type != ControlType::Integer {
        return Some(Disqualifier::NotAnInteger {
            control_type: desc.control_type,
        });
    }
    if desc.range.max <= desc.range.min {
        return Some(Disqualifier::OneValueRange {
            min: desc.range.min,
            max: desc.range.max,
        });
    }
    None
}

/// The first control of [`BRIGHTNESS_CLASS`] this device will let a test sweep, or a named
/// reason there is none.
/// **Two questions and not one**, which is the whole of the repair. The names are looked
/// for first, and only a control the device actually has can be *disqualified*; a device
/// that has none of them declines with a different answer, carrying a different sentence,
/// distinguishable by [`Decline::is_a_fact_about`] rather than by a reader's guess.
///
/// The list is walked in preference order and the first control that is *usable* wins — not
/// the first one that is *named*, which is the neighbouring bug: a camera whose `brightness`
/// is read-only and whose `gamma` is fine would otherwise report that it has nothing to
/// sweep.
#[must_use]
pub fn brightness_class_target(controls: &[ControlDesc]) -> SweepTarget<'_> {
    let mut refused = Vec::new();
    for name in BRIGHTNESS_CLASS {
        let Some(desc) = controls.iter().find(|desc| desc.slug.as_str() == name) else {
            continue;
        };
        match why_not_sweepable(desc) {
            None => return SweepTarget::Found(desc),
            Some(why) => refused.push((desc.slug.clone(), why)),
        }
    }
    SweepTarget::Declined(if refused.is_empty() {
        Decline::NoneNamed {
            examined: controls.len(),
        }
    } else {
        // Every named control, not only the first: a transcript that reported the
        // read-only `brightness` and stayed quiet about the INACTIVE `gain` would send a
        // reader looking for a fault this run had already diagnosed.
        Decline::NoneUsable(refused)
    })
}

// ------------------------------------------- how many samples a control's own range plans

/// How few samples make an arm's assertions worth making, and the arm's own argument for
/// the number.
///
/// The **number is not shared and the mechanism is**, which is the whole shape of this
/// type. What a floor is *for* differs per arm — the R3-over-UDS sweep needs enough events
/// to tell a live progress stream from a report delivered at the end, and the in-process
/// calibration arm needs enough samples for a metric ordering to be a ranking rather than a
/// comparison of two endpoints — so the count and its argument travel together and the
/// `SKIP` line says both. A floor with no argument beside it is a magic number, and the one
/// thing this repository's transcripts are for is telling a reader what a run did not claim.
///
/// Neither of these is [`schema::limits`]'s business: nothing about the *product* changes at
/// two samples, and a two-sample sweep is a perfectly good sweep for an operator. Each arm
/// holds its own `const` under the schema's ceiling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SampleFloor {
    /// The fewest samples the arm's claims can be made over.
    pub count: u32,
    /// Why *that* number, in the arm's own words, as the tail of "…the N this arm needs
    /// to ___". Reaches the transcript, so it is written to be read there.
    pub because: &'static str,
}

/// Why a control's own declared range cannot carry the sweep an arm wanted.
///
/// Both variants are facts about a **range a device declared**, which is what the caller's
/// `SKIP` line says and why they share a type: neither is a defect, and neither is a fact
/// about the socket, the backend, or the code under test. They are kept apart because they
/// come from different authorities — the first is the product's planner refusing, the second
/// is this suite's own floor — and note **N72**'s F5 is the entry about a decline that
/// answered one sentence for several findings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShortSweep {
    /// [`engine::sweep::plan`] refused the range outright.
    Refused {
        /// The control, so the sentence names it without the caller re-formatting.
        control: ControlSlug,
        /// Declared minimum.
        min: i64,
        /// Declared maximum.
        max: i64,
        /// Declared step, as declared — not the effective one, because a device that
        /// declares a step of 0 \[PF:4\] should have that in the transcript.
        step: i64,
        /// The planner's own typed refusal, rendered.
        refusal: String,
    },
    /// The plan is legal, and smaller than the arm's floor.
    UnderFloor {
        /// The control.
        control: ControlSlug,
        /// Declared minimum.
        min: i64,
        /// Declared maximum.
        max: i64,
        /// The effective step the count was computed against.
        step: i64,
        /// The stride the arm asked for.
        stride: i64,
        /// What the planner said that costs.
        samples: u32,
        /// The floor it fell under, with the arm's argument for it.
        floor: SampleFloor,
    },
}

impl fmt::Display for ShortSweep {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ShortSweep::Refused {
                control,
                min,
                max,
                step,
                refusal,
            } => write!(
                f,
                "the sweep planner refuses {control} on its own declared range \
                 {min}..={max} (step {step}): {refusal}"
            ),
            ShortSweep::UnderFloor {
                control,
                min,
                max,
                step,
                stride,
                samples,
                floor,
            } => write!(
                f,
                "{control} declares {min}..={max} with a step of {step}, which a stride of \
                 {stride} plans as {samples} sample(s) — fewer than the {} this arm needs \
                 to {}, so it declines before writing to the camera rather than after",
                floor.count, floor.because
            ),
        }
    }
}

/// What an arm will do with a control, decided from its descriptor and nothing else.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SweepChoice {
    /// Ask for this spec; the product's own planner says it costs this many samples.
    Planned {
        /// What goes to the executor, or on the wire.
        spec: SweepSpec,
        /// What the planner says that costs, before anything is written.
        samples: u32,
    },
    /// Decline, with the sentence that says why — and decline *here*, where nothing has
    /// been written yet.
    Declined(ShortSweep),
}

/// The sweep an arm would ask for over `desc`, priced by the **product's own planner**,
/// before anything is written.
///
/// A stride of a quarter of the control's *declared* range, which is five values where the
/// range divides and fewer where the control's own step does not let it: `brightness` is
/// `0..=255` on one attached camera and `0..=100` on another, so a stride written down in a
/// suite would be a number about somebody's desk rather than about a device.
///
/// The count comes from [`engine::sweep::plan`] rather than from arithmetic repeated in a
/// test. That is the same pure core the executor runs a moment later — design §2.10's one
/// home per law — so this is not a second planner with a second opinion; it is the planner,
/// asked early. **That it is askable early is the entire repair** (note **N72**):
/// `SweepPlan::total()` is a fact about a `ControlDesc`, and a `ControlDesc` is something an
/// arm holds before it has opened a session, let alone moved a sensor.
///
/// Two ways out, and both are declines rather than failures (AGENTS rule 7):
///
/// - **Under `floor`.** A `brightness` declaring `0..=64` with a step of 64 plans two
///   values, and so does one declaring `0..=1`; both are ordinary devices an arm that needs
///   three samples has nothing to say about. What used to happen in both rungs is that the
///   arm swept such a camera and *then* panicked — turning a device *shape* into a red run,
///   which is the lesson `crates/backends/v4l2/tests/hardware.rs`'s enumeration arm carries
///   in writing ("an `assert!(matched > 0)` … turned 'different hardware' into a red run").
///   Worse, it panicked between the sweep and the restore, so it left the camera at the last
///   value it wrote — E13's "a hardware arm that fails between its sweep and its restore
///   leaves the camera moved".
/// - **The planner refused.** A typed refusal is the device saying its range is not one this
///   tool sweeps (`empty_range`, `not_sweepable`), which is a fact to report rather than an
///   error to raise in a test. It cannot fire for a control that cleared
///   [`brightness_class_target`] today; it is handled because the two predicates are
///   different rules, and a `?` that becomes a panic the day they disagree is the shape this
///   whole finding is about.
///
/// **Shared between the two calibration rungs and written once.** It began as a private
/// helper in `crates/client/tests/hardware.rs` (note **N72**), and the sibling rung needed
/// the identical arithmetic against the identical planner for the identical reason. Moving
/// one copy and leaving the other is what F5 cost — the same predicate written twice, and
/// only one of the copies repaired.
#[must_use]
pub fn sweep_for(desc: &ControlDesc, floor: SampleFloor) -> SweepChoice {
    let span = desc.range.max.saturating_sub(desc.range.min);
    let stride = (span / 4).max(desc.range.effective_step());
    let spec = SweepSpec::Uniform { step: stride };
    let planned = match engine::sweep::plan(desc, &spec, false) {
        Ok(planned) => planned,
        Err(refusal) => {
            return SweepChoice::Declined(ShortSweep::Refused {
                control: desc.slug.clone(),
                min: desc.range.min,
                max: desc.range.max,
                step: desc.range.step,
                refusal: refusal.to_string(),
            });
        }
    };
    let samples = planned.total();
    if samples < floor.count {
        return SweepChoice::Declined(ShortSweep::UnderFloor {
            control: desc.slug.clone(),
            min: desc.range.min,
            max: desc.range.max,
            step: desc.range.effective_step(),
            stride,
            samples,
            floor,
        });
    }
    SweepChoice::Planned { spec, samples }
}

// ------------------------------------- which controls a restore report will answer for

/// What a [`RestoreReport`] undertakes to have put back, and what it declines to speak for
/// — the population an AGENTS rule 8 arm may compare against the device, and the named,
/// counted sentence for the rest.
///
/// **Four hardware arms and one answer.** `crates/backends/v4l2/tests/hardware.rs` restores
/// a snapshot after a one-step perturbation, after a calibration session and across a
/// `uvcvideo` cycle, and `crates/client/tests/hardware.rs` restores one over the socket;
/// each then re-reads the device and asserts the values came back, which is what makes rule
/// 8 a checked claim rather than a sentence in a header. Three of them filtered that
/// comparison on the snapshot's own `was_volatile` flag and the fourth filtered it on the
/// report's outcomes, and the difference between those two is a **device finding**, not a
/// style: `VOLATILE` is not how a device says that the value of a control belongs to an
/// algorithm \[PF:24\]. The Logitech BRIO's `white_balance_temperature` is INACTIVE, is
/// **not** VOLATILE, and its own AWB moves it between a restore that reported itself
/// complete and the re-read that checks it — so the arms keyed on the flag were asserting a
/// number against a running algorithm, and went red for it on a schedule nobody controls
/// (PF:24's 2026-08-13 amendment: red in a lit room, green in a dark one, no code between
/// the two runs).
///
/// So the population comes off the **report**, which is the only party that knows.
/// [`RestoreOutcome::OwnedByAutomation`] is the engine having deferred the control, written
/// every automation control, re-read the device and found the partner holding it again —
/// INACTIVE when the snapshot was taken *and* INACTIVE now, which is exactly the narrow
/// predicate PF:24 argued for and which no test can derive for itself without keeping a
/// second copy of D4's two-pass rule. The wide predicate ("exempt everything INACTIVE")
/// would stop asserting restoration for a control a sweep legitimately moved and put back;
/// a **name** — `white_balance_temperature` — would be the repair AGENTS rule 7 forbids
/// outright, because "this control's value is the device's" is a fact the device states and
/// "this control failed today" is a fact about a run.
///
/// The type lives here and not in one of the four suites for the reason [`BRIGHTNESS_CLASS`]
/// does (note **N72**): the same two `#[ignore]`d binaries had already grown two copies of a
/// selection rule once, and a rule inside an ignored hardware binary is a rule nothing can
/// unit-test. `webcam-handler-v4l2` and `webcam-handler-client` both dev-depend on this
/// crate, so one home is reachable from both without a `#[path]` include and without the
/// one-user-per-item tax note **N49** puts on those.
///
/// [`RestoreOutcome::Unrestorable`] is deliberately in neither population: what a control
/// nobody could put back costs a run is [`RestoreReport::is_complete`]'s to decide, and
/// every one of the four arms asserts that separately. Naming it here as well would give
/// one verdict two homes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestorationClaim {
    /// The controls the report says it wrote, or found already holding the recorded value.
    claimed: BTreeSet<String>,
    /// The controls whose automation owns them again \[PF:24\], each with the partner the
    /// pair set named — or `None` where it named none, which is itself worth printing.
    owned: Vec<(ControlSlug, Option<ControlSlug>)>,
    /// How many outcomes the report carried, so a report that spoke about nothing at all
    /// can be told from one that spoke about everything and claimed none of it.
    outcomes: usize,
}

impl RestorationClaim {
    /// Whether an arm may assert that this control reads back at its recorded value.
    ///
    /// Takes a `&str` because the four call sites hold their "before" values in three
    /// different containers — a `BTreeMap<String, _>` read off the device, a [`Snapshot`]'s
    /// entries, and a snapshot that arrived over a socket — and the one thing all three
    /// share is the slug's text. Re-parsing each of them into a [`ControlSlug`] to ask a
    /// question about a name would be validating a string this workspace produced, which is
    /// why the claimed set is kept as text and the declined list is not: one is only ever
    /// *looked up*, the other is only ever printed and asserted.
    #[must_use]
    pub fn speaks_for(&self, slug: &str) -> bool {
        self.claimed.contains(slug)
    }

    /// The controls left to their automation, in slug order, with the partner that owns
    /// each one.
    ///
    /// Exposed so a suite can assert *which* controls were declined rather than only how
    /// many — the difference between a decline a reader can audit and a number.
    #[must_use]
    pub fn left_to_automation(&self) -> &[(ControlSlug, Option<ControlSlug>)] {
        &self.owned
    }

    /// Say what this restore was and was not checked on, and refuse a run that checked
    /// nothing.
    ///
    /// `compared` is what the caller actually asserted against the device, passed in rather
    /// than recomputed here: this type knows what the *report* offered, and only the arm
    /// knows how much of that its own "before" reading could be compared with. Two numbers
    /// that should agree, printed together, is how they stop agreeing loudly instead of
    /// quietly.
    ///
    /// **The empty-claim case fails rather than skips, and that is the choice.** A restore
    /// whose every outcome is [`RestoreOutcome::OwnedByAutomation`] passes
    /// [`RestoreReport::is_complete`] — that is the whole point of note N9 — so an arm that
    /// excluded all of them would print a green line having asserted nothing at all about
    /// rule 8. That is "skip == pass, in a costume" (docs/8 Part C) reached by arithmetic
    /// rather than by a `continue`, and the only defence against arithmetic is a count that
    /// can go red. It is not a reachable state on any device measured so far — a sweep's
    /// target is refused while it is INACTIVE ([`why_not_sweepable`]), and a perturbation's
    /// target has to have moved — which is exactly why it needs an assertion rather than a
    /// comment: an unreachable branch nobody checks is how the next device's finding arrives
    /// as a green run.
    ///
    /// A report with **no outcomes at all** is the other thing entirely and is not a
    /// failure: a camera with nothing writable on it has nothing to restore, and turning
    /// that into a red arm would be converting a fact about a device into a fact about the
    /// code (AGENTS rule 7). It gets its own counted line, because a suite that says nothing
    /// about a camera is a suite a reader cannot tell from one that passed.
    ///
    /// # Panics
    ///
    /// When the report carried outcomes and `compared` is zero.
    pub fn account_for(&self, camera: &str, compared: usize) {
        if self.outcomes == 0 {
            println!(
                "SKIP (partial): {camera} — the restore reported no outcome at all, so this \
                 arm compared nothing against the device; a camera with no writable control \
                 has nothing to put back"
            );
            return;
        }
        if !self.owned.is_empty() {
            println!(
                "SKIP (partial): {camera} — {} of {} restored control(s) are left to their \
                 automation [PF:24], so this arm compared {compared} against the device and \
                 claims nothing about these: {}",
                self.owned.len(),
                self.outcomes,
                self,
            );
        }
        assert!(
            compared > 0,
            "{camera}: the restore reported {} outcome(s) and this arm checked none of them \
             against the device — {} left to their automation, {} claimed. A restoration \
             claim nobody could check is AGENTS rule 8 reading as a pass",
            self.outcomes,
            self.owned.len(),
            self.claimed.len()
        );
    }
}

impl fmt::Display for RestorationClaim {
    /// The controls left to their automation, each followed by the partner that owns it.
    ///
    /// The partner is named because it is what makes the exclusion auditable: a reader who
    /// sees `white_balance_temperature (white_balance_automatic)` can go and switch that
    /// automation off and watch the control become this arm's business again, which is
    /// PF:24's own inverse arm. An unnamed one says so rather than reading as an omission —
    /// the pair set not knowing an owner is a fact about D3's discovery on this device.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let named: Vec<String> = self
            .owned
            .iter()
            .map(|(control, automation)| match automation {
                Some(automation) => format!("{control} ({automation})"),
                None => format!("{control} (no partner in this device's pair set)"),
            })
            .collect();
        f.write_str(&named.join(", "))
    }
}

/// Read a restore report as the two populations an arm may and may not assert over.
///
/// A fold over [`RestoreReport::outcomes`] and nothing else: the device is not consulted,
/// no slug is spelled out here, and the exhaustive match is the point — a fifth outcome
/// added to the schema stops this compiling rather than landing silently in the population
/// that happens to be the default.
#[must_use]
pub fn restoration_claim(report: &RestoreReport) -> RestorationClaim {
    let mut claimed = BTreeSet::new();
    let mut owned = Vec::new();
    for outcome in &report.outcomes {
        match outcome {
            RestoreOutcome::Restored { applied } => {
                claimed.insert(applied.slug.to_string());
            }
            RestoreOutcome::AlreadyCorrect { control } => {
                claimed.insert(control.to_string());
            }
            RestoreOutcome::OwnedByAutomation {
                control,
                automation,
            } => owned.push((control.clone(), automation.clone())),
            // Left to `RestoreReport::is_complete`, which is the one place that decides
            // what a control nobody could put back costs the run.
            RestoreOutcome::Unrestorable { .. } => {}
        }
    }
    // Slug order rather than attempt order, so two runs of the same suite on the same desk
    // print the same sentence and a reader can diff them. The report's own order is a
    // promise about the *writes* and belongs to the report.
    owned.sort_by(|(a, _), (b, _)| a.as_str().cmp(b.as_str()));
    RestorationClaim {
        claimed,
        owned,
        outcomes: report.outcomes.len(),
    }
}

/// Whether the PF:6 clamp probe may use this control: an integer with room above its
/// maximum, and no motor on the other end of it.
fn is_clamp_probe_candidate(desc: &ControlDesc) -> bool {
    matches!(
        desc.control_type,
        ControlType::Integer | ControlType::Integer64
    ) && desc.range.max > desc.range.min
        && desc.range.max.checked_add(CLAMP_PROBE_OVERSHOOT).is_some()
        && !is_motorized(&desc.slug)
}

/// A value one step away from where the control is now — the smallest perturbation that
/// is still a change (design §5 again: minimal travel).
///
/// `None` for a control whose vocabulary this suite cannot move by one: a bitmask's bits
/// mean things it does not know, and guessing one is the "represent, don't invent" line.
#[must_use]
pub fn perturbation(desc: &ControlDesc) -> Option<ControlValue> {
    let current = desc.current.as_ref()?.as_int()?;
    match desc.control_type {
        ControlType::Boolean => Some(ControlValue::Int(i64::from(current == 0))),
        ControlType::Menu | ControlType::IntegerMenu => desc
            .menu
            .keys()
            .map(|index| i64::from(*index))
            .find(|index| *index != current)
            .map(ControlValue::Int),
        ControlType::Integer | ControlType::Integer64 => {
            let step = desc.range.effective_step();
            let up = current.checked_add(step).filter(|v| *v <= desc.range.max);
            let down = current.checked_sub(step).filter(|v| *v >= desc.range.min);
            up.or(down).map(ControlValue::Int)
        }
        // A bitmask's bits mean things this suite does not know; guessing one is exactly
        // the "represent, don't invent" line (D2).
        _ => None,
    }
}

fn read_controls(
    camera: &mut dyn Camera,
    id: &str,
    log: &mut ArmLog<'_>,
) -> Option<Vec<ControlDesc>> {
    match camera.controls() {
        Ok(controls) => Some(controls),
        Err(error) => {
            log.fail(format!("{id}: controls() failed: {error}"));
            None
        }
    }
}

/// The enumerate-then-open preamble every camera-driven arm shares, together with the
/// bookkeeping that decides whether the arm ran or skipped.
struct CameraVisit<'a, 'l> {
    backend: &'a dyn CameraBackend,
    log: &'a mut ArmLog<'l>,
    /// How many cameras the arm actually exercised. Zero means the arm skipped.
    examined: usize,
    /// Why cameras were passed over, in the order they were.
    notes: Vec<String>,
}

impl<'a, 'l> CameraVisit<'a, 'l> {
    fn new(backend: &'a dyn CameraBackend, log: &'a mut ArmLog<'l>) -> Self {
        CameraVisit {
            backend,
            log,
            examined: 0,
            notes: Vec::new(),
        }
    }

    fn cameras(&mut self) -> Option<Vec<CameraInfo>> {
        match self.backend.enumerate() {
            Ok(cameras) if cameras.is_empty() => {
                self.notes
                    .push("the backend enumerated no cameras".to_owned());
                None
            }
            Ok(cameras) => Some(cameras),
            Err(error) => {
                self.notes.push(format!("enumerate() failed: {error}"));
                None
            }
        }
    }

    fn open(&mut self, info: &CameraInfo) -> Option<Box<dyn Camera>> {
        match self.backend.open(&info.id) {
            Ok(camera) => Some(camera),
            // E3 again: a busy or forbidden device has not said anything about what it
            // can do.
            Err(error @ (Error::Busy { .. } | Error::PermissionDenied { .. })) => {
                self.notes
                    .push(format!("{} could not be opened: {error}", info.id));
                None
            }
            Err(error) => {
                self.log
                    .fail(format!("{}: open() failed: {error}", info.id));
                None
            }
        }
    }

    fn note(&mut self, reason: impl Into<String>) {
        self.notes.push(reason.into());
    }

    /// The outcome when the preamble itself could not proceed.
    fn into_skip(self) -> ArmOutcome {
        ArmOutcome::skipped(join_notes(
            &self.notes,
            "the backend offered no camera to test",
        ))
    }

    /// `Ran` when at least one camera was exercised, else a skip naming every reason the
    /// others were passed over.
    ///
    /// **An arm that ran still owes an account of what it did not ask.** The notes were the
    /// skip reason and nothing else until 2026-08-16, so a backend with two cameras — one
    /// exercised, one passed over — reported `ran` and dropped the second camera's reason on
    /// the floor (note **N138**). One camera answering is enough to say the arm ran; it is
    /// not enough to say the arm covered the backend, and AGENTS rule 3's "a named, counted
    /// skip — never silence" is about the second claim. So on the `Ran` path the same notes
    /// go to [`ArmLog::note`], where [`run`] carries them into
    /// [`BatteryReport::notes`]; on the skip path they are already the reason and are not
    /// said twice.
    fn finish(mut self, fallback: &str) -> ArmOutcome {
        if self.examined > 0 {
            for note in std::mem::take(&mut self.notes) {
                self.log.note(note);
            }
            ArmOutcome::Ran
        } else {
            ArmOutcome::skipped(join_notes(&self.notes, fallback))
        }
    }
}

fn join_notes(notes: &[String], fallback: &str) -> String {
    if notes.is_empty() {
        fallback.to_owned()
    } else {
        notes.join("; ")
    }
}

#[cfg(test)]
mod tests {
    use schema::backend::HotplugWatch;
    use schema::camera::CameraId;
    use schema::control::{Applied, ControlFlags};
    use schema::error::Result;

    use super::*;

    /// A backend that exists to be *wrong* in one dimension at a time.
    ///
    /// Rubric rule 2: for every check, construct the input that must trip it. The skip
    /// accounting is the battery's own load-bearing logic, so it gets the same treatment
    /// as any validator — three misbehaviours, each with its own test.
    ///
    /// `open` reports `Busy` rather than a defect on purpose: availability is not
    /// capability (E3), so the camera-driven arms skip instead of failing, and each test
    /// can assert an exact failure count.
    #[derive(Debug)]
    struct StubBackend {
        cameras: Vec<CameraInfo>,
    }

    impl StubBackend {
        /// A backend with nothing attached — every camera-driven arm legitimately skips.
        fn empty() -> StubBackend {
            StubBackend {
                cameras: Vec::new(),
            }
        }

        /// A backend with one well-formed camera, so the enumeration arm runs.
        fn with_one_camera() -> StubBackend {
            let mut info = crate::fixtures::synthetic_basic().invariant.info;
            info.backend = BackendKind::Fake;
            StubBackend {
                cameras: vec![info],
            }
        }
    }

    impl CameraBackend for StubBackend {
        fn kind(&self) -> BackendKind {
            BackendKind::Fake
        }

        fn enumerate(&self) -> Result<Vec<CameraInfo>> {
            Ok(self.cameras.clone())
        }

        fn open(&self, _id: &CameraId) -> Result<Box<dyn Camera>> {
            Err(Error::Busy {
                path: "/dev/video0".into(),
                holders: Vec::new(),
            })
        }

        fn watch(&self) -> Result<Box<dyn HotplugWatch>> {
            Ok(Box::new(SilentWatch))
        }
    }

    #[derive(Debug)]
    struct SilentWatch;

    impl HotplugWatch for SilentWatch {
        fn next_event(&mut self, _deadline: Instant) -> Result<Option<HotplugEvent>> {
            Ok(None)
        }
    }

    /// Every arm except the hotplug one, which the stub can always run.
    fn skips_for_a_deviceless_backend() -> BTreeMap<BatteryArm, String> {
        BatteryArm::ALL
            .iter()
            .copied()
            .filter(|arm| *arm != BatteryArm::HotplugWatch)
            .map(|arm| (arm, format!("the stub backend cannot run {arm}")))
            .collect()
    }

    #[test]
    fn honest_skip_accounting_is_green() {
        // The direction that must also hold: a backend that declares exactly what it
        // cannot do passes, or the other three tests would be proving nothing.
        let report = run(&StubBackend::empty(), &skips_for_a_deviceless_backend());
        assert!(report.is_green(), "{report}");
        assert_eq!(
            report.outcomes.len(),
            BatteryArm::ALL.len(),
            "the report must be total over the arms"
        );
        assert_eq!(
            report.outcome(BatteryArm::HotplugWatch),
            Some(&ArmOutcome::Ran)
        );
    }

    #[test]
    fn an_arm_that_ran_while_it_was_declared_skipped_is_a_failure() {
        // The stale declaration. Without this direction, a backend could silence the
        // whole battery by declaring every arm skipped and still report green.
        let mut skips = skips_for_a_deviceless_backend();
        skips.insert(
            BatteryArm::Enumeration,
            "this camera is imaginary".to_owned(),
        );
        let report = run(&StubBackend::with_one_camera(), &skips);

        assert!(!report.is_green(), "{report}");
        assert_eq!(
            report.outcome(BatteryArm::Enumeration),
            Some(&ArmOutcome::Ran)
        );
        let complaints = report.failures_for(BatteryArm::Enumeration);
        assert_eq!(complaints.len(), 1, "{report}");
        assert!(
            complaints
                .iter()
                .all(|f| f.contains("declared it skipped") && f.contains("stale")),
            "{report}"
        );
        assert_eq!(report.failures.len(), 1, "{report}");
    }

    #[test]
    fn an_arm_that_did_not_run_without_a_declaration_is_a_failure() {
        // "Skip == pass, in any costume" (docs/8 Part C). An undeclared skip is the
        // costume.
        let report = run(&StubBackend::empty(), &BTreeMap::new());

        assert!(!report.is_green(), "{report}");
        for &arm in BatteryArm::ALL {
            if arm == BatteryArm::HotplugWatch {
                continue;
            }
            let complaints = report.failures_for(arm);
            assert_eq!(complaints.len(), 1, "{arm} went unreported: {report}");
            assert!(
                complaints
                    .iter()
                    .all(|f| f.contains("no skip was declared")),
                "{report}"
            );
        }
        assert!(report.failures_for(BatteryArm::HotplugWatch).is_empty());
    }

    #[test]
    fn a_declared_skip_with_an_empty_reason_is_a_failure() {
        // A reason nobody wrote is a reason nobody can audit, and the counted-skip
        // discipline (docs/8 rule 3) is only worth anything if the count comes with
        // words.
        let mut skips = skips_for_a_deviceless_backend();
        skips.insert(BatteryArm::FaultMenu, "   ".to_owned());
        let report = run(&StubBackend::empty(), &skips);

        assert!(!report.is_green(), "{report}");
        let complaints = report.failures_for(BatteryArm::FaultMenu);
        assert_eq!(complaints.len(), 1, "{report}");
        assert!(
            complaints.iter().all(|f| f.contains("empty reason")),
            "{report}"
        );
        // The arm still counts as skipped — the declaration exists, it is just useless.
        assert!(matches!(
            report.outcome(BatteryArm::FaultMenu),
            Some(ArmOutcome::Skipped { .. })
        ));
    }

    #[test]
    fn the_fault_menu_arm_names_why_it_cannot_run_over_the_trait() {
        let report = run(&StubBackend::empty(), &skips_for_a_deviceless_backend());
        let Some(ArmOutcome::Skipped { reason }) = report.outcome(BatteryArm::FaultMenu) else {
            panic!("the fault-menu arm must skip: {report}");
        };
        assert!(
            reason.contains("fault-scripting"),
            "the reason must say what is missing: {reason}"
        );
    }

    // ------------------------------------------- the restore that cannot be skipped (N137)

    /// A camera whose control read fails once the arm has perturbed it.
    ///
    /// The failure is real and is the one AGENTS rule 8 is written for: a camera unplugged
    /// between the write and the read-back, a driver that stopped answering `QUERY_EXT_CTRL`
    /// mid-arm. It is scripted here rather than measured because the *arm's* behaviour is
    /// the subject, and a device that fails on cue is the only way to reach the exit that
    /// used to skip the restore.
    #[derive(Debug)]
    struct ReadFailsAfterTheFirstWrite {
        controls: Vec<ControlDesc>,
        writes: usize,
        reads: usize,
    }

    impl ReadFailsAfterTheFirstWrite {
        fn new() -> ReadFailsAfterTheFirstWrite {
            ReadFailsAfterTheFirstWrite {
                controls: vec![sweepable("brightness")],
                writes: 0,
                reads: 0,
            }
        }
    }

    impl Camera for ReadFailsAfterTheFirstWrite {
        fn info(&self) -> &CameraInfo {
            unreachable!("this arm never asks a camera for its own info")
        }

        fn formats(&self) -> Result<Vec<FormatInfo>> {
            Ok(Vec::new())
        }

        fn controls(&self) -> Result<Vec<ControlDesc>> {
            if self.writes > 0 {
                return Err(Error::DeviceGone {
                    path: "/dev/video0".into(),
                });
            }
            Ok(self.controls.clone())
        }

        fn get(&mut self, _id: ControlId) -> Result<ControlValue> {
            unreachable!("this arm reads values through controls()")
        }

        fn set(&mut self, id: ControlId, value: ControlValue) -> Result<Applied> {
            self.writes += 1;
            let desc = self
                .controls
                .iter_mut()
                .find(|desc| desc.id == id)
                .ok_or_else(|| Error::ControlUnknown {
                    requested: id.to_string(),
                    did_you_mean: Vec::new(),
                })?;
            desc.current = Some(value.clone());
            Ok(Applied {
                control: id,
                slug: desc.slug.clone(),
                requested: value.clone(),
                applied: value,
                warnings: Vec::new(),
            })
        }

        fn start_stream(&mut self, _request: &StreamRequest) -> Result<NegotiatedStream> {
            unreachable!("the snapshot arm does not stream")
        }

        fn streaming(&self) -> Option<NegotiatedStream> {
            None
        }

        fn next_frame(&mut self, _deadline: Instant) -> Result<Frame> {
            unreachable!("the snapshot arm does not stream")
        }

        fn stop_stream(&mut self) -> Result<()> {
            Ok(())
        }
    }

    /// A backend handing out one such camera, and keeping a view of what it now holds.
    #[derive(Debug)]
    struct OneFragileCamera {
        camera: std::sync::Arc<std::sync::Mutex<ReadFailsAfterTheFirstWrite>>,
    }

    impl OneFragileCamera {
        fn new() -> OneFragileCamera {
            OneFragileCamera {
                camera: std::sync::Arc::new(std::sync::Mutex::new(
                    ReadFailsAfterTheFirstWrite::new(),
                )),
            }
        }

        fn brightness(&self) -> Option<ControlValue> {
            self.camera
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .controls
                .first()
                .and_then(|desc| desc.current.clone())
        }
    }

    /// The handle `open` returns: every call forwarded to the one shared camera, so the
    /// test can see what the arm left behind after the arm has dropped its handle.
    #[derive(Debug)]
    struct SharedHandle(std::sync::Arc<std::sync::Mutex<ReadFailsAfterTheFirstWrite>>);

    impl SharedHandle {
        fn locked(&self) -> std::sync::MutexGuard<'_, ReadFailsAfterTheFirstWrite> {
            self.0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
        }
    }

    impl Camera for SharedHandle {
        fn info(&self) -> &CameraInfo {
            unreachable!("this arm never asks a camera for its own info")
        }

        fn formats(&self) -> Result<Vec<FormatInfo>> {
            self.locked().formats()
        }

        fn controls(&self) -> Result<Vec<ControlDesc>> {
            let mut camera = self.locked();
            camera.reads += 1;
            camera.controls()
        }

        fn get(&mut self, id: ControlId) -> Result<ControlValue> {
            self.locked().get(id)
        }

        fn set(&mut self, id: ControlId, value: ControlValue) -> Result<Applied> {
            self.locked().set(id, value)
        }

        fn start_stream(&mut self, request: &StreamRequest) -> Result<NegotiatedStream> {
            self.locked().start_stream(request)
        }

        fn streaming(&self) -> Option<NegotiatedStream> {
            None
        }

        fn next_frame(&mut self, deadline: Instant) -> Result<Frame> {
            self.locked().next_frame(deadline)
        }

        fn stop_stream(&mut self) -> Result<()> {
            Ok(())
        }
    }

    impl CameraBackend for OneFragileCamera {
        fn kind(&self) -> BackendKind {
            BackendKind::Fake
        }

        fn enumerate(&self) -> Result<Vec<CameraInfo>> {
            let mut info = crate::fixtures::synthetic_basic().invariant.info;
            info.backend = BackendKind::Fake;
            Ok(vec![info])
        }

        fn open(&self, _id: &CameraId) -> Result<Box<dyn Camera>> {
            Ok(Box::new(SharedHandle(std::sync::Arc::clone(&self.camera))))
        }

        fn watch(&self) -> Result<Box<dyn HotplugWatch>> {
            Ok(Box::new(SilentWatch))
        }
    }

    #[test]
    fn a_read_that_fails_after_the_perturbation_still_leaves_the_camera_where_it_was_found() {
        // AGENTS rule 8, at the one arm that deliberately moves a camera: "snapshot before,
        // restore after … tests assert restoration". The arm's own early exit sat *between*
        // the two until 2026-08-16 (note **N137**), so a read that failed after the writes
        // had landed returned with the perturbation still on the device — and §2.11 step 4
        // sends the author of every new backend through this suite with their own camera in
        // front of it.
        let backend = OneFragileCamera::new();
        let before = backend
            .brightness()
            .expect("the fixture control has a value");

        let mut failures = Vec::new();
        let mut notes = Vec::new();
        let outcome = {
            let mut log = ArmLog {
                arm: BatteryArm::SnapshotRestoreInverse,
                failures: &mut failures,
                notes: &mut notes,
            };
            arm_snapshot_restore_inverse(&backend, &mut log)
        };

        // The read failure is still reported — the restore is a repair, not a cover-up.
        assert_eq!(outcome, ArmOutcome::Ran, "{failures:?}");
        assert!(
            failures.iter().any(|f| f.contains("controls() failed")),
            "{failures:?}"
        );
        // And the camera is where it was found.
        assert_eq!(
            backend.brightness(),
            Some(before),
            "the arm returned between its perturbation and its restore: {failures:?}"
        );
    }

    // --------------------------------- an arm that ran still says what it did not ask (N138)

    /// A camera that resolves every request through the shared resolver, over the format
    /// list it is given.
    ///
    /// The resolver is the subject of the arm this exercises, so the double calls it rather
    /// than imitating it: a stand-in with its own opinion about what `640x480` resolves to
    /// would be the E5 divergence the whole `ExplicitRequest` arm exists to catch, built
    /// into the test that checks the arm.
    #[derive(Debug)]
    struct ResolvingCamera {
        formats: Vec<FormatInfo>,
        streamed: Option<(u32, u32)>,
        /// What `formats()` answers instead of the list, when a test needs the enumeration
        /// itself to be unavailable rather than empty.
        unavailable: Option<Error>,
    }

    impl Camera for ResolvingCamera {
        fn info(&self) -> &CameraInfo {
            unreachable!("this arm never asks a camera for its own info")
        }

        fn formats(&self) -> Result<Vec<FormatInfo>> {
            match &self.unavailable {
                Some(error) => Err(error.clone()),
                None => Ok(self.formats.clone()),
            }
        }

        fn controls(&self) -> Result<Vec<ControlDesc>> {
            Ok(Vec::new())
        }

        fn get(&mut self, _id: ControlId) -> Result<ControlValue> {
            unreachable!("this camera has no controls")
        }

        fn set(&mut self, _id: ControlId, _value: ControlValue) -> Result<Applied> {
            unreachable!("this camera has no controls")
        }

        fn start_stream(&mut self, request: &StreamRequest) -> Result<NegotiatedStream> {
            let chosen = request.choose(&self.formats)?;
            let interval = schema::camera::FrameInterval::Discrete {
                numerator: 1,
                denominator: 30,
            };
            self.streamed = Some((chosen.width, chosen.height));
            Ok(NegotiatedStream {
                pixel_format: chosen.pixel_format,
                width: chosen.width,
                height: chosen.height,
                bytes_per_line: chosen.width * 2,
                size_image: chosen.width * chosen.height * 2,
                interval,
                adjustments: NegotiatedStream::diff(
                    request,
                    chosen.pixel_format,
                    chosen.width,
                    chosen.height,
                    interval,
                ),
            })
        }

        fn streaming(&self) -> Option<NegotiatedStream> {
            None
        }

        fn next_frame(&mut self, _deadline: Instant) -> Result<Frame> {
            // A frame that agrees with its own header, because `arm_stream_lifecycle` runs
            // against this backend too and a double that streams garbage would fail an arm
            // this test is not about.
            let (width, height) = self.streamed.ok_or_else(|| Error::DeviceGone {
                path: "/dev/video0".into(),
            })?;
            let stride = width * 2;
            Ok(Frame {
                bytes: vec![0x80; (stride * height) as usize],
                pixel_format: PixelFormat::YUYV,
                width,
                height,
                bytes_per_line: stride,
                sequence: 0,
                timestamp_us: 0,
            })
        }

        fn stop_stream(&mut self) -> Result<()> {
            self.streamed = None;
            Ok(())
        }
    }

    /// Two cameras: one this arm can ask everything, and one it cannot.
    ///
    /// **The half-exercised backend.** `arm_explicit_request` puts two claims to each camera
    /// and the *arm* reports one outcome for the backend, so a camera that can answer only
    /// one of them is the shape that decides whether "not asked" survives the arm boundary
    /// (note **N138**). Two second cameras are needed because there are two ways to be
    /// unaskable and the arm answered them differently:
    ///
    /// - `stepwise` can deliver a frame of *any* size down to one pixel, so there is no size
    ///   for it to refuse and the size claim is a named skip.
    /// - `busy` is held by another process, so its enumeration says nothing about what it
    ///   can do — availability is not capability (AGENTS rule 7), and this arm failed on it
    ///   until 2026-08-16.
    #[derive(Debug)]
    struct OneCameraDeliversAnySize {
        second: &'static str,
    }

    impl OneCameraDeliversAnySize {
        fn info(slug: &str) -> CameraInfo {
            let mut info = crate::fixtures::synthetic_basic().invariant.info;
            info.backend = BackendKind::Fake;
            info.id = CameraId::parse(slug).expect("a literal slug");
            info
        }
    }

    impl CameraBackend for OneCameraDeliversAnySize {
        fn kind(&self) -> BackendKind {
            BackendKind::Fake
        }

        fn enumerate(&self) -> Result<Vec<CameraInfo>> {
            Ok(vec![Self::info("discrete"), Self::info(self.second)])
        }

        fn open(&self, id: &CameraId) -> Result<Box<dyn Camera>> {
            let size = if id.as_str().contains("stepwise") {
                schema::camera::FrameSize::Stepwise {
                    min_width: 1,
                    max_width: 1_920,
                    step_width: 1,
                    min_height: 1,
                    max_height: 1_080,
                    step_height: 1,
                }
            } else {
                schema::camera::FrameSize::Discrete {
                    width: 640,
                    height: 480,
                }
            };
            Ok(Box::new(ResolvingCamera {
                formats: vec![FormatInfo {
                    pixel_format: PixelFormat::YUYV,
                    description: "YUYV 4:2:2".to_owned(),
                    flags: 0,
                    sizes: vec![schema::camera::FrameSizeInfo {
                        size,
                        intervals: Vec::new(),
                    }],
                }],
                streamed: None,
                unavailable: (id.as_str().contains("busy")).then(|| Error::Busy {
                    path: "/dev/video9".into(),
                    holders: Vec::new(),
                }),
            }))
        }

        fn watch(&self) -> Result<Box<dyn HotplugWatch>> {
            Ok(Box::new(SilentWatch))
        }
    }

    #[test]
    fn a_claim_an_arm_could_not_put_to_a_camera_is_named_and_counted_even_when_the_arm_ran() {
        // **AGENTS rule 3 at the arm boundary**: "every auto-skipping rung reports a named,
        // counted skip — never silence". `Claim` exists so that "not asked" and "passed" do
        // not collapse, and until 2026-08-16 they collapsed one line after it was decided —
        // a `Claim::NotAsked` became a `CameraVisit` note, and notes were rendered only when
        // the arm ended `Skipped`. So this backend, whose second camera can deliver any size
        // and therefore cannot be asked for one it cannot deliver, reported `ran` and said
        // nothing at all about the half that never happened (note **N138**).
        let backend = OneCameraDeliversAnySize { second: "stepwise" };
        let declared = BTreeMap::new();
        let report = run(&backend, &declared);

        // The arm ran — one camera answered both claims — and it found nothing wrong,
        // because an unaskable camera is availability rather than a conformance verdict (E3).
        assert_eq!(
            report.outcome(BatteryArm::ExplicitRequest),
            Some(&ArmOutcome::Ran),
            "{report}"
        );
        assert!(
            report.failures_for(BatteryArm::ExplicitRequest).is_empty(),
            "{report}"
        );

        // And the claim nobody could put is named, counted, and attributed to its camera.
        let unasked = report.notes_for(BatteryArm::ExplicitRequest);
        assert_eq!(unasked.len(), 1, "{report}");
        assert!(
            unasked[0].contains("stepwise") && unasked[0].contains("1x1"),
            "{report}"
        );
        // The rendered report says so too, so a person reading a green run learns it — which
        // is the whole of what "never silence" asks for.
        assert!(
            report.to_string().contains("1 unasked claim(s):"),
            "{report}"
        );
    }

    #[test]
    fn a_camera_another_process_holds_is_a_named_skip_rather_than_a_conformance_failure() {
        // **AGENTS rule 7 / doctrine E3**, at the one call this arm made an exception of.
        // The arm was careful about `Busy` and `PermissionDenied` from `start_stream` and
        // then failed unconditionally on any `formats()` error — so a camera grabbed by
        // another process between `open` and `formats` turned a whole battery run red for a
        // fact about who holds the device rather than about what it can do (note **N138**).
        // §2.11 step 4 sends the author of every new backend through this suite with their
        // own camera, and a webcam somebody's video call has claimed is the ordinary case.
        let backend = OneCameraDeliversAnySize { second: "busy" };
        let declared = BTreeMap::new();
        let report = run(&backend, &declared);

        assert_eq!(
            report.outcome(BatteryArm::ExplicitRequest),
            Some(&ArmOutcome::Ran),
            "{report}"
        );
        assert!(
            report.failures_for(BatteryArm::ExplicitRequest).is_empty(),
            "a busy camera was reported as a backend that fails D5: {report}"
        );
        let unasked = report.notes_for(BatteryArm::ExplicitRequest);
        assert_eq!(unasked.len(), 1, "{report}");
        assert!(
            unasked[0].contains("busy") && unasked[0].contains("could not be asked"),
            "{report}"
        );
    }

    #[test]
    fn arm_names_are_distinct_and_survive_the_report_prefix() {
        // `failures_for` splits on the arm prefix, so two arms sharing a name would make
        // one arm's findings invisible in the other's.
        let mut seen = BTreeSet::new();
        for &arm in BatteryArm::ALL {
            assert!(seen.insert(arm.as_str()), "{arm} duplicates a name");
        }
    }

    // ------------------------------------------------- what a hardware arm may sweep (N72)
    //
    // These are the tests the finding asks for and the reason the predicate moved here at
    // all. The M7 mutant note **E13** records exercised the *name-absent* arm — it replaced
    // the three control names with names no camera has, so every camera declined and the
    // suite passed in a fifth of a second — and that is the only arm a hardware run on this
    // desk has ever reached, because the three attached cameras either have a usable
    // `brightness` or have no brightness-class control at all. Every *state* arm below is a
    // shape no fixture on this desk can produce, which is exactly the population note
    // **N70** describes: a test that asserts the assumption that produced the code.
    //
    // A run at real hardware could not have found this and cannot now prove it fixed. A
    // table of `ControlDesc` values can do both, which is why these are here rather than in
    // a transcript.

    /// A control this suite would sweep, so each test below can break exactly one thing.
    fn sweepable(slug: &str) -> ControlDesc {
        ControlDesc {
            id: ControlId(0x0098_0900),
            name: slug.to_owned(),
            slug: ControlSlug::parse(slug).expect("a literal slug"),
            control_type: ControlType::Integer,
            range: ControlRange {
                min: 0,
                max: 255,
                step: 1,
            },
            default: 128,
            // HAS_WHICH_MIN_MAX, which most integer controls on this kernel carry [PF:12]:
            // a fixture with a clean flag word would let a predicate that tested the whole
            // word instead of a bit pass by accident.
            flags: ControlFlags::from_raw(0x1000),
            menu: BTreeMap::new(),
            elems: 1,
            elem_size: 4,
            dims: Vec::new(),
            current: Some(ControlValue::Int(128)),
        }
    }

    /// The same control with `raw` or'd into its flag word.
    fn flagged(slug: &str, raw: u32) -> ControlDesc {
        let desc = sweepable(slug);
        ControlDesc {
            flags: ControlFlags::from_raw(0x1000 | raw),
            ..desc
        }
    }

    const DISABLED: u32 = 0x0001;
    const READ_ONLY: u32 = 0x0004;
    const INACTIVE: u32 = 0x0010;
    const WRITE_ONLY: u32 = 0x0040;
    const VOLATILE: u32 = 0x0080;

    #[test]
    fn a_camera_with_a_plain_brightness_is_given_it() {
        // The direction that must also hold, or every test below proves nothing: this is
        // the OBSBOT and the Chicony RGB, which is what E13's sweep arm ran against.
        let controls = vec![sweepable("brightness")];
        let SweepTarget::Found(desc) = brightness_class_target(&controls) else {
            panic!("a plain integer brightness is sweepable");
        };
        assert_eq!(desc.slug.as_str(), "brightness");
    }

    #[test]
    fn preference_order_is_the_list_and_a_disqualified_first_name_does_not_end_the_search() {
        // `gamma` is second in `BRIGHTNESS_CLASS` and must be reached when `brightness`
        // cannot be used — the bug where the list is searched for the first *named* control
        // rather than the first *usable* one turns one camera's read-only brightness into a
        // camera with nothing to sweep.
        let controls = vec![flagged("brightness", READ_ONLY), sweepable("gamma")];
        let SweepTarget::Found(desc) = brightness_class_target(&controls) else {
            panic!("gamma is usable and second in the list");
        };
        assert_eq!(desc.slug.as_str(), "gamma");
    }

    #[test]
    fn a_sensor_with_none_of_the_three_names_declines_about_its_control_set() {
        // The Chicony IR sensor, as E13 transcribes it: three controls, none of them
        // brightness-class. This is the one shape the old message was right about.
        let controls = vec![
            sweepable("user_controls"),
            sweepable("region_of_interest_rectangle"),
            sweepable("region_of_interest_auto_ctrls"),
        ];
        let SweepTarget::Declined(why) = brightness_class_target(&controls) else {
            panic!("no control here is brightness-class");
        };
        assert_eq!(why, Decline::NoneNamed { examined: 3 });
        assert_eq!(why.is_a_fact_about(), "this sensor's control set");
        assert_eq!(
            why.to_string(),
            "exposes none of brightness, gamma, gain among its 3 control(s)"
        );
    }

    #[test]
    fn an_inactive_gain_is_a_fact_about_a_state_and_not_about_a_control_set() {
        // **The finding, as one test.** `gain` is present; `auto_exposure` owns it [PF:3];
        // D3's pairing planner exists precisely to clear that, so the next run with
        // automation off would sweep it. The shipped message said this camera "exposes no
        // sweepable brightness-class control among its N, so this arm declines it — which
        // is a fact about this sensor's control set", which is false in both halves.
        let controls = vec![flagged("gain", INACTIVE)];
        let SweepTarget::Declined(why) = brightness_class_target(&controls) else {
            panic!("an INACTIVE control is not sweepable");
        };
        assert_eq!(
            why,
            Decline::NoneUsable(vec![(
                ControlSlug::parse("gain").expect("a literal slug"),
                Disqualifier::Inactive
            )])
        );
        assert_eq!(
            why.is_a_fact_about(),
            "the state of a control this sensor has"
        );
        assert_eq!(
            why.to_string(),
            "exposes gain (INACTIVE — an automation partner owns it [PF:3]), disqualified by \
             the term in parentheses"
        );
    }

    #[test]
    fn a_brightness_whose_current_is_outside_its_own_range_names_the_reading() {
        // AGENTS rule 6's represented-unknown class, and a PF-class device finding rather
        // than a missing control: the OBSBOT's `Zoom, Continuous` really does report 245 in
        // a range of -100..=100 [PF:4], and a `brightness` doing the same is the shape this
        // arm must report rather than silently reclassify.
        let mut desc = sweepable("brightness");
        desc.current = Some(ControlValue::Int(300));
        let SweepTarget::Declined(why) = brightness_class_target(std::slice::from_ref(&desc))
        else {
            panic!("an out-of-range current is not perturbable");
        };
        assert_eq!(
            why.is_a_fact_about(),
            "the state of a control this sensor has"
        );
        assert!(
            why.to_string()
                .contains("current 300 outside 0..=255 [PF:4]"),
            "{why}"
        );
    }

    #[test]
    fn a_brightness_sitting_off_its_own_step_names_the_step() {
        let mut desc = sweepable("brightness");
        desc.range = ControlRange {
            min: 0,
            max: 254,
            step: 2,
        };
        desc.current = Some(ControlValue::Int(7));
        let SweepTarget::Declined(why) = brightness_class_target(std::slice::from_ref(&desc))
        else {
            panic!("a current off its own step is not perturbable [PF:4]");
        };
        assert!(
            why.to_string()
                .contains("current 7 is not a whole number of steps of 2 above 0 [PF:4]"),
            "{why}"
        );
    }

    #[test]
    fn a_read_only_brightness_names_the_flag_that_refused_it() {
        let controls = vec![flagged("brightness", READ_ONLY)];
        let SweepTarget::Declined(why) = brightness_class_target(&controls) else {
            panic!("a read-only control is not writable");
        };
        assert!(why.to_string().contains("brightness (read-only)"), "{why}");
    }

    #[test]
    fn a_disabled_brightness_is_not_reported_as_a_read_only_one() {
        // `ControlDesc::is_writable` folds READ_ONLY, DISABLED and the class header into one
        // `false`; the diagnosis has to take them apart again, because DISABLED is the
        // device saying "not on this model" and READ_ONLY is "not by you".
        let controls = vec![flagged("brightness", DISABLED)];
        let SweepTarget::Declined(why) = brightness_class_target(&controls) else {
            panic!("a DISABLED control is not writable");
        };
        assert!(why.to_string().contains("brightness (DISABLED)"), "{why}");
    }

    #[test]
    fn a_volatile_brightness_and_a_write_only_one_name_their_own_flags() {
        for (raw, expected) in [
            (VOLATILE, "VOLATILE, so its value is the device's"),
            (WRITE_ONLY, "WRITE_ONLY, so there is nothing to read back"),
        ] {
            let controls = vec![flagged("brightness", raw)];
            let SweepTarget::Declined(why) = brightness_class_target(&controls) else {
                panic!("{expected}");
            };
            assert!(why.to_string().contains(expected), "{why}");
        }
    }

    #[test]
    fn a_menu_typed_brightness_is_declined_by_its_type() {
        // A menu is not a switch [PF:2] and it is not an integer sweep either. `Integer64`
        // is the same finding in a second costume: both are *capabilities*, and neither is
        // an absent control.
        for control_type in [ControlType::Menu, ControlType::Integer64] {
            let mut desc = sweepable("brightness");
            desc.control_type = control_type;
            let SweepTarget::Declined(why) = brightness_class_target(std::slice::from_ref(&desc))
            else {
                panic!("{control_type:?} is not a plain integer");
            };
            assert!(
                why.to_string()
                    .contains(&format!("brightness (type is {control_type:?})")),
                "{why}"
            );
        }
    }

    #[test]
    fn a_compound_typed_brightness_is_declined_before_its_payload_is_read() {
        // PF:1's shape: a type this build does not name, carrying bytes. It must decline as
        // a *type*, never as a missing control and never by touching the payload.
        let mut desc = sweepable("brightness");
        desc.control_type = ControlType::Unknown { raw: 0x0fff };
        desc.current = Some(ControlValue::Bytes(vec![0; 16]));
        let SweepTarget::Declined(why) = brightness_class_target(std::slice::from_ref(&desc))
        else {
            panic!("an opaque payload is not scalar");
        };
        assert!(why.to_string().contains("which carries a payload"), "{why}");
    }

    #[test]
    fn a_one_value_brightness_range_is_declined_by_its_range() {
        let mut desc = sweepable("brightness");
        desc.range = ControlRange {
            min: 5,
            max: 5,
            step: 1,
        };
        desc.current = Some(ControlValue::Int(5));
        let SweepTarget::Declined(why) = brightness_class_target(std::slice::from_ref(&desc))
        else {
            panic!("one value is not a sweep");
        };
        assert!(
            why.to_string().contains("range 5..=5 holds one value"),
            "{why}"
        );
    }

    #[test]
    fn every_disqualified_control_is_a_fact_about_a_state_and_never_about_a_control_set() {
        // The regression this entry exists for, asserted over the whole vocabulary at once
        // rather than one shape at a time: whatever term refuses a control the device
        // *has*, the sentence a `SKIP` line prints must not be the one about which controls
        // the device has. A future term added to `why_not_sweepable` joins this test by
        // being added to the table below, and a term that forgets to costs one line.
        let mut shapes: Vec<ControlDesc> = vec![
            flagged("brightness", READ_ONLY),
            flagged("brightness", DISABLED),
            flagged("brightness", INACTIVE),
            flagged("brightness", VOLATILE),
            flagged("brightness", WRITE_ONLY),
        ];
        let mut no_current = sweepable("brightness");
        no_current.current = None;
        shapes.push(no_current);
        let mut text_current = sweepable("brightness");
        text_current.current = Some(ControlValue::Text("bright".to_owned()));
        shapes.push(text_current);
        let mut out_of_range = sweepable("brightness");
        out_of_range.current = Some(ControlValue::Int(-1));
        shapes.push(out_of_range);
        let mut menu_typed = sweepable("brightness");
        menu_typed.control_type = ControlType::Menu;
        shapes.push(menu_typed);
        let mut single = sweepable("brightness");
        single.range = ControlRange {
            min: 1,
            max: 1,
            step: 1,
        };
        single.current = Some(ControlValue::Int(1));
        shapes.push(single);

        for desc in &shapes {
            let SweepTarget::Declined(why) = brightness_class_target(std::slice::from_ref(desc))
            else {
                panic!("{desc:?} must not be swept");
            };
            assert_eq!(
                why.is_a_fact_about(),
                "the state of a control this sensor has",
                "{why}"
            );
            assert!(
                !why.to_string().contains("exposes none of"),
                "a control the device has was reported as one it does not have: {why}"
            );
        }
    }

    #[test]
    fn every_named_control_is_reported_and_not_only_the_first() {
        // A camera with two brightness-class controls, both refused for different reasons,
        // must say both: a transcript that named only `brightness` would send a reader
        // looking for a `gain` fault that is already recorded.
        let controls = vec![flagged("brightness", READ_ONLY), flagged("gain", INACTIVE)];
        let SweepTarget::Declined(why) = brightness_class_target(&controls) else {
            panic!("neither is usable");
        };
        let text = why.to_string();
        assert!(text.contains("brightness (read-only)"), "{text}");
        assert!(text.contains("gain (INACTIVE"), "{text}");
    }

    #[test]
    fn is_perturbable_and_why_not_perturbable_are_one_rule_and_not_two() {
        // The two must never drift, which is why the bool is *defined* as the reason
        // answering nothing. This asserts the property over every shape above rather than
        // trusting the definition to stay that way.
        let mut shapes = vec![sweepable("brightness")];
        for raw in [READ_ONLY, DISABLED, VOLATILE, WRITE_ONLY, INACTIVE] {
            shapes.push(flagged("brightness", raw));
        }
        let mut opaque = sweepable("brightness");
        opaque.control_type = ControlType::Unknown { raw: 0x0fff };
        shapes.push(opaque);
        let mut unread = sweepable("brightness");
        unread.current = None;
        shapes.push(unread);
        let mut adrift = sweepable("brightness");
        adrift.current = Some(ControlValue::Int(999));
        shapes.push(adrift);

        for desc in &shapes {
            assert_eq!(
                is_perturbable(desc),
                why_not_perturbable(desc).is_none(),
                "{desc:?}"
            );
        }
    }

    #[test]
    fn a_motorized_control_is_refused_by_the_motor_rule_before_anything_else() {
        // Not reachable through `BRIGHTNESS_CLASS` today — none of the three names contains
        // a motor fragment — and asserted anyway, because design §5 is a law about hardware
        // wear rather than a consequence of how three controls happen to be spelled. The
        // day somebody adds `focus_absolute` to the list, this is the term that must fire.
        let desc = sweepable("zoom_absolute");
        assert_eq!(why_not_sweepable(&desc), Some(Disqualifier::Motorized));
    }

    // --------------------------------------- how big a sweep a range plans (N72, amended)
    //
    // [`sweep_for`] arrived here from `crates/client/tests/hardware.rs`, where N72 wrote it
    // and where seven arms still pin it to *that* rung's floor and to the ranges E13
    // transcribed. What belongs here is the half those arms cannot see, because a suite that
    // only ever passes its own constant cannot notice that the constant is the only thing
    // reaching the transcript: that the two rungs' floors produce two different sentences,
    // and that both declines are values a reader can match on rather than prose.

    /// The same control with a range and step a device declared.
    fn ranged(slug: &str, min: i64, max: i64, step: i64) -> ControlDesc {
        let desc = sweepable(slug);
        ControlDesc {
            range: ControlRange { min, max, step },
            current: Some(ControlValue::Int(min)),
            default: min,
            ..desc
        }
    }

    /// The two floors the workspace actually holds, spelled here so the assertion below is
    /// about *them* and not about a pair invented for it.
    const PROGRESS_FLOOR: SampleFloor = SampleFloor {
        count: 3,
        because: "say anything about an arrival profile",
    };
    const ORDERING_FLOOR: SampleFloor = SampleFloor {
        count: 3,
        because: "rank a metric across a sweep rather than compare its two ends",
    };

    #[test]
    fn a_range_under_the_floor_declines_as_a_value_and_names_the_count_the_planner_gave_it() {
        // The finding, as one test: `0..=64` with a step of 64 plans two values, clears every
        // term of `brightness_class_target`, and was therefore selected, swept on a real
        // sensor, and *then* panicked on — in both rungs, twenty lines above one restore and
        // three hundred above the other.
        let desc = ranged("brightness", 0, 64, 64);
        assert_eq!(
            sweep_for(&desc, PROGRESS_FLOOR),
            SweepChoice::Declined(ShortSweep::UnderFloor {
                control: ControlSlug::parse("brightness").expect("a literal slug"),
                min: 0,
                max: 64,
                step: 64,
                stride: 64,
                samples: 2,
                floor: PROGRESS_FLOOR,
            })
        );
    }

    #[test]
    fn the_two_rungs_decline_at_the_same_count_and_do_not_print_the_same_sentence() {
        // Two arms, one number, two unrelated reasons for it — and `SampleFloor::because` is
        // the whole of what keeps them apart in a transcript. A build that dropped the clause
        // would leave `smoke-hw.sh` printing one line for two findings, which is precisely
        // the shape N72's F5 was about one type over.
        let desc = ranged("brightness", 0, 1, 1);
        let progress = sweep_for(&desc, PROGRESS_FLOOR);
        let ordering = sweep_for(&desc, ORDERING_FLOOR);
        assert_ne!(progress, ordering);

        let SweepChoice::Declined(progress) = progress else {
            panic!("a two-value range is under both floors");
        };
        let SweepChoice::Declined(ordering) = ordering else {
            panic!("a two-value range is under both floors");
        };
        assert!(
            progress
                .to_string()
                .contains("the 3 this arm needs to say anything about an arrival profile"),
            "{progress}"
        );
        assert!(
            ordering.to_string().contains(
                "the 3 this arm needs to rank a metric across a sweep rather than compare \
                 its two ends"
            ),
            "{ordering}"
        );
        // And the half both sentences must carry, because *when* the decline happened is the
        // finding rather than a detail of it.
        for why in [&progress, &ordering] {
            assert!(
                why.to_string()
                    .contains("declines before writing to the camera rather than after"),
                "{why}"
            );
        }
    }

    #[test]
    fn a_range_the_planner_refuses_outright_is_a_decline_and_not_a_panic() {
        // A descriptor whose maximum is below its minimum: represented, never corrected (D2),
        // and refused by `engine::sweep::plan` as `empty_range`. Nothing either rung selects
        // can be in that state today — `why_not_sweepable` requires `max > min` — and the two
        // predicates are different rules, so the day they disagree this must be a named
        // decline rather than a `?` that became a panic on somebody's hardware.
        let SweepChoice::Declined(why) =
            sweep_for(&ranged("brightness", 200, 100, 1), ORDERING_FLOOR)
        else {
            panic!("a backwards range is not plannable");
        };
        assert!(
            matches!(
                &why,
                ShortSweep::Refused {
                    min: 200,
                    max: 100,
                    step: 1,
                    ..
                }
            ),
            "{why:?}"
        );
        let text = why.to_string();
        assert!(
            text.contains("the sweep planner refuses brightness"),
            "{text}"
        );
        assert!(text.contains("empty_range"), "{text}");
    }

    #[test]
    fn the_declared_step_reaches_the_refusal_and_the_effective_one_reaches_the_count() {
        // A device declaring a step of 0 is \[PF:4\] territory, and the two halves of this
        // type deliberately report different numbers for it. `Refused` carries the step **as
        // declared**, because a transcript that silently printed 1 would hide the finding;
        // `UnderFloor` carries the *effective* step, because that is the number the count was
        // computed against and a reader checking the arithmetic needs the one that was used.
        let SweepChoice::Declined(refused) =
            sweep_for(&ranged("brightness", 200, 100, 0), ORDERING_FLOOR)
        else {
            panic!("a backwards range is not plannable whatever its step");
        };
        assert!(refused.to_string().contains("(step 0)"), "{refused}");

        let SweepChoice::Declined(under) =
            sweep_for(&ranged("brightness", 0, 1, 0), ORDERING_FLOOR)
        else {
            panic!("a two-value range is under this floor");
        };
        assert!(under.to_string().contains("with a step of 1"), "{under}");
    }

    #[test]
    fn the_count_is_the_planners_and_not_arithmetic_repeated_here() {
        // A control whose own step is 7 cannot take a stride of 25, and `engine::sweep::plan`
        // rounds the request up to 28 rather than writing values the device would silently
        // align \[PF:6\]. Naive arithmetic over the stride this function computes would answer
        // five samples; the planner answers four, and the planner is the one both executors
        // run. This is the assertion that would go red if the count were ever re-derived
        // instead of asked for.
        assert_eq!(
            sweep_for(&ranged("brightness", 0, 100, 7), ORDERING_FLOOR),
            SweepChoice::Planned {
                spec: SweepSpec::Uniform { step: 25 },
                samples: 4,
            }
        );
    }

    #[test]
    fn a_floor_of_zero_declines_nothing_and_is_still_the_planners_answer() {
        // The degenerate floor, and the reason the comparison is `samples < floor.count`
        // rather than a special case: an arm that wants whatever the range offers passes a
        // count of zero and gets a plan, including for the one-value range that a floor of
        // three declines. Nothing in the workspace does this today; it is here because a
        // `SampleFloor` is a caller's number and a fold over values should not have a hole at
        // the bottom of its own parameter.
        let floor = SampleFloor {
            count: 0,
            because: "assert nothing about how many samples there were",
        };
        assert_eq!(
            sweep_for(&ranged("brightness", 50, 50, 1), floor),
            SweepChoice::Planned {
                spec: SweepSpec::Uniform { step: 1 },
                samples: 1,
            }
        );
    }

    // ------------------------- what a restore report will answer for \[PF:24\]
    //
    // The same population argument the sweep tests above make, and for a stronger reason:
    // the shape these are about **is on this desk and cannot be scheduled**. PF:24's
    // amendment measured both hardware arms green on the drifting camera because the room
    // was dark, so a hardware run can neither find this nor prove it fixed — it can only
    // agree with whatever the last hour's light did. A table of `RestoreOutcome` values can
    // do both, on a machine with nothing plugged in.

    fn slug(text: &str) -> ControlSlug {
        ControlSlug::parse(text).expect("a literal slug")
    }

    /// A control the report says it wrote, exactly as asked.
    fn restored(control: &str) -> RestoreOutcome {
        RestoreOutcome::Restored {
            applied: schema::control::Applied {
                control: ControlId(0x0098_0900),
                slug: slug(control),
                requested: ControlValue::Int(128),
                applied: ControlValue::Int(128),
                warnings: Vec::new(),
            },
        }
    }

    /// A control whose automation owns it again — PF:24's outcome, and note N9's success.
    fn owned(control: &str, automation: Option<&str>) -> RestoreOutcome {
        RestoreOutcome::OwnedByAutomation {
            control: slug(control),
            automation: automation.map(slug),
        }
    }

    #[test]
    fn a_control_left_to_its_automation_is_not_one_the_arm_may_assert_a_number_for() {
        // The defect, as a value. `white_balance_temperature` is INACTIVE at both ends of
        // the restore, so its read-back is the AWB algorithm's answer and not a setting
        // \[PF:24\]; `brightness` was written and is the arm's business. The two must not be
        // in the same population, and nothing here spells either name — the outcomes do.
        let claim = restoration_claim(&RestoreReport {
            freed: Vec::new(),
            outcomes: vec![
                restored("brightness"),
                owned("white_balance_temperature", Some("white_balance_automatic")),
            ],
        });
        assert!(claim.speaks_for("brightness"));
        assert!(!claim.speaks_for("white_balance_temperature"));
        assert_eq!(
            claim.left_to_automation(),
            [(
                slug("white_balance_temperature"),
                Some(slug("white_balance_automatic"))
            )]
        );
    }

    #[test]
    fn the_decline_names_every_excluded_control_and_the_automation_that_owns_it() {
        // The audit trail the exclusion is paid for with (AGENTS rule 3): a reader has to be
        // able to switch the named automation off and watch the control become the arm's
        // business again, which is PF:24's own inverse arm. A count on its own would not let
        // them.
        let claim = restoration_claim(&RestoreReport {
            freed: Vec::new(),
            outcomes: vec![
                owned("white_balance_temperature", Some("white_balance_automatic")),
                owned("exposure_time_absolute", Some("auto_exposure")),
                owned("focus_absolute", None),
                restored("brightness"),
            ],
        });
        assert_eq!(
            claim.to_string(),
            "exposure_time_absolute (auto_exposure), focus_absolute (no partner in this \
             device's pair set), white_balance_temperature (white_balance_automatic)"
        );
    }

    #[test]
    fn an_unrestorable_control_belongs_to_is_complete_and_to_neither_population_here() {
        // One verdict, one home. A control nobody could put back is a red run by
        // `RestoreReport::is_complete`, which every arm asserts separately — so counting it
        // here as "left to its automation" would excuse it, and counting it as claimed would
        // make the arm assert a value against a control the restore has already said it
        // could not write.
        let claim = restoration_claim(&RestoreReport {
            freed: Vec::new(),
            outcomes: vec![
                restored("brightness"),
                RestoreOutcome::Unrestorable {
                    control: slug("gamma"),
                    reason: schema::snapshot::UnrestorableReason::StillInactive {
                        automation: Some(slug("gain_automatic")),
                    },
                },
            ],
        });
        assert!(!claim.speaks_for("gamma"));
        assert!(claim.left_to_automation().is_empty());
    }

    #[test]
    #[should_panic(expected = "reading as a pass")]
    fn a_restore_that_claimed_nothing_at_all_is_a_failure_and_not_a_quiet_success() {
        // The whole exclusion, taken to its end: every outcome is `OwnedByAutomation`, so
        // `is_complete()` is **true** (note N9) and an arm filtering on this claim would
        // compare nothing, print a green line and have said nothing whatever about AGENTS
        // rule 8. This is the assertion that stops the repair from becoming the defect it
        // was written against.
        let claim = restoration_claim(&RestoreReport {
            freed: Vec::new(),
            outcomes: vec![
                owned("white_balance_temperature", Some("white_balance_automatic")),
                owned("exposure_time_absolute", Some("auto_exposure")),
            ],
        });
        claim.account_for("cam:nothing-left-to-check", 0);
    }

    #[test]
    fn a_camera_with_nothing_writable_declines_by_name_rather_than_failing() {
        // The other side of the line above, and AGENTS rule 7 is where it sits: "this
        // restore checked nothing because everything was an algorithm's" is a defect in the
        // suite, and "this restore checked nothing because the device has no writable
        // control" is a fact about the device. An empty report must not panic — the counted
        // line it prints instead is what keeps it from reading as a pass.
        restoration_claim(&RestoreReport {
            freed: Vec::new(),
            outcomes: Vec::new(),
        })
        .account_for("cam:three-read-only-controls", 0);
    }
}
