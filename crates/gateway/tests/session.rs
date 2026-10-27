use hermes_tern_gateway::{
    protocol::{Inbound, parse_frame},
    session::{Action, SessionState},
};
use serde_json::{Value, json};
fn snapshot() -> Value {
    json!({"session_id":"s","subscription_id":"sub","authority_epoch":1,"revision":1,"execution_generation":0,"replay_epoch":"e","last_sequence":0,"running":false,"messages":[],"pending":[],"prompts":[],"info":{}})
}
fn state() -> SessionState {
    SessionState::from_snapshot("p", "i", snapshot()).unwrap()
}
fn event(kind: &str, seq: u64, generation: u64, payload: Value) -> Inbound {
    parse_frame(&json!({"jsonrpc":"2.0","method":"event","params":{"type":kind,"session_id":"s","replay_epoch":"e","seq":seq,"execution_generation":generation,"authority_epoch":1,"admission_id":"a","payload":payload}}).to_string()).unwrap()
}
fn receipt(status: &str) -> Value {
    json!({"admission_id":"a","ref":{"session_id":"s","profile_id":"p"},"sequence":1,"status":status,"execution_generation":1,"authority_epoch":1,"outcome":if status=="terminal" {Some("complete")} else {None}})
}
#[test]
fn disconnect_never_resubmits_an_ambiguous_prompt() {
    let mut s = state();
    s.submit("hello", "input-1").unwrap();
    let a = s.apply(Inbound::Disconnected).unwrap();
    assert!(a.iter().all(|a| !matches!(a, Action::Submit { .. })));
    assert!(s.outcome_unknown());
    assert!(!s.can_submit());
}
#[test]
fn snapshot_never_regresses_newer_owner_epoch_route_generation_or_sequence() {
    for field in [
        "authority_epoch",
        "revision",
        "execution_generation",
        "last_sequence",
    ] {
        let mut s = state();
        s.apply(event(
            "session.info",
            1,
            2,
            json!({"revision":3,"model":"newest"}),
        ))
        .unwrap();
        let mut fresh = snapshot();
        fresh["revision"] = json!(3);
        fresh["execution_generation"] = json!(2);
        fresh["last_sequence"] = json!(1);
        fresh[field] = json!(0);
        assert!(s.reconcile(fresh).is_err(), "accepted regressed {field}");
        assert_eq!(s.info["model"], "newest");
        assert_eq!(
            (s.authority_epoch, s.revision, s.generation, s.last_sequence),
            (1, 3, 2, 1)
        );
    }
}
#[test]
fn streamed_events_before_admission_do_not_regress_generation_or_complete_early() {
    let mut s = state();
    s.submit("hello", "input-1").unwrap();
    s.apply(event("message.start", 1, 1, json!({}))).unwrap();
    s.apply(event("message.delta", 2, 1, json!({"text":"hello"})))
        .unwrap();
    let mut queued = receipt("queued");
    queued["execution_generation"] = Value::Null;
    s.accept_receipt(queued).unwrap();
    assert_eq!(s.generation, 1);
    s.apply(event(
        "message.complete",
        3,
        1,
        json!({"text":"hello","status":"complete"}),
    ))
    .unwrap();
    assert!(!s.can_submit());
    s.accept_receipt(receipt("terminal")).unwrap();
    assert!(s.can_submit());
    assert_eq!(
        s.messages.iter().filter(|m| m.role == "assistant").count(),
        1
    );
}
#[test]
fn duplicate_and_foreign_events_do_not_duplicate_text() {
    let mut s = state();
    s.apply(event("message.delta", 1, 1, json!({"text":"A"})))
        .unwrap();
    s.apply(event("message.delta", 1, 1, json!({"text":"A"})))
        .unwrap();
    let mut foreign = json!({"jsonrpc":"2.0","method":"event","params":{"type":"message.delta","session_id":"other","replay_epoch":"e","seq":2,"payload":{"text":"X"}}});
    s.apply(parse_frame(&foreign.to_string()).unwrap()).unwrap();
    foreign["params"]["session_id"] = json!("s");
    assert_eq!(s.messages.back().unwrap().text, "A");
}
#[test]
fn gaps_suspend_controls_and_request_only_one_snapshot() {
    let mut s = state();
    let a = s
        .apply(event("message.delta", 3, 1, json!({"text":"lost"})))
        .unwrap();
    assert!(matches!(a.as_slice(), [Action::Snapshot]));
    assert!(
        s.apply(event("message.delta", 4, 1, json!({"text":"lost"})))
            .unwrap()
            .is_empty()
    );
    assert!(!s.can_submit());
    assert!(s.interrupt().is_err());
}
#[test]
fn approvals_require_matching_live_id_generation_and_choice() {
    let mut s = state();
    s.apply(event("approval.request",1,1,json!({"kind":"approval","prompt_id":"pr","execution_generation":1,"choices":["once","deny"],"command":"danger"}))).unwrap();
    assert!(s.answer_approval("pr", 0, "once").is_err());
    assert!(s.answer_approval("pr", 1, "always").is_err());
    assert!(matches!(
        s.answer_approval("pr", 1, "deny").unwrap(),
        Action::Approval { .. }
    ));
    assert!(s.answer_approval("pr", 1, "once").is_err());
    s.apply(event(
        "approval.settled",
        2,
        1,
        json!({"prompt_id":"pr","execution_generation":1}),
    ))
    .unwrap();
    assert!(s.prompts.is_empty());
}
#[test]
fn unknown_sensitive_requests_are_declined_not_collected() {
    let mut s = state();
    let a=s.apply(parse_frame(r#"{"jsonrpc":"2.0","id":"srq-x","method":"sudo","params":{"session_id":"s","command":"sudo true"}}"#).unwrap()).unwrap();
    assert!(
        matches!(a.as_slice(),[Action::ServerReply{id, result}] if id=="srq-x" && result["value"]=="")
    );
    assert!(s.prompts.is_empty());
}
#[test]
fn foreign_receipt_and_invalid_snapshot_fail_closed() {
    let mut s = state();
    let mut r = receipt("terminal");
    r["ref"]["session_id"] = json!("other");
    assert!(s.accept_receipt(r).is_err());
    let mut v = snapshot();
    v["execution_generation"] = json!(-1);
    assert!(SessionState::from_snapshot("p", "i", v).is_err());
}
#[test]
fn bounded_display_retention_is_explicit() {
    let mut s = state();
    s.apply(event(
        "message.delta",
        1,
        1,
        json!({"text":"x".repeat(100_000)}),
    ))
    .unwrap();
    assert!(s.messages.back().unwrap().text.len() <= 65536);
    assert!(s.display_truncated);
}

#[test]
fn retained_prefix_does_not_keep_the_full_oversize_allocation() {
    let mut s = state();
    s.apply(event(
        "message.delta",
        1,
        1,
        json!({"text":"x".repeat(4*1024*1024)}),
    ))
    .unwrap();
    let row = s.messages.back().unwrap();
    assert!(row.truncated);
    assert!(
        row.text.capacity() <= 65536,
        "retained prefix keeps discarded allocation"
    );
    s.apply(event(
        "message.complete",
        2,
        1,
        json!({"text":"short complete"}),
    ))
    .unwrap();
    assert!(!s.messages.back().unwrap().truncated);
}

#[test]
fn stream_growth_and_tool_replacement_enforce_total_display_budget() {
    for tool in [false, true] {
        let mut v = snapshot();
        let rows: Vec<_> = (0..31)
            .map(|i| json!({"row_id":i,"role":"assistant","text":"x".repeat(65536)}))
            .collect();
        v["messages"] = json!(rows);
        let mut s = SessionState::from_snapshot("p", "i", v).unwrap();
        if tool {
            s.apply(event(
                "tool.start",
                1,
                1,
                json!({"tool_id":"one","name":"same-name","summary":"small"}),
            ))
            .unwrap();
        } else {
            s.apply(event("message.delta", 1, 1, json!({"text":"small"})))
                .unwrap();
        }
        s.apply(event(
            "tool.start",
            2,
            1,
            json!({"tool_id":"two","name":"same-name","summary":"small"}),
        ))
        .unwrap();
        let (kind, payload) = if tool {
            (
                "tool.complete",
                json!({"tool_id":"one","name":"same-name","result_text":"z".repeat(65536)}),
            )
        } else {
            ("message.delta", json!({"text":"z".repeat(65536)}))
        };
        s.apply(event(kind, 3, 1, payload)).unwrap();
        assert!(
            s.messages.iter().map(|m| m.text.len()).sum::<usize>() <= 2 * 1024 * 1024,
            "in-place update bypassed total display budget: tool={tool}"
        );
        assert!(s.display_truncated);
        assert!(!s.messages.iter().any(|m| m.id == "row-0"));
        assert!(s.messages.iter().any(|m| m.id == "tool-1-two"));
    }
}

#[test]
fn clipped_stream_prefix_never_splices_later_deltas_across_missing_bytes() {
    let mut s = state();
    s.apply(event(
        "message.delta",
        1,
        1,
        json!({"text":format!("{}😀", "x".repeat(65535))}),
    ))
    .unwrap();
    let prefix = s.messages.back().unwrap().text.clone();
    assert_eq!(prefix.len(), 65535);
    s.apply(event("message.delta", 2, 1, json!({"text":"Z"})))
        .unwrap();
    assert!(
        s.messages.back().unwrap().text == prefix,
        "a later byte was joined across a missing Unicode scalar"
    );
    s.apply(event(
        "message.complete",
        3,
        1,
        json!({"text":"complete replacement"}),
    ))
    .unwrap();
    assert_eq!(s.messages.back().unwrap().text, "complete replacement");
}

#[test]
fn evicted_stream_does_not_reappear_as_a_complete_looking_suffix() {
    let mut s = state();
    s.apply(event(
        "message.delta",
        1,
        1,
        json!({"text":"original prefix"}),
    ))
    .unwrap();
    for seq in 2..=257 {
        s.apply(event(
            "tool.start",
            seq,
            1,
            json!({"tool_id":seq.to_string(),"name":"tool"}),
        ))
        .unwrap();
    }
    assert_eq!(s.messages.len(), 256);
    assert!(!s.messages.iter().any(|m| m.id == "reply-a-0"));
    s.apply(event(
        "message.delta",
        258,
        1,
        json!({"text":"misleading suffix"}),
    ))
    .unwrap();
    assert!(
        !s.messages.iter().any(|m| m.id == "reply-a-0"),
        "evicted prefix was silently replaced by a suffix"
    );
    s.apply(event(
        "message.complete",
        259,
        1,
        json!({"text":"complete replacement"}),
    ))
    .unwrap();
    assert_eq!(s.messages.back().unwrap().text, "complete replacement");
    assert_eq!(s.messages.len(), 256);
}

#[test]
fn terminal_receipt_recovers_partial_or_clipped_or_evicted_stream_without_duplicates() {
    for kind in ["partial", "clipped", "evicted"] {
        let mut s = state();
        s.submit("hello", "input-1").unwrap();
        s.accept_receipt(receipt("started")).unwrap();
        let body = if kind == "partial" {
            "partial".into()
        } else {
            "x".repeat(70000)
        };
        s.apply(event("message.delta", 1, 1, json!({"text":body})))
            .unwrap();
        if kind == "evicted" {
            for seq in 2..=257 {
                s.apply(event(
                    "tool.start",
                    seq,
                    1,
                    json!({"tool_id":seq.to_string(),"name":"tool"}),
                ))
                .unwrap();
            }
        }
        let mut final_receipt = receipt("terminal");
        final_receipt["result"] = json!({"final_response":"authoritative complete answer"});
        s.accept_receipt(final_receipt.clone()).unwrap();
        s.accept_receipt(final_receipt).unwrap();
        let rows: Vec<_> = s.messages.iter().filter(|m| m.id == "reply-a-0").collect();
        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0].text, "authoritative complete answer",
            "receipt did not recover {kind} stream"
        );
        assert!(!rows[0].truncated);
        assert!(s.messages.len() <= 256);
        assert!(s.messages.iter().map(|m| m.text.len()).sum::<usize>() <= 2 * 1024 * 1024);
    }
}

