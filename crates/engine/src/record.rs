//! Recording video: the device half, the sink half, and one loop built from both
//! (design **D7**, **D10**; docs/7 P6b).
//!
//! [`crate::photo`] is this module's sibling. That one assembles **an** answer — start,
//! settle, one frame, render, write, stop — because a photo is one reply to one request. A
//! recording is the shape [`crate::preview`] already has: a stream that runs for seconds or
//! minutes, and frames that are not answers to anything until the file is closed. So this
//! module is built like that one, and deliberately not on top of the photo path.
//!
//! ## Why a recording is *many short commands* rather than one long one
//!
//! `crate::actor` gives each open camera one blocking thread and one command channel, and
//! everything the daemon asks of a camera goes through it **in arrival order**. A recording
//! written as one long command would therefore make `record_stop` undeliverable: the stop
//! would queue behind the recording it is trying to stop, and the only thing that could end
//! the take would be the take's own duration. That is not a latency problem, it is a verb
//! that does not work.
//!
//! So the device half is turn-based, exactly as `crate::preview`'s is: [`start`] opens the
//! stream, [`turn`] takes **one** frame and returns, [`stop`] ends it. The actor is free
//! between every pair of frames, so a `record_stop` waits at most one
//! [`limits::FRAME_DEADLINE_MS`]. `daemon::preview`'s `drive`/`pump` is the shape the daemon
//! will wrap around it at P6c; note **N111** records why the loop is the caller's and not
//! this module's, because that reason is not visible from the code.
//!
//! ## Two halves, so a later phase can fan one frame to both consumers
//!
//! The device half returns a [`Frame`]; the sink half ([`Recording`]) takes one and never
//! touches a device. Nothing here couples them, and that is deliberate: docs/7 P6c wants the
//! daemon to hand the *same* frame to a preview's viewers and to a muxer, and a `turn` that
//! published straight into a recorder would make that a second capture — a second
//! `STREAMON` on a node V4L2 allows one streamer on, which is to say impossible. The split
//! costs one `Frame` returned by value and buys the arrangement P6c needs.
//!
//! [`drive`] is the third piece, for callers that have no actor at all: `webcam-handler-cli`
//! opens the camera in process (`InProcess`) and wants one call that records and returns. It
//! is the two halves and a loop, and it holds the device for the whole take — which is why
//! the daemon must not use it.
//!
//! ## A recording does not settle first
//!
//! `crate::capture::grab` discards frames until the sensor has converged \[PF:11\], because
//! one dark frame is the whole of a photo. A recording's early frames are *visible*, beside
//! everything that follows, and an agent filming a transition asked for the moment it asked
//! for — so discarding them would move the recording's start away from where the caller put
//! it, silently, by a tenth of a second at 30 fps and by whatever the settle cost at any
//! other rate. [`schema::video::RecordRequest`] therefore carries no settle policy at all,
//! and a caller that wants a warm sensor takes a photo first. Note **N111** carries the
//! decision.
//!
//! ## Two bounds, and the report carries both readings
//!
//! A take is bounded by the caller's **duration**, checked on the engine's monotonic clock
//! ([`crate::settle::Clock`], the seam a test owns), and by the container's
//! `RecordingCaps` — [`caps`] is the one place `webcam-handler-schema::limits` becomes those.
//! They are different bounds on purpose: the duration is what the caller asked for, and the
//! caps are policy backstops whose own docs say they should never be the thing that fires.
//! The third bound is [`limits::RECORDING_MAX_EMPTY_TURNS`], which covers the case neither
//! of the first two can — a device answering promptly and delivering nothing on a clock that
//! is not advancing (`crate::capture`'s `MAX_SETTLE_ROUNDS` is the same argument).
//!
//! [`schema::video::RecordReport`] carries the engine's elapsed milliseconds beside the
//! container's `span_us`, which is measured on the *driver's* frame timestamps. That pair is
//! docs/7 **P6d**'s declared-vs-wall-clock duration bound, and `RecordReport::wall_clock_ms`
//! is where the difference between the two is explained.
//!
//! ## Every fault leaves a parseable file
//!
//! docs/7 P6b asks for it in those words, and it costs two things. The first is [`Files`], a
//! seam with a real implementation and a scriptable [`SinkFault`] menu, so a full disk is a
//! condition a test can arrange rather than one a reviewer imagines. The second is the
//! ordering below: a request is refused, the stream is negotiated, and only then is the file
//! **created** — so a recording refused for its extension, its duration or a container the
//! camera cannot fill does not truncate a file that was already there (note **N51**'s lesson,
//! one verb along).
//!
//! What "parseable" means differs by how the take died, and both halves are asserted rather
//! than described. A **device** failure still runs the close, so the file has its `idx1` and
//! `imaging::avi::read::read_stream` — the strict reader — accepts it. A **sink** failure
//! poisons the writer, so there is no index at all, and the file is read by
//! `imaging::avi::read::recover_frames` and *refused* by the strict one. That distinction is
//! the whole content of D7's "a crash leaves a recoverable `movi` stream".
//!
//! ## A recording does not suspend a preview, and that is not an oversight
//!
//! `crate::preview::while_suspended` lets a photo take a stream from a live preview and give
//! it back, bounded by [`limits::PREVIEW_SUSPEND_MAX_MS`] — ten seconds. A recording's
//! *default* duration is [`limits::DEFAULT_RECORDING_MS`], which is also ten seconds, so the
//! mechanism a photo uses cannot serve a recording without either breaking that bound or
//! refusing the ordinary case it exists to serve. So a camera somebody is watching answers
//! [`Error::Busy`] here, from the device, which is an availability fact and not a capability
//! one (E3). What to do about it — end the preview, refuse the recording, or ask the owner —
//! is a decision for the phase that has both consumers in front of it (docs/7 P6c), not one
//! this module should take on its behalf.
//!
//! ## A frame may contain a person
//!
//! AGENTS, rubric A12. Nothing in this file logs, and the only things that reach an error are
//! numbers, paths the caller named and this module's own vocabulary. [`Recording`]'s `Debug`
//! is the muxer's, which prints counts and never its sink.

use std::io::{Seek, SeekFrom, Write};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use camino::{Utf8Path, Utf8PathBuf};
use imaging::video::{FrameOutcome, Recorder, RecordingCaps, RecordingParams};
use schema::camera::CameraId;
use schema::capture::{Frame, NegotiatedStream};
use schema::error::{Error, Result};
use schema::limits;
use schema::time::Stamp;
use schema::video::{RecordReport, RecordRequest, RecordingEnd, VideoFormat};
use schema::vocabulary::closed_vocabulary;

use crate::actor::OpenCamera;
use crate::settle::{Clock, Millis};

/// What `webcam-handler-schema::limits` says a recording may cost.
///
/// **The one place those three constants become bounds a muxer enforces.**
/// `imaging::video::RecordingCaps` deliberately has no `Default` — its own header argues why:
/// "a caller cannot forget to bound a recording, because there is no bound to forget" — so
/// the policy is the engine's and this function is it.
///
/// All three are *backstops* rather than the bound an honest take meets, and their own docs
/// say so: [`limits::MAX_RECORDING_FRAMES`] is above what the duration cap admits at the
/// fastest mode this project has measured, and [`limits::MAX_RECORDING_BYTES`] is the size a
/// 24 MB/s stream reaches at about ninety seconds. What ends an ordinary recording is the
/// caller's own duration, checked by [`drive`] on the engine's clock — which is why the span
/// cap here is [`limits::MAX_RECORDING_MS`] and not the request's duration. Setting it to the
/// request's would give one take two bounds that describe the same thing, and which of them
/// fired first would depend on whether the driver's clock agreed with ours.
#[must_use]
pub fn caps() -> RecordingCaps {
    RecordingCaps {
        max_bytes: limits::MAX_RECORDING_BYTES,
        max_frames: limits::MAX_RECORDING_FRAMES,
        max_span: Duration::from_millis(limits::MAX_RECORDING_MS),
    }
}

// ------------------------------------------------------------------ the device half

/// What [`start`] negotiated, and everything the sink half needs to open a file for it.
///
/// Every field is public because [`Recording::begin`] takes one of these and a test that
/// wants to talk about a *cap* has to be able to tighten one: the shipped bounds are
/// backstops measured in gibibytes and thousands of frames ([`caps`]), so a suite that could
/// only drive the shipped values could not reach `CapReached::Size` or `CapReached::Frames`
/// at all. The alternative — a `caps` parameter threaded through the request — would put a
/// policy knob on the wire for the sake of a test, which is the worse trade.
#[derive(Debug, Clone)]
pub struct Opened {
    /// The camera the frames are coming off, for the report.
    pub camera: CameraId,
    /// What the device agreed to deliver, with every difference from the request (D5).
    pub negotiated: NegotiatedStream,
    /// The container this recording goes in, decided by [`VideoFormat::resolve`].
    pub format: VideoFormat,
    /// What the muxer needs before its first frame.
    pub params: RecordingParams,
    /// How long the take may run on the engine's clock, from
    /// [`RecordRequest::budget_ms`].
    pub budget_ms: u64,
}

