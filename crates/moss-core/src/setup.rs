//! First-run setup: report what this PC already has for compiling, and install what is missing.
//!
//! Everything is per-user (no admin prompt). Downloads are HTTPS only, resumable, and checked
//! against a SHA-256 pinned in this file before anything is run or unpacked. TeX Live is never
//! installed over; Moss only reports what is missing.

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::tex::{self, Flavor, TexInstall};
use crate::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PartId {
    Miktex,
    Perl,
}

/// A file Moss may download. Pinned: when a new MiKTeX or Perl comes out, update the URL and hash
/// here after checking the publisher's own checksum.
#[derive(Debug, Clone)]
pub struct Source {
    pub url: String,
    pub sha256: String,
    pub file: String,
    /// Expected size in bytes, for the dialog before the download starts.
    pub size: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct PartInfo {
    pub id: PartId,
    pub name: String,
    pub size: u64,
}

pub fn miktex_source() -> Source {
    Source {
        url: "https://miktex.org/download/ctan/systems/win32/miktex/setup/windows-x64/basic-miktex-25.12-x64.exe".into(),
        sha256: "14b42dd9f4b4a7813a8bfd69c8f99316c2888cc4ee26f631f397e163d85d6c62".into(),
        file: "basic-miktex-25.12-x64.exe".into(),
        size: 148_882_184,
    }
}

pub fn perl_source() -> Source {
    Source {
        url: "https://github.com/StrawberryPerl/Perl-Dist-Strawberry/releases/download/SP_54231_64bit/strawberry-perl-5.42.3.1-64bit-portable.zip".into(),
        sha256: "6a081a811781c30aca51dbc036afd93092af91e3297901f02c17043795a10690".into(),
        file: "strawberry-perl-5.42.3.1-64bit-portable.zip".into(),
        size: 304_765_269,
    }
}

pub fn part_info(id: PartId) -> PartInfo {
    match id {
        PartId::Miktex => PartInfo {
            id,
            name: "MiKTeX 25.12 (TeX distribution, for you only)".into(),
            size: miktex_source().size,
        },
        PartId::Perl => PartInfo {
            id,
            name: "Strawberry Perl 5.42 (portable, needed by latexmk)".into(),
            size: perl_source().size,
        },
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AutoInstall {
    /// MiKTeX installs missing packages by itself.
    Yes,
    /// MiKTeX asks first, which blocks a background compile.
    Ask,
    No,
}

#[derive(Debug, Serialize)]
pub struct SetupReport {
    pub tex: Option<TexInstall>,
    pub perl: Option<PathBuf>,
    pub synctex: bool,
    pub pdftoppm: bool,
    pub auto_install: Option<AutoInstall>,
    /// What one click would install, in order. Empty means Moss can compile already.
    pub missing: Vec<PartInfo>,
    /// TeX Live (or an unknown distribution) is in use: Moss reports but never installs MiKTeX.
    pub foreign_tex: bool,
}

/// Look at this PC. Read-only and fast.
pub fn check(custom_bin_dir: Option<&Path>) -> SetupReport {
    let tex = tex::detect(custom_bin_dir).ok();
    let bin = tex.as_ref().map(|t| t.bin_dir.clone());
    let has = |name: &str| bin.as_deref().is_some_and(|d| exe_in(d, name));
    let perl = tex.as_ref().and_then(|t| t.perl.clone());
    let foreign_tex = tex
        .as_ref()
        .is_some_and(|t| t.flavor != Flavor::MiKTeX);
    let auto_install = tex
        .as_ref()
        .filter(|t| t.flavor == Flavor::MiKTeX)
        .and_then(|t| miktex_auto_install(&t.bin_dir));

    let mut missing = Vec::new();
    if tex.is_none() {
        missing.push(part_info(PartId::Miktex));
    }
    if perl.is_none() && !foreign_tex {
        missing.push(part_info(PartId::Perl));
    }
    SetupReport {
        synctex: has("synctex"),
        pdftoppm: has("pdftoppm"),
        tex,
        perl,
        auto_install,
        missing,
        foreign_tex,
    }
}

fn exe_in(dir: &Path, name: &str) -> bool {
    dir.join(format!("{name}.exe")).is_file()
}

fn miktex_auto_install(bin_dir: &Path) -> Option<AutoInstall> {
    let out = hidden(Command::new(bin_dir.join("initexmf.exe")))
        .arg("--show-config-value=[MPM]AutoInstall")
        .stdin(Stdio::null())
        .output()
        .ok()?;
    match String::from_utf8_lossy(&out.stdout).trim() {
        "1" => Some(AutoInstall::Yes),
        "2" => Some(AutoInstall::Ask),
        "0" => Some(AutoInstall::No),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    Download,
    Verify,
    Install,
    Configure,
}

#[derive(Debug, Clone, Serialize)]
pub struct Progress {
    pub part: PartId,
    pub phase: Phase,
    pub done: u64,
    pub total: Option<u64>,
    pub note: String,
}

pub type OnProgress<'a> = &'a mut dyn FnMut(Progress);

fn hidden(mut cmd: Command) -> Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    cmd
}

/// HTTPS only. Plain HTTP is allowed to the loopback address so tests can run a local server.
fn check_url(url: &str) -> Result<()> {
    let loopback = url.starts_with("http://127.0.0.1") || url.starts_with("http://localhost");
    if url.starts_with("https://") || loopback {
        Ok(())
    } else {
        Err(Error::Setup(format!("refusing to download over an insecure link: {url}")))
    }
}

fn sha256_file(path: &Path, cancel: &AtomicBool) -> Result<String> {
    let mut file =
        fs::File::open(path).map_err(|e| Error::io(format!("opening {}", path.display()), e))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(Error::Cancelled);
        }
        let n = file
            .read(&mut buf)
            .map_err(|e| Error::io(format!("reading {}", path.display()), e))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Download `source` into `dir` and return the verified file. A finished, matching file is reused;
/// an interrupted `.part` file is resumed. A hash mismatch deletes the file and fails.
pub fn download(
    part: PartId,
    source: &Source,
    dir: &Path,
    cancel: &AtomicBool,
    on: OnProgress<'_>,
) -> Result<PathBuf> {
    check_url(&source.url)?;
    fs::create_dir_all(dir).map_err(|e| Error::io("creating the setup folder", e))?;
    let target = dir.join(&source.file);
    let partial = dir.join(format!("{}.part", source.file));
    let progress = |phase, done, total: Option<u64>| Progress {
        part,
        phase,
        done,
        total,
        note: String::new(),
    };

    if target.is_file() {
        on(progress(Phase::Verify, 0, None));
        if sha256_file(&target, cancel)?.eq_ignore_ascii_case(&source.sha256) {
            return Ok(target);
        }
        let _ = fs::remove_file(&target);
    }

    let client = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(20))
        .timeout(Duration::from_secs(2 * 60 * 60)) // a stalled link gives up instead of hanging
        .build()
        .map_err(|e| Error::Setup(format!("could not start the download: {e}")))?;

    let mut restarted = false;
    loop {
        let offset = fs::metadata(&partial).map(|m| m.len()).unwrap_or(0);
        let mut req = client.get(&source.url);
        if offset > 0 {
            req = req.header("Range", format!("bytes={offset}-"));
        }
        let mut resp = req
            .send()
            .map_err(|e| Error::Setup(format!("download failed: {e}")))?;
        let status = resp.status();
        if status.as_u16() == 416 && !restarted {
            // The partial file does not fit what the server has. Start again from nothing.
            let _ = fs::remove_file(&partial);
            restarted = true;
            continue;
        }
        if !status.is_success() {
            return Err(Error::Setup(format!("download failed: the server answered {status}")));
        }
        let resumed = status.as_u16() == 206;
        let start = if resumed { offset } else { 0 };
        let total = resp.content_length().map(|n| n + start);

        let mut out = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .append(resumed)
            .truncate(!resumed)
            .open(&partial)
            .map_err(|e| Error::io(format!("opening {}", partial.display()), e))?;
        let mut done = start;
        let mut last_report = start;
        let mut buf = vec![0u8; 64 * 1024];
        on(progress(Phase::Download, done, total));
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(Error::Cancelled); // the .part file stays, so the next try resumes
            }
            let n = resp
                .read(&mut buf)
                .map_err(|e| Error::Setup(format!("download interrupted: {e}")))?;
            if n == 0 {
                break;
            }
            out.write_all(&buf[..n])
                .map_err(|e| Error::io("writing the download", e))?;
            done += n as u64;
            if done - last_report >= 512 * 1024 {
                last_report = done;
                on(progress(Phase::Download, done, total));
            }
        }
        out.flush().map_err(|e| Error::io("writing the download", e))?;
        drop(out);
        if total.is_some_and(|total| done < total) {
            return Err(Error::Setup(
                "download ended early; try again to continue where it stopped".into(),
            ));
        }
        on(progress(Phase::Download, done, total));
        break;
    }