#[test]
fn receipt_does_not_overwrite_observed_completion_or_later_generation() {
    for closed in ["complete", "reused", "new-generation"] {
        let mut s = state();
        s.submit("hello", "input-1").unwrap();
        s.accept_receipt(receipt("started")).unwrap();
        s.apply(event("message.delta", 1, 1, json!({"text":"streamed"})))
            .unwrap();
        if closed == "new-generation" {
            let later = parse_frame(&json!({"jsonrpc":"2.0","method":"event","params":{
                "type":"message.delta","session_id":"s","replay_epoch":"e","seq":2,
                "execution_generation":2,"authority_epoch":1,"admission_id":"b","payload":{"text":"later"}
            }}).to_string()).unwrap();
            s.apply(later).unwrap();
        } else {
            s.apply(event(
                "message.complete",
                2,
                1,
                json!({"text":"live final","response_reused":closed=="reused"}),
            ))
            .unwrap();
        }
        let before: Vec<_> = s
            .messages
            .iter()
            .map(|m| (m.id.clone(), m.text.clone()))
            .collect();
        let mut r = receipt("terminal");
        r["result"] = json!({"final_response":"stored representation"});
        s.accept_receipt(r).unwrap();
        assert_eq!(
            s.messages
                .iter()
                .map(|m| (m.id.clone(), m.text.clone()))
                .collect::<Vec<_>>(),
            before
        );
    }
}

