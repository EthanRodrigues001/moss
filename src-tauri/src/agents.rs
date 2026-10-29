//! Everything the app does for Agents: answer Approvals, undo and redo Agent changes, show how
//! to connect an MCP client to `moss-mcp`, and report the MCP server's status for Settings.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use moss_core::approvals::{Approval, Approvals};
use moss_core::history::{History, SnapshotInfo};
use moss_core::pdfview::PdfTools;
use moss_core::settings::Settings;
use moss_core::tex;
use moss_core::workspace::Workspace;
use serde::Serialize;
use serde_json::Value;
use tauri::State;

use crate::{AppState, CmdResult, blocking, err};

#[tauri::command]
pub(crate) fn approvals_pending(state: State<'_, AppState>) -> CmdResult<Vec<Approval>> {
    Approvals::new(state.workspace.root()).pending().map_err(err)
}

#[tauri::command]
pub(crate) fn approvals_respond(state: State<'_, AppState>, id: String, approve: bool) -> CmdResult<()> {
    Approvals::new(state.workspace.root())
        .respond(&id, approve)
        .map(|_| ())
        .map_err(err)
}

#[tauri::command]
pub(crate) async fn history_list(
    state: State<'_, AppState>,
    project: String,
) -> CmdResult<Vec<SnapshotInfo>> {
    let ws = state.workspace.clone();
    blocking(move || {
        let p = ws.open_project(&project).map_err(err)?;
        History::new(p.dir()).list().map_err(err)
    })
    .await
}

#[tauri::command]
pub(crate) async fn history_undo(
    state: State<'_, AppState>,
    project: String,
) -> CmdResult<Option<SnapshotInfo>> {
    let ws = state.workspace.clone();
    blocking(move || {
        let p = ws.open_project(&project).map_err(err)?;
        History::new(p.dir()).undo().map_err(err)
    })
    .await
}

#[tauri::command]
pub(crate) async fn history_redo(
    state: State<'_, AppState>,
    project: String,
) -> CmdResult<Option<SnapshotInfo>> {
    let ws = state.workspace.clone();
    blocking(move || {
        let p = ws.open_project(&project).map_err(err)?;
        History::new(p.dir()).redo().map_err(err)
    })
    .await
}

#[derive(Serialize)]
pub(crate) struct AgentSetup {
    /// Full path of `moss-mcp.exe`, or `None` when it has not been built or installed.
    command: Option<String>,
    workspace: String,
    /// Ready-to-paste `mcpServers` JSON for Claude Desktop, Cursor and similar clients.
    config: String,
    /// One-line command for Claude Code.
    claude_code: String,
}

/// `moss-mcp.exe` sits next to the app when installed, and in the same Cargo target folder in dev.
fn find_moss_mcp() -> Option<std::path::PathBuf> {
    let dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    let exe = dir.join("moss-mcp.exe");
    exe.is_file().then_some(exe)
}

/// An AI program that can be set up to use Moss, and whether it is.
#[derive(Serialize)]
pub(crate) struct McpClient {
    name: &'static str,
    /// Where its MCP settings live, for the user to find.
    config_file: String,
    /// The program's settings file exists, so it is probably installed.
    installed: bool,
    /// The settings list a `moss` server.
    configured: bool,
    /// What the `moss` entry runs, when it can be read.
    command: Option<String>,
    /// The entry runs the `moss-mcp.exe` that sits next to this app.
    current: bool,
}

/// One thing an Agent did, from a Project's audit log.
#[derive(Serialize)]
pub(crate) struct AgentActivity {
    at: i64,
    project: String,
    action: String,
    detail: String,
}

/// Something `moss-mcp` needs from this PC and whether it is there.
#[derive(Serialize)]
pub(crate) struct McpNeed {
    name: &'static str,
    ok: bool,
    detail: String,
}

#[derive(Serialize)]
pub(crate) struct McpStatus {
    /// `ready` (runs and lists its tools), `missing` (no `moss-mcp.exe` next to the app) or
    /// `broken` (it is there but did not answer).
    state: &'static str,
    exe: Option<String>,
    version: Option<String>,
    problem: Option<String>,
    workspace: String,
    /// Every tool as `moss-mcp --list-tools` reports it (name, title, group, hints).
    tools: Value,
    /// Tools the user switched off.
    disabled: Vec<String>,
    needs: Vec<McpNeed>,
    clients: Vec<McpClient>,
    activity: Vec<AgentActivity>,
    pending_approvals: usize,
}

