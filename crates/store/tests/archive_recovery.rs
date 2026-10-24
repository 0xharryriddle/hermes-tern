use hermes_tern_store::{Store, StoreLimits};
use serde_json::json;
use std::{
    fs,
    os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt, symlink},
    path::{Path, PathBuf},
};

struct Fixture {
    root: PathBuf,
    state: PathBuf,
    store: Option<Store>,
    archive: String,
    staging: String,
    hash: String,
}
impl Fixture {
    fn new() -> Self {
        let root = PathBuf::from(std::env::var_os("TMPDIR").unwrap())
            .join(format!("ht-retained-archive-{}", uuid::Uuid::new_v4()));
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        let state = root.join("state");
        let mut store = Store::open(&state, &root, StoreLimits::default()).unwrap();
        let archive = uuid::Uuid::new_v4().to_string();
        let staging = uuid::Uuid::new_v4().to_string();
        let receipt = store.archive(&archive).unwrap();
        let hash = receipt["database_sha256"].as_str().unwrap().to_owned();
        fs::rename(
            state.join("archives").join(&archive),
            state.join("archives").join(format!(".pending-{staging}")),
        )
        .unwrap();
        Self {
            root,
            state,
            store: Some(store),
            archive,
            staging,
            hash,
        }
    }
    fn pending(&self) -> PathBuf {
        self.state
            .join("archives")
            .join(format!(".pending-{}", self.staging))
    }
    fn final_path(&self) -> PathBuf {
        self.state.join("archives").join(&self.archive)
    }
    fn recover(&mut self) -> hermes_tern_core::error::Result<serde_json::Value> {
        self.store
            .as_mut()
            .unwrap()
            .recover_archive(&self.staging, &self.archive, &self.hash)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        drop(self.store.take());
        fn collect(path: &Path, out: &mut Vec<PathBuf>) {
            out.push(path.to_owned());
            if path.symlink_metadata().unwrap().is_dir() {
                for e in fs::read_dir(path).unwrap() {
                    collect(&e.unwrap().path(), out);
                }
            }
        }
        let mut paths = vec![self.root.join("ownership.json")];
        collect(&self.root, &mut paths);
        fs::write(
            self.root.join("ownership.json"),
            json!({"retention":"retain; no deletion","paths":paths}).to_string(),
        )
        .unwrap();
    }
}

#[test]
fn recover_retained_archive_preserves_original_bytes_and_source_across_retry_restart() {
    let mut f = Fixture::new();
    let dir_inode = f.pending().metadata().unwrap().ino();
    let original: Vec<_> = ["coordination.sqlite3", "manifest.json"]
        .into_iter()
        .map(|n| {
            let p = f.pending().join(n);
            (n, fs::read(&p).unwrap(), p.metadata().unwrap().ino())
        })
        .collect();
    let observed = f
        .store
        .as_ref()
        .unwrap()
        .inspect_staged_archive(&f.staging, &f.archive)
        .unwrap();
    assert_eq!(observed["status"], "staged_verified");
    assert_eq!(observed["published"], false);
    assert_eq!(observed["database_sha256"], f.hash);
    let s = f.store.as_mut().unwrap();
    s.register("hermes", "fixture", "after-capture", "m", "p")
        .unwrap();
    let before = s.observation_snapshot().unwrap();
    let receipt = f.recover().unwrap();
    assert_eq!(receipt["status"], "verified");
    assert_eq!(receipt["source_pruned"], false);
    assert_eq!(receipt["restoration_supported"], false);
    assert!(!f.pending().exists());
    assert_eq!(f.final_path().metadata().unwrap().ino(), dir_inode);
    assert_eq!(
        f.store.as_mut().unwrap().observation_snapshot().unwrap(),
        before
    );
    for (n, bytes, ino) in original {
        let p = f.final_path().join(n);
        assert_eq!(fs::read(&p).unwrap(), bytes);
        assert_eq!(p.metadata().unwrap().ino(), ino);
    }
    assert_eq!(f.recover().unwrap(), receipt);
    drop(f.store.take());
    f.store = Some(Store::open(&f.state, &f.root, StoreLimits::default()).unwrap());
    assert_eq!(f.recover().unwrap(), receipt);
    assert_eq!(
        f.store
            .as_ref()
            .unwrap()
            .inspect_archive(&f.archive)
            .unwrap(),
        receipt
    );
    assert!(
        f.store
            .as_ref()
            .unwrap()
            .inspect_staged_archive(&f.staging, &f.archive)
            .is_err()
    );
}

#[test]
fn recovery_moves_existing_entry_at_archive_capacity_without_removing_other_files() {
    let mut f = Fixture::new();
    for i in 0..7 {
        fs::DirBuilder::new()
            .mode(0o700)
            .create(f.state.join("archives").join(format!("retained-{i}")))
            .unwrap();
    }
    assert!(
        f.store
            .as_mut()
            .unwrap()
            .archive(&uuid::Uuid::new_v4().to_string())
            .is_err()
    );
    f.recover().unwrap();
    assert_eq!(fs::read_dir(f.state.join("archives")).unwrap().count(), 8);
    for i in 0..7 {
        assert!(
            f.state
                .join("archives")
                .join(format!("retained-{i}"))
                .is_dir()
        );
    }
}

