//! Who has the camera (design D13, note N8).
//!
//! `EBUSY` from `open` or from `STREAMON` says the node is taken and nothing else. That is
//! the *availability* answer E3 keeps separate from a capability one — but "busy" on its
//! own is a dead end for the person reading it, and the next thing they will do is run
//! `fuser /dev/video0`. This module is that command, built in, so the refusal can name the
//! process instead of describing the problem.
//!
//! ## Why a `/proc` walk and not a library
//!
//! There is no syscall for "who has this inode open". The kernel's answer lives in
//! `/proc/<pid>/fd/*`, as symlinks, and reading it is the whole of what `fuser` and `lsof`
//! do for this question. A dependency would be a link edge for thirty lines of `read_dir`.
//!
//! ## What it can and cannot see
//!
//! Only processes this user may look at. Another user's process holding the camera is
//! invisible without privilege, and so is one that exited between the `EBUSY` and this
//! walk. So an empty answer means *we could not identify anyone*, which is why
//! [`schema::Error::Busy`] renders an empty list as "an unidentified process" rather than
//! claiming anything about `/proc`.
//!
//! ## The deliberate duplication with `webcam-handler-priv`
//!
//! Note N8 records it and declines to merge it: `modules::video_holders` there asks "is
//! *any* camera in use", to decide whether unloading `uvcvideo` would pull the driver out
//! from under a video call. This asks "who holds *this* node" and answers in
//! [`schema::error::Holder`]. Merging them would drag the product's crate graph inside a
//! root-capable boundary, and thirty lines is the cheaper half of that trade.
//!
//! ## Why this module is public, and why the signal is beside the walk
//!
//! `terminate_holder` (design §5, D10) is diagnose-then-signal, and the diagnosis is here —
//! a second one would be a second answer to "who has this node", which is the defect §2.10
//! names. So the daemon reaches this module by name rather than growing a walk of its own;
//! the daemon already links this crate at its composition root, so no dependency edge moves
//! and the T1 trait keeps its five camera-shaped methods (note **N48** records the two
//! alternatives that lost).
//!
//! **Two questions, not one, and which one gates the signal matters.** [`of`] answers "who
//! has this node", bounded by [`limits::MAX_HOLDERS_REPORTED`] because a refusal listing
//! four hundred processes is less readable than one listing none — so it is the right
//! answer for an [`schema::Error::Busy`] refusal and the wrong one for "may I signal this
//! pid". [`holder`] is the second question, asked of the pid the caller named and unaffected
//! by how many others have the node. Gating on the capped walk is how a browser's fifth
//! process becomes unaddressable while it really does hold the camera (note **N48**).
//!
//! The signal is [`terminate`], a forward to the one `unsafe` block in `sys::signal`, and it is here
//! rather than at the caller because "who holds it" and "ask one of them to let go" are the
//! two halves of one verb: a caller that could reach the walk without reaching the signal
//! would be a caller free to invent its own second half.
//!
//! **What this module will not do.** It never chooses a pid. Every function here takes the
//! one the caller named, and the caller is the wire method whose whole contract is that it
//! names its target (AGENTS: "Killing a process that holds the camera is an explicit
//! command naming its target, never a fallback"). There is no "signal the holders of this
//! node" function, because that is the shape a fallback would take.

use camino::Utf8Path;
use schema::error::{Holder, Result};
use schema::limits;

/// The processes holding `node`, as far as this user can see.
///
/// Best effort by construction, and it must stay that way: this runs while an error is on
/// its way to the caller, and a walk that could itself fail would replace a useful refusal
/// with a less useful one. Every step that could fail is skipped rather than propagated.
///
/// **The under-reporting is load-bearing for `terminate_holder`.** An empty answer means
/// "we could not identify anyone", the cap at [`limits::MAX_HOLDERS_REPORTED`] truncates a
/// busy node, and another user's process is invisible — so a pid that genuinely holds the
/// node can be absent from this list. The verb built on it refuses in that case rather than
/// signalling a pid it could not confirm, which occasionally refuses a legitimate kill and
/// is the correct direction: the alternative is signalling a pid on the strength of a walk
/// that did not see it.
#[must_use]
pub fn of(node: &Utf8Path) -> Vec<Holder> {
    let mut holders = Vec::new();
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return holders;
    };

    for entry in entries.flatten() {
        // Bounded (rubric A14). `/proc` is as long as the process table, and a refusal
        // that listed four hundred processes would be less readable than one that listed
        // none — so the walk stops once it has enough to name.
        if holders.len() >= limits::MAX_HOLDERS_REPORTED {
            break;
        }
        let Some(pid_name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let Ok(pid) = pid_name.parse::<i32>() else {
            // Not a process directory. `/proc` holds plenty that is not.
            continue;
        };
        if has_open(&pid_name, node) {
            holders.push(Holder {
                pid,
                comm: comm_of(&pid_name),
            });
        }
    }
    holders
}

