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
//! **The table below is reconciled against the tree.**
//! `scripts/gates/facade-stability-table-sync.sh` reads it out of this doc comment and holds
//! it to the crates it names in both directions: every module one of those crates declares
//! sits in exactly one row, and every module a row names is one the crate still declares. A
//! row whose *What* cell reads `the whole crate` carries its verdict for every module in that
//! crate. So a module added to the engine stops the gate until somebody has decided which
//! column it belongs in, which is the difference between a contract and a paragraph
//! (notes **N153**, **N158**).
//!
//! | May an embedder hold it? | Where | What | Why |
//! |---|---|---|---|
//! | **Yes** | `webcam-handler-schema` | the whole crate | the vocabulary all four masters share, `CameraBackend` and `Camera` (T1/T2) among them — writing a backend against those traits is a supported thing to do |
//! | **Yes** | `webcam-handler-v4l2`, `webcam-handler-fake` | the whole crate | [`Facade::new`] takes a `Box<dyn CameraBackend>`, so constructing one is the caller's job by construction and no method here could cover it; `V4l2Backend::new` and `FakeBackend::new` are what both composition roots call |
//! | **Yes** | `webcam-handler-engine` | `discover`, `facade`, `pairing`, `photo`, `profile`, `resolve`, `session`, `settle`, `sweep` | the pure cores by name — and every module this facade's own imports and signatures force on a caller, because a headline verb that cannot be called from inside this column would make the column a fiction: [`crate::photo::Destination`] and [`crate::photo::Photograph`] are [`Facade::photo`]'s seam and its answer, [`crate::discover::Discovery`] is half of what [`Facade::profile_probed`] hands back, and [`crate::profile::read`] is the corpus door a `--backend fake` composition root goes through. A module in this row answers for everything on its public surface, which is a rule `photo` broke in three places until 2026-08-21: `photo::take` hands back a `Taken` whose `gap` was a [`crate::preview`] type, it took its camera as a [`crate::actor`] alias, and `photo::from_capture` demanded a [`crate::capture`] one — so holding `photo` meant naming three modules the row below forbids. `Gap` and `OpenCamera` are re-exported as [`crate::photo::Gap`] and [`crate::photo::OpenCamera`], `from_capture` is `pub(crate)`, and claim 6 of `scripts/gates/facade-stability-table-sync.sh` is what reads this row now — the first two were found by review and the third by the reader written for the first two (notes **N324**, **N328**) |
//! | **No** | `webcam-handler-engine` | `actor`, `calibrate`, `capture`, `lifecycle`, `paths`, `preview`, `progress`, `record`, `snapshot`, `store`, `write` | the shell: threads, locks, session state, the state directory and the write path. [`crate::photo::IntoTheSessionTree`] is the destination implementation that belongs to this half in spirit — it writes into D9's session tree, which is `store`'s — and an embedder that wants it wants the daemon or the CLI |
//! | **Yes** | `webcam-handler-imaging` | `avi`, `compare`, `decode`, `encode`, `exif`, `fixtures`, `metrics`, `photo`, `stream_stats`, `video`, `y4m` | pure functions over values, every one of them: bytes and numbers in, bytes and numbers out, no clock and no file |
//! | **Yes** | `webcam-handler-testkit` | `battery`, `corpus`, `fixtures`, `images` | the conformance suite is *for* backend consumers, and a consumer replaying this project's corpus needs the loader and the fixtures beside it |
//! | **No** | `webcam-handler-testkit` | `oracle` | it drives `ffprobe` and `mpv`, which are this repository's own test oracles and not something a consumer inherits |
//! | **No** | `webcam-handler-daemon`, `webcam-handler-cli-core`, `webcam-handler-web` | the whole crate | the long-lived composition, the shared command surface and the browser client are products rather than seams |
//!
//! A crate the table does not name is promised nothing, which is why the one **No** row that
//! answers for whole crates is written out rather than left to that default: it names the
//! crates the design names, and a reader looking for them should find them there rather than
//! infer them from a silence.
//!
//! **`session` sits in the Yes column here and also on `facade-is-the-composition.sh`'s
//! excluded-lifecycle list, and that is not a contradiction: the two lists answer different
//! questions.** This one asks whether an embedder may hold a module; that one names the engine
//! modules `webcam-handler-cli` assembles a *lifecycle* out of, because D18 keeps calibration
//! and recording off this surface. `engine::session` is a pure state machine over values, which is
//! exactly why it is holdable and exactly why the CLI can build a session lifecycle on it.
//! `sweep` used to be on both and the resolution went the other way: the executor has never
//! named `engine::sweep`, so the exemption excused a reach nobody made and it is gone from that
//! list (note **N269**). It stays **Yes** here, because the design names the sweep planner among
//! the pure cores and holding it costs an embedder nothing.
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
//! That is why `photo` is in the **Yes** column: [`Facade::photo`]'s own signature takes a
//! `&mut dyn Destination` and answers a [`crate::photo::Photograph`], so a table that forbade
//! them would forbid calling the headline verb this module exists for — note **N270** records
//! what that cost while nothing reconciled the two. An embedder hands in
//! [`crate::photo::WhereverTheCallerSaid`] or an implementation of its own; what stays out of
//! reach is the pipeline the facade runs around it.

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
    /// to interrupt — is deliberately dropped here, which is why `preview` is in the **No**
    /// column: the type never crosses this boundary. A facade caller has no preview to
    /// interrupt: this composition opens a camera per call and closes it, so nothing in the
    /// caller's process is streaming the device. The daemon is the composition that keeps the
    /// gap, because it is the one with viewers to tell (note **N83**).
    ///
    /// [`crate::photo::Taken`] exists so that a gap cannot go missing in silence, so the drop
    /// is not left resting on this paragraph. The take is assembled by a private
    /// `photo_taken` and this verb is the line that drops the gap, which is what lets
    /// `a_photo_through_the_facade_interrupts_no_preview_and_a_photo_beside_one_does` read the
    /// gap off **this composition's own take** rather than off a pipeline the test assembled
    /// beside it — an expectation taken from a re-run of the subject is the shadow note
    /// **N252** is about, and that arm was one until 2026-08-20 (note **N272**).
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
        self.photo_taken(requested, request, destination, now)?
            .outcome
    }

    /// [`Self::photo`]'s whole composition, with the gap still on it.
    ///
    /// Private on purpose: [`crate::preview::Gap`] is in the stability table's **No** column and
    /// this seam does not change that — no `pub fn` here hands one out. What it buys is that the
    /// claim [`Self::photo`]'s doc comment makes — *this* composition interrupts no preview,
    /// because it opens a camera per call and closes it — is a fact a test can read off the
    /// facade's own take instead of off a second assembly that merely resembles it.
    fn photo_taken(
        &self,
        requested: &CameraSelector,
        request: &schema::capture::PhotoRequest,
        destination: &mut dyn Destination,
        now: Stamp,
    ) -> Result<crate::photo::Taken> {
        let (_, mut camera) = self.open(requested)?;
        Ok(crate::photo::take(
            camera.as_mut(),
            request,
            destination,
            &MonotonicClock::new(),
            now,
        ))
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

    fn profiles() -> Vec<DeviceProfile> {
        testkit::corpus::load_all()
            .expect("the corpus parses")
            .into_iter()
            .map(|(_path, profile)| profile)
            .collect()
    }

    fn facade() -> Facade {
        Facade::new(Box::new(
            fake::FakeBackend::new(profiles()).expect("the corpus replays"),
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
    fn the_watch_is_the_backends_own_and_two_of_them_report_the_same_event() {
        // `Facade::watch` is the one verb §1.3's stated consumer holds — the sibling
        // project's harness waits for a forwarded camera to arrive — and it was asserted
        // nowhere. The claim is every other method's claim: the facade is the composition
        // rather than a second opinion, so a watch taken through it and a watch taken off
        // the backend it drives answer the same scripted event.
        // Two scripted arrivals, one for each watch: the fake's fault queue is shared and
        // each `next_event` takes one. They are queued before the backend is handed over
        // because `Facade` owns it from that point on — which is itself the D18 boundary,
        // and the reason this arm compares two watches rather than reaching for a counter.
        let backend = fake::FakeBackend::new(profiles()).expect("the corpus replays");
        backend.queue_faults(&[fake::Fault::HotplugAdd, fake::Fault::HotplugAdd]);
        let facade = Facade::new(Box::new(backend));

        let mut through = facade.watch().expect("this backend gives out a watch");
        let mut direct = facade
            .backend()
            .watch()
            .expect("this backend gives out a watch");

        // `Instant::now()` is a deadline already spent, so nothing here waits on a clock and
        // no `sleep` stands in for synchronisation: the fake answers a queued event before it
        // looks at the deadline at all, and reaches the deadline only when the queue is
        // empty — which is why an already-spent one is a zero wait rather than a panic.
        let a = through
            .next_event(std::time::Instant::now())
            .expect("the watch is working");
        let b = direct
            .next_event(std::time::Instant::now())
            .expect("the watch is working");
        assert!(
            matches!(&a, Some(schema::backend::HotplugEvent::Added { .. })),
            "the facade's watch reported {a:?} for a scripted arrival"
        );
        assert_eq!(
            a, b,
            "a watch through the facade and a watch off the backend it drives disagreed"
        );
    }

    #[test]
    fn a_host_that_cannot_give_a_watch_is_refused_rather_than_answered_with_no_cameras() {
        // Rule 7, on the one verb whose refusal is easiest to soften: "this host has no
        // hotplug watch to give" is not "no cameras arrived". The fake scripts exactly that
        // host — `Fault::WatchUnavailable`, `DeviceIo` and never `DeviceGone` — and the two
        // assertions below are the two halves the facade's own doc comment claims: the
        // refusal keeps its kind, and the cameras are still all there while it is made.
        //
        // **The second half is asserted against the input that separates the two readings**,
        // not on its own. `Facade::watch` is `self.backend.watch()` over a `&self` that holds
        // no state, so nothing it could be mutated into empties this enumeration: on this host
        // alone, "the cameras are still there" is true by construction and its false branch is
        // unreachable, which is a skip that reads as a pass (notes **N160**, **N231**,
        // **N235**). So a second host is scripted below — one that really does enumerate
        // nothing — and the pair is what carries the claim: each of the two facts this arm
        // holds apart is produced here by its own input (note **N250**).
        let backend = fake::FakeBackend::new(profiles()).expect("the corpus replays");
        backend.queue_fault(fake::Fault::WatchUnavailable);
        let facade = Facade::new(Box::new(backend));

        let refused = facade.watch().expect_err("this host has no watch to give");
        assert!(
            matches!(&refused, schema::Error::DeviceIo { .. }),
            "a host with no watch to give was reported as {refused}"
        );
        assert!(
            !facade.list().expect("lists").cameras.is_empty(),
            "the watch refusal was allowed to read as a machine with no cameras"
        );

        // And the refusal is about the watch rather than about the facade: the next call
        // gets one, which is what makes "the next subscriber starts a fresh watch" reachable
        // through this surface too.
        facade.watch().expect("a watch after the refusal");

        // The separating host: a machine with no cameras at all. It enumerates nothing and
        // still gives out a watch — the opposite pairing from the one above, and the answer to
        // "what would an empty enumeration actually take?". A backend that folded the watch
        // refusal into "no cameras" would have to agree with one of these two hosts and
        // disagree with the other, which is what makes the assertion above about something.
        let empty = Facade::new(Box::new(
            fake::FakeBackend::new(Vec::new()).expect("a host with no cameras is still a host"),
        ));
        assert!(
            empty.list().expect("lists").cameras.is_empty(),
            "a backend built with no profiles enumerated cameras from somewhere"
        );
        empty
            .watch()
            .expect("a machine with no cameras still has a watch to give");
    }

    #[test]
    fn a_photo_through_the_facade_interrupts_no_preview_and_a_photo_beside_one_does() {
        // `Facade::photo` drops `Taken::gap`, and `crate::photo::Taken` exists precisely so
        // that a gap cannot go missing in silence — "a gap nobody counted is exactly the
        // silence rubric rule 3 is about". The drop rests on one claim: this composition
        // opens a camera per call, so nothing in the caller's process is streaming the
        // device. That claim is what is driven here, in both directions — no preview, no
        // gap; a preview, a gap — because without the second half the first is an assertion
        // whose false branch nothing could reach.
        //
        // **The first half reads the gap off the facade's own take.** It was read off a
        // `crate::photo::take` the test assembled beside `Facade::photo` until 2026-08-20,
        // tied to the subject only by the two reports being equal — and a preview interrupt
        // changes no field of `PhotoReport`, so a facade that started a preview around its
        // take and stopped it again left all ten arms in this module green, measured (note
        // **N272**). That is the shadow note **N252** names: an expectation taken from a
        // re-run of the subject rather than from the subject. `Facade::photo_taken` is the
        // seam that ends it — `Facade::photo` is one line over it, so what is asserted here
        // is the composition the verb runs.
        let facade = facade();
        let selector = first(&facade);
        let request = schema::capture::PhotoRequest {
            stream: schema::capture::StreamRequest::default(),
            settle: schema::capture::SettlePolicy {
                spec: schema::capture::SettleSpec::SkipFrames { frames: 0 },
                deadline_ms: 5_000,
            },
            transform: schema::capture::Transform::None,
            sink: schema::capture::Sink::ReturnBytes {
                format: schema::capture::PhotoFormat::Jpeg,
            },
            wait: false,
        };

        let crate::photo::Taken { outcome, gap } = facade
            .photo_taken(
                &selector,
                &request,
                &mut crate::photo::WhereverTheCallerSaid,
                Stamp::epoch(),
            )
            .expect("a photo through the facade");
        let quiet = outcome.expect("a photo off a camera nobody is watching");
        assert!(
            gap.is_none(),
            "the facade's own composition interrupted a preview it does not have, and \
             `Facade::photo` drops that fact on the floor"
        );

        // And the public verb is driven beside it, so the seam is not a second implementation
        // the arm reads instead of the one that ships: `Facade::photo` is the line that drops
        // the gap, and it answers what the take it is one line over answered.
        let public = facade
            .photo(
                &selector,
                &request,
                &mut crate::photo::WhereverTheCallerSaid,
                Stamp::epoch(),
            )
            .expect("a photo through the facade's public verb");
        assert_eq!(
            public.report, quiet.report,
            "`Facade::photo` and the take it is one line over answered differently, so the \
             seam this arm reads the gap off is not the verb's own composition"
        );

        // The other direction, over the same pipeline the facade runs, with the one
        // difference the claim is about: somebody is streaming the camera. A facade cannot
        // produce this state on itself — that is the property under test — so the second
        // half is assembled here, and what it proves is that `gap` is a field this pipeline
        // does fill, which is what makes the assertion above able to go red.
        let (_, mut watched) = facade.open(&selector).expect("opens again");
        // `StreamRequest::default()` rather than `crate::preview::request()`: what this half
        // needs is a camera that is *streaming for somebody*, and pinning the preview's own
        // MJPEG request would tie the arm to whichever camera the corpus happens to list
        // first — the first one today is the Chicony IR module, which has GREY and nothing
        // else.
        watched
            .start_stream(&schema::capture::StreamRequest::default())
            .expect("every camera in the corpus has some mode");
        let interrupted = crate::photo::take(
            watched.as_mut(),
            &request,
            &mut crate::photo::WhereverTheCallerSaid,
            &MonotonicClock::new(),
            Stamp::epoch(),
        );
        interrupted.outcome.expect("a photo during a preview");
        assert!(
            interrupted.gap.is_some(),
            "the same pipeline reported no gap over a camera that was streaming, so the \
             assertion above proves nothing about the facade"
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
