//! Everything `wchd` says to a service manager, and everything it takes from one (docs/7
//! P4e-ii, second half).
//!
//! Note **N58** split this sub-milestone in two: the first half shipped the *order* the
//! daemon stops in and the [`crate::shutdown::Notifying`] seam, with nobody on the other end
//! of it; this is the other end. Four protocols live here and nowhere else — `sd_notify(3)`'s
//! four sentences, the watchdog's ping, `LISTEN_FDS` socket activation, and `$JOURNAL_STREAM`
//! — because each of them is a *wire format* somebody else defined, and a project that spells
//! one of them in two places has two answers to what systemd said (design §2.10, one home per
//! law).
//!
//! ## Why none of it is behind an "are we under systemd?" test
//!
//! There is a tempting shape here: ask `sd_notify::booted()`, or look for `$INVOCATION_ID`,
//! and only then speak. It is rejected, and every protocol in this module is written so that
//! it does not need one:
//!
//! - **Notifying** is a no-op when `$NOTIFY_SOCKET` is unset — `sd_notify::notify` returns
//!   `Ok(())` without opening anything — so [`Supervisor`] is correct for a `wchd` in a
//!   terminal, in a test, and in a container with no service manager at all. That is the
//!   whole argument for `main` passing it unconditionally instead of
//!   [`crate::shutdown::Unsupervised`], which was the shipped implementation only because
//!   nothing had yet been written to talk to (`crate::shutdown`'s note on that type).
//! - **The watchdog** is a task that exists only when `$WATCHDOG_USEC` says a service manager
//!   is holding a stopwatch ([`spawn_watchdog`]).
//! - **Socket activation** is `LISTEN_FDS` being there or not ([`Activation::from_env`]).
//! - **The journald layer** is stderr already *being* the journal, which is a comparison of
//!   two numbers ([`journal_stream_matches`]).
//!
//! Each of those is a fact about this process's environment, and asking the specific question
//! is both cheaper and honest: a daemon under a *different* supervisor that speaks the notify
//! protocol (s6, a container runtime's shim, `systemd-notify` in a shell script) gets served
//! by exactly the same code, and a "are we systemd" guard would have refused it.
//!
//! ## What is deliberately not here
//!
//! **No forking.** There is no `daemon(3)`, no double-fork, no PID file and no
//! `Type=forking` unit — the process a supervisor starts is the process that serves, which is
//! what makes `MAINPID` correct without being sent, what makes `NotifyAccess=main` sufficient,
//! and what makes the state-directory lock D9 hands this process the lock a supervisor can
//! reason about. `scripts/gates/systemd-units.sh` is what keeps that a claim something checks
//! rather than a sentence in a header.
//!
//! **No `sd_notify` variant that unsets the environment.** sd-notify 0.5 offers
//! `notify_and_unset_env` and `watchdog_enabled_and_unset_env`, and both are `unsafe fn` for
//! the reason Rust 2024 made `std::env::remove_var` unsafe: mutating the environment races
//! every other thread's `getenv`, and this daemon is multi-threaded from `#[tokio::main]`
//! onwards. This crate is `#![forbid(unsafe_code)]` and stays that way. What the unsetting
//! buys — a child process that cannot re-adopt the parent's notify socket — costs this daemon
//! nothing, because `wchd` spawns no children at all.

use std::sync::Arc;
use std::time::Duration;

use camino::{Utf8Path, Utf8PathBuf};
use schema::{Error, Result, limits};
use sd_notify::NotifyState;

use crate::server::Wchd;
use crate::shutdown::{Notifying, Shutdown};

/// The real [`Notifying`]: `sd_notify(3)` on `$NOTIFY_SOCKET`.
///
/// **A notify that fails is a `warn!`, never a failure.** The three sentences this sends are
/// bookkeeping for whoever started the daemon; a `wchd` that could not reach its supervisor's
/// socket — the socket was removed, the datagram did not fit, the manager restarted — still
/// has a camera to serve and a client waiting on it. Turning that into a startup failure or a
/// non-zero exit would be an availability problem converted into a capability one, which is
/// E3 and AGENTS rule 7 stated about a service manager instead of a device.
///
/// **Safe to construct and to use unconditionally**, which is why `main` no longer chooses
/// between this and [`crate::shutdown::Unsupervised`]. `sd_notify::notify` reads
/// `$NOTIFY_SOCKET` on every call and returns `Ok(())` when it is unset, so a `wchd` started
/// from a shell sends nothing, opens nothing, and logs nothing — exactly the behaviour
/// `Unsupervised` was written to provide, with the difference that a daemon started *under* a
/// supervisor now gets the other half. `Unsupervised` stays where it is: it is what the
/// shutdown suite's recording double is compared against, and it is the honest value for a
/// caller that wants to be sure nothing leaves the process.
///
/// A unit struct rather than a value holding a socket, because the crate holds no state: each
/// call connects, sends one datagram, and closes. That costs a `connect` per notification —
/// four per daemon lifetime plus one per watchdog interval — and buys the property that
/// matters here, which is that nothing this type holds can go stale across a supervisor
/// restart.
#[derive(Debug, Clone, Copy, Default)]
pub struct Supervisor;

