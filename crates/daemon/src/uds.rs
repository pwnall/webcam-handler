//! The Unix-socket server glue — **the one piece of transport code we own** (docs/7 P4b).
//!
//! It is *version-coupled to jsonrpsee*, pinned at **0.26.0** in
//! `[workspace.dependencies]` and reached here as the `jsonrpsee-server` package rather
//! than through the facade's `server` feature (note N38 has the measurement that forced
//! that shape).
//!
//! ## What the coupling is, concretely
//!
//! jsonrpsee ships no Unix-socket listener, and it does not need to. What it ships is a
//! tower `Service` over `http::Request`/`http::Response` plus
//! [`jsonrpsee_server::serve_with_graceful_shutdown`], whose `io` parameter is bounded by
//! `AsyncRead + AsyncWrite + Send + Unpin` — its doc comment says "TCP connection", its
//! signature says *any byte stream*. So the whole of "UDS support" is [`serve`]'s accept
//! loop with a [`tokio::net::UnixStream`] in that slot, and five names of jsonrpsee's:
//! [`stop_channel`], [`jsonrpsee_server::StopHandle`], [`ServerHandle`],
//! `Server::builder().set_config(…).to_service_builder()`, and
//! `TowerServiceBuilder::build(methods, stop_handle)`.
//!
//! Because the connection carries HTTP, a caller speaks `POST /` with `content-type:
//! application/json`. That is a consequence of mounting jsonrpsee's own service rather than a
//! choice, and it is the fact P4f's client transport has to be built against: it is an
//! HTTP/1.1 client on a `UnixStream`, not a newline-framed JSON-RPC pipe. The same connection
//! carries a **WebSocket upgrade**, which is how P4e-i's subscriptions reach a
//! `webcam-handler-client` that has no TCP listener to use: jsonrpsee's HTTP path builds
//! `RpcServiceCfg::OnlyCalls`, so a `wch_subscribe_*` over `POST /` is answered `-32603` —
//! calls and subscriptions really are two capabilities over one socket, and [`serve`] enables
//! the second with the two bounds it costs.
//!
//! ## What breaks on a bump, and how it is noticed
//!
//! Design §6's risk register calls jsonrpsee 0.x churn out by name — "0.24 → 0.26 all
//! broke API" — and asks that our glue "fail loudly" on an upgrade. Three layers do that,
//! in the order they fire:
//!
//! 1. **The five names above are a compile failure.** They were renamed, re-signatured or
//!    moved at least once each across 0.24 → 0.26; an upgrade PR does not build.
//! 2. **A behavioural change that still compiles is a red test.** `tests/uds.rs` drives a
//!    real client over a real socket and reads a real JSON-RPC answer back, so a release
//!    that stopped serving plain HTTP `POST` on a non-TCP transport, or whose
//!    [`ServerHandle::stop`] stopped ending this accept loop, or that started answering a
//!    WebSocket upgrade this build declines, goes red rather than shipping.
//! 3. **The bounds this server runs under come from `schema::limits`.**
//!    jsonrpsee's own defaults (10 MB bodies, 100 connections, *unlimited* batches) are
//!    somebody else's numbers for somebody else's deployment, and an unbounded batch on
//!    the one socket the daemon always serves is not a bound this project is allowed to
//!    inherit silently (AGENTS, "Bounded everything"). Five bounds are named here: the
//!    four `ServerConfig` fields the wire runs under and the connection count [`serve`]
//!    enforces itself, because jsonrpsee's `max_connections` is not one — see [`serve`].
//!
//!    **Since P5b the `ServerConfig` is built one module away**, by
//!    [`crate::server::wire_bounds`], because `crate::http::rpc` serves the same `Methods`
//!    over the TCP listener's WebSocket route and two copies of that expression would be two
//!    answers to the same question. The argument for each number stays here, where the
//!    transport that has always carried them is; what moved is the expression, not the law.
//!
//!    What is *not* claimed: `ServerConfig` has nine more fields, and this build inherits
//!    them. Two of the ones that are set are bounds in AGENTS's sense —
//!    `message_buffer_capacity` and
//!    `max_subscriptions_per_connection` — and both govern the WebSocket surface only.
//!    P4b turned that surface **off** rather than ship somebody else's channel depth behind
//!    a transport no test drives; **P4e-i turns it on**, with
//!    [`limits::WS_MESSAGE_BUFFER_CAPACITY`] and
//!    [`limits::RPC_MAX_SUBSCRIPTIONS_PER_CONNECTION`] set here and driven to their bounds
//!    by `tests/subscriptions.rs`. So seven bounds are named in this module now, not five.
//!    `keep_alive_timeout` is inherited and inert: it is hyper's HTTP/2 setting and
//!    this transport is HTTP/1.1 over `AF_UNIX`. Note **N38** records the list, and note
//!    **N57** records what enabling the surface cost.
//!
//! ## The socket directory is the authentication model
//!
//! Design D11 puts the socket at `$XDG_RUNTIME_DIR/webcam-handler/wchd.sock` and makes
//! the **directory** mode 0700 the auth model: on Linux `connect(2)` checks search
//! permission on every path component, and the socket file itself is created with
//! `0777 & ~umask`, so the directory is the boundary and the socket inode is not.
//! [`SocketDir::prepare`] therefore asserts the mode rather than assuming it, and refuses
//! to serve from a directory anyone else can walk into.
//!
//! Because that one check carries the whole auth model, it is made about an **inode** and
//! not about a name — and since note **N39**'s hardening it is made about a *descriptor*,
//! which is the strongest form of that available on Linux. [`SocketDir::prepare`] opens the
//! directory with `SOCKET_DIR_OFLAGS` (so "is a directory" and "is not a symlink" are the
//! kernel's refusals rather than a `lstat` somebody could race — that constant's doc says
//! which flag refuses which, because under `O_PATH` the obvious reading is wrong), `fstat`s
//! *that descriptor*, requires `$XDG_RUNTIME_DIR` to exist rather than creating it, checks
//! the mode **and the owner** against `geteuid()`, and holds the descriptor open for the
//! daemon's life. [`SocketDir::bind`] then binds relative to it. Note **N39** has the
//! measurements that forced each of those, and its 2026-08-10 amendment carries what is
//! still open.
//!
//! ## Why the bind is relative to a descriptor, and how, given that Linux has no `bindat`
//!
//! `bind(2)` takes a `sockaddr_un` whose `sun_path` the kernel resolves from the process's
//! root and cwd. There is no `bindat(2)` to pass a `dirfd` to, in rustix or in libc or in a
//! hand-written syscall — so the literal shape N39 asked for does not exist. Two things do:
//!
//! - `fchdir` then bind a relative name. **Rejected:** the working directory is
//!   process-global, and changing it inside a multi-threaded tokio daemon is a data race
//!   with every other thread's relative path for the duration of the call.
//! - Bind `/proc/self/fd/<dirfd>/wchd.sock`. `/proc/self/fd` entries are *magic links*:
//!   resolution through one jumps to the dentry the descriptor holds instead of re-walking
//!   a stored name. That is the dirfd-relative bind, spelled the way Linux offers it, and
//!   it was measured on this host (note N39's amendment) — a directory swapped for another
//!   between the check and the bind leaves the socket in the **checked** inode, and a
//!   checked directory that is *removed* fails the bind closed with `ENOENT`.
//!
//! The one thing that path needs is `/proc` mounted. When it is not, [`SocketDir::bind`]
//! falls back to binding by name and says so at `warn` naming what is no longer protected —
//! a silent downgrade of an authentication model is worse than the window it hides.

use std::os::fd::{AsFd, AsRawFd, OwnedFd};

use camino::{Utf8Path, Utf8PathBuf};
use engine::store::{LockProtocol, StoreLock};
use jsonrpsee_server::{Methods, Server, ServerHandle, stop_channel};
use rustix::fs::{AtFlags, Mode, OFlags};
use rustix::io::Errno;
use rustix::net::{AddressFamily, SocketAddrUnix, SocketFlags, SocketType};
use schema::limits;
// The mode bits a `stat` carries below the file-type bits, from the crate that owns the
// fact rather than written out again here: `engine::store` masks with the same number for
// the same reason one directory along, and two private copies of one POSIX constant is
// design §2.10's second home even while they agree (note **N150**).
use schema::paths::{Env, MODE_BITS};
use schema::{Error, Result};
use tokio::net::{UnixListener, UnixStream};

/// The mode D11 requires of the directory the socket lives in.
///
/// One home for the number: [`SocketDir::prepare`] creates with it, asserts against it,
/// and names it in the refusal, so "0700" is not written three times and cannot come to
/// mean three things.
pub const SOCKET_DIR_MODE: u32 = 0o700;

/// How the socket directory and the base above it are opened, in one place because the
/// combination is the security property and not a spelling.
///
/// **`O_DIRECTORY` is the flag that refuses a symlink here, and `O_NOFOLLOW` is what makes
/// it do so.** Measured on this host: `O_PATH | O_NOFOLLOW` on a symlink to a directory
/// *succeeds* and hands back a descriptor to the link itself (`st_mode` `0o120777`) —
/// `open(2)` says so explicitly for that pair — so `O_NOFOLLOW` alone is not the guard it
/// looks like under `O_PATH`. Adding `O_DIRECTORY` turns that success into `ENOTDIR`,
/// which is the errno
/// [`tests::a_symlinked_socket_directory_is_refused_however_private_its_target_is`] pins.
/// Neither flag may be dropped: without `O_NOFOLLOW` the open follows the link and checks
/// the target, and without `O_DIRECTORY` it opens the link and checks *that*.
///
/// `O_PATH` asks for no read permission and is enough for `fstat`, `statat`, `mkdirat`,
/// `unlinkat` and the `/proc/self/fd` bind; `O_CLOEXEC` because the daemon spawns nothing
/// that should inherit its socket directory.
const SOCKET_DIR_OFLAGS: OFlags = OFlags::DIRECTORY
    .union(OFlags::NOFOLLOW)
    .union(OFlags::CLOEXEC)
    .union(OFlags::PATH);

