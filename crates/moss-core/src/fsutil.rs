//! Reading, writing and listing files inside a Project folder, safely.
//!
//! Every path an agent or the UI passes in is a `/`-separated string relative to the Project.
//! `safe_join` is the only way to turn one into a real path.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use walkdir::WalkDir;

use crate::ignore::{FileKind, is_ignored, kind_of, normalize_to_lf};
use crate::names::check_component;
use crate::{Error, Result};

/// Resolve `rel` under `project_dir`, refusing anything that could escape it, touch Moss's own
/// `.moss` folder, or use a name Windows cannot hold.
pub fn safe_join(project_dir: &Path, rel: &str) -> Result<PathBuf> {
    if rel.is_empty() || rel.starts_with('/') {
        return Err(Error::UnsafePath(rel.to_string()));
    }
    let mut out = project_dir.to_path_buf();
    for (i, component) in rel.split('/').enumerate() {
        if matches!(component, "" | "." | "..") {
            return Err(Error::UnsafePath(rel.to_string()));
        }
        if i == 0 && (component == ".moss" || component == ".git") {
            return Err(Error::UnsafePath(rel.to_string()));
        }
        if let Some(problem) = check_component(component) {
            return Err(Error::InvalidName {
                name: component.to_string(),
                reason: problem.to_string(),
            });
        }
        out.push(component);
    }
    Ok(long_path(out))
}

/// Windows refuses paths over 260 characters unless they carry the `\\?\` prefix, and a Project
/// deep inside Documents with a long name and nested chapter folders gets there. Add the prefix
/// only when needed: it also turns off the OS's own `/` handling, so the path is made all
/// backslashes first. Other tools (`latexmk`) are given the normal path, never this one.
fn long_path(path: PathBuf) -> PathBuf {
    const LIMIT: usize = 240;
    if !cfg!(windows) || path.as_os_str().len() < LIMIT {
        return path;
    }
    let text = path.to_string_lossy();
    if text.starts_with(r"\\?\") || !path.is_absolute() {
        return path;
    }
    PathBuf::from(format!(r"\\?\{}", text.replace('/', "\\")))
}

pub fn read_bytes(project_dir: &Path, rel: &str) -> Result<Vec<u8>> {
    let path = safe_join(project_dir, rel)?;
    fs::read(&path).map_err(|e| Error::io(format!("reading {rel}"), e))
}

pub fn read_text(project_dir: &Path, rel: &str) -> Result<String> {
    String::from_utf8(read_bytes(project_dir, rel)?).map_err(|_| Error::NotUtf8(rel.to_string()))
}

/// Write bytes (LF-normalized for text files) via a temporary file so a crash never leaves a
/// half-written source file behind. Returns true when the content actually changed.
pub fn write_bytes(project_dir: &Path, rel: &str, bytes: &[u8]) -> Result<bool> {
    let path = safe_join(project_dir, rel)?;
    let data = normalize_to_lf(rel, bytes);
    if fs::read(&path).is_ok_and(|existing| existing == *data) {
        return Ok(false);
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| Error::io(format!("creating folder for {rel}"), e))?;
    }
    let tmp = PathBuf::from(format!("{}.moss-tmp", path.display()));
    fs::write(&tmp, &data).map_err(|e| Error::io(format!("writing {rel}"), e))?;
    fs::rename(&tmp, &path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        Error::io(format!("replacing {rel}"), e)
    })?;
    Ok(true)
}

pub fn write_text(project_dir: &Path, rel: &str, content: &str) -> Result<bool> {
    write_bytes(project_dir, rel, content.as_bytes())
}

/// Would `write_text` change the file? Lets a caller skip taking a restore point for a no-op.
pub fn would_change(project_dir: &Path, rel: &str, content: &str) -> Result<bool> {
    let path = safe_join(project_dir, rel)?;
    let data = normalize_to_lf(rel, content.as_bytes());
    Ok(!fs::read(&path).is_ok_and(|existing| existing == *data))
}

#[derive(Debug, Clone, Serialize)]
pub struct FileEntry {
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    pub kind: FileKind,
}

