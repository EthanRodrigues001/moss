//! Names that Windows cannot hold, and case collisions.
//!
//! Overleaf allows names Windows forbids (`aux.tex`, `a:b.tex`) and two names that differ only
//! by case. Sync and Import use these checks to put such files on the Needs attention list
//! instead of failing or overwriting anything.

use std::collections::HashMap;
use std::fmt;

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "detail", rename_all = "snake_case")]
pub enum NameProblem {
    Empty,
    InvalidChar(char),
    Reserved(String),
    TrailingDotOrSpace,
    TooLong(usize),
}

impl fmt::Display for NameProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NameProblem::Empty => write!(f, "name is empty"),
            NameProblem::InvalidChar(c) => write!(f, "contains `{c}`, which Windows does not allow"),
            NameProblem::Reserved(n) => write!(f, "`{n}` is a reserved Windows device name"),
            NameProblem::TrailingDotOrSpace => write!(f, "ends with a dot or space"),
            NameProblem::TooLong(n) => write!(f, "is {n} characters long (limit {MAX_COMPONENT})"),
        }
    }
}

/// Longest single path component we accept. Windows allows 255 UTF-16 units; we keep a margin.
pub const MAX_COMPONENT: usize = 200;

const RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

fn is_invalid_char(c: char) -> bool {
    matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') || c.is_control()
}

fn is_reserved(component: &str) -> bool {
    // `aux.tex` is reserved too: Windows looks only at the part before the first dot.
    let stem = component.split('.').next().unwrap_or(component);
    let stem = stem.trim_end();
    RESERVED.iter().any(|r| r.eq_ignore_ascii_case(stem))
}

/// Check one path component (a single file or folder name).
pub fn check_component(name: &str) -> Option<NameProblem> {
    if name.is_empty() {
        return Some(NameProblem::Empty);
    }
    if let Some(c) = name.chars().find(|&c| is_invalid_char(c)) {
        return Some(NameProblem::InvalidChar(c));
    }
    if is_reserved(name) {
        return Some(NameProblem::Reserved(name.to_string()));
    }
    if name.ends_with('.') || name.ends_with(' ') {
        return Some(NameProblem::TrailingDotOrSpace);
    }
    let len = name.chars().count();
    if len > MAX_COMPONENT {
        return Some(NameProblem::TooLong(len));
    }
    None
}

/// Check a `/`-separated relative path. Returns the first bad component and why.
pub fn check_path(rel: &str) -> Option<(String, NameProblem)> {
    rel.split('/')
        .find_map(|c| check_component(c).map(|p| (c.to_string(), p)))
}

/// A safe replacement for a component, used for suggestions and for naming new Project folders.
pub fn sanitize_component(name: &str) -> String {
    let mut out: String = name
        .chars()
        .map(|c| if is_invalid_char(c) { '_' } else { c })
        .collect();
    out = out.trim_end_matches(['.', ' ']).to_string();
    if out.is_empty() {
        out = "_".to_string();
    }
    if is_reserved(&out) {
        out.insert(0, '_');
    }
    out.chars().take(MAX_COMPONENT).collect()
}

/// Groups of paths that differ only by case and so cannot coexist on Windows.
pub fn case_collisions<'a>(paths: impl IntoIterator<Item = &'a str>) -> Vec<Vec<String>> {
    let mut by_lower: HashMap<String, Vec<String>> = HashMap::new();
    for p in paths {
        by_lower.entry(p.to_lowercase()).or_default().push(p.to_string());
    }
    let mut groups: Vec<Vec<String>> = by_lower.into_values().filter(|g| g.len() > 1).collect();
    for g in &mut groups {
        g.sort();
    }
    groups.sort();
    groups
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn good_names_pass() {
        for n in ["main.tex", "Chapter 1.tex", "fig-2.png", "ünï.tex", "a.b.c"] {
            assert_eq!(check_component(n), None, "{n}");
        }
    }

    #[test]
    fn reserved_names_are_caught_even_with_extension() {
        assert!(matches!(check_component("aux.tex"), Some(NameProblem::Reserved(_))));
        assert!(matches!(check_component("CON"), Some(NameProblem::Reserved(_))));
        assert!(matches!(check_component("com3.bib"), Some(NameProblem::Reserved(_))));
        assert_eq!(check_component("auxiliary.tex"), None);
    }

    #[test]
    fn invalid_chars_and_trailing_dots() {
        assert_eq!(check_component("a:b.tex"), Some(NameProblem::InvalidChar(':')));
        assert_eq!(check_component("what?.tex"), Some(NameProblem::InvalidChar('?')));
        assert_eq!(check_component("name."), Some(NameProblem::TrailingDotOrSpace));
        assert_eq!(check_component(""), Some(NameProblem::Empty));
    }

    #[test]
    fn path_check_reports_the_bad_component() {
        let (c, p) = check_path("chapters/aux/intro.tex").unwrap();
        assert_eq!(c, "aux");
        assert!(matches!(p, NameProblem::Reserved(_)));
        assert_eq!(check_path("chapters/intro.tex"), None);
    }

    #[test]
    fn sanitize_makes_names_safe() {
        for n in ["a:b?", "aux", "x...", "", "CON.txt"] {
            let s = sanitize_component(n);
            assert_eq!(check_component(&s), None, "{n} -> {s}");
        }
        assert_eq!(sanitize_component("a:b"), "a_b");
        assert_eq!(sanitize_component("aux"), "_aux");
    }

    #[test]
    fn finds_case_collisions() {
        let groups = case_collisions(["Fig.png", "fig.png", "other.png"]);
        assert_eq!(groups, vec![vec!["Fig.png".to_string(), "fig.png".to_string()]]);
        assert!(case_collisions(["a.tex", "b.tex"]).is_empty());
    }
}
