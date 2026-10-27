use hermes_tern_gateway::{protocol::parse_frame, session::SessionState};
use serde_json::{Value, json};

fn state(rows: Value) -> SessionState {
    SessionState::from_snapshot(
        "p",
        "i",
        json!({
            "session_id":"s","subscription_id":"sub","authority_epoch":1,"revision":1,
            "execution_generation":2,"replay_epoch":"e","last_sequence":0,"running":false,
            "messages":rows,"pending":[],"prompts":[],"info":{}
        }),
    )
    .unwrap()
}
fn event(s: &mut SessionState, seq: u64, kind: &str, payload: Value) {
    s.apply(
        parse_frame(
            &json!({"jsonrpc":"2.0","method":"event","params":{
                "session_id":"s","replay_epoch":"e","seq":seq,"authority_epoch":1,
                "execution_generation":2,"type":kind,"payload":payload
            }})
            .to_string(),
        )
        .unwrap(),
    )
    .unwrap();
}
#[test]
fn owner_stream_reasoning_is_separate_from_answer_and_available_is_not_duplicated() {
    let mut s = state(json!([]));
    event(&mut s, 1, "reasoning.delta", json!({"text":"Inspect "}));
    let control = s.messages[0].control_id.clone();
    event(&mut s, 2, "reasoning.delta", json!({"text":"the parser."}));
    assert_eq!(
        s.messages[0].control_id, control,
        "stream append replaced row identity"
    );
    event(
        &mut s,
        3,
        "reasoning.available",
        json!({"text":"fallback must not duplicate"}),
    );
    event(&mut s, 4, "message.delta", json!({"text":"Answer"}));
    event(&mut s, 5, "message.complete", json!({"text":"Answer"}));
    assert_eq!(s.messages.len(), 2);
    assert_eq!(s.messages[0].role, "reasoning");
    assert_eq!(s.messages[0].text, "Inspect the parser.");
    assert_eq!(s.messages[1].role, "assistant");
    assert_eq!(s.messages[1].text, "Answer");
    assert!(!s.running);
}
#[test]
fn resume_projects_assistant_reasoning_and_keeps_reasoning_only_rows() {
    let s = state(json!([
        {"row_id":1,"role":"user","text":"Hi","reasoning":"never user reasoning"},
        {"row_id":2,"role":"assistant","text":"Hello","reasoning_content":"Check first."},
        {"row_id":3,"role":"assistant","text":"","reasoning":"Only visible detail."}
    ]));
    assert_eq!(s.messages.len(), 4);
    assert_eq!(s.messages[0].role, "user");
    assert_eq!(s.messages[1].role, "reasoning");
    assert_eq!(s.messages[1].text, "Check first.");
    assert_eq!(s.messages[2].text, "Hello");
    assert_eq!(s.messages[3].text, "Only visible detail.");
    let ids: std::collections::HashSet<_> = s.messages.iter().map(|m| &m.id).collect();
    assert_eq!(ids.len(), s.messages.len());
}
#[test]
fn reasoning_after_tool_stays_in_order_and_does_not_merge_across_tool_boundary() {
    let mut s = state(json!([]));
    event(&mut s, 1, "reasoning.delta", json!({"text":"Before tool"}));
    event(
        &mut s,
        2,
        "tool.start",
        json!({"tool_id":"t","tool":"read_file"}),
    );
    event(&mut s, 3, "reasoning.delta", json!({"text":"After tool"}));
    event(&mut s, 4, "message.complete", json!({"text":"Done"}));
    assert_eq!(
        s.messages
            .iter()
            .map(|m| m.role.as_str())
            .collect::<Vec<_>>(),
        ["reasoning", "tool", "reasoning", "assistant"]
    );
    assert_eq!(s.messages[0].text, "Before tool");
    assert_eq!(s.messages[2].text, "After tool");
}

