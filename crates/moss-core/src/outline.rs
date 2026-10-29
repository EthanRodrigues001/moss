//! Reading the structure of a LaTeX Project without compiling it: the section outline across
//! `\input`/`\include`d files in document order, the labels, references and citations, and the
//! entries of the `.bib` files. It answers "what is in this document" and "what is broken in
//! its cross-references" quickly enough for an agent to ask after every edit.
//!
//! This is a reader, not a TeX engine: macros that build commands or labels are not expanded, so
//! a result is a good guide, and the compile log stays the authority.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::Path;

use serde::Serialize;

use crate::fsutil;
use crate::{Error, Result};

const MAX_DEPTH: usize = 20;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Heading {
    pub file: String,
    /// 1-based line in `file`.
    pub line: u32,
    /// 0 part, 1 chapter, 2 section, 3 subsection, 4 subsubsection.
    pub level: u8,
    pub kind: String,
    pub title: String,
    /// `\section*{...}`: not numbered.
    pub starred: bool,
}

/// A name used or defined at a place: a label, a reference, a citation key, a missing file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Mention {
    pub name: String,
    pub file: String,
    pub line: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BibEntry {
    pub key: String,
    /// `article`, `book`, `inproceedings`, ...
    pub kind: String,
    pub title: Option<String>,
    pub author: Option<String>,
    pub year: Option<String>,
    pub file: String,
    pub line: u32,
}

#[derive(Debug, Default, Serialize)]
pub struct Scan {
    /// Every `.tex` file reached from the root file, in the order the document includes them.
    pub files: Vec<String>,
    pub headings: Vec<Heading>,
    pub labels: Vec<Mention>,
    pub refs: Vec<Mention>,
    pub cites: Vec<Mention>,
    /// `\nocite{*}`: every bibliography entry is meant to appear.
    pub nocite_all: bool,
    /// `\input{x}` / `\include{x}` whose file does not exist.
    pub missing_inputs: Vec<Mention>,
}

/// Read the root file and everything it includes.
pub fn scan(project_dir: &Path, root_file: &str) -> Result<Scan> {
    let mut out = Scan::default();
    let mut seen = HashSet::new();
    scan_file(project_dir, root_file, 0, &mut seen, &mut out)?;
    Ok(out)
}

fn scan_file(dir: &Path, rel: &str, depth: usize, seen: &mut HashSet<String>, out: &mut Scan) -> Result<()> {
    if depth > MAX_DEPTH || !seen.insert(rel.to_string()) {
        return Ok(()); // a cycle, or already read
    }
    let text = match fsutil::read_text(dir, rel) {
        Ok(t) => t,
        Err(Error::NotUtf8(_)) => return Ok(()),
        Err(e) => return Err(e),
    };
    out.files.push(rel.to_string());
    let here = |line: usize, name: String| Mention { name, file: rel.to_string(), line: line as u32 + 1 };

    for (i, raw) in text.lines().enumerate() {
        for cmd in commands(&strip_comment(raw)) {
            let Some(arg) = cmd.arg.as_deref() else { continue };
            match cmd.name.as_str() {
                "part" | "chapter" | "section" | "subsection" | "subsubsection" => {
                    let title = clean_title(arg);
                    if !title.is_empty() {
                        out.headings.push(Heading {
                            file: rel.to_string(),
                            line: i as u32 + 1,
                            level: level_of(&cmd.name),
                            kind: cmd.name.clone(),
                            title,
                            starred: cmd.star,
                        });
                    }
                }
                "label" => out.labels.push(here(i, arg.trim().to_string())),
                "ref" | "eqref" | "pageref" | "autoref" | "cref" | "Cref" | "nameref" | "vref" | "Vref"
                | "cpageref" => {
                    out.refs.extend(keys(arg).map(|k| here(i, k)));
                }
                name if name.contains("cite") && name != "citestyle" => {
                    if name == "nocite" && arg.trim() == "*" {
                        out.nocite_all = true;
                    } else {
                        out.cites.extend(keys(arg).map(|k| here(i, k)));
                    }
                }
                "input" | "include" | "subfile" | "subfileinclude" => {
                    let target = with_tex_extension(arg.trim());
                    if fsutil::safe_join(dir, &target).is_ok_and(|p| p.is_file()) {
                        scan_file(dir, &target, depth + 1, seen, out)?;
                    } else {
                        out.missing_inputs.push(here(i, target));
                    }
                }
                _ => {}
            }
        }
    }
    Ok(())
}

