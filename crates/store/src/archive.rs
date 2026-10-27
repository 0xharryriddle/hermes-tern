//! Private immutable snapshot bundles. Capture is not compaction or restoration.
use super::{Store, StoreLimits, integrity, private_dir, sql, uuid};
use hermes_tern_core::{
    error::{Error, Result},
    json::strict_json,
};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
};

mod image;
mod preflight;
mod recovery;
use image::DatabaseImage;

const DATABASE_BYTES: u64 = 256 * 1024 * 1024;
const MANIFEST_BYTES: u64 = 4096;
const TOTAL_BYTES: u64 = 512 * 1024 * 1024;
const ROOT_ENTRIES: usize = 8;
const BUNDLE_ENTRIES: usize = 8;
const DATABASE: &str = "coordination.sqlite3";
const MANIFEST: &str = "manifest.json";

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Counts {
    actors: String,
    routes: String,
    messages: String,
    tasks: String,
    events: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    message_dispositions: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    schema_migrations: Option<String>,
}
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    archive_version: u32,
    archive_id: String,
    workspace_id: String,
    database_schema: u32,
    database_bytes: String,
    database_sha256: String,
    counts: Counts,
    clock_watermark: String,
    event_watermark: String,
}

fn existing_dir(path: &Path) -> Result<()> {
    let m = fs::symlink_metadata(path)?;
    if !m.is_dir() || m.uid() != rustix::process::geteuid().as_raw() || m.mode() & 0o077 != 0 {
        return Err(Error::Store("unsafe archive directory"));
    }
    Ok(())
}
fn existing_file(path: &Path, cap: u64) -> Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags((rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::NONBLOCK).bits() as i32)
        .open(path)?;
    let m = file.metadata()?;
    if !m.is_file()
        || m.uid() != rustix::process::geteuid().as_raw()
        || m.mode() & 0o077 != 0
        || m.nlink() != 1
        || m.len() > cap
    {
        return Err(Error::Store("unsafe or oversized archive file"));
    }
    Ok(file)
}
fn present(path: &Path) -> Result<bool> {
    match path.symlink_metadata() {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.into()),
    }
}
/// Bound scanning as well as storage. Unknown private regular entries consume budget.
fn usage(root: &Path) -> Result<(usize, u64)> {
    let mut count = 0;
    let mut bytes = 0_u64;
    for entry in fs::read_dir(root)? {
        count += 1;
        if count > ROOT_ENTRIES {
            return Err(Error::Store("archive entry capacity"));
        }
        let path = entry?.path();
        let m = path.symlink_metadata()?;
        if m.is_dir() {
            existing_dir(&path)?;
            for (i, entry) in fs::read_dir(&path)?.enumerate() {
                if i >= BUNDLE_ENTRIES {
                    return Err(Error::Store("archive bundle entry capacity"));
                }
                let file = existing_file(&entry?.path(), TOTAL_BYTES)?;
                bytes = bytes
                    .checked_add(file.metadata()?.len())
                    .ok_or(Error::Store("archive size overflow"))?;
            }
        } else {
            let file = existing_file(&path, TOTAL_BYTES)?;
            bytes = bytes
                .checked_add(file.metadata()?.len())
                .ok_or(Error::Store("archive size overflow"))?;
        }
        if bytes > TOTAL_BYTES {
            return Err(Error::Store("archive byte capacity"));
        }
    }
    Ok((count, bytes))
}
fn fingerprint(file: &mut File) -> Result<(u64, String)> {
    let original = file.metadata()?;
    let mut total = 0_u64;
    let mut hash = Sha256::new();
    file.seek(SeekFrom::Start(0))?;
    let mut chunk = [0_u8; 65536];
    loop {
        let n = file.read(&mut chunk)?;
        if n == 0 {
            break;
        }
        total += n as u64;
        if total > DATABASE_BYTES {
            return Err(Error::Store("archive database size limit"));
        }
        hash.update(&chunk[..n]);
    }
    if total != original.len() || file.metadata()?.len() != original.len() {
        return Err(Error::Store("archive changed during verification"));
    }
    let digest = hash
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    Ok((total, digest))
}
fn same_file(file: &File, path: &Path) -> Result<()> {
    let opened = file.metadata()?;
    let named = path.symlink_metadata()?;
    if !named.is_file()
        || !opened.is_file()
        || opened.uid() != rustix::process::geteuid().as_raw()
        || opened.mode() & 0o077 != 0
        || opened.nlink() != 1
        || opened.dev() != named.dev()
        || opened.ino() != named.ino()
        || opened.len() != named.len()
    {
        return Err(Error::Store("archive file identity changed"));
    }
    Ok(())
}
fn max_limits() -> StoreLimits {
    StoreLimits {
        max_actors: 64,
        ..StoreLimits::default()
    }
}
fn describe_image(
    image: &DatabaseImage,
    archive_id: &str,
    workspace: &str,
    workspace_root: &str,
) -> Result<Manifest> {
    let tx = image.db.unchecked_transaction().map_err(sql)?;
    let version: i64 = tx
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .map_err(sql)?;
    if !matches!(version, 1 | 2) {
        return Err(Error::Store("unsupported archive schema"));
    }
    let tables = {
        let mut q = tx
            .prepare("SELECT name FROM sqlite_schema WHERE type='table' ORDER BY name LIMIT 10")
            .map_err(sql)?;
        q.query_map([], |r| r.get::<_, String>(0))
            .map_err(sql)?
            .collect::<std::result::Result<BTreeSet<_>, _>>()
            .map_err(sql)?
    };
    let mut expected: BTreeSet<String> = [
        "actors",
        "events",
        "messages",
        "meta",
        "routes",
        "sqlite_sequence",
        "tasks",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    if version == 2 {
        expected.extend(["message_dispositions", "schema_migrations"].map(str::to_owned));
    }
    if tables != expected {
        return Err(Error::Store("unexpected archive tables"));
    }
    let check: String = tx
        .query_row("PRAGMA quick_check", [], |r| r.get(0))
        .map_err(sql)?;
    if check != "ok" {
        return Err(Error::Store("archive integrity failure"));
    }
    integrity::validate(&tx, &max_limits())?;
    let (stored,root,clock):(String,String,String)=tx.query_row(
        "SELECT (SELECT value FROM meta WHERE key='workspace_id'),(SELECT value FROM meta WHERE key='workspace_root'),(SELECT value FROM meta WHERE key='clock')",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)),
    ).map_err(sql)?;
    if stored != workspace || root != workspace_root {
        return Err(Error::Store("archive workspace mismatch"));
    }
    let (actors,routes,messages,tasks,events):(i64,i64,i64,i64,i64)=tx.query_row(
        "SELECT (SELECT count(*) FROM actors),(SELECT count(*) FROM routes),(SELECT count(*) FROM messages),(SELECT count(*) FROM tasks),(SELECT count(*) FROM events)",[],
        |r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?)),
    ).map_err(sql)?;
    let watermark: i64 = tx
        .query_row(
            "SELECT COALESCE((SELECT seq FROM sqlite_sequence WHERE name='events'),0)",
            [],
            |r| r.get(0),
        )
        .map_err(sql)?;
    if watermark < 0 {
        return Err(Error::Store("invalid archive watermark"));
    }
    let (dispositions, migrations) = if version == 2 {
        let (d, m): (i64, i64) = tx.query_row(
            "SELECT (SELECT count(*) FROM message_dispositions),(SELECT count(*) FROM schema_migrations)", [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        ).map_err(sql)?;
        (Some(d.to_string()), Some(m.to_string()))
    } else {
        (None, None)
    };
    tx.commit().map_err(sql)?;
    Ok(Manifest {
        archive_version: 1,
        archive_id: archive_id.into(),
        workspace_id: stored,
        database_schema: version as u32,
        database_bytes: image.bytes.to_string(),
        database_sha256: image.hash.clone(),
        counts: Counts {
            actors: actors.to_string(),
            routes: routes.to_string(),
            messages: messages.to_string(),
            tasks: tasks.to_string(),
            events: events.to_string(),
            message_dispositions: dispositions,
            schema_migrations: migrations,
        },
        clock_watermark: clock,
        event_watermark: watermark.to_string(),
    })
}
fn describe(
    path: &Path,
    archive_id: &str,
    workspace: &str,
    workspace_root: &str,
) -> Result<Manifest> {
    let mut image = DatabaseImage::open(path)?;
    let manifest = describe_image(&image, archive_id, workspace, workspace_root)?;
    image.recheck()?;
    Ok(manifest)
}

