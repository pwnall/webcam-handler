//! The opt-in TCP transport: one axum server, the embedded client behind it, and the token
//! gate in front of the two routes that are the camera (D11 and its 2026-08-12 amendment,
//! docs/7 P5a).
//!
//! [`crate::uds`] is this module's model and its counterweight. That transport is **always**
//! served and its auth model is the filesystem; this one exists only because somebody typed
//! `--http`, and its auth model is a secret this process minted ([`super::token`]). What the
//! two have in common is the shape: a listener somebody else bound, a server that runs as a
//! task, a handle the composition root **holds** rather than drops, and a doc comment that
//! says what the lifecycle does not cover.
//!
//! ## Two entry points, and the difference between them is what a test can inject
//!
//! [`serve`] decides nothing. The [`Posture`] arrives already decided ([`super::posture`], one
//! expression, four cells, no socket) and the [`Token`] arrives already minted; this module
//! *reads* them. That is what makes D11's two non-loopback cells assertable on a machine with
//! one interface: a test binds `127.0.0.1:0` and hands in a posture decided about
//! `192.168.1.10:8080`, and what it then establishes is precisely **that the gate is installed
//! because the posture said so**.
//!
//! It establishes nothing about *where the posture came from* — an injected value cannot say
//! anything about that. Which is why [`open`] exists beside it and is what the composition
//! root actually calls: the whole of what `--http` costs, from the two values clap parsed to a
//! running server, in one function that the daemon's own suite drives. `crate::lib`'s header
//! asks for exactly this arrangement — "everything it composes lives in the library beside it,
//! so an integration test can build the same server without a process" — and here it buys
//! something specific, because `main.rs` is a binary and no integration test can call into it.
//! What is left in `main` is: parse, call this, log what it answers, join it.
//!
//! ## The bound address, not the requested one
//!
//! D11's default is `127.0.0.1:0` and the same sentence says "report the bound port", so
//! [`Serving::bound`] is read off the listener with `local_addr()`, and
//! [`Serving::ready_to_open_url`] — the line an operator copies — is built from that and
//! nothing else. The *requested* address is what the posture was decided from, and the two
//! differ in exactly one field: the port, which [`Posture`] has no opinion about, because
//! reach is a property of the address. A URL built from the request would send an operator to
//! port zero.
//!
//! ## Stopping: what is claimed
//!
//! The server watches the daemon's one [`Shutdown`] token — the same clone the subscriptions
//! and the idle-sweep driver watch — so the web listener begins stopping at **step 3** of
//! [`crate::shutdown`]'s order, when that token is cancelled, and not at some later step of
//! its own. `axum::serve(..).with_graceful_shutdown(..)` is how the token reaches hyper: it
//! stops accepting, closes idle keep-alive connections, and lets a response that is being
//! written finish.
//!
//! [`Serving::stopped`] is then **joined** by the composition root, which is the doctrine
//! [`crate::uds::Serving`] states one module along and `main` already applies to the
//! idle-sweep driver and the watchdog: an ending nothing waited for is a maybe, and "the HTTP
//! listener ended" has to be a fact the process waited for rather than a consequence of the
//! runtime being dropped at the end of `main`.
//!
//! ## Stopping: the response that does not end on its own
//!
//! Everything this build serves through the asset fallback is a file of a few kilobytes, so
//! for those the graceful stop is bounded by a `write` to a socket — a property of what is
//! served rather than a guarantee this module provides. The **MJPEG preview** is the one
//! response that has no such property: `multipart/x-mixed-replace` runs until the client goes
//! away, and `axum::serve`'s graceful shutdown waits for a response that is being written, so
//! an open tab would hold this stop open for as long as somebody's browser stayed open. Design
//! §2.6 states the requirement that follows — "an open MJPEG tab must not hang shutdown".
//!
//! Meeting it takes **two** things, and P5b learned the second one by measuring rather than by
//! reasoning. The first is inside the preview: [`super::preview`]'s writer watches the same
//! [`Shutdown`] token this server does, on both of the two places it can wait — for a frame,
//! and for room in its channel — so a cancelled daemon ends the body instead of waiting for a
//! client to close a tab.
//!
//! That is necessary and it is **not sufficient**, which is the finding. Ending a body leaves
//! hyper a final chunk to *write*, and a reader that has stopped reading has a full socket, so
//! the write cannot complete and the connection never finishes; `axum::serve`'s graceful
//! shutdown then waits for a browser to be scrolled back into view. So the second thing is a
//! **bound on the join** — [`Serving::stopped`], [`limits::WEB_LISTENER_STOP_MS`] — after which
//! the listener task is aborted and the daemon says so at `warn`. That is AGENTS' own rule for
//! this case ("open streams are cancelled, never awaited, on shutdown") rather than a
//! concession to it. `crates/daemon/tests/preview.rs` drives the hard version — a tab that has
//! provably stopped reading, with the writer parked in a send — and asserts the stop inside
//! `limits::DAEMON_SHUTDOWN_DRAIN_MS`, so both halves are a bound rather than a paragraph.
//!
//! **A WebSocket is not that response, and that is worth being explicit about**, because it
//! looks like one. An upgraded connection stops belonging to axum the instant hyper hands the
//! socket over — the connection future resolves, the graceful shutdown counts it as finished
//! — so an open browser tab full of subscriptions cannot hold this stop open at all. What ends
//! such a connection is jsonrpsee's `ServerHandle`, which [`Serving::stopped`] asks and whose
//! *timing* is an ordering rather than a detail (see that method, and [`super::rpc`]).
//!
//! **No accept-failure policy of its own.** [`crate::uds::serve`] gives up after
//! [`schema::limits::MAX_CONSECUTIVE_ACCEPT_FAILURES`] consecutive failures because that
//! transport is the daemon's *always-on* one and a daemon that has stopped accepting on it has
//! stopped being a daemon — which is why it reaches `main`'s exit code and therefore
//! `Restart=on-failure`. This listener is opt-in, the Unix socket is unaffected by anything
//! that happens to it, and axum's own accept loop backs off and retries; a fatal failure ends
//! the task, which says so at `error!` **when it happens** rather than at the next teardown.
//! It is stated rather than made to match, because making it match would mean this daemon
//! exits non-zero — and asks a service manager to restart it — when a browser transport it was
//! asked to add as an extra goes away.
//!
//! ## The gate is over the routes, and the assets are outside it
//!
//! The owner ruled (2026-08-12) that **static assets are served without authentication** —
//! the client is open-source code rather than a secret — and that only the resources which
//! *carry or drive the camera* stay behind D11's token: the WebSocket endpoint and the MJPEG
//! preview, which is exactly what [`super::CAMERA_BEARING_PATHS`] names. So in D11's three
//! token-gated cells the gate is a [`Router::route_layer`] over the routes and the asset
//! **fallback** is outside it; in the token-less loopback cell nothing is installed at all.
//! Note **N82** carries the ruling and what it changed; it retires note N76.
//!
//! **`route_layer` and not `layer`, which is the exact inverse of what note N75 argued and
//! for the same reason.** `layer` maps over `path_router`, `fallback_router` **and**
//! `catch_all_fallback`, so it wraps the request for a path that does not exist; `route_layer`
//! maps over `path_router` alone. While the assets were behind the token, `layer` was the one
//! tool that stopped an anonymous `GET /nothing-here` telling a stranger which paths this
//! daemon has. With the assets open there is nothing left to tell — the path table is a
//! directory in a public repository, and the two paths still behind the token are `const`s in
//! the same one — so what is left to protect is the camera, and the camera is on the routes.
//!
//! **The gate is still absent rather than permissive in the token-less cell**, which is note
//! N75's other half and is untouched by the ruling: the branch is one `match` arm at
//! composition, over a value a reviewer can read, rather than a bypass inside the one function
//! whose entire job is to say no.
//!
//! ### What the narrowing costs, and what pays for it
//!
//! "Every route is gated" used to be a property of one call: a request could not reach a
//! handler without meeting the gate, and no list had to be kept. It is now a property of
//! **where a route is registered** — `route_layer` wraps the routes that exist when it is
//! called and says nothing about later ones — so a camera-bearing route merged after that line
//! is a live camera served to strangers. That is a defect class this piece created, and AGENTS
//! rule 1 applies to it. Two things can go red on it and neither implies the other:
//!
//! - **`scripts/gates/web-routes-are-gated.sh`** — every `.route(` in this crate registers a
//!   path [`super::CAMERA_BEARING_PATHS`] names, so a route *nobody named* is a finding rather
//!   than a discovery. A suite can only drive the paths it knows, and the path no test knows is
//!   precisely the one nobody wrote down (`kill-is-never-a-fallback.sh`'s argument about an
//!   absence, one transport along);
//! - **`crates/daemon/tests/preview.rs`'s `every_camera_bearing_route_is_behind_the_gate`** —
//!   every path on that list answers `401` to a request presenting nothing, over a real socket,
//!   and answers something else with the token; and the assets answer `200` to the same
//!   anonymous request, which is the ruling's own requirement rather than a permission.
//!
//! Together they are the partition: every route is named, and every name is gated.
//!
//! ## The client's own subresources, which the ruling is mostly about
//!
//! Note **N76** recorded the constraint this listener used to put on the client: the token
//! rides the URL, a browser does not carry a document's query string over to the subresources
//! that document requests, so `<link rel="stylesheet" href="app.css">` on a page opened at
//! `/?token=…` was fetched with no credential at all and refused — the gate being right. That
//! is why the skeleton `webcam-handler-web` ships is one self-contained file, and why P5c's ES
//! *modules* (subresources by definition, design §2.7) could not be written until somebody
//! chose between a session cookie and a hand-rolled module loader. **The ruling dissolves that
//! question rather than answering it**: an asset request presents no credential and needs
//! none, so the client's module graph is ordinary `import` statements, and no second
//! credential shape was invented — no cookie is read, written or accepted anywhere in this
//! daemon. What still authenticates is what the page drives the camera with: [`super::rpc`]'s
//! `?token=` on the WebSocket (note **N74**'s rule, unchanged) and the same parameter on the
//! preview's `<img src>`.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::extract::Request;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use jsonrpsee_server::{Methods, ServerHandle};
use schema::{Error, Result, limits};
use tokio::net::TcpListener;
use tower_http::compression::CompressionLayer;

