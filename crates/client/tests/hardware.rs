//! R3 over the socket — the daemon, the client, and whatever is plugged into this machine
//! (design §3.1, docs/7 **P4g**).
//!
//! `crates/backends/v4l2/tests/hardware.rs` drives a real camera through the backend trait,
//! in one process. This file drives one through **the shipped daemon**, over `AF_UNIX`, with
//! the shipped client at the other end — everything between a command line and a sensor, in
//! the arrangement an operator actually runs.
//!
//! Until P4g there was no such suite anywhere, and the hole had a name. Note **E12** compared
//! `wch` and `wchc` against these cameras on **five read verbs**, and its own "what it does
//! not establish" lists what this file is for: no write verb had ever been compared against
//! hardware, `photo` is one of the four `device`-bucket exemptions with "no real-hardware
//! comparison anywhere", and nothing at all had been said about "the `wchc` sweep's progress
//! rendering". Two arms, one for each half.
//!
//! ## What a socket adds that the backend rung cannot see
//!
//! Every claim here is about a boundary the in-process rung does not cross. A photo has to be
//! negotiated on the daemon's thread, written by the daemon's process to a path the *client*
//! resolved (D10), and reported in a document that agrees with the file. A sweep's progress
//! has to leave the camera actor, cross a `broadcast`, ride a per-client subscription (note
//! **N57**) and reach a watcher on the other side of a serialization — while the call it
//! belongs to is still outstanding. Neither is a property of a backend, and neither had ever
//! been asserted against a real sensor.
//!
//! ## Two arms, two observers, and the choice is not a preference
//!
//! The photo arm drives the **binary**: `-o` resolution, the exit code and the `--json`
//! document are process facts, and a test that called the executor would be asserting a
//! surface no operator uses. The sweep arm drives the **library**, for the reason
//! `crates/client/src/lib.rs` gives for there being one — the shipped watcher is an indicatif
//! bar that draws *nothing* when standard error is not a terminal, so "the events arrived" is
//! a question only answerable inside the process. That seam exists for exactly this arm.
//!
//! ## Privacy, restoration, motors, bounds
//!
//! **A frame may contain a person, and on this rung it will** (AGENTS "Hardware and
//! privacy"). Nothing here reads a pixel, prints a byte or writes anything into the tree: the
//! photos land in the fixture's temporary `$XDG_STATE_HOME`, which is deleted with the value,
//! and the failure messages below carry counts and never content — a `{:?}` of a payload is
//! how a frame reaches a log.
//!
//! **The sweep writes to the camera and puts it back** (AGENTS rule 8). The restoration is
//! the product's own — `calibrate restore`, the same call a crash recovery makes — and it is
//! *asserted* against a snapshot this suite took before the session opened, over the same
//! socket, so a restore that reported success without writing fails here.
//!
//! **No motor moves.** A brightness-class control is swept, `allow_motion` is false, and the
//! target predicate asks [`testkit::battery::is_motorized`] — the same question the
//! conformance battery asks, so "may this test move the camera somebody is pointing at a
//! person" keeps one answer in the workspace. That is why these arms carry the plain `hw_`
//! prefix rather than `hw_motion_`.
//!
//! **Every bound is a constant somebody else owns.** The sweep's client budget is
//! [`schema::limits::CLIENT_SWEEP_REQUEST_TIMEOUT_MS`], its settle policy is the default one
//! priced in the same module, and the planned sample count is checked against
//! [`schema::limits::MAX_SWEEP_SAMPLES`]. There is no duration written down in this file.
//!
//! ## Availability is not capability (AGENTS rule 7)
//!
//! A camera that cannot take part says so, by name, on a line beginning `SKIP` that
//! `scripts/smoke-hw.sh` greps for and counts — never a silent `continue`, and never a claim
//! that the camera "can't". It is not hypothetical here: the attached Chicony IR sensor
//! exposes **three** controls and none of them is a brightness-class one, so the sweep arm
//! declines it on every run of this suite and says which camera and why.
//!
//! ## Nothing here sleeps
//!
//! The daemon's readiness is a line off its stderr pipe (`support/fixture.rs`). A sweep's
//! progress is asserted from the events themselves. The one [`Instant`] below measures and
//! never decides: elapsed times are printed into the transcript an E-entry is written from,
//! and no assertion reads one.
//!
//! wch-suite: prefix=hw_ recipe=smoke-hw
//! (declared for real in `scripts/smoke-hw.sh`, which is the file the gate reads; this line
//! is the same courtesy the v4l2 rung's header pays, so a reader who found the tests before
//! the script knows which recipe runs them.)

