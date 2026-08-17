//! Kernel bytes to schema values — pure, safe, and Miri's whole population.
//!
//! Nothing in here touches a device. Every function takes the bytes of one ioctl reply
//! and returns a `webcam-handler-schema` value, so the decoding half of the V4L2 backend
//! is executable under Miri over the captured fixtures in `crates/backends/v4l2/fixtures/`
//! (design §2.5; `scripts/miri.sh` selects `sys::decode`).
//!
//! ## Offsets are derived, never transcribed
//!
//! Every field offset comes from [`core::mem::offset_of!`] over the `v4l2-sys-mit`
//! bindgen output, which was generated against this host's real UAPI headers. That is
//! what lets this module read kernel structs field-by-field without either declaring
//! them by hand (banned — design §2.5) or reinterpreting bytes as a struct (which would
//! put `unsafe` on the one path Miri exists to check). The size assertion in this
//! module's tests closes the loop, pinning each struct's width against the fixture
//! captured from it by an independent probe.
//!
//! **Union arms included, since 2026-08-16.** The sentence above was true of every field
//! reached from a struct's root and false of the ten reached *through* a union: the
//! stepwise size and interval arms were read at `union_at + 4`, `+ 8`, `+ 12` and so on,
//! which is a transcription of `v4l2_frmsize_stepwise`'s and `v4l2_frmival_stepwise`'s
//! layouts wearing a derived base (note **N187**). `offset_of!` takes a nested path, so
//! `offset_of!(v4l2_frmivalenum, __bindgen_anon_1.stepwise.max.denominator)` names the
//! whole route and the bindings decide all of it. The arm the `type` word selects is still
//! this module's decision — that is the union-arm judgement design §2.5 wants made over
//! bytes — but where the arm's fields *are* is no longer anybody's here.
//!
//! ## What "represent, don't correct" means down here
//!
//! Out-of-range currents \[PF:4\], out-of-range defaults \[PF:5\], control types nobody
//! has named \[PF:1\], sparse menus \[PF:2\] and flag bits from next year's kernel
//! \[PF:12\] all pass through unchanged. The decoder's only judgement is about *its own*
//! ability to read the buffer: a field that would run off the end returns `None`, because
//! that means the bindings and the buffer disagree, and no amount of guessing improves
//! that answer.

use std::collections::BTreeMap;
use std::mem::offset_of;

use schema::camera::{FrameInterval, FrameSize, PixelFormat};
use schema::control::{
    ControlDesc, ControlFlags, ControlId, ControlRange, ControlSlug, ControlType, ControlValue,
    MenuItem,
};
use schema::limits;
use v4l::v4l_sys::{
    timeval, v4l2_buffer, v4l2_capability, v4l2_captureparm, v4l2_ext_control, v4l2_fmtdesc,
    v4l2_format, v4l2_fract, v4l2_frmivalenum, v4l2_frmsizeenum, v4l2_pix_format,
    v4l2_query_ext_ctrl, v4l2_querymenu, v4l2_requestbuffers, v4l2_streamparm,
};

use super::fields;

/// `V4L2_CTRL_TYPE_INTEGER_MENU`.
pub(crate) const CTRL_TYPE_INTEGER_MENU: u32 = 9;
/// `V4L2_CTRL_TYPE_INTEGER64`.
pub(crate) const CTRL_TYPE_INTEGER64: u32 = 5;
/// `V4L2_FRMSIZE_TYPE_DISCRETE`.
const FRMSIZE_TYPE_DISCRETE: u32 = 1;
/// `V4L2_FRMSIZE_TYPE_CONTINUOUS` — a stepwise range whose step is 1.
const FRMSIZE_TYPE_CONTINUOUS: u32 = 2;
/// `V4L2_FRMSIZE_TYPE_STEPWISE`.
const FRMSIZE_TYPE_STEPWISE: u32 = 3;
/// `V4L2_FRMIVAL_TYPE_DISCRETE`.
const FRMIVAL_TYPE_DISCRETE: u32 = 1;
/// `V4L2_FRMIVAL_TYPE_CONTINUOUS`.
const FRMIVAL_TYPE_CONTINUOUS: u32 = 2;
/// `V4L2_FRMIVAL_TYPE_STEPWISE`.
const FRMIVAL_TYPE_STEPWISE: u32 = 3;

/// The kernel's fixed width for a `char name[32]` field.
const NAME_LEN: usize = 32;
/// `v4l2_capability::driver`.
const DRIVER_LEN: usize = 16;

/// What `VIDIOC_QUERYCAP` said about a node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Capability {
    /// The driver name (`uvcvideo`).
    pub(crate) driver: String,
    /// The card name, verbatim — truncated by the kernel to 32 bytes, and that
    /// truncation is what the user sees, so it is not repaired.
    pub(crate) card: String,
    /// The driver's bus info. Per-USB-*device*, so two logical cameras on one device
    /// report the same string \[PF:13\] — which is why grouping never uses it.
    pub(crate) bus_info: String,
    /// The kernel version word.
    pub(crate) version: u32,
    /// What the whole device can do.
    pub(crate) capabilities: u32,
    /// What *this node* can do. The only authority on capture-vs-metadata \[PF:7\].
    pub(crate) device_caps: u32,
}

/// Whether a reply is at least as wide as the struct the bindings describe.
///
/// Checked by every decoder before a single field is read, and checked against the whole
/// struct rather than against the last field any one decoder happens to touch. A reply
/// short of the declared width means the kernel and the bindings disagree about the ABI,
/// and every field in it — including the ones that fall inside the truncated part — is
/// then suspect. Decoding the readable prefix would produce a *plausible* control, which
/// is the worst possible answer.
fn is_whole<T>(bytes: &[u8]) -> bool {
    bytes.len() >= size_of::<T>()
}

/// Decode a `VIDIOC_QUERYCAP` reply.
pub(crate) fn capability(bytes: &[u8]) -> Option<Capability> {
    if !is_whole::<v4l2_capability>(bytes) {
        return None;
    }
    Some(Capability {
        driver: fields::read_cstr(bytes, offset_of!(v4l2_capability, driver), DRIVER_LEN)?,
        card: fields::read_cstr(bytes, offset_of!(v4l2_capability, card), NAME_LEN)?,
        bus_info: fields::read_cstr(bytes, offset_of!(v4l2_capability, bus_info), NAME_LEN)?,
        version: fields::read_u32(bytes, offset_of!(v4l2_capability, version))?,
        capabilities: fields::read_u32(bytes, offset_of!(v4l2_capability, capabilities))?,
        device_caps: fields::read_u32(bytes, offset_of!(v4l2_capability, device_caps))?,
    })
}

