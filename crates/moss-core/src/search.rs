//! Search and replace across every text file of a Project.
//!
//! Matching is line by line (a match never spans a line break), so a count shown in a search is
//! exactly what a replace then changes. Plain-text queries are escaped into a regular expression,
//! so the same engine serves every option. Files are scanned on several threads.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use regex::{NoExpand, Regex, RegexBuilder};
use serde::{Deserialize, Serialize};

use crate::fsutil::{self, list_files};
use crate::history::History;
use crate::ignore::is_text_path;
use crate::{Error, Result};

/// Files bigger than this are skipped (generated data, not something to search by hand).
const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024;
/// Line snippets kept per file; the count still covers every match.
const MAX_LINES_PER_FILE: usize = 100;
/// Files listed in one report.
const MAX_FILES: usize = 500;
/// Longest snippet sent to the UI, in characters.
const SNIPPET_CHARS: usize = 240;
/// Characters of context kept before the first match when a long line is cut.
const SNIPPET_LEAD: usize = 60;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SearchOptions {
    pub query: String,
    #[serde(default)]
    pub case_sensitive: bool,
    #[serde(default)]
    pub whole_word: bool,
    /// Treat `query` as a regular expression (Rust syntax; `$1` works in the replacement).
    #[serde(default)]
    pub regex: bool,
    /// Globs separated by commas or spaces, e.g. `*.tex, chapters/**, !old`. A leading `!`
    /// excludes. A pattern without `/` matches a file or folder name at any depth. Empty means
    /// every text file.
    #[serde(default)]
    pub include: String,
}