impl Supervisor {
    /// A supervisor to talk to, whether or not anybody is listening.
    #[must_use]
    pub fn new() -> Supervisor {
        Supervisor
    }
}

impl Notifying for Supervisor {
    fn ready(&self, status: &str) {
        // `READY=1` and the first `STATUS=` in one datagram, because that is one write and
        // one thing that happened: a supervisor that read the readiness and then lost the
        // status would show an operator a service that is "active (running)" with nothing
        // under it.
        tell(&[NotifyState::Ready, NotifyState::Status(status)]);
    }

    fn status(&self, status: &str) {
        tell(&[NotifyState::Status(status)]);
    }

    fn stopping(&self, status: &str) {
        tell(&[NotifyState::Stopping, NotifyState::Status(status)]);
    }
}

/// Send one notification, and survive not being able to.
///
/// One place for the failure policy this module's [`Supervisor`] doc argues, so "an io error
/// here is a `warn`" is a property of a function rather than a rule four call sites remember.
/// The states are rendered into the log line the way the protocol spells them, because an
/// operator reading `journalctl` and an operator reading `sd_notify(3)` should be reading the
/// same words.
fn tell(states: &[NotifyState<'_>]) {
    if let Err(err) = sd_notify::notify(states) {
        let sent: Vec<String> = states.iter().map(ToString::to_string).collect();
        tracing::warn!(
            error = %err,
            notification = %sent.join(" "),
            "could not reach the service manager's notify socket; carrying on serving"
        );
    }
}

/// How often the watchdog task pings, given the interval the service manager set.
///
/// A named function over a value rather than arithmetic inline in the task, for the reason
/// `crate::uds::AcceptFailures` is a type: the *decision* is invisible at the call site, and
/// it is the one thing about the watchdog that can be wrong without anything failing to
/// compile. [`limits::WATCHDOG_PING_DIVISOR`] carries the argument for the number; this
/// carries the one property the division has to have, which is that it cannot answer zero —
/// `tokio::time::interval` panics on a zero period, and a panic on the daemon's startup path
/// is not an available failure mode (the same objection `limits::WS_MESSAGE_BUFFER_CAPACITY`
/// answers with a `const` assertion one crate along).
///
/// A service manager that says `WATCHDOG_USEC=1` is answered with one microsecond rather than
/// zero, which is a busy loop the operator explicitly asked for and not a crash.
#[must_use]
pub fn ping_interval(watchdog: Duration) -> Duration {
    let divided = watchdog / limits::WATCHDOG_PING_DIVISOR;
    if divided.is_zero() {
        Duration::from_micros(1)
    } else {
        divided
    }
}

/// Ping the service manager's watchdog until the daemon is asked to stop.
///
/// `None` when there is no watchdog — `$WATCHDOG_USEC` unset, or set for a different pid
/// (`sd_watchdog_enabled(3)` says `WATCHDOG_PID` decides that, and sd-notify implements it) —
/// which is the ordinary case for every `wchd` that is not under a unit with
/// `WatchdogSec=`. A task that pinged into an unset environment would be a timer the daemon
/// pays for and nobody reads.
///
/// **What a ping proves, stated at its real size.** It says the tokio runtime is still
/// scheduling tasks: the timer fired and something ran. It does not say a camera answers, that
/// the socket is accepting, or that any request has completed — and it must not be read as
/// saying so. Making it mean more would mean health-checking a device, which under D12 means
/// *opening* one, and a daemon that held a camera to prove it was alive would be exactly the
/// complaint D12 exists to answer. [`limits::WATCHDOG_PING_DIVISOR`] carries the same
/// sentence beside the number, because that is where somebody tuning it will be looking.
///
/// The handle is returned rather than detached, and `main` joins it after
/// [`crate::shutdown::serve_until_stopped`] returns. It is not passed *through* that function
/// the way the idle-sweep driver is: `housekeeping` there is D12's driver, singular by
/// signature, and widening it to a list would make "which tasks does a stop join" a question
/// answered in two places. By the time the teardown returns, the token this task watches has
/// been cancelled (step 3 of `crate::shutdown`'s order), so awaiting it in `main` is a join
/// and not a wait.
///
/// Must be called from inside a tokio runtime.
#[must_use]
pub fn spawn_watchdog(shutdown: &Shutdown) -> Option<tokio::task::JoinHandle<()>> {
    spawn_watchdog_every(sd_notify::watchdog_enabled(), shutdown)
}

/// [`spawn_watchdog`], over an interval somebody hands in rather than one read from the
/// environment.
///
/// The seam, and it is a parameter for the reason every seam in this crate is one: the
/// alternative is a test that sets `$WATCHDOG_USEC`, which is `std::env::set_var` — a data
/// race and `unsafe` in Rust 2024 — on a process every other test in the binary shares. So
/// the environment is read in exactly one place, the shipped function above, and both
/// directions of the decision are driven from a value here.
fn spawn_watchdog_every(
    watchdog: Option<Duration>,
    shutdown: &Shutdown,
) -> Option<tokio::task::JoinHandle<()>> {
    let Some(watchdog) = watchdog else {
        tracing::debug!("no service-manager watchdog on this process; not pinging one");
        return None;
    };
    let every = ping_interval(watchdog);
    tracing::info!(
        watchdog_ms = watchdog.as_millis(),
        ping_ms = every.as_millis(),
        divisor = limits::WATCHDOG_PING_DIVISOR,
        "pinging the service manager's watchdog; a ping means this runtime is still \
         scheduling, not that a camera answers"
    );

    let stopping = shutdown.clone();
    Some(tokio::spawn(async move {
        let mut cadence = tokio::time::interval(every);
        // The same decision `crate::server::idle_sweep_cadence` makes and for the same
        // reason: under the default `Burst` behaviour a runtime that was busy for three
        // intervals owes three pings, which would be three datagrams back to back telling a
        // service manager nothing it did not already know from the first.
        cadence.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                () = stopping.cancelled() => break,
                _ = cadence.tick() => tell(&[NotifyState::Watchdog]),
            }
        }
    }))
}