    on(progress(Phase::Verify, 0, None));
    let got = sha256_file(&partial, cancel)?;
    if !got.eq_ignore_ascii_case(&source.sha256) {
        let _ = fs::remove_file(&partial);
        return Err(Error::Setup(format!(
            "the downloaded file does not match the expected checksum (expected {}, got {got}); nothing was installed",
            source.sha256
        )));
    }
    fs::rename(&partial, &target).map_err(|e| Error::io("finishing the download", e))?;
    Ok(target)
}

/// Run `cmd` and wait, ending it (and anything it started) when `cancel` is set.
fn run_cancellable(cmd: Command, cancel: &AtomicBool) -> Result<std::process::ExitStatus> {
    let mut child = hidden(cmd)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| Error::io("starting the installer", e))?;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) if cancel.load(Ordering::Relaxed) => {
                crate::compile::kill_tree(&mut child);
                let _ = child.wait();
                return Err(Error::Cancelled);
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(200)),
            Err(e) => return Err(Error::io("waiting for the installer", e)),
        }
    }
}

/// Unpack a zip into `dest`, refusing entries that would land outside it.
pub fn extract_zip(
    zip_path: &Path,
    dest: &Path,
    part: PartId,
    cancel: &AtomicBool,
    on: OnProgress<'_>,
) -> Result<()> {
    let file = fs::File::open(zip_path).map_err(|e| Error::io("opening the download", e))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| Error::Zip(e.to_string()))?;
    let total = archive.len() as u64;
    for i in 0..archive.len() {
        if cancel.load(Ordering::Relaxed) {
            return Err(Error::Cancelled);
        }
        let mut entry = archive.by_index(i).map_err(|e| Error::Zip(e.to_string()))?;
        let Some(rel) = entry.enclosed_name() else {
            let name = String::from_utf8_lossy(entry.name_raw()).into_owned();
            return Err(Error::Setup(format!("unsafe path in the archive: {name}")));
        };
        let out_path = dest.join(rel);
        if entry.is_dir() {
            fs::create_dir_all(&out_path).map_err(|e| Error::io("unpacking", e))?;
        } else {
            if let Some(parent) = out_path.parent() {
                fs::create_dir_all(parent).map_err(|e| Error::io("unpacking", e))?;
            }
            let mut out = fs::File::create(&out_path)
                .map_err(|e| Error::io(format!("writing {}", out_path.display()), e))?;
            std::io::copy(&mut entry, &mut out)
                .map_err(|e| Error::io(format!("writing {}", out_path.display()), e))?;
        }
        if i % 200 == 0 {
            on(Progress {
                part,
                phase: Phase::Install,
                done: i as u64,
                total: Some(total),
                note: String::new(),
            });
        }
    }
    Ok(())
}