use std::sync::Mutex;
use std::time::{Duration, Instant};

use cli_core::{Executor as _, SessionRef, SweepWatcher};
use rustix::process::Signal;
use schema::camera::CameraInfo;
use schema::capture::{PhotoDelivery, PhotoFormat, PhotoReport, SettlePolicy, StreamRequest};
use schema::control::{ControlDesc, ControlSlug, ControlType};
use schema::limits;
use schema::metrics::MetricName;
use schema::progress::{CalibrationProgress, ProgressEvent};
use schema::report::CameraList;
use schema::session::{SweepRequest, SweepSpec};
use schema::snapshot::Snapshot;

#[path = "support/fixture.rs"]
mod fixture;

use fixture::{Daemon, Fixture};

/// A `wchd` driving this machine's cameras.
///
/// The one line in this file that makes it a hardware suite, and the reason both arms are
/// here rather than in `wchc.rs`: every assertion in that file is repeatable on a machine
/// with no camera because a daemon replaying a committed document answers the same way
/// twice. These two are not, and they say what they are instead of pretending.
fn driving_the_hardware(fixture: &Fixture) -> Daemon {
    fixture.spawn(&["--backend", "v4l2"])
}

/// Every camera the daemon can see, or the reason this host has nothing to say.
///
/// The tool's **own** enumeration, over the socket, and not a walk of `/dev/video*`:
/// `/dev/videoN` is probe-order bookkeeping that a `uvcvideo` reload renumbers \[PF:22, note
/// **N63**\], so a suite that named a node would be asserting the order the kernel happened
/// to probe in. It is also the honest shape for a daemon suite — what a client can reach is
/// what the daemon lists.
fn attached(cameras: &CameraList) -> bool {
    if cameras.cameras.is_empty() {
        println!(
            "SKIP: the daemon enumerated no camera on this host, so there is nothing for the \
             R3 socket arms to drive"
        );
        return false;
    }
    true
}

/// Stop the daemon the way a service manager does, and answer what the kernel said.
///
/// The last claim a hardware run can make about the process that held a camera for it: that
/// it exited **on purpose and by request**, rather than having died somewhere in the middle
/// with its work still apparently green. `SIGTERM` and not `Child::kill`'s `SIGKILL` for
/// `crates/daemon/tests/support/supervised.rs`'s reason — the ordered teardown is the thing,
/// and `SIGKILL` runs none of it. The `Drop` in `support/fixture.rs` still runs afterwards
/// and is a no-op on a reaped child, which is what keeps a *failing* assertion from leaving a
/// daemon behind.
fn stopped(daemon: &mut Daemon) -> std::process::ExitStatus {
    let raw = i32::try_from(daemon.child.id()).expect("a pid fits in an i32");
    let pid = rustix::process::Pid::from_raw(raw).expect("a live child has a valid pid");
    rustix::process::kill_process(pid, Signal::TERM).expect("ours to signal");
    daemon
        .child
        .wait()
        .expect("the daemon is this process's child to reap")
}

// ------------------------------------------------------------------- a photo over UDS

