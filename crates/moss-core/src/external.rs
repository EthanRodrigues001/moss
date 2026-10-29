//! Noticing changes made behind an Agent's back.
//!
//! After every Agent tool call, `mark_seen` stores a cheap fingerprint (size and modified time)
//! of each source file in `.moss/mcp-seen.json`. At the start of the next call, `check` compares
//! the files with it. Anything different was written by something other than Moss's own tools:
//! the user typing in the app, another agent, a sync client. Nothing is hashed unless a file's
//! size or time differs, so this costs one directory walk.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};

use crate::fsutil::list_files;
use crate::{Error, Result};

const SEEN_FILE: &str = "mcp-seen.json";

/// (size in bytes, modified time in nanoseconds since 1970)
type Print = (u64, u128);

#[derive(Debug, Default, Serialize, Deserialize)]
struct Seen {
    files: BTreeMap<String, Print>,
}

/// What differs from the state the last Agent call left behind.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Drift {
    pub added: Vec<String>,
    pub modified: Vec<String>,
    pub deleted: Vec<String>,
}

impl Drift {
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.modified.is_empty() && self.deleted.is_empty()
    }

    pub fn file_count(&self) -> usize {
        self.added.len() + self.modified.len() + self.deleted.len()
    }

    /// "main.tex, chapters/one.tex and 3 more", for a restore point label.
    pub fn summary(&self) -> String {
        let all: Vec<&str> = self
            .modified
            .iter()
            .chain(&self.added)
            .chain(&self.deleted)
            .map(String::as_str)
            .collect();
        if all.len() <= 4 {
            all.join(", ")
        } else {
            format!("{} and {} more", all[..4].join(", "), all.len() - 4)
        }
    }
}

fn prints(project_dir: &Path) -> Result<BTreeMap<String, Print>> {
    let mut out = BTreeMap::new();
    for entry in list_files(project_dir)? {
        if entry.is_dir {
            continue;
        }
        let modified = fs::metadata(project_dir.join(&entry.path))
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_nanos());
        out.insert(entry.path, (entry.size, modified));
    }
    Ok(out)
}

fn seen_path(project_dir: &Path) -> std::path::PathBuf {
    project_dir.join(".moss").join(SEEN_FILE)
}

/// Compare the files with what the last Agent call left. `None` when nothing differs, or when
/// there is no earlier call to compare with (the first one only starts the record).
pub fn check(project_dir: &Path) -> Result<Option<Drift>> {
    let Ok(bytes) = fs::read(seen_path(project_dir)) else { return Ok(None) };
    let Ok(seen) = serde_json::from_slice::<Seen>(&bytes) else { return Ok(None) };
    let now = prints(project_dir)?;
    let mut drift = Drift::default();
    for (path, print) in &now {
        match seen.files.get(path) {
            None => drift.added.push(path.clone()),
            Some(old) if old != print => drift.modified.push(path.clone()),
            Some(_) => {}
        }
    }
    drift.deleted = seen.files.keys().filter(|p| !now.contains_key(*p)).cloned().collect();
    Ok((!drift.is_empty()).then_some(drift))
}

/// Remember the files as they are now.
pub fn mark_seen(project_dir: &Path) -> Result<()> {
    let seen = Seen { files: prints(project_dir)? };
    let dir = project_dir.join(".moss");
    fs::create_dir_all(&dir).map_err(|e| Error::io("creating .moss folder", e))?;
    let json = serde_json::to_vec(&seen).map_err(|e| Error::json("writing the change record", e))?;
    fs::write(seen_path(project_dir), json).map_err(|e| Error::io("writing the change record", e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fsutil::write_text;

    #[test]
    fn the_first_call_only_starts_the_record() {
        let tmp = tempfile::tempdir().unwrap();
        write_text(tmp.path(), "a.tex", "one").unwrap();
        assert_eq!(check(tmp.path()).unwrap(), None);
        mark_seen(tmp.path()).unwrap();
        assert_eq!(check(tmp.path()).unwrap(), None, "nothing changed since");
    }

    #[test]
    fn edits_additions_and_deletions_made_outside_are_reported() {
        let tmp = tempfile::tempdir().unwrap();
        write_text(tmp.path(), "a.tex", "one").unwrap();
        write_text(tmp.path(), "b.tex", "two").unwrap();
        mark_seen(tmp.path()).unwrap();

        write_text(tmp.path(), "a.tex", "one, changed").unwrap();
        write_text(tmp.path(), "c.tex", "new").unwrap();
        fs::remove_file(tmp.path().join("b.tex")).unwrap();

        let drift = check(tmp.path()).unwrap().expect("drift");
        assert_eq!(drift.modified, vec!["a.tex"]);
        assert_eq!(drift.added, vec!["c.tex"]);
        assert_eq!(drift.deleted, vec!["b.tex"]);
        assert_eq!(drift.summary(), "a.tex, c.tex, b.tex");

        mark_seen(tmp.path()).unwrap();
        assert_eq!(check(tmp.path()).unwrap(), None);
    }

    #[test]
    fn generated_files_and_the_moss_folder_are_not_drift() {
        let tmp = tempfile::tempdir().unwrap();
        write_text(tmp.path(), "a.tex", "one").unwrap();
        mark_seen(tmp.path()).unwrap();
        fs::write(tmp.path().join("a.aux"), "x").unwrap();
        fs::write(tmp.path().join(".moss/audit.log"), "x").unwrap();
        assert_eq!(check(tmp.path()).unwrap(), None);
    }
}
