//! The session store: inspectable files, atomic writes, one lock (design D9).
//!
//! A calibration session is a directory, not a database row:
//!
//! ```text
//! <state dir>/lock                                        the one advisory fd-lock
//! <state dir>/sessions/<fingerprint-slug>/<task-slug>/<uuidv7>/
//!     session.json      pretty-printed, diffable, jq-able
//!     log.ndjson        append-only; one event per line
//!     photos/<control-slug>/…
//! ```
//!
//! Everything below serves three properties D9 asks for, and each one is a property a
//! test can fail:
//!
//! - **A reader never sees a half-written document.** [`write_json_atomic`] is the one
//!   home for state writes (design §2.10): a temp file *in the destination directory*,
//!   written, `sync_all`ed, renamed over the destination, and the parent directory
//!   fsynced so the rename itself survives a power cut. Everything in between can fail;
//!   the destination file is either the old one or the new one.
//! - **Two processes never interleave.** One advisory `fd-lock` at the state directory's root,
//!   with D9's two protocols spelled as [`LockProtocol`]: the daemon takes it once and holds
//!   it for its lifetime, a daemonless `webcam-handler-cli` takes it for one mutating
//!   operation. Every mutating method here takes a `&`[`StoreLock`], so "under the lock" is a
//!   signature rather than a comment.
//! - **A crash is survivable, and corruption is not silently repaired.** A torn *last*
//!   line of `log.ndjson` is a process that died mid-append; it is dropped. A torn line
//!   anywhere else is corruption, and corruption is a typed refusal.
//!
//! ## Why the layout is derived, never taken verbatim
//!
//! Every path component is run through the one slug transform
//! ([`schema::slug`], via [`CameraFingerprint::slug`] and [`schema::session::task_slug`])
//! even when the value already looks like a slug. That is not belt-and-braces: a session
//! document is a *file the operator can edit*, `ControlSlug::parse` accepts any non-empty
//! string by design, and a `task_slug` of `../../..` would otherwise name a directory
//! outside the session tree. The transform is idempotent on well-formed slugs, so the
//! cost is nothing and the traversal is arithmetically impossible. This is the E4 lesson
//! in a filesystem costume: the hand-edited document is the input to design against.
//!
//! ## What is public, and why
//!
//! [`TempStore`] — the temp-dir double design §2.9 names — is public and gated behind
//! nothing, for the reason [`schema::paths::MapEnv`] is: the daemon's and the CLI's tests
//! need the same fixture as the engine's, and P3b/P3d consume it from other crates. A
//! `#[cfg(test)]` double is a double only this crate can use, and the alternative is
//! three copies of a fixture, which is three copies of a law.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::os::unix::fs::{
    DirBuilderExt as _, MetadataExt as _, OpenOptionsExt as _, PermissionsExt as _,
};

use camino::{Utf8Path, Utf8PathBuf};
use schema::camera::CameraFingerprint;
use schema::control::ControlSlug;
use schema::error::Holder;
use schema::session::{LogEntry, Session};
use schema::slug::{Separator, slugify};
use schema::vocabulary::closed_vocabulary;
use schema::{Error, Result, limits};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// --------------------------------------------------------------------------- the posture

/// The mode every directory in D9's state tree is created with, and the one the tree's root
/// is checked against (note **N142**).
///
/// **A frame may contain a person**, and a calibration session is the one place in this tool
/// where frames come to rest: `photos/<control>/<pass>/<value>.jpg` is a photograph of
/// whatever the camera was pointed at, kept for as long as the operator keeps the session.
/// The session document beside it names the camera, the task and the goal in the operator's
/// own words. None of that is anybody else's on this machine, and until this constant existed
/// the tree was created at the ambient umask — measured 0775 on directories and 0664 on
/// `log.ndjson` and on every sample photo, with `session.json` private only by accident of
/// `tempfile::Builder`'s default.
///
/// D11 has had this for its runtime directory since P4b and D9 did not, which is the whole
/// finding: one home for a number, created with it and asserted against it, so "0700" is not
/// written five times and cannot come to mean five things. `SOCKET_DIR_MODE` in
/// `webcam-handler-daemon` is its sibling and note N39 is the argument both rest on.
pub const STATE_DIR_MODE: u32 = 0o700;

/// The mode every file this store creates is created with.
///
/// [`STATE_DIR_MODE`]'s reason, one level in. It matters even under a 0700 directory,
/// because a mode is what survives the tree being copied, archived or moved somewhere less
/// private — and because a file created 0664 is a decision nobody made.
pub const STATE_FILE_MODE: u32 = 0o600;

/// `create_dir_all` at [`STATE_DIR_MODE`].
///
/// The mode is applied to every component this call creates. It is masked by the process
/// umask, which can only *clear* bits, so a directory this call creates is at most 0700 —
/// N39 makes the same observation about `mkdir` for the same reason, and it is why the check
/// below is about directories this call did **not** create.
///
/// # Errors
///
/// [`Error::StorageIo`] naming the directory.
fn create_private_dir(dir: &Utf8Path) -> Result<()> {
    fs::DirBuilder::new()
        .recursive(true)
        .mode(STATE_DIR_MODE)
        .create(dir.as_std_path())
        .map_err(|err| storage_io(dir, &err))
}

/// Refuse a state directory anybody but its owner can reach, or that its owner is not us
/// (note **N142**).
///
/// **Refused, not repaired**, which is N39's ruling applied to D9's tree: a `chmod` here
/// would hide the fact that the directory was reachable, and for however long it was
/// reachable it may already have been reached. An operator who is not told cannot act, and
/// what they have to act on is a directory of photographs.
///
/// The check is on the *root* rather than on every directory beneath it, because the root is
/// what a traversal has to get through: a 0700 directory is one another account cannot
/// search, so the modes inside it are defence in depth rather than the boundary. That is the
/// same reason N39 gives for asserting the socket **directory** and never the socket file.
///
/// **Both of N39's halves, not one** (note **N150**). The mode says who *may* walk in; the
/// owner says who may change that answer, and a mode-only check misses the case N39's own
/// argument names — "a root daemon traverses a 0700 directory belonging to a user without
/// noticing". A `webcam-handler-daemon` run as root against an `XDG_STATE_HOME` under
/// somebody else's tree would write calibration photographs into a directory that user owns
/// and can read, and a mode-only check would say yes.
///
/// **What "too wide" means here, and why it is not `!= STATE_DIR_MODE`.** The question this
/// asks is who other than the owner can reach the frames, so the mask is
/// `paths::GROUP_AND_OTHER_BITS` and nothing else. Two consequences are deliberate. A
/// *narrower* mode (0500, 0000) passes: it is not an exposure, it is the operator's business,
/// and the first write into it fails with the kernel's own `EACCES` rather than with an
/// invented refusal. And the set-user, set-group and sticky bits are ignored, because Linux
/// **inherits** `S_ISGID` on `mkdir` under a setgid parent — a mask of `!STATE_DIR_MODE`
/// makes this tool create a directory at 2700 and refuse it on the same call, permanently, on
/// a first run, on hosts whose group-managed home carries the bit. It also made the printed
/// remedy false: `chmod -R go-rwx` cannot clear a set-id bit, and the one thing a refusal owes
/// an unattended reader is a way out that works.
///
/// `daemon::uds::check_mode_and_owner` is the sibling and asks for *exact* equality instead,
/// which is not a divergence but a different question: D11 makes the socket directory's mode
/// the whole authentication model, so anything but 0700 there is wrong. Here the mode is
/// privacy, and only the bits that grant it are the subject. That function's doc states the
/// same split from its own side and prices what exactness costs it, so neither check is the
/// only place the reasoning appears.
///
/// `ours` is the effective uid to compare against, **taken as a value** rather than read from
/// the process: a check whose second half can only be exercised by owning a second account is
/// a check with one half nobody drives, and `scripts/gates/uds-permissions.sh` already carries
/// the cost of the real-account arrangement for D11's directory (note N44). The one caller
/// passes `geteuid()`.
///
/// # Errors
///
/// [`Error::StorageIo`] naming the directory, the modes and the one command that fixes it;
/// or naming both uids when the directory belongs to somebody else.
fn check_state_dir(root: &Utf8Path, ours: u32) -> Result<()> {
    let found = fs::metadata(root.as_std_path()).map_err(|err| storage_io(root, &err))?;
    let mode = found.permissions().mode() & schema::paths::MODE_BITS;
    let reachable = mode & schema::paths::GROUP_AND_OTHER_BITS;
    if reachable != 0 {
        return Err(Error::StorageIo {
            path: root.to_owned(),
            errno: None,
            message: format!(
                "is mode {mode:04o}, and this tool's state directory must grant nothing to \
                 group or other ({STATE_DIR_MODE:04o}) — it holds calibration photographs of \
                 whatever the cameras were pointed at, and a frame may contain a person. \
                 Refused rather than quietly tightened, because a directory that was \
                 reachable may already have been read: run `chmod -R go-rwx {root}` when you \
                 have decided that is acceptable"
            ),
        });
    }

    let owner = found.uid();
    if owner != ours {
        return Err(Error::StorageIo {
            path: root.to_owned(),
            errno: None,
            message: format!(
                "is owned by uid {owner} and this process runs as uid {ours} — the mode above \
                 makes it private to its owner, and its owner is not us, so every session \
                 document and every calibration photograph written here would be readable by \
                 somebody else. A process running as root traverses such a directory without \
                 noticing, which is exactly the case this refuses; point $XDG_STATE_HOME at a \
                 directory this user owns"
            ),
        });
    }
    Ok(())
}

// --------------------------------------------------------------------------- the store

/// A session store rooted at one state directory.
///
/// Cheap to build and cheap to clone-by-rebuilding: it owns a path, not a handle. The
/// handles ([`StoreLock`]) are taken per protocol and dropped explicitly.
#[derive(Debug)]
pub struct SessionStore {
    root: Utf8PathBuf,
    /// The scripted fault, when a test has arranged one (design §2.9). `None` in every
    /// shipped code path; see [`SessionStore::arrange`] for why the seam is here rather
    /// than in a `#[cfg(test)]` module.
    scripted: Option<StoreFault>,
}

impl SessionStore {
    /// A store rooted at `root`, which is a state directory that need not exist yet.
    ///
    /// Nothing is created here. Resolving a path and making a directory are different
    /// operations with different failure modes, and `paths::state_dir` deliberately does
    /// only the first.
    #[must_use]
    pub fn new(root: impl Into<Utf8PathBuf>) -> SessionStore {
        SessionStore {
            root: root.into(),
            scripted: None,
        }
    }

    /// The store under this environment's XDG state directory (note N2).
    ///
    /// # Errors
    ///
    /// Whatever [`crate::paths::state_dir`] refuses with — [`Error::StorageIo`] when
    /// neither `$XDG_STATE_HOME` nor `$HOME` gives a usable directory.
    pub fn from_env(env: &dyn schema::paths::Env) -> Result<SessionStore> {
        Ok(SessionStore::new(crate::paths::state_dir(env)?))
    }

    /// The state directory this store is rooted at.
    #[must_use]
    pub fn root(&self) -> &Utf8Path {
        &self.root
    }

    /// `<state dir>/sessions`.
    #[must_use]
    pub fn sessions_dir(&self) -> Utf8PathBuf {
        self.root.join(limits::SESSIONS_DIR)
    }

    /// `<state dir>/lock` — the one advisory lock's file.
    #[must_use]
    pub fn lock_path(&self) -> Utf8PathBuf {
        self.root.join(limits::STORE_LOCK_FILE)
    }

    /// The directory `session` lives in.
    #[must_use]
    pub fn session_dir(&self, session: &Session) -> Utf8PathBuf {
        self.session_dir_for(&session.fingerprint, &session.task_slug, session.id)
    }

    /// The directory a (camera, task, id) triple lives in: D9's layout, component by
    /// component.
    #[must_use]
    pub fn session_dir_for(
        &self,
        fingerprint: &CameraFingerprint,
        task_slug: &str,
        id: Uuid,
    ) -> Utf8PathBuf {
        self.sessions_dir()
            .join(fingerprint.slug())
            .join(schema::session::task_slug(task_slug))
            .join(id.to_string())
    }

    /// Where one **pass** of one control's sample photos go, relative to the session
    /// directory.
    ///
    /// Relative because D9 says a session directory relocates as a unit, and a
    /// [`schema::session::Sample`] records this path verbatim.
    ///
    /// `from` is the number of samples the control already carried when this sweep began —
    /// the index its first sample will take in the control's history. D9's layout writes
    /// `photos/<control-slug>/<value>`, and that name is unique only *within* one sweep:
    /// D8's `precision` exists so a coarse pass can be followed by a fine one, `begin_sweep`
    /// is legal from `Calibrated` for exactly that reason, and the two passes overlap on
    /// the values the second one refines around. A second pass writing `128.jpg` over the
    /// first's leaves the first pass's [`schema::session::Sample`] naming a file that no
    /// longer holds the frame its metrics describe — and on real optics the refinement pass
    /// is run *because* the scene changed, so the two pictures are genuinely different.
    /// The pass component is what keeps "the frame that is scored is the frame that is
    /// stored" true across a control's whole history (note N22).
    ///
    /// A pass that records nothing reuses its predecessor's directory, which is correct:
    /// there is nothing on the record naming what is in it.
    ///
    /// # Errors
    ///
    /// [`Error::ControlUnknown`] when the slug has no usable directory name — a slug of
    /// punctuation is not a control this tool ever enumerated
    /// (`ControlSlug::from_name` refuses those), so it reached the session file by hand.
    pub fn photo_dir_rel(control: &ControlSlug, from: usize) -> Result<Utf8PathBuf> {
        let component = slugify(control.as_str(), Separator::Underscore);
        if component.is_empty() {
            return Err(Error::ControlUnknown {
                requested: control.as_str().to_owned(),
                did_you_mean: Vec::new(),
            });
        }
        Ok(Utf8PathBuf::from(limits::SESSION_PHOTOS_DIR)
            .join(component)
            .join(from.to_string()))
    }

    /// Where one pass of one control's sample photos go on disk.
    ///
    /// # Errors
    ///
    /// As [`SessionStore::photo_dir_rel`].
    pub fn photo_dir(
        session_dir: &Utf8Path,
        control: &ControlSlug,
        from: usize,
    ) -> Result<Utf8PathBuf> {
        Ok(session_dir.join(SessionStore::photo_dir_rel(control, from)?))
    }

