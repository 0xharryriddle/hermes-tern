//! Offline contracts; no live owner, config, provider or network calls.
use hermes_tern_gateway::{
    mcp::{McpReload, Phase, Report, SessionOutcome},
    session::SessionState,
};
use serde_json::{Value, json};
fn snapshot() -> Value {
    json!({"session_id":"s","subscription_id":"sub","authority_epoch":3,"revision":7,"execution_generation":4,"replay_epoch":"epoch","last_sequence":9,"running":false,"messages":[],"pending":[],"prompts":[],"info":{"profile_id":"p","yolo":false,"model":"m","provider":"v"}})
}
fn state() -> SessionState {
    SessionState::from_snapshot("p", "i", snapshot()).unwrap()
}
fn reply(outcome: &str) -> Value {
    json!({"status":"reloaded","session":outcome})
}
fn confirm(m: &mut McpReload, s: &SessionState) -> Value {
    let id = m.id().to_owned();
    m.confirm(s, &id).unwrap()
}
fn setup(s: &SessionState, stage: Phase) -> McpReload {
    let mut m = McpReload::default();
    m.review(s).unwrap();
    if stage != Phase::Review {
        confirm(&mut m, s);
    }
    m
}
fn reconciling(s: &SessionState, outcome: &str) -> McpReload {
    let mut m = setup(s, Phase::Pending);
    m.result(s, reply(outcome)).unwrap();
    m
}
fn change(mut v: Value, key: &str, field: Value) -> Value {
    v[key] = field;
    v
}
fn rejects(base: Value, variants: Value, check: impl Fn(Value) -> bool) {
    for (key, values) in variants.as_object().unwrap() {
        for v in values.as_array().unwrap() {
            assert!(check(change(base.clone(), key, v.clone())), "{key}");
        }
    }
}
fn edit(s: &mut SessionState, n: usize) {
    match n {
        0 => s.profile.push('x'),
        1 => s.instance.push('x'),
        2 => s.session_id.push('x'),
        3 => s.authority_epoch += 1,
        4 => s.subscription_id.push('x'),
        5 => s.revision += 1,
        6 => s.generation += 1,
        7 => s.replay_epoch.push('x'),
        8 => s.last_sequence += 1,
        9 => s.info["model"] = json!("other"),
        10 => s.info["provider"] = json!("other"),
        11 => s.connected = false,
        12 => s.running = true,
        13 => s.info["yolo"] = json!(true),
        _ => {
            s.submit("pending", "input").unwrap();
        }
    }
}
fn phase(m: &McpReload, expected: Phase, attempts: usize, blocked: bool) {
    let actual = (m.phase(), m.attempts(), m.blocked());
    assert_eq!(actual, (expected, attempts, blocked));
}
fn reattach(m: &mut McpReload, s: &mut SessionState, v: Value) {
    m.validate_readback(s, &v).unwrap();
    s.reconcile(v).unwrap();
    m.reconciled();
}
#[test]
fn review_is_local_cancellable_fresh_uuid_and_confirm_is_minimal_once() {
    let s = state();
    let mut m = setup(&s, Phase::Review);
    let old = m.id().to_owned();
    assert!(uuid::Uuid::parse_str(&old).is_ok());
    phase(&m, Phase::Review, 0, false);
    m.cancel_review();
    assert_ne!(m.id(), old);
    phase(&m, Phase::Empty, 0, false);
    m.review(&s).unwrap();
    assert!(m.confirm(&s, &old).is_err());
    let id = m.id().to_owned();
    let request = m.confirm(&s, &id).unwrap();
    assert_eq!(request, json!({"session_id":"s","confirm":true}));
    assert!(m.confirm(&s, &id).is_err());
    m.cancel_review();
    phase(&m, Phase::Pending, 1, true);
}
#[test]
fn every_local_fence_and_can_submit_state_invalidates_confirm() {
    for n in 0..15 {
        let mut s = state();
        let mut m = setup(&s, Phase::Review);
        edit(&mut s, n);
        let id = m.id().to_owned();
        assert!(m.confirm(&s, &id).is_err(), "fence {n}");
        assert_eq!(m.attempts(), 0);
    }
}
#[test]
fn review_requires_idle_can_submit_and_never_yolo() {
    for n in 11..15 {
        let mut s = state();
        edit(&mut s, n);
        assert!(McpReload::default().review(&s).is_err());
    }
}
#[test]
fn preenqueue_refusal_rolls_back_only_pending_and_requires_fresh_consent() {
    let s = state();
    let mut m = setup(&s, Phase::Pending);
    let old = m.id().to_owned();
    m.not_enqueued();
    m.not_enqueued();
    phase(&m, Phase::Failed, 0, false);
    assert_ne!(m.id(), old);
    assert!(m.confirm(&s, &old).is_err());
    m.review(&s).unwrap();
    confirm(&mut m, &s);
    phase(&m, Phase::Pending, 1, true);
}
#[test]
fn eight_admitted_reloads_are_the_per_controller_cap() {
    let mut s = state();
    let mut m = McpReload::default();
    for n in 1..=8 {
        m.review(&s).unwrap();
        confirm(&mut m, &s);
        m.result(&s, reply("unchanged")).unwrap();
        reattach(&mut m, &mut s, snapshot());
        phase(&m, Phase::Reconciled, n, false);
    }
    assert!(m.review(&s).is_err() && m.confirm(&s, "").is_err());
}
#[test]
fn parser_retains_only_public_names_and_exact_outcomes_not_private_acknowledgements() {
    for (name, expected) in [
        ("applied", SessionOutcome::Applied),
        ("unchanged", SessionOutcome::Unchanged),
        ("busy", SessionOutcome::Busy),
    ] {
        let v = json!({"status":"reloaded","session":name,"unavailable":["one"],"session_servers":["two"],"loaded_rev":"012345abcdef","message":"PRIVATE_DIAGNOSTIC","host_ack":{"secret":"PRIVATE_DIAGNOSTIC"},"private_config":"PRIVATE_DIAGNOSTIC"});
        let r = Report::parse(v).unwrap();
        assert!(!format!("{r:?}").contains("PRIVATE_DIAGNOSTIC"));
        assert_eq!(r.session, expected);
        assert_eq!(r.unavailable, ["one"]);
        assert_eq!(r.session_servers.unwrap(), ["two"]);
    }
    let r = Report::parse(json!({"status":"reloaded","session":"busy","unavailable":null,"session_servers":null,"loaded_rev":null,"message":null})).unwrap();
    assert!(r.unavailable.is_empty() && r.session_servers.is_none());
}
#[test]
fn parser_is_strict_about_all_known_field_types() {
    rejects(
        reply("applied"),
        json!({"status":["ok",null],"session":["deferred",false,null],"unavailable":[{}],"session_servers":[false],"message":[7],"loaded_rev":["ABCDEF012345","abcdef01234",3],"coalesced":[0,"false",[]],"turn_isolation":[0,"false",[]]}),
        |v| Report::parse(v).is_err(),
    );
    let invalid = json!([null,[],{"status":"reloaded"},{"session":"busy"}]);
    assert!(
        invalid
            .as_array()
            .unwrap()
            .iter()
            .all(|v| Report::parse(v.clone()).is_err())
    );
    let valid =
        json!({"status":"reloaded","session":"busy","coalesced":false,"turn_isolation":true});
    assert!(Report::parse(valid).is_ok());
}
#[test]
fn optional_acknowledgement_flags_accept_explicit_null_from_pinned_wire_contract() {
    for key in ["coalesced", "turn_isolation"] {
        for flag in [Value::Null, json!(false), json!(true)] {
            let report = Report::parse(change(reply("unchanged"), key, flag));
            assert!(
                report.is_ok(),
                "nullable boolean {key} is valid owner wire data"
            );
        }
    }
}
#[test]
fn parser_bounds_each_array_and_raw_message_and_rejects_unsafe_duplicate_names() {
    for key in ["unavailable", "session_servers"] {
        let variants = json!([
            ["same", "same"],
            [""],
            [" "],
            ["x\u{001b}"],
            ["x\u{202e}"],
            [7]
        ]);
        for names in variants.as_array().unwrap() {
            assert!(Report::parse(change(reply("busy"), key, names.clone())).is_err());
        }
        assert!(Report::parse(change(reply("busy"), key, json!(["é".repeat(129)]))).is_err());
        for count in [128, 129] {
            let names: Vec<_> = (0..count)
                .map(|n| format!("{n:03}{}", "x".repeat(253)))
                .collect();
            assert_eq!(
                Report::parse(change(reply("busy"), key, json!(names))).is_ok(),
                count == 128
            );
        }
    }
    for size in [16_384, 16_385] {
        let valid =
            Report::parse(change(reply("busy"), "message", json!("x".repeat(size)))).is_ok();
        assert_eq!(valid, size == 16_384);
    }
}
#[test]
fn complete_escaped_projection_is_bounded_without_charging_private_data() {
    let names: Vec<_> = (0..128)
        .map(|n| format!("{n:03}{}", "\\".repeat(253)))
        .collect();
    let v = json!({"status":"reloaded","session":"applied","unavailable":names,"session_servers":names});
    assert!(Report::parse(v).is_err());
    let r = Report::parse(change(reply("busy"), "private", json!("z".repeat(70_000)))).unwrap();
    assert!(!format!("{r:?}").contains("zzzz"));
}
#[test]
fn result_is_not_readback_and_other_control_revision_generation_may_advance() {
    let mut s = state();
    let mut m = setup(&s, Phase::Pending);
    s.revision += 2;
    s.generation += 2;
    m.result(&s, reply("applied")).unwrap();
    phase(&m, Phase::Reconciling, 1, true);
    assert!(m.report().is_some() && m.review(&s).is_err());
    assert!(m.result(&s, reply("applied")).is_err());
    let v = json!({"session_id":"s","subscription_id":"sub","authority_epoch":3,"revision":s.revision,"execution_generation":s.generation,"replay_epoch":"epoch","last_sequence":9,"running":false,"messages":[],"pending":[],"prompts":[],"info":{"profile_id":"p","yolo":false}});
    reattach(&mut m, &mut s, v);
    phase(&m, Phase::Reconciled, 1, false);
}
#[test]
fn admitted_result_wrong_owner_or_malformed_reply_is_unknown() {
    for n in [0, 1, 2, 3, 11, 15] {
        let mut s = state();
        let mut m = setup(&s, Phase::Pending);
        let mut v = Value::Null;
        if n != 15 {
            edit(&mut s, n);
            v = reply("busy");
        }
        assert!(m.result(&s, v).is_err() && m.report().is_none());
        phase(&m, Phase::Unknown, 1, true);
        assert!(m.notice.contains("outcome unknown"));
    }
}
#[test]
fn readback_checks_exact_owner_authority_profile_yolo_and_numeric_floors() {
    let s = state();
    let m = reconciling(&s, "applied");
    let valid = change(snapshot(), "execution_generation", json!(5));
    m.validate_readback(&s, &valid).unwrap();
    rejects(
        valid.clone(),
        json!({"session_id":["other"],"authority_epoch":[4,"3"],"revision":[6,null,"7"],"execution_generation":[4,5.0],"last_sequence":[8,null],"replay_epoch":[null],"subscription_id":[null],"info":[{"profile_id":"other","yolo":false},{"profile_id":"p","yolo":true},{"profile_id":"p"}]}),
        |v| m.validate_readback(&s, &v).is_err(),
    );
    for n in 0..4 {
        let mut current = state();
        edit(&mut current, n);
        assert!(m.validate_readback(&current, &valid).is_err());
    }
}
#[test]
fn readback_floors_each_current_fence_but_new_replay_may_restart_sequence() {
    for n in [5, 6, 8] {
        let mut s = state();
        let mut m = reconciling(&s, "unchanged");
        edit(&mut s, n);
        assert!(m.validate_readback(&s, &snapshot()).is_err());
        let v = json!({"session_id":"s","subscription_id":"fresh","authority_epoch":3,"revision":s.revision,"execution_generation":s.generation,"replay_epoch":"new","last_sequence":0,"running":false,"messages":[],"pending":[],"prompts":[],"info":{"profile_id":"p","yolo":false}});
        reattach(&mut m, &mut s, v);
    }
}
#[test]
fn applied_requires_generation_increment_unchanged_busy_do_not() {
    for outcome in ["applied", "unchanged", "busy"] {
        let mut s = state();
        let mut m = reconciling(&s, outcome);
        let accepted = m.validate_readback(&s, &snapshot()).is_ok();
        assert_eq!(accepted, outcome != "applied");
        let v = change(snapshot(), "execution_generation", json!(5));
        reattach(&mut m, &mut s, v);
        assert!(m.notice.contains("MCP reattach readback complete") && m.notice.contains("health"));
        assert!(outcome != "busy" || m.notice.contains("busy"));
    }
    let mut s = state();
    s.generation = u64::MAX;
    let v = change(snapshot(), "execution_generation", json!(u64::MAX));
    assert!(
        reconciling(&s, "applied")
            .validate_readback(&s, &v)
            .is_err()
    );
}
#[test]
fn disconnect_unknown_keep_reports_and_never_reenable_review() {
    let s = state();
    let mut m = setup(&s, Phase::Pending);
    m.disconnected();
    m.not_enqueued();
    m.cancel_review();
    m.reconciled();
    phase(&m, Phase::Unknown, 1, true);
    assert!(m.review(&s).is_err());
    let mut m = setup(&s, Phase::Pending);
    let v = change(reply("busy"), "unavailable", json!(["partial"]));
    m.result(&s, v).unwrap();
    m.unknown();
    m.cancel_review();
    assert_eq!(m.report().unwrap().unavailable, ["partial"]);
    assert!(m.review(&s).is_err());
    let mut m = setup(&s, Phase::Review);
    m.disconnected();
    phase(&m, Phase::Failed, 0, false);
}
#[test]
fn partial_notice_is_distinct_and_nonpending_cannot_settle() {
    let s = state();
    let mut m = McpReload::default();
    m.reconciled();
    phase(&m, Phase::Empty, 0, false);
    assert!(m.confirm(&s, "").is_err() && m.validate_readback(&s, &snapshot()).is_err());
    assert!(m.result(&s, reply("busy")).is_err());
    let mut full = reconciling(&s, "busy");
    full.reconciled();
    m = setup(&s, Phase::Pending);
    let v = change(reply("busy"), "unavailable", json!(["missing"]));
    m.result(&s, v).unwrap();
    m.reconciled();
    assert_ne!(m.notice, full.notice);
    assert!(m.notice.contains("busy") && m.notice.contains("unavailable"));
}
