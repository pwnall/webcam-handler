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
//! Because the connection carries HTTP, a caller speaks `POST /` with
//! `content-type: application/json`. That is a consequence of mounting jsonrpsee's own
//! service rather than a choice, and it is the fact P4f's client transport has to be built
//! against: it is an HTTP/1.1 client on a `UnixStream`, not a newline-framed JSON-RPC
//! pipe. The same connection can carry a WebSocket upgrade — which is how P4e's
//! subscriptions will reach a `wchc` that has no TCP listener to use — and this build
//! turns that half off until P4e brings its bounds and its tests with it (see [`serve`]).
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
//! 3. **The bounds this server runs under are set here, from `schema::limits`.**
//!    jsonrpsee's own defaults (10 MB bodies, 100 connections, *unlimited* batches) are
//!    somebody else's numbers for somebody else's deployment, and an unbounded batch on
//!    the one socket the daemon always serves is not a bound this project is allowed to
//!    inherit silently (AGENTS, "Bounded everything"). Five bounds are named here: the
//!    four `ServerConfig` fields [`serve`] sets and the connection count [`serve`]
//!    enforces itself, because jsonrpsee's `max_connections` is not one — see [`serve`].
//!
//!    What is *not* claimed: `ServerConfig` has nine more fields, and this build inherits
//!    them. Two of them are bounds in AGENTS's sense — `message_buffer_capacity` and
//!    `max_subscriptions_per_connection` — and both govern the WebSocket surface only,
//!    which is why this build turns that surface **off** (`ServerConfig::http_only`) rather than
//!    shipping somebody else's channel depth behind a transport no test drives. P4e's
//!    subscriptions turn it back on, with those two constants and the tests that reach
//!    them. `keep_alive_timeout` is inherited and inert: it is hyper's HTTP/2 setting and
//!    this transport is HTTP/1.1 over `AF_UNIX`. Note **N38** records the list.
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
//! not about a name: the directory is `lstat`ed rather than `stat`ed (a symlink is
//! refused, not followed), `$XDG_RUNTIME_DIR` itself is required to exist rather than
//! created, and [`SocketDir::bind`] re-reads the directory and compares `(st_dev, st_ino)`
//! with what [`SocketDir::prepare`] checked, so the object that was found private and the
//! object bound into are provably the same one. Note **N39** has the measurements that
//! forced each of those.

use std::os::unix::fs::{DirBuilderExt, FileTypeExt, MetadataExt, PermissionsExt};

use camino::{Utf8Path, Utf8PathBuf};
use engine::paths::Env;
use engine::store::{LockProtocol, StoreLock};
use jsonrpsee_server::{
    BatchRequestConfig, Methods, Server, ServerConfig, ServerHandle, stop_channel,
};
use schema::limits;
use schema::{Error, Result};
use tokio::net::{UnixListener, UnixStream};

/// The mode D11 requires of the directory the socket lives in.
///
/// One home for the number: [`SocketDir::prepare`] creates with it, asserts against it,
/// and names it in the refusal, so "0700" is not written three times and cannot come to
/// mean three things.
pub const SOCKET_DIR_MODE: u32 = 0o700;

/// The mode bits a directory's permissions carry — everything below the type bits.
const MODE_BITS: u32 = 0o7777;

/// The runtime directory the daemon serves its socket from, once its mode is known good.
///
/// Holding this value is the evidence that the check happened: [`SocketDir::bind`] is the
/// only way to get a listener out of this module, and it takes `&self`, so a socket
/// cannot be bound in a directory nobody looked at.
#[derive(Debug, Clone)]
pub struct SocketDir {
    path: Utf8PathBuf,
    /// The directory this value's mode was read off, as the kernel identifies it.
    ///
    /// A path is a name and names are re-resolved; `(st_dev, st_ino)` is the object. It is
    /// carried so [`SocketDir::bind`] can prove that the directory it binds into is the
    /// one [`SocketDir::prepare`] found private, rather than trusting that the name still
    /// leads there.
    identity: (u64, u64),
}

