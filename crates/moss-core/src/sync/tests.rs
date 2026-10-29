//! The edge-case matrix from `docs/plans/m2-sync-and-versions.md`, one test per row. Every test
//! runs the real engine against a `MemoryRemote` standing in for Overleaf.

use std::collections::BTreeSet;
use std::fs;
use std::sync::Mutex;

use tempfile::TempDir;

use super::*;
use crate::fsutil::write_text;
use crate::history::{History, VersionKind};
use crate::project::{OverleafLink, Project};
use crate::textdiff::Choice;
use crate::{Error, Result};

struct Env {
    dir: TempDir,
    project: Project,
    remote: MemoryRemote,
}

/// A linked Project whose files, Overleaf's files and the Base snapshot all start identical.
fn env(files: &[(&str, &str)]) -> Env {
    let dir = tempfile::tempdir().unwrap();
    for (path, text) in files {
        write_text(dir.path(), path, text).unwrap();
    }
    let project = Project::init(dir.path(), "Test", "main.tex").unwrap();
    project
        .link_overleaf(&OverleafLink { project_id: "p1".into(), name: "Test".into(), last_synced: None })
        .unwrap();
    project.save_base_snapshot().unwrap();
    Env { dir, project, remote: MemoryRemote::with(files) }
}

impl Env {
    fn write(&self, path: &str, text: &str) {
        write_text(self.dir.path(), path, text).unwrap();
    }
    fn read(&self, path: &str) -> Option<String> {
        fs::read_to_string(self.dir.path().join(path)).ok()
    }
    fn delete(&self, path: &str) {
        fs::remove_file(self.dir.path().join(path)).unwrap();
    }
    fn base(&self, path: &str) -> Option<String> {
        fs::read_to_string(self.dir.path().join(".moss/base").join(path)).ok()
    }
    fn preview_with(&self, remote: &dyn Remote, mode: Mode) -> Preview {
        Syncer::new(&self.project, remote).preview(mode).unwrap().0
    }
    fn preview(&self, mode: Mode) -> Preview {
        self.preview_with(&self.remote, mode)
    }
    fn request(&self, p: &Preview) -> ApplyRequest {
        ApplyRequest {
            mode: p.mode,
            token: p.token.clone(),
            resolutions: vec![],
            allow_mass_delete: false,
            allow_markers: false,
            skip: vec![],
            actor: "You".into(),
        }
    }
    fn apply_with(&self, remote: &dyn Remote, request: &ApplyRequest) -> Result<Applied> {
        Syncer::new(&self.project, remote).apply(request)
    }
    /// Preview then apply with no resolutions; panics unless it finished.
    fn run_with(&self, remote: &dyn Remote, mode: Mode) -> Outcome {
        let p = self.preview_with(remote, mode);
        match self.apply_with(remote, &self.request(&p)).unwrap() {
            Applied::Done { outcome } => outcome,
            Applied::Stale { side } => panic!("unexpectedly stale: {side:?}"),
        }
    }
    fn run(&self, mode: Mode) -> Outcome {
        self.run_with(&self.remote, mode)
    }
    fn in_sync(&self) -> bool {
        local_changes(&self.project).unwrap().is_empty()
    }
}

fn resolve_all(p: &Preview, req: &mut ApplyRequest, choice: FileChoice) {
    req.resolutions = p
        .conflicts
        .iter()
        .map(|c| Resolution { path: c.path.clone(), choice: choice.clone() })
        .collect();
}

fn done(applied: Applied) -> Outcome {
    match applied {
        Applied::Done { outcome } => outcome,
        Applied::Stale { side } => panic!("stale: {side:?}"),
    }
}

/// A remote that records calls and fails on request.
struct Faulty {
    inner: MemoryRemote,
    fail_upload: Mutex<BTreeSet<String>>,
    /// Remote writes allowed before the session "expires".
    writes_until_auth_error: Mutex<Option<usize>>,
    calls: Mutex<Vec<String>>,
}

impl Faulty {
    fn new(inner: &MemoryRemote) -> Faulty {
        Faulty {
            inner: inner.clone(),
            fail_upload: Mutex::default(),
            writes_until_auth_error: Mutex::new(None),
            calls: Mutex::default(),
        }
    }
    fn gate(&self, call: String) -> Result<()> {
        if let Some(left) = self.writes_until_auth_error.lock().unwrap().as_mut() {
            if *left == 0 {
                return Err(Error::OverleafAuth);
            }
            *left -= 1;
        }
        self.calls.lock().unwrap().push(call);
        Ok(())
    }
    fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }
}

