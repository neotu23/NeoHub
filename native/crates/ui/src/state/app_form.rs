//! The app configuration form: the 9 fields the MCP tool already edits, now editable in the
//! window.
//!
//! Same shape as `TaskForm` for a reason: the form owns strings (what the user types), the
//! patch is built on demand and validated field by field, and a field that was not touched is
//! **absent** from the patch so saving cannot clobber a concurrent change from MCP or another
//! window.

use crate::state::text_input::{KeyOutcome, TextField};
use neohub_core::catalog::UpdateAppInput;
use neohub_domain::{App, NodeVersionPref};
use std::collections::BTreeMap;

/// The text fields, in tab order. Toggles are not text, so Tab skips them.
pub const TEXT_FIELDS: [&str; 6] = [
    "name",
    "folder",
    "tags",
    "watch_globs",
    "port_hint",
    "node_version",
];

/// Everything the form shows. Lists are edited as text (comma separated for tags, one glob per
/// line) because a chip editor is more UI than this build needs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AppFormFields {
    pub name: String,
    pub folder: String,
    pub tags: String,
    pub watch_globs: String,
    pub port_hint: String,
    pub node_version: String,
    pub auto_start: bool,
    pub auto_restart_on_change: bool,
}

/// Field-level errors, keyed by field name so the view can highlight the right row.
pub type FormErrors = BTreeMap<&'static str, String>;

/// The form plus the values it started from.
#[derive(Clone, Debug)]
pub struct AppForm {
    app_id: String,
    baseline: AppFormFields,
    fields: AppFormFields,
    errors: FormErrors,
    /// Buffer for the field that has focus. One buffer (not one per field) because only one
    /// field takes keys at a time, which is what makes Tab handling trivial.
    text: TextField,
    /// Index into [`TEXT_FIELDS`].
    focused: usize,
}

impl AppForm {
    /// Open the form for one app.
    pub fn from_app(app: &App) -> Self {
        let fields = AppFormFields {
            name: app.name.clone(),
            folder: app.folder.clone().unwrap_or_default(),
            tags: app.tags.join(", "),
            watch_globs: app.watch_globs.join("\n"),
            port_hint: app
                .port_hint
                .map(|port| port.to_string())
                .unwrap_or_default(),
            node_version: match &app.node_version_pref {
                NodeVersionPref::Auto => "auto".to_string(),
                NodeVersionPref::System => "system".to_string(),
                NodeVersionPref::Explicit { version } => version.clone(),
            },
            auto_start: app.auto_start,
            auto_restart_on_change: app.auto_restart_on_change,
        };
        // `TextField::new` takes the initial text, not an id; the name is loaded by focus.
        let text = TextField::new(fields.name.clone());
        AppForm {
            app_id: app.id.as_str().to_string(),
            baseline: fields.clone(),
            fields,
            errors: FormErrors::new(),
            text,
            focused: 0,
        }
    }

    pub fn app_id(&self) -> &str {
        &self.app_id
    }

    pub fn fields(&self) -> &AppFormFields {
        &self.fields
    }

    pub fn fields_mut(&mut self) -> &mut AppFormFields {
        &mut self.fields
    }

    pub fn errors(&self) -> &FormErrors {
        &self.errors
    }

