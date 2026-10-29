//! Audit log: one JSON line per change an Agent made, in `<project>/.moss/audit.log`.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

use serde::Serialize;

use crate::project::now_unix;
use crate::{Error, Result};

#[derive(Serialize)]
struct Entry<'a> {
    at: i64,
    actor: &'a str,
    action: &'a str,
    detail: &'a str,
}

/// Append one entry. `actor` is who did it (for example `moss-mcp`).
pub fn record(project_dir: &Path, actor: &str, action: &str, detail: &str) -> Result<()> {
    let dir = project_dir.join(".moss");
    fs::create_dir_all(&dir).map_err(|e| Error::io("creating .moss folder", e))?;
    let mut line = serde_json::to_string(&Entry {
        at: now_unix(),
        actor,
        action,
        detail,
    })
    .map_err(|e| Error::json("writing audit log", e))?;
    line.push('\n');
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("audit.log"))
        .map_err(|e| Error::io("opening audit log", e))?;
    file.write_all(line.as_bytes())
        .map_err(|e| Error::io("writing audit log", e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entries_append_as_json_lines() {
        let dir = tempfile::tempdir().unwrap();
        record(dir.path(), "moss-mcp", "write_file", "main.tex").unwrap();
        record(dir.path(), "moss-mcp", "delete_file", "old.tex").unwrap();
        let log = fs::read_to_string(dir.path().join(".moss/audit.log")).unwrap();
        let lines: Vec<_> = log.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[1].contains("\"action\":\"delete_file\""));
    }
}
