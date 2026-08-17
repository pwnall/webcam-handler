//! The typed ioctl calls.
//!
//! Each function fills a zeroed [`Payload`], hands the kernel a pointer to it, and returns
//! the reply's bytes for [`super::decode`] to interpret. No function here decides what an
//! answer *means*: the split is what keeps the interpretation reachable by Miri, which
//! cannot cross an ioctl.
//!
//! Every call site distinguishes the three answers an enumeration ioctl can give —
//! success, `EINVAL` meaning "no such index, stop asking", and everything else meaning a
//! real failure. Folding the middle one into either neighbour is how a sparse menu
//! becomes a dense one \[PF:2\] or a short format list becomes an error.

use std::mem::offset_of;
use std::os::raw::c_int;

use schema::error::{Error, Result};
use v4l::v4l_sys::{
    v4l2_buffer, v4l2_capability, v4l2_captureparm, v4l2_ext_control, v4l2_ext_controls,
    v4l2_fmtdesc, v4l2_format, v4l2_fract, v4l2_frmivalenum, v4l2_frmsizeenum, v4l2_pix_format,
    v4l2_query_ext_ctrl, v4l2_querymenu, v4l2_requestbuffers, v4l2_streamparm,
};
use v4l::v4l2::vidioc;

use super::{Fd, Payload, decode, fields};

/// `V4L2_BUF_TYPE_VIDEO_CAPTURE`.
const BUF_TYPE_VIDEO_CAPTURE: u32 = 1;
/// `V4L2_CTRL_FLAG_NEXT_CTRL` — walk to the next control the device actually has.
const CTRL_FLAG_NEXT_CTRL: u32 = 0x8000_0000;
/// `V4L2_CTRL_FLAG_NEXT_COMPOUND` — and do not skip the compound ones \[PF:1\].
const CTRL_FLAG_NEXT_COMPOUND: u32 = 0x4000_0000;
/// `V4L2_CTRL_WHICH_CUR_VAL`.
const CTRL_WHICH_CUR_VAL: u32 = 0;
/// `V4L2_CTRL_FLAG_HAS_PAYLOAD`.
pub(crate) const CTRL_FLAG_HAS_PAYLOAD: u32 = 0x0100;

/// What an enumeration ioctl said.
///
/// `Exhausted` covers the kernel's two ways of saying "there is nothing more here", both
/// of which are *terminators* rather than failures — V4L2 has no count-first call, so
/// every enumeration ends in an error code:
///
/// - **`EINVAL`** — "no entry at that index". The ordinary end of a list.
/// - **`ENOTTY`** — "this node does not implement this ioctl at all". Measured \[PF:15\]:
///   every metadata node on the seed hardware answers `ENOTTY` to `QUERY_EXT_CTRL` while
///   answering `EINVAL` to `ENUM_FMT`, so a build that accepted only `EINVAL` would report
///   a metadata-only camera's control set as a device error instead of as empty.
#[derive(Debug)]
pub(crate) enum Enumerated<T> {
    /// There was an entry.
    Entry(T),
    /// There was not, and there will not be a later one either.
    Exhausted,
}

/// `VIDIOC_QUERYCAP` — what this node is.
pub(crate) fn querycap(fd: &Fd) -> Result<decode::Capability> {
    let mut payload = Payload::<v4l2_capability>::zeroed();
    call(fd, vidioc::VIDIOC_QUERYCAP, &mut payload, "VIDIOC_QUERYCAP")?;
    decode::capability(payload.bytes()).ok_or_else(|| short_reply("VIDIOC_QUERYCAP"))
}

/// One control from the `VIDIOC_QUERY_EXT_CTRL` walk.
#[derive(Debug)]
pub(crate) struct WalkedControl {
    /// The id the kernel returned. The walk continues from it whether or not the
    /// descriptor could be built, so one control can never hide the ones behind it.
    pub(crate) id: u32,
    /// The descriptor, or `None` for a control whose name slugs to nothing.
    ///
    /// D2 will not invent a handle — an invented one collides silently — and there is no
    /// evidence any device has such a control, so this build reports what it can name
    /// rather than designing a placeholder for a case nobody has seen. If one ever
    /// appears it is a PF finding, and the finding will carry the evidence to design
    /// against.
    pub(crate) desc: Option<schema::control::ControlDesc>,
}

/// One step of the `VIDIOC_QUERY_EXT_CTRL` walk.
///
/// `previous` is the id last returned (0 to start). The `NEXT_CTRL | NEXT_COMPOUND` flags
/// make the kernel hand back the next control it has rather than requiring us to guess
/// ids — and `NEXT_COMPOUND` is what keeps the PF:1 control in the walk instead of
/// silently outside it.
pub(crate) fn query_ext_ctrl(fd: &Fd, previous: u32) -> Result<Enumerated<WalkedControl>> {
    let op = "VIDIOC_QUERY_EXT_CTRL";
    let mut payload = Payload::<v4l2_query_ext_ctrl>::zeroed();
    set_u32(
        &mut payload,
        offset_of!(v4l2_query_ext_ctrl, id),
        previous | CTRL_FLAG_NEXT_CTRL | CTRL_FLAG_NEXT_COMPOUND,
        op,
    )?;
    match call_enumerating(fd, vidioc::VIDIOC_QUERY_EXT_CTRL, &mut payload, op)? {
        Enumerated::Exhausted => Ok(Enumerated::Exhausted),
        Enumerated::Entry(()) => {
            let id = fields::read_u32(payload.bytes(), offset_of!(v4l2_query_ext_ctrl, id))
                .ok_or_else(|| short_reply(op))?;
            Ok(Enumerated::Entry(WalkedControl {
                id,
                desc: decode::control_desc(payload.bytes()),
            }))
        }
    }
}

