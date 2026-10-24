//! Operator decisions preserve uncertain delivery evidence; re-offers use a new key.
use super::{
    MessageInput, Store, StoreLimits, canonical, clock, digest, event, expire, nonempty, sql, uuid,
};
use hermes_tern_core::{
    error::{Error, Result},
    json::strict_json,
};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DispositionInput {
    pub sender: String,
    pub message: String,
    pub delivery_incarnation: String,
    pub decision_id: String,
    pub action: String,
    pub successor_id: Option<String>,
    pub ttl_seconds: Option<u64>,
    pub reason: String,
    pub attest_outcome_unknown: bool,
}
impl DispositionInput {
    pub(super) fn validate(&self) -> Result<()> {
        for id in [
            &self.sender,
            &self.message,
            &self.delivery_incarnation,
            &self.decision_id,
        ] {
            uuid(id)?;
        }
        if !self.attest_outcome_unknown || !nonempty(&self.reason) || self.reason.trim().is_empty()
        {
            return Err(Error::Store(
                "bounded reason and explicit unknown-outcome attestation required",
            ));
        }
        match (
            self.action.as_str(),
            self.successor_id.as_deref(),
            self.ttl_seconds,
        ) {
            ("close", None, None) => (),
            ("reoffer", Some(id), Some(1..=3600)) if id != self.message => uuid(id)?,
            _ => return Err(Error::Store("invalid disposition action or successor")),
        }
        Ok(())
    }
    fn receipt(&self, workspace: &str, decided: i64) -> Value {
        json!({"status":"operator_disposed","workspace_id":workspace,"sender_actor_id":self.sender,
            "message_id":self.message,"delivery_incarnation":self.delivery_incarnation,
            "decision_id":self.decision_id,"action":self.action,"successor_id":self.successor_id,
            "ttl_seconds":self.ttl_seconds,"reason":self.reason,"decided_at":decided.to_string(),
            "original_state":"ambiguous","outcome":"unknown","process_stopped":"not_established",
            "authority":"operator_decision_not_execution_evidence"})
    }
}
fn decision(
    db: &Connection,
    sender: &str,
    message: &str,
) -> Result<Option<(DispositionInput, i64)>> {
    db.query_row("SELECT delivery_incarnation,decision_id,action,successor_id,ttl_seconds,reason,decided FROM message_dispositions WHERE sender=?1 AND message=?2",
        params![sender,message], |r| Ok((DispositionInput {
            sender:sender.into(),message:message.into(),delivery_incarnation:r.get(0)?,decision_id:r.get(1)?,
            action:r.get(2)?,successor_id:r.get(3)?,ttl_seconds:r.get::<_, Option<u32>>(4)?.map(u64::from),reason:r.get(5)?,attest_outcome_unknown:true,
        },r.get(6)?)),
    ).optional().map_err(sql)
}
pub(super) fn decision_receipt(
    db: &Connection,
    workspace: &str,
    sender: &str,
    message: &str,
) -> Result<Option<Value>> {
    Ok(decision(db, sender, message)?.map(|(d, t)| d.receipt(workspace, t)))
}
pub(super) fn origin(db: &Connection, sender: &str, message: &str) -> Result<Option<Value>> {
    db.query_row("SELECT message,decision_id FROM message_dispositions WHERE sender=?1 AND successor_id=?2",
        params![sender,message], |r| Ok(json!({"source_sender_actor_id":sender,"source_message_id":r.get::<_,String>(0)?,
            "decision_id":r.get::<_,String>(1)?,"authorization":"operator_reoffer_not_new_sender_intent"})),
    ).optional().map_err(sql)
}
pub(super) fn capacity(
    db: &Connection,
    limits: &StoreLimits,
    recipient: &str,
    extra_count: i64,
    extra_bytes: i64,
) -> Result<()> {
    let pending: i64 = db.query_row(
        "SELECT count(*) FROM messages m WHERE recipient=?1 AND state IN ('queued','submitted','ambiguous') AND NOT EXISTS(SELECT 1 FROM message_dispositions d WHERE d.sender=m.sender AND d.message=m.id)",
        [recipient], |r| r.get(0),
    ).map_err(sql)?;
    let (count, bytes): (i64, i64) = db
        .query_row(
            "SELECT count(*),COALESCE(sum(length(CAST(body AS BLOB))),0) FROM messages",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(sql)?;
    if pending + extra_count > limits.max_pending_per_actor as i64
        || count + extra_count > limits.max_messages as i64
        || bytes + extra_bytes > limits.max_total_payload_bytes as i64
    {
        return Err(Error::Store("mailbox capacity; accepted work preserved"));
    }
    Ok(())
}
impl Store {
    /// Caller authority is checked by supervisor, never inferred from request text.
    pub fn dispose_message(&mut self, input: &DispositionInput, now: u64) -> Result<Value> {
        input.validate()?;
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sql)?;
        if let Some((previous, decided)) = decision(&tx, &input.sender, &input.message)? {
            if previous != *input {
                return Err(Error::Store("message already has a different disposition"));
            }
            // No clock, expiry or event writes for exact committed replay.
            return Ok(previous.receipt(&self.workspace_id, decided));
        }
        let (state,incarnation,body,hash,created):(String,Option<String>,String,String,i64) = tx.query_row(
            "SELECT state,delivery_incarnation,body,body_hash,created FROM messages WHERE sender=?1 AND id=?2",params![input.sender,input.message],
            |r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?)),
        ).map_err(sql)?;
        if state != "ambiguous" || incarnation.as_deref() != Some(&input.delivery_incarnation) {
            return Err(Error::Store(
                "disposition requires the exact ambiguous delivery",
            ));
        }
        let value = strict_json(body.as_bytes())?;
        if digest(&canonical(value.clone()).to_string()) != hash {
            return Err(Error::Store("corrupt persisted message"));
        }
        let mut successor: MessageInput =
            serde_json::from_value(value).map_err(|_| Error::Store("corrupt persisted message"))?;
        if successor.message_id != input.message {
            return Err(Error::Store("corrupt persisted message"));
        }
        clock(&tx, now)?;
        if i64::try_from(now).ok().is_none_or(|n| n < created) {
            return Err(Error::Store("disposition before source creation"));
        }
        if let (Some(id), Some(ttl)) = (&input.successor_id, input.ttl_seconds) {
            let permitted: bool = tx
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM routes WHERE sender=?1 AND recipient=?2)",
                    params![input.sender, successor.recipient],
                    |r| r.get(0),
                )
                .map_err(sql)?;
            if !permitted {
                return Err(Error::Store("route not allowed"));
            }
            successor.message_id = id.clone();
            successor.ttl_seconds = ttl;
            let body =
                serde_json::to_string(&successor).map_err(|_| Error::Store("invalid successor"))?;
            if body.len() > self.limits.max_payload_bytes {
                return Err(Error::Store("peer payload limit"));
            }
            let hash = digest(
                &canonical(
                    serde_json::to_value(&successor)
                        .map_err(|_| Error::Store("invalid successor"))?,
                )
                .to_string(),
            );
            let expires = now
                .checked_add(ttl)
                .and_then(|n| i64::try_from(n).ok())
                .ok_or(Error::Store("expiry overflow"))?;
            // Insert first to satisfy the FK; admission below includes both new and retained rows.
            tx.execute("INSERT INTO messages(id,sender,recipient,kind,body,body_hash,created,expires,state) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,'queued')",params![id,input.sender,successor.recipient,successor.kind,body,hash,now as i64,expires]).map_err(sql)?;
        }
        tx.execute(
            "INSERT INTO message_dispositions VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            params![
                input.sender,
                input.message,
                input.decision_id,
                input.delivery_incarnation,
                input.action,
                input.successor_id,
                input.ttl_seconds.map(|n| n as i64),
                input.reason,
                now as i64
            ],
        )
        .map_err(sql)?;
        if input.action == "reoffer" {
            expire(&tx, &self.limits, now)?;
            capacity(&tx, &self.limits, &successor.recipient, 0, 0)?;
        }
        event(
            &tx,
            &self.limits,
            if input.action == "close" {
                "message.closed"
            } else {
                "message.reoffered"
            },
            &input.message,
        )?;
        let receipt = input.receipt(&self.workspace_id, now as i64);
        tx.commit().map_err(sql)?;
        Ok(receipt)
    }
}

