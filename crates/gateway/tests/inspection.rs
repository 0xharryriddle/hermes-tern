use hermes_tern_gateway::{
    bootstrap::ConnectionGrant, config::Limits, inspect, transport::RpcClient,
};
use serde_json::{Value, json};
use std::{net::TcpListener, thread, time::Duration};
use tungstenite::{
    Message, accept_hdr,
    handshake::server::{Request, Response},
};
#[test]
fn unbound_inspection_tolerates_background_notifications_without_mutation() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let join = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        #[allow(clippy::result_large_err)]
        let mut ws = accept_hdr(stream, |_: &Request, mut r: Response| {
            r.headers_mut().insert(
                "Sec-WebSocket-Protocol",
                "hermes-gateway-v1".parse().unwrap(),
            );
            Ok(r)
        })
        .unwrap();
        let request: Value =
            serde_json::from_str(&ws.read().unwrap().into_text().unwrap()).unwrap();
        assert_eq!(request["method"], "ping");
        ws.send(Message::Text(json!({"jsonrpc":"2.0","method":"event","params":{"session_id":"other","type":"display.lease","payload":{}}}).to_string().into())).unwrap();
        ws.send(Message::Text(
            json!({"jsonrpc":"2.0","id":request["id"],"result":{"pong":true}})
                .to_string()
                .into(),
        ))
        .unwrap();
    });
    let grant=ConnectionGrant::parse(&serde_json::to_vec(&json!({"url":format!("ws://{addr}/api/ws"),"protocols":["hermes-gateway-v1","hermes-gateway-ticket.fixture"],"profile_id":"p","instance_id":"i"})).unwrap()).unwrap();
    let mut client = RpcClient::connect(grant, Limits::default()).unwrap();
    let result = inspect::call(&mut client, "ping", json!({}), Duration::from_secs(2));
    client.close();
    join.join().unwrap();
    assert_eq!(result.unwrap()["pong"], true);
}
