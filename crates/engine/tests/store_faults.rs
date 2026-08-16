//! The session store's answer to every fault its seam can exhibit (design §2.9, docs/7
//! P3a).
//!
//! The seam table gives the store four faults — full disk, lock held, torn `log.ndjson`
//! line, foreign `schema_version` — and this suite drives each of them **both
//! directions**: the fault produces its typed refusal, and the same operation on a
//! healthy store succeeds. A test that only asserts the sad path is half a test: it
//! cannot tell "the store refuses when the disk is full" from "the store refuses".
//!
//! The walk is an exhaustive `match` over [`StoreFault`], for the reason the fake's own
//! menu test gives: a fault the compiler cannot force somebody to answer is a fault
//! nobody answers. A fifth variant stops this build until the store has a named behaviour
//! for it.
//!
//! Three of the four are arranged **for real** — a real `flock` held over an independent
//! open file description, a real truncated append, a real document carrying a version one
//! ahead of this build's. Only the full disk is synthesized, and its `errno` is the one
//! the kernel gives `/dev/full` (the unit test `the_disk_full_errno_is_the_kernels_own`
//! is what checks that). Note N10's lesson, applied where it applies: the inverse arm is
//! driven by the thing under test wherever the thing under test can be arranged.

use engine::store::{LockProtocol, SessionStore, StoreFault, TempStore};
use schema::camera::CameraFingerprint;
use schema::control::ControlSlug;
use schema::session::{LogEntry, Session, SessionEvent, new_session};
use schema::time::Stamp;
use schema::{Error, ErrorKind, limits};
use uuid::Uuid;

#[test]
fn every_store_fault_has_a_typed_answer_and_a_healthy_twin() {
    for &fault in StoreFault::ALL {
        match fault {
            StoreFault::DiskFull => a_full_disk_refuses_and_an_empty_one_does_not(),
            StoreFault::LockHeld => a_held_lock_refuses_and_a_free_one_does_not(),
            StoreFault::TornLogLine => a_torn_line_is_dropped_or_refused_by_where_it_is(),
            StoreFault::ForeignSchemaVersion => {
                a_foreign_version_refuses_and_the_supported_one_does_not();
            }
        }
    }
}

// ------------------------------------------------------------------------ the behaviours

fn a_full_disk_refuses_and_an_empty_one_does_not() {
    let mut temp = TempStore::new().expect("a temp dir");
    let lock = temp.store().lock(LockProtocol::PerOperation).expect("free");
    let session = session(1_000, "focus");

    // Sad: both writing paths refuse, and both carry ENOSPC rather than a bare string.
    let arranged = temp
        .store_mut()
        .arrange(StoreFault::DiskFull)
        .expect("scriptable");
    let dir = temp.store().session_dir(&session);
    for err in [
        temp.store()
            .save_session(&lock, &session)
            .expect_err("no space for a session"),
        temp.store()
            .append_log(&lock, &dir, &entry("focus_absolute"))
            .expect_err("no space for a log line"),
    ] {
        assert_eq!(err.kind(), ErrorKind::StorageIo);
        let Error::StorageIo { errno, path, .. } = err else {
            unreachable!("kind() said otherwise");
        };
        assert_eq!(
            errno,
            Some(28),
            "a full disk is ENOSPC, and it must be named"
        );
        assert!(
            path.starts_with(temp.root()),
            "the refusal must name the file that failed: {path}"
        );
    }
    // Nothing landed.
    assert!(!dir.join(limits::SESSION_FILE).exists());

    // Healthy: the same two operations on a store with room.
    drop(arranged);
    temp.store_mut().clear_faults();
    let dir = temp
        .store()
        .save_session(&lock, &session)
        .expect("a store with room");
    temp.store()
        .append_log(&lock, &dir, &entry("focus_absolute"))
        .expect("a store with room");
    assert_eq!(temp.store().load_session(&dir).expect("readable"), session);
    assert_eq!(temp.store().load_log(&dir).expect("readable").len(), 1);
}