impl Remote for Faulty {
    fn fetch(&self) -> Result<Fetched> {
        self.inner.fetch()
    }
    fn upload(&self, path: &str, bytes: &[u8]) -> Result<()> {
        self.gate(format!("upload {path}"))?;
        if self.fail_upload.lock().unwrap().contains(path) {
            return Err(Error::Overleaf(format!("{path} was rejected")));
        }
        self.inner.upload(path, bytes)
    }
    fn delete(&self, path: &str) -> Result<()> {
        self.gate(format!("delete {path}"))?;
        self.inner.delete(path)
    }
    fn rename(&self, from: &str, to: &str) -> Result<()> {
        self.gate(format!("rename {from} {to}"))?;
        self.inner.rename(from, to)
    }
}

const DOC: &str = "line1\nline2\nline3\nline4\nline5\n";

// 1
#[test]
fn nothing_changed_means_an_empty_plan_and_no_remote_writes() {
    let e = env(&[("main.tex", DOC)]);
    let faulty = Faulty::new(&e.remote);
    let p = e.preview_with(&faulty, Mode::Sync);
    assert!(p.items.is_empty() && p.conflicts.is_empty());
    let out = e.run_with(&faulty, Mode::Sync);
    assert!(out.complete);
    assert_eq!((out.pulled, out.pushed, out.merged), (0, 0, 0));
    assert!(faulty.calls().is_empty());
}

// 2
#[test]
fn different_files_changed_on_each_side_both_apply() {
    let e = env(&[("main.tex", DOC), ("a.tex", "a\n"), ("b.tex", "b\n")]);
    e.write("a.tex", "a local\n");
    e.remote.put("b.tex", b"b remote\n");
    e.remote.put("new-remote.tex", b"n\n");
    e.write("new-local.tex", "n\n");

    let out = e.run(Mode::Sync);
    assert!(out.complete, "{out:?}");
    assert_eq!(e.read("b.tex").as_deref(), Some("b remote\n"));
    assert_eq!(e.read("new-remote.tex").as_deref(), Some("n\n"));
    assert_eq!(e.remote.text("a.tex").as_deref(), Some("a local\n"));
    assert_eq!(e.remote.text("new-local.tex").as_deref(), Some("n\n"));
    assert!(e.in_sync());
    assert_eq!(e.base("b.tex").as_deref(), Some("b remote\n"));
    assert_eq!(e.base("a.tex").as_deref(), Some("a local\n"));
    // A second run has nothing to do.
    assert!(e.preview(Mode::Sync).items.is_empty());
}

// 3
#[test]
fn edits_to_different_lines_of_one_file_merge_onto_both_sides() {
    let e = env(&[("main.tex", DOC)]);
    e.write("main.tex", "LOCAL1\nline2\nline3\nline4\nline5\n");
    e.remote.put("main.tex", b"line1\nline2\nline3\nline4\nREMOTE5\n");

    let p = e.preview(Mode::Sync);
    assert_eq!(p.items.len(), 1);
    assert_eq!(p.items[0].op, Op::Merge);
    assert!(p.conflicts.is_empty());

    let out = e.run(Mode::Sync);
    let merged = "LOCAL1\nline2\nline3\nline4\nREMOTE5\n";
    assert_eq!(e.read("main.tex").as_deref(), Some(merged));
    assert_eq!(e.remote.text("main.tex").as_deref(), Some(merged));
    assert_eq!(e.base("main.tex").as_deref(), Some(merged));
    assert_eq!((out.merged, out.pushed), (1, 1));
}

// 4
#[test]
fn the_same_line_edited_two_ways_is_a_conflict_nothing_is_written_until_resolved() {
    let e = env(&[("main.tex", DOC)]);
    e.write("main.tex", "line1\nMINE\nline3\nline4\nline5\n");
    e.remote.put("main.tex", b"line1\nTHEIRS\nline3\nline4\nline5\n");

    let p = e.preview(Mode::Sync);
    assert_eq!(p.conflicts.len(), 1);
    assert_eq!(p.conflicts[0].kind, ConflictKind::BothModified);
    assert!(p.conflicts[0].regions.is_some());

    // Applying without an answer is refused and writes nothing.
    let err = e.apply_with(&e.remote, &e.request(&p)).unwrap_err();
    assert!(err.to_string().contains("conflict"), "{err}");
    assert_eq!(e.read("main.tex").as_deref(), Some("line1\nMINE\nline3\nline4\nline5\n"));

    // Per-region choice.
    let mut req = e.request(&p);
    req.resolutions = vec![Resolution {
        path: "main.tex".into(),
        choice: FileChoice::Regions { choices: vec![Choice::Both] },
    }];
    let out = done(e.apply_with(&e.remote, &req).unwrap());
    let both = "line1\nMINE\nTHEIRS\nline3\nline4\nline5\n";
    assert_eq!(e.read("main.tex").as_deref(), Some(both));
    assert_eq!(e.remote.text("main.tex").as_deref(), Some(both));
    assert_eq!(out.resolved, 1);
    assert!(e.in_sync());
}