#[test]
fn fallback_reasoning_after_tools_is_retained_but_same_response_fallback_is_deduplicated() {
    let mut s = state(json!([]));
    event(
        &mut s,
        1,
        "reasoning.available",
        json!({"text":"Before tool"}),
    );
    event(&mut s, 2, "message.delta", json!({"text":"Commentary"}));
    event(
        &mut s,
        3,
        "reasoning.available",
        json!({"text":"same response duplicate"}),
    );
    event(
        &mut s,
        4,
        "tool.start",
        json!({"tool_id":"t","name":"read_file"}),
    );
    event(
        &mut s,
        5,
        "tool.complete",
        json!({"tool_id":"t","name":"read_file"}),
    );
    event(
        &mut s,
        6,
        "reasoning.available",
        json!({"text":"After tool"}),
    );
    event(
        &mut s,
        7,
        "reasoning.available",
        json!({"text":"second duplicate"}),
    );
    assert_eq!(
        s.messages
            .iter()
            .filter(|m| m.role == "reasoning")
            .map(|m| m.text.as_str())
            .collect::<Vec<_>>(),
        ["Before tool", "After tool"]
    );
    event(&mut s, 8, "reasoning.delta", json!({"text":" continuing"}));
    assert_eq!(s.messages.back().unwrap().text, "After tool continuing");
}
#[test]
fn reasoning_retention_is_bounded_sanitized_and_deltas_do_not_bridge_a_truncated_gap() {
    let mut s = state(json!([]));
    event(
        &mut s,
        1,
        "reasoning.available",
        json!({"text":format!("\u{1b}[31m{}\u{202e}", "😀".repeat(20000))}),
    );
    let row = s.messages.front().expect("reasoning retained");
    assert_eq!(row.role, "reasoning");
    assert!(row.truncated && row.text.len() <= 65536);
    assert!(!row.text.contains('\u{1b}') && !row.text.contains('\u{202e}'));
    let prefix = row.text.clone();
    event(&mut s, 2, "reasoning.delta", json!({"text":"not adjacent"}));
    assert_eq!(s.messages.front().unwrap().text, prefix);
    assert!(s.display_truncated);
}
#[test]
fn unscoped_reasoning_cannot_advance_generation_or_invalidate_prompt_state() {
    let mut s = state(json!([]));
    let actions = s
        .apply(
            parse_frame(
                &json!({"jsonrpc":"2.0","method":"event","params":{
                    "session_id":"s","type":"reasoning.delta","replay_epoch":"e","seq":1,
                    "execution_generation":999,"payload":{"text":"unfenced"}
                }})
                .to_string(),
            )
            .unwrap(),
        )
        .unwrap();
    assert!(actions.is_empty());
    assert_eq!(s.generation, 2);
    assert!(s.messages.is_empty());
    assert_eq!(s.last_sequence, 1);
}

#[test]
fn whitespace_only_reasoning_delta_is_not_discarded_between_words() {
    let mut s = state(json!([]));
    event(&mut s, 1, "reasoning.delta", json!({"text":"Check"}));
    event(&mut s, 2, "reasoning.delta", json!({"text":" "}));
    event(&mut s, 3, "reasoning.delta", json!({"text":"first."}));
    assert_eq!(s.messages[0].text, "Check first.");
}

#[test]
fn history_extracts_only_readable_reasoning_details_without_opaque_replay_fields() {
    for details in [
        json!([{"type":"reasoning.summary","summary":"Check the inputs.","signature":"PRIVATE"},
               {"type":"redacted_thinking","data":"PRIVATE"},
               {"type":"reasoning.text","text":"Return the result."}]),
        json!(
            r#"[{"type":"thinking","thinking":"Check the inputs.","signature":"PRIVATE"},{"type":"reasoning.text","text":"Return the result."}]"#
        ),
    ] {
        let s = state(
            json!([{"row_id":1,"role":"assistant","text":"Answer.","reasoning_details":details}]),
        );
        assert_eq!(s.messages.len(), 2);
        assert_eq!(
            s.messages[0].text,
            "Check the inputs.\n\nReturn the result."
        );
        assert!(!s.messages.iter().any(|m| m.text.contains("PRIVATE")));
    }
    for details in [
        json!([{"type":"thinking","signature":"PRIVATE"}]),
        json!("[{\"type\":\"thinking\""),
        json!({"type":"native_assistant","messages":[{"content":[{"type":"text","text":"PRIVATE replay answer"}]}]}),
    ] {
        let s = state(
            json!([{"row_id":1,"role":"assistant","text":"Answer.","reasoning_details":details}]),
        );
        assert_eq!(s.messages.len(), 1);
        assert_eq!(s.messages[0].text, "Answer.");
    }
}
