//! The two XDG directories webcam-handler needs.
//!
//! Ours rather than the `directories` crate's, because that crate reaches MPL-2.0 code
//! through `option-ext` and the license gate refuses it (note N2). We need exactly two
//! paths, both fixed by one paragraph of the XDG Base Directory specification, on one
//! platform; vendoring a cross-platform path library to get them is the worse trade.
//!
//! The environment arrives as a parameter and is never read from the process. That is
//! not purity for its own sake: `std::env::set_var` is a data race against every other
//! thread in the binary — `unsafe` since Rust 2024, which this crate forbids — so a test
//! that wanted to exercise the `$HOME` fallback by mutating the environment would have
//! to serialize itself against every other test in the same process. [`Env`] turns the
//! fallback into a value, and the tests below run in parallel like everything else.

use camino::{Utf8Path, Utf8PathBuf};
use schema::{Error, Result};

/// The directory component this tool owns inside each XDG base directory.
pub const APP_DIR: &str = "webcam-handler";

/// The base directory for persistent state (design D9's session tree).
const XDG_STATE_HOME: &str = "XDG_STATE_HOME";

/// The base directory for per-session runtime objects (the daemon's UDS).
const XDG_RUNTIME_DIR: &str = "XDG_RUNTIME_DIR";

/// The user's home, for the specification's `$XDG_STATE_HOME` fallback.
const HOME: &str = "HOME";

/// A read-only view of an environment.
///
/// One method, because two paths is all we resolve. Implementors are values, so a caller
/// can hand these functions a fabricated environment without touching the process.
pub trait Env {
    /// The value of `key`, or `None` when it is unset or not valid UTF-8.
    ///
    /// Non-UTF-8 reads as unset on purpose: every path in this tool is a
    /// [`camino::Utf8PathBuf`], so a value we cannot represent is a value we cannot use,
    /// and falling back is friendlier than failing.
    fn var(&self, key: &str) -> Option<String>;
}

/// The real process environment.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemEnv;

impl Env for SystemEnv {
    fn var(&self, key: &str) -> Option<String> {
        std::env::var(key).ok()
    }
}

/// A fabricated environment: the scriptable double for the [`Env`] seam.
///
/// Public rather than test-only because the daemon's integration tests and the CLI's
/// golden-output tests need the same fixed environment, and three copies of a two-field
/// map is three copies of a law.
#[derive(Debug, Clone, Default)]
pub struct MapEnv {
    vars: std::collections::BTreeMap<String, String>,
}

impl MapEnv {
    /// An environment in which nothing is set.
    #[must_use]
    pub fn empty() -> Self {
        MapEnv::default()
    }

    /// An environment holding exactly these variables.
    #[must_use]
    pub fn from_pairs(pairs: &[(&str, &str)]) -> Self {
        MapEnv {
            vars: pairs
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
        }
    }

    /// Set one variable, builder-style.
    #[must_use]
    pub fn with(mut self, key: &str, value: &str) -> Self {
        self.vars.insert(key.to_owned(), value.to_owned());
        self
    }
}

impl Env for MapEnv {
    fn var(&self, key: &str) -> Option<String> {
        self.vars.get(key).cloned()
    }
}

/// This tool's persistent state directory: `<XDG state home>/webcam-handler`.
///
/// `$XDG_STATE_HOME` when it is usable, else the specification's `$HOME/.local/state`
/// fallback. Nothing is created here — resolving a path and making a directory are
/// different operations with different failure modes, and the store owns the second one.
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

/// This tool's runtime directory: `<XDG runtime dir>/webcam-handler`.
///
/// No fallback, deliberately. `$XDG_RUNTIME_DIR` is the only directory the platform
/// promises is per-user, 0700, and cleaned at logout; `/tmp` is none of those, and the
/// daemon's socket permissions doctrine (design D11: the UDS directory is 0700) rests on
/// the promise. A missing `$XDG_RUNTIME_DIR` means the process is not in a login
/// session, which the operator needs to be told rather than worked around.
///
/// # Errors
///
/// [`Error::StorageIo`] when `$XDG_RUNTIME_DIR` is unset, empty, or relative.
pub fn runtime_dir(env: &dyn Env) -> Result<Utf8PathBuf> {
    match usable_dir(env, XDG_RUNTIME_DIR) {
        Some(base) => Ok(base.join(APP_DIR)),
        None => Err(Error::StorageIo {
            path: format!("${XDG_RUNTIME_DIR}").into(),
            errno: None,
            message: "unset or not an absolute path; a login session normally sets it, \
                      and the daemon socket has nowhere else that is private to you"
                .to_owned(),
        }),
    }
}

