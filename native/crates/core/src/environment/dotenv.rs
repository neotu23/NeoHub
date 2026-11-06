//! `.env` parsing — one implementation, shared by the spawn path and the editors.
//!
//! Ported from `src/shared/dotenv.ts`, which the Electron build already unified for the
//! same reason: three drifting copies meant a pasted `.env` parsed differently than the
//! same file at spawn time. Behaviour kept on purpose:
//!
//! * `export FOO=bar` prefixes are stripped before key validation;
//! * keys must match `[A-Za-z_][A-Za-z0-9_]*`, otherwise the line is skipped;
//! * double-quoted values may span lines (PEM blocks, embedded JSON) and expand
//!   `\n`, `\r`, `\t`; anything after the closing quote is dropped;
//! * single-quoted values are literal;
//! * unquoted values drop an inline ` #` comment;
//! * an unterminated `"` degrades to a single-line value instead of swallowing the file.

use std::collections::BTreeMap;

/// Parse a `.env` blob into a key→value map.
pub fn parse_dotenv(src: &str) -> BTreeMap<String, String> {
    let lines: Vec<Vec<char>> = src
        .split('\n')
        .map(|line| line.trim_end_matches('\r').chars().collect())
        .collect();

    let mut out = BTreeMap::new();
    let mut i = 0usize;
    while i < lines.len() {
        let line: String = lines[i].iter().collect();
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            i += 1;
            continue;
        }
        let without_export = trimmed
            .strip_prefix("export ")
            .map(str::trim_start)
            .unwrap_or(trimmed);
        let Some(eq) = without_export.find('=') else {
            i += 1;
            continue;
        };
        let key = without_export[..eq].trim();
        if key.is_empty() || !is_valid_key(key) {
            i += 1;
            continue;
        }
        let rest: Vec<char> = without_export[eq + 1..].trim_start().chars().collect();
        if rest.is_empty() {
            out.insert(key.to_string(), String::new());
            i += 1;
            continue;
        }

        let value = if rest[0] == '"' {
            // Double-quoted: may continue over several physical lines.
            let start_line = i;
            let mut acc = rest.clone();
            let mut close = find_closing_double_quote(&acc, 1);
            while close.is_none() && i + 1 < lines.len() {
                i += 1;
                acc.push('\n');
                acc.extend(lines[i].iter());
                close = find_closing_double_quote(&acc, 1);
            }
            match close {
                Some(close) => {
                    let quoted: String = acc[1..close].iter().collect();
                    expand_escapes(&quoted)
                }
                None => {
                    // Unterminated quote: degrade to a single-line value and rewind so the
                    // following lines still parse as their own variables.
                    i = start_line;
                    let raw: String = rest[1..].iter().collect();
                    strip_inline_comment(&raw)
                }
            }
        } else if rest[0] == '\'' {
            let close = rest.iter().skip(1).position(|c| *c == '\'');
            match close {
                Some(offset) => rest[1..1 + offset].iter().collect(),
                None => rest[1..].iter().collect(),
            }
        } else {
            let raw: String = rest.iter().collect();
            strip_inline_comment(&raw)
        };

        out.insert(key.to_string(), value);
        i += 1;
    }
    out
}

/// Heuristic used to mask a variable by default (`SECRET|TOKEN|PASSWORD|KEY|PRIVATE`).
pub fn is_secret_key(key: &str) -> bool {
    let upper = key.to_uppercase();
    ["SECRET", "TOKEN", "PASSWORD", "KEY", "PRIVATE"]
        .iter()
        .any(|needle| upper.contains(needle))
}