/// Start the stream a recording runs on, and refuse a container the stream cannot fill.
///
/// The three request refusals happen **first**, before `VIDIOC_STREAMON`: a sink that is not
/// a path, a path whose extension names a container this build does not write, and a duration
/// past the cap are all statements about the *request*, and paying a stream for one of them
/// would mean a frame of whoever is in front of the lens was captured for a request nobody
/// was going to honour (`crate::photo::take`'s first statement makes the identical argument,
/// debt D-1).
///
/// Then the negotiation, and then D7's pairing through [`VideoFormat::resolve`] — the one
/// home for it. A container that cannot carry the negotiated stream is refused *with the
/// stream stopped*, for `crate::preview::start`'s reason: a camera left streaming for a
/// recording that is not going to happen is a camera the next `open` finds busy.
///
/// # Errors
///
/// [`Error::IllegalTransition`] from the three request predicates.
/// [`Error::FormatUnsupported`] when the container the path names cannot carry what the
/// device negotiated — naming what that container *would* have taken, which is D7's
/// "non-MJPG `record` requests get Y4M or `FormatUnsupported { available }`" arriving at a
/// caller who typed `.avi` over a GREY camera. Otherwise whatever `start_stream` refused
/// with, unchanged: [`Error::Busy`] for a node somebody else is streaming (E3 keeps that
/// distinct from "the camera cannot"), and the device's own answer for everything else.
pub fn start(device: OpenCamera<'_>, request: &RecordRequest) -> Result<Opened> {
    let container = request.container()?;
    let budget_ms = request.budget_ms()?;

    let negotiated = device.start_stream(&request.stream)?;
    let format = match VideoFormat::resolve(container, negotiated.pixel_format) {
        Ok(format) => format,
        Err(refused) => {
            // Stopped before refusing, and the stop's own failure is discarded for
            // `crate::capture::StreamGuard`'s reason: the caller is already holding the
            // error that matters and a second one would replace it.
            let _ = device.stop_stream();
            return Err(refused);
        }
    };

    let params = RecordingParams {
        width: negotiated.width,
        height: negotiated.height,
        pixel_format: negotiated.pixel_format,
        // `imaging::avi::interval_micros` refuses to invent a number for a stepwise range or
        // for a `v4l2_frmivaltypes` value this build cannot read (AGENTS rule 6). A `None`
        // reaching the muxer is honestly reported as `IntervalSource::Provisional` rather
        // than rounded to 30 fps, which is why this is a forward rather than an `unwrap_or`.
        negotiated_interval_us: imaging::avi::interval_micros(&negotiated.interval),
        caps: caps(),
    };
    Ok(Opened {
        camera: device.info().id.clone(),
        negotiated,
        format,
        params,
        budget_ms,
    })
}

/// What one turn of a recording's device loop produced.
#[derive(Debug)]
pub enum Turn {
    /// A frame came off the stream. Hand it to a [`Recording`] — or, at P6c, to a recording
    /// *and* to whoever else is watching.
    Frame(Frame),
    /// [`limits::FRAME_DEADLINE_MS`] passed with no frame.
    ///
    /// **Not an error**, and the decision is `crate::preview::turn`'s, taken here for the
    /// same reason: a device that missed one deadline is slow, and a device that has stopped
    /// delivering is one that misses [`limits::RECORDING_MAX_EMPTY_TURNS`] of them in a row.
    /// Which of the two this is cannot be known from one turn, so this module reports the
    /// turn and the loop counts. Converting the first into the second is what AGENTS rule 7
    /// forbids.
    Idle,
}

/// Take one frame off a running recording's stream.
///
/// The whole of one command's device work, and short on purpose — see this module's header.
///
/// The deadline is [`limits::FRAME_DEADLINE_MS`] from *now*, which is the general "how long a
/// single `DQBUF` may block before the deadline logic gets a turn" bound rather than
/// [`limits::PREVIEW_FRAME_WAIT_MS`]: that one is tightened so a slider feels attached to the
/// camera, and what waits behind a recording's turn is a `record_stop` whose answer is one
/// turn away either way. It is not the `crate::settle::Clock` seam and does not want to be,
/// for the reason `crate::preview::turn` states one file along: the seam exists so a *policy*
/// can be driven without waiting, and there is no policy here — there is one blocking call
/// whose argument is the instant the kernel should give up at.
///
/// # Errors
///
/// Whatever the device refused with, unchanged and unclassified — [`Error::DeviceGone`] for a
/// camera unplugged mid-take, [`Error::DeviceIo`] for a `DQBUF` that failed. A frame that
/// simply did not arrive is [`Turn::Idle`].
pub fn turn(device: OpenCamera<'_>) -> Result<Turn> {
    let deadline = Instant::now() + Duration::from_millis(limits::FRAME_DEADLINE_MS);
    match device.next_frame(deadline) {
        Ok(frame) => Ok(Turn::Frame(frame)),
        Err(Error::SettleTimeout { .. }) => Ok(Turn::Idle),
        Err(err) => Err(err),
    }
}

/// Stop the recording's stream.
///
/// A named function over one forwarded call, for `crate::preview::stop`'s reason: "the
/// recording stops the stream" should be a thing the code *says* at the one place a reader
/// looks for it. The caller is whoever owns the loop, on every path out of it.
///
/// # Errors
///
/// Whatever `stop_stream` refused with. `VIDIOC_STREAMOFF` on a node that is not streaming is
/// not an error, so a duplicate stop is safe.
pub fn stop(device: OpenCamera<'_>) -> Result<()> {
    device.stop_stream()
}

// ------------------------------------------------------------------ the file seam

/// A sink a muxer can write and rewind.
///
/// `Seek` because `imaging::avi::write::AviWriter::finish` seeks back to rewrite six header
/// fields at close; `Send` because the daemon moves a recording between one camera actor's
/// commands. A blanket implementation, so `std::fs::File` is one without saying so.
pub trait WriteSeek: Write + Seek + Send {}

impl<T: Write + Seek + Send> WriteSeek for T {}

/// How a recording's file gets made.
///
/// A seam for note **N51**'s reason, one verb along from the photo it was written about.
/// `webcam-handler-cli` resolves `-o` on a command line somebody typed and opens it here;
/// the daemon cannot, because an `open(2)` that blocks — a fifo, an automounted path — runs
/// inside a camera's actor thread and would park that camera for the life of the process. So
/// the daemon resolves the destination to a **descriptor** before it touches a camera and
/// hands in a [`Files`] that returns the descriptor it already has, exactly as
/// `crate::photo::write_to_open_file` lets it do for a photo.
///
/// It is also the seam a full disk is scripted at ([`ScriptedFiles`], [`SinkFault`]), which
/// is the second thing design §2.9 asks of every seam: a real implementation and a scriptable
/// double with an exhaustive-match fault menu.
///
/// `Send` because the daemon moves one into a camera actor's closure; deliberately not
/// `Sync`, because a destination belongs to one recording and sharing one between two is a
/// question nothing here needs to answer.
pub trait Files: std::fmt::Debug + Send {
    /// Make (or truncate) the file at `path` and hand back something to record into.
    ///
    /// # Errors
    ///
    /// [`Error::StorageIo`] naming the path, when the file could not be made.
    fn create(&mut self, path: &Utf8Path) -> Result<Box<dyn WriteSeek>>;
}

/// Create the file when the recording starts, and truncate whatever was there.
///
/// What `webcam-handler-cli` hands in. `std::fs::File::create`'s semantics exactly, and not
/// `store::write_json_atomic`'s: that is the session store's protocol for documents the tool
/// re-reads and must never find half-written (D9), and it would be exactly wrong here — the
/// point of D7's recoverable `movi` prefix is that an *interrupted* recording leaves a file a
/// reader can still use, and a temp-file-plus-rename leaves nothing at the name the caller
/// asked for.
///
/// Truncating at the start rather than at the end is safe here in a way it is not for a photo
/// (`crate::photo::write_to_open_file` argues the other ordering): a recording's bytes do not
/// exist until it is over, so there is nothing to hold back, and everything that could refuse
/// the request has already refused it by the time [`Recording::begin`] reaches this.
#[derive(Debug, Default, Clone, Copy)]
pub struct OnDisk;

impl Files for OnDisk {
    fn create(&mut self, path: &Utf8Path) -> Result<Box<dyn WriteSeek>> {
        std::fs::File::create(path.as_std_path())
            .map(|file| Box::new(file) as Box<dyn WriteSeek>)
            .map_err(|err| storage_io(path, &err))
    }
}

// ------------------------------------------------------------------ the sink half

/// A recording in progress: the muxer, the file under it, and what the file said.
///
/// **It never touches a device.** Everything here takes a [`Frame`] the caller already has,
/// which is what lets P6c fan one frame out to a preview's viewers and to this at the same
/// time (see the module header).
#[derive(Debug)]
pub struct Recording {
    recorder: Recorder<Tap>,
    /// The kernel's own words about a refused write, shared with the [`Tap`] that saw it.
    ///
    /// The muxer answers a sink failure with [`Error::DeviceIo`] naming its own step, because
    /// from inside a container that is all there is to say. This engine knows more — a path,
    /// and the `errno` the kernel gave — so it upgrades that answer to [`Error::StorageIo`],
    /// and this is where the extra facts are kept. `Arc<Mutex<_>>` rather than a field on the
    /// sink because `Recorder` owns the sink and hands back no accessor to it, and
    /// `Recorder::finish` consumes it.
    failure: Arc<Mutex<Option<SinkFailure>>>,
    path: Utf8PathBuf,
    camera: CameraId,
    format: VideoFormat,
    negotiated: NegotiatedStream,
    started_at: Stamp,
    /// The engine's clock reading when the header was written.
    started_ms: Millis,
    /// How long the take may run on that clock.
    budget_ms: u64,
}

