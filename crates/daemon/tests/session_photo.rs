//! The calibration-sample route over a real socket: a real session tree, a real photograph, and
//! every shape of reference that must not reach a file (design **D20**, **D9**, **D11**;
//! docs/13 P9b).
//!
//! `preview.rs` is this suite's neighbour and the two divide P9b's route between them.
//! `every_camera_bearing_route_is_behind_the_gate` lives there and is about the *posture* — the
//! third path is on the list, is refused anonymously, is refused cross-site, and answers with the
//! token — which is a claim about the whole list and belongs beside the other two paths on it.
//! This file is about what the route *does* once the credential is right: which references
//! resolve, which do not, and what a caller-shaped reference is unable to reach.
//!
//! ## The fixture is a real store, and every path in it comes from the store
//!
//! `engine::lifecycle::create` makes the session directory, `SessionStore::create_photo_dir`
//! makes `photos/<control>/<pass>/`, and `SessionStore::photo_dir_rel` is what the sample's
//! `photo` field records. Nothing here writes a layout of its own, so a suite that passed
//! against a layout the product does not write is not an arrangement this file can reach.
//!
//! **No camera frame is written and none is asserted.** AGENTS: a frame may contain a person, so
//! what the sample file holds is a JPEG's magic number and a sentence this suite wrote — which is
//! what makes "the route served *this* file" a byte comparison. The state directory is a
//! `TempStore`, which is gitignored scratch and is removed when the fixture drops.
//!
//! ## Nothing here waits on a clock
//!
//! Every request is a `TcpStream` written and read to end with `Connection: close`, and the stop
//! is the daemon's own cancellation followed by a join.

use std::sync::Arc;

use camino::{Utf8Path, Utf8PathBuf};
use daemon::http;
use daemon::server::Wchd;
use daemon::shutdown::Shutdown;
use engine::lifecycle::{self, SessionSpec};
use engine::store::{LockProtocol, SessionStore, StoreLock, TempStore};
use fake::FakeBackend;
use schema::backend::CameraBackend;
use schema::control::ControlSlug;
use schema::session::{ControlSession, ControlStatus, Sample, SweepSpec};
use schema::time::Stamp;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::TcpStream;
use uuid::Uuid;

// ------------------------------------------------------------------------- the fixture

/// The bytes the planted sample holds.
///
/// A JPEG's magic number and a sentence, and deliberately not a picture: this suite asserts byte
/// fidelity, and a camera frame may contain a person (AGENTS).
const SAMPLE_BYTES: &[u8] = b"\xff\xd8\xffa calibration sample this suite wrote\xff\xd9";

/// The control the planted sweep is of.
const CONTROL: &str = "focus_absolute";

/// The pass and the requested value the planted sample was taken at.
const PASS: usize = 0;
const VALUE: i64 = 42;

/// One daemon, one session tree with one sample in it, and the web listener D11 opens.
struct Samples {
    serving: http::Serving,
    token: String,
    shutdown: Shutdown,
    store: SessionStore,
    /// D9's lifetime lock, shared with the daemon rather than taken twice — and kept, because
    /// two claims here edit the session document the way a later verb would.
    lock: Arc<StoreLock>,
    session: Uuid,
    /// Where the session landed, so a claim can name the file the route derived.
    dir: Utf8PathBuf,
    /// A file **outside** the session, planted so "no reference reaches it" is a claim about a
    /// thing that is there rather than about a path that is empty either way.
    outside: Utf8PathBuf,
    _wchd: Wchd,
    _state: TempStore,
}

