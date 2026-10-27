//! Offline recovery of an operator-attested intact retained key, never reconstruction.
use hermes_tern_supervisor::operator_key_recovery::recover;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, DirBuilder, OpenOptions},
    io::Write,
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt, symlink},
    path::{Path, PathBuf},
};

const ID: &str = "12345678-1234-4234-8234-123456789abc";

struct Fixture {
    root: PathBuf,
    state: PathBuf,
    key: String,
    digest: String,
}
impl Fixture {
    fn new() -> Self {
        let base = std::env::var_os("TMPDIR").expect("private TMPDIR required");
        let root = PathBuf::from(base).join(format!(
            "operator-key-recovery-test-{}",
            uuid::Uuid::new_v4()
        ));
        DirBuilder::new().mode(0o700).create(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let state = root.join("state");
        DirBuilder::new().mode(0o700).create(&state).unwrap();
        private(&state.join("owner.lock"), b"");
        // Deliberately not SQLite: recovery must inspect metadata, not open a database.
        private(&state.join("coordination.sqlite3"), b"not a database\0");
        let key = "ab".repeat(32);
        let digest = Sha256::digest(key.as_bytes())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        private(&state.join(format!(".pending-{ID}")), key.as_bytes());
        private(&state.join("unrelated"), b"unchanged");
        Self {
            root,
            state,
            key,
            digest,
        }
    }
    fn staged(&self) -> PathBuf {
        self.state.join(format!(".pending-{ID}"))
    }
    fn final_key(&self) -> PathBuf {
        self.state.join("operator.key")
    }
    fn run(&self) -> hermes_tern_core::error::Result<serde_json::Value> {
        recover(&self.state, ID, &self.digest, true)
    }
    fn refuses(&self) {
        let before = inventory(&self.root);
        let result = self.run();
        assert!(result.is_err());
        assert!(before == inventory(&self.root));
        let error = result.unwrap_err().to_string();
        assert!(!error.contains(&self.key));
        assert!(!error.contains(&self.digest));
    }
}

fn private(path: &Path, body: &[u8]) {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .unwrap();
    file.write_all(body).unwrap();
}

type Inventory = Vec<(PathBuf, u64, u64, u32, u32, u64, Vec<u8>)>;
fn inventory(root: &Path) -> Inventory {
    fn visit(root: &Path, path: &Path, rows: &mut Inventory) {
        let m = path.symlink_metadata().unwrap();
        let body = if m.is_file() {
            fs::read(path).unwrap()
        } else if m.is_symlink() {
            fs::read_link(path)
                .unwrap()
                .as_os_str()
                .as_encoded_bytes()
                .to_vec()
        } else {
            Vec::new()
        };
        rows.push((
            path.strip_prefix(root).unwrap().into(),
            m.dev(),
            m.ino(),
            m.mode(),
            m.uid(),
            m.nlink(),
            body,
        ));
        if m.is_dir() {
            let mut entries: Vec<_> = fs::read_dir(path)
                .unwrap()
                .map(|e| e.unwrap().path())
                .collect();
            entries.sort();
            for entry in entries {
                visit(root, &entry, rows);
            }
        }
    }
    let mut rows = Vec::new();
    visit(root, root, &mut rows);
    rows
}

#[test]
fn publishes_same_inode_and_bytes_with_fixed_nonsecret_receipt_and_final_only_retry() {
    let f = Fixture::new();
    let inode = f.staged().metadata().unwrap().ino();
    let db = fs::read(f.state.join("coordination.sqlite3")).unwrap();
    let receipt = f.run().unwrap();
    assert!(
        receipt
            == serde_json::json!({
                "status": "operator_key_recovered", "published": true,
                "new_key_generated": false, "database_opened": false, "runtime_launched": false,
                "key_originality": "operator_attested_not_independently_proven",
                "staging_provenance": "not_established",
                "workers_stopped": "not_established", "authority_policy": "unchanged_key_bytes_only"
            })
    );
    assert!(!f.staged().exists());
    assert!(f.final_key().metadata().unwrap().ino() == inode);
    assert!(fs::read(f.final_key()).unwrap() == f.key.as_bytes());
    assert!(fs::read(f.state.join("coordination.sqlite3")).unwrap() == db);
    assert!(fs::read(f.state.join("unrelated")).unwrap() == b"unchanged");
    let before = inventory(&f.root);
    assert!(f.run().unwrap() == receipt);
    assert!(before == inventory(&f.root));
}

#[test]
fn active_existing_flock_refuses_before_candidate_read() {
    let f = Fixture::new();
    let lock = OpenOptions::new()
        .read(true)
        .open(f.state.join("owner.lock"))
        .unwrap();
    rustix::fs::flock(&lock, rustix::fs::FlockOperation::NonBlockingLockExclusive).unwrap();
    f.refuses();
}

#[test]
fn malformed_ids_digests_and_missing_attestation_refuse_without_effects() {
    let f = Fixture::new();
    let before = inventory(&f.root);
    for id in [
        "",
        "../operator.key",
        ".pending-12345678-1234-4234-8234-123456789abc",
        "12345678123442348234123456789abc",
        "12345678-1234-4234-8234-123456789ABC",
        "12345678-1234-4234-8234-123456789abz",
    ] {
        assert!(recover(&f.state, id, &f.digest, true).is_err());
    }
    for digest in [
        String::new(),
        "a".repeat(63),
        "a".repeat(65),
        "A".repeat(64),
        "z".repeat(64),
        format!("{}\n", f.digest),
    ] {
        assert!(recover(&f.state, ID, &digest, true).is_err());
    }
    assert!(recover(&f.state, ID, &f.digest, false).is_err());
    assert!(before == inventory(&f.root));
}

#[test]
fn absent_state_is_not_created() {
    let f = Fixture::new();
    let missing = f.root.join("missing");
    let before = inventory(&f.root);
    assert!(recover(&missing, ID, &f.digest, true).is_err());
    assert!(before == inventory(&f.root));
}

#[test]
fn both_names_refuse_even_when_bytes_identical() {
    let f = Fixture::new();
    private(&f.final_key(), f.key.as_bytes());
    f.refuses();
}

#[test]
fn neither_name_refuses_without_searching_other_pending_names() {
    let f = Fixture::new();
    fs::rename(
        f.staged(),
        f.state
            .join(".pending-99999999-9999-4999-8999-999999999999"),
    )
    .unwrap();
    f.refuses();
}

#[test]
fn missing_lock_or_journal_refuses_without_recreating_either() {
    for name in ["owner.lock", "coordination.sqlite3"] {
        let f = Fixture::new();
        fs::rename(f.state.join(name), f.state.join(format!("retained-{name}"))).unwrap();
        f.refuses();
    }
}

#[test]
fn wrong_digest_refuses_without_publishing() {
    let f = Fixture::new();
    let before = inventory(&f.root);
    assert!(recover(&f.state, ID, &"0".repeat(64), true).is_err());
    assert!(before == inventory(&f.root));
}

#[test]
fn malformed_secret_bytes_and_oversize_refuse_without_repair() {
    for body in [
        Vec::new(),
        vec![b'a'; 63],
        vec![b'a'; 65],
        vec![b'A'; 64],
        vec![b'g'; 64],
        vec![0xff; 64],
        vec![b'a'; 4097],
        format!("{}\n", "ab".repeat(32)).into_bytes(),
    ] {
        let f = Fixture::new();
        fs::write(f.staged(), body).unwrap();
        f.refuses();
    }
}

#[test]
fn unsafe_permissions_on_files_or_directories_refuse() {
    for name in [
        "owner.lock",
        "coordination.sqlite3",
        ".pending-12345678-1234-4234-8234-123456789abc",
        "state",
        "root",
    ] {
        let f = Fixture::new();
        let path = match name {
            "root" => f.root.clone(),
            "state" => f.state.clone(),
            _ => f.state.join(name),
        };
        fs::set_permissions(
            path,
            fs::Permissions::from_mode(if name == "root" || name == "state" {
                0o755
            } else {
                0o644
            }),
        )
        .unwrap();
        f.refuses();
    }
}

#[test]
fn symlinks_for_each_selected_file_refuse() {
    for name in [
        "owner.lock",
        "coordination.sqlite3",
        ".pending-12345678-1234-4234-8234-123456789abc",
    ] {
        let f = Fixture::new();
        let path = f.state.join(name);
        let retained = f.state.join(format!("retained-{name}"));
        fs::rename(&path, &retained).unwrap();
        symlink(&retained, &path).unwrap();
        f.refuses();
    }
}

#[test]
fn hardlinks_for_each_selected_file_refuse() {
    for name in [
        "owner.lock",
        "coordination.sqlite3",
        ".pending-12345678-1234-4234-8234-123456789abc",
    ] {
        let f = Fixture::new();
        fs::hard_link(f.state.join(name), f.state.join(format!("linked-{name}"))).unwrap();
        f.refuses();
    }
}

#[test]
fn directory_replacements_for_selected_files_refuse() {
    for name in [
        "owner.lock",
        "coordination.sqlite3",
        ".pending-12345678-1234-4234-8234-123456789abc",
    ] {
        let f = Fixture::new();
        let path = f.state.join(name);
        fs::rename(&path, f.state.join(format!("retained-{name}"))).unwrap();
        DirBuilder::new().mode(0o700).create(&path).unwrap();
        f.refuses();
    }
}

#[test]
fn oversized_lock_refuses_but_journal_is_not_subject_to_secret_limit() {
    let f = Fixture::new();
    fs::write(f.state.join("owner.lock"), vec![0u8; 4097]).unwrap();
    f.refuses();
    let f = Fixture::new();
    fs::write(f.state.join("coordination.sqlite3"), vec![0u8; 8192]).unwrap();
    assert!(f.run().is_ok());
}

#[test]
fn final_only_key_is_validated_not_assumed_trustworthy() {
    let f = Fixture::new();
    fs::rename(f.staged(), f.final_key()).unwrap();
    fs::write(f.final_key(), "cd".repeat(32)).unwrap();
    f.refuses();
}

#[test]
fn final_only_symlinks_and_hardlinks_refuse() {
    for hardlink in [false, true] {
        let f = Fixture::new();
        fs::rename(f.staged(), f.final_key()).unwrap();
        if hardlink {
            fs::hard_link(f.final_key(), f.state.join("retained-key")).unwrap();
        } else {
            fs::rename(f.final_key(), f.state.join("retained-key")).unwrap();
            symlink(f.state.join("retained-key"), f.final_key()).unwrap();
        }
        f.refuses();
    }
}

#[test]
fn state_must_be_absolute_canonical_and_not_a_symlink() {
    let f = Fixture::new();
    let before = inventory(&f.root);
    assert!(recover(Path::new("state"), ID, &f.digest, true).is_err());
    assert!(recover(&f.state.join(".."), ID, &f.digest, true).is_err());
    assert!(before == inventory(&f.root));
    let alias = f.root.join("alias");
    symlink(&f.state, &alias).unwrap();
    let before = inventory(&f.root);
    assert!(recover(&alias, ID, &f.digest, true).is_err());
    assert!(before == inventory(&f.root));
    assert!(recover(Path::new("/"), ID, &f.digest, true).is_err());
}

#[test]
fn unrelated_entries_do_not_consume_a_new_publication_slot() {
    let f = Fixture::new();
    for n in 0..1024 {
        private(&f.state.join(format!("unrelated-{n}")), b"retained");
    }
    assert!(f.run().is_ok());
    assert!(fs::read_dir(&f.state).unwrap().count() == 1028);
}