#[test]
#[ignore = "R3: needs a camera attached; run with `just smoke-hw`"]
fn hw_a_photo_over_the_socket_decodes_at_the_negotiated_size_and_an_mjpg_one_is_the_cameras_own_bytes()
 {
    // `crates/backends/v4l2/tests/hardware.rs`'s photo arm, through the socket instead of the
    // backend — deliberately the same claims, because the point is that the daemon and the
    // client did not lose any of them on the way. What is asserted is the photo's agreement
    // with its *own* report, never its content: lighting varies, and a test that reads pixels
    // fails on somebody else's desk.
    //
    // Three things are true here that cannot be true in that file. The path is resolved by
    // the **client** and written by the **daemon** (D10), so a build where the two disagreed
    // about relative paths produces a file nobody can find. The report crossed a
    // serialization, so a field the wire drops reads as a mismatch here. And the base64 half
    // is D10's encoding end to end on a real frame — a quarter of a megabyte of one — where
    // the fake's is a committed fixture a tenth the size.
    let fixture = Fixture::new();
    let mut daemon = driving_the_hardware(&fixture);

    let listed: CameraList = serde_json::from_slice(fixture.run(&["--json", "list"]).ok())
        .expect("`list --json` answers a CameraList");
    if !attached(&listed) {
        return;
    }

    let mut taken = 0usize;
    for info in &listed.cameras {
        if info.capture_node().is_none() {
            println!(
                "SKIP (partial): {} has no capture node, so there is nothing to photograph",
                info.id
            );
            continue;
        }

        // ---------------------------------------------------------- the `ServerPath` half
        //
        // Under the fixture's temporary `$XDG_STATE_HOME`, which is a scratch directory
        // deleted with the value: a frame may contain a person and one must never be able to
        // reach the tree (AGENTS; `scripts/gates/no-frame-bytes-in-repo.sh` sniffs every file
        // in it for exactly this). Named after the bus path rather than the node, because
        // `/dev/videoN` renumbers [PF:22].
        let path = fixture
            .state
            .root()
            .join(format!("{}.jpg", info.fingerprint.bus_path));
        let ran = fixture.run(&["--json", "photo", info.id.as_str(), "-o", path.as_str()]);
        let document = ran.json();
        let report: PhotoReport =
            serde_json::from_value(document.clone()).unwrap_or_else(|error| {
                panic!("{}: the answer is not a PhotoReport: {error}", info.id)
            });

        match &report.delivery {
            PhotoDelivery::Path {
                path: written,
                byte_count,
            } => {
                // The client resolved this path and the daemon wrote it. A build that
                // resolved it on the far side would answer a path relative to the *daemon's*
                // working directory, which is a file an operator cannot find.
                assert_eq!(
                    written, &path,
                    "{}: the daemon wrote somewhere else",
                    info.id
                );
                let on_disk = std::fs::metadata(written.as_std_path())
                    .unwrap_or_else(|error| panic!("{}: {written} is not there: {error}", info.id));
                assert_eq!(
                    on_disk.len(),
                    *byte_count,
                    "{}: the report's byte count is not the file's",
                    info.id
                );
            }
            other => panic!("{}: `-o` answered a {other:?} delivery", info.id),
        }
        // …and no payload rode along with it: a `Path` delivery carrying bytes is a document
        // that disagrees with itself, and on this rung it would be a camera frame base64'd
        // into a terminal nobody asked to receive one in. Asked of the *document* rather than
        // of the parsed report, because a typed parse cannot see a field its type does not
        // declare.
        assert!(
            document.get("bytes").is_none(),
            "{}: a path delivery carried a payload",
            info.id
        );

        let bytes = std::fs::read(path.as_std_path()).expect("the daemon wrote the file");
        // Decodable at the size the report claims — the assertion that catches a frame handed
        // on with the wrong dimensions, which is how a decoder reads past a buffer. `image`
        // rather than this workspace's own decoder, deliberately: a photo checked by the
        // codec that produced it is a document agreeing with itself.
        let decoded = image::load_from_memory(&bytes)
            .unwrap_or_else(|error| panic!("{}: the photo does not decode: {error}", info.id));
        assert_eq!(
            (decoded.width(), decoded.height()),
            (report.width, report.height),
            "{}: the photo's size disagrees with its own report",
            info.id
        );
        // The negotiated stream is *surfaced*, which is D5's rule that what the device agreed
        // to travels with the answer, and the two sizes in this one document agree. They have
        // to here and would not always: a pixel-domain transform swaps the rendered pair and
        // leaves the negotiated one alone, so this is a claim about an untransformed photo
        // (`Transform::None`, which is what no `--transform` means) rather than an identity.
        assert_eq!(
            (report.negotiated.width, report.negotiated.height),
            (report.width, report.height),
            "{}: the negotiated size and the rendered size disagree",
            info.id
        );
        if report.negotiated.pixel_format.is_compressed() {
            assert!(
                report.rendering.is_verbatim(),
                "{}: an MJPG source must pass through, not re-encode [E6]: {:?}",
                info.id,
                report.rendering
            );
        }

        // ---------------------------------------------------------- the `ReturnBytes` half
        //
        // The frame comes back base64'd in the JSON-RPC result (D10), `Remote::photo` checks
        // the payload against the report (`PhotoResponse::bytes_match_the_delivery`) and the
        // shared renderer puts it on standard output. Nothing is written; the bytes live in
        // this process and are counted, never printed.
        let returned = fixture.run(&["photo", info.id.as_str()]);
        let image = returned.ok();
        assert!(!image.is_empty(), "{}: {}", info.id, returned.stderr);
        // A JPEG's own two markers, at both ends: a base64 decode that dropped or shifted a
        // byte would still have produced a plausible length. The failure message says which
        // marker was missing and how many bytes arrived — never a byte of the payload, which
        // is the rule that makes this assertion safe to fail (AGENTS: frames never enter
        // logs).
        assert!(
            image.starts_with(&[0xff, 0xd8]),
            "{}: the {} byte(s) on standard output do not open with a JPEG marker",
            info.id,
            image.len()
        );
        assert!(
            image.ends_with(&[0xff, 0xd9]),
            "{}: the {} byte(s) on standard output have no JPEG end marker, so the payload \
             was truncated on the way through",
            info.id,
            image.len()
        );
        // The summary goes to standard error when the bytes are on standard output, so a
        // pipe holds an image and nothing else.
        assert!(
            returned.stderr.contains("delivery"),
            "{}: {}",
            info.id,
            returned.stderr
        );

        taken += 1;
        println!(
            "{}: {} {}x{} → {} bytes to a file, {} bytes through base64, settled {} frame(s), \
             {}",
            info.id,
            report.negotiated.pixel_format,
            report.width,
            report.height,
            report.delivery.byte_count(),
            image.len(),
            report.frames_settled,
            if report.rendering.is_verbatim() {
                "the camera's own bytes [E6]"
            } else {
                "re-encoded"
            }
        );
    }

    if taken == 0 {
        println!("SKIP: no attached camera could take a photo over the socket");
    }
    let status = stopped(&mut daemon);
    assert!(
        status.success(),
        "the daemon that served {taken} photo(s) did not stop cleanly: {status}"
    );
    println!("wchd stopped on SIGTERM with {status} after {taken} photo(s)");
}

