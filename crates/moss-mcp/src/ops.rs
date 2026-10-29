//! What each MCP tool does, as plain blocking functions over `moss-core`.
//! `server.rs` only adapts these to MCP, so everything here is testable without a client.

use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use moss_core::approvals::{APPROVAL_TIMEOUT, Approvals};
use moss_core::compile::{self, CompileRequest};
use moss_core::fsutil;
use moss_core::external;
use moss_core::history::{AUTHOR_AGENT, AUTHOR_EXTERNAL, History, Rev, VersionKind};
use moss_core::overleaf;
use moss_core::search::{self, SearchOptions};
use moss_core::project::{OverleafLink, Project};
use moss_core::{outline, pdfview, synctex, templates};
use moss_core::sync::link;
use moss_core::sync::remote::Remote;
use moss_core::sync::{
    self, Applied, ApplyRequest, Conflict, FileChoice, Mode, Op, Preview, Resolution, Syncer,
};
use moss_core::textdiff::{Choice, Region};
use moss_core::tex::{self, Engine, TexInstall};
use moss_core::texcheck;
use moss_core::workspace::Workspace;
use moss_core::audit;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

pub type OpResult = Result<Value, String>;

/// A tool result that is a description plus pictures (rendered pages, a figure).
pub struct Media {
    pub info: Value,
    pub images: Vec<Image>,
}

pub struct Image {
    pub bytes: Vec<u8>,
    pub mime: &'static str,
    /// Pixels; 0 when not known (a figure file sent as it is).
    pub width: u32,
    pub height: u32,
}

impl Image {
    fn png(raster: &pdfview::Raster, bytes: Vec<u8>) -> Image {
        Image { bytes, mime: "image/png", width: raster.width, height: raster.height }
    }
}

pub type MediaResult = Result<Media, String>;

/// What to draw for `render_pages`.
pub struct RenderArgs<'a> {
    pub pages: &'a str,
    /// `one_image`, `separate` or empty for "one image when there are several pages".
    pub layout: &'a str,
    pub columns: Option<usize>,
    pub dpi: Option<u32>,
    /// Compile first when the PDF is missing or older than the sources.
    pub rebuild: bool,
}

const MAX_SEPARATE_IMAGES: usize = 12;
/// Default and largest output of a read, in characters (about 15k tokens by default).
const READ_BUDGET: usize = 60_000;
const MAX_READ_BUDGET: usize = 200_000;
const MAX_READ_PATHS: usize = 20;
const MAX_BATCH_FILES: usize = 50;
const MAX_DELETE_PATHS: usize = 100;
const ACTOR: &str = "moss-mcp";

/// One file for `write_files`.
#[derive(Deserialize, JsonSchema)]
pub struct FileSpec {
    /// File path relative to the Project. Missing folders are created.
    pub path: String,
    /// The complete new content of the file.
    pub content: String,
}

/// One replacement for `edit_file`.
#[derive(Deserialize, JsonSchema)]
pub struct EditSpec {
    /// Exact existing text to replace. Must match once unless `replace_all` is set.
    pub old_text: String,
    /// Replacement text.
    pub new_text: String,
    /// Replace every occurrence instead of requiring a unique match.
    pub replace_all: Option<bool>,
}

/// A run of whole lines of a file.
struct Slice {
    content: String,
    start_line: usize,
    end_line: usize,
    total_lines: usize,
    /// Stopped before the requested end because of the output budget.
    truncated: bool,
    /// The first line alone was longer than the budget and was cut short.
    line_cut: bool,
}

impl Slice {
    fn into_json(self, path: &str) -> Value {
        let mut v = json!({
            "path": path,
            "content": self.content,
            "start_line": self.start_line,
            "end_line": self.end_line,
            "total_lines": self.total_lines,
            "truncated": self.truncated,
        });
        if self.truncated {
            v["next_start_line"] = json!(self.end_line + 1);
        }
        if self.line_cut {
            v["line_cut"] = json!(true);
        }
        v
    }
}

/// Lines `start..=end` (1-based) of `text`, stopping on a line boundary once `budget` characters
/// are used. At least one line is always returned.
fn slice_lines(text: &str, start: Option<usize>, end: Option<usize>, budget: usize) -> Result<Slice, String> {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let total = lines.len();
    let start = start.unwrap_or(1).max(1);
    if total == 0 {
        return Ok(Slice { content: String::new(), start_line: 1, end_line: 0, total_lines: 0, truncated: false, line_cut: false });
    }
    if start > total {
        return Err(format!("the file has {total} lines; start_line {start} is past the end"));
    }
    let end = end.unwrap_or(total).min(total);
    if end < start {
        return Err(format!("end_line {end} is before start_line {start}"));
    }
    let mut content = String::new();
    let mut used = 0;
    let mut last = start - 1;
    let mut line_cut = false;
    for line in &lines[start - 1..end] {
        let len = line.chars().count();
        if used + len > budget {
            if last == start - 1 {
                content.extend(line.chars().take(budget));
                last = start;
                line_cut = true;
            }
            break;
        }
        content.push_str(line);
        used += len;
        last += 1;
    }
    Ok(Slice { content, start_line: start, end_line: last, total_lines: total, truncated: last < end, line_cut })
}

/// A version number, `working` (the files now) or `empty` (no files).
fn parse_rev(text: &str) -> Result<Rev, String> {
    match text.trim().to_ascii_lowercase().as_str() {
        "working" | "current" | "now" => Ok(Rev::Working),
        "empty" | "none" => Ok(Rev::Empty),
        other => other
            .trim_start_matches('#')
            .parse::<u64>()
            .map(|id| Rev::Snapshot { id })
            .map_err(|_| format!("`{text}` is not a version number, `working` or `empty`")),
    }
}

/// What a write did to one LaTeX file: size change, and any balance problem it introduced.
fn integrity(path: &str, old: &str, new: &str) -> Value {
    let mut v = json!({
        "lines_before": old.lines().count(),
        "lines_after": new.lines().count(),
        "bytes_before": old.len(),
        "bytes_after": new.len(),
    });
    if texcheck::is_latex(path) {
        let problems = texcheck::new_problems(old, new);
        if !problems.is_empty() {
            v["warnings"] = json!(problems);
        }
    }
    v
}

/// "a, b, c and 4 more", for restore point labels and Approval prompts.
fn summarize<'a>(paths: impl Iterator<Item = &'a str>) -> String {
    let all: Vec<&str> = paths.collect();
    if all.len() <= 5 {
        all.join(", ")
    } else {
        format!("{} and {} more", all[..5].join(", "), all.len() - 5)
    }
}
const MAX_SEARCH_HITS: usize = 200;
const MAX_DIAGNOSTICS: usize = 60;
const MAX_WORST_BOXES: usize = 10;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

fn to_value<T: serde::Serialize>(v: &T) -> OpResult {
    serde_json::to_value(v).map_err(err)
}

/// What one tool call found out about its Project while it ran.
#[derive(Default)]
struct Call {
    active: bool,
    /// The Project the call touched first, and what changed in it before the call began.
    dir: Option<PathBuf>,
    changed: Option<Value>,
}

thread_local! {
    /// Tool calls run on one blocking thread each, so the call's state can live here.
    static CALL: RefCell<Call> = RefCell::new(Call::default());
}

pub struct Ops {
    workspace: Workspace,
    tex_bin_dir: Option<PathBuf>,
    tex: Mutex<Option<TexInstall>>,
    approval_timeout: Duration,
}

impl Ops {
    pub fn new(workspace: Workspace, tex_bin_dir: Option<PathBuf>) -> Ops {
        Ops {
            workspace,
            tex_bin_dir,
            tex: Mutex::new(None),
            approval_timeout: APPROVAL_TIMEOUT,
        }
    }

    #[cfg(test)]
    fn with_approval_timeout(mut self, timeout: Duration) -> Ops {
        self.approval_timeout = timeout;
        self
    }

    fn open(&self, project: &str) -> Result<Project, String> {
        let p = self.workspace.open_project(project).map_err(err)?;
        self.notice_outside_changes(&p);
        Ok(p)
    }

    /// The first time a tool call opens a Project, compare its files with what the previous call
    /// left. Anything different was written by something else (the app, another agent): keep it as
    /// a restore point, log it, and let the call's answer say so.
    fn notice_outside_changes(&self, p: &Project) {
        let first = CALL.with(|c| {
            let mut c = c.borrow_mut();
            let first = c.active && c.dir.is_none();
            if first {
                c.dir = Some(p.dir().to_path_buf());
            }
            first
        });
        if !first {
            return;
        }
        let Ok(Some(drift)) = external::check(p.dir()) else { return };
        let label = format!("Changed outside the Agent tools: {}", drift.summary());
        let id = History::new(p.dir())
            .capture_as(VersionKind::External, AUTHOR_EXTERNAL, &label)
            .ok()
            .flatten()
            .map(|v| v.id);
        let _ = audit::record(p.dir(), ACTOR, "external_change", &drift.summary());
        let note = json!({
            "message": format!(
                "{} file(s) changed since your last call, and not through these tools (the user typing in Moss, or another program). Re-read anything you rely on before editing it.",
                drift.file_count()
            ),
            "restore_point": id,
            "modified": drift.modified,
            "added": drift.added,
            "deleted": drift.deleted,
        });
        CALL.with(|c| c.borrow_mut().changed = Some(note));
    }

