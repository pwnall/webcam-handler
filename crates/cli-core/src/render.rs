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
use schema::capture::{
    Adjustment, NegotiatedStream, PhotoDelivery, PhotoRendering, PhotoReport, TransformApplication,
};
use schema::control::{
    ControlDesc, ControlType, ControlValue, KnownFlag, Unverifiable, WriteWarning,
};
use schema::error::{Error, Result};
use schema::pairing::{AutomationOff, Provenance};
use schema::profile::DeviceProfile;
use schema::progress::{CalibrationProgress, ProgressEvent};
use schema::report::{CameraDetail, CameraList, ControlReport, WriteReport};
use schema::session::{
    BlockedReason, ControlStatus, Session, SessionEvent, SessionList, SessionStatus,
};
use schema::snapshot::{RestoreOutcome, RestoreReport, Snapshot, UnrestorableReason};

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

    /// Write raw bytes, with no newline and no interpretation.
    ///
    /// `wch photo cam:x > shot.jpg` is the reason this exists: with no `-o`, the photo's
    /// bytes *are* the answer, and a `line` would append a byte the image does not have.
    ///
    /// # Errors
    ///
    /// As [`Output::line`].
    pub fn bytes(&mut self, stream: Stream, data: &[u8]) -> Result<()> {
        let sink = match stream {
            Stream::Stdout => &mut self.stdout,
            Stream::Stderr => &mut self.stderr,
        };
        sink.write_all(data)
            .and_then(|()| sink.flush())
            .map_err(|error| Error::StorageIo {
                path: match stream {
                    Stream::Stdout => "<stdout>".into(),
                    Stream::Stderr => "<stderr>".into(),
                },
                errno: error.raw_os_error(),
                message: error.to_string(),
            })
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

    // The pairs. `--json` has carried them since P2 and the table did not, which made
    // `controls --discover-pairs` a verb that writes to the camera and shows a human
    // nothing it learned. The two renderings are two views of one value (this module's
    // first rule), and one of them was missing a field.
    if !report.pairs.is_empty() {
        let mut pairs = crate::render::table();
        pairs.set_header(vec![
            "MANUAL",
            "GOVERNED BY",
            "SWITCHED OFF WITH",
            "EVIDENCE",
        ]);
        for pair in &report.pairs {
            pairs.add_row(vec![
                Cell::new(pair.manual.as_str()),
                Cell::new(pair.automation.as_str()),
                Cell::new(automation_off_text(&pair.off)),
                // The provenance is the point, not decoration: a nomination from the
                // declared table and an observation from this device are different
                // claims, and measured beats declared (E1).
                Cell::new(match pair.provenance {
                    Provenance::Declared => "declared (from the UVC table)",
                    Provenance::Measured => "measured on this device",
                }),
            ]);
        }
        out.line(Stream::Stdout, "\nauto/manual pairs:")?;
        out.line(Stream::Stdout, &pairs.to_string())?;
    }
    Ok(())
}