/// Run `moss-mcp --list-tools`: proves the program starts and says what it offers.
fn list_tools(exe: &Path) -> Result<(String, Value), String> {
    let mut cmd = Command::new(exe);
    cmd.arg("--list-tools").stdout(Stdio::piped()).stderr(Stdio::null());
    #[cfg(windows)]
    std::os::windows::process::CommandExt::creation_flags(&mut cmd, 0x0800_0000); // no console window
    let mut child = cmd.spawn().map_err(|e| format!("could not start it: {e}"))?;
    let started = Instant::now();
    loop {
        match child.try_wait().map_err(|e| e.to_string())? {
            Some(_) => break,
            None if started.elapsed() > Duration::from_secs(8) => {
                let _ = child.kill();
                return Err("it did not answer within 8 seconds".into());
            }
            None => std::thread::sleep(Duration::from_millis(20)),
        }
    }
    let output = child.wait_with_output().map_err(|e| e.to_string())?;
    let listing: Value = serde_json::from_slice(&output.stdout)
        .map_err(|_| "it started but did not list its tools (is it an older build?)".to_string())?;
    let version = listing["version"].as_str().unwrap_or("?").to_string();
    Ok((version, listing["tools"].clone()))
}

fn same_path(a: &str, b: &Path) -> bool {
    let norm = |s: &str| s.replace('\\', "/").to_lowercase();
    norm(a) == norm(&b.display().to_string())
}

/// Look for a `moss` server in a JSON MCP settings file (`mcpServers`, at the top or per project).
fn json_client(name: &'static str, file: Option<PathBuf>, exe: Option<&Path>) -> McpClient {
    let config_file = file.as_ref().map(|f| f.display().to_string()).unwrap_or_default();
    let text = file.as_ref().and_then(|f| std::fs::read_to_string(f).ok());
    let installed = text.is_some();
    let entry = text
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|root| {
            let mut scopes = vec![root["mcpServers"].clone()];
            if let Some(projects) = root["projects"].as_object() {
                scopes.extend(projects.values().map(|p| p["mcpServers"].clone()));
            }
            scopes.into_iter().find_map(|s| s.get("moss").cloned())
        });
    let command = entry.as_ref().and_then(|e| e["command"].as_str()).map(str::to_string);
    McpClient {
        name,
        config_file,
        installed,
        configured: entry.is_some(),
        current: matches!((&command, exe), (Some(c), Some(e)) if same_path(c, e)),
        command,
    }
}

/// Codex keeps MCP servers in `config.toml` under `[mcp_servers.moss]`.
fn codex_client(file: Option<PathBuf>, exe: Option<&Path>) -> McpClient {
    let config_file = file.as_ref().map(|f| f.display().to_string()).unwrap_or_default();
    let text = file.as_ref().and_then(|f| std::fs::read_to_string(f).ok());
    let configured = text.as_deref().is_some_and(|t| t.contains("[mcp_servers.moss]"));
    let command = text.as_deref().filter(|_| configured).and_then(|t| {
        let after = &t[t.find("[mcp_servers.moss]")?..];
        let line = after.lines().skip(1).find(|l| l.trim_start().starts_with("command"))?;
        let value = line.split_once('=')?.1.trim().trim_matches(|c| c == '"' || c == '\'');
        Some(value.replace("\\\\", "\\"))
    });
    McpClient {
        name: "Codex",
        config_file,
        installed: text.is_some(),
        configured,
        current: matches!((&command, exe), (Some(c), Some(e)) if same_path(c, e)),
        command,
    }
}

fn find_clients(exe: Option<&Path>) -> Vec<McpClient> {
    let home = std::env::var_os("USERPROFILE").map(PathBuf::from);
    let appdata = std::env::var_os("APPDATA").map(PathBuf::from);
    vec![
        json_client("Claude Code", home.as_ref().map(|h| h.join(".claude.json")), exe),
        json_client(
            "Claude Desktop",
            appdata.as_ref().map(|a| a.join("Claude").join("claude_desktop_config.json")),
            exe,
        ),
        json_client("Cursor", home.as_ref().map(|h| h.join(".cursor").join("mcp.json")), exe),
        codex_client(home.as_ref().map(|h| h.join(".codex").join("config.toml")), exe),
    ]
}

