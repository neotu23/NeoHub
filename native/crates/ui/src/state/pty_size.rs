//! Turning the window's pixel width into a PTY size.
//!
//! The PTY starts at 120×30 and stays there unless someone tells it otherwise, which means a
//! widened window keeps wrapping at the original width. This module is the arithmetic that fixes
//! that — kept pure so the boundaries (tiny windows, absurd aspect ratios) can be tested without
//! a window.
//!
//! The character width is an estimate: for the monospace face these panes use, advance width is
//! about 0.6 em. Being a little off is harmless (a terminal wraps at the column the PTY believes
//! in), while being wildly off would break escape sequences that move the cursor, which is why the
//! result is clamped rather than trusted blindly.

/// Sidebar width, subtracted from the window before the terminal area is measured.
pub const SIDEBAR_WIDTH: f32 = 260.0;
/// Horizontal padding around the terminal block.
pub const TERMINAL_PADDING: f32 = 48.0;
/// Approximate advance width per character, as a fraction of the font size.
pub const CHAR_WIDTH_RATIO: f32 = 0.6;

/// Smallest terminal the UI will ask for. Below this, escape sequences in dense output start
/// misbehaving and the pane is unreadable anyway.
pub const MIN_COLS: u16 = 20;
pub const MIN_ROWS: u16 = 4;
/// Upper bound: a 400-column PTY is not a real window, it is a measurement bug.
pub const MAX_COLS: u16 = 400;
pub const MAX_ROWS: u16 = 200;

/// Columns that fit in a window of `window_width` pixels at `font_size`.
pub fn cols_for(window_width: f32, font_size: f32) -> u16 {
    if !window_width.is_finite() || !font_size.is_finite() || font_size <= 0.0 {
        return MIN_COLS;
    }
    let usable = (window_width - SIDEBAR_WIDTH - TERMINAL_PADDING).max(0.0);
    let char_width = font_size * CHAR_WIDTH_RATIO;
    if char_width <= 0.0 {
        return MIN_COLS;
    }
    let cols = (usable / char_width).floor();
    cols.clamp(f32::from(MIN_COLS), f32::from(MAX_COLS)) as u16
}

/// Rows for a requested line count, clamped.
pub fn rows_for(requested: usize) -> u16 {
    requested.clamp(MIN_ROWS as usize, MAX_ROWS as usize) as u16
}

/// The pair a resize should ask for.
pub fn pty_size(window_width: f32, font_size: f32, rows: usize) -> (u16, u16) {
    (cols_for(window_width, font_size), rows_for(rows))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wide_window_gets_more_columns_than_a_narrow_one() {
        let narrow = cols_for(900.0, 12.0);
        let wide = cols_for(1800.0, 12.0);
        assert!(wide > narrow, "{wide} should exceed {narrow}");
    }

    #[test]
    fn the_sidebar_and_padding_are_not_counted_as_terminal_space() {
        // 900px window, 12px font: (900 - 260 - 48) / 7.2 = 82 columns.
        assert_eq!(cols_for(900.0, 12.0), 82);
    }

    #[test]
    fn a_tiny_window_clamps_to_the_minimum() {
        assert_eq!(cols_for(200.0, 12.0), MIN_COLS);
        assert_eq!(cols_for(0.0, 12.0), MIN_COLS);
        assert_eq!(cols_for(-100.0, 12.0), MIN_COLS);
    }

    #[test]
    fn an_absurd_width_clamps_to_the_maximum() {
        assert_eq!(cols_for(100_000.0, 12.0), MAX_COLS);
    }

    #[test]
    fn nonsense_inputs_fall_back_to_the_minimum() {
        assert_eq!(cols_for(f32::NAN, 12.0), MIN_COLS);
        assert_eq!(cols_for(900.0, 0.0), MIN_COLS);
        assert_eq!(cols_for(900.0, f32::INFINITY), MIN_COLS);
    }

    #[test]
    fn rows_are_clamped_both_ways() {
        assert_eq!(rows_for(0), MIN_ROWS);
        assert_eq!(rows_for(1), MIN_ROWS);
        assert_eq!(rows_for(30), 30);
        assert_eq!(rows_for(10_000), MAX_ROWS);
    }

    #[test]
    fn the_pair_matches_the_start_default_on_a_typical_window() {
        // 1280 wide, 12px font, 30 lines: something close to the 120x30 the PTY starts with,
        // which is the sanity check that this arithmetic is in the right ballpark.
        let (cols, rows) = pty_size(1280.0, 12.0, 30);
        assert!((100..=140).contains(&cols), "cols={cols}");
        assert_eq!(rows, 30);
    }
}
