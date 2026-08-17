//! Build-side generation.
//!
//! Everything this emits is a **generated artifact**, not a second source of truth: the
//! Rust types are the schema, the T5 trait is the wire surface, and these files are what
//! other tools read. They are committed so consumers do not need a Rust toolchain, and
//! `scripts/gates/schema-artifacts-current.sh` re-runs the emitter and diffs, so a
//! committed copy cannot drift from the types it documents. Nothing in this workspace
//! reads either file back.
//!
//! Three artifacts, three audiences (design D10, docs/7 P6e):
//!
//! | File | Describes | For |
//! |---|---|---|
//! | [`BUNDLE`] | the DTOs | a consumer validating `--json`, a session file or a profile |
//! | [`OPENRPC`] | the daemon's method surface | a consumer speaking JSON-RPC to it |
//! | [`guide::GUIDE_PATH`] | the command surface, in prose | an agent driving the CLI unattended |
//!
//! `generate` writes them; `generate --out DIR` writes the same tree under `DIR` — every
//! path relative to it exactly as it is relative to the repository root — which is how the
//! two freshness gates compare without touching the tree. **The root, not one directory**,
//! since P6e put a generated file outside `schemas/`: an `--out` that meant "where the
//! schemas go" could not relocate the guide, and a gate that had to regenerate in place
//! would be a gate that writes to the thing it is judging.
#![forbid(unsafe_code)]

mod guide;

use std::collections::BTreeMap;
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use camino::{Utf8Path, Utf8PathBuf};
use schemars::{JsonSchema, SchemaGenerator, generate::SchemaSettings};
use serde_json::{Value, json};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("xtask: {err:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let command = args.next().unwrap_or_else(|| "help".to_owned());
    match command.as_str() {
        "generate" => {
            let mut out: Option<Utf8PathBuf> = None;
            while let Some(arg) = args.next() {
                match arg.as_str() {
                    "--out" => {
                        let value = args.next().context("--out needs a directory")?;
                        out = Some(Utf8PathBuf::from(value));
                    }
                    other => bail!("unknown argument {other:?}"),
                }
            }
            let out = match out {
                Some(out) => out,
                None => repo_root()?,
            };
            generate(&out)
        }
        "help" | "--help" | "-h" => {
            println!("usage: xtask generate [--out DIR]   (DIR stands in for the repo root)");
            Ok(())
        }
        other => bail!("unknown command {other:?}; try `xtask help`"),
    }
}

/// Where the generated JSON artifacts live, relative to the repository root.
///
/// Read out of this line by `scripts/gates/schema-artifacts-current.sh`, which names no
/// directory of its own for the same reason it names no filename. `guide::GUIDE_PATH` is the
/// other declaration of the same kind, read the same way by
/// `scripts/gates/agent-guide-current.sh`.
const ARTIFACT_DIR: &str = "schemas";

/// The bundle's filename.
const BUNDLE: &str = "webcam-handler-schema.json";

/// The OpenRPC document's filename.
///
/// Symmetric with [`BUNDLE`], and deliberately not the bare `openrpc.json`:
/// `scripts/gates/cases/schema-artifacts-current.cases.sh` seeds that name as its
/// *orphan* fixture — a committed artifact nothing emits — and an emitter that claimed it
/// would leave that arm failing for the stale reason instead, which is a different claim
/// and a gate quietly checking less than it says (note N10).
const OPENRPC: &str = "webcam-handler-openrpc.json";

/// The OpenRPC specification version this document is written to.
const OPENRPC_VERSION: &str = "1.3.2";

/// Where the OpenRPC document keeps its schemas, as the JSON pointer `schemars` wants.
///
/// OpenRPC puts reusable schemas under `components/schemas` rather than JSON Schema's
/// `$defs`, so the generator is pointed there and every `$ref` it writes resolves inside
/// the document. A document whose references pointed at the bundle beside it would be two
/// files a consumer has to fetch, and one of them could arrive stale on its own.
const OPENRPC_SCHEMAS: &str = "/components/schemas";

/// Where the OpenRPC document keeps the D13 error registry.
const OPENRPC_ERRORS: &str = "/components/errors";

fn repo_root() -> Result<Utf8PathBuf> {
    // The manifest directory is `<root>/xtask`; deriving the root from it means this
    // works from any cwd without shelling out to git.
    let manifest = Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .map(Utf8Path::to_path_buf)
        .context("xtask's manifest directory has no parent")
}

/// Register a type and remember the name to reference it by.
///
/// The list of roots below is the *documented surface*: types a consumer of `--json`
/// output, a session file, or a device profile will actually meet. It is deliberately
/// a list rather than something derived, because "every type in the crate" is not the
/// same claim and would document internal helpers as if they were contracts.
///
/// **`--json` is the whole of the audience here.** A type the wire carries and no `--json`
/// verb prints — `DiscoveryReport`, `TerminationReport`, `ControlWrite`, `Selection`,
/// `SessionRef` — is named in the OpenRPC document instead, under `components/schemas`,
/// which is the document its consumer is reading. Registering it here as well would put a
/// contract in front of a reader who cannot reach it, and the bundle's own emitted-vs-CLI
/// check (`scripts/gates/json-validates.sh`, which maps every verb the CLI's help lists to
/// a bundle type and fails a verb with no row) is what makes "every `--json` answer has a
/// root" a checkable claim rather than this list's good intentions.
fn register<T: JsonSchema>(generator: &mut SchemaGenerator, roots: &mut Vec<String>) {
    let _schema = generator.subschema_for::<T>();
    roots.push(T::schema_name().into_owned());
}

