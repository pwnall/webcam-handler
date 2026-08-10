//! What one uevent packet means to this backend (design §2.5, docs/7 P4d).
//!
//! [`trigger`] takes bytes and returns a value. No socket, no clock, no filesystem — the
//! socket is [`crate::sys::uevent`]'s and the seam between them is this signature. That
//! split is not tidiness: **a packet off a kernel socket is attacker-shaped input**
//! (rubric B10), so the deciding must be reachable by a fixture rather than only by a
//! kernel, and the malformed/truncated/flood packets docs/8 asks for are ordinary test
//! inputs here.
//!
//! This module is deliberately *outside* `src/sys/`, and that is what puts it inside the
//! mutation floor: `.cargo/mutants.toml` excludes `src/sys/` by name — "a mutant there is
//! only decidable against a device" — while this file is named in `examine_globs`, because
//! a pure fold over bytes is exactly the shape the floor exists for.
//!
//! ## How much `kobject-uevent` gives us, honestly
//!
//! Read in full at 0.2.0 before adopting it. The crate owns **no socket**: its whole
//! netlink surface is one function, `UEvent::from_netlink_packet(&[u8])`, which
//! `from_utf8`s the packet, splits it on `'\0'`, reads `key=value` pairs into a map, and
//! insists on `ACTION`, `DEVPATH`, `SUBSYSTEM` and `SEQNUM`. It has no `unwrap`, no
//! indexing and no panic path, and every malformed shape becomes a typed `Err`. Its
//! `netlink-sys` dependency is a dev-dependency of that crate, so none of it ships.
//! Licence MIT, on deny.toml's allowlist and named in design §2.8.
//!
//! So the split is: **the crate owns the key/value split and the action vocabulary; the
//! socket, the group choice, the bind, the read loop, the subsystem filter, the debounce
//! and the re-enumeration are ours.** That is the "~30 lines" docs/7 budgets, and it is
//! why the design calls the crate "a *parser*".
//!
//! Three things it does not do, each of which this module has to:
//!
//! 1. **It validates nothing about `DEVPATH`.** `value.parse::<PathBuf>()` has
//!    `Err = Infallible`, so `DEVPATH=`, `DEVPATH=../../etc` and `DEVPATH=relative` all
//!    parse. Nothing here turns a packet field into a filesystem path — see below.
//! 2. **An `ACTION` it does not know is a hard `Err`,** raised *before* `SUBSYSTEM` is
//!    consulted. A future kernel's new verb on a `tty` would therefore arrive looking
//!    exactly like a corrupt packet. [`NotOurs::UnmodelledAction`] is where that goes
//!    instead: "a shape this build cannot read" and "a shape this build does not model"
//!    must not be the same answer (AGENTS rule 6).
//! 3. **It requires the *whole packet* to be UTF-8.** One bad byte anywhere — including
//!    inside a neighbouring subsystem's device strings — refuses the packet entirely.
//!    That is a safe failure here and a recorded limitation: see [`Unreadable::NotUtf8`].
//!
//! ## Truncation is not detectable here, and does not have to be
//!
//! A uevent packet carries no length field and no terminator, and `SEQNUM` — the last
//! key the kernel writes — is a number, so a packet cut inside it is still a *well-formed
//! packet about a smaller sequence*. No parser can tell the difference from the bytes
//! alone, and one that claimed to would be lying. The guard is one layer down:
//! [`crate::sys::uevent`] reads with `MSG_TRUNC` and drops any datagram that did not fit
//! whole, and netlink delivers a datagram atomically or not at all — so a truncated
//! packet is a fixture's shape, never a kernel's. The prefix walk in this module's tests
//! asserts what is actually true (every prefix is a value, and a prefix that still
//! decodes names the same node) rather than a stronger claim that would have to be
//! disabled the first time it ran.
//!
//! ## One buffer is one packet, and the flood fixture has to respect that
//!
//! [`trigger`] reads its whole argument as a single uevent. Hand it two packets
//! concatenated and it answers about the *last* `SUBSYSTEM` it saw, because the crate's
//! fold overwrites each key as it goes — measured against this host's real burst, where
//! all fifty-six packets in one 12953-byte buffer decode as one `module` event rather than
//! as anything about a camera. That is safe (no panic, and the answer is a value) but it
//! is not a per-packet reading, and it is not a case that can arise: netlink is
//! `SOCK_DGRAM`, so [`crate::sys::uevent::UeventSocket::recv_into`] returns exactly one
//! datagram per call and never a concatenation. Written down because the "flood" fixture
//! rubric B10 asks for must therefore be a *sequence* of packets driven through
//! [`trigger`] one at a time — a single spliced buffer would assert the wrong thing and
//! pass.
//!
//! ## Why no `/dev` path comes out of this function
//!
//! A `video4linux` uevent carries `DEVNAME=video3` (a bare name, no `/dev/` prefix), and
//! composing `/dev/{DEVNAME}` from it would put a packet field into a path — the exact
//! move rubric B10 exists to stop. It is also unnecessary: `HotplugEvent`'s own contract
//! says "**Re-enumeration decides what camera it belongs to** — the event names a node,
//! never a camera" (`schema::backend`), so the watch's answer comes from diffing
//! [`crate::sysfs::nodes`], whose paths the kernel maintains and which `enumerate`
//! already trusts. **The socket is a trigger, not a source of truth.** A dropped,
//! oversized or unparsable packet therefore costs a *trigger* and never desynchronises
//! the watch, which is the property that makes every refusal above affordable.
//!
//! **Nothing a packet said escapes [`trigger`].** [`Trigger::NodeChanged`] carries a
//! direction and nothing else: no `DEVPATH`, no `DEVNAME`, no `SEQNUM`. It carried the
//! sysfs `devpath` briefly, for a debounce diagnostic and a log line that were both
//! described in a doc comment and written nowhere — [`Debounce::observe`] takes a
//! direction and an instant, and this crate has never had a logging dependency (see
//! [`Counts`]). A typed field nothing reads is rubric A8's subject, and here it is worse
//! than inert: keeping a heap copy of attacker-influenced kernel text alive past the parse
//! is the one thing that would leave "no packet text becomes a path" a convention rather
//! than a fact about the type. When a subscriber wants the kernel's own words (P4e), the
//! field arrives with the consumer that reads it.
//!
//! ## The debounce, and the turn rule that keeps a driver cycle visible
//!
//! [`Debounce`] is the second half of this module and it is the same kind of thing as
//! [`trigger`]: a fold over values, here `(a trigger arrived, what time is it)`. Nothing
//! in it reads a clock — every instant is the caller's — which is the settle policy's
//! argument (design D5) applied to a socket instead of to a stream of frames. It is not a
//! second home for that law: `engine::settle` decides when a *control* has stopped moving
//! and this decides when the *kernel* has stopped talking, and the engine is above this
//! crate in the DAG anyway (`scripts/gates/dependency-walls.sh` keeps it there), so the
//! shared shape is the seam — an `Instant` parameter — and not the code.
//!
//! **Why it exists is PF:19**: one physical camera owns two capture nodes and the Dell
//! owns four, so one plug is several packets and re-reading the tree per packet would be
//! several readings of a tree that is still moving. [`limits::HOTPLUG_QUIET_MS`] is sized
//! against the measured gap population and [`limits::HOTPLUG_MAX_DEFERRAL_MS`] is the
//! ceiling that stops a device stuck in an add/remove loop from deferring the reading
//! forever.
//!
//! **The turn rule is the part that is not a timing guess.** A quiet window cannot be
//! sized to separate the removes of a driver cycle from the adds that follow: measured on
//! this host \[PF:21\], the largest gap *inside* the remove phase was 93.6 ms and the gap
//! between the last remove and the first add was 98.5 ms — five milliseconds apart, and
//! both are `modprobe` timings rather than bounds. So the window does not try. Instead, a
//! trigger whose direction reverses the burst being coalesced ends that burst immediately:
//! the tree has passed through a state the caller must be given the chance to see, and
//! coalescing across the turn would report a whole `uvcvideo` cycle as *nothing happened*.
//! With the turn rule the same window is right at 50 ms and at 2 s, which is what a
//! constant sized against two samples of `modprobe` is not.
//!
//! ## What a diff-based watch does not report
//!
//! [`Tracker`] emits the difference between successive readings of the node list, so it
//! can never invent an event and can never drift out of step with the tree — the property
//! that makes every dropped packet above affordable. The price, stated rather than
//! discovered: **a node that leaves and returns entirely between two readings is not
//! reported at all**, because between those two readings nothing changed. The turn rule
//! shrinks that window to the gap between one packet and the reading it forces; it does
//! not close it, and no diff-based watch closes it.

