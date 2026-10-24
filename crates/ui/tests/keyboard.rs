use hermes_tern_gateway::session::SessionState;
use hermes_tern_ui::{composer::Composer, transcript::Transcript, view::render_with_transcript};
use serde_json::json;
use tern_sdk::keys::Key;

fn state() -> SessionState {
    SessionState::from_snapshot("p","i",json!({"session_id":"s","subscription_id":"sub","authority_epoch":1,"revision":1,"execution_generation":0,"replay_epoch":"e","last_sequence":0,"running":false,"messages":[{"row_id":1,"role":"tool","text":"x".repeat(2000)},{"row_id":2,"role":"tool","text":"short"},{"row_id":3,"role":"tool","text":"y".repeat(2000)}],"pending":[],"prompts":[],"info":{}})).unwrap()
}
#[test]
fn keyboard_cycles_foldable_rows_and_composer_without_authority_actions() {
    let s = state();
    let mut t = Transcript::default();
    let tab = Key::named("tab");
    let back = Key {
        shift: true,
        ..Key::named("tab")
    };
    assert!(t.keyboard(&s, &tab, None).unwrap());
    assert!(t.is_selected("row-1", &s.messages[0].control_id));
    let token = t.navigation_revision();
    assert!(t.keyboard(&s, &Key::named("enter"), None).is_err());
    assert!(!t.is_expanded("row-1", &s.messages[0].control_id));
    t.keyboard(&s, &Key::named("enter"), Some(token)).unwrap();
    assert!(t.is_expanded("row-1", &s.messages[0].control_id));
    assert!(
        t.keyboard(&s, &Key::named("space"), Some(token)).is_err(),
        "old drawn state toggled twice"
    );
    t.keyboard(&s, &Key::named("space"), Some(t.navigation_revision()))
        .unwrap();
    t.keyboard(&s, &tab, None).unwrap();
    assert!(t.is_selected("row-3", &s.messages[2].control_id));
    t.keyboard(&s, &tab, None).unwrap();
    assert!(!t.inspecting());
    t.keyboard(&s, &back, None).unwrap();
    assert!(t.is_selected("row-3", &s.messages[2].control_id));
    t.keyboard(&s, &back, None).unwrap();
    assert!(t.is_selected("row-1", &s.messages[0].control_id));
    t.keyboard(&s, &back, None).unwrap();
    assert!(!t.inspecting());
    assert!(s.can_submit());
}
#[test]
fn stale_selection_consumes_enter_and_typing_until_explicit_navigation() {
    let mut s = state();
    let mut t = Transcript::default();
    t.keyboard(&s, &Key::named("tab"), None).unwrap();
    let old = t.navigation_revision();
    s.messages.pop_front();
    t.reconcile(&s);
    assert!(t.inspecting());
    assert!(t.keyboard(&s, &Key::named("enter"), Some(old)).is_err());
    for key in [
        Key {
            name: "x".into(),
            text: Some("x".into()),
            ..Key::default()
        },
        Key {
            name: "paste".into(),
            text: Some("keep".into()),
            ..Key::default()
        },
    ] {
        assert!(t.keyboard(&s, &key, Some(old)).unwrap());
    }
    assert!(
        !t.keyboard(
            &s,
            &Key {
                ctrl: true,
                ..Key::named("c")
            },
            Some(old)
        )
        .unwrap()
    );
    t.keyboard(&s, &Key::named("escape"), None).unwrap();
    assert!(!t.inspecting());
    assert!(!t.keyboard(&s, &Key::named("enter"), None).unwrap());
    s.mark_disconnected();
    t.keyboard(&s, &Key::named("tab"), None).unwrap();
    assert!(t.is_selected("row-3", &s.messages[1].control_id));
}
#[test]
fn selected_row_render_is_visibly_marked_and_editor_draft_is_unchanged() {
    let s = state();
    let mut t = Transcript::default();
    let mut c = Composer::default();
    c.insert("draft 😀").unwrap();
    t.keyboard(&s, &Key::named("tab"), None).unwrap();
    let v = render_with_transcript(&s, &c, None, &t);
    let regions = v.regions();
    let main = regions[0].unwrap();
    let selected = main
        .child_nodes()
        .iter()
        .flat_map(|n| n.child_nodes())
        .filter(|n| {
            n.props()
                .get("class")
                .and_then(|v| v.as_str())
                .is_some_and(|s| s.split_whitespace().any(|c| c == "ht-keyboard-focus"))
        })
        .count();
    assert_eq!(selected, 1);
    assert_eq!(
        regions[1].unwrap().child_nodes()[0].props()["text"],
        "draft 😀"
    );
    assert_eq!(
        regions[1].unwrap().child_nodes()[0].props()["sendable"],
        false
    );
    assert!(
        s.can_submit(),
        "local navigation changed backend submit policy"
    );
}
