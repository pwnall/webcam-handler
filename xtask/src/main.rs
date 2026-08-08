//! Build-side generation.
//!
//! Everything this emits is a **generated artifact**, not a second source of truth: the
//! Rust types are the schema, and these files are what other tools read. They are
//! committed so consumers do not need a Rust toolchain, and
//! `scripts/gates/schema-artifacts-current.sh` re-runs the emitter and diffs, so a
//! committed copy cannot drift from the types it documents.
//!
//! `generate` writes them; `generate --out DIR` writes them somewhere else, which is how
//! the gate compares without touching the tree.
#![forbid(unsafe_code)]

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
            let root = repo_root()?;
            let out = out.unwrap_or_else(|| root.join(ARTIFACT_DIR));
            generate(&out)
        }
        "help" | "--help" | "-h" => {
            println!("usage: xtask generate [--out DIR]");
            Ok(())
        }
        other => bail!("unknown command {other:?}; try `xtask help`"),
    }
}

/// Where generated artifacts live, relative to the repository root.
const ARTIFACT_DIR: &str = "schemas";

/// The bundle's filename.
const BUNDLE: &str = "webcam-handler-schema.json";

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
fn register<T: JsonSchema>(generator: &mut SchemaGenerator, roots: &mut Vec<String>) {
    let _schema = generator.subschema_for::<T>();
    roots.push(T::schema_name().into_owned());
}

fn bundle() -> Result<Value> {
    use schema::camera::{CameraInfo, FormatInfo};
    use schema::capture::{NegotiatedStream, PhotoFormat, Sink, StreamRequest, Transform};
    use schema::control::{Applied, ControlDesc, ControlValue, WriteWarning};
    use schema::error::Error;
    use schema::profile::DeviceProfile;
    use schema::session::{LogEntry, Session};
    use schema::snapshot::{RestoreReport, Snapshot};

    let mut generator = SchemaGenerator::new(SchemaSettings::draft2020_12());
    let mut roots: Vec<String> = Vec::new();

    register::<CameraInfo>(&mut generator, &mut roots);
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
    register::<Snapshot>(&mut generator, &mut roots);
    register::<RestoreReport>(&mut generator, &mut roots);
    register::<Session>(&mut generator, &mut roots);
    register::<LogEntry>(&mut generator, &mut roots);
    register::<DeviceProfile>(&mut generator, &mut roots);
    register::<Error>(&mut generator, &mut roots);

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

fn generate(out_dir: &Utf8Path) -> Result<()> {
    std::fs::create_dir_all(out_dir)
        .with_context(|| format!("creating {out_dir}"))?;

    let bundle = bundle()?;
    // Pretty-printed with a trailing newline: these files are diffed by a gate and read
    // by humans, and serde_json's default map is a BTreeMap, so key order is stable.
    let mut text = serde_json::to_string_pretty(&bundle)?;
    text.push('\n');

    let path = out_dir.join(BUNDLE);
    std::fs::write(&path, text).with_context(|| format!("writing {path}"))?;
    println!("wrote {path}");
    Ok(())
}