/// List every non-ignored file and folder in the Project, folders first within each level.
/// When any source file in the Project (generated files and `.moss` excluded) last changed.
/// Compared with a PDF's time, it says whether the PDF still shows the current sources.
pub fn newest_modified(project_dir: &Path) -> Option<std::time::SystemTime> {
    WalkDir::new(project_dir)
        .min_depth(1)
        .into_iter()
        .filter_entry(|e| {
            e.path()
                .strip_prefix(project_dir)
                .ok()
                .and_then(|p| p.to_str())
                .map(|p| !is_ignored(&p.replace('\\', "/")))
                .unwrap_or(false)
        })
        .flatten()
        .filter(|e| e.file_type().is_file())
        .filter_map(|e| e.metadata().ok()?.modified().ok())
        .max()
}

pub fn list_files(project_dir: &Path) -> Result<Vec<FileEntry>> {
    let mut out = Vec::new();
    let walker = WalkDir::new(project_dir)
        .min_depth(1)
        .sort_by(|a, b| {
            b.file_type()
                .is_dir()
                .cmp(&a.file_type().is_dir())
                .then_with(|| a.file_name().cmp(b.file_name()))
        })
        .into_iter()
        .filter_entry(|e| {
            e.path()
                .strip_prefix(project_dir)
                .ok()
                .and_then(|p| p.to_str())
                .map(|p| !is_ignored(&p.replace('\\', "/")))
                .unwrap_or(false)
        });
    for entry in walker {
        let entry = entry.map_err(|e| Error::io("listing project files", e.into()))?;
        let rel = entry
            .path()
            .strip_prefix(project_dir)
            .ok()
            .and_then(|p| p.to_str())
            .map(|p| p.replace('\\', "/"));
        let Some(rel) = rel else { continue };
        let is_dir = entry.file_type().is_dir();
        let size = if is_dir {
            0
        } else {
            entry.metadata().map(|m| m.len()).unwrap_or(0)
        };
        out.push(FileEntry {
            kind: kind_of(&rel, is_dir),
            path: rel,
            is_dir,
            size,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_join_blocks_escapes_and_moss_folder() {
        let base = Path::new("C:/p");
        for bad in ["../x", "a/../../x", "/abs", "", "a//b", ".moss/project.json", ".git/config", "a/./b"] {
            assert!(safe_join(base, bad).is_err(), "{bad} should be rejected");
        }
        assert!(matches!(
            safe_join(base, "chapters/aux.tex"),
            Err(Error::InvalidName { .. })
        ));
        assert!(safe_join(base, "chapters/intro.tex").is_ok());
    }

    #[cfg(windows)]
    #[test]
    fn paths_beyond_260_characters_can_be_written_and_read() {
        let tmp = tempfile::tempdir().unwrap();
        let deep = format!("{0}/{0}/{0}/{0}/{0}/{0}/chapter.tex", "a_rather_long_folder_name_to_use_up_room");
        assert!(tmp.path().as_os_str().len() + deep.len() > 260);
        assert!(write_text(tmp.path(), &deep, "long").unwrap());
        assert_eq!(read_text(tmp.path(), &deep).unwrap(), "long");
    }

    #[test]
    fn write_normalizes_line_endings_and_skips_noop_writes() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(write_text(tmp.path(), "a/b.tex", "x\r\ny\r\n").unwrap());
        assert_eq!(read_text(tmp.path(), "a/b.tex").unwrap(), "x\ny\n");
        assert!(!write_text(tmp.path(), "a/b.tex", "x\ny\n").unwrap(), "unchanged");
        assert!(!tmp.path().join("a/b.tex.moss-tmp").exists());
    }

    #[test]
    fn listing_hides_ignored_and_orders_folders_first() {
        let tmp = tempfile::tempdir().unwrap();
        write_text(tmp.path(), "main.tex", "x").unwrap();
        write_text(tmp.path(), "main.aux", "x").unwrap();
        write_text(tmp.path(), "chapters/one.tex", "x").unwrap();
        fs::create_dir_all(tmp.path().join(".moss")).unwrap();
        fs::write(tmp.path().join(".moss/project.json"), "{}").unwrap();
        let paths: Vec<String> = list_files(tmp.path()).unwrap().into_iter().map(|e| e.path).collect();
        assert_eq!(paths, vec!["chapters", "chapters/one.tex", "main.tex"]);
    }
}
