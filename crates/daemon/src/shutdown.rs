//! How `wchd` stops, in an order (docs/7 P4e-ii).
//!
//! Everything before this sub-milestone was true of a daemon that *had* stopped: Linux
//! releases an `flock` when the last descriptor on its open file description closes, so a
//! killed `wchd` released the state directory and every camera node its actors held
//! (`crate::state`'s header, `engine::store::StoreLock`'s paragraph). None of that needed
//! code, and none of it is what this module adds. What it adds is the **order**, and one
//! thing the kernel cannot do at all: a subscriber whose stream ends with a reason instead of
//! a socket that goes away underneath it.
//!
//! ## The order, which is the whole point
//!
//! 1. **Wait for whichever comes first** — a stop request from [`Signalling`], or the
//!    transport stopping on its own. The second is the accept loop having *given up*
//!    (`crate::uds::Serving`'s header: sixty-four consecutive `accept` failures), and it is an
//!    error that must still be reported: `main` returns it, the process exits non-zero, and
//!    `Restart=on-failure` restarts a daemon that has stopped serving. Swallowing it would
//!    tell a service manager the service completed.
//! 2. **Say so** — [`Notifying::stopping`], before anything else happens. A service manager
//!    that has been told `STOPPING=1` stops holding the daemon to its start/stop clock, and
//!    saying it *first* is what makes the rest of this list affordable: every step below takes
//!    time, and a supervisor that learns about the stop only when the socket closes has been
//!    counting that time against a timeout.
//! 3. **Cancel** — [`Shutdown::cancel`], which reaches every open subscription. This is the
//!    difference worth having, and it is why cancellation comes *before* the transport stops:
//!    a subscription cancelled here ends with [`crate::events::SHUTTING_DOWN`] in a payload
//!    the client can branch on, and one whose connection is simply closed ends with nothing at
//!    all. AGENTS says open streams are "cancelled, never awaited, on shutdown" — cancelled
//!    *with a reason* is what that costs, and reversing steps 3 and 4 would silently buy the
//!    cheaper half.
//! 4. **Stop accepting** — `crate::uds::Serving::stop`. No new connections; the ones that are
//!    up finish the answer they are writing.
//! 5. **Drain, bounded** — [`limits::DAEMON_SHUTDOWN_DRAIN_MS`]. The requests that were
//!    already accepted get that long and no longer. On expiry the daemon says so at `warn`,
//!    naming the bound and that work was still in flight, and stops anyway (AGENTS rule 3: an
//!    auto-skipping wait is never a silence).
//! 6. **Join housekeeping** — the idle-sweep driver observes the same token and ends itself
//!    (`crate::server::Wchd::spawn_idle_sweeps_every`), and this is where that is *awaited*
//!    rather than assumed. A detached task's ending is a maybe; a joined one's is a fact
//!    something waited for, which is the only version of it a test can assert.
//! 7. **Return.** `main` drops `OwnedState` afterwards, which is the ordered store-lock
//!    release `crate::state`'s header defers to this sub-milestone — "P4e-ii's store-lock
//!    release is about doing it in an order, not about doing it at all".
//!
//! ## SIGTERM ≡ SIGINT
//!
//! AGENTS states it as a daemon rule and this is where it is true: both signals arrive at
//! [`Signalling::next`] and both produce the *same* seven steps. [`Stop`] exists so the log
//! line can name which one asked, and for nothing else — no branch in this module reads it,
//! which is why the tests assert on the observable teardown rather than on the enum. A person
//! pressing Ctrl-C in a terminal and systemd stopping a unit are the same request, and a
//! daemon that drained for one and not the other would be a daemon whose interactive
//! behaviour was not the behaviour under test.
//!
//! ## What a cancellation reaches, and what it does not
//!
//! A [`tokio_util::sync::CancellationToken`] reaches **tokio tasks**. Every subscription is
//! one, so every subscription ends with a reason. The camera actors are not: `engine::actor`
//! gives each open camera its own `std::thread` (`crates/engine/src/actor.rs:442`) because
//! V4L2 ioctls and `DQBUF` block, and a thread parked in `write(2)` on a hung mount cannot be
//! interrupted by a token, a `select!`, or anything else this workspace links. That is note
//! **N59**'s residual, restated at exactly its size and no larger:
//!
//! - the drain bound is what stops such a thread from making the *stop* unbounded — the
//!   daemon gives it [`limits::DAEMON_SHUTDOWN_DRAIN_MS`] and then carries on;
//! - the process exiting is what ends the thread, and the kernel closing its descriptors is
//!   what releases the camera.
//!
//! Nothing here claims to cancel a device operation, and nothing here should be read as
//! claiming it. Ending such a command outright would need a cancellable device thread, which
//! nothing in D12 provides.

use std::time::Duration;

use schema::{Error, Result, limits};
use tokio_util::sync::CancellationToken;

