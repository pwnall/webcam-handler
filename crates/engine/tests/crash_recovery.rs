//! A sweep killed mid-flight, and the recovery that puts the camera back (design §6, gate
//! G3's crash-recovery criterion, docs/7 P3b).
//!
//! Design §6 records the risk in one sentence — *calibration sweeps move physical
//! hardware, and a crashed sweep can leave a camera mis-set* — and one mitigation: the
//! snapshot is persisted to the session directory **before** the first write, so `restore`
//! survives the death of the process that took it. This suite is the assertion behind that
//! sentence.
//!
//! ## What is real here, and what is a double
//!
//! **The kill is real.** A child process is spawned — this same test binary, told to run
//! the sweep half by name — and killed with `SIGKILL`. Not a panic, not an unwound stack,
//! not a `Drop` impl that got a last word: the signal a crash actually looks like, the one
//! no destructor runs after. The exit status is asserted to *be* a signal, so a child that
//! finished on its own could never be mistaken for one that was killed.
//!
//! **The synchronisation is real.** The child announces itself on a pipe and then blocks
//! reading another one; the parent kills it the moment the announcement arrives. No sleep
//! is involved in either direction, and if the child never gets that far the parent fails
//! with everything the child printed rather than hanging.
//!
//! **The state directory is real** — a real `session.json` published by rename, a real
//! `log.ndjson`, a real advisory `flock` which the child holds until the kernel takes it
//! away with the child.
//!
//! **The camera is a double, and it is a double with one unusual property**: its control
//! values live in a file, so they outlive the process that wrote them. That is not
//! decoration — it *is* the property under test. A camera whose state died with the sweep
//! would leave nothing to recover, and a test that restored an in-memory struct would be
//! proving that a struct survived its own scope. The device model is deliberately small
//! (three controls, one automation pair, PF:3's INACTIVE coupling) because the subject is
//! the crash, not the driver; realistic device behaviour is `webcam-handler-fake`'s job
//! and it cannot survive a `SIGKILL`.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::process::ExitStatusExt;
use std::process::{Child, Command, Stdio};
use std::time::Instant;

use camino::{Utf8Path, Utf8PathBuf};
use engine::lifecycle::{self, Recovery, SessionSpec};
use engine::store::{LockProtocol, SessionStore, StoreLock, TempStore, write_json_atomic};
use schema::backend::{BackendKind, Camera};
use schema::camera::{
    CameraFingerprint, CameraId, CameraInfo, DeviceNode, FormatInfo, NodeKind, PixelFormat,
};
use schema::capture::{Frame, NegotiatedStream, StreamRequest};
use schema::control::{
    Applied, ControlDesc, ControlFlags, ControlId, ControlRange, ControlSlug, ControlType,
    ControlValue, KnownFlag, WriteWarning,
};
use schema::error::{Error, Result};
use schema::limits;
use schema::pairing::{AutomationOff, AutomationPair, Provenance};
use schema::session::SessionEvent;
use schema::snapshot::RestoreOutcome;
use schema::time::Stamp;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// The state directory the child is to use.
const STATE_ENV: &str = "WCH_CRASH_STATE_DIR";
/// The file the child's camera keeps its control values in.
const DEVICE_ENV: &str = "WCH_CRASH_DEVICE_FILE";
/// The line the child prints once its sweep has reached the camera.
const READY: &str = "wch-crash-child: the first write is on the device";
/// The test the child runs — the same one that, with no environment pointing at it, is
/// this file's in-process proof of the ordering.
const CHILD_TEST: &str = "a_sweep_persists_its_snapshot_before_its_first_write_reaches_the_camera";

const TASK: &str = "Read text from the DUT display";

// ------------------------------------------------------------------ the persistent camera

/// The device's state, as a file. See this file's header for why it is one.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct DeviceState {
    /// Control slug to current value.
    values: BTreeMap<String, i64>,
    /// Every write that reached the device, in order, across every process that made one.
    writes: Vec<DeviceWrite>,
}

