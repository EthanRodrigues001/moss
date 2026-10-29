//! `moss-mcp`: lets AI agents work on Moss Projects over MCP (stdio).
//!
//! Usage: `moss-mcp [--workspace <folder>]`. Without the flag it uses `MOSS_WORKSPACE`, then the
//! Workspace chosen in the desktop app's settings (default `Documents\Moss`).
//! `moss-mcp --list-tools` prints the version and every tool as JSON and exits; the desktop app
//! uses it to show the tools in Settings.

mod ops;
mod server;

use std::path::PathBuf;

use moss_core::settings::Settings;
use moss_core::workspace::Workspace;
use rmcp::{ServiceExt, transport::stdio};

fn workspace_root(settings: &Settings) -> PathBuf {
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--workspace"
            && let Some(path) = args.next()
        {
            return PathBuf::from(path);
        }
    }
    std::env::var_os("MOSS_WORKSPACE")
        .map(PathBuf::from)
        .unwrap_or_else(|| settings.workspace_root())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::args().any(|a| a == "--list-tools") {
        let listing = serde_json::json!({
            "version": env!("CARGO_PKG_VERSION"),
            "tools": server::catalog(),
        });
        println!("{}", serde_json::to_string(&listing)?);
        return Ok(());
    }
    let settings = Settings::load();
    let root = workspace_root(&settings);
    let workspace = Workspace::open(&root)?;
    eprintln!("moss-mcp: serving Workspace {}", root.display());
    if !settings.disabled_tools.is_empty() {
        eprintln!("moss-mcp: tools switched off in Settings: {}", settings.disabled_tools.join(", "));
    }
    let ops = ops::Ops::new(workspace, settings.tex_bin_dir.clone());
    let service = server::MossServer::new(ops, settings.disabled_tools.clone())
        .serve(stdio())
        .await?;
    service.waiting().await?;
    Ok(())
}
