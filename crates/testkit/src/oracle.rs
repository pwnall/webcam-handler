//! The container oracles: `ffprobe` and `mpv`, asked whether a file this workspace wrote is
//! the file this workspace says it wrote (docs/7 **P6d**, docs/9's "Oracle rung accounting").
//!
//! Everything else that checks a recording in this repository was written here.
//! `imaging::avi::read` is an AVI parser derived from the RIFF/AVI specification *before* the
//! muxer existed and sharing no constant with it, `imaging::y4m`'s `parse_y4m` is the same
//! posture one container along, and `scripts/gates/avi-reparse-is-independent.sh` exists to stop
//! the two sides quietly acquiring a shared helper. That independence is real and it has a
//! ceiling: **both readers were written by the same people, from the same reading of the same
//! specification, for the same muxer.** A misreading shared by the writer and the reader is
//! invisible to both. An oracle is the answer to that — a program that has never read our code,
//! handed our bytes, asked what it sees.
//!
//! ## What each oracle is for, and why one may never stand in for the other
//!
//! **`ffprobe` reads the container**: it demuxes the file, names the format and the codec, and
//! reports one record per frame it actually pulled out of the stream. That is the claim worth
//! having, and every one of [`OracleArm::Demux`], [`OracleArm::Frames`] and [`OracleArm::Rate`]
//! is a reading of its JSON.
//!
//! **`mpv` answers one question `ffprobe` cannot: does the file play, end to end.** And that is
//! *all* it is for here, because **`mpv` is not a second opinion about the demux.** Measured on
//! the P6d host: `mpv` 0.41.0 links the same FFmpeg build `ffprobe` 8.0.1 does — libavcodec
//! 62.11.100, libavformat 62.3.100 — so a demux the two agree about is one program agreeing with
//! itself. Treating that as corroboration is exactly the false-independence error
//! `avi-reparse-is-independent.sh` was written to prevent, arriving from outside the workspace
//! instead of from inside it. So [`OracleArm::Playback`] claims playability and nothing else,
//! and no arm here is checked twice by two names.
//!
//! Playability is not a redundant claim, and a measurement says so: an AVI whose header list
//! survives with **zero frames behind it** is accepted by `ffprobe` (exit 0, `nb_frames: "2"`
//! read straight out of `avih.dwTotalFrames`) and refused by `mpv` (exit 2, nothing to play).
//! Two programs, one library, two different questions.
//!
//! ## `ffprobe`'s exit code is not a validity oracle, and this is the measurement
//!
//! Every damaged file tried on the P6d host — a recording cut mid-frame, an AVI header list with
//! no `movi` behind it, a `movi` prefix a poisoned sink left with its size fields still zero —
//! exits **0**. `ffprobe` exits **1** only when libavformat could not open the input at all
//! (a missing path, or bytes that probe as nothing), and **127** is the shell reporting a binary
//! that is not there. So the three answers separate cleanly —
//!
//! | exit | meaning | this harness |
//! |---|---|---|
//! | 127 / spawn `NotFound` | the oracle is not installed | a named, counted decline |
//! | 1 | libavformat refused the file | a failure, with the diagnostic's first line |
//! | 0 | libavformat accepted the file | **the claims are judged on the JSON** |
//!
//! — and the last row is the whole design. A harness that checked the exit status would be
//! green over a recording holding no frames, which is vacuous, and vacuous is the one thing this
//! project's rungs may not be (docs/8 Part C).
//!
//! What the JSON is judged on is chosen for the same reason. **`nb_frames` is a header claim,
//! not a count** — for AVI it is `avih.dwTotalFrames` verbatim, and a file cut mid-frame still
//! reports it — so this harness counts the `frames` array `-count_frames` produces and treats
//! `nb_frames` as a *second* number that must agree with it. And `nb_read_frames` over-counts on
//! its own, because a truncated JPEG still decodes ("overread 8" is a diagnostic, not an error):
//! only `pkt_size` exposes a short frame, which is why [`Expectation::with_payload_bytes`]
//! exists for the callers that know what they wrote.
//!
//! ## `-v error`, and why the next reader should not go hunting
//!
//! `crates/imaging/fixtures/avi/two-frame-64x48.avi` makes ffmpeg print `[mjpeg] EOI missing,
//! emulating` at `-v warning`. It was checked: **both frames end with `FFD9`**, so the marker is
//! present and this is an ffmpeg quirk on a 409-byte quality-15 JPEG. At `-v error` our healthy
//! files are silent, which is the level this harness uses — and a `-v warning` that treated that
//! line as a finding would have failed the fixture the muxer is pinned to.
//!
//! ## The scope limit, stated rather than left to be discovered
//!
//! **Our containers are constant-frame-rate by construction.** An AVI's `strh` carries one
//! rational for the whole stream and a Y4M header carries one `F` ratio, so both declare the
//! *mean* interval `imaging::video::declared_interval` measured and a player reconstructs every
//! timestamp from it. `ffprobe`'s `pts` values are therefore 0, 1, 2, … in a time base that is
//! the mean, whatever the camera really did between frames. So an oracle here validates **the
//! mean and never the jitter**, and no arm below claims otherwise: per-frame timing is carried
//! by `RecordingSummary::span_us` and `dropped_frames`, measured on the driver's own clock,
//! which is where a caller asking about a transition has to look. D7's CFR carve-out is that
//! limitation and design §3.3 is where it is argued; this paragraph is so that a reader of a
//! green oracle run does not read more into it than it says.
//!
//! ## A frame may contain a person
//!
//! Rubric A12, and this module runs over files that came off a real camera on the R3 rung. No
//! payload is read here: the `ffprobe` invocation asks for named entries only, no arm decodes a
//! sample, and a refusal quotes lengths, counts, rationals and the first line of a tool's own
//! diagnostic — capped, so an oracle that decided to be chatty cannot spill a buffer into a
//! transcript.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::process::Command;

use camino::{Utf8Path, Utf8PathBuf};
use schema::capture::ChromaSiting;
use schema::video::{RecordingSummary, VideoFormat};

use crate::battery::ArmOutcome;
use crate::vocabulary::closed_vocabulary;

closed_vocabulary! {
    /// One of the two programs this rung asks.
    ///
    /// Closed, and the closure is the point: an oracle nobody located is an oracle nobody
    /// declined for, so [`OracleReport`] walks this list rather than a set built from whatever
    /// happened to be found.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub enum Oracle {
        /// libavformat's own inspector: the demux, the frame records, the declared rate.
        Ffprobe,
        /// A player. End-to-end playability, and deliberately nothing else — see the module
        /// doc for why it may not corroborate a demux.
        Mpv,
    }
}

impl Oracle {
    /// The oracle's name, as it appears in reports.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Oracle::Ffprobe => "ffprobe",
            Oracle::Mpv => "mpv",
        }
    }

    /// The executable to look for on `$PATH`.
    ///
    /// The same string as [`Oracle::as_str`] today, and a separate function anyway: what a
    /// report calls a tool and what a host installs it as are two facts that happen to agree.
    #[must_use]
    pub const fn binary(self) -> &'static str {
        match self {
            Oracle::Ffprobe => "ffprobe",
            Oracle::Mpv => "mpv",
        }
    }

    /// The one question this oracle is here to answer, in a clause a decline can print.
    ///
    /// A value rather than a sentence typed into each message, because a decline has to say what
    /// the run *did not learn* — and the two oracles do not answer the same thing, so "an oracle
    /// was missing" would be a decline nobody can price.
    #[must_use]
    pub const fn answers(self) -> &'static str {
        match self {
            Oracle::Ffprobe => {
                "whether libavformat demuxes this file into the frames, geometry and rate its \
                 summary claims"
            }
            Oracle::Mpv => "whether the file plays end to end",
        }
    }

    /// How this oracle is invoked, for the argument list a decline names as its remedy.
    #[must_use]
    pub const fn install_hint(self) -> &'static str {
        match self {
            Oracle::Ffprobe => "install ffmpeg's ffprobe (Debian/Ubuntu: `apt install ffmpeg`)",
            Oracle::Mpv => "install mpv (Debian/Ubuntu: `apt install mpv`)",
        }
    }
}

impl fmt::Display for Oracle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

closed_vocabulary! {
    /// One claim a container oracle makes about a file.
    ///
    /// The same shape [`crate::battery::BatteryArm`] has, and for its reason: `ALL` is generated
    /// from this definition, so an arm cannot be added without joining the walk in [`validate`]
    /// and satisfying the exhaustive `match` that dispatches it.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub enum OracleArm {
        /// libavformat opens the file, names one video stream, and agrees about the container,
        /// the codec and the geometry.
        Demux,
        /// The frames it pulls out are the frames we wrote — counted, sized, and in order.
        Frames,
        /// The rate the file declares is the interval the summary says it declared.
        Rate,
        /// The file plays end to end.
        Playback,
    }
}