/// One matching line. `ranges` are character offsets of the matches inside `text`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LineHit {
    pub line: usize,
    pub text: String,
    pub ranges: Vec<(usize, usize)>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FileHits {
    pub path: String,
    /// Every match in the file, not only those on the listed lines.
    pub count: usize,
    pub lines: Vec<LineHit>,
    pub lines_truncated: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct SearchReport {
    pub files: Vec<FileHits>,
    pub total: usize,
    pub files_searched: usize,
    /// More files matched than are listed.
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct FileReplaced {
    pub path: String,
    pub replacements: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReplaceReport {
    pub files: Vec<FileReplaced>,
    pub replacements: usize,
    /// A restore point was taken before the files were written.
    pub restore_point: bool,
}

/// Build the matcher for the options. A pattern that can match nothing at all (`a*`) is refused,
/// because replacing it would insert text between every character.
fn matcher(opts: &SearchOptions) -> Result<Regex> {
    if opts.query.is_empty() {
        return Err(Error::Entry("type something to search for".into()));
    }
    let mut pattern = if opts.regex { opts.query.clone() } else { regex::escape(&opts.query) };
    if opts.whole_word {
        pattern = format!("(?:{pattern})");
        if edge_is_word(&opts.query, true) || opts.regex {
            pattern = format!(r"\b{pattern}");
        }
        if edge_is_word(&opts.query, false) || opts.regex {
            pattern = format!(r"{pattern}\b");
        }
    }
    let re = RegexBuilder::new(&pattern)
        .case_insensitive(!opts.case_sensitive)
        .build()
        .map_err(|e| Error::Entry(format!("invalid search pattern: {e}")))?;
    if re.is_match("") {
        return Err(Error::Entry("that pattern matches empty text; make it match something".into()));
    }
    Ok(re)
}

/// Is the first (or last) character of a plain query a word character? `\b` next to anything
/// else would demand the opposite and never match (`\bfoo(\b` fails on `foo(`).
fn edge_is_word(query: &str, first: bool) -> bool {
    let c = if first { query.chars().next() } else { query.chars().next_back() };
    c.is_some_and(|c| c.is_alphanumeric() || c == '_')
}

/// `include` patterns turned into regular expressions over the project-relative path.
struct Globs {
    allow: Vec<Regex>,
    deny: Vec<Regex>,
}

impl Globs {
    fn parse(spec: &str) -> Result<Globs> {
        let mut globs = Globs { allow: Vec::new(), deny: Vec::new() };
        for raw in spec.split(|c: char| c == ',' || c.is_whitespace()) {
            let (deny, pattern) = match raw.strip_prefix('!') {
                Some(rest) => (true, rest),
                None => (false, raw),
            };
            let pattern = pattern.trim_start_matches("./").trim_start_matches('/');
            let pattern = pattern.trim_end_matches('/');
            if pattern.is_empty() {
                continue;
            }
            let re = glob_regex(pattern)?;
            if deny { globs.deny.push(re) } else { globs.allow.push(re) }
        }
        Ok(globs)
    }

    fn accepts(&self, path: &str) -> bool {
        (self.allow.is_empty() || self.allow.iter().any(|g| g.is_match(path)))
            && !self.deny.iter().any(|g| g.is_match(path))
    }
}

fn glob_regex(pattern: &str) -> Result<Regex> {
    let mut body = String::new();
    let mut chars = pattern.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '*' if chars.peek() == Some(&'*') => {
                chars.next();
                body.push_str(".*");
            }
            '*' => body.push_str("[^/]*"),
            '?' => body.push_str("[^/]"),
            c => body.push_str(&regex::escape(&c.to_string())),
        }
    }
    // A folder pattern also covers everything inside it.
    let anywhere = if pattern.contains('/') { "^" } else { "(?:^|.*/)" };
    RegexBuilder::new(&format!("{anywhere}{body}(?:/.*)?$"))
        .case_insensitive(true)
        .build()
        .map_err(|e| Error::Entry(format!("invalid file filter `{pattern}`: {e}")))
}

/// Text files of the Project that the options' filter lets through, in listing order.
fn candidate_files(project_dir: &Path, include: &str) -> Result<Vec<String>> {
    let globs = Globs::parse(include)?;
    Ok(list_files(project_dir)?
        .into_iter()
        .filter(|e| !e.is_dir && e.size <= MAX_FILE_BYTES && is_text_path(&e.path))
        .filter(|e| globs.accepts(&e.path))
        .map(|e| e.path)
        .collect())
}

/// Run `work` on every path on several threads and return the `Some` results in path order.
fn scan<T: Send>(paths: &[String], work: impl Fn(&str) -> Option<T> + Sync) -> Vec<T> {
    let next = AtomicUsize::new(0);
    let found = Mutex::new(Vec::new());
    let threads = std::thread::available_parallelism().map_or(2, |n| n.get()).min(8).min(paths.len().max(1));
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(path) = paths.get(i) else { break };
                    if let Some(result) = work(path) {
                        found.lock().unwrap().push((i, result));
                    }
                }
            });
        }
    });
    let mut found = found.into_inner().unwrap();
    found.sort_by_key(|(i, _)| *i);
    found.into_iter().map(|(_, r)| r).collect()
}

pub fn search(project_dir: &Path, opts: &SearchOptions) -> Result<SearchReport> {
    let re = matcher(opts)?;
    let paths = candidate_files(project_dir, &opts.include)?;
    let mut files: Vec<FileHits> = scan(&paths, |path| {
        let text = fsutil::read_text(project_dir, path).ok()?;
        search_text(&re, path, &text)
    });
    let total = files.iter().map(|f| f.count).sum();
    let truncated = files.len() > MAX_FILES;
    files.truncate(MAX_FILES);
    Ok(SearchReport { files, total, files_searched: paths.len(), truncated })
}

fn search_text(re: &Regex, path: &str, text: &str) -> Option<FileHits> {
    let mut count = 0;
    let mut lines = Vec::new();
    let mut lines_truncated = false;
    for (i, line) in text.lines().enumerate() {
        let found: Vec<(usize, usize)> = re.find_iter(line).map(|m| (m.start(), m.end())).collect();
        if found.is_empty() {
            continue;
        }
        count += found.len();
        if lines.len() < MAX_LINES_PER_FILE {
            lines.push(snippet(i + 1, line, &found));
        } else {
            lines_truncated = true;
        }
    }
    (count > 0).then(|| FileHits { path: path.to_string(), count, lines, lines_truncated })
}

