//! Opt-in, provider-free acceptance. Creates only a new isolated logical session.
use hermes_tern_gateway::{
    config::{AttachOptions, Limits, SessionOptions},
    driver::Conversation,
    inspect,
};
use serde_json::json;
#[test]
#[ignore = "read-only canonical metadata probe; existing authorized gateway required"]
fn inspect_capability_handshake_metadata() {
    let root = std::path::PathBuf::from(std::env::var_os("HERMES_TERN_ROOT").unwrap());
    let python = std::path::PathBuf::from(std::env::var_os("HERMES_TERN_PYTHON").unwrap());
    let attach = AttachOptions::new(root, python, None).unwrap();
    let limits = Limits::default();
    let mut client = inspect::connect(&attach, &limits).unwrap();
    inspect::qualify(&mut client, &limits).unwrap();
    let expected = client
        .request("client.capabilities", json!({"server_requests":true}))
        .unwrap();
    for _ in 0..16 {
        match client.recv(limits.request_timeout).unwrap() {
            hermes_tern_gateway::protocol::Inbound::Event(e) => println!(
                "Event type={} has_session={} sequenced={} fields={:?}",
                hermes_tern_core::text::display_text(&e.kind),
                e.session_id.is_some(),
                e.seq.is_some(),
                e.payload.as_object().map(|v| v.keys().collect::<Vec<_>>())
            ),
            hermes_tern_gateway::protocol::Inbound::Response { id, result } => {
                assert_eq!(id, expected);
                assert!(result.is_ok());
                let catalog = inspect::call(
                    &mut client,
                    "commands.catalog",
                    json!({}),
                    limits.request_timeout,
                )
                .unwrap();
                let projected = hermes_tern_gateway::commands::Catalog::parse(catalog).unwrap();
                assert!(projected.rows.iter().any(|row| row.readable));
                println!(
                    "Owner command catalog parsed: {} rows; no handler dispatch",
                    projected.rows.len()
                );
                client.close();
                return;
            }
            _ => panic!("unexpected server request"),
        }
    }
    panic!("metadata limit exceeded");
}
#[test]
#[ignore = "requires an existing authorized Hermes gateway; creates a fresh test session, no inference"]
fn create_detach_resume_same_session_without_inference() {
    let root = std::path::PathBuf::from(
        std::env::var_os("HERMES_TERN_ROOT").expect("explicit runtime root"),
    );
    let python = std::path::PathBuf::from(
        std::env::var_os("HERMES_TERN_PYTHON").expect("explicit interpreter"),
    );
    let cwd = std::path::PathBuf::from(
        std::env::var_os("HERMES_TERN_TEST_CWD").expect("explicit private workspace"),
    );
    let attach = AttachOptions::new(root, python, None).unwrap();
    let limits = Limits::default();
    let client = inspect::connect(&attach, &limits).unwrap();
    let instance = client.instance_id.clone();
    let mut c = Conversation::open(
        client,
        limits.clone(),
        SessionOptions {
            cwd: cwd.clone(),
            resume: None,
            model: None,
            provider: None,
        },
    )
    .unwrap();
    let id = c.state.session_id.clone();
    assert!(!c.state.running);
    assert_eq!(c.state.info["yolo"], false);
    c.detach().unwrap();
    let client = inspect::connect(&attach, &limits).unwrap();
    assert_eq!(client.instance_id, instance);
    let mut c = Conversation::open(
        client,
        limits.clone(),
        SessionOptions {
            cwd,
            resume: Some(id.clone()),
            model: None,
            provider: None,
        },
    )
    .unwrap();
    assert_eq!(c.state.session_id, id);
    assert!(!c.state.running);
    assert!(c.state.messages.is_empty());
    c.detach().unwrap();
    let mut check = inspect::connect(&attach, &limits).unwrap();
    assert_eq!(check.instance_id, instance);
    let history = inspect::call(
        &mut check,
        "session.history",
        json!({"session_id":id}),
        limits.request_timeout,
    )
    .unwrap();
    assert_eq!(history["messages"].as_array().unwrap().len(), 0);
    check.close();
    println!("Create/detach/resume/history passed; no inference. Session {id}");
}
