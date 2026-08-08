//! Reading the device tree from sysfs (design §2.5).
//!
//! No udev: `libudev` is LGPL and §2.8's allowlist rejects it, so the two facts we need —
//! which `/dev/videoN` nodes exist, and which USB interface each one hangs off — are read
//! from `/sys/class/video4linux/` directly. That directory is a kernel-maintained
//! symlink farm and reading it is thirty lines.
//!
//! ## Why the interface path and not `bus_info`
//!
//! `VIDIOC_QUERYCAP` reports `bus_info` per USB *device*. Both Chicony logical cameras
//! answer `usb-0000:00:14.0-4` even though they are separate interfaces hosting separate
//! capture nodes with different formats \[PF:13\]. Grouping on `bus_info` would collapse
//! two cameras into one; grouping on the interface path (`3-4:1.0` vs `3-4:1.2`)
//! separates them, which is what PF:7 measured and what this module implements.
//!
//! ## Everything here is best-effort except the node list
//!
//! A node with no readable `device` link still enumerates — it simply groups alone. Sysfs
//! layouts vary with bus type, and a camera on a bus we have never seen should list, not
//! vanish (E2: the device is the authority, and a missing sysfs attribute is our gap, not
//! its refusal).

use camino::{Utf8Path, Utf8PathBuf};
use schema::camera::UsbId;
use schema::error::{Error, Result};

/// Where the kernel lists V4L2 nodes.
const CLASS_DIR: &str = "/sys/class/video4linux";

/// One `/dev/videoN` node as sysfs describes it, before any ioctl.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SysfsNode {
    /// The device node under `/dev`.
    pub(crate) dev_path: Utf8PathBuf,
    /// The `videoN` name, which is also the sort key: `video10` must not sort before
    /// `video2` when a machine has that many.
    pub(crate) name: String,
    /// The bus interface this node hangs off — `3-4:1.2` for USB. `None` when the link
    /// was unreadable, in which case the node groups alone.
    pub(crate) interface: Option<String>,
    /// `VID:PID` from the USB device above the interface.
    pub(crate) usb_id: Option<UsbId>,
    /// The USB device's serial, when it reports a non-empty one \[PF:8\].
    pub(crate) serial: Option<String>,
}

impl SysfsNode {
    /// The grouping key: the interface path, or the node's own name when there is none.
    ///
    /// Falling back to the name rather than to a shared bucket is deliberate — two nodes
    /// we cannot place are two cameras we cannot place, and merging them would invent a
    /// grouping the kernel never claimed.
    pub(crate) fn group_key(&self) -> &str {
        self.interface.as_deref().unwrap_or(&self.name)
    }
}

/// Every V4L2 node the kernel currently lists, in numeric node order.
///
/// # Errors
///
/// [`Error::StorageIo`] when `/sys/class/video4linux` cannot be read at all. An empty
/// directory is not an error: a machine with no camera is a legitimate machine, and the
/// caller reports "no cameras" rather than a failure.
pub(crate) fn nodes() -> Result<Vec<SysfsNode>> {
    nodes_in(Utf8Path::new(CLASS_DIR))
}

/// [`nodes`] against an arbitrary class directory, so the tests can build one.
pub(crate) fn nodes_in(class_dir: &Utf8Path) -> Result<Vec<SysfsNode>> {
    let entries = match std::fs::read_dir(class_dir) {
        Ok(entries) => entries,
        // No directory means no V4L2 subsystem loaded, which is "no cameras" rather than
        // a failure to enumerate.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(storage_error(class_dir, &error)),
    };

    let mut nodes = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| storage_error(class_dir, &error))?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if !name.starts_with("video") {
            continue;
        }
        let link = class_dir.join(&name);
        nodes.push(node_from(&link, name));
    }
    nodes.sort_by(|a, b| node_order(&a.name).cmp(&node_order(&b.name)).then(a.name.cmp(&b.name)));
    Ok(nodes)
}

/// Describe one node from its `/sys/class/video4linux/<name>` entry.
fn node_from(link: &Utf8Path, name: String) -> SysfsNode {
    // `<link>/device` points at the interface (`…/3-4:1.2`); its parent is the USB
    // device carrying the ids and the serial.
    let interface_dir = std::fs::canonicalize(link.join("device").as_std_path())
        .ok()
        .and_then(|path| Utf8PathBuf::from_path_buf(path).ok());
    let interface = interface_dir
        .as_deref()
        .and_then(Utf8Path::file_name)
        .map(str::to_owned);
    let usb_dir = interface_dir.as_deref().and_then(Utf8Path::parent);

    SysfsNode {
        dev_path: Utf8PathBuf::from(format!("/dev/{name}")),
        name,
        interface,
        usb_id: usb_dir.and_then(usb_id),
        serial: usb_dir.and_then(|dir| attribute(dir, "serial")),
    }
}

