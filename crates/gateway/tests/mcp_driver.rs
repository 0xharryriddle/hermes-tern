//! Actual socket admission and dedicated MCP reattach correlation, not live Hermes.
use hermes_tern_gateway::{
    bootstrap::ConnectionGrant,
    config::{Limits, SessionOptions},
    driver::Conversation,
    mcp::Phase,
    session::Action,
    transport::RpcClient,
};
use serde_json::{Value, json};
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

fn snapshot(generation: u64) -> Value {
    json!({"session_id":"s","subscription_id":"sub","authority_epoch":1,"revision":1,
        "execution_generation":generation,"replay_epoch":"e","last_sequence":0,"running":false,
        "messages":[],"pending":[],"prompts":[],"info":{"yolo":false,"profile_id":"p"}})
}
fn settle(c: &mut Conversation, predicate: impl Fn(&Conversation) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while !predicate(c) {
        assert!(Instant::now() < deadline, "MCP fixture did not settle");
        c.poll().unwrap();
        thread::sleep(Duration::from_millis(5));
    }
}
#[test]
fn unsent_capacity_failure_returns_budget_and_old_replay_snapshot_cannot_complete_mcp_readback() {
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
        let mut replay = None;
        let mut reload = None;
        let reply =
            |ws: &mut tungstenite::WebSocket<std::net::TcpStream>, id: Value, result: Value| {
                ws.send(Message::Text(
                    json!({"jsonrpc":"2.0","id":id,"result":result})
                        .to_string()
                        .into(),
                ))
                .unwrap();
            };
        while let Ok(frame) = ws.read() {
            if frame.is_close() {
                break;
            }
            let request: Value = serde_json::from_str(&frame.into_text().unwrap()).unwrap();
            let method = request["method"].as_str().unwrap();
            methods.push(method.to_owned());
            let id = request["id"].clone();
            match method {
                "ping" => reply(&mut ws, id, json!({"pong":true})),
                "runtime.describe" => reply(
                    &mut ws,
                    id,
                    json!({"profile_id":"p","instance_id":"i","capabilities":["durable-admission-v1","event-replay-v1","local-cli-create-v1"],"session_create":{"sources":["tui"],"parameters":["request_id","source","cwd","yolo"]}}),
                ),
                "client.capabilities" => reply(&mut ws, id, json!({})),
                "session.create" => reply(&mut ws, id, snapshot(2)),
                "session.interrupt" => {
                    accepted_tx.send(()).unwrap();
                    release_rx.recv_timeout(Duration::from_secs(3)).unwrap();
                    reply(&mut ws, id, json!({}));
                }
                "reload.mcp" => {
                    assert_eq!(request["params"], json!({"session_id":"s","confirm":true}));
                    reload = Some(id);
                }
                "session.resume" => {
                    assert_eq!(
                        request["params"],
                        json!({"session_id":"s","display_projection":true})
                    );
                    if replay.is_none() {
                        replay = Some(id);
                        reply(
                            &mut ws,
                            reload.take().unwrap(),
                            json!({"status":"reloaded","session":"applied","session_servers":[]}),
                        );
                    } else {
                        // The older general snapshot is answered first; the dedicated read remains pending.
                        reply(&mut ws, replay.take().unwrap(), snapshot(2));
                        accepted_tx.send(()).unwrap();
                        release_rx.recv_timeout(Duration::from_secs(3)).unwrap();
                        reply(&mut ws, id, snapshot(3));
                    }
                }
                _ => panic!("unexpected MCP fixture method {method}"),
            }
        }
        methods
    });
    let grant = ConnectionGrant::parse(&serde_json::to_vec(&json!({"url":format!("ws://{address}/api/ws"),"protocols":["hermes-gateway-v1","hermes-gateway-ticket.fixture"],"profile_id":"p","instance_id":"i"})).unwrap()).unwrap();
    let limits = Limits {
        max_pending_requests: 2,
        ..Limits::default()
    };
    let client = RpcClient::connect(grant, limits.clone()).unwrap();
    let mut c = Conversation::open(
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
    c.review_mcp().unwrap();
    let old = c.mcp.id().to_owned();
    c.dispatch(Action::Interrupt { generation: 2 }).unwrap();
    accepted_rx.recv_timeout(Duration::from_secs(3)).unwrap();
    c.dispatch(Action::Interrupt { generation: 2 }).unwrap();
    let refused = c.confirm_mcp(&old).is_err();
    let phase = c.mcp.phase();
    let attempts = c.mcp.attempts();
    let sendable = c.can_submit();
    release_tx.send(()).unwrap();
    accepted_rx.recv_timeout(Duration::from_secs(3)).unwrap();
    release_tx.send(()).unwrap();
    for _ in 0..20 {
        c.poll().unwrap();
        thread::sleep(Duration::from_millis(5));
    }
    assert!(refused && sendable);
    assert_eq!(phase, Phase::Failed);
    assert_eq!(attempts, 0);
    assert!(c.confirm_mcp(&old).is_err());
    c.review_mcp().unwrap();
    let id = c.mcp.id().to_owned();
    c.confirm_mcp(&id).unwrap();
    assert!(!c.can_submit());
    assert!(c.review_mcp().is_err() && c.confirm_mcp(&id).is_err());
    c.dispatch(Action::Snapshot).unwrap();
    settle(&mut c, |c| c.mcp.phase() == Phase::Reconciling);
    accepted_rx.recv_timeout(Duration::from_secs(3)).unwrap();
    for _ in 0..20 {
        c.poll().unwrap();
        thread::sleep(Duration::from_millis(5));
    }
    let pending = c.mcp.phase();
    let blocked = !c.can_submit();
    release_tx.send(()).unwrap();
    settle(&mut c, |c| c.mcp.phase() != Phase::Reconciling);
    let final_phase = c.mcp.phase();
    let final_generation = c.state.generation;
    let final_attempts = c.mcp.attempts();
    c.client.close();
    let methods = server.join().unwrap();
    assert_eq!(
        pending,
        Phase::Reconciling,
        "precommit replay snapshot completed MCP readback"
    );
    assert!(blocked);
    assert_eq!(final_phase, Phase::Reconciled);
    assert_eq!(final_generation, 3);
    assert_eq!(final_attempts, 1);
    assert_eq!(methods.iter().filter(|m| *m == "reload.mcp").count(), 1);
    assert_eq!(methods.iter().filter(|m| *m == "session.resume").count(), 2);
    assert!(!methods.iter().any(|m| matches!(
        m.as_str(),
        "prompt.submit" | "config.get" | "config.set" | "session.mutate" | "command.dispatch"
    )));
}
