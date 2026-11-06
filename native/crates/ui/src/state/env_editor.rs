//! Environment variable editor model.
//!
//! The rule this exists to enforce: **a masked value is never written back**. The core only
//! reports "this secret is set"; the editor keeps that as `Masked`, and a save sends `None`
//! for it, which the core interprets as "keep whatever is stored". Typing a new value turns
//! the row into `Plain`/`Secret` text; deleting the row removes it.
//!
//! Scope layering (global → app → task) is shown as data, not guessed: `effective()` returns
//! each key with the scope that wins.

use neohub_core::environment::parse_dotenv;
use neohub_core::service::{EnvScope, EnvVarInput, EnvVarView};
use neohub_domain::{AppId, TaskId};
use std::collections::BTreeMap;

/// What a row currently holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RowValue {
    /// A non-secret value the user can see and edit.
    Text(String),
    /// A secret whose text the core did not send; saving keeps it untouched.
    Masked,
    /// A secret the user just typed (will be encrypted by the core).
    NewSecret(String),
}

impl RowValue {
    pub fn display(&self) -> String {
        match self {
            RowValue::Text(value) => value.clone(),
            RowValue::Masked => "••••••".to_string(),
            RowValue::NewSecret(_) => "••••••".to_string(),
        }
    }
}

/// One editable row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnvRow {
    pub key: String,
    pub value: RowValue,
    pub enabled: bool,
    pub note: Option<String>,
}

impl EnvRow {
    fn to_input(&self) -> Option<EnvVarInput> {
        if self.key.trim().is_empty() {
            return None;
        }
        let (value, is_secret) = match &self.value {
            RowValue::Text(text) => (Some(text.clone()), false),
            RowValue::Masked => (None, true),
            RowValue::NewSecret(text) => (Some(text.clone()), true),
        };
        Some(EnvVarInput {
            key: self.key.trim().to_string(),
            value,
            is_secret,
            enabled: self.enabled,
            note: self.note.clone(),
        })
    }
}

/// Effective value of one key after layering.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EffectiveVar {
    pub key: String,
    pub value: String,
    pub scope: &'static str,
    pub is_secret: bool,
}

/// The editor for one scope.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnvEditor {
    scope: EnvScope,
    rows: Vec<EnvRow>,
    baseline: Vec<EnvRow>,
}

impl EnvEditor {
    pub fn new(scope: EnvScope, vars: Vec<EnvVarView>) -> Self {
        let rows: Vec<EnvRow> = vars
            .into_iter()
            .map(|var| EnvRow {
                key: var.key,
                value: match (var.is_secret, var.value) {
                    (true, _) => RowValue::Masked,
                    (false, Some(value)) => RowValue::Text(value),
                    (false, None) => RowValue::Text(String::new()),
                },
                enabled: var.enabled,
                note: var.note,
            })
            .collect();
        EnvEditor {
            scope,
            baseline: rows.clone(),
            rows,
        }
    }

    pub fn scope(&self) -> &EnvScope {
        &self.scope
    }

    pub fn rows(&self) -> &[EnvRow] {
        &self.rows
    }

    pub fn is_dirty(&self) -> bool {
        self.rows != self.baseline
    }

    /// Add an empty row (the "＋" button).
    pub fn add_row(&mut self) {
        self.rows.push(EnvRow {
            key: String::new(),
            value: RowValue::Text(String::new()),
            enabled: true,
            note: None,
        });
    }

    pub fn remove_row(&mut self, index: usize) {
        if index < self.rows.len() {
            self.rows.remove(index);
        }
    }

    pub fn set_key(&mut self, index: usize, key: impl Into<String>) {
        if let Some(row) = self.rows.get_mut(index) {
            row.key = key.into();
        }
    }

    /// Typing into the value cell. A masked secret becomes an editable secret.
    pub fn set_value(&mut self, index: usize, value: impl Into<String>) {
        let value = value.into();
        if let Some(row) = self.rows.get_mut(index) {
            row.value = match row.value {
                RowValue::Masked => RowValue::NewSecret(value),
                RowValue::NewSecret(_) if value.is_empty() => RowValue::Masked,
                RowValue::NewSecret(_) => RowValue::NewSecret(value),
                RowValue::Text(_) => RowValue::Text(value),
            };
        }
    }

    pub fn toggle_enabled(&mut self, index: usize) {
        if let Some(row) = self.rows.get_mut(index) {
            row.enabled = !row.enabled;
        }
    }

