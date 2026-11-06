//! Interface state: persisted navigation plus the snapshot/event projection.

pub mod edit_session;
pub mod session;
pub mod store;
pub mod task_form;
pub mod text_input;

pub use edit_session::{CloseOutcome, Draft, ExternalOutcome, Revision, SaveBarrier, SaveRequest};
pub use session::{AppSort, GroupBy, Session, SessionStore, View};
pub use store::{AppCard, DashboardTotals, LOG_TAIL_BYTES, SidebarApp, SidebarSection, UiStore};
pub use task_form::{Field, FieldError, ReadinessChoice, TaskForm, TaskFormFields};
pub use text_input::{KeyOutcome, TextField};