#[test]
fn whole_file_mine_and_theirs_and_hand_written_text() {
    for (choice, expect) in [
        (FileChoice::Mine, "MINE\n"),
        (FileChoice::Theirs, "THEIRS\n"),
        (FileChoice::Text { text: "by hand\n".into() }, "by hand\n"),
    ] {
        let e = env(&[("main.tex", "base\n")]);
        e.write("main.tex", "MINE\n");
        e.remote.put("main.tex", b"THEIRS\n");
        let p = e.preview(Mode::Sync);
        let mut req = e.request(&p);
        resolve_all(&p, &mut req, choice);
        done(e.apply_with(&e.remote, &req).unwrap());
        assert_eq!(e.read("main.tex").as_deref(), Some(expect));
        assert_eq!(e.remote.text("main.tex").as_deref(), Some(expect));
        assert!(e.in_sync());
    }
}

#[test]
fn a_wrong_number_of_region_choices_is_an_error_and_changes_nothing() {
    let e = env(&[("main.tex", "base\n")]);
    e.write("main.tex", "MINE\n");
    e.remote.put("main.tex", b"THEIRS\n");
    let p = e.preview(Mode::Sync);
    let mut req = e.request(&p);
    resolve_all(&p, &mut req, FileChoice::Regions { choices: vec![Choice::Mine, Choice::Mine] });
    assert!(e.apply_with(&e.remote, &req).is_err());
    assert_eq!(e.read("main.tex").as_deref(), Some("MINE\n"));
    assert_eq!(e.remote.text("main.tex").as_deref(), Some("THEIRS\n"));
}

// 5
#[test]
fn the_same_edit_on_both_sides_is_adopted_without_a_conflict() {
    let e = env(&[("main.tex", "old\n")]);
    e.write("main.tex", "new\n");
    e.remote.put("main.tex", b"new\n");
    let p = e.preview(Mode::Sync);
    assert!(p.items.is_empty() && p.conflicts.is_empty());
    let faulty = Faulty::new(&e.remote);
    e.run_with(&faulty, Mode::Sync);
    assert!(faulty.calls().is_empty(), "no uploads needed");
    assert_eq!(e.base("main.tex").as_deref(), Some("new\n"));
    assert!(e.in_sync());
}

// 6
#[test]
fn modified_here_deleted_there_is_a_conflict_either_answer_works() {
    for (choice, file_survives) in [(FileChoice::Mine, true), (FileChoice::Theirs, false)] {
        let e = env(&[("main.tex", DOC), ("extra.tex", "x\n")]);
        e.write("extra.tex", "x changed\n");
        e.remote.remove("extra.tex");
        let p = e.preview(Mode::Sync);
        assert_eq!(p.conflicts[0].kind, ConflictKind::ModifiedDeleted);
        let mut req = e.request(&p);
        resolve_all(&p, &mut req, choice);
        done(e.apply_with(&e.remote, &req).unwrap());
        assert_eq!(e.read("extra.tex").is_some(), file_survives);
        assert_eq!(e.remote.text("extra.tex").is_some(), file_survives);
        assert!(e.in_sync());
    }
}

#[test]
fn deleted_here_modified_there_is_a_conflict_either_answer_works() {
    for (choice, file_survives) in [(FileChoice::Mine, false), (FileChoice::Theirs, true)] {
        let e = env(&[("main.tex", DOC), ("extra.tex", "x\n")]);
        e.delete("extra.tex");
        e.remote.put("extra.tex", b"x changed\n");
        let p = e.preview(Mode::Sync);
        assert_eq!(p.conflicts[0].kind, ConflictKind::DeletedModified);
        let mut req = e.request(&p);
        resolve_all(&p, &mut req, choice);
        done(e.apply_with(&e.remote, &req).unwrap());
        assert_eq!(e.read("extra.tex").is_some(), file_survives);
        assert_eq!(e.remote.text("extra.tex").is_some(), file_survives);
        assert!(e.in_sync());
    }
}

#[test]
fn deleted_on_both_sides_is_adopted() {
    let e = env(&[("main.tex", DOC), ("gone.tex", "x\n")]);
    e.delete("gone.tex");
    e.remote.remove("gone.tex");
    assert!(e.preview(Mode::Sync).items.is_empty());
    e.run(Mode::Sync);
    assert!(e.base("gone.tex").is_none());
}

// 7
#[test]
fn both_added_the_same_name_conflicts_unless_identical() {
    let e = env(&[("main.tex", DOC)]);
    e.write("new.tex", "mine\n");
    e.remote.put("new.tex", b"theirs\n");
    let p = e.preview(Mode::Sync);
    assert_eq!(p.conflicts[0].kind, ConflictKind::BothAdded);

    let same = env(&[("main.tex", DOC)]);
    same.write("new.tex", "same\n");
    same.remote.put("new.tex", b"same\n");
    assert!(same.preview(Mode::Sync).conflicts.is_empty());
    same.run(Mode::Sync);
    assert_eq!(same.base("new.tex").as_deref(), Some("same\n"));
}

