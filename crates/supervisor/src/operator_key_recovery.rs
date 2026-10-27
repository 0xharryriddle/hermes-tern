//! Offline publication of an intact operator-attested retained original key.
//!
//! This does not reconstruct a key or establish originality, worker shutdown,
//! revocation, or archive safety. The state directory and its existing parent
//! must both be private. No SQLite connection or runtime is opened here.
use crate::secrets::{PrivateFile, open_directory, same_directory};
use hermes_tern_core::error::{Error, Result};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs::{File, Metadata, OpenOptions},
    io::ErrorKind,
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
};

/// Finish a retained same-directory publication while owning existing exclusion.
///
/// An error can follow a successful rename: only an explicit same-ID/digest
/// retry may finish synchronization. Errors and receipts never include secrets.
pub fn recover(
    state: &Path,
    staging_id: &str,
    expected_operator_key_sha256: &str,
    attest_original_key: bool,
) -> Result<Value> {
    // Validate all caller-supplied authority before even inspecting the filesystem.
    if !attest_original_key
        || !lower_token(expected_operator_key_sha256)
        || !uuid::Uuid::parse_str(staging_id)
            .is_ok_and(|id| id.hyphenated().to_string() == staging_id)
    {
        return Err(Error::Store(
            "operator-key recovery requires exact inputs and original-key attestation",
        ));
    }
    recover_checked(state, staging_id, expected_operator_key_sha256).map_err(|_| {
        Error::Store(
            "operator-key recovery refused; publication may require explicit same-input retry",
        )
    })
}

fn recover_checked(state: &Path, staging_id: &str, expected_digest: &str) -> Result<Value> {
    if !state.is_absolute() || state.canonicalize()?.as_os_str() != state.as_os_str() {
        return Err(Error::Store("canonical absolute private state required"));
    }
    let parent = state
        .parent()
        .ok_or(Error::Store("private state parent required"))?;
    let parent_dir = open_directory(parent)?;
    let state_dir = open_directory(state)?;
    same_directory(&parent_dir, parent)?;
    same_directory(&state_dir, state)?;

    let owner = RetainedMetadata::open(&state.join("owner.lock"), Some(4096))?;
    rustix::fs::flock(
        &owner.file,
        rustix::fs::FlockOperation::NonBlockingLockExclusive,
    )
    .map_err(|_| Error::Store("exclusive existing ownership required"))?;
    owner.recheck()?;
    // Metadata only: never read journal contents and never ask SQLite to open it.
    let journal = RetainedMetadata::open(&state.join("coordination.sqlite3"), None)?;
    let recheck_authority = || -> Result<()> {
        same_directory(&parent_dir, parent)?;
        same_directory(&state_dir, state)?;
        owner.recheck()?;
        journal.recheck()
    };
    recheck_authority()?;

    let staged = state.join(format!(".pending-{staging_id}"));
    let final_key = state.join("operator.key");
    let selected = select_existing(&staged, &final_key)?;
    let mut candidate = PrivateFile::open(&selected)?;
    if !lower_token(candidate.text())
        || !crate::equal_secret(
            &Sha256::digest(candidate.text().as_bytes())
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>(),
            expected_digest,
        )
    {
        return Err(Error::Store(
            "retained operator key does not match exact authority",
        ));
    }
    candidate.recheck()?;
    recheck_authority()?;
    if select_existing(&staged, &final_key)? != selected {
        return Err(Error::Store("operator-key names changed"));
    }
    // Reuse the existing bounded descriptor/no-replace/durability implementation.
    // On final-only retry this synchronizes the same validated inode, no rename.
    candidate.recover_to(&final_key)?;
    recheck_authority()?;
    candidate.recheck()?;
    if select_existing(&staged, &final_key)? != final_key {
        return Err(Error::Store("operator-key names changed"));
    }
    Ok(json!({
        "status": "operator_key_recovered",
        "published": true,
        "new_key_generated": false,
        "database_opened": false,
        "runtime_launched": false,
        "key_originality": "operator_attested_not_independently_proven",
        "staging_provenance": "not_established",
        "workers_stopped": "not_established",
        "authority_policy": "unchanged_key_bytes_only"
    }))
}

fn lower_token(value: &str) -> bool {
    crate::valid_token(value) && value.bytes().all(|byte| !byte.is_ascii_uppercase())
}

fn select_existing(staged: &Path, final_key: &Path) -> Result<PathBuf> {
    fn exists(path: &Path) -> Result<bool> {
        match path.symlink_metadata() {
            Ok(_) => Ok(true),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error.into()),
        }
    }
    match (exists(staged)?, exists(final_key)?) {
        (true, false) => Ok(staged.into()),
        (false, true) => Ok(final_key.into()),
        _ => Err(Error::Store(
            "exactly one retained operator-key name required",
        )),
    }
}

/// Existing non-secret guard files are descriptor-bound without reading bytes.
struct RetainedMetadata {
    file: File,
    path: PathBuf,
    initial: Metadata,
    limit: Option<u64>,
}
impl RetainedMetadata {
    fn open(path: &Path, limit: Option<u64>) -> Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(
                (rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::NONBLOCK).bits() as i32,
            )
            .open(path)?;
        let initial = file.metadata()?;
        let guard = Self {
            file,
            path: path.into(),
            initial,
            limit,
        };
        guard.recheck()?;
        Ok(guard)
    }
    fn recheck(&self) -> Result<()> {
        let opened = self.file.metadata()?;
        let named = self.path.symlink_metadata()?;
        let safe = |m: &Metadata| {
            m.is_file()
                && m.uid() == rustix::process::geteuid().as_raw()
                && m.mode() & 0o077 == 0
                && m.nlink() == 1
                && self.limit.is_none_or(|limit| m.len() <= limit)
        };
        let same = |a: &Metadata, b: &Metadata| {
            a.dev() == b.dev()
                && a.ino() == b.ino()
                && a.uid() == b.uid()
                && a.mode() == b.mode()
                && a.nlink() == b.nlink()
                && a.len() == b.len()
                && a.mtime() == b.mtime()
                && a.mtime_nsec() == b.mtime_nsec()
                && a.ctime() == b.ctime()
                && a.ctime_nsec() == b.ctime_nsec()
        };
        if !safe(&opened)
            || !safe(&named)
            || !same(&self.initial, &opened)
            || !same(&opened, &named)
        {
            return Err(Error::Store(
                "unsafe or changed existing ownership metadata",
            ));
        }
        Ok(())
    }
}
