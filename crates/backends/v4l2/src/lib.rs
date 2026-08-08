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
        Ok(enumerate::group(&probe_nodes()?))
    }

    fn open(&self, id: &CameraId) -> Result<Box<dyn Camera>> {
        let cameras = self.enumerate()?;
        let info = resolve(&cameras, id)?;
        Ok(Box::new(V4l2Camera::open(info)?))
    }

    fn watch(&self) -> Result<Box<dyn HotplugWatch>> {
        Err(unimplemented_here("CameraBackend::watch", HOTPLUG_PHASE))
    }

    fn diagnose(&self) -> Vec<ListHint> {
        sysfs::unbound_video_devices()
            .into_iter()
            .map(|device| ListHint {
                kind: HintKind::DriverlessUsbVideoDevice,
                subject: device,
            })
            .collect()
    }
}

/// Resolve a caller-supplied id or prefix against the enumerated cameras (D1).
fn resolve(cameras: &[CameraInfo], id: &CameraId) -> Result<CameraInfo> {
    let ids: Vec<CameraId> = cameras.iter().map(|c| c.id.clone()).collect();
    match schema::camera::resolve_prefix(&ids, id.as_str()) {
        schema::camera::PrefixMatch::Unique(found) => cameras
            .iter()
            .find(|c| c.id == found)
            .cloned()
            .ok_or_else(|| Error::CameraUnknown {
                requested: id.to_string(),
            }),
        schema::camera::PrefixMatch::None => Err(Error::CameraUnknown {
            requested: id.to_string(),
        }),
        schema::camera::PrefixMatch::Ambiguous(candidates) => Err(Error::CameraAmbiguous {
            requested: id.to_string(),
            candidates,
        }),
    }
}

/// Read every node's sysfs facts and ask each one what it is.
///
/// A node that cannot be opened is left out of its group rather than failing the whole
/// enumeration — one camera another process is misbehaving with must not hide the rest.
/// But if **nothing** could be opened, the first failure is returned: the overwhelmingly
/// common cause is a missing `video` group membership, and answering "no cameras" to that
/// would hide the one error whose message says what to do about it.
fn probe_nodes() -> Result<Vec<ProbedNode>> {
    let nodes = sysfs::nodes()?;
    let mut probed = Vec::with_capacity(nodes.len());
    let mut first_failure = None;

    for node in &nodes {
        match probe_one(node) {
            Ok(entry) => probed.push(entry),
            Err(error) => {
                if first_failure.is_none() {
                    first_failure = Some(error);
                }
            }
        }
    }

    match first_failure {
        Some(error) if probed.is_empty() && !nodes.is_empty() => Err(error),
        _ => Ok(probed),
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
/// enumeration — unless the failure was about the *device* rather than the control.
///
/// `EINVAL` here means "this control has no readable current value", which several
/// drivers say about controls they still enumerate. `EBUSY`, `ENODEV` and a permission
/// refusal are facts about availability (E3) and must not be flattened into "no value".
fn unreadable_current(error: Error) -> Result<Option<ControlValue>> {
    match error {
        Error::DeviceIo { errno, .. } if errno == Some(libc::EINVAL) => Ok(None),
        Error::DeviceIo { errno, .. } if errno == Some(libc::EACCES) => Ok(None),
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
                // A size described with a `type` this build cannot represent. The entry
                // exists and we will not invent dimensions for it; the walk continues so
                // the sizes after it are not lost too.
                ioctl::Enumerated::Entry(None) => continue,
                ioctl::Enumerated::Entry(Some(size)) => size,
            };
            let (width, height) = size.max_dimensions();
            sizes.push(FrameSizeInfo {
                size,
                intervals: self.intervals_for(fourcc, width, height)?,
            });
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
                ioctl::Enumerated::Entry(None) => continue,
                ioctl::Enumerated::Entry(Some(interval)) => intervals.push(interval),
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
    fn an_id_nothing_answers_to_is_unknown_and_an_ambiguous_prefix_names_its_candidates() {
        let cameras = |cards: &[&str]| -> Vec<CameraInfo> {
            let ids = schema::camera::assign_ids(
                &cards.iter().map(|c| (*c).to_owned()).collect::<Vec<_>>(),
            );
            cards
                .iter()
                .zip(ids)
                .map(|(card, id)| CameraInfo {
                    id,
                    fingerprint: schema::camera::CameraFingerprint {
                        bus_path: "1-1:1.0".to_owned(),
                        usb_id: None,
                        card: (*card).to_owned(),
                        driver: "uvcvideo".to_owned(),
                        serial: None,
                    },
                    card: (*card).to_owned(),
                    driver: "uvcvideo".to_owned(),
                    bus_info: "usb-1".to_owned(),
                    nodes: Vec::new(),
                    backend: BackendKind::V4l2,
                })
                .collect()
        };

        let two = cameras(&["Webcam", "Webcam"]);
        let asked = CameraId::parse("cam:web").expect("literal id");
        match resolve(&two, &asked) {
            Err(Error::CameraAmbiguous { candidates, .. }) => assert_eq!(candidates.len(), 2),
            other => panic!("expected ambiguity, got {other:?}"),
        }

        let asked = CameraId::parse("cam:nope").expect("literal id");
        assert!(matches!(
            resolve(&two, &asked),
            Err(Error::CameraUnknown { .. })
        ));

        // And a prefix that does resolve, resolves.
        let one = cameras(&["OBSBOT Tiny 3"]);
        let asked = CameraId::parse("cam:obsbot").expect("literal id");
        assert_eq!(resolve(&one, &asked).expect("resolves").id, one[0].id);
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
