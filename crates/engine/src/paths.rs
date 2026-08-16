//! The state directory: where D9's session tree lives, and the fixtures that fake one.
//!
//! The other XDG directory this tool needs is in [`schema::paths`], and the split is
//! deliberate. `$XDG_RUNTIME_DIR/webcam-handler` is a **transport** fact — the daemon binds a
//! socket in it (D11) and `webcam-handler-client` connects to that socket, and the thin-client
//! wall (T6) forbids `webcam-handler-client` from linking this crate, so a thin client that
//! had to come here to learn where a socket lives could not be a thin client. The state
//! directory is a **storage** fact: it is the first line of opening a session store, and
//! nothing asks for it that is not already linking the engine to read what is inside. One home
//! each (design §2.10), and the line is drawn along what a directory is *for* rather than
//! along which module resolved both first.
//!
//! What that leaves here is [`state_dir`], the `$HOME` fallback the specification gives it,
//! and the temporary-directory fixtures — including [`TempRuntimeDir`], which is about the
//! *runtime* directory and stays on this side anyway. It is a `tempfile::TempDir` wearing a
//! [`camino`] path, and `webcam-handler-schema` has no `tempfile` edge; moving the fixture
//! would have bought one crate's convenience with a dependency on the crate every other
//! crate links. `tempfile` is already here, for [`crate::store::write_json_atomic`]'s
//! in-directory temporary file, so the fixture costs this workspace nothing where it is —
//! and `webcam-handler-client`'s tests can take a dev-dependency on the engine, which
//! `scripts/gates/dependency-walls.sh` counts as no edge at all because a test binary ships
//! nowhere.
//!
//! Ours rather than the `directories` crate's, for [`schema::paths`]'s reason: that crate
//! reaches MPL-2.0 code through `option-ext` and the license gate refuses it (note N2).
//! The environment arrives as a parameter for [`schema::paths::Env`]'s reason, too — it is
//! the same seam, and there is only one of it.

use camino::{Utf8Path, Utf8PathBuf};
use schema::paths::{APP_DIR, Env, MapEnv, XDG_RUNTIME_DIR, usable_dir};
use schema::{Error, Result};

/// The base directory for persistent state (design D9's session tree).
const XDG_STATE_HOME: &str = "XDG_STATE_HOME";

/// The user's home, for the specification's `$XDG_STATE_HOME` fallback.
const HOME: &str = "HOME";

/// This tool's persistent state directory: `<XDG state home>/webcam-handler`.
///
/// `$XDG_STATE_HOME` when it is usable, else the specification's `$HOME/.local/state`
/// fallback. Nothing is created here — resolving a path and making a directory are
/// different operations with different failure modes, and the store owns the second one.
///
/// What "usable" means is [`schema::paths::usable_dir`]'s, not a second copy of it: the
/// three checks the specification asks for govern both XDG directories, so they are
/// spelled once below the pair of them.
///
/// # Errors
///
/// [`Error::StorageIo`] when neither variable gives a usable directory. There is no
/// third fallback: writing sessions into a guessed directory is how a tool loses a
/// calibration run somewhere the operator will never look.
pub fn state_dir(env: &dyn Env) -> Result<Utf8PathBuf> {
    if let Some(base) = usable_dir(env, XDG_STATE_HOME) {
        return Ok(base.join(APP_DIR));
    }
    if let Some(home) = usable_dir(env, HOME) {
        return Ok(home.join(".local").join("state").join(APP_DIR));
    }
    Err(Error::StorageIo {
        path: format!("${XDG_STATE_HOME}").into(),
        errno: None,
        message: format!(
            "unset, and ${HOME} gives no usable directory either; \
             set one so calibration sessions have somewhere to live"
        ),
    })
}

