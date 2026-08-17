//! The mutating verbs that act on a camera or on a process, over both transports.
//!
//! docs/7 P4c lands "the mutating half over RPC"; this file is the half of it that does not
//! touch D9's session tree — `set`, `snapshot`, `restore`, `discover_pairs`,
//! `profile_capture`, `photo` (which writes to a path the caller named), and
//! `terminate_holder` (which writes to nothing at all and signals a process). The eight
//! `calibrate_*` verbs are `calibrate_verbs.rs`'s, because what they change is a document
//! and the laws they answer to are D8's and D9's rather than the camera's. The transports,
//! the fixture and the typed-refusal recovery are `read_verbs.rs`'s, shared through
//! `support/wire.rs` and `support/fixture.rs`: one daemon answers both wires, so an answer
//! that differs differs because of the pipe.
//!
//! ## What a mutating suite has to prove that a read suite does not
//!
//! - **The write goes through the one planner.** `set` reaches `engine::pairing` →
//!   `engine::write::set`, the same functions `webcam-handler-cli set` reaches, in both guard
//!   directions. A second write path is the defect design §2.10 names, and the observable
//!   difference between the two directions — an automation switch-off written *first* — is
//!   what a second path would get wrong.
//! - **Requested is not applied, across a socket.** A driver that clamps is a *warning on
//!   the report* and never an error (AGENTS rule 5, E4, \[PF:6\]). Both halves of every
//!   `{requested, applied}` pair have to survive serialization, so the assertion is made on
//!   what came back over the wire rather than on what the engine produced.
//! - **The camera is left as it was found** (AGENTS rule 8). Three levels of evidence, and
//!   they are not interchangeable: the daemon's own report, a snapshot taken over the wire
//!   before and after, and the device read *behind the daemon's back* through the fake's
//!   shared `CameraState`. The third is the one that cannot be satisfied by a daemon that
//!   only says it restored, and it includes `flags.raw` — a probe that put the values back
//!   and left the automation engaged has not put the camera back \[PF:3\].
//! - **Availability is not capability** (AGENTS rule 7). A camera another process holds is
//!   `Busy`, and it arrives as `Busy` rather than as anything about what the camera can do.
//!
//! ## And what `photo` adds on top of that
//!
//! - **Byte fidelity is the product** (E6, AGENTS "Photos"). A verbatim camera JPEG has to
//!   survive base64 and a socket unchanged, so the payload that comes back is compared
//!   against the bytes the engine produced for the same request at the same instant — not
//!   against a length, and not against a hash of itself.
//! - **A request a socket can build and a command line cannot.** `webcam-handler-cli` resolves
//!   `-o` against the caller's cwd and refuses an extension this build cannot write, both
//!   while parsing; `webcam-handler-daemon` links no `cli-core`, so a relative
//!   `Sink::ServerPath` and a `.webp` one are requests only this surface can meet. Both are
//!   refused **before a camera opens**, which `FakeBackend::opens()` is what makes assertable
//!   (note N34, note N46, debt D-1).
//! - **The answer agrees with itself.** `PhotoResponse::bytes_match_the_delivery` is note
//!   N34's other orphan predicate and the daemon calls it before it sends; every photo answer
//!   this suite receives is checked with it too, from the side `webcam-handler-client` will.
//! - **One streamer per node** (D12). Two photo requests in flight against one camera reach
//!   one actor, and the fake refuses a second `start_stream` while one is running — so two
//!   answers and two sequential streams on one descriptor is the assertion.
//! - **A frame may contain a person.** Nothing on this path holds bytes but
//!   `api::Base64Bytes`, whose `Debug` prints a count (note N36's four subjects are
//!   unchanged by this file), and the daemon logs nothing here at all. One measured caveat:
//!   `jsonrpsee-core`'s `Methods::inner_call` traces the whole response document at `TRACE`,
//!   which the **in-memory** wire goes through and the socket does not — so a
//!   `RUST_LOG=trace` run of this suite would print a base64 payload. Those bytes are a
//!   synthetic frame from a committed profile, which is the only reason it is a note rather
//!   than a refusal to use that wire.
//!
//! ## And what P6c's three add on top of both
//!
//! `record_start`, `record_status` and `record_stop` are one T4 verb and three wire methods,
//! and what only a socket can ask about them is the **state between the calls**. So the
//! claims here are about `daemon::record`'s registry rather than about a container — the
//! muxer's own accounting is `webcam-handler-imaging`'s and the executor's is
//! `webcam-handler-engine`'s, both over the same committed profiles:
//!
//! - **A recording reaches the device.** A take that never streamed writes a header and no
//!   frames, which is indistinguishable from an honest zero-duration take unless somebody
//!   counts — so the frames the report claims are compared against what the file holds,
//!   through the strict reader `crates/imaging` wrote from the specification.
//! - **A camera holds one take, and a second `record_start` is `Busy`** — which means *retry*
//!   to an unattended reader, and retrying is the action that succeeds.
//! - **`record_status` says "running" exactly while one is**, for a camera that never
//!   recorded, one recording now, and one whose take has ended and not been collected.
//! - **`record_stop` on a camera holding nothing is a refusal**, because an agent has to be
//!   able to tell "my recording is over" from "my `record_start` never took".
//! - **A request only a socket can build is refused before a camera opens** — `ReturnBytes`
//!   (note **N110**), a relative path, an unwritable extension, a duration past the cap —
//!   which `FakeBackend::opens()` is what makes assertable, exactly as it is for `photo`.
//!
//! ## What is deliberately not asserted here, and why it is absent
//!
//! **`PermissionDenied`** is EPERM, a kernel answer a backend replaying a document has no
//! honest way to invent — "a fake capability no real device exhibits is a bug in the fake".
//! It is the one refusal AGENTS rule 7 keeps apart that this suite cannot provoke;
//! `DeviceGone` and `SettleTimeout` used to be beside it and are not any more, because
//! `photo` is the first verb here that starts a stream and both faults fire at
//! `next_frame`. What *is* asserted for all of them is that one never becomes another,
//! which is the direction that can actually go wrong; the codes themselves are pinned both
//! ways by `crates/api`'s D13 registry walk.

#[path = "support/fixture.rs"]
mod fixture;
mod support;
#[path = "support/wire.rs"]
mod wire;

use std::io::{BufRead, BufReader, Write};
use std::os::unix::process::ExitStatusExt;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;

use api::{PhotoResponse, WchRpcClient, rpc_code};
use camino::{Utf8Path, Utf8PathBuf};
use engine::paths::TempRuntimeDir;
use engine::settle::FrozenClock;
use jsonrpsee::core::client::Error as ClientError;
use schema::backend::{Camera, CameraBackend};
use schema::camera::{CameraId, PixelFormat};
use schema::capture::{
    Adjustment, PhotoDelivery, PhotoFormat, PhotoRendering, PhotoRequest, SettlePolicy, SettleSpec,
    Sink, StreamRequest, Transform,
};
use schema::control::{ControlDesc, ControlSlug, ControlValue, ControlWrite, WriteWarning};
use schema::error::{Error, ErrorKind};
use schema::pairing::AutomationPair;
use schema::profile::DeviceProfile;
use schema::report::{DiscoveryReport, TerminationReport, TerminationSignal, WriteReport};
use schema::snapshot::{RestoreOutcome, RestoreReport, Snapshot};
use schema::video::{RecordRequest, VideoFormat};

use crate::fixture::{Ask, Fixture, camera, second_camera};
use crate::wire::{Wire, refusal};

// ------------------------------------------------------------------ what the suite writes

/// The writes this suite makes, derived from the device rather than named.
///
/// Every one of them is a fact about the replayed profile that a literal would freeze:
/// which control is writable, which pair this camera actually has, and what "some other
/// value in range" is for it. `crates/testkit`'s fixture is allowed to change; this is
/// allowed to notice.
#[derive(Debug, Clone)]
struct Targets {
    /// A plain scalar write that must land exactly.
    exact: ControlWrite,
    /// The same control, asked for a value past its declared maximum — the PF:6 twin.
    past_the_maximum: ControlWrite,
    /// A control an automation partner currently owns, and the partner that owns it.
    ///
    /// This is the pair the guarded plan has to switch off *before* it writes, and the one
    /// an unguarded plan writes into alone.
    guarded: (ControlWrite, ControlSlug),
    /// A control the device refuses to be written at all \[PF:12\].
    read_only: ControlSlug,
}

/// Pick them off the control set and the pair set this device is in.
fn targets(controls: &[ControlDesc], pairs: &[AutomationPair], ask: &Ask) -> Targets {
    let scalar = describe(controls, &ask.control);
    let resting = scalar
        .current
        .as_ref()
        .and_then(ControlValue::as_int)
        .expect("the fixture's writable scalar reads back");
    // Somewhere else in range, and step-aligned, so an exact write is exact for a reason
    // rather than by luck: the low end unless that is where the control already sits.
    let elsewhere = if resting == scalar.range.min {
        scalar.range.min + scalar.range.step
    } else {
        scalar.range.min
    };
    assert_ne!(elsewhere, resting, "the write would not change anything");

    // A pair whose manual half this device is currently holding INACTIVE — which is the
    // shape a guarded write exists for, and the one PF:3 says follows the automation's
    // value on real hardware.
    let owned = pairs
        .iter()
        .find(|pair| describe(controls, &pair.manual).is_inactive())
        .expect("the replayed profile holds one manual control under its automation");
    let manual = describe(controls, &owned.manual);
    let manual_target = manual
        .current
        .as_ref()
        .and_then(ControlValue::as_int)
        .map(|current| {
            if current == manual.range.min {
                manual.range.min + manual.range.step
            } else {
                manual.range.min
            }
        })
        .expect("the manual half of a pair reads back");

    Targets {
        exact: ControlWrite {
            control: scalar.slug.clone(),
            value: ControlValue::Int(elsewhere),
        },
        past_the_maximum: ControlWrite {
            control: scalar.slug.clone(),
            value: ControlValue::Int(scalar.range.max + 1),
        },
        guarded: (
            ControlWrite {
                control: manual.slug.clone(),
                value: ControlValue::Int(manual_target),
            },
            owned.automation.clone(),
        ),
        read_only: controls
            .iter()
            .find(|desc| !desc.is_writable())
            .map(|desc| desc.slug.clone())
            .expect("the replayed profile has a read-only control [PF:12]"),
    }
}

/// One control's descriptor out of a control set, by slug.
fn describe(controls: &[ControlDesc], slug: &ControlSlug) -> ControlDesc {
    engine::pairing::describe(controls, slug).expect("the fixture named a control it has")
}

// -------------------------------------------------------------------- the shared questions

/// Every answer the five control-shaped mutating verbs give, over one transport.
///
/// The two fields a clock writes are projected out rather than compared: a `Snapshot`
/// carries `taken_at` and a `DeviceProfile` carries `captured_at`, so two runs of the same
/// questions differ there by construction and comparing the whole values would assert that
/// time had stopped. Everything else is compared whole.
#[derive(Debug, PartialEq)]
struct Mutations {
    /// The snapshot's entries and the camera they came from — see above for `taken_at`.
    snapshot: (
        schema::camera::CameraFingerprint,
        Vec<schema::snapshot::SnapshotEntry>,
    ),
    set_guarded: WriteReport,
    set_unguarded: WriteReport,
    clamped: WriteReport,
    discovered: DiscoveryReport,
    /// The profile without its provenance block — see above for `captured_at`.
    profile: (
        u32,
        schema::profile::ProfileInvariant,
        schema::profile::ProfileState,
    ),
    restore: RestoreReport,
}

/// Ask one client all five verbs, and put the camera back.
///
/// Generic over the **generated** client trait, so the method names, the parameter names
/// and the response types all come from `webcam-handler-api`'s declaration: a rename there
/// is a compile failure here rather than a JSON literal that quietly stops matching.
///
/// The order is the one a caller would use and is load-bearing for the suite: snapshot
/// first so there is something to put back, the writes in the middle, and `restore` last,
/// which is what makes running this twice — once per transport — compare two runs against
/// one starting state rather than against each other's residue.
///
/// **Every write here goes to an *unpaired* control, and that is deliberate.** A guarded
/// write to a control an automation partner owns cannot be undone by a restore: putting the
/// partner back re-engages it, so the manual value stays where the write left it and the
/// report says `OwnedByAutomation` — the ordinary success note N9 exists for. That is the
/// right behaviour and it is asserted on its own below; it is also not idempotent, so
/// running this sequence twice would compare the second transport against the first one's
/// residue. The pair goes to its own test, where nothing has to run twice.
async fn mutating_verbs<C: WchRpcClient + Sync>(
    client: &C,
    ask: &Ask,
    targets: &Targets,
) -> Mutations {
    let before: Snapshot = client
        .snapshot(ask.camera.clone())
        .await
        .expect("every writable control reads back");

    let set_guarded = client
        .set(ask.camera.clone(), vec![targets.exact.clone()], true)
        .await
        .expect("an unpaired scalar has nothing to switch off");
    let set_unguarded = client
        .set(ask.camera.clone(), vec![targets.exact.clone()], false)
        .await
        .expect("an unpaired scalar takes a plain write");
    let clamped = client
        .set(
            ask.camera.clone(),
            vec![targets.past_the_maximum.clone()],
            true,
        )
        .await
        .expect("a driver that clamps has not refused anything (E4)");

    let discovered = client
        .discover_pairs(ask.camera.clone())
        .await
        .expect("the probe writes and puts the camera back");
    let profile = client
        .profile_capture(ask.camera.clone(), "the mutating-verbs suite".to_owned())
        .await
        .expect("a capture reads the whole device");

    let restore = client
        .restore(ask.camera.clone(), before.clone())
        .await
        .expect("the snapshot came from this camera");

    Mutations {
        snapshot: (before.camera, before.entries),
        set_guarded,
        set_unguarded,
        clamped,
        discovered,
        profile: (profile.schema_version, profile.invariant, profile.state),
        restore,
    }
}

/// The typed refusals the five verbs can make, each as the code that arrived and the D13
/// error a client recovers.
#[derive(Debug, PartialEq)]
struct Refusals {
    unknown_camera: (i32, Error),
    ambiguous_prefix: (i32, Error),
    unknown_control: (i32, Error),
    read_only_control: (i32, Error),
    another_cameras_snapshot: (i32, Error),
}

/// Ask one client for five things it must refuse.
async fn refusals<C: WchRpcClient + Sync>(
    client: &C,
    ask: &Ask,
    targets: &Targets,
    other_cameras: &Snapshot,
) -> Refusals {
    Refusals {
        unknown_camera: refusal(client.snapshot(ask.unknown_camera.clone()).await),
        ambiguous_prefix: refusal(
            client
                .set(ask.ambiguous.clone(), vec![targets.exact.clone()], true)
                .await,
        ),
        unknown_control: refusal(
            client
                .set(
                    ask.camera.clone(),
                    vec![ControlWrite {
                        control: ask.unknown_control.clone(),
                        value: ControlValue::Int(1),
                    }],
                    true,
                )
                .await,
        ),
        read_only_control: refusal(
            client
                .set(
                    ask.camera.clone(),
                    vec![ControlWrite {
                        control: targets.read_only.clone(),
                        value: ControlValue::Int(1),
                    }],
                    true,
                )
                .await,
        ),
        another_cameras_snapshot: refusal(
            client
                .restore(ask.camera.clone(), other_cameras.clone())
                .await,
        ),
    }
}

/// The device's own state, read behind the daemon's back.
///
/// Values **and** raw flag words: PF:3's INACTIVE coupling is half of "as you found it",
/// and a probe that restored the values but left the automation engaged has not put the
/// camera back.
fn device_state(fixture: &Fixture) -> Vec<(ControlSlug, Option<ControlValue>, u32)> {
    fixture
        .controls()
        .into_iter()
        .map(|desc| (desc.slug, desc.current, desc.flags.raw))
        .collect()
}

// ------------------------------------------------------------------------------- the tests

