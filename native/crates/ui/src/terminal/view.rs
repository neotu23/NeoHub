//! Terminal view model: turns the core's parsed grid into rows a GPUI view can draw.
//!
//! The core owns parsing (bounded terminal state); this layer only decides *which* rows are
//! visible, how wide each cell is, and where the cursor sits. Nothing here touches a
//! process, a database or the raw byte stream, so it is cheap to rebuild every frame.

use neohub_core::logs::TerminalSnapshot;

/// One display row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DisplayRow {
    /// 0-based index within the full scrollback+grid text.
    pub index: usize,
    pub text: String,
    /// True for the row the terminal cursor is on (only meaningful while following).
    pub has_cursor: bool,
}

/// Rendered view over one task's terminal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerminalView {
    /// Full scrollback + grid, one string per row (already trimmed of trailing blanks).
    rows: Vec<String>,
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
        rows.extend(
            snapshot
                .lines
                .iter()
                .map(|line| line.trim_end().to_string()),
        );
        TerminalView {
            rows,
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
        TerminalView {
            rows,
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
    use neohub_core::logs::TerminalSnapshot;

    fn snapshot(lines: &[&str], cursor: (u16, u16), truncated: bool) -> TerminalSnapshot {
        TerminalSnapshot {
            cols: 40,
            rows: lines.len() as u16,
            lines: lines.iter().map(|line| line.to_string()).collect(),
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
