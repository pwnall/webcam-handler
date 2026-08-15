//! The container-oracle rung over fake-generated recordings (docs/7 **P6d**, docs/9's "Oracle
//! rung accounting" row).
//!
//! `testkit::oracle`'s own suite asks whether the harness can tell a good file from a damaged
//! one, over files it builds by calling the muxers directly. **This asks the question one layer
//! up**: a recording that went through the *product's* path — a backend negotiating a stream, an
//! engine driving turns, a container opened by `imaging::video::Recorder` and closed by it — is
//! handed to two programs that have never read any of it, and they are asked whether it is what
//! the [`RecordReport`] says it is.
//!
//! The difference is not decorative. Everything between `start_stream` and `finish` is what the
//! muxers' own suites replace with a fixture: which format was negotiated, which container D7's
//! pairing chose for it, what interval `imaging::avi::interval_micros` read out of the
//! negotiation, and what `imaging::video::declared_interval` measured across timestamps a
//! *backend* produced. A green run here is the whole of that chain agreeing with libavformat.
//!
//! ## Why the fake and not a camera
//!
//! Both, and in two places. The real-camera half is **R3** —
//! `crates/backends/v4l2/tests/hardware.rs`, `#[ignore]`d, run by `just smoke-hw` on a machine
//! with cameras attached — because design §3.1 puts real hardware there and names no oracle rung
//! of its own. This half runs on **every push**, over `corpus/profiles/*.json` replayed by
//! `webcam-handler-fake`, so a muxer change that stops producing files libavformat can read is
//! caught by `just ci` rather than by whoever next has a camera. No camera is opened here and no
//! frame anything writes could contain a person (rubric A12).
//!
//! Two profiles and not one, because they exercise the two containers and the choice between
//! them is D7's rather than this file's: `chicony-rgb` is a real camera whose first format is
//! MJPG, so its takes are AVI, and `chicony-ir` is a real **GREY-only** camera, so its takes are
//! Y4M and there is no way to ask it for anything else.
//!
//! ## The accounting
//!
//! This suite is **not** `#[ignore]`d, for `crates/daemon/tests/web_browser.rs`'s reason: the
//! rung belongs on every push, so `just ci` runs it and `.config/nextest.toml` gives this binary
//! `success-output = "final"` so its decline cannot be printed into a void. Every test below
//! either prints `oracles: N container claim(s) …` or prints one `SKIP oracles: …` line per
//! missing tool, and `scripts/rung-oracles.sh` turns whichever happened into the named verdict
//! `just gate-g6` records. A run that printed neither is a failure there, because a rung nobody
//! can tell apart from one that was never invoked is the defect the ladder exists for.

use std::cell::Cell;
use std::sync::Arc;

use camino::{Utf8Path, Utf8PathBuf};
use engine::record::{OnDisk, run};
use engine::settle::{Clock, Millis};
use fake::FakeBackend;
use schema::backend::{Camera, CameraBackend};
use schema::capture::{Sink, StreamRequest};
use schema::time::Stamp;
use schema::video::{IntervalSource, RecordReport, RecordRequest, VideoFormat};
use testkit::oracle::{self, Expectation, Oracle, OracleArm, OracleReport, Tools};

/// How long each take runs on the engine's clock, in milliseconds.
///
/// Enough turns that the file holds several frames — one frame measures no interval, and the
/// rate arm is the one this rung most wants to make — and small enough that the whole suite is
/// four short files in a scratch directory.
const BUDGET_MS: u64 = 400;

/// What one turn costs the clock below.
///
/// `engine::record::drive` reads the clock exactly once per turn, so the frame count is
/// arithmetic rather than a guess and nothing here waits on a wall clock (AGENTS: no sleep as
/// synchronisation).
const STEP_MS: Millis = 40;

/// A clock the loop moves, because the test is not inside the loop.
///
/// `engine::record`'s own suite carries the identical shape and its comment carries the
/// argument: note **N60** names a `SteppedClock` whose deadline is the subject and a
/// `FrozenClock` whose deadline is not, and neither can serve a test that hands `drive` a clock
/// and gets an answer back with no moment in between.
#[derive(Debug)]
struct TickingClock {
    now: Cell<Millis>,
}