/// Publish how many cameras this machine had when the daemon started.
///
/// **Sent after `READY=1`, and deliberately not part of it.** Readiness means the socket is
/// bound and a client's request will be answered; enumerating cameras is device work — an
/// `open` and a walk of ioctls per node — and a daemon that did it before announcing itself
/// would make its startup time a function of the hardware and would fail to start on a host
/// whose camera is wedged. D12's "cameras open on first use" is the same argument one layer
/// down. So the count arrives second, on a blocking thread, into a `STATUS=` line that
/// replaces the one [`Notifying::ready`] carried.
///
/// **It is a startup fact and the status says so.** Nothing keeps it live, and that is a
/// decision rather than an omission: the hotplug watch (`crate::events::Hotplug`) is
/// subscriber-driven by design — it exists exactly while somebody is subscribed — so keeping
/// this number current would mean either running a watch nobody asked for (a thread and a
/// netlink socket for a line in `systemctl status`) or hanging device enumeration off the
/// idle-sweep cadence, which is the one loop in this daemon that is careful to touch no
/// device at all. Either would buy a status line at the cost of the property D12 is about. An
/// operator who wants the live answer runs `wchc list`, which enumerates when asked.
///
/// **An enumeration that fails becomes a status that says so, never a startup failure.** A
/// machine with no cameras, a `/dev` this user cannot read, a driver that is mid-`modprobe` —
/// all of those are things to tell an operator, and none of them is a reason for a daemon
/// whose socket is already bound and answering to exit. Converting one into the other is the
/// conversion E3 and AGENTS rule 7 forbid.
///
/// The task is not joined, which is the one thing here that is *not* like the rest of this
/// module: it holds nothing, it releases nothing, and it ends by itself in the time an
/// enumeration takes. A runtime that shut down mid-enumeration loses a status line, and the
/// alternative — a handle threaded through the teardown — would make the daemon's stop wait
/// on a device walk it started for a cosmetic reason.
///
/// Must be called from inside a tokio runtime.
pub fn publish_camera_count(wchd: Wchd, notify: Arc<dyn Notifying>) {
    tokio::task::spawn_blocking(move || {
        notify.status(&camera_count_status(&wchd.list_cameras()));
    });
}

/// The `STATUS=` line a startup enumeration turns into, either way it went.
///
/// A pure function over the answer rather than a `match` inside the task, so both endings are
/// assertable without a camera and without a supervisor — and so the *wording* is, which is
/// the whole content of a status line. "at startup" is in the sentence because the number is
/// not live (see [`publish_camera_count`]), and a `systemctl status` that said "2 cameras" an
/// hour after somebody unplugged one would be a lie this module told on purpose.
fn camera_count_status(enumerated: &Result<schema::report::CameraList>) -> String {
    match enumerated {
        Ok(list) => format!(
            "serving; {} camera(s) at startup (not live — `wchc list` enumerates)",
            list.cameras.len()
        ),
        // The typed refusal, rendered. It names what could not be done rather than reporting
        // zero cameras, because "this host has no webcam" and "this daemon cannot look" are
        // different things an operator does different things about (E3).
        Err(error) => format!("serving; the startup camera enumeration failed: {error}"),
    }
}

/// Where the socket the daemon serves on came from.
///
/// Two cases and no third: this daemon serves **one** socket (D11 puts it at
/// `$XDG_RUNTIME_DIR/webcam-handler/wchd.sock` and makes the directory the whole
/// authentication model), so "systemd passed us several" is a refusal rather than a shape to
/// support.
#[derive(Debug)]
pub enum Activation {
    /// Nobody passed a socket. The daemon binds its own, which is D11's ordinary path and
    /// everything `crate::uds::SocketDir` argues.
    Own,
    /// A service manager bound the socket and passed the descriptor in.
    Inherited {
        /// The listener, already registered with this runtime's reactor.
        listener: tokio::net::UnixListener,
        /// Where it is bound, as the kernel reports it — not as anything composed here.
        socket: Utf8PathBuf,
    },
}