    /// Run one tool call. Afterwards the Project's files are remembered, so the next call can tell
    /// what changed in between, and anything found at the start rides along in the answer.
    pub fn tracked(&self, work: impl FnOnce(&Ops) -> OpResult) -> OpResult {
        CALL.with(|c| *c.borrow_mut() = Call { active: true, ..Call::default() });
        let result = work(self);
        let (dir, changed) = CALL.with(|c| {
            let mut c = c.borrow_mut();
            c.active = false;
            (c.dir.take(), c.changed.take())
        });
        if let Some(dir) = dir {
            let _ = external::mark_seen(&dir);
        }
        let Some(changed) = changed else { return result };
        match result {
            Ok(mut value) => {
                match value.as_object_mut() {
                    Some(map) => {
                        map.insert("project_changed_outside".into(), changed);
                    }
                    None => value = json!({ "result": value, "project_changed_outside": changed }),
                }
                Ok(value)
            }
            Err(e) => Err(format!("{e}
(Also: {})", changed["message"].as_str().unwrap_or_default())),
        }
    }

    /// `tracked` for tools that answer with pictures.
    pub fn tracked_media(&self, work: impl FnOnce(&Ops) -> MediaResult) -> MediaResult {
        let mut media = None;
        let outcome = self.tracked(|o| {
            media = Some(work(o)?);
            Ok(Value::Null)
        });
        match (outcome, media) {
            (Ok(note), Some(mut media)) => {
                if let Some(changed) = note.get("project_changed_outside") {
                    media.info["project_changed_outside"] = changed.clone();
                }
                Ok(media)
            }
            (Err(e), _) => Err(e),
            (Ok(_), None) => Err("tool produced no result".into()),
        }
    }

    /// Keep anything the user typed as a Version, then record the change in the audit log.
    fn before_change(&self, p: &Project, action: &str, detail: &str) -> Result<(), String> {
        History::new(p.dir()).capture("Your changes").map_err(err)?;
        audit::record(p.dir(), ACTOR, action, detail).map_err(err)
    }

    /// Record what the Agent just did as one Version, so it shows in the History tab.
    fn after_change(&self, p: &Project, action: &str, detail: &str) -> Result<(), String> {
        History::new(p.dir())
            .capture_as(VersionKind::Agent, AUTHOR_AGENT, &format!("{action}: {detail}"))
            .map(|_| ())
            .map_err(err)
    }

    /// Wait for the user's click unless the Project is trusted.
    fn approve(&self, project: &str, p: &Project, action: &str, detail: &str) -> Result<(), String> {
        if p.meta.trusted {
            return Ok(());
        }
        let queue = Approvals::new(self.workspace.root());
        let request = queue.request(project, action, detail).map_err(err)?;
        eprintln!("moss-mcp: waiting for approval in the Moss app: {action} ({detail})");
        if queue.wait(&request.id, self.approval_timeout).map_err(err)? {
            Ok(())
        } else {
            Err(format!(
                "The user did not approve `{action}` ({detail}). Nothing was changed."
            ))
        }
    }

    // ----- Projects -----

    pub fn list_projects(&self) -> OpResult {
        let mut projects = to_value(&self.workspace.list_projects().map_err(err)?)?;
        // When the sources last changed, to tell a scratch copy from the real thing.
        for project in projects.as_array_mut().into_iter().flatten() {
            let modified = project["dir_name"]
                .as_str()
                .and_then(|dir| self.workspace.open_project(dir).ok())
                .and_then(|p| fsutil::newest_modified(p.dir()))
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs());
            project["last_modified"] = json!(modified);
        }
        Ok(projects)
    }

    /// A new Local Project, from a starter document when `template` is given (see `list_templates`).
    pub fn create_project(&self, name: &str, template: Option<&str>) -> OpResult {
        to_value(&self.workspace.create_project_from(name, template).map_err(err)?)
    }

    /// Import a zip file or a folder from this PC as a new Local Project.
    pub fn import(&self, path: &str, name: Option<&str>) -> OpResult {
        let src = Path::new(path);
        let report = if src.is_dir() {
            self.workspace.import_folder(src, name)
        } else {
            self.workspace.import_zip(src, name)
        }
        .map_err(err)?;
        to_value(&report)
    }

    pub fn project_info(&self, project: &str) -> OpResult {
        let p = self.open(project)?;
        let history = History::new(p.dir()).list().map_err(err)?;
        Ok(json!({
            "name": p.meta.name,
            "folder": project,
            "path": p.dir().display().to_string(),
            "origin": p.origin(),
            "overleaf": p.overleaf_link(),
            "root_file": p.meta.root_file,
            "engine": p.meta.engine,
            "shell_escape": p.meta.shell_escape,
            "trusted": p.meta.trusted,
            "history_snapshots": history.len(),
        }))
    }

    /// Change the Root file or engine. `engine` is `auto`, `pdflatex`, `xelatex` or `lualatex`.
    pub fn set_compile_options(
        &self,
        project: &str,
        root_file: Option<&str>,
        engine: Option<&str>,
    ) -> OpResult {
        let mut p = self.open(project)?;
        if let Some(root) = root_file {
            if !fsutil::safe_join(p.dir(), root).map_err(err)?.is_file() {
                return Err(format!("`{root}` is not a file in this project"));
            }
            p.meta.root_file = root.to_string();
        }
        if let Some(engine) = engine {
            p.meta.engine = match engine {
                "auto" => None,
                "pdflatex" => Some(Engine::Pdflatex),
                "xelatex" => Some(Engine::Xelatex),
                "lualatex" => Some(Engine::Lualatex),
                other => return Err(format!("unknown engine `{other}`")),
            };
        }
        p.save_meta().map_err(err)?;
        audit::record(p.dir(), ACTOR, "set_compile_options", &format!("{root_file:?} {engine:?}"))
            .map_err(err)?;
        Ok(json!({ "root_file": p.meta.root_file, "engine": p.meta.engine }))
    }

    /// Turning shell-escape on lets a document run programs, so it needs an Approval.
    pub fn set_shell_escape(&self, project: &str, enabled: bool) -> OpResult {
        let mut p = self.open(project)?;
        if enabled {
            self.approve(project, &p, "set_shell_escape", "allow documents to run programs (shell-escape)")?;
        }
        p.meta.shell_escape = enabled;
        p.save_meta().map_err(err)?;
        audit::record(p.dir(), ACTOR, "set_shell_escape", &enabled.to_string()).map_err(err)?;
        Ok(json!({ "shell_escape": enabled }))
    }

    // ----- Files -----

    pub fn list_files(&self, project: &str) -> OpResult {
        let p = self.open(project)?;
        to_value(&fsutil::list_files(p.dir()).map_err(err)?)
    }

    /// Read one file, whole or by line range. Output is capped at `max_chars` on a line boundary
    /// and says where to continue (`next_start_line`) so a long file can be read part by part.
    pub fn read_file(
        &self,
        project: &str,
        path: &str,
        start_line: Option<usize>,
        end_line: Option<usize>,
        max_chars: Option<usize>,
    ) -> OpResult {
        let p = self.open(project)?;
        let text = fsutil::read_text(p.dir(), path).map_err(err)?;
        let budget = max_chars.unwrap_or(READ_BUDGET).clamp(1_000, MAX_READ_BUDGET);
        let slice = slice_lines(&text, start_line, end_line, budget)?;
        Ok(slice.into_json(path))
    }

    /// Read several files in one call, sharing one output budget. A file that cannot be read
    /// reports its own error; the others still come back.
    pub fn read_files(&self, project: &str, paths: &[String], max_chars: Option<usize>) -> OpResult {
        if paths.is_empty() {
            return Err("`paths` is empty".into());
        }
        if paths.len() > MAX_READ_PATHS {
            return Err(format!("at most {MAX_READ_PATHS} files per call; ask for the rest in another call"));
        }
        let p = self.open(project)?;
        let mut left = max_chars.unwrap_or(READ_BUDGET).clamp(1_000, MAX_READ_BUDGET);
        let mut files = Vec::new();
        for path in paths {
            if left == 0 {
                files.push(json!({ "path": path, "skipped": "output budget used up; read it in another call" }));
                continue;
            }
            let entry = match fsutil::read_text(p.dir(), path) {
                Err(e) => json!({ "path": path, "error": e.to_string() }),
                Ok(text) => match slice_lines(&text, None, None, left) {
                    Err(e) => json!({ "path": path, "error": e }),
                    Ok(slice) => {
                        left = left.saturating_sub(slice.content.chars().count());
                        slice.into_json(path)
                    }
                },
            };
            files.push(entry);
        }
        Ok(json!({ "files": files }))
    }

    pub fn write_file(&self, project: &str, path: &str, content: &str) -> OpResult {
        let p = self.open(project)?;
        if !fsutil::would_change(p.dir(), path, content).map_err(err)? {
            return Ok(json!({ "path": path, "changed": false }));
        }
        let old = fsutil::read_text(p.dir(), path).unwrap_or_default();
        self.before_change(&p, "write_file", path)?;
        fsutil::write_text(p.dir(), path, content).map_err(err)?;
        self.after_change(&p, "write_file", path)?;
        Ok(json!({ "path": path, "changed": true, "integrity": integrity(path, &old, content) }))
    }

    /// Write several files as one change: one restore point before, one Version after. Every
    /// path is checked first, and files whose content is already identical are left alone.
    pub fn write_files(&self, project: &str, files: &[FileSpec]) -> OpResult {
        if files.is_empty() {
            return Err("`files` is empty".into());
        }
        if files.len() > MAX_BATCH_FILES {
            return Err(format!("at most {MAX_BATCH_FILES} files per call; send the rest in another call"));
        }
        let p = self.open(project)?;
        let mut seen = std::collections::HashSet::new();
        let mut changing = Vec::new();
        let mut warnings: Vec<String> = Vec::new();
        for f in files {
            if !seen.insert(f.path.as_str()) {
                return Err(format!("`{}` appears twice in `files`", f.path));
            }
            if fsutil::would_change(p.dir(), &f.path, &f.content).map_err(err)? {
                changing.push(f);
            }
        }
        if !changing.is_empty() {
            let detail = summarize(changing.iter().map(|f| f.path.as_str()));
            self.before_change(&p, "write_files", &detail)?;
            for f in &changing {
                let old = fsutil::read_text(p.dir(), &f.path).unwrap_or_default();
                warnings.extend(texcheck::new_problems(&old, &f.content).into_iter().map(|w| format!("{}: {w}", f.path)));
                fsutil::write_text(p.dir(), &f.path, &f.content).map_err(err)?;
            }
            self.after_change(&p, "write_files", &detail)?;
        }
        let result: Vec<Value> = files
            .iter()
            .map(|f| json!({ "path": f.path, "changed": changing.iter().any(|c| c.path == f.path) }))
            .collect();
        let mut value = json!({ "changed": changing.len(), "files": result });
        if !warnings.is_empty() {
            value["integrity_warnings"] = json!(warnings);
        }
        Ok(value)
    }

    /// Apply `edits` to one file in order, each against the text the earlier ones left. All or
    /// nothing: if any edit fails, the file is not touched. One restore point for the whole call.
    pub fn edit_file(&self, project: &str, path: &str, edits: &[EditSpec]) -> OpResult {
        if edits.is_empty() {
            return Err("give at least one edit".into());
        }
        let p = self.open(project)?;
        let current = fsutil::read_text(p.dir(), path).map_err(err)?;
        let mut text = current.clone();
        let mut counts = Vec::new();
        for (i, edit) in edits.iter().enumerate() {
            let n = i + 1;
            if edit.old_text.is_empty() {
                return Err(format!("edit {n}: `old_text` must not be empty"));
            }
            // Files are stored with LF endings, so match against LF whatever the caller sent.
            let old = edit.old_text.replace("\r\n", "\n");
            let new = edit.new_text.replace("\r\n", "\n");
            let count = text.matches(old.as_str()).count();
            if count == 0 {
                return Err(format!("edit {n}: `old_text` was not found in {path}. Nothing was changed."));
            }
            if count > 1 && !edit.replace_all.unwrap_or(false) {
                return Err(format!(
                    "edit {n}: `old_text` appears {count} times in {path}; add more surrounding text to make it unique, or set replace_all. Nothing was changed."
                ));
            }
            text = text.replace(old.as_str(), &new);
            counts.push(count);
        }
        let replacements: usize = counts.iter().sum();
        if text == current {
            return Ok(json!({ "path": path, "replacements": replacements, "changed": false }));
        }
        self.before_change(&p, "edit_file", path)?;
        fsutil::write_text(p.dir(), path, &text).map_err(err)?;
        self.after_change(&p, "edit_file", path)?;
        Ok(json!({
            "path": path,
            "replacements": replacements,
            "per_edit": counts,
            "changed": true,
            "integrity": integrity(path, &current, &text),
        }))
    }

    /// Delete files or folders as one change with one Approval. Every path must exist.
    pub fn delete_files(&self, project: &str, paths: &[String]) -> OpResult {
        if paths.is_empty() {
            return Err("give at least one path to delete".into());
        }
        if paths.len() > MAX_DELETE_PATHS {
            return Err(format!("at most {MAX_DELETE_PATHS} paths per call; delete the rest in another call"));
        }
        let p = self.open(project)?;
        let mut targets = Vec::new();
        let mut missing = Vec::new();
        for path in paths {
            let target = fsutil::safe_join(p.dir(), path).map_err(err)?;
            if target.exists() {
                targets.push(target);
            } else {
                missing.push(path.as_str());
            }
        }
        if !missing.is_empty() {
            return Err(format!("does not exist: {}. Nothing was deleted.", missing.join(", ")));
        }
        let detail = summarize(paths.iter().map(String::as_str));
        self.approve(project, &p, "delete_file", &format!("delete {detail}"))?;
        self.before_change(&p, "delete_file", &detail)?;
        for target in &targets {
            // A folder and a file inside it may both be listed; the first removal covers both.
            if target.is_dir() {
                fs::remove_dir_all(target).map_err(err)?;
            } else if target.exists() {
                fs::remove_file(target).map_err(err)?;
            }
        }
        self.after_change(&p, "delete_file", &detail)?;
        Ok(json!({ "deleted": paths, "undo": "call `undo` to bring it back" }))
    }

    pub fn move_file(&self, project: &str, from: &str, to: &str) -> OpResult {
        let p = self.open(project)?;
        let src = fsutil::safe_join(p.dir(), from).map_err(err)?;
        let dst = fsutil::safe_join(p.dir(), to).map_err(err)?;
        if !src.exists() {
            return Err(format!("`{from}` does not exist"));
        }
        if dst.exists() {
            return Err(format!("`{to}` already exists; delete it first or pick another name"));
        }
        self.before_change(&p, "move_file", &format!("{from} -> {to}"))?;
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent).map_err(err)?;
        }
        fs::rename(&src, &dst).map_err(err)?;
        self.after_change(&p, "move_file", &format!("{from} -> {to}"))?;
        Ok(json!({ "from": from, "to": to }))
    }

    /// Search the Project's text files (plain text, or a regular expression), line by line.
    pub fn search(&self, project: &str, opts: &SearchOptions) -> OpResult {
        let p = self.open(project)?;
        let report = search::search(p.dir(), opts).map_err(err)?;
        let mut hits = Vec::new();
        'files: for file in &report.files {
            for l in &file.lines {
                hits.push(json!({ "path": file.path, "line": l.line, "text": l.text }));
                if hits.len() >= MAX_SEARCH_HITS {
                    break 'files;
                }
            }
        }
        let truncated = report.total > hits.len();
        Ok(json!({ "matches": hits, "total_matches": report.total, "truncated": truncated }))
    }

    // ----- Compile -----

    fn tex_install(&self) -> Result<TexInstall, String> {
        let mut cached = self.tex.lock().map_err(err)?;
        if let Some(install) = cached.as_ref() {
            return Ok(install.clone());
        }
        let install = tex::detect(self.tex_bin_dir.as_deref()).map_err(err)?;
        *cached = Some(install.clone());
        Ok(install)
    }

    /// Compile the Root file. The PDF is written even when the document has errors.
    fn run_compile(&self, p: &Project) -> Result<(compile::CompileOutcome, Engine), String> {
        let install = self.tex_install()?;
        let root = p.meta.root_file.clone();
        let source = fsutil::read_text(p.dir(), &root).map_err(err)?;
        let engine = p.meta.engine.unwrap_or_else(|| tex::detect_engine(&source));
        let build = p.build_dir(&root);
        let out = compile::compile(&CompileRequest {
            install: &install,
            project_dir: p.dir(),
            root_file: &root,
            engine,
            build_dir: &build,
            shell_escape: p.meta.shell_escape,
            timeout: Duration::from_secs(300),
        })
        .map_err(err)?;
        Ok((out, engine))
    }

    pub fn compile(&self, project: &str) -> OpResult {
        let p = self.open(project)?;
        let (out, engine) = self.run_compile(&p)?;
        let _ = audit::record(p.dir(), ACTOR, "compile", if out.success { "ok" } else { "failed" });
        let errors = out
            .diagnostics
            .iter()
            .filter(|d| matches!(d.severity, compile::Severity::Error))
            .count();
        let (undefined_citations, undefined_refs) = compile::undefined_in_log(&out.log);
        // How long the document came out, when Poppler is there to count (best effort).
        let pages = out.pdf.as_ref().and_then(|pdf| {
            let tools = pdfview::PdfTools::find(&self.tex_install().ok()?).ok()?;
            tools.info(pdf).ok().map(|i| i.pages)
        });
        Ok(json!({
            "success": out.success,
            "timed_out": out.timed_out,
            "engine": format!("{engine:?}").to_lowercase(),
            "seconds": (out.duration.as_secs_f64() * 10.0).round() / 10.0,
            "pdf": out.pdf.map(|p| p.display().to_string()),
            "pages": pages,
            "error_count": errors,
            "diagnostics": out.diagnostics.iter().take(MAX_DIAGNOSTICS).collect::<Vec<_>>(),
            "diagnostics_truncated": out.diagnostics.len() > MAX_DIAGNOSTICS,
            "boxes": compile::box_summary(&out.log, MAX_WORST_BOXES),
            "undefined_citations": undefined_citations,
            "undefined_refs": undefined_refs,
            "forced_rebuild": out.forced_rebuild,
            "console_tail": compile::console_summary(&out.console, errors > 0),
        }))
    }

    // ----- Looking at the result -----

    fn pdf_path(&self, p: &Project) -> PathBuf {
        let root = &p.meta.root_file;
        let stem = Path::new(root).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        p.build_dir(root).join(format!("{stem}.pdf"))
    }

    /// Whether the sources changed after this PDF was written.
    fn is_stale(&self, p: &Project, pdf: &Path) -> bool {
        let built = fs::metadata(pdf).and_then(|m| m.modified()).ok();
        match (fsutil::newest_modified(p.dir()), built) {
            (Some(source), Some(built)) => source > built,
            _ => false,
        }
    }

    /// The PDF to look at. With `rebuild`, a missing or out-of-date PDF is compiled first, so what
    /// an agent sees is what its edits produced. Returns the path and whether it compiled.
    fn current_pdf(&self, p: &Project, rebuild: bool) -> Result<(PathBuf, bool), String> {
        let pdf = self.pdf_path(p);
        if pdf.is_file() && !(rebuild && self.is_stale(p, &pdf)) {
            return Ok((pdf, false));
        }
        if !rebuild {
            return Err("There is no PDF yet. Call `compile` first (or leave `rebuild` on).".into());
        }
        let (out, _) = self.run_compile(p)?;
        match out.pdf {
            Some(path) => Ok((path, true)),
            None => {
                let first = out
                    .diagnostics
                    .iter()
                    .find(|d| matches!(d.severity, compile::Severity::Error))
                    .map(|d| format!(" First error: {}", d.message))
                    .unwrap_or_default();
                Err(format!("The compile produced no PDF.{first} Call `compile` or `diagnostics` for details."))
            }
        }
    }

    fn pdf_tools(&self) -> Result<pdfview::PdfTools, String> {
        pdfview::PdfTools::find(&self.tex_install()?).map_err(err)
    }

    /// Page count and sizes of the compiled PDF.
    pub fn pdf_info(&self, project: &str) -> OpResult {
        let p = self.open(project)?;
        let (pdf, _) = self.current_pdf(&p, false)?;
        let info = self.pdf_tools()?.info(&pdf).map_err(err)?;
        Ok(json!({
            "root_file": p.meta.root_file,
            "pdf": pdf.display().to_string(),
            "pages": info.pages,
            "title": info.title,
            "page_sizes_pt": info.sizes,
            "stale": self.is_stale(&p, &pdf),
        }))
    }

    /// Pages of the compiled PDF as PNG: `pages` is `3`, `2-5`, `4-`, `last`, `all` or `1,3,5-7`.
    /// `one_image` lays them out in a grid in a single picture; `separate` gives one image each.
    pub fn render_pages(&self, project: &str, args: RenderArgs<'_>) -> MediaResult {
        let p = self.open(project)?;
        let (pdf, rebuilt) = self.current_pdf(&p, args.rebuild)?;
        let tools = self.pdf_tools()?;
        let info = tools.info(&pdf).map_err(err)?;
        let pages = pdfview::parse_pages(args.pages, info.pages).map_err(err)?;
        let one_image = match args.layout {
            "" | "auto" => pages.len() > 1,
            "one_image" | "grid" => true,
            "separate" => false,
            other => return Err(format!("layout must be one_image or separate, not `{other}`")),
        };
        let limit = if one_image { pdfview::MAX_PAGES } else { MAX_SEPARATE_IMAGES };
        if pages.len() > limit {
            return Err(format!(
                "{} pages is too many for one call (most is {limit} {}). Ask for a range such as `1-{limit}`, then the next.",
                pages.len(),
                if one_image { "in one image" } else { "as separate images" }
            ));
        }

        let columns = if one_image {
            args.columns.unwrap_or_else(|| pdfview::default_columns(pages.len())).clamp(1, pages.len())
        } else {
            1
        };
        // A single page can be sharp; a contact sheet is fitted to a width a viewer can show.
        let (width_px, ceiling) = if pages.len() == 1 || !one_image { (1200, 130) } else { (3000, 110) };
        let dpi = args
            .dpi
            .map(|d| d.clamp(40, 300))
            .unwrap_or_else(|| pdfview::fit_dpi(&info.sizes, &pages, columns, width_px, ceiling));

        let rasters = tools.render(&pdf, &pages, dpi).map_err(err)?;
        let mut images = Vec::new();
        let mut rows: Vec<Vec<u32>> = Vec::new();
        if one_image {
            let sheet = pdfview::grid(&rasters, columns).map_err(err)?;
            images.push(Image::png(&sheet, pdfview::encode_png(&sheet).map_err(err)?));
            rows = pages.chunks(columns).map(<[u32]>::to_vec).collect();
        } else {
            for r in &rasters {
                images.push(Image::png(r, pdfview::encode_png(r).map_err(err)?));
            }
        }
        let stale = self.is_stale(&p, &pdf);
        let mut note = String::new();
        if stale {
            note = "The sources changed after this PDF was built; pass rebuild=true (the default) to see the latest.".into();
        }
        Ok(Media {
            info: json!({
                "pages_shown": pages,
                "document_pages": info.pages,
                "layout": if one_image { "one_image" } else { "separate" },
                "columns": columns,
                "dpi": dpi,
                "grid_rows": rows,
                "images": images.iter().map(|i| json!({"width": i.width, "height": i.height})).collect::<Vec<_>>(),
                "rebuilt": rebuilt,
                "stale": stale,
                "note": note,
            }),
            images,
        })
    }

    /// An image file from the Project, for looking at figures.
    pub fn view_image(&self, project: &str, path: &str) -> MediaResult {
        const MAX_BYTES: u64 = 8 * 1024 * 1024;
        let p = self.open(project)?;
        let full = fsutil::safe_join(p.dir(), path).map_err(err)?;
        let mime = match path.rsplit('.').next().map(str::to_ascii_lowercase).as_deref() {
            Some("png") => "image/png",
            Some("jpg" | "jpeg") => "image/jpeg",
            Some("gif") => "image/gif",
            Some("webp") => "image/webp",
            _ => return Err(format!("`{path}` is not a PNG, JPEG, GIF or WebP image. To look at a PDF page use `render_pages`.")),
        };
        let size = fs::metadata(&full).map_err(|_| format!("`{path}` does not exist"))?.len();
        if size > MAX_BYTES {
            return Err(format!("`{path}` is {} MB; the limit is 8 MB", size / 1024 / 1024));
        }
        let bytes = fs::read(&full).map_err(err)?;
        Ok(Media {
            info: json!({ "path": path, "bytes": size, "mime": mime }),
            images: vec![Image { bytes, mime, width: 0, height: 0 }],
        })
    }

    /// Where a source line landed in the PDF (SyncTeX forward search).
    pub fn source_to_pdf(&self, project: &str, file: &str, line: u32, column: Option<u32>) -> OpResult {
        let p = self.open(project)?;
        fsutil::safe_join(p.dir(), file).map_err(err)?;
        let (pdf, _) = self.current_pdf(&p, false)?;
        let hits = synctex::forward(&self.tex_install()?, p.dir(), &pdf, file, line, column.unwrap_or(0)).map_err(err)?;
        Ok(json!({
            "file": file,
            "line": line,
            "found": !hits.is_empty(),
            "hits": hits,
            "units": "PDF points from the top-left corner of the page (72 per inch)",
            "stale": self.is_stale(&p, &pdf),
            "next": if hits.is_empty() {
                "No match: the line may not produce output, or the PDF is out of date. Compile and try again."
            } else {
                "Call `render_pages` with the page number to look at it."
            },
        }))
    }

    /// Which source line produced a point on a PDF page (SyncTeX inverse search).
    pub fn pdf_to_source(&self, project: &str, page: u32, x: f64, y: f64) -> OpResult {
        let p = self.open(project)?;
        let (pdf, _) = self.current_pdf(&p, false)?;
        let hit = synctex::inverse(&self.tex_install()?, p.dir(), &pdf, page, x, y).map_err(err)?;
        Ok(json!({ "page": page, "x": x, "y": y, "source": hit, "stale": self.is_stale(&p, &pdf) }))
    }

    /// The last compile's errors and warnings, read from its log without compiling again.
    pub fn diagnostics(&self, project: &str) -> OpResult {
        let p = self.open(project)?;
        let root = &p.meta.root_file;
        let stem = Path::new(root).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let log_path = p.build_dir(root).join(format!("{stem}.log"));
        let bytes = fs::read(&log_path).map_err(|_| "This Project has not been compiled yet. Call `compile`.".to_string())?;
        let log = String::from_utf8_lossy(&bytes);
        let all = compile::parse_log(&log);
        let (errors, warnings): (Vec<_>, Vec<_>) =
            all.iter().partition(|d| matches!(d.severity, compile::Severity::Error));
        let layout = compile::parse_layout_warnings(&log);
        let pdf = self.pdf_path(&p);
        Ok(json!({
            "root_file": root,
            "error_count": errors.len(),
            "errors": errors.iter().take(MAX_DIAGNOSTICS).collect::<Vec<_>>(),
            "warning_count": warnings.len(),
            "warnings": warnings.iter().take(MAX_DIAGNOSTICS).collect::<Vec<_>>(),
            "layout_count": layout.len(),
            "layout": layout.iter().take(MAX_DIAGNOSTICS).collect::<Vec<_>>(),
            "boxes": compile::box_summary(&log, MAX_WORST_BOXES),
            "stale": pdf.is_file() && self.is_stale(&p, &pdf),
            "note": "Read from the last compile's log. `layout` lists overfull and underfull boxes (text in the margin, loose lines).",
        }))
    }

    /// Delete the build cache (generated files). The next compile starts from scratch; no source
    /// file is touched.
    pub fn clean_build(&self, project: &str) -> OpResult {
        let p = self.open(project)?;
        let build = p.dir().join(moss_core::project::META_DIR).join("build");
        let existed = build.is_dir();
        if existed {
            fs::remove_dir_all(&build).map_err(err)?;
        }
        audit::record(p.dir(), ACTOR, "clean_build", "").map_err(err)?;
        Ok(json!({ "cleaned": existed, "note": "Generated files only; your sources were not touched." }))
    }

    // ----- Reading the document's structure -----

    /// Sections across `\input`/`\include`d files, in document order.
    pub fn outline(&self, project: &str) -> OpResult {
        let p = self.open(project)?;
        let scan = outline::scan(p.dir(), &p.meta.root_file).map_err(err)?;
        Ok(json!({
            "root_file": p.meta.root_file,
            "files": scan.files,
            "headings": scan.headings,
            "labels": scan.labels,
            "missing_inputs": scan.missing_inputs,
        }))
    }

    /// Broken cross-references and citations, and unused labels and bibliography entries.
    pub fn references(&self, project: &str) -> OpResult {
        let p = self.open(project)?;
        let r = outline::references(p.dir(), &p.meta.root_file).map_err(err)?;
        let problems = r.undefined_refs.len()
            + r.undefined_cites.len()
            + r.duplicate_labels.len()
            + r.duplicate_bib_keys.len()
            + r.missing_inputs.len();
        let mut value = to_value(&r)?;
        value["problem_count"] = json!(problems);
        value["note"] = json!("Read from the sources, not the PDF: macros that build labels are not expanded. `unused_*` entries are suggestions, not errors.");
        Ok(value)
    }

    // ----- Starter documents -----

    pub fn list_templates(&self) -> OpResult {
        let list: Vec<Value> = templates::all()
            .iter()
            .map(|t| json!({ "id": t.id, "name": t.name, "description": t.description, "root_file": t.root_file,
                             "files": t.files.iter().map(|(p, _)| p).collect::<Vec<_>>() }))
            .collect();
        Ok(json!({ "templates": list }))
    }

    // ----- History -----

    pub fn history_list(&self, project: &str) -> OpResult {
        let p = self.open(project)?;
        to_value(&History::new(p.dir()).list().map_err(err)?)
    }

    pub fn history_changes(&self, project: &str, snapshot_id: u64) -> OpResult {
        let p = self.open(project)?;
        to_value(&History::new(p.dir()).changes_since(snapshot_id).map_err(err)?)
    }

    pub fn undo(&self, project: &str) -> OpResult {
        let p = self.open(project)?;
        let step = History::new(p.dir()).undo().map_err(err)?;
        audit::record(p.dir(), ACTOR, "undo", "").map_err(err)?;
        match step {
            Some(snapshot) => Ok(json!({ "restored": snapshot })),
            None => Err("nothing to undo".into()),
        }
    }

    pub fn redo(&self, project: &str) -> OpResult {
        let p = self.open(project)?;
        let step = History::new(p.dir()).redo().map_err(err)?;
        audit::record(p.dir(), ACTOR, "redo", "").map_err(err)?;
        match step {
            Some(snapshot) => Ok(json!({ "restored": snapshot })),
            None => Err("nothing to redo".into()),
        }
    }

    pub fn restore(&self, project: &str, snapshot_id: u64) -> OpResult {
        let p = self.open(project)?;
        let snapshot = History::new(p.dir()).restore(snapshot_id).map_err(err)?;
        audit::record(p.dir(), ACTOR, "restore", &snapshot_id.to_string()).map_err(err)?;
        Ok(json!({ "restored": snapshot }))
    }

    // ----- Versions -----

    /// Save the current files as a named Version.
    pub fn history_save(&self, project: &str, message: &str) -> OpResult {
        let p = self.open(project)?;
        match History::new(p.dir()).save_version(message).map_err(err)? {
            Some(v) => {
                audit::record(p.dir(), ACTOR, "history_save", message).map_err(err)?;
                Ok(json!({ "saved": v }))
            }
            None => Ok(json!({ "saved": null, "note": "nothing changed since the last version" })),
        }
    }

    /// What one Version changed, file by file, with line counts.
    pub fn history_show(&self, project: &str, snapshot_id: u64) -> OpResult {
        let p = self.open(project)?;
        let files = History::new(p.dir()).compare_with_parent(snapshot_id).map_err(err)?;
        Ok(json!({ "snapshot_id": snapshot_id, "files": files }))
    }

    /// One file as it was in a Version, read like `read_file`, without touching the working copy.
    pub fn history_read_file(
        &self,
        project: &str,
        snapshot_id: u64,
        path: &str,
        start_line: Option<usize>,
        end_line: Option<usize>,
    ) -> OpResult {
        let p = self.open(project)?;
        fsutil::safe_join(p.dir(), path).map_err(err)?;
        let view = History::new(p.dir()).file_at(Rev::Snapshot { id: snapshot_id }, path).map_err(err)?;
        if !view.exists {
            return Err(format!("`{path}` did not exist in version #{snapshot_id}"));
        }
        if view.binary {
            return Err(format!("`{path}` is not a text file ({} bytes)", view.size));
        }
        let Some(text) = view.text else {
            return Err(format!("`{path}` is {} bytes, too large to read from history in one go", view.size));
        };
        let slice = slice_lines(&text, start_line, end_line, READ_BUDGET)?;
        let mut value = slice.into_json(path);
        value["snapshot_id"] = json!(snapshot_id);
        Ok(value)
    }

    /// What changed between two states: `a` and `b` are a version number, `working` (the files
    /// now) or `empty`. With a `path`, the change as a unified diff; without, the files that differ.
    pub fn history_diff(&self, project: &str, a: &str, b: &str, path: Option<&str>) -> OpResult {
        let p = self.open(project)?;
        let history = History::new(p.dir());
        let (from, to) = (parse_rev(a)?, parse_rev(b)?);
        let Some(path) = path else {
            let files = history.compare(from, to).map_err(err)?;
            return Ok(json!({ "from": a, "to": b, "files": files }));
        };
        fsutil::safe_join(p.dir(), path).map_err(err)?;
        let (old, new) = (history.file_at(from, path).map_err(err)?, history.file_at(to, path).map_err(err)?);
        if old.binary || new.binary {
            return Err(format!("`{path}` is not a text file"));
        }
        if (old.exists && old.text.is_none()) || (new.exists && new.text.is_none()) {
            return Err(format!("`{path}` is too large to diff here"));
        }
        let (old_text, new_text) = (old.text.unwrap_or_default(), new.text.unwrap_or_default());
        let diff = moss_core::textdiff::unified(&old_text, &new_text, &format!("{path} @ {a}"), &format!("{path} @ {b}"));
        let truncated = diff.chars().count() > READ_BUDGET;
        let diff: String = if truncated { diff.chars().take(READ_BUDGET).collect() } else { diff };
        Ok(json!({
            "path": path,
            "from": a,
            "to": b,
            "existed_before": old.exists,
            "exists_after": new.exists,
            "identical": old_text == new_text,
            "diff": diff,
            "truncated": truncated,
        }))
    }

    pub fn history_restore_file(&self, project: &str, snapshot_id: u64, path: &str) -> OpResult {
        let p = self.open(project)?;
        fsutil::safe_join(p.dir(), path).map_err(err)?;
        let v = History::new(p.dir()).restore_file(snapshot_id, path).map_err(err)?;
        audit::record(p.dir(), ACTOR, "history_restore_file", &format!("{path} from #{snapshot_id}"))
            .map_err(err)?;
        Ok(json!({ "restored": v }))
    }

    // ----- Overleaf -----

    fn linked(&self, p: &Project) -> Result<OverleafLink, String> {
        p.overleaf_link()
            .ok_or_else(|| "This Project is not linked to Overleaf (Origin is local).".to_string())
    }

    /// The saved Overleaf sign-in, or an error that tells the agent to ask the user.
    fn session(&self) -> Result<overleaf::Session, String> {
        overleaf::store::load(overleaf::DEFAULT_HOST)
            .map_err(err)?
            .ok_or_else(|| {
                "Not signed in to Overleaf. Ask the user to sign in from the Moss app.".to_string()
            })
    }

    fn remote_of(&self, link: &OverleafLink) -> Result<Box<dyn Remote>, String> {
        let session = if link::is_practice(link) { None } else { Some(self.session()?) };
        link::open_remote(self.workspace.root(), link, session.as_ref()).map_err(err)
    }

    /// Link state plus the files that differ from the last Sync. No network.
    pub fn overleaf_status(&self, project: &str) -> OpResult {
        let p = self.open(project)?;
        let signed_in = overleaf::store::load(overleaf::DEFAULT_HOST).map_err(err)?.is_some();
        let Some(link) = p.overleaf_link() else {
            return Ok(json!({ "linked": false, "signed_in": signed_in }));
        };
        let practice = link::is_practice(&link);
        Ok(json!({
            "linked": true,
            "signed_in": signed_in,
            "practice": practice,
            "overleaf_project": link.name,
            "overleaf_project_id": link.project_id,
            "last_synced": link.last_synced,
            "has_overleaf_history": !practice,
            "local_changes": sync::local_changes(&p).map_err(err)?,
        }))
    }

    /// The signed-in user's Overleaf projects, to pick one for `overleaf_link`.
    pub fn overleaf_projects(&self) -> OpResult {
        let session = self.session()?;
        let projects = overleaf::Client::new(&session)
            .and_then(|c| c.list_projects())
            .map_err(err)?;
        let live: Vec<_> = projects.iter().filter(|p| !p.trashed).collect();
        Ok(json!({ "projects": live }))
    }

    /// Overleaf's own change history for a linked Project: who edited which files, newest first.
    /// Read-only. Pass `next_before` from the previous result as `before` for older entries.
    pub fn overleaf_history(&self, project: &str, before: Option<i64>) -> OpResult {
        let p = self.open(project)?;
        let link = self.linked(&p)?;
        if link::is_practice(&link) {
            return Err("A practice project has no Overleaf history.".into());
        }
        let session = self.session()?;
        let history = overleaf::Client::new(&session)
            .and_then(|c| c.history(&link.project_id, before))
            .map_err(err)?;
        Ok(json!({
            "overleaf_project": link.name,
            "updates": history.updates,
            "next_before": history.next_before,
        }))
    }

    /// Link a Local Project to Overleaf. `existing` needs `overleaf_id` and `name` from
    /// `overleaf_projects`; the first Sync then asks about every file that differs. `new` uploads
    /// the files as a new Overleaf project, which puts them on Overleaf and needs an Approval.
    pub fn overleaf_link(
        &self,
        project: &str,
        how: &str,
        overleaf_id: Option<&str>,
        name: Option<&str>,
    ) -> OpResult {
        let p = self.open(project)?;
        let link = match how {
            "existing" => {
                let id = overleaf_id.ok_or("`overleaf_id` is required to link an existing project")?;
                let name = name.ok_or("`name` is required to link an existing project")?;
                // Catch a mistyped id now, not on the first sync. Skipped in unit tests, which
                // must not depend on whoever is signed in on the machine running them.
                #[cfg(not(test))]
                if let Some(session) = overleaf::store::load(overleaf::DEFAULT_HOST).map_err(err)? {
                    let projects = overleaf::Client::new(&session)
                        .and_then(|c| c.list_projects())
                        .map_err(err)?;
                    find_overleaf_project(&projects, id)?;
                }
                link::link_existing(&p, id, name).map_err(err)?
            }
            "new" => {
                let session = self.session()?;
                self.approve(
                    project,
                    &p,
                    "overleaf_link",
                    &format!("create a new Overleaf project from {}", p.meta.name),
                )?;
                link::link_new_overleaf(&p, &session).map_err(err)?
            }
            other => return Err(format!("how must be existing or new, not `{other}`")),
        };
        audit::record(p.dir(), ACTOR, "overleaf_link", &format!("{how}: {}", link.name)).map_err(err)?;
        Ok(json!({ "linked": link }))
    }

    /// Turn a linked Project back into a Local one. Files are untouched; the Base snapshot is
    /// dropped, so linking again later asks about every file that differs. Needs an Approval.
    pub fn overleaf_unlink(&self, project: &str) -> OpResult {
        let p = self.open(project)?;
        let link = self.linked(&p)?;
        self.approve(project, &p, "overleaf_unlink", &format!("stop syncing with {}", link.name))?;
        link::unlink(&p).map_err(err)?;
        audit::record(p.dir(), ACTOR, "overleaf_unlink", &link.name).map_err(err)?;
        Ok(json!({ "linked": false, "was": link.name }))
    }

    /// What a Pull, Push or Sync would do. Changes nothing.
    pub fn overleaf_review(&self, project: &str, mode: &str) -> OpResult {
        let p = self.open(project)?;
        let link = self.linked(&p)?;
        let remote = self.remote_of(&link)?;
        let (preview, _) = Syncer::new(&p, remote.as_ref()).preview(parse_mode(mode)?).map_err(err)?;
        to_review(&preview)
    }

    /// Check for merge conflicts before syncing: which files overlap, region by region, and which
    /// files Moss can merge on its own. Fetches Overleaf but changes nothing anywhere.
    pub fn overleaf_conflicts(&self, project: &str, mode: &str) -> OpResult {
        let p = self.open(project)?;
        let link = self.linked(&p)?;
        let remote = self.remote_of(&link)?;
        let (preview, _) = Syncer::new(&p, remote.as_ref()).preview(parse_mode(mode)?).map_err(err)?;
        let auto_merged: Vec<&str> = preview
            .items
            .iter()
            .filter(|i| i.op == Op::Merge)
            .map(|i| i.path.as_str())
            .collect();
        let conflicts: Vec<Value> = preview.conflicts.iter().map(conflict_json).collect();
        Ok(json!({
            "mode": preview.mode,
            "has_conflicts": !conflicts.is_empty(),
            "conflict_count": conflicts.len(),
            "conflicts": conflicts,
            "auto_merged": auto_merged,
            "blocked": preview.blocked,
            "next": if conflicts.is_empty() {
                "No conflicts: overleaf_sync (or overleaf_pull / overleaf_push) will go through without questions."
            } else {
                "Answer every conflict in one call: overleaf_resolve with a `resolutions` entry per file (choice mine, theirs, text, or regions with one answer per region)."
            },
        }))
    }

    /// Do a Pull, Push or Sync. Stops and reports when conflicts have no resolution. Anything that
    /// writes to Overleaf waits for the user's Approval unless the Project is trusted.
    pub fn overleaf_sync(
        &self,
        project: &str,
        mode: &str,
        resolutions: Vec<ResolutionArg>,
        allow_mass_delete: bool,
    ) -> OpResult {
        let mode = parse_mode(mode)?;
        let p = self.open(project)?;
        let link = self.linked(&p)?;
        let remote = self.remote_of(&link)?;
        let syncer = Syncer::new(&p, remote.as_ref());
        let (preview, _) = syncer.preview(mode).map_err(err)?;
        if let Some(reason) = &preview.blocked {
            return Err(reason.clone());
        }

        let resolutions: Vec<Resolution> = resolutions
            .into_iter()
            .map(Resolution::try_from)
            .collect::<Result<_, _>>()?;
        if let Some(stray) = resolutions
            .iter()
            .find(|r| !preview.conflicts.iter().any(|c| c.path == r.path))
        {
            return Err(format!(
                "`{}` is not a conflict right now; call `overleaf_conflicts` for the current list",
                stray.path
            ));
        }
        let unanswered: Vec<_> = preview
            .conflicts
            .iter()
            .filter(|c| !resolutions.iter().any(|r| r.path == c.path))
            .collect();
        if !unanswered.is_empty() {
            let mut review = to_review(&preview)?;
            review["applied"] = json!(false);
            review["reason"] = json!(
                "These conflicts need a resolution. Call again with `resolutions` (choice mine, theirs or text)."
            );
            return Ok(review);
        }
        if preview.mass_delete.is_some() && !allow_mass_delete {
            let mut review = to_review(&preview)?;
            review["applied"] = json!(false);
            review["reason"] = json!(
                "This would delete many files. Check the list with the user, then call again with allow_mass_delete."
            );
            return Ok(review);
        }

        if mode != Mode::Pull {
            let detail = format!(
                "{} {} file(s) to {}",
                match mode {
                    Mode::Push => "push",
                    _ => "sync",
                },
                preview.summary.push + preview.summary.merge + preview.summary.delete_there
                    + preview.conflicts.len(),
                link.name
            );
            self.approve(project, &p, "overleaf_sync", &detail)?;
        }
        audit::record(p.dir(), ACTOR, "overleaf_sync", &format!("{mode:?}")).map_err(err)?;

        let request = ApplyRequest {
            mode,
            token: preview.token.clone(),
            resolutions,
            allow_mass_delete,
            allow_markers: false,
            skip: Vec::new(),
            actor: AUTHOR_AGENT.to_string(),
        };
        match syncer.apply(&request).map_err(err)? {
            Applied::Done { outcome } => Ok(json!({ "applied": true, "outcome": outcome })),
            Applied::Stale { side } => Ok(json!({
                "applied": false,
                "reason": format!("{side:?} changed while this was being prepared; nothing was written. Review and try again."),
            })),
        }
    }
}

