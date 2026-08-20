//! What the shipped web client asks this daemon for, asked over the transports it asks on
//! (design **D10**, **D11**, §2.7; docs/7 P5c).
//!
//! `http.rs` and `web_rpc.rs` are this suite's neighbours and the three divide the listener
//! between them. That file's subject is the **credential and D11's matrix**; that one's is
//! **what the wire surface carries over TCP** — the same registration, the subscriptions, the
//! bound, the stop. This one's subject is the **client**: `crates/web/assets/` is hand-written
//! HTML, CSS and ES modules, and every claim below is one of them depending on something this
//! daemon does. (How many of each is `webcam-handler-web`'s own header, stated there and
//! nowhere else — a third copy of that count is exactly what note **N153** is about, and this
//! file used to carry one.)
//!
//! ## What this suite may claim, and the line it does not cross
//!
//! docs/7 P5c draws it: "protocol-level integration tests; whatever DTO-render logic is
//! assertable without a browser — the browser truth is P5d's, and **a browser behavior
//! verified only through the JSON the page consumes is not verified**" (rubric B7). So every
//! test here asserts something about *the daemon's answers*, in the shape a module in
//! `assets/` reads them, and **none of them asserts that anything renders**. There is no
//! headless DOM here, no JSDOM, and no assertion of the form "and therefore the `<select>`
//! has two options": that a sparse menu becomes a `<select>` carrying the device's own
//! indices is P5d's, in a real Chromium, one sub-milestone away. What this file establishes
//! is the half that would make P5d's failure ambiguous if it were missing — that the DTO
//! really does arrive with the holes, the INACTIVE flag, the out-of-range value and the
//! unnameable type in it, over the real socket, from a real daemon.
//!
//! The other half of P5c's assertable surface is `webcam-handler-web`'s own suite: every
//! module embedded, every module typed, and the module graph closed with nothing orphaned in
//! it. Those are properties of a table of bytes; these are properties of a conversation.
//!
//! ## Its own fixture, and why it is not `support/fixture.rs`
//!
//! `preview.rs`'s reason, and the same one: this suite opens a preview, so its camera's MJPEG
//! modes have to be small or the fixture spends its time rendering quarter-megapixel frames in
//! a loop. The shared fixture replays `synthetic_basic` at up to 3840×2160. The rewrite here
//! is the licence `support/fixture.rs::replaying` states — a field changed into a shape a real
//! device has — and 160×120 MJPG is an ordinary webcam mode. Everything else about
//! `synthetic_basic` is left exactly as it is, because the control set *is* the subject: it
//! carries all four edges design §1.2 recorded, on purpose (`testkit::fixtures`).
//!
//! It also needs a **second client**, which no other web suite does: the calibration view
//! watches sweeps it did not start, so the fixture serves the Unix socket as well and one
//! test starts a sweep there and reads it here.
//!
//! ## Nothing here waits on a clock
//!
//! Every read ends when the daemon writes: a frame, an answer, a notification, or a `watch`
//! channel the daemon publishes and this file *awaits*. A daemon that answered none of them
//! becomes a nextest `TIMEOUT` with a test's name on it (`.config/nextest.toml`) rather than a
//! hang.

#[path = "support/tcp.rs"]
mod tcp;
#[path = "support/ws.rs"]
mod ws;

use std::sync::Arc;

use api::rpc_code;
use camino::Utf8PathBuf;
use daemon::http::{self, CAMERA_QUERY_PARAM, PREVIEW_PATH, RPC_PATH, TOKEN_QUERY_PARAM};
use daemon::server::Wchd;
use daemon::shutdown::Shutdown;
use daemon::uds::{self, SocketDir};
use engine::paths::TempRuntimeDir;
use engine::store::{LockProtocol, SessionStore, StoreLock, TempStore};
use fake::FakeBackend;
use schema::backend::CameraBackend;
use schema::camera::{CameraId, FrameInterval, FrameSize, FrameSizeInfo, PixelFormat};
use schema::error::ErrorKind;
use schema::session::{Session, SessionRef, SweepRequest, SweepSpec};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::TcpStream;

use crate::tcp::Answer;
use crate::ws::Ws;

// ------------------------------------------------------------------------- the fixture

/// One daemon, one small-MJPEG camera, both transports, and the token the page is opened
/// with.
struct Client {
    wchd: Wchd,
    camera: CameraId,
    serving: http::Serving,
    socket: Utf8PathBuf,
    uds: uds::Serving,
    token: String,
    shutdown: Shutdown,
    _lock: Arc<StoreLock>,
    _state: TempStore,
    _runtime: TempRuntimeDir,
}

impl Client {
    /// Start one. Must be called from inside a tokio runtime.
    async fn start() -> Client {
        let backend = Arc::new(
            FakeBackend::new(vec![small_mjpeg_camera()])
                .expect("the synthetic profile is this build's version"),
        );
        let camera = backend
            .enumerate()
            .expect("the fake enumerates what it replays")
            .first()
            .expect("one camera")
            .id
            .clone();

        let state = TempStore::new().expect("a state directory");
        let store = SessionStore::new(state.root());
        let lock = Arc::new(
            store
                .lock(LockProtocol::HeldForLifetime)
                .expect("nothing else holds a throw-away state directory"),
        );
        let runtime = TempRuntimeDir::new().expect("a runtime directory");
        let dir = SocketDir::prepare(&runtime.env()).expect("a fresh, private socket directory");
        let listener = dir.bind(&lock).expect("nothing is in the way");

        let shutdown = Shutdown::new();
        let wchd = Wchd::new(
            Arc::clone(&backend) as Arc<dyn CameraBackend>,
            SessionStore::new(state.root()),
            Arc::clone(&lock),
            shutdown.clone(),
        );
        let methods = daemon::server::mount(wchd.clone()).expect("the T5 surface mounts once");
        // The listener `main` opens, over the daemon's own fan-out and stop token — so this
        // suite is about the shipped composition rather than one it arranged.
        let serving = http::open(
            "127.0.0.1:0".parse().expect("a literal address"),
            false,
            methods.clone(),
            wchd.previews(),
            Arc::new(SessionStore::new(state.root())),
            shutdown.clone(),
        )
        .await
        .expect("loopback on an ephemeral port");
        let token = secret(&serving);

        Client {
            wchd,
            camera,
            serving,
            socket: dir.socket_path(),
            uds: uds::serve(listener, methods),
            token,
            shutdown,
            _lock: lock,
            _state: state,
            _runtime: runtime,
        }
    }

    /// Open a WebSocket the way `assets/credential.js` builds one: the path from
    /// `daemon::http::rpc::RPC_PATH`, the credential in the query string, and nothing else.
    ///
    /// One credential, in one form. Note **N74**'s gate admits a request only when *every*
    /// credential it presents verifies, so a client that sent the token twice in two
    /// spellings would be one drift away from refusing itself; `credential.js` is the single
    /// place in that directory that writes it, and this is the request it writes.
    async fn page_socket(&self) -> Ws<TcpStream> {
        let bound = self.serving.bound();
        let stream = TcpStream::connect(bound).await.expect("the listener is up");
        let target = format!("{RPC_PATH}?{TOKEN_QUERY_PARAM}={token}", token = self.token);
        Ws::upgrade(stream, &bound.to_string(), &target)
            .await
            .unwrap_or_else(|status| panic!("the daemon refused the page's socket: {status}"))
    }

    /// A second client on the *Unix* socket — an agent, or `webcam-handler-client`.
    ///
    /// The calibration view's whole reason for existing: `wch_subscribe_calibration` takes no
    /// parameters because the stream is per **client** and every event carries its session id
    /// (`crates/api`'s `WchEvents`), so a browser can watch a sweep somebody else started.
    async fn other_client(&self) -> Ws<tokio::net::UnixStream> {
        Ws::connect(&self.socket).await
    }

    /// One anonymous `GET` for a static asset, exactly as a browser fetches a module.
    ///
    /// No credential, and that is the assertion rather than an omission: since the owner's
    /// ruling of 2026-08-12 the client's own files are served unauthenticated (note **N82**),
    /// and `credential.js` deliberately sends the token to nothing but the camera-bearing routes
    /// `http::CAMERA_BEARING_PATHS` names — three of them since D20, which is why the sentence
    /// quantifies over the list rather than counting it.
    async fn asset(&self, path: &str) -> Answer {
        tcp::get(self.serving.bound(), path, &[])
            .await
            .expect("the listener is up")
    }

    /// The request target `assets/credential.js` builds for the preview `<img>`.
    fn preview_target(&self) -> String {
        format!(
            "{PREVIEW_PATH}?{CAMERA_QUERY_PARAM}={id}&{TOKEN_QUERY_PARAM}={token}",
            // What `encodeURIComponent` produces for an id with a `:` in it, which is what
            // `URLSearchParams` writes and therefore what the shipped page sends.
            id = self.camera.as_str().replace(':', "%3A"),
            token = self.token,
        )
    }

