//! The one door a file a caller named on a command line is read through.
//!
//! **A path off a command line is a caller-supplied number reaching an allocator.**
//! `std::fs::read` sizes its buffer from the length of the file the path names, so `photo diff`,
//! `profile compare`, `restore` and `--profile` each hand the allocator a figure that came off
//! the command line as surely as the extents in a photograph's header did — the shape note
//! **N268** bounded at the decoder, one door too late, and note **N322** bounded at two of the
//! four doors. Measured through the shipped binary before this module existed: a 3 GiB file
//! named to `restore` or to `--profile`, under a memory ceiling below its size, was killed with
//! exit 137 and nothing at all on either stream, which is the one failure shape the `--json`
//! ruling forbids (notes **N127**, **N128**).
//!
//! **Here rather than in `webcam-handler-cli-core`, because the engine cannot call that crate.**
//! `engine::profile::read` is the daemon's `--profile` door and the corpus door a `--backend
//! fake` composition root goes through; `cli_core`'s document verbs are the other two. Both
//! depend on this crate and neither depends on the other, so the alternative to this module is
//! two implementations of one law, which is the second home design §2.10 forbids. The budgets
//! themselves stay in [`crate::limits`], where every other cap is.

use camino::Utf8Path;
use std::io::Read as _;

use crate::error::{Error, Result};

