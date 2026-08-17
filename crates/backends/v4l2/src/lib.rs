//! The V4L2 backend.
//!
//! This is the one crate in the workspace without `#![forbid(unsafe_code)]`: talking to
//! the kernel means ioctls and mmap. The token `unsafe` is confined to `src/sys/` by
//! `scripts/gates/unsafe-scope.sh`, which derives the allowed path from the tree.
//!
//! ## What has landed, and what has not
//!
//! P1 landed the **read path** (docs/7): enumeration, the control model, and the format
//! tree. P2 added the **write and capture paths** — `set` with the read-back D3 requires,
//! and mmap streaming with the format negotiation D5 requires. P4d landed the last one,
//! **hotplug**: `CameraBackend::watch` is a real uevent netlink socket, so every method of
//! T1 and T2 now answers from a device rather than from a schedule. That is what retires
//! note N6's list — the surface it existed to enumerate is empty, and the D13 variant it
//! named goes with it.
//!
//! ## The layering
//!
//! | Module | Owns |
//! |---|---|
//! | `sys` | ioctls, mmap, the bounded wait, `kill(2)`, the uevent netlink socket, and the pure byte-to-schema decoding Miri executes |
//! | `sysfs` | the node list and the bus-interface topology, read without udev |
//! | `enumerate` | the pure grouping rule: nodes to cameras \[PF:7, PF:13\] |
//! | `hotplug` | what one uevent packet is worth, and when a burst of them has finished — folds over values |
//! | `watch` | the blocking loop that joins those folds to the socket, and `CameraBackend::watch` |
//! | [`holders`] | who has a node open, and asking one of them to let go (design §5) |
//!
//! The hotplug edge is in three pieces for the same reason `decode` is split from
//! `ioctl`: `sys::uevent` makes the syscalls, `hotplug` decides what the bytes and the
//! clock mean, and `watch` is the thirty lines that need both. The middle piece is the one
//! a fixture can drive and the mutation floor can examine, so it is the one that is not
//! under `src/sys/`.
//!
//! [`holders`] is the one module here that is **public and not about V4L2**: it is a `/proc`
//! walk plus a `SIGTERM`, and it lives in this crate because a [`schema::Error::Busy`] refusal
//! has to name the process and because this is the only crate that may say `unsafe`.
//! `webcam-handler-daemon` reaches it by name to route `terminate_holder` (design D10, note
//! N48); nothing else outside this crate does.
//!
//! Nothing above `src/sys/` names a kernel type; `scripts/gates/dependency-walls.sh` holds
//! that line for the rest of the workspace by refusing `v4l::` outside this crate.
// Kernel-shaped integers are converted with `try_from`, never `as` (rubric B10).
#![deny(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap
)]
// docs/9's "device/request-driven paths" lint set. Every path in this crate answers a
// request or reads a device, so the whole crate is inside it.
#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::as_conversions
    )
)]

mod enumerate;
pub mod holders;
mod hotplug;
mod sys;
mod sysfs;
mod watch;

use std::collections::BTreeMap;
use std::time::Instant;

use camino::Utf8Path;
use schema::backend::{BackendKind, Camera, CameraBackend, HotplugWatch};
use schema::camera::{CameraId, CameraInfo, FormatInfo, FrameInterval, FrameSizeInfo, PixelFormat};
use schema::capture::{Frame, NegotiatedStream, StreamRequest};
use schema::control::{
    Applied, ControlDesc, ControlId, ControlType, ControlValue, KnownFlag, Unverifiable,
    WriteWarning,
};
use schema::error::{Error, Result};
use schema::limits;
use schema::report::{HintKind, ListHint};

use enumerate::ProbedNode;
use sys::{Fd, ioctl};

/// The ioctl a control write goes through, for error messages that name it.
const SET_CTRL_OP: &str = "VIDIOC_S_EXT_CTRLS";

/// Real cameras, through V4L2.
#[derive(Debug, Default)]
pub struct V4l2Backend {
    /// The reading of the node list the last [`V4l2Backend::enumerate`] answered from,
    /// waiting for the [`V4l2Backend::diagnose`] that explains it — **stamped with the
    /// thread that took it**.
    ///
    /// T1 has two methods here because N7 argued it should: "the cameras" and "why there
    /// might be fewer than you expect" are two facts, and folding the second into
    /// `enumerate`'s return type would put a field almost every caller ignores in the
    /// signature every backend implements. What N7 did *not* say, and what this field is,
    /// is that they are two halves of **one answer**. `probe_nodes` computes both on every
    /// pass; until 2026-08-16 `enumerate` threw the failures away and `diagnose` read the
    /// machine a second time, so the hint explaining a dropped camera could describe a
    /// different moment than the listing it explained (note **N193**).
    ///
    /// **The thread id is the pass's identity, and without one this was a mailbox.** One
    /// `Arc<V4l2Backend>` serves a whole daemon, and six paths across three thread
    /// families reach `enumerate` — the camera actors through `open`, the tokio blocking
    /// pool through `Wchd::resolve` on *every* camera-naming RPC, the preview loop, the
    /// CLI. A pass carrying no identity could be taken by a `diagnose` belonging to a
    /// different call, which is the staleness N193 was raised to fix arriving through a
    /// different door (note **N198**). `engine::resolve::list` is the one assembler and it
    /// calls both in order **on one thread**, so keying on the thread makes the pairing
    /// exact; a `diagnose` from anywhere else finds nothing to take and reads the machine,
    /// which is the answer it would have got before any of this existed.
    last_probe: std::sync::Mutex<Option<(std::thread::ThreadId, Probe)>>,
}

impl V4l2Backend {
    /// A backend reading this machine's devices.
    #[must_use]
    pub fn new() -> V4l2Backend {
        V4l2Backend {
            last_probe: std::sync::Mutex::new(None),
        }
    }

    /// The listing this pass describes, keeping the pass for the `diagnose` that explains
    /// it.
    ///
    /// The grouping and the remembering are one function because the link between them is
    /// the whole of note N193's repair and there was nothing that could go red on it: the
    /// only test drove `remember` directly, so deleting the call from `enumerate` moved no
    /// assertion (note **N198**). A test can drive this with a pass it built, which is what
    /// `a_listing_leaves_the_pass_that_produced_it_for_the_diagnosis` does.
    fn listing(&self, probe: Probe) -> Vec<CameraInfo> {
        let cameras = enumerate::group(&probe.probed);
        self.remember(probe);
        cameras
    }

    /// Keep one pass's failures for the `diagnose` that will ask about them.
    fn remember(&self, probe: Probe) {
        *self
            .last_probe
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) =
            Some((std::thread::current().id(), probe));
    }

    /// The remembered pass, **spent**, and only if this thread is the one that took it.
    ///
    /// A `diagnose` with nothing to take reads the machine itself, which is what a caller
    /// asking for a diagnosis without a listing wants — and it is the only path left that
    /// probes twice, for a pairing no caller in this workspace makes.
    ///
    /// The thread check is what makes "this pass explains this listing" a fact rather than
    /// a hope: a concurrent `enumerate` on another thread can overwrite the slot, and
    /// without the stamp this `diagnose` would take that stranger's pass and present it as
    /// an explanation of a listing it has never seen. Losing the pass to an overwrite is
    /// the safe failure — this call then re-probes, exactly as it did before N193 — and
    /// taking a pass from another thread is the unsafe one, so the stamp is checked rather
    /// than the slot merely being locked (note **N198**).
    fn take_remembered_probe(&self) -> Option<Probe> {
        let mut slot = self
            .last_probe
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match slot.as_ref() {
            Some((took_it, _)) if *took_it == std::thread::current().id() => {
                slot.take().map(|(_, probe)| probe)
            }
            _ => None,
        }
    }
}

/// Everything one pass has to say about the cameras it did **not** list.
///
/// Pure, and separate from [`V4l2Backend::diagnose`] for the reason T1 has a `diagnose` at
/// all: the listing and the explanation of what it dropped are one value's two halves, and
/// a function over that one value is how "two halves" stops being a call order somebody has
/// to keep. `enumerate::group` reads [`Probe::probed`]; this reads [`Probe::unbound`] and
/// [`Probe::unreadable`]; the same `Probe` answers all three.
///
/// The order is the one `diagnose` has always produced: the driverless USB devices, then
/// the nodes that would not open. A camera that is plugged in with nothing driving it is a
/// different problem from a camera whose node is busy, and the first is the one a user can
/// act on without knowing anything about V4L2.
fn hints_for(probe: &Probe) -> Vec<ListHint> {
    let driverless = probe.unbound.iter().map(|device| ListHint {
        kind: HintKind::DriverlessUsbVideoDevice,
        subject: device.clone(),
    });
    let unreadable = probe
        .unreadable
        .values()
        .flatten()
        .map(|(path, error)| ListHint {
            kind: HintKind::NodeUnreadable,
            subject: format!("{path}: {error}"),
        });
    driverless.chain(unreadable).collect()
}

impl CameraBackend for V4l2Backend {
    fn kind(&self) -> BackendKind {
        BackendKind::V4l2
    }

    fn enumerate(&self) -> Result<Vec<CameraInfo>> {
        // The other half of the same reading is kept rather than thrown away for
        // `diagnose` to re-derive from a later one, and `listing` is what keeps it — one
        // function, so the keeping is not a line somebody can delete without a test
        // noticing (note **N198**).
        Ok(self.listing(probe_nodes()?))
    }

    fn open(&self, id: &CameraId) -> Result<Box<dyn Camera>> {
        // An *exact* id, like every backend. Prefix resolution is D1 policy over the
        // whole enumeration and lives in `engine::resolve`; a backend that resolved
        // prefixes too would be a second opinion about what `cam:obsbot` means.
        let info = self
            .enumerate()?
            .into_iter()
            .find(|info| info.id == *id)
            .ok_or_else(|| Error::CameraUnknown {
                requested: id.to_string(),
            })?;
        Ok(Box::new(V4l2Camera::open(info)?))
    }

    fn watch(&self) -> Result<Box<dyn HotplugWatch>> {
        Ok(Box::new(watch::Watch::open()?))
    }

