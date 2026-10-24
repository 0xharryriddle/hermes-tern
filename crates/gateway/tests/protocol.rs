use hermes_tern_core::json::strict_json;
use hermes_tern_gateway::protocol::{Inbound, parse_frame};
#[test]
fn ready_is_event_and_server_requests_have_separate_identity() {
    assert!(matches!(
        parse_frame(
            r#"{"jsonrpc":"2.0","method":"event","params":{"type":"gateway.ready","payload":{}}}"#
        )
        .unwrap(),
        Inbound::Event(_)
    ));
    assert!(matches!(
        parse_frame(r#"{"jsonrpc":"2.0","id":"srq-abc","method":"approval.request","params":{}}"#)
            .unwrap(),
        Inbound::ServerRequest { .. }
    ));
}
#[test]
fn ambiguity_is_never_silently_normalized() {
    for raw in [r#"{"a":1,"a":2}"#, r#"{"a":{"b":1,"b":2}}"#, r#"{} {}"#] {
        assert!(strict_json(raw.as_bytes()).is_err());
    }
    for raw in [
        r#"{"jsonrpc":"2.0","id":1,"result":{},"error":{}}"#,
        r#"{"jsonrpc":"1.0","id":1,"result":{}}"#,
        r#"{"jsonrpc":"2.0","id":null,"result":{}}"#,
        r#"[]"#,
    ] {
        assert!(parse_frame(raw).is_err());
    }
}