impl SocketDir {
    /// Resolve `$XDG_RUNTIME_DIR/webcam-handler`, create it 0700, and **assert the mode
    /// that came back**.
    ///
    /// The environment arrives as a parameter for `engine::paths`'s reason: reading it
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
    /// ## Why the check is about an inode and not about a path
    ///
    /// `std::fs::metadata` follows symlinks and `DirBuilder::recursive(true)` is happy to
    /// find one where it wanted to create a directory, so a `webcam-handler` that is a
    /// **symlink to** a 0700 directory passed the mode check while the socket was bound
    /// wherever the link pointed — and the link can be re-pointed afterwards. Measured on
    /// this tree, both halves (note N39). So the leaf is `lstat`ed and a symlink is a
    /// refusal, and [`SocketDir::bind`] re-checks the inode it was told about rather than
    /// re-resolving the name.
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
    /// refusal is `engine::paths`'s and names the variable), when it does not exist or is
    /// not a directory, when the socket directory cannot be created or read, when it is a
    /// symlink or not a directory, or when its mode is not [`SOCKET_DIR_MODE`].
    pub fn prepare(env: &dyn Env) -> Result<SocketDir> {
        let path = engine::paths::runtime_dir(env)?;
        let base = path.parent().unwrap_or(&path).to_owned();

        let found =
            std::fs::symlink_metadata(base.as_std_path()).map_err(|err| Error::StorageIo {
                path: base.clone(),
                errno: err.raw_os_error(),
                message: format!(
                    "{err} — $XDG_RUNTIME_DIR names the per-user directory the platform \
                 promises is private and cleaned at logout (D11), so a daemon that made \
                 one would be inventing the promise rather than resting on it; a missing \
                 one means this process is not in a login session"
                ),
            })?;
        if found.file_type().is_symlink() || !found.is_dir() {
            return Err(Error::StorageIo {
                path: base,
                errno: None,
                message: "is not a directory (or is a symlink to one), and \
                          $XDG_RUNTIME_DIR must be the platform's own per-user directory \
                          — the daemon's socket permissions rest on what it promises (D11)"
                    .to_owned(),
            });
        }

        match std::fs::DirBuilder::new()
            .recursive(false)
            .mode(SOCKET_DIR_MODE)
            .create(path.as_std_path())
        {
            Ok(()) => {}
            // Already there is the ordinary case and says nothing about the mode, which
            // is what the read-back below is for.
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(err) => return Err(storage_io(&path, &err)),
        }

        let found =
            std::fs::symlink_metadata(path.as_std_path()).map_err(|err| storage_io(&path, &err))?;
        if found.file_type().is_symlink() || !found.is_dir() {
            return Err(Error::StorageIo {
                path,
                errno: None,
                message: "is a symlink or is not a directory, and the daemon's socket \
                          directory has to be a directory this process can see the mode \
                          of — a link's target can be re-pointed between the check and \
                          the bind, so what is checked is the directory itself (D11)"
                    .to_owned(),
            });
        }

        let mode = found.permissions().mode() & MODE_BITS;
        if mode != SOCKET_DIR_MODE {
            return Err(Error::StorageIo {
                path,
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

        Ok(SocketDir {
            path,
            identity: (found.dev(), found.ino()),
        })
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
    /// unlinks cannot restart after any exit that did not clean up, which at this
    /// sub-milestone is *every* exit (P4e owns the shutdown discipline). Unlinking
    /// something at the socket path is also, in most codebases, how one process hijacks
    /// another's socket. Both facts are answered by the same argument, and it is D9's,
    /// not a new law: the daemon holds the state directory's advisory lock
    /// [`LockProtocol::HeldForLifetime`] for as long as it runs, Linux releases an
    /// `flock` when the holding process dies, so a caller that *has* that lock has
    /// already established that no other daemon is alive for this user — and therefore
    /// that anything at the socket path is stale by construction.
    ///
    /// That is why the lock is a parameter rather than a sentence in this comment: the
    /// ordering (lock, then directory, then unlink, then bind) is the whole safety
    /// argument, and a caller cannot get here without the first step. The protocol is
    /// checked because only the lifetime protocol carries the argument — `wch`'s
    /// per-operation lock is released moments later and proves nothing about who is
    /// running.
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
    /// ## Why the directory is checked twice
    ///
    /// [`SocketDir::prepare`] read a mode; this binds a socket. Between the two the name
    /// can come to mean a different object — an attacker who owns the *parent* can
    /// `rename` a directory or a symlink into place, and unlink/rename permission is a
    /// property of the parent rather than of the child. So the directory is read again
    /// here and its `(st_dev, st_ino)` and mode compared with what `prepare` found: the
    /// object that was proved private and the object bound into are the same inode, or
    /// this refuses. That does not close the window between this check and `bind(2)`
    /// itself — closing it needs a directory descriptor and a `bind` relative to it,
    /// which needs a syscall wrapper this workspace does not link (note N39 carries the
    /// obligation) — but it turns "checked a different object" from the default into a
    /// race somebody has to win.
    ///
    /// # Errors
    ///
    /// [`Error::StorageIo`] when the lock is not held for the daemon's lifetime, when the
    /// socket directory is no longer the one whose mode was asserted, when the composed
    /// path is longer than the kernel's `sun_path`, when something that is not a socket
    /// already occupies the path, or when the unlink or the `bind` fails.
    ///
    /// Must be called from inside a tokio runtime: `tokio::net::UnixListener::bind` is
    /// not `async`, but it registers the descriptor with the reactor.
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
                     $XDG_RUNTIME_DIR is too deep for a socket to live under",
                    socket.as_str().len(),
                    limits::MAX_UNIX_SOCKET_PATH_BYTES
                ),
            });
        }