#[test]
fn recovery_rejects_bad_ids_hashes_and_wrong_archive_without_changing_staging() {
    let mut f = Fixture::new();
    let pending = f.pending();
    let inode = pending.metadata().unwrap().ino();
    let before = f.store.as_mut().unwrap().observation_snapshot().unwrap();
    for bad in [
        "",
        "../escape",
        ".pending-00000000-0000-0000-0000-000000000000",
        "00000000-0000-0000-0000-00000000000A",
    ] {
        let s = f.store.as_mut().unwrap();
        assert!(s.inspect_staged_archive(bad, &f.archive).is_err());
        assert!(s.recover_archive(bad, &f.archive, &f.hash).is_err());
        assert!(s.recover_archive(&f.staging, bad, &f.hash).is_err());
    }
    for bad in [
        "".to_owned(),
        "0".repeat(63),
        "0".repeat(64),
        "A".repeat(64),
        "g".repeat(64),
    ] {
        assert!(
            f.store
                .as_mut()
                .unwrap()
                .recover_archive(&f.staging, &f.archive, &bad)
                .is_err()
        );
    }
    assert!(
        f.store
            .as_mut()
            .unwrap()
            .recover_archive(&f.staging, &uuid::Uuid::new_v4().to_string(), &f.hash)
            .is_err()
    );
    assert_eq!(pending.metadata().unwrap().ino(), inode);
    assert!(!f.final_path().exists());
    assert_eq!(
        f.store.as_mut().unwrap().observation_snapshot().unwrap(),
        before
    );
    f.recover().unwrap();
}

#[test]
fn recovery_refuses_partial_corrupt_foreign_and_unsafe_bundles_without_repair() {
    for variant in [
        "partial", "manifest", "database", "foreign", "extra", "mode", "hardlink", "symlink",
    ] {
        let mut f = Fixture::new();
        let dir = f.pending();
        let database = dir.join("coordination.sqlite3");
        let manifest = dir.join("manifest.json");
        match variant {
            "partial" => fs::rename(&manifest, f.root.join("retained-manifest.json")).unwrap(),
            "manifest" => fs::write(&manifest, b"{}").unwrap(),
            "database" => fs::write(&database, b"corrupt").unwrap(),
            "foreign" => {
                let mut v: serde_json::Value =
                    serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
                v["workspace_id"] = json!(uuid::Uuid::new_v4().to_string());
                fs::write(&manifest, v.to_string()).unwrap();
            }
            "extra" => fs::write(dir.join("unexpected"), b"retained").unwrap(),
            "mode" => fs::set_permissions(&database, fs::Permissions::from_mode(0o644)).unwrap(),
            "hardlink" => fs::hard_link(&database, f.root.join("retained-link")).unwrap(),
            _ => {
                let retained = f.root.join("retained-database");
                fs::rename(&database, &retained).unwrap();
                symlink(retained, &database).unwrap();
            }
        }
        let inode = dir.metadata().unwrap().ino();
        let before = f.store.as_mut().unwrap().observation_snapshot().unwrap();
        assert!(
            f.store
                .as_ref()
                .unwrap()
                .inspect_staged_archive(&f.staging, &f.archive)
                .is_err(),
            "{variant}"
        );
        assert!(f.recover().is_err(), "{variant}");
        assert!(dir.exists());
        assert_eq!(dir.metadata().unwrap().ino(), inode);
        assert!(!f.final_path().exists());
        assert_eq!(
            f.store.as_mut().unwrap().observation_snapshot().unwrap(),
            before
        );
    }
}

#[test]
fn recovery_never_chooses_between_staging_and_an_existing_final_target() {
    let mut f = Fixture::new();
    fs::DirBuilder::new()
        .mode(0o700)
        .create(f.final_path())
        .unwrap();
    fs::write(f.final_path().join("winner"), b"winner").unwrap();
    let inode = f.final_path().metadata().unwrap().ino();
    let staged_inode = f.pending().metadata().unwrap().ino();
    assert!(f.recover().is_err());
    assert_eq!(f.final_path().metadata().unwrap().ino(), inode);
    assert_eq!(f.pending().metadata().unwrap().ino(), staged_inode);
    assert_eq!(fs::read(f.final_path().join("winner")).unwrap(), b"winner");
}

#[test]
fn recovery_final_only_retry_requires_intact_expected_digest_and_never_recaptures() {
    let mut f = Fixture::new();
    let receipt = f.recover().unwrap();
    let before = f.store.as_mut().unwrap().observation_snapshot().unwrap();
    assert!(
        f.store
            .as_mut()
            .unwrap()
            .recover_archive(&f.staging, &f.archive, &"0".repeat(64))
            .is_err()
    );
    assert_eq!(f.recover().unwrap(), receipt);
    fs::write(f.final_path().join("manifest.json"), b"{}").unwrap();
    assert!(f.recover().is_err());
    assert_eq!(
        fs::read(f.final_path().join("manifest.json")).unwrap(),
        b"{}"
    );
    assert_eq!(fs::read_dir(f.state.join("archives")).unwrap().count(), 1);
    assert_eq!(
        f.store.as_mut().unwrap().observation_snapshot().unwrap(),
        before
    );
}
