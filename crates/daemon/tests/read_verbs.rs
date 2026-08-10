//! The six read verbs, over both transports the plan names, driven by the generated client.
//!
//! docs/7 P4b's proof obligation, in one file: "read-verb integration tests over in-memory
//! and UDS transports". The two transports are design §2.9's seam and its double — a real
//! `AF_UNIX` socket, and jsonrpsee's in-memory dispatch — and the point of having both is
//! that the *only* difference between them is the pipe: one [`Wchd`] answers both, so an
//! answer that differs differs because of the transport and nothing else.
//!
//! ## Written once, run twice
//!
//! [`read_verbs`] and [`refusals`] take an `impl WchRpcClient`, which is the **generated
//! client** from `webcam-handler-api` — the same code `wchc` will use at P4f, so the
//! request the server parses is the one a real client sends rather than a JSON literal a
//! test author guessed at. Both are called once per transport and their answers compared,
//! so a divergence is one failed `assert_eq!` naming the field rather than two suites that
//! drifted.
//!
//! ## What "equal to the engine" means here, and what it does not
//!
//! Deliverable D of this sub-milestone is that a daemon's answer is the engine's answer.
//! P4f's parity gate makes that byte-exact between `wch` and `wchc`; what is assertable
//! today is the half beneath it — that `wch_get` *is* `engine::pairing::describe`, that
//! `wch_calibrate_status` *is* `engine::lifecycle::status`, and so on for each verb, over
//! the same backend and the same state directory. `crates/cli`'s executor calls those same
//! functions, so the two surfaces agree by construction rather than by comparison; a test
//! that re-assembled the answers a third time would be checking its own arithmetic.
//!
//! The comparisons below therefore call **one engine function each** and never rebuild a
//! DTO by hand. Where a verb is pure backend (`list`, `info`) the comparison is against the
//! backend primitive — `enumerate`, `diagnose`, `formats` — for the same reason.
//!
//! ## The refusals
//!
//! Four ways of asking for something that is not there, plus the thirteen methods this
//! build does not route. Each arrives as a D13 error carrying P4a's code, and each is
//! recovered client-side through `api::codes::typed` — which is the half of the seam
//! `wchc` depends on and the half a server-side test cannot see.

mod support;

use std::fmt;
use std::future::Future;
use std::sync::Arc;

use api::codes::{self, ErrorObjectOwned};
use api::{WchRpcClient, WchRpcServer, rpc_code};
use camino::Utf8PathBuf;
use daemon::server::Wchd;
use daemon::uds::{self, SocketDir};
use engine::lifecycle::{self, SessionSpec};
use engine::paths::TempRuntimeDir;
use engine::store::{LockProtocol, SessionStore, StoreLock, TempStore};
use fake::FakeBackend;
use jsonrpsee::core::DeserializeOwned;
use jsonrpsee::core::client::{BatchResponse, ClientT, Error as ClientError};
use jsonrpsee::core::params::BatchRequestBuilder;
use jsonrpsee::core::traits::ToRpcParams;
use jsonrpsee_server::Methods;
use schema::backend::CameraBackend;
use schema::camera::{CameraId, CameraInfo};
use schema::control::{ControlDesc, ControlSlug};
use schema::error::{Error, ErrorKind};
use schema::report::{CameraDetail, CameraList, ControlReport};
use schema::session::{SessionList, SessionRef, SessionStatus};
use serde_json::value::RawValue;
use tokio::net::UnixStream;

// ------------------------------------------------------------------------ the transports

/// One of the two transports docs/7 P4b names, as something the generated client can drive.
///
/// A single enum with a single [`ClientT`] implementation, rather than two clients: what
/// the suite compares is two pipes carrying one surface, so the code that turns a method
/// call into bytes has to be the same on both sides of the comparison or the comparison is
/// between two encoders.
#[derive(Debug, Clone)]
enum Wire {
    /// jsonrpsee's own in-memory dispatch — design §2.9's double for the RPC transport.
    ///
    /// It drives the real parsing, the real `param_kind = map` decoding, the real handler
    /// and the real `WireError → -320xx` conversion; everything except the socket. Its
    /// fault menu in §2.9 has exactly one entry ("disconnect mid-subscription"), which is
    /// P4e's subject, so at P4b this is a *speed* double and not a fault double — and
    /// nothing here pretends otherwise.
    InMemory(Methods),
    /// A real `AF_UNIX` socket, spoken to by a real HTTP/1.1 client (`support`).
    Uds(Utf8PathBuf),
}

