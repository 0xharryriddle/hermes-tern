//! Fault controls for private archive directory publication, not kernel power loss.
use super::{archive::publish, *};

fn root() -> PathBuf {
    let path = PathBuf::from(std::env::var_os("TMPDIR").unwrap())
        .join(format!("ht-archive-publish-{}", Uuid::new_v4()));
    fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
    path
}
fn inventory(root: &Path) {
    let mut paths = vec![root.to_owned(), root.join("ownership.json")];
    for e in fs::read_dir(root).unwrap() {
        let p = e.unwrap().path();
        paths.push(p.clone());
        if p.is_dir() {
            paths.extend(fs::read_dir(p).unwrap().map(|e| e.unwrap().path()));
        }
    }
    fs::write(
        root.join("ownership.json"),
        json!({"retention":"retain; no deletion","paths":paths}).to_string(),
    )
    .unwrap();
}
#[test]
fn failed_archive_writer_leaves_only_private_staging_and_no_final_bundle() {
    let root = root();
    let id = Uuid::new_v4().to_string();
    let final_path = root.join(&id);
    let mut staging = None;
    let outcome = publish(&root, &id, |dir| {
        staging = Some(dir.to_owned());
        assert!(!final_path.exists());
        let path = dir.join("coordination.sqlite3");
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
            .unwrap();
        fs::write(path, b"partial fixture").unwrap();
        Err(Error::Io(std::io::ErrorKind::StorageFull))
    });
    assert!(outcome.is_err());
    assert!(!final_path.exists());
    let staging = staging.unwrap();
    assert_eq!(staging.metadata().unwrap().mode() & 0o777, 0o700);
    assert_eq!(
        fs::read(staging.join("coordination.sqlite3")).unwrap(),
        b"partial fixture"
    );
    inventory(&root);
}
#[test]
fn racing_final_archive_preserves_winner_inode_without_exposing_candidate() {
    let root = root();
    let id = Uuid::new_v4().to_string();
    let final_path = root.join(&id);
    let mut winner = 0;
    let result = publish(&root, &id, |_dir| {
        assert!(!final_path.exists());
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&final_path)
            .unwrap();
        let path = final_path.join("winner");
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
            .unwrap();
        fs::write(path, b"winner").unwrap();
        winner = final_path.metadata().unwrap().ino();
        Ok(())
    });
    assert!(result.is_err());
    assert_eq!(final_path.metadata().unwrap().ino(), winner);
    assert_eq!(fs::read(final_path.join("winner")).unwrap(), b"winner");
    inventory(&root);
}

fn image_fixture() -> (PathBuf, Store, String) {
    let root = root();
    let mut store = Store::open(&root.join("state"), &root, StoreLimits::default()).unwrap();
    let id = Uuid::new_v4().to_string();
    store.archive(&id).unwrap();
    (root, store, id)
}
fn image_inventory(root: &Path) {
    fn collect(path: &Path, paths: &mut Vec<PathBuf>) {
        paths.push(path.to_owned());
        if path.is_dir() {
            for entry in fs::read_dir(path).unwrap() {
                collect(&entry.unwrap().path(), paths);
            }
        }
    }
    let mut paths = vec![root.join("ownership.json")];
    collect(root, &mut paths);
    fs::write(
        root.join("ownership.json"),
        json!({"retention":"retain; no deletion","paths":paths}).to_string(),
    )
    .unwrap();
}