fn level_of(kind: &str) -> u8 {
    match kind {
        "part" => 0,
        "chapter" => 1,
        "section" => 2,
        "subsection" => 3,
        _ => 4,
    }
}

fn with_tex_extension(name: &str) -> String {
    let name = name.trim_start_matches("./");
    if Path::new(name).extension().is_some() { name.to_string() } else { format!("{name}.tex") }
}

fn keys(arg: &str) -> impl Iterator<Item = String> + '_ {
    arg.split(',').map(str::trim).filter(|k| !k.is_empty()).map(str::to_string)
}

/// A heading's title as plain text: formatting commands and braces removed.
fn clean_title(raw: &str) -> String {
    let mut out = String::new();
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                // Drop the command name, keep what follows; `\&` and friends stay as the symbol.
                if chars.peek().is_some_and(|n| n.is_ascii_alphabetic()) {
                    while chars.peek().is_some_and(|n| n.is_ascii_alphabetic()) {
                        chars.next();
                    }
                    if chars.peek() == Some(&'*') {
                        chars.next();
                    }
                } else if let Some(n) = chars.next() {
                    out.push(n);
                }
            }
            '{' | '}' => {}
            _ => out.push(c),
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[derive(Debug)]
struct Cmd {
    name: String,
    star: bool,
    /// The first `{...}` argument after any `[...]` options.
    arg: Option<String>,
}

/// `line` without its `%` comment (an escaped `\%` stays).
fn strip_comment(line: &str) -> String {
    let mut prev_backslash = false;
    for (i, c) in line.char_indices() {
        if c == '%' && !prev_backslash {
            return line[..i].to_string();
        }
        prev_backslash = c == '\\' && !prev_backslash;
    }
    line.to_string()
}

/// Every `\name*[opt]{arg}` on one line.
fn commands(line: &str) -> Vec<Cmd> {
    let chars: Vec<char> = line.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != '\\' {
            i += 1;
            continue;
        }
        i += 1;
        let start = i;
        while i < chars.len() && (chars[i].is_ascii_alphabetic() || chars[i] == '@') {
            i += 1;
        }
        if i == start {
            i += 1; // `\\`, `\%`, `\{`: a symbol, not a command
            continue;
        }
        let name: String = chars[start..i].iter().collect();
        let star = chars.get(i) == Some(&'*');
        if star {
            i += 1;
        }
        let mut arg = None;
        loop {
            let mut j = i;
            while chars.get(j).is_some_and(|c| *c == ' ' || *c == '\t') {
                j += 1;
            }
            match chars.get(j) {
                Some('[') => match balanced(&chars, j, '[', ']') {
                    Some((_, end)) => i = end,
                    None => break,
                },
                Some('{') => {
                    if let Some((text, end)) = balanced(&chars, j, '{', '}') {
                        arg = Some(text);
                        i = end;
                    }
                    break;
                }
                _ => break,
            }
        }
        out.push(Cmd { name, star, arg });
    }
    out
}

/// The text between the bracket at `open` and its match, and the index after the match.
fn balanced(chars: &[char], open: usize, l: char, r: char) -> Option<(String, usize)> {
    let mut depth = 0usize;
    let mut i = open;
    while i < chars.len() {
        let c = chars[i];
        if c == '\\' {
            i += 2;
            continue;
        }
        if c == l {
            depth += 1;
        } else if c == r {
            depth -= 1;
            if depth == 0 {
                return Some((chars[open + 1..i].iter().collect(), i + 1));
            }
        }
        i += 1;
    }
    None
}