    // ------------------------------------------------------------------- mutations

    /// Write `session.json`, atomically, and return the directory it landed in.
    ///
    /// The `_lock` argument is the point: D9 says state writes happen under the one
    /// advisory lock, and a law expressed as a parameter cannot be forgotten the way a
    /// law expressed as a doc comment can.
    ///
    /// The document's `schema_version` is checked and never corrected. The version field's
    /// whole job is to be believed, so a document carrying a foreign one is **refused**
    /// rather than silently rewritten to this build's — but it is also refused rather than
    /// written, because a store that persisted it would produce a file the tool cannot
    /// read back, and "everything this tool writes, it can load" is the property D9's
    /// versioned load is for (note N13). The [`StoreFault::ForeignSchemaVersion`] fixture
    /// still writes a real foreign document; it just no longer does it by handing one to
    /// this method.
    ///
    /// # Errors
    ///
    /// [`Error::SchemaVersionForeign`] when the document is not at this build's version.
    /// [`Error::StorageIo`] for any filesystem failure, carrying the real `errno` where
    /// the kernel supplied one. A refusal that reached the parent directory's `fsync` has
    /// **published** the document — see `SessionStore::publish_session`, which is the same
    /// write with that distinction kept.
    pub fn save_session(&self, lock: &StoreLock, session: &Session) -> Result<Utf8PathBuf> {
        self.publish_session(lock, session)
            .map_err(|(error, _)| error)
    }

    /// [`SessionStore::save_session`], with which side of the rename it failed on.
    ///
    /// The same write and the same law — `save_session` is this function with the
    /// distinction thrown away, not a second copy of it — and it exists because exactly one
    /// caller has to act on the difference. `lifecycle::persist` replaces the caller's
    /// in-memory [`Session`] with the draft it just wrote; if the document landed and the
    /// directory's `fsync` did not, refusing *and* leaving the caller a state behind is how
    /// the next write rolls the document back (note **N141**).
    ///
    /// `pub(crate)` because that reader is in this crate and a wider surface would be an
    /// invitation to make the decision a second time somewhere else.
    ///
    /// # Errors
    ///
    /// As [`SessionStore::save_session`], paired with the [`Reach`] the failure reached.
    pub(crate) fn publish_session(
        &self,
        _lock: &StoreLock,
        session: &Session,
    ) -> std::result::Result<Utf8PathBuf, (Error, Reach)> {
        if session.schema_version != limits::SESSION_SCHEMA_VERSION {
            return Err((
                Error::SchemaVersionForeign {
                    found: session.schema_version,
                    supported: limits::SESSION_SCHEMA_VERSION,
                },
                Reach::Untouched,
            ));
        }
        let dir = self.session_dir(session);
        let path = dir.join(limits::SESSION_FILE);
        match self.scripted {
            // A future build wrote this file. Arranged by writing real bytes, so the
            // *reader* under test is never handed a fiction (note N10's lesson).
            Some(StoreFault::ForeignSchemaVersion) => {
                let mut ahead = session.clone();
                ahead.schema_version = limits::SESSION_SCHEMA_VERSION.saturating_add(1);
                write_json_atomic_scripted(&path, &ahead, None)?;
            }
            Some(StoreFault::DiskFull) => {
                write_json_atomic_scripted(&path, session, Some(StoreFault::DiskFull))?;
            }
            Some(StoreFault::LockHeld | StoreFault::TornLogLine) | None => {
                write_json_atomic_scripted(&path, session, None)?;
            }
        }
        Ok(dir)
    }

    /// Append one event to `log.ndjson`, healing an unterminated tail first.
    ///
    /// One entry per line, compact — `session.json` is the document a human reads, and a
    /// log is a thing `grep` and `jq -c` read. `O_APPEND` plus an `fsync` of the data
    /// means a concurrent writer cannot interleave *within* a line and a crash can only
    /// lose the tail, which is the case [`SessionStore::load_log`] is built to survive.
    ///
    /// **The heal is what keeps that survivable case survivable** (note **N140**).
    /// [`load_log`](SessionStore::load_log) drops a torn *last* line and refuses a torn
    /// *interior* one, and that pair is deliberate and argued: a crash mid-append cannot
    /// write a terminator after the damage, so damage with a terminator behind it is
    /// corruption and guessing at its contents would invent a session history (note N12
    /// pins it against a seeded mutant). But an append that wrote at whatever byte the last
    /// writer stopped at *turned the first into the second* — one crash plus one later
    /// append produced a file `calibrate status` refused for ever, with no verb that
    /// repaired it. So the tail is inspected under the lock this call already holds, before
    /// anything is written: a tail that parses is given the terminator its writer never
    /// managed, and one that does not is cut back to the last newline, which makes durable
    /// the drop the loader was already going to perform. Nothing is invented either way,
    /// and the interior refusal now means what it says — somebody other than this tool
    /// damaged the file.
    ///
    /// # Errors
    ///
    /// [`Error::StorageIo`] for any filesystem failure, the heal's included: a log this
    /// call could not make appendable is not one it may append to.
    pub fn append_log(&self, _lock: &StoreLock, dir: &Utf8Path, entry: &LogEntry) -> Result<()> {
        let path = dir.join(limits::SESSION_LOG_FILE);
        create_private_dir(dir)?;

        let mut line = serde_json::to_vec(entry).map_err(|err| {
            corrupt(
                &path,
                format!("could not be serialized as a log entry: {err}"),
            )
        })?;
        line.push(b'\n');

        match self.scripted {
            // The process dies between the write and the newline: a real torn write,
            // producing real bytes for the real loader to meet. Half a line of JSON is
            // not JSON, which is the property the fixture needs.
            Some(StoreFault::TornLogLine) => line.truncate(line.len() / 2),
            Some(StoreFault::DiskFull) => return Err(disk_full(&path)),
            Some(StoreFault::LockHeld | StoreFault::ForeignSchemaVersion) | None => {}
        }

        heal_log_tail(&path)?;

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .mode(STATE_FILE_MODE)
            .open(path.as_std_path())
            .map_err(|err| storage_io(&path, &err))?;
        file.write_all(&line)
            .map_err(|err| storage_io(&path, &err))?;
        file.sync_data().map_err(|err| storage_io(&path, &err))
    }

    /// Create one control's photo directory, and return its path on disk.
    ///
    /// # Errors
    ///
    /// [`Error::ControlUnknown`] for a slug with no usable directory name;
    /// [`Error::StorageIo`] when the directory cannot be made.
    pub fn create_photo_dir(
        &self,
        _lock: &StoreLock,
        session_dir: &Utf8Path,
        control: &ControlSlug,
        from: usize,
    ) -> Result<Utf8PathBuf> {
        let dir = SessionStore::photo_dir(session_dir, control, from)?;
        create_private_dir(&dir)?;
        Ok(dir)
    }

    // ------------------------------------------------------------------- reads

    /// Read `session.json` from a session directory, refusing a foreign version.
    ///
    /// The version is read from a probe that deserializes **only** `schema_version`, so a
    /// document this build cannot represent is refused before anything tries to
    /// represent it. A partial parse of a foreign document is the outcome D9 forbids: it
    /// looks like a session and is missing whatever the newer version added.
    ///
    /// # Errors
    ///
    /// [`Error::SchemaVersionForeign`] for any version that is not
    /// [`limits::SESSION_SCHEMA_VERSION`] — older and newer alike, because this build can
    /// read exactly one. [`Error::StorageIo`] when the file is unreadable, is not JSON,
    /// carries no `schema_version` at all, or does not deserialize.
    pub fn load_session(&self, session_dir: &Utf8Path) -> Result<Session> {
        let path = session_dir.join(limits::SESSION_FILE);
        let bytes = fs::read(path.as_std_path()).map_err(|err| storage_io(&path, &err))?;
        parse_session(&path, &bytes)
    }

    /// Read `log.ndjson`, dropping a torn last line and refusing a torn middle one.
    ///
    /// A missing log is an empty log, not a failure: a session that has done nothing yet
    /// has nothing to say.
    ///
    /// # Errors
    ///
    /// [`Error::StorageIo`] naming the line number when a line other than an
    /// unterminated last one fails to parse.
    pub fn load_log(&self, session_dir: &Utf8Path) -> Result<Vec<LogEntry>> {
        let path = session_dir.join(limits::SESSION_LOG_FILE);
        let bytes = match fs::read(path.as_std_path()) {
            Ok(bytes) => bytes,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(err) => return Err(storage_io(&path, &err)),
        };
        parse_log(&path, &bytes)
    }

    /// Every session recorded against `fingerprint`, newest first.
    ///
    /// "Newest first" is a sort over the directory names and nothing else: D9 chose
    /// UUIDv7 precisely so the identifier orders chronologically, and reading timestamps
    /// out of the documents to sort them would make listing cost a parse per session and
    /// fail on the one session whose file is corrupt. Listing does not parse; parsing is
    /// [`SessionStore::load_session`]'s job, which is why a foreign-version session still
    /// appears in this list and still refuses to load.
    ///
    /// A directory whose name is not a UUID is not ours; it is skipped rather than
    /// refused, because an operator's `notes/` beside the sessions must not break
    /// listing.
    ///
    /// # Errors
    ///
    /// [`Error::StorageIo`] when a directory that exists cannot be read. A *missing*
    /// tree is not an error: no sessions yet is an answer, not a failure.
    pub fn sessions_for(&self, fingerprint: &CameraFingerprint) -> Result<Vec<SessionRef>> {
        let slug = fingerprint.slug();
        let mut found = self.under_camera(&self.sessions_dir().join(&slug), &slug)?;
        // Descending: `Uuid`'s ordering is over its bytes, and a v7's leading 48 bits are
        // a big-endian millisecond timestamp, so byte order *is* time order.
        found.sort_by_key(|session| std::cmp::Reverse(session.id));
        Ok(found)
    }

    /// Every session this store holds, for every camera, newest first.
    ///
    /// The same walk [`SessionStore::sessions_for`] does, one directory level higher, and
    /// it parses nothing for the same reason: a session written by a build this one cannot
    /// read still *lists*, and a corrupt document in one camera's tree does not make the
    /// other cameras' sessions invisible. What the caller gets is where the sessions are,
    /// which is what `calibrate list` shows and what a lookup by id searches.
    ///
    /// # Errors
    ///
    /// [`Error::StorageIo`] when a directory that exists cannot be read. A missing tree is
    /// no sessions, which is an answer rather than a failure.
    pub fn all_sessions(&self) -> Result<Vec<SessionRef>> {
        let mut found = Vec::new();
        for camera_dir in read_dir_or_empty(&self.sessions_dir())? {
            let Some(camera) = camera_dir.file_name().map(ToOwned::to_owned) else {
                continue;
            };
            found.extend(self.under_camera(&camera_dir, &camera)?);
        }
        found.sort_by_key(|session| std::cmp::Reverse(session.id));
        Ok(found)
    }

    /// The session directories under one camera's directory, unsorted.
    ///
    /// A directory whose name is not a UUID is not ours; it is skipped rather than
    /// refused, because an operator's `notes/` beside the sessions must not break listing.
    fn under_camera(&self, base: &Utf8Path, camera: &str) -> Result<Vec<SessionRef>> {
        let mut found = Vec::new();
        for task_dir in read_dir_or_empty(base)? {
            let Some(task_slug) = task_dir.file_name().map(ToOwned::to_owned) else {
                continue;
            };
            for session_dir in read_dir_or_empty(&task_dir)? {
                let Some(name) = session_dir.file_name() else {
                    continue;
                };
                let Ok(id) = Uuid::parse_str(name) else {
                    continue;
                };
                if !session_dir.join(limits::SESSION_FILE).is_file() {
                    continue;
                }
                found.push(SessionRef {
                    id,
                    camera: camera.to_owned(),
                    task_slug: task_slug.clone(),
                    dir: session_dir,
                });
            }
        }
        Ok(found)
    }

    // ------------------------------------------------------------------- the lock

    /// Take the one advisory lock at the state directory's root.
    ///
    /// Never blocks. D9 is explicit that a `webcam-handler-cli` meeting a held lock reports it
    /// rather than "corrupting or blocking" — a CLI that hangs waiting for a daemon that will
    /// hold the lock until it exits is a CLI that hangs forever.
    ///
    /// # Errors
    ///
    /// [`Error::StoreLocked`] when somebody else holds it, carrying the holder's identity
    /// when the lock file's record could be read and `None` when it could not — an
    /// unidentified holder is reported as unidentified, never as a plausible pid.
    /// [`Error::StorageIo`] when the lock file itself cannot be opened.
    pub fn lock(&self, protocol: LockProtocol) -> Result<StoreLock> {
        if let Some(fault) = self.scripted {
            match fault {
                StoreFault::LockHeld => {
                    // The arrangement holds a real lock, so the record it reads is a real
                    // holder's — the scripted fault decides *that* the refusal happens,
                    // never what it says.
                    return Err(store_locked(self.holder()));
                }
                StoreFault::DiskFull
                | StoreFault::TornLogLine
                | StoreFault::ForeignSchemaVersion => {}
            }
        }
        StoreLock::acquire(&self.root, protocol)
    }

    /// Run one mutating operation under the lock — D9's daemonless `webcam-handler-cli`
    /// protocol, as one call so a caller cannot take the lock and forget to release it.
    ///
    /// # Errors
    ///
    /// As [`SessionStore::lock`], plus whatever `op` refuses with.
    pub fn with_lock<T>(&self, op: impl FnOnce(&StoreLock) -> Result<T>) -> Result<T> {
        let lock = self.lock(LockProtocol::PerOperation)?;
        op(&lock)
    }

    /// Who holds the lock right now, if anybody does.
    ///
    /// Implemented by *asking the kernel*, not by reading the record: a shared lock is
    /// taken and immediately released, and only if that is refused is the record read.
    /// So a stale record left behind by a process that died — the lock releases on close,
    /// the file does not erase itself — can never be reported as a live holder. The
    /// record is advisory decoration on a fact the kernel owns.
    #[must_use]
    pub fn holder(&self) -> Option<LockRecord> {
        let path = self.lock_path();
        let file = OpenOptions::new()
            .read(true)
            .open(path.as_std_path())
            .ok()?;
        let lock = fd_lock::RwLock::new(file);
        match lock.try_read() {
            // Nobody holds it exclusively.
            Ok(_) => None,
            Err(_) => {
                // A holder mid-write leaves a torn record; an unparsable record is an
                // unidentified holder, which is the honest answer.
                let bytes = fs::read(path.as_std_path()).ok()?;
                serde_json::from_slice(&bytes).ok()
            }
        }
    }

