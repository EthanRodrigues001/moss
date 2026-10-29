//! End-to-end compile tests. They are skipped (with a note) when no TeX install is present,
//! so machines without LaTeX stay green.

use std::fs;
use std::path::Path;
use std::time::Duration;

use moss_core::compile::{CompileRequest, Severity, compile};
use moss_core::tex::{self, Engine, TexInstall};

fn copy_dir(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).unwrap();
    for e in fs::read_dir(src).unwrap().flatten() {
        let to = dst.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_dir(&e.path(), &to);
        } else {
            fs::copy(e.path(), to).unwrap();
        }
    }
}

fn usable_install() -> Option<TexInstall> {
    let Ok(install) = tex::detect(None) else {
        eprintln!("skipped: no TeX installation");
        return None;
    };
    if install.perl.is_none() {
        eprintln!("skipped: no Perl for latexmk");
        return None;
    }
    Some(install)
}

fn request<'a>(install: &'a TexInstall, project: &'a Path, build: &'a Path) -> CompileRequest<'a> {
    CompileRequest {
        install,
        project_dir: project,
        root_file: "main.tex",
        engine: Engine::Pdflatex,
        build_dir: build,
        shell_escape: false,
        timeout: Duration::from_secs(180),
    }
}

fn fixture_copy(tmp: &Path) -> std::path::PathBuf {
    let project = tmp.join("proj");
    copy_dir(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mini"),
        &project,
    );
    project
}

#[test]
fn compiles_multi_file_project_with_include_and_bibtex() {
    let Some(install) = usable_install() else { return };
    let tmp = tempfile::tempdir().unwrap();
    let project = fixture_copy(tmp.path());
    let build = project.join(".moss/build/main");

    let out = compile(&request(&install, &project, &build)).unwrap();
    assert!(out.success, "console:\n{}", out.console);
    assert!(out.pdf.as_ref().is_some_and(|p| p.is_file()));
    assert!(out.diagnostics.iter().all(|d| d.severity != Severity::Error));
    assert!(build.join("main.bbl").is_file(), "bibliography pass did not run:\n{}", out.console);

    let again = compile(&request(&install, &project, &build)).unwrap();
    assert!(again.success);
    assert!(
        again.pdf.as_ref().is_some_and(|p| p.is_file()),
        "a no-change rebuild must still report the existing PDF"
    );
    eprintln!("cold {:?}, warm {:?}", out.duration, again.duration);
}

#[test]
fn errors_still_produce_a_pdf_and_point_at_the_line() {
    let Some(install) = usable_install() else { return };
    let tmp = tempfile::tempdir().unwrap();
    let project = fixture_copy(tmp.path());
    fs::write(
        project.join("chapters/intro.tex"),
        "\\chapter{Intro}\n\\begin{itemize}\nbroken \\undefinedmacro here\n\\end{itemize}\n",
    )
    .unwrap();
    let build = project.join(".moss/build/main");

    let out = compile(&request(&install, &project, &build)).unwrap();
    assert!(!out.success);
    assert!(out.pdf.is_some(), "Overleaf-style: a PDF despite errors");
    let err = out
        .diagnostics
        .iter()
        .find(|d| d.severity == Severity::Error)
        .expect("an error diagnostic");
    assert_eq!(err.file.as_deref(), Some("chapters/intro.tex"));
    assert_eq!(err.line, Some(3));
}

#[test]
fn synctex_jumps_both_ways_between_a_chapter_and_the_pdf() {
    let Some(install) = usable_install() else { return };
    let tmp = tempfile::tempdir().unwrap();
    let project = fixture_copy(tmp.path());
    let build = project.join(".moss/build/main");
    let out = compile(&request(&install, &project, &build)).unwrap();
    let pdf = out.pdf.expect("pdf");

    // Source line in a sub-folder chapter -> a spot on a page.
    let hits = moss_core::synctex::forward(&install, &project, &pdf, "chapters/intro.tex", 2, 1).unwrap();
    let Some(fwd) = hits.first().cloned() else {
        eprintln!("skipped: synctex program not available");
        return;
    };
    assert!(fwd.page >= 1);

    // ...and clicking that spot leads back to the same file.
    let back = moss_core::synctex::inverse(&install, &project, &pdf, fwd.page, fwd.x + fwd.width / 2.0, fwd.y + fwd.height / 2.0)
        .unwrap()
        .expect("a source location");
    assert_eq!(back.file, "chapters/intro.tex");
    assert!(back.line >= 1);
}
