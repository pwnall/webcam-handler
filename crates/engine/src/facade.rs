//! The blessed in-process composition, as a thing an embedder can hold (design D18; FR-W5).
//!
//! The engine has been consumable as a library since P0, and the first consumer that was
//! neither the owner nor the owner's agent harness found the actual cost: the blessed call
//! order lived in `webcam-handler-cli`'s private executor, so an embedder read a CLI to learn
//! it and re-verified a five-module assembly at every upgrade. This module is that assembly,
//! promoted — **and `webcam-handler-cli`'s executor is rebuilt on it**, so it cannot drift
//! from what the CLI ships, and the parity gate that compares the two command-line roots byte
//! for byte transitively pins the facade's answers too.
//!
//! It is not a new layer. Every method here is the same three or four engine calls the
//! executor made, in the same order, with the same seams; what changes is that there is one
//! copy of them.
//!
//! ## What it excludes, and why that is a boundary rather than an omission
//!
//! **Calibration and recording lifecycles are not here.** Both are stateful compositions with
//! a store lock, a session mutex and — in the daemon — an actor's thread behind them. An
//! embedder that wants those wants the daemon (the long-lived composition, §2.1) or the CLI.
//! A facade method that half-owned a session would be a second lifecycle home, which is the
//! defect §2.10 exists to prevent, and `webcam-handler-cli` keeps its own assembly for those
//! two verbs precisely because it *is* one of the two blessed compositions.
//!
//! ## The supported-composition contract
//!
//! Versioning honesty first: this workspace is 0.x and is consumed pinned by revision. The
//! contract is that the seams named below move **deliberately** — a break gets a Changes row
//! in the design and a note in `docs/implementation-notes.md` — and that nothing else is
//! promised at all.
//!
//! | May an embedder hold it? | What |
//! |---|---|
//! | **Yes** | every `webcam-handler-schema` type — the vocabulary all four masters share |
//! | **Yes** | `schema::backend`'s `CameraBackend` / `Camera` traits (T1/T2), including writing a backend against them |
//! | **Yes** | this facade |
//! | **Yes** | the engine's pure cores, by name: `pairing`, `settle`, `sweep`, `session`, `resolve` |
//! | **Yes** | `webcam-handler-imaging`'s pure functions — `metrics`, `compare`, `stream_stats`, the decoders and encoders |
//! | **Yes** | `webcam-handler-testkit::battery` — the conformance suite is *for* backend consumers |
//! | **No** | the engine's shell modules — `actor`, `store`, `lifecycle`, `calibrate`, `record`, `preview`, `photo`'s destinations |
//! | **No** | anything in `webcam-handler-daemon`, `webcam-handler-cli-core` or `webcam-handler-web` |
//!
//! ## What a caller still supplies
//!
//! **The wall clock.** The engine reads none by design (§2.10), so every method that stamps
//! an answer takes the [`Stamp`] its composition root minted. The *monotonic* clock a settle
//! deadline runs on is this module's own — it is the blessed choice rather than a parameter,
//! because an embedder passing a wrong one would get a deadline that cannot expire, which is
//! the failure note **N60** is about.
//!
//! **Where a photograph goes.** [`crate::photo::Destination`] is a seam with two real
//! implementations and a scriptable double, and which one is right is a fact about the
//! caller's process rather than about the camera: `webcam-handler-cli` blocks on a path a
//! person typed, and the daemon must not block an actor thread on `open(2)` (note **N51**).

use std::fmt;

use schema::backend::{Camera, CameraBackend, HotplugWatch};
use schema::camera::{CameraId, CameraInfo};
use schema::control::{ControlDesc, ControlSlug, ControlWrite};
use schema::error::Result;
use schema::profile::DeviceProfile;
use schema::report::{CameraDetail, CameraList, ControlReport, DiscoveryReport, WriteReport};
use schema::selector::CameraSelector;
use schema::snapshot::{RestoreReport, Snapshot};
use schema::time::Stamp;

use crate::photo::{Destination, Photograph};
use crate::settle::MonotonicClock;