fn bundle() -> Result<Value> {
    use schema::camera::{CameraInfo, FormatInfo};
    use schema::capture::{
        NegotiatedStream, PhotoFormat, PhotoReport, PhotoRequest, SettlePolicy, Sink,
        StreamRequest, Transform,
    };
    use schema::control::{Applied, ControlDesc, ControlValue, WriteWarning};
    use schema::error::{Error, Failure};
    use schema::profile::DeviceProfile;
    use schema::report::{CameraDetail, CameraList, ControlReport, WriteReport};
    use schema::session::{LogEntry, Session, SessionList, SessionStatus, SweepRequest};
    use schema::snapshot::{RestoreReport, Snapshot};
    use schema::video::{RecordReport, RecordRequest, RecordStatus};

    let mut generator = SchemaGenerator::new(SchemaSettings::draft2020_12());
    let mut roots: Vec<String> = Vec::new();

    register::<CameraInfo>(&mut generator, &mut roots);
    // The read verbs' answers: `--json` emits these verbatim (design §2.7), so a
    // consumer validating our output needs them named in the bundle.
    register::<CameraList>(&mut generator, &mut roots);
    register::<CameraDetail>(&mut generator, &mut roots);
    register::<ControlReport>(&mut generator, &mut roots);
    register::<FormatInfo>(&mut generator, &mut roots);
    register::<ControlDesc>(&mut generator, &mut roots);
    register::<ControlValue>(&mut generator, &mut roots);
    register::<Applied>(&mut generator, &mut roots);
    register::<WriteWarning>(&mut generator, &mut roots);
    register::<StreamRequest>(&mut generator, &mut roots);
    register::<NegotiatedStream>(&mut generator, &mut roots);
    register::<Transform>(&mut generator, &mut roots);
    register::<PhotoFormat>(&mut generator, &mut roots);
    register::<Sink>(&mut generator, &mut roots);
    // The P2 write and photo answers. `PhotoDelivery`, `PhotoRendering`,
    // `TransformApplication`, `SettleSpec`, `Adjustment` and `AutomationPair` arrive as
    // `$defs` by reachability rather than as roots — a root is a document a `--json` verb
    // emits.
    register::<WriteReport>(&mut generator, &mut roots);
    register::<PhotoRequest>(&mut generator, &mut roots);
    register::<PhotoReport>(&mut generator, &mut roots);
    register::<SettlePolicy>(&mut generator, &mut roots);
    register::<Snapshot>(&mut generator, &mut roots);
    register::<RestoreReport>(&mut generator, &mut roots);
    register::<Session>(&mut generator, &mut roots);
    register::<LogEntry>(&mut generator, &mut roots);
    // The P3d calibration verbs' answers and the one request they take. `SessionListing`
    // arrives as a `$def` by reachability; a root is a document a `--json` verb emits or
    // accepts.
    register::<SweepRequest>(&mut generator, &mut roots);
    register::<SessionStatus>(&mut generator, &mut roots);
    register::<SessionList>(&mut generator, &mut roots);
    // **The live streams, walked rather than listed.** Every subscription's item type is a
    // root: nothing prints one as `--json`, which is what the paragraph on `register` says a
    // root usually means, but a subscription payload is the one other thing a consumer
    // validates our output against — one notification is one of these and nothing else.
    //
    // That is a *law*, so it has the derivable home a law gets. `api::SUBSCRIPTIONS` is the
    // second inventory `wire_surface!` emits from the same declaration as the trait, and
    // `openrpc()` below already walks it; a hand list here was the same rule written twice,
    // and the second copy did not grow (measured: a third subscription reached the OpenRPC
    // document's `x-subscriptions` and neither `x-roots` nor `$defs` here, with every xtask
    // test green — note **N59**). `roots` is sorted and deduped below, so a payload that is
    // also a root for another reason costs nothing.
    for subscription in api::SUBSCRIPTIONS {
        let _schema = subscription.item.schema(&mut generator);
        roots.push(subscription.item.name().into_owned());
    }
    // P6c's three documents: the request `record` sends, the report it answers with, and the
    // status a client polls between them (D7, D10). `RecordingSummary`, `TakeStatus`,
    // `VideoFormat`, `RecordingEnd`, `CapReached` and `IntervalSource` arrive as `$defs` by
    // reachability rather than as roots, because a root is a document a `--json` verb emits or
    // accepts and none of those is one on its own.
    register::<RecordRequest>(&mut generator, &mut roots);
    register::<RecordReport>(&mut generator, &mut roots);
    register::<RecordStatus>(&mut generator, &mut roots);
    register::<DeviceProfile>(&mut generator, &mut roots);
    register::<Error>(&mut generator, &mut roots);
    // P6f's failure document, and a root by the strictest reading of the paragraph above: it
    // is the one thing a `--json` verb prints that is not the verb's answer (owner ruling,
    // 2026-08-15; note **N127**). A consumer validating our output meets it on exactly the
    // runs it most needs to parse, so leaving it to arrive as a `$def` by reachability from
    // `Error` would name the payload in the bundle and not the document carrying it.
    register::<Failure>(&mut generator, &mut roots);

    roots.sort();
    roots.dedup();

    let definitions = generator.take_definitions(true);
    for root in &roots {
        if !definitions.contains_key(root) {
            bail!("root type {root} did not land in the definition map");
        }
    }

    Ok(json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "title": "webcam-handler schema bundle",
        // The tool version, not a schema version: the per-document `schema_version`
        // fields are the compatibility contract, and they live on the documents.
        "x-tool-version": schema::TOOL_VERSION,
        "x-roots": roots,
        "$defs": definitions,
    }))
}

