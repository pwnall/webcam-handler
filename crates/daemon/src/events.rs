//! The two things the daemon says without being asked (design D10, docs/7 P4e-i).
//!
//! `webcam-handler-api`'s second generated trait declares `subscribe_events` and
//! `subscribe_calibration`; `crate::server` implements them; this module is everything
//! underneath — where the events come from, how one source reaches N subscribers, and what
//! a subscriber that stops reading costs. It is transport code, which is where
//! `engine::progress`'s header already said this half belongs: "P4e's daemon bridges this
//! receiver onto whatever its subscription speaks. That bridge is transport code."
//!
//! ## One doctrine, three hops
//!
//! The claim P4e-i is named for is *nothing a client does can wedge the daemon*, and it is
//! made true by there being **no unbounded queue and no blocking send anywhere between a
//! device and a socket**. Three hops, three bounds, and at every one of them the answer to
//! "the consumer is behind" is the same as `engine::progress::ChannelSink`'s: drop, and
//! count.
//!
//! | Hop | Bound | At the bound |
//! |---|---|---|
//! | producer → fan-out | [`limits::SUBSCRIPTION_BROADCAST_DEPTH`] | the *oldest* event is dropped for the receiver that is behind; the producer never waits, and other receivers see nothing |
//! | fan-out → one subscription | none — the subscription task holds one event at a time | — |
//! | subscription → socket | [`limits::WS_MESSAGE_BUFFER_CAPACITY`] | the newest is dropped and counted (`Fanout::dropped`); only that connection is affected |
//! | connection → subscriptions | [`limits::RPC_MAX_SUBSCRIPTIONS_PER_CONNECTION`] | jsonrpsee refuses the *subscribe call* before the handler runs (`-32006`) |
//!
//! The producers are a camera actor's own thread (a sweep, emitting through
//! `ProgressBroadcast`) and one OS thread per running hotplug watch. Neither is a tokio
//! task and neither may block: `tokio::sync::broadcast::Sender::send` is a plain
//! synchronous function that needs no runtime context and never waits, which is the whole
//! reason the fan-out is a `broadcast` rather than an `mpsc` behind a mutex.
//!
//! ## What a lagging subscriber is told, and why the two streams differ
//!
//! `broadcast` reports a receiver's loss as `RecvError::Lagged(n)` — dropped *and counted*,
//! for free, which is rubric rule 3's requirement. What each stream does with that count is
//! not the same, and the difference is in the payload rather than in the transport:
//!
//! - **Hotplug ends the stream.** A `HotplugEvent` is a delta, the vocabulary is closed
//!   (`Added`/`Removed`, no "you missed some"), and a gap leaves a consumer's picture of
//!   the node tree wrong in a way it cannot detect. Ending with the count is the only
//!   answer that is not a quiet lie; the client re-subscribes and re-enumerates, which is
//!   live every time (E2).
//! - **Calibration keeps going.** Every in-flight `CalibrationProgress` variant carries
//!   `index`/`total` — put there, in as many words, so that "a subscriber that connects
//!   mid-sweep has no earlier events to count" — so a gap is self-healing and the next
//!   event repaints a correct bar. Ending a client's view of a twenty-minute sweep because
//!   it was briefly slow would be the transport inventing a failure the payload already
//!   handles.
//!
//! ## A stream whose *source* stops ends, and says so
//!
//! Falling behind is one thing; the producer going away is another, and there is no policy
//! to have about the second — a stream with nothing behind it has nothing to deliver on
//! either vocabulary. The only hotplug watch this daemon can lose is one whose `next_event`
//! failed (an unreadable `/sys`, a netlink socket that errored, an fd limit), and the
//! honest answer is `Feed::Ended`: every reader's stream ends carrying [`WATCH_STOPPED`],
//! the client re-subscribes, and the subscriber that re-subscribes starts a fresh watch.
//!
//! The alternative — which a first draft of this module had, and its comment claimed
//! otherwise — is a subscription that stays open, stays counted in
//! [`SubscriptionActivity::live`], and silently delivers nothing for the rest of the
//! process's life. That is rubric A4's shape (a transient failure leaving a client in a
//! state no verb can leave) with a log line asserting the opposite, and note **N59** carries
//! it. `tokio::sync::broadcast` gives a sender no way to close a channel it does not drop,
//! and these senders live as long as the daemon, which is why the terminal is a *value* in
//! the queue rather than a flag beside it.
//!
//! ## Events with nobody listening are dropped and counted (P4e-i's decision)
//!
//! `broadcast::Sender::send` answers `Err` exactly when `receiver_count() == 0`, and
//! `Fanout::unheard` is what turns that into a number. Nothing is buffered for a
//! subscriber who has not arrived: keeping one long-lived `Receiver` parked so that nothing
//! is "lost" would buffer a whole sweep's events for nobody, which is precisely the
//! unbounded growth `limits::PROGRESS_QUEUE_DEPTH`'s doc rejects one crate down. It is also
//! the posture `schema::progress` already documents — a progress event "is allowed to be
//! dropped when nobody is listening" — and the honest one: a client that was not there did
//! not miss anything it could have acted on, and the session document has the whole sweep
//! either way (D9).

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use engine::actor::Cameras;
use jsonrpsee::core::{SubscriptionError, SubscriptionResult, to_json_raw_value};
use jsonrpsee::{PendingSubscriptionSink, TrySendError};
use schema::backend::HotplugEvent;
use schema::progress::ProgressEvent;
use schema::{Error, Result, limits};
use serde::Serialize;
use tokio::sync::{broadcast, watch};

/// What a lag close carries back to the client that lagged.
///
/// A typed payload rather than `SubscriptionError::from(…)`'s string: jsonrpsee's blanket
/// `impl<T: ToString>` would flatten this to prose a consumer has to parse, and the whole
/// point of naming the count is that a client can act on it.
#[derive(Debug, Serialize)]
struct Lagged {
    /// How many events this subscriber never saw.
    lagged: u64,
}

