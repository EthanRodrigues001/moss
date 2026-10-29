//! SyncTeX: jump from a source line to the PDF position (forward) and from a click in the PDF to
//! the source line (inverse). Uses the `synctex` program that ships with MiKTeX and TeX Live, so
//! the answers match what any other TeX editor would give.
//!
//! All coordinates are PDF points measured from the top-left corner of the page.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;

use crate::tex::TexInstall;
use crate::{Error, Result};

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ForwardHit {
    pub page: u32,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct InverseHit {
    /// Path relative to the Project, `/`-separated.
    pub file: String,
    pub line: u32,
    pub column: i32,
}

fn synctex_exe(install: &TexInstall) -> PathBuf {
    let exe = install.bin_dir.join("synctex.exe");
    if exe.is_file() { exe } else { install.bin_dir.join("synctex") }
}

fn run(install: &TexInstall, project_dir: &Path, args: &[&str]) -> Result<String> {
    let mut cmd = Command::new(synctex_exe(install));
    cmd.current_dir(project_dir).args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let out = cmd
        .output()
        .map_err(|e| Error::io("starting synctex", e))?;
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Native-style absolute path, which is how TeX recorded the file in the `.synctex.gz`.
fn native(path: &Path) -> String {
    let s = path.display().to_string();
    if cfg!(windows) { s.replace('/', "\\") } else { s }
}

/// Where in `pdf` is `file:line` (a path relative to the Project)? A source line that makes a
/// paragraph answers with one box per line of text, top to bottom, so there can be many.
pub fn forward(
    install: &TexInstall,
    project_dir: &Path,
    pdf: &Path,
    file: &str,
    line: u32,
    column: u32,
) -> Result<Vec<ForwardHit>> {
    let source = native(&project_dir.join(file));
    let input = format!("{line}:{column}:{source}");
    let text = run(install, project_dir, &["view", "-i", &input, "-o", &native(pdf)])?;
    Ok(parse_forward(&text))
}

/// Which source line produced the point `(x, y)` on `page` of `pdf`?
pub fn inverse(
    install: &TexInstall,
    project_dir: &Path,
    pdf: &Path,
    page: u32,
    x: f64,
    y: f64,
) -> Result<Option<InverseHit>> {
    let output = format!("{page}:{x:.3}:{y:.3}:{}", native(pdf));
    let text = run(install, project_dir, &["edit", "-o", &output])?;
    Ok(parse_inverse(&text, project_dir))
}

fn fields(text: &str) -> Vec<(&str, &str)> {
    let mut in_result = false;
    let mut out = Vec::new();
    for line in text.lines() {
        if line.starts_with("SyncTeX result begin") {
            in_result = true;
        } else if line.starts_with("SyncTeX result end") {
            break;
        } else if in_result && let Some((k, v)) = line.split_once(':') {
            out.push((k, v));
        }
    }
    out
}

/// Every record in the answer. Each starts at its `Page:` field and ends before the next one.
fn parse_forward(text: &str) -> Vec<ForwardHit> {
    let mut hits = Vec::new();
    let mut current: [Option<f64>; 5] = [None; 5]; // page, h, v, W, H
    let mut flush = |c: &mut [Option<f64>; 5]| {
        if let [Some(page), Some(h), Some(v), Some(w), Some(height)] = *c {
            // `v` is the baseline; the box extends `H` above it.
            hits.push(ForwardHit { page: page as u32, x: h, y: v - height, width: w, height });
        }
        *c = [None; 5];
    };
    for (key, value) in fields(text) {
        let slot = match key {
            "Page" => 0,
            "h" => 1,
            "v" => 2,
            "W" => 3,
            "H" => 4,
            _ => continue,
        };
        if slot == 0 {
            flush(&mut current);
        }
        current[slot] = value.trim().parse::<f64>().ok();
    }
    flush(&mut current);
    hits.dedup();
    hits
}

fn parse_inverse(text: &str, project_dir: &Path) -> Option<InverseHit> {
    let (mut input, mut line, mut column) = (None, None, -1);
    for (key, value) in fields(text) {
        match key {
            "Input" if input.is_none() => input = Some(value.trim().to_string()),
            "Line" if line.is_none() => line = value.trim().parse::<u32>().ok(),
            "Column" => column = value.trim().parse::<i32>().unwrap_or(-1),
            _ => {}
        }
    }
    let file = relative_to(project_dir, &input?)?;
    Some(InverseHit { file, line: line?, column })
}

/// `input` as a Project-relative path, or `None` for files outside the Project (a `.cls` or
/// `.sty` from the TeX install, say).
fn relative_to(project_dir: &Path, input: &str) -> Option<String> {
    let norm = |s: &str| s.replace('\\', "/");
    let base = norm(&project_dir.display().to_string());
    let base = base.trim_end_matches('/');
    let full = norm(input);
    if let Some(rest) = full.strip_prefix("./") {
        return Some(rest.to_string());
    }
    let (head, tail) = (full.get(..base.len())?, full.get(base.len()..)?);
    (head.eq_ignore_ascii_case(base) && tail.starts_with('/')).then(|| tail[1..].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const VIEW: &str = "This is SyncTeX command line utility, version 1.5\n\
SyncTeX result begin\nOutput:x.pdf\nPage:12\nx:122.75\ny:573.92\nh:102.59\nv:576.25\nW:435.98\nH:10.62\n\
before:\noffset:-1\nmiddle:\nafter:\nSyncTeX result end\n";

    #[test]
    fn forward_output_becomes_a_box_above_the_baseline() {
        let hits = parse_forward(VIEW);
        assert_eq!(hits.len(), 1);
        let hit = &hits[0];
        assert_eq!(hit.page, 12);
        assert!((hit.x - 102.59).abs() < 1e-9);
        assert!((hit.y - (576.25 - 10.62)).abs() < 1e-9);
        assert!((hit.width - 435.98).abs() < 1e-9);
    }

    #[test]
    fn forward_with_no_result_is_none() {
        assert!(parse_forward("SyncTeX Warning: No tag for a.tex\n").is_empty());
    }

    #[test]
    fn a_paragraph_gives_one_hit_per_line_of_text() {
        let text = "SyncTeX result begin\nOutput:x.pdf\n\
Page:3\nx:1\ny:10\nh:85\nv:20\nW:400\nH:10\nbefore:\nafter:\n\
Page:3\nx:1\ny:24\nh:85\nv:34\nW:400\nH:10\nbefore:\nafter:\n\
Page:4\nx:1\ny:60\nh:85\nv:70\nW:400\nH:10\nSyncTeX result end\n";
        let hits = parse_forward(text);
        assert_eq!(hits.iter().map(|h| h.page).collect::<Vec<_>>(), vec![3, 3, 4]);
        assert!((hits[1].y - 24.0).abs() < 1e-9);
    }

    #[test]
    fn inverse_output_is_mapped_into_the_project() {
        let text = "SyncTeX result begin\nOutput:m.pdf\n\
Input:C:\\Users\\Ethan\\Documents\\Moss\\projects\\Rep\\chapters\\a.tex\nLine:51\nColumn:-1\nOffset:0\n\
Context:\nSyncTeX result end\n";
        let dir = Path::new("c:/users/ethan/documents/moss/projects/Rep");
        let hit = parse_inverse(text, dir).unwrap();
        assert_eq!(hit.file, "chapters/a.tex");
        assert_eq!(hit.line, 51);
    }

    #[test]
    fn inverse_ignores_files_from_the_tex_install() {
        let text = "SyncTeX result begin\nInput:C:\\MiKTeX\\tex\\latex\\base\\report.cls\nLine:9\n\
Column:0\nSyncTeX result end\n";
        assert_eq!(parse_inverse(text, Path::new("C:/Users/x/projects/Rep")), None);
    }

    #[test]
    fn relative_inputs_lose_their_dot_slash() {
        assert_eq!(relative_to(Path::new("C:/p"), "./chapters/a.tex").as_deref(), Some("chapters/a.tex"));
        // A sibling folder with the same prefix is not inside the Project.
        assert_eq!(relative_to(Path::new("C:/p"), "C:/p2/a.tex"), None);
    }
}