impl Activation {
    /// Ask the environment whether a socket was passed in.
    ///
    /// **Called once, from the composition root, before anything else reads the
    /// environment.** `listenfd`'s `from_env` *removes* `LISTEN_FDS` and `LISTEN_PID` as it
    /// reads them — the protocol says a process must not let a child re-adopt its
    /// descriptors — and removing a variable is a write to the process environment. Doing it
    /// as the daemon's first act keeps that write as far away as this design can get it from
    /// the reads `engine::paths` makes a few lines later, and from every thread the runtime
    /// will eventually be running. It is a residual rather than a solved problem: the tokio
    /// worker threads already exist by the time `main`'s body runs, and the only shape that
    /// would remove the write entirely is reading `/proc/self/environ`, i.e. reimplementing
    /// the protocol to avoid a crate that implements it. Recorded here rather than left for a
    /// reader to notice.
    ///
    /// # Errors
    ///
    /// [`Error::DeviceIo`] when more than one descriptor was passed, when the one that was
    /// passed is not a listening `AF_UNIX` stream socket, or when it is bound to an abstract
    /// or unnamed address. [`Error::StorageIo`] when the directory it is bound in is not the
    /// 0700, owned-by-us directory D11 requires — see `Activation::adopt`, which is where
    /// the whole argument about what can and cannot still be checked lives.
    ///
    /// Must be called from inside a tokio runtime: registering the descriptor with the
    /// reactor needs one.
    pub fn from_env() -> Result<Activation> {
        let mut passed = listenfd::ListenFd::from_env();
        let Some(index) = only_socket(passed.len())? else {
            tracing::debug!(
                "no LISTEN_FDS in the environment; this daemon binds its own socket (D11)"
            );
            return Ok(Activation::Own);
        };

        // `Ok(None)` is "that index holds nothing", which cannot happen for an index this
        // function derived from the count; an `Err` is listenfd having *checked* — it
        // `fstat`s the descriptor and asks `getsockname`/`SO_TYPE` whether it really is a
        // listening `AF_UNIX` stream — and that check is most of why this protocol is a
        // dependency rather than twenty lines here. Both are refusals, and both name what
        // was passed, because an operator staring at this line is holding a `.socket` unit
        // with the wrong `ListenDatagram=`/`ListenStream=` in it.
        let inherited = passed
            .take_unix_listener(index)
            .map_err(|err| Error::DeviceIo {
                operation: "adopt the socket the service manager passed in".to_owned(),
                errno: err.raw_os_error(),
                message: format!(
                    "{err} — `wchd` serves one JSON-RPC socket over AF_UNIX (D11), so the unit \
                 that starts it must use ListenStream= on a filesystem path"
                ),
            })?;
        let Some(inherited) = inherited else {
            return Err(Error::DeviceIo {
                operation: "adopt the socket the service manager passed in".to_owned(),
                errno: None,
                message: format!(
                    "LISTEN_FDS announced {} descriptor(s) and descriptor {index} was not \
                     among them",
                    passed.len()
                ),
            });
        };
        Activation::adopt(inherited)
    }