impl TickingClock {
    fn new() -> TickingClock {
        TickingClock { now: Cell::new(0) }
    }
}

impl Clock for TickingClock {
    fn now_ms(&self) -> Millis {
        let reading = self.now.get();
        self.now.set(reading.saturating_add(STEP_MS));
        reading
    }
}

/// A camera the fake replays from a committed profile.
fn camera_from(profile: &str) -> Box<dyn Camera> {
    let profile = testkit::corpus::load(profile).expect("a committed profile");
    let backend = Arc::new(FakeBackend::from_profile(profile).expect("replays"));
    let id = backend
        .enumerate()
        .expect("enumerate")
        .into_iter()
        .next()
        .expect("one camera")
        .id;
    backend.open(&id).expect("opens")
}

/// Record one take to `path` over the profile named, through the product's own path.
fn record(profile: &str, path: &Utf8Path) -> RecordReport {
    let mut camera = camera_from(profile);
    let request = RecordRequest {
        stream: StreamRequest::default(),
        duration_ms: Some(BUDGET_MS),
        sink: Sink::ServerPath {
            path: path.to_owned(),
        },
        wait: false,
    };
    run(
        camera.as_mut(),
        &request,
        &mut OnDisk,
        &TickingClock::new(),
        Stamp::epoch(),
    )
    .expect("the fake records")
}

/// What the report claims, plus — for AVI — every frame's length as **our own independent
/// reader** sees it.
///
/// The payload arm is the only thing that catches a file cut mid-frame (`nb_frames` is a header
/// claim and a truncated JPEG still decodes), and nothing in a `RecordReport` carries per-frame
/// lengths. `imaging::avi::read::read_stream` does: it is the reader P6a wrote from the RIFF/AVI
/// specification before the muxer existed, sharing no constant with it. So this arm becomes
/// *two* independent readers agreeing about every frame's length, which is worth more than
/// either alone and is exactly what an oracle rung is for.
///
/// Y4M gets none, and deliberately: a Y4M frame's length is `width × height` for `Cmono` by
/// construction, so a number derived here would be this file re-deriving the layout the sink
/// under test uses — a claim that agrees with itself. The count, the geometry, the rate and
/// playability all still run.
fn expectation(report: &RecordReport, bytes: &[u8]) -> Expectation {
    let expected = Expectation::for_take(
        report.format,
        report.negotiated.width,
        report.negotiated.height,
        &report.summary,
    );
    match report.format {
        VideoFormat::Avi => {
            let stream = imaging::avi::read::read_stream(bytes)
                .expect("the strict reader accepts a finished recording");
            expected.with_payload_bytes(
                stream
                    .frames
                    .iter()
                    .map(|frame| u32::try_from(frame.bytes.len()).expect("fits"))
                    .collect(),
            )
        }
        VideoFormat::Y4m => expected,
    }
}

/// Whether this host has both oracles, printing the counted decline when it does not.
///
/// The decline is the rung's own accounting and it names three things — what was missing, what
/// therefore went unasked, and the remedy — because AGENTS' primary consumer has no hands to
/// work any of them out with.
fn oracles_or_decline(what: &str) -> bool {
    let missing: Vec<Oracle> = Oracle::ALL
        .iter()
        .copied()
        .filter(|oracle| oracle::OnPath.locate(*oracle).is_none())
        .collect();
    for oracle in &missing {
        let arms: Vec<&str> = OracleReport::arms_of(*oracle)
            .into_iter()
            .map(OracleArm::as_str)
            .collect();
        println!(
            "SKIP oracles: {oracle} is not on this host, so {} claim(s) about {what} went \
             unasked — {} — and nothing here says {}; remedy: {}",
            arms.len(),
            arms.join(", "),
            oracle.answers(),
            oracle.install_hint()
        );
    }
    missing.is_empty()
}

