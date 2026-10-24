//! One supervisor-owned SQLite journal. Peer content has no command authority.
use hermes_tern_core::error::{Error, Result};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    time::Duration,
};
use uuid::Uuid;

mod archive;
#[cfg(test)]
mod archive_tests;
mod credential;
mod disposition;
pub use disposition::DispositionInput;
#[cfg(test)]
mod fault_tests;
mod integrity;
mod migration;
mod observation;
mod projection;
mod schema;

fn sql(_: rusqlite::Error) -> Error {
    Error::Store("database operation failed; no acceptance claimed")
}
fn digest(s: &str) -> String {
    Sha256::digest(s.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
fn uuid(s: &str) -> Result<()> {
    if Uuid::parse_str(s).ok().is_none_or(|id| id.to_string() != s) {
        return Err(Error::Store("canonical UUID required"));
    }
    Ok(())
}
fn token() -> String {
    format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
}
fn nonempty(s: &str) -> bool {
    !s.is_empty() && s.len() <= 512 && !s.chars().any(char::is_control)
}
#[derive(Debug, Clone)]
pub struct StoreLimits {
    pub max_actors: usize,
    pub max_pending_per_actor: usize,
    pub max_payload_bytes: usize,
    pub max_total_payload_bytes: usize,
    pub max_messages: usize,
    pub max_events: usize,
    pub max_tasks: usize,
}
impl Default for StoreLimits {
    fn default() -> Self {
        Self {
            max_actors: 16,
            max_pending_per_actor: 128,
            max_payload_bytes: 32768,
            max_total_payload_bytes: 16 * 1024 * 1024,
            max_messages: 4096,
            max_events: 32768,
            max_tasks: 1024,
        }
    }
}
impl StoreLimits {
    fn validate(&self) -> Result<()> {
        if !(1..=64).contains(&self.max_actors)
            || !(1..=128).contains(&self.max_pending_per_actor)
            || !(1..=32768).contains(&self.max_payload_bytes)
            || !(1..=16 * 1024 * 1024).contains(&self.max_total_payload_bytes)
            || !(1..=4096).contains(&self.max_messages)
            || !(32..=32768).contains(&self.max_events)
            || !(1..=1024).contains(&self.max_tasks)
        {
            return Err(Error::Config("invalid store limits"));
        }
        Ok(())
    }
}
#[derive(Serialize)]
pub struct Registration {
    pub actor_id: String,
    pub incarnation: String,
    pub credential: String,
}
impl std::fmt::Debug for Registration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Registration")
            .field("actor_id", &self.actor_id)
            .field("incarnation", &self.incarnation)
            .field("credential", &"[redacted]")
            .finish()
    }
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MessageInput {
    pub message_id: String,
    pub recipient: String,
    pub kind: String,
    pub payload: Value,
    pub ttl_seconds: u64,
    pub correlation_id: Option<String>,
}
struct Identity {
    actor: String,
    incarnation: String,
}
/// Operator-attested physical session binding, not canonical-session continuity.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PeerIdentity {
    pub workspace_id: String,
    pub actor_id: String,
    pub incarnation: String,
    pub hermes_home: String,
    pub physical_session_id: String,
}
pub struct Store {
    db: Connection,
    _lock: File,
    pub state_dir: PathBuf,
    pub workspace_id: String,
    observation_epoch: String,
    limits: StoreLimits,
}

