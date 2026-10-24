//! Read-only SQLite over exactly the bytes captured from a retained descriptor.
use super::{DATABASE_BYTES, existing_file, fingerprint, same_file};
use crate::sql;
use hermes_tern_core::error::{Error, Result};
use rusqlite::Connection;
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};

pub(super) struct DatabaseImage {
    pub(super) db: Connection,
    file: File,
    path: PathBuf,
    pub(super) bytes: u64,
    pub(super) hash: String,
}
impl DatabaseImage {
    pub(super) fn open(path: &Path) -> Result<Self> {
        let mut file = existing_file(path, DATABASE_BYTES)?;
        let bytes = file.metadata()?.len();
        // Recheck the exact size used for allocation, not only the prior open check.
        if bytes > DATABASE_BYTES {
            return Err(Error::Store("archive database size limit"));
        }
        let size = usize::try_from(bytes).map_err(|_| Error::Store("archive allocation bound"))?;
        if size < 100 {
            return Err(Error::Store("archive is not a standalone SQLite database"));
        }
        let mut data = Vec::new();
        data.try_reserve_exact(size)
            .map_err(|_| Error::Store("archive image allocation refused"))?;
        data.resize(size, 0);
        file.read_exact(&mut data)?;
        if file.read(&mut [0_u8; 1])? != 0 {
            return Err(Error::Store("archive changed during read"));
        }
        if &data[..16] != b"SQLite format 3\0" || data[18] != 1 || data[19] != 1 {
            return Err(Error::Store("archive is not a standalone SQLite database"));
        }
        let hash = Sha256::digest(&data)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        let mut db = Connection::open_in_memory().map_err(sql)?;
        // The pinned safe helper allocates before read_exact. A complete slice
        // cannot fail/short-read here; fallible file I/O finished before allocation.
        db.deserialize_read_exact(rusqlite::MAIN_DB, data.as_slice(), size, true)
            .map_err(sql)?;
        drop(data);
        db.execute_batch("PRAGMA query_only=ON; PRAGMA trusted_schema=OFF;")
            .map_err(sql)?;
        let mut image = Self {
            db,
            file,
            path: path.into(),
            bytes,
            hash,
        };
        image.recheck()?;
        Ok(image)
    }
    pub(super) fn sync(&self) -> Result<()> {
        self.file.sync_all().map_err(Error::from)
    }
    pub(super) fn relocate(&mut self, path: &Path) -> Result<()> {
        same_file(&self.file, path)?;
        self.path = path.to_owned();
        Ok(())
    }
    pub(super) fn recheck(&mut self) -> Result<()> {
        same_file(&self.file, &self.path)?;
        if fingerprint(&mut self.file)? != (self.bytes, self.hash.clone()) {
            return Err(Error::Store("archive changed after image capture"));
        }
        same_file(&self.file, &self.path)
    }
}