fn a_held_lock_refuses_and_a_free_one_does_not() {
    let mut temp = TempStore::new().expect("a temp dir");

    // Sad, arranged for real: the arrangement *is* a held `flock`.
    let arranged = temp
        .store_mut()
        .arrange(StoreFault::LockHeld)
        .expect("the lock was free to take");
    let err = temp
        .peer()
        .lock(LockProtocol::PerOperation)
        .expect_err("somebody holds it");
    assert_eq!(err.kind(), ErrorKind::StoreLocked);
    match &err {
        Error::StoreLocked {
            holder: Some(holder),
            protocol,
        } => {
            assert_eq!(
                holder.pid,
                i32::try_from(std::process::id()).expect("a pid"),
                "the refusal named the wrong holder"
            );
            // The arrangement takes the lock as a daemon would, so the refusal carries the
            // daemon's protocol — and therefore D9's sentence, which is what a
            // `webcam-handler-cli` meeting a real daemon sees.
            assert_eq!(*protocol, Some(LockProtocol::HeldForLifetime));
        }
        other => panic!("the holder was identifiable and must be named: {other:?}"),
    }
    assert!(
        err.to_string().contains("use webcam-handler-client"),
        "{err}"
    );

    // The scripted refusal answers the same way as the real one, which is the check that
    // keeps the script honest: a double that refused differently would be a double
    // nobody could trust (rubric A9). Compared whole rather than by kind — the holder and
    // the protocol are what a caller acts on, and a script that named neither would still
    // have the right kind.
    let scripted = temp
        .store()
        .lock(LockProtocol::PerOperation)
        .expect_err("scripted");
    assert_eq!(scripted.kind(), ErrorKind::StoreLocked);
    assert_eq!(scripted, err);

    // Healthy: release it, and both protocols may take it again.
    drop(arranged);
    temp.store_mut().clear_faults();
    for &protocol in LockProtocol::ALL {
        let held = temp
            .peer()
            .lock(protocol)
            .unwrap_or_else(|err| panic!("a free lock refused {protocol}: {err}"));
        assert_eq!(held.protocol(), protocol);
        drop(held);
    }
}

