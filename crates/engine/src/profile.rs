//! Assembling a device profile from an open camera (design T3).
//!
//! One home for the T3 split. `webcam-handler-cli profile capture` writes what this produces;
//! the hardware rung compares a live capture against the committed corpus by calling the same
//! function; the daemon's `profile_capture` method will call it at P4. A second copy would be
//! a second opinion about which fields are invariant, and the whole value of the corpus rests
//! on that answer being one answer.
//!
//! The split, restated where it is implemented:
//!
//! - **invariant** — identity, nodes, formats, and the control set with `current` cleared
//!   and the volatile flag bits masked out. This is what "the corpus still resembles the
//!   device" means, and it compares exactly.
//! - **state** — the current values and the raw flag words, which change with use \[PF:3,
//!   PF:4\]. Re-capturing after somebody used the camera must not read as corpus drift.
//!
//! Provenance rides outside both and is never compared.

use schema::backend::{BackendKind, Camera};
use schema::error::Result;
use schema::limits;
use schema::pairing::AutomationPair;
use schema::profile::{
    DeviceProfile, ProfileInvariant, ProfileProvenance, ProfileState, invariant_control,
};
use schema::time::Stamp;

use crate::discover::Discovery;

/// Who and what took a capture. Supplied by the caller because none of it is the engine's
/// to know: the clock, the tool's version, and the person are all facts about the run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureContext {
    /// When. Passed in rather than read here, because the engine reads no clock.
    pub captured_at: Stamp,
    /// The capturing host's kernel release.
    pub kernel: String,
    /// The tool version that took it.
    pub tool_version: String,
    /// Who or what ran the capture.
    pub capturer: String,
    /// Which backend produced it. A profile captured from the fake backend would be
    /// circular corpus, and this field is what makes that visible.
    pub backend: BackendKind,
}

/// Capture everything `camera` can enumerate about itself.
///
/// # Errors
///
/// Whatever the camera says when asked for its formats or controls, and
/// [`schema::Error::IllegalTransition`] naming any control the device declined to read. A
/// capture that could not read part of the device is not a partial profile — it is not a
/// profile, and until 2026-08-16 that sentence was true only because the control walk
/// stopped on the first declined read. Now that the walk carries such a control valueless
/// (rule 7, note **N192**), the refusal has to be made here or a corpus entry silently
/// records a state nobody measured (note **N195**).
pub fn capture(camera: &mut dyn Camera, context: &CaptureContext) -> Result<DeviceProfile> {
    assemble(camera, context, Vec::new())
}

/// Ask the device which controls it pairs, then capture it \[PF:3\].
///
/// **This writes to the camera**, which is the whole of why it is a second function rather
/// than a flag on [`capture`]. That distinction is load-bearing at the call site: a corpus
/// entry is the document every resemblance claim is compared against, and "was this device
/// perturbed to produce it" is a fact a reader of `corpus/` must be able to establish from
/// the verb that was run rather than by reading the profile and guessing.
///
/// ## The decision this function is, recorded where it was taken
///
/// [`ProfileInvariant::measured_pairs`] has been in the schema since T3 and no committed
/// profile carried one until **2026-08-17**, because [`capture`] is a read and D3's probe is
/// a write. The comment that used to sit on the empty `Vec` said the choice belonged "to
/// whoever needs measured pairs in a profile", and the G6 review's finding **M30** is that
/// party: design §3.2 requires every profile-shaped PF finding to be *representable in and
/// asserted from* at least one committed profile, it names PF:3, and PF:3's finding is that
/// INACTIVE tracks pairing **live, in both directions**. A flag word captured with one bit
/// set is not that claim — it is a photograph of one moment — and the only field in the
/// document that can carry the claim is this one. So the answer to "may a corpus entry
/// perturb its subject" is *yes, when the caller says so and the probe puts it back*, and
/// the two halves of that sentence are the two things this function does.
///
/// It is **not** what `webcam-handler-cli profile capture` does by default, for the reason
/// above and one more: the probe walks every automation-shaped control on the device, and a
/// capture is the one operation somebody runs on hardware they have just met.
///
/// ## What the caller gets back besides the profile
///
/// The whole [`Discovery`], because the restore is a promise and a promise nobody checked is
/// docs/8 Part C's subject. A caller that discards it has captured a profile without
/// establishing that the camera came back, and [`Discovery::left_the_camera_alone`] is the
/// one question worth asking of it. The pairs are in the profile; `skipped` and `restored`
/// are facts about the run and belong to the run.
///
/// The probe runs **first**, for [`crate::discover::report`]'s reason: it writes, so the
/// control set read afterwards is the one the camera is actually in. `context.captured_at`
/// is the clock for both — the engine reads none, and a capture that dated its snapshot from
/// a second reading would be one document with two times in it.
///
/// # Errors
///
/// Whatever [`crate::discover::pairs`] says — including a refusal to begin, when the
/// pre-probe snapshot could not be taken — and then whatever [`capture`] says.
pub fn capture_probed(
    camera: &mut dyn Camera,
    context: &CaptureContext,
) -> Result<(DeviceProfile, Discovery)> {
    let found = crate::discover::pairs(camera, context.captured_at)?;
    let profile = assemble(camera, context, found.pairs.clone())?;
    Ok((profile, found))
}

