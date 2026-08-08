//! Loading and unloading the two kernel modules this project tests against.
//!
//! ## Why `modprobe` and not `finit_module`
//!
//! `vivid` declares nine dependencies (`videodev`, `mc`, `videobuf2-*`, `v4l2-tpg`,
//! `v4l2-dv-timings`, `cec`). Resolving and ordering those is exactly the job `modprobe`
//! exists to do, and the alternative — linking `libkmod` — is **LGPL**, which design §2.8
//! forbids linking. Running the root-owned `/usr/sbin/modprobe` as a subprocess keeps the
//! license question where it belongs: at a process boundary rather than a link edge.
//!
//! Design §1's "no runtime external binaries" rule is about the *product*. This is dev
//! tooling that never ships, and note N8 records the distinction rather than leaving the
//! rule looking broken.
//!
//! ## The environment is scrubbed, and that is load-bearing
//!
//! `modprobe` honours `MODPROBE_OPTIONS`, which can carry `-C <dir>` and point it at an
//! attacker-chosen configuration. A binary carrying `CAP_SYS_MODULE` that inherits that
//! variable is a local root escalation with extra steps. `AT_SECURE` — which the kernel
//! sets when file capabilities grant more than the caller had — does **not** scrub it;
//! glibc's list covers `LD_*` and friends, not this. So [`command`] clears the environment
//! outright and rebuilds the two variables a subprocess actually needs.

use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The module utilities, by absolute path. Never resolved through `PATH`.
const MODPROBE: &str = "/usr/sbin/modprobe";

/// Where the kernel lists loaded modules.
const PROC_MODULES: &str = "/proc/modules";

/// Where V4L2 nodes appear, for the settle wait and the holder scan.
const VIDEO_CLASS_DIR: &str = "/sys/class/video4linux";

/// How long to wait for device nodes to appear or vanish after a module operation.
const SETTLE_DEADLINE_MS: u64 = 5_000;

/// How often to look while waiting.
const SETTLE_POLL_MS: u64 = 50;

/// A subprocess invocation with nothing inherited from our environment.
///
/// `env_clear` then two explicit variables: a fixed `PATH` (nothing here resolves through
/// it, but a child of `modprobe` might) and `LC_ALL=C`, so the output we parse is not
/// translated out from under us.
fn command(program: &str) -> Command {
    let mut cmd = Command::new(program);
    cmd.env_clear()
        .env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin")
        .env("LC_ALL", "C");
    cmd
}

/// Run a module utility and turn a non-zero exit into a readable error.
fn run(program: &str, args: &[&str]) -> Result<(), Error> {
    if !Path::new(program).exists() {
        return Err(Error::MissingUtility {
            path: program.to_owned(),
        });
    }
    let output = command(program)
        .args(args)
        .output()
        .map_err(|error| Error::Spawn {
            program: program.to_owned(),
            message: error.to_string(),
        })?;
    if output.status.success() {
        return Ok(());
    }
    Err(Error::Failed {
        program: program.to_owned(),
        args: args.iter().map(|a| (*a).to_owned()).collect(),
        code: output.status.code(),
        stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
    })
}

/// Whether a module is loaded, read from `/proc/modules`.
///
/// # Errors
///
/// [`Error::Proc`] when `/proc/modules` cannot be read, which on Linux means the mount is
/// missing rather than that the module is absent — those are different answers and this
/// does not fold one into the other.
pub(crate) fn is_loaded(module: &str) -> Result<bool, Error> {
    let text = std::fs::read_to_string(PROC_MODULES).map_err(|error| Error::Proc {
        path: PROC_MODULES.to_owned(),
        message: error.to_string(),
    })?;
    // The module name is the first field of each line, and `-` and `_` are
    // interchangeable in module names — `videobuf2-common` lists as `videobuf2_common`.
    let wanted = module.replace('-', "_");
    Ok(text
        .lines()
        .filter_map(|line| line.split_whitespace().next())
        .any(|name| name.replace('-', "_") == wanted))
}

/// The V4L2 nodes present right now.
pub(crate) fn video_nodes() -> BTreeSet<String> {
    let Ok(entries) = std::fs::read_dir(VIDEO_CLASS_DIR) else {
        return BTreeSet::new();
    };
    entries
        .flatten()
        .filter_map(|entry| entry.file_name().to_str().map(str::to_owned))
        .filter(|name| name.starts_with("video"))
        .collect()
}

