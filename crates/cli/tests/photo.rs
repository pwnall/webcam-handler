//! `webcam-handler-cli photo` end to end, against the fake backend (docs/7 G2).
//!
//! G2 names this suite by what it must prove: a photo whose **EXIF reads back through an
//! independent reader**, and a photo from the **GREY-format chicony-ir profile**, because
//! D6 says grayscale is not optional and a pipeline that only ever sees RGB will discover
//! that on somebody's IR camera instead of here.
//!
//! Everything runs as a *subprocess*. The engine's own tests call `engine::photo::take`
//! and are the finer-grained ones; what these add is the half nothing else covers — that
//! the binary parses the flags into the request the engine gets, resolves `-o` the way
//! D10 says, and puts the bytes where it said it did.
//!
//! **The writer and the reader share no code.** `little_exif` writes and `kamadak-exif`
//! reads, which is docs/9's answer to write-only EXIF: a writer that returns `Ok` having
//! produced a segment no reader accepts passes every test that trusts it.
//!
//! Every photo lands in a scratch directory. A frame may contain a person (rubric A12) —
//! these are synthetic, and the habit is the point.

use std::process::Command;

use camino::{Utf8Path, Utf8PathBuf};
use exif::{In, Tag};

/// The `webcam-handler-cli` binary this test drives, built by cargo alongside it.
fn wch() -> Command {
    Command::new(env!("CARGO_BIN_EXE_webcam-handler-cli"))
}

/// A scratch directory under the one scratch root (note N84), and the paths under it.
struct Scratch {
    dir: tempfile::TempDir,
}

impl Scratch {
    fn new() -> Scratch {
        Scratch {
            dir: engine::paths::scratch_dir().expect("a scratch directory"),
        }
    }

    fn path(&self, name: &str) -> Utf8PathBuf {
        Utf8PathBuf::from_path_buf(self.dir.path().join(name)).expect("a utf-8 temp dir")
    }
}

/// The committed profile a camera is replayed from, and the id it enumerates as.
struct Replayed {
    profile: Utf8PathBuf,
    camera: String,
}

fn replayed(name: &str, camera: &str) -> Replayed {
    let profile = Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/profiles")
        .join(format!("{name}.json"));
    assert!(profile.exists(), "the corpus is missing {profile}");
    Replayed {
        profile,
        camera: camera.to_owned(),
    }
}