/// The runtime directory the daemon serves its socket from, once its mode is known good.
///
/// Holding this value is the evidence that the check happened: [`SocketDir::bind`] is the
/// only way to get a listener out of this module, and it takes `&self`, so a socket
/// cannot be bound in a directory nobody looked at.
///
/// **Not `Clone`**, because an `OwnedFd` is not: the descriptor *is* the checked object, so
/// a copy would either duplicate it or (worse) tempt somebody to rebuild the value from the
/// path and lose the guarantee. Nobody cloned it — `main.rs`, `tests/uds.rs` and
/// `tests/support/fixture.rs` each `prepare` their own — and an `Arc<OwnedFd>` is the
/// answer if that ever changes.
#[derive(Debug)]
pub struct SocketDir {
    path: Utf8PathBuf,
    /// The checked directory itself, held open for the daemon's life.
    ///
    /// A path is a name and names are re-resolved; a descriptor is the object. This is what
    /// [`SocketDir::bind`] binds relative to, so "the directory whose mode and owner were
    /// asserted" and "the directory the socket lands in" are one inode rather than two
    /// readings of one name. Opened `O_PATH`, which asks for no read permission and is
    /// enough for `fstat`, `statat`, `unlinkat` and the `/proc/self/fd` bind.
    dir: OwnedFd,
}

impl SocketDir {
    /// Resolve `$XDG_RUNTIME_DIR/webcam-handler`, create it 0700, and **assert the mode
    /// that came back**.
    ///
    /// The environment arrives as a parameter for `schema::paths`'s reason: reading it
    /// from the process would make the daemon's own tests serialize against every other
    /// test in the binary, because `std::env::set_var` is a data race and `unsafe` in
    /// Rust 2024.
    ///
    /// Creating with mode 0700 is not enough on its own and the read-back is not
    /// ceremony. `mkdir`'s mode is masked by the umask — which can only *clear* bits, so
    /// a directory this call creates is always 0700 — but a directory that already exists
    /// keeps whatever mode it already had, and that is the case D11 is about: an
    /// `$XDG_RUNTIME_DIR/webcam-handler` left 0755 by an older build, or by a tmpfiles
    /// rule, or by an operator, is a camera daemon anybody on the machine can talk to.
    /// This is AGENTS rule 5's read-back doctrine — "requested is not applied" — applied
    /// to a directory instead of a control.
    ///
    /// **A wrong mode is refused, not repaired.** A silent `chmod` would hide the fact
    /// that the directory was reachable, and for however long it was 0755 it may already
    /// have been reached; the operator needs to know that, and D11's posture is to err
    /// closed. The refusal names the directory and the mode found, because "permission
    /// posture wrong" is only actionable if it says what to fix.
    ///
    /// ## Why the check is about a descriptor and not about a path
    ///
    /// `std::fs::metadata` follows symlinks and `DirBuilder::recursive(true)` is happy to
    /// find one where it wanted to create a directory, so a `webcam-handler` that is a
    /// **symlink to** a 0700 directory passed the mode check while the socket was bound
    /// wherever the link pointed — and the link can be re-pointed afterwards. Measured on
    /// this tree, both halves (note N39).
    ///
    /// The repair is not a better `lstat`. Every check made through a *name* is a check on
    /// whatever that name meant at the instant it was made, so the question is only how
    /// small the window is. `SOCKET_DIR_OFLAGS` closes it properly: the open itself
    /// refuses a symlink and a non-directory — `ENOTDIR` from the kernel rather than
    /// something this code has to notice — and the descriptor that comes back **is** the
    /// object, so every later question (mode, owner, what is inside it, where the socket
    /// goes) is asked of it and cannot be answered about something else. *Which* flag does
    /// that work is not obvious and is written down where the flags are: under `O_PATH` it
    /// is `O_DIRECTORY`, and `O_NOFOLLOW` is what leaves it a symlink to refuse.
    ///
    /// ## Why the owner is checked
    ///
    /// `st_uid` against `geteuid()`, which is the check N39 recorded as absent by omission.
    /// The ordinary non-root case is nearly self-refuting — a 0700 directory belonging to
    /// somebody else is one this process cannot traverse, so the bind would fail `EACCES`
    /// anyway — but "nearly" is doing work there: a daemon running as root traverses anything,
    /// so a root `webcam-handler-daemon` pointed at a *user's* `$XDG_RUNTIME_DIR` would
    /// happily serve the camera from a directory that user can replace at will. That is the
    /// case this refuses, and it costs one comparison on a `Stat` already read.
    ///
    /// ## Why `$XDG_RUNTIME_DIR` must already exist
    ///
    /// `runtime_dir`'s own doc says the whole doctrine rests on the platform's promise —
    /// "the only directory the platform promises is per-user, 0700, and cleaned at logout"
    /// — and `recursive(true)` worked around exactly that: a `XDG_RUNTIME_DIR` pointing
    /// at a path that did not exist was silently created, chain and all. That turns "you
    /// are not in a login session" into a directory somewhere under `/tmp`, which is the
    /// one place D11 says the socket may not live. The base is now verified, not made: it
    /// must exist, be a directory, and not be a symlink. Only the `webcam-handler`
    /// component is ever created.
    ///
    /// # Errors
    ///
    /// [`Error::StorageIo`] when `$XDG_RUNTIME_DIR` is unset, empty or relative (that
    /// refusal is `schema::paths`'s and names the variable), when it does not exist or is
    /// not a directory, when the socket directory cannot be created or opened, when it is a
    /// symlink or not a directory, when its mode is not [`SOCKET_DIR_MODE`], or when it is
    /// owned by somebody other than this process's effective user.
    pub fn prepare(env: &dyn Env) -> Result<SocketDir> {
        let path = schema::paths::runtime_dir(env)?;
        // Split into "the directory the platform promised" and "the one component this
        // daemon owns", both off the one path `schema::paths` composed, so neither is a
        // second spelling of `schema::paths::APP_DIR`. A path with no final component is
        // not something `runtime_dir` can produce — it joins `APP_DIR` — so the refusal
        // below is unreachable rather than defensive, and it is written as a refusal
        // instead of an `unwrap` because a panic in the composition root is a daemon that
        // does not start and does not say why.
        let (Some(base), Some(leaf)) = (path.parent(), path.file_name()) else {
            return Err(Error::StorageIo {
                path: path.clone(),
                errno: None,
                message: "has no final component, so there is no directory for the daemon \
                          to own under $XDG_RUNTIME_DIR (D11)"
                    .to_owned(),
            });
        };
        let (base, leaf) = (base.to_owned(), leaf.to_owned());

        // [`SOCKET_DIR_OFLAGS`] is the whole of "exists, is a directory, is not a symlink"
        // — asked of the kernel in the open itself rather than of a `lstat` whose answer
        // could be stale by the next call. Which flag refuses what is stated where the
        // constant is, because under `O_PATH` the obvious reading of `O_NOFOLLOW` is wrong.
        let base_fd = rustix::fs::open(base.as_std_path(), SOCKET_DIR_OFLAGS, Mode::empty())
            .map_err(|errno| Error::StorageIo {
                path: base.clone(),
                errno: Some(errno.raw_os_error()),
                message: format!(
                    "{errno} — $XDG_RUNTIME_DIR names the per-user directory the platform \
                     promises is private and cleaned at logout (D11), so a daemon that \
                     made one would be inventing the promise rather than resting on it; a \
                     missing one means this process is not in a login session, and a \
                     symlinked or non-directory one is not the platform's promise either"
                ),
            })?;

        match rustix::fs::mkdirat(&base_fd, leaf.as_str(), dir_mode()) {
            Ok(()) => {}
            // Already there is the ordinary case and says nothing about the mode, which
            // is what the read-back below is for.
            Err(Errno::EXIST) => {}
            Err(errno) => return Err(errno_io(&path, errno)),
        }

        let dir = rustix::fs::openat(&base_fd, leaf.as_str(), SOCKET_DIR_OFLAGS, Mode::empty())
            .map_err(|errno| Error::StorageIo {
                path: path.clone(),
                errno: Some(errno.raw_os_error()),
                message: format!(
                    "{errno} — is a symlink, or is not a directory, or cannot be opened. \
                     The daemon holds its socket directory as a descriptor and opens it \
                     O_NOFOLLOW | O_DIRECTORY, so a symlink is refused by the kernel as \
                     ENOTDIR rather than followed (a link's target can be re-pointed \
                     between the check and the bind); what is checked is the directory \
                     itself (D11)"
                ),
            })?;

        let found = rustix::fs::fstat(&dir).map_err(|errno| errno_io(&path, errno))?;
        check_mode_and_owner(&path, &found)?;

        Ok(SocketDir { path, dir })
    }

    /// The directory, mode already asserted.
    #[must_use]
    pub fn path(&self) -> &Utf8Path {
        &self.path
    }

    /// Where the socket goes.
    #[must_use]
    pub fn socket_path(&self) -> Utf8PathBuf {
        self.path.join(limits::DAEMON_SOCKET_FILE)
    }

