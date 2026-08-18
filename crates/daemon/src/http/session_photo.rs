//! The calibration sample route: one stored photograph, named by *reference* and never by path
//! (design **D20**, **D9**, **D11**; docs/13 P9b).
//!
//! [`super::preview`] is this module's sibling and the two are the daemon's only routes whose
//! response body is a camera frame. That one serves the frame the camera is producing **now**;
//! this one serves a frame the camera produced during a sweep and the session store wrote down.
//! The difference in tense is the whole of the difference in mechanism — there is no fan-out
//! here, no viewer slot and no device — and it changes nothing at all about the posture, which
//! is the point of the paragraph below.
//!
//! ## This route is gated because it carries camera frames
//!
//! **By definition, and it is the first route to join the list since the ruling that made the
//! list a decision.** The owner ruled (2026-08-12) that static assets are served without
//! authentication — they are open-source code, not a secret — and that only the resources which
//! carry or drive the camera stay behind D11's bearer token (note **N82**). A calibration sample
//! is a photograph of whatever the camera was pointed at, kept on the operator's disk for as
//! long as they keep the session (`engine::store::STATE_DIR_MODE`'s doc argues the same fact one
//! layer down, which is why that tree is 0700). AGENTS' sentence — "a frame may contain a
//! person" — is about this file as much as about a live preview, so [`SESSION_PHOTO_PATH`] is on
//! [`super::CAMERA_BEARING_PATHS`] and this module's own test asserts that it is.
//!
//! D20 says the same thing and says why the pair had to go red on it first: this route's
//! addition "is deliberately the first exercise of the defect class N82's ruling created, with
//! both halves going red on a route added without its gate
//! (`web-routes-are-gated.sh`, `every_camera_bearing_route_is_behind_the_gate`) before the route
//! lands". Both halves cover it — the predicate reads this registration's path out of the source
//! and requires it to be on the list, and `crates/daemon/tests/preview.rs` drives the list over a
//! real socket and requires the refusal.
//!
//! ## By reference, which is what makes the traversal question empty
//!
//! The caller names **a session, a control, a sweep pass and a value**. It never names a path,
//! and no byte of what it sends is ever joined onto a directory:
//!
//! - the **session** is a [`Uuid`], compared for equality against the ids
//!   [`SessionStore::all_sessions`] read out of the tree's own directory names — so the session
//!   directory this route reaches is one the store enumerated, not one a request spelled;
//! - the **control** and the **pass** go through [`SessionStore::photo_dir_rel`], which is D9's
//!   layout rule and runs the slug transform over its argument (`engine::store`'s header:
//!   "a `task_slug` of `../../..` would otherwise name a directory outside the session tree").
//!   `../../etc` slugs to `etc`, `..` slugs to nothing at all and is refused;
//! - the **file name** comes from the sample's own recorded `photo` field and is checked to be
//!   exactly what that derived directory plus one ordinary component makes — see this module's
//!   `stored`, which is the whole of the derivation.
//!
//! So there is no arrangement of `../`, `%2e%2e`, a leading `/` or a percent-encoded anything
//! that reaches a file: the worst a caller can do is name a reference nothing answers to, which
//! is a `404`. The session document is *also* a file an operator can hand-edit (E4's lesson, and
//! `engine::store`'s header says so in as many words), which is why the recorded path is
//! **checked against the derived one** rather than trusted for being ours.
//!
//! ## Two statuses for a refusal, and the third for this machine
//!
//! [`super::preview`]'s header argues why an `<img>` gets a projection rather than a registry,
//! and this route has the same consumer: D20's sample grid is `<img src="/session-photo?…">`,
//! which branches on nothing but the status. So the vocabulary is `400` for a request that names
//! no reference at all, `404` for a reference that names no sample this store holds, and `503`
//! for a store or a filesystem that refused — the same three-way split, minus the device, plus
//! one deliberate difference in the **body**.
//!
//! **A refusal here carries no rendering of the error**, where the preview's carries D13's.
//! Every D13 variant that reaches the preview renders a device node, a camera name or an errno;
//! the variants that reach *this* route render a path inside the operator's state directory —
//! their home directory, the camera's fingerprint slug and the task they typed. That is not a
//! frame, and it is still more than a `<img>` needs in order to fire `onerror`. The detail goes
//! to the daemon's log, where the operator is, and the response says which of the three things
//! happened and nothing else.