/// Decode a `VIDIOC_QUERY_EXT_CTRL` reply into the D2 control model.
///
/// The menu and the current value are *not* here: both need further ioctls, and a
/// decoder that pretended otherwise would have to invent them. The returned descriptor
/// carries an empty menu and `current: None`, which the caller fills.
///
/// `None` only when the buffer is shorter than the bindings say the struct is, or when
/// the driver's control name slugs to nothing (D2 refuses to invent a handle, because an
/// invented one collides silently).
pub(crate) fn control_desc(bytes: &[u8]) -> Option<ControlDesc> {
    if !is_whole::<v4l2_query_ext_ctrl>(bytes) {
        return None;
    }
    let name = fields::read_cstr(bytes, offset_of!(v4l2_query_ext_ctrl, name), NAME_LEN)?;
    let slug = ControlSlug::from_name(&name)?;
    let raw_step = fields::read_u64(bytes, offset_of!(v4l2_query_ext_ctrl, step))?;
    let nr_of_dims = fields::read_u32(bytes, offset_of!(v4l2_query_ext_ctrl, nr_of_dims))?;

    Some(ControlDesc {
        id: ControlId(fields::read_u32(
            bytes,
            offset_of!(v4l2_query_ext_ctrl, id),
        )?),
        name,
        slug,
        control_type: ControlType::from_raw(fields::read_u32(
            bytes,
            offset_of!(v4l2_query_ext_ctrl, type_),
        )?),
        range: ControlRange {
            min: fields::read_i64(bytes, offset_of!(v4l2_query_ext_ctrl, minimum))?,
            max: fields::read_i64(bytes, offset_of!(v4l2_query_ext_ctrl, maximum))?,
            // The kernel's step is unsigned and ours is signed. A step past `i64::MAX`
            // saturates, and the direction is chosen for what it does downstream: a
            // saturated step makes only the minimum reachable, so a sweep over such a
            // control plans **one** sample.
            //
            // Mapping it to 0 was the first instinct and is the wrong one —
            // `ControlRange::effective_step` reads 0 as "no usable step" and substitutes
            // 1, which is the *finest* step there is, so a control we could not
            // understand would plan the largest sweep in the table. On a pan motor with a
            // ±468000 range that is the difference between one write and a capped 32.
            // When a device says something we cannot represent, the safe reading is the
            // one that does least.
            step: i64::try_from(raw_step).unwrap_or(i64::MAX),
        },
        default: fields::read_i64(bytes, offset_of!(v4l2_query_ext_ctrl, default_value))?,
        flags: ControlFlags::from_raw(fields::read_u32(
            bytes,
            offset_of!(v4l2_query_ext_ctrl, flags),
        )?),
        menu: BTreeMap::new(),
        elems: fields::read_u32(bytes, offset_of!(v4l2_query_ext_ctrl, elems))?,
        elem_size: fields::read_u32(bytes, offset_of!(v4l2_query_ext_ctrl, elem_size))?,
        dims: dims(bytes, nr_of_dims)?,
        current: None,
    })
}

/// The declared array dimensions, bounded by the fixed width of the kernel's `dims[4]`.
///
/// A driver reporting `nr_of_dims = 9` is describing an array the struct has no room for;
/// we read the four that exist rather than believing the count.
fn dims(bytes: &[u8], nr_of_dims: u32) -> Option<Vec<u32>> {
    const DIMS_CAPACITY: u32 = 4;
    let base = offset_of!(v4l2_query_ext_ctrl, dims);
    let count = nr_of_dims.min(DIMS_CAPACITY);
    let mut out = Vec::new();
    for index in 0..count {
        let step = usize::try_from(index).ok()?.checked_mul(size_of::<u32>())?;
        out.push(fields::read_u32(bytes, base.checked_add(step)?)?);
    }
    Some(out)
}

/// Decode a `VIDIOC_QUERYMENU` reply, choosing the union arm by the *control's* type.
///
/// This is the union-arm decision design §2.5 names. Reading it from bytes rather than
/// through a union field turns "which arm did the kernel write" from a safety obligation
/// into an ordinary, testable branch — and the wrong branch is visibly wrong: reading a
/// named item's 32 name bytes as an `i64` yields a large meaningless number, which one of
/// the tests below pins so the choice cannot silently invert.
pub(crate) fn menu_item(bytes: &[u8], control_type: u32) -> Option<(u32, MenuItem)> {
    if !is_whole::<v4l2_querymenu>(bytes) {
        return None;
    }
    let index = fields::read_u32(bytes, offset_of!(v4l2_querymenu, index))?;
    let union_at = offset_of!(v4l2_querymenu, __bindgen_anon_1);
    let item = match control_type {
        CTRL_TYPE_INTEGER_MENU => MenuItem::Value {
            value: fields::read_i64(bytes, union_at)?,
        },
        // Named for `MENU`, and named for anything else too: a driver that answered
        // QUERYMENU for a type we did not expect wrote *something* there, and 32 bytes
        // read as text degrade into a readable string, where 8 bytes read as an integer
        // degrade into nonsense.
        _ => MenuItem::Name {
            name: fields::read_cstr(bytes, union_at, NAME_LEN)?,
        },
    };
    Some((index, item))
}

/// Decode a `VIDIOC_ENUM_FMT` reply.
pub(crate) fn format_desc(bytes: &[u8]) -> Option<(PixelFormat, String, u32)> {
    if !is_whole::<v4l2_fmtdesc>(bytes) {
        return None;
    }
    let raw = fields::read_u32(bytes, offset_of!(v4l2_fmtdesc, pixelformat))?;
    let description = fields::read_cstr(bytes, offset_of!(v4l2_fmtdesc, description), NAME_LEN)?;
    let flags = fields::read_u32(bytes, offset_of!(v4l2_fmtdesc, flags))?;
    Some((PixelFormat::from_fourcc(raw), description, flags))
}

/// Decode a `VIDIOC_ENUM_FRAMESIZES` reply.
///
/// A `type` outside the three the kernel defines becomes [`FrameSize::Unknown`] carrying
/// the discriminant — the entry exists, and dropping it would make a short size list read
/// as a complete one. `None` means only that the reply was not as wide as the bindings
/// say the struct is.
pub(crate) fn frame_size(bytes: &[u8]) -> Option<FrameSize> {
    if !is_whole::<v4l2_frmsizeenum>(bytes) {
        return None;
    }
    let kind = fields::read_u32(bytes, offset_of!(v4l2_frmsizeenum, type_))?;
    // Each field is named through the union arm that holds it, so the *arm* decides where
    // its fields are — see this module's header. `discrete` and `stepwise` overlay the same
    // bytes, and which of the two is read is the `type` word's decision, exactly as it is
    // in the kernel.
    match kind {
        FRMSIZE_TYPE_DISCRETE => Some(FrameSize::Discrete {
            width: fields::read_u32(
                bytes,
                offset_of!(v4l2_frmsizeenum, __bindgen_anon_1.discrete.width),
            )?,
            height: fields::read_u32(
                bytes,
                offset_of!(v4l2_frmsizeenum, __bindgen_anon_1.discrete.height),
            )?,
        }),
        FRMSIZE_TYPE_CONTINUOUS | FRMSIZE_TYPE_STEPWISE => Some(FrameSize::Stepwise {
            min_width: fields::read_u32(
                bytes,
                offset_of!(v4l2_frmsizeenum, __bindgen_anon_1.stepwise.min_width),
            )?,
            max_width: fields::read_u32(
                bytes,
                offset_of!(v4l2_frmsizeenum, __bindgen_anon_1.stepwise.max_width),
            )?,
            step_width: fields::read_u32(
                bytes,
                offset_of!(v4l2_frmsizeenum, __bindgen_anon_1.stepwise.step_width),
            )?,
            min_height: fields::read_u32(
                bytes,
                offset_of!(v4l2_frmsizeenum, __bindgen_anon_1.stepwise.min_height),
            )?,
            max_height: fields::read_u32(
                bytes,
                offset_of!(v4l2_frmsizeenum, __bindgen_anon_1.stepwise.max_height),
            )?,
            step_height: fields::read_u32(
                bytes,
                offset_of!(v4l2_frmsizeenum, __bindgen_anon_1.stepwise.step_height),
            )?,
        }),
        raw => Some(FrameSize::Unknown { raw }),
    }
}

