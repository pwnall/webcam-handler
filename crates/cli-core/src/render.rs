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
//!
//! **A failure is a different document, not a wrapper around an answer** (owner ruling,
//! 2026-08-15; note **N127**). [`failure`] prints [`schema::error::Failure`] through the same
//! emitter every answer goes through, so the rule above is unchanged in both directions: a
//! `--json` invocation still prints exactly one `webcam-handler-schema` type verbatim, and
//! which type it is says whether the verb answered.

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
use schema::profile::{DeviceProfile, ProfileComparison};
use schema::progress::{CalibrationProgress, ProgressEvent};
use schema::report::{CameraDetail, CameraList, ControlReport, WriteReport};
use schema::session::{
    BlockedReason, ControlStatus, SampleCap, Session, SessionEvent, SessionList, SessionStatus,
    SweepAdjustment,
};
use schema::snapshot::{RestoreOutcome, RestoreReport, Snapshot, UnrestorableReason};
use schema::video::{IntervalSource, RecordReport, RecordingEnd};

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

    /// Write raw bytes to the answer, with no newline and no interpretation.
    ///
    /// `webcam-handler-cli photo cam:x > shot.jpg` is the reason this exists: with no `-o`,
    /// the photo's bytes *are* the answer, and a `line` would append a byte the image does not
    /// have.
    ///
    /// # Errors
    ///
    /// As [`Output::line`].
    pub fn bytes(&mut self, data: &[u8]) -> Result<()> {
        let sink = &mut self.stdout;
        sink.write_all(data)
            .and_then(|()| sink.flush())
            .map_err(|error| Error::StorageIo {
                path: "<stdout>".into(),
                errno: error.raw_os_error(),
                message: error.to_string(),
            })
    }

    /// Write a line of the answer.
    ///
    /// # Errors
    ///
    /// [`Error::StorageIo`] when the stream refuses — a closed pipe is a real outcome and
    /// `webcam-handler-cli list | head -1` must not panic on it.
    pub fn line(&mut self, text: &str) -> Result<()> {
        writeln!(&mut self.stdout, "{text}").map_err(|error| Error::StorageIo {
            path: "<stdout>".into(),
            errno: error.raw_os_error(),
            message: error.to_string(),
        })
    }

    /// Write a line *about* the answer, on standard error, and answer nothing.
    ///
    /// **A note that cannot be printed is not the verb's failure** (docs/11 **M28**, note
    /// **N216**). Every commentary line this surface writes had the shape
    /// `out.note(…)`, so a failing standard error replaced an outcome the
    /// verb had already achieved: the JPEG was on standard output, the snapshot was on disk,
    /// the sweep was persisted — and the process answered `failed: true` and exited 27,
    /// about the *pipe*. Worse under `--json`, where the answer document had already been
    /// printed, so standard output carried an answer **and** a `Failure`, which is exactly
    /// the pair design §2.7 says can never occur: *"a `--json` invocation prints exactly one
    /// `webcam-handler-schema` type, and which type it is says whether the verb answered"*.
    ///
    /// So this returns `()`, and the rule is structural rather than remembered: the type
    /// system has no `?` to offer a caller here, and standard error has no other door —
    /// [`Output::line`] and [`Output::bytes`] write the answer, this writes everything else.
    /// `report_failure`'s human line takes the same route and always did, for the same
    /// reason stated one function along: a refusal whose cause was a closed stream must not
    /// be replaced by one about the stream.
    ///
    /// The write is still *attempted*, and its failure is still visible where such a failure
    /// can be acted on — the answer's own stream refusing is [`Output::line`]'s error, and
    /// that one is propagated.
    pub fn note(&mut self, text: &str) {
        let _ = writeln!(&mut self.stderr, "{text}");
    }
}

/// Say what the pair probe touched and what it put back, on standard error.
///
/// The `--json` document carries the *pairs*; what a probe declined and how the restore went
/// are facts about the run rather than about the camera, and a caller redirecting stdout
/// should still see them.
///
/// **Here rather than in a binary, because it has two binaries.** It began in
/// `crates/cli/src/main.rs`, where `webcam-handler-cli` was the only root that could run a
/// probe; P4f gives `webcam-handler-client` the same two callers — `controls --discover-pairs`
/// over `wch_discover_pairs`, whose answer carries these two fields on the wire precisely so a
/// socket client is not running a write with its restoration report withheld (note N30) — and
/// two copies of a rendering is the second home design §2.10 forbids and the thing the parity
/// gate would then be comparing against itself.
///
/// `program` is a parameter for [`crate::Program`]'s reason: the line names the binary that
/// met the probe, and `webcam-handler-cli:` in `webcam-handler-client`'s mouth would send an
/// operator to the wrong `--help`.
///
/// Takes the two facts rather than a probe result, because its callers hold them in
/// different shapes: a [`schema::report::DiscoveryReport`] over the wire, and the engine's
/// own `Discovery` in `webcam-handler-cli`.
///
/// Writes with `eprintln!` rather than through [`Output`]: its callers are inside
/// [`crate::Executor`] implementations, which are handed no output sink — the seam answers
/// with schema values and renders nothing, and threading a sink through it to carry two
/// notes would be a wider change to the trait than the notes are worth.
pub fn report_probe(
    program: crate::Program,
    skipped: &[schema::pairing::ProbeSkip],
    restored: &RestoreReport,
) {
    for skip in skipped {
        eprintln!("{program}: did not probe {}: {}", skip.control, skip.reason);
    }
    if !restored.is_complete() {
        let stuck: Vec<String> = restored
            .unrestored()
            .iter()
            .map(ToString::to_string)
            .collect();
        eprintln!(
            "{program}: the probe could not put {} control(s) back: {}",
            stuck.len(),
            stuck.join(", ")
        );
    }
}