/// The daemon's one "we are stopping" fact, as something every task can await.
///
/// A newtype over [`tokio_util::sync::CancellationToken`] rather than the token itself, for
/// the reason `engine::store` newtypes a lock and `crate::state` hands out an `Arc` of it:
/// the type is what says *which* fact this is. A bare token in `crate::server::Inner` would
/// be one cancellation token among the several a future daemon might have, and "the one the
/// subscriptions watch" would be a convention rather than a type.
///
/// **A clone is not a second token.** `CancellationToken::clone` shares one piece of state,
/// exactly as `Arc<StoreLock>` shares one open file description, so threading this to
/// `crate::events` and to the idle-sweep driver leaves one answer to "is the daemon
/// stopping" reached from three places — never three answers.
///
/// Not `Copy` and deliberately cheap to `Clone`: the composition root makes one, and
/// everything that has to notice a stop takes a clone of it.
#[derive(Debug, Clone, Default)]
pub struct Shutdown(CancellationToken);

impl Shutdown {
    /// A daemon that has not been asked to stop.
    #[must_use]
    pub fn new() -> Shutdown {
        Shutdown::default()
    }

    /// Ask everything watching this token to end. Idempotent, and never blocks.
    ///
    /// Cancelling a cancelled token is a no-op, which is what makes it safe to call from a
    /// teardown that may already have run — and what makes "the daemon is stopping" a fact
    /// rather than an event with an edge to miss: a task that checks *after* the cancellation
    /// sees it just as a task parked in [`Shutdown::cancelled`] does.
    pub fn cancel(&self) {
        self.0.cancel();
    }

    /// Resolve when the daemon has been asked to stop; already resolved if it has.
    ///
    /// Cancel-safe, and it has to be: every caller uses it as one arm of a `tokio::select!`
    /// beside the work it is racing, so it is dropped and rebuilt on every turn of a loop.
    pub async fn cancelled(&self) {
        self.0.cancelled().await;
    }

    /// Whether the daemon has been asked to stop, at this instant.
    ///
    /// The question [`Shutdown::cancelled`] cannot answer for a caller that is not in a
    /// position to wait — and the one a test uses to say *when* a step happened relative to
    /// the cancellation, which is how the order above is asserted rather than described.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.0.is_cancelled()
    }
}

/// Which signal asked the daemon to stop.
///
/// The two are the same path — see this module's header on `SIGTERM ≡ SIGINT` — and this
/// exists so the one line an operator reads can name which one arrived. Nothing branches on
/// it, which is deliberate: a `match` here is how the two would drift apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stop {
    /// `SIGTERM`: a service manager stopping the unit, or `kill`.
    Terminate,
    /// `SIGINT`: Ctrl-C in the terminal the daemon was started from.
    Interrupt,
}

impl Stop {
    /// The signal's own name, for the log line.
    ///
    /// The kernel's spelling rather than the variant's, because an operator reading
    /// `journalctl` is holding a `systemctl stop` or a Ctrl-C and not this enum.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Stop::Terminate => "SIGTERM",
            Stop::Interrupt => "SIGINT",
        }
    }
}

/// Where a stop request comes from — the seam under [`serve_until_stopped`].
///
/// A seam for `crate::uds::Accepting`'s reason: the behaviour worth asserting is what the
/// daemon *does* about a stop, and arranging a real `SIGTERM` to assert it would mean a test
/// that signals its own process — which changes a process-global disposition every other test
/// in the binary shares, and which cannot express "a stop that never comes" at all. So the
/// source is a parameter, with the real [`Signals`] and a scriptable double, and the six steps
/// after it are driven from a value.
pub trait Signalling: Send + 'static {
    /// The next stop request. Resolves once; nothing calls it twice.
    fn next(&mut self) -> impl Future<Output = Stop> + Send;
}

/// The real thing: `SIGTERM` and `SIGINT`, from `tokio::signal::unix`.
///
/// **Not `tokio::signal::ctrl_c`**, which is `SIGINT` alone and would leave `SIGTERM` at the
/// kernel's default disposition — the whole of what P4b had, and the half a service manager
/// actually uses. Both streams are held in one value and raced in one `select!` because the
/// two are one request (this module's header).
///
/// Registering the handlers is a process-global act, which is why it happens in the
/// composition root and nowhere else: `tokio::signal` installs a handler for the process on
/// first use and never restores the previous disposition, so a library that did it would be
/// changing the signal behaviour of every binary that linked it.
#[derive(Debug)]
pub struct Signals {
    terminate: tokio::signal::unix::Signal,
    interrupt: tokio::signal::unix::Signal,
}