use std::collections::{BTreeSet, VecDeque};
use std::fmt;
use std::time::{Duration, Instant};

use camino::Utf8PathBuf;
use kobject_uevent::{ActionType, Error as ParseError, UEvent};
use schema::backend::HotplugEvent;
use schema::error::Result;
use schema::limits;

/// The subsystem a V4L2 node belongs to.
///
/// **The filter is on this key and not on the `DEVPATH` text.** The group-1 broadcast
/// carries every subsystem on the machine — usb, block, input, drm, media, power supply —
/// and `SUBSYSTEM` is the kernel answering the question directly, while a substring match
/// on a path is a guess that happens to agree.
///
/// The direction the guess fails in is *downwards*, not upwards: a `usb` interface's
/// `DEVPATH` is a **prefix** of its `video4linux` child's, so a usb event never contains
/// the substring and a substring filter drops it correctly. What it gets wrong is any
/// device that hangs *below* a `video4linux` node — an `input` child, or a future bus
/// putting anything under `.../video4linux/videoN/` — whose path contains the word while
/// its subsystem is something else entirely. Nothing on this desk emits one, which is
/// exactly why the packet that pins this rule is synthetic and says so
/// (`tests::a_neighbours_packet_under_a_video4linux_path_is_still_not_ours`); without it
/// the substring spelling passes the whole suite, measured.
const VIDEO_SUBSYSTEM: &str = "video4linux";

/// What one packet is worth to the watch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Trigger {
    /// A `video4linux` node was added or removed. Re-enumeration decides what that means
    /// for the camera list; this only says the tree moved, and which way.
    ///
    /// The direction is the whole payload, and that is the header's "nothing a packet said
    /// escapes this function" stated as a type rather than as a promise.
    NodeChanged {
        /// Which way — the one thing [`Debounce`] needs, for the turn rule.
        action: NodeAction,
    },
    /// A well-formed packet that is not news for this backend.
    NotOurs(NotOurs),
    /// A packet this build could not read. Counted and dropped, never fatal: one
    /// malformed broadcast from a neighbouring subsystem must not be able to end a watch
    /// (rubric B10, docs/8's hostile-bytes row).
    Unreadable(Unreadable),
}

/// Which way a node went.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NodeAction {
    /// `ACTION=add`.
    Added,
    /// `ACTION=remove`.
    Removed,
}

/// Why a well-formed packet is not news. Carried rather than collapsed into one silent
/// drop, because "the machine is busy with other subsystems" and "this kernel speaks a
/// verb we do not model" are different things to see in a counter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NotOurs {
    /// `SUBSYSTEM` was something else. The overwhelming majority.
    OtherSubsystem,
    /// A `video4linux` packet whose action is not `add`/`remove` — `change`, `bind` and
    /// `unbind` are all emitted by a driver cycle and none of them is a node appearing.
    OtherAction,
    /// An `ACTION` string neither this build nor `kobject-uevent` models.
    ///
    /// The subsystem is unknown here and that is not a gap we can close: the parser
    /// refuses the packet on the action before it reads `SUBSYSTEM`, so re-deriving it
    /// would mean parsing the packet a second time, our way, which is the second home
    /// this crate was adopted to avoid.
    ///
    /// A packet cut inside `ACTION=add` lands here too — `ACTION=a` is a verb nobody
    /// models — which makes this the one counter that conflates "a newer kernel" with
    /// "a truncated packet". Both are dropped and neither can end a watch, so the
    /// conflation costs a diagnosis and nothing else; see the header on why truncation
    /// is not detectable in this format at all.
    ///
    /// It is the one [`NotOurs`] that still arms the debounce, and the unknowable
    /// subsystem is why: this build cannot say the packet was not about a camera, so it
    /// answers it the way it answers every other trigger it cannot read — by reading the
    /// tree ([`Tracker::observe_packet`]).
    UnmodelledAction,
}

/// Why a packet could not be read at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Unreadable {
    /// The packet was not UTF-8 *as a whole*.
    ///
    /// `kobject-uevent` validates the entire buffer before it splits it, so one bad byte
    /// in any neighbour's field refuses the packet. That is the conservative direction —
    /// nothing partially-decoded reaches a decision — and its cost is recorded here
    /// rather than discovered: a `video4linux` packet could in principle be lost to a
    /// byte in a field we never read. The watch survives it, because a lost packet is a
    /// lost trigger (see the header).
    NotUtf8,
    /// A required key was absent. The kernel emits all four on every packet; a datagram
    /// missing one did not come from a kernel we understand.
    Missing(&'static str),
    /// A required key was present and unreadable — today only `SEQNUM`, which must parse
    /// as a `u64`.
    Malformed(&'static str),
    /// A refusal this build does not model.
    ///
    /// Unreachable through [`trigger`] with `kobject-uevent` 0.2.0: the three error
    /// variants that land here are either impossible (`InvalidDevPath`, whose
    /// `FromStr for PathBuf` cannot fail) or belong to the crate's *sysfs* constructor,
    /// which this crate never calls. It exists so a future version that adds a refusal
    /// cannot turn a packet into a panic or a compile error at a call site (AGENTS rule
    /// 6's fallback arm), and it deliberately holds no payload: everything that would go
    /// in it is text from the packet, and this value is a counter rather than a message.
    Unrecognized,
}

/// What one uevent packet is worth to this backend.
///
/// Total over every byte string: there is no input for which this panics, allocates
/// unboundedly, or reads a byte the caller did not hand it.
pub(crate) fn trigger(packet: &[u8]) -> Trigger {
    let event = match UEvent::from_netlink_packet(packet) {
        Ok(event) => event,
        Err(error) => return refusal(&error),
    };

    if event.subsystem != VIDEO_SUBSYSTEM {
        return Trigger::NotOurs(NotOurs::OtherSubsystem);
    }
    let action = match event.action {
        ActionType::Add => NodeAction::Added,
        ActionType::Remove => NodeAction::Removed,
        ActionType::Change
        | ActionType::Move
        | ActionType::Online
        | ActionType::Offline
        | ActionType::Bind
        | ActionType::Unbind => return Trigger::NotOurs(NotOurs::OtherAction),
    };
    Trigger::NodeChanged { action }
}

/// Read `kobject-uevent`'s refusal as one of ours.
fn refusal(error: &ParseError) -> Trigger {
    match error {
        // Not a failure: a verb this build does not model. See `NotOurs::UnmodelledAction`.
        ParseError::UnexpectedAction(_) => Trigger::NotOurs(NotOurs::UnmodelledAction),
        ParseError::NotUtf8 => Trigger::Unreadable(Unreadable::NotUtf8),
        ParseError::ActionNotFound => Trigger::Unreadable(Unreadable::Missing("ACTION")),
        ParseError::DevPathNotFound => Trigger::Unreadable(Unreadable::Missing("DEVPATH")),
        ParseError::SubsystemNotFound => Trigger::Unreadable(Unreadable::Missing("SUBSYSTEM")),
        ParseError::SeqMissing => Trigger::Unreadable(Unreadable::Missing("SEQNUM")),
        ParseError::InvalidSeqNum(_) => Trigger::Unreadable(Unreadable::Malformed("SEQNUM")),
        // Everything else, including three variants that are unreachable from
        // `from_netlink_packet` at 0.2.0 and therefore have no arm of their own:
        // `InvalidDevPath` (its `FromStr for PathBuf` has `Err = Infallible`, so the
        // parser cannot raise it) and `Io`/`NotInsideMountpoint` (raised only by the
        // sysfs constructor this crate never calls). An arm no input can reach is an arm
        // no test can turn red, which the mutation floor would report as a survivor and
        // which rubric A8 calls a declaration nobody reads; one honest fallback says the
        // same thing and stays killable.
        _ => Trigger::Unreadable(Unreadable::Unrecognized),
    }
}

/// A trigger that arrived without its bytes.
///
/// Both are the kernel telling us something happened that we could not read, which is
/// exactly as useful as a packet we *could* read: the watch's answer to either is to read
/// the node list again. Kept apart because they say different things about the machine —
/// one is a sender we do not understand, the other is us falling behind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Lost {
    /// A datagram longer than the receive buffer, dropped whole rather than read as a
    /// prefix.
    Oversized,
    /// The kernel dropped broadcasts because our receive queue was full (`ENOBUFS`).
    Overrun,
}

