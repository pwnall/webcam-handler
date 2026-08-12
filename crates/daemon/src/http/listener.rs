//! The opt-in TCP transport: one axum server, the embedded client behind it, and the token
//! gate in front of everything (D11, docs/7 P5a).
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
//! ## Stopping: what is **not** claimed
//!
//! **No bound on an in-flight response.** Everything this build serves through the asset
//! fallback is a file of a few kilobytes, so the graceful stop is bounded by a `write` to a
//! socket; that is a property of what is served, not a guarantee this module provides. The
//! response that does not end on its own is still the MJPEG preview,
//! `multipart/x-mixed-replace`, which by construction runs until the client goes away, and
//! design §2.6 states the requirement it brings ("an open MJPEG tab must not hang shutdown").
//! Meeting it needs the preview's own stream to watch the cancellation token, and it is
//! deliberately not pre-built here: a bound written now would be a bound with nothing to bound
//! (rubric A8).
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
//! ## The gate is installed, or it is absent
//!
//! In D11's three token-gated cells the router is wrapped in [`super::gate::check`]; in the
//! token-less loopback cell it is **not wrapped at all**. The alternative — always install the
//! middleware and let it answer "yes" when the posture says so — puts a bypass branch inside
//! the one function in this daemon whose entire job is to say no, where an inverted condition
//! is a listener serving a live camera to anybody who asks. Here the branch is at composition,
//! in one `match` over the posture, and the gate itself has no way to admit a request that did
//! not present the token.
//!
//! **Every route, and since P5b that includes the WebSocket upgrade.** An upgrade request is
//! an ordinary HTTP request with two headers on it, so it meets the gate before it meets the
//! router, and an anonymous `new WebSocket("ws://…/rpc")` is answered with the same `401` an
//! anonymous `GET /` is. That is not a second decision — it is `router` wrapping one thing
//! instead of a list — and it is asserted over a real socket rather than inferred from the
//! composition (`crates/daemon/tests/web_rpc.rs`).
//!
//! ## The client's own subresources, which this build's page does not have
//!
//! A finding worth writing down where the gate is, because it is P5c's to solve: **the
//! token rides the URL, and a browser does not carry a document's query string over to the
//! subresources that document requests.** A page opened at `/?token=…` asks for `/app.css` —
//! no query, no `Authorization`, no credential — and this gate refuses it, correctly. So the
//! skeleton `webcam-handler-web` ships is a single self-contained file, and the real client
//! (vanilla ES *modules*, which are subresources by definition, design §2.7) will need a
//! decision made on purpose: a cookie set on the gated navigation, or a page that fetches its
//! own modules with the `Authorization` header. Nothing here prejudges it, and P5b did not
//! prejudge it either: the WebSocket endpoint authenticates with the `?token=` form the gate
//! already reads (note **N74**), because that is what a `new WebSocket(url)` can carry, and no
//! cookie is read, written or accepted anywhere in this daemon. [`super::rpc`]'s header
//! records what that form costs on a socket rather than on a navigation, and the one fact
//! that endpoint contributes to note N76's open question.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
use axum::extract::Request;
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use jsonrpsee_server::{Methods, ServerHandle};
use schema::{Error, Result};
use tokio::net::TcpListener;

use super::gate;
use super::posture::{Posture, TokenRule};
use super::rpc;
use super::token::Token;
use crate::shutdown::Shutdown;