// -------------------------------------------------- a calibrate sweep with live progress

/// Every event a sweep delivered, in order, with how long after the call it arrived.
///
/// The observable a subprocess cannot give (see `crates/client/src/lib.rs`'s header): the
/// shipped watcher draws nothing when standard error is not a terminal, so a suite that could
/// only see `wchc`'s output could assert that a sweep *answered* and never that its progress
/// arrived — which is the one property `Remote::calibrate_sweep`'s ordering exists to provide.
///
/// The [`Duration`] is measurement and not synchronization: nothing below branches on one.
/// They are printed because a sweep whose events all landed in a burst at the end and a sweep
/// that reported as it went are indistinguishable from the counts alone, and the transcript
/// this rung produces is read by a person.
#[derive(Debug)]
struct Recording {
    from: Instant,
    seen: Mutex<Vec<(Duration, ProgressEvent)>>,
}

impl Recording {
    fn new() -> Recording {
        Recording {
            from: Instant::now(),
            seen: Mutex::new(Vec::new()),
        }
    }

    fn events(&self) -> Vec<(Duration, ProgressEvent)> {
        self.seen
            .lock()
            .expect("the watcher was not poisoned")
            .clone()
    }
}

impl SweepWatcher for Recording {
    fn event(&self, event: &ProgressEvent) {
        if let Ok(mut seen) = self.seen.lock() {
            seen.push((self.from.elapsed(), event.clone()));
        }
    }
    fn finish(&self) {}
}

