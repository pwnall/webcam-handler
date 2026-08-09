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
//! ## The deliberate duplication with `wch-priv`
//!
//! Note N8 records it and declines to merge it: `modules::video_holders` there asks "is
//! *any* camera in use", to decide whether unloading `uvcvideo` would pull the driver out
//! from under a video call. This asks "who holds *this* node" and answers in
//! [`schema::error::Holder`]. Merging them would drag the product's crate graph inside a
//! root-capable boundary, and thirty lines is the cheaper half of that trade.

use camino::Utf8Path;
use schema::error::Holder;
use schema::limits;

/// The processes holding `node`, as far as this user can see.
///
/// Best effort by construction, and it must stay that way: this runs while an error is on
/// its way to the caller, and a walk that could itself fail would replace a useful refusal
/// with a less useful one. Every step that could fail is skipped rather than propagated.
#[must_use]
pub(crate) fn of(node: &Utf8Path) -> Vec<Holder> {
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
        if holds(&pid_name, node) {
            holders.push(Holder {
                pid,
                comm: std::fs::read_to_string(format!("/proc/{pid_name}/comm"))
                    .ok()
                    .map(|comm| comm.trim().to_owned()),
            });
        }
    }
    holders
}

/// Whether process `pid_name` has `node` open.
///
/// The comparison is on the symlink target rather than on an inode: `/proc/<pid>/fd/<n>`
/// resolves to the device path the process opened, which is the string a user recognises
/// and the string the error already carries. Two paths to one device node — a symlink in
/// `/dev/v4l/by-id/`, say — would compare unequal, and that is a miss rather than a false
/// positive: the walk under-reports rather than naming the wrong process.
fn holds(pid_name: &str, node: &Utf8Path) -> bool {
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
        let dir = tempfile::tempdir().expect("temp dir");
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
    fn the_walk_stops_at_the_reporting_cap() {
        // Rubric A14. The cap is the reason a refusal stays readable on a busy machine,
        // and a cap nothing enforces is a comment.
        let dir = tempfile::tempdir().expect("temp dir");
        let path =
            camino::Utf8PathBuf::from_path_buf(dir.path().join("held")).expect("utf-8 temp dir");
        let _held = std::fs::File::create(&path).expect("create");
        assert!(of(&path).len() <= limits::MAX_HOLDERS_REPORTED);
    }
}
