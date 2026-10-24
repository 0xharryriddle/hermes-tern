use hermes_tern_store::{DispositionInput, MessageInput, Registration, Store, StoreLimits};
use rusqlite::Connection;
use serde_json::{Value, json};
use std::{collections::BTreeSet, fs, os::unix::fs::DirBuilderExt, path::PathBuf};

struct Fixture {
    root: PathBuf,
    state: PathBuf,
    store: Option<Store>,
    a: Registration,
    b: Registration,
}
impl Fixture {
    fn new() -> Self {
        let root = PathBuf::from(std::env::var_os("TMPDIR").unwrap())
            .join(format!("ht-projection-{}", uuid::Uuid::new_v4()));
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        let state = root.join("state");
        let mut s = Store::open(
            &state,
            &root,
            StoreLimits {
                max_actors: 64,
                ..StoreLimits::default()
            },
        )
        .unwrap();
        let a = s
            .register("hermes", "fixture", "a", "model", "provider")
            .unwrap();
        let b = s
            .register("hermes", "fixture", "b", "model", "provider")
            .unwrap();
        s.route(&a.actor_id, &b.actor_id, true).unwrap();
        Self {
            root,
            state,
            store: Some(s),
            a,
            b,
        }
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
                .map(|e| e.unwrap().path()),
        );
        fs::write(
            self.root.join("ownership.json"),
            json!({"paths":paths,"retention":"retain; no deletion"}).to_string(),
        )
        .unwrap();
    }
}
fn id() -> String {
    uuid::Uuid::new_v4().to_string()
}
fn input(recipient: &str) -> MessageInput {
    MessageInput {
        message_id: id(),
        recipient: recipient.into(),
        kind: "context".into(),
        payload: json!({"text":"untrusted projection fixture"}),
        ttl_seconds: 300,
        correlation_id: None,
    }
}
fn page(s: &mut Store, index: &Value, section: &str, offset: &str, limit: u16) -> Value {
    s.state_page(
        index["workspace_id"].as_str().unwrap(),
        index["observation_epoch"].as_str().unwrap(),
        index["revision"].as_str().unwrap(),
        section,
        offset,
        limit,
    )
    .unwrap()
}
fn walk(s: &mut Store, index: &Value, section: &str, limit: u16) -> Vec<Value> {
    let mut out = vec![];
    let mut offset = "0".to_owned();
    for _ in 0..100 {
        let p = page(s, index, section, &offset, limit);
        assert_eq!(p["status"], "ok");
        assert_eq!(p["total"], index["counts"][section]);
        assert_eq!(p["revision"], index["revision"]);
        assert!(p.to_string().len() <= 393216);
        let rows = p["items"].as_array().unwrap();
        out.extend(rows.clone());
        if p["has_more"] == false {
            assert_eq!(p["next_offset"], out.len().to_string());
            assert_eq!(out.len().to_string(), index["counts"][section]);
            return out;
        }
        assert!(!rows.is_empty(), "pagination did not advance");
        offset = p["next_offset"].as_str().unwrap().to_owned();
    }
    panic!("pagination did not terminate");
}

