//! The V4L2 backend.
//!
//! This is the one crate in the workspace without `#![forbid(unsafe_code)]`: talking to
//! the kernel means ioctls and mmap. The token `unsafe` is confined to `src/sys/` by
//! `scripts/gates/unsafe-scope.sh`, which derives the allowed path from the tree.
//!
//! ## What has landed, and what has not
//!
//! P1 landed the **read path** (docs/2): enumeration, the control model, and the format
//! tree. P2 adds the **write and capture paths** — `set` with the read-back D3 requires,
//! and mmap streaming with the format negotiation D5 requires. **Hotplug is the last one
//! left**, and it arrives at P4 with the daemon's uevent socket. The T2 trait is total, so
//! the one method that has not landed returns [`schema::Error::Unimplemented`] naming
//! itself and the phase that lands it — *not* a device error, because the device was never
//! asked, and not a panic, because plugging in a webcam must never be able to panic a
//! library. See note N6, and [`the pinning test`](self#tests) which fails when that set
//! changes.
//!
//! ## The layering
//!
//! | Module | Owns |
//! |---|---|
//! | `sys` | ioctls, mmap, the bounded wait, and the pure byte-to-schema decoding Miri executes |
//! | `sysfs` | the node list and the bus-interface topology, read without udev |
//! | `enumerate` | the pure grouping rule: nodes to cameras \[PF:7, PF:13\] |
//!
//! Nothing above `src/sys/` names a kernel type; `scripts/gates/dependency-walls.sh` holds
//! that line for the rest of the workspace by refusing `v4l::` outside this crate.
// Kernel-shaped integers are converted with `try_from`, never `as` (rubric B10).
#![deny(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap
)]
// docs/4's "device/request-driven paths" lint set. Every path in this crate answers a
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
mod holders;
mod sys;
mod sysfs;

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
/// The phase that lands hotplug (design §2.6: the uevent socket arrives with the daemon).
const HOTPLUG_PHASE: &str = "P4";

/// Real cameras, through V4L2.
#[derive(Debug, Default)]
pub struct V4l2Backend {
    _private: (),
}

impl V4l2Backend {
    /// A backend reading this machine's devices.
    #[must_use]
    pub fn new() -> V4l2Backend {
        V4l2Backend { _private: () }
    }
}

impl CameraBackend for V4l2Backend {
    fn kind(&self) -> BackendKind {
        BackendKind::V4l2
    }

    fn enumerate(&self) -> Result<Vec<CameraInfo>> {
        Ok(enumerate::group(&probe_nodes()?.probed))
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
        Err(unimplemented_here("CameraBackend::watch", HOTPLUG_PHASE))
    }

