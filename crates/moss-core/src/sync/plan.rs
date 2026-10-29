//! Working out what a Sync would do, without doing any of it.
//!
//! Three file sets go in (Base, local, remote); a list of per-file decisions comes out. This is
//! pure: no disk, no network, so every row of the decision table in
//! `docs/plans/m2-sync-and-versions.md` is a plain unit test.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::fileset::{Blob, FileSet};
use crate::textdiff::{Choice, LineStats, MAX_DIFF_BYTES, Merged, Region, line_stats, merge3};
use crate::workspace::Skipped;

/// Which way the user asked to sync.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// Bring Overleaf's changes here. Local-only changes stay on this PC.
    Pull,
    /// Send local changes to Overleaf. Files Overleaf also changed are left for a Pull.
    Push,
    /// Both, merging files changed on both sides.
    Sync,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictKind {
    BothModified,
    BothAdded,
    /// This PC changed the file; Overleaf deleted it.
    ModifiedDeleted,
    /// This PC deleted the file; Overleaf changed it.
    DeletedModified,
}

/// A file Moss cannot settle alone. Mine is this PC, Theirs is Overleaf.
#[derive(Debug, Clone, Serialize)]
pub struct Conflict {
    pub path: String,
    pub kind: ConflictKind,
    pub binary: bool,
    pub mine_exists: bool,
    pub theirs_exists: bool,
    pub base: Option<String>,
    pub mine: Option<String>,
    pub theirs: Option<String>,
    /// Line-by-line view of a text file, with the overlapping edits as conflict regions.
    pub regions: Option<Vec<Region>>,
}

/// The user's answer for one [`Conflict`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Resolution {
    pub path: String,
    pub choice: FileChoice,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum FileChoice {
    /// The whole file as it is on this PC (including "deleted").
    Mine,
    /// The whole file as it is on Overleaf (including "deleted").
    Theirs,
    /// One choice per conflicting region, in order.
    Regions { choices: Vec<Choice> },
    /// Exactly this text.
    Text { text: String },
}

#[derive(Debug, Clone)]
pub(crate) enum Decision {
    /// Overleaf's copy replaces or creates the local one; `None` deletes the local file.
    Pull(Option<Blob>),
    /// This PC's copy replaces or creates Overleaf's; `None` deletes it there.
    Push(Option<Blob>),
    /// Both changed in compatible ways: the merged text goes to both sides.
    Merge { text: String },
    Conflict { info: Conflict, merged: Option<Merged>, mine: Option<Blob>, theirs: Option<Blob> },
    /// Both sides already agree; only the Base snapshot needs to catch up.
    Adopt,
    /// Overleaf renamed `from` to this path.
    PullRename { from: String, blob: Blob },
    /// This PC renamed `from` to this path.
    PushRename { from: String, blob: Blob },
}

