use hermes_tern_store::{MessageInput, Registration, Store, StoreLimits};
use rusqlite::{Connection, params};
use serde_json::json;
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
            .join(format!("ht-recovery-{}", uuid::Uuid::new_v4()));
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        let state = root.join("state");
        let mut store = Store::open(&state, &root, limits.clone()).unwrap();
        let a = store.register("hermes", "p", "a", "m", "p").unwrap();
        let b = store.register("hermes", "p", "b", "m", "p").unwrap();
        store.route(&a.actor_id, &b.actor_id, true).unwrap();
        Self {
            root,
            state,
            store: Some(store),
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
        // Exact generated-file inventory; retain private test state, never sweep it.
        let mut paths = vec![self.root.clone(), self.state.clone()];
        paths.extend(
            fs::read_dir(&self.state)
                .unwrap()
                .map(|p| p.unwrap().path()),
        );
        paths.push(self.root.join("ownership.json"));
        fs::write(
            self.root.join("ownership.json"),
            json!({
                "retention":"retain; no deletion", "paths":paths,
            })
            .to_string(),
        )
        .unwrap();
    }
}
fn message(to: &str) -> MessageInput {
    MessageInput {
        message_id: uuid::Uuid::new_v4().to_string(),
        recipient: to.into(),
        kind: "context".into(),
        payload: json!({"text":"recovery fixture, not agent output"}),
        ttl_seconds: 300,
        correlation_id: None,
    }
}

#[test]
fn revoked_route_holds_queued_mail_but_keeps_delivery_acknowledgeable() {
    let mut f = Fixture::new(StoreLimits::default());
    let first = message(&f.b.actor_id);
    let held = message(&f.b.actor_id);
    let s = f.store.as_mut().unwrap();
    s.send(&f.a.credential, &first, 100).unwrap();
    assert_eq!(s.receive(&f.b.credential, 101).unwrap().len(), 1);
    s.send(&f.a.credential, &held, 102).unwrap();
    s.route(&f.a.actor_id, &f.b.actor_id, false).unwrap();
    assert!(
        s.receive(&f.b.credential, 103).unwrap().is_empty(),
        "revoked route delivered queued work"
    );
    // Revocation cannot recall an already-delivered tool result or erase its ACK.
    s.ack(&f.b.credential, &f.a.actor_id, &first.message_id, 104)
        .unwrap();
    assert!(
        s.send(&f.a.credential, &message(&f.b.actor_id), 104)
            .is_err()
    );
    // Sender may observe its previous durable admission without admitting new work.
    assert_eq!(
        s.send(&f.a.credential, &held, 104).unwrap()["duplicate"],
        true
    );
    f.reopen();
    let s = f.store.as_mut().unwrap();
    assert!(s.receive(&f.b.credential, 105).unwrap().is_empty());
    s.route(&f.a.actor_id, &f.b.actor_id, true).unwrap();
    let delivered = s.receive(&f.b.credential, 106).unwrap();
    assert_eq!(delivered.len(), 1);
    assert_eq!(delivered[0]["message_id"], held.message_id);
}

#[test]
fn journal_admission_reserves_capacity_to_settle_accepted_work() {
    let mut f = Fixture::new(StoreLimits {
        max_events: 32,
        ..StoreLimits::default()
    });
    let task = uuid::Uuid::new_v4().to_string();
    let s = f.store.as_mut().unwrap();
    s.claim(&f.b.credential, &task, 100, 300).unwrap();
    let mut accepted = vec![];
    for _ in 0..40 {
        let m = message(&f.b.actor_id);
        if s.send(&f.a.credential, &m, 100).is_err() {
            break;
        }
        accepted.push(m);
    }
    assert!(!accepted.is_empty());
    assert!(
        accepted.len() < 40,
        "journal must reject new work before its bound"
    );
    // Management writes cannot consume capacity already owed to accepted work.
    for _ in 0..40 {
        if s.route(&f.a.actor_id, &f.b.actor_id, true).is_err() {
            break;
        }
    }
    f.reopen();
    let s = f.store.as_mut().unwrap();
    let mut consumed = 0;
    loop {
        let rows = s
            .receive(&f.b.credential, 102)
            .expect("journal stranded accepted delivery");
        if rows.is_empty() {
            break;
        }
        for m in rows {
            s.ack(
                &f.b.credential,
                &f.a.actor_id,
                m["message_id"].as_str().unwrap(),
                102,
            )
            .expect("journal stranded accepted ACK");
            consumed += 1;
        }
    }
    assert_eq!(consumed, accepted.len());
    s.complete(&f.b.credential, &task, "1", "fixture complete", 103)
        .expect("journal stranded task completion");
    assert!(
        s.messages()
            .unwrap()
            .iter()
            .all(|m| m["state"] == "consumed")
    );
    let db = f.db();
    let events: i64 = db
        .query_row("SELECT count(*) FROM events", [], |r| r.get(0))
        .unwrap();
    assert!(events <= 32);
}

