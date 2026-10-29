//! One sync at a time per Project. The app and `moss-mcp` are separate processes, and a sync can
//! run for minutes while uploading, so two of them applying together could settle the same files
//! twice. This is an operating-system file lock: it frees itself when the process ends, so a crash
//! never leaves a Project stuck.

use std::fs::{self, File, OpenOptions, TryLockError};
use std::path::Path;

use crate::project::META_DIR;
use crate::{Error, Result};

pub(crate) struct SyncLock {
    // Held only so the lock lasts until this value is dropped.
    _file: File,
}

impl SyncLock {
    /// Take the lock, or say that another sync holds it. Never waits: the other sync may be long.
    pub(crate) fn acquire(project_dir: &Path) -> Result<SyncLock> {
        let meta = project_dir.join(META_DIR);
        fs::create_dir_all(&meta).map_err(|e| Error::io("creating the Project's .moss folder", e))?;
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(meta.join("sync.lock"))
            .map_err(|e| Error::io("opening the sync lock", e))?;
        match file.try_lock() {
            Ok(()) => Ok(SyncLock { _file: file }),
            Err(TryLockError::WouldBlock) => Err(Error::Sync(
                "Another sync is already running for this Project (another Moss window or an agent). \
                 Wait for it to finish, then review again."
                    .into(),
            )),
            Err(TryLockError::Error(e)) => Err(Error::io("locking the Project for sync", e)),
        }
    }
}
