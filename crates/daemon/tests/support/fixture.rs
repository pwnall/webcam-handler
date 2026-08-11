//! One running daemon, two replayed cameras, and everything a suite asks it about.
//!
//! Shared for the same reason `support/wire.rs` is, and by the same four binaries —
//! `read_verbs.rs`, `mutating_verbs.rs`, `calibrate_verbs.rs` and `method_surface.rs`: they
//! make the same claims over the same surface from four directions, and a second fixture
//! would let them disagree about what "this camera" and "this control" mean while all of them
//! stayed green. Included by path so each binary compiles only the modules it uses;
//! `support/mod.rs` states what adding a fifth includer costs.
//!
//! Everything here is **derived**. The ambiguous prefix is computed from the two ids rather
//! than written down, the control is taken off the device, and the second camera is the
//! committed profile with two identity fields rewritten — because a literal here is a
//! literal that stops being true the day the id scheme changes, and a fixture that stopped
//! being ambiguous would take its assertion with it silently.

use std::sync::Arc;

use camino::Utf8PathBuf;
use daemon::server::Wchd;
use daemon::uds::{self, SocketDir};
use engine::lifecycle::{self, SessionSpec};
use engine::paths::TempRuntimeDir;
use engine::store::{LockProtocol, SessionStore, StoreLock, TempStore};
use fake::FakeBackend;
use jsonrpsee_server::Methods;
use schema::backend::CameraBackend;
use schema::camera::{CameraId, CameraInfo};
use schema::control::{ControlDesc, ControlSlug};

use crate::wire::Wire;

/// The task every session the fixture opens is recorded under.
///
/// One spelling, because a suite that names a session by task has to name the one the
/// fixture wrote — and D9 keys the slot on (camera fingerprint, task), so a second spelling
/// is a session nothing answers to rather than a compile error.
pub(crate) const SESSION_TASK: &str = "exposure";

/// A daemon over two replayed cameras and a state directory with one session in it.
///
/// Everything the transports need is held here because dropping any of it would take the
/// socket, the lock or the state tree with it — the arrangement a real `wchd` has, which is
/// why this is one value and not six locals.
pub(crate) struct Fixture {
    pub(crate) backend: Arc<FakeBackend>,
    /// The daemon behind both wires, for the questions no wire answers.
    ///
    /// Every suite that includes this module compiles every item in it (note **N49**), so
    /// this field has a reader *here* as well as in `subscriptions.rs`: [`Fixture::replaying`]
    /// asserts the daemon starts with nothing subscribed, which is what makes a count a
    /// suite reads later a difference that suite caused rather than a leftover.
    pub(crate) wchd: Wchd,
    pub(crate) cameras: Vec<CameraInfo>,
    pub(crate) store: SessionStore,
    pub(crate) socket: Utf8PathBuf,
    pub(crate) methods: Methods,
    pub(crate) handle: uds::Serving,
    _lock: Arc<StoreLock>,
    _state: TempStore,
    _runtime: TempRuntimeDir,
}

impl Fixture {
    /// Start one. Must be called from inside a tokio runtime.
    pub(crate) fn start() -> Fixture {
        Fixture::start_behind(|fake| Arc::clone(fake) as Arc<dyn CameraBackend>)
    }

    /// The same, with the daemon's backend wrapped in whatever the caller hands back.
    ///
    /// The fixture keeps the `FakeBackend` either way, because the counters
    /// (`opens`, `closes`, `streams_started`) and the fault menu are what a suite observes;
    /// what a decorator adds is a *test-only behaviour* the fake must not grow, in
    /// `daemon::server`'s own `Announcing` tradition — a `FakeBackend` that could be told to
    /// block would be claiming something no replayed profile does, which AGENTS calls a bug
    /// in the fake.
    pub(crate) fn start_behind(
        wrap: impl FnOnce(&Arc<FakeBackend>) -> Arc<dyn CameraBackend>,
    ) -> Fixture {
        // Two cameras, because half of what these suites assert is about *which* camera: an
        // ambiguous prefix needs two candidates, narrowing a session listing by camera needs
        // two fingerprints, and a snapshot refused for belonging to another device needs a
        // second device to have come from.
        Fixture::replaying(
            vec![testkit::fixtures::synthetic_basic(), second_camera()],
            wrap,
        )
    }

