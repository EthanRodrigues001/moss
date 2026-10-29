//! Settings screen: where Projects live, which TeX install to use, and per-Project compile options.

use std::path::PathBuf;

use moss_core::project::root_file_candidates;
use moss_core::settings::Settings;
use moss_core::tex::Engine;
use moss_core::workspace::Workspace;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::{AppState, CmdResult, TexStatus, blocking, detect_tex, err};

#[derive(Serialize)]
pub(crate) struct SettingsView {
    /// The folder Moss is using right now.
    running_workspace: String,
    /// The folder it will use after a restart.
    workspace: String,
    workspace_is_custom: bool,
    tex_bin_dir: Option<String>,
}

fn view(state: &AppState, settings: &Settings) -> SettingsView {
    SettingsView {
        running_workspace: state.workspace.root().display().to_string(),
        workspace: settings.workspace_root().display().to_string(),
        workspace_is_custom: settings.workspace.is_some(),
        tex_bin_dir: settings.tex_bin_dir.as_ref().map(|p| p.display().to_string()),
    }
}

#[tauri::command]
pub(crate) fn settings_get(state: State<'_, AppState>) -> SettingsView {
    view(&state, &Settings::load())
}

/// Choose the Workspace folder (or `None` for the default). Takes effect after a restart so
/// nothing in the running app points at two folders.
#[tauri::command]
pub(crate) fn settings_set_workspace(
    state: State<'_, AppState>,
    dir: Option<String>,
) -> CmdResult<SettingsView> {
    let mut settings = Settings::load();
    let chosen = dir.filter(|d| !d.trim().is_empty()).map(PathBuf::from);
    if let Some(path) = &chosen {
        Workspace::open(path).map_err(err)?; // proves the folder is usable and sets up its layout
    }
    settings.workspace = chosen;
    settings.save().map_err(err)?;
    Ok(view(&state, &settings))
}

/// Choose the folder holding `latexmk` (or `None` to search normally), then look again.
#[tauri::command]
pub(crate) fn settings_set_tex_dir(
    state: State<'_, AppState>,
    dir: Option<String>,
) -> CmdResult<TexStatus> {
    let mut settings = Settings::load();
    settings.tex_bin_dir = dir.filter(|d| !d.trim().is_empty()).map(PathBuf::from);
    settings.save().map_err(err)?;
    Ok(detect_tex(&state))
}

#[derive(Serialize)]
pub(crate) struct ProjectSettings {
    root_file: String,
    /// `auto`, `pdflatex`, `xelatex` or `lualatex`.
    engine: String,
    shell_escape: bool,
    trusted: bool,
    /// `.tex` files with `\documentclass`, for the Root file picker.
    root_candidates: Vec<String>,
}

#[derive(Deserialize)]
pub(crate) struct ProjectPatch {
    root_file: Option<String>,
    engine: Option<String>,
    shell_escape: Option<bool>,
    trusted: Option<bool>,
}

fn project_view(p: &moss_core::project::Project) -> ProjectSettings {
    ProjectSettings {
        root_file: p.meta.root_file.clone(),
        engine: match p.meta.engine {
            None => "auto",
            Some(Engine::Pdflatex) => "pdflatex",
            Some(Engine::Xelatex) => "xelatex",
            Some(Engine::Lualatex) => "lualatex",
        }
        .to_string(),
        shell_escape: p.meta.shell_escape,
        trusted: p.meta.trusted,
        root_candidates: root_file_candidates(p.dir()),
    }
}

#[tauri::command]
pub(crate) async fn project_settings_get(
    state: State<'_, AppState>,
    project: String,
) -> CmdResult<ProjectSettings> {
    let ws = state.workspace.clone();
    blocking(move || Ok(project_view(&ws.open_project(&project).map_err(err)?))).await
}

#[tauri::command]
pub(crate) async fn project_settings_set(
    state: State<'_, AppState>,
    project: String,
    patch: ProjectPatch,
) -> CmdResult<ProjectSettings> {
    let ws = state.workspace.clone();
    blocking(move || {
        let mut p = ws.open_project(&project).map_err(err)?;
        if let Some(root) = patch.root_file {
            if !moss_core::fsutil::safe_join(p.dir(), &root).map_err(err)?.is_file() {
                return Err(format!("`{root}` is not a file in this project"));
            }
            p.meta.root_file = root;
        }
        if let Some(engine) = patch.engine {
            p.meta.engine = match engine.as_str() {
                "auto" => None,
                "pdflatex" => Some(Engine::Pdflatex),
                "xelatex" => Some(Engine::Xelatex),
                "lualatex" => Some(Engine::Lualatex),
                other => return Err(format!("unknown engine `{other}`")),
            };
        }
        if let Some(on) = patch.shell_escape {
            p.meta.shell_escape = on;
        }
        if let Some(on) = patch.trusted {
            p.meta.trusted = on;
        }
        p.save_meta().map_err(err)?;
        Ok(project_view(&p))
    })
    .await
}
