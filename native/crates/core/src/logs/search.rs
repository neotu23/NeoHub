//! Log search over the retained raw text.
//!
//! Search runs over **raw** output (escape sequences included) so a hit maps to a real byte
//! range; the UI shows the terminal's rendered text. Regexes come from users, so an invalid
//! pattern is an error, never a silent literal match.

use regex::{Regex, RegexBuilder};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchOptions {
    pub regex: bool,
    pub case_sensitive: bool,
    /// Hard cap on returned lines (per task).
    pub limit: usize,
}

impl Default for SearchOptions {
    fn default() -> Self {
        SearchOptions {
            regex: false,
            case_sensitive: false,
            limit: 200,
        }
    }
}

/// One matching line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineMatch {
    /// 1-based line number within the searched text.
    pub line_no: usize,
    pub line: String,
}

#[derive(Debug, thiserror::Error)]
pub enum SearchError {
    #[error("invalid regular expression: {0}")]
    InvalidRegex(String),
}

/// Search `text` line by line.
pub fn search_text(
    text: &str,
    query: &str,
    options: &SearchOptions,
) -> Result<Vec<LineMatch>, SearchError> {
    if query.is_empty() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    if options.regex {
        let pattern = RegexBuilder::new(query)
            .case_insensitive(!options.case_sensitive)
            .build()
            .map_err(|err| SearchError::InvalidRegex(err.to_string()))?;
        collect(text, options.limit, |line| pattern.is_match(line), &mut out);
    } else {
        let needle = if options.case_sensitive {
            query.to_string()
        } else {
            query.to_lowercase()
        };
        collect(
            text,
            options.limit,
            |line| {
                if options.case_sensitive {
                    line.contains(&needle)
                } else {
                    line.to_lowercase().contains(&needle)
                }
            },
            &mut out,
        );
    }
    Ok(out)
}

fn collect(text: &str, limit: usize, matches: impl Fn(&str) -> bool, out: &mut Vec<LineMatch>) {
    for (index, line) in text.lines().enumerate() {
        if matches(line) {
            out.push(LineMatch {
                line_no: index + 1,
                line: line.to_string(),
            });
            if out.len() >= limit {
                break;
            }
        }
    }
}

/// Compile a user regex once (readiness log matchers reuse this).
pub fn compile(pattern: &str, case_insensitive: bool) -> Result<Regex, SearchError> {
    RegexBuilder::new(pattern)
        .case_insensitive(case_insensitive)
        .build()
        .map_err(|err| SearchError::InvalidRegex(err.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &str = "booting\nlistening on 4319\n[fixture] tick 1\nError: boom\n";

    #[test]
    fn literal_search_is_case_insensitive_by_default() {
        let hits = search_text(TEXT, "tick", &SearchOptions::default()).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].line_no, 3);
        let upper = search_text(TEXT, "TICK", &SearchOptions::default()).unwrap();
        assert_eq!(upper.len(), 1);
    }

    #[test]
    fn case_sensitive_search_respects_the_flag() {
        let options = SearchOptions {
            case_sensitive: true,
            ..Default::default()
        };
        assert!(search_text(TEXT, "TICK", &options).unwrap().is_empty());
        assert_eq!(search_text(TEXT, "tick", &options).unwrap().len(), 1);
    }

    #[test]
    fn regex_search_uses_the_pattern() {
        let options = SearchOptions {
            regex: true,
            ..Default::default()
        };
        let hits = search_text(TEXT, r"tick \d+", &options).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].line, "[fixture] tick 1");
    }

    #[test]
    fn an_invalid_regex_is_an_error_not_a_literal() {
        let options = SearchOptions {
            regex: true,
            ..Default::default()
        };
        assert!(matches!(
            search_text(TEXT, "[unclosed", &options),
            Err(SearchError::InvalidRegex(_))
        ));
    }

    #[test]
    fn the_limit_is_enforced() {
        let many: String = (0..100).map(|i| format!("line {i}\n")).collect();
        let options = SearchOptions {
            limit: 5,
            ..Default::default()
        };
        assert_eq!(search_text(&many, "line", &options).unwrap().len(), 5);
    }

    #[test]
    fn an_empty_query_returns_nothing() {
        assert!(
            search_text(TEXT, "", &SearchOptions::default())
                .unwrap()
                .is_empty()
        );
    }
}