/// The OpenRPC document: the T5 surface, its DTOs and the D13 codes it refuses with.
///
/// ## Subscriptions are an extension, and that is the honest encoding
///
/// OpenRPC 1.3.2 — the version this document is written to — has **no notion of a
/// server-initiated notification stream**. Three encodings were available and two of them
/// publish something false:
///
/// | Encoding | What it claims | Honest? |
/// |---|---|---|
/// | `wch_subscribe_events` as a `method` | true of the subscribe *call*; silent about the payload, which is the only interesting part | half |
/// | `wch_unsubscribe_events` as a `method` too | **false** — its callback is `params.one::<RpcSubscriptionId>()`, positional only, and every method here declares `"paramStructure": "by-name"` | no |
/// | a top-level `x-subscriptions` array | complete about both names, the notification name and the item schema; invisible to a stock OpenRPC tool | yes, and non-standard |
///
/// So `methods` is exactly the *call* surface — `api::METHODS`, unchanged — and the two
/// subscriptions are described beside it, with their item schemas resolving into the same
/// `components/schemas` every other `$ref` here does. Every string comes from
/// `api::SUBSCRIPTIONS`; nothing is retyped. Note **N57** records the decision.
///
/// Everything here is *derived*. The method list is `webcam-handler-api`'s `METHODS`,
/// which that crate declares in the same tokens as the trait itself, so this emitter
/// cannot describe a method the daemon does not serve or miss one it does — a hand list
/// here is exactly the drift docs/9 names when it says "a Rust trait does not reify its
/// methods". Parameter and result schemas come from the Rust types through the same
/// `schemars` derives the bundle uses. The errors come from the registry: `ErrorKind::ALL`
/// walked, each code from `api::rpc_code`, each sample from `Error::sample` — no code and
/// no message is retyped here.
///
/// Every method carries the **whole** registry rather than a subset. Which D13 variants a
/// given operation can actually produce is a fact about the daemon's routing (P4b, P4c),
/// not about the trait, and a subset invented here would be a claim with nothing behind
/// it. What the trait does know it says in prose: each method's `# Errors` section is in
/// its `description`.
fn openrpc() -> Result<Value> {
    use schema::error::{Error, ErrorKind};

    // A second generator, pointed at OpenRPC's schema section instead of `$defs`. Two
    // generators are not two sources of truth: both read the same `schemars` derives on
    // the same Rust types, and the only thing that differs is where a `$ref` has to point
    // for each document to stand on its own.
    let mut generator = SchemaGenerator::new(SchemaSettings::draft2020_12().with(|settings| {
        settings.definitions_path = OPENRPC_SCHEMAS.into();
    }));

    let mut errors = serde_json::Map::new();
    let mut error_refs: Vec<Value> = Vec::new();
    for &kind in ErrorKind::ALL {
        let sample = Error::sample(kind);
        let name = error_component_name(kind)?;
        error_refs.push(json!({ "$ref": format!("#{OPENRPC_ERRORS}/{name}") }));
        errors.insert(
            name,
            json!({
                "code": api::rpc_code(kind),
                // A representative rendering and a representative payload, from the
                // registry's own walkable population. The real message is per-occurrence —
                // it is the variant's `Display`, filled in with the device's answer — so a
                // fixed sentence here would be a second rendering of something D13 says
                // has exactly one.
                "message": sample.to_string(),
                "data": serde_json::to_value(&sample)?,
            }),
        );
    }

    let error_names = d13_wire_names()?;
    let mut methods: Vec<Value> = Vec::with_capacity(api::METHODS.len());
    for method in api::METHODS {
        let mut params: Vec<Value> = Vec::with_capacity(method.params.len());
        for param in method.params {
            params.push(json!({
                "name": param.name,
                // Asked of the parameter's type, never asserted here. `TypeRef` owns the
                // question because `crates/api` is where it can be *checked* against the
                // real `RpcModule` — which is what
                // `an_optional_parameter_may_be_left_out_and_a_required_one_may_not` does.
                // A constant `true` here would publish a document declaring invalid a
                // request the daemon serves, and nothing could notice, because the value
                // would be the emitter's opinion rather than the type's.
                "required": !param.ty.admits_absence(),
                "schema": serde_json::to_value(param.ty.schema(&mut generator))?,
            }));
        }
        methods.push(json!({
            "name": method.name,
            "summary": name_d13_errors(&method.summary(), &error_names),
            "description": name_d13_errors(&method.description(), &error_names),
            // Named parameters are D10's posture for the whole surface, not a per-method
            // choice. `by-name` rather than `either` because the server's positional path
            // is not uniform: an exhausted sequence is not an absent optional, so
            // `wch_calibrate_list` with `[]` is refused where `wch_info` with `["cam:x"]`
            // is served. The document commits to the shape that always works.
            "paramStructure": "by-name",
            "params": params,
            "result": {
                "name": method.result.name(),
                "required": true,
                "schema": serde_json::to_value(method.result.schema(&mut generator))?,
            },
            "errors": error_refs,
        }));
    }

    // The subscriptions, from the second inventory `wire_surface!` emits — see this
    // function's doc for why they are not in `methods`. The item schema is asked of the
    // Rust type through the *same* generator, so a subscription payload's `$ref` resolves
    // inside this document exactly as a method parameter's does.
    let mut subscriptions: Vec<Value> = Vec::with_capacity(api::SUBSCRIPTIONS.len());
    for subscription in api::SUBSCRIPTIONS {
        subscriptions.push(json!({
            "name": subscription.name,
            "unsubscribe": subscription.unsubscribe,
            // The method name each notification arrives under, which is not derivable from
            // the other two by a consumer that does not know jsonrpsee's defaults.
            "notification": subscription.notification,
            "summary": name_d13_errors(&subscription.summary(), &error_names),
            "description": name_d13_errors(&subscription.description(), &error_names),
            // A named content descriptor, like a method's `result`: a consumer generating a
            // client needs something to call the payload.
            "item": {
                "name": subscription.item.name(),
                "required": true,
                "schema": serde_json::to_value(subscription.item.schema(&mut generator))?,
            },
            "errors": error_refs,
        }));
    }

    // The shape of every `data` above, named once instead of nineteen times.
    let error_data = serde_json::to_value(generator.subschema_for::<Error>())?;

    let definitions = generator.take_definitions(true);

    Ok(json!({
        "openrpc": OPENRPC_VERSION,
        "info": {
            "title": "webcam-handler",
            // The tool version, not a protocol version: the wire's compatibility contract
            // is the method names, the parameter names and the D13 codes, and each of
            // those is a diff in this file when it changes.
            "version": schema::TOOL_VERSION,
            "license": { "name": env!("CARGO_PKG_LICENSE") },
            "description": "The webcam-handler daemon's JSON-RPC surface (design D10, T5), \
                            generated from the Rust trait and committed as documentation — \
                            never read back, never a second source of truth. The daemon \
                            listens on a Unix domain socket (D11), so this document \
                            describes methods rather than a URL. Errors are the closed D13 \
                            registry under `components/errors`: `code` from a closed \
                            numeric range, `message` the error's own rendering, `data` the \
                            typed error itself. The daemon also carries server-initiated \
                            streams, which OpenRPC 1.3.2 has no shape for: they are described \
                            under the `x-subscriptions` extension, reached over a WebSocket \
                            upgrade on the same socket, and each one's `item` is the payload \
                            of one notification.",
        },
        "methods": methods,
        "x-subscriptions": subscriptions,
        "components": {
            "errors": errors,
            "schemas": definitions,
        },
        "x-d13-error-data": error_data,
    }))
}

/// The key an error kind lands under in `components/errors`.
///
/// Its own serde spelling, so the document cannot name a variant the registry does not —
/// the same string `data`'s `kind` tag carries, and the same one
/// `crates/api/fixtures/d13-rpc-codes.tsv` pins the code against.
fn error_component_name(kind: schema::error::ErrorKind) -> Result<String> {
    match serde_json::to_value(kind)? {
        Value::String(name) => Ok(name),
        other => bail!("{kind:?} serializes as {other}, which is not a component name"),
    }
}