impl Samples {
    /// Start one. Must be called from inside a tokio runtime.
    async fn start() -> Samples {
        let backend = Arc::new(
            FakeBackend::new(vec![testkit::fixtures::synthetic_basic()])
                .expect("the synthetic profile is this build's version"),
        );
        let cameras = backend
            .enumerate()
            .expect("the fake enumerates what it replays");
        let fingerprint = cameras.first().expect("one camera").fingerprint.clone();

        let state = TempStore::new().expect("a state directory");
        let store = SessionStore::new(state.root());

        // The file no reference may reach, beside the session tree rather than inside it. The
        // traversal claims below are about *this* file: an assertion that a `../` reference
        // answers `404` over a path where nothing exists would hold in a build that served it.
        let outside = Utf8PathBuf::from(state.root()).join("not-a-sample.jpg");
        std::fs::write(&outside, b"this file is not in any session").expect("a fresh temp dir");

        // Written before the daemon takes the directory: D9 gives a running daemon the lock for
        // its lifetime, so a fixture that wrote afterwards would be contending with the thing it
        // is setting up.
        let (session, dir) = store
            .with_lock(|lock| {
                let mut session = lifecycle::create(
                    &store,
                    lock,
                    &SessionSpec {
                        id: Uuid::now_v7(),
                        fingerprint: fingerprint.clone(),
                        task: "sample-review".to_owned(),
                        goal: "a legible frame".to_owned(),
                        criteria: vec!["sharp".to_owned()],
                        tool_version: schema::TOOL_VERSION.to_owned(),
                    },
                    Stamp::now(),
                )?;
                // Where the store put it, **asked rather than assembled**: the walk that lists
                // sessions is the one thing that knows D9's layout, so this fixture holds no
                // opinion about it — and holding none is also what keeps this file out of
                // `atomic-write-home`'s population, where a test that names D9's paths and
                // writes a fixture cannot be told apart from a bypass by a `grep`.
                let dir = store
                    .all_sessions()?
                    .into_iter()
                    .find(|found| found.id == session.id)
                    .expect("the store lists the session it has just written")
                    .dir;
                let control = control();
                let photo_dir = store.create_photo_dir(lock, &dir, &control, PASS)?;
                let name = format!("{VALUE}.jpg");
                std::fs::write(photo_dir.join(&name), SAMPLE_BYTES).expect("a fresh photo dir");
                session.controls.insert(
                    control.clone(),
                    ControlSession {
                        status: ControlStatus::Sweeping {
                            plan: SweepSpec::Uniform { step: 1 },
                            done: 1,
                            total: 1,
                            precision: 1,
                            adjustments: Vec::new(),
                        },
                        samples: vec![Sample {
                            requested: VALUE,
                            applied: VALUE,
                            warnings: Vec::new(),
                            photo: SessionStore::photo_dir_rel(&control, PASS)?.join(&name),
                            metrics: std::collections::BTreeMap::new(),
                            captured_at: Stamp::now(),
                        }],
                    },
                );
                store.save_session(lock, &session)?;
                Ok((session.id, dir))
            })
            .expect("a fresh state directory takes the daemonless lock");

        let lock = Arc::new(
            store
                .lock(LockProtocol::HeldForLifetime)
                .expect("nothing else holds a throw-away state directory"),
        );
        let shutdown = Shutdown::new();
        let wchd = Wchd::new(
            Arc::clone(&backend) as Arc<dyn CameraBackend>,
            SessionStore::new(state.root()),
            Arc::clone(&lock),
            shutdown.clone(),
        );
        let methods = daemon::server::mount(wchd.clone()).expect("the T5 surface mounts once");
        // The listener `main` opens, over the daemon's own store and stop token — so this suite
        // is about the shipped composition rather than one it arranged.
        let serving = http::open(
            "127.0.0.1:0".parse().expect("a literal address"),
            false,
            methods,
            wchd.previews(),
            Arc::new(SessionStore::new(state.root())),
            shutdown.clone(),
        )
        .await
        .expect("loopback on an ephemeral port");
        let token = secret(&serving);

        Samples {
            serving,
            token,
            shutdown,
            store: SessionStore::new(state.root()),
            lock,
            session,
            dir,
            outside,
            _wchd: wchd,
            _state: state,
        }
    }

    /// The request target D20's grid builds for the planted sample, with the credential on it.
    fn target(&self) -> String {
        self.reference(
            &self.session.to_string(),
            CONTROL,
            &PASS.to_string(),
            &VALUE.to_string(),
        )
    }

    /// A target naming whatever four things the caller says, credential included.
    ///
    /// Every field is a `&str` rather than a typed value, because half of what this suite drives
    /// is spellings no typed value can hold — `../..`, `%2e%2e%2f`, an absolute path.
    fn reference(&self, session: &str, control: &str, pass: &str, value: &str) -> String {
        format!(
            "{path}?{s}={session}&{c}={control}&{p}={pass}&{v}={value}&{t}={secret}",
            path = http::SESSION_PHOTO_PATH,
            s = http::SESSION_QUERY_PARAM,
            c = http::CONTROL_QUERY_PARAM,
            p = http::PASS_QUERY_PARAM,
            v = http::VALUE_QUERY_PARAM,
            t = http::TOKEN_QUERY_PARAM,
            secret = self.token,
        )
    }

