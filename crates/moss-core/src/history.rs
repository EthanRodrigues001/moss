//! History: versions of a Project, so every Agent write, Sync apply and restore can be undone and
//! redone, and so the History tab can show a git-style log with diffs.
//!
//! Layout inside a Project: `.moss/history/index.json` (the timeline) and
//! `.moss/history/blobs/<sha256>` (file contents, shared between versions). A version is the
//! list of non-ignored files at one moment, plus who made it, why, and how many lines it changed
//! against the version before it. Nothing is ever rewritten, so a restore is itself undoable: it
//! captures the current state first.
//!
//! The timeline is linear. `cursor` marks the version that matches the working tree right now.
//! Undo moves it back, redo moves it forward, and any new capture while the cursor is not at the
//! end appends after the end (older versions stay reachable with `restore`).
//!
//! Versions are the state *after* a change. Callers save "Your changes" before an Agent or
//! file-tree change (a no-op when nothing was typed since the last version) and one version
//! describing the change after it.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::fsutil::{list_files, safe_join};
use crate::project::now_unix;
use crate::textdiff::{LineStats, MAX_DIFF_BYTES, line_stats};
use crate::{Error, Result};

/// Oldest versions are dropped (and their unshared blobs deleted) beyond this many.
const MAX_SNAPSHOTS: usize = 200;
const LOCK_WAIT: Duration = Duration::from_secs(5);
const STALE_LOCK: Duration = Duration::from_secs(30);

/// Why a version exists. Drives the badge and wording in the History tab.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VersionKind {
    /// Taken automatically to keep the user's own edits safe.
    #[default]
    Auto,
    /// Saved by the user with a message.
    Manual,
    Agent,
    Pull,
    Push,
    Merge,
    Restore,
    /// Files changed by something other than Moss's own tools since an Agent last looked.
    External,
}

