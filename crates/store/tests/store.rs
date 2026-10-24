use hermes_tern_store::{MessageInput, Store, StoreLimits};
use serde_json::json;
use std::{fs, path::PathBuf};
fn temp() -> PathBuf {
    let p = PathBuf::from(std::env::var_os("TMPDIR").unwrap())
        .join(format!("ht-store-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&p).unwrap();
    p
}
fn message(to: &str, id: &str, text: &str) -> MessageInput {
    MessageInput {
        message_id: id.into(),
        recipient: to.into(),
        kind: "context".into(),
        payload: json!({"text":text}),
        ttl_seconds: 300,
        correlation_id: None,
    }
}
#[test]
fn admission_is_routed_authenticated_deduplicated_and_durable() {
    let root = temp();
    let state = root.join("state");
    let mut store = Store::open(&state, &root, StoreLimits::default()).unwrap();
    let a = store
        .register("hermes", "profile", "session-a", "model", "provider")
        .unwrap();
    let b = store
        .register("hermes", "profile", "session-b", "model", "provider")
        .unwrap();
    let aid = a.actor_id.clone();
    let bid = b.actor_id.clone();
    let mid = uuid::Uuid::new_v4().to_string();
    let input = message(&bid, &mid, "untrusted context");
    assert!(store.send(&a.credential, &input, 100).is_err());
    store.route(&aid, &bid, true).unwrap();
    let first = store.send(&a.credential, &input, 100).unwrap();
    assert_eq!(first["state"], "queued");
    assert_eq!(
        store.send(&a.credential, &input, 101).unwrap()["sequence"],
        first["sequence"]
    );
    assert!(
        store
            .send(&a.credential, &message(&bid, &mid, "different"), 101)
            .is_err()
    );
    assert!(store.send("forged-token", &input, 100).is_err());
    drop(store);
    let mut store = Store::open(&state, &root, StoreLimits::default()).unwrap();
    let read = store.receive(&b.credential, 102).unwrap();
    assert_eq!(read.len(), 1);
    assert_eq!(read[0]["sender_actor_id"], aid);
    assert_eq!(read[0]["payload"]["text"], "untrusted context");
    assert!(store.ack(&a.credential, &aid, &mid, 103).is_err());
    assert_eq!(
        store.ack(&b.credential, &aid, &mid, 103).unwrap()["state"],
        "consumed"
    );
    assert_eq!(
        store.ack(&b.credential, &aid, &mid, 103).unwrap()["state"],
        "consumed"
    );
}
#[test]
fn restart_keeps_unacknowledged_delivery_ambiguous() {
    let root = temp();
    let state = root.join("state");
    let mut s = Store::open(&state, &root, StoreLimits::default()).unwrap();
    let a = s.register("hermes", "p", "a", "m", "p").unwrap();
    let b = s.register("hermes", "p", "b", "m", "p").unwrap();
    s.route(&a.actor_id, &b.actor_id, true).unwrap();
    let mid = uuid::Uuid::new_v4().to_string();
    s.send(&a.credential, &message(&b.actor_id, &mid, "x"), 100)
        .unwrap();
    assert_eq!(s.receive(&b.credential, 101).unwrap().len(), 1);
    drop(s);
    let mut s = Store::open(&state, &root, StoreLimits::default()).unwrap();
    assert!(s.receive(&b.credential, 102).unwrap().is_empty());
    assert_eq!(s.messages().unwrap()[0]["state"], "ambiguous");
    assert_eq!(
        s.ack(&b.credential, &a.actor_id, &mid, 103).unwrap()["state"],
        "consumed"
    );
}
#[test]
fn private_store_lock_limits_expiry_and_revocation_fail_closed() {
    let root = temp();
    let state = root.join("state");
    let limits = StoreLimits {
        max_pending_per_actor: 1,
        ..StoreLimits::default()
    };
    let mut s = Store::open(&state, &root, limits.clone()).unwrap();
    assert!(Store::open(&state, &root, limits).is_err());
    let a = s.register("hermes", "p", "a", "m", "p").unwrap();
    let b = s.register("hermes", "p", "b", "m", "p").unwrap();
    s.route(&a.actor_id, &b.actor_id, true).unwrap();
    let mut input = message(&b.actor_id, &uuid::Uuid::new_v4().to_string(), "x");
    input.ttl_seconds = 1;
    s.send(&a.credential, &input, 100).unwrap();
    assert!(
        s.send(
            &a.credential,
            &message(&b.actor_id, &uuid::Uuid::new_v4().to_string(), "y"),
            100
        )
        .is_err()
    );
    assert!(s.receive(&b.credential, 101).unwrap().is_empty());
    s.send(
        &a.credential,
        &message(&b.actor_id, &uuid::Uuid::new_v4().to_string(), "y"),
        102,
    )
    .unwrap();
    let rotated = s.rotate(&a.actor_id).unwrap();
    assert!(
        s.send(
            &a.credential,
            &message(&b.actor_id, &uuid::Uuid::new_v4().to_string(), "z"),
            103
        )
        .is_err()
    );
    assert_ne!(a.incarnation, rotated.incarnation);
    assert!(!format!("{rotated:?}").contains(&rotated.credential));
}
#[test]
fn task_lease_completion_is_fenced_and_expiry_does_not_reassign_writer() {
    let root = temp();
    let mut s = Store::open(&root.join("state"), &root, StoreLimits::default()).unwrap();
    let a = s.register("hermes", "p", "a", "m", "p").unwrap();
    let b = s.register("hermes", "p", "b", "m", "p").unwrap();
    let task = uuid::Uuid::new_v4().to_string();
    let claim = s.claim(&a.credential, &task, 100, 30).unwrap();
    let generation = claim["generation"].as_str().unwrap();
    assert!(
        s.complete(&b.credential, &task, generation, "forged", 101)
            .is_err()
    );
    assert!(s.complete(&a.credential, &task, "2", "stale", 101).is_err());
    assert!(s.claim(&b.credential, &task, 200, 30).is_err());
    assert!(
        s.complete(&a.credential, &task, generation, "late", 200)
            .is_err()
    );
    s.release_stopped(&task, &a.actor_id, &a.incarnation, generation, true)
        .unwrap();
    let claim = s.claim(&b.credential, &task, 201, 30).unwrap();
    assert_eq!(claim["generation"], "2");
    assert_eq!(
        s.complete(&b.credential, &task, "2", "bounded result", 202)
            .unwrap()["state"],
        "complete"
    );
}