/// Serialize a schema value as the `--json` answer.
fn json<T: serde::Serialize>(value: &T, out: &mut Output) -> Result<()> {
    let text = serde_json::to_string_pretty(value).map_err(|error| Error::StorageIo {
        path: "<stdout>".into(),
        errno: None,
        message: format!("could not serialize the answer: {error}"),
    })?;
    out.line(&text)
}

/// The failure document, on standard output, through the one `--json` emitter.
///
/// **Through [`json`] and never a `to_string_pretty` of its own**, which is the whole of what
/// this function contributes: `scripts/gates/cli-parity.sh`'s fork case seeds its defect into
/// that emitter and builds both binaries, so a refusal document rendered beside it rather than
/// through it would be the one `--json` answer in this workspace that a fork could not reach.
/// One emitter, one place a byte can go wrong.
///
/// Standard output, because that is the `--json` channel: note **N124** measured that a caller
/// redirecting stdout lost the failure entirely, and putting the repair on standard error would
/// be the same defect wearing a different costume. The human line on standard error is
/// unchanged and is `crate::report_failure`'s other half.
///
/// # Errors
///
/// As [`Output::line`] — a closed pipe is a real outcome, and a caller that piped a refusal
/// into `head` must not panic on it.
pub(crate) fn failure(document: &schema::error::Failure, out: &mut Output) -> Result<()> {
    json(document, out)
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
        out.line("no cameras")?;
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
        out.line(&table.to_string())?;
    }

    // D1's diagnosis. On stderr because it is about the answer rather than part of it.
    for hint in &list.hints {
        out.note(&format!("note: {}", hint.message()));
    }
    Ok(())
}