/// `VIDIOC_QUERYMENU` for one index.
///
/// `Exhausted` here means *this index* is a hole, not that the menu has ended: menus are
/// sparse and the caller keeps walking \[PF:2\].
pub(crate) fn querymenu(
    fd: &Fd,
    control_id: u32,
    index: u32,
    control_type: u32,
) -> Result<Enumerated<(u32, schema::control::MenuItem)>> {
    let mut payload = Payload::<v4l2_querymenu>::zeroed();
    set_u32(
        &mut payload,
        offset_of!(v4l2_querymenu, id),
        control_id,
        "VIDIOC_QUERYMENU",
    )?;
    set_u32(
        &mut payload,
        offset_of!(v4l2_querymenu, index),
        index,
        "VIDIOC_QUERYMENU",
    )?;
    match call_enumerating(
        fd,
        vidioc::VIDIOC_QUERYMENU,
        &mut payload,
        "VIDIOC_QUERYMENU",
    )? {
        Enumerated::Exhausted => Ok(Enumerated::Exhausted),
        Enumerated::Entry(()) => decode::menu_item(payload.bytes(), control_type)
            .map(Enumerated::Entry)
            .ok_or_else(|| short_reply("VIDIOC_QUERYMENU")),
    }
}

/// `VIDIOC_ENUM_FMT` for one index on the capture queue.
pub(crate) fn enum_fmt(
    fd: &Fd,
    index: u32,
) -> Result<Enumerated<(schema::camera::PixelFormat, String, u32)>> {
    let mut payload = Payload::<v4l2_fmtdesc>::zeroed();
    set_u32(
        &mut payload,
        offset_of!(v4l2_fmtdesc, index),
        index,
        "VIDIOC_ENUM_FMT",
    )?;
    set_u32(
        &mut payload,
        offset_of!(v4l2_fmtdesc, type_),
        BUF_TYPE_VIDEO_CAPTURE,
        "VIDIOC_ENUM_FMT",
    )?;
    match call_enumerating(fd, vidioc::VIDIOC_ENUM_FMT, &mut payload, "VIDIOC_ENUM_FMT")? {
        Enumerated::Exhausted => Ok(Enumerated::Exhausted),
        Enumerated::Entry(()) => decode::format_desc(payload.bytes())
            .map(Enumerated::Entry)
            .ok_or_else(|| short_reply("VIDIOC_ENUM_FMT")),
    }
}

/// `VIDIOC_ENUM_FRAMESIZES` for one index of one pixel format.
///
/// A size whose `type` this build cannot interpret comes back as
/// `FrameSize::Unknown` rather than as an absence, so nothing the driver listed is lost.
pub(crate) fn enum_framesizes(
    fd: &Fd,
    pixel_format: u32,
    index: u32,
) -> Result<Enumerated<schema::camera::FrameSize>> {
    let mut payload = Payload::<v4l2_frmsizeenum>::zeroed();
    set_u32(
        &mut payload,
        offset_of!(v4l2_frmsizeenum, index),
        index,
        "VIDIOC_ENUM_FRAMESIZES",
    )?;
    set_u32(
        &mut payload,
        offset_of!(v4l2_frmsizeenum, pixel_format),
        pixel_format,
        "VIDIOC_ENUM_FRAMESIZES",
    )?;
    match call_enumerating(
        fd,
        vidioc::VIDIOC_ENUM_FRAMESIZES,
        &mut payload,
        "VIDIOC_ENUM_FRAMESIZES",
    )? {
        Enumerated::Exhausted => Ok(Enumerated::Exhausted),
        Enumerated::Entry(()) => decode::frame_size(payload.bytes())
            .map(Enumerated::Entry)
            .ok_or_else(|| short_reply("VIDIOC_ENUM_FRAMESIZES")),
    }
}

/// `VIDIOC_ENUM_FRAMEINTERVALS` for one index at one size, per PF:9's nesting.
pub(crate) fn enum_frameintervals(
    fd: &Fd,
    pixel_format: u32,
    width: u32,
    height: u32,
    index: u32,
) -> Result<Enumerated<schema::camera::FrameInterval>> {
    let mut payload = Payload::<v4l2_frmivalenum>::zeroed();
    let op = "VIDIOC_ENUM_FRAMEINTERVALS";
    set_u32(&mut payload, offset_of!(v4l2_frmivalenum, index), index, op)?;
    set_u32(
        &mut payload,
        offset_of!(v4l2_frmivalenum, pixel_format),
        pixel_format,
        op,
    )?;
    set_u32(&mut payload, offset_of!(v4l2_frmivalenum, width), width, op)?;
    set_u32(
        &mut payload,
        offset_of!(v4l2_frmivalenum, height),
        height,
        op,
    )?;
    match call_enumerating(fd, vidioc::VIDIOC_ENUM_FRAMEINTERVALS, &mut payload, op)? {
        Enumerated::Exhausted => Ok(Enumerated::Exhausted),
        Enumerated::Entry(()) => decode::frame_interval(payload.bytes())
            .map(Enumerated::Entry)
            .ok_or_else(|| short_reply(op)),
    }
}

/// `VIDIOC_G_EXT_CTRLS` for one scalar control.
pub(crate) fn get_scalar(
    fd: &Fd,
    control_id: u32,
    control_type: u32,
) -> Result<schema::control::ControlValue> {
    let op = "VIDIOC_G_EXT_CTRLS";
    let mut control = Payload::<v4l2_ext_control>::zeroed();
    set_u32(
        &mut control,
        offset_of!(v4l2_ext_control, id),
        control_id,
        op,
    )?;

    let mut controls = ext_controls_header(&mut control, op)?;
    call_ext_ctrls(fd, vidioc::VIDIOC_G_EXT_CTRLS, &mut controls, op)?;

    decode::control_scalar(control.bytes(), control_type).ok_or_else(|| short_reply(op))
}

