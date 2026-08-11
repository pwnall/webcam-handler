//! The progress seam: where a running sweep says what it is doing (design §2.9, D8).
//!
//! The *events* are `schema::progress` — DTOs, because docs/7's risk register says a hook
//! that is not schema-shaped from the start is a hook P4 re-plumbs. This module is the
//! other half: the seam that carries them, with a real implementation and a double, the
//! same shape [`crate::settle::Clock`] and [`schema::paths::Env`] already have.
//!
//! ## Emitting cannot fail, and that is a decision
//!
//! [`ProgressSink::emit`] returns nothing. Every other seam in this engine has a fault
//! menu, and this one deliberately does not, because there is no failure here a caller
//! could act on: a subscriber that hung up, a queue that filled, a terminal that closed —
//! none of them are reasons to abandon a sweep that is holding a camera and has a
//! pre-sweep snapshot on disk. A sink that could refuse would put "the progress bar
//! failed" on the list of things that can end a calibration, and that is the wrong list.
//!
//! What it must not do instead is lose events *quietly*: [`ChannelSink`] counts what it
//! drops and says so on demand, so "the consumer fell behind" is a number rather than a
//! silence (rubric rule 3).
//!
//! ## Why a trait and not a closure
//!
//! A `&dyn Fn(&Event)` would work today and be untransportable tomorrow: P4e's
//! subscription needs an object it can hold, name, and drop when the client disconnects,
//! and a closure over a CLI's progress bar is none of those. The trait is object-safe and
//! takes `&self`, so one sink can be shared by the actor thread that emits and the
//! transport that forwards.

use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};

use schema::limits;
use schema::progress::ProgressEvent;

/// Somewhere for a sweep to report what it is doing.
pub trait ProgressSink: std::fmt::Debug + Send + Sync {
    /// Take one event. Never fails, never blocks — see this module's header.
    fn emit(&self, event: &ProgressEvent);
}

/// Nobody is listening.
///
/// The default for every caller that does not want progress, and a real implementation
/// rather than an `Option<&dyn ProgressSink>`: a sweep that had to ask whether anyone was
/// listening before it reported anything would grow that question at five call sites.
#[derive(Debug, Clone, Copy, Default)]
pub struct Silent;

impl ProgressSink for Silent {
    fn emit(&self, _event: &ProgressEvent) {}
}

/// The real one: hands each event to a bounded channel.
///
/// Bounded by [`schema::limits::PROGRESS_QUEUE_DEPTH`] and **lossy at the bound**. The
/// three available behaviours for a full queue are block, grow, and drop; the first stops
/// the camera for a consumer's benefit and the second lets a consumer that stopped reading
/// decide how much memory a sweep uses. Dropping is the only one whose worst case is a
/// repaint — as long as the drop is counted, which is what [`ChannelSink::dropped`] is
/// for. What gets dropped is the *newest* event, because that is the only end of an
/// `mpsc` queue a sender can reach: a slow consumer sees an older prefix plus a count of
/// what it missed, rather than a gap it cannot detect.
///
/// `std::sync::mpsc` and not a runtime's channel: the engine names no async runtime (note
/// N5's wall), and P4e's daemon bridges this receiver onto whatever its subscription
/// speaks. That bridge is transport code, which is where transport belongs.
#[derive(Debug)]
pub struct ChannelSink {
    tx: SyncSender<ProgressEvent>,
    dropped: AtomicUsize,
}

impl ChannelSink {
    /// A sink and the receiver its events arrive on.
    ///
    /// Depth is [`schema::limits::PROGRESS_QUEUE_DEPTH`], not a parameter: "how much
    /// progress may pile up" is one of the bounds design §2.10 keeps in one table, and a
    /// caller-supplied depth is how a table stops being the answer.
    #[must_use]
    pub fn new() -> (ChannelSink, Receiver<ProgressEvent>) {
        let (tx, rx) = sync_channel(limits::PROGRESS_QUEUE_DEPTH);
        (
            ChannelSink {
                tx,
                dropped: AtomicUsize::new(0),
            },
            rx,
        )
    }

    /// How many events this sink could not deliver.
    ///
    /// Both causes count here, and they are different facts a caller may want to tell
    /// apart later: a full queue means the consumer is slow, and a closed one means it is
    /// gone. Neither is an error for the sweep, and a caller rendering "N events dropped"
    /// is telling the truth in both cases.
    #[must_use]
    pub fn dropped(&self) -> usize {
        self.dropped.load(Ordering::Relaxed)
    }
}