/// One backend, driven the way this project drives it.
///
/// Cheap to construct and cheap to hold: it owns the backend and nothing else. Cameras are
/// opened per call, exactly as `webcam-handler-cli` opens them, so a `Facade` holds no device
/// between calls and two of them over two backends do not interact.
pub struct Facade {
    backend: Box<dyn CameraBackend>,
}

impl fmt::Debug for Facade {
    /// The backend's own name and nothing else — a `Facade` holds no camera, no frame and no
    /// path between calls, and printing what it drives is the whole of what it has to say.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Facade")
            .field("backend", &self.backend.name())
            .finish()
    }
}

impl Facade {
    /// Drive `backend`.
    #[must_use]
    pub fn new(backend: Box<dyn CameraBackend>) -> Self {
        Facade { backend }
    }

    /// The backend this facade drives, for a caller that needs to ask it something this
    /// surface does not offer.
    ///
    /// A deliberate escape hatch rather than an oversight: T1 is a supported seam, and a
    /// facade that hid it would make an embedder that needs one backend method fork the whole
    /// composition. What it is *not* is a second way to do what this type does — every method
    /// below is here because doing it by hand is the assembly D18 promoted.
    #[must_use]
    pub fn backend(&self) -> &dyn CameraBackend {
        self.backend.as_ref()
    }

    /// Every camera on this machine, and why the list might be empty (D1).
    ///
    /// # Errors
    ///
    /// Whatever the backend refuses enumeration with.
    pub fn list(&self) -> Result<CameraList> {
        crate::resolve::list(self.backend.as_ref())
    }

    /// The camera a selector names, against the live enumeration (D14).
    ///
    /// Enumerating first is what lets an ambiguity name its candidates, which is the
    /// difference between `CameraAmbiguous` being actionable and being a shrug.
    ///
    /// # Errors
    ///
    /// [`schema::Error::CameraUnknown`] when nothing matches, [`schema::Error::CameraAmbiguous`]
    /// when several do, or whatever the backend refuses enumeration with.
    pub fn resolve(&self, requested: &CameraSelector) -> Result<CameraInfo> {
        let cameras = self.backend.enumerate()?;
        crate::resolve::camera(&cameras, requested).cloned()
    }

    /// Resolve a selector and open the camera it names.
    ///
    /// The pair comes back because both halves are wanted: the `CameraInfo` is what an answer
    /// carries, and the `Camera` is what the next call drives. Backends look up by id and
    /// never resolve, which is why resolution happens here and once (§2.10).
    ///
    /// # Errors
    ///
    /// [`Self::resolve`]'s, or whatever the backend refuses the open with —
    /// [`schema::Error::Busy`] when another process holds the device,
    /// [`schema::Error::PermissionDenied`] when the node is there and unopenable.
    pub fn open(&self, requested: &CameraSelector) -> Result<(CameraInfo, Box<dyn Camera>)> {
        let info = self.resolve(requested)?;
        let camera = self.backend.open(&info.id)?;
        Ok((info, camera))
    }

    /// One camera's identity, nodes and format tree.
    ///
    /// # Errors
    ///
    /// [`Self::open`]'s, or whatever the device refuses its format enumeration with.
    pub fn detail(&self, requested: &CameraSelector) -> Result<CameraDetail> {
        let (info, camera) = self.open(requested)?;
        Ok(CameraDetail {
            formats: camera.formats()?,
            info,
        })
    }

    /// One camera's control set, with the automation pairs D3 *declares* for it.
    ///
    /// Nothing is written and nothing is measured — measuring means toggling the device's
    /// automation, which is [`Self::discover_pairs`] and is a different verb on purpose
    /// (note N30).
    ///
    /// # Errors
    ///
    /// [`Self::open`]'s, or whatever the device refuses the control walk with.
    pub fn controls(&self, requested: &CameraSelector) -> Result<ControlReport> {
        let (info, camera) = self.open(requested)?;
        let controls = camera.controls()?;
        Ok(ControlReport {
            pairs: crate::pairing::in_effect(&controls, Vec::new()),
            camera: info.id,
            controls,
        })
    }

