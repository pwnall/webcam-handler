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
pub use fault::Fault;

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
    /// Raised whenever a fault is scripted, so a seam that is *waiting out a caller's
    /// deadline* wakes the instant a test speaks rather than when the deadline arrives.
    ///
    /// Guarded by `faults`, which is the mutex every waiter holds. It exists for exactly
    /// one seam — [`FakeWatch::next_event`], the only one in this fake whose contract is
    /// "block until an event or the deadline" — and it is what lets that contract be
    /// honoured without a `sleep`: the wait ends when another thread says so.
    scripted: Arc<Condvar>,
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
            scripted: Arc::new(Condvar::new()),
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
        self.scripted.notify_all();
    }

    /// Script several faults, each to fire once.
    pub fn queue_faults(&self, faults: &[Fault]) {
        {
            let mut queue = lock(&self.faults);
            for &fault in faults {
                queue.queue(fault);
            }
        }
        self.scripted.notify_all();
    }

    /// Script `fault` to fire until [`FakeBackend::release_fault`].
    ///
    /// The menu's conditions — a sensor that never settles — are held rather than queued,
    /// because "once" is the wrong duration for a condition.
    pub fn hold_fault(&self, fault: Fault) {
        lock(&self.faults).hold(fault);
        self.scripted.notify_all();
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
        // Resolved first, so the busy refusal names the node the caller asked for rather
        // than whichever camera happened to be listed first.
        if take_fault(&self.faults, Fault::Busy) {
            return Err(Error::busy(lock(state).capture_path(), Vec::new()));
        }
        Ok(FakeCamera::new(Arc::clone(state), Arc::clone(&self.faults)))
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
        Ok(self
            .cameras
            .iter()
            .map(|state| lock(state).info().clone())
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
            scripted: Arc::clone(&self.scripted),
            node,
        }))
    }
}

/// The hotplug seam: it yields what the fault menu was told to yield, and otherwise waits
/// out the caller's deadline and says the deadline arrived.
#[derive(Debug)]
struct FakeWatch {
    faults: Arc<Mutex<FaultQueue>>,
    scripted: Arc<Condvar>,
    /// The node the scripted events name.
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
        let mut queue = lock(&self.faults);
        loop {
            // Checked before the events, because a watch that has failed has nothing left to
            // yield — see [`Fault::WatchFails`].
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
            // re-read from the queue every time round, so a wake nobody caused costs one
            // turn and never an invented event.
            let (guard, _timed_out) = self
                .scripted
                .wait_timeout(queue, budget)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            queue = guard;
        }
    }
}