impl ProgressSink for ChannelSink {
    fn emit(&self, event: &ProgressEvent) {
        // `try_send`, so a consumer that stopped reading cannot stop the sweep. The clone
        // is on this side of the bound rather than before it, so the drop path costs one
        // allocation and the delivery path costs the one it was always going to cost.
        match self.tx.try_send(event.clone()) {
            Ok(()) => {}
            Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) => {
                self.dropped.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}

/// The double: keeps every event, in order, for a test to walk.
///
/// Ships with the library rather than living behind `#[cfg(test)]`, for the reason
/// [`crate::store::TempStore`] does: the callers that need to assert a sweep's event
/// sequence are in other crates, and a double they cannot reach is a double that gets
/// re-implemented once per crate.
#[derive(Debug, Default)]
pub struct Recorder {
    events: Mutex<Vec<ProgressEvent>>,
}

impl Recorder {
    /// An empty recorder.
    #[must_use]
    pub fn new() -> Recorder {
        Recorder::default()
    }

    /// Everything emitted so far, in order.
    #[must_use]
    pub fn events(&self) -> Vec<ProgressEvent> {
        self.locked().clone()
    }

    /// The event names, in order — what a sequence assertion actually reads.
    #[must_use]
    pub fn sequence(&self) -> Vec<&'static str> {
        self.locked()
            .iter()
            .map(|event| event.progress.name())
            .collect()
    }

    /// How many events have arrived.
    #[must_use]
    pub fn len(&self) -> usize {
        self.locked().len()
    }

    /// Whether nothing has been emitted.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.locked().is_empty()
    }

    /// A poisoned lock is not a fault this double has an opinion about: the panic that
    /// poisoned it is the failure, and it is already on its way to the test runner.
    fn locked(&self) -> std::sync::MutexGuard<'_, Vec<ProgressEvent>> {
        self.events
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl ProgressSink for Recorder {
    fn emit(&self, event: &ProgressEvent) {
        self.locked().push(event.clone());
    }
}

#[cfg(test)]
mod tests {
    use schema::control::ControlSlug;
    use schema::progress::CalibrationProgress;
    use schema::session::SweepSpec;
    use schema::time::Stamp;
    use uuid::Uuid;

    use super::*;

    fn event(index: u32) -> ProgressEvent {
        ProgressEvent {
            session: Uuid::nil(),
            at: Stamp::epoch(),
            progress: CalibrationProgress::SweepStarted {
                control: ControlSlug::parse("focus_absolute").expect("literal slug"),
                plan: SweepSpec::All,
                total: index,
            },
        }
    }

    #[test]
    fn the_recorder_keeps_every_event_in_the_order_it_arrived() {
        let recorder = Recorder::new();
        assert!(recorder.is_empty());
        for index in 0..3 {
            recorder.emit(&event(index));
        }
        assert_eq!(recorder.len(), 3);
        assert_eq!(
            recorder.sequence(),
            vec!["sweep_started", "sweep_started", "sweep_started"]
        );
        // Order, not just membership: a sink that reversed or reordered would pass a
        // set-shaped assertion and fail every sequence a consumer depends on.
        let totals: Vec<u32> = recorder
            .events()
            .iter()
            .map(|event| match event.progress {
                CalibrationProgress::SweepStarted { total, .. } => total,
                _ => u32::MAX,
            })
            .collect();
        assert_eq!(totals, vec![0, 1, 2]);
    }

    #[test]
    fn a_channel_sink_delivers_what_a_live_consumer_reads_and_drops_nothing() {
        let (sink, rx) = ChannelSink::new();
        sink.emit(&event(7));
        assert_eq!(
            sink.dropped(),
            0,
            "a delivered event was counted as dropped"
        );
        assert_eq!(rx.recv().expect("one event"), event(7));
    }

    #[test]
    fn a_consumer_that_stopped_reading_costs_counted_drops_and_never_the_sweep() {
        // The bound doing its job, in both directions: everything up to
        // `PROGRESS_QUEUE_DEPTH` is queued for a consumer that has not got to it yet, and
        // everything past it is dropped *and counted*. A sink that blocked here would
        // hang this test; one that grew would report zero drops and quietly own the
        // memory.
        let (sink, rx) = ChannelSink::new();
        for index in 0..limits::PROGRESS_QUEUE_DEPTH {
            sink.emit(&event(u32::try_from(index).unwrap_or(u32::MAX)));
        }
        assert_eq!(sink.dropped(), 0, "the queue refused what it had room for");

        sink.emit(&event(u32::MAX));
        assert_eq!(sink.dropped(), 1, "the overflow was not counted");
        assert_eq!(
            rx.recv().expect("the queue is full, so it is not empty"),
            event(0),
            "the queue dropped what it had already accepted"
        );

        // And a consumer that goes away entirely: still counted, still no refusal.
        drop(rx);
        sink.emit(&event(1));
        assert_eq!(sink.dropped(), 2);
    }

    #[test]
    fn the_silent_sink_accepts_everything_and_keeps_nothing() {
        // It exists so a caller that does not want progress does not have to answer
        // "is anyone listening?" at every emit site. The only thing to assert is that it
        // is total.
        let silent = Silent;
        for index in 0..1_000 {
            silent.emit(&event(index));
        }
    }
}