impl OracleArm {
    /// The arm's name, as it appears in reports.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            OracleArm::Demux => "demux",
            OracleArm::Frames => "frames",
            OracleArm::Rate => "rate",
            OracleArm::Playback => "playback",
        }
    }

    /// Which oracle answers this arm.
    ///
    /// One-way by construction: an arm belongs to exactly one program, so a claim can never be
    /// satisfied by whichever of the two happened to be installed. That is the module doc's
    /// false-independence rule expressed as a total function rather than as a convention.
    #[must_use]
    pub const fn oracle(self) -> Oracle {
        match self {
            OracleArm::Demux | OracleArm::Frames | OracleArm::Rate => Oracle::Ffprobe,
            OracleArm::Playback => Oracle::Mpv,
        }
    }
}

impl fmt::Display for OracleArm {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What the caller says the file is, for an oracle to disagree with.
///
/// **Built from the recording's own answer document**, which is what makes a green run mean
/// something: [`Expectation::for_take`] takes the [`RecordingSummary`] the muxer produced, so
/// every arm below compares a third party's reading against *this workspace's claim about the
/// same bytes* rather than against a number a test typed. A muxer that wrote a file and reported
/// it wrongly fails here; a muxer that wrote it wrongly and reported the same wrongness fails
/// here too, because `ffprobe` is reading the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Expectation {
    /// The container the recording says it is in.
    pub container: VideoFormat,
    /// The negotiated frame width.
    pub width: u32,
    /// The negotiated frame height.
    pub height: u32,
    /// How many frames the summary says the file holds.
    pub frames: u32,
    /// The interval the summary says the file's own rate field declares.
    pub declared_interval_us: u32,
    /// Each frame's payload length, for a caller that knows them.
    ///
    /// `None` for a real capture, where nothing outside the muxer ever saw a frame's length —
    /// and the [`OracleArm::Frames`] arm then asserts only that every payload is non-empty.
    /// `Some` for a take this workspace generated, where it is the **only** thing that catches a
    /// file cut mid-frame: `nb_frames` is a header claim, `nb_read_frames` over-counts because a
    /// truncated JPEG still decodes, and `pkt_size` is where the 350 bytes of a 1162-byte frame
    /// show up.
    pub payload_bytes: Option<Vec<u32>>,
    /// The chroma siting the recording was opened with, for a payload that states one.
    ///
    /// `None` — the default — for every recording whose container and colorspace name no
    /// siting: AVI's MJPEG carries its own sampling inside the bitstream, and Y4M's `Cmono` and
    /// `C422` tags name none at all (measured: `ffprobe` answers `unspecified` for both). `Some`
    /// only for a 4:2:0 raw recording, where the header *must* state one and this workspace
    /// chooses which — which is precisely the claim that had no reader (note **N200**, and
    /// **N210** for why it had none until now).
    pub chroma_siting: Option<ChromaSiting>,
}

impl Expectation {
    /// What a finished take claims about itself.
    ///
    /// The geometry is the caller's because a summary does not carry one — it is
    /// `NegotiatedStream`'s, one type up, and asking for it here keeps this crate from depending
    /// on which of the two documents the caller happens to be holding.
    #[must_use]
    pub fn for_take(
        container: VideoFormat,
        width: u32,
        height: u32,
        summary: &RecordingSummary,
    ) -> Expectation {
        Expectation {
            container,
            width,
            height,
            frames: summary.frames_written,
            declared_interval_us: summary.declared_interval_us,
            payload_bytes: None,
            chroma_siting: None,
        }
    }

    /// Add the per-frame payload lengths, for a caller that generated them.
    #[must_use]
    pub fn with_payload_bytes(mut self, bytes: Vec<u32>) -> Expectation {
        self.payload_bytes = Some(bytes);
        self
    }

    /// Add the chroma siting the recording was opened with, for a 4:2:0 raw take.
    ///
    /// The caller's, not derived here, and that is the whole point of the arm it enables: the
    /// siting is a **capture-time input** (`imaging::video::RecordingParams::chroma_siting`),
    /// so an expectation that computed it from the file's own colorspace would be this
    /// workspace comparing its answer against its answer — which is the move that let a tag
    /// mean something nobody had asked a reader about.
    #[must_use]
    pub fn with_chroma_siting(mut self, siting: ChromaSiting) -> Expectation {
        self.chroma_siting = Some(siting);
        self
    }

    /// The `format_name` libavformat gives this container.
    ///
    /// Transcribed from what `ffprobe` answered on the P6d host and pinned here rather than
    /// accepted from the file, because "it demuxed as *something*" is not the claim: a Y4M that
    /// probed as `avi` would be a file our own reader is wrong about.
    const fn format_name(container: VideoFormat) -> &'static str {
        match container {
            VideoFormat::Avi => "avi",
            VideoFormat::Y4m => "yuv4mpegpipe",
        }
    }

    /// The `codec_name` libavformat gives this container's payload.
    const fn codec_name(container: VideoFormat) -> &'static str {
        match container {
            // Our AVI carries the camera's own JPEG bitstream verbatim (E6), and `mjpeg` is what
            // libavcodec calls that whether the FourCC was `MJPG` or `JPEG`.
            VideoFormat::Avi => "mjpeg",
            // Y4M's payload is planes, so there is no codec to name and libavformat says so.
            VideoFormat::Y4m => "rawvideo",
        }
    }

    /// The `chroma_location` `ffprobe` reads out of a file written at this siting.
    ///
    /// [`Expectation::format_name`]'s shape, for its reason: transcribed from what the third
    /// party answered rather than accepted from the file, because "it reported *a* siting" is
    /// not the claim. An exhaustive match over [`ChromaSiting`], so a third member of that
    /// vocabulary cannot be added without somebody measuring what a reader makes of it.
    ///
    /// Measured 2026-08-16 with `ffprobe` 8.0.1 (note **N200**'s table): `C420` and `C420jpeg`
    /// both read `center`, `C420mpeg2` reads `left`, `C420paldv` reads `topleft`. The two rows
    /// this build can write are the first and the third.
    const fn chroma_location(siting: ChromaSiting) -> &'static str {
        match siting {
            ChromaSiting::Centred => "center",
            ChromaSiting::HorizontallyCosited => "left",
        }
    }
}

/// What one oracle run found, in [`crate::battery::BatteryReport`]'s vocabulary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OracleReport {
    /// The file that was asked about.
    pub file: Utf8PathBuf,
    /// Every arm's outcome — the map is total over [`OracleArm::ALL`], so a missing arm is not
    /// expressible.
    pub outcomes: BTreeMap<OracleArm, ArmOutcome>,
    /// The oracles this host does not have, and the sentence a decline prints for each.
    ///
    /// Kept apart from [`Self::outcomes`] deliberately. An arm that did not run because a
    /// **binary is missing** is a fact about the host and belongs in a named, counted skip; an
    /// arm that did not run because an earlier arm found the file **broken** is a fact about the
    /// file, and printing it as a skip is how a red run learns to read as a quiet one.
    pub absent: BTreeMap<Oracle, String>,
    /// Everything wrong, arm-prefixed. Empty means the run is green.
    pub failures: Vec<String>,
    /// Whether [`Self::absent`] is what **this host** is missing, or what a double invented.
    ///
    /// Carried on the report because [`Self::print`] is the one call that puts a decline where
    /// a census will count it, and a decline is only true if a host was actually asked (note
    /// **N235**). [`Tools::absences_are_this_hosts`] is where each implementation answers.
    pub measured_on_this_host: bool,
}

impl OracleReport {
    /// Whether the run found nothing wrong.
    ///
    /// A run in which every arm declined is green, and that is not a loophole: the declines are
    /// in [`Self::absent`], [`Self::print`] prints one line each, and `scripts/rung-oracles.sh`
    /// turns them into the rung's verdict. What must never happen is a *failure* wearing a
    /// skip's clothes, which is why the two live in different fields.
    #[must_use]
    pub fn is_green(&self) -> bool {
        self.failures.is_empty()
    }

    /// This arm's outcome. `None` cannot happen for an arm in [`OracleArm::ALL`]; the signature
    /// is `Option` only because `BTreeMap` says so.
    #[must_use]
    pub fn outcome(&self, arm: OracleArm) -> Option<&ArmOutcome> {
        self.outcomes.get(&arm)
    }

    /// The failures mentioning `arm`.
    #[must_use]
    pub fn failures_for(&self, arm: OracleArm) -> Vec<&str> {
        let prefix = format!("{arm}: ");
        self.failures
            .iter()
            .filter(|failure| failure.starts_with(&prefix))
            .map(String::as_str)
            .collect()
    }

    /// How many arms executed.
    #[must_use]
    pub fn ran(&self) -> usize {
        self.outcomes.values().filter(|o| o.ran()).count()
    }

    /// The arms an absent oracle would have answered.
    #[must_use]
    pub fn arms_of(oracle: Oracle) -> Vec<OracleArm> {
        OracleArm::ALL
            .iter()
            .copied()
            .filter(|arm| arm.oracle() == oracle)
            .collect()
    }

