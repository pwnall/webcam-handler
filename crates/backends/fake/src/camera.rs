//! One replayed camera: the control graph and the stream (T2).
//!
//! Everything here is a *replay* of a captured profile, and E5 is the constraint that
//! shapes it: **a fake capability no real device exhibits is a bug in the fake.** So the
//! ranges, menus, flags and out-of-range values all come from the profile, the clamping
//! behaviour is PF:6's, the INACTIVE coupling is PF:3's and fires only for pairs the
//! profile records as *measured*, and the menu holes are refused the way PF:2 says a
//! driver refuses them.
//!
//! What the model deliberately does **not** invent:
//!
//! - It does not refuse a write to an INACTIVE control. Real hardware accepts it and
//!   then lets the automation overwrite the value on the next frame \[PF:3\]; refusing
//!   would be a capability no device has, and it would hide the fact that avoiding the
//!   futile write is the *engine's* job (D3's guarded set).
//! - It does not correct an out-of-range current or default. `zoom_continuous` reads back
//!   245 out of a declared `[-100..100]` for as long as nobody writes it \[PF:4, PF:5\].
//! - It does not couple controls the profile never measured. The declared pairing table
//!   is a *nomination* (E1); a fake that coupled on nominations would be asserting
//!   behaviour nobody observed on the device it claims to replay.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Instant;

use camino::Utf8PathBuf;
use schema::backend::{BackendKind, Camera};
use schema::camera::{CameraId, CameraInfo, FormatInfo, FrameInterval, FrameSizeInfo, PixelFormat};
use schema::capture::{Adjustment, Frame, NegotiatedStream, StreamRequest};
use schema::control::{
    Applied, ControlDesc, ControlFlags, ControlId, ControlRange, ControlSlug, ControlType,
    ControlValue, KnownFlag, WriteWarning,
};
use schema::error::{Error, Result};
use schema::limits;
use schema::pairing::AutomationPair;
use schema::profile::DeviceProfile;

use crate::fault::{Fault, FaultQueue};
use crate::frames::{self, Knob, Scene};

/// `EINVAL`, the errno a driver returns for a value it will not take. Spelled out rather
/// than pulled from `libc`: this crate links nothing but schema and imaging (§2.8).
const EINVAL: i32 = 22;

/// The ioctl a control write would have gone through on real hardware. Error messages
/// name it so a fake-backend failure reads like the hardware failure it stands in for.
const SET_CTRL_OP: &str = "VIDIOC_S_EXT_CTRLS";

/// The ioctl a frame read would have gone through.
const DQBUF_OP: &str = "VIDIOC_DQBUF";

/// The frame interval used when a profile records a size with no intervals at all.
const FALLBACK_INTERVAL: FrameInterval = FrameInterval::Discrete {
    numerator: 1,
    denominator: 30,
};

/// Take a lock, recovering from poisoning.
///
/// A poisoned lock here means a test panicked while holding it; a second panic would
/// replace a useful failure with a confusing one, and `unwrap` is banned on
/// request-driven paths anyway.
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Whether `fault` fires now.
pub(crate) fn take_fault(faults: &Mutex<FaultQueue>, fault: Fault) -> bool {
    lock(faults).take(fault)
}

/// The live device behind one or more open handles.
///
/// Shared rather than copied per handle: two `open` calls on one camera are two views of
/// one device, and a fake where the second handle could not see the first one's writes
/// would be modelling something that does not exist.
#[derive(Debug)]
pub(crate) struct CameraState {
    info: CameraInfo,
    profile: DeviceProfile,
    /// Enumeration order, which is the profile's order — `controls()` must not resort
    /// the device's own listing.
    order: Vec<ControlId>,
    controls: BTreeMap<ControlId, ControlDesc>,
    /// Only the pairs the profile *measured* (E1, E5).
    pairs: Vec<AutomationPair>,
    stream: Option<NegotiatedStream>,
    frames_delivered: u32,
    timestamp_us: i64,
}