/// Decode a `VIDIOC_ENUM_FRAMEINTERVALS` reply. An unrecognized `type` becomes
/// [`FrameInterval::Unknown`], for the reason [`frame_size`] gives.
pub(crate) fn frame_interval(bytes: &[u8]) -> Option<FrameInterval> {
    if !is_whole::<v4l2_frmivalenum>(bytes) {
        return None;
    }
    let kind = fields::read_u32(bytes, offset_of!(v4l2_frmivalenum, type_))?;
    match kind {
        FRMIVAL_TYPE_DISCRETE => Some(FrameInterval::Discrete {
            numerator: fields::read_u32(
                bytes,
                offset_of!(v4l2_frmivalenum, __bindgen_anon_1.discrete.numerator),
            )?,
            denominator: fields::read_u32(
                bytes,
                offset_of!(v4l2_frmivalenum, __bindgen_anon_1.discrete.denominator),
            )?,
        }),
        // `v4l2_frmival_stepwise` is {min, max, step}, each a `v4l2_fract`; the schema
        // keeps min and max and drops the step, which the kernel documents as advisory
        // for CONTINUOUS. Naming `step` here and discarding it would be a field this
        // build reads and refuses to say it read, so it is simply not named — the two
        // `v4l2_fract`s below are the whole of what the schema carries.
        FRMIVAL_TYPE_CONTINUOUS | FRMIVAL_TYPE_STEPWISE => Some(FrameInterval::Stepwise {
            min_numerator: fields::read_u32(
                bytes,
                offset_of!(v4l2_frmivalenum, __bindgen_anon_1.stepwise.min.numerator),
            )?,
            min_denominator: fields::read_u32(
                bytes,
                offset_of!(v4l2_frmivalenum, __bindgen_anon_1.stepwise.min.denominator),
            )?,
            max_numerator: fields::read_u32(
                bytes,
                offset_of!(v4l2_frmivalenum, __bindgen_anon_1.stepwise.max.numerator),
            )?,
            max_denominator: fields::read_u32(
                bytes,
                offset_of!(v4l2_frmivalenum, __bindgen_anon_1.stepwise.max.denominator),
            )?,
        }),
        raw => Some(FrameInterval::Unknown { raw }),
    }
}

/// Decode the scalar a `VIDIOC_G_EXT_CTRLS` reply left in one `v4l2_ext_control`.
///
/// `INTEGER64` reads the 64-bit arm; everything else reads the 32-bit one, which is what
/// the kernel writes for `INTEGER`, `BOOLEAN`, `MENU`, `BITMASK` and `INTEGER_MENU`
/// alike. Compound controls do not come through here — their bytes are the payload
/// buffer the caller supplied, not a union arm.
pub(crate) fn control_scalar(bytes: &[u8], control_type: u32) -> Option<ControlValue> {
    if !is_whole::<v4l2_ext_control>(bytes) {
        return None;
    }
    let union_at = offset_of!(v4l2_ext_control, __bindgen_anon_1);
    if control_type == CTRL_TYPE_INTEGER64 {
        Some(ControlValue::Int(fields::read_i64(bytes, union_at)?))
    } else {
        Some(ControlValue::Int(i64::from(fields::read_i32(
            bytes, union_at,
        )?)))
    }
}

/// Which union arm a scalar write goes into, and the value narrowed to fit it.
///
/// [`ScalarArm`]'s whole reason for existing is that [`control_scalar`] chooses an arm by
/// control type when *reading*, and a write that chose differently would put four bytes
/// into a neighbouring field while the kernel reported success — a defect no read-back
/// could detect, because the read would look in the arm the write did not fill.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScalarArm {
    /// The 64-bit arm, for `V4L2_CTRL_TYPE_INTEGER64`.
    Wide(i64),
    /// The 32-bit arm, for everything else the kernel writes inline.
    Narrow(i32),
}

/// The arm and value one scalar write occupies — [`control_scalar`]'s inverse.
///
/// The one encoder in a module named for decoding, and it lives here on purpose: the arm
/// the reader looks in and the arm the writer fills are one decision, and two homes for it
/// is two chances to disagree. It is pure and total, so Miri runs it alongside the
/// decoders.
///
/// `None` means the value cannot be expressed: a request wider than the 32-bit field the
/// control's type declares. Refused rather than truncated — an `as` cast would write a
/// different number and the read-back would then report an adjustment the device never
/// made.
pub(crate) fn scalar_arm(control_type: u32, value: i64) -> Option<ScalarArm> {
    if control_type == CTRL_TYPE_INTEGER64 {
        Some(ScalarArm::Wide(value))
    } else {
        i32::try_from(value).ok().map(ScalarArm::Narrow)
    }
}

// ------------------------------------------------------------------ the streaming path

/// `V4L2_CAP_TIMEPERFRAME` — the bit that says a `G_PARM` answer's interval means
/// anything.
const CAP_TIMEPERFRAME: u32 = 0x1000;
/// `V4L2_BUF_FLAG_ERROR` — this buffer was dequeued and its contents are not a frame.
pub(crate) const BUF_FLAG_ERROR: u32 = 0x0040;

/// The timestamp decoder below reads two 64-bit halves, which is `timeval`'s shape on
/// every target this project builds for. A target where it is not stops the build here
/// rather than silently reporting a garbage frame time.
const _: () = assert!(size_of::<timeval>() == 16);

/// The byte offset of a `v4l2_pix_format` field inside a `v4l2_format`.
///
/// Two derived offsets composed, never a transcribed constant: the union's own offset
/// within `v4l2_format`, plus the field's offset within the `pix` arm. Composing them is
/// what lets this module reach into a union without an `unsafe` field access whose
/// obligation would be "prove which arm was written".
pub(crate) const fn pix_field(field_offset: usize) -> Option<usize> {
    offset_of!(v4l2_format, fmt).checked_add(field_offset)
}

/// The byte offset of a `v4l2_captureparm` field inside a `v4l2_streamparm`.
pub(crate) const fn capture_parm_field(field_offset: usize) -> Option<usize> {
    offset_of!(v4l2_streamparm, parm).checked_add(field_offset)
}

/// What the driver agreed to stream, as `G_FMT` reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PixFormat {
    /// The fourcc the driver settled on.
    pub(crate) pixel_format: PixelFormat,
    /// Frame width in pixels.
    pub(crate) width: u32,
    /// Frame height in pixels.
    pub(crate) height: u32,
    /// Bytes per row; 0 for a compressed bitstream.
    pub(crate) bytes_per_line: u32,
    /// The driver's maximum frame size, which bounds the buffers it will allocate.
    pub(crate) size_image: u32,
}

/// Decode the capture half of a `VIDIOC_G_FMT`/`S_FMT` reply.
///
/// The reply is the *negotiated* format, which is the whole reason both calls read it
/// back: `S_FMT` adjusts silently and reports success, exactly as a control write does
/// (D5 is D3 applied to formats).
pub(crate) fn pix_format(bytes: &[u8]) -> Option<PixFormat> {
    if !is_whole::<v4l2_format>(bytes) {
        return None;
    }
    Some(PixFormat {
        pixel_format: PixelFormat::from_fourcc(fields::read_u32(
            bytes,
            pix_field(offset_of!(v4l2_pix_format, pixelformat))?,
        )?),
        width: fields::read_u32(bytes, pix_field(offset_of!(v4l2_pix_format, width))?)?,
        height: fields::read_u32(bytes, pix_field(offset_of!(v4l2_pix_format, height))?)?,
        bytes_per_line: fields::read_u32(
            bytes,
            pix_field(offset_of!(v4l2_pix_format, bytesperline))?,
        )?,
        size_image: fields::read_u32(bytes, pix_field(offset_of!(v4l2_pix_format, sizeimage))?)?,
    })
}