    /// One line per oracle this host does not have, in the shape the runner counts.
    ///
    /// A decline names what was missing, what it therefore did not claim, and the remedy —
    /// because AGENTS' primary consumer has no hands to work any of the three out with.
    ///
    /// Returned rather than printed so that a caller can hold one **without emitting it**, and
    /// that is not a convenience: `every_host_the_fault_menu_names_declines_by_name_rather_than_passing`
    /// drives [`ScriptedTools`], whose absences are invented, and a fabricated decline printed
    /// into the run log would be read by `scripts/rung-oracles.sh` as this host lacking a tool it
    /// has. Watched happening: the rung reported `SKIPPED — the rung declined, 4 time(s)` on a
    /// machine with both oracles installed, which is a false absence wearing the accounting's own
    /// words.
    #[must_use]
    pub fn decline_lines(&self) -> Vec<String> {
        self.absent
            .iter()
            .map(|(oracle, reason)| {
                let arms: Vec<&str> = OracleReport::arms_of(*oracle)
                    .into_iter()
                    .map(OracleArm::as_str)
                    .collect();
                format!(
                    "SKIP oracles: {oracle} is not on this host ({reason}), so {} claim(s) about \
                     {} went unasked — {} — and nothing here says {}; remedy: {}",
                    arms.len(),
                    self.file,
                    arms.join(", "),
                    oracle.answers(),
                    oracle.install_hint()
                )
            })
            .collect()
    }

    /// The line the runner counts when this run made claims, or `None` when it made none.
    #[must_use]
    pub fn run_line(&self) -> Option<String> {
        let ran = self.ran();
        (ran > 0).then(|| {
            format!(
                "oracles: {ran} container claim(s) about {} checked by {}",
                self.file,
                self.checked_by().join(" and ")
            )
        })
    }

    /// Print this run the way `scripts/rung-oracles.sh` accounts for it.
    ///
    /// Two line shapes and the runner greps for both, which is `crates/daemon/tests/
    /// web_browser.rs`'s arrangement and its reason: **a rung that printed neither is a rung
    /// nobody can tell apart from one that was never invoked**, so the runner treats that as a
    /// failure and this function is what makes one of the two always appear.
    ///
    /// **A fabricated decline cannot leave through here** (note **N235**, the G6 review's
    /// finding 12 against B10). This is the second half of the split
    /// [`Self::decline_lines`] describes, and until 2026-08-17 the split was held by a comment:
    /// the fabricated-host arm knew to call `decline_lines()` instead of this, and nothing
    /// stopped the *next* arm from calling this. Two censuses read these lines —
    /// `scripts/smoke-hw.sh`'s `^\s*SKIP` grep over a saved run log, which is what
    /// `$WCH_SMOKE_HW_ACCOUNT` re-reads, and `scripts/rung-oracles.sh`'s narrower anchor — and
    /// both believe whatever printed the sentence. So the report says whether a host was asked
    /// and this refuses to emit an absence no host reported. It is the treatment
    /// `battery::RestorationClaim::account_for` got at the same time (note **N231**), for the
    /// same reason: the refusal comes before the printing, because a line already in the log
    /// cannot be taken back.
    ///
    /// # Panics
    ///
    /// When the report carries absences a [`Tools`] double invented.
    pub fn print(&self) {
        assert!(
            self.absent.is_empty() || self.measured_on_this_host,
            "{}: this report's {} absent oracle(s) were invented by a Tools double, and \
             printing them would put a decline about a host nobody asked into a log two \
             censuses count (note N235). Assert `decline_lines()` instead.",
            self.file,
            self.absent.len()
        );
        for line in self.decline_lines() {
            println!("{line}");
        }
        if let Some(line) = self.run_line() {
            println!("{line}");
        }
    }

    /// The oracles that actually answered something, for the run line.
    fn checked_by(&self) -> Vec<&'static str> {
        let used: BTreeSet<Oracle> = self
            .outcomes
            .iter()
            .filter(|(_, outcome)| outcome.ran())
            .map(|(arm, _)| arm.oracle())
            .collect();
        used.into_iter().map(Oracle::as_str).collect()
    }
}

impl fmt::Display for OracleReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "oracle report for {}", self.file)?;
        for (arm, outcome) in &self.outcomes {
            match outcome {
                ArmOutcome::Ran => writeln!(f, "  {arm} ({}): ran", arm.oracle())?,
                ArmOutcome::Skipped { reason } => {
                    writeln!(f, "  {arm} ({}): skipped — {reason}", arm.oracle())?;
                }
            }
        }
        if self.failures.is_empty() {
            writeln!(f, "  no failures")
        } else {
            writeln!(f, "  {} failure(s):", self.failures.len())?;
            for failure in &self.failures {
                writeln!(f, "    - {failure}")?;
            }
            Ok(())
        }
    }
}

// ------------------------------------------------------------------- the host seam

/// Where an oracle binary is found, if it is here at all.
///
/// Design §2.9's shape: a real implementation and a scriptable double. The double is not a
/// convenience — **both oracles are installed on the machine P6d was built on**, so the decline
/// path could not otherwise be exercised at all, and a decline nobody has ever watched fire is a
/// decline nobody can trust. `crates/daemon/tests/web_browser.rs` reaches its own precondition
/// arms the same way: by inventing the host rather than by waiting to meet one.
pub trait Tools: fmt::Debug {
    /// The program to run for `oracle`, or `None` when this host does not have it.
    fn locate(&self, oracle: Oracle) -> Option<Utf8PathBuf>;

    /// Whether a `None` from [`Self::locate`] is **this host's** absence or an invented one.
    ///
    /// No default, so a third implementation has to answer rather than inherit (note
    /// **N235**). It is the one thing a report cannot work out for itself, and
    /// [`OracleReport::print`] is where it decides whether a decline may reach a log that two
    /// censuses count.
    fn absences_are_this_hosts(&self) -> bool;
}

/// The real lookup: `$PATH`, in order, first executable wins.
///
/// Written out rather than taken from a crate, and the reason is the dependency inventory rather
/// than the twelve lines: this is dev-only code, but `scripts/gates/dependency-walls.sh` and the
/// cargo-deny allowlist are about the *workspace*, and a `which`-shaped crate would be a licence
/// and a supply chain adopted to answer a question `std::env::var_os("PATH")` answers.
#[derive(Debug, Clone, Copy, Default)]
pub struct OnPath;

impl Tools for OnPath {
    fn locate(&self, oracle: Oracle) -> Option<Utf8PathBuf> {
        use std::os::unix::fs::PermissionsExt;

        let path = std::env::var_os("PATH")?;
        for directory in std::env::split_paths(&path) {
            let candidate = directory.join(oracle.binary());
            let Ok(metadata) = std::fs::metadata(&candidate) else {
                continue;
            };
            // Both halves: a directory named `ffprobe` is not a program, and a file nobody may
            // execute is not one either. Checking only `exists()` turns a broken host into a
            // failure at spawn time, which reads as the file being wrong.
            if metadata.is_file() && metadata.permissions().mode() & 0o111 != 0 {
                return Utf8PathBuf::from_path_buf(candidate).ok();
            }
        }
        None
    }

    /// Yes: this is the `$PATH` of the process that is running.
    fn absences_are_this_hosts(&self) -> bool {
        true
    }
}

closed_vocabulary! {
    /// A host this rung might meet, as a value.
    ///
    /// The fault menu design §2.9 asks of every seam, and an exhaustive `match` in
    /// [`ScriptedTools::locate`] so a fault cannot be added without deciding what it hides.
    /// Every variant **removes** an oracle and none invents one: a double that claimed a binary
    /// this host does not have would fail at spawn, which is a different finding from the one
    /// each of these is for.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub enum HostFault {
        /// Neither oracle is installed — the shape a fresh CI runner has.
        NoOracles,
        /// `mpv` is here and `ffprobe` is not.
        NoProbe,
        /// `ffprobe` is here and `mpv` is not — the shape a headless server usually has.
        NoPlayer,
    }
}

/// A host with one or both oracles taken away.
///
/// Delegates to [`OnPath`] for whatever the fault does not hide, so the double is *this* host
/// minus something rather than a fiction: an arm driven through it still runs a real program
/// against a real file, and only the absence is invented.
#[derive(Debug, Clone, Copy)]
pub struct ScriptedTools {
    fault: HostFault,
}

impl ScriptedTools {
    /// A host missing what `fault` names.
    #[must_use]
    pub const fn new(fault: HostFault) -> ScriptedTools {
        ScriptedTools { fault }
    }
}

impl Tools for ScriptedTools {
    fn locate(&self, oracle: Oracle) -> Option<Utf8PathBuf> {
        let hidden = match (self.fault, oracle) {
            (HostFault::NoOracles, _)
            | (HostFault::NoProbe, Oracle::Ffprobe)
            | (HostFault::NoPlayer, Oracle::Mpv) => true,
            (HostFault::NoProbe, Oracle::Mpv) | (HostFault::NoPlayer, Oracle::Ffprobe) => false,
        };
        if hidden { None } else { OnPath.locate(oracle) }
    }

