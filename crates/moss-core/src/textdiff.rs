//! Line diffs and the three-way merge behind Sync.
//!
//! Both work on whole lines (terminators included), so joining the pieces back together always
//! reproduces the exact bytes, including a missing final newline.

use serde::{Deserialize, Serialize};
use similar::{ChangeTag, MergeResolution, TextDiff, TextMerge};

use crate::{Error, Result};

/// Text above this size is not diffed (the history and sync screens show a size note instead).
pub const MAX_DIFF_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LineStats {
    pub added: usize,
    pub removed: usize,
}

impl LineStats {
    pub fn add(&mut self, other: LineStats) {
        self.added += other.added;
        self.removed += other.removed;
    }
}

/// The change from `old` to `new` as a unified diff with three lines of context.
pub fn unified(old: &str, new: &str, old_label: &str, new_label: &str) -> String {
    TextDiff::from_lines(old, new)
        .unified_diff()
        .context_radius(3)
        .header(old_label, new_label)
        .to_string()
}

/// Lines added and removed going from `old` to `new`. Oversized text counts as fully replaced.
pub fn line_stats(old: &str, new: &str) -> LineStats {
    if old == new {
        return LineStats::default();
    }
    if old.len() > MAX_DIFF_BYTES || new.len() > MAX_DIFF_BYTES {
        return LineStats {
            added: new.lines().count(),
            removed: old.lines().count(),
        };
    }
    let mut stats = LineStats::default();
    for change in TextDiff::from_lines(old, new).iter_all_changes() {
        match change.tag() {
            ChangeTag::Insert => stats.added += 1,
            ChangeTag::Delete => stats.removed += 1,
            ChangeTag::Equal => {}
        }
    }
    stats
}

/// One stretch of a merged file: either settled text or an overlapping edit that needs a choice.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Region {
    Stable { text: String },
    Conflict { base: String, mine: String, theirs: String },
}

