//! Unknown optional frames retain sequence accounting, never execution authority.
use hermes_tern_gateway::{
    protocol::{Inbound, parse_frame},
    session::{Action, SessionState},
};
use serde_json::{Value, json};

fn state() -> SessionState {
    SessionState::from_snapshot("p", "i", json!({
        "session_id":"s","subscription_id":"sub","authority_epoch":1,
        "revision":1,"execution_generation":2,"replay_epoch":"e","last_sequence":0,
        "running":false,"messages":[],"pending":[],"prompts":[],"info":{"model":"unchanged"},
        "todo_state":{"revision":3,"todos":[{"id":"task","content":"Current plan","status":"pending"}]}
    })).unwrap()
}
fn raw(kind: &str, sequence: u64, generation: u64, payload: Value) -> Value {
    json!({"jsonrpc":"2.0","method":"event","params":{
        "type":kind,"session_id":"s","replay_epoch":"e","seq":sequence,
        "execution_generation":generation,"authority_epoch":1,"payload":payload
    }})
}
fn event(value: Value) -> Inbound {
    parse_frame(&value.to_string()).unwrap()
}
fn approval(s: &mut SessionState) {
    s.apply(event(raw(
        "approval.request",
        1,
        2,
        json!({
            "kind":"approval","prompt_id":"pr","execution_generation":2,
            "choices":["once","deny"],"command":"review me"
        }),
    )))
    .unwrap();
}

#[test]
fn unknown_future_generation_cannot_erase_current_approval_or_task_plan() {
    for kind in ["session.usage", "mcp.future_status", "future.optional"] {
        let mut s = state();
        approval(&mut s);
        let token = s.prompts["pr"].control_id.clone();
        let plan = s.task_plan.snapshot_id().to_owned();
        let status = s.status.clone();
        assert!(
            s.apply(event(raw(
                kind,
                2,
                999,
                json!({
                    "running":false,"revision":999,"model":"PRIVATE-UNSUPPORTED"
                })
            )))
            .unwrap()
            .is_empty()
        );
        assert_eq!(
            s.generation, 2,
            "unknown {kind} acquired generation authority"
        );
        assert_eq!(s.last_sequence, 2);
        assert_eq!(s.revision, 1);
        assert_eq!(s.info["model"], "unchanged");
        assert_eq!(s.status, status);
        assert_eq!(s.task_plan.snapshot_id(), plan);
        assert_eq!(s.prompts["pr"].control_id, token);
        assert!(s.running && s.connected && !s.is_suspended());
        assert!(matches!(
            s.answer_approval("pr", 2, "deny").unwrap(),
            Action::Approval { .. }
        ));
    }
}

#[test]
fn unknown_idle_frame_does_not_block_send_or_cause_a_following_known_frame_to_gap() {
    let mut s = state();
    let plan = s.task_plan.snapshot_id().to_owned();
    let mut value = raw("future.optional", 1, 999, json!({"running":true}));
    value["params"]
        .as_object_mut()
        .unwrap()
        .remove("authority_epoch");
    assert!(s.apply(event(value)).unwrap().is_empty());
    assert_eq!(s.generation, 2);
    assert!(!s.running && s.can_submit());
    assert_eq!(s.task_plan.snapshot_id(), plan);
    assert!(
        s.apply(event(raw(
            "status.update",
            2,
            2,
            json!({"text":"current status"})
        )))
        .unwrap()
        .is_empty()
    );
    assert_eq!(s.status, "current status");
    assert_eq!(s.last_sequence, 2);
    assert!(!s.is_suspended() && s.can_submit());
}

#[test]
fn unknown_kind_does_not_bypass_replay_gap_epoch_or_authority_checks() {
    for (field, value) in [
        ("seq", json!(2)),
        ("replay_epoch", json!("other")),
        ("authority_epoch", json!(2)),
    ] {
        let mut s = state();
        let mut frame = raw("future.optional", 1, 999, json!({}));
        frame["params"][field] = value;
        assert!(matches!(
            s.apply(event(frame)).unwrap().as_slice(),
            [Action::Snapshot]
        ));
        assert_eq!(s.generation, 2);
        assert!(s.is_suspended() && !s.can_submit());
    }
    let mut s = state();
    let mut frame = raw("future.optional", 1, 999, json!({}));
    frame["params"].as_object_mut().unwrap().remove("seq");
    assert!(s.apply(event(frame)).is_err());
    assert_eq!(s.generation, 2);
}

#[test]
fn foreign_and_duplicate_unknown_events_are_inert_without_consuming_new_sequence() {
    let mut s = state();
    let mut frame = raw("future.optional", 1, 999, json!({}));
    frame["params"]["session_id"] = json!("foreign");
    assert!(s.apply(event(frame)).unwrap().is_empty());
    assert_eq!(s.last_sequence, 0);
    assert!(
        s.apply(event(raw("status.update", 1, 2, json!({"text":"current"}))))
            .unwrap()
            .is_empty()
    );
    assert!(
        s.apply(event(raw("future.optional", 1, 999, json!({}))))
            .unwrap()
            .is_empty()
    );
    assert_eq!((s.generation, s.last_sequence), (2, 1));
    assert_eq!(s.status, "current");
}

#[test]
fn known_execution_and_driver_update_events_keep_their_generation_transition() {
    for (kind, payload) in [
        ("session.updated", json!({"operation":"model","revision":2})),
        ("message.start", json!({})),
        ("session.info", json!({"model":"next"})),
        ("todo.updated", json!({"revision":1,"todos":[]})),
    ] {
        let mut s = state();
        approval(&mut s);
        assert!(s.apply(event(raw(kind, 2, 3, payload))).unwrap().is_empty());
        assert_eq!(s.generation, 3, "known {kind} lost its transition");
        assert!(s.prompts.is_empty());
        assert_eq!(s.last_sequence, 2);
        assert!(!s.is_suspended());
    }
}