/// A control this arm may run a whole calibration session over.
///
/// The same three names and the same predicates as the backend rung's `brightness_class_target`
/// — "a brightness-class control" is what docs/7 asks for and the three UVC controls that move
/// luma directly are the population. It is written twice because a `tests/` binary's items are
/// private to it and the workspace has nowhere shared to put a *selection* that is only ever
/// made by a test; what is **not** written twice is the part that matters, since
/// [`testkit::battery::is_perturbable`] and [`testkit::battery::is_motorized`] are the battery's
/// own, so no hardware arm anywhere can touch a control another one may not.
fn brightness_class_target(controls: &[ControlDesc]) -> Option<&ControlDesc> {
    const BRIGHTNESS_CLASS: [&str; 3] = ["brightness", "gamma", "gain"];
    BRIGHTNESS_CLASS.iter().find_map(|name| {
        controls.iter().find(|desc| {
            desc.slug.as_str() == *name
                && testkit::battery::is_perturbable(desc)
                && !testkit::battery::is_motorized(&desc.slug)
                && !desc.is_inactive()
                && desc.control_type == ControlType::Integer
                && desc.range.max > desc.range.min
        })
    })
}

/// What one control was holding when a snapshot was taken.
fn recorded<'a>(snapshot: &'a Snapshot, control: &ControlSlug) -> Option<&'a schema::ControlValue> {
    snapshot
        .entries
        .iter()
        .find(|entry| &entry.control == control)
        .map(|entry| &entry.value)
}

#[test]
#[ignore = "R3: needs a camera attached; run with `just smoke-hw`"]
fn hw_a_sweep_over_the_socket_delivers_its_progress_live_and_leaves_the_camera_where_it_found_it() {
    // D8's headline arc over a real sensor and a real socket at the same time: snapshot,
    // start, plan, sweep, restore — with the sweep's progress crossing a subscription while
    // the call it belongs to is still outstanding.
    //
    // Two claims are new here and neither belongs to the backend rung.
    //
    // **The events arrive, terminal one included.** `wch_calibrate_sweep` answers only the
    // final session, so every event below came off the *separate* `wch_subscribe_calibration`
    // stream (note **N57**) on the same connection, and the last of them is the one note
    // **N69** is about: the answer and the `SweepFinished` leave the daemon on two different
    // tasks and reach one writer in whichever order that runtime scheduled them, so a client
    // that stopped reading when its call returned lost a bar's closing line about one run in
    // a hundred. That defect was found and fixed against a scripted double; this is the arm
    // that makes it a claim about a real daemon driving a real camera.
    //
    // **The camera goes back.** The sweep writes to it — that is what a sweep is — and AGENTS
    // rule 8 asks for the restoration to be asserted, not assumed. The witness is a snapshot
    // taken over the same socket before the session opened, so a `calibrate restore` that
    // reported success without writing fails here rather than in an operator's next session.
    let fixture = Fixture::new();
    let mut daemon = driving_the_hardware(&fixture);
    let mut remote = client::remote::Remote::connect(
        &fixture.socket(),
        // The budget the shipped binary picks for this verb, from the module that prices it
        // against the sweep's own caps. A number written here would be this suite deciding
        // how long a camera is allowed to take.
        Duration::from_millis(limits::CLIENT_SWEEP_REQUEST_TIMEOUT_MS),
    )
    .expect("the daemon is listening");

    let listed = remote.list().expect("the daemon enumerates");
    if !attached(&listed) {
        return;
    }

    let mut swept = 0usize;
    for info in &listed.cameras {
        let Some(control) = sweep_one_camera(&mut remote, info) else {
            continue;
        };
        swept += 1;
        println!("{}: {control} swept and restored over the socket", info.id);
    }

    if swept == 0 {
        println!(
            "SKIP: no attached camera offered a capture node and a sweepable brightness-class \
             control, so no sweep ran over the socket"
        );
    }
    let status = stopped(&mut daemon);
    assert!(
        status.success(),
        "the daemon that ran {swept} sweep(s) did not stop cleanly: {status}"
    );
    println!("wchd stopped on SIGTERM with {status} after {swept} sweep(s)");
}

