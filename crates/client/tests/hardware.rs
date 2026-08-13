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
//! socket, so a restore that reported success without writing fails here. The population it
//! is asserted over is the restore report's own claim
//! ([`testkit::battery::restoration_claim`]): a control the device's automation owns at both
//! ends is excluded, named and counted, because its read-back is an algorithm's answer and
//! not a setting \[PF:24\] — and a restore whose exclusions left nothing to compare fails
//! rather than passing quietly.
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
//! The one number that *is* this suite's own is [`MIN_SAMPLES`], and it is a number about
//! what these assertions can see rather than about what the product allows — its own doc
//! comment argues that, and a `const` assertion holds it under the schema's ceiling.
//!
//! ## Availability is not capability (AGENTS rule 7)
//!
//! A camera that cannot take part says so, by name, on a line beginning `SKIP` that
//! `scripts/smoke-hw.sh` greps for and counts — never a silent `continue`, and never a claim
//! that the camera "can't". It is not hypothetical here: the attached Chicony IR sensor
//! exposes **three** controls and none of them is a brightness-class one, so the sweep arm
//! declines it on every run of this suite and says which camera and why.
//!
//! **Three declines and not one**, which note **N72** is about. "This sensor has no
//! brightness-class control" is a fact about a control *set*; "it has one and an automation
//! partner owns it" is a fact about a *state* that D3 exists to clear; "its declared range
//! plans two samples" is a fact about a *range*. The first two are
//! [`testkit::battery::brightness_class_target`]'s to tell apart and it names the term that
//! refused; the third is [`sweep_for`]'s, and it is taken **before the session opens** — the
//! shipped version asserted it after the sweep and above the restore, which turned a device
//! shape into a red run and left the camera where the sweep had put it.
//!
//! ## The parts that need no camera
//!
//! Both arms above are `#[ignore]`d and need a camera. The plain tests at the foot of this
//! file do not: what they exercise is [`sweep_for`], a fold over a `ControlDesc`, and they
//! run on every `just ci` on a machine with nothing plugged in — because the shapes they are
//! about (a `brightness` whose step is its whole range) are not on this desk, and a guard
//! against a device nobody here owns is still a guard that has to be able to go red.
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
use testkit::battery;

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

/// How few samples make this arm's assertions worth making, and this arm's argument for the
/// number.
///
/// Three, and the number is this suite's rather than the product's, which is why it is here
/// and not in [`schema::limits`]: nothing about the *daemon* changes at two samples, and a
/// two-sample sweep is a perfectly good sweep for an operator. What needs three is the
/// *claim* — `check_progress` reads an arrival profile out of the timings and counts a
/// `ValueSet`/`SampleTaken` pair per sample, and two of anything cannot distinguish "the
/// events arrived as the work happened" from "they all landed at the end", which is the one
/// property this arm exists to observe (note **N69**, and E13's 0.52 / 1.02 / 1.61 / 2.18 /
/// 2.84 s column).
///
/// The `because` clause carries that argument into the `SKIP` line, because the sibling rung
/// declines at the same count for an entirely different reason and a transcript that said
/// only "fewer than the 3 this arm needs" would make the two look like one rule.
const MIN_SAMPLES: battery::SampleFloor = battery::SampleFloor {
    count: 3,
    because: "say anything about an arrival profile",
};

/// The floor has to sit under the schema's ceiling, or this arm would decline every plan the
/// product allows and read as a suite that ran.
///
/// A `const` assertion for note **N70**'s reason: a relation nothing can evaluate is a
/// relation nobody has checked, and this one costs a compile rather than a run.
const _: () = assert!(MIN_SAMPLES.count <= limits::MAX_SWEEP_SAMPLES);

