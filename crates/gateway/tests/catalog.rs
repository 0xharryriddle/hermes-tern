use hermes_tern_gateway::catalog::{Catalog, Scope};
use serde_json::{Value, json};

fn row(id: &str) -> Value {
    json!({"session_id":id,"title":"same title","model":"model","source":"tui","running":false})
}
fn response(scope: &str, rows: Vec<Value>) -> Value {
    json!({"scope":scope,"sessions":rows})
}
#[test]
fn catalog_preserves_exact_identity_and_inert_bounded_metadata() {
    let mut a = row("opaque-A");
    a["title"] = json!("untrusted\u{1b}[31m\u{202e}\n$(touch nope)");
    let c = Catalog::from_responses(
        response("live", vec![a]),
        response("stored", vec![row("opaque-B")]),
    )
    .unwrap();
    assert_eq!(c.entries(Scope::Live)[0].session_id, "opaque-A");
    assert_eq!(c.entries(Scope::Stored)[0].session_id, "opaque-B");
    let title = &c.entries(Scope::Live)[0].title;
    assert!(!title.contains('\u{1b}') && !title.contains('\u{202e}') && !title.contains('\n'));
    assert!(title.contains("$(touch nope)"));
    assert_eq!(c.entries(Scope::Live)[0].model.as_deref(), Some("model"));
}
#[test]
fn catalog_rejects_wrong_scope_malformed_identity_duplicate_and_oversized_rows() {
    for bad in [
        json!({"scope":"stored","sessions":[]}),
        json!({"scope":"live"}),
        response("live", vec![json!(true)]),
        response("live", vec![row("")]),
        response("live", vec![row("bad\u{202e}")]),
        response("live", vec![row("bad\nidentity")]),
        response("live", vec![row("x"), row("x")]),
        response("live", vec![json!({"session_id":"x","running":"yes"})]),
        response("live", (0..201).map(|n| row(&format!("id-{n}"))).collect()),
    ] {
        assert!(Catalog::from_responses(bad, response("stored", vec![])).is_err());
    }
    let mut huge = row("x");
    huge["title"] = json!("x".repeat(4097));
    assert!(
        Catalog::from_responses(response("live", vec![huge]), response("stored", vec![])).is_err()
    );
}
#[test]
fn catalog_discloses_caps_without_inventing_total_or_dropping_cross_scope_rows() {
    let rows = (0..200).map(|n| row(&format!("id-{n}"))).collect();
    let c = Catalog::from_responses(
        response("live", rows),
        response("stored", vec![row("id-0")]),
    )
    .unwrap();
    assert_eq!(c.entries(Scope::Live).len(), 200);
    assert!(c.at_limit(Scope::Live));
    assert!(!c.at_limit(Scope::Stored));
    assert_eq!(c.entries(Scope::Stored).len(), 1);
}
