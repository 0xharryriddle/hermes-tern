use hermes_tern_store::{Registration, Store, StoreLimits};
use rusqlite::Connection;
use serde_json::{Value, json};
use std::{fs, os::unix::fs::DirBuilderExt, path::PathBuf};

struct Fixture {
    root: PathBuf,
    state: PathBuf,
    store: Option<Store>,
    a: Registration,
    b: Registration,
    limits: StoreLimits,
}
impl Fixture {
    fn new(limits: StoreLimits) -> Self {
        let root = PathBuf::from(std::env::var_os("TMPDIR").unwrap())
            .join(format!("ht-task-release-{}", uuid::Uuid::new_v4()));
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        let state = root.join("state");
        let mut s = Store::open(&state, &root, limits.clone()).unwrap();
        let a = s
            .register("hermes", "fixture", "a", "model", "provider")
            .unwrap();
        let b = s
            .register("hermes", "fixture", "b", "model", "provider")
            .unwrap();
        Self {
            root,
            state,
            store: Some(s),
            a,
            b,
            limits,
        }
    }
    fn reopen(&mut self) {
        drop(self.store.take());
        self.store = Some(Store::open(&self.state, &self.root, self.limits.clone()).unwrap());
    }
    fn db(&self) -> Connection {
        Connection::open(self.state.join("coordination.sqlite3")).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        drop(self.store.take());
        let mut paths = vec![
            self.root.clone(),
            self.state.clone(),
            self.root.join("ownership.json"),
        ];
        paths.extend(
            fs::read_dir(&self.state)
                .unwrap()
                .map(|p| p.unwrap().path()),
        );
        fs::write(
            self.root.join("ownership.json"),
            json!({"retention":"retain; no deletion","paths":paths}).to_string(),
        )
        .unwrap();
    }
}
fn release(
    s: &mut Store,
    task: &str,
    actor: &Registration,
    generation: &str,
) -> hermes_tern_core::error::Result<Value> {
    s.release_stopped(task, &actor.actor_id, &actor.incarnation, generation, true)
}
fn expected(task: &str, a: &Registration, generation: &str) -> Value {
    json!({"task_id":task,"owner_actor_id":a.actor_id,"incarnation":a.incarnation,"generation":generation,
        "state":"released","released":true,"process_stopped":"operator_attested_not_enforced"})
}

#[test]
fn exact_release_replay_is_nonmutating_but_old_generation_cannot_release_reclaimed_task() {
    let mut f = Fixture::new(StoreLimits::default());
    let task = uuid::Uuid::new_v4().to_string();
    let s = f.store.as_mut().unwrap();
    s.claim(&f.a.credential, &task, 100, 300).unwrap();
    let first = release(s, &task, &f.a, "1").unwrap();
    assert_eq!(first, expected(&task, &f.a, "1"));
    let before = s.observation_snapshot().unwrap();
    assert_eq!(release(s, &task, &f.a, "1").unwrap(), first);
    assert_eq!(s.observation_snapshot().unwrap(), before);
    assert_eq!(
        s.claim(&f.a.credential, &task, 101, 300).unwrap()["generation"],
        "2"
    );
    let before = s.observation_snapshot().unwrap();
    assert!(release(s, &task, &f.a, "1").is_err());
    assert_eq!(s.observation_snapshot().unwrap(), before);
    s.complete(&f.a.credential, &task, "2", "fixture", 102)
        .unwrap();
    let before = s.inspect_task(&task).unwrap();
    assert!(release(s, &task, &f.a, "2").is_err());
    assert_eq!(s.inspect_task(&task).unwrap(), before);
}

#[test]
fn release_refuses_each_stale_identity_field_and_noncanonical_generation_without_mutation() {
    let mut f = Fixture::new(StoreLimits::default());
    let task = uuid::Uuid::new_v4().to_string();
    let other = uuid::Uuid::new_v4().to_string();
    let db = f.db();
    let s = f.store.as_mut().unwrap();
    s.claim(&f.a.credential, &task, 100, 300).unwrap();
    let before = s.observation_snapshot().unwrap();
    for (t, owner, inc, generation, attests) in [
        (
            other.as_str(),
            f.a.actor_id.as_str(),
            f.a.incarnation.as_str(),
            "1",
            true,
        ),
        (
            task.as_str(),
            f.b.actor_id.as_str(),
            f.a.incarnation.as_str(),
            "1",
            true,
        ),
        (
            task.as_str(),
            f.a.actor_id.as_str(),
            f.b.incarnation.as_str(),
            "1",
            true,
        ),
        (
            task.as_str(),
            f.a.actor_id.as_str(),
            f.a.incarnation.as_str(),
            "2",
            true,
        ),
        (
            task.as_str(),
            f.a.actor_id.as_str(),
            f.a.incarnation.as_str(),
            "1",
            false,
        ),
    ] {
        assert!(
            s.release_stopped(t, owner, inc, generation, attests)
                .is_err()
        );
        assert_eq!(s.observation_snapshot().unwrap(), before);
    }
    for generation in ["0", "-1", "+1", "01", "1.0", " 1", "9223372036854775808"] {
        assert!(release(s, &task, &f.a, generation).is_err());
    }
    assert_eq!(s.observation_snapshot().unwrap(), before);
    assert_eq!(
        db.query_row("SELECT value FROM meta WHERE key='clock'", [], |r| r
            .get::<_, String>(0))
            .unwrap(),
        "100"
    );
    assert_eq!(
        release(s, &task, &f.a, "1").unwrap(),
        expected(&task, &f.a, "1")
    );
}

