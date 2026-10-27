use hermes_tern_gateway::session::SessionState;
use hermes_tern_ui::{composer::Composer, transcript::Transcript, view::render_with_transcript};
use serde_json::{Value, json};
use tern_sdk::ui::Node;

fn keyed<'a, M>(node: &'a Node<M>, key: &str) -> Option<&'a Node<M>> {
    if node.props().get("key").and_then(Value::as_str) == Some(key) {
        return Some(node);
    }
    node.child_nodes().iter().find_map(|n| keyed(n, key))
}
fn state() -> SessionState {
    SessionState::from_snapshot("p", "i", json!({
        "session_id":"s","subscription_id":"sub","authority_epoch":1,"revision":1,
        "execution_generation":2,"replay_epoch":"e","last_sequence":0,"running":false,
        "messages":[{"row_id":1,"role":"assistant","reasoning":"Check the actual parser.","text":"The answer."}],
        "pending":[],"prompts":[],"info":{}
    })).unwrap()
}
#[test]
fn short_reasoning_is_collapsed_until_explicit_local_expansion_without_hiding_answer() {
    let s = state();
    let mut local = Transcript::default();
    let mut composer = Composer::default();
    composer.insert("keep tiếng Việt 😀").unwrap();
    let view = render_with_transcript(&s, &composer, None, &local);
    let regions = view.regions();
    let main = regions[0].as_ref().unwrap();
    let reason = s
        .messages
        .iter()
        .find(|m| m.role == "reasoning")
        .expect("reasoning row");
    let row = keyed(main, &reason.id).unwrap();
    assert_eq!(row.props()["class"], "ht-message ht-reasoning");
    assert_eq!(keyed(row, "role").unwrap().props()["text"], "Reasoning");
    assert!(
        keyed(row, "body").is_none(),
        "collapsed reasoning leaks its body"
    );
    assert_eq!(
        keyed(row, &format!("show-{}", reason.control_id))
            .unwrap()
            .props()["text"],
        "Show reasoning"
    );
    assert_eq!(
        keyed(keyed(main, "row-1").unwrap(), "body")
            .unwrap()
            .props()["text"],
        "The answer."
    );
    local
        .select(&s, &reason.id, &reason.control_id, true)
        .unwrap();
    let expanded = render_with_transcript(&s, &composer, None, &local);
    let row = keyed(expanded.regions()[0].as_ref().unwrap(), &reason.id).unwrap();
    assert_eq!(
        keyed(row, "body").unwrap().props()["text"],
        "Check the actual parser."
    );
    assert_eq!(
        keyed(row, &format!("collapse-{}", reason.control_id))
            .unwrap()
            .props()["text"],
        "Hide reasoning"
    );
    assert_eq!(composer.text(), "keep tiếng Việt 😀");
    assert!(s.can_submit());
}
#[test]
fn reasoning_toggle_is_local_offline_and_fenced_against_snapshot_replacement() {
    let mut s = state();
    let mut local = Transcript::default();
    let reason = s
        .messages
        .iter()
        .find(|m| m.role == "reasoning")
        .expect("reasoning row")
        .clone();
    s.mark_disconnected();
    local
        .select(&s, &reason.id, &reason.control_id, true)
        .unwrap();
    assert!(local.is_expanded(&reason.id, &reason.control_id));
    let fresh = state();
    assert!(
        local
            .select(&fresh, &reason.id, &reason.control_id, true)
            .is_err()
    );
    local.reconcile(&fresh);
    assert!(!local.is_expanded(&reason.id, &reason.control_id));
}
