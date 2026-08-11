//! What a real signal does to a real `wchd` that is in the middle of something (docs/7
//! P4e-ii).
//!
//! `daemon::shutdown`'s own unit tests drive the teardown from values: a scripted signal
//! source, a scriptable transport, a recording supervisor, and a paused clock. They are how
//! the *order* is asserted, and they can assert combinations no real server can be made to
//! produce. What none of them can say is that any of it happens when the **kernel** delivers
//! `SIGTERM` to the shipped binary — a signal is a fact about a process, so the daemon here is
//! a real `wchd` spawned as a subprocess, exactly as `tests/lock.rs` and `tests/systemd.rs`
//! spawn one for their own process-shaped claims. The three share `support/wchd.rs`.
//!
//! Two tests, one per signal, over one helper. AGENTS states `SIGTERM ≡ SIGINT` as a daemon
//! rule and `daemon::shutdown`'s header states it as a design one; here it is a **comparison**
//! — both tests compare what they observed against the same [`a_drained_stop`], so a build
//! that drained for one signal and cut the other off fails on the field that differs rather
//! than on a list somebody remembered to copy twice.
//!
//! ## Holding a sweep still, in a process this test cannot reach into
//!
//! The in-process subscription suite arms a `Gate` around the fake backend and releases it
//! over a channel. Nothing of the sort is reachable here: `fake::Fault` is an in-process menu,
//! no profile field and no environment variable slows the fake down, and nothing in the fake
//! or the engine sleeps. What the sweep does have is a **blocking write to a path this test
//! can compute**: every sample photo goes through `engine::photo::WhereverTheCallerSaid`,
//! which is `std::fs::write`, deliberately *not* the non-blocking descriptor note N51 gave the
//! photo verb (`crates/engine/src/calibrate.rs` says why: the destination is the session
//! tree's own, under a directory this process made, so there is nothing there for a client to
//! swap). `std::fs::write` on a **fifo with no reader blocks in `open(2)`** — which is the
//! same measured fact `tests/mutating_verbs.rs` uses from the other side, where it is the
//! hazard rather than the tool.
//!
//! So the wedge is a fifo at the path the sweep is about to write, pre-created by this test,
//! and the release is this test opening the read end. The path is derivable rather than
//! observed: `SessionStore::session_dir` from the `Session` the daemon answered with,
//! `SessionStore::photo_dir_rel` for the pass, and the value from the sweep's own plan through
//! the same `engine::sweep::plan` the daemon runs.
//!
//! **Two values, and the fifo on the second**, which is what makes "unfinished" a positive
//! observation: the first sample completes and its progress reaches the subscriber over the
//! socket, and the second is still inside `open(2)` when the signal arrives.
//!
//! ## One connection, and why that is the honest shape rather than the convenient one
//!
//! The subscription and the sweep ride **the same** WebSocket, which is what
//! `subscriptions.rs`'s `one_websocket_connection_carries_a_subscription_and_a_call_at_the
//! _same_time` says the transport is for. It also decides a race this suite would otherwise be
//! measuring instead of the daemon: jsonrpsee sends a cancelled subscription's close frame
//! from a task it spawned, *after* the subscription body returns, and it does not hold a
//! connection open for one — so a subscription alone on an idle connection is racing the
//! daemon's own teardown to the socket. `daemon::shutdown`'s step 3 waits for the
//! subscriptions to end before it stops the transport, which is what turns that from an even
//! race into a rare one (measured: without the wait, this suite saw the ending about half the
//! time). A connection with a **call in flight** removes what is left of it: jsonrpsee's
//! graceful shutdown drives that call to completion before the connection closes, so the close
//! frame has the whole drain window to be written in. The residual — an *idle* connection's
//! subscription, under enough load — is `daemon::shutdown`'s to state and not this suite's to
//! hide.
//!
//! ## Nothing here sleeps, and the one window that is a window
//!
//! Starting is the daemon's own stderr line (`support/wchd.rs`); the sweep reaching its second
//! sample is the subscription delivering the first one's events; the teardown reaching the
//! subscriptions is the ending frame arriving on that stream; and stopping is `wait`, which
//! returns when the kernel has reaped the process. No step waits out a duration.
//!
//! What *is* a duration is the daemon's own: from the signal until this test opens the fifo,
//! the daemon is inside [`schema::limits::DAEMON_SHUTDOWN_DRAIN_MS`] — twenty seconds — and a
//! machine that stalled this test for longer than that between two adjacent statements would
//! see the daemon give up on the drain, warn, and exit with the second sample unwritten. The
//! statements are adjacent for exactly that reason, and note **N60** is why it is written down
//! rather than left to be discovered: a suite whose assertions can be decided by load is a
//! suite whose failures get re-run instead of read.

