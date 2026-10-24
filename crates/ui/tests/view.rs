use hermes_tern_gateway::{protocol::parse_frame, session::SessionState};
use hermes_tern_ui::{
    composer::Composer,
    view::{UiAction, render},
};
use serde_json::json;
fn state() -> SessionState {
    SessionState::from_snapshot("p","i",json!({"session_id":"s","subscription_id":"sub","authority_epoch":1,"revision":1,"execution_generation":0,"replay_epoch":"e","last_sequence":0,"running":false,"messages":[],"pending":[],"prompts":[],"info":{}})).unwrap()
}
#[test]
fn native_editor_uses_utf16_and_never_sends_when_disconnected() {
    let mut s = state();
    let mut c = Composer::default();
    c.insert("a😀").unwrap();
    let v = render(&s, &c, None);
    let ed = &v.regions()[1].as_ref().unwrap().child_nodes()[0];
    assert_eq!(ed.props()["cursor"], 3);
    assert_eq!(ed.props()["sendable"], true);
    s.mark_disconnected();
    let v = render(&s, &c, None);
    let ed = &v.regions()[1].as_ref().unwrap().child_nodes()[0];
    assert_eq!(ed.props()["sendable"], false);
    assert_eq!(ed.props()["text"], "a😀");
}
#[test]
fn native_controls_carry_exact_prompt_and_generation() {
    let mut s = state();
    let c = Composer::default();
    s.apply(parse_frame(r#"{"jsonrpc":"2.0","method":"event","params":{"session_id":"s","type":"approval.request","replay_epoch":"e","seq":1,"execution_generation":4,"payload":{"kind":"approval","prompt_id":"pr","execution_generation":4,"choices":["once","deny"],"command":"test"}}}"#).unwrap()).unwrap();
    let actions = hermes_tern_ui::view::prompt_actions(&s);
    assert!(actions.iter().any(|a|matches!(a,UiAction::Approve{id,generation,choice,..} if id=="pr"&&*generation==4&&choice=="deny")));
    assert!(
        !actions
            .iter()
            .any(|a| matches!(a,UiAction::Approve{choice,..} if choice=="always"))
    );
    let _ = render(&s, &c, None);
}

#[test]
fn queued_native_prompt_actions_are_fenced_by_the_rendered_prompt_instance() {
    for kind in ["approval", "clarify"] {
        let mut s = state();
        let prompt = |seq, generation| {
            parse_frame(&json!({"jsonrpc":"2.0","method":"event","params":{
                "session_id":"s","type":format!("{kind}.request"),"replay_epoch":"e",
                "seq":seq,"execution_generation":generation,"payload":{"kind":kind,"prompt_id":"pr",
                "execution_generation":generation,"choices":["once","deny"],"command":"test","question":"answer"}
            }}).to_string()).unwrap()
        };
        s.apply(prompt(1, 4)).unwrap();
        let old = hermes_tern_ui::view::prompt_actions(&s);
        assert_eq!(old.len(), 2);
        for action in &old {
            action.validate_control(&s).unwrap();
        }
        // An already decoded action must not address a replacement prompt,
        // even with identical upstream ID, generation and displayed content.
        s.apply(prompt(2, 4)).unwrap();
        for action in &old {
            assert!(action.validate_control(&s).is_err());
        }
        let current = hermes_tern_ui::view::prompt_actions(&s);
        for action in &current {
            action.validate_control(&s).unwrap();
        }
        s.apply(prompt(3, 5)).unwrap();
        for action in &current {
            assert!(action.validate_control(&s).is_err());
        }
        assert!(!s.prompts["pr"].answered);
    }
}

#[test]
fn prompt_node_identity_is_stable_for_repaints_but_changes_on_replacement() {
    let mut s = state();
    let prompt = |seq| {
        parse_frame(
            &json!({"jsonrpc":"2.0","method":"event","params":{
                "session_id":"s","type":"approval.request","replay_epoch":"e","seq":seq,
                "execution_generation":4,"payload":{"kind":"approval","prompt_id":"pr",
                "execution_generation":4,"choices":["once","deny"],"command":"test"}
            }})
            .to_string(),
        )
        .unwrap()
    };
    s.apply(prompt(1)).unwrap();
    let key = |s: &SessionState| {
        let v = render(s, &Composer::default(), None);
        let children = v.regions()[0].as_ref().unwrap().child_nodes();
        children.last().unwrap().props()["key"].clone()
    };
    let before = key(&s);
    assert_eq!(before, key(&s));
    s.apply(prompt(2)).unwrap();
    assert_ne!(
        before,
        key(&s),
        "replacement reused an actionable node identity"
    );
}

#[test]
fn decoded_interrupt_control_cannot_target_a_later_generation() {
    let mut s = state();
    s.running = true;
    let old = UiAction::InterruptTurn {
        generation: s.generation,
    };
    old.validate_control(&s).unwrap();
    s.generation += 1;
    assert!(old.validate_control(&s).is_err());
    UiAction::InterruptTurn {
        generation: s.generation,
    }
    .validate_control(&s)
    .unwrap();
    s.mark_disconnected();
    assert!(
        UiAction::InterruptTurn {
            generation: s.generation
        }
        .validate_control(&s)
        .is_err()
    );
}