fn a_torn_line_is_dropped_or_refused_by_where_it_is() {
    let mut temp = TempStore::new().expect("a temp dir");
    let lock = temp.store().lock(LockProtocol::PerOperation).expect("free");
    let session = session(2_000, "focus");
    let dir = temp
        .store()
        .save_session(&lock, &session)
        .expect("writable");

    // A first, whole event.
    temp.store()
        .append_log(&lock, &dir, &entry("focus_absolute"))
        .expect("appendable");

    // Then a process dies mid-append. This is a *real* truncated write; the loader below
    // meets real bytes.
    let arranged = temp
        .store_mut()
        .arrange(StoreFault::TornLogLine)
        .expect("scriptable");
    temp.store()
        .append_log(&lock, &dir, &entry("brightness"))
        .expect("the write itself succeeded — it was just short");
    drop(arranged);
    temp.store_mut().clear_faults();

    // **The tear is a partial record, and that is asserted rather than assumed.** Every
    // other assertion in this test reads the log through `load_log`, and through
    // `load_log` an entry that was never written and an entry that was written torn and
    // dropped look identical. So a fault that quietly stopped tearing — writing nothing,
    // or a single `{` — would leave this test green while the loader stopped meeting the
    // bytes the fixture exists to hand it. P3f's mutation run found exactly that:
    // truncating to `len % 2` instead of `len / 2` survived the whole workspace suite.
    // A fixture that has stopped producing the condition it is named for is "skip reads
    // as pass" in a fault-menu costume.
    let mut whole = serde_json::to_vec(&entry("brightness")).expect("serializable");
    whole.push(b'\n');
    let raw = std::fs::read(dir.join(limits::SESSION_LOG_FILE).as_std_path()).expect("readable");
    let first_terminator = raw
        .iter()
        .position(|byte| *byte == b'\n')
        .expect("the whole entry before the tear is terminated");
    assert_eq!(
        &raw[first_terminator + 1..],
        &whole[..whole.len() / 2],
        "the tear must be the first half of the entry that was being appended"
    );

    // A torn *last* line is a crash mid-append, and D9 drops it. The event before it
    // survives: dropping the tail must not lose the history.
    let loaded = temp
        .store()
        .load_log(&dir)
        .expect("a torn tail is survivable");
    assert_eq!(loaded.len(), 1, "the whole entry before the tear was lost");
    assert_eq!(loaded[0], entry("focus_absolute"));

    // Now the same tear in the middle: damage with a terminator behind it, which a crash
    // mid-append cannot produce and which this store therefore refuses rather than guesses
    // at. **Written as bytes**, and that is the change note **N140** made: an append used to
    // put this file into this state by writing at whatever byte the last writer stopped at,
    // so the store manufactured its own corruption. It heals the tail now
    // (`a_crash_torn_tail_is_healed_by_the_next_append_rather_than_left_to_refuse_for_ever`),
    // which leaves an interior tear meaning what it says: somebody other than this tool
    // damaged the file. The refusal itself is untouched — note N12 pins it against a seeded
    // mutant, and it is still the right answer.
    let mut corrupted = raw.clone();
    corrupted.extend_from_slice(b"\n");
    corrupted.extend_from_slice(&whole);
    temp.plant_log(&dir, &corrupted).expect("writable");
    let err = temp
        .store()
        .load_log(&dir)
        .expect_err("a torn middle line is corruption");
    assert_eq!(err.kind(), ErrorKind::StorageIo);
    assert!(
        err.to_string().contains("line 2"),
        "the refusal must name the line: {err}"
    );

    // Healthy: an untorn log of the same events loads whole.
    let clean = temp.store().session_dir(&session).join("clean");
    for event in ["focus_absolute", "brightness", "zoom_absolute"] {
        temp.store()
            .append_log(&lock, &clean, &entry(event))
            .expect("appendable");
    }
    assert_eq!(temp.store().load_log(&clean).expect("readable").len(), 3);
}

fn a_foreign_version_refuses_and_the_supported_one_does_not() {
    let mut temp = TempStore::new().expect("a temp dir");
    let lock = temp.store().lock(LockProtocol::PerOperation).expect("free");
    let session = session(3_000, "focus");

    // Sad: a document from a build ahead of this one — real bytes, written by the store.
    let arranged = temp
        .store_mut()
        .arrange(StoreFault::ForeignSchemaVersion)
        .expect("scriptable");
    let dir = temp
        .store()
        .save_session(&lock, &session)
        .expect("writable");
    drop(arranged);
    temp.store_mut().clear_faults();

    let err = temp
        .store()
        .load_session(&dir)
        .expect_err("a version this build does not read");
    assert_eq!(
        err,
        Error::SchemaVersionForeign {
            found: limits::SESSION_SCHEMA_VERSION + 1,
            supported: limits::SESSION_SCHEMA_VERSION,
        },
        "the refusal must name both versions so the operator knows which build to use"
    );

    // Healthy: rewritten by this build, it loads — and the document is the one that went
    // in, not a repaired approximation of the foreign one.
    temp.store()
        .save_session(&lock, &session)
        .expect("writable");
    assert_eq!(temp.store().load_session(&dir).expect("readable"), session);
}

// ------------------------------------------------------------------------ fixtures

fn fingerprint() -> CameraFingerprint {
    CameraFingerprint {
        bus_path: "3-1:1.0".to_owned(),
        usb_id: None,
        card: "OBSBOT Tiny 3".to_owned(),
        driver: "uvcvideo".to_owned(),
        serial: None,
    }
}

