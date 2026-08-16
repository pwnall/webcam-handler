//! The web client's files, as bytes inside the binary that serves them (design §2.7, D11).
//!
//! Design §2.7 gives the client three properties in one sentence — "no build step, no npm, no
//! CDN (assets embed; external fetches would violate both the offline posture and the license
//! inventory)" — and this crate is where the third one is true. `assets/` is a directory of
//! hand-written HTML, CSS and, from P5c, ES modules; `rust-embed` turns it into a table
//! compiled into `webcam-handler-daemon`, and [`get`] is the only way back out.
//!
//! ## What is here
//!
//! Eleven files: a page, a stylesheet, and nine ES modules — the JSON-RPC-over-WebSocket
//! helper, the credential, a DOM helper, and one module each for the camera list's composition
//! root, the control panel, the preview, the photo trigger, the recording line and the
//! calibration view. P5c is the sub-milestone that made them a client rather than a skeleton
//! and the recording line arrived at P6c with note **N117**'s ruling. Every one of them is
//! hand-written; nothing here was generated, bundled, minified or vendored (§2.7, AGENTS: "the
//! web client vendors or hand-writes everything — no CDN, no npm").
//!
//! Both numbers in that sentence are checked against the directory by
//! `the_prose_at_the_top_of_this_crate_counts_the_directory_underneath_it`, and they are
//! checked because they were wrong: the count here and the one in `content_type`'s doc drifted
//! apart over two sub-milestones and neither could go red (docs/11 **L37**, note **N153**).
//!
//! **They are ordinary subresources, and until 2026-08-12 they could not have been.** The
//! token rides the URL, and a browser does not carry a document's query string over to the
//! subresources that document asks for — so `<link rel="stylesheet" …>` on a page opened at
//! `/?token=…` was fetched with no credential and the gate refused it, which was the gate
//! being right. A module graph is one request per module, so §2.7's vanilla ES modules could
//! not have inlined their way out. Note **N76** recorded that constraint and its two candidate
//! answers.
//!
//! The owner ruled on 2026-08-12 that **static assets are served without authentication** —
//! these files are open-source code rather than a secret — and only the WS endpoint and the
//! MJPEG preview stay behind D11's token (note **N82**, which retires N76;
//! `daemon::http::listener`'s header carries the same finding beside the gate). So the module
//! graph below is ordinary `import` statements, and the one place in it that writes the
//! credential into a URL is `assets/credential.js`, which writes it into exactly the two
//! routes `daemon::http::CAMERA_BEARING_PATHS` names.
//!
//! ## What this crate asserts about those files, and what it deliberately does not
//!
//! Four things, and all four are properties of *bytes in a table* rather than of a running
//! page: every asset is embedded rather than read off a source tree
//! (`the_assets_are_embedded_rather_than_read_from_this_source_tree`), every asset has a
//! content type this crate can name (`every_asset_has_a_content_type`), **every file the
//! client refers to is a file this build embeds and every embedded file is referred to**
//! (`every_reference_the_client_makes_names_a_file_this_build_embeds`) — which is what stops a
//! renamed module from becoming a `404` in a module graph, and therefore a page that loads
//! nothing — and the page declares every script it loads as a module
//! (`the_page_declares_every_script_it_loads_as_a_module`), because a classic script may not
//! `import` and the resulting page renders perfectly while doing nothing at all.
//!
//! What is **not** asserted here, or anywhere in Rust, is that any of it renders. docs/7 P5c
//! draws that line in as many words — "a browser behavior verified only through the JSON the
//! page consumes is not verified" — so the claim that a sparse menu becomes a `<select>`
//! carrying the device's own indices belongs to P5d's Chromium rung, and an approximation of
//! it here would be worse than its absence.
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
/// Three of the four now have shipped assets — this client is one page, one stylesheet and
/// nine modules — and `.mjs` is the one that does not, kept because it is the other spelling
/// the ecosystem uses for the thing this directory is full of and because the failure it
/// prevents is expensive and quiet: a module served as `application/octet-stream` is a script
/// a browser declines to execute, on a page that then does nothing and says nothing. The test
/// below covers the fallback arm, so the entry with no asset is not the only thing keeping
/// this function honest.
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
    use std::collections::BTreeSet;

    use super::*;

    /// The markers a reference to another file in this directory begins with.
    ///
    /// Four, because four is what a page and a module graph with no build step can produce:
    /// `import … from "x"` and a bare `import "x"` in JavaScript (and `@import "x"` in CSS,
    /// which the same marker catches), and `src=`/`href=` in HTML. There is no dynamic
    /// `import()` in this client and no `new URL(…, import.meta.url)`, both of which would
    /// be a reference this scan cannot see — which is a reason not to write one rather than
    /// a reason to write a parser.
    const REFERENCE_MARKERS: [&str; 4] = ["from ", "import ", "src=", "href="];

    /// This crate's own source, so the prose at the top of it can be checked like anything else.
    ///
    /// The only file in the workspace that reads itself, and it is worth the oddity: what is
    /// being asserted is two sentences, and there is no other way to reach them. `rust-embed`
    /// puts `assets/` in a table this crate can count; the paragraph that says how big that
    /// table is is a comment, which is bytes on disk and nothing else.
    const OWN_SOURCE: &str = include_str!("lib.rs");

    /// The small numbers this file's prose counts in, spelled the way it spells them.
    ///
    /// House style writes a count of files as a word rather than a digit, so a check that
    /// compared numbers would be checking prose nobody writes. Thirteen of them, `zero` through
    /// `twelve`, and **the end of the table is not a failure mode**: [`number_word`] answers
    /// `None` past it. It had to be, because the arm below drives this table one past the
    /// directory's real count on purpose — so a twelfth asset would have met `NUMBER_WORDS[13]`
    /// and *panicked* where the whole point was to report a stale sentence, and a test that
    /// panics names Rust rather than the drift (note **N153**).
    const NUMBER_WORDS: [&str; 13] = [
        "zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten",
        "eleven", "twelve",
    ];

    /// How this file's prose spells `n`, or `None` for a count it has no word for.
    fn number_word(n: usize) -> Option<&'static str> {
        NUMBER_WORDS.get(n).copied()
    }

    /// [`number_word`], capitalised, because one of the two sentences starts with it.
    fn capitalised(n: usize) -> Option<String> {
        let word = number_word(n)?;
        let (first, rest) = word.split_at(1);
        Some(format!("{}{rest}", first.to_uppercase()))
    }

    /// This file's doc comments, unwrapped into the sentences a reader reads.
    ///
    /// **A doc comment is wrapped to the column limit, so a sentence in one is a sentence *and a
    /// line break*.** `content_type`'s count is written `…one page, one stylesheet and\n/// nine
    /// modules…`, and a search of the raw source for that sentence as a reader reads it finds
    /// nothing there. Until this existed the second half of the reconciler below matched the only
    /// unwrapped copy in the file — the narrative comment in the test itself — so `content_type`'s
    /// count could be edited to any number at all and this suite stayed green, which is the
    /// finding it was written to close arriving one line lower down (note **N153**).
    ///
    /// **`//!` and `///` only**, which is the other half of the repair rather than an
    /// optimisation. What is reconciled is the prose this crate *documents itself with*; the
    /// test's own account of what went wrong quotes the old wording, and a haystack that included
    /// it would be satisfied by a comment about the defect instead of by the doc that carries it.
    fn documented_prose(source: &str) -> String {
        let mut prose = String::with_capacity(source.len());
        for line in source.lines() {
            let trimmed = line.trim_start();
            let Some(text) = trimmed
                .strip_prefix("//!")
                .or_else(|| trimmed.strip_prefix("///"))
            else {
                continue;
            };
            // One space per line, which is what unwraps a sentence: the break between `and` and
            // `nine modules` becomes the space it was written as.
            prose.push(' ');
            prose.push_str(text.trim());
        }
        prose
    }

    /// Where this crate's prose fails to state `files` and `modules` exactly once. Empty is
    /// green.
    ///
    /// A pure function over the source text so the arms below can drive it with numbers the
    /// directory does not have, which is the half that proves it can go red: a reconciler whose
    /// failing direction has never been seen is a reconciler nobody has checked (AGENTS rule 2).
    ///
    /// **Once**, not merely somewhere. The finding this closes is two sentences in one file that
    /// stated different counts, so a check satisfied by *a* correct copy would be satisfied by
    /// the tree that produced the finding. Counting occurrences of the assembled sentence also
    /// keeps this function's own format templates out of its own haystack — they carry the
    /// placeholders rather than the numbers, so they can never be a match — and the haystack is
    /// [`documented_prose`] rather than `source`, which is what makes the second sentence a
    /// sentence this can find at all.
    fn counts_the_client(source: &str, files: usize, modules: usize) -> Vec<String> {
        let mut stale = Vec::new();
        let prose = documented_prose(source);
        let (Some(files_word), Some(modules_word)) = (capitalised(files), number_word(modules))
        else {
            // A count past the end of the table is reported rather than indexed: the sentence
            // cannot even be assembled, so it certainly is not in the file, which is the same
            // answer by a shorter road.
            stale.push(format!(
                "this file's prose has no word for {files} file(s) or {modules} module(s); \
                 `NUMBER_WORDS` counts to {last} and `assets/` has outgrown it",
                last = NUMBER_WORDS.len() - 1
            ));
            return stale;
        };
        let header =
            format!("{files_word} files: a page, a stylesheet, and {modules_word} ES modules");
        let said = prose.matches(&header).count();
        if said != 1 {
            stale.push(format!(
                "this crate's header says `{header}` {said} time(s) and should say it once; \
                 `assets/` holds {files} file(s) and {modules} module(s) today"
            ));
        }
        let typed = format!("one page, one stylesheet and {modules_word} modules");
        let named = prose.matches(&typed).count();
        if named != 1 {
            stale.push(format!(
                "`content_type`'s doc says `{typed}` {named} time(s) and should say it once; \
                 `assets/` holds {modules} module(s) today"
            ));
        }
        stale
    }

    #[test]
    fn the_prose_at_the_top_of_this_crate_counts_the_directory_underneath_it() {
        // **Two sentences in this file state how many files the client has, and until the G6
        // review nothing could go red on either** (docs/11 **L37**, note **N153**). They had
        // drifted in opposite directions and disagreed with each other as well as with the
        // directory: the header said "ten files … eight ES modules" and enumerated eight roles,
        // `content_type`'s doc said "one page, one stylesheet and nine modules", and `assets/`
        // held eleven files and nine modules. `recording.js` landed at P6c and neither sentence
        // moved, which is the ordinary way a count in a comment stops being one.
        //
        // A count in prose is not decoration here. The header is where a reader is told what
        // this crate *is*, and a reader who believes there are eight modules when there are
        // nine has been told the client is smaller than it is — which is the same class as a
        // skip that reads as a pass, one document along.
        //
        // **And this comment was, for one afternoon, half of what the check checked.** The
        // second sentence is line-wrapped in `content_type`'s doc and the paragraph above quotes
        // it whole, so the one match a search of the raw source found was *here*: `content_type`
        // could be edited to say four modules and this test stayed green, while an editor who
        // re-wrapped this comment would have turned it red blaming a doc that had not moved.
        // `counts_the_client` reads [`documented_prose`] for that reason, and this paragraph is
        // no longer in the haystack it searches.
        let files = paths().count();
        let modules = paths().filter(|path| path.ends_with(".js")).count();
        assert!(files > 2, "the asset walk found {files} file(s)");
        assert!(modules > 0, "the asset walk found no modules");

        let stale = counts_the_client(OWN_SOURCE, files, modules);
        assert!(stale.is_empty(), "{stale:?}");

        // Both directions, driven rather than asserted about — and driven **per sentence**,
        // which is the half that was missing. A file added to `assets/` and a module added to it
        // are the two ways this drifts; the file count is stated in the header alone and the
        // module count in both sentences, so a module one out is *two* complaints and a file one
        // out is one. Arms that only asked whether the answer was non-empty were satisfied by the
        // header's complaint alone, which is how the second sentence went unchecked while the arm
        // that was supposed to check it went red for the first one's sake (note **N153**).
        assert_eq!(
            counts_the_client(OWN_SOURCE, files + 1, modules).len(),
            1,
            "a file count one ahead of the directory should be stale in the header and nowhere \
             else"
        );
        assert_eq!(
            counts_the_client(OWN_SOURCE, files, modules + 1).len(),
            2,
            "a module count one ahead of the directory should be stale in both sentences that \
             state it"
        );
        // …and one past the end of the number table, which is the arm above arriving at a client
        // that has grown: a count this file's prose has no word for is a complaint, never an
        // index off the end of `NUMBER_WORDS`.
        assert_eq!(
            counts_the_client(OWN_SOURCE, NUMBER_WORDS.len(), modules).len(),
            1,
            "a count past the end of the number table should be a sentence rather than a panic"
        );
    }

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
        // the machine running `webcam-handler-daemon`.
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

    #[test]
    fn every_reference_the_client_makes_names_a_file_this_build_embeds() {
        // **The module graph, closed and with nothing orphaned in it.** Two claims, walked
        // together from [`INDEX`] because that is the only file the daemon names:
        //
        // 1. every reference resolves to an asset this build embeds — a renamed module is a
        //    `404` inside a module graph, which a browser reports by running *nothing*, and
        //    is therefore the cheapest way to ship a page that silently does not work;
        // 2. every asset is reachable — a file nobody imports is bytes in
        //    `webcam-handler-daemon` that no request will ever ask for, and the honest fix for
        //    one is to delete it.
        //
        // Neither implies the other, and neither is a claim about a browser: this is a graph
        // over a table of names fixed at compile time, which is exactly the kind of thing
        // that *can* be settled without one. Whether Chrome then executes what it fetched is
        // P5d's, in Chrome.
        let mut reached: BTreeSet<String> = BTreeSet::new();
        let mut queue = vec![INDEX.to_owned()];
        let mut references = 0_usize;
        while let Some(name) = queue.pop() {
            if !reached.insert(name.clone()) {
                continue;
            }
            let asset = get(&name)
                .unwrap_or_else(|| panic!("the client refers to {name}, which is not embedded"));
            for reference in loadable(&name, asset.bytes()) {
                references += 1;
                queue.push(reference);
            }
        }

        let embedded: BTreeSet<String> = paths().map(|path| path.into_owned()).collect();
        assert_eq!(
            reached, embedded,
            "the client's module graph and this crate's asset table are not the same set of \
             files"
        );
        // Not vacuous in either direction: a page with no references at all would satisfy
        // the equality above on a directory holding only the page.
        assert!(references > 0, "nothing in this client refers to anything");
        assert!(embedded.len() > 1, "{embedded:?}");
    }

    #[test]
    fn the_page_declares_every_script_it_loads_as_a_module() {
        // §2.7's client is ES *modules*, and a classic script may not `import` — so a
        // `<script>` on this page without `type="module"` is a file the browser fetches,
        // parses, and refuses at the first import statement. The failure is quiet enough to
        // be worth a test: the page still renders, and none of it does anything.
        //
        // A claim about the document's bytes, not about a browser's behaviour: what is
        // asserted is that the attribute is written, which is the half that lives in this
        // directory.
        let index = get(INDEX).expect("the client's page");
        let text = String::from_utf8(index.bytes().to_vec()).expect("the page is UTF-8");
        let mut scripts = 0_usize;
        for tag in text.split("<script").skip(1) {
            scripts += 1;
            let opening = tag.split_once('>').map_or(tag, |(opening, _rest)| opening);
            assert!(
                opening.contains(r#"type="module""#),
                "a <script{opening}> that is not a module"
            );
        }
        assert_eq!(scripts, 1, "the page loads more than one entry point");
    }

    /// The parts of one asset that can refer to another, as text.
    ///
    /// Comments are removed first, and that is what keeps this a dozen lines instead of two
    /// parsers: every module in `assets/` opens with a long header that argues about
    /// `import`, credentials and `<img>` elements, and a scan that read those would find
    /// references to files nobody ships. The rule is deliberately coarse — a *whole-line*
    /// `//` comment in JavaScript and CSS, an `<!-- … -->` in HTML — so its cost is stated
    /// rather than hidden: a **trailing** comment on a line of code, carrying one of
    /// [`REFERENCE_MARKERS`] followed by a quote, would be read as a reference and would
    /// fail this test. That is a rule about how to write a comment in this directory, and it
    /// is cheaper than the alternative, which is a JavaScript parser in an asset crate whose
    /// dependency wall is `rust-embed` and nothing else (T6).
    fn loadable(name: &str, bytes: &[u8]) -> Vec<String> {
        let text = String::from_utf8_lossy(bytes).into_owned();
        let code = if name.ends_with(".html") {
            let mut out = String::with_capacity(text.len());
            let mut rest = text.as_str();
            while let Some((before, after)) = rest.split_once("<!--") {
                out.push_str(before);
                rest = after.split_once("-->").map_or("", |(_comment, tail)| tail);
            }
            out.push_str(rest);
            out
        } else {
            text.lines()
                .filter(|line| !line.trim_start().starts_with("//"))
                .collect::<Vec<_>>()
                .join("\n")
        };
        references(&code)
    }

    /// Every quoted file name `code` refers to, keyed off [`REFERENCE_MARKERS`].
    ///
    /// A leading `./` or `/` is stripped, because those are the two spellings a relative
    /// module specifier and an absolute request path take, and `get`'s table is keyed on
    /// neither (the daemon strips the one leading `/` an HTTP path has — `lookup` in
    /// `daemon::http::listener`).
    fn references(code: &str) -> Vec<String> {
        let mut found = Vec::new();
        for marker in REFERENCE_MARKERS {
            let mut rest = code;
            while let Some(at) = rest.find(marker) {
                let after = rest.split_at(at + marker.len()).1;
                rest = after;
                let after = after.trim_start();
                let mut characters = after.chars();
                let Some(quote @ ('"' | '\'')) = characters.next() else {
                    continue;
                };
                let body = characters.as_str();
                let Some(end) = body.find(quote) else {
                    continue;
                };
                let name = body.split_at(end).0;
                found.push(
                    name.strip_prefix("./")
                        .or_else(|| name.strip_prefix('/'))
                        .unwrap_or(name)
                        .to_owned(),
                );
            }
        }
        found
    }
}