// 8
#[test]
fn binary_changed_on_both_sides_is_a_whole_file_conflict() {
    let e = env(&[("main.tex", DOC)]);
    fs::write(e.dir.path().join("fig.png"), [0u8, 1, 2]).unwrap();
    e.remote.put("fig.png", &[0u8, 1, 2]);
    e.project.save_base_snapshot().unwrap();
    fs::write(e.dir.path().join("fig.png"), [0u8, 9, 9]).unwrap();
    e.remote.put("fig.png", &[0u8, 7, 7]);

    let p = e.preview(Mode::Sync);
    assert!(p.conflicts[0].binary);
    assert!(p.conflicts[0].regions.is_none());
    let mut req = e.request(&p);
    resolve_all(&p, &mut req, FileChoice::Theirs);
    done(e.apply_with(&e.remote, &req).unwrap());
    assert_eq!(fs::read(e.dir.path().join("fig.png")).unwrap(), [0u8, 7, 7]);
}

// 25
#[test]
fn a_text_file_that_is_not_utf8_is_treated_as_binary() {
    let e = env(&[("main.tex", DOC)]);
    fs::write(e.dir.path().join("notes.txt"), "ok\n").unwrap();
    e.remote.put("notes.txt", b"ok\n");
    e.project.save_base_snapshot().unwrap();
    fs::write(e.dir.path().join("notes.txt"), [0xffu8, 0xfe, b'a']).unwrap();
    e.remote.put("notes.txt", &[0xffu8, 0xfe, b'b']);
    let p = e.preview(Mode::Sync);
    assert!(p.conflicts[0].binary);
}

// 9
#[test]
fn overleaf_changing_after_the_review_makes_apply_stale_and_writes_nothing() {
    let e = env(&[("main.tex", DOC), ("a.tex", "a\n")]);
    e.write("a.tex", "a local\n");
    let p = e.preview(Mode::Sync);
    e.remote.put("main.tex", b"someone edited on Overleaf\n");
    let applied = e.apply_with(&e.remote, &e.request(&p)).unwrap();
    assert!(matches!(applied, Applied::Stale { side: StaleSide::Overleaf }));
    assert_eq!(e.remote.text("a.tex").as_deref(), Some("a\n"), "nothing was pushed");
    assert_eq!(e.read("main.tex").as_deref(), Some(DOC), "nothing was pulled");
}

// 21
#[test]
fn local_edits_after_the_review_make_apply_stale() {
    let e = env(&[("main.tex", DOC)]);
    e.remote.put("main.tex", b"remote\n");
    let p = e.preview(Mode::Sync);
    e.write("main.tex", "typed meanwhile\n");
    let applied = e.apply_with(&e.remote, &e.request(&p)).unwrap();
    assert!(matches!(applied, Applied::Stale { side: StaleSide::Here }));
    assert_eq!(e.read("main.tex").as_deref(), Some("typed meanwhile\n"));
}

// 10
#[test]
fn one_rejected_file_does_not_stop_the_others_and_a_rerun_finishes() {
    let e = env(&[("main.tex", DOC)]);
    e.write("a.tex", "a\n");
    e.write("bad.tex", "bad\n");
    e.write("c.tex", "c\n");
    let faulty = Faulty::new(&e.remote);
    faulty.fail_upload.lock().unwrap().insert("bad.tex".into());

    let out = e.run_with(&faulty, Mode::Push);
    assert!(!out.complete);
    assert_eq!(out.pushed, 2);
    assert_eq!(out.failures.len(), 1);
    assert_eq!(out.failures[0].path, "bad.tex");
    assert!(e.remote.text("a.tex").is_some() && e.remote.text("c.tex").is_some());
    assert!(e.remote.text("bad.tex").is_none());
    assert!(e.base("a.tex").is_some(), "settled files advance Base");
    assert!(e.base("bad.tex").is_none(), "the failed file does not");
    assert!(e.project.overleaf_link().unwrap().last_synced.is_none());

    faulty.fail_upload.lock().unwrap().clear();
    let again = e.run_with(&faulty, Mode::Push);
    assert!(again.complete);
    assert_eq!(again.pushed, 1);
    assert_eq!(e.remote.text("bad.tex").as_deref(), Some("bad\n"));
    assert!(e.project.overleaf_link().unwrap().last_synced.is_some());
    assert!(e.in_sync());
}

// 11
#[test]
fn an_expired_session_stops_the_run_and_leaves_a_consistent_state() {
    let e = env(&[("main.tex", DOC), ("pulled.tex", "p\n")]);
    e.remote.put("pulled.tex", b"p remote\n");
    e.write("a.tex", "a\n");
    e.write("b.tex", "b\n");
    let faulty = Faulty::new(&e.remote);
    *faulty.writes_until_auth_error.lock().unwrap() = Some(1);

    let out = e.run_with(&faulty, Mode::Sync);
    assert!(out.aborted.is_some());
    assert!(!out.complete);
    assert_eq!(e.read("pulled.tex").as_deref(), Some("p remote\n"), "local pulls already applied");
    assert_eq!(out.pushed, 1);
    // Whatever was not sent is still a local change, so the next Sync sends it.
    *faulty.writes_until_auth_error.lock().unwrap() = None;
    let again = e.run_with(&faulty, Mode::Sync);
    assert!(again.complete);
    assert!(e.remote.text("a.tex").is_some() && e.remote.text("b.tex").is_some());
    assert!(e.in_sync());
}