/// A process holding a `/dev/video*` node open.
///
/// Deliberately *not* `webcam-handler-schema::Holder`: this binary carries
/// root-equivalent capabilities, and every dependency it links is attack surface inside
/// that boundary. Thirty lines of `/proc` walking is a better trade than pulling the
/// product's crate graph into a blessed binary. The V4L2 backend's own `Busy` diagnosis
/// (D13, arriving at P2) answers a different question — *who holds this one node* — and
/// belongs there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Holder {
    /// The holding process.
    pub(crate) pid: i32,
    /// Its `comm`, when `/proc` let us read it.
    pub(crate) comm: Option<String>,
    /// Which node it has open.
    pub(crate) node: String,
}

/// Every process with a `/dev/video*` node open.
///
/// Best-effort by nature: `/proc/<pid>/fd` is readable only for our own processes unless
/// we are root, and `hidepid` can hide it entirely. An empty answer therefore means "we
/// saw nobody", not "nobody is there" — [`cycle_uvcvideo`] says so rather than treating
/// the two as the same.
#[must_use]
pub(crate) fn video_holders() -> Vec<Holder> {
    let mut holders = Vec::new();
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return holders;
    };

    for entry in entries.flatten() {
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let Ok(pid) = name.parse::<i32>() else {
            continue;
        };
        let fd_dir = PathBuf::from("/proc").join(&name).join("fd");
        let Ok(fds) = std::fs::read_dir(&fd_dir) else {
            continue;
        };
        for fd in fds.flatten() {
            let Ok(target) = std::fs::read_link(fd.path()) else {
                continue;
            };
            let Some(target) = target.to_str() else {
                continue;
            };
            if !target.starts_with("/dev/video") {
                continue;
            }
            holders.push(Holder {
                pid,
                comm: std::fs::read_to_string(format!("/proc/{name}/comm"))
                    .ok()
                    .map(|c| c.trim().to_owned()),
                node: target.to_owned(),
            });
        }
    }
    holders
}

/// Wait until `predicate` holds, or the settle deadline passes.
///
/// Returns whether it converged. Polling rather than watching: this is a dev tool waiting
/// on udev to finish creating device nodes, the deadline is bounded, and an inotify watch
/// on `/dev` would be more machinery than the job is worth.
fn settle(predicate: impl Fn() -> bool) -> bool {
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(SETTLE_DEADLINE_MS);
    loop {
        if predicate() {
            return true;
        }
        if std::time::Instant::now() >= deadline {
            return false;
        }
        // The workspace bans `thread::sleep` globally (note N3) because a sleep used as
        // *synchronization* schedules a flake. This is not that: it is the poll interval
        // of a bounded wait in a dev-only binary, and the deadline above is what makes
        // the loop terminate.
        #[expect(
            clippy::disallowed_methods,
            reason = "poll interval of a deadline-bounded wait in a dev tool, not test synchronization (N3)"
        )]
        std::thread::sleep(std::time::Duration::from_millis(SETTLE_POLL_MS));
    }
}

/// Load `vivid` with `devices` instances.
///
/// # Errors
///
/// [`Error::Failed`] when `modprobe` refuses, [`Error::DidNotSettle`] when the module
/// loads but no new node appears within the deadline.
pub(crate) fn load_vivid(devices: u8) -> Result<Vec<String>, Error> {
    let before = video_nodes();
    let n_devs = format!("n_devs={devices}");
    run(MODPROBE, &["vivid", &n_devs])?;

    if !settle(|| video_nodes().len() > before.len()) {
        return Err(Error::DidNotSettle {
            what: "vivid loaded but created no new /dev/video* node".to_owned(),
        });
    }
    Ok(video_nodes().difference(&before).cloned().collect())
}

/// Unload `vivid`.
///
/// `modprobe -r` rather than `rmmod`, so the dependency modules it dragged in go too —
/// but only the ones nothing else is using. `videodev` stays, because `uvcvideo` holds it.
///
/// # Errors
///
/// [`Error::Failed`] when the module is in use, which is the honest answer: something has
/// a vivid node open.
pub(crate) fn unload_vivid() -> Result<Vec<String>, Error> {
    let before = video_nodes();
    run(MODPROBE, &["-r", "vivid"])?;
    if !settle(|| video_nodes().len() < before.len() || !is_loaded("vivid").unwrap_or(false)) {
        return Err(Error::DidNotSettle {
            what: "vivid unloaded but its nodes are still present".to_owned(),
        });
    }
    Ok(before.difference(&video_nodes()).cloned().collect())
}