#[test]
fn complete_state_pages_reach_every_retained_record_beyond_overview_limits() {
    let mut f = Fixture::new();
    let s = f.store.as_mut().unwrap();
    let mut actors = vec![f.a.actor_id.clone(), f.b.actor_id.clone()];
    for n in 2..13 {
        actors.push(
            s.register("hermes", "fixture", &format!("peer-{n}"), "m", "p")
                .unwrap()
                .actor_id,
        );
    }
    let mut routes = BTreeSet::from([(f.a.actor_id.clone(), f.b.actor_id.clone())]);
    'routes: for sender in &actors {
        for recipient in &actors {
            if sender != recipient && routes.insert((sender.clone(), recipient.clone())) {
                s.route(sender, recipient, true).unwrap();
            }
            if routes.len() == 130 {
                break 'routes;
            }
        }
    }
    let mut message_ids = BTreeSet::new();
    let mut task_ids = BTreeSet::new();
    for _ in 0..130 {
        let m = input(&f.b.actor_id);
        message_ids.insert(m.message_id.clone());
        s.send(&f.a.credential, &m, 100).unwrap();
        s.receive(&f.b.credential, 100).unwrap();
        s.ack(&f.b.credential, &f.a.actor_id, &m.message_id, 100)
            .unwrap();
        let task = id();
        s.claim(&f.a.credential, &task, 100, 300).unwrap();
        task_ids.insert(task);
    }
    let overview = s.observation_snapshot().unwrap();
    assert_eq!(overview["messages"]["truncated"], true);
    let index = s.state_index().unwrap();
    assert_eq!(index["counts"]["messages"], "130");
    assert_eq!(index["counts"]["tasks"], "130");
    assert_eq!(index["counts"]["routes"], "130");
    let messages = walk(s, &index, "messages", 37);
    let actual: BTreeSet<_> = messages
        .iter()
        .map(|v| v["message"]["message_id"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(actual, message_ids);
    let tasks = walk(s, &index, "tasks", 128);
    assert_eq!(
        tasks
            .iter()
            .map(|v| v["task"]["task_id"].as_str().unwrap().to_owned())
            .collect::<BTreeSet<_>>(),
        task_ids
    );
    let got_routes = walk(s, &index, "routes", 128);
    assert_eq!(
        got_routes
            .iter()
            .map(|v| (
                v["sender_actor_id"].as_str().unwrap().to_owned(),
                v["recipient_actor_id"].as_str().unwrap().to_owned()
            ))
            .collect::<BTreeSet<_>>(),
        routes
    );
    assert_eq!(walk(s, &index, "actors", 1).len(), 13);
    for section in ["dispositions", "migrations"] {
        assert!(walk(s, &index, section, 128).is_empty());
    }
    let events = walk(s, &index, "events", 128);
    for (n, e) in events.iter().enumerate() {
        assert_eq!(e["sequence"], (n + 1).to_string());
    }
    assert_eq!(s.state_index().unwrap(), index);
    assert_eq!(s.observation_snapshot().unwrap(), overview);
    let body = json!([index, messages, tasks, got_routes, events]).to_string();
    assert!(!body.contains(&f.a.credential));
    assert!(!body.contains("token_hash"));
}

#[test]
fn pages_preserve_sender_scoped_payloads_disposition_and_exact_result_projection() {
    let mut f = Fixture::new();
    let s = f.store.as_mut().unwrap();
    let a = input(&f.b.actor_id);
    let mut b = input(&f.a.actor_id);
    b.message_id = a.message_id.clone();
    b.payload = json!({"text":"other sender"});
    s.route(&f.b.actor_id, &f.a.actor_id, true).unwrap();
    s.send(&f.a.credential, &a, 100).unwrap();
    s.send(&f.b.credential, &b, 100).unwrap();
    s.receive(&f.b.credential, 100).unwrap();
    let rotated = s.rotate(&f.b.actor_id).unwrap();
    let decision = DispositionInput {
        sender: f.a.actor_id.clone(),
        message: a.message_id.clone(),
        delivery_incarnation: f.b.incarnation.clone(),
        decision_id: id(),
        action: "reoffer".into(),
        successor_id: Some(id()),
        ttl_seconds: Some(300),
        reason: "unknown fixture outcome".into(),
        attest_outcome_unknown: true,
    };
    s.dispose_message(&decision, 101).unwrap();
    let task = id();
    s.claim(&rotated.credential, &task, 101, 300).unwrap();
    s.complete(&rotated.credential, &task, "1", "untrusted result", 102)
        .unwrap();
    let index = s.state_index().unwrap();
    let messages = walk(s, &index, "messages", 128);
    assert_eq!(messages.len(), 3);
    for item in &messages {
        assert_eq!(
            *item,
            s.inspect_message(
                item["message"]["sender_actor_id"].as_str().unwrap(),
                item["message"]["message_id"].as_str().unwrap()
            )
            .unwrap()
        );
    }
    let successor = messages
        .iter()
        .find(|v| v["message"]["message_id"] == decision.successor_id.as_ref().unwrap().as_str())
        .unwrap();
    assert_eq!(
        successor["origin"]["authorization"],
        "operator_reoffer_not_new_sender_intent"
    );
    let decisions = walk(s, &index, "dispositions", 128);
    assert_eq!(decisions.len(), 1);
    assert_eq!(decisions[0]["decision_id"], decision.decision_id);
    assert_eq!(
        walk(s, &index, "tasks", 128),
        vec![s.inspect_task(&task).unwrap()]
    );
    assert_eq!(s.state_index().unwrap(), index);
}

#[test]
fn stale_revision_clock_only_change_and_restart_refuse_partial_state_assembly() {
    let mut f = Fixture::new();
    let s = f.store.as_mut().unwrap();
    let index = s.state_index().unwrap();
    assert_eq!(page(s, &index, "actors", "0", 1)["status"], "ok");
    s.route(&f.a.actor_id, &f.b.actor_id, false).unwrap();
    let stale = page(s, &index, "actors", "1", 1);
    assert_eq!(stale["status"], "restart_required");
    assert_eq!(stale["reason"], "state_changed");
    assert_eq!(stale["items"], json!([]));
    assert!(stale["next_offset"].is_null());
    let fresh = s.state_index().unwrap();
    let watermark = fresh["event_watermark"].clone();
    s.receive(&f.a.credential, 500).unwrap(); // Empty receive commits clock only, without an event.
    let now = s.state_index().unwrap();
    assert_eq!(now["event_watermark"], watermark);
    assert_ne!(now["revision"], fresh["revision"]);
    assert_eq!(page(s, &fresh, "actors", "1", 1)["reason"], "state_changed");
    drop(f.store.take());
    let mut reopened = Store::open(
        &f.state,
        &f.root,
        StoreLimits {
            max_actors: 64,
            ..StoreLimits::default()
        },
    )
    .unwrap();
    assert_eq!(
        page(&mut reopened, &now, "actors", "0", 1)["reason"],
        "epoch_changed"
    );
    assert_eq!(reopened.state_index().unwrap()["revision"], now["revision"]);
}

#[test]
fn encoded_byte_budget_never_truncates_a_record_or_reports_false_completion() {
    let mut f = Fixture::new();
    let s = f.store.as_mut().unwrap();
    let result = "\u{1b}".repeat(32768);
    for _ in 0..3 {
        let task = id();
        s.claim(&f.a.credential, &task, 100, 300).unwrap();
        s.complete(&f.a.credential, &task, "1", &result, 100)
            .unwrap();
    }
    let index = s.state_index().unwrap();
    let p = page(s, &index, "tasks", "0", 128);
    assert_eq!(p["items"].as_array().unwrap().len(), 1);
    assert_eq!(p["byte_limited"], true);
    assert_eq!(p["has_more"], true);
    assert_eq!(p["next_offset"], "1");
    let all = walk(s, &index, "tasks", 128);
    assert_eq!(all.len(), 3);
    for item in all {
        assert_eq!(item["task"]["result"], result);
    }
}

#[test]
fn malformed_foreign_offsets_and_end_pages_fail_closed_without_mutation() {
    let mut f = Fixture::new();
    let s = f.store.as_mut().unwrap();
    let index = s.state_index().unwrap();
    let workspace = index["workspace_id"].as_str().unwrap();
    let epoch = index["observation_epoch"].as_str().unwrap();
    let rev = index["revision"].as_str().unwrap();
    for bad in ["-1", "01", "+0", " 0", "9223372036854775808"] {
        assert!(
            s.state_page(workspace, epoch, rev, "actors", bad, 1)
                .is_err()
        );
    }
    for bad in [0, 129] {
        assert!(
            s.state_page(workspace, epoch, rev, "actors", "0", bad)
                .is_err()
        );
    }
    for bad in ["meta", "actors;DELETE FROM actors", "", "credentials"] {
        assert!(s.state_page(workspace, epoch, rev, bad, "0", 1).is_err());
    }
    assert!(s.state_page(&id(), epoch, rev, "actors", "0", 1).is_err());
    for bad in ["", "A", "not-a-digest"] {
        assert!(
            s.state_page(workspace, epoch, bad, "actors", "0", 1)
                .is_err()
        );
    }
    assert!(
        s.state_page(workspace, epoch, rev, "actors", "3", 1)
            .is_err()
    );
    let end = page(s, &index, "actors", "2", 128);
    assert_eq!(end["items"], json!([]));
    assert_eq!(end["next_offset"], "2");
    assert_eq!(end["has_more"], false);
    assert_eq!(s.state_index().unwrap(), index);
}

#[test]
fn revision_covers_allocation_counters_and_precise_task_generations() {
    let mut f = Fixture::new();
    let s = f.store.as_mut().unwrap();
    let task = id();
    s.claim(&f.a.credential, &task, 100, 300).unwrap();
    let before = s.state_index().unwrap();
    let db = Connection::open(f.state.join("coordination.sqlite3")).unwrap();
    db.execute(
        "UPDATE tasks SET generation=9007199254740993 WHERE id=?1",
        [&task],
    )
    .unwrap();
    assert_eq!(page(s, &before, "tasks", "0", 1)["reason"], "state_changed");
    let index = s.state_index().unwrap();
    assert_eq!(
        page(s, &index, "tasks", "0", 1)["items"][0]["task"]["generation"],
        "9007199254740993"
    );
    db.execute(
        "UPDATE sqlite_sequence SET seq=9007199254740993 WHERE name='events'",
        [],
    )
    .unwrap();
    let next = s.state_index().unwrap();
    assert_eq!(next["event_watermark"], "9007199254740993");
    assert_ne!(next["revision"], index["revision"]);
    assert_eq!(next["event_replay_complete"], false);
    assert_eq!(page(s, &index, "tasks", "0", 1)["reason"], "state_changed");
}

#[test]
fn empty_store_pages_and_nonempty_reads_never_touch_any_logical_row() {
    let mut f = Fixture::new();
    let db = Connection::open(f.state.join("coordination.sqlite3")).unwrap();
    for table in [
        "actors",
        "routes",
        "messages",
        "tasks",
        "events",
        "meta",
        "message_dispositions",
        "schema_migrations",
    ] {
        for op in ["INSERT", "UPDATE", "DELETE"] {
            db.execute_batch(&format!("CREATE TRIGGER refuse_{table}_{op} BEFORE {op} ON {table} BEGIN SELECT RAISE(ABORT,'read must not write'); END;")).unwrap();
        }
    }
    let s = f.store.as_mut().unwrap();
    let index = s.state_index().unwrap();
    for section in [
        "actors",
        "routes",
        "messages",
        "tasks",
        "events",
        "dispositions",
        "migrations",
    ] {
        walk(s, &index, section, 1);
    }
    assert_eq!(s.state_index().unwrap(), index);
    drop(db);
    drop(f.store.take());
    let empty = f.root.join("empty-state");
    let mut s = Store::open(&empty, &f.root, StoreLimits::default()).unwrap();
    let index = s.state_index().unwrap();
    assert_eq!(index["clock_watermark"], "0");
    assert_eq!(index["event_watermark"], "0");
    assert_eq!(index["message_watermark"], "0");
    assert_eq!(index["event_replay_complete"], true);
    for section in [
        "actors",
        "routes",
        "messages",
        "tasks",
        "events",
        "dispositions",
        "migrations",
    ] {
        assert!(walk(&mut s, &index, section, 128).is_empty());
    }
    drop(s);
    let paths: Vec<_> = fs::read_dir(&empty)
        .unwrap()
        .map(|p| p.unwrap().path())
        .collect();
    fs::write(
        empty.join("ownership.json"),
        json!({"root":empty,"paths":paths,"retention":"retain; no deletion"}).to_string(),
    )
    .unwrap();
}

#[test]
fn invalid_counters_extra_table_and_overlimit_state_refuse_without_success_envelopes() {
    for mutation in [
        "UPDATE sqlite_sequence SET seq=0 WHERE name='events'",
        "INSERT INTO sqlite_sequence(name,seq) VALUES('events',3)",
        "INSERT INTO sqlite_sequence(name,seq) VALUES('unknown',0)",
        "UPDATE sqlite_sequence SET seq='bad' WHERE name='events'",
        "UPDATE sqlite_sequence SET seq=NULL WHERE name='events'",
        "CREATE TABLE z_unexpected(id TEXT)",
        "UPDATE actors SET model=printf('%0513d',0)",
    ] {
        let mut f = Fixture::new();
        let s = f.store.as_mut().unwrap();
        let index = s.state_index().unwrap();
        let db = Connection::open(f.state.join("coordination.sqlite3")).unwrap();
        db.execute_batch(mutation).unwrap();
        assert!(s.state_index().is_err(), "accepted {mutation}");
        assert!(
            s.state_page(
                index["workspace_id"].as_str().unwrap(),
                index["observation_epoch"].as_str().unwrap(),
                index["revision"].as_str().unwrap(),
                "actors",
                "0",
                1
            )
            .is_err(),
            "accepted page: {mutation}"
        );
    }
}

#[test]
fn same_count_no_event_row_mutations_and_missing_event_tail_change_revision() {
    let mut f = Fixture::new();
    let s = f.store.as_mut().unwrap();
    let db = Connection::open(f.state.join("coordination.sqlite3")).unwrap();
    let mut index = s.state_index().unwrap();
    for mutation in [
        "UPDATE actors SET model='changed'",
        "UPDATE events SET kind='actor.rotated' WHERE sequence=1",
    ] {
        db.execute_batch(mutation).unwrap();
        assert_eq!(page(s, &index, "actors", "0", 1)["reason"], "state_changed");
        let current = s.state_index().unwrap();
        assert_eq!(current["counts"], index["counts"]);
        assert_eq!(current["event_watermark"], index["event_watermark"]);
        assert_ne!(current["revision"], index["revision"]);
        index = current;
    }
    db.execute("DELETE FROM events WHERE sequence=3", [])
        .unwrap();
    let current = s.state_index().unwrap();
    assert_eq!(current["event_watermark"], "3");
    assert_eq!(current["event_replay_complete"], false);
    assert_ne!(current["revision"], index["revision"]);
    assert_eq!(walk(s, &current, "events", 128).len(), 2);
}

#[test]
fn invalid_persisted_rows_refuse_projection_instead_of_partial_success() {
    let mut f = Fixture::new();
    let s = f.store.as_mut().unwrap();
    let m = input(&f.b.actor_id);
    s.send(&f.a.credential, &m, 100).unwrap();
    let index = s.state_index().unwrap();
    let db = Connection::open(f.state.join("coordination.sqlite3")).unwrap();
    db.execute("UPDATE messages SET body='{}'", []).unwrap();
    assert!(s.state_index().is_err());
    assert!(
        s.state_page(
            index["workspace_id"].as_str().unwrap(),
            index["observation_epoch"].as_str().unwrap(),
            index["revision"].as_str().unwrap(),
            "messages",
            "0",
            128
        )
        .is_err()
    );
    assert_eq!(
        db.query_row("SELECT body FROM messages", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "{}"
    );
}