    /// Bind the daemon's socket, replacing a socket file left behind by a dead daemon.
    ///
    /// ## Why the unlink is safe, and why it takes the lock as a parameter
    ///
    /// `bind(2)` on an existing path is `EADDRINUSE` unconditionally — a Unix socket file
    /// is not a lock and outlives the process that made one — so a daemon that never
    /// unlinks cannot restart after any exit that did not clean up, which is still *every*
    /// exit: P4e-ii's shutdown discipline stops the daemon in an order and deliberately does
    /// not add an unlink to it, because the exits that matter here are the ones that ran no
    /// code at all (`SIGKILL`, a power cut) and a cleanup only the orderly path performs is a
    /// cleanup the failing path cannot rely on. Unlinking
    /// something at the socket path is also, in most codebases, how one process hijacks
    /// another's socket. Both facts are answered by the same argument, and it is D9's,
    /// not a new law: the daemon holds the state directory's advisory lock
    /// [`LockProtocol::HeldForLifetime`] for as long as it runs, Linux releases an
    /// `flock` when the holding process dies, so a caller that *has* that lock has
    /// already established that no other daemon is alive for this user — and therefore
    /// that anything at the socket path is stale by construction.
    ///
    /// That is why the lock is a parameter rather than a sentence in this comment: the
    /// ordering (lock, then directory, then unlink, then bind) is the whole safety argument,
    /// and a caller cannot get here without the first step. The protocol is checked because
    /// only the lifetime protocol carries the argument — `webcam-handler-cli`'s per-operation
    /// lock is released moments later and proves nothing about who is running.
    ///
    /// The folklore alternative — connect to the socket and treat `ECONNREFUSED` as
    /// "stale" — is rejected: it is racy (a daemon may be mid-startup), it is weaker (a
    /// wedged daemon holding the socket accepts and never answers), and it asks the
    /// socket a question the lock already answers.
    ///
    /// ## What is not unlinked
    ///
    /// Only a socket. A regular file, a directory, a symlink or a fifo at the socket path
    /// is refused: those are not something this daemon left behind, and deleting an
    /// operator's file because it sits where we want to bind is a data-loss bug wearing a
    /// cleanup routine.
    ///
    /// ## Why the directory is checked again, and what that is now worth
    ///
    /// [`SocketDir::prepare`] read a mode and an owner; this binds a socket. Everything
    /// below happens through [`SocketDir`]'s held descriptor — the `statat` for a leftover
    /// socket, the `unlinkat` that removes it, and the bind itself — so "an attacker who
    /// owns the *parent* renames a directory into place between the two" is no longer a
    /// race this has to win: renaming a name does not move a descriptor, and the socket
    /// lands in the inode whose mode and owner were asserted. Measured, both halves (note
    /// N39's amendment).
    ///
    /// The `fstat` re-check that remains is therefore about the inode's *own* mutability
    /// rather than about which inode it is: a `chmod 0777` on the checked directory between
    /// `prepare` and here is still a refusal, and that is a question about the right object,
    /// which is what N39's bullet list was complaining about.
    ///
    /// ## What [`limits::MAX_UNIX_SOCKET_PATH_BYTES`] protects now
    ///
    /// Not this bind. The address bound is `/proc/self/fd/<n>/wchd.sock`, about 25 bytes,
    /// so it cannot overflow `sun_path` however deep `$XDG_RUNTIME_DIR` is. The check stays
    /// because the **client** connects by the real name, and a socket a client cannot
    /// address is a daemon that starts and serves nobody — so the refusal is on the caller's
    /// behalf and its message says so, or the check reads as dead code to the next reviewer.
    ///
    /// # Errors
    ///
    /// [`Error::StorageIo`] when the lock is not held for the daemon's lifetime, when the
    /// socket directory is no longer at the mode and owner that were asserted, when the
    /// path a client would have to use is longer than the kernel's `sun_path`, when
    /// something that is not a socket already occupies the path, or when the unlink, the
    /// `socket`, the `bind` or the `listen` fails.
    ///
    /// Must be called from inside a tokio runtime: registering the descriptor with the
    /// reactor needs one.
    pub fn bind(&self, held: &StoreLock) -> Result<UnixListener> {
        let socket = self.socket_path();
        self.still_the_directory_that_was_checked()?;

        if held.protocol() != LockProtocol::HeldForLifetime {
            return Err(Error::StorageIo {
                path: socket,
                errno: None,
                message: format!(
                    "the state directory's lock is held as {}, and only {} proves no \
                     other daemon is running — which is the whole reason a leftover \
                     socket here may be removed (D9)",
                    held.protocol(),
                    LockProtocol::HeldForLifetime
                ),
            });
        }

        if socket.as_str().len() > limits::MAX_UNIX_SOCKET_PATH_BYTES {
            return Err(Error::StorageIo {
                path: socket.clone(),
                errno: None,
                message: format!(
                    "is {} bytes long and a Unix socket path may be at most {} — \
                     $XDG_RUNTIME_DIR is too deep for a client to reach a socket under. \
                     The daemon binds through a descriptor and would not have tripped on \
                     the length itself; this refusal is on behalf of the \
                     `webcam-handler-client` that would \
                     have to connect by this name and could not",
                    socket.as_str().len(),
                    limits::MAX_UNIX_SOCKET_PATH_BYTES
                ),
            });
        }

        // Relative to the descriptor, not to the name: what is inspected and unlinked is
        // what is inside the directory whose mode and owner were asserted, whatever the
        // name now leads to.
        match rustix::fs::statat(
            &self.dir,
            limits::DAEMON_SOCKET_FILE,
            AtFlags::SYMLINK_NOFOLLOW,
        ) {
            Ok(existing) if is_socket(&existing) => {
                tracing::info!(
                    socket = %socket,
                    "removing a socket left by a dead daemon; this process holds the state lock, \
                     so no live daemon owns it"
                );
                rustix::fs::unlinkat(&self.dir, limits::DAEMON_SOCKET_FILE, AtFlags::empty())
                    .map_err(|errno| errno_io(&socket, errno))?;
            }
            Ok(_) => {
                return Err(Error::StorageIo {
                    path: socket.clone(),
                    errno: None,
                    message: "is not a socket, so it is not a socket this daemon left \
                              behind; move it aside rather than having the daemon delete it"
                        .to_owned(),
                });
            }
            Err(Errno::NOENT) => {}
            Err(errno) => return Err(errno_io(&socket, errno)),
        }

        let address = self.bind_address(&socket);
        self.bind_listener(&socket, &address)
    }

    /// Which address `bind(2)` will be given, as a value.
    ///
    /// A value and not a `String` because the choice is the security property: the magic
    /// link is what makes the bind land in the inode `prepare` checked, and binding by name
    /// is the downgrade note N39's residual 1 names. Returning the two apart is what lets
    /// [`SocketDir::bind`] warn on exactly one of them, and what lets a test drive the arm a
    /// host with `/proc` mounted can never produce.
    fn bind_address(&self, socket: &Utf8Path) -> BindAddress {
        match self.relative_address() {
            Some(address) => BindAddress::ThroughTheDescriptor(address),
            None => BindAddress::ByName(socket.to_string()),
        }
    }

    /// Create, bind and listen on the socket at `address`.
    ///
    /// Ours rather than `tokio::net::UnixListener::bind`'s because the address has to be
    /// composed from a descriptor — see this module's header for why `/proc/self/fd` *is*
    /// the dirfd-relative bind on Linux, and why `fchdir` is not. `SocketFlags::NONBLOCK`
    /// is set at creation because `tokio::net::UnixListener::from_std` checks it and
    /// refuses a blocking descriptor (`tokio-1.53.1/src/net/unix/listener.rs`), and
    /// `CLOEXEC` because this process spawns nothing that should inherit the daemon's
    /// listening socket.
    ///
    /// `address` is a parameter rather than something this reads for itself, for the reason
    /// [`Accepting`] is a trait: the fallback arm cannot be produced on a host that has
    /// `/proc` mounted, and a branch no test can reach is a branch that is only correct
    /// until somebody edits it. `socket` stays alongside it because it is what the *errors*
    /// name — an operator needs the path they configured, not `/proc/self/fd/7/wchd.sock`.
    fn bind_listener(&self, socket: &Utf8Path, address: &BindAddress) -> Result<UnixListener> {
        // Said here rather than at the decision, so the warning and the bind it describes
        // cannot come apart: a downgraded bind is exactly the set of binds that announced
        // themselves (note N39's residual 1 — "never a silent downgrade").
        if let BindAddress::ByName(_) = address {
            tracing::warn!(
                socket = %socket,
                "/proc is not mounted, so the socket is bound by name rather than through \
                 the checked directory's descriptor; the directory's mode and owner were \
                 still asserted, but the window between that check and the bind is open"
            );
        }
        let address =
            SocketAddrUnix::new(address.as_str()).map_err(|errno| errno_io(socket, errno))?;

        let listener = rustix::net::socket_with(
            AddressFamily::UNIX,
            SocketType::STREAM,
            SocketFlags::CLOEXEC | SocketFlags::NONBLOCK,
            None,
        )
        .map_err(|errno| errno_io(socket, errno))?;
        rustix::net::bind(&listener, &address).map_err(|errno| errno_io(socket, errno))?;
        rustix::net::listen(&listener, limits::DAEMON_LISTEN_BACKLOG)
            .map_err(|errno| errno_io(socket, errno))?;

        let listener = std::os::unix::net::UnixListener::from(listener);
        UnixListener::from_std(listener).map_err(|err| storage_io(socket, &err))
    }

    /// `/proc/self/fd/<dirfd>/wchd.sock`, or `None` when `/proc` is not mounted.
    ///
    /// Asked here rather than assumed, because the answer decides whether the bind is
    /// protected: a container without procfs gets today's behaviour and a `warn!` naming
    /// what is not being protected (note N39's residual 1), never a silent downgrade.
    fn relative_address(&self) -> Option<String> {
        let magic = format!(
            "/proc/self/fd/{}/{}",
            self.dir.as_fd().as_raw_fd(),
            limits::DAEMON_SOCKET_FILE
        );
        // The directory the magic link lives in, not the socket: the socket is what is
        // about to be created, so its absence is the ordinary case and says nothing about
        // whether procfs is there.
        let parent = format!("/proc/self/fd/{}", self.dir.as_fd().as_raw_fd());
        rustix::fs::statat(rustix::fs::CWD, parent.as_str(), AtFlags::empty())
            .ok()
            .map(|_| magic)
    }

    /// Refuse unless the checked directory is still at the mode and owner it was checked at.
    ///
    /// Asked of the *descriptor*, so this is no longer "is the name still the same object" —
    /// the descriptor answers that by construction. What it catches is the inode changing
    /// under itself: a `chmod` or a `chown` between [`SocketDir::prepare`] and
    /// [`SocketDir::bind`] leaves the socket's authentication model weaker than the one that
    /// was asserted, and D11's posture is to err closed.
    ///
    /// **There is no `(st_dev, st_ino)` comparison here, deliberately.** Before N39's
    /// hardening the identity was re-read and compared, because binding by *name* could
    /// land in a different object; since the bind goes through this descriptor, substitution
    /// is defeated structurally rather than detected — and two `fstat`s of one open
    /// descriptor cannot disagree, so a comparison between them is an arm no input reaches
    /// and no test can turn red (rubric A8). The two checks that remain both have a red
    /// direction, driven by
    /// [`tests::a_socket_directory_re_permissioned_between_the_check_and_the_bind_is_refused`]
    /// and [`tests::a_socket_directory_owned_by_somebody_else_is_refused`].
    fn still_the_directory_that_was_checked(&self) -> Result<()> {
        let found = rustix::fs::fstat(&self.dir).map_err(|errno| errno_io(&self.path, errno))?;
        check_mode_and_owner(&self.path, &found)
    }
}

