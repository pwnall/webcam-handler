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
//! - It does not refuse a mis-sized compound payload. `vivid` reshapes a compound control
//!   across a format negotiation and *applies* the payload at its new length, reporting
//!   success \[PF:17\]; refusing would be a capability no measured driver has, and it
//!   would hide the adjustment a compound restore really produces (note **N136**).

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Instant;

use camino::Utf8PathBuf;
use schema::backend::{BackendKind, Camera};
use schema::camera::{CameraId, CameraInfo, FormatInfo, FrameInterval, FrameSizeInfo, PixelFormat};
use schema::capture::{Frame, NegotiatedStream, StreamRequest};
use schema::control::{
    Applied, ControlDesc, ControlFlags, ControlId, ControlSlug, ControlType, ControlValue,
    KnownFlag, WriteWarning,
};
use schema::error::{Error, Occupation, Result};
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
    /// Whether this camera has vanished from the machine (design D19).
    ///
    /// Set by `Fault::DeviceGoneMidStream` when it fires, and it is what makes the fake's
    /// device loss a *loss* rather than one refused call: a camera that answered `DeviceGone`
    /// to a frame and went on being enumerated would be a shape no real machine has, and a
    /// consumer's re-enumeration — the thing D19 says a caller does next — would find it and
    /// carry on. Once set, this camera is absent from `enumerate` and refuses every further
    /// operation with `DeviceGone` (E5: the double resembles by having the same shape, and
    /// this is the shape).
    gone: bool,
    frames_delivered: u32,
    /// How many streams this camera has started, ever.
    ///
    /// Cumulative and never reset, unlike `frames_delivered`: a caller asking "how many
    /// times did that operation start the sensor" is asking about the whole run. It is an
    /// *observation* of the double, not a capability claim, so it costs the resemblance
    /// rule (E5) nothing — and it is what lets a test hold a caller to "one capture per
    /// sample" instead of hoping.
    streams_started: u64,
    /// How many handles have been opened onto this camera, and how many have gone away.
    ///
    /// The same kind of observation as `streams_started`, for the caller D12 introduces:
    /// the daemon's camera actor opens on first use and closes on idle, and "the
    /// descriptor went away" is the only honest form of that claim — an actor that merely
    /// forgot its handle would pass a test that asked whether it had *decided* to close.
    /// Two counters rather than a gauge because both directions matter and a gauge cannot
    /// tell "never opened" from "opened and closed".
    opens: u64,
    closes: u64,
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
            gone: false,
            frames_delivered: 0,
            streams_started: 0,
            opens: 0,
            closes: 0,
            timestamp_us: 0,
        }
    }

    /// How many streams this camera has started since it was opened.
    pub(crate) fn streams_started(&self) -> u64 {
        self.streams_started
    }

    /// How many handles have been opened onto this camera, ever.
    pub(crate) fn opens(&self) -> u64 {
        self.opens
    }

    /// How many of those handles have been dropped, ever.
    pub(crate) fn closes(&self) -> u64 {
        self.closes
    }

    /// Record a handle being opened.
    pub(crate) fn opened(&mut self) {
        self.opens = self.opens.saturating_add(1);
    }

    /// Record a handle going away, which on a real device is the descriptor closing.
    pub(crate) fn closed(&mut self) {
        self.closes = self.closes.saturating_add(1);
    }

    /// Whether this camera has left the machine (design D19).
    pub(crate) fn is_gone(&self) -> bool {
        self.gone
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

        // **Dispatch belongs to the descriptor** (design §2.3), which means the
        // `HAS_PAYLOAD` flag and not the control's type. The two agree on every control in
        // `corpus/` and in `synthetic-basic.json`, because each of those is either a plain
        // scalar or a plain payload — but they separate on an *array* control, where the
        // kernel keeps the element type (`INTEGER`, say) and sets `HAS_PAYLOAD` because the
        // value is `elems × elem_size` bytes behind a pointer. This crate dispatched on the
        // type until 2026-08-16 and would have written such a control as a scalar, which is
        // the mirror image of the P2 ioctl-dispatch defect that put the contract note in
        // §2.3 in the first place — the fake on the wrong side of it this time (note
        // **N135**). `crates/backends/v4l2/src/lib.rs`'s `set` reads the same flag, so the
        // two backends now decide the shape of a write the same way (E5).
        let applied = if desc.flags.has(KnownFlag::HasPayload) {
            payload_write(&desc, requested)?
        } else if desc.control_type == ControlType::Button {
            // A button has no value; writing it is the whole effect.
            requested.clone()
        } else if desc.control_type.is_menu() {
            menu_write(&desc, requested)?
        } else if desc.control_type == ControlType::String {
            text_write(&desc, requested)?
        } else {
            // Everything the descriptor did not flag as a payload takes a scalar, whatever
            // its type is called — including a type this build cannot name (D2). That is
            // what the V4L2 backend does with the same descriptor: `(false, Int)` reaches
            // `set_scalar` carrying the raw type, and a `Bytes` value at such a control is
            // the typed refusal `scalar_write` produces here.
            scalar_write(&desc, requested, force_clamp)?
        };
        // Simulating what the driver did is this crate's business; describing it is the
        // schema's (§2.10), so every arm above lands in the same classifier the V4L2
        // backend uses on its own read-back.
        let warnings = WriteWarning::classify(&desc, requested, &applied);

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
    // The descriptor decides the shape here for the same reason it decides it on a write
    // (§2.3, note **N135**): a control the device flagged `HAS_PAYLOAD` reads back as bytes
    // whatever its element type is called, and seeding an array control with an `Int` would
    // hand out a first `get` no driver could produce.
    let initial = match desc.control_type {
        // A button and a class header have no value to read.
        ControlType::Button | ControlType::ControlClass => None,
        _ if desc.flags.has(KnownFlag::HasPayload) => {
            Some(ControlValue::Bytes(vec![0; payload_len(&desc)]))
        }
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
///
/// The *simulation* is here — clamp into the declared range, then round down to the step,
/// which is what the seed hardware's drivers do. The *description* of what that did is
/// [`WriteWarning::classify`]'s, in the schema, so the fake and the V4L2 backend cannot
/// describe the same adjustment differently (E5).
fn scalar_write(
    desc: &ControlDesc,
    requested: &ControlValue,
    force_clamp: bool,
) -> Result<ControlValue> {
    let value = requested
        .as_int()
        .ok_or_else(|| invalid_write(desc, "a scalar control takes an integer value".to_owned()))?;
    // The ClampOnWrite fault: a driver whose *actual* range is narrower than the one it
    // declares, which is what makes a caller that trusts `requested` wrong.
    let target = if force_clamp { desc.range.max } else { value };

    Ok(ControlValue::Int(
        desc.range.align_down(desc.range.clamp_value(target)),
    ))
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

/// PF:17 — a mis-sized payload is *adjusted* to the shape the control currently has, not
/// refused.
///
/// This function refused it until 2026-08-16, on the reasonable-sounding argument that "a
/// driver checks `elem_size × elems`". \[PF:17\] measured the opposite on `vivid`: a
/// 300-byte write to a `u8 pixel array` that had reshaped to 240 bytes came back
/// **applied at 240 bytes**, with no error — the write succeeded and the difference was
/// visible only in the read-back. Rubric A9 is what settles which side was wrong: a fake
/// capability no real device exhibits is a bug in the fake, and a refusal here would let
/// the engine ship a "the device rejected the payload" path nothing can reach, while
/// hiding the [`WriteWarning::Adjusted`](schema::control::WriteWarning::Adjusted) path that
/// a compound restore actually takes (note **N136**).
///
/// The *shape* mismatch is still a refusal, and is a different fact: a caller handing an
/// integer to a pointer control has said something the descriptor contradicts, and that is
/// §2.3's typed refusal rather than a driver adjustment.
fn payload_write(desc: &ControlDesc, requested: &ControlValue) -> Result<ControlValue> {
    let ControlValue::Bytes(bytes) = requested else {
        return Err(invalid_write(
            desc,
            "a compound control takes an opaque byte payload".to_owned(),
        ));
    };
    let expected = payload_len(desc);
    if expected == 0 || bytes.len() == expected {
        return Ok(requested.clone());
    }
    // Truncated or zero-extended to the declared shape, which is what a driver writing
    // into its own `elems × elem_size` buffer leaves behind. `classify` turns the
    // difference into `Adjusted`, so the caller is told rather than obeyed.
    let mut applied = bytes.clone();
    applied.resize(expected, 0);
    Ok(ControlValue::Bytes(applied))
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
        let info = {
            let mut live = lock(&state);
            live.opened();
            live.info().clone()
        };
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
    /// own peak is — which would be the fake validating the fake (docs/8 Part C).
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

/// Dropping a handle is what closing a device node is, so the double models it as one.
///
/// The stream is deliberately *not* stopped here: the shared camera state outlives every
/// handle by construction (two opens are two views of one device), and a real driver frees
/// a stream when the last descriptor on it closes — which is a property of the node, not of
/// this object. What is recorded is the close itself, because "the descriptor went away" is
/// the only honest form of D12's idle-close claim.
impl Drop for FakeCamera {
    fn drop(&mut self) {
        lock(&self.state).closed();
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
        let mut descriptors = lock(&self.state).descriptors();
        if take_fault(&self.faults, Fault::ControlReadDeclined) {
            // One control, and the walk carries on — which is the claim
            // `v4l2::walked_current` makes and the fake had no way to exhibit. The first
            // writable one that *would* have a value: a control the descriptor already
            // predicts is valueless would make the fault unobservable.
            if let Some(desc) = descriptors
                .iter_mut()
                .find(|desc| desc.is_writable() && desc.current.is_some())
            {
                desc.current = None;
            }
        }
        Ok(descriptors)
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
            // V4L2 allows one streamer per node, and says so with EBUSY (D12). The stream in
            // the way is one *this* handle opened, so the refusal says so — the same door
            // `webcam-handler-v4l2`'s `start_stream` takes, which is what E5 asks of this
            // backend: a claim the real one does not make is a bug in the fake (note **N221**).
            return Err(Error::busy_here(
                state.capture_path(),
                Occupation::Streaming,
            ));
        }
        let negotiated = negotiate(&state, request)?;
        state.stream = Some(negotiated.clone());
        state.frames_delivered = 0;
        state.streams_started = state.streams_started.saturating_add(1);
        Ok(negotiated)
    }

    fn streaming(&self) -> Option<NegotiatedStream> {
        // The same field `start_stream` refuses a second `STREAMON` from, read rather than
        // shadowed — and it is the *shared* state, so a second handle onto this camera sees
        // the stream the first one started, exactly as a second `open(2)` on a node sees a
        // stream another process holds.
        lock(&self.state).stream.clone()
    }

    fn next_frame(&mut self, deadline: Instant) -> Result<Frame> {
        // **Each fault taken where it decides the answer, and never before** (rubric A9's
        // fault-menu half; the G6 review's L35, note **N232**). All three used to come off
        // the queue at the top, so a call that reported a `FrameTimeout` also consumed the
        // `SettleNeverConverges` a test had scripted for the frame *after* it — and a call
        // that could produce no frame at all, because no stream was running, emptied the
        // queue on its way to `EINVAL`. A one-shot removed by a call that reported
        // something else is a scripted claim that silently never fires, which is the one
        // thing a fault menu may not do.
        let mut state = lock(&self.state);

        if take_fault(&self.faults, Fault::FrameTimeout) {
            // The deadline is reported as spent rather than slept through: a fake that
            // slept would be scheduling a flake (N3), and the caller's clock is the
            // engine's business anyway.
            let budget = deadline.saturating_duration_since(Instant::now());
            return Err(Error::SettleTimeout {
                waited_ms: u64::try_from(budget.as_millis()).unwrap_or(u64::MAX),
                frames_seen: state.frames_delivered,
            });
        }
        if take_fault(&self.faults, Fault::DeviceGoneMidStream) {
            state.stream = None;
            // The camera leaves the machine, it does not merely refuse this frame: D19's
            // contract is a sentence about `list`, about a watcher's removal event and about
            // a later return being a *new arrival*, and none of those can be driven against a
            // double that stayed enumerable.
            state.gone = true;
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

        // Last, and after the stream check above: this one changes a frame rather than
        // replacing it, so it is only spent by a call that is going to render one.
        let unsettled = take_fault(&self.faults, Fault::SettleNeverConverges);
        let scene = Scene {
            width: stream.width,
            height: stream.height,
            focus: state.knob(frames::FOCUS_CONTROL_SLUG),
            brightness: state.knob(frames::BRIGHTNESS_CONTROL_SLUG),
            frame_index: state.frames_delivered,
            unsettled,
        };
        let bytes = frames::encode(&frames::render(&scene), stream.pixel_format)?;

        // A lost run of frames advances both counters before this frame is stamped, so what
        // a consumer sees is a sequence that skipped and a clock that moved on by exactly the
        // frames that never arrived (design D16). Spent here rather than beside the two
        // faults above because it changes a frame rather than replacing one, which is the
        // same rule `SettleNeverConverges` is placed by.
        // Only once a frame has already gone out: a gap is a *difference* between two
        // sequence numbers, so a scripted loss before the first frame would leave the take
        // starting at sequence three with nothing to have skipped — invisible to every
        // consumer, and a fault that cannot be observed is the thing this menu exists to
        // prevent. Queued and not yet spent, so the next frame fires it.
        if state.frames_delivered > 0 && take_fault(&self.faults, Fault::FrameGap) {
            state.frames_delivered = state
                .frames_delivered
                .saturating_add(crate::fault::FRAME_GAP_FRAMES);
            state.timestamp_us = state.timestamp_us.saturating_add(
                interval_micros(stream.interval)
                    .saturating_mul(i64::from(crate::fault::FRAME_GAP_FRAMES)),
            );
        }

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
        return Err(Error::format_unsupported(request.pixel_format, Vec::new()));
    }

    // The formats this backend can actually synthesize frames for. Filtered *before* the
    // chooser rather than inside it: "which formats can I produce" is this crate's
    // limitation, and teaching the shared resolver about it would make the schema know
    // something only the fake is true of.
    let usable: Vec<FormatInfo> = formats
        .iter()
        .filter(|f| frames::can_synthesize(f.pixel_format) && !f.sizes.is_empty())
        .cloned()
        .collect();

    // A format the device offers and this backend cannot synthesize is *adjusted*, not
    // refused: a driver in the same position adjusts and says so (D5), and the shared diff
    // below reports it. So the name is dropped from the request the resolver sees — this
    // camera *has* the format, and the only thing that cannot produce it is the stand-in.
    //
    // A format the device does not offer at all is a refusal, and that guard used to be
    // three lines here. It is [`StreamRequest::choose`]'s since 2026-08-16, because a rule
    // D5 states once must not live in the backend that happens to have read it: the V4L2
    // backend had no such guard and photographed something else for three phases (note
    // **N134**). Dropping the name is what keeps *this* crate's limitation from arriving
    // at the shared resolver dressed as the device's.
    //
    // **What dropping the name costs when `usable` is empty**, which is the second
    // behavioural delta of this repair rather than the one it was first credited with (note
    // **N138**): the resolver then refuses with `available: []`, where the deleted guard
    // refused naming every format the device has. Unreachable today — every format in the
    // five committed profiles and in `synthetic-basic.json` is synthesizable — and the
    // *sentence* is honest either way since `format_formats`' empty arm stopped claiming
    // "the capture node enumerated no formats", which was false for exactly this caller. A
    // camera whose whole enumeration this crate cannot draw has nothing to offer a stream,
    // and "nothing would be accepted" is what it should say.
    let asked_for_the_unsynthesizable = request.pixel_format.is_some_and(|wanted| {
        formats.iter().any(|f| f.pixel_format == wanted)
            && !usable.iter().any(|f| f.pixel_format == wanted)
    });
    let to_resolve = if asked_for_the_unsynthesizable {
        StreamRequest {
            pixel_format: None,
            ..request.clone()
        }
    } else {
        request.clone()
    };
    let chosen = to_resolve.choose(&usable)?;

    // The size list of the format that was chosen, for the interval lookup below.
    let sizes = usable
        .iter()
        .find(|f| f.pixel_format == chosen.pixel_format)
        .map_or(&[][..], |f| f.sizes.as_slice());
    let interval = choose_interval(sizes, chosen.width, chosen.height, request);
    let bytes_per_line = bytes_per_line(chosen.pixel_format, chosen.width);
    Ok(NegotiatedStream {
        pixel_format: chosen.pixel_format,
        width: chosen.width,
        height: chosen.height,
        bytes_per_line,
        size_image: size_image(
            chosen.pixel_format,
            chosen.width,
            chosen.height,
            bytes_per_line,
        ),
        interval,
        // Choosing is this backend's business; describing how the choice differs from the
        // request is one law, and it lives in the schema (§2.10).
        adjustments: NegotiatedStream::diff(
            request,
            chosen.pixel_format,
            chosen.width,
            chosen.height,
            interval,
        ),
    })
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
        .find(|s| s.size.max_dimensions() == Some((width, height)))
        .or_else(|| sizes.first());
    let Some(entry) = at_size else {
        return FALLBACK_INTERVAL;
    };
    // `stated_interval`, not `request.interval`: a request carrying `Unstated` said the
    // caller asked for nothing, and the fake reading it as "an interval that is not on the
    // list" would report an adjustment for a request nobody made — while the V4L2 backend
    // read the device back and reported none. Same document, two backends, two answers
    // (E5, note **N199**).
    match request.stated_interval() {
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
        // An interval whose shape this build cannot read cannot drive a clock, and neither
        // can one the device declined to name. The fake falls back rather than inventing a
        // rate — and a fake that invented one would be claiming a capability no real device
        // demonstrated (E5).
        FrameInterval::Unknown { .. } | FrameInterval::Unstated => match FALLBACK_INTERVAL {
            FrameInterval::Discrete {
                numerator,
                denominator,
            } => (i64::from(numerator), i64::from(denominator)),
            _ => (1, 30),
        },
    };
    if denominator == 0 {
        return 0;
    }
    numerator.saturating_mul(1_000_000) / denominator
}
