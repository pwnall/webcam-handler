//! The V4L2 backend.
//!
//! This is the one crate in the workspace without `#![forbid(unsafe_code)]`: talking to
//! the kernel means ioctls and mmap. The token `unsafe` is confined to `src/sys/` by
//! `scripts/gates/unsafe-scope.sh`, which derives the allowed path from the tree.
//!
//! ## What P1 lands, and what it does not
//!
//! P1 is the **read path** (docs/2): enumeration, the control model, and the format tree.
//! Writes, streaming and hotplug arrive at P2 and P4 with their own gates. The T2 trait
//! is total, so the methods that have not landed return
//! [`schema::Error::Unimplemented`] naming themselves and the phase that lands them —
//! *not* a device error, because the device was never asked, and not a panic, because
//! plugging in a webcam must never be able to panic a library. See note N6, and
//! [`the pinning test`](self#tests) which fails when that set changes.
//!
//! ## The layering
//!
//! | Module | Owns |
//! |---|---|
//! | `sys` | ioctls, and the pure byte-to-schema decoding Miri executes |
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
mod sys;
mod sysfs;

use std::collections::BTreeMap;
use std::time::Instant;

use camino::Utf8Path;
use schema::backend::{BackendKind, Camera, CameraBackend, HotplugWatch};
use schema::camera::{CameraId, CameraInfo, FormatInfo, FrameSizeInfo, PixelFormat};
use schema::capture::{Frame, NegotiatedStream, StreamRequest};
use schema::control::{Applied, ControlDesc, ControlId, ControlType, ControlValue, KnownFlag};
use schema::error::{Error, Result};
use schema::limits;
use schema::report::{HintKind, ListHint};

use enumerate::ProbedNode;
use sys::{Fd, ioctl};

/// The phase that lands the half of the T2 surface P1 does not.
const WRITE_PATH_PHASE: &str = "P2";
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

/// One open camera.
#[derive(Debug)]
pub struct V4l2Camera {
    info: CameraInfo,
    fd: Fd,
}

impl V4l2Camera {
    /// Open the node frames come from, or — for a metadata-only camera — its first node,
    /// so `controls` and `formats` can still answer for it.
    fn open(info: CameraInfo) -> Result<V4l2Camera> {
        let path = Self::working_node(&info)?.to_owned();
        let fd = Fd::open(&path)?;
        Ok(V4l2Camera { info, fd })
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
        let mut previous = 0u32;

        for _ in 0..limits::MAX_CONTROLS_PER_DEVICE {
            let walked = match ioctl::query_ext_ctrl(&self.fd, previous)? {
                ioctl::Enumerated::Exhausted => break,
                ioctl::Enumerated::Entry(walked) => walked,
            };
            // `NEXT_CTRL` promises strictly increasing ids. A driver that repeats one
            // would otherwise spin here until the cap, reporting the same control over
            // and over; stopping is the honest response to a device contradicting itself.
            if walked.id <= previous && previous != 0 {
                break;
            }
            previous = walked.id;

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

    fn set(&mut self, _id: ControlId, _value: ControlValue) -> Result<Applied> {
        Err(unimplemented_here("Camera::set", WRITE_PATH_PHASE))
    }

    fn start_stream(&mut self, _request: &StreamRequest) -> Result<NegotiatedStream> {
        Err(unimplemented_here("Camera::start_stream", WRITE_PATH_PHASE))
    }

    fn next_frame(&mut self, _deadline: Instant) -> Result<Frame> {
        Err(unimplemented_here("Camera::next_frame", WRITE_PATH_PHASE))
    }

    fn stop_stream(&mut self) -> Result<()> {
        Err(unimplemented_here("Camera::stop_stream", WRITE_PATH_PHASE))
    }
}

impl V4l2Camera {
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
/// waits for. Pinned by a test, so P2 cannot land without emptying its half.
#[must_use]
pub fn unimplemented_surface() -> BTreeMap<&'static str, &'static str> {
    BTreeMap::from([
        ("Camera::set", WRITE_PATH_PHASE),
        ("Camera::start_stream", WRITE_PATH_PHASE),
        ("Camera::next_frame", WRITE_PATH_PHASE),
        ("Camera::stop_stream", WRITE_PATH_PHASE),
        ("CameraBackend::watch", HOTPLUG_PHASE),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_unfinished_half_of_the_trait_is_named_and_says_which_phase_lands_it() {
        // The pin. When P2 implements the write path it must delete four rows here, and
        // this test is what makes forgetting impossible.
        let surface = unimplemented_surface();
        assert_eq!(surface.len(), 5);
        assert_eq!(surface.get("Camera::set"), Some(&"P2"));
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
    fn hw_a_node_that_implements_no_control_ioctl_answers_empty_rather_than_erroring() {
        // PF:15, at a level the public surface cannot reach. Every metadata node on the
        // seed hardware answers ENOTTY to QUERY_EXT_CTRL — "this node does not implement
        // that ioctl" — which is a terminator, not a failure. A build accepting only
        // EINVAL reports a metadata-only camera's control set as a device error.
        //
        // This lives in the crate rather than in `tests/` because `V4l2Camera` is private
        // and the bug is only reachable through a node `open` would never pick on
        // hardware that also has a capture node.
        let Ok(nodes) = sysfs::nodes() else {
            return;
        };
        let mut metadata_nodes = 0usize;
        for node in &nodes {
            let Ok(fd) = Fd::open(&node.dev_path) else {
                continue;
            };
            let Ok(cap) = ioctl::querycap(&fd) else {
                continue;
            };
            if cap.device_caps & schema::camera::CAP_VIDEO_CAPTURE != 0 {
                continue;
            }
            metadata_nodes += 1;

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
            };

            let controls = camera.controls().unwrap_or_else(|error| {
                panic!(
                    "{}: controls() failed with {error}; ENOTTY from a node that does not \
                     implement the control ioctls is a terminator, not a failure [PF:15]",
                    node.dev_path
                )
            });
            assert!(
                controls.is_empty(),
                "{}: a node with no control ioctl reported {} control(s)",
                node.dev_path,
                controls.len()
            );
            assert!(
                camera
                    .formats()
                    .expect("formats() must not fail either")
                    .is_empty(),
                "{}: a node with no capture capability reported formats",
                node.dev_path
            );
        }
        assert!(
            metadata_nodes > 0,
            "this host exposes no non-capture node, so PF:15 was not exercised"
        );
        println!("{metadata_nodes} non-capture node(s) answered empty rather than erroring");
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