    fn diagnose(&self) -> Vec<ListHint> {
        // **Both** halves come out of one pass, and that is the whole of what a hint is
        // for: it explains a listing, so it has to describe the moment the listing
        // describes. Note N193 moved the unreadable nodes onto the remembered pass and
        // left the driverless-device walk reading sysfs freshly, which is the same defect
        // in the other half (note **N198**).
        //
        // From the pass that produced the listing this is explaining, when there is one.
        // Only a `diagnose` nobody paired with an `enumerate` — on this thread — reads the
        // machine here, and then it is answering about now because there is no listing for
        // it to be about.
        match self.take_remembered_probe() {
            Some(probe) => hints_for(&probe),
            None => probe_nodes().as_ref().map(hints_for).unwrap_or_default(),
        }
    }
}

/// What one pass over the node list learned, successes and failures alike.
#[derive(Debug, Default)]
struct Probe {
    /// The nodes that answered `QUERYCAP`.
    probed: Vec<ProbedNode>,
    /// The nodes that did not, by the group they belong to. Keyed on the sysfs grouping
    /// key, which is readable without opening anything.
    unreadable: BTreeMap<String, Vec<(camino::Utf8PathBuf, Error)>>,
    /// The USB devices presenting a video-class interface with nothing bound to it.
    ///
    /// Read here rather than in [`V4l2Backend::diagnose`] for the reason the field above
    /// exists: a hint is an explanation *of a listing*, so it has to describe the same
    /// moment. This half was still a second reading of the machine after note N193 —
    /// `diagnose` walked sysfs freshly, so a camera whose driver bound between the listing
    /// and the diagnosis was reported driverless by a pass that had already seen it work
    /// (note **N198**).
    unbound: Vec<String>,
}

/// Read every node's sysfs facts and ask each one what it is.
///
/// **A group with an unreadable node is not described at all**, and that is the whole
/// point of keeping the failures. A node is classified by its `device_caps`, which only an
/// open node reports, so a group whose capture node could not be opened would enumerate
/// with its metadata node alone — and `capture_node()` would then answer `None`, which
/// every caller above reads as "this camera cannot capture". That is E3's forbidden
/// conversion exactly: a busy or vanished node answered as a capability. Dropping the
/// group says "we could not read this device", which is true; keeping a partial one says
/// something false about what the device can do.
///
/// The drop is not silent — [`V4l2Backend::diagnose`] reports each unreadable node — and
/// if **nothing** could be read, the first failure is returned rather than an empty list:
/// the overwhelmingly common cause is a missing `video` group membership, and "no cameras"
/// would hide the one message that says what to do about it.
fn probe_nodes() -> Result<Probe> {
    let nodes = sysfs::nodes()?;
    let mut probe = Probe {
        unbound: sysfs::unbound_video_devices(),
        ..Probe::default()
    };
    let mut first_failure = None;

    for node in &nodes {
        match probe_one(node) {
            Ok(entry) => probe.probed.push(entry),
            Err(error) => {
                if first_failure.is_none() {
                    first_failure = Some(error.clone());
                }
                probe
                    .unreadable
                    .entry(node.group_key().to_owned())
                    .or_default()
                    .push((node.dev_path.clone(), error));
            }
        }
    }

    // Every group that lost a node loses the rest of itself too.
    probe
        .probed
        .retain(|node| !probe.unreadable.contains_key(&node.group_key));

    match first_failure {
        Some(error) if probe.probed.is_empty() && !nodes.is_empty() => Err(error),
        _ => Ok(probe),
    }
}

fn probe_one(node: &sysfs::SysfsNode) -> Result<ProbedNode> {
    let fd = Fd::open(&node.dev_path)?;
    let cap = ioctl::querycap(&fd)?;
    Ok(ProbedNode {
        dev_path: node.dev_path.clone(),
        group_key: node.group_key().to_owned(),
        usb_id: node.usb_id,
        serial: node.serial.clone(),
        driver: cap.driver,
        card: cap.card,
        bus_info: cap.bus_info,
        capabilities: cap.capabilities,
        device_caps: cap.device_caps,
    })
}

/// A running stream: what the device agreed to, and the buffers it is filling.
///
/// The mappings live here rather than beside the fd because their lifetime is the
/// *stream's*, not the camera's: `stop_stream` drops this whole value, and dropping it
/// unmaps every buffer before `REQBUFS(0)` tells the driver they are free. Getting that
/// order wrong is a use-after-free the kernel is entitled to punish.
#[derive(Debug)]
struct StreamState {
    /// What `S_FMT`/`S_PARM` settled on, with every difference from the request (D5).
    negotiated: NegotiatedStream,
    /// The driver's buffers, indexed the way the driver indexes them.
    buffers: Vec<sys::mmap::Mapping>,
    /// How many frames this stream has delivered, so a timeout can say whether the camera
    /// is slow or dead (E3).
    frames_delivered: u32,
}

/// One open camera.
#[derive(Debug)]
pub struct V4l2Camera {
    info: CameraInfo,
    fd: Fd,
    /// `Some` between `start_stream` and `stop_stream`.
    ///
    /// No `Drop` impl reaches for this: closing the fd is what releases a V4L2 stream, the
    /// driver stops and frees its buffers on last close, and `Fd`'s own `Drop` does that.
    /// The mappings are unmapped by their own `Drop` in the same breath. A hand-written
    /// teardown here would be a second copy of a thing the kernel already guarantees.
    stream: Option<StreamState>,
}

impl V4l2Camera {
    /// Open the node frames come from, or — for a metadata-only camera — its first node,
    /// so `controls` and `formats` can still answer for it.
    fn open(info: CameraInfo) -> Result<V4l2Camera> {
        let path = Self::working_node(&info)?.to_owned();
        let fd = Fd::open(&path)?;
        Ok(V4l2Camera {
            info,
            fd,
            stream: None,
        })
    }

    fn working_node(info: &CameraInfo) -> Result<&Utf8Path> {
        info.capture_node()
            .or_else(|| info.nodes.first())
            .map(|node| node.path.as_path())
            .ok_or_else(|| Error::CameraUnknown {
                requested: info.id.to_string(),
            })
    }

    /// The control descriptor for one id, freshly queried.
    ///
    /// Re-queried rather than cached because flags change under us: the INACTIVE bit
    /// tracks whether an automation partner owns the control *right now* \[PF:3\], and a
    /// cached descriptor would report last week's answer.
    ///
    /// **Freshly, not exhaustively.** This used to answer by running the whole of
    /// [`Camera::controls`] and picking one entry out of it — a `QUERY_EXT_CTRL` per
    /// control, a `QUERYMENU` sweep per menu control and a `G_EXT_CTRLS` per readable one,
    /// to answer about a single id. `get` and `set` both come through here, so a guarded
    /// write paid that walk once per planned write, which on vivid's 77 controls is a
    /// sweep's inner loop (docs/11 §8 P1, note **N192**). The walk now stops the moment the
    /// question is answered — a **prefix of the `QUERY_EXT_CTRL` walk**, in the same order,
    /// with the `QUERYMENU` sweeps and `G_EXT_CTRLS` of the controls it steps over left out
    /// (note **N199**). Every call it makes is a call the old walk made, which is what makes
    /// this cheap enough to be worth doing on a device-driven path without a probe behind
    /// it. The targeted `QUERY_EXT_CTRL` that would cost *one* ioctl is a different bet and
    /// N192 says why it was not taken.
    fn describe(&self, id: ControlId) -> Result<ControlDesc> {
        self.walk_controls(Some(id))?
            .into_iter()
            .find(|desc| desc.id == id)
            .ok_or_else(|| Error::ControlUnknown {
                requested: id.to_string(),
                did_you_mean: Vec::new(),
            })
    }

    /// The `QUERY_EXT_CTRL` walk, once, for both questions asked of it.
    ///
    /// `wanted: None` is [`Camera::controls`] — every control this node has. `wanted:
    /// Some(id)` is [`V4l2Camera::describe`], and it is the *same walk*: the same ioctl in
    /// the same order, stopping the moment the question is answered. That is what makes
    /// the cheap version of docs/11's P1 safe to land without a probe behind it — **every
    /// call it makes is a call the walk was already making** (rule 4), so there is no new
    /// device behaviour to be wrong about.
    ///
    /// It is a prefix of the `QUERY_EXT_CTRL` walk and a *subsequence* of the whole thing:
    /// the `QUERYMENU` sweeps and the `G_EXT_CTRLS` of the controls it steps over are
    /// dropped. Both halves of that sentence are the point — nothing new is asked, and less
    /// is asked — and calling the whole thing a "strict prefix" overstated it in the one
    /// direction that matters for rule 4 (note **N199**). The expensive-to-justify version,
    /// a direct `QUERY_EXT_CTRL` on the id with `NEXT_CTRL` cleared, is one ioctl instead
    /// of a prefix and is a claim about how a driver answers a call this build has never
    /// made; note **N192** carries the judgement and the evidence it would need.
    ///
    /// Two things the targeted walk must not lose, and does not: the strictly-increasing
    /// guard below still bounds a device contradicting itself, and a control whose name
    /// slugs to nothing is still stepped over rather than answered with.
    fn walk_controls(&self, wanted: Option<ControlId>) -> Result<Vec<ControlDesc>> {
        let mut controls = Vec::new();
        let mut previous: Option<u32> = None;

        for _ in 0..limits::MAX_CONTROLS_PER_DEVICE {
            let walked = match ioctl::query_ext_ctrl(&self.fd, previous.unwrap_or(0))? {
                ioctl::Enumerated::Exhausted => break,
                ioctl::Enumerated::Entry(walked) => walked,
            };
            // `NEXT_CTRL` promises strictly increasing ids. A driver that repeats one
            // would otherwise spin here until the cap, reporting the same control over
            // and over; stopping is the honest response to a device contradicting itself.
            //
            // "Have we seen one yet" is an `Option` rather than `previous != 0`, which
            // was the same question asked of a value that can legitimately *be* zero: a
            // driver answering id 0 every time would have skipped the guard on every
            // iteration and walked to the cap.
            if previous.is_some_and(|last| walked.id <= last) {
                break;
            }
            previous = Some(walked.id);

            if let Some(ControlId(target)) = wanted {
                // The same promise, read the other way: a walk that has gone past the id
                // it was sent for will not meet it later.
                if walked.id > target {
                    break;
                }
                if walked.id != target {
                    continue;
                }
            }

            // A control whose name slugs to nothing has no handle D2 will invent; the
            // walk steps past it rather than stopping, so it cannot hide the rest.
            if let Some(mut desc) = walked.desc {
                self.read_menu(&mut desc)?;
                // Not `self.read_current(&desc)?`: a control the device would not read is
                // carried valueless, and only a device that has *gone* ends the walk. The
                // whole argument is on [`walked_current`].
                desc.current = walked_current(wanted, self.read_current(&desc))?;
                controls.push(desc);
            }

            // A targeted walk has met its id — with or without a descriptor for it — so
            // there is nothing further to ask this device.
            if wanted.is_some() {
                break;
            }
        }

        Ok(controls)
    }