/// Run `webcam-handler-cli photo` against a replayed camera, returning stdout and stderr.
fn photo(device: &Replayed, extra: &[&str]) -> (String, String) {
    let output = wch()
        .args([
            "--backend",
            "fake",
            "--profile",
            device.profile.as_str(),
            "photo",
            &device.camera,
        ])
        .args(extra)
        .output()
        .expect("webcam-handler-cli runs");
    assert!(
        output.status.success(),
        "webcam-handler-cli photo {extra:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    (
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

/// The EXIF an independent reader finds in a file.
fn exif_of(path: &Utf8Path) -> exif::Exif {
    let bytes = std::fs::read(path).unwrap_or_else(|error| panic!("{path} is unreadable: {error}"));
    let mut cursor = std::io::Cursor::new(bytes);
    exif::Reader::new()
        .read_from_container(&mut cursor)
        .unwrap_or_else(|error| panic!("{path} carries no readable EXIF: {error}"))
}

fn field(exif: &exif::Exif, tag: Tag) -> String {
    exif.get_field(tag, In::PRIMARY)
        .unwrap_or_else(|| panic!("{tag} is missing"))
        .display_value()
        .to_string()
}

#[test]
fn wch_photo_produces_a_jpeg_whose_exif_reads_back_through_an_independent_reader() {
    let scratch = Scratch::new();
    let path = scratch.path("shot.jpg");
    let device = replayed("chicony-rgb", "cam:integrated-camera-integrated-c");
    photo(&device, &["-o", path.as_str()]);

    let bytes = std::fs::read(&path).expect("the file exists");
    assert!(
        bytes.starts_with(&[0xff, 0xd8]),
        "a .jpg that is not a JPEG is not a photo"
    );

    let exif = exif_of(&path);
    // The four fields D6 asks a photo to carry about itself, each read by a reader that
    // shares no code with the writer.
    let make = field(&exif, Tag::Make);
    assert!(make.contains("uvcvideo"), "{make}");
    assert!(
        field(&exif, Tag::Model).contains("Integrated Camera"),
        "the card name identifies the device"
    );
    assert!(
        field(&exif, Tag::Software).contains("webcam-handler"),
        "the tool version rides along"
    );
    assert_eq!(
        field(&exif, Tag::Orientation),
        "row 0 at top and column 0 at left",
        "an untransformed photo is orientation 1"
    );

    // The control values in effect, which is what makes a calibration sample
    // self-describing without its session file.
    //
    // Read as *bytes*, not through `display_value`: `UserComment` is an UNDEF tag, so a
    // reader renders it as hex — and the eight-byte `ASCII\0\0\0` prefix in front of the
    // text is the encoding declaration the standard requires. A test that compared the
    // rendered form would be asserting about the reader.
    let comment = exif
        .get_field(Tag::UserComment, In::PRIMARY)
        .expect("the comment is present");
    let exif::Value::Undefined(bytes, _) = &comment.value else {
        panic!("UserComment is an UNDEF tag, got {:?}", comment.value);
    };
    assert!(
        bytes.starts_with(b"ASCII\0\0\0"),
        "without the encoding declaration a reader sees an eight-byte-shifted string"
    );
    let text = String::from_utf8_lossy(&bytes[8..]);
    assert!(text.starts_with("controls: "), "{text}");
    assert!(text.contains("brightness=128"), "{text}");
}

#[test]
fn wch_photo_from_the_grey_camera_produces_a_decodable_jpeg_because_grayscale_is_not_optional() {
    // D6's own phrasing. The Chicony IR camera's only format is GREY, so a `.jpg` from it
    // is necessarily a re-encode — and the answer must say so rather than claiming a byte
    // fidelity it does not have.
    let scratch = Scratch::new();
    let path = scratch.path("ir.jpg");
    let device = replayed("chicony-ir", "cam:integrated-camera-integrated-i");
    let (stdout, _) = photo(&device, &["-o", path.as_str()]);

    assert!(
        stdout.contains("GREY"),
        "the report must name the format the device delivered: {stdout}"
    );
    assert!(
        !stdout.contains("unmodified"),
        "GREY is not a bitstream; there is nothing to pass through: {stdout}"
    );

    let bytes = std::fs::read(&path).expect("the file exists");
    assert!(bytes.starts_with(&[0xff, 0xd8]));
    // Decodable, not merely present: a zero-content `.jpg` starts with the same two bytes.
    let decoded = image::load_from_memory(&bytes).expect("a real JPEG decodes");
    assert_eq!((decoded.width(), decoded.height()), (640, 360));
    // And it still carries its EXIF, so the re-encoded path is as self-describing as the
    // verbatim one.
    assert!(field(&exif_of(&path), Tag::Make).contains("uvcvideo"));
}

#[test]
fn wch_photo_with_a_transform_stamps_the_orientation_rather_than_rotating_the_bytes() {
    // E6: rotating a pass-through JPEG must not cost a re-encode. Asserted from *outside*
    // the process, by comparing the two files' bytes: the rotated one is byte-identical
    // to the unrotated one except for its header, which is the strongest available form
    // of "nothing touched the pixels".
    let scratch = Scratch::new();
    let plain = scratch.path("plain.jpg");
    let turned = scratch.path("turned.jpg");
    let device = replayed("chicony-rgb", "cam:integrated-camera-integrated-c");

    photo(&device, &["-o", plain.as_str()]);
    photo(&device, &["-o", turned.as_str(), "--transform", "rot90"]);

    assert_eq!(
        field(&exif_of(&turned), Tag::Orientation),
        "row 0 at right and column 0 at top",
        "rot90 is EXIF orientation 6"
    );

    // The entropy-coded scan is everything after the last header segment; comparing the
    // *tails* is a cheap, honest stand-in that does not need a JPEG parser here. The fake
    // synthesizes deterministically, so two photos of the same scene share their scan.
    let plain_bytes = std::fs::read(&plain).expect("read");
    let turned_bytes = std::fs::read(&turned).expect("read");
    let tail = 4096.min(plain_bytes.len()).min(turned_bytes.len());
    assert_eq!(
        &plain_bytes[plain_bytes.len() - tail..],
        &turned_bytes[turned_bytes.len() - tail..],
        "the transform reached the pixels; on a pass-through JPEG it must not"
    );
}

/// `webcam-handler-cli --json photo diff <a> <b>`, parsed as the one document it prints.
///
/// A document verb (§2.7), so no backend and no profile: it reads two files and answers.
fn diff(a: &Utf8Path, b: &Utf8Path) -> schema::metrics::PhotoComparison {
    let output = wch()
        .args(["--json", "photo", "diff", a.as_str(), b.as_str()])
        .output()
        .expect("webcam-handler-cli runs");
    assert!(
        output.status.success(),
        "webcam-handler-cli photo diff failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("standard output carries a PhotoComparison")
}

/// `webcam-handler-cli --json photo …`, parsed as the one document that verb prints.
///
/// The sibling of [`photo`], which reads the human rendering. Both exist because the two
/// documents this suite reconciles are printed by two different runs of one verb.
fn photo_report(device: &Replayed, extra: &[&str]) -> schema::capture::PhotoReport {
    let output = wch()
        .args([
            "--json",
            "--backend",
            "fake",
            "--profile",
            device.profile.as_str(),
            "photo",
            &device.camera,
        ])
        .args(extra)
        .output()
        .expect("webcam-handler-cli runs");
    assert!(
        output.status.success(),
        "webcam-handler-cli --json photo {extra:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("standard output carries a PhotoReport")
}

/// One of the two classes of camera D6 partitions this build's JPEG sink by, and what each
/// one does with a requested transform.
///
/// **The partition is the population, and one profile is not it.** A camera whose frames
/// arrive already compressed gets the verbatim sink, where the orientation *is* the
/// transform; a camera that delivers raw pixels gets the re-encode sink, where the pixels
/// are turned and the tag has nothing left to say. Every arm below walks both rows, because
/// the defect these arms were written for — a stored raster and an Orientation tag both
/// carrying the same quarter turn, so every EXIF-aware reader turns it twice — is invisible
/// on the first row and lives on the second (note **N267**).
struct JpegSinkClass {
    /// The committed profile the camera is replayed from.
    profile: &'static str,
    /// The id it enumerates as.
    camera: &'static str,
    /// Where a `--transform rot90` goes on this class's JPEG, as the report states it.
    application: schema::capture::TransformApplication,
    /// What an independent EXIF reader must then find in that JPEG, in `kamadak-exif`'s
    /// own words.
    orientation: &'static str,
}

/// The two rows, named once. `chicony-ir`'s only format is GREY, which is why it is the
/// re-encode row — D6's "grayscale is not optional", read the other way round.
const JPEG_SINK_CLASSES: [JpegSinkClass; 2] = [
    JpegSinkClass {
        profile: "chicony-rgb",
        camera: "cam:integrated-camera-integrated-c",
        application: schema::capture::TransformApplication::ExifOrientation { orientation: 6 },
        orientation: "row 0 at right and column 0 at top",
    },
    JpegSinkClass {
        profile: "chicony-ir",
        camera: "cam:integrated-camera-integrated-i",
        application: schema::capture::TransformApplication::Pixels,
        orientation: "row 0 at top and column 0 at left",
    },
];

#[test]
fn photo_diff_scores_one_frame_written_to_both_sinks() {
    // One frame, one transform, two of this build's own sinks, and D17's whole purpose is that
    // the two are comparable. They are written differently on purpose (D6): on a camera whose
    // frames arrive compressed the JPEG sink passes the bitstream through and stamps the
    // rotation as an EXIF Orientation, and the PNG sink has no bitstream to pass through so it
    // turns the pixels. A reader that dropped the tag would therefore hand `photo diff`
    // 2592x1944 against 1944x2592 and refuse it a similarity score — which is what this arm is
    // here to notice, from outside the process, where the unit arms in `imaging::compare`
    // cannot look.
    //
    // Walked over both classes, because a writer that stamped a tag onto a raster it had
    // already turned would put the *re-encode* row back into two shapes while leaving this
    // row green.
    for class in &JPEG_SINK_CLASSES {
        let scratch = Scratch::new();
        let device = replayed(class.profile, class.camera);
        let turned_jpeg = scratch.path("turned.jpg");
        let turned_png = scratch.path("turned.png");
        photo(
            &device,
            &["-o", turned_jpeg.as_str(), "--transform", "rot90"],
        );
        photo(
            &device,
            &["-o", turned_png.as_str(), "--transform", "rot90"],
        );

        let turned = diff(&turned_jpeg, &turned_png);
        assert_eq!(
            (turned.a.width, turned.a.height),
            (turned.b.width, turned.b.height),
            "the tool's own JPEG and PNG of one rot90 frame from {} came back at two shapes: \
             {}x{} against {}x{}",
            class.profile,
            turned.a.width,
            turned.a.height,
            turned.b.width,
            turned.b.height
        );
        let score = turned.ssim.score().unwrap_or_else(|| {
            panic!(
                "the tool's own JPEG and PNG of one rot90 frame from {} have no similarity \
                 score: {:?}",
                class.profile, turned.ssim
            )
        });
        // A floor and not a range, because a range is a claim about the *type* and this arm
        // owes a claim about the *pixels*. Measured through the shipped binary: both sides are
        // one frame, so the pair scores 1.0 on the verbatim row and 0.9994 on the re-encode
        // row, while the pair a reader that turned one side the *other* way would produce —
        // this build's own rot90 and rot270 renderings of the same frame, which is what a
        // misread Orientation 6 amounts to — scores −0.1852 and 0.0563 on the same two
        // cameras. SSIM is defined on [-1, 1] and a range over it would have admitted both.
        assert!(
            score >= 0.9,
            "the tool's own JPEG and PNG of one rot90 frame from {} agree only to {score}: the \
             reader turned one of them a different way",
            class.profile
        );

        // The untransformed pair scores too, which is what stops a green run above from being a
        // run in which the rotation was the only thing being asserted — and what separates
        // "orientation honoured" from "orientation ignored on both sides".
        let plain_jpeg = scratch.path("plain.jpg");
        let plain_png = scratch.path("plain.png");
        photo(&device, &["-o", plain_jpeg.as_str()]);
        photo(&device, &["-o", plain_png.as_str()]);
        let plain = diff(&plain_jpeg, &plain_png);
        assert!(
            plain.ssim.score().is_some(),
            "an untransformed pair from the same frame of {} has no similarity score either: \
             {:?}",
            class.profile,
            plain.ssim
        );
        assert_eq!(
            (turned.a.width, turned.a.height),
            (plain.a.height, plain.a.width),
            "a quarter turn exchanges the two extents on {}, whichever sink wrote the file",
            class.profile
        );
    }
}

#[test]
fn a_jpeg_carries_the_orientation_its_own_raster_still_needs_and_no_other() {
    // The writer's half of the arm above, asserted where a similarity score cannot see it: a
    // photograph whose pixels were already turned must carry Orientation 1, and one whose
    // pixels were left alone must carry the tag that says to turn them. Stamping the
    // *requested* transform on both is the defect — the re-encode row comes out with turned
    // pixels and a tag saying to turn them again, which `photo diff` sees as two shapes and
    // every other EXIF-aware reader sees as a picture on its side.
    for class in &JPEG_SINK_CLASSES {
        let scratch = Scratch::new();
        let device = replayed(class.profile, class.camera);
        let path = scratch.path("turned.jpg");
        let report = photo_report(&device, &["-o", path.as_str(), "--transform", "rot90"]);

        assert_eq!(
            report.transform, class.application,
            "{} took its rot90 through {:?} and this suite's partition says {:?}; the two rows \
             below are the partition, so a class that changed sinks makes every claim here a \
             claim about one sink twice",
            class.profile, report.transform, class.application
        );
        assert_eq!(
            field(&exif_of(&path), Tag::Orientation),
            class.orientation,
            "{}'s rot90 JPEG went to the sink through {:?} and carries the wrong Orientation: a \
             raster that was already turned and a tag that says to turn it is a photograph \
             every EXIF-aware reader turns twice, and a raster nobody turned with a tag that \
             says so is one nobody turns at all",
            class.profile,
            report.transform
        );

        // And the stored raster itself, read by the `image` crate rather than by this build's
        // own reader, because the claim above is only worth having if the two answers are
        // about the same file: the re-encode row stores the turned raster and the verbatim row
        // stores the camera's.
        let stored = image::load_from_memory(&std::fs::read(&path).expect("the file exists"))
            .expect("a real JPEG decodes");
        let negotiated = (report.negotiated.width, report.negotiated.height);
        let expected = match class.application {
            schema::capture::TransformApplication::Pixels => (negotiated.1, negotiated.0),
            _ => negotiated,
        };
        assert_eq!(
            (stored.width(), stored.height()),
            expected,
            "{}'s rot90 JPEG stores a {}x{} raster where the {:?} path stores {}x{}",
            class.profile,
            stored.width(),
            stored.height(),
            report.transform,
            expected.0,
            expected.1
        );
    }
}

/// Which transforms exchange a photograph's two extents when they are honoured, as a table
/// this suite commits rather than computes.
///
/// The EXIF standard's own partition: 1, 2, 3 and 4 name permutations that keep the raster's
/// shape and 5–8 name the ones that turn it. Written out because the reconciliation below is
/// about two documents disagreeing, and a table derived from the code under test could not
/// notice them agreeing on the wrong answer.
const TRANSFORMS_THAT_EXCHANGE_THE_EXTENTS: [(schema::capture::Transform, bool); 6] = [
    (schema::capture::Transform::None, false),
    (schema::capture::Transform::HFlip, false),
    (schema::capture::Transform::VFlip, false),
    (schema::capture::Transform::Rot90, true),
    (schema::capture::Transform::Rot180, false),
    (schema::capture::Transform::Rot270, true),
];

#[test]
fn the_two_documents_this_build_prints_about_one_photograph_agree_about_its_shape() {
    // `photo` answers the **stored** raster and `photo diff` answers the **displayed** shape.
    // This is the arm that keeps the relationship between them from being prose: over every
    // member of `Transform::ALL`, on the sink where the two can differ, it holds in the
    // direction the tables say. The convention itself is stated in `imaging::compare`'s module
    // header and in note **N267**, and **not yet in the two schema descriptions an agent
    // reads** — `PhotoMeasurements.width` still says "Width in pixels, as decoded", which is
    // the one point N267 puts in front of the owner. This arm is what makes the answer
    // discoverable meanwhile: whichever way that is ruled, the two documents cannot drift
    // apart without it going red.
    assert_eq!(
        TRANSFORMS_THAT_EXCHANGE_THE_EXTENTS.len(),
        schema::capture::Transform::ALL.len(),
        "the committed table names {} transforms and this build has {}; a table that has \
         stopped covering the vocabulary is a reconciliation over a subset of it",
        TRANSFORMS_THAT_EXCHANGE_THE_EXTENTS.len(),
        schema::capture::Transform::ALL.len()
    );

    let class = &JPEG_SINK_CLASSES[0];
    let device = replayed(class.profile, class.camera);
    for (transform, exchanges) in TRANSFORMS_THAT_EXCHANGE_THE_EXTENTS {
        assert!(
            schema::capture::Transform::ALL.contains(&transform),
            "{transform:?} is not a member of this build's transform vocabulary"
        );
        let scratch = Scratch::new();
        let path = scratch.path("shot.jpg");
        let report = photo_report(
            &device,
            &["-o", path.as_str(), "--transform", transform.as_str()],
        );
        let measured = diff(&path, &path);

        let stored = (report.width, report.height);
        let displayed = (measured.a.width, measured.a.height);
        let expected = if exchanges {
            (stored.1, stored.0)
        } else {
            stored
        };
        assert_eq!(
            displayed,
            expected,
            "`photo` said {}x{} and `photo diff` said {}x{} about one {transform:?} \
             photograph, where the stored raster and the displayed shape are {}",
            stored.0,
            stored.1,
            displayed.0,
            displayed.1,
            if exchanges {
                "a quarter turn apart"
            } else {
                "the same shape"
            }
        );
    }
}

#[test]
fn wch_photo_to_a_png_sink_produces_a_decodable_png_at_the_negotiated_size() {
    let scratch = Scratch::new();
    let path = scratch.path("shot.png");
    let device = replayed("chicony-rgb", "cam:integrated-camera-integrated-c");
    let (stdout, _) = photo(&device, &["-o", path.as_str()]);

    let bytes = std::fs::read(&path).expect("the file exists");
    assert!(bytes.starts_with(&[0x89, b'P', b'N', b'G']));
    let decoded = image::load_from_memory(&bytes).expect("a real PNG decodes");
    assert!(
        stdout.contains(&format!("{}x{}", decoded.width(), decoded.height())),
        "the report's size must be the image's: {stdout}"
    );
}

#[test]
fn wch_photo_json_reports_the_negotiated_stream_and_the_rendering_it_chose() {
    let scratch = Scratch::new();
    let path = scratch.path("shot.jpg");
    let device = replayed("chicony-rgb", "cam:integrated-camera-integrated-c");
    let (stdout, _) = photo(&device, &["-o", path.as_str(), "--json"]);

    let report: schema::capture::PhotoReport =
        serde_json::from_str(&stdout).expect("--json emits the schema document and nothing else");
    assert_eq!(report.camera.as_str(), device.camera);
    assert!(report.rendering.is_verbatim(), "{:?}", report.rendering);
    assert_eq!(
        report.frames_settled,
        schema::limits::DEFAULT_SETTLE_SKIP_FRAMES
    );
    match &report.delivery {
        schema::capture::PhotoDelivery::Path {
            path: reported,
            byte_count,
        } => {
            assert_eq!(reported, &path);
            let on_disk = std::fs::metadata(&path).expect("stat").len();
            assert_eq!(on_disk, *byte_count, "the count is the file's, not a guess");
        }
        other => panic!("a -o photo must report a path, got {other:?}"),
    }
}

#[test]
fn wch_photo_without_a_path_writes_the_image_to_standard_output_and_the_table_to_stderr() {
    // `webcam-handler-cli photo cam:x > shot.jpg` has to be a photo, not a photo with a table
    // in it.
    let device = replayed("chicony-rgb", "cam:integrated-camera-integrated-c");
    let output = wch()
        .args([
            "--backend",
            "fake",
            "--profile",
            device.profile.as_str(),
            "photo",
            &device.camera,
        ])
        .output()
        .expect("webcam-handler-cli runs");
    assert!(output.status.success());

    assert!(
        output.stdout.starts_with(&[0xff, 0xd8]),
        "standard output must begin with the image, not with a table"
    );
    let summary = String::from_utf8_lossy(&output.stderr);
    assert!(
        summary.contains("rendering"),
        "the summary belongs on stderr: {summary}"
    );
    assert!(
        image::load_from_memory(&output.stdout).is_ok(),
        "the piped bytes are a decodable image"
    );
}

#[test]
fn a_photo_never_lands_where_the_repository_can_see_it() {
    // Rubric A12 as a habit rather than a hope: every test above writes under a scratch
    // directory, and this asserts the directory really is somewhere a frame cannot be
    // mistaken for part of this project. A frame may contain a person, and
    // `no-frame-bytes-in-repo.sh` sniffs every file its walk finds — this is the half that
    // keeps an *uncommitted* one from appearing to it either.
    //
    // **This test used to assert the path was outside the worktree, and the 2026-08-12
    // ruling made that false on purpose** (note N84): test scratch now lives under
    // `target/`. What replaces it is stronger rather than weaker, and it is deliberately
    // two claims in two places — the general law is asserted once, in
    // `schema::paths`'s own tests, where the choice is made ("the scratch root is inside
    // the build directory, and the `.gitignore` beside it names `/target/`"), and this
    // file asserts the local one: the frame this binary writes is under that root and
    // nowhere else. The old wording could only ever have said where the photo is *not*.
    let scratch = Scratch::new();
    let path = scratch.path("shot.jpg");
    let root = schema::paths::scratch_root()
        .expect("a scratch root")
        .canonicalize_utf8()
        .expect("the scratch root resolves");
    let resolved = scratch
        .dir
        .path()
        .canonicalize()
        .expect("the scratch directory resolves");
    let resolved = Utf8PathBuf::from_path_buf(resolved).expect("a utf-8 scratch directory");
    assert!(
        resolved.starts_with(&root),
        "{resolved} is not under {root}; a photo belongs in the one scratch root and nowhere else"
    );

    let device = replayed("chicony-rgb", "cam:integrated-camera-integrated-c");
    photo(&device, &["-o", path.as_str()]);
    assert!(
        path.exists(),
        "and the scratch path is a real, writable one"
    );
}
