//! Turning what a caller typed into the camera they meant, and what a caller who typed
//! nothing is told (design D1).
//!
//! One home. `webcam-handler-cli` resolves here before opening; the daemon resolves here
//! before dispatching to an actor. Backends do **not** resolve — `CameraBackend::open` takes
//! an id that already means exactly one camera, which is why a backend can answer it with a
//! lookup rather than with a policy.
//!
//! That split matters beyond tidiness. Prefix resolution needs the *whole* enumeration to
//! decide whether a prefix is ambiguous, so a backend resolving it would be answering a
//! question about a set it happens to own — and two backends would then have two opinions
//! about what `cam:obsbot` means. Here there is one.

use schema::backend::CameraBackend;
use schema::camera::{CameraId, CameraInfo, PrefixMatch, resolve_prefix};
use schema::error::{Error, Result};
use schema::report::CameraList;
use schema::selector::CameraSelector;

/// Everything `list` answers: the enumeration, and why it might be empty (D1).
///
/// Two backend calls and one rule, and the rule is the reason this is a function rather
/// than two lines at each caller: **an empty enumeration is diagnosed, not shrugged at.**
/// "No cameras" and "your camera is plugged in with no driver bound to it" \[PF:14\] are
/// different facts, and a `list` that carried only the first would send an operator
/// looking for a webcam that is right there. Only the backend knows what its own absence
/// looks like, which is why the diagnosis is asked of it (note N7).
///
/// One home because there are two composition roots and P4f's parity gate compares their
/// `--json` byte for byte: `webcam-handler-cli list` reaches this through T4's in-process
/// executor and `wch_list` reaches it through T5's server, and a second assembly is how the
/// two come to disagree about what a listing carries.
///
/// # Errors
///
/// Whatever the backend refuses enumeration with. The diagnosis is infallible by
/// declaration — a backend with nothing to add says nothing.
pub fn list(backend: &dyn CameraBackend) -> Result<CameraList> {
    Ok(CameraList {
        cameras: backend.enumerate()?,
        hints: backend.diagnose(),
    })
}

/// Find the camera `requested` names, in whichever of D14's five spellings it was written.
///
/// An id resolves as it always has: the `cam:` prefix is optional on input, an exact match
/// always beats being a prefix of something longer, and an ambiguous prefix names its
/// candidates rather than picking one. The four spellings v3 adds are filters over the same
/// live listing — a node path against **any** of a camera's nodes, a bus path and a USB id
/// and a serial against the fingerprint — and they answer with the same two failures,
/// because "resolved to nothing" and "resolved to more than one" is all resolution has ever
/// been able to fail with (D14: zero new error kinds).
///
/// **The listing is the whole machine, always.** Selection filters what is *returned*, never
/// what is *enumerated*: D1 assigns collision ordinals over the whole enumeration, so a
/// resolver handed a pre-narrowed list would answer `cam:obsbot-tiny-3`-class names that
/// depend on how the caller asked — and an id whose meaning depends on the question is not
/// an id.
///
/// # Errors
///
/// [`Error::CameraUnknown`] when nothing matches — carrying what was asked for, because
/// "no such camera" is unactionable without it. [`Error::CameraAmbiguous`] when several
/// do, carrying every candidate so the caller can say which they meant.
pub fn camera<'a>(cameras: &'a [CameraInfo], requested: &CameraSelector) -> Result<&'a CameraInfo> {
    match requested {
        CameraSelector::Id(id) => by_id(cameras, id),
        // One shape for the four fingerprint- and address-tier spellings: filter the live
        // listing by a predicate, then let the count decide. Written once rather than four
        // times because "none" and "several" are the two answers, and four copies of that
        // is four chances for one of them to go missing.
        CameraSelector::NodePath(path) => narrow(cameras, requested, |info| {
            info.nodes.iter().any(|node| node.path == *path)
        }),
        CameraSelector::BusPath(bus) => {
            narrow(cameras, requested, |info| info.fingerprint.bus_path == *bus)
        }
        CameraSelector::UsbId(usb) => narrow(cameras, requested, |info| {
            info.fingerprint.usb_id == Some(*usb)
        }),
        // A device that reports no serial matches no `serial:` request \[PF:8\] — the
        // comparison is against `Some(text)`, so an absent serial is an absence and never a
        // wildcard.
        CameraSelector::Serial(serial) => narrow(cameras, requested, |info| {
            info.fingerprint.serial.as_deref() == Some(serial.as_str())
        }),
    }
}