/// What the end of a *source* carries back to everybody reading it.
///
/// [`Lagged`]'s sibling and for its reason: a client that is told why its stream ended can
/// decide whether to re-subscribe, and one that is told nothing has to guess. The token is a
/// fixed string rather than an errno or a message because the failure itself is the
/// operator's (it is logged, with its errno) and a client's only move is the same one for
/// every cause — subscribe again, and re-enumerate, which is live every time (E2).
#[derive(Debug, Serialize)]
struct Ended {
    /// Why this stream stopped.
    ended: &'static str,
}

/// The token an `Ended` payload carries when a hotplug watch stopped under its
/// subscribers.
///
/// A `const` because it is the one string on this path and it is asserted by name from two
/// sides — the subscription suite reads it off a real stream, and `Hotplug::watching` writes
/// it — so a build that changed one and not the other is a red test rather than a client
/// branching on a sentence that no longer exists.
pub const WATCH_STOPPED: &str = "the hotplug watch stopped";

/// The token an `Ended` payload carries when the *daemon* is stopping (docs/7 P4e-ii).
///
/// [`WATCH_STOPPED`]'s sibling, pinned for its reason and telling the client something
/// different: the source did not fail, the process is going away, and re-subscribing is worth
/// doing against the daemon that starts next rather than immediately. It is the whole of what
/// `crate::shutdown`'s step 3 buys — a stream that ends *with a reason* instead of a socket
/// that closes underneath a client — so it is asserted from both sides, here and off a real
/// stream in the daemon's subscription suite.
pub const SHUTTING_DOWN: &str = "the daemon is shutting down";

/// What one fan-out hands a subscriber: an event, or the end of the thing producing them.
///
/// **The terminal travels in the channel rather than beside it**, and that ordering is the
/// whole of why it is a value: a subscriber that is behind must receive what it already has
/// before it is told the source stopped, and a signal raced against the queue would end a
/// stream that still had deliverable events in it. `tokio::sync::broadcast` offers a sender
/// no close at all — dropping the `Sender` is the only one, and these senders live as long
/// as the daemon does — so the value carries what the channel cannot.
#[derive(Debug, Clone)]
enum Feed<T> {
    /// One event, for every subscriber attached when it was sent.
    Event(T),
    /// The source stopped: every stream reading this fan-out ends, naming the reason.
    Ended(&'static str),
}

/// Everything the daemon can be subscribed to. One of each, never two.
#[derive(Debug)]
pub(crate) struct Events {
    /// Node arrivals and departures, from a watch that runs only while somebody is
    /// listening — see [`Hotplug`].
    ///
    /// An `Arc` because its watch **thread** has to hold it: the thread outlives the
    /// request that started it and has to reach the same `running` flag a later subscriber
    /// checks, and a second `Hotplug` with a flag of its own is exactly the drift that
    /// would let two threads run against one socket.
    pub(crate) hotplug: Arc<Hotplug>,
    /// Every sweep's progress, from every session. The sink side is [`ProgressBroadcast`].
    pub(crate) calibration: Fanout<ProgressEvent>,
    /// How many subscriptions are open across **both** streams.
    ///
    /// Shared with each [`Fanout`] rather than derived from them, because the question
    /// P4e-i's disconnect assertion asks — "is every subscription this connection owned
    /// gone?" — is about the daemon rather than about a stream, and two counts would be two
    /// things to wait on.
    live: watch::Sender<usize>,
    /// How many events this daemon has failed to deliver, whatever stopped them.
    ///
    /// `live`'s sibling and for its reason: one number across both streams, because the
    /// question it answers — "has this daemon finished losing what it was going to lose?" —
    /// is about the daemon. A `watch` and not an atomic because a loss is an **event**: the
    /// itemised counts in [`StreamActivity`] can be read at any moment, but only a change
    /// can be *waited for*, and a caller that polled a counter instead would be spinning on
    /// the scheduler. Note **N17** pre-authorised exactly this shape — "a query on the sink,
    /// not a failure of `emit`".
    ///
    /// It is the **sum** of what [`StreamActivity::lost`] itemises, kept beside the parts
    /// rather than derived from them because a `watch` has to be *told*. That the two agree
    /// is not left to hope: the daemon's subscription suite waits on this number and then
    /// asserts the itemisation against it, so a bump that reached one and missed the other
    /// is a red test rather than a drift.
    lost: watch::Sender<u64>,
    /// The daemon's "we are stopping" token, watched by every open subscription.
    ///
    /// A clone of the one `crate::shutdown::Shutdown` the composition root made, which is the
    /// same token and not a second one (that type's header says why the distinction is worth
    /// stating). It lives *here* rather than beside the streams because a subscription is the
    /// only thing in this module that waits on anything: the fan-out never blocks a producer,
    /// so the one place a stop has to arrive is the loop between a receiver and a socket.
    shutdown: crate::shutdown::Shutdown,
}

impl Events {
    /// A daemon's event surface, with nothing running yet.
    pub(crate) fn new(shutdown: crate::shutdown::Shutdown) -> Events {
        let live = watch::Sender::new(0);
        let lost = watch::Sender::new(0);
        Events {
            hotplug: Arc::new(Hotplug::new(live.clone(), lost.clone())),
            calibration: Fanout::new(live.clone(), lost.clone()),
            live,
            lost,
            shutdown,
        }
    }

    /// The token every subscription this daemon opens is watching.
    ///
    /// Handed out rather than hidden for `crate::state::OwnedState::token`'s reason: the value
    /// is shareable by construction, so a caller that has it is provably watching the daemon's
    /// one stop rather than a token of its own.
    pub(crate) fn shutdown(&self) -> &crate::shutdown::Shutdown {
        &self.shutdown
    }