    pub fn toggle_secret(&mut self, index: usize) {
        if let Some(row) = self.rows.get_mut(index) {
            row.value = match &row.value {
                RowValue::Text(value) => {
                    // Turning a visible value into a secret hides it immediately.
                    if value.is_empty() {
                        RowValue::NewSecret(String::new())
                    } else {
                        RowValue::NewSecret(value.clone())
                    }
                }
                RowValue::Masked | RowValue::NewSecret(_) => RowValue::Text(String::new()),
            };
        }
    }

    pub fn set_note(&mut self, index: usize, note: impl Into<String>) {
        if let Some(row) = self.rows.get_mut(index) {
            let note = note.into();
            row.note = (!note.trim().is_empty()).then(|| note.trim().to_string());
        }
    }

    /// Paste a `.env` blob: existing keys are updated, new ones appended.
    pub fn paste_dotenv(&mut self, blob: &str) -> usize {
        let parsed = parse_dotenv(blob);
        let mut updated = 0;
        for (key, value) in parsed {
            match self.rows.iter_mut().find(|row| row.key == key) {
                Some(row) => {
                    row.value = match row.value {
                        RowValue::Masked => RowValue::NewSecret(value),
                        RowValue::NewSecret(_) => RowValue::NewSecret(value),
                        RowValue::Text(_) => RowValue::Text(value),
                    };
                }
                None => {
                    self.rows.push(EnvRow {
                        key,
                        value: RowValue::Text(value),
                        enabled: true,
                        note: None,
                    });
                }
            }
            updated += 1;
        }
        updated
    }

    /// Payload for `SetEnvVars`: masked rows carry `None` and are left untouched by the core.
    pub fn to_inputs(&self) -> Vec<EnvVarInput> {
        self.rows.iter().filter_map(EnvRow::to_input).collect()
    }

    /// After a successful save the core returns the new view; adopt it as the new baseline.
    pub fn ack(&mut self, vars: Vec<EnvVarView>) {
        let scope = self.scope.clone();
        *self = EnvEditor::new(scope, vars);
    }
}

/// Merge the three scopes for display. Later scopes win (global → app → task), matching the
/// spawn-time layering, and the winning scope is reported so the UI can show the source.
pub fn effective(
    global: &[EnvVarView],
    app: &[EnvVarView],
    task: &[EnvVarView],
) -> Vec<EffectiveVar> {
    let mut merged: BTreeMap<String, EffectiveVar> = BTreeMap::new();
    for (scope_name, vars) in [("全局", global), ("应用", app), ("任务", task)] {
        for var in vars {
            if !var.enabled {
                continue;
            }
            let value = match &var.value {
                Some(value) => value.clone(),
                // The view never sees secret text; show a mask instead of an empty value.
                None => "••••••".to_string(),
            };
            merged.insert(
                var.key.clone(),
                EffectiveVar {
                    key: var.key.clone(),
                    value,
                    scope: scope_name,
                    is_secret: var.is_secret,
                },
            );
        }
    }
    merged.into_values().collect()
}

/// Scope helper for the UI's three tabs.
pub fn scope_for(tab: ScopeTab, app: Option<&AppId>, task: Option<&TaskId>) -> Option<EnvScope> {
    match tab {
        ScopeTab::Global => Some(EnvScope::Global),
        ScopeTab::App => app.map(|app_id| EnvScope::App(app_id.clone())),
        ScopeTab::Task => task.map(|task_id| EnvScope::Task(task_id.clone())),
    }
}

/// The three tabs of the env editor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScopeTab {
    Global,
    App,
    Task,
}