/// The newest Agent actions across every Project, from each Project's audit log.
fn recent_activity(workspace: &Workspace, limit: usize) -> Vec<AgentActivity> {
    let Ok(projects) = workspace.list_projects() else { return Vec::new() };
    let mut all = Vec::new();
    for project in projects {
        let log = workspace.projects_dir().join(&project.dir_name).join(".moss").join("audit.log");
        let Ok(text) = std::fs::read_to_string(&log) else { continue };
        for line in text.lines().rev().take(40) {
            let Ok(entry) = serde_json::from_str::<Value>(line) else { continue };
            if entry["actor"] != "moss-mcp" {
                continue;
            }
            all.push(AgentActivity {
                at: entry["at"].as_i64().unwrap_or(0),
                project: project.name.clone(),
                action: entry["action"].as_str().unwrap_or("").to_string(),
                detail: entry["detail"].as_str().unwrap_or("").to_string(),
            });
        }
    }
    all.sort_by_key(|a| std::cmp::Reverse(a.at));
    all.truncate(limit);
    all
}

/// What the tools lean on: a TeX install to compile and Poppler (shipped with MiKTeX) for page images.
fn needs() -> Vec<McpNeed> {
    match tex::detect(Settings::load().tex_bin_dir.as_deref()) {
        Err(e) => vec![McpNeed { name: "TeX install", ok: false, detail: e.to_string() }],
        Ok(install) => {
            let poppler = PdfTools::find(&install);
            vec![
                McpNeed { name: "TeX install", ok: true, detail: install.bin_dir.display().to_string() },
                McpNeed {
                    name: "Page images (Poppler)",
                    ok: poppler.is_ok(),
                    detail: match poppler {
                        Ok(_) => "found next to the TeX install".into(),
                        Err(e) => format!("{e}. `view_pages` and `pdf_info` need it."),
                    },
                },
            ]
        }
    }
}

#[tauri::command]
pub(crate) async fn mcp_status(state: State<'_, AppState>) -> CmdResult<McpStatus> {
    let workspace = state.workspace.clone();
    blocking(move || {
        let exe = find_moss_mcp();
        let (state_name, version, problem, tools) = match exe.as_deref().map(list_tools) {
            None => ("missing", None, Some("moss-mcp.exe was not found next to the app.".to_string()), Value::Array(vec![])),
            Some(Ok((v, tools))) => ("ready", Some(v), None, tools),
            Some(Err(e)) => ("broken", None, Some(e), Value::Array(vec![])),
        };
        Ok(McpStatus {
            state: state_name,
            exe: exe.as_ref().map(|p| p.display().to_string()),
            version,
            problem,
            workspace: workspace.root().display().to_string(),
            tools,
            disabled: Settings::load().disabled_tools,
            needs: needs(),
            clients: find_clients(exe.as_deref()),
            activity: recent_activity(&workspace, 8),
            pending_approvals: Approvals::new(workspace.root()).pending().map(|p| p.len()).unwrap_or(0),
        })
    })
    .await
}

/// Switch tools on or off for agents. Takes effect the next time an agent starts `moss-mcp`.
/// Returns the full list of switched-off tools.
#[tauri::command]
pub(crate) fn mcp_set_tools_enabled(names: Vec<String>, enabled: bool) -> CmdResult<Vec<String>> {
    let mut settings = Settings::load();
    toggle_tools(&mut settings.disabled_tools, names, enabled);
    settings.save().map_err(err)?;
    Ok(settings.disabled_tools)
}

/// Add `names` to the switched-off list, or remove them from it; the list stays sorted and unique.
fn toggle_tools(disabled: &mut Vec<String>, names: Vec<String>, enabled: bool) {
    disabled.retain(|n| !names.contains(n));
    if !enabled {
        disabled.extend(names);
    }
    disabled.sort();
    disabled.dedup();
}