/// How one conflict should be settled, as an agent states it.
#[derive(Deserialize, JsonSchema)]
pub struct ResolutionArg {
    /// The conflicting file's path, as listed in `conflicts`.
    pub path: String,
    /// "mine" (the whole file as on this PC), "theirs" (the whole file as on Overleaf), "text"
    /// (use `text`) or "regions" (one entry in `regions` per conflict region, in order).
    pub choice: String,
    /// The complete final text of the file when `choice` is "text".
    pub text: Option<String>,
    /// One answer per conflict region of the file, in order, when `choice` is "regions".
    pub regions: Option<Vec<RegionArg>>,
}

/// The answer for one conflicting region of a file.
#[derive(Deserialize, JsonSchema)]
pub struct RegionArg {
    /// "mine", "theirs", "both" (mine, then theirs) or "text" (use `text`).
    pub choice: String,
    /// The text for this region when `choice` is "text".
    pub text: Option<String>,
}

impl TryFrom<RegionArg> for Choice {
    type Error = String;

    fn try_from(a: RegionArg) -> Result<Choice, String> {
        match a.choice.as_str() {
            "mine" => Ok(Choice::Mine),
            "theirs" => Ok(Choice::Theirs),
            "both" => Ok(Choice::Both),
            "text" => Ok(Choice::Text { text: a.text.ok_or("choice `text` needs `text`")? }),
            other => Err(format!("region choice must be mine, theirs, both or text, not `{other}`")),
        }
    }
}

