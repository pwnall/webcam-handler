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

use std::sync::{Arc, Mutex};
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
    }

    /// Script several faults, each to fire once.
    pub fn queue_faults(&self, faults: &[Fault]) {
        let mut queue = lock(&self.faults);
        for &fault in faults {
            queue.queue(fault);
        }
    }

    /// Script `fault` to fire until [`FakeBackend::release_fault`].
    ///
    /// The menu's conditions — a sensor that never settles — are held rather than queued,
    /// because "once" is the wrong duration for a condition.
    pub fn hold_fault(&self, fault: Fault) {
        lock(&self.faults).hold(fault);
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
            return Err(Error::Busy {
                path: lock(state).capture_path(),
                holders: Vec::new(),
            });
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
            node,
        }))
    }
}

/// The hotplug seam: it yields what the fault menu was told to yield, and otherwise says
/// the deadline arrived.
#[derive(Debug)]
struct FakeWatch {
    faults: Arc<Mutex<FaultQueue>>,
    /// The node the scripted events name.
    node: Utf8PathBuf,
}

impl HotplugWatch for FakeWatch {
    fn next_event(&mut self, _deadline: Instant) -> Result<Option<HotplugEvent>> {
        // Returns immediately rather than waiting out the deadline: a fake that slept
        // would be scheduling a flake (N3 bans `thread::sleep` for exactly this), and the
        // caller learns the same thing either way — `Ok(None)` means "nothing happened",
        // which is an answer and not an error (E3).
        if take_fault(&self.faults, Fault::HotplugAdd) {
            return Ok(Some(HotplugEvent::Added {
                path: self.node.clone(),
            }));
        }
        if take_fault(&self.faults, Fault::HotplugRemove) {
            return Ok(Some(HotplugEvent::Removed {
                path: self.node.clone(),
            }));
        }
        Ok(None)
    }
}
