use hermes_tern_gateway::{
    models::{Catalog, ModelControl, Phase},
    session::SessionState,
};
use serde_json::{Value, json};
fn state() -> SessionState {
    SessionState::from_snapshot("profile", "owner", json!({"session_id":"s","subscription_id":"sub","authority_epoch":1,"revision":7,"execution_generation":3,"replay_epoch":"e","last_sequence":0,"running":false,"messages":[],"pending":[],"prompts":[],"info":{"model":"old-model","provider":"old-provider","yolo":false}})).unwrap()
}
fn options() -> Value {
    json!({"model":"old-model","provider":"old-provider","providers":[
        {"slug":"provider-exact","name":"Visible provider","authenticated":true,"models":["model-exact","unavailable"],"unavailable_models":["unavailable"],"api_url":"PRIVATE-URL"},
        {"slug":"no-auth","name":"Not authenticated","authenticated":false,"models":["same"]}]})
}
fn ready() -> ModelControl {
    let mut m = ModelControl::default();
    let id = m.loading();
    m.loaded(&id, options()).unwrap();
    m
}
#[test]
fn model_catalog_preserves_exact_identity_and_auth_unavailable_rows_without_secrets() {
    let c = Catalog::parse(options()).unwrap();
    assert_eq!(c.rows.len(), 3);
    assert_eq!(c.rows[0].provider, "provider-exact");
    assert_eq!(c.rows[0].model, "model-exact");
    assert!(c.rows[0].selectable);
    assert!(!c.rows[1].selectable && !c.rows[2].selectable);
    assert!(!format!("{c:?}").contains("PRIVATE-URL"));
}
#[test]
fn invalid_or_oversized_model_catalog_never_becomes_empty_success() {
    for bad in [
        json!({}),
        json!({"providers":true}),
        json!({"providers":[{"slug":"bad\n","name":"bad","models":[]}]}),
        json!({"providers":[{"slug":"p","name":"p","models":["a","a"]}]}),
        json!({"providers":[{"slug":"p","name":"p","models":[3]}]}),
        json!({"providers":[{"slug":"p","name":"p","models":["x".repeat(1025)]}]}),
    ] {
        assert!(Catalog::parse(bad).is_err());
    }
    let rows = (0..4097).map(|n| format!("m-{n}")).collect::<Vec<_>>();
    assert!(Catalog::parse(json!({"providers":[{"slug":"p","name":"p","models":rows}]})).is_err());
    assert!(
        Catalog::parse(json!({"providers":[]}))
            .unwrap()
            .rows
            .is_empty()
    );
}
#[test]
fn selection_and_apply_bind_exact_catalog_identity_and_snapshot_fences() {
    let s = state();
    let mut m = ready();
    let catalog = m.catalog_id().to_owned();
    assert!(m.select(&s, "foreign", 0).is_err());
    assert!(m.select(&s, &catalog, 1).is_err());
    m.select(&s, &catalog, 0).unwrap();
    let control = m.control_id().unwrap().to_owned();
    let payload = m.apply(&s, &control, false).unwrap();
    assert_eq!(payload["session_id"], "s");
    assert_eq!(payload["expected_revision"], 7);
    assert_eq!(payload["expected_generation"], 3);
    assert_eq!(payload["operation"], "model");
    assert_eq!(
        payload["payload"],
        json!({"model":"model-exact","provider":"provider-exact"})
    );
    assert!(
        payload["request_id"]
            .as_str()
            .is_some_and(|id| !id.is_empty())
    );
    assert!(
        m.apply(&s, &control, false).is_err(),
        "duplicate click sent again"
    );
    assert_eq!(s.info["model"], "old-model");
}
#[test]
fn backend_confirmation_is_explicit_target_bound_and_never_optimistic() {
    let mut s = state();
    let mut m = ready();
    let catalog = m.catalog_id().to_owned();
    m.select(&s, &catalog, 0).unwrap();
    let first = m.control_id().unwrap().to_owned();
    let request = m.apply(&s, &first, false).unwrap();
    m.result(&s,json!({"session_id":"s","operation":"model","status":"confirmation_required","confirm":"token-exact","target_model":"model-exact","target_provider":"provider-exact","confirm_message":"Cost guard\u{1b}","warnings":[]})).unwrap();
    assert_eq!(m.phase(), Phase::Confirmation);
    assert_eq!(s.info["model"], "old-model");
    let confirm = m.control_id().unwrap().to_owned();
    assert_ne!(first, confirm);
    assert!(m.apply(&s, &first, true).is_err());
    assert!(m.apply(&s, &confirm, false).is_err());
    s.revision += 1;
    assert!(m.apply(&s, &confirm, true).is_err(), "stale consent reused");
    s.revision -= 1;
    let confirmed = m.apply(&s, &confirm, true).unwrap();
    assert_eq!(confirmed["payload"]["confirm"], "token-exact");
    assert_ne!(
        request["request_id"], confirmed["request_id"],
        "changed payload reused receipt identity"
    );
}
#[test]
fn deferred_refusal_disconnect_and_old_inventory_cannot_change_active_route_or_resend() {
    for status in ["deferred", "committed"] {
        let s = state();
        let mut m = ready();
        let catalog = m.catalog_id().to_owned();
        m.select(&s, &catalog, 0).unwrap();
        let control = m.control_id().unwrap().to_owned();
        m.apply(&s, &control, false).unwrap();
        m.result(&s,json!({"session_id":"s","operation":"model","status":status,"revision":8,"execution_generation":3,"model":"model-exact","provider":"provider-exact"})).unwrap();
        assert_eq!(s.info["model"], "old-model");
        assert!(m.apply(&s, &control, false).is_err());
    }
    let mut s = state();
    let mut m = ready();
    let old = m.catalog_id().to_owned();
    m.select(&s, &old, 0).unwrap();
    let control = m.control_id().unwrap().to_owned();
    s.mark_disconnected();
    assert!(m.apply(&s, &control, false).is_err());
    let new = m.loading();
    assert_ne!(old, new);
    assert!(m.loaded(&old, options()).is_err());
    m.loaded(&new, options()).unwrap();
    assert!(m.select(&state(), &old, 0).is_err());
}

#[test]
fn committed_model_readback_must_reach_its_receipt_revision_and_generation() {
    let s = state();
    let mut m = ready();
    let catalog = m.catalog_id().to_owned();
    m.select(&s, &catalog, 0).unwrap();
    let control = m.control_id().unwrap().to_owned();
    m.apply(&s, &control, false).unwrap();
    m.result(
        &s,
        json!({"session_id":"s","operation":"model","revision":8,"execution_generation":4}),
    )
    .unwrap();
    for (revision, generation) in [(7, 3), (8, 3), (7, 4)] {
        assert!(
            m.validate_readback(
                &json!({"session_id":"s","revision":revision,"execution_generation":generation})
            )
            .is_err()
        );
        assert_eq!(m.phase(), Phase::Reconciling);
    }
    assert!(
        m.validate_readback(&json!({"session_id":"other","revision":8,"execution_generation":4}))
            .is_err()
    );
    assert!(
        m.validate_readback(&json!({"session_id":"s","revision":8,"execution_generation":4}))
            .is_ok()
    );
    assert!(
        m.validate_readback(&json!({"session_id":"s","revision":9,"execution_generation":5}))
            .is_ok()
    );
}