impl TryFrom<ResolutionArg> for Resolution {
    type Error = String;

    fn try_from(a: ResolutionArg) -> Result<Resolution, String> {
        let choice = match a.choice.as_str() {
            "mine" => FileChoice::Mine,
            "theirs" => FileChoice::Theirs,
            "text" => FileChoice::Text {
                text: a.text.ok_or_else(|| format!("{}: choice `text` needs `text`", a.path))?,
            },
            "regions" => FileChoice::Regions {
                choices: a
                    .regions
                    .ok_or_else(|| format!("{}: choice `regions` needs `regions`", a.path))?
                    .into_iter()
                    .map(Choice::try_from)
                    .collect::<Result<_, _>>()
                    .map_err(|e| format!("{}: {e}", a.path))?,
            },
            other => {
                return Err(format!(
                    "{}: choice must be mine, theirs, text or regions, not `{other}`",
                    a.path
                ));
            }
        };
        Ok(Resolution { path: a.path, choice })
    }
}

/// The account's project with this id, or why it cannot be linked.
#[cfg_attr(test, allow(dead_code))]
fn find_overleaf_project<'a>(
    projects: &'a [overleaf::RemoteProject],
    id: &str,
) -> Result<&'a overleaf::RemoteProject, String> {
    let found = projects.iter().find(|p| p.id == id).ok_or_else(|| {
        format!("No Overleaf project with id `{id}` in this account. Call `overleaf_projects` for the ids.")
    })?;
    if found.trashed {
        return Err(format!("`{}` is in the Overleaf trash; restore it there first.", found.name));
    }
    Ok(found)
}

