use hermes_tern_gateway::{models::ModelControl, session::SessionState};
use hermes_tern_ui::{composer::Composer, models::ModelPicker};
use serde_json::{Value, json};
use tern_sdk::ui::Node;
fn state() -> SessionState {
    SessionState::from_snapshot("p","i",json!({"session_id":"s","subscription_id":"sub","authority_epoch":1,"revision":1,"execution_generation":0,"replay_epoch":"e","last_sequence":0,"running":false,"messages":[],"pending":[],"prompts":[],"info":{"model":"active","provider":"current","yolo":false}})).unwrap()
}
fn text<M>(n: &Node<M>) -> String {
    let mut out = n
        .props()
        .get("text")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    for child in n.child_nodes() {
        out.push_str(&text(child));
    }
    out
}
fn all(p: &ModelPicker, s: &SessionState, m: &ModelControl) -> String {
    let v = p.render(s, m, &Composer::default(), None);
    v.regions()
        .iter()
        .flatten()
        .map(|n| text(n))
        .collect::<Vec<_>>()
        .join("\n")
}
#[test]
fn model_picker_pages_exact_options_and_proposes_before_apply_without_optimistic_badge() {
    let s = state();
    let mut m = ModelControl::default();
    let id = m.loading();
    m.loaded(&id,json!({"providers":[{"slug":"exact-p","name":"provider","authenticated":true,"models":(0..30).map(|n|format!("m-{n:02}")).collect::<Vec<_>>()}]})).unwrap();
    let mut p = ModelPicker::default();
    p.set_open(true);
    let out = all(&p, &s, &m);
    assert!(out.contains("Active route: active · current"));
    assert!(out.contains("Select m-00") && !out.contains("Select m-29"));
    m.select(&s, &id, 0).unwrap();
    let out = all(&p, &s, &m);
    assert!(out.contains("Apply to this session"));
    assert!(out.contains("Active route: active · current"));
    p.page(&m, 0, 1);
    assert!(!all(&p, &s, &m).contains("Select m-00"));
    p.page(&m, 0, 2);
    assert_eq!(p.current_page(), 1);
}
#[test]
fn model_picker_loading_refusal_and_unknown_inventory_are_named_and_not_selectable() {
    let s = state();
    let mut m = ModelControl::default();
    let mut p = ModelPicker::default();
    p.set_open(true);
    m.loading();
    assert!(all(&p, &s, &m).contains("Loading"));
    m.failed("Model inventory unavailable");
    let out = all(&p, &s, &m);
    assert!(out.contains("unavailable") && !out.contains("Apply to this session"));
    assert!(out.contains("Return to conversation"));
}
#[test]
fn conversation_dock_groups_runtime_and_model_navigation_without_two_extra_vertical_rows() {
    let v = hermes_tern_ui::view::render(&state(), &Composer::default(), None);
    let dock = v.regions()[1].as_ref().unwrap().child_nodes();
    assert_eq!(
        dock.len(),
        4,
        "model entry added an extra dock row that clips expanded output"
    );
    assert_eq!(dock[2].props()["key"], "navigation");
    assert!(
        dock[2].props()["class"]
            .as_str()
            .unwrap()
            .contains("ht-navigation")
    );
    assert_eq!(dock[2].child_nodes().len(), 3);
    assert!(text(&dock[2]).contains("Runtime details · F2"));
    assert!(text(&dock[2]).contains("Session model · F3"));
    assert!(text(&dock[2]).contains("Commands · F4"));
}
