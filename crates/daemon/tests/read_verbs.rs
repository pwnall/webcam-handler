//! The six read verbs, over both transports the plan names, driven by the generated client.
//!
//! docs/7 P4b's proof obligation, in one file: "read-verb integration tests over in-memory
//! and UDS transports". The two transports are design §2.9's seam and its double — a real
//! `AF_UNIX` socket, and jsonrpsee's in-memory dispatch — and the point of having both is
//! that the *only* difference between them is the pipe: one `Wchd` answers both, so an
//! answer that differs differs because of the transport and nothing else.
//!
//! The transports and the fixture live in `support/wire.rs` and `support/fixture.rs`,
//! shared with `mutating_verbs.rs`, because two suites over one surface must not be able to
//! disagree about what they are talking to.
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
//! Four ways of asking for something that is not there. Each arrives as a D13 error
//! carrying P4a's code, and each is recovered client-side through `api::codes::typed` —
//! which is the half of the seam `wchc` depends on and the half a server-side test cannot
//! see.
//!
//! There used to be a fifth: a method this build did not route yet, answering
//! `Error::Unimplemented` with the phase that would land it. P4c landed all of them, so the
//! arm inverted rather than moved (note **N43**), and P4d then deleted the variant (note
//! **N6**) — so the D13 registry's eighteen members no longer include one that means "not
//! built yet", and there is nothing left for a sixth arm to ask about.

#[path = "support/fixture.rs"]
mod fixture;
mod support;
#[path = "support/wire.rs"]
mod wire;

use api::{WchRpcClient, rpc_code};
use engine::lifecycle;
use jsonrpsee::core::client::Error as ClientError;
use schema::backend::CameraBackend;
use schema::control::ControlDesc;
use schema::error::{Error, ErrorKind};
use schema::report::{CameraDetail, CameraList, ControlReport};
use schema::session::{SessionList, SessionRef, SessionStatus};
use tokio::net::UnixStream;

use crate::fixture::{Ask, Fixture, camera};
use crate::wire::refusal;

/// The two session references this suite asks about.
///
/// Not on [`Ask`], because a session is not something every suite over this surface names:
/// the fixture's own is derived from `fixture::SESSION_TASK` so it matches the document the
/// fixture wrote, and the unknown one is a fresh UUIDv7 built **once** — the refusal names
/// the id, so two readings would make the two transports disagree for a reason that is not
/// the transport.
#[derive(Debug, Clone)]
struct Sessions {
    session: SessionRef,
    unknown_session: SessionRef,
}

fn sessions() -> Sessions {
    Sessions {
        session: SessionRef::Task {
            task: fixture::SESSION_TASK.to_owned(),
        },
        unknown_session: SessionRef::Id {
            id: uuid::Uuid::now_v7(),
        },
    }
}

// -------------------------------------------------------------------- the shared questions

/// Every answer the six routed read verbs give, over one transport.
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
/// Generic over the **generated** client trait rather than over [`wire::Wire`], which
/// is what makes it a test of the surface: the method names, the parameter names and the
/// response types all come from `webcam-handler-api`'s declaration, so a rename there is a
/// compile failure here rather than a JSON literal that quietly stops matching.
async fn read_verbs<C: WchRpcClient + Sync>(client: &C, ask: &Ask, which: &Sessions) -> Answers {
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
            .calibrate_status(ask.camera.clone(), which.session.clone())
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
}

/// Ask one client for four things that are not there.
async fn refusals<C: WchRpcClient + Sync>(client: &C, ask: &Ask, which: &Sessions) -> Refusals {
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
                .calibrate_status(ask.camera.clone(), which.unknown_session.clone())
                .await,
        ),
    }
}

// ------------------------------------------------------------------------------- the tests

#[tokio::test]
async fn the_read_verbs_answer_the_same_over_both_transports() {
    // docs/7 P4b's first proof obligation. One daemon, two pipes, one set of questions —
    // so an inequality here is the transport and can be nothing else.
    let fixture = Fixture::start();
    let ask = fixture.ask();
    let which = sessions();

    let [(first_name, first_wire), (second_name, second_wire)] = fixture.wires();
    let first = read_verbs(&first_wire, &ask, &which).await;
    let second = read_verbs(&second_wire, &ask, &which).await;

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
    assert_eq!(first.status.session.task, fixture::SESSION_TASK);
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
    let which = sessions();
    let controls = fixture.controls();
    let fingerprint = camera(&fixture.cameras, 0).fingerprint.clone();

    let (_, wire) = fixture
        .wires()
        .into_iter()
        .next()
        .expect("two transports were built");
    let answers = read_verbs(&wire, &ask, &which).await;

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
        lifecycle::status(&fixture.store, &fingerprint, &which.session)
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
    let which = sessions();

    let [(first_name, first_wire), (second_name, second_wire)] = fixture.wires();
    let first = refusals(&first_wire, &ask, &which).await;
    let second = refusals(&second_wire, &ask, &which).await;
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

    // Every refusal used a distinct code, so none of the assertions above is passing on a
    // registry that had collapsed to one error.
    let codes: std::collections::BTreeSet<i32> = [
        first.unknown_camera.0,
        first.ambiguous_prefix.0,
        first.unknown_control.0,
        first.unknown_session.0,
    ]
    .into_iter()
    .collect();
    assert_eq!(codes.len(), 4, "{codes:?}");
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
    let which = sessions();
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
    read_verbs(&in_memory, &ask, &which).await;
}
