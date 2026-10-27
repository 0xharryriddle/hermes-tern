use hermes_tern_gateway::{attachments::Attachments, session::SessionState};
use hermes_tern_ui::{attachments::AttachmentPanel, composer::Composer};
use serde_json::{Value, json};
use tern_sdk::{keys::Key, ui::Node};
fn state() -> SessionState {
    SessionState::from_snapshot("p","i",json!({"session_id":"s","subscription_id":"sub","authority_epoch":1,"revision":1,"execution_generation":2,"replay_epoch":"e","last_sequence":0,"running":false,"messages":[],"pending":[],"prompts":[],"info":{"yolo":false}})).unwrap()
}
fn text<M>(n: &Node<M>) -> String {
    let mut t = n
        .props()
        .get("text")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    for n in n.child_nodes() {
        t.push_str(&text(n));
    }
    t
}
#[test]
fn attachment_path_editor_never_changes_or_submits_the_conversation_draft() {
    let mut p = AttachmentPanel::default();
    p.set_open(true);
    let mut draft = Composer::default();
    draft.insert("keep 😀").unwrap();
    assert!(
        p.key(&Key {
            name: "paste".into(),
            text: Some("/private/file.txt".into()),
            ..Key::default()
        })
        .unwrap()
    );
    let path = p.path().to_owned();
    let token = p.control_id();
    assert!(p.key(&Key::named("enter")).unwrap());
    assert_eq!(p.path(), path);
    assert_eq!(draft.text(), "keep 😀");
    assert!(p.accepts(token));
    let s = state();
    let view = p.render(&s, &Attachments::default(), &draft, None, true);
    let all = view
        .regions()
        .iter()
        .flatten()
        .map(|node| text(node))
        .collect::<String>();
    assert!(
        all.contains("Review local file")
            && all.contains("keep 😀")
            && !all.contains("Stage reviewed bytes")
    );
    assert!(p.key(&Key::named("escape")).unwrap());
    assert!(!p.accepts(token));
}
#[test]
fn prior_panel_and_path_callbacks_lose_authority_after_edit_or_reopen() {
    let mut p = AttachmentPanel::default();
    p.set_open(true);
    let old = p.control_id();
    p.key(&Key {
        name: "paste".into(),
        text: Some("/a".into()),
        ..Key::default()
    })
    .unwrap();
    assert!(!p.accepts(old));
    let edited = p.control_id();
    p.set_open(false);
    assert!(!p.accepts(edited));
    p.set_open(true);
    assert!(!p.accepts(edited));
    assert!(p.accepts(p.control_id()));
}
#[test]
fn control_tainted_or_oversized_path_is_refused_without_repairing_existing_input() {
    let mut p = AttachmentPanel::default();
    p.set_open(true);
    for bad in [
        "/a\u{1b}/b".into(),
        "/a\n/b".into(),
        "/a\u{202e}/b".into(),
        "x".repeat(4097),
    ] {
        assert!(
            p.key(&Key {
                name: "paste".into(),
                text: Some(bad),
                ..Key::default()
            })
            .is_err()
        );
        assert_eq!(p.path(), "");
    }
}
