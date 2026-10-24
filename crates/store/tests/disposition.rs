use hermes_tern_store::{DispositionInput, MessageInput, Registration, Store, StoreLimits};
use rusqlite::Connection;
use serde_json::json;
use std::{fs, os::unix::fs::DirBuilderExt, path::PathBuf};

struct Fixture {
    root: PathBuf,
    state: PathBuf,
    store: Option<Store>,
    a: Registration,
    b: Registration,
    message: MessageInput,
    limits: StoreLimits,
}
impl Fixture {
    fn new(limits: StoreLimits) -> Self {
        let root = PathBuf::from(std::env::var_os("TMPDIR").unwrap())
            .join(format!("ht-disposition-{}", uuid::Uuid::new_v4()));
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        let state = root.join("state");
        let mut s = Store::open(&state, &root, limits.clone()).unwrap();
        let a = s
            .register("hermes", "fixture", "a", "model", "provider")
            .unwrap();
        let b = s
            .register("hermes", "fixture", "b", "model", "provider")
            .unwrap();
        s.route(&a.actor_id, &b.actor_id, true).unwrap();
        let message = MessageInput {
            message_id: uuid::Uuid::new_v4().to_string(),
            recipient: b.actor_id.clone(),
            kind: "context".into(),
            payload: json!({"text":"untrusted original"}),
            ttl_seconds: 300,
            correlation_id: Some(uuid::Uuid::new_v4().to_string()),
        };
        s.send(&a.credential, &message, 100).unwrap();
        s.receive(&b.credential, 101).unwrap();
        drop(s);
        let s = Store::open(&state, &root, limits.clone()).unwrap();
        Self {
            root,
            state,
            store: Some(s),
            a,
            b,
            message,
            limits,
        }
    }
    fn request(&self, reoffer: bool) -> DispositionInput {
        DispositionInput {
            sender: self.a.actor_id.clone(),
            message: self.message.message_id.clone(),
            delivery_incarnation: self.b.incarnation.clone(),
            decision_id: uuid::Uuid::new_v4().to_string(),
            action: if reoffer { "reoffer" } else { "close" }.into(),
            successor_id: reoffer.then(|| uuid::Uuid::new_v4().to_string()),
            ttl_seconds: reoffer.then_some(300),
            reason: "operator accepts unresolved outcome".into(),
            attest_outcome_unknown: true,
        }
    }
    fn db(&self) -> Connection {
        Connection::open(self.state.join("coordination.sqlite3")).unwrap()
    }
    fn reopen(&mut self) {
        drop(self.store.take());
        self.store = Some(Store::open(&self.state, &self.root, self.limits.clone()).unwrap());
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
        fn inventory(path: &std::path::Path, paths: &mut Vec<PathBuf>) {
            for entry in fs::read_dir(path).unwrap() {
                let p = entry.unwrap().path();
                paths.push(p.clone());
                if p.is_dir() {
                    inventory(&p, paths);
                }
            }
        }
        inventory(&self.state, &mut paths);
        fs::write(
            self.root.join("ownership.json"),
            json!({"paths":paths,"retention":"retain; no deletion"}).to_string(),
        )
        .unwrap();
    }
}
fn clock(db: &Connection) -> String {
    db.query_row("SELECT value FROM meta WHERE key='clock'", [], |r| r.get(0))
        .unwrap()
}

#[test]
fn reoffer_preserves_original_and_old_ack_cannot_consume_successor() {
    let mut f = Fixture::new(StoreLimits::default());
    let req = f.request(true);
    let id = req.successor_id.as_ref().unwrap();
    let s = f.store.as_mut().unwrap();
    let old = s.inspect_message(&req.sender, &req.message).unwrap()["message"].clone();
    let receipt = s.dispose_message(&req, 102).unwrap();
    assert_eq!(receipt["action"], "reoffer");
    assert_eq!(receipt["successor_id"], *id);
    let original = s.inspect_message(&req.sender, &req.message).unwrap();
    assert_eq!(original["message"], old);
    assert_eq!(original["disposition"]["decision_id"], req.decision_id);
    assert!(
        s.ack(&f.b.credential, &req.sender, &req.message, 103)
            .is_err()
    );
    let received = s.receive(&f.b.credential, 104).unwrap();
    assert_eq!(received.len(), 1);
    assert_eq!(received[0]["message_id"], *id);
    assert_eq!(received[0]["payload"], f.message.payload);
    assert_eq!(
        received[0]["correlation_id"],
        f.message.correlation_id.clone().unwrap()
    );
    assert_eq!(received[0]["origin"]["source_message_id"], req.message);
    assert_eq!(
        received[0]["origin"]["authorization"],
        "operator_reoffer_not_new_sender_intent"
    );
    assert!(
        s.ack(&f.b.credential, &req.sender, &req.message, 105)
            .is_err()
    );
    assert_eq!(
        s.inspect_message(&req.sender, id).unwrap()["message"]["state"],
        "submitted"
    );
    s.ack(&f.b.credential, &req.sender, id, 106).unwrap();
    let retry = s.send(&f.a.credential, &f.message, 107).unwrap();
    assert_eq!(retry["disposition"]["decision_id"], req.decision_id);
    assert_eq!(retry["state"], "ambiguous");
    f.reopen();
    let s = f.store.as_mut().unwrap();
    assert_eq!(s.dispose_message(&req, 0).unwrap(), receipt);
    assert!(s.receive(&f.b.credential, 108).unwrap().is_empty());
}

