//! `CameraBackend::watch` for real devices (design §2.5, docs/7 P4d).
//!
//! Three things live here and nothing else: the node list the watch diffs, the blocking
//! loop that turns a caller's deadline into a bounded `poll`, and the [`HotplugWatch`]
//! impl that joins them. Everything that *decides* anything is in [`crate::hotplug`] —
//! what a packet is worth, when a burst has finished, what changed since the last reading
//! — because that half is a fold over values a unit test can drive and this half is not.
//! The socket itself is one layer further down again, in [`crate::sys::uevent`], for the
//! reason `sys/mod.rs` gives: kernel-facing edges live there.
//!
//! ## Parity with the fake, which is what this sub-milestone means by "landed"
//!
//! The fake's scripted watch has passed the battery's `HotplugWatch` arm since P0
//! (`testkit::battery::arm_hotplug_watch`), so the target behaviour was written down
//! before this existed:
//!
//! - `watch()` succeeds;
//! - a deadline that arrives first is `Ok(None)`, never an error (E3) — "a caller polling
//!   on a cadence never has to interpret a timeout as a failure";
//! - an event names a non-empty node path;
//! - and the deadline is *honored*: the arm allows `deadline + 2 s` as a scheduling
//!   allowance, and a watch that ignored its deadline would be "a hang that has not
//!   happened yet".
//!
//! The battery is run against the fake, not against this (docs/7: "the battery runs
//! against the fake, whose watch arm has been green since P0"), so the arm's four claims
//! are restated here as unit tests over a real socket. What the battery cannot state, and
//! what this module's tests do, is the standing warning the P1 refusal carried: *a watch
//! that returned `Ok(None)` forever would be indistinguishable from a working one on a
//! quiet machine.* So the quiet case asserts a **lower** bound on the wait as well as an
//! upper one, and the events themselves are proven from committed packets rather than
//! from a poll that timed out.

use std::fmt;
use std::time::Instant;

use camino::Utf8PathBuf;
use schema::backend::{HotplugEvent, HotplugWatch};
use schema::error::Result;
use schema::limits;

use crate::hotplug::{Lost, Nodes, Tracker};
use crate::sys::uevent::{Received, UeventSocket};
use crate::sysfs;

/// The node list, read from sysfs.
///
/// [`sysfs::node_paths`] and **not** `crate::probe_nodes`: the latter opens every node for
/// `QUERYCAP`, which would make a watch a camera holder — colliding with the `uvcvideo`
/// interlock the R3 arm runs under (design §2.13) and with D12's "the daemon never opens a
/// camera until first use". So it also cannot race the driver into an ioctl on a
/// half-registered node.
///
/// It is not [`sysfs::nodes`] either, and the difference is measurable rather than
/// stylistic: `nodes` follows each node's `device` symlink and reads `idVendor`,
/// `idProduct` and `serial` under it — on this desk, ten nodes' worth of `canonicalize`
/// plus thirty small reads — and a watch discards every one of those, because a
/// [`HotplugEvent`] names a node and nothing else. `node_paths` is the `read_dir` on its
/// own: **it opens no file at all**, which matters most on the path a hotplug burst
/// provokes and against a tree the driver is still moving.
#[derive(Debug, Default)]
pub(crate) struct SysfsNodes;

impl Nodes for SysfsNodes {
    fn list(&mut self) -> Result<Vec<Utf8PathBuf>> {
        sysfs::node_paths()
    }
}

/// A hotplug watch over the kernel's uevent broadcast.
pub(crate) struct Watch<N> {
    socket: UeventSocket,
    tracker: Tracker<N>,
    /// One datagram's worth of scratch, allocated once. A `recv` that would not fit here
    /// reports the datagram's real size and the packet is dropped whole
    /// (`sys::uevent::Received::Oversized`), so this is a bound and not a guess.
    buf: Vec<u8>,
}

