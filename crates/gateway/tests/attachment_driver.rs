//! Real socket admission, not-enqueued rollback and no duplicate attachment upload.
use hermes_tern_gateway::{
    attachments::Phase,
    bootstrap::ConnectionGrant,
    config::{Limits, SessionOptions},
    driver::Conversation,
    session::Action,
    transport::RpcClient,
};
use serde_json::{Value, json};
use std::{
    fs,
    net::TcpListener,
    os::unix::fs::DirBuilderExt,
    path::PathBuf,
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
fn settle(c: &mut Conversation, predicate: impl Fn(&Conversation) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while !predicate(c) {
        assert!(Instant::now() < deadline, "attachment fixture not settled");
        c.poll().unwrap();
        thread::sleep(Duration::from_millis(5));
    }
}
#[test]
fn unsent_attachment_does_not_consume_budget_or_lock_send_and_explicit_review_sends_once() {
    let root = PathBuf::from(std::env::var_os("TMPDIR").expect("private TMPDIR"))
        .join(format!("ht-attachment-admission-{}", uuid::Uuid::new_v4()));
    fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
    let path = root.join("sample.txt");
    fs::write(&path, b"one reviewed snapshot").unwrap();
    fs::write(
        root.join("ownership.json"),
        serde_json::to_vec(&json!({"retention":"retain;no cleanup",
        "paths":[root,path,root.join("ownership.json")]}))
        .unwrap(),
    )
    .unwrap();
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
        let mut methods = Vec::new();
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
                    ws.send(Message::Text(json!({"jsonrpc":"2.0","method":"event","params":{"type":"session.info","session_id":"s","replay_epoch":"e","seq":1,"execution_generation":0,"authority_epoch":1,"payload":{"capacity_drained":true}}}).to_string().into())).unwrap();
                    continue;
                }
                "file.attach" => {
                    assert_eq!(
                        v["params"],
                        json!({"session_id":"s","name":"sample.txt","data_url":"data:text/plain;base64,b25lIHJldmlld2VkIHNuYXBzaG90"})
                    );
                    json!({"attached":true,"uploaded":true,"name":"sample.txt","ref_path":"attachments/sample.txt","ref_text":"@file:attachments/sample.txt"})
                }
                _ => panic!("unexpected attachment method {method}"),
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
            cwd: root,
            resume: None,
            model: None,
            provider: None,
        },
    )
    .unwrap();
    c.prepare_attachment(path.to_str().unwrap()).unwrap();
    let old = c.attachments.id().to_owned();
    c.dispatch(Action::Interrupt { generation: 0 }).unwrap();
    accepted_rx.recv_timeout(Duration::from_secs(3)).unwrap();
    let refused = c.stage_attachment(&old).is_err();
    let phase = c.attachments.phase();
    let attempts = c.attachments.attempts();
    let sendable = c.can_submit();
    release_tx.send(()).unwrap();
    settle(&mut c, |c| c.state.info["capacity_drained"] == true);
    assert!(
        c.stage_attachment(&old).is_err(),
        "unsent failure must require another explicit review"
    );
    c.prepare_attachment(path.to_str().unwrap()).unwrap();
    let current = c.attachments.id().to_owned();
    c.stage_attachment(&current).unwrap();
    let blocked = c.can_submit();
    settle(&mut c, |c| c.attachments.phase() != Phase::Pending);
    let result = c.attachments.receipt().unwrap().reference().to_owned();
    let final_attempts = c.attachments.attempts();
    c.client.close();
    let methods = server.join().unwrap();
    assert!(refused && sendable && !blocked);
    assert_eq!(phase, Phase::Failed);
    assert_eq!(attempts, 0);
    assert_eq!(final_attempts, 1);
    assert_eq!(result, "@file:attachments/sample.txt");
    assert_eq!(methods.iter().filter(|m| *m == "file.attach").count(), 1);
    assert!(!methods.iter().any(|m| matches!(
        m.as_str(),
        "prompt.submit" | "command.dispatch" | "session.mutate" | "config.set"
    )));
}
