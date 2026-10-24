use hermes_tern_core::error::Error;
use hermes_tern_store::{Registration, Store, StoreLimits};
use rusqlite::Connection;
use serde_json::json;
use std::{fs, os::unix::fs::DirBuilderExt, path::PathBuf, time::Duration};

struct Fixture {
    root: PathBuf,
    state: PathBuf,
    store: Option<Store>,
    actor: Registration,
}
impl Fixture {
    fn new() -> Self {
        let root = PathBuf::from(std::env::var_os("TMPDIR").unwrap())
            .join(format!("ht-credential-{}", uuid::Uuid::new_v4()));
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        let state = root.join("state");
        let mut s = Store::open(&state, &root, StoreLimits::default()).unwrap();
        let actor = s
            .register("omp", "fixture", "session", "model", "provider")
            .unwrap();
        Self {
            root,
            state,
            store: Some(s),
            actor,
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        drop(self.store.take());
        let mut paths = vec![
            self.root.clone(),
            self.state.clone(),
            self.root.join("ownership.json"),
        ];
        paths.extend(
            fs::read_dir(&self.state)
                .unwrap()
                .map(|p| p.unwrap().path()),
        );
        fs::write(
            self.root.join("ownership.json"),
            json!({"paths":paths,"retention":"retain; no deletion"}).to_string(),
        )
        .unwrap();
    }
}

#[test]
fn exact_credential_callback_does_not_mutate_registry_clock_events_or_deliveries() {
    let mut f = Fixture::new();
    let s = f.store.as_mut().unwrap();
    let workspace = s.workspace_id.clone();
    let before = s.state_index().unwrap();
    let mut calls = 0;
    let result = s
        .with_current_credential(
            &workspace,
            &f.actor.actor_id,
            &f.actor.incarnation,
            &f.actor.credential,
            || {
                calls += 1;
                Ok("filesystem-only-result")
            },
        )
        .unwrap();
    assert_eq!(result, "filesystem-only-result");
    assert_eq!(calls, 1);
    assert_eq!(s.state_index().unwrap(), before);
    let failed: Result<(), _> = s.with_current_credential(
        &workspace,
        &f.actor.actor_id,
        &f.actor.incarnation,
        &f.actor.credential,
        || Err(Error::Store("fixture publication failure")),
    );
    assert!(failed.is_err());
    assert_eq!(s.state_index().unwrap(), before);
}

#[test]
fn wrong_workspace_actor_incarnation_and_secret_never_reach_publication() {
    let mut f = Fixture::new();
    let s = f.store.as_mut().unwrap();
    let workspace = s.workspace_id.clone();
    let before = s.state_index().unwrap();
    let mut calls = 0;
    let other = uuid::Uuid::new_v4().to_string();
    for (w, a, i, t) in [
        (
            other.as_str(),
            f.actor.actor_id.as_str(),
            f.actor.incarnation.as_str(),
            f.actor.credential.as_str(),
        ),
        (
            workspace.as_str(),
            other.as_str(),
            f.actor.incarnation.as_str(),
            f.actor.credential.as_str(),
        ),
        (
            workspace.as_str(),
            f.actor.actor_id.as_str(),
            other.as_str(),
            f.actor.credential.as_str(),
        ),
        (
            workspace.as_str(),
            f.actor.actor_id.as_str(),
            f.actor.incarnation.as_str(),
            "invalid",
        ),
        (
            workspace.as_str(),
            f.actor.actor_id.as_str(),
            f.actor.incarnation.as_str(),
            "0000000000000000000000000000000000000000000000000000000000000000",
        ),
        (
            "../escape",
            f.actor.actor_id.as_str(),
            f.actor.incarnation.as_str(),
            f.actor.credential.as_str(),
        ),
    ] {
        assert!(
            s.with_current_credential(w, a, i, t, || {
                calls += 1;
                Ok(())
            })
            .is_err()
        );
    }
    assert_eq!(calls, 0);
    assert_eq!(s.state_index().unwrap(), before);
    s.with_current_credential(
        &workspace,
        &f.actor.actor_id,
        &f.actor.incarnation,
        &f.actor.credential,
        || {
            calls += 1;
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(calls, 1);
}

#[test]
fn rotated_credential_is_not_reactivated_and_new_tuple_is_accepted_after_reopen() {
    let mut f = Fixture::new();
    let s = f.store.as_mut().unwrap();
    let workspace = s.workspace_id.clone();
    let new = s.rotate(&f.actor.actor_id).unwrap();
    let mut calls = 0;
    assert!(
        s.with_current_credential(
            &workspace,
            &f.actor.actor_id,
            &f.actor.incarnation,
            &f.actor.credential,
            || {
                calls += 1;
                Ok(())
            }
        )
        .is_err()
    );
    assert_eq!(calls, 0);
    drop(f.store.take());
    let mut s = Store::open(&f.state, &f.root, StoreLimits::default()).unwrap();
    let before = s.state_index().unwrap();
    assert!(
        s.with_current_credential(
            &workspace,
            &f.actor.actor_id,
            &new.incarnation,
            &f.actor.credential,
            || {
                calls += 1;
                Ok(())
            }
        )
        .is_err()
    );
    s.with_current_credential(
        &workspace,
        &new.actor_id,
        &new.incarnation,
        &new.credential,
        || {
            calls += 1;
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(calls, 1);
    assert_eq!(s.state_index().unwrap(), before);
}

#[test]
fn publication_callback_holds_registry_writer_exclusion_without_writing_rows() {
    let mut f = Fixture::new();
    let s = f.store.as_mut().unwrap();
    let workspace = s.workspace_id.clone();
    let db = Connection::open(f.state.join("coordination.sqlite3")).unwrap();
    db.busy_timeout(Duration::ZERO).unwrap();
    let before = s.state_index().unwrap();
    s.with_current_credential(
        &workspace,
        &f.actor.actor_id,
        &f.actor.incarnation,
        &f.actor.credential,
        || {
            let failed = db
                .execute("UPDATE actors SET model='racing'", [])
                .unwrap_err();
            assert_eq!(
                failed.sqlite_error_code(),
                Some(rusqlite::ErrorCode::DatabaseBusy)
            );
            assert_eq!(
                db.query_row("SELECT count(*) FROM actors", [], |r| r.get::<_, i64>(0))
                    .unwrap(),
                1
            );
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(s.state_index().unwrap(), before);
    // The writer control succeeds once the credential publication boundary ends.
    assert_eq!(
        db.execute("UPDATE actors SET model='racing'", []).unwrap(),
        1
    );
    assert_ne!(s.state_index().unwrap()["revision"], before["revision"]);
}