/// One camera's whole session, or `None` with a named reason on standard output.
///
/// A function rather than a loop body because the two "this camera cannot take part" exits
/// are the point (AGENTS rule 7): each is a `SKIP` line naming the camera and what it lacks,
/// which `scripts/smoke-hw.sh` greps and counts, and neither is reachable without saying so.
/// On the hardware this was written against, the second one fires every run — the Chicony IR
/// sensor exposes three controls and no brightness-class one.
fn sweep_one_camera(remote: &mut client::remote::Remote, info: &CameraInfo) -> Option<ControlSlug> {
    if info.capture_node().is_none() {
        println!(
            "SKIP (partial): {} has no capture node, so a sweep has nothing to photograph",
            info.id
        );
        return None;
    }
    // The read verb, not `--discover-pairs`: that one is a probe that writes, and a sweep is
    // already the write this arm is about.
    let report = remote
        .controls(&info.id, false)
        .unwrap_or_else(|error| panic!("{}: controls failed over the socket: {error}", info.id));
    let Some(desc) = brightness_class_target(&report.controls).cloned() else {
        println!(
            "SKIP (partial): {} exposes no sweepable brightness-class control among its {}, so \
             this arm declines it — which is a fact about this sensor's control set and not \
             about the socket",
            info.id,
            report.controls.len()
        );
        return None;
    };
    let control = desc.slug.clone();

    // The witness, over the same socket, before anything is written.
    let witness = remote
        .snapshot(&info.id)
        .unwrap_or_else(|error| panic!("{}: snapshot failed: {error}", info.id));
    let held = recorded(&witness, &control).cloned().unwrap_or_else(|| {
        panic!(
            "{}: {control} is writable and not in its own snapshot",
            info.id
        )
    });

    let session = remote
        .calibrate_start(
            &info.id,
            "R3 socket calibration",
            "a sample an operator would call correctly exposed",
            &["the operator's own eye on the sample photos".to_owned()],
        )
        .unwrap_or_else(|error| panic!("{}: a session would not open: {error}", info.id));
    // By id rather than by task, which is what makes `remote::SweepFilter` the exact filter
    // rather than the control-shaped approximation it falls back to.
    let which = SessionRef::Id { id: session.id };
    remote
        .calibrate_plan(&info.id, &which, std::slice::from_ref(&control), false)
        .unwrap_or_else(|error| panic!("{control}: the plan was refused: {error}"));

    // Five values across the control's *declared* range, aligned to its own step by the
    // planner. Derived from the device rather than written down: `brightness` is 0..=255 on
    // one attached camera and 0..=100 on another.
    let span = desc.range.max.saturating_sub(desc.range.min);
    let stride = (span / 4).max(desc.range.effective_step());
    let watcher = Recording::new();
    let request = SweepRequest {
        control: control.clone(),
        plan: SweepSpec::Uniform { step: stride },
        // No motor moves on this rung's plain `hw_` prefix, and the target predicate has
        // already refused a motorized control. Both, because a prefix is a convention and
        // this flag is the daemon's own refusal (design §5).
        allow_motion: false,
        stream: StreamRequest::default(),
        settle: SettlePolicy::default(),
        photo_format: PhotoFormat::Jpeg,
    };
    let began = Instant::now();
    let finished = remote
        .calibrate_sweep(&info.id, &which, &request, &watcher)
        .unwrap_or_else(|error| panic!("{control}: the sweep failed on {}: {error}", info.id));
    let took = began.elapsed();
    assert_eq!(
        finished.id, session.id,
        "{control}: another session answered"
    );

    check_progress(info, session.id, &control, &watcher, took);

    // ---------------------------------------------------------------------- and back
    //
    // The product's own restoration — the same call an ordinary session end and a crash
    // recovery both make — reading the snapshot the daemon persisted before the sweep's first
    // write.
    let restore = remote
        .calibrate_restore(&info.id, &which)
        .unwrap_or_else(|error| panic!("{control}: the restore failed: {error}"));
    assert!(
        restore.is_complete(),
        "{}: the restore reported itself incomplete: {:?}",
        info.id,
        restore.unrestored()
    );
    // …and the report is checked against the device rather than believed. A second snapshot
    // over the same socket, compared with the first: every control that was not VOLATILE at
    // witness time is back where the session found it. A volatile value cannot be
    // meaningfully restored and the snapshot says which those are, which is why they are
    // skipped here rather than silently tolerated.
    let after = remote
        .snapshot(&info.id)
        .unwrap_or_else(|error| panic!("{}: the closing snapshot failed: {error}", info.id));
    let mut compared = 0usize;
    for entry in &witness.entries {
        if entry.was_volatile {
            continue;
        }
        compared += 1;
        assert_eq!(
            recorded(&after, &entry.control),
            Some(&entry.value),
            "{}: {} is {:?} and the session found it at {:?}",
            info.id,
            entry.control,
            recorded(&after, &entry.control),
            entry.value
        );
    }
    assert_eq!(
        recorded(&after, &control),
        Some(&held),
        "{}: the swept control itself did not come back",
        info.id
    );
    println!(
        "{}: restored — {} outcome(s), {compared} non-volatile control(s) compared against the \
         opening snapshot, {control} back at {held:?}",
        info.id,
        restore.outcomes.len()
    );
    Some(control)
}

