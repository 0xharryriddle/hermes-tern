//! Complete retained actor credentials are recoverable only under current authority.
use crate::{Supervisor, secrets, valid_token};
use hermes_tern_core::{
    error::{Error, Result},
    json::strict_json,
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::Path;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Credential {
    schema_version: u32,
    pub(super) workspace_id: String,
    pub(super) actor_id: String,
    pub(super) incarnation: String,
    pub(super) credential: String,
}
fn canonical_id(id: &str) -> Result<()> {
    if uuid::Uuid::parse_str(id)
        .ok()
        .is_none_or(|u| u.to_string() != id)
    {
        return Err(Error::Store("canonical credential identity required"));
    }
    Ok(())
}
impl Credential {
    pub(super) fn parse(text: &str) -> Result<Self> {
        if text.len() > 4096 {
            return Err(Error::Store("credential exceeds private file limit"));
        }
        let value: Self = serde_json::from_value(strict_json(text.as_bytes())?)
            .map_err(|_| Error::Store("invalid credential envelope"))?;
        if value.schema_version != 1 || !valid_token(&value.credential) {
            return Err(Error::Store("invalid credential envelope"));
        }
        for id in [&value.workspace_id, &value.actor_id, &value.incarnation] {
            canonical_id(id)?;
        }
        Ok(value)
    }
}
fn present(path: &Path) -> Result<bool> {
    match path.symlink_metadata() {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.into()),
    }
}
impl Supervisor {
    pub(super) fn credential_publication(
        &mut self,
        staging_id: &str,
        actor: &str,
        incarnation: &str,
        recover: bool,
    ) -> Result<Value> {
        for id in [staging_id, actor, incarnation] {
            canonical_id(id)?;
        }
        let state = self.store.state_dir.clone();
        let parent = state.join("credentials");
        let state_dir = secrets::open_directory(&state)?;
        let dir = secrets::open_directory(&parent)?;
        secrets::same_directory(&state_dir, &state)?;
        secrets::same_directory(&dir, &parent)?;
        let staging = parent.join(format!(".pending-{staging_id}"));
        let final_path = parent.join(format!("{actor}-{incarnation}.json"));
        let staged = present(&staging)?;
        if recover && staged && present(&final_path)? {
            return Err(Error::Store(
                "both staged and final credentials exist; preserve both",
            ));
        }
        if !recover && !staged {
            return Err(Error::Store("staged credential not found"));
        }
        // Final-only retry verifies current authority, not an absent staging UUID's provenance.
        let selected = if staged { &staging } else { &final_path };
        let mut file = secrets::PrivateFile::open(selected)?;
        let credential = Credential::parse(file.text())?;
        if credential.actor_id != actor
            || credential.incarnation != incarnation
            || credential.workspace_id != self.store.workspace_id
        {
            return Err(Error::Store(
                "credential envelope differs from requested identity",
            ));
        }
        self.store.with_current_credential(&credential.workspace_id,actor,incarnation,&credential.credential,||{
            secrets::same_directory(&state_dir,&state)?;
            secrets::same_directory(&dir,&parent)?;
            file.recheck()?;
            if recover {file.recover_to(&final_path)?;}
            secrets::same_directory(&state_dir,&state)?;
            secrets::same_directory(&dir,&parent)?;
            file.recheck()?;
            if recover {
                Ok(json!({"status":"credential_recovered","workspace_id":credential.workspace_id,"actor_id":actor,
                    "incarnation":incarnation,"credential_file":final_path,"published":true,"authority_changed":false,
                    "staging_provenance":"not_established"}))
            } else {
                Ok(json!({"status":"staged_verified","workspace_id":credential.workspace_id,"actor_id":actor,
                    "incarnation":incarnation,"staging_id":staging_id,"staging_file":staging,"credential_file":final_path,
                    "published":false,"authority_changed":false}))
            }
        })
    }
}