/// The bytes of `path`, read only if there are no more than `budget` of them.
///
/// `subject` names what the caller was asking for — `"photograph"`, `"device profile"` — and
/// reaches the refusal message, because a caller who typed two paths needs to know which one
/// this is about and what it was being read as.
///
/// **Two mechanisms, and the second is the one that makes this a class rather than a spelling.**
/// The `metadata` call costs one `stat(2)` and no buffer, so an oversized *regular* file is
/// refused having allocated nothing — that is the fast path and it is what the message about a
/// known size comes from. But `st_size` is not the readable length of anything that is not a
/// regular file: `stat -c '%s %F' /dev/zero` answers `0 character special file`, so a bound read
/// off `stat(2)` alone passes at zero and the read that follows never ends. Measured through the
/// shipped binary with only the fast path in place: `--json photo diff /dev/zero /dev/zero`
/// under a 2 GiB ceiling was killed at exit 137 with zero bytes on both streams — the very
/// failure the fast path had just been written to prevent, reached through the same function
/// (note **N329**; a ban names the class and not one spelling of it, note **N249**, rubric A17).
/// So the read itself is bounded at one byte past the budget, which answers the same way for a
/// character device, a FIFO, a `/proc` entry, and a file that grows between the `stat` and the
/// read.
///
/// One byte past, so that "exactly at the budget" and "past it" are different observations
/// rather than the same one: a file of exactly `budget` bytes is read whole and answered, and
/// `budget + 1` is what makes the refusal below reachable at all.
///
/// # Errors
///
/// [`Error::StorageIo`] naming the path: for a file that cannot be opened or read, and for one
/// whose bytes are past `budget` — separately for the two ways this build can learn that, since
/// only one of them can also say how many bytes there are.
pub fn read_under_budget(path: &Utf8Path, budget: u64, subject: &str) -> Result<Vec<u8>> {
    let unreadable = |errno: Option<i32>, message: String| Error::StorageIo {
        path: path.to_owned(),
        errno,
        message,
    };
    let reported = std::fs::metadata(path)
        .map_err(|error| unreadable(error.raw_os_error(), error.to_string()))?
        .len();
    if reported > budget {
        return Err(unreadable(
            None,
            format!(
                "is {reported} bytes, which is past this build's budget of {budget} bytes for a \
                 {subject} named on a command line",
            ),
        ));
    }

    let file = std::fs::File::open(path)
        .map_err(|error| unreadable(error.raw_os_error(), error.to_string()))?;
    let mut bytes = Vec::new();
    file.take(budget.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| unreadable(error.raw_os_error(), error.to_string()))?;
    // `try_from` rather than `as`, for AGENTS' reason about every number that crosses a width:
    // the saturation is unreachable on any target this builds for, and the direction it
    // saturates in refuses rather than admits.
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > budget {
        return Err(unreadable(
            None,
            format!(
                "reads past this build's budget of {budget} bytes for a {subject} named on a \
                 command line, whatever its own reported length of {reported} bytes says",
            ),
        ));
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use camino::Utf8PathBuf;

    use super::*;

    /// This crate's own manifest, as a file with a length and contents this build can read.
    ///
    /// A committed file rather than a written fixture, so this module needs no scratch
    /// directory and no `tempfile`: what the arms below vary is the *budget*, and a real file of
    /// a known length is all a budget needs to sit above, on, or below.
    fn manifest() -> Utf8PathBuf {
        Utf8Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml")
    }

    fn length_of(path: &Utf8Path) -> u64 {
        std::fs::metadata(path)
            .expect("this crate's manifest is there")
            .len()
    }

    #[test]
    fn a_file_exactly_at_the_budget_is_read_whole() {
        // The other side of the bound (note **N255**): a budget is only a bound if something
        // inside it comes back. The size is read off the file rather than assumed, because an
        // arm asserting "exactly at the budget" about a file that was not would be green for the
        // wrong reason.
        let path = manifest();
        let size = length_of(&path);
        let bytes = read_under_budget(&path, size, "device profile")
            .expect("a file exactly at the budget is inside it");
        assert_eq!(
            u64::try_from(bytes.len()).unwrap_or(u64::MAX),
            size,
            "a file at the budget must come back whole, not truncated to the budget"
        );
    }

    #[test]
    fn a_file_one_byte_past_the_budget_is_refused_by_its_own_reported_length() {
        // The fast path: `stat(2)` answers, nothing is allocated, and the refusal can say how
        // many bytes there are because the file system knew.
        let path = manifest();
        let size = length_of(&path);
        let refusal = read_under_budget(&path, size - 1, "device profile")
            .expect_err("a file past the budget must be refused rather than read");
        let Error::StorageIo {
            path: named,
            errno,
            message,
        } = &refusal
        else {
            panic!("a file past the budget was refused as {refusal:?}");
        };
        assert_eq!(named, &path, "the refusal names the file it refused");
        assert_eq!(
            *errno, None,
            "no system call failed; this build declined to make one"
        );
        assert!(
            message.contains(&size.to_string()) && message.contains(&(size - 1).to_string()),
            "the refusal says what the file is and what this build will spend: {message}"
        );
    }

    #[test]
    fn a_file_whose_reported_length_is_not_its_readable_length_is_refused_by_the_read() {
        // **The class the fast path alone cannot close, and the measurement that found it.**
        // `st_size` is the readable length of a regular file and of nothing else: `stat -c '%s
        // %F' /dev/zero` answers `0 character special file`, so a bound read off `stat(2)`
        // passes at zero and the read behind it never ends. Measured through the shipped binary
        // with only the fast path in place — `--json photo diff /dev/zero /dev/zero` under a
        // 2 GiB ceiling was killed at exit 137 with zero bytes on both streams, which is the
        // failure shape the fast path had just been written to prevent (note **N329**).
        //
        // `/dev/zero` rather than a FIFO because this workspace drives V4L2 and does not build
        // anywhere it is missing, and a sixty-four byte budget because what is under test is
        // that the read stops — proving it with the product's own half-gigabyte budget would
        // allocate half a gigabyte to learn nothing extra.
        //
        // **How this arm goes red is worth knowing before it does.** Narrow the bound to
        // `budget` and it fails in milliseconds, on the `expect_err` below, because sixty-four
        // bytes is not past sixty-four. Delete the bound and there is nothing left to end the
        // read: the arm stops finishing rather than failing, and what turns that into a named
        // failure is the three-minute deadline `.config/nextest.toml` gives every test — which
        // is what that deadline is for.
        let path = Utf8Path::new("/dev/zero");
        assert!(
            path.exists(),
            "this arm's fixture is the kernel's own endless file and it is not there"
        );
        let refusal = read_under_budget(path, 64, "photograph")
            .expect_err("a file with no end must be refused rather than read");
        let Error::StorageIo {
            path: named,
            errno,
            message,
        } = &refusal
        else {
            panic!("an endless file was refused as {refusal:?}");
        };
        assert_eq!(named, path, "the refusal names the file it refused");
        assert_eq!(
            *errno, None,
            "no system call failed; this build stopped reading"
        );
        assert!(
            message.contains("reads past this build's budget of 64 bytes")
                && message.contains("reported length of 0 bytes"),
            "the refusal has to separate itself from the fast path's, because the two are \
             different facts about the file and only one of them is what `stat(2)` said: \
             {message}"
        );
    }

    #[test]
    fn a_path_that_is_not_there_is_refused_with_the_systems_own_errno() {
        // Availability is not capability (AGENTS rule 7): a missing file is the operating
        // system's answer and carries its number, which is what tells an unattended reader a
        // typo apart from a file this build declined to spend memory on. The two refusals above
        // carry no errno for exactly that reason, so this arm is what makes that assertion mean
        // something.
        let path = manifest().with_file_name("no-such-file-here.toml");
        let refusal = read_under_budget(&path, 1024, "device profile")
            .expect_err("a path that is not there cannot be read");
        let Error::StorageIo { errno, .. } = &refusal else {
            panic!("a missing file was refused as {refusal:?}");
        };
        assert_eq!(
            *errno,
            Some(2),
            "ENOENT is the system's own answer and it belongs in the document"
        );
    }
}