pub fn private_dir(path: &Path) -> Result<()> {
    if !path.is_absolute() {
        return Err(Error::Config("state directory must be absolute"));
    }
    if !path.exists() {
        fs::DirBuilder::new().mode(0o700).create(path)?;
    }
    let m = fs::symlink_metadata(path)?;
    if !m.is_dir() || m.uid() != rustix::process::geteuid().as_raw() || m.mode() & 0o077 != 0 {
        return Err(Error::Store(
            "state directory must be a real same-owner private directory",
        ));
    }
    Ok(())
}
pub fn private_file(path: &Path) -> Result<File> {
    state_file(path, true)
}
fn state_file(path: &Path, create: bool) -> Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(create)
        .truncate(false)
        .mode(0o600)
        .custom_flags(rustix::fs::OFlags::NOFOLLOW.bits() as i32)
        .open(path)?;
    let m = file.metadata()?;
    if !m.is_file()
        || m.uid() != rustix::process::geteuid().as_raw()
        || m.mode() & 0o077 != 0
        || m.nlink() != 1
    {
        return Err(Error::Store("unsafe state file"));
    }
    Ok(file)
}
impl Store {
    pub fn open(state_dir: &Path, workspace: &Path, limits: StoreLimits) -> Result<Self> {
        Self::open_mode(state_dir, workspace, limits, false)
    }
    /// Run the caller's authority guard under exclusive ownership, before any
    /// SQLite open/create, initialization or recovery. `database_exists` means
    /// the name exists, not that its bytes are valid or initialized. The guard
    /// must not recurse into the store. Refusal releases ownership without
    /// opening the database; the state directory/owner.lock may already exist.
    pub fn open_with_startup_guard<T>(
        state_dir: &Path,
        workspace: &Path,
        limits: StoreLimits,
        guard: impl FnOnce(bool) -> Result<T>,
    ) -> Result<(Self, T)> {
        Self::open_mode_guarded(state_dir, workspace, limits, false, guard)
    }
    fn open_mode(
        state_dir: &Path,
        workspace: &Path,
        limits: StoreLimits,
        migration: bool,
    ) -> Result<Self> {
        Self::open_mode_guarded(state_dir, workspace, limits, migration, |_| Ok(()))
            .map(|(store, ())| store)
    }
    fn open_mode_guarded<T>(
        state_dir: &Path,
        workspace: &Path,
        limits: StoreLimits,
        migration: bool,
        guard: impl FnOnce(bool) -> Result<T>,
    ) -> Result<(Self, T)> {
        limits.validate()?;
        if migration && !state_dir.is_dir() {
            return Err(Error::Store("migration requires an existing store"));
        }
        private_dir(state_dir)?;
        let workspace = workspace.canonicalize()?;
        if !workspace.is_dir() {
            return Err(Error::Config("workspace must be directory"));
        }
        let workspace = workspace
            .to_str()
            .ok_or(Error::Config("workspace must be UTF-8"))?;
        let lock = state_file(&state_dir.join("owner.lock"), !migration)?;
        rustix::fs::flock(&lock, rustix::fs::FlockOperation::NonBlockingLockExclusive)
            .map_err(|_| Error::Store("supervisor already owns this state directory"))?;
        let db_path = state_dir.join("coordination.sqlite3");
        let database_exists = match fs::symlink_metadata(&db_path) {
            Ok(_) => true,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
            Err(e) => return Err(e.into()),
        };
        let resource = guard(database_exists)?;
        state_file(&db_path, !migration)?;
        for suffix in ["-wal", "-shm", "-journal"] {
            let path = PathBuf::from(format!("{}{suffix}", db_path.display()));
            if path.symlink_metadata().is_ok() {
                private_file(&path)?;
            }
        }
        let mut db = Connection::open(&db_path).map_err(sql)?;
        db.busy_timeout(Duration::from_secs(2)).map_err(sql)?;
        let version: i64 = db
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .map_err(sql)?;
        if version == 1 && !migration {
            return Err(Error::Store(
                "schema v1 requires explicit migration with migrate-store",
            ));
        }
        if (migration && !matches!(version, 1 | 2)) || (!migration && !matches!(version, 0 | 2)) {
            return Err(Error::Store("unsupported coordination schema"));
        }
        db.execute_batch("PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA wal_autocheckpoint=128; PRAGMA journal_size_limit=8388608; PRAGMA max_page_count=65536;").map_err(sql)?;
        if version == 0 {
            schema::initialize(&mut db, workspace)?;
        }
        let actual: String = db
            .query_row(
                "SELECT value FROM meta WHERE key='workspace_root'",
                [],
                |r| r.get(0),
            )
            .map_err(sql)?;
        if actual != workspace {
            return Err(Error::Store("state belongs to another workspace"));
        }
        let workspace_id: String = db
            .query_row("SELECT value FROM meta WHERE key='workspace_id'", [], |r| {
                r.get(0)
            })
            .map_err(sql)?;
        uuid(&workspace_id)?;
        let integrity: String = db
            .query_row("PRAGMA quick_check", [], |r| r.get(0))
            .map_err(sql)?;
        if integrity != "ok" {
            return Err(Error::Store("coordination integrity failure"));
        }
        let recovery = db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sql)?;
        integrity::validate(&recovery, &limits)?;
        // A read may have crossed the process boundary. Never automatically requeue it.
        if !migration {
            recovery
                .execute(
                    "UPDATE messages SET state='ambiguous' WHERE state='submitted'",
                    [],
                )
                .map_err(sql)?;
        }
        recovery.commit().map_err(sql)?;
        Ok((
            Self {
                db,
                _lock: lock,
                state_dir: state_dir.into(),
                workspace_id,
                observation_epoch: Uuid::new_v4().to_string(),
                limits,
            },
            resource,
        ))
    }
    pub fn register(
        &mut self,
        kind: &str,
        profile: &str,
        session: &str,
        model: &str,
        provider: &str,
    ) -> Result<Registration> {
        self.register_with_publisher(kind, profile, session, model, provider, |_| Ok(()))
    }
    /// Publish the secret durably before committing its authority. A failed
    /// callback rolls back every registry/event change; an orphan file alone
    /// grants no authority. The callback must not recurse into this store.
    pub fn register_with_publisher(
        &mut self,
        kind: &str,
        profile: &str,
        session: &str,
        model: &str,
        provider: &str,
        publish: impl FnOnce(&Registration) -> Result<()>,
    ) -> Result<Registration> {
        if !matches!(kind, "hermes" | "omp")
            || ![profile, session, model, provider]
                .into_iter()
                .all(nonempty)
        {
            return Err(Error::Store("invalid actor binding"));
        }
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sql)?;
        let count: i64 = tx
            .query_row("SELECT count(*) FROM actors", [], |r| r.get(0))
            .map_err(sql)?;
        if count >= self.limits.max_actors as i64 {
            return Err(Error::Store("actor capacity"));
        }
        let registration = Registration {
            actor_id: Uuid::new_v4().to_string(),
            incarnation: Uuid::new_v4().to_string(),
            credential: token(),
        };
        tx.execute(
            "INSERT INTO actors VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
            params![
                registration.actor_id,
                registration.incarnation,
                digest(&registration.credential),
                kind,
                profile,
                session,
                model,
                provider
            ],
        )
        .map_err(sql)?;
        event(
            &tx,
            &self.limits,
            "actor.registered",
            &registration.actor_id,
        )?;
        publish(&registration)?;
        tx.commit().map_err(sql)?;
        Ok(registration)
    }
    pub fn rotate(&mut self, actor: &str) -> Result<Registration> {
        self.rotate_with_publisher(actor, |_| Ok(()))
    }
    /// As with registration, old authority remains valid until publication and
    /// the new registry incarnation both commit. Never auto-retry a lost reply.
    pub fn rotate_with_publisher(
        &mut self,
        actor: &str,
        publish: impl FnOnce(&Registration) -> Result<()>,
    ) -> Result<Registration> {
        uuid(actor)?;
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sql)?;
        let registration = Registration {
            actor_id: actor.into(),
            incarnation: Uuid::new_v4().to_string(),
            credential: token(),
        };
        let n = tx
            .execute(
                "UPDATE actors SET incarnation=?1,token_hash=?2 WHERE id=?3",
                params![
                    registration.incarnation,
                    digest(&registration.credential),
                    actor
                ],
            )
            .map_err(sql)?;
        if n != 1 {
            return Err(Error::Store("actor not found"));
        }
        tx.execute(
            "UPDATE messages SET state='ambiguous' WHERE recipient=?1 AND state='submitted'",
            [actor],
        )
        .map_err(sql)?;
        event(&tx, &self.limits, "actor.rotated", actor)?;
        publish(&registration)?;
        tx.commit().map_err(sql)?;
        Ok(registration)
    }
    pub fn route(&mut self, sender: &str, recipient: &str, allow: bool) -> Result<()> {
        uuid(sender)?;
        uuid(recipient)?;
        if sender == recipient {
            return Err(Error::Store("self route refused"));
        }
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sql)?;
        if allow {
            tx.execute(
                "INSERT OR IGNORE INTO routes VALUES(?1,?2)",
                params![sender, recipient],
            )
            .map_err(sql)?;
        } else {
            tx.execute(
                "DELETE FROM routes WHERE sender=?1 AND recipient=?2",
                params![sender, recipient],
            )
            .map_err(sql)?;
        }
        event(
            &tx,
            &self.limits,
            if allow {
                "route.allowed"
            } else {
                "route.denied"
            },
            recipient,
        )?;
        tx.commit().map_err(sql)
    }
    pub fn send(&mut self, credential: &str, input: &MessageInput, now: u64) -> Result<Value> {
        uuid(&input.message_id)?;
        uuid(&input.recipient)?;
        if let Some(id) = &input.correlation_id {
            uuid(id)?;
        }
        if !matches!(
            input.kind.as_str(),
            "context" | "task_offer" | "task_result" | "question" | "answer"
        ) || !(1..=3600).contains(&input.ttl_seconds)
        {
            return Err(Error::Store("invalid peer envelope"));
        }
        if !input.payload.is_object() {
            return Err(Error::Store("payload must be an attributed data object"));
        }
        let body = serde_json::to_string(input).map_err(|_| Error::Store("invalid payload"))?;
        if body.len() > self.limits.max_payload_bytes {
            return Err(Error::Store("peer payload limit"));
        }
        let hash = digest(
            &canonical(serde_json::to_value(input).map_err(|_| Error::Store("invalid payload"))?)
                .to_string(),
        );
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sql)?;
        let identity = authenticate(&tx, credential)?;
        clock(&tx, now)?;
        let existing: Option<(i64, String, String)> = tx
            .query_row(
                "SELECT sequence,body_hash,state FROM messages WHERE sender=?1 AND id=?2",
                params![identity.actor, input.message_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()
            .map_err(sql)?;
        if let Some((seq, stored, state)) = existing {
            if hash != stored {
                return Err(Error::Store("message id conflicts with accepted content"));
            }
            let disposition = disposition::decision_receipt(
                &tx,
                &self.workspace_id,
                &identity.actor,
                &input.message_id,
            )?;
            let origin = disposition::origin(&tx, &identity.actor, &input.message_id)?;
            tx.commit().map_err(sql)?;
            return Ok(
                json!({"sequence":seq.to_string(),"message_id":input.message_id,"state":state,"duplicate":true,"disposition":disposition,"origin":origin}),
            );
        }
        let permitted: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM routes WHERE sender=?1 AND recipient=?2)",
                params![identity.actor, input.recipient],
                |r| r.get(0),
            )
            .map_err(sql)?;
        if !permitted {
            return Err(Error::Store("route not allowed"));
        }
        expire(&tx, &self.limits, now)?;
        disposition::capacity(&tx, &self.limits, &input.recipient, 1, body.len() as i64)?;
        let expiry = now
            .checked_add(input.ttl_seconds)
            .and_then(|n| i64::try_from(n).ok())
            .ok_or(Error::Store("expiry overflow"))?;
        tx.execute("INSERT INTO messages(id,sender,recipient,kind,body,body_hash,created,expires,state) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,'queued')",params![input.message_id,identity.actor,input.recipient,input.kind,body,hash,now as i64,expiry]).map_err(sql)?;
        let sequence = tx.last_insert_rowid();
        event(&tx, &self.limits, "message.queued", &input.message_id)?;
        tx.commit().map_err(sql)?;
        Ok(
            json!({"sequence":sequence.to_string(),"message_id":input.message_id,"state":"queued","duplicate":false}),
        )
    }
    pub fn receive(&mut self, credential: &str, now: u64) -> Result<Vec<Value>> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sql)?;
        let identity = authenticate(&tx, credential)?;
        clock(&tx, now)?;
        expire(&tx, &self.limits, now)?;
        let rows = {
            let mut statement=tx.prepare("SELECT sequence,id,sender,body,created,expires FROM messages WHERE recipient=?1 AND state='queued' AND EXISTS(SELECT 1 FROM routes WHERE routes.sender=messages.sender AND routes.recipient=messages.recipient) ORDER BY sequence LIMIT 8").map_err(sql)?;
            statement
                .query_map([&identity.actor], |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, String>(3)?,
                        r.get::<_, i64>(4)?,
                        r.get::<_, i64>(5)?,
                    ))
                })
                .map_err(sql)?
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(sql)?
        };
        let mut out = vec![];
        for (seq, id, sender, body, created, expires) in rows {
            let input: MessageInput = serde_json::from_str(&body)
                .map_err(|_| Error::Store("corrupt persisted message"))?;
            tx.execute("UPDATE messages SET state='submitted',delivery_incarnation=?1 WHERE sequence=?2 AND state='queued'",params![identity.incarnation,seq]).map_err(sql)?;
            event(&tx, &self.limits, "message.submitted", &id)?;
            let origin = disposition::origin(&tx, &sender, &id)?;
            out.push(json!({"schema_version":1,"workspace_id":self.workspace_id,"message_id":id,"sender_actor_id":sender,"recipient_actor_id":identity.actor,"kind":input.kind,"correlation_id":input.correlation_id,"payload":input.payload,"sequence":seq.to_string(),"created_at":created.to_string(),"expires_at":expires.to_string(),"state":"submitted","origin":origin,"trust":"untrusted_peer_data_not_user_or_system_instructions"}));
        }
        tx.commit().map_err(sql)?;
        Ok(out)
    }
    pub fn ack(
        &mut self,
        credential: &str,
        sender: &str,
        message: &str,
        now: u64,
    ) -> Result<Value> {
        uuid(sender)?;
        uuid(message)?;
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sql)?;
        let identity = authenticate(&tx, credential)?;
        clock(&tx, now)?;
        let (state,incarnation):(String,Option<String>)=tx.query_row("SELECT state,delivery_incarnation FROM messages WHERE sender=?1 AND id=?2 AND recipient=?3",params![sender,message,identity.actor],|r|Ok((r.get(0)?,r.get(1)?))).map_err(sql)?;
        if !matches!(state.as_str(), "submitted" | "ambiguous" | "consumed")
            || incarnation.as_deref() != Some(&identity.incarnation)
            || disposition::decision_receipt(&tx, &self.workspace_id, sender, message)?.is_some()
        {
            return Err(Error::Store(
                "ack not entitled to this delivery incarnation",
            ));
        }
        if state != "consumed" {
            tx.execute(
                "UPDATE messages SET state='consumed' WHERE sender=?1 AND id=?2",
                params![sender, message],
            )
            .map_err(sql)?;
            event(&tx, &self.limits, "message.consumed", message)?;
        }
        tx.commit().map_err(sql)?;
        Ok(json!({"message_id":message,"state":"consumed"}))
    }
    /// Validate the current credential and every identity field before peer work.
    pub fn verify_peer(&self, credential: &str, peer: &PeerIdentity) -> Result<()> {
        for id in [&peer.workspace_id, &peer.actor_id, &peer.incarnation] {
            uuid(id)?;
        }
        if peer.workspace_id != self.workspace_id
            || !nonempty(&peer.physical_session_id)
            || peer.physical_session_id.trim() != peer.physical_session_id
            || !nonempty(&peer.hermes_home)
            || !Path::new(&peer.hermes_home).is_absolute()
            || credential.len() != 64
            || !credential.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(Error::Store("unproven peer identity"));
        }
        let valid: bool = self.db.query_row(
            "SELECT EXISTS(SELECT 1 FROM actors WHERE id=?1 AND incarnation=?2 AND token_hash=?3 AND kind='hermes' AND profile=?4 AND session=?5)",
            params![peer.actor_id, peer.incarnation, digest(credential), peer.hermes_home, peer.physical_session_id],
            |r| r.get(0),
        ).map_err(sql)?;
        if !valid {
            return Err(Error::Store(
                "peer binding revoked or does not match runtime context",
            ));
        }
        Ok(())
    }
    pub fn peer_status(&self, credential: &str, peer: &PeerIdentity) -> Result<Value> {
        self.verify_peer(credential, peer)?;
        let mut q = self.db.prepare(
            "SELECT actors.id,actors.kind FROM routes JOIN actors ON actors.id=routes.recipient WHERE routes.sender=?1 ORDER BY actors.id LIMIT 64",
        ).map_err(sql)?;
        let peers = q
            .query_map([&peer.actor_id], |r| {
                Ok(json!({"actor_id":r.get::<_,String>(0)?,"kind":r.get::<_,String>(1)?}))
            })
            .map_err(sql)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(sql)?;
        Ok(
            json!({"workspace_id":self.workspace_id,"actor_id":peer.actor_id,"incarnation":peer.incarnation,"identity_scope":"operator_attested_physical_session","sendable_peers":peers,"sandbox":false}),
        )
    }
    pub fn actors(&self) -> Result<Vec<Value>> {
        let mut q=self.db.prepare("SELECT id,incarnation,kind,profile,session,model,provider FROM actors ORDER BY id LIMIT 64").map_err(sql)?;
        q.query_map([],|r|Ok(json!({"actor_id":r.get::<_,String>(0)?,"incarnation":r.get::<_,String>(1)?,"kind":r.get::<_,String>(2)?,"profile":r.get::<_,String>(3)?,"session_id":r.get::<_,String>(4)?,"model":r.get::<_,String>(5)?,"provider":r.get::<_,String>(6)?}))).map_err(sql)?.collect::<std::result::Result<Vec<_>,_>>().map_err(sql)
    }
    pub fn messages(&self) -> Result<Vec<Value>> {
        observation::message_metadata(&self.db)
    }
    pub fn claim(&mut self, credential: &str, task: &str, now: u64, ttl: u64) -> Result<Value> {
        uuid(task)?;
        if !(1..=3600).contains(&ttl) {
            return Err(Error::Store("invalid lease lifetime"));
        }
        let deadline = now
            .checked_add(ttl)
            .and_then(|n| i64::try_from(n).ok())
            .ok_or(Error::Store("lease deadline overflow"))?;
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sql)?;
        let identity = authenticate(&tx, credential)?;
        clock(&tx, now)?;
        let row: Option<(i64, String, String, String, i64)> = tx
            .query_row(
                "SELECT generation,state,owner,incarnation,deadline FROM tasks WHERE id=?1",
                [task],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .optional()
            .map_err(sql)?;
        let generation = match row {
            Some((g, state, owner, inc, old_deadline))
                if state == "active"
                    && owner == identity.actor
                    && inc == identity.incarnation
                    && old_deadline > now as i64 =>
            {
                tx.commit().map_err(sql)?;
                return Ok(
                    json!({"task_id":task,"generation":g.to_string(),"deadline":old_deadline.to_string(),"state":"active"}),
                );
            }
            Some((g, state, _, _, _)) if state == "released" => g
                .checked_add(1)
                .ok_or(Error::Store("generation overflow"))?,
            Some(_) => {
                return Err(Error::Store(
                    "task held; expiry is not proof the previous writer stopped",
                ));
            }
            None => {
                let count: i64 = tx
                    .query_row("SELECT count(*) FROM tasks", [], |r| r.get(0))
                    .map_err(sql)?;
                if count >= self.limits.max_tasks as i64 {
                    return Err(Error::Store("task capacity"));
                }
                1
            }
        };
        tx.execute("INSERT INTO tasks VALUES(?1,?2,?3,?4,?5,'active',NULL) ON CONFLICT(id) DO UPDATE SET owner=excluded.owner,incarnation=excluded.incarnation,generation=excluded.generation,deadline=excluded.deadline,state='active',result=NULL",params![task,identity.actor,identity.incarnation,generation,deadline]).map_err(sql)?;
        event(&tx, &self.limits, "task.claimed", task)?;
        tx.commit().map_err(sql)?;
        Ok(
            json!({"task_id":task,"generation":generation.to_string(),"deadline":deadline.to_string(),"state":"active"}),
        )
    }
    pub fn complete(
        &mut self,
        credential: &str,
        task: &str,
        generation: &str,
        result: &str,
        now: u64,
    ) -> Result<Value> {
        uuid(task)?;
        let g: i64 = generation
            .parse()
            .map_err(|_| Error::Store("invalid generation"))?;
        if g <= 0 || g.to_string() != generation || result.len() > 32768 {
            return Err(Error::Store("noncanonical generation or oversized result"));
        }
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sql)?;
        let identity = authenticate(&tx, credential)?;
        clock(&tx, now)?;
        let n=tx.execute("UPDATE tasks SET state='complete',result=?1 WHERE id=?2 AND owner=?3 AND incarnation=?4 AND generation=?5 AND state='active' AND deadline>?6",params![result,task,identity.actor,identity.incarnation,g,now as i64]).map_err(sql)?;
        if n != 1 {
            let replay:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM tasks WHERE id=?1 AND owner=?2 AND incarnation=?3 AND generation=?4 AND state='complete' AND result=?5)",params![task,identity.actor,identity.incarnation,g,result],|r|r.get(0)).map_err(sql)?;
            if !replay {
                return Err(Error::Store("stale task completion"));
            }
        } else {
            event(&tx, &self.limits, "task.complete", task)?;
        }
        tx.commit().map_err(sql)?;
        Ok(json!({"task_id":task,"generation":generation,"state":"complete"}))
    }
    /// Release exactly the attested lease, never a later claimant of the task.
    /// An exact already-released retry is inert. No process is stopped or proved stopped.
    pub fn release_stopped(
        &mut self,
        task: &str,
        owner: &str,
        incarnation: &str,
        generation: &str,
        operator_attests_stopped: bool,
    ) -> Result<Value> {
        for id in [task, owner, incarnation] {
            uuid(id)?;
        }
        let g = generation
            .parse::<i64>()
            .map_err(|_| Error::Store("invalid generation"))?;
        if g <= 0 || g.to_string() != generation {
            return Err(Error::Store("noncanonical generation"));
        }
        if !operator_attests_stopped {
            return Err(Error::Store(
                "explicit prior-writer-stopped attestation required",
            ));
        }
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sql)?;
        // Match the persisted lease, not the actor's possibly rotated credential.
        let state: Option<String> = tx.query_row(
            "SELECT state FROM tasks WHERE id=?1 AND owner=?2 AND incarnation=?3 AND generation=?4",
            params![task, owner, incarnation, g], |r| r.get(0),
        ).optional().map_err(sql)?;
        match state.as_deref() {
            Some("active") => {
                let changed = tx.execute(
                    "UPDATE tasks SET state='released' WHERE id=?1 AND owner=?2 AND incarnation=?3 AND generation=?4 AND state='active'",
                    params![task, owner, incarnation, g],
                ).map_err(sql)?;
                if changed != 1 {
                    return Err(Error::Store("stale task release"));
                }
                event(&tx, &self.limits, "task.operator_released", task)?;
            }
            Some("released") => {}
            _ => return Err(Error::Store("stale or non-releasable task lease")),
        }
        tx.commit().map_err(sql)?;
        Ok(
            json!({"task_id":task,"owner_actor_id":owner,"incarnation":incarnation,"generation":generation,
            "state":"released","released":true,"process_stopped":"operator_attested_not_enforced"}),
        )
    }
}
fn authenticate(tx: &Transaction<'_>, credential: &str) -> Result<Identity> {
    if credential.len() != 64 || !credential.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(Error::Store("invalid actor credential"));
    }
    tx.query_row(
        "SELECT id,incarnation FROM actors WHERE token_hash=?1",
        [digest(credential)],
        |r| {
            Ok(Identity {
                actor: r.get(0)?,
                incarnation: r.get(1)?,
            })
        },
    )
    .optional()
    .map_err(sql)?
    .ok_or(Error::Store("actor credential revoked or unknown"))
}
fn clock(tx: &Transaction<'_>, now: u64) -> Result<()> {
    let now = i64::try_from(now).map_err(|_| Error::Store("invalid clock"))?;
    let previous: String = tx
        .query_row("SELECT value FROM meta WHERE key='clock'", [], |r| r.get(0))
        .map_err(sql)?;
    let previous = previous
        .parse::<i64>()
        .map_err(|_| Error::Store("corrupt clock watermark"))?;
    if now < previous {
        return Err(Error::Store(
            "clock moved backwards; operator reconciliation required",
        ));
    }
    tx.execute(
        "UPDATE meta SET value=?1 WHERE key='clock'",
        [now.to_string()],
    )
    .map_err(sql)?;
    Ok(())
}
fn expire(tx: &Transaction<'_>, limits: &StoreLimits, now: u64) -> Result<()> {
    let expired = {
        let mut statement = tx.prepare(
            "SELECT sequence,id FROM messages WHERE state='queued' AND expires<=?1 ORDER BY sequence LIMIT 4096",
        ).map_err(sql)?;
        statement
            .query_map([now as i64], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
            })
            .map_err(sql)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(sql)?
    };
    for (sequence, id) in expired {
        tx.execute(
            "UPDATE messages SET state='expired' WHERE sequence=?1",
            [sequence],
        )
        .map_err(sql)?;
        // Expiry settles queued liability; the row and its notice commit together.
        event(tx, limits, "message.expired", &id)?;
    }
    Ok(())
}
fn event(tx: &Transaction<'_>, limits: &StoreLimits, kind: &str, entity: &str) -> Result<()> {
    let count: i64 = tx
        .query_row("SELECT count(*) FROM events", [], |r| r.get(0))
        .map_err(sql)?;
    // Evaluate after the row mutation, inside the same transaction. Every queued
    // message still needs submit + ACK; submitted/ambiguous needs ACK; active
    // tasks need completion or explicit release. No unrelated write may spend
    // these slots. This reserves event rows, not physical disk space.
    let remaining: i64 = tx.query_row(
        "SELECT (SELECT COALESCE(sum(CASE state WHEN 'queued' THEN 2 WHEN 'submitted' THEN 1 WHEN 'ambiguous' THEN 1 ELSE 0 END),0) FROM messages m WHERE NOT EXISTS(SELECT 1 FROM message_dispositions d WHERE d.sender=m.sender AND d.message=m.id))
         + (SELECT count(*) FROM tasks WHERE state='active')",
        [], |r| r.get(0),
    ).map_err(sql)?;
    // Ordinary traffic cannot spend the bounded revocation allowance. Account
    // for safety events already committed (including this one) so revoking
    // authority does not subsequently block the reserved ACK/completion path.
    // This is a finite allowance, not unlimited emergency logging or disk space.
    let safety_budget = (limits.max_events / 4).min(256) as i64;
    let safety_used: i64 = tx
        .query_row(
            "SELECT count(*) FROM events WHERE kind IN ('actor.rotated','route.denied')",
            [],
            |r| r.get(0),
        )
        .map_err(sql)?;
    let is_safety = matches!(kind, "actor.rotated" | "route.denied");
    let is_settlement = matches!(
        kind,
        "message.submitted"
            | "message.consumed"
            | "message.expired"
            | "message.closed"
            | "task.complete"
            | "task.operator_released"
    );
    // Settling reduces remaining debt by the event it adds: it cannot consume
    // existing safety headroom. Do not retroactively strand older accepted work
    // whose journal predates this allowance; still enforce its settlement bound.
    let safety_remaining = if is_settlement {
        0
    } else {
        (safety_budget - safety_used - i64::from(is_safety)).max(0)
    };
    if count + 1 + remaining + safety_remaining > limits.max_events as i64 {
        return Err(Error::Store("journal capacity reserved for accepted work"));
    }
    tx.execute(
        "INSERT INTO events(kind,entity) VALUES(?1,?2)",
        params![kind, entity],
    )
    .map_err(sql)?;
    Ok(())
}
fn canonical(v: Value) -> Value {
    match v {
        Value::Object(map) => {
            let sorted: std::collections::BTreeMap<_, _> =
                map.into_iter().map(|(k, v)| (k, canonical(v))).collect();
            Value::Object(sorted.into_iter().collect())
        }
        Value::Array(a) => Value::Array(a.into_iter().map(canonical).collect()),
        v => v,
    }
}