/// The T3 split itself, over a control set read now and a pair set measured or not.
///
/// One home for the assembly, so the read-only verb and the probing one cannot disagree
/// about which fields are invariant — the whole value of the corpus rests on that being one
/// answer, and it is the reason this module exists rather than each root building a document.
fn assemble(
    camera: &mut dyn Camera,
    context: &CaptureContext,
    measured_pairs: Vec<AutomationPair>,
) -> Result<DeviceProfile> {
    let info = camera.info().clone();
    let formats = camera.formats()?;
    let controls = camera.controls()?;
    // Every control, not only the writable ones: `state.values` records the read-only ones
    // too, and a corpus entry is a *reading*, not a plan to write.
    let declined: Vec<&str> = controls
        .iter()
        .filter(|desc| desc.value_was_declined())
        .map(|desc| desc.slug.as_str())
        .collect();
    if !declined.is_empty() {
        return Err(crate::refusal::illegal(
            format!("unread_controls({})", declined.join(",")),
            format!("profile capture {}", info.id),
        ));
    }

    Ok(DeviceProfile {
        schema_version: limits::PROFILE_SCHEMA_VERSION,
        provenance: ProfileProvenance {
            captured_at: context.captured_at,
            kernel: context.kernel.clone(),
            tool_version: context.tool_version.clone(),
            capturer: context.capturer.clone(),
            backend: context.backend,
        },
        invariant: ProfileInvariant {
            info,
            formats,
            controls: controls.iter().map(invariant_control).collect(),
            // Empty when the caller came through `capture`, which reads and does not write:
            // it says "this capture measured nothing", never "this device has no pairs".
            // `capture_probed` is the caller that fills it, and its doc carries the decision
            // that let it — a corpus entry may perturb its subject when the verb that took
            // it said so out loud and the probe put the camera back (note **N239**).
            measured_pairs,
        },
        state: ProfileState {
            values: controls
                .iter()
                .filter_map(|desc| desc.current.clone().map(|v| (desc.slug.clone(), v)))
                .collect(),
            flags: controls
                .iter()
                .map(|desc| (desc.slug.clone(), desc.flags.raw))
                .collect(),
        },
    })
}