/// Apply `rewrite` to every `description` and `summary` an artifact carries.
///
/// Two keys rather than one because the OpenRPC document puts a method's first sentence in
/// `summary` and the rest in `description`, so a rule applied to only one of them would
/// hold for half of every doc comment. A walker taking the rewrite rather than performing
/// one, because "prose in these files is written for a consumer, not for rustdoc" is a rule
/// with more than one clause and a second walk would be a second place to state it
/// differently (design §2.10).
fn rewrite_prose(value: &mut Value, rewrite: &dyn Fn(&str) -> String) {
    match value {
        Value::Object(map) => {
            for (key, child) in map.iter_mut() {
                if matches!(key.as_str(), "description" | "summary")
                    && let Value::String(text) = child
                {
                    *text = rewrite(text);
                } else {
                    rewrite_prose(child, rewrite);
                }
            }
        }
        Value::Array(items) => items
            .iter_mut()
            .for_each(|item| rewrite_prose(item, rewrite)),
        _ => {}
    }
}

/// Undo rustdoc's bracket escaping.
///
/// Doc comments write `\[PF:8\]` because rustdoc reads `[PF:8]` as a shortcut reference
/// link and `-D warnings` turns the unresolved target into an error. The backslashes are
/// an artifact of that escape, not content: rustdoc renders them away, and a consumer of
/// these files should see the same citation the design documents use.
fn unescape_doc_brackets(text: &str) -> String {
    text.replace("\\[", "[").replace("\\]", "]")
}

/// Rewrite D13 citations into the spelling the OpenRPC document keys them under.
///
/// A method's doc comment cites a variant the way a Rust reader resolves it —
/// ``[`schema::Error::SettleTimeout`]``, an intra-doc link — while the document keys the
/// same error `#/components/errors/settle_timeout`. Every method carries the *whole*
/// registry by design, so its `# Errors` prose is the only per-method error information a
/// consumer has, and a citation ending in an identifier that appears nowhere else in the
/// file is a dead end. The citation is rewritten rather than duplicated: the Rust source
/// keeps the link a Rust reader can follow, and the document gets the name it can look up.
///
/// **Applied to the method and subscription prose only**, which is where the mismatch is. The
/// DTO descriptions in either artifact sit beside the vocabulary itself — `ErrorKind`'s
/// alternatives *are* `"const": "device_gone"` — so rewriting there turns a link into
/// "See `device_gone`" one line above `"const": "device_gone"`, which is a sentence with
/// nothing in it.
///
/// This ran over the whole document for one repair, on the argument that the narrow scope left
/// ``[`crate::Error::Busy`]`` behind in a `PhotoRequest.wait` description — a dead identifier
/// for the reader D10 commits these files for. That was a real defect and this was the wrong
/// fix for it: [`unlink_citations`] runs over the whole document and turns the same citation
/// into `` `Error::Busy` ``, a name the reader can find under `$defs/Error`, while the
/// eighteen `ErrorKind` descriptions keep saying which variant they are about (docs/11
/// **M22**, notes **N218** and **N222**).
///
/// `names` maps a variant's Rust spelling to its wire one and is built by walking
/// `ErrorKind::ALL` — the vocabulary, not a table here — so a variant this cannot rewrite
/// is one the registry does not have, and is left exactly as written rather than guessed
/// at.
fn name_d13_errors(text: &str, names: &BTreeMap<String, String>) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find("[`") {
        let (before, from_open) = rest.split_at(open);
        out.push_str(before);
        let inner_start = &from_open["[`".len()..];
        let Some(close) = inner_start.find("`]") else {
            // An unterminated citation is prose, not a citation. Emit the rest verbatim.
            out.push_str(from_open);
            return out;
        };
        let (inner, after) = inner_start.split_at(close);
        match d13_variant(inner).and_then(|variant| names.get(variant)) {
            Some(wire) => out.push_str(&format!("`{wire}`")),
            None => out.push_str(&format!("[`{inner}`]")),
        }
        rest = &after["`]".len()..];
    }
    out.push_str(rest);
    out
}

/// The paths a doc comment resolves the **D13 registry** through.
///
/// Three, because three crates write these comments and each reaches the one type
/// differently: `schema::error::Error` is `Error` inside its own crate, `crate::Error` from
/// a sibling module, and `schema::Error` from `webcam-handler-api`. Matching any path that
/// merely *ends* in `::Error` would be wider than the claim — a future `pairing::Error`
/// with a `Busy` variant would be rewritten into D13's spelling of somebody else's error —
/// so the set is exact and the inverse arm of
/// `a_d13_citation_becomes_the_name_the_document_keys_it_under_and_nothing_else_moves` is
/// what keeps it that way.
const D13_CITATION_PATHS: &[&str] = &["Error", "crate::Error", "schema::Error"];

/// The variant a D13 citation names.
///
/// Anything else — ``[`ControlReport`]``, ``[`Sink::ReturnBytes`]``, another crate's
/// `Error` — is not a D13 citation and gets `None`, which is what leaves it alone.
fn d13_variant(citation: &str) -> Option<&str> {
    let (path, variant) = citation.rsplit_once("::")?;
    D13_CITATION_PATHS.contains(&path).then_some(variant)
}

/// Every D13 variant's Rust spelling, mapped to the one the artifacts key it under.
///
/// Walked from `ErrorKind::ALL`, which the vocabulary macro generates, so this cannot fall
/// behind the registry — and neither side of a pair is retyped: the Rust name is the
/// variant's own `Debug`, and the wire name is [`error_component_name`]'s.
fn d13_wire_names() -> Result<BTreeMap<String, String>> {
    schema::error::ErrorKind::ALL
        .iter()
        .map(|&kind| Ok((format!("{kind:?}"), error_component_name(kind)?)))
        .collect()
}