#[tauri::command]
pub(crate) fn agent_setup(state: State<'_, AppState>) -> AgentSetup {
    let command = find_moss_mcp().map(|p| p.display().to_string());
    let shown = command.clone().unwrap_or_else(|| "C:\\path\\to\\moss-mcp.exe".into());
    let workspace = state.workspace.root().display().to_string();
    let config = serde_json::json!({
        "mcpServers": {
            "moss": { "command": shown, "args": ["--workspace", workspace] }
        }
    });
    AgentSetup {
        claude_code: format!("claude mcp add moss -- \"{shown}\" --workspace \"{workspace}\""),
        config: serde_json::to_string_pretty(&config).unwrap_or_default(),
        command,
        workspace,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXE: &str = "C:\\Program Files\\Moss\\moss-mcp.exe";

    fn write(dir: &Path, name: &str, text: &str) -> PathBuf {
        let file = dir.join(name);
        std::fs::write(&file, text).unwrap();
        file
    }

    #[test]
    fn a_json_client_is_found_at_the_top_or_inside_a_project_and_compared_with_this_exe() {
        let dir = tempfile::tempdir().unwrap();
        let exe = Path::new(EXE);

        let top = write(dir.path(), "a.json", r#"{"mcpServers":{"moss":{"command":"c:/program files/moss/MOSS-MCP.exe"}}}"#);
        let c = json_client("Claude Desktop", Some(top), Some(exe));
        assert!(c.installed && c.configured && c.current, "slashes and case do not matter");

        let nested = write(
            dir.path(),
            "b.json",
            r#"{"mcpServers":{},"projects":{"C:/x":{"mcpServers":{"moss":{"command":"D:\\old\\moss-mcp.exe"}}}}}"#,
        );
        let c = json_client("Claude Code", Some(nested), Some(exe));
        assert!(c.configured && !c.current);
        assert_eq!(c.command.as_deref(), Some("D:\\old\\moss-mcp.exe"));

        let other = write(dir.path(), "c.json", r#"{"mcpServers":{"other":{"command":"x"}}}"#);
        let c = json_client("Cursor", Some(other), Some(exe));
        assert!(c.installed && !c.configured && !c.current);

        let c = json_client("Cursor", Some(dir.path().join("missing.json")), Some(exe));
        assert!(!c.installed && !c.configured);
        assert!(!json_client("Cursor", None, None).installed);

        let broken = write(dir.path(), "d.json", "{ not json");
        let c = json_client("Cursor", Some(broken), Some(exe));
        assert!(c.installed && !c.configured, "a damaged file is not an error");
    }

    #[test]
    fn codex_keeps_its_mcp_servers_in_toml() {
        let dir = tempfile::tempdir().unwrap();
        let exe = Path::new(EXE);
        let toml = "model = \"x\"\n\n[mcp_servers.other]\ncommand = \"nope\"\n\n[mcp_servers.moss]\ncommand = \"C:\\\\Program Files\\\\Moss\\\\moss-mcp.exe\"\nargs = [\"--workspace\", \"D:\\\\Moss\"]\n";
        let c = codex_client(Some(write(dir.path(), "config.toml", toml)), Some(exe));
        assert!(c.configured && c.current, "{:?}", c.command);

        let c = codex_client(Some(write(dir.path(), "empty.toml", "model = \"x\"\n")), Some(exe));
        assert!(c.installed && !c.configured && c.command.is_none());
    }

    #[test]
    fn recent_activity_merges_projects_newest_first_and_ignores_other_actors() {
        let dir = tempfile::tempdir().unwrap();
        let ws = Workspace::open(dir.path()).unwrap();
        let a = ws.create_project("Alpha").unwrap();
        let b = ws.create_project("Beta").unwrap();
        for (p, action, detail) in [(&a, "write_file", "a.tex"), (&b, "edit_file", "b.tex"), (&a, "delete_file", "old.tex")] {
            let dir = ws.projects_dir().join(&p.dir_name);
            moss_core::audit::record(&dir, "moss-mcp", action, detail).unwrap();
            // The audit log stamps whole seconds; keep the three entries apart.
            std::thread::sleep(Duration::from_millis(1100));
        }
        moss_core::audit::record(&ws.projects_dir().join(&a.dir_name), "someone-else", "x", "y").unwrap();

        let all = recent_activity(&ws, 10);
        let details: Vec<&str> = all.iter().map(|e| e.detail.as_str()).collect();
        assert_eq!(details, ["old.tex", "b.tex", "a.tex"]);
        assert_eq!(all[1].project, "Beta");
        assert_eq!(recent_activity(&ws, 2).len(), 2);
    }

    #[test]
    fn the_built_moss_mcp_lists_its_tools_when_asked() {
        let exe = Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/debug/moss-mcp.exe");
        if !exe.is_file() {
            eprintln!("skipped: build moss-mcp first (cargo build -p moss-mcp)");
            return;
        }
        let (version, tools) = list_tools(&exe).unwrap();
        assert!(!version.is_empty() && version != "?");
        let tools = tools.as_array().unwrap();
        assert!(tools.len() >= 40, "only {} tools", tools.len());
        assert!(tools.iter().any(|t| t["name"] == "read_file" && t["read_only"] == true));
        assert!(list_tools(Path::new("C:\\no\\such\\moss-mcp.exe")).is_err());
    }

    #[test]
    fn switching_tools_on_and_off_keeps_the_list_sorted_and_unique() {
        let mut off = vec!["undo".to_string()];
        toggle_tools(&mut off, vec!["delete_file".into(), "undo".into()], false);
        assert_eq!(off, ["delete_file", "undo"]);
        toggle_tools(&mut off, vec!["undo".into()], true);
        assert_eq!(off, ["delete_file"]);
        toggle_tools(&mut off, vec!["nothing".into()], true);
        assert_eq!(off, ["delete_file"]);
    }
}