/// A temporary directory under [`schema::paths::scratch_root`], removed when it is dropped.
///
/// **The one constructor for a scratch directory in this workspace's Rust**, and the reason it
/// is one rather than fifteen is the 2026-08-12 ruling (note **N84**): `tempfile::tempdir()`
/// reads `$TMPDIR`, `$TMPDIR` is a 16 GB tmpfs on the machine this project is developed on,
/// and "where does test scratch go" is a decision that has to be revisable in one place to be
/// revisable at all. `scripts/gates/scratch-has-one-home.sh` is what keeps it one: a
/// `tempfile::tempdir()` typed out of habit is red there.
///
/// It is here rather than beside the path it joins for this module header's reason —
/// `webcam-handler-schema` carries no `tempfile` edge and is not gaining one for a fixture —
/// and it is public, and un-gated by any feature, for [`crate::store::TempStore`]'s reason:
/// the daemon's tests, the CLI's, the backend's and `webcam-handler-client`'s all need it, and
/// a `#[cfg(test)]` helper is a helper only this crate can use.
///
/// [`TempRuntimeDir`] below is the deliberate exception and the two are worth reading
/// together.
///
/// # Errors
///
/// Whatever [`schema::paths::scratch_root`] refuses with, or [`Error::StorageIo`] when the
/// directory cannot be created inside it.
pub fn scratch_dir() -> Result<tempfile::TempDir> {
    let root = schema::paths::scratch_root()?;
    let storage_io = |err: &std::io::Error| Error::StorageIo {
        path: root.as_str().into(),
        errno: err.raw_os_error(),
        message: err.to_string(),
    };
    // **At `store::STATE_DIR_MODE`, not at the ambient umask** (note **N142**). `tempfile`
    // creates a directory at `0o777 & !umask` by default — measured 0775 on the machine this
    // project is developed on — and a scratch session tree holds exactly what a real one
    // does: sample photographs of whatever the camera was pointed at. A fixture that was more
    // open than the thing it stands in for is a fixture that cannot pin the thing's posture,
    // and the store's own startup check would refuse it (which is how this was found).
    //
    // **The mode is given at the `mkdir`, not set afterwards** (note **N150**).
    // `photo::IntoTheSessionTree` spends ten lines arguing that a `set_permissions` after a
    // create leaves a window in which the object exists at the wider mode, and this call is
    // the same shape one object up — an empty directory leaks nothing, but a rule that holds
    // in one place and not its neighbour is a rule nobody can rely on. `tempfile`'s
    // `Builder::permissions` passes it to the `mkdir`, so there is no window to argue about.
    tempfile::Builder::new()
        .permissions(std::os::unix::fs::PermissionsExt::from_mode(
            crate::store::STATE_DIR_MODE,
        ))
        .tempdir_in(&root)
        .map_err(|err| storage_io(&err))
}

/// A private `$XDG_RUNTIME_DIR` that disappears with the value.
///
/// The runtime half of [`crate::store::TempStore`], and public for the same reason: the
/// daemon's own tests, the CLI's subprocess tests and P4f's `webcam-handler-client` tests all
/// need a runtime directory that is real, empty, and gone afterwards, and three copies of that
/// fixture is three copies of a law (design §2.10). [`schema::paths::runtime_dir`]
/// deliberately has no `/tmp` fallback — the 0700 socket-directory promise rests on the
/// platform's — so without this a test would have to weaken the thing it is testing.
///
/// It is in this crate rather than beside the resolver it fabricates an environment for
/// because it is a `tempfile::TempDir`, and `webcam-handler-schema` carries no `tempfile`
/// edge; see this module's header for why that trade goes this way.
///
/// Nothing is created *inside* it: `<base>/webcam-handler` is the daemon's to make, at
/// the mode D11 requires, and a fixture that made it first would be answering the
/// question the daemon's startup check exists to ask.
///
/// No `keep()` twin of [`crate::store::TempStore::keep`]: that one exists because a
/// *session tree* is what an operator reads after a failing store test, and it has a caller
/// there. A runtime directory holds a socket and nothing else worth reading afterwards, and
/// a public function nothing reads is the defect rubric A8 names — so it arrives with the
/// debugging path that wants it, if one ever does.
#[derive(Debug)]
pub struct TempRuntimeDir {
    /// Dropped last of all, which removes the tree.
    #[expect(
        dead_code,
        reason = "held for its `Drop`: the field being unread is what makes the \
                  directory disappear with the value"
    )]
    dir: tempfile::TempDir,
    base: Utf8PathBuf,
}

