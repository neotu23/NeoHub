//! Removing secrets from anything that leaves the process.
//!
//! The rule from the Electron build: the UI may show a secret locally, but MCP responses,
//! log tails, search results and error messages must not carry one. Redaction is applied by
//! the MCP layer on the way out — never as an afterthought in each tool.

use std::collections::BTreeSet;

/// Placeholder written in place of a secret.
pub const MASK: &str = "***";

/// Which secrets to remove: the configured values plus anything that looks like ciphertext.
#[derive(Clone, Debug, Default)]
pub struct Redactor {
    secrets: BTreeSet<String>,
}

impl Redactor {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a literal secret (env value, MCP token) to scrub.
    pub fn add_secret(&mut self, secret: impl Into<String>) {
        let secret = secret.into();
        // Very short values would redact unrelated text; ignore them.
        if secret.len() >= 4 {
            self.secrets.insert(secret);
        }
    }

    pub fn is_empty(&self) -> bool {
        self.secrets.is_empty()
    }

    /// Scrub a string: literals first, then ciphertext-shaped and header-shaped values.
    pub fn scrub(&self, input: &str) -> String {
        let mut out = input.to_string();
        // Longest first: a secret that contains another must not leave a fragment behind.
        let mut secrets: Vec<&String> = self.secrets.iter().collect();
        secrets.sort_by_key(|secret| std::cmp::Reverse(secret.len()));
        for secret in secrets {
            if out.contains(secret.as_str()) {
                out = out.replace(secret.as_str(), MASK);
            }
        }
        out = scrub_ciphertext(&out);
        scrub_tokens(&out)
    }

    /// Scrub a JSON value recursively (MCP tool results are JSON).
    pub fn scrub_json(&self, value: &serde_json::Value) -> serde_json::Value {
        match value {
            serde_json::Value::String(text) => serde_json::Value::String(self.scrub(text)),
            serde_json::Value::Array(items) => {
                serde_json::Value::Array(items.iter().map(|item| self.scrub_json(item)).collect())
            }
            serde_json::Value::Object(map) => serde_json::Value::Object(
                map.iter()
                    .map(|(key, value)| (key.clone(), self.scrub_json(value)))
                    .collect(),
            ),
            other => other.clone(),
        }
    }
}

/// `enc1:…` (Electron safeStorage) and `nhk1:…` (native ciphertext) never leave the process.
fn scrub_ciphertext(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(position) = find_prefix(rest) {
        out.push_str(&rest[..position]);
        out.push_str(MASK);
        // Skip the ciphertext run (base64-ish characters up to a delimiter).
        let after = &rest[position..];
        let end = after
            .find(|c: char| c.is_whitespace() || c == '"' || c == ',' || c == '}')
            .unwrap_or(after.len());
        rest = &after[end..];
    }
    out.push_str(rest);
    out
}

fn find_prefix(input: &str) -> Option<usize> {
    [input.find("enc1:"), input.find("nhk1:")]
        .into_iter()
        .flatten()
        .min()
}

/// `Bearer <token>` in an echoed header or error message.
fn scrub_tokens(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(position) = rest.to_lowercase().find("bearer ") {
        let after = &rest[position + "bearer ".len()..];
        let end = after
            .find(|c: char| c.is_whitespace() || c == '"' || c == ',')
            .unwrap_or(after.len());
        if end == 0 {
            break;
        }
        out.push_str(&rest[..position]);
        out.push_str("Bearer ");
        out.push_str(MASK);
        rest = &after[end..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_secret_values_are_scrubbed_everywhere() {
        let mut redactor = Redactor::new();
        redactor.add_secret("hunter2");
        let text = "GET /x failed with password hunter2 and again hunter2";
        let scrubbed = redactor.scrub(text);
        assert!(!scrubbed.contains("hunter2"));
        assert_eq!(scrubbed.matches(MASK).count(), 2);
    }

    #[test]
    fn ciphertext_never_leaves_the_process() {
        let redactor = Redactor::new();
        let scrubbed = redactor.scrub(r#"{"value":"enc1:v10:AAAA","other":"nhk1:Zm9v"}"#);
        assert!(!scrubbed.contains("enc1:"));
        assert!(!scrubbed.contains("nhk1:"));
        assert!(scrubbed.contains(MASK));
    }

    #[test]
    fn bearer_tokens_are_masked_in_echoed_headers() {
        let redactor = Redactor::new();
        let scrubbed = redactor.scrub("Authorization: Bearer abcdef123456");
        assert_eq!(scrubbed, "Authorization: Bearer ***");
    }

    #[test]
    fn json_scrubbing_walks_objects_and_arrays() {
        let mut redactor = Redactor::new();
        redactor.add_secret("tops3cret");
        let value = serde_json::json!({
            "apps": [{"env": {"PASSWORD": "tops3cret"}, "note": "ok"}],
            "cipher": "enc1:xyz"
        });
        let scrubbed = redactor.scrub_json(&value);
        let text = scrubbed.to_string();
        assert!(!text.contains("tops3cret"));
        assert!(!text.contains("enc1:"));
        assert!(text.contains("\"note\":\"ok\""), "non-secrets survive");
    }

    #[test]
    fn a_secret_that_contains_another_leaves_no_fragment() {
        let mut redactor = Redactor::new();
        redactor.add_secret("abc");
        redactor.add_secret("abcdef");
        let scrubbed = redactor.scrub("value=abcdef");
        assert_eq!(scrubbed, format!("value={MASK}"), "no tail left behind");
    }

    #[test]
    fn very_short_values_are_not_registered() {
        let mut redactor = Redactor::new();
        redactor.add_secret("ab");
        assert!(
            redactor.is_empty(),
            "two characters would redact everything"
        );
        assert_eq!(redactor.scrub("abc"), "abc");
    }
}