use std::sync::Arc;

use axum::Router;
use axum::extract::{Request, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use camino::Utf8PathBuf;
use engine::store::SessionStore;
use schema::capture::PhotoFormat;
use schema::control::ControlSlug;
use schema::session::Sample;
use uuid::Uuid;

/// The path the stored calibration samples answer on.
///
/// One spelling, with three readers that must agree — the route below, this crate's suites, and
/// D20's sample grid, which builds `/session-photo?session=…&control=…&pass=…&value=…&token=…`
/// from the URL its page was opened at. It is also one of [`super::CAMERA_BEARING_PATHS`], which
/// is the list that says this route stays gated whatever a future router looks like.
pub const SESSION_PHOTO_PATH: &str = "/session-photo";

/// The query parameter naming the session: its UUIDv7, exactly as `wch_calibrate_list` reports
/// it and exactly as the session's own directory is named.
pub const SESSION_QUERY_PARAM: &str = "session";

/// The query parameter naming the control: its D2 slug, as every other surface spells it.
pub const CONTROL_QUERY_PARAM: &str = "control";

/// The query parameter naming the sweep pass.
///
/// D9's `photos/<control>/<pass>/<value>.<ext>` carries the pass because a control's history is
/// longer than one sweep: it is "the number of samples the control already carried when this
/// sweep began" ([`SessionStore::photo_dir_rel`]), so a refinement pass never overwrites the
/// coarse pass's evidence (note N22). A grid that left it out would be asking for one of two
/// pictures without saying which.
pub const PASS_QUERY_PARAM: &str = "pass";

/// The query parameter naming the sample within its pass: the **requested** value.
///
/// Requested and not applied, because that is the one of D3's pair that names a file.
/// `engine::calibrate`'s header states the reason: "the plan's values are deduplicated, so a
/// requested value is unique within a sweep; an applied value is not" — two requests a driver
/// collapses onto one applied value are two samples with two photographs, and keying on the
/// applied value would serve the second of them for both.
pub const VALUE_QUERY_PARAM: &str = "value";

/// What a request that named no reference is told.
///
/// A sentence naming the four parameters, for the reader who is a person with a URL bar or a
/// client one version behind. It lists no session and no control — this daemon does not
/// volunteer an inventory of the operator's calibration sessions to whoever asked, which is
/// [`super::preview`]'s `NO_CAMERA` argument about a different inventory.
const NO_REFERENCE: &str = "name a sample: \
                            /session-photo?session=<uuid>&control=<slug>&pass=<n>&value=<n>\n";

/// What a reference that resolves to nothing is told.
const NO_SUCH_SAMPLE: &str = "no sample in that session answers to that control, pass and value\n";

/// What a caller is told when the store or the filesystem refused.
///
/// Fixed, and the reason is this module's header: the D13 variants reachable from here render a
/// path inside the operator's state directory. The detail is logged where the operator is.
const UNREADABLE: &str = "that sample could not be read; see the daemon's log\n";

/// One part of a body with no exact size hint — `super::preview`'s `Part`, for [`described`]'s
/// reason and not for a stream's.
type Part = Result<Vec<u8>, std::convert::Infallible>;

/// Mount the sample route over `store`.
///
/// Returns a `Router` for `super::listener::router` to merge, in [`super::preview::mount`]'s
/// shape and for its reason: this module owns what its route does and owns nothing about where
/// the route sits. No gate is applied here — which routes are behind D11's token is one decision
/// in one `match` (design §2.10) — and no compression layer, because what this serves is already
/// a compressed image.
///
/// `get` and `head`, and the `head` is not a formality: [`super::preview::mount`]'s doc records
/// what note **N179** cost on the sibling route, where `axum::routing::get` answered a `HEAD`
/// out of the `get` slot and ran the whole handler for a request that asked for no bytes. Here
/// that would be a session tree walked, a document parsed and a photograph read into memory for
/// a body hyper discards. [`described`] answers from the route's own knowledge instead and
/// **opens nothing** — its signature is the proof, because it takes no [`State`] at all.
pub(super) fn mount(store: Arc<SessionStore>) -> Router {
    Router::new()
        .route(SESSION_PHOTO_PATH, get(sample).head(described))
        .with_state(store)
}

/// One stored sample, or the refusal that stopped it being found.
///
/// The store walk, the document parse and the file read all happen on
/// [`tokio::task::spawn_blocking`], which is `crate::server`'s own rule for this work: "the
/// session store need no open camera and hold nothing between calls, so they go to
/// `tokio::task::spawn_blocking` — a blocking pool thread rather than a runtime worker".
/// A route that read a photograph on a runtime worker would park it on a disk.
async fn sample(State(store): State<Arc<SessionStore>>, request: Request) -> Response {
    let asked = match Reference::of(request.uri().query()) {
        Ok(asked) => asked,
        Err(why) => return no_reference(&why),
    };
    let found = tokio::task::spawn_blocking(move || stored(&store, &asked)).await;
    match found {
        Ok(Ok(found)) => found.into_response(),
        Ok(Err(why)) => unresolved(&why),
        // The blocking pool refused or the task panicked. It is this daemon's state rather than
        // the caller's reference, so it is the `503` arm and not the `404` one (AGENTS rule 7:
        // availability is not capability).
        Err(err) => {
            tracing::warn!(error = %err, "a stored calibration sample could not be looked up");
            (StatusCode::SERVICE_UNAVAILABLE, UNREADABLE).into_response()
        }
    }
}

/// What a `HEAD` of this route is told: that the route is here, and nothing whatever about a
/// sample.
///
/// **The [`super::preview::described`] precedent, applied to a store instead of to a device**
/// (note **N179**). RFC 9110 §9.3.2 asks a server to send the header fields a `GET` would have
/// sent and lets it omit the ones "for which a value is determined only while generating the
/// content" — and on this route almost everything is one of those. The `Content-Type` is the
/// *sample's* extension, the `Content-Length` is the file's size, and the **status** itself is
/// what the store says: [`stored`] learns whether the reference resolves by walking the session
/// tree and parsing a document, which is exactly the work this endpoint exists not to do for a
/// request that asked for no bytes.
///
/// **So a `200` here is a statement about the route and never about a sample.** What this can
/// decide is [`Reference::of`] and no more: a request carrying none of the four parameters is
/// the `400` a `GET` would give, a parameter that is no spelling of what it names is the `404` a
/// `GET` would give, and **every well-formed reference answers `200`** — including one naming a
/// session that does not exist, which a `GET` refuses with `404`. A caller that needs to know
/// whether a sample is there asks for it: the `GET`'s status *is* the store's answer, and
/// `wch_calibrate_status` is the wire surface's.
///
/// The `Cache-Control` is sent because it is a fact about this route rather than about a file,
/// and because a `HEAD` that advertised a cacheable answer would be describing a response this
/// route never sends.
async fn described(request: Request) -> Response {
    match Reference::of(request.uri().query()) {
        // A body with no exact size hint, and not an empty one — `axum::routing::Route`'s future
        // sets `Content-Length` from the body's size hint *before* it strips the body for a
        // `HEAD`, so an empty body would put `content-length: 0` on this answer. RFC 9110 §8.6
        // permits that header here only when it equals what a `GET` would have sent, which is a
        // number this endpoint deliberately does not go and look up.
        Ok(_) => (
            [(header::CACHE_CONTROL, HeaderValue::from_static("no-store"))],
            axum::body::Body::from_stream(tokio_stream::empty::<Part>()),
        )
            .into_response(),
        Err(why) => no_reference(&why),
    }
}

/// The four things a request has to say, read from the query string alone.
///
/// A value with a parser of its own so both methods answer the same question the same way, and
/// so the whole of "what may a caller name" is assertable without a socket, a store or a runtime
/// (AGENTS: "pure cores take values"). Nothing here touches the filesystem, and nothing that
/// comes out of here is a path.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Reference {
    session: Uuid,
    control: ControlSlug,
    pass: usize,
    value: i64,
}