#[test]
fn exact_decision_replay_is_inert_and_conflicts_or_stale_tuple_never_mutate() {
    let mut f = Fixture::new(StoreLimits::default());
    let mut req = f.request(false);
    let db = f.db();
    let s = f.store.as_mut().unwrap();
    let before = s.observation_snapshot().unwrap();
    let original = req.clone();
    for case in 0..6 {
        req = original.clone();
        match case {
            0 => req.delivery_incarnation = uuid::Uuid::new_v4().to_string(),
            1 => req.sender = f.b.actor_id.clone(),
            2 => req.attest_outcome_unknown = false,
            3 => req.reason = " ".into(),
            4 => req.successor_id = Some(uuid::Uuid::new_v4().to_string()),
            _ => req.decision_id = "bad".into(),
        }
        assert!(s.dispose_message(&req, 102).is_err());
        assert_eq!(s.observation_snapshot().unwrap(), before);
        assert_eq!(clock(&db), "101");
    }
    req = original;
    let receipt = s.dispose_message(&req, 102).unwrap();
    let before = s.observation_snapshot().unwrap();
    db.execute_batch("CREATE TRIGGER deny_decision BEFORE INSERT ON message_dispositions BEGIN SELECT RAISE(ABORT,'fixture'); END; CREATE TRIGGER deny_event BEFORE INSERT ON events BEGIN SELECT RAISE(ABORT,'fixture'); END; CREATE TRIGGER deny_clock BEFORE UPDATE ON meta BEGIN SELECT RAISE(ABORT,'fixture'); END;").unwrap();
    assert_eq!(s.dispose_message(&req, 0).unwrap(), receipt);
    assert_eq!(s.observation_snapshot().unwrap(), before);
    let mut conflict = req.clone();
    conflict.reason = "changed".into();
    assert!(s.dispose_message(&conflict, 102).is_err());
    conflict = req.clone();
    conflict.decision_id = uuid::Uuid::new_v4().to_string();
    assert!(s.dispose_message(&conflict, 102).is_err());
    assert_eq!(clock(&db), "102");
}

#[test]
fn reoffer_is_route_and_capacity_checked_but_close_can_settle_a_full_journal() {
    let mut f = Fixture::new(StoreLimits {
        max_events: 32,
        max_pending_per_actor: 1,
        ..StoreLimits::default()
    });
    let reoffer = f.request(true);
    let close = f.request(false);
    let db = f.db();
    let s = f.store.as_mut().unwrap();
    s.route(&f.a.actor_id, &f.b.actor_id, false).unwrap();
    let before = s.observation_snapshot().unwrap();
    assert!(s.dispose_message(&reoffer, 102).is_err());
    assert_eq!(s.observation_snapshot().unwrap(), before);
    s.route(&f.a.actor_id, &f.b.actor_id, true).unwrap();
    for _ in 0..40 {
        if s.route(&f.a.actor_id, &f.b.actor_id, true).is_err() {
            break;
        }
    }
    for _ in 0..7 {
        s.rotate(&f.b.actor_id).unwrap();
    }
    assert_eq!(
        db.query_row("SELECT count(*) FROM events", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        31
    );
    assert!(s.dispose_message(&reoffer, 102).is_err());
    let receipt = s.dispose_message(&close, 102).unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM events", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        32
    );
    assert_eq!(s.dispose_message(&close, 0).unwrap(), receipt);
    assert_eq!(
        s.inspect_message(&close.sender, &close.message).unwrap()["message"]["state"],
        "ambiguous"
    );
    f.reopen();
    assert_eq!(
        f.store
            .as_mut()
            .unwrap()
            .dispose_message(&close, 0)
            .unwrap(),
        receipt
    );
}