#[test]
fn verified_image_is_read_only_and_does_not_follow_a_replaced_database_path() {
    let (root, store, id) = image_fixture();
    let mut archive = store.verified_archive(&id).unwrap();
    let db = archive.database();
    assert!(
        db.execute("UPDATE meta SET value='4' WHERE key='clock'", [])
            .is_err()
    );
    db.pragma_update(None, "query_only", false).unwrap();
    assert!(
        db.execute("UPDATE meta SET value='4' WHERE key='clock'", [])
            .is_err(),
        "the deserialized image must be read-only even without query_only"
    );
    let path = store
        .state_dir
        .join("archives")
        .join(&id)
        .join("coordination.sqlite3");
    let retained = root.join("retained-original.sqlite3");
    fs::rename(&path, &retained).unwrap();
    fs::copy(&retained, &path).unwrap();
    assert_ne!(
        path.metadata().unwrap().ino(),
        retained.metadata().unwrap().ino()
    );
    assert!(
        archive.recheck().is_err(),
        "same bytes under a new inode passed recheck"
    );
    let other = Connection::open(&path).unwrap();
    other
        .execute("UPDATE meta SET value='7' WHERE key='clock'", [])
        .unwrap();
    drop(other);
    assert_eq!(
        archive
            .database()
            .query_row("SELECT value FROM meta WHERE key='clock'", [], |r| r
                .get::<_, String>(0))
            .unwrap(),
        "0"
    );
    drop(archive);
    drop(store);
    image_inventory(&root);
}

#[test]
fn verified_image_is_stable_across_in_place_changes_but_final_recheck_refuses() {
    let (root, store, id) = image_fixture();
    let mut archive = store.verified_archive(&id).unwrap();
    let path = store
        .state_dir
        .join("archives")
        .join(&id)
        .join("coordination.sqlite3");
    let inode = path.metadata().unwrap().ino();
    let other = Connection::open(&path).unwrap();
    other
        .execute("UPDATE meta SET value='9' WHERE key='clock'", [])
        .unwrap();
    drop(other);
    assert_eq!(path.metadata().unwrap().ino(), inode);
    assert_eq!(
        archive
            .database()
            .query_row("SELECT value FROM meta WHERE key='clock'", [], |r| r
                .get::<_, String>(0))
            .unwrap(),
        "0"
    );
    assert!(archive.recheck().is_err());
    drop(archive);
    drop(store);
    image_inventory(&root);
}

#[test]
fn verified_archive_rechecks_manifest_identity_bytes_and_private_file_metadata() {
    for mutation in [
        "manifest-bytes",
        "manifest-inode",
        "database-mode",
        "database-hardlink",
        "manifest-mode",
        "manifest-hardlink",
    ] {
        let (root, store, id) = image_fixture();
        let mut archive = store.verified_archive(&id).unwrap();
        archive.recheck().unwrap();
        let dir = store.state_dir.join("archives").join(&id);
        let path = dir.join("coordination.sqlite3");
        let manifest = dir.join("manifest.json");
        match mutation {
            "manifest-bytes" => fs::write(&manifest, b"{}").unwrap(),
            "manifest-inode" => {
                let retained = root.join("retained-manifest.json");
                fs::rename(&manifest, &retained).unwrap();
                fs::copy(&retained, &manifest).unwrap();
            }
            "database-mode" => {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
            }
            "database-hardlink" => {
                fs::hard_link(&path, root.join("retained-database-link")).unwrap()
            }
            "manifest-mode" => {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&manifest, fs::Permissions::from_mode(0o644)).unwrap();
            }
            _ => fs::hard_link(&manifest, root.join("retained-manifest-link")).unwrap(),
        }
        assert!(archive.recheck().is_err(), "accepted {mutation}");
        drop(archive);
        drop(store);
        image_inventory(&root);
    }
}

#[test]
fn verified_image_rejects_truncated_and_oversized_files_before_loading() {
    let (root, store, id) = image_fixture();
    let path = store
        .state_dir
        .join("archives")
        .join(&id)
        .join("coordination.sqlite3");
    let original = fs::read(&path).unwrap();
    fs::write(&path, &original[..99]).unwrap();
    assert!(store.verified_archive(&id).is_err());
    fs::write(&path, &original).unwrap();
    fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .unwrap()
        .set_len(256 * 1024 * 1024 + 1)
        .unwrap();
    assert!(store.verified_archive(&id).is_err());
    fs::write(&path, &original).unwrap();
    store.verified_archive(&id).unwrap().recheck().unwrap();
    drop(store);
    image_inventory(&root);
}
