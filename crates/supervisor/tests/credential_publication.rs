use hermes_tern_store::StoreLimits;
use hermes_tern_supervisor::{Client, Operation, Supervisor};
use serde_json::{Value, json};
use std::{
    fs,
    os::unix::fs::DirBuilderExt,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
};

struct Fixture {
    root: PathBuf,
    state: PathBuf,
    stop: Arc<AtomicBool>,
    join: Option<thread::JoinHandle<()>>,
    operator: Client,
}
impl Fixture {
    fn new() -> Self {
        let root = PathBuf::from(std::env::var_os("TMPDIR").unwrap())
            .join(format!("ht-credentials-{}", uuid::Uuid::new_v4()));
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        let state = root.join("state");
        let stop = Arc::new(AtomicBool::new(false));
        let server = Supervisor::bind(&state, &root, StoreLimits::default()).unwrap();
        let signal = stop.clone();
        let join = Some(thread::spawn(move || server.run(signal).unwrap()));
        let operator = Client::operator(&state).unwrap();
        Self {
            root,
            state,
            stop,
            join,
            operator,
        }
    }
    fn register(&self, session: &str) -> hermes_tern_core::error::Result<Value> {
        self.operator.call(Operation::Register {
            kind: "hermes".into(),
            profile: "fixture".into(),
            session: session.into(),
            model: "fixture".into(),
            provider: "no-inference".into(),
        })
    }
    fn block_publication(&self) {
        // Move only this fixture's directory, preserving all generated credentials.
        fs::rename(
            self.state.join("credentials"),
            self.root.join("retained-credentials"),
        )
        .unwrap();
        fs::write(self.state.join("credentials"), "not a directory").unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.join.take().unwrap().join().unwrap();
        let mut paths = vec![
            self.root.clone(),
            self.state.clone(),
            self.root.join("ownership.json"),
        ];
        for dir in [
            &self.root,
            &self.state,
            &self.state.join("credentials"),
            &self.root.join("retained-credentials"),
        ] {
            if dir.is_dir() {
                paths.extend(fs::read_dir(dir).unwrap().map(|e| e.unwrap().path()));
            }
        }
        fs::write(
            self.root.join("ownership.json"),
            json!({"retention":"retain; no deletion","paths":paths}).to_string(),
        )
        .unwrap();
    }
}

#[test]
fn failed_credential_publication_does_not_admit_an_unusable_actor() {
    let f = Fixture::new();
    f.register("positive-control").unwrap();
    let before = f.operator.call(Operation::Actors {}).unwrap();
    f.block_publication();
    assert!(f.register("must-not-commit").is_err());
    assert_eq!(
        f.operator.call(Operation::Actors {}).unwrap(),
        before,
        "actor committed without durable credential"
    );
}

#[test]
fn operator_actor_snapshot_recovers_current_credential_path_without_secret() {
    let f = Fixture::new();
    let first = f.register("reconcile").unwrap();
    let actor = first["actor_id"].as_str().unwrap();
    let latest = f
        .operator
        .call(Operation::Rotate {
            actor: actor.into(),
        })
        .unwrap();
    let listing = f.operator.call(Operation::Actors {}).unwrap();
    let row = &listing["actors"][0];
    assert_eq!(
        row["credential_file"], latest["credential_file"],
        "operator cannot reconcile lost rotation reply"
    );
    assert_eq!(row["incarnation"], latest["incarnation"]);
    assert!(row.get("credential").is_none());
    let old = Client::peer(
        &f.state,
        &PathBuf::from(first["credential_file"].as_str().unwrap()),
    )
    .unwrap();
    let current = Client::peer(
        &f.state,
        &PathBuf::from(row["credential_file"].as_str().unwrap()),
    )
    .unwrap();
    assert!(old.call(Operation::Receive {}).is_err());
    assert!(current.call(Operation::Receive {}).is_ok());
    assert!(current.call(Operation::Actors {}).is_err());
}

#[test]
fn failed_rotation_publication_preserves_old_credential_incarnation_and_delivery() {
    let f = Fixture::new();
    let a = f.register("a").unwrap();
    let b = f.register("b").unwrap();
    let aid = a["actor_id"].as_str().unwrap();
    let bid = b["actor_id"].as_str().unwrap();
    let sender = Client::peer(
        &f.state,
        &PathBuf::from(a["credential_file"].as_str().unwrap()),
    )
    .unwrap();
    let recipient = Client::peer(
        &f.state,
        &PathBuf::from(b["credential_file"].as_str().unwrap()),
    )
    .unwrap();
    f.operator
        .call(Operation::Route {
            sender: aid.into(),
            recipient: bid.into(),
            allow: true,
        })
        .unwrap();
    let mid = uuid::Uuid::new_v4().to_string();
    sender
        .call(Operation::Send {
            message: hermes_tern_store::MessageInput {
                message_id: mid.clone(),
                recipient: bid.into(),
                kind: "context".into(),
                payload: json!({"text":"fixture"}),
                ttl_seconds: 300,
                correlation_id: None,
            },
        })
        .unwrap();
    assert_eq!(
        recipient.call(Operation::Receive {}).unwrap()["messages"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let before = f.operator.call(Operation::Actors {}).unwrap();
    f.block_publication();
    assert!(
        f.operator
            .call(Operation::Rotate { actor: bid.into() })
            .is_err()
    );
    assert_eq!(
        f.operator.call(Operation::Actors {}).unwrap(),
        before,
        "failed rotation revoked old authority"
    );
    assert_eq!(
        f.operator.call(Operation::Messages {}).unwrap()["messages"][0]["state"],
        "submitted"
    );
    assert_eq!(
        recipient
            .call(Operation::Ack {
                sender: aid.into(),
                message: mid
            })
            .unwrap()["state"],
        "consumed"
    );
}