    // ------------------------------------------------------------------- the fault seam

    /// Arrange a fault from the store seam's menu (design §2.9).
    ///
    /// Three of the four are arranged **for real**, and that is deliberate: a held lock is
    /// a real `flock` taken from an independent open file description, a torn line is a
    /// real truncated append, and a foreign `schema_version` is a real document written
    /// with a version one ahead of this build's. Only [`StoreFault::DiskFull`] is
    /// synthesized, because filling a filesystem needs privileges no test may assume —
    /// and its `errno` is cross-checked against a real kernel `ENOSPC` by
    /// `the_disk_full_errno_is_the_kernels_own`, so the number is the kernel's answer and
    /// not the author's belief (note N10).
    ///
    /// The seam lives on the shipped type rather than in a `#[cfg(test)]` module for the
    /// reason [`TempStore`] is public: other crates' tests need it, and they compile this
    /// one without `cfg(test)`. It defaults to `None`, it is only reachable through this
    /// method, and every honoring site matches exhaustively — so a fifth fault does not
    /// compile until every site has an answer for it.
    ///
    /// # Errors
    ///
    /// [`Error::StoreLocked`] if a [`StoreFault::LockHeld`] arrangement cannot take the
    /// lock because somebody else already has it; [`Error::StorageIo`] if the lock file
    /// cannot be opened.
    pub fn arrange(&mut self, fault: StoreFault) -> Result<FaultArrangement> {
        let held = match fault {
            StoreFault::LockHeld => {
                // A real second holder over a real second open file description: `flock`
                // treats two opens of one file as two contenders even inside one process.
                Some(StoreLock::acquire(
                    &self.root,
                    LockProtocol::HeldForLifetime,
                )?)
            }
            StoreFault::DiskFull | StoreFault::TornLogLine | StoreFault::ForeignSchemaVersion => {
                None
            }
        };
        self.scripted = Some(fault);
        Ok(FaultArrangement { _held: held })
    }

    /// Stop honoring a scripted fault — the healthy direction of every fault arm.
    pub fn clear_faults(&mut self) {
        self.scripted = None;
    }

    /// The fault currently scripted on this store, if any.
    #[must_use]
    pub fn scripted(&self) -> Option<StoreFault> {
        self.scripted
    }
}

/// One session, as found by a directory walk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRef {
    /// The session's UUIDv7 — its directory name, and its chronological order.
    pub id: Uuid,
    /// The camera directory it sits under: the fingerprint slug (D9's
    /// `<fingerprint-slug>`), taken from the directory name rather than from the
    /// document, because a listing parses nothing.
    pub camera: String,
    /// The task directory it sits under.
    pub task_slug: String,
    /// Where it lives.
    pub dir: Utf8PathBuf,
}

// ---------------------------------------------------------------------- atomic writes

/// How far a failed atomic write got — which is a different question from what went wrong.
///
/// D13's registry answers *what the filesystem refused*; this answers *what the destination
/// now holds*, and a caller needs both (note **N141**). The publication is the rename, so
/// every failure before it leaves the old document in place and every failure after it
/// leaves the new one — and until N141 the contract claimed the first for both, while
/// `lifecycle::persist` believed it and left the caller's in-memory session one state behind
/// a document that had already landed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reach {
    /// The rename never ran, so the destination is exactly as it was. Every failure up to
    /// and including the temporary file's `sync_all` is this one.
    Untouched,
    /// The rename ran and the parent directory's `fsync` refused. The new document is
    /// published and every reader sees it; what is in doubt is only whether the rename
    /// survives a power cut, which is the promise note N12 says those two `fsync`s buy and
    /// nothing else in this suite can observe.
    Published,
}

/// Write `value` to `path` as pretty-printed JSON, atomically.
///
/// **The one home for state writes** (design §2.10, rubric A5). The sequence, and what
/// each step is for:
///
/// 1. a temporary file **in the destination's own directory**, so the rename below is a
///    rename and not a cross-device copy — a copy is not atomic;
/// 2. the whole document written from bytes already in memory, so a serialization
///    failure cannot leave a half-document anywhere;
/// 3. `sync_all` on the temp file, so the rename cannot be reordered ahead of the
///    contents by the filesystem;
/// 4. `rename` over the destination — the one step the kernel promises is atomic;
/// 5. `fsync` of the parent directory, so the rename itself survives a power cut.
///
/// Pretty-printed, with a trailing newline, because D9 asks for diffable and jq-able
/// files and a diff of a one-line JSON document is not a diff.
///
/// # Errors
///
/// [`Error::StorageIo`] carrying the path that failed and the real `errno` where one
/// exists. **A failure at steps 1–4 leaves the destination exactly as it was; a failure at
/// step 5 leaves the new document published** — the rename is the publication, and no
/// answer this function can give un-renames it. Step 5's failure is still returned, because
/// a state store that silently dropped a durability promise would be hiding the one thing
/// D9 asked for. A caller that has to tell the two apart uses
/// `SessionStore::publish_session`, which is where the distinction has a reader.
pub fn write_json_atomic<T: Serialize + ?Sized>(path: &Utf8Path, value: &T) -> Result<()> {
    write_json_atomic_scripted(path, value, None).map_err(|(error, _)| error)
}

fn write_json_atomic_scripted<T: Serialize + ?Sized>(
    path: &Utf8Path,
    value: &T,
    scripted: Option<StoreFault>,
) -> std::result::Result<(), (Error, Reach)> {
    // Everything through the rename is `Reach::Untouched` by construction, which is why the
    // classification is a closure over one variant rather than a decision at each `?`.
    let untouched = |error: Error| (error, Reach::Untouched);

    let dir = path
        .parent()
        .ok_or_else(|| corrupt(path, "has no parent directory to write a temp file in"))
        .map_err(untouched)?;
    create_private_dir(dir).map_err(untouched)?;

    // Serialized first, in full. `to_writer_pretty` would stream into the temp file and
    // wrap any `errno` the write hit inside a `serde_json::Error`, which loses the number
    // D13's `StorageIo` exists to carry.
    let mut bytes = serde_json::to_vec_pretty(value)
        .map_err(|err| corrupt(path, format!("could not be serialized: {err}")))
        .map_err(untouched)?;
    bytes.push(b'\n');

    let mut temp = tempfile::Builder::new()
        .prefix(".wch-")
        .suffix(".tmp")
        // By decision rather than by `tempfile`'s default, which is the same 0600 and is
        // not this project's to promise (note **N142**).
        .permissions(fs::Permissions::from_mode(STATE_FILE_MODE))
        .tempfile_in(dir.as_std_path())
        .map_err(|err| untouched(storage_io(dir, &err)))?;
    temp.write_all(&bytes)
        .map_err(|err| untouched(storage_io(dir, &err)))?;
    temp.as_file()
        .sync_all()
        .map_err(|err| untouched(storage_io(dir, &err)))?;

    if let Some(fault) = scripted {
        match fault {
            // Fired *here*, in the one window atomicity is about: the bytes are written
            // and synced, and the destination has not been touched. The temp file is
            // unlinked as it drops, so the failure leaves the directory as it found it.
            StoreFault::DiskFull => return Err(untouched(disk_full(path))),
            StoreFault::LockHeld | StoreFault::TornLogLine | StoreFault::ForeignSchemaVersion => {}
        }
    }

    temp.persist(path.as_std_path())
        .map_err(|err| untouched(storage_io(path, &err.error)))?;
    // Past this line the document is published, and every reader — including the next
    // process — sees the new one.
    fsync_dir(dir).map_err(|error| (error, Reach::Published))
}

/// How far back [`last_terminator`] reads at a time while hunting the last `\n`.
///
/// **A buffer size and nothing else** (note **N140**, corrected from the window the heal
/// first landed with). It keeps the common case cheap: a log grows, an append happens per sample, and
/// reading the file whole on each one would make appending quadratic in the history an
/// append-only file exists to avoid. Eight kibibytes finds the terminator in one read for
/// every entry this build writes.
///
/// It is deliberately **not** a bound on what the heal will parse. It was one, under the
/// premise that "a tail longer than this cannot be a log line this build wrote" — and that
/// premise is false: [`schema::session::SessionEvent::Started`] carries the operator's or
/// the agent's own `goal` text, which nothing in `limits` bounds, and a harness writing a
/// task description into it can exceed any figure chosen here. The consequence was worse
/// than the premise: past the window the tail was dropped **without being parsed**, while
/// `parse_log` parses an unterminated last segment and keeps it when it parses — so the two
/// halves of D9's one rule disagreed for exactly the files nobody would think to check, and
/// when the over-long line was the only one in the file the heal truncated it to nothing.
///
/// The bound the heal actually has is the one [`SessionStore::load_log`] already has: the
/// file. That call `read`s the whole thing on every `calibrate status`, so a heal that reads
/// one unterminated tail after a crash costs strictly less than the read that follows it,
/// and inventing a tighter bound here is what made the two disagree.
const LOG_SCAN_CHUNK: u64 = 8 * 1024;

/// Make `log.ndjson` appendable again after a crash left it mid-line (note **N140**).
///
/// Reads one byte in the ordinary case — the last one — because a file that ends in a
/// terminator has nothing to heal, and that is every append but the first one after a
/// crash. Only an unterminated tail costs more.
///
/// The two answers, and neither invents anything:
///
/// - **A tail that parses** is a whole entry whose terminator never reached the platter.
///   Its newline is written, and the event is kept: the writer had already serialized the
///   whole thing, so dropping it would throw away a record that is entirely present.
/// - **A tail that does not parse** is the torn write
///   [`SessionStore::load_log`] already drops. `set_len` back to the last newline makes that
///   drop durable, which is the whole repair: without it the next append puts a terminator
///   behind the damage and the loader's *interior* refusal — argued, and correct — starts
///   firing on a file only this tool ever wrote.
///
/// **The two answers are the loader's own, for a tail of any length.** Nothing is dropped
/// without being parsed first, which is what keeps this function and `parse_log` one rule
/// rather than two that agree on the cases somebody happened to write a fixture for
/// ([`LOG_SCAN_CHUNK`] carries what that cost and why the earlier bound was wrong).
///
/// A file that is missing, empty, or already terminated is left exactly as it is.
///
/// # Errors
///
/// [`Error::StorageIo`] for any filesystem failure.
fn heal_log_tail(path: &Utf8Path) -> Result<()> {
    let mut file = match OpenOptions::new()
        .read(true)
        .write(true)
        .open(path.as_std_path())
    {
        Ok(file) => file,
        // The first append creates the file; there is no tail yet.
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(storage_io(path, &err)),
    };
    let len = file.metadata().map_err(|err| storage_io(path, &err))?.len();
    if len == 0 {
        return Ok(());
    }

    let mut last = [0_u8; 1];
    file.seek(SeekFrom::Start(len.saturating_sub(1)))
        .map_err(|err| storage_io(path, &err))?;
    file.read_exact(&mut last)
        .map_err(|err| storage_io(path, &err))?;
    if last[0] == b'\n' {
        return Ok(());
    }

    let cut = last_terminator(&mut file, path, len)?;
    // The tail in full, however long it is — because the question below is exactly the
    // question `parse_log` asks of the same bytes, and asking it of a prefix would give a
    // different answer for the same file (note **N140**). A `goal` an agent wrote can be any
    // length, so "too long to be one of ours" is not a fact this code can establish; and
    // dropping bytes *without parsing them* is the one thing the loader never does.
    let mut tail = vec![0_u8; usize::try_from(len.saturating_sub(cut)).unwrap_or(0)];
    file.seek(SeekFrom::Start(cut))
        .map_err(|err| storage_io(path, &err))?;
    file.read_exact(&mut tail)
        .map_err(|err| storage_io(path, &err))?;

    let whole = std::str::from_utf8(&tail)
        .ok()
        .is_some_and(|line| serde_json::from_str::<LogEntry>(line).is_ok());
    if whole {
        file.seek(SeekFrom::Start(len))
            .map_err(|err| storage_io(path, &err))?;
        file.write_all(b"\n")
            .map_err(|err| storage_io(path, &err))?;
    } else {
        file.set_len(cut).map_err(|err| storage_io(path, &err))?;
    }
    // `sync_all` rather than `sync_data`: the truncating arm's whole effect is the file's
    // length, and the repair has to be on the platter before the append that follows it —
    // otherwise a second crash leaves the damage the first one caused *and* a terminator
    // behind it, which is the state this function exists to make unreachable.
    file.sync_all().map_err(|err| storage_io(path, &err))
}

/// One past the last `\n` in `file`, or `0` when it holds none.
///
/// Walks backwards a [`LOG_SCAN_CHUNK`] at a time rather than reading the file whole, for
/// that constant's reason.
fn last_terminator(file: &mut File, path: &Utf8Path, len: u64) -> Result<u64> {
    let mut probe = len;
    loop {
        let from = probe.saturating_sub(LOG_SCAN_CHUNK);
        let mut chunk = vec![0_u8; usize::try_from(probe.saturating_sub(from)).unwrap_or(0)];
        file.seek(SeekFrom::Start(from))
            .map_err(|err| storage_io(path, &err))?;
        file.read_exact(&mut chunk)
            .map_err(|err| storage_io(path, &err))?;
        if let Some(index) = chunk.iter().rposition(|byte| *byte == b'\n') {
            return Ok(from
                .saturating_add(u64::try_from(index).unwrap_or(0))
                .saturating_add(1));
        }
        if from == 0 {
            return Ok(0);
        }
        probe = from;
    }
}

/// `fsync` a directory, so a rename into it survives a power cut.
///
/// A rename is atomic with respect to *readers*; making it durable with respect to a
/// crash is a separate promise, and it is the parent directory's fsync that gives it.
fn fsync_dir(dir: &Utf8Path) -> Result<()> {
    let handle = File::open(dir.as_std_path()).map_err(|err| storage_io(dir, &err))?;
    handle.sync_all().map_err(|err| storage_io(dir, &err))
}