    /// Fill in a control's menu items, tolerating the holes \[PF:2\].
    fn read_menu(&self, desc: &mut ControlDesc) -> Result<()> {
        if !desc.control_type.is_menu() {
            return Ok(());
        }
        // The declared range bounds the walk, and `MAX_MENU_INDICES` bounds the declared
        // range: a driver reporting `0..=i64::MAX` must not become an infinite loop.
        let first = u32::try_from(desc.range.min.max(0)).unwrap_or(0);
        let last = u32::try_from(desc.range.max.max(0)).unwrap_or(u32::MAX);
        let stop = last.min(first.saturating_add(limits::MAX_MENU_INDICES));

        for index in first..=stop {
            match ioctl::querymenu(&self.fd, desc.id.0, index, desc.control_type.to_raw())? {
                // EINVAL on an index is a *hole*, not the end of the menu: the Chicony's
                // Auto Exposure has items {1, 3} and index 2 answers EINVAL between them.
                ioctl::Enumerated::Exhausted => {}
                ioctl::Enumerated::Entry((index, item)) => {
                    desc.menu.insert(index, item);
                }
            }
        }
        Ok(())
    }

    /// Read a control's current value, or leave it `None` when reading is meaningless.
    fn read_current(&self, desc: &ControlDesc) -> Result<Option<ControlValue>> {
        // A button has no value, a control class is a header rather than a control, and a
        // write-only control's value means nothing. None of these is a failure.
        if matches!(
            desc.control_type,
            ControlType::Button | ControlType::ControlClass
        ) || desc.flags.has(KnownFlag::WriteOnly)
            || desc.flags.has(KnownFlag::Disabled)
        {
            return Ok(None);
        }

        if ioctl::has_payload(desc.flags.raw) {
            // `elem_size × elems` is device-supplied; `payload_len` is where it is
            // bounded before it can become an allocation (rubric B10).
            let Some(len) = sys::decode::payload_len(desc.elem_size, desc.elems) else {
                return Ok(None);
            };
            return match ioctl::get_payload(&self.fd, desc.id.0, len) {
                Ok(bytes) => Ok(Some(ControlValue::Bytes(bytes))),
                Err(error) => Ok(unreadable_current(error)?),
            };
        }

        match ioctl::get_scalar(&self.fd, desc.id.0, desc.control_type.to_raw()) {
            Ok(value) => Ok(Some(value)),
            Err(error) => Ok(unreadable_current(error)?),
        }
    }
}

/// A control the device declined to read is reported as valueless, not as a failed
/// enumeration — but only for the two errno values that are facts about the *control*.
///
/// The distinction is E3's, applied one level down. `EINVAL` and `EACCES` from
/// `G_EXT_CTRLS` on a single control both mean "this control has no readable current
/// value": the UAPI documents `EACCES` as the answer for attempting to read a write-only
/// control, and several drivers answer `EINVAL` for controls they enumerate but will not
/// read. Neither says anything about our access to the *device* — an fd we could not open
/// would have failed at `Fd::open`, long before here.
///
/// Everything else propagates. `EBUSY`, `ENODEV` and a permission refusal *of the device*
/// are availability facts, and flattening one of those into "no value" would report a
/// camera someone unplugged mid-enumeration as a camera whose controls have no values.
fn unreadable_current(error: Error) -> Result<Option<ControlValue>> {
    match error {
        // Matched on the errno rather than on the variant: `Fd::open` maps `EACCES` to
        // `PermissionDenied`, and that mapping is right *there* and wrong here, so this
        // path must not accept the variant — only the raw code from an ioctl on an fd we
        // already hold.
        Error::DeviceIo { errno, .. }
            if errno == Some(libc::EINVAL) || errno == Some(libc::EACCES) =>
        {
            Ok(None)
        }
        other => Err(other),
    }
}

/// What a negotiation reports for a device that would not name a frame interval.
///
/// `sys::decode::capture_interval` answers `None` for a driver that cleared
/// `V4L2_CAP_TIMEPERFRAME` — the driver saying its `timeperframe` field means nothing —
/// and one line above the `sys` boundary that `None` used to become
/// `FrameInterval::Unknown { raw: 0 }`. `raw` is documented as *the kernel's `type`
/// discriminant, preserved exactly*, and there was no discriminant: `0` was this build's
/// invention, and not even a spare one, since `0` is what a driver filling in nothing
/// writes. D2 asks for the unknown to be represented, not for the evidence to be
/// manufactured, so the vocabulary grew the fourth answer it was short of
/// ([`FrameInterval::Unstated`], note **N194**).
///
/// A function rather than a `.unwrap_or(…)` because that is what the defect was: one
/// value, chosen once, on a line nothing could ask about.
fn stated_interval(reported: Option<FrameInterval>) -> FrameInterval {
    reported.unwrap_or(FrameInterval::Unstated)
}

/// The same read, asked the question **this walk** is answering.
///
/// `wanted` is the walk's subject, and it decides the whole of this function. A walk sent
/// for one id is [`V4l2Camera::describe`], which is how `get` and `set` read the device:
/// they were handed one id, so an availability fact about that id *is* their answer and it
/// is passed up unchanged. A walk with no `wanted` is [`Camera::controls`], which is not
/// asking about a control at all — it is describing a device, and one control the driver
/// declined to read says nothing about the other seventeen. Ending the enumeration there
/// answers "what can this camera do" with "something went wrong reading one knob", which
/// is availability converted into capability at the level D2 exists to prevent (AGENTS
/// rule 7, E3); rule 6 gives the shape of the repair, and the control is carried valueless
/// (note **N192**).
///
/// **Passing the subject in is not decoration.** The two policies used to be two
/// functions, and which one `describe` got was decided by which of them the shared walk
/// happened to call — so the tolerant one reached `set`, whose write then landed and whose
/// read-back propagated the refusal the walk had just swallowed, losing D3's
/// `{requested, applied}` from a write the device had taken (note **N196**). The subject
/// is now an argument, so both policies are one function two tests can drive.
///
/// **Exactly one refusal is carried, and it is the one the UAPI makes about a control.**
/// `EBUSY` from `G_EXT_CTRLS` is documented as the answer for a control whose *device
/// function* another application has taken over — a fact about one knob, on a camera that
/// is answering `QUERY_EXT_CTRL` for every control on it. Everything else propagates:
///
/// - [`Error::DeviceGone`], because the subject of the enumeration has stopped existing,
///   every remaining read will fail the same way, and a full list of valueless controls
///   would describe a camera nobody can photograph as one whose knobs happen to have no
///   values — the same conversion with the arguments swapped, and worse, because it looks
///   like an answer.
/// - [`Error::PermissionDenied`] and [`Error::DeviceIo`], because rule 7 names `EBUSY`,
///   `ENODEV`, `EPERM` and a timeout as four things that stay distinct, and the first
///   version of this tolerance converted three of them into "no value". `DeviceIo` is the
///   sharp one: `sys::ioctl::short_reply` is a `DeviceIo` with no errno reading *the
///   kernel's reply was shorter than the bindings describe*, so a bindgen or offset defect
///   in the one crate that carries `unsafe` was arriving as an absent control value.
///
/// **What a carried control costs.** The absence is visible and the reason is not:
/// [`ControlDesc::value_was_declined`] is the predicate that separates it from an absence
/// the descriptor predicted, and it is derived from fields that are already on the wire.
/// *Which* refusal is lost — but the population is now one errno, so there is exactly one
/// thing it could have been. Note **N192** records the trade and what would reverse it.
///
/// **The menu is deliberately not treated this way.** `read_menu`'s `?` still ends the
/// walk, because the two failures are not alike: a missing *value* is an absence a reader
/// can see, while a missing *menu item* is invisible — menus are legitimately sparse
/// \[PF:2\], so a partially read one looks exactly like a complete one, and D3's pair
/// discovery finds "Manual Mode" by name in it.
fn walked_current(
    wanted: Option<ControlId>,
    read: Result<Option<ControlValue>>,
) -> Result<Option<ControlValue>> {
    match read {
        Ok(current) => Ok(current),
        // The caller named this control, so this refusal is the answer it asked for.
        Err(error) if wanted.is_some() => Err(error),
        Err(Error::Busy { .. }) => Ok(None),
        Err(other) => Err(other),
    }
}

/// What an `S_EXT_CTRLS` failure means for the control it was aimed at.
///
/// The write-side sibling of [`unreadable_current`], and deliberately **not** a reuse of
/// it: the same errno means different things in the two directions. From a *read*,
/// `EACCES` means "this control has no readable value" and the enumeration carries on;
/// from a *write* the UAPI documents it as the answer for a read-only control, and the
/// caller's next move is to stop asking rather than to try again. Flattening the write
/// into the read's answer would report a refused write as a successful one with no value.
///
/// `EINVAL` keeps its message but gains the control's name: "invalid argument" about an
/// unnamed ioctl is the least actionable sentence in the registry, and by here we know
/// exactly which control and which value the device would not take.
///
/// Everything else — `EBUSY`, `ENODEV`, a device-level permission refusal — passes
/// through untouched. Those are availability facts (E3) and none of them is a statement
/// about the control.
/// The refusal for a value whose shape is not the one this control takes.
///
/// `EINVAL`, and the errno is the driver's own answer for a value it will not accept —
/// spelled here because this build refuses it before the ioctl rather than after. The
/// fake produces the identical shape for the identical input (E5), which is what makes
/// "the two backends behave alike" a checkable claim rather than a hope.
fn shape_mismatch(desc: &ControlDesc, wants_payload: bool, given: &ControlValue) -> Error {
    let wanted = if wants_payload {
        "an opaque byte payload"
    } else {
        "an integer"
    };
    let got = match given {
        ControlValue::Int(_) => "an integer",
        ControlValue::Text(_) => "a string",
        ControlValue::Bytes(bytes) => {
            return Error::DeviceIo {
                operation: format!("{SET_CTRL_OP} ({})", desc.slug),
                errno: Some(libc::EINVAL),
                message: format!(
                    "this control takes {wanted}; a {}-byte payload would be handed to the \
                     kernel as a pointer it does not read as one",
                    bytes.len()
                ),
            };
        }
    };
    Error::DeviceIo {
        operation: format!("{SET_CTRL_OP} ({})", desc.slug),
        errno: Some(libc::EINVAL),
        message: format!("this control takes {wanted}, not {got}"),
    }
}