/// The `videoN` suffix as a number, so ordering is numeric rather than lexicographic.
fn node_order(name: &str) -> u32 {
    name.strip_prefix("video")
        .and_then(|n| n.parse::<u32>().ok())
        .unwrap_or(u32::MAX)
}

/// `idVendor`/`idProduct` from a USB device directory.
fn usb_id(usb_dir: &Utf8Path) -> Option<UsbId> {
    let vendor = u16::from_str_radix(&attribute(usb_dir, "idVendor")?, 16).ok()?;
    let product = u16::from_str_radix(&attribute(usb_dir, "idProduct")?, 16).ok()?;
    Some(UsbId { vendor, product })
}

/// A sysfs attribute, trimmed. `None` when it is missing, unreadable, **or empty**.
///
/// The empty case is load-bearing and was measured: the OBSBOT's `serial` file exists and
/// contains nothing. PF:8 says the OBSBOT "reports none", and a present-but-empty file is
/// how it says so — an empty string treated as a serial would make `CameraFingerprint`
/// carry `Some("")`, which compares unequal to the `None` a differently-reported absence
/// produces, and `calibrate apply` would refuse the camera that recorded the session.
fn attribute(dir: &Utf8Path, name: &str) -> Option<String> {
    let text = std::fs::read_to_string(dir.join(name).as_std_path()).ok()?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_owned())
    }
}

