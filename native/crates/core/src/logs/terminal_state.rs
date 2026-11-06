//! Bounded terminal state for one task run.
//!
//! The ANSI parser is the maintained `vte` crate (the same parser Alacritty and Zed use) —
//! never a regex that strips escape sequences. `vte` keeps its state across `advance` calls,
//! so a UTF-8 sequence or an escape sequence split across PTY chunks is parsed correctly.
//!
//! What we model: a fixed grid, a bounded scrollback, cursor movement, line/display erase,
//! carriage-return overwriting (progress bars), and wide characters (CJK/emoji occupy two
//! cells). What we deliberately do not model: colours/attributes (the UI renders the theme's
//! own colours), mouse modes, and the alternate screen (recorded, but drawn inline).
//!
//! The snapshot is the authoritative thing a re-opened window draws: it carries the grid,
//! the scrollback length and how many bytes were consumed, so a window never replays a
//! randomly truncated ANSI fragment.

use std::collections::VecDeque;
use unicode_width::UnicodeWidthChar;

/// Scrollback lines kept per task (bounded so a chatty build cannot grow without limit).
pub const SCROLLBACK_LINES: usize = 2_000;

/// One grid cell. `None` means "second half of a wide character".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cell {
    Blank,
    Char(char),
    WideContinuation,
}

/// One SGR colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum AnsiColor {
    #[default]
    Default,
    /// 256-colour palette index (0-15 are the classic 16, 16-255 the extended cube).
    Indexed(u8),
    /// 24-bit colour.
    Rgb(u8, u8, u8),
}

/// Style applied to a run of cells.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct CellStyle {
    pub fg: AnsiColor,
    pub bold: bool,
}

/// A horizontally contiguous run of same-styled text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextRun {
    pub text: String,
    pub style: CellStyle,
}

/// Rendered result for the UI: plain lines plus where the cursor sits.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerminalSnapshot {
    pub cols: u16,
    pub rows: u16,
    pub lines: Vec<String>,
    /// Same rows as `lines`, split into same-style runs so the view can colour them.
    pub styled: Vec<Vec<TextRun>>,
    pub scrollback_lines: usize,
    pub cursor_row: u16,
    pub cursor_col: u16,
    /// Bytes consumed by the parser so far — the resume point for a new subscriber.
    pub parsed_bytes: u64,
    /// True once scrollback has been dropped from the front.
    pub truncated: bool,
}

/// Live terminal state for one task run.
pub struct TerminalState {
    cols: u16,
    rows: u16,
    grid: Vec<Vec<Cell>>,
    /// Per-cell style, parallel to `grid`.
    styles: Vec<Vec<CellStyle>>,
    /// Style new characters inherit (driven by SGR).
    current_style: CellStyle,
    scrollback: VecDeque<Vec<Cell>>,
    cursor_row: usize,
    cursor_col: usize,
    /// The cursor sits on the last column after a full line: the *next* character wraps.
    /// Without this flag the next character would overwrite the last one.
    pending_wrap: bool,
    parser: vte::Parser,
    parsed_bytes: u64,
    truncated: bool,
    /// Last SGR parameters seen (kept so tests can prove they never leak into the text).
    last_sgr: Vec<u16>,
    saw_alternate_screen: bool,
}

impl TerminalState {
    pub fn new(cols: u16, rows: u16) -> Self {
        let cols = cols.max(2);
        let rows = rows.max(2);
        TerminalState {
            cols,
            rows,
            grid: vec![vec![Cell::Blank; cols as usize]; rows as usize],
            styles: vec![vec![CellStyle::default(); cols as usize]; rows as usize],
            current_style: CellStyle::default(),
            scrollback: VecDeque::new(),
            cursor_row: 0,
            cursor_col: 0,
            pending_wrap: false,
            parser: vte::Parser::new(),
            parsed_bytes: 0,
            truncated: false,
            last_sgr: Vec::new(),
            saw_alternate_screen: false,
        }
    }

