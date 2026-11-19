//! The difference between two versions of a script, by lines: what the
//! owner is shown before he approves.
//!
//! The lines the two have in common are found with Myers' algorithm, which
//! costs time and memory by how much the two differ. A script is at most
//! 128 KiB, but one that shares nothing with the other would still cost too
//! much, so the search stops after [`MAX_EDITS`] changed lines. Past that
//! the difference is still exact and still complete, only not the shortest:
//! what the search did not match is shown as removed and added whole. Lines
//! at the start and at the end that are the same are always found.
//!
//! Applying a difference to the old text gives the new text, whatever the
//! two are: `tests::any_difference_rebuilds_both_texts` holds it to that.

/// One line of a difference.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Line {
    /// In both.
    Same(String),
    /// Only in the old text.
    Removed(String),
    /// Only in the new text.
    Added(String),
}

/// How many changed lines the search for common lines goes through.
pub const MAX_EDITS: usize = 1500;

/// Lines of unchanged text shown around a change.
pub const CONTEXT: usize = 3;

/// A text as its lines. A line keeps no line ending; a text that does not
/// end with one has the same lines as one that does, and [`Difference`]
/// says separately when only that differs.
fn lines_of(text: &str) -> Vec<&str> {
    let mut lines: Vec<&str> = text.split('\n').collect();
    if lines.last() == Some(&"") {
        lines.pop();
    }
    lines
        .into_iter()
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .collect()
}

/// The difference between two texts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Difference {
    pub lines: Vec<Line>,
    /// The two differ in bytes but not in any line: in line endings, or in
    /// whether the last line has one.
    pub only_line_endings: bool,
}

impl Difference {
    pub fn added(&self) -> usize {
        self.lines
            .iter()
            .filter(|line| matches!(line, Line::Added(_)))
            .count()
    }

    pub fn removed(&self) -> usize {
        self.lines
            .iter()
            .filter(|line| matches!(line, Line::Removed(_)))
            .count()
    }

    /// No line differs.
    pub fn is_empty(&self) -> bool {
        self.added() == 0 && self.removed() == 0
    }

    /// The difference as text, in the unified form: `@@ -old +new @@`
    /// headers, then lines that start with a space, `-` or `+`, with
    /// [`CONTEXT`] unchanged lines around each change. `show` is applied to
    /// every line's text before it is written.
    pub fn unified(&self, show: impl Fn(&str) -> String) -> String {
        let changed: Vec<usize> = self
            .lines
            .iter()
            .enumerate()
            .filter(|(_, line)| !matches!(line, Line::Same(_)))
            .map(|(index, _)| index)
            .collect();
        let mut out = String::new();
        let mut at = 0;
        // The numbers of the old and the new line that `self.lines[i]` is.
        let mut numbers = Vec::with_capacity(self.lines.len());
        let (mut old, mut new) = (1, 1);
        for line in &self.lines {
            numbers.push((old, new));
            match line {
                Line::Same(_) => {
                    old += 1;
                    new += 1;
                }
                Line::Removed(_) => old += 1,
                Line::Added(_) => new += 1,
            }
        }
        while at < changed.len() {
            // One hunk: changes whose contexts touch.
            let first = changed[at];
            let mut last = first;
            while at + 1 < changed.len() && changed[at + 1] <= last + 2 * CONTEXT + 1 {
                at += 1;
                last = changed[at];
            }
            at += 1;
            let from = first.saturating_sub(CONTEXT);
            let to = (last + CONTEXT + 1).min(self.lines.len());
            let hunk = &self.lines[from..to];
            let olds = hunk
                .iter()
                .filter(|line| !matches!(line, Line::Added(_)))
                .count();
            let news = hunk
                .iter()
                .filter(|line| !matches!(line, Line::Removed(_)))
                .count();
            let (old_start, new_start) = numbers[from];
            // An empty side is written with the line before it, as `diff`.
            let start = |start: usize, count: usize| if count == 0 { start - 1 } else { start };
            out.push_str(&format!(
                "@@ -{},{olds} +{},{news} @@\n",
                start(old_start, olds),
                start(new_start, news)
            ));
            for line in hunk {
                let (mark, text) = match line {
                    Line::Same(text) => (' ', text),
                    Line::Removed(text) => ('-', text),
                    Line::Added(text) => ('+', text),
                };
                out.push(mark);
                out.push_str(&show(text));
                out.push('\n');
            }
        }
        out
    }
}