/// The one holder `pid` is, if it holds `node` — asked without walking the process table.
///
/// `terminate_holder`'s **gate**, and the reason it is this rather than a lookup in [`of`]'s
/// answer: [`of`] stops at [`limits::MAX_HOLDERS_REPORTED`], which is right for a refusal
/// somebody has to read and wrong for "does this pid hold it". A browser keeps several
/// processes on one node, so the fifth holder of `/dev/video0` is one the walk would not
/// mention — and gating the signal on the walk would answer [`schema::Error::HolderGone`]
/// for a pid that really does hold the device, leaving no pid the caller could name to free
/// the camera. That is the verb at its least usable in the one situation it exists for.
///
/// It is strictly *better* evidence than membership in the walk, not weaker: [`holds`] asks
/// the kernel about this pid's own descriptors rather than about the first four processes
/// `/proc` happened to list. Everything the walk under-reports for its other reasons still
/// refuses here — another user's process is unreadable, so this answers `None` and the verb
/// declines to signal a pid it could not confirm (note **N48**).
///
/// The [`Holder`] it builds is the one the report carries, in the same shape a
/// [`schema::Error::Busy`] refusal carries, so both verbs name a process the same way.
#[must_use]
pub fn holder(pid: i32, node: &Utf8Path) -> Option<Holder> {
    holds(pid, node).then(|| Holder {
        pid,
        comm: comm_of(&pid.to_string()),
    })
}

/// The command name behind a `/proc` entry, for the readable half of a [`Holder`].
///
/// `None` rather than a placeholder when it cannot be read — a process that exited between
/// the walk and this read is ordinary, and `Holder`'s own rendering is what decides how an
/// unnamed one is printed.
fn comm_of(pid_name: &str) -> Option<String> {
    std::fs::read_to_string(format!("/proc/{pid_name}/comm"))
        .ok()
        .map(|comm| comm.trim().to_owned())
}

/// Whether **this one process** has `node` open, asked without walking the process table.
///
/// The question `terminate_holder` asks twice: once through [`holder`] to decide whether to
/// signal at all, and once here immediately before it signals (design §5: "refuses if the
/// pid no longer holds the device"). Separate from [`of`] because the question is different:
/// [`of`] answers "who", which is bounded and may truncate, and this answers "does this
/// pid", which must not be affected by how many *other* processes have the node — a pid past
/// [`limits::MAX_HOLDERS_REPORTED`] in the walk's order is one the walk would not mention
/// and this still finds. [`holder`] is where that sentence is spent; a verb that gated on
/// [`of`] would make it a claim about a call nobody makes.
///
/// It is also the cheapest thing that can be run in the instruction or two before a signal,
/// which is the whole of what narrows the pid-reuse race that cannot be closed (note N48).
#[must_use]
pub fn holds(pid: i32, node: &Utf8Path) -> bool {
    pid > 0 && has_open(&pid.to_string(), node)
}

/// Ask `pid` to let go of the camera: `SIGTERM`, and nothing else, ever.
///
/// A forward to `sys::signal::term`, which is where the syscall's one `unsafe`
/// block lives. Nothing is verified here — the caller has already established that this pid
/// holds the node, and doing it again inside would put a second copy of the rule between
/// the check and the signal it is meant to be adjacent to.
///
/// # Errors
///
/// As `sys::signal::term`: [`schema::Error::HolderGone`] when the process exited
/// between the check and this call, [`schema::Error::PermissionDenied`] for a process this
/// uid may not signal, and [`schema::Error::IllegalTransition`] for a pid that names a
/// process group rather than a process.
pub fn terminate(pid: i32) -> Result<()> {
    crate::sys::signal::term(pid)
}