    /// One `GET`, read to end as bytes.
    async fn get(&self, target: &str) -> Answer {
        self.request("GET", target, &[]).await
    }

    /// One request with the method spelled out and whatever headers a claim needs.
    async fn request(&self, method: &str, target: &str, headers: &[(&str, &str)]) -> Answer {
        let bound = self.serving.bound();
        let mut socket = TcpStream::connect(bound).await.expect("the listener is up");
        let mut head = format!(
            "{method} {target} HTTP/1.1\r\n\
             Host: {bound}\r\n\
             Connection: close\r\n"
        );
        for (name, value) in headers {
            head.push_str(&format!("{name}: {value}\r\n"));
        }
        head.push_str("\r\n");
        socket
            .write_all(head.as_bytes())
            .await
            .expect("the request was written");
        let mut whole = Vec::new();
        socket
            .read_to_end(&mut whole)
            .await
            .expect("the connection was readable");
        Answer::of(whole)
    }

    /// Stop it the way `main` stops it, and wait.
    async fn stop(self) {
        self.shutdown.cancel();
        self.serving.stopped().await.expect("the listener ended");
    }
}

/// The control the fixture sweeps, as a value.
fn control() -> ControlSlug {
    ControlSlug::parse(CONTROL).expect("a literal slug")
}

/// One whole HTTP answer, split into the head a claim reads and the body it compares.
///
/// Bytes and not a `String`, because the body under test is a JPEG: a lossy UTF-8 conversion
/// rewrites every byte above 0x7f, so a suite that compared text could not tell "served the
/// sample" from "served something else of the same length".
struct Answer {
    head: String,
    body: Vec<u8>,
}

impl Answer {
    fn of(whole: Vec<u8>) -> Answer {
        let split = whole
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .expect("an HTTP answer has a blank line after its head");
        Answer {
            head: String::from_utf8_lossy(&whole[..split]).into_owned(),
            body: whole[split + 4..].to_vec(),
        }
    }

    /// The status code, as a number — the claims here are inequalities as often as equalities.
    fn status(&self) -> u16 {
        self.head
            .lines()
            .next()
            .and_then(|line| line.split_whitespace().nth(1))
            .and_then(|code| code.parse().ok())
            .unwrap_or_else(|| panic!("no status code in {head:.64}", head = self.head))
    }

    /// Does the head carry this field, case-insensitively? hyper writes canonical lowercase.
    fn carries(&self, field: &str) -> bool {
        self.head
            .to_ascii_lowercase()
            .contains(&field.to_ascii_lowercase())
    }
}

/// The token out of the URL the daemon printed.
///
/// The same six lines `http.rs` and `preview.rs` carry, and duplicated for their reason: what
/// they share is a fact about the *URL D11 asks the daemon to print*, and each suite presents it
/// to a different surface.
fn secret(serving: &http::Serving) -> String {
    let url = serving.ready_to_open_url();
    url.split_once(&format!("{param}=", param = http::TOKEN_QUERY_PARAM))
        .map(|(_, secret)| secret.to_owned())
        .unwrap_or_else(|| panic!("no token in {url}"))
}

// ------------------------------------------------------------------------- the claims

#[tokio::test]
async fn a_sample_is_served_by_reference_and_the_bytes_are_the_ones_on_disk() {
    // D20's requirement, end to end: "sample review needs the sample bytes". The four
    // parameters are a *reference* — no path crosses the wire — and what comes back is the file
    // the store's own layout rules put on disk, byte for byte. A build that served a different
    // sample of the same size, or a re-encoded copy, fails the comparison rather than the shape.
    let samples = Samples::start().await;

    let answer = samples.get(&samples.target()).await;
    assert_eq!(answer.status(), 200, "{head:.256}", head = answer.head);
    assert_eq!(answer.body, SAMPLE_BYTES, "the bytes were not the sample's");
    // The type comes from the sample's own extension, which is the one fact about the file this
    // route reads out of the record rather than deriving.
    assert!(
        answer.carries("content-type: image/jpeg"),
        "{}",
        answer.head
    );
    // A stored frame is not put on the browser's disk a second time (design §5's reason for the
    // preview, which is the same reason here).
    assert!(answer.carries("cache-control: no-store"), "{}", answer.head);
    // And the file the route derived is where the store put it, which is what makes the claim
    // above about *derivation* rather than about a coincidence.
    let derived = samples
        .dir
        .join(SessionStore::photo_dir_rel(&control(), PASS).expect("a usable component"))
        .join(format!("{VALUE}.jpg"));
    assert!(derived.exists(), "{derived}");

    samples.stop().await;
}

