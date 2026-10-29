//! App-wide settings, shared by the desktop app and `moss-mcp` so both use the same Workspace and
//! the same TeX install. Stored at `%APPDATA%\Moss\settings.json`.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::workspace::Workspace;
use crate::{Error, Result};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    /// Folder that holds every Project. `None` means `Documents\Moss`.
    #[serde(default)]
    pub workspace: Option<PathBuf>,
    /// Folder with `latexmk` and the engines, for a TeX install Moss cannot find by itself.
    #[serde(default)]
    pub tex_bin_dir: Option<PathBuf>,
    /// MCP tools the user switched off in Settings. `moss-mcp` hides them from agents.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub disabled_tools: Vec<String>,
    /// First-run setup has been shown (finished or skipped), so it does not open again by itself.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub onboarded: bool,
}

impl Settings {
    pub fn file() -> PathBuf {
        let base = std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| Workspace::default_root().join(".config"));
        base.join("Moss").join("settings.json")
    }

    /// Missing or unreadable settings give the defaults rather than an error: a damaged file must
    /// never stop the app from starting.
    pub fn load() -> Settings {
        Settings::load_from(&Settings::file())
    }

    pub fn load_from(file: &Path) -> Settings {
        fs::read_to_string(file)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> Result<()> {
        self.save_to(&Settings::file())
    }

    pub fn save_to(&self, file: &Path) -> Result<()> {
        if let Some(dir) = file.parent() {
            fs::create_dir_all(dir).map_err(|e| Error::io("creating settings folder", e))?;
        }
        let json = serde_json::to_string_pretty(self).map_err(|e| Error::json("writing settings", e))?;
        let tmp = file.with_extension("json.tmp");
        fs::write(&tmp, json).map_err(|e| Error::io("writing settings", e))?;
        fs::rename(&tmp, file).map_err(|e| Error::io("replacing settings", e))
    }

    /// The Workspace folder in effect.
    pub fn workspace_root(&self) -> PathBuf {
        self.workspace.clone().unwrap_or_else(Workspace::default_root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_defaults_when_missing_or_damaged() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("Moss").join("settings.json");
        assert_eq!(Settings::load_from(&file), Settings::default());

        let settings = Settings {
            workspace: Some(PathBuf::from("D:/Thesis")),
            tex_bin_dir: None,
            disabled_tools: vec!["overleaf_push".into()],
            onboarded: true,
        };
        settings.save_to(&file).unwrap();
        assert_eq!(Settings::load_from(&file), settings);
        assert_eq!(settings.workspace_root(), PathBuf::from("D:/Thesis"));

        fs::write(&file, "{ not json").unwrap();
        assert_eq!(Settings::load_from(&file), Settings::default());
    }
}
