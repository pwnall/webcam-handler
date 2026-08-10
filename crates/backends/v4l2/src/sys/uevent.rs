//! The uevent netlink socket (design §2.5, docs/7 P4d).
//!
//! Hotplug notification without udev: `libudev` is LGPL and §2.8's allowlist rejects it,
//! so the kernel's own `NETLINK_KOBJECT_UEVENT` broadcast is the source. This module is
//! the socket — open it, wait on it, read one datagram out of it — and **nothing else**.
//! What a datagram *means* is [`crate::hotplug`]'s, which is a pure function over bytes
//! so the hostile-packet fixtures can drive it without a kernel.
//!
//! ## Why it lives under `src/sys/` with no `unsafe` in it
//!
//! The rule is about kernel-facing edges, not about the token: docs/7 P4d places the
//! socket "inside `src/sys/` like every unsafe **or kernel-facing** edge", and design
//! §2.5 says "the uevent socket code lives inside the same module for the same reason".
//! It happens to contain no `unsafe` block at all, because `rustix` is a *safe* wrapper
//! whose `sockaddr_nl` comes from `linux-raw-sys` — bindgen output over the kernel UAPI
//! headers, which is exactly what AGENTS' "no hand-declared kernel structs" asks for.
//! Writing this with `libc` would have cost four blocks and a hand-maintained
//! `sockaddr_nl` one crate further away. The count in [`super`]'s header is unchanged by
//! this file, and that is the finding rather than an accident.
//!
//! ## The four choices in this file, each measured
//!
//! Measured 2026-08-10 on kernel `7.0.0-29-generic` by an independent probe (see
//! `PF:21`), not by this code:
//!
//! - **No privilege.** `socket(AF_NETLINK, SOCK_DGRAM, NETLINK_KOBJECT_UEVENT)` and
//!   `bind` with `nl_groups = 1` both succeed with an empty effective capability set.
//!   Note N8 predicted `CAP_NET_ADMIN`; the prediction is disproved, and PF:21 carries
//!   the transcript. Nothing here asks for a capability, and nothing here should.
//! - **`nl_pid = 0`, not `getpid()`.** The kernel assigns a unique port id per socket
//!   when the field is zero. Binding `getpid()` works exactly once per process: the
//!   probe measured `EADDRINUSE` on the second socket, and `wchd` will hold one watch
//!   while a subscription opens another. `kobject-uevent`'s own example binds
//!   `process::id()`; this deliberately does not copy it.
//! - **`nl_groups = 1`, and only group 1.** Group 1 is the kernel's own broadcast in the
//!   `action@devpath\0KEY=VALUE\0…` form. Group 2 is `systemd-udevd`'s rebroadcast of
//!   the same events behind a binary `libudev` header — subscribing to both would double
//!   every event and make us depend on udev running, which is the dependency design §7
//!   removed.
//! - **Receive only.** The probe measured `EPERM` on an unprivileged multicast send and
//!   on a unicast to another netlink port, so a non-root local user cannot forge a
//!   packet into this socket. That sizes the threat honestly — the bytes are *untrusted*
//!   (root, udev, and a malicious USB device's strings all reach this buffer) without
//!   being any local user's to choose — and it does not retire rubric B10: the parser
//!   still treats every packet as hostile input.

use std::os::fd::{AsFd, OwnedFd};
use std::time::Instant;

use rustix::io::Errno;
use rustix::net::netlink::SocketAddrNetlink;
use rustix::net::{AddressFamily, RecvFlags, SocketFlags, SocketType, netlink};
use schema::error::{Error, Result};

use super::wait::{self, Ready};

/// The kernel's own uevent broadcast group. See the header for why not group 2.
const KERNEL_UEVENT_GROUP: u32 = 1;

/// Let the kernel pick the port id, so two sockets can coexist in one process.
const KERNEL_ASSIGNS_THE_PORT: u32 = 0;

/// A bound, non-blocking `NETLINK_KOBJECT_UEVENT` socket.
///
/// Closes on drop. There is no `send` half by construction: the kernel refuses one from
/// an unprivileged process anyway, and a socket that cannot talk cannot be talked into
/// anything.
#[derive(Debug)]
pub(crate) struct UeventSocket {
    socket: OwnedFd,
}

