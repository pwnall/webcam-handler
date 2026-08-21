//! Camera selection over the committed corpus (design D14; FR-W1).
//!
//! The unit arms in `engine::resolve` drive hand-built fixtures, which is the right shape for
//! "does the matcher match". This file asks the neighbouring question that only real captured
//! devices can answer: **do D14's spellings behave the way it says on the machines this
//! project has actually seen?** The fixture is *every* committed profile — `corpus::load_all`
//! walks the directory and filters nothing, so the population is whatever `corpus/profiles/`
//! holds rather than a number this comment would have to keep. What it holds today is the
//! four physical cameras these arms are about — a Chicony whose two logical cameras share one
//! USB id and one serial \[PF:13, PF:8\], an OBSBOT that reports no serial at all, a
//! four-node Dell \[PF:19\] and a Logitech — and the kernel-virtual `vivid` capture beside
//! them, all replayed through the fake as one machine. The arms below quantify over that
//! population rather than counting it, which is why a profile joining the corpus makes this
//! file's host bigger and none of its claims wrong.
//!
//! That last word is the point. Replaying every committed profile at once builds a host no
//! desk has ever had, and its collisions are *the fixtures*: the corpus was captured on
//! different days, so its `/dev/videoN` paths overlap between profiles exactly as PF:22 says
//! they would after a driver reload. A design that treated a node path as identity would be
//! green on any one of these profiles and wrong on the pair.

use engine::resolve;
use fake::FakeBackend;
use schema::backend::CameraBackend;
use schema::camera::CameraInfo;
use schema::error::Error;
use schema::selector::{CameraSelector, SelectorScheme, parse};
use testkit::corpus;

/// Every committed profile, replayed as one machine.
fn machine() -> Vec<CameraInfo> {
    let profiles: Vec<schema::profile::DeviceProfile> = corpus::load_all()
        .expect("the corpus parses")
        .into_iter()
        .map(|(_path, profile)| profile)
        .collect();
    assert!(
        profiles.len() >= 2,
        "a one-camera corpus cannot exhibit an ambiguity"
    );
    FakeBackend::new(profiles)
        .expect("the corpus replays")
        .enumerate()
        .expect("the fake enumerates")
}

fn resolve_one(cameras: &[CameraInfo], spelling: &str) -> Result<CameraInfo, Error> {
    let selector = parse(spelling).expect("the spelling parses");
    resolve::camera(cameras, &selector).cloned()
}

#[test]
fn every_camera_in_the_corpus_is_found_by_the_bus_path_it_was_captured_at() {
    // The fingerprint tier's simplest claim, over every profile rather than a chosen one: a
    // bus path is an interface, one camera sits on it, and the answer is that camera.
    let cameras = machine();
    for info in &cameras {
        let spelling = format!("bus:{}", info.fingerprint.bus_path);
        let found = resolve_one(&cameras, &spelling).unwrap_or_else(|err| {
            panic!("{spelling} did not resolve: {err}");
        });
        assert_eq!(found.id, info.id, "{spelling} named another camera");
    }
    assert!(!cameras.is_empty());
}

#[test]
fn the_two_logical_cameras_behind_one_usb_id_answer_ambiguously_and_name_both() {
    // PF:13's shape, and D14 calls it "the normal case somewhere" rather than an edge. The
    // Chicony is one USB device hosting an RGB camera and an IR camera, and every consumer
    // that says `usb:04f2:b83c` on that machine means one of two things.
    let cameras = machine();
    let shared: Vec<&CameraInfo> = cameras
        .iter()
        .filter(|info| {
            info.fingerprint.usb_id.is_some_and(|usb| {
                cameras
                    .iter()
                    .filter(|other| other.fingerprint.usb_id == Some(usb))
                    .count()
                    > 1
            })
        })
        .collect();
    assert!(
        shared.len() >= 2,
        "the corpus no longer holds a shared-usb_id pair; PF:13's fixture is what pins this \
         refusal, and without it this arm asserts nothing"
    );

    let usb = shared[0].fingerprint.usb_id.expect("filtered on Some");
    let spelling = format!("usb:{usb}");
    match resolve_one(&cameras, &spelling) {
        Err(Error::CameraAmbiguous {
            requested,
            candidates,
        }) => {
            assert_eq!(requested, spelling);
            assert_eq!(candidates.len(), shared.len());
            for info in &shared {
                assert!(
                    candidates.contains(&info.id),
                    "{} shares the id and is not among the candidates",
                    info.id
                );
            }
        }
        other => panic!("expected an ambiguity for {spelling}, got {other:?}"),
    }
}