        match std::fs::symlink_metadata(socket.as_std_path()) {
            Ok(existing) if existing.file_type().is_socket() => {
                tracing::info!(
                    socket = %socket,
                    "removing a socket left by a dead daemon; this process holds the state lock, \
                     so no live daemon owns it"
                );
                std::fs::remove_file(socket.as_std_path())
                    .map_err(|err| storage_io(&socket, &err))?;
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
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(storage_io(&socket, &err)),
        }

        UnixListener::bind(socket.as_std_path()).map_err(|err| storage_io(&socket, &err))
    }

    /// Refuse unless the path still leads to the inode whose mode was asserted.
    fn still_the_directory_that_was_checked(&self) -> Result<()> {
        let found = std::fs::symlink_metadata(self.path.as_std_path())
            .map_err(|err| storage_io(&self.path, &err))?;
        let mode = found.permissions().mode() & MODE_BITS;
        if found.file_type().is_symlink()
            || !found.is_dir()
            || (found.dev(), found.ino()) != self.identity
            || mode != SOCKET_DIR_MODE
        {
            return Err(Error::StorageIo {
                path: self.path.clone(),
                errno: None,
                message: format!(
                    "is not the directory whose mode was asserted at startup any more \
                     (now mode {mode:04o}); something replaced it between the check and \
                     the bind, and the socket's whole authentication model is that \
                     directory's privacy (D11), so nothing is bound here"
                ),
            });
        }
        Ok(())
    }
}

/// A running server, and the reason it will eventually stop.
///
/// A value rather than a bare [`ServerHandle`] because *why* the accept loop ended is a
/// fact the composition root has to act on. jsonrpsee's `ServerHandle::stopped` is
/// `watch::Sender::closed()` — it resolves when the last receiver is dropped, which the
/// accept loop dropping its own would do — so a loop that gave up would otherwise look
/// exactly like a clean stop, and `wchd` would exit `SUCCESS` after announcing that it had
/// stopped accepting connections. A supervisor reads that as "the service completed", and
/// `Restart=on-failure` declines to restart it.
#[derive(Debug)]
#[must_use = "a server nobody waits on is a daemon that exits as soon as it starts"]
pub struct Serving {
    handle: ServerHandle,
    accepting: tokio::task::JoinHandle<Result<()>>,
}

