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
    CAP_META_CAPTURE, CAP_VIDEO_CAPTURE, CameraFingerprint, CameraInfo, NodeKind, PixelFormat,
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
use schema::snapshot::{ControlRole, Snapshot, SnapshotEntry};
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
    fn skipped(reason: impl Into<String>) -> ArmOutcome {
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
    };

    for &arm in BatteryArm::ALL {
        let mut failures = Vec::new();
        let outcome = {
            let mut log = ArmLog {
                arm,
                failures: &mut failures,
            };
            arm.execute(backend, &mut log)
        };
        report.failures.extend(failures);

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
}

impl ArmLog<'_> {
    fn fail(&mut self, message: impl AsRef<str>) {
        let arm = self.arm;
        self.failures
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

        for (id, value) in &perturbations {
            if let Err(error) = camera.set(*id, value.clone()) {
                visit.log.fail(format!(
                    "perturbing control {id} to {value} failed: {error}"
                ));
            }
        }

        // Non-vacuity: if nothing actually moved, "restore put it back" proves nothing.
        let Some(perturbed) = read_controls(camera.as_mut(), &info.id.to_string(), visit.log)
        else {
            continue;
        };
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
                "{}: {} perturbations left every control where it was, so the restore \
                 below would prove nothing",
                info.id,
                perturbations.len()
            )
        });

        for entry in snapshot.restore_order() {
            if let Err(error) = camera.set(entry.id, entry.value.clone()) {
                visit.log.fail(format!(
                    "restoring {} to {} failed: {error}",
                    entry.control, entry.value
                ));
            }
        }

        let Some(after) = read_controls(camera.as_mut(), &info.id.to_string(), visit.log) else {
            continue;
        };
        compare_control_state(&before, &after, &flags_before, visit.log);
    }

    visit.finish("no camera offered a control this arm could perturb")
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
    fn finish(self, fallback: &str) -> ArmOutcome {
        if self.examined > 0 {
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
    use schema::control::ControlFlags;
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
}