fn storage_error(path: &Utf8Path, error: &std::io::Error) -> Error {
    Error::StorageIo {
        path: path.to_owned(),
        errno: error.raw_os_error(),
        message: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::symlink;

    use super::*;

    /// Build a sysfs-shaped tree: a class directory of symlinks into a device tree.
    ///
    /// The shape is the measured one — `…/usb3/3-4/3-4:1.0/video4linux/video0` — because
    /// a test against an invented layout would prove nothing about the layout we read.
    struct FakeSysfs {
        _dir: tempfile::TempDir,
        root: Utf8PathBuf,
    }

    impl FakeSysfs {
        fn new() -> FakeSysfs {
            let dir = tempfile::tempdir().expect("temp dir");
            let root = Utf8PathBuf::from_path_buf(dir.path().to_owned()).expect("utf-8 temp dir");
            std::fs::create_dir_all(root.join("class").as_std_path()).expect("class dir");
            FakeSysfs { _dir: dir, root }
        }

        fn class_dir(&self) -> Utf8PathBuf {
            self.root.join("class")
        }

        /// Add a node on `interface` under USB device `usb`, with the given attributes.
        fn add(&self, node: &str, usb: &str, interface: &str, attrs: &[(&str, &str)]) {
            let usb_dir = self.root.join("devices").join(usb);
            let iface_dir = usb_dir.join(interface);
            let node_dir = iface_dir.join("video4linux").join(node);
            std::fs::create_dir_all(node_dir.as_std_path()).expect("node dir");
            for (name, value) in attrs {
                std::fs::write(usb_dir.join(name).as_std_path(), value).expect("attribute");
            }
            symlink(
                node_dir.as_std_path(),
                self.class_dir().join(node).as_std_path(),
            )
            .expect("class symlink");
            // The kernel's `device` link points from the node at its interface.
            symlink(
                iface_dir.as_std_path(),
                node_dir.join("device").as_std_path(),
            )
            .expect("device symlink");
        }
    }

    /// The measured topology of the development machine [PF:7, PF:13].
    fn seed_hardware() -> FakeSysfs {
        let fs = FakeSysfs::new();
        let chicony = &[
            ("idVendor", "04f2"),
            ("idProduct", "b83c"),
            ("serial", "0001"),
        ];
        // Both Chicony logical cameras live on one USB device, on different interfaces.
        fs.add("video0", "3-4", "3-4:1.0", chicony);
        fs.add("video1", "3-4", "3-4:1.0", chicony);
        fs.add("video2", "3-4", "3-4:1.2", chicony);
        fs.add("video3", "3-4", "3-4:1.2", chicony);
        // The OBSBOT's serial file exists and is empty — measured, and the reason
        // `attribute` treats empty as absent.
        let obsbot = &[("idVendor", "3564"), ("idProduct", "ff02"), ("serial", "\n")];
        fs.add("video4", "3-1", "3-1:1.0", obsbot);
        fs.add("video5", "3-1", "3-1:1.0", obsbot);
        fs
    }

    #[test]
    fn nodes_group_by_usb_interface_not_by_usb_device() {
        // PF:7 and PF:13 together: one USB device, two cameras, and the only field that
        // tells them apart is the interface path.
        let fs = seed_hardware();
        let nodes = nodes_in(&fs.class_dir()).expect("the class directory reads");
        assert_eq!(nodes.len(), 6);

        let keys: Vec<&str> = nodes.iter().map(SysfsNode::group_key).collect();
        assert_eq!(
            keys,
            vec!["3-4:1.0", "3-4:1.0", "3-4:1.2", "3-4:1.2", "3-1:1.0", "3-1:1.0"]
        );
        let distinct: std::collections::BTreeSet<&str> = keys.into_iter().collect();
        assert_eq!(
            distinct.len(),
            3,
            "six nodes on two USB devices are three cameras"
        );
    }

    #[test]
    fn nodes_come_back_in_numeric_order_not_lexicographic_order() {
        let fs = FakeSysfs::new();
        for node in ["video10", "video2", "video1"] {
            fs.add(node, "1-1", "1-1:1.0", &[]);
        }
        let names: Vec<String> = nodes_in(&fs.class_dir())
            .expect("reads")
            .into_iter()
            .map(|n| n.name)
            .collect();
        assert_eq!(names, vec!["video1", "video2", "video10"]);
    }

    #[test]
    fn an_empty_serial_file_is_an_absent_serial() {
        // PF:8, as the filesystem actually presents it. `Some("")` would compare unequal
        // to `None` in `CameraFingerprint::differing_fields`, so a session recorded
        // before a kernel that spells absence differently would stop applying.
        let fs = seed_hardware();
        let nodes = nodes_in(&fs.class_dir()).expect("reads");
        let obsbot = nodes
            .iter()
            .find(|n| n.name == "video4")
            .expect("the OBSBOT node");
        assert_eq!(obsbot.serial, None);
        assert_eq!(
            obsbot.usb_id,
            Some(UsbId {
                vendor: 0x3564,
                product: 0xff02
            })
        );

        let chicony = nodes
            .iter()
            .find(|n| n.name == "video0")
            .expect("the Chicony node");
        assert_eq!(chicony.serial.as_deref(), Some("0001"));
    }

    #[test]
    fn a_node_with_no_device_link_groups_alone_rather_than_with_the_others() {
        // Sysfs layouts vary by bus. A node we cannot place must still list, and must not
        // be merged into somebody else's camera.
        let fs = seed_hardware();
        let orphan = fs.class_dir().join("video9");
        std::fs::create_dir_all(orphan.as_std_path()).expect("orphan dir");

        let nodes = nodes_in(&fs.class_dir()).expect("reads");
        let orphan = nodes
            .iter()
            .find(|n| n.name == "video9")
            .expect("the orphan still lists");
        assert_eq!(orphan.interface, None);
        assert_eq!(orphan.group_key(), "video9");
        assert!(
            nodes
                .iter()
                .filter(|n| n.group_key() == "video9")
                .count()
                == 1,
            "an unplaceable node is its own group"
        );
    }

    #[test]
    fn a_missing_class_directory_is_no_cameras_rather_than_a_failure() {
        let fs = FakeSysfs::new();
        let missing = fs.root.join("no-such-subsystem");
        assert_eq!(nodes_in(&missing).expect("absence is not an error"), vec![]);
    }

    #[test]
    fn non_video_entries_in_the_class_directory_are_ignored() {
        let fs = FakeSysfs::new();
        fs.add("video0", "1-1", "1-1:1.0", &[]);
        std::fs::write(fs.class_dir().join("v4l-subdev0").as_std_path(), b"")
            .expect("a subdev entry");
        let nodes = nodes_in(&fs.class_dir()).expect("reads");
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].name, "video0");
    }

    #[test]
    fn the_real_class_directory_reads_without_erroring() {
        // Not an assertion about what is attached — CI has no camera. It asserts the
        // reader survives whatever this host has, including nothing.
        let nodes = nodes().expect("the real /sys/class/video4linux reads or is absent");
        for node in &nodes {
            assert!(node.dev_path.as_str().starts_with("/dev/video"), "{node:?}");
        }
    }
}