/// Read a committed device profile from `path`, refusing one this build cannot replay.
///
/// The other half of T3's round trip, and here for [`capture`]'s reason: **both** composition
/// roots read these documents to build a fake backend — `webcam-handler-cli --backend fake
/// --profile …` and `webcam-handler-daemon --backend fake --profile …` — and a version check
/// written at each of them is two answers to "can this build replay this document". Design
/// §2.11 says a backend is constructed at the roots and nowhere else; what the roots must not
/// each own is the *reading*.
///
/// The version is read from a probe that deserializes **only** `schema_version`, which is
/// the same shape `crate::store` uses for a session document and for the same reason: a
/// document this build cannot represent is refused before anything tries to represent it,
/// so a profile from a future version is refused *for its version* rather than for whichever
/// field this build's shape happens to be missing. `fake::FakeBackend::new` checks the
/// version again from its own values, which is not redundant — a profile can reach a backend
/// without passing through a file.
///
/// # Errors
///
/// [`schema::Error::StorageIo`] naming the path when it cannot be read, is not JSON, carries
/// no `schema_version`, or does not deserialize; and [`schema::Error::SchemaVersionForeign`]
/// for a version this build does not speak (D9's doctrine, applied to the corpus: a foreign
/// document is a typed refusal, never a best-effort parse).
pub fn read(path: &camino::Utf8Path) -> Result<DeviceProfile> {
    let bytes = std::fs::read(path).map_err(|error| schema::Error::StorageIo {
        path: path.to_owned(),
        errno: error.raw_os_error(),
        message: error.to_string(),
    })?;
    let unreadable = |message: String| schema::Error::StorageIo {
        path: path.to_owned(),
        errno: None,
        message,
    };

    let probe: VersionProbe = serde_json::from_slice(&bytes)
        .map_err(|error| unreadable(format!("is not a JSON document: {error}")))?;
    match probe.schema_version {
        None => {
            return Err(unreadable(
                "carries no schema_version; every device profile this tool writes has one, \
                 so this file was not written by it"
                    .to_owned(),
            ));
        }
        Some(found) if found != limits::PROFILE_SCHEMA_VERSION => {
            return Err(schema::Error::SchemaVersionForeign {
                found,
                supported: limits::PROFILE_SCHEMA_VERSION,
            });
        }
        Some(_) => {}
    }

    serde_json::from_slice(&bytes)
        .map_err(|error| unreadable(format!("is not a device profile: {error}")))
}

/// `uname -r`, for a [`CaptureContext`]'s provenance.
///
/// Read from `/proc/sys/kernel/osrelease` rather than by running `uname`: design §1 bans
/// runtime external binaries, and this is one line of a pseudo-file. A host without
/// `/proc` records the absence rather than a guess.
///
/// It lives beside the field it fills rather than in a composition root, because P4c gave it a
/// second one: `webcam-handler-cli profile capture` and `webcam-handler-daemon`'s
/// `wch_profile_capture` write the same document, and two readings of one host fact could
/// disagree about what `"(unknown)"` means. It is *not* part of [`CaptureContext`]'s
/// construction, because the rest of that value — the clock, the tool version, who asked — is
/// the caller's to supply and this is the only field a host can answer for itself.
#[must_use]
pub fn kernel_release() -> String {
    std::fs::read_to_string("/proc/sys/kernel/osrelease")
        .map(|text| text.trim().to_owned())
        .unwrap_or_else(|_| "(unknown)".to_owned())
}

/// Only the field that decides whether the rest may be read.
///
/// The same probe `crate::store` uses on a session document, spelled again rather than
/// shared because the two are different documents with different versions — `store`'s
/// answers about [`schema::limits::SESSION_SCHEMA_VERSION`] and this one about
/// [`schema::limits::PROFILE_SCHEMA_VERSION`], and a shared struct would invite one call
/// site to check the other's number.
#[derive(Debug, serde::Deserialize)]
struct VersionProbe {
    schema_version: Option<u32>,
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::time::Instant;

    use schema::backend::Camera;
    use schema::camera::{
        CameraFingerprint, CameraId, CameraInfo, DeviceNode, FormatInfo, NodeKind,
    };
    use schema::control::{
        Applied, ControlDesc, ControlFlags, ControlId, ControlRange, ControlSlug, ControlType,
        ControlValue, KnownFlag,
    };
    use schema::error::Error;

    use super::*;

    /// A camera that answers with exactly what it was built with. Not the fake backend:
    /// this module's subject is the *split*, and a double that returns fixed values keeps
    /// the test about that rather than about a replay engine.
    #[derive(Debug)]
    struct StubCamera {
        info: CameraInfo,
        controls: Vec<ControlDesc>,
    }

    impl Camera for StubCamera {
        fn info(&self) -> &CameraInfo {
            &self.info
        }

        fn formats(&self) -> Result<Vec<FormatInfo>> {
            Ok(Vec::new())
        }

