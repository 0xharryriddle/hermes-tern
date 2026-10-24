use hermes_tern_store::MessageInput;
use hermes_tern_supervisor::{Client, Operation};
use serde_json::json;
use std::{
    fs,
    path::PathBuf,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};
struct OwnedChild(Child);
impl Drop for OwnedChild {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
        }
        let _ = self.0.wait();
    }
}
// Fixture scheduling allowance, not a product latency/IPC timeout guarantee.
fn wait_ready(
    child: &mut Child,
    timeout: Duration,
    mut probe: impl FnMut() -> bool,
) -> Result<Duration, &'static str> {
    let started = Instant::now();
    loop {
        if child
            .try_wait()
            .map_err(|_| "fixture process status failed")?
            .is_some()
        {
            return Err("supervisor exited before readiness");
        }
        if started.elapsed() >= timeout {
            return Err("supervisor startup deadline");
        }
        let ready = probe();
        if child
            .try_wait()
            .map_err(|_| "fixture process status failed")?
            .is_some()
        {
            return Err("supervisor exited before readiness");
        }
        if started.elapsed() >= timeout {
            return Err("supervisor startup deadline");
        }
        if ready {
            return Ok(started.elapsed());
        }
        thread::sleep(Duration::from_millis(10));
    }
}
fn start(state: &std::path::Path, root: &std::path::Path) -> OwnedChild {
    let mut child = OwnedChild(
        Command::new(env!("CARGO_BIN_EXE_hermes-tern"))
            .args(["serve", "--state"])
            .arg(state)
            .arg("--workspace")
            .arg(root)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let elapsed = wait_ready(&mut child.0, Duration::from_secs(15), || {
        Client::operator(state)
            .and_then(|c| c.call(Operation::Hello {}))
            .is_ok()
    })
    .unwrap_or_else(|error| panic!("{error}; fixture retained at {}", root.display()));
    eprintln!("fixture supervisor ready after {elapsed:?}");
    child
}
#[test]
fn startup_accepts_a_delayed_live_process_not_a_late_or_exited_one() {
    let mut live = OwnedChild(Command::new("sleep").arg("30").spawn().unwrap());
    let start = Instant::now();
    let elapsed = wait_ready(&mut live.0, Duration::from_secs(2), || {
        start.elapsed() >= Duration::from_millis(100)
    })
    .unwrap();
    assert!(elapsed >= Duration::from_millis(100));
    assert_eq!(
        wait_ready(&mut live.0, Duration::from_millis(20), || false),
        Err("supervisor startup deadline")
    );
    assert_eq!(
        wait_ready(&mut live.0, Duration::from_millis(20), || {
            thread::sleep(Duration::from_millis(40));
            true
        }),
        Err("supervisor startup deadline")
    );
    let mut exited = Command::new("false").spawn().unwrap();
    exited.wait().unwrap();
    assert_eq!(
        wait_ready(&mut exited, Duration::from_secs(1), || true),
        Err("supervisor exited before readiness")
    );
}

#[test]
fn killed_supervisor_preserves_accepted_work_and_never_redelivers_ambiguous_mail() {
    let root = PathBuf::from(std::env::var_os("TMPDIR").unwrap())
        .join(format!("ht-crash-{}", uuid::Uuid::new_v4().simple()));
    fs::create_dir(&root).unwrap();
    let state = root.join("state");
    let mut child = start(&state, &root);
    let operator = Client::operator(&state).unwrap();
    let mut actors = vec![];
    for i in 0..4 {
        actors.push(
            operator
                .call(Operation::Register {
                    kind: "hermes".into(),
                    profile: "fixture".into(),
                    session: format!("session-{i}"),
                    model: "fixture-no-model".into(),
                    provider: "no-inference".into(),
                })
                .unwrap(),
        );
    }
    for n in [1usize, 2, 4] {
        for i in 0..n {
            let a = &actors[i];
            let b = &actors[(i + 1) % 4];
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
            for j in 0..8 {
                peer.call(Operation::Send {
                    message: MessageInput {
                        message_id: uuid::Uuid::new_v4().to_string(),
                        recipient: bid.into(),
                        kind: "context".into(),
                        payload: json!({"n":n,"i":i,"j":j}),
                        ttl_seconds: 300,
                        correlation_id: None,
                    },
                })
                .unwrap();
            }
        }
    }
    let accepted = operator.call(Operation::Messages {}).unwrap()["messages"]
        .as_array()
        .unwrap()
        .len();
    assert_eq!(accepted, 56);
    let peer = Client::peer(
        &state,
        &PathBuf::from(actors[1]["credential_file"].as_str().unwrap()),
    )
    .unwrap();
    let delivered = peer.call(Operation::Receive {}).unwrap();
    assert_eq!(delivered["messages"].as_array().unwrap().len(), 8);
    child.0.kill().unwrap();
    child.0.wait().unwrap();
    let mut restarted = start(&state, &root);
    let rows = Client::operator(&state)
        .unwrap()
        .call(Operation::Messages {})
        .unwrap();
    let rows = rows["messages"].as_array().unwrap();
    assert_eq!(rows.len(), 56);
    assert_eq!(rows.iter().filter(|r| r["state"] == "ambiguous").count(), 8);
    for a in &actors {
        let peer = Client::peer(
            &state,
            &PathBuf::from(a["credential_file"].as_str().unwrap()),
        )
        .unwrap();
        loop {
            let response = peer.call(Operation::Receive {}).unwrap();
            let messages = response["messages"].as_array().unwrap();
            if messages.is_empty() {
                break;
            }
            for m in messages {
                assert!(
                    !delivered["messages"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|old| old["message_id"] == m["message_id"])
                );
                peer.call(Operation::Ack {
                    sender: m["sender_actor_id"].as_str().unwrap().into(),
                    message: m["message_id"].as_str().unwrap().into(),
                })
                .unwrap();
            }
        }
    }
    let rows = Client::operator(&state)
        .unwrap()
        .call(Operation::Messages {})
        .unwrap();
    assert_eq!(
        rows["messages"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|r| r["state"] == "consumed")
            .count(),
        48
    );
    operator.call(Operation::Shutdown {}).unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    while restarted.0.try_wait().unwrap().is_none() {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(10));
    }
    assert!(
        Client::operator(&state)
            .unwrap()
            .call(Operation::Hello {})
            .is_err()
    );
}
