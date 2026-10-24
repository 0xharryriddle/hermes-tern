use hermes_tern_gateway::{
    bootstrap::ConnectionGrant, config::Limits, protocol::Inbound, transport::RpcClient,
};
use serde_json::{Value, json};
use std::{net::TcpListener, thread, time::Duration};
use tungstenite::{
    Message, accept_hdr,
    handshake::server::{Request, Response},
};

fn server(
    echo: bool,
    f: impl FnOnce(tungstenite::WebSocket<std::net::TcpStream>) + Send + 'static,
) -> (ConnectionGrant, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let join = thread::spawn(move || {
        let (s, _) = listener.accept().unwrap();
        s.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        // Callback's error type is fixed by tungstenite, not our API.
        #[allow(clippy::result_large_err)]
        let ws = accept_hdr(s, move |req: &Request, mut response: Response| {
            assert_eq!(req.uri().path(), "/api/ws");
            assert!(req.headers().get("Origin").is_none());
            if echo {
                response.headers_mut().insert(
                    "Sec-WebSocket-Protocol",
                    "hermes-gateway-v1".parse().unwrap(),
                );
            }
            Ok(response)
        })
        .unwrap();
        f(ws);
    });
    let grant = ConnectionGrant::parse(&serde_json::to_vec(&json!({"url":format!("ws://{addr}/api/ws"),"protocols":["hermes-gateway-v1","hermes-gateway-ticket.test-only"],"profile_id":"p","instance_id":"i"})).unwrap()).unwrap();
    (grant, join)
}
fn next(c: &mut RpcClient) -> hermes_tern_core::error::Result<Inbound> {
    c.recv(Duration::from_secs(2))
}
#[test]
fn requires_exact_subprotocol_echo() {
    let (g, j) = server(false, |_| {});
    assert!(RpcClient::connect(g, Limits::default()).is_err());
    j.join().unwrap();
}
#[test]
fn events_can_precede_response_and_request_is_single_json() {
    let (g, j) = server(true, |mut ws| {
        let text = ws.read().unwrap().into_text().unwrap();
        let v: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["method"], "ping");
        ws.send(Message::Text(json!({"jsonrpc":"2.0","method":"event","params":{"type":"gateway.ready","payload":{}}}).to_string().into())).unwrap();
        ws.send(Message::Text(
            json!({"jsonrpc":"2.0","id":v["id"],"result":{"pong":true}})
                .to_string()
                .into(),
        ))
        .unwrap();
        let _ = ws.read();
    });
    let mut c = RpcClient::connect(g, Limits::default()).unwrap();
    let id = c.request("ping", json!({})).unwrap();
    assert!(matches!(next(&mut c).unwrap(), Inbound::Event(_)));
    assert!(
        matches!(next(&mut c).unwrap(), Inbound::Response{id: got, result: Ok(v)} if got == id && v["pong"] == true)
    );
    c.close();
    j.join().unwrap();
}
#[test]
fn binary_and_oversized_messages_fail_closed() {
    for frame in [
        Message::Binary(vec![0; 4].into()),
        Message::Text("x".repeat(2048).into()),
    ] {
        let (g, j) = server(true, |mut ws| {
            let _ = ws.send(frame);
            let _ = ws.read();
        });
        let mut c = RpcClient::connect(
            g,
            Limits {
                max_frame_bytes: 1024,
                ..Limits::default()
            },
        )
        .unwrap();
        assert!(next(&mut c).is_err());
        c.close();
        j.join().unwrap();
    }
}
#[test]
fn missing_response_times_out_without_retransmission() {
    let (g, j) = server(true, |mut ws| {
        ws.read().unwrap();
        thread::sleep(Duration::from_millis(200));
        assert!(ws.read().is_err() || !ws.can_read());
    });
    let mut c = RpcClient::connect(
        g,
        Limits {
            request_timeout: Duration::from_millis(50),
            ..Limits::default()
        },
    )
    .unwrap();
    c.request("ping", json!({})).unwrap();
    assert!(next(&mut c).is_err());
    c.close();
    j.join().unwrap();
}
#[test]
fn invalid_json_duplicates_and_unknown_response_ids_disconnect() {
    for raw in [
        r#"{"jsonrpc":"2.0","id":1,"result":{},"result":true}"#,
        r#"{"jsonrpc":"2.0","id":991,"result":{}}"#,
    ] {
        let (g, j) = server(true, move |mut ws| {
            ws.send(Message::Text(raw.into())).unwrap();
            let _ = ws.read();
        });
        let mut c = RpcClient::connect(g, Limits::default()).unwrap();
        assert!(next(&mut c).is_err());
        c.close();
        j.join().unwrap();
    }
}