pub(super) fn validate(db: &Connection, limits: &StoreLimits, clock: i64) -> Result<()> {
    let require = |ok| {
        if ok {
            Ok(())
        } else {
            Err(Error::Store("invalid persisted disposition or migration"))
        }
    };
    let bounded:bool=db.query_row("SELECT (SELECT count(*) FROM message_dispositions)<=?1 AND (SELECT count(*) FROM schema_migrations)<=1
        AND NOT EXISTS(SELECT 1 FROM message_dispositions WHERE length(CAST(reason AS BLOB))>512 OR length(sender)!=36 OR length(message)!=36 OR length(decision_id)!=36 OR length(delivery_incarnation)!=36 OR length(successor_id)>36)
        AND NOT EXISTS(SELECT 1 FROM schema_migrations WHERE to_version!=2 OR length(archive_id)!=36 OR length(database_sha256)!=64)", [limits.max_messages as i64], |r|r.get(0)).map_err(sql)?;
    require(bounded)?;
    let mut q = db
        .prepare("SELECT archive_id,database_sha256 FROM schema_migrations")
        .map_err(sql)?;
    let mut rows = q.query([]).map_err(sql)?;
    while let Some(r) = rows.next().map_err(sql)? {
        uuid(&r.get::<_, String>(0).map_err(sql)?)?;
        require(super::integrity::hex_digest(
            &r.get::<_, String>(1).map_err(sql)?,
        ))?;
    }
    let mut q = db
        .prepare("SELECT sender,message FROM message_dispositions")
        .map_err(sql)?;
    let mut rows = q.query([]).map_err(sql)?;
    while let Some(r) = rows.next().map_err(sql)? {
        let sender: String = r.get(0).map_err(sql)?;
        let message: String = r.get(1).map_err(sql)?;
        let (d, t) = decision(db, &sender, &message)?.ok_or(Error::Store("missing disposition"))?;
        d.validate()?;
        require(t >= 0 && t <= clock)?;
        let (state,inc,source_body,sequence,created):(String,Option<String>,String,i64,i64)=db.query_row("SELECT state,delivery_incarnation,body,sequence,created FROM messages WHERE sender=?1 AND id=?2",params![sender,message],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).map_err(sql)?;
        require(
            state == "ambiguous" && inc.as_deref() == Some(&d.delivery_incarnation) && t >= created,
        )?;
        if let (Some(id), Some(ttl)) = (&d.successor_id, d.ttl_seconds) {
            let (body, seq, created): (String, i64, i64) = db
                .query_row(
                    "SELECT body,sequence,created FROM messages WHERE sender=?1 AND id=?2",
                    params![sender, id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .map_err(sql)?;
            let mut expected: MessageInput = serde_json::from_str(&source_body)
                .map_err(|_| Error::Store("invalid disposition source"))?;
            expected.message_id = id.clone();
            expected.ttl_seconds = ttl;
            require(
                seq > sequence
                    && created == t
                    && canonical(strict_json(body.as_bytes())?)
                        == canonical(
                            serde_json::to_value(expected)
                                .map_err(|_| Error::Store("invalid disposition source"))?,
                        ),
            )?;
        }
    }
    Ok(())
}