fn session(millis: u64, task: &str) -> Session {
    let id = Uuid::new_v7(uuid::Timestamp::from_unix(
        uuid::NoContext,
        millis / 1_000,
        u32::try_from((millis % 1_000) * 1_000_000).expect("sub-second nanos fit"),
    ));
    new_session(
        id,
        fingerprint(),
        task,
        "legible text",
        "0.1.0",
        Stamp::epoch(),
    )
}

fn entry(control: &str) -> LogEntry {
    LogEntry {
        at: Stamp::epoch(),
        event: SessionEvent::SweepStarted {
            control: ControlSlug::parse(control).expect("literal slug"),
            total: 4,
            precision: 1,
            adjustments: Vec::new(),
        },
    }
}

/// The repair note **N140** landed: an append inspects the tail it is about to write behind.
///
/// Both directions, because the two tails are two different answers and getting either one
/// wrong loses something. An unparsable tail is the crash `load_log` already drops, and the
/// heal makes that drop durable; a *parseable* one is a whole entry whose terminator never
/// reached the platter, and dropping that would throw away a record that is entirely there.
///
/// The bug this turns red: `append_log` writing at whatever byte the last writer stopped at.
/// That puts a terminator behind the damage, which turns a survivable torn tail into the
/// interior corruption `load_log` refuses — permanently, with no verb that repairs it.
#[test]
fn a_crash_torn_tail_is_healed_by_the_next_append_rather_than_left_to_refuse_for_ever() {
    let temp = TempStore::new().expect("a temp dir");
    let lock = temp.store().lock(LockProtocol::PerOperation).expect("free");
    let session = session(5_000, "focus");
    let dir = temp.store().session_dir(&session);
    let path = dir.join(limits::SESSION_LOG_FILE);
    let line = |control: &str| {
        let mut bytes = serde_json::to_vec(&entry(control)).expect("serializable");
        bytes.push(b'\n');
        bytes
    };

    // A crash that stopped mid-JSON. Half a line is not a line, so the entry is gone — but
    // the one before it, and the ones after, must not be.
    let mut torn = line("focus_absolute");
    let brightness = line("brightness");
    torn.extend_from_slice(&brightness[..brightness.len() / 2]);
    temp.plant_log(&dir, &torn).expect("writable");

    temp.store()
        .append_log(&lock, &dir, &entry("zoom_absolute"))
        .expect("appendable");
    let loaded = temp
        .store()
        .load_log(&dir)
        .expect("a log a crash tore and this build appended to is still readable");
    assert_eq!(
        loaded,
        vec![entry("focus_absolute"), entry("zoom_absolute")],
        "the torn tail was not dropped, or the history before it was"
    );

    // A crash that stopped between the entry and its terminator. The record is whole, so it
    // is kept rather than thrown away for want of one byte.
    let mut unterminated = line("focus_absolute");
    let whole = line("brightness");
    unterminated.extend_from_slice(&whole[..whole.len() - 1]);
    temp.plant_log(&dir, &unterminated).expect("writable");

    temp.store()
        .append_log(&lock, &dir, &entry("zoom_absolute"))
        .expect("appendable");
    assert_eq!(
        temp.store().load_log(&dir).expect("readable"),
        vec![
            entry("focus_absolute"),
            entry("brightness"),
            entry("zoom_absolute")
        ],
        "an entry that was entirely written was discarded for want of its newline"
    );

    // And a healthy log is left exactly as it was: the heal reads a byte and does nothing,
    // so an append cannot rewrite history it had no reason to touch.
    let before = std::fs::read(path.as_std_path()).expect("readable");
    temp.store()
        .append_log(&lock, &dir, &entry("pan_absolute"))
        .expect("appendable");
    let after = std::fs::read(path.as_std_path()).expect("readable");
    assert_eq!(
        &after[..before.len()],
        before.as_slice(),
        "the heal changed a file that had nothing wrong with it"
    );
}

