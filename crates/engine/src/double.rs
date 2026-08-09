//! The engine's scripted camera, shared by every module's tests.
//!
//! **Not a second fake.** `webcam-handler-fake` replays a captured profile and is the
//! double the *battery* runs against; it is a dev-dependency here and the modules whose
//! subject is realistic device behaviour (photo, discovery) use it. What it cannot do is
//! misbehave on cue at a chosen step — a write that fails on the third control, a control
//! that stops being writable between a snapshot and its restore — because those are
//! failures no real device exhibits *on demand*, and E5 forbids teaching the fake to.
//!
//! So this is the fault-injection half: a camera assembled from a list of controls with a
//! per-control script. It has no physics, no profile and no opinions; the tests that need
//! those use the fake.
//!
//! One copy, in one module, because five copies of a `Camera` impl is five places a trait
//! change breaks.

use std::collections::BTreeMap;
use std::time::Instant;

use schema::backend::{BackendKind, Camera};
use schema::camera::{
    CameraFingerprint, CameraId, CameraInfo, DeviceNode, FormatInfo, FrameInterval, FrameSize,
    FrameSizeInfo, NodeKind, PixelFormat,
};
use schema::capture::{Frame, NegotiatedStream, StreamRequest};
use schema::control::{
    Applied, ControlDesc, ControlFlags, ControlId, ControlRange, ControlSlug, ControlType,
    ControlValue, KnownFlag, WriteWarning,
};
use schema::error::{Error, Result};

