//! Approvals: a click in the Moss app that an Agent waits for before a risky action runs.
//!
//! `moss-mcp` and the desktop app are separate processes, so the queue is a folder of small JSON
//! files in the Workspace (`.approvals/<id>.json`). The agent side writes a pending request and
//! polls it; the app lists pending requests and records the answer. An unanswered request is
//! denied after five minutes.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::project::now_unix;
use crate::{Error, Result};

/// How long an Agent waits for a click before the request counts as denied.
pub const APPROVAL_TIMEOUT: Duration = Duration::from_secs(5 * 60);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Pending,
    Approved,
    Denied,
    /// Nobody answered in time.
    Expired,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Approval {
    pub id: String,
    /// Folder name of the Project the action targets.
    pub project: String,
    /// Short tool name, e.g. `delete_file`.
    pub action: String,
    /// Human-readable description of exactly what will happen.
    pub detail: String,
    pub created_at: i64,
    pub status: Status,
}

pub struct Approvals {
    dir: PathBuf,
}

impl Approvals {
    pub fn new(workspace_root: &Path) -> Approvals {
        Approvals {
            dir: workspace_root.join(".approvals"),
        }
    }

    pub fn request(&self, project: &str, action: &str, detail: &str) -> Result<Approval> {
        fs::create_dir_all(&self.dir).map_err(|e| Error::io("creating approvals folder", e))?;
        let approval = Approval {
            id: uuid::Uuid::new_v4().simple().to_string(),
            project: project.to_string(),
            action: action.to_string(),
            detail: detail.to_string(),
            created_at: now_unix(),
            status: Status::Pending,
        };
        self.write(&approval)?;
        Ok(approval)
    }

    pub fn get(&self, id: &str) -> Result<Approval> {
        let path = self.path(id)?;
        let text = fs::read_to_string(&path)
            .map_err(|_| Error::Approval(format!("no approval request `{id}`")))?;
        serde_json::from_str(&text).map_err(|e| Error::json("reading approval request", e))
    }

    /// Requests still waiting for an answer, oldest first. Stale ones are marked expired.
    pub fn pending(&self) -> Result<Vec<Approval>> {
        let Ok(entries) = fs::read_dir(&self.dir) else { return Ok(Vec::new()) };
        let now = now_unix();
        let mut out = Vec::new();
        for entry in entries.flatten() {
            let Ok(text) = fs::read_to_string(entry.path()) else { continue };
            let Ok(mut approval) = serde_json::from_str::<Approval>(&text) else { continue };
            if approval.status != Status::Pending {
                continue;
            }
            if now - approval.created_at >= APPROVAL_TIMEOUT.as_secs() as i64 {
                approval.status = Status::Expired;
                self.write(&approval)?;
                continue;
            }
            out.push(approval);
        }
        out.sort_by_key(|a| a.created_at);
        Ok(out)
    }

    /// Record the user's answer. Only a pending request can be answered.
    pub fn respond(&self, id: &str, approve: bool) -> Result<Approval> {
        let mut approval = self.get(id)?;
        if approval.status != Status::Pending {
            return Err(Error::Approval(format!("request `{id}` was already answered")));
        }
        approval.status = if approve { Status::Approved } else { Status::Denied };
        self.write(&approval)?;
        Ok(approval)
    }

    /// Block until the request is answered or `timeout` passes. Returns true only for approved.
    pub fn wait(&self, id: &str, timeout: Duration) -> Result<bool> {
        let start = Instant::now();
        loop {
            let approval = self.get(id)?;
            match approval.status {
                Status::Approved => return Ok(true),
                Status::Denied | Status::Expired => return Ok(false),
                Status::Pending if start.elapsed() >= timeout => {
                    let mut expired = approval;
                    expired.status = Status::Expired;
                    self.write(&expired)?;
                    return Ok(false);
                }
                Status::Pending => std::thread::sleep(Duration::from_millis(250)),
            }
        }
    }

    fn path(&self, id: &str) -> Result<PathBuf> {
        if id.is_empty() || !id.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(Error::Approval(format!("`{id}` is not an approval id")));
        }
        Ok(self.dir.join(format!("{id}.json")))
    }

    fn write(&self, approval: &Approval) -> Result<()> {
        let json = serde_json::to_string_pretty(approval)
            .map_err(|e| Error::json("writing approval request", e))?;
        fs::write(self.path(&approval.id)?, json).map_err(|e| Error::io("writing approval request", e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approve_then_wait_returns_true() {
        let ws = tempfile::tempdir().unwrap();
        let queue = Approvals::new(ws.path());
        let req = queue.request("Report", "delete_file", "delete chapters/old.tex").unwrap();
        assert_eq!(queue.pending().unwrap().len(), 1);
        queue.respond(&req.id, true).unwrap();
        assert!(queue.wait(&req.id, Duration::from_secs(1)).unwrap());
        assert!(queue.pending().unwrap().is_empty());
    }

    #[test]
    fn denied_and_timed_out_requests_are_false() {
        let ws = tempfile::tempdir().unwrap();
        let queue = Approvals::new(ws.path());
        let denied = queue.request("Report", "delete_file", "x").unwrap();
        queue.respond(&denied.id, false).unwrap();
        assert!(!queue.wait(&denied.id, Duration::from_secs(1)).unwrap());

        let silent = queue.request("Report", "delete_file", "y").unwrap();
        assert!(!queue.wait(&silent.id, Duration::from_millis(300)).unwrap());
        assert_eq!(queue.get(&silent.id).unwrap().status, Status::Expired);
    }

    #[test]
    fn answers_are_final_and_ids_are_checked() {
        let ws = tempfile::tempdir().unwrap();
        let queue = Approvals::new(ws.path());
        let req = queue.request("Report", "delete_file", "x").unwrap();
        queue.respond(&req.id, false).unwrap();
        assert!(queue.respond(&req.id, true).is_err());
        assert!(queue.get("../evil").is_err());
    }
}
