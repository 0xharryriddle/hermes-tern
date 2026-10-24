use hermes_tern_gateway::{
    bootstrap::ConnectionGrant,
    config::{Limits, SessionOptions},
    driver::Conversation,
    transport::RpcClient,
};
use serde_json::{Value, json};
use std::{
    net::TcpListener,
    thread,
    time::{Duration, Instant},
};
use tungstenite::{
    Message, accept_hdr,
    handshake::server::{Request, Response},
};
fn snapshot() -> Value {
    json!({"session_id":"s","subscription_id":"sub","authority_epoch":1,"revision":1,"execution_generation":0,"replay_epoch":"e","last_sequence":0,"running":false,"messages":[],"pending":[],"prompts":[],"info":{"yolo":false}})
}
fn reply(ws: &mut tungstenite::WebSocket<std::net::TcpStream>, id: &Value, result: Value) {
    ws.send(Message::Text(
        json!({"jsonrpc":"2.0","id":id,"result":result})
            .to_string()
            .into(),
    ))
    .unwrap();
}
#[test]
fn conversation_drives_create_submit_denial_receipt_and_detach() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
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
        let mut submitted = false;
        loop {
            let frame = ws.read().unwrap();
            if frame.is_close() {
                break;
            }
            let v: Value = serde_json::from_str(&frame.into_text().unwrap()).unwrap();
            let method = v["method"].as_str().unwrap();
            methods.push(method.to_owned());
            match method {
                "ping" => reply(&mut ws, &v["id"], json!({"pong":true})),
                "runtime.describe" => reply(
                    &mut ws,
                    &v["id"],
                    json!({"profile_id":"p","instance_id":"i","capabilities":["durable-admission-v1","event-replay-v1","local-cli-create-v1"],"session_create":{"sources":["tui"],"parameters":["request_id","source","cwd","yolo","model","provider"]}}),
                ),
                "client.capabilities" => {
                    assert_eq!(v["params"]["server_requests"], true);
                    reply(&mut ws, &v["id"], json!({}));
                }
                "session.create" => {
                    assert_eq!(v["params"]["yolo"], false);
                    assert_eq!(v["params"]["source"], "tui");
                    reply(&mut ws, &v["id"], snapshot());
                }
                "prompt.submit" => {
                    assert!(!submitted, "no automatic repeat");
                    submitted = true;
                    assert_eq!(v["params"]["text"], "hello");
                    reply(
                        &mut ws,
                        &v["id"],
                        json!({"admission_id":"a","ref":{"profile_id":"p","session_id":"s"},"sequence":1,"authority_epoch":1,"execution_generation":1,"status":"started","outcome":null}),
                    );
                    ws.send(Message::Text(json!({"jsonrpc":"2.0","method":"event","params":{"type":"approval.request","session_id":"s","replay_epoch":"e","seq":1,"execution_generation":1,"authority_epoch":1,"admission_id":"a","payload":{"kind":"approval","prompt_id":"pr","execution_generation":1,"choices":["once","deny"],"command":"danger"}}}).to_string().into())).unwrap();
                }
                "approval.respond" => {
                    assert_eq!(v["params"]["choice"], "deny");
                    assert_eq!(v["params"]["execution_generation"], 1);
                    reply(
                        &mut ws,
                        &v["id"],
                        json!({"status":"resolved","prompt_id":"pr"}),
                    );
                }
                "prompt.receipt" => {
                    let terminal = methods.iter().any(|s| s == "approval.respond");
                    reply(
                        &mut ws,
                        &v["id"],
                        json!({"admission_id":"a","ref":{"profile_id":"p","session_id":"s"},"sequence":1,"authority_epoch":1,"execution_generation":1,"status":if terminal {"terminal"} else {"started"},"outcome":if terminal {Some("completed")} else {None},"result":{"final_response":"Denied safely"}}),
                    );
                }
                "session.detach" => {
                    reply(
                        &mut ws,
                        &v["id"],
                        json!({"session_id":"s","subscription_id":"sub","detached":true}),
                    );
                    break;
                }
                _ => panic!("unexpected method {method}"),
            }
        }
        assert!(methods.iter().any(|s| s == "approval.respond"));
    });
    let g=ConnectionGrant::parse(&serde_json::to_vec(&json!({"url":format!("ws://{addr}/api/ws"),"protocols":["hermes-gateway-v1","hermes-gateway-ticket.fixture"],"profile_id":"p","instance_id":"i"})).unwrap()).unwrap();
    let limits = Limits::default();
    let client = RpcClient::connect(g, limits.clone()).unwrap();
    let opts = SessionOptions {
        cwd: std::env::current_dir().unwrap(),
        resume: None,
        model: None,
        provider: None,
    };
    let mut c = Conversation::open(client, limits, opts).unwrap();
    c.submit("hello").unwrap();
    let until = Instant::now() + Duration::from_secs(3);
    while c.state.terminal.is_none() {
        assert!(Instant::now() < until);
        c.poll().unwrap();
        if let Some(p) = c.state.prompts.values().find(|p| !p.answered).cloned() {
            let a = c.state.skip_prompt(&p.id, p.generation).unwrap();
            c.dispatch(a).unwrap();
        }
        thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(
        c.state.terminal.as_ref().unwrap()["result"]["final_response"],
        "Denied safely"
    );
    c.detach().unwrap();
    server.join().unwrap();
}