/// `info`.
pub(crate) fn info(detail: &CameraDetail, as_json: bool, out: &mut Output) -> Result<()> {
    if as_json {
        return json(detail, out);
    }

    out.line(&identity_table(&detail.info).to_string())?;

    if detail.formats.is_empty() {
        out.line("\nformats: none (this camera has no capture node)")?;
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
    out.line(&table.to_string())
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
        .map(frame_interval_text)
        .collect::<Vec<_>>()
        .join(", ")
}

/// One frame interval from a device's vocabulary, as a human reads it.
///
/// Every renderer of a [`FrameInterval`] goes through here, and that is the point rather than
/// tidiness: [`adjustment_text`] had its own, spelled `fps().unwrap_or(0.0)`, so on the
/// exact devices the vocabulary's fourth and degenerate answers are about — a driver that
/// offers no interval, a driver that writes `1/0` — the D5 line a human reads said
/// **"asked 30, got 0"** (note **N199**). A rate that does not exist has a spelling here,
/// and it is not a number.
fn frame_interval_text(interval: &FrameInterval) -> String {
    match interval {
        FrameInterval::Discrete {
            numerator,
            denominator,
        } => interval.fps().map_or_else(
            // The device's own fraction, because there is no rate to print and the
            // fraction is what it actually said (D2).
            || format!("({numerator}/{denominator}, not a rate)"),
            |fps| format!("{}", round2(fps)),
        ),
        FrameInterval::Stepwise {
            min_numerator,
            min_denominator,
            max_numerator,
            max_denominator,
        } => format!("{min_numerator}/{min_denominator}-{max_numerator}/{max_denominator}"),
        FrameInterval::Unknown { raw } => format!("(unreadable shape {raw:#x})"),
        // Not "unreadable": the device answered, and the answer was that it does not
        // negotiate a frame interval on this node.
        FrameInterval::Unstated => "(not offered)".to_owned(),
    }
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
    out.line(&table.to_string())?;

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
        out.line(&format!("\n{} menu: {items}", desc.slug.as_str()))?;
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
        out.note(&format!(
            "note: {} control(s) report a default or current value outside their own \
                 declared range, marked `!` above: {names}. This is the device's answer, \
                 reported rather than corrected [PF:4, PF:5].",
            odd.len()
        ));
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
        out.line("\nauto/manual pairs:")?;
        out.line(&pairs.to_string())?;
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
        // **Two absences, and this column is where a person finds out which.** A button
        // and a write-only control have no value by their own declaration, and `—` is the
        // right word for that — it is also what this table already prints for "no
        // meaningful range" and "no flags", so it has to keep meaning *nothing to say*. A
        // control the device was asked about and would not answer is a different fact, and
        // the tolerance that carries it through the walk (rule 7, note **N192**) is only
        // honest if a reader can see it (note **N199**).
        None if desc.value_was_declined() => "(declined)".to_owned(),
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
    out.line(&table.to_string())?;

    // The marks the table cannot carry, on stderr so a piped table stays a table. Read
    // from the schema's own predicates, so `--json` supports the same conclusion.
    if desc.current_out_of_range() {
        out.note(&format!(
            "note: {}'s current value is outside its declared range [PF:4] — reported, \
                 not corrected",
            desc.slug
        ));
    }
    if desc.default_out_of_range() {
        out.note(&format!(
            "note: {}'s default is outside its declared range [PF:5]",
            desc.slug
        ));
    }
    if desc.is_inactive() {
        out.note(&format!(
            "note: {} is INACTIVE — an automation control owns it right now [PF:3]",
            desc.slug
        ));
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
    out.line(&table.to_string())?;

    if !report.disabled_automation.is_empty() {
        // A guarded write changes more than the caller named, and that is a change to the
        // camera they are entitled to hear about at the moment it happens.
        out.note(&format!(
            "note: switched off to make the write stick: {}",
            report
                .disabled_automation
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if !report.is_exact() {
        out.note(&format!(
            "note: {} write(s) did not land exactly as asked",
            report.inexact().len()
        ));
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
        None => out.line(&text),
        Some(path) => {
            std::fs::write(path, format!("{text}\n")).map_err(|error| Error::StorageIo {
                path: path.to_path_buf(),
                errno: error.raw_os_error(),
                message: error.to_string(),
            })?;
            out.note(&format!(
                "wrote {path} ({} control(s))",
                snapshot.entries.len()
            ));
            Ok(())
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
    out.line(&table.to_string())?;

    // A restore repairs the session as well as the camera, and the second half has nothing
    // to do with the snapshot: a sweep killed before its first sample leaves a control every
    // verb refuses and nothing to put back (note **N139**). A run that changed a status on
    // disk and printed an empty table would be telling the operator nothing happened.
    if !report.freed.is_empty() {
        out.note(&format!(
            "note: {} control(s) were left mid-sweep by a process that is gone and have \
                 been given back: {}",
            report.freed.len(),
            report
                .freed
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }

    // The one line that matters, and it is on stderr because a caller scripting a restore
    // wants the exit code and the table, not prose in the middle of them.
    if !report.is_complete() {
        out.note(&format!(
            "note: {} control(s) did not come back: {}",
            report.unrestored().len(),
            report
                .unrestored()
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    Ok(())
}

/// `photo` — where the bytes went, and what was done to them.
///
/// With no `-o`, the bytes go to standard output and the summary to standard error, so
/// `webcam-handler-cli photo cam:x > shot.jpg` is a photo and not a photo with a table in it.
/// `--json` requires `-o` for exactly that reason, and clap enforces it.
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
        out.bytes(bytes)?;
    }

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
    // Where the summary goes is decided by whether the *answer* was the bytes. With `-o` the
    // table is the answer and goes to standard output; with the photo itself on standard
    // output the table is commentary, and commentary on the answer's own stream would land
    // inside the image (note **N216** for why the two doors are two functions).
    if returned.is_some() {
        out.note(&table.to_string());
    } else {
        out.line(&table.to_string())?;
    }

    if !report.negotiated.is_exact() {
        out.note(&format!(
            "note: the device adjusted the request: {}",
            report
                .negotiated
                .adjustments
                .iter()
                .map(adjustment_text)
                .collect::<Vec<_>>()
                .join("; ")
        ));
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
                UnrestorableReason::NeverRecorded => {
                    "the device would not read it when the snapshot was taken, so there \
                     is no value to put back"
                        .to_owned()
                }
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
            frame_interval_text(requested),
            frame_interval_text(negotiated)
        ),
    }
}

// ------------------------------------------------------------------ recording

/// `record` — what the take turned out to be.
///
/// The `--json` half is [`RecordReport`] verbatim, which is the whole of this crate's `--json`
/// contract; the human half is the table beside it. The two show the **same** facts, which is
/// why every row below is a field of the document rather than something computed here — with
/// one exception that proves the rule: the mean interval is
/// [`schema::video::RecordingSummary::measured_interval_us`], the schema's own subtraction,
/// so a `--json` consumer reaches the identical number by calling the identical function
/// rather than by re-deriving `span / (frames - 1)` and getting `span / frames`.
///
/// **The rate rows are the payload rather than the metadata**, which is why there are two of
/// them. The notes' Expected usage item 10 is blunt about what a recording is for — *"did this
/// take 200 ms or 2 s"* — so a reader has to be able to tell a rate that was *observed* from
/// one the camera was merely asked for, and `interval_source` is the field that says which
/// this file's header field is. Either container may report `measured` since P6d — note
/// **N106**'s amendment, and the oracle that settled it — but a take of one frame measures
/// nothing in either, so the "measured" row beside it is still what keeps a `negotiated` header
/// from costing the reader the measurement.
///
/// The path goes in the table and never on standard output as bytes: a recording is not
/// returned as bytes at all (note **N110**), so unlike `photo` this renderer has no stream to
/// share and no `--json` restriction to enforce.
pub(crate) fn record(report: &RecordReport, as_json: bool, out: &mut Output) -> Result<()> {
    if as_json {
        return json(report, out);
    }

    let summary = &report.summary;
    let mut table = table();
    table.set_header(vec!["FIELD", "VALUE"]);
    for (field, value) in [
        ("camera", report.camera.to_string()),
        ("started at", report.started_at.to_string()),
        ("file", report.path.to_string()),
        ("container", report.format.to_string()),
        ("stream", stream_text(&report.negotiated)),
        ("ended", ended_text(report.ended)),
        ("frames", summary.frames_written.to_string()),
        ("dropped", summary.dropped_frames.to_string()),
        ("bytes", summary.bytes_written.to_string()),
        ("wall clock", format!("{} ms", report.wall_clock_ms)),
        ("declared rate", declared_rate_text(report)),
        ("measured rate", measured_rate_text(report)),
    ] {
        table.add_row(vec![Cell::new(field), Cell::new(value)]);
    }
    out.line(&table.to_string())?;

    // The three notes an operator acts on, on standard error so the table stays the answer.
    // Each names something the fields above state but do not interpret — which is the split
    // this module's header describes: the renderer may explain a field and may not compute a
    // fact the document does not carry.
    if let Some(cap) = summary.cap_reached {
        out.note(&format!(
            "note: the {} cap ended this recording before its duration was spent; the \
                 file holds everything up to that point",
            format!("{cap:?}").to_lowercase()
        ));
    }
    if summary.dropped_frames > 0 {
        out.note(&format!(
            "note: the driver's sequence numbers say {} frame(s) never arrived; a dropped \
                 frame reads as a slow transition unless it is counted",
            summary.dropped_frames
        ));
    }
    if !report.negotiated.is_exact() {
        out.note(&format!(
            "note: the device adjusted the request: {}",
            report
                .negotiated
                .adjustments
                .iter()
                .map(adjustment_text)
                .collect::<Vec<_>>()
                .join("; ")
        ));
    }
    Ok(())
}

/// Why a recording stopped, as a phrase.
///
/// An exhaustive `match` over the closed vocabulary, so a sixth ending is a compile error here
/// rather than a row that renders as `Debug` — which is `schema::video::RecordingEnd`'s own
/// requirement of its consumers, and the reason it is a `closed_vocabulary!`.
fn ended_text(ended: RecordingEnd) -> String {
    match ended {
        RecordingEnd::Duration => "the duration you asked for was spent".to_owned(),
        RecordingEnd::Cap => "a size, frame or span cap refused a frame".to_owned(),
        RecordingEnd::DeviceQuiet => "the camera stopped delivering frames".to_owned(),
        RecordingEnd::Stopped => "you stopped it".to_owned(),
        RecordingEnd::DeviceFailed => "the device refused mid-take".to_owned(),
    }
}

/// The rate the finished file declares, and where that number came from.
///
/// The provenance is in the same cell as the number rather than in a column of its own,
/// because the two are one fact: a declared interval whose source a reader has to look up in
/// another row is a number they will read as measured.
fn declared_rate_text(report: &RecordReport) -> String {
    let interval = report.summary.declared_interval_us;
    let source = match report.summary.interval_source {
        IntervalSource::Measured => "measured across this take",
        IntervalSource::Negotiated => "what the camera was asked for",
        IntervalSource::Provisional => "a placeholder; nothing was measured or negotiated",
    };
    format!("{} ({source})", interval_text(interval))
}

/// The rate this take actually delivered, when it delivered enough frames to measure one.
///
/// `schema::video::RecordingSummary::measured_interval_us` and never an arithmetic of this
/// file's own — the schema states the three refusals (fewer than two frames, a clock that ran
/// backwards, a mean that truncates to zero) once, so a table and a `--json` consumer cannot
/// disagree about what "measured nothing" is.
fn measured_rate_text(report: &RecordReport) -> String {
    report.summary.measured_interval_us().map_or_else(
        || "not measured; this take spans too little to have a rate".to_owned(),
        |mean| {
            let interval = u32::try_from(mean).unwrap_or(u32::MAX);
            interval_text(interval)
        },
    )
}

/// One frame interval, in microseconds and in frames per second.
///
/// Both, because they answer different questions: a caller comparing this take against
/// `negotiated` wants the interval, and a person reading a table wants the rate. A zero
/// interval is neither — it is a rate nobody can divide by — and says so rather than printing
/// an infinity.
fn interval_text(interval_us: u32) -> String {
    if interval_us == 0 {
        return "0 µs (no rate)".to_owned();
    }
    let fps = 1_000_000.0 / f64::from(interval_us);
    format!("{interval_us} µs ({fps:.1} fps)")
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
    out.line(&header.to_string())?;

    // **Two different empties, and printing one sentence for both told a user their plan had
    // done nothing.** `controls` is the per-control *status* map — a control appears in it once
    // a sweep has touched it — while `queue` is what `calibrate plan` drafts. A freshly planned
    // session therefore has a populated `queue` and an empty `controls`, which this arm used to
    // render as "no controls queued yet — `calibrate plan` drafts them": advice to run the verb
    // that had just been run, on the one screen that was supposed to confirm it.
    //
    // Found by writing the README's calibration walkthrough against the real binaries rather
    // than from the source (owner's request 2, note **N90**), which is the first time anybody
    // had read this screen in the order a new user meets it.
    // **Two different empties, and printing one sentence for both told a user their plan had
    // done nothing.** `controls` is the per-control *status* map — a control appears in it once
    // a sweep has touched it — while `queue` is what `calibrate plan` drafts. A freshly planned
    // session therefore has a populated `queue` and an empty `controls`, which this arm used to
    // render as "no controls queued yet — `calibrate plan` drafts them": advice to run the verb
    // that had just been run, on the one screen that was supposed to confirm it.
    //
    // Found by writing the README's calibration walkthrough against the real binaries rather
    // than from the source (owner's request 2, note **N90**), which is the first time anybody
    // had read this screen in the order a new user meets it.
    if session.controls.is_empty() {
        if session.queue.is_empty() {
            return out.line("\nno controls queued yet — `calibrate plan` drafts them");
        }
        let queued = session
            .queue
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", ");
        return out.line(
            &format!(
                "\n{} control(s) queued and none swept yet — `calibrate sweep` takes the samples: {queued}",
                session.queue.len()
            ),
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
    out.line(&controls.to_string())?;

    // The one sentence a caller needs before `apply`: whether anything is still pending.
    // Read from the schema's own predicate, so `--json` supports the same conclusion.
    if !session.is_settled() {
        out.note(
            "note: this session still has queued work; `calibrate apply` needs --partial \
             until it settles",
        );
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
        ControlStatus::Sweeping {
            done,
            total,
            precision,
            ..
        } => StatusCells {
            state: format!("sweeping {done}/{total}"),
            value: dash(),
            // The stride the planner arrived at, which is the number a caller compares
            // against the `--precision` it typed (note **N145**). A dash here would be the
            // table saying "no spacing" about a sweep whose whole subject is one.
            //
            // **Zero is one fact and it is not that one** (note **N149**'s shape, a page on
            // from the `Calibrated` arm below). This build's planner cannot produce a zero
            // stride — `engine::sweep::precision_of` falls back to the descriptor's
            // `effective_step`, which is at least 1 — so a zero can only have come from a
            // document written before the field existed (`#[serde(default)]`). Naming that
            // is the whole difference between "this sweep has no spacing" and "nobody
            // recorded one", and a dash would say the first about the second.
            precision: if *precision == 0 {
                "(not recorded)".to_owned()
            } else {
                format!("{precision} (planned)")
            },
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
        out.line(&format!(
            "\nhistory: {} event(s), last at {}",
            status.log.len(),
            last.at
        ))?;
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
            // The D13 discriminant in **the registry's own serde spelling**, which is what
            // the `--json` view of this same line carries and what an agent dispatches on
            // (note **N149**); `{failure:?}` printed `IllegalTransition` beside a document
            // saying `illegal_transition`, which is two spellings for one value. And a
            // *known* reason is what earns the parenthesis: an absent `failure` is a line
            // whose writer could not name one, and inventing "(none)" would read as a kind.
            let named = failure
                .as_ref()
                .and_then(|kind| serde_json::to_value(kind).ok())
                .and_then(|value| value.as_str().map(ToOwned::to_owned))
                .map_or_else(String::new, |name| format!(" ({name})"));
            out.note(&format!(
                "note: the sweep of {control} stopped after {taken} of {total} sample(s) \
                     at {}: {detail}{named}",
                entry.at
            ));
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
        return out.line("no sessions");
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
    out.line(&table.to_string())
}

// ------------------------------------------------------------------ sweep progress

/// Where a running sweep's events go while the CLI renders them.
///
/// This crate's seam, not the engine's, and the wall is why: `webcam-handler-client` links no
/// engine (T6), so the shared command surface cannot name `engine::progress::ProgressSink`.
/// Each binary bridges the stream it has — an in-process sink for `webcam-handler-cli`, a
/// subscription for `webcam-handler-client` at P4e — onto this one object, and the rendering
/// happens once, here. The events themselves are schema DTOs on both sides, so nothing is
/// translated in between.
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
///
/// **What a `--json` caller loses by that is nothing it needs, and that is a property of the
/// answer rather than of this function** (note **N145**). A progress stream is a *live*
/// view; the facts on it that outlive the sweep are on the answer document and in the session
/// history — `ControlStatus::Sweeping` carries the stride and every planner adjustment, and
/// `SessionEvent::SweepStarted` carries the same pair for a reader who arrives later. It
/// was not true when this function was written: the answer said `"total": 251` and the
/// adjustments existed only on an event this discards, which is the shape M14 was raised
/// about. An agent that wants the events as they happen subscribes to them through
/// `webcam-handler-client`, which is the surface that has them.
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
            CalibrationProgress::SweepStarted {
                total, adjustments, ..
            } => {
                self.bar
                    .set_draw_target(indicatif::ProgressDrawTarget::stderr());
                self.bar.set_length(u64::from(*total));
                self.bar.set_position(0);
                // **An adjustment is printed, not merely set as the message** (note
                // **N145**). The bar's message is overwritten by the next `ValueSet` a
                // settle later and wiped by `finish_and_clear`, so a sweep whose stride was
                // widened forty-fold announced that fact for a few seconds to whoever
                // happened to be looking. It is the one thing on this stream that changes
                // what every photograph the sweep is about to take is worth, so it goes to
                // scrollback where a terminal keeps it — the same reason `is_terminal`
                // events are printed below.
                if !adjustments.is_empty() {
                    self.bar.println(progress_line(&event.progress));
                }
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
        CalibrationProgress::SweepStarted {
            control,
            total,
            precision,
            adjustments,
            ..
        } => {
            let mut line = format!("sweeping {control}: {total} sample(s)");
            // The stride, because "251 sample(s)" and "251 sample(s) every 40" are different
            // news to somebody who asked for every value (note **N145**). Zero is a
            // single-value plan and has no stride to report.
            if *precision != 0 {
                line.push_str(&format!(" every {precision}"));
            }
            // What the planner did to the spec, at the moment it happened rather than never.
            // Rendered here so both roots say it: `webcam-handler-cli` and
            // `webcam-handler-client` share this function, and a difference between the two
            // would be the parity claim quietly stopping being true.
            for adjustment in adjustments {
                line.push_str(&format!("; {}", sweep_adjustment_line(adjustment)));
            }
            line
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

/// One planner adjustment, in words (note **N145**).
///
/// An exhaustive `match` rather than a `Debug` rendering, so a fifth kind cannot acquire a
/// spelling by accident (rubric rule 6) — and because the numbers are what a caller acts on:
/// "251 of a requested 10 001" is a sentence an agent can compare against the precision it
/// asked for, and `Capped { requested: 10001, .. }` is not.
fn sweep_adjustment_line(adjustment: &SweepAdjustment) -> String {
    match adjustment {
        SweepAdjustment::Clamped { requested, planned } => {
            format!("{requested} clamped to {planned}, outside this control's range")
        }
        SweepAdjustment::StepAligned { requested, planned } => {
            format!("{requested} aligned to {planned}, the nearest step this control has")
        }
        SweepAdjustment::Deduplicated { dropped } => {
            format!("{dropped} value(s) collapsed onto ones already planned")
        }
        SweepAdjustment::Capped {
            requested,
            planned,
            limit,
            cap,
        } => {
            let which = match cap {
                // Named rather than numbered, because the two caps are refused for different
                // reasons and only one of them is about wear (design §5).
                SampleCap::Total => "the sweep cap",
                SampleCap::Motion => "the motion cap, because this control moves motors",
            };
            format!(
                "{planned} of a requested {requested} sample(s), strided to fit {limit} — \
                 {which}"
            )
        }
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
        None => out.line(&text),
        Some(path) => {
            // A trailing newline: the committed corpus is diffed by humans and by
            // `git`, and a file without one is a permanent "\ No newline" in every diff.
            std::fs::write(path, format!("{text}\n")).map_err(|error| Error::StorageIo {
                path: path.to_path_buf(),
                errno: error.raw_os_error(),
                message: error.to_string(),
            })?;
            out.note(&format!("wrote {path}"));
            Ok(())
        }
    }
}

/// `profile compare` — what the two documents say about the device, and about where it is.
///
/// Two rows, because D15's partition is the answer's shape and a reader has the two questions
/// it separates: *is this the same device* and *what identity moved*. A camera reached over a
/// forwarded bus is expected to fill the second row and must leave the first one empty, and a
/// rendering that ran them together would hide exactly the distinction the verb exists for.
///
/// Every word in the verdict column comes from the value's own `Display` — `DeviceDifference`
/// names the sections it found and puts the differing control slugs underneath theirs — rather
/// than from a walk written here. That is design §2.10 at its narrowest: which sections count
/// as a disagreement, how they are spelled and in what order they are listed is settled in
/// `schema::profile` and read here, so this table and the `--json` document cannot come to
/// disagree about what differs.
pub(crate) fn comparison(
    comparison: &ProfileComparison,
    as_json: bool,
    out: &mut Output,
) -> Result<()> {
    if as_json {
        return json(comparison, out);
    }

    let mut table = table();
    table.set_header(vec!["HALF", "VERDICT"]);
    table.add_row(vec![
        Cell::new("device"),
        // "same device" in words rather than an empty cell, for the reason `ProfileComparison`
        // gives for saying it in its own `Display`: an empty string is the one answer a reader
        // cannot tell from a failure to print.
        Cell::new(if comparison.device_matches() {
            "same device".to_owned()
        } else {
            format!("differs: {}", comparison.device)
        }),
    ]);
    table.add_row(vec![
        Cell::new("identity"),
        Cell::new(if comparison.identity.is_empty() {
            "same address".to_owned()
        } else {
            format!("differs: {}", comparison.identity.join(", "))
        }),
    ]);
    out.line(&table.to_string())?;

    // The owner's 2026-08-13 ruling as the one line a reader can act on. It is computed by
    // the answer rather than here, so a caller reading `--json` reaches the same conclusion
    // from the same three fields — the rule this module opens with — and this tool reports
    // the shape rather than guessing what it means for somebody else's rig.
    if comparison.device_differs_only_in_the_format_tree() {
        out.note(
            "note: the format tree is the only device section that differs, and a camera may \
             advertise a different one each time it is plugged in",
        );
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
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
    ///
    /// `pub(crate)` so `crate::tests` can assert what `crate::report_failure` puts on each of
    /// the two streams: that function is this module's emitter plus the process's other
    /// channel, and a second buffer written beside this one would be two answers to "what did
    /// the program print" in the two places most likely to disagree.
    #[derive(Clone, Default)]
    pub(crate) struct Buffer(Arc<Mutex<Vec<u8>>>);

    impl Buffer {
        pub(crate) fn text(&self) -> String {
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

    #[test]
    fn a_value_the_device_declined_reads_differently_from_one_the_control_never_had() {
        // The trade note N192 accepted rests on a reader being able to tell a predicted
        // absence from an unpredicted one, and this column is where a person makes that
        // distinction. It printed `—` for both — the same literal this table already uses
        // for "no meaningful range" and "no flags", so a control the device declined read
        // exactly like a button (note **N199**).
        let button = ControlDesc {
            current: None,
            ..desc("Reset", ControlType::Button)
        };
        assert_eq!(current_text(&button), "—");

        let declined = ControlDesc {
            current: None,
            ..desc("Auto Exposure", ControlType::Integer)
        };
        assert_eq!(current_text(&declined), "(declined)");
        assert_ne!(current_text(&declined), current_text(&button));

        // And a value that arrived still prints as itself, including one outside its
        // declared range [PF:4].
        assert_eq!(
            current_text(&desc("Brightness", ControlType::Integer)),
            "50"
        );
    }

    #[test]
    fn an_interval_with_no_rate_in_it_is_never_rendered_as_a_rate() {
        // D5's human line, on the two devices the interval vocabulary's fourth and
        // degenerate answers exist for. `fps().unwrap_or(0.0)` made both of them read
        // "asked 30, got 0" — a rate nothing measured, printed as a measurement (note
        // **N199**).
        let asked = FrameInterval::Discrete {
            numerator: 1,
            denominator: 30,
        };
        for (got, expected) in [
            (FrameInterval::Unstated, "(not offered)"),
            (FrameInterval::Unknown { raw: 9 }, "(unreadable shape 0x9)"),
            (
                FrameInterval::Discrete {
                    numerator: 1,
                    denominator: 0,
                },
                "(1/0, not a rate)",
            ),
        ] {
            let text = adjustment_text(&Adjustment::Interval {
                requested: asked,
                negotiated: got,
            });
            assert_eq!(text, format!("asked 30, got {expected}"));
            assert!(!text.ends_with("got 0"), "{text}");
        }

        // The ordinary case still reads as a pair of rates, or the line above is a
        // rendering nobody would keep.
        assert_eq!(
            adjustment_text(&Adjustment::Interval {
                requested: asked,
                negotiated: FrameInterval::Discrete {
                    numerator: 1,
                    denominator: 15,
                },
            }),
            "asked 30, got 15"
        );
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
        // `TempDir::new_in` over the one scratch root rather than `tempfile::tempdir()`,
        // which reads `$TMPDIR` (the 2026-08-12 ruling, note N84). This crate is the thin
        // client's half and links no engine, so it reaches the root through
        // `schema::paths` — where the choice is made — instead of through
        // `engine::paths::scratch_dir`, which is the same root wearing a constructor.
        let root = schema::paths::scratch_root().expect("a scratch root");
        let dir = tempfile::TempDir::new_in(&root).expect("a scratch directory");
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
        // The path goes to stderr so `webcam-handler-cli profile capture -o f.json` prints
        // nothing a pipeline would have to strip.
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
    fn a_restore_that_gave_a_stranded_control_back_says_so_on_both_the_table_and_the_document() {
        // **Rubric A8, on the two surfaces the field was added for** (notes **N139** and
        // **N150**). A process killed between `begin_sweep` and its first sample leaves a
        // control every verb refuses and no snapshot describes; `calibrate restore` gives it
        // back, and `RestoreReport::freed` is where the answer says so. `engine::lifecycle`
        // proves the verb repairs the session — what neither surface had was a caller who
        // could tell: a run that changed a status on disk and then printed an empty table,
        // or `{"outcomes":[]}`, is telling an unattended caller that nothing happened, which
        // is the one thing that had not happened.
        let slug = |s: &str| ControlSlug::parse(s).expect("literal slug");
        let repaired = RestoreReport {
            outcomes: Vec::new(),
            freed: vec![slug("brightness"), slug("focus_absolute")],
        };

        let (mut out, _, stderr) = captured();
        restore(&repaired, false, &mut out).expect("rendering into a buffer cannot fail");
        let note = stderr.text();
        // Both controls by name, and the count, because "some controls" is not something an
        // operator can go and look at.
        for control in &repaired.freed {
            assert!(
                note.contains(control.as_str()),
                "the restore repaired {control} and told the operator nothing: {note:?}"
            );
        }
        assert!(note.contains('2'), "{note:?}");
        // And it is the *session* repair rather than a camera write: an outcome table this
        // report does not have must not be described as one.
        assert!(note.contains("mid-sweep"), "{note:?}");

        // The other direction, and the one that keeps the note meaning something: nothing
        // stranded says nothing at all.
        let (mut out, _, stderr) = captured();
        restore(
            &RestoreReport {
                outcomes: Vec::new(),
                freed: Vec::new(),
            },
            false,
            &mut out,
        )
        .expect("rendering into a buffer cannot fail");
        assert!(
            stderr.text().is_empty(),
            "a restore that found nothing stranded volunteered a note anyway: {}",
            stderr.text()
        );

        // `--json`, which is the surface the primary consumer reads: the same value, as the
        // document, with nothing computed on the side.
        let (mut out, stdout, stderr) = captured();
        restore(&repaired, true, &mut out).expect("rendering into a buffer cannot fail");
        let document = stdout.text();
        assert_eq!(
            serde_json::from_str::<RestoreReport>(&document).expect("valid JSON"),
            repaired
        );
        assert!(
            document.contains("freed") && document.contains("brightness"),
            "the answer an agent parses does not carry what the run repaired: {document}"
        );
        assert!(stderr.text().is_empty(), "{}", stderr.text());
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
    fn a_freshly_planned_session_says_a_sweep_is_next_and_not_that_nothing_is_queued() {
        // The state a user is in for exactly one command: `calibrate plan` has queued
        // something and `calibrate sweep` has not run, so the per-control *status* map is
        // empty while the *queue* is not. Rendering both empties with one sentence told them
        // to run the verb they had just run — found while writing the README's walkthrough
        // against the real binaries (note **N90**, owner's request 2).
        let mut planned = session_fixture();
        planned.controls.clear();

        let (mut out, stdout, _stderr) = captured();
        session(&planned, false, &mut out).expect("rendering into a buffer cannot fail");

        let text = stdout.text();
        assert!(
            !text.contains("no controls queued yet"),
            "a planned session was told its plan had not happened: {text}"
        );
        assert!(text.contains("calibrate sweep"), "{text}");
        // The queue itself, so the sentence is an answer and not just a different noise.
        for control in &planned.queue {
            assert!(text.contains(control.as_str()), "{control}: {text}");
        }
    }

    #[test]
    fn a_session_with_nothing_queued_at_all_still_points_at_plan() {
        // The other half, and the arm that keeps the original sentence reachable: a session
        // straight out of `calibrate start` has an empty queue *and* an empty status map, and
        // for that one `calibrate plan` really is the next verb.
        let mut fresh = session_fixture();
        fresh.controls.clear();
        fresh.queue.clear();

        let (mut out, stdout, _stderr) = captured();
        session(&fresh, false, &mut out).expect("rendering into a buffer cannot fail");

        let text = stdout.text();
        assert!(text.contains("no controls queued yet"), "{text}");
        assert!(text.contains("calibrate plan"), "{text}");
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
                    failure: Some(schema::ErrorKind::DeviceGone),
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
        // **The registry's own serde spelling, and only that one** (note **N149**). This is
        // the line an agent branches on, and the `--json` view of the same event carries
        // `"device_gone"`; printing Rust's `Debug` here put two spellings of one value on
        // the two surfaces whose whole job is to agree.
        assert!(
            note.trim_end().ends_with("(device_gone)"),
            "the discriminant is missing, or spelled some way other than the registry's own: \
             {note}"
        );
        assert!(
            !note.contains("DeviceGone"),
            "the human line spells the discriminant one way and the document another: {note}"
        );

        // And `--json` carries the whole history, so the human rendering computes nothing
        // a reader of the document could not.
        let (mut out, stdout, _) = captured();
        status(&stopped, true, &mut out).expect("rendering into a buffer cannot fail");
        assert_eq!(
            serde_json::from_str::<SessionStatus>(&stdout.text()).expect("valid JSON"),
            stopped
        );

        // **The line whose writer could not name a reason** (note **N149**). The second
        // producer of this event is `engine::lifecycle::free_stranded_sweeps`, running in a
        // later process over a control a killed sweep left behind: the process that knew why
        // died without writing it down, so `failure` is absent rather than plausible. The
        // rendering has to be absent too — a parenthesis reading "(none)" or "()" is a shape
        // an agent dispatching on the discriminant would read as one.
        let unexplained = SessionStatus {
            session: session_fixture(),
            log: vec![schema::session::LogEntry {
                at: schema::time::Stamp::epoch(),
                event: SessionEvent::SweepInterrupted {
                    control: ControlSlug::parse("focus_absolute").expect("literal slug"),
                    taken: 0,
                    total: 5,
                    failure: None,
                    detail: "no samples were taken; gave the control back".to_owned(),
                },
            }],
        };
        let (mut out, _, stderr) = captured();
        status(&unexplained, false, &mut out).expect("rendering into a buffer cannot fail");
        let note = stderr.text();
        // The story it does have, in words, since there is no discriminant to carry it —
        // and nothing after it, which is what "prints nothing at all where the field is
        // absent" has to mean on a line that ends in the detail.
        assert!(
            note.trim_end().ends_with("gave the control back"),
            "an absent reason was rendered as something after the detail, and anything there \
             is a shape a reader takes for a kind: {note}"
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
                precision: 64,
                adjustments: Vec::new(),
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

        // **The running sweep's precision cell, both branches** (note **N145**). The stride
        // is the number a caller compares against the `--precision` it typed, and it is the
        // one fact `total` alone cannot be turned into — so a table that dropped it would
        // leave an agent believing a resolution it never got.
        let sweeping = status_cells(&statuses[2]);
        assert!(
            sweeping.precision.contains("64"),
            "the stride the planner arrived at is not on the row that announces the sweep: {}",
            sweeping.precision
        );
        assert!(
            sweeping.precision.contains("planned"),
            "the stride a sweep has not finished walking is the planned one, not a measured \
             one, and the cell has to say which: {}",
            sweeping.precision
        );

        // And zero, which is a *different* sentence here from the one it is two arms down.
        // `engine::sweep::precision_of` falls back to the descriptor's `effective_step` and
        // that is never below 1, so no sweep this build plans carries a zero: one on the
        // document came from a build that had no such field. A dash would say "this sweep
        // has no spacing", and "(single sample)" would say something the document does not
        // support — either is note N149's collapse of an absent value into a plausible one.
        let older = status_cells(&ControlStatus::Sweeping {
            plan: SweepSpec::All,
            done: 3,
            total: 16,
            precision: 0,
            adjustments: Vec::new(),
        });
        // The dash comes from the table's own vocabulary rather than typed here: it is
        // whatever a status with no precision at all renders as.
        assert_ne!(
            older.precision,
            status_cells(&ControlStatus::Untouched).precision,
            "a stride nobody recorded reads as a sweep with no stride"
        );
        assert_ne!(
            older.precision, single.precision,
            "a document with no stride recorded reads as a control calibrated from one sample"
        );

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
                precision: 4,
                adjustments: Vec::new(),
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
    fn a_sweep_the_planner_trimmed_says_so_on_the_line_that_announces_it() {
        // Note **N145**. `SweepAdjustment` was built for every sweep and read by nobody: an
        // agent that asked for a stride of 1 across a 10 000-wide range was told "251
        // sample(s)" and left believing its precision request had been honoured. Both
        // directions, because a line that always mentioned an adjustment would be as
        // useless as one that never did.
        //
        // **The walk is over `SweepAdjustmentKind::ALL`, not over an array typed here**
        // (note **N148**). This test claimed to prove "a fifth kind cannot acquire a spelling
        // nobody looked at" while iterating a hand-written four-element array, which is
        // exactly the hand list rubric rule 6 bans — a fifth variant would have been caught
        // by the compiler's `match` in `sweep_adjustment_line` and by nothing here. The
        // `match` below is what makes the claim true: a fifth kind is a fifth member of `ALL`
        // and a missing arm right here.
        use schema::session::{SweepAdjustmentKind, SweepSpec};

        let slug = |s: &str| ControlSlug::parse(s).expect("literal slug");
        let started =
            |precision: i64, adjustments: Vec<SweepAdjustment>| CalibrationProgress::SweepStarted {
                control: slug("focus_absolute"),
                plan: SweepSpec::All,
                total: 251,
                precision,
                adjustments,
            };

        let plain = progress_line(&started(0, Vec::new()));
        assert_eq!(plain, "sweeping focus_absolute: 251 sample(s)");

        // The stride on its own is news even when nothing was adjusted: a caller who asked
        // for every value and got every fortieth learns it here.
        let strided = progress_line(&started(40, Vec::new()));
        assert!(strided.contains("every 40"), "{strided}");

        let trimmed = progress_line(&started(
            40,
            vec![SweepAdjustment::Capped {
                requested: 10_001,
                planned: 251,
                limit: 256,
                cap: SampleCap::Total,
            }],
        ));
        assert!(trimmed.contains("10001"), "{trimmed}");
        assert!(trimmed.contains("251"), "{trimmed}");
        assert!(
            trimmed.len() > strided.len(),
            "the adjustment left no trace on the line: {trimmed}"
        );

        let example = |kind: SweepAdjustmentKind| match kind {
            SweepAdjustmentKind::Clamped => SweepAdjustment::Clamped {
                requested: -5,
                planned: 0,
            },
            SweepAdjustmentKind::StepAligned => SweepAdjustment::StepAligned {
                requested: 7,
                planned: 8,
            },
            SweepAdjustmentKind::Deduplicated => SweepAdjustment::Deduplicated { dropped: 2 },
            SweepAdjustmentKind::Capped => SweepAdjustment::Capped {
                requested: 400,
                planned: 32,
                limit: 32,
                cap: SampleCap::Motion,
            },
        };
        let mut seen = std::collections::BTreeSet::new();
        for kind in SweepAdjustmentKind::ALL {
            let adjustment = example(*kind);
            // The example really is of that kind — otherwise a `match` arm that returned the
            // wrong variant would leave one kind unwalked and one walked twice, and the
            // `seen` check below would report a duplicate spelling instead of the real fault.
            assert_eq!(adjustment.kind(), *kind);
            assert!(
                seen.insert(sweep_adjustment_line(&adjustment)),
                "{adjustment:?} duplicates a spelling"
            );
        }
        assert_eq!(
            seen.len(),
            SweepAdjustmentKind::ALL.len(),
            "the walk did not reach every kind"
        );
        // The motion cap says *why*, because §5's reason is the operator's business: a
        // sweep trimmed for wear and one trimmed for size are not the same news.
        let motion = sweep_adjustment_line(&example(SweepAdjustmentKind::Capped));
        assert!(motion.contains("motors"), "{motion}");
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