impl Recording {
    /// Create the file and write the container's header.
    ///
    /// The header is complete before the first frame in **both** containers, which is what
    /// makes an interrupted recording recoverable at all (D7 L0's documented promise, and
    /// `imaging::y4m`'s for free) — so a take that dies one frame in leaves a file rather
    /// than a fragment.
    ///
    /// `clock` and `now` are both arguments because the engine reads no clock: `now` is the
    /// wall time the report is stamped with, and `clock` is the monotonic one the duration
    /// runs on. They are different things and conflating them is how an NTP step becomes a
    /// duration (`crate::photo::take` takes the same pair for the same reason).
    ///
    /// # Errors
    ///
    /// [`Error::StorageIo`] naming `path` when the file cannot be made or the header cannot
    /// be written. [`Error::FormatUnsupported`] when the container cannot carry
    /// [`Opened::params`]'s pixel format — asked a second time by the muxer, one layer below
    /// [`start`]'s, and it is one rule asked twice rather than two rules. Otherwise whatever
    /// the container refuses at `begin`: a degenerate frame extent, an odd extent on a
    /// subsampled axis, a `max_bytes` the container cannot address.
    pub fn begin(
        opened: &Opened,
        path: &Utf8Path,
        files: &mut dyn Files,
        clock: &dyn Clock,
        now: Stamp,
    ) -> Result<Recording> {
        let failure: Arc<Mutex<Option<SinkFailure>>> = Arc::new(Mutex::new(None));
        let sink = files.create(path)?;
        let tap = Tap {
            inner: sink,
            failure: Arc::clone(&failure),
        };
        let recorder = match Recorder::begin(opened.format, tap, opened.params) {
            Ok(recorder) => recorder,
            Err(refused) => return Err(as_storage_io(&failure, path, refused)),
        };
        Ok(Recording {
            recorder,
            failure,
            path: path.to_owned(),
            camera: opened.camera.clone(),
            format: opened.format,
            negotiated: opened.negotiated.clone(),
            started_at: now,
            started_ms: clock.now_ms(),
            budget_ms: opened.budget_ms,
        })
    }

    /// Append one frame.
    ///
    /// A cap is **not** an error here either: `imaging::video::FrameOutcome::Refused` is the
    /// answer, and the refusal is sticky — a later, smaller frame is refused too, because a
    /// recording with a hole in the middle answers "how long did this take" wrongly. The
    /// caller's obligation is to stop asking the device, which is what
    /// `a_recording_stops_asking_the_device_the_moment_a_cap_refuses_a_frame` holds.
    ///
    /// # Errors
    ///
    /// [`Error::StorageIo`] naming the path when the file refused the write — carrying the
    /// kernel's own `errno`, which the muxer never saw. Otherwise the muxer's own
    /// [`Error::DeviceIo`] for a frame this open sink cannot carry: a pixel format that is not
    /// the stream's, a geometry that is not the stream's, a payload whose length disagrees
    /// with that geometry. The two are kept apart because they are different findings — one
    /// is a disk and one is a driver — and AGENTS rule 7 is the line between them.
    pub fn write(&mut self, frame: &Frame) -> Result<FrameOutcome> {
        match self.recorder.write_frame(frame) {
            Ok(outcome) => Ok(outcome),
            Err(refused) => Err(as_storage_io(&self.failure, &self.path, refused)),
        }
    }

    /// Close the container and report what the take turned out to be.
    ///
    /// `ended` is the caller's, because only the caller knows: [`drive`] answers from its own
    /// bounds, and a daemon driving turns answers `record_stop` with
    /// [`RecordingEnd::Stopped`]. It is a parameter rather than a field for that reason —
    /// a recording that decided its own ending would be answering a question its caller had
    /// already answered.
    ///
    /// # Errors
    ///
    /// [`Error::StorageIo`] naming the path when the file refused the index, a seek or the
    /// flush, or when it had already refused something earlier — in which case **the file is
    /// still a recoverable prefix** and the module header says which reader gets it.
    pub fn finish(self, ended: RecordingEnd, clock: &dyn Clock) -> Result<RecordReport> {
        let wall_clock_ms = clock.now_ms().saturating_sub(self.started_ms);
        let Recording {
            recorder,
            failure,
            path,
            camera,
            format,
            negotiated,
            started_at,
            ..
        } = self;
        let summary = match recorder.finish() {
            Ok(summary) => summary,
            Err(refused) => return Err(as_storage_io(&failure, &path, refused)),
        };
        Ok(RecordReport {
            camera,
            started_at,
            path,
            format,
            negotiated,
            summary,
            wall_clock_ms,
            ended,
        })
    }
}

// ------------------------------------------------------------------ the loop

/// Record until a bound says stop, one turn at a time.
///
/// **The loop for a caller that has no actor**, which is `webcam-handler-cli`: it holds the
/// device for the whole take, so `webcam-handler-daemon` must drive [`turn`] from its own
/// command loop instead (this module's header argues why, and note **N111** records it).
///
/// Three ways out, and each is a [`RecordingEnd`] rather than an error, because all three
/// describe a recording that happened:
///
/// - the caller's duration is spent, checked **before** each turn on `clock`, so a budget of
///   zero records a header and no frames rather than one frame;
/// - a cap refused a frame, at which point the loop stops asking the device — a recording
///   that went on streaming for the ninety seconds after its size cap would be a bug the
///   summary could not show;
/// - [`limits::RECORDING_MAX_EMPTY_TURNS`] turns in a row brought nothing.
///
/// # Errors
///
/// The device's, unchanged, from [`turn`]; and the file's, from [`Recording::write`]. A
/// caller holding one of those still holds a recording it must [`Recording::finish`] —
/// [`run`] is what does that on every path, and the reason it is written once there.
pub fn drive(
    device: OpenCamera<'_>,
    recording: &mut Recording,
    clock: &dyn Clock,
) -> Result<RecordingEnd> {
    let mut idle: u32 = 0;
    loop {
        if clock.now_ms().saturating_sub(recording.started_ms) >= recording.budget_ms {
            return Ok(RecordingEnd::Duration);
        }
        match turn(&mut *device)? {
            Turn::Frame(frame) => {
                idle = 0;
                match recording.write(&frame)? {
                    FrameOutcome::Written => {}
                    FrameOutcome::Refused(_) => return Ok(RecordingEnd::Cap),
                }
            }
            Turn::Idle => {
                idle = idle.saturating_add(1);
                if idle >= limits::RECORDING_MAX_EMPTY_TURNS {
                    return Ok(RecordingEnd::DeviceQuiet);
                }
            }
        }
    }
}

/// Record one file, start to finish, on a camera this caller owns (design D7, D10).
///
/// [`start`], [`Recording::begin`], [`drive`], [`Recording::finish`] — and the stream stopped
/// on every path out, including the panicking one, which is why the stop is a drop guard
/// rather than a line at the end (`crate::capture::StreamGuard`'s argument: the most popular
/// V4L2 crate panics on a control type this kernel emits \[PF:1\], so "the work panicked" is a
/// measured failure mode, and a camera left streaming is a camera the next `open` finds
/// busy).
///
/// **The container is closed even when the take failed**, and that ordering is docs/7 P6b's
/// "every fault leaves a parseable file". A device that vanished mid-recording gets its
/// `idx1` written and then the caller gets [`Error::DeviceGone`]; the report the close
/// produced is discarded, because a report answering a request that failed would be this
/// function deciding a `DeviceGone` was a successful recording, which is the conversion
/// AGENTS rule 7 forbids. What the caller has instead is the path it named and a file both
/// readers can open.
///
/// # Errors
///
/// [`Error::IllegalTransition`] from the request's own three predicates;
/// [`Error::FormatUnsupported`] when the container the path names cannot carry what the
/// camera negotiated; [`Error::StorageIo`] naming the path for anything the file refused; and
/// the device's own answer, unchanged, for anything it refused.
pub fn run(
    camera: OpenCamera<'_>,
    request: &RecordRequest,
    files: &mut dyn Files,
    clock: &dyn Clock,
    now: Stamp,
) -> Result<RecordReport> {
    let path = request.server_path()?.to_owned();
    let opened = start(&mut *camera, request)?;
    let streaming = StreamGuard { camera };
    let mut recording = Recording::begin(&opened, &path, files, clock, now)?;
    match drive(&mut *streaming.camera, &mut recording, clock) {
        Ok(ended) => recording.finish(ended, clock),
        Err(refused) => {
            // The close first, so the file the caller was promised is a file; then the
            // device's own words. Its answer is discarded rather than merged: this take
            // failed, and a report is what a take that did not fail produces.
            let _ = recording.finish(RecordingEnd::DeviceFailed, clock);
            Err(refused)
        }
    }
}

/// Stops the stream when it goes out of scope, however it goes out of scope.
struct StreamGuard<'device> {
    camera: OpenCamera<'device>,
}

impl Drop for StreamGuard<'_> {
    fn drop(&mut self) {
        // A stop that fails has nothing left to tell anyone: the caller is already holding
        // either a report or the error that matters, and a second error would replace the
        // useful one. Dropped deliberately, like `crate::capture::StreamGuard`'s.
        let _ = self.camera.stop_stream();
    }
}

// ------------------------------------------------------------------ the sink's own words

/// What the kernel said when it refused a write, a seek or a flush.
#[derive(Debug, Clone)]
struct SinkFailure {
    errno: Option<i32>,
    message: String,
}

/// The sink, plus the first thing it refused.
///
/// A pass-through wrapper and nothing else. It exists because two layers know half the answer
/// each: `imaging` sees the failure and answers [`Error::DeviceIo`] naming the muxer step,
/// because a container has no business knowing what a path is; this engine knows the path and
/// wants the `errno`, because [`Error::StorageIo`] is D13's disk-shaped variant and an
/// unattended caller reading `ENOSPC` beside a path can act on it. Neither layer can produce
/// that answer alone, so the fact is captured here on its way past.
///
/// **The first error, not the last.** A poisoned muxer refuses every later call without
/// touching the sink, so in practice there is only one; keeping the first is what makes that
/// true rather than incidental, and a later failure caused by the first one would be a
/// diagnosis of a consequence.
struct Tap {
    inner: Box<dyn WriteSeek>,
    failure: Arc<Mutex<Option<SinkFailure>>>,
}

impl Tap {
    /// Remember `outcome`'s error, if it has one and nothing has failed yet.
    fn note<T>(&mut self, outcome: std::io::Result<T>) -> std::io::Result<T> {
        if let Err(err) = &outcome {
            let mut slot = lock(&self.failure);
            if slot.is_none() {
                *slot = Some(SinkFailure {
                    errno: err.raw_os_error(),
                    message: err.to_string(),
                });
            }
        }
        outcome
    }
}