use super::gate;
use super::posture::{Posture, TokenRule};
use super::token::Token;
use super::{preview, rpc};
use crate::preview::Previews;
use crate::shutdown::Shutdown;

/// What a request for a path this build does not serve is told.
///
/// A sentence rather than a body, and short on purpose: the only reader is a person who typed
/// a URL by hand or a client of P5b's that is a version behind. A listing of what *does* exist
/// would be this daemon volunteering its surface to whoever asked, which is a habit worth not
/// having on the transport that carries a camera. Since the 2026-08-12 ruling this answer is
/// reachable **without** the token, which strengthens the argument rather than changing it:
/// "whoever asked" now includes somebody who has presented nothing.
const NOT_FOUND: &str = "no such asset\n";

/// The referrer policy every response this listener writes carries.
///
/// `no-referrer` and not `same-origin` or `strict-origin-when-cross-origin`, and the reason is
/// the token's. It rides the document's URL ([`Token::ready_to_open_url`]), so the URL an
/// operator's browser holds for this page **contains the key to their camera** — and a
/// `Referer` header is that URL, sent to whatever the page linked to. Same-origin leakage back
/// to this daemon is harmless, since the credential came from here; a link *out* of the page is
/// not, and it costs nothing to be right about both. The page P5a ships has no links and P5c's
/// may, which is the wrong order to discover a header in.
///
/// It is a header on the response rather than a `referrerpolicy=` attribute on an element,
/// because the attribute is one anchor's and this is the document's — and because a policy the
/// *daemon* states cannot be lost by a client edit that forgets one tag.
const REFERRER_POLICY: &str = "no-referrer";