// ---------------------------------------------------------------------- parsing

/// Only the field that decides whether the rest may be read.
#[derive(Debug, Deserialize)]
struct VersionProbe {
    schema_version: Option<u32>,
}

fn parse_session(path: &Utf8Path, bytes: &[u8]) -> Result<Session> {
    let probe: VersionProbe = serde_json::from_slice(bytes)
        .map_err(|err| corrupt(path, format!("is not a JSON document: {err}")))?;
    match probe.schema_version {
        None => Err(corrupt(
            path,
            "carries no schema_version; every document D9 writes has one from day one, \
             so this file was not written by this tool",
        )),
        Some(found) if found != limits::SESSION_SCHEMA_VERSION => {
            Err(Error::SchemaVersionForeign {
                found,
                supported: limits::SESSION_SCHEMA_VERSION,
            })
        }
        Some(_) => serde_json::from_slice(bytes)
            .map_err(|err| corrupt(path, format!("is not a session document: {err}"))),
    }
}

fn parse_log(path: &Utf8Path, bytes: &[u8]) -> Result<Vec<LogEntry>> {
    let segments: Vec<&[u8]> = bytes.split(|byte| *byte == b'\n').collect();
    let last = segments.len().saturating_sub(1);

    let mut entries = Vec::new();
    for (index, segment) in segments.iter().enumerate() {
        // A blank line carries no event. Skipped rather than refused: an editor or a
        // stray `>>` that added a newline has damaged nothing, and reporting corruption
        // for it would teach an operator to distrust the report that matters.
        if segment.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        let parsed = std::str::from_utf8(segment)
            .ok()
            .and_then(|line| serde_json::from_str::<LogEntry>(line).ok());
        match parsed {
            Some(entry) => entries.push(entry),
            None if index == last => {
                // The last segment, which — because `split` always yields an empty final
                // segment for a file that ends in a newline, and empty segments are
                // skipped above — can only be reached by a file that does *not* end in a
                // newline. So this arm is exactly "an unterminated last line", with no
                // separate flag to keep in step; a torn line in a file that is properly
                // terminated falls to the refusing arm below, which is right, because a
                // crash mid-append cannot write a terminator after the damage. (An
                // earlier version carried that flag as well. It could not change any
                // outcome, and a branch no fixture can flip is a branch nobody tests.)
                //
                // A process that died between the write and its terminator. D9 drops it,
                // silently, because a crash
                // mid-append is expected and benign — the event never completed, so no
                // information is lost by forgetting it.
                break;
            }
            None => {
                return Err(corrupt(
                    path,
                    format!(
                        "line {} is not a log entry, and it is not the unterminated last \
                         line a crash would leave — this file is corrupt, and guessing at \
                         its contents would invent a session history",
                        index.saturating_add(1)
                    ),
                ));
            }
        }
    }
    Ok(entries)
}

/// The entries of a directory as UTF-8 paths, or nothing when the directory is absent.
fn read_dir_or_empty(dir: &Utf8Path) -> Result<Vec<Utf8PathBuf>> {
    let listing = match fs::read_dir(dir.as_std_path()) {
        Ok(listing) => listing,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(storage_io(dir, &err)),
    };
    let mut out = Vec::new();
    for entry in listing {
        let entry = entry.map_err(|err| storage_io(dir, &err))?;
        // A non-UTF-8 name cannot be one of ours: every component this store writes comes
        // out of the slug transform, which emits ASCII.
        if let Ok(path) = Utf8PathBuf::from_path_buf(entry.path())
            && path.is_dir()
        {
            out.push(path);
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------- the lock

/// D9's two locking protocols, defined in the error registry that carries them.
///
/// The definition lives in `webcam-handler-schema` rather than here, where the locking is,
/// because [`Error::StoreLocked`] has to *say* which protocol holds the lock — that is the
/// fact D9's refusal turns on, it therefore crosses the wire, and `schema` cannot depend on
/// this crate (note N40). Re-exported so every `engine::store::LockProtocol` in the tree
/// still names one type: the vocabulary moved, the law did not split.
///
/// Recording which one is in force is why [`LockRecord`] carries it.
pub use schema::error::LockProtocol;

/// What the lock file says about whoever holds it.
///
/// Advisory decoration on a fact the kernel owns (see [`SessionStore::holder`]): the lock
/// is the `flock`, and this is how the holder introduces itself so a refusal can name
/// something more useful than "somebody".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LockRecord {
    /// The holding process.
    pub pid: i32,
    /// Its `comm`, when `/proc` let us read it — the same field, from the same place, as
    /// the `Busy` refusal's holder walk.
    ///
    /// It is fifteen characters at most, because that is what `comm` is (`TASK_COMM_LEN` is
    /// 16 including the NUL), and since note **N90** gave every binary here the
    /// `webcam-handler-` prefix that is exactly what all four of them report. So this field
    /// says "something of ours" and the `pid` beside it says *which* — which is the division
    /// of labour it always had, now with nothing left over.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comm: Option<String>,
    /// Which protocol it is following.
    pub protocol: LockProtocol,
}

impl LockRecord {
    /// This process, following `protocol`.
    #[must_use]
    pub fn for_this_process(protocol: LockProtocol) -> LockRecord {
        LockRecord {
            pid: LockRecord::pid_or_unknown(std::process::id()),
            comm: fs::read_to_string("/proc/self/comm")
                .ok()
                .map(|comm| comm.trim().to_owned())
                .filter(|comm| !comm.is_empty()),
            protocol,
        }
    }

    /// A raw pid as the record's `i32`, or `-1` when it does not fit.
    ///
    /// `std::process::id` is a `u32` and a pid is an `i32`; a pid that does not fit is not
    /// a pid, and reporting one nobody can look up is worse than reporting none. `-1`
    /// never names a process, where any positive fallback names one somebody would then go
    /// and look for.
    ///
    /// **A value, not a call**, so that branch is reachable from a test: a process cannot
    /// choose its own pid, and P3f's mutation run found `-1` could become `1` with the
    /// whole workspace green. The engine's own rule — pure cores take values — is what
    /// makes the difference between a line nothing can watch and a line one assertion can.
    fn pid_or_unknown(raw: u32) -> i32 {
        i32::try_from(raw).unwrap_or(-1)
    }

    /// This record as the D13 registry's holder type.
    #[must_use]
    pub fn holder(&self) -> Holder {
        Holder {
            pid: self.pid,
            comm: self.comm.clone(),
        }
    }
}

/// A held advisory lock over one state directory.
///
/// Released when dropped. The release is the *close*, not an explicit unlock, and that is
/// worth stating because the code below looks odd otherwise: `fd_lock`'s guard borrows
/// its `RwLock`, so a struct holding both is self-referential and cannot be written
/// without `unsafe`, which this crate forbids. The guard is therefore forgotten — which
/// suppresses nothing but its own `flock(LOCK_UN)` — and the `RwLock` (and with it the
/// open file) is owned here. Linux releases an `flock` when the last descriptor referring
/// to its open file description is closed, so dropping this value releases the lock, and
/// a process that dies releases it too. `a_dropped_lock_is_released` is the test that
/// keeps this paragraph honest.
#[derive(Debug)]
pub struct StoreLock {
    protocol: LockProtocol,
    path: Utf8PathBuf,
    /// Owned so that dropping this value closes the descriptor and releases the lock.
    _lock: fd_lock::RwLock<File>,
}

impl StoreLock {
    fn acquire(root: &Utf8Path, protocol: LockProtocol) -> Result<StoreLock> {
        create_private_dir(root)?;
        // Every mutating operation in D9 passes through here, which is why the tree's mode
        // is asserted here (note **N142**): it is the one funnel, and a check the reads do
        // not pay for is a check `calibrate status` still answers under.
        check_state_dir(root, rustix::process::geteuid().as_raw())?;
        let path = root.join(limits::STORE_LOCK_FILE);
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(STATE_FILE_MODE)
            .open(path.as_std_path())
            .map_err(|err| storage_io(&path, &err))?;

        let mut lock = fd_lock::RwLock::new(file);
        match lock.try_write() {
            Ok(guard) => std::mem::forget(guard),
            Err(err) if err.kind() == io::ErrorKind::WouldBlock => {
                return Err(store_locked(read_record(&path)));
            }
            Err(err) => return Err(storage_io(&path, &err)),
        }

        // Introduce ourselves. Written in place rather than through
        // `write_json_atomic`, and this is the one deliberate exception to that home: a
        // rename replaces the *inode*, and the lock is held on this inode's open file
        // description. Making the lock file atomic would make it stop being the lock.
        // The write happens while we hold the lock, so no other holder can be writing it
        // at the same time, and a reader that catches it torn reports an unidentified
        // holder rather than a wrong one.
        let record = LockRecord::for_this_process(protocol);
        write_record(&path, &record)?;

        Ok(StoreLock {
            protocol,
            path,
            _lock: lock,
        })
    }

    /// The protocol this lock was taken under.
    #[must_use]
    pub fn protocol(&self) -> LockProtocol {
        self.protocol
    }

    /// The lock file.
    #[must_use]
    pub fn path(&self) -> &Utf8Path {
        &self.path
    }
}

fn write_record(path: &Utf8Path, record: &LockRecord) -> Result<()> {
    let bytes = serde_json::to_vec(record)
        .map_err(|err| corrupt(path, format!("lock record could not be serialized: {err}")))?;
    let mut file = OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(path.as_std_path())
        .map_err(|err| storage_io(path, &err))?;
    file.write_all(&bytes)
        .map_err(|err| storage_io(path, &err))?;
    file.sync_data().map_err(|err| storage_io(path, &err))
}

fn read_record(path: &Utf8Path) -> Option<LockRecord> {
    let bytes = fs::read(path.as_std_path()).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// D9's refusal, built from whatever the holder's record said — which may be nothing.
///
/// One function because the two callers must not be able to disagree: `holder` and
/// `protocol` are two readings of *one* record, and a refusal that named a pid without the
/// protocol (or the reverse) would either withhold the advice D9 owes the caller or attach
/// it to a holder we could not identify. Both fields therefore fall together.
fn store_locked(record: Option<LockRecord>) -> Error {
    Error::StoreLocked {
        holder: record.as_ref().map(LockRecord::holder),
        protocol: record.map(|record| record.protocol),
    }
}

// ---------------------------------------------------------------------- the fault menu

closed_vocabulary! {
    /// A fault the session-store seam can be told to exhibit (design §2.9's store row,
    /// variant for variant).
    ///
    /// `ALL` is generated from this definition, so a fifth fault joins every walk over it
    /// in the same token — a fault the compiler cannot force somebody to answer is a
    /// fault nobody answers.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub enum StoreFault {
        /// The filesystem is out of space: every write refuses with
        /// [`Error::StorageIo`] carrying `ENOSPC`.
        DiskFull,
        /// Somebody else holds the state directory's lock:
        /// [`Error::StoreLocked`].
        LockHeld,
        /// A process dies mid-append, leaving `log.ndjson` ending in half a record.
        TornLogLine,
        /// A future build wrote `session.json`: loading it is
        /// [`Error::SchemaVersionForeign`].
        ForeignSchemaVersion,
    }
}

impl StoreFault {
    /// The fault's name, for reports and test failure messages.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            StoreFault::DiskFull => "disk_full",
            StoreFault::LockHeld => "lock_held",
            StoreFault::TornLogLine => "torn_log_line",
            StoreFault::ForeignSchemaVersion => "foreign_schema_version",
        }
    }
}

impl std::fmt::Display for StoreFault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What arranging a fault left behind.
///
/// Kept alive by the caller for as long as the fault should last: a
/// [`StoreFault::LockHeld`] arrangement *is* a held lock, and dropping this releases it.
#[derive(Debug)]
#[must_use = "dropping the arrangement releases whatever the fault was holding"]
pub struct FaultArrangement {
    _held: Option<StoreLock>,
}

// ---------------------------------------------------------------------- errors

/// `ENOSPC`, the number.
///
/// Named here rather than pulled from `libc` (which this crate does not depend on) and
/// *validated against the kernel* by `the_disk_full_errno_is_the_kernels_own`, which asks
/// `/dev/full` what a full filesystem answers. A constant the author believes is exactly
/// the shape note N10 was written about.
const ENOSPC: i32 = 28;

fn disk_full(path: &Utf8Path) -> Error {
    storage_io(path, &io::Error::from_raw_os_error(ENOSPC))
}

/// A filesystem failure, with the real `errno` where the kernel supplied one.
fn storage_io(path: &Utf8Path, err: &io::Error) -> Error {
    Error::StorageIo {
        path: path.to_owned(),
        errno: err.raw_os_error(),
        message: err.to_string(),
    }
}

/// A persisted document that is not what it claims to be.
///
/// [`Error::StorageIo`] with no `errno`, because the kernel did nothing wrong: the read
/// succeeded and the bytes are the problem. D13 is closed and has no "corrupt document"
/// variant; `StorageIo` is the one that carries a path and a message, and it is the
/// variant note N4 added for exactly the D9 fault menu's filesystem failures.
/// `SchemaVersionForeign` would be a lie (the version is not what is wrong) and
/// `IllegalTransition` names a state machine that is not involved.
fn corrupt(path: &Utf8Path, message: impl Into<String>) -> Error {
    Error::StorageIo {
        path: path.to_owned(),
        errno: None,
        message: message.into(),
    }
}

// ---------------------------------------------------------------------- the double

/// A store rooted in a temporary directory that disappears with it — design §2.9's
/// "temp-dir store".
///
/// Public and gated behind nothing, for [`schema::paths::MapEnv`]'s reason: the engine's
/// tests, the daemon's integration tests and the CLI's subprocess tests all need a state
/// directory that is real, empty, and gone afterwards, and three copies of that fixture
/// is three copies of a law (design §2.10). It also keeps the privacy rule mechanical —
/// a test store lives under [`crate::paths::scratch_dir`], which is inside `target/` and
/// therefore both gitignored and pruned by every gate that walks the tree, so nothing a
/// test writes can land in the repository or be mistaken for something that did
/// (rubric A12, note **N84**).
#[derive(Debug)]
pub struct TempStore {
    /// Dropped last of all, which removes the tree.
    dir: tempfile::TempDir,
    store: SessionStore,
}