/// What a request for a path this build does not serve is told.
///
/// A sentence rather than a body, and short on purpose: the only reader is a person who typed
/// a URL by hand or a client of P5b's that is a version behind. A listing of what *does* exist
/// would be this daemon volunteering its surface to whoever asked, which is a habit worth not
/// having on the transport that carries a camera.
const NOT_FOUND: &str = "no such asset\n";

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
    shutdown: Shutdown,
) -> Result<Serving> {
    let posture = Posture::of(requested, insecure_loopback_requested);
    let token = match posture.token() {
        TokenRule::Required => Some(Arc::new(Token::mint()?)),
        TokenRule::NotRequired => None,
    };
    let listener = bind(requested).await?;
    serve(listener, posture, token, methods, shutdown)
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
    /// # Errors
    ///
    /// [`Error::DeviceIo`] when the task panicked or was aborted. A server that *failed* — a
    /// fatal accept error — is not reported here: it said so at `error!` at the moment it
    /// happened, and this module's header argues why it is not this daemon's exit code.
    pub async fn stopped(self) -> Result<()> {
        // `AlreadyStoppedError` is somebody having asked already, which is the outcome this
        // call wanted — `crate::uds::Serving::stop` discards it for the same reason.
        let _ = self.ending.stop();
        self.serving.await.map_err(|err| Error::DeviceIo {
            operation: "join the web listener".to_owned(),
            errno: None,
            message: err.to_string(),
        })
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
    shutdown: Shutdown,
) -> Result<Serving> {
    let bound = listener.local_addr().map_err(|err| Error::DeviceIo {
        operation: "read the address the web listener was bound to".to_owned(),
        errno: err.raw_os_error(),
        message: err.to_string(),
    })?;
    let wire = rpc::mount(methods);
    let ending = wire.ending.clone();
    let router = router(posture, token.clone(), wire.route)?;

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

/// The routes, and D11's gate over them or not at all.
///
/// One `fallback` for the assets and no route table for them: every path that is not the
/// wire's is answered by the same handler, which serves the asset of that name or refuses
/// with `404`. A table of asset routes would be a second list of the client's files — one in
/// `webcam-handler-web`'s `assets/` and one here — and the second copy is the one that stops
/// being true when P5c adds a module.
///
/// `wire` is [`super::rpc`]'s single route, merged rather than declared here, so this function
/// still holds no opinion about what the wire surface is — see that module's header for what
/// mounting it does and does not cost. It is a *route* and the assets are the *fallback*, so
/// the one path that is an endpoint takes precedence over the one that is a name in a table;
/// a request for `/rpc` therefore never reaches `web::get`, which is a property this daemon
/// gets from the router rather than from an asset that happens not to be called `rpc`.
///
/// `Router::layer` wraps the fallback as well as the routes (it maps over `path_router`,
/// `fallback_router` **and** `catch_all_fallback`), which is why the gate covers a request for
/// a path that does not exist **and** the WebSocket upgrade. `route_layer` is the one that
/// would not, and it is the wrong tool here for exactly that reason: it would leave an
/// anonymous request for `/anything` answered by the 404 handler, telling a stranger which
/// paths this daemon has.
fn router(posture: Posture, token: Option<Arc<Token>>, wire: Router) -> Result<Router> {
    let routes = Router::new().merge(wire).fallback(asset);
    match (posture.token(), token) {
        // D11's three gated cells: loopback without the flag, and both non-loopback cells.
        (TokenRule::Required, Some(token)) => {
            Ok(routes.layer(axum::middleware::from_fn_with_state(token, gate::check)))
        }
        // D11's one token-less cell — loopback, and only behind the named flag. No gate,
        // rather than a gate that says yes.
        (TokenRule::NotRequired, None) => Ok(routes),
        (TokenRule::Required, None) => Err(ungated(
            "D11 requires the bearer token for this bind and none was minted, so the gate \
             would have nothing to check",
        )),
        (TokenRule::NotRequired, Some(_)) => Err(ungated(
            "a token was minted for a bind D11 serves without one, so it would be printed in \
             a URL and never checked",
        )),
    }
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

        let gated_without_a_token = router(Posture::of(bind, false), None, wire())
            .expect_err("a gate with nothing to check is not a gate");
        assert_eq!(gated_without_a_token.kind(), schema::ErrorKind::DeviceIo);

        let open_with_a_token = router(Posture::of(bind, true), Some(Arc::clone(&token)), wire())
            .expect_err("a token that is never checked is a URL that lies");
        assert_eq!(open_with_a_token.kind(), schema::ErrorKind::DeviceIo);

        // ... and the two agreeing arrangements do get one, which is what makes the assertions
        // above about the disagreement rather than about `router` refusing everything.
        assert!(
            router(Posture::of(bind, false), Some(token), wire()).is_ok(),
            "D11's default cell"
        );
        assert!(
            router(Posture::of(bind, true), None, wire()).is_ok(),
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
        let web = open(address("127.0.0.1:0"), true, no_methods(), shutdown.clone())
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