#[test]
fn stored_lease_incarnation_remains_releasable_after_rotation_and_restart() {
    let mut f = Fixture::new(StoreLimits::default());
    let task = uuid::Uuid::new_v4().to_string();
    let s = f.store.as_mut().unwrap();
    s.claim(&f.a.credential, &task, 100, 300).unwrap();
    let rotated = s.rotate(&f.a.actor_id).unwrap();
    assert!(release(s, &task, &rotated, "1").is_err());
    f.reopen();
    let s = f.store.as_mut().unwrap();
    assert_eq!(
        release(s, &task, &f.a, "1").unwrap(),
        expected(&task, &f.a, "1")
    );
    f.reopen();
    let s = f.store.as_mut().unwrap();
    let before = s.observation_snapshot().unwrap();
    assert_eq!(
        release(s, &task, &f.a, "1").unwrap(),
        expected(&task, &f.a, "1")
    );
    assert_eq!(s.observation_snapshot().unwrap(), before);
    s.claim(&f.b.credential, &task, 101, 300).unwrap();
    assert!(release(s, &task, &f.a, "1").is_err());
    s.complete(&f.b.credential, &task, "2", "fixture", 102)
        .unwrap();
}

#[test]
fn failed_release_event_rolls_back_the_task_and_retry_records_one_transition() {
    let mut f = Fixture::new(StoreLimits::default());
    let task = uuid::Uuid::new_v4().to_string();
    let db = f.db();
    let s = f.store.as_mut().unwrap();
    s.claim(&f.a.credential, &task, 100, 300).unwrap();
    let snapshot = s.observation_snapshot().unwrap();
    let before = s.inspect_task(&task).unwrap();
    db.execute_batch("CREATE TRIGGER fail_release BEFORE INSERT ON events WHEN NEW.kind='task.operator_released' BEGIN SELECT RAISE(ABORT,'fixture'); END").unwrap();
    assert!(release(s, &task, &f.a, "1").is_err());
    assert_eq!(s.inspect_task(&task).unwrap(), before);
    assert_eq!(s.observation_snapshot().unwrap(), snapshot);
    assert_eq!(
        db.query_row("SELECT value FROM meta WHERE key='clock'", [], |r| r
            .get::<_, String>(0))
            .unwrap(),
        "100"
    );
    db.execute_batch("DROP TRIGGER fail_release").unwrap();
    release(s, &task, &f.a, "1").unwrap();
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM events WHERE kind='task.operator_released'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
    f.reopen();
    assert_eq!(
        release(f.store.as_mut().unwrap(), &task, &f.a, "1").unwrap(),
        expected(&task, &f.a, "1")
    );
}

#[test]
fn release_uses_reserved_settlement_capacity_and_exact_retry_succeeds_at_full_journal() {
    let mut f = Fixture::new(StoreLimits {
        max_events: 32,
        ..StoreLimits::default()
    });
    let task = uuid::Uuid::new_v4().to_string();
    let db = f.db();
    let s = f.store.as_mut().unwrap();
    s.claim(&f.a.credential, &task, 100, 300).unwrap();
    for _ in 0..40 {
        if s.route(&f.a.actor_id, &f.b.actor_id, true).is_err() {
            break;
        }
    }
    for _ in 0..8 {
        s.rotate(&f.a.actor_id).unwrap();
    }
    assert_eq!(
        db.query_row("SELECT count(*) FROM events", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        31
    );
    let receipt = release(s, &task, &f.a, "1").unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM events", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        32
    );
    let before = s.observation_snapshot().unwrap();
    assert_eq!(release(s, &task, &f.a, "1").unwrap(), receipt);
    assert_eq!(s.observation_snapshot().unwrap(), before);
    f.reopen();
    assert_eq!(
        release(f.store.as_mut().unwrap(), &task, &f.a, "1").unwrap(),
        receipt
    );
}
