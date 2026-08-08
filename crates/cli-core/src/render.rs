//! Rendering: the same values, twice.
//!
//! Every verb has a human rendering and a `--json` rendering, and they are two views of
//! one schema value. The rule that keeps them honest is that neither may compute anything
//! the other cannot: the "out of range" mark on a control row comes from
//! [`schema::report::ControlReport::self_contradicting`], not from a comparison written
//! here, so a reader of `--json` can reach the same conclusion.
//!
//! `--json` is the schema document and nothing else — no envelope, no timestamp, no tool
//! version. Adding one would make every consumer unwrap before validating, and the
//! committed bundle would stop describing what we actually emit.

use std::io::Write as _;

use camino::Utf8Path;
use comfy_table::{Cell, ContentArrangement, Table, presets};
use schema::camera::{CameraInfo, FrameInterval, FrameSize};
use schema::control::{ControlDesc, ControlType, ControlValue, KnownFlag};
use schema::error::{Error, Result};
use schema::profile::DeviceProfile;
use schema::report::{CameraDetail, CameraList, ControlReport};

/// Where rendered output goes.
///
/// Streams are separated so a table never lands in the middle of a `--json` document:
/// notes and hints go to standard error, the answer goes to standard output. A shell
/// pipeline redirecting stdout gets exactly the document and nothing else.
pub struct Output {
    stdout: Box<dyn std::io::Write>,
    stderr: Box<dyn std::io::Write>,
}

impl std::fmt::Debug for Output {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `Box<dyn Write>` is not `Debug`, and the two sinks have nothing to say about
        // themselves anyway. Written out rather than derived so the workspace's
        // `missing_debug_implementations` lint stays on.
        f.debug_struct("Output").finish_non_exhaustive()
    }
}

/// Which stream a line belongs on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stream {
    /// The answer.
    Stdout,
    /// Everything about the answer.
    Stderr,
}

impl Output {
    /// The process's own streams.
    #[must_use]
    pub fn process() -> Output {
        Output {
            stdout: Box::new(std::io::stdout()),
            stderr: Box::new(std::io::stderr()),
        }
    }

    /// Streams a test can read back.
    #[must_use]
    pub fn to_buffers(stdout: Box<dyn std::io::Write>, stderr: Box<dyn std::io::Write>) -> Output {
        Output { stdout, stderr }
    }

    /// Write a line.
    ///
    /// # Errors
    ///
    /// [`Error::StorageIo`] when the stream refuses — a closed pipe is a real outcome and
    /// `wch list | head -1` must not panic on it.
    pub fn line(&mut self, stream: Stream, text: &str) -> Result<()> {
        let sink = match stream {
            Stream::Stdout => &mut self.stdout,
            Stream::Stderr => &mut self.stderr,
        };
        writeln!(sink, "{text}").map_err(|error| Error::StorageIo {
            path: match stream {
                Stream::Stdout => "<stdout>".into(),
                Stream::Stderr => "<stderr>".into(),
            },
            errno: error.raw_os_error(),
            message: error.to_string(),
        })
    }
}

/// Serialize a schema value as the `--json` answer.
fn json<T: serde::Serialize>(value: &T, out: &mut Output) -> Result<()> {
    let text = serde_json::to_string_pretty(value).map_err(|error| Error::StorageIo {
        path: "<stdout>".into(),
        errno: None,
        message: format!("could not serialize the answer: {error}"),
    })?;
    out.line(Stream::Stdout, &text)
}

fn table() -> Table {
    let mut table = Table::new();
    table
        .load_style(presets::UTF8_HORIZONTAL_ONLY)
        .set_content_arrangement(ContentArrangement::Dynamic);
    table
}

/// `list`.
pub(crate) fn list(list: &CameraList, as_json: bool, out: &mut Output) -> Result<()> {
    if as_json {
        return json(list, out);
    }

    if list.cameras.is_empty() {
        out.line(Stream::Stdout, "no cameras")?;
    } else {
        let mut table = table();
        table.set_header(vec!["ID", "CARD", "CAPTURE NODE", "BUS PATH", "DRIVER"]);
        for camera in &list.cameras {
            table.add_row(vec![
                Cell::new(camera.id.as_str()),
                Cell::new(&camera.card),
                // A camera with no capture node is listed, and says so rather than
                // showing a blank a reader would take for a rendering bug.
                Cell::new(
                    camera
                        .capture_node()
                        .map_or("(none — metadata only)", |node| node.path.as_str()),
                ),
                Cell::new(&camera.fingerprint.bus_path),
                Cell::new(&camera.driver),
            ]);
        }
        out.line(Stream::Stdout, &table.to_string())?;
    }

    // D1's diagnosis. On stderr because it is about the answer rather than part of it.
    for hint in &list.hints {
        out.line(Stream::Stderr, &format!("note: {}", hint.message()))?;
    }
    Ok(())
}