impl Wire {
    /// One request in, the JSON-RPC response document out.
    async fn round_trip(&self, request: String) -> Result<String, ClientError> {
        match self {
            Wire::InMemory(methods) => {
                // The subscription buffer, which nothing on this surface uses: T5 declares
                // no subscription until P4e (note N29).
                let (answer, _subscriptions) = methods.raw_json_request(&request, 1).await?;
                Ok(answer.to_string())
            }
            Wire::Uds(socket) => {
                // A socket nobody is listening on is a *transport* failure, kept distinct
                // from a refusal the daemon sent (E3, at the transport): `refusal` below
                // panics if the two are ever confused, and the last test in this file
                // depends on this arm being reachable.
                let answer = support::call(socket, &request)
                    .await
                    .map_err(|err| ClientError::Transport(Box::new(err)))?;
                Ok(support::body(&answer).to_owned())
            }
        }
    }
}

impl ClientT for Wire {
    fn request<R, Params>(
        &self,
        method: &str,
        params: Params,
    ) -> impl Future<Output = Result<R, ClientError>> + Send
    where
        R: DeserializeOwned,
        Params: ToRpcParams + Send,
    {
        // `params` is whatever the generated client built — an `ObjectParams` for every
        // method on this surface, because D10 makes every parameter named. It is
        // serialized here and never inspected: a transport that understood the surface
        // would be a second client.
        let request = envelope(method, params);
        async move {
            let answer = self.round_trip(request?).await?;
            decode(&answer)
        }
    }

    async fn notification<Params>(&self, _method: &str, _params: Params) -> Result<(), ClientError>
    where
        Params: ToRpcParams + Send,
    {
        // Refused rather than implemented, because T5 declares no notification: every
        // method answers, and a fire-and-forget call to one of them would be a request
        // whose refusal nobody could see. A transport double that silently accepted one
        // would be a capability this daemon does not have.
        Err(ClientError::Custom(
            "the T5 surface has no notifications".to_owned(),
        ))
    }

    async fn batch_request<'a, R>(
        &self,
        _batch: BatchRequestBuilder<'a>,
    ) -> Result<BatchResponse<'a, R>, ClientError>
    where
        R: DeserializeOwned + fmt::Debug + 'a,
    {
        // Batching is a transport property, not a surface one: the bound the daemon serves
        // under is `schema::limits::RPC_MAX_BATCH` and it is asserted where the transport
        // is, in `uds.rs`. Nothing this project ships batches — `wch` and `wchc` run one
        // verb per invocation — so a batching client here would be a client with no
        // product behind it.
        Err(ClientError::Custom(
            "this suite sends one call at a time".to_owned(),
        ))
    }
}

/// The request id every call in this suite uses.
///
/// One call per connection, and the daemon echoes what it was sent, so the value only has
/// to be something [`decode`] can recognise the answer by — which it does not need to,
/// because there is never a second answer in flight.
const REQUEST_ID: u32 = 1;

/// Wrap a method name and the generated client's parameters in a JSON-RPC request.
fn envelope<Params: ToRpcParams>(method: &str, params: Params) -> Result<String, ClientError> {
    let params = params.to_rpc_params()?;
    // `None` for a method with no parameters at all (`wch_list`), and the key is left out
    // rather than sent as `null` — which `crates/api`'s own registration test measured to
    // be the shape a by-name server accepts.
    let request = match params {
        Some(params) => serde_json::json!({
            "jsonrpc": "2.0", "id": REQUEST_ID, "method": method, "params": params,
        }),
        None => serde_json::json!({ "jsonrpc": "2.0", "id": REQUEST_ID, "method": method }),
    };
    Ok(request.to_string())
}