impl Write for Tap {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let outcome = self.inner.write(buf);
        self.note(outcome)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        let outcome = self.inner.flush();
        self.note(outcome)
    }
}

impl Seek for Tap {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        let outcome = self.inner.seek(pos);
        self.note(outcome)
    }
}

/// Upgrade a muxer's refusal to a storage failure when the *sink* is what refused.
///
/// The muxer's own [`Error::DeviceIo`] travels unchanged when nothing was wrong with the
/// file, because then the finding really is about a frame — a geometry that is not the
/// stream's, a payload whose length disagrees with it — and reporting a driver's defect as a
/// disk failure would be exactly the collapse AGENTS rule 7 forbids in the other direction.
fn as_storage_io(failure: &Mutex<Option<SinkFailure>>, path: &Utf8Path, refused: Error) -> Error {
    match lock(failure).as_ref() {
        Some(sink) => Error::StorageIo {
            path: path.to_owned(),
            errno: sink.errno,
            message: sink.message.clone(),
        },
        None => refused,
    }
}

/// A filesystem failure, with the real `errno` where the kernel supplied one.
fn storage_io(path: &Utf8Path, err: &std::io::Error) -> Error {
    Error::StorageIo {
        path: path.to_owned(),
        errno: err.raw_os_error(),
        message: err.to_string(),
    }
}

/// A poisoned mutex here means a panic while one `Option` was being written, and the value
/// behind it is a diagnosis rather than state anything depends on — so the recording carries
/// on with what it has, exactly as `crate::store`'s lock helpers do.
fn lock(failure: &Mutex<Option<SinkFailure>>) -> std::sync::MutexGuard<'_, Option<SinkFailure>> {
    failure.lock().unwrap_or_else(PoisonError::into_inner)
}

// ------------------------------------------------------------------ the fault menu

closed_vocabulary! {
    /// A fault the [`Files`] seam can be told to exhibit (design §2.9's shape, at the seam
    /// docs/7 P6b adds).
    ///
    /// `ALL` is generated from this definition and the tests walk it, because a fault the
    /// compiler cannot force somebody to answer is a fault nobody answers — `store::StoreFault`
    /// and `fake::Fault` are the two menus this one joins.
    ///
    /// The four are not four sizes of one failure: each reaches a **different path** through
    /// the muxer, and the file each leaves behind is read by a different reader.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub enum SinkFault {
        /// The file cannot be made at all: `create` refuses with `ENOSPC`.
        ///
        /// The one arm that leaves **no file**, which is the honest outcome — nothing was
        /// promised — and is why the walk over this menu has a per-variant arm rather than
        /// one shared assertion.
        CreateRefused,
        /// Every write past [`ScriptedFiles::with_budget`] bytes refuses with `ENOSPC`.
        ///
        /// The disk that fills **mid-recording**, which is docs/7 P6b's named case: the
        /// muxer is poisoned where it stands, so the file has no `idx1` and no patched
        /// header, and what can still read it is `imaging::avi::read::recover_frames` and not
        /// `read_stream`. A budget of zero is the same fault at the header, which is the
        /// disk that was already full.
        WritesRefused,
        /// Every seek refuses with `ESPIPE`: the sink cannot be rewound.
        ///
        /// A fifo, a pipe, a socket — a real thing to name as a recording's destination, and
        /// the one the AVI container cannot use, because `AviWriter::finish` rewrites six
        /// header fields in place. It is refused at [`Recording::begin`], before a byte of
        /// header is written and therefore **before a frame of whoever is in front of the
        /// lens has been captured**, because the muxer asks the sink where it is before it
        /// asks it to hold anything. That makes it the arm whose file is *empty* rather than
        /// absent, partial or whole — four faults, four states, which is what stops this walk
        /// from asserting one thing four times.
        SeekRefused,
        /// The close-time flush refuses with `EIO`.
        ///
        /// The last thing `finish` does, after every byte and every patch. It is the arm
        /// where the file on disk is **whole** and the answer is still a failure, because a
        /// flush that refused is a write this process cannot say landed — reporting success
        /// there would be a claim about somebody else's disk.
        FlushRefused,
    }
}

impl SinkFault {
    /// The fault's name, for reports and test failure messages.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            SinkFault::CreateRefused => "create_refused",
            SinkFault::WritesRefused => "writes_refused",
            SinkFault::SeekRefused => "seek_refused",
            SinkFault::FlushRefused => "flush_refused",
        }
    }
}

impl std::fmt::Display for SinkFault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// `ENOSPC`, the number.
///
/// Named here rather than pulled from `libc` (which this crate does not depend on) and
/// validated against the kernel by `the_disk_full_errno_is_the_kernels_own`, which asks
/// `/dev/full` what a full filesystem answers — `crate::store`'s constant carries the same
/// name for the same reason, and note **N10** is why a number the author believes is not
/// enough.
const ENOSPC: i32 = 28;

/// `ESPIPE`: the seek a pipe cannot do.
const ESPIPE: i32 = 29;

/// `EIO`: the write that reached the device and did not land.
const EIO: i32 = 5;

/// A [`Files`] that makes real files and then breaks them on cue (design §2.9).
///
/// **It writes to the real filesystem**, and that is the whole point: docs/7 P6b's criterion
/// is that "every fault leaves a parseable file", so the bytes a fault leaves behind have to
/// be bytes a reader can actually be handed. A double that returned an in-memory buffer would
/// let the claim be asserted against something no disk ever held.
///
/// Public for `crate::store::TempStore`'s reason: the daemon's own recording tests at docs/7
/// P6c need the same fault arranged, and a second copy of a fault menu is a second fault menu
/// (design §2.10).
#[derive(Debug)]
pub struct ScriptedFiles {
    fault: SinkFault,
    /// How many bytes [`SinkFault::WritesRefused`] lets through before it starts refusing.
    budget: u64,
}

impl ScriptedFiles {
    /// A [`Files`] that exhibits `fault`.
    ///
    /// [`SinkFault::WritesRefused`] refuses from the very first byte until
    /// [`ScriptedFiles::with_budget`] says otherwise: a disk that was already full is the
    /// simpler of the two conditions, so it is the one that needs no arranging.
    #[must_use]
    pub fn new(fault: SinkFault) -> ScriptedFiles {
        ScriptedFiles { fault, budget: 0 }
    }

    /// How many bytes reach the file before [`SinkFault::WritesRefused`] starts refusing.
    #[must_use]
    pub fn with_budget(mut self, bytes: u64) -> ScriptedFiles {
        self.budget = bytes;
        self
    }
}

impl Files for ScriptedFiles {
    fn create(&mut self, path: &Utf8Path) -> Result<Box<dyn WriteSeek>> {
        if self.fault == SinkFault::CreateRefused {
            return Err(storage_io(path, &std::io::Error::from_raw_os_error(ENOSPC)));
        }
        let file =
            std::fs::File::create(path.as_std_path()).map_err(|err| storage_io(path, &err))?;
        Ok(Box::new(Faulty {
            inner: file,
            fault: self.fault,
            budget: self.budget,
            written: 0,
        }))
    }
}

/// One scripted file: a real one, until the fault fires.
///
/// `write` short-writes down to the budget before it refuses, which is what a filesystem
/// running out of space actually does — the last chunk lands partially and the next call
/// answers `ENOSPC`. That matters for what this double is for: a recording cut in the middle
/// of a frame is the case `recover_frames` exists to survive, and a double that refused
/// cleanly on a chunk boundary would never produce one.
#[derive(Debug)]
struct Faulty {
    inner: std::fs::File,
    fault: SinkFault,
    budget: u64,
    written: u64,
}

impl Write for Faulty {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if self.fault != SinkFault::WritesRefused {
            return self.inner.write(buf);
        }
        let room = self.budget.saturating_sub(self.written);
        if room == 0 {
            return Err(std::io::Error::from_raw_os_error(ENOSPC));
        }
        let take = usize::try_from(room).unwrap_or(usize::MAX).min(buf.len());
        let wrote = self.inner.write(buf.get(..take).unwrap_or(buf))?;
        self.written = self
            .written
            .saturating_add(u64::try_from(wrote).unwrap_or(u64::MAX));
        Ok(wrote)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        if self.fault == SinkFault::FlushRefused {
            return Err(std::io::Error::from_raw_os_error(EIO));
        }
        self.inner.flush()
    }
}

impl Seek for Faulty {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        if self.fault == SinkFault::SeekRefused {
            return Err(std::io::Error::from_raw_os_error(ESPIPE));
        }
        self.inner.seek(pos)
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::sync::Arc;
    use std::time::Instant;

    use fake::{FakeBackend, Fault};
    use imaging::avi::read;
    use schema::ErrorKind;
    use schema::backend::{Camera, CameraBackend};
    use schema::camera::PixelFormat;
    use schema::capture::{PhotoFormat, Sink, StreamRequest};
    use schema::control::{Applied, ControlDesc, ControlId, ControlValue};
    use schema::video::CapReached;

    use super::*;
    use crate::double::{ScriptedCamera, integer};
    use crate::settle::FrozenClock;

    /// A directory that disappears with the test, and paths inside it.
    ///
    /// Every file this suite writes goes here, which is `crate::paths::scratch_dir` — inside
    /// `target/`, therefore gitignored and pruned by every gate that walks the tree, so
    /// nothing a recording test writes can land in the repository or be mistaken for corpus
    /// (rubric A12, note **N84**).
    struct Scratch {
        dir: tempfile::TempDir,
    }

    impl Scratch {
        fn new() -> Scratch {
            Scratch {
                dir: crate::paths::scratch_dir().expect("a scratch directory"),
            }
        }

        fn path(&self, name: &str) -> Utf8PathBuf {
            Utf8PathBuf::from_path_buf(self.dir.path().join(name)).expect("utf-8 temp dir")
        }
    }