#[tokio::test]
async fn the_mutating_verbs_answer_the_same_over_both_transports() {
    // The same claim P4b's read suite makes, for the half that writes. One daemon, two
    // pipes, one sequence of questions that ends by putting the camera back — so the second
    // run starts where the first one did and an inequality is the transport.
    let fixture = Fixture::start();
    let ask = fixture.ask();
    let controls = fixture.controls();
    let targets = targets(
        &controls,
        &engine::pairing::in_effect(&controls, Vec::new()),
        &ask,
    );
    let start = device_state(&fixture);

    let [(first_name, first_wire), (second_name, second_wire)] = fixture.wires();
    let first = mutating_verbs(&first_wire, &ask, &targets).await;
    let second = mutating_verbs(&second_wire, &ask, &targets).await;

    assert_eq!(
        first, second,
        "{first_name} and {second_name} answered differently"
    );

    // Not vacuous: an answer that was empty on both sides would compare equal and say
    // nothing. Every verb has to have found something, and the writes have to have written
    // something.
    assert!(!first.snapshot.1.is_empty());
    assert_eq!(first.set_unguarded.writes.len(), 1);
    assert_eq!(first.set_guarded.writes.len(), 1);
    assert!(
        first.set_guarded.disabled_automation.is_empty(),
        "the target is unpaired, which is what makes this sequence idempotent — see \
         `mutating_verbs`"
    );
    assert!(!first.clamped.is_exact(), "the driver clamped nothing");
    assert!(!first.discovered.controls.controls.is_empty());
    assert!(!first.discovered.controls.pairs.is_empty());
    assert!(!first.profile.1.controls.is_empty());
    assert!(!first.restore.outcomes.is_empty());

    // And the camera is where the first run found it, which is the property that makes the
    // comparison above a comparison of two transports rather than of two starting states.
    assert_eq!(device_state(&fixture), start);
}

#[tokio::test]
async fn every_mutating_verb_answers_what_the_engine_answers() {
    // The daemon assembles nothing of its own. Each comparison calls **one** engine
    // function — the same one `crates/cli`'s executor calls — against a second handle onto
    // the very device the actor is driving, because the fake's `CameraState` is shared
    // across every handle it hands out.
    //
    // `wch_discover_pairs` is the one that matters most here: its answer is *assembled*
    // (probe, then read the control set the camera is now in, then merge declared with
    // measured), and note N34 booked that assembly's move into `engine::discover` against
    // this sub-milestone precisely so that this comparison can be against one function
    // rather than against a second copy of three rules.
    let fixture = Fixture::start();
    let ask = fixture.ask();
    let controls = fixture.controls();
    let pairs = engine::pairing::in_effect(&controls, Vec::new());
    let targets = targets(&controls, &pairs, &ask);

    let (_, wire) = fixture
        .wires()
        .into_iter()
        .next()
        .expect("two transports were built");

    // `snapshot` is `engine::snapshot::take` over the pair set the engine says is in effect
    // for this device — the roles in it are pair-aware, so a daemon that guessed from the
    // slug would order its restore differently.
    let over_the_wire: Snapshot = wire
        .snapshot(ask.camera.clone())
        .await
        .expect("every writable control reads back");
    let mut handle = fixture
        .backend
        .open(&ask.camera)
        .expect("the fake hands out a second view of one device");
    let directly = engine::snapshot::take(handle.as_mut(), &pairs, schema::time::Stamp::now())
        .expect("the engine reads the same device");
    assert_eq!(over_the_wire.camera, directly.camera);
    assert_eq!(over_the_wire.entries, directly.entries);

    // `set` is `engine::write::set`, guard flag and all. Asserted by writing the *same*
    // request twice — once over the wire and once through the engine — and comparing the
    // two reports: the second write is a no-op on the device, so the reports agree on
    // everything except that.
    let written = wire
        .set(ask.camera.clone(), vec![targets.exact.clone()], false)
        .await
        .expect("an unpaired scalar takes a plain write");
    let engine_written =
        engine::write::set(handle.as_mut(), &pairs, &[targets.exact.target()], false)
            .expect("the engine writes the same value");
    assert_eq!(written, engine_written);

    // `discover_pairs` is `engine::discover::report`, whole — the N34 assembly with one
    // home. The probe restores what it touched, so running it twice is running it from the
    // same starting state.
    let probed = wire
        .discover_pairs(ask.camera.clone())
        .await
        .expect("the probe writes and puts the camera back");
    let engine_probed =
        engine::discover::report(handle.as_mut(), schema::time::Stamp::now()).expect("probes");
    assert_eq!(probed, engine_probed);

    // `profile_capture` is `engine::profile::capture`, and the three host facts the daemon
    // supplies are the ones with one home each: the kernel release the engine reads, the
    // schema crate's tool version, and the backend the registry is over — which is what
    // makes "a profile captured from the fake backend would be circular corpus" visible.
    let captured: DeviceProfile = wire
        .profile_capture(ask.camera.clone(), "a parity check".to_owned())
        .await
        .expect("a capture reads the whole device");
    assert_eq!(
        captured.provenance.kernel,
        engine::profile::kernel_release()
    );
    assert_eq!(captured.provenance.tool_version, schema::TOOL_VERSION);
    assert_eq!(captured.provenance.backend, fixture.backend.kind());
    assert_eq!(captured.provenance.capturer, "a parity check");
    let engine_captured = engine::profile::capture(
        handle.as_mut(),
        &engine::profile::CaptureContext {
            captured_at: captured.provenance.captured_at,
            kernel: captured.provenance.kernel.clone(),
            tool_version: captured.provenance.tool_version.clone(),
            capturer: captured.provenance.capturer.clone(),
            backend: captured.provenance.backend,
        },
    )
    .expect("the engine reads the same device");
    assert_eq!(captured, engine_captured);

    // And `restore` is `engine::snapshot::restore` — run over the wire, which is also what
    // puts this test's own writes back. The engine's copy is given the *same* starting
    // state rather than the one the wire's restore left behind: a restore reports what it
    // had to do, so running it twice in a row would compare "put it back" against "it was
    // already there" and fail for a reason that is not the daemon's.
    let put_back = wire
        .restore(ask.camera.clone(), over_the_wire.clone())
        .await
        .expect("the snapshot came from this camera");
    engine::write::set(handle.as_mut(), &pairs, &[targets.exact.target()], false)
        .expect("the same write again, so both restores start from one state");
    let engine_put_back = engine::snapshot::restore(handle.as_mut(), &pairs, &over_the_wire)
        .expect("the engine restores the same device");
    assert_eq!(put_back, engine_put_back);
}

#[tokio::test]
async fn a_guarded_write_switches_the_automation_off_first_and_an_unguarded_one_does_not() {
    // The one thing `guarded` chooses, and the only thing that differs between the two
    // plans: a manual control an automation partner owns cannot take a write until the
    // partner is switched off, so the plan writes the partner **first** and says which one
    // it disabled. A second write path — anywhere — would be free to get the order right
    // and the report wrong, or the reverse, which is why both are asserted.
    let fixture = Fixture::start();
    let ask = fixture.ask();
    let controls = fixture.controls();
    let pairs = engine::pairing::in_effect(&controls, Vec::new());
    let targets = targets(&controls, &pairs, &ask);
    let (write, automation) = targets.guarded.clone();

    let (_, wire) = fixture
        .wires()
        .into_iter()
        .next()
        .expect("two transports were built");

    // The unguarded arm runs **first**, and the order is the whole test. A guarded write
    // switches the automation off and leaves it off, so a guarded call before this one
    // would leave nothing for the planner to switch off — and an implementation that
    // ignored the flag entirely would then answer "disabled nothing" and look correct.
    // Asking for the unguarded write while the automation is still engaged is what makes
    // the two arms distinguishable.
    let unguarded = wire
        .set(ask.camera.clone(), vec![write.clone()], false)
        .await
        .expect("an unguarded write goes out alone");
    assert!(
        unguarded.disabled_automation.is_empty(),
        "the caller said they meant it and the planner switched something off anyway: {:?}",
        unguarded.disabled_automation
    );
    assert_eq!(
        unguarded
            .writes
            .iter()
            .map(|applied| &applied.slug)
            .collect::<Vec<_>>(),
        vec![&write.control]
    );

    // The guarded twin, over the same control, with the automation still where the device
    // had it. `plan` puts the switch-off in front of the write and says which control it
    // disabled — the fact a caller putting the camera back needs (D4).
    let guarded = wire
        .set(ask.camera.clone(), vec![write.clone()], true)
        .await
        .expect("the planner switches the automation off first");
    assert_eq!(guarded.disabled_automation, vec![automation.clone()]);
    let order: Vec<&ControlSlug> = guarded.writes.iter().map(|applied| &applied.slug).collect();
    assert_eq!(
        order,
        vec![&automation, &write.control],
        "the switch-off has to be written before the control it frees"
    );
}

#[tokio::test]
async fn a_clamped_write_crosses_the_wire_as_a_warning_and_never_as_a_refusal() {
    // AGENTS rule 5 and E4, at the one place they can be broken by a *transport*: both
    // halves of `{requested, applied}` and the warning that explains the gap have to
    // survive serialization. A daemon that answered an error here — or a wire shape that
    // kept only `applied` — would be the collapse rubric A10 exists to prevent, and the
    // caller would never learn the driver had moved their value.
    let fixture = Fixture::start();
    let ask = fixture.ask();
    let controls = fixture.controls();
    let targets = targets(
        &controls,
        &engine::pairing::in_effect(&controls, Vec::new()),
        &ask,
    );
    let asked = targets.past_the_maximum.value.clone();

    for (name, wire) in fixture.wires() {
        let report: WriteReport = wire
            .set(
                ask.camera.clone(),
                vec![targets.past_the_maximum.clone()],
                true,
            )
            .await
            .unwrap_or_else(|err| panic!("{name}: a clamp is not a refusal: {err}"));

        let applied = report
            .writes
            .iter()
            .find(|applied| applied.slug == targets.exact.control)
            .unwrap_or_else(|| panic!("{name}: the write is not in its own report"));
        assert_eq!(
            applied.requested, asked,
            "{name}: the request was rewritten"
        );
        assert_ne!(
            applied.applied, asked,
            "{name}: the device did not clamp, so this asserts nothing"
        );
        assert!(!applied.is_exact(), "{name}");
        assert!(!report.is_exact(), "{name}");
        assert!(
            applied
                .warnings
                .iter()
                .any(|warning| matches!(warning, WriteWarning::Clamped { .. })),
            "{name}: the clamp crossed the wire without its explanation: {:?}",
            applied.warnings
        );
        // The declared range travelled with it, which is what makes the warning checkable
        // by a client that never saw the descriptor \[PF:6\].
        let Some(WriteWarning::Clamped { range, .. }) = applied
            .warnings
            .iter()
            .find(|warning| matches!(warning, WriteWarning::Clamped { .. }))
        else {
            panic!("{name}: the warning above went missing between two assertions");
        };
        assert_eq!(range, &describe(&controls, &targets.exact.control).range);
    }
}

#[tokio::test]
async fn the_probe_writes_to_the_camera_and_the_answer_says_it_put_it_back() {
    // AGENTS rule 8 over a socket, for the verb that writes without being asked to write
    // anything: `discover_pairs` toggles every automation-shaped control it dares to and
    // restores what it touched. Three levels of evidence, in the order of how much they
    // are worth.
    let fixture = Fixture::start();
    let ask = fixture.ask();
    let before = device_state(&fixture);

    let (_, wire) = fixture
        .wires()
        .into_iter()
        .next()
        .expect("two transports were built");
    let report: DiscoveryReport = wire
        .discover_pairs(ask.camera.clone())
        .await
        .expect("the probe writes and puts the camera back");

    // Not vacuous: a probe that toggled nothing would restore perfectly and prove nothing.
    // The measured pairs are the witness that the camera really moved — they exist only
    // because a toggle changed an INACTIVE bit and the probe watched it (E1).
    assert!(
        report
            .controls
            .pairs
            .iter()
            .any(|pair| pair.provenance == schema::pairing::Provenance::Measured),
        "the probe measured nothing, so nothing was written: {:?}",
        report.controls.pairs
    );

    // 1. The daemon's own report — necessary, and on its own it is restoration by
    //    assumption.
    assert!(
        report.restored.is_complete(),
        "the probe says it could not put {:?} back",
        report.restored.unrestored()
    );

    // 2. A snapshot over the wire, before and after. Compared on entries rather than on the
    //    whole document, because `taken_at` differs by construction.
    let after_over_the_wire: Snapshot = wire
        .snapshot(ask.camera.clone())
        .await
        .expect("every writable control reads back");
    let engine_snapshot = {
        let controls = fixture.controls();
        let pairs = engine::pairing::in_effect(&controls, Vec::new());
        let mut handle = fixture.backend.open(&ask.camera).expect("the fake opens");
        engine::snapshot::take(handle.as_mut(), &pairs, schema::time::Stamp::now())
            .expect("the engine reads the same device")
    };
    assert_eq!(after_over_the_wire.entries, engine_snapshot.entries);

    // 3. The device's own answer, values and raw flag words — the level a daemon that only
    //    *said* it restored cannot satisfy \[PF:3\].
    assert_eq!(device_state(&fixture), before);
}

#[tokio::test]
async fn a_restore_puts_back_what_a_write_moved_and_an_owned_control_counts_as_restored() {
    // The write/restore round trip over the wire, and note N9's fourth outcome inside it: a
    // guarded write switches an automation control off, and putting *that* back re-engages
    // the automation — so the manual control it governs comes back as
    // `OwnedByAutomation`, which is a **success**. A suite that read `is_complete()` as
    // "every control was written" would go red here, which is the bug N9 was written for.
    let fixture = Fixture::start();
    let ask = fixture.ask();
    let controls = fixture.controls();
    let pairs = engine::pairing::in_effect(&controls, Vec::new());
    let targets = targets(&controls, &pairs, &ask);
    let (write, automation) = targets.guarded.clone();
    let before = device_state(&fixture);

    let (_, wire) = fixture
        .wires()
        .into_iter()
        .next()
        .expect("two transports were built");

    let snapshot: Snapshot = wire
        .snapshot(ask.camera.clone())
        .await
        .expect("every writable control reads back");
    wire.set(ask.camera.clone(), vec![write.clone()], true)
        .await
        .expect("the planner switches the automation off first");

    // The witness that this test is not comparing a camera with itself.
    assert_ne!(
        device_state(&fixture),
        before,
        "the guarded write changed nothing"
    );

    let report: RestoreReport = wire
        .restore(ask.camera.clone(), snapshot)
        .await
        .expect("the snapshot came from this camera");

    // A restore that could not finish is a report saying so, not an error — and nothing
    // here could not finish.
    assert!(
        report.is_complete(),
        "a restore that could not finish: {:?}",
        report.unrestored()
    );

    // The automation control the plan switched off is written back **first** (D4), so the
    // camera is answering to itself again.
    assert!(
        report.outcomes.iter().any(|outcome| matches!(
            outcome,
            RestoreOutcome::Restored { applied } if applied.slug == automation
        )),
        "the automation the write switched off was not put back: {report:?}"
    );

    // And the manual control it governs comes back as `OwnedByAutomation` — a **success**.
    // Its value is not written, because its owner is back and the value is that
    // automation's to choose; a suite that read this as a failure would re-introduce
    // exactly the bug note N9 was written for.
    assert!(
        report.outcomes.iter().any(|outcome| matches!(
            outcome,
            RestoreOutcome::OwnedByAutomation { control, .. } if control == &write.control
        )),
        "the control the automation owns again is not reported as owned: {report:?}"
    );

    // So "as you found it" is the *flags* and the automation's own value, and the manual
    // value is deliberately not among them (PF:3, N9). Asserting the whole device state
    // equal here would be asserting something D4 does not promise and no real device does.
    let flags = |state: &[(ControlSlug, Option<ControlValue>, u32)]| {
        state
            .iter()
            .map(|(slug, _, raw)| (slug.clone(), *raw))
            .collect::<Vec<_>>()
    };
    let after = device_state(&fixture);
    assert_eq!(
        flags(&after),
        flags(&before),
        "the coupling did not come back"
    );
    let value_of = |state: &[(ControlSlug, Option<ControlValue>, u32)], slug: &ControlSlug| {
        state
            .iter()
            .find(|(name, _, _)| name == slug)
            .map(|(_, value, _)| value.clone())
            .expect("the camera still has the control")
    };
    assert_eq!(
        value_of(&after, &automation),
        value_of(&before, &automation)
    );
    assert_eq!(
        value_of(&after, &write.control),
        Some(write.value.clone()),
        "the manual control was written back over its automation's answer"
    );
}