#[test]
fn a_device_that_reports_no_serial_is_reachable_by_no_serial_spelling() {
    // PF:8, over the device that actually exhibits it: the OBSBOT reports none. The claim is
    // *unreachability* — an absent serial must not become a wildcard that hands a caller
    // whichever camera happened to be first.
    let cameras = machine();
    let serial_less: Vec<&CameraInfo> = cameras
        .iter()
        .filter(|info| info.fingerprint.serial.is_none())
        .collect();
    assert!(
        !serial_less.is_empty(),
        "the corpus no longer holds a serial-less device; PF:8's fixture is what pins this"
    );

    for info in &serial_less {
        // Nothing this device could be asked for by. The spelling below is the one thing a
        // caller might try — the camera's own id — as a `serial:` body, which is exactly the
        // mistake an absent serial invites.
        let spelling = format!("serial:{}", info.id.body());
        assert!(
            matches!(
                resolve_one(&cameras, &spelling),
                Err(Error::CameraUnknown { .. })
            ),
            "{spelling} resolved to something on a machine where it names nothing"
        );
    }
}

#[test]
fn a_serial_two_devices_report_is_ambiguous_rather_than_first_wins() {
    // The Chicony pair reports `"0001"` on both halves, which is precisely why PF:8 says a
    // serial is recorded and not trusted as identity. A resolver that returned the first
    // match would hand a caller the RGB camera when they meant the IR one, silently.
    let cameras = machine();
    let mut serials: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
    for info in &cameras {
        if let Some(serial) = info.fingerprint.serial.as_deref() {
            *serials.entry(serial).or_default() += 1;
        }
    }
    let Some((shared, count)) = serials.iter().find(|(_, count)| **count > 1) else {
        panic!("the corpus no longer holds two devices reporting one serial (PF:8's fixture)");
    };

    let spelling = format!("serial:{shared}");
    match resolve_one(&cameras, &spelling) {
        Err(Error::CameraAmbiguous { candidates, .. }) => assert_eq!(candidates.len(), *count),
        other => panic!("expected an ambiguity for {spelling}, got {other:?}"),
    }
}

#[test]
fn a_node_path_two_profiles_share_is_ambiguous_which_is_what_makes_it_an_address() {
    // The corpus's own accident, promoted to a fixture. These profiles were captured on
    // different days, so `/dev/video8` belongs to the OBSBOT in one and to the Dell in
    // another — which is exactly what PF:22 says a `uvcvideo` reload does to a live machine.
    // D14 answers it the only honest way: two cameras, named.
    let cameras = machine();
    let mut owners: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
    for info in &cameras {
        for node in &info.nodes {
            *owners.entry(node.path.as_str()).or_default() += 1;
        }
    }
    let Some((shared, count)) = owners.iter().find(|(_, count)| **count > 1) else {
        panic!(
            "no two committed profiles share a node path; this arm's fixture is the corpus's \
             own capture-order collision, and without one it asserts nothing"
        );
    };

    match resolve_one(&cameras, shared) {
        Err(Error::CameraAmbiguous {
            requested,
            candidates,
        }) => {
            assert_eq!(&requested, shared);
            assert_eq!(candidates.len(), *count);
        }
        other => panic!("expected an ambiguity for {shared}, got {other:?}"),
    }

    // And the other direction on the same machine: a node exactly one camera owns resolves.
    let Some((sole, _)) = owners.iter().find(|(_, count)| **count == 1) else {
        panic!("every node path in the corpus is shared, which no machine has ever looked like");
    };
    resolve_one(&cameras, sole).unwrap_or_else(|err| panic!("{sole} did not resolve: {err}"));
}

#[test]
fn every_scheme_in_the_vocabulary_resolves_something_on_this_machine() {
    // The walk that makes the four arms above a *population* rather than four chosen cases: a
    // sixth spelling cannot join `SelectorScheme::ALL` without somebody deciding what it
    // resolves to over the corpus. The Id arm is D1's, unchanged since P0, and is here so the
    // walk is complete rather than nearly complete.
    let cameras = machine();
    let mut exercised = 0;
    for scheme in SelectorScheme::ALL {
        let spelling = match scheme {
            SelectorScheme::Id => cameras
                .first()
                .map(|info| info.id.to_string())
                .expect("the corpus replays at least one camera"),
            SelectorScheme::NodePath => cameras
                .iter()
                .find_map(|info| {
                    let path = info.nodes.first()?.path.as_str();
                    let owners = cameras
                        .iter()
                        .filter(|other| other.nodes.iter().any(|node| node.path == path))
                        .count();
                    (owners == 1).then(|| path.to_owned())
                })
                .expect("some node in the corpus belongs to exactly one camera"),
            SelectorScheme::BusPath => cameras
                .first()
                .map(|info| format!("bus:{}", info.fingerprint.bus_path))
                .expect("a camera"),
            SelectorScheme::UsbId => cameras
                .iter()
                .find_map(|info| {
                    let usb = info.fingerprint.usb_id?;
                    let owners = cameras
                        .iter()
                        .filter(|other| other.fingerprint.usb_id == Some(usb))
                        .count();
                    (owners == 1).then(|| format!("usb:{usb}"))
                })
                .expect("some committed device does not share its USB id"),
            SelectorScheme::Serial => cameras
                .iter()
                .find_map(|info| {
                    let serial = info.fingerprint.serial.as_deref()?;
                    let owners = cameras
                        .iter()
                        .filter(|other| other.fingerprint.serial.as_deref() == Some(serial))
                        .count();
                    (owners == 1).then(|| format!("serial:{serial}"))
                })
                .expect("some committed device reports a serial nobody else does"),
        };
        let parsed = parse(&spelling).expect("the spelling parses");
        assert_eq!(parsed.scheme(), *scheme, "{spelling} named another scheme");
        resolve_one(&cameras, &spelling)
            .unwrap_or_else(|err| panic!("{scheme:?} spelling {spelling} did not resolve: {err}"));
        exercised += 1;
    }
    assert_eq!(exercised, SelectorScheme::ALL.len());
}