#[test]
fn next_admission_preserves_previous_reply_when_generation_advances_by_receipt_or_info() {
    for advance in ["event", "receipt", "info"] {
        let mut s = state();
        s.apply(event("message.delta", 1, 1, json!({"text":"first"})))
            .unwrap();
        s.apply(event(
            "message.complete",
            2,
            1,
            json!({"text":"first final"}),
        ))
        .unwrap();
        let mut seq = 3;
        if advance == "receipt" {
            let mut r = receipt("started");
            r["admission_id"] = json!("b");
            r["execution_generation"] = json!(2);
            s.accept_receipt(r).unwrap();
        } else if advance == "info" {
            s.apply(event(
                "session.info",
                seq,
                1,
                json!({"execution_generation":2,"running":true}),
            ))
            .unwrap();
            seq += 1;
        }
        for (kind, payload) in [
            ("message.start", json!({})),
            ("message.delta", json!({"text":"second"})),
            ("message.complete", json!({"text":"second final"})),
        ] {
            let f = json!({"jsonrpc":"2.0","method":"event","params":{
                "type":kind,"session_id":"s","replay_epoch":"e","seq":seq,
                "execution_generation":2,"authority_epoch":1,"admission_id":"b","payload":payload
            }});
            s.apply(parse_frame(&f.to_string()).unwrap()).unwrap();
            seq += 1;
        }
        let rows: Vec<_> = s
            .messages
            .iter()
            .map(|m| (m.id.as_str(), m.text.as_str()))
            .collect();
        assert_eq!(
            rows,
            vec![("reply-a-0", "first final"), ("reply-b-0", "second final")],
            "advance via {advance}"
        );
    }
}