impl Reference {
    /// The reference this request names, or why it names none.
    ///
    /// The query grammar is the one [`super::preview::parameter`] reads, which is the one
    /// [`super::gate`] reads: split on `&`, then on the first `=`. A third *reader* of it rather
    /// than a third rule — the gate answers "which credentials does this request present", the
    /// preview answers "which camera", and this answers "which sample".
    fn of(query: Option<&str>) -> Result<Reference, Unnamed> {
        let session = named(query, SESSION_QUERY_PARAM)?;
        let control = named(query, CONTROL_QUERY_PARAM)?;
        let pass = named(query, PASS_QUERY_PARAM)?;
        let value = named(query, VALUE_QUERY_PARAM)?;
        Ok(Reference {
            session: Uuid::parse_str(&session)
                .map_err(|_| Unnamed::NotAValue(SESSION_QUERY_PARAM))?,
            // `ControlSlug::parse` takes any non-empty string by design — "a slug that matches no
            // control is a `ControlUnknown` at lookup time, which is a better error than a parse
            // failure" — and this route keeps that, because the lookup below is where a slug
            // naming nothing becomes a `404`. What stops it naming a *directory* is
            // `SessionStore::photo_dir_rel`'s slug transform, not a refusal here.
            control: ControlSlug::parse(&control).ok_or(Unnamed::NotAValue(CONTROL_QUERY_PARAM))?,
            pass: pass
                .parse::<usize>()
                .map_err(|_| Unnamed::NotAValue(PASS_QUERY_PARAM))?,
            value: value
                .parse::<i64>()
                .map_err(|_| Unnamed::NotAValue(VALUE_QUERY_PARAM))?,
        })
    }
}