/// `VIDIOC_S_EXT_CTRLS` for one scalar control.
///
/// The value goes into the union arm the control's *type* selects, which is the same
/// choice [`decode::control_scalar`] makes when reading: 64 bits for
/// `V4L2_CTRL_TYPE_INTEGER64`, 32 for everything else. Getting that backwards writes four
/// bytes of a neighbouring field and the kernel reports success.
pub(crate) fn set_scalar(fd: &Fd, control_id: u32, control_type: u32, value: i64) -> Result<()> {
    let op = "VIDIOC_S_EXT_CTRLS";
    let mut control = Payload::<v4l2_ext_control>::zeroed();
    set_u32(
        &mut control,
        offset_of!(v4l2_ext_control, id),
        control_id,
        op,
    )?;

    let union_at = offset_of!(v4l2_ext_control, __bindgen_anon_1);
    let arm = decode::scalar_arm(control_type, value).ok_or_else(|| Error::DeviceIo {
        operation: op.to_owned(),
        errno: None,
        message: format!(
            "{value} does not fit the 32-bit value field this control's type declares"
        ),
    })?;
    match arm {
        decode::ScalarArm::Wide(wide) => fields::write_i64(control.bytes_mut(), union_at, wide),
        decode::ScalarArm::Narrow(narrow) => {
            fields::write_i32(control.bytes_mut(), union_at, narrow)
        }
    }
    .ok_or_else(|| short_reply(op))?;

    let mut controls = ext_controls_header(&mut control, op)?;
    call_ext_ctrls(fd, vidioc::VIDIOC_S_EXT_CTRLS, &mut controls, op)
}

/// `VIDIOC_S_EXT_CTRLS` for one compound control, from a caller-supplied buffer.
///
/// The buffer is the caller's — it came from a [`schema::control::ControlValue::Bytes`],
/// which for this backend can only have come from a previous `get_payload` — and its
/// length is what goes in `size`. A payload of the wrong length is the device's to refuse.
pub(crate) fn set_payload(fd: &Fd, control_id: u32, bytes: &[u8]) -> Result<()> {
    let op = "VIDIOC_S_EXT_CTRLS";
    if bytes.is_empty() {
        return Err(short_reply(op));
    }
    let size = u32::try_from(bytes.len()).map_err(|_| Error::DeviceIo {
        operation: op.to_owned(),
        errno: None,
        message: format!("a payload of {} bytes does not fit a u32 size", bytes.len()),
    })?;
    // Copied rather than pointed at: the kernel's `controls` field is `*mut`, and handing
    // it the address of a `&[u8]` would be lending it a write capability over memory the
    // caller still owns and believes is immutable.
    let mut buffer = bytes.to_vec();

    let mut control = Payload::<v4l2_ext_control>::zeroed();
    set_u32(
        &mut control,
        offset_of!(v4l2_ext_control, id),
        control_id,
        op,
    )?;
    set_u32(&mut control, offset_of!(v4l2_ext_control, size), size, op)?;
    fields::write_usize(
        control.bytes_mut(),
        offset_of!(v4l2_ext_control, __bindgen_anon_1),
        buffer.as_mut_ptr().expose_provenance(),
    )
    .ok_or_else(|| short_reply(op))?;

    let mut controls = ext_controls_header(&mut control, op)?;
    call_ext_ctrls(fd, vidioc::VIDIOC_S_EXT_CTRLS, &mut controls, op)
}

/// `VIDIOC_G_EXT_CTRLS` for one compound control, into a caller-sized buffer.
///
/// `len` must already have passed [`decode::payload_len`], which is where the
/// device-supplied `elem_size × elems` is bounded (rubric B10). This function refuses a
/// zero length rather than handing the kernel an empty buffer with a live pointer.
pub(crate) fn get_payload(fd: &Fd, control_id: u32, len: usize) -> Result<Vec<u8>> {
    let op = "VIDIOC_G_EXT_CTRLS";
    if len == 0 {
        return Err(short_reply(op));
    }
    let mut buffer = vec![0u8; len];
    let size = u32::try_from(len).map_err(|_| short_reply(op))?;

    let mut control = Payload::<v4l2_ext_control>::zeroed();
    set_u32(
        &mut control,
        offset_of!(v4l2_ext_control, id),
        control_id,
        op,
    )?;
    set_u32(&mut control, offset_of!(v4l2_ext_control, size), size, op)?;
    fields::write_usize(
        control.bytes_mut(),
        offset_of!(v4l2_ext_control, __bindgen_anon_1),
        buffer.as_mut_ptr().expose_provenance(),
    )
    .ok_or_else(|| short_reply(op))?;

    let mut controls = ext_controls_header(&mut control, op)?;
    call_ext_ctrls(fd, vidioc::VIDIOC_G_EXT_CTRLS, &mut controls, op)?;

    Ok(buffer)
}