    /// The same, over profiles the caller doctored.
    ///
    /// The licence `second_camera` already takes, widened by one step: a suite that needs a
    /// device *shaped* differently — a capture node at a path the test itself can hold open,
    /// say — rewrites a field of the committed fixture rather than committing a second
    /// document nobody would keep current. What it must not do is rewrite a field into a
    /// shape no device exhibits, which is what makes this a constructor here rather than a
    /// second fixture next door.
    ///
    /// [`Fixture::start`] and [`Fixture::start_behind`] go through here rather than beside
    /// it, so there is one arrangement of socket, lock, store and sessions and the suites
    /// cannot disagree about it.
    pub(crate) fn replaying(
        profiles: Vec<schema::profile::DeviceProfile>,
        wrap: impl FnOnce(&Arc<FakeBackend>) -> Arc<dyn CameraBackend>,
    ) -> Fixture {
        let backend = Arc::new(
            FakeBackend::new(profiles).expect("the synthetic profile is this build's version"),
        );
        let cameras = backend
            .enumerate()
            .expect("the fake enumerates what it replays");

        let state = TempStore::new().expect("a state directory");
        let store = SessionStore::new(state.root());

        // The sessions `calibrate status` and `calibrate list` answer about — **one per
        // camera**, so that listing every session and listing one camera's are different
        // answers rather than the same one twice. They are written before the daemon takes
        // the directory: D9 gives a running daemon the lock for its lifetime, so a fixture
        // that wrote afterwards would be contending with the thing it is setting up.
        for index in 0..cameras.len() {
            store
                .with_lock(|lock| {
                    lifecycle::create(
                        &store,
                        lock,
                        &SessionSpec {
                            id: uuid::Uuid::now_v7(),
                            fingerprint: camera(&cameras, index).fingerprint.clone(),
                            task: SESSION_TASK.to_owned(),
                            goal: "a legible frame".to_owned(),
                            criteria: vec!["sharp".to_owned()],
                            tool_version: schema::TOOL_VERSION.to_owned(),
                        },
                        schema::time::Stamp::now(),
                    )
                })
                .expect("a fresh state directory takes the daemonless lock");
        }

        // D9's lifetime lock, taken once and *shared* with the daemon rather than taken
        // twice: `flock` denies a second open file description in this process exactly as
        // it denies another's, so a fixture that handed `Wchd` a lock of its own would be
        // arranging the failure `crate::state`'s header exists to prevent.
        let lock = Arc::new(
            store
                .lock(LockProtocol::HeldForLifetime)
                .expect("nothing else holds a throw-away state directory"),
        );
        let runtime = TempRuntimeDir::new().expect("a runtime directory");
        let dir = SocketDir::prepare(&runtime.env()).expect("a fresh, private socket directory");
        let listener = dir.bind(&lock).expect("nothing is in the way");

        // One daemon, two transports. `mount` consumes a clone; the value behind it is an
        // `Arc`, so both wires reach the same registry, the same backend and the same store
        // — which is what makes a difference between them a difference in transport. The
        // fixture keeps its own handle because the subscription suite asks the daemon what
        // its subscriptions are doing, which is a question no wire answers (note N17: "a
        // *query* on the sink, not a failure of `emit`").
        let wchd = Wchd::new(
            wrap(&backend),
            SessionStore::new(state.root()),
            Arc::clone(&lock),
        );
        let methods: Methods =
            daemon::server::mount(wchd.clone()).expect("the T5 surface mounts once");

        let fixture = Fixture {
            wchd,
            handle: uds::serve(listener, methods.clone()),
            socket: dir.socket_path(),
            methods,
            backend,
            cameras,
            store,
            _lock: lock,
            _state: state,
            _runtime: runtime,
        };
        // The baseline every counter in `subscriptions.rs` is read against: a daemon that
        // has answered nothing has nothing subscribed, has lost nothing, and — D12 — has
        // opened no camera. Asserted rather than assumed, because a fixture that started a
        // watch or a stream of its own would make every later difference a difference from
        // an unknown number.
        let opening = fixture.wchd.subscriptions();
        assert_eq!(opening.live, 0, "a fresh daemon is already subscribed to");
        assert_eq!(opening.hotplug.lost() + opening.calibration.lost(), 0);
        // The third clause, which the sentence above promised and an earlier version of this
        // block did not write (note **N59**). It is the one of the three that a *fixture*
        // change could plausibly break — a warm-up probe, an eager actor — and it would
        // break every `opens()` assertion in `mutating_verbs.rs` by one while every
        // `opens() == 0` refusal assertion stayed satisfied by a different history.
        assert_eq!(
            fixture.backend.opens(),
            0,
            "a fresh daemon has already opened a camera"
        );
        fixture
    }