#[test]
fn journal_pressure_keeps_revocation_available_without_spending_settlement_slots() {
    let mut f = Fixture::new(StoreLimits {
        max_events: 32,
        ..StoreLimits::default()
    });
    let task = uuid::Uuid::new_v4().to_string();
    let s = f.store.as_mut().unwrap();
    s.route(&f.b.actor_id, &f.a.actor_id, true).unwrap();
    s.claim(&f.b.credential, &task, 100, 300).unwrap();
    let mut admitted = 0;
    for _ in 0..40 {
        if s.send(&f.a.credential, &message(&f.b.actor_id), 100)
            .is_err()
        {
            break;
        }
        admitted += 1;
    }
    assert!(admitted > 0 && admitted < 40);
    // Ordinary administrative writes must not consume the safety reserve either.
    for _ in 0..40 {
        if s.route(&f.a.actor_id, &f.b.actor_id, true).is_err() {
            break;
        }
    }
    f.reopen();
    let s = f.store.as_mut().unwrap();
    s.route(&f.b.actor_id, &f.a.actor_id, false)
        .expect("ordinary traffic prevented operator route revocation");
    let rotated = s
        .rotate(&f.a.actor_id)
        .expect("ordinary traffic prevented operator credential revocation");
    assert!(s.receive(&f.a.credential, 101).is_err());
    assert!(s.receive(&rotated.credential, 101).unwrap().is_empty());
    // Consume the rest of the documented eight-slot safety allowance for a
    // 32-event fixture, then prove saturation is explicit and settlement remains.
    let mut last = rotated;
    for _ in 0..6 {
        last = s.rotate(&f.a.actor_id).unwrap();
    }
    let mut publisher_called = false;
    assert!(
        s.rotate_with_publisher(&f.a.actor_id, |_| {
            publisher_called = true;
            Ok(())
        })
        .is_err()
    );
    assert!(!publisher_called, "refused rotation published a candidate");
    assert!(s.receive(&last.credential, 101).unwrap().is_empty());
    // Reverse-route revocation must not harm delivery/ACK of accepted forward mail.
    let mut consumed = 0;
    loop {
        let messages = s.receive(&f.b.credential, 102).unwrap();
        if messages.is_empty() {
            break;
        }
        for m in messages {
            s.ack(
                &f.b.credential,
                &f.a.actor_id,
                m["message_id"].as_str().unwrap(),
                102,
            )
            .unwrap();
            consumed += 1;
        }
    }
    assert_eq!(consumed, admitted);
    s.complete(&f.b.credential, &task, "1", "fixture complete", 103)
        .unwrap();
    f.reopen();
    assert!(
        f.store
            .as_mut()
            .unwrap()
            .receive(&f.a.credential, 104)
            .is_err()
    );
    let db = f.db();
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM routes WHERE sender=?1 AND recipient=?2",
            params![f.b.actor_id, f.a.actor_id],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    assert!(
        db.query_row("SELECT count(*) FROM events", [], |r| r.get::<_, i64>(0))
            .unwrap()
            <= 32
    );
}

