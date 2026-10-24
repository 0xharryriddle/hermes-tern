//! Supervisor-only startup policy. Caller already holds the store ownership lock.
use crate::{equal_secret, secrets, valid_token};
use hermes_tern_core::error::{Error, Result};
use std::{fs, io::ErrorKind, path::Path};

pub(super) fn prepare(dir: &Path, database_exists: bool) -> Result<secrets::PrivateFile> {
    let path = dir.join("operator.key");
    let key_exists = match path.symlink_metadata() {
        Ok(_) => true,
        Err(e) if e.kind() == ErrorKind::NotFound => false,
        Err(e) => return Err(e.into()),
    };
    if database_exists && !key_exists {
        return Err(Error::Store(
            "operator key missing for existing journal; preserve state for explicit recovery",
        ));
    }
    if !database_exists {
        // An absent main DB with retained actor files/sidecars is not a fresh
        // installation. Only an empty bootstrap or a completed key-first
        // publication may proceed. Never infer freshness from SQLite contents.
        for entry in fs::read_dir(dir)? {
            let name = entry?.file_name();
            if name != "owner.lock" && !(key_exists && name == "operator.key") {
                return Err(Error::Store(
                    "incomplete coordinator state; preserve files for explicit recovery",
                ));
            }
        }
    }
    let created = if !key_exists {
        let token = format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        );
        secrets::create(&path, &token)?;
        Some(token)
    } else {
        None
    };
    let mut file = secrets::PrivateFile::open(&path)?;
    if !valid_token(file.text())
        || created
            .as_ref()
            .is_some_and(|token| !equal_secret(token, file.text()))
    {
        return Err(Error::Store("invalid or changed operator credential file"));
    }
    secrets::prepare_directory(dir)?;
    file.sync()?;
    Ok(file)
}