/// Turn a JSON-RPC response document into the method's answer, or into a client error.
///
/// Parsed from the **text**, not through `serde_json::Value`: an error object carries its
/// `data` as a `RawValue`, which only serde_json's own text deserializer can capture, and
/// that payload is the whole D13 error `api::codes::typed` reconstructs.
fn decode<R: DeserializeOwned>(answer: &str) -> Result<R, ClientError> {
    let document: std::collections::BTreeMap<String, Box<RawValue>> = serde_json::from_str(answer)?;
    if let Some(error) = document.get("error") {
        return Err(ClientError::Call(serde_json::from_str::<ErrorObjectOwned>(
            error.get(),
        )?));
    }
    match document.get("result") {
        Some(result) => Ok(serde_json::from_str(result.get())?),
        None => Err(ClientError::Custom(format!(
            "a JSON-RPC response with neither a result nor an error: {answer}"
        ))),
    }
}

// ---------------------------------------------------------------------------- the fixture

/// A daemon over two replayed cameras and a state directory with one session in it.
///
/// Everything the transports need is held here because dropping any of it would take the
/// socket, the lock or the state tree with it — the arrangement a real `wchd` has, which is
/// why this is one value and not six locals.
struct Fixture {
    backend: Arc<FakeBackend>,
    cameras: Vec<CameraInfo>,
    store: SessionStore,
    socket: Utf8PathBuf,
    methods: Methods,
    handle: uds::Serving,
    _lock: StoreLock,
    _state: TempStore,
    _runtime: TempRuntimeDir,
}

impl Fixture {
    /// Start one. Must be called from inside a tokio runtime.
    fn start() -> Fixture {
        // Two cameras, because half of what this suite asserts is about *which* camera: an
        // ambiguous prefix needs two candidates, and narrowing a session listing by camera
        // needs two fingerprints. One camera would make both unreachable rather than
        // untested.
        let backend = Arc::new(
            FakeBackend::new(vec![testkit::fixtures::synthetic_basic(), second_camera()])
                .expect("the synthetic profile is this build's version"),
        );
        let cameras = backend
            .enumerate()
            .expect("the fake enumerates what it replays");

        let state = TempStore::new().expect("a state directory");
        let store = SessionStore::new(state.root());

        // The sessions `calibrate status` and `calibrate list` answer about — **one per
        // camera**, so that listing every session and listing one camera's are different
        // answers rather than the same one twice. They are written before the daemon takes
        // the directory: D9 gives a running daemon the lock for its lifetime, so a fixture
        // that wrote afterwards would be contending with the thing it is setting up.
        for index in 0..cameras.len() {
            store
                .with_lock(|lock| {
                    lifecycle::create(
                        &store,
                        lock,
                        &SessionSpec {
                            id: uuid::Uuid::now_v7(),
                            fingerprint: camera(&cameras, index).fingerprint.clone(),
                            task: "exposure".to_owned(),
                            goal: "a legible frame".to_owned(),
                            criteria: vec!["sharp".to_owned()],
                            tool_version: schema::TOOL_VERSION.to_owned(),
                        },
                        schema::time::Stamp::now(),
                    )
                })
                .expect("a fresh state directory takes the daemonless lock");
        }

        let lock = store
            .lock(LockProtocol::HeldForLifetime)
            .expect("nothing else holds a throw-away state directory");
        let runtime = TempRuntimeDir::new().expect("a runtime directory");
        let dir = SocketDir::prepare(&runtime.env()).expect("a fresh, private socket directory");
        let listener = dir.bind(&lock).expect("nothing is in the way");

        // One daemon, two transports. `into_rpc` consumes a clone; the value behind it is
        // an `Arc`, so both wires reach the same registry, the same backend and the same
        // store — which is what makes a difference between them a difference in transport.
        let wchd = Wchd::new(
            Arc::clone(&backend) as Arc<dyn CameraBackend>,
            SessionStore::new(state.root()),
        );
        let methods: Methods = wchd.into_rpc().into();

        Fixture {
            handle: uds::serve(listener, methods.clone()),
            socket: dir.socket_path(),
            methods,
            backend,
            cameras,
            store,
            _lock: lock,
            _state: state,
            _runtime: runtime,
        }
    }