#[test]
fn prior_journal_without_safety_allowance_can_still_settle_accepted_work() {
    let mut f = Fixture::new(StoreLimits {
        max_events: 32,
        ..StoreLimits::default()
    });
    let m = message(&f.b.actor_id);
    let s = f.store.as_mut().unwrap();
    s.send(&f.a.credential, &m, 100).unwrap();
    drop(f.store.take());
    let db = f.db();
    // A valid journal admitted by the prior writer: 30 events plus the two
    // delivery/ACK slots fit, but no additional safety reserve was promised.
    let count: i64 = db
        .query_row("SELECT count(*) FROM events", [], |r| r.get(0))
        .unwrap();
    for _ in count..30 {
        db.execute(
            "INSERT INTO events(kind,entity) VALUES('route.allowed',?1)",
            [&f.b.actor_id],
        )
        .unwrap();
    }
    drop(db);
    f.reopen();
    let s = f.store.as_mut().unwrap();
    assert!(
        s.send(&f.a.credential, &message(&f.b.actor_id), 101)
            .is_err()
    );
    assert_eq!(
        s.receive(&f.b.credential, 101)
            .expect("new reserve blocked old accepted delivery")
            .len(),
        1
    );
    s.ack(&f.b.credential, &f.a.actor_id, &m.message_id, 102)
        .unwrap();
    assert_eq!(s.messages().unwrap()[0]["state"], "consumed");
    assert_eq!(
        f.db()
            .query_row("SELECT count(*) FROM events", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        32
    );
}

#[test]
fn reopen_rejects_semantically_corrupt_rows_before_recovery_mutates_them() {
    // These all leave valid SQLite pages: quick_check alone cannot establish them.
    let mutations = [
        (
            "payload digest",
            "UPDATE messages SET body_hash=printf('%064d',0)",
        ),
        (
            "payload recipient",
            "UPDATE messages SET body=json_set(body,'$.recipient',sender)",
        ),
        (
            "duplicate JSON field",
            "UPDATE messages SET body=replace(body,'\"kind\":\"context\"','\"kind\":\"context\",\"kind\":\"context\"')",
        ),
        (
            "delivery incarnation",
            "UPDATE messages SET delivery_incarnation=NULL",
        ),
        (
            "noncanonical clock",
            "UPDATE meta SET value='00100' WHERE key='clock'",
        ),
        (
            "negative clock",
            "UPDATE meta SET value='-1' WHERE key='clock'",
        ),
        (
            "foreign key",
            "UPDATE messages SET sender='00000000-0000-0000-0000-000000000000'",
        ),
        (
            "invalid token digest",
            "UPDATE actors SET token_hash='invalid' WHERE session='a'",
        ),
        (
            "invalid actor incarnation",
            "UPDATE actors SET incarnation='not-a-uuid' WHERE session='b'",
        ),
        (
            "task result lifecycle",
            "UPDATE tasks SET result='not complete yet'",
        ),
        ("negative task deadline", "UPDATE tasks SET deadline=-1"),
        (
            "unknown event",
            "UPDATE events SET kind='invented' WHERE sequence=1",
        ),
        (
            "missing workspace metadata",
            "DELETE FROM meta WHERE key='workspace_id'",
        ),
        (
            "extra metadata",
            "INSERT INTO meta VALUES('unexpected','value')",
        ),
        (
            "oversized message body",
            "UPDATE messages SET body=printf('%040000d',0)",
        ),
        ("wrong envelope kind", "UPDATE messages SET kind='question'"),
        ("wrong expiry", "UPDATE messages SET expires=expires+1"),
        (
            "self route",
            "INSERT INTO routes SELECT id,id FROM actors WHERE session='a'",
        ),
        ("null route", "INSERT INTO routes VALUES(NULL,NULL)"),
        ("invalid task generation", "UPDATE tasks SET generation=1.5"),
        (
            "null actor identity",
            "INSERT INTO actors SELECT NULL,incarnation,printf('%064d',1),kind,profile,'c',model,provider FROM actors WHERE session='a'",
        ),
    ];
    let mut incorrectly_opened = vec![];
    for (label, mutation) in mutations {
        let mut f = Fixture::new(StoreLimits::default());
        let m = message(&f.b.actor_id);
        let s = f.store.as_mut().unwrap();
        s.send(&f.a.credential, &m, 100).unwrap();
        s.claim(&f.a.credential, &uuid::Uuid::new_v4().to_string(), 100, 300)
            .unwrap();
        f.reopen(); // Real intact-store positive control.
        f.store
            .as_mut()
            .unwrap()
            .receive(&f.b.credential, 100)
            .unwrap();
        drop(f.store.take());
        let db = f.db();
        db.pragma_update(None, "foreign_keys", false).unwrap();
        assert!(
            db.execute(mutation, []).unwrap() > 0,
            "mutation did not run: {label}"
        );
        assert_eq!(
            db.query_row("PRAGMA quick_check", [], |r| r.get::<_, String>(0))
                .unwrap(),
            "ok",
            "fixture must reach semantic rather than page corruption: {label}"
        );
        let opened = Store::open(&f.state, &f.root, f.limits.clone());
        if opened.is_ok() {
            incorrectly_opened.push(label);
        }
        drop(opened);
        let state: String = db
            .query_row("SELECT state FROM messages", [], |r| r.get(0))
            .unwrap();
        if state != "submitted" {
            incorrectly_opened.push(label);
        }
    }
    assert!(
        incorrectly_opened.is_empty(),
        "startup accepted or mutated corrupt rows: {incorrectly_opened:?}"
    );
}

#[test]
fn restart_rejects_clock_rollback_and_rotated_delivery_ack_without_losing_mail() {
    let mut f = Fixture::new(StoreLimits::default());
    let m = message(&f.b.actor_id);
    let s = f.store.as_mut().unwrap();
    s.send(&f.a.credential, &m, 100).unwrap();
    s.receive(&f.b.credential, 101).unwrap();
    let rotated = s.rotate(&f.b.actor_id).unwrap();
    f.reopen();
    let s = f.store.as_mut().unwrap();
    assert!(s.receive(&rotated.credential, 100).is_err());
    assert!(
        s.ack(&rotated.credential, &f.a.actor_id, &m.message_id, 102)
            .is_err()
    );
    assert!(
        s.ack(&f.b.credential, &f.a.actor_id, &m.message_id, 102)
            .is_err()
    );
    assert_eq!(s.messages().unwrap()[0]["state"], "ambiguous");
    assert!(s.receive(&rotated.credential, 102).unwrap().is_empty());
    let db = f.db();
    let count: i64 = db
        .query_row(
            "SELECT count(*) FROM messages WHERE id=?1",
            params![m.message_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);
}