/// The line as shown in the list: indentation dropped, a very long line cut around its first
/// match. `found` holds byte offsets into `line`; the result holds character offsets.
fn snippet(number: usize, line: &str, found: &[(usize, usize)]) -> LineHit {
    let chars: Vec<char> = line.chars().collect();
    let to_char = |byte: usize| line[..byte].chars().count();
    let indent = chars.iter().take_while(|c| c.is_whitespace()).count();
    let first = to_char(found[0].0);
    let mut from = indent.min(first);
    if first.saturating_sub(from) > SNIPPET_LEAD {
        from = first - SNIPPET_LEAD;
    }
    let to = (from + SNIPPET_CHARS).min(chars.len());
    let text: String = chars[from..to].iter().collect::<String>().trim_end().to_string();
    let shown = text.chars().count();
    let ranges = found
        .iter()
        .map(|&(s, e)| (to_char(s), to_char(e)))
        .filter(|&(s, e)| s >= from && e <= from + shown)
        .map(|(s, e)| (s - from, e - from))
        .collect();
    LineHit { line: number, text, ranges }
}

/// Replace every match in the text, line by line. Returns the new text and how many matches
/// were replaced.
fn replace_text(re: &Regex, text: &str, replacement: &str, regex_mode: bool) -> (String, usize) {
    let mut out = String::with_capacity(text.len());
    let mut count = 0;
    for piece in text.split_inclusive('\n') {
        let (content, newline) = match piece.strip_suffix('\n') {
            Some(content) => (content, "\n"),
            None => (piece, ""),
        };
        let n = re.find_iter(content).count();
        if n == 0 {
            out.push_str(piece);
            continue;
        }
        count += n;
        let replaced = if regex_mode {
            re.replace_all(content, replacement)
        } else {
            re.replace_all(content, NoExpand(replacement))
        };
        out.push_str(&replaced);
        out.push_str(newline);
    }
    (out, count)
}

/// Replace matches in the Project's files, or only in `only` when given. A restore point is
/// taken first (as one step), so Undo and History bring everything back. Nothing is written
/// when there is nothing to replace.
pub fn replace(
    project_dir: &Path,
    opts: &SearchOptions,
    replacement: &str,
    only: Option<&[String]>,
) -> Result<ReplaceReport> {
    let re = matcher(opts)?;
    let mut paths = candidate_files(project_dir, &opts.include)?;
    if let Some(only) = only {
        paths.retain(|p| only.contains(p));
    }
    let planned: Vec<(String, String, usize)> = scan(&paths, |path| {
        let text = fsutil::read_text(project_dir, path).ok()?;
        let (new, n) = replace_text(&re, &text, replacement, opts.regex);
        (n > 0 && new != text).then(|| (path.to_string(), new, n))
    });
    if planned.is_empty() {
        return Ok(ReplaceReport { files: Vec::new(), replacements: 0, restore_point: false });
    }
    History::new(project_dir).capture(&restore_label(&opts.query, replacement))?;
    let mut files = Vec::new();
    for (path, new, n) in &planned {
        fsutil::write_text(project_dir, path, new)?;
        files.push(FileReplaced { path: path.clone(), replacements: *n });
    }
    let replacements = files.iter().map(|f| f.replacements).sum();
    Ok(ReplaceReport { files, replacements, restore_point: true })
}

fn restore_label(query: &str, replacement: &str) -> String {
    let short = |s: &str| {
        let one_line = s.lines().next().unwrap_or("");
        if one_line.chars().count() > 30 {
            format!("{}…", one_line.chars().take(30).collect::<String>())
        } else {
            one_line.to_string()
        }
    };
    format!("Before replacing \"{}\" with \"{}\"", short(query), short(replacement))
}