pub const AUTHOR_YOU: &str = "You";
pub const AUTHOR_AGENT: &str = "Agent";
pub const AUTHOR_OVERLEAF: &str = "Overleaf";
pub const AUTHOR_EXTERNAL: &str = "Outside Moss";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stats {
    pub files: usize,
    pub added: usize,
    pub removed: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Snapshot {
    id: u64,
    /// The version's subject line.
    label: String,
    created_at: i64,
    #[serde(default)]
    kind: VersionKind,
    #[serde(default)]
    author: String,
    #[serde(default)]
    sha: String,
    #[serde(default)]
    stats: Option<Stats>,
    /// Project-relative path to blob hash.
    files: BTreeMap<String, String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Index {
    next_id: u64,
    cursor: Option<usize>,
    snapshots: Vec<Snapshot>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SnapshotInfo {
    pub id: u64,
    /// Seven hex characters, shown in the log.
    pub sha: String,
    pub label: String,
    pub kind: VersionKind,
    pub author: String,
    pub created_at: i64,
    pub file_count: usize,
    /// What this version changed against the one before it.
    pub stats: Stats,
    /// True for the version that matches the working tree.
    pub current: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ChangeKind {
    Added,
    Modified,
    Deleted,
}

#[derive(Debug, Clone, Serialize)]
pub struct Change {
    pub path: String,
    pub kind: ChangeKind,
}

/// One side of a comparison.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Rev {
    /// No files at all (the parent of the first version).
    Empty,
    Snapshot { id: u64 },
    /// The files on disk right now.
    Working,
}

#[derive(Debug, Clone, Serialize)]
pub struct FileChange {
    pub path: String,
    pub kind: ChangeKind,
    pub binary: bool,
    pub added: usize,
    pub removed: usize,
}

/// A file's content at one revision, for the diff viewer.
#[derive(Debug, Clone, Serialize)]
pub struct FileView {
    pub exists: bool,
    pub binary: bool,
    pub size: usize,
    /// Set when the file is text of a size the diff viewer can take.
    pub text: Option<String>,
    pub too_large: bool,
}

pub struct History {
    project_dir: PathBuf,
    dir: PathBuf,
}

impl History {
    pub fn new(project_dir: &Path) -> History {
        History {
            project_dir: project_dir.to_path_buf(),
            dir: project_dir.join(".moss").join("history"),
        }
    }

    /// Take a restore point of the working tree unless it already matches the cursor version.
    /// Returns the new version, or `None` when nothing changed.
    pub fn capture(&self, label: &str) -> Result<Option<SnapshotInfo>> {
        self.capture_as(VersionKind::Auto, AUTHOR_YOU, label)
    }

    pub fn capture_as(
        &self,
        kind: VersionKind,
        author: &str,
        label: &str,
    ) -> Result<Option<SnapshotInfo>> {
        let _lock = self.lock()?;
        let mut index = self.load()?;
        let tree = self.working_tree()?;
        if self.matches_cursor(&index, &tree) {
            return Ok(None);
        }
        let info = self.append(&mut index, kind, author, label, tree);
        self.save(&index)?;
        Ok(Some(info))
    }

    /// The user's "Save version". When nothing changed since the latest automatic version, that
    /// version is given the user's message; after an Agent or Sync version the name goes on a
    /// new version with no changes. `None` means this state is already a saved version.
    pub fn save_version(&self, message: &str) -> Result<Option<SnapshotInfo>> {
        let message = message.trim();
        if message.is_empty() {
            return Err(Error::History("a version needs a message".into()));
        }
        let _lock = self.lock()?;
        let mut index = self.load()?;
        let tree = self.working_tree()?;
        if self.matches_cursor(&index, &tree) {
            let Some(c) = index.cursor else { return Ok(None) };
            let snap = &mut index.snapshots[c];
            match snap.kind {
                VersionKind::Manual => return Ok(None),
                VersionKind::Auto => {
                    snap.kind = VersionKind::Manual;
                    snap.label = message.to_string();
                    snap.author = AUTHOR_YOU.to_string();
                    let info = info_of(snap, true);
                    self.save(&index)?;
                    return Ok(Some(info));
                }
                // An Agent, Pull or Push version keeps its own record; the user's name goes on a
                // new, empty version beside it.
                _ => {}
            }
        }
        let info = self.append(&mut index, VersionKind::Manual, AUTHOR_YOU, message, tree);
        self.save(&index)?;
        Ok(Some(info))
    }

    /// Record an event that changed no local file (a Push) as its own version, so the log shows
    /// it. `stats` says what was sent.
    pub fn mark(
        &self,
        kind: VersionKind,
        author: &str,
        label: &str,
        stats: Stats,
    ) -> Result<SnapshotInfo> {
        let _lock = self.lock()?;
        let mut index = self.load()?;
        let tree = self.working_tree()?;
        let mut info = self.append(&mut index, kind, author, label, tree);
        if let Some(last) = index.snapshots.last_mut() {
            last.stats = Some(stats);
            info.stats = stats;
        }
        self.save(&index)?;
        Ok(info)
    }

    /// Oldest first. Fills in line counts that older indexes lack.
    pub fn list(&self) -> Result<Vec<SnapshotInfo>> {
        let mut index = self.load()?;
        if index.snapshots.iter().any(|s| s.stats.is_none() || s.sha.is_empty()) {
            let _lock = self.lock()?;
            index = self.load()?;
            self.backfill(&mut index);
            self.save(&index)?;
        }
        Ok(index
            .snapshots
            .iter()
            .enumerate()
            .map(|(i, s)| info_of(s, index.cursor == Some(i)))
            .collect())
    }

    /// Step back to the state before the latest change. Captures the current state first so
    /// `redo` can return to it. `None` means there is nothing earlier.
    pub fn undo(&self) -> Result<Option<SnapshotInfo>> {
        let _lock = self.lock()?;
        let mut index = self.load()?;
        self.capture_into(&mut index, "Before undo")?;
        let Some(target) = self.step(&index, false) else {
            self.save(&index)?;
            return Ok(None);
        };
        self.restore_index(&mut index, target).map(Some)
    }

    /// Step forward again after an undo. `None` means there is nothing later.
    pub fn redo(&self) -> Result<Option<SnapshotInfo>> {
        let _lock = self.lock()?;
        let mut index = self.load()?;
        self.capture_into(&mut index, "Before redo")?;
        let Some(target) = self.step(&index, true) else {
            self.save(&index)?;
            return Ok(None);
        };
        self.restore_index(&mut index, target).map(Some)
    }

    /// Put the working tree back to version `id`. The current state is captured first.
    pub fn restore(&self, id: u64) -> Result<SnapshotInfo> {
        let _lock = self.lock()?;
        let mut index = self.load()?;
        let target = position(&index, id)?;
        self.capture_into(&mut index, "Before restore")?;
        // Append the restored state as a new entry so one undo steps back over the restore.
        let files = index.snapshots[target].files.clone();
        let label = format!("Restored version {}", short(&index.snapshots[target]));
        self.append(&mut index, VersionKind::Restore, AUTHOR_YOU, &label, files);
        let last = index.snapshots.len() - 1;
        self.restore_index(&mut index, last)
    }

    /// Put one file back to how it was in version `id` (or delete it if it did not exist then).
    pub fn restore_file(&self, id: u64, path: &str) -> Result<SnapshotInfo> {
        let file = safe_join(&self.project_dir, path)?;
        let _lock = self.lock()?;
        let mut index = self.load()?;
        let target = position(&index, id)?;
        self.capture_into(&mut index, "Before restore")?;
        match index.snapshots[target].files.get(path).cloned() {
            Some(hash) => {
                if let Some(parent) = file.parent() {
                    fs::create_dir_all(parent)
                        .map_err(|e| Error::io(format!("creating folder for {path}"), e))?;
                }
                let bytes = fs::read(self.blob_path(&hash))
                    .map_err(|e| Error::io(format!("reading stored copy of {path}"), e))?;
                fs::write(&file, bytes).map_err(|e| Error::io(format!("restoring {path}"), e))?;
            }
            None => {
                if file.exists() {
                    fs::remove_file(&file)
                        .map_err(|e| Error::io(format!("removing {path}"), e))?;
                    remove_empty_parents(&file, &self.project_dir);
                }
            }
        }
        let tree = self.working_tree()?;
        let label = format!("Restored {path} from {}", short(&index.snapshots[target]));
        let info = self.append(&mut index, VersionKind::Restore, AUTHOR_YOU, &label, tree);
        self.save(&index)?;
        Ok(info)
    }

    /// What differs between version `id` and the working tree.
    pub fn changes_since(&self, id: u64) -> Result<Vec<Change>> {
        Ok(self
            .compare(Rev::Snapshot { id }, Rev::Working)?
            .into_iter()
            .map(|c| Change { path: c.path, kind: c.kind })
            .collect())
    }

    /// The files that differ between two revisions, with line counts.
    pub fn compare(&self, from: Rev, to: Rev) -> Result<Vec<FileChange>> {
        let index = self.load()?;
        let a = self.tree_of(&index, from)?;
        let b = self.tree_of(&index, to)?;
        let mut out = Vec::new();
        for (path, new_hash) in &b {
            match a.get(path) {
                None => out.push(self.file_change(path, ChangeKind::Added, from, to, &index)?),
                Some(old) if old != new_hash => {
                    out.push(self.file_change(path, ChangeKind::Modified, from, to, &index)?);
                }
                Some(_) => {}
            }
        }
        for path in a.keys().filter(|p| !b.contains_key(*p)) {
            out.push(self.file_change(path, ChangeKind::Deleted, from, to, &index)?);
        }
        out.sort_by(|x, y| x.path.cmp(&y.path));
        Ok(out)
    }

    /// What version `id` changed: a comparison with the version before it.
    pub fn compare_with_parent(&self, id: u64) -> Result<Vec<FileChange>> {
        let index = self.load()?;
        let pos = position(&index, id)?;
        let from = match pos.checked_sub(1) {
            Some(p) => Rev::Snapshot { id: index.snapshots[p].id },
            None => Rev::Empty,
        };
        self.compare(from, Rev::Snapshot { id })
    }

    /// The revision before version `id` (for the diff viewer's left side).
    pub fn parent_of(&self, id: u64) -> Result<Rev> {
        let index = self.load()?;
        let pos = position(&index, id)?;
        Ok(match pos.checked_sub(1) {
            Some(p) => Rev::Snapshot { id: index.snapshots[p].id },
            None => Rev::Empty,
        })
    }

    /// The version that matches the working tree, if any (the left side of the Working copy row).
    pub fn cursor_rev(&self) -> Result<Rev> {
        let index = self.load()?;
        Ok(match index.cursor.and_then(|c| index.snapshots.get(c)) {
            Some(s) => Rev::Snapshot { id: s.id },
            None => Rev::Empty,
        })
    }

    /// One file at one revision.
    pub fn file_at(&self, rev: Rev, path: &str) -> Result<FileView> {
        let index = self.load()?;
        let bytes = self.read_rev(&index, rev, path)?;
        Ok(view_of(bytes))
    }

    fn file_change(
        &self,
        path: &str,
        kind: ChangeKind,
        from: Rev,
        to: Rev,
        index: &Index,
    ) -> Result<FileChange> {
        let old = self.read_rev(index, from, path)?;
        let new = self.read_rev(index, to, path)?;
        let (binary, stats) = stats_of(old.as_deref(), new.as_deref());
        Ok(FileChange {
            path: path.to_string(),
            kind,
            binary,
            added: stats.added,
            removed: stats.removed,
        })
    }

    fn read_rev(&self, index: &Index, rev: Rev, path: &str) -> Result<Option<Vec<u8>>> {
        match rev {
            Rev::Empty => Ok(None),
            Rev::Working => {
                let file = safe_join(&self.project_dir, path)?;
                match fs::read(&file) {
                    Ok(b) => Ok(Some(b)),
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
                    Err(e) => Err(Error::io(format!("reading {path}"), e)),
                }
            }
            Rev::Snapshot { id } => {
                let snap = &index.snapshots[position(index, id)?];
                match snap.files.get(path) {
                    None => Ok(None),
                    Some(hash) => fs::read(self.blob_path(hash))
                        .map(Some)
                        .map_err(|e| Error::io(format!("reading stored copy of {path}"), e)),
                }
            }
        }
    }

    /// Path to hash for a revision. The working tree is hashed without storing anything.
    fn tree_of(&self, index: &Index, rev: Rev) -> Result<BTreeMap<String, String>> {
        match rev {
            Rev::Empty => Ok(BTreeMap::new()),
            Rev::Snapshot { id } => Ok(index.snapshots[position(index, id)?].files.clone()),
            Rev::Working => {
                let mut tree = BTreeMap::new();
                for entry in list_files(&self.project_dir)? {
                    if entry.is_dir {
                        continue;
                    }
                    let bytes = fs::read(self.project_dir.join(&entry.path))
                        .map_err(|e| Error::io(format!("reading {}", entry.path), e))?;
                    tree.insert(entry.path, hex(&Sha256::digest(&bytes)));
                }
                Ok(tree)
            }
        }
    }

    /// The nearest version in one direction whose files differ from the current ones. Versions
    /// that only mark an event (a Push) hold the same files, so one Undo must not stop on them.
    fn step(&self, index: &Index, forward: bool) -> Option<usize> {
        let cursor = index.cursor?;
        let here = &index.snapshots.get(cursor)?.files;
        let differs = |i: &usize| &index.snapshots[*i].files != here;
        if forward {
            (cursor + 1..index.snapshots.len()).find(differs)
        } else {
            (0..cursor).rev().find(differs)
        }
    }

    fn capture_into(&self, index: &mut Index, label: &str) -> Result<()> {
        let tree = self.working_tree()?;
        if !self.matches_cursor(index, &tree) {
            self.append(index, VersionKind::Auto, AUTHOR_YOU, label, tree);
        }
        Ok(())
    }

    fn matches_cursor(&self, index: &Index, tree: &BTreeMap<String, String>) -> bool {
        index
            .cursor
            .and_then(|c| index.snapshots.get(c))
            .is_some_and(|s| &s.files == tree)
    }

    fn append(
        &self,
        index: &mut Index,
        kind: VersionKind,
        author: &str,
        label: &str,
        files: BTreeMap<String, String>,
    ) -> SnapshotInfo {
        let parent = index.snapshots.last();
        let stats = self.stats_between(parent.map(|p| &p.files), &files);
        let sha = sha_of(parent.map(|p| p.sha.as_str()), &files);
        let snapshot = Snapshot {
            id: index.next_id,
            label: label.to_string(),
            created_at: now_unix(),
            kind,
            author: author.to_string(),
            sha,
            stats: Some(stats),
            files,
        };
        index.next_id += 1;
        index.snapshots.push(snapshot);
        index.cursor = Some(index.snapshots.len() - 1);
        if index.snapshots.len() > MAX_SNAPSHOTS {
            let drop = index.snapshots.len() - MAX_SNAPSHOTS;
            index.snapshots.drain(..drop);
            index.cursor = index.cursor.map(|c| c.saturating_sub(drop));
            self.collect_garbage(index);
        }
        let last = index.snapshots.len() - 1;
        info_of(&index.snapshots[last], true)
    }

    /// Line counts between two file sets, reading the stored copies.
    fn stats_between(
        &self,
        old: Option<&BTreeMap<String, String>>,
        new: &BTreeMap<String, String>,
    ) -> Stats {
        let empty = BTreeMap::new();
        let old = old.unwrap_or(&empty);
        let read = |hash: Option<&String>| hash.and_then(|h| fs::read(self.blob_path(h)).ok());
        let mut stats = Stats::default();
        let paths: BTreeSet<&String> = old.keys().chain(new.keys()).collect();
        for path in paths {
            let (o, n) = (old.get(path), new.get(path));
            if o == n {
                continue;
            }
            stats.files += 1;
            let (_, lines) = stats_of(read(o).as_deref(), read(n).as_deref());
            stats.added += lines.added;
            stats.removed += lines.removed;
        }
        stats
    }

    fn backfill(&self, index: &mut Index) {
        let mut prev: Option<(String, BTreeMap<String, String>)> = None;
        for snap in &mut index.snapshots {
            if snap.sha.is_empty() {
                snap.sha = sha_of(prev.as_ref().map(|p| p.0.as_str()), &snap.files);
            }
            if snap.stats.is_none() {
                snap.stats = Some(self.stats_between(prev.as_ref().map(|p| &p.1), &snap.files));
            }
            prev = Some((snap.sha.clone(), snap.files.clone()));
        }
    }

    fn restore_index(&self, index: &mut Index, target: usize) -> Result<SnapshotInfo> {
        let snap = index.snapshots[target].clone();
        let current = self.working_tree()?;
        for path in current.keys().filter(|p| !snap.files.contains_key(*p)) {
            let file = safe_join(&self.project_dir, path)?;
            fs::remove_file(&file).map_err(|e| Error::io(format!("removing {path}"), e))?;
            remove_empty_parents(&file, &self.project_dir);
        }
        for (path, hash) in &snap.files {
            if current.get(path) == Some(hash) {
                continue;
            }
            let file = safe_join(&self.project_dir, path)?;
            if let Some(parent) = file.parent() {
                fs::create_dir_all(parent)
                    .map_err(|e| Error::io(format!("creating folder for {path}"), e))?;
            }
            let bytes = fs::read(self.blob_path(hash))
                .map_err(|e| Error::io(format!("reading stored copy of {path}"), e))?;
            fs::write(&file, bytes).map_err(|e| Error::io(format!("restoring {path}"), e))?;
        }
        index.cursor = Some(target);
        self.save(index)?;
        Ok(info_of(&snap, true))
    }

    /// Hash every non-ignored file and store its bytes as a blob if not already stored.
    fn working_tree(&self) -> Result<BTreeMap<String, String>> {
        let mut tree = BTreeMap::new();
        for entry in list_files(&self.project_dir)? {
            if entry.is_dir {
                continue;
            }
            let bytes = fs::read(self.project_dir.join(&entry.path))
                .map_err(|e| Error::io(format!("reading {}", entry.path), e))?;
            let hash = hex(&Sha256::digest(&bytes));
            let blob = self.blob_path(&hash);
            if !blob.exists() {
                if let Some(parent) = blob.parent() {
                    fs::create_dir_all(parent)
                        .map_err(|e| Error::io("creating history folder", e))?;
                }
                fs::write(&blob, &bytes).map_err(|e| Error::io("storing history copy", e))?;
            }
            tree.insert(entry.path, hash);
        }
        Ok(tree)
    }

    fn collect_garbage(&self, index: &Index) {
        let keep: BTreeSet<&String> = index.snapshots.iter().flat_map(|s| s.files.values()).collect();
        let Ok(entries) = fs::read_dir(self.dir.join("blobs")) else { return };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !keep.contains(&name) {
                let _ = fs::remove_file(entry.path());
            }
        }
    }

    fn blob_path(&self, hash: &str) -> PathBuf {
        self.dir.join("blobs").join(hash)
    }

    fn load(&self) -> Result<Index> {
        match fs::read_to_string(self.dir.join("index.json")) {
            Ok(text) => serde_json::from_str(&text)
                .map_err(|e| Error::json("reading history index", e)),
            Err(_) => Ok(Index::default()),
        }
    }

    fn save(&self, index: &Index) -> Result<()> {
        fs::create_dir_all(&self.dir).map_err(|e| Error::io("creating history folder", e))?;
        let json = serde_json::to_string(index).map_err(|e| Error::json("writing history index", e))?;
        let tmp = self.dir.join("index.json.tmp");
        fs::write(&tmp, json).map_err(|e| Error::io("writing history index", e))?;
        fs::rename(&tmp, self.dir.join("index.json"))
            .map_err(|e| Error::io("replacing history index", e))
    }

    /// The app and `moss-mcp` are separate processes that can both write history. A lock file
    /// keeps their read-modify-write cycles from interleaving; a stale one is broken after 30 s.
    fn lock(&self) -> Result<Lock> {
        fs::create_dir_all(&self.dir).map_err(|e| Error::io("creating history folder", e))?;
        let path = self.dir.join("index.lock");
        let started = Instant::now();
        loop {
            match fs::OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(_) => return Ok(Lock { path }),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    let stale = fs::metadata(&path)
                        .and_then(|m| m.modified())
                        .ok()
                        .and_then(|t| t.elapsed().ok())
                        .is_some_and(|age| age > STALE_LOCK);
                    if stale {
                        let _ = fs::remove_file(&path);
                        continue;
                    }
                    if started.elapsed() > LOCK_WAIT {
                        return Err(Error::History(
                            "history is busy (another Moss window or agent is writing it)".into(),
                        ));
                    }
                    std::thread::sleep(Duration::from_millis(25));
                }
                Err(e) => return Err(Error::io("locking history", e)),
            }
        }
    }
}