#[tokio::test]
async fn the_failure_directions_cross_both_transports_as_the_same_typed_error() {
    // Every refusal these five verbs can make over the fake, recovered client-side through
    // `api::codes::typed` — the half `webcam-handler-client` depends on and the half a
    // server-side assertion cannot see.
    let fixture = Fixture::start();
    let ask = fixture.ask();
    let controls = fixture.controls();
    let targets = targets(
        &controls,
        &engine::pairing::in_effect(&controls, Vec::new()),
        &ask,
    );

    // A snapshot that genuinely belongs to the *other* camera the fixture replays — its
    // fingerprint differs in two fields, which is what the refusal has to name.
    let other = camera(&fixture.cameras, 1).id.clone();
    let (_, wire) = fixture
        .wires()
        .into_iter()
        .next()
        .expect("two transports were built");
    let other_cameras: Snapshot = wire
        .snapshot(other)
        .await
        .expect("the second camera answers too");

    let [(first_name, first_wire), (second_name, second_wire)] = fixture.wires();
    let first = refusals(&first_wire, &ask, &targets, &other_cameras).await;
    let second = refusals(&second_wire, &ask, &targets, &other_cameras).await;
    assert_eq!(
        first, second,
        "{first_name} and {second_name} refused differently"
    );

    assert_eq!(
        first.unknown_camera,
        (
            rpc_code(ErrorKind::CameraUnknown),
            Error::CameraUnknown {
                requested: ask.unknown_camera.to_string(),
            }
        )
    );
    assert_eq!(
        first.ambiguous_prefix,
        (
            rpc_code(ErrorKind::CameraAmbiguous),
            engine::resolve::camera(&fixture.cameras, &ask.ambiguous)
                .expect_err("two cameras answer to this prefix")
        )
    );

    // The unknown control's suggestions are the planner's, so `set brightnes=1` over the
    // wire names what `get brightnes` would.
    assert_eq!(
        first.unknown_control,
        (
            rpc_code(ErrorKind::ControlUnknown),
            engine::pairing::describe(&controls, &ask.unknown_control)
                .expect_err("no such control")
        )
    );

    // A control the device will not be written is `ControlReadOnly` and not a device error
    // \[PF:12\]: the camera is fine, the request is not.
    assert_eq!(
        first.read_only_control.0,
        rpc_code(ErrorKind::ControlReadOnly)
    );
    assert_eq!(
        first.read_only_control.1,
        Error::ControlReadOnly {
            control: targets.read_only.clone(),
        }
    );

    // And a snapshot from another camera is refused **naming the fields that differ**,
    // which is what a caller acts on — read off the enumeration rather than transcribed.
    assert_eq!(
        first.another_cameras_snapshot.0,
        rpc_code(ErrorKind::FingerprintMismatch)
    );
    let Error::FingerprintMismatch { fields } = &first.another_cameras_snapshot.1 else {
        panic!("{:?}", first.another_cameras_snapshot);
    };
    assert_eq!(
        fields,
        &camera(&fixture.cameras, 1)
            .fingerprint
            .differing_fields(&camera(&fixture.cameras, 0).fingerprint)
            .into_iter()
            .map(str::to_owned)
            .collect::<Vec<String>>()
    );

    // Every refusal used a distinct code, so none of the assertions above is passing on a
    // registry that had collapsed to one error.
    let codes: std::collections::BTreeSet<i32> = [
        first.unknown_camera.0,
        first.ambiguous_prefix.0,
        first.unknown_control.0,
        first.read_only_control.0,
        first.another_cameras_snapshot.0,
    ]
    .into_iter()
    .collect();
    assert_eq!(codes.len(), 5, "{codes:?}");
}

#[tokio::test]
async fn a_camera_another_process_holds_is_busy_and_stays_busy_across_the_wire() {
    // AGENTS rule 7 on the mutating half: EBUSY is an *availability* answer and must not
    // arrive looking like anything about what the camera can do.
    //
    // The fault is queued against the camera **no verb in this test has used**, and that is
    // the whole mechanism: a queued `Fault::Busy` is looked for at `CameraBackend::open`,
    // and the actor keeps its `Box<dyn Camera>` between commands — so a fault queued after
    // a camera is open would sit there unfired and this test would assert nothing.
    let fixture = Fixture::start();
    let ask = fixture.ask();
    let untouched = camera(&fixture.cameras, 1).id.clone();

    for (name, wire) in fixture.wires() {
        fixture.backend.queue_fault(fake::Fault::Busy);
        let (code, error) = refusal(wire.snapshot(untouched.clone()).await);
        assert_eq!(code, rpc_code(ErrorKind::Busy), "{name}");
        assert_eq!(error.kind(), ErrorKind::Busy, "{name}");
        // The three it must never become: a camera somebody else is using is not a camera
        // that cannot do this, is not a camera that is gone, and is not the kernel failing.
        for confusable in [
            ErrorKind::FormatUnsupported,
            ErrorKind::DeviceGone,
            ErrorKind::DeviceIo,
        ] {
            assert_ne!(error.kind(), confusable, "{name}");
            assert_ne!(code, rpc_code(confusable), "{name}");
        }
    }

    // …and the camera the fault was not queued against still answers, so the refusal above
    // was the fault rather than a daemon that had stopped working.
    let (_, wire) = fixture
        .wires()
        .into_iter()
        .next()
        .expect("two transports were built");
    wire.snapshot(ask.camera.clone())
        .await
        .expect("nothing is holding this one");
}

#[tokio::test]
async fn the_camera_verbs_leave_d9s_session_tree_alone_and_never_answer_store_locked() {
    // The trap `crate::state`'s header exists for, from the other side. A daemon holds one
    // advisory lock for its lifetime, so a handler that reached for D9's *daemonless* protocol
    // would not hang — it would answer this client `StoreLocked`, naming the daemon's own pid
    // and advising it to use `webcam-handler-client` against the daemon it is already talking
    // to. `tests/lock.rs` pins the symptom; this is the negative for the five verbs P4c's
    // first step routes, which write to a camera and to nothing else.
    let fixture = Fixture::start();
    let ask = fixture.ask();
    let controls = fixture.controls();
    let targets = targets(
        &controls,
        &engine::pairing::in_effect(&controls, Vec::new()),
        &ask,
    );

    let before = engine::lifecycle::list(&fixture.store, None).expect("the store walks");
    assert_eq!(before.sessions.len(), 2, "the fixture wrote two sessions");

    let (_, wire) = fixture
        .wires()
        .into_iter()
        .next()
        .expect("two transports were built");
    let answers = mutating_verbs(&wire, &ask, &targets).await;
    assert!(!answers.restore.outcomes.is_empty(), "nothing ran");

    // Nothing the five verbs did touched a session document — not its content, not its
    // event log, not its `updated_at`.
    assert_eq!(
        engine::lifecycle::list(&fixture.store, None).expect("the store walks"),
        before
    );

    // And the refusals they *can* make are never that one.
    let other = camera(&fixture.cameras, 1).id.clone();
    let other_cameras: Snapshot = wire
        .snapshot(other)
        .await
        .expect("the second camera answers");
    let refused = refusals(&wire, &ask, &targets, &other_cameras).await;
    for (code, error) in [
        refused.unknown_camera,
        refused.ambiguous_prefix,
        refused.unknown_control,
        refused.read_only_control,
        refused.another_cameras_snapshot,
    ] {
        assert_ne!(error.kind(), ErrorKind::StoreLocked, "{error}");
        assert_ne!(code, rpc_code(ErrorKind::StoreLocked), "{error}");
    }
}

#[tokio::test]
async fn the_mutating_half_is_really_crossing_the_socket() {
    // Every comparison in this file depends on this and none of them can see it: a
    // `Wire::Uds` that had quietly fallen back to the in-memory module would make the whole
    // suite one transport run twice, and every `assert_eq!` would still pass. It matters
    // more here than next door, because the socket is the only thing between a client and a
    // write to somebody's camera (D11).
    let mut fixture = Fixture::start();
    let ask = fixture.ask();
    let controls = fixture.controls();
    let targets = targets(
        &controls,
        &engine::pairing::in_effect(&controls, Vec::new()),
        &ask,
    );
    let [(_, in_memory), (_, over_uds)] = fixture.wires();

    over_uds
        .set(ask.camera.clone(), vec![targets.exact.clone()], false)
        .await
        .expect("the socket is serving");

    fixture.handle.stop();
    fixture
        .handle
        .stopped()
        .await
        .expect("the server was asked to stop");

    // A transport failure, not a refusal the daemon sent — E3's distinction at the
    // transport layer, and the one P4f's "the daemon is not running" message is built on.
    match over_uds
        .set(ask.camera.clone(), vec![targets.exact.clone()], false)
        .await
    {
        Err(ClientError::Transport(_)) => {}
        other => panic!("a socket nobody is listening on answered: {other:?}"),
    }

    // And the in-memory wire never needed the socket, which is the other half of "two".
    in_memory
        .set(ask.camera.clone(), vec![targets.exact.clone()], false)
        .await
        .expect("in-memory dispatch does not go through a socket");
}

// ------------------------------------------------------------------------------- `photo`

/// A photo request that settles immediately, so every assertion below is about the wire.
///
/// `SkipFrames { frames: 0 }` rather than the default policy because the frames matter
/// twice over here: the fake's synthesizer is deterministic in the frame index, and
/// `start_stream` resets it, so one frame per photo is what makes "the same request twice
/// produces the same bytes" a property rather than a coincidence. Nothing waits for the
/// deadline either way — the fake reports a deadline as spent rather than sleeping through
/// it.
fn photo_request(sink: Sink) -> PhotoRequest {
    PhotoRequest {
        stream: StreamRequest::default(),
        settle: SettlePolicy {
            spec: SettleSpec::SkipFrames { frames: 0 },
            deadline_ms: 5_000,
        },
        transform: Transform::None,
        sink,
        // The default, and the shape every request sent before D12's flag existed. The
        // tests that are about the flag build their own.
        wait: false,
    }
}

/// The same photo, taken through the engine on a second view of the same device.
///
/// `now` is the wall time the daemon's answer recorded, handed back in rather than read
/// again: it is what goes in the EXIF, so a second reading would produce a photo that
/// differs in its header for a reason that is not the wire.
///
/// The *monotonic* reading the settle runs on is [`FrozenClock`], for the reason it exists
/// (note N60): this comparison is about bytes, so a `SettleTimeout` here would be a correct
/// answer to a question it is not asking. The settle it drives counts frames — the request
/// these callers build skips none — and the daemon's own side of the comparison necessarily
/// runs on the real clock it constructs for itself, so this removes the one of the two
/// wall-clock exposures a test can reach.
fn engine_photo(
    fixture: &Fixture,
    camera: &CameraId,
    request: &PhotoRequest,
    now: schema::time::Stamp,
) -> engine::photo::Photograph {
    let mut handle = fixture
        .backend
        .open(camera)
        .expect("the fake hands out a second view of one device");
    engine::photo::take(
        handle.as_mut(),
        request,
        &mut engine::photo::WhereverTheCallerSaid,
        &FrozenClock,
        now,
    )
    .outcome
    .expect("the engine takes the same photo")
}

#[tokio::test]
async fn a_returned_photo_crosses_the_wire_byte_for_byte_as_the_cameras_own_bitstream() {
    // E6 is the product, and this is the crossing that could quietly cost it: the payload
    // rides as base64 in a JSON string, and a transport encoding that changed one byte
    // would still decode into a picture nobody could tell apart by looking.
    //
    // So the comparison is against the bytes `engine::photo::take` produces for the same
    // request at the same instant, over a second view of the same device — not against a
    // length, not against a hash of what arrived. Both wires, because the socket and the
    // in-memory dispatch serialize the same document through different code.
    let fixture = Fixture::start();
    let camera = camera(&fixture.cameras, 0).id.clone();
    let request = photo_request(Sink::ReturnBytes {
        format: PhotoFormat::Jpeg,
    });

    for (name, wire) in fixture.wires() {
        let answer: PhotoResponse = wire
            .photo(camera.clone(), request.clone())
            .await
            .unwrap_or_else(|err| panic!("{name}: {err}"));

        // Note N34's predicate, from the side `webcam-handler-client` will call it from (P4f).
        // The daemon has already refused to send an answer that fails it; this is the client
        // half.
        assert!(answer.bytes_match_the_delivery(), "{name}: {answer:?}");

        // The camera's own bitstream, and the report says so rather than leaving a caller
        // to infer it from a byte count.
        assert_eq!(
            answer.report.rendering,
            PhotoRendering::Verbatim {
                source: PixelFormat::MJPG
            },
            "{name}"
        );
        let PhotoDelivery::Bytes { format, byte_count } = &answer.report.delivery else {
            panic!("{name}: a ReturnBytes sink must report bytes");
        };
        assert_eq!(*format, PhotoFormat::Jpeg, "{name}");

        let bytes = answer
            .bytes
            .as_ref()
            .unwrap_or_else(|| panic!("{name}: a ReturnBytes sink hands the bytes back"));
        assert_eq!(
            u64::try_from(bytes.len()).expect("a fixture that fits"),
            *byte_count,
            "{name}"
        );
        assert!(
            bytes.len() > 100,
            "{name}: the payload has to be worth carrying"
        );
        // Still a JPEG on arrival: base64 is a transport encoding and nothing else.
        assert_eq!(bytes.as_slice().get(..2), Some(&[0xff, 0xd8][..]), "{name}");

        let directly = engine_photo(&fixture, &camera, &request, answer.report.taken_at);
        assert_eq!(
            bytes.as_slice(),
            directly
                .returned
                .as_deref()
                .expect("the engine hands the bytes back too"),
            "{name}: the wire changed the camera's bytes"
        );
        // And the whole report, so nothing the device said about the photo was dropped or
        // rewritten on the way out (rubric A10).
        assert_eq!(answer.report, directly.report, "{name}");
    }
}

#[tokio::test]
async fn a_photo_written_to_a_server_path_lands_on_the_daemons_host_and_says_where() {
    // D10's other sink, and the one whose whole point is that the file appears on the
    // machine running the daemon rather than the one running the client. The destination is
    // a throw-away directory under `$TMPDIR` and never inside the tree:
    // `no-frame-bytes-in-repo.sh` content-sniffs every file in the repository, and a photo
    // written into it would be a frame in the repository.
    let fixture = Fixture::start();
    let camera = camera(&fixture.cameras, 0).id.clone();
    let scratch = TempRuntimeDir::new().expect("a throw-away directory");

    for (name, wire) in fixture.wires() {
        let path = scratch.base().join(format!("{name}.jpg"));
        let request = photo_request(Sink::ServerPath { path: path.clone() });
        let answer: PhotoResponse = wire
            .photo(camera.clone(), request.clone())
            .await
            .unwrap_or_else(|err| panic!("{name}: {err}"));

        assert!(answer.bytes_match_the_delivery(), "{name}: {answer:?}");
        assert!(
            answer.bytes.is_none(),
            "{name}: a caller who asked for a file got a copy of it as well"
        );
        let PhotoDelivery::Path {
            path: reported,
            byte_count,
        } = &answer.report.delivery
        else {
            panic!("{name}: a path sink must report a path");
        };
        assert_eq!(reported, &path, "{name}");

        let written = std::fs::read(&path).unwrap_or_else(|err| panic!("{name}: {path}: {err}"));
        assert_eq!(
            u64::try_from(written.len()).expect("a fixture that fits"),
            *byte_count,
            "{name}"
        );

        // The bytes that reached the daemon's disk are the engine's, which is the same
        // claim the `ReturnBytes` arm makes about the ones that reached the socket.
        let engine_path = scratch.base().join(format!("{name}-engine.jpg"));
        let directly = engine_photo(
            &fixture,
            &camera,
            &photo_request(Sink::ServerPath {
                path: engine_path.clone(),
            }),
            answer.report.taken_at,
        );
        assert!(
            directly.returned.is_none(),
            "{name}: the engine handed back a copy of a file it wrote"
        );
        assert_eq!(
            written,
            std::fs::read(&engine_path).unwrap_or_else(|err| panic!("{name}: {err}")),
            "{name}"
        );
    }
}