    /// No: every absence this reports is [`HostFault`]'s invention, whatever the host has.
    fn absences_are_this_hosts(&self) -> bool {
        false
    }
}

// --------------------------------------------------------------------- the harness

/// How much of a tool's own diagnostic a failure may quote.
///
/// A frame may contain a person (rubric A12). Nothing below reads a payload, but a tool that
/// decided to dump one into its standard error would otherwise reach a transcript through this
/// harness — so the quote is bounded, and bounded here rather than at each call site.
const DIAGNOSTIC_BYTES: usize = 200;

/// Ask both oracles about `file`, on this host.
///
/// The [`OnPath`] arrangement of [`validate_with`], which is what every caller outside this
/// module's own tests wants.
#[must_use]
pub fn validate(file: &Utf8Path, expected: &Expectation) -> OracleReport {
    validate_with(&OnPath, file, expected)
}

/// Ask both oracles about `file`, on the host `tools` describes.
///
/// Every arm in [`OracleArm::ALL`] gets an outcome, and an oracle this host does not have puts
/// its arms in [`OracleReport::absent`] rather than passing them: a claim nobody made is never a
/// claim that held.
#[must_use]
pub fn validate_with(tools: &dyn Tools, file: &Utf8Path, expected: &Expectation) -> OracleReport {
    let mut report = OracleReport {
        file: file.to_owned(),
        outcomes: BTreeMap::new(),
        absent: BTreeMap::new(),
        failures: Vec::new(),
        measured_on_this_host: tools.absences_are_this_hosts(),
    };

    // The file first, and before either tool: "ffprobe could not open it" and "there is nothing
    // at that path" are answers a caller must be able to tell apart, and only one of them is
    // about the muxer.
    if !file.is_file() {
        report.failures.push(format!(
            "demux: {file} is not a file, so no oracle was asked about it; a recording that was \
             reported and never written is the failure this check exists for"
        ));
        for &arm in OracleArm::ALL {
            report.outcomes.insert(
                arm,
                ArmOutcome::skipped(format!("there is no file at {file} to ask about")),
            );
        }
        return report;
    }

    let probe = tools.locate(Oracle::Ffprobe);
    let player = tools.locate(Oracle::Mpv);
    let probed = match &probe {
        Some(binary) => run_ffprobe(binary, file, &mut report),
        None => {
            report.absent.insert(
                Oracle::Ffprobe,
                format!("no `{}` on this host's PATH", Oracle::Ffprobe.binary()),
            );
            None
        }
    };
    if player.is_none() {
        report.absent.insert(
            Oracle::Mpv,
            format!("no `{}` on this host's PATH", Oracle::Mpv.binary()),
        );
    }

    for &arm in OracleArm::ALL {
        let outcome = match arm {
            OracleArm::Demux => match &probed {
                Some(probed) => arm_demux(probed, expected, &mut report),
                None => declined(arm, &probe),
            },
            OracleArm::Frames => match &probed {
                Some(probed) => arm_frames(probed, expected, &mut report),
                None => declined(arm, &probe),
            },
            OracleArm::Rate => match &probed {
                Some(probed) => arm_rate(probed, expected, &mut report),
                None => declined(arm, &probe),
            },
            OracleArm::Playback => match &player {
                Some(binary) => arm_playback(binary, file, expected, &mut report),
                None => declined(arm, &player),
            },
        };
        report.outcomes.insert(arm, outcome);
    }

    report
}

/// The skip an arm gets when its oracle produced no reading.
///
/// Two different sentences, because they are two different facts and AGENTS rule 7 is the line
/// between them: a **missing binary** is about the host and is counted in
/// [`OracleReport::absent`], while an oracle that was *present* and answered nothing has already
/// pushed a failure and this skip only says the arm had nothing to read.
fn declined(arm: OracleArm, located: &Option<Utf8PathBuf>) -> ArmOutcome {
    match located {
        None => ArmOutcome::skipped(format!(
            "{} is not installed on this host, so nothing answered {}",
            arm.oracle(),
            arm.oracle().answers()
        )),
        Some(_) => ArmOutcome::skipped(format!(
            "{} refused the file, which is the failure recorded against the demux arm; there is \
             nothing here for this arm to read",
            arm.oracle()
        )),
    }
}

// ------------------------------------------------------------------------ ffprobe

/// What one `ffprobe` invocation produced.
#[derive(Debug)]
struct Probed {
    stream: serde_json::Value,
    format: serde_json::Value,
    /// One record per frame libavformat pulled out of the stream, in file order.
    frames: Vec<serde_json::Value>,
}

/// Run `ffprobe` once and keep everything three arms need out of one process.
///
/// One invocation rather than three, and it is not only about time: three runs of a program over
/// a file that is being written by nothing are three chances for the answers to disagree for a
/// reason that is not the file's. The *arms* stay separate — each one reads its own part of this
/// and goes red on its own — which is the property that matters.
fn run_ffprobe(binary: &Utf8Path, file: &Utf8Path, report: &mut OracleReport) -> Option<Probed> {
    // `-v error` for the reason in the module doc: our healthy fixtures print an `EOI missing`
    // warning that is an ffmpeg quirk rather than a finding. `-count_frames` is what makes
    // `nb_read_frames` exist at all, and the `frame=` entries are what make the count checkable
    // against something other than a header field.
    let output = Command::new(binary.as_std_path())
        .args([
            "-v",
            "error",
            "-print_format",
            "json",
            "-select_streams",
            "v:0",
            "-count_frames",
            "-show_entries",
            // `chroma_location` is here because a header field this workspace *writes* has to
            // be readable back by somebody who is not us: `imaging::y4m` states a 4:2:0 chroma
            // siting on every raw recording and there is no `C` tag that declines to (note
            // **N200**), so "the tag we chose means the siting we meant" was a claim resting on
            // a table in a note rather than on anything that could go red. One token here is
            // what turns it into an arm.
            "stream=codec_name,width,height,r_frame_rate,nb_frames,nb_read_frames,chroma_location\
             :format=format_name,nb_streams\
             :frame=pkt_size,pts",
        ])
        .arg(file.as_std_path())
        .output();

    let output = match output {
        Ok(output) => output,
        Err(err) => {
            // Located and then unusable. Deliberately a failure and not a decline: this host
            // *has* the oracle, so calling it a skip is how a rung stops running while its
            // transcript still says it declined for want of a tool.
            report.failures.push(format!(
                "demux: {binary} was found on this host and would not run: {err}"
            ));
            return None;
        }
    };

    if !output.status.success() {
        report.failures.push(format!(
            "demux: ffprobe refused {file} (exit {}), so libavformat could not open it at all — \
             which is the one thing an exit code here does mean: {}",
            output
                .status
                .code()
                .map_or_else(|| "signal".to_owned(), |code| code.to_string()),
            diagnostic(&output.stderr)
        ));
        return None;
    }

    let parsed: serde_json::Value = match serde_json::from_slice(&output.stdout) {
        Ok(parsed) => parsed,
        Err(err) => {
            report.failures.push(format!(
                "demux: ffprobe accepted {file} and produced {} bytes this harness could not \
                 read as JSON: {err}",
                output.stdout.len()
            ));
            return None;
        }
    };

    let stream = parsed
        .get("streams")
        .and_then(|streams| streams.get(0))
        .cloned();
    let Some(stream) = stream else {
        report.failures.push(format!(
            "demux: ffprobe accepted {file} and reported no video stream in it"
        ));
        return None;
    };
    let frames = parsed
        .get("frames")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    Some(Probed {
        stream,
        format: parsed
            .get("format")
            .cloned()
            .unwrap_or(serde_json::Value::Null),
        frames,
    })
}

fn arm_demux(probed: &Probed, expected: &Expectation, report: &mut OracleReport) -> ArmOutcome {
    let container = Expectation::format_name(expected.container);
    require(
        report,
        OracleArm::Demux,
        text(&probed.format, "format_name") == Some(container),
        || {
            format!(
                "the file was written as {} and libavformat probed it as {:?}",
                expected.container,
                text(&probed.format, "format_name").unwrap_or("nothing")
            )
        },
    );
    require(
        report,
        OracleArm::Demux,
        number(&probed.format, "nb_streams") == Some(1),
        || {
            format!(
                "a recording carries one video stream and this file has {:?}",
                number(&probed.format, "nb_streams")
            )
        },
    );
    let codec = Expectation::codec_name(expected.container);
    require(
        report,
        OracleArm::Demux,
        text(&probed.stream, "codec_name") == Some(codec),
        || {
            format!(
                "a {} recording's payload is {codec} and libavcodec named it {:?}",
                expected.container,
                text(&probed.stream, "codec_name").unwrap_or("nothing")
            )
        },
    );
    require(
        report,
        OracleArm::Demux,
        number(&probed.stream, "width") == Some(u64::from(expected.width))
            && number(&probed.stream, "height") == Some(u64::from(expected.height)),
        || {
            format!(
                "the stream was negotiated at {}x{} and the file says {:?}x{:?}",
                expected.width,
                expected.height,
                number(&probed.stream, "width"),
                number(&probed.stream, "height")
            )
        },
    );
    // The one header field this workspace *decides* rather than measures, read back by
    // somebody who is not us. Only asserted where the caller says a siting was stated: a Y4M
    // `Cmono` or `C422` file names none, and an AVI's sampling lives inside the MJPEG
    // bitstream — so a claim made about every file would be a claim about ffprobe's default
    // rather than about anything this build wrote (note **N210**).
    if let Some(siting) = expected.chroma_siting {
        let wanted = Expectation::chroma_location(siting);
        require(
            report,
            OracleArm::Demux,
            text(&probed.stream, "chroma_location") == Some(wanted),
            || {
                format!(
                    "the recording was opened at {siting:?}, which is written as a header tag \
                     meaning {wanted:?}, and libavformat read the siting back as {:?}",
                    text(&probed.stream, "chroma_location").unwrap_or("nothing")
                )
            },
        );
    }
    ArmOutcome::Ran
}