/// Run one of the four `*_EXT_CTRLS` calls over a prepared one-entry header.
///
/// One function, and therefore one `unsafe` block, for reads and writes alike: the
/// obligation is identical in both directions — the same header shape, the same pointer
/// graph, the same liveness — and stating it four times would be four chances to state it
/// differently. The *caller* owns the parts that do differ, and each of them is checked in
/// safe code before this is reached: `size` is bounded by `decode::payload_len` on the way
/// in and by `u32::try_from` on the way out, and the union arm is chosen by control type.
fn call_ext_ctrls(
    fd: &Fd,
    request: vidioc::_IOC_TYPE,
    controls: &mut Payload<v4l2_ext_controls>,
    op: &str,
) -> Result<()> {
    // SAFETY: `controls` is a live, exclusively borrowed `v4l2_ext_controls`, correctly
    // aligned and valid for `size_of::<v4l2_ext_controls>()` writable bytes — the width
    // the `_IOWR`-encoded `request` declares. Its `count` is 1 and its `controls` field
    // holds the address of the caller's `v4l2_ext_control`, which outlives this call: the
    // caller holds that binding across it, and the `&mut` borrow that planted the address
    // has ended, so the kernel is the only accessor. When that entry carries a payload
    // pointer its `size` is the length of the live allocation it points at (bounded
    // non-zero before it became one), so the kernel touches at most that many bytes; when
    // it does not, `size` is 0 and the union holds an inline scalar the kernel
    // dereferences nothing from.
    let ret = unsafe { v4l::v4l2::ioctl(fd.raw(), request, controls.as_mut_ptr()) };
    // `control_error`, not `device_error`: this call is about one control, and an `EBUSY`
    // from it is the device function's, not the node's (note **N197**).
    ret.map_err(|error| control_error(fd, op, &error))
}

/// The one-entry `v4l2_ext_controls` header pointing at `control`.
fn ext_controls_header(
    control: &mut Payload<v4l2_ext_control>,
    op: &str,
) -> Result<Payload<v4l2_ext_controls>> {
    let mut controls = Payload::<v4l2_ext_controls>::zeroed();
    // The first union member is `ctrl_class`/`which`; both name the same word, and
    // CUR_VAL is zero — set explicitly rather than relying on the zeroing, because a
    // reader should not have to know that.
    set_u32(
        &mut controls,
        offset_of!(v4l2_ext_controls, __bindgen_anon_1),
        CTRL_WHICH_CUR_VAL,
        op,
    )?;
    set_u32(&mut controls, offset_of!(v4l2_ext_controls, count), 1, op)?;
    fields::write_usize(
        controls.bytes_mut(),
        offset_of!(v4l2_ext_controls, controls),
        control.as_mut_ptr().expose_provenance(),
    )
    .ok_or_else(|| short_reply(op))?;
    Ok(controls)
}

// ------------------------------------------------------------------ the streaming path

/// `V4L2_MEMORY_MMAP` — the driver allocates, we map (design §2.5).
const MEMORY_MMAP: u32 = 1;

/// `VIDIOC_S_FMT` on the capture queue, reading back what the driver settled on.
///
/// The read-back is not a courtesy: `S_FMT` adjusts the caller's request in place and
/// returns success, so the struct that comes back *is* the negotiated format. A caller
/// that ignored it would report the size it asked for rather than the size it will get,
/// which is D3's mistake wearing D5's clothes.
pub(crate) fn set_format(
    fd: &Fd,
    pixel_format: u32,
    width: u32,
    height: u32,
) -> Result<decode::PixFormat> {
    let op = "VIDIOC_S_FMT";
    let mut payload = Payload::<v4l2_format>::zeroed();
    set_u32(
        &mut payload,
        offset_of!(v4l2_format, type_),
        BUF_TYPE_VIDEO_CAPTURE,
        op,
    )?;
    for (field, value) in [
        (offset_of!(v4l2_pix_format, pixelformat), pixel_format),
        (offset_of!(v4l2_pix_format, width), width),
        (offset_of!(v4l2_pix_format, height), height),
    ] {
        let at = decode::pix_field(field).ok_or_else(|| short_reply(op))?;
        set_u32(&mut payload, at, value, op)?;
    }
    // Everything else stays zero, which is what the UAPI asks for: `field` zero is
    // `V4L2_FIELD_ANY`, and a zeroed `bytesperline`/`sizeimage` tells the driver to
    // compute them.
    call(fd, vidioc::VIDIOC_S_FMT, &mut payload, op)?;
    decode::pix_format(payload.bytes()).ok_or_else(|| short_reply(op))
}

/// `VIDIOC_S_PARM`, asking for a frame interval and reading back what was granted.
///
/// A driver with no `V4L2_CAP_TIMEPERFRAME` answers `None` rather than an error: it has
/// said its interval field means nothing, which is a fact about the device and not a
/// failure of the call (E3).
pub(crate) fn set_interval(
    fd: &Fd,
    numerator: u32,
    denominator: u32,
) -> Result<Option<schema::camera::FrameInterval>> {
    let op = "VIDIOC_S_PARM";
    let mut payload = Payload::<v4l2_streamparm>::zeroed();
    set_u32(
        &mut payload,
        offset_of!(v4l2_streamparm, type_),
        BUF_TYPE_VIDEO_CAPTURE,
        op,
    )?;
    for (field, value) in [
        (
            offset_of!(v4l2_captureparm, timeperframe) + offset_of!(v4l2_fract, numerator),
            numerator,
        ),
        (
            offset_of!(v4l2_captureparm, timeperframe) + offset_of!(v4l2_fract, denominator),
            denominator,
        ),
    ] {
        let at = decode::capture_parm_field(field).ok_or_else(|| short_reply(op))?;
        set_u32(&mut payload, at, value, op)?;
    }
    call(fd, vidioc::VIDIOC_S_PARM, &mut payload, op)?;
    reported_interval(payload.bytes(), op)
}