/// What the watcher saw, asserted and then written into the transcript.
fn check_progress(
    info: &CameraInfo,
    session: uuid::Uuid,
    control: &ControlSlug,
    watcher: &Recording,
    took: Duration,
) {
    let seen = watcher.events();
    assert!(
        !seen.is_empty(),
        "{}: no progress reached the watcher, so the subscription and the call did not overlap",
        info.id
    );
    // Every event is this session's and this control's — `remote::SweepFilter`'s promise, and
    // the only thing standing between a bar and a second sweep's numbers on a shared daemon.
    // Both, because the filter uses one *or* the other depending on how the session was named
    // and a build that swapped them would still satisfy the weaker check.
    for (_, event) in &seen {
        assert_eq!(event.session, session, "{event:?}");
        assert_eq!(*event.progress.control(), *control, "{event:?}");
    }

    // The first event is the sweep's start, and it carries the size of the plan: the one
    // event a mid-sweep subscriber has no earlier events to reconstruct, and therefore the
    // one a bar's whole first frame depends on.
    let Some((_, CalibrationProgress::SweepStarted { total, .. })) =
        seen.first().map(|(at, event)| (at, &event.progress))
    else {
        panic!("{}: the sweep's first event is missing: {seen:?}", info.id);
    };
    let total = *total;
    assert!(
        total >= 3,
        "{control}: {total} sample(s) is too few to say anything about a sweep"
    );
    // Bounded by the cap the schema owns, checked against a plan a *device's* range produced
    // — which is the direction that can fail: the range is the camera's and the planner is
    // ours.
    assert!(
        total <= limits::MAX_SWEEP_SAMPLES,
        "{control}: a plan of {total} exceeds MAX_SWEEP_SAMPLES"
    );

    // **The last is the sweep's end, and this is the assertion note N69 exists under.** The
    // daemon's answer and its `SweepFinished` race on two tasks; before `Remote`'s bounded
    // tail existed, the answer winning cost this event, measured at 34 µs late and failing 2
    // runs in 150 under load. Against real hardware every sample is a settle and a capture,
    // which widens every other window in the sweep and leaves this one exactly as narrow —
    // so this is the same race, run on the machine the product runs on.
    let finished = matches!(
        seen.last().map(|(_, event)| &event.progress),
        Some(CalibrationProgress::SweepFinished { .. })
    );
    assert!(
        finished,
        "{}: the sweep's terminal event never reached the watcher [N69]: {seen:?}",
        info.id
    );

    // One `ValueSet` and one `SampleTaken` per planned sample: the two events a bar advances
    // on, counted rather than sampled, so a build that emitted progress only at the end would
    // still fail even though its first and last events were right.
    let counted = |wanted: fn(&CalibrationProgress) -> bool| -> u32 {
        u32::try_from(
            seen.iter()
                .filter(|(_, event)| wanted(&event.progress))
                .count(),
        )
        .expect("a bounded sweep produces a small count")
    };
    assert_eq!(
        counted(|progress| matches!(progress, CalibrationProgress::ValueSet { .. })),
        total,
        "{}: {control}: a value was set without saying so",
        info.id
    );
    assert_eq!(
        counted(|progress| matches!(progress, CalibrationProgress::SampleTaken { .. })),
        total,
        "{}: {control}: a sample was taken without saying so",
        info.id
    );

    // The metrics that crossed the wire are numbers in their own domain. This is the client's
    // half of what the backend rung asserts of the samples on disk, and it is a *wire* claim
    // as much as an imaging one: a NaN has no JSON spelling, so a metric that arrived at all
    // arrived as a number, and the three that are fractions have a range to be outside of.
    for (_, event) in &seen {
        let CalibrationProgress::SampleTaken { metrics, .. } = &event.progress else {
            continue;
        };
        for (metric, score) in metrics {
            assert!(
                score.is_finite(),
                "{}: {control}: {metric} came off the wire as {score}",
                info.id
            );
            if matches!(
                metric,
                MetricName::ClippedHighlights | MetricName::ClippedShadows | MetricName::MeanLuma
            ) {
                assert!(
                    (0.0..=1.0).contains(score),
                    "{}: {control}: {metric} is a fraction and arrived as {score}",
                    info.id
                );
            }
        }
    }

    // ------------------------------------------------------------------ the transcript
    //
    // Printed and not asserted, deliberately. Whether the frame got brighter is the *backend*
    // rung's claim about this control and it has one home; what this arm has to show a reader
    // is that the events arrived while the work was happening, which is an arrival profile
    // rather than a predicate. A sweep whose numbers all sat at the same millisecond would
    // pass every assertion above and be obvious in the two lines below.
    println!(
        "{}: swept {control} — {total} sample(s), {} event(s) in {:.2}s, first at {:.2}s, last \
         at {:.2}s",
        info.id,
        seen.len(),
        took.as_secs_f64(),
        seen.first().map(|(at, _)| at.as_secs_f64()).unwrap_or(0.0),
        seen.last().map(|(at, _)| at.as_secs_f64()).unwrap_or(0.0),
    );
    for (at, event) in &seen {
        if let CalibrationProgress::SampleTaken {
            index,
            requested,
            applied,
            metrics,
            ..
        } = &event.progress
        {
            println!(
                "{}:   sample {index}/{total} at {:.2}s — {control}={requested} (applied \
                 {applied}) {}",
                info.id,
                at.as_secs_f64(),
                metrics
                    .iter()
                    .map(|(name, score)| format!("{name}:{score:.4}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            );
        }
    }
}
