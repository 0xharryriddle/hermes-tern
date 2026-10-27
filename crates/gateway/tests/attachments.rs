//! Admission and draft-reference consent remain distinct from owner staging.
use hermes_tern_gateway::{
    attachments::{Attachments, Phase},
    session::SessionState,
};
use serde_json::json;
use std::os::unix::fs::DirBuilderExt;
use std::{fs, path::PathBuf};
fn state() -> SessionState {
    SessionState::from_snapshot("p","i",json!({"session_id":"s","subscription_id":"sub","authority_epoch":1,"revision":1,"execution_generation":2,"replay_epoch":"e","last_sequence":0,"running":false,"messages":[],"pending":[],"prompts":[],"info":{"yolo":false}})).unwrap()
}
fn file() -> PathBuf {
    let root = PathBuf::from(std::env::var_os("TMPDIR").expect("private TMPDIR required"))
        .join(format!("ht-attachment-state-{}", uuid::Uuid::new_v4()));
    fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
    let path = root.join("sample.txt");
    fs::write(&path, b"reviewed text").unwrap();
    fs::write(
        root.join("ownership.json"),
        serde_json::to_vec(&json!({
            "retention":"retain;no cleanup","paths":[root,path,root.join("ownership.json")]
        }))
        .unwrap(),
    )
    .unwrap();
    path
}
fn receipt() -> serde_json::Value {
    json!({"attached":true,"uploaded":true,"name":"sample.txt","ref_path":"attachments/sample.txt","ref_text":"@file:attachments/sample.txt","api_key":"PRIVATE"})
}
#[test]
fn local_prepare_stage_receipt_and_reference_add_are_four_distinct_steps() {
    let s = state();
    let path = file();
    let mut a = Attachments::default();
    a.prepare(&s, path.to_str().unwrap()).unwrap();
    assert_eq!(a.phase(), Phase::Prepared);
    assert!(!a.blocked());
    let id = a.id().to_owned();
    assert!(a.reference(&s, &id).is_err());
    let p = a.stage(&s, &id).unwrap();
    assert_eq!(p["session_id"], "s");
    assert!(p.get("path").is_none());
    assert_eq!(a.phase(), Phase::Pending);
    assert!(a.blocked());
    assert!(a.stage(&s, &id).is_err());
    a.result(&s, receipt()).unwrap();
    assert_eq!(a.phase(), Phase::Staged);
    assert!(a.reference(&s, &id).is_err());
    let current = a.id().to_owned();
    assert_eq!(
        a.reference(&s, &current).unwrap(),
        "@file:attachments/sample.txt"
    );
    a.added(&s, &current).unwrap();
    assert_eq!(a.phase(), Phase::Added);
    assert!(a.reference(&s, &current).is_err());
    assert!(!a.blocked());
}
#[test]
fn path_edits_and_owner_generation_changes_reject_old_upload_and_add_authority() {
    let mut s = state();
    let path = file();
    let mut a = Attachments::default();
    a.prepare(&s, path.to_str().unwrap()).unwrap();
    let old = a.id().to_owned();
    a.invalidate_prepared();
    assert!(a.stage(&s, &old).is_err());
    a.prepare(&s, path.to_str().unwrap()).unwrap();
    let id = a.id().to_owned();
    s.generation += 1;
    assert!(a.stage(&s, &id).is_err());
    s.generation -= 1;
    a.stage(&s, &id).unwrap();
    s.revision += 1;
    assert!(a.result(&s, receipt()).is_err());
    assert_eq!(a.phase(), Phase::Unknown);
    assert!(a.blocked());
    assert!(a.prepare(&s, path.to_str().unwrap()).is_err());
}
#[test]
fn malformed_or_refused_admitted_upload_is_unknown_not_an_empty_retryable_result() {
    let s = state();
    let path = file();
    for result in [
        json!({}),
        json!({"attached":false}),
        json!({"type":"exec","output":"PRIVATE"}),
    ] {
        let mut a = Attachments::default();
        a.prepare(&s, path.to_str().unwrap()).unwrap();
        let id = a.id().to_owned();
        a.stage(&s, &id).unwrap();
        assert!(a.result(&s, result).is_err());
        assert!(a.blocked());
        assert!(a.receipt().is_none());
        assert!(a.prepared().is_none());
    }
}
#[test]
fn unsent_refusal_restores_explicit_review_but_does_not_clear_an_unknown_outcome() {
    let s = state();
    let path = file();
    let mut a = Attachments::default();
    a.prepare(&s, path.to_str().unwrap()).unwrap();
    let id = a.id().to_owned();
    a.stage(&s, &id).unwrap();
    a.not_enqueued();
    assert_eq!(a.phase(), Phase::Failed);
    assert!(!a.blocked());
    assert_eq!(a.attempts(), 0);
    a.prepare(&s, path.to_str().unwrap()).unwrap();
    let id = a.id().to_owned();
    a.stage(&s, &id).unwrap();
    a.disconnected();
    a.not_enqueued();
    assert_eq!(a.phase(), Phase::Unknown);
    assert!(a.blocked());
}
#[test]
fn per_connection_eight_upload_bound_retains_admitted_attempts_after_local_reference_adds() {
    let s = state();
    let path = file();
    let mut a = Attachments::default();
    for n in 1..=8 {
        a.prepare(&s, path.to_str().unwrap()).unwrap();
        let id = a.id().to_owned();
        a.stage(&s, &id).unwrap();
        assert_eq!(a.attempts(), n);
        a.result(&s, receipt()).unwrap();
        let id = a.id().to_owned();
        a.added(&s, &id).unwrap();
    }
    a.prepare(&s, path.to_str().unwrap()).unwrap();
    let id = a.id().to_owned();
    assert!(a.stage(&s, &id).is_err());
    assert_eq!(a.attempts(), 8);
    assert!(!a.blocked());
}

#[test]
fn closing_or_editing_locally_cannot_clear_a_staging_result_that_may_persist_in_the_owner() {
    let s = state();
    let path = file();
    let mut a = Attachments::default();
    a.prepare(&s, path.to_str().unwrap()).unwrap();
    let id = a.id().to_owned();
    a.stage(&s, &id).unwrap();
    a.invalidate_prepared();
    assert_eq!(a.phase(), Phase::Pending);
    assert_eq!(a.attempts(), 1);
    a.unknown();
    a.invalidate_prepared();
    assert_eq!(a.phase(), Phase::Unknown);
    assert_eq!(a.attempts(), 1);
    assert!(a.stage(&s, &id).is_err());
    assert!(a.receipt().is_none());
}
