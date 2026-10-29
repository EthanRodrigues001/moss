//! Compiling a Project with `latexmk`.
//!
//! Output goes to a persistent Build cache so unchanged passes are skipped on the next
//! compile. We pass `-f` so a document with errors still produces a PDF, like Overleaf.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime};

use serde::Serialize;

use crate::tex::{Engine, Flavor, TexInstall};
use crate::{Error, Result};

pub struct CompileRequest<'a> {
    pub install: &'a TexInstall,
    pub project_dir: &'a Path,
    /// Root file, relative to the project folder, using `/` separators.
    pub root_file: &'a str,
    pub engine: Engine,
    pub build_dir: &'a Path,
    pub shell_escape: bool,
    pub timeout: Duration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Debug, Clone, Serialize)]
pub struct Diagnostic {
    pub severity: Severity,
    pub message: String,
    pub file: Option<String>,
    pub line: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct CompileOutcome {
    /// `latexmk` finished with exit code 0.
    pub success: bool,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    /// Present whenever a fresh PDF was written, even if the document had errors.
    pub pdf: Option<PathBuf>,
    pub duration: Duration,
    pub diagnostics: Vec<Diagnostic>,
    /// Raw `latexmk` console output.
    pub console: String,
    /// The TeX log of this run, for box and reference summaries.
    #[serde(skip)]
    pub log: String,
    /// `latexmk` first answered from an earlier failed run ("gave an error in previous
    /// invocation"), so the errors were old news. The compile was run again with a forced rebuild
    /// and everything here is from that second run.
    pub forced_rebuild: bool,
}

/// Reject root paths that could point outside the project folder.
pub fn check_relative(path: &str) -> Result<()> {
    let p = Path::new(path);
    let bad = p.is_absolute()
        || p.components().any(|c| {
            matches!(
                c,
                std::path::Component::ParentDir | std::path::Component::Prefix(_)
            )
        });
    if bad {
        return Err(Error::UnsafePath(path.to_string()));
    }
    Ok(())
}

/// `\include{chapters/intro}` writes `chapters/intro.aux`, and latexmk will not create the
/// folder inside the output directory, so mirror the project's folders there first.
fn mirror_dirs(src: &Path, dst: &Path) -> Result<()> {
    fs::create_dir_all(dst).map_err(|e| Error::io(format!("creating {}", dst.display()), e))?;
    let entries =
        fs::read_dir(src).map_err(|e| Error::io(format!("reading {}", src.display()), e))?;
    for entry in entries.flatten() {
        let name = entry.file_name();
        if name.to_string_lossy().starts_with('.') {
            continue; // .moss, .git and friends
        }
        if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            mirror_dirs(&entry.path(), &dst.join(&name))?;
        }
    }
    Ok(())
}

fn command_path(install: &TexInstall) -> std::ffi::OsString {
    let mut dirs = vec![install.bin_dir.clone()];
    if let Some(perl) = install.perl.as_ref().and_then(|p| p.parent()) {
        dirs.push(perl.to_path_buf());
    }
    if let Some(existing) = std::env::var_os("PATH") {
        dirs.extend(std::env::split_paths(&existing));
    }
    std::env::join_paths(dirs).unwrap_or_default()
}

/// One compile at a time per Build cache. The app, an Agent's `compile` and `view_pages` can all
/// start one, and two `latexmk` runs writing the same `.aux`, `.out` and PDF leave a truncated
/// PDF ("Page dictionary kid reference points to wrong type of object") and a half-written
/// `.out` ("File ended while scanning use of \@@BOOKMARK"). The second compile waits its turn,
/// then finds nothing left to rebuild. The lock is an operating-system file lock, so it frees
/// itself if a process dies.
struct BuildLock {
    _file: fs::File,
}

impl BuildLock {
    fn acquire(build_dir: &Path, wait: Duration) -> Result<BuildLock> {
        let path = build_dir.join("compile.lock");
        let file = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(&path)
            .map_err(|e| Error::io(format!("opening {}", path.display()), e))?;
        let waited = Instant::now();
        loop {
            match file.try_lock() {
                Ok(()) => return Ok(BuildLock { _file: file }),
                Err(fs::TryLockError::WouldBlock) if waited.elapsed() < wait => {
                    std::thread::sleep(Duration::from_millis(100));
                }
                Err(fs::TryLockError::WouldBlock) => {
                    return Err(Error::Compile(
                        "Another compile of this Project is still running. Wait for it to finish, then compile again."
                            .into(),
                    ));
                }
                Err(fs::TryLockError::Error(e)) => {
                    return Err(Error::io("locking the Build cache", e));
                }
            }
        }
    }
}