/// Where downloads are kept: `%LOCALAPPDATA%\Moss\setup`.
pub fn setup_dir() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_default()
        .join("Moss/setup")
}

/// Install Perl into `perl_home` (the folder that will hold `perl\bin`). It is unpacked beside the
/// final folder and swapped in, so a cancelled or failed unpack leaves nothing half-installed.
pub fn install_perl(
    source: &Source,
    downloads: &Path,
    perl_home: &Path,
    cancel: &AtomicBool,
    on: OnProgress<'_>,
) -> Result<()> {
    let zip_path = download(PartId::Perl, source, downloads, cancel, on)?;
    let staging = perl_home.with_extension("new");
    let _ = fs::remove_dir_all(&staging);
    let result = extract_zip(&zip_path, &staging, PartId::Perl, cancel, on);
    if let Err(e) = result {
        let _ = fs::remove_dir_all(&staging);
        return Err(e);
    }
    let _ = fs::remove_dir_all(perl_home);
    if let Some(parent) = perl_home.parent() {
        fs::create_dir_all(parent).map_err(|e| Error::io("creating the Perl folder", e))?;
    }
    fs::rename(&staging, perl_home).map_err(|e| Error::io("placing Perl", e))?;
    let perl = perl_home.join("perl/bin/perl.exe");
    let ok = hidden(Command::new(&perl))
        .arg("-e")
        .arg("print 1")
        .stdin(Stdio::null())
        .output()
        .map(|o| o.status.success() && o.stdout == b"1")
        .unwrap_or(false);
    if !ok {
        return Err(Error::Setup(format!(
            "Perl was unpacked but did not run ({}); it was left in place for inspection",
            perl.display()
        )));
    }
    Ok(())
}