fn arm_frames(probed: &Probed, expected: &Expectation, report: &mut OracleReport) -> ArmOutcome {
    // The count libavformat produced by *pulling frames out*, which is the only one of the three
    // numbers here that is not a claim somebody wrote into a header.
    let read = probed.frames.len();
    require(
        report,
        OracleArm::Frames,
        u64::try_from(read).ok() == Some(u64::from(expected.frames)),
        || {
            format!(
                "the summary says this file holds {} frame(s) and libavformat pulled {read} out \
                 of it",
                expected.frames
            )
        },
    );

    // `nb_frames` is `avih.dwTotalFrames` for AVI and absent for Y4M, so it is checked when it
    // is there and never demanded: a header that claims more frames than the container holds is
    // the shape a crashed take leaves, and it is exactly what an exit code will not tell you.
    if let Some(claimed) = number(&probed.stream, "nb_frames") {
        require(
            report,
            OracleArm::Frames,
            u64::try_from(read).ok() == Some(claimed),
            || {
                format!(
                    "the file's own header claims {claimed} frame(s) and libavformat pulled \
                     {read} out of it; a header that outruns its container is what a crash \
                     leaves behind"
                )
            },
        );
    }
    // ffprobe's two counts, against each other. Absent when it read nothing, which the count
    // above has already reported against.
    if let Some(counted) = number(&probed.stream, "nb_read_frames") {
        require(
            report,
            OracleArm::Frames,
            u64::try_from(read).ok() == Some(counted),
            || format!("ffprobe counted {counted} frame(s) and listed {read}"),
        );
    }

    let sizes: Vec<Option<u64>> = probed
        .frames
        .iter()
        .map(|frame| number(frame, "pkt_size"))
        .collect();
    for (index, size) in sizes.iter().enumerate() {
        require(
            report,
            OracleArm::Frames,
            size.is_some_and(|size| size > 0),
            || format!("frame {index} arrived with a packet size of {size:?}"),
        );
    }
    if let Some(payloads) = &expected.payload_bytes {
        require(
            report,
            OracleArm::Frames,
            payloads.len() == sizes.len(),
            || {
                format!(
                    "the caller named {} payload length(s) and the file yielded {}",
                    payloads.len(),
                    sizes.len()
                )
            },
        );
        for (index, (ours, theirs)) in payloads.iter().zip(&sizes).enumerate() {
            require(
                report,
                OracleArm::Frames,
                *theirs == Some(u64::from(*ours)),
                || {
                    format!(
                        "frame {index} was written with a {ours}-byte payload and libavformat \
                         read {theirs:?} — a short frame here is a file cut mid-write, which no \
                         exit code and no frame count reports"
                    )
                },
            );
        }
    }

    // Presentation timestamps advance. Our containers are CFR, so these are frame indices in a
    // time base that is the mean interval (see the module doc's scope limit) — a repeated or
    // backwards one is a muxer that wrote a frame twice or an index that lost its order.
    let mut previous: Option<i64> = None;
    for (index, frame) in probed.frames.iter().enumerate() {
        let pts = frame.get("pts").and_then(serde_json::Value::as_i64);
        require(
            report,
            OracleArm::Frames,
            pts.is_some_and(|pts| previous.is_none_or(|last| pts > last)),
            || format!("frame {index} has pts {pts:?}, which does not follow {previous:?}"),
        );
        if let Some(pts) = pts {
            previous = Some(pts);
        }
    }
    ArmOutcome::Ran
}

fn arm_rate(probed: &Probed, expected: &Expectation, report: &mut OracleReport) -> ArmOutcome {
    let Some(rate) = text(&probed.stream, "r_frame_rate") else {
        report.failures.push(format!(
            "{}: the file declares no frame rate at all",
            OracleArm::Rate
        ));
        return ArmOutcome::Ran;
    };
    let parsed = rate
        .split_once('/')
        .and_then(|(num, den)| Some((num.parse::<u128>().ok()?, den.parse::<u128>().ok()?)));
    let Some((num, den)) = parsed else {
        report.failures.push(format!(
            "{}: libavformat read the rate as {rate:?}, which is not a rational",
            OracleArm::Rate
        ));
        return ArmOutcome::Ran;
    };

    // Cross-multiplied rather than divided: `1000000/66666` is 15.0002 fps and no float this
    // harness computes is the number to compare against. `r_frame_rate` arrives reduced —
    // `1000000/50000` comes back as `20/1` — so the equality has to be between the two
    // rationals and not between their spellings.
    let ours = u128::from(expected.declared_interval_us);
    require(
        report,
        OracleArm::Rate,
        ours > 0 && num.checked_mul(ours) == den.checked_mul(1_000_000),
        || {
            format!(
                "the summary says this file declares a {ours} µs interval — {} fps — and \
                 libavformat reads its rate field as {rate}",
                1_000_000_u128.checked_div(ours).unwrap_or(0)
            )
        },
    );
    ArmOutcome::Ran
}

// ---------------------------------------------------------------------------- mpv

fn arm_playback(
    binary: &Utf8Path,
    file: &Utf8Path,
    expected: &Expectation,
    report: &mut OracleReport,
) -> ArmOutcome {
    if expected.frames == 0 {
        // A recording of no frames is a legitimate file in both containers — `begin` writes a
        // complete header, which is what makes a crashed take recoverable — and there is
        // nothing in it to play. Refusing to ask is the honest answer; asking and accepting
        // whatever came back would make this arm green for a file with frames missing, which is
        // the one thing it is here to catch.
        return ArmOutcome::skipped(
            "the summary says this take wrote no frames, and a file with nothing in it has no \
             end-to-end playback to claim",
        );
    }

    // `--no-config` so a user's `mpv.conf` cannot change a verdict; `--vo=null --ao=null` so
    // nothing is drawn or played on the machine running the rung; `--untimed` so the run costs
    // decode time rather than the recording's own duration; `--msg-level=all=error` so a
    // diagnostic reaches us and a status line does not.
    let output = Command::new(binary.as_std_path())
        .args([
            "--no-config",
            "--vo=null",
            "--ao=null",
            "--untimed",
            "--msg-level=all=error",
        ])
        .arg(file.as_std_path())
        .output();

    let output = match output {
        Ok(output) => output,
        Err(err) => {
            report.failures.push(format!(
                "{}: {binary} was found on this host and would not run: {err}",
                OracleArm::Playback
            ));
            return ArmOutcome::Ran;
        }
    };
    require(report, OracleArm::Playback, output.status.success(), || {
        format!(
            "mpv could not play {} of {} to the end (exit {}): {}",
            expected.frames,
            expected.container,
            output
                .status
                .code()
                .map_or_else(|| "signal".to_owned(), |code| code.to_string()),
            diagnostic(&output.stderr)
        )
    });
    ArmOutcome::Ran
}

// ------------------------------------------------------------------------ helpers

/// Record `message` against `arm` unless `ok`.
///
/// [`crate::battery::ArmLog::require`]'s shape, prefixing with the arm's name here so no arm can
/// forget to say which one it is — and so [`OracleReport::failures_for`] can find it.
fn require(report: &mut OracleReport, arm: OracleArm, ok: bool, message: impl FnOnce() -> String) {
    if !ok {
        report.failures.push(format!("{arm}: {}", message().trim()));
    }
}

fn text<'a>(value: &'a serde_json::Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(serde_json::Value::as_str)
}

/// A JSON field ffprobe writes as a **string of digits**, read as a number.
///
/// `-print_format json` quotes `nb_frames`, `nb_read_frames` and `pkt_size` and leaves `width`,
/// `height` and `nb_streams` bare, which is a thing about ffprobe rather than about JSON — so
/// both spellings are accepted here and a field's type is not a claim this harness makes.
fn number(value: &serde_json::Value, key: &str) -> Option<u64> {
    let field = value.get(key)?;
    match field {
        serde_json::Value::Number(number) => number.as_u64(),
        serde_json::Value::String(text) => text.parse().ok(),
        _ => None,
    }
}

