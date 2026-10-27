//! Real loopback transport admission: unsent requests are not unknown side effects.
use hermes_tern_gateway::{
    bootstrap::ConnectionGrant,
    commands::Phase,
    config::{Limits, SessionOptions},
    driver::Conversation,
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

fn reply(ws: &mut tungstenite::WebSocket<std::net::TcpStream>, id: &Value, result: Value) {
    ws.send(Message::Text(
        json!({"jsonrpc":"2.0","id":id,"result":result})
            .to_string()
            .into(),
    ))
    .unwrap();
}
fn poll_until(c: &mut Conversation, predicate: impl Fn(&Conversation) -> bool) {
    let until = Instant::now() + Duration::from_secs(3);
    while !predicate(c) {
        assert!(Instant::now() < until, "command fixture did not settle");
        c.poll().unwrap();
        thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn unsent_catalog_and_read_capacity_refusals_allow_explicit_retry_without_unknown_lockout() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let (accepted_tx, accepted_rx) = mpsc::sync_channel(1);
    let (release_tx, release_rx) = mpsc::sync_channel(1);
    let server = thread::spawn(move || {
        let (socket, _) = listener.accept().unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        #[allow(clippy::result_large_err)]
        let mut ws = accept_hdr(socket, |_: &Request, mut r: Response| {
            r.headers_mut().insert(
                "Sec-WebSocket-Protocol",
                "hermes-gateway-v1".parse().unwrap(),
            );
            Ok(r)
        })
        .unwrap();
        let mut methods = vec![];
        let mut drains = 0;
        while let Ok(frame) = ws.read() {
            if frame.is_close() {
                break;
            }
            let v: Value = serde_json::from_str(&frame.into_text().unwrap()).unwrap();
            let method = v["method"].as_str().unwrap();
            methods.push(method.to_owned());
            let result = match method {
                "ping" => json!({"pong":true}),
                "runtime.describe" => {
                    json!({"profile_id":"p","instance_id":"i","capabilities":["durable-admission-v1","event-replay-v1","local-cli-create-v1"],"session_create":{"sources":["tui"],"parameters":["request_id","source","cwd","yolo"]}})
                }
                "client.capabilities" => json!({}),
                "session.create" => {
                    json!({"session_id":"s","subscription_id":"sub","authority_epoch":1,"revision":1,"execution_generation":0,"replay_epoch":"e","last_sequence":0,"running":false,"messages":[],"pending":[],"prompts":[],"info":{"yolo":false}})
                }
                "session.interrupt" => {
                    accepted_tx.send(()).unwrap();
                    release_rx.recv_timeout(Duration::from_secs(3)).unwrap();
                    reply(&mut ws, &v["id"], json!({}));
                    drains += 1;
                    ws.send(Message::Text(json!({"jsonrpc":"2.0","method":"event","params":{"type":"session.info","session_id":"s","replay_epoch":"e","seq":drains,"execution_generation":0,"authority_epoch":1,"payload":{"capacity_drains":drains,"yolo":false}}}).to_string().into())).unwrap();
                    continue;
                }
                "commands.catalog" => {
                    assert_eq!(v["params"], json!({"session_id":"s"}));
                    json!({"pairs":[["/memory","Memory inspection"]],"canon":{"/memory":"/memory"},"commands":{"/memory":{}},"skills":{},"plugin_commands":{}})
                }
                "command.dispatch" => {
                    assert_eq!(
                        v["params"],
                        json!({"session_id":"s","name":"memory","arg":""})
                    );
                    json!({"type":"exec","output":"owner-read-once"})
                }
                _ => panic!("unexpected command fixture request {method}"),
            };
            reply(&mut ws, &v["id"], result);
        }
        methods
    });
    let grant=ConnectionGrant::parse(&serde_json::to_vec(&json!({"url":format!("ws://{addr}/api/ws"),"protocols":["hermes-gateway-v1","hermes-gateway-ticket.fixture"],"profile_id":"p","instance_id":"i"})).unwrap()).unwrap();
    let limits = Limits {
        max_pending_requests: 1,
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

    c.dispatch(Action::Interrupt { generation: 0 }).unwrap();
    accepted_rx.recv_timeout(Duration::from_secs(3)).unwrap();
    let catalog_refused = c.load_commands().is_err();
    let catalog_phase = c.commands.phase();
    release_tx.send(()).unwrap();
    poll_until(&mut c, |c| c.state.info["capacity_drains"] == 1);
    c.load_commands().unwrap();
    poll_until(&mut c, |c| c.commands.phase() != Phase::Loading);
    let old = c.commands.id().to_owned();
    c.dispatch(Action::Interrupt { generation: 0 }).unwrap();
    accepted_rx.recv_timeout(Duration::from_secs(3)).unwrap();
    let read_refused = c.read_command(&old, 0).is_err();
    let read_phase = c.commands.phase();
    let sendable_after_refusal = c.can_submit();
    release_tx.send(()).unwrap();
    poll_until(&mut c, |c| c.state.info["capacity_drains"] == 2);
    let explicit_retry = c.load_commands().is_ok();
    if explicit_retry {
        poll_until(&mut c, |c| c.commands.phase() != Phase::Loading);
        let current = c.commands.id().to_owned();
        c.read_command(&current, 0).unwrap();
        poll_until(&mut c, |c| c.commands.phase() != Phase::Running);
    }
    let output = c.commands.output().join("");
    let healthy = c.can_submit();
    c.client.close();
    let methods = server.join().unwrap();
    assert!(catalog_refused && read_refused);
    assert_eq!(catalog_phase, Phase::Failed);
    assert_eq!(
        read_phase,
        Phase::Failed,
        "an unsent read was wrongly classified as unknown side effects"
    );
    assert!(sendable_after_refusal && explicit_retry && healthy);
    assert_eq!(output, "owner-read-once");
    assert_eq!(
        methods.iter().filter(|m| *m == "commands.catalog").count(),
        2
    );
    assert_eq!(
        methods.iter().filter(|m| *m == "command.dispatch").count(),
        1
    );
    assert!(!methods.iter().any(|m| matches!(
        m.as_str(),
        "prompt.submit" | "session.mutate" | "config.set"
    )));
}