// ----- Bibliography -----

/// Every entry of every `.bib` file in the Project.
pub fn bib_entries(project_dir: &Path) -> Result<Vec<BibEntry>> {
    let mut out = Vec::new();
    for entry in fsutil::list_files(project_dir)? {
        if entry.is_dir || !entry.path.to_ascii_lowercase().ends_with(".bib") {
            continue;
        }
        if let Ok(text) = fsutil::read_text(project_dir, &entry.path) {
            out.extend(parse_bib(&text, &entry.path));
        }
    }
    Ok(out)
}

pub fn parse_bib(text: &str, file: &str) -> Vec<BibEntry> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    let mut line = 1u32;
    let mut counted = 0;
    while i < chars.len() {
        if chars[i] != '@' {
            i += 1;
            continue;
        }
        line += chars[counted..i].iter().filter(|c| **c == '\n').count() as u32;
        counted = i;
        let start = i + 1;
        let mut j = start;
        while j < chars.len() && chars[j].is_ascii_alphabetic() {
            j += 1;
        }
        let kind: String = chars[start..j].iter().collect::<String>().to_ascii_lowercase();
        while chars.get(j).is_some_and(|c| c.is_whitespace()) {
            j += 1;
        }
        let (open, close) = match chars.get(j) {
            Some('{') => ('{', '}'),
            Some('(') => ('(', ')'),
            _ => {
                i = j.max(i + 1);
                continue;
            }
        };
        let Some((body, end)) = balanced(&chars, j, open, close) else { break };
        i = end;
        if matches!(kind.as_str(), "comment" | "string" | "preamble") || kind.is_empty() {
            continue;
        }
        let Some((key, fields)) = body.split_once(',') else { continue };
        let fields = parse_fields(fields);
        out.push(BibEntry {
            key: key.trim().to_string(),
            kind,
            title: fields.get("title").cloned(),
            author: fields.get("author").cloned(),
            year: fields.get("year").cloned(),
            file: file.to_string(),
            line,
        });
    }
    out
}

fn parse_fields(body: &str) -> BTreeMap<String, String> {
    let chars: Vec<char> = body.chars().collect();
    let mut out = BTreeMap::new();
    let mut i = 0;
    while i < chars.len() {
        while chars.get(i).is_some_and(|c| c.is_whitespace() || *c == ',') {
            i += 1;
        }
        let start = i;
        while chars.get(i).is_some_and(|c| *c != '=' && *c != ',') {
            i += 1;
        }
        if chars.get(i) != Some(&'=') {
            break;
        }
        let name: String = chars[start..i].iter().collect::<String>().trim().to_ascii_lowercase();
        i += 1;
        while chars.get(i).is_some_and(|c| c.is_whitespace()) {
            i += 1;
        }
        let value = match chars.get(i) {
            Some('{') => balanced(&chars, i, '{', '}').map(|(v, end)| {
                i = end;
                v
            }),
            Some('"') => {
                let from = i + 1;
                let mut j = from;
                while j < chars.len() && chars[j] != '"' {
                    j += if chars[j] == '\\' { 2 } else { 1 };
                }
                let v: String = chars[from..j.min(chars.len())].iter().collect();
                i = (j + 1).min(chars.len());
                Some(v)
            }
            _ => {
                let from = i;
                while chars.get(i).is_some_and(|c| *c != ',') {
                    i += 1;
                }
                Some(chars[from..i].iter().collect::<String>())
            }
        };
        if let Some(v) = value {
            out.insert(name, clean_title(&v));
        }
    }
    out
}

// ----- Cross-reference check -----

