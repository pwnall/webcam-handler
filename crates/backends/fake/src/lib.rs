//! The fake camera backend.
//!
//! Replays captured device profiles (T3), scripts the fault menu, and synthesizes frames
//! whose content responds to control values. A capability no real device exhibits is a bug
//! in this crate, not a feature of it (E5).
//!
//! ## What "replay" means here
//!
//! A [`schema::DeviceProfile`] is a JSON capture of everything a backend could enumerate
//! about one camera: identity, nodes, formats, the full control set with its menus,
//! ranges and flags, the automation pairs a probe *measured* on it, and the control
//! values it held at capture time. [`FakeBackend`] turns that document back into a
//! device:
//!
//! - `enumerate` hands out the profile's identity, with two deliberate rewrites — a fresh
//!   [`schema::CameraId`] assigned by D1's collision rules, and `backend: Fake`, so a
//!   fake-backend run can never be mistaken for a hardware one.
//! - `open` gives a [`FakeCamera`] whose control graph clamps \[PF:6\], aligns to step,
//!   refuses menu holes \[PF:2\], refuses read-only controls \[PF:12\], couples INACTIVE
//!   flags live in both directions \[PF:3\], and replays out-of-range currents and defaults
//!   without correcting them \[PF:4, PF:5\].
//! - Streaming synthesizes frames whose luma follows `brightness` and whose sharpness
//!   peaks at the focus control's declared default (see [`frames`]).
//!
//! ## Resemblance is the constraint (E5)
//!
//! Every behavioural claim above is asserted *against the profile it replays*, not
//! against a hand-written expectation: if the profile says a control's range stops at
//! 10000, the clamp stops at 10000; if the profile records `white_balance_automatic` as a
//! measured pair, toggling it flips exactly the partner's INACTIVE bit and nothing else.
//! The tests that hold that line live in `tests/resemblance.rs`, and the conformance
//! battery — which is backend-agnostic and knows nothing about this crate — runs in
//! `tests/battery.rs` from P0 (design G0).
#![forbid(unsafe_code)]
// docs/9's "device/request-driven paths" lint set. Every path in this crate answers a
// request or replays device data, so the whole crate is inside it. `not(test)` because a
// test asserting an invariant with `.expect("literal fixture")` is stating a
// precondition, not risking a device.
#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::as_conversions
    )
)]

mod camera;
mod fault;

pub mod frames;

use std::sync::{Arc, Condvar, Mutex};
use std::time::Instant;

use camino::Utf8PathBuf;
use schema::backend::{BackendKind, Camera, CameraBackend, HotplugEvent, HotplugWatch};
use schema::camera::{CameraId, CameraInfo, assign_ids};
use schema::error::{Error, Result};
use schema::limits;
use schema::profile::DeviceProfile;

pub use camera::FakeCamera;
pub use fault::{FRAME_GAP_FRAMES, Fault};

use camera::{CameraState, lock, take_fault};
use fault::FaultQueue;

/// A backend that replays captured device profiles.
#[derive(Debug)]
pub struct FakeBackend {
    /// One entry per profile. Shared with every [`FakeCamera`] opened from it, because
    /// two handles onto one camera are two views of one device.
    cameras: Vec<Arc<Mutex<CameraState>>>,
    /// Shared with the cameras and the watch, so a fault scripted before `open` still
    /// fires after it.
    faults: Arc<Mutex<FaultQueue>>,
    /// The machine the cameras sit on: what it has announced, and the wait a watch parks
    /// on. Shared with every camera, because a device leaving is something the *machine*
    /// says and a camera is where this double learns of it.
    machine: Arc<Machine>,
}

