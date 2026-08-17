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
//! What it must not do instead is lose events *quietly*, and that obligation belongs to
//! whatever a composition root puts behind this trait: `daemon::events::Fanout` counts its
//! `unheard` and its `lagged`, and `webcam-handler-cli` renders on the sweep's own thread
//! and can lose nothing at all (rubric rule 3).
//!
//! ## Why a trait and not a closure
//!
//! A `&dyn Fn(&Event)` would work today and be untransportable tomorrow: P4e's
//! subscription needs an object it can hold, name, and drop when the client disconnects,
//! and a closure over a CLI's progress bar is none of those. The trait is object-safe and
//! takes `&self`, so one sink can be shared by the actor thread that emits and the
//! transport that forwards.
//!
//! ## What used to be here, and why it is not
//!
//! A `ChannelSink` — a `std::sync::mpsc::sync_channel` bounded by a
//! `limits::PROGRESS_QUEUE_DEPTH`, lossy at the bound and counting its drops — was built
//! at P4c for the daemon that had not arrived yet, on the reasoning that the engine names
//! no async runtime (note N5's wall) and something would have to bridge a `Receiver` onto
//! a subscription. **Both consumers arrived and neither took it** (docs/11's L16, note
//! **N230**): `webcam-handler-cli` implements the trait straight onto its own
//! `SweepWatcher`, because an in-process sweep runs on the calling thread and has no
//! boundary to cross, and `daemon::events::ProgressBroadcast` implements it onto a
//! `tokio::sync::broadcast` bounded by `limits::SUBSCRIPTION_BROADCAST_DEPTH`, because a
//! fan-out to N subscribers is not a queue of one. The seam was right and the *default*
//! implementation was a guess, and it sat here with a full test suite and two doc comments
//! naming readers that did not exist.
//!
//! The lesson is the one worth keeping if a third root ever needs a queue here: the bound
//! and the drop policy belong to the root that owns the boundary, because only it knows
//! how many consumers there are and what falling behind costs them.

use std::sync::Mutex;

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
                precision: 1,
                adjustments: Vec::new(),
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