/// How an automation control is switched off, in a phrase.
fn automation_off_text(off: &AutomationOff) -> String {
    match off {
        AutomationOff::Value { value } => format!("set to {value}"),
        // By name, never by index: menu indices are per-device \[PF:2\], and printing an
        // index would invite a reader to type it at a camera that numbers differently.
        AutomationOff::MenuItemNamed { patterns } => {
            format!("the menu item matching {:?}", patterns.join(" or "))
        }
    }
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

/// `get` — one control, in full.
pub(crate) fn control(desc: &ControlDesc, as_json: bool, out: &mut Output) -> Result<()> {
    if as_json {
        return json(desc, out);
    }

    let mut table = table();
    table.set_header(vec!["FIELD", "VALUE"]);
    for (field, value) in [
        ("control", desc.slug.to_string()),
        ("name", desc.name.clone()),
        ("id", desc.id.to_string()),
        ("type", type_text(desc.control_type)),
        ("current", current_text(desc)),
        ("default", desc.default.to_string()),
        ("range", range_text(desc)),
        ("flags", flags_text(desc)),
    ] {
        table.add_row(vec![Cell::new(field), Cell::new(value)]);
    }
    out.line(Stream::Stdout, &table.to_string())?;

    // The marks the table cannot carry, on stderr so a piped table stays a table. Read
    // from the schema's own predicates, so `--json` supports the same conclusion.
    if desc.current_out_of_range() {
        out.line(
            Stream::Stderr,
            &format!(
                "note: {}'s current value is outside its declared range [PF:4] — reported, \
                 not corrected",
                desc.slug
            ),
        )?;
    }
    if desc.default_out_of_range() {
        out.line(
            Stream::Stderr,
            &format!(
                "note: {}'s default is outside its declared range [PF:5]",
                desc.slug
            ),
        )?;
    }
    if desc.is_inactive() {
        out.line(
            Stream::Stderr,
            &format!(
                "note: {} is INACTIVE — an automation control owns it right now [PF:3]",
                desc.slug
            ),
        )?;
    }
    Ok(())
}

/// `set` — what was asked, what the device took, and why they differ.
pub(crate) fn writes(report: &WriteReport, as_json: bool, out: &mut Output) -> Result<()> {
    if as_json {
        return json(report, out);
    }

    let mut table = table();
    table.set_header(vec!["CONTROL", "REQUESTED", "APPLIED", "WHY"]);
    for applied in &report.writes {
        table.add_row(vec![
            Cell::new(applied.slug.as_str()),
            Cell::new(applied.requested.to_string()),
            Cell::new(applied.applied.to_string()),
            Cell::new(if applied.warnings.is_empty() {
                "—".to_owned()
            } else {
                applied
                    .warnings
                    .iter()
                    .map(warning_text)
                    .collect::<Vec<_>>()
                    .join("; ")
            }),
        ]);
    }
    out.line(Stream::Stdout, &table.to_string())?;

    if !report.disabled_automation.is_empty() {
        // A guarded write changes more than the caller named, and that is a change to the
        // camera they are entitled to hear about at the moment it happens.
        out.line(
            Stream::Stderr,
            &format!(
                "note: switched off to make the write stick: {}",
                report
                    .disabled_automation
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        )?;
    }
    if !report.is_exact() {
        out.line(
            Stream::Stderr,
            &format!(
                "note: {} write(s) did not land exactly as asked",
                report.inexact().len()
            ),
        )?;
    }
    Ok(())
}

/// `snapshot`. Always JSON — a snapshot is a document `restore` reads back, not a view.
pub(crate) fn snapshot(
    snapshot: &Snapshot,
    destination: Option<&Utf8Path>,
    out: &mut Output,
) -> Result<()> {
    let text = serde_json::to_string_pretty(snapshot).map_err(|error| Error::StorageIo {
        path: destination.map_or_else(|| "<stdout>".into(), Utf8Path::to_path_buf),
        errno: None,
        message: format!("could not serialize the snapshot: {error}"),
    })?;

    match destination {
        None => out.line(Stream::Stdout, &text),
        Some(path) => {
            std::fs::write(path, format!("{text}\n")).map_err(|error| Error::StorageIo {
                path: path.to_path_buf(),
                errno: error.raw_os_error(),
                message: error.to_string(),
            })?;
            out.line(
                Stream::Stderr,
                &format!("wrote {path} ({} control(s))", snapshot.entries.len()),
            )
        }
    }
}

/// `restore` — what went back, and what did not.
pub(crate) fn restore(report: &RestoreReport, as_json: bool, out: &mut Output) -> Result<()> {
    if as_json {
        return json(report, out);
    }

    let mut table = table();
    table.set_header(vec!["CONTROL", "OUTCOME"]);
    for outcome in &report.outcomes {
        let (control, text) = outcome_text(outcome);
        table.add_row(vec![Cell::new(control), Cell::new(text)]);
    }
    out.line(Stream::Stdout, &table.to_string())?;

    // The one line that matters, and it is on stderr because a caller scripting a restore
    // wants the exit code and the table, not prose in the middle of them.
    if !report.is_complete() {
        out.line(
            Stream::Stderr,
            &format!(
                "note: {} control(s) did not come back: {}",
                report.unrestored().len(),
                report
                    .unrestored()
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        )?;
    }
    Ok(())
}

/// `photo` — where the bytes went, and what was done to them.
///
/// With no `-o`, the bytes go to standard output and the summary to standard error, so
/// `wch photo cam:x > shot.jpg` is a photo and not a photo with a table in it. `--json`
/// requires `-o` for exactly that reason, and clap enforces it.
pub(crate) fn photo(
    report: &PhotoReport,
    returned: Option<&[u8]>,
    as_json: bool,
    out: &mut Output,
) -> Result<()> {
    if as_json {
        // `--json` requires `-o`, so there are no bytes to place and the document is the
        // whole of standard output. clap enforces that rather than this function, because
        // "you cannot have both" is a usage rule and belongs where usage rules are.
        return json(report, out);
    }

    // The bytes first and the table after, both because a reader piping stdout wants the
    // image at byte zero and because the summary describes what was just written.
    if let Some(bytes) = returned {
        out.bytes(Stream::Stdout, bytes)?;
    }
    let summary = if returned.is_some() {
        Stream::Stderr
    } else {
        Stream::Stdout
    };

    let mut table = table();
    table.set_header(vec!["FIELD", "VALUE"]);
    for (field, value) in [
        ("camera", report.camera.to_string()),
        ("taken at", report.taken_at.to_string()),
        ("size", format!("{}x{}", report.width, report.height)),
        ("stream", stream_text(&report.negotiated)),
        ("rendering", rendering_text(report.rendering)),
        ("transform", transform_text(report.transform)),
        ("frames settled", report.frames_settled.to_string()),
        ("delivery", delivery_text(&report.delivery)),
    ] {
        table.add_row(vec![Cell::new(field), Cell::new(value)]);
    }
    out.line(summary, &table.to_string())?;

    if !report.negotiated.is_exact() {
        out.line(
            Stream::Stderr,
            &format!(
                "note: the device adjusted the request: {}",
                report
                    .negotiated
                    .adjustments
                    .iter()
                    .map(adjustment_text)
                    .collect::<Vec<_>>()
                    .join("; ")
            ),
        )?;
    }
    Ok(())
}

/// One write warning, in a phrase.
fn warning_text(warning: &WriteWarning) -> String {
    match warning {
        WriteWarning::Clamped {
            requested,
            applied,
            range,
        } => format!(
            "clamped {requested} into [{}..={}] as {applied} [PF:6]",
            range.min, range.max
        ),
        WriteWarning::StepAligned {
            requested,
            applied,
            step,
        } => format!("aligned {requested} to step {step} as {applied}"),
        WriteWarning::Adjusted { requested, applied } => {
            format!("the device took {applied} for {requested}, for a reason it did not give")
        }
        WriteWarning::Unverified { because } => match because {
            Unverifiable::TypeHasNoValue => {
                "written; this control has no value to read back".to_owned()
            }
            Unverifiable::WriteOnly => {
                "written; the device flags this control write-only".to_owned()
            }
            Unverifiable::DeviceDeclinedToRead => {
                "written; the device then declined to read it back".to_owned()
            }
        },
    }
}

/// One restore outcome, as `(control, what happened)`.
fn outcome_text(outcome: &RestoreOutcome) -> (String, String) {
    match outcome {
        RestoreOutcome::Restored { applied } => (
            applied.slug.to_string(),
            if applied.is_exact() {
                format!("restored to {}", applied.applied)
            } else {
                format!(
                    "written {} and took {} — not where it was",
                    applied.requested, applied.applied
                )
            },
        ),
        RestoreOutcome::AlreadyCorrect { control } => {
            (control.to_string(), "already correct".to_owned())
        }
        RestoreOutcome::OwnedByAutomation {
            control,
            automation,
        } => (
            control.to_string(),
            match automation {
                Some(a) => format!("back under {a}, as it was [PF:3]"),
                None => "back under automation, as it was [PF:3]".to_owned(),
            },
        ),
        RestoreOutcome::Unrestorable { control, reason } => (
            control.to_string(),
            match reason {
                UnrestorableReason::StillInactive {
                    automation: Some(a),
                } => {
                    format!("still owned by {a} [PF:3]")
                }
                UnrestorableReason::StillInactive { automation: None } => {
                    "still INACTIVE, and no pair names what owns it [PF:3]".to_owned()
                }
                UnrestorableReason::Volatile => {
                    "volatile — the value is the device's to choose".to_owned()
                }
                UnrestorableReason::NoLongerWritable => "no longer writable".to_owned(),
                UnrestorableReason::WriteFailed { error } => format!("the write failed: {error}"),
            },
        ),
    }
}

fn transform_text(application: TransformApplication) -> String {
    match application {
        TransformApplication::Identity => "none".to_owned(),
        TransformApplication::Pixels => "applied to the pixels".to_owned(),
        TransformApplication::ExifOrientation { orientation } => {
            format!("EXIF Orientation {orientation}; the bitstream is untouched [E6]")
        }
    }
}

fn rendering_text(rendering: PhotoRendering) -> String {
    match rendering {
        PhotoRendering::Verbatim { source } => {
            format!("the camera's own {source} bytes, unmodified [E6]")
        }
        PhotoRendering::DecodedAndEncoded { source, target } => {
            format!("{source} decoded and re-encoded as {target}")
        }
        PhotoRendering::ConvertedAndEncoded { source, target } => {
            format!("{source} converted and encoded as {target}")
        }
    }
}

fn delivery_text(delivery: &PhotoDelivery) -> String {
    match delivery {
        PhotoDelivery::Bytes { format, byte_count } => {
            format!("{byte_count} bytes of {format} returned")
        }
        PhotoDelivery::Path { path, byte_count } => format!("{path} ({byte_count} bytes)"),
    }
}

fn stream_text(negotiated: &NegotiatedStream) -> String {
    let rate = negotiated.interval.fps().map_or_else(
        || "no rate reported".to_owned(),
        |fps| format!("{fps:.0} fps"),
    );
    format!(
        "{} {}x{} @ {rate}",
        negotiated.pixel_format, negotiated.width, negotiated.height
    )
}

fn adjustment_text(adjustment: &Adjustment) -> String {
    match adjustment {
        Adjustment::PixelFormat {
            requested,
            negotiated,
        } => format!("asked {requested}, got {negotiated}"),
        Adjustment::Size {
            requested_width,
            requested_height,
            negotiated_width,
            negotiated_height,
        } => format!(
            "asked {requested_width}x{requested_height}, got {negotiated_width}x{negotiated_height}"
        ),
        Adjustment::Interval {
            requested,
            negotiated,
        } => format!(
            "asked {}, got {}",
            requested.fps().unwrap_or(0.0),
            negotiated.fps().unwrap_or(0.0)
        ),
    }
}

// ------------------------------------------------------------------ calibration

/// `calibrate start`, `plan`, `sweep` and `select` — the session, as it stands.
pub(crate) fn session(session: &Session, as_json: bool, out: &mut Output) -> Result<()> {
    if as_json {
        return json(session, out);
    }

    let mut header = table();
    header.set_header(vec!["FIELD", "VALUE"]);
    for (field, value) in [
        ("session", session.id.to_string()),
        ("camera", session.fingerprint.slug()),
        ("task", session.task.clone()),
        ("goal", session.goal.clone()),
        ("updated", session.updated_at.to_string()),
    ] {
        header.add_row(vec![Cell::new(field), Cell::new(value)]);
    }
    for (nth, criterion) in session.criteria.iter().enumerate() {
        header.add_row(vec![
            Cell::new(format!("criterion {}", nth.saturating_add(1))),
            Cell::new(criterion),
        ]);
    }
    out.line(Stream::Stdout, &header.to_string())?;

    if session.controls.is_empty() {
        return out.line(
            Stream::Stdout,
            "\nno controls queued yet — `calibrate plan` drafts them",
        );
    }

    let mut controls = table();
    controls.set_header(vec![
        "CONTROL",
        "STATE",
        "VALUE",
        "PRECISION",
        "SCORE",
        "CHOSEN BY",
        "SAMPLES",
    ]);
    // Queue order first — the operator's chosen order is the one thing a table can show
    // that a map cannot — then anything calibrated outside the queue, in slug order.
    let queued = session.queue.iter();
    let unqueued = session
        .controls
        .keys()
        .filter(|slug| !session.queue.contains(slug));
    for slug in queued.chain(unqueued) {
        let Some(entry) = session.controls.get(slug) else {
            // A queued control nothing has happened to yet has no entry, and D8 says the
            // absence *is* `Untouched`. Shown, because a queue whose rows vanished would
            // read as a queue nobody drafted.
            controls.add_row(vec![
                Cell::new(slug.as_str()),
                Cell::new("untouched"),
                Cell::new("—"),
                Cell::new("—"),
                Cell::new("—"),
                Cell::new("—"),
                Cell::new("0"),
            ]);
            continue;
        };
        let StatusCells {
            state,
            value,
            precision,
            score,
            chosen_by,
        } = status_cells(&entry.status);
        controls.add_row(vec![
            Cell::new(slug.as_str()),
            Cell::new(state),
            Cell::new(value),
            Cell::new(precision),
            Cell::new(score),
            Cell::new(chosen_by),
            Cell::new(entry.samples.len().to_string()),
        ]);
    }
    out.line(Stream::Stdout, &controls.to_string())?;

    // The one sentence a caller needs before `apply`: whether anything is still pending.
    // Read from the schema's own predicate, so `--json` supports the same conclusion.
    if !session.is_settled() {
        out.line(
            Stream::Stderr,
            "note: this session still has queued work; `calibrate apply` needs --partial \
             until it settles",
        )?;
    }
    Ok(())
}

/// One control's status, as the five cells a row shows for it.
struct StatusCells {
    state: String,
    value: String,
    precision: String,
    score: String,
    chosen_by: String,
}

/// An exhaustive match, so a seventh D8 status cannot acquire a rendering by accident.
fn status_cells(status: &ControlStatus) -> StatusCells {
    let dash = || "—".to_owned();
    match status {
        ControlStatus::Untouched => StatusCells {
            state: "untouched".to_owned(),
            value: dash(),
            precision: dash(),
            score: dash(),
            chosen_by: dash(),
        },
        ControlStatus::AutoDisabled {
            automation,
            parked_value,
        } => StatusCells {
            state: format!(
                "auto-disabled ({})",
                automation
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            value: parked_value.map_or_else(dash, |v| format!("{v} (parked)")),
            precision: dash(),
            score: dash(),
            chosen_by: dash(),
        },
        ControlStatus::Sweeping { done, total, .. } => StatusCells {
            state: format!("sweeping {done}/{total}"),
            value: dash(),
            precision: dash(),
            score: dash(),
            chosen_by: dash(),
        },
        ControlStatus::Calibrated {
            value,
            precision,
            score,
            selector,
        } => StatusCells {
            state: "calibrated".to_owned(),
            value: value.to_string(),
            // Zero means "no spacing to report" — one sample — rather than a spacing of
            // nothing, and a refinement pass dividing it down must not read it as a
            // divisor (`session::sampled_precision`).
            precision: if *precision == 0 {
                "(single sample)".to_owned()
            } else {
                precision.to_string()
            },
            score: score.map_or_else(dash, |s| format!("{s:.4}")),
            // D8's own spelling, from the schema, so the table and the session file agree.
            chosen_by: selector.label(),
        },
        ControlStatus::Deferred { reason } => StatusCells {
            state: format!("deferred: {reason}"),
            value: dash(),
            precision: dash(),
            score: dash(),
            chosen_by: dash(),
        },
        ControlStatus::Blocked { reason } => StatusCells {
            state: format!("blocked: {}", blocked_text(reason)),
            value: dash(),
            precision: dash(),
            score: dash(),
            chosen_by: dash(),
        },
    }
}

/// Why the device will not let a control be calibrated, in a phrase.
fn blocked_text(reason: &BlockedReason) -> String {
    match reason {
        BlockedReason::ReadOnly => "the device flags it read-only [PF:12]".to_owned(),
        BlockedReason::Disabled => "the device flags it disabled".to_owned(),
        BlockedReason::InactiveWithoutPartner => {
            "INACTIVE, and no pair names what owns it [PF:3]".to_owned()
        }
        BlockedReason::NotSweepable { control_type } => {
            format!("a {control_type} control has no ordered range to sweep")
        }
        BlockedReason::Other { detail } => detail.clone(),
    }
}

/// `calibrate status` — the session, plus the history that explains it.
pub(crate) fn status(status: &SessionStatus, as_json: bool, out: &mut Output) -> Result<()> {
    if as_json {
        return json(status, out);
    }
    session(&status.session, false, out)?;

    if let Some(last) = status.log.last() {
        out.line(
            Stream::Stdout,
            &format!(
                "\nhistory: {} event(s), last at {}",
                status.log.len(),
                last.at
            ),
        )?;
    }
    // Every sweep that stopped, in full, on stderr. This is the question `status` is asked
    // after the terminal that showed the live progress is gone, and the samples on the
    // document say *where* a sweep stopped without ever saying why.
    for entry in &status.log {
        if let SessionEvent::SweepInterrupted {
            control,
            taken,
            total,
            failure,
            detail,
        } = &entry.event
        {
            out.line(
                Stream::Stderr,
                &format!(
                    "note: the sweep of {control} stopped after {taken} of {total} sample(s) \
                     at {}: {detail} ({failure:?})",
                    entry.at
                ),
            )?;
        }
    }
    Ok(())
}

/// `calibrate list`.
pub(crate) fn sessions(list: &SessionList, as_json: bool, out: &mut Output) -> Result<()> {
    if as_json {
        return json(list, out);
    }
    if list.sessions.is_empty() {
        return out.line(Stream::Stdout, "no sessions");
    }

    let mut table = table();
    table.set_header(vec!["SESSION", "CAMERA", "TASK", "PATH"]);
    for found in &list.sessions {
        table.add_row(vec![
            Cell::new(found.id.to_string()),
            Cell::new(&found.camera),
            Cell::new(&found.task_slug),
            Cell::new(found.path.as_str()),
        ]);
    }
    out.line(Stream::Stdout, &table.to_string())
}

// ------------------------------------------------------------------ sweep progress

/// Where a running sweep's events go while the CLI renders them.
///
/// This crate's seam, not the engine's, and the wall is why: `wchc` links no engine (T6),
/// so the shared command surface cannot name `engine::progress::ProgressSink`. Each binary
/// bridges the stream it has — an in-process sink for `wch`, a subscription for `wchc` at
/// P4e — onto this one object, and the rendering happens once, here. The events themselves
/// are schema DTOs on both sides, so nothing is translated in between.
pub trait SweepWatcher: Send + Sync + std::fmt::Debug {
    /// One event, as it happens.
    fn event(&self, event: &ProgressEvent);

    /// Take the display down, before anything else writes to the terminal.
    fn finish(&self);
}

/// The watcher a sweep should use for this invocation.
///
/// `--json` gets [`Quiet`]: the document goes to standard output and the bar to standard
/// error, so they cannot collide — but a caller redirecting both into one file would find
/// a document with a progress bar in it, and the answer is not to draw one. Otherwise the
/// bar, which indicatif itself hides when standard error is not a terminal.
#[must_use]
pub(crate) fn watcher(as_json: bool) -> Box<dyn SweepWatcher> {
    if as_json {
        Box::new(Quiet)
    } else {
        Box::new(Bar::new())
    }
}

/// Nobody is watching.
#[derive(Debug, Clone, Copy, Default)]
pub struct Quiet;

impl SweepWatcher for Quiet {
    fn event(&self, _event: &ProgressEvent) {}
    fn finish(&self) {}
}

/// An indicatif bar over the sweep's own `index`/`total`.
///
/// The counts come from the events rather than from anything this side counted, which is
/// what P4e's mid-sweep subscriber needs (`schema::progress`'s own reasoning): a bar that
/// tracked its own position would start at zero for a client that connected halfway.
#[derive(Debug)]
pub struct Bar {
    bar: indicatif::ProgressBar,
}

/// The bar's layout. `{msg}` is [`progress_line`]'s answer, and it comes first because it
/// is the part that says what the sweep is *doing* — a bar with no message is a sweep of
/// something, somewhere, at some value.
///
/// A constant with a test behind it ([`the_progress_bar_template_parses`]): the fallback
/// below is a real fallback, and a template that had quietly stopped parsing would drop
/// every line this module renders without failing anything.
const BAR_TEMPLATE: &str = "{msg}\n{bar:24} {pos}/{len}";

impl Bar {
    /// A bar drawing to standard error.
    #[must_use]
    pub fn new() -> Bar {
        // Hidden until a sweep starts and says how big it is: a bar of unknown length that
        // appears before the first event is a spinner that promises nothing.
        let bar = indicatif::ProgressBar::hidden();
        bar.set_style(
            indicatif::ProgressStyle::with_template(BAR_TEMPLATE)
                // A malformed template is a typo in the line above, not a reason to end a
                // calibration: the default bar still counts, and the test named there is
                // what keeps this arm from being taken silently.
                .unwrap_or_else(|_| indicatif::ProgressStyle::default_bar()),
        );
        Bar { bar }
    }
}

impl Default for Bar {
    fn default() -> Bar {
        Bar::new()
    }
}

impl SweepWatcher for Bar {
    fn event(&self, event: &ProgressEvent) {
        match &event.progress {
            CalibrationProgress::SweepStarted { total, .. } => {
                self.bar
                    .set_draw_target(indicatif::ProgressDrawTarget::stderr());
                self.bar.set_length(u64::from(*total));
                self.bar.set_position(0);
            }
            CalibrationProgress::ValueSet { index, .. } => {
                self.bar.set_position(u64::from(index.saturating_sub(1)));
            }
            CalibrationProgress::SampleTaken { index, .. } => {
                self.bar.set_position(u64::from(*index));
            }
            CalibrationProgress::SweepFinished { .. }
            | CalibrationProgress::SweepInterrupted { .. } => {}
        }
        self.bar.set_message(progress_line(&event.progress));
        if event.progress.is_terminal() {
            // Printed rather than left on the bar: the last thing a sweep said is the
            // thing worth keeping on screen, and `finish` is about to clear the bar.
            self.bar.println(progress_line(&event.progress));
        }
    }

    fn finish(&self) {
        self.bar.finish_and_clear();
    }
}

/// The line one progress event puts in front of a human.
///
/// Separate from the bar because a terminal is not available in a test and a rendering
/// nobody can assert is a rendering nobody checked. Every variant is spelled here, and the
/// walk below proves no two share a spelling.
fn progress_line(progress: &CalibrationProgress) -> String {
    match progress {
        CalibrationProgress::SweepStarted { control, total, .. } => {
            format!("sweeping {control}: {total} sample(s)")
        }
        CalibrationProgress::ValueSet {
            control,
            index,
            total,
            requested,
            applied,
            warnings,
        } => {
            let took = if requested == applied {
                format!("{applied}")
            } else {
                // PF:6 the moment it happens, rather than when the session file is read.
                format!("{requested} → {applied}")
            };
            let mut line = format!("{control} {index}/{total}: set {took}, settling");
            if !warnings.is_empty() {
                line.push_str(&format!(" ({})", warnings.len()));
            }
            line
        }
        CalibrationProgress::SampleTaken {
            control,
            index,
            total,
            applied,
            photo,
            ..
        } => format!("{control} {index}/{total}: {applied} photographed as {photo}"),
        CalibrationProgress::SweepFinished { control, samples } => {
            format!("{control}: {samples} sample(s) taken; nothing selected yet")
        }
        CalibrationProgress::SweepInterrupted {
            control,
            taken,
            total,
            detail,
            ..
        } => format!("{control}: stopped after {taken} of {total}: {detail}"),
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
                pairs: Vec::new(),
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
                pairs: Vec::new(),
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
                pairs: Vec::new(),
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
        // `from_raw` — which is the drift docs/9's derived-population rule exists to stop.
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

    #[test]
    fn every_write_warning_and_restore_outcome_has_a_distinct_rendering() {
        // The same completeness rule one phase later. These two vocabularies carry
        // payloads and so have no generated `ALL`; the populations below are exhaustive
        // `match`-shaped constructions, which means adding a variant stops the build here
        // rather than producing a table row nobody wrote.
        let range = schema::control::ControlRange {
            min: 0,
            max: 100,
            step: 5,
        };
        let warnings = vec![
            WriteWarning::Clamped {
                requested: 500,
                applied: 100,
                range,
            },
            WriteWarning::StepAligned {
                requested: 7,
                applied: 5,
                step: 5,
            },
            WriteWarning::Adjusted {
                requested: ControlValue::Int(50),
                applied: ControlValue::Int(42),
            },
            WriteWarning::Unverified {
                because: Unverifiable::TypeHasNoValue,
            },
            WriteWarning::Unverified {
                because: Unverifiable::WriteOnly,
            },
            WriteWarning::Unverified {
                because: Unverifiable::DeviceDeclinedToRead,
            },
        ];
        let mut seen = std::collections::BTreeSet::new();
        for warning in &warnings {
            assert!(
                seen.insert(warning_text(warning)),
                "{warning:?} duplicates a spelling"
            );
        }
        // The `Unverifiable` half is a generated `ALL`, so its coverage is checkable: a
        // new reason must appear above or this count stops matching.
        assert_eq!(
            warnings
                .iter()
                .filter(|w| matches!(w, WriteWarning::Unverified { .. }))
                .count(),
            Unverifiable::ALL.len(),
            "a reason was added to the vocabulary and not to this walk"
        );

        let slug = |s: &str| ControlSlug::parse(s).expect("literal slug");
        let outcomes = vec![
            RestoreOutcome::Restored {
                applied: schema::Applied {
                    control: schema::control::ControlId(1),
                    slug: slug("a"),
                    requested: ControlValue::Int(1),
                    applied: ControlValue::Int(1),
                    warnings: Vec::new(),
                },
            },
            RestoreOutcome::AlreadyCorrect { control: slug("b") },
            RestoreOutcome::OwnedByAutomation {
                control: slug("c"),
                automation: Some(slug("c_auto")),
            },
            RestoreOutcome::OwnedByAutomation {
                control: slug("d"),
                automation: None,
            },
            RestoreOutcome::Unrestorable {
                control: slug("e"),
                reason: UnrestorableReason::StillInactive {
                    automation: Some(slug("e_auto")),
                },
            },
            RestoreOutcome::Unrestorable {
                control: slug("f"),
                reason: UnrestorableReason::StillInactive { automation: None },
            },
            RestoreOutcome::Unrestorable {
                control: slug("g"),
                reason: UnrestorableReason::Volatile,
            },
            RestoreOutcome::Unrestorable {
                control: slug("h"),
                reason: UnrestorableReason::NoLongerWritable,
            },
            RestoreOutcome::Unrestorable {
                control: slug("i"),
                reason: UnrestorableReason::WriteFailed {
                    error: schema::Error::DeviceGone {
                        path: "/dev/video0".into(),
                    },
                },
            },
        ];
        let mut seen = std::collections::BTreeSet::new();
        for outcome in &outcomes {
            let (_, text) = outcome_text(outcome);
            assert!(
                seen.insert(text.clone()),
                "{outcome:?} renders as {text:?}, which is taken"
            );
        }

        // A restore that *worked* and one that did not must not read alike: the whole
        // point of `OwnedByAutomation` is that a caller can tell them apart at a glance.
        let (_, owned) = outcome_text(&outcomes[2]);
        let (_, taken) = outcome_text(&outcomes[4]);
        assert!(owned.contains("as it was"), "{owned}");
        assert!(!taken.contains("as it was"), "{taken}");
    }

    #[test]
    fn every_photo_rendering_and_transform_application_has_a_distinct_rendering() {
        let renderings = vec![
            PhotoRendering::Verbatim {
                source: PixelFormat::MJPG,
            },
            PhotoRendering::DecodedAndEncoded {
                source: PixelFormat::MJPG,
                target: schema::PhotoFormat::Png,
            },
            PhotoRendering::ConvertedAndEncoded {
                source: PixelFormat::YUYV,
                target: schema::PhotoFormat::Png,
            },
        ];
        let mut seen = std::collections::BTreeSet::new();
        for rendering in renderings {
            assert!(
                seen.insert(rendering_text(rendering)),
                "{rendering:?} duplicates a spelling"
            );
        }
        // Only the verbatim rendering may claim byte fidelity, and it is the only one that
        // does. A table that said "unmodified" about a re-encode would be the E6 claim
        // made falsely, in the one place a human reads it.
        assert!(rendering_text(renderings_verbatim()).contains("unmodified"));
        assert!(
            !rendering_text(PhotoRendering::DecodedAndEncoded {
                source: PixelFormat::MJPG,
                target: schema::PhotoFormat::Png,
            })
            .contains("unmodified")
        );

        let mut seen = std::collections::BTreeSet::new();
        for application in [
            TransformApplication::Identity,
            TransformApplication::Pixels,
            TransformApplication::ExifOrientation { orientation: 6 },
        ] {
            assert!(
                seen.insert(transform_text(application)),
                "{application:?} duplicates a spelling"
            );
        }
    }

    fn renderings_verbatim() -> PhotoRendering {
        PhotoRendering::Verbatim {
            source: PixelFormat::MJPG,
        }
    }

    // ------------------------------------------------------------------ calibration

    fn session_fixture() -> Session {
        use schema::session::{ControlSession, Sample, Selector};

        let slug = |s: &str| ControlSlug::parse(s).expect("literal slug");
        let mut session = schema::session::new_session(
            uuid::Uuid::nil(),
            camera("Test", true).fingerprint,
            "read text from the DUT display",
            "legible text",
            "0.1.0",
            schema::time::Stamp::epoch(),
        );
        session.criteria = vec!["text clarity".to_owned()];
        session.queue = vec![slug("focus_absolute"), slug("brightness")];
        session.controls.insert(
            slug("focus_absolute"),
            ControlSession {
                status: ControlStatus::Calibrated {
                    value: 512,
                    precision: 64,
                    score: Some(1234.5),
                    selector: Selector::Metric {
                        name: schema::metrics::MetricName::Sharpness,
                    },
                },
                samples: vec![Sample {
                    requested: 512,
                    applied: 512,
                    warnings: Vec::new(),
                    photo: "photos/focus_absolute/512.jpg".into(),
                    metrics: BTreeMap::new(),
                    captured_at: schema::time::Stamp::epoch(),
                }],
            },
        );
        session.controls.insert(
            slug("privacy"),
            ControlSession {
                status: ControlStatus::Blocked {
                    reason: BlockedReason::ReadOnly,
                },
                samples: Vec::new(),
            },
        );
        session
    }

    #[test]
    fn a_session_table_names_who_chose_and_keeps_the_operators_queue_order() {
        let (mut out, stdout, stderr) = captured();
        session(&session_fixture(), false, &mut out).expect("rendering into a buffer cannot fail");

        let text = stdout.text();
        // D8's own spelling of the selector, from the schema, so the table and the session
        // file say the same thing.
        assert!(text.contains("metric:sharpness"), "{text}");
        assert!(text.contains("512"), "{text}");
        assert!(text.contains("1234.5"), "{text}");
        // The device's reason, not a shrug.
        assert!(text.contains("read-only"), "{text}");
        // Queue order, not map order: `brightness` sorts first alphabetically and is
        // queued second.
        let focus = text.find("focus_absolute").expect("the calibrated control");
        let brightness = text.find("brightness").expect("the queued control");
        assert!(focus < brightness, "the queue order was re-sorted: {text}");
        // A queued control nothing has happened to still has a row: D8 says the absence
        // *is* `Untouched`, and a vanished row would read as a queue nobody drafted.
        assert!(text.contains("untouched"), "{text}");
        // And the one sentence a caller needs before `apply`.
        assert!(stderr.text().contains("--partial"), "{}", stderr.text());
    }

    #[test]
    fn a_settled_session_says_nothing_about_partial() {
        // The inverse of the note above: a session with nothing pending must not tell a
        // caller to pass a flag it does not need.
        let mut fixture = session_fixture();
        fixture
            .queue
            .retain(|slug| slug.as_str() == "focus_absolute");
        let (mut out, _, stderr) = captured();
        session(&fixture, false, &mut out).expect("rendering into a buffer cannot fail");
        assert!(stderr.text().is_empty(), "{}", stderr.text());
    }

    #[test]
    fn the_json_rendering_of_a_session_is_the_document_and_nothing_else() {
        let (mut out, stdout, stderr) = captured();
        let fixture = session_fixture();
        session(&fixture, true, &mut out).expect("rendering into a buffer cannot fail");
        let back: Session = serde_json::from_str(&stdout.text()).expect("valid JSON");
        assert_eq!(back, fixture);
        assert!(stderr.text().is_empty(), "{}", stderr.text());
    }

    #[test]
    fn a_status_rendering_says_why_a_sweep_stopped_and_the_json_carries_the_same_history() {
        // The question `calibrate status` is asked after the terminal that showed the live
        // progress is gone. The document alone says a control stopped at 2 of 5; only the
        // history says the camera was pulled out.
        let stopped = SessionStatus {
            session: session_fixture(),
            log: vec![schema::session::LogEntry {
                at: schema::time::Stamp::epoch(),
                event: SessionEvent::SweepInterrupted {
                    control: ControlSlug::parse("focus_absolute").expect("literal slug"),
                    taken: 2,
                    total: 5,
                    failure: schema::ErrorKind::DeviceGone,
                    detail: "/dev/video0 disappeared".to_owned(),
                },
            }],
        };
        let (mut out, stdout, stderr) = captured();
        status(&stopped, false, &mut out).expect("rendering into a buffer cannot fail");
        assert!(
            stdout.text().contains("history: 1 event(s)"),
            "{}",
            stdout.text()
        );
        let note = stderr.text();
        assert!(note.contains("stopped after 2 of 5"), "{note}");
        assert!(note.contains("disappeared"), "{note}");

        // And `--json` carries the whole history, so the human rendering computes nothing
        // a reader of the document could not.
        let (mut out, stdout, _) = captured();
        status(&stopped, true, &mut out).expect("rendering into a buffer cannot fail");
        assert_eq!(
            serde_json::from_str::<SessionStatus>(&stdout.text()).expect("valid JSON"),
            stopped
        );
    }

    #[test]
    fn an_empty_listing_says_so_and_a_populated_one_names_the_directory() {
        let (mut out, stdout, _) = captured();
        sessions(
            &SessionList {
                sessions: Vec::new(),
            },
            false,
            &mut out,
        )
        .expect("rendering into a buffer cannot fail");
        assert!(stdout.text().contains("no sessions"), "{}", stdout.text());

        let list = SessionList {
            sessions: vec![schema::session::SessionListing {
                id: uuid::Uuid::nil(),
                camera: "obsbot-tiny-3-3-1-1-0".to_owned(),
                task_slug: "read-text".to_owned(),
                path: "/state/tree/obsbot-tiny-3-3-1-1-0/read-text/0".into(),
            }],
        };
        let (mut out, stdout, _) = captured();
        sessions(&list, false, &mut out).expect("rendering into a buffer cannot fail");
        let text = stdout.text();
        assert!(text.contains("obsbot-tiny-3-3-1-1-0"), "{text}");
        assert!(text.contains("read-text"), "{text}");
        // The path, because the whole point of an inspectable session tree (D9) is that a
        // reader can go and look.
        assert!(text.contains("/state/tree/"), "{text}");
    }

    #[test]
    fn every_control_status_and_blocked_reason_has_a_distinct_rendering() {
        // The same completeness rule as the write warnings above, for D8's two closed
        // vocabularies: a status or a reason sharing a spelling with another is one a
        // reader of the table cannot tell apart.
        use schema::session::{Selector, SweepSpec};

        let slug = |s: &str| ControlSlug::parse(s).expect("literal slug");
        let statuses = vec![
            ControlStatus::Untouched,
            ControlStatus::AutoDisabled {
                automation: vec![slug("focus_automatic_continuous")],
                parked_value: Some(200),
            },
            ControlStatus::Sweeping {
                plan: SweepSpec::All,
                done: 3,
                total: 16,
            },
            ControlStatus::Calibrated {
                value: 512,
                precision: 64,
                score: Some(1.0),
                selector: Selector::Human,
            },
            ControlStatus::Deferred {
                reason: "no lens".to_owned(),
            },
            ControlStatus::Blocked {
                reason: BlockedReason::ReadOnly,
            },
        ];
        let mut seen = std::collections::BTreeSet::new();
        for status in &statuses {
            let cells = status_cells(status);
            assert!(
                seen.insert(cells.state.clone()),
                "{status:?} renders as {:?}, which is taken",
                cells.state
            );
        }
        // Six, because D8's vocabulary has six and a seventh must not slip in unrendered.
        assert_eq!(seen.len(), 6);

        // A single-sample sweep has no spacing to report, and the table must not print a
        // precision of zero as though it were one a refinement pass could divide down.
        let single = status_cells(&ControlStatus::Calibrated {
            value: 1,
            precision: 0,
            score: None,
            selector: Selector::Agent,
        });
        assert!(single.precision.contains("single"), "{}", single.precision);

        let mut seen = std::collections::BTreeSet::new();
        for reason in [
            BlockedReason::ReadOnly,
            BlockedReason::Disabled,
            BlockedReason::InactiveWithoutPartner,
            BlockedReason::NotSweepable {
                control_type: "rect".to_owned(),
            },
            BlockedReason::Other {
                detail: "something else".to_owned(),
            },
        ] {
            assert!(
                seen.insert(blocked_text(&reason)),
                "{reason:?} duplicates a spelling"
            );
        }
    }

    #[test]
    fn every_progress_event_has_a_distinct_line_and_a_clamp_shows_at_the_moment_it_happens() {
        use schema::session::SweepSpec;

        let slug = |s: &str| ControlSlug::parse(s).expect("literal slug");
        let events = vec![
            CalibrationProgress::SweepStarted {
                control: slug("focus_absolute"),
                plan: SweepSpec::All,
                total: 16,
            },
            CalibrationProgress::ValueSet {
                control: slug("focus_absolute"),
                index: 1,
                total: 16,
                requested: 42,
                applied: 42,
                warnings: Vec::new(),
            },
            CalibrationProgress::SampleTaken {
                control: slug("focus_absolute"),
                index: 1,
                total: 16,
                requested: 42,
                applied: 42,
                photo: "photos/focus_absolute/42.jpg".into(),
                metrics: BTreeMap::new(),
            },
            CalibrationProgress::SweepFinished {
                control: slug("focus_absolute"),
                samples: 16,
            },
            CalibrationProgress::SweepInterrupted {
                control: slug("focus_absolute"),
                taken: 3,
                total: 16,
                failure: schema::ErrorKind::DeviceGone,
                detail: "/dev/video0 disappeared".to_owned(),
            },
        ];
        let mut seen = std::collections::BTreeSet::new();
        for progress in &events {
            assert!(
                seen.insert(progress_line(progress)),
                "{progress:?} duplicates a line"
            );
        }

        // PF:6 while it happens: a write the driver moved says so on the bar, not only in
        // the session file afterwards.
        let moved = progress_line(&CalibrationProgress::ValueSet {
            control: slug("focus_absolute"),
            index: 2,
            total: 16,
            requested: 42,
            applied: 40,
            warnings: vec![WriteWarning::StepAligned {
                requested: 42,
                applied: 40,
                step: 5,
            }],
        });
        assert!(moved.contains("42 → 40"), "{moved}");
        // …and an exact write does not decorate itself with an arrow to nowhere.
        let exact = progress_line(&events[1]);
        assert!(!exact.contains('→'), "{exact}");

        // The finished line says what a sweep deliberately does *not* do (D8): choose.
        let finished = progress_line(&events[3]);
        assert!(finished.contains("nothing selected"), "{finished}");
    }

    #[test]
    fn the_progress_bar_template_parses_and_shows_the_line_the_events_produce() {
        // `Bar::new` falls back to indicatif's default style if this does not parse, and
        // the default style has no `{msg}` in it — so a broken template would silently
        // throw away every line `progress_line` composes.
        indicatif::ProgressStyle::with_template(BAR_TEMPLATE).expect("the bar's own template");
        assert!(
            BAR_TEMPLATE.contains("{msg}"),
            "a bar with no message says a sweep is happening and not what it is doing"
        );
    }

    #[test]
    fn the_json_answer_draws_no_progress_bar_and_the_human_one_does() {
        // The bar goes to standard error and the document to standard output, so they
        // cannot collide — but a caller redirecting both into one file would get a document
        // with a bar in it, and the answer is not to draw one.
        let quiet = watcher(true);
        let drawn = watcher(false);
        assert_eq!(format!("{quiet:?}"), "Quiet");
        assert!(format!("{drawn:?}").starts_with("Bar"), "{drawn:?}");

        // Both are total: a watcher that panicked on an event would take a sweep with it.
        let event = ProgressEvent {
            session: uuid::Uuid::nil(),
            at: schema::time::Stamp::epoch(),
            progress: CalibrationProgress::SweepFinished {
                control: ControlSlug::parse("brightness").expect("literal slug"),
                samples: 2,
            },
        };
        quiet.event(&event);
        quiet.finish();
        drawn.event(&event);
        drawn.finish();
    }
}