/// One write, and what the session document looked like when it arrived.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct DeviceWrite {
    control: String,
    value: i64,
    /// Whether `session.json` already carried its pre-sweep snapshot at the instant this
    /// write reached the device.
    ///
    /// The ordering §6 depends on, observed **at the device** rather than inferred from
    /// the order two functions were called in. An implementation that persisted the
    /// snapshot after its first write leaves `false` here, and leaves a crash with
    /// nothing to restore from.
    saw_pre_snapshot: bool,
}

/// The camera as the operator left it.
fn as_found() -> DeviceState {
    DeviceState {
        values: BTreeMap::from([
            ("white_balance_automatic".to_owned(), 1),
            ("white_balance_temperature".to_owned(), 4600),
            ("brightness".to_owned(), 50),
        ]),
        writes: Vec::new(),
    }
}

fn fingerprint() -> CameraFingerprint {
    CameraFingerprint {
        bus_path: "3-1:1.0".to_owned(),
        usb_id: None,
        card: "Crash Fixture".to_owned(),
        driver: "crash-fixture".to_owned(),
        serial: None,
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

fn slug(name: &str) -> ControlSlug {
    ControlSlug::parse(name).expect("literal slug")
}

fn control(
    id: u32,
    name: &str,
    control_type: ControlType,
    range: ControlRange,
    current: i64,
) -> ControlDesc {
    ControlDesc {
        id: ControlId(id),
        name: name.to_owned(),
        slug: slug(name),
        control_type,
        default: range.min,
        range,
        flags: ControlFlags::from_raw(0),
        menu: BTreeMap::new(),
        elems: 1,
        elem_size: 4,
        dims: Vec::new(),
        current: Some(ControlValue::Int(current)),
    }
}

fn range(min: i64, max: i64) -> ControlRange {
    ControlRange { min, max, step: 1 }
}

/// A camera whose control values live in a file.
#[derive(Debug)]
struct PersistentCamera {
    /// Where the device's state lives.
    path: Utf8PathBuf,
    /// The session document to consult when a write arrives, when there is one.
    session_file: Option<Utf8PathBuf>,
    state: DeviceState,
    info: CameraInfo,
}

impl PersistentCamera {
    /// Open the device at `path`, reading whatever the last process left in it.
    fn open(path: &Utf8Path, session_file: Option<Utf8PathBuf>) -> PersistentCamera {
        let bytes = std::fs::read(path.as_std_path()).expect("the device file exists");
        let state: DeviceState = serde_json::from_slice(&bytes).expect("the device file parses");
        PersistentCamera {
            path: path.to_owned(),
            session_file,
            state,
            info: CameraInfo {
                id: CameraId::parse("cam:crash").expect("literal id"),
                fingerprint: fingerprint(),
                card: "Crash Fixture".to_owned(),
                driver: "crash-fixture".to_owned(),
                bus_info: "usb-crash".to_owned(),
                nodes: vec![DeviceNode {
                    path: "/dev/null".into(),
                    kind: NodeKind::VideoCapture,
                    device_caps: 0,
                    capabilities: 0,
                }],
                backend: BackendKind::Fake,
            },
        }
    }

    /// Lay the device down as the operator left it.
    fn plant(path: &Utf8Path) {
        write_json_atomic(path, &as_found()).expect("a writable device file");
    }

    /// The device's state right now, read from the file rather than from memory — which is
    /// how a process that did not do the writing sees them.
    fn read(path: &Utf8Path) -> DeviceState {
        let bytes = std::fs::read(path.as_std_path()).expect("the device file exists");
        serde_json::from_slice(&bytes).expect("the device file parses")
    }

    fn value_of(&self, name: &str) -> Option<i64> {
        self.state.values.get(name).copied()
    }

    /// The descriptors, with PF:3's coupling applied: the manual control is INACTIVE
    /// exactly while its automation partner is engaged.
    fn descriptors(&self) -> Vec<ControlDesc> {
        let value = |name: &str| self.state.values.get(name).copied().unwrap_or_default();
        let automatic = value("white_balance_automatic");
        let mut temperature = control(
            2,
            "white_balance_temperature",
            ControlType::Integer,
            range(2800, 6500),
            value("white_balance_temperature"),
        );
        if automatic != 0 {
            temperature.flags = ControlFlags::from_raw(KnownFlag::Inactive.bit());
        }
        vec![
            control(
                1,
                "white_balance_automatic",
                ControlType::Boolean,
                range(0, 1),
                automatic,
            ),
            temperature,
            control(
                3,
                "brightness",
                ControlType::Integer,
                range(0, 100),
                value("brightness"),
            ),
        ]
    }

    /// Whether the session document already carries its pre-sweep snapshot.
    fn saw_pre_snapshot(&self) -> bool {
        let Some(path) = &self.session_file else {
            return false;
        };
        let Ok(bytes) = std::fs::read(path.as_std_path()) else {
            return false;
        };
        serde_json::from_slice::<serde_json::Value>(&bytes)
            .ok()
            .and_then(|document| document.get("pre_snapshot").cloned())
            .is_some_and(|found| !found.is_null())
    }

    /// Publish the device's state.
    ///
    /// Through the store's own atomic write, and for the store's own reason: this process
    /// is about to be killed, and a device file caught half-written would make the suite
    /// flake instead of fail.
    fn publish(&self) {
        write_json_atomic(&self.path, &self.state).expect("a writable device file");
    }
}

impl Camera for PersistentCamera {
    fn info(&self) -> &CameraInfo {
        &self.info
    }

    fn formats(&self) -> Result<Vec<FormatInfo>> {
        // No format, so `start_stream` refuses without anyone scripting a lie. This
        // suite's subject is control state across a crash; a frame never enters it.
        Ok(Vec::new())
    }

    fn controls(&self) -> Result<Vec<ControlDesc>> {
        Ok(self.descriptors())
    }

    fn get(&mut self, id: ControlId) -> Result<ControlValue> {
        self.descriptors()
            .into_iter()
            .find(|desc| desc.id == id)
            .and_then(|desc| desc.current)
            .ok_or_else(|| Error::ControlUnknown {
                requested: id.to_string(),
                did_you_mean: Vec::new(),
            })
    }

    fn set(&mut self, id: ControlId, value: ControlValue) -> Result<Applied> {
        let desc = self
            .descriptors()
            .into_iter()
            .find(|desc| desc.id == id)
            .ok_or_else(|| Error::ControlUnknown {
                requested: id.to_string(),
                did_you_mean: Vec::new(),
            })?;
        let requested = value.as_int().ok_or_else(|| Error::DeviceIo {
            operation: "VIDIOC_S_EXT_CTRLS".to_owned(),
            errno: Some(22),
            message: format!("{} takes an integer", desc.slug),
        })?;

        // A driver clamps and reports success [PF:6]; the read-back is what tells the
        // truth, and this double tells it the same way.
        let applied = requested.clamp(desc.range.min, desc.range.max);
        self.state.values.insert(desc.slug.to_string(), applied);
        self.state.writes.push(DeviceWrite {
            control: desc.slug.to_string(),
            value: applied,
            saw_pre_snapshot: self.saw_pre_snapshot(),
        });
        self.publish();

        let applied = ControlValue::Int(applied);
        Ok(Applied {
            control: id,
            slug: desc.slug.clone(),
            warnings: WriteWarning::classify(&desc, &value, &applied),
            requested: value,
            applied,
        })
    }

    fn start_stream(&mut self, request: &StreamRequest) -> Result<NegotiatedStream> {
        Err(Error::format_unsupported(
            request.pixel_format,
            Vec::<PixelFormat>::new(),
        ))
    }

    fn streaming(&self) -> Option<NegotiatedStream> {
        // A fixture that refuses every `start_stream` is never streaming, and saying so is
        // not the same as refusing: this is the one question on the trait whose answer is a
        // fact rather than an operation.
        None
    }

    fn next_frame(&mut self, _deadline: Instant) -> Result<Frame> {
        Err(Error::DeviceIo {
            operation: "next_frame".to_owned(),
            errno: None,
            message: "this fixture does not stream".to_owned(),
        })
    }

    fn stop_stream(&mut self) -> Result<()> {
        Ok(())
    }
}

// ------------------------------------------------------------------ the sweep both halves run

/// Where one run of the sweep keeps its things.
#[derive(Debug)]
struct Plan {
    state_root: Utf8PathBuf,
    device: Utf8PathBuf,
}

impl Plan {
    /// The plan the parent handed us, when we are the child.
    fn from_env() -> Option<Plan> {
        let state_root = std::env::var(STATE_ENV).ok()?;
        let device = std::env::var(DEVICE_ENV).ok()?;
        Some(Plan {
            state_root: Utf8PathBuf::from(state_root),
            device: Utf8PathBuf::from(device),
        })
    }
}

fn spec() -> SessionSpec {
    SessionSpec {
        id: Uuid::new_v7(uuid::Timestamp::from_unix(uuid::NoContext, 1_000, 0)),
        fingerprint: fingerprint(),
        task: TASK.to_owned(),
        goal: "the DUT's serial number is legible".to_owned(),
        criteria: Vec::new(),
        tool_version: "0.1.0".to_owned(),
    }
}

fn now() -> Stamp {
    Stamp::from_millis(1_000).expect("in range")
}

/// Everything a sweep does before its first sample: take the lock, open the session,
/// persist the camera as found, and write.
///
/// One function, run by the child *and* by the in-process arm, so the two cannot drift
/// into testing different code.
fn open_and_write(plan: &Plan) -> (StoreLock, PersistentCamera) {
    let store = SessionStore::new(plan.state_root.clone());
    // The daemon's protocol: held for the run. The child is killed holding it, which is
    // how the parent gets to assert that a dead holder's lock is the kernel's to release.
    let lock = store
        .lock(LockProtocol::HeldForLifetime)
        .expect("nobody else holds the state directory");
    let mut session = lifecycle::create(&store, &lock, &spec(), now()).expect("a free slot");

    let session_file = store.session_dir(&session).join(limits::SESSION_FILE);
    let mut camera = PersistentCamera::open(&plan.device, Some(session_file));

    lifecycle::sweep_write(
        &store,
        &lock,
        &mut session,
        &mut camera,
        &[white_balance_pair()],
        &[
            (slug("white_balance_temperature"), ControlValue::Int(6000)),
            (slug("brightness"), ControlValue::Int(0)),
        ],
        now(),
    )
    .expect("a willing camera and a writable store");

    // The lock goes back to the caller rather than being dropped here: the child has to
    // keep holding it while it waits to be killed, so that the kill is what releases it.
    (lock, camera)
}

// ------------------------------------------------------------------ the tests

/// The ordering design §6 rests on — and, when a parent asks for it by name, the child
/// half of the crash test.
///
/// Both arms run `open_and_write`. With no environment pointing at a state directory this
/// asserts the ordering in this process; with one, it announces itself and waits for the
/// signal that ends it.
#[test]
fn a_sweep_persists_its_snapshot_before_its_first_write_reaches_the_camera() {
    if let Some(plan) = Plan::from_env() {
        let _held = open_and_write(&plan);
        let mut out = std::io::stdout();
        writeln!(out, "{READY}").expect("the parent is listening");
        out.flush().expect("the parent is listening");

        // Blocking on a pipe rather than on a clock: no sleep is a synchronisation
        // (rubric Part C), and reading stdin also means a parent that dies without killing
        // us closes the pipe and lets us go instead of leaving a process behind.
        let mut ignored = String::new();
        let _ = std::io::stdin().read_line(&mut ignored);
        return;
    }

    let temp = TempStore::new().expect("a temp dir");
    let device = temp.root().join("device-under-test.json");
    PersistentCamera::plant(&device);
    let plan = Plan {
        state_root: temp.root().to_owned(),
        device: device.clone(),
    };

    let (lock, camera) = open_and_write(&plan);
    assert_eq!(
        camera.value_of("brightness"),
        Some(0),
        "the sweep did write"
    );
    drop(lock);

    let state = PersistentCamera::read(&device);
    assert!(
        !state.writes.is_empty(),
        "no write reached the device, so this proves nothing about their order"
    );
    let first = state.writes.first().expect("a write happened");
    assert!(
        first.saw_pre_snapshot,
        "the first write ({}) reached the camera before the snapshot was on disk — a crash \
         here leaves a mis-set camera and no record to put it back",
        first.control
    );
    assert!(
        state.writes.iter().all(|write| write.saw_pre_snapshot),
        "{:?}",
        state.writes
    );

    let store = SessionStore::new(temp.root().to_owned());
    let session = lifecycle::resume(&store, &fingerprint(), TASK)
        .expect("readable")
        .expect("still open");
    assert!(
        session.pre_snapshot.is_some(),
        "the document carries no snapshot at all"
    );
}

#[test]
fn a_sweep_killed_between_its_write_and_its_restore_recovers_from_the_persisted_snapshot() {
    // G3's crash-recovery criterion (docs/9's "Crash-recovery case" row). A real child
    // process, a real SIGKILL between the write and the restore, and a recovery run by a
    // process that never saw the sweep.
    let temp = TempStore::new().expect("a temp dir");
    let device = temp.root().join("device-under-test.json");
    PersistentCamera::plant(&device);

    let mut child = spawn_child(temp.root(), &device);
    wait_for_ready(&mut child);

    // The crash. `Child::kill` is SIGKILL on Unix: no unwinding, no `Drop`, no last-gasp
    // restore — which is the whole point. A test that let the child exit tidily would be
    // asserting that a well-behaved program cleans up after itself.
    child.kill().expect("the child is still running");
    let status = child.wait().expect("the child is reapable");
    assert_eq!(
        status.signal(),
        Some(9),
        "the child exited on its own instead of being killed: {status:?}"
    );
    assert_eq!(status.code(), None, "a killed process has no exit code");

    // The camera is mis-set, exactly as design §6 describes. If it were not, there would
    // be nothing to recover and every assertion below would pass vacuously.
    let crashed = PersistentCamera::read(&device);
    assert_eq!(crashed.values.get("brightness"), Some(&0));
    assert_eq!(crashed.values.get("white_balance_automatic"), Some(&0));
    assert_eq!(crashed.values.get("white_balance_temperature"), Some(&6000));
    assert!(
        crashed
            .writes
            .first()
            .is_some_and(|write| write.saw_pre_snapshot),
        "the snapshot was not on disk when the camera moved: {:?}",
        crashed.writes
    );

    // The lock the child was holding died with it: an `flock` is released when the last
    // descriptor on its open file description closes, and a killed process closes all of
    // them (P3a's `a_dropped_lock_is_released`, from the other side).
    let store = SessionStore::new(temp.root().to_owned());
    assert!(
        store.holder().is_none(),
        "a dead process is still reported as holding the state directory"
    );

    // A process that never saw the sweep picks the session up and puts the camera back.
    let mut session = lifecycle::resume(&store, &fingerprint(), TASK)
        .expect("the crashed session is readable")
        .expect("a session that never finished is still open");
    let persisted = session
        .pre_snapshot
        .clone()
        .expect("the snapshot the sweep persisted before it wrote");
    assert_eq!(persisted.camera, fingerprint());

    let session_file = store.session_dir(&session).join("session.json");
    let mut camera = PersistentCamera::open(&device, Some(session_file));
    let recovery = store
        .with_lock(|lock| {
            lifecycle::recover(
                &store,
                lock,
                &mut session,
                &mut camera,
                &[white_balance_pair()],
                now(),
            )
        })
        .expect("the lock is free and the snapshot is this camera's");

    let Recovery::Restored { report } = &recovery else {
        panic!("the crashed sweep was not recovered: {recovery:?}");
    };
    assert!(report.is_complete(), "{report:?}");
    assert!(
        report.outcomes.iter().any(|outcome| matches!(
            outcome,
            RestoreOutcome::OwnedByAutomation { control, .. }
                if control.as_str() == "white_balance_temperature"
        )),
        "the control its automation owns again must be reported as owned, not as a \
         failure (note N9): {report:?}"
    );

    // The camera is where the operator left it: the automation back on, the manual control
    // it owns INACTIVE again, and the unrelated control at its old value.
    let restored = PersistentCamera::read(&device);
    assert_eq!(restored.values.get("white_balance_automatic"), Some(&1));
    assert_eq!(restored.values.get("brightness"), Some(&50));
    assert!(
        camera
            .controls()
            .expect("readable")
            .iter()
            .any(|desc| desc.slug.as_str() == "white_balance_temperature" && desc.is_inactive()),
        "the coupling is not back: {:?}",
        restored.values
    );

    // And the record was consumed — atomically, and only after the camera was back.
    assert_eq!(session.pre_snapshot, None);
    let dir = store.session_dir(&session);
    assert_eq!(
        store
            .load_session(&dir)
            .expect("readable")
            .pre_snapshot
            .as_ref(),
        None,
        "the document still offers a snapshot that has already been put back"
    );
    let log: Vec<SessionEvent> = store
        .load_log(&dir)
        .expect("readable")
        .into_iter()
        .map(|entry| entry.event)
        .collect();
    assert!(
        log.iter()
            .any(|event| matches!(event, SessionEvent::SnapshotTaken { .. })),
        "{log:?}"
    );
    assert!(
        log.iter()
            .any(|event| matches!(event, SessionEvent::Restored { unrestored: 0, .. })),
        "{log:?}"
    );

    // The other direction of "recovery restores from the persisted snapshot": with the
    // snapshot consumed, a second attempt has nothing to put back and says so instead of
    // writing to the camera again.
    let before = PersistentCamera::read(&device).writes.len();
    let again = store
        .with_lock(|lock| {
            lifecycle::recover(
                &store,
                lock,
                &mut session,
                &mut camera,
                &[white_balance_pair()],
                now(),
            )
        })
        .expect("nothing to do is not a failure");
    assert_eq!(again, Recovery::NothingPersisted);
    assert_eq!(
        PersistentCamera::read(&device).writes.len(),
        before,
        "a session with no snapshot wrote to the camera anyway"
    );
}

// ------------------------------------------------------------------ the child, as a process

fn spawn_child(state_root: &Utf8Path, device: &Utf8Path) -> Child {
    let binary = std::env::current_exe().expect("this test binary has a path");
    Command::new(binary)
        // libtest's own selection, so the child runs exactly the sweep half. A name that
        // matched nothing would exit 0 with no output, and `wait_for_ready` fails loudly
        // on that rather than blocking.
        .args(["--exact", CHILD_TEST, "--nocapture", "--test-threads=1"])
        .env(STATE_ENV, state_root.as_str())
        .env(DEVICE_ENV, device.as_str())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("the test binary is executable")
}

/// Block until the child says its write is on the device.
///
/// Reading a pipe, not sleeping on a guess: the announcement is the observable event, and
/// end-of-file without it means the child died or ran the wrong test — reported with
/// everything it printed, because a crash suite that hangs is worse than one that fails.
fn wait_for_ready(child: &mut Child) {
    let stdout = child.stdout.take().expect("stdout was piped");
    let mut transcript = Vec::new();
    for line in BufReader::new(stdout).lines() {
        let Ok(line) = line else { break };
        if line.contains(READY) {
            return;
        }
        transcript.push(line);
    }
    panic!(
        "the child ended without announcing its write; it printed:\n{}",
        transcript.join("\n")
    );
}