    /// Stop both transports the way `main` stops them, and wait for both.
    async fn stop(mut self) {
        self.shutdown.cancel();
        self.serving
            .stopped()
            .await
            .expect("the web listener ended");
        self.uds.stop();
        self.uds
            .stopped()
            .await
            .expect("the Unix socket was asked to stop");
    }
}

/// The token out of the URL the daemon printed, never off the `Token` value.
///
/// `http.rs`'s reason: a build that published one secret and gated on another would pass
/// every assertion here if this reached into the value instead of reading the line an
/// operator copies into a browser.
fn secret(serving: &http::Serving) -> String {
    let url = serving.ready_to_open_url();
    url.split_once(&format!("?{TOKEN_QUERY_PARAM}="))
        .map(|(_url, token)| token.to_owned())
        .expect("D11's default cell publishes a token")
}

/// The committed profile with its MJPEG modes rewritten small.
///
/// `preview.rs`'s fixture, and its argument: 160×120 at 30 fps is an ordinary webcam mode, and
/// it makes every frame in this file a few kilobytes rather than a quarter-megapixel render.
/// **The controls are untouched**, which is the point — they are what the panel is generated
/// from.
fn small_mjpeg_camera() -> schema::profile::DeviceProfile {
    let mut profile = testkit::fixtures::synthetic_basic();
    for format in &mut profile.invariant.formats {
        if format.pixel_format == PixelFormat::MJPG {
            format.sizes = vec![FrameSizeInfo {
                size: FrameSize::Discrete {
                    width: 160,
                    height: 120,
                },
                intervals: vec![FrameInterval::Discrete {
                    numerator: 1,
                    denominator: 30,
                }],
            }];
        }
    }
    profile
}

/// The `result` of a JSON-RPC answer, or a failure naming what the daemon said instead.
fn result(answer: &Value, about: &str) -> Value {
    answer
        .get("result")
        .unwrap_or_else(|| panic!("{about} was refused: {answer}"))
        .clone()
}

/// The `error` of a JSON-RPC answer, or a failure saying it was not refused.
///
/// **Nothing here prints a whole answer.** A `wch_photo` reply carries the camera's bytes as
/// base64 and a frame may contain a person (AGENTS), so the failure message names the error
/// member and never the document — `preview.rs` makes the same promise for the same reason.
fn refusal<'a>(answer: &'a Value, about: &str) -> &'a Value {
    answer
        .get("error")
        .unwrap_or_else(|| panic!("{about} was answered rather than refused"))
}

// -------------------------------------------------------- the client's own files, served

#[tokio::test(flavor = "multi_thread")]
async fn every_module_of_the_client_is_served_with_a_content_type_a_browser_will_run() {
    // **A module served as `application/octet-stream` is a client that does not run**, and
    // that failure is silent from both ends: the daemon logs a `200`, and the browser fetches
    // the file, declines to execute it, and renders a page with nothing in it.
    // `http.rs`'s `every_asset_this_build_embeds_is_served_to_a_request_presenting_nothing`
    // establishes the *status* over this population; this establishes the header beside it,
    // which is the half that decides whether the bytes do anything.
    //
    // The population is `web::paths()` — the crate's own table — so every module P5c added
    // joins by existing, and the expectation is `web::get`'s own answer rather than a list
    // here: what would be wrong is the daemon serving something *other* than what the asset
    // crate types the file as, and comparing against a transcription could not see that.
    let client = Client::start().await;

    let mut served = 0_usize;
    let mut modules = 0_usize;
    for name in web::paths() {
        let typed = web::get(&name).expect("a path the embed itself just listed");
        let answer = client.asset(&format!("/{name}")).await;
        assert_eq!(answer.status(), 200, "/{name}");
        assert_eq!(
            answer.header("Content-Type"),
            Some(typed.content_type()),
            "/{name} is served as something other than what this build types it as"
        );
        assert_ne!(
            typed.content_type(),
            web::UNKNOWN_CONTENT_TYPE,
            "/{name} reaches a browser as an unknown type, which for a module means it never \
             runs"
        );
        served += 1;
        if name.ends_with(".js") {
            modules += 1;
        }
    }
    assert!(served > 0, "the asset table is empty");
    // Not vacuous about the thing this test is actually about: a client with no ES modules in
    // it would satisfy every assertion above and would not be §2.7's client.
    assert!(modules > 0, "this build embeds no ES modules at all");

    // And the document at `/` is the one that loads them. The entry point is named in the
    // page rather than known to the daemon (`web::INDEX` is the only file it spells), so this
    // is where "the URL an operator opens starts the client" is a fact rather than a diagram.
    let page = client.asset("/").await;
    assert_eq!(page.status(), 200);
    assert!(
        page.body().contains("app.js"),
        "the page served at / does not name an entry module"
    );

    client.stop().await;
}

/// Where `source` fails to declare `name` as `value`, exactly once. Empty is green.
///
/// A pure function over text, so the arms below can drive it with a number the tree does not
/// carry — the half that proves a reconciler can go red (AGENTS rule 2).
fn declares(source: &str, name: &str, value: u64) -> Vec<String> {
    let mut drift = Vec::new();
    let exact = format!("const {name} = {value};");
    if source.matches(exact.as_str()).count() != 1 {
        drift.push(format!(
            "assets/rpc.js does not declare `{exact}` exactly once; `schema::limits` says \
             {value}"
        ));
    }
    // The *name* has to be there once whatever its value is, so a constant that was renamed or
    // deleted is a different sentence from one that drifted. Without it the line above reports
    // "the number is wrong" about a number that is no longer in the file at all.
    let declaration = format!("const {name} =");
    let declared = source.matches(declaration.as_str()).count();
    if declared != 1 {
        drift.push(format!(
            "assets/rpc.js declares `{name}` {declared} time(s); a bound the page reads twice is \
             two bounds"
        ));
    }
    drift
}

#[test]
fn the_bounds_the_page_runs_on_are_the_ones_this_build_declares() {
    // **The one place a `limits` constant crosses into JavaScript, checked** (docs/11 **L38**,
    // note **N157**). AGENTS puts every bound in `webcam-handler-schema::limits` and asks that
    // something read each one, and until 2026-08-16 the web client read none of them: it had no
    // per-call timeout and no liveness at all, so a socket severed without a FIN left every call
    // parked under a banner reading `connected`. The owner's ruling gave it two, and a browser
    // cannot `use` a Rust constant — so the numbers are a second copy, and a second copy is only
    // as good as the thing that reconciles it. This is that thing.
    //
    // It lives in this suite rather than in `webcam-handler-web` because this is the file whose
    // subject is *what the shipped client asks this daemon for*: it already builds the two
    // camera-bearing URLs from `daemon::http`'s own constants rather than from transcriptions of
    // them, for the reason its `wire` helper states. `crates/web` has one dependency on purpose
    // and its manifest argues at length for that, so the crate that owns both halves of this
    // comparison is this one.
    //
    // What it reads is `web::get`, not the file on disk: the bytes asserted about are the bytes
    // a browser is served (`debug-embed`), so a source tree that had been edited without a
    // rebuild cannot make this pass.
    let module = web::get("rpc.js").expect("the client's JSON-RPC helper");
    let source = String::from_utf8(module.bytes().to_vec()).expect("the module is UTF-8");

    let bounds = [
        ("CALL_TIMEOUT_MS", schema::limits::CLIENT_REQUEST_TIMEOUT_MS),
        ("HEARTBEAT_MS", schema::limits::CLIENT_WS_HEARTBEAT_MS),
    ];
    // Distinct, or one declaration in the page could satisfy both rows and the pair would be
    // checking half of what it claims to.
    assert_ne!(
        bounds[0].1, bounds[1].1,
        "the two bounds are the same number"
    );

    for (name, value) in bounds {
        let drift = declares(&source, name, value);
        assert!(drift.is_empty(), "{drift:?}");
        // Both directions, driven rather than asserted about: a number that moved in `limits`
        // and not in the page is the whole failure mode, so it is the one that has to be seen.
        assert!(
            !declares(&source, name, value + 1).is_empty(),
            "`{name}` was accepted at a value this build does not declare"
        );
        assert!(
            !declares(&source, &format!("{name}_THAT_IS_NOT_THERE"), value).is_empty(),
            "a constant this page does not declare at all was accepted"
        );
    }
}

/// Where `source` fails to declare `name` as the string literal `value`, exactly once. Empty is
/// green.
///
/// [`declares`]'s sibling, for the other kind of second copy a browser is forced into: that one
/// reconciles the *numbers* `schema::limits` owns, this one the *names* `daemon::http` owns. Same
/// two sentences, and the split between them is the same — a constant that drifted and a constant
/// that is no longer there are different findings, and reporting the first about the second sends
/// a reader looking for a value in a file that has no such name in it.
fn spells(source: &str, name: &str, value: &str) -> Vec<String> {
    let mut drift = Vec::new();
    let exact = format!("const {name} = \"{value}\";");
    if source.matches(exact.as_str()).count() != 1 {
        drift.push(format!(
            "assets/credential.js does not declare `{exact}` exactly once; `daemon::http` says \
             `{value}`"
        ));
    }
    let declaration = format!("const {name} =");
    let declared = source.matches(declaration.as_str()).count();
    if declared != 1 {
        drift.push(format!(
            "assets/credential.js declares `{name}` {declared} time(s); a wire name the page \
             writes twice is two wire names"
        ));
    }
    drift
}