/// The address the daemon's socket is bound at, and how much protection that spelling
/// carries.
///
/// See [`SocketDir::bind_address`]. The distinction exists so the downgrade cannot happen
/// silently: the two spellings are different values, `bind` warns on one of them, and a
/// test can hand either to [`SocketDir::bind_listener`].
#[derive(Debug, Clone, PartialEq, Eq)]
enum BindAddress {
    /// `/proc/self/fd/<dirfd>/wchd.sock` — the dirfd-relative bind, spelled the way Linux
    /// offers it. The socket lands in the inode whose mode and owner were asserted.
    ThroughTheDescriptor(String),
    /// The socket's real path. `/proc` is not mounted, so the check-to-bind window that
    /// note N39 closed is open again on this host.
    ByName(String),
}

impl BindAddress {
    /// The address as `bind(2)` wants it.
    fn as_str(&self) -> &str {
        match self {
            BindAddress::ThroughTheDescriptor(address) | BindAddress::ByName(address) => address,
        }
    }
}

/// Whether a `stat` describes a socket.
///
/// Through rustix's own `S_IFMT` decode rather than a mask written here, because
/// `std::os::unix::fs::FileTypeExt` wants a `std::fs::Metadata` this code deliberately no
/// longer has — everything is asked of the descriptor now.
fn is_socket(found: &rustix::fs::Stat) -> bool {
    rustix::fs::FileType::from_raw_mode(found.st_mode) == rustix::fs::FileType::Socket
}

/// The mode [`SocketDir::prepare`] creates the directory with.
fn dir_mode() -> Mode {
    Mode::from_bits_truncate(SOCKET_DIR_MODE)
}

/// D11's two facts about the socket directory, asked of one `stat` in one place.
///
/// One home for both, because [`SocketDir::prepare`] and [`SocketDir::bind`] ask exactly
/// the same question of exactly the same descriptor and a second copy would be a second
/// opinion (design §2.10).
///
/// **Exact equality here, a subset one directory along, and the difference is the question**
/// (note **N150**). `engine::store::check_state_dir` guards D9's session tree and asks only
/// `paths::GROUP_AND_OTHER_BITS` — who other than the owner can reach the frames — so there a
/// *narrower* mode passes and an inherited `S_ISGID` is ignored. D11 makes **this**
/// directory's mode the entire authentication model for a socket with no token and no
/// peer-credential check, so anything that is not `SOCKET_DIR_MODE` is wrong here whichever
/// way it differs. That function's doc carries the other end of this sentence, because a
/// reader who meets either check first should not have to guess the other one exists.
///
/// The residue that divergence leaves is named rather than assumed away: [`SocketDir::prepare`]
/// `mkdirat`s and then arrives here, so a `$XDG_RUNTIME_DIR` carrying the set-group bit would
/// make this daemon refuse the directory it had just created — N150's break, one directory
/// along. It stays exact anyway. `$XDG_RUNTIME_DIR` is `/run/user/<uid>`, made 0700 by the
/// login manager rather than by a site's group policy, so the setgid arrangement that is
/// ordinary on a shared *home* does not occur on it; and if it ever did, a daemon whose whole
/// authentication is this mode word is a daemon that should say so rather than serve.
fn check_mode_and_owner(path: &Utf8Path, found: &rustix::fs::Stat) -> Result<()> {
    let mode = found.st_mode & MODE_BITS;
    if mode != SOCKET_DIR_MODE {
        return Err(Error::StorageIo {
            path: path.to_owned(),
            errno: None,
            message: format!(
                "is mode {mode:04o}, and the daemon's socket directory must be \
                 {SOCKET_DIR_MODE:04o} — filesystem permissions are the only thing \
                 authenticating this socket (D11), so serving from a directory \
                 another account can walk into would be serving the camera to them; \
                 fix the mode and start again"
            ),
        });
    }

    let owner = found.st_uid;
    let ours = rustix::process::geteuid().as_raw();
    if owner != ours {
        return Err(Error::StorageIo {
            path: path.to_owned(),
            errno: None,
            message: format!(
                "is owned by uid {owner} and this daemon runs as uid {ours} — a directory \
                 somebody else owns is one they can replace or re-permission under a \
                 running daemon, and filesystem permissions are the only thing \
                 authenticating this socket (D11). A root daemon traverses a 0700 \
                 directory belonging to a user without noticing, which is exactly the case \
                 this refuses"
            ),
        });
    }
    Ok(())
}

/// D11's two facts, asked about a directory this daemon did not open for itself.
///
/// [`SocketDir::prepare`] is the strong form of this: it opens the directory once, holds the
/// descriptor for the daemon's life, and binds *relative to it*, so what was checked and what
/// is served from are one inode rather than one name (note **N39**). That shape is available
/// only to a process that does its own bind, and since P4e-ii there is a second startup path
/// where the bind already happened somewhere else — `crate::systemd::Activation::adopt`, where
/// a service manager passes a socket in. The question "is the directory 0700 and ours" is
/// exactly as meaningful there, and [`check_mode_and_owner`] is its one home (design §2.10),
/// so this is how the other caller reaches it rather than spelling the mode and the `geteuid`
/// comparison a second time.
///
/// What it deliberately does **not** claim is N39's substitution defence. The open here is by
/// name, because a name is all an inherited socket's `local_addr()` gives; `SOCKET_DIR_OFLAGS`
/// still makes "is a directory, is not a symlink" the kernel's refusal, and that is the whole
/// of what a name can buy. `crate::systemd::Activation::adopt` states the residual and names
/// what closes it, which is the unit file's `DirectoryMode=`.
///
/// # Errors
///
/// [`Error::StorageIo`] when the directory cannot be opened, is a symlink or is not a
/// directory, is not [`SOCKET_DIR_MODE`], or is owned by somebody other than this process's
/// effective user.
pub(crate) fn check_directory_mode_and_owner(path: &Utf8Path) -> Result<()> {
    let dir = rustix::fs::open(path.as_std_path(), SOCKET_DIR_OFLAGS, Mode::empty()).map_err(
        |errno| Error::StorageIo {
            path: path.to_owned(),
            errno: Some(errno.raw_os_error()),
            message: format!(
                "{errno} — is a symlink, or is not a directory, or cannot be opened. \
                 Filesystem permissions on this directory are the whole of what \
                 authenticates the daemon's socket (D11), so a socket inside one this \
                 process cannot even look at is not one it will serve"
            ),
        },
    )?;
    let found = rustix::fs::fstat(&dir).map_err(|errno| errno_io(path, errno))?;
    check_mode_and_owner(path, &found)
}

/// A running server, and the reason it will eventually stop.
///
/// A value rather than a bare [`ServerHandle`] because *why* the accept loop ended is a fact
/// the composition root has to act on. jsonrpsee's `ServerHandle::stopped` is
/// `watch::Sender::closed()` — it resolves when the last receiver is dropped, which the accept
/// loop dropping its own would do — so a loop that gave up would otherwise look exactly like a
/// clean stop, and `webcam-handler-daemon` would exit `SUCCESS` after announcing that it had
/// stopped accepting connections. A supervisor reads that as "the service completed", and
/// `Restart=on-failure` declines to restart it.
#[derive(Debug)]
#[must_use = "a server nobody waits on is a daemon that exits as soon as it starts"]
pub struct Serving {
    handle: ServerHandle,
    /// The accept loop, until somebody has taken its answer. `None` afterwards, because a
    /// [`tokio::task::JoinHandle`] polled after it has completed **panics**, and since
    /// P4e-ii [`Serving::stopped`] is called twice on the ordinary path — once as a
    /// `select!` arm waiting for a stop, once as the drain that bounds it.
    accepting: Option<tokio::task::JoinHandle<Result<()>>>,
    /// What the loop answered, kept so that answer survives being asked for twice.
    ///
    /// `Ok(())` until it has answered, which is not a placeholder: a loop that is still
    /// running has refused nothing, and the only caller that reads this before then is one
    /// whose own `select!` arm is about to be cancelled.
    verdict: Result<()>,
}

impl Serving {
    /// Ask the server to stop. Idempotent from the caller's point of view.
    ///
    /// Ending the accept loop is all this does — in-flight connections finish their current
    /// answer and end. It is not the drain and it is not the cancellation: `daemon::shutdown`
    /// is where the order those three go in lives, and this is step four of it.
    pub fn stop(&self) {
        // `AlreadyStoppedError` means somebody already asked, including the accept loop
        // itself on its give-up path. That is the outcome this call wanted.
        let _ = self.handle.stop();
    }

    /// Wait until the server has stopped, and answer why.
    ///
    /// `Ok(())` for a stop somebody asked for; the accept loop's own refusal otherwise.
    ///
    /// ## Cancel-safe, and asked twice on purpose
    ///
    /// `daemon::shutdown::serve_until_stopped` races this against a signal and then, having
    /// stopped the server, awaits it again under [`limits::DAEMON_SHUTDOWN_DRAIN_MS`]. Both
    /// halves of that need something this originally did not have. A `JoinHandle` that has
    /// already yielded its value panics when it is polled again, and a `select!` arm is
    /// *dropped* wherever it happened to be — including between the two awaits below, where
    /// the accept loop's answer had been taken and the connections had not yet gone. So the
    /// handle is taken out of the way and the answer is kept: dropping this future mid-flight
    /// loses nothing, and asking twice is the same question with the same answer rather than
    /// a panic on the daemon's stopping path.
    ///
    /// # Errors
    ///
    /// [`Error::DeviceIo`] when the loop gave up after
    /// [`limits::MAX_CONSECUTIVE_ACCEPT_FAILURES`] consecutive `accept` failures, carrying
    /// the last one's errno, or when the accept task itself panicked or was cancelled.
    pub async fn stopped(&mut self) -> Result<()> {
        if let Some(accepting) = self.accepting.as_mut() {
            let reason = match accepting.await {
                Ok(reason) => reason,
                Err(err) => Err(Error::DeviceIo {
                    operation: "accept connections on the daemon socket".to_owned(),
                    errno: None,
                    message: err.to_string(),
                }),
            };
            // Only after the await has produced a value, so a cancellation before it leaves
            // the handle exactly where the next call expects to find it.
            self.accepting = None;
            self.verdict = reason;
        }
        // Then the connections, which is what `ServerHandle::stopped` is for. The accept
        // loop signals a stop on its way out, so this cannot wait forever on a client
        // holding an idle keep-alive connection open.
        self.handle.clone().stopped().await;
        self.verdict.clone()
    }
}