/// The same two answers for a tail longer than the heal's backward-scan chunk (note
/// **N140**).
///
/// **The bug this turns red is the window the heal first landed with.** It read back at most
/// eight kibibytes of unterminated tail and dropped anything longer *without parsing it*, on
/// the premise that "a tail longer than this cannot be a log line this build wrote". The
/// premise is false: `SessionEvent::Started` carries the operator's or the agent's own `goal`
/// text and nothing in `limits` bounds it, so an AI harness writing a task description makes
/// a line of any length. And the consequence was worse than the premise — `parse_log` parses
/// an unterminated last segment and *keeps* it when it parses, so the two halves of D9's one
/// rule gave different answers for the same file, and when the over-long line was the only
/// one in the log the heal truncated the file to nothing.
///
/// The fixtures are the cases the first test could not have: its lines are all short.
#[test]
fn a_torn_tail_longer_than_the_scan_chunk_is_still_parsed_before_it_is_kept_or_dropped() {
    let temp = TempStore::new().expect("a temp dir");
    let lock = temp.store().lock(LockProtocol::PerOperation).expect("free");
    let session = session(6_000, "focus");
    let dir = temp.store().session_dir(&session);
    let path = dir.join(limits::SESSION_LOG_FILE);

    // A `goal` of 32 KiB — four times the scan chunk, and a plausible thing for an agent
    // harness to write, which is the whole point of the case.
    let long = LogEntry {
        at: Stamp::epoch(),
        event: SessionEvent::Started {
            goal: "photograph the device under test until the serial number is legible; "
                .repeat(512),
        },
    };
    let mut serialized = serde_json::to_vec(&long).expect("serializable");
    serialized.push(b'\n');
    assert!(
        serialized.len() > 8 * 1024,
        "the fixture is inside the old window, so it proves nothing: {} bytes",
        serialized.len()
    );

    // Case one, and the destructive one: the over-long line is the *only* line, and its
    // terminator never landed. It parses, so it is a record that is entirely present.
    temp.plant_log(&dir, &serialized[..serialized.len() - 1])
        .expect("writable");
    temp.store()
        .append_log(&lock, &dir, &entry("zoom_absolute"))
        .expect("appendable");
    assert_eq!(
        temp.store().load_log(&dir).expect("readable"),
        vec![long.clone(), entry("zoom_absolute")],
        "an entry longer than the scan chunk was thrown away without being read"
    );

    // Case two, the other direction at the same length: an over-long tail that does not
    // parse is the torn write the loader drops, and the heal makes that durable — while the
    // whole line in front of it survives.
    let mut torn = serialized.clone();
    torn.extend_from_slice(&serialized[..serialized.len() / 2]);
    temp.plant_log(&dir, &torn).expect("writable");
    temp.store()
        .append_log(&lock, &dir, &entry("pan_absolute"))
        .expect("appendable");
    assert_eq!(
        temp.store().load_log(&dir).expect("readable"),
        vec![long, entry("pan_absolute")],
        "the over-long torn tail was kept, or the whole line before it was lost"
    );
    assert!(
        std::fs::read(path.as_std_path()).expect("readable").len() > serialized.len(),
        "the heal emptied a log that held a whole entry"
    );
}

/// A store that is not a [`TempStore`] still refuses the same way — the double is a
/// fixture, not a different implementation.
#[test]
fn the_double_is_the_same_store_rooted_somewhere_disposable() {
    let temp = TempStore::new().expect("a temp dir");
    let plain = SessionStore::new(temp.root().join("elsewhere"));
    let lock = plain.lock(LockProtocol::PerOperation).expect("free");
    let session = session(4_000, "focus");
    let dir = plain.save_session(&lock, &session).expect("writable");
    assert_eq!(plain.load_session(&dir).expect("readable"), session);
    assert!(
        dir.starts_with(temp.root().join("elsewhere")),
        "{dir} is not under the store it was made from"
    );
}