/// Every route `source` declares — the `const NAME = "/…";` lines, by value.
///
/// A **derived population**, which is what makes the partition below a claim rather than a list:
/// a route the page learns to build and nobody gated is as much a finding as a gated route the
/// page can no longer reach, and neither is visible to a check that walks a hand-written table.
/// The shape it matches is the one `credential.js` is written in and says it is written in, one
/// declaration per line.
fn routes_the_page_builds(source: &str) -> std::collections::BTreeSet<String> {
    source
        .lines()
        .filter_map(|line| {
            let rest = line.trim().strip_prefix("const ")?;
            let (_, value) = rest.split_once(" = \"")?;
            let path = value.strip_suffix("\";")?;
            path.starts_with('/').then(|| path.to_owned())
        })
        .collect()
}

#[test]
fn the_urls_the_page_builds_are_the_routes_this_daemon_serves() {
    // **The third side of a triangle whose other two sides were already there.**
    // `scripts/gates/web-routes-are-gated.sh` reconciles this crate's route *registrations*
    // against `http::CAMERA_BEARING_PATHS`, and `every_camera_bearing_route_is_behind_the_gate`
    // (crates/daemon/tests/preview.rs) reconciles that list against a `401` on a socket. Neither
    // has the *page* as a subject — and `/session-photo`'s only consumer is a page, which built
    // its URL out of five literals nothing compared with anything (note **N275**).
    //
    // What that cost was already in the tree when this was written: `credential.js`'s doc comment
    // named `daemon::http::samples::SESSION_PHOTO_PATH`, and there is no `samples` module. The
    // spelling that mattered happened to be right; the sentence beside it had already drifted,
    // which is what an unreconciled second copy looks like on the way to being wrong.
    //
    // `/rpc` and `/preview` have consumer proof incidentally — a drifted `RPC_PATH` fails every
    // browser claim that opens the page's socket, and a drifted `PREVIEW_PATH` or camera
    // parameter fails the claim that reads `naturalWidth` off a painted frame. `/session-photo`
    // had neither, and the browser rung self-skips on a host without node, so the text
    // reconciliation is the half that runs everywhere.
    //
    // What it reads is `web::get`, not the file on disk — `the_bounds_the_page_runs_on_are_the_
    // ones_this_build_declares`' reason, verbatim: the bytes asserted about are the bytes a
    // browser is served (`debug-embed`), so a source tree edited without a rebuild cannot make
    // this pass.
    let module = web::get("credential.js").expect("the client's one credential writer");
    let source = String::from_utf8(module.bytes().to_vec()).expect("the module is UTF-8");

    let names = [
        ("RPC_PATH", http::RPC_PATH),
        ("PREVIEW_PATH", http::PREVIEW_PATH),
        ("SESSION_PHOTO_PATH", http::SESSION_PHOTO_PATH),
        ("TOKEN_PARAM", http::TOKEN_QUERY_PARAM),
        ("CAMERA_PARAM", http::CAMERA_QUERY_PARAM),
        ("SESSION_PARAM", http::SESSION_QUERY_PARAM),
        ("CONTROL_PARAM", http::CONTROL_QUERY_PARAM),
        ("PASS_PARAM", http::PASS_QUERY_PARAM),
        ("VALUE_PARAM", http::VALUE_QUERY_PARAM),
    ];
    // Pairwise distinct, or one declaration in the page could satisfy two rows and this pair of
    // sentences would be checking less than it claims to — `the_bounds…`' `assert_ne!`,
    // generalised to nine.
    for (i, (left_name, left)) in names.iter().enumerate() {
        for (right_name, right) in &names[i + 1..] {
            assert_ne!(
                left, right,
                "`{left_name}` and `{right_name}` are the same string, so one declaration in the \
                 page satisfies both rows"
            );
        }
    }

    for (name, value) in names {
        let drift = spells(&source, name, value);
        assert!(drift.is_empty(), "{drift:?}");
        // Both directions, driven rather than asserted about: a name that moved in `daemon::http`
        // and not in the page is the whole failure mode, so it is the one that has to be seen.
        assert!(
            !spells(&source, name, &format!("{value}-not-this")).is_empty(),
            "`{name}` was accepted at a spelling this daemon does not serve"
        );
        assert!(
            !spells(&source, &format!("{name}_THAT_IS_NOT_THERE"), value).is_empty(),
            "a wire name this page does not declare at all was accepted"
        );
    }

    // **The partition**: every route the page builds is one this daemon keeps behind the gate,
    // and every route behind the gate is one the page can reach. The second half is what a
    // reviewer reads when a camera-bearing route is added and its consumer is forgotten; the
    // first is what they read when a page learns to fetch something nobody gated.
    let built = routes_the_page_builds(&source);
    let gated: std::collections::BTreeSet<String> = http::CAMERA_BEARING_PATHS
        .iter()
        .map(|path| (*path).to_owned())
        .collect();
    assert_eq!(
        built, gated,
        "the routes assets/credential.js builds and the routes on \
         `daemon::http::CAMERA_BEARING_PATHS` are not the same set"
    );

    // Both directions of the partition, driven over text this tree does not carry — the same
    // reason the drift arms above are driven rather than reasoned about.
    let without = source.replace(
        &format!(
            "const SESSION_PHOTO_PATH = \"{}\";",
            http::SESSION_PHOTO_PATH
        ),
        "",
    );
    assert_ne!(
        routes_the_page_builds(&without),
        gated,
        "a page that builds no URL for a camera-bearing route was accepted"
    );
    let with_extra = format!("{source}\nconst SNAPSHOT_PATH = \"/snapshot\";\n");
    assert_ne!(
        routes_the_page_builds(&with_extra),
        gated,
        "a page that builds a URL for a route nobody gated was accepted"
    );
}

/// The body of the top-level function `name` in `source`, brace to brace.
///
/// The same coarse rule [`code_lines`] states and for the same reason: from the line that
/// declares the function to the `}` in the first column that ends it. What is read out of it is
/// property *names*, and a JavaScript parser to find those would be a second opinion about a
/// language this repository does not otherwise read.
fn body_of(source: &str, name: &str) -> String {
    let opening = format!("function {name}(");
    let start = source
        .find(opening.as_str())
        .unwrap_or_else(|| panic!("assets/calibrate-flow.js declares `{name}`"));
    let rest = &source[start..];
    let end = rest
        .find("\n}\n")
        .unwrap_or_else(|| panic!("`{name}` is closed by a `}}` in the first column"));
    rest[..end].to_owned()
}

/// Every wire field `name`'s body reads off a document this daemon answered.
///
/// **A derived population**, which is the whole point: the four rendering branches this
/// reconciles were written by reading `schema::report::WriteReport` and
/// `schema::snapshot::RestoreReport` and typing the field names out again, and note **N273** is
/// what that costs when one of them is wrong — `report.applied.length` and `report.complete`,
/// neither of which any version of either type has carried, on a path no claim walked. The
/// receivers are named because they are the ones bound to a document off the wire: `report` is
/// the answer, `write` is an element of `report.writes`, `outcome` is an element of
/// `report.outcomes`.
fn wire_fields_read(body: &str) -> std::collections::BTreeSet<String> {
    let mut fields = std::collections::BTreeSet::new();
    for receiver in ["report.", "write.", "outcome."] {
        let mut rest = body;
        while let Some(at) = rest.find(receiver) {
            rest = &rest[at + receiver.len()..];
            let field: String = rest
                .chars()
                .take_while(|c| c.is_ascii_lowercase() || *c == '_')
                .collect();
            if !field.is_empty() {
                fields.insert(field);
            }
        }
    }
    fields
}

/// Every object key anywhere in `document`, at any depth.
fn keys_of(document: &Value) -> std::collections::BTreeSet<String> {
    let mut keys = std::collections::BTreeSet::new();
    match document {
        Value::Object(map) => {
            for (key, value) in map {
                keys.insert(key.clone());
                keys.extend(keys_of(value));
            }
        }
        Value::Array(items) => {
            for item in items {
                keys.extend(keys_of(item));
            }
        }
        _ => {}
    }
    keys
}

/// Which of `fields` no document in `documents` carries.
fn unanswered(documents: &[&Value], fields: &std::collections::BTreeSet<String>) -> Vec<String> {
    let mut carried = std::collections::BTreeSet::new();
    for document in documents {
        carried.extend(keys_of(document));
    }
    fields.difference(&carried).cloned().collect()
}