/// `VIDIOC_G_PARM` — the interval the device is running at, when it will say.
pub(crate) fn get_interval(fd: &Fd) -> Result<Option<schema::camera::FrameInterval>> {
    let op = "VIDIOC_G_PARM";
    let mut payload = Payload::<v4l2_streamparm>::zeroed();
    set_u32(
        &mut payload,
        offset_of!(v4l2_streamparm, type_),
        BUF_TYPE_VIDEO_CAPTURE,
        op,
    )?;
    call(fd, vidioc::VIDIOC_G_PARM, &mut payload, op)?;
    reported_interval(payload.bytes(), op)
}

/// A `G_PARM`/`S_PARM` reply's interval, with the three answers kept apart.
///
/// `Err` is a reply too short to read — this build's problem, reported as one. `Ok(None)`
/// is the driver saying it does not negotiate an interval on this node. `Ok(Some(_))` is
/// the fraction the driver wrote, degenerate or not: a `1/0` is the *device's* answer and
/// carrying it is D2, where turning it into "the device offered nothing" was an invented
/// claim about the capability bit (note **N199**).
fn reported_interval(bytes: &[u8], op: &str) -> Result<Option<schema::camera::FrameInterval>> {
    match decode::capture_interval(bytes).ok_or_else(|| short_reply(op))? {
        decode::ReportedInterval::NotOffered => Ok(None),
        decode::ReportedInterval::Offered(interval) => Ok(Some(interval)),
    }
}

/// `VIDIOC_REQBUFS` — ask the driver for `count` mmap buffers, and learn how many it gave.
///
/// `count` of zero is the documented way to release every buffer, and this function is
/// how a stream is torn down as well as how it is set up.
pub(crate) fn request_buffers(fd: &Fd, count: u32) -> Result<u32> {
    let op = "VIDIOC_REQBUFS";
    let mut payload = Payload::<v4l2_requestbuffers>::zeroed();
    set_u32(
        &mut payload,
        offset_of!(v4l2_requestbuffers, count),
        count,
        op,
    )?;
    set_u32(
        &mut payload,
        offset_of!(v4l2_requestbuffers, type_),
        BUF_TYPE_VIDEO_CAPTURE,
        op,
    )?;
    set_u32(
        &mut payload,
        offset_of!(v4l2_requestbuffers, memory),
        MEMORY_MMAP,
        op,
    )?;
    call(fd, vidioc::VIDIOC_REQBUFS, &mut payload, op)?;
    decode::granted_buffers(payload.bytes()).ok_or_else(|| short_reply(op))
}

/// `VIDIOC_QUERYBUF` — where buffer `index` lives, so it can be mapped.
pub(crate) fn query_buffer(fd: &Fd, index: u32) -> Result<decode::BufferMapping> {
    let op = "VIDIOC_QUERYBUF";
    let mut payload = buffer_request(index, op)?;
    call(fd, vidioc::VIDIOC_QUERYBUF, &mut payload, op)?;
    decode::buffer_mapping(payload.bytes()).ok_or_else(|| short_reply(op))
}

/// `VIDIOC_QBUF` — hand buffer `index` back to the driver to fill.
pub(crate) fn queue_buffer(fd: &Fd, index: u32) -> Result<()> {
    let op = "VIDIOC_QBUF";
    let mut payload = buffer_request(index, op)?;
    call(fd, vidioc::VIDIOC_QBUF, &mut payload, op)
}

/// `VIDIOC_DQBUF` — take the next filled buffer.
///
/// Blocking, because [`super::Fd::open`] does not set `O_NONBLOCK`; the caller bounds it
/// with [`super::wait::readable`] first, so this only ever runs when a buffer is ready.
pub(crate) fn dequeue_buffer(fd: &Fd) -> Result<decode::Dequeued> {
    let op = "VIDIOC_DQBUF";
    // The index is an *output* here — the driver says which buffer it filled — so the
    // request carries only the queue type and the memory model.
    let mut payload = buffer_request(0, op)?;
    call(fd, vidioc::VIDIOC_DQBUF, &mut payload, op)?;
    decode::dequeued(payload.bytes()).ok_or_else(|| short_reply(op))
}

/// `VIDIOC_STREAMON`.
pub(crate) fn stream_on(fd: &Fd) -> Result<()> {
    stream_switch(fd, vidioc::VIDIOC_STREAMON, "VIDIOC_STREAMON")
}

/// `VIDIOC_STREAMOFF`. Idempotent in the kernel, and therefore here.
pub(crate) fn stream_off(fd: &Fd) -> Result<()> {
    stream_switch(fd, vidioc::VIDIOC_STREAMOFF, "VIDIOC_STREAMOFF")
}

/// The two stream switches take a bare buffer-type `int`, not a struct.
fn stream_switch(fd: &Fd, request: vidioc::_IOC_TYPE, op: &str) -> Result<()> {
    let mut payload = Payload::<c_int>::zeroed();
    fields::write_u32(payload.bytes_mut(), 0, BUF_TYPE_VIDEO_CAPTURE)
        .ok_or_else(|| short_reply(op))?;
    call(fd, request, &mut payload, op)
}

/// A `v4l2_buffer` naming one index on the mmap capture queue.
fn buffer_request(index: u32, op: &str) -> Result<Payload<v4l2_buffer>> {
    let mut payload = Payload::<v4l2_buffer>::zeroed();
    set_u32(&mut payload, offset_of!(v4l2_buffer, index), index, op)?;
    set_u32(
        &mut payload,
        offset_of!(v4l2_buffer, type_),
        BUF_TYPE_VIDEO_CAPTURE,
        op,
    )?;
    set_u32(
        &mut payload,
        offset_of!(v4l2_buffer, memory),
        MEMORY_MMAP,
        op,
    )?;
    Ok(payload)
}