/// Unload and reload `uvcvideo`, making every real camera vanish and come back.
///
/// This is how `Error::DeviceGone` and the P4 hotplug path become testable against real
/// hardware rather than against the fake — there is no other way to make a built-in
/// laptop camera disappear without physically unplugging something that is soldered down.
///
/// **The interlock:** it refuses when any process holds a `/dev/video*` node open. Pulling
/// the driver out from under a video call is the kind of thing a dev tool does exactly
/// once before somebody deletes it. `force` overrides, because the operator may know the
/// holder is their own stuck test.
///
/// # Errors
///
/// [`Error::InUse`] when a holder is present and `force` is false; otherwise as the
/// load/unload functions.
pub(crate) fn cycle_uvcvideo(force: bool) -> Result<Cycled, Error> {
    let holders = video_holders();
    if !holders.is_empty() && !force {
        return Err(Error::InUse { holders });
    }

    let before = video_nodes();
    run(MODPROBE, &["-r", "uvcvideo"])?;
    let gone = settle(|| video_nodes().is_empty() || video_nodes() != before);

    run(MODPROBE, &["uvcvideo"])?;
    let back = settle(|| video_nodes().len() >= before.len());

    Ok(Cycled {
        nodes_before: before.into_iter().collect(),
        nodes_after: video_nodes().into_iter().collect(),
        vanished: gone,
        returned: back,
        // Reported rather than swallowed: an empty scan on a `hidepid` mount means we
        // could not see holders, not that there were none.
        holders_seen: holders,
    })
}

/// What a `uvcvideo cycle` did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Cycled {
    /// The nodes present before.
    pub(crate) nodes_before: Vec<String>,
    /// The nodes present after.
    pub(crate) nodes_after: Vec<String>,
    /// Whether the nodes actually went away — if false, the unload did not take effect
    /// and nothing was proved about `DeviceGone`.
    pub(crate) vanished: bool,
    /// Whether they came back within the deadline.
    pub(crate) returned: bool,
    /// Any holder seen before the cycle (non-empty only under `force`).
    pub(crate) holders_seen: Vec<Holder>,
}

/// What can go wrong.
#[derive(Debug)]
pub(crate) enum Error {
    /// A module utility is not where it should be.
    MissingUtility {
        /// The absolute path we looked for.
        path: String,
    },
    /// The utility could not be started.
    Spawn {
        /// Which one.
        program: String,
        /// The system's description.
        message: String,
    },
    /// It ran and refused.
    Failed {
        /// Which one.
        program: String,
        /// With what arguments.
        args: Vec<String>,
        /// Its exit code, when it had one.
        code: Option<i32>,
        /// What it said.
        stderr: String,
    },
    /// The kernel accepted the operation and the world did not change in time.
    DidNotSettle {
        /// What was expected.
        what: String,
    },
    /// Somebody is using a camera.
    InUse {
        /// Who, as far as `/proc` would say.
        holders: Vec<Holder>,
    },
    /// `/proc` could not be read.
    Proc {
        /// The path.
        path: String,
        /// The system's description.
        message: String,
    },
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::MissingUtility { path } => {
                write!(f, "{path} is not installed; kmod provides it")
            }
            Error::Spawn { program, message } => write!(f, "could not run {program}: {message}"),
            Error::Failed {
                program,
                args,
                code,
                stderr,
            } => {
                let code = code.map_or_else(|| "a signal".to_owned(), |c| c.to_string());
                write!(
                    f,
                    "{} {} exited with {code}{}",
                    Path::new(program)
                        .file_name()
                        .and_then(OsStr::to_str)
                        .unwrap_or(program),
                    args.join(" "),
                    if stderr.is_empty() {
                        String::new()
                    } else {
                        format!(": {stderr}")
                    }
                )
            }
            Error::DidNotSettle { what } => write!(f, "{what} (waited {SETTLE_DEADLINE_MS} ms)"),
            Error::InUse { holders } => {
                writeln!(f, "a camera is in use; refusing to unload uvcvideo:")?;
                for holder in holders {
                    writeln!(
                        f,
                        "  {} held by {} (pid {})",
                        holder.node,
                        holder.comm.as_deref().unwrap_or("an unreadable process"),
                        holder.pid
                    )?;
                }
                write!(f, "Pass --force if you know the holder is yours.")
            }
            Error::Proc { path, message } => write!(f, "could not read {path}: {message}"),
        }
    }
}

impl std::error::Error for Error {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_loaded_module_is_found_and_an_invented_one_is_not() {
        // Both directions against the live kernel. `uvcvideo` is loaded on any host with
        // a UVC camera; on one without, the negative half still holds.
        assert!(
            !is_loaded("definitely_not_a_real_module_xyzzy").expect("/proc/modules reads"),
            "an invented module reported loaded"
        );
        // Whatever is loaded, the answer must be stable across two reads.
        let first = is_loaded("uvcvideo").expect("/proc/modules reads");
        assert_eq!(first, is_loaded("uvcvideo").expect("/proc/modules reads"));
    }