pub(super) struct VerifiedArchive {
    image: DatabaseImage,
    manifest: Manifest,
    manifest_file: File,
    manifest_bytes: Vec<u8>,
    dir: PathBuf,
}
impl VerifiedArchive {
    pub(super) fn database(&self) -> &Connection {
        &self.image.db
    }
    pub(super) fn database_schema(&self) -> u32 {
        self.manifest.database_schema
    }
    pub(super) fn database_hash(&self) -> &str {
        &self.manifest.database_sha256
    }
    pub(super) fn receipt(&self) -> Value {
        receipt(&self.dir, &self.manifest)
    }
    pub(super) fn recheck(&mut self) -> Result<()> {
        bundle_names(&self.dir)?;
        let path = self.dir.join(MANIFEST);
        same_file(&self.manifest_file, &path)?;
        self.manifest_file.seek(SeekFrom::Start(0))?;
        let mut after = Vec::new();
        (&mut self.manifest_file)
            .take(MANIFEST_BYTES + 1)
            .read_to_end(&mut after)?;
        if after != self.manifest_bytes {
            return Err(Error::Store("archive manifest changed"));
        }
        same_file(&self.manifest_file, &path)?;
        self.image.recheck()
    }
}
fn bundle_names(dir: &Path) -> Result<()> {
    existing_dir(dir)?;
    let names = fs::read_dir(dir)?
        .take(3)
        .map(|e| e.map(|e| e.file_name()))
        .collect::<std::io::Result<BTreeSet<_>>>()?;
    if names
        != [DATABASE, MANIFEST]
            .into_iter()
            .map(std::ffi::OsString::from)
            .collect()
    {
        return Err(Error::Store("incomplete or unexpected archive bundle"));
    }
    Ok(())
}
fn verify_bundle(
    dir: &Path,
    archive_id: &str,
    workspace: &str,
    workspace_root: &str,
) -> Result<VerifiedArchive> {
    bundle_names(dir)?;
    let path = dir.join(MANIFEST);
    let mut file = existing_file(&path, MANIFEST_BYTES)?;
    let mut data = Vec::new();
    (&mut file)
        .take(MANIFEST_BYTES + 1)
        .read_to_end(&mut data)?;
    if data.len() as u64 > MANIFEST_BYTES {
        return Err(Error::Store("archive manifest size limit"));
    }
    let manifest: Manifest = serde_json::from_value(strict_json(&data)?)
        .map_err(|_| Error::Store("invalid archive manifest"))?;
    if manifest.archive_version != 1
        || manifest.archive_id != archive_id
        || manifest.workspace_id != workspace
    {
        return Err(Error::Store("archive manifest identity mismatch"));
    }
    let image = DatabaseImage::open(&dir.join(DATABASE))?;
    let actual = describe_image(&image, archive_id, workspace, workspace_root)?;
    if actual != manifest {
        return Err(Error::Store("archive manifest does not match database"));
    }
    let mut archive = VerifiedArchive {
        image,
        manifest,
        manifest_file: file,
        manifest_bytes: data,
        dir: dir.into(),
    };
    archive.recheck()?;
    Ok(archive)
}
fn receipt(dir: &Path, manifest: &Manifest) -> Value {
    json!({"status":"verified","archive_id":manifest.archive_id,"archive_dir":dir,
        "workspace_id":manifest.workspace_id,"database_schema":manifest.database_schema,
        "database_bytes":manifest.database_bytes,"database_sha256":manifest.database_sha256,
        "counts":manifest.counts,"event_watermark":manifest.event_watermark,"clock_watermark":manifest.clock_watermark,
        "content":"sensitive_coordination_data_and_credential_hashes_not_execution_evidence",
        "restoration_supported":false,"source_pruned":false})
}
/// The caller supplies complete synced files. Failure retains exact private staging.
pub(super) fn publish(
    root: &Path,
    id: &str,
    write: impl FnOnce(&Path) -> Result<()>,
) -> Result<PathBuf> {
    let staging = root.join(format!(".pending-{}", uuid::Uuid::new_v4()));
    fs::DirBuilder::new().mode(0o700).create(&staging)?;
    write(&staging)?;
    File::open(&staging)?.sync_all()?;
    let target = root.join(id);
    rustix::fs::renameat_with(
        rustix::fs::CWD,
        &staging,
        rustix::fs::CWD,
        &target,
        rustix::fs::RenameFlags::NOREPLACE,
    )
    .map_err(|e| Error::from(std::io::Error::from(e)))?;
    File::open(root)?.sync_all()?;
    Ok(target)
}
impl Store {
    fn archive_workspace_root(&self) -> Result<String> {
        self.db
            .query_row(
                "SELECT value FROM meta WHERE key='workspace_root'",
                [],
                |r| r.get(0),
            )
            .map_err(sql)
    }
    pub fn inspect_archive(&self, id: &str) -> Result<Value> {
        Ok(self.verified_archive(id)?.receipt())
    }
    pub(super) fn verified_archive(&self, id: &str) -> Result<VerifiedArchive> {
        uuid(id)?;
        let root = self.state_dir.join("archives");
        existing_dir(&root)?;
        let dir = root.join(id);
        verify_bundle(
            &dir,
            id,
            &self.workspace_id,
            &self.archive_workspace_root()?,
        )
    }
    pub fn archive(&mut self, id: &str) -> Result<Value> {
        uuid(id)?;
        let root = self.state_dir.join("archives");
        private_dir(&root)?;
        File::open(&self.state_dir)?.sync_all()?;
        if present(&root.join(id))? {
            let mut archive = self.verified_archive(id)?;
            // Rename may have succeeded before a prior archive-root sync failed.
            // Verification alone cannot finish that publication boundary.
            File::open(&root)?.sync_all()?;
            archive.recheck()?;
            return Ok(archive.receipt());
        }
        let (entries, retained) = usage(&root)?;
        // Reserve the full per-database cap, not an optimistic live row estimate.
        if entries >= ROOT_ENTRIES || retained + DATABASE_BYTES + MANIFEST_BYTES > TOTAL_BYTES {
            return Err(Error::Store("archive capacity; retained files preserved"));
        }
        let size: i64 = self
            .db
            .query_row(
                "SELECT page_count*page_size FROM pragma_page_count(),pragma_page_size()",
                [],
                |r| r.get(0),
            )
            .map_err(sql)?;
        if size <= 0 || size as u64 > DATABASE_BYTES {
            return Err(Error::Store("source database exceeds archive bound"));
        }
        let workspace_root = self.archive_workspace_root()?;
        let dir = publish(&root, id, |staging| {
            let path = staging.join(DATABASE);
            let file = OpenOptions::new()
                .read(true)
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&path)?;
            // Pinned SQLite 3.53.2 accepts an existing empty output file and copies
            // the logical read snapshot, including committed WAL and sqlite_sequence.
            self.db
                .execute(
                    "VACUUM main INTO ?1",
                    [path
                        .to_str()
                        .ok_or(Error::Store("archive path must be UTF-8"))?],
                )
                .map_err(sql)?;
            same_file(&file, &path)?;
            file.sync_all()?;
            let manifest = describe(&path, id, &self.workspace_id, &workspace_root)?;
            let body = serde_json::to_vec(&manifest)
                .map_err(|_| Error::Store("archive manifest encoding"))?;
            if body.len() as u64 > MANIFEST_BYTES {
                return Err(Error::Store("archive manifest size limit"));
            }
            let mut output = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(staging.join(MANIFEST))?;
            output.write_all(&body)?;
            output.sync_all()?;
            verify_bundle(staging, id, &self.workspace_id, &workspace_root)?;
            Ok(())
        })?;
        let archive = verify_bundle(&dir, id, &self.workspace_id, &workspace_root)?;
        Ok(archive.receipt())
    }
}