        fn controls(&self) -> Result<Vec<ControlDesc>> {
            Ok(self.controls.clone())
        }

        fn get(&mut self, _id: ControlId) -> Result<ControlValue> {
            Err(not_this_double("get"))
        }

        fn set(&mut self, _id: ControlId, _value: ControlValue) -> Result<Applied> {
            Err(not_this_double("set"))
        }

        fn start_stream(
            &mut self,
            _request: &schema::capture::StreamRequest,
        ) -> Result<schema::capture::NegotiatedStream> {
            Err(not_this_double("start_stream"))
        }

        fn streaming(&self) -> Option<schema::capture::NegotiatedStream> {
            // Not a refusal: a camera that cannot start a stream is never running one,
            // and this is the one method on the trait whose honest answer is a value
            // rather than an error.
            None
        }

        fn next_frame(&mut self, _deadline: Instant) -> Result<schema::capture::Frame> {
            Err(not_this_double("next_frame"))
        }

        fn stop_stream(&mut self) -> Result<()> {
            Err(not_this_double("stop_stream"))
        }
    }

    /// What this double answers to the `Camera` methods its subject does not reach.
    ///
    /// It used to answer `Error::Unimplemented { arrives_in: "never" }`, and note N43
    /// flagged that as the wrong shape while the variant still existed — "a test double
    /// saying 'not this one' is not a phase schedule". P4d deleted the variant (N6), so
    /// the replacement had to be chosen rather than inherited, and three of the registry's
    /// eighteen are ruled out by law: a panic breaks the "plugging in a webcam cannot
    /// panic the library" rule, `FormatUnsupported` would be a capability answer about a
    /// device that was never asked (E3), and `DeviceIo` would blame a kernel that is not
    /// in this test at all.
    ///
    /// [`Error::IllegalTransition`] is what is left and it is also what is meant: *this
    /// object does not do that*, named with the operation attempted. `cli-core`'s
    /// `unreachable_photo` already uses it for the same shape of refusal, so this is the
    /// registry's existing vocabulary for it rather than a new reading of a variant.
    fn not_this_double(op: &str) -> Error {
        Error::IllegalTransition {
            from: "stub_camera".to_owned(),
            op: op.to_owned(),
        }
    }

    fn control(slug: &str, raw_flags: u32, current: i64) -> ControlDesc {
        ControlDesc {
            id: ControlId(1),
            name: slug.to_owned(),
            slug: ControlSlug::parse(slug).expect("literal slug"),
            control_type: ControlType::Integer,
            range: ControlRange {
                min: 0,
                max: 100,
                step: 1,
            },
            default: 50,
            flags: ControlFlags::from_raw(raw_flags),
            menu: BTreeMap::new(),
            elems: 1,
            elem_size: 4,
            dims: Vec::new(),
            current: Some(ControlValue::Int(current)),
        }
    }

    fn stub(controls: Vec<ControlDesc>) -> StubCamera {
        StubCamera {
            info: CameraInfo {
                id: CameraId::parse("cam:test").expect("literal id"),
                fingerprint: CameraFingerprint {
                    bus_path: "3-4:1.0".to_owned(),
                    usb_id: None,
                    card: "Test".to_owned(),
                    driver: "uvcvideo".to_owned(),
                    serial: None,
                },
                card: "Test".to_owned(),
                driver: "uvcvideo".to_owned(),
                bus_info: "usb-1".to_owned(),
                nodes: vec![DeviceNode {
                    path: "/dev/video0".into(),
                    kind: NodeKind::VideoCapture,
                    device_caps: 0x0420_0001,
                    capabilities: 0x84a0_0001,
                }],
                backend: BackendKind::V4l2,
            },
            controls,
        }
    }

    fn context() -> CaptureContext {
        CaptureContext {
            captured_at: Stamp::epoch(),
            kernel: "7.0.0-29-generic".to_owned(),
            tool_version: "0.1.0".to_owned(),
            capturer: "test".to_owned(),
            backend: BackendKind::V4l2,
        }
    }

