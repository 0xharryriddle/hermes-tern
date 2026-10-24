//! Private secret publication; caller owns the supervisor's exclusive store lock.
use hermes_tern_core::error::{Error, Result};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
};

#[cfg(test)]
mod reader_tests;

/// A bounded private read; plain readers never synchronize or publish anything.
pub(super) struct PrivateFile {
    file: File,
    path: PathBuf,
    text: String,
}
impl PrivateFile {
    pub(super) fn open(path: &Path) -> Result<Self> {
        let mut file = OpenOptions::new()
            .read(true)
            .custom_flags(
                (rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::NONBLOCK).bits() as i32,
            )
            .open(path)?;
        same_file(&file, path)?;
        let text = read_bounded(&mut file)?;
        let mut value = Self {
            file,
            path: path.into(),
            text,
        };
        value.recheck()?;
        Ok(value)
    }
    pub(super) fn text(&self) -> &str {
        &self.text
    }
    pub(super) fn into_text(self) -> String {
        self.text
    }
    pub(super) fn recheck(&mut self) -> Result<()> {
        same_file(&self.file, &self.path)?;
        if read_bounded(&mut self.file)? != self.text {
            return Err(Error::Store("private file changed during verification"));
        }
        same_file(&self.file, &self.path)
    }
    /// Only the store-lock owner may finish an existing file's publication.
    pub(super) fn sync(&mut self) -> Result<()> {
        self.recheck()?;
        let parent = self
            .path
            .parent()
            .ok_or(Error::Store("private file parent required"))?;
        let dir = open_directory(parent)?;
        same_directory(&dir, parent)?;
        self.file.sync_all()?;
        dir.sync_all()?;
        same_directory(&dir, parent)?;
        self.recheck()
    }

    /// Move this complete retained file to its fixed sibling name, never replace.
    /// Caller validates current registry authority while holding writer exclusion.
    pub(super) fn recover_to(&mut self, target: &Path) -> Result<()> {
        let parent = self
            .path
            .parent()
            .ok_or(Error::Store("private file parent required"))?
            .to_owned();
        if target.parent() != Some(parent.as_path()) {
            return Err(Error::Store(
                "credential recovery requires sibling destination",
            ));
        }
        let state = parent
            .parent()
            .ok_or(Error::Store("private directory parent required"))?;
        let state_dir = open_directory(state)?;
        let dir = open_directory(&parent)?;
        same_directory(&state_dir, state)?;
        same_directory(&dir, &parent)?;
        self.recheck()?;
        // Complete the credentials directory entry's durability before the move.
        state_dir.sync_all()?;
        self.file.sync_all()?;
        dir.sync_all()?;
        same_directory(&state_dir, state)?;
        same_directory(&dir, &parent)?;
        self.recheck()?;
        if self.path != target {
            rustix::fs::renameat_with(
                &dir,
                self.path
                    .file_name()
                    .ok_or(Error::Store("private file name required"))?,
                &dir,
                target
                    .file_name()
                    .ok_or(Error::Store("private target name required"))?,
                rustix::fs::RenameFlags::NOREPLACE,
            )
            .map_err(|e| Error::from(std::io::Error::from(e)))?;
            same_file(&self.file, target)?;
            self.path = target.into();
        }
        dir.sync_all()?;
        same_directory(&state_dir, state)?;
        same_directory(&dir, &parent)?;
        self.recheck()
    }
}
fn read_bounded(file: &mut File) -> Result<String> {
    file.seek(SeekFrom::Start(0))?;
    let mut text = String::new();
    file.take(4097).read_to_string(&mut text)?;
    if text.len() > 4096 {
        return Err(Error::Store("private file exceeds limit"));
    }
    Ok(text)
}
fn same_file(file: &File, path: &Path) -> Result<()> {
    let opened = file.metadata()?;
    let named = path.symlink_metadata()?;
    if !opened.is_file()
        || opened.uid() != rustix::process::geteuid().as_raw()
        || opened.mode() & 0o077 != 0
        || opened.nlink() != 1
        || opened.len() > 4096
        || !named.is_file()
        || opened.dev() != named.dev()
        || opened.ino() != named.ino()
        || opened.len() != named.len()
    {
        return Err(Error::Store("unsafe or changed private file"));
    }
    Ok(())
}
pub(super) fn open_directory(path: &Path) -> Result<File> {
    OpenOptions::new()
        .read(true)
        .custom_flags((rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::DIRECTORY).bits() as i32)
        .open(path)
        .map_err(Error::from)
}
pub(super) fn same_directory(file: &File, path: &Path) -> Result<()> {
    let opened = file.metadata()?;
    let named = path.symlink_metadata()?;
    if !opened.is_dir()
        || !named.is_dir()
        || opened.uid() != rustix::process::geteuid().as_raw()
        || opened.mode() & 0o077 != 0
        || opened.dev() != named.dev()
        || opened.ino() != named.ino()
    {
        return Err(Error::Store("unsafe or changed private directory"));
    }
    Ok(())
}