/// The machine underneath the replayed cameras (design D19).
///
/// It holds the hotplug events the machine has produced and the wait a watch parks on.
/// **Events and faults are deliberately two queues**: a fault is a thing a test scripted at
/// a seam, and `no_fault_fires_unless_it_was_scripted` is a claim about that queue, so a
/// removal the *fake itself* produces when a camera vanishes may not arrive by pushing a
/// [`Fault::HotplugRemove`] nobody scripted. It is also the wrong shape — a scripted
/// `HotplugRemove` names whichever node `watch` picked, and a camera leaving names its own.
#[derive(Debug, Default)]
pub(crate) struct Machine {
    /// Everything the machine has announced, oldest first, and never dropped.
    ///
    /// **A log with a per-watch cursor rather than a queue anybody may drain**, because that
    /// is the shape the real watch has and a queue is not (note **N301**). A
    /// `v4l2::hotplug::Tracker` is *primed* from the node tree when it opens — its own doc
    /// says "a watch on a machine that already has ten cameras does not announce ten
    /// arrivals" — so a node that was already absent when a subscriber arrived is never
    /// announced to it, and two watches each see everything that happened after they opened.
    /// A shared queue gets both of those wrong in opposite directions: it replays a departure
    /// to a watch that opened after it, which is a fake capability no real stack exhibits
    /// \[PF:17, note **N136**\], and it lets the first watch to read consume the event the
    /// second one was waiting for.
    ///
    /// Nothing trims it, and nothing needs to: the only producers are a camera vanishing and
    /// a camera coming back, both of which a test drives one at a time.
    announced: Mutex<Vec<HotplugEvent>>,
    /// Raised whenever a fault is scripted or the machine announces something, so a seam
    /// that is *waiting out a caller's deadline* wakes the instant something happens rather
    /// than when the deadline arrives.
    ///
    /// **Guarded by `announced`, and every notifier takes that mutex before it signals —
    /// including the ones whose news is on the other queue** ([`Machine::stirred`]). A
    /// notifier that signalled without it could land in the window between
    /// [`FakeWatch::next_event`] releasing the fault queue and parking on this condvar, and a
    /// wake that lands there is lost: the watch then sleeps out its caller's whole deadline
    /// with the answer already sitting in the queue (note **N301**). It exists for exactly
    /// one seam — `next_event`, the only one in this fake whose contract is "block until an
    /// event or the deadline" — and it is what lets that contract be honoured without a
    /// `sleep`: the wait ends when another thread says so.
    woken: Condvar,
}

/// Where a camera that vanished comes back (design D19).
///
/// The address is the caller's to state rather than this backend's to invent, because the
/// caller is standing in for the thing that decides it: the partner rig picks the vhci port
/// it re-attaches on, and a fake that incremented a port number would be making up a fact
/// about a machine it is not on. Every field here is an *identity* field in D15's sense —
/// where the device is — and nothing that says what the device is appears in it, which is
/// what makes "the same device at a different address" a claim this type can even express.
#[derive(Debug, Clone)]
pub enum Reattachment {
    /// Back in the socket it came out of.
    ///
    /// The commonest replug there is — somebody put the plug back — and the one where D19's
    /// "different address" clause has nothing to say: the fingerprint comes back identical,
    /// so a session store keyed by [`schema::camera::CameraFingerprint::slug`] finds its own
    /// directory again and a pre-sweep snapshot is restorable onto the device it was taken
    /// from. `/dev/videoN` may still have moved on a real machine \[PF:22\]; this double
    /// keeps the numbers, because the numbers are the one thing D15 already says nothing
    /// about.
    WhereItWas,
    /// At another address on the same machine.
    ///
    /// What the partner rig arranges when it re-attaches on a different vhci port, and the
    /// case D19's last sentence is about.
    At {
        /// The USB interface path the device now sits on —
        /// [`schema::camera::CameraFingerprint::bus_path`], spelled as the kernel spells it
        /// (`3-4:1.0`).
        bus_path: String,
        /// The `QUERYCAP` bus info string at the new address (`usb-0000:00:14.0-4`).
        bus_info: String,
        /// The number of the group's first `/dev/videoN`; the rest of the group follows it
        /// in enumeration order, which is how a driver hands node numbers to one device's
        /// interfaces.
        first_node: u32,
    },
}

