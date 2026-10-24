//! Kill at the durable-publisher/registry boundary using the real Store transaction.
//! Environment branching is confined to this test executable, not production code.
use hermes_tern_store::{Registration, Store, StoreLimits, private_file};
use serde_json::{Value, json};
use std::{
    fs,
    io::Write,
    os::unix::fs::DirBuilderExt,
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
fn publish(
    root: &std::path::Path,
    r: &Registration,
    before_commit: bool,
) -> hermes_tern_core::error::Result<()> {
    let mut file = private_file(&root.join("candidate.json"))?;
    file.write_all(serde_json::to_string(r).unwrap().as_bytes())?;
    file.sync_all()?;
    fs::File::open(root)?.sync_all()?;
    if before_commit {
        fs::write(root.join("ready"), "before_commit")?;
        loop {
            thread::park();
        }
    }
    Ok(())
}
#[test]
fn killed_publisher_never_activates_an_uncommitted_credential() {
    if let Ok(root) = std::env::var("HT_TEST_PUBLICATION_ROOT") {
        let root = PathBuf::from(root);
        let mode = std::env::var("HT_TEST_PUBLICATION_MODE").unwrap();
        let mut store = Store::open(&root.join("state"), &root, StoreLimits::default()).unwrap();
        let before_commit = mode.ends_with("before");
        if mode.starts_with("rotate") {
            let actor = std::env::var("HT_TEST_PUBLICATION_ACTOR").unwrap();
            store
                .rotate_with_publisher(&actor, |r| publish(&root, r, before_commit))
                .unwrap();
        } else {
            store
                .register_with_publisher("hermes", "fixture", "new", "m", "p", |r| {
                    publish(&root, r, before_commit)
                })
                .unwrap();
        }
        fs::write(root.join("ready"), "after_commit").unwrap();
        loop {
            thread::park();
        }
    }
    for mode in [
        "register-before",
        "register-after",
        "rotate-before",
        "rotate-after",
    ] {
        let root = PathBuf::from(std::env::var_os("TMPDIR").unwrap())
            .join(format!("ht-publication-kill-{}", uuid::Uuid::new_v4()));
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        let state = root.join("state");
        let mut s = Store::open(&state, &root, StoreLimits::default()).unwrap();
        let old = s.register("hermes", "fixture", "old", "m", "p").unwrap();
        drop(s);
        let mut child = OwnedChild(
            Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "killed_publisher_never_activates_an_uncommitted_credential",
                    "--nocapture",
                ])
                .env("HT_TEST_PUBLICATION_ROOT", &root)
                .env("HT_TEST_PUBLICATION_MODE", mode)
                .env("HT_TEST_PUBLICATION_ACTOR", &old.actor_id)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        while !root.join("ready").exists() {
            assert!(
                child.0.try_wait().unwrap().is_none(),
                "test worker exited before boundary"
            );
            assert!(
                Instant::now() < deadline,
                "test worker did not reach publication boundary"
            );
            thread::sleep(Duration::from_millis(5));
        }
        child.0.kill().unwrap();
        child.0.wait().unwrap();
        let candidate: Value =
            serde_json::from_slice(&fs::read(root.join("candidate.json")).unwrap()).unwrap();
        let credential = candidate["credential"].as_str().unwrap();
        let mut s = Store::open(&state, &root, StoreLimits::default()).unwrap();
        let committed = mode.ends_with("after");
        assert_eq!(
            s.claim(credential, &uuid::Uuid::new_v4().to_string(), 100, 300)
                .is_ok(),
            committed,
            "candidate authority at {mode}"
        );
        let old_is_valid = !mode.starts_with("rotate") || !committed;
        assert_eq!(
            s.claim(&old.credential, &uuid::Uuid::new_v4().to_string(), 100, 300)
                .is_ok(),
            old_is_valid,
            "old authority at {mode}"
        );
        let expected_actors = if mode == "register-after" { 2 } else { 1 };
        assert_eq!(s.actors().unwrap().len(), expected_actors);
        drop(s);
        let mut paths = vec![root.clone(), state.clone(), root.join("ownership.json")];
        paths.extend(fs::read_dir(&root).unwrap().map(|r| r.unwrap().path()));
        paths.extend(fs::read_dir(&state).unwrap().map(|r| r.unwrap().path()));
        fs::write(
            root.join("ownership.json"),
            json!({"retention":"retain; no deletion","paths":paths}).to_string(),
        )
        .unwrap();
    }
}
