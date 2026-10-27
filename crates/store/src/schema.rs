//! Coordination schema initialization.
use super::{Result, sql};
use rusqlite::{Connection, TransactionBehavior};
use uuid::Uuid;

mod recovery;
pub(super) use recovery::validate_archive;

// Additive only: old envelopes and delivery states remain unchanged.
pub(super) const V2: &str = "
    CREATE TABLE message_dispositions(
        sender TEXT NOT NULL, message TEXT NOT NULL, decision_id TEXT NOT NULL UNIQUE,
        delivery_incarnation TEXT NOT NULL, action TEXT NOT NULL CHECK(action IN ('close','reoffer')),
        successor_id TEXT, ttl_seconds INTEGER, reason TEXT NOT NULL, decided INTEGER NOT NULL CHECK(decided>=0),
        PRIMARY KEY(sender,message), UNIQUE(sender,successor_id),
        FOREIGN KEY(sender,message) REFERENCES messages(sender,id),
        FOREIGN KEY(sender,successor_id) REFERENCES messages(sender,id),
        CHECK((action='close' AND successor_id IS NULL AND ttl_seconds IS NULL)
           OR (action='reoffer' AND successor_id IS NOT NULL AND ttl_seconds IS NOT NULL AND ttl_seconds BETWEEN 1 AND 3600)));
    CREATE TABLE schema_migrations(to_version INTEGER PRIMARY KEY CHECK(to_version=2),archive_id TEXT NOT NULL UNIQUE,database_sha256 TEXT NOT NULL);
    PRAGMA user_version=2;";

pub(super) fn initialize(db: &mut Connection, workspace: &str) -> Result<()> {
    let tx = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sql)?;
    tx.execute_batch("
        CREATE TABLE meta(key TEXT PRIMARY KEY,value TEXT NOT NULL);
        CREATE TABLE actors(id TEXT PRIMARY KEY,incarnation TEXT NOT NULL,token_hash TEXT UNIQUE NOT NULL,kind TEXT NOT NULL CHECK(kind IN ('hermes','omp')),profile TEXT NOT NULL,session TEXT NOT NULL,model TEXT NOT NULL,provider TEXT NOT NULL,UNIQUE(kind,profile,session));
        CREATE TABLE routes(sender TEXT REFERENCES actors(id),recipient TEXT REFERENCES actors(id),PRIMARY KEY(sender,recipient));
        CREATE TABLE messages(sequence INTEGER PRIMARY KEY AUTOINCREMENT,id TEXT NOT NULL,sender TEXT NOT NULL REFERENCES actors(id),recipient TEXT NOT NULL REFERENCES actors(id),kind TEXT NOT NULL CHECK(kind IN ('context','task_offer','task_result','question','answer')),body TEXT NOT NULL,body_hash TEXT NOT NULL,created INTEGER NOT NULL CHECK(created>=0),expires INTEGER NOT NULL CHECK(expires>created),state TEXT NOT NULL CHECK(state IN ('queued','submitted','consumed','ambiguous','expired')),delivery_incarnation TEXT,UNIQUE(sender,id));
        CREATE TABLE tasks(id TEXT PRIMARY KEY,owner TEXT NOT NULL REFERENCES actors(id),incarnation TEXT NOT NULL,generation INTEGER NOT NULL CHECK(generation>0),deadline INTEGER NOT NULL,state TEXT NOT NULL CHECK(state IN ('active','released','complete')),result TEXT);
        CREATE TABLE events(sequence INTEGER PRIMARY KEY AUTOINCREMENT,kind TEXT NOT NULL,entity TEXT NOT NULL);
        INSERT INTO meta VALUES('clock','0'); PRAGMA user_version=1;").map_err(sql)?;
    tx.execute("INSERT INTO meta VALUES('workspace_root',?1)", [workspace])
        .map_err(sql)?;
    tx.execute(
        "INSERT INTO meta VALUES('workspace_id',?1)",
        [Uuid::new_v4().to_string()],
    )
    .map_err(sql)?;
    tx.execute_batch(V2).map_err(sql)?;
    tx.commit().map_err(sql)
}