/// How many buffers `VIDIOC_REQBUFS` actually granted.
///
/// The driver is free to give fewer than were asked for — and to give *zero*, which the
/// caller must not read as "the request was honoured". D5's theme again, one field wide.
pub(crate) fn granted_buffers(bytes: &[u8]) -> Option<u32> {
    if !is_whole::<v4l2_requestbuffers>(bytes) {
        return None;
    }
    fields::read_u32(bytes, offset_of!(v4l2_requestbuffers, count))
}

/// Where one buffer lives, from a `VIDIOC_QUERYBUF` reply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BufferMapping {
    /// The offset to hand `mmap`, in the device's own address space.
    pub(crate) offset: u32,
    /// How many bytes the buffer holds.
    pub(crate) length: u32,
}

/// Decode a `VIDIOC_QUERYBUF` reply's mapping fields.
pub(crate) fn buffer_mapping(bytes: &[u8]) -> Option<BufferMapping> {
    if !is_whole::<v4l2_buffer>(bytes) {
        return None;
    }
    Some(BufferMapping {
        // `m` is a union whose first member is the mmap offset; the other members are a
        // userptr, a plane array and a dmabuf fd, none of which this build requests.
        offset: fields::read_u32(bytes, offset_of!(v4l2_buffer, m))?,
        length: fields::read_u32(bytes, offset_of!(v4l2_buffer, length))?,
    })
}

/// What a `VIDIOC_DQBUF` reply says about the buffer it handed back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Dequeued {
    /// Which buffer, so the caller can find its mapping and queue it back.
    pub(crate) index: u32,
    /// How many bytes the driver wrote. Device-supplied, and bounded by the mapping
    /// before it becomes a length (rubric B10).
    pub(crate) bytes_used: u32,
    /// The driver's sequence number. Gaps mean dropped frames, which is a fact worth
    /// carrying rather than smoothing over.
    pub(crate) sequence: u32,
    /// The driver's timestamp on its own clock, in microseconds.
    pub(crate) timestamp_us: i64,
    /// The buffer's flag word, raw. `BUF_FLAG_ERROR` is the one this build reads.
    pub(crate) flags: u32,
}

impl Dequeued {
    /// Whether the driver marked this buffer as carrying no usable frame.
    pub(crate) const fn is_error(self) -> bool {
        self.flags & BUF_FLAG_ERROR != 0
    }
}

/// Decode a `VIDIOC_DQBUF` reply.
pub(crate) fn dequeued(bytes: &[u8]) -> Option<Dequeued> {
    if !is_whole::<v4l2_buffer>(bytes) {
        return None;
    }
    let stamp_at = offset_of!(v4l2_buffer, timestamp);
    let seconds = fields::read_i64(bytes, stamp_at.checked_add(offset_of!(timeval, tv_sec))?)?;
    let micros = fields::read_i64(bytes, stamp_at.checked_add(offset_of!(timeval, tv_usec))?)?;

    Some(Dequeued {
        index: fields::read_u32(bytes, offset_of!(v4l2_buffer, index))?,
        bytes_used: fields::read_u32(bytes, offset_of!(v4l2_buffer, bytesused))?,
        sequence: fields::read_u32(bytes, offset_of!(v4l2_buffer, sequence))?,
        // Saturating rather than wrapping: a driver whose clock is far enough out to
        // overflow this has told us something implausible, and a wrapped timestamp would
        // make a frame look like it arrived before the one before it.
        timestamp_us: seconds.saturating_mul(1_000_000).saturating_add(micros),
        flags: fields::read_u32(bytes, offset_of!(v4l2_buffer, flags))?,
    })
}

/// What a `VIDIOC_G_PARM`/`S_PARM` reply says about the frame interval.
///
/// Two answers, because the caller has to tell them apart and for a while it could not: a
/// bare `Option<FrameInterval>` collapsed "the driver said it does not negotiate an
/// interval on this node" into the same value as "the driver said `1/0`", and one line up
/// both became [`FrameInterval::Unstated`] — a value whose own documentation states as
/// fact that the capability bit was cleared (note **N199**).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReportedInterval {
    /// The driver cleared `V4L2_CAP_TIMEPERFRAME`: its `timeperframe` field means
    /// nothing, whatever is in it.
    NotOffered,
    /// The driver set the bit, and this is the fraction it wrote — **verbatim**,
    /// degenerate or not.
    ///
    /// A zero numerator or denominator is not corrected and not dropped: it is the
    /// device's answer, and D2 asks for what cannot be interpreted to be carried rather
    /// than tidied away. [`FrameInterval::fps`] answers `None` for it and
    /// `imaging::avi::interval_micros` refuses it, so nothing downstream divides by it.
    Offered(FrameInterval),
}

/// The frame interval a `VIDIOC_G_PARM`/`S_PARM` reply reports.
///
/// `None` — the outer one — means the reply was **shorter than the bindings describe**,
/// which is this build's problem rather than the device's; the caller turns it into
/// `short_reply`, exactly as every other decoder here does.
pub(crate) fn capture_interval(bytes: &[u8]) -> Option<ReportedInterval> {
    if !is_whole::<v4l2_streamparm>(bytes) {
        return None;
    }
    let capability = fields::read_u32(
        bytes,
        capture_parm_field(offset_of!(v4l2_captureparm, capability))?,
    )?;
    if capability & CAP_TIMEPERFRAME == 0 {
        return Some(ReportedInterval::NotOffered);
    }
    let per_frame = capture_parm_field(offset_of!(v4l2_captureparm, timeperframe))?;
    let numerator = fields::read_u32(
        bytes,
        per_frame.checked_add(offset_of!(v4l2_fract, numerator))?,
    )?;
    let denominator = fields::read_u32(
        bytes,
        per_frame.checked_add(offset_of!(v4l2_fract, denominator))?,
    )?;
    Some(ReportedInterval::Offered(FrameInterval::Discrete {
        numerator,
        denominator,
    }))
}

/// A payload control's bytes, bounded before anything is allocated.
///
/// `elem_size × elems` is device-supplied and therefore attacker-shaped (rubric B10).
/// The product is computed with checked arithmetic and compared against
/// [`limits::MAX_CONTROL_PAYLOAD_BYTES`] here, in safe code, before it can become a
/// length anywhere.
pub(crate) fn payload_len(elem_size: u32, elems: u32) -> Option<usize> {
    let len = usize::try_from(elem_size)
        .ok()?
        .checked_mul(usize::try_from(elems).ok()?)?;
    if len == 0 || len > limits::MAX_CONTROL_PAYLOAD_BYTES {
        None
    } else {
        Some(len)
    }
}

#[cfg(test)]
mod tests {
    use schema::control::KnownFlag;

    use super::*;