/// Whether the process whose `/proc` directory is `pid_name` has `node` open.
///
/// The comparison is on the symlink target rather than on an inode: `/proc/<pid>/fd/<n>`
/// resolves to the device path the process opened, which is the string a user recognises
/// and the string the error already carries. Two paths to one device node — a symlink in
/// `/dev/v4l/by-id/`, say — would compare unequal, and that is a miss rather than a false
/// positive: the walk under-reports rather than naming the wrong process.
fn has_open(pid_name: &str, node: &Utf8Path) -> bool {
    let Ok(fds) = std::fs::read_dir(format!("/proc/{pid_name}/fd")) else {
        // Another user's process, or one that exited while we were looking. Both are
        // ordinary, and neither is worth reporting as a failure of the walk.
        return false;
    };
    fds.flatten().any(|fd| {
        std::fs::read_link(fd.path()).is_ok_and(|target| target.as_os_str() == node.as_str())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_this_process_holds_open_names_this_process() {
        // The walk is exercised against a file this test opens itself, so it runs on any
        // host, camera or no camera — and it is the *positive* direction, which a walk
        // that always returned nothing would fail.
        let dir = engine::paths::scratch_dir().expect("a scratch directory");
        let path =
            camino::Utf8PathBuf::from_path_buf(dir.path().join("held")).expect("utf-8 temp dir");
        let held = std::fs::File::create(&path).expect("create");

        let holders = of(&path);
        let me = std::process::id();
        assert!(
            holders
                .iter()
                .any(|h| u32::try_from(h.pid).is_ok_and(|pid| pid == me)),
            "the walk did not find this process holding {path}: {holders:?}"
        );
        // The `comm` is the readable half; without it a refusal is a bare number.
        assert!(
            holders
                .iter()
                .any(|h| h.comm.as_deref().is_some_and(|c| !c.is_empty())),
            "no holder carried a command name: {holders:?}"
        );

        // The inverse, over the same walk: once the file is closed, nobody holds it.
        drop(held);
        assert!(
            of(&path).is_empty(),
            "a file nobody has open must name nobody"
        );
    }

    #[test]
    fn a_path_nothing_has_open_names_nobody_rather_than_guessing() {
        assert!(of(camino::Utf8Path::new("/dev/video-nothing-is-here")).is_empty());
    }

    #[test]
    fn asking_about_one_pid_answers_both_ways_without_walking_the_table() {
        // The re-verification `terminate_holder` runs in the instruction before it signals.
        // Both directions over the same file, and the negative one is what stops a verb
        // built on this from signalling a pid on the strength of an answer that is always
        // `true`.
        let dir = engine::paths::scratch_dir().expect("a scratch directory");
        let path =
            camino::Utf8PathBuf::from_path_buf(dir.path().join("held")).expect("utf-8 temp dir");
        let held = std::fs::File::create(&path).expect("create");
        let me = i32::try_from(std::process::id()).expect("a pid fits");

        assert!(holds(me, &path), "this process has {path} open");
        // A pid nothing answers to, and the two broadcast spellings `kill(2)` would read as
        // a group — none of which may ever be reported as holding anything, because the
        // caller signals what this says holds the node.
        assert!(!holds(i32::MAX, &path));
        for not_a_process in [0, -1] {
            assert!(!holds(not_a_process, &path), "{not_a_process}");
        }

        drop(held);
        assert!(
            !holds(me, &path),
            "the file is closed and still reads as held"
        );
    }

    #[test]
    fn the_walk_stops_at_the_reporting_cap() {
        // Rubric A14. The cap is the reason a refusal stays readable on a busy machine,
        // and a cap nothing enforces is a comment.
        let dir = engine::paths::scratch_dir().expect("a scratch directory");
        let path =
            camino::Utf8PathBuf::from_path_buf(dir.path().join("held")).expect("utf-8 temp dir");
        let _held = std::fs::File::create(&path).expect("create");
        assert!(of(&path).len() <= limits::MAX_HOLDERS_REPORTED);
    }

    #[test]
    fn a_holder_the_capped_walk_did_not_mention_is_still_found_by_name() {
        // The sentence [`holds`]'s doc has always made, now made about a call somebody
        // makes. The cap is [`of`]'s and it is right there; what it must not do is decide
        // whether a pid may be signalled, because a browser holds one node from several
        // processes and the fifth of them is a legitimate target the walk would not name.
        //
        // The subject is *this* process throughout, so no second process is needed to make
        // the point: the walk is capped and this pid may or may not be inside the cap, but
        // `holder` answers about this pid either way. What makes the arm non-vacuous is the
        // pairing — the same pid, the same node, one answer from a bounded walk and one
        // from a direct question.
        let dir = engine::paths::scratch_dir().expect("a scratch directory");
        let path =
            camino::Utf8PathBuf::from_path_buf(dir.path().join("held")).expect("utf-8 temp dir");
        let held = std::fs::File::create(&path).expect("create");
        let me = i32::try_from(std::process::id()).expect("a pid fits");

        let found = holder(me, &path).expect("this process has the node open");
        assert_eq!(found.pid, me);
        assert!(
            found.comm.as_deref().is_some_and(|comm| !comm.is_empty()),
            "the holder was named by a bare number: {found:?}"
        );
        // The same value the walk builds, so a report and a refusal name a process
        // identically however the answer was reached.
        assert!(of(&path).contains(&found), "{found:?}");

        // The refusing direction, which is what keeps this from being "yes" for everything:
        // a pid nothing answers to, and the two spellings `kill(2)` would read as a group.
        assert_eq!(holder(i32::MAX, &path), None);
        for not_a_process in [0, -1] {
            assert_eq!(holder(not_a_process, &path), None, "{not_a_process}");
        }

        drop(held);
        assert_eq!(
            holder(me, &path),
            None,
            "the file is closed and still reads as held"
        );
    }
}