// 12
#[test]
fn an_empty_project_from_overleaf_is_not_taken_as_delete_everything() {
    let e = env(&[("main.tex", DOC), ("a.tex", "a\n")]);
    let empty = MemoryRemote::new();
    let p = e.preview_with(&empty, Mode::Sync);
    assert!(p.blocked.is_some());
    assert!(e.apply_with(&empty, &e.request(&p)).is_err());
    assert!(e.read("main.tex").is_some());
    // Pushing is still safe: it never deletes on this PC.
    assert!(e.preview_with(&empty, Mode::Push).blocked.is_none());
}

// 13
#[test]
fn deleting_many_files_needs_an_explicit_yes() {
    let files: Vec<(String, String)> = (0..10).map(|i| (format!("f{i}.tex"), format!("{i}\n"))).collect();
    let refs: Vec<(&str, &str)> = files.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
    let e = env(&refs);
    for i in 0..6 {
        e.remote.remove(&format!("f{i}.tex"));
    }
    let p = e.preview(Mode::Pull);
    let md = p.mass_delete.clone().expect("flagged");
    assert_eq!((md.side.as_str(), md.count, md.total), ("here", 6, 10));
    assert!(e.apply_with(&e.remote, &e.request(&p)).is_err());
    assert!(e.read("f0.tex").is_some());

    let mut req = e.request(&p);
    req.allow_mass_delete = true;
    done(e.apply_with(&e.remote, &req).unwrap());
    assert!(e.read("f0.tex").is_none() && e.read("f9.tex").is_some());
}

#[test]
fn a_few_deletions_do_not_trip_the_guard() {
    let files: Vec<(String, String)> = (0..10).map(|i| (format!("f{i}.tex"), format!("{i}\n"))).collect();
    let refs: Vec<(&str, &str)> = files.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
    let e = env(&refs);
    e.remote.remove("f0.tex");
    e.remote.remove("f1.tex");
    assert!(e.preview(Mode::Pull).mass_delete.is_none());
}

// 14
#[test]
fn names_windows_cannot_hold_and_case_twins_are_left_alone() {
    let e = env(&[("main.tex", DOC)]);
    e.remote.put("aux.tex", b"x");
    e.remote.put("Dup.tex", b"d");
    e.remote.put("dup.tex", b"d2");
    let p = e.preview(Mode::Sync);
    let skipped: BTreeSet<_> = p.needs_attention.iter().map(|s| s.path.as_str()).collect();
    assert!(skipped.contains("aux.tex"), "{skipped:?}");
    e.run(Mode::Sync);
    assert!(e.read("aux.tex").is_none());
    // A second sync does not treat the skipped names as deletions or re-report changes.
    assert!(e.preview(Mode::Sync).items.is_empty());
    assert!(e.remote.text("aux.tex").is_some(), "never deleted on Overleaf");
}

#[test]
fn a_local_file_that_differs_only_by_case_from_an_overleaf_file_is_not_uploaded() {
    let e = env(&[("main.tex", DOC)]);
    e.remote.put("Notes.tex", b"remote\n");
    e.write("notes.tex", "local\n");
    let p = e.preview(Mode::Push);
    assert!(p.items.iter().all(|i| i.path != "notes.tex"));
    assert!(p.needs_attention.iter().any(|s| s.path == "notes.tex"));
}

// 15
#[test]
fn crlf_line_endings_are_not_a_change() {
    let e = env(&[("main.tex", "a\nb\n")]);
    fs::write(e.dir.path().join("main.tex"), "a\r\nb\r\n").unwrap();
    assert!(local_changes(&e.project).unwrap().is_empty());
    assert!(e.preview(Mode::Sync).items.is_empty());

    fs::write(e.dir.path().join("main.tex"), "a\r\nB\r\n").unwrap();
    e.run(Mode::Push);
    assert_eq!(e.remote.text("main.tex").as_deref(), Some("a\nB\n"));
}

// 16
#[test]
fn ignored_files_never_take_part() {
    let e = env(&[("main.tex", DOC)]);
    e.write("main.aux", "junk");
    e.write("main.synctex.gz", "junk");
    e.write("sub/notes.log", "junk");
    assert!(e.preview(Mode::Sync).items.is_empty());
    e.remote.put("remote.aux", b"junk");
    assert!(e.preview(Mode::Sync).items.is_empty());
    e.run(Mode::Sync);
    assert!(e.remote.text("main.aux").is_none());
    assert!(e.read("remote.aux").is_none());
}