impl Machine {
    /// Record what the machine did, and wake whoever is watching for it.
    ///
    /// Every event of one act goes on under one lock, because a camera leaving is one act
    /// with one removal per node and a watch that read half of it would be holding a tree
    /// nothing ever had — which is also how `v4l2::hotplug::Tracker::rescan` queues a
    /// reading's whole difference before anybody pops it.
    pub(crate) fn announce(&self, events: Vec<HotplugEvent>) {
        let mut announced = lock(&self.announced);
        announced.extend(events);
        drop(announced);
        self.woken.notify_all();
    }

    /// Wake a parked watch because a test has spoken.
    ///
    /// The guard is taken and dropped without reading anything, and that is the whole of what
    /// this line does: a waiter holds `announced` from before it checks the fault queue until
    /// `wait_timeout` atomically releases it, so acquiring it here is what makes "the watch is
    /// parked, or it has not looked yet" the only two states a notifier can find (note
    /// **N301**). Without it a fault scripted in that window is neither seen nor signalled and
    /// the watch sleeps to its deadline.
    fn stirred(&self) {
        drop(lock(&self.announced));
        self.woken.notify_all();
    }

    /// How much the machine has said, which is where a watch opening now starts reading.
    fn announced_so_far(&self) -> usize {
        lock(&self.announced).len()
    }
}

impl FakeBackend {
    /// Replay these profiles, in this order.
    ///
    /// Camera ids are re-derived from the card names through D1's rules rather than taken
    /// from the documents, so replaying the same profile twice produces two distinct
    /// cameras instead of one id that means both.
    ///
    /// # Errors
    ///
    /// [`Error::SchemaVersionForeign`] for a profile this build does not read. Refusing
    /// up front beats replaying half a document whose meaning has changed (D9's rule,
    /// applied to T3).
    pub fn new(profiles: Vec<DeviceProfile>) -> Result<FakeBackend> {
        for profile in &profiles {
            if !profile.version_is_supported() {
                return Err(Error::SchemaVersionForeign {
                    found: profile.schema_version,
                    supported: limits::PROFILE_SCHEMA_VERSION,
                });
            }
        }

        let cards: Vec<String> = profiles
            .iter()
            .map(|profile| profile.invariant.info.card.clone())
            .collect();
        let ids = assign_ids(&cards);
        let cameras = profiles
            .into_iter()
            .zip(ids)
            .map(|(profile, id)| Arc::new(Mutex::new(CameraState::from_profile(profile, id))))
            .collect();

        Ok(FakeBackend {
            cameras,
            faults: Arc::new(Mutex::new(FaultQueue::default())),
            machine: Arc::new(Machine::default()),
        })
    }

    /// Replay a single profile.
    ///
    /// # Errors
    ///
    /// As [`FakeBackend::new`].
    pub fn from_profile(profile: DeviceProfile) -> Result<FakeBackend> {
        FakeBackend::new(vec![profile])
    }

    /// Script `fault` to fire once.
    pub fn queue_fault(&self, fault: Fault) {
        lock(&self.faults).queue(fault);
        // Told rather than discovered: a watch parked on its caller's deadline is woken
        // here, so scripting a hotplug event is an *event* for the seam waiting on it and
        // a test never has to wait out a duration to see one.
        self.machine.stirred();
    }

    /// Script several faults, each to fire once.
    pub fn queue_faults(&self, faults: &[Fault]) {
        {
            let mut queue = lock(&self.faults);
            for &fault in faults {
                queue.queue(fault);
            }
        }
        self.machine.stirred();
    }

    /// Script `fault` to fire until [`FakeBackend::release_fault`].
    ///
    /// The menu's conditions — a sensor that never settles — are held rather than queued,
    /// because "once" is the wrong duration for a condition.
    pub fn hold_fault(&self, fault: Fault) {
        lock(&self.faults).hold(fault);
        self.machine.stirred();
    }

