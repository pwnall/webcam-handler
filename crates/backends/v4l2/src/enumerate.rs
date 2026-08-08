//! Turning device nodes into cameras (design D1, §2.5).
//!
//! The grouping rule, and the whole of it: **nodes belong to the same camera when they
//! hang off the same bus interface**. Not the same USB device — one device hosts several
//! logical cameras \[PF:7\] — and not adjacent numbering, and above all not the same
//! `bus_info`, which is per-device and therefore identical for cameras that are not
//! \[PF:13\].
//!
//! What a node *is* comes from `device_caps`, never from being the lowest-numbered member
//! of its group: the Chicony IR camera's capture node is `video2` and its metadata node is
//! `video3`, while the RGB camera's are `video0` and `video1`, and only the capability
//! word says which is which \[PF:7\].
//!
//! The grouping itself is a pure function of the node list — [`group`] takes values and
//! returns values — so the rule is testable without a device, and the tests below pin it
//! against the measured topology of the seed hardware.

use schema::backend::BackendKind;
use schema::camera::{
    CameraFingerprint, CameraId, CameraInfo, DeviceNode, NodeKind, UsbId, assign_ids,
};
use schema::limits;

/// One node, described enough to be grouped: the sysfs facts plus what `QUERYCAP` said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProbedNode {
    /// The `/dev/videoN` path.
    pub(crate) dev_path: camino::Utf8PathBuf,
    /// The bus interface path, or the node name when sysfs would not say.
    pub(crate) group_key: String,
    /// `VID:PID`, when the node sits on USB.
    pub(crate) usb_id: Option<UsbId>,
    /// The USB serial, when there is a non-empty one \[PF:8\].
    pub(crate) serial: Option<String>,
    /// `QUERYCAP`'s driver name.
    pub(crate) driver: String,
    /// `QUERYCAP`'s card name, verbatim.
    pub(crate) card: String,
    /// `QUERYCAP`'s bus info. Recorded, never grouped on \[PF:13\].
    pub(crate) bus_info: String,
    /// The whole device's capability word.
    pub(crate) capabilities: u32,
    /// **This node's** capability word — the only authority on what it is.
    pub(crate) device_caps: u32,
}

impl ProbedNode {
    fn to_device_node(&self) -> DeviceNode {
        DeviceNode {
            path: self.dev_path.clone(),
            kind: NodeKind::from_device_caps(self.device_caps),
            device_caps: self.device_caps,
            capabilities: self.capabilities,
        }
    }
}

/// Group probed nodes into cameras and assign D1 ids.
///
/// Pure: no filesystem, no ioctl. Groups keep the order of their first node, so the
/// camera list follows node numbering even though the grouping does not.
pub(crate) fn group(nodes: &[ProbedNode]) -> Vec<CameraInfo> {
    // A `Vec` of buckets rather than a map, because insertion order *is* the camera
    // order and a `BTreeMap` keyed on `3-4:1.0` would sort `3-1:1.0` ahead of it —
    // reordering the camera list by an accident of USB port numbering.
    let mut groups: Vec<(&str, Vec<&ProbedNode>)> = Vec::new();
    for node in nodes {
        match groups.iter_mut().find(|(key, _)| *key == node.group_key) {
            Some((_, members)) => {
                // A group larger than any real topology is a grouping we have stopped
                // believing (`limits::MAX_NODES_PER_CAMERA`); the camera still lists,
                // with the nodes we are willing to attribute to it.
                if members.len() < limits::MAX_NODES_PER_CAMERA {
                    members.push(node);
                }
            }
            None => groups.push((node.group_key.as_str(), vec![node])),
        }
    }

    // Resolving each group's representative *before* ids are assigned keeps the two lists
    // the same length by construction, and drops the empty bucket `group` cannot produce
    // without needing a sentinel to stand in for it.
    let groups: Vec<(&str, Vec<&ProbedNode>, &ProbedNode)> = groups
        .into_iter()
        .filter_map(|(key, members)| {
            let head = representative(&members)?;
            Some((key, members, head))
        })
        .collect();

    let cards: Vec<String> = groups
        .iter()
        .map(|(_, _, head)| head.card.clone())
        .collect();
    let ids = assign_ids(&cards);

    groups
        .into_iter()
        .zip(ids)
        .map(|((key, members, head), id)| info_for(key, &members, head, id))
        .collect()
}

/// The node whose `QUERYCAP` answer describes the camera.
///
/// The capture node when there is one, else the first — a metadata-only group is a real
/// shape, and it is listed rather than hidden so that streaming it can be a typed refusal
/// instead of a camera that mysteriously does not exist. `None` only for an empty group,
/// which [`group`] does not construct.
fn representative<'a>(members: &[&'a ProbedNode]) -> Option<&'a ProbedNode> {
    members
        .iter()
        .copied()
        .find(|node| node.device_caps & schema::camera::CAP_VIDEO_CAPTURE != 0)
        .or_else(|| members.first().copied())
}