/// Sync the containing directory entry even on retry after an earlier sync failed.
pub(super) fn prepare_directory(path: &Path) -> Result<()> {
    hermes_tern_store::private_dir(path)?;
    let dir = open_directory(path)?;
    same_directory(&dir, path)?;
    let parent = path
        .parent()
        .ok_or(Error::Store("private directory parent required"))?;
    open_directory(parent)?.sync_all()?;
    same_directory(&dir, path)
}

pub(super) fn create(path: &Path, body: &str) -> Result<()> {
    if body.len() > 4096 {
        return Err(Error::Store("private file exceeds limit"));
    }
    publish(path, |file| {
        file.write_all(body.as_bytes())?;
        Ok(())
    })
}

fn publish(path: &Path, write: impl FnOnce(&mut File) -> Result<()>) -> Result<()> {
    let parent = path
        .parent()
        .ok_or(Error::Store("secret parent required"))?;
    prepare_directory(parent)?;
    // Retain failed/uncommitted publications, but never grow them without bound.
    // Only the store-lock owner calls this path; peers cannot provision files.
    if std::fs::read_dir(parent)?.take(1024).count() >= 1024 {
        return Err(Error::Store(
            "private publication capacity; operator reconciliation required",
        ));
    }
    if path.symlink_metadata().is_ok() {
        return Err(Error::Store("private publication target exists"));
    }
    let staging = parent.join(format!(".pending-{}", uuid::Uuid::new_v4()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&staging)?;
    write(&mut file)?;
    file.sync_all()?;
    rustix::fs::renameat_with(
        rustix::fs::CWD,
        &staging,
        rustix::fs::CWD,
        path,
        rustix::fs::RenameFlags::NOREPLACE,
    )
    .map_err(|e| Error::from(std::io::Error::from(e)))?;
    File::open(parent)?.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        os::unix::fs::{DirBuilderExt, MetadataExt},
    };
    fn fixture() -> std::path::PathBuf {
        let root = std::path::PathBuf::from(std::env::var_os("TMPDIR").unwrap())
            .join(format!("ht-secret-write-{}", uuid::Uuid::new_v4()));
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        root
    }
    fn inventory(root: &Path) {
        let paths: Vec<_> = fs::read_dir(root)
            .unwrap()
            .map(|p| p.unwrap().path())
            .collect();
        fs::write(root.join("ownership.json"),serde_json::json!({"root":root,"retention":"retain; no deletion","paths":paths,"manifest":root.join("ownership.json")}).to_string()).unwrap();
    }
    #[test]
    fn partial_write_failure_never_publishes_a_final_secret() {
        let root = fixture();
        let path = root.join("operator.key");
        let outcome = publish(&path, |file| {
            file.write_all(b"partial")?;
            file.sync_all()?;
            Err(Error::Io(std::io::ErrorKind::StorageFull))
        });
        inventory(&root);
        assert!(outcome.is_err());
        assert!(
            !path.exists(),
            "partial secret was exposed under final name"
        );
        create(&path, "fixture-complete").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "fixture-complete");
        assert_eq!(path.metadata().unwrap().mode() & 0o777, 0o600);
        inventory(&root);
    }
    #[test]
    fn racing_final_path_is_never_replaced() {
        let root = fixture();
        let path = root.join("binding.json");
        let mut winner = 0;
        let outcome = publish(&path, |file| {
            file.write_all(b"candidate")?;
            let mut peer = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&path)?;
            peer.write_all(b"winner")?;
            winner = peer.metadata()?.ino();
            Ok(())
        });
        assert!(outcome.is_err());
        assert_eq!(fs::read(&path).unwrap(), b"winner");
        assert_eq!(path.metadata().unwrap().ino(), winner);
        inventory(&root);
    }
    #[test]
    fn private_publication_refuses_oversize_and_stops_at_directory_capacity() {
        let root = fixture();
        let oversized = root.join("oversized");
        assert!(
            create(&oversized, &"x".repeat(4097)).is_err(),
            "oversized private file accepted"
        );
        assert!(!oversized.exists());
        for i in 0..1023 {
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(root.join(format!("retained-{i}")))
                .unwrap();
        }
        create(&root.join("last"), "fixture").unwrap();
        assert!(create(&root.join("overflow"), "fixture").is_err());
        assert!(!root.join("overflow").exists());
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1024);
        inventory(&root);
    }
    #[test]
    fn final_secret_is_absent_until_writer_finishes_and_conflicts_never_clobber() {
        let root = fixture();
        let path = root.join("binding.json");
        let mut observed_absent = false;
        publish(&path, |file| {
            file.write_all(b"fixture")?;
            observed_absent = !path.exists();
            Ok(())
        })
        .unwrap();
        let before = path.metadata().unwrap().ino();
        assert!(create(&path, "replacement").is_err());
        assert_eq!(fs::read(&path).unwrap(), b"fixture");
        assert_eq!(path.metadata().unwrap().ino(), before);
        inventory(&root);
        assert!(
            observed_absent,
            "secret visible before write and fsync completed"
        );
    }
}