/// For tests and callers that want a quick per-file count map.
pub fn counts(report: &SearchReport) -> BTreeMap<&str, usize> {
    report.files.iter().map(|f| (f.path.as_str(), f.count)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        let d = tmp.path();
        fsutil::write_text(d, "main.tex", "Hello world\nhello again, Hello!\n  indented hello\n").unwrap();
        fsutil::write_text(d, "chapters/one.tex", "The word helloworld and a hello.\n").unwrap();
        fsutil::write_text(d, "refs.bib", "@book{hello2020, title={Hello}}\n").unwrap();
        fsutil::write_text(d, "notes.txt", "nothing here\n").unwrap();
        std::fs::write(d.join("figure.png"), b"hello\0hello").unwrap();
        tmp
    }

    fn opts(query: &str) -> SearchOptions {
        SearchOptions { query: query.into(), ..Default::default() }
    }

    #[test]
    fn plain_search_is_case_insensitive_by_default_and_counts_every_match() {
        let tmp = project();
        let report = search(tmp.path(), &opts("hello")).unwrap();
        let map = counts(&report);
        assert_eq!(map["main.tex"], 4);
        assert_eq!(map["chapters/one.tex"], 2);
        assert_eq!(map["refs.bib"], 2);
        assert!(!map.contains_key("notes.txt"));
        assert!(!map.contains_key("figure.png"), "binary files are skipped");
        assert_eq!(report.total, 8);
        let main = report.files.iter().find(|f| f.path == "main.tex").unwrap();
        assert_eq!(main.lines.iter().map(|l| l.line).collect::<Vec<_>>(), vec![1, 2, 3]);
        assert_eq!(main.lines[2].text, "indented hello", "indentation is dropped");
        assert_eq!(main.lines[2].ranges, vec![(9, 14)]);
    }

    #[test]
    fn case_sensitive_whole_word_and_regex() {
        let tmp = project();
        let mut o = opts("hello");
        o.case_sensitive = true;
        assert_eq!(search(tmp.path(), &o).unwrap().total, 5);
        o.whole_word = true;
        let report = search(tmp.path(), &o).unwrap();
        assert_eq!(counts(&report)["chapters/one.tex"], 1, "helloworld is not the word hello");
        let mut r = opts(r"hel+o\w*");
        r.regex = true;
        assert!(search(tmp.path(), &r).unwrap().total >= 8);
        r.query = "(".into();
        assert!(search(tmp.path(), &r).is_err(), "invalid regex is an error, not a panic");
        r.query = "a*".into();
        assert!(search(tmp.path(), &r).is_err(), "a pattern matching empty text is refused");
    }

    #[test]
    fn whole_word_with_a_symbol_at_the_edge_still_matches() {
        let tmp = tempfile::tempdir().unwrap();
        fsutil::write_text(tmp.path(), "a.tex", "call foo( now\n").unwrap();
        let mut o = opts("foo(");
        o.whole_word = true;
        assert_eq!(search(tmp.path(), &o).unwrap().total, 1);
    }

    #[test]
    fn include_globs_filter_files_and_folders() {
        let tmp = project();
        let mut o = opts("hello");
        o.include = "*.bib".into();
        assert_eq!(counts(&search(tmp.path(), &o).unwrap()).keys().copied().collect::<Vec<_>>(), vec!["refs.bib"]);
        o.include = "chapters".into();
        assert_eq!(counts(&search(tmp.path(), &o).unwrap()).keys().copied().collect::<Vec<_>>(), vec!["chapters/one.tex"]);
        o.include = "*.tex, !chapters/**".into();
        assert_eq!(counts(&search(tmp.path(), &o).unwrap()).keys().copied().collect::<Vec<_>>(), vec!["main.tex"]);
    }

    #[test]
    fn moss_folder_and_generated_files_are_not_searched() {
        let tmp = project();
        std::fs::create_dir_all(tmp.path().join(".moss")).unwrap();
        std::fs::write(tmp.path().join(".moss/note.txt"), "hello").unwrap();
        std::fs::write(tmp.path().join("main.aux"), "hello").unwrap();
        let report = search(tmp.path(), &opts("hello")).unwrap();
        assert!(report.files.iter().all(|f| !f.path.starts_with(".moss") && f.path != "main.aux"));
    }

    #[test]
    fn long_lines_are_cut_around_the_match() {
        let line = format!("{}needle{}", "a".repeat(500), "b".repeat(500));
        let tmp = tempfile::tempdir().unwrap();
        fsutil::write_text(tmp.path(), "a.tex", &line).unwrap();
        let report = search(tmp.path(), &opts("needle")).unwrap();
        let hit = &report.files[0].lines[0];
        assert!(hit.text.chars().count() <= SNIPPET_CHARS);
        let (s, e) = hit.ranges[0];
        assert_eq!(hit.text.chars().skip(s).take(e - s).collect::<String>(), "needle");
    }

    #[test]
    fn replace_everywhere_takes_one_restore_point_and_can_be_undone() {
        let tmp = project();
        let before = fsutil::read_text(tmp.path(), "main.tex").unwrap();
        let report = replace(tmp.path(), &opts("hello"), "Goodbye", None).unwrap();
        assert_eq!(report.replacements, 8);
        assert!(report.restore_point);
        assert_eq!(
            fsutil::read_text(tmp.path(), "main.tex").unwrap(),
            "Goodbye world\nGoodbye again, Goodbye!\n  indented Goodbye\n"
        );
        assert_eq!(search(tmp.path(), &opts("hello")).unwrap().total, 0);
        let history = History::new(tmp.path());
        let undone = history.undo().unwrap();
        assert!(undone.is_some());
        assert_eq!(fsutil::read_text(tmp.path(), "main.tex").unwrap(), before);
    }

    #[test]
    fn replace_in_one_file_only() {
        let tmp = project();
        let only = vec!["refs.bib".to_string()];
        let report = replace(tmp.path(), &opts("hello"), "bye", Some(&only)).unwrap();
        assert_eq!(report.files.len(), 1);
        assert_eq!(report.replacements, 2);
        assert_eq!(search(tmp.path(), &opts("hello")).unwrap().total, 6);
    }

    #[test]
    fn regex_replacement_expands_groups_but_plain_text_does_not() {
        let tmp = tempfile::tempdir().unwrap();
        fsutil::write_text(tmp.path(), "a.tex", "fig 3 and fig 12\ncost $5\n").unwrap();
        let mut o = opts(r"fig (\d+)");
        o.regex = true;
        replace(tmp.path(), &o, "Figure $1", None).unwrap();
        assert_eq!(fsutil::read_text(tmp.path(), "a.tex").unwrap(), "Figure 3 and Figure 12\ncost $5\n");
        replace(tmp.path(), &opts("$5"), "$$6 $1", None).unwrap();
        assert_eq!(fsutil::read_text(tmp.path(), "a.tex").unwrap(), "Figure 3 and Figure 12\ncost $$6 $1\n");
    }

    #[test]
    fn nothing_to_replace_writes_nothing_and_takes_no_restore_point() {
        let tmp = project();
        let report = replace(tmp.path(), &opts("zzz"), "y", None).unwrap();
        assert_eq!(report.replacements, 0);
        assert!(!report.restore_point);
        assert!(!tmp.path().join(".moss/history").exists());
    }

    #[test]
    fn replacing_with_the_same_text_changes_nothing() {
        let tmp = project();
        let report = replace(tmp.path(), &opts("world"), "world", None).unwrap();
        assert_eq!(report.replacements, 0);
    }
}
