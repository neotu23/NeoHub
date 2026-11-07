//! Terminal view model: turns the core's parsed grid into rows a GPUI view can draw.
//!
//! The core owns parsing (bounded terminal state); this layer only decides *which* rows are
//! visible, how wide each cell is, and where the cursor sits. Nothing here touches a
//! process, a database or the raw byte stream, so it is cheap to rebuild every frame.

use neohub_core::logs::{CellStyle, TerminalSnapshot, TextRun};

/// One display row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DisplayRow {
    /// 0-based index within the full scrollback+grid text.
    pub index: usize,
    pub text: String,
    /// True for the row the terminal cursor is on (only meaningful while following).
    pub has_cursor: bool,
    /// Same row split into coloured runs (empty when the row has no explicit styling).
    pub spans: Vec<TextRun>,
}

/// Rendered view over one task's terminal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerminalView {
    /// Full scrollback + grid, one string per row (already trimmed of trailing blanks).
    rows: Vec<String>,
    /// Per-row styled runs, parallel to `rows` (scrollback rows carry default styles).
    styled: Vec<Vec<TextRun>>,
    cursor_row: usize,
    cursor_col: usize,
    scrollback_rows: usize,
    truncated: bool,
    /// Index of the first visible row when the view is *not* following the tail.
    scroll_top: usize,
    /// True while the viewport tracks the live tail.
    follow: bool,
}

impl TerminalView {
    /// Build from a core snapshot. `scrollback` carries the historical rows the core kept;
    /// `snapshot.lines` is the live grid.
    pub fn from_snapshot(snapshot: &TerminalSnapshot, scrollback: Vec<String>) -> Self {
        let mut rows: Vec<String> = scrollback
            .into_iter()
            .map(|line| line.trim_end().to_string())
            .collect();
        let scrollback_rows = rows.len();
        let mut styled: Vec<Vec<TextRun>> = rows
            .iter()
            .map(|line| {
                vec![TextRun {
                    text: line.clone(),
                    style: CellStyle::default(),
                }]
            })
            .collect();
        rows.extend(
            snapshot
                .lines
                .iter()
                .map(|line| line.trim_end().to_string()),
        );
        styled.extend(snapshot.styled.iter().cloned());
        TerminalView {
            rows,
            styled,
            cursor_row: scrollback_rows + snapshot.cursor_row as usize,
            cursor_col: snapshot.cursor_col as usize,
            scrollback_rows,
            truncated: snapshot.truncated,
            scroll_top: 0,
            follow: true,
        }
    }

    /// Build from plain text (export/search path) — no cursor, no truncation flag.
    pub fn from_text(text: &str) -> Self {
        let rows: Vec<String> = text
            .lines()
            .map(|line| line.trim_end().to_string())
            .collect();
        let last = rows.len().saturating_sub(1);
        let styled: Vec<Vec<TextRun>> = rows
            .iter()
            .map(|line| {
                vec![TextRun {
                    text: line.clone(),
                    style: CellStyle::default(),
                }]
            })
            .collect();
        TerminalView {
            rows,
            styled,
            cursor_row: last,
            cursor_col: 0,
            scrollback_rows: 0,
            truncated: false,
            scroll_top: 0,
            follow: true,
        }
    }

    pub fn total_rows(&self) -> usize {
        self.rows.len()
    }

    pub fn rows(&self) -> &[String] {
        &self.rows
    }

    pub fn cursor(&self) -> (usize, usize) {
        (self.cursor_row, self.cursor_col)
    }

    pub fn is_truncated(&self) -> bool {
        self.truncated
    }

    pub fn scrollback_rows(&self) -> usize {
        self.scrollback_rows
    }

    /// Rows to draw for a viewport `height` rows tall.
    ///
    /// * `follow = true` → the tail, with the cursor row last;
    /// * `follow = false` → a fixed window starting at `scroll_top` (the user scrolled up).
    pub fn visible(&self, height: usize, follow: bool) -> Vec<DisplayRow> {
        let height = height.max(1);
        let total = self.rows.len();
        let start = if follow {
            total.saturating_sub(height)
        } else {
            self.scroll_top.min(total.saturating_sub(1))
        };
        let end = (start + height).min(total);
        (start..end)
            .map(|index| DisplayRow {
                index,
                text: self.rows[index].clone(),
                has_cursor: follow && index == self.cursor_row,
                spans: self.styled.get(index).cloned().unwrap_or_else(|| {
                    vec![TextRun {
                        text: self.rows[index].clone(),
                        style: CellStyle::default(),
                    }]
                }),
            })
            .collect()
    }

    pub fn is_following(&self) -> bool {
        self.follow
    }

