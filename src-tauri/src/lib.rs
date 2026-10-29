//! The Moss desktop app: a thin Tauri layer over `moss-core`.
//!
//! Commands take a Project's `dir_name` (its folder inside `Workspace/projects`) and never a raw
//! path, so the UI cannot point the core at arbitrary places on disk.

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use moss_core::compile::{self, CompileRequest, Diagnostic};
use moss_core::fsutil::{self, FileEntry};
use moss_core::settings::Settings;
use moss_core::synctex;
use moss_core::tex::{self, TexInstall, detect_engine};
use moss_core::workspace::{ImportReport, ProjectSummary, Workspace};
use serde::Serialize;
use tauri::{Manager, State, ipc::Response};

mod agents;
mod chrome_bridge;
mod overleaf;
mod preferences;
mod search;
mod setup;
mod tree;
mod versions;

pub(crate) struct AppState {
    pub(crate) workspace: Workspace,
    pub(crate) tex: Mutex<Option<TexInstall>>,
    pub(crate) overleaf: Mutex<Option<moss_core::overleaf::Session>>,
    pub(crate) bridge: Mutex<Option<std::sync::Arc<chrome_bridge::Bridge>>>,
    /// The open Review of each Project (what Overleaf and this PC held when it was made).
    pub(crate) staged: Mutex<std::collections::HashMap<String, moss_core::sync::Staged>>,
}

pub(crate) type CmdResult<T> = std::result::Result<T, String>;

pub(crate) fn err(e: moss_core::Error) -> String {
    e.to_string()
}

/// Run blocking core work off the UI thread.
pub(crate) async fn blocking<T, F>(f: F) -> CmdResult<T>
where
    T: Send + 'static,
    F: FnOnce() -> CmdResult<T> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| format!("background task failed: {e}"))?
}

#[derive(Serialize)]
pub(crate) struct TexStatus {
    found: bool,
    install: Option<TexInstall>,
    problem: Option<String>,
}

#[derive(Serialize)]
struct WorkspaceInfo {
    root: PathBuf,
}

#[derive(Serialize)]
struct CompileSummary {
    success: bool,
    timed_out: bool,
    duration_ms: u64,
    has_pdf: bool,
    engine: String,
    diagnostics: Vec<Diagnostic>,
    console_tail: String,
}

pub(crate) fn detect_tex(state: &AppState) -> TexStatus {
    match tex::detect(Settings::load().tex_bin_dir.as_deref()) {
        Ok(install) => {
            let problem = install
                .perl
                .is_none()
                .then(|| "Perl was not found; latexmk needs it".to_string());
            *state.tex.lock().unwrap() = Some(install.clone());
            TexStatus {
                found: problem.is_none(),
                install: Some(install),
                problem,
            }
        }
        Err(e) => TexStatus {
            found: false,
            install: None,
            problem: Some(e.to_string()),
        },
    }
}

/// The detected TeX install, detecting it first if nothing is cached yet.
fn tex_install(state: &AppState) -> CmdResult<TexInstall> {
    let cached = state.tex.lock().unwrap().clone();
    match cached {
        Some(install) => Ok(install),
        None => {
            let status = detect_tex(state);
            status
                .install
                .ok_or_else(|| status.problem.unwrap_or_else(|| "TeX not found".into()))
        }
    }
}

#[tauri::command]
fn tex_status(state: State<'_, AppState>) -> TexStatus {
    detect_tex(&state)
}

#[tauri::command]
fn workspace_info(state: State<'_, AppState>) -> WorkspaceInfo {
    WorkspaceInfo {
        root: state.workspace.root().to_path_buf(),
    }
}

#[tauri::command]
fn list_projects(state: State<'_, AppState>) -> CmdResult<Vec<ProjectSummary>> {
    state.workspace.list_projects().map_err(err)
}

#[tauri::command]
fn create_project(state: State<'_, AppState>, name: String) -> CmdResult<ProjectSummary> {
    state.workspace.create_project(name.trim()).map_err(err)
}

#[tauri::command]
fn project_set_archived(
    state: State<'_, AppState>,
    project: String,
    archived: bool,
) -> CmdResult<ProjectSummary> {
    state.workspace.set_archived(&project, archived).map_err(err)
}

#[tauri::command]
fn project_rename(state: State<'_, AppState>, project: String, name: String) -> CmdResult<ProjectSummary> {
    state.workspace.rename_project(&project, &name).map_err(err)
}

