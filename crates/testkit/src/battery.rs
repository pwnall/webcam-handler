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
#[must_use]
pub fn is_perturbable(desc: &ControlDesc) -> bool {
    desc.is_writable()
        && desc.control_type.is_scalar()
        && !desc.is_volatile()
        && !desc.flags.has(KnownFlag::WriteOnly)
        && matches!(
            desc.current,
            Some(ControlValue::Int(v))
                if desc.range.contains(v) && is_step_aligned(&desc.range, v)
        )
}

/// Whether `value` sits on one of the control's steps, counting from its minimum.
fn is_step_aligned(range: &ControlRange, value: i64) -> bool {
    value
        .checked_sub(range.min)
        .is_some_and(|offset| offset % range.effective_step() == 0)
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
}