    /// Stop a held fault.
    pub fn release_fault(&self, fault: Fault) {
        lock(&self.faults).release(fault);
    }

    /// The one-shot faults still waiting to fire, in order.
    #[must_use]
    pub fn pending_faults(&self) -> Vec<Fault> {
        lock(&self.faults).pending()
    }

    /// The faults that fire until released.
    #[must_use]
    pub fn held_faults(&self) -> Vec<Fault> {
        lock(&self.faults).held()
    }

    /// How many streams have been started across every camera this backend replays.
    ///
    /// Cumulative, and asked of the *backend* rather than of a handle, because a caller
    /// holding a `Box<dyn Camera>` cannot ask a `FakeCamera` anything (T2 hides exactly
    /// that) and two handles onto one camera are two views of one device.
    ///
    /// It exists so a test can hold an operation to how many times it started the sensor.
    /// "One capture per sample" is a real property of a calibration sweep — a second
    /// capture is a second moment and doubles a twenty-minute sweep — and on a
    /// deterministic synthesizer two captures of one scene are byte-identical, so nothing
    /// in the *frames* can tell the two implementations apart. This can.
    #[must_use]
    pub fn streams_started(&self) -> u64 {
        self.cameras
            .iter()
            .map(|state| lock(state).streams_started())
            .sum()
    }

    /// How many handles have been opened across every camera this backend replays.
    ///
    /// The same shape and the same argument as [`FakeBackend::streams_started`], for D12's
    /// claim instead of D8's: the daemon "never opens a camera until first use and closes
    /// on idle", and a caller holding a `Box<dyn Camera>` cannot ask a [`FakeCamera`]
    /// anything. This can — and it is what makes "nothing opened" a fact rather than a
    /// thing the actor's own bookkeeping says about itself.
    #[must_use]
    pub fn opens(&self) -> u64 {
        self.cameras.iter().map(|state| lock(state).opens()).sum()
    }

    /// How many of those handles have gone away, which on a real device is the descriptor
    /// closing.
    ///
    /// The counter that makes an idle close assertable: "the actor decided to close" is
    /// bookkeeping, and this is the descriptor.
    #[must_use]
    pub fn closes(&self) -> u64 {
        self.cameras.iter().map(|state| lock(state).closes()).sum()
    }

    /// The profiles this backend replays, in enumeration order.
    ///
    /// The resemblance tests read the expectation from here: the fake's claims are
    /// checked against the document, never against a second copy of the document written
    /// out by hand (E5).
    #[must_use]
    pub fn profiles(&self) -> Vec<DeviceProfile> {
        self.cameras
            .iter()
            .map(|state| lock(state).profile().clone())
            .collect()
    }

    /// Open a camera as the concrete [`FakeCamera`] rather than as a trait object.
    ///
    /// [`CameraBackend::open`] hands out `Box<dyn Camera>`, which is what the engine
    /// wants and what deliberately hides everything this backend knows that a real one
    /// does not. Tests need the other side of that: `FakeCamera::focus_optimum` is a
    /// statement about the *model*, and a resemblance test has to be able to ask for it.
    ///
    /// # Errors
    ///
    /// As [`CameraBackend::open`].
    pub fn open_fake(&self, id: &CameraId) -> Result<FakeCamera> {
        let state = self.find(id).ok_or_else(|| Error::CameraUnknown {
            requested: id.to_string(),
        })?;
        // **A camera that vanished is an id this backend's listing does not name, and that is
        // the answer both backends give** (design D19; notes **N300**, **N301**).
        // `V4l2Backend::open` resolves through its own `enumerate()` and answers
        // `CameraUnknown` for an id the listing does not carry — which a device that left is,
        // because D19's fourth sentence says the listing stops naming it — and this backend
        // said `DeviceGone` for the same event until 2026-08-20, so one machine event had two
        // D13 kinds, two wire codes, two exit codes and two different things an unattended
        // agent does next (docs/11 H1's shape). The real backend cannot tell "this id was
        // never here" from "this id is gone", because it has no memory across an `enumerate`;
        // this one has, and E5 says a shape no real stack exhibits is a bug in the double
        // rather than a feature of it. The refusal a *handle* makes is the other question and
        // stays `DeviceGone` — an fd whose device left answers `ENODEV`, which is
        // `FakeCamera::still_here`. Checked before the busy fault because a camera the listing
        // does not name cannot be held by anybody.
        if lock(state).is_gone() {
            return Err(Error::CameraUnknown {
                requested: id.to_string(),
            });
        }
        // Resolved first, so the busy refusal names the node the caller asked for rather
        // than whichever camera happened to be listed first.
        if take_fault(&self.faults, Fault::Busy) {
            return Err(Error::busy(lock(state).capture_path(), Vec::new()));
        }
        Ok(FakeCamera::new(
            Arc::clone(state),
            Arc::clone(&self.faults),
            Arc::clone(&self.machine),
        ))
    }