/// One parameter's value, or the refusal for a request that left it out.
fn named(query: Option<&str>, name: &'static str) -> Result<String, Unnamed> {
    super::preview::parameter(query, name).ok_or(Unnamed::NotAsked(name))
}

/// Why a request names no sample this route could look for.
///
/// A value rather than a rendered [`Response`], so [`Reference::of`] stays a pure function over
/// a query string that both handlers can be asserted through without a socket — and so the
/// rendering stays in one place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Unnamed {
    /// The request carried no parameter of this name at all.
    NotAsked(&'static str),
    /// It carried one whose value is no spelling of what the name means.
    NotAValue(&'static str),
}

/// What a request that named no sample is answered with.
///
/// `400` for a request that left a parameter out — the caller has to name a sample, and this
/// route will not list the ones this machine holds ([`NO_REFERENCE`] argues that). `404` for a
/// value that is not a value, because a `pass` of `later` and a `session` of `../..` name no
/// sample in exactly the way a well-formed reference to a sample nobody took does, and a client
/// fixes both by asking for something else. It is [`super::preview::no_camera`]'s split, on
/// four parameters instead of one.
fn no_reference(why: &Unnamed) -> Response {
    match why {
        Unnamed::NotAsked(_) => (StatusCode::BAD_REQUEST, NO_REFERENCE).into_response(),
        Unnamed::NotAValue(_) => (StatusCode::NOT_FOUND, NO_SUCH_SAMPLE).into_response(),
    }
}

/// Why a reference that parsed still names no sample.
#[derive(Debug)]
enum Unresolved {
    /// Nothing in this store answers to it. Every step of the lookup that comes up empty is
    /// this one, and deliberately: a session that is not there, a control the session never
    /// swept, a pass it never ran and a value it never photographed are four ways of saying
    /// "ask for something else", and separating them would be this daemon telling a caller
    /// which parts of a reference it guessed right.
    NoSuchSample,
    /// The store or the filesystem refused. This machine's state, never the reference's fault.
    Refused(schema::Error),
}

/// D13's answer, as the two statuses left after [`no_reference`] has taken the first.
fn unresolved(why: &Unresolved) -> Response {
    match why {
        Unresolved::NoSuchSample => (StatusCode::NOT_FOUND, NO_SUCH_SAMPLE).into_response(),
        Unresolved::Refused(err) => {
            // The one place the detail goes, and it goes to the operator rather than to the
            // page. See this module's header for why the body says less than the preview's.
            tracing::warn!(error = %err, "a stored calibration sample could not be read");
            (StatusCode::SERVICE_UNAVAILABLE, UNREADABLE).into_response()
        }
    }
}

/// The bytes one reference resolves to, and the type they are.
///
/// Built rather than borrowed because it crosses back off a blocking task.
#[derive(Debug)]
struct Found {
    content_type: &'static str,
    bytes: Vec<u8>,
}

impl IntoResponse for Found {
    fn into_response(self) -> Response {
        (
            [
                (
                    header::CONTENT_TYPE,
                    HeaderValue::from_static(self.content_type),
                ),
                // A stored frame is served to the operator who asked for it and is not put on
                // their browser's disk a second time. Design §5's sentence is about the preview
                // — "served, never stored" — and the reason it gives is the one that applies
                // here too: a cache is storage, and what would be stored is a photograph of
                // whatever the camera was pointed at.
                (header::CACHE_CONTROL, HeaderValue::from_static("no-store")),
            ],
            self.bytes,
        )
            .into_response()
    }
}

/// Resolve `asked` against `store`, and read what it names.
///
/// **This is the whole of the path derivation, and every component of the result comes from the
/// store or from the session's own document.** In order:
///
/// 1. the session directory is one [`SessionStore::all_sessions`] found by walking the tree,
///    picked by comparing the [`Uuid`] it parsed out of that directory's *name* with the one the
///    caller sent. A caller that names a session this store does not hold gets no directory at
///    all, so there is nothing to escape from;
/// 2. the pass directory is [`SessionStore::photo_dir_rel`] — D9's layout rule, which runs the
///    D2 slug transform over the control and formats the pass as a number. `../../etc` becomes
///    `etc`; `..` becomes the empty string, which that function refuses;
/// 3. the file name is the one the **sample's own record** carries, and it is admitted only if
///    the recorded path is exactly the derived directory plus that one component. That check is
///    what makes a hand-edited `session.json` — E4's lesson, and `engine::store`'s header names
///    it — unable to point this route anywhere: a `photo` of `../../../etc/passwd` has a parent
///    the derived directory is not, and one of `photos/x/0/..` has no file name at all.
///
/// The bytes are then read from `session_dir.join(&sample.photo)`, which by the check above is
/// the same value as the derived directory joined with the derived name: the record and the
/// derivation agree, or there is no answer.
fn stored(store: &SessionStore, asked: &Reference) -> Result<Found, Unresolved> {
    let found = store
        .all_sessions()
        .map_err(Unresolved::Refused)?
        .into_iter()
        .find(|session| session.id == asked.session)
        .ok_or(Unresolved::NoSuchSample)?;
    let session = store
        .load_session(&found.dir)
        .map_err(Unresolved::Refused)?;
    let control = session
        .controls
        .get(&asked.control)
        .ok_or(Unresolved::NoSuchSample)?;
    let dir_rel = SessionStore::photo_dir_rel(&asked.control, asked.pass)
        .map_err(|_| Unresolved::NoSuchSample)?;

    let sample = control
        .samples
        .iter()
        .find(|sample| sample.requested == asked.value && recorded_under(sample, &dir_rel))
        .ok_or(Unresolved::NoSuchSample)?;
    let content_type = sample
        .photo
        .extension()
        .and_then(PhotoFormat::from_extension)
        .map(content_type)
        .ok_or(Unresolved::NoSuchSample)?;

    let file = found.dir.join(&sample.photo);
    let bytes = std::fs::read(&file).map_err(|err| match err.kind() {
        // A record naming a file that is not there is the same answer as a record that is not
        // there: the operator moved or deleted the photograph, and the caller has to ask for
        // something else. It is not this machine failing.
        std::io::ErrorKind::NotFound => Unresolved::NoSuchSample,
        _ => Unresolved::Refused(schema::Error::StorageIo {
            path: file.clone(),
            errno: err.raw_os_error(),
            message: err.to_string(),
        }),
    })?;
    Ok(Found {
        content_type,
        bytes,
    })
}

/// Is this sample's recorded photo exactly `dir_rel` plus one ordinary component?
///
/// The check [`stored`]'s third step is about. `file_name()` answers `None` for a path that ends
/// in `..`, and rebuilding the path from the pieces is what refuses every other shape — an
/// absolute path, a parent this route did not derive, a trailing separator, a `.` component the
/// join would normalise away.
fn recorded_under(sample: &Sample, dir_rel: &Utf8PathBuf) -> bool {
    sample
        .photo
        .file_name()
        .is_some_and(|name| dir_rel.join(name) == sample.photo)
}

/// The media type of one photo format.
///
/// An exhaustive `match` over this build's own closed vocabulary, which is the one case AGENTS
/// rule 6 does *not* ask for a payload-carrying fallback: [`PhotoFormat`] is a schema type this
/// workspace declares, not device vocabulary, so a fourth member has to be answered here rather
/// than absorbed. `the_formats_this_build_writes_all_have_a_media_type` is what makes that a
/// compile-and-test event instead of an image a browser is asked to guess at.
///
/// `image/x-portable-anymap` for the PPM family because [`PhotoFormat::Ppm`] is the one member
/// that covers more than one file extension (`.ppm` colour and `.pgm` grey — see
/// [`PhotoFormat::from_extension`]), and the `anymap` type is the registered-by-convention name
/// for exactly that family. It is the one of the three no browser paints, which is honest: the
/// escape hatch for tooling is served as what it is.
const fn content_type(format: PhotoFormat) -> &'static str {
    match format {
        PhotoFormat::Jpeg => "image/jpeg",
        PhotoFormat::Png => "image/png",
        PhotoFormat::Ppm => "image/x-portable-anymap",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A reference whose four parameters are the ones a page would send.
    fn asked() -> String {
        format!(
            "{SESSION_QUERY_PARAM}=0198e2c0-0000-7000-8000-000000000001\
             &{CONTROL_QUERY_PARAM}=focus_absolute&{PASS_QUERY_PARAM}=0&{VALUE_QUERY_PARAM}=42"
        )
    }

    #[test]
    fn the_reference_is_read_out_of_the_query_string_and_percent_decoded() {
        // The URL D20's grid builds, and the same URL with `encodeURIComponent` applied to the
        // parts that have nothing to encode plus a `token=` beside them — which is what the
        // shipped page actually sends, because the gate has already read the other half of this
        // string.
        let reference = Reference::of(Some(&asked())).expect("a well-formed reference");
        assert_eq!(reference.control.as_str(), "focus_absolute");
        assert_eq!(reference.pass, 0);
        assert_eq!(reference.value, 42);

        let with_credential = format!("token=c0ffee&{}", asked());
        assert_eq!(
            Reference::of(Some(&with_credential)).expect("a credential is somebody else's"),
            reference
        );
        // Percent-decoded through the one decoder this listener has: a negative value arrives as
        // `-8`, and a client that encoded the hyphen would otherwise name nothing.
        let encoded = asked().replace("value=42", "value=%2D8");
        assert_eq!(
            Reference::of(Some(&encoded))
                .expect("a percent-encoded value")
                .value,
            -8
        );
    }

    #[test]
    fn every_parameter_is_required_and_missing_one_is_the_four_hundred() {
        // Each of the four, left out on its own, so a build that stopped reading one of them
        // fails here rather than by serving the wrong picture. The walk is over the parameter
        // names, so a fifth parameter joins this claim by existing rather than by being
        // remembered.
        for name in [
            SESSION_QUERY_PARAM,
            CONTROL_QUERY_PARAM,
            PASS_QUERY_PARAM,
            VALUE_QUERY_PARAM,
        ] {
            let full = asked();
            let query = full
                .split('&')
                .filter(|pair| !pair.starts_with(&format!("{name}=")))
                .collect::<Vec<&str>>()
                .join("&");
            let why = Reference::of(Some(&query)).expect_err("{name} was left out");
            assert_eq!(why, Unnamed::NotAsked(name), "{name}");
            assert_eq!(
                no_reference(&why).status(),
                StatusCode::BAD_REQUEST,
                "{name}"
            );
        }
        // And nothing at all, which is what a bare `/session-photo` sends.
        assert_eq!(
            no_reference(&Reference::of(None).expect_err("no reference")).status(),
            StatusCode::BAD_REQUEST
        );
    }

    #[test]
    fn a_parameter_that_is_not_a_value_names_no_sample_rather_than_being_a_bad_request() {
        // The `404` arm, at every parameter that has a grammar — and the traversal shapes are
        // *here*, in a pure function, because that is where they stop: a `session` of `../..`
        // is not a UUID, so nothing downstream ever sees it.
        for (name, spelling) in [
            (SESSION_QUERY_PARAM, "../.."),
            (SESSION_QUERY_PARAM, "not-a-uuid"),
            (SESSION_QUERY_PARAM, "%2E%2E%2F"),
            (PASS_QUERY_PARAM, "../0"),
            (PASS_QUERY_PARAM, "-1"),
            (VALUE_QUERY_PARAM, "later"),
        ] {
            let query = asked()
                .split('&')
                .map(|pair| {
                    if pair.starts_with(&format!("{name}=")) {
                        format!("{name}={spelling}")
                    } else {
                        pair.to_owned()
                    }
                })
                .collect::<Vec<_>>()
                .join("&");
            let why = Reference::of(Some(&query))
                .expect_err("{name}={spelling} is no spelling of that parameter");
            assert_eq!(why, Unnamed::NotAValue(name), "{name}={spelling}");
            assert_eq!(
                no_reference(&why).status(),
                StatusCode::NOT_FOUND,
                "{name}={spelling}"
            );
        }
        // An empty `control=` reads as a parameter nobody sent, which is the `400`: the query
        // grammar drops empty values (`super::preview::parameter`), and a caller who typed
        // `control=` has not named a control.
        let empty = asked().replace("control=focus_absolute", "control=");
        assert_eq!(
            Reference::of(Some(&empty)).expect_err("an empty control"),
            Unnamed::NotAsked(CONTROL_QUERY_PARAM)
        );
    }

    #[test]
    fn a_control_that_looks_like_a_path_is_carried_as_a_slug_and_never_as_one() {
        // `ControlSlug::parse` takes it — that is its documented contract, and this route does
        // not become a second place that decides what a control may be called. What makes it
        // harmless is the step after: D9's layout rule slugs it, so a caller-shaped control
        // names a directory *inside* the pass tree or names nothing at all.
        let traversing = ControlSlug::parse("../../etc").expect("any non-empty string");
        assert_eq!(
            SessionStore::photo_dir_rel(&traversing, 0).expect("`etc` is a usable component"),
            Utf8PathBuf::from("photos/etc/0")
        );
        let nothing = ControlSlug::parse("..").expect("any non-empty string");
        assert!(
            SessionStore::photo_dir_rel(&nothing, 0).is_err(),
            "a control that slugs to nothing named a directory"
        );
        // And an absolute spelling, which is the other shape a caller reaches for.
        let absolute = ControlSlug::parse("/etc/passwd").expect("any non-empty string");
        assert_eq!(
            SessionStore::photo_dir_rel(&absolute, 0).expect("`etc_passwd` is a usable component"),
            Utf8PathBuf::from("photos/etc_passwd/0")
        );
    }

    #[test]
    fn a_recorded_photo_path_is_admitted_only_when_it_is_the_derived_one() {
        // The hand-edited-document arm, which is the only way caller-independent text reaches a
        // path here. Both directions: the shape `engine::calibrate` writes is admitted, and
        // every shape an edit could put in its place is not.
        let dir_rel = Utf8PathBuf::from("photos/focus_absolute/0");
        let sample = |photo: &str| Sample {
            requested: 42,
            applied: 42,
            warnings: Vec::new(),
            photo: Utf8PathBuf::from(photo),
            metrics: std::collections::BTreeMap::new(),
            captured_at: schema::time::Stamp::now(),
        };

        assert!(recorded_under(
            &sample("photos/focus_absolute/0/42.jpg"),
            &dir_rel
        ));
        for edited in [
            "photos/focus_absolute/0/../../../../etc/passwd",
            "photos/focus_absolute/0/..",
            "/etc/passwd",
            "../42.jpg",
            "photos/focus_absolute/1/42.jpg",
            "photos/focus_absolute/0/nested/42.jpg",
            "42.jpg",
            "",
        ] {
            assert!(
                !recorded_under(&sample(edited), &dir_rel),
                "{edited:?} was admitted as a path inside {dir_rel}"
            );
        }
    }

    #[test]
    fn the_formats_this_build_writes_all_have_a_media_type() {
        // The walk is over `PhotoFormat::ALL`, so a fourth format joins this claim by existing.
        // A photograph served without a media type — or with somebody else's — is a grid of
        // broken images, and the `match` above is exhaustive so the compiler catches the
        // addition first; this is what catches an addition answered with a copied line.
        let mut seen = std::collections::BTreeSet::new();
        for &format in PhotoFormat::ALL.iter() {
            let media = content_type(format);
            assert!(media.starts_with("image/"), "{format} is served as {media}");
            assert!(
                seen.insert(media),
                "{format} shares its media type with another format"
            );
            // And the extension this build writes maps back to the format it wrote, which is
            // the lookup `stored` does — a format whose `extension()` and `from_extension` had
            // drifted would serve every sample as somebody else's type.
            assert_eq!(
                PhotoFormat::from_extension(format.extension()),
                Some(format)
            );
        }
        assert_eq!(seen.len(), PhotoFormat::ALL.len());
    }

    #[test]
    fn the_path_is_a_path_and_it_is_on_the_list_that_keeps_it_gated() {
        // `SESSION_PHOTO_PATH` has three readers that must agree (see the constant), and a value
        // that stopped being a path would register a route axum never matches. The second half
        // is the one D20 asks for by name: this route serves stored camera frames, so it is
        // camera-bearing by definition, and a build that took it off the list would serve a
        // session's photographs to anybody who asked.
        assert!(SESSION_PHOTO_PATH.starts_with('/'), "{SESSION_PHOTO_PATH}");
        assert!(
            super::super::CAMERA_BEARING_PATHS.contains(&SESSION_PHOTO_PATH),
            "the calibration samples are not on the list of paths that stay gated"
        );
    }

    #[test]
    fn the_refusals_say_what_to_do_and_name_nothing_of_the_operators() {
        // Three fixed sentences, asserted as values. What they must not contain is the thing
        // this route is about: a path on the operator's disk, the name of a session, or a
        // rendering of something (`super::gate::REFUSAL`'s third clause, one route along).
        for body in [NO_REFERENCE, NO_SUCH_SAMPLE, UNREADABLE] {
            assert!(!body.contains("/home/"), "{body}");
            assert!(!body.contains("panicked") && !body.contains('{'), "{body}");
        }
        assert!(NO_REFERENCE.contains(SESSION_QUERY_PARAM), "{NO_REFERENCE}");
        assert!(NO_REFERENCE.contains(CONTROL_QUERY_PARAM), "{NO_REFERENCE}");
        assert!(NO_REFERENCE.contains(PASS_QUERY_PARAM), "{NO_REFERENCE}");
        assert!(NO_REFERENCE.contains(VALUE_QUERY_PARAM), "{NO_REFERENCE}");
    }
}
