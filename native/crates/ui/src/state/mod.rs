//! Interface state: persisted navigation plus the snapshot/event projection.

pub mod session;
pub mod store;

pub use session::{AppSort, GroupBy, Session, SessionStore, View};
pub use store::{AppCard, DashboardTotals, LOG_TAIL_BYTES, SidebarApp, SidebarSection, UiStore};
