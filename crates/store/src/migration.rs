//! Explicit offline additive upgrade; never run delivery recovery during migration.
use super::{Store, StoreLimits, integrity, schema, sql, uuid};
use hermes_tern_core::error::{Error, Result};
use rusqlite::{
    Connection, OptionalExtension, TransactionBehavior, params, types::Value as SqlValue,
};
use serde_json::{Value, json};
use std::{fs::File, path::Path};

fn equal_rows(live: &Connection, archive: &Connection, table: &str) -> Result<bool> {
    // Table names come only from the fixed list below, never request text.
    let query = format!("SELECT * FROM {table} ORDER BY 1,2");
    let mut a = live.prepare(&query).map_err(sql)?;
    let mut b = archive.prepare(&query).map_err(sql)?;
    if a.column_count() != b.column_count() {
        return Ok(false);
    }
    let columns = a.column_count();
    let mut ar = a.query([]).map_err(sql)?;
    let mut br = b.query([]).map_err(sql)?;
    loop {
        match (ar.next().map_err(sql)?, br.next().map_err(sql)?) {
            (None, None) => return Ok(true),
            (Some(a), Some(b)) => {
                for i in 0..columns {
                    if a.get::<_, SqlValue>(i).map_err(sql)?
                        != b.get::<_, SqlValue>(i).map_err(sql)?
                    {
                        return Ok(false);
                    }
                }
            }
            _ => return Ok(false),
        }
    }
}

impl Store {
    pub fn migrate(
        state: &Path,
        workspace: &Path,
        archive_id: &str,
        limits: StoreLimits,
    ) -> Result<Value> {
        uuid(archive_id)?;
        let mut store = Self::open_mode(state, workspace, limits, true)?;
        let version: i64 = store
            .db
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .map_err(sql)?;
        if version == 2 {
            let witness: Option<(String, String)> = store
                .db
                .query_row(
                    "SELECT archive_id,database_sha256 FROM schema_migrations WHERE to_version=2",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()
                .map_err(sql)?;
            let (id, hash) = witness.ok_or(Error::Store("store was not migrated from v1"))?;
            if id != archive_id {
                return Err(Error::Store(
                    "migration archive ID differs from committed witness",
                ));
            }
            let mut archive = store.verified_archive(archive_id)?;
            if archive.database_schema() != 1 || archive.database_hash() != hash {
                return Err(Error::Store(
                    "migration archive differs from committed witness",
                ));
            }
            File::open(state)?.sync_all()?;
            File::open(state.join("archives"))?.sync_all()?;
            archive.recheck()?;
            return Ok(migration_receipt(&store.workspace_id, archive_id, &hash));
        }
        // Archive's no-clobber retry path completes its publication sync before success.
        let published = store.archive(archive_id)?;
        let mut archive = store.verified_archive(archive_id)?;
        if archive.database_schema() != 1 {
            return Err(Error::Store("migration requires a v1 archive"));
        }
        let hash = archive.database_hash().to_owned();
        if published != archive.receipt() {
            return Err(Error::Store("archive changed after publication"));
        }
        let tx = store
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sql)?;
        integrity::validate(&tx, &store.limits)?;
        for table in [
            "meta",
            "actors",
            "routes",
            "messages",
            "tasks",
            "events",
            "sqlite_sequence",
        ] {
            if !equal_rows(&tx, archive.database(), table)? {
                return Err(Error::Store(
                    "migration archive is stale; choose a new archive ID",
                ));
            }
        }
        tx.execute_batch(schema::V2).map_err(sql)?;
        tx.execute(
            "INSERT INTO schema_migrations VALUES(2,?1,?2)",
            params![archive_id, hash],
        )
        .map_err(sql)?;
        integrity::validate(&tx, &store.limits)?;
        archive.recheck()?;
        tx.commit().map_err(sql)?;
        Ok(migration_receipt(&store.workspace_id, archive_id, &hash))
    }
}
fn migration_receipt(workspace: &str, archive_id: &str, hash: &str) -> Value {
    json!({"status":"migrated","workspace_id":workspace,"from_schema":1,"to_schema":2,
        "archive_id":archive_id,"database_sha256":hash,"delivery_recovery_performed":false,
        "source_rows_pruned":false,"runtime_launched":false})
}
