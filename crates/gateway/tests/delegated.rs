use hermes_tern_gateway::{
    delegated::{Delegated, Phase},
    session::SessionState,
};
use serde_json::{Value, json};
fn state() -> SessionState {
    SessionState::from_snapshot("p", "i", json!({"session_id":"s","subscription_id":"sub","authority_epoch":1,"revision":1,"execution_generation":2,"replay_epoch":"e","last_sequence":0,"running":false,"messages":[],"pending":[],"prompts":[],"info":{"yolo":false}})).unwrap()
}
fn roster() -> Value {
    json!({"subagents":[{"subagent_id":"sa-1","parent_id":null,"goal":"review","depth":0,"model":"m","status":"running","started_at":1.5,"tool_count":2,"last_tool":"read_file","accepting_steer":true,"delegation_id":"batch"}],"delegations":[]})
}
#[test]
fn one_current_explicit_read_has_no_submit_or_control_authority() {
    let s = state();
    let mut d = Delegated::default();
    let token = d.loading(&s, 4).unwrap();
    assert_eq!(d.phase(), Phase::Loading);
    assert!(d.loading(&s, 4).is_err());
    d.loaded(&s, &token, roster()).unwrap();
    assert_eq!(d.phase(), Phase::Ready);
    assert_eq!(d.catalog(&s, 4).unwrap().rows.len(), 1);
    assert_eq!(d.catalog(&s, 4).unwrap().rows[0].id, "sa-1");
    assert!(d.catalog(&s, 5).is_none());
    assert!(s.can_submit());
}
#[test]
fn closed_or_reopened_panel_discards_pending_reply_without_enabling_second_pending_read() {
    let s = state();
    let mut d = Delegated::default();
    let token = d.loading(&s, 4).unwrap();
    d.panel_changed(5);
    assert!(d.loading(&s, 5).is_err());
    assert!(d.loaded(&s, &token, roster()).is_err());
    assert_eq!(d.phase(), Phase::Failed);
    assert!(d.catalog(&s, 5).is_none());
    assert!(d.loading(&s, 5).is_ok());
}
#[test]
fn every_runtime_fence_change_and_suspension_rejects_old_reply() {
    for field in [
        "profile",
        "instance",
        "session",
        "authority",
        "revision",
        "generation",
        "suspended",
        "disconnected",
    ] {
        let mut s = state();
        let mut d = Delegated::default();
        let token = d.loading(&s, 1).unwrap();
        match field {
            "profile" => s.profile = "different".into(),
            "instance" => s.instance = "different".into(),
            "session" => s.session_id = "different".into(),
            "authority" => s.authority_epoch += 1,
            "revision" => s.revision += 1,
            "generation" => s.generation += 1,
            "disconnected" => s.mark_disconnected(),
            "suspended" => {
                s.apply(hermes_tern_gateway::protocol::parse_frame(r#"{"jsonrpc":"2.0","method":"event","params":{"type":"session.replay_gap","session_id":"s","payload":{}}}"#).unwrap()).unwrap();
                assert!(s.connected && s.is_suspended());
            }
            _ => unreachable!(),
        }
        assert!(d.loaded(&s, &token, roster()).is_err(), "{field}");
        assert!(d.catalog(&s, 1).is_none(), "{field}");
    }
}
#[test]
fn obsolete_response_cannot_destroy_new_read_and_ready_inventory_is_invalidated_on_generation_change()
 {
    let mut s = state();
    let mut d = Delegated::default();
    let old = d.loading(&s, 1).unwrap();
    d.failed("unavailable");
    let new = d.loading(&s, 1).unwrap();
    assert!(d.loaded(&s, &old, roster()).is_err());
    assert_eq!(d.phase(), Phase::Loading);
    d.loaded(&s, &new, roster()).unwrap();
    assert!(d.catalog(&s, 1).is_some());
    s.generation += 1;
    assert!(d.catalog(&s, 1).is_none());
    d.reconcile(&s);
    assert_eq!(d.phase(), Phase::Failed);
    assert!(d.catalog(&s, 1).is_none());
}
#[test]
fn empty_success_differs_from_invalid_refused_unsent_and_disconnect_and_can_be_read_explicitly_again()
 {
    let s = state();
    let mut d = Delegated::default();
    let token = d.loading(&s, 1).unwrap();
    d.loaded(&s, &token, json!({"subagents":[],"delegations":[]}))
        .unwrap();
    assert!(d.catalog(&s, 1).unwrap().rows.is_empty());
    let token = d.loading(&s, 1).unwrap();
    assert!(d.loaded(&s, &token, json!({"subagents":null})).is_err());
    assert_eq!(d.phase(), Phase::Failed);
    assert!(d.catalog(&s, 1).is_none());
    d.loading(&s, 1).unwrap();
    d.failed("Delegated agents unavailable; reload explicitly");
    assert_eq!(d.phase(), Phase::Failed);
    let old = d.loading(&s, 1).unwrap();
    d.disconnected();
    assert_eq!(d.phase(), Phase::Failed);
    assert!(d.loaded(&s, &old, roster()).is_err());
    assert!(s.can_submit());
}

#[test]
fn actual_pending_capacity_refuses_unsent_read_without_disabling_send_then_explicit_read_succeeds()
{
    use hermes_tern_gateway::{
        bootstrap::ConnectionGrant,
        config::{Limits, SessionOptions},
        driver::Conversation,
        session::Action,
        transport::RpcClient,
    };
    use std::{
        net::TcpListener,
        sync::mpsc,
        thread,
        time::{Duration, Instant},
    };
    use tungstenite::{
        Message, accept_hdr,
        handshake::server::{Request, Response},
    };
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let (accepted_tx, accepted_rx) = mpsc::sync_channel(1);
    let (release_tx, release_rx) = mpsc::sync_channel(1);
    let server = thread::spawn(move || {
        let (socket, _) = listener.accept().unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        #[allow(clippy::result_large_err)]
        let mut ws = accept_hdr(socket, |_: &Request, mut response: Response| {
            response.headers_mut().insert(
                "Sec-WebSocket-Protocol",
                "hermes-gateway-v1".parse().unwrap(),
            );
            Ok(response)
        })
        .unwrap();
        let mut methods = Vec::new();
        while let Ok(frame) = ws.read() {
            if frame.is_close() {
                break;
            }
            let request: Value = serde_json::from_str(&frame.into_text().unwrap()).unwrap();
            let method = request["method"].as_str().unwrap();
            methods.push(method.to_owned());
            let result = match method {
                "ping" => json!({"pong":true}),
                "runtime.describe" => {
                    json!({"profile_id":"p","instance_id":"i","capabilities":["durable-admission-v1","event-replay-v1","local-cli-create-v1"],"session_create":{"sources":["tui"],"parameters":["request_id","source","cwd","yolo"]}})
                }
                "client.capabilities" => json!({}),
                "session.create" => {
                    json!({"session_id":"s","subscription_id":"sub","authority_epoch":1,"revision":1,"execution_generation":2,"replay_epoch":"e","last_sequence":0,"running":false,"messages":[],"pending":[],"prompts":[],"info":{"yolo":false}})
                }
                "session.interrupt" => {
                    accepted_tx.send(()).unwrap();
                    release_rx.recv_timeout(Duration::from_secs(3)).unwrap();
                    json!({})
                }
                "subagent.list" => {
                    assert_eq!(request["params"], json!({"session_id":"s"}));
                    roster()
                }
                _ => panic!("unexpected roster method {method}"),
            };
            ws.send(Message::Text(
                json!({"jsonrpc":"2.0","id":request["id"],"result":result})
                    .to_string()
                    .into(),
            ))
            .unwrap();
            if method == "session.interrupt" {
                ws.send(Message::Text(json!({"jsonrpc":"2.0","method":"event","params":{"type":"session.info","session_id":"s","replay_epoch":"e","seq":1,"execution_generation":2,"authority_epoch":1,"payload":{"capacity_drained":true}}}).to_string().into())).unwrap();
            }
        }
        methods
    });
    let grant = ConnectionGrant::parse(&serde_json::to_vec(&json!({"url":format!("ws://{address}/api/ws"),"protocols":["hermes-gateway-v1","hermes-gateway-ticket.fixture"],"profile_id":"p","instance_id":"i"})).unwrap()).unwrap();
    let limits = Limits {
        max_pending_requests: 1,
        ..Limits::default()
    };
    let client = RpcClient::connect(grant, limits.clone()).unwrap();
    let mut conversation = Conversation::open(
        client,
        limits,
        SessionOptions {
            cwd: std::env::current_dir().unwrap(),
            resume: None,
            model: None,
            provider: None,
        },
    )
    .unwrap();
    conversation
        .dispatch(Action::Interrupt { generation: 2 })
        .unwrap();
    accepted_rx.recv_timeout(Duration::from_secs(3)).unwrap();
    let refused = conversation.load_delegated(4).is_err();
    let phase = conversation.delegated.phase();
    let sendable = conversation.can_submit();
    release_tx.send(()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    while conversation.state.info["capacity_drained"] != true {
        assert!(Instant::now() < deadline, "capacity did not drain");
        conversation.poll().unwrap();
        thread::sleep(Duration::from_millis(5));
    }
    conversation.load_delegated(4).unwrap();
    let readable_while_pending = conversation.can_submit();
    let deadline = Instant::now() + Duration::from_secs(3);
    while conversation.delegated.phase() == Phase::Loading {
        assert!(Instant::now() < deadline, "roster did not settle");
        conversation.poll().unwrap();
        thread::sleep(Duration::from_millis(5));
    }
    let count = conversation
        .delegated
        .catalog(&conversation.state, 4)
        .unwrap()
        .rows
        .len();
    conversation.client.close();
    let methods = server.join().unwrap();
    assert!(refused && sendable && readable_while_pending);
    assert_eq!(phase, Phase::Failed);
    assert_eq!(count, 1);
    assert_eq!(
        methods
            .iter()
            .filter(|method| *method == "subagent.list")
            .count(),
        1
    );
    assert!(!methods.iter().any(|method| matches!(
        method.as_str(),
        "prompt.submit"
            | "subagent.steer"
            | "subagent.interrupt"
            | "process.kill"
            | "session.mutate"
    )));
}