    /// Bring a camera that vanished back, at `at` (design D19).
    ///
    /// **The one sentence of D19's contract that is about the machine rather than about a
    /// seam**, which is why it is a verb here and not a member of the fault menu: a device
    /// returning is not a failure a `Camera` call exhibits, and `Fault::ALL`'s walks ask what
    /// observing each member *looks like* at a seam. What a test says here is what the
    /// partner rig does — re-attach the vhci port — and what it gets back is what D19 promises
    /// a consumer: **a new arrival whose fingerprint says it is the same device at a different
    /// address**, which is D14 and D15's split doing its job.
    ///
    /// What moves and what does not: `fingerprint.bus_path`, `bus_info` and every
    /// `/dev/videoN` in the group take the address the caller named; the card, the driver, the
    /// USB id, the serial, the format tree, the control set and the measured pairs do not
    /// move at all, because none of them is a fact about where the device is plugged in. The
    /// [`schema::CameraId`] does not move either, and that is D1's rule rather than a
    /// shortcut: an id is derived from the card names of everything attached, so a device
    /// whose card name came back unchanged onto a machine whose other cameras did not move
    /// gets the same id — which is exactly why [`schema::CameraInfo::differing_fields`]
    /// excludes it and why the *fingerprint* is what D19 says tells the consumer.
    ///
    /// **What this deliberately does not claim**: what a returning device's control values
    /// read. The knobs come back where the loss left them, which is a stand-in and not a
    /// measurement — a driver re-initialises a device it re-enumerates, and nothing on this
    /// rig has ever measured what that leaves behind. `declared` until a rig that can produce
    /// a real return contributes the transcript (E5, design §2.12).
    ///
    /// # Errors
    ///
    /// [`Error::CameraUnknown`] for an id this backend never replayed, and
    /// [`Error::IllegalTransition`] for a camera that never left: a device that is already
    /// here cannot arrive, and answering a caller's mistake with a silent second arrival
    /// would put an event on the watch that no machine produced.
    pub fn device_returns(&self, id: &CameraId, at: &Reattachment) -> Result<CameraInfo> {
        let state = self.find(id).ok_or_else(|| Error::CameraUnknown {
            requested: id.to_string(),
        })?;
        let info = {
            let mut live = lock(state);
            if !live.is_gone() {
                return Err(Error::IllegalTransition {
                    from: "attached".to_owned(),
                    op: format!("bring {id} back; only a camera that vanished can come back"),
                });
            }
            live.returns(at);
            live.info().clone()
        };
        // Announced last, so a watch woken by it reads a listing that already names the
        // camera: an `Added` a consumer cannot then find would be a shape no machine has.
        //
        // **One arrival per node**, the mirror of the removal and for the same reason: the
        // kernel emits a uevent per interface and `v4l2::hotplug::Tracker::rescan` turns each
        // new path into its own `Added`, so a double that announced one event for a
        // four-node device would be handing a node-level consumer a tree the machine never
        // had (note **N301**). Read off the nodes the camera came back with, in the order it
        // declares them, which is also the order the numbers were handed out.
        self.machine.announce(
            info.nodes
                .iter()
                .map(|node| HotplugEvent::Added {
                    path: node.path.clone(),
                })
                .collect(),
        );
        Ok(info)
    }