/// What a scripted camera does when a particular control is written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum OnWrite {
    /// Take the value.
    Accept,
    /// Take a different one, the way a driver that clamps does \[PF:6\].
    Apply(i64),
    /// Refuse, with this error.
    Fail(Error),
    /// Take the value, and flip the INACTIVE bit of the named controls to match whether
    /// the written value is non-zero \[PF:3\]. The empirical-discovery probe's subject.
    Couple(Vec<&'static str>),
    /// Take the value, and clear the named controls' INACTIVE bit **whatever the value
    /// was**.
    ///
    /// A device whose flag does not follow the automation control's value deterministically
    /// — the mode the flag tracks is not one we wrote. Real: `INACTIVE` is updated by the
    /// driver on its own schedule, which is why PF:3 records the flag diff as a *measured*
    /// method rather than a guarantee. This is the shape D4's second restore pass exists
    /// for, and without it that branch would be code no test could reach.
    ClearsInactive(Vec<&'static str>),
    /// Take the value, and clear the named controls' INACTIVE bit **only** when the value
    /// written equals the first field.
    ///
    /// A menu-valued automation control where one position hands the manual control to the
    /// user and the others do not — which is `auto_exposure` on the seed hardware, whose
    /// sparse menu has `Manual Mode` at index 1 and `Aperture Priority Mode` at index 3
    /// \[PF:2\].
    FreesAt(i64, Vec<&'static str>),
    /// Take the value, and *exchange* which controls are owned: a non-zero value frees the
    /// first list and freezes the second, a zero value does the reverse.
    ///
    /// A mode change rather than a switch. Real: a camera that hands `exposure_time_absolute`
    /// to the user in manual mode may take `gain` away from them in the same breath, and
    /// the two halves of that need different "off" recipes.
    Swaps(Vec<&'static str>, Vec<&'static str>),
    /// Take the value, set the named controls INACTIVE, and then **refuse every later
    /// write** to this control.
    ///
    /// A candidate whose toggle cannot be undone, which is what makes a stale diff baseline
    /// visible: the residue it leaves is what the next candidate would otherwise be blamed
    /// for.
    LatchesInactive(Vec<&'static str>),
}

/// A camera assembled from controls and a script.
#[derive(Debug)]
pub(crate) struct ScriptedCamera {
    info: CameraInfo,
    controls: Vec<ControlDesc>,
    script: BTreeMap<ControlId, OnWrite>,
    /// Every write that reached the device, in order — so a test can assert that a plan
    /// that failed part way did not keep going.
    pub(crate) writes: Vec<(ControlSlug, ControlValue)>,
    streaming: Option<NegotiatedStream>,
    frames: Vec<Frame>,
    /// Controls whose script has already fired once and now refuses — the `LatchesInactive`
    /// half.
    latched: std::collections::BTreeSet<ControlId>,
    /// Whether this camera offers any format at all. A camera that offers none is D1's
    /// metadata-only shape, and it is the one way to make `start_stream` refuse without
    /// scripting a lie.
    formats: bool,
    /// How many times the stream was stopped, so a test can assert that it was.
    pub(crate) stops: u32,
}

impl ScriptedCamera {
    /// A camera exposing `controls`, accepting every write.
    pub(crate) fn new(controls: Vec<ControlDesc>) -> ScriptedCamera {
        ScriptedCamera {
            info: info("cam:test", "3-4:1.0"),
            controls,
            script: BTreeMap::new(),
            writes: Vec::new(),
            streaming: None,
            frames: Vec::new(),
            latched: std::collections::BTreeSet::new(),
            formats: true,
            stops: 0,
        }
    }

    /// A camera that enumerates no format — D1's metadata-only shape, which is listed and
    /// refuses to stream.
    pub(crate) fn without_formats(mut self) -> ScriptedCamera {
        self.formats = false;
        self
    }

    /// Script what happens when `slug` is written.
    pub(crate) fn on_write(mut self, slug: &str, action: OnWrite) -> ScriptedCamera {
        if let Some(desc) = self.controls.iter().find(|d| d.slug.as_str() == slug) {
            self.script.insert(desc.id, action);
        }
        self
    }

    /// Give this camera an identity, for the fingerprint-mismatch tests.
    pub(crate) fn identified(mut self, id: &str, bus_path: &str) -> ScriptedCamera {
        self.info = info(id, bus_path);
        self
    }

    /// The same camera under a different card name.
    ///
    /// Its own builder rather than a parameter on [`ScriptedCamera::identified`] because
    /// the fingerprint's fields are what a mismatch *names* (D13), and a fixture that could
    /// only change them all at once could not show that the refusal names the ones that
    /// differ and not the ones that agree.
    pub(crate) fn carded(mut self, card: &str) -> ScriptedCamera {
        self.info.card = card.to_owned();
        self.info.fingerprint.card = card.to_owned();
        self
    }

    /// The frames `next_frame` will hand out, in order. Running out is a timeout, which
    /// is how a "the camera stopped delivering" test is written without a clock.
    pub(crate) fn with_frames(mut self, frames: Vec<Frame>) -> ScriptedCamera {
        self.frames = frames;
        self
    }

    /// The current value of one control, for a test's own assertions.
    pub(crate) fn value_of(&self, slug: &str) -> Option<ControlValue> {
        self.controls
            .iter()
            .find(|d| d.slug.as_str() == slug)
            .and_then(|d| d.current.clone())
    }

    /// Whether one control reports itself INACTIVE right now.
    pub(crate) fn is_inactive(&self, slug: &str) -> bool {
        self.controls
            .iter()
            .find(|d| d.slug.as_str() == slug)
            .is_some_and(ControlDesc::is_inactive)
    }

    /// Make a control read-only after construction — the "it stopped being writable
    /// between the snapshot and the restore" fixture.
    pub(crate) fn make_read_only(&mut self, slug: &str) {
        if let Some(desc) = self.controls.iter_mut().find(|d| d.slug.as_str() == slug) {
            desc.flags = ControlFlags::from_raw(desc.flags.raw | KnownFlag::ReadOnly.bit());
        }
    }

    fn set_inactive(&mut self, slug: &str, inactive: bool) {
        if let Some(desc) = self.controls.iter_mut().find(|d| d.slug.as_str() == slug) {
            let bit = KnownFlag::Inactive.bit();
            let raw = if inactive {
                desc.flags.raw | bit
            } else {
                desc.flags.raw & !bit
            };
            desc.flags = ControlFlags::from_raw(raw);
        }
    }
}

impl Camera for ScriptedCamera {
    fn info(&self) -> &CameraInfo {
        &self.info
    }

    fn formats(&self) -> Result<Vec<FormatInfo>> {
        if !self.formats {
            return Ok(Vec::new());
        }
        Ok(vec![FormatInfo {
            pixel_format: PixelFormat::YUYV,
            description: "YUYV 4:2:2".to_owned(),
            flags: 0,
            sizes: vec![FrameSizeInfo {
                size: FrameSize::Discrete {
                    width: 32,
                    height: 16,
                },
                intervals: vec![FrameInterval::Discrete {
                    numerator: 1,
                    denominator: 30,
                }],
            }],
        }])
    }

    fn controls(&self) -> Result<Vec<ControlDesc>> {
        Ok(self.controls.clone())
    }

    fn get(&mut self, id: ControlId) -> Result<ControlValue> {
        self.controls
            .iter()
            .find(|d| d.id == id)
            .and_then(|d| d.current.clone())
            .ok_or_else(|| Error::ControlUnknown {
                requested: id.to_string(),
                did_you_mean: Vec::new(),
            })
    }

    fn set(&mut self, id: ControlId, value: ControlValue) -> Result<Applied> {
        let desc = self
            .controls
            .iter()
            .find(|d| d.id == id)
            .cloned()
            .ok_or_else(|| Error::ControlUnknown {
                requested: id.to_string(),
                did_you_mean: Vec::new(),
            })?;
        if !desc.is_writable() {
            return Err(Error::ControlReadOnly { control: desc.slug });
        }

        let action = self.script.get(&id).cloned().unwrap_or(OnWrite::Accept);
        if matches!(action, OnWrite::LatchesInactive(_)) && self.latched.contains(&id) {
            return Err(Error::DeviceIo {
                operation: "VIDIOC_S_EXT_CTRLS".to_owned(),
                errno: Some(16),
                message: "this control latched and will not move again".to_owned(),
            });
        }
        let applied = match &action {
            OnWrite::Fail(error) => return Err(error.clone()),
            OnWrite::Apply(other) => ControlValue::Int(*other),
            OnWrite::Accept
            | OnWrite::Couple(_)
            | OnWrite::ClearsInactive(_)
            | OnWrite::FreesAt(_, _)
            | OnWrite::Swaps(_, _)
            | OnWrite::LatchesInactive(_) => value.clone(),
        };

        self.writes.push((desc.slug.clone(), value.clone()));
        if let Some(entry) = self.controls.iter_mut().find(|d| d.id == id) {
            entry.current = Some(applied.clone());
        }
        match &action {
            OnWrite::Couple(partners) => {
                let engaged = applied.as_int().is_some_and(|v| v != 0);
                for partner in partners.clone() {
                    self.set_inactive(partner, engaged);
                }
            }
            OnWrite::ClearsInactive(partners) => {
                for partner in partners.clone() {
                    self.set_inactive(partner, false);
                }
            }
            OnWrite::FreesAt(free_at, partners) => {
                let frees = applied.as_int() == Some(*free_at);
                for partner in partners.clone() {
                    self.set_inactive(partner, !frees);
                }
            }
            OnWrite::Swaps(freed_when_set, frozen_when_set) => {
                let engaged = applied.as_int().is_some_and(|v| v != 0);
                for partner in freed_when_set.clone() {
                    self.set_inactive(partner, !engaged);
                }
                for partner in frozen_when_set.clone() {
                    self.set_inactive(partner, engaged);
                }
            }
            OnWrite::LatchesInactive(partners) => {
                for partner in partners.clone() {
                    self.set_inactive(partner, true);
                }
                self.latched.insert(id);
            }
            OnWrite::Accept | OnWrite::Apply(_) | OnWrite::Fail(_) => {}
        }

        Ok(Applied {
            control: id,
            slug: desc.slug.clone(),
            warnings: WriteWarning::classify(&desc, &value, &applied),
            requested: value,
            applied,
        })
    }

    fn start_stream(&mut self, request: &StreamRequest) -> Result<NegotiatedStream> {
        let formats = self.formats()?;
        let chosen = request
            .choose(&formats)
            .ok_or_else(|| Error::FormatUnsupported {
                requested: request.pixel_format,
                available: Vec::new(),
            })?;
        let interval = FrameInterval::Discrete {
            numerator: 1,
            denominator: 30,
        };
        let negotiated = NegotiatedStream {
            pixel_format: chosen.pixel_format,
            width: chosen.width,
            height: chosen.height,
            bytes_per_line: chosen.width * 2,
            size_image: chosen.width * chosen.height * 2,
            interval,
            adjustments: NegotiatedStream::diff(
                request,
                chosen.pixel_format,
                chosen.width,
                chosen.height,
                interval,
            ),
        };
        self.streaming = Some(negotiated.clone());
        Ok(negotiated)
    }

    fn next_frame(&mut self, _deadline: Instant) -> Result<Frame> {
        if self.streaming.is_none() {
            return Err(Error::DeviceIo {
                operation: "next_frame".to_owned(),
                errno: None,
                message: "the stream is not running".to_owned(),
            });
        }
        // Running out of scripted frames is a camera that stopped delivering, which is
        // what the settle loop's deadline exists for. The error carries no frame count:
        // counting is the *engine's* job, and a double that did it would be answering the
        // question under test.
        if self.frames.is_empty() {
            return Err(Error::SettleTimeout {
                waited_ms: 0,
                frames_seen: 0,
            });
        }
        Ok(self.frames.remove(0))
    }

    fn stop_stream(&mut self) -> Result<()> {
        self.stops = self.stops.saturating_add(1);
        self.streaming = None;
        Ok(())
    }
}

/// An identity for a scripted camera.
fn info(id: &str, bus_path: &str) -> CameraInfo {
    CameraInfo {
        id: CameraId::parse(id).expect("literal id"),
        fingerprint: CameraFingerprint {
            bus_path: bus_path.to_owned(),
            usb_id: None,
            card: "Scripted".to_owned(),
            driver: "scripted".to_owned(),
            serial: None,
        },
        card: "Scripted".to_owned(),
        driver: "scripted".to_owned(),
        bus_info: "usb-scripted".to_owned(),
        nodes: vec![DeviceNode {
            path: "/dev/video0".into(),
            kind: NodeKind::VideoCapture,
            device_caps: 0x0420_0001,
            capabilities: 0x84a0_0001,
        }],
        backend: BackendKind::V4l2,
    }
}

// ------------------------------------------------------------------ control builders

/// An integer control with a `[0..100]` range.
pub(crate) fn integer(slug: &str, current: i64) -> ControlDesc {
    ControlDesc {
        id: ControlId(control_id(slug)),
        name: slug.to_owned(),
        slug: ControlSlug::parse(slug).expect("literal slug"),
        control_type: ControlType::Integer,
        range: ControlRange {
            min: 0,
            max: 100,
            step: 1,
        },
        default: 50,
        flags: ControlFlags::from_raw(0),
        menu: BTreeMap::new(),
        elems: 1,
        elem_size: 4,
        dims: Vec::new(),
        current: Some(ControlValue::Int(current)),
    }
}

/// A boolean control, the shape most automation controls have.
pub(crate) fn boolean(slug: &str, current: i64) -> ControlDesc {
    ControlDesc {
        control_type: ControlType::Boolean,
        range: ControlRange {
            min: 0,
            max: 1,
            step: 1,
        },
        default: 0,
        ..integer(slug, current)
    }
}

/// The same control with extra raw flag bits set.
pub(crate) fn flagged(desc: ControlDesc, bits: u32) -> ControlDesc {
    ControlDesc {
        flags: ControlFlags::from_raw(desc.flags.raw | bits),
        ..desc
    }
}

/// A stable, distinct id per slug, so two controls in one fixture never collide and a
/// failure message names something reproducible.
fn control_id(slug: &str) -> u32 {
    // FNV-1a, because it is four lines and the property needed is distinctness rather
    // than distribution. The high bit is cleared so ids stay in the range the kernel uses.
    let mut hash = 0x811c_9dc5u32;
    for byte in slug.as_bytes() {
        hash ^= u32::from(*byte);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash & 0x7fff_ffff
}

/// A YUYV frame of the size [`ScriptedCamera::formats`] offers.
pub(crate) fn frame(sequence: u32) -> Frame {
    Frame {
        // 32x16 YUYV is two bytes per pixel. The content is a constant: this double has
        // no physics, and a test that needed pixel content would be using the fake.
        bytes: vec![0x80; 32 * 16 * 2],
        pixel_format: PixelFormat::YUYV,
        width: 32,
        height: 16,
        bytes_per_line: 64,
        sequence,
        timestamp_us: i64::from(sequence) * 33_333,
    }
}