/// The schemes whose spelling a caller **cannot** build out of the document this tool answers
/// with, each with the reason it is here.
///
/// A named, counted exception rather than a silence: the arm below walks
/// [`SelectorScheme::ALL`], so a sixth scheme is either read back out of the document or is
/// listed here, and a scheme listed here that *has* become readable fails the arm rather than
/// sitting on as a stale excuse.
const NOT_IN_THE_DOCUMENT: &[(SelectorScheme, &str)] = &[(
    SelectorScheme::UsbId,
    "`UsbId` derives `Serialize`, so `fingerprint.usb_id` is an object of two decimal \
     integers while the grammar takes four-digit lower-case hex — asked in hex, answered in \
     decimal, in one exchange. Whether that shape changes is the owner's call under note \
     **N109**'s three options and note **N325** records the measurement",
)];

#[test]
fn every_spelling_this_tool_answers_with_can_be_read_back_out_of_its_own_document() {
    // **The round trip the primary consumer actually performs.** An agent asks `list`, holds
    // the `--json` answer and has to name one of those cameras again; a harness comparing two
    // machines holds the same document and has to ask the other machine about the device it
    // describes. Both of them build a selector out of a listing entry by substituting a
    // string, so what matters is not that the *type* round-trips through serde — the schema
    // suite drives that — but that the document carries a value D14's grammar takes.
    //
    // Driven over `SelectorScheme::ALL` and over every camera the corpus replays, with the one
    // scheme that cannot be read back declared above rather than skipped in silence. An
    // absent fact is a third case and neither of the first two: a device that reports no
    // serial \[PF:8\] and a virtual driver that is on no USB \[the vivid capture\] carry
    // `null`, which is an absence this build represents and not a shape it got wrong.
    let cameras = machine();
    let mut read_back = 0;
    let mut absent = 0;
    let mut excepted = 0;
    for info in &cameras {
        let document = serde_json::to_value(info).expect("a listing entry serializes");
        for scheme in SelectorScheme::ALL {
            let (field, carried) = spelling_in_document(&document, *scheme);
            let Some(spelling) = carried else {
                if document
                    .pointer(field)
                    .is_none_or(serde_json::Value::is_null)
                {
                    absent += 1;
                    continue;
                }
                let reason = NOT_IN_THE_DOCUMENT
                    .iter()
                    .find(|(excepted, _)| excepted == scheme)
                    .map(|(_, reason)| *reason);
                assert!(
                    reason.is_some(),
                    "{scheme:?} is carried at {field} in a shape D14's grammar does not take, \
                     and nothing in NOT_IN_THE_DOCUMENT says why: {}",
                    document[field.trim_start_matches('/')]
                );
                excepted += 1;
                continue;
            };
            let parsed = parse(&spelling).unwrap_or_else(|err| {
                panic!(
                    "{field} answered {spelling}, which this build's \
                     own grammar refuses: {err}"
                )
            });
            assert_eq!(
                parsed.scheme(),
                *scheme,
                "{field} answered {spelling}, which parses as another scheme"
            );
            assert_eq!(
                parsed.to_string(),
                spelling,
                "{field}'s value does not survive the grammar unchanged"
            );
            // And it names something. `CameraAmbiguous` is an answer — the corpus is built
            // out of collisions on purpose — but `CameraUnknown` would mean this tool printed
            // a value that names no camera it can see.
            if let Err(err) = resolve_one(&cameras, &spelling) {
                assert!(
                    matches!(err, Error::CameraAmbiguous { .. }),
                    "{spelling} came out of this tool's own answer and resolved to nothing: \
                     {err}"
                );
            }
            read_back += 1;
        }
    }
    // Neither an empty walk nor a walk that excepted everything (note **N231**).
    assert!(
        read_back >= cameras.len(),
        "only {read_back} spellings were read back"
    );
    assert_eq!(
        excepted,
        cameras.len() * NOT_IN_THE_DOCUMENT.len() - absent_usb_ids(&cameras),
        "every camera that carries a USB id at all must have hit the declared exception"
    );
    assert!(
        absent > 0,
        "the corpus holds a device with an absent fact, and this walk must \
         have met it rather than treating an absence as a shape"
    );
}