// 17
#[test]
fn nested_new_folders_push_and_empty_folders_disappear_on_pull() {
    let tmp = tempfile::tempdir().unwrap();
    let folder = FolderRemote::new(tmp.path().join("practice")).unwrap();
    let e = env(&[("main.tex", DOC)]);
    folder.upload("main.tex", DOC.as_bytes()).unwrap();
    e.write("chapters/deep/one.tex", "one\n");
    e.run_with(&folder, Mode::Push);
    assert!(tmp.path().join("practice/chapters/deep/one.tex").exists());

    folder.delete("chapters/deep/one.tex").unwrap();
    e.run_with(&folder, Mode::Pull);
    assert!(!e.dir.path().join("chapters").exists(), "empty folders are cleaned up");
}

// 18, 19
#[test]
fn a_rename_on_overleaf_moves_the_local_file() {
    let e = env(&[("main.tex", DOC), ("chapters/a.tex", "content of a\n")]);
    e.remote.remove("chapters/a.tex");
    e.remote.put("sections/a.tex", b"content of a\n");
    let p = e.preview(Mode::Pull);
    assert_eq!(p.items.len(), 1, "{:?}", p.items);
    assert_eq!(p.items[0].op, Op::Rename);
    assert_eq!(p.items[0].from.as_deref(), Some("chapters/a.tex"));
    e.run(Mode::Pull);
    assert_eq!(e.read("sections/a.tex").as_deref(), Some("content of a\n"));
    assert!(e.read("chapters/a.tex").is_none());
    assert!(e.in_sync());
}

#[test]
fn a_local_rename_is_sent_as_a_rename_not_a_delete_and_upload() {
    let e = env(&[("main.tex", DOC), ("old.tex", "keep my history\n")]);
    fs::rename(e.dir.path().join("old.tex"), e.dir.path().join("new.tex")).unwrap();
    let faulty = Faulty::new(&e.remote);
    e.run_with(&faulty, Mode::Push);
    assert_eq!(faulty.calls(), vec!["rename old.tex new.tex".to_string()]);
    assert_eq!(e.remote.text("new.tex").as_deref(), Some("keep my history\n"));
    assert!(e.remote.text("old.tex").is_none());
    assert!(e.in_sync());
}

#[test]
fn a_rename_on_one_side_with_an_edit_on_the_other_is_a_conflict_not_a_loss() {
    let e = env(&[("main.tex", DOC), ("a.tex", "one\ntwo\n")]);
    e.remote.remove("a.tex");
    e.remote.put("b.tex", b"one\ntwo\n");
    e.write("a.tex", "one\ntwo\nlocal edit\n");
    let p = e.preview(Mode::Sync);
    // Overleaf's "delete a + add b" meets a local edit of a: a conflict on a, plus b arrives.
    assert!(p.conflicts.iter().any(|c| c.path == "a.tex" && c.kind == ConflictKind::ModifiedDeleted));
    assert!(p.items.iter().any(|i| i.path == "b.tex"));
    assert!(e.read("a.tex").unwrap().contains("local edit"));
}

#[test]
fn empty_files_are_never_paired_as_renames() {
    let e = env(&[("main.tex", DOC), ("one.tex", ""), ("two.tex", "")]);
    e.remote.remove("one.tex");
    e.remote.put("three.tex", b"");
    let p = e.preview(Mode::Pull);
    assert!(p.items.iter().all(|i| i.op != Op::Rename), "{:?}", p.items);
}

// 22
#[test]
fn pull_leaves_local_only_changes_alone_and_does_not_pretend_a_merge_is_synced() {
    let e = env(&[("main.tex", DOC), ("mine.tex", "m\n"), ("both.tex", "1\n2\n3\n4\n5\n6\n7\n")]);
    e.write("mine.tex", "m edited\n");
    e.write("both.tex", "ONE\n2\n3\n4\n5\n6\n7\n");
    e.remote.put("both.tex", b"1\n2\n3\n4\n5\n6\nSEVEN\n");
    e.remote.put("theirs.tex", b"t\n");

    let out = e.run(Mode::Pull);
    assert!(out.complete);
    assert_eq!(e.read("theirs.tex").as_deref(), Some("t\n"));
    assert_eq!(e.read("mine.tex").as_deref(), Some("m edited\n"));
    assert_eq!(e.remote.text("mine.tex").as_deref(), Some("m\n"), "Pull never sends");
    assert_eq!(e.read("both.tex").as_deref(), Some("ONE\n2\n3\n4\n5\n6\nSEVEN\n"));
    assert_eq!(e.remote.text("both.tex").as_deref(), Some("1\n2\n3\n4\n5\n6\nSEVEN\n"));
    assert_eq!(e.base("both.tex").as_deref(), Some("1\n2\n3\n4\n5\n6\n7\n"), "Base only moves when both sides agree");

    // A later Sync converges without a conflict.
    let p = e.preview(Mode::Sync);
    assert!(p.conflicts.is_empty());
    e.run(Mode::Sync);
    assert_eq!(e.remote.text("both.tex").as_deref(), Some("ONE\n2\n3\n4\n5\n6\nSEVEN\n"));
    assert_eq!(e.remote.text("mine.tex").as_deref(), Some("m edited\n"));
    assert!(e.in_sync());
}

