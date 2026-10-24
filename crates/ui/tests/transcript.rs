use hermes_tern_gateway::session::SessionState;
use hermes_tern_ui::{
    composer::Composer,
    transcript::Transcript,
    view::{UiAction, render_with_transcript},
};
use serde_json::json;
use tern_sdk::ui::Node;

fn state(texts: &[String]) -> SessionState {
    SessionState::from_snapshot("p", "i", json!({
        "session_id":"s","subscription_id":"sub","authority_epoch":1,"revision":1,
        "execution_generation":0,"replay_epoch":"e","last_sequence":0,"running":false,
        "messages":texts.iter().enumerate().map(|(i,t)|json!({"row_id":i,"role":"tool","text":t})).collect::<Vec<_>>(),
        "pending":[],"prompts":[],"info":{}
    })).unwrap()
}
fn keyed<'a, M>(node: &'a Node<M>, key: &str) -> Option<&'a Node<M>> {
    if node.props().get("key").and_then(serde_json::Value::as_str) == Some(key) {
        return Some(node);
    }
    node.child_nodes().iter().find_map(|n| keyed(n, key))
}
#[test]
fn long_rows_fold_by_bytes_or_lines_without_losing_retained_text() {
    for text in ["x".repeat(2000), "line\n".repeat(20), "😀".repeat(400)] {
        let s = state(std::slice::from_ref(&text));
        let mut local = Transcript::default();
        let mut draft = Composer::default();
        draft.insert("keep tiếng Việt 😀").unwrap();
        let v = render_with_transcript(&s, &draft, None, &local);
        let row = keyed(v.regions()[0].as_ref().unwrap(), "row-0").unwrap();
        let body = keyed(row, "body").unwrap().props()["text"]
            .as_str()
            .unwrap();
        assert!(body.len() <= 1024 && body.lines().count() <= 12);
        assert!(text.starts_with(body) && body.len() < text.len());
        assert!(keyed(row, &format!("show-{}", s.messages[0].control_id)).is_some());
        local
            .select(&s, "row-0", &s.messages[0].control_id, true)
            .unwrap();
        let expanded = render_with_transcript(&s, &draft, None, &local);
        let row = keyed(expanded.regions()[0].as_ref().unwrap(), "row-0").unwrap();
        assert_eq!(keyed(row, "body").unwrap().props()["text"], text);
        assert!(keyed(row, &format!("collapse-{}", s.messages[0].control_id)).is_some());
        assert_eq!(
            expanded.regions()[1].as_ref().unwrap().child_nodes()[0].props()["text"],
            "keep tiếng Việt 😀"
        );
        assert!(s.can_submit());
    }
}
#[test]
fn only_one_row_opens_and_duplicate_navigation_is_inert() {
    let mut s = state(&["a".repeat(2000), "b".repeat(2000)]);
    let mut local = Transcript::default();
    let a = s.messages[0].control_id.clone();
    let b = s.messages[1].control_id.clone();
    local.select(&s, "row-0", &a, true).unwrap();
    local.select(&s, "row-0", &a, true).unwrap();
    assert!(local.is_expanded("row-0", &a));
    local.select(&s, "row-1", &b, true).unwrap();
    assert!(!local.is_expanded("row-0", &a) && local.is_expanded("row-1", &b));
    // A late collapse for another row must not close the current one.
    local.select(&s, "row-0", &a, false).unwrap();
    assert!(local.is_expanded("row-1", &b));
    assert!(local.select(&s, "absent", &a, true).is_err());
    assert!(local.is_expanded("row-1", &b));
    s.messages.pop_back();
    local.reconcile(&s);
    assert!(!local.is_expanded("row-1", &b));
    assert!(local.select(&s, "row-1", &b, true).is_err());
    s.mark_disconnected();
    local.select(&s, "row-0", &a, true).unwrap();
    assert!(local.is_expanded("row-0", &a));
    assert!(!s.can_submit());
}
#[test]
fn fold_boundary_and_retention_loss_have_distinct_notices() {
    let s = state(&["x".repeat(1024), "a\n".repeat(12), "z".repeat(70000)]);
    let v = render_with_transcript(&s, &Composer::default(), None, &Transcript::default());
    let regions = v.regions();
    let main = regions[0].as_ref().unwrap();
    for id in ["row-0", "row-1"] {
        let row = keyed(main, id).unwrap();
        assert!(!row.child_nodes().iter().any(|n| {
            n.props()
                .get("text")
                .is_some_and(|v| v == "Show retained text")
        }));
        assert!(keyed(row, "truncated").is_none());
    }
    let row = keyed(main, "row-2").unwrap();
    assert!(keyed(row, &format!("show-{}", s.messages[2].control_id)).is_some());
    assert!(
        keyed(row, "truncated").unwrap().props()["text"]
            .as_str()
            .unwrap()
            .contains("truncated")
    );
    assert_eq!(s.messages.back().unwrap().text.len(), 65536);
}

#[test]
fn decoded_inspection_refuses_replacement_and_snapshot_reuse_of_row_identity() {
    let mut s = state(&["a".repeat(2000)]);
    let original = s.messages[0].control_id.clone();
    let stale = UiAction::InspectText {
        id: "row-0".into(),
        control_id: original.clone(),
        expand: true,
    };
    let mut local = Transcript::default();
    local.select(&s, "row-0", &original, true).unwrap();
    stale.validate_control(&s).unwrap();
    // Even an equal-text snapshot with the same upstream row ID is a new instance.
    s = state(&["a".repeat(2000)]);
    assert_ne!(s.messages[0].control_id, original);
    assert!(stale.validate_control(&s).is_err());
    assert!(local.select(&s, "row-0", &original, true).is_err());
    local.reconcile(&s);
    assert!(!local.is_expanded("row-0", &s.messages[0].control_id));
    s.mark_disconnected();
    let current = UiAction::InspectText {
        id: "row-0".into(),
        control_id: s.messages[0].control_id.clone(),
        expand: true,
    };
    current.validate_control(&s).unwrap();
    local
        .select(&s, "row-0", &s.messages[0].control_id, true)
        .unwrap();
    assert!(local.is_expanded("row-0", &s.messages[0].control_id));
}
