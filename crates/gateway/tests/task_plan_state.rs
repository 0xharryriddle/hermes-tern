use hermes_tern_gateway::{
    protocol::{Inbound, parse_frame},
    session::{Action, SessionState},
};
use serde_json::{Value, json};

fn plan(revision: u64, name: &str) -> Value {
    json!({"revision":revision,"todos":[{"id":"task-1","content":name,"status":"in_progress"}]})
}
fn snapshot(todo: Value) -> Value {
    json!({"session_id":"s","subscription_id":"sub","authority_epoch":1,"revision":1,"execution_generation":2,"replay_epoch":"e","last_sequence":0,"running":false,"messages":[],"pending":[],"prompts":[],"info":{},"todo_state":todo})
}
fn state(todo: Value) -> SessionState {
    SessionState::from_snapshot("p", "i", snapshot(todo)).unwrap()
}
fn event(kind: &str, seq: u64, generation: u64, payload: Value) -> Inbound {
    parse_frame(&json!({"jsonrpc":"2.0","method":"event","params":{"type":kind,"session_id":"s","replay_epoch":"e","seq":seq,"execution_generation":generation,"authority_epoch":1,"payload":payload}}).to_string()).unwrap()
}
fn text(s: &SessionState) -> String {
    s.task_plan
        .catalog()
        .map(|p| {
            p.rows
                .iter()
                .map(|r| r.text.as_str())
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}

#[test]
fn snapshot_and_full_event_project_only_owner_reported_plan_without_control_actions() {
    let mut s = state(plan(3, "Inspect parser 😀"));
    assert!(text(&s).contains("Inspect parser 😀"));
    assert_eq!(s.task_plan.catalog().unwrap().revision, 3);
    let actions = s
        .apply(event("todo.updated", 1, 2, plan(4, "Verify behavior")))
        .unwrap();
    assert!(actions.is_empty());
    assert!(text(&s).contains("Verify behavior"));
    assert!(s.messages.is_empty() && s.prompts.is_empty() && s.can_submit());
}

#[test]
fn malformed_optional_plan_never_disconnects_disables_send_or_erases_approval_authority() {
    let mut s = state(json!({"revision":1,"todos":[{"id":"t","content":"PRIVATE","status":true}]}));
    assert!(s.task_plan.catalog().is_none() && s.can_submit());
    s.apply(event("approval.request",1,2,json!({"kind":"approval","prompt_id":"pr","execution_generation":2,"choices":["once","deny"],"command":"review"}))).unwrap();
    let actions = s
        .apply(event(
            "todo.updated",
            2,
            2,
            json!({"revision":2,"todos":false}),
        ))
        .unwrap();
    assert!(actions.is_empty() && s.connected && !s.is_suspended());
    assert!(matches!(
        s.answer_approval("pr", 2, "deny").unwrap(),
        Action::Approval { .. }
    ));
}

#[test]
fn foreign_duplicate_unfenced_and_old_generation_events_cannot_replace_plan() {
    let mut s = state(plan(5, "Current"));
    s.apply(event("todo.updated", 1, 1, plan(99, "OLD")))
        .unwrap();
    let mut foreign = json!({"jsonrpc":"2.0","method":"event","params":{"type":"todo.updated","session_id":"other","replay_epoch":"e","seq":2,"authority_epoch":1,"execution_generation":2,"payload":plan(99,"FOREIGN")}});
    s.apply(parse_frame(&foreign.to_string()).unwrap()).unwrap();
    foreign["params"]["session_id"] = json!("s");
    foreign["params"]
        .as_object_mut()
        .unwrap()
        .remove("execution_generation");
    s.apply(parse_frame(&foreign.to_string()).unwrap()).unwrap();
    s.apply(event("todo.updated", 2, 2, plan(99, "DUPLICATE")))
        .unwrap();
    assert!(text(&s).contains("Current"));
    assert!(
        !text(&s).contains("OLD")
            && !text(&s).contains("FOREIGN")
            && !text(&s).contains("DUPLICATE")
    );
    s.apply(event("todo.updated", 3, 2, plan(6, "New")))
        .unwrap();
    assert!(text(&s).contains("New"));
}

#[test]
fn same_scope_snapshot_cannot_regress_task_revision_and_equal_conflict_requires_new_revision() {
    let mut s = state(plan(5, "Newest"));
    let mut fresh = snapshot(plan(3, "Older snapshot"));
    fresh["last_sequence"] = json!(1);
    s.reconcile(fresh).unwrap();
    assert!(text(&s).contains("Newest"));
    s.apply(event("todo.updated", 2, 2, plan(5, "CONFLICT")))
        .unwrap();
    assert!(s.task_plan.catalog().is_none());
    s.apply(event("todo.updated", 3, 2, plan(4, "OLD")))
        .unwrap();
    assert!(s.task_plan.catalog().is_none());
    s.apply(event("todo.updated", 4, 2, plan(6, "Recovered")))
        .unwrap();
    assert!(text(&s).contains("Recovered"));
}

#[test]
fn generation_gap_disconnect_and_epoch_reconcile_invalidate_old_plan_without_controlling_work() {
    let mut s = state(plan(7, "Old"));
    s.apply(event("session.info", 1, 3, json!({}))).unwrap();
    assert!(s.task_plan.catalog().is_none());
    s.apply(event("todo.updated", 2, 3, plan(1, "New generation")))
        .unwrap();
    assert!(text(&s).contains("New generation"));
    let actions = s
        .apply(event("notice", 4, 3, json!({"text":"gap"})))
        .unwrap();
    assert!(matches!(actions.as_slice(), [Action::Snapshot]));
    assert!(s.task_plan.catalog().is_none());
    let mut fresh = snapshot(plan(1, "Reconciled"));
    fresh["authority_epoch"] = json!(2);
    fresh["execution_generation"] = json!(3);
    fresh["replay_epoch"] = json!("new-replay");
    fresh["last_sequence"] = json!(5);
    s.reconcile(fresh).unwrap();
    assert!(text(&s).contains("Reconciled"));
    s.apply(Inbound::Disconnected).unwrap();
    assert!(s.task_plan.catalog().is_none());
    assert!(!s.can_submit());
}

#[test]
fn explicit_clear_is_distinct_from_unreported_and_invalid_newer_snapshot_is_not_current() {
    let mut s = state(plan(3, "Reported"));
    s.apply(event(
        "todo.updated",
        1,
        2,
        json!({"revision":4,"todos":[]}),
    ))
    .unwrap();
    assert!(s.task_plan.catalog().unwrap().rows.is_empty());
    assert_eq!(s.task_plan.catalog().unwrap().revision, 4);
    s.apply(event(
        "todo.updated",
        2,
        2,
        json!({"revision":5,"todos":[{"id":"bad","content":"x","status":"bogus"}]}),
    ))
    .unwrap();
    assert!(s.task_plan.catalog().is_none() && s.can_submit());
    assert!(state(Value::Null).task_plan.catalog().is_none());
    assert!(
        state(json!({"revision":0,"todos":[]}))
            .task_plan
            .catalog()
            .is_none()
    );
}

#[test]
fn receipt_generation_and_payload_generation_invalidate_plan_but_raw_tool_result_cannot_write_it() {
    let mut s = state(plan(9, "Previous generation"));
    s.accept_receipt(json!({"admission_id":"a","ref":{"session_id":"s","profile_id":"p"},"sequence":1,"status":"terminal","execution_generation":3,"authority_epoch":1,"outcome":"complete"})).unwrap();
    assert!(s.task_plan.catalog().is_none());
    s.apply(event("todo.updated", 1, 3, plan(1, "Fresh report")))
        .unwrap();
    s.apply(event("tool.complete", 2, 3, json!({"tool_id":"todo-call","name":"todo_list","todos":[{"id":"forged","content":"RAW-ONLY","status":"completed"}],"revision":999}))).unwrap();
    assert!(text(&s).contains("Fresh report") && !text(&s).contains("RAW-ONLY"));
    s.apply(event(
        "session.info",
        3,
        3,
        json!({"execution_generation":4}),
    ))
    .unwrap();
    assert!(s.task_plan.catalog().is_none());
}

#[test]
fn snapshot_pending_receipt_advancement_discards_snapshot_plan_from_the_previous_generation() {
    let mut v = snapshot(plan(3, "STALE-AT-HYDRATION"));
    v["pending"] = json!([{"admission_id":"a","ref":{"session_id":"s","profile_id":"p"},"sequence":1,"status":"started","execution_generation":3,"authority_epoch":1}]);
    let s = SessionState::from_snapshot("p", "i", v).unwrap();
    assert_eq!(s.generation, 3);
    assert!(s.task_plan.catalog().is_none());
}

#[test]
fn reconcile_never_reapplies_old_snapshot_plan_after_a_pending_receipt_catches_up_generation() {
    let mut s = state(plan(1, "Current generation"));
    s.apply(event("todo.updated", 1, 3, plan(1, "Current generation")))
        .unwrap();
    let mut v = snapshot(plan(50, "STALE-HIGH-TASK-REVISION"));
    v["last_sequence"] = json!(1);
    v["pending"] = json!([{"admission_id":"a","ref":{"session_id":"s","profile_id":"p"},"sequence":1,"status":"started","execution_generation":3,"authority_epoch":1}]);
    s.reconcile(v).unwrap();
    assert_eq!(s.generation, 3);
    assert!(!text(&s).contains("STALE-HIGH-TASK-REVISION"));
}

#[test]
fn unfenced_optional_task_frame_cannot_advance_execution_or_drop_current_human_controls() {
    for missing in ["authority_epoch", "execution_generation"] {
        let mut s = state(plan(2, "Current plan"));
        s.apply(event("approval.request",1,2,json!({"kind":"approval","prompt_id":"pr","execution_generation":2,"choices":["once","deny"],"command":"review"}))).unwrap();
        let mut raw = json!({"jsonrpc":"2.0","method":"event","params":{"type":"todo.updated","session_id":"s","replay_epoch":"e","seq":2,"authority_epoch":1,"execution_generation":999,"payload":plan(99,"UNFENCED")}});
        raw["params"].as_object_mut().unwrap().remove(missing);
        assert!(
            s.apply(parse_frame(&raw.to_string()).unwrap())
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            s.generation, 2,
            "optional frame changed generation without {missing}"
        );
        assert!(text(&s).contains("Current plan"));
        assert!(matches!(
            s.answer_approval("pr", 2, "deny").unwrap(),
            Action::Approval { .. }
        ));
    }
}