impl Signals {
    /// Register both handlers on this process.
    ///
    /// Must be called from inside a tokio runtime: the registration goes through the
    /// runtime's signal driver.
    ///
    /// # Errors
    ///
    /// [`Error::DeviceIo`] when the kernel refuses the registration — the same variant
    /// `engine::actor` answers with when it cannot start a thread and `crate::server::mount`
    /// answers with when the surface will not compose, and for the same reason: it is *this
    /// process* failing to perform an operation, which must never be spelled like a device
    /// declining one (E3, AGENTS rule 7).
    pub fn real() -> Result<Signals> {
        Ok(Signals {
            terminate: register(tokio::signal::unix::SignalKind::terminate(), "SIGTERM")?,
            interrupt: register(tokio::signal::unix::SignalKind::interrupt(), "SIGINT")?,
        })
    }
}

/// One signal handler, or the refusal that says which one could not be installed.
fn register(
    kind: tokio::signal::unix::SignalKind,
    named: &str,
) -> Result<tokio::signal::unix::Signal> {
    tokio::signal::unix::signal(kind).map_err(|err| Error::DeviceIo {
        operation: format!("install a handler for {named}"),
        errno: err.raw_os_error(),
        message: err.to_string(),
    })
}

impl Signalling for Signals {
    async fn next(&mut self) -> Stop {
        tokio::select! {
            received = self.terminate.recv() => stopped_by(Stop::Terminate, received),
            received = self.interrupt.recv() => stopped_by(Stop::Interrupt, received),
        }
    }
}

/// What a signal stream that ended means.
///
/// `Signal::recv` answers `None` when the stream will produce nothing further, which for a
/// registered Unix signal does not happen — the driver lives as long as the runtime. It is
/// still written rather than assumed away, and it is read as a **stop request**, because the
/// alternative for a caller inside a `select!` is a branch that resolves instantly for ever:
/// a daemon spinning at 100% of a core is a worse answer to "the signal driver went away"
/// than a daemon that stops.
fn stopped_by(stop: Stop, received: Option<()>) -> Stop {
    if received.is_none() {
        tracing::warn!(
            signal = stop.as_str(),
            "the signal driver ended; treating that as a request to stop"
        );
    }
    stop
}

/// What the daemon tells the thing supervising it — the seam the systemd integration plugs
/// into.
///
/// Three sentences of `sd_notify(3)`'s vocabulary and no more: `READY=1`, `STATUS=…` and
/// `STOPPING=1`. They are a *trait* rather than three calls into a crate because the daemon
/// has to run identically with nobody listening — from a shell, from a test, from a container
/// without a notify socket — and because the order in which they are sent relative to the
/// teardown is a claim this module makes (see the header) and therefore a claim something has
/// to be able to assert. A recording double is the only way to assert an order of
/// side-effects that leave the process.
///
/// This task ships the trait, [`Unsupervised`], and the recording double its tests use; the
/// real `sd-notify` implementation, socket activation and the journald layer are the
/// follow-up's (note **N58**'s split).
pub trait Notifying: Send + Sync {
    /// The daemon is serving. Sent once, from the composition root, after the socket is bound.
    fn ready(&self, status: &str);

    /// A one-line description of what the daemon is doing, for an operator reading
    /// `systemctl status`.
    fn status(&self, status: &str);

    /// The daemon has begun stopping. Sent **first** in the teardown, for the reason step 2
    /// of this module's header gives.
    fn stopping(&self, status: &str);
}

/// Nobody is supervising this process, so there is nobody to tell.
///
/// The shipped implementation until the follow-up swaps it, and the correct one for every
/// `wchd` started from a shell. It is a no-op rather than a log line: what these calls carry
/// is already logged as the events they describe, and a second copy on stderr would be the
/// daemon narrating its own bookkeeping.
#[derive(Debug, Clone, Copy)]
pub struct Unsupervised;

impl Notifying for Unsupervised {
    fn ready(&self, _status: &str) {}

    fn status(&self, _status: &str) {}

    fn stopping(&self, _status: &str) {}
}

/// Serve until a stop is asked for, then tear down in the order this module's header states.
///
/// The composition root's main loop: it returns when the daemon has stopped, and what it
/// returns is why. `main`'s `?` is what turns "the accept loop gave up" into a non-zero exit
/// and therefore into a restart.
///
/// `housekeeping` is the idle-sweep driver's handle. It is a parameter rather than something
/// this starts, because the composition root is where D12's driver is started and a function
/// that both started and joined it would be a second opinion about whether the daemon has one.
///
/// # Errors
///
/// Whatever `crate::uds::Serving::stopped` refuses with — [`Error::DeviceIo`] for an accept
/// loop that gave up after [`limits::MAX_CONSECUTIVE_ACCEPT_FAILURES`] consecutive failures,
/// carrying the last errno. A stop somebody asked for is `Ok(())`, **including** one whose
/// drain ran out of time: the daemon was asked to stop and it stopped, and the work that was
/// still in flight is a `warn!` rather than an exit code, because a non-zero exit there would
/// ask `Restart=on-failure` to start the daemon again on a machine that is shutting down.
pub async fn serve_until_stopped(
    serving: &mut crate::uds::Serving,
    shutdown: &Shutdown,
    signals: impl Signalling,
    notify: &dyn Notifying,
    housekeeping: tokio::task::JoinHandle<()>,
) -> Result<()> {
    stop_in_order(serving, shutdown, signals, notify, housekeeping).await
}

