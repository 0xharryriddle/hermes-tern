//! The caller's authority guard runs under ownership and before any SQLite open.
use hermes_tern_core::error::{Error, Result};
use hermes_tern_store::{Store, StoreLimits};
use std::{cell::Cell, fs, path::PathBuf};
use uuid::Uuid;

fn fixture() -> (PathBuf, PathBuf) {
    let root = PathBuf::from(std::env::var_os("TMPDIR").expect("private TMPDIR required"))
        .join(format!("ht-startup-guard-{}", Uuid::new_v4()));
    fs::create_dir(&root).unwrap();
    let state = root.join("state");
    (root, state)
}

#[test]
fn failed_fresh_guard_never_creates_database_and_releases_owner_lock() {
    let (root, state) = fixture();
    let calls = Cell::new(0);
    let r = Store::open_with_startup_guard(
        &state,
        &root,
        StoreLimits::default(),
        |exists| -> Result<()> {
            assert!(!exists);
            assert!(state.join("owner.lock").is_file());
            calls.set(calls.get() + 1);
            Err(Error::Store("fixture authority refusal"))
        },
    );
    assert!(r.is_err());
    assert_eq!(calls.get(), 1);
    assert!(!state.join("coordination.sqlite3").exists());
    let (_store, resource) =
        Store::open_with_startup_guard(&state, &root, StoreLimits::default(), |exists| {
            assert!(!exists);
            Ok("validated resource")
        })
        .unwrap();
    assert_eq!(resource, "validated resource");
}

#[test]
fn a_competing_owner_cannot_enter_guard() {
    let (root, state) = fixture();
    let owner = Store::open(&state, &root, StoreLimits::default()).unwrap();
    let entered = Cell::new(false);
    let r = Store::open_with_startup_guard(&state, &root, StoreLimits::default(), |_| {
        entered.set(true);
        Ok(())
    });
    assert!(r.is_err());
    assert!(!entered.get());
    drop(owner);
    assert!(
        Store::open_with_startup_guard(&state, &root, StoreLimits::default(), |exists| {
            assert!(exists);
            Ok(())
        })
        .is_ok()
    );
}

#[test]
fn existing_empty_or_invalid_database_is_not_classified_as_new() {
    for body in [b"".as_slice(), b"not a SQLite database".as_slice()] {
        let (root, state) = fixture();
        fs::create_dir(&state).unwrap();
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&state, fs::Permissions::from_mode(0o700)).unwrap();
        let db = state.join("coordination.sqlite3");
        fs::write(&db, body).unwrap();
        fs::set_permissions(&db, fs::Permissions::from_mode(0o600)).unwrap();
        let observed = Cell::new(false);
        let r = Store::open_with_startup_guard(
            &state,
            &root,
            StoreLimits::default(),
            |exists| -> Result<()> {
                observed.set(exists);
                Err(Error::Store("fixture authority refusal"))
            },
        );
        assert!(r.is_err());
        assert!(observed.get());
        assert_eq!(fs::read(&db).unwrap(), body);
        assert!(!state.join("coordination.sqlite3-wal").exists());
    }
}
