//! The web client's files, as bytes inside the binary that serves them (design §2.7, D11).
//!
//! Design §2.7 gives the client three properties in one sentence — "no build step, no npm, no
//! CDN (assets embed; external fetches would violate both the offline posture and the license
//! inventory)" — and this crate is where the third one is true. `assets/` is a directory of
//! hand-written HTML, CSS and, from P5c, ES modules; `rust-embed` turns it into a table
//! compiled into `wchd`, and [`get`] is the only way back out.
//!
//! ## What is here at P5a, and what is not
//!
//! A **skeleton**: one page, whose styles are inline. P5c lands the client — the camera list,
//! the control panel generated from the `controls` DTO, the preview `<img>`, the calibration
//! view. What P5a needed from this crate is exactly what a listener and a token gate need to
//! be *about* something: bytes with a content type, at a path, that a browser will render.
//!
//! **One file rather than a page and a stylesheet, and the reason has since been ruled away.**
//! The token rides the URL, and a browser does not carry a document's query string over to the
//! subresources that document asks for — so `<link rel="stylesheet" href="app.css">` on a page
//! opened at `/?token=…` was fetched as `/app.css` with no credential, and the gate refused it,
//! which was the gate being right. A skeleton that shipped that would have been a page that
//! rendered unstyled in every token-gated cell, and the real client could not have inlined its
//! way out (§2.7's vanilla ES modules are subresources by definition). Note **N76** recorded
//! that constraint and its two candidate answers.
//!
//! The owner ruled on 2026-08-12 that **static assets are served without authentication** —
//! these files are open-source code rather than a secret — and only the WS endpoint and the
//! MJPEG preview stay behind D11's token (note **N82**, which retires N76;
//! `daemon::http::listener`'s header carries the same finding beside the gate). So P5c's module
//! graph is ordinary `import` statements and a second file here is an ordinary file. This one
//! keeps its inline styles because a skeleton with a stylesheet beside it would be two files
//! saying what one file says, not because it must.
//!
//! ## The seam, and why the daemon does not link `rust-embed`
//!
//! Everything below is an inherent function on values. `webcam-handler-daemon` asks this
//! crate *what a path holds*; it does not derive an embed, import a trait, or know that
//! `rust-embed` exists (design §2.10, one home per law — "what these files are" is this
//! crate's law, and "how bytes become an HTTP response" is the daemon's). The practical half
//! of that is that a caller needs no `use rust_embed::RustEmbed` in scope to read an asset,
//! and the reviewable half is that the embed machinery has one home and one reader.
//!
//! The content types live here for the same reason. A table in the daemon would be a table
//! that has to be edited when a *file in this crate* is added, which is the shape a stale
//! `Content-Type` arrives in; here, `every_asset_has_a_content_type` — a test in this
//! file — walks the embedded names and refuses one this crate cannot type.
//!
//! ## `..` in a request path
//!
//! It cannot name anything. With `debug-embed` on (the manifest argues it), [`get`] is a
//! lookup in a table of names fixed at compile time, so a path that is not one of those
//! names has no answer to give — the traversal question is closed by the shape rather than by
//! a check somebody has to remember to keep. The daemon still strips the leading `/` and
//! nothing else; there is no normalization step here to disagree with a browser's.
#![forbid(unsafe_code)]
// The same rule the daemon states about itself, for the same reason: the argument to [`get`]
// arrived over a socket, from somebody else, and a panic on that path takes the camera's
// owner down with it.
#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )
)]

use std::borrow::Cow;

/// The page a browser lands on, and the one thing in this crate the daemon spells by name.
///
/// D11's ready-to-open URL points at `/`, which is not a file — so somebody has to say which
/// file `/` means, and it is this crate rather than the router: "the client's entry point" is
/// a fact about the client. A daemon that carried the string would be a daemon that keeps
/// serving a page this crate had renamed.
pub const INDEX: &str = "index.html";