/// A private `$XDG_RUNTIME_DIR` that disappears with the value.
///
/// The runtime half of [`crate::store::TempStore`], and public for the same reason: the
/// daemon's own tests, the CLI's subprocess tests and P4f's `wchc` tests all need a
/// runtime directory that is real, empty, and gone afterwards, and three copies of that
/// fixture is three copies of a law (design §2.10). [`runtime_dir`] deliberately has no
/// `/tmp` fallback — the 0700 socket-directory promise rests on the platform's — so
/// without this a test would have to weaken the thing it is testing.
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
    /// The prefix is three characters on purpose. A Unix socket path is bounded by
    /// `sockaddr_un::sun_path` (108 bytes on Linux) and `$TMPDIR` is not always `/tmp` —
    /// `scripts/mutants.sh` exports a scratch one inside `target/` — so a chatty prefix
    /// would turn a socket test into an `ENAMETOOLONG` nobody expected.
    ///
    /// # Errors
    ///
    /// [`Error::StorageIo`] when the temporary directory cannot be made, or when its path
    /// is not UTF-8: every path in this tool is a [`camino`] path, so a `$TMPDIR` we
    /// cannot represent is one we cannot use.
    pub fn new() -> Result<TempRuntimeDir> {
        let dir = tempfile::Builder::new()
            .prefix("wch")
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

/// The value of `key` when it names a directory we are allowed to believe.
///
/// The specification asks for all three checks and each one is a real failure we would
/// otherwise inherit: an unset variable falls back, an *empty* one "must be considered
/// as unset", and "if an implementation encounters a relative path it should consider
/// the path invalid and ignore it". The last is the one that bites — a stray
/// `XDG_STATE_HOME=.` would otherwise scatter session directories through whatever
/// working directory the daemon happened to start in.
fn usable_dir(env: &dyn Env, key: &str) -> Option<Utf8PathBuf> {
    let raw = env.var(key)?;
    if raw.is_empty() {
        return None;
    }
    let path = Utf8PathBuf::from(raw);
    if path.is_absolute() { Some(path) } else { None }
}

#[cfg(test)]
mod tests {
    use super::*;
    use schema::ErrorKind;

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
        // `XDG_STATE_HOME= wchd` exports an empty string, not an absent variable.
        let env = MapEnv::from_pairs(&[("XDG_STATE_HOME", ""), ("HOME", "/home/someone")]);
        assert_eq!(
            state_dir(&env).expect("HOME is set"),
            "/home/someone/.local/state/webcam-handler"
        );
        let runtime = MapEnv::from_pairs(&[("XDG_RUNTIME_DIR", "")]);
        assert_eq!(
            runtime_dir(&runtime).expect_err("empty is unset").kind(),
            ErrorKind::StorageIo
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
    fn the_runtime_dir_honors_its_variable_and_refuses_without_it() {
        let env = MapEnv::from_pairs(&[("XDG_RUNTIME_DIR", "/run/user/1000")]);
        assert_eq!(
            runtime_dir(&env).expect("the variable is set"),
            "/run/user/1000/webcam-handler"
        );

        // No `/tmp` fallback: both directions asserted, because the fallback is exactly
        // the convenience fix that would weaken the 0700 socket-directory promise.
        let err = runtime_dir(&MapEnv::from_pairs(&[("HOME", "/home/someone")]))
            .expect_err("HOME is not a runtime dir");
        assert_eq!(err.kind(), ErrorKind::StorageIo);
        assert!(err.to_string().contains("XDG_RUNTIME_DIR"), "{err}");
    }

    #[test]
    fn the_temp_runtime_dir_is_a_real_directory_the_resolver_accepts() {
        // The fixture's whole job is to satisfy `runtime_dir`'s three checks — set,
        // non-empty, absolute — with a directory that actually exists, so that a socket
        // test exercises the resolver rather than working around it.
        let runtime = TempRuntimeDir::new().expect("a temporary directory");
        assert!(runtime.base().is_absolute(), "{}", runtime.base());
        assert!(runtime.base().as_std_path().is_dir(), "{}", runtime.base());

        let resolved = runtime_dir(&runtime.env()).expect("the fixture sets the variable");
        assert_eq!(resolved, runtime.base().join(APP_DIR));

        // And nothing is created inside it: whether `<base>/webcam-handler` exists, and
        // at what mode, is the daemon's startup check to decide.
        assert!(!resolved.as_std_path().exists(), "{resolved}");
    }

    #[test]
    fn the_system_env_reads_the_process_environment() {
        // Not a claim about any particular variable — the process environment is not
        // ours to control — only that the real implementation is wired to it. `PATH` is
        // present in every environment a test binary can run in; if it is not, the
        // assertion below is the honest reading either way.
        let system = SystemEnv;
        assert_eq!(system.var("WCH_A_VARIABLE_NOBODY_SETS"), None);
        assert_eq!(std::env::var("PATH").ok(), system.var("PATH"));
    }
}
