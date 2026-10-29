//! The other side of a Sync, behind one small interface.
//!
//! `OverleafRemote` (in `overleaf::remote`) talks to Overleaf. `FolderRemote` is a folder on this
//! PC that behaves like an Overleaf project: it backs the Practice project. `MemoryRemote` holds
//! the files in memory and is what the tests use, because it can also hold names Windows
//! cannot (`aux.tex`) to prove those are handled.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use super::fileset::FileSet;
use crate::fsutil::{safe_join, write_bytes};
use crate::workspace::Skipped;
use crate::{Error, Result};

/// What the remote holds right now, plus entries Moss will not touch.
#[derive(Debug, Default)]
pub struct Fetched {
    pub files: FileSet,
    /// Entries left alone: names Windows cannot hold, names that differ only by case.
    pub skipped: Vec<Skipped>,
}

pub trait Remote {
    /// Everything in the project, as one consistent snapshot.
    fn fetch(&self) -> Result<Fetched>;
    /// Create or replace a file, creating folders as needed.
    fn upload(&self, path: &str, bytes: &[u8]) -> Result<()>;
    fn delete(&self, path: &str) -> Result<()>;
    /// Move a file. Fails if `from` is missing or `to` exists.
    fn rename(&self, from: &str, to: &str) -> Result<()>;
}

/// A folder acting as an Overleaf project.
pub struct FolderRemote {
    root: PathBuf,
}

impl FolderRemote {
    pub fn new(root: impl Into<PathBuf>) -> Result<FolderRemote> {
        let root = root.into();
        fs::create_dir_all(&root)
            .map_err(|e| Error::io(format!("creating {}", root.display()), e))?;
        Ok(FolderRemote { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}

impl Remote for FolderRemote {
    fn fetch(&self) -> Result<Fetched> {
        Ok(Fetched {
            files: FileSet::from_project_dir(&self.root)?,
            skipped: Vec::new(),
        })
    }

    fn upload(&self, path: &str, bytes: &[u8]) -> Result<()> {
        write_bytes(&self.root, path, bytes).map(|_| ())
    }

    fn delete(&self, path: &str) -> Result<()> {
        let file = safe_join(&self.root, path)?;
        fs::remove_file(&file).map_err(|e| Error::io(format!("deleting {path} on the remote"), e))?;
        remove_empty_parents(&file, &self.root);
        Ok(())
    }

    fn rename(&self, from: &str, to: &str) -> Result<()> {
        let (a, b) = (safe_join(&self.root, from)?, safe_join(&self.root, to)?);
        if !a.is_file() {
            return Err(Error::Sync(format!("cannot rename {from}: it is not on the remote")));
        }
        if b.exists() {
            return Err(Error::Sync(format!("cannot rename to {to}: that name is taken")));
        }
        if let Some(parent) = b.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| Error::io(format!("creating folder for {to}"), e))?;
        }
        fs::rename(&a, &b).map_err(|e| Error::io(format!("renaming {from}"), e))?;
        remove_empty_parents(&a, &self.root);
        Ok(())
    }
}

fn remove_empty_parents(file: &Path, stop_at: &Path) {
    let mut dir = file.parent();
    while let Some(d) = dir {
        if d == stop_at || fs::remove_dir(d).is_err() {
            break;
        }
        dir = d.parent();
    }
}

/// An in-memory project. Cloning shares the same files, so a test can edit "Overleaf" while a
/// sync holds the other handle.
#[derive(Clone, Default)]
pub struct MemoryRemote {
    files: Arc<Mutex<BTreeMap<String, Vec<u8>>>>,
}

impl MemoryRemote {
    pub fn new() -> MemoryRemote {
        MemoryRemote::default()
    }

    pub fn with(files: &[(&str, &str)]) -> MemoryRemote {
        let remote = MemoryRemote::new();
        for (path, text) in files {
            remote.put(path, text.as_bytes());
        }
        remote
    }

    pub fn put(&self, path: &str, bytes: &[u8]) {
        self.files.lock().unwrap().insert(path.to_string(), bytes.to_vec());
    }

    pub fn remove(&self, path: &str) {
        self.files.lock().unwrap().remove(path);
    }

    pub fn get(&self, path: &str) -> Option<Vec<u8>> {
        self.files.lock().unwrap().get(path).cloned()
    }

    pub fn text(&self, path: &str) -> Option<String> {
        self.get(path).and_then(|b| String::from_utf8(b).ok())
    }

    pub fn paths(&self) -> Vec<String> {
        self.files.lock().unwrap().keys().cloned().collect()
    }
}

impl Remote for MemoryRemote {
    fn fetch(&self) -> Result<Fetched> {
        // Go through a zip, as the real download does, so the same filtering applies.
        let mut raw = FileSet::default();
        for (path, bytes) in self.files.lock().unwrap().iter() {
            raw.insert(path, bytes);
        }
        let (files, skipped) = FileSet::from_zip(&raw.to_zip()?)?;
        Ok(Fetched { files, skipped })
    }

    fn upload(&self, path: &str, bytes: &[u8]) -> Result<()> {
        self.put(path, bytes);
        Ok(())
    }

    fn delete(&self, path: &str) -> Result<()> {
        match self.files.lock().unwrap().remove(path) {
            Some(_) => Ok(()),
            None => Err(Error::Sync(format!("cannot delete {path}: it is not on the remote"))),
        }
    }

    fn rename(&self, from: &str, to: &str) -> Result<()> {
        let mut files = self.files.lock().unwrap();
        if files.contains_key(to) {
            return Err(Error::Sync(format!("cannot rename to {to}: that name is taken")));
        }
        let bytes = files
            .remove(from)
            .ok_or_else(|| Error::Sync(format!("cannot rename {from}: it is not on the remote")))?;
        files.insert(to.to_string(), bytes);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_remote_behaves_like_a_project() {
        let tmp = tempfile::tempdir().unwrap();
        let r = FolderRemote::new(tmp.path().join("practice")).unwrap();
        r.upload("main.tex", b"one\r\n").unwrap();
        r.upload("chapters/a.tex", b"a").unwrap();
        let f = r.fetch().unwrap();
        assert_eq!(f.files.len(), 2);
        assert_eq!(f.files.get("main.tex").unwrap().bytes, b"one\n");

        r.rename("chapters/a.tex", "sections/b.tex").unwrap();
        assert!(!r.root().join("chapters").exists(), "empty folder removed");
        assert!(r.rename("main.tex", "sections/b.tex").is_err(), "name taken");
        assert!(r.rename("missing.tex", "x.tex").is_err());

        r.delete("main.tex").unwrap();
        assert!(r.delete("main.tex").is_err());
        assert_eq!(r.fetch().unwrap().files.len(), 1);
    }

    #[test]
    fn memory_remote_filters_like_a_real_download() {
        let r = MemoryRemote::with(&[("ok.tex", "x"), ("aux.tex", "x"), ("a.aux", "junk")]);
        let f = r.fetch().unwrap();
        assert_eq!(f.files.files.keys().collect::<Vec<_>>(), ["ok.tex"]);
        assert_eq!(f.skipped.len(), 1);
        assert_eq!(f.skipped[0].path, "aux.tex");
    }
}