/// `info`.
pub(crate) fn info(detail: &CameraDetail, as_json: bool, out: &mut Output) -> Result<()> {
    if as_json {
        return json(detail, out);
    }

    out.line(Stream::Stdout, &identity_table(&detail.info).to_string())?;

    if detail.formats.is_empty() {
        out.line(
            Stream::Stdout,
            "\nformats: none (this camera has no capture node)",
        )?;
        return Ok(());
    }

    let mut table = table();
    table.set_header(vec!["FORMAT", "DESCRIPTION", "SIZE", "FPS"]);
    for format in &detail.formats {
        for entry in &format.sizes {
            table.add_row(vec![
                Cell::new(format.pixel_format.to_string()),
                Cell::new(&format.description),
                Cell::new(size_text(&entry.size)),
                Cell::new(intervals_text(&entry.intervals)),
            ]);
        }
        // A format the driver offers with no sizes under it is a fact, not a blank row to
        // skip [PF:9]: the nesting is what makes "MJPG goes to 4K and YUYV does not"
        // visible, so an empty nest must be visible too.
        if format.sizes.is_empty() {
            table.add_row(vec![
                Cell::new(format.pixel_format.to_string()),
                Cell::new(&format.description),
                Cell::new("(no sizes enumerated)"),
                Cell::new(""),
            ]);
        }
    }
    out.line(Stream::Stdout, &table.to_string())
}

fn identity_table(info: &CameraInfo) -> Table {
    let mut table = table();
    table.set_header(vec!["FIELD", "VALUE"]);
    let usb = info
        .fingerprint
        .usb_id
        .map_or_else(|| "(not on USB)".to_owned(), |id| id.to_string());
    // PF:8: absence is the common case, and it is spelled out rather than left blank.
    let serial = info
        .fingerprint
        .serial
        .clone()
        .unwrap_or_else(|| "(none reported)".to_owned());
    for (field, value) in [
        ("id", info.id.as_str().to_owned()),
        ("card", info.card.clone()),
        ("driver", info.driver.clone()),
        ("bus_info", info.bus_info.clone()),
        ("bus_path", info.fingerprint.bus_path.clone()),
        ("usb_id", usb),
        ("serial", serial),
        ("backend", info.backend.as_str().to_owned()),
    ] {
        table.add_row(vec![Cell::new(field), Cell::new(value)]);
    }
    for node in &info.nodes {
        table.add_row(vec![
            Cell::new("node"),
            Cell::new(format!(
                "{} — {} (device_caps {:#010x})",
                node.path,
                node_kind_text(&node.kind),
                node.device_caps
            )),
        ]);
    }
    table
}

fn node_kind_text(kind: &schema::camera::NodeKind) -> String {
    use schema::camera::NodeKind;
    match kind {
        NodeKind::VideoCapture => "capture".to_owned(),
        NodeKind::MetaCapture => "metadata".to_owned(),
        NodeKind::VideoOutput => "output".to_owned(),
        // Represent, don't discard (D2, applied to nodes).
        NodeKind::Other { device_caps } => format!("unrecognized ({device_caps:#010x})"),
    }
}

fn size_text(size: &FrameSize) -> String {
    match *size {
        FrameSize::Discrete { width, height } => format!("{width}x{height}"),
        FrameSize::Stepwise {
            min_width,
            max_width,
            step_width,
            min_height,
            max_height,
            step_height,
        } => format!(
            "{min_width}-{max_width}/{step_width} x {min_height}-{max_height}/{step_height}"
        ),
        // The driver offered a size shaped in a way this build cannot read. Shown, with
        // the discriminant, because a row that vanished would make the list look complete.
        FrameSize::Unknown { raw } => format!("(unreadable shape {raw:#x})"),
    }
}