/// Where a connection comes from — the seam under [`serve`]'s accept loop.
///
/// One method, and it exists because the loop's most important behaviour is what it does
/// when `accept` *fails*: giving up after a run of failures is the difference between a
/// daemon that spins at 100% of a core on an `EMFILE` that will not clear and one that
/// stops and says so. Arranging sixty-four real `EMFILE`s in a test would mean lowering
/// this process's descriptor limit, which is a syscall this workspace does not link and a
/// global this workspace's tests may not touch — so the listener is a parameter, with the
/// real `UnixListener` and a scriptable double, the way every other seam here is.
trait Accepting: Send + 'static {
    /// The next connection, or the error `accept(2)` gave.
    fn accept(&self) -> impl Future<Output = std::io::Result<UnixStream>> + Send;
}

impl Accepting for UnixListener {
    async fn accept(&self) -> std::io::Result<UnixStream> {
        UnixListener::accept(self)
            .await
            .map(|(stream, _peer)| stream)
    }
}

/// Serve `methods` over `listener` until the returned value is stopped.
///
/// Returns immediately; the accept loop runs as a tokio task, so the caller must already
/// be inside a runtime. [`Serving::stop`] ends the loop and [`Serving::stopped`] resolves
/// once it and every connection it spawned are gone — which is the whole of this module's
/// lifecycle and is deliberately less than the daemon's: nothing here handles a signal,
/// bounds the wait, or unlinks the socket on the way out. `daemon::shutdown` is where those
/// live, and it drives this pair as steps four and five of an order. The socket file is left
/// where it is on purpose; [`SocketDir::bind`] is what makes that harmless.
///
/// jsonrpsee spells the per-connection future `serve_with_graceful_shutdown`, and its
/// "graceful" is about *one connection's* in-flight request finishing rather than being
/// dropped mid-answer. It is still not this daemon claiming a drain, and the distinction
/// survives P4e-ii: what a subscription gets when the daemon stops is a **cancellation with
/// a reason** from `daemon::shutdown`'s token, delivered before this transport is stopped at
/// all, and no name in this module should be read as standing in for that.
///
/// ## The connection bound is this loop's, not jsonrpsee's
///
/// `ServerConfig::max_connections` sounds like the bound `limits::DAEMON_MAX_CONNECTIONS`
/// documents — "so a client that leaks connections is refused rather than able to exhaust
/// the daemon's file descriptors, which on this process are also the camera's" — and it is
/// not. jsonrpsee acquires that permit inside `TowerService::call`, per HTTP *request*, and
/// releases it when the response is written (`jsonrpsee-server-0.26.0/src/server.rs`), so
/// an accepted connection that never sends a byte consumes none of it and holds a
/// descriptor forever. Measured: with the cap at 32, 128 idle connections were all
/// accepted. So the permit is taken **here**, at accept, and held for the connection's
/// life; the config's own cap is left set as well, where it bounds concurrent requests.
///
/// ## WebSocket upgrades, and the two bounds they cost
///
/// `enable_ws` defaults to on, and with it come two of jsonrpsee's numbers: 1024 for
/// `message_buffer_capacity` (a channel depth, which AGENTS says lives in `schema::limits`)
/// and 1024 for `max_subscriptions_per_connection`. P4b left the surface **off** because
/// nothing subscribed and no test drove an upgrade, which would have been shipping an
/// unbounded, untested transport for a consumer that did not exist yet (rubric A8, note
/// N38).
///
/// P4e-i is that consumer, so the surface is on and both numbers are this project's
/// ([`crate::server::wire_bounds`], which is where the expression lives now that two
/// transports read it): [`limits::WS_MESSAGE_BUFFER_CAPACITY`] bounds what one subscription
/// may hold unwritten
/// before the daemon drops and counts, and [`limits::RPC_MAX_SUBSCRIPTIONS_PER_CONNECTION`]
/// bounds how many streams one connection may open — refused as a `-32006` answer to the
/// *subscribe call*, before any handler runs, so connect-and-abandon costs a client its own
/// slots and nobody else's. `crate::events` is where what happens at the first bound is
/// argued; `tests/subscriptions.rs` drives both.
///
/// P5b adds the second consumer and changes nothing here: `crate::http::rpc` mounts the same
/// `Methods` on the TCP listener, so a browser's WebSocket runs under the same two numbers,
/// through the same `enable_ws`, with a `ConnectionGuard` of its own.
///
/// **What is deliberately still inherited:** `ping_config` is `None`, so a peer that opens
/// a WebSocket, subscribes and then never reads again is never reaped by an inactivity
/// timer. That is bounded rather than unbounded — [`limits::DAEMON_MAX_CONNECTIONS`]
/// connections times [`limits::WS_MESSAGE_BUFFER_CAPACITY`] messages, and the fan-out in
/// front of it never waits on any of them — and it is left alone on purpose: turning
/// `enable_ws_ping` on adds two constants whose behavioural half cannot be asserted without
/// waiting out a timer, which is the shape AGENTS bans. Note **N57** records it as the
/// residual.
pub fn serve(listener: UnixListener, methods: impl Into<Methods>) -> Serving {
    serve_accepting(listener, methods)
}

/// [`serve`], over anything that accepts connections. See [`Accepting`].
fn serve_accepting<L: Accepting>(listener: L, methods: impl Into<Methods>) -> Serving {
    let methods: Methods = methods.into();
    let (stop_handle, server_handle) = stop_channel();

    // The six bounds, from the one function both transports read them through since P5b
    // (`crate::server::wire_bounds`) — this module's header still argues *why* each of them
    // is this project's number rather than jsonrpsee's, and that argument is the reason the
    // function exists rather than a second copy of the expression.
    let service_builder = Server::builder()
        .set_config(crate::server::wire_bounds())
        .to_service_builder();

    // One permit per accepted connection, held for its whole life — see this function's
    // doc for why jsonrpsee's cap of the same name is a different bound.
    let connections = std::sync::Arc::new(tokio::sync::Semaphore::new(
        usize::try_from(limits::DAEMON_MAX_CONNECTIONS).unwrap_or(usize::MAX),
    ));
    let ending = server_handle.clone();

    let accepting = tokio::spawn(async move {
        let mut failures = AcceptFailures::none();
        loop {
            let stream = tokio::select! {
                accepted = listener.accept() => match accepted {
                    Ok(stream) => {
                        failures.succeeded();
                        stream
                    }
                    Err(err) => {
                        if failures.failed() {
                            tracing::warn!(error = %err, "a connection could not be accepted");
                            continue;
                        }
                        tracing::error!(
                            error = %err,
                            limit = limits::MAX_CONSECUTIVE_ACCEPT_FAILURES,
                            "too many consecutive accept failures; no longer accepting connections"
                        );
                        // The connections that are still up are told, so that whoever is
                        // waiting on this server is not left waiting on a keep-alive that
                        // nothing will ever end.
                        let _ = ending.stop();
                        return Err(Error::DeviceIo {
                            operation: "accept a connection on the daemon socket".to_owned(),
                            errno: err.raw_os_error(),
                            message: format!(
                                "{} consecutive failures, the last of them: {err}",
                                limits::MAX_CONSECUTIVE_ACCEPT_FAILURES
                            ),
                        });
                    }
                },
                // A fresh clone inherits the original's unseen state, so a stop that
                // arrived while we were blocked in `accept` is still pending here.
                () = stop_handle.clone().shutdown() => return Ok(()),
            };

            let Ok(permit) = std::sync::Arc::clone(&connections).try_acquire_owned() else {
                // Dropped, which closes it: a client past the bound is refused rather
                // than queued, because a queue of connections is the descriptor
                // exhaustion the bound exists to prevent.
                drop(stream);
                tracing::warn!(
                    limit = limits::DAEMON_MAX_CONNECTIONS,
                    "refused a connection: this daemon serves at most that many at once"
                );
                continue;
            };

            // `conn_id` lives behind an `Arc<AtomicU32>` inside the builder, so the clone
            // still hands out distinct connection ids.
            let service = service_builder
                .clone()
                .build(methods.clone(), stop_handle.clone());
            let stopped = stop_handle.clone();
            tokio::spawn(async move {
                if let Err(err) = jsonrpsee_server::serve_with_graceful_shutdown(
                    stream,
                    service,
                    stopped.shutdown(),
                )
                .await
                {
                    // Client-side hang-ups land here and are ordinary. Nothing about a
                    // connection is worth an operator's attention at the default level.
                    tracing::debug!(error = %err, "a connection ended with an error");
                }
                // Released here and nowhere else, so the bound counts connections that
                // are still up rather than connections that were ever made.
                drop(permit);
            });
        }
    });

    Serving {
        handle: server_handle,
        accepting: Some(accepting),
        verdict: Ok(()),
    }
}

/// Consecutive `accept` failures, as a value.
///
/// A fold rather than a counter inline in the loop, so both of its answers can be
/// asserted without arranging 64 real `EMFILE`s — the engine's rule ("pure cores take
/// values") applied to the one piece of policy the accept loop has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AcceptFailures(u32);

impl AcceptFailures {
    /// No failures yet.
    const fn none() -> AcceptFailures {
        AcceptFailures(0)
    }

    /// Record a failure; `true` when accepting should continue.
    ///
    /// The Nth consecutive failure is the last one tolerated, where N is
    /// [`limits::MAX_CONSECUTIVE_ACCEPT_FAILURES`].
    fn failed(&mut self) -> bool {
        self.0 = self.0.saturating_add(1);
        self.0 < limits::MAX_CONSECUTIVE_ACCEPT_FAILURES
    }

    /// Record a success. Failures are only interesting in a row: one `ECONNABORTED`
    /// between two good connections says nothing about the daemon.
    fn succeeded(&mut self) {
        self.0 = 0;
    }
}

/// A filesystem failure as D13's `StorageIo`, carrying the kernel's errno.
fn storage_io(path: &Utf8Path, err: &std::io::Error) -> Error {
    Error::StorageIo {
        path: path.to_owned(),
        errno: err.raw_os_error(),
        message: err.to_string(),
    }
}