fn unwritable_control(desc: &ControlDesc, error: Error) -> Error {
    match error {
        Error::DeviceIo { errno, .. } if errno == Some(libc::EACCES) => Error::ControlReadOnly {
            control: desc.slug.clone(),
        },
        Error::DeviceIo { errno, message, .. } if errno == Some(libc::EINVAL) => Error::DeviceIo {
            operation: format!("{SET_CTRL_OP} ({})", desc.slug),
            errno,
            message,
        },
        other => other,
    }
}

impl Camera for V4l2Camera {
    fn info(&self) -> &CameraInfo {
        &self.info
    }

    fn formats(&self) -> Result<Vec<FormatInfo>> {
        let mut formats = Vec::new();
        for index in 0..limits::MAX_FORMATS_PER_NODE {
            let (pixel_format, description, flags) = match ioctl::enum_fmt(&self.fd, index)? {
                ioctl::Enumerated::Exhausted => break,
                ioctl::Enumerated::Entry(entry) => entry,
            };
            formats.push(FormatInfo {
                sizes: self.sizes_for(pixel_format)?,
                pixel_format,
                description,
                flags,
            });
        }
        Ok(formats)
    }

    fn controls(&self) -> Result<Vec<ControlDesc>> {
        self.walk_controls(None)
    }

    fn get(&mut self, id: ControlId) -> Result<ControlValue> {
        let desc = self.describe(id)?;
        // Unvalidated on purpose [PF:4]: the OBSBOT's `Zoom, Continuous` declares
        // [-100..100] and holds 245, and that is a fact about the device.
        self.read_current(&desc)?
            .ok_or_else(|| Error::ControlUnknown {
                requested: desc.slug.to_string(),
                did_you_mean: Vec::new(),
            })
    }

    fn set(&mut self, id: ControlId, value: ControlValue) -> Result<Applied> {
        // Freshly queried, and that is load-bearing three times over: the range the
        // read-back is explained against, the flags that decide whether the write is
        // allowed at all, and the INACTIVE bit an automation partner may have set since
        // the caller last looked \[PF:3\].
        let desc = self.describe(id)?;
        if !desc.is_writable() {
            // PF:12's `Privacy`, plus DISABLED controls and class headers. One "you may
            // not write this" refusal in D13, and all three are the device stating a
            // capability limit rather than a passing condition (E3).
            return Err(Error::ControlReadOnly { control: desc.slug });
        }

        // **The descriptor chooses the ioctl shape, not the value.** `read_current`
        // decides by the control's `HAS_PAYLOAD` flag, and a write that decided by the
        // caller's variant instead would be the two directions disagreeing about what a
        // control *is*.
        //
        // The consequence of getting this backwards is not a type error, which is why it
        // is checked rather than assumed: `set_payload` plants a heap address in
        // `v4l2_ext_control`'s union, and for a control the kernel does not treat as a
        // pointer control, `uvc_ctrl_set` ignores `size` entirely and takes the low 32
        // bits of that address as the value — clamped into range and reported as an
        // ordinary driver adjustment. On a PTZ control that is a motor driven to its
        // limit by an allocator. The fake refuses the same input (E5); so does this now.
        match (ioctl::has_payload(desc.flags.raw), &value) {
            (false, ControlValue::Int(scalar)) => {
                ioctl::set_scalar(&self.fd, id.0, desc.control_type.to_raw(), *scalar)
            }
            (true, ControlValue::Bytes(bytes)) => ioctl::set_payload(&self.fd, id.0, bytes),
            (wants_payload, given) => Err(shape_mismatch(&desc, wants_payload, given)),
        }
        .map_err(|error| unwritable_control(&desc, error))?;

        // D3's read-back, and the backend's obligation rather than the engine's: only the
        // thing holding the fd can say what the device took \[PF:6\].
        //
        // Two ways it can be unavailable, and they are different facts. A BUTTON or a
        // WRITE_ONLY control is unreadable *by its descriptor*, which `classify` reads for
        // itself; a device that enumerates a control and then declines to read it is a
        // surprise the descriptor did not predict, and only the code that made the call
        // knows it happened. Both land in the same warning, so neither can pass for an
        // observation — `applied` equal to `requested` with no warning would be a claim
        // nobody checked.
        let (applied, declined) = match desc.unverifiable() {
            Some(_) => (value.clone(), false),
            None => match self.read_current(&desc)? {
                Some(read) => (read, false),
                None => (value.clone(), true),
            },
        };
        let warnings = if declined {
            vec![WriteWarning::Unverified {
                because: Unverifiable::DeviceDeclinedToRead,
            }]
        } else {
            WriteWarning::classify(&desc, &value, &applied)
        };

        Ok(Applied {
            control: id,
            slug: desc.slug.clone(),
            warnings,
            requested: value,
            applied,
        })
    }

    fn start_stream(&mut self, request: &StreamRequest) -> Result<NegotiatedStream> {
        if self.stream.is_some() {
            // What the driver would say a moment later: `S_FMT` on a streaming node
            // answers `EBUSY`. Saying it here keeps a half-finished re-negotiation from
            // tearing down buffers the caller is still dequeuing from. The holder list is
            // empty rather than naming this process — D13's `holders` is for the
            // `/proc` walk that identifies *other* processes, and inventing a one-entry
            // list here would make "who has it" answerable two different ways.
            //
            // It said that and did the opposite until 2026-08-16 (note **N191**). The walk
            // was run over `self.fd.path()`, a node **this** process holds — it is holding
            // it to make this refusal — so it found us, and D10's `terminate_holder` reads
            // a `Busy` holder list as the list of pids that would free the camera. N48
            // point 5 is the sentence that was broken: "naming this process's pid would
            // invite a client to kill the daemon it is talking to." The layer that is
            // right about who holds this stream is this one, so it is the one that
            // answers, and `engine::actor`'s own `Busy` says the same thing in the same
            // words for the same reason.
            //
            // **Nothing caught it one layer up**, and the comment that landed with note
            // N191 said `daemon::server::not_this_daemon` did. It does not:
            // `not_this_daemon` refuses a `terminate_holder` *request* naming this
            // process's pid, which is a different value on a different verb — a client
            // reading the pid out of a `Busy` payload got a refusal only when it went on
            // to ask for the kill (note **N197**). `sys::ioctl::control_error` and
            // `holders::others_holding` are what hold the rest of this class.
            return Err(Error::Busy {
                holders: Vec::new(),
                path: self.fd.path().to_owned(),
            });
        }
        // A metadata-only camera is a shape D1 supports on purpose: it is listed, and
        // streaming it is a typed refusal rather than a surprise.
        if self.info.capture_node().is_none() {
            return Err(Error::format_unsupported(request.pixel_format, Vec::new()));
        }

        let negotiated = self.negotiate(request)?;
        match self.map_buffers(request.buffer_count) {
            Ok(buffers) => {
                self.stream = Some(StreamState {
                    negotiated: negotiated.clone(),
                    buffers,
                    frames_delivered: 0,
                });
                Ok(negotiated)
            }
            Err(error) => {
                // Half a stream is worse than none: the driver is holding buffers nobody
                // will dequeue, and the next `start_stream` would find the node busy. The
                // release cannot report anything useful — we are already failing, and the
                // caller needs the *first* error, not this one.
                self.release_buffers();
                Err(error)
            }
        }
    }

    fn streaming(&self) -> Option<NegotiatedStream> {
        // The field is `Some` between `start_stream` and `stop_stream` and holds what the
        // driver agreed to, so this is a read rather than a second record: there is no
        // ioctl that asks a node whether it is streaming, and the only reason this backend
        // can answer at all is that it already has to, one function up, to refuse a second
        // `STREAMON` the way the driver would.
        self.stream.as_ref().map(|state| state.negotiated.clone())
    }

    fn next_frame(&mut self, deadline: Instant) -> Result<Frame> {
        let started = Instant::now();
        // The loop exists for one reason: a buffer the driver marks `V4L2_BUF_FLAG_ERROR`
        // carries no frame, and handing one back as a frame is how a decoder ends up
        // reading a half-written JPEG. It is requeued and the wait resumes against the
        // *same* deadline, so a device producing nothing but corrupt frames times out
        // rather than spinning.
        loop {
            let Some(state) = self.stream.as_ref() else {
                return Err(Error::DeviceIo {
                    operation: "VIDIOC_DQBUF".to_owned(),
                    errno: None,
                    message: "the stream is not running".to_owned(),
                });
            };
            let frames_delivered = state.frames_delivered;

            if !sys::wait::readable(&self.fd, deadline)? {
                return Err(Error::SettleTimeout {
                    waited_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
                    frames_seen: frames_delivered,
                });
            }

            let dequeued = ioctl::dequeue_buffer(&self.fd)?;
            let frame = self.take_frame(&dequeued);
            // Back to the driver either way, and before the error return: a buffer we
            // dequeued and did not requeue is one the device will never fill again, so a
            // stream that hits a few corrupt frames would starve itself.
            ioctl::queue_buffer(&self.fd, dequeued.index)?;

            match frame {
                Some(frame) => {
                    if let Some(state) = self.stream.as_mut() {
                        state.frames_delivered = state.frames_delivered.saturating_add(1);
                    }
                    return Ok(frame);
                }
                None => continue,
            }
        }
    }

    fn stop_stream(&mut self) -> Result<()> {
        // Idempotent, like `VIDIOC_STREAMOFF` itself: stopping a stopped stream is not an
        // error, and a caller unwinding from a failure should not have to know whether it
        // got as far as starting.
        if self.stream.is_none() {
            return Ok(());
        }
        let stopped = ioctl::stream_off(&self.fd);
        // The mappings go before `REQBUFS(0)`, which is what tells the driver the buffers
        // are free — unmapping after that would be unmapping memory the kernel has
        // already taken back.
        self.stream = None;
        let released = ioctl::request_buffers(&self.fd, 0);

        // Both are reported, and `STREAMOFF` wins: it is the one whose failure means
        // frames may still be arriving.
        stopped?;
        released.map(|_| ())
    }
}

