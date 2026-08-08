//! The device-profile corpus loader (design §3.2).
//!
//! `corpus/profiles/` holds T3 profiles captured by the tool from real hardware,
//! committed with provenance and immutable once committed. Tests read them through here
//! so that "which profiles exist" is always a `find` over the directory and never a list
//! somebody maintains — docs/4's second structural rule, and the reason the corpus-floor
//! gate (P1) can count what it covers.
//!
//! **A profile that fails to parse is a loud failure, not a skipped file.** A loader that
//! shrugged at a malformed profile would turn corpus rot into a green run, which is
//! docs/3 Part C's "skip == pass" wearing a filesystem.
//!
//! The hand-authored minimal-repro profile is deliberately *not* here — it lives in
//! [`crate::fixtures`], so this directory stays uniformly tool-captured (§3.2).

use camino::{Utf8Path, Utf8PathBuf};
use schema::error::{Error, Result};
use schema::profile::DeviceProfile;

/// The corpus directory, relative to the repository root.
const PROFILES_SUBDIR: &str = "corpus/profiles";

/// The repository root, found by walking up from this crate's manifest directory.
///
/// The marker is the corpus directory itself: a checkout has one, and a crate vendored
/// into somebody else's tree does not — in which case the caller gets a typed error
/// rather than a silently empty corpus.
pub fn repo_root() -> Result<Utf8PathBuf> {
    let manifest = Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut candidate: Option<&Utf8Path> = Some(manifest.as_path());
    while let Some(dir) = candidate {
        if dir.join(PROFILES_SUBDIR).is_dir() {
            return Ok(dir.to_owned());
        }
        candidate = dir.parent();
    }
    Err(Error::StorageIo {
        path: manifest,
        errno: None,
        message: format!("no ancestor directory contains {PROFILES_SUBDIR}"),
    })
}

/// The directory the committed device profiles live in.
pub fn profiles_dir() -> Result<Utf8PathBuf> {
    Ok(repo_root()?.join(PROFILES_SUBDIR))
}

/// Every committed profile's path, in a stable order.
///
/// Derived from the directory listing. Non-JSON files are ignored (a README belongs in a
/// corpus directory); a `.json` file that is not a profile is a parse failure at load
/// time, not a quiet omission here.
pub fn profile_paths() -> Result<Vec<Utf8PathBuf>> {
    paths_in(&profiles_dir()?)
}

/// How many profiles the corpus holds.
///
/// The number the P1 corpus-floor gate counts against. At P0 it is legitimately zero:
/// real profiles arrive with `wch profile capture` (§3.2), and the hermetic fixture in
/// [`crate::fixtures`] is deliberately not corpus.
pub fn count() -> Result<usize> {
    Ok(profile_paths()?.len())
}

/// Load one committed profile by file name, with or without the `.json` extension.
pub fn load(name: &str) -> Result<DeviceProfile> {
    load_named(&profiles_dir()?, name)
}

/// Load one profile by name from `dir`. [`load`]'s engine, exposed for the same reason
/// [`load_dir`] is: a test needs to be able to point it at a directory it built.
pub fn load_named(dir: &Utf8Path, name: &str) -> Result<DeviceProfile> {
    let stem = name.strip_suffix(".json").unwrap_or(name);
    load_path(&dir.join(format!("{stem}.json")))
}

/// Load every committed profile, paired with the path it came from.
pub fn load_all() -> Result<Vec<(Utf8PathBuf, DeviceProfile)>> {
    load_dir(&profiles_dir()?)
}

/// Load every profile in `dir`. The corpus loader's engine, exposed so a test can point
/// it at a directory it built — including one holding a deliberately malformed profile.
pub fn load_dir(dir: &Utf8Path) -> Result<Vec<(Utf8PathBuf, DeviceProfile)>> {
    paths_in(dir)?
        .into_iter()
        .map(|path| {
            let profile = load_path(&path)?;
            Ok((path, profile))
        })
        .collect()
}

/// Read and parse one profile document.
///
/// Every failure is typed and names the file: unreadable, not a profile at all, and
/// written by a version this build does not understand are three different problems, and
/// D13 keeps them apart.
pub fn load_path(path: &Utf8Path) -> Result<DeviceProfile> {
    let bytes = std::fs::read(path).map_err(|error| storage_error(path, &error))?;
    let profile: DeviceProfile =
        serde_json::from_slice(&bytes).map_err(|error| Error::StorageIo {
            path: path.to_owned(),
            errno: None,
            message: format!("not a device profile: {error}"),
        })?;
    if !profile.version_is_supported() {
        return Err(Error::SchemaVersionForeign {
            found: profile.schema_version,
            supported: schema::limits::PROFILE_SCHEMA_VERSION,
        });
    }
    Ok(profile)
}