/// Delete a Project's folder from this PC. The UI asks the user to type the name first.
#[tauri::command]
async fn project_delete(state: State<'_, AppState>, project: String) -> CmdResult<()> {
    let ws = state.workspace.clone();
    let key = project.clone();
    blocking(move || ws.delete_project(&project).map_err(err)).await?;
    state.staged.lock().unwrap().remove(&key);
    Ok(())
}

#[tauri::command]
async fn import_zip(state: State<'_, AppState>, path: String) -> CmdResult<ImportReport> {
    let ws = state.workspace.clone();
    blocking(move || ws.import_zip(&PathBuf::from(path), None).map_err(err)).await
}

#[tauri::command]
async fn import_folder(state: State<'_, AppState>, path: String) -> CmdResult<ImportReport> {
    let ws = state.workspace.clone();
    blocking(move || ws.import_folder(&PathBuf::from(path), None).map_err(err)).await
}

#[tauri::command]
fn list_files(state: State<'_, AppState>, project: String) -> CmdResult<Vec<FileEntry>> {
    let p = state.workspace.open_project(&project).map_err(err)?;
    fsutil::list_files(p.dir()).map_err(err)
}

#[tauri::command]
fn read_text_file(state: State<'_, AppState>, project: String, path: String) -> CmdResult<String> {
    let p = state.workspace.open_project(&project).map_err(err)?;
    fsutil::read_text(p.dir(), &path).map_err(err)
}

/// A project file's raw bytes (images and PDFs for the preview), sent without JSON overhead.
#[tauri::command]
fn read_binary_file(state: State<'_, AppState>, project: String, path: String) -> CmdResult<Response> {
    const MAX_PREVIEW_BYTES: u64 = 64 * 1024 * 1024;
    let p = state.workspace.open_project(&project).map_err(err)?;
    let file = fsutil::safe_join(p.dir(), &path).map_err(err)?;
    let size = std::fs::metadata(&file).map(|m| m.len()).map_err(|e| e.to_string())?;
    if size > MAX_PREVIEW_BYTES {
        return Err(format!("{path} is {} MB, too large to preview", size / (1024 * 1024)));
    }
    fsutil::read_bytes(p.dir(), &path).map(Response::new).map_err(err)
}

#[tauri::command]
fn write_text_file(
    state: State<'_, AppState>,
    project: String,
    path: String,
    content: String,
) -> CmdResult<bool> {
    let p = state.workspace.open_project(&project).map_err(err)?;
    fsutil::write_text(p.dir(), &path, &content).map_err(err)
}

#[tauri::command]
async fn compile_project(state: State<'_, AppState>, project: String) -> CmdResult<CompileSummary> {
    let ws = state.workspace.clone();
    let install = tex_install(&state)?;
    blocking(move || {
        let p = ws.open_project(&project).map_err(err)?;
        let root = p.meta.root_file.clone();
        let source = fsutil::read_text(p.dir(), &root).map_err(err)?;
        let engine = p.meta.engine.unwrap_or_else(|| detect_engine(&source));
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
        let tail: Vec<&str> = out.console.lines().rev().take(40).collect();
        Ok(CompileSummary {
            success: out.success,
            timed_out: out.timed_out,
            duration_ms: out.duration.as_millis() as u64,
            has_pdf: out.pdf.is_some(),
            engine: format!("{engine:?}").to_lowercase(),
            diagnostics: out.diagnostics,
            console_tail: tail.into_iter().rev().collect::<Vec<_>>().join("\n"),
        })
    })
    .await
}

/// The latest PDF for the Project's Root file, sent as raw bytes rather than a JSON array.
#[tauri::command]
fn read_pdf(state: State<'_, AppState>, project: String) -> CmdResult<Response> {
    let p = state.workspace.open_project(&project).map_err(err)?;
    let stem = std::path::Path::new(&p.meta.root_file)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let pdf = p.build_dir(&p.meta.root_file).join(format!("{stem}.pdf"));
    std::fs::read(&pdf)
        .map(Response::new)
        .map_err(|_| "No PDF yet. Compile the project first.".to_string())
}

/// Where the PDF for the Project's Root file lives (it may not exist yet).
fn pdf_path(p: &moss_core::project::Project) -> PathBuf {
    let stem = std::path::Path::new(&p.meta.root_file)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    p.build_dir(&p.meta.root_file).join(format!("{stem}.pdf"))
}