/// Everything `--http` costs the composition root: the posture, the token, the socket, the
/// server.
///
/// The order inside is the order it has to be. The **posture is decided first**, from the
/// address the operator asked for and before anything is bound, because that is the decision
/// every other line here depends on — and because a socket that is already open is a bad place
/// to discover that it should not have been. The **token is minted second**, and only in the
/// cells that need one: a secret minted for the token-less cell would be a secret printed in a
/// URL and never checked, which is a lie an operator would act on. The **bind is third**, so a
/// `--http` naming a port somebody else holds costs nothing but the refusal.
///
/// `methods` is the T5 surface, taken as a **value** and never built here: it is the one
/// [`crate::server::mount`] produced for the Unix socket, cloned, which is D10's "one wire
/// surface, one home" and the reason [`super::rpc`] can serve a browser without declaring a
/// method name. A parameter rather than a `Wchd` for the same reason `posture` and `token`
/// are parameters — this module reads decisions, it does not make them.
///
/// [`Token::mint`]'s doc says it is called once, from the composition root, on the path that
/// opens the TCP listener. It still is: this function is that path, called from `main` and
/// from the suite that drives what `main` drives.
///
/// # Errors
///
/// Whatever [`bind`] refuses with for an address this machine will not give out, and whatever
/// [`Token::mint`] refuses with when the kernel will not produce randomness — both
/// [`Error::DeviceIo`]. A daemon that answered either by serving anyway would be serving a
/// camera on a socket it could not describe, or through a gate holding a key it never got.
pub async fn open(
    requested: SocketAddr,
    insecure_loopback_requested: bool,
    methods: Methods,
    previews: Previews,
    shutdown: Shutdown,
) -> Result<Serving> {
    let posture = Posture::of(requested, insecure_loopback_requested);
    let token = match posture.token() {
        TokenRule::Required => Some(Arc::new(Token::mint()?)),
        TokenRule::NotRequired => None,
    };
    let listener = bind(requested).await?;
    serve(listener, posture, token, methods, previews, shutdown)
}

/// Bind the address `--http` asked for.
///
/// Separate from [`open`] because binding is the step that can fail for reasons an operator
/// caused — a port in use, an address that is not on this machine, a privileged port — and
/// because it is what lets the suite drive [`serve`] on an ephemeral port with a posture of
/// its own choosing.
///
/// # Errors
///
/// [`Error::DeviceIo`] carrying the kernel's errno, which is the variant this crate already
/// uses for "this process could not perform an operation" — [`crate::shutdown::Signals::real`]
/// for a handler it could not install, [`Token::mint`] for a syscall that refused. It is not
/// [`Error::StorageIo`]: nothing here touches a path.
pub async fn bind(requested: SocketAddr) -> Result<TcpListener> {
    TcpListener::bind(requested)
        .await
        .map_err(|err| Error::DeviceIo {
            operation: format!("bind the web listener to {requested}"),
            errno: err.raw_os_error(),
            message: err.to_string(),
        })
}

