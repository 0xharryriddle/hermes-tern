use hermes_tern_store::{MessageInput, Registration, Store, StoreLimits};
use rusqlite::Connection;
use serde_json::{Value, json};
use std::{fs, os::unix::fs::DirBuilderExt, path::PathBuf};

struct Fixture {
    root: PathBuf,
    state: PathBuf,
    store: Option<Store>,
    a: Registration,
    b: Registration,
}
impl Fixture {
    fn new(limits: StoreLimits) -> Self {
        let root = PathBuf::from(std::env::var_os("TMPDIR").unwrap())
            .join(format!("ht-observation-{}", uuid::Uuid::new_v4()));
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        let state = root.join("state");
        let mut s = Store::open(&state, &root, limits).unwrap();
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
fn message(recipient: &str) -> MessageInput {
    MessageInput {
        message_id: uuid::Uuid::new_v4().to_string(),
        recipient: recipient.into(),
        kind: "context".into(),
        payload: json!({"text":"observation fixture"}),
        ttl_seconds: 1,
        correlation_id: None,
    }
}
fn events(s: &mut Store, snapshot: &Value, after: &str, limit: u16) -> Value {
    s.observation_events(
        snapshot["workspace_id"].as_str().unwrap(),
        snapshot["observation_epoch"].as_str().unwrap(),
        after,
        limit,
    )
    .unwrap()
}

#[test]
fn empty_snapshot_and_cursor_zero_are_valid_without_fabricating_events() {
    let mut f = Fixture::new(StoreLimits::default());
    drop(f.store.take());
    let other = f.root.join("empty-state");
    let mut s = Store::open(&other, &f.root, StoreLimits::default()).unwrap();
    let snap = s.observation_snapshot().unwrap();
    assert_eq!(snap["event_watermark"], "0");
    assert_eq!(
        snap["counts"],
        json!({"actors":"0","routes":"0","messages":"0","tasks":"0","events":"0"})
    );
    let page = events(&mut s, &snap, "0", 1);
    assert_eq!(page["status"], "ok");
    assert_eq!(page["events"], json!([]));
    assert_eq!(page["next_after"], "0");
    assert_eq!(page["has_more"], false);
    assert_eq!(s.observation_snapshot().unwrap(), snap);
    drop(s);
    let paths: Vec<_> = fs::read_dir(&other)
        .unwrap()
        .map(|p| p.unwrap().path())
        .collect();
    fs::write(
        other.join("ownership.json"),
        json!({"root":other,"paths":paths,"retention":"retain; no deletion"}).to_string(),
    )
    .unwrap();
}

#[test]
fn snapshot_bounds_each_section_and_retains_exact_totals_at_max_actor_width() {
    let mut f = Fixture::new(StoreLimits {
        max_actors: 64,
        ..StoreLimits::default()
    });
    let s = f.store.as_mut().unwrap();
    let mut ids = vec![f.a.actor_id.clone(), f.b.actor_id.clone()];
    // Escape-heavy but valid model/provider text exercises encoded response growth.
    for i in 2..64 {
        ids.push(
            s.register(
                "hermes",
                "fixture",
                &format!("actor-{i}"),
                &"\"".repeat(512),
                &"\\".repeat(512),
            )
            .unwrap()
            .actor_id,
        );
    }
    for recipient in &ids[2..] {
        s.route(&ids[0], recipient, true).unwrap();
    }
    for recipient in ids.iter().filter(|id| *id != &ids[1]) {
        s.route(&ids[1], recipient, true).unwrap();
    }
    for recipient in &ids[3..7] {
        s.route(&ids[2], recipient, true).unwrap();
    }
    for _ in 0..130 {
        s.claim(&f.a.credential, &uuid::Uuid::new_v4().to_string(), 100, 300)
            .unwrap();
    }
    let snap = s.observation_snapshot().unwrap();
    assert_eq!(snap["actors"]["total"], "64");
    assert_eq!(snap["actors"]["items"].as_array().unwrap().len(), 64);
    assert_eq!(snap["actors"]["truncated"], false);
    for key in ["routes", "tasks"] {
        assert_eq!(snap[key]["total"], "130");
        assert_eq!(snap["counts"][key], "130");
        assert_eq!(snap[key]["items"].as_array().unwrap().len(), 128);
        assert_eq!(snap[key]["truncated"], true);
    }
    assert!(snap.to_string().len() < 393216);
    assert_eq!(s.observation_snapshot().unwrap(), snap);
}

#[test]
fn expiry_publishes_a_notice_once_without_inspection_expiring_work() {
    let mut f = Fixture::new(StoreLimits::default());
    let m = message(&f.b.actor_id);
    let s = f.store.as_mut().unwrap();
    s.send(&f.a.credential, &m, 100).unwrap();
    let before = s.observation_snapshot().unwrap();
    assert_eq!(
        s.inspect_message(&f.a.actor_id, &m.message_id).unwrap()["message"]["state"],
        "queued"
    );
    assert_eq!(
        s.observation_snapshot().unwrap(),
        before,
        "inspection changed state or watermark"
    );
    assert!(s.receive(&f.b.credential, 101).unwrap().is_empty());
    assert_eq!(
        s.inspect_message(&f.a.actor_id, &m.message_id).unwrap()["message"]["state"],
        "expired"
    );
    let page = events(s, &before, before["event_watermark"].as_str().unwrap(), 128);
    assert_eq!(
        page["events"].as_array().unwrap().len(),
        1,
        "expiry mutated state without replay notice"
    );
    assert_eq!(page["events"][0]["kind"], "message.expired");
    assert_eq!(page["events"][0]["entity_id"], m.message_id);
    s.receive(&f.b.credential, 102).unwrap();
    assert!(
        events(s, &before, page["next_after"].as_str().unwrap(), 128)["events"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    drop(f.store.take());
    let mut reopened = Store::open(&f.state, &f.root, StoreLimits::default()).unwrap();
    assert_eq!(
        reopened
            .inspect_message(&f.a.actor_id, &m.message_id)
            .unwrap()["message"]["state"],
        "expired"
    );
}

#[test]
fn send_triggered_expiry_and_failed_admission_are_atomic() {
    let mut f = Fixture::new(StoreLimits {
        max_messages: 1,
        ..StoreLimits::default()
    });
    let m = message(&f.b.actor_id);
    let s = f.store.as_mut().unwrap();
    s.send(&f.a.credential, &m, 100).unwrap();
    let before = s.observation_snapshot().unwrap();
    assert!(
        s.send(&f.a.credential, &message(&f.b.actor_id), 101)
            .is_err()
    );
    assert_eq!(
        s.observation_snapshot().unwrap(),
        before,
        "failed admission committed expiry or event"
    );
    assert_eq!(
        s.inspect_message(&f.a.actor_id, &m.message_id).unwrap()["message"]["state"],
        "queued"
    );
    // The same send-triggered expiry must be observable when admission can commit.
    drop(f.store.take());
    let mut s = Store::open(&f.state, &f.root, StoreLimits::default()).unwrap();
    let before = s.observation_snapshot().unwrap();
    s.send(&f.a.credential, &message(&f.b.actor_id), 101)
        .unwrap();
    let page = events(
        &mut s,
        &before,
        before["event_watermark"].as_str().unwrap(),
        128,
    );
    let kinds: Vec<_> = page["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["kind"].as_str().unwrap())
        .collect();
    assert_eq!(kinds, vec!["message.expired", "message.queued"]);
}

#[test]
fn replay_detects_missing_prefix_middle_or_tail_and_refuses_future_or_foreign_cursors() {
    for removed in [1_i64, 2, 3] {
        let mut f = Fixture::new(StoreLimits::default());
        let before = f.store.as_mut().unwrap().observation_snapshot().unwrap();
        let s = f.store.as_mut().unwrap();
        let positive = events(s, &before, "0", 128);
        assert_eq!(positive["events"].as_array().unwrap().len(), 3);
        assert_eq!(events(s, &before, "4", 128)["reason"], "future_cursor");
        assert!(
            s.observation_events(
                &uuid::Uuid::new_v4().to_string(),
                before["observation_epoch"].as_str().unwrap(),
                "0",
                128
            )
            .is_err()
        );
        let db = f.db();
        assert_eq!(
            db.execute("DELETE FROM events WHERE sequence=?1", [removed])
                .unwrap(),
            1
        );
        let s = f.store.as_mut().unwrap();
        let gap = events(s, &before, "0", 128);
        assert_eq!(gap["status"], "snapshot_required");
        assert_eq!(gap["reason"], "replay_gap");
        assert_eq!(
            gap["event_watermark"], "3",
            "tail loss moved watermark backward"
        );
        assert_eq!(gap["events"], json!([]));
        assert!(gap["next_after"].is_null());
        assert_eq!(
            s.observation_snapshot().unwrap()["event_replay_complete"],
            false
        );
    }
}

#[test]
fn event_pagination_has_exact_successors_without_duplicates_or_mutation() {
    let mut f = Fixture::new(StoreLimits::default());
    let s = f.store.as_mut().unwrap();
    for _ in 0..130 {
        s.rotate(&f.a.actor_id).unwrap();
    }
    let before = s.observation_snapshot().unwrap();
    assert_eq!(before["counts"]["events"], "133");
    let first = events(s, &before, "0", 128);
    assert_eq!(first["events"].as_array().unwrap().len(), 128);
    assert_eq!(first["next_after"], "128");
    assert_eq!(first["has_more"], true);
    let second = events(s, &before, "128", 128);
    assert_eq!(second["events"].as_array().unwrap().len(), 5);
    assert_eq!(second["events"][0]["sequence"], "129");
    assert_eq!(second["next_after"], "133");
    assert_eq!(second["has_more"], false);
    let last = events(s, &before, "133", 1);
    assert_eq!(last["events"], json!([]));
    assert_eq!(last["next_after"], "133");
    assert_eq!(s.observation_snapshot().unwrap(), before);
}

#[test]
fn exact_message_keys_and_large_task_counters_remain_attributed_and_precise() {
    let mut f = Fixture::new(StoreLimits::default());
    let id = uuid::Uuid::new_v4().to_string();
    let s = f.store.as_mut().unwrap();
    s.route(&f.b.actor_id, &f.a.actor_id, true).unwrap();
    let mut first = message(&f.b.actor_id);
    first.message_id = id.clone();
    first.payload = json!({"text":"sender-a"});
    s.send(&f.a.credential, &first, 100).unwrap();
    let mut second = message(&f.a.actor_id);
    second.message_id = id.clone();
    second.payload = json!({"text":"sender-b"});
    s.send(&f.b.credential, &second, 100).unwrap();
    assert_eq!(
        s.inspect_message(&f.a.actor_id, &id).unwrap()["message"]["payload"]["text"],
        "sender-a"
    );
    assert_eq!(
        s.inspect_message(&f.b.actor_id, &id).unwrap()["message"]["payload"]["text"],
        "sender-b"
    );
    assert!(
        s.inspect_message(&uuid::Uuid::new_v4().to_string(), &id)
            .is_err()
    );
    let task = uuid::Uuid::new_v4().to_string();
    s.claim(&f.a.credential, &task, 101, 300).unwrap();
    // Precision control beyond JSON/Luau's exact double integer range.
    let db = f.db();
    db.execute(
        "UPDATE tasks SET generation=9007199254740993 WHERE id=?1",
        [&task],
    )
    .unwrap();
    let s = f.store.as_mut().unwrap();
    assert_eq!(
        s.inspect_task(&task).unwrap()["task"]["generation"],
        "9007199254740993"
    );
    assert_eq!(
        s.observation_snapshot().unwrap()["tasks"]["items"][0]["generation"],
        "9007199254740993"
    );
    let result = "\u{1b}".repeat(32768);
    s.complete(&f.a.credential, &task, "9007199254740993", &result, 102)
        .unwrap();
    let inspection = s.inspect_task(&task).unwrap();
    assert_eq!(inspection["task"]["result"], result);
    assert!(!inspection.to_string().contains('\u{1b}'));
    assert!(inspection.to_string().len() < 393216);
    assert!(s.inspect_task(&uuid::Uuid::new_v4().to_string()).is_err());
}
