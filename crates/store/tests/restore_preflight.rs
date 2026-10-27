use hermes_tern_core::error::{Error, Result};
use hermes_tern_store::{MessageInput, Store, StoreLimits};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
};

struct Fixture {
    root: PathBuf,
    state: PathBuf,
    archive: String,
    receipt: Value,
    live: Option<Store>,
}
impl Fixture {
    fn new(unresolved: Option<&str>) -> Self {
        let root = PathBuf::from(std::env::var_os("TMPDIR").unwrap())
            .join(format!("ht-preflight-{}", uuid::Uuid::new_v4()));
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        let state = root.join("state");
        let mut live = Store::open(&state, &root, StoreLimits::default()).unwrap();
        let a = live
            .register("hermes", "fixture", "a", "m", "no-inference")
            .unwrap();
        let b = live
            .register("hermes", "fixture", "b", "m", "no-inference")
            .unwrap();
        live.route(&a.actor_id, &b.actor_id, true).unwrap();
        if let Some(kind) = unresolved {
            if kind == "task" {
                live.claim(&a.credential, &uuid::Uuid::new_v4().to_string(), 300, 100)
                    .unwrap();
            } else {
                let input = MessageInput {
                    message_id: uuid::Uuid::new_v4().to_string(),
                    recipient: b.actor_id,
                    kind: "context".into(),
                    payload: json!({"text":"synthetic"}),
                    ttl_seconds: 300,
                    correlation_id: None,
                };
                live.send(&a.credential, &input, 100).unwrap();
                if kind == "submitted" {
                    live.receive(&b.credential, 101).unwrap();
                }
            }
        }
        let archive = uuid::Uuid::new_v4().to_string();
        let receipt = live.archive(&archive).unwrap();
        Self {
            root,
            state,
            archive,
            receipt,
            live: Some(live),
        }
    }
    fn stop(&mut self) {
        drop(self.live.take());
    }
    fn run(&self, guard: impl FnMut() -> Result<()>) -> Result<Value> {
        Store::preflight_recovery(
            &self.state,
            &self.root,
            &self.archive,
            self.receipt["workspace_id"].as_str().unwrap(),
            self.receipt["database_sha256"].as_str().unwrap(),
            guard,
        )
    }
    fn inventory(&self) -> BTreeMap<PathBuf, (u64, u32, Vec<u8>)> {
        fn scan(path: &Path, out: &mut BTreeMap<PathBuf, (u64, u32, Vec<u8>)>) {
            let m = path.symlink_metadata().unwrap();
            let bytes = if m.is_file() {
                fs::read(path).unwrap()
            } else {
                vec![]
            };
            out.insert(path.to_owned(), (m.ino(), m.mode(), bytes));
            if m.is_dir() {
                for entry in fs::read_dir(path).unwrap() {
                    scan(&entry.unwrap().path(), out);
                }
            }
        }
        let mut out = BTreeMap::new();
        scan(&self.root, &mut out);
        out
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.stop();
        let paths: Vec<_> = self.inventory().into_keys().collect();
        fs::write(
            self.root.join("ownership.json"),
            json!({"retention":"retain;no deletion","paths":paths}).to_string(),
        )
        .unwrap();
    }
}
#[test]
fn offline_preflight_verifies_exact_archive_without_opening_or_recovering_live_database() {
    let mut f = Fixture::new(None);
    f.stop();
    // Deliberately invalid live bytes prove no SQLite open/checkpoint/recovery path.
    fs::write(
        f.state.join("coordination.sqlite3"),
        b"not a live database; preserve exactly",
    )
    .unwrap();
    let before = f.inventory();
    let mut guard_calls = 0;
    let report = f
        .run(|| {
            guard_calls += 1;
            Ok(())
        })
        .unwrap();
    assert_eq!(guard_calls, 2);
    assert_eq!(report["status"], "diagnostic_verified");
    assert_eq!(report["activation_supported"], false);
    assert_eq!(report["live_state_inspected"], false);
    assert_eq!(report["key_originality"], "not_established");
    assert_eq!(report["workers_stopped"], "not_established");
    assert_eq!(
        report["archive"]["database_sha256"],
        f.receipt["database_sha256"]
    );
    assert_eq!(f.inventory(), before);
}
#[test]
fn offline_preflight_refuses_active_ownership_guard_failure_and_missing_lock_without_creating_anything()
 {
    let mut f = Fixture::new(None);
    assert!(f.run(|| Ok(())).is_err());
    f.stop();
    let before = f.inventory();
    assert!(
        f.run(|| Err(Error::Store("fixture key guard refused")))
            .is_err()
    );
    assert_eq!(f.inventory(), before);
    fs::rename(
        f.state.join("owner.lock"),
        f.root.join("retained-owner-lock"),
    )
    .unwrap();
    let before = f.inventory();
    assert!(f.run(|| Ok(())).is_err());
    assert_eq!(f.inventory(), before);
}
#[test]
fn offline_preflight_refuses_wrong_expectations_unsafe_bundle_and_archive_schema() {
    let mut f = Fixture::new(None);
    f.stop();
    let before = f.inventory();
    for (workspace, hash) in [
        (
            uuid::Uuid::new_v4().to_string(),
            f.receipt["database_sha256"].as_str().unwrap().into(),
        ),
        (
            f.receipt["workspace_id"].as_str().unwrap().into(),
            "0".repeat(64),
        ),
        (
            f.receipt["workspace_id"].as_str().unwrap().into(),
            "A".repeat(64),
        ),
    ] {
        assert!(
            Store::preflight_recovery(&f.state, &f.root, &f.archive, &workspace, &hash, || Ok(()))
                .is_err()
        );
    }
    assert_eq!(f.inventory(), before);
    let file = f
        .state
        .join("archives")
        .join(&f.archive)
        .join("manifest.json");
    fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).unwrap();
    let before = f.inventory();
    assert!(f.run(|| Ok(())).is_err());
    assert_eq!(f.inventory(), before);
}
#[test]
fn offline_preflight_refuses_unresolved_mail_and_active_tasks_without_reclassifying_them() {
    for kind in ["queued", "submitted", "task"] {
        let mut f = Fixture::new(Some(kind));
        f.stop();
        let before = f.inventory();
        assert!(
            f.run(|| Ok(())).is_err(),
            "{kind} archive was treated as settled"
        );
        assert_eq!(f.inventory(), before);
    }
}

#[test]
fn offline_preflight_rejects_bundle_directory_substitution_even_with_same_file_inodes() {
    let mut f = Fixture::new(None);
    f.stop();
    let bundle = f.state.join("archives").join(&f.archive);
    let retained = f.root.join("retained-bundle-directory");
    let mut checks = 0;
    let result = f.run(|| {
        checks += 1;
        if checks == 2 {
            fs::rename(&bundle, &retained).unwrap();
            fs::DirBuilder::new().mode(0o700).create(&bundle).unwrap();
            for name in ["coordination.sqlite3", "manifest.json"] {
                fs::rename(retained.join(name), bundle.join(name)).unwrap();
            }
        }
        Ok(())
    });
    assert_eq!(checks, 2, "post-verification guard was not reached");
    assert!(result.is_err(), "replacement bundle directory was accepted");
}