/// What one [`UeventSocket::recv_into`] saw.
///
/// Four outcomes and only one of them is an error, which is the point: a watch that ends
/// because a neighbouring subsystem broadcast something odd is a watch that stops working
/// on a busy machine (rubric B10).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Received {
    /// A whole datagram, occupying the first `len` bytes of the caller's buffer.
    Packet {
        /// How much of the buffer the datagram filled.
        len: usize,
    },
    /// A datagram longer than the buffer, dropped whole rather than parsed from its
    /// prefix.
    ///
    /// `bytes` is the kernel's report of the datagram's *real* size under `MSG_TRUNC`,
    /// which is a number the sender chose: it is carried for diagnosis and is never used
    /// as a length or an index (rubric B10).
    Oversized {
        /// The size the kernel reported, which may exceed the buffer.
        bytes: usize,
    },
    /// The kernel dropped broadcasts because this socket's receive queue was full
    /// (`ENOBUFS`).
    ///
    /// Not an error. A lost packet is a lost *trigger*: what the watch does about a
    /// trigger is re-read the device tree, and the tree is whatever it is by then.
    Overrun,
    /// Nothing queued (`EAGAIN`), so a drain loop is done.
    Empty,
}

impl UeventSocket {
    /// Open and bind the socket.
    ///
    /// # Errors
    ///
    /// [`Error::DeviceIo`] naming the call and carrying its `errno`. Deliberately *not*
    /// [`Error::PermissionDenied`] on `EPERM`: that variant carries a device node path
    /// and a "join the `video` group" hint, and neither would be true of a netlink
    /// socket. A refusal here is the kernel refusing us, with no more specific home in
    /// D13 — which is what [`Error::DeviceIo`] is for (note N4).
    pub(crate) fn open() -> Result<UeventSocket> {
        let socket = rustix::net::socket_with(
            AddressFamily::NETLINK,
            SocketType::DGRAM,
            // CLOEXEC so a camera actor's `Command` does not hand a daemon's watch to a
            // child; NONBLOCK so the drain below can never block behind `poll`'s answer.
            SocketFlags::CLOEXEC | SocketFlags::NONBLOCK,
            Some(netlink::KOBJECT_UEVENT),
        )
        .map_err(|errno| {
            socket_error(
                "socket(AF_NETLINK, SOCK_DGRAM, NETLINK_KOBJECT_UEVENT)",
                errno,
            )
        })?;

        rustix::net::bind(
            &socket,
            &SocketAddrNetlink::new(KERNEL_ASSIGNS_THE_PORT, KERNEL_UEVENT_GROUP),
        )
        .map_err(|errno| socket_error("bind(AF_NETLINK, nl_pid=0, nl_groups=1)", errno))?;

        Ok(UeventSocket { socket })
    }

    /// A socket over an already-open datagram descriptor.
    ///
    /// For tests that need to *put* packets into one: the kernel refuses an unprivileged
    /// send on this netlink protocol \[PF:21\], so the drain's arithmetic is exercised over
    /// a `UnixDatagram` pair instead — which it does not care about, because `recv` with
    /// `MSG_TRUNC` behaves the same on any datagram socket.
    #[cfg(test)]
    pub(crate) fn from_datagram_fd(socket: OwnedFd) -> UeventSocket {
        UeventSocket { socket }
    }