/// What a watch has seen since it opened.
///
/// Every refusal in this module is "counted, dropped, never fatal" (docs/8's hostile-bytes
/// row), and this is the counting half: without it "one malformed broadcast must not end
/// the watch" would be indistinguishable from "malformed broadcasts are invisible".
///
/// It is read two ways and neither is a new API. [`Tracker`] carries it in its `Debug`, so
/// any layer holding the watch can print it without this crate growing a logging
/// dependency it has never had; and this crate's own tests read the fields by name, which
/// is where "a malformed neighbour did not end the watch" is actually asserted.
///
/// `u64` rather than `u32`: a machine that broadcasts a packet every millisecond for a
/// month is a machine this counter should still be able to describe, and saturating
/// arithmetic means it never wraps into a smaller, more comforting number.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Counts {
    /// Datagrams read off the socket, whatever they turned out to be.
    pub(crate) packets: u64,
    /// `video4linux` add/remove packets — the ones that armed the debounce.
    pub(crate) node_changes: u64,
    /// Well-formed packets about something else.
    pub(crate) not_ours: u64,
    /// Packets this build could not read.
    pub(crate) unreadable: u64,
    /// Datagrams too long for the receive buffer ([`Lost::Oversized`]).
    pub(crate) oversized: u64,
    /// Times the kernel reported it had dropped broadcasts ([`Lost::Overrun`]).
    ///
    /// Kept apart from every other counter because it is the one number that says *this
    /// machine outran us*, which is the finding that would send somebody to `SO_RCVBUF`.
    /// Three measured `uvcvideo` cycles never raised it \[PF:21\].
    pub(crate) overrun: u64,
    /// Times the node list was read again.
    pub(crate) rescans: u64,
}

impl Counts {
    /// Add one, saturating. A counter that wrapped would answer a question about a busy
    /// machine with a number from a quiet one.
    fn bump(counter: &mut u64) {
        *counter = counter.saturating_add(1);
    }
}

/// When a burst of triggers has finished, decided as a fold over instants the caller
/// supplies.
///
/// See the module header for the turn rule and for why the quiet window is not
/// load-bearing.
#[derive(Debug)]
pub(crate) struct Debounce {
    /// How long the socket must go quiet before a burst is finished.
    quiet: Duration,
    /// The longest a burst may defer the reading, however busy the socket stays.
    ceiling: Duration,
    /// When the burst being coalesced started.
    first: Option<Instant>,
    /// The most recent trigger in it.
    last: Option<Instant>,
    /// The direction every trigger in the burst so far agreed on, when they had one.
    direction: Option<NodeAction>,
    /// A trigger reversed the burst's direction, so it is finished whatever the clock
    /// says.
    turned: bool,
}

impl Debounce {
    /// The policy the watch runs, out of [`limits`].
    pub(crate) fn from_limits() -> Debounce {
        Debounce::new(
            Duration::from_millis(limits::HOTPLUG_QUIET_MS),
            Duration::from_millis(limits::HOTPLUG_MAX_DEFERRAL_MS),
        )
    }

    /// A policy with the two windows named, so a test can state the ones it means.
    pub(crate) fn new(quiet: Duration, ceiling: Duration) -> Debounce {
        Debounce {
            quiet,
            ceiling,
            first: None,
            last: None,
            direction: None,
            turned: false,
        }
    }

    /// A trigger arrived at `at`, going `direction` — or going nowhere in particular, for
    /// a [`Lost`] one whose bytes never got read.
    pub(crate) fn observe(&mut self, direction: Option<NodeAction>, at: Instant) {
        if let (Some(pending), Some(arriving)) = (self.direction, direction)
            && pending != arriving
        {
            self.turned = true;
        }
        self.first.get_or_insert(at);
        // `max` rather than assignment: a caller handing instants out of order must not
        // be able to move the deadline backwards and make an already-finished burst
        // unfinished again.
        self.last = Some(self.last.map_or(at, |last| last.max(at)));
        self.direction = self.direction.or(direction);
    }

    /// Has the burst finished?
    pub(crate) fn due(&self, now: Instant) -> bool {
        if self.turned {
            return true;
        }
        // `checked_add` on both: an `Instant` near the representable end of the clock
        // must answer "not yet" rather than panicking in a release build and wrapping in
        // a debug one. A burst that can never come due is still bounded — the caller's
        // own deadline is what returns control to it.
        let quiet_over = self
            .last
            .and_then(|last| last.checked_add(self.quiet))
            .is_some_and(|deadline| now >= deadline);
        let ceiling_over = self
            .first
            .and_then(|first| first.checked_add(self.ceiling))
            .is_some_and(|deadline| now >= deadline);
        quiet_over || ceiling_over
    }

    /// When a bounded wait should give up and ask [`Debounce::due`] again, if a burst is
    /// being coalesced at all.
    pub(crate) fn next_deadline(&self) -> Option<Instant> {
        let quiet = self.last?.checked_add(self.quiet)?;
        let ceiling = self.first.and_then(|first| first.checked_add(self.ceiling));
        Some(ceiling.map_or(quiet, |ceiling| quiet.min(ceiling)))
    }

    /// The reading happened; the next trigger opens a new burst.
    pub(crate) fn taken(&mut self) {
        self.first = None;
        self.last = None;
        self.direction = None;
        self.turned = false;
    }
}

/// Where the watch reads the node list.
///
/// A seam, for the reason `daemon::uds`'s `Accepting` is one: "twenty triggers cost one
/// reading" is the claim the debounce exists to make, and a test that had to cycle a
/// driver to state it would be a test nobody runs. The real implementation is
/// `crate::watch::SysfsNodes`, which is a `read_dir` of `/sys/class/video4linux` and opens
/// **no file at all** — not the nodes (the heavier `QUERYCAP` walk belongs to `enumerate`,
/// and design D12 says the daemon opens a camera on first use, which a watch is not) and
/// not the sysfs attributes under them either, since a node path is the whole of what an
/// event names.
pub(crate) trait Nodes: fmt::Debug + Send {
    /// Every `/dev/video*` node the kernel currently lists.
    ///
    /// # Errors
    ///
    /// Whatever reading the tree failed with. A failure does not spend the trigger that
    /// asked for it: the burst stays armed, so a transient failure is retried on the next
    /// call rather than losing the change that provoked it.
    fn list(&mut self) -> Result<Vec<Utf8PathBuf>>;
}

/// The watch's clock-free half: the debounce, the node set it last saw, and the events it
/// has decided to hand out.
///
/// Everything here takes its instants from the caller and its node list from [`Nodes`], so
/// the whole of the watch's *policy* is reachable from a unit test — the blocking socket
/// loop in `crate::watch` is the only part that needs a kernel.
#[derive(Debug)]
pub(crate) struct Tracker<N> {
    nodes: N,
    debounce: Debounce,
    /// The tree as of the last reading. Primed when the watch opens, so a watch on a
    /// machine that already has ten cameras does not announce ten arrivals.
    known: BTreeSet<Utf8PathBuf>,
    /// Decided events waiting to be handed out, oldest first.
    queued: VecDeque<HotplugEvent>,
    counts: Counts,
}

impl<N: Nodes> Tracker<N> {
    /// Prime a tracker against the tree as it is now.
    ///
    /// # Errors
    ///
    /// Whatever [`Nodes::list`] failed with. Priming is not optional: a tracker that
    /// started empty would report every node on the machine as an arrival the first time
    /// anything happened.
    pub(crate) fn primed(mut nodes: N, debounce: Debounce) -> Result<Tracker<N>> {
        let known = nodes.list()?.into_iter().collect();
        Ok(Tracker {
            nodes,
            debounce,
            known,
            queued: VecDeque::new(),
            counts: Counts::default(),
        })
    }