    fn find(&self, id: &CameraId) -> Option<&Arc<Mutex<CameraState>>> {
        self.cameras
            .iter()
            .find(|state| lock(state).info().id == *id)
    }
}

impl CameraBackend for FakeBackend {
    fn kind(&self) -> BackendKind {
        BackendKind::Fake
    }

    fn enumerate(&self) -> Result<Vec<CameraInfo>> {
        // A camera that vanished mid-stream is not on this machine any more (design D19), so
        // it is not in the listing either. Filtered rather than removed from `self.cameras`,
        // because the fault is scripted against a backend a test still holds — and because
        // "the device came back" is the next sentence D19 writes, which wants the state to
        // still be here to come back from.
        Ok(self
            .cameras
            .iter()
            .filter_map(|state| {
                let state = lock(state);
                (!state.is_gone()).then(|| state.info().clone())
            })
            .collect())
    }

    fn open(&self, id: &CameraId) -> Result<Box<dyn Camera>> {
        Ok(Box::new(self.open_fake(id)?))
    }

    fn watch(&self) -> Result<Box<dyn HotplugWatch>> {
        // The refusal a host without `NETLINK_KOBJECT_UEVENT` makes, scripted — see
        // [`Fault::WatchUnavailable`]. `DeviceIo` and never `DeviceGone`: the *cameras* are
        // here and enumerate perfectly, and only the watch is missing (E3).
        if take_fault(&self.faults, Fault::WatchUnavailable) {
            return Err(schema::Error::DeviceIo {
                operation: "watch for hotplug events".to_owned(),
                errno: None,
                message: "this host has no hotplug watch to give".to_owned(),
            });
        }
        let node = self
            .cameras
            .first()
            .and_then(|state| {
                lock(state)
                    .info()
                    .capture_node()
                    .map(|node| node.path.clone())
            })
            .unwrap_or_else(|| Utf8PathBuf::from("/dev/video0"));
        Ok(Box::new(FakeWatch {
            faults: Arc::clone(&self.faults),
            // **Primed, exactly as `v4l2::hotplug::Tracker::primed` is** (note **N301**): a
            // watch reads what the machine says from here on and never what it said before
            // this call. A double that replayed a departure to a watch opened afterwards
            // would be telling that subscriber about a machine that had already changed —
            // and `daemon::events` runs its watch thread only while somebody is listening, so
            // "the camera left while nobody was subscribed" is a sequence a real consumer
            // meets.
            seen: self.machine.announced_so_far(),
            machine: Arc::clone(&self.machine),
            node,
        }))
    }
}

/// The hotplug seam: it yields what the fault menu was told to yield, and otherwise waits
/// out the caller's deadline and says the deadline arrived.
#[derive(Debug)]
struct FakeWatch {
    faults: Arc<Mutex<FaultQueue>>,
    /// How far into [`Machine::announced`] this watch has read.
    ///
    /// Its own cursor rather than a shared queue's front, which is what makes this watch
    /// primed at the moment it opened and what keeps two watches from eating each other's
    /// events — see [`CameraBackend::watch`]'s own note.
    seen: usize,
    /// What the machine has announced, and the wait this seam parks on.
    machine: Arc<Machine>,
    /// The node the scripted events name.
    ///
    /// Only the *scripted* ones: [`Fault::HotplugAdd`] and [`Fault::HotplugRemove`] say "a
    /// node appeared" and "a node disappeared" without saying which, so they name whichever
    /// node this backend listed first. An event the machine itself produced — a camera that
    /// vanished mid-stream, a camera that came back — names the node it happened to, and
    /// comes through [`Machine::announced`] instead.
    node: Utf8PathBuf,
}