impl TempStore {
    /// A new, empty store under [`crate::paths::scratch_dir`].
    ///
    /// # Errors
    ///
    /// [`Error::StorageIo`] when the temporary directory cannot be made, or when its
    /// path is not UTF-8 — every path in this tool is a [`camino`] path, and a scratch
    /// root we cannot represent is one we cannot use.
    pub fn new() -> Result<TempStore> {
        let dir = crate::paths::scratch_dir()?;
        let root = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).map_err(|path| {
            corrupt(
                "the scratch root".into(),
                format!("is not valid UTF-8: {}", path.display()),
            )
        })?;
        let store = SessionStore::new(root);
        Ok(TempStore { dir, store })
    }

    /// The store.
    #[must_use]
    pub fn store(&self) -> &SessionStore {
        &self.store
    }

    /// The store, for arranging faults on.
    #[must_use]
    pub fn store_mut(&mut self) -> &mut SessionStore {
        &mut self.store
    }

    /// The state directory's path.
    #[must_use]
    pub fn root(&self) -> &Utf8Path {
        self.store.root()
    }

    /// A second, independent store over the same directory — the "another process" half
    /// of every lock test.
    ///
    /// Independent where it matters: taking the lock through this store opens the lock
    /// file again, and `flock` treats two opens of one file as two contenders even inside
    /// one process. So the two-orderings test needs no second process, and therefore no
    /// sleep to synchronize with one.
    #[must_use]
    pub fn peer(&self) -> SessionStore {
        SessionStore::new(self.root().to_owned())
    }

    /// Lay `log.ndjson` down byte for byte, for the shapes a crash leaves behind.
    ///
    /// [`StoreFault::TornLogLine`]'s family, and it lives beside it for the same reason: the
    /// bytes a torn append leaves are a *store* fixture, and D9's two answers to them — drop
    /// an unterminated last line, refuse a torn interior one — are only assertable against
    /// real bytes (note **N140**). A suite that reached for `std::fs::write` to plant them
    /// would be a raw state-directory write, which `scripts/gates/atomic-write-home.sh`
    /// reserves to this file; a fixture is not a reason to weaken that rule, so the fixture
    /// comes here instead.
    ///
    /// Deliberately **not** atomic and deliberately not under the lock: the whole point is
    /// to produce a file the store's own protocol never would.
    ///
    /// # Errors
    ///
    /// [`Error::StorageIo`] naming the file.
    pub fn plant_log(&self, dir: &Utf8Path, bytes: &[u8]) -> Result<()> {
        create_private_dir(dir)?;
        let path = dir.join(limits::SESSION_LOG_FILE);
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(STATE_FILE_MODE)
            .open(path.as_std_path())
            .map_err(|err| storage_io(&path, &err))?;
        file.write_all(bytes).map_err(|err| storage_io(&path, &err))
    }

    /// Keep the directory instead of deleting it, and return its path.
    ///
    /// For the failure a temp-dir fixture makes hardest to debug: the tree is gone by the
    /// time you read the assertion.
    #[must_use]
    pub fn keep(self) -> Utf8PathBuf {
        let root = self.root().to_owned();
        let _ = self.dir.keep();
        root
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use schema::ErrorKind;
    use schema::camera::CameraFingerprint;
    use schema::session::{ControlSession, ControlStatus, SessionEvent, new_session};
    use schema::time::Stamp;

    use super::*;

    fn fingerprint(card: &str, bus: &str) -> CameraFingerprint {
        CameraFingerprint {
            bus_path: bus.to_owned(),
            usb_id: None,
            card: card.to_owned(),
            driver: "uvcvideo".to_owned(),
            serial: None,
        }
    }

    fn session(id: Uuid, task: &str) -> Session {
        new_session(
            id,
            fingerprint("OBSBOT Tiny 3", "3-1:1.0"),
            task,
            "legible text",
            "0.1.0",
            Stamp::epoch(),
        )
    }

    fn uuid_v7(millis: u64) -> Uuid {
        Uuid::new_v7(uuid::Timestamp::from_unix(
            uuid::NoContext,
            millis / 1_000,
            u32::try_from((millis % 1_000) * 1_000_000).expect("sub-second nanos fit"),
        ))
    }

    fn entry(control: &str, total: u32) -> LogEntry {
        LogEntry {
            at: Stamp::epoch(),
            event: SessionEvent::SweepStarted {
                control: ControlSlug::parse(control).expect("literal slug"),
                total,
                precision: 1,
                adjustments: Vec::new(),
            },
        }
    }

    // ------------------------------------------------------------------ layout

    #[test]
    fn the_layout_is_d9s_layout() {
        let store = SessionStore::new("/state/webcam-handler");
        let id = uuid_v7(1_000);
        let session = session(id, "Read text from the DUT display");
        assert_eq!(
            store.session_dir(&session),
            Utf8PathBuf::from(format!(
                "/state/webcam-handler/sessions/obsbot-tiny-3-3-1-1-0/\
                 read-text-from-the-dut-display/{id}"
            ))
        );
        assert_eq!(
            store.lock_path(),
            Utf8PathBuf::from("/state/webcam-handler/lock")
        );
    }

    #[test]
    fn the_fingerprint_component_comes_from_the_one_slug_transform() {
        // Not a second slugifier: the directory name is whatever
        // `CameraFingerprint::slug` says, so a change there moves the directories and
        // this test is what notices.
        let store = SessionStore::new("/state");
        let fp = fingerprint("Integrated Camera: Integrated C", "1-6:1.0");
        let dir = store.session_dir_for(&fp, "focus", uuid_v7(1));
        assert!(
            dir.as_str().contains(&fp.slug()),
            "{dir} does not carry {}",
            fp.slug()
        );
    }

    #[test]
    fn a_hand_edited_task_slug_cannot_escape_the_session_tree() {
        // The E4 lesson in a filesystem costume: a session document is a file an operator
        // can edit, so the path components are *derived* rather than believed.
        let store = SessionStore::new("/state");
        let dir = store.session_dir_for(&fingerprint("cam", "1-1"), "../../etc", uuid_v7(1));
        assert!(
            dir.starts_with("/state/sessions"),
            "{dir} left the session tree"
        );
        assert!(!dir.as_str().contains(".."), "{dir} carries a traversal");
    }

    #[test]
    fn photo_paths_are_relative_so_a_session_directory_relocates_as_a_unit() {
        let control = ControlSlug::parse("focus_absolute").expect("literal slug");
        let rel = SessionStore::photo_dir_rel(&control, 0).expect("a usable slug");
        assert_eq!(rel, Utf8PathBuf::from("photos/focus_absolute/0"));
        assert!(rel.is_relative(), "{rel} is absolute");

        // A second pass over the same control writes somewhere else, so its photos cannot
        // land on the first pass's — the property note N22 exists for.
        let refined = SessionStore::photo_dir_rel(&control, 5).expect("a usable slug");
        assert_eq!(refined, Utf8PathBuf::from("photos/focus_absolute/5"));
        assert_ne!(rel, refined);

        // A hand-edited slug cannot walk out of the session directory: the transform is
        // the one home and it emits `[a-z0-9_]` runs, so the separators are gone before a
        // path is ever built.
        let traversal = ControlSlug::parse("../../etc").expect("non-empty");
        let neutral = SessionStore::photo_dir_rel(&traversal, 0).expect("it slugs to something");
        assert_eq!(neutral, Utf8PathBuf::from("photos/etc/0"));
        assert!(!neutral.as_str().contains(".."), "{neutral}");

        // And a slug that names no directory at all is refused rather than made up.
        let nothing = ControlSlug::parse("../..").expect("non-empty");
        assert_eq!(
            SessionStore::photo_dir_rel(&nothing, 0)
                .expect_err("punctuation is not a control")
                .kind(),
            ErrorKind::ControlUnknown
        );
    }

    // ------------------------------------------------------------------ atomic writes

    #[test]
    fn a_written_document_is_pretty_printed_and_reads_back() {
        let temp = TempStore::new().expect("a temp dir");
        let lock = temp.store().lock(LockProtocol::PerOperation).expect("free");
        let session = session(uuid_v7(10), "focus");

        let dir = temp
            .store()
            .save_session(&lock, &session)
            .expect("a writable store");
        let raw =
            fs::read_to_string(dir.join(limits::SESSION_FILE).as_std_path()).expect("written");
        assert!(raw.contains('\n'), "a one-line document is not diffable");
        assert!(raw.ends_with('\n'), "no trailing newline: {raw:?}");
        assert_eq!(temp.store().load_session(&dir).expect("readable"), session);
    }

    #[test]
    fn a_document_this_build_could_not_read_back_is_refused_rather_than_written() {
        // Note N13. The store writes what it is handed, and P3b is the first caller that
        // could hand it a version — so the one home checks it once instead of every
        // lifecycle path checking it separately and one of them forgetting. Without this,
        // a session whose `schema_version` was edited (or carried forward from a load that
        // will exist one day) persists as a file `load_session` refuses: the tool's own
        // write, unreadable by the tool.
        let temp = TempStore::new().expect("a temp dir");
        let lock = temp.store().lock(LockProtocol::PerOperation).expect("free");
        let mut ahead = session(uuid_v7(15), "focus");
        ahead.schema_version = limits::SESSION_SCHEMA_VERSION.saturating_add(1);

        let err = temp
            .store()
            .save_session(&lock, &ahead)
            .expect_err("this build cannot read that back");
        assert_eq!(
            err,
            Error::SchemaVersionForeign {
                found: limits::SESSION_SCHEMA_VERSION.saturating_add(1),
                supported: limits::SESSION_SCHEMA_VERSION,
            }
        );
        assert!(
            !temp
                .store()
                .session_dir(&ahead)
                .join(limits::SESSION_FILE)
                .exists(),
            "the refusal wrote the document anyway"
        );

        // Both directions, and the refusal is about the version and nothing else: the same
        // document at this build's version lands and reads back.
        let mut supported = ahead.clone();
        supported.schema_version = limits::SESSION_SCHEMA_VERSION;
        let dir = temp
            .store()
            .save_session(&lock, &supported)
            .expect("a supported version");
        assert_eq!(
            temp.store().load_session(&dir).expect("readable"),
            supported
        );
    }

    #[test]
    fn an_interrupted_write_leaves_the_destination_untouched() {
        // Atomicity, proven rather than asserted: the first document is on disk, the
        // second write is interrupted after its bytes are written and synced and before
        // the rename, and a reader still sees the *whole* first document.
        let mut temp = TempStore::new().expect("a temp dir");
        let lock = temp.store().lock(LockProtocol::PerOperation).expect("free");
        let first = session(uuid_v7(20), "focus");
        let dir = temp.store().save_session(&lock, &first).expect("writable");

        let mut second = first.clone();
        second.goal = "a different goal that must not appear on disk".to_owned();
        second.notes = vec!["x".repeat(4_096)];

        let arranged = temp
            .store_mut()
            .arrange(StoreFault::DiskFull)
            .expect("a scriptable seam");
        let err = temp
            .store()
            .save_session(&lock, &second)
            .expect_err("the disk is full");
        assert_eq!(err.kind(), ErrorKind::StorageIo);

        let loaded = temp
            .store()
            .load_session(&dir)
            .expect("the old file survived");
        assert_eq!(
            loaded, first,
            "a reader saw something other than the old file"
        );
        assert!(
            !loaded.goal.contains("must not appear"),
            "the interrupted write reached the destination"
        );

        // And nothing was left behind: the temp file is unlinked as it drops, so the
        // directory holds exactly the document it held before.
        let names: Vec<String> = fs::read_dir(dir.as_std_path())
            .expect("readable")
            .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec![limits::SESSION_FILE.to_owned()], "{names:?}");

        drop(arranged);
        temp.store_mut().clear_faults();
        temp.store()
            .save_session(&lock, &second)
            .expect("a healthy store writes the same document");
        assert_eq!(
            temp.store().load_session(&dir).expect("readable").goal,
            second.goal
        );
    }

    #[test]
    fn the_named_home_writes_and_reads_back_any_document() {
        // `write_json_atomic` is what design §2.10 names, so it is called by its own name
        // here and not only through the store's methods: a home nothing reaches by its
        // published path is a home in name.
        let temp = TempStore::new().expect("a temp dir");
        let path = temp.root().join("nested/deeper/thing.json");
        write_json_atomic(&path, &vec![1_u32, 2, 3]).expect("it makes the directories too");
        let back: Vec<u32> =
            serde_json::from_slice(&fs::read(path.as_std_path()).expect("written"))
                .expect("valid JSON");
        assert_eq!(back, vec![1, 2, 3]);
    }

    #[test]
    fn a_photo_directory_is_made_under_the_session_and_nowhere_else() {
        let temp = TempStore::new().expect("a temp dir");
        let lock = temp.store().lock(LockProtocol::PerOperation).expect("free");
        let session = session(uuid_v7(120), "focus");
        let dir = temp
            .store()
            .save_session(&lock, &session)
            .expect("writable");
        let slug = ControlSlug::parse("focus_absolute").expect("literal slug");

        let made = temp
            .store()
            .create_photo_dir(&lock, &dir, &slug, 0)
            .expect("makeable");
        assert!(made.is_dir(), "{made} was not made");
        assert_eq!(made, dir.join("photos/focus_absolute/0"));

        // And the refusal travels: a slug with no directory name makes no directory.
        let nothing = ControlSlug::parse("---").expect("non-empty");
        assert_eq!(
            temp.store()
                .create_photo_dir(&lock, &dir, &nothing, 0)
                .expect_err("not a control")
                .kind(),
            ErrorKind::ControlUnknown
        );
    }

    #[test]
    fn a_store_from_an_environment_is_rooted_at_that_environments_state_dir() {
        // The seam `paths` owns (note N2), consumed rather than re-derived: a store built
        // from an environment must land under the same directory `state_dir` resolves.
        let env = schema::paths::MapEnv::from_pairs(&[("XDG_STATE_HOME", "/var/lib/state")]);
        let store = SessionStore::from_env(&env).expect("the variable is set");
        assert_eq!(store.root(), "/var/lib/state/webcam-handler");
        assert_eq!(
            store.sessions_dir(),
            Utf8PathBuf::from("/var/lib/state/webcam-handler/sessions")
        );

        // And the refusal travels rather than being papered over with a guess.
        assert_eq!(
            SessionStore::from_env(&schema::paths::MapEnv::empty())
                .expect_err("nothing is set")
                .kind(),
            ErrorKind::StorageIo
        );
    }

    #[test]
    fn a_held_lock_names_the_file_it_holds_and_the_store_reports_no_script_by_default() {
        let mut temp = TempStore::new().expect("a temp dir");
        assert_eq!(
            temp.store().scripted(),
            None,
            "a store must not arrive with a fault already scripted"
        );

        let held = temp.store().lock(LockProtocol::PerOperation).expect("free");
        assert_eq!(held.path(), temp.store().lock_path());
        assert!(held.path().is_file(), "the lock file was not created");
        drop(held);

        let arranged = temp
            .store_mut()
            .arrange(StoreFault::DiskFull)
            .expect("scriptable");
        assert_eq!(temp.store().scripted(), Some(StoreFault::DiskFull));
        drop(arranged);
        temp.store_mut().clear_faults();
        assert_eq!(temp.store().scripted(), None);
    }

    #[test]
    fn a_write_publishes_a_new_inode_instead_of_overwriting_the_old_one() {
        // The observable shadow of atomicity, and the reason this test exists rather than
        // a comment: "a reader never sees a partial document" is a property about a
        // window no hermetic test can open. What *is* observable is the mechanism that
        // closes the window — a `rename` publishes a *different file*, an overwrite
        // edits the one that is already there. So an implementation that swapped
        // temp-then-rename for `fs::write` would leave every other test in this file
        // green, and turns this one red.
        use std::os::unix::fs::MetadataExt;

        let temp = TempStore::new().expect("a temp dir");
        let lock = temp.store().lock(LockProtocol::PerOperation).expect("free");
        let session = session(uuid_v7(21), "focus");
        let dir = temp
            .store()
            .save_session(&lock, &session)
            .expect("writable");
        let path = dir.join(limits::SESSION_FILE);

        let before = fs::metadata(path.as_std_path()).expect("written").ino();
        let mut second = session.clone();
        second.goal = "revised".to_owned();
        temp.store().save_session(&lock, &second).expect("writable");
        let after = fs::metadata(path.as_std_path()).expect("rewritten").ino();

        assert_ne!(
            before, after,
            "the document was edited in place; a reader can see it half-written"
        );
        assert_eq!(temp.store().load_session(&dir).expect("readable"), second);
    }

    #[test]
    fn the_disk_full_errno_is_the_kernels_own() {
        // Note N10: the injected `ENOSPC` must be the number the kernel produces, not the
        // number the author believes. `/dev/full` is a filesystem that is always full,
        // and it costs nothing.
        let Ok(mut full) = OpenOptions::new().write(true).open("/dev/full") else {
            println!("SKIP: /dev/full is absent, so the kernel cannot be asked for ENOSPC");
            return;
        };
        let err = full
            .write_all(b"anything")
            .and_then(|()| full.flush())
            .expect_err("/dev/full is always full");
        assert_eq!(
            err.raw_os_error(),
            Some(ENOSPC),
            "the kernel disagrees with our ENOSPC: {err}"
        );

        // And the fault carries it through to D13, errno included.
        let Error::StorageIo { errno, .. } = disk_full("/state/session.json".into()) else {
            panic!("disk full is a storage error");
        };
        assert_eq!(errno, Some(ENOSPC));
    }

    #[test]
    fn a_write_with_no_parent_directory_is_refused_rather_than_panicking() {
        let err = write_json_atomic("/".into(), &42_u32).expect_err("root has no parent");
        assert_eq!(err.kind(), ErrorKind::StorageIo);
    }

    #[test]
    fn fsyncing_a_directory_is_supported_here_and_its_failure_is_typed() {
        // Named for exactly what it proves, which is *not* durability. Deleting the
        // `fsync_dir` call from `write_json_atomic` leaves every test in this file green,
        // because the effect of a directory fsync is visible only across a power cut and
        // no hermetic test can produce one — recorded as note N12 rather than covered by
        // a test named for a property it does not check (rubric Part C).
        //
        // What is checkable is that the call is well-formed on this filesystem (some
        // refuse `fsync` on a read-only directory descriptor) and that a failure becomes
        // a typed refusal instead of a panic.
        let temp = TempStore::new().expect("a temp dir");
        fsync_dir(temp.root()).expect("fsync of a directory is supported here");
        assert_eq!(
            fsync_dir(&temp.root().join("not-a-directory"))
                .expect_err("it does not exist")
                .kind(),
            ErrorKind::StorageIo
        );
    }

    #[test]
    fn a_write_the_parent_fsync_refused_reports_the_document_as_published_not_untouched() {
        // Note **N141**, arranged for real rather than scripted. A directory at mode 0300
        // may be written into and renamed within by its owner and may **not** be opened for
        // reading, so the temp file, the write, the `sync_all` and the rename all land and
        // `fsync_dir`'s `File::open` is `EACCES`. That is the one window this classification
        // exists for, and it cannot be reached by any fault that fires earlier.
        use std::os::unix::fs::PermissionsExt as _;

        let temp = TempStore::new().expect("a temp dir");
        let closed = temp.root().join("closed");
        fs::create_dir_all(closed.as_std_path()).expect("creatable");
        let path = closed.join("document.json");
        fs::set_permissions(closed.as_std_path(), fs::Permissions::from_mode(0o300))
            .expect("the mode is ours to set");

        let (error, reach) =
            write_json_atomic_scripted(&path, &serde_json::json!({ "n": 1 }), None).expect_err(
                "a directory that cannot be opened for reading cannot be fsynced — unless \
                 this suite is running as root, which bypasses the mode and is not a \
                 configuration this project's tests run in",
            );
        assert_eq!(reach, Reach::Published);
        assert_eq!(error.kind(), ErrorKind::StorageIo);

        // The whole point of the distinction: the destination holds the *new* document. A
        // caller told "untouched" here is told the write did not happen when it did.
        fs::set_permissions(closed.as_std_path(), fs::Permissions::from_mode(0o700))
            .expect("the mode is ours to set");
        assert_eq!(
            fs::read_to_string(path.as_std_path()).expect("the rename landed"),
            "{\n  \"n\": 1\n}\n"
        );

        // The other direction, out of the same function: a refusal before the rename leaves
        // the destination exactly as it was, and classifies itself that way.
        let (_, reach) = write_json_atomic_scripted(
            &path,
            &serde_json::json!({ "n": 2 }),
            Some(StoreFault::DiskFull),
        )
        .expect_err("no space");
        assert_eq!(reach, Reach::Untouched);
        assert_eq!(
            fs::read_to_string(path.as_std_path()).expect("still there"),
            "{\n  \"n\": 1\n}\n",
            "a failure before the rename published something"
        );
    }

    // ------------------------------------------------------------------ the tree's posture

    /// Every path under `root`, files and directories alike, depth first.
    fn walk(root: &Utf8Path) -> Vec<Utf8PathBuf> {
        let mut found = Vec::new();
        let Ok(listing) = fs::read_dir(root.as_std_path()) else {
            return found;
        };
        for entry in listing.flatten() {
            let Ok(path) = Utf8PathBuf::from_path_buf(entry.path()) else {
                continue;
            };
            if path.is_dir() {
                found.extend(walk(&path));
            }
            found.push(path);
        }
        found
    }

    #[test]
    fn every_directory_and_file_this_store_creates_is_private_to_its_owner() {
        // Note **N142**. Measured before the repair, under this project's own umask:
        // directories 0775, `log.ndjson` 0664, every calibration sample photo 0664, and
        // `session.json` 0600 only by accident of `tempfile::Builder`'s default. A session
        // tree holds photographs of whatever the cameras were pointed at, and a frame may
        // contain a person.
        //
        // The walk is over what a session actually leaves behind rather than over a list
        // written here, so a file a later sub-milestone adds is covered the day it lands
        // rather than the day somebody remembers to add it to an inventory.
        let temp = TempStore::new().expect("a temp dir");
        let lock = temp.store().lock(LockProtocol::PerOperation).expect("free");
        let session = session(uuid_v7(70), "focus");
        let dir = temp
            .store()
            .save_session(&lock, &session)
            .expect("writable");
        temp.store()
            .append_log(&lock, &dir, &entry("focus_absolute", 4))
            .expect("appendable");
        temp.store()
            .create_photo_dir(
                &lock,
                &dir,
                &ControlSlug::parse("focus_absolute").expect("literal slug"),
                0,
            )
            .expect("creatable");

        let found = walk(temp.root());
        assert!(
            found.len() >= 7,
            "the walk found almost nothing, so it proves almost nothing: {found:?}"
        );
        for path in &found {
            let metadata = fs::metadata(path.as_std_path()).expect("it was just walked");
            let mode = metadata.permissions().mode() & schema::paths::MODE_BITS;
            let expected = if metadata.is_dir() {
                STATE_DIR_MODE
            } else {
                STATE_FILE_MODE
            };
            // **The nine bits this tool chose, against the mode it chose them as.** The
            // top three — set-user, set-group, sticky — are masked off because they are
            // inherited from whatever `$TMPDIR` this host uses rather than asked for
            // (note **N150**; `paths::MODE_BITS`' own doc argues the inheritance), and a
            // test that compared them would be red on a group-managed home and green here.
            //
            // Masking all the way down to `GROUP_AND_OTHER_BITS` — which is what
            // `check_state_dir` above asks, for its own reason — is not the same claim and
            // was briefly what this asserted: it let a file this tool creates at 0700, or a
            // photo directory at 0500, pass a message naming 0600 and 0700. The check's
            // question is who can reach the frames; this test's question is whether one
            // decision produced every mode in the tree, so it is the exact one.
            let chosen = mode & !schema::paths::INHERITED_MODE_BITS;
            assert_eq!(
                chosen, expected,
                "{path} is mode {mode:04o}, and this tree is kept at {expected:04o}"
            );
        }
    }

    #[test]
    fn a_state_directory_anybody_can_walk_into_is_refused_rather_than_tightened() {
        // N39's ruling, applied to D9's tree: a wrong mode is refused because a `chmod`
        // would hide the fact that the directory was reachable, and for however long it was
        // reachable it may already have been read. `set_permissions` rather than a mode at
        // `mkdir`, because the umask can only clear bits and this arrangement has to *add*
        // them.
        let temp = TempStore::new().expect("a temp dir");
        let root = temp.root().join("wider");
        create_private_dir(&root).expect("creatable");
        fs::set_permissions(root.as_std_path(), fs::Permissions::from_mode(0o755))
            .expect("the mode is ours to set");

        let store = SessionStore::new(root.clone());
        let refused = store
            .lock(LockProtocol::PerOperation)
            .expect_err("a state directory the whole machine can walk into");
        assert_eq!(refused.kind(), ErrorKind::StorageIo);
        let rendered = refused.to_string();
        assert!(rendered.contains("0755"), "{rendered}");
        assert!(rendered.contains("0700"), "{rendered}");
        assert!(
            rendered.contains("chmod"),
            "the refusal has to name the way out: {rendered}"
        );
        assert_eq!(
            fs::metadata(root.as_std_path())
                .expect("still there")
                .permissions()
                .mode()
                & schema::paths::MODE_BITS,
            0o755,
            "the directory was repaired, which is exactly what note N39 forbids"
        );

        // Healthy twin: the same store over the same directory, once its mode is right.
        fs::set_permissions(
            root.as_std_path(),
            fs::Permissions::from_mode(STATE_DIR_MODE),
        )
        .expect("the mode is ours to set");
        drop(store.lock(LockProtocol::PerOperation).expect("private now"));
    }

    #[test]
    fn a_state_directory_this_tool_creates_under_a_setgid_parent_is_not_refused_on_the_spot() {
        // **The check has to be about who can read the frames, not about the mode word**
        // (note **N150**). Linux inherits `S_ISGID` on `mkdir` under a setgid parent — the
        // arrangement below is one line of `chmod g+s`, and it is ordinary on group-managed
        // and shared homes. With the exposure mask written as `!STATE_DIR_MODE` this tool
        // created the directory at 2700 and refused it *on the same call*: a first run, on a
        // directory nobody but this tool had ever touched, refused permanently — and the
        // remedy it printed, `chmod -R go-rwx`, cannot clear a set-id bit, so an operator
        // following it would loop.
        //
        // The set-group bit is not an exposure: it decides which group new files inherit,
        // and with `go` clear nobody in that group can reach them anyway.
        let temp = TempStore::new().expect("a temp dir");
        let parent = temp.root().join("group-managed");
        create_private_dir(&parent).expect("creatable");
        fs::set_permissions(parent.as_std_path(), fs::Permissions::from_mode(0o2700))
            .expect("the mode is ours to set");

        let root = parent.join("webcam-handler");
        let store = SessionStore::new(root.clone());
        drop(
            store
                .lock(LockProtocol::PerOperation)
                .expect("a directory this tool just created is not one it may refuse"),
        );

        // Non-vacuous: if the kernel or the filesystem under `$TMPDIR` did not propagate the
        // bit there is nothing here to have got wrong, and a green run would prove nothing.
        let mode = fs::metadata(root.as_std_path())
            .expect("created")
            .permissions()
            .mode()
            & schema::paths::MODE_BITS;
        assert_eq!(
            mode & 0o2000,
            0o2000,
            "this filesystem did not inherit the set-group bit, so this test is not arranged: \
             {mode:04o}"
        );
        assert_eq!(mode & schema::paths::GROUP_AND_OTHER_BITS, 0);
    }

    #[test]
    fn a_private_state_directory_somebody_else_owns_is_refused_naming_both_uids() {
        // **N39's other half** (note **N150**). Its own argument is that "a root daemon
        // traverses a 0700 directory belonging to a user without noticing, which is exactly
        // the case this refuses" — and D9's tree had the mode check and not the owner one, so
        // a `webcam-handler-daemon` run as root with `$XDG_STATE_HOME` under somebody else's
        // home would write calibration photographs into a directory that user can read, and
        // be told everything was fine.
        //
        // The uid is a parameter rather than a syscall for exactly this reason: the arm that
        // matters needs a second account, and a check whose failing direction only runs on a
        // host with one is a check that is green because nobody drove it.
        let temp = TempStore::new().expect("a temp dir");
        let root = temp.root().join("somebody-elses");
        create_private_dir(&root).expect("creatable");

        let ours = rustix::process::geteuid().as_raw();
        let refused = check_state_dir(&root, ours.wrapping_add(1))
            .expect_err("a private directory belonging to another account");
        assert_eq!(refused.kind(), ErrorKind::StorageIo);
        let rendered = refused.to_string();
        assert!(rendered.contains(&ours.to_string()), "{rendered}");
        assert!(
            rendered.contains(&ours.wrapping_add(1).to_string()),
            "the refusal names one uid and not the other, so a reader cannot tell which \
             account to fix: {rendered}"
        );

        // The other direction, over the same directory: ours is ours.
        check_state_dir(&root, ours).expect("a directory this account owns");
    }

    // ------------------------------------------------------------------ the log

    #[test]
    fn appending_and_loading_round_trips_in_order() {
        let temp = TempStore::new().expect("a temp dir");
        let lock = temp.store().lock(LockProtocol::PerOperation).expect("free");
        let session = session(uuid_v7(30), "focus");
        let dir = temp
            .store()
            .save_session(&lock, &session)
            .expect("writable");

        let events = vec![entry("focus_absolute", 4), entry("brightness", 8)];
        for event in &events {
            temp.store()
                .append_log(&lock, &dir, event)
                .expect("appendable");
        }
        assert_eq!(temp.store().load_log(&dir).expect("readable"), events);

        // One entry per line, so `grep` and `jq -c` work on it.
        let raw =
            fs::read_to_string(dir.join(limits::SESSION_LOG_FILE).as_std_path()).expect("written");
        assert_eq!(raw.lines().count(), 2, "{raw}");
    }

    #[test]
    fn a_missing_log_is_an_empty_log() {
        let temp = TempStore::new().expect("a temp dir");
        let dir = temp.root().join("sessions/none");
        assert_eq!(
            temp.store().load_log(&dir).expect("absent is empty"),
            vec![]
        );
    }

    #[test]
    fn a_log_that_exists_and_cannot_be_read_refuses_instead_of_reporting_no_history() {
        // "Absent is empty" is one errno wide. Widening the guard to *every* read failure
        // turns a permission problem, a mount that went away or a damaged file into "this
        // session has done nothing" — an availability failure reshaped into a capability
        // answer, which is the conversion AGENTS rule 7 forbids, and here it is silent.
        // P3f's mutation run replaced the `NotFound` guard with `true` and the whole
        // workspace stayed green, because the only case anybody had built was the one the
        // guard is for.
        //
        // The fault is a real kernel refusal rather than a scripted one: `log.ndjson` is a
        // *directory*, so `read` is `EISDIR` for every user including root — no `chmod`,
        // no privilege assumption (the same trick note N15 uses, for the same reason).
        let temp = TempStore::new().expect("a temp dir");
        let dir = temp.root().join("sessions/unreadable");
        fs::create_dir_all(dir.join(limits::SESSION_LOG_FILE).as_std_path())
            .expect("a directory where a file belongs");

        let err = temp
            .store()
            .load_log(&dir)
            .expect_err("a log that exists and cannot be read is not an empty log");
        assert_eq!(err.kind(), ErrorKind::StorageIo);
        let Error::StorageIo { errno, .. } = err else {
            panic!("the refusal must carry the kernel's own number");
        };
        // And the number is the kernel's, asked for here rather than transcribed as a
        // second believed constant (note N10): reading any directory answers with it.
        let from_the_kernel = fs::read(temp.root().as_std_path())
            .expect_err("a directory is not a file")
            .raw_os_error();
        assert!(
            from_the_kernel.is_some(),
            "the kernel gave no errno for reading a directory"
        );
        assert_eq!(errno, from_the_kernel);
    }

    #[test]
    fn a_heal_that_could_not_open_the_log_refuses_rather_than_reporting_it_appendable() {
        // The twin of the test above, one guard along — and the guard the repair that added
        // it did not bring the test for. `heal_log_tail` reads `NotFound` as "there is no
        // tail yet", which is right and is the first append; the floor widened that guard to
        // *every* open failure and the whole workspace stayed green, exactly as it did for
        // `load_log`'s guard at P3f.
        //
        // What the widened guard costs is not bookkeeping. [`SessionStore::append_log`] says
        // "a log this call could not make appendable is not one it may append to", and the
        // heal is the whole of what keeps a torn tail survivable: an open that failed
        // transiently — `EMFILE`, with the process out of descriptors, is the ordinary one —
        // would be reported as healed, and the append behind it would put a terminator after
        // the damage. That is the *interior* corruption `load_log` refuses for ever, and the
        // state this function exists to make unreachable (note **N140**).
        //
        // Asserted at the function because `append_log` cannot see the difference today: its
        // own open meets the same kernel refusal one line later and answers the same
        // `StorageIo`. The fault is the neighbour's real one — `log.ndjson` is a
        // *directory*, so the open is `EISDIR` for every user including root.
        let temp = TempStore::new().expect("a temp dir");
        let dir = temp.root().join("sessions/unhealable");
        let path = dir.join(limits::SESSION_LOG_FILE);
        fs::create_dir_all(path.as_std_path()).expect("a directory where a file belongs");

        let err = heal_log_tail(&path).expect_err("a log that cannot be opened cannot be healed");
        assert_eq!(err.kind(), ErrorKind::StorageIo);
        let Error::StorageIo { errno, .. } = err else {
            panic!("the refusal must carry the kernel's own number");
        };
        let from_the_kernel = OpenOptions::new()
            .read(true)
            .write(true)
            .open(temp.root().as_std_path())
            .expect_err("a directory does not open read-write")
            .raw_os_error();
        assert!(
            from_the_kernel.is_some(),
            "the kernel gave no errno for opening a directory read-write"
        );
        assert_eq!(errno, from_the_kernel);

        // Both directions: the case the guard is actually for is not a failure, or the first
        // append to every session would refuse.
        heal_log_tail(&dir.join("not-written-yet.ndjson"))
            .expect("a log that does not exist yet has no tail to heal");
    }

    /// The malformed fixtures, as bytes. Each one is a shape a crash or an editor
    /// actually produces, and each has an asserted outcome.
    #[test]
    fn the_malformed_log_fixtures_each_get_the_outcome_d9_names() {
        let good = serde_json::to_vec(&entry("focus_absolute", 4)).expect("serializable");
        let other = serde_json::to_vec(&entry("brightness", 8)).expect("serializable");
        let path = Utf8Path::new("/state/log.ndjson");

        let mut torn_tail = good.clone();
        torn_tail.push(b'\n');
        torn_tail.extend_from_slice(&other[..other.len() / 2]);

        let mut torn_middle = good[..good.len() / 2].to_vec();
        torn_middle.push(b'\n');
        torn_middle.extend_from_slice(&good);
        torn_middle.push(b'\n');

        let mut bad_utf8_middle = vec![0xff, 0xfe, b'\n'];
        bad_utf8_middle.extend_from_slice(&good);
        bad_utf8_middle.push(b'\n');

        let mut bad_utf8_tail = good.clone();
        bad_utf8_tail.push(b'\n');
        bad_utf8_tail.extend_from_slice(&[0xff, 0xfe]);

        let mut blank_middle = good.clone();
        blank_middle.extend_from_slice(b"\n\n");
        blank_middle.extend_from_slice(&other);
        blank_middle.push(b'\n');

        // An editor that re-indented the file, or a `>>` that landed on a line of its
        // own: whitespace is as empty as empty, and must not read as corruption.
        let mut whitespace_middle = good.clone();
        whitespace_middle.extend_from_slice(b"\n \t \n");
        whitespace_middle.extend_from_slice(&other);
        whitespace_middle.push(b'\n');

        // A malformed line that IS followed by a terminator — the shape a crash cannot
        // produce, so it is corruption wherever it sits, last line included.
        let mut torn_but_terminated = good.clone();
        torn_but_terminated.push(b'\n');
        torn_but_terminated.extend_from_slice(&other[..other.len() / 2]);
        torn_but_terminated.push(b'\n');

        // (bytes, what a loader must do with them)
        let dropped = [
            ("a torn last line", torn_tail, 1_usize),
            ("an invalid-UTF-8 last line", bad_utf8_tail, 1),
            ("an empty file", Vec::new(), 0),
            ("a file of one newline", b"\n".to_vec(), 0),
            ("a file of only whitespace", b"  \t\n \n".to_vec(), 0),
            ("a blank line in the middle", blank_middle, 2),
            ("a whitespace line in the middle", whitespace_middle, 2),
        ];
        for (name, bytes, kept) in dropped {
            let entries = parse_log(path, &bytes)
                .unwrap_or_else(|err| panic!("{name} must load, not refuse: {err}"));
            assert_eq!(entries.len(), kept, "{name}");
        }

        // (bytes, the line the message must name)
        let refused = [
            ("a torn middle line", torn_middle, "line 1"),
            ("an invalid-UTF-8 middle line", bad_utf8_middle, "line 1"),
            (
                "a torn last line with a terminator",
                torn_but_terminated,
                "line 2",
            ),
        ];
        for (name, bytes, line) in refused {
            let Err(err) = parse_log(path, &bytes) else {
                panic!("{name} is corruption, not a crash: it loaded");
            };
            assert_eq!(err.kind(), ErrorKind::StorageIo, "{name}");
            assert!(
                err.to_string().contains(line),
                "{name}: the message must name {line}: {err}"
            );
        }
    }

    #[test]
    fn a_torn_last_line_that_happens_to_parse_is_kept() {
        // The discriminator is "did it parse", not "was the file terminated": a crash
        // that tore exactly at the newline lost nothing, and dropping a complete record
        // would lose an event that really happened.
        let good = serde_json::to_vec(&entry("focus_absolute", 4)).expect("serializable");
        let entries = parse_log(Utf8Path::new("/state/log.ndjson"), &good).expect("valid");
        assert_eq!(entries.len(), 1);
    }

    // ------------------------------------------------------------------ versioned load

    #[test]
    fn a_foreign_schema_version_is_refused_in_both_directions() {
        let temp = TempStore::new().expect("a temp dir");
        let lock = temp.store().lock(LockProtocol::PerOperation).expect("free");

        for found in [0_u32, limits::SESSION_SCHEMA_VERSION.saturating_add(7)] {
            // Built from bytes rather than by handing `save_session` a foreign document,
            // which it now refuses (note N13). The subject here is the *loader*, and the
            // fixture it has to meet is a real file written by a build that is not this
            // one — so the version is edited in the serialized form, exactly where another
            // build's would differ.
            let session = session(uuid_v7(40), "focus");
            let dir = temp.store().session_dir(&session);
            let serde_json::Value::Object(mut document) =
                serde_json::to_value(&session).expect("serializable")
            else {
                panic!("a session document is a JSON object");
            };
            document.insert("schema_version".to_owned(), serde_json::Value::from(found));
            write_json_atomic(
                &dir.join(limits::SESSION_FILE),
                &serde_json::Value::Object(document),
            )
            .expect("writable");
            let err = temp
                .store()
                .load_session(&dir)
                .expect_err("a version this build does not read");
            assert_eq!(
                err,
                Error::SchemaVersionForeign {
                    found,
                    supported: limits::SESSION_SCHEMA_VERSION,
                },
                "version {found}"
            );
        }

        // The healthy twin: the supported version loads.
        let session = session(uuid_v7(41), "focus");
        let dir = temp
            .store()
            .save_session(&lock, &session)
            .expect("writable");
        assert_eq!(temp.store().load_session(&dir).expect("readable"), session);
    }

    #[test]
    fn a_document_without_a_version_is_refused_rather_than_assumed_current() {
        let path = Utf8Path::new("/state/session.json");
        let err = parse_session(path, br#"{"task": "focus"}"#).expect_err("no version");
        assert_eq!(err.kind(), ErrorKind::StorageIo);
        assert!(err.to_string().contains("schema_version"), "{err}");

        // Not JSON at all is the same kind of answer, and never a panic.
        assert_eq!(
            parse_session(path, b"not json")
                .expect_err("not a document")
                .kind(),
            ErrorKind::StorageIo
        );
    }

    #[test]
    fn a_current_version_with_a_broken_body_does_not_half_parse() {
        let path = Utf8Path::new("/state/session.json");
        let bytes = format!(
            r#"{{"schema_version": {}}}"#,
            limits::SESSION_SCHEMA_VERSION
        );
        let err = parse_session(path, bytes.as_bytes()).expect_err("no session in there");
        assert_eq!(err.kind(), ErrorKind::StorageIo);
    }

    // ------------------------------------------------------------------ lookup

    #[test]
    fn sessions_are_found_by_fingerprint_newest_first() {
        let temp = TempStore::new().expect("a temp dir");
        let lock = temp.store().lock(LockProtocol::PerOperation).expect("free");
        let mine = fingerprint("OBSBOT Tiny 3", "3-1:1.0");
        let theirs = fingerprint("Integrated Camera", "1-6:1.0");

        // Deliberately written oldest-last, so a store that returned insertion order
        // would pass by accident.
        let ids = [uuid_v7(3_000), uuid_v7(1_000), uuid_v7(2_000)];
        for (index, id) in ids.iter().enumerate() {
            let mut s = new_session(
                *id,
                mine.clone(),
                if index == 2 { "exposure" } else { "focus" },
                "g",
                "0.1.0",
                Stamp::epoch(),
            );
            s.controls.insert(
                ControlSlug::parse("focus_absolute").expect("literal slug"),
                ControlSession::default(),
            );
            temp.store().save_session(&lock, &s).expect("writable");
        }
        let other = new_session(
            uuid_v7(9_000),
            theirs.clone(),
            "focus",
            "g",
            "0.1.0",
            Stamp::epoch(),
        );
        temp.store().save_session(&lock, &other).expect("writable");

        let found = temp.store().sessions_for(&mine).expect("listable");
        assert_eq!(
            found.iter().map(|s| s.id).collect::<Vec<_>>(),
            vec![ids[0], ids[2], ids[1]],
            "not newest first"
        );
        assert!(
            found.iter().any(|s| s.task_slug == "exposure"),
            "the task directory is carried: {found:?}"
        );

        // The other camera's session is not in this camera's list, and its own list has
        // exactly the one.
        assert!(!found.iter().any(|s| s.id == other.id));
        assert_eq!(
            temp.store().sessions_for(&theirs).expect("listable").len(),
            1
        );

        // A camera with nothing recorded lists nothing, rather than failing.
        let stranger = fingerprint("Nothing Here", "9-9:9.9");
        assert!(
            temp.store()
                .sessions_for(&stranger)
                .expect("empty")
                .is_empty()
        );
    }

    #[test]
    fn a_directory_that_is_not_a_session_is_skipped_rather_than_refused() {
        let temp = TempStore::new().expect("a temp dir");
        let lock = temp.store().lock(LockProtocol::PerOperation).expect("free");
        let fp = fingerprint("OBSBOT Tiny 3", "3-1:1.0");
        let id = uuid_v7(50);
        temp.store()
            .save_session(&lock, &session(id, "focus"))
            .expect("writable");

        let base = temp.store().sessions_dir().join(fp.slug()).join("focus");
        fs::create_dir_all(base.join("notes-i-took").as_std_path()).expect("makeable");
        fs::create_dir_all(base.join(uuid_v7(60).to_string()).as_std_path())
            .expect("makeable, and empty of any session.json");

        let found = temp.store().sessions_for(&fp).expect("listable");
        assert_eq!(found.iter().map(|s| s.id).collect::<Vec<_>>(), vec![id]);
    }

    #[test]
    fn a_foreign_version_session_still_lists_because_listing_does_not_parse() {
        let mut temp = TempStore::new().expect("a temp dir");
        let lock = temp.store().lock(LockProtocol::PerOperation).expect("free");
        let fp = fingerprint("OBSBOT Tiny 3", "3-1:1.0");
        let arranged = temp
            .store_mut()
            .arrange(StoreFault::ForeignSchemaVersion)
            .expect("scriptable");
        temp.store()
            .save_session(&lock, &session(uuid_v7(70), "focus"))
            .expect("writable");
        drop(arranged);

        let found = temp.store().sessions_for(&fp).expect("listable");
        assert_eq!(found.len(), 1, "a session that cannot load still exists");
        assert_eq!(
            temp.store()
                .load_session(&found[0].dir)
                .expect_err("but it does not load")
                .kind(),
            ErrorKind::SchemaVersionForeign
        );
    }

    // ------------------------------------------------------------------ the lock

    #[test]
    fn the_two_protocols_are_recorded_and_both_exclude_a_second_taker() {
        let temp = TempStore::new().expect("a temp dir");
        for &protocol in LockProtocol::ALL {
            let held = temp.store().lock(protocol).expect("free");
            assert_eq!(held.protocol(), protocol);

            let record = temp.store().holder().expect("somebody holds it");
            assert_eq!(record.protocol, protocol);
            assert_eq!(
                record.pid,
                i32::try_from(std::process::id()).expect("a pid")
            );

            let err = temp
                .peer()
                .lock(LockProtocol::PerOperation)
                .expect_err("held");
            assert_eq!(err.kind(), ErrorKind::StoreLocked);
            match err {
                Error::StoreLocked {
                    holder: Some(holder),
                    protocol: Some(reported),
                } => {
                    assert_eq!(holder.pid, record.pid);
                    assert_eq!(holder.comm, record.comm);
                    // The protocol reaches the refusal from the same record the pid did,
                    // for both protocols in turn — which is what makes the advice D9 owes
                    // the caller a fact about the holder rather than a guess about the
                    // world. The rendering itself is `LockProtocol::advice`'s, asserted in
                    // `webcam-handler-schema`.
                    assert_eq!(reported, protocol);
                }
                other => panic!("a held lock must name its holder where it can: {other:?}"),
            }
            drop(held);
        }
    }

    #[test]
    fn a_dropped_lock_is_released() {
        // The paragraph on `StoreLock` claims the release is the close. This is what
        // keeps that claim honest: without it, the `mem::forget` would leak the lock for
        // the life of the process and every later `lock()` in this suite would refuse.
        let temp = TempStore::new().expect("a temp dir");
        let first = temp.store().lock(LockProtocol::PerOperation).expect("free");
        assert!(temp.peer().lock(LockProtocol::PerOperation).is_err());
        drop(first);
        let second = temp
            .peer()
            .lock(LockProtocol::PerOperation)
            .expect("released on drop");
        drop(second);
        assert!(temp.store().holder().is_none(), "nobody holds it now");
    }

    #[test]
    fn both_orderings_refuse_the_second_taker() {
        // Holder-then-taker, and taker-then-holder. No sleep and no second process: two
        // opens of one file are two `flock` contenders even inside one process, which is
        // what makes this synchronous and therefore deterministic.
        let temp = TempStore::new().expect("a temp dir");

        let daemon = temp
            .store()
            .lock(LockProtocol::HeldForLifetime)
            .expect("free");
        assert_eq!(
            temp.peer()
                .lock(LockProtocol::PerOperation)
                .expect_err("the daemon has it")
                .kind(),
            ErrorKind::StoreLocked
        );
        drop(daemon);

        let cli = temp.peer().lock(LockProtocol::PerOperation).expect("free");
        assert_eq!(
            temp.store()
                .lock(LockProtocol::HeldForLifetime)
                .expect_err("the CLI has it")
                .kind(),
            ErrorKind::StoreLocked
        );
        drop(cli);

        // Both directions: with nobody holding it, either protocol may take it.
        drop(
            temp.store()
                .lock(LockProtocol::HeldForLifetime)
                .expect("free again"),
        );
    }

    #[test]
    fn with_lock_releases_when_the_operation_ends_including_on_refusal() {
        let temp = TempStore::new().expect("a temp dir");
        let session = session(uuid_v7(80), "focus");
        let dir = temp
            .store()
            .with_lock(|lock| temp.store().save_session(lock, &session))
            .expect("free");
        assert!(dir.join(limits::SESSION_FILE).is_file());
        assert!(temp.store().holder().is_none(), "still held afterwards");

        let err = temp
            .store()
            .with_lock(|_| {
                Err::<(), _>(Error::SessionConflict {
                    detail: "the operation refused".to_owned(),
                })
            })
            .expect_err("the operation refused");
        assert_eq!(err.kind(), ErrorKind::SessionConflict);
        assert!(
            temp.store().holder().is_none(),
            "a refusing operation must not keep the lock"
        );
    }

    #[test]
    fn an_unidentifiable_holder_is_reported_as_unidentified() {
        // The lock record is decoration; the lock is the kernel's. A holder whose record is
        // unreadable must produce `holder: None` — an honest "somebody" — rather than an
        // invented pid. The protocol falls with it: advising somebody to switch to
        // `webcam-handler-client` on the strength of a record we could not read would be
        // inventing the one fact the advice turns on.
        let temp = TempStore::new().expect("a temp dir");
        let held = temp.store().lock(LockProtocol::PerOperation).expect("free");
        fs::write(temp.store().lock_path().as_std_path(), b"{ torn").expect("writable");

        match temp.peer().lock(LockProtocol::PerOperation) {
            Err(err @ Error::StoreLocked { holder: None, .. }) => {
                assert!(
                    matches!(err, Error::StoreLocked { protocol: None, .. }),
                    "an unidentified holder named a protocol: {err:?}"
                );
                assert!(!err.to_string().contains("webcam-handler-client"), "{err}");
            }
            other => panic!("expected an unidentified holder, got {other:?}"),
        }
        assert!(
            temp.store().holder().is_none(),
            "an unreadable record names nobody"
        );
        drop(held);
    }

    #[test]
    fn nobody_holds_a_lock_nobody_took() {
        let temp = TempStore::new().expect("a temp dir");
        assert!(
            temp.store().holder().is_none(),
            "the file does not even exist"
        );

        // And a *stale* record — the file left behind by a process that exited — is not
        // reported as a holder, because the kernel is asked first.
        let held = temp.store().lock(LockProtocol::PerOperation).expect("free");
        drop(held);
        assert!(
            !fs::read(temp.store().lock_path().as_std_path())
                .expect("the file outlives the lock")
                .is_empty(),
            "the record is still on disk; it is the *lock* that is gone"
        );
        assert!(
            temp.store().holder().is_none(),
            "a stale record is not a holder"
        );
    }

    // ------------------------------------------------------------------ the double

    #[test]
    fn the_temp_store_is_a_real_directory_that_disappears() {
        let root = {
            let temp = TempStore::new().expect("a temp dir");
            let lock = temp.store().lock(LockProtocol::PerOperation).expect("free");
            temp.store()
                .save_session(&lock, &session(uuid_v7(90), "focus"))
                .expect("writable");
            let root = temp.root().to_owned();
            assert!(root.is_dir(), "{root} is not a directory");
            root
        };
        assert!(!root.exists(), "{root} outlived its fixture");
    }

    #[test]
    fn a_kept_store_stays_on_disk_for_the_operator_to_read() {
        let temp = TempStore::new().expect("a temp dir");
        let lock = temp.store().lock(LockProtocol::PerOperation).expect("free");
        temp.store()
            .save_session(&lock, &session(uuid_v7(100), "focus"))
            .expect("writable");
        drop(lock);
        let root = temp.keep();
        assert!(root.is_dir(), "{root} was removed anyway");
        fs::remove_dir_all(root.as_std_path()).expect("ours to remove");
    }

    #[test]
    fn every_fault_name_and_protocol_name_is_distinct() {
        let mut seen = std::collections::BTreeSet::new();
        for &fault in StoreFault::ALL {
            assert!(seen.insert(fault.as_str()), "{fault:?} duplicates a name");
        }
        let mut seen = std::collections::BTreeSet::new();
        for &protocol in LockProtocol::ALL {
            assert!(
                seen.insert(protocol.as_str()),
                "{protocol:?} duplicates a name"
            );
        }

        // And what each one *renders as* is that name. Both `Display` impls are one line,
        // and P3f's mutation run replaced each body with `Ok(())` — every message that
        // interpolates a fault or a protocol would have carried an empty string where the
        // name belongs — with nothing in the workspace red. A name nobody prints and a
        // name that prints as nothing are the same name to a reader.
        for &fault in StoreFault::ALL {
            assert_eq!(format!("{fault}"), fault.as_str());
        }
        for &protocol in LockProtocol::ALL {
            assert_eq!(format!("{protocol}"), protocol.as_str());
        }
    }

    #[test]
    fn the_lock_record_names_this_process_by_pid_and_by_comm() {
        // `an_unidentifiable_holder_is_reported_as_unidentified` covers the record that
        // cannot be read. Nothing covered the record that can, so P3f's mutation run
        // inverted the emptiness filter on `comm` — making *every* record carry
        // `comm: None` — with the workspace green. "Somebody holds the lock" and
        // "webcam-handler- (pid 1234) holds the lock" were indistinguishable to the suite,
        // and the whole reason the record exists is the difference between them.
        let record = LockRecord::for_this_process(LockProtocol::HeldForLifetime);
        assert_eq!(record.protocol, LockProtocol::HeldForLifetime);
        assert_eq!(record.pid, LockRecord::pid_or_unknown(std::process::id()));
        assert!(record.pid > 0, "this process has a pid");

        // The expectation is read from `/proc`, not from a belief about what this test
        // binary is called.
        let from_proc = fs::read_to_string("/proc/self/comm")
            .expect("Linux, which this project targets")
            .trim()
            .to_owned();
        assert!(!from_proc.is_empty(), "/proc/self/comm named nothing");
        assert_eq!(record.comm.as_deref(), Some(from_proc.as_str()));
        assert_eq!(record.holder().comm.as_deref(), Some(from_proc.as_str()));
    }

    #[test]
    fn a_pid_too_large_for_the_record_is_minus_one_rather_than_a_plausible_process() {
        // The fallback's whole job is to be un-lookupable. `1` is `init`.
        assert_eq!(LockRecord::pid_or_unknown(u32::MAX), -1);
        assert_eq!(
            LockRecord::pid_or_unknown(u32::try_from(i32::MAX).expect("i32::MAX fits a u32") + 1),
            -1
        );
        assert_eq!(LockRecord::pid_or_unknown(4_194_304), 4_194_304);
    }

    #[test]
    fn a_session_tree_that_cannot_be_listed_refuses_instead_of_reporting_no_sessions() {
        // The same one-errno-wide guard as `load_log`'s, on the listing path, and the
        // consequence is worse: an unreadable state directory reads as "this camera has
        // never been calibrated", so `calibrate start` opens a second session beside the
        // one it could not see — which is what `SessionConflict` exists to prevent (N14).
        // P3f's mutation run widened the guard to every failure and the workspace stayed
        // green.
        //
        // Real kernel refusal again: `sessions/` is a *file*, so `read_dir` is `ENOTDIR`
        // for every user including root.
        let temp = TempStore::new().expect("a temp dir");
        fs::write(
            temp.store().sessions_dir().as_std_path(),
            b"not a directory",
        )
        .expect("writable");

        assert_eq!(
            temp.store()
                .all_sessions()
                .expect_err("a tree that exists and cannot be read is not an empty tree")
                .kind(),
            ErrorKind::StorageIo
        );
        assert_eq!(
            temp.store()
                .sessions_for(&fingerprint("Integrated Camera", "usb-0000:00:14.0-4"))
                .expect_err("the same answer, asked per camera")
                .kind(),
            ErrorKind::StorageIo
        );
    }

    #[test]
    fn a_sample_index_survives_a_round_trip_through_the_store() {
        // The store's subject is documents, not fields — but a document that loses its
        // per-control state on the way through is a store that works on empty sessions.
        let temp = TempStore::new().expect("a temp dir");
        let lock = temp.store().lock(LockProtocol::PerOperation).expect("free");
        let mut s = session(uuid_v7(110), "focus");
        let slug = ControlSlug::parse("focus_absolute").expect("literal slug");
        s.queue = vec![slug.clone()];
        s.controls.insert(
            slug.clone(),
            ControlSession {
                status: ControlStatus::Sweeping {
                    plan: schema::session::SweepSpec::Uniform { step: 8 },
                    done: 1,
                    total: 4,
                    precision: 8,
                    adjustments: Vec::new(),
                },
                samples: vec![schema::session::Sample {
                    requested: 42,
                    applied: 40,
                    warnings: Vec::new(),
                    photo: SessionStore::photo_dir_rel(&slug, 0)
                        .expect("a usable slug")
                        .join("42.jpg"),
                    metrics: BTreeMap::new(),
                    captured_at: Stamp::epoch(),
                }],
            },
        );
        let dir = temp.store().save_session(&lock, &s).expect("writable");
        assert_eq!(temp.store().load_session(&dir).expect("readable"), s);
    }
}