    /// The two transports, in the order a failure should be read in.
    fn wires(&self) -> [(&'static str, Wire); 2] {
        [
            ("in-memory", Wire::InMemory(self.methods.clone())),
            ("uds", Wire::Uds(self.socket.clone())),
        ]
    }

    /// What every call in this suite asks for.
    fn ask(&self) -> Ask {
        let first = camera(&self.cameras, 0).id.clone();
        let second = camera(&self.cameras, 1).id.clone();

        // An ambiguous prefix, derived rather than written: the fake replays one profile
        // twice, so the second camera's id is the first's plus a suffix, and dropping the
        // last character of the shorter one is a prefix of both that is an exact match for
        // neither. Written out, this would be a literal that stops being ambiguous the
        // day the id scheme changes.
        let shorter = if first.as_str().len() <= second.as_str().len() {
            first.as_str()
        } else {
            second.as_str()
        };
        let mut trimmed = shorter.to_owned();
        trimmed.pop();
        let ambiguous = CameraId::parse(&trimmed).expect("a non-empty prefix");
        assert_ne!(ambiguous, first, "the prefix is an exact match after all");
        assert_ne!(ambiguous, second, "the prefix is an exact match after all");

        // A control this camera really has, taken off the device rather than named.
        let control = self
            .controls()
            .first()
            .map(|desc| desc.slug.clone())
            .expect("the synthetic profile has controls");

        Ask {
            camera: first,
            ambiguous,
            unknown_camera: CameraId::parse("cam:nothing-answers-to-this").expect("a literal id"),
            control,
            unknown_control: ControlSlug::parse("brightnes").expect("a literal slug"),
            session: SessionRef::Task {
                task: "exposure".to_owned(),
            },
            unknown_session: SessionRef::Id {
                id: uuid::Uuid::now_v7(),
            },
        }
    }

    /// The first camera's controls, read straight off the backend.
    fn controls(&self) -> Vec<ControlDesc> {
        self.backend
            .open(&camera(&self.cameras, 0).id)
            .expect("the fake opens")
            .controls()
            .expect("the fake answers")
    }
}

/// A second device, made from the committed fixture by renaming it.
///
/// Replaying `synthetic_basic` twice would give two cameras with one **fingerprint** —
/// D9 keys the session tree on the fingerprint, so both would own the same sessions and
/// "one camera's sessions" would be indistinguishable from "every session". Two devices
/// that share a bus path are also not a thing hardware does \[PF:13\].
///
/// A rename rather than a second committed document: what this fixture needs is *a second
/// camera*, and a hand-written profile would be a second fixture to keep current for a
/// property that has nothing to do with the device it describes. The name stays a prefix
/// of nothing and a superset of the first, which is what keeps the ambiguous-prefix arm
/// derivable.
fn second_camera() -> schema::profile::DeviceProfile {
    let mut profile = testkit::fixtures::synthetic_basic();
    profile.invariant.info.card.push_str(" Two");
    profile.invariant.info.fingerprint.card = profile.invariant.info.card.clone();
    profile.invariant.info.fingerprint.bus_path = "3-5:1.0".to_owned();
    profile
}

/// One camera out of the enumeration, by position.
fn camera(cameras: &[CameraInfo], index: usize) -> &CameraInfo {
    cameras
        .get(index)
        .unwrap_or_else(|| panic!("the fixture replays fewer than {} cameras", index + 1))
}

/// Everything the suite asks each transport for.
#[derive(Debug, Clone)]
struct Ask {
    camera: CameraId,
    ambiguous: CameraId,
    unknown_camera: CameraId,
    control: ControlSlug,
    unknown_control: ControlSlug,
    session: SessionRef,
    unknown_session: SessionRef,
}

// -------------------------------------------------------------------- the shared questions

/// Every answer the six routed verbs give, over one transport.
#[derive(Debug, PartialEq)]
struct Answers {
    list: CameraList,
    info: CameraDetail,
    controls: ControlReport,
    get: ControlDesc,
    status: SessionStatus,
    every_session: SessionList,
    one_cameras_sessions: SessionList,
}

/// Ask one client all six read verbs.
///
/// Generic over the **generated** client trait rather than over this file's [`Wire`], which
/// is what makes it a test of the surface: the method names, the parameter names and the
/// response types all come from `webcam-handler-api`'s declaration, so a rename there is a
/// compile failure here rather than a JSON literal that quietly stops matching.
async fn read_verbs<C: WchRpcClient + Sync>(client: &C, ask: &Ask) -> Answers {
    Answers {
        list: client.list().await.expect("the fake enumerates"),
        info: client
            .info(ask.camera.clone())
            .await
            .expect("the camera resolves and opens"),
        controls: client
            .controls(ask.camera.clone())
            .await
            .expect("the camera answers"),
        get: client
            .get(ask.camera.clone(), ask.control.clone())
            .await
            .expect("the control is one this camera has"),
        status: client
            .calibrate_status(ask.camera.clone(), ask.session.clone())
            .await
            .expect("the fixture opened this session"),
        // Both shapes of the one optional parameter on this surface: `None` is every
        // session on the machine, `Some` is one camera's (note the `camera` key is
        // omissible, which `crates/api` measured against a real module).
        every_session: client
            .calibrate_list(None)
            .await
            .expect("listing parses nothing, so it cannot fail on a document"),
        one_cameras_sessions: client
            .calibrate_list(Some(ask.camera.clone()))
            .await
            .expect("the camera resolves"),
    }
}

/// The typed refusals, each as the code that arrived and the D13 error a client recovers.
#[derive(Debug, PartialEq)]
struct Refusals {
    unknown_camera: (i32, Error),
    ambiguous_prefix: (i32, Error),
    unknown_control: (i32, Error),
    unknown_session: (i32, Error),
    unrouted_method: (i32, Error),
}

/// Ask one client for four things that are not there, and one verb this build does not
/// route.
async fn refusals<C: WchRpcClient + Sync>(client: &C, ask: &Ask) -> Refusals {
    Refusals {
        unknown_camera: refusal(client.info(ask.unknown_camera.clone()).await),
        ambiguous_prefix: refusal(client.info(ask.ambiguous.clone()).await),
        unknown_control: refusal(
            client
                .get(ask.camera.clone(), ask.unknown_control.clone())
                .await,
        ),
        unknown_session: refusal(
            client
                .calibrate_status(ask.camera.clone(), ask.unknown_session.clone())
                .await,
        ),
        // The thirteenth kind of answer this build gives: a method that is registered,
        // reachable, and waiting for P4c. `snapshot` stands for all thirteen because the
        // set is asserted where it is derived (`daemon::server`'s own tests); what is
        // asserted here is that the refusal survives the wire as a D13 error.
        unrouted_method: refusal(client.snapshot(ask.camera.clone()).await),
    }
}

/// Recover the D13 error a refusal carried, the way `wchc` will.
///
/// The code is returned beside the error rather than checked here, so the caller asserts it
/// against the kind it expected: `codes::typed` already refuses an object whose code and
/// payload disagree, and re-checking that inside this helper would be a branch no input can
/// reach.
fn refusal<T: fmt::Debug>(answer: Result<T, ClientError>) -> (i32, Error) {
    match answer {
        Ok(answered) => panic!("expected a refusal, got {answered:?}"),
        Err(ClientError::Call(object)) => {
            let code = object.code();
            let error = codes::typed(&object).unwrap_or_else(|| {
                panic!("a refusal that is not a D13 error crossed the wire: {object:?}")
            });
            (code, error)
        }
        // The distinction E3 keeps everywhere else, at the transport: a camera's answer
        // must never arrive looking like a broken pipe, or the reverse.
        Err(other) => panic!("a typed refusal arrived as a transport failure: {other}"),
    }
}

// ------------------------------------------------------------------------------- the tests

#[tokio::test]
async fn the_read_verbs_answer_the_same_over_both_transports() {
    // docs/7 P4b's first proof obligation. One daemon, two pipes, one set of questions —
    // so an inequality here is the transport and can be nothing else.
    let fixture = Fixture::start();
    let ask = fixture.ask();

    let [(first_name, first_wire), (second_name, second_wire)] = fixture.wires();
    let first = read_verbs(&first_wire, &ask).await;
    let second = read_verbs(&second_wire, &ask).await;

    assert_eq!(
        first, second,
        "{first_name} and {second_name} answered differently"
    );

    // Not vacuous: an answer that was empty on both sides would compare equal and say
    // nothing. Every verb has to have found something.
    assert_eq!(first.list.cameras.len(), 2);
    assert!(!first.info.formats.is_empty());
    assert!(!first.controls.controls.is_empty());
    assert!(!first.controls.pairs.is_empty());
    assert_eq!(first.status.session.task, "exposure");
    assert_eq!(first.every_session.sessions.len(), 2);
    assert_eq!(first.one_cameras_sessions.sessions.len(), 1);
}

#[tokio::test]
async fn every_read_verb_answers_what_the_engine_answers() {
    // Deliverable D's assertable half: the daemon assembles nothing of its own. Each
    // comparison calls **one** engine or backend function — the same one `crates/cli`'s
    // executor calls — rather than rebuilding the document, because a test that rebuilt it
    // would be checking its own copy of the assembly against the daemon's.
    let fixture = Fixture::start();
    let ask = fixture.ask();
    let controls = fixture.controls();
    let fingerprint = camera(&fixture.cameras, 0).fingerprint.clone();

    let (_, wire) = fixture
        .wires()
        .into_iter()
        .next()
        .expect("two transports were built");
    let answers = read_verbs(&wire, &ask).await;

    // `list` is `engine::resolve::list` and nothing else — compared against the engine
    // function rather than against the two backend calls behind it, because `wch list`
    // reaches the same function and a comparison against the ingredients would pass while
    // the two surfaces assembled them differently (design §2.10, and P4f's parity gate a
    // sub-milestone later).
    assert_eq!(
        answers.list,
        engine::resolve::list(fixture.backend.as_ref()).expect("the fake enumerates")
    );

    // `info` is the resolved camera and the device's own format tree.
    assert_eq!(&answers.info.info, camera(&fixture.cameras, 0));
    assert_eq!(
        answers.info.formats,
        fixture
            .backend
            .open(&ask.camera)
            .expect("the fake opens")
            .formats()
            .expect("the fake answers")
    );

    // `controls` is the device's control set and the pair set the engine says is in
    // effect for it — read-only, so nothing was measured (note N30).
    assert_eq!(answers.controls.controls, controls);
    assert_eq!(answers.controls.camera, ask.camera);
    assert_eq!(
        answers.controls.pairs,
        engine::pairing::in_effect(&controls, Vec::new())
    );

    // `get` is the engine's lookup, which is also where a miss gets its suggestions.
    assert_eq!(
        answers.get,
        engine::pairing::describe(&controls, &ask.control).expect("the camera has it")
    );

    // The two calibrate verbs are the engine's, whole.
    assert_eq!(
        answers.status,
        lifecycle::status(&fixture.store, &fingerprint, &ask.session)
            .expect("the session is there")
    );
    assert_eq!(
        answers.every_session,
        lifecycle::list(&fixture.store, None).expect("the store walks")
    );
    assert_eq!(
        answers.one_cameras_sessions,
        lifecycle::list(&fixture.store, Some(&fingerprint)).expect("the store walks")
    );

    // And the two listings are not accidentally the same question — which is the whole
    // reason the fixture opens a session per camera. Narrowing has to narrow, or a daemon
    // that ignored the `camera` parameter would answer both calls identically and every
    // assertion above would still hold.
    assert_eq!(answers.every_session.sessions.len(), 2);
    assert_eq!(answers.one_cameras_sessions.sessions.len(), 1);
    let other = camera(&fixture.cameras, 1).fingerprint.clone();
    assert_ne!(
        answers.one_cameras_sessions,
        lifecycle::list(&fixture.store, Some(&other)).expect("the store walks")
    );
}

#[tokio::test]
async fn the_failure_directions_cross_both_transports_as_the_same_typed_error() {
    // Deliverable E. Each refusal arrives as a D13 error with P4a's code and is recovered
    // client-side through `api::codes` — which is the half `wchc` depends on and the half a
    // server-side assertion cannot see.
    let fixture = Fixture::start();
    let ask = fixture.ask();

    let [(first_name, first_wire), (second_name, second_wire)] = fixture.wires();
    let first = refusals(&first_wire, &ask).await;
    let second = refusals(&second_wire, &ask).await;
    assert_eq!(
        first, second,
        "{first_name} and {second_name} refused differently"
    );

    // The codes are P4a's, asserted against the kind each refusal is supposed to be —
    // which is what makes this more than "some error came back".
    assert_eq!(
        first.unknown_camera,
        (
            rpc_code(ErrorKind::CameraUnknown),
            Error::CameraUnknown {
                requested: ask.unknown_camera.to_string(),
            }
        )
    );

    // The ambiguous prefix names its candidates, and they are the engine's — a daemon that
    // picked one would be the silent-wrong-camera defect D1 exists to prevent.
    assert_eq!(
        first.ambiguous_prefix,
        (
            rpc_code(ErrorKind::CameraAmbiguous),
            engine::resolve::camera(&fixture.cameras, &ask.ambiguous)
                .expect_err("two cameras answer to this prefix")
        )
    );

    // The unknown control's suggestions are the planner's, so `get brightnes` over the
    // wire names what `set brightnes=1` would.
    assert_eq!(
        first.unknown_control,
        (
            rpc_code(ErrorKind::ControlUnknown),
            engine::pairing::describe(&fixture.controls(), &ask.unknown_control)
                .expect_err("no such control")
        )
    );

    // A session id nothing answers to is a refusal, not an empty document: a status verb
    // that invented one would let a later verb write to a camera with nothing recording it.
    assert_eq!(
        first.unknown_session.0,
        rpc_code(ErrorKind::IllegalTransition)
    );
    assert_eq!(first.unknown_session.1.kind(), ErrorKind::IllegalTransition);

    // And the thirteen methods this build does not route say so in D13's words, over the
    // wire, with the phase that lands them (note N43).
    assert_eq!(
        first.unrouted_method,
        (
            rpc_code(ErrorKind::Unimplemented),
            Error::Unimplemented {
                operation: "wch_snapshot".to_owned(),
                arrives_in: "P4c".to_owned(),
            }
        )
    );

    // Every refusal used a distinct code, so none of the assertions above is passing on a
    // registry that had collapsed to one error.
    let codes: std::collections::BTreeSet<i32> = [
        first.unknown_camera.0,
        first.ambiguous_prefix.0,
        first.unknown_control.0,
        first.unknown_session.0,
        first.unrouted_method.0,
    ]
    .into_iter()
    .collect();
    assert_eq!(codes.len(), 5, "{codes:?}");
}

#[tokio::test]
async fn the_two_wires_are_genuinely_two() {
    // Every comparison above depends on this and none of them can see it: a `Wire::Uds`
    // that had quietly fallen back to the in-memory module would make the whole suite one
    // transport run twice, and every `assert_eq!` would still pass.
    //
    // So: the socket answers while the server is up, stops answering when it is stopped —
    // and the in-memory dispatch keeps answering either way, because it never went through
    // a socket at all.
    let mut fixture = Fixture::start();
    let ask = fixture.ask();
    let [(_, in_memory), (_, over_uds)] = fixture.wires();

    over_uds.list().await.expect("the socket is serving");

    let socket = fixture.socket.clone();
    fixture.handle.stop();
    // Resolves once the accept loop and every connection it spawned are gone, and answers
    // *why* it stopped — `Ok` because this test asked, rather than because the daemon gave
    // up on `accept`, which is the distinction `wchd`'s exit code rests on.
    fixture
        .handle
        .stopped()
        .await
        .expect("the server was asked to stop");
    assert!(
        UnixStream::connect(socket.as_std_path()).await.is_err(),
        "something is still listening on {socket}"
    );

    // The wire goes with the socket. A `Wire::Uds` that had quietly been dispatching in
    // memory would answer here — and what comes back is a *transport* failure, not a
    // refusal the daemon sent, which is E3's distinction at the transport layer and the
    // one P4f's "the daemon is not running" message is built on.
    match over_uds.list().await {
        Err(ClientError::Transport(_)) => {}
        other => panic!("a socket nobody is listening on answered: {other:?}"),
    }

    // And the in-memory wire never needed the socket, which is the other half of "two".
    read_verbs(&in_memory, &ask).await;
}
