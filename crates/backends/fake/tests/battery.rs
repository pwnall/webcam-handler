//! The fake runs the backend conformance battery, from P0 (design G0).
//!
//! And — the half that makes the first half mean something — three deliberately broken
//! backends, each wrong in one way the battery exists to catch. Rubric rule 2: for every
//! test, write the buggy implementation and watch it fail. A battery that has only ever
//! been run against a passing backend is a battery nobody has shown can go red.
//!
//! The broken variants wrap the real backend rather than reimplementing one, so each is
//! *exactly* the honest fake plus one defect and the failure it produces cannot be
//! attributed to anything else.

use std::collections::BTreeMap;
use std::time::Instant;

use fake::FakeBackend;
use schema::backend::{BackendKind, Camera, CameraBackend, HotplugWatch};
use schema::camera::{CameraId, CameraInfo, FormatInfo};
use schema::capture::{Frame, NegotiatedStream, StreamRequest};
use schema::control::{Applied, ControlDesc, ControlId, ControlValue};
use schema::error::{Error, Result};
use testkit::battery::{self, ArmOutcome, BatteryArm};
use testkit::fixtures;

/// The one arm no backend can run through T1/T2, declared with its reason.
///
/// Written out here rather than imported from the battery, because a backend author's
/// declaration is a *claim about their backend* — the fake's fault menu is walked
/// exhaustively in `tests/faults.rs`, and this line is where that is asserted to be the
/// arrangement.
fn declared_skips() -> BTreeMap<BatteryArm, String> {
    BTreeMap::from([(
        BatteryArm::FaultMenu,
        "the fake's fault menu is walked exhaustively by crates/backends/fake/tests/faults.rs; \
         the T1/T2 surface the battery speaks has no fault-scripting seam"
            .to_owned(),
    )])
}

fn honest_backend() -> FakeBackend {
    FakeBackend::from_profile(fixtures::synthetic_basic()).expect("the fixture replays")
}

#[test]
fn the_fake_passes_the_battery() {
    let report = battery::run(&honest_backend(), &declared_skips());
    assert!(report.is_green(), "{report}");

    // Green is not enough: an arm that skipped for a *declared* reason would also be
    // green, so every arm but the declared one must have actually run.
    for &arm in BatteryArm::ALL {
        let outcome = report.outcome(arm);
        if arm == BatteryArm::FaultMenu {
            assert!(
                matches!(outcome, Some(ArmOutcome::Skipped { .. })),
                "{arm} should have skipped: {report}"
            );
        } else {
            assert!(
                outcome.is_some_and(ArmOutcome::ran),
                "{arm} did not run: {report}"
            );
        }
    }
}

#[test]
fn a_backend_that_enumerates_one_camera_twice_fails_the_enumeration_arm() {
    let broken = DuplicateIds(honest_backend());
    let report = battery::run(&broken, &declared_skips());

    assert!(!report.is_green(), "{report}");
    let complaints = report.failures_for(BatteryArm::Enumeration);
    assert!(
        complaints.iter().any(|f| f.contains("not unique")),
        "{report}"
    );
}

#[test]
fn a_backend_that_refuses_an_out_of_range_write_fails_the_write_arm() {
    // The E5 violation this catches: the V4L2 specification *permits* ERANGE, and
    // uvcvideo clamps instead [PF:6]. A fake that refused would let the engine ship a
    // "the device rejected it" path no device ever takes, and hide the read-back the
    // whole doctrine exists for.
    let broken = RefusesToClamp(honest_backend());
    let report = battery::run(&broken, &declared_skips());

    assert!(!report.is_green(), "{report}");
    let complaints = report.failures_for(BatteryArm::WriteReadBack);
    assert!(
        complaints
            .iter()
            .any(|f| f.contains("clamped success, never an error")),
        "{report}"
    );
}

#[test]
fn a_backend_that_corrects_an_out_of_range_current_fails_the_control_model_arm() {
    // PF:4's inverse: `zoom_continuous` reads 245 out of a declared `[-100..100]`, and a
    // backend that tidied that away would erase the finding from every test above it.
    let broken = CorrectsOutOfRange(honest_backend());
    let report = battery::run(&broken, &declared_skips());

    assert!(!report.is_green(), "{report}");
    let complaints = report.failures_for(BatteryArm::ControlModel);
    assert!(
        complaints
            .iter()
            .any(|f| f.contains("zoom_continuous") && f.contains("never corrected [PF:4]")),
        "{report}"
    );
}

#[test]
fn a_backend_that_substitutes_a_format_the_camera_lacks_fails_the_explicit_request_arm() {
    // **The defect the G6 review found in `webcam-handler-v4l2`, wearing the fake.** D5's
    // "an explicit request still wins … or a typed refusal" was enforced in this crate and
    // nowhere else, so a caller naming a format the camera does not enumerate was answered
    // with a photograph in another one on the only backend attached to a real device — for
    // three phases, with two tests pinning the contract and both of them running over the
    // stand-in that honoured it (note **N134**).
    //
    // The repair moved the rule into the shared resolver, and this backend is what proves
    // the battery arm can still see it: a backend that drops the name before resolving is
    // exactly what the real one used to be.
    let broken = SubstitutesAnAbsentFormat(honest_backend());
    let report = battery::run(&broken, &declared_skips());

    assert!(!report.is_green(), "{report}");
    let complaints = report.failures_for(BatteryArm::ExplicitRequest);
    assert!(
        complaints
            .iter()
            .any(|f| f.contains("asked for WCHX") && f.contains("got a stream in")),
        "{report}"
    );
}