/// The transport being stopped — the seam under [`serve_until_stopped`].
///
/// `crate::uds::Accepting`'s argument one layer out. The three cases the order has to survive
/// are "a transport that stops when it is asked", "a transport that gave up before anybody
/// asked" and "a transport that never finishes draining", and the third is the one no real
/// server can be *made* to do on demand: it would mean holding a connection open past a
/// graceful stop, which is a fact about hyper's state machine rather than about this daemon.
/// So the transport is a parameter, with `crate::uds::Serving` as the real implementation and
/// a scriptable double beside it.
///
/// Two methods rather than one because a stop is asked for while something else is waiting:
/// `stop` takes `&self` and `stopped` takes `&mut self`, which is exactly the shape
/// [`stop_in_order`] needs and the reason the drain can be a second call rather than a second
/// object.
trait Stopping: Send {
    /// End the accept loop. Idempotent.
    fn stop(&self);

    /// Wait until the accept loop and every connection it spawned are gone, and answer why
    /// the loop ended. Cancel-safe, and callable twice — see `crate::uds::Serving::stopped`,
    /// where both properties are argued and where they cost something.
    fn stopped(&mut self) -> impl Future<Output = Result<()>> + Send;
}

impl Stopping for crate::uds::Serving {
    fn stop(&self) {
        crate::uds::Serving::stop(self);
    }

    fn stopped(&mut self) -> impl Future<Output = Result<()>> + Send {
        crate::uds::Serving::stopped(self)
    }
}

/// Why the daemon is stopping, as the value the teardown carries to its end.
///
/// A value rather than a `bool`, because the two endings differ in what the process's exit
/// code has to be and that difference is the reason `crate::uds::Serving::stopped` answers a
/// `Result` at all.
#[derive(Debug)]
enum Ending {
    /// Somebody asked. The ordinary case, and a clean exit.
    Asked(Stop),
    /// The accept loop ended by itself, which is a failure however this teardown goes.
    AcceptLoopEnded(Result<()>),
}

impl Ending {
    /// What `main` should exit on, for a teardown that could not get the answer again.
    ///
    /// Only the drain-expiry path needs this: everywhere else the accept loop's own verdict is
    /// available from the drain, and it is the same value.
    ///
    /// **The [`Ending::AcceptLoopEnded`] arm cannot be reached through `crate::uds::Serving`**,
    /// and it is written anyway. That transport's `stopped()` resolves only once the accept
    /// task *and* every connection are gone, so an ending that arrived through it has already
    /// drained by construction and the timeout below cannot fire after one. What the arm is
    /// for is the same thing `crate::uds`'s `BindAddress::ByName` is for: the [`Stopping`] seam
    /// can produce the combination, a test does, and a build that answered `Ok(())` here would
    /// turn a daemon that stopped serving into a clean exit for want of one branch nobody had
    /// ever executed.
    fn outcome(self) -> Result<()> {
        match self {
            Ending::Asked(_) => Ok(()),
            Ending::AcceptLoopEnded(outcome) => outcome,
        }
    }
}