/// A running web listener: where it is, what it decided, and the task serving it.
///
/// Held by the composition root for its lifetime, like [`crate::uds::Serving`]. The
/// `#[must_use]` is not decoration: a value that is dropped drops the [`tokio::task::JoinHandle`],
/// which detaches the task, which turns "the web listener ended" back into the maybe this
/// module exists to avoid.
///
/// It carries the posture and the token because the two lines `main` writes about this
/// listener are readings of them — D11's warning and D11's ready-to-open URL — and a
/// composition root that kept its own copies would be a second place where the address in the
/// URL and the address in the warning could disagree.
#[derive(Debug)]
#[must_use = "a listener nobody joins is a listener whose ending nothing waited for"]
pub struct Serving {
    bound: SocketAddr,
    posture: Posture,
    /// `Some` exactly in the cells [`Posture::token`] says are gated — [`serve`] refuses to
    /// build anything else.
    token: Option<Arc<Token>>,
    /// The WebSocket endpoint's stop, held here for the listener's life.
    ///
    /// It is jsonrpsee's `watch::Sender`, so **dropping this value stops the JSON-RPC
    /// connections** whether or not anybody asked — which is the safety net under a dropped
    /// [`Serving`], not the mechanism. The mechanism is [`Serving::stopped`], and *when* it
    /// fires is an ordering rather than a detail: see [`super::rpc`]'s header.
    ending: ServerHandle,
    serving: tokio::task::JoinHandle<()>,
}

impl Serving {
    /// The address the kernel gave this listener — the one D11 asks the daemon to report.
    ///
    /// With `--http`'s default of `127.0.0.1:0` this is where the port comes from.
    #[must_use]
    pub fn bound(&self) -> SocketAddr {
        self.bound
    }

    /// What D11's matrix decided about the address this listener was asked for.
    ///
    /// Its readers are the composition root, for [`Posture::warning`], and the suite. An
    /// accessor rather than a `warning()` of its own, because the warning has one home
    /// ([`super::posture`]) and a delegating method here would be a second place to change its
    /// wording.
    #[must_use]
    pub fn posture(&self) -> &Posture {
        &self.posture
    }

    /// The line an operator copies — D11's "printed as a ready-to-open URL".
    ///
    /// Two shapes, one per side of the matrix, and it lives here because this is the only
    /// value that holds both halves of it: the *bound* address and the token that was actually
    /// installed in the gate. A composition root building this string from `--http`'s argument
    /// would print `http://127.0.0.1:0/…`.
    ///
    /// In the token-less loopback cell it is the **plain** URL. Not a URL with an empty
    /// `?token=`, which would look like a token that failed to render, and not a second
    /// sentence explaining that this listener has no token — the daemon says that once, where
    /// the operator asked for it.
    #[must_use]
    pub fn ready_to_open_url(&self) -> String {
        match &self.token {
            Some(token) => token.ready_to_open_url(self.bound),
            None => format!("http://{bound}/", bound = self.bound),
        }
    }