    /// What every subscription on this daemon is doing. See [`SubscriptionActivity`].
    pub(crate) fn activity(&self) -> SubscriptionActivity {
        SubscriptionActivity {
            live: *self.live.borrow(),
            hotplug: self.hotplug.events.activity(),
            calibration: self.calibration.activity(),
        }
    }

    /// The live count, as something to await a change on rather than to poll.
    pub(crate) fn watch_live(&self) -> watch::Receiver<usize> {
        self.live.subscribe()
    }

    /// The loss count, as something to await a change on rather than to poll. See
    /// [`Events::lost`].
    pub(crate) fn watch_lost(&self) -> watch::Receiver<u64> {
        self.lost.subscribe()
    }

    /// Whether a hotplug watch thread is running, as something to await rather than poll.
    ///
    /// See [`Hotplug::running`]: the type's header claims the watch exists exactly while
    /// somebody is listening, and this is what makes that a claim something can check.
    pub(crate) fn watch_hotplug_running(&self) -> watch::Receiver<bool> {
        self.hotplug.watch_running()
    }
}

/// One event stream, fanned out to every subscriber, bounded and counted.
///
/// The generic is not abstraction for its own sake: the two streams differ in what they
/// carry and in what a lagging subscriber is told, and in *nothing else*. Writing the
/// second one out would be a second answer to "what happens when a subscriber falls
/// behind", in the file whose whole subject is that there is one.
#[derive(Debug)]
pub struct Fanout<T> {
    /// The fan-out itself. `broadcast` and not `watch`, because a `watch` coalesces and
    /// both of these vocabularies are *deltas*: two plugs that became one reading would be
    /// an event nobody can reconstruct. Not `mpsc`, which is single-consumer, and not a
    /// `Vec<Sender>` behind a mutex, which is a hand-rolled broadcast with a reaping
    /// problem.
    events: broadcast::Sender<Feed<T>>,
    /// Events emitted with nobody subscribed. See this module's header: a documented drop,
    /// not a silence.
    unheard: AtomicU64,
    /// What the connection of the most recently accepted subscription on this stream allowed
    /// it to hold unwritten — jsonrpsee's `message_buffer_capacity`, asked of the sink.
    ///
    /// **A bound this daemon *sets* and nothing *reads* is a bound that silently reverts to
    /// somebody else's default** (rubric A8), which is the objection note **N38** made when
    /// P4b turned the WebSocket surface off rather than inherit two numbers. `daemon::uds`
    /// sets [`limits::WS_MESSAGE_BUFFER_CAPACITY`] on every connection and jsonrpsee's
    /// `ServerConfig` gives no way to read it back, so the one place the configured number
    /// is observable is the sink a real connection hands a subscription — and this is where
    /// it is published. Zero until a subscription has been accepted, which is a fact about
    /// this stream rather than a claim about the configuration.
    buffer: AtomicUsize,
    /// Events a subscription could not hand to its connection because the connection's
    /// buffer was full ([`limits::WS_MESSAGE_BUFFER_CAPACITY`]).
    dropped: AtomicU64,
    /// Events a subscriber never saw because it fell more than
    /// [`limits::SUBSCRIPTION_BROADCAST_DEPTH`] behind the fan-out.
    missed: AtomicU64,
    /// The daemon's live-subscription count, **shared with every other stream**.
    ///
    /// One number rather than one per stream, because "is this subscription reaped" is a
    /// question about the daemon: `ServerHandle::stopped()` resolving turns on every
    /// subscription task having ended, not on the hotplug ones having. Per-stream counts
    /// are still available and are `broadcast::Sender::receiver_count()`, which is exact and
    /// needs no second bookkeeping.
    ///
    /// A `watch` and not an atomic because `receiver_count()` can be *read* but not *waited
    /// for*. "The subscription was reaped" is an event, and a test that polled a counter for
    /// it would be a test with a sleep in it under another name.
    live: watch::Sender<usize>,
    /// The daemon's total loss count, shared with every other stream — [`Events::lost`].
    ///
    /// Every one of the three counters above bumps it as well, which is what makes "this
    /// daemon has finished losing" a thing to await rather than to guess at.
    lost: watch::Sender<u64>,
}

impl<T: Clone + Send + 'static> Fanout<T> {
    /// An empty stream, bounded at [`limits::SUBSCRIPTION_BROADCAST_DEPTH`], counting into
    /// `live` and `lost`.
    fn new(live: watch::Sender<usize>, lost: watch::Sender<u64>) -> Fanout<T> {
        let (events, _) = broadcast::channel(limits::SUBSCRIPTION_BROADCAST_DEPTH);
        Fanout {
            events,
            unheard: AtomicU64::new(0),
            buffer: AtomicUsize::new(0),
            dropped: AtomicU64::new(0),
            missed: AtomicU64::new(0),
            live,
            lost,
        }
    }

    /// Record `count` events this stream will never deliver, in both places at once.
    ///
    /// The one function every loss goes through, so that the itemised counter and the
    /// waitable total cannot be bumped separately — which is the only way the two could
    /// come to disagree.
    fn lose(&self, counter: &AtomicU64, count: u64) {
        counter.fetch_add(count, Ordering::Relaxed);
        self.lost
            .send_modify(|lost| *lost = lost.saturating_add(count));
    }

    /// Hand `event` to every subscriber. Never fails, never waits.
    ///
    /// The two properties are one sentence: `broadcast::Sender::send` is synchronous, does
    /// not need a runtime, and drops the oldest event for a receiver that is behind rather
    /// than making the sender wait. A producer here is a camera actor's own thread or the
    /// hotplug watch's, and either one waiting on a subscriber is the wedge this
    /// sub-milestone exists to make unrepresentable.
    fn emit(&self, event: T) {
        if self.events.send(Feed::Event(event)).is_err() {
            self.lose(&self.unheard, 1);
        }
    }