#[test]
fn the_report_fields_the_flow_renders_are_fields_these_reports_carry() {
    // **Note N273's class, closed for the branches no claim walks.** Apply and Restore render
    // `schema::report::WriteReport` and `schema::snapshot::RestoreReport`, and the versions that
    // shipped read `report.applied.length` and `report.complete` — two fields no version of
    // either type has ever carried, so every click threw a `TypeError` into `#flow-status` after
    // the verb had already succeeded. The browser rung now clicks both buttons, which holds the
    // *ordinary* answer. It does not hold the four branches underneath: automation switched off
    // to make a write stick, a write the device clamped, a control that could not be put back,
    // and a stranded sweep given back. Every one of those reads more field names off the wire,
    // on a path no fixture this rung serves produces.
    //
    // So the reports are built here, in Rust, out of the real types — which is what makes this
    // go red on a **rename** rather than on a shape a test invented. A doctored JSON answer fed
    // to the page would agree with a renamed field forever, because the invented fixture and the
    // page would both be wrong in the same direction (note **N252**'s family).
    //
    // What it deliberately does not claim: that the page *renders* anything. That is P5d's line,
    // stated at the top of this file, and the sentence it produces is the browser rung's.
    let module = web::get("calibrate-flow.js").expect("the client's calibration flow");
    let source = String::from_utf8(module.bytes().to_vec()).expect("the module is UTF-8");

    let slug = |name: &str| schema::control::ControlSlug::parse(name).expect("a non-empty slug");
    let value = |v: i64| schema::control::ControlValue::Int(v);
    let clamped = schema::control::Applied {
        control: schema::control::ControlId(0x0098_0900),
        slug: slug("brightness"),
        requested: value(300),
        applied: value(255),
        warnings: Vec::new(),
    };
    let write_report = serde_json::to_value(schema::report::WriteReport {
        camera: CameraId::parse("cam:one").expect("a non-empty camera id"),
        writes: vec![clamped.clone()],
        disabled_automation: vec![slug("auto_exposure")],
    })
    .expect("a write report serializes");
    let restore_report = serde_json::to_value(schema::snapshot::RestoreReport {
        outcomes: vec![
            schema::snapshot::RestoreOutcome::Restored {
                applied: clamped.clone(),
            },
            schema::snapshot::RestoreOutcome::AlreadyCorrect {
                control: slug("contrast"),
            },
            schema::snapshot::RestoreOutcome::OwnedByAutomation {
                control: slug("exposure_time_absolute"),
                automation: Some(slug("auto_exposure")),
            },
            schema::snapshot::RestoreOutcome::Unrestorable {
                control: slug("gain"),
                reason: schema::snapshot::UnrestorableReason::Volatile,
            },
        ],
        freed: vec![slug("saturation")],
    })
    .expect("a restore report serializes");

    // Every branch is present in the two documents above, so a field read on any of them is a
    // field this reconciles. A report with an empty `disabled_automation` or no `Unrestorable`
    // would make the sentences below vacuous about exactly the branches they exist for.
    let documents = [&write_report, &restore_report];
    for name in ["applySentence", "restoreSentence"] {
        let fields = wire_fields_read(&body_of(&source, name));
        assert!(
            !fields.is_empty(),
            "`{name}` reads no wire field at all, so this reconciliation is about nothing"
        );
        let missing = unanswered(&documents, &fields);
        assert!(
            missing.is_empty(),
            "assets/calibrate-flow.js's `{name}` reads {missing:?} off a document this daemon \
             answers, and neither `WriteReport` nor `RestoreReport` carries it"
        );
    }

    // Both directions, driven over text this tree does not carry: the arm that matters is a page
    // reading a field that has been renamed in Rust, and it is the one that has to be seen.
    let renamed = source.replace("report.writes", "report.written");
    assert_ne!(renamed, source, "the seed changed nothing");
    assert_eq!(
        unanswered(
            &documents,
            &wire_fields_read(&body_of(&renamed, "applySentence"))
        ),
        vec!["written".to_owned()],
        "a page reading a field off `WriteReport` that no version of it carries was accepted"
    );
    let dropped = source.replace("outcome.reason", "outcome.excuse");
    assert_ne!(dropped, source, "the seed changed nothing");
    assert_eq!(
        unanswered(
            &documents,
            &wire_fields_read(&body_of(&dropped, "restoreSentence"))
        ),
        vec!["excuse".to_owned()],
        "a page reading a field off `RestoreOutcome` that no version of it carries was accepted"
    );

    // **The outcome vocabulary, both ways.** `outcomeWords` renders one phrase per
    // `RestoreOutcome` tag and has a payload-carrying fallback for a daemon newer than this page
    // (AGENTS rule 6), so a tag it does not name is not a crash — it is a phrase an operator
    // cannot act on. The tags are `serde(tag = "outcome", rename_all = "snake_case")`'s, read out
    // of the document rather than transcribed.
    let vocabulary = body_of(&source, "outcomeWords");
    let mut tags: Vec<String> = restore_report["outcomes"]
        .as_array()
        .expect("outcomes is an array")
        .iter()
        .map(|outcome| {
            outcome["outcome"]
                .as_str()
                .expect("every outcome is tagged")
                .to_owned()
        })
        .collect();
    tags.sort();
    tags.dedup();
    assert_eq!(
        tags.len(),
        4,
        "the four outcomes above are four distinct tags"
    );
    for tag in &tags {
        assert!(
            vocabulary.contains(&format!("case \"{tag}\":")),
            "assets/calibrate-flow.js's `outcomeWords` has no phrase for `{tag}`, which this \
             daemon answers, so an operator would read the fallback instead"
        );
    }
    assert!(
        !vocabulary.contains("case \"restored_exactly\":"),
        "a tag `RestoreOutcome` does not carry was accepted as one it does"
    );
}

/// How many lines of code `source` gives the top-level function `name`.
///
/// **The rule is stated once, here and in the sentence this checks**: non-blank, non-comment
/// lines from the `export` that declares the function to the `}` in the first column that ends
/// it. Coarse on purpose — a JavaScript parser to count a function would be a second opinion
/// about a language this repository does not otherwise read, and what the count is *for* is a
/// budget in a design document, which is a size and not a measurement.
fn code_lines(source: &str, name: &str) -> usize {
    let opening = format!("export async function {name}(");
    let mut counted = 0;
    let mut inside = false;
    for line in source.lines() {
        inside = inside || line.starts_with(opening.as_str());
        if !inside {
            continue;
        }
        let trimmed = line.trim_start();
        let comment =
            trimmed.starts_with("//") || trimmed.starts_with("/*") || trimmed.starts_with('*');
        if !trimmed.is_empty() && !comment {
            counted += 1;
        }
        if line == "}" {
            break;
        }
    }
    counted
}

/// Every comment in `source`, unwrapped into the sentences a reader reads.
///
/// **A sentence in a wrapped comment is a sentence and a line break**, so a search of the raw
/// file for the sentence a reader sees finds nothing at all — and a check that then happens to
/// match somewhere else is worse than one that matches nowhere. That is not hypothetical: the
/// same repair pass found half of `webcam-handler-web`'s file-count reconciler being satisfied by
/// a comment in its own test rather than by the doc it named (note **N153**). Both spellings this
/// module uses are unwrapped — `//` for its header, ` * ` for the JSDoc blocks — because which
/// one a sentence lands in is a decision about where it belongs, not about whether it counts.
fn commented_prose(source: &str) -> String {
    let mut prose = String::with_capacity(source.len());
    for line in source.lines() {
        let trimmed = line.trim_start();
        let Some(text) = trimmed
            .strip_prefix("//")
            .or_else(|| trimmed.strip_prefix("* "))
        else {
            continue;
        };
        prose.push(' ');
        prose.push_str(text.trim());
    }
    prose
}

/// Where `source`'s own header fails to say `name` is `lines` long, exactly once. Empty is green.
fn counts_itself(source: &str, name: &str, lines: usize) -> Vec<String> {
    let sentence = format!("`{name}` is {lines} lines of code");
    let said = commented_prose(source).matches(sentence.as_str()).count();
    if said == 1 {
        return Vec::new();
    }
    vec![format!(
        "assets/rpc.js says `{sentence}` {said} time(s) and should say it once; the function is \
         {lines} line(s) of code today"
    )]
}

#[test]
fn the_client_helper_states_its_own_size_and_the_size_it_states_is_the_one_it_has() {
    // **A count in a comment is worth what the thing that checks it is worth** (note **N158**).
    // This module's header opens by measuring itself against design §2.7's "~50-line" budget, and
    // for two sub-milestones the number in that sentence was fifty-three while the function was
    // seventy-one lines and then a hundred and twelve: it grew by more than half in the same
    // batch that repaired the identical defect class in `webcam-handler-web`'s header (note
    // **N153**), in the file being edited, and nothing could go red on it.
    //
    // The overshoot itself is not this test's business — a design number is the owner's and N158
    // is where it is argued. What is this test's business is that the sentence stays true, so
    // that widening the budget is something somebody decides rather than something that happens.
    //
    // `web::get` rather than the file on disk, for the reason the bounds above are read that way:
    // what a reader of the shipped client is told is what the shipped client carries.
    let module = web::get("rpc.js").expect("the client's JSON-RPC helper");
    let source = String::from_utf8(module.bytes().to_vec()).expect("the module is UTF-8");

    let lines = code_lines(&source, "connect");
    // Not vacuous: a rule that found nothing would make `0 lines of code` a sentence somebody
    // could write in the header and satisfy this with.
    assert!(lines > 20, "`connect` counted as {lines} line(s) of code");

    let drift = counts_itself(&source, "connect", lines);
    assert!(drift.is_empty(), "{drift:?}");
    // Both directions, driven with sizes the function does not have — the half that proves a
    // reconciler can go red (AGENTS rule 2), and the half whose absence is the finding.
    for wrong in [lines - 1, lines + 1] {
        assert!(
            !counts_itself(&source, "connect", wrong).is_empty(),
            "the header was accepted stating a size (`{wrong}`) the function does not have"
        );
    }
}