    /// Read one packet and fold what it is worth into the debounce.
    ///
    /// Returns the decision. The drain discards it — the counters above already hold
    /// everything a production caller would do with it — and the tests read it, which is
    /// what makes "this fixture is a `video4linux` remove" an assertion rather than an
    /// assumption behind the counter.
    ///
    /// ## Which refusals still arm the debounce, and why that is not symmetry for its own
    /// sake
    ///
    /// A packet this build *could not read* arms it, exactly as [`Tracker::observe_lost`]
    /// arms it for a packet whose bytes never arrived. That is the header's claim — "a
    /// dropped, oversized or unparsable packet costs a *trigger* and never desynchronises
    /// the watch" — made true rather than asserted: the refusals in [`trigger`] are
    /// deliberately liberal (one non-UTF-8 byte anywhere refuses the whole packet,
    /// including bytes in fields this build never reads), and the thing that makes liberal
    /// refusal affordable is that the answer to *any* trigger is to read the tree again.
    /// Without this, a camera's `remove` lost to one bad byte in a neighbour's field is a
    /// node that stays listed until something unrelated happens.
    ///
    /// [`NotOurs::UnmodelledAction`] arms it for the same reason: `kobject-uevent` refuses
    /// on `ACTION` *before* it reads `SUBSYSTEM`, so that packet's subsystem is unknowable
    /// and it may have been about a camera.
    ///
    /// [`NotOurs::OtherSubsystem`] and [`NotOurs::OtherAction`] do **not** arm it. Those
    /// are packets this build positively knows are not news, and they are the overwhelming
    /// majority — thirty-six of fifty-six in one measured cycle \[PF:21\] — so arming on
    /// them would make every busy machine a rescan loop. The cost of the arms that do fire
    /// is bounded by the debounce itself: one reading per quiet window however many
    /// malformed packets arrive, which is the bound [`Lost`] already accepts.
    pub(crate) fn observe_packet(&mut self, packet: &[u8], at: Instant) -> Trigger {
        Counts::bump(&mut self.counts.packets);
        let decision = trigger(packet);
        match &decision {
            Trigger::NodeChanged { action } => {
                Counts::bump(&mut self.counts.node_changes);
                self.debounce.observe(Some(*action), at);
            }
            Trigger::NotOurs(NotOurs::UnmodelledAction) => {
                Counts::bump(&mut self.counts.not_ours);
                self.debounce.observe(None, at);
            }
            Trigger::NotOurs(NotOurs::OtherSubsystem | NotOurs::OtherAction) => {
                Counts::bump(&mut self.counts.not_ours);
            }
            Trigger::Unreadable(_) => {
                Counts::bump(&mut self.counts.unreadable);
                self.debounce.observe(None, at);
            }
        }
        decision
    }

    /// A trigger arrived whose bytes never got read.
    ///
    /// It arms the debounce exactly like a packet that did: a lost packet is a lost
    /// *trigger*, and the answer to a trigger is to read the tree, which is the same
    /// answer whatever the bytes said. It has no direction, so it can neither open a turn
    /// nor close one — a burst it lands in the middle of is still that burst.
    pub(crate) fn observe_lost(&mut self, lost: Lost, at: Instant) {
        Counts::bump(&mut self.counts.packets);
        match lost {
            Lost::Oversized => Counts::bump(&mut self.counts.oversized),
            Lost::Overrun => Counts::bump(&mut self.counts.overrun),
        }
        self.debounce.observe(None, at);
    }

    /// Has the burst finished, so the tree is worth reading again?
    pub(crate) fn due(&self, now: Instant) -> bool {
        self.debounce.due(now)
    }

    /// When a bounded wait should give up and ask again.
    pub(crate) fn next_deadline(&self) -> Option<Instant> {
        self.debounce.next_deadline()
    }

    /// Read the node list and queue the difference from the last reading.
    ///
    /// # Errors
    ///
    /// Whatever [`Nodes::list`] failed with, with the burst left armed — see the trait's
    /// own note.
    pub(crate) fn rescan(&mut self) -> Result<()> {
        let now: BTreeSet<Utf8PathBuf> = self.nodes.list()?.into_iter().collect();
        Counts::bump(&mut self.counts.rescans);
        for gone in self.known.difference(&now) {
            self.queued
                .push_back(HotplugEvent::Removed { path: gone.clone() });
        }
        for fresh in now.difference(&self.known) {
            self.queued.push_back(HotplugEvent::Added {
                path: fresh.clone(),
            });
        }
        self.known = now;
        self.debounce.taken();
        Ok(())
    }

    /// The oldest event this tracker has decided on, if any.
    pub(crate) fn pop(&mut self) -> Option<HotplugEvent> {
        self.queued.pop_front()
    }

    /// What this tracker has seen, by name rather than through `Debug`.
    #[cfg(test)]
    pub(crate) fn counts(&self) -> Counts {
        self.counts
    }

