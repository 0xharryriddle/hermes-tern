//! Reconcile files only against already-committed actor authority.
use super::{Store, authenticate, sql, uuid};
use hermes_tern_core::error::{Error, Result};
use rusqlite::TransactionBehavior;

impl Store {
    /// Run a filesystem-only callback while the exact current credential is fenced.
    /// The callback must not reenter the store or grant new registry authority.
    pub fn with_current_credential<T>(
        &mut self,
        workspace: &str,
        actor: &str,
        incarnation: &str,
        credential: &str,
        publish: impl FnOnce() -> Result<T>,
    ) -> Result<T> {
        for id in [workspace, actor, incarnation] {
            uuid(id)?;
        }
        if workspace != self.workspace_id {
            return Err(Error::Store("credential workspace mismatch"));
        }
        // No DML: this reservation excludes registry writers across the filesystem
        // effect. A successful file move does not create or change actor authority.
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sql)?;
        let stored_workspace: String = tx
            .query_row("SELECT value FROM meta WHERE key='workspace_id'", [], |r| {
                r.get(0)
            })
            .map_err(sql)?;
        let identity = authenticate(&tx, credential)?;
        if stored_workspace != workspace
            || identity.actor != actor
            || identity.incarnation != incarnation
        {
            return Err(Error::Store(
                "credential does not match current actor incarnation",
            ));
        }
        let result = publish()?;
        tx.commit().map_err(sql)?;
        Ok(result)
    }
}