/// How many committed cameras carry no USB id at all — the vivid capture, which is on no bus.
fn absent_usb_ids(cameras: &[CameraInfo]) -> usize {
    cameras
        .iter()
        .filter(|info| info.fingerprint.usb_id.is_none())
        .count()
}

/// Where a scheme's spelling lives in a serialized [`CameraInfo`], and the spelling itself when
/// the document carries it as a string D14's grammar takes.
///
/// The JSON pointer is returned either way so a failure names the field rather than the scheme
/// alone: what a reader has to go and look at is the property, and `usb_id` is one property of
/// one struct rather than a fact about selectors.
fn spelling_in_document(
    document: &serde_json::Value,
    scheme: SelectorScheme,
) -> (&'static str, Option<String>) {
    let (field, prefixed) = match scheme {
        SelectorScheme::Id => ("/id", false),
        SelectorScheme::NodePath => ("/nodes/0/path", false),
        SelectorScheme::BusPath => ("/fingerprint/bus_path", true),
        SelectorScheme::UsbId => ("/fingerprint/usb_id", true),
        SelectorScheme::Serial => ("/fingerprint/serial", true),
    };
    let body = document
        .pointer(field)
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);
    let spelling = body.map(|body| match (prefixed, scheme.prefix()) {
        (true, Some(prefix)) => format!("{prefix}{body}"),
        _ => body,
    });
    (field, spelling)
}

#[test]
fn the_ids_a_listing_hands_out_are_the_same_whichever_spelling_asked() {
    // D14's load-bearing claim — selection never filters enumeration — over the corpus, where
    // it can actually go wrong: D1 assigns collision ordinals across the whole machine, and
    // two of these profiles are the same card name. An implementation that narrowed the
    // listing before resolving would answer with ordinals computed over the narrowed set.
    let cameras = machine();
    for info in &cameras {
        let by_id = resolve_one(&cameras, info.id.as_str()).expect("an id resolves");
        let by_bus = resolve_one(&cameras, &format!("bus:{}", info.fingerprint.bus_path))
            .expect("a bus path resolves");
        assert_eq!(by_id.id, by_bus.id);
        assert_eq!(by_id.id, info.id, "an id moved between two ways of asking");
        assert_eq!(
            by_id, by_bus,
            "two spellings answered with different documents"
        );
    }
}

#[test]
fn a_spelling_no_camera_can_match_is_refused_naming_the_vocabulary() {
    // The refusal an unattended caller reads, over a real machine rather than an empty one:
    // there *are* cameras here, and the request still cannot name one, so the message has to
    // be about the request.
    let cameras = machine();
    let refusal = resolve_one(&cameras, "bus:nowhere").expect_err("nothing sits there");
    assert!(matches!(refusal, Error::CameraUnknown { .. }));
    assert!(
        !refusal.to_string().contains("selector is one of"),
        "a well-formed spelling that matched nothing must not be told its grammar is wrong"
    );

    let mistake = parse("buspath:3-4:1.0").expect_err("an unknown scheme is refused");
    let message = mistake.to_string();
    for scheme in SelectorScheme::ALL {
        assert!(
            message.contains(scheme.example()),
            "the refusal does not teach {scheme:?}: {message}"
        );
    }
}

#[test]
fn a_selector_built_by_hand_resolves_exactly_as_its_parsed_twin_does() {
    // The facade and the wire both hand `resolve::camera` a `CameraSelector` they did not
    // parse from a string — D18's embedder builds one, and a test writes one. This is the arm
    // that says the two roads meet: a constructed selector and a parsed one are one value.
    let cameras = machine();
    let info = cameras.first().expect("a camera");
    let constructed = CameraSelector::BusPath(info.fingerprint.bus_path.clone());
    let parsed = parse(&format!("bus:{}", info.fingerprint.bus_path)).expect("parses");
    assert_eq!(constructed, parsed);
    assert_eq!(
        resolve::camera(&cameras, &constructed)
            .expect("resolves")
            .id,
        resolve::camera(&cameras, &parsed).expect("resolves").id
    );
}