/// Run an ioctl whose only acceptable answer is success.
fn call<T: Copy>(
    fd: &Fd,
    request: vidioc::_IOC_TYPE,
    payload: &mut Payload<T>,
    op: &str,
) -> Result<()> {
    // SAFETY: `payload` is a live, exclusively borrowed `Payload<T>`; its pointer is
    // correctly aligned for `T` and valid for `size_of::<T>()` **readable and writable**
    // bytes, which covers every direction the `_IOC`-encoded `request` can declare —
    // `_IOR` (QUERYCAP), `_IOW` (STREAMON/STREAMOFF) and `_IOWR` (the rest) alike. Stating
    // one direction would be a safety claim narrower than the calls this function serves,
    // and rubric B10 counts a false safety claim as a defect even when the code works.
    //
    // Nothing is dereferenced *out* of that buffer, and that is a fact about the values in
    // it rather than about the ten structs it carries. One of them has a `__user` pointer:
    // `v4l2_buffer`'s `m.planes`, which `QUERYBUF`/`QBUF`/`DQBUF` bring here and which the
    // v4l2 core walks — `length` entries of it — for a *multi-planar* queue. This build
    // never asks for one. `buffer_request` starts from a zeroed payload and writes
    // `V4L2_BUF_TYPE_VIDEO_CAPTURE`, the single-planar queue, whose `m` the core reads
    // inline as `offset`/`userptr`/`fd`; on that queue `m.planes` is never followed.
    //
    // The zeroed `length` is the second half, and it is a *refusal* rather than a bound:
    // vb2 sizes a plane walk by `vb->num_planes`, which is the driver's number and not
    // ours, and what stands between a null `m.planes` and that walk is
    // `__verify_planes_array` answering `-EINVAL` when `b->m.planes` is null or
    // `b->length` is under `vb->num_planes` — before anything is dereferenced. Zero is
    // under every `num_planes` a driver can report, so the call is refused rather than
    // followed (note **N199** corrects the mechanism this comment first claimed).
    //
    // The obligation is therefore discharged by two values a test can read back rather
    // than by a sentence, which is what
    // `the_buffer_ioctls_never_hand_the_kernel_a_plane_pointer` is for — and by
    // `buffer_request` being the only thing in this module that builds one, which is what
    // `only_one_place_in_this_module_builds_the_buffer_the_safety_comment_is_about` holds.
    // The calls that plant a pointer *on purpose* go through `call_ext_ctrls`, which
    // discharges that separately.
    let ret = unsafe { v4l::v4l2::ioctl(fd.raw(), request, payload.as_mut_ptr()) };
    ret.map_err(|error| device_error(fd, op, &error))
}

/// Run an enumeration ioctl, reading `EINVAL` as the terminator it is.
fn call_enumerating<T: Copy>(
    fd: &Fd,
    request: vidioc::_IOC_TYPE,
    payload: &mut Payload<T>,
    op: &str,
) -> Result<Enumerated<()>> {
    // SAFETY: `payload` is a live, exclusively borrowed `Payload<T>`, correctly aligned
    // for `T` and valid for `size_of::<T>()` readable and writable bytes — the width every
    // `_IOWR`-encoded enumeration request here declares. The five structs it carries —
    // `v4l2_query_ext_ctrl`, `v4l2_querymenu`, `v4l2_fmtdesc`, `v4l2_frmsizeenum`,
    // `v4l2_frmivalenum` — are plain data the kernel fills in place, and none of them has
    // a `__user` member for the kernel to follow. That is a **different** discharge from
    // `call`'s: `call` carries `v4l2_buffer`, whose `m.planes` is a pointer, and its
    // obligation is about two values rather than about the shape of the struct. Saying
    // "identical to `call`" inherited a claim about a struct this function never carries
    // (note **N199**).
    //
    // The error handling below is safe code and is not part of this obligation.
    let ret = unsafe { v4l::v4l2::ioctl(fd.raw(), request, payload.as_mut_ptr()) };
    match ret {
        Ok(()) => Ok(Enumerated::Entry(())),
        Err(error) if matches!(error.raw_os_error(), Some(libc::EINVAL | libc::ENOTTY)) => {
            Ok(Enumerated::Exhausted)
        }
        Err(error) => Err(device_error(fd, op, &error)),
    }
}

fn set_u32<T: Copy>(payload: &mut Payload<T>, offset: usize, value: u32, op: &str) -> Result<()> {
    fields::write_u32(payload.bytes_mut(), offset, value).ok_or_else(|| Error::DeviceIo {
        operation: op.to_owned(),
        errno: None,
        message: format!(
            "the request field at offset {offset} does not fit in a {}-byte argument; the \
             kernel headers this build was compiled against disagree with the ones it is \
             running on",
            size_of::<T>()
        ),
    })
}

fn short_reply(op: &str) -> Error {
    Error::DeviceIo {
        operation: op.to_owned(),
        errno: None,
        message: "the kernel's reply was shorter than the bindings describe".to_owned(),
    }
}

/// Map an ioctl failure onto the D13 registry, keeping E3's distinctions.
///
/// **`EACCES` is deliberately not here.** On an fd we already hold, `EACCES` is never
/// "you may not use this device" — that answer arrived at `Fd::open` and is mapped there.
/// From an ioctl it is a fact about the *operation*: the UAPI's answer for reading a
/// write-only control, or writing a read-only one. Mapping it to
/// [`Error::PermissionDenied`] here would tell a user to join the `video` group because a
/// control had no readable value, and would make the caller's own `EACCES` handling
/// unreachable — which is exactly what it did before this comment existed.
/// **`EBUSY` names other processes, never this one.** Every ioctl here runs on a
/// descriptor this process is holding, so a raw `/proc` walk over `fd.path()` finds the
/// caller by construction — and D13's `holders` is what `terminate_holder` reads as *the
/// pids that would free this camera*, so naming ourselves invites a client to kill the
/// daemon it is talking to (note **N48** point 5, note **N197**).
fn device_error(fd: &Fd, op: &str, error: &std::io::Error) -> Error {
    classify(fd, op, error, || crate::holders::others_holding(fd.path()))
}