/// A tool's own words about why it refused, bounded and on one line.
fn diagnostic(stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    let first = text
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("");
    let mut clipped: String = first.chars().take(DIAGNOSTIC_BYTES).collect();
    if first.chars().count() > DIAGNOSTIC_BYTES {
        clipped.push('…');
    }
    if clipped.is_empty() {
        "it said nothing".to_owned()
    } else {
        clipped
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;
    use std::time::Duration;

    use imaging::video::{FrameOutcome, Recorder, RecordingCaps, RecordingParams};
    use schema::camera::PixelFormat;
    use schema::capture::Frame;

    use super::*;

    const WIDTH: u32 = 64;
    const HEIGHT: u32 = 48;
    /// 20 fps as an interval, and deliberately not the muxers' provisional 33 333: a container
    /// that dropped what it measured and wrote the placeholder would otherwise satisfy the rate
    /// arm below.
    const INTERVAL_US: i64 = 50_000;

    /// Where the damaged files go: `engine::paths::scratch_dir`, which is the one home a
    /// temporary file has in this workspace since the 2026-08-12 ruling (note **N84**), and
    /// **not** `tempfile::tempdir()`, which `scripts/gates/scratch-has-one-home.sh` bans.
    fn scratch() -> tempfile::TempDir {
        engine::paths::scratch_dir().expect("a scratch directory")
    }

    /// A JPEG whose length this test knows, because the payload arm's whole subject is lengths.
    ///
    /// The square size varies with the seed so **no two frames in a take are byte-identical**,
    /// which `a_frame_cut_in_half_fails_even_though_every_count_still_agrees` depends on: it
    /// finds the last frame's payload in the finished file by looking for those bytes, and a
    /// repeated frame would make the search ambiguous.
    fn jpeg(seed: u32) -> Vec<u8> {
        let image = imaging::fixtures::checkerboard(WIDTH, HEIGHT, 4 + seed);
        imaging::encode::jpeg(&imaging::Decoded::Gray(image), 30).expect("encodes")
    }

    fn mjpg_frames(count: u32) -> Vec<Frame> {
        (0..count)
            .map(|index| Frame {
                bytes: jpeg(index),
                pixel_format: PixelFormat::MJPG,
                width: WIDTH,
                height: HEIGHT,
                bytes_per_line: 0,
                sequence: index,
                timestamp_us: i64::from(index) * INTERVAL_US,
            })
            .collect()
    }

    fn grey_frames(count: u32) -> Vec<Frame> {
        let source = imaging::fixtures::gradient(WIDTH, HEIGHT);
        let bytes = imaging::fixtures::pack_grey(&source);
        (0..count)
            .map(|index| Frame {
                bytes: bytes.clone(),
                pixel_format: PixelFormat::GREY,
                width: WIDTH,
                height: HEIGHT,
                bytes_per_line: 0,
                sequence: index,
                timestamp_us: i64::from(index) * INTERVAL_US,
            })
            .collect()
    }

    /// The only frames in this suite whose container has a chroma siting to state.
    ///
    /// NV12 is the 4:2:0 member of D6's raw set, so it is the one that makes `imaging::y4m`
    /// write a `C420…` tag — which is the tag nothing outside this workspace had ever been
    /// asked about (note **N210**). GREY writes `Cmono` and YUYV writes `C422`, and `ffprobe`
    /// answers `unspecified` for both.
    fn nv12_frames(count: u32) -> Vec<Frame> {
        let source = imaging::fixtures::gradient(WIDTH, HEIGHT);
        let bytes = imaging::fixtures::pack_nv12(&source);
        (0..count)
            .map(|index| Frame {
                bytes: bytes.clone(),
                pixel_format: PixelFormat::NV12,
                width: WIDTH,
                height: HEIGHT,
                bytes_per_line: 0,
                sequence: index,
                timestamp_us: i64::from(index) * INTERVAL_US,
            })
            .collect()
    }

    /// Write one recording to a real path and answer what it turned out to be.
    ///
    /// A file rather than a buffer because both oracles are separate processes that take a path
    /// — which is the whole point of them and also the reason this module cannot be tested
    /// against an in-memory sink the way the muxers are.
    fn record(dir: &tempfile::TempDir, name: &str, frames: &[Frame]) -> (Utf8PathBuf, Expectation) {
        // The pixel format of a take with no frames in it is still the *stream's*, which is why
        // it is a parameter of the recording rather than a property of its first frame.
        record_in(
            dir,
            name,
            PixelFormat::MJPG,
            frames,
            ChromaSiting::default(),
        )
    }

    fn record_in(
        dir: &tempfile::TempDir,
        name: &str,
        pixel_format: PixelFormat,
        frames: &[Frame],
        chroma_siting: ChromaSiting,
    ) -> (Utf8PathBuf, Expectation) {
        let pixel_format = frames
            .first()
            .map_or(pixel_format, |frame| frame.pixel_format);
        let container = VideoFormat::for_pixel_format(pixel_format)
            .expect("D7 carries every format this suite writes");
        let path = Utf8PathBuf::from_path_buf(dir.path().join(name)).expect("utf-8 scratch");
        let mut bytes = Vec::new();
        let mut recorder = Recorder::begin(
            container,
            Cursor::new(&mut bytes),
            RecordingParams {
                width: WIDTH,
                height: HEIGHT,
                pixel_format,
                negotiated_interval_us: Some(u32::try_from(INTERVAL_US).expect("fits")),
                chroma_siting,
                caps: RecordingCaps {
                    max_bytes: 1 << 22,
                    max_frames: 1_024,
                    max_span: Duration::from_secs(60),
                },
            },
        )
        .expect("begin");
        for frame in frames {
            assert_eq!(
                recorder.write_frame(frame).expect("write_frame"),
                FrameOutcome::Written
            );
        }
        let summary = recorder.finish().expect("finish");
        std::fs::write(path.as_std_path(), &bytes).expect("write");

        let mut expected = Expectation::for_take(container, WIDTH, HEIGHT, &summary);
        if container == VideoFormat::Avi && !frames.is_empty() {
            expected = expected.with_payload_bytes(
                frames
                    .iter()
                    .map(|frame| u32::try_from(frame.bytes.len()).expect("fits"))
                    .collect(),
            );
        }
        // Which recordings state a siting: the raw container's 4:2:0 arm and no other. AVI's
        // sampling is inside the MJPEG bitstream, and `Cmono` and `C422` name none — so the
        // expectation carries a siting exactly where the file does, and a claim about the other
        // five would be a claim about ffprobe's default (note **N210**).
        if container == VideoFormat::Y4m && pixel_format == PixelFormat::NV12 {
            expected = expected.with_chroma_siting(chroma_siting);
        }
        (path, expected)
    }

    /// Whether this host can answer at all, printing the counted decline when it cannot.
    ///
    /// The rung's own accounting, in the module the rung is about: a test that quietly returned
    /// on a host with no oracles would be "skip == pass in a costume" (docs/8 Part C), and
    /// `scripts/rung-oracles.sh` reads exactly these lines.
    fn both_oracles_or_decline(what: &str) -> bool {
        let missing: Vec<Oracle> = Oracle::ALL
            .iter()
            .copied()
            .filter(|oracle| OnPath.locate(*oracle).is_none())
            .collect();
        for oracle in &missing {
            println!(
                "SKIP oracles: {oracle} is not on this host, so {what} was not checked — \
                 nothing here says {}; remedy: {}",
                oracle.answers(),
                oracle.install_hint()
            );
        }
        missing.is_empty()
    }

    #[test]
    fn both_containers_are_what_two_third_parties_say_they_are() {
        // The green arm, and the one that makes every red arm below mean something: a harness
        // that failed on healthy files would prove nothing by failing on damaged ones.
        if !both_oracles_or_decline("either container's healthy output") {
            return;
        }
        let dir = scratch();
        for (name, frames) in [
            ("healthy.avi", mjpg_frames(4)),
            ("healthy.y4m", grey_frames(4)),
        ] {
            let (path, expected) = record(&dir, name, &frames);
            let report = validate(&path, &expected);
            report.print();
            assert!(report.is_green(), "{name}: {report}");
            assert_eq!(report.ran(), OracleArm::ALL.len(), "{name}: {report}");
            assert!(report.absent.is_empty(), "{name}: {report}");
        }
    }

    #[test]
    fn the_chroma_siting_a_raw_recording_states_is_the_one_a_third_party_reads_back() {
        // **The claim that had no reader.** `imaging::y4m` states a 4:2:0 chroma siting on
        // every raw recording — there is no `C` tag that declines to, which is the whole of
        // note **N200** — and until this arm existed the only thing asserting what the tag
        // *meant* was a unit test comparing this workspace's string against this workspace's
        // expectation. That is the move that produced the defect in the first place: a tag
        // believed to say nothing, with no reader consulted. N200's table is a measurement
        // somebody took once by hand; this is the same measurement in a check that can go red
        // (the G6 review's finding 4 against B8; note **N210**).
        //
        // Both members of the vocabulary, because a single arm would be satisfied by a writer
        // that ignored its input and happened to emit the one tag under test — which is
        // exactly what the pre-repair writer did.
        if !both_oracles_or_decline("what a reader makes of the 4:2:0 siting we write") {
            return;
        }
        let dir = scratch();
        for (name, siting) in [
            ("centred.y4m", ChromaSiting::Centred),
            ("cosited.y4m", ChromaSiting::HorizontallyCosited),
        ] {
            let (path, expected) =
                record_in(&dir, name, PixelFormat::NV12, &nv12_frames(4), siting);
            assert_eq!(
                expected.chroma_siting,
                Some(siting),
                "{name}: a 4:2:0 raw take that carries no siting to check is not this arm"
            );
            let report = validate(&path, &expected);
            report.print();
            assert!(report.is_green(), "{name}: {report}");
            assert_eq!(report.ran(), OracleArm::ALL.len(), "{name}: {report}");
        }

        // The other direction, so the arm above is about the siting rather than about a claim
        // that fires on anything: a colorspace with no siting to state carries none, and the
        // demux arm asks nothing about it. Without this the two arms above would be satisfied
        // by an `Expectation` that always said `Centred` and a probe that always said
        // `center`.
        let (path, expected) = record(&dir, "mono.y4m", &grey_frames(2));
        assert_eq!(
            expected.chroma_siting, None,
            "Cmono names no siting, so nothing here may claim one"
        );
        let report = validate(&path, &expected);
        report.print();
        assert!(report.is_green(), "mono.y4m: {report}");
    }

    #[test]
    fn a_file_libavformat_cannot_open_at_all_is_a_failure_and_not_a_decline() {
        // The FAILURE row of the module doc's exit-code table. Bytes that probe as nothing make
        // ffprobe exit 1, and a harness that read a non-zero exit as "the oracle was not
        // available" would turn every broken file into a green run with a skip line.
        if !both_oracles_or_decline("a file ffprobe refuses") {
            return;
        }
        let dir = scratch();
        let (_, expected) = record(&dir, "reference.avi", &mjpg_frames(2));
        let path = Utf8PathBuf::from_path_buf(dir.path().join("garbage.avi")).expect("utf-8");
        std::fs::write(path.as_std_path(), [0x5au8; 512]).expect("write");

        let report = validate(&path, &expected);
        assert!(!report.is_green(), "a refused file passed: {report}");
        assert!(
            report.absent.is_empty(),
            "a refused file was reported as a missing oracle: {report}"
        );
        // The failure has to say the file was **refused**, and this is the assertion that makes
        // the test about the exit code at all. A hand-applied mutant that stopped reading the
        // exit status survived a version of this test that only asked for *a* demux failure:
        // ffprobe writes `{}` on exit 1, the JSON has no `streams`, and the harness answered
        // "ffprobe accepted this file and reported no video stream in it" — which is a
        // different finding sent to a reader who would go looking for a muxer bug. AGENTS rule
        // 7 is the line between "the tool refused" and "the tool answered nothing".
        let demux = report.failures_for(OracleArm::Demux).join(" | ");
        assert!(
            demux.contains("ffprobe refused"),
            "the refusal was reported as something else: {report}"
        );
        assert!(
            demux.contains("exit 1"),
            "the refusal does not name the status that meant it: {report}"
        );
        assert!(
            demux.contains("Invalid data"),
            "the refusal does not carry ffprobe's own words: {report}"
        );
        // And mpv refuses it too, on its own arm, from its own process — the one claim the two
        // programs make separately.
        assert!(
            !report.failures_for(OracleArm::Playback).is_empty(),
            "{report}"
        );
    }

    #[test]
    fn a_file_with_no_frames_in_it_fails_however_many_its_header_claims() {
        // The ACCEPTED row, and the reason the exit code is not the oracle: a header list with
        // nothing behind it is exit 0, and ffprobe reports `nb_frames` straight out of
        // `avih.dwTotalFrames` — so a harness that read either would be green over a recording
        // that captured nothing.
        //
        // Both halves of the file are ours: the zero-frame take is what `begin` writes, and the
        // expectation is the four-frame take's. That is a recording reported as four frames
        // whose file holds none, which is what a take that died in its first turn leaves.
        if !both_oracles_or_decline("a file holding no frames") {
            return;
        }
        let dir = scratch();
        let (_, four) = record(&dir, "four.avi", &mjpg_frames(4));
        let (empty, none) = record(&dir, "none.avi", &[]);
        assert_eq!(none.frames, 0);

        // Honest about itself first: a zero-frame file *matching* a zero-frame claim is a legal
        // file and the harness says so, declining only the playback arm and naming why.
        let honest = validate(&empty, &none);
        honest.print();
        assert!(honest.is_green(), "{honest}");
        assert!(
            matches!(
                honest.outcome(OracleArm::Playback),
                Some(ArmOutcome::Skipped { .. })
            ),
            "{honest}"
        );

        // And then the claim that is false — asked **without** the payload lengths, which is
        // what a real capture looks like: nothing outside the muxer ever saw a camera frame's
        // length, so `Expectation::payload_bytes` is `None` on the whole R3 rung. A
        // hand-applied mutant that stopped comparing the frame count survived a version of this
        // test that supplied them, because the count of *payload lengths* disagreed instead and
        // covered for it. The blind expectation is the one that has only the count to go on.
        let blind = Expectation {
            payload_bytes: None,
            ..four.clone()
        };
        let report = validate(&empty, &blind);
        assert!(!report.is_green(), "an empty recording passed: {report}");
        let frames = report.failures_for(OracleArm::Frames).join(" | ");
        assert!(
            frames.contains("libavformat pulled 0 out of it"),
            "the count of frames actually demuxed is not what went red: {report}"
        );
        assert!(
            !report.failures_for(OracleArm::Playback).is_empty(),
            "mpv played a file with nothing in it: {report}"
        );

        // And with the lengths supplied as well, so the arm that fires first does not hide the
        // one this test is named for.
        let told = validate(&empty, &four);
        assert!(!told.is_green(), "{told}");
        assert!(
            told.failures_for(OracleArm::Frames)
                .join(" | ")
                .contains("libavformat pulled 0 out of it"),
            "{told}"
        );
    }

    #[test]
    fn a_frame_cut_in_half_fails_even_though_every_count_still_agrees() {
        // The measurement that decides this harness's whole shape. A finished AVI truncated
        // inside its last frame is **exit 0**, its header still claims every frame, and
        // `-count_frames` still counts them all — because a truncated JPEG decodes, with
        // `overread 8` as a diagnostic rather than an error. The only place the damage is
        // visible is `pkt_size`, which is why `Expectation::with_payload_bytes` exists.
        if !both_oracles_or_decline("a file cut inside its last frame") {
            return;
        }
        let dir = scratch();
        let frames = mjpg_frames(3);
        let (path, expected) = record(&dir, "cut.avi", &frames);
        let whole = std::fs::read(path.as_std_path()).expect("read");

        // Cut the tail off the last frame's payload, and the `idx1` behind it with it. The
        // offset is **found** rather than typed: the last frame's bytes are looked for in the
        // finished file, so a literal cannot stop being the middle of a frame the day the
        // fixture's JPEG changes size. An eighth is taken rather than a half deliberately —
        // measured on the P6d host, a JPEG cut to under half its length stops decoding at all
        // and libavformat then emits no frame record, which is the *count* going wrong and not
        // the thing this test is about.
        let last = frames.last().expect("three frames").bytes.as_slice();
        let at = whole
            .windows(last.len())
            .position(|window| window == last)
            .expect("the last frame's payload is in the file it was written to");
        let cut_at = at + last.len() - (last.len() / 8);
        let cut = Utf8PathBuf::from_path_buf(dir.path().join("halved.avi")).expect("utf-8");
        std::fs::write(cut.as_std_path(), &whole[..cut_at]).expect("write");

        let report = validate(&cut, &expected);
        assert!(!report.is_green(), "a cut frame passed: {report}");
        let found = report.failures_for(OracleArm::Frames).join(" | ");
        assert!(
            found.contains("byte payload"),
            "the failure is not the short payload: {report}"
        );

        // The non-vacuity that makes the sentence above true rather than lucky: the same file
        // with the payload lengths withheld is **green**, because nothing else in the JSON
        // knows the frame is short. This is the assertion that a harness checking exit codes,
        // frame counts or `nb_frames` would pass on.
        let blind = Expectation {
            payload_bytes: None,
            ..expected.clone()
        };
        let unnoticed = validate(&cut, &blind);
        assert!(
            unnoticed.is_green(),
            "something other than pkt_size saw the cut, so this test is not about what it says \
             it is about: {unnoticed}"
        );
    }

    #[test]
    fn a_rate_the_file_does_not_declare_is_a_failure_in_either_container() {
        // The arm note **N106**'s amendment turns on: the padded `F` denominator and AVI's two
        // binary homes are both read back here, and a container that declared one rate while
        // its summary claimed another is the defect this catches. Driven by *lying to the
        // harness* rather than by damaging the file, because a muxer that wrote the wrong rate
        // would produce exactly this disagreement.
        if !both_oracles_or_decline("a mis-declared frame rate") {
            return;
        }
        let dir = scratch();
        for (name, frames) in [("rate.avi", mjpg_frames(3)), ("rate.y4m", grey_frames(3))] {
            let (path, expected) = record(&dir, name, &frames);
            assert!(validate(&path, &expected).is_green(), "{name}");

            let lied = Expectation {
                declared_interval_us: expected.declared_interval_us + 1,
                ..expected.clone()
            };
            let report = validate(&path, &lied);
            assert!(
                !report.failures_for(OracleArm::Rate).is_empty(),
                "{name}: a rate one microsecond out went unnoticed: {report}"
            );
        }
    }

    #[test]
    fn a_geometry_or_a_container_the_file_does_not_have_is_a_failure() {
        // The demux arm, driven the same way. "It demuxed as something" is not the claim: a Y4M
        // that probed as an AVI, or a 64x48 stream that arrived 32x24, would each be a file one
        // of our own readers is wrong about.
        if !both_oracles_or_decline("a mis-declared container or geometry") {
            return;
        }
        let dir = scratch();
        let (path, expected) = record(&dir, "shape.y4m", &grey_frames(2));
        for wrong in [
            Expectation {
                container: VideoFormat::Avi,
                ..expected.clone()
            },
            Expectation {
                width: WIDTH / 2,
                ..expected.clone()
            },
            Expectation {
                height: HEIGHT + 2,
                ..expected.clone()
            },
        ] {
            let report = validate(&path, &wrong);
            assert!(
                !report.failures_for(OracleArm::Demux).is_empty(),
                "{wrong:?} went unnoticed: {report}"
            );
        }
    }

    #[test]
    fn a_recording_that_was_reported_and_never_written_is_named_rather_than_probed() {
        // Before either tool: "there is nothing at that path" and "ffprobe could not open it"
        // are different findings, and only one of them is about a muxer. A harness that handed
        // a missing path to ffprobe would report the muxer's file as invalid data.
        let dir = scratch();
        let (_, expected) = record(&dir, "reference.avi", &mjpg_frames(2));
        let missing =
            Utf8PathBuf::from_path_buf(dir.path().join("never-written.avi")).expect("utf-8");
        let report = validate(&missing, &expected);
        assert!(!report.is_green(), "{report}");
        assert!(
            report.failures.iter().any(|f| f.contains("is not a file")),
            "{report}"
        );
        assert!(report.absent.is_empty(), "{report}");
        assert_eq!(report.ran(), 0, "{report}");
    }

    #[test]
    fn every_host_the_fault_menu_names_declines_by_name_rather_than_passing() {
        // The decline path, exercised against a host this test invents — which is the only way
        // it can be exercised at all, since **both oracles are installed here**. A decline
        // nobody has ever watched fire is a decline nobody can trust, and
        // `crates/daemon/tests/web_browser.rs` reaches its own precondition arms the same way.
        //
        // The walk is over `HostFault::ALL`, so a fault added to the menu has to be answered
        // rather than inherited, and each arm asserts three things: the report is **green**
        // (nothing was claimed, so nothing is wrong), the missing oracle is **named** in
        // `absent`, and its arms did **not** run. A decline that left an arm marked `Ran` would
        // be the skip-reads-as-pass defect with the accounting still saying it declined.
        if !both_oracles_or_decline("the fabricated-host decline arms") {
            return;
        }
        let dir = scratch();
        let (path, expected) = record(&dir, "declined.avi", &mjpg_frames(2));

        for &fault in HostFault::ALL {
            let host = ScriptedTools::new(fault);
            let report = validate_with(&host, &path, &expected);
            // Deliberately **not** `report.print()`: these absences are invented, and a
            // fabricated `SKIP oracles:` line in the run log is read by
            // `scripts/rung-oracles.sh` as this host lacking a tool it has. The lines are
            // asserted instead, which is a stronger claim than printing them anyway. Since
            // note **N235** the discipline is the report's rather than this comment's, and
            // `a_decline_a_double_invented_cannot_be_printed_into_a_counted_log` is what
            // holds it for the arm that comes after this one.
            assert!(!report.measured_on_this_host, "{fault:?}: {report}");
            assert!(report.is_green(), "{fault:?}: {report}");

            let hidden: Vec<Oracle> = Oracle::ALL
                .iter()
                .copied()
                .filter(|oracle| host.locate(*oracle).is_none())
                .collect();
            assert!(!hidden.is_empty(), "{fault:?} hides nothing");
            assert_eq!(
                report.absent.keys().copied().collect::<Vec<_>>(),
                hidden,
                "{fault:?}: {report}"
            );
            for oracle in &hidden {
                for arm in OracleReport::arms_of(*oracle) {
                    assert!(
                        matches!(report.outcome(arm), Some(ArmOutcome::Skipped { .. })),
                        "{fault:?}: {arm} ran without {oracle}: {report}"
                    );
                }
            }
            // And whatever the fault left behind still answered, so the double removes an
            // oracle rather than disabling the harness.
            let expected_ran = OracleArm::ALL
                .iter()
                .filter(|arm| !hidden.contains(&arm.oracle()))
                .count();
            assert_eq!(report.ran(), expected_ran, "{fault:?}: {report}");

            // The sentence the runner would count, asserted where it can be read rather than
            // where it would be mistaken for this host's own answer. Three things per line,
            // because a decline that cannot say what it did not do is the skip that reads as a
            // pass (docs/8 Part C).
            let lines = report.decline_lines();
            assert_eq!(lines.len(), hidden.len(), "{fault:?}: {lines:?}");
            for (oracle, line) in hidden.iter().zip(&lines) {
                assert!(line.starts_with("SKIP oracles: "), "{line}");
                assert!(line.contains(oracle.as_str()), "{line}");
                assert!(line.contains(oracle.answers()), "{line}");
                assert!(line.contains(oracle.install_hint()), "{line}");
            }
            assert_eq!(
                report.run_line().is_some(),
                expected_ran > 0,
                "{fault:?}: a run line and a run disagree"
            );
        }
    }

    #[test]
    #[should_panic(expected = "invented by a Tools double")]
    fn a_decline_a_double_invented_cannot_be_printed_into_a_counted_log() {
        // **Note N235**, and the inverse of the arm above. That arm keeps its invented declines
        // out of the run log by *not calling* `print()`, under a comment saying why — which is
        // the discipline N121 asked for held by a convention rather than by the code. Two
        // censuses read these lines and neither can tell a real absence from an invented one:
        // `scripts/smoke-hw.sh:138` greps `^\s*SKIP` out of a saved run log (and
        // `$WCH_SMOKE_HW_ACCOUNT` re-reads any log, including a whole `just ci`), and
        // `scripts/rung-oracles.sh` greps a narrower anchor out of its own. So the next arm
        // written against a `HostFault` gets a panic instead of a green run with a false
        // absence in it.
        //
        // No `both_oracles_or_decline` guard, deliberately: `HostFault::NoOracles` hides both
        // whatever this host has, so the report carries two invented absences on every machine
        // and the property is reachable everywhere. A guard here would be a red arm that stops
        // being reachable on the hosts that need it most.
        let dir = scratch();
        let (path, expected) = record(&dir, "invented.avi", &mjpg_frames(2));
        let report = validate_with(&ScriptedTools::new(HostFault::NoOracles), &path, &expected);
        assert_eq!(report.absent.len(), Oracle::ALL.len(), "{report}");
        report.print();
    }

    #[test]
    fn a_report_with_nothing_absent_prints_whatever_asked_for_it() {
        // The other direction, so the refusal above is about a *fabricated absence* and not
        // about doubles: a report that declines nothing has nothing to fake, so `print()` is
        // free whoever built it. Without this arm the assertion could tighten to "no double may
        // print" and nothing would notice.
        let dir = scratch();
        let (path, expected) = record(&dir, "nothing-absent.avi", &mjpg_frames(2));
        let mut report = validate_with(&ScriptedTools::new(HostFault::NoOracles), &path, &expected);
        report.absent.clear();
        assert!(!report.measured_on_this_host, "{report}");
        report.print();
    }

    #[test]
    fn every_arm_belongs_to_exactly_one_oracle_and_every_oracle_has_an_arm() {
        // The false-independence rule as a property rather than as a paragraph: an arm answered
        // by whichever program happened to be installed would make a decline unpriceable, and
        // an oracle with no arm is a program this rung runs for nothing.
        for &oracle in Oracle::ALL {
            assert!(
                !OracleReport::arms_of(oracle).is_empty(),
                "{oracle} answers no arm"
            );
            assert!(!oracle.answers().is_empty());
            assert!(!oracle.install_hint().is_empty());
        }
        let claimed: usize = Oracle::ALL
            .iter()
            .map(|oracle| OracleReport::arms_of(*oracle).len())
            .sum();
        assert_eq!(
            claimed,
            OracleArm::ALL.len(),
            "an arm is answered by two oracles, or by none"
        );
    }
}
