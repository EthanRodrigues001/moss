//! A quick sanity check of LaTeX source after a write: do braces, `\begin`/`\end` pairs and
//! `$` signs still balance? It is a heuristic (verbatim text and unusual macros can fool it), so
//! it only reports what a write *changed*, never a problem the file already had.

use std::collections::BTreeMap;

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Balance {
    /// Opening minus closing braces, outside comments and not counting `\{` or `\}`.
    pub braces: i64,
    /// Single `$` signs (`$$` counts as two), outside comments and not counting `\$`.
    pub dollars: usize,
    /// `\begin{x}` minus `\end{x}` per environment name.
    pub environments: BTreeMap<String, i64>,
}

pub fn balance(text: &str) -> Balance {
    let mut b = Balance::default();
    for line in text.lines() {
        let code = strip_comment(line);
        let mut chars = code.char_indices().peekable();
        while let Some((i, c)) = chars.next() {
            match c {
                '\\' => {
                    // `\{`, `\$` and friends are text, not structure. A control word is read whole.
                    if let Some((_, next)) = chars.peek().copied() {
                        if next.is_ascii_alphabetic() {
                            let word: String = code[i + 1..]
                                .chars()
                                .take_while(char::is_ascii_alphabetic)
                                .collect();
                            for _ in 0..word.len() {
                                chars.next();
                            }
                            if word == "begin" || word == "end" {
                                let rest = &code[i + 1 + word.len()..];
                                if let Some(name) = rest.strip_prefix('{').and_then(|r| r.split('}').next()) {
                                    let delta = if word == "begin" { 1 } else { -1 };
                                    *b.environments.entry(name.to_string()).or_insert(0) += delta;
                                }
                            }
                        } else {
                            chars.next();
                        }
                    }
                }
                '{' => b.braces += 1,
                '}' => b.braces -= 1,
                '$' => b.dollars += 1,
                _ => {}
            }
        }
    }
    b.environments.retain(|_, n| *n != 0);
    b
}

/// The line up to an unescaped `%`.
fn strip_comment(line: &str) -> &str {
    let mut escaped = false;
    for (i, c) in line.char_indices() {
        match c {
            '\\' if !escaped => escaped = true,
            '%' if !escaped => return &line[..i],
            _ => escaped = false,
        }
    }
    line
}

/// What a change from `old` to `new` made worse, in plain words. Empty when nothing did.
pub fn new_problems(old: &str, new: &str) -> Vec<String> {
    let (before, after) = (balance(old), balance(new));
    let mut out = Vec::new();
    if after.braces != before.braces && after.braces != 0 {
        out.push(format!(
            "braces no longer balance: {} more `{}` than `{}` (it was {} before)",
            after.braces.abs(),
            if after.braces > 0 { "{" } else { "}" },
            if after.braces > 0 { "}" } else { "{" },
            before.braces
        ));
    }
    for (name, n) in &after.environments {
        if before.environments.get(name) != Some(n) {
            out.push(format!(
                "`{name}` has {} \\{} without a match",
                n.abs(),
                if *n > 0 { "begin" } else { "end" }
            ));
        }
    }
    if after.dollars % 2 == 1 && after.dollars != before.dollars {
        out.push("an odd number of `$` signs: some maths is not closed".to_string());
    }
    out
}

/// Is this a path whose source is LaTeX?
pub fn is_latex(path: &str) -> bool {
    [".tex", ".sty", ".cls", ".ltx"].iter().any(|e| path.to_ascii_lowercase().ends_with(e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn balanced_text_reports_nothing() {
        let text = "\\begin{document}\n\\textbf{a} $x$ \\{ 50\\% % ignored {\n\\end{document}\n";
        assert_eq!(balance(text), Balance { braces: 0, dollars: 2, environments: BTreeMap::new() });
        assert!(new_problems("", text).is_empty());
    }

    #[test]
    fn a_dropped_brace_environment_or_dollar_is_named() {
        let old = "\\begin{table}\n\\textbf{a} $x$\n\\end{table}\n";
        let new = "\\begin{table}\n\\textbf{a $x\n";
        let problems = new_problems(old, new);
        assert_eq!(problems.len(), 3, "{problems:?}");
        assert!(problems[0].contains("braces"));
        assert!(problems[1].contains("table"));
        assert!(problems[2].contains("`$`"));
    }

    #[test]
    fn a_problem_the_file_already_had_is_not_repeated() {
        let old = "\\begin{a}\n{ oops\n";
        let new = "\\begin{a}\n{ oops\nmore text\n";
        assert!(new_problems(old, new).is_empty());
    }

    #[test]
    fn display_math_dollars_pair_up() {
        assert!(new_problems("", "$$x$$ and $y$").is_empty());
    }
}