/// How to settle one [`Region::Conflict`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Choice {
    Mine,
    Theirs,
    /// Mine, then Theirs.
    Both,
    Text { text: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Merged {
    pub regions: Vec<Region>,
}

impl Merged {
    pub fn conflict_count(&self) -> usize {
        self.regions
            .iter()
            .filter(|r| matches!(r, Region::Conflict { .. }))
            .count()
    }

    /// The merged text, when no region needs a choice.
    pub fn clean_text(&self) -> Option<String> {
        if self.conflict_count() > 0 {
            return None;
        }
        self.resolve(&[]).ok()
    }

    /// Build the final text, taking one [`Choice`] per conflict, in order.
    pub fn resolve(&self, choices: &[Choice]) -> Result<String> {
        if choices.len() != self.conflict_count() {
            return Err(Error::Sync(format!(
                "expected {} choice(s) for this file, got {}",
                self.conflict_count(),
                choices.len()
            )));
        }
        let mut next = choices.iter();
        let mut out = String::new();
        for region in &self.regions {
            match region {
                Region::Stable { text } => out.push_str(text),
                Region::Conflict { mine, theirs, .. } => match next.next() {
                    Some(Choice::Mine) => out.push_str(mine),
                    Some(Choice::Theirs) => out.push_str(theirs),
                    Some(Choice::Both) => {
                        out.push_str(mine);
                        if !mine.is_empty() && !mine.ends_with('\n') {
                            out.push('\n');
                        }
                        out.push_str(theirs);
                    }
                    Some(Choice::Text { text }) => out.push_str(text),
                    None => unreachable!("length checked above"),
                },
            }
        }
        Ok(out)
    }
}

/// Merge `mine` and `theirs`, two edits of `base`. Edits that touch different lines (or make the
/// same change) merge on their own; overlapping edits become [`Region::Conflict`].
pub fn merge3(base: &str, mine: &str, theirs: &str) -> Merged {
    if mine == theirs || base == theirs {
        return stable(mine);
    }
    if base == mine {
        return stable(theirs);
    }
    if [base, mine, theirs].iter().any(|t| t.len() > MAX_DIFF_BYTES) {
        // Too big to diff: one conflict for the whole file.
        return Merged {
            regions: vec![Region::Conflict {
                base: base.to_string(),
                mine: mine.to_string(),
                theirs: theirs.to_string(),
            }],
        };
    }
    let merge = TextMerge::from_lines(base, mine, theirs);
    let base_text = |r: std::ops::Range<usize>| -> String { r.filter_map(|i| merge.base_line(i)).collect() };
    let mine_text = |r: std::ops::Range<usize>| -> String { r.filter_map(|i| merge.ours_line(i)).collect() };
    let theirs_text = |r: std::ops::Range<usize>| -> String { r.filter_map(|i| merge.theirs_line(i)).collect() };

    let mut regions: Vec<Region> = Vec::new();
    let push_stable = |regions: &mut Vec<Region>, text: String| {
        if text.is_empty() {
            return;
        }
        if let Some(Region::Stable { text: last }) = regions.last_mut() {
            last.push_str(&text);
        } else {
            regions.push(Region::Stable { text });
        }
    };
    for region in merge.regions() {
        match region.resolution() {
            MergeResolution::Conflict => regions.push(Region::Conflict {
                base: base_text(region.base_range()),
                mine: mine_text(region.ours_range()),
                theirs: theirs_text(region.theirs_range()),
            }),
            MergeResolution::Theirs => {
                push_stable(&mut regions, theirs_text(region.theirs_range()));
            }
            _ => push_stable(&mut regions, mine_text(region.ours_range())),
        }
    }
    Merged { regions }
}

fn stable(text: &str) -> Merged {
    Merged {
        regions: if text.is_empty() {
            Vec::new()
        } else {
            vec![Region::Stable { text: text.to_string() }]
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stats_count_added_and_removed_lines() {
        let s = line_stats("a\nb\nc\n", "a\nB\nc\nd\n");
        assert_eq!(s, LineStats { added: 2, removed: 1 });
        assert_eq!(line_stats("same", "same"), LineStats::default());
        assert_eq!(line_stats("", "x\ny\n"), LineStats { added: 2, removed: 0 });
    }

    #[test]
    fn edits_to_different_lines_merge_cleanly() {
        let m = merge3("one\ntwo\nthree\n", "ONE\ntwo\nthree\n", "one\ntwo\nTHREE\n");
        assert_eq!(m.clean_text().as_deref(), Some("ONE\ntwo\nTHREE\n"));
    }

    #[test]
    fn same_change_on_both_sides_is_not_a_conflict() {
        let m = merge3("a\nb\n", "a\nX\n", "a\nX\n");
        assert_eq!(m.clean_text().as_deref(), Some("a\nX\n"));
    }

    #[test]
    fn the_same_line_edited_two_ways_is_a_conflict() {
        let m = merge3("a\nb\nc\n", "a\nmine\nc\n", "a\ntheirs\nc\n");
        assert_eq!(m.conflict_count(), 1);
        assert!(m.clean_text().is_none());
        let Region::Conflict { base, mine, theirs } = &m.regions[1] else {
            panic!("expected the middle region to be the conflict: {:?}", m.regions)
        };
        assert_eq!((base.as_str(), mine.as_str(), theirs.as_str()), ("b\n", "mine\n", "theirs\n"));
        assert_eq!(m.resolve(&[Choice::Mine]).unwrap(), "a\nmine\nc\n");
        assert_eq!(m.resolve(&[Choice::Theirs]).unwrap(), "a\ntheirs\nc\n");
        assert_eq!(m.resolve(&[Choice::Both]).unwrap(), "a\nmine\ntheirs\nc\n");
        assert_eq!(
            m.resolve(&[Choice::Text { text: "by hand\n".into() }]).unwrap(),
            "a\nby hand\nc\n"
        );
    }

    #[test]
    fn choices_must_match_the_conflicts() {
        let m = merge3("a\n", "b\n", "c\n");
        assert_eq!(m.conflict_count(), 1);
        assert!(m.resolve(&[]).is_err());
        assert!(m.resolve(&[Choice::Mine, Choice::Mine]).is_err());
    }

    #[test]
    fn two_separate_conflicts_each_take_a_choice() {
        let base = "a\nx\nb\nc\nd\ny\ne\n";
        let mine = "a\nX1\nb\nc\nd\nY1\ne\n";
        let theirs = "a\nX2\nb\nc\nd\nY2\ne\n";
        let m = merge3(base, mine, theirs);
        assert_eq!(m.conflict_count(), 2);
        assert_eq!(
            m.resolve(&[Choice::Theirs, Choice::Mine]).unwrap(),
            "a\nX2\nb\nc\nd\nY1\ne\n"
        );
    }

    #[test]
    fn a_missing_final_newline_survives() {
        let m = merge3("a\nb", "A\nb", "a\nb");
        assert_eq!(m.clean_text().as_deref(), Some("A\nb"));
        let m = merge3("a\nb", "a\nb", "a\nB");
        assert_eq!(m.clean_text().as_deref(), Some("a\nB"));
    }

    #[test]
    fn both_added_without_an_ancestor_conflicts_unless_identical() {
        assert_eq!(merge3("", "x\n", "x\n").clean_text().as_deref(), Some("x\n"));
        let m = merge3("", "mine\n", "theirs\n");
        assert_eq!(m.conflict_count(), 1);
    }

    #[test]
    fn deleting_a_line_on_one_side_merges_with_an_edit_elsewhere() {
        let m = merge3("a\nb\nc\nd\n", "a\nc\nd\n", "a\nb\nc\nD\n");
        assert_eq!(m.clean_text().as_deref(), Some("a\nc\nD\n"));
    }

    #[test]
    fn edits_to_neighbouring_lines_are_a_conflict_like_in_git() {
        assert_eq!(merge3("a\nb\nc\n", "a\nc\n", "a\nb\nC\n").conflict_count(), 1);
    }

    #[test]
    fn crlf_text_round_trips() {
        let m = merge3("a\r\nb\r\nc\r\n", "A\r\nb\r\nc\r\n", "a\r\nb\r\nC\r\n");
        assert_eq!(m.clean_text().as_deref(), Some("A\r\nb\r\nC\r\n"));
    }
}
