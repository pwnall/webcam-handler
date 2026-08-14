//! What the shipped web client asks this daemon for, asked over the transports it asks on
//! (design **D10**, **D11**, §2.7; docs/7 P5c).
//!
//! `http.rs` and `web_rpc.rs` are this suite's neighbours and the three divide the listener
//! between them. That file's subject is the **credential and D11's matrix**; that one's is
//! **what the wire surface carries over TCP** — the same registration, the subscriptions, the
//! bound, the stop. This one's subject is the **client**: `crates/web/assets/` is ten files of
//! hand-written HTML, CSS and ES modules, and every claim below is one of them depending on
//! something this daemon does.
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
    /// and `credential.js` deliberately sends the token to nothing but the two camera-bearing
    /// routes.
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
        if let Some(total) = event["total"].as_u64() {
            assert!(total > 0, "{event}");
            if let Some(index) = event["index"].as_u64() {
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