// 23
#[test]
fn push_skips_files_overleaf_also_changed_but_sends_the_rest() {
    let e = env(&[("main.tex", DOC), ("clash.tex", "1\n2\n3\n4\n5\n6\n7\n"), ("fine.tex", "f\n")]);
    e.write("clash.tex", "ONE\n2\n3\n4\n5\n6\n7\n");
    e.remote.put("clash.tex", b"1\n2\n3\n4\n5\n6\nSEVEN\n");
    e.write("fine.tex", "f edited\n");
    e.remote.put("theirs-only.tex", b"t\n");

    let p = e.preview(Mode::Push);
    let clash = p.items.iter().find(|i| i.path == "clash.tex").unwrap();
    assert!(!clash.applies);
    assert!(clash.note.as_deref().unwrap().contains("Pull first"));
    assert!(p.conflicts.is_empty(), "a blocked file is not asked about");
    let out = e.run(Mode::Push);
    assert_eq!(out.pushed, 1);
    assert_eq!(e.remote.text("fine.tex").as_deref(), Some("f edited\n"));
    assert_eq!(e.remote.text("clash.tex").as_deref(), Some("1\n2\n3\n4\n5\n6\nSEVEN\n"));
    assert!(e.read("theirs-only.tex").is_none(), "Push never pulls");
}

// 24
#[test]
fn a_deleted_root_file_is_warned_about() {
    let e = env(&[("main.tex", DOC), ("a.tex", "a\n")]);
    e.remote.remove("main.tex");
    let p = e.preview(Mode::Pull);
    assert!(p.warnings.iter().any(|w| w.contains("main.tex")), "{:?}", p.warnings);
}

// 26
#[test]
fn resolved_text_with_conflict_markers_is_refused_unless_confirmed() {
    let e = env(&[("main.tex", "base\n")]);
    e.write("main.tex", "MINE\n");
    e.remote.put("main.tex", b"THEIRS\n");
    let p = e.preview(Mode::Sync);
    let mut req = e.request(&p);
    resolve_all(&p, &mut req, FileChoice::Text { text: "<<<<<<< mine\nMINE\n=======\nTHEIRS\n>>>>>>> theirs\n".into() });
    assert!(e.apply_with(&e.remote, &req).is_err());
    req.allow_markers = true;
    assert!(e.apply_with(&e.remote, &req).is_ok());
}

// 27
#[test]
fn the_log_shows_what_a_sync_did_and_one_undo_goes_back_before_the_pull() {
    let e = env(&[("main.tex", DOC), ("a.tex", "a\n")]);
    e.write("a.tex", "a typed locally\n");
    e.remote.put("main.tex", b"line1\nline2\nline3\nline4\nEDITED ON OVERLEAF\n");
    let out = e.run(Mode::Sync);
    assert!(out.complete);

    let history = History::new(e.dir.path());
    let kinds: Vec<_> = history.list().unwrap().iter().map(|v| (v.kind, v.author.clone())).collect();
    assert!(kinds.contains(&(VersionKind::Auto, "You".to_string())), "{kinds:?}");
    assert!(kinds.contains(&(VersionKind::Pull, "Overleaf".to_string())), "{kinds:?}");
    assert!(kinds.contains(&(VersionKind::Push, "You".to_string())), "{kinds:?}");
    let pull = history.list().unwrap().into_iter().find(|v| v.kind == VersionKind::Pull).unwrap();
    assert_eq!(pull.stats.files, 1);

    history.undo().unwrap().unwrap();
    assert_eq!(e.read("main.tex").as_deref(), Some(DOC), "the pull is undone");
    assert_eq!(e.read("a.tex").as_deref(), Some("a typed locally\n"), "typed edits are kept");
}

// 30
#[test]
fn a_project_that_was_never_synced_deletes_nothing_and_asks_about_differences() {
    let dir = tempfile::tempdir().unwrap();
    write_text(dir.path(), "main.tex", "local main\n").unwrap();
    write_text(dir.path(), "only-here.tex", "h\n").unwrap();
    write_text(dir.path(), "same.tex", "same\n").unwrap();
    let project = Project::init(dir.path(), "Test", "main.tex").unwrap();
    project
        .link_overleaf(&OverleafLink { project_id: "p".into(), name: "T".into(), last_synced: None })
        .unwrap();
    // No Base snapshot.
    let remote = MemoryRemote::with(&[("main.tex", "remote main\n"), ("only-there.tex", "t\n"), ("same.tex", "same\n")]);

    let (p, _) = Syncer::new(&project, &remote).preview(Mode::Sync).unwrap();
    assert_eq!(p.conflicts.len(), 1);
    assert_eq!(p.conflicts[0].path, "main.tex");
    assert!(p.warnings.iter().any(|w| w.contains("not been synced")));
    assert!(p.items.iter().all(|i| i.op != Op::Delete));
    let push = p.items.iter().find(|i| i.path == "only-here.tex").unwrap();
    assert_eq!(push.direction, Direction::Push);
    let pull = p.items.iter().find(|i| i.path == "only-there.tex").unwrap();
    assert_eq!(pull.direction, Direction::Pull);
}

