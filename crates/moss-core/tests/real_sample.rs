//! Runs a real zip through Import and Compile. Ignored by default because it needs a document
//! that is not in the repo. Run with:
//!
//!   MOSS_SAMPLE_ZIP=C:\path\to\report.zip cargo test -p moss-core --test real_sample -- --ignored --nocapture

use std::path::PathBuf;
use std::time::Duration;

use moss_core::compile::{CompileRequest, Severity, compile};
use moss_core::project::Origin;
use moss_core::tex::{self, detect_engine};
use moss_core::workspace::Workspace;

#[test]
#[ignore = "needs MOSS_SAMPLE_ZIP"]
fn import_and_compile_a_real_zip() {
    let Some(zip) = std::env::var_os("MOSS_SAMPLE_ZIP").map(PathBuf::from) else {
        eprintln!("MOSS_SAMPLE_ZIP not set");
        return;
    };
    let tmp = tempfile::tempdir().unwrap();
    let ws = Workspace::open(tmp.path()).unwrap();

    let report = ws.import_zip(&zip, None).unwrap();
    eprintln!(
        "imported `{}` -> {} files, {} ignored, root `{}`",
        report.project.name, report.files_imported, report.ignored, report.project.root_file
    );
    for s in &report.skipped {
        eprintln!("  needs attention: {} ({})", s.path, s.reason);
    }
    for w in &report.warnings {
        eprintln!("  warning: {w}");
    }
    assert_eq!(report.project.origin, Origin::Local);

    let project = ws.open_project(&report.project.dir_name).unwrap();
    let root_src = moss_core::fsutil::read_text(project.dir(), &project.meta.root_file).unwrap();
    let engine = detect_engine(&root_src);
    eprintln!("engine: {engine:?}");

    let install = tex::detect(None).expect("TeX installation");
    let build = project.build_dir(&project.meta.root_file);
    let req = CompileRequest {
        install: &install,
        project_dir: project.dir(),
        root_file: &project.meta.root_file,
        engine,
        build_dir: &build,
        shell_escape: false,
        timeout: Duration::from_secs(300),
    };

    let cold = compile(&req).unwrap();
    eprintln!(
        "cold: {:?}, success={}, pdf={}",
        cold.duration,
        cold.success,
        cold.pdf.is_some()
    );
    let warm = compile(&req).unwrap();
    eprintln!("warm: {:?}", warm.duration);

    let errors: Vec<_> = cold
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    eprintln!("{} errors, {} warnings", errors.len(), cold.diagnostics.len() - errors.len());
    for d in errors.iter().take(10) {
        eprintln!("  {}:{} {}", d.file.as_deref().unwrap_or("?"), d.line.unwrap_or(0), d.message);
    }
    assert!(cold.pdf.is_some(), "a PDF is produced even with errors");
}