/// Stop `latexmk` and everything it started. `Child::kill` ends only `latexmk` itself; on Windows
/// its `pdflatex` keeps running and goes on writing the PDF after the compile has been reported
/// as timed out.
pub(crate) fn kill_tree(child: &mut std::process::Child) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let _ = Command::new("taskkill")
            .args(["/PID", &child.id().to_string(), "/T", "/F"])
            .creation_flags(0x0800_0000)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    let _ = child.kill();
}

pub fn compile(req: &CompileRequest<'_>) -> Result<CompileOutcome> {
    check_relative(req.root_file)?;
    if req.install.perl.is_none() {
        return Err(Error::PerlNotFound);
    }
    let root_path = req.project_dir.join(req.root_file);
    if !root_path.is_file() {
        return Err(Error::RootFileMissing(req.root_file.to_string()));
    }
    mirror_dirs(req.project_dir, req.build_dir)?;
    let _lock = BuildLock::acquire(req.build_dir, req.timeout)?;

    let mut run = attempt(req, false)?;
    let mut forced_rebuild = false;
    if !run.timed_out && run.exit_code != Some(0) && reused_failed_run(&run.console) {
        // latexmk saw nothing changed and replayed the last failure, so the diagnostics would be
        // stale: a fixed error would still be reported. Run again, forcing every pass.
        run = attempt(req, true)?;
        forced_rebuild = true;
    }

    let stem = Path::new(req.root_file)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let pdf_path = req.build_dir.join(format!("{stem}.pdf"));
    // A PDF older than this compile is left over from an earlier run, so ignore it, unless
    // latexmk succeeded: then it found nothing to rebuild and the existing PDF is current.
    let up_to_date = run.exit_code == Some(0) && pdf_path.is_file();
    let fresh = up_to_date
        || fs::metadata(&pdf_path)
            .and_then(|m| m.modified())
            .map(|t| t >= run.started_at - Duration::from_secs(1))
            .unwrap_or(false);

    let log_text = fs::read(req.build_dir.join(format!("{stem}.log")))
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .unwrap_or_default();

    Ok(CompileOutcome {
        success: run.exit_code == Some(0),
        exit_code: run.exit_code,
        timed_out: run.timed_out,
        pdf: fresh.then_some(pdf_path),
        duration: run.duration,
        diagnostics: parse_log(&log_text),
        console: run.console,
        log: log_text,
        forced_rebuild,
    })
}

struct Attempt {
    exit_code: Option<i32>,
    timed_out: bool,
    started_at: SystemTime,
    duration: Duration,
    console: String,
}

/// Does the console show `latexmk` answering from a previous failed run instead of compiling?
fn reused_failed_run(console: &str) -> bool {
    console.contains("gave an error in previous invocation")
}

/// One `latexmk` run. `force` adds `-g`, which redoes every pass whatever the files say.
fn attempt(req: &CompileRequest<'_>, force: bool) -> Result<Attempt> {
    let console_path = req.build_dir.join("latexmk.console.txt");
    let console_file = fs::File::create(&console_path)
        .map_err(|e| Error::io(format!("creating {}", console_path.display()), e))?;
    let console_err = console_file
        .try_clone()
        .map_err(|e| Error::io("cloning console log handle", e))?;

    let mut cmd = Command::new(&req.install.latexmk);
    cmd.current_dir(req.project_dir)
        .env("PATH", command_path(req.install))
        .arg(req.engine.latexmk_flag())
        .arg("-interaction=nonstopmode")
        .arg("-file-line-error")
        .arg("-synctex=1")
        .arg("-f")
        .arg(format!("-outdir={}", req.build_dir.display()));
    if force {
        cmd.arg("-g");
    }
    if req.shell_escape {
        cmd.arg("-shell-escape");
    }
    if req.install.flavor == Flavor::MiKTeX {
        // Install missing packages silently. Without this MiKTeX opens one "install package?"
        // dialog per package, which blocks the compile and is useless for a background run.
        cmd.arg(req.engine.latexmk_command("--enable-installer"));
    }
    cmd.arg(req.root_file)
        .stdin(Stdio::null())
        .stdout(Stdio::from(console_file))
        .stderr(Stdio::from(console_err));
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW: no console flash from the GUI
    }

    let started_at = SystemTime::now();
    let timer = Instant::now();
    let mut child = cmd
        .spawn()
        .map_err(|e| Error::io(format!("starting {}", req.install.latexmk.display()), e))?;

    let (exit_code, timed_out) = loop {
        match child.try_wait() {
            Ok(Some(status)) => break (status.code(), false),
            Ok(None) if timer.elapsed() >= req.timeout => {
                kill_tree(&mut child);
                let _ = child.wait();
                break (None, true);
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(25)),
            Err(e) => return Err(Error::io("waiting for latexmk", e)),
        }
    };
    Ok(Attempt {
        exit_code,
        timed_out,
        started_at,
        duration: timer.elapsed(),
        console: fs::read(&console_path)
            .map(|b| String::from_utf8_lossy(&b).into_owned())
            .unwrap_or_default(),
    })
}