fn scratch_path(dir: &tempfile::TempDir, name: &str) -> Utf8PathBuf {
    Utf8PathBuf::from_path_buf(dir.path().join(name)).expect("utf-8 scratch")
}

#[test]
fn a_recording_the_product_path_produced_is_the_file_two_third_parties_read_back() {
    // The rung. Two profiles, two containers, four claims each, and the expectation is built
    // from the report the engine answered — so an oracle disagreeing with it is the engine
    // having described a file it did not write.
    if !oracles_or_decline("a recording made over the fake backend") {
        return;
    }
    let dir = engine::paths::scratch_dir().expect("a scratch directory");
    let mut checked = 0;
    for (profile, name, container) in [
        ("chicony-rgb", "rgb.avi", VideoFormat::Avi),
        ("chicony-ir", "ir.y4m", VideoFormat::Y4m),
    ] {
        let path = scratch_path(&dir, name);
        let report = record(profile, &path);
        assert_eq!(report.format, container, "{profile}");
        assert!(
            report.summary.frames_written > 1,
            "{profile}: {} frame(s) is not a recording an oracle can say anything about",
            report.summary.frames_written
        );

        let bytes = std::fs::read(path.as_std_path()).expect("the file the report named exists");
        assert_eq!(
            u64::try_from(bytes.len()).expect("fits"),
            report.summary.bytes_written,
            "{profile}: the file's own length is not the one the summary reported"
        );

        let expected = expectation(&report, &bytes);
        let found = oracle::validate(&path, &expected);
        found.print();
        assert!(found.is_green(), "{profile}: {found}");
        assert_eq!(found.ran(), OracleArm::ALL.len(), "{profile}: {found}");
        checked += 1;
    }
    assert_eq!(
        checked,
        VideoFormat::ALL.len(),
        "the rung walked fewer containers than this build writes"
    );
}

#[test]
fn the_rate_libavformat_reads_back_is_the_mean_the_take_measured_in_either_container() {
    // The claim note **N106**'s amendment turns on, made at the layer where the number is
    // produced rather than at the layer where it is formatted. Both containers now patch their
    // header at close — AVI's two binary fields, Y4M's zero-padded `F` denominator — and a
    // third party reading the padded field as the rate it names is the measurement that
    // retired N106's ground (evidence **E17**).
    //
    // The `Measured` assertion is what makes the rate arm non-vacuous here: a container that
    // declared the *negotiated* interval would also be internally consistent, and `ffprobe`
    // would agree with it just as happily. What it would not be is the interval the frames
    // arrived at.
    if !oracles_or_decline("the rate a finished take declares") {
        return;
    }
    let dir = engine::paths::scratch_dir().expect("a scratch directory");
    for (profile, name) in [("chicony-rgb", "rate.avi"), ("chicony-ir", "rate.y4m")] {
        let path = scratch_path(&dir, name);
        let report = record(profile, &path);
        assert_eq!(
            report.summary.interval_source,
            IntervalSource::Measured,
            "{profile}: a take of {} frames declared an interval it did not measure",
            report.summary.frames_written
        );
        let mean = report
            .summary
            .measured_interval_us()
            .expect("a take of several frames measures a mean");
        assert_eq!(
            u64::from(report.summary.declared_interval_us),
            mean,
            "{profile}: the header declares one number and the subtraction gives another"
        );

        let bytes = std::fs::read(path.as_std_path()).expect("read");
        let found = oracle::validate(&path, &expectation(&report, &bytes));
        found.print();
        assert!(
            found.failures_for(OracleArm::Rate).is_empty(),
            "{profile}: {found}"
        );

        // And the same file with the rate mis-stated by one microsecond is a failure, so the
        // arm above is a comparison rather than a formality.
        let lied = Expectation {
            declared_interval_us: report.summary.declared_interval_us + 1,
            ..expectation(&report, &bytes)
        };
        assert!(
            !oracle::validate(&path, &lied)
                .failures_for(OracleArm::Rate)
                .is_empty(),
            "{profile}: a rate one microsecond out went unnoticed"
        );
    }
}
