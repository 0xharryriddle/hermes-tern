use hermes_tern_store::{MessageInput, Store, StoreLimits};
use rusqlite::Connection;
use serde_json::{Value, json};
use std::{
    fs,
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt, symlink},
    path::{Path, PathBuf},
};

struct Fixture {
    root: PathBuf,
    state: PathBuf,
    store: Option<Store>,
}
impl Fixture {
    fn new() -> Self {
        let root = PathBuf::from(std::env::var_os("TMPDIR").unwrap())
            .join(format!("ht-archive-store-{}", uuid::Uuid::new_v4()));
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        let state = root.join("state");
        let store = Store::open(&state, &root, StoreLimits::default()).unwrap();
        Self {
            root,
            state,
            store: Some(store),
        }
    }
    fn reopen(&mut self) {
        drop(self.store.take());
        self.store = Some(Store::open(&self.state, &self.root, StoreLimits::default()).unwrap());
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        drop(self.store.take());
        fn inventory(p: &Path, out: &mut Vec<PathBuf>) {
            out.push(p.to_owned());
            if p.symlink_metadata().unwrap().is_dir() {
                for e in fs::read_dir(p).unwrap() {
                    inventory(&e.unwrap().path(), out);
                }
            }
        }
        let mut paths = vec![];
        inventory(&self.root, &mut paths);
        paths.push(self.root.join("ownership.json"));
        fs::write(
            self.root.join("ownership.json"),
            json!({"retention":"retain; no deletion","paths":paths}).to_string(),
        )
        .unwrap();
    }
}
fn id() -> String {
    uuid::Uuid::new_v4().to_string()
}
fn bundle(f: &Fixture, id: &str) -> PathBuf {
    f.state.join("archives").join(id)
}
fn private_bytes(path: &Path, body: &[u8]) {
    use std::io::Write;
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(path)
        .unwrap();
    file.write_all(body).unwrap();
}