#[test]
fn receipt_recovery_is_stable_when_late_stream_events_arrive() {
    let mut s = state();
    s.submit("hello", "input-1").unwrap();
    s.accept_receipt(receipt("started")).unwrap();
    let mut r = receipt("terminal");
    r["result"] = json!({"final_response":"stored final"});
    s.accept_receipt(r).unwrap();
    for (seq, kind, payload) in [
        (1, "message.start", json!({})),
        (2, "message.delta", json!({"text":"delayed partial"})),
        (3, "message.interim", json!({"text":"delayed interim"})),
    ] {
        s.apply(event(kind, seq, 1, payload)).unwrap();
    }
    assert!(!s.running);
    assert_eq!(s.messages.back().unwrap().text, "stored final");
    s.apply(event(
        "message.complete",
        4,
        1,
        json!({"text":"live final"}),
    ))
    .unwrap();
    assert_eq!(s.messages.back().unwrap().text, "live final");
    assert_eq!(
        s.messages.iter().filter(|m| m.role == "assistant").count(),
        1
    );
}

#[test]
fn resnapshot_retains_unknown_submission_and_checks_session_identity() {
    let mut s = state();
    s.submit("not acknowledged", "input-x").unwrap();
    s.apply(Inbound::Disconnected).unwrap();
    let mut wrong = snapshot();
    wrong["session_id"] = json!("other");
    assert!(s.reconcile(wrong).is_err());
    s.reconcile(snapshot()).unwrap();
    assert!(s.outcome_unknown());
    assert!(!s.can_submit());
}

#[test]
fn late_queued_receipt_cannot_reopen_terminal_admission() {
    let mut s = state();
    s.submit("hello", "x").unwrap();
    s.accept_receipt(receipt("terminal")).unwrap();
    s.accept_receipt(receipt("queued")).unwrap();
    assert!(s.admissions.is_empty());
    assert!(s.can_submit());
}

#[test]
fn batch_clarify_rejects_foreign_answers_and_withdraws_exact_card() {
    let mut s = state();
    s.apply(parse_frame(r#"{"jsonrpc":"2.0","id":"srq-c","method":"clarify","params":{"session_id":"s","questions":[{"qid":"q1","question":"Which?"}]}}"#).unwrap()).unwrap();
    assert!(s.answer_batch("srq-c", 0, json!({"forged":"yes"})).is_err());
    let action = s.answer_batch("srq-c", 0, json!({"q1":"yes"})).unwrap();
    assert!(matches!(action,Action::ServerReply{result,..} if result["answers"]["q1"]=="yes"));
    s.apply(event(
        "request.cancel",
        1,
        0,
        json!({"id":"srq-c","reason":"resolved"}),
    ))
    .unwrap();
    assert!(s.prompts.is_empty());
}