fn is_valid_key(key: &str) -> bool {
    let mut chars = key.chars();
    match chars.next() {
        Some(first) if first.is_ascii_alphabetic() || first == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// First unescaped `"` at or after `from` (an even run of backslashes leaves it unescaped,
/// so PEM blocks and embedded JSON survive).
fn find_closing_double_quote(chars: &[char], from: usize) -> Option<usize> {
    let mut i = from;
    while i < chars.len() {
        if chars[i] == '"' {
            let mut backslashes = 0usize;
            let mut j = i;
            while j > 0 && chars[j - 1] == '\\' {
                backslashes += 1;
                j -= 1;
            }
            if backslashes.is_multiple_of(2) {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

fn strip_inline_comment(raw: &str) -> String {
    let trimmed = raw.trim_end();
    match trimmed.find(" #") {
        Some(idx) => trimmed[..idx].trim_end().to_string(),
        None => trimmed.to_string(),
    }
}

fn expand_escapes(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.peek() {
                Some('n') => {
                    chars.next();
                    out.push('\n');
                }
                Some('r') => {
                    chars.next();
                    out.push('\r');
                }
                Some('t') => {
                    chars.next();
                    out.push('\t');
                }
                _ => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_common_shapes() {
        let parsed = parse_dotenv(
            "# comment\n\nFOO=bar\nexport BAZ=qux\nEMPTY=\nQUOTED=\"a b\"\nSINGLE='x y'\n",
        );
        assert_eq!(parsed.get("FOO").unwrap(), "bar");
        assert_eq!(
            parsed.get("BAZ").unwrap(),
            "qux",
            "export prefix is stripped"
        );
        assert_eq!(parsed.get("EMPTY").unwrap(), "");
        assert_eq!(parsed.get("QUOTED").unwrap(), "a b");
        assert_eq!(
            parsed.get("SINGLE").unwrap(),
            "x y",
            "single quotes stay literal"
        );
    }

    #[test]
    fn double_quotes_expand_escapes_and_drop_trailing_comments() {
        let parsed = parse_dotenv("MULTI=\"line1\\nline2\" # trailing\n");
        assert_eq!(parsed.get("MULTI").unwrap(), "line1\nline2");
        let tabbed = parse_dotenv("TAB=\"a\\tb\"\n");
        assert_eq!(tabbed.get("TAB").unwrap(), "a\tb");
    }

    #[test]
    fn a_multiline_double_quoted_value_survives() {
        let parsed =
            parse_dotenv("PRIVATE_KEY=\"-----BEGIN-----\nabc\ndef\n-----END-----\"\nNEXT=1\n");
        assert_eq!(
            parsed.get("PRIVATE_KEY").unwrap(),
            "-----BEGIN-----\nabc\ndef\n-----END-----"
        );
        assert_eq!(
            parsed.get("NEXT").unwrap(),
            "1",
            "parsing resumes after the block"
        );
    }

    #[test]
    fn an_unterminated_quote_degrades_to_one_line() {
        let parsed = parse_dotenv("BROKEN=\"no end\nAFTER=ok\n");
        assert_eq!(parsed.get("BROKEN").unwrap(), "no end");
        assert_eq!(
            parsed.get("AFTER").unwrap(),
            "ok",
            "a malformed line must not swallow the rest of the file"
        );
    }

    #[test]
    fn unquoted_values_drop_inline_comments_and_keep_inner_spaces() {
        let parsed = parse_dotenv("URL=http://x/y # note\nMSG=hello world\n");
        assert_eq!(parsed.get("URL").unwrap(), "http://x/y");
        assert_eq!(parsed.get("MSG").unwrap(), "hello world");
    }

    #[test]
    fn invalid_keys_and_lines_are_skipped() {
        let parsed = parse_dotenv("1BAD=no\nnot a var\nOK=1\n");
        assert!(!parsed.contains_key("1BAD"));
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed.get("OK").unwrap(), "1");
    }

    #[test]
    fn secret_heuristic_matches_the_legacy_regex() {
        for key in [
            "API_SECRET",
            "GH_TOKEN",
            "DB_PASSWORD",
            "PRIVATE_KEY",
            "apiKey",
        ] {
            assert!(is_secret_key(key), "{key} should be treated as a secret");
        }
        for key in ["PORT", "NODE_ENV", "DEMO_FLAG"] {
            assert!(
                !is_secret_key(key),
                "{key} should not be treated as a secret"
            );
        }
    }
}
