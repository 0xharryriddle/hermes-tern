use hermes_tern_gateway::bootstrap::ConnectionGrant;
use serde_json::json;
fn grant(url: &str) -> Vec<u8> {
    serde_json::to_vec(&json!({"url":url,"protocols":["hermes-gateway-v1","hermes-gateway-ticket.test-only"],"profile_id":"p","instance_id":"i"})).unwrap()
}
#[test]
fn only_literal_loopback_canonical_urls_are_accepted() {
    for url in ["ws://127.0.0.1:1234/api/ws", "ws://[::1]:1234/api/ws"] {
        assert!(ConnectionGrant::parse(&grant(url)).is_ok());
    }
    for url in [
        "ws://example.com/api/ws",
        "ws://localhost/api/ws",
        "ws://127.0.0.1:0/api/ws",
        "wss://127.0.0.1/api/ws",
        "ws://a@127.0.0.1/api/ws",
        "ws://127.0.0.1/api/ws?ticket=secret",
        "ws://127.0.0.1/api/ws#x",
        "ws://127.0.0.1/other",
    ] {
        assert!(ConnectionGrant::parse(&grant(url)).is_err(), "{url}");
    }
}
#[test]
fn malformed_grants_and_secret_debug_are_rejected() {
    let g = ConnectionGrant::parse(&grant("ws://127.0.0.1:1234/api/ws")).unwrap();
    assert!(!format!("{g:?}").contains("test-only"));
    for s in [
        r#"{}"#,
        r#"{"url":"ws://127.0.0.1/api/ws","url":"ws://127.0.0.1/api/ws"}"#,
    ] {
        assert!(ConnectionGrant::parse(s.as_bytes()).is_err());
    }
    let mut v: serde_json::Value = serde_json::from_slice(&grant("ws://127.0.0.1/api/ws")).unwrap();
    for protocols in [
        json!(["hermes-gateway-v1"]),
        json!(["hermes-gateway-v1", "hermes-gateway-ticket.bad\nvalue"]),
        json!(["hermes-gateway-v1", "hermes-gateway-v1"]),
    ] {
        v["protocols"] = protocols;
        assert!(ConnectionGrant::parse(&serde_json::to_vec(&v).unwrap()).is_err());
    }
}

#[test]
fn helper_success_and_timeout_are_bounded_and_reaped() {
    use hermes_tern_gateway::{
        bootstrap::discover,
        config::{AttachOptions, Limits},
    };
    use std::{
        fs,
        time::{Duration, Instant},
    };
    let root = std::path::PathBuf::from(std::env::var_os("TMPDIR").expect("private TMPDIR"))
        .join(format!("ht-bootstrap-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(root.join("ui-tui/scripts")).unwrap();
    let path = root.join("ui-tui/scripts/gateway_bootstrap.py");
    let options = AttachOptions::new(&root, "/usr/bin/python3", Some(root.clone())).unwrap();
    let fault_limits = Limits {
        connect_timeout: Duration::from_millis(500),
        ..Limits::default()
    };
    // Positive startup must tolerate scheduling slower than the fault-arm budget.
    fs::write(&path, "import time\ndef bootstrap(start, recover):\n    assert not start and not recover\n    time.sleep(0.75)\n    return {'url':'ws://127.0.0.1:1234/api/ws','protocols':['hermes-gateway-v1','hermes-gateway-ticket.fixture'],'profile_id':'p','instance_id':'i'}\n").unwrap();
    // Use the unchanged production startup allowance for the positive arm.
    // The deliberately short fault deadline below is a different assertion.
    let result = discover(&options, &Limits::default());
    fs::write(
        &path,
        "import time\ndef bootstrap(start, recover):\n    time.sleep(10)\n",
    )
    .unwrap();
    let begin = Instant::now();
    let timeout = discover(&options, &fault_limits);
    fs::remove_file(path).unwrap();
    fs::remove_dir(root.join("ui-tui/scripts")).unwrap();
    fs::remove_dir(root.join("ui-tui")).unwrap();
    fs::remove_dir(root).unwrap();
    assert!(result.is_ok(), "{result:?}");
    assert!(timeout.is_err());
    assert!(begin.elapsed() < Duration::from_secs(2));
}

#[test]
fn selected_profile_refuses_wrong_grant_before_any_socket_connection() {
    use hermes_tern_gateway::{
        config::{AttachOptions, Limits},
        inspect,
    };
    use std::{fs, net::TcpListener};
    let root = std::path::PathBuf::from(std::env::var_os("TMPDIR").expect("private TMPDIR"))
        .join(format!("ht-selected-profile-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(root.join("ui-tui/scripts")).unwrap();
    let home = root.join("selected");
    fs::create_dir(&home).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    fs::write(root.join("ui-tui/scripts/gateway_bootstrap.py"),format!("import os\ndef bootstrap(start,recover):\n    assert (start,recover)==(False,False)\n    assert os.environ['HERMES_HOME']=={home:?}\n    return {{'url':'ws://{}/api/ws','protocols':['hermes-gateway-v1','hermes-gateway-ticket.synthetic'],'profile_id':'/wrong-profile','instance_id':'synthetic'}}\n",listener.local_addr().unwrap())).unwrap();
    let options = AttachOptions::new(&root, "/usr/bin/python3", Some(home.clone())).unwrap();
    let result = inspect::connect_selected(&options, &Limits::default(), &home);
    assert!(matches!(
        result,
        Err(hermes_tern_core::error::Error::Bootstrap(
            "selected profile differs from grant"
        ))
    ));
    assert!(matches!(listener.accept(),Err(e) if e.kind()==std::io::ErrorKind::WouldBlock));
    fs::write(root.join("ownership.json"),serde_json::to_vec(&json!({"retention":"retain;no deletion","paths":[root,root.join("ui-tui"),root.join("ui-tui/scripts"),root.join("ui-tui/scripts/gateway_bootstrap.py"),home,root.join("ownership.json")]})).unwrap()).unwrap();
}