/// [`serve_until_stopped`], over anything that can be stopped. See [`Stopping`].
async fn stop_in_order<T: Stopping>(
    transport: &mut T,
    shutdown: &Shutdown,
    mut signals: impl Signalling,
    notify: &dyn Notifying,
    housekeeping: tokio::task::JoinHandle<()>,
) -> Result<()> {
    // 1. Whichever comes first. The transport arm is not an alternative way of asking to
    //    stop — it is the accept loop having given up — so it is logged differently and it
    //    changes what this function returns.
    let ending = tokio::select! {
        stop = signals.next() => Ending::Asked(stop),
        served = transport.stopped() => Ending::AcceptLoopEnded(served),
    };
    match &ending {
        Ending::Asked(stop) => {
            // The one line an operator looks for when a daemon goes away, and it names which
            // request arrived because "systemd stopped it" and "somebody pressed Ctrl-C" are
            // different things to be told at 3am. What happens next is identical.
            tracing::info!(signal = stop.as_str(), "wchd is stopping");
        }
        Ending::AcceptLoopEnded(Err(error)) => {
            // The accept loop already logged the errno and the run of failures; what this
            // adds is that the *daemon* is now tearing down because of it, which is the fact
            // the lines below belong to.
            tracing::warn!(%error, "the daemon stopped serving without being asked to; tearing down");
        }
        Ending::AcceptLoopEnded(Ok(())) => {
            // The transport stopped and refused nothing, which in the shipped daemon means
            // somebody else holds a stop handle — nobody does. Written rather than folded
            // into the arm above because a `warn` that named an error there would be
            // inventing one, and because this is the shape a future second caller of
            // `Serving::stop` would arrive in.
            tracing::warn!("the daemon stopped serving without being asked to; tearing down");
        }
    }

    // 2. Before anything else takes time. See the header.
    notify.stopping("stopping: cancelling subscriptions and draining in-flight requests");

    // 3. Every open subscription ends with a reason rather than with a socket that closed
    //    under it — and it ends *before* the transport that carries it is stopped, which is
    //    the whole difference this step buys.
    shutdown.cancel();

    // 4. No new connections.
    transport.stop();

    // 5. The drain, bounded. Called on the same value a `select!` arm above may have
    //    cancelled mid-flight, which is why `Serving::stopped` is cancel-safe and answers
    //    twice; on the give-up path this is what waits for the connections that loop left up.
    let drain = Duration::from_millis(limits::DAEMON_SHUTDOWN_DRAIN_MS);
    let outcome = match tokio::time::timeout(drain, transport.stopped()).await {
        Ok(verdict) => verdict,
        Err(_elapsed) => {
            // Never a silence (AGENTS rule 3). It names the bound because the sentence an
            // operator needs is "this was our deadline, not the kernel's" — and because the
            // work that outlived it is very often a calibration sweep, which
            // `limits::DAEMON_SHUTDOWN_DRAIN_MS` explains is recoverable from the session
            // document rather than lost.
            tracing::warn!(
                drain_ms = limits::DAEMON_SHUTDOWN_DRAIN_MS,
                "requests were still in flight when the shutdown drain ran out; stopping anyway"
            );
            notify.status("stopping: work was still in flight when the drain bound expired");
            ending.outcome()
        }
    };

    // 6. The idle-sweep driver observes the same token, so this is a join and not a wait:
    //    it has already been asked to end. Awaited rather than dropped because "housekeeping
    //    ended" is a fact this order claims, and a detached task's ending is a maybe.
    if let Err(err) = housekeeping.await {
        // A panicked or aborted driver, which is not a reason to fail the stop — there is
        // nothing left for it to sweep — but is exactly the kind of thing that must not
        // disappear because it happened during shutdown.
        tracing::warn!(error = %err, "the idle-sweep driver did not end cleanly");
    }

    // 7. `main` drops `OwnedState` after this returns.
    outcome
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use schema::ErrorKind;
    use tokio::sync::watch;

    use super::*;

    /// At most one stop request, then silence.
    ///
    /// The silence is the half that matters as much as the request, and it does two jobs. A
    /// double that answered repeatedly would let a teardown that looped still pass — "the
    /// second signal is not a second teardown" is a property of the caller rather than of the
    /// source. And `None` is a daemon **nobody signalled**, which is the only honest script for
    /// the case where the accept loop is the thing that ended (see [`teardown`]).
    struct Scripted(Option<Stop>);

    impl Signalling for Scripted {
        async fn next(&mut self) -> Stop {
            match self.0.take() {
                Some(stop) => stop,
                None => std::future::pending().await,
            }
        }
    }

    /// What a scripted transport does when it is waited on.
    #[derive(Debug, Clone)]
    enum Answer {
        /// The ordinary case: it ends when it is stopped.
        WhenStopped,
        /// It ended before anybody asked, carrying the accept loop's refusal.
        GaveUp(Error),
        /// It never finishes draining — a connection that outlives the bound.
        Never,
        /// It gave up before anybody asked, and then never finished draining what that loop
        /// had left up. **The real transport cannot do this** — see [`Ending::outcome`] — and
        /// that is exactly why it is here: the arm it drives is otherwise a branch nothing
        /// reaches and no test can turn red, which is the objection `crate::uds`'s
        /// `BindAddress::ByName` answers one module along and in the same way.
        GaveUpThenNever(Error),
    }

    /// The steps of a teardown, in the order they happened, each carrying whether the
    /// cancellation had already reached the subscriptions when it did.
    ///
    /// The flag is what makes the *order* assertable without a scheduler in it: "the
    /// subscriptions had not been cancelled when the supervisor was told" and "they had been
    /// by the time the accept loop stopped" are two readings of one token taken at two
    /// synchronous call sites, rather than two tasks racing to record themselves.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Step {
        Ready { cancelled: bool },
        Stopping { cancelled: bool },
        Status { cancelled: bool },
        TransportStopped { cancelled: bool },
    }

    /// Everything a teardown did, in order.
    #[derive(Debug)]
    struct Recorder {
        shutdown: Shutdown,
        steps: std::sync::Mutex<Vec<Step>>,
    }

    impl Recorder {
        fn new(shutdown: &Shutdown) -> Arc<Recorder> {
            Arc::new(Recorder {
                shutdown: shutdown.clone(),
                steps: std::sync::Mutex::new(Vec::new()),
            })
        }

        /// Record one step, stamped with the token's state at the instant it happened.
        fn record(&self, step: impl FnOnce(bool) -> Step) {
            let cancelled = self.shutdown.is_cancelled();
            self.steps
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(step(cancelled));
        }

        fn steps(&self) -> Vec<Step> {
            self.steps
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone()
        }
    }

    /// A supervisor that writes down what it was told and when.
    struct Recording(Arc<Recorder>);

    impl Notifying for Recording {
        fn ready(&self, _status: &str) {
            self.0.record(|cancelled| Step::Ready { cancelled });
        }

        fn status(&self, _status: &str) {
            self.0.record(|cancelled| Step::Status { cancelled });
        }

        fn stopping(&self, _status: &str) {
            self.0.record(|cancelled| Step::Stopping { cancelled });
        }
    }

    /// A transport that answers however it was scripted to.
    struct Scriptable {
        answer: Answer,
        /// Whether `stop` has been called, as something [`Answer::WhenStopped`] can await —
        /// a flag with a channel behind it rather than a poll, for the reason nothing in this
        /// workspace sleeps to synchronize.
        stopped: watch::Sender<bool>,
        recorder: Arc<Recorder>,
    }

    impl Scriptable {
        fn new(answer: Answer, recorder: &Arc<Recorder>) -> Scriptable {
            Scriptable {
                answer,
                stopped: watch::Sender::new(false),
                recorder: Arc::clone(recorder),
            }
        }
    }

    impl Stopping for Scriptable {
        fn stop(&self) {
            self.recorder
                .record(|cancelled| Step::TransportStopped { cancelled });
            self.stopped.send_replace(true);
        }

        async fn stopped(&mut self) -> Result<()> {
            match self.answer.clone() {
                Answer::GaveUp(refusal) => Err(refusal),
                Answer::GaveUpThenNever(refusal) => {
                    self.answer = Answer::Never;
                    Err(refusal)
                }
                Answer::Never => std::future::pending().await,
                Answer::WhenStopped => {
                    let mut asked = self.stopped.subscribe();
                    asked
                        .wait_for(|stopped| *stopped)
                        .await
                        .expect("the sender outlives this borrow");
                    Ok(())
                }
            }
        }
    }

    /// A housekeeping task that records, at a moment only a *join* can be sure of, that it
    /// ended.
    ///
    /// The sleep is not synchronization and never runs on a clock: these tests are on tokio's
    /// paused clock, where an idle runtime advances time to the next timer, so a teardown that
    /// awaits this handle lets it finish and one that dropped the handle returns before the
    /// runtime was ever idle. Both directions, deterministically, with nothing waited for in
    /// wall time.
    fn housekeeping_that_announces_its_end()
    -> (tokio::task::JoinHandle<()>, Arc<watch::Sender<bool>>) {
        let ended = Arc::new(watch::Sender::new(false));
        let announce = Arc::clone(&ended);
        let handle = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(1)).await;
            announce.send_replace(true);
        });
        (handle, ended)
    }

    /// A teardown driven from a scripted signal and a scripted transport.
    ///
    /// `signalled` is an `Option` rather than a `Stop`, and the `None` is load-bearing: step 1
    /// is a race, so a script in which *both* arms are ready is a test whose ending
    /// `tokio::select!` picks at random. The daemon has no such state — either somebody
    /// signalled a daemon that was serving, or an accept loop gave up on a daemon nobody
    /// signalled — so every case below scripts exactly one of them ready and the race is
    /// removed rather than tolerated.
    async fn teardown(signalled: Option<Stop>, answer: Answer) -> (Result<()>, Vec<Step>, bool) {
        let shutdown = Shutdown::new();
        let recorder = Recorder::new(&shutdown);
        let notify = Recording(Arc::clone(&recorder));
        let mut transport = Scriptable::new(answer, &recorder);
        let (housekeeping, ended) = housekeeping_that_announces_its_end();

        let outcome = stop_in_order(
            &mut transport,
            &shutdown,
            Scripted(signalled),
            &notify,
            housekeeping,
        )
        .await;

        assert!(
            shutdown.is_cancelled(),
            "the teardown returned without cancelling the subscriptions"
        );
        (outcome, recorder.steps(), *ended.borrow())
    }

    #[tokio::test(start_paused = true)]
    async fn sigterm_and_sigint_are_one_teardown() {
        // AGENTS's "SIGTERM ≡ SIGINT", asserted on what the daemon *did* rather than on the
        // enum that named the signal — an assertion about the enum would pass on a build that
        // branched on it two lines later. The comparison is of the whole recording, so a
        // difference in any step, in the order of the steps, or in what the process would exit
        // with is a red test.
        let (terminated, term_steps, term_joined) =
            teardown(Some(Stop::Terminate), Answer::WhenStopped).await;
        let (interrupted, int_steps, int_joined) =
            teardown(Some(Stop::Interrupt), Answer::WhenStopped).await;

        assert_eq!(terminated, Ok(()));
        assert_eq!(interrupted, Ok(()));
        assert_eq!(
            term_steps, int_steps,
            "the two signals took different paths"
        );
        assert_eq!((term_joined, int_joined), (true, true));
    }

    #[tokio::test(start_paused = true)]
    async fn the_supervisor_is_told_before_the_subscriptions_end_and_before_the_socket_closes() {
        // Steps 2, 3 and 4 of this module's header, which are an *order* and therefore the
        // one thing about them worth asserting. Each step carries the token's state at the
        // instant it ran:
        //
        // - `Stopping { cancelled: false }` — the supervisor heard about the stop before
        //   anything else took time, which is what stops it counting a drain against a
        //   start/stop timeout;
        // - `TransportStopped { cancelled: true }` — every subscription had already been
        //   ended *with a reason* before the transport carrying it stopped. Swapping steps 3
        //   and 4 leaves this `false`, and a client's stream ending with nothing at all.
        let (outcome, steps, joined) = teardown(Some(Stop::Terminate), Answer::WhenStopped).await;

        assert_eq!(outcome, Ok(()));
        assert_eq!(
            steps,
            vec![
                Step::Stopping { cancelled: false },
                Step::TransportStopped { cancelled: true },
            ]
        );
        // Step 6, and the reason it is a join: a driver whose handle was dropped is a driver
        // whose ending nothing waited for.
        assert!(
            joined,
            "the teardown returned before housekeeping had ended"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn an_accept_loop_that_gave_up_is_reported_rather_than_swallowed() {
        // `crate::uds::Serving`'s header, carried all the way to `main`: a daemon that stopped
        // accepting has stopped serving, and reporting it as a clean exit tells
        // `Restart=on-failure` not to restart it. The teardown still runs — a give-up is not a
        // reason to leave subscriptions hanging — which is the half a `return` on this path
        // would silently drop.
        //
        // Nobody signalled this daemon, which is what makes it the accept loop's ending rather
        // than a race between two ready arms (see [`teardown`]).
        const EMFILE: i32 = 24;
        let refusal = Error::DeviceIo {
            operation: "accept a connection on the daemon socket".to_owned(),
            errno: Some(EMFILE),
            message: "64 consecutive failures".to_owned(),
        };

        let (outcome, steps, joined) = teardown(None, Answer::GaveUp(refusal.clone())).await;

        assert_eq!(outcome, Err(refusal));
        assert_eq!(outcome.expect_err("reported").kind(), ErrorKind::DeviceIo);
        assert_eq!(
            steps,
            vec![
                Step::Stopping { cancelled: false },
                Step::TransportStopped { cancelled: true },
            ],
            "an accept loop that gave up skipped part of the teardown"
        );
        assert!(
            joined,
            "the teardown returned before housekeeping had ended"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_drain_that_runs_out_of_time_says_so_and_stops_anyway() {
        // The bound `limits::DAEMON_SHUTDOWN_DRAIN_MS` exists for, and the case its doc says
        // is the *usual* one on real hardware: a calibration sweep holds the session mutex for
        // minutes, so a stop asked for mid-sweep reaches the bound rather than the sweep's
        // end. Three things have to be true and none implies the others — it ends, it says
        // why, and the rest of the teardown still happens.
        //
        // Nothing waits in wall time: the clock is paused, so `tokio::time::timeout` fires
        // when the runtime goes idle. A build that dropped the bound hangs here and becomes a
        // named nextest TIMEOUT rather than a green test.
        let shutdown = Shutdown::new();
        let recorder = Recorder::new(&shutdown);
        let notify = Recording(Arc::clone(&recorder));
        let mut transport = Scriptable::new(Answer::Never, &recorder);
        let (housekeeping, ended) = housekeeping_that_announces_its_end();

        let (outcome, logged) = crate::logging::captured(async {
            stop_in_order(
                &mut transport,
                &shutdown,
                Scripted(Some(Stop::Terminate)),
                &notify,
                housekeeping,
            )
            .await
        })
        .await;

        // Asked to stop, and it stopped: an exit code that asked a service manager to restart
        // the daemon on a machine that is shutting down would be the wrong answer to
        // "somebody was still talking to it".
        assert_eq!(outcome, Ok(()));
        assert!(
            logged.contains("WARN"),
            "the drain expired silently: {logged:?}"
        );
        assert!(
            logged.contains(&limits::DAEMON_SHUTDOWN_DRAIN_MS.to_string()),
            "the warning did not name the bound: {logged:?}"
        );
        assert_eq!(
            recorder.steps(),
            vec![
                Step::Stopping { cancelled: false },
                Step::TransportStopped { cancelled: true },
                Step::Status { cancelled: true },
            ],
            "a drain that ran out of time left the teardown half done"
        );
        assert!(
            ended.borrow().to_owned(),
            "the teardown returned before housekeeping had ended"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_daemon_that_gave_up_and_could_not_drain_still_exits_on_the_failure() {
        // The one combination `crate::uds::Serving` cannot produce and the [`Stopping`] seam
        // can — [`Ending::outcome`] says why it is written anyway. Both halves of the answer
        // matter here and they pull in opposite directions: the drain expiring is *not* a
        // failure (a stop is a stop), and the accept loop having given up *is* one however the
        // stop went, so a build that let the expiry decide the exit code would restart nothing
        // after the failure that most needs a restart.
        const EMFILE: i32 = 24;
        let refusal = Error::DeviceIo {
            operation: "accept a connection on the daemon socket".to_owned(),
            errno: Some(EMFILE),
            message: "64 consecutive failures".to_owned(),
        };

        let (outcome, steps, joined) =
            teardown(None, Answer::GaveUpThenNever(refusal.clone())).await;

        assert_eq!(outcome, Err(refusal));
        assert_eq!(
            steps,
            vec![
                Step::Stopping { cancelled: false },
                Step::TransportStopped { cancelled: true },
                Step::Status { cancelled: true },
            ],
            "the drain expired without the teardown finishing"
        );
        assert!(
            joined,
            "the teardown returned before housekeeping had ended"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_drain_that_finishes_inside_the_bound_says_nothing() {
        // The other direction of the same claim, and the reason the assertion above can go
        // red: a build that warned unconditionally would pass it while telling every operator
        // of every clean stop that their requests had been cut off.
        let shutdown = Shutdown::new();
        let recorder = Recorder::new(&shutdown);
        let notify = Recording(Arc::clone(&recorder));
        let mut transport = Scriptable::new(Answer::WhenStopped, &recorder);
        let (housekeeping, _ended) = housekeeping_that_announces_its_end();

        let (outcome, logged) = crate::logging::captured(async {
            stop_in_order(
                &mut transport,
                &shutdown,
                Scripted(Some(Stop::Interrupt)),
                &notify,
                housekeeping,
            )
            .await
        })
        .await;

        assert_eq!(outcome, Ok(()));
        assert!(
            !logged.contains("WARN"),
            "a clean stop reported work it had cut off: {logged:?}"
        );
    }

    #[tokio::test]
    async fn the_real_source_answers_both_signals_and_nothing_else() {
        // The real seam, driven for real: `Signals::real()` registers two handlers on this
        // process and both of them have to reach the same `next`. A double cannot assert this
        // — what it would assert is that a value this test wrote is the value this test read —
        // and the half that has actually been wrong in other daemons is registering `SIGINT`
        // alone (`tokio::signal::ctrl_c`) and leaving `SIGTERM` at the kernel's default, which
        // is the disposition that kills the process instead of draining it.
        //
        // Signalling this process is safe here because `cargo nextest` runs every test in its
        // own process; the handlers registered below are this test's and outlive nothing.
        let mut signals = Signals::real().expect("this host allows a SIGTERM and SIGINT handler");
        let me = rustix::process::getpid();

        rustix::process::kill_process(me, rustix::process::Signal::TERM).expect("ours to signal");
        assert_eq!(signals.next().await, Stop::Terminate);

        rustix::process::kill_process(me, rustix::process::Signal::INT).expect("ours to signal");
        assert_eq!(signals.next().await, Stop::Interrupt);
    }

    #[test]
    fn a_stop_names_the_signal_the_operator_sent() {
        // The one thing the enum is for. A rename that made these two agree — or that spelled
        // them as variant names — would leave an operator reading `journalctl` unable to tell
        // a `systemctl stop` from somebody's Ctrl-C.
        assert_eq!(Stop::Terminate.as_str(), "SIGTERM");
        assert_eq!(Stop::Interrupt.as_str(), "SIGINT");
    }

    #[tokio::test]
    async fn a_cancelled_token_stays_cancelled_and_every_clone_sees_it() {
        // The property everything downstream rests on: `crate::events`' subscriptions and the
        // idle-sweep driver each hold a *clone*, and if a clone were a second token they would
        // watch a fact nobody sets. Both directions — a fresh token is not cancelled, and a
        // task that checks after the cancellation sees it exactly as one parked in `cancelled`
        // does, which is why there is no edge to miss.
        let shutdown = Shutdown::new();
        let elsewhere = shutdown.clone();
        assert!(!shutdown.is_cancelled());
        assert!(!elsewhere.is_cancelled());

        shutdown.cancel();
        assert!(elsewhere.is_cancelled(), "a clone was a second token");
        elsewhere.cancelled().await;
        // Idempotent: a teardown that ran twice must not be a different teardown.
        elsewhere.cancel();
        assert!(shutdown.is_cancelled());
    }
}