/// What an asset whose extension this crate does not know is served as.
///
/// Deliberately the least useful correct answer. A browser meeting `application/octet-stream`
/// on a stylesheet does not apply it and does not guess — the failure is immediate, local and
/// obvious, which is what makes it a safe default for a table that is four entries long. The
/// unit test below is what stops it from ever being the answer for a *shipped* asset; this
/// constant is for the case where somebody adds a file and the test has not run yet.
pub const UNKNOWN_CONTENT_TYPE: &str = "application/octet-stream";

/// The `assets/` directory, embedded.
///
/// Private, and it is the only item in this crate that knows `rust-embed` is involved. The
/// public surface is [`get`] and [`paths`], which answer values.
#[derive(rust_embed::RustEmbed)]
#[folder = "assets/"]
struct Embedded;

/// One file of the web client: its bytes and what it is.
///
/// The bytes are a [`Cow`] because that is what `rust-embed` answers, and it is `Borrowed` for
/// every asset in a build with `debug-embed` on — the whole directory is `&'static [u8]`
/// inside the binary, so serving a page copies nothing.
#[derive(Debug, Clone)]
pub struct Asset {
    bytes: Cow<'static, [u8]>,
    content_type: &'static str,
}

impl Asset {
    /// The file, verbatim.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// What to put in the response's `Content-Type`, [`UNKNOWN_CONTENT_TYPE`] when this crate
    /// cannot say.
    #[must_use]
    pub fn content_type(&self) -> &'static str {
        self.content_type
    }
}

/// The asset at `path`, or `None` when there is no such file.
///
/// `path` is relative and carries no leading slash: `index.html`, `app.css`. That is the form
/// `rust-embed` keys the table on, and the daemon reaches it by stripping the one leading `/`
/// an HTTP request path always has — one transformation, in one place, rather than a
/// normalizer here and another there.
///
/// **`None` is the whole of the refusal.** There is no case where this answers bytes for a
/// path that is not in `assets/`: with `debug-embed` the lookup is a match against names
/// fixed at compile time (this module's header), so `../../etc/passwd` is not a traversal
/// that has to be caught, it is a name that is not in the table.
#[must_use]
pub fn get(path: &str) -> Option<Asset> {
    let file = <Embedded as rust_embed::RustEmbed>::get(path)?;
    Some(Asset {
        bytes: file.data,
        content_type: content_type(path),
    })
}

/// Every asset this build embedded, by path.
///
/// Its readers are tests — this crate's, which asserts each one is typed, and the daemon's,
/// which serves each one over a real socket and would otherwise be asserting about a list it
/// had written itself. A suite that names its fixtures cannot notice a file that stopped
/// being embedded.
pub fn paths() -> impl Iterator<Item = Cow<'static, str>> {
    <Embedded as rust_embed::RustEmbed>::iter()
}