    /// Wait for the server task to end.
    ///
    /// Consumes the handle, because a task is joined once and a second call would be a poll of
    /// a [`tokio::task::JoinHandle`] that has already yielded — which panics.
    /// [`crate::uds::Serving`] keeps its answer instead, and it has to: [`crate::shutdown`]'s
    /// order awaits that one twice, in a `select!` arm and then in a drain. Nothing awaits this
    /// one twice, so the type says so rather than carrying machinery for a caller that does not
    /// exist.
    ///
    /// It is a **join and not a wait**: the composition root calls it after the teardown has
    /// already cancelled the token this server watches, exactly as it does for the watchdog
    /// task.
    ///
    /// ## This is also where the WebSocket connections are ended, and that is an ordering
    ///
    /// An upgraded connection stops belonging to axum the moment hyper hands the socket over,
    /// so the graceful shutdown neither waits for one nor closes one; jsonrpsee's
    /// [`ServerHandle`] is what ends them, and it is asked **here** rather than when the
    /// daemon's token was cancelled. The difference is `crate::shutdown`'s step 3 against its
    /// step 4, on the other transport: a cancelled subscription ends with
    /// [`crate::events::SHUTTING_DOWN`] in a payload the client can branch on, and a transport
    /// stopped in the same instant closes the connection with that reason still in flight —
    /// which that module measured rather than reasoned about. By the time the composition root
    /// joins this listener the teardown has already cancelled and waited for the subscribers,
    /// so the stop below lands where step 4 lands.
    ///
    /// **A surviving mutant, recorded rather than left for a review to find.** Deleting the
    /// `stop()` below leaves every test in this workspace passing, and it is not a gap a
    /// better test would close: jsonrpsee's stop channel is a `watch::Sender`, so *dropping*
    /// [`ServerHandle`] ends the connections exactly as asking it to does — and this value is
    /// consumed here, so the drop happens a few instructions later whatever this line says.
    /// What the explicit call buys is that "the WebSocket connections end at the join" is
    /// something this code **says**, rather than a consequence of somebody else's channel
    /// shape that the next edit could remove by moving a field. The *ordering* is not in the
    /// same position: a mutant that stops the transport when the token is cancelled instead
    /// fails `web_rpc.rs`'s ending test on most runs, which is the same race
    /// `crate::shutdown`'s header measured one transport along.
    ///
    /// ## The join is **bounded**, and P5b is where that stopped being optional
    ///
    /// [`schema::limits::WEB_LISTENER_STOP_MS`], and the case it exists for was measured
    /// rather than predicted: **an open MJPEG tab whose reader has stopped reading makes a
    /// graceful shutdown wait forever, and cancelling the stream does not fix it.**
    /// `axum::serve` waits for a response that is being written; [`super::preview`]'s writer
    /// watches the same cancellation this server does and ends its body when it fires — which
    /// is necessary and is *not* sufficient, because ending a body leaves hyper a final chunk
    /// to **write**, and a client with a full socket cannot be written to. Without a bound
    /// here, `webcam-handler-daemon` stops when somebody scrolls a browser tab back into view.
    ///
    /// So on expiry the listener task is **aborted** — its sockets close with it, which is what
    /// unblocks nothing and ends everything — and the daemon says so at `warn`, naming the
    /// bound and what was still in flight (AGENTS rule 3: a bounded wait that expires is never
    /// a silence). That is the rule AGENTS states for exactly this case: open streams are
    /// "cancelled, never awaited, on shutdown".
    ///
    /// It answers `Ok` on expiry, deliberately. A stop that abandoned a stalled browser is a
    /// stop that worked; reporting it as a failure would reach `main`'s exit code and ask a
    /// service manager to restart a daemon whose only sin was that somebody left a tab open.
    ///
    /// # Errors
    ///
    /// [`Error::DeviceIo`] when the task panicked. A server that *failed* — a fatal accept
    /// error — is not reported here: it said so at `error!` at the moment it happened, and this
    /// module's header argues why it is not this daemon's exit code. Nor is the expiry above.
    pub async fn stopped(self) -> Result<()> {
        // `AlreadyStoppedError` is somebody having asked already, which is the outcome this
        // call wanted — `crate::uds::Serving::stop` discards it for the same reason.
        let _ = self.ending.stop();
        let bound = Duration::from_millis(limits::WEB_LISTENER_STOP_MS);
        // Taken before the join, because `timeout` consumes the handle and a dropped
        // `JoinHandle` *detaches* a task rather than ending it — which would leave the
        // connection this bound exists to abandon still holding its socket.
        let abandon = self.serving.abort_handle();
        match tokio::time::timeout(bound, self.serving).await {
            Ok(joined) => joined.map_err(|err| Error::DeviceIo {
                operation: "join the web listener".to_owned(),
                errno: None,
                message: err.to_string(),
            }),
            Err(_) => {
                abandon.abort();
                tracing::warn!(
                    bound_ms = limits::WEB_LISTENER_STOP_MS,
                    "the web listener still had a response in flight and was ended; \
                     a client that stopped reading cannot be written to"
                );
                Ok(())
            }
        }
    }
}

/// Serve the embedded client over `listener` until `shutdown` is cancelled.
///
/// Returns as soon as the server is spawned; the caller must already be inside a runtime.
/// Everything this function decides is in the one `match` over `posture` inside `router` —
/// see the module header for what it takes as values and why.
///
/// `token` must be `Some` exactly when `posture` requires one. That is a fact about the caller
/// rather than about a request, and it is checked rather than assumed for the reason
/// `crate::shutdown`'s unreachable `Ending` arm is written: both disagreements are ways to open
/// a socket onto a camera that nobody meant to open. A `Required` posture with no token would
/// be a gate with nothing to check; a `NotRequired` posture with a token would be a
/// composition root that minted a secret, printed it in a URL, and served the page to anyone
/// who left it out.
///
/// # Errors
///
/// [`Error::DeviceIo`] when `local_addr()` refuses — a listener that cannot say what it is
/// bound to cannot have its port reported, and D11 asks for the port — or when the posture and
/// the token disagree.
pub fn serve(
    listener: TcpListener,
    posture: Posture,
    token: Option<Arc<Token>>,
    methods: Methods,
    previews: Previews,
    shutdown: Shutdown,
) -> Result<Serving> {
    let bound = listener.local_addr().map_err(|err| Error::DeviceIo {
        operation: "read the address the web listener was bound to".to_owned(),
        errno: err.raw_os_error(),
        message: err.to_string(),
    })?;
    let wire = rpc::mount(methods);
    let ending = wire.ending.clone();
    let router = router(posture, token.clone(), wire.route, preview::mount(previews))?;

    let serving = tokio::spawn(async move {
        let served = axum::serve(listener, router)
            .with_graceful_shutdown(async move { shutdown.cancelled().await })
            .await;
        if let Err(err) = served {
            // Reported here, at the instant it happens, and not carried out to the join: a
            // listener that stopped accepting at 03:00 must not first be mentioned by a
            // teardown at 09:00. See this module's header for why it is not an exit code.
            tracing::error!(
                error = %err,
                address = %bound,
                "the web listener stopped serving; the Unix socket is unaffected"
            );
        }
    });

    Ok(Serving {
        bound,
        posture,
        token,
        ending,
        serving,
    })
}