#[path = "support/supervised.rs"]
mod supervised;
#[path = "support/wchd.rs"]
mod wchd;
#[path = "support/ws.rs"]
mod ws;

use std::os::unix::fs::FileTypeExt;
use std::process::Command;

use camino::Utf8Path;
use engine::store::SessionStore;
use rustix::process::Signal;
use schema::control::ControlDesc;
use schema::profile::DeviceProfile;
use schema::progress::{CalibrationProgress, ProgressEvent};
use schema::session::{Session, SessionRef, SweepRequest, SweepSpec};
use serde_json::{Value, json};

use crate::supervised::{replaying, signal};
use crate::wchd::{Daemon, Scratch};
use crate::ws::Ws;

/// The id the sweep request carries, so its answer can be recognised as *its* answer.
///
/// Out of the way of [`Ws::call`]'s own counter: the sweep is written with [`Ws::write`] and
/// collected much later with [`Ws::answer`], which matches no ids, so the assertion that the
/// frame belongs to the request is this test's to make.
const SWEEP_REQUEST_ID: u32 = 1_000;

/// How many samples the sweep takes: one that finishes, one that is wedged.
const SAMPLES: usize = 2;

/// Everything a client, a filesystem and a service manager can see about one stop.
///
/// A record rather than a run of assertions inside the helper, because the claim P4e-ii's row
/// is worded for is that the *two signals produce the same outcome* — and an outcome that is
/// two lists of assertions can drift into two different lists while both stay green.
/// `daemon::shutdown`'s own `sigterm_and_sigint_are_one_teardown` compares recordings for the
/// same reason, one layer down and over values.
#[derive(Debug, PartialEq)]
struct Drained {
    /// The payload the open subscription's stream ended with.
    ending: Value,
    /// Whether the daemon was still running once that ending had arrived.
    alive_with_work_in_flight: bool,
    /// How many samples the sweep's own answer carried.
    swept_in_flight: usize,
    /// How many the session document on disk holds, read the way `calibrate_status` reads it.
    swept_on_disk: usize,
    /// Whether the wedged sample's bytes really crossed the fifo this test put in its way.
    photo_crossed_the_fifo: bool,
    /// Whether the process exited zero.
    exited_cleanly: bool,
    /// Whether anything still holds D9's advisory lock over the state directory.
    store_still_locked: bool,
    /// Whether the socket file is still on disk.
    socket_still_there: bool,
}

