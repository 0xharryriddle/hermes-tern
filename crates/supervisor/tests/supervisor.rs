use hermes_tern_store::StoreLimits;
use hermes_tern_supervisor::{Client, Operation, Supervisor};
use serde_json::json;
use std::{
    fs,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
};
fn temp() -> PathBuf {
    let root = PathBuf::from(std::env::var_os("TMPDIR").unwrap())
        .join(format!("ht-ipc-{}", uuid::Uuid::new_v4().simple()));
    fs::create_dir(&root).unwrap();
    root
}
#[test]
fn real_socket_operator_and_peer_authority_are_separate() {
    let root = temp();
    let state = root.join("state");
    let stop = Arc::new(AtomicBool::new(false));
    let server = Supervisor::bind(&state, &root, StoreLimits::default()).unwrap();
    let s = stop.clone();
    let join = thread::spawn(move || server.run(s).unwrap());
    let operator = Client::operator(&state).unwrap();
    let a = operator
        .call(Operation::Register {
            kind: "hermes".into(),
            profile: "p".into(),
            session: "a".into(),
            model: "m".into(),
            provider: "p".into(),
        })
        .unwrap();
    let b = operator
        .call(Operation::Register {
            kind: "hermes".into(),
            profile: "p".into(),
            session: "b".into(),
            model: "m".into(),
            provider: "p".into(),
        })
        .unwrap();
    let aid = a["actor_id"].as_str().unwrap();
    let bid = b["actor_id"].as_str().unwrap();
    operator
        .call(Operation::Route {
            sender: aid.into(),
            recipient: bid.into(),
            allow: true,
        })
        .unwrap();
    let peer = Client::peer(
        &state,
        &PathBuf::from(a["credential_file"].as_str().unwrap()),
    )
    .unwrap();
    assert!(
        peer.call(Operation::Route {
            sender: aid.into(),
            recipient: bid.into(),
            allow: true
        })
        .is_err()
    );
    assert!(
        peer.call(Operation::Register {
            kind: "hermes".into(),
            profile: "p".into(),
            session: "forged".into(),
            model: "m".into(),
            provider: "p".into()
        })
        .is_err()
    );
    let sent = peer
        .call(Operation::Send {
            message: hermes_tern_store::MessageInput {
                message_id: uuid::Uuid::new_v4().to_string(),
                recipient: bid.into(),
                kind: "context".into(),
                payload: json!({"text":"peer context is not approval"}),
                ttl_seconds: 300,
                correlation_id: None,
            },
        })
        .unwrap();
    assert_eq!(sent["state"], "queued");
    let peer_b = Client::peer(
        &state,
        &PathBuf::from(b["credential_file"].as_str().unwrap()),
    )
    .unwrap();
    let read = peer_b.call(Operation::Receive {}).unwrap();
    assert_eq!(read["messages"].as_array().unwrap().len(), 1);
    assert_eq!(read["messages"][0]["sender_actor_id"], aid);
    stop.store(true, Ordering::Release);
    join.join().unwrap();
    let server = Supervisor::bind(&state, &root, StoreLimits::default()).unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let s = stop.clone();
    let join = thread::spawn(move || server.run(s).unwrap());
    assert_eq!(
        Client::operator(&state)
            .unwrap()
            .call(Operation::Messages {})
            .unwrap()["messages"][0]["state"],
        "ambiguous"
    );
    stop.store(true, Ordering::Release);
    join.join().unwrap();
}
#[test]
fn wire_rejects_unknown_fields_forged_sender_and_bad_versions() {
    for raw in [
        r#"{"v":2,"id":"x","auth":"secret","op":{"type":"receive"}}"#,
        r#"{"v":1,"id":"x","auth":"secret","op":{"type":"receive","sender":"forged"}}"#,
        r#"{"v":1,"v":1,"id":"x","auth":"secret","op":{"type":"receive"}}"#,
    ] {
        assert!(hermes_tern_supervisor::validate_request(raw.as_bytes()).is_err());
    }
}

#[test]
fn valid_credential_shape_does_not_mask_unknown_fields_on_unit_operations() {
    let token = "a".repeat(64);
    for kind in ["hello", "actors", "messages", "receive", "shutdown"] {
        let valid = json!({"v":1,"id":"probe","auth":token,"op":{"type":kind}});
        assert!(hermes_tern_supervisor::validate_request(valid.to_string().as_bytes()).is_ok());
        let forged = json!({"v":1,"id":"probe","auth":token,"op":{"type":kind,"sender":"forged"}});
        assert!(
            hermes_tern_supervisor::validate_request(forged.to_string().as_bytes()).is_err(),
            "{kind}"
        );
    }
}