#[test]
fn a_backend_that_substitutes_a_size_nothing_fits_fails_the_explicit_request_arm() {
    // The half that needed no hardware to measure and was true of *both* backends: a
    // `--size` smaller than every mode the camera has resolved to the format's largest,
    // which on the OBSBOT is 3840x2160 for a request of 320x240 (owner ruling, 2026-08-16
    // — note **N134**).
    let broken = SubstitutesAnUnfittableSize(honest_backend());
    let report = battery::run(&broken, &declared_skips());

    assert!(!report.is_green(), "{report}");
    let complaints = report.failures_for(BatteryArm::ExplicitRequest);
    assert!(
        complaints
            .iter()
            .any(|f| f.contains("asked for 1x1") && f.contains("is not an adjustment")),
        "{report}"
    );
}

// ------------------------------------------------------------------ broken variants

/// Enumerates its first camera twice, so two cameras share an id.
#[derive(Debug)]
struct DuplicateIds(FakeBackend);

impl CameraBackend for DuplicateIds {
    fn kind(&self) -> BackendKind {
        self.0.kind()
    }

    fn enumerate(&self) -> Result<Vec<CameraInfo>> {
        let mut cameras = self.0.enumerate()?;
        if let Some(first) = cameras.first().cloned() {
            cameras.push(first);
        }
        Ok(cameras)
    }

    fn open(&self, id: &CameraId) -> Result<Box<dyn Camera>> {
        self.0.open(id)
    }

    fn watch(&self) -> Result<Box<dyn HotplugWatch>> {
        self.0.watch()
    }
}

/// Turns a clamped write into an error, the way the specification allows and no UVC
/// driver does.
#[derive(Debug)]
struct RefusesToClamp(FakeBackend);

impl CameraBackend for RefusesToClamp {
    fn kind(&self) -> BackendKind {
        self.0.kind()
    }

    fn enumerate(&self) -> Result<Vec<CameraInfo>> {
        self.0.enumerate()
    }

    fn open(&self, id: &CameraId) -> Result<Box<dyn Camera>> {
        Ok(Box::new(StrictCamera(self.0.open(id)?)))
    }

    fn watch(&self) -> Result<Box<dyn HotplugWatch>> {
        self.0.watch()
    }
}

#[derive(Debug)]
struct StrictCamera(Box<dyn Camera>);

impl Camera for StrictCamera {
    fn info(&self) -> &CameraInfo {
        self.0.info()
    }

    fn formats(&self) -> Result<Vec<FormatInfo>> {
        self.0.formats()
    }

    fn controls(&self) -> Result<Vec<ControlDesc>> {
        self.0.controls()
    }

    fn get(&mut self, id: ControlId) -> Result<ControlValue> {
        self.0.get(id)
    }

    fn set(&mut self, id: ControlId, value: ControlValue) -> Result<Applied> {
        let out_of_range = self
            .0
            .controls()?
            .iter()
            .find(|desc| desc.id == id)
            .zip(value.as_int())
            .is_some_and(|(desc, wanted)| !desc.range.contains(wanted));
        if out_of_range {
            return Err(Error::DeviceIo {
                operation: "VIDIOC_S_EXT_CTRLS".to_owned(),
                // ERANGE — what the specification permits and the hardware does not do.
                errno: Some(34),
                message: "Numerical result out of range".to_owned(),
            });
        }
        self.0.set(id, value)
    }

    fn start_stream(&mut self, request: &StreamRequest) -> Result<NegotiatedStream> {
        self.0.start_stream(request)
    }

    fn streaming(&self) -> Option<NegotiatedStream> {
        self.0.streaming()
    }

    fn next_frame(&mut self, deadline: Instant) -> Result<Frame> {
        self.0.next_frame(deadline)
    }

    fn stop_stream(&mut self) -> Result<()> {
        self.0.stop_stream()
    }
}

/// Drops a named pixel format the camera does not offer, and lets the ranking answer —
/// which is what `webcam-handler-v4l2` did until 2026-08-16.
#[derive(Debug)]
struct SubstitutesAnAbsentFormat(FakeBackend);

impl CameraBackend for SubstitutesAnAbsentFormat {
    fn kind(&self) -> BackendKind {
        self.0.kind()
    }

    fn enumerate(&self) -> Result<Vec<CameraInfo>> {
        self.0.enumerate()
    }

    fn open(&self, id: &CameraId) -> Result<Box<dyn Camera>> {
        Ok(Box::new(SubstitutingCamera {
            inner: self.0.open(id)?,
            drop_format: true,
            drop_size: false,
        }))
    }