    /// Toggle each automation-shaped control and record what it freezes \[PF:3\].
    ///
    /// **This writes to the camera.** It snapshots first and restores after, and the answer
    /// carries what the restore achieved — a caller that ignores `DiscoveryReport::restored`
    /// has been handed a promise rather than a result.
    ///
    /// # Errors
    ///
    /// [`Self::open`]'s, or whatever the device refuses a write or the walk with.
    pub fn discover_pairs(
        &self,
        requested: &CameraSelector,
        now: Stamp,
    ) -> Result<DiscoveryReport> {
        let (_, mut camera) = self.open(requested)?;
        crate::discover::report(camera.as_mut(), now)
    }

    /// One control's full descriptor and current value.
    ///
    /// # Errors
    ///
    /// [`Self::open`]'s, or [`schema::Error::ControlUnknown`] naming the nearest candidates —
    /// the same suggestion list a write's planner produces, so `get` and `set` cannot disagree
    /// about what a near-miss means.
    pub fn get(&self, requested: &CameraSelector, control: &ControlSlug) -> Result<ControlDesc> {
        let (_, camera) = self.open(requested)?;
        crate::pairing::describe(&camera.controls()?, control)
    }

    /// Write controls, with D3's guarded-write planning when `guarded` (the default posture).
    ///
    /// Requested is not applied (E4): every write reads back, and the report carries
    /// `{requested, applied}` per control along with any warning the driver's own clamping
    /// produced \[PF:6\].
    ///
    /// # Errors
    ///
    /// [`Self::open`]'s, [`schema::Error::ControlUnknown`], [`schema::Error::ControlReadOnly`],
    /// [`schema::Error::ControlInactive`] when a guard would be violated, or whatever the
    /// device refused the write with.
    pub fn set(
        &self,
        requested: &CameraSelector,
        writes: &[ControlWrite],
        guarded: bool,
    ) -> Result<WriteReport> {
        let (_, mut camera) = self.open(requested)?;
        crate::write::set_requested(camera.as_mut(), writes, guarded)
    }

    /// Every control's value right now, in the order a restore must put them back (D4).
    ///
    /// # Errors
    ///
    /// [`Self::open`]'s, or whatever the device refuses the control walk with.
    pub fn snapshot(&self, requested: &CameraSelector, now: Stamp) -> Result<Snapshot> {
        let (_, mut camera) = self.open(requested)?;
        crate::snapshot::take_in_effect(camera.as_mut(), now)
    }

    /// Put a snapshot back, automation before manual (D4), and report what that achieved.
    ///
    /// # Errors
    ///
    /// [`Self::open`]'s, or whatever the device refused a write with. A restore that could not
    /// put a control back is reported rather than raised — see [`RestoreReport`], because a
    /// partial restore is a fact a caller has to be told rather than an operation to retry.
    pub fn restore(
        &self,
        requested: &CameraSelector,
        snapshot: &Snapshot,
    ) -> Result<RestoreReport> {
        let (_, mut camera) = self.open(requested)?;
        crate::snapshot::restore_in_effect(camera.as_mut(), snapshot)
    }

    /// Take one photograph: negotiate, settle, capture, stamp, deliver.
    ///
    /// The whole D6 pipeline behind one call, which is the request FR-W5 actually made — the
    /// order of those five steps is the thing an embedder was reverse-engineering.
    ///
    /// `destination` is the seam that decides where the bytes go; see the module doc for why
    /// it is a parameter rather than a choice made here. The monotonic clock the settle
    /// deadline runs on is this module's.
    ///
    /// The take's [`crate::preview::Gap`] — what this photo did to a preview, when there was one
    /// to interrupt — is deliberately dropped here. A facade caller has no preview to
    /// interrupt: this composition opens a camera per call and closes it, so nothing in the
    /// caller's process is streaming the device. The daemon is the composition that keeps the
    /// gap, because it is the one with viewers to tell (note **N83**).
    ///
    /// # Errors
    ///
    /// [`Self::open`]'s, [`schema::Error::FormatUnsupported`] when the camera cannot deliver
    /// what was asked for, [`schema::Error::SettleTimeout`] when the picture never stabilized,
    /// [`schema::Error::DeviceGone`] when the camera left mid-capture, or a storage refusal
    /// from the destination.
    pub fn photo(
        &self,
        requested: &CameraSelector,
        request: &schema::capture::PhotoRequest,
        destination: &mut dyn Destination,
        now: Stamp,
    ) -> Result<Photograph> {
        let (_, mut camera) = self.open(requested)?;
        crate::photo::take(
            camera.as_mut(),
            request,
            destination,
            &MonotonicClock::new(),
            now,
        )
        .outcome
    }