#[tokio::test]
async fn a_reference_outside_the_session_is_not_found() {
    // Each of the four fields wrong in turn, so a build that stopped comparing one of them
    // fails here rather than by serving the wrong picture. The control arm is the interesting
    // one: `hue` is a real control slug that this session never swept, so the refusal is about
    // *this session's* record rather than about a spelling.
    let samples = Samples::start().await;

    // The intact reference, first, so every `404` below is a difference this test caused.
    assert_eq!(samples.get(&samples.target()).await.status(), 200);

    let elsewhere = Uuid::now_v7().to_string();
    for (about, target) in [
        (
            "a session nobody wrote",
            samples.reference(&elsewhere, CONTROL, "0", "42"),
        ),
        (
            "a control this session never swept",
            samples.reference(&samples.session.to_string(), "hue", "0", "42"),
        ),
        (
            "a pass this control never ran",
            samples.reference(&samples.session.to_string(), CONTROL, "1", "42"),
        ),
        (
            "a value this pass never photographed",
            samples.reference(&samples.session.to_string(), CONTROL, "0", "43"),
        ),
    ] {
        let answer = samples.get(&target).await;
        assert_eq!(
            answer.status(),
            404,
            "{about}: {head:.128}",
            head = answer.head
        );
        assert!(
            !answer.body.starts_with(&SAMPLE_BYTES[..3]),
            "{about} was answered with a sample"
        );
    }

    samples.stop().await;
}

#[tokio::test]
async fn a_record_that_names_a_file_the_layout_would_not_write_is_not_served() {
    // The hand-edited-document arm, over a real socket. `engine::store`'s header names this
    // input — "a session document is a *file the operator can edit*" — and the route's answer is
    // that the recorded path is admitted only when it is the one D9's layout rules derive.
    //
    // Three edits, each a different way the record can stop being the derivation, and the
    // control that makes them mean something: the same reference is a `200` before each edit.
    let samples = Samples::start().await;
    assert_eq!(samples.get(&samples.target()).await.status(), 200);

    for (about, photo) in [
        (
            "a path that climbs out of the session",
            format!("photos/{CONTROL}/{PASS}/../../../../not-a-sample.jpg"),
        ),
        ("an absolute path", samples.outside.to_string()),
        (
            "an extension this build never writes",
            format!("photos/{CONTROL}/{PASS}/{VALUE}.webp"),
        ),
    ] {
        let mut session = samples
            .store
            .load_session(&samples.dir)
            .expect("the fixture wrote a readable session");
        session
            .controls
            .get_mut(&control())
            .expect("the swept control")
            .samples[0]
            .photo = Utf8PathBuf::from(&photo);
        samples
            .store
            .save_session(&samples.lock, &session)
            .expect("this process holds the lock");

        let answer = samples.get(&samples.target()).await;
        assert_eq!(
            answer.status(),
            404,
            "{about} ({photo}): {head:.128}",
            head = answer.head
        );
        assert!(
            !answer.body.windows(4).any(|w| w == b"this"),
            "{about} reached a file outside the session"
        );
    }

    samples.stop().await;
}