    /// Wait until a packet is queued, or until `deadline`.
    ///
    /// `Ok(false)` means the deadline arrived first, which is an answer rather than a
    /// failure (E3) and is what `HotplugWatch::next_event` turns into `Ok(None)`.
    ///
    /// # Errors
    ///
    /// [`Error::DeviceIo`] for a `poll` failure that is not an interruption, and for the
    /// hangup a netlink socket has no business reporting. `POLLNVAL` cannot arise —
    /// [`wait::until_readable`] takes a borrow of this socket's own `OwnedFd`, so the
    /// descriptor is open for the whole call by signature — and a `POLLHUP` from a
    /// broadcast socket is a kernel this build does not understand rather than news about
    /// a camera, so neither may be spelled [`Error::DeviceGone`].
    pub(crate) fn wait_readable(&self, deadline: Instant) -> Result<bool> {
        match wait::until_readable(self.socket.as_fd(), "poll(uevent socket)", deadline)? {
            Ready::Timeout => Ok(false),
            Ready::Readable => Ok(true),
            Ready::HangUp => Err(Error::DeviceIo {
                operation: "poll(uevent socket)".to_owned(),
                errno: None,
                message: "the uevent netlink socket reported a hangup".to_owned(),
            }),
        }
    }

    /// Read one queued datagram into `buf`, without blocking.
    ///
    /// # Errors
    ///
    /// [`Error::DeviceIo`] for a `recv` failure that is not one of the three ordinary
    /// outcomes ([`Received::Empty`], [`Received::Overrun`], an interruption).
    pub(crate) fn recv_into(&self, buf: &mut [u8]) -> Result<Received> {
        // MSG_TRUNC makes the kernel report the datagram's own size rather than how much
        // of it fitted, which is the only way to tell "a packet that fits" from "the
        // first 8 KiB of a bigger one". MSG_DONTWAIT is belt to the socket's braces: the
        // descriptor is already O_NONBLOCK, and a caller that ever clears that must not
        // silently acquire a blocking read here.
        let outcome = rustix::net::recv(
            &self.socket,
            &mut *buf,
            RecvFlags::DONTWAIT | RecvFlags::TRUNC,
        );
        let (filled, datagram) = match outcome {
            Ok(sizes) => sizes,
            Err(errno) if errno == Errno::AGAIN => return Ok(Received::Empty),
            // The receive queue overflowed and the kernel threw broadcasts away. The
            // socket keeps working; the errno is reported once, on the next `recv`.
            Err(errno) if errno == Errno::NOBUFS => return Ok(Received::Overrun),
            // The socket is non-blocking, so `EINTR` cannot mean "a wait was cut short".
            // Ending this drain one packet early costs a trigger, and the descriptor is
            // still readable, so the next wait returns immediately.
            Err(errno) if errno == Errno::INTR => return Ok(Received::Empty),
            Err(errno) => return Err(socket_error("recv(uevent socket)", errno)),
        };

        // `datagram` is the sender's number, so it is checked before it is believed —
        // the same discipline every device-supplied length in this crate gets (rubric
        // B10). A packet that did not fit is dropped whole: a truncated key/value block
        // parses into a *plausible* event, and "half a packet" must never be spelled the
        // same way as "a packet".
        if datagram > buf.len() {
            return Ok(Received::Oversized { bytes: datagram });
        }
        Ok(Received::Packet { len: filled })
    }
}

