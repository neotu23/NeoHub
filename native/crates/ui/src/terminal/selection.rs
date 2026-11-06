//! Text selection over terminal rows.
//!
//! Columns are *cells*, not bytes: a CJK character or an emoji occupies two cells, so a
//! selection anchored at cell 3 must slice at character 2 rather than byte 3. Getting this
//! wrong is how terminals end up copying replacement characters.

/// A cell position in the terminal buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct CellPos {
    pub row: usize,
    pub col: usize,
}

/// An ordered selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selection {
    pub start: CellPos,
    pub end: CellPos,
}

impl Selection {
    /// Normalise so `start <= end` regardless of drag direction.
    pub fn new(anchor: CellPos, focus: CellPos) -> Self {
        if anchor <= focus {
            Selection {
                start: anchor,
                end: focus,
            }
        } else {
            Selection {
                start: focus,
                end: anchor,
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }

    /// Extract the selected text from `rows`.
    pub fn text(&self, rows: &[String]) -> String {
        if rows.is_empty() {
            return String::new();
        }
        let start_row = self.start.row.min(rows.len() - 1);
        let end_row = self.end.row.min(rows.len() - 1);
        let mut out = String::new();
        for (index, line) in rows.iter().enumerate().take(end_row + 1).skip(start_row) {
            let row = index;
            let from = if row == start_row {
                char_index_for_col(line, self.start.col)
            } else {
                0
            };
            let to = if row == end_row {
                char_index_for_col(line, self.end.col)
            } else {
                line.chars().count()
            };
            let slice: String = line
                .chars()
                .skip(from)
                .take(to.saturating_sub(from))
                .collect();
            out.push_str(&slice);
            if row != end_row {
                out.push('\n');
            }
        }
        out
    }
}

/// Map a display cell to a character index, honouring double-width characters.
///
/// A cell that falls on the *second* half of a wide character snaps back to that character,
/// so a selection can never start or end in the middle of one.
pub fn char_index_for_col(line: &str, col: usize) -> usize {
    let mut cell = 0usize;
    for (index, ch) in line.chars().enumerate() {
        let width = char_width(ch);
        if col < cell + width {
            return index;
        }
        cell += width;
    }
    line.chars().count()
}

/// Cell width of one character (CJK and most emoji are two cells wide).
pub fn char_width(ch: char) -> usize {
    unicode_width::UnicodeWidthChar::width(ch)
        .unwrap_or(1)
        .max(1)
}

/// Total cell width of a string.
pub fn display_width(text: &str) -> usize {
    text.chars().map(char_width).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROWS: [&str; 3] = ["first line", "中文 abc", "third"];

    fn rows() -> Vec<String> {
        ROWS.iter().map(|row| row.to_string()).collect()
    }

    #[test]
    fn a_selection_inside_one_row_returns_that_slice() {
        let selection = Selection::new(CellPos { row: 0, col: 0 }, CellPos { row: 0, col: 5 });
        assert_eq!(selection.text(&rows()), "first");
    }

    #[test]
    fn a_drag_downwards_and_upwards_gives_the_same_text() {
        let a = Selection::new(CellPos { row: 0, col: 6 }, CellPos { row: 2, col: 3 });
        let b = Selection::new(CellPos { row: 2, col: 3 }, CellPos { row: 0, col: 6 });
        assert_eq!(a.text(&rows()), b.text(&rows()));
        assert!(a.text(&rows()).starts_with("line\n"));
        assert!(a.text(&rows()).ends_with("\nthi"));
    }

    #[test]
    fn wide_characters_do_not_split() {
        // Row 1 is "中文 abc": cells are 中(2) 文(2) space(1) a(1) b(1) c(1).
        // Selecting cells 0..4 must return "中文", not a broken half-character.
        let selection = Selection::new(CellPos { row: 1, col: 0 }, CellPos { row: 1, col: 4 });
        assert_eq!(selection.text(&rows()), "中文");
        // Starting on the second half of 文 still snaps to a character boundary.
        let selection = Selection::new(CellPos { row: 1, col: 3 }, CellPos { row: 1, col: 6 });
        // Cells 3..6 cover 文(2-3), space(4) and a(5).
        assert_eq!(selection.text(&rows()), "文 a");
    }

    #[test]
    fn an_emoji_is_two_cells_and_survives_selection() {
        let row = "🚀 go";
        assert_eq!(display_width(row), 5, "emoji 2 + space 1 + g 1 + o 1");
        let selection = Selection::new(CellPos { row: 0, col: 0 }, CellPos { row: 0, col: 2 });
        assert_eq!(selection.text(&[row.to_string()]), "🚀");
    }

    #[test]
    fn an_out_of_range_selection_is_clamped_not_panicking() {
        let selection = Selection::new(CellPos { row: 0, col: 0 }, CellPos { row: 99, col: 99 });
        let text = selection.text(&rows());
        assert!(text.contains("third"));
        assert!(selection.text(&[]).is_empty());
    }

    #[test]
    fn an_empty_selection_produces_no_text() {
        let pos = CellPos { row: 1, col: 2 };
        let selection = Selection::new(pos, pos);
        assert!(selection.is_empty());
        assert_eq!(selection.text(&rows()), "");
    }
}