    /// Take an inherited listener, having established that D11 still holds around it.
    ///
    /// Separate from [`Activation::from_env`] so the refusals below can be driven by a test
    /// that binds its own socket — the shape of the environment is systemd's business and the
    /// shape of the *socket* is this daemon's, and only the second is worth a unit test
    /// (`scripts/gates/socket-activation.sh` drives the first, under a real
    /// `systemd-socket-activate`, for note **N44**'s reason).
    ///
    /// ## What D11 still checks here
    ///
    /// D11's authentication model is filesystem permissions on the socket's **directory**:
    /// `connect(2)` needs search permission on every component, and the socket inode is
    /// created `0777 & ~umask` so it is not the boundary. That question is exactly as
    /// answerable about a socket somebody else bound as about one this daemon bound, so it is
    /// asked, with the same function — `crate::uds::check_directory_mode_and_owner`, the one
    /// home for "0700 and `st_uid == geteuid()`" (design §2.10). A second spelling of it here
    /// would be a second opinion about the daemon's whole auth model.
    ///
    /// An **abstract or unnamed** address is refused outright and before any of that. An
    /// abstract-namespace socket (`@name`) has no filesystem presence at all: no directory, no
    /// mode, no owner, and reachable by any process in the network namespace that can spell
    /// the name. There is nothing to check because there is nothing there, so a `wchd` serving
    /// one would be a camera daemon with *no* authentication rather than one whose
    /// authentication this build could not verify.
    ///
    /// ## What it cannot check, which is note **N39**'s defence
    ///
    /// N39's hardening holds the socket directory as a descriptor and binds *relative to it*,
    /// so a directory swapped between the check and the bind cannot move the socket. None of
    /// that is available here, and the reason is simply that **the bind already happened** —
    /// before this process existed, in `systemd`, from a `ListenStream=` string. What is left
    /// is a check on a name: resolve the socket's path from `local_addr()`, open its parent,
    /// and look. A directory substituted between systemd's bind and this open is a window
    /// this build detects nothing about.
    ///
    /// That is not a hole this code can close and it is not left implied: **the unit file is
    /// what makes the directory right**, which is why `packaging/systemd/wchd.socket` carries
    /// `DirectoryMode=0700` and why `scripts/gates/systemd-units.sh` asserts it. The daemon
    /// detects rather than defeats, says which path it is on at startup so an operator can
    /// see it, and refuses when what it can see is wrong.
    fn adopt(inherited: std::os::unix::net::UnixListener) -> Result<Activation> {
        let address = inherited.local_addr().map_err(|err| Error::DeviceIo {
            operation: "ask the kernel where the inherited socket is bound".to_owned(),
            errno: err.raw_os_error(),
            message: err.to_string(),
        })?;
        let Some(path) = address.as_pathname() else {
            return Err(Error::DeviceIo {
                operation: "adopt the socket the service manager passed in".to_owned(),
                errno: None,
                message: format!(
                    "it is bound to {}, and D11 makes the 0700 directory a socket lives in \
                     the whole of this daemon's authentication — an address with no \
                     filesystem presence has no directory, no mode and no owner, so serving \
                     a camera on one would be serving it to every process that can spell the \
                     name. Use ListenStream= with an absolute path",
                    if address.is_unnamed() {
                        "no address at all".to_owned()
                    } else {
                        format!("the abstract namespace ({address:?})")
                    }
                ),
            });
        };
        let socket =
            Utf8PathBuf::from_path_buf(path.to_path_buf()).map_err(|path| Error::DeviceIo {
                operation: "adopt the socket the service manager passed in".to_owned(),
                errno: None,
                message: format!("it is bound at {path:?}, which is not UTF-8"),
            })?;

        let Some(directory) = socket.parent() else {
            return Err(Error::DeviceIo {
                operation: "adopt the socket the service manager passed in".to_owned(),
                errno: None,
                message: format!("{socket} has no parent directory to check the mode of"),
            });
        };
        crate::uds::check_directory_mode_and_owner(directory)?;

        // `from_std` refuses a blocking descriptor, and systemd's is blocking: the manager
        // creates it with `socket(2)` and never sets `O_NONBLOCK`, because a service that
        // wanted one would be the one to know.
        inherited
            .set_nonblocking(true)
            .map_err(|err| storage_io(&socket, &err))?;
        let listener = tokio::net::UnixListener::from_std(inherited)
            .map_err(|err| storage_io(&socket, &err))?;

        tracing::info!(
            socket = %socket,
            "serving the socket the service manager bound and passed in; its directory's \
             mode and owner were checked, but the bind was not ours, so note N39's \
             substitution defence does not apply — the unit's DirectoryMode= is what makes \
             the directory right"
        );
        Ok(Activation::Inherited { listener, socket })
    }
}

/// Which passed descriptor to serve, or a refusal naming how many there were.
///
/// A pure function over the count, because "this daemon serves one socket" is a policy and
/// the two ways to get it wrong (none, several) have nothing to do with descriptors. `None`
/// is not an error: it is the ordinary daemon, started by a person.
///
/// The index is 0 rather than 3 — listenfd's own numbering is relative, so `LISTEN_FDS_FIRST_FD`
/// is its problem and not this module's.
fn only_socket(passed: usize) -> Result<Option<usize>> {
    match passed {
        0 => Ok(None),
        1 => Ok(Some(0)),
        several => Err(Error::DeviceIo {
            operation: "adopt the sockets the service manager passed in".to_owned(),
            errno: None,
            message: format!(
                "LISTEN_FDS announced {several} descriptors and `wchd` serves exactly one \
                 socket (D11) — there is no rule here that would say which of them is the \
                 JSON-RPC socket, and picking the first would be a guess an operator could \
                 not see. Give the daemon one .socket unit with one ListenStream="
            ),
        }),
    }
}

/// Whether stderr already *is* the journal, so the fmt layer would be a second copy.
///
/// systemd documents the test and this implements exactly it: a service whose output goes to
/// the journal is started with `$JOURNAL_STREAM` set to the `device:inode` of that stream, and
/// a process compares it against `fstat` of its own stderr. The comparison is what makes it
/// reliable — the variable alone would be inherited by a child whose stderr had been
/// redirected somewhere else entirely, which is the case it exists to distinguish.
///
/// Everything unreadable is `false`, which is the safe direction: a daemon that wrongly logs
/// to stderr under systemd writes every line twice, and a daemon that wrongly installs the
/// journald layer in a terminal writes to a socket nobody is reading. The first is noise; the
/// second is silence, and AGENTS rule 3 has an opinion about which of those is worse.
#[must_use]
pub fn stderr_is_the_journal() -> bool {
    let Some(declared) = std::env::var_os("JOURNAL_STREAM") else {
        return false;
    };
    let Some(declared) = declared.to_str() else {
        return false;
    };
    let Ok(found) = rustix::fs::fstat(std::io::stderr()) else {
        return false;
    };
    journal_stream_matches(declared, found.st_dev, found.st_ino)
}