impl TempRuntimeDir {
    /// A new, empty runtime base under `$TMPDIR`.
    ///
    /// **`$TMPDIR` and not [`scratch_dir`], and this is the one exception the 2026-08-12
    /// ruling has** (note **N84**). Everything else in this workspace's tests moved under
    /// `target/`; a directory whose entire purpose is to hold a socket did not, because
    /// `sockaddr_un::sun_path` is 108 bytes on Linux — 107 once the NUL is counted, which is
    /// [`schema::limits::MAX_UNIX_SOCKET_PATH_BYTES`] — and `target/` is 37 bytes deep before
    /// anything is joined to it on the checkout this was measured on:
    ///
    /// ```text
    /// 41 bytes  /tmp/wchXXXXXXXX/webcam-handler/wchd.sock
    /// 93 bytes  <checkout>/target/wch-scratch/wchXXXXXXXX/webcam-handler/wchd.sock
    /// ```
    ///
    /// 93 fits, with fourteen bytes of headroom that belong to *where somebody cloned this
    /// repository* rather than to anything in this file — and a suite whose verdict moves with
    /// the checkout path is notes N52, N66 and N68 in a fourth dimension. What is bounded by a
    /// kernel constant is kept on the shortest path available; the tmpfs the ruling was about
    /// is not at risk from a socket and an empty directory. `scripts/gates/lib.sh`'s
    /// `gate_socket_scratch_root` is the same decision for the shell gates that bind one, with
    /// the measurement for their deeper paths (146 bytes against a bound of 107).
    ///
    /// The prefix is three characters for the other half of the same arithmetic: `$TMPDIR` is
    /// not always `/tmp` — `scripts/mutants.sh` exports a build root through it — so a chatty
    /// prefix would turn a socket test into an `ENAMETOOLONG` nobody expected. What reclaims
    /// one of these after a `kill -9` is `gate_scratch_sweep`, which sweeps this root too.
    ///
    /// # Errors
    ///
    /// [`Error::StorageIo`] when the temporary directory cannot be made, or when its path
    /// is not UTF-8: every path in this tool is a [`camino`] path, so a `$TMPDIR` we
    /// cannot represent is one we cannot use.
    pub fn new() -> Result<TempRuntimeDir> {
        let dir = tempfile::Builder::new()
            .prefix("wch")
            // wch-scratch-exempt: a socket lives here and `sun_path` is 107 bytes; see above
            .tempdir()
            .map_err(|err| Error::StorageIo {
                path: "$TMPDIR".into(),
                errno: err.raw_os_error(),
                message: err.to_string(),
            })?;
        let base = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).map_err(|path| {
            Error::StorageIo {
                path: "$TMPDIR".into(),
                errno: None,
                message: format!("is not valid UTF-8: {}", path.display()),
            }
        })?;
        Ok(TempRuntimeDir { dir, base })
    }

    /// The directory `$XDG_RUNTIME_DIR` would name.
    #[must_use]
    pub fn base(&self) -> &Utf8Path {
        &self.base
    }

    /// An environment in which `$XDG_RUNTIME_DIR` is this directory and nothing else is
    /// set.
    ///
    /// [`MapEnv::with`] is builder-style, so a test that also needs a state directory
    /// writes `runtime.env().with("XDG_STATE_HOME", …)` rather than assembling a second
    /// map.
    #[must_use]
    pub fn env(&self) -> MapEnv {
        MapEnv::from_pairs(&[(XDG_RUNTIME_DIR, self.base.as_str())])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use schema::ErrorKind;
    use schema::paths::runtime_dir;

    #[test]
    fn a_scratch_session_tree_is_as_private_as_the_real_one_and_is_created_that_way() {
        // Notes **N142** and **N150**. A scratch session tree holds exactly what a real one
        // does — sample photographs of whatever the camera was pointed at — so a fixture more
        // open than the thing it stands in for cannot pin that thing's posture, and the
        // store's own startup check refuses it (which is how the umask default was found).
        //
        // What this assertion cannot see is the *window*, which is the other half: the mode
        // is given to `mkdir` rather than set afterwards, so the directory never exists at
        // the wider one. `photo::IntoTheSessionTree` argues the same shape one object down,
        // and the two now agree.
        use std::os::unix::fs::PermissionsExt as _;

        let dir = scratch_dir().expect("a scratch dir");
        let mode = std::fs::metadata(dir.path())
            .expect("just created")
            .permissions()
            .mode()
            & schema::paths::MODE_BITS;
        assert_eq!(
            mode & schema::paths::GROUP_AND_OTHER_BITS,
            0,
            "a scratch session tree came out at mode {mode:04o}, which somebody else can walk \
             into"
        );
        assert_eq!(mode & 0o700, crate::store::STATE_DIR_MODE);
    }

    #[test]
    fn xdg_state_home_wins_when_it_is_set() {
        let env = MapEnv::from_pairs(&[
            ("XDG_STATE_HOME", "/var/lib/state"),
            ("HOME", "/home/someone"),
        ]);
        assert_eq!(
            state_dir(&env).expect("both variables are set"),
            "/var/lib/state/webcam-handler"
        );
    }

    #[test]
    fn the_home_fallback_is_the_specifications_dot_local_state() {
        let env = MapEnv::from_pairs(&[("HOME", "/home/someone")]);
        assert_eq!(
            state_dir(&env).expect("HOME is set"),
            "/home/someone/.local/state/webcam-handler"
        );
    }

    #[test]
    fn with_neither_variable_set_the_state_dir_is_refused_rather_than_guessed() {
        let err = state_dir(&MapEnv::empty()).expect_err("nothing is set");
        assert_eq!(err.kind(), ErrorKind::StorageIo);
        // The message has to name the variable the operator should set; "storage error"
        // is not actionable.
        let rendered = err.to_string();
        assert!(rendered.contains("XDG_STATE_HOME"), "{rendered}");
        assert!(rendered.contains("HOME"), "{rendered}");
    }

    #[test]
    fn an_empty_variable_counts_as_unset() {
        // The specification's wording, and the shape a shell produces by accident:
        // `XDG_STATE_HOME= webcam-handler-daemon` exports an empty string, not an absent
        // variable. The runtime directory's half of this rule is asserted where the resolver
        // is.
        let env = MapEnv::from_pairs(&[("XDG_STATE_HOME", ""), ("HOME", "/home/someone")]);
        assert_eq!(
            state_dir(&env).expect("HOME is set"),
            "/home/someone/.local/state/webcam-handler"
        );
    }

    #[test]
    fn a_relative_path_is_ignored_rather_than_resolved_against_the_working_directory() {
        // The bug this catches: joining APP_DIR onto a relative value and writing
        // sessions into wherever the daemon was started from.
        let env = MapEnv::from_pairs(&[("XDG_STATE_HOME", "state"), ("HOME", "/home/someone")]);
        assert_eq!(
            state_dir(&env).expect("HOME is set"),
            "/home/someone/.local/state/webcam-handler"
        );

        let relative_home = MapEnv::from_pairs(&[("HOME", "someone")]);
        assert_eq!(
            state_dir(&relative_home)
                .expect_err("a relative HOME is no HOME")
                .kind(),
            ErrorKind::StorageIo
        );
    }

    #[test]
    fn the_temp_runtime_dir_is_a_real_directory_the_resolver_accepts() {
        // The fixture's whole job is to satisfy `runtime_dir`'s three checks — set,
        // non-empty, absolute — with a directory that actually exists, so that a socket
        // test exercises the resolver rather than working around it. The resolver is one
        // crate down now, which is exactly why this arm asserts against it rather than
        // against the fixture's own idea of what it made.
        let runtime = TempRuntimeDir::new().expect("a temporary directory");
        assert!(runtime.base().is_absolute(), "{}", runtime.base());
        assert!(runtime.base().as_std_path().is_dir(), "{}", runtime.base());

        let resolved = runtime_dir(&runtime.env()).expect("the fixture sets the variable");
        assert_eq!(resolved, runtime.base().join(APP_DIR));

        // And nothing is created inside it: whether `<base>/webcam-handler` exists, and
        // at what mode, is the daemon's startup check to decide.
        assert!(!resolved.as_std_path().exists(), "{resolved}");
    }
}
