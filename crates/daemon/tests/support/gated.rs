//! A backend that stops a sweep *inside* a control write, and holds it there.
//!
//! Shared by the two binaries that need a sweep to be provably mid-flight —
//! `calibrate_verbs.rs`, which interleaves a queue edit with it (note **N47**), and
//! `subscriptions.rs`, which drops a subscriber's connection during it (docs/7 P4e-i's
//! "disconnect-mid-sweep semantics — the sweep continues, the subscription is reaped").
//! Included by path so that the three binaries which do not need it do not compile it;
//! `support/mod.rs` states what adding an includer costs (note **N49**), and the answer
//! here is that both of them use every item below.
//!
//! **It is not a sleep and not a duration.** The hold ends when another thread hands over a
//! token, and the announcement on the way in is what tells that thread the device is
//! provably occupied — which is what turns "the sweep is inside a write" from a guess about
//! scheduling into an observation. `daemon::server`'s own `Announcing` decorator states the
//! same argument for idle closes.
//!
//! It gates `Camera::set` rather than `next_frame` because that is where a *sweep* is when
//! its session document is half-written: `engine::lifecycle::sweep_write` has already
//! persisted the pre-sweep snapshot and is inside the write it took the snapshot for.

use std::sync::Arc;

use schema::backend::{Camera, CameraBackend};
use schema::camera::CameraId;
use schema::control::{ControlDesc, ControlValue};

/// A gate the test opens one write at a time.
///
/// Armed rather than always-on, and that is not a convenience: `calibrate_start`'s probe
/// *writes* to the camera (D3's toggle-and-restore), so a decorator that held every write
/// from the moment the camera opened would stop the setup rather than the sweep. The flag
/// is raised after the session is queued and lowered once the interleaving under test has
/// happened, so what is held is exactly the sweep.
#[derive(Debug)]
pub(crate) struct Gate {
    pub(crate) armed: std::sync::atomic::AtomicBool,
    /// Says a write has begun and is waiting. Buffered by one, because the actor is one
    /// thread and can only be inside one write.
    pub(crate) entered: std::sync::mpsc::SyncSender<()>,
    /// One token lets one write through. Buffered, so the test's send does not itself have
    /// to wait for the device.
    pub(crate) release: std::sync::Mutex<std::sync::mpsc::Receiver<()>>,
}

/// A backend whose control writes pass through a [`Gate`].
///
/// A decorator over the real fake, in `daemon::server`'s `Announcing` tradition: blocking on
/// command is this test's synchronisation, not a capability any device has, and a
/// `FakeBackend` that could be told to block would be claiming something no replayed profile
/// does — which AGENTS calls a bug in the fake.
///
/// It gates `Camera::set` rather than `next_frame` because that is where a *sweep* is when
/// its session document is half-written: `engine::lifecycle::sweep_write` has already
/// persisted the pre-sweep snapshot and is inside the write it took the snapshot for.
#[derive(Debug)]
pub(crate) struct Blocking {
    pub(crate) inner: Arc<fake::FakeBackend>,
    pub(crate) gate: Arc<Gate>,
}

/// One open camera, forwarding everything and asking the gate before each write.
#[derive(Debug)]
pub(crate) struct Held {
    camera: Box<dyn Camera>,
    gate: Arc<Gate>,
}

/// A poisoned lock here means a test thread panicked holding a channel, which is not a
/// reason to replace a useful failure with a confusing one.
pub(crate) fn lock<T>(mutex: &std::sync::Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl CameraBackend for Blocking {
    fn kind(&self) -> schema::backend::BackendKind {
        self.inner.kind()
    }

    fn enumerate(&self) -> schema::Result<Vec<schema::camera::CameraInfo>> {
        self.inner.enumerate()
    }

    fn open(&self, id: &CameraId) -> schema::Result<Box<dyn Camera>> {
        Ok(Box::new(Held {
            camera: self.inner.open(id)?,
            gate: Arc::clone(&self.gate),
        }))
    }

    fn watch(&self) -> schema::Result<Box<dyn schema::backend::HotplugWatch>> {
        self.inner.watch()
    }

    fn diagnose(&self) -> Vec<schema::report::ListHint> {
        self.inner.diagnose()
    }
}

impl Camera for Held {
    fn info(&self) -> &schema::camera::CameraInfo {
        self.camera.info()
    }

    fn formats(&self) -> schema::Result<Vec<schema::camera::FormatInfo>> {
        self.camera.formats()
    }

    fn controls(&self) -> schema::Result<Vec<ControlDesc>> {
        self.camera.controls()
    }

    fn get(&mut self, id: schema::control::ControlId) -> schema::Result<ControlValue> {
        self.camera.get(id)
    }

    fn set(
        &mut self,
        id: schema::control::ControlId,
        value: ControlValue,
    ) -> schema::Result<schema::control::Applied> {
        if self.gate.armed.load(std::sync::atomic::Ordering::Acquire) {
            // Announce first, then wait: the test's next line depends on this camera being
            // *inside* the write it is about to reason about.
            let _ = self.gate.entered.send(());
            // Ends when the test hands over a token, never when a duration passes.
            let _ = lock(&self.gate.release).recv();
        }
        self.camera.set(id, value)
    }

    fn start_stream(
        &mut self,
        request: &schema::capture::StreamRequest,
    ) -> schema::Result<schema::capture::NegotiatedStream> {
        self.camera.start_stream(request)
    }

    fn streaming(&self) -> Option<schema::capture::NegotiatedStream> {
        self.camera.streaming()
    }

    fn next_frame(
        &mut self,
        deadline: std::time::Instant,
    ) -> schema::Result<schema::capture::Frame> {
        self.camera.next_frame(deadline)
    }

    fn stop_stream(&mut self) -> schema::Result<()> {
        self.camera.stop_stream()
    }
}