    /// The two transports, in the order a failure should be read in.
    pub(crate) fn wires(&self) -> [(&'static str, Wire); 2] {
        [
            ("in-memory", Wire::InMemory(self.methods.clone())),
            ("uds", Wire::Uds(self.socket.clone())),
        ]
    }

    /// Which camera, and which control, every call in a suite names.
    pub(crate) fn ask(&self) -> Ask {
        let first = camera(&self.cameras, 0).id.clone();
        let second = camera(&self.cameras, 1).id.clone();

        // An ambiguous prefix, derived rather than written: the fake replays one profile
        // twice, so the second camera's id is the first's plus a suffix, and dropping the
        // last character of the shorter one is a prefix of both that is an exact match for
        // neither. Written out, this would be a literal that stops being ambiguous the
        // day the id scheme changes.
        let shorter = if first.as_str().len() <= second.as_str().len() {
            first.as_str()
        } else {
            second.as_str()
        };
        let mut trimmed = shorter.to_owned();
        trimmed.pop();
        let ambiguous = CameraId::parse(&trimmed).expect("a non-empty prefix");
        assert_ne!(ambiguous, first, "the prefix is an exact match after all");
        assert_ne!(ambiguous, second, "the prefix is an exact match after all");

        // A control this camera really has, taken off the device rather than named — and
        // one a suite may *write* to, because the mutating half asks the same question of
        // the same name. Writable and not a menu: a menu's indices have holes \[PF:2\], so
        // "some other value in range" is not a thing a suite can derive for one.
        let control = self
            .controls()
            .into_iter()
            .find(|desc| desc.is_writable() && !desc.control_type.is_menu())
            .map(|desc| desc.slug)
            .expect("the synthetic profile has a writable scalar control");

        Ask {
            camera: first,
            ambiguous,
            unknown_camera: CameraId::parse("cam:nothing-answers-to-this").expect("a literal id"),
            control,
            unknown_control: ControlSlug::parse("brightnes").expect("a literal slug"),
        }
    }

    /// The first camera's controls, read straight off the backend.
    ///
    /// Behind the daemon's back, deliberately: this is the *device's* answer rather than
    /// the daemon's, which is the strongest evidence a suite has that a verb left a camera
    /// where it found it. `CameraState` is shared across every handle the fake hands out,
    /// so this reads the same device the actor is driving — and it costs one
    /// `FakeBackend::opens()`, which a test also counting descriptors has to account for.
    pub(crate) fn controls(&self) -> Vec<ControlDesc> {
        self.backend
            .open(&camera(&self.cameras, 0).id)
            .expect("the fake opens")
            .controls()
            .expect("the fake answers")
    }
}

/// A second device, made from the committed fixture by renaming it.
///
/// Replaying `synthetic_basic` twice would give two cameras with one **fingerprint** —
/// D9 keys the session tree on the fingerprint, so both would own the same sessions and
/// "one camera's sessions" would be indistinguishable from "every session". Two devices
/// that share a bus path are also not a thing hardware does \[PF:13\].
///
/// A rename rather than a second committed document: what this fixture needs is *a second
/// camera*, and a hand-written profile would be a second fixture to keep current for a
/// property that has nothing to do with the device it describes. The name stays a prefix
/// of nothing and a superset of the first, which is what keeps the ambiguous-prefix arm
/// derivable.
pub(crate) fn second_camera() -> schema::profile::DeviceProfile {
    let mut profile = testkit::fixtures::synthetic_basic();
    profile.invariant.info.card.push_str(" Two");
    profile.invariant.info.fingerprint.card = profile.invariant.info.card.clone();
    profile.invariant.info.fingerprint.bus_path = "3-5:1.0".to_owned();
    profile
}

/// One camera out of the enumeration, by position.
pub(crate) fn camera(cameras: &[CameraInfo], index: usize) -> &CameraInfo {
    cameras
        .get(index)
        .unwrap_or_else(|| panic!("the fixture replays fewer than {} cameras", index + 1))
}

/// The camera and control names every suite asks about.
#[derive(Debug, Clone)]
pub(crate) struct Ask {
    pub(crate) camera: CameraId,
    pub(crate) ambiguous: CameraId,
    pub(crate) unknown_camera: CameraId,
    pub(crate) control: ControlSlug,
    pub(crate) unknown_control: ControlSlug,
}