struct Lock {
    path: PathBuf,
}

impl Drop for Lock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn position(index: &Index, id: u64) -> Result<usize> {
    index
        .snapshots
        .iter()
        .position(|s| s.id == id)
        .ok_or_else(|| Error::History(format!("no version with id {id}")))
}

fn info_of(s: &Snapshot, current: bool) -> SnapshotInfo {
    SnapshotInfo {
        id: s.id,
        sha: s.sha.clone(),
        label: s.label.clone(),
        kind: s.kind,
        author: if s.author.is_empty() { AUTHOR_YOU.to_string() } else { s.author.clone() },
        created_at: s.created_at,
        file_count: s.files.len(),
        stats: s.stats.unwrap_or_default(),
        current,
    }
}

fn short(s: &Snapshot) -> String {
    if s.sha.is_empty() { format!("#{}", s.id) } else { s.sha.clone() }
}

/// Seven hex characters naming a version: its parent and its exact file contents.
fn sha_of(parent: Option<&str>, files: &BTreeMap<String, String>) -> String {
    let mut h = Sha256::new();
    h.update(b"moss-version\n");
    h.update(parent.unwrap_or("").as_bytes());
    h.update(b"\n");
    for (path, hash) in files {
        h.update(path.as_bytes());
        h.update(b"\t");
        h.update(hash.as_bytes());
        h.update(b"\n");
    }
    hex(&h.finalize())[..7].to_string()
}