/// Pull errors (`file:line: message`, from `-file-line-error`) and warnings out of a TeX log.
pub fn parse_log(log: &str) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for line in log.lines() {
        if let Some(d) = parse_error_line(line) {
            out.push(d);
        } else if let Some(msg) = line.strip_prefix("LaTeX Warning: ") {
            let line_no = msg
                .rsplit_once("on input line ")
                .and_then(|(_, n)| n.trim_end_matches('.').trim().parse::<u32>().ok());
            out.push(Diagnostic {
                severity: Severity::Warning,
                message: msg.to_string(),
                file: None,
                line: line_no,
            });
        }
    }
    out
}

/// Overfull and underfull boxes: text running into the margin, or lines stretched thin. Not
/// errors, but what a person sees as a badly set page. The line is where the paragraph starts,
/// and the file is the one TeX was reading (worked out from the log's parentheses).
pub fn parse_layout_warnings(log: &str) -> Vec<Diagnostic> {
    layout_boxes(log)
        .into_iter()
        .map(|b| Diagnostic {
            severity: Severity::Warning,
            message: b.message,
            file: b.file,
            line: b.line,
        })
        .collect()
}

#[derive(Debug, Clone, Serialize)]
pub struct BoxWarning {
    /// `overfull_hbox`, `underfull_hbox`, `overfull_vbox` or `underfull_vbox`.
    pub kind: &'static str,
    pub file: Option<String>,
    pub line: Option<u32>,
    /// How far an overfull box sticks out, in points.
    pub pt: Option<f64>,
    /// How loose an underfull box is (TeX's badness, 10000 at worst).
    pub badness: Option<u32>,
    pub message: String,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct BoxSummary {
    pub overfull_hbox: usize,
    pub overfull_vbox: usize,
    pub underfull_hbox: usize,
    pub underfull_vbox: usize,
    /// The worst few: the widest overfull boxes first, then the loosest underfull ones.
    pub worst: Vec<BoxWarning>,
}

pub fn box_summary(log: &str, worst: usize) -> BoxSummary {
    let boxes = layout_boxes(log);
    let mut summary = BoxSummary::default();
    for b in &boxes {
        match b.kind {
            "overfull_hbox" => summary.overfull_hbox += 1,
            "overfull_vbox" => summary.overfull_vbox += 1,
            "underfull_hbox" => summary.underfull_hbox += 1,
            _ => summary.underfull_vbox += 1,
        }
    }
    let weight = |w: &BoxWarning| w.pt.unwrap_or(0.0) * 1000.0 + f64::from(w.badness.unwrap_or(0)) / 100.0;
    let mut ranked = boxes;
    ranked.sort_by(|a, b| weight(b).total_cmp(&weight(a)));
    ranked.truncate(worst);
    summary.worst = ranked;
    summary
}

fn layout_boxes(log: &str) -> Vec<BoxWarning> {
    const KINDS: [(&str, &str); 4] = [
        ("Overfull \\hbox", "overfull_hbox"),
        ("Underfull \\hbox", "underfull_hbox"),
        ("Overfull \\vbox", "overfull_vbox"),
        ("Underfull \\vbox", "underfull_vbox"),
    ];
    let mut out = Vec::new();
    let mut files = FileStack::default();
    for line in log.lines() {
        let Some((_, kind)) = KINDS.into_iter().find(|(prefix, _)| line.starts_with(prefix)) else {
            files.feed(line);
            continue;
        };
        let at = |marker: &str| {
            line.split_once(marker)
                .and_then(|(_, rest)| rest.split(|c: char| !c.is_ascii_digit()).next())
                .and_then(|n| n.parse::<u32>().ok())
        };
        let pt = line
            .split_once('(')
            .and_then(|(_, rest)| rest.split(|c: char| !(c.is_ascii_digit() || c == '.')).next())
            .and_then(|n| n.parse::<f64>().ok());
        out.push(BoxWarning {
            kind,
            file: files.current(),
            line: at("lines ").or_else(|| at("at line ")),
            pt: if kind.starts_with("overfull") { pt } else { None },
            badness: at("badness "),
            message: line.trim().to_string(),
        });
    }
    out
}

/// Which source file TeX was reading, from the parentheses in its log: `(./chapters/a.tex` opens
/// a file and the matching `)` closes it. Other parentheses are tracked as anonymous entries so
/// the counts stay balanced. Log lines wrap, so this is a good guess, not a promise.
#[derive(Default)]
struct FileStack {
    stack: Vec<Option<String>>,
}

impl FileStack {
    fn feed(&mut self, line: &str) {
        // Message text has parentheses of its own; only lines that are file bookkeeping count.
        if line.starts_with("LaTeX ") || line.starts_with("Package ") || line.starts_with('!') {
            return;
        }
        for (i, c) in line.char_indices() {
            match c {
                '(' => {
                    let rest = &line[i + 1..];
                    let end = rest
                        .find(|c: char| c.is_whitespace() || c == '(' || c == ')')
                        .unwrap_or(rest.len());
                    let token = &rest[..end];
                    let lower = token.to_ascii_lowercase();
                    let is_source = lower.ends_with(".tex") || lower.ends_with(".bbl");
                    self.stack.push(is_source.then(|| token.trim_start_matches("./").replace('\\', "/")));
                }
                ')' => {
                    self.stack.pop();
                }
                _ => {}
            }
        }
    }