    /// Scroll up by `lines`, pausing follow. Returns the new top row.
    ///
    /// The first scroll starts from the tail, so one wheel tick shows the previous screen
    /// rather than jumping to row 0.
    pub fn scroll_up(&mut self, lines: usize, height: usize) -> usize {
        let max_top = self.rows.len().saturating_sub(height.max(1));
        let base = if self.follow {
            max_top
        } else {
            self.scroll_top
        };
        self.scroll_top = base.saturating_sub(lines);
        self.follow = false;
        self.scroll_top
    }

    /// Scroll back down; reaching the bottom resumes following.
    pub fn scroll_down(&mut self, lines: usize, height: usize) -> (usize, bool) {
        let max_top = self.rows.len().saturating_sub(height.max(1));
        self.scroll_top = (self.scroll_top + lines).min(max_top);
        self.follow = self.scroll_top >= max_top;
        (self.scroll_top, self.follow)
    }

    /// Jump back to the live tail.
    pub fn follow_tail(&mut self) {
        self.follow = true;
        self.scroll_top = 0;
    }

    /// Visible text as one string (copy the visible screen).
    pub fn visible_text(&self, height: usize, follow: bool) -> String {
        self.visible(height, follow)
            .into_iter()
            .map(|row| row.text)
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Whole buffer as text (export).
    pub fn text(&self) -> String {
        self.rows.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neohub_core::logs::{CellStyle, TerminalSnapshot, TextRun};

    fn snapshot(lines: &[&str], cursor: (u16, u16), truncated: bool) -> TerminalSnapshot {
        TerminalSnapshot {
            cols: 40,
            rows: lines.len() as u16,
            lines: lines.iter().map(|line| line.to_string()).collect(),
            styled: lines
                .iter()
                .map(|line| {
                    vec![TextRun {
                        text: line.to_string(),
                        style: CellStyle::default(),
                    }]
                })
                .collect(),
            scrollback_lines: 0,
            cursor_row: cursor.0,
            cursor_col: cursor.1,
            parsed_bytes: 100,
            truncated,
        }
    }

    #[test]
    fn following_shows_the_tail_with_the_cursor_on_its_row() {
        let view = TerminalView::from_snapshot(
            &snapshot(&["one", "two", "three", "four"], (3, 4), false),
            vec![],
        );
        let rows = view.visible(2, true);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].text, "three");
        assert_eq!(rows[1].text, "four");
        assert!(rows[1].has_cursor);
        assert_eq!(view.cursor(), (3, 4));
    }

    #[test]
    fn scrolling_up_pauses_follow_and_scrolling_back_resumes() {
        let mut view = TerminalView::from_snapshot(
            &snapshot(&["a", "b", "c", "d", "e"], (4, 0), false),
            vec![],
        );
        let top = view.scroll_up(2, 2);
        assert_eq!(top, 1, "two rows up from the tail");
        let rows = view.visible(2, false);
        assert_eq!(rows[0].text, "b");
        assert!(
            !rows.iter().any(|row| row.has_cursor),
            "cursor only while following"
        );

        let (_, following) = view.scroll_down(5, 2);
        assert!(following, "scrolling to the bottom resumes following");
        assert_eq!(view.visible(2, following).last().unwrap().text, "e");
    }

    #[test]
    fn scrollback_rows_come_before_the_live_grid() {
        let view = TerminalView::from_snapshot(
            &snapshot(&["now"], (0, 3), true),
            vec!["old-1".into(), "old-2".into()],
        );
        assert_eq!(view.scrollback_rows(), 2);
        assert_eq!(view.rows()[0], "old-1");
        assert_eq!(view.rows()[2], "now");
        assert!(view.is_truncated());
        assert_eq!(
            view.cursor(),
            (2, 3),
            "the cursor index accounts for scrollback"
        );
    }

    #[test]
    fn trailing_blank_cells_are_trimmed_but_inner_spacing_is_kept() {
        let view = TerminalView::from_snapshot(&snapshot(&["a  b    "], (0, 7), false), vec![]);
        assert_eq!(view.rows()[0], "a  b");
    }

    #[test]
    fn an_empty_terminal_renders_nothing_instead_of_panicking() {
        let view = TerminalView::from_text("");
        assert_eq!(view.total_rows(), 0);
        assert!(view.visible(10, true).is_empty());
        assert!(view.visible(10, false).is_empty());
    }

    #[test]
    fn text_export_keeps_every_row() {
        let view = TerminalView::from_text("one\ntwo\nthree");
        assert_eq!(view.text(), "one\ntwo\nthree");
        assert_eq!(view.visible_text(2, true), "two\nthree");
    }
}