    /// The node source, so a test can move the tree under a watch and count the readings.
    #[cfg(test)]
    pub(crate) fn nodes_mut(&mut self) -> &mut N {
        &mut self.nodes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The uevent fixtures. The first four are datagrams a real kernel broadcast during a
    // `uvcvideo` cycle on 2026-08-10; the `hostile-` ones are **synthetic by
    // construction**, each a named mutation of the first, because no kernel produces
    // them. `fixtures/README.md` carries the provenance and says which is which.
    /// A Chicony capture node appearing. 228 bytes, and note the trailing NUL.
    const ADD_VIDEO0: &[u8] = include_bytes!("../fixtures/uevent-add-video-node.bin");
    /// A Dell node leaving — the longest `DEVPATH` on this desk \[PF:19\].
    const REMOVE_VIDEO6: &[u8] = include_bytes!("../fixtures/uevent-remove-video-node.bin");
    /// A `usb` interface binding: another subsystem *and* another verb, at once.
    const BIND_USB: &[u8] = include_bytes!("../fixtures/uevent-bind-usb-interface.bin");
    /// The nearest neighbour: a `media` node from the same camera, in the same burst.
    const ADD_MEDIA0: &[u8] = include_bytes!("../fixtures/uevent-add-media-node.bin");
    /// One whole cycle, 56 length-framed datagrams — see [`framed`].
    const CYCLE_BURST: &[u8] = include_bytes!("../fixtures/uevent-cycle-burst.bin");

    /// Every NUL removed: no separators, and no terminator either.
    const NO_NUL: &[u8] = include_bytes!("../fixtures/uevent-hostile-no-nul.bin");
    /// The header line, cut mid-path, and nothing after it.
    const TRUNCATED_HEADER: &[u8] =
        include_bytes!("../fixtures/uevent-hostile-truncated-header.bin");
    /// `SUBSYSTEM=video4linux` with the `=` turned into a `_`.
    const KEY_WITHOUT_EQUALS: &[u8] =
        include_bytes!("../fixtures/uevent-hostile-key-without-equals.bin");
    /// Every number absurd, including a `SEQNUM` one past `u64::MAX`.
    const ABSURD_NUMBERS: &[u8] = include_bytes!("../fixtures/uevent-hostile-absurd-numbers.bin");
    /// A run of 64 NULs spliced between two fields, and 32 more after the last.
    const EMBEDDED_NULS: &[u8] = include_bytes!("../fixtures/uevent-hostile-embedded-nuls.bin");
    /// Four bytes no UTF-8 decoder accepts, inside a field this build never reads.
    const NOT_UTF8: &[u8] = include_bytes!("../fixtures/uevent-hostile-not-utf8.bin");
    /// 16 KiB — twice the receive buffer — of otherwise well-formed packet.
    const ENORMOUS: &[u8] = include_bytes!("../fixtures/uevent-hostile-enormous.bin");

    /// Split a length-framed fixture back into datagrams.
    ///
    /// The framing is **ours, not the kernel's**: a file has no datagram boundaries, and
    /// this module's header measured what a plain concatenation does — all 56 packets
    /// decode as one `module` event, which is safe and is not a per-packet reading. So the
    /// flood fixture carries each datagram behind its own little-endian `u32` length, and
    /// a test that drives it drives a *sequence*, which is the only shape that asserts
    /// what the socket actually delivers.
    fn framed(fixture: &[u8]) -> Vec<&[u8]> {
        let mut packets = Vec::new();
        let mut rest = fixture;
        while let Some((header, tail)) = rest.split_at_checked(4) {
            let len = u32::from_le_bytes(header.try_into().expect("four bytes"));
            let len = usize::try_from(len).expect("a frame length fits a usize");
            let (packet, next) = tail.split_at_checked(len).expect("a whole frame");
            packets.push(packet);
            rest = next;
        }
        assert!(rest.is_empty(), "{} trailing bytes", rest.len());
        packets
    }

    /// A packet in the kernel's own shape: a header line, then NUL-separated pairs.
    ///
    /// Kept beside the captured fixtures rather than replaced by them: a synthetic packet
    /// is how a *branch* nothing on this desk emits gets reached — no `video4linux`
    /// `change`, `bind` or `unbind` appeared in any of the three captured cycles \[PF:21\]
    /// — while only a captured one proves the shape is what this kernel writes.
    fn packet(action: &str, subsystem: &str, name: &str) -> Vec<u8> {
        let devpath = format!("/devices/pci0000:00/usb3/3-4/3-4:1.0/{subsystem}/{name}");
        let fields = [
            format!("ACTION={action}"),
            format!("DEVPATH={devpath}"),
            format!("SUBSYSTEM={subsystem}"),
            "MAJOR=81".to_owned(),
            "MINOR=0".to_owned(),
            format!("DEVNAME={name}"),
            "SEQNUM=12345".to_owned(),
        ];
        let mut bytes = format!("{action}@{devpath}").into_bytes();
        for field in fields {
            bytes.push(0);
            bytes.extend_from_slice(field.as_bytes());
        }
        bytes
    }

    /// A packet whose `DEVPATH` and `SUBSYSTEM` are chosen apart, which the composed
    /// [`packet`] above cannot do.
    ///
    /// Synthetic and honestly so: no `uvcvideo` cycle on this desk produces a non-camera
    /// device hanging below a `video4linux` node, so there is no capture to use — and the
    /// rule the discrimination pins is the one [`VIDEO_SUBSYSTEM`]'s doc exists to state.
    fn packet_with(action: &str, subsystem: &str, devpath: &str) -> Vec<u8> {
        let fields = [
            format!("ACTION={action}"),
            format!("DEVPATH={devpath}"),
            format!("SUBSYSTEM={subsystem}"),
            "SEQNUM=12345".to_owned(),
        ];
        let mut bytes = format!("{action}@{devpath}").into_bytes();
        for field in fields {
            bytes.push(0);
            bytes.extend_from_slice(field.as_bytes());
        }
        bytes
    }

    #[test]
    fn a_video_node_appearing_and_leaving_are_the_two_triggers_and_they_are_distinguished() {
        assert_eq!(
            trigger(&packet("add", "video4linux", "video0")),
            Trigger::NodeChanged {
                action: NodeAction::Added,
            }
        );
        assert_eq!(
            trigger(&packet("remove", "video4linux", "video0")),
            Trigger::NodeChanged {
                action: NodeAction::Removed,
            }
        );
    }

    #[test]
    fn a_neighbours_packet_under_a_video4linux_path_is_still_not_ours() {
        // The rule `VIDEO_SUBSYSTEM`'s doc states, and the only packet in this suite that
        // can state it: `SUBSYSTEM` says one thing and `DEVPATH` contains the other. Every
        // other packet here — synthetic and captured alike — has the two agreeing, so
        // replacing the `SUBSYSTEM` comparison with `devpath.contains("video4linux")` (the
        // exact "simplification" the doc forbids) passed all twenty-two of them. It does
        // not pass this one.
        //
        // The mirror case is asserted too, because "the filter reads SUBSYSTEM" needs both
        // halves: a `video4linux` packet whose path does *not* contain the word is still
        // ours, which a substring filter would drop.
        assert_eq!(
            trigger(&packet_with(
                "add",
                "input",
                "/devices/pci0000:00/usb3/3-4/3-4:1.0/video4linux/video0/input1",
            )),
            Trigger::NotOurs(NotOurs::OtherSubsystem),
            "the subsystem key says `input`, whatever the path spells"
        );
        assert_eq!(
            trigger(&packet_with(
                "remove",
                "video4linux",
                "/devices/virtual/vout/vout0"
            )),
            Trigger::NodeChanged {
                action: NodeAction::Removed,
            },
            "the subsystem key says `video4linux`, whatever the path spells"
        );
    }

    #[test]
    fn the_filter_is_on_the_subsystem_key_and_a_driver_cycles_other_verbs_are_dropped() {
        // The group-1 broadcast is every subsystem on the machine, and one `uvcvideo`
        // cycle emits `bind`/`unbind`/`change` alongside the ten adds and ten removes
        // this host's four cameras produce [PF:19]. Each of those is *not news*, and
        // none of them is an error.
        assert_eq!(
            trigger(&packet("add", "tty", "ttyS6")),
            Trigger::NotOurs(NotOurs::OtherSubsystem)
        );
        assert_eq!(
            trigger(&packet("add", "usb", "3-4:1.0")),
            Trigger::NotOurs(NotOurs::OtherSubsystem)
        );
        for verb in ["change", "bind", "unbind", "move", "online", "offline"] {
            assert_eq!(
                trigger(&packet(verb, "video4linux", "video0")),
                Trigger::NotOurs(NotOurs::OtherAction),
                "{verb} is not a node appearing"
            );
        }
    }

    #[test]
    fn a_verb_this_build_does_not_model_is_carried_rather_than_called_malformed() {
        // AGENTS rule 6, and the trap in `kobject-uevent`'s error vocabulary: an
        // unknown `ACTION` is its *first* refusal, ahead of the subsystem it would have
        // been filtered by. A build that read that as "malformed" would count a future
        // kernel's new verb on a `tty` as a corrupt packet.
        assert_eq!(
            trigger(&packet("frobnicate", "video4linux", "video0")),
            Trigger::NotOurs(NotOurs::UnmodelledAction)
        );
        assert_eq!(
            trigger(&packet("frobnicate", "tty", "ttyS6")),
            Trigger::NotOurs(NotOurs::UnmodelledAction)
        );
    }

    #[test]
    fn every_malformed_shape_is_a_value_rather_than_a_panic_and_they_are_told_apart() {
        // The hostile-bytes claim in miniature; the committed fixtures widen the
        // population, they do not change what is asserted. "Truncated" and "unreadable
        // field" must not be spelled the same way (AGENTS rule 6).
        let whole = packet("add", "video4linux", "video0");

        assert_eq!(
            trigger(&[]),
            Trigger::Unreadable(Unreadable::Missing("ACTION"))
        );

        // Every prefix of a real packet, byte by byte. Two claims, and the second is the
        // one worth reading: a prefix is always a *value*, and a prefix that still
        // decodes describes the same node the whole packet does.
        //
        // What this walk deliberately does **not** claim is that truncation is
        // detectable. It is not, and that is a property of the wire format rather than
        // of this parser: a uevent packet carries no length and no terminator, and
        // `SEQNUM` is last, so the first 197 bytes of the 204 below are a well-formed
        // packet about sequence 1. The guard for that lives one layer down —
        // `sys::uevent` reads with `MSG_TRUNC` and drops a datagram that did not fit
        // *whole* — and a netlink datagram is delivered atomically or not at all, so a
        // prefix is a fixture's shape and never a kernel's. Recorded here rather than
        // papered over with an assertion that would have to be wrong.
        //
        // A prefix also reaches `NotOurs`, and that is the same fact wearing a second
        // hat: `ACTION=add` cut short is `ACTION=a`, which is a verb this build does not
        // model. Truncation and a future kernel are genuinely indistinguishable in this
        // format, both are dropped, and only the counter differs.
        let mut refused = 0_u32;
        let mut decoded = 0_u32;
        for cut in 0..whole.len() {
            match trigger(whole.get(..cut).expect("a prefix of the packet")) {
                Trigger::Unreadable(_) | Trigger::NotOurs(_) => refused += 1,
                Trigger::NodeChanged { action } => {
                    decoded += 1;
                    assert_eq!(action, NodeAction::Added, "at {cut} bytes");
                }
            }
        }
        assert!(
            refused > 0 && decoded > 0,
            "{refused} dropped, {decoded} decoded"
        );

        // …while the whole packet decodes, which is what keeps the loop above from
        // passing for the wrong reason.
        assert!(matches!(
            trigger(&whole),
            Trigger::NodeChanged {
                action: NodeAction::Added,
                ..
            }
        ));

        // One byte spliced into the middle refuses the whole packet, per the header's
        // recorded limitation.
        let mut spliced = whole.clone();
        spliced.insert(1, 0xFF);
        assert_eq!(trigger(&spliced), Trigger::Unreadable(Unreadable::NotUtf8));

        // A field that is present and unreadable is a different answer from one that is
        // absent.
        let mut renumbered = whole.clone();
        let text = String::from_utf8(std::mem::take(&mut renumbered)).expect("ascii");
        assert_eq!(
            trigger(text.replace("SEQNUM=12345", "SEQNUM=tomorrow").as_bytes()),
            Trigger::Unreadable(Unreadable::Malformed("SEQNUM"))
        );
        assert_eq!(
            trigger(
                text.replace("SUBSYSTEM=video4linux", "SUB=video4linux")
                    .as_bytes()
            ),
            Trigger::Unreadable(Unreadable::Missing("SUBSYSTEM"))
        );
        assert_eq!(
            trigger(text.replace("DEVPATH=", "DEV_PATH=").as_bytes()),
            Trigger::Unreadable(Unreadable::Missing("DEVPATH"))
        );
        // All four required keys, each named by its own absence — including `SEQNUM`,
        // whose "missing" arm is otherwise reachable only through the prefix walk above,
        // which asserts the class and not the key.
        assert_eq!(
            trigger(text.replace("SEQNUM=12345", "SEQ=12345").as_bytes()),
            Trigger::Unreadable(Unreadable::Missing("SEQNUM"))
        );
        assert_eq!(
            trigger(text.replace("ACTION=add", "VERB=add").as_bytes()),
            Trigger::Unreadable(Unreadable::Missing("ACTION"))
        );
    }

    #[test]
    fn a_hostile_devpath_never_becomes_a_slash_dev_path_because_nothing_here_makes_one() {
        // The header's claim. A packet naming `DEVNAME=../../etc/shadow` still yields
        // nothing but a direction — which is now a fact about `Trigger`'s shape rather
        // than an assertion about a field's contents, and that is the stronger statement:
        // there is no field for the text to be carried in, so no later edit can carry it
        // without changing the type in a diff somebody has to read.
        //
        // The equality below is the whole assertion. It compares against a value built
        // from no packet at all, so it goes red the moment `NodeChanged` grows a payload
        // that a packet fills in.
        let whole = String::from_utf8(packet("add", "video4linux", "video0")).expect("ascii");
        let hostile = whole
            .replace("DEVNAME=video0", "DEVNAME=../../../etc/shadow")
            .replace("DEVPATH=", "DEVPATH=../..")
            .replace("MINOR=0", "MINOR=999999999999999999999");
        assert_eq!(
            trigger(hostile.as_bytes()),
            Trigger::NodeChanged {
                action: NodeAction::Added,
            },
            "the packet is still a well-formed video4linux add, and still says nothing else"
        );
        // …and the benign packet it was made from produces the identical value, which is
        // the same claim from the other side: the two packets differ in every field a
        // path could be built from and the trigger cannot tell them apart.
        assert_eq!(
            trigger(whole.as_bytes()),
            trigger(hostile.as_bytes()),
            "a hostile field changed what came out of the parse"
        );
    }

    #[test]
    fn the_packets_this_kernel_actually_broadcasts_decode_the_way_the_synthetic_ones_do() {
        // The synthetic packets above prove the branches; these prove the branches are
        // reachable from bytes a kernel wrote. Two differences from `kobject-uevent`'s own
        // committed fixture are load-bearing and neither was predicted: a real packet ends
        // in a **trailing NUL** (harmless — a segment with no `=` is skipped), and
        // `DEVNAME` is a bare name with no `/dev/` prefix.
        assert_eq!(ADD_VIDEO0.last(), Some(&0), "the trailing NUL is the shape");
        assert!(
            std::str::from_utf8(ADD_VIDEO0)
                .expect("ascii")
                .contains("\0DEVNAME=video0\0"),
            "a bare name, never a path"
        );

        assert_eq!(
            trigger(ADD_VIDEO0),
            Trigger::NodeChanged {
                action: NodeAction::Added,
            }
        );
        assert_eq!(
            trigger(REMOVE_VIDEO6),
            Trigger::NodeChanged {
                action: NodeAction::Removed,
            }
        );
        // The two nearest neighbours in the same burst — a `usb` interface binding and
        // the *same camera's* `media` node. Both are dropped, and neither of them is the
        // packet that discriminates the subsystem filter from a path substring: their
        // `DEVPATH`s are `.../3-4:1.1` and `.../3-4:1.0/media0`, so neither contains the
        // word `video4linux` and a substring filter drops them for the wrong reason and
        // gets the right answer. The discriminating packet has to be synthetic, and it is
        // `a_neighbours_packet_under_a_video4linux_path_is_still_not_ours`.
        assert_eq!(trigger(BIND_USB), Trigger::NotOurs(NotOurs::OtherSubsystem));
        assert_eq!(
            trigger(ADD_MEDIA0),
            Trigger::NotOurs(NotOurs::OtherSubsystem)
        );
        for (name, fixture) in [("bind usb", BIND_USB), ("add media", ADD_MEDIA0)] {
            assert!(
                !fixture
                    .windows(VIDEO_SUBSYSTEM.len())
                    .any(|window| window == VIDEO_SUBSYSTEM.as_bytes()),
                "{name}: this fixture does contain the word, so it would discriminate \
                 after all and the comment above is wrong"
            );
        }
    }

    #[test]
    fn one_whole_driver_cycle_is_twenty_node_changes_among_fifty_six_packets() {
        // PF:19's arithmetic generalised, from the wire rather than from prose: four
        // cameras, ten `video4linux` nodes, and one `uvcvideo` cycle broadcasting 56
        // packets of which 36 are somebody else's. That ratio is why the subsystem filter
        // and the debounce are both load-bearing rather than tidy.
        let packets = framed(CYCLE_BURST);
        assert_eq!(packets.len(), 56);

        let mut added = 0;
        let mut removed = 0;
        let mut not_ours = 0;
        for packet in &packets {
            match trigger(packet) {
                Trigger::NodeChanged {
                    action: NodeAction::Added,
                    ..
                } => added += 1,
                Trigger::NodeChanged {
                    action: NodeAction::Removed,
                    ..
                } => removed += 1,
                Trigger::NotOurs(_) => not_ours += 1,
                Trigger::Unreadable(reason) => {
                    panic!("a real kernel packet was unreadable: {reason:?}")
                }
            }
        }
        assert_eq!((added, removed, not_ours), (10, 10, 36));

        // …and the concatenation is *not* the same thing, which is why the fixture is
        // framed. Spliced into one buffer the whole burst decodes as a single event about
        // the last packet's subsystem — safe, a value, and the wrong question. Netlink is
        // `SOCK_DGRAM` so this cannot arise; asserted so that a future "simplification" of
        // the fixture format has to argue with a red test.
        let spliced: Vec<u8> = packets.concat();
        assert_eq!(
            trigger(&spliced),
            Trigger::NotOurs(NotOurs::OtherSubsystem),
            "56 packets in one buffer are one packet, not 56"
        );
    }

    #[test]
    fn every_hostile_fixture_is_a_typed_refusal_or_a_dropped_packet_and_never_a_panic() {
        // Rubric B10, as a table: a packet off a kernel socket is attacker-shaped input.
        // Each row is a committed byte fixture and the answer it must produce — and the
        // answers are deliberately *different* from one another, because "malformed" and
        // "a shape this build cannot read" must not be spelled the same way (AGENTS rule
        // 6).
        for (name, fixture, expected) in [
            (
                "no NUL anywhere: no separators and no terminator",
                NO_NUL,
                Trigger::Unreadable(Unreadable::Missing("ACTION")),
            ),
            (
                "the header line, cut mid-path",
                TRUNCATED_HEADER,
                Trigger::Unreadable(Unreadable::Missing("ACTION")),
            ),
            (
                "a key with no `=`, so the segment is skipped and its key is absent",
                KEY_WITHOUT_EQUALS,
                Trigger::Unreadable(Unreadable::Missing("SUBSYSTEM")),
            ),
            (
                "a declared number past `u64::MAX`",
                ABSURD_NUMBERS,
                Trigger::Unreadable(Unreadable::Malformed("SEQNUM")),
            ),
            (
                "four bytes no UTF-8 decoder accepts",
                NOT_UTF8,
                Trigger::Unreadable(Unreadable::NotUtf8),
            ),
        ] {
            assert_eq!(trigger(fixture), expected, "{name}");
        }

        // Two hostile shapes are *not* refusals, and saying so is the point: a run of
        // empty segments is skipped rather than mistaken for a field, and an enormous
        // packet is a large well-formed packet rather than a reason to fall over. The
        // *socket* is what refuses the second one, by size, before this ever sees it —
        // `sys::uevent` reads with `MSG_TRUNC` and drops a datagram that did not fit whole.
        assert!(
            matches!(
                trigger(EMBEDDED_NULS),
                Trigger::NodeChanged {
                    action: NodeAction::Added,
                    ..
                }
            ),
            "96 embedded NULs are 96 empty segments, not a corrupt packet"
        );
        assert_eq!(ENORMOUS.len(), 2 * limits::UEVENT_PACKET_BYTES);
        assert!(
            matches!(
                trigger(ENORMOUS),
                Trigger::NodeChanged {
                    action: NodeAction::Added,
                    ..
                }
            ),
            "a 16 KiB packet is a packet"
        );

        // The empty datagram, which has no fixture because a zero-byte file says nothing a
        // reader could check.
        assert_eq!(
            trigger(&[]),
            Trigger::Unreadable(Unreadable::Missing("ACTION"))
        );
    }

    /// A 64-bit xorshift, so the walk below is a *fixed* sequence rather than a different
    /// test on every run.
    ///
    /// Hand-rolled rather than a dependency: this needs eight lines of arithmetic and no
    /// distributional quality at all, and a `rand` edge in a product crate's dev-dependency
    /// graph would be a supply-chain decision taken to save eight lines.
    struct Xorshift(u64);

    impl Xorshift {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }

        fn below(&mut self, bound: usize) -> usize {
            usize::try_from(self.next() % u64::try_from(bound).expect("a small bound"))
                .expect("smaller than the bound")
        }
    }

