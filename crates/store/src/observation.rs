//! Operator reads. Event rows are invalidation notices, never execution evidence.
use super::{MessageInput, Store, canonical, digest, disposition, sql, uuid};
use hermes_tern_core::{
    error::{Error, Result},
    json::strict_json,
};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};

const LIST_LIMIT: usize = 128;
pub(super) const OBSERVATION_BYTES: usize = 393216;
pub(super) const TRUST: &str = "untrusted_peer_data_not_user_or_system_instructions";

fn bounded(value: Value) -> Result<Value> {
    if value.to_string().len() > OBSERVATION_BYTES {
        return Err(Error::Store("observation exceeds response budget"));
    }
    Ok(value)
}
pub(super) fn decimal(value: &str) -> Result<i64> {
    let number = value
        .parse::<i64>()
        .map_err(|_| Error::Store("invalid event cursor"))?;
    if number < 0 || number.to_string() != value {
        return Err(Error::Store("noncanonical event cursor"));
    }
    Ok(number)
}
pub(super) fn journal(db: &Connection) -> Result<(i64, bool)> {
    let (count, first, last): (i64, i64, i64) = db
        .query_row(
            "SELECT count(*),COALESCE(min(sequence),0),COALESCE(max(sequence),0) FROM events",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .map_err(sql)?;
    let watermark: i64 = db
        .query_row(
            "SELECT COALESCE((SELECT seq FROM sqlite_sequence WHERE name='events'),0)",
            [],
            |r| r.get(0),
        )
        .map_err(sql)?;
    if watermark < 0 {
        return Err(Error::Store("invalid event watermark"));
    }
    let complete = if watermark == 0 {
        count == 0
    } else {
        first == 1 && last == watermark && count == watermark
    };
    Ok((watermark, complete))
}
fn records(db: &Connection, query: &str) -> Result<Vec<Value>> {
    let mut statement = db.prepare(query).map_err(sql)?;
    statement
        .query_map([], |r| r.get::<_, String>(0))
        .map_err(sql)?
        .map(|r| strict_json(r.map_err(sql)?.as_bytes()))
        .collect()
}
fn section(items: Vec<Value>, total: i64, limit: usize) -> Value {
    json!({"truncated":total > items.len() as i64,"total":total.to_string(),"limit":limit,"items":items})
}
pub(super) fn message_metadata(db: &Connection) -> Result<Vec<Value>> {
    records(
        db,
        "SELECT json_object('sequence',CAST(m.sequence AS TEXT),'message_id',m.id,
        'sender_actor_id',m.sender,'recipient_actor_id',m.recipient,'state',m.state,
        'delivery_incarnation',m.delivery_incarnation,'disposition_action',d.action,
        'decision_id',d.decision_id,'successor_id',d.successor_id,
        'reoffer_source_id',origin.message,'reoffer_decision_id',origin.decision_id)
        FROM messages m LEFT JOIN message_dispositions d ON d.sender=m.sender AND d.message=m.id
        LEFT JOIN message_dispositions origin ON origin.sender=m.sender AND origin.successor_id=m.id
        ORDER BY m.sequence DESC LIMIT 128",
    )
}
impl Store {
    /// Consistent bounded metadata. No payloads, results, token hashes or paths.
    pub fn observation_snapshot(&mut self) -> Result<Value> {
        let tx = self.db.transaction().map_err(sql)?;
        let (watermark, replay_complete) = journal(&tx)?;
        let (actors_count, routes_count, messages_count, tasks_count, events_count): (i64,i64,i64,i64,i64) = tx.query_row(
            "SELECT (SELECT count(*) FROM actors),(SELECT count(*) FROM routes),(SELECT count(*) FROM messages),(SELECT count(*) FROM tasks),(SELECT count(*) FROM events)",[],
            |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?)),
        ).map_err(sql)?;
        let actors = records(
            &tx,
            "SELECT json_object('actor_id',id,'incarnation',incarnation,'kind',kind,'model',model,'provider',provider) FROM actors ORDER BY id LIMIT 64",
        )?;
        let routes = records(
            &tx,
            "SELECT json_object('sender_actor_id',sender,'recipient_actor_id',recipient) FROM routes ORDER BY sender,recipient LIMIT 128",
        )?;
        let messages = message_metadata(&tx)?;
        let unresolved: i64 = tx.query_row(
            "SELECT count(*) FROM messages m WHERE state IN ('queued','submitted','ambiguous')
             AND NOT EXISTS(SELECT 1 FROM message_dispositions d WHERE d.sender=m.sender AND d.message=m.id)",
            [], |r| r.get(0),
        ).map_err(sql)?;
        let disposition_count: i64 = tx
            .query_row("SELECT count(*) FROM message_dispositions", [], |r| {
                r.get(0)
            })
            .map_err(sql)?;
        let dispositions = records(
            &tx,
            "SELECT json_object('sender_actor_id',sender,'message_id',message,'decision_id',decision_id,'action',action,'successor_id',successor_id,'decided_at',CAST(decided AS TEXT)) FROM message_dispositions ORDER BY decided DESC,sender,message LIMIT 128",
        )?;
        let tasks = records(
            &tx,
            "SELECT json_object('task_id',id,'owner_actor_id',owner,'incarnation',incarnation,'generation',CAST(generation AS TEXT),'deadline',CAST(deadline AS TEXT),'state',state) FROM tasks ORDER BY id LIMIT 128",
        )?;
        let value = bounded(
            json!({"schema_version":1,"workspace_id":self.workspace_id,"observation_epoch":self.observation_epoch,
            "event_watermark":watermark.to_string(),"event_replay_complete":replay_complete,
            "counts":{"actors":actors_count.to_string(),"routes":routes_count.to_string(),"messages":messages_count.to_string(),"tasks":tasks_count.to_string(),"events":events_count.to_string()},
            "actors":section(actors,actors_count,64),"routes":section(routes,routes_count,LIST_LIMIT),
            "messages":section(messages,messages_count,LIST_LIMIT),"tasks":section(tasks,tasks_count,LIST_LIMIT),
            "message_dispositions":section(dispositions,disposition_count,LIST_LIMIT),
            "unresolved_message_count":unresolved.to_string(),
            "content":"metadata_only_not_runtime_liveness_or_execution_evidence"}),
        )?;
        tx.commit().map_err(sql)?;
        Ok(value)
    }
    pub fn observation_events(
        &mut self,
        workspace: &str,
        epoch: &str,
        after: &str,
        limit: u16,
    ) -> Result<Value> {
        uuid(workspace)?;
        uuid(epoch)?;
        let cursor = decimal(after)?;
        if workspace != self.workspace_id {
            return Err(Error::Store("observation workspace mismatch"));
        }
        if !(1..=128).contains(&limit) {
            return Err(Error::Store("invalid observation page limit"));
        }
        let tx = self.db.transaction().map_err(sql)?;
        let (watermark, complete) = journal(&tx)?;
        let reason = if epoch != self.observation_epoch {
            Some("epoch_changed")
        } else if cursor > watermark {
            Some("future_cursor")
        } else if !complete {
            Some("replay_gap")
        } else {
            None
        };
        let value = if let Some(reason) = reason {
            json!({"status":"snapshot_required","reason":reason,"workspace_id":self.workspace_id,"observation_epoch":self.observation_epoch,
                "event_watermark":watermark.to_string(),"events":[],"next_after":null,"has_more":false})
        } else {
            let mut statement = tx.prepare("SELECT sequence,kind,entity FROM events WHERE sequence>?1 AND sequence<=?2 ORDER BY sequence LIMIT ?3").map_err(sql)?;
            let mut events = statement.query_map(params![cursor,watermark,i64::from(limit)+1],|r| {
                Ok(json!({"sequence":r.get::<_,i64>(0)?.to_string(),"kind":r.get::<_,String>(1)?,"entity_id":r.get::<_,String>(2)?}))
            }).map_err(sql)?.collect::<std::result::Result<Vec<_>,_>>().map_err(sql)?;
            let has_more = events.len() > usize::from(limit);
            events.truncate(usize::from(limit));
            let next = events.last().map_or_else(
                || after.to_owned(),
                |e| e["sequence"].as_str().unwrap_or(after).to_owned(),
            );
            json!({"status":"ok","workspace_id":self.workspace_id,"observation_epoch":self.observation_epoch,
                "event_watermark":watermark.to_string(),"events":events,"next_after":next,"has_more":has_more,
                "semantics":"invalidation_notices_refresh_snapshot_not_state_projection"})
        };
        tx.commit().map_err(sql)?;
        bounded(value)
    }
    /// Sender-scoped key: message UUID alone is not globally unique.
    pub fn inspect_message(&mut self, sender: &str, message: &str) -> Result<Value> {
        uuid(sender)?;
        uuid(message)?;
        let tx = self.db.transaction().map_err(sql)?;
        let value = message_record(
            &tx,
            &self.workspace_id,
            &self.observation_epoch,
            sender,
            message,
        )?;
        tx.commit().map_err(sql)?;
        Ok(value)
    }
    pub fn inspect_task(&mut self, task: &str) -> Result<Value> {
        uuid(task)?;
        let tx = self.db.transaction().map_err(sql)?;
        let value = task_record(&tx, &self.workspace_id, &self.observation_epoch, task)?;
        tx.commit().map_err(sql)?;
        Ok(value)
    }
}