    fn current(&self) -> Option<String> {
        self.stack.iter().rev().find_map(|f| f.clone())
    }
}

/// Citation keys and label names the log reports as undefined, in order, without repeats.
pub fn undefined_in_log(log: &str) -> (Vec<String>, Vec<String>) {
    let mut cites = Vec::new();
    let mut refs = Vec::new();
    for line in log.lines() {
        let Some(msg) = line.strip_prefix("LaTeX Warning: ") else { continue };
        let target = if msg.starts_with("Citation `") {
            &mut cites
        } else if msg.starts_with("Reference `") {
            &mut refs
        } else {
            continue;
        };
        if !msg.contains(" undefined") {
            continue;
        }
        if let Some(name) = msg.split('`').nth(1).and_then(|r| r.split('\'').next()) {
            if !target.iter().any(|n| n == name) {
                target.push(name.to_string());
            }
        }
    }
    (cites, refs)
}

/// The end of `latexmk`'s console worth showing: font paths and other noise left out, and, when
/// the compile had errors, only the part from "Collected error summary" on if it is there.
pub fn console_summary(console: &str, has_errors: bool) -> String {
    let noise = |l: &str| {
        let t = l.trim();
        t.is_empty()
            || (t.starts_with('<') && t.ends_with('>'))
            || t.contains("\\fonts\\")
            || t.contains("/fonts/")
            || [".pfb", ".pfa", ".otf", ".ttf"].iter().any(|e| t.contains(e))
    };
    let kept: Vec<&str> = console.lines().filter(|l| !noise(l)).collect();
    let from = if has_errors {
        kept.iter().position(|l| l.contains("Collected error summary"))
    } else {
        None
    };
    let lines = match from {
        Some(i) => &kept[i..],
        None => &kept[kept.len().saturating_sub(15)..],
    };
    lines.join("\n")
}

fn parse_error_line(line: &str) -> Option<Diagnostic> {
    // Windows drive letters ("C:/x.tex:3: ...") also contain a colon, so split from the left
    // only after skipping an optional drive prefix.
    let (prefix_len, rest) = match line.as_bytes() {
        [d, b':', ..] if d.is_ascii_alphabetic() => (2, &line[2..]),
        _ => (0, line),
    };
    let (file, rest) = rest.split_once(':')?;
    let (ln, msg) = rest.split_once(':')?;
    let line_no = ln.trim().parse::<u32>().ok()?;
    let file = format!("{}{}", &line[..prefix_len], file);
    let looks_like_file = file.contains('.') && !file.contains(' ');
    if !looks_like_file || msg.trim().is_empty() {
        return None;
    }
    Some(Diagnostic {
        severity: Severity::Error,
        message: msg.trim().to_string(),
        file: Some(file.trim_start_matches("./").to_string()),
        line: Some(line_no),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_file_line_errors_and_warnings() {
        let log = "\
./Chapter2.tex:22: LaTeX Error: Unknown float option H.
LaTeX Warning: Citation x on page 1 undefined on input line 12.
some unrelated line: with a colon
";
        let d = parse_log(log);
        assert_eq!(d.len(), 2);
        assert_eq!(d[0].severity, Severity::Error);
        assert_eq!(d[0].file.as_deref(), Some("Chapter2.tex"));
        assert_eq!(d[0].line, Some(22));
        assert_eq!(d[1].severity, Severity::Warning);
        assert_eq!(d[1].line, Some(12));
    }

    #[test]
    fn layout_warnings_name_the_lines_of_badly_set_paragraphs() {
        let log = "\
Overfull \\hbox (12.3pt too wide) in paragraph at lines 40--44
Underfull \\hbox (badness 10000) in alignment at lines 7--9
Overfull \\vbox (3.0pt too high) has occurred while \\output is active
Overfull \\hbox (1.0pt too wide) detected at line 15
LaTeX Warning: not a box.
";
        let w = parse_layout_warnings(log);
        assert_eq!(w.len(), 4);
        assert_eq!(w[0].line, Some(40));
        assert_eq!(w[1].line, Some(7));
        assert_eq!(w[2].line, None);
        assert_eq!(w[3].line, Some(15));
        assert!(w.iter().all(|d| d.severity == Severity::Warning));
    }

    #[test]
    fn boxes_name_the_file_and_are_ranked_worst_first() {
        let log = "(./main.tex (./chapters/one.tex\n\
Overfull \\hbox (2.5pt too wide) in paragraph at lines 10--12\n\
Underfull \\hbox (badness 10000) in paragraph at lines 20--21\n\
) (./chapters/two.tex\n\
Overfull \\hbox (31.0pt too wide) in paragraph at lines 4--5\n\
)\n\
Overfull \\hbox (1.0pt too wide) in paragraph at lines 99--99\n\
)";
        let s = box_summary(log, 2);
        assert_eq!((s.overfull_hbox, s.underfull_hbox), (3, 1));
        assert_eq!(s.worst.len(), 2);
        assert_eq!(s.worst[0].file.as_deref(), Some("chapters/two.tex"));
        assert_eq!(s.worst[0].pt, Some(31.0));
        let all = parse_layout_warnings(log);
        assert_eq!(all[0].file.as_deref(), Some("chapters/one.tex"));
        assert_eq!(all[3].file.as_deref(), Some("main.tex"));
    }

    #[test]
    fn undefined_citations_and_references_are_listed_once() {
        let log = "LaTeX Warning: Citation `smith2020' on page 3 undefined on input line 9.\n\
LaTeX Warning: Citation `smith2020' on page 4 undefined on input line 30.\n\
LaTeX Warning: Reference `fig:a' on page 1 undefined on input line 5.\n\
LaTeX Warning: Label `x' multiply defined.\n";
        let (cites, refs) = undefined_in_log(log);
        assert_eq!(cites, vec!["smith2020"]);
        assert_eq!(refs, vec!["fig:a"]);
    }

    #[test]
    fn console_summary_drops_font_paths_and_keeps_the_error_summary() {
        let console = "Run number 1\n<C:\\Users\\x\\fonts\\type1\\a.pfb>\nLatexmk: Errors\nCollected error summary (may duplicate other messages):\n  pdflatex: gave an error\n";
        assert_eq!(
            console_summary(console, true),
            "Collected error summary (may duplicate other messages):\n  pdflatex: gave an error"
        );
        assert!(!console_summary(console, false).contains("pfb"));
    }

    #[test]
    fn a_replayed_failure_is_recognised() {
        assert!(reused_failed_run("Latexmk: Nothing to do\nlatexmk: gave an error in previous invocation of pdflatex"));
        assert!(!reused_failed_run("Run number 1 of rule 'pdflatex'"));
    }

    #[test]
    fn handles_windows_drive_letters() {
        let d = parse_log("C:/proj/a.tex:7: Undefined control sequence.");
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].file.as_deref(), Some("C:/proj/a.tex"));
        assert_eq!(d[0].line, Some(7));
    }

    #[test]
    fn rejects_paths_that_escape_the_project() {
        assert!(check_relative("../evil.tex").is_err());
        assert!(check_relative("C:/evil.tex").is_err());
        assert!(check_relative("chapters/intro.tex").is_ok());
    }
}
