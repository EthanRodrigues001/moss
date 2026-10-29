//! Creating, renaming, deleting and adding single files and folders inside a Project, as the
//! file tree does. Every path is Project-relative and goes through `safe_join`, so nothing can
//! reach outside the Project or Moss's own `.moss` folder.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::fsutil::{safe_join, write_bytes};
use crate::names::check_component;
use crate::{Error, Result};

fn entry_err(message: impl Into<String>) -> Error {
    Error::Entry(message.into())
}

/// Create an empty file. Missing folders on the way are created. Fails if it already exists.
pub fn create_file(project_dir: &Path, rel: &str) -> Result<()> {
    let path = safe_join(project_dir, rel)?;
    if path.exists() {
        return Err(entry_err(format!("`{rel}` already exists")));
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| Error::io(format!("creating folder for {rel}"), e))?;
    }
    fs::write(&path, b"").map_err(|e| Error::io(format!("creating {rel}"), e))
}

pub fn create_folder(project_dir: &Path, rel: &str) -> Result<()> {
    let path = safe_join(project_dir, rel)?;
    if path.exists() {
        return Err(entry_err(format!("`{rel}` already exists")));
    }
    fs::create_dir_all(&path).map_err(|e| Error::io(format!("creating {rel}"), e))
}

/// Rename or move a file or folder. Refuses to overwrite, and to move a folder into itself.
pub fn rename(project_dir: &Path, from: &str, to: &str) -> Result<()> {
    let src = safe_join(project_dir, from)?;
    let dst = safe_join(project_dir, to)?;
    if !src.exists() {
        return Err(entry_err(format!("`{from}` does not exist")));
    }
    if to.starts_with(&format!("{from}/")) {
        return Err(entry_err("a folder cannot be moved into itself"));
    }
    // Windows paths are case-insensitive, so `a.tex` -> `A.tex` is a rename, not a clash.
    if dst.exists() && !from.eq_ignore_ascii_case(to) {
        return Err(entry_err(format!("`{to}` already exists")));
    }
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent).map_err(|e| Error::io(format!("creating folder for {to}"), e))?;
    }
    fs::rename(&src, &dst).map_err(|e| Error::io(format!("renaming {from}"), e))
}

pub fn delete(project_dir: &Path, rel: &str) -> Result<()> {
    let path = safe_join(project_dir, rel)?;
    if !path.exists() {
        return Err(entry_err(format!("`{rel}` does not exist")));
    }
    if path.is_dir() {
        fs::remove_dir_all(&path)
    } else {
        fs::remove_file(&path)
    }
    .map_err(|e| Error::io(format!("deleting {rel}"), e))
}

#[derive(Debug, Serialize)]
pub struct Skipped {
    pub name: String,
    pub reason: String,
}

#[derive(Debug, Default, Serialize)]
pub struct AddReport {
    /// Project-relative paths of the files that were added.
    pub added: Vec<String>,
    pub skipped: Vec<Skipped>,
}

/// Copy files from elsewhere on this PC into `folder` (empty for the Project root). Files that
/// already exist or have names Windows cannot hold are skipped, not overwritten.
pub fn add_files(project_dir: &Path, folder: &str, sources: &[PathBuf]) -> Result<AddReport> {
    let mut report = AddReport::default();
    for source in sources {
        let name = source
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let skip = |reason: String| Skipped { name: name.clone(), reason };
        if let Some(problem) = check_component(&name) {
            report.skipped.push(skip(problem.to_string()));
            continue;
        }
        let rel = if folder.is_empty() { name.clone() } else { format!("{folder}/{name}") };
        if safe_join(project_dir, &rel)?.exists() {
            report.skipped.push(skip("a file with this name is already there".into()));
            continue;
        }
        match fs::read(source) {
            Ok(bytes) => {
                write_bytes(project_dir, &rel, &bytes)?;
                report.added.push(rel);
            }
            Err(e) => report.skipped.push(skip(e.to_string())),
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn create_rename_delete_round_trip() {
        let dir = project();
        create_file(dir.path(), "chapters/intro.tex").unwrap();
        assert!(create_file(dir.path(), "chapters/intro.tex").is_err());
        create_folder(dir.path(), "figures").unwrap();
        rename(dir.path(), "chapters/intro.tex", "chapters/start.tex").unwrap();
        assert!(dir.path().join("chapters/start.tex").is_file());
        delete(dir.path(), "chapters").unwrap();
        assert!(!dir.path().join("chapters").exists());
        assert!(delete(dir.path(), "chapters").is_err());
    }

    #[test]
    fn rename_refuses_overwrites_and_loops_but_allows_case_changes() {
        let dir = project();
        create_file(dir.path(), "a.tex").unwrap();
        create_file(dir.path(), "b.tex").unwrap();
        assert!(rename(dir.path(), "a.tex", "b.tex").is_err());
        create_folder(dir.path(), "sec").unwrap();
        assert!(rename(dir.path(), "sec", "sec/inner").is_err());
        rename(dir.path(), "a.tex", "A.tex").unwrap();
    }

    #[test]
    fn nothing_can_reach_outside_the_project() {
        let dir = project();
        assert!(create_file(dir.path(), "../x.tex").is_err());
        assert!(delete(dir.path(), ".moss").is_err());
        assert!(rename(dir.path(), "a.tex", "../b.tex").is_err());
    }

    #[test]
    fn add_files_skips_clashes_and_bad_names() {
        let dir = project();
        let outside = tempfile::tempdir().unwrap();
        let logo = outside.path().join("logo.png");
        fs::write(&logo, [1u8, 2, 3]).unwrap();
        let bad = outside.path().join("aux.txt"); // reserved device name on Windows
        fs::write(&bad, "x").unwrap();

        let first = add_files(dir.path(), "figures", &[logo.clone(), bad]).unwrap();
        assert_eq!(first.added, vec!["figures/logo.png"]);
        assert_eq!(first.skipped.len(), 1);

        let again = add_files(dir.path(), "figures", &[logo]).unwrap();
        assert!(again.added.is_empty());
        assert_eq!(again.skipped.len(), 1);
    }
}