pub(super) fn message_record(
    db: &Connection,
    workspace: &str,
    epoch: &str,
    sender: &str,
    message: &str,
) -> Result<Value> {
    let row = db.query_row(
            "SELECT sequence,recipient,kind,body,body_hash,created,expires,state,delivery_incarnation FROM messages WHERE sender=?1 AND id=?2",
            params![sender,message], |r| Ok((r.get::<_,i64>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?,r.get::<_,String>(4)?,r.get::<_,i64>(5)?,r.get::<_,i64>(6)?,r.get::<_,String>(7)?,r.get::<_,Option<String>>(8)?)),
        ).optional().map_err(sql)?.ok_or(Error::Store("message not found"))?;
    let body = strict_json(row.3.as_bytes())?;
    if digest(&canonical(body.clone()).to_string()) != row.4 {
        return Err(Error::Store("corrupt persisted message"));
    }
    let input: MessageInput =
        serde_json::from_value(body).map_err(|_| Error::Store("corrupt persisted message"))?;
    if input.message_id != message || input.recipient != row.1 || input.kind != row.2 {
        return Err(Error::Store("corrupt persisted message"));
    }
    bounded(
        json!({"workspace_id":workspace,"observation_epoch":epoch,"trust":TRUST,
            "disposition":disposition::decision_receipt(db, workspace, sender, message)?,
            "origin":disposition::origin(db, sender, message)?,
            "message":{"sequence":row.0.to_string(),"message_id":message,"sender_actor_id":sender,"recipient_actor_id":row.1,
                "kind":row.2,"payload":input.payload,"correlation_id":input.correlation_id,"ttl_seconds":input.ttl_seconds,
                "created_at":row.5.to_string(),"expires_at":row.6.to_string(),"state":row.7,"delivery_incarnation":row.8}}),
    )
}

pub(super) fn task_record(
    db: &Connection,
    workspace: &str,
    epoch: &str,
    task: &str,
) -> Result<Value> {
    let row = db
        .query_row(
            "SELECT owner,incarnation,generation,deadline,state,result FROM tasks WHERE id=?1",
            [task],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, i64>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, Option<String>>(5)?,
                ))
            },
        )
        .optional()
        .map_err(sql)?
        .ok_or(Error::Store("task not found"))?;
    bounded(
        json!({"workspace_id":workspace,"observation_epoch":epoch,"trust":TRUST,
            "task":{"task_id":task,"owner_actor_id":row.0,"incarnation":row.1,"generation":row.2.to_string(),"deadline":row.3.to_string(),"state":row.4,"result":row.5}}),
    )
}