/// The same, for the syscalls that come back as a rustix [`Errno`] rather than an
/// [`std::io::Error`].
///
/// A second spelling of one law would be a finding; this is the same law reached through a
/// different error type, and it exists so `errno` is carried rather than flattened into the
/// message — the D13 registry has a field for it and a caller that has to parse a string to
/// find `ENOENT` is a caller nobody gave an errno to.
fn errno_io(path: &Utf8Path, errno: Errno) -> Error {
    Error::StorageIo {
        path: path.to_owned(),
        errno: Some(errno.raw_os_error()),
        message: std::io::Error::from(errno).to_string(),
    }
}

#[cfg(test)]
mod tests {
    // The tests still read and set modes through `std::fs`, deliberately: the subject is
    // what `SocketDir` does to a directory on disk, and asking through the same API the
    // code under test uses would let a shared misreading of `st_mode` pass twice.
    use std::os::unix::fs::PermissionsExt;

    use engine::paths::TempRuntimeDir;
    use engine::store::TempStore;
    use jsonrpsee_server::RpcModule;
    use schema::ErrorKind;
    use schema::paths::MapEnv;

    use super::*;
    // The one writer this crate's tests read a `tracing` line back through; `crate::logging`
    // states why it has one home.
    use crate::logging::capturing;

    /// A held lifetime lock over a throw-away state directory.
    ///
    /// [`SocketDir::bind`] takes one by signature, so every test that binds has to say
    /// out loud that it is a daemon — which is the point of the parameter.
    fn daemon_lock(store: &TempStore) -> StoreLock {
        store
            .store()
            .lock(LockProtocol::HeldForLifetime)
            .expect("an unlocked state directory")
    }

    #[test]
    fn the_socket_directory_is_created_private() {
        // The mode is asserted off the filesystem rather than off the call that set it:
        // `mkdir`'s mode argument is masked by the umask, and a test that trusted the
        // argument would pass under a umask that cleared the bits.
        let runtime = TempRuntimeDir::new().expect("a temporary directory");
        let dir = SocketDir::prepare(&runtime.env()).expect("a fresh runtime directory");

        let mode = std::fs::metadata(dir.path().as_std_path())
            .expect("the directory was just created")
            .permissions()
            .mode()
            & MODE_BITS;
        assert_eq!(mode, SOCKET_DIR_MODE, "{:04o}", mode);
        assert!(dir.path().ends_with("webcam-handler"), "{}", dir.path());
    }

    #[test]
    fn a_group_readable_socket_directory_is_refused_rather_than_repaired() {
        // The other direction, and the one docs/9's UDS-permissions row exists for: a
        // pre-existing directory keeps its own mode, so `create` alone proves nothing.
        // 0770 rather than 0777 because it is the mode a helpful `chmod g+rwx` leaves,
        // which is how this happens in the field.
        let runtime = TempRuntimeDir::new().expect("a temporary directory");
        let existing = runtime.base().join("webcam-handler");
        std::fs::DirBuilder::new()
            .create(existing.as_std_path())
            .expect("a directory nobody has looked at yet");
        // `chmod`, not `mkdir`'s mode argument: `mkdir(2)` masks its mode with the umask,
        // which is the very fact the code under test is about, so a fixture that seeded
        // the mode that way would be seeding *the umask* — 0750 under the distributions'
        // default 022, 0700 under 077 — and this test would be asserting the developer's
        // shell rather than the daemon. `set_permissions` is not masked.
        std::fs::set_permissions(
            existing.as_std_path(),
            std::fs::Permissions::from_mode(0o770),
        )
        .expect("ours to chmod");
        assert_eq!(
            mode_of(&existing),
            0o770,
            "the fixture could not seed the mode this test is about"
        );

        let err = SocketDir::prepare(&runtime.env()).expect_err("0770 is not 0700");
        assert_eq!(err.kind(), ErrorKind::StorageIo);
        let rendered = err.to_string();
        // The refusal has to name what was found and what is wanted, or an operator
        // cannot act on it.
        assert!(rendered.contains("0770"), "{rendered}");
        assert!(rendered.contains("0700"), "{rendered}");

        // And it is a refusal, not a repair: the directory is exactly as it was.
        assert_eq!(mode_of(&existing), 0o770);
    }

    /// A directory's own permission bits, without following a link to somewhere else.
    fn mode_of(path: &Utf8Path) -> u32 {
        std::fs::symlink_metadata(path.as_std_path())
            .unwrap_or_else(|err| panic!("{path} cannot be read: {err}"))
            .permissions()
            .mode()
            & MODE_BITS
    }

    #[test]
    fn a_symlinked_socket_directory_is_refused_however_private_its_target_is() {
        // The hole this check exists to close, and the reason the directory is opened
        // `O_NOFOLLOW`. `create_dir_all` is happy to find a symlink where it wanted a
        // directory (it falls back to `is_dir()`, which follows), and `metadata` reports
        // the *target's* mode — so a `webcam-handler` symlinked at a 0700 directory passed
        // the whole of D11's check while the socket was bound wherever the link pointed. The
        // link can then be re-pointed at a 0777 directory before the bind, by whoever owns
        // the parent, which on the many hosts that synthesise `$XDG_RUNTIME_DIR` under
        // `/tmp` is not necessarily us (note N39).
        let runtime = TempRuntimeDir::new().expect("a temporary directory");
        let base = runtime.base().join("base");
        let target = runtime.base().join("elsewhere");
        std::fs::create_dir(base.as_std_path()).expect("a runtime base");
        std::fs::create_dir(target.as_std_path()).expect("somewhere else");
        std::fs::set_permissions(
            target.as_std_path(),
            std::fs::Permissions::from_mode(SOCKET_DIR_MODE),
        )
        .expect("ours to chmod");
        std::os::unix::fs::symlink(
            target.as_std_path(),
            base.join("webcam-handler").as_std_path(),
        )
        .expect("a symlink where the socket directory goes");

        let env = MapEnv::from_pairs(&[("XDG_RUNTIME_DIR", base.as_str())]);
        let err = SocketDir::prepare(&env)
            .expect_err("a link to a private directory is not a private directory");
        assert_eq!(err.kind(), ErrorKind::StorageIo);
        assert!(err.to_string().contains("symlink"), "{err}");
        // The refusal is the *kernel's*, not a branch here: `O_NOFOLLOW | O_DIRECTORY`
        // makes a symlink `ENOTDIR` in the `openat` itself, so there is no window between
        // "we checked it is not a link" and "we used it". The errno is carried rather than
        // flattened into prose, which is what makes that checkable.
        let Error::StorageIo { errno, .. } = &err else {
            panic!("{err:?}");
        };
        assert_eq!(*errno, Some(Errno::NOTDIR.raw_os_error()), "{err}");
        assert!(
            !target
                .join(limits::DAEMON_SOCKET_FILE)
                .as_std_path()
                .exists(),
            "the refusal happened before anything was bound"
        );

        // …and *which* flag did the refusing, measured rather than assumed, because the
        // obvious reading is wrong. Under `O_PATH` a symlink with `O_NOFOLLOW` **opens** —
        // `open(2)` says the descriptor then refers to the link itself — so `O_DIRECTORY`
        // is the flag carrying D11's whole authentication model here. Dropping it (for a
        // better "not a directory" diagnosis, say) would leave this open succeeding on a
        // link, and only the accident that a Linux symlink is always 0777 would still
        // refuse it.
        let link = base.join("webcam-handler");
        let without_directory = rustix::fs::open(
            link.as_std_path(),
            SOCKET_DIR_OFLAGS.difference(OFlags::DIRECTORY),
            Mode::empty(),
        )
        .expect("O_PATH | O_NOFOLLOW opens a symlink rather than refusing it");
        let found = rustix::fs::fstat(&without_directory).expect("a descriptor to the link");
        assert_eq!(
            rustix::fs::FileType::from_raw_mode(found.st_mode),
            rustix::fs::FileType::Symlink,
            "O_NOFOLLOW under O_PATH was expected to hand back the link itself"
        );
        assert!(
            SOCKET_DIR_OFLAGS.contains(OFlags::DIRECTORY)
                && SOCKET_DIR_OFLAGS.contains(OFlags::NOFOLLOW),
            "both flags are load-bearing: NOFOLLOW leaves a symlink to refuse and DIRECTORY refuses it"
        );
    }

    #[test]
    fn a_runtime_directory_the_platform_did_not_make_is_refused_rather_than_created() {
        // `schema::paths::runtime_dir`'s doc says the socket doctrine "rests on the
        // promise" that `$XDG_RUNTIME_DIR` is per-user, 0700 and cleaned at logout, and
        // that a missing one "means the process is not in a login session, which the
        // operator needs to be told rather than worked around". A recursive create is
        // exactly that workaround: it turns "you are not in a login session" into a
        // directory under `/tmp` that nobody promised anything about.
        let runtime = TempRuntimeDir::new().expect("a temporary directory");
        let absent = runtime.base().join("no").join("such").join("session");
        let env = MapEnv::from_pairs(&[("XDG_RUNTIME_DIR", absent.as_str())]);

        let err = SocketDir::prepare(&env).expect_err("nobody made that directory");
        assert_eq!(err.kind(), ErrorKind::StorageIo);
        assert!(err.to_string().contains("login session"), "{err}");
        assert!(
            !absent.as_std_path().exists(),
            "the daemon created a runtime directory the platform had not"
        );

        // The other direction, with the same environment: a base that exists is served
        // from, and only the one component the daemon owns is created.
        std::fs::create_dir_all(absent.as_std_path()).expect("ours to make");
        let dir = SocketDir::prepare(&env).expect("a real base is a servable one");
        assert_eq!(mode_of(dir.path()), SOCKET_DIR_MODE);
    }