#[tokio::test]
async fn no_caller_shaped_reference_becomes_a_filesystem_path() {
    // Every spelling a caller reaches for, against a file that really exists one directory up
    // (`Samples::outside`) — so a `404` here is "nothing was reached" rather than "nothing was
    // there". None of these can work, and the reason is structural rather than a filter: a
    // session is matched by a parsed `Uuid` against directory names the store walked, and a
    // control goes through D9's slug transform, so no byte a caller sends is ever a path
    // component.
    let samples = Samples::start().await;
    let session = samples.session.to_string();

    let traversals = [
        // ... in the session, which is not a UUID and so names no directory at all.
        samples.reference("../..", CONTROL, "0", "42"),
        samples.reference("%2E%2E%2F%2E%2E", CONTROL, "0", "42"),
        samples.reference(&format!("{session}%2F..%2F.."), CONTROL, "0", "42"),
        // ... in the control, which is slugged: `../../..` becomes nothing, and
        // `../../not-a-sample.jpg` becomes `not_a_sample_jpg`.
        samples.reference(&session, "../../..", "0", "42"),
        samples.reference(&session, "%2e%2e%2f%2e%2e", "0", "42"),
        samples.reference(&session, "../../not-a-sample.jpg", "0", "42"),
        samples.reference(&session, "/etc/passwd", "0", "42"),
        // ... in the pass, which is a `usize`.
        samples.reference(&session, CONTROL, "../0", "42"),
        samples.reference(&session, CONTROL, "-1", "42"),
        samples.reference(&session, CONTROL, "%2e%2e", "42"),
        // ... and in the value, which is an `i64`.
        samples.reference(&session, CONTROL, "0", "42.jpg"),
        samples.reference(&session, CONTROL, "0", "../42"),
    ];
    for target in traversals {
        let answer = samples.get(&target).await;
        assert_eq!(
            answer.status(),
            404,
            "{target}: {head:.128}",
            head = answer.head
        );
        assert!(
            !answer.body.windows(4).any(|w| w == b"this"),
            "{target} reached the file beside the session tree"
        );
        assert!(
            !answer.body.starts_with(&SAMPLE_BYTES[..3]),
            "{target} reached the sample by a spelling nobody meant"
        );
    }
    // The control that makes every line above a claim: the well-formed reference still answers.
    assert_eq!(samples.get(&samples.target()).await.status(), 200);

    samples.stop().await;
}

#[tokio::test]
async fn a_head_answers_about_the_route_and_opens_no_file() {
    // Note **N179**'s precedent, applied to a store instead of a device. The proof that the
    // `HEAD` did not look is that it disagrees with the `GET` about the *same* reference, twice
    // over and for two different reasons:
    //
    //   - a session this store does not hold: `GET` walks the tree and says `404`, `HEAD` says
    //     `200` because it never walked it;
    //   - the planted sample with its **file deleted** and its record left alone: `GET` reads
    //     the record, fails to open the file and says `404`; `HEAD` still says `200`, which it
    //     could not do if it had opened anything.
    //
    // And what it must not carry is a `Content-Length`: RFC 9110 §8.6 permits that field on a
    // `HEAD` only when it equals what a `GET` would send, which is a number this endpoint
    // deliberately does not go and look up.
    let samples = Samples::start().await;

    let elsewhere = samples.reference(&Uuid::now_v7().to_string(), CONTROL, "0", "42");
    assert_eq!(samples.get(&elsewhere).await.status(), 404);
    let described = samples.request("HEAD", &elsewhere, &[]).await;
    assert_eq!(
        described.status(),
        200,
        "{head:.256}",
        head = described.head
    );
    assert!(
        !described.carries("content-length:"),
        "{head:.256}",
        head = described.head
    );
    assert!(described.body.is_empty(), "a HEAD answered with a body");
    // It is still an answer about *this* route, which a bare `200` from anywhere would not be.
    assert!(
        described.carries("cache-control: no-store"),
        "{head:.256}",
        head = described.head
    );

    let file = samples
        .dir
        .join(SessionStore::photo_dir_rel(&control(), PASS).expect("a usable component"))
        .join(format!("{VALUE}.jpg"));
    assert_eq!(samples.get(&samples.target()).await.status(), 200);
    std::fs::remove_file(&file).expect("the fixture's own scratch file");
    assert_eq!(
        samples.get(&samples.target()).await.status(),
        404,
        "a record naming a file that is gone was served"
    );
    assert_eq!(
        samples
            .request("HEAD", &samples.target(), &[])
            .await
            .status(),
        200,
        "a HEAD of this route opened the file it was asked about"
    );

    samples.stop().await;
}