impl CameraState {
    /// Build the live state from a captured profile.
    pub(crate) fn from_profile(profile: DeviceProfile, id: CameraId) -> CameraState {
        let mut info = profile.invariant.info.clone();
        info.id = id;
        // A fake-backend run must never be mistakable for a hardware one, whatever the
        // captured document said (the profile's own provenance block keeps the record of
        // where it came from).
        info.backend = BackendKind::Fake;

        let mut order = Vec::with_capacity(profile.invariant.controls.len());
        let mut controls = BTreeMap::new();
        for invariant in &profile.invariant.controls {
            // The state block folded back in: current values and the INACTIVE-class flags
            // as the capture caught them (T3).
            let live = profile
                .live_control(&invariant.slug)
                .unwrap_or_else(|| invariant.clone());
            let live = with_initial_value(live);
            order.push(live.id);
            controls.insert(live.id, live);
        }

        let pairs = profile.invariant.measured_pairs.clone();
        CameraState {
            info,
            profile,
            order,
            controls,
            pairs,
            stream: None,
            frames_delivered: 0,
            timestamp_us: 0,
        }
    }

    pub(crate) fn info(&self) -> &CameraInfo {
        &self.info
    }

    /// The document this camera replays. The resemblance tests read their expectations
    /// from here rather than from a second copy written by hand (E5).
    pub(crate) fn profile(&self) -> &DeviceProfile {
        &self.profile
    }

    fn descriptors(&self) -> Vec<ControlDesc> {
        self.order
            .iter()
            .filter_map(|id| self.controls.get(id).cloned())
            .collect()
    }

    fn id_of(&self, slug: &ControlSlug) -> Option<ControlId> {
        self.controls
            .values()
            .find(|desc| &desc.slug == slug)
            .map(|desc| desc.id)
    }

    /// The slugs a caller who named something unknown might have meant.
    fn known_slugs(&self) -> Vec<ControlSlug> {
        self.order
            .iter()
            .filter_map(|id| self.controls.get(id))
            .map(|desc| desc.slug.clone())
            .collect()
    }

    /// Write one control, exactly as a UVC driver would.
    fn write(
        &mut self,
        id: ControlId,
        requested: &ControlValue,
        force_clamp: bool,
    ) -> Result<Applied> {
        let desc = self
            .controls
            .get(&id)
            .cloned()
            .ok_or_else(|| Error::ControlUnknown {
                requested: id.to_string(),
                did_you_mean: self.known_slugs(),
            })?;

        // PF:12 — the Chicony's `Privacy`. Also the resting place for DISABLED and for
        // control-class headers: D13 has one "you may not write this" variant and it
        // names the control, which is what a caller needs either way.
        if !desc.is_writable() {
            return Err(Error::ControlReadOnly { control: desc.slug });
        }

        let (applied, warnings) = if desc.control_type == ControlType::Button {
            // A button has no value; writing it is the whole effect.
            (requested.clone(), Vec::new())
        } else if desc.control_type.is_menu() {
            (menu_write(&desc, requested)?, Vec::new())
        } else if desc.control_type.is_scalar() {
            scalar_write(&desc, requested, force_clamp)?
        } else if desc.control_type == ControlType::String {
            (text_write(&desc, requested)?, Vec::new())
        } else {
            (payload_write(&desc, requested)?, Vec::new())
        };

        if desc.control_type != ControlType::Button
            && let Some(entry) = self.controls.get_mut(&id)
        {
            entry.current = Some(applied.clone());
        }
        self.apply_coupling(id);

        Ok(Applied {
            control: id,
            slug: desc.slug,
            requested: requested.clone(),
            applied,
            warnings,
        })
    }

    /// PF:3, live and in both directions: switching an automation control on sets the
    /// INACTIVE flag on the manual partners the profile measured for it, and switching it
    /// off clears theirs. Nothing else moves.
    fn apply_coupling(&mut self, automation_id: ControlId) {
        let Some(automation) = self.controls.get(&automation_id).cloned() else {
            return;
        };
        let Some(current) = automation.current.as_ref().and_then(ControlValue::as_int) else {
            return;
        };

        for pair in self.pairs.clone() {
            if pair.automation != automation.slug {
                continue;
            }
            // No resolvable "off" value means we cannot tell engaged from disengaged, and
            // guessing would be inventing behaviour (D2's line, applied to the fake).
            let Some(off) = pair.off.resolve(&automation) else {
                continue;
            };
            let Some(partner) = self.id_of(&pair.manual) else {
                continue;
            };
            self.set_inactive(partner, current != off);
        }
    }

    /// The [`Fault::InactiveFlip`] fault: the device changes a flag behind our back.
    fn flip_measured_partners(&mut self) {
        let partners: Vec<ControlId> = self
            .pairs
            .iter()
            .filter_map(|pair| self.id_of(&pair.manual))
            .collect();
        for id in partners {
            let inactive = self
                .controls
                .get(&id)
                .is_some_and(schema::control::ControlDesc::is_inactive);
            self.set_inactive(id, !inactive);
        }
    }