    // The fixtures are raw ioctl replies captured from real hardware; see
    // `crates/backends/v4l2/fixtures/README.md` for provenance.
    const QUERYCAP_CHICONY_RGB: &[u8] = include_bytes!("../../fixtures/querycap-chicony-rgb.bin");
    const QUERYCAP_CHICONY_IR_META: &[u8] =
        include_bytes!("../../fixtures/querycap-chicony-ir-metadata-node.bin");
    const QUERYCAP_OBSBOT: &[u8] = include_bytes!("../../fixtures/querycap-obsbot-tiny3.bin");
    const ROI_RECT: &[u8] = include_bytes!("../../fixtures/query_ext_ctrl-chicony-roi-rect.bin");
    const AUTO_EXPOSURE: &[u8] =
        include_bytes!("../../fixtures/query_ext_ctrl-chicony-auto-exposure.bin");
    const WHITE_BALANCE_TEMPERATURE: &[u8] =
        include_bytes!("../../fixtures/query_ext_ctrl-chicony-white-balance-temperature.bin");
    const PRIVACY: &[u8] = include_bytes!("../../fixtures/query_ext_ctrl-chicony-privacy.bin");
    const ZOOM_CONTINUOUS: &[u8] =
        include_bytes!("../../fixtures/query_ext_ctrl-obsbot-zoom-continuous.bin");
    const POWER_LINE_FREQUENCY: &[u8] =
        include_bytes!("../../fixtures/query_ext_ctrl-obsbot-power-line-frequency.bin");
    const PAN_ABSOLUTE: &[u8] =
        include_bytes!("../../fixtures/query_ext_ctrl-obsbot-pan-absolute.bin");
    const MENU_ITEM_1: &[u8] =
        include_bytes!("../../fixtures/querymenu-chicony-auto-exposure-1.bin");
    const MENU_ITEM_3: &[u8] =
        include_bytes!("../../fixtures/querymenu-chicony-auto-exposure-3.bin");
    const ENUM_FMT_MJPG: &[u8] = include_bytes!("../../fixtures/enum_fmt-chicony-mjpg.bin");
    const FRAMESIZE_MJPG: &[u8] =
        include_bytes!("../../fixtures/enum_framesizes-chicony-mjpg-0.bin");
    const FRAMEINTERVAL_MJPG: &[u8] =
        include_bytes!("../../fixtures/enum_frameintervals-chicony-mjpg-0-0.bin");

    /// The size/offset assertion design §2.5 requires wherever kernel layout is relied
    /// on. The fixtures were captured by an independent probe against this host's
    /// headers; if bindgen and that probe disagree about a struct's width, every offset
    /// below is meaningless and this is where it must stop.
    #[test]
    fn the_bindings_and_the_captured_replies_agree_on_every_struct_width() {
        let cases: [(&str, usize, usize); 6] = [
            (
                "v4l2_capability",
                size_of::<v4l2_capability>(),
                QUERYCAP_CHICONY_RGB.len(),
            ),
            (
                "v4l2_query_ext_ctrl",
                size_of::<v4l2_query_ext_ctrl>(),
                ROI_RECT.len(),
            ),
            (
                "v4l2_querymenu",
                size_of::<v4l2_querymenu>(),
                MENU_ITEM_1.len(),
            ),
            (
                "v4l2_fmtdesc",
                size_of::<v4l2_fmtdesc>(),
                ENUM_FMT_MJPG.len(),
            ),
            (
                "v4l2_frmsizeenum",
                size_of::<v4l2_frmsizeenum>(),
                FRAMESIZE_MJPG.len(),
            ),
            (
                "v4l2_frmivalenum",
                size_of::<v4l2_frmivalenum>(),
                FRAMEINTERVAL_MJPG.len(),
            ),
        ];
        for (name, bindings, fixture) in cases {
            assert_eq!(
                bindings, fixture,
                "{name}: the bindings say {bindings} bytes, the captured reply is {fixture}"
            );
        }
    }

    #[test]
    fn a_capture_node_reports_device_caps_that_differ_from_the_whole_device() {
        let cap = capability(QUERYCAP_CHICONY_RGB).expect("a full reply decodes");
        assert_eq!(cap.driver, "uvcvideo");
        assert_eq!(cap.card, "Integrated Camera: Integrated C");
        assert_eq!(cap.bus_info, "usb-0000:00:14.0-4");
        assert_eq!(cap.device_caps, 0x0420_0001);
        assert_eq!(cap.capabilities, 0x84a0_0001);
        assert_ne!(
            cap.device_caps, cap.capabilities,
            "the difference is the whole reason both are carried"
        );
    }

    #[test]
    fn a_metadata_node_is_not_a_capture_node_and_shares_the_capture_nodes_bus_info() {
        // PF:13, byte for byte: `bus_info` cannot tell two logical cameras apart, and
        // `device_caps` is what tells a metadata node from a capture one [PF:7].
        let meta = capability(QUERYCAP_CHICONY_IR_META).expect("decodes");
        let capture = capability(QUERYCAP_CHICONY_RGB).expect("decodes");
        assert_eq!(
            meta.bus_info, capture.bus_info,
            "PF:13 — two logical cameras, one bus_info"
        );
        assert_ne!(meta.card, capture.card);
        assert_eq!(meta.device_caps & schema::camera::CAP_VIDEO_CAPTURE, 0);
        assert_ne!(meta.device_caps & schema::camera::CAP_META_CAPTURE, 0);
    }

    #[test]
    fn a_card_name_the_kernel_truncated_arrives_truncated() {
        // The field is 32 bytes with no NUL to spare; repairing it would invent a name
        // the user's other tools do not show.
        let cap = capability(QUERYCAP_OBSBOT).expect("decodes");
        assert_eq!(cap.card, "OBSBOT Tiny 3: OBSBOT Tiny 3 St");
    }

    #[test]
    fn the_control_type_that_panics_the_v4l_crate_decodes_instead() {
        // PF:1, as bytes. 0x0107 is `RECT`; the point is that the decoder is total, so
        // even the successor type nobody has invented would land in `Unknown`.
        let desc = control_desc(ROI_RECT).expect("decodes");
        assert_eq!(desc.name, "Region of Interest Rectangle");
        assert_eq!(desc.slug.as_str(), "region_of_interest_rectangle");
        assert_eq!(desc.control_type, ControlType::Rect);
        assert_eq!(desc.control_type.to_raw(), 0x0107);
        assert_eq!(desc.elem_size, 16);
        assert_eq!(desc.elems, 1);
        assert!(desc.flags.has(KnownFlag::HasPayload));
    }

    #[test]
    fn an_out_of_range_default_is_reported_not_corrected() {
        // PF:5 — the OBSBOT declares [0..2] and defaults to 3.
        let desc = control_desc(POWER_LINE_FREQUENCY).expect("decodes");
        assert_eq!(desc.range.min, 0);
        assert_eq!(desc.range.max, 2);
        assert_eq!(desc.default, 3);
        assert!(desc.default_out_of_range());
    }

    #[test]
    fn a_read_only_control_keeps_its_flag_and_a_stepped_one_keeps_its_step() {
        // PF:12 — Privacy is READ_ONLY, and its flag word carries none of the 0x1000 bit
        // that most controls on this kernel do.
        let privacy = control_desc(PRIVACY).expect("decodes");
        assert!(privacy.flags.has(KnownFlag::ReadOnly));
        assert!(!privacy.is_writable());
        assert!(!privacy.flags.has(KnownFlag::HasWhichMinMax));

        // PF:3's control, caught with automation holding it: INACTIVE is set, and the
        // step is not 1.
        let wbt = control_desc(WHITE_BALANCE_TEMPERATURE).expect("decodes");
        assert!(wbt.is_inactive());
        assert!(wbt.flags.has(KnownFlag::HasWhichMinMax));
        assert_eq!(wbt.range.step, 10);
        assert_eq!(wbt.range.effective_step(), 10);
    }

    #[test]
    fn wide_ranges_and_large_steps_survive_as_themselves() {
        let pan = control_desc(PAN_ABSOLUTE).expect("decodes");
        assert_eq!(pan.range.min, -468_000);
        assert_eq!(pan.range.max, 468_000);
        assert_eq!(pan.range.step, 3_600);

        let zoom = control_desc(ZOOM_CONTINUOUS).expect("decodes");
        assert_eq!(zoom.range.min, -100);
        assert_eq!(zoom.range.max, 100);
        // PF:4's out-of-range *current* is a G_EXT_CTRLS fact; QUERY_EXT_CTRL carries no
        // current at all, and the decoder must not invent one.
        assert_eq!(zoom.current, None);
    }

