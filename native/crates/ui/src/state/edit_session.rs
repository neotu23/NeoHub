//! Editing drafts with revisions and a close barrier.
//!
//! Three failure modes this exists to prevent, all of which the Electron build had to fix:
//!
//! 1. **A refresh overwriting what the user is typing.** A snapshot arrives (because MCP or
//!    the monitor changed something) while a field is dirty — the local draft must win and be
//!    flagged as a conflict, not silently replaced.
//! 2. **A stale save overwriting a newer edit.** The save that was sent carries the revision
//!    it was built from; if the user typed again in the meantime, the acknowledgement must
//!    not clear the draft.
//! 3. **Losing input when the window closes.** `prepare_close` collects every pending edit; a
//!    failure keeps the window and the text.

use std::collections::BTreeMap;
use std::fmt;

/// Monotonic revision of one draft.
pub type Revision = u64;

/// Result of applying an external value to a draft.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExternalOutcome {
    /// Nothing local was pending: the draft took the new value.
    Applied,
    /// The user had unsaved edits: the local value stays, the external one is kept aside.
    PreservedLocal,
}

/// One editable entity (an app, a task, the settings block, …).
pub struct Draft<T: Clone + PartialEq> {
    entity: String,
    local: T,
    baseline: T,
    revision: Revision,
    /// Value that arrived from outside while the draft was dirty.
    conflict: Option<T>,
    /// Last save failure, shown next to the entity until dismissed.
    error: Option<String>,
}

impl<T: Clone + PartialEq> Draft<T> {
    pub fn new(entity: impl Into<String>, baseline: T) -> Self {
        Draft {
            entity: entity.into(),
            local: baseline.clone(),
            baseline,
            revision: 0,
            conflict: None,
            error: None,
        }
    }

    pub fn entity(&self) -> &str {
        &self.entity
    }

    pub fn local(&self) -> &T {
        &self.local
    }

    pub fn local_mut(&mut self) -> &mut T {
        &mut self.local
    }

    pub fn revision(&self) -> Revision {
        self.revision
    }

    pub fn conflict(&self) -> Option<&T> {
        self.conflict.as_ref()
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn clear_error(&mut self) {
        self.error = None;
    }

    /// True when the local value differs from the last saved value.
    pub fn is_dirty(&self) -> bool {
        self.local != self.baseline
    }

    /// Apply a user edit. Any edit bumps the revision, even if the value returns to the
    /// baseline: a pending save built from an older revision must stay stale.
    pub fn edit(&mut self, edit: impl FnOnce(&mut T)) {
        edit(&mut self.local);
        self.revision += 1;
        self.error = None;
    }

    /// Adopt a value from a snapshot.
    pub fn apply_external(&mut self, external: T) -> ExternalOutcome {
        if self.is_dirty() {
            self.conflict = Some(external);
            ExternalOutcome::PreservedLocal
        } else {
            self.local = external.clone();
            self.baseline = external;
            self.conflict = None;
            ExternalOutcome::Applied
        }
    }

    /// Accept the external value, discarding the local edits (explicit user choice).
    pub fn take_conflict(&mut self) -> Option<T> {
        let external = self.conflict.take()?;
        self.local = external.clone();
        self.baseline = external;
        self.error = None;
        Some(self.local.clone())
    }

    /// Build a save request for the current draft, if it is dirty.
    pub fn take_save(&self) -> Option<SaveRequest<T>> {
        if !self.is_dirty() {
            return None;
        }
        Some(SaveRequest {
            entity: self.entity.clone(),
            payload: self.local.clone(),
            revision: self.revision,
        })
    }

    /// The save succeeded and was built from the *current* revision: adopt it as baseline.
    pub fn ack(&mut self, saved: T, revision: Revision) {
        if revision == self.revision {
            self.baseline = saved;
            self.error = None;
        } else {
            // A newer edit arrived while this save was in flight. The stored value becomes
            // the baseline (that *is* what the database now holds), but the local revision
            // keeps climbing so the next flush carries strictly newer text. Never rewind
            // `revision` here: doing so would let a stale save look newer than it is.
            self.baseline = saved;
        }
    }

    /// The save failed: keep the text, surface the error.
    pub fn reject(&mut self, error: impl Into<String>) {
        self.error = Some(error.into());
    }
}

/// A pending write.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SaveRequest<T> {
    pub entity: String,
    pub payload: T,
    /// Draft revision this payload was built from.
    pub revision: Revision,
}

/// Outcome of "the window is closing".
pub enum CloseOutcome<T: Clone + PartialEq> {
    /// Nothing is pending: the window may go away.
    Ready,
    /// These writes must land first.
    Pending(Vec<SaveRequest<T>>),
    /// A previous save failed: keep the window and show why.
    Blocked(Vec<String>),
}

/// Collects pending edits for the close handshake.
#[derive(Default)]
pub struct SaveBarrier {
    failures: BTreeMap<String, String>,
}

impl SaveBarrier {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_failure(&mut self, entity: impl Into<String>, message: impl Into<String>) {
        self.failures.insert(entity.into(), message.into());
    }

    pub fn clear_failure(&mut self, entity: &str) {
        self.failures.remove(entity);
    }

    pub fn failures(&self) -> &BTreeMap<String, String> {
        &self.failures
    }