/// Hand-written, and the buffer is why.
///
/// `HotplugWatch` requires `Debug`, and `hotplug::Counts`' own doc offers a caller's
/// `Debug` of the watch as the production read path for the counters — "any layer holding
/// the watch can print it without this crate growing a logging dependency". A derived impl
/// would answer that invitation with eight thousand array elements, whose live prefix is
/// the last datagram read: raw `PRODUCT=`, `MODALIAS=` and serial strings from whichever
/// subsystem broadcast last, which is a neighbouring device's identifiers recorded by a
/// daemon nobody asked to record them. Its **length** is the only thing about it worth
/// printing, and it is what `sys::mmap` prints of its region for the same reason.
impl<N: fmt::Debug> fmt::Debug for Watch<N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Watch")
            .field("socket", &self.socket)
            .field("tracker", &self.tracker)
            .field("buf_bytes", &self.buf.len())
            .finish()
    }
}

impl Watch<SysfsNodes> {
    /// Bind the socket and prime the tracker against the tree as it is now.
    ///
    /// **The socket is bound first and the tree read second, deliberately.** A node that
    /// appears between the two is in the primed set *and* has a packet queued for it: the
    /// packet triggers a reading that finds nothing changed, which costs one `read_dir`.
    /// The other order would drop the node's packet on the floor before the socket
    /// existed and never notice it arrived.
    ///
    /// # Errors
    ///
    /// [`schema::Error::DeviceIo`] if the socket cannot be opened or bound — measured to
    /// need no privilege on this kernel \[PF:21\], so a refusal here is a container or an
    /// LSM rather than a missing capability. [`schema::Error::StorageIo`] if
    /// `/sys/class/video4linux` cannot be read at all.
    pub(crate) fn open() -> Result<Watch<SysfsNodes>> {
        let socket = UeventSocket::open()?;
        let tracker = Tracker::primed(SysfsNodes, crate::hotplug::Debounce::from_limits())?;
        Ok(Watch {
            socket,
            tracker,
            buf: vec![0; limits::UEVENT_PACKET_BYTES],
        })
    }
}

impl<N: Nodes> Watch<N> {
    /// Read what is queued on the socket, up to [`limits::MAX_UEVENTS_PER_DRAIN`] packets,
    /// stopping early once the tracker has enough to act on.
    ///
    /// The bound is on this pass and not on [`HotplugWatch::next_event`], which loops over
    /// it — see the constant's own doc, which says which bound is whose.
    ///
    /// `at` is the instant every packet in this pass is dated with: one clock read per
    /// drain rather than one per packet, so the fold sees a coherent instant and the tests
    /// can supply their own.
    ///
    /// Returns how many datagrams were taken off the socket.
    ///
    /// # Errors
    ///
    /// [`schema::Error::DeviceIo`] for a `recv` failure that is not one of the ordinary
    /// outcomes. An empty queue, an oversized datagram and a receive-queue overrun are all
    /// values, not errors — a watch that ended because a neighbouring subsystem said
    /// something odd is a watch that stops working on a busy machine (rubric B10).
    fn drain(&mut self, at: Instant) -> Result<u32> {
        let mut taken = 0;
        while taken < limits::MAX_UEVENTS_PER_DRAIN {
            match self.socket.recv_into(&mut self.buf)? {
                Received::Empty => break,
                Received::Packet { len } => {
                    taken += 1;
                    match self.buf.get(..len) {
                        Some(packet) => {
                            self.tracker.observe_packet(packet, at);
                        }
                        // `len` is how much of our own buffer the kernel filled, so it
                        // cannot exceed it — but it arrived from a syscall and this crate
                        // does not index on a number a syscall chose (rubric B10). The
                        // honest fallback is the one the oversize case already uses.
                        None => self.tracker.observe_lost(Lost::Oversized, at),
                    }
                }
                Received::Oversized { .. } => {
                    taken += 1;
                    self.tracker.observe_lost(Lost::Oversized, at);
                }
                Received::Overrun => {
                    taken += 1;
                    self.tracker.observe_lost(Lost::Overrun, at);
                }
            }
            // The turn rule (see `hotplug`'s header) makes a burst finish in the middle of
            // a drain, and reading on past it would coalesce the two halves of a driver
            // cycle after all. Whatever is left stays queued on the socket, which is
            // readable again the moment the caller comes back.
            if self.tracker.due(at) {
                break;
            }
        }
        Ok(taken)
    }
}

