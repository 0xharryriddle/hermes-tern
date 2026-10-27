//! Offline archive diagnostic; never opens live SQLite or activates authority.
use super::{Store, existing_dir, existing_file, same_file, sql, uuid, verify_bundle};
use hermes_tern_core::error::{Error, Result};
use serde_json::{Value, json};
use std::{
    fs::{File, OpenOptions},
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::Path,
};

fn retained_directory(path: &Path) -> Result<File> {
    existing_dir(path)?;
    Ok(OpenOptions::new()
        .read(true)
        .custom_flags((rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::DIRECTORY).bits() as i32)
        .open(path)?)
}
fn directory_unchanged(file: &File, path: &Path) -> Result<()> {
    existing_dir(path)?;
    let opened = file.metadata()?;
    let named = path.symlink_metadata()?;
    if opened.dev() != named.dev() || opened.ino() != named.ino() {
        return Err(Error::Store(
            "offline diagnostic directory identity changed",
        ));
    }
    Ok(())
}
impl Store {
    /// Verify a settled v2 archive against separately retained expectations.
    /// The guard must only inspect/recheck its own key; it runs under exclusive
    /// existing-lock ownership. This is not proof of key history, stopped workers,
    /// live-journal equivalence, restore safety or permission to activate state.
    pub fn preflight_recovery(
        state: &Path,
        workspace: &Path,
        archive_id: &str,
        expected_workspace_id: &str,
        expected_database_sha256: &str,
        mut key_guard: impl FnMut() -> Result<()>,
    ) -> Result<Value> {
        uuid(archive_id)?;
        uuid(expected_workspace_id)?;
        if expected_database_sha256.len() != 64
            || !expected_database_sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(Error::Store("exact lowercase archive SHA-256 required"));
        }
        if !state.is_absolute() || state.canonicalize()? != state {
            return Err(Error::Config("existing canonical state path required"));
        }
        let workspace = workspace.canonicalize()?;
        if !workspace.is_dir() {
            return Err(Error::Config("existing workspace directory required"));
        }
        let root = workspace
            .to_str()
            .ok_or(Error::Config("workspace must be UTF-8"))?;
        let state_dir = retained_directory(state)?;
        let lock_path = state.join("owner.lock");
        let lock = existing_file(&lock_path, 4096)?;
        rustix::fs::flock(&lock, rustix::fs::FlockOperation::NonBlockingLockExclusive).map_err(
            |_| Error::Store("supervisor already owns state; offline diagnostic refused"),
        )?;
        key_guard()?;
        let archives_path = state.join("archives");
        let archives_dir = retained_directory(&archives_path)?;
        let bundle_path = archives_path.join(archive_id);
        let bundle_dir = retained_directory(&bundle_path)?;
        let mut archive = verify_bundle(&bundle_path, archive_id, expected_workspace_id, root)?;
        if archive.database_schema() != 2 || archive.database_hash() != expected_database_sha256 {
            return Err(Error::Store(
                "offline diagnostic archive schema or digest differs",
            ));
        }
        crate::schema::validate_archive(archive.database())?;
        // Disposed ambiguous work is still historically unresolved; do not infer
        // that local retirement proved remote effects absent or safe to repeat.
        let (mail, tasks): (i64, i64) = archive.database().query_row(
            "SELECT (SELECT count(*) FROM messages WHERE state IN ('queued','submitted','ambiguous')),(SELECT count(*) FROM tasks WHERE state='active')",
            [], |r| Ok((r.get(0)?, r.get(1)?)),
        ).map_err(sql)?;
        if mail != 0 || tasks != 0 {
            return Err(Error::Store(
                "archive contains unresolved work; explicit reconciliation required",
            ));
        }
        key_guard()?;
        archive.recheck()?;
        same_file(&lock, &lock_path)?;
        directory_unchanged(&bundle_dir, &bundle_path)?;
        directory_unchanged(&archives_dir, &archives_path)?;
        directory_unchanged(&state_dir, state)?;
        if state.canonicalize()? != state || workspace.canonicalize()? != workspace {
            return Err(Error::Store("offline diagnostic root identity changed"));
        }
        Ok(
            json!({"status":"diagnostic_verified", "archive":archive.receipt(),
            "archive_schema_definition":"exact_supported_v2",
            "archived_unresolved_messages":"0", "archived_active_tasks":"0",
            "activation_supported":false, "restore_authorized":false,
            "key_originality":"not_established", "workers_stopped":"not_established",
            "live_state_inspected":false, "live_journal_equivalence":"not_established"}),
        )
    }
}