    /// A clock whose steps the **loop** takes, because the test is not inside the loop.
    ///
    /// Note **N60** names two shapes: a `SteppedClock` where the deadline is the subject, and
    /// a [`FrozenClock`] where it is not. Both are used below, and neither can serve the two
    /// tests whose subject is [`drive`]'s own duration — a `SteppedClock` is moved by the
    /// test, and here the test hands the loop a clock and gets an answer back with nothing in
    /// between. The first design tried was a camera that advanced the test's clock as it
    /// delivered, which is faithful and does not compile: `crate::actor::OpenCamera` is
    /// `dyn Camera + 'static`, so a camera cannot hold a borrow of a fixture.
    ///
    /// So the clock steps itself, once per reading, and [`drive`] reads it **exactly once per
    /// turn** — a reading is a turn and `step` is what one turn costs. That makes the frame
    /// count arithmetic rather than a guess, which is what every assertion below relies on,
    /// and it keeps the rule that matters: no `sleep`, no wall clock, and a deadline the test
    /// owns (AGENTS: no sleep as synchronisation).
    ///
    /// It is deliberately **not** in `crate::settle` beside the other two. Those are the
    /// product's doubles, used by four modules' suites; this one is a fixture for one loop's
    /// two tests, and a third shape in the shared home would invite the question N60 already
    /// answered.
    #[derive(Debug)]
    struct TickingClock {
        now: Cell<Millis>,
        step: Millis,
    }

    impl TickingClock {
        fn new(step: Millis) -> TickingClock {
            TickingClock {
                now: Cell::new(0),
                step,
            }
        }
    }

    impl Clock for TickingClock {
        fn now_ms(&self) -> Millis {
            let reading = self.now.get();
            self.now.set(reading.saturating_add(self.step));
            reading
        }
    }

    /// The fake replaying a committed profile, and the backend kept so its counters can be
    /// read.
    ///
    /// `streams_started()` is the only thing that can tell "refused before the camera was
    /// touched" from "refused after a stream had already run", and a test named for the first
    /// must be able to see it — `crate::photo`'s suite learned that the same way.
    fn watched(profile: &str) -> (Arc<FakeBackend>, Box<dyn Camera>) {
        let profile = testkit::corpus::load(profile).expect("a committed profile");
        let backend = Arc::new(FakeBackend::from_profile(profile).expect("replays"));
        let id = backend
            .enumerate()
            .expect("enumerate")
            .into_iter()
            .next()
            .expect("one camera")
            .id;
        let camera = backend.open(&id).expect("opens");
        (backend, camera)
    }

    fn camera_from(profile: &str) -> Box<dyn Camera> {
        watched(profile).1
    }

    fn request(path: &Utf8Path, duration_ms: Option<u64>) -> RecordRequest {
        RecordRequest {
            stream: StreamRequest::default(),
            duration_ms,
            sink: Sink::ServerPath {
                path: path.to_owned(),
            },
        }
    }

    /// Bounds no test trips by accident, so every cap assertion below is deliberate.
    fn generous() -> RecordingCaps {
        RecordingCaps {
            max_bytes: 1 << 24,
            max_frames: 1_024,
            max_span: Duration::from_secs(600),
        }
    }

    /// A camera that counts how often it was asked for a frame.
    ///
    /// The only observable that can hold "the loop stopped asking the device": the muxer's
    /// refusal is sticky, so a build that kept streaming for the eighty-eight seconds after
    /// its size cap would write the identical file.
    struct Counting {
        inner: Box<dyn Camera>,
        asked: u32,
        stopped: u32,
    }