    #[tokio::test]
    async fn a_socket_directory_substituted_between_the_check_and_the_bind_is_defeated() {
        // N39's scenario, and the claim it has now: `prepare` checked an inode and `bind`
        // used to create a socket at a *name*, so an attacker who owns the parent could
        // make the name mean something else in between — `rename` is a property of the
        // parent directory, not of the child. The old repair was to notice
        // (`(st_dev, st_ino)` re-read and compared), which left the window between the
        // notice and `bind(2)` open. The repair now is that there is nothing to notice:
        // the directory is held as a descriptor and the bind goes through it, so renaming
        // a *name* does not move the socket.
        let store = TempStore::new().expect("a state directory");
        let held = daemon_lock(&store);
        let runtime = TempRuntimeDir::new().expect("a temporary directory");
        let dir = SocketDir::prepare(&runtime.env()).expect("a fresh runtime directory");

        // The same name, a different directory — and private, so that what is being
        // tested is the *substitution* rather than the mode. The checked directory is
        // moved aside rather than removed, so the assertion can say where the socket
        // actually went instead of only that the bind failed.
        let moved_aside = runtime.base().join("moved-aside");
        let substitute = runtime.base().join("substitute");
        std::fs::create_dir(substitute.as_std_path()).expect("ours to make");
        std::fs::set_permissions(
            substitute.as_std_path(),
            std::fs::Permissions::from_mode(SOCKET_DIR_MODE),
        )
        .expect("ours to chmod");
        std::fs::rename(dir.path().as_std_path(), moved_aside.as_std_path())
            .expect("the parent is ours");
        std::fs::rename(substitute.as_std_path(), dir.path().as_std_path())
            .expect("the parent is ours");

        let listener = dir
            .bind(&held)
            .expect("a descriptor does not follow a rename");
        drop(listener);

        assert!(
            is_a_socket(&moved_aside.join(limits::DAEMON_SOCKET_FILE)),
            "the socket did not land in the directory whose mode and owner were asserted"
        );
        assert!(
            !dir.socket_path().as_std_path().exists(),
            "the socket landed in the directory the attacker substituted — this is the \
             defect note N39 was about, and binding by name is how it happens"
        );
    }

    #[tokio::test]
    async fn a_socket_directory_removed_between_the_check_and_the_bind_fails_closed() {
        // The other half of the substitution scenario, and the one where "defeated" is not
        // available: a directory that is *unlinked* rather than moved leaves the daemon
        // holding a descriptor to an inode with no name, so there is nowhere to put a
        // socket. That must be a refusal and not a bind into whatever now answers to the
        // name — measured on this host (note N39's amendment: "bind into a DELETED
        // directory: errno=2").
        let store = TempStore::new().expect("a state directory");
        let held = daemon_lock(&store);
        let runtime = TempRuntimeDir::new().expect("a temporary directory");
        let dir = SocketDir::prepare(&runtime.env()).expect("a fresh runtime directory");

        let substitute = runtime.base().join("substitute");
        std::fs::create_dir(substitute.as_std_path()).expect("ours to make");
        std::fs::set_permissions(
            substitute.as_std_path(),
            std::fs::Permissions::from_mode(SOCKET_DIR_MODE),
        )
        .expect("ours to chmod");
        std::fs::remove_dir(dir.path().as_std_path()).expect("ours to remove");
        std::fs::rename(substitute.as_std_path(), dir.path().as_std_path())
            .expect("the parent is ours");

        let err = dir
            .bind(&held)
            .expect_err("the checked directory has no name any more");
        assert_eq!(err.kind(), ErrorKind::StorageIo);
        assert!(
            !dir.socket_path().as_std_path().exists(),
            "a socket was bound in a directory nobody checked"
        );
    }

    #[tokio::test]
    async fn a_socket_directory_re_permissioned_between_the_check_and_the_bind_is_refused() {
        // What the re-check is still for, now that substitution is defeated rather than
        // detected: the checked inode's *own* mode can change under a running daemon, and
        // a `chmod 0777` between `prepare` and `bind` would leave the socket's whole
        // authentication model weaker than the one that was asserted. A descriptor sees
        // the inode's current mode, so this is a question about the right object — which
        // is what note N39's bullet list was complaining about.
        let store = TempStore::new().expect("a state directory");
        let held = daemon_lock(&store);
        let runtime = TempRuntimeDir::new().expect("a temporary directory");
        let dir = SocketDir::prepare(&runtime.env()).expect("a fresh runtime directory");

        std::fs::set_permissions(
            dir.path().as_std_path(),
            std::fs::Permissions::from_mode(0o777),
        )
        .expect("ours to chmod");

        let err = dir
            .bind(&held)
            .expect_err("0777 is not the mode that was asserted");
        assert_eq!(err.kind(), ErrorKind::StorageIo);
        assert!(err.to_string().contains("0777"), "{err}");
        assert!(
            !dir.socket_path().as_std_path().exists(),
            "a socket was bound in a directory anyone can walk into"
        );

        // The other direction, on the same descriptor: put the mode back and the same
        // call serves, so the refusal is about the mode and not about having asked twice.
        std::fs::set_permissions(
            dir.path().as_std_path(),
            std::fs::Permissions::from_mode(SOCKET_DIR_MODE),
        )
        .expect("ours to chmod");
        drop(dir.bind(&held).expect("0700 is the mode that was asserted"));
    }

    #[test]
    fn a_socket_directory_owned_by_somebody_else_is_refused() {
        // The check note N39 recorded as "absent by omission rather than by argument",
        // driven the only way an unprivileged test can drive it: the *predicate* is the
        // subject, because arranging a directory owned by another uid needs privileges
        // this suite does not have and must not acquire (note N44 is the precedent — the
        // other-uid half of the UDS row is a shell predicate for exactly this reason).
        //
        // So the fixture is a `Stat` with one field moved, and the arms are both
        // directions: our own uid passes, and a uid that is not ours is refused with a
        // message that names both. `geteuid() + 1` cannot collide with us and cannot
        // wrap — a uid of `u32::MAX` is `(uid_t)-1`, which no process runs as.
        let runtime = TempRuntimeDir::new().expect("a temporary directory");
        let dir = SocketDir::prepare(&runtime.env()).expect("a fresh runtime directory");
        let ours = rustix::fs::stat(dir.path().as_std_path()).expect("it was just made");

        check_mode_and_owner(dir.path(), &ours).expect("a directory this process owns");

        let mut theirs = ours;
        theirs.st_uid = rustix::process::geteuid().as_raw() + 1;
        let err =
            check_mode_and_owner(dir.path(), &theirs).expect_err("a directory somebody else owns");
        assert_eq!(err.kind(), ErrorKind::StorageIo);
        let rendered = err.to_string();
        assert!(rendered.contains(&theirs.st_uid.to_string()), "{rendered}");
        assert!(
            rendered.contains(&rustix::process::geteuid().as_raw().to_string()),
            "{rendered}"
        );
        // And the mode is still checked when the owner is wrong's neighbour is right: the
        // two refusals are separate, so one cannot stand in for the other.
        let mut wrong_mode = ours;
        wrong_mode.st_mode = (ours.st_mode & !MODE_BITS) | 0o755;
        assert!(
            check_mode_and_owner(dir.path(), &wrong_mode)
                .expect_err("0755 is not 0700")
                .to_string()
                .contains("0755")
        );
    }

    /// Whether a path names a socket, asked through `std::fs` rather than through the
    /// module's own `is_socket` — a test that reused the code under test's decode would
    /// pass on a shared misreading of `S_IFMT`.
    fn is_a_socket(path: &Utf8Path) -> bool {
        use std::os::unix::fs::FileTypeExt;

        std::fs::symlink_metadata(path.as_std_path())
            .is_ok_and(|found| found.file_type().is_socket())
    }

    #[test]
    fn the_socket_is_where_d11_says_it_is() {
        // A *pin*, in the tradition of `crates/api/fixtures/d13-rpc-codes.tsv`: the path is a
        // compatibility contract with a client that is not written yet (P4f's
        // `webcam-handler-client` connects to it), so moving it has to be a diff somebody
        // wrote on purpose rather than a rename that stayed green.
        let runtime = TempRuntimeDir::new().expect("a temporary directory");
        let dir = SocketDir::prepare(&runtime.env()).expect("a fresh runtime directory");
        assert_eq!(
            dir.socket_path(),
            runtime.base().join("webcam-handler").join("wchd.sock")
        );
    }

    #[test]
    fn without_a_runtime_directory_variable_there_is_no_socket_directory() {
        // `schema::paths` owns this refusal; the assertion here is that the daemon asks
        // it rather than inventing a `/tmp` fallback of its own.
        let err = SocketDir::prepare(&MapEnv::empty()).expect_err("nothing is set");
        assert_eq!(err.kind(), ErrorKind::StorageIo);
        assert!(err.to_string().contains("XDG_RUNTIME_DIR"), "{err}");
    }

    #[tokio::test]
    async fn a_socket_left_by_a_dead_daemon_is_replaced() {
        let store = TempStore::new().expect("a state directory");
        let held = daemon_lock(&store);
        let runtime = TempRuntimeDir::new().expect("a temporary directory");
        let dir = SocketDir::prepare(&runtime.env()).expect("a fresh runtime directory");

        // The first daemon binds and then dies without cleaning up — which is every exit
        // this sub-milestone has, so this is the ordinary case and not the exotic one.
        let dead = dir.bind(&held).expect("nothing is in the way");
        drop(dead);
        assert!(
            dir.socket_path().as_std_path().exists(),
            "dropping a listener does not unlink its socket; the rest of this test is \
             about the file that is left"
        );

        let _listener = dir.bind(&held).expect("the leftover socket is stale");

        // Asked of a *client*, not of `local_addr()`. Since note N39's hardening the
        // address passed to `bind(2)` is `/proc/self/fd/<n>/wchd.sock` — the dirfd-relative
        // spelling Linux offers, this module's header says why — so `local_addr()` reports
        // that magic path and is no longer the way to ask where the socket is. What a
        // reader of this test wants to know is that a client which knows only D11's name
        // reaches this listener, and connecting is the assertion that says so; it would go
        // red if the socket had landed in any other inode.
        std::os::unix::net::UnixStream::connect(dir.socket_path().as_std_path()).unwrap_or_else(
            |err| {
                panic!("a client cannot reach {}: {err}", dir.socket_path());
            },
        );
        assert!(
            is_a_socket(&dir.socket_path()),
            "{} is not a socket",
            dir.socket_path()
        );
    }

