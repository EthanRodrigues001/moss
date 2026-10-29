//! Finding a usable TeX installation on this machine.

use std::env;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Flavor {
    MiKTeX,
    TeXLive,
    Unknown,
}

#[derive(Debug, Clone, Serialize)]
pub struct TexInstall {
    pub flavor: Flavor,
    pub bin_dir: PathBuf,
    pub latexmk: PathBuf,
    pub perl: Option<PathBuf>,
}

/// Engines `latexmk` can drive. pdfLaTeX is the default, like Overleaf.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Engine {
    Pdflatex,
    Xelatex,
    Lualatex,
}

impl Engine {
    pub fn latexmk_flag(self) -> &'static str {
        match self {
            Engine::Pdflatex => "-pdf",
            Engine::Xelatex => "-xelatex",
            Engine::Lualatex => "-lualatex",
        }
    }

    /// `latexmk` option that overrides the engine command line.
    /// `extra` goes between the program name and latexmk's own `%O %S`.
    pub fn latexmk_command(self, extra: &str) -> String {
        let (option, program) = match self {
            Engine::Pdflatex => ("-pdflatex", "pdflatex"),
            Engine::Xelatex => ("-xelatex", "xelatex"),
            Engine::Lualatex => ("-lualatex", "lualatex"),
        };
        format!("{option}={program} {extra} %O %S")
    }
}

/// Pick the engine for a root file: an explicit magic comment wins, then packages
/// that only work with XeLaTeX/LuaLaTeX, then pdfLaTeX.
pub fn detect_engine(root_source: &str) -> Engine {
    for line in root_source.lines().take(20) {
        if let Some(engine) = magic_comment_engine(line) {
            return engine;
        }
    }
    let uses = |pkg: &str| {
        root_source.lines().any(|l| {
            let l = l.trim_start();
            !l.starts_with('%') && l.contains("\\usepackage") && l.contains(pkg)
        })
    };
    if uses("fontspec") || uses("unicode-math") || uses("polyglossia") {
        return Engine::Xelatex;
    }
    Engine::Pdflatex
}

/// Parses `% !TEX program = xelatex` and `%!TEX TS-program = xelatex`.
fn magic_comment_engine(line: &str) -> Option<Engine> {
    let lower = line.trim().to_ascii_lowercase();
    let rest = lower.strip_prefix('%')?.trim_start_matches(['!', ' ']);
    let rest = rest.strip_prefix("tex")?.trim_start();
    let value = rest
        .strip_prefix("ts-program")
        .or_else(|| rest.strip_prefix("program"))?
        .trim_start_matches([' ', '='])
        .trim();
    match value {
        "xelatex" | "xetex" => Some(Engine::Xelatex),
        "lualatex" | "luatex" => Some(Engine::Lualatex),
        "pdflatex" | "pdftex" => Some(Engine::Pdflatex),
        _ => None,
    }
}

fn exe_in(dir: &Path, name: &str) -> Option<PathBuf> {
    let candidates: &[&str] = if cfg!(windows) {
        &[".exe", ".bat", ".cmd"]
    } else {
        &[""]
    };
    candidates
        .iter()
        .map(|ext| dir.join(format!("{name}{ext}")))
        .find(|p| p.is_file())
}

fn known_tex_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let var = |k: &str| env::var_os(k).map(PathBuf::from);
    if let Some(local) = var("LOCALAPPDATA") {
        dirs.push(local.join("Programs/MiKTeX/miktex/bin/x64"));
    }
    for k in ["ProgramFiles", "ProgramFiles(x86)"] {
        if let Some(pf) = var(k) {
            dirs.push(pf.join("MiKTeX/miktex/bin/x64"));
        }
    }
    if let Ok(entries) = std::fs::read_dir("C:/texlive") {
        let mut years: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
        years.sort();
        years.reverse(); // newest first
        for y in years {
            dirs.push(y.join("bin/windows"));
            dirs.push(y.join("bin/win64"));
        }
    }
    dirs
}

