//! Revision-bound operator state enumeration, not an event reducer or restore format.
use super::{Store, disposition, integrity, observation, sql, uuid};
use hermes_tern_core::{
    error::{Error, Result},
    json::strict_json,
};
use rusqlite::{Connection, params, types::ValueRef};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

// Literal SQL only. Ordered keys are unique; no peer-selected identifier enters SQL.
const TABLES: [(&str, &str); 9] = [
    ("actors", "SELECT * FROM actors ORDER BY id"),
    ("events", "SELECT * FROM events ORDER BY sequence"),
    (
        "message_dispositions",
        "SELECT * FROM message_dispositions ORDER BY sender,message",
    ),
    ("messages", "SELECT * FROM messages ORDER BY sequence"),
    ("meta", "SELECT * FROM meta ORDER BY key"),
    ("routes", "SELECT * FROM routes ORDER BY sender,recipient"),
    (
        "schema_migrations",
        "SELECT * FROM schema_migrations ORDER BY to_version",
    ),
    (
        "sqlite_sequence",
        "SELECT * FROM sqlite_sequence ORDER BY name",
    ),
    ("tasks", "SELECT * FROM tasks ORDER BY id"),
];

fn framed(hash: &mut Sha256, bytes: &[u8]) {
    hash.update((bytes.len() as u64).to_be_bytes());
    hash.update(bytes);
}
fn revision(db: &Connection) -> Result<String> {
    let mut hash = Sha256::new();
    hash.update(b"hermes-tern-operator-state-v1\0schema-2\0");
    for (table, query) in TABLES {
        hash.update(b"table");
        framed(&mut hash, table.as_bytes());
        let mut stmt = db.prepare(query).map_err(sql)?;
        let columns = stmt.column_count();
        hash.update((columns as u64).to_be_bytes());
        let mut rows = stmt.query([]).map_err(sql)?;
        while let Some(row) = rows.next().map_err(sql)? {
            hash.update(b"row");
            for col in 0..columns {
                match row.get_ref(col).map_err(sql)? {
                    ValueRef::Null => hash.update(b"n"),
                    ValueRef::Integer(value) => {
                        hash.update(b"i");
                        hash.update(value.to_be_bytes());
                    }
                    ValueRef::Text(bytes) => {
                        hash.update(b"t");
                        framed(&mut hash, bytes);
                    }
                    _ => return Err(Error::Store("invalid state revision cell type")),
                }
            }
        }
        hash.update(b"end");
    }
    Ok(hash.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

fn checked_index(
    db: &Connection,
    store_workspace: &str,
    epoch: &str,
    limits: &super::StoreLimits,
) -> Result<Value> {
    let version: i64 = db
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .map_err(sql)?;
    if version != 2 {
        return Err(Error::Store("state projection requires schema v2"));
    }
    let tables = {
        let mut q = db
            .prepare("SELECT name FROM sqlite_schema WHERE type='table' ORDER BY name LIMIT 10")
            .map_err(sql)?;
        q.query_map([], |r| r.get::<_, String>(0))
            .map_err(sql)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(sql)?
    };
    if tables != TABLES.map(|(name, _)| name) {
        return Err(Error::Store("unexpected state projection tables"));
    }
    integrity::validate(db, limits)?;
    let workspace: String = db
        .query_row("SELECT value FROM meta WHERE key='workspace_id'", [], |r| {
            r.get(0)
        })
        .map_err(sql)?;
    if workspace != store_workspace {
        return Err(Error::Store("state projection workspace mismatch"));
    }
    // sqlite_sequence has no declared unique constraint; refuse duplicate/unknown entries.
    let sequences_ok:bool=db.query_row("SELECT count(*)<=2 AND count(*)=count(DISTINCT name) AND NOT EXISTS(SELECT 1 FROM sqlite_sequence WHERE name NOT IN ('events','messages') OR typeof(seq)!='integer' OR seq<0 OR name IS NULL) FROM sqlite_sequence",[],|r|r.get(0)).map_err(sql)?;
    if !sequences_ok {
        return Err(Error::Store("invalid state allocation counters"));
    }
    let (messages_watermark,events_watermark,clock):(i64,i64,String)=db.query_row(
        "SELECT COALESCE((SELECT seq FROM sqlite_sequence WHERE name='messages'),0),COALESCE((SELECT seq FROM sqlite_sequence WHERE name='events'),0),(SELECT value FROM meta WHERE key='clock')",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)),
    ).map_err(sql)?;
    let sequences_cover_rows:bool=db.query_row("SELECT NOT EXISTS(SELECT 1 FROM messages WHERE sequence<=0 OR sequence>?1) AND NOT EXISTS(SELECT 1 FROM events WHERE sequence<=0 OR sequence>?2)",params![messages_watermark,events_watermark],|r|r.get(0)).map_err(sql)?;
    if !sequences_cover_rows {
        return Err(Error::Store(
            "state allocation counters behind retained rows",
        ));
    }
    let (_, complete) = observation::journal(db)?;
    let mut counts = serde_json::Map::new();
    for (section, query) in [
        ("actors", "SELECT count(*) FROM actors"),
        ("routes", "SELECT count(*) FROM routes"),
        ("messages", "SELECT count(*) FROM messages"),
        ("tasks", "SELECT count(*) FROM tasks"),
        ("dispositions", "SELECT count(*) FROM message_dispositions"),
        ("migrations", "SELECT count(*) FROM schema_migrations"),
        ("events", "SELECT count(*) FROM events"),
    ] {
        let count: i64 = db.query_row(query, [], |r| r.get(0)).map_err(sql)?;
        counts.insert(section.into(), json!(count.to_string()));
    }
    Ok(
        json!({"schema_version":1,"database_schema":2,"workspace_id":workspace,"observation_epoch":epoch,
        "revision":revision(db)?,"counts":counts,"clock_watermark":clock,"event_watermark":events_watermark.to_string(),
        "message_watermark":messages_watermark.to_string(),"event_replay_complete":complete,
        "max_page_limit":128,"response_bytes":observation::OBSERVATION_BYTES,
        "content":"persisted_state_not_runtime_liveness_execution_evidence_or_restore_authority",
        "trust":observation::TRUST}),
    )
}
fn page_query(section: &str) -> Result<&'static str> {
    match section {
        "actors" => Ok(
            "SELECT json_object('actor_id',id,'incarnation',incarnation,'kind',kind,'profile',profile,'session_id',session,'model',model,'provider',provider) FROM actors ORDER BY id LIMIT ?1 OFFSET ?2",
        ),
        "routes" => Ok(
            "SELECT json_object('sender_actor_id',sender,'recipient_actor_id',recipient) FROM routes ORDER BY sender,recipient LIMIT ?1 OFFSET ?2",
        ),
        "messages" => Ok("SELECT sender,id FROM messages ORDER BY sequence LIMIT ?1 OFFSET ?2"),
        "tasks" => Ok("SELECT id FROM tasks ORDER BY id LIMIT ?1 OFFSET ?2"),
        "dispositions" => Ok(
            "SELECT sender,message FROM message_dispositions ORDER BY sender,message LIMIT ?1 OFFSET ?2",
        ),
        "migrations" => Ok(
            "SELECT json_object('to_version',CAST(to_version AS TEXT),'archive_id',archive_id,'database_sha256',database_sha256) FROM schema_migrations ORDER BY to_version LIMIT ?1 OFFSET ?2",
        ),
        "events" => Ok(
            "SELECT json_object('sequence',CAST(sequence AS TEXT),'kind',kind,'entity_id',entity) FROM events ORDER BY sequence LIMIT ?1 OFFSET ?2",
        ),
        _ => Err(Error::Store("invalid state section")),
    }
}
impl Store {
    pub fn state_index(&mut self) -> Result<Value> {
        let tx = self.db.transaction().map_err(sql)?;
        let value = checked_index(
            &tx,
            &self.workspace_id,
            &self.observation_epoch,
            &self.limits,
        )?;
        tx.commit().map_err(sql)?;
        Ok(value)
    }
    pub fn state_page(
        &mut self,
        workspace: &str,
        epoch: &str,
        expected_revision: &str,
        section: &str,
        offset: &str,
        limit: u16,
    ) -> Result<Value> {
        uuid(workspace)?;
        uuid(epoch)?;
        if workspace != self.workspace_id {
            return Err(Error::Store("state projection workspace mismatch"));
        }
        if !integrity::hex_digest(expected_revision) || !(1..=128).contains(&limit) {
            return Err(Error::Store("invalid state revision or page limit"));
        }
        let start = observation::decimal(offset)?;
        let query = page_query(section)?;
        let tx = self.db.transaction().map_err(sql)?;
        let index = checked_index(
            &tx,
            &self.workspace_id,
            &self.observation_epoch,
            &self.limits,
        )?;
        let reason = if epoch != self.observation_epoch {
            Some("epoch_changed")
        } else if index["revision"] != expected_revision {
            Some("state_changed")
        } else {
            None
        };
        if let Some(reason) = reason {
            let value = json!({"status":"restart_required","reason":reason,"workspace_id":self.workspace_id,
                "observation_epoch":self.observation_epoch,"revision":index["revision"],"items":[],"next_offset":null,"has_more":false});
            tx.commit().map_err(sql)?;
            return Ok(value);
        }
        let total = observation::decimal(
            index["counts"][section]
                .as_str()
                .ok_or(Error::Store("missing state count"))?,
        )?;
        if start > total {
            return Err(Error::Store("state offset beyond collection"));
        }
        let mut value = json!({"schema_version":1,"status":"ok","workspace_id":self.workspace_id,"observation_epoch":self.observation_epoch,
            "revision":index["revision"],"section":section,"offset":offset,"limit":limit,"total":total.to_string(),
            "items":[],"next_offset":total.to_string(),"has_more":false,"byte_limited":false,"trust":observation::TRUST});
        // Reserve the largest next-offset width and the longer boolean spellings.
        let mut encoded_bytes = value.to_string().len();
        let mut items = Vec::new();
        let mut byte_limited = false;
        {
            let mut q = tx.prepare(query).map_err(sql)?;
            let mut rows = q.query(params![i64::from(limit), start]).map_err(sql)?;
            while let Some(row) = rows.next().map_err(sql)? {
                let item = match section {
                    "messages" => observation::message_record(
                        &tx,
                        workspace,
                        epoch,
                        &row.get::<_, String>(0).map_err(sql)?,
                        &row.get::<_, String>(1).map_err(sql)?,
                    )?,
                    "tasks" => observation::task_record(
                        &tx,
                        workspace,
                        epoch,
                        &row.get::<_, String>(0).map_err(sql)?,
                    )?,
                    "dispositions" => disposition::decision_receipt(
                        &tx,
                        workspace,
                        &row.get::<_, String>(0).map_err(sql)?,
                        &row.get::<_, String>(1).map_err(sql)?,
                    )?
                    .ok_or(Error::Store("missing state disposition"))?,
                    _ => strict_json(row.get::<_, String>(0).map_err(sql)?.as_bytes())?,
                };
                let size = item.to_string().len() + usize::from(!items.is_empty());
                if encoded_bytes + size > observation::OBSERVATION_BYTES {
                    if items.is_empty() {
                        return Err(Error::Store("state record exceeds response budget"));
                    }
                    byte_limited = true;
                    break;
                }
                encoded_bytes += size;
                items.push(item);
            }
        }
        let next = start + items.len() as i64;
        if !byte_limited && next != total.min(start + i64::from(limit)) {
            return Err(Error::Store("incomplete state page"));
        }
        value["next_offset"] = json!(next.to_string());
        value["has_more"] = json!(next < total);
        value["byte_limited"] = json!(byte_limited);
        value["items"] = json!(items);
        if value.to_string().len() > observation::OBSERVATION_BYTES {
            return Err(Error::Store("state page exceeds response budget"));
        }
        tx.commit().map_err(sql)?;
        Ok(value)
    }
}