    /// End every stream reading this fan-out, naming why.
    ///
    /// The half `tokio::sync::broadcast` does not have: a `Sender` cannot close a channel it
    /// does not drop, and this one is owned by [`Events`] for the daemon's whole life. See
    /// [`Feed`] for why the terminal is a value rather than a signal beside the queue.
    ///
    /// A terminal nobody was there to hear is **not** counted: [`Fanout::unheard`] counts
    /// *events* a subscriber would have wanted, and "the source you were not reading stopped"
    /// is not one.
    fn end(&self, reason: &'static str) {
        let _ = self.events.send(Feed::Ended(reason));
    }

    /// Record the message buffer the connection of a newly accepted subscription gave it.
    ///
    /// See [`Fanout::buffer`]: this is the only place the number `daemon::uds::serve`
    /// configured is observable from inside the daemon.
    fn accepted_with(&self, buffer: usize) {
        self.buffer.store(buffer, Ordering::Relaxed);
    }

    /// Take a receiver, and count it as live until it is dropped.
    ///
    /// Taken **before** the subscription is accepted, always: `broadcast` delivers only
    /// what was sent after a receiver existed, so attaching after the accept round trip
    /// would drop every event that arrived during it.
    fn attach(&self) -> Attached<T> {
        let events = self.events.subscribe();
        self.live.send_modify(|live| *live += 1);
        Attached {
            events,
            counted: Counted(self.live.clone()),
        }
    }

    /// This stream's counters, as one value.
    fn activity(&self) -> StreamActivity {
        StreamActivity {
            subscribers: self.events.receiver_count(),
            unheard: self.unheard.load(Ordering::Relaxed),
            buffer: self.buffer.load(Ordering::Relaxed),
            dropped: self.dropped.load(Ordering::Relaxed),
            missed: self.missed.load(Ordering::Relaxed),
        }
    }
}

/// What one event stream is doing, and what it has cost.
///
/// Every field is a number an operator would want and an integration test asserts, which is
/// deliberately the same list: a counter that exists only for a test is a counter nobody
/// maintains, and `engine::progress::ChannelSink::dropped` set the precedent one crate down.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamActivity {
    /// How many subscriptions are reading this stream right now.
    pub subscribers: usize,
    /// Events emitted while nobody was subscribed at all — dropped by decision, counted so
    /// the decision is visible (see this module's header).
    pub unheard: u64,
    /// What the most recently accepted subscription's connection let it hold unwritten.
    ///
    /// Zero until one has been accepted. See `Fanout::buffer` for why a number the daemon
    /// *configures* is published as a number the daemon *observed*.
    pub buffer: usize,
    /// Events one subscriber lost because its connection's buffer was full.
    pub dropped: u64,
    /// Events one subscriber lost by falling behind the fan-out itself.
    pub missed: u64,
}

impl StreamActivity {
    /// Every event this stream failed to deliver, whichever bound stopped it.
    ///
    /// The number the claim "nothing is lost silently" is about: a caller comparing what a
    /// producer emitted against what a subscriber received wants the whole shortfall, and
    /// three fields is three chances to leave one out.
    #[must_use]
    pub const fn lost(&self) -> u64 {
        self.unheard
            .saturating_add(self.dropped)
            .saturating_add(self.missed)
    }
}

/// What every subscription on this daemon is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SubscriptionActivity {
    /// How many subscriptions are open, across every stream.
    pub live: usize,
    /// `subscribe_events`.
    pub hotplug: StreamActivity,
    /// `subscribe_calibration`.
    pub calibration: StreamActivity,
}

/// One subscription's receiver, and the live count it is part of.
///
/// A guard rather than a bare `broadcast::Receiver`, so that "the subscription was reaped"
/// is a property of the type: whatever ends the subscription task — a disconnect, an
/// unsubscribe, a lag close, the server stopping — the count comes back down on the way
/// out, because dropping the receiver is the one thing all of those have in common. A
/// decrement written at the end of the forwarding loop instead would be a decrement four
/// `return`s can skip.
#[derive(Debug)]
pub struct Attached<T> {
    events: broadcast::Receiver<Feed<T>>,
    /// Declared **after** `events`, and that ordering is the whole of what a waiter on the
    /// live count is promised.
    ///
    /// Rust drops a struct's own `Drop::drop` before any of its fields, so a decrement
    /// written there would publish "nobody is subscribed" while this receiver was still
    /// alive — and a test woken by that publication would then read a
    /// `broadcast::Sender::receiver_count()` of one and fail for a reason on the wrong side
    /// of the assertion. Fields are dropped in declaration order instead, so the receiver is
    /// gone *before* the number says so.
    #[expect(
        dead_code,
        reason = "held for its Drop and for its position after `events`; a field whose \
                  whole job is when it is dropped is never read, and reading it would be \
                  the thing that made this ordering incidental"
    )]
    counted: Counted,
}

/// The live count's decrement, as a value with a drop rather than a line in a loop.
///
/// [`Attached`]'s field, for the reason its doc gives; a struct of its own because that is
/// the only way to put a `Drop` *after* another field's.
#[derive(Debug)]
struct Counted(watch::Sender<usize>);

impl Drop for Counted {
    fn drop(&mut self) {
        self.0.send_modify(|live| *live = live.saturating_sub(1));
    }
}

/// What a subscriber that fell behind the fan-out is told. See this module's header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OnLag {
    /// End the stream, naming the count. For a vocabulary of deltas.
    EndTheStream,
    /// Count it and carry on. For a vocabulary where every event repaints the whole state.
    KeepGoing,
}