/// The difference between `old` and `new`, by lines.
pub fn difference(old: &str, new: &str) -> Difference {
    let a = lines_of(old);
    let b = lines_of(new);
    let prefix = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let suffix = a[prefix..]
        .iter()
        .rev()
        .zip(b[prefix..].iter().rev())
        .take_while(|(x, y)| x == y)
        .count();
    let mut lines = Vec::with_capacity(a.len().max(b.len()));
    lines.extend(a[..prefix].iter().map(|line| Line::Same((*line).into())));
    middle(
        &a[prefix..a.len() - suffix],
        &b[prefix..b.len() - suffix],
        &mut lines,
    );
    lines.extend(
        a[a.len() - suffix..]
            .iter()
            .map(|line| Line::Same((*line).into())),
    );
    let same_lines = lines.iter().all(|line| matches!(line, Line::Same(_)));
    Difference {
        lines,
        only_line_endings: same_lines && old != new,
    }
}

/// Myers' search over two runs of lines that differ at both ends. What it
/// does not match within [`MAX_EDITS`] is written as removed, then added.
fn middle(a: &[&str], b: &[&str], out: &mut Vec<Line>) {
    let (n, m) = (a.len() as isize, b.len() as isize);
    if n == 0 || m == 0 {
        out.extend(a.iter().map(|line| Line::Removed((*line).into())));
        out.extend(b.iter().map(|line| Line::Added((*line).into())));
        return;
    }
    let limit = MAX_EDITS.min((n + m) as usize) as isize;
    // `v[k + offset]` is the furthest x reached on diagonal k = x - y. One
    // copy is kept for each number of edits, to walk back through.
    let offset = limit + 1;
    let mut v = vec![0isize; (2 * limit + 3) as usize];
    let mut trace: Vec<Vec<isize>> = Vec::new();
    let mut reached = None;
    'search: for d in 0..=limit {
        trace.push(v[(offset - d - 1) as usize..=(offset + d + 1) as usize].to_vec());
        let mut k = -d;
        while k <= d {
            let index = (k + offset) as usize;
            let mut x = if k == -d || (k != d && v[index - 1] < v[index + 1]) {
                v[index + 1]
            } else {
                v[index - 1] + 1
            };
            let mut y = x - k;
            while x < n && y < m && a[x as usize] == b[y as usize] {
                x += 1;
                y += 1;
            }
            v[index] = x;
            if x >= n && y >= m {
                reached = Some(d);
                break 'search;
            }
            k += 2;
        }
    }
    let Some(edits) = reached else {
        // Too different to search through: all of one, then all of the other.
        out.extend(a.iter().map(|line| Line::Removed((*line).into())));
        out.extend(b.iter().map(|line| Line::Added((*line).into())));
        return;
    };
    // Walk back from the end. `trace[d]` holds diagonals -d-1..=d+1 as they
    // were before the round of `d` edits.
    let mut steps = Vec::new();
    let (mut x, mut y) = (n, m);
    for d in (1..=edits).rev() {
        let before = &trace[d as usize];
        let at = |k: isize| before[(k + d + 1) as usize];
        let k = x - y;
        let previous = if k == -d || (k != d && at(k - 1) < at(k + 1)) {
            k + 1
        } else {
            k - 1
        };
        let previous_x = at(previous);
        let previous_y = previous_x - previous;
        while x > previous_x && y > previous_y {
            x -= 1;
            y -= 1;
            steps.push(Line::Same(a[x as usize].into()));
        }
        if x == previous_x {
            y -= 1;
            steps.push(Line::Added(b[y as usize].into()));
        } else {
            x -= 1;
            steps.push(Line::Removed(a[x as usize].into()));
        }
    }
    while x > 0 && y > 0 {
        x -= 1;
        y -= 1;
        steps.push(Line::Same(a[x as usize].into()));
    }
    steps.reverse();
    out.extend(steps);
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn rebuild(difference: &Difference) -> (Vec<String>, Vec<String>) {
        let (mut old, mut new) = (Vec::new(), Vec::new());
        for line in &difference.lines {
            match line {
                Line::Same(text) => {
                    old.push(text.clone());
                    new.push(text.clone());
                }
                Line::Removed(text) => old.push(text.clone()),
                Line::Added(text) => new.push(text.clone()),
            }
        }
        (old, new)
    }

    fn owned(lines: Vec<&str>) -> Vec<String> {
        lines.into_iter().map(str::to_owned).collect()
    }

    #[test]
    fn a_change_in_the_middle_is_the_shortest_difference() {
        let old = "a\nb\nc\nd\ne\n";
        let new = "a\nb\nX\nd\nY\ne\n";
        let found = difference(old, new);
        assert_eq!(
            found.lines,
            [
                Line::Same("a".into()),
                Line::Same("b".into()),
                Line::Removed("c".into()),
                Line::Added("X".into()),
                Line::Same("d".into()),
                Line::Added("Y".into()),
                Line::Same("e".into()),
            ]
        );
        assert_eq!((found.added(), found.removed()), (2, 1));
        assert!(!found.only_line_endings);
    }

    #[test]
    fn the_unified_form_has_headers_context_and_marks() {
        let old: String = (1..=20).map(|n| format!("line {n}\n")).collect();
        let new = old
            .replace("line 5\n", "line five\n")
            .replace("line 16\n", "");
        let text = difference(&old, &new).unified(str::to_owned);
        assert_eq!(
            text,
            "@@ -2,7 +2,7 @@\n line 2\n line 3\n line 4\n-line 5\n+line five\n line 6\n line 7\n line 8\n\
             @@ -13,7 +13,6 @@\n line 13\n line 14\n line 15\n-line 16\n line 17\n line 18\n line 19\n"
        );
        // Changes whose contexts touch are one hunk.
        let near = old
            .replace("line 5\n", "line five\n")
            .replace("line 9\n", "line nine\n");
        let text = difference(&old, &near).unified(str::to_owned);
        assert_eq!(text.matches("@@ -").count(), 1, "{text}");
        assert!(text.starts_with("@@ -2,11 +2,11 @@\n"), "{text}");
    }

    #[test]
    fn a_first_version_is_all_added_and_an_equal_one_is_empty() {
        let all = difference("", "a\nb\n");
        assert_eq!(
            all.lines,
            [Line::Added("a".into()), Line::Added("b".into())]
        );
        assert_eq!(all.unified(str::to_owned), "@@ -0,0 +1,2 @@\n+a\n+b\n");
        let none = difference("a\nb\n", "a\nb\n");
        assert!(none.is_empty() && !none.only_line_endings);
        assert_eq!(none.unified(str::to_owned), "");
    }

    #[test]
    fn a_change_of_line_endings_alone_is_said_and_shows_no_line() {
        let found = difference("a\nb\n", "a\r\nb");
        assert!(found.is_empty());
        assert!(found.only_line_endings);
    }

    #[test]
    fn two_texts_with_nothing_in_common_are_still_shown_whole() {
        let old: String = (0..4000).map(|n| format!("old {n}\n")).collect();
        let new: String = (0..4000).map(|n| format!("new {n}\n")).collect();
        let started = std::time::Instant::now();
        let found = difference(&format!("top\n{old}end\n"), &format!("top\n{new}end\n"));
        assert!(
            started.elapsed() < std::time::Duration::from_secs(20),
            "bounded: {:?}",
            started.elapsed()
        );
        assert_eq!((found.removed(), found.added()), (4000, 4000));
        assert_eq!(found.lines.first(), Some(&Line::Same("top".into())));
        assert_eq!(found.lines.last(), Some(&Line::Same("end".into())));
        let (rebuilt_old, rebuilt_new) = rebuild(&found);
        assert_eq!(rebuilt_old.len(), 4002);
        assert_eq!(rebuilt_new[1], "new 0");
    }

    #[test]
    fn what_a_line_holds_is_shown_through_the_callers_filter() {
        let found = difference("a\n", "a\x1b[2J\n");
        let text = found.unified(|line| line.replace('\x1b', "<ESC>"));
        assert_eq!(text, "@@ -1,1 +1,1 @@\n-a\n+a<ESC>[2J\n");
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(400))]
        /// Whatever the two texts, the difference holds every line of the
        /// old one in order and every line of the new one in order.
        #[test]
        fn any_difference_rebuilds_both_texts(
            old in proptest::collection::vec("[abc]{0,2}", 0..40),
            new in proptest::collection::vec("[abc]{0,2}", 0..40),
        ) {
            let join = |lines: &[String]| lines.iter().map(|line| format!("{line}\n")).collect::<String>();
            let found = difference(&join(&old), &join(&new));
            let (rebuilt_old, rebuilt_new) = rebuild(&found);
            prop_assert_eq!(rebuilt_old, old.clone());
            prop_assert_eq!(rebuilt_new, new.clone());
            // And it is the shortest: as many lines kept as the longest
            // common subsequence has.
            let (n, m) = (old.len(), new.len());
            let mut table = vec![vec![0usize; m + 1]; n + 1];
            for i in 0..n {
                for j in 0..m {
                    table[i + 1][j + 1] = if old[i] == new[j] {
                        table[i][j] + 1
                    } else {
                        table[i][j + 1].max(table[i + 1][j])
                    };
                }
            }
            let kept = found.lines.iter().filter(|line| matches!(line, Line::Same(_))).count();
            prop_assert_eq!(kept, table[n][m]);
        }
    }

    #[test]
    fn lines_are_split_at_line_breaks_only() {
        assert_eq!(lines_of("a\n\nb"), owned(vec!["a", "", "b"]));
        assert_eq!(lines_of("a\r\nb\r\n"), owned(vec!["a", "b"]));
        assert!(lines_of("").is_empty());
        assert_eq!(lines_of("\n"), owned(vec![""]));
    }
}