/// What `source` declares the string constant `name` as, or `None` when it does not declare
/// exactly one.
///
/// [`declares`]'s shape for a name rather than a number, and `None` covers both ways the
/// question has no answer: not declared at all, and declared twice. A page with two spellings of
/// the method it pings with is a page whose liveness depends on which one runs.
fn declared_string(source: &str, name: &str) -> Option<String> {
    let declaration = format!("const {name} = \"");
    let mut after = source.split(declaration.as_str());
    let _before = after.next()?;
    let value = after.next()?;
    if after.next().is_some() {
        return None;
    }
    value.split('"').next().map(str::to_owned)
}

#[tokio::test(flavor = "multi_thread")]
async fn the_heartbeat_the_page_sends_is_answered_on_the_socket_the_page_sends_it_on() {
    // **The dependency claim `assets/rpc.js` bets a connection on, measured here rather than
    // read somewhere else** (docs/11 **L38**, note **N157**). The heartbeat treats *silence* as
    // a dead socket and closes it, and the reason that is evidence about the connection rather
    // than about a busy camera is one sentence in that module's header: `wch_ping` is not a
    // registered method, so jsonrpsee answers `-32601` without a handler running. Until this
    // test the sentence rested on nothing this repository had run: the `-32601` claim is
    // asserted for the **Unix** transport (`tests/uds.rs`) and once more for a foreign name over
    // HTTP (`tests/http.rs`), never over the TCP WebSocket the page actually opens and never for
    // the name the page actually sends.
    //
    // **If it were false the cost would be one-sided and silent.** Every idle tab would ask,
    // hear nothing, and close its own socket a heartbeat later — the banner reading "the
    // connection closed" on a daemon that is perfectly healthy — while the browser rung stayed
    // green, because the claim it drives severs the link first and therefore only ever sees the
    // failing path. That is rubric **A9**'s second half, a claim about a dependency nobody had
    // read, and it is the class this review found three times.
    //
    // The name is read out of the served module rather than typed here, for the reason the two
    // bounds above are: what has to be answered is what the page sends.
    let module = web::get("rpc.js").expect("the client's JSON-RPC helper");
    let source = String::from_utf8(module.bytes().to_vec()).expect("the module is UTF-8");
    let method =
        declared_string(&source, "HEARTBEAT_METHOD").expect("the page declares one ping method");

    let client = Client::start().await;

    // **Why `-32601` can be asserted rather than merely allowed.** The page treats *any* answer
    // as proof of life, so a build that registered this name would break nothing — but a test
    // that accepted either answer would be a test with no failing direction. So the surface's
    // own vocabulary is asked first: this name is not in it, which is what makes the refusal
    // below the only correct answer and makes registering it a red line with an explanation
    // rather than a silent change of meaning (N157's *Retires when*).
    let methods = daemon::server::mount(client.wchd.clone()).expect("the T5 surface mounts once");
    assert!(
        !methods.method_names().any(|name| name == method),
        "`{method}` is a registered method now; the page's heartbeat is a call that reaches a \
         handler, so N157's reasoning has to move with it"
    );

    let mut page = client.page_socket().await;

    // The positive control first, on the same socket: a registered method answers a result. It
    // is what stops "the refusal came back" from being satisfied by a fixture whose socket
    // carries nothing at all — the failure this whole test is about looks exactly like that.
    let listing = page.call("wch_list", json!({})).await;
    assert!(
        result(&listing, "wch_list").get("cameras").is_some(),
        "the page's socket did not carry an ordinary call: {listing}"
    );

    let answered = page.call(&method, json!({})).await;
    let error = refusal(&answered, &method);
    assert_eq!(
        error["code"],
        json!(-32601),
        "the heartbeat was answered with something other than jsonrpsee's method-not-found: \
         {answered}"
    );
    // …and it carries no D13 payload, which is the other half of what `rpc.js` says about it:
    // `RpcError.kind` is `null` for a protocol refusal, because a name that is not registered is
    // not a device error and inventing one would be the collapse the registry exists to prevent.
    assert_eq!(
        error["data"]["kind"],
        Value::Null,
        "a protocol refusal arrived carrying a D13 discriminant: {answered}"
    );

    client.stop().await;
}

// ------------------------------------------------- the DTO the control panel is made from

#[tokio::test(flavor = "multi_thread")]
async fn the_control_report_the_panel_is_generated_from_carries_every_edge_it_must_represent() {
    // **The four device behaviours `assets/controls.js` is written around, read off the page's
    // own socket as JSON.** Each is a measured finding with a PF number, each is in this
    // build's fixture because of that finding, and each is a different way a naive panel is
    // wrong — so each is asserted separately rather than as "the report parsed".
    //
    // What is **not** asserted: that any of it renders. That is P5d's, in Chromium (this
    // file's header).
    let client = Client::start().await;
    let mut page = client.page_socket().await;

    // The camera list's own fields first, because they are the page's first call and the
    // panel has nothing to be about without one.
    let listing = result(&page.call("wch_list", json!({})).await, "wch_list");
    let camera = listing["cameras"]
        .as_array()
        .and_then(|cameras| cameras.first())
        .expect("the fixture replays one camera")
        .clone();
    for field in ["id", "card", "driver", "nodes"] {
        assert!(
            camera.get(field).is_some(),
            "the camera list renders {field} and the daemon did not send it: {camera}"
        );
    }
    // **`hints` is absent rather than empty, and the page has to code for that.** D1's
    // diagnosis of what is *not* in a listing is `#[serde(skip_serializing_if =
    // "Vec::is_empty")]`, so a healthy machine answers a document with no `hints` key at all —
    // which is the opposite of the rule `SessionStatus::log` states for itself one method
    // along ("always emitted, empty or not: this is an *answer*, and a consumer counting the
    // history should meet a list with nothing in it rather than a missing key"), and that
    // asymmetry is asserted here rather than discovered by a client that iterated `undefined`.
    assert!(
        listing.get("hints").is_none(),
        "a listing with nothing to diagnose carried a hints key: {listing}"
    );

    let report = result(
        &page
            .call("wch_controls", json!({ "camera": client.camera.as_str() }))
            .await,
        "wch_controls",
    );
    let controls = report["controls"].as_array().expect("a control array");
    assert!(!controls.is_empty(), "{report}");

    // (1) **A sparse menu \[PF:2\].** The map crosses JSON as an *object*, so its keys arrive
    // as strings — which is the first thing a hand-written client has to get right, because
    // `"10" < "3"` lexically and an option built from a position rather than a key writes an
    // index the device never declared. The hole is the assertion: a menu whose declared
    // indices are `0..n` would satisfy "there is a menu" and would prove nothing.
    let sparse = controls
        .iter()
        .filter_map(|control| control["menu"].as_object())
        .find(|menu| {
            let mut indices: Vec<u64> = menu
                .keys()
                .map(|key| key.parse::<u64>().expect("a menu index is a number"))
                .collect();
            indices.sort_unstable();
            indices.windows(2).any(|pair| pair[1] - pair[0] > 1)
        })
        .expect("the fixture carries a menu with a hole in it [PF:2]");
    for item in sparse.values() {
        assert!(
            item["kind"] == json!("name") || item["kind"] == json!("value"),
            "a menu item the page has no arm for: {item}"
        );
    }

    // (2) **A control an automation partner owns right now \[PF:3\]**, and the pair that names
    // the owner. Both halves, because the panel shows one *because of* the other: an INACTIVE
    // control with nothing to name would leave the page saying "something owns this".
    let inactive = controls
        .iter()
        .find(|control| {
            control["flags"]["known"]
                .as_array()
                .is_some_and(|flags| flags.contains(&json!("inactive")))
        })
        .expect("the fixture carries a live INACTIVE coupling [PF:3]");
    let owner = report["pairs"]
        .as_array()
        .expect("a pair list")
        .iter()
        .find(|pair| pair["manual"] == inactive["slug"])
        .expect("the INACTIVE control's automation partner is nameable");
    assert!(
        owner["off"]["kind"] == json!("value") || owner["off"]["kind"] == json!("menu_item_named"),
        "a way of switching automation off the page has no arm for: {owner}"
    );

    // (3) **A current value outside its own declared range \[PF:4\]**, reported and not
    // corrected — the case a slider element cannot represent at all, which is why the panel
    // puts the number beside it.
    let out_of_range = controls
        .iter()
        .find(|control| {
            let value = control["current"]["value"].as_i64();
            let (min, max) = (
                control["range"]["min"].as_i64(),
                control["range"]["max"].as_i64(),
            );
            match (value, min, max) {
                (Some(value), Some(min), Some(max)) => value < min || value > max,
                _ => false,
            }
        })
        .expect("the fixture carries a current value outside its declared range [PF:4]");
    assert_eq!(
        out_of_range["current"]["kind"],
        json!("int"),
        "{out_of_range}"
    );

    // (4) **A control type this build cannot name \[PF:1\]**, with its raw discriminant and
    // its payload. AGENTS rule 6 is the reason it is on the wire at all, and the reason the
    // panel's type switch has a fallback arm that renders something.
    let unnameable = controls
        .iter()
        .find(|control| control["type"]["kind"] == json!("unknown"))
        .expect("the fixture carries a control type this build does not name [PF:1]");
    assert!(
        unnameable["type"]["raw"].as_u64().is_some(),
        "an unknown type with no discriminant to show: {unnameable}"
    );
    assert_eq!(
        unnameable["current"]["kind"],
        json!("bytes"),
        "an opaque control whose value is not opaque: {unnameable}"
    );

    // And the flags every control carries: the decoded set the page renders as chips, and the
    // raw word beside it, so a bit this build cannot name is data rather than a silence.
    for control in controls {
        assert!(control["flags"]["known"].is_array(), "{control}");
        assert!(control["flags"]["raw"].as_u64().is_some(), "{control}");
        assert!(
            control["flags"]["unknown_bits"].as_u64().is_some(),
            "{control}"
        );
    }

    client.stop().await;
}