    /// Capture a device profile: everything this backend can enumerate about one camera (T3).
    ///
    /// # Errors
    ///
    /// [`Self::open`]'s, or whatever the device refused an enumeration with.
    pub fn profile(
        &self,
        requested: &CameraSelector,
        capturer: &str,
        now: Stamp,
    ) -> Result<DeviceProfile> {
        let (_, mut camera) = self.open(requested)?;
        crate::profile::capture(camera.as_mut(), &self.context(capturer, now))
    }

    /// Capture a device profile **and** probe its automation pairs empirically \[PF:3\].
    ///
    /// **This writes to the camera**, and the [`crate::discover::Discovery`] beside the
    /// profile says what the probe touched and what it put back. A caller that wants a reading
    /// rather than a measurement wants [`Self::profile`].
    ///
    /// # Errors
    ///
    /// [`Self::open`]'s, or whatever the device refused a write or an enumeration with.
    pub fn profile_probed(
        &self,
        requested: &CameraSelector,
        capturer: &str,
        now: Stamp,
    ) -> Result<(DeviceProfile, crate::discover::Discovery)> {
        let (_, mut camera) = self.open(requested)?;
        crate::profile::capture_probed(camera.as_mut(), &self.context(capturer, now))
    }

    /// Watch for cameras arriving and leaving.
    ///
    /// # Errors
    ///
    /// [`schema::Error::DeviceIo`] on a host that cannot give out a watch at all — which is a
    /// real answer rather than a failure of this machine's cameras (E3), and is why the
    /// refusal is distinct from an empty enumeration.
    pub fn watch(&self) -> Result<Box<dyn HotplugWatch>> {
        self.backend.watch()
    }

    /// Open a camera by an id already resolved.
    ///
    /// For a caller holding a [`CameraId`] out of a previous answer rather than a selector a
    /// user typed — a watcher acting on a hotplug event, say. Selection policy is
    /// [`Self::resolve`]'s and this deliberately performs none: an id is exactly one camera or
    /// it is nothing.
    ///
    /// # Errors
    ///
    /// Whatever the backend refuses the open with.
    pub fn open_id(&self, id: &CameraId) -> Result<Box<dyn Camera>> {
        self.backend.open(id)
    }