/// The same classification for an ioctl about **one control**, with no `/proc` walk at all.
///
/// `EBUSY` from `G_EXT_CTRLS`/`S_EXT_CTRLS` is not "somebody else has this node". The UAPI
/// documents it as the answer for a control whose *device function* another application has
/// taken over, on a node this process has open and is using — so the holder list is the
/// wrong question, and the only pid a walk over this descriptor could produce is our own
/// (note **N197**).
///
/// Not walking is also what makes the tolerant control enumeration affordable. `controls()`
/// carries a declined read rather than propagating it, so the refusal's holder list is
/// built and immediately discarded — on vivid's 77 controls that was up to 77 full
/// process-table walks thrown away, in the same change that removed a whole enumeration
/// from `describe` (note **N197**).
fn control_error(fd: &Fd, op: &str, error: &std::io::Error) -> Error {
    classify(fd, op, error, Vec::new)
}

/// The D13 mapping both of the above share, differing only in who an `EBUSY` names.
fn classify(
    fd: &Fd,
    op: &str,
    error: &std::io::Error,
    holders: impl FnOnce() -> Vec<schema::error::Holder>,
) -> Error {
    match error.raw_os_error() {
        Some(libc::EBUSY) => Error::Busy {
            holders: holders(),
            path: fd.path().to_owned(),
        },
        Some(libc::ENODEV | libc::ENXIO) => Error::DeviceGone {
            path: fd.path().to_owned(),
        },
        Some(libc::EPERM) => Error::PermissionDenied {
            path: fd.path().to_owned(),
            hint: "add yourself to the `video` group, then log out and back in".to_owned(),
        },
        errno => Error::DeviceIo {
            operation: op.to_owned(),
            errno,
            message: error.to_string(),
        },
    }
}