    #[tokio::test]
    async fn a_bind_by_name_still_serves_and_says_that_it_is_the_unprotected_spelling() {
        // Note N39's residual 1: `/proc/self/fd` needs `/proc`, and a minimal container
        // without procfs falls back to binding by name. That arm cannot be produced on a
        // host that has `/proc` mounted — which is every host this suite runs on — so
        // without the address being a parameter it is a branch nothing has ever executed,
        // in the module whose whole subject is that a check nobody drives is a check
        // nobody has. Same argument as `Accepting`, one function along.
        let store = TempStore::new().expect("a state directory");
        let held = daemon_lock(&store);
        let runtime = TempRuntimeDir::new().expect("a temporary directory");
        let dir = SocketDir::prepare(&runtime.env()).expect("a fresh runtime directory");
        let socket = dir.socket_path();

        // The fallback arm: the address is the real name, and the bind must still produce
        // a listener a client reaches by D11's path.
        let (listener, logged) = capturing(|| {
            dir.bind_listener(&socket, &BindAddress::ByName(socket.to_string()))
                .expect("binding by name is the fallback, not a failure")
        });
        std::os::unix::net::UnixStream::connect(socket.as_std_path())
            .unwrap_or_else(|err| panic!("a client cannot reach {socket}: {err}"));
        assert!(is_a_socket(&socket), "{socket} is not a socket");
        assert!(
            logged.contains("WARN"),
            "the downgrade was silent: {logged:?}"
        );
        assert!(logged.contains("/proc is not mounted"), "{logged:?}");
        drop(listener);
        std::fs::remove_file(socket.as_std_path()).expect("ours to remove");

        // The other direction, and the reason the assertion above can go red: the ordinary
        // spelling binds through the descriptor and says nothing, because there is nothing
        // to warn about. `bind` is the caller under test here — it is what chooses.
        let (listener, logged) = capturing(|| dir.bind(&held).expect("nothing is in the way"));
        assert!(
            !logged.contains("WARN"),
            "a protected bind warned about itself: {logged:?}"
        );
        drop(listener);

        // …and the choice itself is a value rather than a side effect, so which spelling a
        // host got is a thing a test can name. `/proc` is mounted here, so this is the
        // protected one.
        assert!(
            matches!(
                dir.bind_address(&socket),
                BindAddress::ThroughTheDescriptor(address)
                    if address.starts_with("/proc/self/fd/")
            ),
            "{:?}",
            dir.bind_address(&socket)
        );
    }

    #[tokio::test]
    async fn something_that_is_not_a_socket_is_refused_rather_than_deleted() {
        let store = TempStore::new().expect("a state directory");
        let held = daemon_lock(&store);
        let runtime = TempRuntimeDir::new().expect("a temporary directory");
        let dir = SocketDir::prepare(&runtime.env()).expect("a fresh runtime directory");

        // A directory rather than a regular file, for a gate's sake and not a
        // behaviour's: `atomic-write-home.sh` treats any raw write primitive in a file
        // that names the runtime directory as a store bypass, and the claim — "what is
        // there is not a socket" — is the same either way.
        std::fs::create_dir(dir.socket_path().as_std_path()).expect("the path is free");

        let err = dir
            .bind(&held)
            .expect_err("a directory is not a stale socket");
        assert_eq!(err.kind(), ErrorKind::StorageIo);
        assert!(err.to_string().contains("not a socket"), "{err}");
        assert!(
            dir.socket_path().as_std_path().exists(),
            "the daemon deleted something it did not put there"
        );
    }

    #[tokio::test]
    async fn a_socket_path_the_kernel_cannot_hold_is_refused_before_the_bind() {
        // `sun_path` is 108 bytes and `$TMPDIR` is not always shallow —
        // `scripts/mutants.sh` runs the whole suite inside a scratch tree. Without this
        // check the failure is a bare `ENAMETOOLONG` from `bind`, which reads as a daemon
        // bug rather than as a directory that is too deep to serve from.
        let store = TempStore::new().expect("a state directory");
        let held = daemon_lock(&store);
        let runtime = TempRuntimeDir::new().expect("a temporary directory");

        let mut deep = runtime.base().to_owned();
        while deep.as_str().len() <= limits::MAX_UNIX_SOCKET_PATH_BYTES {
            deep.push("deeper");
        }
        // Made by this test rather than by the daemon: `$XDG_RUNTIME_DIR` is a promise the
        // platform makes and `SocketDir::prepare` verifies it rather than creating it, so
        // a fixture that left it missing would be exercising *that* refusal instead of the
        // path-length one.
        std::fs::create_dir_all(deep.as_std_path()).expect("a deep temporary directory");
        let env = MapEnv::from_pairs(&[("XDG_RUNTIME_DIR", deep.as_str())]);

        // The directory itself is fine — it is only the composed socket path that does
        // not fit, which is why the refusal belongs to `bind` and not to `prepare`.
        let dir = SocketDir::prepare(&env).expect("a deep directory is still a directory");
        let err = dir
            .bind(&held)
            .expect_err("the socket path is past sun_path");
        assert_eq!(err.kind(), ErrorKind::StorageIo);
        let rendered = err.to_string();
        assert!(
            rendered.contains(&limits::MAX_UNIX_SOCKET_PATH_BYTES.to_string()),
            "{rendered}"
        );
        assert!(
            !dir.socket_path().as_std_path().exists(),
            "the refusal happened before anything was bound"
        );
    }

    #[tokio::test]
    async fn a_per_operation_lock_does_not_authorize_removing_a_socket() {
        // D9's two protocols are not interchangeable here: only the lifetime lock
        // establishes that no other daemon is alive, and that is the entire argument for
        // unlinking a path somebody else might be serving.
        let store = TempStore::new().expect("a state directory");
        let momentary = store
            .store()
            .lock(LockProtocol::PerOperation)
            .expect("an unlocked state directory");
        let runtime = TempRuntimeDir::new().expect("a temporary directory");
        let dir = SocketDir::prepare(&runtime.env()).expect("a fresh runtime directory");

        let err = dir
            .bind(&momentary)
            .expect_err("a per-operation lock is a `webcam-handler-cli`'s, not a daemon's");
        assert_eq!(err.kind(), ErrorKind::StorageIo);
        assert!(err.to_string().contains("held_for_lifetime"), "{err}");
        assert!(
            !dir.socket_path().as_std_path().exists(),
            "the refusal happened before anything was bound"
        );
    }

    /// A listener that never produces a connection, only the errno it was built with.
    ///
    /// The scriptable double for [`Accepting`]. `EMFILE` because that is the failure the
    /// give-up path exists for: an accept error is usually about one client and clears by
    /// itself, and `EMFILE` is about this process and does not.
    struct NeverAccepts(i32);

    impl Accepting for NeverAccepts {
        async fn accept(&self) -> std::io::Result<UnixStream> {
            Err(std::io::Error::from_raw_os_error(self.0))
        }
    }

    #[tokio::test]
    async fn giving_up_on_accept_is_a_failure_the_daemon_reports_rather_than_a_clean_stop() {
        // `webcam-handler-daemon`'s exit code is made of this. The accept loop ending is not
        // by itself distinguishable from somebody asking the server to stop — jsonrpsee's
        // `ServerHandle::stopped` resolves when the last stop-handle receiver is dropped,
        // which a loop that gave up would do — so without a reason travelling back, a daemon
        // that hit a persistent `EMFILE` would log one line, exit 0, and be left alone by
        // `Restart=on-failure` while `webcam-handler-client` reported that nobody was
        // listening.
        const EMFILE: i32 = 24;

        let mut serving = serve_accepting(NeverAccepts(EMFILE), RpcModule::new(()));
        let err = serving
            .stopped()
            .await
            .expect_err("the loop gave up and said nothing");

        assert_eq!(err.kind(), ErrorKind::DeviceIo);
        let rendered = err.to_string();
        assert!(rendered.contains("accept"), "{rendered}");
        assert!(
            rendered.contains(&limits::MAX_CONSECUTIVE_ACCEPT_FAILURES.to_string()),
            "{rendered}"
        );
        assert!(
            matches!(err, Error::DeviceIo { errno, .. } if errno == Some(EMFILE)),
            "the refusal dropped the kernel's own reason: {err:?}"
        );
    }

    #[tokio::test]
    async fn the_reason_the_loop_ended_survives_being_asked_for_twice() {
        // What `daemon::shutdown` does on every stop: it races `stopped()` against a signal
        // and then, having stopped the server, awaits it again as the drain. Before P4e-ii
        // that second call polled a `JoinHandle` that had already yielded, which **panics** —
        // on the daemon's stopping path, in a task nobody joins, so the process would have
        // exited without its teardown and without saying why.
        //
        // The give-up arm is used because it is the answer that has content: an ending that
        // forgot what it was would come back as `Ok(())` and turn a daemon that stopped
        // serving into a clean exit, which is the whole thing `Serving` exists to prevent.
        const EMFILE: i32 = 24;

        let mut serving = serve_accepting(NeverAccepts(EMFILE), RpcModule::new(()));
        let first = serving.stopped().await.expect_err("the loop gave up");
        let again = serving
            .stopped()
            .await
            .expect_err("the second answer was not the first");
        assert_eq!(first, again);
    }

    #[tokio::test]
    async fn a_stop_somebody_asked_for_is_not_a_failure() {
        // The other direction of the same answer, because a `stopped()` that always
        // refused would pass the test above while making every clean shutdown an error.
        let listener = {
            let store = TempStore::new().expect("a state directory");
            let held = daemon_lock(&store);
            let runtime = TempRuntimeDir::new().expect("a temporary directory");
            let dir = SocketDir::prepare(&runtime.env()).expect("a fresh runtime directory");
            let listener = dir.bind(&held).expect("nothing is in the way");
            // The fixtures go away here; the bound listener outlives them, which is all
            // this test needs.
            listener
        };

        let mut serving = serve_accepting(listener, RpcModule::new(()));
        serving.stop();
        serving.stopped().await.expect("nobody gave up on anything");
    }

    #[test]
    fn accepting_survives_scattered_failures_and_gives_up_on_a_run_of_them() {
        // Both directions of the one policy the accept loop has. Without the reset, a
        // long-lived daemon would eventually stop accepting because of failures spread
        // over days; without the limit, an `EMFILE` that never clears is a spin.
        let mut failures = AcceptFailures::none();
        for _ in 0..(limits::MAX_CONSECUTIVE_ACCEPT_FAILURES * 4) {
            assert!(failures.failed(), "one failure is not a run of them");
            failures.succeeded();
        }

        let mut run = AcceptFailures::none();
        for attempt in 1..limits::MAX_CONSECUTIVE_ACCEPT_FAILURES {
            assert!(run.failed(), "gave up after {attempt} failures");
        }
        assert!(
            !run.failed(),
            "accepted a {}th consecutive failure",
            limits::MAX_CONSECUTIVE_ACCEPT_FAILURES
        );
    }
}