/// The routes, the compression layer over the half that wants one, D11's gate over the routes
/// or over nothing, and the referrer policy over everything.
///
/// One `fallback` for the assets and no route table for them: every path that is not the
/// wire's or the preview's is answered by the same handler, which serves the asset of that
/// name or refuses with `404`. A table of asset routes would be a second list of the client's
/// files — one in `webcam-handler-web`'s `assets/` and one here — and the second copy is the
/// one that stops being true when P5c adds a module.
///
/// `wire` is [`super::rpc`]'s single route and `preview` is [`super::preview`]'s, merged
/// rather than declared here, so this function still holds no opinion about what either of
/// them does — see those modules' headers. They are *routes* and the assets are the
/// *fallback*, so the two paths that are endpoints take precedence over the one that is a name
/// in a table; a request for `/rpc` therefore never reaches `web::get`, which is a property
/// this daemon gets from the router rather than from an asset that happens not to be called
/// `rpc`.
///
/// **That split is also the security boundary now**, which is worth saying where the two lines
/// are: since the owner's 2026-08-12 ruling the gate is over the *routes* and not over the
/// *fallback*, so "is this a route or a file?" and "does this need the token?" are the same
/// question. Adding a route is therefore a security decision whatever else it is
/// (`scripts/gates/web-routes-are-gated.sh`), and adding a file is not.
///
/// ## The compression layer, and the route it is deliberately not over
///
/// `tower_http::CompressionLayer` is applied to **the assets and the wire**, and the preview
/// is merged in afterwards, so the preview's service is not inside it. That is the exclusion
/// docs/7 P5b asks for, made by composition: there is no predicate to invert, no content-type
/// list to keep current, and a mutant that puts the preview under the layer has to *move a
/// line* rather than flip a boolean. `CompressionLayer`'s own default predicate would compress
/// the preview — it declines `image/*`, and `multipart/x-mixed-replace` is not `image/*` —
/// which is why leaving it to a default would be leaving it to the wrong answer.
///
/// It is over the assets because that is what compresses: the client is HTML, CSS and ES
/// modules (design §2.7), which is the traffic gzip was designed for. It is over the wire
/// route because jsonrpsee's HTTP answers are JSON, and a `wch_controls` listing is the
/// largest thing this daemon puts on that socket; the upgrade that shares the path is a `101`
/// with no body, which the layer passes through untouched.
///
/// ## The gate, and the one word that carries the owner's ruling
///
/// `Router::route_layer` maps over `path_router` and **neither** fallback, so the gate covers
/// the WebSocket upgrade and the preview — the two routes — and does not cover a request for a
/// path that is not one of them. That is the 2026-08-12 ruling in one word: the assets are
/// served to anybody, and what stays behind D11's token is what carries or drives the camera.
/// `Router::layer` is what this was, note **N75** argued for it at length, and the module
/// header records why that argument inverted rather than merely lapsed.
///
/// **Nothing may be merged into this router after that line.** `route_layer` wraps the routes
/// that exist when it is called and says nothing about later ones, which is the defect class
/// the header names and `scripts/gates/web-routes-are-gated.sh` answers. It *panics* on a
/// router with no routes at all — axum refusing to install a layer that could not run — which
/// is the one arrangement in which this call would silently do nothing.
///
/// ## The referrer policy, over everything and outermost
///
/// [`REFERRER_POLICY`] is applied with `Router::layer` and applied **last**, so it is the
/// outermost middleware and lands on every response this listener writes — the page, the
/// assets, the preview's frames, the `404`, and the gate's own `401`, which is a response to a
/// request whose URL may have carried the token in the first place. A header that covered only
/// the half of the router somebody remembered would be the second list this module spent the
/// paragraph above refusing to keep.
fn router(
    posture: Posture,
    token: Option<Arc<Token>>,
    wire: Router,
    preview: Router,
) -> Result<Router> {
    let routes = Router::new()
        .merge(wire)
        .fallback(asset)
        .layer(CompressionLayer::new())
        .merge(preview);
    let served = match (posture.token(), token) {
        // D11's three gated cells: loopback without the flag, and both non-loopback cells.
        (TokenRule::Required, Some(token)) => {
            routes.route_layer(axum::middleware::from_fn_with_state(token, gate::check))
        }
        // D11's one token-less cell — loopback, and only behind the named flag. No gate,
        // rather than a gate that says yes.
        (TokenRule::NotRequired, None) => routes,
        (TokenRule::Required, None) => {
            return Err(ungated(
                "D11 requires the bearer token for this bind and none was minted, so the gate \
                 would have nothing to check",
            ));
        }
        (TokenRule::NotRequired, Some(_)) => {
            return Err(ungated(
                "a token was minted for a bind D11 serves without one, so it would be printed \
                 in a URL and never checked",
            ));
        }
    };
    Ok(served.layer(axum::middleware::map_response(no_referrer)))
}

