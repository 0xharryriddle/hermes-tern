//! SQLite-engine faults, not simulated success responses or filesystem ENOSPC.
use super::*;
use std::time::Instant;

fn fixture() -> (PathBuf, Store, Registration, Registration) {
    let root = PathBuf::from(std::env::var_os("TMPDIR").unwrap())
        .join(format!("ht-sqlite-fault-{}", Uuid::new_v4()));
    fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
    let state = root.join("state");
    fs::write(
        root.join("ownership.json"),
        serde_json::json!({
            "retention":"retain; no deletion", "root":root, "paths":[
                root.join("ownership.json"), state, state.join("owner.lock"),
                state.join("coordination.sqlite3"), state.join("coordination.sqlite3-wal"),
                state.join("coordination.sqlite3-shm"), state.join("coordination.sqlite3-journal")
            ],
        })
        .to_string(),
    )
    .unwrap();
    let mut store = Store::open(&state, &root, StoreLimits::default()).unwrap();
    let a = store.register("hermes", "p", "a", "m", "p").unwrap();
    let b = store.register("hermes", "p", "b", "m", "p").unwrap();
    store.route(&a.actor_id, &b.actor_id, true).unwrap();
    (root, store, a, b)
}
fn message(recipient: &str, text: &str) -> MessageInput {
    MessageInput {
        message_id: Uuid::new_v4().to_string(),
        recipient: recipient.into(),
        kind: "context".into(),
        payload: json!({"text":text}),
        ttl_seconds: 300,
        correlation_id: None,
    }
}
fn counts(db: &Connection) -> (i64, i64, String) {
    db.query_row("SELECT (SELECT count(*) FROM messages), (SELECT count(*) FROM events), (SELECT value FROM meta WHERE key='clock')", [], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap()
}

#[test]
fn failed_initial_metadata_write_rolls_back_schema_and_version() {
    let root = PathBuf::from(std::env::var_os("TMPDIR").unwrap())
        .join(format!("ht-schema-fault-{}", Uuid::new_v4()));
    fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
    let path = root.join("init.sqlite3");
    private_file(&path).unwrap();
    fs::write(root.join("ownership.json"),json!({"retention":"retain","paths":[root,path,root.join("ownership.json"),root.join("init.sqlite3-wal"),root.join("init.sqlite3-shm")] }).to_string()).unwrap();
    let mut db = Connection::open(&path).unwrap();
    db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA max_page_count=32")
        .unwrap();
    // Exceeds the real SQLite page cap at the metadata INSERT, after DDL succeeds.
    assert!(schema::initialize(&mut db, &"x".repeat(1024 * 1024)).is_err());
    drop(db);
    let mut db = Connection::open(&path).unwrap();
    assert_eq!(
        db.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        0,
        "failed metadata left committed schema version"
    );
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM sqlite_schema WHERE type='table'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0,
        "failed initialization left partial tables"
    );
    schema::initialize(&mut db, root.to_str().unwrap()).unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM meta", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        3
    );
}

#[test]
fn sqlite_full_rolls_back_admission_and_preserves_prior_work_after_reopen() {
    let (root, mut s, a, b) = fixture();
    let prior = message(&b.actor_id, "accepted before storage pressure");
    s.send(&a.credential, &prior, 100).unwrap();
    let before = counts(&s.db);
    let pages: i64 =
        s.db.pragma_query_value(None, "page_count", |r| r.get(0))
            .unwrap();
    let imposed: i64 =
        s.db.pragma_update_and_check(None, "max_page_count", pages, |r| r.get(0))
            .unwrap();
    assert_eq!(imposed, pages);
    // Positive fault control: SQLite itself returns SQLITE_FULL on this connection.
    let fault =
        s.db.execute(
            "INSERT INTO events(kind,entity) VALUES('probe',zeroblob(1048576))",
            [],
        )
        .unwrap_err();
    assert_eq!(
        fault.sqlite_error_code(),
        Some(rusqlite::ErrorCode::DiskFull)
    );
    assert!(s.db.is_autocommit());
    let input = message(&b.actor_id, &"x".repeat(30000));
    let error = s.send(&a.credential, &input, 101).unwrap_err();
    assert_eq!(
        error.to_string(),
        "storage error: database operation failed; no acceptance claimed"
    );
    assert_eq!(
        counts(&s.db),
        before,
        "message, clock and event must all roll back"
    );
    assert!(s.db.is_autocommit());
    let state = s.state_dir.clone();
    drop(s);
    let mut s = Store::open(&state, &root, StoreLimits::default()).unwrap();
    // Open restores the production bound after the test-only limit is removed.
    assert_eq!(s.messages().unwrap().len(), 1);
    let result = s.send(&a.credential, &input, 101).unwrap();
    assert_eq!(result["duplicate"], false);
    assert_eq!(
        s.send(&a.credential, &input, 101).unwrap()["duplicate"],
        true
    );
    let delivered = s.receive(&b.credential, 102).unwrap();
    assert_eq!(delivered.len(), 2);
    assert_eq!(delivered[0]["message_id"], prior.message_id);
}

#[test]
fn sqlite_full_during_task_result_does_not_commit_completion_or_event() {
    let (root, mut s, a, _) = fixture();
    let task = Uuid::new_v4().to_string();
    s.claim(&a.credential, &task, 100, 300).unwrap();
    let before = counts(&s.db);
    let pages: i64 =
        s.db.pragma_query_value(None, "page_count", |r| r.get(0))
            .unwrap();
    s.db.pragma_update(None, "max_page_count", pages).unwrap();
    let result = "x".repeat(30000);
    assert!(s.complete(&a.credential, &task, "1", &result, 101).is_err());
    assert_eq!(counts(&s.db), before);
    let row: (String, Option<String>) =
        s.db.query_row("SELECT state,result FROM tasks WHERE id=?1", [&task], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    assert_eq!(row, ("active".into(), None));
    let state = s.state_dir.clone();
    drop(s);
    let mut s = Store::open(&state, &root, StoreLimits::default()).unwrap();
    assert_eq!(
        s.complete(&a.credential, &task, "1", &result, 101).unwrap()["state"],
        "complete"
    );
    // Exact result replay is idempotent even after the original deadline.
    assert_eq!(
        s.complete(&a.credential, &task, "1", &result, 500).unwrap()["state"],
        "complete"
    );
    assert!(
        s.complete(&a.credential, &task, "1", "changed", 500)
            .is_err()
    );
}

#[test]
fn sqlite_busy_is_bounded_and_refused_admission_is_retryable_without_duplicates() {
    let (_, mut s, a, b) = fixture();
    let competing = Connection::open(s.state_dir.join("coordination.sqlite3")).unwrap();
    competing.execute_batch("BEGIN IMMEDIATE").unwrap();
    let before = counts(&competing);
    let input = message(&b.actor_id, "busy fixture");
    let start = Instant::now();
    assert!(s.send(&a.credential, &input, 100).is_err());
    assert!(
        start.elapsed() < Duration::from_secs(5),
        "busy wait exceeded bound"
    );
    assert_eq!(counts(&competing), before);
    competing.execute_batch("ROLLBACK").unwrap();
    assert_eq!(
        s.send(&a.credential, &input, 100).unwrap()["duplicate"],
        false
    );
    assert_eq!(
        s.send(&a.credential, &input, 100).unwrap()["duplicate"],
        true
    );
    assert_eq!(s.messages().unwrap().len(), 1);
}