impl<N: Nodes + 'static> HotplugWatch for Watch<N> {
    fn next_event(&mut self, deadline: Instant) -> Result<Option<HotplugEvent>> {
        loop {
            if let Some(event) = self.tracker.pop() {
                return Ok(Some(event));
            }
            let now = Instant::now();
            if self.tracker.due(now) {
                self.tracker.rescan()?;
                continue;
            }
            // E3: the deadline arriving first is an answer, not a failure. Checked after
            // the queue and the debounce so a deadline that has already passed still hands
            // over work that is ready, rather than making the caller poll again for it.
            if now >= deadline {
                return Ok(None);
            }
            // Wait for whichever comes first: something to read, or the burst going quiet.
            // Never past the caller's deadline, which is the bound the battery's arm
            // measures (rubric A14).
            let budget = match self.tracker.next_deadline() {
                Some(wake) if wake < deadline => wake,
                _ => deadline,
            };
            if self.socket.wait_readable(budget)? {
                self.drain(Instant::now())?;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::os::fd::OwnedFd;
    use std::os::unix::net::UnixDatagram;
    use std::time::Duration;

    use camino::Utf8Path;

    use super::*;
    use crate::hotplug::Debounce;

    /// One real `video4linux` `add`, captured from this kernel — see
    /// `fixtures/README.md`.
    const ADD_VIDEO0: &[u8] = include_bytes!("../fixtures/uevent-add-video-node.bin");
    /// Its `remove` counterpart, from the same cycle.
    const REMOVE_VIDEO6: &[u8] = include_bytes!("../fixtures/uevent-remove-video-node.bin");
    /// A neighbouring subsystem's packet: the filter must drop it and it must not arm
    /// anything.
    const BIND_USB: &[u8] = include_bytes!("../fixtures/uevent-bind-usb-interface.bin");

    /// A node list the test drives, counting how often it was asked.
    ///
    /// The seam the debounce's central claim needs: "twenty triggers cost one reading" is
    /// not a statement about time, it is a statement about how many times this was called.
    #[derive(Debug)]
    struct CountingNodes {
        present: BTreeSet<Utf8PathBuf>,
        calls: u32,
    }

    impl CountingNodes {
        fn with(paths: &[&str]) -> CountingNodes {
            CountingNodes {
                present: paths.iter().map(|path| Utf8PathBuf::from(*path)).collect(),
                calls: 0,
            }
        }
    }

    impl Nodes for CountingNodes {
        fn list(&mut self) -> Result<Vec<Utf8PathBuf>> {
            self.calls += 1;
            Ok(self.present.iter().cloned().collect())
        }
    }

    /// The ten nodes this host presents — four cameras, and the Dell owns four of them
    /// \[PF:19\].
    const TEN_NODES: &[&str] = &[
        "/dev/video0",
        "/dev/video1",
        "/dev/video2",
        "/dev/video3",
        "/dev/video4",
        "/dev/video5",
        "/dev/video6",
        "/dev/video7",
        "/dev/video8",
        "/dev/video9",
    ];

    /// A watch whose socket is one end of a datagram pair, so a test can *put* packets
    /// into it: netlink refuses an unprivileged send \[PF:21\], and the drain's arithmetic
    /// does not care which address family produced the datagram.
    fn paired_watch(
        nodes: CountingNodes,
        debounce: Debounce,
    ) -> (UnixDatagram, Watch<CountingNodes>) {
        let (near, far) = UnixDatagram::pair().expect("a datagram pair");
        let tracker = Tracker::primed(nodes, debounce).expect("prime");
        let watch = Watch {
            socket: UeventSocket::from_datagram_fd(OwnedFd::from(near)),
            tracker,
            buf: vec![0; limits::UEVENT_PACKET_BYTES],
        };
        (far, watch)
    }

    #[test]
    fn a_burst_of_node_removals_costs_one_reading_and_still_reports_every_node() {
        // PF:19 is the whole reason this exists: one physical camera owns two capture
        // nodes and the Dell owns four, so one plug is several packets. The claim is
        // about *how many times the tree was read*, not about how long anything took —
        // there is no sleep here and the only real `Instant::now()` is the origin, which
        // nothing waits on.
        let quiet = Duration::from_millis(limits::HOTPLUG_QUIET_MS);
        let (far, mut watch) = paired_watch(
            CountingNodes::with(TEN_NODES),
            Debounce::new(quiet, Duration::from_secs(60)),
        );
        watch.tracker.nodes_mut().calls = 0;
        let t0 = Instant::now();

        // The kernel unregisters the nodes one at a time and broadcasts as it goes, so
        // the tree shrinks under us mid-burst. A build that read it per packet would read
        // it ten times and describe nine trees nobody asked about.
        for (step, node) in TEN_NODES.iter().enumerate() {
            watch
                .tracker
                .nodes_mut()
                .present
                .remove(Utf8Path::new(node));
            far.send(REMOVE_VIDEO6).expect("queue a remove");
            let step = u64::try_from(step).expect("a ten-element loop index");
            assert_eq!(
                watch
                    .drain(t0 + Duration::from_millis(step * 3))
                    .expect("drain"),
                1
            );
        }
        assert_eq!(
            watch.tracker.nodes_mut().calls,
            0,
            "the tree was read inside the burst"
        );

        // Still inside the quiet window, one millisecond short of it.
        let settled = t0 + Duration::from_millis(27);
        assert!(
            !watch
                .tracker
                .due(settled + quiet - Duration::from_millis(1))
        );
        assert!(watch.tracker.due(settled + quiet));

        watch.tracker.rescan().expect("rescan");
        assert_eq!(
            watch.tracker.nodes_mut().calls,
            1,
            "ten triggers cost more than one reading"
        );
        let mut removed = Vec::new();
        while let Some(event) = watch.tracker.pop() {
            match event {
                HotplugEvent::Removed { path } => removed.push(path),
                HotplugEvent::Added { path } => panic!("nothing arrived, yet {path} did"),
            }
        }
        assert_eq!(removed.len(), TEN_NODES.len(), "{removed:?}");
    }

    #[test]
    fn without_the_debounce_the_same_burst_costs_one_reading_per_packet() {
        // The inverse arm, and the reason the test above can go red: the *undebounced*
        // build is `Debounce::new(ZERO, ZERO)`, which is due the instant a trigger lands.
        // Ten packets, ten readings, each describing a tree that is still moving.
        let (far, mut watch) = paired_watch(
            CountingNodes::with(TEN_NODES),
            Debounce::new(Duration::ZERO, Duration::ZERO),
        );
        watch.tracker.nodes_mut().calls = 0;
        let t0 = Instant::now();

        for node in TEN_NODES {
            watch
                .tracker
                .nodes_mut()
                .present
                .remove(Utf8Path::new(node));
            far.send(REMOVE_VIDEO6).expect("queue a remove");
            watch.drain(t0).expect("drain");
            assert!(watch.tracker.due(t0));
            watch.tracker.rescan().expect("rescan");
        }
        let expected = u32::try_from(TEN_NODES.len()).expect("ten fits");
        assert_eq!(watch.tracker.nodes_mut().calls, expected);

        // Same ten events in the end — which is the point. The debounce changes what a
        // reading costs, never what the caller is eventually told.
        let mut removed = 0;
        while let Some(event) = watch.tracker.pop() {
            assert!(matches!(event, HotplugEvent::Removed { .. }), "{event:?}");
            removed += 1;
        }
        assert_eq!(removed, TEN_NODES.len());
    }

    #[test]
    fn a_drain_stops_at_the_turn_so_a_driver_cycle_is_not_coalesced_into_nothing() {
        // The turn rule, at the level where it does its work. One `uvcvideo` cycle puts
        // its removes and its adds into the same socket queue, ~100 ms apart on this host
        // \[PF:21\] — well inside a 250 ms quiet window. Draining straight through would
        // read the tree once, after everything had come back, and report *nothing
        // happened* for a cycle that took every camera away.
        let quiet = Duration::from_millis(limits::HOTPLUG_QUIET_MS);
        let (far, mut watch) = paired_watch(
            CountingNodes::with(TEN_NODES),
            Debounce::new(quiet, Duration::from_secs(60)),
        );
        watch.tracker.nodes_mut().calls = 0;
        let t0 = Instant::now();

        for _ in 0..4 {
            far.send(REMOVE_VIDEO6).expect("queue a remove");
        }
        far.send(ADD_VIDEO0).expect("queue an add");
        far.send(ADD_VIDEO0).expect("queue an add");

        // Every camera is gone by the time the first add lands.
        watch.tracker.nodes_mut().present.clear();
        assert_eq!(
            watch.drain(t0).expect("drain"),
            5,
            "the drain read past the packet that turned the burst"
        );
        assert!(
            watch.tracker.due(t0),
            "a burst that changed direction is finished whatever the clock says"
        );
        watch.tracker.rescan().expect("rescan");
        assert_eq!(watch.tracker.nodes_mut().calls, 1);
        assert_eq!(
            watch.tracker.counts().node_changes,
            5,
            "{:?}",
            watch.tracker.counts()
        );

        let mut removed = 0;
        while let Some(event) = watch.tracker.pop() {
            assert!(matches!(event, HotplugEvent::Removed { .. }), "{event:?}");
            removed += 1;
        }
        assert_eq!(removed, TEN_NODES.len(), "the removes were reported");

        // …and the add that turned the burst is not lost: the rest of it is still on the
        // socket, and the next drain opens a fresh burst going the other way.
        watch
            .tracker
            .nodes_mut()
            .present
            .extend(TEN_NODES.iter().map(|node| Utf8PathBuf::from(*node)));
        let later = t0 + Duration::from_millis(1);
        assert_eq!(watch.drain(later).expect("drain"), 1);
        assert!(
            !watch.tracker.due(later),
            "a fresh burst has its full window"
        );
        assert!(watch.tracker.due(later + quiet));
        watch.tracker.rescan().expect("rescan");
        let mut added = 0;
        while let Some(event) = watch.tracker.pop() {
            assert!(matches!(event, HotplugEvent::Added { .. }), "{event:?}");
            added += 1;
        }
        assert_eq!(added, TEN_NODES.len(), "the adds were reported");
    }

    #[test]
    fn a_flood_faster_than_it_is_consumed_is_read_in_bounded_batches_and_never_lost() {
        // Rubric B10's third hostile shape. The sender is put into non-blocking mode and
        // pushed until the kernel refuses, which is "faster than they are consumed"
        // arranged rather than described.
        let (far, mut watch) = paired_watch(
            CountingNodes::with(TEN_NODES),
            Debounce::new(
                Duration::from_millis(limits::HOTPLUG_QUIET_MS),
                Duration::from_secs(60),
            ),
        );
        watch.tracker.nodes_mut().calls = 0;
        far.set_nonblocking(true).expect("non-blocking sender");

        let mut sent = 0_u32;
        // Bounded so a kernel with an enormous socket buffer cannot turn this into an
        // unbounded loop: three drains' worth is more than enough to prove the bound.
        while sent < limits::MAX_UEVENTS_PER_DRAIN * 3 && far.send(BIND_USB).is_ok() {
            sent += 1;
        }
        assert!(
            sent > limits::MAX_UEVENTS_PER_DRAIN,
            "the sender only fitted {sent} packets, which cannot overrun one drain"
        );

        let t0 = Instant::now();
        let first = watch.drain(t0).expect("drain");
        assert_eq!(
            first,
            limits::MAX_UEVENTS_PER_DRAIN,
            "one drain read {first} packets, past its own bound"
        );
        // None of them was ours, so nothing was armed and nothing was read.
        assert!(!watch.tracker.due(t0 + Duration::from_secs(3_600)));
        assert_eq!(watch.tracker.nodes_mut().calls, 0);

        // The rest is still there: a bounded drain defers packets, it does not drop them.
        let mut drained = first;
        while drained < sent {
            let taken = watch.drain(t0).expect("drain");
            assert!(
                taken > 0,
                "the socket stopped answering with {drained} of {sent} read"
            );
            drained += taken;
        }
        assert_eq!(drained, sent);
        let counts = watch.tracker.counts();
        assert_eq!(counts.packets, u64::from(sent), "{counts:?}");
        assert_eq!(counts.not_ours, u64::from(sent), "{counts:?}");
        assert_eq!(counts.unreadable, 0, "{counts:?}");
    }

    #[test]
    fn a_flood_of_other_peoples_packets_is_all_read_and_still_answers_at_the_deadline() {
        // The same flood one layer up, because that is the layer the bound is *stated* at
        // and the layer nothing drove: `a_flood_faster_than_it_is_consumed_…` calls
        // `drain` by hand, so "one drain reads at most 64" was asserted where it holds and
        // "one `next_event` reads at most 64" was asserted nowhere — and it is not true.
        // `next_event` loops until it has an event or the caller's deadline arrives, which
        // is right (unread datagrams are datagrams the kernel throws away) and is what
        // `limits::MAX_UEVENTS_PER_DRAIN`'s doc now says.
        //
        // So the two claims that *are* true are the ones asserted here: every queued
        // packet is read — a bounded drain defers, it never drops — and the caller's
        // deadline is still what ends the call, however much the socket had to say. The
        // packets are all `BIND_USB`, so nothing arms the debounce and nothing is popped:
        // the storm is pure work, which is the shape the doc's paragraph is about.
        let (far, mut watch) = paired_watch(
            CountingNodes::with(TEN_NODES),
            Debounce::new(
                Duration::from_millis(limits::HOTPLUG_QUIET_MS),
                Duration::from_secs(60),
            ),
        );
        watch.tracker.nodes_mut().calls = 0;
        far.set_nonblocking(true).expect("non-blocking sender");

        let mut sent = 0_u32;
        while sent < limits::MAX_UEVENTS_PER_DRAIN * 3 && far.send(BIND_USB).is_ok() {
            sent += 1;
        }
        assert!(
            sent > limits::MAX_UEVENTS_PER_DRAIN,
            "the sender only fitted {sent} packets, so one drain would have sufficed"
        );

        let poll = Duration::from_millis(50);
        let started = Instant::now();
        let answer = watch
            .next_event(started + poll)
            .expect("a storm of somebody else's packets is not a failure");
        let waited = started.elapsed();

        assert_eq!(answer, None, "a `usb` bind is not a camera event");
        assert!(
            waited <= poll + Duration::from_secs(2),
            "a 50 ms deadline was honored only after {waited:?}"
        );
        let counts = watch.tracker.counts();
        assert_eq!(
            counts.packets,
            u64::from(sent),
            "{sent} packets were queued and {counts:?} came off the socket"
        );
        assert_eq!(counts.not_ours, u64::from(sent), "{counts:?}");
        assert_eq!(watch.tracker.nodes_mut().calls, 0, "the tree was re-read");
    }

    #[test]
    fn printing_a_watch_shows_its_counters_and_not_the_last_packet_it_read() {
        // `HotplugWatch` requires `Debug` and `hotplug::Counts` offers it as the way a
        // holding layer reads the counters, so what a `?watch` field renders is a product
        // decision rather than a derive. A derived impl prints all 8192 scratch bytes,
        // whose live prefix is the last datagram — a neighbouring device's `PRODUCT=`,
        // `MODALIAS=` and serial strings, recorded by a daemon nobody asked to record
        // them, with the counters buried under them.
        let (far, mut watch) = paired_watch(
            CountingNodes::with(TEN_NODES),
            Debounce::new(Duration::ZERO, Duration::from_secs(60)),
        );
        far.send(BIND_USB).expect("queue a neighbour's packet");
        watch.drain(Instant::now()).expect("drain");

        let rendered = format!("{watch:?}");
        assert!(rendered.contains("packets: 1"), "{rendered}");
        assert!(
            rendered.contains(&format!("buf_bytes: {}", limits::UEVENT_PACKET_BYTES)),
            "the buffer's size is the one thing about it worth printing: {rendered}"
        );
        assert!(
            !rendered.contains("MODALIAS"),
            "the last packet a neighbouring subsystem broadcast is in this line: {rendered}"
        );
        assert!(
            rendered.len() < 1_000,
            "{} bytes of Debug output for one watch",
            rendered.len()
        );
    }

    #[test]
    fn a_quiet_socket_answers_at_its_deadline_and_not_before_it() {
        // The battery's `HotplugWatch` arm, restated against a real netlink socket — and
        // with the lower bound the battery cannot carry, because the fake in its
        // population returns immediately by design. Without that bound, "a watch that
        // returned `Ok(None)` forever" passes every assertion the arm makes on a quiet
        // machine, which is the failure the P1 refusal's comment named.
        //
        // `Ok(None)` is asserted rather than tolerated, and a stray uevent cannot make
        // that flaky: the quiet window is 250 ms, so nothing a passing USB device does
        // inside a 50 ms deadline can produce an *event* — at most it arms a burst that
        // this call will not live to see finish.
        const {
            assert!(
                limits::HOTPLUG_QUIET_MS > 50,
                "the paragraph above is only true while the quiet window outlasts the poll"
            );
        }
        let mut watch = Watch::open().expect("bind NETLINK_KOBJECT_UEVENT group 1");
        let started = Instant::now();
        let poll = Duration::from_millis(50);
        let answer = watch
            .next_event(started + poll)
            .expect("a timeout is Ok(None)");
        let waited = started.elapsed();

        assert_eq!(answer, None, "after {waited:?}");
        assert!(
            waited >= poll - Duration::from_millis(10),
            "a 50 ms deadline was answered after {waited:?}, so nothing waited"
        );
        assert!(
            waited <= poll + Duration::from_secs(2),
            "a 50 ms deadline was honored only after {waited:?}"
        );
    }

    #[test]
    fn an_already_spent_deadline_answers_immediately_rather_than_blocking() {
        // The bound at its edge: `poll` reads a negative timeout as "wait forever", which
        // is the one value this must never pass (`sys::wait` carries that argument). A
        // caller that hands over a deadline in the past gets an answer, not a hang.
        let mut watch = Watch::open().expect("bind NETLINK_KOBJECT_UEVENT group 1");
        let started = Instant::now();
        watch
            .next_event(started - Duration::from_secs(1))
            .expect("a spent deadline is not a failure");
        assert!(started.elapsed() < Duration::from_secs(1), "it blocked");
    }

    #[test]
    fn a_settled_burst_is_reported_when_it_settles_and_not_when_the_caller_gives_up() {
        // What `Tracker::next_deadline` is *for*, stated as behaviour. `next_event` waits
        // for whichever comes first — something to read, or the burst going quiet — and
        // without the second half a caller that offered a generous deadline waits all of
        // it for an event that was ready in a quarter of a second.
        //
        // The mutation floor found this, and it is worth saying how: `Debounce`'s own
        // `next_deadline` is asserted four ways in `hotplug`, but nothing asserted that
        // the `Tracker` *forwards* it, and nothing asserted the consequence. Replacing
        // `Tracker::next_deadline` with `None` passed the whole suite, because every
        // other test here either drives `drain`/`rescan` by hand or hands over a deadline
        // it is content to wait out. This is the one that cannot.
        //
        // The bound is one-sided and enormous on purpose: the claim is "it came back
        // long before the deadline", not a latency figure. Nothing sleeps — the wait is
        // `next_event`'s own deadline, which is the bound the trait declares (rubric A14)
        // and not synchronisation (note N3).
        let quiet = Duration::from_millis(20);
        let (far, mut watch) = paired_watch(
            CountingNodes::with(TEN_NODES),
            Debounce::new(quiet, Duration::from_secs(60)),
        );
        watch
            .tracker
            .nodes_mut()
            .present
            .remove(Utf8Path::new("/dev/video6"));
        far.send(REMOVE_VIDEO6).expect("queue a remove");

        let generous = Duration::from_secs(10);
        let started = Instant::now();
        let event = watch
            .next_event(started + generous)
            .expect("a queued packet is not an error");
        let waited = started.elapsed();

        assert!(
            matches!(event, Some(HotplugEvent::Removed { .. })),
            "{event:?} after {waited:?}"
        );
        assert!(
            waited < generous / 2,
            "the burst settled after {quiet:?} and the event arrived only after \
             {waited:?}, so the wait ran to the caller's deadline instead of to the \
             burst's"
        );
    }

    #[test]
    fn the_watch_is_primed_so_a_machine_that_already_has_cameras_reports_no_arrivals() {
        // The other half of "a skip must not read as pass": a watch that started with an
        // empty idea of the tree would announce every node on the machine the first time
        // anything at all happened, and on this desk that is ten arrivals nobody plugged
        // in.
        let (far, mut watch) = paired_watch(
            CountingNodes::with(TEN_NODES),
            Debounce::new(Duration::ZERO, Duration::from_secs(60)),
        );
        let t0 = Instant::now();
        far.send(ADD_VIDEO0).expect("queue an add");
        watch.drain(t0).expect("drain");
        watch.tracker.rescan().expect("rescan");
        assert_eq!(
            watch.tracker.pop(),
            None,
            "a primed watch invented an arrival"
        );
    }
}