    fn set_inactive(&mut self, id: ControlId, inactive: bool) {
        if let Some(desc) = self.controls.get_mut(&id) {
            let bit = KnownFlag::Inactive.bit();
            let raw = if inactive {
                desc.flags.raw | bit
            } else {
                desc.flags.raw & !bit
            };
            desc.flags = ControlFlags::from_raw(raw);
        }
    }

    /// The value of a control named by slug, for the frame model.
    fn knob(&self, slug: &str) -> Option<Knob> {
        let slug = ControlSlug::parse(slug)?;
        let desc = self.controls.values().find(|desc| desc.slug == slug)?;
        Some(Knob {
            value: desc.current.as_ref().and_then(ControlValue::as_int)?,
            range: desc.range,
            default: desc.default,
        })
    }

    /// The device node frames would have come from, for error messages that name a path.
    pub(crate) fn capture_path(&self) -> Utf8PathBuf {
        self.info
            .capture_node()
            .map_or_else(|| Utf8PathBuf::from("/dev/null"), |node| node.path.clone())
    }
}

/// Give a control a value to read back when the capture recorded none.
///
/// A control with no current is a control `get` cannot answer for, and the profile's
/// state block does not always cover every entry. Scalars fall back to their declared
/// default — including a default outside its own range, which is replayed rather than
/// corrected \[PF:5\].
fn with_initial_value(mut desc: ControlDesc) -> ControlDesc {
    if desc.current.is_some() {
        return desc;
    }
    let initial = match desc.control_type {
        // A button and a class header have no value to read.
        ControlType::Button | ControlType::ControlClass => None,
        ControlType::String => Some(ControlValue::Text(String::new())),
        other if other.is_scalar() => Some(ControlValue::Int(desc.default)),
        _ => Some(ControlValue::Bytes(vec![0; payload_len(&desc)])),
    };
    desc.current = initial;
    desc
}

/// A compound control's payload length, bounded by the limits table because
/// `elem_size × elems` is device-supplied and a lying descriptor must not become an
/// allocation (rubric B10, on this side of the ioctl too).
fn payload_len(desc: &ControlDesc) -> usize {
    let elems = usize::try_from(desc.elems).unwrap_or(0);
    let elem_size = usize::try_from(desc.elem_size).unwrap_or(0);
    elems
        .saturating_mul(elem_size)
        .min(limits::MAX_CONTROL_PAYLOAD_BYTES)
}

