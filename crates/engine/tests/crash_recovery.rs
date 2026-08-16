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
//!
//! ## Two crashes, because §6's story has two halves and only one of them was pinned
//!
//! The suite began with one: a kill between the write and the restore, which is the
//! *camera's* half — the snapshot is on disk, so the next process can put the device back.
//! Its fixture reached [`lifecycle::sweep_write`] directly and never ran a sweep, so the
//! *session's* half was pinned by nothing: no test in this file mentioned
//! [`calibrate::run`], `begin_sweep` or [`ControlStatus::Sweeping`], and the state the crash
//! story is about was a state the crash suite never produced. The G6 review named that as its
//! own smell — a test whose fixture cannot exercise the rule it pins — and note **N139** is
//! what it was hiding: a sweep killed before its first sample left the control in
//! `Sweeping { done: 0 }`, which every shipped verb refuses, **for the life of the state
//! directory**.
//!
//! So the second rung runs a real [`calibrate::run`] in the child and kills it from inside
//! the first sample. That makes the camera a streaming one — a 16×16 synthetic gradient,
//! generated here, never a frame off a device (AGENTS: a frame may contain a person) — and
//! it makes the two rungs share every piece of fixture except where the child stops.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::process::ExitStatusExt;
use std::process::{Child, Command, Stdio};
use std::time::Instant;

use camino::{Utf8Path, Utf8PathBuf};
use engine::calibrate::{self, SweepContext, SweepRequest};
use engine::lifecycle::{self, Recovery, SessionSpec};
use engine::progress::Silent;
use engine::settle::FrozenClock;
use engine::store::{LockProtocol, SessionStore, StoreLock, TempStore, write_json_atomic};
use schema::backend::{BackendKind, Camera};
use schema::camera::{
    CameraFingerprint, CameraId, CameraInfo, DeviceNode, FormatInfo, FrameInterval, FrameSize,
    FrameSizeInfo, NodeKind, PixelFormat,
};
use schema::capture::{Frame, NegotiatedStream, StreamRequest};
use schema::control::{
    Applied, ControlDesc, ControlFlags, ControlId, ControlRange, ControlSlug, ControlType,
    ControlValue, KnownFlag, WriteWarning,
};
use schema::error::{Error, Result};
use schema::limits;
use schema::pairing::{AutomationOff, AutomationPair, Provenance};
use schema::session::{ControlStatus, SessionEvent, SweepSpec};
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

/// The line the second child prints from inside its first sample.
const SWEEP_READY: &str = "wch-crash-child: the sweep is inside its first sample";
/// The test that second child runs, in the same double-duty shape as [`CHILD_TEST`].
const SWEEP_CHILD_TEST: &str =
    "a_sweep_is_sweeping_on_disk_before_the_first_sample_reaches_the_camera";

/// The control the sweeping rung sweeps. Motorless, so §5's `--allow-motion` is not part of
/// this suite's subject, and unpaired, so a sample's write needs no automation switched off.
const SWEEP_CONTROL: &str = "brightness";

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

/// What the child does the first time its sweep asks this camera for a stream.
///
/// The hook is at `start_stream` rather than at the write, because that is the instant the
/// sweep's *durable* state is `Sweeping { done: 0 }` with the pre-sweep snapshot armed and
/// nothing yet recorded — the one moment note **N139** is about, and the moment no test in
/// this file could previously produce.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AtFirstStream {
    /// Carry on. The in-process arm runs its sweep to the end and asserts what the document
    /// said at this instant.
    CarryOn,
    /// Announce on standard output and block on standard input, so the parent kills this
    /// process at a moment it chose rather than one it guessed (this file's header).
    AnnounceAndWait,
}

/// The synthetic frame this camera delivers: a 16×16 grey gradient, generated here.
///
/// Generated rather than loaded, and grey rather than anything a sensor produces, for the
/// reason AGENTS gives once: a frame may contain a person, so no camera frame enters this
/// repository. Sixteen square because the subject is a crash and not an image — the sweep
/// decodes it, scores it and writes it, and none of those three cares how big it is.
const FRAME_EDGE: u32 = 16;

