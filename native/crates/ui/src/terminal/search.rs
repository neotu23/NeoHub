//! In-terminal search with next/previous navigation.
//!
//! Reuses the core's matcher (literal or regex, case sensitivity, invalid-pattern errors), so
//! the terminal panel and the global log search cannot disagree about what "matches".

use neohub_core::logs::{LineMatch, SearchError, SearchOptions, search_text};

/// One match, located on a row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchHit {
    /// 0-based row index in the terminal buffer.
    pub row: usize,
    /// The row text that matched.
    pub line: String,
}

/// All matches plus the navigation cursor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MatchList {
    hits: Vec<SearchHit>,
    current: Option<usize>,
    query: String,
}

impl MatchList {
    /// Empty list (no search performed yet).
    pub fn empty() -> Self {
        MatchList {
            hits: Vec::new(),
            current: None,
            query: String::new(),
        }
    }

    /// Run a search over the terminal rows.
    pub fn run(rows: &[String], query: &str, options: &SearchOptions) -> Result<Self, SearchError> {
        let text = rows.join("\n");
        let matches: Vec<LineMatch> = search_text(&text, query, options)?;
        let hits: Vec<SearchHit> = matches
            .into_iter()
            .map(|found| SearchHit {
                row: found.line_no.saturating_sub(1),
                line: found.line,
            })
            .collect();
        let current = (!hits.is_empty()).then_some(0);
        Ok(MatchList {
            hits,
            current,
            query: query.to_string(),
        })
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    pub fn hits(&self) -> &[SearchHit] {
        &self.hits
    }

    pub fn len(&self) -> usize {
        self.hits.len()
    }

    pub fn is_empty(&self) -> bool {
        self.hits.is_empty()
    }

    /// Current match index (0-based).
    pub fn current(&self) -> Option<usize> {
        self.current
    }

    pub fn current_hit(&self) -> Option<&SearchHit> {
        self.current.and_then(|index| self.hits.get(index))
    }

    /// 1-based label for the UI ("3 / 12"), empty when there are no matches.
    pub fn label(&self) -> String {
        match (self.current, self.hits.len()) {
            (Some(index), total) if total > 0 => format!("{} / {}", index + 1, total),
            _ => String::new(),
        }
    }

    /// Advance to the next match, wrapping around.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Option<&SearchHit> {
        if self.hits.is_empty() {
            return None;
        }
        let index = match self.current {
            Some(index) => (index + 1) % self.hits.len(),
            None => 0,
        };
        self.current = Some(index);
        self.hits.get(index)
    }

    /// Go to the previous match, wrapping around.
    pub fn previous(&mut self) -> Option<&SearchHit> {
        if self.hits.is_empty() {
            return None;
        }
        let index = match self.current {
            Some(0) | None => self.hits.len() - 1,
            Some(index) => index - 1,
        };
        self.current = Some(index);
        self.hits.get(index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(lines: &[&str]) -> Vec<String> {
        lines.iter().map(|line| line.to_string()).collect()
    }

    #[test]
    fn a_literal_search_finds_every_row_and_reports_positions() {
        let rows = rows(&["boot", "listening on 4319", "ready", "listening on 4320"]);
        let list = MatchList::run(&rows, "listening", &SearchOptions::default()).expect("search");
        assert_eq!(list.len(), 2);
        assert_eq!(list.current_hit().map(|hit| hit.row), Some(1));
        assert_eq!(list.label(), "1 / 2");
    }

    #[test]
    fn next_and_previous_wrap_around() {
        let rows = rows(&["a", "hit one", "b", "hit two"]);
        let mut list = MatchList::run(&rows, "hit", &SearchOptions::default()).expect("search");
        assert_eq!(list.next().map(|hit| hit.row), Some(3));
        assert_eq!(list.label(), "2 / 2");
        assert_eq!(
            list.next().map(|hit| hit.row),
            Some(1),
            "wraps to the first"
        );
        assert_eq!(
            list.previous().map(|hit| hit.row),
            Some(3),
            "wraps backwards"
        );
    }

    #[test]
    fn regex_search_is_available_and_invalid_patterns_are_errors() {
        let rows = rows(&["port 4319", "port 4320", "no port here"]);
        let options = SearchOptions {
            regex: true,
            ..Default::default()
        };
        let list = MatchList::run(&rows, r"port \d+", &options).expect("regex");
        assert_eq!(list.len(), 2);
        assert!(MatchList::run(&rows, "[unclosed", &options).is_err());
    }

    #[test]
    fn no_matches_is_an_empty_list_not_a_panic() {
        let rows = rows(&["a", "b"]);
        let list = MatchList::run(&rows, "zzz", &SearchOptions::default()).expect("search");
        assert!(list.is_empty());
        assert_eq!(list.label(), "");
        assert_eq!(list.current(), None);
        let mut list = list;
        assert!(list.next().is_none());
        assert!(list.previous().is_none());
    }

    #[test]
    fn the_result_limit_is_honoured() {
        let rows: Vec<String> = (0..50).map(|index| format!("match {index}")).collect();
        let options = SearchOptions {
            limit: 5,
            ..Default::default()
        };
        let list = MatchList::run(&rows, "match", &options).expect("search");
        assert_eq!(list.len(), 5);
    }
}