/// `$JOURNAL_STREAM`'s `device:inode`, against a stream's own.
///
/// A pure function over three values rather than a body inside [`stderr_is_the_journal`], for
/// the reason every seam in this crate is one: both directions have to be assertable, and
/// *neither* of them can be arranged from a test. Setting `$JOURNAL_STREAM` is
/// `std::env::set_var` — a data race, `unsafe` in Rust 2024, and process-global in a binary
/// every other test shares — and arranging the *matching* case needs a real journal socket on
/// stderr, which is a property of the machine (`scripts/gates/socket-activation.sh` is where
/// the machine-dependent half is driven, with a named counted skip where there is no
/// journal). Over three values, all four interesting inputs are ordinary arguments.
///
/// Decimal on both sides: `sd-daemon(3)`'s `$JOURNAL_STREAM` is "the device and inode numbers
/// of the standard output or standard error … formatted in decimal and separated by a colon".
/// Anything else — no colon, a non-number, extra fields, an empty string — is malformed and
/// answers `false` rather than being guessed at (AGENTS rule 6: represent the unknown, never
/// "correct" it).
#[must_use]
pub fn journal_stream_matches(declared: &str, device: u64, inode: u64) -> bool {
    let Some((left, right)) = declared.split_once(':') else {
        return false;
    };
    let (Ok(left), Ok(right)) = (left.parse::<u64>(), right.parse::<u64>()) else {
        return false;
    };
    (left, right) == (device, inode)
}

/// The journald layer, or the io error that says why there is none.
///
/// Thin on purpose: what is worth arguing is *when* it is installed, and that argument lives
/// in `crate::logging` beside the fmt layer it replaces. What is worth having here is that the
/// crate edge is in the module that owns the systemd protocols, so "who talks to systemd" is
/// still one grep.
///
/// # Errors
///
/// Whatever connecting to `/run/systemd/journal/socket` failed with — a host with no journal,
/// a container that did not bind-mount it, a sandbox that refuses the connect.
pub fn journald_layer() -> std::io::Result<tracing_journald::Layer> {
    tracing_journald::layer()
}

/// A filesystem failure as D13's `StorageIo`, carrying the kernel's errno.
///
/// The same two lines `crate::uds` has, for the same reason and deliberately not shared with
/// it: that one is about paths this daemon composed, this one about a path the kernel reported
/// back for a descriptor somebody else bound.
fn storage_io(path: &Utf8Path, err: &std::io::Error) -> Error {
    Error::StorageIo {
        path: path.to_owned(),
        errno: err.raw_os_error(),
        message: err.to_string(),
    }
}

#[cfg(test)]
mod tests {
    // `from_abstract_name` is Linux's own extension to the address type, which is exactly
    // right for a test whose subject is an address shape only Linux has.
    use std::os::linux::net::SocketAddrExt as _;
    use std::os::unix::net::{SocketAddr, UnixListener};

    use engine::paths::TempRuntimeDir;
    use schema::ErrorKind;
    use schema::report::{CameraList, HintKind, ListHint};

    use super::*;
    use crate::logging::capturing;

    #[test]
    fn the_watchdog_ping_arrives_inside_the_interval_the_manager_set() {
        // The whole of the arithmetic, and the reason it is a function: a build that pinged
        // *at* the interval loses the first race with a busy scheduler and gets the daemon
        // killed, which is the failure mode a watchdog is supposed to prevent rather than
        // cause. Asserted as the relation rather than as the number, so it is
        // `limits::WATCHDOG_PING_DIVISOR` that decides and this test that checks the
        // decision's shape.
        let interval = Duration::from_secs(30);
        assert_eq!(
            ping_interval(interval),
            interval / limits::WATCHDOG_PING_DIVISOR
        );
        assert!(
            ping_interval(interval) < interval,
            "a ping at or past the interval is a killed daemon"
        );

        // The rounding-to-zero end, which is the one that would be a panic rather than a bug:
        // `tokio::time::interval` refuses a zero period, on the daemon's startup path.
        assert!(!ping_interval(Duration::from_micros(1)).is_zero());
        assert!(!ping_interval(Duration::ZERO).is_zero());
    }