// ------------------------------------------------------------ requested is not applied

#[tokio::test(flavor = "multi_thread")]
async fn a_write_the_driver_clamped_answers_with_both_numbers_and_names_the_clamp() {
    // **The fact a slider snapping back is a rendering of.** A write past the maximum is
    // accepted by the driver, which silently takes the maximum and reports success \[PF:6\] —
    // so what the page shows afterwards is `applied`, and what makes that honest rather than a
    // widget twitching is that `requested` came back too (D3, E4, AGENTS rule 5).
    //
    // Driven over the page's own socket, with the request `assets/app.js` builds: a `writes`
    // array of one named pair and the guarded flag the checkbox above the panel sets.
    let client = Client::start().await;
    let mut page = client.page_socket().await;

    let report = result(
        &page
            .call(
                "wch_set",
                json!({
                    "camera": client.camera.as_str(),
                    "writes": [{ "control": "brightness", "value": { "kind": "int", "value": 5_000 } }],
                    "guarded": false,
                }),
            )
            .await,
        "wch_set",
    );
    let applied = report["writes"]
        .as_array()
        .and_then(|writes| writes.first())
        .expect("a write report with the write in it")
        .clone();

    assert_eq!(
        applied["requested"],
        json!({ "kind": "int", "value": 5_000 })
    );
    assert_ne!(
        applied["applied"], applied["requested"],
        "the device took a value it declares out of range: {applied}"
    );
    // The warning, and its shape: the panel renders `clamped` as a sentence naming the range,
    // which it can only do because the range rides on the warning rather than being looked up
    // again against a descriptor that may have moved.
    let warning = applied["warnings"]
        .as_array()
        .and_then(|warnings| warnings.first())
        .expect("a clamp with no warning is a clamp nobody can see");
    assert_eq!(warning["kind"], json!("clamped"), "{warning}");
    assert!(warning["range"]["max"].as_i64().is_some(), "{warning}");
    assert_eq!(warning["applied"], applied["applied"]["value"], "{warning}");

    // …and the other direction, so the assertions above are about a clamp rather than about a
    // daemon that warns on everything: a write the device takes exactly carries no warnings at
    // all, and the panel then shows one number instead of two.
    let exact = result(
        &page
            .call(
                "wch_set",
                json!({
                    "camera": client.camera.as_str(),
                    "writes": [{ "control": "brightness", "value": { "kind": "int", "value": 64 } }],
                    "guarded": false,
                }),
            )
            .await,
        "wch_set",
    );
    let taken = exact["writes"][0].clone();
    assert_eq!(taken["applied"], taken["requested"], "{taken}");
    assert!(
        taken["warnings"]
            .as_array()
            .is_none_or(|warnings| warnings.is_empty()),
        "an exact write warned about something: {taken}"
    );

    client.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn the_guarded_checkbox_is_two_operations_and_the_panel_can_tell_them_apart() {
    // **D3's guard, as the two different operations the panel's one checkbox selects
    // between** — and the surprise is which. `wch_set`'s `# Errors` names
    // `ControlInactive` "naming the automation to disable", which reads as the refusal an
    // *unguarded* write to an INACTIVE control meets. It is not: `engine::pairing`'s planner
    // takes the unguarded path before it looks at `INACTIVE` at all, so an unguarded write to
    // a control an automation partner owns is **performed as asked** and the partner keeps
    // owning it. `ControlInactive` is the *guarded* path's refusal, for the case where there
    // is no partner to switch off \[PF:3\] or none whose "off" this build can resolve.
    //
    // That matters to a page more than it looks: the honest thing for the panel to say about
    // an unguarded write to an INACTIVE control is not "refused", it is "written, to a control
    // something else is driving". Both directions are asserted in one test because neither is
    // meaningful alone — a write that succeeds proves nothing about the flag, and a flag that
    // clears proves nothing about what the other setting does.
    let client = Client::start().await;
    let mut page = client.page_socket().await;
    let inactive = "exposure_time_absolute";

    let unguarded = result(
        &page
            .call(
                "wch_set",
                json!({
                    "camera": client.camera.as_str(),
                    "writes": [{ "control": inactive, "value": { "kind": "int", "value": 200 } }],
                    "guarded": false,
                }),
            )
            .await,
        "an unguarded write to an INACTIVE control",
    );
    assert!(
        unguarded.get("disabled_automation").is_none(),
        "an unguarded write switched automation off: {unguarded}"
    );
    let still_owned = result(
        &page
            .call("wch_controls", json!({ "camera": client.camera.as_str() }))
            .await,
        "wch_controls",
    );
    assert!(
        inactive_flag(&still_owned, inactive),
        "an unguarded write released the automation that owns the control: {still_owned}"
    );

    let planned = result(
        &page
            .call(
                "wch_set",
                json!({
                    "camera": client.camera.as_str(),
                    "writes": [{ "control": inactive, "value": { "kind": "int", "value": 200 } }],
                    "guarded": true,
                }),
            )
            .await,
        "a guarded write to an INACTIVE control",
    );
    // The switched-off automation is *named*, which is the whole reason the panel shows the
    // whole report rather than the row it asked for: an operator who touched one control and
    // found another had changed would otherwise have no way to know why.
    assert_eq!(
        planned["disabled_automation"],
        json!(["auto_exposure"]),
        "a guarded write that did not say what it switched off: {planned}"
    );

    // …and the panel's repaint sees the coupling move \[PF:3\], which is why a write is
    // followed by a fresh `wch_controls` rather than by patching one widget: the flag that
    // changed is on a control the operator never touched.
    let after = result(
        &page
            .call("wch_controls", json!({ "camera": client.camera.as_str() }))
            .await,
        "wch_controls",
    );
    assert!(
        !inactive_flag(&after, inactive),
        "the control is still owned by its automation after a guarded write: {after}"
    );

    client.stop().await;
}

/// Whether one control in a `ControlReport` is currently owned by an automation partner.
fn inactive_flag(report: &Value, slug: &str) -> bool {
    report["controls"]
        .as_array()
        .expect("a control array")
        .iter()
        .find(|control| control["slug"] == json!(slug))
        .unwrap_or_else(|| panic!("this camera has no control called {slug}"))["flags"]["known"]
        .as_array()
        .is_some_and(|known| known.contains(&json!("inactive")))
}

// ------------------------------------------------------------------ the refusal vocabulary

#[tokio::test(flavor = "multi_thread")]
async fn a_refusal_carries_the_discriminant_the_page_branches_on() {
    // **AGENTS rule 7, at the one place a browser can act on it.** `assets/rpc.js` reads
    // `error.data.kind` and every view in that directory renders *that* rather than the
    // message — because `busy`, `device_gone` and `permission_denied` are three different
    // things to do next and "unavailable" is none of them (design E3).
    //
    // Three refusals with three different names, because a daemon that had degraded into one
    // blanket refusal would satisfy "it refused". The numeric code is checked beside each one
    // through `api::rpc_code` rather than against a literal, so a renumbering is a diff in the
    // registry rather than a silent disagreement between a page and a daemon — and the *name*
    // is what the page uses, precisely because it survives one.
    let client = Client::start().await;
    let mut page = client.page_socket().await;

    let cases = [
        (
            "wch_info",
            json!({ "camera": "cam:nothing-answers-to-this" }),
            ErrorKind::CameraUnknown,
            "camera_unknown",
        ),
        (
            "wch_get",
            json!({ "camera": client.camera.as_str(), "control": "brightnes" }),
            ErrorKind::ControlUnknown,
            "control_unknown",
        ),
        (
            "wch_set",
            json!({
                "camera": client.camera.as_str(),
                "writes": [{ "control": "privacy", "value": { "kind": "int", "value": 1 } }],
                "guarded": true,
            }),
            ErrorKind::ControlReadOnly,
            "control_read_only",
        ),
    ];

    let mut names = std::collections::BTreeSet::new();
    for (method, params, kind, spelling) in cases {
        let answer = page.call(method, params).await;
        let error = refusal(&answer, method);
        assert_eq!(error["code"], json!(rpc_code(kind)), "{method}: {error}");
        assert_eq!(
            error["data"]["kind"],
            json!(spelling),
            "{method} refused without the name the page reads"
        );
        assert!(
            error["message"]
                .as_str()
                .is_some_and(|text| !text.is_empty()),
            "{method}: a refusal with nothing to show a person: {error}"
        );
        names.insert(spelling);
    }
    assert_eq!(names.len(), 3, "{names:?}");

    client.stop().await;
}

// --------------------------------------------------- the photo button and the live preview

#[tokio::test(flavor = "multi_thread")]
async fn a_photo_over_the_pages_socket_leaves_the_pages_preview_streaming() {
    // **The composition P5c's photo button depends on, driven the way the page drives it.**
    // The owner ruled (2026-08-12, note **N83**) that a photo suspends and resumes the preview
    // inside the actor that owns the device, and rejected the two client-side designs — a
    // "stop the preview" affordance, and a page that tears its own `<img>` down and re-opens
    // it. `assets/photo.js` therefore holds no reference to the preview element at all.
    //
    // `preview.rs` asserts the *engine's* half of that, with the photo taken in process over a
    // `Methods` value. What is asserted here is the arrangement the page actually has: the
    // preview is an `<img>` request on the TCP listener, the photo is a `wch_photo` on the
    // WebSocket beside it, and the first survives the second — which no test in this workspace
    // could see before, because no other suite has both of the page's transports open at once.
    let client = Client::start().await;
    let mut page = client.page_socket().await;
    let mut interruptions = client.wchd.watch_preview_interruptions();
    assert_eq!(
        *interruptions.borrow(),
        0,
        "a fresh daemon has paused a preview"
    );

    let mut frames = Frames::open(client.serving.bound(), &client.preview_target()).await;
    assert_eq!(frames.status(), 200, "{head}", head = frames.head);
    assert!(
        frames
            .header("Content-Type")
            .is_some_and(|value| value.starts_with("multipart/x-mixed-replace")),
        "the preview is not the content type an <img> paints successive frames of: {head}",
        head = frames.head
    );
    let before = frames.until(2).await;

    // The button, as the page presses it: the sink is `return_bytes`, and every other field of
    // the request is the daemon's default — which is what `assets/photo.js` sends and why it
    // sends it.
    let answer = page
        .call(
            "wch_photo",
            json!({
                "camera": client.camera.as_str(),
                "request": { "sink": { "kind": "return_bytes", "format": "jpeg" } },
            }),
        )
        .await;
    let taken = result(&answer, "wch_photo");
    // The report the page renders, and nothing that would print a frame: `delivery` says how
    // many bytes are on their way and `rendering` says which of D6's three paths produced them
    // — a verbatim answer is the camera's own bitstream (E6), which is what makes a photo the
    // product rather than our idea of it.
    assert!(
        taken["report"]["delivery"]["byte_count"]
            .as_u64()
            .is_some_and(|count| count > 0)
    );
    assert!(taken["report"]["rendering"]["kind"].as_str().is_some());
    assert!(
        taken["bytes"].is_string(),
        "a return_bytes sink answered no bytes"
    );

    // The pause is counted rather than inferred: `Fanout::skipped` must **not** move for it —
    // folding a suspension into a drop count would tell an operator their socket was slow when
    // their camera was busy being a camera (note N83) — so the daemon keeps a count of its own,
    // and this is it.
    interruptions
        .wait_for(|paused| *paused == 1)
        .await
        .expect("the daemon publishes its interruption count");

    // …and the page's own `<img>` request is still delivering parts, on the same connection it
    // opened before the photo. A client that had to re-open it would fail here with a stream
    // that ended.
    let after = frames.until(before + 2).await;
    assert!(
        after > before,
        "the preview stopped delivering after a photo: {before} parts before, {after} after"
    );

    client.stop().await;
}

// ------------------------------------------------------------------- the calibration view

#[tokio::test(flavor = "multi_thread")]
async fn the_page_can_watch_a_sweep_it_did_not_start() {
    // **What `wch_subscribe_calibration`'s parameterless shape is for.** The stream is per
    // *client* and every event carries its session id (`crates/api`'s `WchEvents`), so the
    // calibration view can paint a sweep an agent started on the Unix socket — which is the
    // arrangement this project actually has, since `webcam-handler-client` and
    // `webcam-handler-cli` are where sweeps are driven from and this page deliberately starts
    // none (`assets/calibration.js` says why).
    //
    // Two connections, and the sweep is put on the second with `Ws::write` and collected much
    // later with `Ws::answer`, because that is the only shape in which the events and the
    // answer are in flight together — a helper that waited for the answer first would be
    // testing a request/response pipe and calling it a subscription.
    let client = Client::start().await;
    let mut page = client.page_socket().await;
    let mut agent = client.other_client().await;

    let subscription = result(
        &page.call("wch_subscribe_calibration", json!({})).await,
        "wch_subscribe_calibration",
    );

    let opened: Session = serde_json::from_value(result(
        &agent
            .call(
                "wch_calibrate_start",
                json!({
                    "camera": client.camera.as_str(),
                    "task": SWEEP_TASK,
                    "goal": "a legible frame",
                    "criteria": ["sharp"],
                }),
            )
            .await,
        "wch_calibrate_start",
    ))
    .expect("a session");
    let which = SessionRef::Id { id: opened.id };
    result(
        &agent
            .call(
                "wch_calibrate_plan",
                json!({
                    "camera": client.camera.as_str(),
                    "session": &which,
                    "controls": ["brightness"],
                    "order": false,
                }),
            )
            .await,
        "wch_calibrate_plan",
    );
    agent
        .write(
            &json!({
                "jsonrpc": "2.0",
                "id": SWEEP_REQUEST_ID,
                "method": "wch_calibrate_sweep",
                "params": {
                    "camera": client.camera.as_str(),
                    "session": &which,
                    "request": SweepRequest::new(
                        schema::control::ControlSlug::parse("brightness").expect("a literal slug"),
                        SweepSpec::Explicit { values: vec![0, 255] },
                    ),
                },
            })
            .to_string(),
        )
        .await;

    // The events, as the view reads them: a `progress` discriminant it has an arm for, the
    // session id it prefixes every line with, and — on every in-flight variant — the
    // `index`/`total` pair that lets a subscriber which connected mid-sweep paint a truthful
    // bar from the first event it sees rather than by counting what it has received.
    let mut kinds = Vec::new();
    loop {
        let params = page.notification().await;
        assert_eq!(params["subscription"], subscription, "{params}");
        let event = params["result"].clone();
        assert_eq!(
            event["session"],
            json!(opened.id),
            "an event for a session this page did not open: {event}"
        );
        let progress = event["progress"]
            .as_str()
            .expect("every event names what happened")
            .to_owned();
        // **Required per discriminant, not read if present** (the G6 review's L24, which
        // needed no note of its own — the repair is this comment). This used to be `if let Some(total) = …` around `if let Some(index) =
        // …`, under the sentence above claiming every in-flight variant carries the pair —
        // so the one failure the sentence is about, a variant that stopped carrying them,
        // skipped both assertions and the test reported green. Which variant carries what
        // is `schema::progress::CalibrationProgress`'s decision and it is asserted in both
        // directions: a `sweep_finished` that grew an `index` is a change to what the view
        // must paint, and it should be somebody's decision rather than a surprise.
        let (wants_total, wants_index) = match progress.as_str() {
            // The bar's first event: how big the sweep is, before any sample exists.
            "sweep_started" => (true, false),
            // The in-flight pair, which is what lets a subscriber that connected mid-sweep
            // paint a truthful bar from the first event it sees.
            "value_set" | "sample_taken" => (true, true),
            // The endings. `sweep_finished` carries `samples`, `sweep_interrupted` carries
            // `taken` alongside the plan's `total` — neither is an index into a bar.
            "sweep_finished" => (false, false),
            "sweep_interrupted" => (true, false),
            other => panic!("the view has no arm for a `{other}` event: {event}"),
        };
        let total = event["total"].as_u64();
        let index = event["index"].as_u64();
        assert_eq!(
            total.is_some(),
            wants_total,
            "a `{progress}` event and its `total`: {event}"
        );
        assert_eq!(
            index.is_some(),
            wants_index,
            "a `{progress}` event and its `index`: {event}"
        );
        if let Some(total) = total {
            assert!(total > 0, "{event}");
            if let Some(index) = index {
                assert!(index <= total, "a bar past its own end: {event}");
            }
        }
        let terminal = progress == "sweep_finished" || progress == "sweep_interrupted";
        kinds.push(progress);
        if terminal {
            break;
        }
    }
    assert_eq!(
        kinds.first().map(String::as_str),
        Some("sweep_started"),
        "the view's first line was not the start of the sweep: {kinds:?}"
    );
    assert_eq!(
        kinds.last().map(String::as_str),
        Some("sweep_finished"),
        "{kinds:?}"
    );
    assert!(
        kinds.len() > 2,
        "a sweep that emitted only its ends: {kinds:?}"
    );

    // The call that produced them, answered on the connection it was written to.
    let answered = agent.answer().await;
    assert_eq!(answered["id"], json!(SWEEP_REQUEST_ID), "{answered}");
    result(&answered, "wch_calibrate_sweep");

    // …and the two documents the view reads afterwards, over the page's own socket. The
    // listing is built from the directory tree alone — nothing is parsed out of a session
    // document (D9) — and the status is where the `{requested, applied}` pairs and the
    // per-control state live.
    let listing = result(
        &page
            .call(
                "wch_calibrate_list",
                json!({ "camera": client.camera.as_str() }),
            )
            .await,
        "wch_calibrate_list",
    );
    let sessions = listing["sessions"].as_array().expect("a session list");
    assert_eq!(sessions.len(), 1, "{listing}");
    for field in ["id", "task_slug", "path"] {
        assert!(sessions[0].get(field).is_some(), "{listing}");
    }

    let status = result(
        &page
            .call(
                "wch_calibrate_status",
                json!({ "camera": client.camera.as_str(), "session": &which }),
            )
            .await,
        "wch_calibrate_status",
    );
    assert!(status["log"].is_array(), "{status}");
    let swept = status["session"]["controls"]["brightness"].clone();
    assert!(
        swept["status"]["status"].as_str().is_some(),
        "the view switches on this field and it is spelled `status`, not `kind`: {swept}"
    );
    let samples = swept["samples"].as_array().expect("a sample list");
    assert_eq!(samples.len(), 2, "one sample per swept value: {swept}");
    for sample in samples {
        // D3 applies inside a sweep too \[PF:6\]: a sample labelled with a value the camera
        // never held would poison every comparison built on it, so the view shows both and the
        // wire has to carry both.
        assert!(sample["requested"].as_i64().is_some(), "{sample}");
        assert!(sample["applied"].as_i64().is_some(), "{sample}");
    }
    // The one thing a browser cannot do with this document: a sample's `photo` is a path
    // relative to a session directory on the **daemon's** filesystem (D9), so the view renders
    // it as text rather than as a link a browser would refuse to follow. `crates/api`'s header
    // says the same thing from the other side — "a browser client (P5c) can open neither, and
    // reads the documents rather than the files".
    assert!(
        samples[0]["photo"]
            .as_str()
            .is_some_and(|path| !path.is_empty()),
        "{samples:?}"
    );

    client.stop().await;
}

/// The task this suite's one session is opened under.
const SWEEP_TASK: &str = "p5c sweep watched from the page";

/// The id the sweep request carries, out of the way of `Ws::call`'s own counter.
const SWEEP_REQUEST_ID: u32 = 9_101;

#[tokio::test(flavor = "multi_thread")]
async fn the_camera_lists_subscription_ends_with_a_reason_rather_than_a_closed_socket() {
    // **The other subscription the page holds, and it ends differently on purpose.**
    // `assets/app.js` keeps `wch_subscribe_events` open so the camera list stays live, and
    // that stream is **closed rather than resynced** when a subscriber falls behind: a
    // `HotplugEvent` is a delta, a gap makes a consumer's picture of the node tree wrong in a
    // way it cannot detect, and the vocabulary has no variant meaning "you missed some"
    // (`crates/api`'s `WchEvents`). So the page's documented answer is to re-subscribe and
    // re-enumerate, and the whole of that answer depends on the ending arriving as a
    // *payload* — a socket that simply closed is indistinguishable from the daemon crashing.
    //
    // `web_rpc.rs` asserts the same mechanism for the *calibration* stream, and the two are
    // not one claim: they are different subscriptions with different loss semantics, and the
    // page does different things with their endings.
    let client = Client::start().await;
    let mut page = client.page_socket().await;
    let mut subscribers = client.wchd.watch_subscribers();

    let subscription = result(
        &page.call("wch_subscribe_events", json!({})).await,
        "wch_subscribe_events",
    );
    subscribers
        .wait_for(|live| *live == 1)
        .await
        .expect("the daemon publishes its subscriber count");

    // …and the list is still answerable on the same connection, which is what makes the
    // subscription an addition to the page rather than a mode it entered.
    assert!(
        result(&page.call("wch_list", json!({})).await, "wch_list")["cameras"]
            .as_array()
            .is_some_and(|cameras| cameras.len() == 1)
    );

    client.shutdown.cancel();
    assert_eq!(
        page.ending(&subscription).await,
        json!({ "ended": daemon::events::SHUTTING_DOWN }),
        "the page's device-change stream ended without telling it why"
    );

    client.stop().await;
}

// ------------------------------------------------------------------------- the preview reader

/// The preview response, read far enough to count its parts.
///
/// Hand-written for `support/tcp.rs`'s reason and one more: that client reads to end-of-file
/// and this response has no end until somebody causes one. It is deliberately **not**
/// `preview.rs`'s reader, which decodes the chunked layer and parses each part — nothing here
/// asks what is *inside* a part, only whether more of them keep arriving, and the multipart
/// delimiter is findable in the raw stream because a chunk-length line cannot contain it.
/// A reader that occasionally misses a delimiter split across two chunks simply reads one
/// frame longer, which is why this counts "at least N" rather than exactly N.
struct Frames {
    socket: TcpStream,
    /// The response head, split at the blank line that ends it.
    head: String,
    /// Everything after the head, undecoded.
    body: Vec<u8>,
}

impl Frames {
    /// Connect, send one `GET`, and read as far as the end of the response head.
    async fn open(bound: std::net::SocketAddr, target: &str) -> Frames {
        let mut socket = TcpStream::connect(bound).await.expect("the listener is up");
        let request = format!(
            "GET {target} HTTP/1.1\r\n\
             Host: {bound}\r\n\
             \r\n"
        );
        socket
            .write_all(request.as_bytes())
            .await
            .expect("the request is written");

        let mut raw: Vec<u8> = Vec::new();
        loop {
            if let Some(at) = raw.windows(4).position(|window| window == b"\r\n\r\n") {
                let (head, body) = raw.split_at(at);
                return Frames {
                    socket,
                    head: String::from_utf8_lossy(head).into_owned(),
                    body: body.get(4..).unwrap_or_default().to_vec(),
                };
            }
            let mut chunk = [0_u8; 4_096];
            let read = socket.read(&mut chunk).await.expect("the daemon answers");
            assert_ne!(read, 0, "the daemon closed before answering: {raw:?}");
            raw.extend_from_slice(chunk.get(..read).unwrap_or_default());
        }
    }

    /// The status code, as a number.
    fn status(&self) -> u16 {
        self.head
            .lines()
            .next()
            .and_then(|line| line.split_whitespace().nth(1))
            .and_then(|code| code.parse().ok())
            .unwrap_or_else(|| panic!("no status code in {head:?}", head = self.head))
    }

    /// One header's value, matched case-insensitively the way HTTP defines field names.
    fn header(&self, name: &str) -> Option<&str> {
        self.head.lines().skip(1).find_map(|line| {
            let (field, value) = line.split_once(':')?;
            field
                .trim()
                .eq_ignore_ascii_case(name)
                .then(|| value.trim())
        })
    }

    /// Read until at least `wanted` multipart delimiters have arrived, and answer how many.
    ///
    /// Ends when the daemon writes; a preview that stopped producing frames is a nextest
    /// `TIMEOUT` with a test's name on it rather than a hang.
    async fn until(&mut self, wanted: usize) -> usize {
        loop {
            let seen = self.delimiters();
            if seen >= wanted {
                return seen;
            }
            let mut chunk = [0_u8; 8_192];
            let read = self
                .socket
                .read(&mut chunk)
                .await
                .expect("the preview stream is readable");
            assert_ne!(read, 0, "the preview ended after {seen} of {wanted} parts");
            self.body
                .extend_from_slice(chunk.get(..read).unwrap_or_default());
        }
    }

    /// How many multipart delimiters are in what has been read so far.
    fn delimiters(&self) -> usize {
        let delimiter = format!("--{boundary}", boundary = http::preview::BOUNDARY);
        self.body
            .windows(delimiter.len())
            .filter(|window| *window == delimiter.as_bytes())
            .count()
    }
}