    /// Which field is being edited, by name.
    pub fn focused_field(&self) -> &'static str {
        TEXT_FIELDS[self.focused.min(TEXT_FIELDS.len() - 1)]
    }

    /// Current text buffer (what the caret is in).
    pub fn text(&self) -> &TextField {
        &self.text
    }

    /// Move focus to a field, loading its current value into the buffer.
    pub fn focus_field(&mut self, field: &str) {
        let Some(index) = TEXT_FIELDS.iter().position(|candidate| *candidate == field) else {
            return;
        };
        self.focused = index;
        self.text.set_text(self.value_of(field).to_string());
    }

    fn value_of(&self, field: &str) -> &str {
        match field {
            "name" => &self.fields.name,
            "folder" => &self.fields.folder,
            "tags" => &self.fields.tags,
            "watch_globs" => &self.fields.watch_globs,
            "port_hint" => &self.fields.port_hint,
            "node_version" => &self.fields.node_version,
            _ => "",
        }
    }

    /// Feed a keystroke to the focused field.
    ///
    /// Tab / Shift-Tab move between fields and carry the value along; Escape closes the panel
    /// (the caller decides what that means); Enter is a no-op because the form is saved with
    /// the button, not per field.
    pub fn apply_key(&mut self, key: &str, characters: Option<&str>) -> KeyOutcome {
        match key {
            "tab" => {
                self.commit_text();
                self.focused = (self.focused + 1) % TEXT_FIELDS.len();
                let field = self.focused_field();
                self.text.set_text(self.value_of(field).to_string());
                KeyOutcome::Changed
            }
            "escape" => KeyOutcome::Escape,
            "enter" => KeyOutcome::Submit,
            _ => {
                let outcome = self.text.apply_key(key, characters);
                if outcome == KeyOutcome::Changed {
                    self.commit_text();
                }
                outcome
            }
        }
    }

    /// Push the buffer into the focused field (and clear its error: the user is fixing it).
    fn commit_text(&mut self) {
        let field = self.focused_field();
        let value = self.text.text().to_string();
        match field {
            "name" => self.fields.name = value,
            "folder" => self.fields.folder = value,
            "tags" => self.fields.tags = value,
            "watch_globs" => self.fields.watch_globs = value,
            "port_hint" => self.fields.port_hint = value,
            "node_version" => self.fields.node_version = value,
            _ => return,
        }
        self.errors.remove(field);
    }

    /// Set one field, clearing its error: the user is fixing it.
    pub fn set(&mut self, field: &'static str, value: String) {
        match field {
            "name" => self.fields.name = value,
            "folder" => self.fields.folder = value,
            "tags" => self.fields.tags = value,
            "watch_globs" => self.fields.watch_globs = value,
            "port_hint" => self.fields.port_hint = value,
            "node_version" => self.fields.node_version = value,
            _ => return,
        }
        self.errors.remove(field);
    }

    pub fn set_toggle(&mut self, field: &'static str, value: bool) {
        match field {
            "auto_start" => self.fields.auto_start = value,
            "auto_restart_on_change" => self.fields.auto_restart_on_change = value,
            _ => {}
        }
    }

    /// Anything to save?
    pub fn is_dirty(&self) -> bool {
        self.fields != self.baseline
    }

    /// Build the patch. Only fields that differ from the baseline are included, so a save
    /// cannot overwrite a change someone else made to a field this form never touched.
    pub fn to_patch(&self) -> Result<UpdateAppInput, FormErrors> {
        let mut errors = FormErrors::new();
        let mut patch = UpdateAppInput::default();

        let name = self.fields.name.trim();
        if name.is_empty() {
            errors.insert("name", "名称不能为空".to_string());
        } else if name != self.baseline.name.as_str() {
            patch.name = Some(name.to_string());
        }

        let folder = self.fields.folder.trim();
        let baseline_folder = self.baseline.folder.trim();
        if folder != baseline_folder {
            // An emptied field means "no folder", which is a real value (`Some(None)`), not
            // "leave it alone".
            patch.folder = Some((!folder.is_empty()).then(|| folder.to_string()));
        }

        if self.fields.tags != self.baseline.tags {
            let tags = split_list(&self.fields.tags, ',');
            if let Some(duplicate) = first_duplicate(&tags) {
                errors.insert("tags", format!("标签重复：{duplicate}"));
            } else {
                patch.tags = Some(tags);
            }
        }

        if self.fields.watch_globs != self.baseline.watch_globs {
            let globs = self
                .fields
                .watch_globs
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>();
            if let Some(bad) = globs.iter().find(|glob| glob.starts_with('/')) {
                errors.insert("watch_globs", format!("请用相对路径：{bad}"));
            } else {
                patch.watch_globs = Some(globs);
            }
        }

        if self.fields.port_hint != self.baseline.port_hint {
            match self.fields.port_hint.trim() {
                "" => patch.port_hint = Some(None),
                raw => match raw.parse::<u16>() {
                    Ok(0) => {
                        errors.insert("port_hint", "端口必须大于 0".to_string());
                    }
                    Ok(port) => patch.port_hint = Some(Some(port)),
                    Err(_) => {
                        errors.insert("port_hint", "端口必须是 1-65535 的数字".to_string());
                    }
                },
            }
        }

        if self.fields.node_version != self.baseline.node_version {
            match self.fields.node_version.trim() {
                "auto" => patch.node_version_pref = Some(NodeVersionPref::Auto),
                "system" => patch.node_version_pref = Some(NodeVersionPref::System),
                "" => {
                    errors.insert("node_version", "填 auto、system 或具体版本号".to_string());
                }
                version => {
                    // Loose check: `v` prefix is common, anything non-numeric is a typo.
                    let trimmed = version.strip_prefix('v').unwrap_or(version);
                    if trimmed.chars().next().is_some_and(|c| c.is_ascii_digit())
                        && trimmed.chars().all(|c| c.is_ascii_digit() || c == '.')
                    {
                        patch.node_version_pref = Some(NodeVersionPref::Explicit {
                            version: trimmed.to_string(),
                        });
                    } else {
                        errors.insert("node_version", format!("版本号无法识别：{version}"));
                    }
                }
            }
        }

        if self.fields.auto_start != self.baseline.auto_start {
            patch.auto_start = Some(self.fields.auto_start);
        }
        if self.fields.auto_restart_on_change != self.baseline.auto_restart_on_change {
            patch.auto_restart_on_change = Some(self.fields.auto_restart_on_change);
        }

        if errors.is_empty() {
            Ok(patch)
        } else {
            Err(errors)
        }
    }

    /// Record a save that the core rejected, keeping the user's input on screen.
    pub fn apply_error(&mut self, field: &'static str, message: String) {
        self.errors.insert(field, message);
    }

    /// The core accepted the save: the new values become the baseline.
    pub fn commit(&mut self) {
        self.baseline = self.fields.clone();
        self.errors.clear();
    }

    /// Re-read the app after a save so the form shows what was actually stored.
    pub fn reset_from_app(&mut self, app: &App) {
        *self = AppForm::from_app(app);
    }
}

