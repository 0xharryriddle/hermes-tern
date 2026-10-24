//! Retained private reader controls; fixture values only, never user credentials.
use super::*;
use std::{
    fs,
    os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt, symlink},
    path::PathBuf,
};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = PathBuf::from(std::env::var_os("TMPDIR").unwrap())
            .join(format!("ht-private-reader-{}", uuid::Uuid::new_v4()));
        fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
        create(&path.join("fixture"), "nonsecret-fixture").unwrap();
        Self(path)
    }
    fn file(&self) -> PathBuf {
        self.0.join("fixture")
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let paths: Vec<_> = fs::read_dir(&self.0)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        fs::write(
            self.0.join("ownership.json"),
            serde_json::json!({"root":self.0,"paths":paths,"retention":"retain; no deletion"})
                .to_string(),
        )
        .unwrap();
    }
}

#[test]
fn retained_private_reader_refuses_replaced_inode_and_changed_bytes() {
    for replace in [false, true] {
        let f = Fixture::new();
        let path = f.file();
        let mut read = PrivateFile::open(&path).unwrap();
        assert_eq!(read.text(), "nonsecret-fixture");
        if replace {
            let retained = f.0.join("retained-original");
            fs::rename(&path, &retained).unwrap();
            fs::copy(&retained, &path).unwrap();
            assert_ne!(
                path.metadata().unwrap().ino(),
                retained.metadata().unwrap().ino()
            );
        } else {
            fs::write(&path, b"different-fixture").unwrap();
        }
        assert!(
            read.sync().is_err(),
            "changed private file was acknowledged"
        );
        assert_eq!(read.text(), "nonsecret-fixture");
    }
}

#[test]
fn retained_private_reader_rechecks_permissions_and_link_count() {
    for mode_change in [false, true] {
        let f = Fixture::new();
        let path = f.file();
        let mut read = PrivateFile::open(&path).unwrap();
        if mode_change {
            fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        } else {
            fs::hard_link(&path, f.0.join("retained-link")).unwrap();
        }
        assert!(read.sync().is_err());
        assert!(PrivateFile::open(&path).is_err());
    }
}

#[test]
fn private_reader_is_bounded_and_refuses_symlinks_and_invalid_text() {
    let f = Fixture::new();
    let path = f.file();
    fs::write(&path, b"x".repeat(4096)).unwrap();
    let mut read = PrivateFile::open(&path).unwrap();
    assert_eq!(read.text().len(), 4096);
    read.sync().unwrap();
    fs::write(&path, b"x".repeat(4097)).unwrap();
    assert!(PrivateFile::open(&path).is_err());
    fs::write(&path, [0xff, 0xfe]).unwrap();
    assert!(PrivateFile::open(&path).is_err());
    fs::write(&path, b"nonsecret-fixture").unwrap();
    let alias = f.0.join("alias");
    symlink(&path, &alias).unwrap();
    assert!(PrivateFile::open(&alias).is_err());
    PrivateFile::open(&path).unwrap().sync().unwrap();
}

#[test]
fn exact_private_sync_needs_no_new_file_or_free_publication_slot() {
    let f = Fixture::new();
    let path = f.file();
    let inode = path.metadata().unwrap().ino();
    for i in 0..1023 {
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(f.0.join(format!("retained-{i}")))
            .unwrap();
    }
    assert!(create(&f.0.join("overflow"), "nonsecret").is_err());
    let mut read = PrivateFile::open(&path).unwrap();
    read.sync().unwrap();
    assert_eq!(read.text(), "nonsecret-fixture");
    assert_eq!(path.metadata().unwrap().ino(), inode);
    assert_eq!(fs::read_dir(&f.0).unwrap().count(), 1024);
}
