//! `wch photo` end to end, against the fake backend (docs/2 G2).
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
//! reads, which is docs/4's answer to write-only EXIF: a writer that returns `Ok` having
//! produced a segment no reader accepts passes every test that trusts it.
//!
//! Every photo lands in a scratch directory. A frame may contain a person (rubric A12) —
//! these are synthetic, and the habit is the point.

use std::process::Command;

use camino::{Utf8Path, Utf8PathBuf};
use exif::{In, Tag};

/// The `wch` binary this test drives, built by cargo alongside it.
fn wch() -> Command {
    Command::new(env!("CARGO_BIN_EXE_wch"))
}

/// A scratch directory outside the repository, and the paths under it.
struct Scratch {
    dir: tempfile::TempDir,
}

impl Scratch {
    fn new() -> Scratch {
        Scratch {
            dir: tempfile::tempdir().expect("a scratch directory"),
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

/// Run `wch photo` against a replayed camera, returning stdout and stderr.
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
        .expect("wch runs");
    assert!(
        output.status.success(),
        "wch photo {extra:?} failed: {}",
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
    // `wch photo cam:x > shot.jpg` has to be a photo, not a photo with a table in it.
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
        .expect("wch runs");
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
fn a_photo_never_lands_in_the_repository() {
    // Rubric A12 as a habit rather than a hope: every test above writes under a scratch
    // directory, and this asserts the directory really is outside the tree. A frame may
    // contain a person, and `no-frame-bytes-in-repo.sh` scans what is committed — this is
    // the half that keeps an *uncommitted* one from appearing either.
    let scratch = Scratch::new();
    let path = scratch.path("shot.jpg");
    let repo = Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let repo = repo
        .canonicalize_utf8()
        .expect("the repository root resolves");
    assert!(
        !path.starts_with(&repo),
        "{path} is inside {repo}; photos must not be written into the tree"
    );

    let device = replayed("chicony-rgb", "cam:integrated-camera-integrated-c");
    photo(&device, &["-o", path.as_str()]);
    assert!(
        path.exists(),
        "and the scratch path is a real, writable one"
    );
}