/// What a stream does about `missed` events it will never deliver.
///
/// A named fold rather than a `match` inside the forwarding loop, and the reason is that it
/// is the one decision in this module a test cannot otherwise reach: forcing a real
/// `broadcast` lag means keeping a subscription task from running while
/// [`limits::SUBSCRIPTION_BROADCAST_DEPTH`] events go past it, which is a fact about the
/// scheduler wearing a fact about the queue. Here both arms and the payload are exact, and
/// `tests/subscriptions.rs` asserts the conservation law the two of them are part of —
/// every event a subscriber did not receive is counted somewhere.
fn lag_verdict(on_lag: OnLag, missed: u64) -> std::result::Result<(), SubscriptionError> {
    match on_lag {
        OnLag::KeepGoing => Ok(()),
        // Named rather than merely closed: a client that is told *how many* it missed can
        // decide whether to re-enumerate, and one that is told nothing has to assume the
        // worst every time.
        OnLag::EndTheStream => Err(SubscriptionError::from_json(to_json_raw_value(&Lagged {
            lagged: missed,
        })?)),
    }
}

/// The close a subscriber gets when the *source* stopped, whatever it was watching.
///
/// [`lag_verdict`]'s sibling with no policy in it, because there is no policy to have: a
/// stream whose producer is gone has nothing left to deliver on either vocabulary, and a
/// subscription that stayed open would be exactly the quiet lie ending a lagging hotplug
/// stream exists to refuse — one layer up, and worse, because no later event can repaint it.
fn ended(reason: &'static str) -> SubscriptionError {
    match to_json_raw_value(&Ended { ended: reason }) {
        Ok(payload) => SubscriptionError::from_json(payload),
        // A `&'static str` in a one-field struct does not fail to serialize; if it somehow
        // did, the client still has to be told the stream is over, and jsonrpsee's blanket
        // `From<T: ToString>` is the carrier that cannot fail.
        Err(err) => SubscriptionError::from(err),
    }
}

/// The hotplug half: a watch thread that exists exactly while somebody is subscribed.
///
/// **Lazily started, and that is a decision rather than an optimisation.**
/// `CameraBackend::watch` can fail — a container without `NETLINK_KOBJECT_UEVENT`, an LSM,
/// a backend that has no watch to give — and a daemon that refused to *start* over it would
/// answer nothing on a host where enumeration works perfectly, which is the
/// availability-versus-capability conversion E3 forbids at the composition root. Started
/// here, the failure is a refusal of the subscription that asked for it, which is what a
/// D13 refusal on this surface means everywhere else.
///
/// **Ended when the last subscriber goes**, which the shutdown discipline P4e-ii owns does
/// not have to be asked for: a thread parked in `poll(2)` for nobody is a thread that has
/// to be told to stop, and one that stops when its last reader leaves never has to be. That
/// property is published rather than asserted in prose — [`Hotplug::running`] — because a
/// claim about a thread nothing can observe is a claim no test can make (rubric A8).
///
/// **P4e-ii lands, and that argument is still the whole of it: there is no shutdown token
/// here, deliberately.** The stop reaches the *subscriptions* ([`forward`]'s new arm), each of
/// which returns and drops its [`Attached`] on the way out; dropping it drops the
/// `broadcast::Receiver` before the count it holds ([`Counted`], and that ordering is stated
/// where it is enforced). So by the time the last cancelled subscription has ended,
/// `receiver_count()` is zero, and [`Hotplug::give_up`] answers `true` on the watch thread's
/// next turn — which is at most one [`limits::HOTPLUG_WATCH_DEADLINE_MS`] away, because that
/// is what the thread's `next_event` deadline buys. Giving this thread a token of its own
/// would add a second way for it to end, and a second way to end is a second interleaving to
/// argue about in the one place note **N59** records getting an interleaving wrong.
///
/// What that costs, stated rather than discovered: a stop may outlive the daemon's own
/// teardown by up to that deadline. It is bounded by a constant, it holds no lock and no
/// camera, and the process exiting is what ends it — the same sentence this workspace already
/// writes about the actors' device threads (`crate::shutdown`'s residual).
#[derive(Debug)]
pub(crate) struct Hotplug {
    events: Fanout<HotplugEvent>,
    /// Whether a watch thread is running, as one fact in one place.
    ///
    /// A `watch::Sender` *inside* a `std::sync::Mutex` rather than a `bool` beside one,
    /// because the flag has to do two things and a second copy of it would be a second
    /// answer to "is anything watching": it is **decided under exclusion** (starting a watch
    /// binds a netlink socket and spawns a thread, and two subscribers arriving together
    /// must not start two), and it is **awaited** (the property this type's header states —
    /// the watch exists exactly while somebody is listening — is only a claim if something
    /// outside can be told when it changes).
    ///
    /// The mutex is `std` and never held across an `await`: the daemon's one rule about
    /// blocking is that nothing which can park a thread runs on a runtime worker. Every use
    /// of this lock is inside one blocking-pool closure or inside the watch thread itself.
    running: std::sync::Mutex<watch::Sender<bool>>,
}

impl Hotplug {
    fn new(live: watch::Sender<usize>, lost: watch::Sender<u64>) -> Hotplug {
        Hotplug {
            events: Fanout::new(live, lost),
            running: std::sync::Mutex::new(watch::Sender::new(false)),
        }
    }

    /// Whether a watch thread is running, as something to **await** rather than poll.
    fn watch_running(&self) -> watch::Receiver<bool> {
        lock(&self.running).subscribe()
    }