    #[test]
    fn a_capture_the_device_declined_part_of_is_refused_rather_than_committed_as_a_profile() {
        // This function's own doc: "a capture that could not read part of the device is
        // not a partial profile — it is not a profile." It stopped being true when the
        // control walk began carrying a declined read as a valueless control: the control
        // kept its `invariant` row and quietly dropped out of `state.values`, so a corpus
        // entry captured while another process held one control's device function would
        // be committed as the device's measured state (note **N195**).
        //
        // The corpus is what every `resemblance` claim is compared against, so a profile
        // that is missing a value nobody can tell was missing is drift with provenance on
        // it.
        let mut camera = stub(vec![ControlDesc {
            current: None,
            ..control("auto_exposure", 0, 1)
        }]);
        let error = capture(&mut camera, &context())
            .expect_err("a device that would not answer for a control was not fully read");
        assert_eq!(error.kind(), schema::ErrorKind::IllegalTransition);
        assert!(error.to_string().contains("auto_exposure"), "{error}");

        // The inverse, and the reason the predicate is the descriptor's: a control that
        // *declares* it has no value was fully read, and a device with a button on it must
        // still be capturable.
        let mut with_a_button = stub(vec![ControlDesc {
            control_type: ControlType::Button,
            current: None,
            ..control("reset", 0, 0)
        }]);
        let profile = capture(&mut with_a_button, &context()).expect("captures");
        assert_eq!(profile.invariant.controls.len(), 1);
        assert!(profile.state.values.is_empty());
    }

    #[test]
    fn the_invariant_section_carries_no_current_value_and_no_volatile_flag() {
        // The T3 split, at the point it is made. INACTIVE is set here because an
        // automation partner held the control when the capture was taken [PF:3], and it
        // must not reach the section that compares exactly.
        let mut camera = stub(vec![control("white_balance_temperature", 0x1010, 4600)]);
        let profile = capture(&mut camera, &context()).expect("captures");

        let slug = ControlSlug::parse("white_balance_temperature").expect("literal slug");
        let invariant = profile
            .control(&slug)
            .expect("the control is in the profile");
        assert_eq!(invariant.current, None);
        assert!(!invariant.flags.has(KnownFlag::Inactive));
        assert!(invariant.flags.has(KnownFlag::HasWhichMinMax));

        // …and the state block carries both.
        assert_eq!(
            profile.state.values.get(&slug),
            Some(&ControlValue::Int(4600))
        );
        assert_eq!(profile.state.flags.get(&slug), Some(&0x1010));
    }

    #[test]
    fn using_the_camera_between_two_captures_does_not_read_as_corpus_drift() {
        // The property the split exists for, end to end: capture, use the camera,
        // capture again, and the invariant sections must still compare equal.
        let fresh = capture(
            &mut stub(vec![control("white_balance_temperature", 0x1000, 4600)]),
            &context(),
        )
        .expect("captures");
        let after_use = capture(
            &mut stub(vec![control("white_balance_temperature", 0x1010, 6500)]),
            &context(),
        )
        .expect("captures");

        assert!(fresh.invariant_matches(&after_use));
        assert_ne!(fresh.state, after_use.state, "…but the state did move");
    }

    #[test]
    fn a_changed_device_does_read_as_drift() {
        // The inverse direction. If the *device* changes, the corpus must notice.
        let before =
            capture(&mut stub(vec![control("brightness", 0, 50)]), &context()).expect("captures");
        let mut moved = stub(vec![control("brightness", 0, 50)]);
        moved.controls[0].range.max = 255;
        let after = capture(&mut moved, &context()).expect("captures");
        assert!(!before.invariant_matches(&after));
    }

    #[test]
    fn a_control_with_no_readable_value_is_absent_from_the_state_block_not_zero_in_it() {
        // A button, a class header, or a write-only control has no current value, and
        // recording a 0 for it would be inventing device state.
        let mut valueless = control("user_controls", 0x44, 0);
        valueless.control_type = ControlType::ControlClass;
        valueless.current = None;
        let profile = capture(
            &mut stub(vec![valueless, control("brightness", 0, 50)]),
            &context(),
        )
        .expect("captures");

        assert_eq!(profile.state.values.len(), 1);
        assert!(
            !profile
                .state
                .values
                .contains_key(&ControlSlug::parse("user_controls").expect("literal slug"))
        );
        // It is still in the invariant section: the control exists, it just has no value.
        assert_eq!(profile.invariant.controls.len(), 2);
        // And its flags are recorded, because flags are readable even when values are not.
        assert_eq!(profile.state.flags.len(), 2);
    }