#[tokio::test]
async fn a_request_that_names_no_reference_is_told_what_to_name() {
    // The `400` arm, at every parameter, over a real socket — and its body says what to send
    // rather than what this machine holds. A route that answered `404` here would be telling a
    // client that its reference is wrong when it has not made one.
    let samples = Samples::start().await;
    let credential = format!(
        "{t}={secret}",
        t = http::TOKEN_QUERY_PARAM,
        secret = samples.token
    );

    let bare = samples
        .get(&format!(
            "{path}?{credential}",
            path = http::SESSION_PHOTO_PATH
        ))
        .await;
    assert_eq!(bare.status(), 400, "{head:.128}", head = bare.head);
    let told = String::from_utf8_lossy(&bare.body).into_owned();
    for param in [
        http::SESSION_QUERY_PARAM,
        http::CONTROL_QUERY_PARAM,
        http::PASS_QUERY_PARAM,
        http::VALUE_QUERY_PARAM,
    ] {
        assert!(told.contains(param), "{told}");
    }
    // ... and it names nothing of the operator's: not a session, not a task, not a path.
    assert!(!told.contains(&samples.session.to_string()), "{told}");
    assert!(!told.contains(samples.dir.as_str()), "{told}");

    // One parameter short is the same answer, at each of the four in turn.
    for missing in [
        http::SESSION_QUERY_PARAM,
        http::CONTROL_QUERY_PARAM,
        http::PASS_QUERY_PARAM,
        http::VALUE_QUERY_PARAM,
    ] {
        let whole = samples.target();
        let (path, query) = whole.split_once('?').expect("the target carries a query");
        let target = format!(
            "{path}?{kept}",
            kept = query
                .split('&')
                .filter(|pair| !pair.starts_with(&format!("{missing}=")))
                .collect::<Vec<&str>>()
                .join("&")
        );
        assert_eq!(
            samples.get(&target).await.status(),
            400,
            "{missing} was optional after all"
        );
    }

    samples.stop().await;
}

/// Every file the session tree holds, from the filesystem rather than from a list.
///
/// A **derived population** (docs/9's second structural rule): the session directory contains a
/// document, a log and a photograph today, and whatever a later phase adds tomorrow — and the
/// claim below is about all of them, so it walks rather than names. Naming them would also be
/// this file spelling D9's own file names, which `atomic-write-home`'s header asks a test not to
/// do beside a fixture write.
fn every_file_under(dir: &Utf8Path) -> Vec<Utf8PathBuf> {
    let mut found = Vec::new();
    let entries = std::fs::read_dir(dir).expect("the fixture's own session tree");
    for entry in entries {
        let path = Utf8PathBuf::from_path_buf(entry.expect("a readable entry").path())
            .expect("this suite writes UTF-8 paths");
        if path.is_dir() {
            found.extend(every_file_under(&path));
        } else {
            found.push(path);
        }
    }
    found
}

#[tokio::test]
async fn no_other_door_on_this_listener_serves_a_stored_frame() {
    // **The privacy clause, as a claim about the whole listener.** The samples route is gated;
    // what this asserts is that adding it did not open a second, ungated way to the same bytes.
    // The asset fallback is the only thing this daemon serves without a credential (note N82),
    // it is a lookup in a table of names fixed at compile time, and nothing in the session tree
    // is in that table — so every file the tree holds, asked for by every shape of its own path,
    // is refused by the asset half, with **no credential**, which is where a stranger would ask.
    let samples = Samples::start().await;
    let mut doors = Vec::new();
    let held = every_file_under(&samples.dir);
    assert!(
        held.len() >= 3,
        "a session tree with a swept control holds a document, a log and a photograph: {held:?}"
    );
    for file in &held {
        // Its absolute path, which is already a well-formed request target; the same path
        // relative to the session directory, which is what the record inside the document
        // carries; and its bare name, which is the shape the asset table's keys have.
        doors.push(file.to_string());
        let relative = file
            .strip_prefix(&samples.dir)
            .expect("the walk stayed inside the tree");
        doors.push(format!("/{relative}"));
        doors.push(format!("/{name}", name = file.file_name().unwrap_or("")));
    }
    for door in doors {
        for (about, headers) in [
            ("with no credential", Vec::new()),
            (
                "with the token",
                vec![("Authorization", format!("Bearer {t}", t = samples.token))],
            ),
        ] {
            let borrowed: Vec<(&str, &str)> = headers
                .iter()
                .map(|(name, value)| (*name, value.as_str()))
                .collect();
            let answer = samples.request("GET", &door, &borrowed).await;
            assert_ne!(
                answer.status(),
                200,
                "{door} {about} was served by a door that is not the gated route"
            );
            assert!(
                !answer.body.starts_with(&SAMPLE_BYTES[..3]),
                "{door} {about} handed back a stored frame"
            );
        }
    }
    // The control: the one door that is meant to serve it still does.
    assert_eq!(samples.get(&samples.target()).await.status(), 200);

    samples.stop().await;
}
