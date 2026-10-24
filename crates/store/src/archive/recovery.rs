//! Finish publication of an explicitly selected, complete retained bundle.
//! This never restores database authority, repairs bytes, or frees retained capacity.
use super::{
    DATABASE, Error, File, MANIFEST, OpenOptions, Path, Result, Store, Value, existing_dir,
    present, uuid, verify_bundle,
};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};

fn directory(path: &Path) -> Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags((rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::DIRECTORY).bits() as i32)
        .open(path)?;
    same_directory(&file, path)?;
    Ok(file)
}
fn same_directory(file: &File, path: &Path) -> Result<()> {
    existing_dir(path)?;
    let opened = file.metadata()?;
    let named = path.symlink_metadata()?;
    if !opened.is_dir()
        || opened.uid() != rustix::process::geteuid().as_raw()
        || opened.mode() & 0o077 != 0
        || opened.dev() != named.dev()
        || opened.ino() != named.ino()
    {
        return Err(Error::Store("archive directory identity changed"));
    }
    Ok(())
}
fn expected_hash(value: &str) -> Result<()> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
    {
        return Err(Error::Store("expected canonical archive digest"));
    }
    Ok(())
}
impl Store {
    pub fn inspect_staged_archive(&self, staging_id: &str, archive_id: &str) -> Result<Value> {
        uuid(staging_id)?;
        uuid(archive_id)?;
        let root = self.state_dir.join("archives");
        existing_dir(&root)?;
        let path = root.join(format!(".pending-{staging_id}"));
        let archive = verify_bundle(
            &path,
            archive_id,
            &self.workspace_id,
            &self.archive_workspace_root()?,
        )?;
        let mut value = archive.receipt();
        value["status"] = serde_json::json!("staged_verified");
        value["published"] = serde_json::json!(false);
        value["staging_id"] = serde_json::json!(staging_id);
        Ok(value)
    }

    pub fn recover_archive(
        &mut self,
        staging_id: &str,
        archive_id: &str,
        database_sha256: &str,
    ) -> Result<Value> {
        uuid(staging_id)?;
        uuid(archive_id)?;
        expected_hash(database_sha256)?;
        let root_path = self.state_dir.join("archives");
        let root = directory(&root_path)?;
        let staging_name = format!(".pending-{staging_id}");
        let staging = root_path.join(&staging_name);
        let target = root_path.join(archive_id);
        let staged = present(&staging)?;
        if staged && present(&target)? {
            return Err(Error::Store(
                "both staged and final archives exist; preserve both",
            ));
        }
        // An absent staging name can only reconcile exact final ID/hash identity.
        // It is not proof the final bundle came from that historical staging UUID.
        let selected = if staged { &staging } else { &target };
        let dir = directory(selected)?;
        let mut archive = verify_bundle(
            selected,
            archive_id,
            &self.workspace_id,
            &self.archive_workspace_root()?,
        )?;
        if archive.database_hash() != database_sha256 {
            return Err(Error::Store("archive differs from expected digest"));
        }
        archive.image.sync()?;
        archive.manifest_file.sync_all()?;
        dir.sync_all()?;
        archive.recheck()?;
        same_directory(&dir, selected)?;
        same_directory(&root, &root_path)?;
        if staged {
            rustix::fs::renameat_with(
                &root,
                &staging_name,
                &root,
                archive_id,
                rustix::fs::RenameFlags::NOREPLACE,
            )
            .map_err(|e| Error::from(std::io::Error::from(e)))?;
            // Retarget only the same retained descriptors, never reopen query bytes.
            same_directory(&dir, &target)?;
            super::same_file(&archive.manifest_file, &target.join(MANIFEST))?;
            archive.image.relocate(&target.join(DATABASE))?;
            archive.dir = target.clone();
        }
        root.sync_all()?;
        same_directory(&root, &root_path)?;
        same_directory(&dir, &target)?;
        archive.recheck()?;
        Ok(archive.receipt())
    }
}