    #[test]
    fn no_byte_string_at_all_makes_the_parser_panic() {
        // The claim the fixture table cannot make: `trigger` is total. Rubric B10 says a
        // packet is attacker-shaped input, and a table of eight shapes is eight shapes.
        //
        // Deterministic by construction — one fixed seed, no clock, no entropy — so a
        // failure is reproducible from the seed printed in this line rather than from a
        // lucky rerun. The corpus is the real captured packets, mutated the four ways a
        // wire corrupts one: a flipped byte, a spliced byte, a cut, and a doubling.
        let mut rng = Xorshift(0x5715_1105_0BEE_F00D);
        let corpus: Vec<&[u8]> = framed(CYCLE_BURST);

        let mut node_changes = 0_u32;
        let mut not_ours = 0_u32;
        let mut unreadable = 0_u32;
        for round in 0..20_000_u32 {
            let mut bytes = if round % 4 == 0 {
                // A purely random buffer, up to twice the receive buffer: nothing about
                // it is packet-shaped, which is the case a parser written against a
                // grammar it trusts gets wrong.
                let len = rng.below(2 * limits::UEVENT_PACKET_BYTES);
                (0..len)
                    .map(|_| u8::try_from(rng.next() & 0xff).expect("masked to a byte"))
                    .collect::<Vec<u8>>()
            } else {
                corpus[rng.below(corpus.len())].to_vec()
            };
            match round % 4 {
                1 if !bytes.is_empty() => {
                    let at = rng.below(bytes.len());
                    bytes[at] ^= u8::try_from(rng.next() & 0xff).expect("masked to a byte");
                }
                2 if !bytes.is_empty() => {
                    let at = rng.below(bytes.len());
                    bytes.insert(at, u8::try_from(rng.next() & 0xff).expect("masked"));
                }
                3 if !bytes.is_empty() => {
                    let cut = rng.below(bytes.len());
                    bytes.truncate(cut);
                    let doubled = bytes.clone();
                    bytes.extend_from_slice(&doubled);
                }
                _ => {}
            }

            match trigger(&bytes) {
                // The one invariant worth asserting beyond "it returned": a packet can
                // only be *ours* if it says so. A parser that defaulted the subsystem, or
                // that read it from the header line, would fail here rather than merely
                // surviving.
                Trigger::NodeChanged { .. } => {
                    node_changes += 1;
                    assert!(
                        bytes
                            .windows(VIDEO_SUBSYSTEM.len())
                            .any(|window| window == VIDEO_SUBSYSTEM.as_bytes()),
                        "round {round}: a packet that never says `video4linux` was ours"
                    );
                }
                Trigger::NotOurs(_) => not_ours += 1,
                Trigger::Unreadable(_) => unreadable += 1,
            }
        }

        // Every outcome was reached, so the loop is not passing because it only ever hit
        // one arm — the way a fuzz loop most often goes quietly useless.
        assert!(
            node_changes > 0 && not_ours > 0 && unreadable > 0,
            "{node_changes} ours, {not_ours} theirs, {unreadable} unreadable"
        );
    }

