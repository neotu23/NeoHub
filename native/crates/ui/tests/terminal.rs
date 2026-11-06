//! Terminal behaviour against the core's real parser: two tasks never mix, rebuilding the
//! view does not duplicate history, exited logs stay searchable, and wide characters keep
//! their alignment all the way from PTY bytes to display rows.

use neohub_core::logs::{SearchOptions, TerminalState};
use neohub_ui::terminal::selection::CellPos;
use neohub_ui::terminal::{MatchList, Selection, TerminalView, display_width};

fn parse(chunks: &[&[u8]], cols: u16, rows: u16) -> TerminalView {
    let mut state = TerminalState::new(cols, rows);
    for chunk in chunks {
        state.feed(chunk);
    }
    TerminalView::from_snapshot(&state.snapshot(), vec![])
}

#[test]
fn two_tasks_streaming_at_different_rates_never_mix() {
    // Task A: one line per chunk. Task B: several lines per chunk (a faster producer).
    let mut slow = TerminalState::new(80, 10);
    let mut fast = TerminalState::new(80, 10);
    for index in 1..=5 {
        slow.feed(format!("\r\nslow-{index}").as_bytes());
    }
    for index in 1..=12 {
        fast.feed(format!("\r\nfast-{index}").as_bytes());
    }

    let slow_view = TerminalView::from_snapshot(&slow.snapshot(), vec![]);
    let fast_view = TerminalView::from_snapshot(&fast.snapshot(), vec![]);

    let slow_text = slow_view.text();
    let fast_text = fast_view.text();
    assert!(slow_text.contains("slow-5"), "slow task kept its output");
    assert!(fast_text.contains("fast-12"), "fast task kept its output");
    assert!(
        !slow_text.contains("fast-"),
        "output from task B leaked into A"
    );
    assert!(
        !fast_text.contains("slow-"),
        "output from task A leaked into B"
    );
}

#[test]
fn rebuilding_the_view_does_not_duplicate_history() {
    let mut state = TerminalState::new(60, 8);
    state.feed(b"one\r\ntwo\r\nthree");
    let snapshot = state.snapshot();

    // The detail pane is torn down and recreated (tab switch / window reopen).
    let first = TerminalView::from_snapshot(&snapshot, vec![]);
    let rebuilt = TerminalView::from_snapshot(&state.snapshot(), vec![]);
    assert_eq!(first.total_rows(), rebuilt.total_rows());
    assert_eq!(first.text(), rebuilt.text());
    assert!(!rebuilt.text().contains("one\none"), "no duplicated rows");
}

#[test]
fn an_exited_task_keeps_searchable_output() {
    let mut state = TerminalState::new(80, 10);
    state.feed(b"starting\r\nbuild failed: exit 7\r\n");
    // The process has exited; the retained terminal text is still what the pane shows.
    let view = TerminalView::from_snapshot(&state.snapshot(), vec![]);
    let list = MatchList::run(view.rows(), "build failed", &SearchOptions::default())
        .expect("search a finished task");
    assert_eq!(list.len(), 1);
    assert_eq!(list.current_hit().map(|hit| hit.row), Some(1));
}

#[test]
fn wide_characters_stay_aligned_from_pty_bytes_to_rows() {
    // Split the UTF-8 sequence across chunks, as a real PTY read does.
    let bytes = "构建完成 🚀 ok".as_bytes();
    let view = parse(&[&bytes[..5], &bytes[5..13], &bytes[13..]], 40, 4);
    assert_eq!(view.rows()[0], "构建完成 🚀 ok");
    assert_eq!(
        display_width(&view.rows()[0]),
        14,
        "4 CJK (8 cells) + space + 2 + space + o + k"
    );

    // Selecting the two CJK characters must not split them.
    let selection = Selection::new(CellPos { row: 0, col: 0 }, CellPos { row: 0, col: 4 });
    assert_eq!(selection.text(view.rows()), "构建");
}

#[test]
fn scrolling_a_finished_task_shows_older_rows_and_resumes_following() {
    let lines: Vec<String> = (0..40).map(|index| format!("line-{index:02}")).collect();
    let text = lines.join("\n");
    let mut view = TerminalView::from_text(&text);
    assert!(view.is_following());

    let top = view.scroll_up(5, 10);
    assert!(!view.is_following());
    let visible = view.visible(10, false);
    assert_eq!(visible.len(), 10);
    assert_eq!(visible[0].index, top);
    assert!(visible[0].text.starts_with("line-"));

    let (_, following) = view.scroll_down(100, 10);
    assert!(following, "scrolling to the bottom resumes follow");
    let tail = view.visible(10, true);
    assert_eq!(tail.last().map(|row| row.text.as_str()), Some("line-39"));
}

#[test]
fn the_same_output_renders_identically_after_a_resize() {
    let mut state = TerminalState::new(80, 10);
    state.feed(b"alpha\r\nbeta\r\ngamma");
    let before = TerminalView::from_snapshot(&state.snapshot(), vec![]).text();

    // The visible terminal changes size: the core re-lays-out the grid, the view re-reads it.
    state.resize(120, 20);
    let after = TerminalView::from_snapshot(&state.snapshot(), vec![]).text();
    assert!(
        after.contains("alpha") && after.contains("gamma"),
        "content survives a resize: {after:?}"
    );
    let content_rows = |text: &str| text.lines().filter(|line| !line.trim().is_empty()).count();
    assert_eq!(
        content_rows(&before),
        content_rows(&after),
        "resizing changes the grid height but must not invent or drop content rows"
    );
}
