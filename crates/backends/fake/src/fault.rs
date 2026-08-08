//! The scriptable fault menu (design §2.9).
//!
//! Every seam in this design has a real implementation and a scriptable double, and the
//! double's fault menu is an **exhaustive-match-walked enum** — because a fault the
//! compiler cannot force the fake to script is a fault nobody tests. Adding a variant to
//! [`Fault`] adds it to `Fault::ALL`, and every walk over `ALL` (including this crate's
//! own menu test) grows with it.
//!
//! ## What each fault means, and how long it lasts
//!
//! One-shot faults are consumed by the first operation that looks for them; held faults
//! stay until released. The distinction is not decoration — "the device vanished" happens
//! once, and "the sensor never settles" is a condition.
//!
//! | Fault | Lasts | Observed by |
//! |---|---|---|
//! | [`Fault::DeviceGoneMidStream`] | one shot | `next_frame` → [`schema::Error::DeviceGone`], stream torn down |
//! | [`Fault::Busy`] | one shot | `open` → [`schema::Error::Busy`] |
//! | [`Fault::ClampOnWrite`] | one shot | `set` moves an in-range write to the maximum, reported as [`schema::control::WriteWarning::Adjusted`] \[PF:6\] |
//! | [`Fault::InactiveFlip`] | one shot | `controls` reports every measured manual partner's INACTIVE bit toggled \[PF:3\] |
//! | [`Fault::SettleNeverConverges`] | held | every frame's exposure differs, so no settle policy converges \[PF:11\] |
//! | [`Fault::FrameTimeout`] | one shot | `next_frame` → [`schema::Error::SettleTimeout`] |
//! | [`Fault::HotplugAdd`] | one shot | the watch yields [`schema::backend::HotplugEvent::Added`] |
//! | [`Fault::HotplugRemove`] | one shot | the watch yields [`schema::backend::HotplugEvent::Removed`] |

use std::collections::BTreeSet;
use std::fmt;

use crate::vocabulary::closed_vocabulary;

closed_vocabulary! {
    /// A fault the fake can be told to exhibit.
    ///
    /// The menu is design §2.9's row for the T1/T2 seam, variant for variant.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub enum Fault {
        /// The device disappears while a stream is running.
        DeviceGoneMidStream,
        /// Another process holds the device.
        Busy,
        /// The driver's real range is narrower than the one it declares, so a perfectly
        /// in-range write does not stick \[PF:6\].
        ///
        /// The warning is [`schema::control::WriteWarning::Adjusted`] rather than
        /// `Clamped`, and that is the point of the fault: the declared range does not
        /// explain the move, so nothing may claim it does.
        ClampOnWrite,
        /// INACTIVE flips without anybody writing an automation control \[PF:3\].
        InactiveFlip,
        /// Frames keep arriving and never stabilize \[PF:11\].
        SettleNeverConverges,
        /// A frame does not arrive before the deadline.
        FrameTimeout,
        /// A device node appears.
        HotplugAdd,
        /// A device node disappears.
        HotplugRemove,
    }
}

impl Fault {
    /// The fault's name, for reports and test failure messages.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Fault::DeviceGoneMidStream => "device_gone_mid_stream",
            Fault::Busy => "busy",
            Fault::ClampOnWrite => "clamp_on_write",
            Fault::InactiveFlip => "inactive_flip",
            Fault::SettleNeverConverges => "settle_never_converges",
            Fault::FrameTimeout => "frame_timeout",
            Fault::HotplugAdd => "hotplug_add",
            Fault::HotplugRemove => "hotplug_remove",
        }
    }
}

impl fmt::Display for Fault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What the fake has been told to do wrong.
///
/// Shared by the backend, every camera it opened, and the hotplug watch, because a fault
/// scripted before `open` must still fire after it.
#[derive(Debug, Default)]
pub(crate) struct FaultQueue {
    /// Faults that fire once, in the order they were queued.
    pending: Vec<Fault>,
    /// Faults that fire until released.
    held: BTreeSet<Fault>,
}

impl FaultQueue {
    /// Script `fault` to fire once.
    pub(crate) fn queue(&mut self, fault: Fault) {
        self.pending.push(fault);
    }

    /// Script `fault` to fire until [`FaultQueue::release`].
    pub(crate) fn hold(&mut self, fault: Fault) {
        self.held.insert(fault);
    }

    /// Stop a held fault.
    pub(crate) fn release(&mut self, fault: Fault) {
        self.held.remove(&fault);
    }

    /// Whether `fault` fires now, consuming it if it was a one-shot.
    ///
    /// Consumption removes the *first* matching entry rather than requiring it at the
    /// head of the queue: a test that scripts a busy `open` and a clamped `set` should
    /// not have to know which the code under test reaches first.
    pub(crate) fn take(&mut self, fault: Fault) -> bool {
        if self.held.contains(&fault) {
            return true;
        }
        match self.pending.iter().position(|queued| *queued == fault) {
            Some(index) => {
                self.pending.remove(index);
                true
            }
            None => false,
        }
    }

    /// The one-shot faults still waiting to fire, in order.
    pub(crate) fn pending(&self) -> Vec<Fault> {
        self.pending.clone()
    }

    /// The faults that fire until released.
    pub(crate) fn held(&self) -> Vec<Fault> {
        self.held.iter().copied().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fault_names_are_distinct() {
        let mut seen = BTreeSet::new();
        for &fault in Fault::ALL {
            assert!(seen.insert(fault.as_str()), "{fault:?} duplicates a name");
        }
    }

    #[test]
    fn a_queued_fault_fires_once_and_a_held_one_keeps_firing() {
        let mut queue = FaultQueue::default();
        queue.queue(Fault::Busy);
        assert!(queue.take(Fault::Busy));
        assert!(!queue.take(Fault::Busy), "a one-shot must not fire twice");

        queue.hold(Fault::SettleNeverConverges);
        assert!(queue.take(Fault::SettleNeverConverges));
        assert!(queue.take(Fault::SettleNeverConverges));
        queue.release(Fault::SettleNeverConverges);
        assert!(!queue.take(Fault::SettleNeverConverges));
    }

    #[test]
    fn a_fault_fires_even_when_another_was_queued_first() {
        // The trap this rules out: a queue that only ever inspects its head makes the
        // order two unrelated faults were scripted in load-bearing.
        let mut queue = FaultQueue::default();
        queue.queue(Fault::Busy);
        queue.queue(Fault::ClampOnWrite);
        assert!(queue.take(Fault::ClampOnWrite));
        assert_eq!(queue.pending(), vec![Fault::Busy]);
    }

    #[test]
    fn queueing_the_same_fault_twice_fires_it_twice() {
        let mut queue = FaultQueue::default();
        queue.queue(Fault::FrameTimeout);
        queue.queue(Fault::FrameTimeout);
        assert!(queue.take(Fault::FrameTimeout));
        assert!(queue.take(Fault::FrameTimeout));
        assert!(!queue.take(Fault::FrameTimeout));
        assert!(queue.held().is_empty());
    }
}