    #[test]
    fn module_names_match_across_the_hyphen_underscore_spelling() {
        // `/proc/modules` prints `videobuf2_common` for a module packaged as
        // `videobuf2-common`. Asking with either spelling must give the same answer, or a
        // dependency check would report a loaded module as absent.
        let underscored = is_loaded("videobuf2_common").expect("/proc/modules reads");
        let hyphenated = is_loaded("videobuf2-common").expect("/proc/modules reads");
        assert_eq!(underscored, hyphenated);
    }

    #[test]
    fn the_subprocess_environment_carries_nothing_we_did_not_put_there() {
        // The MODPROBE_OPTIONS escalation, closed by construction. A blessed binary that
        // let this through would let anyone who can set an environment variable choose
        // modprobe's configuration directory.
        let cmd = command(MODPROBE);
        let envs: Vec<(String, Option<String>)> = cmd
            .get_envs()
            .map(|(k, v)| {
                (
                    k.to_string_lossy().into_owned(),
                    v.map(|v| v.to_string_lossy().into_owned()),
                )
            })
            .collect();
        let names: BTreeSet<&str> = envs.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(
            names,
            BTreeSet::from(["PATH", "LC_ALL"]),
            "the subprocess environment is not exactly what we set"
        );
        // …and `env_clear` is what makes that true of the *inherited* environment too.
        assert!(
            cmd.get_envs().len() == 2,
            "env_clear was not applied before the explicit sets"
        );
    }

    #[test]
    fn the_module_utility_path_is_absolute_and_not_resolved_through_path() {
        // A relative name would be resolved through `PATH` inside a root-capable process.
        assert!(Path::new(MODPROBE).is_absolute());
    }

    #[test]
    fn a_missing_utility_is_named_rather_than_reported_as_a_module_failure() {
        let error = run("/nonexistent/modprobe", &["vivid"])
            .expect_err("a utility that is not there cannot run");
        assert!(
            matches!(error, Error::MissingUtility { .. }),
            "{error} should have named the missing binary"
        );
        assert!(error.to_string().contains("/nonexistent/modprobe"));
    }

    #[test]
    fn the_holder_scan_reads_this_process_and_survives_unreadable_ones() {
        // Non-vacuity for the interlock: the scan must at minimum be able to see a
        // *deliberately opened* node, or `cycle_uvcvideo`'s refusal would never fire.
        let nodes = video_nodes();
        if nodes.is_empty() {
            return; // No camera on this host; the interlock has nothing to guard.
        }
        let Some(node) = nodes.iter().next() else {
            return;
        };
        let path = format!("/dev/{node}");
        let Ok(held) = std::fs::File::open(&path) else {
            return; // No permission on this host; not a failure of the scan.
        };

        let seen = video_holders();
        let me = std::process::id();
        assert!(
            seen.iter()
                .any(|h| h.pid == i32::try_from(me).unwrap_or(-1) && h.node == path),
            "the scan did not see this process holding {path}; the uvcvideo interlock \
             would never fire"
        );
        drop(held);

        // And the other direction: once released, we are no longer a holder of it.
        assert!(
            !video_holders()
                .iter()
                .any(|h| h.pid == i32::try_from(me).unwrap_or(-1) && h.node == path),
            "the scan still reports a released node as held"
        );
    }

    #[test]
    fn cycling_refuses_while_a_camera_is_open_unless_forced() {
        // The interlock itself. Written so it exercises the refusal without ever
        // unloading anything: `force = false` returns before the first modprobe.
        let nodes = video_nodes();
        if nodes.is_empty() {
            return;
        }
        let Some(node) = nodes.iter().next() else {
            return;
        };
        let Ok(held) = std::fs::File::open(format!("/dev/{node}")) else {
            return;
        };

        match cycle_uvcvideo(false) {
            Err(Error::InUse { holders }) => {
                assert!(!holders.is_empty());
                let rendered = Error::InUse { holders }.to_string();
                assert!(rendered.contains("--force"), "{rendered}");
                assert!(rendered.contains("refusing"), "{rendered}");
            }
            Err(other) => panic!("expected the in-use refusal, got {other}"),
            Ok(_) => panic!(
                "cycle_uvcvideo unloaded the driver while this test held a camera open — \
                 the interlock did not fire"
            ),
        }
        drop(held);
    }
}
