//! Interface state: persisted navigation plus the snapshot/event projection.

pub mod app_form;
pub mod command_labels;
pub mod command_palette;
pub mod edit_session;
pub mod env_editor;
pub mod session;
pub mod settings_form;
pub mod sparkline;
pub mod store;
pub mod task_form;
pub mod text_input;
pub mod toast;

pub use edit_session::{CloseOutcome, Draft, ExternalOutcome, Revision, SaveBarrier, SaveRequest};
pub use env_editor::{EffectiveVar, EnvEditor, EnvRow, RowValue, ScopeTab, effective, scope_for};
pub use session::{AppSort, GroupBy, Session, SessionStore, View};
pub use settings_form::{NumberField, SettingsForm, pending_native_fields};
pub use store::{AppCard, DashboardTotals, LOG_TAIL_BYTES, SidebarApp, SidebarSection, UiStore};
pub use task_form::{Field, FieldError, ReadinessChoice, TaskForm, TaskFormFields};
pub use text_input::{KeyOutcome, TextField};
