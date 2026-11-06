//! Interface strings, ported from the Electron renderer's `i18n/locales/*.ts`.
//!
//! Embedded at compile time (no runtime file lookup), with a deliberate fallback chain:
//! requested language → English → the key itself. A missing translation must never render an
//! empty label, and the fallback is visible in tests.
//!
//! `system` resolves to the OS locale: Chinese when the environment says so, otherwise
//! English — the same two languages the Electron build shipped.

use neohub_domain::LanguageSetting;
use std::collections::BTreeMap;

const EN_JSON: &str = include_str!("../locales/en.json");
const ZH_JSON: &str = include_str!("../locales/zh-CN.json");

/// Loaded string tables.
pub struct Locales {
    en: BTreeMap<String, String>,
    zh: BTreeMap<String, String>,
}

impl Locales {
    pub fn embedded() -> Self {
        Locales {
            en: parse(EN_JSON),
            zh: parse(ZH_JSON),
        }
    }

    /// Resolve the language, following the OS locale for `system`.
    pub fn resolve(setting: LanguageSetting) -> LanguageSetting {
        match setting {
            LanguageSetting::System => {
                let locale = std::env::var("LANG")
                    .or_else(|_| std::env::var("LC_ALL"))
                    .unwrap_or_default()
                    .to_lowercase();
                if locale.contains("zh") {
                    LanguageSetting::ZhCn
                } else {
                    LanguageSetting::En
                }
            }
            explicit => explicit,
        }
    }

    /// Look up `key`, falling back to English, then to the key itself.
    pub fn text(&self, language: LanguageSetting, key: &str) -> String {
        let language = Self::resolve(language);
        let table = match language {
            LanguageSetting::ZhCn => &self.zh,
            _ => &self.en,
        };
        table
            .get(key)
            .or_else(|| self.en.get(key))
            .cloned()
            .unwrap_or_else(|| key.to_string())
    }

    /// `text` with `{placeholder}` substitutions.
    pub fn text_with(
        &self,
        language: LanguageSetting,
        key: &str,
        args: &[(&str, String)],
    ) -> String {
        let mut out = self.text(language, key);
        for (name, value) in args {
            out = out.replace(&format!("{{{name}}}"), value);
        }
        out
    }

    /// True when `key` exists in both tables (used by the parity test).
    pub fn has_both(&self, key: &str) -> bool {
        self.en.contains_key(key) && self.zh.contains_key(key)
    }

    pub fn keys(&self) -> impl Iterator<Item = &String> {
        self.en.keys()
    }
}

fn parse(json: &str) -> BTreeMap<String, String> {
    serde_json::from_str(json).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_key_exists_in_both_languages() {
        let locales = Locales::embedded();
        let missing: Vec<&String> = locales
            .keys()
            .filter(|key| !locales.has_both(key))
            .collect();
        assert!(missing.is_empty(), "untranslated keys: {missing:?}");
    }

    #[test]
    fn strings_resolve_in_the_requested_language() {
        let locales = Locales::embedded();
        assert_eq!(
            locales.text(LanguageSetting::En, "nav.dashboard"),
            "Dashboard"
        );
        assert_eq!(
            locales.text(LanguageSetting::ZhCn, "nav.dashboard"),
            "仪表盘"
        );
    }

    #[test]
    fn an_unknown_key_falls_back_to_the_key_itself() {
        let locales = Locales::embedded();
        assert_eq!(
            locales.text(LanguageSetting::ZhCn, "no.such.key"),
            "no.such.key",
            "a missing key renders visibly, never as an empty label"
        );
    }

    #[test]
    fn placeholders_are_substituted() {
        let locales = Locales::embedded();
        let text = locales.text_with(
            LanguageSetting::En,
            "tasks.count",
            &[("count", "3".into()), ("running", "1".into())],
        );
        assert_eq!(text, "3 tasks · 1 running");
    }

    #[test]
    fn system_language_follows_the_locale() {
        // The resolver reads LANG/LC_ALL; whatever this machine says, the result must be one
        // of the two shipped languages (never `system`).
        let resolved = Locales::resolve(LanguageSetting::System);
        assert!(matches!(
            resolved,
            LanguageSetting::En | LanguageSetting::ZhCn
        ));
        assert_eq!(
            Locales::resolve(LanguageSetting::ZhCn),
            LanguageSetting::ZhCn
        );
    }
}