    #[test]
    fn a_captured_profile_reads_back_as_itself_and_a_foreign_one_is_refused_by_version() {
        // T3's round trip through the one reader both composition roots use. The document
        // is produced by `capture` rather than written out by hand, so this cannot drift
        // from the shape the tool actually writes.
        let dir = crate::paths::scratch_dir().expect("a scratch directory");
        let path = camino::Utf8PathBuf::from_path_buf(dir.path().join("profile.json"))
            .expect("a UTF-8 temporary directory");
        let captured =
            capture(&mut stub(vec![control("brightness", 0, 50)]), &context()).expect("captures");
        std::fs::write(
            &path,
            serde_json::to_vec(&captured).expect("a profile serializes"),
        )
        .expect("the temporary directory is writable");
        assert_eq!(read(&path).expect("this build wrote it"), captured);

        // A version this build does not speak is refused for its version — before
        // anything looks for the fields such a document would not have.
        let future = path.with_file_name("future.json");
        std::fs::write(&future, br#"{"schema_version":99}"#).expect("writable");
        assert_eq!(
            read(&future)
                .expect_err("99 is not this build's version")
                .kind(),
            schema::ErrorKind::SchemaVersionForeign
        );

        // And the two ways of not being a profile at all, each naming the path — which is
        // the whole reason this refusal is not left to the backend, where the path is gone.
        let garbage = path.with_file_name("garbage.json");
        std::fs::write(&garbage, b"not json").expect("writable");
        let err = read(&garbage).expect_err("not a device profile");
        assert_eq!(err.kind(), schema::ErrorKind::StorageIo);
        assert!(err.to_string().contains("garbage.json"), "{err}");

        let missing = path.with_file_name("nowhere.json");
        let err = read(&missing).expect_err("no such file");
        assert_eq!(err.kind(), schema::ErrorKind::StorageIo);
        assert!(err.to_string().contains("nowhere.json"), "{err}");

        // JSON, and even a plausible document, but from no version at all: a file this
        // tool did not write. Refused rather than read at whatever version this build
        // happens to be, which would be the best-effort parse D9 forbids.
        let versionless = path.with_file_name("versionless.json");
        let mut stripped = serde_json::to_value(&captured).expect("a profile is a JSON document");
        stripped
            .as_object_mut()
            .expect("a profile is a JSON object")
            .remove("schema_version");
        std::fs::write(
            &versionless,
            serde_json::to_vec(&stripped).expect("still a JSON document"),
        )
        .expect("writable");
        let err = read(&versionless).expect_err("no schema_version");
        assert_eq!(err.kind(), schema::ErrorKind::StorageIo);
        assert!(err.to_string().contains("schema_version"), "{err}");
    }

    #[test]
    fn the_capturing_backend_is_recorded_so_circular_corpus_is_visible() {
        let mut fake_context = context();
        fake_context.backend = BackendKind::Fake;
        let profile = capture(&mut stub(Vec::new()), &fake_context).expect("captures");
        assert_eq!(profile.provenance.backend, BackendKind::Fake);
        assert_eq!(profile.schema_version, limits::PROFILE_SCHEMA_VERSION);
    }

    #[test]
    fn the_kernel_release_is_read_without_running_a_program() {
        // Design §1: no runtime external binaries. On this host the file is there; on a
        // host without /proc the absence is recorded rather than guessed at. The test
        // moved here with the function, from `crates/cli`'s binary, when P4c gave the
        // provenance field a second author.
        let release = kernel_release();
        assert!(!release.is_empty());
        if std::path::Path::new("/proc/sys/kernel/osrelease").exists() {
            assert_ne!(release, "(unknown)");
            assert!(!release.contains('\n'), "{release:?} was not trimmed");
        }
    }
}