impl ScopeTab {
    pub fn label(self) -> &'static str {
        match self {
            ScopeTab::Global => "全局",
            ScopeTab::App => "应用",
            ScopeTab::Task => "任务",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(key: &str, value: Option<&str>, is_secret: bool, enabled: bool) -> EnvVarView {
        EnvVarView {
            key: key.into(),
            value: value.map(str::to_string),
            is_secret,
            enabled,
            note: None,
        }
    }

    /// The regression test for "mask characters must never be written back".
    #[test]
    fn a_masked_secret_is_sent_as_none_not_as_the_mask() {
        let editor = EnvEditor::new(
            EnvScope::Global,
            vec![view("DB_PASSWORD", None, true, true)],
        );
        assert_eq!(editor.rows()[0].value.display(), "••••••");
        let inputs = editor.to_inputs();
        assert_eq!(inputs.len(), 1);
        assert_eq!(inputs[0].key, "DB_PASSWORD");
        assert_eq!(
            inputs[0].value, None,
            "the mask must never be sent as the value"
        );
        assert!(inputs[0].is_secret);
    }

    #[test]
    fn typing_a_new_secret_replaces_the_mask_once() {
        let mut editor = EnvEditor::new(
            EnvScope::App(AppId::from_existing("app")),
            vec![view("TOKEN", None, true, true)],
        );
        editor.set_value(0, "hunter2");
        assert_eq!(
            editor.rows()[0].value,
            RowValue::NewSecret("hunter2".into())
        );
        let inputs = editor.to_inputs();
        assert_eq!(inputs[0].value.as_deref(), Some("hunter2"));
        // Clearing the field again returns to "keep what is stored".
        editor.set_value(0, "");
        assert_eq!(editor.rows()[0].value, RowValue::Masked);
        assert_eq!(editor.to_inputs()[0].value, None);
    }

    #[test]
    fn plain_values_round_trip_and_are_not_marked_secret() {
        let mut editor = EnvEditor::new(
            EnvScope::Global,
            vec![view("PORT", Some("4319"), false, true)],
        );
        editor.set_value(0, "4320");
        let inputs = editor.to_inputs();
        assert_eq!(inputs[0].value.as_deref(), Some("4320"));
        assert!(!inputs[0].is_secret);
    }

    #[test]
    fn toggling_secret_hides_a_visible_value() {
        let mut editor = EnvEditor::new(
            EnvScope::Global,
            vec![view("API_KEY", Some("abc"), false, true)],
        );
        editor.toggle_secret(0);
        assert_eq!(editor.rows()[0].value, RowValue::NewSecret("abc".into()));
        assert_eq!(editor.rows()[0].value.display(), "••••••");
        assert!(editor.to_inputs()[0].is_secret);
    }

    #[test]
    fn adding_removing_and_toggling_rows_tracks_dirtiness() {
        let mut editor = EnvEditor::new(EnvScope::Global, vec![view("A", Some("1"), false, true)]);
        assert!(!editor.is_dirty());
        editor.add_row();
        assert!(editor.is_dirty());
        editor.set_key(1, "B");
        editor.set_value(1, "2");
        assert_eq!(editor.to_inputs().len(), 2);
        editor.remove_row(1);
        assert!(!editor.is_dirty(), "back to the saved state");
        editor.toggle_enabled(0);
        assert!(editor.is_dirty());
        assert!(!editor.to_inputs()[0].enabled);
    }

    #[test]
    fn pasting_a_dotenv_updates_existing_keys_and_appends_new_ones() {
        let mut editor = EnvEditor::new(
            EnvScope::Global,
            vec![
                view("EXISTING", Some("old"), false, true),
                view("SECRET", None, true, true),
            ],
        );
        let updated = editor.paste_dotenv("EXISTING=new\nNEW_KEY=1\nSECRET=fresh\n# comment\n");
        assert_eq!(updated, 3);
        assert_eq!(editor.rows()[0].value, RowValue::Text("new".into()));
        assert_eq!(
            editor.rows()[1].value,
            RowValue::NewSecret("fresh".into()),
            "a pasted secret stays secret"
        );
        assert_eq!(editor.rows()[2].key, "NEW_KEY");
        assert_eq!(editor.rows().len(), 3, "no duplicate rows");
    }

    #[test]
    fn empty_keys_are_dropped_from_the_payload() {
        let mut editor = EnvEditor::new(EnvScope::Global, vec![]);
        editor.add_row();
        editor.set_value(0, "orphan");
        assert!(
            editor.to_inputs().is_empty(),
            "a row without a key is not a variable"
        );
    }

    #[test]
    fn the_effective_value_reports_which_scope_wins() {
        let global = vec![
            view("API_URL", Some("global"), false, true),
            view("ONLY_GLOBAL", Some("g"), false, true),
        ];
        let app = vec![view("API_URL", Some("app"), false, true)];
        let task = vec![
            view("API_URL", Some("task"), false, true),
            view("TASK_SECRET", None, true, true),
        ];
        let merged = effective(&global, &app, &task);
        let by_key: BTreeMap<_, _> = merged.iter().map(|var| (var.key.clone(), var)).collect();
        assert_eq!(by_key["API_URL"].value, "task");
        assert_eq!(by_key["API_URL"].scope, "任务");
        assert_eq!(by_key["ONLY_GLOBAL"].scope, "全局");
        assert_eq!(
            by_key["TASK_SECRET"].value, "••••••",
            "a secret shows as a mask in the preview"
        );
    }

    #[test]
    fn disabled_variables_do_not_participate_in_the_effective_view() {
        let global = vec![view("A", Some("1"), false, false)];
        let app = vec![view("A", Some("2"), false, true)];
        let merged = effective(&global, &app, &[]);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].value, "2");
        assert_eq!(merged[0].scope, "应用");
    }
}