    /// Feed raw PTY bytes. State (including partial UTF-8/escape sequences) persists.
    pub fn feed(&mut self, bytes: &[u8]) {
        self.parsed_bytes += bytes.len() as u64;
        let mut performer = Performer { state: self };
        // `advance` needs ownership of the parser while the performer borrows the state, so
        // the parser is swapped out and back.
        let mut parser = std::mem::replace(&mut performer.state.parser, vte::Parser::new());
        parser.advance(&mut performer, bytes);
        performer.state.parser = parser;
    }

    pub fn resize(&mut self, cols: u16, rows: u16) {
        let cols = cols.max(2) as usize;
        let rows = rows.max(2) as usize;
        if cols == self.cols as usize && rows == self.rows as usize {
            return;
        }
        for line in &mut self.grid {
            line.resize(cols, Cell::Blank);
        }
        for line in &mut self.styles {
            line.resize(cols, CellStyle::default());
        }
        if rows > self.grid.len() {
            self.grid.extend(
                std::iter::repeat_with(|| vec![Cell::Blank; cols]).take(rows - self.grid.len()),
            );
        } else if rows < self.grid.len() {
            // Push the lines that fall off the top into scrollback rather than losing them.
            let drop_count = self.grid.len() - rows;
            let dropped: Vec<Vec<Cell>> = self.grid.drain(..drop_count).collect();
            for line in dropped {
                self.push_scrollback(line);
            }
            self.cursor_row = self.cursor_row.saturating_sub(drop_count);
        }
        self.cols = cols as u16;
        self.rows = rows as u16;
        self.cursor_row = self.cursor_row.min(rows.saturating_sub(1));
        self.cursor_col = self.cursor_col.min(cols.saturating_sub(1));
    }

    pub fn snapshot(&self) -> TerminalSnapshot {
        TerminalSnapshot {
            cols: self.cols,
            rows: self.rows,
            lines: self.grid.iter().map(|line| line_to_string(line)).collect(),
            styled: self
                .grid
                .iter()
                .enumerate()
                .map(|(row, line)| {
                    let styles = self.styles.get(row);
                    line_to_runs(line, styles.map(Vec::as_slice))
                })
                .collect(),
            scrollback_lines: self.scrollback.len(),
            cursor_row: self.cursor_row as u16,
            cursor_col: self.cursor_col as u16,
            parsed_bytes: self.parsed_bytes,
            truncated: self.truncated,
        }
    }