/// Copy the compiled PDF to a place the user picked ("Save PDF as...").
#[tauri::command]
fn pdf_save_as(state: State<'_, AppState>, project: String, dest: String) -> CmdResult<()> {
    let p = state.workspace.open_project(&project).map_err(err)?;
    let src = pdf_path(&p);
    if !src.is_file() {
        return Err("No PDF yet. Compile the project first.".into());
    }
    std::fs::copy(&src, &dest)
        .map(|_| ())
        .map_err(|e| format!("Could not save the PDF: {e}"))
}

/// The compiled PDF's file name, and the folder holding it, for "Open PDF" in the default viewer.
#[tauri::command]
fn pdf_open(state: State<'_, AppState>, project: String) -> CmdResult<()> {
    let p = state.workspace.open_project(&project).map_err(err)?;
    let src = pdf_path(&p);
    if !src.is_file() {
        return Err("No PDF yet. Compile the project first.".into());
    }
    // `explorer <file>` opens a file with its default app without going through a shell.
    std::process::Command::new("explorer")
        .arg(src)
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Source line to PDF position, for "show me this line in the PDF".
#[tauri::command]
async fn synctex_forward(
    state: State<'_, AppState>,
    project: String,
    file: String,
    line: u32,
    column: u32,
) -> CmdResult<Vec<synctex::ForwardHit>> {
    let ws = state.workspace.clone();
    let install = tex_install(&state)?;
    blocking(move || {
        let p = ws.open_project(&project).map_err(err)?;
        synctex::forward(&install, p.dir(), &pdf_path(&p), &file, line, column).map_err(err)
    })
    .await
}

/// A click in the PDF to the source line behind it.
#[tauri::command]
async fn synctex_inverse(
    state: State<'_, AppState>,
    project: String,
    page: u32,
    x: f64,
    y: f64,
) -> CmdResult<Option<synctex::InverseHit>> {
    let ws = state.workspace.clone();
    let install = tex_install(&state)?;
    blocking(move || {
        let p = ws.open_project(&project).map_err(err)?;
        synctex::inverse(&install, p.dir(), &pdf_path(&p), page, x, y).map_err(err)
    })
    .await
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }
            let workspace = Workspace::open(Settings::load().workspace_root())?;
            app.manage(setup::SetupState::default());
            app.manage(AppState {
                workspace,
                tex: Mutex::new(None),
                overleaf: Mutex::new(None),
                bridge: Mutex::new(None),
                staged: Mutex::new(std::collections::HashMap::new()),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            tex_status,
            setup::setup_check,
            setup::setup_install,
            setup::setup_cancel,
            setup::setup_self_test,
            setup::setup_onboarded,
            setup::setup_mark_onboarded,
            setup::setup_create_tour,
            setup::update_check,
            setup::update_install,
            workspace_info,
            list_projects,
            create_project,
            project_set_archived,
            project_rename,
            project_delete,
            import_zip,
            import_folder,
            list_files,
            read_text_file,
            read_binary_file,
            write_text_file,
            compile_project,
            read_pdf,
            pdf_save_as,
            pdf_open,
            synctex_forward,
            synctex_inverse,
            preferences::settings_get,
            preferences::settings_set_workspace,
            preferences::settings_set_tex_dir,
            preferences::project_settings_get,
            preferences::project_settings_set,
            tree::tree_create,
            tree::tree_rename,
            tree::tree_delete,
            tree::tree_add_files,
            agents::approvals_pending,
            agents::approvals_respond,
            agents::history_list,
            agents::history_undo,
            agents::history_redo,
            versions::history_save,
            versions::history_version,
            versions::history_working,
            versions::history_file,
            versions::history_restore,
            versions::history_restore_file,
            versions::sync_status,
            versions::sync_preview,
            versions::sync_file,
            versions::sync_apply,
            versions::overleaf_history,
            versions::sync_link_practice,
            versions::sync_link_new,
            versions::sync_link_existing,
            versions::sync_unlink,
            versions::practice_create,
            agents::agent_setup,
            agents::mcp_status,
            agents::mcp_set_tools_enabled,
            overleaf::overleaf_status,
            overleaf::overleaf_open_login,
            overleaf::overleaf_connect_pasted,
            overleaf::overleaf_sign_out,
            overleaf::overleaf_projects,
            overleaf::overleaf_pull,
            overleaf::overleaf_write_test,
            chrome_bridge::overleaf_start_chrome_pairing,
            chrome_bridge::overleaf_stop_chrome_pairing,
            chrome_bridge::reveal_in_explorer,
            chrome_bridge::open_external,
            search::search_project,
            search::replace_in_project,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Moss");
}