    #[test]
    fn a_menus_holes_are_holes_and_the_union_arm_is_chosen_by_type() {
        // PF:2 — indices 1 and 3 exist; 0 and 2 return EINVAL and therefore have no
        // fixture, which is the finding rather than an omission.
        let (index, item) = menu_item(MENU_ITEM_1, ControlType::Menu.to_raw()).expect("decodes");
        assert_eq!(index, 1);
        assert_eq!(
            item,
            MenuItem::Name {
                name: "Manual Mode".to_owned()
            }
        );
        let (index, item) = menu_item(MENU_ITEM_3, ControlType::Menu.to_raw()).expect("decodes");
        assert_eq!(index, 3);
        assert_eq!(item.name(), Some("Aperture Priority Mode"));

        // The inverse direction: reading a *named* item's bytes as an integer menu is
        // the wrong arm, and it must be visibly wrong rather than plausibly wrong.
        let (_, wrong_arm) = menu_item(MENU_ITEM_1, CTRL_TYPE_INTEGER_MENU).expect("decodes");
        assert!(
            matches!(wrong_arm, MenuItem::Value { value } if value > 1_000_000),
            "the wrong union arm must produce nonsense, not a plausible index: {wrong_arm:?}"
        );
    }

    #[test]
    fn the_menu_control_itself_declares_a_range_wider_than_its_live_indices() {
        // The reason menus are a sparse map and not a Vec: the declared range is [0..3]
        // and only two of those four indices exist.
        let desc = control_desc(AUTO_EXPOSURE).expect("decodes");
        assert_eq!(desc.control_type, ControlType::Menu);
        assert_eq!((desc.range.min, desc.range.max), (0, 3));
        assert!(desc.menu.is_empty(), "the menu is the caller's to fill");
    }

    #[test]
    fn formats_sizes_and_intervals_decode_from_their_replies() {
        let (format, description, flags) = format_desc(ENUM_FMT_MJPG).expect("decodes");
        assert_eq!(format, PixelFormat::MJPG);
        assert_eq!(description, "Motion-JPEG");
        assert_ne!(flags & 1, 0, "MJPG is declared compressed");

        assert_eq!(
            frame_size(FRAMESIZE_MJPG),
            Some(FrameSize::Discrete {
                width: 1280,
                height: 720
            })
        );
        let interval = frame_interval(FRAMEINTERVAL_MJPG).expect("decodes");
        assert_eq!(
            interval,
            FrameInterval::Discrete {
                numerator: 1,
                denominator: 30
            }
        );
        assert_eq!(interval.fps(), Some(30.0));
    }

    #[test]
    fn a_truncated_reply_decodes_to_none_rather_than_to_a_plausible_value() {
        // The inverse of every test above. A buffer the bindings disagree with must not
        // produce a control that merely looks wrong.
        // Including one byte short of the full width: the fields this decoder happens
        // to read all live in the first 104 bytes, so a prefix-only check would have let
        // a 231-byte reply through as a plausible control.
        for length in [0usize, 8, 104, size_of::<v4l2_query_ext_ctrl>() - 1] {
            assert_eq!(
                control_desc(ROI_RECT.get(..length).expect("in range")),
                None,
                "a {length}-byte reply must not decode"
            );
        }
        assert!(
            control_desc(ROI_RECT).is_some(),
            "…while the whole reply must"
        );
        assert_eq!(capability(&QUERYCAP_CHICONY_RGB[..16]), None);
        assert_eq!(
            menu_item(&MENU_ITEM_1[..8], ControlType::Menu.to_raw()),
            None
        );
        assert_eq!(frame_size(&FRAMESIZE_MJPG[..8]), None);
        assert_eq!(frame_interval(&FRAMEINTERVAL_MJPG[..8]), None);
        assert_eq!(format_desc(&ENUM_FMT_MJPG[..8]), None);
    }

    #[test]
    fn an_unrecognized_size_or_interval_type_is_carried_rather_than_guessed_or_dropped() {
        // Not `None`: the entry exists and the driver described it, so the *shape* is
        // what we cannot read, not the fact. A dropped entry would make a short list read
        // as a complete one — "this camera does not offer 4K" invented out of our own
        // ignorance.
        let mut bytes = FRAMESIZE_MJPG.to_vec();
        fields::write_u32(&mut bytes, offset_of!(v4l2_frmsizeenum, type_), 99).expect("fits");
        let size = frame_size(&bytes).expect("the entry is carried");
        assert_eq!(size, FrameSize::Unknown { raw: 99 });
        assert!(!size.is_interpretable());
        assert_eq!(size.max_dimensions(), None);

        let mut bytes = FRAMEINTERVAL_MJPG.to_vec();
        fields::write_u32(&mut bytes, offset_of!(v4l2_frmivalenum, type_), 99).expect("fits");
        assert_eq!(
            frame_interval(&bytes),
            Some(FrameInterval::Unknown { raw: 99 })
        );

        // A truncated reply is still `None` — that is a different failure, and the two
        // must not be spelled the same way.
        assert_eq!(frame_size(&FRAMESIZE_MJPG[..8]), None);
    }

    #[test]
    fn a_continuous_size_decodes_through_the_stepwise_arm() {
        // No attached camera reports CONTINUOUS, so this one is constructed from a real
        // reply rather than captured — the mutation is named in the test's own body.
        let mut bytes = FRAMESIZE_MJPG.to_vec();
        fields::write_u32(&mut bytes, offset_of!(v4l2_frmsizeenum, type_), 2).expect("fits");
        let union_at = offset_of!(v4l2_frmsizeenum, __bindgen_anon_1);
        for (index, value) in [640u32, 1920, 16, 480, 1080, 8].into_iter().enumerate() {
            fields::write_u32(&mut bytes, union_at + index * 4, value).expect("fits");
        }
        assert_eq!(
            frame_size(&bytes),
            Some(FrameSize::Stepwise {
                min_width: 640,
                max_width: 1920,
                step_width: 16,
                min_height: 480,
                max_height: 1080,
                step_height: 8,
            })
        );
    }

    #[test]
    fn a_stepwise_interval_decodes_through_the_stepwise_arm() {
        // The interval half of the test above, and the reason both are written this way:
        // the *decoder* reaches its fields through `offset_of!` over the bindings, and the
        // bytes here are laid out by an independent transcription of what the kernel
        // documents `v4l2_frmival_stepwise` to be — {min, max, step}, each a `v4l2_fract`.
        // Two derivations of one layout is what makes a wrong one visible; a fixture
        // written through the same `offset_of!` the decoder reads through could not go red
        // for any offset at all.
        //
        // The `step` fraction is the third one and the schema does not carry it, so the
        // last pair below is what a decoder reading `max` one field too far would return.
        let mut bytes = FRAMEINTERVAL_MJPG.to_vec();
        fields::write_u32(&mut bytes, offset_of!(v4l2_frmivalenum, type_), 3).expect("fits");
        let union_at = offset_of!(v4l2_frmivalenum, __bindgen_anon_1);
        for (index, value) in [1u32, 30, 1, 5, 1, 1].into_iter().enumerate() {
            fields::write_u32(&mut bytes, union_at + index * 4, value).expect("fits");
        }
        assert_eq!(
            frame_interval(&bytes),
            Some(FrameInterval::Stepwise {
                min_numerator: 1,
                min_denominator: 30,
                max_numerator: 1,
                max_denominator: 5,
            })
        );

        // CONTINUOUS reads the same arm — the kernel documents its `step` as advisory,
        // not as a second shape.
        fields::write_u32(&mut bytes, offset_of!(v4l2_frmivalenum, type_), 2).expect("fits");
        assert!(matches!(
            frame_interval(&bytes),
            Some(FrameInterval::Stepwise {
                min_denominator: 30,
                ..
            })
        ));
    }