/// What a file's extension says it is.
///
/// Four entries, which is the whole vocabulary design §2.7 gives the client: a page, a
/// stylesheet, and ES modules under either of the two spellings the ecosystem uses. The
/// alternative was `rust-embed`'s `mime-guess` feature, and the manifest argues why a
/// thousand-entry table is not what four extensions need — the short version is that the
/// forcing function is not the table's size, it is
/// `every_asset_has_a_content_type`'s walk over the assets that actually exist.
///
/// Three of the four have no shipped asset yet, and that is said out loud rather than left to
/// be noticed: the skeleton is one HTML file (this module's header says why it is not two).
/// They are here because §2.7 names them as what P5c is made of and each is one line, while
/// the failure they prevent — a module served as `application/octet-stream`, which a browser
/// declines to execute — costs a debugging session. The test below covers the fallback arm,
/// so the entries that have no asset are still not the only thing keeping this function
/// honest.
///
/// `charset=utf-8` on the text types because the files are UTF-8 and saying so is what stops
/// a browser guessing; the guess is usually right and "usually" is not a property to serve a
/// control panel on.
fn content_type(path: &str) -> &'static str {
    match path.rsplit_once('.') {
        Some((_stem, "html")) => "text/html; charset=utf-8",
        Some((_stem, "css")) => "text/css; charset=utf-8",
        // `text/javascript` rather than `application/javascript`: the HTML standard calls the
        // latter legacy, and a module script is served under whichever of them the browser
        // gets — this is the one the specification points at.
        Some((_stem, "js" | "mjs")) => "text/javascript; charset=utf-8",
        _ => UNKNOWN_CONTENT_TYPE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_index_page_is_embedded_and_is_html() {
        // The one asset the daemon spells by name, so a rename that left `INDEX` behind is a
        // 404 at `/` — which in the shipped daemon is the URL D11 asks an operator to open.
        let index = get(INDEX).expect("the page the ready-to-open URL points at");

        assert_eq!(index.content_type(), "text/html; charset=utf-8");
        assert!(
            index.bytes().starts_with(b"<!doctype html>"),
            "the index is not an HTML document"
        );
    }

    #[test]
    fn the_assets_are_embedded_rather_than_read_from_this_source_tree() {
        // The `debug-embed` decision, as something that can go red. Without the feature this
        // is a debug build reading `assets/` off the filesystem at an absolute path, and the
        // observable difference is exactly this: `Cow::Borrowed` is a slice of the binary,
        // `Cow::Owned` is a `Vec` that was just read from a directory that may not exist on
        // the machine running `wchd`.
        //
        // Asserted over every asset rather than over one, because the feature is per-build
        // and a single sample would pass on a tree where somebody had added a file the walk
        // could not reach.
        for path in paths() {
            let asset = get(&path).expect("a path the embed itself just listed");
            assert!(
                matches!(asset.bytes, Cow::Borrowed(_)),
                "{path} was read from the filesystem at run time, not embedded"
            );
        }
    }

    #[test]
    fn every_asset_has_a_content_type() {
        // The forcing function the manifest's `mime-guess` paragraph names. A `.svg` or a
        // `.png` added to `assets/` without an entry in `content_type` reaches a browser as
        // `application/octet-stream`, which for a stylesheet is a page that renders unstyled
        // and for a module is a script that never runs — a failure that is easy to see and
        // hard to attribute. This is where it is attributed.
        let mut counted = 0;
        for path in paths() {
            counted += 1;
            assert_ne!(
                content_type(&path),
                UNKNOWN_CONTENT_TYPE,
                "{path} has no content type; add its extension to `content_type`"
            );
        }
        assert!(
            counted > 0,
            "the asset walk found nothing, so the loop above asserted nothing — which is \
             exactly how a crate that had stopped embedding its own directory would look"
        );
    }

    #[test]
    fn a_path_that_is_not_an_asset_has_no_answer() {
        // Including the two shapes a hostile one takes. Neither is a traversal this code
        // catches — with the table fixed at compile time there is nothing to traverse — and
        // that is the claim: `None` for a name that is not in the table, whatever the name is
        // made of.
        assert!(get("nothing-here.html").is_none());
        assert!(get("../../etc/passwd").is_none());
        assert!(get("/index.html").is_none(), "the key carries no leading /");
        assert!(get("").is_none());
    }

    #[test]
    fn the_content_types_are_the_four_a_no_build_step_client_has() {
        // The table on its own, including the fallback — which is the arm the assertion above
        // it can never reach while every shipped asset is typed.
        assert_eq!(content_type("index.html"), "text/html; charset=utf-8");
        assert_eq!(content_type("app.css"), "text/css; charset=utf-8");
        assert_eq!(content_type("app.js"), "text/javascript; charset=utf-8");
        assert_eq!(content_type("app.mjs"), "text/javascript; charset=utf-8");
        assert_eq!(content_type("favicon.ico"), UNKNOWN_CONTENT_TYPE);
        assert_eq!(
            content_type("README"),
            UNKNOWN_CONTENT_TYPE,
            "a file with no extension at all"
        );
        // The extension is the part after the *last* dot, so a name that merely contains one
        // is not typed by it — `app.css.bak` is not a stylesheet.
        assert_eq!(content_type("app.css.bak"), UNKNOWN_CONTENT_TYPE);
    }
}