    /// Attach a subscriber, starting the watch if this is the first one.
    ///
    /// **Blocking**, and the caller runs it on the blocking pool: `CameraBackend::watch`
    /// binds a netlink socket and reads `/sys/class/video4linux` (note N53), and spawning a
    /// thread is a syscall.
    ///
    /// **Everything happens under the one lock, and the receiver is taken last.** That
    /// ordering is what makes the start/stop race benign in all three directions, and the
    /// third is the one a first draft of this file got wrong (note **N59**):
    ///
    /// - a thread about to give up takes this lock and asks whether anybody is still
    ///   listening, so a subscriber that has already attached keeps it alive;
    /// - a subscriber that arrives after a thread has given up finds `running` false and
    ///   starts a fresh one;
    /// - a subscriber that arrives while a *failing* thread is ending its subscribers'
    ///   streams ([`Hotplug::give_up`]) attaches **after** the terminal was sent, so it is
    ///   not handed somebody else's ending — which is only true because the terminal travels
    ///   in the channel ([`Feed`]) and the receiver is taken under this lock.
    ///
    /// There is no interleaving in which a subscriber is attached to a stream nothing is
    /// feeding, and none in which one is ended before it began.
    ///
    /// # Errors
    ///
    /// Whatever the backend refuses `watch()` with, or [`Error::DeviceIo`] when the thread
    /// cannot be spawned — the same variant `engine::actor` uses for that, because it is
    /// this process failing to perform an operation rather than a device declining one.
    fn attach(self: &Arc<Hotplug>, cameras: &Cameras) -> Result<Attached<HotplugEvent>> {
        let running = lock(&self.running);
        if !*running.borrow() {
            let watch = cameras.watch()?;
            let source = Arc::clone(self);
            std::thread::Builder::new()
                .name("wchd-hotplug".to_owned())
                .spawn(move || source.watching(watch))
                .map_err(|err| Error::DeviceIo {
                    operation: "start the hotplug watch thread".to_owned(),
                    errno: err.raw_os_error(),
                    message: err.to_string(),
                })?;
            running.send_replace(true);
        }
        let attached = self.events.attach();
        drop(running);
        Ok(attached)
    }

    /// The watch thread's whole body: read, publish, and stop when nobody is left.
    ///
    /// It reads a clock, and that is not a contradiction of the doctrine every deadline in
    /// this workspace follows (`engine::settle`: the caller stamps it). **This thread *is*
    /// the caller.** `HotplugWatch::next_event` takes an `Instant` because the watch itself
    /// must not read one, and somebody has to; here it is the one loop that owns a watch, on
    /// its own thread, computing its own budget.
    fn watching(self: Arc<Hotplug>, mut watch: Box<dyn schema::backend::HotplugWatch>) {
        loop {
            let deadline =
                Instant::now() + Duration::from_millis(limits::HOTPLUG_WATCH_DEADLINE_MS);
            match watch.next_event(deadline) {
                // The deadline arrived first, which is an answer and not an error (E3).
                Ok(None) => {}
                Ok(Some(event)) => self.events.emit(event),
                Err(error) => {
                    // The socket is gone or the tree cannot be read. Worth an operator's
                    // attention — a path and an errno, never a frame (see `crate::logging`)
                    // — and worth ending the thread over: `next_event` that failed once
                    // fails immediately, so continuing would be the spin
                    // `MAX_CONSECUTIVE_ACCEPT_FAILURES` exists to prevent one layer up.
                    tracing::warn!(
                        %error,
                        "the hotplug watch failed; its subscribers' streams are ending"
                    );
                    self.give_up(Some(WATCH_STOPPED));
                    return;
                }
            }

            if self.give_up(None) {
                return;
            }
        }
    }

    /// Put the watch down, and say whether this thread is finished.
    ///
    /// **The one exit**, and it is one function because the two ways out of
    /// [`Hotplug::watching`] have to take the same lock and answer the same question in the
    /// same order — a first draft had the failure path clear the flag *without* consulting
    /// the readers, which let a subscriber attach in the window and left it reading a stream
    /// with no thread behind it, falsifying [`Hotplug::attach`]'s own argument (note
    /// **N59**).
    ///
    /// `ended` is `Some` when the *source* failed, and then every reader's stream ends with
    /// it: the alternative is a subscription that stays open, stays counted and silently
    /// delivers nothing, which is the state rubric A4 calls one no verb can leave. When it
    /// is `None` the thread is only leaving because nobody is left to read, and there is
    /// nothing to tell — the readers are the ones who went.
    ///
    /// `receiver_count` and not the shared live count: what decides whether this thread has
    /// a reader is *this* stream's readers, and a calibration subscriber is not one.
    fn give_up(&self, ended: Option<&'static str>) -> bool {
        let running = lock(&self.running);
        match ended {
            Some(reason) => {
                self.events.end(reason);
                running.send_replace(false);
                true
            }
            None if self.events.events.receiver_count() == 0 => {
                running.send_replace(false);
                true
            }
            None => false,
        }
    }
}