    #[tokio::test(start_paused = true)]
    async fn no_watchdog_in_the_environment_is_no_task_at_all() {
        // Both directions of the decision, driven from a value because the input is an
        // environment variable and a test may not write one (see `spawn_watchdog_every`).
        // The `None` arm is what a `wchd` in a terminal gets, and a build that spawned
        // anyway would be paying for a timer whose datagrams go nowhere.
        let shutdown = Shutdown::new();
        assert!(
            spawn_watchdog_every(None, &shutdown).is_none(),
            "a daemon nobody is watching started a watchdog task"
        );

        // …and the other arm really is a task, which really ends on the token. Nothing waits
        // in wall time: the clock is paused, so the join below resolves when the runtime goes
        // idle, and a build whose task ignored the token would hang here into a named nextest
        // TIMEOUT rather than a green test.
        let running =
            spawn_watchdog_every(Some(Duration::from_secs(30)), &shutdown).expect("a watchdog");
        shutdown.cancel();
        running.await.expect("the watchdog task ended cleanly");
    }

    #[test]
    fn a_supervisor_nobody_is_running_is_silent_rather_than_noisy() {
        // The failure policy, driven through the real `sd_notify` call: with `$NOTIFY_SOCKET`
        // unset — which is this test process, and every `wchd` in a terminal — each of the
        // three sentences is a no-op that answers `Ok(())`, so running `Supervisor`
        // unconditionally costs an unsupervised daemon nothing at all. That is the whole
        // argument for `main` no longer choosing between this and `Unsupervised`.
        //
        // The assertion is the *absence* of a warning, and it is what makes the `warn!` in
        // `tell` meaningful: a build that logged on every no-op would put a line in the
        // journal for every watchdog ping, and one that logged nothing at all would hide a
        // supervisor that had actually gone away. The real-delivery half — that these three
        // arrive as datagrams when somebody *is* listening — is `tests/systemd.rs`, which
        // spawns a `wchd` with a notify socket it holds.
        let ((), logged) = capturing(|| {
            let supervisor = Supervisor::new();
            supervisor.ready("serving");
            supervisor.status("still serving");
            supervisor.stopping("stopping");
        });
        assert!(
            !logged.contains("WARN"),
            "an unsupervised daemon warned about its own bookkeeping: {logged:?}"
        );
    }

    #[test]
    fn a_startup_enumeration_that_failed_is_a_status_and_not_a_refusal() {
        // AGENTS rule 7 and E3, at the one place in this module where they can be broken: a
        // daemon whose socket is already bound and answering must not exit because it could
        // not walk `/dev`. Both endings are a `String`, and both name what happened.
        let counted = camera_count_status(&Ok(CameraList {
            cameras: Vec::new(),
            hints: vec![ListHint {
                kind: HintKind::NodeUnreadable,
                subject: "/dev/video0".to_owned(),
            }],
        }));
        assert!(counted.contains('0'), "{counted}");
        assert!(counted.contains("startup"), "{counted}");

        let refused = camera_count_status(&Err(Error::DeviceIo {
            operation: "enumerate cameras".to_owned(),
            errno: Some(13),
            message: "Permission denied".to_owned(),
        }));
        assert!(refused.contains("enumerate cameras"), "{refused}");
        assert!(refused.contains("Permission denied"), "{refused}");
        // And it does not read as "this host has no webcam", which is the conversion the rule
        // forbids: those are different facts an operator does different things about.
        assert!(!refused.contains('0'), "{refused}");
    }

    #[test]
    fn one_socket_is_the_only_number_of_sockets_this_daemon_serves() {
        // The policy, both directions. Zero is a `wchd` a person started and must not be a
        // refusal; one is the unit file; anything else is a refusal that has to name the
        // count, because the operator's next act is to look at a `.socket` unit and count the
        // `ListenStream=` lines in it.
        assert_eq!(only_socket(0), Ok(None));
        assert_eq!(only_socket(1), Ok(Some(0)));

        let refused = only_socket(2).expect_err("this daemon serves one socket");
        assert_eq!(refused.kind(), ErrorKind::DeviceIo);
        assert!(refused.to_string().contains('2'), "{refused}");
        assert!(refused.to_string().contains("ListenStream"), "{refused}");
    }

    #[test]
    fn an_abstract_socket_is_refused_because_there_is_nothing_to_authenticate_with() {
        // The refusal that needs no systemd: an abstract-namespace socket is bindable by this
        // test in two lines, and it is the one shape where D11's authentication model does not
        // merely go unchecked but does not exist — no directory, no mode, no owner, reachable
        // by anything in the network namespace that can spell the name. A build that adopted
        // one would serve a camera to the whole machine and log that it was serving.
        let address =
            SocketAddr::from_abstract_name(b"wch-systemd-tests").expect("a short abstract name");
        let listener = UnixListener::bind_addr(&address).expect("the abstract namespace");

        let refused = Activation::adopt(listener).expect_err("an abstract socket has no directory");
        assert_eq!(refused.kind(), ErrorKind::DeviceIo);
        let rendered = refused.to_string();
        assert!(rendered.contains("abstract"), "{rendered}");
        assert!(rendered.contains("ListenStream"), "{rendered}");
    }