fn parse_mode(mode: &str) -> Result<Mode, String> {
    match mode {
        "pull" => Ok(Mode::Pull),
        "push" => Ok(Mode::Push),
        "sync" => Ok(Mode::Sync),
        other => Err(format!("mode must be pull, push or sync, not `{other}`")),
    }
}

/// Keep what an agent reads small: long file bodies are clipped.
fn clip(text: &Option<String>) -> Option<String> {
    const MAX: usize = 20_000;
    text.as_ref().map(|t| {
        if t.len() <= MAX {
            t.clone()
        } else {
            let end = (0..=MAX).rev().find(|i| t.is_char_boundary(*i)).unwrap_or(0);
            format!("{}\n[clipped, {} more bytes]", &t[..end], t.len() - end)
        }
    })
}

const CONTEXT_LINES: usize = 3;

fn tail_lines(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(CONTEXT_LINES)..].join("\n")
}

fn head_lines(text: &str) -> String {
    text.lines().take(CONTEXT_LINES).collect::<Vec<_>>().join("\n")
}

/// One conflict as an agent should read it. A text file that overlaps in places is shown as its
/// numbered conflict regions with a few lines of context around each, so the agent answers each
/// region (`choice: "regions"`); anything else (binary, added/deleted on one side) is shown whole.
fn conflict_json(c: &Conflict) -> Value {
    let Some(regions) = &c.regions else {
        return json!({
            "path": c.path,
            "kind": c.kind,
            "binary": c.binary,
            "mine_exists": c.mine_exists,
            "theirs_exists": c.theirs_exists,
            "base": clip(&c.base),
            "mine": clip(&c.mine),
            "theirs": clip(&c.theirs),
        });
    };
    let mut out = Vec::new();
    for (i, region) in regions.iter().enumerate() {
        let Region::Conflict { base, mine, theirs } = region else { continue };
        let before = match i.checked_sub(1).and_then(|j| regions.get(j)) {
            Some(Region::Stable { text }) => tail_lines(text),
            _ => String::new(),
        };
        let after = match regions.get(i + 1) {
            Some(Region::Stable { text }) => head_lines(text),
            _ => String::new(),
        };
        out.push(json!({
            "index": out.len(),
            "before": before,
            "base": base,
            "mine": mine,
            "theirs": theirs,
            "after": after,
        }));
    }
    json!({
        "path": c.path,
        "kind": c.kind,
        "binary": c.binary,
        "mine_exists": c.mine_exists,
        "theirs_exists": c.theirs_exists,
        "regions": out,
    })
}

