use hermes_tern_gateway::{commands::Commands, session::SessionState};
use hermes_tern_ui::{commands::CommandPanel, composer::Composer, view::UiAction};
use serde_json::{Value, json};
use tern_sdk::{keys::Key, ui::Node};
fn state() -> SessionState {
    SessionState::from_snapshot("p","i",json!({"session_id":"s","subscription_id":"sub","authority_epoch":1,"revision":1,"execution_generation":0,"replay_epoch":"e","last_sequence":0,"running":false,"messages":[],"pending":[],"prompts":[],"info":{"yolo":false}})).unwrap()
}
fn text<M>(n: &Node<M>) -> String {
    let mut s = n
        .props()
        .get("text")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    for n in n.child_nodes() {
        s.push_str(&text(n));
    }
    s
}
fn all(p: &CommandPanel, s: &SessionState, c: &Commands, d: &Composer) -> String {
    p.render(s, c, d, None)
        .regions()
        .iter()
        .flatten()
        .map(|n| text(n))
        .collect::<String>()
}
fn catalog() -> Value {
    let mut rows = vec![
        json!(["/memory", "Read memory"]),
        json!(["/skills", "Read skills"]),
    ];
    rows.extend((0..20).map(|i| json!([format!("/private-{i}"), "PRIVATE detail"])));
    json!({"pairs":rows,"canon":{"/memory":"/memory","/skills":"/skills"},"commands":{"/memory":{},"/skills":{}},"skills":{},"plugin_commands":{}})
}
#[test]
fn command_panel_is_inspection_only_with_exact_visible_read_rows_and_preserved_draft() {
    let s = state();
    let mut c = Commands::default();
    let id = c.loading(&s).unwrap();
    c.loaded(&s, &id, catalog()).unwrap();
    let mut p = CommandPanel::default();
    p.set_open(true);
    let mut draft = Composer::default();
    draft.insert("keep 😀").unwrap();
    let a = all(&p, &s, &c, &draft);
    assert!(
        a.contains("Owner commands")
            && a.contains("Read /memory")
            && a.contains("keep 😀")
            && !a.contains("PRIVATE detail")
    );
    assert!(p.visible(0) && !p.visible(12));
    assert!(p.key(&c, &Key::named("enter")));
    assert_eq!(draft.text(), "keep 😀");
    p.page(&c, 0, 1);
    assert!(!p.visible(0));
    assert!(!all(&p, &s, &c, &draft).contains("Read /memory"));
    assert!(p.key(&c, &Key::named("escape")));
    assert!(!p.is_open());
}
#[test]
fn command_output_chunks_are_local_paged_and_never_turn_into_send_actions() {
    let s = state();
    let mut c = Commands::default();
    let id = c.loading(&s).unwrap();
    c.loaded(&s, &id, catalog()).unwrap();
    c.read(&s, &id, 0).unwrap();
    c.result(
        &s,
        json!({"type":"exec","output":format!("{}last-chunk","x".repeat(4096))}),
    )
    .unwrap();
    let mut p = CommandPanel::default();
    p.set_open(true);
    let d = Composer::default();
    assert!(!all(&p, &s, &c, &d).contains("last-chunk"));
    p.output_page(&c, 0, 1);
    assert!(all(&p, &s, &c, &d).contains("last-chunk"));
    let v = p.render(&s, &c, &d, None);
    fn no_send(n: &Node<UiAction>) {
        assert!(!matches!(
            n.props().get("sendable"),
            Some(Value::Bool(true))
        ));
        for child in n.child_nodes() {
            no_send(child);
        }
    }
    for n in v.regions().iter().flatten() {
        no_send(n);
    }
}

#[test]
fn decoded_read_action_cannot_cross_panel_close_and_reopen() {
    let mut p = CommandPanel::default();
    p.set_open(true);
    let old = p.control_id();
    assert!(p.accepts_read(old, 0));
    p.set_open(false);
    assert!(!p.accepts_read(old, 0));
    p.set_open(true);
    assert!(
        !p.accepts_read(old, 0),
        "queued typed action survived panel reopening"
    );
    assert!(p.accepts_read(p.control_id(), 0));
}

#[test]
fn completed_owner_output_precedes_the_catalog_so_short_results_are_not_below_its_scroll_fold() {
    let s = state();
    let mut c = Commands::default();
    let id = c.loading(&s).unwrap();
    c.loaded(&s, &id, catalog()).unwrap();
    c.read(&s, &id, 0).unwrap();
    c.result(&s, json!({"type":"exec","output":"complete-owner-result"}))
        .unwrap();
    let mut panel = CommandPanel::default();
    panel.set_open(true);
    let out = all(&panel, &s, &c, &Composer::default());
    assert!(out.find("complete-owner-result").unwrap() < out.find("Read /memory").unwrap());
}