    #[tokio::test]
    async fn an_inherited_socket_in_a_private_directory_is_adopted_and_says_which_path_it_is_on() {
        // The green direction, and the reason the refusals above can go red: a socket bound
        // by somebody else, in a 0700 directory this user owns, *is* served — with a line
        // saying the bind was not ours, so an operator reading the journal can tell the two
        // startup paths apart. Bound here rather than by systemd because what is under test is
        // the adoption; `scripts/gates/socket-activation.sh` is where a real
        // `systemd-socket-activate` drives the same code.
        let runtime = TempRuntimeDir::new().expect("a temporary directory");
        let directory = runtime.base().join("webcam-handler");
        std::fs::create_dir(directory.as_std_path()).expect("ours to make");
        std::fs::set_permissions(
            directory.as_std_path(),
            std::os::unix::fs::PermissionsExt::from_mode(crate::uds::SOCKET_DIR_MODE),
        )
        .expect("ours to chmod");
        let socket = directory.join(schema::limits::DAEMON_SOCKET_FILE);
        let bound = UnixListener::bind(socket.as_std_path()).expect("nothing is in the way");

        let (adopted, logged) =
            capturing(|| Activation::adopt(bound).expect("a private directory"));
        let Activation::Inherited { socket: where_, .. } = adopted else {
            panic!("an inherited socket was reported as a socket this daemon bound");
        };
        assert_eq!(where_, socket);
        assert!(logged.contains("passed in"), "{logged:?}");
        assert!(logged.contains("N39"), "{logged:?}");
    }

    #[tokio::test]
    async fn an_inherited_socket_in_a_directory_anyone_can_walk_into_is_refused() {
        // D11 asked of a socket this daemon did not bind. The check is
        // `crate::uds::check_directory_mode_and_owner`'s — the same one the self-bound path
        // makes — so this arm is about the *reuse* as much as the refusal: a build that
        // skipped it would serve the camera from a directory every account on the machine can
        // reach, having been handed the socket by a unit file with the wrong `DirectoryMode=`.
        let runtime = TempRuntimeDir::new().expect("a temporary directory");
        let directory = runtime.base().join("webcam-handler");
        std::fs::create_dir(directory.as_std_path()).expect("ours to make");
        std::fs::set_permissions(
            directory.as_std_path(),
            std::os::unix::fs::PermissionsExt::from_mode(0o755),
        )
        .expect("ours to chmod");
        let socket = directory.join(schema::limits::DAEMON_SOCKET_FILE);
        let bound = UnixListener::bind(socket.as_std_path()).expect("nothing is in the way");

        let refused = Activation::adopt(bound).expect_err("0755 is not 0700");
        assert_eq!(refused.kind(), ErrorKind::StorageIo);
        let rendered = refused.to_string();
        assert!(rendered.contains("0755"), "{rendered}");
        assert!(rendered.contains("0700"), "{rendered}");
    }

    #[test]
    fn the_journal_stream_comparison_is_about_the_stream_and_not_about_the_variable() {
        // systemd's own test, both directions and the malformed middle. The non-matching case
        // is the one the comparison exists for: `$JOURNAL_STREAM` is inherited by children,
        // so a `wchd` started from a unit whose stderr somebody redirected has the variable
        // and *is not* on the journal — a build that trusted the variable alone would send
        // every line to a socket nobody is reading, which is silence rather than noise.
        assert!(journal_stream_matches("42:1337", 42, 1337));
        assert!(!journal_stream_matches("42:1337", 42, 1338));
        assert!(!journal_stream_matches("42:1337", 43, 1337));

        // Malformed is `false`, never a guess (AGENTS rule 6).
        for malformed in [
            "",
            "42",
            ":",
            "42:",
            ":1337",
            "42:1337:9",
            "0x2a:0x539",
            "42;1337",
            " 42:1337",
            "-1:-1",
        ] {
            assert!(
                !journal_stream_matches(malformed, 42, 1337),
                "{malformed:?} was read as a journal stream"
            );
        }
    }

    #[test]
    fn this_process_is_not_on_the_journal_and_the_daemon_can_tell() {
        // The shipped function, driven for real. `cargo nextest` sets no `$JOURNAL_STREAM` and
        // this process's stderr is a pipe to the harness, so the honest answer is `false`.
        //
        // **What this arm catches, at its real size**: a detection that answers without
        // looking. It cannot catch the more interesting defect — a detection that trusts the
        // variable *being set* — because in this process the variable is not set at all, and
        // a test may not set one (`std::env::set_var` is a data race, `unsafe` in Rust 2024,
        // and process-global in a binary every other test shares). That defect is caught one
        // level out, by `tests/systemd.rs`, which spawns a `wchd` with `$JOURNAL_STREAM` set
        // to a `device:inode` that is not its stderr and asserts the readiness line is still
        // on stderr. Written down here because an assertion whose reach is smaller than its
        // name is how a suite comes to believe it has covered something.
        //
        // It is an assertion rather than a conditional skip: the false branch of "if the
        // variable is set" cannot go red, and a test that cannot go red is not a test
        // (AGENTS, "no assertion inside a conditional whose false branch cannot go red").
        assert!(!stderr_is_the_journal());
    }
}