/// Undo rustdoc's *linking*, which is an instruction to a tool that is not running.
///
/// ``[`RecordRequest::container`]`` reaches a reader of `schemas/*.json` as brackets around
/// an identifier they cannot open — note **N123**'s finding about clap help, at the two
/// artifacts D10 commits **so a consumer needs no Rust toolchain** (docs/11 **M22**, note
/// **N218**). The committed files carried 124 and 103 of them.
///
/// The citation keeps its text and loses its brackets, so what is left is a code span naming
/// the thing: `RecordRequest::container`. That is the honest residue — most of these name a
/// type the document really has, keyed under `$defs` or `components/schemas`, and the reader
/// can find it. What it must not become is a *claim*: this does not rewrite the name into a
/// JSON pointer, because a field's Rust name and its serde name are not always the same
/// string and a wrong pointer is worse than a plain name.
///
/// A leading `crate::` goes with the brackets — and with **no** brackets, which is the half
/// that had to be added (docs/11 §9.3's class, note **N222**). It is a Rust-internal spelling,
/// meaning "whichever crate this comment was written in", and it means nothing at all outside
/// the source tree; whether the author reached for a link or for a plain code span does not
/// change that. The first pass below reads the linked form and the second the bare one, and
/// the second is why `` `crate::limits::MAX_RECORDING_MS` `` cannot be published as written.
///
/// A citation spelled as a **real markdown link** — ``[`X`](crate::X)`` — carries its target
/// as well as its brackets, and rustdoc accepts both spellings for one thing. Dropping the
/// brackets alone would leave `` `X`(crate::X) ``: the instruction to the absent tool, now
/// promoted to prose. The target goes with the link.
///
/// Runs **after** [`name_d13_errors`], so a D13 citation in a method's prose has already
/// become the name the document keys it under and never arrives here.
fn unlink_citations(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find("[`") {
        let (before, from_open) = rest.split_at(open);
        out.push_str(before);
        let inner_start = &from_open["[`".len()..];
        let Some(close) = inner_start.find("`]") else {
            // An unterminated citation is prose, not a citation — the same reading
            // `name_d13_errors` takes, and for its reason: prose is not required to be
            // well-formed markdown, and an emitter that truncated it would lose the rest.
            out.push_str(from_open);
            return out;
        };
        let (inner, after) = inner_start.split_at(close);
        out.push('`');
        out.push_str(inner.strip_prefix("crate::").unwrap_or(inner));
        out.push('`');
        rest = &after["`]".len()..];
        // The link form's target, when there is one. A parenthetical that is *not* a target
        // is separated from the citation by a space and is left alone, which is what keeps
        // this from eating the sentence after a bracket.
        if let Some(target) = rest.strip_prefix('(')
            && let Some(end) = target.find(')')
        {
            rest = &target[end + ")".len()..];
        }
    }
    out.push_str(rest);
    unqualify_crate_paths(&out)
}

/// Drop the `crate::` from a code span that never was a link.
///
/// [`unlink_citations`]'s second half, split out because the two read different syntax for one
/// rule and a single loop doing both would have to say which it was in the middle of. Keyed on
/// the opening backtick and the prefix together, so a `crate::` in running prose — which no
/// doc comment in this workspace writes — is not this function's business, and an unterminated
/// span is prose rather than a span, as everywhere else here.
fn unqualify_crate_paths(text: &str) -> String {
    const QUALIFIED: &str = "`crate::";
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find(QUALIFIED) {
        let (before, from_open) = rest.split_at(open);
        out.push_str(before);
        let named = &from_open[QUALIFIED.len()..];
        if !named.contains('`') {
            out.push_str(from_open);
            return out;
        }
        out.push('`');
        rest = named;
    }
    out.push_str(rest);
    out
}

/// The bytes one generated artifact is committed as.
///
/// Pretty-printed with a trailing newline, and with the prose rewritten for the consumer
/// who reads these files without a Rust toolchain: these files are diffed by a gate and
/// read by humans, and `serde_json`'s default map is a `BTreeMap`, so object key order is
/// stable across runs. Arrays are the emitter's own business — `methods` is in the trait's
/// declaration order and the error list is in `ErrorKind::ALL`'s, so neither can shuffle
/// between runs and make the gate flap.
///
/// Two rewrites over every prose string, in an order that matters: the escaping rustdoc made
/// us write comes off first, and only then is what is left un-linked. The third,
/// [`name_d13_errors`], is applied where the document is built rather than here, because it is
/// the *method* prose that keys errors by their wire name — see its own doc for the scope
/// argument, which has been made twice now.
fn artifact_text(mut value: Value) -> Result<String> {
    rewrite_prose(&mut value, &unescape_doc_brackets);
    rewrite_prose(&mut value, &unlink_citations);
    let mut text = serde_json::to_string_pretty(&value)?;
    text.push('\n');
    Ok(text)
}

/// Write one generated artifact.
///
/// One helper rather than a write per artifact, because "generated artifacts are
/// unescaped, pretty-printed and newline-terminated" is a rule about every file the gate
/// diffs, and a second artifact that restated it could restate it differently.
fn write_artifact(out_dir: &Utf8Path, name: &str, value: Value) -> Result<()> {
    let path = out_dir.join(name);
    std::fs::write(&path, artifact_text(value)?).with_context(|| format!("writing {path}"))?;
    println!("wrote {path}");
    Ok(())
}

/// Write one generated file whose bytes are already text.
///
/// The guide's path carries a directory, which the JSON artifacts' names do not, so the
/// parent is created here rather than once for the whole run: `root` is the repository root
/// or a scratch stand-in for it, and a stand-in has nothing under it yet.
fn write_text(root: &Utf8Path, relative: &str, text: &str) -> Result<()> {
    let path = root.join(relative);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("creating {parent}"))?;
    }
    std::fs::write(&path, text).with_context(|| format!("writing {path}"))?;
    println!("wrote {path}");
    Ok(())
}