impl HotplugWatch for FakeWatch {
    /// **It honours the deadline, and that is a correction rather than a feature** (note
    /// N57).
    ///
    /// This used to return immediately whatever deadline it was given, with the argument
    /// that "a fake that slept would be scheduling a flake". The argument was about
    /// `sleep`, and the conclusion was one step too far: `HotplugWatch::next_event`'s
    /// contract is *block until an event or until `deadline`*, and a watch that answers
    /// `Ok(None)` instantly and forever is a watch whose only honest consumer is a caller
    /// that polls on a cadence of its own. P4e-i's daemon is not that caller — it runs one
    /// thread per watch, in a loop, which against the old behaviour was a spin at 100% of a
    /// core. AGENTS reads both ways: a fake capability no real device exhibits is a bug in
    /// the fake, and so is a real behaviour the fake refuses to exhibit.
    ///
    /// **Nothing here sleeps**, and the distinction is the one N3 draws. The wait is a
    /// `Condvar` a scripted fault *ends* — `FakeBackend::queue_fault` notifies — so a test
    /// that scripts an event sees it immediately and never waits out a duration. What is
    /// left is the caller's own deadline, which is a bound the trait declares rather than
    /// synchronisation: `testkit::battery`'s arm passes 50 ms and asserts the answer comes
    /// back inside it, and that assertion was vacuous until now.
    fn next_event(&mut self, deadline: Instant) -> Result<Option<HotplugEvent>> {
        // The machine's queue is the one this seam parks on, so the guard is held across the
        // wait; the fault queue is taken and released inside each turn. That order — never
        // the other one — is what keeps a camera announcing its own departure from meeting a
        // watch that is holding the fault queue it is about to want.
        let mut announced = lock(&self.machine.announced);
        loop {
            {
                let mut queue = lock(&self.faults);
                // Checked before the events, because a watch that has failed has nothing left
                // to yield — see [`Fault::WatchFails`].
                if queue.take(Fault::WatchFails) {
                    return Err(schema::Error::DeviceIo {
                        operation: "read the hotplug watch".to_owned(),
                        errno: None,
                        message: "the watch this backend gave out stopped working".to_owned(),
                    });
                }
                if queue.take(Fault::HotplugAdd) {
                    return Ok(Some(HotplugEvent::Added {
                        path: self.node.clone(),
                    }));
                }
                if queue.take(Fault::HotplugRemove) {
                    return Ok(Some(HotplugEvent::Removed {
                        path: self.node.clone(),
                    }));
                }
            }
            // Then what the machine itself did (design D19): a camera that vanished mid-stream
            // announced its removal, and a camera that came back announced its arrival. After
            // the scripted faults rather than before them, so the menu's stated observables
            // stay exactly what they were and `subscriptions.rs`'s "a failed watch is answered
            // before a queued arrival" keeps its order.
            if let Some(event) = announced.get(self.seen).cloned() {
                self.seen = self.seen.saturating_add(1);
                return Ok(Some(event));
            }
            // A deadline that has already passed is a zero wait rather than a panic, which
            // is what makes "an already-spent deadline answers immediately" a case a test
            // can arrange with `Instant::now()` and no clock at all.
            let budget = deadline.saturating_duration_since(Instant::now());
            if budget.is_zero() {
                // `Ok(None)` means the deadline arrived first — a normal outcome, not an
                // error, so a caller polling on a cadence never has to interpret a timeout
                // as a failure (E3).
                return Ok(None);
            }
            // Spurious wake-ups are why this is a loop rather than one wait: the answer is
            // re-read from both queues every time round, so a wake nobody caused costs one
            // turn and never an invented event.
            let (guard, _timed_out) = self
                .machine
                .woken
                .wait_timeout(announced, budget)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            announced = guard;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc;
    use std::time::Duration;

    use super::{Arc, HotplugEvent, Machine, Utf8PathBuf, lock};

    /// How long "the notifier did not get through" is asserted over.
    ///
    /// An absence needs a bound and this is a generous one: `notify_all` on a condvar nobody
    /// is parked on returns in nanoseconds, so a build that signalled without taking the
    /// mutex crosses this line by six orders of magnitude. It is never *waited out* on a
    /// healthy build — the passing direction blocks on a mutex this thread holds, which
    /// cannot end early — so it is a ceiling on the red arm's patience rather than a sleep.
    const LONG_ENOUGH_TO_BE_AN_ABSENCE: Duration = Duration::from_millis(250);

    /// Every wake of [`Machine::woken`] is signalled while its own mutex is held (note
    /// **N301**).
    ///
    /// [`FakeWatch::next_event`] takes `announced` at the top of its turn and keeps it until
    /// `wait_timeout` atomically hands it back, checking the *fault* queue inside that hold.
    /// So a notifier that signalled without acquiring `announced` could land in the window
    /// between the watch releasing the fault queue and parking on the condvar, and a wake that
    /// lands there is lost — the watch then sleeps out its caller's entire deadline with the
    /// answer already queued. The pairing is what makes that window unreachable, and this arm
    /// is the pairing stated as a claim: while a waiter holds `announced`, a notifier gets no
    /// further than the mutex.
    ///
    /// Driven for both notifiers, because they are two code paths and only one of them
    /// reaches `announced` on its own business: [`Machine::announce`], whose news is on this
    /// queue, and [`Machine::stirred`], whose news is on the fault queue and which therefore
    /// has to take a lock it does not otherwise need.
    #[test]
    fn nothing_wakes_a_parked_watch_without_holding_the_mutex_it_parked_on() {
        for (which, notify) in [
            ("stirred (a fault was scripted)", 0_u8),
            ("announce (a camera left)", 1_u8),
        ] {
            let machine = Arc::new(Machine::default());
            // This thread stands in for a watch that has taken `announced` and is about to
            // park: from here until the guard is dropped, nothing may reach `notify_all`.
            let parked = lock(&machine.announced);

            let arrived = Arc::new(AtomicBool::new(false));
            let (about_to, started) = mpsc::channel();
            let notifier = {
                let machine = Arc::clone(&machine);
                let arrived = Arc::clone(&arrived);
                std::thread::spawn(move || {
                    about_to.send(()).expect("the test is still listening");
                    match notify {
                        0 => machine.stirred(),
                        _ => machine.announce(vec![HotplugEvent::Removed {
                            path: Utf8PathBuf::from("/dev/video0"),
                        }]),
                    }
                    arrived.store(true, Ordering::SeqCst);
                })
            };
            // Blocking and unbounded: the notifier has begun, so what the bound below measures
            // is the notify itself and never this thread's scheduling.
            started.recv().expect("the notifier thread starts");

            let (done, finished) = mpsc::channel();
            let watcher = std::thread::spawn(move || {
                notifier.join().expect("the notifier thread does not panic");
                // A closed channel is what the receiver reads if this test has already given
                // up, which is the passing direction and not a failure of this thread.
                let _ = done.send(());
            });
            assert!(
                finished.recv_timeout(LONG_ENOUGH_TO_BE_AN_ABSENCE).is_err(),
                "{which} signalled the watch's condvar while another thread held the mutex \
                 that condvar is paired with, which is a wake a parked watch can miss"
            );
            assert!(
                !arrived.load(Ordering::SeqCst),
                "{which} got past a held `announced`"
            );

            // And the other direction, without which the arm above passes on a notifier that
            // never ran: releasing the mutex lets it through.
            drop(parked);
            finished
                .recv_timeout(LONG_ENOUGH_TO_BE_AN_ABSENCE)
                .unwrap_or_else(|_| panic!("{which} never completed once the mutex was released"));
            assert!(arrived.load(Ordering::SeqCst), "{which} did not run at all");
            watcher.join().expect("the joining thread does not panic");
        }
    }
}