    #[test]
    fn a_step_too_wide_for_the_schema_saturates_toward_doing_less_not_more() {
        // The direction matters more than the value. Mapping an unrepresentable step to 0
        // would send it through `effective_step`'s "no usable step" path, which
        // substitutes 1 — the finest step in the table — so the control we understood
        // least would plan the largest sweep. Saturating leaves only the minimum
        // reachable.
        let mut bytes = PAN_ABSOLUTE.to_vec();
        fields::write_u64(&mut bytes, offset_of!(v4l2_query_ext_ctrl, step), u64::MAX)
            .expect("fits");
        let desc = control_desc(&bytes).expect("decodes");
        assert_eq!(desc.range.step, i64::MAX);
        assert_eq!(desc.range.effective_step(), i64::MAX);

        // Concretely, over this control's real ±468000 range: one reachable value, not
        // the 936001 that a step of 1 would have offered a sweep planner.
        let span = desc.range.max.saturating_sub(desc.range.min);
        assert_eq!(span / desc.range.effective_step(), 0);

        // A step that *does* fit is untouched — the saturation must not swallow real ones.
        let desc = control_desc(PAN_ABSOLUTE).expect("decodes");
        assert_eq!(desc.range.step, 3_600);
    }

    #[test]
    fn a_control_named_only_in_punctuation_has_no_handle_and_says_so() {
        // D2 refuses to invent a slug, because an invented handle collides silently.
        let mut bytes = PRIVACY.to_vec();
        let name_at = offset_of!(v4l2_query_ext_ctrl, name);
        bytes
            .get_mut(name_at..name_at + NAME_LEN)
            .expect("in range")
            .fill(0);
        bytes[name_at] = b'!';
        assert_eq!(control_desc(&bytes), None);
    }

    #[test]
    fn a_dimension_count_past_the_arrays_width_reads_the_array_not_past_it() {
        let mut bytes = ROI_RECT.to_vec();
        fields::write_u32(&mut bytes, offset_of!(v4l2_query_ext_ctrl, nr_of_dims), 9)
            .expect("fits");
        let desc = control_desc(&bytes).expect("decodes");
        assert_eq!(
            desc.dims.len(),
            4,
            "the kernel's dims[] is four wide however many the driver claims"
        );
    }

    #[test]
    fn scalars_read_the_arm_their_type_selects() {
        let mut bytes = vec![0u8; size_of::<v4l2_ext_control>()];
        let union_at = offset_of!(v4l2_ext_control, __bindgen_anon_1);
        // 245 — PF:4's out-of-range current, as the kernel would return it.
        fields::write_u32(&mut bytes, union_at, 245).expect("fits");
        assert_eq!(control_scalar(&bytes, 1), Some(ControlValue::Int(245)));

        // A negative 32-bit value must sign-extend rather than arrive as 4 billion.
        fields::write_u32(&mut bytes, union_at, u32::MAX).expect("fits");
        assert_eq!(control_scalar(&bytes, 1), Some(ControlValue::Int(-1)));
        // …and the 64-bit arm reads all eight bytes.
        assert_eq!(
            control_scalar(&bytes, CTRL_TYPE_INTEGER64),
            Some(ControlValue::Int(0xffff_ffff))
        );
        assert_eq!(control_scalar(&bytes[..4], 1), None);
    }

    #[test]
    fn a_scalar_write_fills_exactly_the_arm_the_matching_read_looks_in() {
        // The defect this rules out is silent in both directions: a write into the wrong
        // union arm leaves four bytes in a neighbouring field, the kernel reports success,
        // and the read-back looks in the arm nobody filled — so the value reads as
        // whatever was there before and nothing anywhere says a write went astray. The
        // round trip is asserted through the *pair* of functions rather than against a
        // transcribed offset, because agreeing with each other is the property.
        let mut bytes = vec![0u8; size_of::<v4l2_ext_control>()];
        let union_at = offset_of!(v4l2_ext_control, __bindgen_anon_1);

        for (control_type, value) in [
            (1u32, 245i64),           // INTEGER, PF:4's out-of-range current
            (1, -468_000),            // INTEGER, the OBSBOT's pan minimum
            (1, i64::from(i32::MIN)), // the narrow arm's edge
            (1, i64::from(i32::MAX)),
            (2, 1), // BOOLEAN
            (3, 3), // MENU
            (CTRL_TYPE_INTEGER64, i64::MAX),
            (CTRL_TYPE_INTEGER64, i64::MIN),
        ] {
            bytes.fill(0);
            match scalar_arm(control_type, value).expect("representable") {
                ScalarArm::Wide(wide) => fields::write_i64(&mut bytes, union_at, wide),
                ScalarArm::Narrow(narrow) => fields::write_i32(&mut bytes, union_at, narrow),
            }
            .expect("fits");
            assert_eq!(
                control_scalar(&bytes, control_type),
                Some(ControlValue::Int(value)),
                "type {control_type} value {value} did not survive the round trip"
            );
        }
    }

    #[test]
    fn a_value_wider_than_a_32_bit_control_is_refused_rather_than_truncated() {
        // Truncation is the dangerous answer: `i32::MAX + 1` would arrive as `i32::MIN`,
        // the device would take it, and the read-back would report a plausible clamp the
        // device never performed.
        assert_eq!(scalar_arm(1, i64::from(i32::MAX) + 1), None);
        assert_eq!(scalar_arm(1, i64::from(i32::MIN) - 1), None);
        // The 64-bit arm has no such limit, which is the other half of the claim.
        assert_eq!(
            scalar_arm(CTRL_TYPE_INTEGER64, i64::MAX),
            Some(ScalarArm::Wide(i64::MAX))
        );
    }

    // ------------------------------------------------------------- the streaming path
    //
    // These decoders have **no captured fixture**: the independent Python probe that
    // produced the `.bin` files in `fixtures/` never ran a stream, so there is no recorded
    // `DQBUF` reply to replay. Recorded rather than glossed over — the width assertion in
    // `the_bindings_and_the_captured_replies_agree_on_every_struct_width` therefore does
    // not cover `v4l2_buffer`, `v4l2_format` or `v4l2_streamparm`, and what stands in for
    // it is the round trip below: a reply built through the *same derived offsets* the
    // decoder reads, so the two agree about layout by construction, plus the R3 rung,
    // where a real driver fills the struct and the frames either decode or they do not.

