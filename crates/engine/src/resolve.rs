//! Turning what a caller typed into the camera they meant (design D1).
//!
//! One home. `wch` resolves here before opening; the daemon will resolve here at P4
//! before dispatching to an actor. Backends do **not** resolve — `CameraBackend::open`
//! takes an id that already means exactly one camera, which is why a backend can answer
//! it with a lookup rather than with a policy.
//!
//! That split matters beyond tidiness. Prefix resolution needs the *whole* enumeration to
//! decide whether a prefix is ambiguous, so a backend resolving it would be answering a
//! question about a set it happens to own — and two backends would then have two opinions
//! about what `cam:obsbot` means. Here there is one.

use schema::camera::{CameraId, CameraInfo, PrefixMatch, resolve_prefix};
use schema::error::{Error, Result};

/// Find the camera `requested` names, accepting any unambiguous prefix (D1).
///
/// The `cam:` prefix is optional on input, an exact match always beats being a prefix of
/// something longer, and an ambiguous prefix names its candidates rather than picking one.
///
/// # Errors
///
/// [`Error::CameraUnknown`] when nothing matches — carrying what was asked for, because
/// "no such camera" is unactionable without it. [`Error::CameraAmbiguous`] when several
/// do, carrying every candidate so the caller can say which they meant.
pub fn camera<'a>(cameras: &'a [CameraInfo], requested: &CameraId) -> Result<&'a CameraInfo> {
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

    fn ask(s: &str) -> CameraId {
        CameraId::parse(s).expect("a non-empty id")
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
        assert_eq!(camera(&list, &list[0].id).expect("resolves").id, list[0].id);
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
}