/// The `.json` files in `dir`, sorted so every walk over the corpus is reproducible.
fn paths_in(dir: &Utf8Path) -> Result<Vec<Utf8PathBuf>> {
    let entries = std::fs::read_dir(dir).map_err(|error| storage_error(dir, &error))?;
    let mut paths = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| storage_error(dir, &error))?;
        let path = Utf8PathBuf::from_path_buf(entry.path()).map_err(|raw| Error::StorageIo {
            path: dir.to_owned(),
            errno: None,
            message: format!("{} is not a UTF-8 path", raw.display()),
        })?;
        if path.is_file() && path.extension() == Some("json") {
            paths.push(path);
        }
    }
    paths.sort();
    Ok(paths)
}

fn storage_error(path: &Utf8Path, error: &std::io::Error) -> Error {
    Error::StorageIo {
        path: path.to_owned(),
        errno: error.raw_os_error(),
        message: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;

    use super::*;

    #[test]
    fn the_repository_root_is_found_by_walking_up_from_this_crate() {
        let root = repo_root().expect("the corpus directory exists in a checkout");
        assert!(
            root.join("Cargo.toml").is_file(),
            "{root} does not look like the workspace root"
        );
        assert!(root.join(PROFILES_SUBDIR).is_dir());
    }

    #[test]
    fn every_committed_profile_parses() {
        // The corpus is legitimately empty at P0; what this pins is that whatever is in
        // it loads. A profile that stops parsing must break here, not in whichever suite
        // happens to replay it next.
        let loaded = load_all().expect("every committed profile parses");
        assert_eq!(
            loaded.len(),
            count().expect("counting the corpus"),
            "the loader and the counter disagree about the corpus"
        );
        for (path, profile) in loaded {
            assert!(
                profile.version_is_supported(),
                "{path} carries schema_version {}",
                profile.schema_version
            );
        }
    }

    #[test]
    fn a_malformed_profile_is_a_loud_failure_not_a_skipped_file() {
        // The inverse direction of the test above: the loader must not be able to report
        // "all clear" over a directory it could not read.
        let dir = tempfile::tempdir().expect("temp dir");
        let root = Utf8PathBuf::from_path_buf(dir.path().to_owned()).expect("UTF-8 temp dir");

        let good = root.join("good.json");
        std::fs::write(
            &good,
            serde_json::to_vec(&crate::fixtures::synthetic_basic()).expect("serialize"),
        )
        .expect("write");
        assert_eq!(load_dir(&root).expect("one good profile").len(), 1);

        let bad = root.join("bad.json");
        let mut file = std::fs::File::create(&bad).expect("create");
        file.write_all(b"{\"schema_version\": 1, \"provenance\": ")
            .expect("write");
        drop(file);

        let error = load_dir(&root).expect_err("a truncated profile must fail loudly");
        assert!(
            matches!(&error, Error::StorageIo { path, .. } if path == &bad),
            "the error must name the file: {error}"
        );
    }

    #[test]
    fn a_profile_from_a_future_version_is_refused_by_version_before_anything_else() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = Utf8PathBuf::from_path_buf(dir.path().to_owned()).expect("UTF-8 temp dir");
        let mut profile = crate::fixtures::synthetic_basic();
        profile.schema_version = schema::limits::PROFILE_SCHEMA_VERSION + 1;
        std::fs::write(
            root.join("future.json"),
            serde_json::to_vec(&profile).expect("serialize"),
        )
        .expect("write");

        let error = load_dir(&root).expect_err("a foreign version must be refused");
        assert!(
            matches!(error, Error::SchemaVersionForeign { .. }),
            "expected a version refusal, got {error}"
        );
    }

    #[test]
    fn a_profile_loads_by_name_and_a_name_nothing_answers_to_says_which_file_it_looked_for() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = Utf8PathBuf::from_path_buf(dir.path().to_owned()).expect("UTF-8 temp dir");
        std::fs::write(
            root.join("obsbot-tiny3.json"),
            crate::fixtures::synthetic_basic_json(),
        )
        .expect("write");

        // With and without the extension, because both spellings reach this from a
        // command line.
        assert!(load_named(&root, "obsbot-tiny3").is_ok());
        assert!(load_named(&root, "obsbot-tiny3.json").is_ok());

        // The other direction, and the reason the error carries a path: "no such
        // profile" is unactionable without the name it tried.
        let error = load_named(&root, "chicony-ir").expect_err("no such profile");
        assert!(
            matches!(&error, Error::StorageIo { path, .. } if path.file_name() == Some("chicony-ir.json")),
            "{error}"
        );
        // And the same over the real corpus directory, which at P0 holds nothing.
        assert!(load("nothing-has-this-name").is_err());
    }

    #[test]
    fn a_non_json_file_in_the_corpus_is_ignored_rather_than_parsed() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = Utf8PathBuf::from_path_buf(dir.path().to_owned()).expect("UTF-8 temp dir");
        std::fs::write(root.join("README.md"), b"why these profiles exist").expect("write");
        assert!(load_dir(&root).expect("no profiles").is_empty());
    }
}