/// Install MiKTeX for the current user, then switch on on-the-fly package installs.
pub fn install_miktex(
    source: &Source,
    downloads: &Path,
    cancel: &AtomicBool,
    on: OnProgress<'_>,
) -> Result<TexInstall> {
    let setup = download(PartId::Miktex, source, downloads, cancel, on)?;
    on(Progress {
        part: PartId::Miktex,
        phase: Phase::Install,
        done: 0,
        total: None,
        note: "Installing MiKTeX; this takes a few minutes".into(),
    });
    let mut cmd = Command::new(&setup);
    cmd.args(["--unattended", "--private", "--package-set=basic"]);
    let status = run_cancellable(cmd, cancel)?;
    if !status.success() {
        return Err(Error::Setup(format!(
            "the MiKTeX installer stopped with {status}; nothing else was changed"
        )));
    }
    let install = tex::detect(None).map_err(|_| {
        Error::Setup("MiKTeX finished installing but latexmk was not found".into())
    })?;
    configure_miktex(&install.bin_dir, on)?;
    Ok(install)
}

/// Let MiKTeX fetch missing packages without asking, and refresh its package list.
pub fn configure_miktex(bin_dir: &Path, on: OnProgress<'_>) -> Result<()> {
    on(Progress {
        part: PartId::Miktex,
        phase: Phase::Configure,
        done: 0,
        total: None,
        note: "Turning on automatic package installs".into(),
    });
    let status = hidden(Command::new(bin_dir.join("initexmf.exe")))
        .arg("--set-config-value=[MPM]AutoInstall=1")
        .stdin(Stdio::null())
        .status()
        .map_err(|e| Error::io("running initexmf", e))?;
    if !status.success() {
        return Err(Error::Setup("could not turn on automatic package installs".into()));
    }
    // Best effort: a stale list only means the first compile looks a package up itself.
    let _ = hidden(Command::new(bin_dir.join("miktex.exe")))
        .args(["packages", "update-package-database"])
        .stdin(Stdio::null())
        .status();
    Ok(())
}

/// Packages the Templates and most papers use, so the first real compile does not stall.
pub const COMMON_PACKAGES: &[&str] = &[
    "amsmath", "amsfonts", "amscls", "biblatex", "biber", "pgf", "hyperref", "geometry", "IEEEtran",
    "beamer", "microtype", "siunitx", "cleveref", "booktabs", "graphicx", "xcolor", "listings",
    "enumitem", "fancyhdr", "natbib",
];

/// Install `packages` one at a time so the dialog can show progress. A package that is already
/// there, or that MiKTeX does not know, is skipped rather than stopping the rest.
pub fn warm_packages(
    bin_dir: &Path,
    packages: &[&str],
    cancel: &AtomicBool,
    on: OnProgress<'_>,
) -> Result<()> {
    for (i, name) in packages.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return Err(Error::Cancelled);
        }
        on(Progress {
            part: PartId::Miktex,
            phase: Phase::Configure,
            done: i as u64,
            total: Some(packages.len() as u64),
            note: format!("Package {name}"),
        });
        let mut cmd = Command::new(bin_dir.join("miktex.exe"));
        cmd.args(["packages", "install", name]);
        // Already installed, unknown or failed is fine: a compile fetches what it needs.
        if let Err(Error::Cancelled) = run_cancellable(cmd, cancel) {
            return Err(Error::Cancelled);
        }
    }
    Ok(())
}

#[derive(Debug, Serialize)]
pub struct SelfTest {
    pub ok: bool,
    pub seconds: f64,
    pub pages: Option<u32>,
    /// What went wrong, with the fix to try, when `ok` is false.
    pub problem: Option<String>,
}

const TEST_DOC: &str = "\\documentclass{article}\n\\usepackage{amsmath}\n\\begin{document}\nMoss compiles: $e^{i\\pi}+1=0$.\n\\end{document}\n";