impl Serving {
    /// Ask the server to stop. Idempotent from the caller's point of view.
    ///
    /// Ending the accept loop is all this does — in-flight connections finish their
    /// current answer and end. It is not a drain and not a signal handler; both are P4e's.
    pub fn stop(&self) {
        // `AlreadyStoppedError` means somebody already asked, including the accept loop
        // itself on its give-up path. That is the outcome this call wanted.
        let _ = self.handle.stop();
    }

    /// Wait until the server has stopped, and answer why.
    ///
    /// `Ok(())` for a stop somebody asked for; the accept loop's own refusal otherwise.
    ///
    /// # Errors
    ///
    /// [`Error::DeviceIo`] when the loop gave up after
    /// [`limits::MAX_CONSECUTIVE_ACCEPT_FAILURES`] consecutive `accept` failures, carrying
    /// the last one's errno, or when the accept task itself panicked or was cancelled.
    pub async fn stopped(&mut self) -> Result<()> {
        let reason = match (&mut self.accepting).await {
            Ok(reason) => reason,
            Err(err) => Err(Error::DeviceIo {
                operation: "accept connections on the daemon socket".to_owned(),
                errno: None,
                message: err.to_string(),
            }),
        };
        // Then the connections, which is what `ServerHandle::stopped` is for. The accept
        // loop signals a stop on its way out, so this cannot wait forever on a client
        // holding an idle keep-alive connection open.
        self.handle.clone().stopped().await;
        reason
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
/// once it and every connection it spawned are gone — which is the whole of P4b's
/// lifecycle and is deliberately less than P4e's: nothing here handles a signal, drains a
/// subscription, or unlinks the socket on the way out. The socket file is left where it is
/// on purpose; [`SocketDir::bind`] is what makes that harmless.
///
/// jsonrpsee spells the per-connection future `serve_with_graceful_shutdown`, and its
/// "graceful" is about *one connection's* in-flight request finishing rather than being
/// dropped mid-answer. It is not this daemon claiming a drain: P4e's shutdown discipline —
/// `CancellationToken` teardown, cancelled preview streams, an orderly store-lock release —
/// is not here, and no name in this module should be read as standing in for it.
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
/// ## WebSocket upgrades are off until P4e
///
/// `enable_ws` defaults to on, and with it come two of jsonrpsee's numbers this project
/// has not chosen — `message_buffer_capacity` (a channel depth, which AGENTS says lives in
/// `schema::limits`) and `max_subscriptions_per_connection`. Nothing at P4b subscribes and
/// no test drives an upgrade, so leaving the surface enabled would be shipping an
/// unbounded, untested transport for a consumer that does not exist yet (rubric A8). P4e's
/// subscriptions turn it on together with those constants and the tests that reach them.
pub fn serve(listener: UnixListener, methods: impl Into<Methods>) -> Serving {
    serve_accepting(listener, methods)
}

/// [`serve`], over anything that accepts connections. See [`Accepting`].
fn serve_accepting<L: Accepting>(listener: L, methods: impl Into<Methods>) -> Serving {
    let methods: Methods = methods.into();
    let (stop_handle, server_handle) = stop_channel();

    let service_builder = Server::builder()
        .set_config(
            ServerConfig::builder()
                .max_connections(limits::DAEMON_MAX_CONNECTIONS)
                .max_request_body_size(limits::RPC_MAX_REQUEST_BYTES)
                .max_response_body_size(limits::RPC_MAX_RESPONSE_BYTES)
                .set_batch_request_config(BatchRequestConfig::Limit(limits::RPC_MAX_BATCH))
                .http_only()
                .build(),
        )
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
        accepting,
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

#[cfg(test)]
mod tests {
    use engine::paths::{MapEnv, TempRuntimeDir};
    use engine::store::TempStore;
    use jsonrpsee_server::RpcModule;
    use schema::ErrorKind;

    use super::*;

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
        // The hole this check exists to close, and the reason the mode is read with
        // `lstat`. `create_dir_all` is happy to find a symlink where it wanted a directory
        // (it falls back to `is_dir()`, which follows), and `metadata` reports the
        // *target's* mode — so a `webcam-handler` symlinked at a 0700 directory passed the
        // whole of D11's check while the socket was bound wherever the link pointed. The
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
        assert!(
            !target
                .join(limits::DAEMON_SOCKET_FILE)
                .as_std_path()
                .exists(),
            "the refusal happened before anything was bound"
        );
    }

    #[test]
    fn a_runtime_directory_the_platform_did_not_make_is_refused_rather_than_created() {
        // `engine::paths::runtime_dir`'s doc says the socket doctrine "rests on the
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
    async fn a_socket_directory_replaced_between_the_check_and_the_bind_is_refused() {
        // `prepare` read a mode off an inode; `bind` creates a socket at a *name*. An
        // attacker who owns the parent can make the name mean something else in between —
        // `rename` is a property of the parent directory, not of the child — so the
        // directory is read again and matched against the one whose mode was asserted.
        // Without that, D11's whole authentication model is a check on an object the
        // daemon may no longer be serving from.
        let store = TempStore::new().expect("a state directory");
        let held = daemon_lock(&store);
        let runtime = TempRuntimeDir::new().expect("a temporary directory");
        let dir = SocketDir::prepare(&runtime.env()).expect("a fresh runtime directory");

        // The same name, a different directory — and private, so that what is refused is
        // the *substitution* rather than the mode.
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
            .expect_err("the directory whose mode was asserted is gone");
        assert_eq!(err.kind(), ErrorKind::StorageIo);
        assert!(err.to_string().contains("asserted at startup"), "{err}");
        assert!(
            !dir.socket_path().as_std_path().exists(),
            "a socket was bound in a directory nobody checked"
        );
    }

    #[test]
    fn the_socket_is_where_d11_says_it_is() {
        // A *pin*, in the tradition of `crates/api/fixtures/d13-rpc-codes.tsv`: the path
        // is a compatibility contract with a client that is not written yet (P4f's
        // `wchc` connects to it), so moving it has to be a diff somebody wrote on
        // purpose rather than a rename that stayed green.
        let runtime = TempRuntimeDir::new().expect("a temporary directory");
        let dir = SocketDir::prepare(&runtime.env()).expect("a fresh runtime directory");
        assert_eq!(
            dir.socket_path(),
            runtime.base().join("webcam-handler").join("wchd.sock")
        );
    }

    #[test]
    fn without_a_runtime_directory_variable_there_is_no_socket_directory() {
        // `engine::paths` owns this refusal; the assertion here is that the daemon asks
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

        let listener = dir.bind(&held).expect("the leftover socket is stale");
        assert_eq!(
            listener
                .local_addr()
                .ok()
                .and_then(|addr| addr.as_pathname().map(std::path::Path::to_path_buf)),
            Some(dir.socket_path().into_std_path_buf())
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
            .expect_err("a per-operation lock is a `wch`'s, not a daemon's");
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
        // `wchd`'s exit code is made of this. The accept loop ending is not by itself
        // distinguishable from somebody asking the server to stop — jsonrpsee's
        // `ServerHandle::stopped` resolves when the last stop-handle receiver is dropped,
        // which a loop that gave up would do — so without a reason travelling back, a
        // daemon that hit a persistent `EMFILE` would log one line, exit 0, and be left
        // alone by `Restart=on-failure` while `wchc` reported that nobody was listening.
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
