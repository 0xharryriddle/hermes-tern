//! Direct library contract; production process/fault paths are exercised in Python.
use hermes_tern_omp::exchange;
use serde_json::{Value, json};
use std::{fs, path::PathBuf};
use uuid::Uuid;

fn state() -> PathBuf {
    let root = PathBuf::from(std::env::var_os("TMPDIR").expect("private TMPDIR required"))
        .join(format!("ht-omp-lib-{}", Uuid::new_v4()));
    fs::create_dir(&root).unwrap();
    root.join("state")
}
fn request(op: Value) -> Vec<u8> {
    serde_json::to_vec(&json!({"v":1,"request_id":Uuid::new_v4().to_string(),"op":op})).unwrap()
}
#[test]
fn valid_offer_replays_exactly_without_a_second_identity() {
    let state = state();
    let body = request(json!({"type":"offer","text":"Review the change","source":"untrusted"}));
    let first = exchange(&state, &body).unwrap();
    assert_eq!(first, exchange(&state, &body).unwrap());
    let p = &first["proposal"];
    assert_eq!(p["text"], "Review the change");
    assert_ne!(p["handoff_id"], p["attempt_id"]);
    assert_ne!(p["target_id"], p["attempt_id"]);
}
#[test]
fn inspected_fence_is_not_completion_and_never_regrants() {
    let state = state();
    let offered = exchange(
        &state,
        &request(json!({"type":"offer","text":"Review only","source":"fixture"})),
    )
    .unwrap();
    let p = &offered["proposal"];
    let arm = request(
        json!({"type":"arm","handoff_id":p["handoff_id"],"attempt_id":p["attempt_id"],
        "target_id":p["target_id"],"digest":p["digest"]}),
    );
    assert_eq!(exchange(&state, &arm).unwrap()["newly_armed"], true);
    assert_eq!(exchange(&state, &arm).unwrap()["newly_armed"], false);
    let view = exchange(
        &state,
        &request(json!({"type":"inspect","handoff_id":p["handoff_id"]})),
    )
    .unwrap();
    assert_eq!(view["armed"], true);
    assert_eq!(view["observations"], json!([]));
    assert!(view.get("completed").is_none());
}
#[test]
fn closed_operation_envelopes_reject_extra_fields_before_touching_state() {
    let state = state();
    for op in [
        json!({"type":"inspect","handoff_id":Uuid::new_v4().to_string(),"extra":true}),
        json!({"type":"prepare","handoff_id":Uuid::new_v4().to_string(),"extra":true}),
        json!({"type":"offer","text":"Normal","source":"fixture","extra":true}),
    ] {
        assert!(exchange(&state, &request(op)).is_err());
        assert!(!state.exists());
    }
}
#[test]
fn command_prefix_and_unicode_controls_refuse_but_plain_unicode_survives() {
    let state = state();
    for text in [
        " /quit",
        "!command",
        "$variable",
        "@agent",
        "\u{202e}spoof",
        "bad\u{1b}[31m",
    ] {
        assert!(
            exchange(
                &state,
                &request(json!({"type":"offer","text":text,"source":"fixture"}))
            )
            .is_err()
        );
        assert!(!state.exists());
    }
    assert!(
        exchange(
            &state,
            &request(json!({"type":"offer","text":"Tiếng Việt\nplain text","source":"fixture"}))
        )
        .is_ok()
    );
}