    #[test]
    fn a_burst_is_finished_when_the_socket_goes_quiet_or_when_the_ceiling_arrives() {
        // The fold, on instants the test invents. `t0` is read once and nothing ever waits
        // on it: no sleep, and the deadline arithmetic is the whole of what is under test
        // (AGENTS: settle logic runs on a stepped clock).
        let quiet = Duration::from_millis(50);
        let ceiling = Duration::from_millis(500);
        let t0 = Instant::now();

        let mut debounce = Debounce::new(quiet, ceiling);
        assert!(!debounce.due(t0), "an unarmed burst is never due");
        assert_eq!(debounce.next_deadline(), None);

        // A 95 ms burst of twenty triggers, all going the same way.
        for step in 0..20 {
            debounce.observe(
                Some(NodeAction::Removed),
                t0 + Duration::from_millis(step * 5),
            );
        }
        let last = t0 + Duration::from_millis(95);
        assert!(!debounce.due(last + quiet - Duration::from_millis(1)));
        assert!(debounce.due(last + quiet));
        assert_eq!(debounce.next_deadline(), Some(last + quiet));

        // Taken, and the next trigger opens a new burst with its own full window.
        debounce.taken();
        assert!(
            !debounce.due(last + ceiling * 10),
            "a taken burst stays taken"
        );

        // The ceiling arm: a stream that never goes quiet still fires, which is the
        // difference between a debounce and a hang wearing one as a costume.
        let mut stuck = Debounce::new(quiet, ceiling);
        for step in 0..10_000 {
            stuck.observe(Some(NodeAction::Added), t0 + Duration::from_millis(step));
        }
        assert!(stuck.due(t0 + ceiling));
        assert_eq!(stuck.next_deadline(), Some(t0 + ceiling));
        // …and it is the *ceiling* that fired, not the quiet window: the last trigger was
        // 9999 ms in, so quiet alone would still be waiting.
        let mut patient = Debounce::new(quiet, Duration::from_secs(3_600));
        for step in 0..10_000 {
            patient.observe(Some(NodeAction::Added), t0 + Duration::from_millis(step));
        }
        assert!(!patient.due(t0 + ceiling));
    }

    #[test]
    fn a_trigger_that_reverses_the_burst_ends_it_whatever_the_clock_says() {
        // The turn rule. Measured on this host \[PF:21\], the largest gap *inside* the
        // remove phase of a driver cycle was 93.6 ms and the gap between the last remove
        // and the first add was 98.5 ms — five milliseconds apart. No quiet window
        // separates those, so the window does not try to; the direction change does.
        let quiet = Duration::from_millis(250);
        let t0 = Instant::now();

        let mut turning = Debounce::new(quiet, Duration::from_secs(60));
        for step in 0..10 {
            turning.observe(Some(NodeAction::Removed), t0 + Duration::from_millis(step));
        }
        let turn = t0 + Duration::from_millis(11);
        assert!(
            !turning.due(turn),
            "still one burst, still inside the window"
        );
        turning.observe(Some(NodeAction::Added), turn);
        assert!(
            turning.due(turn),
            "the burst changed direction and did not end"
        );

        // The inverse, and the reason the assertion above can go red: the same instants
        // with the same direction throughout are *not* due, so it is the turn that fired
        // and not the clock.
        let mut steady = Debounce::new(quiet, Duration::from_secs(60));
        for step in 0..=10 {
            steady.observe(Some(NodeAction::Removed), t0 + Duration::from_millis(step));
        }
        steady.observe(Some(NodeAction::Removed), turn);
        assert!(!steady.due(turn));

        // A trigger with no direction — a packet whose bytes never got read — can neither
        // open a turn nor close one. It arms the burst and nothing else, because "we lost
        // a packet" says nothing about which way the tree moved.
        let mut lost = Debounce::new(quiet, Duration::from_secs(60));
        lost.observe(Some(NodeAction::Removed), t0);
        lost.observe(None, turn);
        assert!(
            !lost.due(turn),
            "a lost packet turned a burst it cannot see"
        );
        assert!(lost.due(turn + quiet), "…but it still armed one");
    }

