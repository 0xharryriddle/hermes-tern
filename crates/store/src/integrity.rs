//! Bounded semantic validation before recovery changes any durable row.
use super::{StoreLimits, canonical, digest, nonempty, sql, uuid};
use hermes_tern_core::{
    error::{Error, Result},
    json::strict_json,
};
use rusqlite::Connection;

fn require(valid: bool) -> Result<()> {
    if valid {
        Ok(())
    } else {
        Err(Error::Store("coordination semantic integrity failure"))
    }
}
pub(super) fn hex_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

pub(super) fn validate(db: &Connection, limits: &StoreLimits) -> Result<()> {
    let version: i64 = db
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .map_err(sql)?;
    require(matches!(version, 1 | 2))?;
    // Bound counts and encoded bytes in SQLite before allocating row strings.
    let bounded: bool = db.query_row(
        "SELECT (SELECT count(*) FROM actors)<=?1
         AND (SELECT count(*) FROM messages)<=?2
         AND (SELECT count(*) FROM events)<=?3
         AND (SELECT count(*) FROM tasks)<=?4
         AND (SELECT count(*) FROM routes)<=4096
         AND (SELECT count(*) FROM meta)=3
         AND NOT EXISTS(SELECT 1 FROM meta WHERE key NOT IN ('clock','workspace_id','workspace_root'))
         AND NOT EXISTS(SELECT 1 FROM actors WHERE
             length(CAST(id AS BLOB))!=36 OR length(CAST(incarnation AS BLOB))!=36 OR length(CAST(token_hash AS BLOB))!=64
             OR length(CAST(profile AS BLOB))>512 OR length(CAST(session AS BLOB))>512
             OR length(CAST(model AS BLOB))>512 OR length(CAST(provider AS BLOB))>512)
         AND NOT EXISTS(SELECT 1 FROM messages WHERE length(CAST(body AS BLOB))>?5)
         AND (SELECT COALESCE(sum(length(CAST(body AS BLOB))),0) FROM messages)<=?6
         AND NOT EXISTS(SELECT 1 FROM tasks WHERE length(CAST(result AS BLOB))>32768)",
        rusqlite::params![limits.max_actors as i64, limits.max_messages as i64, limits.max_events as i64,
            limits.max_tasks as i64, limits.max_payload_bytes as i64, limits.max_total_payload_bytes as i64],
        |r| r.get(0),
    ).map_err(sql)?;
    require(bounded)?;
    require(
        !db.prepare("PRAGMA foreign_key_check")
            .map_err(sql)?
            .exists([])
            .map_err(sql)?,
    )?;
    let watermark: String = db
        .query_row("SELECT value FROM meta WHERE key='clock'", [], |r| r.get(0))
        .map_err(sql)?;
    let clock = watermark
        .parse::<i64>()
        .map_err(|_| Error::Store("corrupt clock watermark"))?;
    require(clock >= 0 && clock.to_string() == watermark)?;
    if version == 2 {
        super::disposition::validate(db, limits, clock)?;
    }

    let mut actors = db
        .prepare("SELECT id,incarnation,token_hash,kind,profile,session,model,provider FROM actors")
        .map_err(sql)?;
    let mut rows = actors.query([]).map_err(sql)?;
    while let Some(r) = rows.next().map_err(sql)? {
        uuid(&r.get::<_, String>(0).map_err(sql)?)?;
        uuid(&r.get::<_, String>(1).map_err(sql)?)?;
        require(hex_digest(&r.get::<_, String>(2).map_err(sql)?))?;
        require(matches!(
            r.get::<_, String>(3).map_err(sql)?.as_str(),
            "hermes" | "omp"
        ))?;
        for index in 4..8 {
            require(nonempty(&r.get::<_, String>(index).map_err(sql)?))?;
        }
    }
    require(
        !db.query_row(
            "SELECT EXISTS(SELECT 1 FROM routes WHERE sender IS NULL OR recipient IS NULL OR sender=recipient)",
            [],
            |r| r.get::<_, bool>(0),
        )
        .map_err(sql)?,
    )?;

    let mut messages = db.prepare("SELECT id,sender,recipient,kind,body,body_hash,created,expires,state,delivery_incarnation FROM messages").map_err(sql)?;
    let mut rows = messages.query([]).map_err(sql)?;
    while let Some(r) = rows.next().map_err(sql)? {
        let id: String = r.get(0).map_err(sql)?;
        let sender: String = r.get(1).map_err(sql)?;
        let recipient: String = r.get(2).map_err(sql)?;
        for value in [&id, &sender, &recipient] {
            uuid(value)?;
        }
        require(sender != recipient)?;
        let kind: String = r.get(3).map_err(sql)?;
        let body: String = r.get(4).map_err(sql)?;
        let hash: String = r.get(5).map_err(sql)?;
        let value = strict_json(body.as_bytes())?;
        require(hex_digest(&hash) && digest(&canonical(value.clone()).to_string()) == hash)?;
        let input: super::MessageInput =
            serde_json::from_value(value).map_err(|_| Error::Store("corrupt persisted message"))?;
        let created: i64 = r.get(6).map_err(sql)?;
        let expires: i64 = r.get(7).map_err(sql)?;
        require(
            input.message_id == id
                && input.recipient == recipient
                && input.kind == kind
                && matches!(
                    kind.as_str(),
                    "context" | "task_offer" | "task_result" | "question" | "answer"
                )
                && input.payload.is_object()
                && (1..=3600).contains(&input.ttl_seconds)
                && created >= 0
                && created <= clock
                && created.checked_add(input.ttl_seconds as i64) == Some(expires),
        )?;
        if let Some(id) = input.correlation_id {
            uuid(&id)?;
        }
        let state: String = r.get(8).map_err(sql)?;
        let incarnation: Option<String> = r.get(9).map_err(sql)?;
        match state.as_str() {
            "queued" | "expired" => require(incarnation.is_none())?,
            "submitted" | "consumed" | "ambiguous" => {
                uuid(
                    incarnation
                        .as_deref()
                        .ok_or(Error::Store("missing delivery incarnation"))?,
                )?;
            }
            _ => return Err(Error::Store("invalid persisted delivery state")),
        }
    }
    let mut tasks = db
        .prepare("SELECT id,owner,incarnation,generation,deadline,state,result FROM tasks")
        .map_err(sql)?;
    let mut rows = tasks.query([]).map_err(sql)?;
    while let Some(r) = rows.next().map_err(sql)? {
        for index in 0..3 {
            uuid(&r.get::<_, String>(index).map_err(sql)?)?;
        }
        require(r.get::<_, i64>(3).map_err(sql)? > 0 && r.get::<_, i64>(4).map_err(sql)? > 0)?;
        let result: Option<String> = r.get(6).map_err(sql)?;
        match r.get::<_, String>(5).map_err(sql)?.as_str() {
            "active" | "released" => require(result.is_none())?,
            "complete" => require(result.is_some())?,
            _ => return Err(Error::Store("invalid persisted task state")),
        }
    }
    let mut events = db
        .prepare("SELECT sequence,kind,entity FROM events ORDER BY sequence")
        .map_err(sql)?;
    let mut rows = events.query([]).map_err(sql)?;
    while let Some(r) = rows.next().map_err(sql)? {
        require(r.get::<_, i64>(0).map_err(sql)? > 0)?;
        let kind: String = r.get(1).map_err(sql)?;
        require(
            matches!(
                kind.as_str(),
                "actor.registered"
                    | "actor.rotated"
                    | "route.allowed"
                    | "route.denied"
                    | "message.queued"
                    | "message.submitted"
                    | "message.consumed"
                    | "message.expired"
                    | "task.claimed"
                    | "task.complete"
                    | "task.operator_released"
            ) || (version == 2 && matches!(kind.as_str(), "message.closed" | "message.reoffered")),
        )?;
        uuid(&r.get::<_, String>(2).map_err(sql)?)?;
    }
    Ok(())
}