    /// Scrollback + grid as plain text (search, copy, export).
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        for line in self.scrollback.iter().chain(self.grid.iter()) {
            out.push_str(line_to_string(line).trim_end());
            out.push('\n');
        }
        out
    }

    pub fn last_sgr(&self) -> &[u16] {
        &self.last_sgr
    }

    pub fn saw_alternate_screen(&self) -> bool {
        self.saw_alternate_screen
    }

    fn push_scrollback(&mut self, line: Vec<Cell>) {
        self.scrollback.push_back(line);
        while self.scrollback.len() > SCROLLBACK_LINES {
            self.scrollback.pop_front();
            self.truncated = true;
        }
    }

    fn newline(&mut self) {
        if self.cursor_row + 1 < self.rows as usize {
            self.cursor_row += 1;
        } else {
            // Scrolled: the top line leaves the grid and joins the scrollback.
            let line = self.grid.remove(0);
            self.push_scrollback(line);
            self.grid.push(vec![Cell::Blank; self.cols as usize]);
            if !self.styles.is_empty() {
                self.styles.remove(0);
                self.styles
                    .push(vec![CellStyle::default(); self.cols as usize]);
            }
        }
        self.cursor_col = 0;
        self.pending_wrap = false;
    }

    fn put_char(&mut self, ch: char) {
        let width = UnicodeWidthChar::width(ch).unwrap_or(1).max(1);
        if self.pending_wrap || self.cursor_col + width > self.cols as usize {
            // Wrap exactly like a terminal: the last column stays occupied.
            self.pending_wrap = false;
            self.newline();
        }
        let row = self.cursor_row;
        let col = self.cursor_col;
        self.grid[row][col] = Cell::Char(ch);
        if let Some(line) = self.styles.get_mut(row) {
            if let Some(cell) = line.get_mut(col) {
                *cell = self.current_style;
            }
            if width == 2
                && let Some(cell) = line.get_mut(col + 1)
            {
                *cell = self.current_style;
            }
        }
        if width == 2 && col + 1 < self.cols as usize {
            self.grid[row][col + 1] = Cell::WideContinuation;
        }
        let next = col + width;
        if next >= self.cols as usize {
            self.cursor_col = self.cols as usize - 1;
            self.pending_wrap = true;
        } else {
            self.cursor_col = next;
        }
    }

    fn erase_line(&mut self, mode: u16) {
        let row = self.cursor_row;
        let cols = self.cols as usize;
        let (from, to) = match mode {
            1 => (0, self.cursor_col.min(cols - 1) + 1),
            2 => (0, cols),
            _ => (self.cursor_col, cols),
        };
        for col in from..to {
            self.grid[row][col] = Cell::Blank;
            if let Some(line) = self.styles.get_mut(row)
                && let Some(cell) = line.get_mut(col)
            {
                *cell = CellStyle::default();
            }
        }
    }

    fn erase_display(&mut self, mode: u16) {
        match mode {
            2 | 3 => {
                for row in 0..self.rows as usize {
                    for col in 0..self.cols as usize {
                        self.grid[row][col] = Cell::Blank;
                    }
                }
                self.cursor_row = 0;
                self.cursor_col = 0;
            }
            1 => {
                for row in 0..self.cursor_row {
                    for col in 0..self.cols as usize {
                        self.grid[row][col] = Cell::Blank;
                    }
                }
                self.erase_line(1);
            }
            _ => {
                self.erase_line(0);
                for row in self.cursor_row + 1..self.rows as usize {
                    for col in 0..self.cols as usize {
                        self.grid[row][col] = Cell::Blank;
                    }
                }
            }
        }
    }
}

struct Performer<'a> {
    state: &'a mut TerminalState,
}

impl vte::Perform for Performer<'_> {
    fn print(&mut self, c: char) {
        self.state.put_char(c);
    }

    fn execute(&mut self, byte: u8) {
        match byte {
            0x0d => {
                // CR: overwrite the line (progress bars)
                self.state.cursor_col = 0;
                self.state.pending_wrap = false;
            }
            0x0a => self.state.newline(), // LF
            0x0b | 0x0c => self.state.newline(),
            0x08 => {
                // BS
                self.state.cursor_col = self.state.cursor_col.saturating_sub(1);
            }
            0x09 => {
                // TAB
                let next = (self.state.cursor_col / 8 + 1) * 8;
                self.state.cursor_col = next.min(self.state.cols as usize - 1);
            }
            _ => {}
        }
    }

    fn csi_dispatch(
        &mut self,
        params: &vte::Params,
        intermediates: &[u8],
        _ignore: bool,
        action: char,
    ) {
        // Any explicit cursor/erase operation cancels a pending wrap.
        self.state.pending_wrap = false;
        let values: Vec<u16> = params.iter().map(|p| p[0]).collect();
        let first = values.first().copied().unwrap_or(0);
        let private = intermediates.first().copied() == Some(b'?');

        match action {
            'm' => {
                self.state.last_sgr = values.clone();
                apply_sgr(&mut self.state.current_style, &values);
            }
            'K' => self.state.erase_line(first),
            'J' => self.state.erase_display(first),
            'H' | 'f' => {
                let row = values.first().copied().unwrap_or(1).max(1) as usize - 1;
                let col = values.get(1).copied().unwrap_or(1).max(1) as usize - 1;
                self.state.cursor_row = row.min(self.state.rows as usize - 1);
                self.state.cursor_col = col.min(self.state.cols as usize - 1);
            }
            'A' => {
                let n = first.max(1) as usize;
                self.state.cursor_row = self.state.cursor_row.saturating_sub(n);
            }
            'B' => {
                let n = first.max(1) as usize;
                self.state.cursor_row =
                    (self.state.cursor_row + n).min(self.state.rows as usize - 1);
            }
            'C' => {
                let n = first.max(1) as usize;
                self.state.cursor_col =
                    (self.state.cursor_col + n).min(self.state.cols as usize - 1);
            }
            'D' => {
                let n = first.max(1) as usize;
                self.state.cursor_col = self.state.cursor_col.saturating_sub(n);
            }
            'G' => {
                let col = first.max(1) as usize - 1;
                self.state.cursor_col = col.min(self.state.cols as usize - 1);
            }
            'd' => {
                let row = first.max(1) as usize - 1;
                self.state.cursor_row = row.min(self.state.rows as usize - 1);
            }
            'h' | 'l'
                // Track the alternate-screen switch so callers can explain a TUI's output,
                // but keep drawing into the same grid (we do not emulate two screens).
                if private && values.iter().any(|v| *v == 1049 || *v == 47) => {
                    self.state.saw_alternate_screen = true;
                }
            _ => {}
        }
    }
}