    #[test]
    fn out_of_order_instants_cannot_make_a_finished_burst_unfinished() {
        // A caller handing instants backwards is a caller with a bug, and the fold's
        // answer to one is to stay monotone rather than to move its own deadline
        // backwards — which would be a burst that never finishes.
        let quiet = Duration::from_millis(50);
        let t0 = Instant::now();
        let mut debounce = Debounce::new(quiet, Duration::from_secs(60));
        debounce.observe(Some(NodeAction::Added), t0 + Duration::from_millis(100));
        debounce.observe(Some(NodeAction::Added), t0);
        assert_eq!(
            debounce.next_deadline(),
            Some(t0 + Duration::from_millis(100) + quiet)
        );
    }

    #[test]
    fn a_lost_packet_costs_a_trigger_and_never_a_change() {
        // `Received::{Oversized, Overrun}` are values rather than errors, and this is why
        // that is affordable: the watch's answer to any trigger is to read the tree, so a
        // packet we could not read costs the same as one we could. The counters keep them
        // apart because an overrun is the one that says *this machine outran us*.
        let t0 = Instant::now();
        let mut tracker = Tracker::primed(
            StubNodes::with(&["/dev/video0"]),
            Debounce::new(Duration::from_millis(10), Duration::from_secs(60)),
        )
        .expect("prime");

        tracker.observe_lost(Lost::Overrun, t0);
        tracker.observe_lost(Lost::Oversized, t0);
        assert!(tracker.due(t0 + Duration::from_millis(10)));

        tracker.nodes_mut().present.clear();
        tracker.rescan().expect("rescan");
        assert_eq!(
            tracker.pop(),
            Some(HotplugEvent::Removed {
                path: "/dev/video0".into()
            }),
            "a lost packet lost the change it was about"
        );

        let counts = tracker.counts();
        assert_eq!(
            (
                counts.packets,
                counts.overrun,
                counts.oversized,
                counts.rescans
            ),
            (2, 1, 1, 1),
            "{counts:?}"
        );
        assert_eq!(
            (counts.node_changes, counts.not_ours, counts.unreadable),
            (0, 0, 0)
        );
    }

    #[test]
    fn a_packet_this_build_could_not_read_still_costs_a_trigger_rather_than_the_change() {
        // The header's affordability argument, driven rather than asserted: the refusals
        // in `trigger` are liberal on purpose — one non-UTF-8 byte anywhere in the buffer
        // refuses the whole packet, including a byte in a field this build never reads —
        // and what makes that affordable is that a packet we could not read is answered
        // the same way as one we could: by reading the tree.
        //
        // Written as the thing that goes wrong. `NOT_UTF8` is a `video4linux` `add`
        // packet with four bad bytes spliced in [see fixtures/README.md]; the node it
        // names leaves the tree; and the subscriber must be told. Before the arming was
        // symmetrical this returned `None` — the counter went up, the debounce stayed
        // unarmed, `due` stayed false, and the node stayed listed until something
        // unrelated happened. On a single-node camera, or on the last packet of a burst,
        // "something unrelated" never comes.
        let t0 = Instant::now();
        let mut tracker = Tracker::primed(
            StubNodes::with(&["/dev/video0"]),
            Debounce::new(Duration::from_millis(10), Duration::from_secs(60)),
        )
        .expect("prime");

        assert_eq!(
            tracker.observe_packet(NOT_UTF8, t0),
            Trigger::Unreadable(Unreadable::NotUtf8),
            "the fixture this test is about stopped being unreadable"
        );
        assert!(
            tracker.due(t0 + Duration::from_millis(10)),
            "an unreadable packet did not arm the burst, so nothing will re-read the tree"
        );

        tracker.nodes_mut().present.clear();
        tracker.rescan().expect("rescan");
        assert_eq!(
            tracker.pop(),
            Some(HotplugEvent::Removed {
                path: "/dev/video0".into()
            }),
            "the change the unreadable packet announced was lost"
        );

        // …and the arm that must *not* fire, which is what keeps the one above from being
        // "arm on everything": a well-formed packet about another subsystem is a packet
        // this build positively knows is not news, and thirty-six of fifty-six in a
        // measured cycle are exactly that [PF:21]. Arming on those would make a busy
        // machine a rescan loop.
        let mut quiet = Tracker::primed(
            StubNodes::with(&["/dev/video0"]),
            Debounce::new(Duration::from_millis(10), Duration::from_secs(60)),
        )
        .expect("prime");
        assert_eq!(
            quiet.observe_packet(BIND_USB, t0),
            Trigger::NotOurs(NotOurs::OtherSubsystem)
        );
        assert!(
            !quiet.due(t0 + Duration::from_secs(3_600)),
            "a neighbour's packet armed a reading of our tree"
        );

        // The third arm: an `ACTION` nobody models *does* arm it, because the parser
        // refuses on the verb before it reads `SUBSYSTEM`, so this build cannot say the
        // packet was not about a camera.
        assert_eq!(
            quiet.observe_packet(&packet("frobnicate", "tty", "ttyS6"), t0),
            Trigger::NotOurs(NotOurs::UnmodelledAction)
        );
        assert!(quiet.due(t0 + Duration::from_millis(10)));
    }

    #[test]
    fn a_failed_reading_does_not_spend_the_trigger_that_asked_for_it() {
        // The other direction of the same property. `/sys/class/video4linux` becoming
        // unreadable is a real error and the caller is told so — but the burst stays
        // armed, so the change that provoked it is retried rather than lost. A `taken()`
        // before the read would have been a silent hole exactly the size of one transient
        // failure.
        let t0 = Instant::now();
        let mut tracker = Tracker::primed(
            StubNodes::with(&["/dev/video0"]),
            Debounce::new(Duration::ZERO, Duration::from_secs(60)),
        )
        .expect("prime");
        assert!(
            matches!(
                tracker.observe_packet(REMOVE_VIDEO6, t0),
                Trigger::NodeChanged {
                    action: NodeAction::Removed,
                    ..
                }
            ),
            "the fixture that arms the burst is the one this test names"
        );
        assert!(tracker.due(t0));

        tracker.nodes_mut().unreadable = true;
        assert!(tracker.rescan().is_err(), "a broken sysfs is an error");
        assert!(tracker.due(t0), "the failed reading spent the trigger");

        tracker.nodes_mut().unreadable = false;
        tracker.nodes_mut().present.clear();
        tracker.rescan().expect("rescan");
        assert!(matches!(tracker.pop(), Some(HotplugEvent::Removed { .. })));
        assert!(!tracker.due(t0), "a reading that worked did spend it");
    }

    /// A node list a test states outright, with a switch for "the tree cannot be read".
    #[derive(Debug)]
    struct StubNodes {
        present: Vec<Utf8PathBuf>,
        unreadable: bool,
    }

    impl StubNodes {
        fn with(paths: &[&str]) -> StubNodes {
            StubNodes {
                present: paths.iter().map(|path| Utf8PathBuf::from(*path)).collect(),
                unreadable: false,
            }
        }
    }

    impl Nodes for StubNodes {
        fn list(&mut self) -> Result<Vec<Utf8PathBuf>> {
            if self.unreadable {
                return Err(schema::error::Error::StorageIo {
                    path: "/sys/class/video4linux".into(),
                    errno: None,
                    message: "the stub was told the tree cannot be read".to_owned(),
                });
            }
            Ok(self.present.clone())
        }
    }
}