#[tokio::test]
async fn the_negotiated_format_and_size_are_surfaced_when_they_differ_from_the_request() {
    // D5 through the wire: "requested is not applied" is not only a control rule. A caller
    // that asked for a square megapixel frame got 640x480, and the answer has to say so —
    // both as the adjustment list and as the dimensions of the image the bytes actually
    // encode, because a client that trusted its own request would mis-scale every photo.
    let fixture = Fixture::start();
    let camera = camera(&fixture.cameras, 0).id.clone();
    let (_, wire) = fixture
        .wires()
        .into_iter()
        .next()
        .expect("two transports were built");

    let request = PhotoRequest {
        stream: StreamRequest {
            width: Some(1_000),
            height: Some(1_000),
            ..StreamRequest::default()
        },
        ..photo_request(Sink::ReturnBytes {
            format: PhotoFormat::Jpeg,
        })
    };
    let answer: PhotoResponse = wire
        .photo(camera.clone(), request.clone())
        .await
        .expect("a size the device does not offer is adjusted, not refused");

    assert!(
        !answer.report.negotiated.is_exact(),
        "the device delivered a size nobody offered: {:?}",
        answer.report.negotiated
    );
    assert_eq!(
        answer.report.negotiated.adjustments,
        vec![Adjustment::Size {
            requested_width: 1_000,
            requested_height: 1_000,
            negotiated_width: answer.report.negotiated.width,
            negotiated_height: answer.report.negotiated.height,
        }]
    );
    assert_ne!(
        (
            answer.report.negotiated.width,
            answer.report.negotiated.height
        ),
        (1_000, 1_000)
    );
    // The image's own dimensions, which are the negotiated ones because nothing was
    // rotated — the field a caller lays out a viewport from.
    assert_eq!(
        (answer.report.width, answer.report.height),
        (
            answer.report.negotiated.width,
            answer.report.negotiated.height
        )
    );

    // The twin: a request the device *can* meet exactly reports no adjustment at all, so
    // the arm above is measuring a difference rather than always finding one.
    let exact = PhotoRequest {
        stream: StreamRequest {
            width: Some(1_920),
            height: Some(1_080),
            ..StreamRequest::default()
        },
        ..photo_request(Sink::ReturnBytes {
            format: PhotoFormat::Jpeg,
        })
    };
    let answered: PhotoResponse = wire
        .photo(camera.clone(), exact)
        .await
        .expect("the device offers 1920x1080");
    assert!(
        answered.report.negotiated.is_exact(),
        "{:?}",
        answered.report
    );
    assert_eq!(
        (
            answered.report.negotiated.width,
            answered.report.negotiated.height
        ),
        (1_920, 1_080)
    );

    // And the pixel format is reported whether or not it was asked for: a caller who said
    // nothing still learns which of the camera's formats it got.
    assert_eq!(answer.report.negotiated.pixel_format, PixelFormat::MJPG);
}

#[tokio::test]
async fn a_sink_only_a_socket_can_build_is_refused_before_any_camera_is_opened() {
    // Note N34's `is_addressable` and debt D-1's extension refusal, which are the two things
    // this daemon adds on top of an engine call — and the two requests neither
    // `webcam-handler-cli` nor `webcam-handler-client` can produce, because
    // `cli_core::Command::photo_request` resolves against the caller's cwd and refuses an
    // unwritable extension while parsing.
    //
    // The evidence that they are refused *early* is `FakeBackend::opens()`: a request this
    // build was never going to honour must not cost anybody a descriptor, and on a machine
    // with one webcam that is the difference between a typo and an interrupted call.
    let fixture = Fixture::start();
    let camera = camera(&fixture.cameras, 0).id.clone();
    let scratch = TempRuntimeDir::new().expect("a throw-away directory");
    let (_, wire) = fixture
        .wires()
        .into_iter()
        .next()
        .expect("two transports were built");
    assert_eq!(fixture.backend.opens(), 0, "the fixture opened a camera");

    // A relative path. Under systemd the daemon's working directory is `/`, so resolving
    // this would write `/out.jpg` as the daemon's uid. The directory named here does not
    // exist, deliberately: a build that stopped refusing would then fail with `StorageIo`
    // rather than dropping a frame into whatever cwd the test runner happened to have.
    let relative = "nowhere-a-daemon-should-write/out.jpg";
    let (code, error) = refusal(
        wire.photo(
            camera.clone(),
            photo_request(Sink::ServerPath {
                path: relative.into(),
            }),
        )
        .await,
    );
    assert_eq!(code, rpc_code(ErrorKind::IllegalTransition));
    assert_eq!(error.kind(), ErrorKind::IllegalTransition);
    assert!(error.to_string().contains(relative), "{error}");
    assert!(error.to_string().contains("absolute"), "{error}");
    // Not the camera's fault and not the disk's: neither was asked (E3).
    assert_ne!(error.kind(), ErrorKind::FormatUnsupported);
    assert_ne!(error.kind(), ErrorKind::StorageIo);
    assert!(
        !camino::Utf8Path::new(relative).exists(),
        "a relative sink was resolved somewhere after all"
    );

    // An extension this build cannot write. Before P4c the engine's own comment answered
    // this case with "the CLI refuses it while building the sink" and fell through to
    // `PhotoFormat::Jpeg`, so `/tmp/x.webp` got JPEG bytes in a file whose name lied about
    // them (debt D-1).
    let webp = scratch.base().join("x.webp");
    let (code, error) = refusal(
        wire.photo(
            camera.clone(),
            photo_request(Sink::ServerPath { path: webp.clone() }),
        )
        .await,
    );
    assert_eq!(code, rpc_code(ErrorKind::IllegalTransition));
    assert!(error.to_string().contains("webp"), "{error}");
    for format in PhotoFormat::ALL {
        assert!(
            error.to_string().contains(format.extension()),
            "the three it does write: {error}"
        );
    }
    assert!(!webp.exists(), "a refused photo was written anyway");

    // A settle deadline no capture may hold a camera for (note **N147**). It joins these two
    // because it is the same kind of question — one about the *request*, answerable with no
    // filesystem and no enumeration — and because it was answered in the wrong place:
    // `engine::capture::grab` refuses it, but `grab` runs inside the camera's actor, after
    // this daemon has resolved a camera, opened the destination and taken a seat in the
    // queue. What that cost a caller was a zero-length file at a path they named.
    let long = scratch.base().join("long-settle.jpg");
    let mut over = photo_request(Sink::ServerPath { path: long.clone() });
    over.settle.deadline_ms = schema::limits::MAX_SETTLE_DEADLINE_MS + 1;
    let (code, error) = refusal(wire.photo(camera.clone(), over).await);
    assert_eq!(code, rpc_code(ErrorKind::IllegalTransition));
    assert!(
        error
            .to_string()
            .contains(&schema::limits::MAX_SETTLE_DEADLINE_MS.to_string()),
        "the refusal must name the bound an unattended caller has to fit inside: {error}"
    );
    assert_ne!(
        error.kind(),
        ErrorKind::SettleTimeout,
        "nothing waited (E3)"
    );
    assert!(
        !long.exists(),
        "a request this build was never going to honour left a file where none was"
    );

    // None of the three refusals opened a camera, which is the whole reason they are
    // answered where they are rather than inside `engine::photo`.
    assert_eq!(
        fixture.backend.opens(),
        0,
        "a refused request opened a camera"
    );

    // The twin, and the arm that would go red if this daemon started refusing paths rather
    // than refusing *these* paths: an absolute path with no extension at all is a JPEG,
    // because that is a filename the caller chose and we do not get to rename it.
    let unnamed = scratch.base().join("photo");
    let answer: PhotoResponse = wire
        .photo(
            camera.clone(),
            photo_request(Sink::ServerPath {
                path: unnamed.clone(),
            }),
        )
        .await
        .expect("a path with no extension names no encoding to refuse");
    assert!(answer.bytes_match_the_delivery());
    assert_eq!(
        std::fs::read(&unnamed).expect("the file exists").get(..2),
        Some(&[0xff, 0xd8][..]),
        "a bare path is a JPEG"
    );
    assert_eq!(fixture.backend.opens(), 1, "the photo that was honoured");
}

/// A fifo at `path`, made by the one program that can make one without a syscall wrapper.
///
/// This crate is `#![forbid(unsafe_code)]` and the workspace confines `libc` to the V4L2
/// backend, so `mkfifo(3)` is out of reach from here. `mkfifo(1)` is coreutils and the suite
/// already forks children for `spawn_holder`; a host without it fails loudly rather than
/// skipping, because a skip that reads as a pass is the one thing AGENTS forbids more than a
/// shelled-out fixture.
fn mkfifo(path: &Utf8Path) {
    let made = Command::new("mkfifo")
        .arg(path.as_str())
        .status()
        .expect("mkfifo(1) is coreutils and this test needs a fifo it cannot make itself");
    assert!(made.success(), "mkfifo {path}: {made:?}");
    assert!(path.exists(), "{path}");
}