/// Whether a control's flag word says its value is a compound payload.
pub(crate) fn has_payload(flags: u32) -> bool {
    flags & CTRL_FLAG_HAS_PAYLOAD != 0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `V4L2_BUF_TYPE_VIDEO_CAPTURE_MPLANE` — the queue this build does not use, spelled
    /// here so the assertion below names what it is refusing rather than a number.
    const BUF_TYPE_VIDEO_CAPTURE_MPLANE: u32 = 9;

    /// A descriptor on a file only this process has open, in the scratch tree.
    ///
    /// `/dev/null` would not do: hundreds of processes hold it and
    /// `limits::MAX_HOLDERS_REPORTED` is 4, so the non-vacuity arm below would be a coin
    /// toss — the same reasoning `refusing_a_second_stream_names_nobody_rather_than_the_
    /// process_making_the_refusal` gives one crate up (note **N191**).
    fn a_node_only_this_process_holds(tag: &str) -> (camino::Utf8PathBuf, Fd) {
        let root = schema::paths::scratch_root().expect("a scratch root under target/");
        let path = root.join(format!("wch-{tag}-{}.node", std::process::id()));
        std::fs::File::create(path.as_std_path()).expect("a file this process holds");
        let fd = Fd::open(&path).expect("a descriptor on a file we just made");
        (path, fd)
    }

    #[test]
    fn only_one_place_in_this_module_builds_the_buffer_the_safety_comment_is_about() {
        // `call`'s SAFETY discharge is a claim about **every** producer of a
        // `Payload<v4l2_buffer>` in this module — the union is all-zero and `length` is
        // zero — and the assertion below it reads one builder's output. A second builder
        // added later would make the comment false with nothing to notice, because a
        // sound-looking zeroed `v4l2_buffer` payload two hundred lines away compiles
        // exactly as well (note **N199**).
        //
        // Reading this module's own source is the cheapest thing that can go red on that,
        // and it is honest about being a source check: it does not prove the second
        // builder would be wrong, it proves nobody added one without reading this test's
        // name.
        let source = include_str!("ioctl.rs");
        // Assembled rather than written, so this scan does not find *itself* — a needle
        // spelled whole here would be its own second match and the count would be a
        // constant 2 whatever the module did.
        let needle = concat!("Payload::<v4l2_", "buffer>::zeroed()");
        let built = source.matches(needle).count();
        assert_eq!(
            built, 1,
            "`call`'s SAFETY comment discharges its obligation on the values \
             `buffer_request` writes, so a second `v4l2_buffer` payload in this module \
             needs the comment and `the_buffer_ioctls_never_hand_the_kernel_a_plane_pointer` \
             extended to cover it"
        );
        assert!(
            source.contains("fn buffer_request("),
            "the one builder is `buffer_request`, and it has been renamed"
        );
    }

    #[test]
    fn a_refusal_about_a_descriptor_this_process_holds_does_not_name_this_process() {
        // N48 point 5, at the layer that was still getting it wrong after M5 repaired
        // `start_stream`: every ioctl in this module runs on an fd **this** process holds,
        // so a `/proc` walk over `fd.path()` finds the caller by construction and puts its
        // pid in the field `terminate_holder` reads as *the pids that would free this
        // camera* (note **N197**). `wch_set` refusing an auto-owned control answered
        // "held by webcam-handler-dae (pid …)" all the way through B7.
        let (path, fd) = a_node_only_this_process_holds("ioctl-busy");
        let busy = std::io::Error::from_raw_os_error(libc::EBUSY);

        // A node-level refusal — `S_FMT` on a streaming node, `STREAMON` on a taken one —
        // still walks, because there really can be another process to name. It just may
        // not name us.
        let Error::Busy { holders, path: at } = device_error(&fd, "VIDIOC_STREAMON", &busy) else {
            panic!("EBUSY is D13's `Busy`");
        };
        assert_eq!(at, path);
        assert!(
            holders.is_empty(),
            "a node-level refusal named the process making it: {holders:?}"
        );

        // A *control* refusal does not walk at all. `EBUSY` from `G_EXT_CTRLS`/`S_EXT_CTRLS`
        // is the UAPI's answer for a control whose device function another application has
        // taken over — it is not "somebody else has this node", and the only holder a walk
        // could find over this descriptor is us.
        let Error::Busy { holders, .. } = control_error(&fd, "VIDIOC_S_EXT_CTRLS", &busy) else {
            panic!("EBUSY is D13's `Busy`");
        };
        assert!(holders.is_empty(), "{holders:?}");

        // Non-vacuity, and the reason this test opens a file nobody else has: an empty
        // list is a decision only if the walk had something to say. `fd` is still alive
        // here on purpose — it is the descriptor the refusals above were made on.
        let walked = crate::holders::of(&path);
        let mine = i32::try_from(std::process::id()).expect("a pid fits in an i32");
        assert!(
            walked.iter().any(|holder| holder.pid == mine),
            "the /proc walk over a node this process holds did not name it, so the \
             assertions above would pass for the wrong reason: {walked:?}"
        );

        drop(fd);
        std::fs::remove_file(path.as_std_path()).expect("the scratch node is ours to remove");
    }

    #[test]
    fn a_control_refusal_keeps_every_distinction_a_node_refusal_does() {
        // The inverse of the arm above: `control_error` differs from `device_error` in
        // *one* respect — it does not walk `/proc` — and a copy that quietly collapsed
        // E3's other distinctions would be the defect rule 7 names, in the function
        // written to fix a different one.
        let (path, fd) = a_node_only_this_process_holds("ioctl-vocabulary");
        for (errno, kind) in [
            (libc::ENODEV, schema::ErrorKind::DeviceGone),
            (libc::ENXIO, schema::ErrorKind::DeviceGone),
            (libc::EPERM, schema::ErrorKind::PermissionDenied),
            (libc::EIO, schema::ErrorKind::DeviceIo),
            (libc::ETIMEDOUT, schema::ErrorKind::DeviceIo),
            (libc::EBUSY, schema::ErrorKind::Busy),
        ] {
            let error = std::io::Error::from_raw_os_error(errno);
            assert_eq!(
                control_error(&fd, "VIDIOC_G_EXT_CTRLS", &error).kind(),
                device_error(&fd, "VIDIOC_G_EXT_CTRLS", &error).kind(),
                "errno {errno} is classified differently by the two mappers"
            );
            assert_eq!(
                control_error(&fd, "VIDIOC_G_EXT_CTRLS", &error).kind(),
                kind
            );
        }

        // And `EACCES` is still deliberately absent from both: on an fd we already hold it
        // is a fact about the *operation*, and this module's own doc says why.
        let denied = std::io::Error::from_raw_os_error(libc::EACCES);
        assert_eq!(
            control_error(&fd, "VIDIOC_G_EXT_CTRLS", &denied).kind(),
            schema::ErrorKind::DeviceIo
        );

        drop(fd);
        std::fs::remove_file(path.as_std_path()).expect("the scratch node is ours to remove");
    }

    #[test]
    fn the_buffer_ioctls_never_hand_the_kernel_a_plane_pointer() {
        // `call`'s SAFETY comment says the kernel dereferences nothing out of the payload,
        // and `v4l2_buffer` — the struct `QUERYBUF`, `QBUF` and `DQBUF` carry through it —
        // is the one that could make that false: its `m` union holds `planes`, a `__user`
        // array the v4l2 core walks for a multi-planar queue. The comment used to discharge
        // the obligation by asserting the struct held no pointers, which is not the
        // obligation the struct has (note **N190**); it now discharges it on two values,
        // and these are those two values.
        //
        // One request builder serves all three ioctls, so one assertion covers all three.
        let payload = buffer_request(3, "VIDIOC_QUERYBUF").expect("a one-buffer request");
        let bytes = payload.bytes();

        // The queue. `V4L2_BUF_TYPE_VIDEO_CAPTURE` is where `m` is read inline; the
        // `_MPLANE` queue is where it is followed.
        let queue = fields::read_u32(bytes, offset_of!(v4l2_buffer, type_));
        assert_eq!(queue, Some(BUF_TYPE_VIDEO_CAPTURE));
        assert_ne!(queue, Some(BUF_TYPE_VIDEO_CAPTURE_MPLANE));

        // And the union, whole. `offset`, `userptr`, `planes` and `fd` are four readings
        // of the same bytes, and all-zero is the only value that is none of them — so a
        // plane pointer cannot be in there whatever the queue word said. `length` bounds
        // the walk the core would do, and it is zero for the same reason.
        let m_at = offset_of!(v4l2_buffer, m);
        let m = bytes
            .get(m_at..m_at.saturating_add(size_of::<v4l::v4l_sys::v4l2_buffer__bindgen_ty_1>()))
            .expect("the union lies inside the payload");
        assert!(m.iter().all(|byte| *byte == 0), "{m:?}");
        assert_eq!(
            fields::read_u32(bytes, offset_of!(v4l2_buffer, length)),
            Some(0)
        );

        // The index is the one field the caller chose, so the assertions above are about a
        // request that was actually filled in rather than about a zeroed buffer.
        assert_eq!(
            fields::read_u32(bytes, offset_of!(v4l2_buffer, index)),
            Some(3)
        );
    }
}