/// Stamp [`REFERRER_POLICY`] on one response.
///
/// `insert` rather than `append`: this is the daemon's policy for its own responses, and two
/// `Referrer-Policy` headers is a browser picking one of them — the same "whichever layer
/// parsed it last" question [`super::gate`] refuses to have about a credential, in a place
/// where the answer is only a policy. Nothing else in this daemon writes the header, so the
/// value replaced is always this one.
async fn no_referrer(mut response: Response) -> Response {
    response.headers_mut().insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static(REFERRER_POLICY),
    );
    response
}

/// The refusal both halves of that disagreement share.
///
/// [`Error::DeviceIo`] for [`crate::server::mount`]'s reason: it is *this process* failing to
/// compose itself, which must never be spelled like a device declining something (E3, AGENTS
/// rule 7). `errno` is `None` because no syscall was involved — the field carries the kernel's
/// answer when there is one, and inventing a number would be worse than the empty option D13
/// provides for exactly this.
fn ungated(disagreement: &str) -> Error {
    Error::DeviceIo {
        operation: "open the web listener".to_owned(),
        errno: None,
        message: format!("refusing to serve the web client: {disagreement}"),
    }
}

/// One asset, or a `404`.
///
/// Every method, deliberately: nothing the *asset* half serves changes anything, so `GET` and
/// `POST` differ in nothing an operator could observe, and a `405` surface here would be
/// routing policy invented for a fallback that has no verbs. Where methods do mean something
/// is [`super::rpc`]'s route, and the answer there is jsonrpsee's rather than this file's.
/// `HEAD` needs no special case — hyper omits the body of a response to one.
async fn asset(request: Request) -> Response {
    match lookup(request.uri().path()) {
        Some(asset) => (
            [(header::CONTENT_TYPE, asset.content_type())],
            // One copy of a file measured in kilobytes, per request. The alternative is
            // threading `webcam-handler-web`'s `Cow<'static, [u8]>` out through this signature
            // to build a body that borrows the binary — which buys a branch whose `Owned` arm
            // no build of this daemon can reach, since the embed is unconditional (that
            // crate's manifest argues `debug-embed`).
            asset.bytes().to_vec(),
        )
            .into_response(),
        None => (StatusCode::NOT_FOUND, NOT_FOUND).into_response(),
    }
}