    fn watch(&self) -> Result<Box<dyn HotplugWatch>> {
        self.0.watch()
    }
}

/// Drops a named size no mode fits, and lets the format's largest answer — which is what
/// *both* backends did until the owner's ruling of 2026-08-16.
#[derive(Debug)]
struct SubstitutesAnUnfittableSize(FakeBackend);

impl CameraBackend for SubstitutesAnUnfittableSize {
    fn kind(&self) -> BackendKind {
        self.0.kind()
    }

    fn enumerate(&self) -> Result<Vec<CameraInfo>> {
        self.0.enumerate()
    }

    fn open(&self, id: &CameraId) -> Result<Box<dyn Camera>> {
        Ok(Box::new(SubstitutingCamera {
            inner: self.0.open(id)?,
            drop_format: false,
            drop_size: true,
        }))
    }

    fn watch(&self) -> Result<Box<dyn HotplugWatch>> {
        self.0.watch()
    }
}

/// A camera that answers an unanswerable request by quietly forgetting the half it cannot
/// answer.
///
/// One double with two switches rather than two doubles, because the defect is one shape —
/// "resolve what is left" — and the two halves of the contract differ only in which field
/// gets forgotten.
#[derive(Debug)]
struct SubstitutingCamera {
    inner: Box<dyn Camera>,
    drop_format: bool,
    drop_size: bool,
}

impl Camera for SubstitutingCamera {
    fn info(&self) -> &CameraInfo {
        self.inner.info()
    }

    fn formats(&self) -> Result<Vec<FormatInfo>> {
        self.inner.formats()
    }

    fn controls(&self) -> Result<Vec<ControlDesc>> {
        self.inner.controls()
    }

    fn get(&mut self, id: ControlId) -> Result<ControlValue> {
        self.inner.get(id)
    }

    fn set(&mut self, id: ControlId, value: ControlValue) -> Result<Applied> {
        self.inner.set(id, value)
    }

    fn start_stream(&mut self, request: &StreamRequest) -> Result<NegotiatedStream> {
        let offered = self.inner.formats()?;
        let mut request = request.clone();
        if self.drop_format
            && request
                .pixel_format
                .is_some_and(|wanted| !offered.iter().any(|f| f.pixel_format == wanted))
        {
            request.pixel_format = None;
        }
        if self.drop_size
            && let (Some(width), Some(height)) = (request.width, request.height)
            && !offered
                .iter()
                .flat_map(|format| format.sizes.iter())
                .any(|entry| entry.size.largest_within(width, height).is_some())
        {
            request.width = None;
            request.height = None;
        }
        self.inner.start_stream(&request)
    }

    fn streaming(&self) -> Option<NegotiatedStream> {
        self.inner.streaming()
    }

    fn next_frame(&mut self, deadline: Instant) -> Result<Frame> {
        self.inner.next_frame(deadline)
    }

    fn stop_stream(&mut self) -> Result<()> {
        self.inner.stop_stream()
    }
}

/// Tidies every current value into its declared range on the way out.
#[derive(Debug)]
struct CorrectsOutOfRange(FakeBackend);

impl CameraBackend for CorrectsOutOfRange {
    fn kind(&self) -> BackendKind {
        self.0.kind()
    }

    fn enumerate(&self) -> Result<Vec<CameraInfo>> {
        self.0.enumerate()
    }

    fn open(&self, id: &CameraId) -> Result<Box<dyn Camera>> {
        Ok(Box::new(TidyCamera(self.0.open(id)?)))
    }

    fn watch(&self) -> Result<Box<dyn HotplugWatch>> {
        self.0.watch()
    }
}

#[derive(Debug)]
struct TidyCamera(Box<dyn Camera>);

impl Camera for TidyCamera {
    fn info(&self) -> &CameraInfo {
        self.0.info()
    }

    fn formats(&self) -> Result<Vec<FormatInfo>> {
        self.0.formats()
    }

    fn controls(&self) -> Result<Vec<ControlDesc>> {
        let mut controls = self.0.controls()?;
        for desc in &mut controls {
            if let Some(ControlValue::Int(value)) = desc.current
                && !desc.range.contains(value)
            {
                desc.current = Some(ControlValue::Int(
                    value.clamp(desc.range.min, desc.range.max),
                ));
            }
        }
        Ok(controls)
    }

    fn get(&mut self, id: ControlId) -> Result<ControlValue> {
        self.0.get(id)
    }

    fn set(&mut self, id: ControlId, value: ControlValue) -> Result<Applied> {
        self.0.set(id, value)
    }

    fn start_stream(&mut self, request: &StreamRequest) -> Result<NegotiatedStream> {
        self.0.start_stream(request)
    }

    fn streaming(&self) -> Option<NegotiatedStream> {
        self.0.streaming()
    }

    fn next_frame(&mut self, deadline: Instant) -> Result<Frame> {
        self.0.next_frame(deadline)
    }

    fn stop_stream(&mut self) -> Result<()> {
        self.0.stop_stream()
    }
}
