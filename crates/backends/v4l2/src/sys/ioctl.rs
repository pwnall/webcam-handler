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

use schema::error::{Error, Result};
use v4l::v4l_sys::{
    v4l2_capability, v4l2_ext_control, v4l2_ext_controls, v4l2_fmtdesc, v4l2_frmivalenum,
    v4l2_frmsizeenum, v4l2_query_ext_ctrl, v4l2_querymenu,
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
    // SAFETY: `controls` holds a zeroed `v4l2_ext_controls` whose `count` is 1 and whose
    // `controls` field holds the address of `control`, a live, correctly aligned,
    // writable `v4l2_ext_control` — both bindings are in scope for the whole call, and
    // the `&mut` borrow that planted the address has ended, so nothing aliases it while
    // the kernel writes. `control` carries no payload pointer (`size` is 0 and the union
    // holds an inline scalar), so the kernel dereferences nothing beyond that one entry.
    // The pointer passed is to `controls` itself, valid for
    // `size_of::<v4l2_ext_controls>()` writable bytes.
    let ret =
        unsafe { v4l::v4l2::ioctl(fd.raw(), vidioc::VIDIOC_G_EXT_CTRLS, controls.as_mut_ptr()) };
    ret.map_err(|error| device_error(fd, op, &error))?;

    decode::control_scalar(control.bytes(), control_type).ok_or_else(|| short_reply(op))
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
    // SAFETY: as `call`, plus the two pointers this struct carries. `controls.controls`
    // holds the address of `control`, and `control`'s union holds the address of
    // `buffer`'s allocation; both bindings are still in scope, so both allocations are
    // live for the whole call. Neither is aliased during it — the `&mut` borrows that
    // planted the addresses have ended, and nothing else reads or writes either binding
    // between here and the return, so the kernel is the only writer. `control.size` is
    // `len`, which came from `decode::payload_len`: bounded against
    // `limits::MAX_CONTROL_PAYLOAD_BYTES` and non-zero, so the kernel writes at most as
    // many bytes as `buffer` holds.
    let ret =
        unsafe { v4l::v4l2::ioctl(fd.raw(), vidioc::VIDIOC_G_EXT_CTRLS, controls.as_mut_ptr()) };
    ret.map_err(|error| device_error(fd, op, &error))?;

    Ok(buffer)
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

/// Run an ioctl whose only acceptable answer is success.
fn call<T: Copy>(
    fd: &Fd,
    request: vidioc::_IOC_TYPE,
    payload: &mut Payload<T>,
    op: &str,
) -> Result<()> {
    // SAFETY: `payload` is a live, exclusively borrowed `Payload<T>`; its pointer is
    // correctly aligned for `T` and valid for `size_of::<T>()` writable bytes, which is
    // exactly the width the `_IOWR`-encoded `request` declares for this call. The struct
    // holds no pointers — the two calls that plant one document that separately — so the
    // kernel dereferences nothing else.
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
    // SAFETY: identical to `call` — same borrow, same alignment, same width, no pointers
    // in the struct. Only the *interpretation* of the error differs.
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
fn device_error(fd: &Fd, op: &str, error: &std::io::Error) -> Error {
    match error.raw_os_error() {
        Some(libc::EBUSY) => Error::Busy {
            path: fd.path().to_owned(),
            holders: Vec::new(),
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