/// Character ranges of `needle` inside `text`, case-insensitively.
///
/// Character indices, not byte offsets: slicing UTF-8 by bytes on a non-boundary panics, and a
/// log line with CJK or emoji is normal. Rendering splits on the same character vector, so the two
/// agree by construction.
pub fn highlight_ranges(text: &str, needle: &str) -> Vec<(usize, usize)> {
    let needle: Vec<char> = needle.chars().flat_map(|c| c.to_lowercase()).collect();
    if needle.is_empty() {
        return Vec::new();
    }
    let chars: Vec<char> = text.chars().collect();
    let lowered: Vec<char> = text.chars().flat_map(|c| c.to_lowercase()).collect();
    // A character can lowercase into several (rare, but it happens), so the lowered vector is not
    // guaranteed to line up with `chars`. Only trust it when lengths match; otherwise fall back to
    // a case-sensitive scan rather than mis-highlighting.
    let aligned = lowered.len() == chars.len();
    let haystack = if aligned { lowered } else { chars.clone() };

    let mut ranges = Vec::new();
    let mut index = 0;
    while index + needle.len() <= haystack.len() {
        if haystack[index..index + needle.len()] == needle[..] {
            ranges.push((index, index + needle.len()));
            // Skip past the match: overlapping hits on the same text would draw twice.
            index += needle.len();
        } else {
            index += 1;
        }
    }
    ranges
}

/// Split one styled run into pieces, marking the highlighted ones.
///
/// `offset` is the run's starting character index within the row, so ranges computed for the whole
/// row can be applied per run.
pub fn split_run_for_highlight(
    run_text: &str,
    offset: usize,
    ranges: &[(usize, usize)],
) -> Vec<(String, bool)> {
    if ranges.is_empty() || run_text.is_empty() {
        return vec![(run_text.to_string(), false)];
    }
    let chars: Vec<char> = run_text.chars().collect();
    let end = offset + chars.len();
    let mut cuts: Vec<usize> = vec![offset];
    for (start, stop) in ranges {
        for boundary in [*start, *stop] {
            if boundary > offset && boundary < end {
                cuts.push(boundary);
            }
        }
    }
    cuts.sort_unstable();
    cuts.dedup();

    let mut pieces = Vec::new();
    for (position, start) in cuts.iter().enumerate() {
        let stop = cuts.get(position + 1).copied().unwrap_or(end);
        if stop <= *start {
            continue;
        }
        let slice: String = chars[*start - offset..stop - offset].iter().collect();
        let highlighted = ranges
            .iter()
            .any(|(range_start, range_stop)| *start >= *range_start && *start < *range_stop);
        pieces.push((slice, highlighted));
    }
    pieces
}

#[cfg(test)]
mod highlight_tests {
    use super::*;

    #[test]
    fn a_plain_substring_is_found() {
        assert_eq!(highlight_ranges("error: boom", "boom"), vec![(7, 11)]);
    }

    #[test]
    fn matching_is_case_insensitive() {
        assert_eq!(highlight_ranges("Error: BOOM", "boom"), vec![(7, 11)]);
        assert_eq!(highlight_ranges("boom boom", "BOOM"), vec![(0, 4), (5, 9)]);
    }

    #[test]
    fn an_empty_needle_highlights_nothing() {
        assert!(highlight_ranges("anything", "").is_empty());
    }

    #[test]
    fn a_needle_longer_than_the_line_finds_nothing() {
        assert!(highlight_ranges("short", "much longer needle").is_empty());
    }

    #[test]
    fn overlapping_hits_do_not_draw_twice() {
        // "aaa" contains "aa" twice overlapping; one highlight per region is what we want.
        assert_eq!(highlight_ranges("aaaa", "aa"), vec![(0, 2), (2, 4)]);
    }

    #[test]
    fn non_ascii_text_is_measured_in_characters() {
        // Byte offsets would be (0,6) here, which would slice a multi-byte character in half.
        assert_eq!(highlight_ranges("错误: boom", "boom"), vec![(4, 8)]);
        assert_eq!(highlight_ranges("中文中文", "中文"), vec![(0, 2), (2, 4)]);
    }

    #[test]
    fn a_run_is_split_around_the_match() {
        let pieces = split_run_for_highlight("error: boom", 0, &[(7, 11)]);
        assert_eq!(
            pieces,
            vec![("error: ".to_string(), false), ("boom".to_string(), true)]
        );
    }

    #[test]
    fn a_run_with_no_match_comes_back_whole() {
        assert_eq!(
            split_run_for_highlight("nothing here", 0, &[(50, 60)]),
            vec![("nothing here".to_string(), false)]
        );
    }

    #[test]
    fn offsets_line_up_across_runs() {
        // Second run starts at character 7: the match lands inside it.
        let pieces = split_run_for_highlight("boom!", 7, &[(7, 11)]);
        assert_eq!(
            pieces,
            vec![("boom".to_string(), true), ("!".to_string(), false)]
        );
    }
}