    /// The provenance block every captured profile carries.
    ///
    /// One home for the three host facts, so a profile captured through this facade and one
    /// captured over a socket carry the same block.
    fn context(&self, capturer: &str, now: Stamp) -> crate::profile::CaptureContext {
        crate::profile::CaptureContext {
            captured_at: now,
            kernel: crate::profile::kernel_release(),
            tool_version: schema::TOOL_VERSION.to_owned(),
            capturer: capturer.to_owned(),
            backend: self.backend.kind(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facade() -> Facade {
        let profiles = testkit::corpus::load_all()
            .expect("the corpus parses")
            .into_iter()
            .map(|(_path, profile)| profile)
            .collect();
        Facade::new(Box::new(
            fake::FakeBackend::new(profiles).expect("the corpus replays"),
        ))
    }

    fn first(facade: &Facade) -> CameraSelector {
        CameraSelector::Id(
            facade
                .list()
                .expect("lists")
                .cameras
                .first()
                .expect("a camera")
                .id
                .clone(),
        )
    }

    #[test]
    fn the_facade_answers_the_same_listing_the_resolver_does() {
        // The facade is the composition, not a second opinion: `list` is `resolve::list`, so
        // a facade that had grown its own assembly would answer differently here.
        let facade = facade();
        let through = facade.list().expect("lists");
        let direct = crate::resolve::list(facade.backend()).expect("lists");
        assert_eq!(through, direct);
        assert!(!through.cameras.is_empty());
    }

    #[test]
    fn resolving_and_opening_answer_about_the_same_camera() {
        let facade = facade();
        let selector = first(&facade);
        let resolved = facade.resolve(&selector).expect("resolves");
        let (opened, _camera) = facade.open(&selector).expect("opens");
        assert_eq!(resolved, opened);
    }

    #[test]
    fn every_spelling_reaches_the_same_camera_through_the_facade() {
        // D14 through D18: an embedder holds the selector vocabulary, not an id grammar, and
        // this is the arm that says the facade did not narrow it on the way through.
        let facade = facade();
        let info = facade
            .list()
            .expect("lists")
            .cameras
            .into_iter()
            .next()
            .expect("a camera");
        let by_id = CameraSelector::Id(info.id.clone());
        let by_bus = schema::selector::parse(&format!("bus:{}", info.fingerprint.bus_path))
            .expect("a bus path parses");
        assert_eq!(
            facade.resolve(&by_id).expect("resolves").id,
            facade.resolve(&by_bus).expect("resolves").id
        );
    }

    #[test]
    fn a_selector_nothing_answers_to_is_refused_in_the_words_the_composition_uses() {
        // The facade refuses what the composition refuses, in the same words — the claim
        // docs/13 P7d asks for, asserted rather than assumed.
        let facade = facade();
        let selector = schema::selector::parse("bus:nowhere").expect("parses");
        let through = facade.resolve(&selector).expect_err("nothing sits there");
        let cameras = facade.backend().enumerate().expect("enumerates");
        let direct = crate::resolve::camera(&cameras, &selector).expect_err("nothing sits there");
        assert_eq!(through.kind(), direct.kind());
        assert_eq!(through.to_string(), direct.to_string());
    }

    #[test]
    fn the_control_report_is_the_one_the_engine_assembles() {
        let facade = facade();
        let selector = first(&facade);
        let report = facade.controls(&selector).expect("reads controls");
        let (info, camera) = facade.open(&selector).expect("opens");
        let controls = camera.controls().expect("reads controls");
        assert_eq!(report.camera, info.id);
        assert_eq!(report.controls, controls);
        assert_eq!(
            report.pairs,
            crate::pairing::in_effect(&controls, Vec::new())
        );
    }

    #[test]
    fn a_profile_captured_through_the_facade_carries_the_provenance_a_capture_owes() {
        let facade = facade();
        let selector = first(&facade);
        let profile = facade
            .profile(&selector, "the facade's test", Stamp::epoch())
            .expect("captures");
        assert_eq!(profile.provenance.capturer, "the facade's test");
        assert_eq!(profile.provenance.tool_version, schema::TOOL_VERSION);
        assert_eq!(
            profile.provenance.backend,
            schema::backend::BackendKind::Fake,
            "a profile captured from the fake must say so, or it is circular corpus"
        );
    }

    #[test]
    fn the_facade_holds_no_camera_between_calls() {
        // The property that makes it cheap to hold and safe to share: each call opens and
        // closes, so nothing is left claiming the device between them. A facade that kept a
        // camera open would be a second claim on hardware whose whole contract is that one
        // streamer holds it — and the failure would appear at the *second* caller, which is
        // exactly the shape a test has to reach for.
        let facade = facade();
        let selector = first(&facade);
        let (_, first_open) = facade.open(&selector).expect("opens");
        drop(first_open);
        let (_, second_open) = facade.open(&selector).expect("opens again");
        drop(second_open);
        // And a read verb after both, because the interesting failure is a facade that
        // stranded the device rather than one that refused an open outright.
        assert!(
            !facade
                .controls(&selector)
                .expect("reads controls")
                .controls
                .is_empty(),
            "the camera answered nothing after being opened and closed twice"
        );
    }
}