/// A poisoned lock here means a thread panicked holding it, which is the failure already on
/// its way to the operator rather than a reason to replace it with a second one. The same
/// helper, for the same reason, as `engine::actor`'s.
fn lock<T>(mutex: &std::sync::Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The daemon's [`engine::progress::ProgressSink`]: every sweep's events, fanned out.
///
/// **One per daemon, not one per session**, which is what `schema::progress`'s own header
/// settles: "the session id is on every event rather than established once at subscription
/// time, because P4e's subscription is per *client* and a client may watch a daemon running
/// more than one session".
///
/// It replaces the `engine::progress::Silent` the sweep emitted into at P4c, at the seam that
/// file said was "already the shape P4e needs". `engine::progress::ChannelSink` stays exactly
/// where it is — that one is `webcam-handler-cli`'s, feeding indicatif over a
/// `std::sync::mpsc` the engine can name without a runtime.
#[derive(Debug)]
pub(crate) struct ProgressBroadcast(Arc<Events>);

impl ProgressBroadcast {
    /// A sink over `events`' calibration stream.
    pub(crate) fn new(events: Arc<Events>) -> ProgressBroadcast {
        ProgressBroadcast(events)
    }
}

impl engine::progress::ProgressSink for ProgressBroadcast {
    fn emit(&self, event: &ProgressEvent) {
        // Called from a camera actor's own thread, in the middle of a sweep that is holding
        // the device. It cannot fail and it cannot wait — `engine::progress`'s header says
        // why in full ("a sink that could refuse would put 'the progress bar failed' on the
        // list of things that can end a calibration") — and `Fanout::emit` is both.
        self.0.calibration.emit(event.clone());
    }
}

// ----------------------------------------------------------------- the subscription body

/// One subscriber's whole life: accept, forward until something ends it, and let go.
///
/// **The one implementation both subscriptions use**, because everything about a
/// subscription except *what it carries* and *what a lag means* is the same thing said
/// twice — and a second copy is where the daemon would come to have two answers to "what
/// happens when a subscriber stops reading".
///
/// Six ways it ends, and each one releases the receiver on the way out, which is what
/// [`Attached`]'s `Drop` makes a property of the type rather than of this function:
///
/// - the client went away or unsubscribed (`sink.closed()`, or a `Closed` from `try_send`)
///   — the case docs/7 calls "the subscription is reaped", and the reason it is a `select!`
///   rather than a send that eventually notices: a stream with no events would otherwise
///   hold a task for a client that is already gone;
/// - the **daemon** is stopping ([`crate::shutdown::Shutdown`]), and the client is told
///   [`SHUTTING_DOWN`] — see below;
/// - the **source** stopped ([`Feed::Ended`]), which is a hotplug watch that failed — the
///   client is told, by name, so it can re-subscribe and re-enumerate;
/// - the fan-out closed, which only happens when the daemon itself is going away;
/// - the subscriber lagged and its stream's policy is [`OnLag::EndTheStream`];
/// - the event could not be serialized, which is this process failing rather than the
///   client, and is the one arm that has never fired.
///
/// **Why the stop is an arm here rather than a socket that closes.** `crate::shutdown` cancels
/// this token *before* it stops the transport, and the whole reason for that order is this
/// `select!`: a subscription reached by the token ends with a payload naming
/// [`SHUTTING_DOWN`], and one whose connection is simply torn out from under it ends with
/// nothing a client can branch on. AGENTS says open streams are "cancelled, never awaited, on
/// shutdown" — this is the cancelled half, and the reason it is not the same thing as waiting
/// for a client to notice its socket went away.
///
/// **`try_send` and never `send`.** `SubscriptionSink::send` waits for room, which would
/// park this task on a client that stopped reading — and with it, nothing else, because the
/// fan-out is in front of it. But "nothing a client does can wedge the daemon" is a claim
/// about a *bound*, and a task parked indefinitely on a socket buffer is an unbounded wait
/// wearing a task. `engine::progress::ChannelSink` makes the same choice one crate down and
/// states the reason there.
async fn forward<T>(
    pending: PendingSubscriptionSink,
    mut attached: Attached<T>,
    counters: &Fanout<T>,
    shutdown: &crate::shutdown::Shutdown,
    on_lag: OnLag,
) -> SubscriptionResult
where
    T: Clone + Send + Serialize + 'static,
{
    let mut sink = pending.accept().await?;
    // The one place the buffer this connection was configured with is observable — see
    // [`Fanout::buffer`]. Asked of the sink rather than read from `schema::limits`, because
    // the question is what the *connection* gave this subscription and the constant is only
    // what `daemon::uds::serve` asked for.
    counters.accepted_with(sink.max_capacity());

    loop {
        // The borrow of `sink` ends with this block, which is what lets the arm below take
        // it mutably: `closed()` reads the sink and `try_send` writes it, and they are one
        // loop apart rather than one `select!` apart.
        let received = {
            let closed = std::pin::pin!(sink.closed());
            // Both waits are cancel-safe and both are rebuilt every turn: `closed` reads the
            // sink, and `Shutdown::cancelled` is a token that stays cancelled, so a stop that
            // arrived while this task was between polls is still here when the arm is
            // rebuilt. There is no edge to miss.
            let stopping = std::pin::pin!(shutdown.cancelled());
            tokio::select! {
                () = closed => return Ok(()),
                () = stopping => return Err(ended(SHUTTING_DOWN)),
                received = attached.events.recv() => received,
            }
        };

        let event = match received {
            Ok(Feed::Event(event)) => event,
            // The producer is gone. Ending with a reason is the only answer that is not the
            // quiet lie this module refuses one policy up: a stream that stayed open would
            // stay counted, deliver nothing for ever, and give the client nothing to act on.
            Ok(Feed::Ended(reason)) => return Err(ended(reason)),
            Err(broadcast::error::RecvError::Lagged(missed)) => {
                counters.lose(&counters.missed, missed);
                lag_verdict(on_lag, missed)?;
                continue;
            }
            // The daemon is going away. Nothing to tell the client that the connection
            // closing will not tell it.
            Err(broadcast::error::RecvError::Closed) => return Ok(()),
        };

        match sink.try_send(to_json_raw_value(&event)?) {
            Ok(()) => {}
            // This connection's buffer is full: the newest event is dropped and counted,
            // and the stream continues. See this module's header for why that is the
            // answer at every hop here.
            Err(TrySendError::Full(_)) => {
                counters.lose(&counters.dropped, 1);
            }
            Err(TrySendError::Closed(_)) => return Ok(()),
        }
    }
}

/// `subscribe_events`' body: hotplug, ending the stream on a gap.
///
/// # Errors
///
/// Never after the accept, except the lag close this module's header argues for. Before it,
/// whatever `CameraBackend::watch` refused — rejected as the D13 error every refusal on
/// this surface is, because a `PendingSubscriptionSink` can still carry a JSON-RPC code.
pub(crate) async fn subscribe_hotplug(
    events: Arc<Events>,
    pending: PendingSubscriptionSink,
    attached: Attached<HotplugEvent>,
) -> SubscriptionResult {
    forward(
        pending,
        attached,
        &events.hotplug.events,
        events.shutdown(),
        OnLag::EndTheStream,
    )
    .await
}

/// `subscribe_calibration`'s body: sweep progress, surviving a gap.
///
/// # Errors
///
/// Never. A sweep that fails reports it *as an event* (`SweepInterrupted`), because the
/// sweep's own caller is the one being refused.
pub(crate) async fn subscribe_calibration(
    events: Arc<Events>,
    pending: PendingSubscriptionSink,
) -> SubscriptionResult {
    let attached = events.calibration.attach();
    forward(
        pending,
        attached,
        &events.calibration,
        events.shutdown(),
        OnLag::KeepGoing,
    )
    .await
}

/// Attach a hotplug subscriber, starting the watch if nothing is watching yet.
///
/// **Blocking** — see [`Hotplug::attach`]. Split out from [`subscribe_hotplug`] because the
/// two halves belong on two different threads: this one binds a socket and spawns, and the
/// other one is a task.
///
/// # Errors
///
/// Whatever the backend refuses `watch()` with, or [`Error::DeviceIo`] when the watch
/// thread cannot be started.
pub(crate) fn attach_hotplug(events: &Events, cameras: &Cameras) -> Result<Attached<HotplugEvent>> {
    events.hotplug.attach(cameras)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_lagging_subscriber_ends_one_stream_and_is_only_counted_on_the_other() {
        // The two policies this module's header argues for, and the payload one of them
        // carries. Both directions in one test because the *difference* is the claim: a
        // build that ended both streams would make a briefly-slow progress bar cost a
        // client its view of a twenty-minute sweep, and one that ended neither would hand a
        // hotplug consumer a node tree it believes and cannot check.
        assert!(
            lag_verdict(OnLag::KeepGoing, 7).is_ok(),
            "a progress subscriber was ended for falling behind"
        );

        let ended = lag_verdict(OnLag::EndTheStream, 7)
            .expect_err("a hotplug subscriber was left believing a tree it had missed");
        // The count reaches the client as a *number*, not as prose: jsonrpsee's blanket
        // `From<T: ToString>` would have flattened it, and a consumer that has to parse a
        // sentence to learn how much it missed is a consumer that will not.
        assert_eq!(
            serde_json::to_value(&ended).expect("a subscription error serializes"),
            serde_json::json!({ "lagged": 7 }),
            "the lag close did not name the count"
        );
    }

    #[test]
    fn a_fresh_fan_out_has_nobody_listening_and_says_so_by_counting() {
        // The decision this module's header records, at the smallest scale that can hold
        // it: an event emitted with nobody subscribed is dropped — nothing is buffered for
        // a client that has not arrived — and the drop is a number rather than a silence
        // (rubric rule 3). Both directions, because a `Fanout` that counted every event as
        // unheard would pass the first half alone.
        let live = watch::Sender::new(0);
        let lost = watch::Sender::new(0);
        let fanout: Fanout<u32> = Fanout::new(live, lost);
        fanout.emit(1);
        fanout.emit(2);
        assert_eq!(fanout.activity().unheard, 2);
        assert_eq!(fanout.activity().subscribers, 0);
        // The itemised count and the waitable total are bumped by one function, so they
        // cannot disagree — which is what lets a test wait on the second and then assert
        // the first (see [`Events::lost`]).
        assert_eq!(*fanout.lost.borrow(), fanout.activity().lost());

        let mut attached = fanout.attach();
        assert_eq!(fanout.activity().subscribers, 1);
        fanout.emit(3);
        assert_eq!(
            fanout.activity().unheard,
            2,
            "a delivered event was counted"
        );
        assert_eq!(*fanout.lost.borrow(), 2, "a delivered event was counted");
        assert!(
            matches!(
                attached.events.try_recv().expect("one event"),
                Feed::Event(3)
            ),
            "the fan-out delivered something other than the event it was given"
        );

        // A source that stops ends the stream *after* what it already sent, which is the
        // ordering [`Feed`] exists for: the terminal is queued behind the event above rather
        // than racing it, so a subscriber that is behind still gets what it was owed.
        fanout.emit(4);
        fanout.end(WATCH_STOPPED);
        assert!(
            matches!(
                attached.events.try_recv().expect("the queued event"),
                Feed::Event(4)
            ),
            "the terminal overtook an event that had already been sent"
        );
        assert!(
            matches!(
                attached.events.try_recv().expect("the terminal"),
                Feed::Ended(WATCH_STOPPED)
            ),
            "the end of the source did not reach the subscriber"
        );
        // …and it is not a *loss*: `unheard` counts events a subscriber would have wanted,
        // and "the source you were reading stopped" is the answer rather than a shortfall.
        assert_eq!(
            fanout.activity().unheard,
            2,
            "the terminal was counted as a lost event"
        );

        // And both come back down when the receiver goes, whatever ended it — which is the
        // property `tests/subscriptions.rs` waits on to say "reaped".
        //
        // **What no test here can claim** is the *order* of those two: that the receiver is
        // gone before the count says so is a fact about Rust's drop order (fields after
        // `Drop::drop`, in declaration order — see [`Attached::counted`]), and observing it
        // would need a thread scheduled between two adjacent statements. It is stated where
        // it is enforced instead, and what depends on it is one assertion in the daemon's
        // subscription suite: a waiter woken by `live == 0` reads `subscribers` immediately
        // and expects zero.
        drop(attached);
        assert_eq!(fanout.activity().subscribers, 0);
        assert_eq!(*fanout.live.borrow(), 0);
    }
}
