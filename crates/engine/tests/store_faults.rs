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
            // daemon's protocol — and therefore D9's sentence, which is what a `wch`
            // meeting a real daemon sees.
            assert_eq!(*protocol, Some(LockProtocol::HeldForLifetime));
        }
        other => panic!("the holder was identifiable and must be named: {other:?}"),
    }
    assert!(err.to_string().contains("use wchc"), "{err}");

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

    // Now the same tear in the middle: a later run appends after it, so the damage is no
    // longer at the end and no longer explainable as a crash.
    temp.store()
        .append_log(&lock, &dir, &entry("zoom_absolute"))
        .expect("appendable");
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
        },
    }
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