#[derive(Debug, Serialize)]
pub struct References {
    pub labels: Vec<Mention>,
    pub bib: Vec<BibEntry>,
    /// `\ref{x}` with no `\label{x}`: prints as `??`.
    pub undefined_refs: Vec<Mention>,
    /// `\cite{x}` with no entry `x` in any `.bib` file: prints as `[?]`.
    pub undefined_cites: Vec<Mention>,
    /// Labels nothing refers to. Harmless, but often a sign of a forgotten reference.
    pub unused_labels: Vec<Mention>,
    /// Bibliography entries nothing cites (empty when `\nocite{*}` is used).
    pub unused_bib: Vec<BibEntry>,
    pub duplicate_labels: Vec<Mention>,
    pub duplicate_bib_keys: Vec<BibEntry>,
    pub missing_inputs: Vec<Mention>,
}

pub fn references(project_dir: &Path, root_file: &str) -> Result<References> {
    let scan = scan(project_dir, root_file)?;
    let bib = bib_entries(project_dir)?;

    let defined: BTreeSet<&str> = scan.labels.iter().map(|m| m.name.as_str()).collect();
    let used_refs: BTreeSet<&str> = scan.refs.iter().map(|m| m.name.as_str()).collect();
    let keys: BTreeSet<&str> = bib.iter().map(|b| b.key.as_str()).collect();
    let used_cites: BTreeSet<&str> = scan.cites.iter().map(|m| m.name.as_str()).collect();

    let undefined_refs = scan.refs.iter().filter(|m| !defined.contains(m.name.as_str())).cloned().collect();
    let undefined_cites = if bib.is_empty() && scan.cites.is_empty() {
        Vec::new()
    } else {
        scan.cites.iter().filter(|m| !keys.contains(m.name.as_str())).cloned().collect()
    };
    let unused_labels = scan.labels.iter().filter(|m| !used_refs.contains(m.name.as_str())).cloned().collect();
    let unused_bib = if scan.nocite_all {
        Vec::new()
    } else {
        bib.iter().filter(|b| !used_cites.contains(b.key.as_str())).cloned().collect()
    };

    let mut seen = HashSet::new();
    let duplicate_labels = scan.labels.iter().filter(|m| !seen.insert(m.name.as_str())).cloned().collect();
    let mut seen = HashSet::new();
    let duplicate_bib_keys = bib.iter().filter(|b| !seen.insert(b.key.as_str())).cloned().collect();

    Ok(References {
        labels: scan.labels,
        bib,
        undefined_refs,
        undefined_cites,
        unused_labels,
        unused_bib,
        duplicate_labels,
        duplicate_bib_keys,
        missing_inputs: scan.missing_inputs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fsutil::write_text;

    fn project(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for (path, text) in files {
            write_text(dir.path(), path, text).unwrap();
        }
        dir
    }

    const MAIN: &str = "\\documentclass{report}\n\\begin{document}\n\
\\chapter{Intro}\\label{ch:intro}\n\
% \\section{Commented out}\n\
\\input{chapters/method}\n\
\\include{chapters/results}\n\
\\input{chapters/missing}\n\
\\section*{Acknowledgements}\n\
\\end{document}\n";

    #[test]
    fn the_outline_follows_includes_in_document_order() {
        let dir = project(&[
            ("main.tex", MAIN),
            ("chapters/method.tex", "\\section{Method \\textbf{Two}}\\label{sec:m}\n\\subsection{Data}\n"),
            ("chapters/results.tex", "\\chapter{Results}\nSee Section~\\ref{sec:m}.\n"),
        ]);
        let scan = scan(dir.path(), "main.tex").unwrap();
        assert_eq!(scan.files, ["main.tex", "chapters/method.tex", "chapters/results.tex"]);
        let outline: Vec<_> = scan.headings.iter().map(|h| (h.level, h.title.as_str(), h.file.as_str(), h.line)).collect();
        assert_eq!(
            outline,
            [
                (1, "Intro", "main.tex", 3),
                (2, "Method Two", "chapters/method.tex", 1),
                (3, "Data", "chapters/method.tex", 2),
                (1, "Results", "chapters/results.tex", 1),
                (2, "Acknowledgements", "main.tex", 8),
            ],
            "the commented-out section is not listed"
        );
        assert!(scan.headings[4].starred);
        assert_eq!(scan.missing_inputs.len(), 1);
        assert_eq!(scan.missing_inputs[0].name, "chapters/missing.tex");
    }

    #[test]
    fn include_cycles_and_unreadable_files_do_not_hang_or_fail() {
        let dir = project(&[
            ("main.tex", "\\input{a}\n"),
            ("a.tex", "\\section{A}\n\\input{main}\n"),
        ]);
        let found = scan(dir.path(), "main.tex").unwrap();
        assert_eq!(found.headings.len(), 1);
        assert!(scan(dir.path(), "../x.tex").is_err(), "paths cannot leave the project");
    }

    #[test]
    fn commands_read_options_stars_and_nested_braces() {
        let cmds = commands(r"\section*[Short]{A {nested} title} text \cite[p.~3]{knuth,lamport} \\ \%");
        let by: Vec<_> = cmds.iter().map(|c| (c.name.as_str(), c.star, c.arg.as_deref())).collect();
        assert_eq!(
            by,
            [("section", true, Some("A {nested} title")), ("cite", false, Some("knuth,lamport"))]
        );
        assert_eq!(strip_comment(r"100\% sure % a comment"), r"100\% sure ");
    }

    const BIB: &str = "@article{knuth84,\n  title = {The {TeXbook}},\n  author = \"Donald Knuth\",\n  year = 1984\n}\n\
@comment{ignored}\n@string{foo = \"bar\"}\n\
@book{lamport94, title={LaTeX: A Document\n Preparation System}, year={1994}}\n\
@article{unused99, title={Never cited}}\n";

    #[test]
    fn bib_entries_give_key_title_author_year_and_line() {
        let entries = parse_bib(BIB, "refs.bib");
        let keys: Vec<_> = entries.iter().map(|e| e.key.as_str()).collect();
        assert_eq!(keys, ["knuth84", "lamport94", "unused99"]);
        assert_eq!(entries[0].title.as_deref(), Some("The TeXbook"));
        assert_eq!(entries[0].author.as_deref(), Some("Donald Knuth"));
        assert_eq!(entries[0].year.as_deref(), Some("1984"));
        assert_eq!(entries[1].title.as_deref(), Some("LaTeX: A Document Preparation System"));
        assert_eq!((entries[0].line, entries[1].line, entries[2].line), (1, 8, 10));
        assert_eq!(entries[1].kind, "book");
    }

    #[test]
    fn references_find_the_broken_ones() {
        let dir = project(&[
            (
                "main.tex",
                "\\section{A}\\label{sec:a}\\label{sec:a}\nSee \\ref{sec:a}, \\ref{sec:nope} and \\cite{knuth84,ghost}.\n\\label{orphan}\n",
            ),
            ("refs.bib", BIB),
        ]);
        let r = references(dir.path(), "main.tex").unwrap();
        assert_eq!(r.undefined_refs.iter().map(|m| m.name.as_str()).collect::<Vec<_>>(), ["sec:nope"]);
        assert_eq!(r.undefined_cites.iter().map(|m| m.name.as_str()).collect::<Vec<_>>(), ["ghost"]);
        assert_eq!(r.unused_labels.iter().map(|m| m.name.as_str()).collect::<Vec<_>>(), ["orphan"]);
        assert_eq!(r.unused_bib.iter().map(|b| b.key.as_str()).collect::<Vec<_>>(), ["lamport94", "unused99"]);
        assert_eq!(r.duplicate_labels.len(), 1);
        assert_eq!(r.duplicate_labels[0].name, "sec:a");
    }

    #[test]
    fn nocite_star_means_nothing_is_unused_and_no_bib_means_no_cite_complaints() {
        let dir = project(&[("main.tex", "\\nocite{*}\n"), ("refs.bib", BIB)]);
        assert!(references(dir.path(), "main.tex").unwrap().unused_bib.is_empty());
        let plain = project(&[("main.tex", "\\section{A}\n")]);
        let r = references(plain.path(), "main.tex").unwrap();
        assert!(r.undefined_cites.is_empty() && r.unused_bib.is_empty());
    }
}