fn synthetic_frame(sequence: u32) -> Frame {
    let edge = FRAME_EDGE as usize;
    let bytes = (0..edge * edge)
        .map(|index| u8::try_from(index % 256).unwrap_or(0))
        .collect();
    Frame {
        bytes,
        pixel_format: PixelFormat::GREY,
        width: FRAME_EDGE,
        height: FRAME_EDGE,
        bytes_per_line: FRAME_EDGE,
        sequence,
        timestamp_us: i64::from(sequence),
    }
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
    /// What to do the first time a stream is asked for.
    at_first_stream: AtFirstStream,
    /// Whether the hook has already fired, so a second sample does not announce again.
    announced: bool,
    /// What `session.json` said about [`SWEEP_CONTROL`] the first time a stream was asked
    /// for — the in-process arm's whole assertion.
    sweeping_at_first_stream: Option<(u32, u32)>,
    /// What the device agreed to, while it is streaming.
    stream: Option<NegotiatedStream>,
    /// How many frames it has delivered, so each one carries its own sequence number.
    delivered: u32,
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
            at_first_stream: AtFirstStream::CarryOn,
            announced: false,
            sweeping_at_first_stream: None,
            stream: None,
            delivered: 0,
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

    /// Announce and block the first time a stream is asked for, rather than carrying on.
    fn halting_at_the_first_stream(mut self) -> PersistentCamera {
        self.at_first_stream = AtFirstStream::AnnounceAndWait;
        self
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

    /// What the session document says about [`SWEEP_CONTROL`] right now, as
    /// `(done, total)` — `None` unless it is mid-sweep.
    ///
    /// Read out of the file rather than out of the caller's `Session`, for the reason the
    /// device file is a file: the claim is about what *survives this process*, and an
    /// in-memory struct would be the sweep agreeing with itself.
    fn sweeping_on_disk(&self) -> Option<(u32, u32)> {
        let path = self.session_file.as_ref()?;
        let bytes = std::fs::read(path.as_std_path()).ok()?;
        let document: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
        let status = document
            .get("controls")?
            .get(SWEEP_CONTROL)?
            .get("status")?
            .clone();
        let parsed: ControlStatus = serde_json::from_value(status).ok()?;
        match parsed {
            ControlStatus::Sweeping { done, total, .. } => Some((done, total)),
            _ => None,
        }
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
        // One size, one interval, one format: the least a sweep needs to negotiate, so that
        // the frame model stays out of the way of the subject. No camera frame ever enters
        // this suite — see [`synthetic_frame`].
        Ok(vec![FormatInfo {
            pixel_format: PixelFormat::GREY,
            description: "8-bit greyscale".to_owned(),
            flags: 0,
            sizes: vec![FrameSizeInfo {
                size: FrameSize::Discrete {
                    width: FRAME_EDGE,
                    height: FRAME_EDGE,
                },
                intervals: vec![FrameInterval::Discrete {
                    numerator: 1,
                    denominator: 30,
                }],
            }],
        }])
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
        // The one moment the second rung is about, recorded before anything else happens:
        // the guarded write is on the device, the snapshot is on disk, and the document says
        // this control is sweeping with nothing recorded.
        if !self.announced {
            self.announced = true;
            self.sweeping_at_first_stream = self.sweeping_on_disk();
            if self.at_first_stream == AtFirstStream::AnnounceAndWait {
                announce_and_wait(SWEEP_READY);
            }
        }

        let interval = FrameInterval::Discrete {
            numerator: 1,
            denominator: 30,
        };
        let negotiated = NegotiatedStream {
            pixel_format: PixelFormat::GREY,
            width: FRAME_EDGE,
            height: FRAME_EDGE,
            bytes_per_line: FRAME_EDGE,
            size_image: FRAME_EDGE * FRAME_EDGE,
            interval,
            adjustments: NegotiatedStream::diff(
                request,
                PixelFormat::GREY,
                FRAME_EDGE,
                FRAME_EDGE,
                interval,
            ),
        };
        self.stream = Some(negotiated.clone());
        Ok(negotiated)
    }

    fn streaming(&self) -> Option<NegotiatedStream> {
        // The device is the authority on itself (AGENTS rule 4), so this answers from what
        // the last `start_stream`/`stop_stream` left behind rather than from a flag a caller
        // set.
        self.stream.clone()
    }

    fn next_frame(&mut self, _deadline: Instant) -> Result<Frame> {
        if self.stream.is_none() {
            return Err(Error::DeviceIo {
                operation: "next_frame".to_owned(),
                errno: None,
                message: "the stream is not running".to_owned(),
            });
        }
        // Endless, so the settle policy's frame count rather than the fixture's patience is
        // what decides when a sample is taken. A camera that ran out would answer this
        // suite's question with `SettleTimeout`, which is a correct answer to a question it
        // is not asking (note N60).
        let frame = synthetic_frame(self.delivered);
        self.delivered = self.delivered.saturating_add(1);
        Ok(frame)
    }

    fn stop_stream(&mut self) -> Result<()> {
        self.stream = None;
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

/// The sweep the second rung runs: two samples of [`SWEEP_CONTROL`], through the real
/// [`calibrate::run`].
///
/// Two values rather than one because the finding is about a sweep that had more to do when
/// it died, and explicit rather than [`SweepSpec::All`] because a hundred and one samples of
/// a synthetic gradient would prove nothing that two do not.
fn sweep_request() -> SweepRequest {
    SweepRequest::new(
        slug(SWEEP_CONTROL),
        SweepSpec::Explicit {
            values: vec![10, 20],
        },
    )
}

/// Run one sweep, exactly as a composition root does, and answer what the camera saw.
///
/// The clock is [`FrozenClock`] rather than a stepped one because the settle deadline is not
/// this suite's subject: a deadline that cannot expire removes `SettleTimeout` from the set
/// of outcomes, so a red run here is about the crash rather than about how busy the machine
/// was (note **N60**). The lock is returned rather than dropped for [`open_and_write`]'s
/// reason.
fn run_one_sweep(plan: &Plan, at_first_stream: AtFirstStream) -> (StoreLock, PersistentCamera) {
    let store = SessionStore::new(plan.state_root.clone());
    let lock = store
        .lock(LockProtocol::HeldForLifetime)
        .expect("nobody else holds the state directory");
    let mut session = match lifecycle::resume(&store, &fingerprint(), TASK).expect("readable") {
        Some(resumed) => resumed,
        None => lifecycle::create(&store, &lock, &spec(), now()).expect("a free slot"),
    };

    let session_file = store.session_dir(&session).join(limits::SESSION_FILE);
    let mut camera = PersistentCamera::open(&plan.device, Some(session_file));
    if at_first_stream == AtFirstStream::AnnounceAndWait {
        camera = camera.halting_at_the_first_stream();
    }

    let clock = FrozenClock;
    let context = SweepContext {
        store: &store,
        lock: &lock,
        clock: &clock,
        progress: &Silent,
        started_at: now(),
    };
    calibrate::run(&context, &mut session, &mut camera, &sweep_request())
        .expect("a willing camera and a writable store");
    (lock, camera)
}

/// Say the line and block until the parent answers or goes away.
///
/// Blocking on a pipe rather than on a clock: no sleep is a synchronisation (rubric Part C),
/// and reading standard input also means a parent that dies without killing us closes the
/// pipe and lets us go instead of leaving a process behind.
fn announce_and_wait(line: &str) {
    let mut out = std::io::stdout();
    writeln!(out, "{line}").expect("the parent is listening");
    out.flush().expect("the parent is listening");
    let mut ignored = String::new();
    let _ = std::io::stdin().read_line(&mut ignored);
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
        announce_and_wait(READY);
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

    let mut child = spawn_child(CHILD_TEST, temp.root(), &device);
    wait_for_ready(&mut child, READY);

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
    assert_eq!(
        again,
        Recovery::NothingPersisted { freed: Vec::new() },
        "a second recovery found something to repair, so the first one did not finish"
    );
    assert_eq!(
        PersistentCamera::read(&device).writes.len(),
        before,
        "a session with no snapshot wrote to the camera anyway"
    );
}

/// The durable half of the same ordering — and, when a parent asks for it by name, the
/// child half of the sweeping crash test.
///
/// The claim: by the time a sweep asks the camera for its first frame, `session.json`
/// already says the control is `Sweeping` with nothing recorded. That is the state note
/// **N139** is about, and it is worth asserting from *inside* the sweep because it is the
/// only window in which it holds — one sample later `done` is 1 and the control has an exit.
#[test]
fn a_sweep_is_sweeping_on_disk_before_the_first_sample_reaches_the_camera() {
    if let Some(plan) = Plan::from_env() {
        let _held = run_one_sweep(&plan, AtFirstStream::AnnounceAndWait);
        return;
    }

    let temp = TempStore::new().expect("a temp dir");
    let device = temp.root().join("device-under-test.json");
    PersistentCamera::plant(&device);
    let plan = Plan {
        state_root: temp.root().to_owned(),
        device,
    };

    let (lock, camera) = run_one_sweep(&plan, AtFirstStream::CarryOn);
    assert_eq!(
        camera.sweeping_at_first_stream,
        Some((0, 2)),
        "the document did not say this control was sweeping with nothing recorded when the \
         first sample reached the camera, so a kill at that instant would leave something \
         other than the state this suite is about"
    );
    drop(lock);
}

#[test]
fn a_sweep_killed_before_its_first_sample_leaves_the_control_sweepable_again() {
    // Note **N139**, and the half of design §6's crash story this suite could not see until
    // its fixture ran a real sweep. A child begins one, announces itself from inside the
    // first sample and is killed there; the repair verb runs; and the question is whether
    // the control the crash touched can be swept at all afterwards.
    let temp = TempStore::new().expect("a temp dir");
    let device = temp.root().join("device-under-test.json");
    PersistentCamera::plant(&device);

    let mut child = spawn_child(SWEEP_CHILD_TEST, temp.root(), &device);
    wait_for_ready(&mut child, SWEEP_READY);
    child.kill().expect("the child is still running");
    let status = child.wait().expect("the child is reapable");
    assert_eq!(
        status.signal(),
        Some(9),
        "the child exited on its own instead of being killed: {status:?}"
    );

    // The state the finding is about, on disk, written by a process that no longer exists.
    // Without this the rest of the test would pass on a session that never swept.
    let store = SessionStore::new(temp.root().to_owned());
    let mut session = lifecycle::resume(&store, &fingerprint(), TASK)
        .expect("the crashed session is readable")
        .expect("a session that never finished is still open");
    let stranded = session
        .controls
        .get(&slug(SWEEP_CONTROL))
        .expect("the sweep reached the document");
    assert!(
        matches!(stranded.status, ControlStatus::Sweeping { done: 0, .. }),
        "the child did not leave the control mid-sweep: {:?}",
        stranded.status
    );
    assert!(
        stranded.samples.is_empty(),
        "the child recorded a sample, so this is not the zero-sample case"
    );

    // The shipped repair verb, run by a process that never saw the sweep — `calibrate
    // restore`'s own function, not a private one this test reached for.
    let session_file = store.session_dir(&session).join(limits::SESSION_FILE);
    let mut camera = PersistentCamera::open(&device, Some(session_file));
    store
        .with_lock(|lock| lifecycle::restore(&store, lock, &mut session, &mut camera, now()))
        .expect("the lock is free and the snapshot is this camera's");

    // The property, and it is deliberately the whole verb rather than the status field: a
    // repair that moved the status without making the control sweepable would be a repair
    // that only looked like one.
    let clock = FrozenClock;
    let outcome = store
        .with_lock(|lock| {
            let context = SweepContext {
                store: &store,
                lock,
                clock: &clock,
                progress: &Silent,
                started_at: now(),
            };
            calibrate::run(&context, &mut session, &mut camera, &sweep_request())
        })
        .expect(
            "a sweep killed before its first sample left the control in a state every verb \
             refuses, and the repair verb did not give it back",
        );
    assert_eq!(outcome.samples.len(), 2);

    // And the history says what happened, for the reader who arrives after the terminal is
    // gone (note N18): a sweep that started, an interruption, and a sweep that finished.
    let log: Vec<SessionEvent> = store
        .load_log(&store.session_dir(&session))
        .expect("readable")
        .into_iter()
        .map(|entry| entry.event)
        .collect();
    assert!(
        log.iter()
            .any(|event| matches!(event, SessionEvent::SweepInterrupted { taken: 0, .. })),
        "nothing on disk says the first sweep was abandoned: {log:?}"
    );
    assert!(
        log.iter()
            .any(|event| matches!(event, SessionEvent::SweepFinished { samples: 2, .. })),
        "{log:?}"
    );
}

// ------------------------------------------------------------------ the child, as a process

fn spawn_child(test: &str, state_root: &Utf8Path, device: &Utf8Path) -> Child {
    let binary = std::env::current_exe().expect("this test binary has a path");
    Command::new(binary)
        // libtest's own selection, so the child runs exactly the half the parent named. A
        // name that matched nothing would exit 0 with no output, and `wait_for_ready` fails
        // loudly on that rather than blocking.
        .args(["--exact", test, "--nocapture", "--test-threads=1"])
        .env(STATE_ENV, state_root.as_str())
        .env(DEVICE_ENV, device.as_str())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("the test binary is executable")
}

/// Block until the child says it has reached the moment the parent is waiting for.
///
/// Reading a pipe, not sleeping on a guess: the announcement is the observable event, and
/// end-of-file without it means the child died or ran the wrong test — reported with
/// everything it printed, because a crash suite that hangs is worse than one that fails.
fn wait_for_ready(child: &mut Child, ready: &str) {
    let stdout = child.stdout.take().expect("stdout was piped");
    let mut transcript = Vec::new();
    for line in BufReader::new(stdout).lines() {
        let Ok(line) = line else { break };
        if line.contains(ready) {
            return;
        }
        transcript.push(line);
    }
    panic!(
        "the child ended without announcing its write; it printed:\n{}",
        transcript.join("\n")
    );
}