#[derive(Debug, Clone)]
pub(crate) struct Entry {
    pub path: String,
    pub decision: Decision,
    /// False when this run leaves the file alone (wrong direction, or Overleaf changed it too).
    pub applies: bool,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    Pull,
    Push,
    Both,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Op {
    Add,
    Modify,
    Delete,
    Rename,
    Merge,
    Conflict,
}

/// One row of the Review.
#[derive(Debug, Clone, Serialize)]
pub struct Item {
    pub path: String,
    /// Old name, for a rename.
    pub from: Option<String>,
    pub direction: Direction,
    pub op: Op,
    /// False when this run leaves the file alone (see `note`).
    pub applies: bool,
    pub note: Option<String>,
    pub binary: bool,
    pub lines: LineStats,
}

pub(crate) struct Plan {
    pub entries: Vec<Entry>,
    pub needs_attention: Vec<Skipped>,
}

/// Decide what to do with every path. `remote_names` are every name Overleaf holds, including
/// the ones skipped, so a local file cannot be uploaded over a case-only twin.
pub(crate) fn plan(
    base: &FileSet,
    local: &FileSet,
    remote: &FileSet,
    remote_skipped: &[Skipped],
    mode: Mode,
) -> Plan {
    let mut paths: BTreeSet<&String> = BTreeSet::new();
    paths.extend(base.files.keys());
    paths.extend(local.files.keys());
    paths.extend(remote.files.keys());

    let mut entries: Vec<Entry> = Vec::new();
    for path in paths {
        if let Some(decision) = classify(path, base.get(path), local.get(path), remote.get(path)) {
            entries.push(Entry { path: path.clone(), decision, applies: true, note: None });
        }
    }
    detect_renames(&mut entries, base);

    let mut needs_attention: Vec<Skipped> = remote_skipped.to_vec();
    keep_case_twins_apart(&mut entries, remote, remote_skipped, &mut needs_attention);

    for entry in &mut entries {
        restrict_to_mode(entry, mode);
    }
    Plan { entries, needs_attention }
}

fn same(a: Option<&Blob>, b: Option<&Blob>) -> bool {
    a.map(|x| &x.hash) == b.map(|x| &x.hash)
}

fn classify(path: &str, base: Option<&Blob>, local: Option<&Blob>, remote: Option<&Blob>) -> Option<Decision> {
    if same(local, remote) {
        return if same(local, base) { None } else { Some(Decision::Adopt) };
    }
    if same(local, base) {
        return Some(Decision::Pull(remote.cloned()));
    }
    if same(remote, base) {
        return Some(Decision::Push(local.cloned()));
    }
    // Both sides changed, differently.
    let kind = match (base, local, remote) {
        (None, Some(_), Some(_)) => ConflictKind::BothAdded,
        (Some(_), Some(_), Some(_)) => ConflictKind::BothModified,
        (Some(_), Some(_), None) => ConflictKind::ModifiedDeleted,
        (Some(_), None, Some(_)) => ConflictKind::DeletedModified,
        // Unreachable: absent on every side means both sides "agree".
        _ => return None,
    };
    let texts = (
        base.map(Blob::text),
        local.map(Blob::text),
        remote.map(Blob::text),
    );
    let all_text = !matches!(texts, (Some(None), _, _) | (_, Some(None), _) | (_, _, Some(None)));
    let small = [base, local, remote]
        .into_iter()
        .flatten()
        .all(|b| b.bytes.len() <= MAX_DIFF_BYTES);
    let (base_t, mine_t, theirs_t) = (
        texts.0.flatten(),
        texts.1.flatten(),
        texts.2.flatten(),
    );

    // A modify/modify or add/add of text can often merge; deletes always need a decision.
    if all_text
        && small
        && let (Some(m), Some(t)) = (mine_t, theirs_t)
    {
        let merged = merge3(base_t.unwrap_or(""), m, t);
        if let Some(text) = merged.clean_text() {
            return Some(Decision::Merge { text });
        }
        let info = Conflict {
            path: path.to_string(),
            kind,
            binary: false,
            mine_exists: true,
            theirs_exists: true,
            base: base_t.map(str::to_string),
            mine: Some(m.to_string()),
            theirs: Some(t.to_string()),
            regions: Some(merged.regions.clone()),
        };
        return Some(Decision::Conflict {
            info,
            merged: Some(merged),
            mine: local.cloned(),
            theirs: remote.cloned(),
        });
    }
    let info = Conflict {
        path: path.to_string(),
        kind,
        binary: !all_text,
        mine_exists: local.is_some(),
        theirs_exists: remote.is_some(),
        base: if all_text && small { base_t.map(str::to_string) } else { None },
        mine: if all_text && small { mine_t.map(str::to_string) } else { None },
        theirs: if all_text && small { theirs_t.map(str::to_string) } else { None },
        regions: None,
    };
    Some(Decision::Conflict { info, merged: None, mine: local.cloned(), theirs: remote.cloned() })
}

/// A delete plus an add of identical content on one side is a rename. Empty files are skipped
/// (every empty file looks like every other one).
fn detect_renames(entries: &mut Vec<Entry>, base: &FileSet) {
    for pull in [true, false] {
        let mut removed: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        for (i, e) in entries.iter().enumerate() {
            let is_delete = matches!(
                (&e.decision, pull),
                (Decision::Pull(None), true) | (Decision::Push(None), false)
            );
            if !is_delete {
                continue;
            }
            if let Some(b) = base.get(&e.path).filter(|b| !b.bytes.is_empty()) {
                removed.entry(b.hash.clone()).or_default().push(i);
            }
        }
        let mut gone: BTreeSet<usize> = BTreeSet::new();
        let mut renamed: Vec<(usize, String, Blob)> = Vec::new();
        for (i, e) in entries.iter().enumerate() {
            let added = match (&e.decision, pull) {
                (Decision::Pull(Some(b)), true) | (Decision::Push(Some(b)), false)
                    if base.get(&e.path).is_none() =>
                {
                    b
                }
                _ => continue,
            };
            if let Some(src) = removed.get_mut(&added.hash).and_then(Vec::pop) {
                gone.insert(src);
                renamed.push((i, entries[src].path.clone(), added.clone()));
            }
        }
        for (i, from, blob) in renamed {
            entries[i].decision = if pull {
                Decision::PullRename { from, blob }
            } else {
                Decision::PushRename { from, blob }
            };
        }
        let mut index = 0;
        entries.retain(|_| {
            let keep = !gone.contains(&index);
            index += 1;
            keep
        });
    }
}

fn keep_case_twins_apart(
    entries: &mut Vec<Entry>,
    remote: &FileSet,
    remote_skipped: &[Skipped],
    needs_attention: &mut Vec<Skipped>,
) {
    let mut taken: BTreeMap<String, String> = BTreeMap::new();
    for path in remote.files.keys().map(String::as_str).chain(remote_skipped.iter().map(|s| s.path.as_str())) {
        taken.insert(path.to_lowercase(), path.to_string());
    }
    entries.retain(|e| {
        let is_new_upload = matches!(e.decision, Decision::Push(Some(_)) | Decision::PushRename { .. });
        if !is_new_upload {
            return true;
        }
        match taken.get(&e.path.to_lowercase()) {
            Some(existing) if *existing != e.path => {
                needs_attention.push(Skipped {
                    path: e.path.clone(),
                    reason: format!("differs only by case from `{existing}` on Overleaf"),
                });
                false
            }
            _ => true,
        }
    });
}

fn restrict_to_mode(entry: &mut Entry, mode: Mode) {
    let (applies, note) = match (&entry.decision, mode) {
        (Decision::Adopt, _) => (true, None),
        (Decision::Pull(_) | Decision::PullRename { .. }, Mode::Push) => {
            (false, Some("Overleaf has a newer version; Pull to get it.".to_string()))
        }
        (Decision::Push(_) | Decision::PushRename { .. }, Mode::Pull) => {
            (false, Some("Stays on this PC (Pull does not send changes).".to_string()))
        }
        (Decision::Merge { .. } | Decision::Conflict { .. }, Mode::Push) => {
            (false, Some("Overleaf also changed this file. Pull first.".to_string()))
        }
        _ => (true, None),
    };
    entry.applies = applies;
    entry.note = note;
}

/// The Review's view of a plan.
pub(crate) fn items_of(plan: &Plan, local: &FileSet, remote: &FileSet) -> Vec<Item> {
    plan.entries
        .iter()
        .filter(|e| !matches!(e.decision, Decision::Adopt))
        .map(|e| item_of(e, local, remote))
        .collect()
}

fn item_of(e: &Entry, local: &FileSet, remote: &FileSet) -> Item {
    let text = |b: Option<&Blob>| b.and_then(Blob::text).unwrap_or("").to_string();
    let lines_between = |old: Option<&Blob>, new: Option<&Blob>| match (old.map(Blob::text), new.map(Blob::text)) {
        (Some(None), _) | (_, Some(None)) => (true, LineStats::default()),
        _ => (false, line_stats(&text(old), &text(new))),
    };
    let (direction, op, from, binary, lines) = match &e.decision {
        Decision::Pull(new) => {
            let old = local.get(&e.path);
            let (bin, lines) = lines_between(old, new.as_ref());
            let op = match (old, new) {
                (None, Some(_)) => Op::Add,
                (_, None) => Op::Delete,
                _ => Op::Modify,
            };
            (Direction::Pull, op, None, bin, lines)
        }
        Decision::Push(new) => {
            let old = remote.get(&e.path);
            let (bin, lines) = lines_between(old, new.as_ref());
            let op = match (old, new) {
                (None, Some(_)) => Op::Add,
                (_, None) => Op::Delete,
                _ => Op::Modify,
            };
            (Direction::Push, op, None, bin, lines)
        }
        Decision::Merge { text: merged } => {
            let lines = line_stats(&text(local.get(&e.path)), merged);
            (Direction::Both, Op::Merge, None, false, lines)
        }
        Decision::Conflict { info, .. } => (Direction::Both, Op::Conflict, None, info.binary, LineStats::default()),
        Decision::Adopt => (Direction::Both, Op::Modify, None, false, LineStats::default()),
        Decision::PullRename { from, .. } => (Direction::Pull, Op::Rename, Some(from.clone()), false, LineStats::default()),
        Decision::PushRename { from, .. } => (Direction::Push, Op::Rename, Some(from.clone()), false, LineStats::default()),
    };
    Item {
        path: e.path.clone(),
        from,
        direction,
        op,
        applies: e.applies,
        note: e.note.clone(),
        binary,
        lines,
    }
}