fn to_review(preview: &Preview) -> OpResult {
    let conflicts: Vec<Value> = preview.conflicts.iter().map(conflict_json).collect();
    Ok(json!({
        "mode": preview.mode,
        "summary": preview.summary,
        "items": preview.items,
        "conflicts": conflicts,
        "needs_attention": preview.needs_attention,
        "warnings": preview.warnings,
        "mass_delete": preview.mass_delete,
        "blocked": preview.blocked,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ops() -> (tempfile::TempDir, Ops, String) {
        let dir = tempfile::tempdir().unwrap();
        let ws = Workspace::open(dir.path()).unwrap();
        let created = ws.create_project("Report").unwrap();
        let folder = created.dir_name.clone();
        let ops = Ops::new(ws, None).with_approval_timeout(Duration::from_millis(300));
        (dir, ops, folder)
    }

    impl Ops {
        fn read(&self, p: &str, path: &str) -> OpResult {
            self.read_file(p, path, None, None, None)
        }

        fn edit1(&self, p: &str, path: &str, old: &str, new: &str, all: bool) -> OpResult {
            let edit = EditSpec { old_text: old.into(), new_text: new.into(), replace_all: Some(all) };
            self.edit_file(p, path, &[edit])
        }

        fn delete1(&self, p: &str, path: &str) -> OpResult {
            self.delete_files(p, &[path.to_string()])
        }

        fn versions(&self, p: &str) -> usize {
            self.history_list(p).unwrap().as_array().unwrap().len()
        }

        fn trust(&self, p: &str) {
            let mut project = self.open(p).unwrap();
            project.meta.trusted = true;
            project.save_meta().unwrap();
        }
    }

    fn spec(path: &str, content: &str) -> FileSpec {
        FileSpec { path: path.into(), content: content.into() }
    }

    #[test]
    fn a_long_file_is_read_by_line_range_and_in_parts() {
        let (_dir, ops, p) = ops();
        let text: String = (1..=300).map(|i| format!("line {i:03} {}\n", "x".repeat(8))).collect();
        ops.write_file(&p, "long.tex", &text).unwrap();

        let part = ops.read_file(&p, "long.tex", Some(10), Some(12), None).unwrap();
        assert_eq!(part["content"], "line 010 xxxxxxxx\nline 011 xxxxxxxx\nline 012 xxxxxxxx\n");
        assert_eq!((part["start_line"].as_u64(), part["end_line"].as_u64()), (Some(10), Some(12)));
        assert_eq!(part["total_lines"], 300);
        assert_eq!(part["truncated"], false);
        assert!(part.get("next_start_line").is_none());

        // Read in parts of at most 1,000 characters and glue them back together.
        let mut whole = String::new();
        let mut next = Some(1);
        let mut calls = 0;
        while let Some(start) = next {
            let r = ops.read_file(&p, "long.tex", Some(start), None, Some(1_000)).unwrap();
            whole.push_str(r["content"].as_str().unwrap());
            assert!(r["content"].as_str().unwrap().chars().count() <= 1_000);
            next = r["next_start_line"].as_u64().map(|n| n as usize);
            calls += 1;
        }
        assert!(calls > 1, "the file does not fit in one part");
        assert_eq!(whole, text);

        assert!(ops.read_file(&p, "long.tex", Some(301), None, None).unwrap_err().contains("300 lines"));
        assert!(ops.read_file(&p, "long.tex", Some(5), Some(2), None).is_err());
        let empty = {
            ops.write_file(&p, "empty.tex", "").unwrap();
            ops.read(&p, "empty.tex").unwrap()
        };
        assert_eq!((empty["total_lines"].as_u64(), empty["content"].as_str()), (Some(0), Some("")));
    }

    #[test]
    fn several_files_are_read_in_one_call_with_one_budget() {
        let (_dir, ops, p) = ops();
        ops.write_file(&p, "a.tex", "alpha\n").unwrap();
        ops.write_file(&p, "b.tex", &"b".repeat(1_500)).unwrap();
        ops.write_file(&p, "c.tex", "gamma\n").unwrap();
        let paths: Vec<String> = ["a.tex", "nope.tex", "b.tex", "c.tex"].iter().map(|s| s.to_string()).collect();
        let r = ops.read_files(&p, &paths, Some(1_000)).unwrap();
        let files = r["files"].as_array().unwrap();
        assert_eq!(files[0]["content"], "alpha\n");
        assert!(files[1]["error"].is_string(), "a missing file reports its own error");
        assert_eq!(files[2]["truncated"], false, "one long line is cut, not skipped: {:?}", files[2]);
        assert_eq!(files[2]["line_cut"], true);
        assert!(files[3]["skipped"].is_string(), "the budget ran out before c.tex");
        assert!(ops.read_files(&p, &[], None).is_err());
    }

    #[test]
    fn write_files_is_one_version_and_leaves_identical_files_alone() {
        let (_dir, ops, p) = ops();
        let batch = [spec("a.tex", "one"), spec("sub/b.tex", "two"), spec("c.tex", "three")];
        ops.write_files(&p, &batch).unwrap();
        let after_first = ops.versions(&p);

        let again = ops.write_files(&p, &[spec("a.tex", "one"), spec("c.tex", "THREE")]).unwrap();
        assert_eq!(again["changed"], 1);
        assert_eq!(again["files"][0]["changed"], false);
        assert_eq!(again["files"][1]["changed"], true);
        assert_eq!(ops.versions(&p), after_first + 1, "two files, one Version");

        let same = ops.write_files(&p, &[spec("a.tex", "one")]).unwrap();
        assert_eq!(same["changed"], 0);
        assert_eq!(ops.versions(&p), after_first + 1, "a no-op takes no restore point");
        assert_eq!(ops.write_file(&p, "a.tex", "one").unwrap()["changed"], false);
        assert_eq!(ops.versions(&p), after_first + 1);

        ops.undo(&p).unwrap();
        assert_eq!(ops.read(&p, "c.tex").unwrap()["content"], "three");

        assert!(ops.write_files(&p, &[spec("x.tex", "1"), spec("x.tex", "2")]).is_err(), "duplicate path");
        assert!(ops.write_files(&p, &[spec("ok.tex", "1"), spec("../bad.tex", "2")]).is_err());
        assert!(ops.read(&p, "ok.tex").is_err(), "a bad path in the batch writes nothing");
        assert!(ops.write_files(&p, &[]).is_err());
    }

    #[test]
    fn several_edits_apply_in_order_or_not_at_all() {
        let (_dir, ops, p) = ops();
        ops.write_file(&p, "a.tex", "cat dog fish").unwrap();
        let before = ops.versions(&p);
        let edits = [
            EditSpec { old_text: "cat".into(), new_text: "bird".into(), replace_all: None },
            // The second edit sees the result of the first.
            EditSpec { old_text: "bird dog".into(), new_text: "owl".into(), replace_all: None },
        ];
        let done = ops.edit_file(&p, "a.tex", &edits).unwrap();
        assert_eq!(done["replacements"], 2);
        assert_eq!(ops.read(&p, "a.tex").unwrap()["content"], "owl fish");
        assert_eq!(ops.versions(&p), before + 1, "two edits, one Version");

        let bad = [
            EditSpec { old_text: "owl".into(), new_text: "hen".into(), replace_all: None },
            EditSpec { old_text: "missing".into(), new_text: "x".into(), replace_all: None },
        ];
        let err = ops.edit_file(&p, "a.tex", &bad).unwrap_err();
        assert!(err.contains("edit 2") && err.contains("Nothing was changed"), "{err}");
        assert_eq!(ops.read(&p, "a.tex").unwrap()["content"], "owl fish", "all or nothing");
        assert!(ops.edit_file(&p, "a.tex", &[]).is_err());

        // An edit that changes nothing takes no restore point.
        let same = EditSpec { old_text: "owl".into(), new_text: "owl".into(), replace_all: None };
        assert_eq!(ops.edit_file(&p, "a.tex", &[same]).unwrap()["changed"], false);
        assert_eq!(ops.versions(&p), before + 1);
    }

    #[test]
    fn several_files_are_deleted_with_one_approval_and_come_back_with_one_undo() {
        let (_dir, ops, p) = ops();
        ops.write_files(&p, &[spec("a.tex", "A"), spec("dir/b.tex", "B"), spec("c.tex", "C")]).unwrap();
        let paths = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();

        let denied = ops.delete_files(&p, &paths(&["a.tex", "c.tex"])).unwrap_err();
        assert!(denied.contains("did not approve") && denied.contains("a.tex, c.tex"), "{denied}");
        assert!(ops.read(&p, "a.tex").is_ok());

        ops.trust(&p);
        let missing = ops.delete_files(&p, &paths(&["a.tex", "ghost.tex"])).unwrap_err();
        assert!(missing.contains("ghost.tex") && missing.contains("Nothing was deleted"), "{missing}");
        assert!(ops.read(&p, "a.tex").is_ok(), "one missing path deletes nothing");

        // A folder and a file inside it can both be listed.
        ops.delete_files(&p, &paths(&["a.tex", "dir", "dir/b.tex", "c.tex"])).unwrap();
        for gone in ["a.tex", "dir/b.tex", "c.tex"] {
            assert!(ops.read(&p, gone).is_err(), "{gone} should be gone");
        }
        ops.undo(&p).unwrap();
        for (path, text) in [("a.tex", "A"), ("dir/b.tex", "B"), ("c.tex", "C")] {
            assert_eq!(ops.read(&p, path).unwrap()["content"], text);
        }
        assert!(ops.delete_files(&p, &[]).is_err());
    }

    #[test]
    fn write_then_undo_restores_the_old_text() {
        let (_dir, ops, p) = ops();
        ops.write_file(&p, "notes.tex", "first").unwrap();
        ops.write_file(&p, "notes.tex", "second").unwrap();
        ops.undo(&p).unwrap();
        let read = ops.read(&p, "notes.tex").unwrap();
        assert_eq!(read["content"], "first");
        ops.redo(&p).unwrap();
        assert_eq!(ops.read(&p, "notes.tex").unwrap()["content"], "second");
    }

    #[test]
    fn edit_requires_a_unique_match() {
        let (_dir, ops, p) = ops();
        ops.write_file(&p, "a.tex", "cat dog cat").unwrap();
        assert!(ops.edit1(&p, "a.tex", "cat", "bird", false).is_err());
        assert!(ops.edit1(&p, "a.tex", "fish", "bird", false).is_err());
        ops.edit1(&p, "a.tex", "dog", "owl", false).unwrap();
        ops.edit1(&p, "a.tex", "cat", "bird", true).unwrap();
        assert_eq!(ops.read(&p, "a.tex").unwrap()["content"], "bird owl bird");
    }

    #[test]
    fn delete_waits_for_approval_and_is_denied_on_timeout() {
        let (_dir, ops, p) = ops();
        ops.write_file(&p, "keep.tex", "x").unwrap();
        let denied = ops.delete1(&p, "keep.tex").unwrap_err();
        assert!(denied.contains("did not approve"), "{denied}");
        assert!(ops.read(&p, "keep.tex").is_ok(), "file must survive");
    }

    #[test]
    fn delete_runs_in_a_trusted_project_and_can_be_undone() {
        let (dir, ops, p) = ops();
        let mut project = ops.open(&p).unwrap();
        project.meta.trusted = true;
        project.save_meta().unwrap();
        ops.write_file(&p, "gone.tex", "bye").unwrap();
        ops.delete1(&p, "gone.tex").unwrap();
        assert!(ops.read(&p, "gone.tex").is_err());
        ops.undo(&p).unwrap();
        assert_eq!(ops.read(&p, "gone.tex").unwrap()["content"], "bye");
        let log = fs::read_to_string(dir.path().join("projects").join(&p).join(".moss/audit.log")).unwrap();
        assert!(log.contains("delete_file"));
    }

    #[test]
    fn paths_cannot_escape_the_project() {
        let (_dir, ops, p) = ops();
        assert!(ops.write_file(&p, "../evil.tex", "x").is_err());
        assert!(ops.read(&p, ".moss/project.json").is_err());
        assert!(ops.move_file(&p, "main.tex", "../x.tex").is_err());
    }

    #[test]
    fn search_finds_lines_case_insensitively() {
        let (_dir, ops, p) = ops();
        ops.write_file(&p, "a.tex", "Hello World\nbye").unwrap();
        let find = |query: &str, case_sensitive: bool, regex: bool| SearchOptions {
            query: query.into(),
            case_sensitive,
            regex,
            ..Default::default()
        };
        let hits = ops.search(&p, &find("world", false, false)).unwrap();
        assert_eq!(hits["matches"][0]["line"], 1);
        assert!(ops.search(&p, &find("world", true, false)).unwrap()["matches"].as_array().unwrap().is_empty());
        assert_eq!(ops.search(&p, &find("W.rld", false, true)).unwrap()["total_matches"], 1);
    }

    #[test]
    fn move_refuses_to_overwrite() {
        let (_dir, ops, p) = ops();
        ops.write_file(&p, "a.tex", "a").unwrap();
        ops.write_file(&p, "b.tex", "b").unwrap();
        assert!(ops.move_file(&p, "a.tex", "b.tex").is_err());
        ops.move_file(&p, "a.tex", "sub/c.tex").unwrap();
        assert_eq!(ops.read(&p, "sub/c.tex").unwrap()["content"], "a");
    }

    #[test]
    fn enabling_shell_escape_needs_approval() {
        let (_dir, ops, p) = ops();
        assert!(ops.set_shell_escape(&p, true).is_err());
        assert!(ops.set_shell_escape(&p, false).is_ok());
    }

    #[test]
    fn a_practice_project_syncs_through_the_agent_tools_with_approval_and_conflicts() {
        let (dir, ops, _) = ops();
        let made = moss_core::sync::link::create_practice_sample(&ops.workspace, "Try").unwrap();
        let p = made.project.dir_name.clone();
        assert_eq!(ops.overleaf_status(&p).unwrap()["practice"], true);

        // Overleaf side edits one file; the agent edits another.
        let practice = dir.path().join(".moss/practice/Try");
        fs::write(practice.join("chapters/method.tex"), "\\section{Method}\nOverleaf edit.\n").unwrap();
        ops.write_file(&p, "refs.bib", "@book{x}\n").unwrap();

        let review = ops.overleaf_review(&p, "sync").unwrap();
        assert_eq!(review["summary"]["pull"], 1);
        assert_eq!(review["summary"]["push"], 1);

        // Pull needs no approval and writes nothing to Overleaf.
        let pulled = ops.overleaf_sync(&p, "pull", vec![], false).unwrap();
        assert_eq!(pulled["applied"], true);
        assert!(fs::read_to_string(dir.path().join("projects").join(&p).join("chapters/method.tex"))
            .unwrap()
            .contains("Overleaf edit"));
        assert!(fs::read_to_string(practice.join("refs.bib")).unwrap().contains("@book{knuth"));

        // Push waits for an approval nobody gives.
        let denied = ops.overleaf_sync(&p, "push", vec![], false).unwrap_err();
        assert!(denied.contains("did not approve"), "{denied}");
        assert!(fs::read_to_string(practice.join("refs.bib")).unwrap().contains("@book{knuth"));

        // In a trusted Project it goes through.
        let mut project = ops.open(&p).unwrap();
        project.meta.trusted = true;
        project.save_meta().unwrap();
        let pushed = ops.overleaf_sync(&p, "push", vec![], false).unwrap();
        assert_eq!(pushed["applied"], true);
        assert_eq!(fs::read_to_string(practice.join("refs.bib")).unwrap(), "@book{x}\n");

        // A conflict is reported, not guessed.
        ops.write_file(&p, "chapters/intro.tex", "mine\n").unwrap();
        fs::write(practice.join("chapters/intro.tex"), "theirs\n").unwrap();
        let stopped = ops.overleaf_sync(&p, "sync", vec![], false).unwrap();
        assert_eq!(stopped["applied"], false);
        assert_eq!(stopped["conflicts"][0]["path"], "chapters/intro.tex");
        let done = ops
            .overleaf_sync(
                &p,
                "sync",
                vec![ResolutionArg {
                    path: "chapters/intro.tex".into(),
                    choice: "text".into(),
                    text: Some("merged\n".into()),
                    regions: None,
                }],
                false,
            )
            .unwrap();
        assert_eq!(done["applied"], true);
        assert_eq!(fs::read_to_string(practice.join("chapters/intro.tex")).unwrap(), "merged\n");

        // The log names who did what.
        let list = ops.history_list(&p).unwrap();
        let text = list.to_string();
        assert!(text.contains("\"Agent\"") && text.contains("\"Overleaf\""), "{text}");
    }

    #[test]
    fn local_projects_have_no_overleaf_tools_to_run() {
        let (_dir, ops, p) = ops();
        assert_eq!(ops.overleaf_status(&p).unwrap()["linked"], false);
        assert!(ops.overleaf_review(&p, "sync").is_err());
        assert!(parse_mode("fetch").is_err());
    }

    #[test]
    fn linking_and_unlinking_follow_the_approval_rules() {
        let (_dir, ops, p) = ops();
        assert!(ops.overleaf_link(&p, "sideways", None, None).is_err());
        assert!(ops.overleaf_link(&p, "existing", None, Some("Thesis")).is_err(), "id is required");
        assert!(ops.overleaf_link(&p, "existing", Some("abc123"), None).is_err(), "name is required");

        // Linking to an existing project writes nothing to Overleaf, so it needs no Approval.
        let linked = ops.overleaf_link(&p, "existing", Some("abc123"), Some("Thesis")).unwrap();
        assert_eq!(linked["linked"]["project_id"], "abc123");
        let status = ops.overleaf_status(&p).unwrap();
        assert_eq!(status["linked"], true);
        assert_eq!(status["practice"], false);
        assert_eq!(status["has_overleaf_history"], true);
        assert!(ops.overleaf_link(&p, "existing", Some("x"), Some("y")).is_err(), "already linked");

        // Unlinking drops the Base snapshot, so it waits for an Approval nobody gives.
        let denied = ops.overleaf_unlink(&p).unwrap_err();
        assert!(denied.contains("did not approve"), "{denied}");
        assert_eq!(ops.overleaf_status(&p).unwrap()["linked"], true);

        let mut project = ops.open(&p).unwrap();
        project.meta.trusted = true;
        project.save_meta().unwrap();
        assert_eq!(ops.overleaf_unlink(&p).unwrap()["was"], "Thesis");
        assert_eq!(ops.overleaf_status(&p).unwrap()["linked"], false);
        assert!(ops.overleaf_unlink(&p).is_err(), "nothing left to unlink");
    }

    #[test]
    fn conflicts_are_checked_region_by_region_and_resolved_with_one_answer_each() {
        let (dir, ops, _) = ops();
        let made = moss_core::sync::link::create_practice_sample(&ops.workspace, "Try").unwrap();
        let p = made.project.dir_name.clone();
        let practice = dir.path().join(".moss/practice/Try");
        let mut project = ops.open(&p).unwrap();
        project.meta.trusted = true;
        project.save_meta().unwrap();

        // Settle a five-line file on both sides first.
        ops.write_file(&p, "notes.tex", "l1\nl2\nl3\nl4\nl5\n").unwrap();
        ops.overleaf_sync(&p, "push", vec![], false).unwrap();
        assert!(ops.overleaf_conflicts(&p, "sync").unwrap()["has_conflicts"] == false);

        // This PC edits line 1 and line 3; Overleaf edits only line 3.
        ops.write_file(&p, "notes.tex", "ONE\nl2\nMINE\nl4\nl5\n").unwrap();
        fs::write(practice.join("notes.tex"), "l1\nl2\nTHEIRS\nl4\nl5\n").unwrap();

        let checked = ops.overleaf_conflicts(&p, "sync").unwrap();
        assert_eq!(checked["conflict_count"], 1);
        let regions = checked["conflicts"][0]["regions"].as_array().unwrap();
        assert_eq!(regions.len(), 1, "line 1 merges on its own; only line 3 overlaps");
        assert_eq!(regions[0]["mine"], "MINE\n");
        assert_eq!(regions[0]["theirs"], "THEIRS\n");
        assert!(regions[0]["before"].as_str().unwrap().contains("l2"));
        assert!(regions[0]["after"].as_str().unwrap().contains("l4"));

        // A mistyped choice or a stray path is refused instead of guessed.
        let bad = |choice: &str, path: &str| {
            ops.overleaf_sync(
                &p,
                "sync",
                vec![ResolutionArg { path: path.into(), choice: choice.into(), text: None, regions: None }],
                false,
            )
            .unwrap_err()
        };
        assert!(bad("Mine", "notes.tex").contains("must be"));
        assert!(bad("text", "notes.tex").contains("needs `text`"));
        assert!(bad("regions", "notes.tex").contains("needs `regions`"));
        assert!(bad("mine", "other.tex").contains("not a conflict"));
        assert_eq!(fs::read_to_string(practice.join("notes.tex")).unwrap(), "l1\nl2\nTHEIRS\nl4\nl5\n");

        // The wrong number of answers writes nothing; the right one keeps both sides.
        let region = |choice: &str| RegionArg { choice: choice.into(), text: None };
        let resolve = |regions: Vec<RegionArg>| {
            ops.overleaf_sync(
                &p,
                "sync",
                vec![ResolutionArg { path: "notes.tex".into(), choice: "regions".into(), text: None, regions: Some(regions) }],
                false,
            )
        };
        assert!(resolve(vec![]).is_err());
        assert_eq!(fs::read_to_string(practice.join("notes.tex")).unwrap(), "l1\nl2\nTHEIRS\nl4\nl5\n");
        assert_eq!(resolve(vec![region("both")]).unwrap()["applied"], true);
        let merged = "ONE\nl2\nMINE\nTHEIRS\nl4\nl5\n";
        assert_eq!(fs::read_to_string(practice.join("notes.tex")).unwrap(), merged);
        assert_eq!(ops.read(&p, "notes.tex").unwrap()["content"], merged);
        assert_eq!(ops.overleaf_conflicts(&p, "sync").unwrap()["has_conflicts"], false);
    }

    // ----- Looking at the result -----

    /// Three pages (A4, A4, landscape), written by hand. Poppler rebuilds the xref table itself.
    const THREE_PAGES: &str = "%PDF-1.4\n1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n\
2 0 obj<</Type/Pages/Kids[3 0 R 4 0 R 5 0 R]/Count 3>>endobj\n\
3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 595 842]>>endobj\n\
4 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 595 842]>>endobj\n\
5 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 842 595]>>endobj\n\
trailer<</Root 1 0 R/Size 6>>\n%%EOF\n";

    /// A Project with a ready-made PDF in its build folder, or `None` without Poppler.
    fn with_pdf() -> Option<(tempfile::TempDir, Ops, String)> {
        let (dir, ops, p) = ops();
        if ops.pdf_tools().is_err() {
            eprintln!("skipped: no Poppler tools");
            return None;
        }
        let project = ops.open(&p).unwrap();
        let pdf = ops.pdf_path(&project);
        fs::create_dir_all(pdf.parent().unwrap()).unwrap();
        fs::write(&pdf, THREE_PAGES).unwrap();
        Some((dir, ops, p))
    }

    fn render<'a>(pages: &'a str, layout: &'a str) -> RenderArgs<'a> {
        RenderArgs { pages, layout, columns: None, dpi: None, rebuild: false }
    }

    fn is_png(image: &Image) -> bool {
        image.bytes.starts_with(&[0x89, b'P', b'N', b'G'])
    }

    #[test]
    fn pages_render_as_one_contact_sheet_or_as_separate_images() {
        let Some((_dir, ops, p)) = with_pdf() else { return };

        let info = ops.pdf_info(&p).unwrap();
        assert_eq!(info["pages"], 3);
        assert_eq!(info["page_sizes_pt"][2][0], 842.0, "page 3 is landscape");

        // Several pages default to one image: a 2-column grid, page numbers by row.
        let sheet = ops.render_pages(&p, render("all", "")).unwrap();
        assert_eq!(sheet.images.len(), 1);
        assert!(is_png(&sheet.images[0]));
        assert_eq!(sheet.info["layout"], "one_image");
        assert_eq!(sheet.info["columns"], 2);
        assert_eq!(sheet.info["grid_rows"], json!([[1, 2], [3]]));
        assert!(sheet.images[0].width <= 3200, "fitted to the width limit: {}", sheet.images[0].width);

        // The same pages, one image each, in the order asked for.
        let each = ops.render_pages(&p, render("1,3", "separate")).unwrap();
        assert_eq!(each.images.len(), 2);
        assert!(each.images[1].width > each.images[1].height, "the landscape page stays landscape");
        assert_eq!(each.info["pages_shown"], json!([1, 3]));

        // One page by default, sharper than a contact sheet cell.
        let one = ops.render_pages(&p, render("2", "")).unwrap();
        assert_eq!(one.info["layout"], "separate");
        assert!(one.info["dpi"].as_u64().unwrap() >= sheet.info["dpi"].as_u64().unwrap());

        // Columns and dpi are honoured.
        let narrow = ops
            .render_pages(&p, RenderArgs { columns: Some(1), dpi: Some(50), ..render("1-3", "one_image") })
            .unwrap();
        assert_eq!(narrow.info["grid_rows"], json!([[1], [2], [3]]));
        assert_eq!(narrow.info["dpi"], 50);
    }

    #[test]
    fn bad_page_requests_explain_themselves() {
        let Some((_dir, ops, p)) = with_pdf() else { return };
        let e = |pages: &str, layout: &str| ops.render_pages(&p, render(pages, layout)).err().unwrap();
        assert!(e("9", "").contains("3 page"));
        assert!(e("0", "").contains("not a page"));
        assert!(e("1", "sideways").contains("layout must be"));
        // A Project with no PDF, when compiling is not allowed.
        let blank = ops.create_project("Blank", None).unwrap();
        let blank = blank["dir_name"].as_str().unwrap();
        assert!(ops.render_pages(blank, render("1", "")).err().unwrap().contains("no PDF yet"));
    }

    #[test]
    fn a_pdf_older_than_the_sources_is_reported_stale() {
        let Some((_dir, ops, p)) = with_pdf() else { return };
        let project = ops.open(&p).unwrap();
        let pdf = ops.pdf_path(&project);
        let old = std::time::SystemTime::now() - Duration::from_secs(3600);
        fs::File::options().write(true).open(&pdf).unwrap().set_modified(old).unwrap();
        assert_eq!(ops.pdf_info(&p).unwrap()["stale"], true);
        let shown = ops.render_pages(&p, render("1", "")).unwrap();
        assert_eq!(shown.info["stale"], true);
        assert!(shown.info["note"].as_str().unwrap().contains("rebuild"));
        // Generated files never make a PDF stale.
        fs::write(project.dir().join("main.aux"), "x").unwrap();
        fs::File::options().write(true).open(&pdf).unwrap().set_modified(std::time::SystemTime::now()).unwrap();
        assert_eq!(ops.pdf_info(&p).unwrap()["stale"], false);
    }

    #[test]
    fn figure_files_can_be_viewed_but_nothing_else() {
        let (_dir, ops, p) = ops();
        let project = ops.open(&p).unwrap();
        let pixel = pdfview::Raster { width: 2, height: 2, rgb: vec![200; 12] };
        fs::create_dir_all(project.dir().join("figs")).unwrap();
        fs::write(project.dir().join("figs/a.PNG"), pdfview::encode_png(&pixel).unwrap()).unwrap();
        let seen = ops.view_image(&p, "figs/a.PNG").unwrap();
        assert_eq!(seen.images[0].mime, "image/png");
        assert!(is_png(&seen.images[0]));
        assert!(ops.view_image(&p, "main.tex").err().unwrap().contains("not a PNG"));
        assert!(ops.view_image(&p, "figs/none.png").err().unwrap().contains("does not exist"));
        assert!(ops.view_image(&p, "../x.png").is_err());
    }

    #[test]
    fn diagnostics_are_read_from_the_log_and_clean_build_removes_only_generated_files() {
        let (_dir, ops, p) = ops();
        assert!(ops.diagnostics(&p).unwrap_err().contains("not been compiled"));

        let project = ops.open(&p).unwrap();
        let build = project.build_dir("main.tex");
        fs::create_dir_all(&build).unwrap();
        fs::write(
            build.join("main.log"),
            "./main.tex:8: Undefined control sequence.\nLaTeX Warning: Reference `x' on page 1 undefined on input line 12.\n\
Overfull \\hbox (9pt too wide) in paragraph at lines 20--22\n",
        )
        .unwrap();
        let d = ops.diagnostics(&p).unwrap();
        assert_eq!((d["error_count"].as_u64(), d["warning_count"].as_u64(), d["layout_count"].as_u64()), (Some(1), Some(1), Some(1)));
        assert_eq!(d["errors"][0]["line"], 8);
        assert_eq!(d["layout"][0]["line"], 20);

        assert_eq!(ops.clean_build(&p).unwrap()["cleaned"], true);
        assert!(!build.exists());
        assert!(project.dir().join("main.tex").is_file(), "sources stay");
        assert_eq!(ops.clean_build(&p).unwrap()["cleaned"], false, "cleaning twice is fine");
    }

    #[test]
    fn every_template_creates_a_project_whose_structure_reads_cleanly() {
        let (_dir, ops, _) = ops();
        let listed = ops.list_templates().unwrap();
        let ids: Vec<&str> = listed["templates"].as_array().unwrap().iter().map(|t| t["id"].as_str().unwrap()).collect();
        assert_eq!(ids, ["article", "paper", "report", "thesis", "beamer", "letter", "cv", "unicode"]);
        for id in ids {
            let made = ops.create_project(&format!("T {id}"), Some(id)).unwrap();
            let p = made["dir_name"].as_str().unwrap();
            let outline = ops.outline(p).unwrap();
            assert!(outline["missing_inputs"].as_array().unwrap().is_empty(), "{id}");
            let refs = ops.references(p).unwrap();
            assert_eq!(refs["problem_count"], 0, "{id}: {refs}");
        }
        let e = ops.create_project("Bad", Some("novel")).unwrap_err();
        assert!(e.contains("unknown template") && e.contains("thesis"), "{e}");
        // No template: the one-line starter.
        let plain = ops.create_project("Plain", None).unwrap();
        assert_eq!(ops.outline(plain["dir_name"].as_str().unwrap()).unwrap()["headings"], json!([]));
    }

    /// Needs a TeX install (and the network the first time, for packages), so it is run by hand:
    /// `cargo test -p moss-mcp -- --ignored all_templates_compile`. It caught a missing package in
    /// the report template that no offline check could.
    #[test]
    #[ignore = "compiles every template with the real TeX install"]
    fn all_templates_compile_without_errors_and_can_be_viewed() {
        let (_dir, ops, _) = ops();
        if ops.tex_install().is_err() {
            eprintln!("skipped: no TeX installation");
            return;
        }
        for t in templates::all() {
            let made = ops.create_project(&format!("Real {}", t.id), Some(t.id)).unwrap();
            let p = made["dir_name"].as_str().unwrap();
            let built = ops.compile(p).unwrap();
            let diag = ops.diagnostics(p).unwrap();
            assert_eq!(built["success"], true, "{}: {}", t.id, diag["errors"]);
            assert_eq!(diag["error_count"], 0, "{}: {}", t.id, diag["errors"]);
            let pages = built["pages"].as_u64().unwrap_or_else(|| panic!("{}: no page count", t.id));
            assert!(pages >= 1, "{}", t.id);
            let seen = ops.render_pages(p, render("all", "")).unwrap();
            assert!(!seen.images.is_empty() && is_png(&seen.images[0]), "{}", t.id);
            assert_eq!(seen.info["stale"], false, "{}", t.id);
        }
    }

    #[test]
    fn the_report_template_outline_follows_its_chapters() {
        let (_dir, ops, _) = ops();
        let made = ops.create_project("R", Some("report")).unwrap();
        let p = made["dir_name"].as_str().unwrap();
        let outline = ops.outline(p).unwrap();
        let titles: Vec<&str> = outline["headings"].as_array().unwrap().iter().map(|h| h["title"].as_str().unwrap()).collect();
        assert_eq!(&titles[..3], ["Introduction", "Background", "Objectives"]);
        assert!(titles.contains(&"Conclusion") && titles.contains(&"Raw Data"));
        let files = outline["files"].as_array().unwrap();
        assert_eq!(files[0], "main.tex");
        assert_eq!(files.len(), 6);
        // Breaking a reference is caught without compiling.
        ops.write_file(p, "chapters/extra.tex", "See \\ref{nowhere} and \\cite{ghost}.\n").unwrap();
        ops.edit1(p, "main.tex", "\\include{chapters/conclusion}", "\\include{chapters/conclusion}\n\\include{chapters/extra}", false).unwrap();
        let refs = ops.references(p).unwrap();
        assert_eq!(refs["undefined_refs"][0]["name"], "nowhere");
        assert_eq!(refs["undefined_cites"][0]["name"], "ghost");
        assert_eq!(refs["problem_count"], 2);
    }

    #[test]
    fn linking_checks_the_id_against_the_account() {
        let project = |id: &str, name: &str, trashed: bool| overleaf::RemoteProject {
            id: id.into(),
            name: name.into(),
            last_updated: None,
            access_level: None,
            owner: None,
            archived: false,
            trashed,
        };
        let projects = vec![project("a1", "Thesis", false), project("b2", "Old", true)];
        assert_eq!(find_overleaf_project(&projects, "a1").unwrap().name, "Thesis");
        assert!(find_overleaf_project(&projects, "zzz").unwrap_err().contains("No Overleaf project"));
        assert!(find_overleaf_project(&projects, "b2").unwrap_err().contains("trash"));
    }

    #[test]
    fn overleaf_history_needs_a_real_link() {
        let (_dir, ops, p) = ops();
        assert!(ops.overleaf_history(&p, None).unwrap_err().contains("not linked"));

        let made = moss_core::sync::link::create_practice_sample(&ops.workspace, "Try").unwrap();
        let practice = made.project.dir_name;
        assert_eq!(ops.overleaf_status(&practice).unwrap()["has_overleaf_history"], false);
        let e = ops.overleaf_history(&practice, None).unwrap_err();
        assert!(e.contains("practice"), "{e}");
    }

    #[test]
    fn saving_a_version_and_restoring_one_file() {
        let (_dir, ops, p) = ops();
        ops.write_file(&p, "a.tex", "one").unwrap();
        let saved = ops.history_save(&p, "First draft").unwrap();
        let id = saved["saved"]["id"].as_u64().unwrap();
        assert!(ops.history_save(&p, "Again").unwrap()["saved"].is_null(), "already saved");
        ops.write_file(&p, "a.tex", "two").unwrap();
        let latest = ops.history_list(&p).unwrap().as_array().unwrap().last().unwrap()["id"].as_u64().unwrap();
        let shown = ops.history_show(&p, latest).unwrap();
        assert!(shown["files"].to_string().contains("a.tex"));
        ops.history_restore_file(&p, id, "a.tex").unwrap();
        assert_eq!(ops.read(&p, "a.tex").unwrap()["content"], "one");
    }

    #[test]
    fn an_old_version_can_be_read_and_compared_without_restoring_it() {
        let (_dir, ops, p) = ops();
        ops.write_file(&p, "a.tex", "one\ntwo\nthree\n").unwrap();
        let id = ops.history_save(&p, "First").unwrap()["saved"]["id"].as_u64().unwrap();
        ops.write_file(&p, "a.tex", "one\n2\nthree\n").unwrap();

        let old = ops.history_read_file(&p, id, "a.tex", Some(2), Some(2)).unwrap();
        assert_eq!(old["content"], "two\n");
        assert_eq!(ops.read(&p, "a.tex").unwrap()["content"], "one\n2\nthree\n", "working copy untouched");
        assert!(ops.history_read_file(&p, id, "nope.tex", None, None).is_err());

        let diff = ops.history_diff(&p, &id.to_string(), "working", Some("a.tex")).unwrap();
        let text = diff["diff"].as_str().unwrap();
        assert!(text.contains("-two") && text.contains("+2"), "{text}");
        let files = ops.history_diff(&p, &id.to_string(), "working", None).unwrap();
        assert!(files["files"].to_string().contains("a.tex"));
        assert!(ops.history_diff(&p, "abc", "working", None).is_err());
    }

    #[test]
    fn a_write_reports_size_and_new_balance_problems() {
        let (_dir, ops, p) = ops();
        ops.write_file(&p, "a.tex", "\\textbf{a}\n").unwrap();
        let edit = ops.edit1(&p, "a.tex", "\\textbf{a}", "\\textbf{a", false).unwrap();
        assert_eq!(edit["integrity"]["lines_after"], 1);
        assert!(edit["integrity"]["warnings"][0].as_str().unwrap().contains("braces"), "{edit}");
        let clean = ops.edit1(&p, "a.tex", "\\textbf{a", "\\textbf{a}", false).unwrap();
        assert!(clean["integrity"]["warnings"].is_null());
    }

    #[test]
    fn changes_made_behind_the_tools_are_kept_and_reported_on_the_next_call() {
        let (dir, ops, p) = ops();
        ops.tracked(|o| o.write_file(&p, "a.tex", "one")).unwrap();
        // Something else (the app, another agent) writes the file.
        let project_dir = dir.path().join("projects").join(&p);
        std::thread::sleep(Duration::from_millis(20));
        fsutil::write_text(&project_dir, "a.tex", "one, edited elsewhere").unwrap();

        let seen = ops.tracked(|o| o.read(&p, "a.tex")).unwrap();
        let note = &seen["project_changed_outside"];
        assert_eq!(note["modified"][0], "a.tex", "{seen}");
        let id = note["restore_point"].as_u64().expect("kept as a restore point");
        let kinds: Vec<Value> = ops.history_list(&p).unwrap().as_array().unwrap().clone();
        assert!(kinds.iter().any(|v| v["id"] == id && v["kind"] == "external"), "{kinds:?}");

        let again = ops.tracked(|o| o.read(&p, "a.tex")).unwrap();
        assert!(again.get("project_changed_outside").is_none(), "reported once");
        let audit = fs::read_to_string(project_dir.join(".moss/audit.log")).unwrap();
        assert!(audit.contains("external_change"));
    }
}