    /// Decide whether the window may close, and which writes must run first.
    pub fn prepare_close<T: Clone + PartialEq>(&self, drafts: &[&Draft<T>]) -> CloseOutcome<T> {
        if !self.failures.is_empty() {
            return CloseOutcome::Blocked(self.failures.values().cloned().collect());
        }
        let pending: Vec<SaveRequest<T>> = drafts
            .iter()
            .filter_map(|draft| draft.take_save())
            .collect();
        if pending.is_empty() {
            CloseOutcome::Ready
        } else {
            CloseOutcome::Pending(pending)
        }
    }
}

impl fmt::Debug for SaveBarrier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SaveBarrier")
            .field("failures", &self.failures)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Debug, PartialEq, Eq)]
    struct Fields {
        name: String,
        command: String,
    }

    fn draft() -> Draft<Fields> {
        Draft::new(
            "task-1",
            Fields {
                name: "dev".into(),
                command: "pnpm run dev".into(),
            },
        )
    }

    #[test]
    fn editing_marks_the_draft_dirty_and_bumps_the_revision() {
        let mut draft = draft();
        assert!(!draft.is_dirty());
        draft.edit(|fields| fields.command = "pnpm run start".into());
        assert!(draft.is_dirty());
        assert_eq!(draft.revision(), 1);
        let save = draft.take_save().expect("dirty drafts produce a save");
        assert_eq!(save.revision, 1);
        assert_eq!(save.entity, "task-1");
    }

    #[test]
    fn an_external_refresh_cannot_overwrite_a_dirty_draft() {
        let mut draft = draft();
        draft.edit(|fields| fields.name = "mine".into());
        let outcome = draft.apply_external(Fields {
            name: "theirs".into(),
            command: "something else".into(),
        });
        assert_eq!(outcome, ExternalOutcome::PreservedLocal);
        assert_eq!(draft.local().name, "mine", "the user's text survives");
        assert_eq!(
            draft.conflict().map(|c| c.name.as_str()),
            Some("theirs"),
            "the external value is kept for an explicit choice"
        );
    }

    #[test]
    fn a_clean_draft_follows_external_changes() {
        let mut draft = draft();
        let outcome = draft.apply_external(Fields {
            name: "renamed".into(),
            command: "x".into(),
        });
        assert_eq!(outcome, ExternalOutcome::Applied);
        assert_eq!(draft.local().name, "renamed");
        assert!(!draft.is_dirty());
    }

    #[test]
    fn taking_the_conflict_value_discards_the_local_edit() {
        let mut draft = draft();
        draft.edit(|fields| fields.name = "mine".into());
        draft.apply_external(Fields {
            name: "theirs".into(),
            command: "c".into(),
        });
        let taken = draft.take_conflict().expect("a conflict was recorded");
        assert_eq!(taken.name, "theirs");
        assert!(!draft.is_dirty());
    }

    #[test]
    fn a_stale_save_cannot_clear_a_newer_edit() {
        let mut draft = draft();
        draft.edit(|fields| fields.command = "cmd-A".into()); // revision 1
        let sent = draft.take_save().expect("save");
        // The user keeps typing while the save is in flight.
        draft.edit(|fields| fields.command = "cmd-B".into()); // revision 2
        draft.ack(sent.payload.clone(), sent.revision);
        assert!(
            draft.is_dirty(),
            "the newer edit is still pending after the stale save lands"
        );
        assert_eq!(draft.local().command, "cmd-B", "the user's newer text wins");
        let next = draft.take_save().expect("a follow-up save is required");
        assert_eq!(next.payload.command, "cmd-B");
    }

    #[test]
    fn a_failed_save_keeps_the_text_and_the_error() {
        let mut draft = draft();
        draft.edit(|fields| fields.command = "broken".into());
        let sent = draft.take_save().expect("save");
        draft.reject("command must not be empty");
        assert!(
            draft.is_dirty(),
            "the draft stays dirty so the user can retry"
        );
        assert_eq!(draft.local().command, "broken");
        assert_eq!(draft.error(), Some("command must not be empty"));
        // Retrying clears the error but keeps the payload.
        draft.edit(|fields| fields.command = "fixed".into());
        assert!(draft.error().is_none());
        let retry = draft.take_save().expect("retry");
        assert_eq!(retry.payload.command, "fixed");
        let _ = sent;
    }

    #[test]
    fn the_barrier_reports_pending_writes_and_failures() {
        let mut first = draft();
        first.edit(|fields| fields.name = "a".into());
        let clean = draft();
        let barrier = SaveBarrier::new();
        match barrier.prepare_close(&[&first, &clean]) {
            CloseOutcome::Pending(saves) => assert_eq!(saves.len(), 1),
            other => panic!(
                "expected pending saves, got {:?}",
                std::mem::discriminant(&other)
            ),
        }

        let mut failing = SaveBarrier::new();
        failing.record_failure("task-1", "disk full");
        match failing.prepare_close(&[&first]) {
            CloseOutcome::Blocked(reasons) => assert_eq!(reasons, vec!["disk full".to_string()]),
            _ => panic!("a recorded failure must block the close"),
        }
        failing.clear_failure("task-1");
        assert!(matches!(
            failing.prepare_close(&[&clean]),
            CloseOutcome::Ready
        ));
    }
}
