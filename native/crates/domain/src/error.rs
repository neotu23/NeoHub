//! Domain errors.
//!
//! Every error carries enough information for the caller to point at a field or an
//! entity: the UI highlights the input, MCP returns a validation error, and neither
//! leaves a half-applied transaction behind (`native/docs/interfaces.md` §6).

use crate::ids::{AppId, TaskId};

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum DomainError {
    #[error("{entity} not found: {id}")]
    NotFound { entity: &'static str, id: String },

    #[error("{field}: {message}")]
    InvalidInput {
        field: &'static str,
        message: String,
    },

    #[error("path does not exist or is not a directory: {path}")]
    PathMissing { path: String },

    #[error("this folder is already registered: {path}")]
    DuplicatePath { path: String, existing: AppId },

    #[error("dependency cycle between tasks: {tasks:?}")]
    DependencyCycle { tasks: Vec<TaskId> },

    #[error("task {task_id} depends on unknown task {missing}")]
    UnknownDependency { task_id: TaskId, missing: TaskId },

    #[error("an app cannot depend on itself")]
    SelfDependency { task_id: TaskId },

    #[error("a task named \"{name}\" already exists in this app")]
    DuplicateTaskName { name: String },

    #[error("one-shot tasks must use readiness \"exit\"")]
    OneShotRequiresExitReadiness,

    #[error("task \"{name}\" is still required by: {dependents:?}")]
    TaskHasDependents {
        name: String,
        dependents: Vec<String>,
    },

    #[error("cannot remove an app while it is {state}")]
    AppIsLive { state: &'static str },

    #[error("{key} cannot be decrypted with the available key material")]
    SecretUnavailable { key: String },
}

pub type Result<T> = std::result::Result<T, DomainError>;
