use hermes_tern_gateway::{mcp::McpReload, session::SessionState};
use hermes_tern_ui::{composer::Composer, mcp::McpPanel};
use serde_json::{Value, json};
use tern_sdk::{keys::Key, ui::Node};
fn state() -> SessionState {
    SessionState::from_snapshot("profile-exact", "owner-exact", json!({"session_id":"s","subscription_id":"sub","authority_epoch":1,"revision":1,"execution_generation":2,"replay_epoch":"e","last_sequence":0,"running":false,"messages":[],"pending":[],"prompts":[],"info":{"yolo":false,"profile_id":"profile-exact"}})).unwrap()
}
fn text<M>(n: &Node<M>) -> String {
    let mut t = n
        .props()
        .get("text")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    for c in n.child_nodes() {
        t.push('\n');
        t.push_str(&text(c));
    }
    t
}
fn render(s: &SessionState, m: &McpReload, p: &McpPanel, draft: &Composer) -> String {
    p.render(s, m, draft, None, true)
        .regions()
        .iter()
        .flatten()
        .map(|n| text(n))
        .collect::<Vec<_>>()
        .join("\n")
}
#[test]
fn review_discloses_qualified_scope_and_side_effecting_reattach_without_sending_draft() {
    let s = state();
    let mut m = McpReload::default();
    m.review(&s).unwrap();
    let mut panel = McpPanel::default();
    panel.set_open(true);
    let mut draft = Composer::default();
    draft.insert("keep 😀").unwrap();
    let all = render(&s, &m, &panel, &draft);
    for expected in [
        "profile-exact",
        "owner-exact",
        "every chat",
        "reattach",
        "hooks",
        "Confirm profile reload and reattach",
        "keep 😀",
    ] {
        assert!(all.contains(expected), "missing {expected}");
    }
    assert!(!all.contains("Apply to this session"));
    assert!(panel.key(&Key::named("enter")));
    assert_eq!(draft.text(), "keep 😀");
    assert_eq!(m.attempts(), 0);
}
#[test]
fn panel_opening_fences_rotate_and_pending_cannot_offer_another_confirm() {
    let s = state();
    let mut m = McpReload::default();
    m.review(&s).unwrap();
    let id = m.id().to_owned();
    let mut p = McpPanel::default();
    p.set_open(true);
    let old = p.control_id();
    assert!(p.accepts(old));
    assert!(p.key(&Key::named("escape")));
    assert!(!p.accepts(old));
    p.set_open(true);
    assert!(!p.accepts(old));
    m.confirm(&s, &id).unwrap();
    assert!(
        !render(&s, &m, &p, &Composer::default()).contains("Confirm profile reload and reattach")
    );
    m.unknown();
    p.set_open(false);
    p.set_open(true);
    let all = render(&s, &m, &p, &Composer::default());
    assert!(all.contains("outcome unknown"));
    assert!(!all.contains("Review another reload"));
    assert!(!p.key(&Key {
        ctrl: true,
        ..Key::named("d")
    }));
}
