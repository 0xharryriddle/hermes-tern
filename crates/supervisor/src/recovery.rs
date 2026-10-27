//! Read-only operator-key inspection for the offline archive diagnostic.
use crate::{secrets::PrivateFile, valid_token};
use hermes_tern_core::error::{Error, Result};
use hermes_tern_store::Store;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::Path;

/// Does not prepare/create a key, open live SQLite, start IPC or restore authority.
pub fn preflight(
    state: &Path,
    workspace: &Path,
    archive_id: &str,
    workspace_id: &str,
    database_sha256: &str,
    expected_operator_key_sha256: &str,
) -> Result<Value> {
    if expected_operator_key_sha256.len() != 64
        || !expected_operator_key_sha256
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(Error::Store("exact retained operator-key SHA-256 required"));
    }
    let mut key: Option<PrivateFile> = None;
    let mut report = Store::preflight_recovery(
        state,
        workspace,
        archive_id,
        workspace_id,
        database_sha256,
        || {
            if let Some(retained) = key.as_mut() {
                return retained.recheck();
            }
            let retained = PrivateFile::open(&state.join("operator.key"))?;
            if !valid_token(retained.text()) {
                return Err(Error::Store("operator credential is malformed"));
            }
            let digest: String = Sha256::digest(retained.text().as_bytes())
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect();
            if digest != expected_operator_key_sha256 {
                return Err(Error::Store(
                    "operator credential differs from retained expectation",
                ));
            }
            key = Some(retained);
            Ok(())
        },
    )?;
    report["operator_key_expectation"] =
        serde_json::json!("matched_retained_digest;historical_originality_not_established");
    Ok(report)
}