    #[test]
    fn a_dequeued_buffer_reports_what_the_driver_wrote_and_when() {
        let mut bytes = vec![0u8; size_of::<v4l2_buffer>()];
        fields::write_u32(&mut bytes, offset_of!(v4l2_buffer, index), 2).expect("fits");
        fields::write_u32(&mut bytes, offset_of!(v4l2_buffer, bytesused), 91_234).expect("fits");
        fields::write_u32(&mut bytes, offset_of!(v4l2_buffer, sequence), 17).expect("fits");
        let stamp_at = offset_of!(v4l2_buffer, timestamp);
        fields::write_i64(&mut bytes, stamp_at + offset_of!(timeval, tv_sec), 1_700).expect("fits");
        fields::write_i64(&mut bytes, stamp_at + offset_of!(timeval, tv_usec), 250_000)
            .expect("fits");

        let buffer = dequeued(&bytes).expect("a whole reply decodes");
        assert_eq!(buffer.index, 2);
        assert_eq!(buffer.bytes_used, 91_234);
        assert_eq!(buffer.sequence, 17);
        assert_eq!(
            buffer.timestamp_us, 1_700_250_000,
            "seconds and microseconds are one timeline, not two fields"
        );
        assert!(!buffer.is_error());

        // The error flag, which is the reason a caller must not treat every dequeued
        // buffer as a frame.
        fields::write_u32(&mut bytes, offset_of!(v4l2_buffer, flags), BUF_FLAG_ERROR)
            .expect("fits");
        assert!(dequeued(&bytes).expect("decodes").is_error());

        // A short reply is a disagreement about the ABI, and decoding its readable prefix
        // would produce a plausible frame — the worst possible answer.
        assert!(dequeued(&bytes[..8]).is_none());
    }

    #[test]
    fn a_negotiated_format_decodes_out_of_the_pix_arm_of_the_union() {
        // The union is the interesting part: `pix` is one of eight arms, and reading the
        // wrong one yields numbers that look like a frame size.
        let mut bytes = vec![0u8; size_of::<v4l2_format>()];
        for (field, value) in [
            (offset_of!(v4l2_pix_format, width), 1_280u32),
            (offset_of!(v4l2_pix_format, height), 720),
            (
                offset_of!(v4l2_pix_format, pixelformat),
                PixelFormat::MJPG.to_fourcc(),
            ),
            (offset_of!(v4l2_pix_format, bytesperline), 0),
            (offset_of!(v4l2_pix_format, sizeimage), 1 << 21),
        ] {
            let at = pix_field(field).expect("inside the struct");
            fields::write_u32(&mut bytes, at, value).expect("fits");
        }

        let decoded = pix_format(&bytes).expect("a whole reply decodes");
        assert_eq!(decoded.pixel_format, PixelFormat::MJPG);
        assert_eq!((decoded.width, decoded.height), (1_280, 720));
        assert_eq!(decoded.bytes_per_line, 0, "a bitstream has no stride");
        assert_eq!(decoded.size_image, 1 << 21);
        assert!(pix_format(&bytes[..4]).is_none());

        // A fourcc nobody has named survives as itself [PF:1's rule, one type over].
        let at = pix_field(offset_of!(v4l2_pix_format, pixelformat)).expect("inside");
        fields::write_u32(&mut bytes, at, 0x3231_3841).expect("fits");
        let odd = pix_format(&bytes).expect("decodes");
        assert_eq!(odd.pixel_format.to_fourcc(), 0x3231_3841);
    }

    #[test]
    fn what_the_driver_said_about_the_interval_and_what_it_said_nothing_about_are_two_answers() {
        // Three replies, three answers, and until 2026-08-16 the first and the third were
        // the same value — a bare `None` that one line up became `FrameInterval::Unstated`,
        // whose committed documentation states as fact that the driver *cleared*
        // `V4L2_CAP_TIMEPERFRAME` (note **N199**). A driver that set the bit and wrote
        // `1/0` therefore got a value asserting the opposite of what it did, a CLI line
        // reading `(not offered)`, and a "did we misread anything" predicate answering no
        // for the one case where the answer was yes.
        let mut bytes = vec![0u8; size_of::<v4l2_streamparm>()];
        let per_frame = capture_parm_field(offset_of!(v4l2_captureparm, timeperframe))
            .expect("inside the struct");
        fields::write_u32(&mut bytes, per_frame + offset_of!(v4l2_fract, numerator), 1)
            .expect("fits");
        fields::write_u32(
            &mut bytes,
            per_frame + offset_of!(v4l2_fract, denominator),
            30,
        )
        .expect("fits");

        assert_eq!(
            capture_interval(&bytes),
            Some(ReportedInterval::NotOffered),
            "without the capability bit the field is not an interval, however it reads"
        );

        let capability =
            capture_parm_field(offset_of!(v4l2_captureparm, capability)).expect("inside");
        fields::write_u32(&mut bytes, capability, CAP_TIMEPERFRAME).expect("fits");
        assert_eq!(
            capture_interval(&bytes),
            Some(ReportedInterval::Offered(FrameInterval::Discrete {
                numerator: 1,
                denominator: 30
            }))
        );

        // A degenerate fraction with the bit *set* is the device's own answer and is
        // carried verbatim (D2): the driver said something, and it is not this build's to
        // replace with "the driver said nothing". Nothing downstream divides by it —
        // `FrameInterval::fps` answers `None` and `imaging::avi::interval_micros` refuses
        // it — which is what makes carrying it safe as well as honest.
        fields::write_u32(
            &mut bytes,
            per_frame + offset_of!(v4l2_fract, denominator),
            0,
        )
        .expect("fits");
        let degenerate = capture_interval(&bytes).expect("a whole reply");
        assert_eq!(
            degenerate,
            ReportedInterval::Offered(FrameInterval::Discrete {
                numerator: 1,
                denominator: 0
            })
        );
        assert_ne!(degenerate, ReportedInterval::NotOffered);

        // And a reply this build could not read at all is neither: it is our defect, and
        // the caller turns it into `short_reply` rather than into a fact about the device.
        assert_eq!(capture_interval(&bytes[..4]), None);
    }

    #[test]
    fn a_buffer_query_and_a_request_report_what_the_driver_granted() {
        let mut bytes = vec![0u8; size_of::<v4l2_buffer>()];
        fields::write_u32(&mut bytes, offset_of!(v4l2_buffer, m), 0x0001_0000).expect("fits");
        fields::write_u32(&mut bytes, offset_of!(v4l2_buffer, length), 4_147_200).expect("fits");
        assert_eq!(
            buffer_mapping(&bytes),
            Some(BufferMapping {
                offset: 0x0001_0000,
                length: 4_147_200
            })
        );

        let mut request = vec![0u8; size_of::<v4l2_requestbuffers>()];
        fields::write_u32(&mut request, offset_of!(v4l2_requestbuffers, count), 3).expect("fits");
        assert_eq!(
            granted_buffers(&request),
            Some(3),
            "the driver is free to grant fewer than were asked for"
        );
        // Zero is a *successful* answer meaning "none", not a failure, and the caller is
        // the one that turns it into an error naming what it asked for.
        fields::write_u32(&mut request, offset_of!(v4l2_requestbuffers, count), 0).expect("fits");
        assert_eq!(granted_buffers(&request), Some(0));
        assert_eq!(granted_buffers(&request[..2]), None);
    }

    #[test]
    fn a_payload_length_is_bounded_before_it_can_become_an_allocation() {
        // Rubric B10: `elem_size × elems` is device-supplied, so a lying driver is input.
        assert_eq!(payload_len(16, 1), Some(16));
        assert_eq!(payload_len(0, 1), None, "an empty payload is not a payload");
        assert_eq!(payload_len(4, 0), None);
        assert_eq!(
            payload_len(u32::MAX, u32::MAX),
            None,
            "the product must not wrap into something allocatable"
        );
        let cap = u32::try_from(limits::MAX_CONTROL_PAYLOAD_BYTES).expect("cap fits in u32");
        assert_eq!(payload_len(cap, 1), Some(limits::MAX_CONTROL_PAYLOAD_BYTES));
        assert_eq!(payload_len(cap, 2), None);
    }
}