/// What a stop looks like from outside, whichever signal asked for it.
///
/// Every field is a claim, and none of them implies the others:
///
/// - the **ending** is the whole of what step 3 of `daemon::shutdown`'s order buys. A daemon
///   that stopped its transport without cancelling would end this stream too — with nothing at
///   all, on a socket that closed, indistinguishable from the process crashing — so the
///   assertion is the *reason*, not the end;
/// - **alive with work in flight** is the direct observation that the daemon does not exit
///   while a request it accepted is unfinished. It is a `try_wait` at one instant, which is
///   why it is not the only drain assertion here;
/// - **swept in flight and swept on disk** are that same claim made in a way a loaded machine
///   cannot decide. The sweep was signalled at its second sample of two, and both numbers are
///   [`SAMPLES`]: the daemon answered the request it had accepted, and the document it had
///   been writing all along holds the sample it was inside `write(2)` for. A build that exited
///   without draining kills that write, and the second sample is simply not there;
/// - **the photo crossed the fifo** is what says the wedge was the sweep and not this test's
///   idea of it: the bytes came out of the daemon, through the path it computed, into a reader
///   this test opened;
/// - **exiting cleanly** is what a service manager reads. A drain that ran out of time is
///   still a stop somebody asked for, so a non-zero exit here would ask `Restart=on-failure`
///   to start the daemon again on a machine that is shutting down;
/// - **the lock is free** is D9's ordered release, which `daemon::state`'s header deferred to
///   this sub-milestone — and it is the kernel's guarantee only because the process is gone;
/// - **the socket is still there** because `daemon::uds` argues at length that it is
///   deliberately not unlinked: the next `wchd` binds by replacing it under a directory
///   descriptor it verified, and a stop that tidied it away would be racing a daemon that had
///   already started.
fn a_drained_stop() -> Drained {
    Drained {
        // Read from the daemon's own constant rather than written out: a build that changed
        // the token a client branches on changes this with it.
        ending: json!({ "ended": daemon::events::SHUTTING_DOWN }),
        alive_with_work_in_flight: true,
        swept_in_flight: SAMPLES,
        swept_on_disk: SAMPLES,
        photo_crossed_the_fifo: true,
        exited_cleanly: true,
        store_still_locked: false,
        socket_still_there: true,
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_sigterm_mid_sweep_ends_the_subscription_with_a_reason_and_drains_what_it_accepted() {
    // `systemctl stop`, and the signal every service manager uses. What makes this a test of
    // the *drain* rather than of the signal is that the daemon is asked to stop while it is
    // provably unfinished — inside a sweep's second sample, with a subscriber watching.
    assert_eq!(stopped_by(Signal::TERM).await, a_drained_stop());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_sigint_mid_sweep_ends_the_subscription_with_a_reason_and_drains_what_it_accepted() {
    // Ctrl-C in the terminal a `wchd` was started from, which AGENTS says is the same request
    // — and this is where "the same" is a measurement rather than a sentence. The comparison
    // is against the same value the SIGTERM test compares against, so a build that branched on
    // `Stop` two lines later fails here rather than in review.
    assert_eq!(stopped_by(Signal::INT).await, a_drained_stop());
}

/// Start a `wchd`, wedge a sweep inside it, signal it, and answer what everybody saw.
///
/// The arrangement, in the order it has to happen in:
///
/// 1. a `wchd` replaying the committed synthetic profile — the camera count, the control and
///    its range all have to be numbers this test knows, and a v4l2 daemon would be replaying
///    whatever is plugged into the machine running CI. It is the same document `Fixture`
///    hands the in-process suites, so "this camera" means the same thing here as there;
/// 2. a subscription, opened **before** the sweep, because a fan-out buffers nothing for a
///    subscriber who has not arrived (`daemon::events`);
/// 3. a session, a plan, and a fifo at the second sample's photo path;
/// 4. the sweep, written and not awaited. The client is `support/ws.rs`'s, which has no
///    request timeout of any kind — `crates/api/src/lib.rs` warns that a *generated* client's
///    has to be raised or disabled for this one call, because a sweep is minutes of camera
///    time and jsonrpsee's default is sixty seconds;
/// 5. the first sample's events, read off the subscription, which is how this test learns the
///    sweep is in flight rather than guessing;
/// 6. the signal, delivered by the kernel to the pid this test spawned;
/// 7. everything after it.
async fn stopped_by(stop: Signal) -> Drained {
    let scratch = Scratch::new();
    // Loaded from the file the daemon is about to be pointed at rather than constructed
    // beside it — `testkit::fixtures::load_synthetic_basic` exists for exactly that
    // distinction — so the device this test plans a sweep against and the device the daemon
    // replays are provably one document.
    let profile = testkit::fixtures::load_synthetic_basic().expect("the committed fixture");
    let mut daemon = Daemon::serving(
        replaying(&scratch, &testkit::fixtures::synthetic_basic_path()),
        &scratch.socket(),
    );
    let store = SessionStore::from_env(&scratch.env()).expect("the fixture sets both variables");

    // **The daemon never self-daemonizes**, and D9's lock record is where that is visible: the
    // process holding the state directory is the one this test spawned, not a child it forked
    // off and left behind. `tests/lock.rs` asserts the *refusal* a `wch` meets carries the
    // same pid; this is the record itself, and it is here because everything below reads that
    // record again after the process is gone — an assertion that it is free afterwards is
    // worth nothing without one that it was held before.
    let holder = store
        .holder()
        .expect("a serving daemon holds the state directory for its lifetime");
    assert_eq!(
        holder.pid,
        daemon.pid(),
        "the state directory is held by a process other than the daemon this test started"
    );

    // **One connection, carrying the subscription and the sweep at once** — the shape
    // `subscriptions.rs` establishes the transport can do, and here it is what makes the
    // ending an *ordering* rather than a race. See this suite's header: a connection with a
    // call in flight is held open by jsonrpsee's graceful shutdown until that call is
    // answered, so the frame the cancellation produces has the whole drain window to be
    // written in, rather than however many microseconds separate two tasks.
    let mut client = Ws::connect(&scratch.socket()).await;
    let subscription = result(
        client.call("wch_subscribe_calibration", json!({})).await,
        "wch_subscribe_calibration",
    );

    let listed = client.call("wch_list", json!({})).await;
    let cameras = listed["result"]["cameras"]
        .as_array()
        .expect("a camera list");
    assert_eq!(cameras.len(), 1, "one profile replays one camera: {listed}");
    let camera = cameras.first().expect("one camera")["id"].clone();

    let session: Session = serde_json::from_value(result(
        client
            .call(
                "wch_calibrate_start",
                json!({
                    "camera": camera,
                    "task": "stopping",
                    "goal": "a legible frame",
                    "criteria": [],
                }),
            )
            .await,
        "wch_calibrate_start",
    ))
    .expect("a session document");
    let which = SessionRef::Id { id: session.id };
    let control = swept_control(&profile);
    result(
        client
            .call(
                "wch_calibrate_plan",
                json!({
                    "camera": camera,
                    "session": which,
                    "controls": [control.slug],
                    "order": false,
                }),
            )
            .await,
        "wch_calibrate_plan",
    );

    // The wedge. Both values come from the planner the daemon itself runs, so the file name
    // this test creates is the one the sweep will compute — an explicit value is aligned and
    // clamped to the control's range before it becomes a sample, and a fifo at a path nothing
    // writes would leave this test asserting a stop that had nothing to drain.
    let sweep = SweepRequest::new(
        control.slug.clone(),
        SweepSpec::Explicit {
            values: vec![control.range.min, control.range.max],
        },
    );
    let planned = engine::sweep::plan(&control, &sweep.plan, sweep.allow_motion)
        .expect("a writable scalar control with an ordered range");
    assert_eq!(
        planned.values.len(),
        SAMPLES,
        "the wedge has to be the *last* of a sweep that has already emitted: {planned:?}"
    );
    let photos = store
        .session_dir(&session)
        .join(SessionStore::photo_dir_rel(&control.slug, 0).expect("a usable slug"));
    std::fs::create_dir_all(photos.as_std_path()).expect("the session directory is this test's");
    let wedge = photos.join(format!(
        "{}.{}",
        planned.values.last().expect("two values"),
        sweep.photo_format.extension()
    ));
    mkfifo(&wedge);

    // The sweep, in flight and unanswered.
    client
        .write(
            &json!({
                "jsonrpc": "2.0",
                "id": SWEEP_REQUEST_ID,
                "method": "wch_calibrate_sweep",
                "params": { "camera": camera, "session": which, "request": sweep },
            })
            .to_string(),
        )
        .await;

    // …until it is provably inside the second sample. Three events, in the order
    // `engine::calibrate` emits them, and the third is the one that says a whole sample was
    // taken: what follows it is the write this test has put a fifo in front of.
    let seen = until_the_first_sample_is_taken(&mut client, &subscription).await;
    assert!(
        matches!(
            seen.as_slice(),
            [
                CalibrationProgress::SweepStarted { .. },
                CalibrationProgress::ValueSet { index: 1, .. },
                CalibrationProgress::SampleTaken { index: 1, .. },
            ]
        ),
        "the sweep did not reach its second sample: {seen:?}"
    );

    // ---------------------------------------------------------------- the signal, for real
    //
    // The kernel delivers it to the pid this test spawned. Everything from here to the fifo's
    // read end is inside the daemon's own drain bound — see this suite's header.
    signal(&daemon, stop);
    // **Watched red**: against a teardown that cancels nothing, this read never returns and
    // the run becomes a named nextest TIMEOUT at 180s rather than a failure — because the
    // release below is downstream of it, and a sweep nobody released is a daemon nobody can
    // finish waiting for. That is the shape `.config/nextest.toml`'s deadline exists to give,
    // and it is the same one `subscriptions.rs`'s `until_the_sweep_stops` records.
    let ending = client.ending(&subscription).await;
    let alive_with_work_in_flight = daemon.still_running();

    // The release. A thread rather than a bare open, because a fifo's buffer is 64 KiB and a
    // photo need not be: the writer stays parked until somebody is reading, so the reader has
    // to read to end-of-file, which is the daemon closing the file it has finished writing.
    let crossing =
        tokio::task::spawn_blocking(move || std::fs::read(wedge.as_std_path()).expect("the fifo"));

    // The request the daemon accepted, answered. This frame is the drain from the client's
    // side: jsonrpsee's graceful shutdown drives a connection's in-flight calls to completion
    // before it closes, and a daemon that stopped without draining takes the connection down
    // with the answer unwritten — which fails on the read rather than here.
    let answered = client.answer().await;
    assert_eq!(
        answered["id"],
        json!(SWEEP_REQUEST_ID),
        "the answer belonged to another request: {answered}"
    );
    let swept: Session =
        serde_json::from_value(result(answered, "wch_calibrate_sweep")).expect("a session");
    let photo = crossing.await.expect("the reading thread");

    let exited_cleanly = daemon.wait();

    // And the same claim from the other side of the socket, read the way `calibrate_status`
    // reads it — `method_surface.rs` and note N47's idiom. This is the assertion that does not
    // depend on catching a process at an instant: the document holds what the sweep recorded,
    // and the sweep recorded its second sample only if it was allowed to finish it.
    let on_disk = engine::lifecycle::status(&store, &session.fingerprint, &which)
        .expect("the store walks after the daemon is gone");

    Drained {
        ending,
        alive_with_work_in_flight,
        swept_in_flight: samples(&swept, &control),
        swept_on_disk: samples(&on_disk.session, &control),
        photo_crossed_the_fifo: photo.get(..2) == Some(&[0xff, 0xd8][..]),
        exited_cleanly,
        // Released because the process is gone, which is the whole of `crate::state`'s
        // argument: `flock` ends with the last descriptor on its open file description.
        store_still_locked: store.holder().is_some(),
        // Still there, and still a socket. `daemon::uds` states why the daemon does not unlink
        // it, so a build that started tidying up on the way out goes red here.
        socket_still_there: std::fs::symlink_metadata(scratch.socket().as_std_path())
            .is_ok_and(|meta| meta.file_type().is_socket()),
    }
}

/// The control this suite sweeps: writable, scalar, and the camera's own.
///
/// Taken off the profile rather than named, exactly as `support/fixture.rs` takes it off the
/// device: a literal here is a literal that stops being true the day the fixture changes.
/// Not a menu, because a menu's indices have holes \[PF:2\], so "the values between these two"
/// is not a thing a sweep planner can derive for one.
fn swept_control(profile: &DeviceProfile) -> ControlDesc {
    profile
        .invariant
        .controls
        .iter()
        .find(|desc| desc.is_writable() && !desc.control_type.is_menu())
        .cloned()
        .expect("the synthetic profile has a writable scalar control")
}

/// How many samples a session's document holds for `control`.
fn samples(session: &Session, control: &ControlDesc) -> usize {
    session
        .controls
        .get(&control.slug)
        .map_or(0, |entry| entry.samples.len())
}

/// The `result` of an answer, or a panic naming the call that was refused.
fn result(answer: Value, called: &str) -> Value {
    answer
        .get("result")
        .cloned()
        .unwrap_or_else(|| panic!("{called} was refused: {answer}"))
}

/// Read this subscription's events until a whole sample has been taken, and answer them all.
///
/// The synchronization the header calls "the sweep reaching its second sample": each read ends
/// when the **daemon writes**, so a sweep that never emitted is a nextest `TIMEOUT` with a
/// test's name on it rather than a hang (`.config/nextest.toml`).
async fn until_the_first_sample_is_taken(
    watching: &mut Ws,
    subscription: &Value,
) -> Vec<CalibrationProgress> {
    let mut seen = Vec::new();
    loop {
        let params = watching.notification().await;
        if params.get("subscription") != Some(subscription) {
            continue;
        }
        let event: ProgressEvent =
            serde_json::from_value(params["result"].clone()).expect("a progress event decodes");
        let taken = matches!(event.progress, CalibrationProgress::SampleTaken { .. });
        seen.push(event.progress);
        if taken {
            return seen;
        }
    }
}

/// A fifo at `path`, made by the one program that can make one without a syscall wrapper.
///
/// This crate is `#![forbid(unsafe_code)]` and the workspace confines `libc` to the V4L2
/// backend, so `mkfifo(3)` is out of reach from here. `mkfifo(1)` is coreutils, and
/// `tests/mutating_verbs.rs` already shells out to it for the same reason and against the same
/// blocking `open(2)`; a host without it fails loudly rather than skipping, because a skip that
/// reads as a pass is the one thing AGENTS forbids more than a shelled-out fixture.
fn mkfifo(path: &Utf8Path) {
    let made = Command::new("mkfifo")
        .arg(path.as_str())
        .status()
        .expect("mkfifo(1) is coreutils and this test needs a fifo it cannot make itself");
    assert!(made.success(), "mkfifo {path}: {made:?}");
    assert!(path.exists(), "{path}");
}