    impl std::fmt::Debug for Counting {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("Counting")
                .field("asked", &self.asked)
                .field("stopped", &self.stopped)
                .finish_non_exhaustive()
        }
    }

    impl Counting {
        fn new(inner: Box<dyn Camera>) -> Counting {
            Counting {
                inner,
                asked: 0,
                stopped: 0,
            }
        }
    }

    impl Camera for Counting {
        fn info(&self) -> &schema::camera::CameraInfo {
            self.inner.info()
        }

        fn formats(&self) -> Result<Vec<schema::camera::FormatInfo>> {
            self.inner.formats()
        }

        fn controls(&self) -> Result<Vec<ControlDesc>> {
            self.inner.controls()
        }

        fn get(&mut self, id: ControlId) -> Result<ControlValue> {
            self.inner.get(id)
        }

        fn set(&mut self, id: ControlId, value: ControlValue) -> Result<Applied> {
            self.inner.set(id, value)
        }

        fn start_stream(&mut self, request: &StreamRequest) -> Result<NegotiatedStream> {
            self.inner.start_stream(request)
        }

        fn streaming(&self) -> Option<NegotiatedStream> {
            self.inner.streaming()
        }

        fn next_frame(&mut self, deadline: Instant) -> Result<Frame> {
            self.asked = self.asked.saturating_add(1);
            self.inner.next_frame(deadline)
        }

        fn stop_stream(&mut self) -> Result<()> {
            self.stopped = self.stopped.saturating_add(1);
            self.inner.stop_stream()
        }
    }

    /// A JPEG frame of the size [`ScriptedCamera`] negotiates.
    ///
    /// `crate::double::frame` yields **YUYV** and `ScriptedCamera::compressed` changes the
    /// *negotiation* rather than the scripted frames, so a test that needs the AVI container
    /// has to build its own — `fake::frames` synthesises its bitstream for the same reason one
    /// crate along.
    fn compressed_frame(sequence: u32) -> Frame {
        let bytes = imaging::encode::jpeg(
            &imaging::decode::Decoded::Gray(imaging::fixtures::gradient(32, 16)),
            40,
        )
        .expect("encodes");
        Frame {
            bytes,
            pixel_format: PixelFormat::MJPG,
            width: 32,
            height: 16,
            bytes_per_line: 0,
            sequence,
            timestamp_us: i64::from(sequence) * 33_333,
        }
    }

    /// Record over an already-open camera with caps of the test's choosing.
    ///
    /// The shipped bounds are backstops in gibibytes and thousands of frames ([`caps`]), so a
    /// suite that could only drive those could not reach a cap at all — see [`Opened`]'s own
    /// note on why its fields are public.
    fn record_with(
        camera: OpenCamera<'_>,
        path: &Utf8Path,
        caps_override: RecordingCaps,
        clock: &dyn Clock,
    ) -> Result<RecordReport> {
        let asking = request(path, Some(limits::MAX_RECORDING_MS));
        let mut opened = start(&mut *camera, &asking)?;
        opened.params.caps = caps_override;
        let mut recording =
            Recording::begin(&opened, path, &mut OnDisk, clock, Stamp::epoch()).expect("begins");
        let ended = drive(&mut *camera, &mut recording, clock)?;
        stop(&mut *camera).expect("stops");
        recording.finish(ended, clock)
    }

    #[test]
    fn a_recording_asked_to_come_back_as_bytes_never_reaches_the_camera() {
        // Note **N110**: this verb takes one of D10's two sink variants. The refusal has to
        // arrive before a `STREAMON`, because a frame of whoever is in front of the lens
        // captured for a request nobody was going to honour is the defect debt D-1 named one
        // verb along — and `streams_started()` is what makes the name of this test checkable,
        // since `!path.exists()` alone is also true of a build that streamed, captured a
        // frame and only then refused.
        let scratch = Scratch::new();
        let (backend, mut camera) = watched("chicony-rgb");
        let asking = RecordRequest {
            stream: StreamRequest::default(),
            duration_ms: None,
            sink: Sink::ReturnBytes {
                format: PhotoFormat::Jpeg,
            },
        };
        let refusal = run(
            camera.as_mut(),
            &asking,
            &mut OnDisk,
            &FrozenClock,
            Stamp::epoch(),
        )
        .expect_err("a recording does not come back as bytes");
        assert_eq!(refusal.kind(), ErrorKind::IllegalTransition);
        assert_eq!(backend.streams_started(), 0, "the refusal cost a stream");

        // The other direction, so the arm above refuses a *sink shape* rather than refusing
        // recordings: the same camera, asked for a path.
        let path = scratch.path("take.avi");
        run(
            camera.as_mut(),
            &request(&path, Some(0)),
            &mut OnDisk,
            &FrozenClock,
            Stamp::epoch(),
        )
        .expect("a path sink is served");
        assert_eq!(backend.streams_started(), 1);
        assert!(path.exists(), "a served recording wrote no file");
    }

    #[test]
    fn a_request_this_build_will_not_honour_is_refused_before_the_stream_starts() {
        // The other two request predicates, at the layer that has a camera to spend. Both are
        // `IllegalTransition` (note **N46**), both arrive with `streams_started() == 0`, and
        // both leave no file — which is the second half of the claim, because a build that
        // created the file before it refused would have truncated an operator's existing
        // recording on the way to saying no.
        let scratch = Scratch::new();
        let (backend, mut camera) = watched("chicony-rgb");

        let unwritable = scratch.path("take.mkv");
        let refusal = run(
            camera.as_mut(),
            &request(&unwritable, None),
            &mut OnDisk,
            &FrozenClock,
            Stamp::epoch(),
        )
        .expect_err("this build writes no Matroska");
        assert_eq!(refusal.kind(), ErrorKind::IllegalTransition);
        assert!(!unwritable.exists(), "a refused recording wrote a file");

        let too_long = scratch.path("take.avi");
        let refusal = run(
            camera.as_mut(),
            &request(&too_long, Some(limits::MAX_RECORDING_MS + 1)),
            &mut OnDisk,
            &FrozenClock,
            Stamp::epoch(),
        )
        .expect_err("a millisecond past the cap is past it");
        assert_eq!(refusal.kind(), ErrorKind::IllegalTransition);
        assert!(!too_long.exists());
        assert_eq!(
            backend.streams_started(),
            0,
            "a request nobody was going to honour cost a stream"
        );
    }

    #[test]
    fn a_refused_recording_leaves_a_file_that_was_already_there_untouched() {
        // The ordering `Recording::begin` depends on: everything that can refuse the request
        // refuses it, the stream is negotiated, and only then is the file created. Truncating
        // first is the shape note **N51** was written about one verb along — there the fix was
        // to truncate after the bytes existed, and here it is to create only once nothing can
        // still say no.
        let scratch = Scratch::new();
        let path = scratch.path("previous.avi");
        std::fs::write(path.as_std_path(), b"an operator's earlier recording").expect("writes");

        // A GREY camera and a `.avi` sink: refused by the pairing, which is the *latest* of
        // the refusals — it needs the negotiation — so if any of them truncated, this one
        // would.
        let mut camera = camera_from("chicony-ir");
        let refusal = run(
            camera.as_mut(),
            &request(&path, Some(1_000)),
            &mut OnDisk,
            &FrozenClock,
            Stamp::epoch(),
        )
        .expect_err("AVI carries no GREY");
        assert_eq!(refusal.kind(), ErrorKind::FormatUnsupported);
        assert_eq!(
            std::fs::read(path.as_std_path()).expect("still there"),
            b"an operator's earlier recording",
            "a refused recording truncated a file it never wrote to"
        );
    }

    #[test]
    fn the_caps_a_recording_runs_under_are_the_three_the_limits_table_names() {
        // Rubric A8, as an execution: three constants whose own docs said "no product reader
        // yet" until this function existed. Asserted against the constants rather than
        // against literals, so a re-tuned bound moves this test with it — and the *span* cap
        // is deliberately `MAX_RECORDING_MS` and not the request's duration, because a take
        // bounded twice by the same number would report whichever of the two the driver's
        // clock happened to reach first.
        let bounds = caps();
        assert_eq!(bounds.max_bytes, limits::MAX_RECORDING_BYTES);
        assert_eq!(bounds.max_frames, limits::MAX_RECORDING_FRAMES);
        assert_eq!(
            bounds.max_span,
            Duration::from_millis(limits::MAX_RECORDING_MS)
        );

        // And the fourth constant reaching the executor: a request that named no duration is
        // `DEFAULT_RECORDING_MS` long.
        let scratch = Scratch::new();
        let path = scratch.path("take.avi");
        let mut camera = camera_from("chicony-rgb");
        let opened = start(camera.as_mut(), &request(&path, None)).expect("starts");
        assert_eq!(opened.budget_ms, limits::DEFAULT_RECORDING_MS);
        assert_eq!(opened.params.caps, bounds);
        assert_eq!(
            opened.format,
            VideoFormat::Avi,
            "an MJPG camera records AVI"
        );
        stop(camera.as_mut()).expect("stops");
    }

    #[test]
    fn every_cap_in_the_vocabulary_ends_the_recording_and_the_summary_names_it() {
        // The claim `imaging::video`'s own walk cannot make: that the **executor** passes the
        // bounds through and stops when it is told. The muxer's enforcement is tested where
        // the muxer is; what is untested there is a `RecordingCaps` this engine assembled, a
        // loop this engine wrote, and an ending this engine reports. Walked rather than
        // written three times, so a cap that stops being passed through goes red here.
        //
        // Every arm runs on a `FrozenClock`, which is note **N60**'s second shape doing
        // exactly what it is for: a deadline that cannot expire makes the *cap* the only
        // reachable ending, however loaded the machine running this.
        let scratch = Scratch::new();

        // The size a two-frame recording costs this camera in this container, measured rather
        // than predicted: it is a bound that admits exactly two frames and refuses the third,
        // whatever the fake's exposure ramp does to a JPEG's length.
        let mut camera = camera_from("chicony-rgb");
        let control = record_with(
            camera.as_mut(),
            &scratch.path("control.avi"),
            RecordingCaps {
                max_frames: 2,
                ..generous()
            },
            &FrozenClock,
        )
        .expect("a two-frame control recording");
        assert_eq!(control.summary.frames_written, 2);
        let interval = u64::from(control.summary.declared_interval_us);

        let mut checked = 0;
        for &cap in CapReached::ALL {
            let bounds = match cap {
                CapReached::Frames => RecordingCaps {
                    max_frames: 2,
                    ..generous()
                },
                // The span is measured from the first frame written, so a budget of one
                // interval admits frames 0 and 1 and refuses frame 2.
                CapReached::Span => RecordingCaps {
                    max_span: Duration::from_micros(interval),
                    ..generous()
                },
                CapReached::Size => RecordingCaps {
                    max_bytes: control.summary.bytes_written,
                    ..generous()
                },
            };
            let mut camera = camera_from("chicony-rgb");
            let report = record_with(
                camera.as_mut(),
                &scratch.path(&format!("{cap:?}.avi")),
                bounds,
                &FrozenClock,
            )
            .unwrap_or_else(|err| panic!("{cap:?}: {err}"));

            assert_eq!(report.ended, RecordingEnd::Cap, "{cap:?}");
            assert_eq!(
                report.summary.cap_reached,
                Some(cap),
                "{cap:?} was reached and the summary named something else"
            );
            assert_eq!(
                report.summary.frames_written, 2,
                "{cap:?} let the wrong number of frames through"
            );
            // The file a cap left behind is a *finished* one: a recording that stopped
            // because it was told to is not a recording that failed, so the close ran and the
            // strict reader accepts it.
            let bytes = std::fs::read(&report.path).expect("the file exists");
            let stream = read::read_stream(&bytes).expect("a capped recording is still closed");
            assert_eq!(stream.frames.len(), 2, "{cap:?}");
            checked += 1;
        }
        assert_eq!(checked, CapReached::ALL.len(), "the walk skipped a cap");
    }

    #[test]
    fn a_recording_stops_asking_the_device_the_moment_a_cap_refuses_a_frame() {
        // The bug the summary could not show: a take whose size cap was reached after two
        // seconds and which went on streaming for the eighty-eight after it, holding a camera
        // nobody else could open. The muxer's refusal is sticky, so the file would be
        // byte-identical either way; the only observable is how often the *device* was asked.
        let scratch = Scratch::new();
        let mut camera = Counting::new(camera_from("chicony-rgb"));
        let report = record_with(
            &mut camera,
            &scratch.path("capped.avi"),
            RecordingCaps {
                max_frames: 2,
                ..generous()
            },
            &FrozenClock,
        )
        .expect("records");

        assert_eq!(report.summary.frames_written, 2);
        assert_eq!(
            camera.asked, 3,
            "the loop asked the device for frames after the cap had stopped writing them"
        );
    }

    #[test]
    fn the_wall_clock_budget_ends_a_recording_whose_camera_is_still_delivering() {
        // The bound the caller actually asked for, on a clock the test owns. The fake
        // synthesises frames for ever, so the *only* thing that can end this is the budget —
        // and the arithmetic is exact rather than approximate because `drive` reads the clock
        // once per turn: `begin` takes the reading at 0, and turns see 40, 80, 120, 160 and
        // then 200, which is the first at or past the budget. Four frames, and a wall clock of
        // 240 once `finish` takes its own reading.
        let scratch = Scratch::new();
        let path = scratch.path("take.avi");
        let clock = TickingClock::new(40);
        let mut camera = camera_from("chicony-rgb");
        let report = run(
            camera.as_mut(),
            &request(&path, Some(200)),
            &mut OnDisk,
            &clock,
            Stamp::epoch(),
        )
        .expect("records");

        assert_eq!(report.ended, RecordingEnd::Duration);
        assert_eq!(
            report.summary.cap_reached, None,
            "a recording that ran its duration met no cap"
        );
        assert_eq!(report.summary.frames_written, 4);
        assert_eq!(report.wall_clock_ms, 240);
        assert!(
            report.wall_clock_ms >= 200,
            "the budget was declared spent early"
        );
        // The pair docs/7 P6d subtracts. The driver's own span is the frame timestamps and it
        // is *shorter* than the wall clock for an honest take, because the header write, the
        // close and every turn that brought no frame are outside it.
        let span_ms = report.summary.span_us.expect("four frames span something") / 1_000;
        assert!(
            span_ms <= report.wall_clock_ms,
            "a driver span of {span_ms} ms inside {} ms of wall clock",
            report.wall_clock_ms
        );
    }

    #[test]
    fn a_camera_that_stopped_delivering_ends_the_recording_rather_than_running_to_its_deadline() {
        // The bound that covers what the other two cannot: a device answering promptly and
        // delivering nothing, on a clock that is not advancing. Without
        // `RECORDING_MAX_EMPTY_TURNS` this test does not fail — it **hangs**, which is the
        // strongest form the inverse can take here (`crate::capture`'s `MAX_SETTLE_ROUNDS`
        // test says the same about the settle loop).
        //
        // `ScriptedCamera` runs out of scripted frames, which is how it says "nothing right
        // now" with no wall-clock wait at all (note N3), and `FrozenClock` is what makes the
        // deadline unreachable — so the only way out is the empty-turn bound.
        let scratch = Scratch::new();
        let path = scratch.path("short.y4m");
        let mut camera = Counting::new(Box::new(
            ScriptedCamera::new(vec![integer("brightness", 50)])
                .with_frames((0..3).map(crate::double::frame).collect()),
        ));

        let report = run(
            &mut camera,
            &request(&path, Some(limits::MAX_RECORDING_MS)),
            &mut OnDisk,
            &FrozenClock,
            Stamp::epoch(),
        )
        .expect("three frames is a recording");

        assert_eq!(report.ended, RecordingEnd::DeviceQuiet);
        assert_eq!(report.summary.frames_written, 3);
        assert_eq!(
            report.format,
            VideoFormat::Y4m,
            "a YUYV camera records raw planes (D7)"
        );
        // The bound is *this* bound and not merely "some bound": three frames, then exactly
        // `RECORDING_MAX_EMPTY_TURNS` turns that brought nothing. Without this the constant
        // could be any positive number and no test would notice — which is rubric A8's
        // complaint about a constant nobody reads, one step subtler.
        assert_eq!(
            camera.asked,
            3 + limits::RECORDING_MAX_EMPTY_TURNS,
            "the silence budget is not the one the limits table names"
        );
        assert_eq!(camera.stopped, 1, "the stream was left running");

        // The frames that did arrive are in the file, which is the whole point of calling this
        // an *ending* rather than an error (AGENTS rule 7).
        let bytes = std::fs::read(path.as_std_path()).expect("the file exists");
        assert!(bytes.starts_with(b"YUV4MPEG2 "), "not a Y4M file");
        assert_eq!(
            bytes
                .windows(6)
                .filter(|window| *window == b"FRAME\n")
                .count(),
            3,
            "the frames that arrived did not reach the file"
        );
    }

    #[test]
    fn a_frame_that_did_not_arrive_is_an_idle_turn_and_a_device_that_went_away_is_not() {
        // AGENTS rule 7 at the one place this module could break it, and
        // `crate::preview::turn`'s decision taken again for the recording loop: the two are
        // answered by different code paths, and flattening them would spend
        // `RECORDING_MAX_EMPTY_TURNS` pretending an unplugged camera was a slow one.
        let mut quiet = ScriptedCamera::new(vec![integer("brightness", 50)])
            .with_frames(vec![crate::double::frame(0)]);
        quiet
            .start_stream(&StreamRequest::default())
            .expect("streams");
        assert!(matches!(
            turn(&mut quiet).expect("one frame is scripted"),
            Turn::Frame(_)
        ));
        assert!(matches!(
            turn(&mut quiet).expect("an empty turn is not an error"),
            Turn::Idle
        ));

        let mut gone = ScriptedCamera::new(vec![integer("brightness", 50)]).frames_refused(
            Error::DeviceGone {
                path: Utf8PathBuf::from("/dev/video0"),
            },
        );
        gone.start_stream(&StreamRequest::default())
            .expect("streams");
        assert_eq!(
            turn(&mut gone).expect_err("the device is gone").kind(),
            ErrorKind::DeviceGone
        );
    }

    #[test]
    fn every_sink_fault_is_a_storage_failure_naming_the_path_and_leaves_what_it_can() {
        // docs/7 P6b's "every fault leaves a parseable file", walked over the menu rather than
        // written once per fault — a fault the compiler cannot force somebody to answer is a
        // fault nobody answers (`store::StoreFault`'s and `fake::Fault`'s rule).
        //
        // Each arm reaches a different path through the muxer and leaves a different file, so
        // the assertions are per-variant and exhaustive: a fifth fault does not compile until
        // this walk has an answer for it. Which of the two readers succeeds is asserted in
        // **both** directions, because a change that made a truncated recording strictly
        // parseable would mean the close had run over a poisoned sink.
        let scratch = Scratch::new();

        // What a four-frame recording costs, so `WritesRefused` can be given a budget that
        // lands in the middle of a frame rather than on a chunk boundary.
        let mut camera = camera_from("chicony-rgb");
        let whole = record_with(
            camera.as_mut(),
            &scratch.path("whole.avi"),
            RecordingCaps {
                max_frames: 4,
                ..generous()
            },
            &FrozenClock,
        )
        .expect("a control recording");
        assert_eq!(whole.summary.frames_written, 4);

        let mut checked = 0;
        for &fault in SinkFault::ALL {
            let path = scratch.path(&format!("{fault}.avi"));
            let mut files = match fault {
                SinkFault::WritesRefused => {
                    ScriptedFiles::new(fault).with_budget(whole.summary.bytes_written / 2)
                }
                other => ScriptedFiles::new(other),
            };
            let mut camera = camera_from("chicony-rgb");
            let mut opened = start(
                camera.as_mut(),
                &request(&path, Some(limits::MAX_RECORDING_MS)),
            )
            .expect("starts");
            // Bounded at four frames so the take *ends* rather than running for ever: the two
            // close-time faults are only reachable once something has asked the container to
            // close, and on this camera nothing else would.
            opened.params.caps = RecordingCaps {
                max_frames: 4,
                ..generous()
            };
            let refused = record_faulted(camera.as_mut(), &opened, &path, &mut files)
                .expect_err("every fault in this menu refuses");

            // One answer for the whole menu: D13's disk-shaped variant, naming the path the
            // caller asked for, because an unattended reader handed a `DeviceIo` for a full
            // disk will go and look at the camera.
            assert_eq!(refused.kind(), ErrorKind::StorageIo, "{fault}");
            let Error::StorageIo { path: named, .. } = &refused else {
                panic!("{fault}: a storage failure carries a path");
            };
            assert_eq!(named, &path, "{fault}");

            let bytes = std::fs::read(path.as_std_path()).unwrap_or_default();
            let strict = read::read_stream(&bytes).is_ok();
            let recovered = read::recover_frames(&bytes).len();
            match fault {
                // Nothing was promised and nothing is there.
                SinkFault::CreateRefused => {
                    assert!(
                        !path.exists(),
                        "{fault}: a file that was never created exists"
                    );
                }
                // The disk filled mid-take: a `movi` prefix and no index, so the recovery
                // reader gets the whole frames and the strict one refuses.
                SinkFault::WritesRefused => {
                    assert!(!strict, "{fault}: a poisoned sink produced an indexed file");
                    assert!(
                        (1..4).contains(&recovered),
                        "{fault}: expected a cut-short prefix, got {recovered} frames"
                    );
                }
                // A sink that cannot be rewound cannot carry an AVI, and the muxer asks it
                // where it is before it asks it to hold anything — so the file exists, is
                // empty, and no frame was ever captured for it.
                SinkFault::SeekRefused => {
                    assert!(path.exists(), "{fault}: the file was never created");
                    assert!(
                        bytes.is_empty(),
                        "{fault}: {} bytes reached a sink that cannot seek",
                        bytes.len()
                    );
                    assert!(!strict, "{fault}: an empty file parsed strictly");
                    assert_eq!(recovered, 0, "{fault}");
                }
                // The one arm where the file on disk is whole and the answer is still a
                // failure: a flush that refused is a write this process cannot say landed.
                SinkFault::FlushRefused => {
                    assert!(
                        strict,
                        "{fault}: every byte and every patch landed, so the strict reader must \
                         still accept the file"
                    );
                    assert_eq!(recovered, 4, "{fault}");
                }
            }
            checked += 1;
        }
        assert_eq!(checked, SinkFault::ALL.len(), "the walk skipped a fault");
    }

    /// [`record_with`]'s shape for a take whose *file* is the thing under test.
    ///
    /// It takes an already-negotiated [`Opened`] because the fault walk tightens the caps to
    /// give the recording an ending, and it closes the container on the failing path exactly
    /// as [`run`] does — which is the behaviour being asserted: a fault leaves a file.
    fn record_faulted(
        camera: OpenCamera<'_>,
        opened: &Opened,
        path: &Utf8Path,
        files: &mut dyn Files,
    ) -> Result<RecordReport> {
        let mut recording = Recording::begin(opened, path, files, &FrozenClock, Stamp::epoch())?;
        let outcome = drive(&mut *camera, &mut recording, &FrozenClock);
        stop(&mut *camera).expect("stops");
        match outcome {
            Ok(ended) => recording.finish(ended, &FrozenClock),
            Err(refused) => {
                let _ = recording.finish(RecordingEnd::DeviceFailed, &FrozenClock);
                Err(refused)
            }
        }
    }

    #[test]
    fn the_disk_full_errno_is_the_kernels_own() {
        // Note **N10**: the injected `ENOSPC` must be the number the kernel produces, not the
        // number the author believes. `/dev/full` is a filesystem that is always full and it
        // costs nothing — `crate::store`'s suite asks it the same question about its own copy
        // of the constant, and two homes for a fixture's number is exactly what would let one
        // of them drift into fiction.
        let Ok(mut full) = std::fs::OpenOptions::new().write(true).open("/dev/full") else {
            println!("SKIP: /dev/full is absent, so the kernel cannot be asked for ENOSPC");
            return;
        };
        let err = full
            .write_all(b"anything")
            .and_then(|()| full.flush())
            .expect_err("/dev/full is always full");
        assert_eq!(
            err.raw_os_error(),
            Some(ENOSPC),
            "the kernel disagrees with our ENOSPC: {err}"
        );

        // And the errno reaches D13 rather than being replaced by the muxer's own words,
        // which is the whole of what `Tap` exists for: `imaging` answers `DeviceIo` naming its
        // step, because a container has no business knowing what a path is.
        let scratch = Scratch::new();
        let path = scratch.path("full.avi");
        let mut camera = camera_from("chicony-rgb");
        let refused = run(
            camera.as_mut(),
            &request(&path, Some(1_000)),
            &mut ScriptedFiles::new(SinkFault::WritesRefused),
            &FrozenClock,
            Stamp::epoch(),
        )
        .expect_err("a disk with no room refuses the header");
        let Error::StorageIo { errno, .. } = refused else {
            panic!("a full disk is a storage failure");
        };
        assert_eq!(errno, Some(ENOSPC));
    }

    #[test]
    fn a_grey_camera_records_y4m_and_a_request_for_avi_is_refused_naming_what_avi_carries() {
        // D7's MJPG-only muxer policy, proven against a device that really cannot do MJPG
        // rather than against a synthetic one: `chicony-ir` is a committed profile of a real
        // camera whose only format is GREY, and the fake replays it.
        let scratch = Scratch::new();

        // Asked for by name, and served.
        let named = scratch.path("ir.y4m");
        let clock = TickingClock::new(40);
        let mut camera = camera_from("chicony-ir");
        let report = run(
            camera.as_mut(),
            &request(&named, Some(120)),
            &mut OnDisk,
            &clock,
            Stamp::epoch(),
        )
        .expect("a GREY camera records");
        assert_eq!(report.format, VideoFormat::Y4m);
        assert_eq!(report.negotiated.pixel_format, PixelFormat::GREY);
        let bytes = std::fs::read(named.as_std_path()).expect("the file exists");
        assert!(bytes.starts_with(b"YUV4MPEG2 "), "not a Y4M file");
        assert_eq!(
            bytes
                .windows(6)
                .filter(|window| *window == b"FRAME\n")
                .count(),
            usize::try_from(report.summary.frames_written).expect("fits")
        );
        assert!(report.summary.frames_written > 0, "a file of no frames");

        // Not asked for at all, and answered from the negotiated stream — the arm AGENTS'
        // handless consumer depends on, since an agent that has not enumerated a camera's
        // formats cannot know to type `.y4m`.
        let unnamed = scratch.path("ir-unnamed");
        let clock = TickingClock::new(40);
        let mut camera = camera_from("chicony-ir");
        let report = run(
            camera.as_mut(),
            &request(&unnamed, Some(120)),
            &mut OnDisk,
            &clock,
            Stamp::epoch(),
        )
        .expect("a path with no extension lets the stream decide");
        assert_eq!(report.format, VideoFormat::Y4m);

        // And asked for a container this stream cannot fill: refused, naming what AVI would
        // have taken, and never quietly redirected to the other container — a caller who typed
        // `.avi` and received a Y4M has been answered a question it did not ask.
        let wrong = scratch.path("ir.avi");
        let (backend, mut camera) = watched("chicony-ir");
        let refusal = run(
            camera.as_mut(),
            &request(&wrong, Some(120)),
            &mut OnDisk,
            &FrozenClock,
            Stamp::epoch(),
        )
        .expect_err("AVI carries no GREY");
        let Error::FormatUnsupported {
            requested,
            available,
        } = &refusal
        else {
            panic!("a container that cannot carry the stream is a capability answer: {refusal}");
        };
        assert_eq!(*requested, Some(PixelFormat::GREY));
        assert_eq!(*available, vec![PixelFormat::MJPG, PixelFormat::JPEG]);
        assert!(!wrong.exists(), "a refused recording wrote a file");
        assert_eq!(
            backend.streams_started(),
            1,
            "the refusal is about the negotiation, so it happens after one"
        );
        assert!(
            camera.streaming().is_none(),
            "the refused recording left the camera streaming"
        );
    }

    #[test]
    fn a_recording_run_to_its_duration_lands_a_file_the_strict_reader_accepts() {
        // The whole pipeline over the fake, checked by the adversarial parser docs/7 P6a
        // required: `imaging::avi::read::read_stream` was written from the RIFF/AVI
        // specification before the muxer existed and refuses every size field the bytes do not
        // back. What this adds to the muxer's own suite is that the **engine** assembled the
        // parameters — the extent and the negotiated interval — so a build that handed the
        // muxer the *requested* size instead of the negotiated one fails here.
        let scratch = Scratch::new();
        let path = scratch.path("take.avi");
        let clock = TickingClock::new(50);
        let mut camera = camera_from("chicony-rgb");
        let report = run(
            camera.as_mut(),
            &request(&path, Some(200)),
            &mut OnDisk,
            &clock,
            Stamp::epoch(),
        )
        .expect("records");

        let bytes = std::fs::read(path.as_std_path()).expect("the file exists");
        assert_eq!(
            u64::try_from(bytes.len()).expect("fits"),
            report.summary.bytes_written,
            "the summary disagrees with the file's own length"
        );
        let stream = read::read_stream(&bytes).expect("the strict reader accepts our own output");
        assert_eq!(report.summary.frames_written, 3);
        assert_eq!(
            stream.frames.len(),
            usize::try_from(report.summary.frames_written).expect("fits")
        );
        assert_eq!(stream.headers.width, report.negotiated.width);
        assert_eq!(stream.headers.height, report.negotiated.height);
        assert_eq!(report.camera, camera.info().id);
        assert_eq!(report.started_at, Stamp::epoch());
        assert_eq!(report.path, path);
        assert_eq!(report.format, VideoFormat::Avi);
    }

    #[test]
    fn the_negotiated_stream_reaches_the_report_when_it_differs_from_the_request() {
        // D5 at the layer above the backend, for a stream that ran for a while rather than for
        // one frame: the caller asked for something this camera does not have, and the answer
        // says so rather than quietly being a different recording.
        let scratch = Scratch::new();
        let path = scratch.path("adjusted.avi");
        let clock = TickingClock::new(40);
        let mut camera = camera_from("chicony-rgb");
        let asking = RecordRequest {
            stream: StreamRequest {
                width: Some(9_999),
                height: Some(9_999),
                ..StreamRequest::default()
            },
            ..request(&path, Some(80))
        };
        let report = run(
            camera.as_mut(),
            &asking,
            &mut OnDisk,
            &clock,
            Stamp::epoch(),
        )
        .expect("records");

        assert!(
            !report.negotiated.is_exact(),
            "a request for 9999x9999 was honoured exactly"
        );
        assert!(report.negotiated.width < 9_999);
        // And the file is the *negotiated* size rather than the requested one, which is the
        // difference between reporting an adjustment and making one.
        let bytes = std::fs::read(path.as_std_path()).expect("the file exists");
        let stream = read::read_stream(&bytes).expect("parses");
        assert_eq!(stream.headers.width, report.negotiated.width);
    }

    #[test]
    fn a_device_that_vanished_mid_recording_leaves_an_indexed_file_and_the_devices_own_words() {
        // The other half of "every fault leaves a parseable file", and the half whose reader
        // is the strict one: the *sink* is healthy here, so the close runs, the `idx1` lands
        // and `read_stream` accepts the file. A build that abandoned the recording on a device
        // failure would leave a header full of placeholders and no index, which is exactly
        // what the `SeekRefused` arm of the fault walk looks like — so the two tests together
        // say which failure leaves which file rather than only that something is readable.
        let scratch = Scratch::new();
        let path = scratch.path("unplugged.avi");
        let (backend, mut camera) = watched("chicony-rgb");
        backend.queue_fault(Fault::DeviceGoneMidStream);

        let refusal = run(
            camera.as_mut(),
            &request(&path, Some(limits::MAX_RECORDING_MS)),
            &mut OnDisk,
            &FrozenClock,
            Stamp::epoch(),
        )
        .expect_err("the camera was unplugged");
        assert_eq!(
            refusal.kind(),
            ErrorKind::DeviceGone,
            "the device's own refusal was replaced"
        );

        let bytes = std::fs::read(path.as_std_path()).expect("the file exists");
        let stream = read::read_stream(&bytes).expect("a device failure still closes the file");
        assert_eq!(
            stream.index.len(),
            stream.frames.len(),
            "the close ran and wrote an index that does not match the frames"
        );
    }

    #[test]
    fn every_ending_in_the_vocabulary_is_one_a_recording_can_reach() {
        // An ending nothing can produce is a word in a report rather than an answer. The two
        // the loop never produces are exactly the two a `record_stop` and a mid-take device
        // failure produce at docs/7 P6c, so they are driven here through the turn-based half —
        // the surface that daemon will hold. Walked, so a sixth ending has to be given a
        // scenario rather than inheriting one.
        let scratch = Scratch::new();
        let mut checked = 0;
        for &ending in RecordingEnd::ALL {
            let path = scratch.path(&format!("{ending:?}.avi"));
            let reached = match ending {
                RecordingEnd::Duration => {
                    let clock = TickingClock::new(40);
                    let mut camera = camera_from("chicony-rgb");
                    run(
                        camera.as_mut(),
                        &request(&path, Some(80)),
                        &mut OnDisk,
                        &clock,
                        Stamp::epoch(),
                    )
                    .expect("records")
                    .ended
                }
                RecordingEnd::Cap => {
                    let mut camera = camera_from("chicony-rgb");
                    record_with(
                        camera.as_mut(),
                        &path,
                        RecordingCaps {
                            max_frames: 1,
                            ..generous()
                        },
                        &FrozenClock,
                    )
                    .expect("records")
                    .ended
                }
                RecordingEnd::DeviceQuiet => {
                    let mut camera = ScriptedCamera::new(vec![integer("brightness", 50)])
                        .compressed()
                        .with_frames((0..2).map(compressed_frame).collect());
                    run(
                        &mut camera,
                        &request(&path, Some(limits::MAX_RECORDING_MS)),
                        &mut OnDisk,
                        &FrozenClock,
                        Stamp::epoch(),
                    )
                    .expect("records")
                    .ended
                }
                // The two the loop never produces: a caller that ended the take, and a caller
                // holding the device's own refusal. Both are the turn-based half, driven
                // exactly as `record_stop` will drive it.
                RecordingEnd::Stopped | RecordingEnd::DeviceFailed => {
                    let mut camera = camera_from("chicony-rgb");
                    let opened =
                        start(camera.as_mut(), &request(&path, Some(1_000))).expect("starts");
                    let mut recording =
                        Recording::begin(&opened, &path, &mut OnDisk, &FrozenClock, Stamp::epoch())
                            .expect("begins");
                    let Turn::Frame(frame) = turn(camera.as_mut()).expect("a frame") else {
                        panic!("the fake delivers");
                    };
                    recording.write(&frame).expect("writes");
                    stop(camera.as_mut()).expect("stops");
                    recording
                        .finish(ending, &FrozenClock)
                        .expect("finishes")
                        .ended
                }
            };
            assert_eq!(reached, ending, "{ending:?} was not the ending it produced");
            assert!(path.exists(), "{ending:?} left no file");
            checked += 1;
        }
        assert_eq!(
            checked,
            RecordingEnd::ALL.len(),
            "the walk skipped an ending"
        );
    }
}