fn info_for(
    group_key: &str,
    members: &[&ProbedNode],
    head: &ProbedNode,
    id: CameraId,
) -> CameraInfo {
    CameraInfo {
        id,
        fingerprint: CameraFingerprint {
            // The interface path, not `bus_info`: a fingerprint built on `bus_info` could
            // not tell the Chicony IR camera from the RGB one, and `calibrate apply`
            // would replay an IR session onto the RGB sensor [PF:13].
            bus_path: group_key.to_owned(),
            usb_id: head.usb_id,
            card: head.card.clone(),
            driver: head.driver.clone(),
            serial: head.serial.clone(),
        },
        card: head.card.clone(),
        driver: head.driver.clone(),
        bus_info: head.bus_info.clone(),
        nodes: members.iter().map(|node| node.to_device_node()).collect(),
        backend: BackendKind::V4l2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The measured `device_caps` words from the development machine.
    const CAPTURE_CAPS: u32 = 0x0420_0001;
    const METADATA_CAPS: u32 = 0x04a0_0000;
    const DEVICE_CAPS: u32 = 0x84a0_0001;

    fn node(
        dev: &str,
        group_key: &str,
        card: &str,
        bus_info: &str,
        device_caps: u32,
        serial: Option<&str>,
    ) -> ProbedNode {
        ProbedNode {
            dev_path: camino::Utf8PathBuf::from(dev),
            group_key: group_key.to_owned(),
            usb_id: Some(UsbId {
                vendor: 0x04f2,
                product: 0xb83c,
            }),
            serial: serial.map(str::to_owned),
            driver: "uvcvideo".to_owned(),
            card: card.to_owned(),
            bus_info: bus_info.to_owned(),
            capabilities: DEVICE_CAPS,
            device_caps,
        }
    }

    /// The development machine's six nodes, exactly as measured (PF:7, PF:13).
    fn seed_hardware() -> Vec<ProbedNode> {
        let chicony_bus = "usb-0000:00:14.0-4";
        vec![
            node(
                "/dev/video0",
                "3-4:1.0",
                "Integrated Camera: Integrated C",
                chicony_bus,
                CAPTURE_CAPS,
                Some("0001"),
            ),
            node(
                "/dev/video1",
                "3-4:1.0",
                "Integrated Camera: Integrated C",
                chicony_bus,
                METADATA_CAPS,
                Some("0001"),
            ),
            node(
                "/dev/video2",
                "3-4:1.2",
                "Integrated Camera: Integrated I",
                chicony_bus,
                CAPTURE_CAPS,
                Some("0001"),
            ),
            node(
                "/dev/video3",
                "3-4:1.2",
                "Integrated Camera: Integrated I",
                chicony_bus,
                METADATA_CAPS,
                Some("0001"),
            ),
            node(
                "/dev/video4",
                "3-1:1.0",
                "OBSBOT Tiny 3: OBSBOT Tiny 3 St",
                "usb-0000:00:14.0-1",
                CAPTURE_CAPS,
                None,
            ),
            node(
                "/dev/video5",
                "3-1:1.0",
                "OBSBOT Tiny 3: OBSBOT Tiny 3 St",
                "usb-0000:00:14.0-1",
                METADATA_CAPS,
                None,
            ),
        ]
    }

    #[test]
    fn six_nodes_on_two_usb_devices_are_three_cameras() {
        let cameras = group(&seed_hardware());
        assert_eq!(cameras.len(), 3);
        assert_eq!(
            cameras
                .iter()
                .map(|c| c.fingerprint.bus_path.as_str())
                .collect::<Vec<_>>(),
            vec!["3-4:1.0", "3-4:1.2", "3-1:1.0"]
        );
        for camera in &cameras {
            assert_eq!(camera.nodes.len(), 2);
            assert_eq!(camera.backend, BackendKind::V4l2);
        }
    }

    #[test]
    fn two_cameras_sharing_one_bus_info_stay_two_cameras() {
        // PF:13, as the grouping rule's counter-example: `bus_info` is identical for the
        // Chicony RGB and IR cameras, so anything grouping on it would produce two.
        let cameras = group(&seed_hardware());
        let chicony: Vec<&CameraInfo> = cameras
            .iter()
            .filter(|c| c.bus_info == "usb-0000:00:14.0-4")
            .collect();
        assert_eq!(chicony.len(), 2, "one bus_info, two cameras");
        assert_ne!(chicony[0].id, chicony[1].id);
        assert_ne!(chicony[0].fingerprint, chicony[1].fingerprint);
        assert!(
            !chicony[0].fingerprint.matches(&chicony[1].fingerprint),
            "an IR session must not apply to the RGB sensor"
        );
        assert_eq!(
            chicony[0]
                .fingerprint
                .differing_fields(&chicony[1].fingerprint),
            vec!["bus_path", "card"]
        );
    }

    #[test]
    fn the_capture_node_is_found_by_capability_not_by_numbering() {
        // PF:7. The IR camera's capture node is video2 and its metadata node is video3;
        // "the lowest-numbered node" would be right here by luck and wrong elsewhere, so
        // the classification reads `device_caps`.
        let cameras = group(&seed_hardware());
        let ir = cameras
            .iter()
            .find(|c| c.card.ends_with('I'))
            .expect("the IR camera");
        let capture = ir.capture_node().expect("it has a capture node");
        assert_eq!(capture.path, "/dev/video2");
        assert_eq!(capture.kind, NodeKind::VideoCapture);
        assert_eq!(
            ir.nodes
                .iter()
                .filter(|n| n.kind == NodeKind::MetaCapture)
                .map(|n| n.path.as_str())
                .collect::<Vec<_>>(),
            vec!["/dev/video3"]
        );
    }

    #[test]
    fn a_reversed_node_order_finds_the_same_capture_node() {
        // The inverse of the test above: if classification secretly depended on order,
        // reversing the list would change the answer.
        let mut reversed = seed_hardware();
        reversed.reverse();
        let cameras = group(&reversed);
        let ir = cameras
            .iter()
            .find(|c| c.card.ends_with('I'))
            .expect("the IR camera");
        assert_eq!(ir.capture_node().expect("capture node").path, "/dev/video2");
    }

    #[test]
    fn every_camera_gets_a_distinct_id_derived_from_its_card_name() {
        let cameras = group(&seed_hardware());
        let ids: Vec<&str> = cameras.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(
            ids,
            vec![
                "cam:integrated-camera-integrated-c",
                "cam:integrated-camera-integrated-i",
                "cam:obsbot-tiny-3-obsbot-tiny-3-st",
            ]
        );
        // And the prefix an agent would actually type resolves.
        let owned: Vec<CameraId> = cameras.iter().map(|c| c.id.clone()).collect();
        assert_eq!(
            schema::camera::resolve_prefix(&owned, "cam:obsbot"),
            schema::camera::PrefixMatch::Unique(owned[2].clone())
        );
    }

    #[test]
    fn two_identical_cameras_get_ordinal_ids_rather_than_one_id_meaning_both() {
        let mut nodes = Vec::new();
        for (index, iface) in ["1-1:1.0", "1-2:1.0"].into_iter().enumerate() {
            nodes.push(node(
                &format!("/dev/video{index}"),
                iface,
                "Webcam",
                "usb-1",
                CAPTURE_CAPS,
                None,
            ));
        }
        let cameras = group(&nodes);
        assert_eq!(
            cameras.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(),
            vec!["cam:webcam", "cam:webcam-2"]
        );
    }

    #[test]
    fn a_metadata_only_group_lists_with_no_capture_node() {
        // A real shape: the camera is listed, and streaming it becomes a typed refusal
        // rather than a camera that mysteriously is not there.
        let nodes = vec![node(
            "/dev/video9",
            "9-9:1.0",
            "Metadata Only",
            "usb-9",
            METADATA_CAPS,
            None,
        )];
        let cameras = group(&nodes);
        assert_eq!(cameras.len(), 1);
        assert!(cameras[0].capture_node().is_none());
        assert_eq!(cameras[0].nodes[0].kind, NodeKind::MetaCapture);
    }

    #[test]
    fn a_node_kind_this_build_cannot_name_keeps_its_capability_word() {
        // D2's rule applied to nodes: represent, don't discard.
        let nodes = vec![node(
            "/dev/video9",
            "9-9:1.0",
            "Something New",
            "usb-9",
            0x0100_0000,
            None,
        )];
        let cameras = group(&nodes);
        assert_eq!(
            cameras[0].nodes[0].kind,
            NodeKind::Other {
                device_caps: 0x0100_0000
            }
        );
    }

    #[test]
    fn an_implausibly_large_group_is_bounded_and_the_camera_still_lists() {
        let mut nodes = Vec::new();
        for index in 0..(limits::MAX_NODES_PER_CAMERA + 5) {
            nodes.push(node(
                &format!("/dev/video{index}"),
                "1-1:1.0",
                "Improbable",
                "usb-1",
                CAPTURE_CAPS,
                None,
            ));
        }
        let cameras = group(&nodes);
        assert_eq!(cameras.len(), 1);
        assert_eq!(cameras[0].nodes.len(), limits::MAX_NODES_PER_CAMERA);
    }

    #[test]
    fn grouping_nothing_produces_nothing_rather_than_a_nameless_camera() {
        assert!(group(&[]).is_empty());
    }
}