#[tokio::test]
async fn a_server_path_that_is_not_a_regular_file_is_refused_before_the_camera_is_touched() {
    // Note **N51**. A photo's bytes used to be written with `std::fs::write`, whose `open`
    // carries no `O_NONBLOCK`: on a fifo it blocks until somebody opens the read end. That
    // open ran *inside the camera's actor closure*, and nothing bounded a submitted command
    // — so a
    // client that ran `mkfifo /tmp/x.jpg` and named it would park that camera's one thread
    // for the life of the process. Everything after that follows: the request never answers,
    // the actor's queue fills and every later request for that camera is `Busy`, and D12's
    // idle close never fires because `CameraActor::sweep` sees `busy` first — the operator's
    // webcam is unusable by any application until `webcam-handler-daemon` is restarted.
    //
    // Neither `webcam-handler-cli` nor `webcam-handler-client` can send this: `-o` is resolved
    // on a command line somebody typed. It is `daemon::server::open_destination`'s rule,
    // beside the two `addressable` answers, and for their reason — a request this build was
    // never going to honour must not cost anybody a descriptor.
    //
    // **What P4e-i changed under this test, without changing what it asserts.** The refusal
    // used to be a `stat` of the *name*; it is now an `O_NONBLOCK` open plus an `fstat` of
    // the *descriptor*, which is the same verdict reached in a way a client cannot step
    // through — the sibling test below is the one that can tell the two apart. The fifo arm
    // is also stronger than it reads: without `O_NONBLOCK` the open does not return, so a
    // build that lost the flag turns this into a named nextest `TIMEOUT` rather than a wrong
    // answer.
    let fixture = Fixture::start();
    let camera = camera(&fixture.cameras, 0).id.clone();
    let scratch = TempRuntimeDir::new().expect("a throw-away directory");
    let (_, wire) = fixture
        .wires()
        .into_iter()
        .next()
        .expect("two transports were built");
    assert_eq!(fixture.backend.opens(), 0, "the fixture opened a camera");

    // The fifo, and a directory beside it — one is the wedge and the other is the ordinary
    // typo, and both are "that path is not a file this daemon may write".
    let fifo = scratch.base().join("wedge.jpg");
    mkfifo(&fifo);
    let directory = scratch.base().join("a-directory.jpg");
    std::fs::create_dir(&directory).expect("a writable scratch directory");

    for (destination, what) in [(&fifo, "fifo"), (&directory, "directory")] {
        let (code, error) = refusal(
            wire.photo(
                camera.clone(),
                photo_request(Sink::ServerPath {
                    path: destination.clone(),
                }),
            )
            .await,
        );
        assert_eq!(code, rpc_code(ErrorKind::IllegalTransition), "{what}");
        assert_eq!(error.kind(), ErrorKind::IllegalTransition, "{what}");
        assert!(error.to_string().contains(destination.as_str()), "{error}");
        assert!(error.to_string().contains(what), "{error}");
        // Not the disk declining to be written and not the camera declining to offer a
        // format: neither was asked (E3).
        assert_ne!(error.kind(), ErrorKind::StorageIo, "{what}");
        assert_ne!(error.kind(), ErrorKind::FormatUnsupported, "{what}");
    }

    // Nothing opened, which is the half that says the camera was never at risk — and the
    // half a build that blocked in `open(2)` could not reach, because this line would never
    // run.
    assert_eq!(
        fixture.backend.opens(),
        0,
        "a request that was always going to be refused opened a camera"
    );

    // And the camera is still usable afterwards, which is the claim the wedge would break:
    // one actor, one descriptor, answering the next request rather than queued behind an
    // `open` nobody will ever satisfy.
    let after = scratch.base().join("after.jpg");
    let answer: PhotoResponse = wire
        .photo(
            camera,
            photo_request(Sink::ServerPath {
                path: after.clone(),
            }),
        )
        .await
        .expect("the camera is still answering");
    assert!(answer.bytes_match_the_delivery());
    assert!(after.exists(), "{after}");
    assert_eq!(fixture.backend.opens(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_path_swapped_after_the_check_cannot_redirect_the_photo_or_park_the_camera() {
    // Note **N51**'s *remaining* ground, which is what P4e-i discharges. The refusal above
    // has been there since P4c and a `stat`-then-`open` passes it: the daemon looked at the
    // name, saw a regular file, opened a camera, and only then resolved the same name a
    // second time — so a client that replaced the path in between won the race, and the
    // second resolution blocked in `open(2)` on a fifo forever with the camera's one thread
    // inside it. N51 called that "shipping half of it would trade a wedge for a leak".
    //
    // The fix is to make the *descriptor* the destination, resolved once and before any
    // camera is touched. This test is the difference between the two builds, and it is a
    // positive observation rather than the absence of a hang: the bytes are found in the
    // inode this daemon checked, under the name the client moved it to.
    //
    // Nothing here sleeps. The swap happens while the camera is provably inside a frame,
    // announced by the `Holding` decorator, and the release is this test speaking.
    //
    // The red-on-inverse, watched before this was called done: with the daemon's destination
    // replaced by one that resolves the *name* at write time
    // (`engine::photo::WhereverTheCallerSaid`, which is the right answer for
    // `webcam-handler-cli` and the wrong one here), the write blocks in `open(2)` on the fifo
    // below and the run becomes a named `TIMEOUT` (`.config/nextest.toml`, 60 s x 3) — which
    // is this test's name on a failure, and is also exactly the harm note N51 measured.
    let (entered, entrances) = std::sync::mpsc::sync_channel::<()>(1);
    let (release, held) = std::sync::mpsc::sync_channel::<()>(1);
    let fixture = Fixture::start_behind(|fake| {
        Arc::new(Holding {
            inner: Arc::clone(fake),
            entered: std::sync::Mutex::new(entered),
            release: std::sync::Mutex::new(Some(held)),
        }) as Arc<dyn CameraBackend>
    });
    let camera = camera(&fixture.cameras, 0).id.clone();
    let scratch = TempRuntimeDir::new().expect("a throw-away directory");

    // The destination the client names, existing and ordinary at the moment it is checked.
    let named = scratch.base().join("shot.jpg");
    std::fs::write(&named, b"an operator's earlier photo").expect("a writable scratch directory");

    let (_, wire) = fixture
        .wires()
        .into_iter()
        .next()
        .expect("two transports were built");
    let request = photo_request(Sink::ServerPath {
        path: named.clone(),
    });
    let taking = tokio::spawn(async move { wire.photo(camera.clone(), request).await });

    // The daemon has opened the destination and is now inside the frame. A blocking `recv`
    // on the pool rather than a sleep: it ends when the device says so.
    tokio::task::spawn_blocking(move || entrances.recv())
        .await
        .expect("the blocking pool is alive")
        .expect("the photo reached the device");

    // The swap: the file that was checked moves aside, and a fifo takes its name. A build
    // that resolved the name again would open *this* and never come back.
    let moved = scratch.base().join("moved.jpg");
    std::fs::rename(&named, &moved).expect("the checked file can be renamed");
    mkfifo(&named);

    drop(release);
    let answer: PhotoResponse = taking
        .await
        .expect("the photo task")
        .expect("the photo answered, so nothing opened the fifo");
    assert!(answer.bytes_match_the_delivery());

    // The bytes are in the inode this daemon checked — under its new name, because that is
    // where the client put it. Nothing else could be true: the descriptor was resolved once.
    let written = std::fs::read(&moved).expect("the checked file holds the photo");
    assert_eq!(
        u64::try_from(written.len()).expect("a photo fits"),
        answer.report.delivery.byte_count(),
        "the photo did not land in the file this daemon checked"
    );
    assert_ne!(
        written, b"an operator's earlier photo",
        "the destination was never written"
    );

    // And the fifo is untouched — still a fifo, never opened, never written. The answer's
    // `delivery` still reports the path the *request* named, which is the honest thing to
    // say: a client that moves its own destination mid-capture is told where it asked for
    // the photo, not where it then put the file.
    let replacement = std::fs::symlink_metadata(&named).expect("the fifo is still there");
    assert!(
        std::os::unix::fs::FileTypeExt::is_fifo(&replacement.file_type()),
        "{named}"
    );
    let PhotoDelivery::Path { path: reported, .. } = &answer.report.delivery else {
        panic!("a path sink must report a path");
    };
    assert_eq!(reported, &named);

    // The camera is still usable, which is the claim the wedge would break.
    let (_, wire) = fixture
        .wires()
        .into_iter()
        .next()
        .expect("two transports were built");
    let after = scratch.base().join("after.jpg");
    wire.photo(
        camera_id(&fixture),
        photo_request(Sink::ServerPath {
            path: after.clone(),
        }),
    )
    .await
    .expect("the camera is still answering");
    assert!(after.exists(), "{after}");
    assert_eq!(fixture.backend.opens(), 1, "one camera, one descriptor");
}

#[tokio::test(flavor = "multi_thread")]
async fn d12s_wait_flag_crosses_the_wire_both_ways_and_neither_spelling_changes_the_photo() {
    // D12: "a second capture request queues or is refused with `Busy` **per its `wait`
    // flag**". The field is new on a committed wire shape (note **N42**'s first item), and
    // what is asserted here is that the daemon *reads* it: the two spellings, sent at the
    // same moment to the same busy camera, get different answers.
    //
    // **An earlier version of this test could not tell them apart, and said it could** (note
    // **N59**). It sent both values behind a single held command, where the queue is eight
    // deep and both were simply enqueued — so `wch_photo` ignoring `request.wait` entirely
    // (`enqueueing(false)`) passed all 861 tests in the workspace, measured. The missing
    // ingredient was not a second value; it was a **full queue**, and the missing signal was
    // "somebody is parked waiting for a seat", which the daemon now publishes
    // (`Wchd::watch_waiting_captures`) because `limits::CAMERA_ENQUEUE_WAITERS` needs a
    // reader anyway.
    //
    // With it, every wait below is a signal: the decorator announces that the device is
    // occupied, a `Busy` answer announces that the queue is full, and the waiting count
    // announces that the `wait: true` request is parked in it. Nothing sleeps, and the
    // sixty-second-shaped risk of "assert it has not answered yet" is not taken at all —
    // what is compared is two *outcomes*, not two timings.
    //
    // The bound on the queue itself stays where it is exact (`engine::actor`, note N42's
    // division). What is new here is the wire's own half: the flag arrives, both ways, and
    // it changes which of D12's two answers a request gets.
    let (entered, entrances) = std::sync::mpsc::sync_channel::<()>(1);
    let (release, held) = std::sync::mpsc::sync_channel::<()>(1);
    let fixture = Fixture::start_behind(|fake| {
        Arc::new(Holding {
            inner: Arc::clone(fake),
            entered: std::sync::Mutex::new(entered),
            release: std::sync::Mutex::new(Some(held)),
        }) as Arc<dyn CameraBackend>
    });
    let camera = camera(&fixture.cameras, 0).id.clone();
    let bytes = photo_request(Sink::ReturnBytes {
        format: PhotoFormat::Jpeg,
    });
    assert!(!bytes.wait, "the default is the shape older clients send");

    let (_, wire) = fixture
        .wires()
        .into_iter()
        .next()
        .expect("two transports were built");
    let holder = wire.clone();
    let holding = {
        let (camera, request) = (camera.clone(), bytes.clone());
        tokio::spawn(async move { holder.photo(camera, request).await })
    };

    // The actor's one thread is now inside the first photo's `next_frame`.
    tokio::task::spawn_blocking(move || entrances.recv())
        .await
        .expect("the blocking pool is alive")
        .expect("the first photo reached the device");
    assert_eq!(fixture.backend.streams_started(), 1);

    // Fill the camera's command queue while its one thread is held. The queue is
    // `CAMERA_COMMAND_QUEUE_DEPTH` deep and the held photo is *running* rather than queued,
    // so exactly that many of these take seats and the rest are refused — a count, not a
    // guess, and it stays true because nothing can drain while the thread is held.
    let seats = schema::limits::CAMERA_COMMAND_QUEUE_DEPTH;
    let (refusals, mut turned_away) = tokio::sync::mpsc::channel::<ErrorKind>(seats * 2);
    for _ in 0..seats * 2 {
        let (wire, camera, request) = (wire.clone(), camera.clone(), bytes.clone());
        let refusals = refusals.clone();
        tokio::spawn(async move {
            // Only the refusals are reported: a filler that took a seat answers when the
            // device is released, which is minutes of camera time away in a real daemon and
            // is not what this rendezvous is about.
            if let Err(err) = wire.photo(camera, request).await {
                let _ = refusals
                    .send(refusal::<PhotoResponse>(Err(err)).1.kind())
                    .await;
            }
        });
    }
    drop(refusals);
    for refused in 0..seats {
        let kind = turned_away
            .recv()
            .await
            .unwrap_or_else(|| panic!("only {refused} of the fillers past the seats were refused"));
        assert_eq!(
            kind,
            ErrorKind::Busy,
            "a full command queue refused something other than D12's `Busy`"
        );
    }

    // Now the two spellings, at a camera whose queue is provably full. `wait: true` parks —
    // which the daemon publishes, so the release below happens *after* it has provably done
    // so — and `wait: false` takes D12's refusal immediately.
    let mut parked = fixture.wchd.watch_waiting_captures();
    let waiting = {
        let (wire, camera) = (wire.clone(), camera.clone());
        let request = PhotoRequest {
            wait: true,
            ..bytes.clone()
        };
        tokio::spawn(async move { wire.photo(camera, request).await })
    };
    parked
        .wait_for(|parked| *parked == 1)
        .await
        .expect("the daemon publishes how many captures are waiting");

    let (_, refused) = refusal(
        wire.photo(
            camera.clone(),
            PhotoRequest {
                wait: false,
                ..bytes.clone()
            },
        )
        .await,
    );
    assert_eq!(
        refused.kind(),
        ErrorKind::Busy,
        "`wait: false` waited instead of taking D12's refusal: {refused}"
    );

    // Released, and the difference is the assertion: the request that asked to wait is
    // *served*, where the one that did not was refused at the same instant, at the same
    // camera, over the same wire.
    drop(release);
    let answer: PhotoResponse = waiting
        .await
        .expect("the waiting request's task")
        .expect("`wait: true` was refused where it should have queued");
    assert!(answer.bytes_match_the_delivery());
    holding
        .await
        .expect("the held request's task")
        .expect("the photo that was holding the device");

    // One descriptor throughout: the flag chose how to *enqueue* and changed nothing about
    // what the camera did. The stream count is the held photo, the seated fillers and the
    // waiter — every request that was not refused took exactly one.
    assert_eq!(
        fixture.backend.streams_started(),
        u64::try_from(seats + 2).expect("a small bound")
    );
    assert_eq!(fixture.backend.opens(), 1);
}

/// The camera every photo in this file names, for a test that needed it twice.
fn camera_id(fixture: &Fixture) -> CameraId {
    camera(&fixture.cameras, 0).id.clone()
}

#[tokio::test]
async fn an_empty_camera_id_is_refused_rather_than_naming_the_only_camera() {
    // Note **N50**, over the wire this sub-milestone put it on. `CameraId` is a
    // `#[serde(transparent)]` newtype and its derived `Deserialize` accepted any string,
    // while resolution is a *prefix* match by design — so `{"camera": ""}` was a prefix of
    // every id and resolved silently to the only camera on a one-camera host. P4c is the
    // commit that made that reachable on `wch_set`, `wch_photo`, `wch_terminate_holder` and
    // the calibrate verbs, which is why the refusal is asserted here rather than only in
    // `crates/schema`.
    //
    // Raw JSON over the real socket, because the generated client takes a typed `CameraId`
    // and cannot produce the request this is about. That is the whole point of the defect:
    // only a hand-written client could send it.
    let fixture = Fixture::start();
    assert_eq!(fixture.backend.opens(), 0, "the fixture opened a camera");

    for method in ["wch_info", "wch_snapshot", "wch_discover_pairs"] {
        let request =
            format!(r#"{{"jsonrpc":"2.0","id":1,"method":"{method}","params":{{"camera":""}}}}"#);
        let answer = support::call(&fixture.socket, &request)
            .await
            .expect("the daemon is serving");
        let document: serde_json::Value =
            serde_json::from_str(support::body(&answer)).expect("a JSON-RPC document");
        // `-32602` is jsonrpsee's invalid-params, which is what a parameter that does not
        // deserialize *is*: the request never became a call, so no D13 variant is the right
        // answer and inventing one would be this surface claiming the machine had an opinion
        // about a camera nobody named.
        assert_eq!(
            document["error"]["code"].as_i64(),
            Some(-32602),
            "{method}: {document}"
        );
        assert!(
            document.get("result").is_none(),
            "{method} answered about a camera the caller did not name: {document}"
        );
    }

    // The camera is what makes it a defect rather than a curiosity: a build that resolved
    // `""` would have opened one by now.
    assert_eq!(
        fixture.backend.opens(),
        0,
        "an empty camera id reached a device"
    );

    // The twin, so the arm above refuses the empty id rather than refusing every id: a
    // shortest-possible *non-empty* prefix still resolves, which is D1's rule and the reason
    // the empty one was reachable at all.
    let asked = fixture.ask();
    let (_, wire) = fixture
        .wires()
        .into_iter()
        .next()
        .expect("two transports were built");
    wire.info(asked.camera).await.expect("a camera it names");
}

#[tokio::test]
async fn every_refusal_the_photo_path_can_make_crosses_the_wire_as_itself() {
    // The three AGENTS rule 7 keeps apart on this verb, plus the sink's own. `photo` is the
    // first routed verb that starts a stream, so it is the first place `SettleTimeout` and
    // `DeviceGone` have a producer at all — and the first place they could be confused with
    // each other, since both arrive from `next_frame`.
    let fixture = Fixture::start();
    let camera = camera(&fixture.cameras, 0).id.clone();
    let scratch = TempRuntimeDir::new().expect("a throw-away directory");
    let (_, wire) = fixture
        .wires()
        .into_iter()
        .next()
        .expect("two transports were built");
    let bytes = || {
        photo_request(Sink::ReturnBytes {
            format: PhotoFormat::Jpeg,
        })
    };

    // A format the device does not list at all. The camera *is* saying what it cannot
    // offer, so this one is `FormatUnsupported` and the answer names what there is.
    let (code, error) = refusal(
        wire.photo(
            camera.clone(),
            PhotoRequest {
                stream: StreamRequest {
                    pixel_format: Some(PixelFormat::NV12),
                    ..StreamRequest::default()
                },
                ..bytes()
            },
        )
        .await,
    );
    assert_eq!(code, rpc_code(ErrorKind::FormatUnsupported));
    let Error::FormatUnsupported {
        requested,
        available,
        // The format half, so neither a size refusal (note **N138**) nor a container one
        // (note **N211**) can answer for it — and `photo` names no container at all.
        size: None,
        container: None,
    } = &error
    else {
        panic!("{error:?}");
    };
    assert_eq!(*requested, Some(PixelFormat::NV12));
    assert!(available.contains(&PixelFormat::MJPG), "{available:?}");

    // A frame that did not arrive before the caller's deadline. Not `DeviceGone`: a slow
    // sensor is still there.
    //
    // The deadline is zero and the fault is queued, and both are load-bearing: the fake
    // reports a spent deadline rather than sleeping through one (nothing here waits for a
    // wall clock), and `frames_seen` is what distinguishes "no frame arrived" from "a frame
    // arrived after the deadline had passed" — so a fault that stopped firing would leave
    // this arm timing out for the other reason and the count is what notices.
    fixture.backend.queue_fault(fake::Fault::FrameTimeout);
    let (code, timed_out) = refusal(
        wire.photo(
            camera.clone(),
            PhotoRequest {
                settle: SettlePolicy {
                    spec: SettleSpec::SkipFrames { frames: 0 },
                    deadline_ms: 0,
                },
                ..bytes()
            },
        )
        .await,
    );
    assert_eq!(code, rpc_code(ErrorKind::SettleTimeout));
    let Error::SettleTimeout { frames_seen, .. } = &timed_out else {
        panic!("{timed_out:?}");
    };
    assert_eq!(*frames_seen, 0, "a frame arrived after all");
    assert_ne!(timed_out.kind(), ErrorKind::DeviceGone);

    // A device that vanished mid-stream. Not `SettleTimeout`: nothing is coming.
    fixture
        .backend
        .queue_fault(fake::Fault::DeviceGoneMidStream);
    let (code, gone) = refusal(wire.photo(camera.clone(), bytes()).await);
    assert_eq!(code, rpc_code(ErrorKind::DeviceGone));
    assert_eq!(gone.kind(), ErrorKind::DeviceGone);
    assert_ne!(gone.kind(), ErrorKind::SettleTimeout);

    // A path the filesystem refused. The capture succeeded and the photo did not land, and
    // reporting success would be the worst of both — a caller would believe there is a file.
    let unwritable = scratch.base().join("no-such-directory/shot.jpg");
    let (code, storage) = refusal(
        wire.photo(
            camera.clone(),
            photo_request(Sink::ServerPath {
                path: unwritable.clone(),
            }),
        )
        .await,
    );
    assert_eq!(code, rpc_code(ErrorKind::StorageIo));
    assert!(storage.to_string().contains("shot.jpg"), "{storage}");
    assert!(!unwritable.exists());

    // Four refusals, four codes, so none of the assertions above is passing on a registry
    // that had collapsed to one error.
    let codes: std::collections::BTreeSet<i32> = [
        rpc_code(ErrorKind::FormatUnsupported),
        rpc_code(ErrorKind::SettleTimeout),
        rpc_code(ErrorKind::DeviceGone),
        rpc_code(ErrorKind::StorageIo),
    ]
    .into_iter()
    .collect();
    assert_eq!(codes.len(), 4, "{codes:?}");

    // And the camera still answers, so every refusal above was the fault it was given
    // rather than a daemon that had stopped working.
    wire.photo(camera, bytes())
        .await
        .expect("nothing is wrong with this camera");
}

// ------------------------------------------------------ D12: one streamer per node

/// A backend whose first frame is held until the test lets go.
///
/// A decorator over the real fake, in `daemon::server`'s `Announcing` tradition and for the
/// same reason: blocking on command is this test's synchronisation, not a capability any
/// device has, and a `FakeBackend` that could be told to block would be claiming something
/// no replayed profile does — which AGENTS calls a bug in the fake.
///
/// It is not a sleep. The hold ends when another thread says so, and the announcement is
/// what tells that thread the device is provably occupied.
#[derive(Debug)]
struct Holding {
    inner: Arc<fake::FakeBackend>,
    entered: std::sync::Mutex<std::sync::mpsc::SyncSender<()>>,
    /// Taken by the first camera this backend opens, so exactly one frame is ever held.
    release: std::sync::Mutex<Option<std::sync::mpsc::Receiver<()>>>,
}

/// One open camera, forwarding everything and holding its first frame.
#[derive(Debug)]
struct Held {
    camera: Box<dyn Camera>,
    entered: std::sync::mpsc::SyncSender<()>,
    release: Option<std::sync::mpsc::Receiver<()>>,
}

/// A poisoned lock here means a test thread panicked holding a channel, which is not a
/// reason to replace a useful failure with a confusing one.
fn lock<T>(mutex: &std::sync::Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl CameraBackend for Holding {
    fn kind(&self) -> schema::backend::BackendKind {
        self.inner.kind()
    }

    fn enumerate(&self) -> schema::Result<Vec<schema::camera::CameraInfo>> {
        self.inner.enumerate()
    }

    fn open(&self, id: &CameraId) -> schema::Result<Box<dyn Camera>> {
        let camera = self.inner.open(id)?;
        Ok(Box::new(Held {
            camera,
            entered: lock(&self.entered).clone(),
            release: lock(&self.release).take(),
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
        self.camera.set(id, value)
    }

    fn start_stream(
        &mut self,
        request: &StreamRequest,
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
        if let Some(release) = self.release.take() {
            // Announce first, then block: the test's next line depends on this camera
            // being *inside* the frame it is about to wait for.
            let _ = self.entered.send(());
            // Ends when the test drops its sender, never when a duration passes.
            let _ = release.recv();
        }
        self.camera.next_frame(deadline)
    }

    fn stop_stream(&mut self) -> schema::Result<()> {
        self.camera.stop_stream()
    }
}

#[tokio::test]
async fn a_photo_while_a_photo_is_in_flight_shares_one_descriptor_and_never_streams_twice() {
    // D12: "the actor enforces exclusive streaming (V4L2 allows one streamer per node)".
    // The fake models the kernel's half — a second `start_stream` while one is running is
    // `Error::Busy`, which `fake`'s own resemblance suite pins in both directions — so
    // *both photos answering* is the assertion, and it is one a daemon that reached past
    // the actor could not pass: `Inner` deliberately holds no backend handle, and a handler
    // that opened its own would put a second descriptor on the node and get that refusal.
    //
    // What this cannot observe is the instant the second request joined the queue; what it
    // does observe is that the first is provably inside `next_frame` when the second is
    // sent, that one descriptor served both, and that the two streams were sequential.
    let (entered, entrances) = std::sync::mpsc::sync_channel::<()>(1);
    let (release, held) = std::sync::mpsc::sync_channel::<()>(1);
    let fixture = Fixture::start_behind(|fake| {
        Arc::new(Holding {
            inner: Arc::clone(fake),
            entered: std::sync::Mutex::new(entered),
            release: std::sync::Mutex::new(Some(held)),
        }) as Arc<dyn CameraBackend>
    });
    let camera = camera(&fixture.cameras, 0).id.clone();
    let request = photo_request(Sink::ReturnBytes {
        format: PhotoFormat::Jpeg,
    });

    let (_, first_wire) = fixture
        .wires()
        .into_iter()
        .next()
        .expect("two transports were built");
    let second_wire = first_wire.clone();
    let (first_camera, second_camera) = (camera.clone(), camera.clone());
    let (first_request, second_request) = (request.clone(), request.clone());

    let first = tokio::spawn(async move { first_wire.photo(first_camera, first_request).await });

    // The actor's one thread is now inside the first photo's `next_frame`. A blocking recv
    // on the pool rather than a sleep: it ends when the device says so.
    tokio::task::spawn_blocking(move || entrances.recv())
        .await
        .expect("the blocking pool is alive")
        .expect("the first photo reached the device");
    assert_eq!(
        fixture.backend.streams_started(),
        1,
        "a second stream started while the first was still running"
    );

    let second =
        tokio::spawn(async move { second_wire.photo(second_camera, second_request).await });

    // Let go. Everything queued behind the first photo runs now, one command at a time.
    drop(release);
    let first = first
        .await
        .expect("the first request's task")
        .expect("the first photo");
    let second = second
        .await
        .expect("the second request's task")
        .expect("the second photo — a second streamer would have been refused Busy");

    assert!(first.bytes_match_the_delivery());
    assert!(second.bytes_match_the_delivery());
    // Two streams, and one descriptor: the second request reached the same actor rather
    // than a second `open` on the same node.
    assert_eq!(fixture.backend.streams_started(), 2);
    assert_eq!(fixture.backend.opens(), 1);
    assert_eq!(fixture.backend.closes(), 0);
}

// ------------------------------------------------- `terminate_holder`, the explicit kill
//
// AGENTS.md, "Hardware and privacy": *"Killing a process that holds the camera is an
// explicit command naming its target, never a fallback."* Design §5 adds the two halves
// this section is about: the command "names both the camera and the pid, refuses if the
// pid no longer holds the device, and is never a fallback behavior of anything else".
//
// ## Everything here is real except the camera
//
// The holder walk is the real `/proc/*/fd` walk, the signal is a real `SIGTERM`, and the
// process that receives it is a **child this test forked and is willing to lose** — the
// same construction `crates/engine/tests/crash_recovery.rs` uses for the same reason, and
// with the same synchronisation: the child announces on a pipe and blocks reading another,
// so nothing here sleeps and nothing guesses when it is ready.
//
// The camera is the fake, with one field of the committed profile rewritten: its capture
// node points at a file inside the test's own throw-away directory rather than at
// `/dev/videoN`. That rewrite is the whole reason this is safe to run on a developer's
// laptop. `FakeBackend` replays real-looking node paths, and on a machine with a webcam
// `/dev/video0` is a path some *real* program may hold — a suite that pointed the verb at
// one would be a suite that signals a stranger's video call. The walk compares a symlink
// target as a string (`webcam-handler-v4l2::holders`), so a plain file under `$TMPDIR` is a
// node for its purposes, and its own unit suite already proves the walk works on one.
//
// ## What each direction is worth
//
// - **`HolderGone`** is the easy direction and the one a suite could fake: over the fake
//   backend nothing holds anything, so *every* call would answer it and a test that only
//   asserted that would be asserting a vacuum. So the refusal is asserted against a pid
//   that is definitely alive and definitely not a holder — **this test process's own** —
//   which is also the only pid it is safe to be wrong about: a build that signalled it
//   would take the test runner down rather than pass quietly.
// - **The `TerminationReport`** is the direction that needs a real holder, a real signal
//   and a real death, and it asserts all three: the answer names the target back, the
//   child's exit status *is* a signal, and `still_held` says the node is free.
// - **`still_held: true`** is not an error and must not be rendered as one (E4, and the
//   field's own doc). It is arranged by a second holder this test owns — itself — so the
//   arm is deterministic rather than a race against a process that might ignore `SIGTERM`.
// - **Never a fallback** is the claim with no positive evidence, so it is asserted as an
//   absence with a witness: a camera refused `Busy` through four other verbs, and the
//   child still holding the node afterwards.
//
// ## The race this cannot close, and does not claim to
//
// Between the re-verification and `kill(2)` the holder can exit, be reaped, and have its
// pid recycled onto an unrelated process. Note **N48** prices what bounds it. Nothing here
// tests it, because a test that could arrange it would be a test that signals a stranger.

/// The file the child is to open and hold.
const NODE_ENV: &str = "WCH_HOLDER_NODE";
/// The line the child prints once it has the node open.
const READY: &str = "wch-holder-child: the node is open";
/// The test the child runs, named the way libtest selects it.
const CHILD_TEST: &str = "a_process_that_opens_a_node_is_found_holding_it";

// ------------------------------------------------------------------ the child, as a process

/// Open the node, say so, and wait to be signalled — or, with nothing pointing at a node,
/// assert the mechanism the child half depends on.
///
/// Two arms in one test for `crash_recovery.rs`'s reason: the child is this binary told to
/// run one test by name, so the test has to exist and has to be honest when it is run by
/// nextest instead. The in-process arm is not decoration — it is the claim every other test
/// in this file rests on, that a process holding an ordinary file is found by the same walk
/// a `Busy` refusal names holders with.
#[test]
fn a_process_that_opens_a_node_is_found_holding_it() {
    let me = i32::try_from(std::process::id()).expect("a pid fits");

    if let Ok(node) = std::env::var(NODE_ENV) {
        let node = Utf8PathBuf::from(node);
        let _held = std::fs::File::open(&node).expect("the parent created the node");
        assert!(v4l2::holders::holds(me, &node), "{node}");

        let mut out = std::io::stdout();
        writeln!(out, "{READY}").expect("the parent is listening");
        out.flush().expect("the parent is listening");

        // Blocking on a pipe rather than on a clock: no sleep is a synchronisation, and
        // reading stdin also means a parent that died without signalling us closes the pipe
        // and lets us go instead of leaving a process behind. `SIGTERM`'s default
        // disposition ends this line, which is what the parent is asserting.
        let mut ignored = String::new();
        let _ = std::io::stdin().read_line(&mut ignored);
        return;
    }

    let scratch = TempRuntimeDir::new().expect("a throw-away directory");
    let node = scratch.base().join("node");
    std::fs::write(&node, b"").expect("a writable scratch directory");
    assert!(!v4l2::holders::holds(me, &node), "nothing is open yet");

    let held = std::fs::File::open(&node).expect("the file exists");
    assert!(v4l2::holders::holds(me, &node), "{node}");
    assert!(
        v4l2::holders::of(&node)
            .iter()
            .any(|holder| holder.pid == me),
        "the walk that names a Busy refusal's holders did not name this process"
    );

    drop(held);
    assert!(
        !v4l2::holders::holds(me, &node),
        "a closed file still reads as held"
    );
}

/// Start a child holding `node`, and wait until it says so.
fn spawn_holder(node: &Utf8Path) -> Child {
    let binary = std::env::current_exe().expect("this test binary has a path");
    let mut child = Command::new(binary)
        // libtest's own selection, so the child runs exactly the arm above. A name that
        // matched nothing would exit 0 with no output, which `wait_for_ready` reports as a
        // failure rather than blocking on.
        .args(["--exact", CHILD_TEST, "--nocapture", "--test-threads=1"])
        .env(NODE_ENV, node.as_str())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("the test binary is executable");

    let stdout = child.stdout.take().expect("stdout was piped");
    let mut transcript = Vec::new();
    for line in BufReader::new(stdout).lines() {
        let Ok(line) = line else { break };
        if line.contains(READY) {
            return child;
        }
        transcript.push(line);
    }
    // Nothing is left holding the node, so the tests below would refuse rather than
    // misfire — but a suite that carried on from here would be asserting a vacuum. Killed
    // *and reaped* before the panic: a test binary that left a zombie behind would be
    // leaking a process to report that it could not talk to one.
    let _ = child.kill();
    let _ = child.wait();
    panic!(
        "the child ended without opening the node; it printed:\n{}",
        transcript.join("\n")
    );
}

/// The child's pid, as the wire spells one.
fn pid_of(child: &Child) -> i32 {
    i32::try_from(child.id()).expect("a pid fits")
}

// ------------------------------------------------------------------ the camera, doctored

/// The committed profile with its capture node pointed at a path this test owns.
///
/// `second_camera`'s licence, applied to the one field that decides which inode the holder
/// walk compares against. A profile is immutable corpus and this does not touch it — the
/// value is doctored in the test, which is the difference between "a device we made up" and
/// "the device we have, asked about somewhere safe".
fn camera_whose_node_is(path: &Utf8Path) -> DeviceProfile {
    let mut profile = testkit::fixtures::synthetic_basic();
    let node = profile
        .invariant
        .info
        .nodes
        .iter_mut()
        .find(|node| node.kind == schema::camera::NodeKind::VideoCapture)
        .expect("the synthetic profile has a capture node");
    node.path = path.to_owned();
    profile
}

/// A daemon whose first camera's capture node is `path`, plus a second camera so that
/// `Fixture::ask`'s derived ambiguous prefix still has two candidates.
fn daemon_over(path: &Utf8Path) -> Fixture {
    Fixture::replaying(vec![camera_whose_node_is(path), second_camera()], |fake| {
        Arc::clone(fake) as Arc<dyn CameraBackend>
    })
}

/// A scratch directory with an empty file in it, standing in for a device node.
fn scratch_node() -> (TempRuntimeDir, Utf8PathBuf) {
    let scratch = TempRuntimeDir::new().expect("a throw-away directory");
    let node = scratch.base().join("video-under-test");
    std::fs::write(&node, b"").expect("a writable scratch directory");
    (scratch, node)
}

/// The first camera and the first wire, which every test here uses.
fn ask(fixture: &Fixture) -> (CameraId, Wire) {
    let camera = camera(&fixture.cameras, 0).id.clone();
    let (_, wire) = fixture
        .wires()
        .into_iter()
        .next()
        .expect("two transports were built");
    (camera, wire)
}

// ------------------------------------------------------------------------------- the tests

#[tokio::test]
async fn a_pid_that_does_not_hold_the_device_is_refused_and_nothing_is_signalled() {
    // Design §5's refusal, and the one this verb exists to make impossible to skip:
    // "signalling a pid that has been recycled would kill somebody else's process".
    //
    // The pid is **this test process's own**, which is the strongest target available and
    // the only safe one. It is alive, it is signallable, and it does not hold the node — so
    // a build that answered anything but `HolderGone` would either signal a live process
    // (this one, loudly) or invent a reason not to. A pid that named nothing at all would
    // have let a build that never signals anything pass.
    let (_scratch, node) = scratch_node();
    let fixture = daemon_over(&node);
    let (camera, wire) = ask(&fixture);
    let me = i32::try_from(std::process::id()).expect("a pid fits");
    assert!(
        !v4l2::holders::holds(me, &node),
        "this test holds the node it is about to claim it does not"
    );

    let (code, error) = refusal(wire.terminate_holder(camera.clone(), me).await);
    assert_eq!(code, rpc_code(ErrorKind::HolderGone));
    assert_eq!(error.kind(), ErrorKind::HolderGone);
    assert!(error.to_string().contains(&me.to_string()), "{error}");
    // Not a device answer and not a permission answer: the camera was never asked and
    // neither was the kernel's uid check (E3).
    assert_ne!(error.kind(), ErrorKind::DeviceIo);
    assert_ne!(error.kind(), ErrorKind::PermissionDenied);

    // Still here. `kill(2)` is synchronous in the sense that matters — the signal is queued
    // before it returns — so a process that was going to be signalled by that call has been
    // by the time the refusal arrives, and this line runs on the process that would have
    // received it.
    assert_eq!(
        i32::try_from(std::process::id()).expect("a pid fits"),
        me,
        "this test process was replaced, which is not a thing that happens"
    );

    // And the two ways of naming a camera wrongly still refuse for their own reasons, so
    // the verb resolves before it diagnoses.
    //
    // The descriptor count is read *now* rather than compared against zero: `Fixture::ask`
    // takes its control off the device, which costs a `FakeBackend::opens()` of its own —
    // the caveat that type's own doc carries. What the assertion below is about is the
    // verb, so the baseline is whatever the fixture spent before it ran.
    let asked = fixture.ask();
    let opens_before = fixture.backend.opens();
    for (wrong, kind) in [
        (asked.unknown_camera, ErrorKind::CameraUnknown),
        (asked.ambiguous, ErrorKind::CameraAmbiguous),
    ] {
        let (_, error) = refusal(wire.terminate_holder(wrong, me).await);
        assert_eq!(error.kind(), kind);
    }
    // Nothing about this verb opens a camera: the thing it acts on is a process.
    assert_eq!(fixture.backend.opens(), opens_before);
}

#[tokio::test]
async fn a_holder_the_caller_named_is_signalled_and_the_answer_names_it_back() {
    // The whole verb, end to end, against a process this test forked and is willing to
    // lose. Every assertion is about something that actually happened: a real walk found a
    // real holder, a real `SIGTERM` reached it, and the child's exit status *is* a signal —
    // so a child that had simply finished on its own could not be mistaken for one that was
    // signalled.
    let (_scratch, node) = scratch_node();
    let fixture = daemon_over(&node);
    let (camera, wire) = ask(&fixture);

    let mut child = spawn_holder(&node);
    let pid = pid_of(&child);
    assert!(
        v4l2::holders::holds(pid, &node),
        "the child announced but the walk cannot see it"
    );

    let report: TerminationReport = wire
        .terminate_holder(camera.clone(), pid)
        .await
        .expect("the child holds the node");

    // The answer names the target back — the camera the caller named, and the holder as it
    // was diagnosed *before* the signal, in the same `Holder` type an `Error::Busy` refusal
    // carries so that both verbs name a process the same way.
    assert_eq!(report.camera, camera);
    assert_eq!(report.holder.pid, pid);
    assert!(
        report.holder.comm.is_some_and(|comm| !comm.is_empty()),
        "the holder was named by a bare number"
    );
    // One signal, chosen by the product: the vocabulary has one member, so `SIGKILL` is not
    // representable and no wire field could ask for it.
    assert_eq!(report.signal, TerminationSignal::Term);
    assert_eq!(TerminationSignal::ALL, &[TerminationSignal::Term]);
    // **`still_held` is deliberately not asserted here.** Answering `false` requires the
    // forked child to have received `SIGTERM` and left `/proc/<pid>/fd` inside
    // `limits::TERMINATE_RECHECK_MS` of *wall* clock, which is a race against the scheduler
    // on a loaded runner and would be the one duration-dependent assertion in a workspace
    // whose rule is that nothing synchronises on a sleep. Both directions of the field are
    // pinned where the clock is an argument instead — `daemon::server`'s
    // `a_still_held_answer_is_immediate_when_nothing_holds_the_node_and_bounded_when_something_does`,
    // on a paused clock — and the `true` direction is arranged deterministically over the
    // wire below, in `a_node_somebody_else_still_holds_answers_still_held_rather_than_an_error`.
    // What this test is about is the signal, and the next two lines are that.

    let status = child.wait().expect("the child was spawned");
    assert_eq!(
        status.signal(),
        Some(libc_sigterm()),
        "the child ended some other way: {status:?}"
    );
    assert!(!v4l2::holders::holds(pid, &node));

    // And the second call is the refusal, because the pid the caller named is gone. Running
    // it twice is not idempotent and must not be: the second call is about a pid that no
    // longer holds anything, and answering `Ok` would be answering about a process that
    // does not exist.
    let (_, again) = refusal(wire.terminate_holder(camera, pid).await);
    assert_eq!(again.kind(), ErrorKind::HolderGone);
}

#[tokio::test]
async fn a_holder_past_the_reporting_cap_can_still_be_named_and_signalled() {
    // `webcam-handler-v4l2::holders::of` stops at `limits::MAX_HOLDERS_REPORTED`, because a
    // `Busy` refusal listing four hundred processes is less readable than one listing none.
    // That is right for the refusal and wrong as the *gate*: a browser holds one node from
    // several processes, `/proc`'s iteration order is not stable, and a pid the capped walk
    // did not mention really does hold the device. Gating on the walk answered
    // `HolderGone { pid }` for it — false, and with no pid left that a caller could name to
    // free the camera, which is the verb at its least usable in the one situation it exists
    // for.
    //
    // `holders::holder` is the gate instead, and its doc has always said so; this is the
    // arrangement that makes the sentence true of a call somebody makes.
    let (_scratch, node) = scratch_node();
    let fixture = daemon_over(&node);
    let (camera, wire) = ask(&fixture);

    // One more holder than the walk will ever mention, so at least one of them is invisible
    // to it whatever order `/proc` happens to be in.
    let mut children: Vec<Child> = (0..=schema::limits::MAX_HOLDERS_REPORTED)
        .map(|_| spawn_holder(&node))
        .collect();
    let walked: Vec<i32> = v4l2::holders::of(&node)
        .into_iter()
        .map(|holder| holder.pid)
        .collect();
    assert_eq!(
        walked.len(),
        schema::limits::MAX_HOLDERS_REPORTED,
        "the cap this test is about is not being reached: {walked:?}"
    );

    // The one the walk did not name — derived from the walk rather than chosen, because
    // which holders fit inside the cap is `/proc`'s business and not this test's.
    let unmentioned = children
        .iter()
        .position(|child| !walked.contains(&pid_of(child)))
        .expect("one more holder than the cap means one the walk did not mention");
    let mut named = children.remove(unmentioned);
    let pid = pid_of(&named);
    assert!(
        v4l2::holders::holds(pid, &node),
        "the child the walk did not mention is not holding the node either"
    );

    let report: TerminationReport = wire
        .terminate_holder(camera, pid)
        .await
        .expect("a pid the walk did not mention still holds the node");
    assert_eq!(report.holder.pid, pid);
    assert!(
        report.holder.comm.is_some_and(|comm| !comm.is_empty()),
        "the holder was named by a bare number"
    );
    let status = named.wait().expect("the child was spawned");
    assert_eq!(
        status.signal(),
        Some(libc_sigterm()),
        "the child ended some other way: {status:?}"
    );

    // And the others were left alone: the verb signals the pid it was given and never the
    // ones it happened to walk past.
    for child in &mut children {
        assert_eq!(
            child.try_wait().expect("the child was spawned"),
            None,
            "a holder the caller did not name was signalled"
        );
        let _ = child.kill();
        let _ = child.wait();
    }
}

#[tokio::test]
async fn a_node_somebody_else_still_holds_answers_still_held_rather_than_an_error() {
    // E4, at the far end of the doctrine: signalling is a *request*, so "signalled, and the
    // device is still taken" is an answer rather than a failure — the field's own doc says
    // a caller has to be able to tell it from "signalled, device now free" without
    // guessing, and a daemon that rendered it as an error would leave them re-running
    // `info` to find out.
    //
    // The second holder is **this test process**, which makes the arm deterministic: a
    // child that ignored `SIGTERM` would be a race against a process that might die anyway,
    // and arranging one needs a signal disposition a test in this workspace cannot set.
    let (_scratch, node) = scratch_node();
    let fixture = daemon_over(&node);
    let (camera, wire) = ask(&fixture);

    let mut child = spawn_holder(&node);
    let pid = pid_of(&child);
    let _also_held = std::fs::File::open(&node).expect("the node exists");

    let report: TerminationReport = wire
        .terminate_holder(camera, pid)
        .await
        .expect("still_held is an answer, not a refusal");
    assert_eq!(report.holder.pid, pid);
    assert!(
        report.still_held,
        "this test is holding the node and the daemon says nobody is"
    );

    // The signal was still sent, and to the process the caller named rather than to
    // whatever else had the node open: the child is dead and this process is not.
    let status = child.wait().expect("the child was spawned");
    assert_eq!(status.signal(), Some(libc_sigterm()));
    let me = i32::try_from(std::process::id()).expect("a pid fits");
    assert!(
        v4l2::holders::holds(me, &node),
        "the verb signalled a holder the caller did not name"
    );
}

#[tokio::test]
async fn four_verbs_that_want_the_device_meet_a_busy_one_without_signalling_anything() {
    // Rubric B8 and the T5 trait's own sentence: "nothing in this surface kills a process
    // to get a device free". The claim is an absence, so it needs a witness — a process
    // holding the node throughout, alive at the end.
    //
    // **This test is a sample and its name says so.** `Busy` is the state a fallback would
    // be tempted by, and it is provoked here through the four verbs most likely to want the
    // device — four of the eighteen non-`terminate_holder` methods, so a retry added to
    // `wch_photo` would not be seen from here. The claim over the *whole* surface is
    // structural rather than driven, and it has its own predicate:
    // `scripts/gates/kill-is-never-a-fallback.sh` counts the call sites of
    // `v4l2::holders::terminate` outside the backend crate and fails on a second one,
    // wherever it appears. This arm is what proves the four verbs behave; that gate is what
    // proves the fifth cannot.
    let (_scratch, node) = scratch_node();
    let fixture = daemon_over(&node);
    let (camera, wire) = ask(&fixture);

    let mut child = spawn_holder(&node);
    let pid = pid_of(&child);

    for verb in ["info", "controls", "snapshot", "discover_pairs"] {
        fixture.backend.queue_fault(fake::Fault::Busy);
        let (_, error) = match verb {
            "info" => refusal(wire.info(camera.clone()).await),
            "controls" => refusal(wire.controls(camera.clone()).await),
            "snapshot" => refusal(wire.snapshot(camera.clone()).await),
            _ => refusal(wire.discover_pairs(camera.clone()).await),
        };
        assert_eq!(error.kind(), ErrorKind::Busy, "{verb}");
        assert!(
            v4l2::holders::holds(pid, &node),
            "{verb} signalled the holder to get the device free"
        );
    }

    // Alive, and provably so rather than merely unmentioned: `try_wait` answers `None` for
    // a child that has not exited.
    assert_eq!(
        child.try_wait().expect("the child was spawned"),
        None,
        "a verb other than terminate_holder ended the holder"
    );

    // And the explicit command, on the same child, does end it — so the absence above is an
    // absence rather than a suite that could not have noticed a signal.
    let report: TerminationReport = wire
        .terminate_holder(camera, pid)
        .await
        .expect("the child holds the node");
    assert_eq!(report.holder.pid, pid);
    let status = child.wait().expect("the child was spawned");
    assert_eq!(status.signal(), Some(libc_sigterm()));
}

/// `SIGTERM`'s number, without linking `libc` here.
///
/// This crate is `#![forbid(unsafe_code)]` and the workspace confines `libc` to the V4L2
/// backend, so the constant is written out with the reason it is safe to: it is fixed by
/// the Linux ABI on every architecture this project builds for, and the test that would
/// notice a change is `webcam-handler-v4l2`'s own
/// `the_only_signal_this_build_can_send_is_the_one_the_vocabulary_has`, which compares the
/// module's constant against `libc::SIGTERM`.
const fn libc_sigterm() -> i32 {
    15
}

// ------------------------------------------------------------------ P6c's three

/// The shortest take this build will make, for the arms that need a take to be **over**.
///
/// One millisecond, and it was `Some(0)` until 2026-08-17: a budget of zero ran no turn at
/// all, so the file was a container header with nothing in it and the answer said the
/// recording succeeded. Note **N213** made that a refusal at both spellings — the flag and
/// the wire field — so the fixtures that wanted "a take that is already finished" ask for the
/// smallest one there is instead. `engine::record::drive` checks the budget before each turn,
/// so this is one frame and out, which is what these arms need: nothing waits on a camera.
const SHORTEST_TAKE_MS: Option<u64> = Some(1);

/// A recording of `duration_ms` into `path`.
///
/// `Sink::ServerPath` and never `ReturnBytes`: note **N110** narrows this verb to one of
/// D10's two sink variants, and the other one is a *refusal* this suite asserts rather than a
/// shape it can drive.
fn record_request(path: &Utf8Path, duration_ms: Option<u64>) -> RecordRequest {
    RecordRequest {
        stream: StreamRequest::default(),
        duration_ms,
        sink: Sink::ServerPath {
            path: path.to_owned(),
        },
        // The default, and the shape every request written before D12's flag existed. The
        // photo tests above are where the flag itself can go red.
        wait: false,
    }
}

#[tokio::test]
async fn a_recording_reaches_the_device_and_the_report_counts_what_the_file_holds() {
    // The claim a status document alone cannot make: that frames came **off the camera** and
    // into a container. A build whose driver never issued a turn writes a header, closes it,
    // and answers a report whose `frames_written` is zero — which is also what an honest
    // zero-duration take answers, so the only discriminating assertion is a non-zero count on
    // *both* sides: the daemon's report, and the file read back by
    // `imaging::avi::read::read_stream`, which shares no code with the muxer that wrote it.
    let fixture = Fixture::start();
    let camera = camera(&fixture.cameras, 0).id.clone();
    let scratch = TempRuntimeDir::new().expect("a throw-away directory");
    let path = scratch.base().join("take.avi");
    let (_, wire) = fixture
        .wires()
        .into_iter()
        .next()
        .expect("two transports were built");

    let started = wire
        .record_start(camera.clone(), record_request(&path, Some(400)))
        .await
        .expect("a camera with no take on it");
    assert!(started.is_running(), "{started:?}");
    let take = started.take.as_ref().expect("a started take carries one");
    assert_eq!(take.path, path);
    assert_eq!(take.format, VideoFormat::Avi, "an MJPG camera records AVI");
    assert_eq!(
        take.budget_ms, 400,
        "the request's duration reached the take"
    );
    assert_eq!(take.negotiated.pixel_format, PixelFormat::MJPG);

    // **Polled until the take has a frame, and that is what makes the assertion below about the
    // device rather than about this machine.** `record_stop` ends a take at its next turn, so a
    // stop that arrives before the driver's first `DQBUF` collects an honest report of zero
    // frames — and on a loaded host, where the fake is synthesizing a 3840x2160 JPEG for that
    // first turn, it does: this test failed that way twice in one afternoon's runs before the
    // loop below existed. Polling `record_status` is D10's own progress mechanism rather than a
    // wait invented here (`webcam-handler-client`'s poll loop is the same call), and each poll
    // is a live enumeration, so the loop is paced by the work it is asking about. The bound is
    // asserted so a take that never reaches the device is a named failure instead of a hang.
    let mut polls = 0;
    loop {
        let status = wire
            .record_status(camera.clone())
            .await
            .expect("the camera resolves");
        let reached = status
            .take
            .as_ref()
            .is_some_and(|take| take.frames_written > 0 || take.ended.is_some());
        if reached {
            break;
        }
        polls += 1;
        assert!(
            polls < 4_096,
            "the recording never reached the device: {status:?}"
        );
    }

    let report = wire
        .record_stop(camera.clone())
        .await
        .expect("the take this call started");
    assert_eq!(report.path, path);
    assert!(
        report.summary.frames_written > 0,
        "the recording never reached the device: {report:?}"
    );

    let bytes = std::fs::read(&path).expect("the file the daemon named");
    let stream = imaging::avi::read::read_stream(&bytes)
        .expect("a take the daemon closed is one the strict reader accepts");
    assert_eq!(
        u32::try_from(stream.frames.len()).expect("fewer frames than a u32"),
        report.summary.frames_written,
        "the report counted frames the file does not hold"
    );
    // And the slot is empty afterwards: `record_stop` collects, so a camera whose take has
    // been handed over is a camera that can record again.
    let after = wire
        .record_status(camera)
        .await
        .expect("the camera resolves");
    assert!(after.take.is_none(), "{after:?}");
}

#[tokio::test]
async fn a_second_start_is_told_to_retry_and_the_status_tracks_the_one_take_a_camera_has() {
    // Three states of one slot, in the order a caller meets them — and the refusal in the
    // middle, which is the one an unattended agent acts on. `Busy` means *retry* (AGENTS),
    // and retrying is exactly right here: the take that is running is bounded by its own
    // duration. The `holders` list is empty on purpose — naming the pid would name this
    // daemon and invite a client to kill the process it is talking to.
    let fixture = Fixture::start();
    let camera = camera(&fixture.cameras, 0).id.clone();
    let scratch = TempRuntimeDir::new().expect("a throw-away directory");
    let (_, wire) = fixture
        .wires()
        .into_iter()
        .next()
        .expect("two transports were built");

    // 1. Never recorded.
    let idle = wire
        .record_status(camera.clone())
        .await
        .expect("the camera resolves");
    assert!(idle.take.is_none(), "{idle:?}");
    assert!(!idle.is_running());
    assert_eq!(idle.camera, camera, "a status names the camera it is about");

    // 2. Recording. The take runs long enough that the assertions below are about a *live*
    // one rather than about whatever the scheduler left behind — the stop ends it, so the
    // duration is a ceiling and not a wait.
    let path = scratch.base().join("take.avi");
    wire.record_start(camera.clone(), record_request(&path, Some(30_000)))
        .await
        .expect("a free camera");
    let running = wire
        .record_status(camera.clone())
        .await
        .expect("the camera resolves");
    assert!(running.is_running(), "{running:?}");

    let (code, error) = refusal(
        wire.record_start(
            camera.clone(),
            record_request(&scratch.base().join("second.avi"), Some(400)),
        )
        .await,
    );
    assert_eq!(code, rpc_code(ErrorKind::Busy));
    assert_eq!(error.kind(), ErrorKind::Busy);
    match &error {
        Error::Busy {
            holders,
            this_process,
            ..
        } => {
            assert!(holders.is_empty(), "{error:?}");
            // …and it says whose camera it is, which is the half that was missing (docs/11
            // **M19**, note **N217**): the pid is withheld on purpose, and without this the
            // sentence read "held by an unidentified process" about the very daemon
            // answering.
            assert_eq!(*this_process, Some(schema::error::Occupation::Recording));
        }
        other => panic!("wrong variant: {other:?}"),
    }
    // The refusal cost the second request nothing: no file where the second take would have
    // gone, so a camera that is busy is a camera that leaves the filesystem alone.
    assert!(
        !scratch.base().join("second.avi").exists(),
        "a refused recording created its destination"
    );

    // 3. Ended and not collected. `record_stop` ends the take, and the status *after* the
    // collect is the first state again — those two are one answer because what a caller can
    // do about either is start a recording.
    let report = wire
        .record_stop(camera.clone())
        .await
        .expect("the running take");
    assert_eq!(
        report.ended,
        schema::video::RecordingEnd::Stopped,
        "a take the caller ended reports that it was stopped"
    );
    let collected = wire
        .record_status(camera.clone())
        .await
        .expect("the camera resolves");
    assert!(collected.take.is_none(), "{collected:?}");

    // And the camera records again, so the `Busy` above was a state rather than a latch.
    wire.record_start(camera.clone(), record_request(&path, SHORTEST_TAKE_MS))
        .await
        .expect("the slot came free");
    wire.record_stop(camera).await.expect("the second take");
}

#[tokio::test]
async fn a_photograph_during_a_take_is_told_who_has_the_camera_and_waiting_does_not_change_it() {
    // **The refusal an agent meets most often, and what it used to say** (docs/11 **M19**,
    // note **N217**). A `wch_photo` while this daemon is recording answered
    // `{"kind":"busy","holders":[]}`, which rendered as *"held by an unidentified
    // process"* — about the daemon the caller was talking to. The guide's remedy for it was
    // `--wait`, and `--wait` cannot reach this refusal at all: it queues on the camera's
    // command queue, and `Recordings::not_recording` answers before the actor is touched.
    //
    // So the test asks the daemon what is true (`record_status`), asks it again for the
    // refusal, and fails a message that attributes the hold to somebody else — and then
    // asks with `wait: true` to pin the guide's corrected sentence.
    let fixture = Fixture::start();
    let camera = camera(&fixture.cameras, 0).id.clone();
    let scratch = TempRuntimeDir::new().expect("a throw-away directory");
    let (_, wire) = fixture
        .wires()
        .into_iter()
        .next()
        .expect("two transports were built");

    let path = scratch.base().join("take.avi");
    wire.record_start(camera.clone(), record_request(&path, Some(30_000)))
        .await
        .expect("a free camera");
    // The premise, from the daemon rather than from this test's expectations: the camera
    // really is recording, so the refusal below is about a take rather than about anything
    // else that could make a camera busy.
    let running = wire
        .record_status(camera.clone())
        .await
        .expect("the camera resolves");
    assert!(running.is_running(), "{running:?}");

    for wait in [false, true] {
        let (code, error) = refusal(
            wire.photo(
                camera.clone(),
                PhotoRequest {
                    wait,
                    ..photo_request(Sink::ReturnBytes {
                        format: PhotoFormat::Jpeg,
                    })
                },
            )
            .await,
        );
        assert_eq!(code, rpc_code(ErrorKind::Busy), "wait: {wait}");
        let Error::Busy {
            holders,
            this_process,
            ..
        } = &error
        else {
            panic!("wait: {wait}: wrong variant: {error:?}");
        };
        // The pid stays withheld — that decision is `crate::record`'s and it stands — and
        // what replaces it is the work, which is the thing the caller waits for.
        assert!(holders.is_empty(), "wait: {wait}: {error:?}");
        assert_eq!(
            *this_process,
            Some(schema::error::Occupation::Recording),
            "wait: {wait}: the daemon knows precisely what has this camera"
        );
        let rendered = error.to_string();
        assert!(
            !rendered.contains("unidentified"),
            "wait: {wait}: the refusal sends the caller looking for a process to blame: \
             {rendered}"
        );
        // What the caller is waiting for, taken from the *vocabulary* rather than written
        // out here: this arm pinned the literal `record_status` until 2026-08-17, which is a
        // verb no surface offers — `webcam-handler-client` has no such command and the wire
        // spells the method `wch_record_status` — so the assertion held the wrong name in
        // place instead of being able to go red on it (note **N220**).
        assert!(
            rendered.ends_with(schema::error::Occupation::Recording.advice()),
            "wait: {wait}: the refusal says to retry and not what to retry on: {rendered}"
        );
    }

    // And the camera comes back, so the refusal was a state rather than a latch — which is
    // what makes `Busy`'s *retry* disposition true.
    wire.record_stop(camera.clone())
        .await
        .expect("the running take");
    wire.photo(
        camera,
        photo_request(Sink::ReturnBytes {
            format: PhotoFormat::Jpeg,
        }),
    )
    .await
    .expect("a camera whose take has ended takes photographs again");
}

#[tokio::test]
async fn a_stop_with_no_recording_is_refused_in_words_that_name_the_verb_that_makes_one() {
    // Deliberately not a bland success: an unattended caller has to be able to tell "my
    // recording is over" from "my `record_start` never took", and those are the same call's
    // two answers. `IllegalTransition` is note **N46**'s variant — the request names
    // something this build will not do — and the remedy is in the sentence because AGENTS'
    // primary consumer has no hands to work it out with.
    let fixture = Fixture::start();
    let camera = camera(&fixture.cameras, 0).id.clone();
    let (_, wire) = fixture
        .wires()
        .into_iter()
        .next()
        .expect("two transports were built");

    let (code, error) = refusal(wire.record_stop(camera.clone()).await);
    assert_eq!(code, rpc_code(ErrorKind::IllegalTransition));
    assert_eq!(error.kind(), ErrorKind::IllegalTransition);
    let rendered = error.to_string();
    assert!(rendered.contains("record_start"), "{rendered}");
    // Not the camera's fault and not the disk's: neither was asked (E3, AGENTS rule 7).
    assert_ne!(error.kind(), ErrorKind::DeviceGone);
    assert_ne!(error.kind(), ErrorKind::Busy);

    // And the camera it named really is one this daemon has: a refusal about an unknown
    // camera would be a different answer, and this assertion is what stops the arm above
    // being satisfied by a fixture that could not resolve anything.
    wire.record_status(camera)
        .await
        .expect("the camera resolves");
}

#[tokio::test]
async fn a_recording_request_only_a_socket_can_build_is_refused_before_any_camera_is_opened() {
    // `photo`'s claim at the verb that landed last, and the four shapes are the ones
    // `cli_core::Command::record_request` refuses while parsing — so a socket is the only
    // thing that can send them. The evidence that each is refused *early* is
    // `FakeBackend::opens()`: a request this build was never going to honour must not cost
    // anybody a descriptor, and on a machine with one webcam that is the difference between
    // a typo and an interrupted call.
    let fixture = Fixture::start();
    let camera = camera(&fixture.cameras, 0).id.clone();
    let scratch = TempRuntimeDir::new().expect("a throw-away directory");
    let (_, wire) = fixture
        .wires()
        .into_iter()
        .next()
        .expect("two transports were built");
    assert_eq!(fixture.backend.opens(), 0, "the fixture opened a camera");

    // 1. A recording asked for as bytes. Note **N110**: a take at its own cap is thirty-two
    //    times `limits::RPC_MAX_RESPONSE_BYTES` before base64 grows it by a third, so this
    //    variant names an answer this build cannot send — and the refusal has to carry the
    //    remedy, because the alternative is an agent retrying the same request.
    let mut asking = record_request(&scratch.base().join("take.avi"), Some(400));
    asking.sink = Sink::ReturnBytes {
        format: PhotoFormat::Jpeg,
    };
    let (code, error) = refusal(wire.record_start(camera.clone(), asking).await);
    assert_eq!(code, rpc_code(ErrorKind::IllegalTransition));
    assert!(
        error.to_string().contains("absolute server path"),
        "{error}"
    );

    // 2. A relative path. Under systemd this daemon's working directory is `/`, so resolving
    //    it would write there as the daemon's uid. The directory named does not exist,
    //    deliberately: a build that stopped refusing would then fail with `StorageIo` rather
    //    than dropping a recording into whatever cwd the test runner happened to have.
    let relative = "nowhere-a-daemon-should-write/take.avi";
    let (code, error) = refusal(
        wire.record_start(camera.clone(), record_request(relative.into(), Some(400)))
            .await,
    );
    assert_eq!(code, rpc_code(ErrorKind::IllegalTransition));
    assert!(error.to_string().contains(relative), "{error}");
    assert!(
        !camino::Utf8Path::new(relative).exists(),
        "a relative sink was resolved somewhere after all"
    );

    // 3. An extension naming a container this build does not write — the `.webp` defect one
    //    container along (debt D-1). Deliberately **not** `FormatUnsupported`: that variant is
    //    the camera saying what it cannot offer, and `.mkv` is not the camera's fault (E3).
    let mkv = scratch.base().join("take.mkv");
    let (code, error) = refusal(
        wire.record_start(camera.clone(), record_request(&mkv, Some(400)))
            .await,
    );
    assert_eq!(code, rpc_code(ErrorKind::IllegalTransition));
    assert_ne!(error.kind(), ErrorKind::FormatUnsupported);
    for &container in VideoFormat::ALL {
        assert!(error.to_string().contains(container.extension()), "{error}");
    }
    assert!(!mkv.exists(), "a refused recording created its destination");

    // 4. A duration past the cap, refused rather than clamped: an agent that asked for five
    //    minutes and silently received two cannot tell that from a camera that stopped.
    let long = scratch.base().join("long.avi");
    let (code, error) = refusal(
        wire.record_start(
            camera.clone(),
            record_request(&long, Some(schema::limits::MAX_RECORDING_MS + 1)),
        )
        .await,
    );
    assert_eq!(code, rpc_code(ErrorKind::IllegalTransition));
    assert!(
        error
            .to_string()
            .contains(&schema::limits::MAX_RECORDING_MS.to_string()),
        "{error}"
    );
    assert!(!long.exists());

    // The whole point of the four: nothing above cost a descriptor, and the camera is still
    // one this daemon can record.
    assert_eq!(
        fixture.backend.opens(),
        0,
        "a request nobody was going to honour opened a camera"
    );
    let path = scratch.base().join("served.avi");
    wire.record_start(camera.clone(), record_request(&path, SHORTEST_TAKE_MS))
        .await
        .expect("a request this build honours");
    wire.record_stop(camera).await.expect("the take");
    assert!(path.exists(), "a served recording wrote no file");
}

#[tokio::test]
async fn a_polled_status_counts_the_frames_the_finished_report_counts() {
    // The cross-check `schema::video::TakeStatus::frames_written` promises: a running take's
    // count and the finished `RecordingSummary`'s are the same number, arrived at on two
    // sides — the daemon counts what the container **accepted**, and the container counts
    // what it wrote. A build that counted frames the muxer refused, or that stopped counting
    // at all, would leave a progress mechanism that told an agent nothing was happening while
    // a file grew.
    //
    // Nothing sleeps: `Wchd::watch_finished_recordings` is what says the driver put the take
    // down, and "the take ended" is an event — a test that polled the registry for it would
    // be a test with a sleep in it under another name (AGENTS).
    let fixture = Fixture::start();
    let camera = camera(&fixture.cameras, 0).id.clone();
    let scratch = TempRuntimeDir::new().expect("a throw-away directory");
    let path = scratch.base().join("counted.avi");
    let (_, wire) = fixture
        .wires()
        .into_iter()
        .next()
        .expect("two transports were built");

    let mut finished = fixture.wchd.watch_finished_recordings();
    assert_eq!(
        *finished.borrow_and_update(),
        0,
        "the fixture recorded already"
    );
    assert_eq!(fixture.wchd.running_recordings(), 0);

    let started = wire
        .record_start(camera.clone(), record_request(&path, Some(400)))
        .await
        .expect("a free camera");
    assert_eq!(
        started
            .take
            .as_ref()
            .expect("a started take carries one")
            .frames_written,
        0,
        "a take that has just written its header holds no frames"
    );

    // The take ends on its own duration, and this is where that becomes a fact rather than a
    // guess about how long a driver takes.
    finished
        .changed()
        .await
        .expect("the daemon outlives its driver");
    assert_eq!(*finished.borrow_and_update(), 1);

    let polled = wire
        .record_status(camera.clone())
        .await
        .expect("the camera resolves");
    let take = polled.take.as_ref().expect("an uncollected take is here");
    assert!(!polled.is_running(), "{polled:?}");
    assert_eq!(take.ended, Some(schema::video::RecordingEnd::Duration));
    assert_eq!(take.budget_ms, 400);
    assert!(
        take.elapsed_ms >= take.budget_ms,
        "a take that ended on its duration ran for less than it: {take:?}"
    );

    let report = wire.record_stop(camera).await.expect("the ended take");
    assert_eq!(
        take.frames_written, report.summary.frames_written,
        "the status and the summary counted different recordings"
    );
    assert!(report.summary.frames_written > 0, "{report:?}");
    assert_eq!(fixture.wchd.running_recordings(), 0);
}

#[tokio::test]
async fn a_recording_over_an_earlier_one_leaves_no_tail_of_the_take_it_replaced() {
    // The daemon's file seam has an obligation `webcam-handler-cli`'s does not, and it is
    // invisible without this test. `engine::record::OnDisk` truncates because
    // `File::create` does; this daemon opens the destination **before** the camera and
    // without `O_TRUNC` (note **N51**: the length must not move until nothing can still
    // refuse), so the truncation has to happen at `Files::create`. Without it a short take
    // written over a long one leaves the previous recording's tail past the end of the new
    // file — bytes no reader of either container is looking for, in a file whose name says
    // it is a recording.
    let fixture = Fixture::start();
    let camera = camera(&fixture.cameras, 0).id.clone();
    let scratch = TempRuntimeDir::new().expect("a throw-away directory");
    let path = scratch.base().join("reused.avi");
    let (_, wire) = fixture
        .wires()
        .into_iter()
        .next()
        .expect("two transports were built");

    // A long file at the destination, longer than any take this test makes. Eight mebibytes
    // rather than half of one since note **N213**: the shortest take this build accepts is a
    // millisecond and takes a frame, and one synthesized MJPG frame at the replayed camera's
    // largest mode is about a mebibyte — a seed smaller than that would make the assertion
    // below fail for arithmetic rather than for a tail left behind.
    std::fs::write(path.as_std_path(), vec![0x7f; 8 * 1024 * 1024])
        .expect("a writable scratch dir");
    let before = std::fs::metadata(path.as_std_path())
        .expect("just written")
        .len();

    wire.record_start(camera.clone(), record_request(&path, SHORTEST_TAKE_MS))
        .await
        .expect("a free camera");
    let report = wire.record_stop(camera).await.expect("the take");

    let bytes = std::fs::read(&path).expect("the file the daemon named");
    assert_eq!(
        u64::try_from(bytes.len()).expect("a length this host can represent"),
        report.summary.bytes_written,
        "the file on disk is not the size the report claims"
    );
    assert!(
        u64::try_from(bytes.len()).expect("a length this host can represent") < before,
        "the recording left the file it replaced underneath it"
    );
    imaging::avi::read::read_stream(&bytes)
        .expect("a one-frame take is still a file the strict reader accepts");
}