/// Split a user-typed list, trimming and dropping empties.
fn split_list(input: &str, separator: char) -> Vec<String> {
    input
        .split(separator)
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_string)
        .collect()
}

fn first_duplicate(items: &[String]) -> Option<String> {
    let mut seen = std::collections::HashSet::new();
    items
        .iter()
        .find(|item| !seen.insert(item.to_lowercase()))
        .cloned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use neohub_domain::{AppId, PackageManager};

    fn app() -> App {
        App {
            id: AppId::from_existing("app-1"),
            name: "web".into(),
            path: "/tmp/web".into(),
            color: "#89b4fa".into(),
            icon: None,
            node_version_pref: NodeVersionPref::Explicit {
                version: "20.11.0".into(),
            },
            package_manager: Some(PackageManager::Pnpm),
            default_script: Some("dev".into()),
            custom_command: None,
            working_dir: "/tmp/web".into(),
            auto_restart_on_change: true,
            auto_start: false,
            watch_globs: vec!["src/**/*.ts".into()],
            port_hint: Some(3000),
            tags: vec!["api".into(), "web".into()],
            folder: Some("Work".into()),
            last_started_at: None,
            last_exit_code: None,
            created_at: 0,
            updated_at: 0,
        }
    }

    #[test]
    fn the_form_opens_with_the_stored_values() {
        let form = AppForm::from_app(&app());
        assert_eq!(form.fields().name, "web");
        assert_eq!(form.fields().folder, "Work");
        assert_eq!(form.fields().tags, "api, web");
        assert_eq!(form.fields().watch_globs, "src/**/*.ts");
        assert_eq!(form.fields().port_hint, "3000");
        assert_eq!(form.fields().node_version, "20.11.0");
        assert!(form.fields().auto_restart_on_change);
        assert!(!form.fields().auto_start);
        assert!(!form.is_dirty(), "an untouched form is not dirty");
    }

    #[test]
    fn an_untouched_form_produces_an_empty_patch() {
        let form = AppForm::from_app(&app());
        let patch = form.to_patch().expect("valid");
        assert!(patch.name.is_none());
        assert!(patch.folder.is_none(), "unchanged folder must not be sent");
        assert!(patch.tags.is_none());
        assert!(patch.watch_globs.is_none());
        assert!(patch.port_hint.is_none());
        assert!(patch.node_version_pref.is_none());
        assert!(patch.auto_start.is_none());
        assert!(patch.auto_restart_on_change.is_none());
    }

    #[test]
    fn only_the_edited_fields_reach_the_patch() {
        let mut form = AppForm::from_app(&app());
        form.set("name", "web-app".into());
        assert!(form.is_dirty());
        let patch = form.to_patch().expect("valid");
        assert_eq!(patch.name.as_deref(), Some("web-app"));
        assert!(patch.tags.is_none(), "tags were not touched");
        assert!(patch.port_hint.is_none());
    }

    #[test]
    fn clearing_a_field_writes_an_explicit_null() {
        let mut form = AppForm::from_app(&app());
        form.set("folder", "  ".into());
        form.set("port_hint", String::new());
        let patch = form.to_patch().expect("valid");
        assert_eq!(
            patch.folder,
            Some(None),
            "an emptied folder means 'no folder'"
        );
        assert_eq!(patch.port_hint, Some(None));
    }

    #[test]
    fn lists_are_parsed_from_what_the_user_types() {
        let mut form = AppForm::from_app(&app());
        form.set("tags", " api ,  api, web ,,".into());
        assert!(
            form.to_patch().is_err(),
            "duplicates are rejected, not silently merged"
        );
        form.set("tags", "api, web, cli".into());
        let patch = form.to_patch().expect("valid");
        assert_eq!(
            patch.tags,
            Some(vec!["api".into(), "web".into(), "cli".into()])
        );

        form.set("watch_globs", " src/**/*.ts\n\n tests/**/*.ts \n".into());
        let patch = form.to_patch().expect("valid");
        assert_eq!(
            patch.watch_globs,
            Some(vec!["src/**/*.ts".into(), "tests/**/*.ts".into()]),
            "blank lines are dropped"
        );
    }

    #[test]
    fn absolute_globs_are_rejected() {
        let mut form = AppForm::from_app(&app());
        form.set("watch_globs", "/Users/me/web/src/**".into());
        let errors = form
            .to_patch()
            .expect_err("absolute globs are relative to the app root");
        assert!(errors.contains_key("watch_globs"));
    }

    #[test]
    fn node_version_accepts_the_three_forms() {
        let mut form = AppForm::from_app(&app());
        form.set("node_version", "auto".into());
        assert_eq!(
            form.to_patch().expect("valid").node_version_pref,
            Some(NodeVersionPref::Auto)
        );
        form.set("node_version", "system".into());
        assert_eq!(
            form.to_patch().expect("valid").node_version_pref,
            Some(NodeVersionPref::System)
        );
        form.set("node_version", "v18.20.4".into());
        assert_eq!(
            form.to_patch().expect("valid").node_version_pref,
            Some(NodeVersionPref::Explicit {
                version: "18.20.4".into()
            }),
            "a leading v is normalised away"
        );
        form.set("node_version", "latest".into());
        assert!(form.to_patch().unwrap_err().contains_key("node_version"));
    }

    #[test]
    fn invalid_values_are_reported_per_field() {
        let mut form = AppForm::from_app(&app());
        form.set("name", "   ".into());
        form.set("port_hint", "70000".into());
        let errors = form.to_patch().expect_err("invalid");
        assert!(errors.contains_key("name"));
        assert!(errors.contains_key("port_hint"));

        form.set("port_hint", "0".into());
        assert!(form.to_patch().unwrap_err().contains_key("port_hint"));
        form.set("port_hint", "8080".into());
        // `name` is still blank, so the form is still invalid - fixing one field must not
        // report the whole form as clean.
        assert!(form.to_patch().unwrap_err().contains_key("name"));
        form.set("name", "web".into());
        assert!(form.to_patch().is_ok(), "all fields valid again");
    }

    #[test]
    fn toggles_are_only_sent_when_flipped() {
        let mut form = AppForm::from_app(&app());
        form.set_toggle("auto_start", true);
        let patch = form.to_patch().expect("valid");
        assert_eq!(patch.auto_start, Some(true));
        assert!(
            patch.auto_restart_on_change.is_none(),
            "the untouched toggle stays out of the patch"
        );
    }

    #[test]
    fn typing_lands_in_the_focused_field() {
        let mut form = AppForm::from_app(&app());
        form.focus_field("name");
        assert_eq!(
            form.text().text(),
            "web",
            "focusing loads the current value"
        );
        // Clear it, then type a new name.
        for _ in 0..3 {
            form.apply_key("backspace", None);
        }
        for character in ["w", "e", "b", "2"] {
            form.apply_key(character, Some(character));
        }
        assert_eq!(
            form.fields().name,
            "web2",
            "the buffer is pushed on every change"
        );
        assert_eq!(
            form.to_patch().expect("valid").name.as_deref(),
            Some("web2")
        );
    }

    #[test]
    fn tab_moves_between_fields_and_carries_the_value() {
        let mut form = AppForm::from_app(&app());
        form.focus_field("name");
        assert_eq!(form.focused_field(), "name");
        form.apply_key("tab", None);
        assert_eq!(form.focused_field(), "folder");
        assert_eq!(
            form.text().text(),
            "Work",
            "the next field's value is loaded"
        );
        // Wrap-around after the last field.
        for _ in 0..5 {
            form.apply_key("tab", None);
        }
        assert_eq!(form.focused_field(), "name");
        assert_eq!(form.text().text(), "web");
    }

    #[test]
    fn an_edit_clears_only_that_fields_error() {
        let mut form = AppForm::from_app(&app());
        form.apply_error("name", "名称冲突".into());
        form.apply_error("port_hint", "端口无效".into());
        form.focus_field("name");
        form.apply_key("x", Some("x"));
        assert!(
            !form.errors().contains_key("name"),
            "editing fixes the error"
        );
        assert!(
            form.errors().contains_key("port_hint"),
            "an unrelated error stays"
        );
    }

    #[test]
    fn escape_and_enter_are_reported_not_swallowed() {
        let mut form = AppForm::from_app(&app());
        assert_eq!(form.apply_key("escape", None), KeyOutcome::Escape);
        assert_eq!(form.apply_key("enter", None), KeyOutcome::Submit);
        assert_eq!(
            form.apply_key("left", None),
            KeyOutcome::Ignored,
            "arrows with no room to move change nothing"
        );
    }

    #[test]
    fn a_committed_form_is_clean_and_a_rejected_save_keeps_the_text() {
        let mut form = AppForm::from_app(&app());
        form.set("name", "renamed".into());
        form.apply_error("name", "名称冲突".into());
        assert_eq!(form.fields().name, "renamed", "input survives a rejection");
        assert!(form.errors().contains_key("name"));

        form.commit();
        assert!(!form.is_dirty());
        assert!(form.errors().is_empty());
        assert_eq!(form.fields().name, "renamed");
    }
}