impl V4l2Camera {
    /// Ask the driver for the caller's format and report what it actually agreed to (D5).
    ///
    /// What to ask *for* is resolved against the device's own enumeration by
    /// [`StreamRequest::choose`], not against the node's current format. That distinction
    /// is load-bearing and was learned the hard way: a V4L2 node's format is **persistent
    /// device state**, so building the request on top of `G_FMT` makes
    /// `StreamRequest::default()` mean "whatever the last program left this camera set
    /// to". The first hardware run of the streaming suite streamed 1920×1080; the second,
    /// after a test had negotiated 3×3 down to 320×180, streamed 320×180 — same code,
    /// same camera, different answer. A photo verb whose resolution depends on what ran
    /// before it is not one anybody can use, and the shared chooser is also what keeps the
    /// fake's answer to the same request identical (E5).
    ///
    /// **Including the refusal**, since 2026-08-16. This function used to raise
    /// [`Error::FormatUnsupported`] only for a device whose whole format list was empty or
    /// unreadable, because that was the only thing `choose` could not answer — so a caller
    /// naming a format this camera does not enumerate was ranked into another one and
    /// photographed, while the fake refused the same request. The refusal is the shared
    /// resolver's now, on the D5 sentence both backends were supposed to be reading (note
    /// **N134**), and this line is what makes "a divergence between the stand-in and the
    /// real thing convicts whichever side is wrong" cost one `?` instead of a guard per
    /// backend.
    fn negotiate(&self, request: &StreamRequest) -> Result<NegotiatedStream> {
        let formats = self.formats()?;
        let chosen = request.choose(&formats)?;
        let wanted = ioctl::set_format(
            &self.fd,
            chosen.pixel_format.to_fourcc(),
            chosen.width,
            chosen.height,
        )?;

        // The interval is a separate negotiation with its own capability bit, and a
        // device that does not implement it has said so rather than failed: `interval`
        // then reports `Unstated`, which is D2's "represent what the device would not
        // say" rather than a fabricated 30 fps.
        // `stated_interval`, not `request.interval`: `Unstated` is a document saying the
        // caller asked for nothing, and the schema collapses it with an omitted field so
        // the two backends cannot answer it differently (note **N199**).
        let interval = match request.stated_interval() {
            Some(FrameInterval::Discrete {
                numerator,
                denominator,
            }) => ioctl::set_interval(&self.fd, numerator, denominator)?,
            // A stepwise or unreadable interval is not something `S_PARM` can be asked
            // for — its field is one fraction — so the request is left alone and the
            // answer below reports whatever the device is running at. The difference is
            // then visible as an adjustment rather than as a silently ignored field.
            _ => ioctl::get_interval(&self.fd)?,
        };
        let interval = stated_interval(interval);

        Ok(NegotiatedStream {
            pixel_format: wanted.pixel_format,
            width: wanted.width,
            height: wanted.height,
            bytes_per_line: wanted.bytes_per_line,
            size_image: wanted.size_image,
            interval,
            adjustments: NegotiatedStream::diff(
                request,
                wanted.pixel_format,
                wanted.width,
                wanted.height,
                interval,
            ),
        })
    }

    /// Ask for buffers, map each one, and hand them all to the driver to fill.
    ///
    /// The count the caller asked for is a request in both directions: bounded above by
    /// [`limits::MAX_BUFFERS_PER_STREAM`] before the driver is asked to allocate anything,
    /// and reported back by the driver, which is free to grant fewer.
    fn map_buffers(&self, requested: u32) -> Result<Vec<sys::mmap::Mapping>> {
        let asked = requested.clamp(1, limits::MAX_BUFFERS_PER_STREAM);
        let granted = ioctl::request_buffers(&self.fd, asked)?;
        if granted > asked {
            // The driver's reply is device-supplied and therefore input (rubric B10): it
            // reaches an allocation and a loop bound, and the *request* being clamped says
            // nothing about the answer. A driver granting more than it was asked for has
            // contradicted itself, and believing it would mean mapping buffers on its say-so.
            //
            // Released before refusing, or the node stays holding them.
            self.release_buffers();
            return Err(Error::DeviceIo {
                operation: "VIDIOC_REQBUFS".to_owned(),
                errno: None,
                message: format!("the driver granted {granted} buffers for a request of {asked}"),
            });
        }
        if granted == 0 {
            // Not a capability answer: the device can stream, it just has no memory to do
            // it with right now (E3). `REQBUFS` succeeding with a count of zero is the
            // documented way a driver says that, and reading it as success would make
            // `STREAMON` fail with something far less legible.
            return Err(Error::DeviceIo {
                operation: "VIDIOC_REQBUFS".to_owned(),
                errno: None,
                message: format!("the driver granted 0 of the {asked} buffers asked for"),
            });
        }

        let mut buffers = Vec::with_capacity(usize::try_from(granted).unwrap_or(0));
        for index in 0..granted {
            let mapping = ioctl::query_buffer(&self.fd, index)?;
            buffers.push(sys::mmap::Mapping::map(
                &self.fd,
                mapping.offset,
                mapping.length,
            )?);
            // Queued as it is mapped rather than in a second pass: a buffer that is mapped
            // and not queued is one the driver will never fill, and the two loops could
            // drift apart.
            ioctl::queue_buffer(&self.fd, index)?;
        }
        ioctl::stream_on(&self.fd)?;
        Ok(buffers)
    }

    /// Undo whatever `map_buffers` managed before it failed.
    ///
    /// Best effort by construction: this runs while another error is on its way to the
    /// caller, and that error is the one worth reporting.
    fn release_buffers(&self) {
        let _ = ioctl::stream_off(&self.fd);
        let _ = ioctl::request_buffers(&self.fd, 0);
    }

    /// Copy one dequeued buffer out, or `None` when the driver says it is not a frame.
    ///
    /// The copy happens *before* the buffer is requeued — the driver is free to start
    /// overwriting it the moment it has it back — and it is a copy rather than a borrow
    /// because a `Frame` outlives the buffer it came from by design.
    fn take_frame(&self, dequeued: &sys::decode::Dequeued) -> Option<Frame> {
        let state = self.stream.as_ref()?;
        if dequeued.is_error() {
            return None;
        }
        let index = usize::try_from(dequeued.index).ok()?;
        // The index is device-supplied: a driver naming a buffer it never gave us gets
        // `None` and a requeue, not an index into the vector (rubric B10).
        let mapping = state.buffers.get(index)?;
        let bytes = mapping.bytes(dequeued.bytes_used);
        if bytes.is_empty() {
            // A zero-length frame is not a frame. It happens on the first buffer of some
            // UVC streams, and passing it on would make the JPEG-marker check downstream
            // report a corrupt bitstream instead of an empty one.
            return None;
        }

        Some(Frame {
            bytes: bytes.to_vec(),
            pixel_format: state.negotiated.pixel_format,
            width: state.negotiated.width,
            height: state.negotiated.height,
            bytes_per_line: state.negotiated.bytes_per_line,
            sequence: dequeued.sequence,
            timestamp_us: dequeued.timestamp_us,
        })
    }

    /// Every size this format offers, with the intervals available *at that size*.
    ///
    /// The nesting is not decoration: the OBSBOT reaches far higher in MJPG than in YUYV
    /// on the same cable — 3840×2160 against 640×480 when PF:9 measured it, 1920×1440
    /// against 640×480 since the device stopped advertising 4K \[PF:9, PF:23\] — so a
    /// flat list would be a lie.
    fn sizes_for(&self, pixel_format: PixelFormat) -> Result<Vec<FrameSizeInfo>> {
        let fourcc = pixel_format.to_fourcc();
        let mut sizes = Vec::new();

        for index in 0..limits::MAX_FRAME_SIZES_PER_FORMAT {
            let size = match ioctl::enum_framesizes(&self.fd, fourcc, index)? {
                ioctl::Enumerated::Exhausted => break,
                ioctl::Enumerated::Entry(size) => size,
            };
            // Intervals are enumerated *at a size*, so a size whose dimensions this build
            // cannot read has none to ask for — the entry is still carried, with an empty
            // interval list, rather than dropped.
            let intervals = match size.max_dimensions() {
                Some((width, height)) => self.intervals_for(fourcc, width, height)?,
                None => Vec::new(),
            };
            sizes.push(FrameSizeInfo { size, intervals });
        }
        Ok(sizes)
    }