/// Apply one SGR parameter list to `style`.
///
/// Covers the codes the old renderer actually honoured: reset (0), bold (1) / normal (22),
/// the 8 classic foreground colours (30-37), bright (90-97), default (39), and the extended
/// forms 38;5;n (256-colour) and 38;2;r;g;b (truecolor). Anything else is ignored rather than
/// guessed at.
pub fn apply_sgr(style: &mut CellStyle, values: &[u16]) {
    if values.is_empty() {
        *style = CellStyle::default();
        return;
    }
    let mut index = 0;
    while index < values.len() {
        match values[index] {
            0 => *style = CellStyle::default(),
            1 => style.bold = true,
            22 => style.bold = false,
            39 => style.fg = AnsiColor::Default,
            30..=37 => style.fg = AnsiColor::Indexed((values[index] - 30) as u8),
            90..=97 => style.fg = AnsiColor::Indexed((values[index] - 90 + 8) as u8),
            38 => {
                match values.get(index + 1).copied() {
                    // 38;5;n — 256-colour palette
                    Some(5) => {
                        if let Some(value) = values.get(index + 2) {
                            style.fg = AnsiColor::Indexed(*value as u8);
                            index += 2;
                        }
                    }
                    // 38;2;r;g;b — truecolor
                    Some(2) => {
                        if let (Some(r), Some(g), Some(b)) = (
                            values.get(index + 2),
                            values.get(index + 3),
                            values.get(index + 4),
                        ) {
                            style.fg = AnsiColor::Rgb(*r as u8, *g as u8, *b as u8);
                            index += 4;
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        index += 1;
    }
}

/// Split one grid row into same-style runs.
fn line_to_runs(line: &[Cell], styles: Option<&[CellStyle]>) -> Vec<TextRun> {
    let mut runs: Vec<TextRun> = Vec::new();
    let mut current = CellStyle::default();
    let mut text = String::new();
    for (index, cell) in line.iter().enumerate() {
        let style = styles
            .and_then(|styles| styles.get(index).copied())
            .unwrap_or_default();
        let ch = match cell {
            Cell::Blank => ' ',
            Cell::Char(ch) => *ch,
            Cell::WideContinuation => continue,
        };
        if style != current && !text.is_empty() {
            runs.push(TextRun {
                text: std::mem::take(&mut text),
                style: current,
            });
        }
        current = style;
        text.push(ch);
    }
    if !text.is_empty() {
        runs.push(TextRun {
            text,
            style: current,
        });
    }
    // Drop runs that are pure trailing blanks so the view does not paint empty boxes.
    while runs
        .last()
        .map(|run| run.text.trim().is_empty())
        .unwrap_or(false)
    {
        runs.pop();
    }
    runs
}

fn line_to_string(line: &[Cell]) -> String {
    let mut out = String::with_capacity(line.len());
    for cell in line {
        match cell {
            Cell::Blank => out.push(' '),
            Cell::Char(c) => out.push(*c),
            Cell::WideContinuation => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed_all(state: &mut TerminalState, chunks: &[&[u8]]) {
        for chunk in chunks {
            state.feed(chunk);
        }
    }

    #[test]
    fn sgr_colours_become_runs_and_do_not_leak_into_plain_text() {
        let mut state = TerminalState::new(40, 4);
        state.feed(b"\x1b[32mgreen\x1b[0m plain \x1b[1;34mboldblue\x1b[0m");
        let snapshot = state.snapshot();
        let runs = &snapshot.styled[0];
        assert_eq!(runs[0].text, "green");
        assert_eq!(runs[0].style.fg, AnsiColor::Indexed(2));
        assert_eq!(runs[1].text, " plain ");
        assert_eq!(runs[1].style.fg, AnsiColor::Default);
        assert_eq!(runs[2].text, "boldblue");
        assert_eq!(runs[2].style.fg, AnsiColor::Indexed(4));
        assert!(
            runs[2].style.bold,
            "1;34 sets bold and blue on the same run"
        );
        assert_eq!(
            snapshot.lines[0].trim_end(),
            "green plain boldblue",
            "the plain text view is unaffected by colour"
        );
    }

    #[test]
    fn extended_colours_are_parsed() {
        let mut state = TerminalState::new(40, 4);
        state.feed(b"\x1b[38;5;208morange\x1b[0m \x1b[38;2;1;2;3mrgb\x1b[0m");
        let runs = &state.snapshot().styled[0];
        assert_eq!(runs[0].style.fg, AnsiColor::Indexed(208));
        assert_eq!(runs[1].text, " ");
        assert_eq!(runs[2].style.fg, AnsiColor::Rgb(1, 2, 3));
    }

    #[test]
    fn erasing_a_line_clears_its_styles_too() {
        let mut state = TerminalState::new(20, 3);
        state.feed(b"\x1b[31mred text\x1b[0m\r\x1b[K");
        let runs = &state.snapshot().styled[0];
        assert!(
            runs.iter().all(|run| run.style.fg == AnsiColor::Default),
            "erase-line resets the style of the cells it clears: {runs:?}"
        );
    }

    #[test]
    fn plain_lines_land_on_consecutive_rows() {
        let mut state = TerminalState::new(40, 5);
        state.feed(b"first\r\nsecond\r\n");
        let snapshot = state.snapshot();
        assert_eq!(snapshot.lines[0].trim_end(), "first");
        assert_eq!(snapshot.lines[1].trim_end(), "second");
        assert_eq!(snapshot.cursor_row, 2);
    }

    #[test]
    fn utf8_split_across_chunks_is_reassembled() {
        let mut state = TerminalState::new(40, 5);
        // "中文" as UTF-8, split in the middle of the first character.
        let bytes = "中文".as_bytes();
        feed_all(&mut state, &[&bytes[0..2], &bytes[2..4], &bytes[4..]]);
        let snapshot = state.snapshot();
        assert_eq!(snapshot.lines[0].trim_end(), "中文");
        assert_eq!(snapshot.parsed_bytes, bytes.len() as u64);
    }

    #[test]
    fn emoji_and_cjk_occupy_two_cells() {
        let mut state = TerminalState::new(20, 3);
        state.feed("中文🚀".as_bytes());
        let snapshot = state.snapshot();
        assert_eq!(snapshot.lines[0].trim_end(), "中文🚀");
        assert_eq!(
            snapshot.cursor_col, 6,
            "two wide CJK + one wide emoji = 6 cells"
        );
    }

    #[test]
    fn ansi_sequences_split_across_chunks_are_parsed_not_printed() {
        let mut state = TerminalState::new(20, 3);
        // "\x1b[31mRED\x1b[0m" split inside the escape sequence.
        feed_all(&mut state, &[b"\x1b[3", b"1mRED", b"\x1b[0", b"m"]);
        let snapshot = state.snapshot();
        assert_eq!(
            snapshot.lines[0].trim_end(),
            "RED",
            "escape sequences must never appear as text"
        );
        assert_eq!(state.last_sgr(), &[0]);
    }

    #[test]
    fn carriage_return_overwrites_the_line() {
        let mut state = TerminalState::new(20, 3);
        for frame in ["0%", "50%", "100%"] {
            state.feed(format!("\r{frame}   ").as_bytes());
        }
        let snapshot = state.snapshot();
        assert!(
            snapshot.lines[0].starts_with("100%"),
            "got {:?}",
            snapshot.lines[0]
        );
        assert!(!snapshot.lines[0].contains("0%  1"), "no leftover frames");
    }

    #[test]
    fn erase_line_and_cursor_movement() {
        let mut state = TerminalState::new(20, 4);
        state.feed(b"aaaa\r\nbbbb\r\ncccc");
        // Move up one line, to column 3, erase to end of line.
        state.feed(b"\x1b[1A\x1b[3G\x1b[K");
        let snapshot = state.snapshot();
        assert_eq!(
            snapshot.lines[1].trim_end(),
            "bb",
            "3G is 1-based, so column 3 leaves two characters"
        );
        assert_eq!(
            snapshot.lines[2].trim_end(),
            "cccc",
            "other lines untouched"
        );
    }

    #[test]
    fn erase_display_clears_the_grid() {
        let mut state = TerminalState::new(20, 3);
        state.feed(b"one\r\ntwo\r\nthree");
        state.feed(b"\x1b[2J");
        let snapshot = state.snapshot();
        assert_eq!(snapshot.lines[0].trim_end(), "");
        assert_eq!(snapshot.lines[2].trim_end(), "");
        assert_eq!((snapshot.cursor_row, snapshot.cursor_col), (0, 0));
    }

    #[test]
    fn long_lines_scroll_into_a_bounded_scrollback() {
        let mut state = TerminalState::new(10, 3);
        for index in 0..(SCROLLBACK_LINES + 50) {
            state.feed(format!("line{index}\r\n").as_bytes());
        }
        let snapshot = state.snapshot();
        assert_eq!(snapshot.scrollback_lines, SCROLLBACK_LINES);
        assert!(
            snapshot.truncated,
            "dropping old scrollback must be reported"
        );
        let text = state.to_text();
        assert!(
            !text.contains("line0\n"),
            "the oldest lines are the ones evicted"
        );
        assert!(text.contains(&format!("line{}", SCROLLBACK_LINES + 49)));
    }

    #[test]
    fn a_line_longer_than_the_grid_wraps_instead_of_being_truncated() {
        let mut state = TerminalState::new(10, 4);
        state.feed(b"0123456789ABCDEF");
        let snapshot = state.snapshot();
        assert_eq!(snapshot.lines[0].trim_end(), "0123456789");
        assert_eq!(snapshot.lines[1].trim_end(), "ABCDEF");
    }

    #[test]
    fn resize_keeps_content_and_reports_new_dimensions() {
        let mut state = TerminalState::new(20, 4);
        state.feed(b"hello\r\nworld");
        state.resize(10, 6);
        let snapshot = state.snapshot();
        assert_eq!((snapshot.cols, snapshot.rows), (10, 6));
        assert_eq!(snapshot.lines[0].trim_end(), "hello");
        state.resize(30, 2);
        let snapshot = state.snapshot();
        assert_eq!((snapshot.cols, snapshot.rows), (30, 2));
        assert!(
            snapshot.scrollback_lines >= 1,
            "lines pushed off the top go to scrollback, not to /dev/null"
        );
    }
}