/// The sweep this arm would ask the daemon for, priced from the descriptor before anything is
/// written — [`testkit::battery::sweep_for`], which is where the whole of it lives.
///
/// It was private to this file when note **N72** wrote it, and it is not any more: the
/// in-process calibration arm in `crates/backends/v4l2/tests/hardware.rs` carried the same
/// finding and needed the same arithmetic against the same planner, and a second copy of a
/// rule is what F5 of that same note was about. What stays here is the one thing that is
/// genuinely this arm's — [`MIN_SAMPLES`], the floor and the argument for it.
fn sweep_for(desc: &ControlDesc) -> battery::SweepChoice {
    battery::sweep_for(desc, MIN_SAMPLES)
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
    // Two questions, and the second one is note **N72**'s finding: "this sensor does not
    // have a brightness-class control" and "it has one and something about it stops this arm
    // today" are facts about different things, and AGENTS rule 7 forbids a test converting
    // one into the other. The predicate is [`testkit::battery::brightness_class_target`] —
    // the battery's, beside `is_perturbable` and `is_motorized`, where a unit test over
    // `ControlDesc` values can reach it and where the v4l2 rung asks the same question of
    // the same code.
    let desc = match battery::brightness_class_target(&report.controls) {
        battery::SweepTarget::Found(desc) => desc.clone(),
        battery::SweepTarget::Declined(why) => {
            println!(
                "SKIP (partial): {} {why}, so this arm declines it — which is a fact about {} \
                 and not about the socket",
                info.id,
                why.is_a_fact_about()
            );
            return None;
        }
    };
    let control = desc.slug.clone();

    // ------------------------------------------------- priced before anything is written
    //
    // The last decline that can be taken for free, and the reason it is taken *here*: below
    // this line the arm opens a session, and `wch_calibrate_start` runs D3's empirical pair
    // probe, which writes to the camera and puts it back. Everything from that call onward
    // is a path where a failure leaves work half-done on a real device (E13 records the
    // shape and the three times it was met by hand), so a question answerable from a
    // descriptor gets answered before the descriptor is all this arm has touched.
    let (spec, samples) = match sweep_for(&desc) {
        battery::SweepChoice::Planned { spec, samples } => (spec, samples),
        battery::SweepChoice::Declined(why) => {
            println!(
                "SKIP (partial): {} {why} — which is a fact about this control's declared \
                 range on this sensor and not about the socket",
                info.id
            );
            return None;
        }
    };

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

    let watcher = Recording::new();
    let request = SweepRequest {
        control: control.clone(),
        // The spec `sweep_for` priced, and the same value — not a second derivation of it,
        // which would be a way for the number this arm checked and the number it asked for
        // to stop being the same number.
        plan: spec,
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

    check_progress(info, session.id, &control, samples, &watcher, took);

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
    // over the same socket, compared with the first: every control **the report says it put
    // back** is where the session found it, and which those are is
    // [`testkit::battery::restoration_claim`]'s answer rather than this file's.
    //
    // The filter was the witness snapshot's own `was_volatile` flag until PF:24 measured
    // what that flag does not say. A sweep photographs the scene at every sample and a
    // camera's auto-white-balance reacts to precisely that, so on the Logitech BRIO
    // `white_balance_temperature` — INACTIVE, **not** VOLATILE — moved between a
    // `calibrate restore` that reported itself complete and the closing snapshot two
    // round-trips later. Nothing about the socket, the daemon or the engine was wrong in
    // those runs; the arm was demanding a number from an algorithm.
    //
    // Asking the report is what makes the exclusion the *device's* statement. It is
    // `OwnedByAutomation` only for a control that was INACTIVE when the snapshot was taken
    // **and** is INACTIVE now — a control this session's own sweep switched an automation
    // off for and then handed back stays in the population, which is the half a blanket
    // "skip INACTIVE controls" rule would have thrown away along with the defect. Excluding
    // by *name* would have been the version AGENTS rule 7 forbids outright.
    //
    // Two things are still asserted and neither is negotiable: the swept control itself
    // below, and `restore.is_complete()` above, which is where a control nobody could put
    // back — VOLATILE ones included, since those reach the report as `Unrestorable` — costs
    // the run. `account_for` adds the third: a restore whose exclusions ate the whole
    // population is a red arm, not a green one that compared nothing.
    let after = remote
        .snapshot(&info.id)
        .unwrap_or_else(|error| panic!("{}: the closing snapshot failed: {error}", info.id));
    let claim = battery::restoration_claim(&restore);
    let mut compared = 0usize;
    for entry in &witness.entries {
        if !claim.speaks_for(entry.control.as_str()) {
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
    // The swept control is asserted whatever the claim says about anything else, and it can
    // never be excluded by it: `brightness_class_target` refuses an INACTIVE control before
    // the session opens, so a sweep target that came back as `OwnedByAutomation` would mean
    // the sweep left an automation holding the control it borrowed — which is a defect this
    // line is entitled to report as one.
    assert_eq!(
        recorded(&after, &control),
        Some(&held),
        "{}: the swept control itself did not come back",
        info.id
    );
    println!(
        "{}: restored — {} outcome(s), {compared} claimed control(s) compared against the \
         opening snapshot, {control} back at {held:?}",
        info.id,
        restore.outcomes.len()
    );
    claim.account_for(info.id.as_str(), compared);
    Some(control)
}

/// What the watcher saw, asserted and then written into the transcript.
///
/// `expected` is what [`sweep_for`] priced off the descriptor before the session opened, and
/// it enters as an argument rather than being re-derived here so that the number this
/// function checks is the number the request carried.
fn check_progress(
    info: &CameraInfo,
    session: uuid::Uuid,
    control: &ControlSlug,
    expected: u32,
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
    // **The daemon planned what this client priced.** What stood here was `total >= 3`, which
    // this arm can no longer reach — `sweep_for` declines a shorter plan before the session
    // opens (note **N72**) — and an assertion whose false branch is unreachable is
    // decoration, which AGENTS' "no assertion inside a conditional whose false branch cannot
    // go red" is the same rule about.
    //
    // What replaces it is a claim only this rung can make. The client planned from the
    // descriptor it read over the socket; the daemon planned from the descriptor its own
    // camera actor read, from the same pure core, and answered a count that crossed a
    // serialization to get here. They agree, and a build where `SweepSpec::Uniform`'s step
    // were dropped on the wire — the plan silently becoming `All`, every step of a
    // 0..=255 range — is red here rather than a sweep that takes a hundred and fifty extra
    // photos and passes.
    assert_eq!(
        total, expected,
        "{}: {control}: this client planned {expected} sample(s) from the device's declared \
         range and the daemon's plan is {total}",
        info.id
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

// ------------------------------------------------------ what this arm decides before it runs
//
// **These tests need no camera and they are the point** (note **N72**). Both arms above are
// ignored and run on a desk with hardware on it; what [`sweep_for`] decides is a fold over a
// `ControlDesc`, and a fold over values is testable at every `just ci` on a machine with
// nothing plugged in.
//
// That distinction is the finding. The shape these arms guard against — a `brightness`
// declaring `0..=64` with a step of 64, or `0..=1` — is not on this desk, and "I could not
// reproduce it on my hardware" is not a reason to leave the guard unexercised; it is a reason
// the guard cannot be exercised any other way. A hardware run would have proved these arms
// green on three cameras that all declare a wide `brightness`, which is the population note
// **N70** describes: a test that asserts the assumption that produced the code.
//
// They carry no `hw_` prefix, so `scripts/smoke-hw.sh`'s `test(/(^|::)hw_/)` selection does
// not see them and the workspace suite does.

/// A `brightness` with the range a device declared, and nothing else unusual.
fn brightness(min: i64, max: i64, step: i64) -> ControlDesc {
    ControlDesc {
        id: schema::control::ControlId(0x0098_0900),
        name: "Brightness".to_owned(),
        slug: ControlSlug::parse("brightness").expect("a literal slug"),
        control_type: ControlType::Integer,
        range: schema::control::ControlRange { min, max, step },
        default: min,
        // HAS_WHICH_MIN_MAX, which most integer controls on this kernel carry \[PF:12\].
        flags: schema::control::ControlFlags::from_raw(0x1000),
        menu: std::collections::BTreeMap::new(),
        elems: 1,
        elem_size: 4,
        dims: Vec::new(),
        current: Some(schema::ControlValue::Int(min)),
    }
}

/// What [`sweep_for`] said, as text, so a test can read the sentence an operator would.
fn declined(desc: &ControlDesc) -> String {
    match sweep_for(desc) {
        battery::SweepChoice::Declined(why) => why.to_string(),
        battery::SweepChoice::Planned { spec, samples } => {
            panic!("{desc:?} was planned as {samples} sample(s) of {spec:?} rather than declined")
        }
    }
}

#[test]
fn a_brightness_whose_step_is_its_whole_range_is_declined_rather_than_swept() {
    // The seed shape: `0..=64` with a step of 64 plans exactly two values, 0 and 64. It
    // clears `brightness_class_target` — it is writable, active, integer-typed, and its
    // maximum is above its minimum — so the arm *selects* it, and what used to happen next
    // was a sweep on a real camera followed by `assert!(total >= 3)`. A device shape became
    // a red run, and the panic sat twenty lines above `calibrate_restore`, so the camera
    // stayed at 64.
    let desc = brightness(0, 64, 64);
    let why = declined(&desc);
    assert!(why.contains("2 sample(s)"), "{why}");
    assert!(why.contains("0..=64 with a step of 64"), "{why}");
    assert!(
        why.contains("before writing to the camera"),
        "the decline has to say when it happened, because when is the finding: {why}"
    );
}

#[test]
fn a_two_valued_brightness_is_declined_rather_than_swept() {
    // The same count from the other direction — a range of `0..=1`, where the step is 1 and
    // the *range* is what runs out. Both are ordinary devices; neither is a defect.
    let why = declined(&brightness(0, 1, 1));
    assert!(why.contains("2 sample(s)"), "{why}");
    assert!(why.contains("0..=1"), "{why}");
}

#[test]
fn a_single_valued_range_is_declined_and_the_planner_is_not_asked_to_invent_a_second_value() {
    // `min == max` is a legal descriptor and the planner answers one value for it. One is
    // below the floor, so it declines here — not with the planner's refusal, because the
    // planner does not refuse it.
    let why = declined(&brightness(50, 50, 1));
    assert!(why.contains("1 sample(s)"), "{why}");
}

#[test]
fn the_ranges_the_attached_cameras_declare_plan_the_five_samples_e13_transcribed() {
    // The direction that must also hold, or every assertion above is a suite that declines
    // everything. These are the two real ranges E13 recorded — the OBSBOT's `0..=100` at a
    // stride of 25 and the Chicony RGB's `0..=255` at a stride of 63 — and both must still
    // sweep, with the same five samples that entry's table carries.
    for (min, max, step, stride) in [(0_i64, 100_i64, 1_i64, 25_i64), (0, 255, 1, 63)] {
        let desc = brightness(min, max, step);
        let battery::SweepChoice::Planned { spec, samples } = sweep_for(&desc) else {
            panic!("{min}..={max} is the range a camera on this desk declares");
        };
        assert_eq!(spec, SweepSpec::Uniform { step: stride });
        assert_eq!(samples, 5, "{min}..={max}");
    }
}

#[test]
fn the_count_is_the_planners_and_not_arithmetic_repeated_here() {
    // A control whose own step is 7 cannot take a stride of 25, and
    // `engine::sweep::plan` rounds the request up to 28 rather than writing values the
    // device would silently align \[PF:6\]. Naive arithmetic over the stride this file
    // computes would answer five samples; the planner answers four, and the planner is the
    // one the daemon runs. This is the assertion that would go red if the count here were
    // ever re-derived instead of asked for.
    let battery::SweepChoice::Planned { spec, samples } = sweep_for(&brightness(0, 100, 7)) else {
        panic!("a 0..=100 range plans a sweep whatever its step");
    };
    assert_eq!(spec, SweepSpec::Uniform { step: 25 });
    assert_eq!(samples, 4);
}

#[test]
fn a_range_the_planner_refuses_outright_is_a_decline_and_not_a_panic() {
    // A descriptor whose maximum is below its minimum: represented, never corrected (D2),
    // and refused by `engine::sweep::plan` as `empty_range`. Nothing this arm selects can be
    // in that state today — `brightness_class_target` requires `max > min` — and the two
    // predicates are different rules in different crates, so the day they disagree this must
    // be a named decline rather than a `?` that became a panic on somebody's hardware.
    let why = declined(&brightness(200, 100, 1));
    assert!(
        why.contains("the sweep planner refuses brightness"),
        "{why}"
    );
    assert!(why.contains("empty_range"), "{why}");
}

#[test]
fn the_floor_is_a_boundary_and_a_plan_that_just_clears_it_is_swept() {
    // The other side of every decline above, and the reason the floor is a `<` rather than
    // a `<=`: a `0..=2` range plans exactly [`MIN_SAMPLES`] values and must still run. A
    // guard that declined here would be this repair overshooting into the defect it is
    // named after — turning a device shape into a skip instead of into a red run, which is
    // quieter and no more honest.
    let battery::SweepChoice::Planned { samples, .. } = sweep_for(&brightness(0, 2, 1)) else {
        panic!("a three-value plan is exactly what this arm can assert over");
    };
    assert_eq!(samples, MIN_SAMPLES.count);
}