/// D1's id grammar, unchanged since P0.
fn by_id<'a>(cameras: &'a [CameraInfo], requested: &CameraId) -> Result<&'a CameraInfo> {
    let ids: Vec<CameraId> = cameras.iter().map(|info| info.id.clone()).collect();
    match resolve_prefix(&ids, requested.as_str()) {
        PrefixMatch::Unique(id) => {
            cameras
                .iter()
                .find(|info| info.id == id)
                .ok_or_else(|| Error::CameraUnknown {
                    // Unreachable: the id came out of this very list. Expressed as an
                    // error rather than an `expect` because a resolver that can panic is
                    // a resolver a malformed request can crash.
                    requested: requested.to_string(),
                })
        }
        PrefixMatch::None => Err(Error::CameraUnknown {
            requested: requested.to_string(),
        }),
        PrefixMatch::Ambiguous(candidates) => Err(Error::CameraAmbiguous {
            requested: requested.to_string(),
            candidates,
        }),
    }
}

/// Narrow the live listing by one predicate, and let the count answer.
///
/// Ambiguity is a first-class refusal here rather than an edge case, because on the seed
/// hardware it is the normal case: the two Chicony logical cameras share one `usb_id`
/// \[PF:13\], so `usb:04f2:b83c` names two cameras on a machine that has one Chicony. The
/// candidates carry what tells them apart.
fn narrow<'a>(
    cameras: &'a [CameraInfo],
    requested: &CameraSelector,
    matches: impl Fn(&CameraInfo) -> bool,
) -> Result<&'a CameraInfo> {
    let mut found = cameras.iter().filter(|info| matches(info));
    let Some(first) = found.next() else {
        return Err(Error::CameraUnknown {
            requested: requested.to_string(),
        });
    };
    if found.next().is_none() {
        return Ok(first);
    }
    Err(Error::CameraAmbiguous {
        requested: requested.to_string(),
        candidates: cameras
            .iter()
            .filter(|info| matches(info))
            .map(|info| info.id.clone())
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use schema::backend::BackendKind;
    use schema::camera::{CameraFingerprint, assign_ids};

    use super::*;

    fn cameras(cards: &[&str]) -> Vec<CameraInfo> {
        let owned: Vec<String> = cards.iter().map(|c| (*c).to_owned()).collect();
        assign_ids(&owned)
            .into_iter()
            .zip(cards)
            .map(|(id, card)| CameraInfo {
                id,
                fingerprint: CameraFingerprint {
                    bus_path: (*card).to_owned(),
                    usb_id: None,
                    card: (*card).to_owned(),
                    driver: "uvcvideo".to_owned(),
                    serial: None,
                },
                card: (*card).to_owned(),
                driver: "uvcvideo".to_owned(),
                bus_info: "usb-1".to_owned(),
                nodes: Vec::new(),
                backend: BackendKind::V4l2,
            })
            .collect()
    }

    fn ask(s: &str) -> CameraSelector {
        schema::selector::parse(s).expect("a spelling the one parser accepts")
    }

    #[test]
    fn an_unambiguous_prefix_resolves_with_or_without_the_cam_prefix() {
        // The seed hardware's real ids are long; D1 promises agents need not type them.
        let list = cameras(&[
            "OBSBOT Tiny 3: OBSBOT Tiny 3 St",
            "Integrated Camera: Integrated C",
        ]);
        assert_eq!(
            camera(&list, &ask("cam:obsbot")).expect("resolves").id,
            list[0].id
        );
        assert_eq!(
            camera(&list, &ask("obsbot")).expect("resolves").id,
            list[0].id
        );
        // And the full id, which must not be ambiguous against itself.
        assert_eq!(
            camera(&list, &CameraSelector::Id(list[0].id.clone()))
                .expect("resolves")
                .id,
            list[0].id
        );
    }

    #[test]
    fn an_ambiguous_prefix_names_every_candidate_rather_than_choosing() {
        let list = cameras(&["Webcam", "Webcam"]);
        match camera(&list, &ask("cam:web")) {
            Err(Error::CameraAmbiguous {
                requested,
                candidates,
            }) => {
                assert_eq!(requested, "cam:web");
                assert_eq!(candidates.len(), 2);
                // Naming one and picking it would be the silent-wrong-camera defect.
                assert_ne!(candidates[0], candidates[1]);
            }
            other => panic!("expected ambiguity, got {other:?}"),
        }
    }

    #[test]
    fn an_exact_id_beats_being_the_prefix_of_a_longer_one() {
        // D1's rule, and the seed hardware's own trap: `obsbot-tiny` is a prefix of
        // `obsbot-tiny-3`, and a caller who typed the whole thing meant the whole thing.
        let list = cameras(&["OBSBOT Tiny", "OBSBOT Tiny 3"]);
        assert_eq!(
            camera(&list, &ask("cam:obsbot-tiny")).expect("resolves").id,
            list[0].id
        );
    }

    #[test]
    fn a_name_nothing_answers_to_says_what_was_asked_for() {
        let list = cameras(&["Webcam"]);
        match camera(&list, &ask("cam:nope")) {
            Err(Error::CameraUnknown { requested }) => assert_eq!(requested, "cam:nope"),
            other => panic!("expected CameraUnknown, got {other:?}"),
        }
        // And over an empty enumeration, which is the machine-with-no-cameras case.
        assert!(matches!(
            camera(&[], &ask("cam:anything")),
            Err(Error::CameraUnknown { .. })
        ));
    }

    // ------------------------------------------------------------------ D14's four others

    /// A machine shaped like the seed hardware: a Chicony whose two logical cameras share
    /// one USB id \[PF:13\] and report the same untrustworthy serial \[PF:8\], and an
    /// OBSBOT on another port that reports no serial at all.
    fn machine() -> Vec<CameraInfo> {
        fn node(path: &str) -> schema::camera::DeviceNode {
            schema::camera::DeviceNode {
                path: path.into(),
                kind: schema::camera::NodeKind::VideoCapture,
                device_caps: schema::camera::CAP_VIDEO_CAPTURE,
                capabilities: schema::camera::CAP_VIDEO_CAPTURE,
            }
        }
        fn camera_info(
            id: &str,
            bus_path: &str,
            usb: Option<(u16, u16)>,
            serial: Option<&str>,
            nodes: Vec<schema::camera::DeviceNode>,
        ) -> CameraInfo {
            CameraInfo {
                id: CameraId::parse(id).expect("a literal id"),
                fingerprint: CameraFingerprint {
                    bus_path: bus_path.to_owned(),
                    usb_id: usb.map(|(vendor, product)| schema::camera::UsbId { vendor, product }),
                    card: id.to_owned(),
                    driver: "uvcvideo".to_owned(),
                    serial: serial.map(str::to_owned),
                },
                card: id.to_owned(),
                driver: "uvcvideo".to_owned(),
                bus_info: format!("usb-{bus_path}"),
                nodes,
                backend: BackendKind::V4l2,
            }
        }
        vec![
            camera_info(
                "cam:integrated-camera",
                "3-4:1.0",
                Some((0x04f2, 0xb83c)),
                Some("0001"),
                vec![node("/dev/video0"), node("/dev/video1")],
            ),
            camera_info(
                "cam:integrated-ir-camera",
                "3-4:1.2",
                Some((0x04f2, 0xb83c)),
                Some("0001"),
                vec![node("/dev/video2"), node("/dev/video3")],
            ),
            camera_info(
                "cam:obsbot-tiny-3",
                "1-2:1.0",
                Some((0x3564, 0x1010)),
                None,
                vec![node("/dev/video4")],
            ),
        ]
    }

    #[test]
    fn a_node_path_finds_the_camera_that_owns_the_node_whichever_node_it_is() {
        let list = machine();
        // The capture node, and the *second* node of the same camera: D14 addresses by what
        // a caller can see in `/dev`, not only by the node this build would stream from.
        assert_eq!(
            camera(&list, &ask("/dev/video0")).expect("resolves").id,
            list[0].id
        );
        assert_eq!(
            camera(&list, &ask("/dev/video1")).expect("resolves").id,
            list[0].id
        );
        assert_eq!(
            camera(&list, &ask("/dev/video4")).expect("resolves").id,
            list[2].id
        );
        // A node this machine does not have is a request nothing can match.
        assert!(matches!(
            camera(&list, &ask("/dev/video9")),
            Err(Error::CameraUnknown { requested }) if requested == "/dev/video9"
        ));
    }

    #[test]
    fn a_node_path_is_an_address_so_the_same_spelling_moves_when_the_numbers_do() {
        // PF:22, asserted rather than described: node numbers are probe-order bookkeeping,
        // so a renumbering moves the answer — which is the whole reason D14 documents this
        // spelling as session-scoped and resolves it against the live listing every call.
        let before = machine();
        assert_eq!(
            camera(&before, &ask("/dev/video0")).expect("resolves").id,
            before[0].id
        );
        let mut after = machine();
        after[0].nodes = vec![schema::camera::DeviceNode {
            path: "/dev/video4".into(),
            kind: schema::camera::NodeKind::VideoCapture,
            device_caps: schema::camera::CAP_VIDEO_CAPTURE,
            capabilities: schema::camera::CAP_VIDEO_CAPTURE,
        }];
        after[2].nodes = vec![schema::camera::DeviceNode {
            path: "/dev/video0".into(),
            kind: schema::camera::NodeKind::VideoCapture,
            device_caps: schema::camera::CAP_VIDEO_CAPTURE,
            capabilities: schema::camera::CAP_VIDEO_CAPTURE,
        }];
        assert_eq!(
            camera(&after, &ask("/dev/video0")).expect("resolves").id,
            after[2].id,
            "the node path followed the node, which is what an address does"
        );
    }

    #[test]
    fn a_bus_path_names_one_camera_because_it_is_the_interface_it_sits_on() {
        let list = machine();
        assert_eq!(
            camera(&list, &ask("bus:3-4:1.2")).expect("resolves").id,
            list[1].id
        );
        // The colons inside the path are part of it, and the scheme's own colon is not.
        assert!(matches!(
            camera(&list, &ask("bus:3-4")),
            Err(Error::CameraUnknown { requested }) if requested == "bus:3-4"
        ));
    }

    #[test]
    fn a_usb_id_the_two_logical_cameras_share_is_ambiguous_and_names_both() {
        // PF:13's shape, and D14 calls it the normal case somewhere rather than an edge:
        // one Chicony is two logical cameras behind one VID:PID.
        let list = machine();
        match camera(&list, &ask("usb:04f2:b83c")) {
            Err(Error::CameraAmbiguous {
                requested,
                candidates,
            }) => {
                assert_eq!(requested, "usb:04f2:b83c");
                assert_eq!(candidates, vec![list[0].id.clone(), list[1].id.clone()]);
            }
            other => panic!("expected ambiguity, got {other:?}"),
        }
        // And the other device's id, which is not shared, resolves.
        assert_eq!(
            camera(&list, &ask("usb:3564:1010")).expect("resolves").id,
            list[2].id
        );
    }

    #[test]
    fn a_serial_less_device_matches_no_serial_request() {
        // PF:8: the OBSBOT reports none, and an absent serial is an absence — never a
        // wildcard that would hand a caller the wrong camera.
        let list = machine();
        assert!(matches!(
            camera(&list, &ask("serial:anything")),
            Err(Error::CameraUnknown { requested }) if requested == "serial:anything"
        ));
        // The Chicony pair reports the same one, which is exactly why PF:8 says a serial is
        // recorded and not trusted as identity.
        assert!(matches!(
            camera(&list, &ask("serial:0001")),
            Err(Error::CameraAmbiguous { candidates, .. }) if candidates.len() == 2
        ));
    }

    #[test]
    fn a_serial_only_one_device_reports_resolves_to_it() {
        let mut list = machine();
        list[2].fingerprint.serial = Some("OB-77".to_owned());
        assert_eq!(
            camera(&list, &ask("serial:OB-77")).expect("resolves").id,
            list[2].id
        );
    }

    #[test]
    fn selection_never_moves_an_id_because_it_never_filters_the_enumeration() {
        // D14's load-bearing claim, tested where it can go red: every spelling is resolved
        // against the whole machine, so the ids the answers carry are the ids `list` hands
        // out — a resolver that narrowed the listing first would reassign D1's collision
        // ordinals and answer with a name that depends on how it was asked.
        let list = machine();
        for spelling in ["cam:obsbot", "/dev/video4", "bus:1-2:1.0", "usb:3564:1010"] {
            assert_eq!(
                camera(&list, &ask(spelling)).expect("resolves").id,
                list[2].id,
                "{spelling} named a different camera than the others"
            );
        }
    }
}
