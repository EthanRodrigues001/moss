//! The MCP surface: each tool validates its arguments and hands off to `ops`.

use std::sync::Arc;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use rmcp::{
    ServerHandler,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{CallToolResult, ContentBlock, ServerCapabilities, ServerConfig},
    tool, tool_handler, tool_router,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::ops::{EditSpec, FileSpec, MediaResult, OpResult, Ops, RenderArgs, ResolutionArg};

const INSTRUCTIONS: &str = "\
Moss edits LaTeX Projects stored as folders on the user's PC. Every Project is named by its \
`project` folder name: call `list_projects` first and pass that name to the other tools. \
Paths are relative to the Project and use `/`. \
Every change you make takes a restore point first, so `undo`, `redo` and `restore` can bring \
anything back, including deleted files. Deleting files and enabling shell-escape wait for the \
user to click Approve in the Moss app (unless the Project is trusted); if they decline, nothing \
changes.\n\
Reading: `list_files` shows every file's size. Read small files whole, and several at once with \
`read_file` `paths`. For a long file, find the line with `search` or `outline`, then `read_file` \
with `start_line` and `end_line`; a read stops at about 60,000 characters and returns \
`next_start_line` to carry on from. \
Writing: `edit_file` takes several `edits` for one file in one call (all or nothing), \
`write_files` writes several files in one call and `delete_file` takes several `paths`. Each \
call is one restore point and one Approval, so batch related changes. Use `edit_file` for \
changes inside a file and `write_file` for a new file or a full rewrite.\n\
Run `compile` after editing; `diagnostics` re-reads the last compile's errors, warnings and badly \
set boxes. To see the result, `view_pages` returns compiled pages as images: one page, a range \
(`2-5`), a list (`1,3,5-7`), `last` or `all`, laid out in one image (a contact sheet, good for \
layout and flow) or as separate images (good for reading text; one or two pages per image read \
best). `pdf_info` gives the page count. `source_to_pdf` finds which page a source line landed \
on and `pdf_to_source` goes back. `outline` lists sections across included files, and \
`references` finds broken `\\ref` and `\\cite`, unused labels and bibliography entries. \
`create_project` can start from a template (`list_templates`: article, paper, report, thesis, \
beamer, letter, cv, unicode). `view_image` shows a figure file from the Project.\n\
Overleaf: `overleaf_status` says whether the Project is linked, whether the user is signed in, \
and what differs from the last Sync. To link a Local Project, find the Overleaf project with \
`overleaf_projects`, then `overleaf_link` (how=existing) or create one from the files \
(how=new, needs Approval); `overleaf_unlink` undoes it. For a linked Project, `overleaf_review` \
shows what a pull, push or sync would do (it changes nothing). When the user asks you to pull, \
push or sync, do it with `overleaf_pull`, `overleaf_push` or `overleaf_sync`; never sync unasked. \
Check first with `overleaf_conflicts`: edits to different lines merge on their own, and only \
edits to the same lines conflict. Pull never needs approval; push and sync wait for the user's \
Approval (skipped in a trusted Project). If there are conflicts the call stops and lists them \
region by region with this PC's text (mine) and Overleaf's (theirs); read both, decide, and \
answer all of them with `overleaf_resolve`. If a choice is a judgement call about the user's \
writing, ask the user instead of guessing. Push never overwrites Overleaf changes it has not \
merged, so pull first when it reports files left alone. `overleaf_history` is Overleaf's own \
record of who edited what, and \
`history_list` is the Project's version log on this PC (each entry names its author: You, \
Agent, Overleaf or Moss). `history_read_file` and `history_diff` let you look at an old \
version before deciding to restore it.\n\
If a result has `project_changed_outside`, files changed since your last call and not through \
these tools (the user typing in Moss, or another program). They were saved as a restore point \
first; re-read what you rely on before editing it.";

/// The tools by group, in the order Settings shows them. Every tool must be listed exactly once
/// (a test checks it), so Settings can never show a tool without a group.
const GROUPS: &[(&str, &[&str])] = &[
    (
        "Projects",
        &["list_projects", "create_project", "list_templates", "import_project", "project_info"],
    ),
    ("Reading", &["list_files", "read_file", "search", "outline", "references"]),
    ("Writing", &["write_file", "write_files", "edit_file", "move_file", "delete_file"]),
    (
        "Build",
        &["compile", "diagnostics", "set_compile_options", "set_shell_escape", "clean_build"],
    ),
    ("Preview", &["view_pages", "pdf_info", "view_image", "source_to_pdf", "pdf_to_source"]),
    (
        "History",
        &[
            "history_list",
            "history_show",
            "history_changes",
            "history_read_file",
            "history_diff",
            "history_save",
            "history_restore_file",
            "restore",
            "undo",
            "redo",
        ],
    ),
    (
        "Overleaf",
        &[
            "overleaf_status",
            "overleaf_projects",
            "overleaf_history",
            "overleaf_link",
            "overleaf_unlink",
            "overleaf_review",
            "overleaf_conflicts",
            "overleaf_pull",
            "overleaf_push",
            "overleaf_resolve",
            "overleaf_sync",
        ],
    ),
];

/// Tools that can wait for the user's click in the Moss app (unless the Project is trusted).
const NEEDS_APPROVAL: &[&str] = &[
    "delete_file",
    "set_shell_escape",
    "overleaf_link",
    "overleaf_unlink",
    "overleaf_push",
    "overleaf_resolve",
    "overleaf_sync",
];

/// One tool as Settings shows it.
#[derive(Serialize)]
pub struct ToolInfo {
    pub name: String,
    pub title: String,
    pub description: String,
    pub group: &'static str,
    /// Does not change the Project (or anything else).
    pub read_only: bool,
    /// Can delete or overwrite things that are not restorable from the Project's History.
    pub destructive: bool,
    /// Talks to Overleaf over the network.
    pub online: bool,
    /// May wait for the user's click in the Moss app.
    pub needs_approval: bool,
}

/// Every tool, grouped and in display order, read from the tool declarations below.
pub fn catalog() -> Vec<ToolInfo> {
    let tools = MossServer::tool_router().list_all();
    let mut out = Vec::new();
    for (group, names) in GROUPS {
        for name in *names {
            let Some(tool) = tools.iter().find(|t| t.name == *name) else { continue };
            let hints = tool.annotations.as_ref();
            out.push(ToolInfo {
                name: tool.name.to_string(),
                title: tool.title.clone().unwrap_or_else(|| tool.name.to_string()),
                description: tool.description.as_deref().unwrap_or_default().to_string(),
                group,
                read_only: hints.and_then(|h| h.read_only_hint).unwrap_or(false),
                destructive: hints.and_then(|h| h.destructive_hint).unwrap_or(false),
                online: hints.and_then(|h| h.open_world_hint).unwrap_or(false),
                needs_approval: NEEDS_APPROVAL.contains(name),
            });
        }
    }
    out
}

#[derive(Clone)]
pub struct MossServer {
    ops: Arc<Ops>,
    tool_router: ToolRouter<Self>,
    /// Tools the user switched off in Settings.
    disabled: Arc<Vec<String>>,
}

#[derive(Deserialize, JsonSchema)]
pub struct CreateArgs {
    /// Name for the new Project, for example "Thesis".
    name: String,
    /// Optional starter document from `list_templates`: article, paper, report, thesis, beamer,
    /// letter, cv or unicode. Without it the Project starts with a one-line main.tex.
    template: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct ViewPagesArgs {
    /// Project folder name from `list_projects`.
    project: String,
    /// Which pages: "3", "2-5", "4-" (to the end), "last", "all" or "1,3,5-7". Defaults to "1".
    pages: Option<String>,
    /// "one_image" puts every requested page in one picture, in a grid (the default for several
    /// pages); "separate" returns one image per page.
    layout: Option<String>,
    /// Pages per row in a one_image layout. Defaults to a roughly square grid of at most 4.
    columns: Option<u32>,
    /// Resolution. Defaults to a size that fits the pages in the image; 40-300.
    dpi: Option<u32>,
    /// Compile first when the PDF is missing or older than the sources. Defaults to true.
    rebuild: Option<bool>,
}

#[derive(Deserialize, JsonSchema)]
pub struct SourceToPdfArgs {
    /// Project folder name from `list_projects`.
    project: String,
    /// Source file relative to the Project, for example "chapters/method.tex".
    file: String,
    /// 1-based line in that file.
    line: u32,
    /// Optional 1-based column.
    column: Option<u32>,
}

#[derive(Deserialize, JsonSchema)]
pub struct PdfToSourceArgs {
    /// Project folder name from `list_projects`.
    project: String,
    /// 1-based page.
    page: u32,
    /// Horizontal position in PDF points from the left edge of the page.
    x: f64,
    /// Vertical position in PDF points from the top edge of the page.
    y: f64,
}

#[derive(Deserialize, JsonSchema)]
pub struct ImportArgs {
    /// Absolute path on this PC to a .zip file or to a folder.
    path: String,
    /// Optional name for the new Project; defaults to the zip or folder name.
    name: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct ProjectArgs {
    /// Project folder name from `list_projects`.
    project: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct PathArgs {
    /// Project folder name from `list_projects`.
    project: String,
    /// File or folder path relative to the Project, for example "chapters/intro.tex".
    path: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct ReadArgs {
    /// Project folder name from `list_projects`.
    project: String,
    /// One file path relative to the Project, for example "chapters/intro.tex".
    path: Option<String>,
    /// Several file paths to read in one call (up to 20) instead of `path`. They share one output
    /// budget; a file that does not fit is cut on a line boundary or skipped.
    paths: Option<Vec<String>>,
    /// First line to return, 1-based. Only with `path`. Defaults to 1.
    start_line: Option<u32>,
    /// Last line to return, inclusive. Only with `path`. Defaults to the end of the file.
    end_line: Option<u32>,
    /// Most characters to return, 1,000 to 200,000. Defaults to 60,000. When the file is longer
    /// the result says `truncated` and gives `next_start_line` to continue from.
    max_chars: Option<u32>,
}

#[derive(Deserialize, JsonSchema)]
pub struct WriteArgs {
    /// Project folder name from `list_projects`.
    project: String,
    /// File path relative to the Project. Missing folders are created.
    path: String,
    /// The complete new content of the file.
    content: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct WriteFilesArgs {
    /// Project folder name from `list_projects`.
    project: String,
    /// The files to write, up to 50. Each one is created or fully replaced.
    files: Vec<FileSpec>,
}

#[derive(Deserialize, JsonSchema)]
pub struct EditArgs {
    /// Project folder name from `list_projects`.
    project: String,
    /// File path relative to the Project.
    path: String,
    /// Several replacements to make in this file, applied in order. All or nothing.
    edits: Option<Vec<EditSpec>>,
    /// For a single replacement: exact existing text. Must match once unless `replace_all`.
    old_text: Option<String>,
    /// For a single replacement: the new text (may be empty to delete the old text).
    new_text: Option<String>,
    /// For a single replacement: replace every occurrence instead of requiring a unique match.
    replace_all: Option<bool>,
}

#[derive(Deserialize, JsonSchema)]
pub struct DeleteArgs {
    /// Project folder name from `list_projects`.
    project: String,
    /// One file or folder path relative to the Project.
    path: Option<String>,
    /// Several files or folders to delete in one call (up to 100), with one Approval, instead of `path`.
    paths: Option<Vec<String>>,
}

#[derive(Deserialize, JsonSchema)]
pub struct MoveArgs {
    /// Project folder name from `list_projects`.
    project: String,
    /// Existing path.
    from: String,
    /// New path. Must not exist yet.
    to: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct SearchArgs {
    /// Project folder name from `list_projects`.
    project: String,
    /// Text to look for (plain text unless `regex` is set).
    query: String,
    /// Match letter case exactly. Defaults to false.
    case_sensitive: Option<bool>,
    /// Match whole words only. Defaults to false.
    whole_word: Option<bool>,
    /// Treat `query` as a regular expression (Rust syntax). Defaults to false.
    regex: Option<bool>,
    /// Only search files matching these globs, separated by commas or spaces, e.g.
    /// `*.tex, chapters/**, !old`. A leading `!` excludes. Defaults to every text file.
    include: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct SnapshotArgs {
    /// Project folder name from `list_projects`.
    project: String,
    /// Snapshot id from `history_list`.
    snapshot_id: u64,
}

#[derive(Deserialize, JsonSchema)]
pub struct SaveArgs {
    /// Project folder name from `list_projects`.
    project: String,
    /// Short description of this version, like a commit message.
    message: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct RestoreFileArgs {
    /// Project folder name from `list_projects`.
    project: String,
    /// Snapshot id from `history_list`.
    snapshot_id: u64,
    /// File path relative to the Project.
    path: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct HistoryReadArgs {
    /// Project folder name from `list_projects`.
    project: String,
    /// Snapshot id from `history_list`.
    snapshot_id: u64,
    /// File path relative to the Project.
    path: String,
    /// First line to read (1-based). Defaults to the start.
    start_line: Option<usize>,
    /// Last line to read, inclusive. Defaults to the end.
    end_line: Option<usize>,
}

#[derive(Deserialize, JsonSchema)]
pub struct HistoryDiffArgs {
    /// Project folder name from `list_projects`.
    project: String,
    /// The older side: a snapshot id from `history_list`, `working` (the files now) or `empty`.
    a: String,
    /// The newer side, in the same form. Use `working` to compare a version with the files now.
    b: String,
    /// File to diff. Leave out to list which files differ.
    path: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct ReviewArgs {
    /// Project folder name from `list_projects`.
    project: String,
    /// "pull" (bring Overleaf's changes here), "push" (send local changes) or "sync" (both).
    mode: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct OverleafHistoryArgs {
    /// Project folder name from `list_projects`.
    project: String,
    /// `next_before` from the previous result, to get older entries.
    before: Option<i64>,
}

#[derive(Deserialize, JsonSchema)]
pub struct LinkArgs {
    /// Project folder name from `list_projects`.
    project: String,
    /// "existing" (an Overleaf project that is already there) or "new" (create one from these files).
    how: String,
    /// Overleaf project id from `overleaf_projects`; required for "existing".
    overleaf_id: Option<String>,
    /// Overleaf project name from `overleaf_projects`; required for "existing".
    name: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct ConflictsArgs {
    /// Project folder name from `list_projects`.
    project: String,
    /// "pull", "push" or "sync". Defaults to "sync", which shows every conflict.
    mode: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct SyncModeArgs {
    /// Project folder name from `list_projects`.
    project: String,
    /// Confirm deleting many files (only after the user agreed).
    allow_mass_delete: Option<bool>,
}

#[derive(Deserialize, JsonSchema)]
pub struct ResolveArgs {
    /// Project folder name from `list_projects`.
    project: String,
    /// The mode that reported the conflicts: "pull", "push" or "sync".
    mode: String,
    /// One answer per conflicting file.
    resolutions: Vec<ResolutionArg>,
    /// Confirm deleting many files (only after the user agreed).
    allow_mass_delete: Option<bool>,
}

#[derive(Deserialize, JsonSchema)]
pub struct SyncArgs {
    /// Project folder name from `list_projects`.
    project: String,
    /// "pull", "push" or "sync".
    mode: String,
    /// Answers for the conflicts a previous call reported.
    resolutions: Option<Vec<ResolutionArg>>,
    /// Confirm deleting many files (only after the user agreed).
    allow_mass_delete: Option<bool>,
}

#[derive(Deserialize, JsonSchema)]
pub struct CompileOptionsArgs {
    /// Project folder name from `list_projects`.
    project: String,
    /// New Root file path (the .tex file with \documentclass).
    root_file: Option<String>,
    /// One of "auto", "pdflatex", "xelatex", "lualatex".
    engine: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct ShellEscapeArgs {
    /// Project folder name from `list_projects`.
    project: String,
    /// Allow documents to run programs. Turning this on needs the user's approval.
    enabled: bool,
}

/// What a tool that takes `path` or `paths` was given.
enum Paths {
    One(String),
    Many(Vec<String>),
}

fn paths_arg(path: Option<String>, paths: Option<Vec<String>>) -> Result<Paths, String> {
    match (path, paths) {
        (Some(p), None) => Ok(Paths::One(p)),
        (None, Some(v)) => Ok(Paths::Many(v)),
        (Some(_), Some(_)) => Err("give either `path` or `paths`, not both".into()),
        (None, None) => Err("give `path` (one file) or `paths` (several)".into()),
    }
}

fn edits_arg(a: EditArgs) -> Result<Vec<EditSpec>, String> {
    match (a.edits, a.old_text, a.new_text) {
        (Some(edits), None, None) => Ok(edits),
        (None, Some(old_text), Some(new_text)) => {
            Ok(vec![EditSpec { old_text, new_text, replace_all: a.replace_all }])
        }
        (None, Some(_), None) | (None, None, Some(_)) => {
            Err("a single replacement needs both `old_text` and `new_text`".into())
        }
        (None, None, None) => Err("give `edits`, or `old_text` and `new_text`".into()),
        _ => Err("use either `edits` or `old_text`/`new_text`, not both".into()),
    }
}

#[tool_router(router = tool_router)]
impl MossServer {
    pub fn new(ops: Ops, disabled: Vec<String>) -> Self {
        let mut tool_router = Self::tool_router();
        for name in &disabled {
            tool_router.disable_route(name.clone());
        }
        Self {
            ops: Arc::new(ops),
            tool_router,
            disabled: Arc::new(disabled),
        }
    }

    async fn run(
        &self,
        work: impl FnOnce(&Ops) -> OpResult + Send + 'static,
    ) -> Result<String, String> {
        let ops = Arc::clone(&self.ops);
        let value = tokio::task::spawn_blocking(move || ops.tracked(work))
            .await
            .map_err(|e| format!("tool crashed: {e}"))??;
        serde_json::to_string_pretty(&value).map_err(|e| e.to_string())
    }

    /// Like `run`, for tools that answer with pictures: the description first, then each image.
    async fn run_media(
        &self,
        work: impl FnOnce(&Ops) -> MediaResult + Send + 'static,
    ) -> Result<CallToolResult, String> {
        let ops = Arc::clone(&self.ops);
        let media = tokio::task::spawn_blocking(move || ops.tracked_media(work))
            .await
            .map_err(|e| format!("tool crashed: {e}"))??;
        let text = serde_json::to_string_pretty(&media.info).map_err(|e| e.to_string())?;
        let mut blocks = vec![ContentBlock::text(text)];
        blocks.extend(media.images.iter().map(|i| ContentBlock::image(STANDARD.encode(&i.bytes), i.mime)));
        Ok(CallToolResult::success(blocks))
    }

    async fn sync_with(&self, mode: &'static str, a: SyncModeArgs) -> Result<String, String> {
        self.run(move |o| {
            o.overleaf_sync(&a.project, mode, Vec::new(), a.allow_mass_delete.unwrap_or(false))
        })
        .await
    }

    // ----- Projects -----

    #[tool(
        name = "list_projects",
        title = "List projects",
        description = "List every Project with its folder name, Root file and Origin (local or overleaf).",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn list_projects(&self) -> Result<String, String> {
        self.run(|o| o.list_projects()).await
    }

    #[tool(
        name = "create_project",
        title = "Create a project",
        description = "Create a new Local Project: empty with a starter main.tex, or from a template (article, paper, report, thesis, beamer, letter, cv, unicode; see list_templates).",
        annotations(read_only_hint = false, destructive_hint = false, open_world_hint = false)
    )]
    async fn create_project(&self, Parameters(a): Parameters<CreateArgs>) -> Result<String, String> {
        self.run(move |o| o.create_project(&a.name, a.template.as_deref())).await
    }

    #[tool(
        name = "list_templates",
        title = "List templates",
        description = "The starter documents `create_project` can use, with what each contains.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn list_templates(&self) -> Result<String, String> {
        self.run(|o| o.list_templates()).await
    }

    #[tool(
        name = "import_project",
        title = "Import a project",
        description = "Import a .zip file or a folder from this PC as a new Local Project.",
        annotations(read_only_hint = false, destructive_hint = false, open_world_hint = false)
    )]
    async fn import_project(&self, Parameters(a): Parameters<ImportArgs>) -> Result<String, String> {
        self.run(move |o| o.import(&a.path, a.name.as_deref())).await
    }

    #[tool(
        name = "project_info",
        title = "Project details",
        description = "Details of one Project: path, Origin, Root file, engine, shell-escape, trusted, history size.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn project_info(&self, Parameters(a): Parameters<ProjectArgs>) -> Result<String, String> {
        self.run(move |o| o.project_info(&a.project)).await
    }

    // ----- Reading -----

    #[tool(
        name = "list_files",
        title = "List files",
        description = "List every file and folder in a Project with its size (generated files are hidden). Use the sizes to decide what to read whole and what to read by line range.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn list_files(&self, Parameters(a): Parameters<ProjectArgs>) -> Result<String, String> {
        self.run(move |o| o.list_files(&a.project)).await
    }

    #[tool(
        name = "read_file",
        title = "Read files",
        description = "Read text files from a Project. `path` reads one file, whole or a line range (`start_line`, `end_line`); `paths` reads several files in one call. Output stops at `max_chars` (60,000 by default) on a line boundary and returns `total_lines`, `truncated` and `next_start_line` so a long file can be read part by part.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn read_file(&self, Parameters(a): Parameters<ReadArgs>) -> Result<String, String> {
        let max = a.max_chars.map(|m| m as usize);
        match paths_arg(a.path, a.paths)? {
            Paths::One(path) => {
                let (start, end) = (a.start_line.map(|n| n as usize), a.end_line.map(|n| n as usize));
                self.run(move |o| o.read_file(&a.project, &path, start, end, max)).await
            }
            Paths::Many(paths) => {
                if a.start_line.is_some() || a.end_line.is_some() {
                    return Err("`start_line` and `end_line` only work with a single `path`".into());
                }
                self.run(move |o| o.read_files(&a.project, &paths, max)).await
            }
        }
    }

    #[tool(
        name = "search",
        title = "Search text",
        description = "Find text across a Project's text files, as plain text or a regular expression, optionally whole-word or limited to some files. Returns path, line and text; read around a hit with `read_file` and a line range.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn search(&self, Parameters(a): Parameters<SearchArgs>) -> Result<String, String> {
        self.run(move |o| {
            let options = moss_core::search::SearchOptions {
                query: a.query,
                case_sensitive: a.case_sensitive.unwrap_or(false),
                whole_word: a.whole_word.unwrap_or(false),
                regex: a.regex.unwrap_or(false),
                include: a.include.unwrap_or_default(),
            };
            o.search(&a.project, &options)
        })
        .await
    }

    #[tool(
        name = "outline",
        title = "Document outline",
        description = "The document's sections (part, chapter, section, subsection) across all included files in order, with file and line; also the labels and any missing \\input files.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn outline(&self, Parameters(a): Parameters<ProjectArgs>) -> Result<String, String> {
        self.run(move |o| o.outline(&a.project)).await
    }

    #[tool(
        name = "references",
        title = "Check references",
        description = "Check cross-references and citations from the sources: \\ref with no \\label, \\cite with no bibliography entry, duplicates, unused labels and unused bibliography entries. Lists every .bib entry with its key, title, author and year.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn references(&self, Parameters(a): Parameters<ProjectArgs>) -> Result<String, String> {
        self.run(move |o| o.references(&a.project)).await
    }

    // ----- Writing -----

    #[tool(
        name = "write_file",
        title = "Write a file",
        description = "Create a file or replace its whole content. A restore point is taken first; writing identical content changes nothing. For several files use `write_files`.",
        annotations(read_only_hint = false, destructive_hint = false, idempotent_hint = true, open_world_hint = false)
    )]
    async fn write_file(&self, Parameters(a): Parameters<WriteArgs>) -> Result<String, String> {
        self.run(move |o| o.write_file(&a.project, &a.path, &a.content)).await
    }

    #[tool(
        name = "write_files",
        title = "Write several files",
        description = "Create or replace up to 50 files in one call: one restore point before and one Version after, instead of two per file. Files that already have this content are left alone.",
        annotations(read_only_hint = false, destructive_hint = false, idempotent_hint = true, open_world_hint = false)
    )]
    async fn write_files(&self, Parameters(a): Parameters<WriteFilesArgs>) -> Result<String, String> {
        self.run(move |o| o.write_files(&a.project, &a.files)).await
    }

    #[tool(
        name = "edit_file",
        title = "Edit a file",
        description = "Replace exact text in a file without resending the rest of it. Pass `edits` (a list of old_text/new_text, applied in order, all or nothing) or a single old_text and new_text. Each old_text must match exactly once unless replace_all is set; a failed edit changes nothing.",
        annotations(read_only_hint = false, destructive_hint = false, idempotent_hint = false, open_world_hint = false)
    )]
    async fn edit_file(&self, Parameters(a): Parameters<EditArgs>) -> Result<String, String> {
        let project = a.project.clone();
        let path = a.path.clone();
        let edits = edits_arg(a)?;
        self.run(move |o| o.edit_file(&project, &path, &edits)).await
    }

    #[tool(
        name = "move_file",
        title = "Move or rename",
        description = "Rename or move a file or folder. Fails if the destination exists.",
        annotations(read_only_hint = false, destructive_hint = false, idempotent_hint = false, open_world_hint = false)
    )]
    async fn move_file(&self, Parameters(a): Parameters<MoveArgs>) -> Result<String, String> {
        self.run(move |o| o.move_file(&a.project, &a.from, &a.to)).await
    }

    #[tool(
        name = "delete_file",
        title = "Delete files",
        description = "Delete a file or folder (`path`), or several at once (`paths`, up to 100, one Approval). Needs the user's approval in the Moss app unless the Project is trusted; undo brings it all back. Nothing is deleted if any path is missing.",
        annotations(read_only_hint = false, destructive_hint = true, idempotent_hint = false, open_world_hint = false)
    )]
    async fn delete_file(&self, Parameters(a): Parameters<DeleteArgs>) -> Result<String, String> {
        let paths = match paths_arg(a.path, a.paths)? {
            Paths::One(p) => vec![p],
            Paths::Many(v) => v,
        };
        self.run(move |o| o.delete_files(&a.project, &paths)).await
    }

    // ----- Build -----

    #[tool(
        name = "compile",
        title = "Compile",
        description = "Compile the Project's Root file with the user's TeX install. Returns success, errors and warnings with file and line, the PDF path, page count, a summary of overfull and underfull boxes (with the file), and any undefined citations or references. If latexmk replays an old failure it is rebuilt from scratch (`forced_rebuild`).",
        annotations(read_only_hint = false, destructive_hint = false, idempotent_hint = true, open_world_hint = false)
    )]
    async fn compile(&self, Parameters(a): Parameters<ProjectArgs>) -> Result<String, String> {
        self.run(move |o| o.compile(&a.project)).await
    }

    #[tool(
        name = "diagnostics",
        title = "Errors and warnings",
        description = "The last compile's errors and warnings (with file and line) and its overfull/underfull boxes, read from the log without compiling again.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn diagnostics(&self, Parameters(a): Parameters<ProjectArgs>) -> Result<String, String> {
        self.run(move |o| o.diagnostics(&a.project)).await
    }

    #[tool(
        name = "set_compile_options",
        title = "Set compile options",
        description = "Change the Root file and/or the engine (auto, pdflatex, xelatex, lualatex).",
        annotations(read_only_hint = false, destructive_hint = false, idempotent_hint = true, open_world_hint = false)
    )]
    async fn set_compile_options(
        &self,
        Parameters(a): Parameters<CompileOptionsArgs>,
    ) -> Result<String, String> {
        self.run(move |o| o.set_compile_options(&a.project, a.root_file.as_deref(), a.engine.as_deref()))
            .await
    }

    #[tool(
        name = "set_shell_escape",
        title = "Allow shell-escape",
        description = "Allow or forbid documents running programs while compiling. Enabling needs the user's approval.",
        annotations(read_only_hint = false, destructive_hint = false, idempotent_hint = true, open_world_hint = false)
    )]
    async fn set_shell_escape(
        &self,
        Parameters(a): Parameters<ShellEscapeArgs>,
    ) -> Result<String, String> {
        self.run(move |o| o.set_shell_escape(&a.project, a.enabled)).await
    }

    #[tool(
        name = "clean_build",
        title = "Clean build files",
        description = "Delete the generated build files so the next compile starts from scratch. Sources are not touched.",
        annotations(read_only_hint = false, destructive_hint = false, idempotent_hint = true, open_world_hint = false)
    )]
    async fn clean_build(&self, Parameters(a): Parameters<ProjectArgs>) -> Result<String, String> {
        self.run(move |o| o.clean_build(&a.project)).await
    }

    // ----- Preview -----

    #[tool(
        name = "view_pages",
        title = "View pages",
        description = "See the compiled document: pages as images. pages = one page, a range (2-5), a list (1,3,5-7), last or all. layout one_image puts them in one picture (a contact sheet to judge layout and flow; the default for several pages), separate gives one image per page (best for reading text: one or two pages at a time). Compiles first if the PDF is missing or out of date.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn view_pages(&self, Parameters(a): Parameters<ViewPagesArgs>) -> Result<CallToolResult, String> {
        self.run_media(move |o| {
            o.render_pages(
                &a.project,
                RenderArgs {
                    pages: a.pages.as_deref().unwrap_or("1"),
                    layout: a.layout.as_deref().unwrap_or(""),
                    columns: a.columns.map(|c| c as usize),
                    dpi: a.dpi,
                    rebuild: a.rebuild.unwrap_or(true),
                },
            )
        })
        .await
    }

    #[tool(
        name = "pdf_info",
        title = "PDF info",
        description = "Page count, page sizes and whether the PDF is out of date, for the last compile.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn pdf_info(&self, Parameters(a): Parameters<ProjectArgs>) -> Result<String, String> {
        self.run(move |o| o.pdf_info(&a.project)).await
    }

    #[tool(
        name = "view_image",
        title = "View an image",
        description = "Look at an image file from the Project (PNG, JPEG, GIF or WebP, up to 8 MB), such as a figure.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn view_image(&self, Parameters(a): Parameters<PathArgs>) -> Result<CallToolResult, String> {
        self.run_media(move |o| o.view_image(&a.project, &a.path)).await
    }

    #[tool(
        name = "source_to_pdf",
        title = "Source line to PDF page",
        description = "Which page and position a source line landed on in the compiled PDF (SyncTeX). Then use view_pages on that page.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn source_to_pdf(&self, Parameters(a): Parameters<SourceToPdfArgs>) -> Result<String, String> {
        self.run(move |o| o.source_to_pdf(&a.project, &a.file, a.line, a.column)).await
    }

    #[tool(
        name = "pdf_to_source",
        title = "PDF point to source line",
        description = "Which source file and line produced a point on a PDF page (SyncTeX). x and y are PDF points from the top-left corner.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn pdf_to_source(&self, Parameters(a): Parameters<PdfToSourceArgs>) -> Result<String, String> {
        self.run(move |o| o.pdf_to_source(&a.project, a.page, a.x, a.y)).await
    }

    // ----- History -----

    #[tool(
        name = "history_list",
        title = "List restore points",
        description = "List restore points (History snapshots), oldest first. `current` marks the one matching the files now.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn history_list(&self, Parameters(a): Parameters<ProjectArgs>) -> Result<String, String> {
        self.run(move |o| o.history_list(&a.project)).await
    }

    #[tool(
        name = "history_show",
        title = "Show a version",
        description = "The files one version changed, with lines added and removed.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn history_show(&self, Parameters(a): Parameters<SnapshotArgs>) -> Result<String, String> {
        self.run(move |o| o.history_show(&a.project, a.snapshot_id)).await
    }

    #[tool(
        name = "history_changes",
        title = "Changes since a version",
        description = "Files added, modified or deleted since a restore point.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn history_changes(&self, Parameters(a): Parameters<SnapshotArgs>) -> Result<String, String> {
        self.run(move |o| o.history_changes(&a.project, a.snapshot_id)).await
    }

    #[tool(
        name = "history_read_file",
        title = "Read a file from a version",
        description = "Read a file as it was in a restore point, with the same line range options as `read_file`. Changes nothing, so look here before `history_restore_file`.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn history_read_file(&self, Parameters(a): Parameters<HistoryReadArgs>) -> Result<String, String> {
        self.run(move |o| o.history_read_file(&a.project, a.snapshot_id, &a.path, a.start_line, a.end_line))
            .await
    }

    #[tool(
        name = "history_diff",
        title = "Compare two versions",
        description = "Compare two states, each a snapshot id, `working` (the files now) or `empty`. With `path`, returns a unified diff of that file; without, the list of files that differ with line counts.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn history_diff(&self, Parameters(a): Parameters<HistoryDiffArgs>) -> Result<String, String> {
        self.run(move |o| o.history_diff(&a.project, &a.a, &a.b, a.path.as_deref())).await
    }

    #[tool(
        name = "history_save",
        title = "Save a version",
        description = "Save the current files as a named version in the Project's log (like a commit).",
        annotations(read_only_hint = false, destructive_hint = false, open_world_hint = false)
    )]
    async fn history_save(&self, Parameters(a): Parameters<SaveArgs>) -> Result<String, String> {
        self.run(move |o| o.history_save(&a.project, &a.message)).await
    }

    #[tool(
        name = "history_restore_file",
        title = "Restore one file",
        description = "Put one file back to how it was in a version. The current state is saved first, so this can be undone.",
        annotations(read_only_hint = false, destructive_hint = true, idempotent_hint = true, open_world_hint = false)
    )]
    async fn history_restore_file(&self, Parameters(a): Parameters<RestoreFileArgs>) -> Result<String, String> {
        self.run(move |o| o.history_restore_file(&a.project, a.snapshot_id, &a.path)).await
    }

    #[tool(
        name = "restore",
        title = "Restore the project",
        description = "Put the Project back to any restore point. The current state is saved first, so this can be undone too.",
        annotations(read_only_hint = false, destructive_hint = true, idempotent_hint = true, open_world_hint = false)
    )]
    async fn restore(&self, Parameters(a): Parameters<SnapshotArgs>) -> Result<String, String> {
        self.run(move |o| o.restore(&a.project, a.snapshot_id)).await
    }

    #[tool(
        name = "undo",
        title = "Undo",
        description = "Step back to the state before the latest change.",
        annotations(read_only_hint = false, destructive_hint = true, idempotent_hint = false, open_world_hint = false)
    )]
    async fn undo(&self, Parameters(a): Parameters<ProjectArgs>) -> Result<String, String> {
        self.run(move |o| o.undo(&a.project)).await
    }

    #[tool(
        name = "redo",
        title = "Redo",
        description = "Step forward again after an undo.",
        annotations(read_only_hint = false, destructive_hint = true, idempotent_hint = false, open_world_hint = false)
    )]
    async fn redo(&self, Parameters(a): Parameters<ProjectArgs>) -> Result<String, String> {
        self.run(move |o| o.redo(&a.project)).await
    }

    // ----- Overleaf -----

    #[tool(
        name = "overleaf_status",
        title = "Overleaf status",
        description = "Whether the Project is linked to Overleaf, when it last synced, and which files changed here since.",
        annotations(read_only_hint = true, open_world_hint = true)
    )]
    async fn overleaf_status(&self, Parameters(a): Parameters<ProjectArgs>) -> Result<String, String> {
        self.run(move |o| o.overleaf_status(&a.project)).await
    }

    #[tool(
        name = "overleaf_projects",
        title = "List Overleaf projects",
        description = "The signed-in user's Overleaf projects (id, name, owner, last updated), to pick one for `overleaf_link`.",
        annotations(read_only_hint = true, open_world_hint = true)
    )]
    async fn overleaf_projects(&self) -> Result<String, String> {
        self.run(|o| o.overleaf_projects()).await
    }

    #[tool(
        name = "overleaf_history",
        title = "Overleaf history",
        description = "Overleaf's own change history for a linked Project: who edited, created, deleted or renamed which files, newest first. Read-only. Pass `next_before` from the result as `before` for older entries.",
        annotations(read_only_hint = true, open_world_hint = true)
    )]
    async fn overleaf_history(&self, Parameters(a): Parameters<OverleafHistoryArgs>) -> Result<String, String> {
        self.run(move |o| o.overleaf_history(&a.project, a.before)).await
    }

    #[tool(
        name = "overleaf_link",
        title = "Link to Overleaf",
        description = "Link a Local Project to Overleaf: how=existing (with overleaf_id and name from `overleaf_projects`; the first sync then asks about every differing file) or how=new (uploads the files as a new Overleaf project; needs the user's Approval).",
        annotations(read_only_hint = false, destructive_hint = false, open_world_hint = true)
    )]
    async fn overleaf_link(&self, Parameters(a): Parameters<LinkArgs>) -> Result<String, String> {
        self.run(move |o| o.overleaf_link(&a.project, &a.how, a.overleaf_id.as_deref(), a.name.as_deref()))
            .await
    }

    #[tool(
        name = "overleaf_unlink",
        title = "Unlink from Overleaf",
        description = "Stop syncing a Project with Overleaf and make it Local again. Files are untouched. Needs the user's Approval.",
        annotations(read_only_hint = false, destructive_hint = false, idempotent_hint = true, open_world_hint = true)
    )]
    async fn overleaf_unlink(&self, Parameters(a): Parameters<ProjectArgs>) -> Result<String, String> {
        self.run(move |o| o.overleaf_unlink(&a.project)).await
    }

    #[tool(
        name = "overleaf_review",
        title = "Review a sync",
        description = "What a pull, push or sync would do: files to write, upload, merge or delete, and conflicts with both texts. Changes nothing.",
        annotations(read_only_hint = true, open_world_hint = true)
    )]
    async fn overleaf_review(&self, Parameters(a): Parameters<ReviewArgs>) -> Result<String, String> {
        self.run(move |o| o.overleaf_review(&a.project, &a.mode)).await
    }

    #[tool(
        name = "overleaf_conflicts",
        title = "Check for conflicts",
        description = "Check for merge conflicts before syncing. Fetches Overleaf and changes nothing. Lists each conflicting file as numbered regions (base, mine = this PC, theirs = Overleaf, with surrounding lines), plus the files Moss merges on its own. Edits to different lines never conflict.",
        annotations(read_only_hint = true, open_world_hint = true)
    )]
    async fn overleaf_conflicts(&self, Parameters(a): Parameters<ConflictsArgs>) -> Result<String, String> {
        self.run(move |o| o.overleaf_conflicts(&a.project, a.mode.as_deref().unwrap_or("sync")))
            .await
    }

    #[tool(
        name = "overleaf_pull",
        title = "Pull from Overleaf",
        description = "Bring Overleaf's changes to this PC, merging with local edits line by line. Never writes to Overleaf, so no Approval. Stops and lists conflicts if any need an answer; then use `overleaf_resolve`.",
        annotations(read_only_hint = false, destructive_hint = false, idempotent_hint = false, open_world_hint = true)
    )]
    async fn overleaf_pull(&self, Parameters(a): Parameters<SyncModeArgs>) -> Result<String, String> {
        self.sync_with("pull", a).await
    }

    #[tool(
        name = "overleaf_push",
        title = "Push to Overleaf",
        description = "Send this PC's changes to Overleaf. Never overwrites an Overleaf change it has not merged: such files are skipped and reported, so pull first. Needs the user's Approval unless the Project is trusted. Use only when the user asked to push.",
        annotations(read_only_hint = false, destructive_hint = false, idempotent_hint = false, open_world_hint = true)
    )]
    async fn overleaf_push(&self, Parameters(a): Parameters<SyncModeArgs>) -> Result<String, String> {
        self.sync_with("push", a).await
    }

    #[tool(
        name = "overleaf_resolve",
        title = "Resolve conflicts",
        description = "Settle the conflicts that `overleaf_conflicts` or a stopped pull/push/sync listed, then finish the sync. `resolutions` has one entry per conflicting file: choice mine (this PC), theirs (Overleaf), text (the full final text) or regions (one answer per region: mine, theirs, both or text). Answer every conflict in one call. Push and sync need Approval.",
        annotations(read_only_hint = false, destructive_hint = false, idempotent_hint = false, open_world_hint = true)
    )]
    async fn overleaf_resolve(&self, Parameters(a): Parameters<ResolveArgs>) -> Result<String, String> {
        if a.resolutions.is_empty() {
            return Err("`resolutions` is empty; call `overleaf_conflicts` to see what needs an answer".into());
        }
        self.run(move |o| {
            o.overleaf_sync(&a.project, &a.mode, a.resolutions, a.allow_mass_delete.unwrap_or(false))
        })
        .await
    }

    #[tool(
        name = "overleaf_sync",
        title = "Sync with Overleaf",
        description = "Pull and push in one go (mode sync), or pull / push with `mode`. Stops with the conflicts listed if any file needs a decision; call `overleaf_resolve` (or this tool again with `resolutions`). Push and sync need the user's Approval.",
        annotations(read_only_hint = false, destructive_hint = false, idempotent_hint = false, open_world_hint = true)
    )]
    async fn overleaf_sync(&self, Parameters(a): Parameters<SyncArgs>) -> Result<String, String> {
        self.run(move |o| {
            o.overleaf_sync(
                &a.project,
                &a.mode,
                a.resolutions.unwrap_or_default(),
                a.allow_mass_delete.unwrap_or(false),
            )
        })
        .await
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for MossServer {
    fn get_info(&self) -> ServerConfig {
        let mut text = INSTRUCTIONS.to_string();
        if !self.disabled.is_empty() {
            text.push_str(&format!(
                "\nThe user has switched off these tools in Moss Settings, so they are not available: {}.",
                self.disabled.join(", ")
            ));
        }
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build()).with_instructions(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tool_is_grouped_titled_and_hinted() {
        let tools = MossServer::tool_router().list_all();
        let grouped: Vec<&str> = GROUPS.iter().flat_map(|(_, names)| names.iter().copied()).collect();
        for tool in &tools {
            assert!(grouped.contains(&&*tool.name), "{} has no group", tool.name);
            assert!(tool.title.as_deref().is_some_and(|t| !t.is_empty()), "{} has no title", tool.name);
            let hints = tool.annotations.as_ref().unwrap_or_else(|| panic!("{} has no hints", tool.name));
            assert!(hints.read_only_hint.is_some(), "{} does not say whether it is read-only", tool.name);
            assert!(hints.open_world_hint.is_some(), "{} does not say whether it is online", tool.name);
            if hints.read_only_hint == Some(false) {
                assert!(hints.destructive_hint.is_some(), "{} does not say whether it is destructive", tool.name);
            }
        }
        for name in &grouped {
            assert!(tools.iter().any(|t| t.name == *name), "{name} is grouped but is not a tool");
        }
        assert_eq!(grouped.len(), tools.len(), "a tool is grouped twice");
        for name in NEEDS_APPROVAL {
            assert!(grouped.contains(name), "{name} is in NEEDS_APPROVAL but is not a tool");
        }
    }

    #[test]
    fn catalog_lists_every_tool_in_group_order() {
        let catalog = catalog();
        assert_eq!(catalog.len(), MossServer::tool_router().list_all().len());
        assert_eq!(catalog[0].group, "Projects");
        let delete = catalog.iter().find(|t| t.name == "delete_file").unwrap();
        assert!(delete.destructive && delete.needs_approval && !delete.read_only);
        let read = catalog.iter().find(|t| t.name == "read_file").unwrap();
        assert!(read.read_only && !read.needs_approval && !read.online);
        assert!(catalog.iter().find(|t| t.name == "overleaf_pull").unwrap().online);
    }

    #[test]
    fn a_tool_switched_off_in_settings_is_hidden_and_the_agent_is_told() {
        let dir = tempfile::tempdir().unwrap();
        let workspace = moss_core::workspace::Workspace::open(dir.path()).unwrap();
        let server = MossServer::new(Ops::new(workspace, None), vec!["delete_file".into(), "overleaf_push".into()]);
        let visible = server.tool_router.list_all();
        assert!(visible.iter().all(|t| t.name != "delete_file" && t.name != "overleaf_push"));
        assert_eq!(visible.len(), MossServer::tool_router().list_all().len() - 2);
        let info = server.get_info();
        let text = info.instructions.unwrap_or_default();
        assert!(text.contains("switched off") && text.contains("delete_file, overleaf_push"), "{text}");
    }

    #[test]
    fn instructions_name_only_real_tools() {
        let names: Vec<String> = MossServer::tool_router().list_all().iter().map(|t| t.name.to_string()).collect();
        // Every `backticked_word` in the instructions that looks like a tool name must exist.
        for word in INSTRUCTIONS.split('`').skip(1).step_by(2) {
            let looks_like_tool = word.contains('_') && word.chars().all(|c| c.is_ascii_lowercase() || c == '_');
            let is_argument = ["start_line", "end_line", "next_start_line", "project_changed_outside"].contains(&word);
            if looks_like_tool && !is_argument {
                assert!(names.iter().any(|n| n == word), "instructions mention `{word}`, which is not a tool");
            }
        }
    }
}