#[test]
fn event_failure_rolls_back_successor_decision_and_clock_and_id_collision_refuses() {
    let mut f = Fixture::new(StoreLimits::default());
    let mut req = f.request(true);
    let db = f.db();
    let s = f.store.as_mut().unwrap();
    let before = s.observation_snapshot().unwrap();
    db.execute_batch("CREATE TRIGGER deny_reoffer BEFORE INSERT ON events WHEN NEW.kind='message.reoffered' BEGIN SELECT RAISE(ABORT,'fixture'); END;").unwrap();
    assert!(s.dispose_message(&req, 102).is_err());
    assert_eq!(s.observation_snapshot().unwrap(), before);
    assert_eq!(clock(&db), "101");
    assert_eq!(
        db.query_row("SELECT count(*) FROM message_dispositions", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    db.execute_batch("DROP TRIGGER deny_reoffer").unwrap();
    req.successor_id = Some(req.message.clone());
    assert!(s.dispose_message(&req, 102).is_err());
    req.successor_id = Some(uuid::Uuid::new_v4().to_string());
    s.dispose_message(&req, 102).unwrap();
    let archive = s.archive(&uuid::Uuid::new_v4().to_string()).unwrap();
    assert_eq!(archive["counts"]["message_dispositions"], "1");
    assert_eq!(archive["database_schema"], 2);
    f.reopen();
}

#[test]
fn replacement_consumes_storage_budget_and_pending_slot_is_released_by_close() {
    let mut f = Fixture::new(StoreLimits {
        max_messages: 1,
        max_pending_per_actor: 1,
        ..StoreLimits::default()
    });
    let req = f.request(true);
    let close = f.request(false);
    let s = f.store.as_mut().unwrap();
    let before = s.observation_snapshot().unwrap();
    assert!(s.dispose_message(&req, 102).is_err());
    assert_eq!(s.observation_snapshot().unwrap(), before);
    s.dispose_message(&close, 102).unwrap();
    assert!(s.dispose_message(&req, 102).is_err());
    let mut g = Fixture::new(StoreLimits {
        max_pending_per_actor: 1,
        ..StoreLimits::default()
    });
    let req = g.request(true);
    let s = g.store.as_mut().unwrap();
    s.dispose_message(&req, 102).unwrap(); // one old ambiguous + one new queued consumes ONE pending slot
    let input = MessageInput {
        message_id: uuid::Uuid::new_v4().to_string(),
        recipient: g.b.actor_id.clone(),
        kind: "context".into(),
        payload: json!({}),
        ttl_seconds: 300,
        correlation_id: None,
    };
    assert!(s.send(&g.a.credential, &input, 103).is_err());
    s.receive(&g.b.credential, 104).unwrap();
    s.ack(
        &g.b.credential,
        &g.a.actor_id,
        req.successor_id.as_ref().unwrap(),
        105,
    )
    .unwrap();
    s.send(&g.a.credential, &input, 106).unwrap();
}

#[test]
fn forged_disposition_or_successor_provenance_fails_reopen() {
    for mutation in [
        "UPDATE message_dispositions SET delivery_incarnation='00000000-0000-0000-0000-000000000000'",
        "UPDATE message_dispositions SET reason=''",
        "UPDATE message_dispositions SET decided=999999",
        "UPDATE schema_migrations SET database_sha256='not a hash'",
    ] {
        let mut f = Fixture::new(StoreLimits::default());
        let req = f.request(true);
        let s = f.store.as_mut().unwrap();
        s.dispose_message(&req, 102).unwrap();
        drop(f.store.take());
        let db = f.db();
        if mutation.contains("schema_migrations") {
            db.execute(
                "INSERT INTO schema_migrations VALUES(2,?1,?2)",
                rusqlite::params![uuid::Uuid::new_v4().to_string(), "a".repeat(64)],
            )
            .unwrap();
        }

        db.execute_batch(mutation).unwrap();
        drop(db);
        assert!(
            Store::open(&f.state, &f.root, f.limits.clone()).is_err(),
            "accepted corruption: {mutation}"
        );
    }
}

#[test]
fn only_ambiguous_delivery_is_disposable_and_ack_first_wins() {
    let mut f = Fixture::new(StoreLimits::default());
    let req = f.request(true);
    let s = f.store.as_mut().unwrap();
    s.ack(&f.b.credential, &req.sender, &req.message, 102)
        .unwrap();
    let before = s.observation_snapshot().unwrap();
    assert!(s.dispose_message(&req, 103).is_err());
    assert_eq!(s.observation_snapshot().unwrap(), before);
    let queued = MessageInput {
        message_id: uuid::Uuid::new_v4().to_string(),
        recipient: f.b.actor_id.clone(),
        kind: "context".into(),
        payload: json!({}),
        ttl_seconds: 1,
        correlation_id: None,
    };
    s.send(&f.a.credential, &queued, 104).unwrap();
    let mut attempt = req.clone();
    attempt.message = queued.message_id.clone();
    let before = s.observation_snapshot().unwrap();
    assert!(s.dispose_message(&attempt, 104).is_err());
    assert_eq!(s.observation_snapshot().unwrap(), before);
    s.receive(&f.b.credential, 105).unwrap();
    assert_eq!(
        s.inspect_message(&attempt.sender, &attempt.message)
            .unwrap()["message"]["state"],
        "expired"
    );
    assert!(s.dispose_message(&attempt, 105).is_err());
    let submitted = MessageInput {
        message_id: uuid::Uuid::new_v4().to_string(),
        ttl_seconds: 300,
        ..queued
    };
    s.send(&f.a.credential, &submitted, 106).unwrap();
    s.receive(&f.b.credential, 107).unwrap();
    attempt.message = submitted.message_id.clone();
    assert!(s.dispose_message(&attempt, 107).is_err());
    let rotated = s.rotate(&f.b.actor_id).unwrap();
    let mut stale = attempt.clone();
    stale.delivery_incarnation = rotated.incarnation.clone();
    assert!(s.dispose_message(&stale, 108).is_err());
    s.dispose_message(&attempt, 108).unwrap();
    assert_eq!(s.receive(&rotated.credential, 109).unwrap().len(), 1);
}

#[test]
fn successor_expiry_and_payload_budgets_preserve_original_without_redelivery() {
    let mut f = Fixture::new(StoreLimits {
        max_pending_per_actor: 1,
        ..StoreLimits::default()
    });
    let mut req = f.request(true);
    req.ttl_seconds = Some(1);
    let s = f.store.as_mut().unwrap();
    s.dispose_message(&req, 1000).unwrap(); // old envelope expired; unknown delivery did not.
    assert!(s.receive(&f.b.credential, 1001).unwrap().is_empty());
    assert_eq!(
        s.inspect_message(&req.sender, req.successor_id.as_ref().unwrap())
            .unwrap()["message"]["state"],
        "expired"
    );
    assert_eq!(
        s.observation_snapshot().unwrap()["unresolved_message_count"],
        "0"
    );
    let db = f.db();
    let size: i64 = db
        .query_row(
            "SELECT length(CAST(body AS BLOB)) FROM messages WHERE id=?1",
            [&req.message],
            |r| r.get(0),
        )
        .unwrap();
    drop(db);
    let mut limited = Fixture::new(StoreLimits {
        max_total_payload_bytes: size as usize,
        ..StoreLimits::default()
    });
    let attempt = limited.request(true);
    let s = limited.store.as_mut().unwrap();
    let before = s.observation_snapshot().unwrap();
    assert!(s.dispose_message(&attempt, 102).is_err());
    assert_eq!(s.observation_snapshot().unwrap(), before);
}

#[test]
fn successor_link_content_and_sequence_corruption_fail_before_recovery() {
    for mutation in [
        "UPDATE message_dispositions SET ttl_seconds=299",
        "UPDATE message_dispositions SET successor_id=message",
        "UPDATE messages SET created=created+1,expires=expires+1 WHERE sequence=2",
    ] {
        let mut f = Fixture::new(StoreLimits::default());
        let req = f.request(true);
        let s = f.store.as_mut().unwrap();
        s.dispose_message(&req, 102).unwrap();
        s.receive(&f.b.credential, 104).unwrap();
        drop(f.store.take());
        let db = f.db();
        db.execute_batch(mutation).unwrap();
        assert!(
            Store::open(&f.state, &f.root, f.limits.clone()).is_err(),
            "accepted {mutation}"
        );
        assert_eq!(
            db.query_row("SELECT state FROM messages WHERE sequence=2", [], |r| {
                r.get::<_, String>(0)
            })
            .unwrap(),
            "submitted"
        );
    }
}