/// Write every generated artifact under `root`, at the path each is committed at.
fn generate(root: &Utf8Path) -> Result<()> {
    let artifacts = root.join(ARTIFACT_DIR);
    std::fs::create_dir_all(&artifacts).with_context(|| format!("creating {artifacts}"))?;

    write_artifact(&artifacts, BUNDLE, bundle()?)?;
    write_artifact(&artifacts, OPENRPC, openrpc()?)?;
    write_text(root, guide::GUIDE_PATH, &guide::guide()?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use schema::error::{Error, ErrorKind};

    use super::*;

    /// Every artifact this emitter writes, by the name it is committed under.
    ///
    /// Walked rather than named per test, so a third artifact joins the suite by being
    /// emitted rather than by somebody remembering to add it here.
    fn artifacts() -> Result<Vec<(&'static str, Value)>> {
        Ok(vec![(BUNDLE, bundle()?), (OPENRPC, openrpc()?)])
    }

    /// Every `$ref` in `value`, in document order.
    fn references(value: &Value, found: &mut Vec<String>) {
        match value {
            Value::Object(map) => {
                for (key, child) in map {
                    if key == "$ref"
                        && let Value::String(target) = child
                    {
                        found.push(target.clone());
                    } else {
                        references(child, found);
                    }
                }
            }
            Value::Array(items) => items.iter().for_each(|item| references(item, found)),
            _ => {}
        }
    }

    #[test]
    fn every_reference_a_generated_artifact_emits_resolves_inside_it() {
        // A committed document is what a consumer without a Rust toolchain validates
        // against, so a `$ref` that points at nothing — or at a second file they were
        // never told to fetch — is the artifact failing at the one job it has. The gate
        // diffs these files; it cannot read them.
        for (name, document) in artifacts().expect("the artifacts are emitted") {
            let mut found = Vec::new();
            references(&document, &mut found);
            // Not a vacuous pass: both artifacts are almost entirely cross-references, so
            // a walk that found none would mean the walker, not the document, is broken.
            assert!(
                found.len() > 100,
                "{name} emitted only {} references; the walk is not walking",
                found.len()
            );

            for target in &found {
                let pointer = target
                    .strip_prefix('#')
                    .unwrap_or_else(|| panic!("{name} points outside itself: {target}"));
                assert!(
                    document.pointer(pointer).is_some(),
                    "{name} references {target}, which resolves to nothing"
                );
            }
        }
    }

    #[test]
    fn the_openrpc_document_describes_exactly_the_methods_the_wire_trait_carries() {
        let document = openrpc().expect("the document is emitted");
        let methods = document["methods"]
            .as_array()
            .expect("methods is an array")
            .clone();

        // The population is `webcam-handler-api`'s own inventory, which that crate
        // declares in the same tokens as the trait — so this compares the document with
        // the wire surface rather than with a second opinion about it. A count on its own
        // would pass a document that described nineteen of the wrong methods.
        let names = d13_wire_names().expect("the registry names itself");
        assert_eq!(methods.len(), api::METHODS.len(), "{methods:?}");
        for (emitted, method) in methods.iter().zip(api::METHODS) {
            assert_eq!(emitted["name"], json!(method.name));
            // Through the citation rewrite, so a first sentence that ever names an error
            // fails this for the right reason rather than for the spelling.
            assert_eq!(
                emitted["summary"],
                json!(name_d13_errors(&method.summary(), &names))
            );
            assert_eq!(
                emitted["params"]
                    .as_array()
                    .expect("params is an array")
                    .len(),
                method.params.len(),
                "{} lost a parameter on the way into the document",
                method.name
            );
            // The result is a named content descriptor, not a bare schema: an OpenRPC
            // consumer generating a client needs something to call the return value.
            assert_eq!(emitted["result"]["name"], json!(method.result.name()));
        }
    }

    #[test]
    fn the_openrpc_document_describes_every_subscription_and_names_none_of_them_a_method() {
        // The other half of the walk above, over the population `methods` deliberately does
        // not carry — see `openrpc`'s doc for the three encodings and why this is the one
        // that publishes nothing false. Both directions matter and both are here: every row
        // of `api::SUBSCRIPTIONS` is described, and no subscription spelling appears among
        // the methods, which is what would make a stock OpenRPC client generate a call the
        // HTTP half of this socket answers `-32603`.
        let document = openrpc().expect("the document is emitted");
        let described = document["x-subscriptions"]
            .as_array()
            .expect("x-subscriptions is an array");

        assert_eq!(described.len(), api::SUBSCRIPTIONS.len(), "{described:?}");
        assert!(!described.is_empty(), "the surface subscribes to nothing");
        for (emitted, subscription) in described.iter().zip(api::SUBSCRIPTIONS) {
            assert_eq!(emitted["name"], json!(subscription.name));
            // Both wire names, because a consumer that cannot close a stream is a consumer
            // that leaks one — and the unsubscribe spelling is jsonrpsee's derivation
            // rather than ours, so a document that omitted it would leave a client
            // guessing at it.
            assert_eq!(emitted["unsubscribe"], json!(subscription.unsubscribe));
            assert_eq!(emitted["notification"], json!(subscription.notification));
            // The item is the whole reason this section exists: a subscribe call emitted as
            // a method would carry an opaque id and say nothing about the payload.
            assert_eq!(emitted["item"]["name"], json!(subscription.item.name()));
            assert!(
                emitted["item"]["schema"].is_object(),
                "{} carries no item schema",
                subscription.name
            );
        }

        let methods = document["methods"].as_array().expect("methods is an array");
        let named: Vec<&str> = methods
            .iter()
            .filter_map(|method| method["name"].as_str())
            .collect();
        for subscription in api::SUBSCRIPTIONS {
            for spelling in subscription.names() {
                assert!(
                    !named.contains(&spelling),
                    "{spelling} is described as a method, which promises a call the HTTP \
                     half of this socket refuses"
                );
            }
        }
    }

    #[test]
    fn every_subscriptions_payload_is_a_root_of_the_bundle_and_not_only_of_the_document() {
        // **The law `bundle()` states, checked from the other end.** A subscription's item
        // is a root because a notification payload is the one thing besides `--json` that a
        // consumer validates our output against — and the two artifacts have two audiences,
        // so a payload that reached the OpenRPC document and not the bundle would leave a
        // consumer following the bundle's own contract with nothing to validate against.
        //
        // Written even though `bundle()` now *derives* the roots, because the walk is the
        // law and the derivation is only this build's way of obeying it: the hand list it
        // replaced looked equally obeyed, and a third subscription reached
        // `x-subscriptions` while reaching neither `x-roots` nor `$defs`, with all eight
        // xtask tests green (measured — note **N59**). This is the assertion that was
        // missing, and it fails on that experiment.
        let bundle = bundle().expect("the bundle is emitted");
        let roots = bundle["x-roots"].as_array().expect("x-roots is an array");
        let definitions = bundle["$defs"].as_object().expect("$defs is an object");

        assert!(
            !api::SUBSCRIPTIONS.is_empty(),
            "the surface subscribes to nothing"
        );
        for subscription in api::SUBSCRIPTIONS {
            let item = subscription.item.name();
            assert!(
                roots.contains(&json!(item)),
                "{}'s payload {item} is not a root of the bundle",
                subscription.name
            );
            assert!(
                definitions.contains_key(item.as_ref()),
                "{}'s payload {item} is named as a root and defined nowhere",
                subscription.name
            );
        }
    }

    #[test]
    fn every_d13_variant_reaches_the_document_with_the_code_the_registry_gives_it() {
        let document = openrpc().expect("the document is emitted");
        let errors = document["components"]["errors"]
            .as_object()
            .expect("the error registry is an object");

        // `ErrorKind::ALL` is generated by the vocabulary macro, so this walk cannot shrink
        // when a variant is added — and `api::rpc_code` is an exhaustive match, so the
        // code it answers with is the one the daemon will actually send. Neither number
        // nor name is retyped in this file.
        assert_eq!(errors.len(), ErrorKind::ALL.len());
        for &kind in ErrorKind::ALL {
            let name = error_component_name(kind).expect("a kind names itself");
            let emitted = errors
                .get(&name)
                .unwrap_or_else(|| panic!("{name} has no entry in the document"));
            assert_eq!(emitted["code"], json!(api::rpc_code(kind)), "{name}");
            // The message is the error's own rendering, which is what D13 says crosses the
            // wire — a document that showed a different sentence would be teaching a
            // consumer to match on a string nobody sends.
            assert_eq!(
                emitted["message"],
                json!(Error::sample(kind).to_string()),
                "{name}"
            );
            assert_eq!(emitted["data"]["kind"], json!(name), "{name}");
        }
    }

    #[test]
    fn no_document_a_verb_answers_with_can_be_mistaken_for_the_failure_document() {
        // **The half of the owner's 2026-08-15 ruling that has to be *checked* rather than
        // observed** (note **N127**): "no success document may be mistakable for it". A
        // failure is told apart from an answer by one property name —
        // `schema::error::FAILURE_MARKER` — and that discrimination is worth nothing unless
        // no answer can carry it.
        //
        // Both populations are walked and neither is a hand list. The verbs come from
        // `crates/cli-core/json-contracts.tsv`, which since P6e is the one home of the
        // verb-to-document mapping (note **N122**), and the shapes come from the bundle this
        // emitter has just produced — so a field added to a `--json` answer is judged here on
        // the day it lands, without anybody remembering that this rule exists.
        //
        // This is the emitter's business rather than `webcam-handler-cli-core`'s because the
        // question is about the *shapes*, and the shapes are `schemars`' answer about the Rust
        // types. `scripts/gates/json-validates.sh` asks the same question of the documents the
        // shipped binary actually prints, which is the other end of it.
        let marker = schema::error::FAILURE_MARKER;
        let bundle = bundle().expect("the bundle is emitted");
        let definitions = bundle["$defs"].as_object().expect("$defs is an object");

        let mut inspected = 0;
        for contract in cli_core::contracts::json_contracts() {
            let schema = definitions.get(contract.document).unwrap_or_else(|| {
                panic!(
                    "{} answers with {}, which the bundle does not define",
                    contract.verb, contract.document
                )
            });
            // `properties` is what serde will emit for a struct, so a document type that does
            // not declare the marker cannot print it. A type with no `properties` at all is
            // not an object and cannot carry a named field either.
            if let Some(properties) = schema.get("properties").and_then(Value::as_object) {
                assert!(
                    !properties.contains_key(marker),
                    "`{}` answers with {}, which declares a `{marker}` property — a caller \
                     branching on the failure marker would read a successful answer as a \
                     refusal",
                    contract.verb,
                    contract.document
                );
            }
            inspected += 1;
        }
        assert!(inspected > 10, "{inspected} contract(s) inspected");

        // Not vacuous, and the reason the walk above means anything: the failure document
        // *is* in the bundle, it *does* declare the marker, and it requires it — so the
        // property being checked is one that exists rather than one nobody emits.
        let failure = definitions
            .get("Failure")
            .expect("the failure document is a bundle type");
        assert!(
            failure["properties"]
                .as_object()
                .expect("Failure is an object")
                .contains_key(marker),
            "the failure document no longer declares `{marker}`"
        );
        for required in [marker, "error", "message"] {
            assert!(
                failure["required"]
                    .as_array()
                    .expect("Failure requires fields")
                    .contains(&json!(required)),
                "the failure document no longer requires `{required}`; a consumer cannot \
                 branch on a field that may be absent"
            );
        }
        assert!(
            bundle["x-roots"]
                .as_array()
                .expect("x-roots is an array")
                .contains(&json!("Failure")),
            "the failure document is defined and is not a root; it is what `--json` prints on \
             every failing run"
        );
    }

    #[test]
    fn the_artifacts_are_byte_identical_across_two_runs() {
        // The gate regenerates and diffs, so an emitter that shuffled a map or a vector
        // between runs would fail CI at random and teach everybody to re-run it. Object
        // keys sort themselves (`serde_json`'s map is a `BTreeMap`); the arrays are this
        // emitter's own, and this is what says so.
        for ((name, first), (_, second)) in artifacts()
            .expect("the artifacts are emitted")
            .into_iter()
            .zip(artifacts().expect("the artifacts are emitted again"))
        {
            assert_eq!(
                artifact_text(first).expect("the first run renders"),
                artifact_text(second).expect("the second run renders"),
                "{name} is not stable across runs"
            );
        }
    }

    #[test]
    fn rustdoc_escaping_is_undone_in_a_summary_as_well_as_a_description() {
        // The widening this document needed: an OpenRPC method's first sentence lands in
        // `summary`, so a citation that fell there would keep the backslashes rustdoc made
        // us write. The third key is the inverse arm — the rule is about the prose keys,
        // not about every string in the file, and a `name` that read `[PF:8]` would be a
        // method nobody could call.
        let mut value = json!({
            "summary": "A summary citing \\[PF:8\\].",
            "description": "A description citing \\[N:10\\].",
            "name": "left \\[alone\\]",
        });
        rewrite_prose(&mut value, &|text| unescape_doc_brackets(text));
        assert_eq!(value["summary"], json!("A summary citing [PF:8]."));
        assert_eq!(value["description"], json!("A description citing [N:10]."));
        assert_eq!(value["name"], json!("left \\[alone\\]"));
    }

    #[test]
    fn a_d13_citation_becomes_the_name_the_document_keys_it_under_and_nothing_else_moves() {
        let names = d13_wire_names().expect("the registry names itself");

        // The three spellings the workspace's doc comments actually use, all rewritten to
        // the one string a consumer can look up in `components/errors` — measured, not
        // assumed: those are the prefixes the emitted artifacts carried before this.
        assert_eq!(
            name_d13_errors(
                "[`schema::Error::SettleTimeout`], [`crate::Error::Busy`] and \
                 [`Error::HolderGone`] refuse it.",
                &names
            ),
            "`settle_timeout`, `busy` and `holder_gone` refuse it."
        );

        // The inverse arm, and it matters more than the positive one: this rewrite must
        // touch nothing but D13 citations. A type link, a variant of some *other* enum, and
        // an unterminated bracket all survive intact — the last one because prose is not
        // required to be well-formed markdown and an emitter that truncated it would lose
        // whatever came after.
        for untouched in [
            "[`ControlReport`] is the answer.",
            "[`Sink::ReturnBytes`] hands them back.",
            "[`schema::Error::NoSuchVariant`] is not in the registry.",
            "[`crate::pairing::Error::Busy`] is somebody else's Error.",
            "an unterminated [`citation",
            "no citation at all",
        ] {
            assert_eq!(name_d13_errors(untouched, &names), untouched);
        }
    }

    #[test]
    fn no_prose_in_a_committed_artifact_speaks_to_a_toolchain_that_is_not_there() {
        // **The whole of docs/11 M22**, and the reason it is asserted over the *rendered*
        // artifacts rather than over the rewrite: D10 commits these two files so a consumer
        // needs no Rust toolchain, and ``[`RecordRequest::container`]`` reaches that consumer
        // as brackets around an identifier they cannot open. Note **N123** found the class in
        // clap help and note **N148** measured this pile — 124 links in the document and 103
        // in the bundle — and said not to add to it. This is what empties it and what keeps
        // it empty.
        //
        // Every string, not only `description` and `summary`: the walker `rewrite_prose` uses
        // is keyed on those two, so a link that landed anywhere else would be a link no
        // rewrite reaches, which is exactly the thing worth knowing.
        //
        // **Two spellings, because a link is not the only way to address a tool that is not
        // running** (note **N222**). The first pass of this scanned for `` [` `` alone, and a
        // doc comment written the same week published `` `crate::limits::MAX_RECORDING_MS` ``
        // into `$defs/Occupation` in both files — the identical defect, in the batch that
        // named it, invisible to the check that had just been written for it. A leading
        // `crate::` resolves for exactly one reader and these files are committed for the
        // other one.
        const ADDRESSED_TO_RUSTDOC: &[(&str, &str)] = &[
            (
                "[`",
                "rustdoc links, which promise a reader with no toolchain a page",
            ),
            (
                "crate::",
                "`crate::` paths, which name a crate a reader with no toolchain has no copy of",
            ),
        ];
        for (name, document) in artifacts().expect("the artifacts are emitted") {
            let text = artifact_text(document).expect("the artifact renders");
            for (spelling, what) in ADDRESSED_TO_RUSTDOC {
                let found: Vec<&str> = text
                    .lines()
                    .filter(|line| line.contains(spelling))
                    .map(str::trim)
                    .collect();
                assert!(
                    found.is_empty(),
                    "{name} publishes {what}, on {} line(s):\n{}",
                    found.len(),
                    found.join("\n")
                );
            }
        }
    }

    #[test]
    fn a_citation_loses_its_link_and_keeps_its_name_and_nothing_else_moves() {
        // The rewrite as a string function, with its inverse arms — the citation keeps the
        // name a reader can look up in `$defs`, loses the brackets that promised a link, and
        // loses a `crate::` prefix that means "whichever crate this comment was written in".
        assert_eq!(
            unlink_citations("[`RecordRequest::container`] decides the file."),
            "`RecordRequest::container` decides the file."
        );
        assert_eq!(
            unlink_citations("[`crate::limits::MAX_SWEEP_SAMPLES`] caps it."),
            "`limits::MAX_SWEEP_SAMPLES` caps it."
        );
        assert_eq!(
            unlink_citations("[`A`] and [`B`]"),
            "`A` and `B`",
            "a line with two citations keeps both"
        );
        // A code span that never was a link carries the same dead prefix and until 2026-08-17
        // kept it, which is how `` `crate::limits::MAX_RECORDING_MS` `` reached both committed
        // files (note **N222**).
        assert_eq!(
            unlink_citations("capped by `crate::limits::MAX_RECORDING_MS` at the outside"),
            "capped by `limits::MAX_RECORDING_MS` at the outside"
        );
        // And the link spelling of a citation loses its target with its brackets. Leaving it
        // published `` `X`(crate::X) ``: the same instruction to the same absent tool, no
        // longer even wearing the syntax that says it is one. The shape is in this tree —
        // `schema::capture` and `schema::camera` write it — and no item carrying it is
        // published today, which is why this arm is the only thing standing between the two.
        assert_eq!(
            unlink_citations("[`Error::size_unsupported`](crate::Error::size_unsupported) is."),
            "`Error::size_unsupported` is."
        );

        // …and it touches nothing else. A code span that was never a link, a citation
        // rustdoc's escaping already turned into prose, an unterminated bracket, an
        // unterminated code span, and a real markdown link whose text is not code all survive
        // — the last because a document may legitimately point at a file, and flattening that
        // would remove the only working link in it.
        for untouched in [
            "`wch_record_status` is a method a client calls",
            "a size of 1280x720",
            "an unterminated [`citation",
            "an unterminated `crate::span",
            "[the design](docs/6-claude-fable-design-v2.md) is a document",
            "[PF:15] is a probe finding",
            "no citation at all",
        ] {
            assert_eq!(unlink_citations(untouched), untouched);
        }
    }

    #[test]
    fn no_method_description_cites_a_d13_variant_the_way_rustdoc_spells_it() {
        // The whole-document arm: the rewrite above is a string function, and this is what
        // says it reaches the file. A `schema::Error::SettleTimeout` left in a method's
        // `# Errors` prose is an identifier a toolchain-less consumer cannot resolve to
        // anything else in the document — the registry beside it is keyed `settle_timeout`.
        let document = openrpc().expect("the document is emitted");
        let methods = document["methods"].as_array().expect("methods is an array");
        let mut named = 0;
        for method in methods {
            for key in ["summary", "description"] {
                let prose = method[key].as_str().expect("prose is a string");
                assert!(
                    !prose.contains("Error::"),
                    "{} still cites a D13 variant as Rust: {prose}",
                    method["name"]
                );
                named += usize::from(prose.contains("`camera_unknown`"));
            }
        }
        // Not vacuous: the wire spellings are in there, in prose, where the citations used
        // to be. `camera_unknown` is the one every method's `# Errors` section reaches,
        // directly or through "As `wch_info`".
        assert!(named > 0, "no method names an error the document keys");
    }
}
