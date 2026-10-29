//! Preview and apply: the part of Sync that touches files and the remote.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::fileset::{Blob, FileSet};
use super::lock::SyncLock;
use super::plan::{
    Conflict, Decision, Entry, FileChoice, Item, Mode, Plan, Resolution, items_of, plan,
};
use super::remote::{Fetched, Remote};
use crate::fsutil::{safe_join, write_bytes};
use crate::history::{
    AUTHOR_OVERLEAF, AUTHOR_YOU, FileView, History, SnapshotInfo, Stats, VersionKind, view_of,
};
use crate::project::{META_DIR, Project, now_unix};
use crate::textdiff::line_stats;
use crate::workspace::Skipped;
use crate::{Error, Result};

/// Deleting this many files, and more than this share of a side, needs an explicit yes.
const MASS_DELETE_MIN: usize = 5;
const MASS_DELETE_PERCENT: usize = 30;
const MOSS_AUTHOR: &str = "Moss";

#[derive(Debug, Clone, Default, Serialize)]
pub struct Summary {
    /// Files Overleaf will write here.
    pub pull: usize,
    /// Files this PC will send to Overleaf.
    pub push: usize,
    pub merge: usize,
    pub conflict: usize,
    /// Files that will be deleted on this PC.
    pub delete_here: usize,
    /// Files that will be deleted on Overleaf.
    pub delete_there: usize,
    /// Files Overleaf changed (what a Pull would bring).
    pub behind: usize,
    /// Files changed on this PC (what a Push would send).
    pub ahead: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct MassDelete {
    /// `"here"` or `"overleaf"`.
    pub side: String,
    pub count: usize,
    pub total: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct Preview {
    /// Hand this back to `apply`; it changes whenever either side does.
    pub token: String,
    pub mode: Mode,
    pub items: Vec<Item>,
    pub conflicts: Vec<Conflict>,
    pub needs_attention: Vec<Skipped>,
    pub warnings: Vec<String>,
    pub mass_delete: Option<MassDelete>,
    /// A reason apply will refuse, such as Overleaf returning an empty project.
    pub blocked: Option<String>,
    pub summary: Summary,
}

/// The three file sets behind a Preview, kept by the caller to answer "show me this file".
pub struct Staged {
    pub base: FileSet,
    pub local: FileSet,
    pub remote: FileSet,
}

impl Staged {
    /// One file on each side, for the diff viewer. `mine` is this PC, `theirs` is Overleaf.
    pub fn views(&self, path: &str) -> StagedFile {
        let view = |set: &FileSet| view_of(set.get(path).map(|b| b.bytes.clone()));
        StagedFile {
            base: view(&self.base),
            mine: view(&self.local),
            theirs: view(&self.remote),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct StagedFile {
    pub base: FileView,
    pub mine: FileView,
    pub theirs: FileView,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ApplyRequest {
    pub mode: Mode,
    pub token: String,
    #[serde(default)]
    pub resolutions: Vec<Resolution>,
    #[serde(default)]
    pub allow_mass_delete: bool,
    /// Resolved text that still contains conflict markers is refused unless this is set.
    #[serde(default)]
    pub allow_markers: bool,
    /// Paths the user unticked in the Review; they are left alone this time.
    #[serde(default)]
    pub skip: Vec<String>,
    /// Who is pressing the button, for the History log (`You` or `Agent`).
    #[serde(default = "default_actor")]
    pub actor: String,
}

fn default_actor() -> String {
    AUTHOR_YOU.to_string()
}

/// Which side moved since the Review was made.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum StaleSide {
    Overleaf,
    Here,
}

#[derive(Debug, Clone, Serialize)]
pub struct Failure {
    pub path: String,
    pub error: String,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Outcome {
    pub pulled: usize,
    pub pushed: usize,
    pub merged: usize,
    pub resolved: usize,
    pub failures: Vec<Failure>,
    /// Set when the run stopped early, such as an expired Overleaf session.
    pub aborted: Option<String>,
    /// Files left alone because Overleaf changed them too (Push) or the user unticked them.
    pub left_alone: usize,
    /// True when every file was settled and the last-synced time moved.
    pub complete: bool,
    pub versions: Vec<SnapshotInfo>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "result", rename_all = "lowercase")]
pub enum Applied {
    /// Either side changed after the Review; nothing was written. Review again.
    Stale { side: StaleSide },
    Done { outcome: Outcome },
}

#[derive(Debug, Clone, Serialize)]
pub struct LocalChange {
    pub path: String,
    pub kind: crate::history::ChangeKind,
}

/// Files that differ from the Base snapshot, no network needed.
pub fn local_changes(project: &Project) -> Result<Vec<LocalChange>> {
    use crate::history::ChangeKind::{Added, Deleted, Modified};
    let base = FileSet::from_base(project.dir())?;
    let local = FileSet::from_project_dir(project.dir())?;
    let mut out = Vec::new();
    for (path, blob) in &local.files {
        match base.get(path) {
            None => out.push(LocalChange { path: path.clone(), kind: Added }),
            Some(b) if b.hash != blob.hash => out.push(LocalChange { path: path.clone(), kind: Modified }),
            Some(_) => {}
        }
    }
    for path in base.files.keys().filter(|p| local.get(p).is_none()) {
        out.push(LocalChange { path: path.clone(), kind: Deleted });
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

pub struct Syncer<'a> {
    project: &'a Project,
    remote: &'a dyn Remote,
}

struct Gathered {
    base: FileSet,
    local: FileSet,
    fetched: Fetched,
}

impl<'a> Syncer<'a> {
    pub fn new(project: &'a Project, remote: &'a dyn Remote) -> Syncer<'a> {
        Syncer { project, remote }
    }

    fn gather(&self) -> Result<Gathered> {
        Ok(Gathered {
            base: FileSet::from_base(self.project.dir())?,
            local: FileSet::from_project_dir(self.project.dir())?,
            fetched: self.remote.fetch()?,
        })
    }

    /// What would happen, changing nothing. Fetches Overleaf once.
    pub fn preview(&self, mode: Mode) -> Result<(Preview, Staged)> {
        let g = self.gather()?;
        let planned = plan(&g.base, &g.local, &g.fetched.files, &g.fetched.skipped, mode);
        let preview = self.describe(&planned, &g, mode, &[]);
        Ok((preview, Staged { base: g.base, local: g.local, remote: g.fetched.files }))
    }

    fn describe(
        &self,
        planned: &Plan,
        g: &Gathered,
        mode: Mode,
        resolutions: &[Resolution],
    ) -> Preview {
        let items = items_of(planned, &g.local, &g.fetched.files);
        let conflicts: Vec<Conflict> = planned
            .entries
            .iter()
            .filter_map(|e| match &e.decision {
                Decision::Conflict { info, .. } if e.applies => Some(info.clone()),
                _ => None,
            })
            .collect();

        let mut summary = Summary::default();
        for e in &planned.entries {
            count(&mut summary, e);
        }

        let mut warnings = Vec::new();
        let root = &self.project.meta.root_file;
        let root_deleted = planned.entries.iter().any(|e| {
            e.applies && &e.path == root && matches!(e.decision, Decision::Pull(None))
        });
        if root_deleted {
            warnings.push(format!(
                "Overleaf deleted `{root}`, which Moss compiles. Pick a new Root file in Settings afterwards."
            ));
        }
        if g.base.is_empty() && self.project.overleaf_link().is_some() && !g.local.is_empty() {
            warnings.push(
                "This Project has not been synced before, so Moss cannot tell which side deleted a file. Nothing will be deleted; files that differ need your choice."
                    .to_string(),
            );
        }

        let blocked = (mode != Mode::Push && g.fetched.files.is_empty() && !g.base.is_empty()).then(|| {
            "Overleaf returned an empty project. Moss will not delete everything on that basis; check the project on Overleaf first."
                .to_string()
        });

        let tasks = tasks_for(planned, resolutions, mode, &[]);
        let mass_delete = mass_delete_of(&tasks, &g.local, &g.fetched.files);

        Preview {
            token: token_of(g),
            mode,
            items,
            conflicts,
            needs_attention: planned.needs_attention.clone(),
            warnings,
            mass_delete,
            blocked,
            summary,
        }
    }

    /// Do what the Review showed. Re-fetches Overleaf first and refuses (as `Stale`) when either
    /// side no longer matches the token.
    pub fn apply(&self, request: &ApplyRequest) -> Result<Applied> {
        let _lock = SyncLock::acquire(self.project.dir())?;
        let g = self.gather()?;
        let current = token_of(&g);
        if let Some(side) = stale_side(&request.token, &current) {
            return Ok(Applied::Stale { side });
        }
        let planned = plan(&g.base, &g.local, &g.fetched.files, &g.fetched.skipped, request.mode);
        let preview = self.describe(&planned, &g, request.mode, &request.resolutions);
        if let Some(reason) = &preview.blocked {
            return Err(Error::Sync(reason.clone()));
        }

        let skip: BTreeSet<&str> = request.skip.iter().map(String::as_str).collect();
        let unresolved: Vec<&str> = planned
            .entries
            .iter()
            .filter(|e| e.applies && !skip.contains(e.path.as_str()))
            .filter(|e| matches!(e.decision, Decision::Conflict { .. }))
            .filter(|e| !request.resolutions.iter().any(|r| r.path == e.path))
            .map(|e| e.path.as_str())
            .collect();
        if !unresolved.is_empty() {
            return Err(Error::Sync(format!(
                "{} conflict(s) still need an answer: {}",
                unresolved.len(),
                unresolved.join(", ")
            )));
        }

        let tasks = build_tasks(&planned, request, &skip)?;
        let md = mass_delete_of(&tasks, &g.local, &g.fetched.files);
        if let Some(m) = md
            && !request.allow_mass_delete
        {
            return Err(Error::Sync(format!(
                "this would delete {} of {} files {}; confirm to go ahead",
                m.count,
                m.total,
                if m.side == "here" { "on this PC" } else { "on Overleaf" }
            )));
        }

        let outcome = self.execute(tasks, &planned, &g, request)?;
        Ok(Applied::Done { outcome })
    }

    fn execute(
        &self,
        tasks: Vec<Task>,
        planned: &Plan,
        g: &Gathered,
        request: &ApplyRequest,
    ) -> Result<Outcome> {
        let dir = self.project.dir();
        let history = History::new(dir);
        let mut outcome = Outcome {
            left_alone: planned
                .entries
                .iter()
                .filter(|e| !matches!(e.decision, Decision::Adopt))
                .filter(|e| !e.applies || request.skip.contains(&e.path))
                .count(),
            ..Outcome::default()
        };

        // Keep anything typed since the last version before files change.
        history.capture_as(VersionKind::Auto, AUTHOR_YOU, "Your changes")?;

        let mut lnow: BTreeMap<String, Option<Blob>> = BTreeMap::new();
        let mut rnow: BTreeMap<String, Option<Blob>> = BTreeMap::new();
        let mut local_ok: BTreeSet<usize> = BTreeSet::new();
        let (mut pulled_files, mut merged_files, mut resolved_files) = (0usize, 0usize, 0usize);

        // 1. This PC.
        for (i, task) in tasks.iter().enumerate() {
            if task.local.is_none() && task.rename_local_from.is_none() {
                continue;
            }
            match apply_local(dir, task, &mut lnow) {
                Ok(()) => {
                    local_ok.insert(i);
                    match task.class {
                        Class::Pull => pulled_files += 1,
                        Class::Merge => merged_files += 1,
                        Class::Resolved => resolved_files += 1,
                        _ => {}
                    }
                }
                Err(e) => outcome.failures.push(Failure { path: task.path.clone(), error: e.to_string() }),
            }
        }
        if !local_ok.is_empty() {
            let (kind, author, label) = local_version_label(pulled_files, merged_files, resolved_files);
            if let Some(v) = history.capture_as(kind, author, &label)? {
                outcome.versions.push(v);
            }
        }
        outcome.pulled = pulled_files;
        outcome.merged = merged_files;
        outcome.resolved = resolved_files;

        // 2. Overleaf. A path whose local step failed is not sent: the two sides would drift.
        let mut push_stats = Stats::default();
        'remote: for (i, task) in tasks.iter().enumerate() {
            if task.remote.is_none() && task.rename_remote_from.is_none() {
                continue;
            }
            let needs_local = task.local.is_some() || task.rename_local_from.is_some();
            if needs_local && !local_ok.contains(&i) {
                continue;
            }
            match apply_remote(self.remote, task, g, &mut rnow) {
                Ok(lines) => {
                    outcome.pushed += 1;
                    push_stats.files += 1;
                    push_stats.added += lines.added;
                    push_stats.removed += lines.removed;
                }
                Err(Error::OverleafAuth) => {
                    outcome.aborted =
                        Some("Your Overleaf session expired. Sign in again and Sync to finish.".to_string());
                    break 'remote;
                }
                Err(e) => outcome.failures.push(Failure { path: task.path.clone(), error: e.to_string() }),
            }
        }

        // 3. Settle the Base snapshot file by file, only where both sides now agree.
        let mut touched: BTreeSet<String> = BTreeSet::new();
        for task in &tasks {
            touched.insert(task.path.clone());
            touched.extend(task.rename_local_from.clone());
            touched.extend(task.rename_remote_from.clone());
        }
        for path in &touched {
            let l = lnow.get(path).cloned().unwrap_or_else(|| g.local.get(path).cloned());
            let r = rnow.get(path).cloned().unwrap_or_else(|| g.fetched.files.get(path).cloned());
            if l.as_ref().map(|b| &b.hash) == r.as_ref().map(|b| &b.hash) {
                set_base(dir, path, l.as_ref())?;
            }
        }

        if outcome.pushed > 0 {
            let label = format!(
                "Pushed {} file{} to Overleaf",
                outcome.pushed,
                if outcome.pushed == 1 { "" } else { "s" }
            );
            outcome.versions.push(history.mark(VersionKind::Push, &request.actor, &label, push_stats)?);
        }

        outcome.complete = outcome.failures.is_empty() && outcome.aborted.is_none();
        if outcome.complete
            && let Some(mut link) = self.project.overleaf_link()
        {
            link.last_synced = Some(now_unix());
            self.project.link_overleaf(&link)?;
        }
        Ok(outcome)
    }
}

fn count(summary: &mut Summary, e: &Entry) {
    use Decision::*;
    match &e.decision {
        Pull(_) | PullRename { .. } => summary.behind += 1,
        Push(_) | PushRename { .. } => summary.ahead += 1,
        Merge { .. } | Conflict { .. } => {
            summary.behind += 1;
            summary.ahead += 1;
        }
        Adopt => {}
    }
    if !e.applies {
        return;
    }
    match &e.decision {
        Pull(None) => summary.delete_here += 1,
        Pull(Some(_)) | PullRename { .. } => summary.pull += 1,
        Push(None) => summary.delete_there += 1,
        Push(Some(_)) | PushRename { .. } => summary.push += 1,
        Merge { .. } => summary.merge += 1,
        Conflict { .. } => summary.conflict += 1,
        Adopt => {}
    }
}

/// What a path turns into: the local and remote end state, plus renames.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Class {
    Pull,
    Push,
    Merge,
    Resolved,
    Adopt,
}

struct Task {
    path: String,
    class: Class,
    /// `Some(x)` makes the local file `x` (`None` deletes it); `None` leaves it.
    local: Option<Option<Blob>>,
    remote: Option<Option<Blob>>,
    rename_local_from: Option<String>,
    rename_remote_from: Option<String>,
}

/// Plan entries to tasks, for the Review's mass-delete check, where conflicts are not yet
/// answered (their delete is only counted once a resolution picks it).
fn tasks_for(planned: &Plan, resolutions: &[Resolution], mode: Mode, skip: &[String]) -> Vec<Task> {
    let request = ApplyRequest {
        mode,
        token: String::new(),
        resolutions: resolutions.to_vec(),
        allow_mass_delete: false,
        allow_markers: true,
        skip: skip.to_vec(),
        actor: default_actor(),
    };
    let skip_set: BTreeSet<&str> = skip.iter().map(String::as_str).collect();
    build_tasks(planned, &request, &skip_set).unwrap_or_default()
}

fn build_tasks(planned: &Plan, request: &ApplyRequest, skip: &BTreeSet<&str>) -> Result<Vec<Task>> {
    let mut tasks = Vec::new();
    for e in &planned.entries {
        if !e.applies || skip.contains(e.path.as_str()) {
            continue;
        }
        let both = request.mode == Mode::Sync;
        let task = match &e.decision {
            Decision::Adopt => Task::new(&e.path, Class::Adopt),
            Decision::Pull(b) => Task { local: Some(b.clone()), ..Task::new(&e.path, Class::Pull) },
            Decision::Push(b) => Task { remote: Some(b.clone()), ..Task::new(&e.path, Class::Push) },
            Decision::PullRename { from, blob } => Task {
                local: Some(Some(blob.clone())),
                rename_local_from: Some(from.clone()),
                ..Task::new(&e.path, Class::Pull)
            },
            Decision::PushRename { from, blob } => Task {
                remote: Some(Some(blob.clone())),
                rename_remote_from: Some(from.clone()),
                ..Task::new(&e.path, Class::Push)
            },
            Decision::Merge { text } => {
                let blob = Blob::new(&e.path, text.as_bytes());
                Task {
                    local: Some(Some(blob.clone())),
                    remote: both.then_some(Some(blob)),
                    ..Task::new(&e.path, Class::Merge)
                }
            }
            Decision::Conflict { merged, mine, theirs, .. } => {
                let Some(resolution) = request.resolutions.iter().find(|r| r.path == e.path) else {
                    continue; // unanswered: only reachable from the Review's own count
                };
                let blob = resolve(&e.path, &resolution.choice, merged.as_ref(), mine, theirs, request.allow_markers)?;
                Task {
                    local: Some(blob.clone()),
                    remote: both.then_some(blob),
                    ..Task::new(&e.path, Class::Resolved)
                }
            }
        };
        tasks.push(task);
    }
    Ok(tasks)
}

impl Task {
    fn new(path: &str, class: Class) -> Task {
        Task {
            path: path.to_string(),
            class,
            local: None,
            remote: None,
            rename_local_from: None,
            rename_remote_from: None,
        }
    }
}

fn resolve(
    path: &str,
    choice: &FileChoice,
    merged: Option<&crate::textdiff::Merged>,
    mine: &Option<Blob>,
    theirs: &Option<Blob>,
    allow_markers: bool,
) -> Result<Option<Blob>> {
    let text = match choice {
        FileChoice::Mine => return Ok(mine.clone()),
        FileChoice::Theirs => return Ok(theirs.clone()),
        FileChoice::Regions { choices } => merged
            .ok_or_else(|| Error::Sync(format!("{path} has no regions to choose between")))?
            .resolve(choices)?,
        FileChoice::Text { text } => text.clone(),
    };
    if !allow_markers && has_conflict_markers(&text) {
        return Err(Error::Sync(format!(
            "{path} still contains conflict markers (<<<<<<< or >>>>>>>); remove them or confirm"
        )));
    }
    Ok(Some(Blob::new(path, text.as_bytes())))
}

fn has_conflict_markers(text: &str) -> bool {
    text.lines().any(|l| l.starts_with("<<<<<<<") || l.starts_with(">>>>>>>"))
}

fn mass_delete_of(tasks: &[Task], local: &FileSet, remote: &FileSet) -> Option<MassDelete> {
    let here = tasks.iter().filter(|t| matches!(t.local, Some(None))).count();
    let there = tasks.iter().filter(|t| matches!(t.remote, Some(None))).count();
    let over = |n: usize, total: usize| n >= MASS_DELETE_MIN && n * 100 > MASS_DELETE_PERCENT * total.max(1);
    if over(here, local.len()) {
        return Some(MassDelete { side: "here".into(), count: here, total: local.len() });
    }
    if over(there, remote.len()) {
        return Some(MassDelete { side: "overleaf".into(), count: there, total: remote.len() });
    }
    None
}

fn apply_local(dir: &Path, task: &Task, lnow: &mut BTreeMap<String, Option<Blob>>) -> Result<()> {
    if let Some(from) = &task.rename_local_from {
        let (a, b) = (safe_join(dir, from)?, safe_join(dir, &task.path)?);
        if let Some(parent) = b.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| Error::io(format!("creating folder for {}", task.path), e))?;
        }
        fs::rename(&a, &b).map_err(|e| Error::io(format!("renaming {from}"), e))?;
        remove_empty_parents(&a, dir);
        lnow.insert(from.clone(), None);
    }
    match &task.local {
        None => {}
        Some(Some(blob)) => {
            write_bytes(dir, &task.path, &blob.bytes)?;
            lnow.insert(task.path.clone(), Some(blob.clone()));
        }
        Some(None) => {
            let file = safe_join(dir, &task.path)?;
            match fs::remove_file(&file) {
                Ok(()) => remove_empty_parents(&file, dir),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(Error::io(format!("deleting {}", task.path), e)),
            }
            lnow.insert(task.path.clone(), None);
        }
    }
    Ok(())
}

/// Returns the line counts of what was sent, for the log.
fn apply_remote(
    remote: &dyn Remote,
    task: &Task,
    g: &Gathered,
    rnow: &mut BTreeMap<String, Option<Blob>>,
) -> Result<crate::textdiff::LineStats> {
    let mut lines = crate::textdiff::LineStats::default();
    if let Some(from) = &task.rename_remote_from {
        let Some(Some(blob)) = &task.remote else {
            return Err(Error::Sync("a rename needs the new content".into()));
        };
        // Overleaf's own rename keeps the file's history; if it refuses, send the new name and
        // remove the old one instead.
        if remote.rename(from, &task.path).is_err() {
            remote.upload(&task.path, &blob.bytes)?;
            remote.delete(from)?;
        }
        rnow.insert(from.clone(), None);
        rnow.insert(task.path.clone(), Some(blob.clone()));
        return Ok(lines);
    }
    match &task.remote {
        None => {}
        Some(Some(blob)) => {
            let old = g.fetched.files.get(&task.path);
            let (old_text, new_text) = (old.and_then(Blob::text), blob.text());
            if let (Some(o), Some(n)) = (old_text.or(old.is_none().then_some("")), new_text) {
                lines = line_stats(o, n);
            }
            remote.upload(&task.path, &blob.bytes)?;
            rnow.insert(task.path.clone(), Some(blob.clone()));
        }
        Some(None) => {
            // Already gone is as good as deleted.
            if g.fetched.files.get(&task.path).is_some() {
                remote.delete(&task.path)?;
            }
            rnow.insert(task.path.clone(), None);
        }
    }
    Ok(lines)
}

fn local_version_label(pulled: usize, merged: usize, resolved: usize) -> (VersionKind, &'static str, String) {
    let files = |n: usize| format!("{n} file{}", if n == 1 { "" } else { "s" });
    if merged + resolved > 0 {
        let mut label = format!("Merged with Overleaf: {}", files(pulled + merged + resolved));
        if resolved > 0 {
            label.push_str(&format!(", {resolved} conflict{} resolved", if resolved == 1 { "" } else { "s" }));
        }
        (VersionKind::Merge, MOSS_AUTHOR, label)
    } else {
        (VersionKind::Pull, AUTHOR_OVERLEAF, format!("Pulled from Overleaf: {}", files(pulled)))
    }
}

fn base_path(dir: &Path, rel: &str) -> PathBuf {
    dir.join(META_DIR).join("base").join(rel)
}

fn set_base(dir: &Path, rel: &str, blob: Option<&Blob>) -> Result<()> {
    let path = base_path(dir, rel);
    match blob {
        Some(b) => {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)
                    .map_err(|e| Error::io(format!("creating base folder for {rel}"), e))?;
            }
            fs::write(&path, &b.bytes).map_err(|e| Error::io(format!("saving base copy of {rel}"), e))
        }
        None => {
            match fs::remove_file(&path) {
                Ok(()) => remove_empty_parents(&path, &dir.join(META_DIR).join("base")),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(Error::io(format!("removing base copy of {rel}"), e)),
            }
            Ok(())
        }
    }
}

fn remove_empty_parents(file: &Path, stop_at: &Path) {
    let mut d = file.parent();
    while let Some(dir) = d {
        if dir == stop_at || fs::remove_dir(dir).is_err() {
            break;
        }
        d = dir.parent();
    }
}

/// `r<digest of Overleaf's files>.l<digest of this PC's files>`.
fn token_of(g: &Gathered) -> String {
    format!("r{}.l{}", &g.fetched.files.digest()[..16], &g.local.digest()[..16])
}

fn stale_side(requested: &str, current: &str) -> Option<StaleSide> {
    if requested == current {
        return None;
    }
    let part = |t: &str, tag: char| t.split('.').find(|p| p.starts_with(tag)).map(str::to_string);
    if part(requested, 'r') != part(current, 'r') {
        Some(StaleSide::Overleaf)
    } else {
        Some(StaleSide::Here)
    }
}