#[test]
fn archive_preserves_wal_delivery_and_exact_retry_after_source_advances() {
    let mut f = Fixture::new();
    let aid = id();
    let s = f.store.as_mut().unwrap();
    let a = s.register("hermes", "fixture", "a", "m", "p").unwrap();
    let b = s.register("hermes", "fixture", "b", "m", "p").unwrap();
    s.route(&a.actor_id, &b.actor_id, true).unwrap();
    let message = MessageInput {
        message_id: id(),
        recipient: b.actor_id.clone(),
        kind: "context".into(),
        payload: json!({"text":"fixture"}),
        ttl_seconds: 300,
        correlation_id: None,
    };
    s.send(&a.credential, &message, 100).unwrap();
    s.receive(&b.credential, 101).unwrap();
    let before = s.observation_snapshot().unwrap();
    let receipt = s.archive(&aid).unwrap();
    assert_eq!(s.observation_snapshot().unwrap(), before);
    let dir = PathBuf::from(receipt["archive_dir"].as_str().unwrap());
    let path = dir.join("coordination.sqlite3");
    let db =
        Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    assert_eq!(
        db.query_row("SELECT state FROM messages", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "submitted"
    );
    assert_eq!(
        db.query_row(
            "SELECT seq FROM sqlite_sequence WHERE name='events'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap()
        .to_string(),
        before["event_watermark"]
    );
    drop(db);
    let original = fs::read(&path).unwrap();
    let ino = path.metadata().unwrap().ino();
    s.ack(&b.credential, &a.actor_id, &message.message_id, 102)
        .unwrap();
    assert_eq!(s.archive(&aid).unwrap(), receipt);
    f.reopen();
    let s = f.store.as_mut().unwrap();
    assert_eq!(s.inspect_archive(&aid).unwrap(), receipt);
    assert_eq!(s.archive(&aid).unwrap(), receipt);
    assert_eq!(fs::read(&path).unwrap(), original);
    assert_eq!(path.metadata().unwrap().ino(), ino);
    assert_eq!(s.messages().unwrap()[0]["state"], "consumed");
    assert_eq!(dir.metadata().unwrap().mode() & 0o777, 0o700);
    for entry in fs::read_dir(&dir).unwrap() {
        assert_eq!(entry.unwrap().metadata().unwrap().mode() & 0o777, 0o600);
    }
}

#[test]
fn archive_preserves_missing_tail_watermark_instead_of_fabricating_history() {
    let mut f = Fixture::new();
    let s = f.store.as_mut().unwrap();
    let a = s.register("hermes", "fixture", "a", "m", "p").unwrap();
    s.rotate(&a.actor_id).unwrap();
    let db = Connection::open(f.state.join("coordination.sqlite3")).unwrap();
    assert_eq!(
        db.execute("DELETE FROM events WHERE sequence=2", [])
            .unwrap(),
        1
    );
    let before = s.observation_snapshot().unwrap();
    assert_eq!(before["event_watermark"], "2");
    assert_eq!(before["event_replay_complete"], false);
    let aid = id();
    let receipt = s.archive(&aid).unwrap();
    assert_eq!(receipt["event_watermark"], "2");
    assert_eq!(receipt["counts"]["events"], "1");
    assert_eq!(s.observation_snapshot().unwrap(), before);
    let copy = Connection::open_with_flags(
        bundle(&f, &aid).join("coordination.sqlite3"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    assert_eq!(
        copy.query_row(
            "SELECT seq FROM sqlite_sequence WHERE name='events'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        2
    );
    assert_eq!(
        copy.query_row("SELECT max(sequence) FROM events", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    drop(copy);
    drop(db);
    f.reopen();
    assert_eq!(
        f.store.as_ref().unwrap().inspect_archive(&aid).unwrap(),
        receipt
    );
}

#[test]
fn failed_snapshot_validation_retains_staging_and_never_changes_source() {
    let mut f = Fixture::new();
    let aid = id();
    let db = Connection::open(f.state.join("coordination.sqlite3")).unwrap();
    db.execute(
        "UPDATE meta SET value='invalid-clock' WHERE key='clock'",
        [],
    )
    .unwrap();
    assert!(f.store.as_mut().unwrap().archive(&aid).is_err());
    let root = f.state.join("archives");
    let staged: Vec<_> = fs::read_dir(&root)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(staged.len(), 1);
    assert!(staged[0].join("coordination.sqlite3").is_file());
    assert!(!staged[0].join("manifest.json").exists());
    assert!(!bundle(&f, &aid).exists());
    assert_eq!(
        db.query_row("SELECT value FROM meta WHERE key='clock'", [], |r| r
            .get::<_, String>(0))
            .unwrap(),
        "invalid-clock"
    );
    db.execute("UPDATE meta SET value='0' WHERE key='clock'", [])
        .unwrap();
    let receipt = f.store.as_mut().unwrap().archive(&aid).unwrap();
    assert_eq!(receipt["clock_watermark"], "0");
    assert!(staged[0].join("coordination.sqlite3").is_file());
    assert_eq!(fs::read_dir(root).unwrap().count(), 2);
}

#[test]
fn oversized_database_refuses_without_reading_or_replacing_it() {
    let mut f = Fixture::new();
    let aid = id();
    f.store.as_mut().unwrap().archive(&aid).unwrap();
    let path = bundle(&f, &aid).join("coordination.sqlite3");
    let ino = path.metadata().unwrap().ino();
    fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .unwrap()
        .set_len(256 * 1024 * 1024 + 1)
        .unwrap();
    assert!(f.store.as_ref().unwrap().inspect_archive(&aid).is_err());
    assert!(f.store.as_mut().unwrap().archive(&aid).is_err());
    assert_eq!(path.metadata().unwrap().ino(), ino);
    assert_eq!(path.metadata().unwrap().len(), 256 * 1024 * 1024 + 1);
}

#[test]
fn archive_rejects_invalid_ids_without_creating_directories() {
    let mut f = Fixture::new();
    for value in [
        "",
        "../escape",
        "not-a-uuid",
        "00000000-0000-0000-0000-00000000000A",
    ] {
        assert!(f.store.as_mut().unwrap().archive(value).is_err());
        assert!(f.store.as_mut().unwrap().inspect_archive(value).is_err());
    }
    assert!(f.store.as_mut().unwrap().inspect_archive(&id()).is_err());
    assert!(!f.state.join("archives").exists());
}

#[test]
fn corrupt_partial_or_foreign_archives_are_never_replaced() {
    for mutation in [
        "database",
        "manifest",
        "count",
        "foreign",
        "duplicate",
        "extra",
        "oversized",
    ] {
        let mut f = Fixture::new();
        let aid = id();
        f.store.as_mut().unwrap().archive(&aid).unwrap();
        let dir = bundle(&f, &aid);
        let path = dir.join("manifest.json");
        let mut manifest: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        match mutation {
            "database" => {
                let db = Connection::open(dir.join("coordination.sqlite3")).unwrap();
                db.execute("UPDATE meta SET value='9' WHERE key='clock'", [])
                    .unwrap();
            }
            "manifest" => fs::write(&path, "{}").unwrap(),
            "count" => {
                manifest["counts"]["messages"] = json!("99");
                fs::write(&path, manifest.to_string()).unwrap();
            }
            "foreign" => {
                manifest["workspace_id"] = json!(id());
                fs::write(&path, manifest.to_string()).unwrap();
            }
            "duplicate" => fs::write(
                &path,
                format!("{{\"archive_id\":\"{aid}\",{}", &manifest.to_string()[1..]),
            )
            .unwrap(),
            "extra" => private_bytes(&dir.join("unexpected"), b"retained"),
            "oversized" => fs::write(&path, " ".repeat(4097)).unwrap(),
            _ => unreachable!(),
        }
        let before = fs::read(&path).unwrap();
        assert!(
            f.store.as_mut().unwrap().inspect_archive(&aid).is_err(),
            "{mutation}"
        );
        assert!(
            f.store.as_mut().unwrap().archive(&aid).is_err(),
            "{mutation}"
        );
        assert_eq!(fs::read(&path).unwrap(), before, "replaced {mutation}");
    }
    let mut f = Fixture::new();
    let aid = id();
    fs::DirBuilder::new()
        .mode(0o700)
        .create(f.state.join("archives"))
        .unwrap();
    fs::DirBuilder::new()
        .mode(0o700)
        .create(bundle(&f, &aid))
        .unwrap();
    assert!(f.store.as_mut().unwrap().archive(&aid).is_err());
    assert_eq!(fs::read_dir(bundle(&f, &aid)).unwrap().count(), 0);
}

#[test]
fn archive_refuses_symlinks_hardlinks_and_nonprivate_paths() {
    for variant in [
        "root-symlink",
        "final-symlink",
        "file-symlink",
        "hardlink",
        "public",
    ] {
        let mut f = Fixture::new();
        let aid = id();
        let outside = f.root.join("outside");
        fs::DirBuilder::new().mode(0o700).create(&outside).unwrap();
        let arch = f.state.join("archives");
        if variant == "root-symlink" {
            symlink(&outside, &arch).unwrap();
        } else {
            fs::DirBuilder::new().mode(0o700).create(&arch).unwrap();
            if variant == "final-symlink" {
                symlink(&outside, bundle(&f, &aid)).unwrap();
            } else {
                f.store.as_mut().unwrap().archive(&aid).unwrap();
                let dir = bundle(&f, &aid);
                let db = dir.join("coordination.sqlite3");
                if variant == "hardlink" {
                    fs::hard_link(&db, outside.join("linked")).unwrap();
                }
                if variant == "public" {
                    fs::set_permissions(&db, fs::Permissions::from_mode(0o644)).unwrap();
                }
                if variant == "file-symlink" {
                    let original = outside.join("original");
                    fs::rename(&db, &original).unwrap();
                    symlink(&original, &db).unwrap();
                }
            }
        }
        assert!(
            f.store.as_mut().unwrap().archive(&aid).is_err(),
            "{variant}"
        );
        assert!(
            f.store.as_mut().unwrap().inspect_archive(&aid).is_err(),
            "{variant}"
        );
    }
}

#[test]
fn retained_staging_counts_toward_entry_and_byte_capacity_but_exact_retries_still_work() {
    let mut f = Fixture::new();
    let aid = id();
    let receipt = f.store.as_mut().unwrap().archive(&aid).unwrap();
    let root = f.state.join("archives");
    for i in 0..7 {
        fs::DirBuilder::new()
            .mode(0o700)
            .create(root.join(format!(".pending-{i}")))
            .unwrap();
    }
    assert!(f.store.as_mut().unwrap().archive(&id()).is_err());
    assert_eq!(fs::read_dir(&root).unwrap().count(), 8);
    assert_eq!(f.store.as_mut().unwrap().archive(&aid).unwrap(), receipt);
    let mut g = Fixture::new();
    let root = g.state.join("archives");
    fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
    let p = root.join("retained-partial");
    private_bytes(&p, b"");
    fs::OpenOptions::new()
        .write(true)
        .open(&p)
        .unwrap()
        .set_len(512 * 1024 * 1024)
        .unwrap();
    assert!(g.store.as_mut().unwrap().archive(&id()).is_err());
    assert_eq!(fs::read_dir(root).unwrap().count(), 1);
}