    fn diagnose(&self) -> Vec<ListHint> {
        let mut hints: Vec<ListHint> = sysfs::unbound_video_devices()
            .into_iter()
            .map(|device| ListHint {
                kind: HintKind::DriverlessUsbVideoDevice,
                subject: device,
            })
            .collect();

        // The cameras `enumerate` declined to describe. Without this the drop would be
        // silent, and a camera vanishing because one of its nodes was busy is exactly the
        // kind of absence a user needs told rather than left to infer.
        if let Ok(probe) = probe_nodes() {
            for (path, error) in probe.unreadable.values().flatten() {
                hints.push(ListHint {
                    kind: HintKind::NodeUnreadable,
                    subject: format!("{path}: {error}"),
                });
            }
        }
        hints
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
    let mut probe = Probe::default();
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
    fn describe(&self, id: ControlId) -> Result<ControlDesc> {
        let controls = self.controls()?;
        controls
            .into_iter()
            .find(|desc| desc.id == id)
            .ok_or_else(|| Error::ControlUnknown {
                requested: id.to_string(),
                did_you_mean: Vec::new(),
            })
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

            // A control whose name slugs to nothing has no handle D2 will invent; the
            // walk steps past it rather than stopping, so it cannot hide the rest.
            let Some(mut desc) = walked.desc else {
                continue;
            };
            self.read_menu(&mut desc)?;
            desc.current = self.read_current(&desc)?;
            controls.push(desc);
        }

        Ok(controls)
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
            return Err(Error::Busy {
                holders: holders::of(self.fd.path()),
                path: self.fd.path().to_owned(),
            });
        }
        // A metadata-only camera is a shape D1 supports on purpose: it is listed, and
        // streaming it is a typed refusal rather than a surprise.
        if self.info.capture_node().is_none() {
            return Err(Error::FormatUnsupported {
                requested: request.pixel_format,
                available: Vec::new(),
            });
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
    fn negotiate(&self, request: &StreamRequest) -> Result<NegotiatedStream> {
        let formats = self.formats()?;
        let chosen = request
            .choose(&formats)
            .ok_or_else(|| Error::FormatUnsupported {
                requested: request.pixel_format,
                available: formats.iter().map(|f| f.pixel_format).collect(),
            })?;
        let wanted = ioctl::set_format(
            &self.fd,
            chosen.pixel_format.to_fourcc(),
            chosen.width,
            chosen.height,
        )?;

        // The interval is a separate negotiation with its own capability bit, and a
        // device that does not implement it has said so rather than failed: `interval`
        // then reports `Unknown`, which is D2's "represent what you cannot interpret"
        // rather than a fabricated 30 fps.
        let interval = match request.interval {
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
        let interval = interval.unwrap_or(FrameInterval::Unknown { raw: 0 });

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
    /// The nesting is not decoration: the OBSBOT offers MJPG to 3840×2160 while YUYV
    /// stops at 640×480 on the same cable, so a flat list would be a lie \[PF:9\].
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

/// The typed refusal for a trait method this phase has not landed.
fn unimplemented_here(operation: &str, arrives_in: &str) -> Error {
    Error::Unimplemented {
        operation: operation.to_owned(),
        arrives_in: arrives_in.to_owned(),
    }
}

/// The methods that answer [`Error::Unimplemented`] in this build, and the phase each
/// waits for. Pinned by a test, so a phase cannot land without emptying its rows.
#[must_use]
pub fn unimplemented_surface() -> BTreeMap<&'static str, &'static str> {
    BTreeMap::from([("CameraBackend::watch", HOTPLUG_PHASE)])
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

    #[test]
    fn the_unfinished_half_of_the_trait_is_named_and_says_which_phase_lands_it() {
        // The pin, one row from empty. P2 deleted four of the five; note N6 says the
        // `Unimplemented` variant retires when P4 deletes the last one, and this test is
        // what makes that a compile-time obligation rather than a memory.
        let surface = unimplemented_surface();
        assert_eq!(surface.len(), 1);
        for landed in [
            "Camera::set",
            "Camera::start_stream",
            "Camera::next_frame",
            "Camera::stop_stream",
        ] {
            assert_eq!(surface.get(landed), None, "{landed} landed at P2");
        }
        assert_eq!(surface.get("CameraBackend::watch"), Some(&"P4"));

        // Every entry renders as the D13 refusal, blaming this build rather than the
        // device — the distinction N6 exists for.
        for (operation, phase) in &surface {
            let error = unimplemented_here(operation, phase);
            assert_eq!(error.kind(), schema::error::ErrorKind::Unimplemented);
            let rendered = error.to_string();
            assert!(rendered.contains(operation), "{rendered}");
            assert!(rendered.contains(phase), "{rendered}");
        }
    }

    #[test]
    fn the_backend_reports_itself_as_v4l2_so_no_run_can_be_mistaken_for_a_fake_one() {
        let backend = V4l2Backend::new();
        assert_eq!(backend.kind(), BackendKind::V4l2);
        assert_eq!(backend.name(), "v4l2");
    }

    #[test]
    fn watch_refuses_with_the_phase_that_lands_it_rather_than_pretending_to_be_quiet() {
        // A watch that returned `Ok(None)` forever would be indistinguishable from a
        // working one on a quiet machine, which is exactly the "skip reads as pass"
        // failure in a different costume.
        let error = V4l2Backend::new()
            .watch()
            .expect_err("watch is not implemented at P1");
        assert_eq!(error.kind(), schema::error::ErrorKind::Unimplemented);
        assert!(error.to_string().contains("P4"), "{error}");
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
        let backend = V4l2Backend::new();
        let hints = backend.diagnose();
        let enumerated = backend.enumerate().map(|c| c.len()).unwrap_or(0);
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