    fn intervals_for(
        &self,
        fourcc: u32,
        width: u32,
        height: u32,
    ) -> Result<Vec<schema::camera::FrameInterval>> {
        let mut intervals = Vec::new();
        for index in 0..limits::MAX_FRAME_INTERVALS_PER_SIZE {
            match ioctl::enum_frameintervals(&self.fd, fourcc, width, height, index)? {
                ioctl::Enumerated::Exhausted => break,
                ioctl::Enumerated::Entry(interval) => intervals.push(interval),
            }
        }
        Ok(intervals)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A control descriptor for the pure error-mapping tests.
    fn desc(slug: &str, flags: u32) -> ControlDesc {
        use schema::control::{ControlFlags, ControlRange, ControlSlug};

        ControlDesc {
            id: ControlId(0x0098_0900),
            name: slug.to_owned(),
            slug: ControlSlug::parse(slug).expect("literal slug"),
            control_type: ControlType::Integer,
            range: ControlRange {
                min: 0,
                max: 100,
                step: 1,
            },
            default: 0,
            flags: ControlFlags::from_raw(flags),
            menu: BTreeMap::new(),
            elems: 1,
            elem_size: 4,
            dims: Vec::new(),
            current: Some(ControlValue::Int(0)),
        }
    }

    #[test]
    fn an_eacces_from_a_write_is_a_read_only_control_and_not_a_permission_problem() {
        // The same errno the *read* path reads as "no readable value". From a write the
        // UAPI documents it as "read-only control", and the caller's next move differs:
        // stop asking, rather than join the `video` group or try again.
        let control = desc("privacy", 0);
        let mapped = unwritable_control(
            &control,
            Error::DeviceIo {
                operation: SET_CTRL_OP.to_owned(),
                errno: Some(libc::EACCES),
                message: "Permission denied".to_owned(),
            },
        );
        assert_eq!(
            mapped,
            Error::ControlReadOnly {
                control: control.slug.clone()
            }
        );
        assert_ne!(
            mapped.kind(),
            schema::error::ErrorKind::PermissionDenied,
            "a read-only control is not a permission problem with the device"
        );
    }

    #[test]
    fn an_einval_from_a_write_names_the_control_that_refused_the_value() {
        // "Invalid argument" about an unnamed ioctl is the least actionable sentence in
        // the registry, and by the time we are here we know exactly which control it was.
        let control = desc("brightness", 0);
        let mapped = unwritable_control(
            &control,
            Error::DeviceIo {
                operation: SET_CTRL_OP.to_owned(),
                errno: Some(libc::EINVAL),
                message: "Invalid argument".to_owned(),
            },
        );
        assert_eq!(mapped.kind(), schema::error::ErrorKind::DeviceIo);
        assert!(mapped.to_string().contains("brightness"), "{mapped}");
    }

    #[test]
    fn an_availability_failure_from_a_write_passes_through_untouched() {
        // E3: `EBUSY`, `ENODEV` and a device-level permission refusal say nothing about
        // the control, and a write path that rewrote them as `ControlReadOnly` would
        // report an unplugged camera as a camera whose controls cannot be written.
        let control = desc("brightness", 0);
        for error in [
            Error::Busy {
                path: camino::Utf8PathBuf::from("/dev/video0"),
                holders: Vec::new(),
            },
            Error::DeviceGone {
                path: camino::Utf8PathBuf::from("/dev/video0"),
            },
            Error::PermissionDenied {
                path: camino::Utf8PathBuf::from("/dev/video0"),
                hint: "join the video group".to_owned(),
            },
        ] {
            assert_eq!(
                unwritable_control(&control, error.clone()),
                error,
                "an availability fact must not become a capability answer"
            );
        }
    }

    #[test]
    fn a_value_whose_shape_the_control_does_not_take_is_refused_before_the_ioctl() {
        // The one that matters: `set_payload` plants a heap address in the union, and for
        // a control the kernel does not treat as a pointer control it takes the low 32
        // bits of that address as the value — clamped into range and reported as an
        // ordinary driver adjustment. On a PTZ control that is a motor driven to its limit
        // by an allocator. The shape comes from the *descriptor*, not from the caller.
        let scalar = desc("brightness", 0);
        let payload = shape_mismatch(&scalar, false, &ControlValue::Bytes(vec![0; 4]));
        assert_eq!(payload.kind(), schema::error::ErrorKind::DeviceIo);
        let rendered = payload.to_string();
        assert!(rendered.contains("brightness"), "{rendered}");
        assert!(rendered.contains("integer"), "{rendered}");
        assert!(rendered.contains("pointer"), "{rendered}");

        // The mirror image is harmless at the kernel — an `Int` on a payload control
        // leaves `size` zero and the kernel answers `EFAULT` — but it is still a caller
        // asking for something this control does not take, and answering it here names the
        // control rather than relaying an errno from two layers down.
        let compound = desc("region_of_interest_rectangle", 0);
        let integer = shape_mismatch(&compound, true, &ControlValue::Int(5));
        assert!(integer.to_string().contains("payload"), "{integer}");

        // A text value has no V4L2 spelling a read of the same control would produce.
        let text = shape_mismatch(&scalar, false, &ControlValue::Text("x".to_owned()));
        assert!(text.to_string().contains("not a string"), "{text}");
    }

    /// A camera whose descriptor is a file this process made, with a stream running.
    ///
    /// Not a device, and it does not need to be: `start_stream`'s first act is to refuse a
    /// second stream, and it must do that without asking the device anything. A file only
    /// this process has open is also the one node a `/proc` walk can be *predicted* about,
    /// which the assertion below needs — `/dev/null` would answer with four of the
    /// hundreds of processes that hold it (`limits::MAX_HOLDERS_REPORTED`).
    fn streaming_camera(node: &Utf8Path) -> Option<V4l2Camera> {
        let fd = Fd::open(node).ok()?;
        Some(V4l2Camera {
            info: CameraInfo {
                id: CameraId::parse("cam:already-streaming").expect("literal id"),
                fingerprint: schema::camera::CameraFingerprint {
                    bus_path: "1-1:1.0".to_owned(),
                    usb_id: None,
                    card: "Already Streaming".to_owned(),
                    driver: "uvcvideo".to_owned(),
                    serial: None,
                },
                card: "Already Streaming".to_owned(),
                driver: "uvcvideo".to_owned(),
                bus_info: "usb-1".to_owned(),
                nodes: Vec::new(),
                backend: BackendKind::V4l2,
            },
            fd,
            stream: Some(StreamState {
                negotiated: NegotiatedStream {
                    pixel_format: PixelFormat::MJPG,
                    width: 1280,
                    height: 720,
                    bytes_per_line: 0,
                    size_image: 0,
                    interval: FrameInterval::Discrete {
                        numerator: 1,
                        denominator: 30,
                    },
                    adjustments: Vec::new(),
                },
                buffers: Vec::new(),
                frames_delivered: 0,
            }),
        })
    }

    #[test]
    fn refusing_a_second_stream_names_nobody_rather_than_the_process_making_the_refusal() {
        // N48 point 5: "naming this process's pid would invite a client to kill the daemon
        // it is talking to." A `Busy` holder list is what `terminate_holder` reads as the
        // pids that would free the camera, and the refusal below is for a stream *this*
        // process is running — so there is no other holder to name, and the one the walk
        // would find is the caller (note **N191**).
        let root = schema::paths::scratch_root().expect("a scratch root under target/");
        let node = root.join(format!("wch-self-busy-{}.node", std::process::id()));
        std::fs::File::create(node.as_std_path()).expect("a file this process holds");

        let mut camera = streaming_camera(&node).expect("a descriptor on a file we just made");
        let error = camera
            .start_stream(&StreamRequest::default())
            .expect_err("a camera that is already streaming refuses a second stream");

        let Error::Busy { holders, path } = &error else {
            panic!("a second stream must be refused as Busy, got {error}");
        };
        assert_eq!(path, &node);
        assert!(
            holders.is_empty(),
            "the refusal for a stream this process is running named {holders:?}"
        );

        // Non-vacuity, and the whole reason this test uses a file nobody else has open:
        // an empty list is a decision only if the walk had something to say. `camera` is
        // still alive here on purpose — it owns the descriptor the walk looks for, which
        // is exactly the descriptor the refusal was made while holding.
        let walked = holders::of(&node);
        let mine = i32::try_from(std::process::id()).expect("a pid fits in an i32");
        assert!(
            walked.iter().any(|holder| holder.pid == mine),
            "the /proc walk over a node this process holds did not name it, so the \
             assertion above would pass for the wrong reason: {walked:?}"
        );

        drop(camera);
        std::fs::remove_file(node.as_std_path()).expect("the scratch node is ours to remove");
    }

    /// One pass over a two-camera machine where one camera's capture node would not open.
    ///
    /// The `Busy` is the shape that makes the drop matter — an *availability* refusal, so
    /// the camera exists and is simply not describable right now (E3).
    fn a_pass_that_lost_a_camera() -> Probe {
        let node = |dev: &str, key: &str, card: &str, caps: u32| ProbedNode {
            dev_path: camino::Utf8PathBuf::from(dev),
            group_key: key.to_owned(),
            usb_id: None,
            serial: None,
            driver: "uvcvideo".to_owned(),
            card: card.to_owned(),
            bus_info: "usb-1".to_owned(),
            capabilities: 0x84a0_0001,
            device_caps: caps,
        };
        let mut probe = Probe {
            probed: vec![
                node("/dev/video0", "1-1:1.0", "The Readable One", 0x0420_0001),
                node("/dev/video1", "1-1:1.0", "The Readable One", 0x04a0_0000),
            ],
            unreadable: BTreeMap::new(),
            unbound: Vec::new(),
        };
        probe.unreadable.insert(
            "1-2:1.0".to_owned(),
            vec![(
                camino::Utf8PathBuf::from("/dev/video2"),
                Error::Busy {
                    path: camino::Utf8PathBuf::from("/dev/video2"),
                    holders: Vec::new(),
                },
            )],
        );
        probe
    }

    #[test]
    fn a_listing_and_the_hint_explaining_what_it_dropped_are_one_readings_two_halves() {
        // T1's whole reason for `diagnose` (note **N7**): "the cameras" and "why there
        // might be fewer than you expect" are two facts, and they have to be two facts
        // *about the same moment*. `probe_nodes` produces both on every pass, and this is
        // that pass answering both questions (note **N193**).
        let probe = a_pass_that_lost_a_camera();
        let cameras = enumerate::group(&probe.probed);
        let hints = hints_for(&probe);

        assert_eq!(cameras.len(), 1, "the group that read is listed");
        assert_eq!(cameras[0].card, "The Readable One");
        assert_eq!(hints.len(), 1, "the group that did not is explained");
        assert_eq!(hints[0].kind, HintKind::NodeUnreadable);
        assert!(hints[0].subject.contains("/dev/video2"), "{:?}", hints[0]);

        // The halves are complementary rather than merely both present: nothing the hints
        // name is in the listing, and nothing in the listing is unexplained. A hint about
        // a node this listing *does* describe would be the two readings disagreeing, which
        // is the shape the second probe could produce and this one cannot.
        for camera in &cameras {
            for node in &camera.nodes {
                assert!(
                    !hints
                        .iter()
                        .any(|hint| hint.subject.contains(node.path.as_str())),
                    "{} is both listed and reported unreadable",
                    node.path
                );
            }
        }
    }

    #[test]
    fn a_listing_leaves_the_pass_that_produced_it_for_the_diagnosis() {
        // The load-bearing half of note N193, and the one nothing could go red on: the
        // only arm below drove `remember` directly, so deleting the call inside
        // `enumerate` left every assertion in place and only a dead-code lint to notice
        // (note **N198**). `listing` is that link as a function, so a test can hand it a
        // pass and ask the object what it kept.
        let backend = V4l2Backend::new();
        let probe = a_pass_that_lost_a_camera();
        let cameras = backend.listing(probe);

        assert_eq!(cameras.len(), 1, "the group that read is listed");
        let hints = backend.diagnose();
        assert!(
            hints.iter().any(|hint| {
                hint.kind == HintKind::NodeUnreadable && hint.subject.contains("/dev/video2")
            }),
            "the listing did not leave its pass behind: {hints:?}"
        );
    }

    #[test]
    fn a_diagnosis_will_not_explain_a_listing_another_thread_asked_for() {
        // One `Arc<V4l2Backend>` serves a whole daemon and six paths reach `enumerate`
        // from three thread families, so a pass with no identity is a mailbox: a `wch_list`
        // could take the pass a `wch_photo`'s `resolve` had just left and present it as an
        // explanation of a listing it never saw — N193's own defect through another door
        // (note **N198**).
        //
        // The safe failure is losing the pass, and that is what this asserts: a `diagnose`
        // on a thread that did not take the pass finds nothing to take. It then reads the
        // machine, which on this host is whatever this host has — so the assertion is only
        // about the seeded node, which exists nowhere.
        let backend = std::sync::Arc::new(V4l2Backend::new());
        backend.remember(a_pass_that_lost_a_camera());

        let elsewhere = std::sync::Arc::clone(&backend);
        let hints = std::thread::spawn(move || elsewhere.diagnose())
            .join()
            .expect("the diagnosing thread finished");
        assert!(
            !hints
                .iter()
                .any(|hint| hint.subject.contains("/dev/video2")),
            "a diagnosis took a pass another thread was holding: {hints:?}"
        );

        // And the pass is still here for the thread it belongs to — losing it to a
        // stranger would be the repair costing the thing it was protecting.
        assert!(
            backend
                .diagnose()
                .iter()
                .any(|hint| hint.subject.contains("/dev/video2")),
            "the owning thread lost its own pass"
        );
    }

    #[test]
    fn a_pass_explains_the_cameras_nothing_is_driving_as_well_as_the_nodes_that_would_not_open() {
        // The other half of a hint, and it was still a second reading of the machine after
        // N193: `diagnose` walked `/sys/bus/usb/devices` freshly, so a camera whose driver
        // bound between the listing and the diagnosis was reported driverless by a pass
        // that had already seen it working (note **N198**).
        let backend = V4l2Backend::new();
        let mut probe = a_pass_that_lost_a_camera();
        probe.unbound = vec!["3-2".to_owned()];
        backend.listing(probe);

        let hints = backend.diagnose();
        assert!(
            hints.iter().any(|hint| {
                hint.kind == HintKind::DriverlessUsbVideoDevice && hint.subject == "3-2"
            }),
            "the driverless device came from somewhere other than the pass: {hints:?}"
        );
        assert!(
            hints
                .iter()
                .any(|hint| hint.kind == HintKind::NodeUnreadable),
            "and both halves still arrive: {hints:?}"
        );
    }

    #[test]
    fn diagnose_explains_the_listing_it_was_asked_about_rather_than_reading_the_machine_again() {
        // The half above states the property over a value; this states it over the object,
        // and it is the one that could not be true before. The seeded pass names a node
        // that is not on any host, so a `diagnose` that probed for itself cannot produce
        // it — and a `diagnose` that read the pass `enumerate` left must.
        let backend = V4l2Backend::new();
        backend.remember(a_pass_that_lost_a_camera());

        let named = |hints: &[ListHint]| {
            hints.iter().any(|hint| {
                hint.kind == HintKind::NodeUnreadable && hint.subject.contains("/dev/video2")
            })
        };
        assert!(
            named(&backend.diagnose()),
            "the diagnosis did not carry the pass it was given"
        );

        // And the pass is spent. It explains one listing; a later question with no listing
        // behind it gets an answer about the machine now, which on a host where every node
        // reads is no unreadable node at all.
        let again = backend.diagnose();
        assert!(
            !named(&again),
            "a spent pass explained a second question: {again:?}"
        );
    }

    #[test]
    fn the_only_refusal_a_walk_carries_is_the_one_the_uapi_makes_about_a_control() {
        // AGENTS rule 7 names four things that stay distinct — `EBUSY`, `ENODEV`, `EPERM`
        // and a timeout — and says "no code or test converts one into the other". The
        // first version of this tolerance converted three of the four into "this control
        // has no value" (note **N196**).
        //
        // `EBUSY` is the one that belongs here, and it belongs on its own evidence: the
        // UAPI documents it as `G_EXT_CTRLS`'s answer for a control whose *device
        // function* another application has taken over, which is a fact about one knob on
        // a camera that is answering every other query put to it.
        for (error, why) in [
            (
                Error::PermissionDenied {
                    path: camino::Utf8PathBuf::from("/dev/video0"),
                    hint: "join the video group".to_owned(),
                },
                "a permission refusal is not a control with no value",
            ),
            (
                Error::DeviceIo {
                    operation: "VIDIOC_G_EXT_CTRLS".to_owned(),
                    errno: Some(libc::EIO),
                    message: "Input/output error".to_owned(),
                },
                "an I/O error is not a control with no value",
            ),
            (
                // Our own defect, and the sharp reason the catch-all had to go: this is
                // what `sys::ioctl::short_reply` produces — "the kernel's reply was
                // shorter than the bindings describe" — so a bindgen or offset defect in
                // the one crate that carries `unsafe` used to read as an absent value.
                Error::DeviceIo {
                    operation: "VIDIOC_G_EXT_CTRLS".to_owned(),
                    errno: None,
                    message: "the kernel's reply was shorter than the bindings describe".to_owned(),
                },
                "a short reply is this build's bug, not the device's answer",
            ),
            (
                Error::DeviceGone {
                    path: camino::Utf8PathBuf::from("/dev/video0"),
                },
                "a device that has gone ends the walk",
            ),
        ] {
            assert_eq!(
                walked_current(None, Err(error.clone())),
                Err(error.clone()),
                "{why}: {error}"
            );
        }
    }

    #[test]
    fn a_walk_sent_for_one_control_answers_about_that_control_rather_than_tolerating_it() {
        // `describe` is `get` and `set`'s reading of the device, and both were handed one
        // id — so an availability fact about that id *is* their answer (note **N196**).
        // Tolerating it there is worse than it looks: `set` would proceed to write, the
        // read-back would then propagate the same refusal, and D3's `{requested, applied}`
        // pair would be lost from a write the device actually took.
        let busy = Error::Busy {
            path: camino::Utf8PathBuf::from("/dev/video0"),
            holders: Vec::new(),
        };
        assert_eq!(
            walked_current(Some(ControlId(9_963_776)), Err(busy.clone())),
            Err(busy.clone()),
            "a caller that named one control gets that control's availability answer"
        );
        // The same input, asked the enumeration's question, is carried.
        assert_eq!(walked_current(None, Err(busy)), Ok(None));
    }

    #[test]
    fn one_control_the_device_will_not_read_is_carried_valueless_rather_than_ending_the_walk() {
        // AGENTS rule 7 at the level D2 exists to protect: `controls()` answers "what can
        // this camera do", and a driver that declines one `G_EXT_CTRLS` has not said
        // anything about the other seventeen. Before this, every errno but `EINVAL` and
        // `EACCES` came out of the walk as the *device's* refusal (note **N192**).
        //
        // `EBUSY` is the sharp one and it is why this is not hypothetical: the UAPI's
        // answer for a control whose device function another application has taken over.
        // Reported through the walk it read as "another process holds this camera", from
        // a camera that was answering `QUERY_EXT_CTRL` for every control on it. It is also
        // the *only* refusal carried — `the_only_refusal_a_walk_carries_is_the_one_the_
        // uapi_makes_about_a_control` is the arm that holds the other three to rule 7.
        let busy = Error::Busy {
            path: camino::Utf8PathBuf::from("/dev/video0"),
            holders: vec![schema::error::Holder {
                pid: 4321,
                comm: Some("something-else".to_owned()),
            }],
        };
        assert_eq!(
            walked_current(None, Err(busy.clone())),
            Ok(None),
            "the walk ended on {busy}, which is a fact about one control"
        );

        // And a value that was read is still a value: the tolerance is about failures, and
        // a walk that answered `None` for everything would be the same defect wearing the
        // other sign.
        assert_eq!(
            walked_current(None, Ok(Some(ControlValue::Int(245)))),
            Ok(Some(ControlValue::Int(245)))
        );
        assert_eq!(walked_current(None, Ok(None)), Ok(None));
    }

    #[test]
    fn a_device_that_names_no_frame_interval_is_reported_as_saying_nothing_not_as_shape_zero() {
        // D2's "represent the unknown" and its limit. `Unknown { raw }` is the kernel's own
        // `type` discriminant preserved exactly, and a device that cleared
        // `V4L2_CAP_TIMEPERFRAME` gave no discriminant to preserve — so the value that used
        // to come out of here was a fabricated one (note **N194**). Zero is the worst
        // available fabrication: it is what a driver that filled in nothing would write, so
        // "the device said shape 0" and "the device said nothing" were the same answer.
        assert_eq!(stated_interval(None), FrameInterval::Unstated);
        assert_ne!(stated_interval(None), FrameInterval::Unknown { raw: 0 });

        // Anything the device *did* say is passed through untouched, including a shape
        // this build cannot read — that one really is `Unknown`, and it carries the
        // driver's own number.
        for said in [
            FrameInterval::Discrete {
                numerator: 1,
                denominator: 30,
            },
            FrameInterval::Unknown { raw: 99 },
        ] {
            assert_eq!(stated_interval(Some(said)), said);
        }
    }

    #[test]
    fn a_camera_that_vanished_mid_walk_ends_it_rather_than_listing_valueless_controls() {
        // The other direction, and the reason the tolerance above is not "ignore
        // everything": once the node is gone every remaining read fails the same way, so
        // the list that came out would describe a camera nobody can photograph as one
        // whose controls happen to have no values. That is E3's conversion with the
        // arguments swapped, and it is worse than the refusal because it looks like an
        // answer.
        let gone = Error::DeviceGone {
            path: camino::Utf8PathBuf::from("/dev/video0"),
        };
        assert_eq!(walked_current(None, Err(gone.clone())), Err(gone));
    }

    #[test]
    fn the_backend_reports_itself_as_v4l2_so_no_run_can_be_mistaken_for_a_fake_one() {
        let backend = V4l2Backend::new();
        assert_eq!(backend.kind(), BackendKind::V4l2);
        assert_eq!(backend.name(), "v4l2");
    }

    #[test]
    fn open_wants_an_exact_id_and_does_not_resolve_prefixes_of_its_own() {
        // D1's prefix rule lives in `engine::resolve`, over the whole enumeration. A
        // backend that also resolved would be a second opinion about what `cam:obsbot`
        // means, and the two could disagree the moment a second backend was attached.
        //
        // On a host with no camera this asserts the empty case; on one with cameras it
        // asserts a prefix of a real id is refused. Both are the same claim, and neither
        // needs particular hardware.
        let backend = V4l2Backend::new();
        let Ok(cameras) = backend.enumerate() else {
            return;
        };

        let asked = CameraId::parse("cam:nothing-answers-to-this").expect("literal id");
        assert!(
            matches!(backend.open(&asked), Err(Error::CameraUnknown { .. })),
            "an id nothing answers to must be CameraUnknown"
        );

        if let Some(first) = cameras.first() {
            // A strict prefix of a real id: `open` must refuse it rather than resolve it.
            let body = first.id.body();
            let Some(shortened) = body.get(..body.len().saturating_sub(1)) else {
                return;
            };
            if shortened.is_empty() || shortened == body {
                return;
            }
            let prefix = CameraId::parse(shortened).expect("a non-empty prefix");
            assert!(
                matches!(backend.open(&prefix), Err(Error::CameraUnknown { .. })),
                "{prefix} is a prefix of {}, and the backend resolved it",
                first.id
            );
        }
    }

    #[test]
    #[ignore = "R3: needs a camera attached; run with `just smoke-hw`"]
    fn hw_describing_one_control_says_what_the_whole_walk_says_about_it() {
        // The evidence docs/11 §8 P1's repair rests on, taken from the device rather than
        // reasoned from the UAPI (rule 4). `describe` no longer runs the enumeration to the
        // end — it stops at the id it was sent for — so "the walk's answer about a control"
        // and "a targeted walk's answer about that control" have to be the same value, for
        // every control on every camera this host has. The two things a wrong early stop
        // would produce are both caught here: a descriptor that differs, and an id the
        // targeted walk cannot find at all.
        //
        // It is also the arm that would price the *next* step. A direct `QUERY_EXT_CTRL`
        // with `NEXT_CTRL` cleared costs one ioctl instead of a prefix, and note **N192**
        // declines it because nothing here has ever made that call; this comparison is the
        // shape the measurement would take.
        let backend = V4l2Backend::new();
        let Ok(cameras) = backend.enumerate() else {
            return;
        };
        let mut compared = 0usize;
        let mut cameras_seen = 0usize;

        for info in &cameras {
            let Ok(camera) = V4l2Camera::open(info.clone()) else {
                continue;
            };
            let Ok(walked) = camera.controls() else {
                continue;
            };
            if walked.is_empty() {
                continue;
            }
            cameras_seen += 1;

            for desc in &walked {
                let described = camera.describe(desc.id).unwrap_or_else(|error| {
                    panic!(
                        "{}: the walk reports {} and `describe` answered {error}",
                        info.id, desc.slug
                    )
                });
                assert_eq!(
                    &described, desc,
                    "{}: the targeted walk and the whole walk disagree about {}",
                    info.id, desc.slug
                );
                compared += 1;
            }

            // And the other direction: an id no control has must be `ControlUnknown`
            // rather than the next control after it, which is what a targeted walk that
            // forgot to compare ids would answer.
            let highest = walked.iter().map(|desc| desc.id.0).max().unwrap_or(0);
            assert!(
                matches!(
                    camera.describe(ControlId(highest.saturating_add(1))),
                    Err(Error::ControlUnknown { .. })
                ),
                "{}: an id past every control it has was not ControlUnknown",
                info.id
            );
        }

        assert!(
            cameras_seen > 0,
            "no camera on this host reported any control, so nothing was compared"
        );
        println!("{compared} control(s) compared across {cameras_seen} camera(s)");
    }

    #[test]
    #[ignore = "R3: needs a camera attached; run with `just smoke-hw`"]
    fn hw_every_node_answers_the_control_and_format_ioctls_without_erroring() {
        // PF:15, at a level the public surface cannot reach: `open` picks a capture node,
        // and the bug only appears on a node that implements no control ioctl at all.
        //
        // The property is **`controls()` and `formats()` never fail**, on any node. An
        // earlier version asserted the stronger "a non-capture node reports no controls",
        // which is false and was caught the first time `vivid` was loaded: vivid's *video
        // output* nodes are not capture nodes and have 77 controls apiece. "Not a capture
        // node" and "implements no control ioctl" are different claims, and only the
        // second one is PF:15.
        let Ok(nodes) = sysfs::nodes() else {
            return;
        };
        let mut examined = 0usize;
        let mut non_capture = 0usize;
        let mut control_less = 0usize;

        for node in &nodes {
            let Ok(fd) = Fd::open(&node.dev_path) else {
                continue;
            };
            let Ok(cap) = ioctl::querycap(&fd) else {
                continue;
            };
            let is_capture = cap.device_caps & schema::camera::CAP_VIDEO_CAPTURE != 0;
            if !is_capture {
                non_capture += 1;
            }
            examined += 1;

            let camera = V4l2Camera {
                info: CameraInfo {
                    id: CameraId::parse("cam:probe").expect("literal id"),
                    fingerprint: schema::camera::CameraFingerprint {
                        bus_path: node.group_key().to_owned(),
                        usb_id: node.usb_id,
                        card: cap.card.clone(),
                        driver: cap.driver.clone(),
                        serial: node.serial.clone(),
                    },
                    card: cap.card,
                    driver: cap.driver,
                    bus_info: cap.bus_info,
                    nodes: Vec::new(),
                    backend: BackendKind::V4l2,
                },
                fd,
                stream: None,
            };

            let controls = camera.controls().unwrap_or_else(|error| {
                panic!(
                    "{}: controls() failed with {error}; a node that does not implement \
                     the control ioctls answers ENOTTY, which terminates the walk rather \
                     than failing it [PF:15]",
                    node.dev_path
                )
            });
            if controls.is_empty() {
                control_less += 1;
            }
            camera.formats().unwrap_or_else(|error| {
                panic!(
                    "{}: formats() failed with {error}; same terminator, same rule [PF:15]",
                    node.dev_path
                )
            });
        }

        assert!(examined > 0, "no node on this host could be opened");
        assert!(
            non_capture > 0,
            "every node on this host is a capture node, so the PF:15 path — a node `open` \
             would never pick — was not exercised"
        );
        // Non-vacuity for the finding itself: at least one node must answer *nothing*,
        // which on this hardware is the ENOTTY terminator rather than a device that
        // genuinely has no controls.
        assert!(
            control_less > 0,
            "no node reported an empty control set, so the ENOTTY terminator was never \
             reached"
        );
        println!(
            "{examined} node(s) answered both ioctls; {non_capture} non-capture, \
             {control_less} with no controls"
        );
    }

    #[test]
    fn a_group_missing_a_node_is_refused_rather_than_described_without_it() {
        // The E3 property, at the level it can be stated without a device: a camera is
        // only ever built from a group *all* of whose nodes answered.
        //
        // Why it matters is the shape of the failure it prevents. `NodeKind` comes from
        // `device_caps`, which only an open node reports. So a group whose capture node
        // could not be opened, described from its surviving metadata node, would have
        // `capture_node() == None` — and every caller above reads that as "this camera
        // cannot capture". A busy node would have answered a capability question.
        let node = |dev: &str, caps: u32| enumerate::ProbedNode {
            dev_path: camino::Utf8PathBuf::from(dev),
            group_key: "3-4:1.0".to_owned(),
            usb_id: None,
            serial: None,
            driver: "uvcvideo".to_owned(),
            card: "Test".to_owned(),
            bus_info: "usb-1".to_owned(),
            capabilities: 0x84a0_0001,
            device_caps: caps,
        };

        // Both nodes present: one camera, with a capture node.
        let whole = enumerate::group(&[
            node("/dev/video0", 0x0420_0001),
            node("/dev/video1", 0x04a0_0000),
        ]);
        assert_eq!(whole.len(), 1);
        assert!(whole[0].capture_node().is_some());

        // The capture node missing is exactly the dangerous shape — and it is what
        // `probe_nodes` now refuses to construct, because the camera it produces is a
        // truthful-looking lie.
        let partial = enumerate::group(&[node("/dev/video1", 0x04a0_0000)]);
        assert_eq!(partial.len(), 1);
        assert!(
            partial[0].capture_node().is_none(),
            "this is the misleading camera; `probe_nodes` must never hand one out"
        );
    }

    #[test]
    fn an_unreadable_node_is_reported_rather_than_dropped_in_silence() {
        // The other half of refusing the group: the refusal has to be visible. On a host
        // where every node reads, there is nothing to report — which is the case this
        // machine and CI both exercise, so the assertion is written both ways.
        //
        // **The order is the product's**, and it was the other way round until 2026-08-16:
        // `diagnose` before `enumerate` is the one pairing no caller in this workspace
        // makes, so on the only non-`#[ignore]`d test that drives a real machine this arm
        // was exercising the fallback that probes for itself and never the path
        // `engine::resolve::list` takes (note **N198**).
        let backend = V4l2Backend::new();
        let enumerated = backend.enumerate().map(|c| c.len()).unwrap_or(0);
        let hints = backend.diagnose();
        let unreadable = hints
            .iter()
            .filter(|hint| hint.kind == HintKind::NodeUnreadable)
            .count();

        for hint in hints.iter().filter(|h| h.kind == HintKind::NodeUnreadable) {
            assert!(
                !hint.subject.is_empty(),
                "an unreadable-node hint must name the node"
            );
            assert!(hint.message().contains("not about what the camera can do"));
        }

        // The claim that holds either way, and the one worth asserting: a host that
        // reported no unreadable node must have enumerated successfully. The two are the
        // same fact seen from both ends — `probe_nodes` returns the failure when nothing
        // read, and records a hint when something did.
        if unreadable == 0 {
            assert!(
                backend.enumerate().is_ok(),
                "no node was reported unreadable, yet enumeration failed"
            );
        }
        println!("{enumerated} camera(s), {unreadable} unreadable node(s)");
    }

    #[test]
    fn enumeration_survives_whatever_this_host_has_attached_including_nothing() {
        // Not an assertion about hardware — CI has none. What it pins is that
        // enumeration never panics and never invents a camera, which is the PF:1 lesson
        // stated at the top of the stack rather than the bottom.
        let backend = V4l2Backend::new();
        match backend.enumerate() {
            Ok(cameras) => {
                for camera in &cameras {
                    assert_eq!(camera.backend, BackendKind::V4l2);
                    assert!(!camera.nodes.is_empty(), "{camera:?}");
                    assert!(!camera.fingerprint.bus_path.is_empty(), "{camera:?}");
                }
            }
            // A machine whose nodes all refuse to open answers with the actionable error
            // rather than with an empty list.
            Err(error) => assert!(
                matches!(
                    error,
                    Error::PermissionDenied { .. } | Error::Busy { .. } | Error::DeviceGone { .. }
                ),
                "enumeration failed for an unexpected reason: {error}"
            ),
        }
    }
}