#[test]
fn unticked_files_are_left_alone_for_this_run() {
    let e = env(&[("main.tex", DOC)]);
    e.write("a.tex", "a\n");
    e.write("b.tex", "b\n");
    let p = e.preview(Mode::Push);
    let mut req = e.request(&p);
    req.skip = vec!["b.tex".into()];
    done(e.apply_with(&e.remote, &req).unwrap());
    assert!(e.remote.text("a.tex").is_some());
    assert!(e.remote.text("b.tex").is_none());
    assert_eq!(local_changes(&e.project).unwrap().len(), 1);
}

#[test]
fn the_staged_files_answer_what_each_side_holds() {
    let e = env(&[("main.tex", "base\n")]);
    e.write("main.tex", "mine\n");
    e.remote.put("main.tex", b"theirs\n");
    let (_, staged) = Syncer::new(&e.project, &e.remote).preview(Mode::Sync).unwrap();
    let v = staged.views("main.tex");
    assert_eq!(v.base.text.as_deref(), Some("base\n"));
    assert_eq!(v.mine.text.as_deref(), Some("mine\n"));
    assert_eq!(v.theirs.text.as_deref(), Some("theirs\n"));
    assert!(!staged.views("missing.tex").mine.exists);
}

#[test]
fn local_changes_list_added_modified_and_deleted_against_base() {
    let e = env(&[("main.tex", DOC), ("gone.tex", "g\n"), ("edit.tex", "e\n")]);
    e.write("new.tex", "n\n");
    e.write("edit.tex", "e2\n");
    e.delete("gone.tex");
    let changes = local_changes(&e.project).unwrap();
    let summary: Vec<_> = changes.iter().map(|c| (c.path.as_str(), format!("{:?}", c.kind))).collect();
    assert_eq!(
        summary,
        vec![
            ("edit.tex", "Modified".to_string()),
            ("gone.tex", "Deleted".to_string()),
            ("new.tex", "Added".to_string()),
        ]
    );
}

#[test]
fn works_the_same_against_a_folder_acting_as_overleaf() {
    let tmp = tempfile::tempdir().unwrap();
    let folder = FolderRemote::new(tmp.path().join("practice")).unwrap();
    let e = env(&[("main.tex", DOC)]);
    folder.upload("main.tex", DOC.as_bytes()).unwrap();

    e.write("main.tex", "LOCAL\nline2\nline3\nline4\nline5\n");
    folder.upload("main.tex", b"line1\nline2\nline3\nline4\nREMOTE\n").unwrap();
    folder.upload("figs/a.png", &[0u8, 1, 2]).unwrap();
    let out = e.run_with(&folder, Mode::Sync);
    assert!(out.complete);
    let merged = "LOCAL\nline2\nline3\nline4\nREMOTE\n";
    assert_eq!(fs::read_to_string(tmp.path().join("practice/main.tex")).unwrap(), merged);
    assert_eq!(e.read("main.tex").as_deref(), Some(merged));
    assert_eq!(fs::read(e.dir.path().join("figs/a.png")).unwrap(), [0u8, 1, 2]);
    assert!(e.in_sync());
}

#[test]
fn a_second_sync_on_the_same_project_is_refused_until_the_first_finishes() {
    let e = env(&[("main.tex", DOC)]);
    e.write("main.tex", "LOCAL\nline2\nline3\nline4\nline5\n");
    let p = e.preview(Mode::Push);

    // Something else (the app, or an agent) is part way through applying a sync.
    let held = super::lock::SyncLock::acquire(e.dir.path()).unwrap();
    let busy = e.apply_with(&e.remote, &e.request(&p)).unwrap_err().to_string();
    assert!(busy.contains("Another sync is already running"), "{busy}");
    assert_eq!(e.remote.text("main.tex").as_deref(), Some(DOC), "nothing was uploaded");
    assert!(e.preview(Mode::Push).items.len() == 1, "a Review is still allowed while one runs");

    drop(held);
    assert!(matches!(e.apply_with(&e.remote, &e.request(&p)).unwrap(), Applied::Done { .. }));
    assert_eq!(e.remote.text("main.tex").as_deref(), Some("LOCAL\nline2\nline3\nline4\nline5\n"));
}