/// Map a netlink socket failure onto the D13 registry.
fn socket_error(operation: &str, errno: Errno) -> Error {
    Error::DeviceIo {
        operation: operation.to_owned(),
        errno: Some(errno.raw_os_error()),
        message: errno.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use schema::limits;

    use super::*;

    /// The bit `CAP_NET_ADMIN` occupies in a capability mask (`capability.h`).
    const CAP_NET_ADMIN: u32 = 12;

    /// This process's effective capability mask, from `/proc/self/status`.
    fn effective_capabilities() -> u64 {
        let status = std::fs::read_to_string("/proc/self/status").expect("/proc/self/status");
        let line = status
            .lines()
            .find_map(|line| line.strip_prefix("CapEff:"))
            .expect("CapEff is present on every Linux this crate builds for");
        u64::from_str_radix(line.trim(), 16).expect("CapEff is a hexadecimal mask")
    }

    #[test]
    fn the_uevent_socket_binds_with_no_cap_net_admin_which_is_what_pf21_measured() {
        // Design §8 item 10, and the standing regression for PF:21. Note N8 granted
        // `CAP_NET_ADMIN` on a prediction that binding this socket needs it; an
        // independent probe disproved that on 2026-08-10, and this is what notices if a
        // kernel upgrade brings the prediction back.
        //
        // The first assertion is what stops the second one passing vacuously: a run that
        // *held* the capability could not measure whether the bind needs it, and "we did
        // not try unprivileged" must never be spelled the same way as "unprivileged
        // works" (AGENTS rule 7, applied to us rather than to the device).
        let effective = effective_capabilities();
        assert_eq!(
            effective & (1 << CAP_NET_ADMIN),
            0,
            "this process holds CAP_NET_ADMIN (CapEff {effective:#x}), so this run cannot \
             measure whether binding NETLINK_KOBJECT_UEVENT needs it"
        );

        let socket = UeventSocket::open().expect("bind NETLINK_KOBJECT_UEVENT group 1");

        // And a second one, in the same process: `nl_pid = 0` is what makes that work,
        // and the probe measured `EADDRINUSE` for the `getpid()` spelling the parser
        // crate's own example uses.
        let second = UeventSocket::open().expect("a second unprivileged socket");
        drop(second);
        drop(socket);
    }

    #[test]
    fn a_quiet_socket_answers_at_its_deadline_rather_than_erroring_or_hanging() {
        // E3, and the shape the battery's hotplug arm bounds: a timeout is an answer.
        // Both directions of the *wait* live in `sys::wait`'s socketpair test; what this
        // adds is that a real netlink socket with nothing on it takes that path, and
        // takes it within its deadline rather than blocking on the kernel's own idea of
        // one.
        let socket = UeventSocket::open().expect("bind NETLINK_KOBJECT_UEVENT group 1");
        let started = Instant::now();
        let ready = socket
            .wait_readable(started + Duration::from_millis(20))
            .expect("a quiet socket is not a failure");
        assert!(
            !ready,
            "a quiet machine broadcast a uevent during this test"
        );
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "a 20 ms deadline blocked for {:?}",
            started.elapsed()
        );
    }

    #[test]
    fn a_quiet_socket_reads_empty_rather_than_a_zero_length_packet() {
        // The inverse matters: a `recv` that reported `Packet { len: 0 }` on an empty
        // queue would hand the parser a zero-length buffer forever, and the parser would
        // dutifully call it unreadable — a busy loop that looks like a working drain.
        let socket = UeventSocket::open().expect("bind NETLINK_KOBJECT_UEVENT group 1");
        let mut buf = vec![0u8; limits::UEVENT_PACKET_BYTES];
        assert_eq!(
            socket
                .recv_into(&mut buf)
                .expect("an empty queue is not a failure"),
            Received::Empty
        );
    }

    #[test]
    fn a_datagram_longer_than_the_buffer_is_dropped_whole_rather_than_read_as_a_prefix() {
        // The one hostile-shape claim this module owns rather than the parser: the
        // *socket* must not hand out a prefix. Netlink refuses an unprivileged send
        // (PF:21), so the datagram is arranged on a Unix datagram socketpair instead —
        // the outcome under test is `recv`'s arithmetic over `MSG_TRUNC`, which does not
        // care which address family produced the packet.
        //
        // `OwnedFd::from(near)` and not `from_raw_fd`: a test in this directory *may*
        // say `unsafe`, and the counted table in `sys/mod.rs` is a claim about the whole
        // tree, so a block that a safe conversion covers is a row nobody should have to
        // read.
        let (near, far) = std::os::unix::net::UnixDatagram::pair().expect("a datagram pair");
        far.send(&[0xAB; 64]).expect("send a 64-byte datagram");
        let socket = UeventSocket {
            socket: OwnedFd::from(near),
        };

        let mut small = [0u8; 16];
        assert_eq!(
            socket
                .recv_into(&mut small)
                .expect("a big datagram is not a failure"),
            Received::Oversized { bytes: 64 },
            "a 64-byte datagram must not be delivered as a 16-byte packet"
        );

        // …while one that fits arrives whole, which is what makes the refusal above a
        // size rule rather than a broken read.
        far.send(&[0xCD; 8]).expect("send an 8-byte datagram");
        assert_eq!(
            socket.recv_into(&mut small).expect("a small datagram"),
            Received::Packet { len: 8 }
        );
    }
}