/// Text means valid UTF-8 with no NUL bytes.
fn as_text(bytes: &[u8]) -> Option<&str> {
    if bytes.contains(&0) {
        return None;
    }
    std::str::from_utf8(bytes).ok()
}

/// Whether the change is to a binary file, and the line counts for text.
fn stats_of(old: Option<&[u8]>, new: Option<&[u8]>) -> (bool, LineStats) {
    let old_text = old.map(as_text);
    let new_text = new.map(as_text);
    if matches!(old_text, Some(None)) || matches!(new_text, Some(None)) {
        return (true, LineStats::default());
    }
    let old = old_text.flatten().unwrap_or("");
    let new = new_text.flatten().unwrap_or("");
    (false, line_stats(old, new))
}

/// How the diff viewer sees a file's bytes (`None` means the file does not exist).
pub fn view_of(bytes: Option<Vec<u8>>) -> FileView {
    let Some(bytes) = bytes else {
        return FileView { exists: false, binary: false, size: 0, text: None, too_large: false };
    };
    let size = bytes.len();
    match as_text(&bytes) {
        None => FileView { exists: true, binary: true, size, text: None, too_large: false },
        Some(_) if size > MAX_DIFF_BYTES => {
            FileView { exists: true, binary: false, size, text: None, too_large: true }
        }
        Some(text) => FileView {
            exists: true,
            binary: false,
            size,
            text: Some(text.to_string()),
            too_large: false,
        },
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fsutil::write_text;

    fn project() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        write_text(dir.path(), "main.tex", "one").unwrap();
        dir
    }

    fn text(dir: &Path, rel: &str) -> Option<String> {
        fs::read_to_string(dir.join(rel)).ok()
    }

    #[test]
    fn capture_skips_when_nothing_changed() {
        let dir = project();
        let h = History::new(dir.path());
        assert!(h.capture("first").unwrap().is_some());
        assert!(h.capture("again").unwrap().is_none());
        write_text(dir.path(), "main.tex", "two").unwrap();
        assert!(h.capture("edit").unwrap().is_some());
        assert_eq!(h.list().unwrap().len(), 2);
    }

    #[test]
    fn undo_and_redo_walk_the_timeline() {
        let dir = project();
        let h = History::new(dir.path());
        h.capture("before agent write").unwrap();
        write_text(dir.path(), "main.tex", "two").unwrap();
        write_text(dir.path(), "extra/new.tex", "x").unwrap();

        h.undo().unwrap().expect("something to undo");
        assert_eq!(text(dir.path(), "main.tex").as_deref(), Some("one"));
        assert!(text(dir.path(), "extra/new.tex").is_none());
        assert!(!dir.path().join("extra").exists(), "empty folder removed");

        h.redo().unwrap().expect("something to redo");
        assert_eq!(text(dir.path(), "main.tex").as_deref(), Some("two"));
        assert_eq!(text(dir.path(), "extra/new.tex").as_deref(), Some("x"));
        assert!(h.redo().unwrap().is_none());
    }

    #[test]
    fn undo_with_nothing_earlier_is_none() {
        let dir = project();
        let h = History::new(dir.path());
        assert!(h.undo().unwrap().is_none());
        assert_eq!(text(dir.path(), "main.tex").as_deref(), Some("one"));
    }

    #[test]
    fn a_restore_can_itself_be_undone() {
        let dir = project();
        let h = History::new(dir.path());
        let first = h.capture("v1").unwrap().unwrap();
        write_text(dir.path(), "main.tex", "two").unwrap();
        h.restore(first.id).unwrap();
        assert_eq!(text(dir.path(), "main.tex").as_deref(), Some("one"));
        h.undo().unwrap();
        assert_eq!(text(dir.path(), "main.tex").as_deref(), Some("two"));
    }

    #[test]
    fn deleted_files_come_back() {
        let dir = project();
        let h = History::new(dir.path());
        h.capture("before delete").unwrap();
        fs::remove_file(dir.path().join("main.tex")).unwrap();
        h.undo().unwrap();
        assert_eq!(text(dir.path(), "main.tex").as_deref(), Some("one"));
    }

    #[test]
    fn changes_since_lists_added_modified_deleted() {
        let dir = project();
        write_text(dir.path(), "gone.tex", "g").unwrap();
        let h = History::new(dir.path());
        let snap = h.capture("base").unwrap().unwrap();
        write_text(dir.path(), "main.tex", "two").unwrap();
        write_text(dir.path(), "new.tex", "n").unwrap();
        fs::remove_file(dir.path().join("gone.tex")).unwrap();
        let mut changes = h.changes_since(snap.id).unwrap();
        changes.sort_by(|a, b| a.path.cmp(&b.path));
        let summary: Vec<_> = changes.iter().map(|c| (c.path.as_str(), c.kind)).collect();
        assert_eq!(
            summary,
            vec![
                ("gone.tex", ChangeKind::Deleted),
                ("main.tex", ChangeKind::Modified),
                ("new.tex", ChangeKind::Added),
            ]
        );
    }

    #[test]
    fn unknown_version_is_an_error() {
        let dir = project();
        assert!(History::new(dir.path()).restore(99).is_err());
    }

    #[test]
    fn versions_know_their_author_kind_sha_and_line_counts() {
        let dir = project();
        write_text(dir.path(), "main.tex", "a\nb\nc\n").unwrap();
        let h = History::new(dir.path());
        let first = h.capture_as(VersionKind::Auto, AUTHOR_YOU, "Your changes").unwrap().unwrap();
        assert_eq!(first.stats, Stats { files: 1, added: 3, removed: 0 });
        assert_eq!(first.sha.len(), 7);

        write_text(dir.path(), "main.tex", "a\nB\nc\nd\n").unwrap();
        write_text(dir.path(), "new.bib", "@book{}\n").unwrap();
        let second = h.capture_as(VersionKind::Agent, AUTHOR_AGENT, "write main.tex").unwrap().unwrap();
        assert_eq!(second.stats, Stats { files: 2, added: 3, removed: 1 });
        assert_eq!((second.kind, second.author.as_str()), (VersionKind::Agent, "Agent"));
        assert_ne!(first.sha, second.sha);
        assert!(h.list().unwrap().last().unwrap().current);
    }

    #[test]
    fn save_version_names_the_current_state_or_adds_one() {
        let dir = project();
        let h = History::new(dir.path());
        assert!(h.save_version("   ").is_err(), "a message is required");

        let saved = h.save_version("First draft").unwrap().unwrap();
        assert_eq!((saved.kind, saved.label.as_str()), (VersionKind::Manual, "First draft"));
        assert!(h.save_version("Again").unwrap().is_none(), "nothing new and already named");

        write_text(dir.path(), "main.tex", "two").unwrap();
        let auto = h.capture("Your changes").unwrap().unwrap();
        let renamed = h.save_version("Second draft").unwrap().unwrap();
        assert_eq!(renamed.id, auto.id, "the unnamed version took the message");
        assert_eq!(renamed.kind, VersionKind::Manual);
        assert_eq!(h.list().unwrap().len(), 2);
    }

    #[test]
    fn compare_reports_files_with_line_counts_and_flags_binaries() {
        let dir = project();
        fs::write(dir.path().join("pic.png"), [0u8, 1, 2, 3]).unwrap();
        let h = History::new(dir.path());
        let v1 = h.capture("v1").unwrap().unwrap();
        write_text(dir.path(), "main.tex", "one\ntwo\n").unwrap();
        fs::write(dir.path().join("pic.png"), [0u8, 9, 9]).unwrap();
        write_text(dir.path(), "added.tex", "x\n").unwrap();
        let v2 = h.capture("v2").unwrap().unwrap();

        let changes = h.compare_with_parent(v2.id).unwrap();
        let by_path: BTreeMap<_, _> = changes.iter().map(|c| (c.path.as_str(), c)).collect();
        assert_eq!(by_path["main.tex"].kind, ChangeKind::Modified);
        assert_eq!((by_path["main.tex"].added, by_path["main.tex"].removed), (2, 1));
        assert!(by_path["pic.png"].binary);
        assert_eq!(by_path["added.tex"].kind, ChangeKind::Added);

        // The first version's parent is empty, so everything is new.
        let first = h.compare_with_parent(v1.id).unwrap();
        assert!(first.iter().all(|c| c.kind == ChangeKind::Added));

        // Unsaved edits show up against the cursor version without being stored.
        write_text(dir.path(), "main.tex", "changed\n").unwrap();
        let working = h.compare(h.cursor_rev().unwrap(), Rev::Working).unwrap();
        assert_eq!(working.len(), 1);
        assert_eq!(working[0].path, "main.tex");
        assert_eq!(h.list().unwrap().len(), 2, "comparing does not add a version");
    }

    #[test]
    fn file_at_returns_text_missing_and_binary_views() {
        let dir = project();
        fs::write(dir.path().join("pic.png"), [0u8, 1]).unwrap();
        let h = History::new(dir.path());
        let v1 = h.capture("v1").unwrap().unwrap();
        write_text(dir.path(), "main.tex", "two").unwrap();

        let old = h.file_at(Rev::Snapshot { id: v1.id }, "main.tex").unwrap();
        assert_eq!(old.text.as_deref(), Some("one"));
        let now = h.file_at(Rev::Working, "main.tex").unwrap();
        assert_eq!(now.text.as_deref(), Some("two"));
        assert!(!h.file_at(Rev::Empty, "main.tex").unwrap().exists);
        assert!(h.file_at(Rev::Working, "pic.png").unwrap().binary);
        assert!(!h.file_at(Rev::Working, "nope.tex").unwrap().exists);
    }

    #[test]
    fn restore_file_changes_only_that_file_and_can_be_undone() {
        let dir = project();
        write_text(dir.path(), "other.tex", "keep").unwrap();
        let h = History::new(dir.path());
        let v1 = h.capture("v1").unwrap().unwrap();
        write_text(dir.path(), "main.tex", "edited").unwrap();
        write_text(dir.path(), "other.tex", "also edited").unwrap();
        write_text(dir.path(), "late.tex", "added later").unwrap();

        let r = h.restore_file(v1.id, "main.tex").unwrap();
        assert_eq!(r.kind, VersionKind::Restore);
        assert_eq!(text(dir.path(), "main.tex").as_deref(), Some("one"));
        assert_eq!(text(dir.path(), "other.tex").as_deref(), Some("also edited"));

        // A file that did not exist then is removed.
        h.restore_file(v1.id, "late.tex").unwrap();
        assert!(text(dir.path(), "late.tex").is_none());

        h.undo().unwrap();
        assert_eq!(text(dir.path(), "late.tex").as_deref(), Some("added later"));
        assert!(h.restore_file(v1.id, "../escape.tex").is_err());
    }

    #[test]
    fn old_indexes_without_the_new_fields_still_load_and_get_filled_in() {
        let dir = project();
        let h = History::new(dir.path());
        h.capture("v1").unwrap();
        // Strip the new fields from the stored index, as an index from an earlier build would be.
        let path = dir.path().join(".moss/history/index.json");
        let mut v: serde_json::Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        for s in v["snapshots"].as_array_mut().unwrap() {
            let o = s.as_object_mut().unwrap();
            for k in ["kind", "author", "sha", "stats"] {
                o.remove(k);
            }
        }
        fs::write(&path, v.to_string()).unwrap();

        let list = h.list().unwrap();
        assert_eq!(list[0].sha.len(), 7);
        assert_eq!(list[0].author, "You");
        assert_eq!(list[0].stats.files, 1);
    }

    #[test]
    fn a_stale_lock_is_broken() {
        let dir = project();
        let h = History::new(dir.path());
        h.capture("v1").unwrap();
        // A leftover lock from a crashed process is old enough to be removed.
        let lock = dir.path().join(".moss/history/index.lock");
        fs::write(&lock, "").unwrap();
        let old = std::time::SystemTime::now() - Duration::from_secs(120);
        fs::File::options().write(true).open(&lock).unwrap().set_modified(old).unwrap();
        write_text(dir.path(), "main.tex", "two").unwrap();
        assert!(h.capture("v2").unwrap().is_some());
        assert!(!lock.exists(), "lock released after the write");
    }

    #[test]
    fn a_marker_shows_in_the_log_but_undo_steps_over_it() {
        let dir = project();
        let h = History::new(dir.path());
        h.capture("v1").unwrap();
        write_text(dir.path(), "main.tex", "two").unwrap();
        h.capture("v2").unwrap();
        let marker = h
            .mark(VersionKind::Push, "You", "Pushed to Overleaf", Stats { files: 1, added: 2, removed: 1 })
            .unwrap();
        assert_eq!(marker.stats.added, 2);
        assert_eq!(h.list().unwrap().len(), 3);

        h.undo().unwrap().expect("undo steps over the marker to v1");
        assert_eq!(text(dir.path(), "main.tex").as_deref(), Some("one"));
        h.redo().unwrap().expect("redo comes back");
        assert_eq!(text(dir.path(), "main.tex").as_deref(), Some("two"));
    }

    #[test]
    fn the_oldest_versions_are_dropped_past_the_cap() {
        let dir = project();
        let h = History::new(dir.path());
        for i in 0..(MAX_SNAPSHOTS + 5) {
            write_text(dir.path(), "main.tex", &format!("v{i}")).unwrap();
            h.capture("edit").unwrap();
        }
        let list = h.list().unwrap();
        assert_eq!(list.len(), MAX_SNAPSHOTS);
        assert!(list.last().unwrap().current);
        assert_eq!(text(dir.path(), "main.tex").unwrap(), format!("v{}", MAX_SNAPSHOTS + 4));
    }
}