/// PF:2 — a menu index that is a hole is refused the way `VIDIOC_S_EXT_CTRLS` refuses it,
/// not silently accepted. The Chicony's `Auto Exposure` has no index 2, and a fake that
/// took one would let a planner ship code no device runs.
fn menu_write(desc: &ControlDesc, requested: &ControlValue) -> Result<ControlValue> {
    let value = requested
        .as_int()
        .ok_or_else(|| invalid_write(desc, "a menu control takes an integer index".to_owned()))?;
    let index = u32::try_from(value)
        .ok()
        .filter(|i| desc.menu.contains_key(i));
    match index {
        Some(_) => Ok(ControlValue::Int(value)),
        None => Err(invalid_write(
            desc,
            format!(
                "{value} is not one of this menu's indices ({})",
                desc.menu
                    .keys()
                    .map(u32::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        )),
    }
}

/// PF:6 — an out-of-range write succeeds and applies the clamped value, with a warning.
/// It is not an error, because the driver did not report one.
fn scalar_write(
    desc: &ControlDesc,
    requested: &ControlValue,
    force_clamp: bool,
) -> Result<(ControlValue, Vec<WriteWarning>)> {
    let value = requested
        .as_int()
        .ok_or_else(|| invalid_write(desc, "a scalar control takes an integer value".to_owned()))?;
    // The ClampOnWrite fault: a driver whose *actual* range is narrower than the one it
    // declares, which is what makes a caller that trusts `requested` wrong.
    let target = if force_clamp { desc.range.max } else { value };

    let mut warnings = Vec::new();
    let clamped = clamp_to(&desc.range, target);
    if clamped != value {
        warnings.push(WriteWarning::Clamped {
            requested: value,
            applied: clamped,
            range: desc.range,
        });
    }
    let aligned = align_to_step(&desc.range, clamped);
    if aligned != clamped {
        warnings.push(WriteWarning::StepAligned {
            requested: clamped,
            applied: aligned,
            step: desc.range.effective_step(),
        });
    }
    Ok((ControlValue::Int(aligned), warnings))
}

fn text_write(desc: &ControlDesc, requested: &ControlValue) -> Result<ControlValue> {
    match requested {
        ControlValue::Text(_) => Ok(requested.clone()),
        _ => Err(invalid_write(
            desc,
            "a string control takes a string value".to_owned(),
        )),
    }
}

fn payload_write(desc: &ControlDesc, requested: &ControlValue) -> Result<ControlValue> {
    let ControlValue::Bytes(bytes) = requested else {
        return Err(invalid_write(
            desc,
            "a compound control takes an opaque byte payload".to_owned(),
        ));
    };
    let expected = payload_len(desc);
    if expected > 0 && bytes.len() != expected {
        return Err(invalid_write(
            desc,
            format!(
                "this control's payload is {expected} bytes ({} elements of {}), not {}",
                desc.elems,
                desc.elem_size,
                bytes.len()
            ),
        ));
    }
    Ok(requested.clone())
}

/// Clamp without believing a descriptor that declares its minimum above its maximum —
/// `i64::clamp` panics on that, and a descriptor is device data (D2).
fn clamp_to(range: &ControlRange, value: i64) -> i64 {
    if range.min > range.max {
        value
    } else {
        value.clamp(range.min, range.max)
    }
}

/// Round down to the nearest step from the minimum, the way a driver does.
fn align_to_step(range: &ControlRange, value: i64) -> i64 {
    let step = range.effective_step();
    if step <= 1 || range.min > range.max {
        return value;
    }
    let offset = value.saturating_sub(range.min);
    let aligned = range.min.saturating_add(offset - offset.rem_euclid(step));
    clamp_to(range, aligned)
}

fn invalid_write(desc: &ControlDesc, message: String) -> Error {
    Error::DeviceIo {
        operation: format!("{SET_CTRL_OP} ({})", desc.slug),
        errno: Some(EINVAL),
        message,
    }
}

/// One open handle onto a replayed camera.
#[derive(Debug)]
pub struct FakeCamera {
    state: Arc<Mutex<CameraState>>,
    faults: Arc<Mutex<FaultQueue>>,
    /// A copy, because `Camera::info` hands out a reference and the shared state is
    /// behind a lock.
    info: CameraInfo,
}

impl FakeCamera {
    pub(crate) fn new(
        state: Arc<Mutex<CameraState>>,
        faults: Arc<Mutex<FaultQueue>>,
    ) -> FakeCamera {
        let info = lock(&state).info().clone();
        FakeCamera {
            state,
            faults,
            info,
        }
    }

    /// The focus value at which this camera's synthesized frames are sharpest, or `None`
    /// when the replayed profile has no focus control.
    ///
    /// A *stated* property of the model (see [`crate::frames`]): the optimum is the focus
    /// control's declared default. A calibration test reads that default out of the
    /// profile and expects the peak there, rather than asking the synthesizer where its
    /// own peak is — which would be the fake validating the fake (docs/3 Part C).
    #[must_use]
    pub fn focus_optimum(&self) -> Option<i64> {
        let knob = lock(&self.state).knob(frames::FOCUS_CONTROL_SLUG)?;
        Some(frames::focus_optimum(&knob.range, knob.default))
    }

    /// The profile this camera replays.
    #[must_use]
    pub fn profile(&self) -> DeviceProfile {
        lock(&self.state).profile.clone()
    }
}

impl Camera for FakeCamera {
    fn info(&self) -> &CameraInfo {
        &self.info
    }

    fn formats(&self) -> Result<Vec<FormatInfo>> {
        Ok(lock(&self.state).profile.invariant.formats.clone())
    }

    fn controls(&self) -> Result<Vec<ControlDesc>> {
        if take_fault(&self.faults, Fault::InactiveFlip) {
            lock(&self.state).flip_measured_partners();
        }
        Ok(lock(&self.state).descriptors())
    }

    fn get(&mut self, id: ControlId) -> Result<ControlValue> {
        let state = lock(&self.state);
        let desc = state
            .controls
            .get(&id)
            .ok_or_else(|| Error::ControlUnknown {
                requested: id.to_string(),
                did_you_mean: state.known_slugs(),
            })?;
        // Replayed as-is: a current value outside its declared range is a fact about the
        // device, and correcting it here would erase PF:4 from every test above.
        desc.current.clone().ok_or_else(|| Error::DeviceIo {
            operation: format!("VIDIOC_G_EXT_CTRLS ({})", desc.slug),
            errno: Some(EINVAL),
            message: "this control has no readable value".to_owned(),
        })
    }

    fn set(&mut self, id: ControlId, value: ControlValue) -> Result<Applied> {
        let force_clamp = take_fault(&self.faults, Fault::ClampOnWrite);
        lock(&self.state).write(id, &value, force_clamp)
    }

    fn start_stream(&mut self, request: &StreamRequest) -> Result<NegotiatedStream> {
        let mut state = lock(&self.state);
        if state.stream.is_some() {
            // V4L2 allows one streamer per node, and says so with EBUSY (D12).
            return Err(Error::Busy {
                path: state.capture_path(),
                holders: Vec::new(),
            });
        }
        let negotiated = negotiate(&state, request)?;
        state.stream = Some(negotiated.clone());
        state.frames_delivered = 0;
        Ok(negotiated)
    }

    fn next_frame(&mut self, deadline: Instant) -> Result<Frame> {
        let timed_out = take_fault(&self.faults, Fault::FrameTimeout);
        let device_gone = take_fault(&self.faults, Fault::DeviceGoneMidStream);
        let unsettled = take_fault(&self.faults, Fault::SettleNeverConverges);
        let mut state = lock(&self.state);

        if timed_out {
            // The deadline is reported as spent rather than slept through: a fake that
            // slept would be scheduling a flake (N3), and the caller's clock is the
            // engine's business anyway.
            let budget = deadline.saturating_duration_since(Instant::now());
            return Err(Error::SettleTimeout {
                waited_ms: u64::try_from(budget.as_millis()).unwrap_or(u64::MAX),
                frames_seen: state.frames_delivered,
            });
        }
        if device_gone {
            state.stream = None;
            return Err(Error::DeviceGone {
                path: state.capture_path(),
            });
        }

        let Some(stream) = state.stream.clone() else {
            return Err(Error::DeviceIo {
                operation: DQBUF_OP.to_owned(),
                errno: Some(EINVAL),
                message: "the stream is not running".to_owned(),
            });
        };

        let scene = Scene {
            width: stream.width,
            height: stream.height,
            focus: state.knob(frames::FOCUS_CONTROL_SLUG),
            brightness: state.knob(frames::BRIGHTNESS_CONTROL_SLUG),
            frame_index: state.frames_delivered,
            unsettled,
        };
        let bytes = frames::encode(&frames::render(&scene), stream.pixel_format)?;

        let frame = Frame {
            bytes,
            pixel_format: stream.pixel_format,
            width: stream.width,
            height: stream.height,
            bytes_per_line: stream.bytes_per_line,
            sequence: state.frames_delivered,
            timestamp_us: state.timestamp_us,
        };
        state.frames_delivered = state.frames_delivered.saturating_add(1);
        state.timestamp_us = state
            .timestamp_us
            .saturating_add(interval_micros(stream.interval));
        Ok(frame)
    }

    fn stop_stream(&mut self) -> Result<()> {
        // Idempotent, like `VIDIOC_STREAMOFF`: stopping a stopped stream is not an error.
        lock(&self.state).stream = None;
        Ok(())
    }
}

/// D5's negotiation: prefer what was asked for, report every way the answer differs.
fn negotiate(state: &CameraState, request: &StreamRequest) -> Result<NegotiatedStream> {
    let formats = &state.profile.invariant.formats;
    if state.info.capture_node().is_none() {
        // A metadata-only group is a real shape (D1): it is listed, and streaming it is a
        // typed refusal rather than a surprise.
        return Err(Error::FormatUnsupported {
            requested: request.pixel_format,
            available: Vec::new(),
        });
    }

    let usable: Vec<&FormatInfo> = formats
        .iter()
        .filter(|f| frames::can_synthesize(f.pixel_format) && !f.sizes.is_empty())
        .collect();
    let Some(first) = usable.first().copied() else {
        return Err(Error::FormatUnsupported {
            requested: request.pixel_format,
            available: formats.iter().map(|f| f.pixel_format).collect(),
        });
    };

    let mut adjustments = Vec::new();
    let chosen = match request.pixel_format {
        None => first,
        Some(wanted) => match usable.iter().copied().find(|f| f.pixel_format == wanted) {
            Some(found) => found,
            None if formats.iter().any(|f| f.pixel_format == wanted) => {
                // The device offers it; this backend cannot synthesize it. A driver in
                // the same position adjusts and says so (D5), so that is what happens.
                adjustments.push(Adjustment::PixelFormat {
                    requested: wanted,
                    negotiated: first.pixel_format,
                });
                first
            }
            None => {
                return Err(Error::FormatUnsupported {
                    requested: Some(wanted),
                    available: usable.iter().map(|f| f.pixel_format).collect(),
                });
            }
        },
    };

    let (width, height) = choose_size(&chosen.sizes, request);
    if let (Some(requested_width), Some(requested_height)) = (request.width, request.height)
        && (requested_width, requested_height) != (width, height)
    {
        adjustments.push(Adjustment::Size {
            requested_width,
            requested_height,
            negotiated_width: width,
            negotiated_height: height,
        });
    }

    let interval = choose_interval(&chosen.sizes, width, height, request);
    if let Some(requested_interval) = request.interval
        && requested_interval != interval
    {
        adjustments.push(Adjustment::Interval {
            requested: requested_interval,
            negotiated: interval,
        });
    }

    let bytes_per_line = bytes_per_line(chosen.pixel_format, width);
    Ok(NegotiatedStream {
        pixel_format: chosen.pixel_format,
        width,
        height,
        bytes_per_line,
        size_image: size_image(chosen.pixel_format, width, height, bytes_per_line),
        interval,
        adjustments,
    })
}

/// The size the device would settle on: the exact request when it is offered, otherwise
/// the largest offered size that fits inside it, otherwise the first the device listed.
fn choose_size(sizes: &[FrameSizeInfo], request: &StreamRequest) -> (u32, u32) {
    let offered: Vec<(u32, u32)> = sizes.iter().map(|s| s.size.max_dimensions()).collect();
    let first = offered.first().copied().unwrap_or((640, 480));
    let (Some(width), Some(height)) = (request.width, request.height) else {
        // Nothing was asked for, so nothing was adjusted: the device's own first entry.
        return first;
    };
    if offered.contains(&(width, height)) {
        return (width, height);
    }
    offered
        .iter()
        .copied()
        .filter(|(w, h)| *w <= width && *h <= height)
        .max_by_key(|(w, h)| u64::from(*w) * u64::from(*h))
        .unwrap_or(first)
}

/// The interval offered at the chosen size: the request when it is on the list, else the
/// first entry, else a nominal 30 fps for a profile that recorded none.
fn choose_interval(
    sizes: &[FrameSizeInfo],
    width: u32,
    height: u32,
    request: &StreamRequest,
) -> FrameInterval {
    let at_size = sizes
        .iter()
        .find(|s| s.size.max_dimensions() == (width, height))
        .or_else(|| sizes.first());
    let Some(entry) = at_size else {
        return FALLBACK_INTERVAL;
    };
    match request.interval {
        Some(wanted) if entry.intervals.contains(&wanted) => wanted,
        _ => entry
            .intervals
            .first()
            .copied()
            .unwrap_or(FALLBACK_INTERVAL),
    }
}

/// The stride a driver reports: zero for a compressed bitstream, the packed row length
/// otherwise.
fn bytes_per_line(format: PixelFormat, width: u32) -> u32 {
    if format.is_compressed() {
        0
    } else if format == PixelFormat::YUYV {
        // Whole 4:2:2 pairs, which is how an odd width is padded.
        width.div_ceil(2).saturating_mul(4)
    } else {
        width
    }
}

/// The driver's declared maximum frame size.
fn size_image(format: PixelFormat, width: u32, height: u32, bytes_per_line: u32) -> u32 {
    if format.is_compressed() {
        // What UVC drivers typically declare for MJPG: two bytes per pixel of headroom.
        width.saturating_mul(height).saturating_mul(2)
    } else if format == PixelFormat::NV12 {
        let luma = bytes_per_line.saturating_mul(height);
        luma.saturating_add(luma / 2)
    } else {
        bytes_per_line.saturating_mul(height)
    }
}

/// One frame interval in microseconds, for the synthetic timestamp clock.
fn interval_micros(interval: FrameInterval) -> i64 {
    let (numerator, denominator) = match interval {
        FrameInterval::Discrete {
            numerator,
            denominator,
        }
        | FrameInterval::Stepwise {
            min_numerator: numerator,
            min_denominator: denominator,
            ..
        } => (i64::from(numerator), i64::from(denominator)),
    };
    if denominator == 0 {
        return 0;
    }
    numerator.saturating_mul(1_000_000) / denominator
}