fn intervals_text(intervals: &[FrameInterval]) -> String {
    if intervals.is_empty() {
        return "(none)".to_owned();
    }
    intervals
        .iter()
        .map(|interval| match interval {
            FrameInterval::Discrete { .. } => interval
                .fps()
                .map_or_else(|| "?".to_owned(), |fps| format!("{}", round2(fps))),
            FrameInterval::Stepwise {
                min_numerator,
                min_denominator,
                max_numerator,
                max_denominator,
            } => format!("{min_numerator}/{min_denominator}-{max_numerator}/{max_denominator}"),
            FrameInterval::Unknown { raw } => format!("(unreadable shape {raw:#x})"),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Two decimal places, so 59.9401197… prints as 59.94 rather than as a wall of digits.
fn round2(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

/// `controls`.
pub(crate) fn controls(report: &ControlReport, as_json: bool, out: &mut Output) -> Result<()> {
    if as_json {
        return json(report, out);
    }

    let mut table = table();
    table.set_header(vec![
        "CONTROL", "TYPE", "RANGE", "STEP", "DEFAULT", "CURRENT", "FLAGS",
    ]);
    for desc in &report.controls {
        table.add_row(vec![
            Cell::new(desc.slug.as_str()),
            Cell::new(type_text(desc.control_type)),
            Cell::new(range_text(desc)),
            Cell::new(desc.range.step.to_string()),
            Cell::new(mark(desc.default.to_string(), desc.default_out_of_range())),
            Cell::new(mark(current_text(desc), desc.current_out_of_range())),
            Cell::new(flags_text(desc)),
        ]);
    }
    out.line(Stream::Stdout, &table.to_string())?;

    // Menus print under the table rather than inside a cell: the holes are the point
    // [PF:2], and a hole is only visible when the indices are.
    for desc in report.controls.iter().filter(|d| !d.menu.is_empty()) {
        let items = desc
            .menu
            .iter()
            .map(|(index, item)| match item.name() {
                Some(name) => format!("{index}={name}"),
                None => format!("{index}={item:?}"),
            })
            .collect::<Vec<_>>()
            .join(", ");
        out.line(
            Stream::Stdout,
            &format!("\n{} menu: {items}", desc.slug.as_str()),
        )?;
    }

    // The self-contradicting rows, named once more where a reader will not miss them.
    // Computed by the schema, so `--json` supports the same conclusion.
    let odd = report.self_contradicting();
    if !odd.is_empty() {
        let names = odd
            .iter()
            .map(|d| d.slug.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        out.line(
            Stream::Stderr,
            &format!(
                "note: {} control(s) report a default or current value outside their own \
                 declared range, marked `!` above: {names}. This is the device's answer, \
                 reported rather than corrected [PF:4, PF:5].",
                odd.len()
            ),
        )?;
    }
    Ok(())
}

/// Append the out-of-range mark. One spelling, used for both the default and the current.
fn mark(text: String, out_of_range: bool) -> String {
    if out_of_range {
        format!("{text} !")
    } else {
        text
    }
}

fn type_text(control_type: ControlType) -> String {
    match control_type {
        ControlType::Integer => "int".to_owned(),
        ControlType::Boolean => "bool".to_owned(),
        ControlType::Menu => "menu".to_owned(),
        ControlType::Button => "button".to_owned(),
        ControlType::Integer64 => "int64".to_owned(),
        ControlType::ControlClass => "class".to_owned(),
        ControlType::String => "string".to_owned(),
        ControlType::Bitmask => "bitmask".to_owned(),
        ControlType::IntegerMenu => "intmenu".to_owned(),
        ControlType::U8 => "u8[]".to_owned(),
        ControlType::U16 => "u16[]".to_owned(),
        ControlType::U32 => "u32[]".to_owned(),
        ControlType::Area => "area".to_owned(),
        ControlType::Rect => "rect".to_owned(),
        // PF:1's whole point, at the point a human reads it: a type this build cannot
        // interpret still has a row, and the row says what the kernel called it.
        ControlType::Unknown { raw } => format!("unknown({raw:#x})"),
    }
}

fn range_text(desc: &ControlDesc) -> String {
    if desc.control_type.is_scalar() || desc.range.min != desc.range.max {
        format!("{}..{}", desc.range.min, desc.range.max)
    } else {
        "—".to_owned()
    }
}

fn current_text(desc: &ControlDesc) -> String {
    match &desc.current {
        Some(ControlValue::Int(value)) => value.to_string(),
        Some(ControlValue::Text(text)) => text.clone(),
        // Never the bytes: a payload is opaque and printing it would be noise at best.
        Some(ControlValue::Bytes(bytes)) => format!("<{} bytes>", bytes.len()),
        None => "—".to_owned(),
    }
}

fn flags_text(desc: &ControlDesc) -> String {
    let mut parts: Vec<String> = desc
        .flags
        .known
        .iter()
        .map(|flag| flag_text(*flag).to_owned())
        .collect();
    // Next year's bit is data, not a surprise [PF:12] — so it prints too.
    if desc.flags.unknown_bits != 0 {
        parts.push(format!("+{:#x}", desc.flags.unknown_bits));
    }
    if parts.is_empty() {
        "—".to_owned()
    } else {
        parts.join(",")
    }
}

fn flag_text(flag: KnownFlag) -> &'static str {
    match flag {
        KnownFlag::Disabled => "disabled",
        KnownFlag::Grabbed => "grabbed",
        KnownFlag::ReadOnly => "ro",
        KnownFlag::Update => "update",
        KnownFlag::Inactive => "inactive",
        KnownFlag::Slider => "slider",
        KnownFlag::WriteOnly => "wo",
        KnownFlag::Volatile => "volatile",
        KnownFlag::HasPayload => "payload",
        KnownFlag::ExecuteOnWrite => "exec-on-write",
        KnownFlag::ModifyLayout => "modify-layout",
        KnownFlag::DynamicArray => "dyn-array",
        KnownFlag::HasWhichMinMax => "has-min-max",
    }
}

/// `profile capture`. Always JSON — a device profile is a document, not a view.
pub(crate) fn profile(
    profile: &DeviceProfile,
    destination: Option<&Utf8Path>,
    out: &mut Output,
) -> Result<()> {
    let text = serde_json::to_string_pretty(profile).map_err(|error| Error::StorageIo {
        path: destination.map_or_else(|| "<stdout>".into(), Utf8Path::to_path_buf),
        errno: None,
        message: format!("could not serialize the profile: {error}"),
    })?;

    match destination {
        None => out.line(Stream::Stdout, &text),
        Some(path) => {
            // A trailing newline: the committed corpus is diffed by humans and by
            // `git`, and a file without one is a permanent "\ No newline" in every diff.
            std::fs::write(path, format!("{text}\n")).map_err(|error| Error::StorageIo {
                path: path.to_path_buf(),
                errno: error.raw_os_error(),
                message: error.to_string(),
            })?;
            out.line(Stream::Stderr, &format!("wrote {path}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::{Arc, Mutex};

    use schema::backend::BackendKind;
    use schema::camera::{
        CameraFingerprint, CameraId, CameraInfo, DeviceNode, FormatInfo, FrameSizeInfo, NodeKind,
        PixelFormat, UsbId,
    };
    use schema::control::{
        ControlFlags, ControlId, ControlRange, ControlSlug, ControlValue, MenuItem,
    };
    use schema::report::{HintKind, ListHint};

    use super::*;

    /// A writer a test can read back.
    #[derive(Clone, Default)]
    struct Buffer(Arc<Mutex<Vec<u8>>>);

    impl Buffer {
        fn text(&self) -> String {
            String::from_utf8_lossy(&self.0.lock().expect("the buffer lock").clone()).into_owned()
        }
    }

    impl std::io::Write for Buffer {
        fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
            self.0
                .lock()
                .expect("the buffer lock")
                .extend_from_slice(data);
            Ok(data.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn captured() -> (Output, Buffer, Buffer) {
        let stdout = Buffer::default();
        let stderr = Buffer::default();
        let out = Output::to_buffers(Box::new(stdout.clone()), Box::new(stderr.clone()));
        (out, stdout, stderr)
    }

    fn camera(card: &str, with_capture_node: bool) -> CameraInfo {
        let mut nodes = vec![DeviceNode {
            path: "/dev/video1".into(),
            kind: NodeKind::MetaCapture,
            device_caps: 0x04a0_0000,
            capabilities: 0x84a0_0001,
        }];
        if with_capture_node {
            nodes.insert(
                0,
                DeviceNode {
                    path: "/dev/video0".into(),
                    kind: NodeKind::VideoCapture,
                    device_caps: 0x0420_0001,
                    capabilities: 0x84a0_0001,
                },
            );
        }
        CameraInfo {
            id: CameraId::parse("cam:test").expect("literal id"),
            fingerprint: CameraFingerprint {
                bus_path: "3-4:1.0".to_owned(),
                usb_id: Some(UsbId {
                    vendor: 0x04f2,
                    product: 0xb83c,
                }),
                card: card.to_owned(),
                driver: "uvcvideo".to_owned(),
                serial: None,
            },
            card: card.to_owned(),
            driver: "uvcvideo".to_owned(),
            bus_info: "usb-0000:00:14.0-4".to_owned(),
            nodes,
            backend: BackendKind::V4l2,
        }
    }

    #[test]
    fn an_empty_listing_says_so_and_puts_the_diagnosis_on_stderr() {
        // The `--json` document must stay parseable when a hint is present, which is why
        // hints are not printed to stdout.
        let (mut out, stdout, stderr) = captured();
        let list = CameraList {
            cameras: Vec::new(),
            hints: vec![ListHint {
                kind: HintKind::DriverlessUsbVideoDevice,
                subject: "1-2".to_owned(),
            }],
        };
        list_render(&list, false, &mut out);
        assert!(stdout.text().contains("no cameras"), "{}", stdout.text());
        assert!(stderr.text().contains("1-2"), "{}", stderr.text());
        assert!(
            !stdout.text().contains("1-2"),
            "the hint leaked into stdout"
        );
    }

    /// Named to avoid shadowing the module function inside the test module.
    fn list_render(value: &CameraList, as_json: bool, out: &mut Output) {
        list(value, as_json, out).expect("rendering into a buffer cannot fail");
    }

    #[test]
    fn json_output_is_the_schema_document_and_nothing_else() {
        let (mut out, stdout, stderr) = captured();
        let list = CameraList {
            cameras: vec![camera("Test", true)],
            hints: vec![ListHint {
                kind: HintKind::DriverlessUsbVideoDevice,
                subject: "1-2".to_owned(),
            }],
        };
        list_render(&list, true, &mut out);

        // Round-trips, and equals the value it was rendered from — no envelope, no
        // timestamp, nothing a consumer would have to unwrap before validating.
        let back: CameraList = serde_json::from_str(&stdout.text()).expect("valid JSON");
        assert_eq!(back, list);
        // Stderr stays empty in JSON mode for a listing that has hints: the hint is *in*
        // the document.
        assert!(stderr.text().is_empty(), "{}", stderr.text());
    }

    #[test]
    fn a_metadata_only_camera_says_it_has_no_capture_node_rather_than_showing_a_blank() {
        let (mut out, stdout, _) = captured();
        list_render(
            &CameraList {
                cameras: vec![camera("Meta Only", false)],
                hints: Vec::new(),
            },
            false,
            &mut out,
        );
        assert!(stdout.text().contains("metadata only"), "{}", stdout.text());
    }

    fn desc(name: &str, control_type: ControlType) -> ControlDesc {
        ControlDesc {
            id: ControlId(1),
            name: name.to_owned(),
            slug: ControlSlug::from_name(name).expect("a nameable control"),
            control_type,
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
            current: Some(ControlValue::Int(50)),
        }
    }

    #[test]
    fn a_control_type_this_build_cannot_interpret_still_gets_a_row_naming_it() {
        // PF:1 at the surface a human sees: `controls` must not quietly drop the RECT.
        let (mut out, stdout, _) = captured();
        let mut rect = desc("Region of Interest Rectangle", ControlType::Rect);
        rect.current = Some(ControlValue::Bytes(vec![0; 16]));
        rect.flags = ControlFlags::from_raw(0x1100);
        let mut future = desc("Something New", ControlType::Unknown { raw: 0x0999 });
        future.current = None;

        controls(
            &ControlReport {
                camera: CameraId::parse("cam:test").expect("literal id"),
                controls: vec![rect, future],
            },
            false,
            &mut out,
        )
        .expect("rendering into a buffer cannot fail");

        let text = stdout.text();
        assert!(text.contains("region_of_interest_rectangle"), "{text}");
        assert!(text.contains("rect"), "{text}");
        assert!(text.contains("<16 bytes>"), "{text}");
        assert!(text.contains("payload"), "{text}");
        assert!(text.contains("unknown(0x999)"), "{text}");
    }

    #[test]
    fn a_sparse_menu_prints_its_indices_so_the_holes_are_visible() {
        // PF:2. A menu rendered as a list of names would hide the fact that index 2 does
        // not exist, which is the one thing about this menu worth knowing.
        let (mut out, stdout, _) = captured();
        let mut auto_exposure = desc("Auto Exposure", ControlType::Menu);
        auto_exposure.menu = BTreeMap::from([
            (
                1,
                MenuItem::Name {
                    name: "Manual Mode".to_owned(),
                },
            ),
            (
                3,
                MenuItem::Name {
                    name: "Aperture Priority Mode".to_owned(),
                },
            ),
        ]);
        controls(
            &ControlReport {
                camera: CameraId::parse("cam:test").expect("literal id"),
                controls: vec![auto_exposure],
            },
            false,
            &mut out,
        )
        .expect("rendering into a buffer cannot fail");

        let text = stdout.text();
        assert!(text.contains("1=Manual Mode"), "{text}");
        assert!(text.contains("3=Aperture Priority Mode"), "{text}");
        assert!(!text.contains("2="), "index 2 does not exist: {text}");
    }

    #[test]
    fn a_value_outside_its_own_declared_range_is_marked_and_explained() {
        // PF:4 and PF:5, in the rendering. Both marks come from the schema's own
        // predicate, so `--json` supports the same reading.
        let (mut out, stdout, stderr) = captured();
        let mut zoom = desc("Zoom, Continuous", ControlType::Integer);
        zoom.range = ControlRange {
            min: -100,
            max: 100,
            step: 1,
        };
        zoom.current = Some(ControlValue::Int(245));
        let mut plf = desc("Power Line Frequency", ControlType::Menu);
        plf.range = ControlRange {
            min: 0,
            max: 2,
            step: 1,
        };
        plf.default = 3;
        plf.current = Some(ControlValue::Int(0));

        controls(
            &ControlReport {
                camera: CameraId::parse("cam:test").expect("literal id"),
                controls: vec![zoom, plf, desc("Brightness", ControlType::Integer)],
            },
            false,
            &mut out,
        )
        .expect("rendering into a buffer cannot fail");

        assert!(stdout.text().contains("245 !"), "{}", stdout.text());
        assert!(stdout.text().contains("3 !"), "{}", stdout.text());
        let note = stderr.text();
        assert!(note.contains("zoom_continuous"), "{note}");
        assert!(note.contains("power_line_frequency"), "{note}");
        assert!(
            !note.contains("brightness"),
            "the ordinary row was marked: {note}"
        );
        assert!(note.contains("2 control(s)"), "{note}");
    }

    #[test]
    fn frame_rates_print_rounded_and_sizes_nest_under_their_format() {
        // PF:9: the nesting is the finding. A flat size list would claim YUYV reaches 4K.
        let (mut out, stdout, _) = captured();
        let detail = CameraDetail {
            info: camera("Test", true),
            formats: vec![
                FormatInfo {
                    pixel_format: PixelFormat::MJPG,
                    description: "Motion-JPEG".to_owned(),
                    flags: 1,
                    sizes: vec![FrameSizeInfo {
                        size: FrameSize::Discrete {
                            width: 3840,
                            height: 2160,
                        },
                        intervals: vec![FrameInterval::Discrete {
                            numerator: 1001,
                            denominator: 30000,
                        }],
                    }],
                },
                FormatInfo {
                    pixel_format: PixelFormat::YUYV,
                    description: "YUYV 4:2:2".to_owned(),
                    flags: 0,
                    sizes: vec![FrameSizeInfo {
                        size: FrameSize::Discrete {
                            width: 640,
                            height: 480,
                        },
                        intervals: Vec::new(),
                    }],
                },
            ],
        };
        info(&detail, false, &mut out).expect("rendering into a buffer cannot fail");

        let text = stdout.text();
        assert!(text.contains("3840x2160"), "{text}");
        assert!(
            text.contains("29.97"),
            "the awkward NTSC rate rounds: {text}"
        );
        assert!(text.contains("640x480"), "{text}");
        assert!(
            text.contains("(none)"),
            "a size with no intervals says so: {text}"
        );
        // The identity block carries PF:13's two different bus fields, both of them.
        assert!(text.contains("usb-0000:00:14.0-4"), "{text}");
        assert!(text.contains("3-4:1.0"), "{text}");
        assert!(
            text.contains("(none reported)"),
            "PF:8's absent serial: {text}"
        );
    }

    #[test]
    fn a_size_this_build_cannot_read_still_gets_a_row_naming_its_discriminant() {
        // The inverse of dropping it: an omitted row would make the format list read as
        // complete, which is a capability claim invented out of our own ignorance.
        let (mut out, stdout, _) = captured();
        let detail = CameraDetail {
            info: camera("Test", true),
            formats: vec![FormatInfo {
                pixel_format: PixelFormat::MJPG,
                description: "Motion-JPEG".to_owned(),
                flags: 1,
                sizes: vec![FrameSizeInfo {
                    size: FrameSize::Unknown { raw: 0x63 },
                    intervals: vec![FrameInterval::Unknown { raw: 0x63 }],
                }],
            }],
        };
        info(&detail, false, &mut out).expect("rendering into a buffer cannot fail");
        let text = stdout.text();
        assert!(text.contains("unreadable shape 0x63"), "{text}");
        assert!(text.contains("MJPG"), "{text}");
    }

    #[test]
    fn a_written_profile_lands_on_disk_with_a_trailing_newline_and_says_where() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path =
            camino::Utf8PathBuf::from_path_buf(dir.path().join("p.json")).expect("utf-8 temp dir");
        let (mut out, stdout, stderr) = captured();
        let document = schema::profile::DeviceProfile {
            schema_version: schema::limits::PROFILE_SCHEMA_VERSION,
            provenance: schema::profile::ProfileProvenance {
                captured_at: schema::time::Stamp::epoch(),
                kernel: "test".to_owned(),
                tool_version: "0.1.0".to_owned(),
                capturer: "test".to_owned(),
                backend: BackendKind::V4l2,
            },
            invariant: schema::profile::ProfileInvariant {
                info: camera("Test", true),
                formats: Vec::new(),
                controls: Vec::new(),
                measured_pairs: Vec::new(),
            },
            state: schema::profile::ProfileState::default(),
        };

        profile(&document, Some(&path), &mut out).expect("writing to a temp dir");
        let written = std::fs::read_to_string(&path).expect("the file exists");
        assert!(written.ends_with("}\n"), "no trailing newline");
        assert_eq!(
            serde_json::from_str::<schema::profile::DeviceProfile>(&written).expect("valid"),
            document
        );
        // The path goes to stderr so `wch profile capture -o f.json` prints nothing a
        // pipeline would have to strip.
        assert!(stdout.text().is_empty(), "{}", stdout.text());
        assert!(stderr.text().contains(path.as_str()), "{}", stderr.text());
    }

    #[test]
    fn every_known_flag_and_control_type_has_a_distinct_rendering() {
        // Both vocabularies are generated `ALL`s, so a new variant lands here without
        // anyone remembering to extend a list — and two variants sharing a spelling
        // would make one of them unreadable in the table.
        let mut seen = std::collections::BTreeSet::new();
        for &flag in KnownFlag::ALL {
            assert!(
                seen.insert(flag_text(flag)),
                "{flag:?} duplicates a spelling"
            );
        }
        // `ControlType` carries a payload, so it has no generated `ALL`. The population
        // is derived from the *decoder* instead: every discriminant `from_raw` names is a
        // type this table must spell, and one it does not name is `Unknown`, whose
        // rendering carries the raw value and is therefore distinct by construction. A
        // hand-written list here would silently stop covering a variant added to
        // `from_raw` — which is the drift docs/4's derived-population rule exists to stop.
        let named: Vec<ControlType> = (0..=0x0110u32)
            .map(ControlType::from_raw)
            .filter(|t| !matches!(t, ControlType::Unknown { .. }))
            .collect();
        assert!(
            named.len() >= 14,
            "the decoder names only {} types; this walk has stopped covering them",
            named.len()
        );
        let mut seen = std::collections::BTreeSet::new();
        for control_type in named {
            assert!(
                seen.insert(type_text(control_type)),
                "{control_type:?} duplicates a spelling"
            );
        }
        // And the open-ended arm keeps its payload visible, so two unknown types never
        // render the same way.
        assert_ne!(
            type_text(ControlType::Unknown { raw: 0x900 }),
            type_text(ControlType::Unknown { raw: 0x901 })
        );
    }
}