fn flavor_of(dir: &Path) -> Flavor {
    let s = dir.to_string_lossy().to_ascii_lowercase();
    if s.contains("miktex") {
        Flavor::MiKTeX
    } else if s.contains("texlive") {
        Flavor::TeXLive
    } else {
        Flavor::Unknown
    }
}

/// Where first-run setup puts its portable Perl (`%LOCALAPPDATA%\Moss\perl`). Not on `PATH`, so
/// detection looks here itself.
pub fn moss_perl_bin() -> PathBuf {
    env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_default()
        .join("Moss/perl/perl/bin")
}

fn find_perl(tex_bin: &Path) -> Option<PathBuf> {
    let mut dirs: Vec<PathBuf> = vec![tex_bin.to_path_buf()];
    if let Some(path) = env::var_os("PATH") {
        dirs.extend(env::split_paths(&path));
    }
    dirs.push(moss_perl_bin());
    dirs.push(PathBuf::from("C:/Strawberry/perl/bin"));
    // Git for Windows ships an MSYS perl that does not run MiKTeX's latexmk reliably.
    dirs.into_iter()
        .filter(|d| !d.to_string_lossy().to_ascii_lowercase().contains("git"))
        .find_map(|d| exe_in(&d, "perl"))
}

/// Look for `latexmk` in the user's chosen folder, then `PATH`, then the usual
/// MiKTeX and TeX Live locations.
pub fn detect(custom_bin_dir: Option<&Path>) -> Result<TexInstall> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(c) = custom_bin_dir {
        dirs.push(c.to_path_buf());
    }
    if let Some(path) = env::var_os("PATH") {
        dirs.extend(env::split_paths(&path));
    }
    dirs.extend(known_tex_dirs());

    let (bin_dir, latexmk) = dirs
        .into_iter()
        .find_map(|d| exe_in(&d, "latexmk").map(|exe| (d, exe)))
        .ok_or(Error::TexNotFound)?;

    Ok(TexInstall {
        flavor: flavor_of(&bin_dir),
        perl: find_perl(&bin_dir),
        bin_dir,
        latexmk,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_engine_is_pdflatex() {
        assert_eq!(detect_engine("\\documentclass{report}\n"), Engine::Pdflatex);
    }

    #[test]
    fn latexmk_command_overrides_the_engine_line() {
        assert_eq!(
            Engine::Pdflatex.latexmk_command("--enable-installer"),
            "-pdflatex=pdflatex --enable-installer %O %S"
        );
        assert_eq!(
            Engine::Xelatex.latexmk_command("--enable-installer"),
            "-xelatex=xelatex --enable-installer %O %S"
        );
    }

    #[test]
    fn magic_comment_wins() {
        let src = "% !TEX program = lualatex\n\\documentclass{article}\n\\usepackage{fontspec}\n";
        assert_eq!(detect_engine(src), Engine::Lualatex);
        let src = "%!TEX TS-program = xelatex\n\\documentclass{article}\n";
        assert_eq!(detect_engine(src), Engine::Xelatex);
    }

    #[test]
    fn fontspec_selects_xelatex_but_comments_do_not() {
        assert_eq!(
            detect_engine("\\documentclass{article}\n\\usepackage{fontspec}\n"),
            Engine::Xelatex
        );
        assert_eq!(
            detect_engine("\\documentclass{article}\n% \\usepackage{fontspec}\n"),
            Engine::Pdflatex
        );
    }

    #[test]
    fn flavor_from_path() {
        assert_eq!(
            flavor_of(Path::new("C:/Users/x/AppData/Local/Programs/MiKTeX/miktex/bin/x64")),
            Flavor::MiKTeX
        );
        assert_eq!(flavor_of(Path::new("C:/texlive/2025/bin/windows")), Flavor::TeXLive);
    }
}