/// Compile a tiny document in a temporary folder with the detected install.
pub fn self_test(custom_bin_dir: Option<&Path>) -> SelfTest {
    let fail = |problem: String| SelfTest {
        ok: false,
        seconds: 0.0,
        pages: None,
        problem: Some(problem),
    };
    let install = match tex::detect(custom_bin_dir) {
        Ok(i) => i,
        Err(e) => return fail(format!("{e}. Install LaTeX from the setup screen.")),
    };
    if install.perl.is_none() {
        return fail("latexmk needs Perl, but none was found. Install Perl from the setup screen.".into());
    }
    let dir = match tempfile::tempdir() {
        Ok(d) => d,
        Err(e) => return fail(format!("could not make a temporary folder: {e}")),
    };
    let project = dir.path().join("p");
    let build = dir.path().join("build");
    if fs::create_dir_all(&project).is_err() || fs::write(project.join("main.tex"), TEST_DOC).is_err() {
        return fail("could not write the test document".into());
    }
    let outcome = crate::compile::compile(&crate::compile::CompileRequest {
        install: &install,
        project_dir: &project,
        root_file: "main.tex",
        engine: tex::Engine::Pdflatex,
        build_dir: &build,
        shell_escape: false,
        timeout: Duration::from_secs(300), // a first compile may download packages
    });
    match outcome {
        Ok(o) if o.success && o.pdf.is_some() => {
            let pages = o
                .pdf
                .as_deref()
                .and_then(|pdf| crate::pdfview::PdfTools::find(&install).ok()?.info(pdf).ok())
                .map(|i| i.pages);
            SelfTest {
                ok: true,
                seconds: o.duration.as_secs_f64(),
                pages,
                problem: None,
            }
        }
        Ok(o) if o.timed_out => fail("the test compile timed out; MiKTeX may still be downloading packages. Try again.".into()),
        Ok(o) => {
            let first = o
                .diagnostics
                .iter()
                .find(|d| d.severity == crate::compile::Severity::Error)
                .map(|d| d.message.clone())
                .unwrap_or_else(|| o.console.lines().last().unwrap_or("unknown error").to_string());
            fail(format!("the test compile failed: {first}"))
        }
        Err(e) => fail(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader};
    use std::net::TcpListener;

    fn sha(bytes: &[u8]) -> String {
        Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
    }

    /// A one-shot-per-connection HTTP server that serves `body`, honouring `Range: bytes=N-`.
    fn serve(body: Vec<u8>, slow: bool) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut range = 0usize;
                let mut line = String::new();
                while reader.read_line(&mut line).unwrap_or(0) > 0 {
                    if let Some(v) = line.to_ascii_lowercase().strip_prefix("range: bytes=") {
                        range = v.trim().trim_end_matches('-').parse().unwrap_or(0);
                    }
                    if line == "\r\n" {
                        break;
                    }
                    line.clear();
                }
                let (status, chunk) = if range > 0 {
                    ("206 Partial Content", &body[range..])
                } else {
                    ("200 OK", &body[..])
                };
                let head = format!(
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    chunk.len()
                );
                let _ = stream.write_all(head.as_bytes());
                for piece in chunk.chunks(8192) {
                    if stream.write_all(piece).is_err() {
                        break;
                    }
                    if slow {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                }
            }
        });
        format!("http://127.0.0.1:{port}/file.bin")
    }

    fn source(url: String, body: &[u8]) -> Source {
        Source {
            url,
            sha256: sha(body),
            file: "file.bin".into(),
            size: body.len() as u64,
        }
    }

    fn quiet(_: Progress) {}

    #[test]
    fn download_verifies_and_keeps_the_file() {
        let body: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
        let src = source(serve(body.clone(), false), &body);
        let dir = tempfile::tempdir().unwrap();
        let cancel = AtomicBool::new(false);
        let path = download(PartId::Perl, &src, dir.path(), &cancel, &mut quiet).unwrap();
        assert_eq!(fs::read(path).unwrap(), body);
        assert!(!dir.path().join("file.bin.part").exists());
    }

    #[test]
    fn a_wrong_hash_fails_and_deletes_the_download() {
        let body = vec![7u8; 10_000];
        let mut src = source(serve(body.clone(), false), &body);
        src.sha256 = sha(b"something else");
        let dir = tempfile::tempdir().unwrap();
        let cancel = AtomicBool::new(false);
        let err = download(PartId::Perl, &src, dir.path(), &cancel, &mut quiet).unwrap_err();
        assert!(err.to_string().contains("checksum"), "{err}");
        assert!(!dir.path().join("file.bin").exists());
        assert!(!dir.path().join("file.bin.part").exists());
    }

    #[test]
    fn cancel_stops_and_the_next_try_resumes() {
        let body: Vec<u8> = (0..2_000_000u32).map(|i| (i % 253) as u8).collect();
        let src = source(serve(body.clone(), true), &body);
        let dir = tempfile::tempdir().unwrap();
        let cancel = AtomicBool::new(false);
        let mut seen = 0u64;
        let err = download(PartId::Perl, &src, dir.path(), &cancel, &mut |p: Progress| {
            seen = p.done;
            if p.done > 600_000 {
                cancel.store(true, Ordering::Relaxed);
            }
        })
        .unwrap_err();
        assert!(matches!(err, Error::Cancelled), "{err}");
        assert!(seen > 600_000);
        let kept = fs::metadata(dir.path().join("file.bin.part")).unwrap().len();
        assert!(kept > 0 && (kept as usize) < body.len());

        cancel.store(false, Ordering::Relaxed);
        let path = download(PartId::Perl, &src, dir.path(), &cancel, &mut quiet).unwrap();
        assert_eq!(fs::read(path).unwrap(), body);
    }

    #[test]
    fn plain_http_to_other_hosts_is_refused() {
        let src = Source {
            url: "http://example.com/x.exe".into(),
            sha256: String::new(),
            file: "x.exe".into(),
            size: 0,
        };
        let dir = tempfile::tempdir().unwrap();
        let cancel = AtomicBool::new(false);
        let err = download(PartId::Miktex, &src, dir.path(), &cancel, &mut quiet).unwrap_err();
        assert!(err.to_string().contains("insecure"), "{err}");
    }

    #[test]
    fn zip_entries_cannot_escape_the_folder() {
        let dir = tempfile::tempdir().unwrap();
        let zip_path = dir.path().join("evil.zip");
        {
            let file = fs::File::create(&zip_path).unwrap();
            let mut w = zip::ZipWriter::new(file);
            let opts = zip::write::SimpleFileOptions::default();
            w.start_file("../outside.txt", opts).unwrap();
            w.write_all(b"x").unwrap();
            w.finish().unwrap();
        }
        let dest = dir.path().join("out");
        let cancel = AtomicBool::new(false);
        let err = extract_zip(&zip_path, &dest, PartId::Perl, &cancel, &mut quiet).unwrap_err();
        assert!(err.to_string().contains("unsafe"), "{err}");
        assert!(!dir.path().join("outside.txt").exists());
    }

    #[test]
    fn perl_is_unpacked_from_a_zip_and_swapped_in() {
        let dir = tempfile::tempdir().unwrap();
        let zip_path = dir.path().join("p.zip");
        {
            let file = fs::File::create(&zip_path).unwrap();
            let mut w = zip::ZipWriter::new(file);
            let opts = zip::write::SimpleFileOptions::default();
            w.start_file("perl/bin/perl.exe", opts).unwrap();
            w.write_all(b"not really perl").unwrap();
            w.finish().unwrap();
        }
        let dest = dir.path().join("perl");
        let cancel = AtomicBool::new(false);
        extract_zip(&zip_path, &dest, PartId::Perl, &cancel, &mut quiet).unwrap();
        assert!(dest.join("perl/bin/perl.exe").is_file());
    }

    #[test]
    fn self_test_compiles_when_tex_is_installed() {
        if tex::detect(None).map(|t| t.perl.is_some()).unwrap_or(false) {
            let result = self_test(None);
            assert!(result.ok, "{:?}", result.problem);
            assert_eq!(result.pages, Some(1));
        }
    }

    #[test]
    fn missing_parts_follow_what_was_found() {
        let report = check(Some(Path::new("Z:/definitely/not/here")));
        if report.tex.is_none() {
            assert_eq!(report.missing[0].id, PartId::Miktex);
        } else if report.perl.is_none() && !report.foreign_tex {
            assert_eq!(report.missing[0].id, PartId::Perl);
        } else {
            assert!(report.missing.is_empty());
        }
    }
}
