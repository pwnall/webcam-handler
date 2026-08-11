//! The runtime directory: where the daemon's socket lives, and the environment that
//! answers for it.
//!
//! ## Why the runtime half is here and the state half is not
//!
//! `$XDG_RUNTIME_DIR/webcam-handler` is a **transport** fact. Two processes have to agree
//! on it and they have almost nothing else in common: `wchd` binds the socket there
//! (design D11) and `wchc` connects to it. The thin-client wall (T6, enforced by
//! `scripts/gates/dependency-walls.sh`) says `webcam-handler-client` links no backend and
//! no engine, so a client that had to reach `webcam-handler-engine` to learn where a
//! socket lives would be linking a session store to find a file descriptor. This crate is
//! the only home below both ends of that socket, which is the same argument
//! [`crate::limits::DAEMON_SOCKET_FILE`] is already here for — and the directory a socket
//! hangs in cannot live one crate above the socket's own file name without the pair of
//! them becoming two answers to one question (design §2.10).
//!
//! The **state** directory is the other half, and it stays in
//! `webcam-handler-engine::paths` on purpose. D9's session tree is a *storage* fact:
//! resolving it is the first line of opening a store, and nothing reaches for it that is
//! not already linking the engine to read what is inside. So the line is drawn along what
//! a directory is *for* — a socket's home versus a session tree's — rather than along
//! which module happened to resolve both first. The rejected alternative was to move both
//! halves here, which would have put D9's layout one crate away from the only code that
//! walks it and left `webcam-handler-engine::store` reaching upward for its own root.
//!
//! ## Ours rather than a path crate's
//!
//! The `directories` crate reaches MPL-2.0 code through `option-ext` and the license gate
//! refuses it (note N2). We need exactly two paths, both fixed by one paragraph of the XDG
//! Base Directory specification, on one platform; vendoring a cross-platform path library
//! to get them is the worse trade.
//!
//! ## The environment is a parameter
//!
//! It is never read from the process. That is not purity for its own sake:
//! `std::env::set_var` is a data race against every other thread in the binary — `unsafe`
//! since Rust 2024, which this crate forbids — so a test that wanted to exercise a
//! fallback by mutating the environment would have to serialize itself against every other
//! test in the same process. [`Env`] turns the fallback into a value, and the tests below
//! run in parallel like everything else.

use camino::Utf8PathBuf;

use crate::error::{Error, Result};

/// The directory component this tool owns inside each XDG base directory.
///
/// One constant for both halves, which is why it is on this side of the split: the state
/// directory is `<XDG state home>/webcam-handler` too, and `webcam-handler-engine::paths`
/// joins this same value rather than spelling the name a second time.
pub const APP_DIR: &str = "webcam-handler";

/// The base directory for per-session runtime objects (the daemon's UDS).
///
/// Public because the name is what a *fixture* sets: `engine::paths::TempRuntimeDir::env`
/// fabricates an environment in which this variable is a real directory, and a fixture
/// that spelled the variable itself would be the second spelling this module exists to
/// prevent.
pub const XDG_RUNTIME_DIR: &str = "XDG_RUNTIME_DIR";

/// A read-only view of an environment.
///
/// One method, because two paths is all this tool resolves. Implementors are values, so a
/// caller can hand these functions a fabricated environment without touching the process.
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
/// Public rather than test-only because the daemon's integration tests, the CLI's
/// golden-output tests and P4f's `wchc` tests need the same fixed environment, and three
/// copies of a two-field map is three copies of a law (design §2.10).
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

/// This tool's runtime directory: `<XDG runtime dir>/webcam-handler`.
///
/// No fallback, deliberately. `$XDG_RUNTIME_DIR` is the only directory the platform
/// promises is per-user, 0700, and cleaned at logout; `/tmp` is none of those, and the
/// daemon's socket permissions doctrine (design D11: the UDS directory is 0700) rests on
/// the promise. A missing `$XDG_RUNTIME_DIR` means the process is not in a login
/// session, which the operator needs to be told rather than worked around.
///
/// Read by `webcam-handler-daemon::uds`, which binds the socket, and — the reason this
/// function is in this crate rather than in the engine — by `wchc`, which connects to it.
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

/// The value of `key` when it names a directory we are allowed to believe.
///
/// The specification asks for all three checks and each one is a real failure we would
/// otherwise inherit: an unset variable falls back, an *empty* one "must be considered
/// as unset", and "if an implementation encounters a relative path it should consider
/// the path invalid and ignore it". The last is the one that bites — a stray
/// `XDG_STATE_HOME=.` would otherwise scatter session directories through whatever
/// working directory the daemon happened to start in.
///
/// Public, and that is the one consequence of splitting the two directories across two
/// crates: the rule is the *specification's* and it governs both halves, so
/// `webcam-handler-engine::paths::state_dir` calls this rather than carrying a second copy
/// of three checks whose whole point is that they are the same three (design §2.10).
#[must_use]
pub fn usable_dir(env: &dyn Env, key: &str) -> Option<Utf8PathBuf> {
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
    use crate::error::ErrorKind;

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
    fn an_empty_variable_counts_as_unset() {
        // The specification's wording, and the shape a shell produces by accident:
        // `XDG_RUNTIME_DIR= wchd` exports an empty string, not an absent variable.
        let env = MapEnv::from_pairs(&[("XDG_RUNTIME_DIR", "")]);
        assert_eq!(
            runtime_dir(&env).expect_err("empty is unset").kind(),
            ErrorKind::StorageIo
        );
    }

    #[test]
    fn a_relative_path_is_ignored_rather_than_resolved_against_the_working_directory() {
        // The bug this catches: joining APP_DIR onto a relative value and binding the
        // daemon's socket into whatever directory it was started from — where the 0700
        // promise D11 rests on does not hold.
        let env = MapEnv::from_pairs(&[("XDG_RUNTIME_DIR", "run/user/1000")]);
        assert_eq!(
            runtime_dir(&env)
                .expect_err("relative is no runtime dir")
                .kind(),
            ErrorKind::StorageIo
        );
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