/// The asset a request path names, if any.
///
/// The **one** transformation between an HTTP path and an asset key: strip the single leading
/// `/` that every request target has, and read the empty remainder as the index page —
/// `web::INDEX`, spelled there rather than here, because which file `/` means is a fact about
/// the client. No normalization, no `..` handling, no case folding: the lookup is a match
/// against names fixed at compile time, so a path that is not one of them has no answer to
/// give (that crate's `get` states it).
fn lookup(path: &str) -> Option<web::Asset> {
    let relative = path.strip_prefix('/')?;
    if relative.is_empty() {
        web::get(web::INDEX)
    } else {
        web::get(relative)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An address string this file wrote, as a value.
    fn address(text: &str) -> SocketAddr {
        text.parse().expect("a socket address the tests wrote")
    }

    /// A wire surface with nothing on it.
    ///
    /// This module composes a listener; *what the listener serves over the wire* is
    /// [`super::rpc`]'s claim and `crates/daemon/tests/web_rpc.rs`'s, over a real `Wchd` and
    /// the registration `crate::server::mount` produced. An empty registration here is
    /// therefore honest rather than lazy: every assertion below is about the posture, the
    /// token, the URL or the asset table, and none of them would be made stronger by a
    /// surface with methods on it.
    fn no_methods() -> Methods {
        Methods::new()
    }

    /// A preview fan-out over a backend that replays no cameras.
    ///
    /// Honest for the same reason [`no_methods`] is: every assertion in this module is about
    /// the posture, the token, the URL or the asset table, and none of them would be made
    /// stronger by a fan-out with a camera behind it. What the preview route *does* is
    /// `crate::http::preview`'s claim and `crates/daemon/tests/preview.rs`'s, over a real
    /// `Wchd` and the fake backend.
    fn no_cameras() -> Previews {
        Previews::new(
            Arc::new(engine::actor::Cameras::new(Arc::new(
                fake::FakeBackend::new(Vec::new()).expect("a backend replaying no cameras"),
            ))),
            engine::settle::MonotonicClock::new(),
            Shutdown::new(),
        )
    }

    #[test]
    fn the_index_page_is_what_the_root_path_means() {
        // The one path D11's ready-to-open URL points at. Both spellings answer the same file,
        // and the name comes from the asset crate rather than from a literal here.
        let root = lookup("/").expect("the root path serves the client's entry point");
        let named = lookup("/index.html").expect("and so does the file's own name");

        assert_eq!(root.bytes(), named.bytes());
        assert_eq!(root.content_type(), "text/html; charset=utf-8");
    }

    #[test]
    fn a_path_that_names_no_asset_has_no_answer() {
        // Including the shapes a hostile request takes. None of them is caught by a check here
        // — there is nothing to catch, because the table of names is fixed at compile time —
        // and that is the claim being pinned.
        for path in [
            "/nothing-here",
            "/../../etc/passwd",
            "/index.html/",
            "//index.html",
            "index.html",
        ] {
            assert!(lookup(path).is_none(), "{path} served something");
        }
    }

    #[test]
    fn a_posture_and_a_token_that_disagree_do_not_get_a_router() {
        // Neither disagreement can come from `open`, and both are refused rather than resolved,
        // because both resolutions are a socket onto a camera that somebody did not mean to
        // open. Both directions, so a build that dropped one arm goes red here.
        let bind = address("127.0.0.1:0");
        let token = Arc::new(Token::mint().expect("the kernel has a CSPRNG"));

        let gated_without_a_token = router(Posture::of(bind, false), None, wire(), preview())
            .expect_err("a gate with nothing to check is not a gate");
        assert_eq!(gated_without_a_token.kind(), schema::ErrorKind::DeviceIo);

        let open_with_a_token = router(
            Posture::of(bind, true),
            Some(Arc::clone(&token)),
            wire(),
            preview(),
        )
        .expect_err("a token that is never checked is a URL that lies");
        assert_eq!(open_with_a_token.kind(), schema::ErrorKind::DeviceIo);

        // ... and the two agreeing arrangements do get one, which is what makes the assertions
        // above about the disagreement rather than about `router` refusing everything.
        assert!(
            router(Posture::of(bind, false), Some(token), wire(), preview()).is_ok(),
            "D11's default cell"
        );
        assert!(
            router(Posture::of(bind, true), None, wire(), preview()).is_ok(),
            "D11's token-less cell"
        );
    }

    /// The wire route, mounted over an empty surface, as the parameter [`router`] takes.
    ///
    /// The [`rpc::Mounted::ending`] handle is dropped with the value, which stops a server
    /// nothing ever started serving on — the routers built here are never handed to
    /// `axum::serve`.
    fn wire() -> Router {
        rpc::mount(no_methods()).route
    }

    /// The preview route, over a fan-out with no cameras behind it.
    fn preview() -> Router {
        super::preview::mount(no_cameras())
    }

    #[tokio::test]
    async fn the_url_is_built_from_the_bound_address_and_carries_the_token() {
        // D11's "default `127.0.0.1:0` → report the bound port", as the property of the one
        // line an operator copies. Port zero is a request and never an answer, so a URL
        // carrying it is a URL nobody can open.
        let shutdown = Shutdown::new();
        let web = open(
            address("127.0.0.1:0"),
            false,
            no_methods(),
            no_cameras(),
            shutdown.clone(),
        )
        .await
        .expect("loopback on an ephemeral port");

        let bound = web.bound();
        assert_ne!(bound.port(), 0, "the requested port reached the listener");
        let url = web.ready_to_open_url();
        assert!(url.starts_with(&format!("http://{bound}/?")), "{url}");
        assert!(url.contains(&bound.port().to_string()), "{url}");

        // Ended the way the daemon ends it, and joined — a test that dropped the handle would
        // leave behind exactly the detached task this type's `must_use` is about.
        shutdown.cancel();
        web.stopped().await.expect("the server task ended");
    }

    #[tokio::test]
    async fn the_token_less_cell_prints_a_plain_url_and_no_warning() {
        // D11's second cell, through the function `main` calls. The URL has no `?token=` in
        // it — an empty one would read as a token that failed to render — and the posture asks
        // for no warning, because loopback is not what D11's warning is about.
        let shutdown = Shutdown::new();
        let web = open(
            address("127.0.0.1:0"),
            true,
            no_methods(),
            no_cameras(),
            shutdown.clone(),
        )
        .await
        .expect("loopback on an ephemeral port");

        let url = web.ready_to_open_url();
        assert_eq!(url, format!("http://{bound}/", bound = web.bound()));
        assert!(!url.contains("token"), "{url}");
        assert_eq!(web.posture().warning(), None);

        shutdown.cancel();
        web.stopped().await.expect("the server task ended");
    }
}
