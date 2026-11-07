//! Toasts: the one place a transient message appears.
//!
//! The rule that shapes this module: **errors do not expire**. An error that disappears on its
//! own is an error the user never read, and the operations that produce one here (a failed save,
//! a refused database reset) are exactly the ones where being told matters. Information and
//! success messages expire, because they are confirmations of something that just visibly
//! happened.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// What kind of message this is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToastKind {
    Info,
    Success,
    Error,
}

impl ToastKind {
    /// Does this kind disappear by itself?
    ///
    /// Errors never do: see the module note.
    pub fn expires(self) -> bool {
        !matches!(self, ToastKind::Error)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            ToastKind::Info => "info",
            ToastKind::Success => "done",
            ToastKind::Error => "error",
        }
    }
}

/// One message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Toast {
    pub id: u64,
    pub kind: ToastKind,
    pub message: String,
    /// When it was pushed (only used for expiry).
    pub created: Instant,
}

/// Bounded queue of messages, newest last.
#[derive(Debug)]
pub struct Toasts {
    items: VecDeque<Toast>,
    capacity: usize,
    next_id: u64,
    /// How long an expiring message stays.
    lifetime: Duration,
}

impl Default for Toasts {
    fn default() -> Self {
        Toasts::new(4, Duration::from_secs(4))
    }
}

impl Toasts {
    pub fn new(capacity: usize, lifetime: Duration) -> Self {
        Toasts {
            items: VecDeque::with_capacity(capacity.max(1)),
            capacity: capacity.max(1),
            next_id: 1,
            lifetime,
        }
    }

    /// Add a message and return its id.
    ///
    /// When the queue is full the **oldest expiring** message is dropped first, so a burst of
    /// confirmations cannot push an error off the screen.
    pub fn push(&mut self, kind: ToastKind, message: impl Into<String>) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        let toast = Toast {
            id,
            kind,
            message: message.into(),
            created: Instant::now(),
        };
        if self.items.len() == self.capacity {
            match self
                .items
                .iter()
                .position(|existing| existing.kind.expires())
            {
                Some(index) => {
                    self.items.remove(index);
                }
                // Only errors are on screen: drop the oldest one anyway, otherwise the newest
                // message would never be visible.
                None => {
                    self.items.pop_front();
                }
            }
        }
        self.items.push_back(toast);
        id
    }

    pub fn messages(&self) -> Vec<&Toast> {
        self.items.iter().collect()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Remove messages whose lifetime is over. Returns how many went away.
    pub fn expire(&mut self, now: Instant) -> usize {
        let lifetime = self.lifetime;
        let before = self.items.len();
        self.items
            .retain(|toast| !toast.kind.expires() || now.duration_since(toast.created) < lifetime);
        before - self.items.len()
    }

    /// Remove one message (the user dismissed it).
    pub fn dismiss(&mut self, id: u64) -> bool {
        let before = self.items.len();
        self.items.retain(|toast| toast.id != id);
        self.items.len() != before
    }

    pub fn clear(&mut self) {
        self.items.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_keep_their_order_and_kind() {
        let mut toasts = Toasts::default();
        toasts.push(ToastKind::Info, "saved");
        toasts.push(ToastKind::Error, "save failed");
        let messages = toasts.messages();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].message, "saved");
        assert_eq!(messages[1].kind, ToastKind::Error);
    }

    #[test]
    fn errors_never_expire_but_confirmations_do() {
        let mut toasts = Toasts::new(4, Duration::from_millis(10));
        toasts.push(ToastKind::Success, "exported");
        toasts.push(ToastKind::Error, "export failed");
        // Expire with a timestamp well past the lifetime: build it from a pushed toast's clock.
        let later = Instant::now() + Duration::from_millis(50);
        let removed = toasts.expire(later);
        assert_eq!(removed, 1, "only the confirmation expires");
        let remaining = toasts.messages();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].kind, ToastKind::Error);
    }

    #[test]
    fn a_burst_of_confirmations_does_not_evict_an_error() {
        let mut toasts = Toasts::new(2, Duration::from_secs(4));
        toasts.push(ToastKind::Error, "database reset failed");
        toasts.push(ToastKind::Success, "one");
        toasts.push(ToastKind::Success, "two");
        let messages = toasts.messages();
        assert_eq!(messages.len(), 2);
        assert!(
            messages.iter().any(|toast| toast.kind == ToastKind::Error),
            "the error survives: {:?}",
            messages.iter().map(|t| &t.message).collect::<Vec<_>>()
        );
        assert_eq!(
            messages.last().unwrap().message,
            "two",
            "the newest is shown"
        );
    }

    #[test]
    fn only_errors_still_evict_when_the_queue_is_full_of_them() {
        let mut toasts = Toasts::new(2, Duration::from_secs(4));
        toasts.push(ToastKind::Error, "first");
        toasts.push(ToastKind::Error, "second");
        toasts.push(ToastKind::Error, "third");
        let messages = toasts.messages();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages.last().unwrap().message, "third");
        assert_eq!(messages[0].message, "second", "the oldest error goes");
    }

    #[test]
    fn dismissing_by_id_removes_exactly_one() {
        let mut toasts = Toasts::default();
        let first = toasts.push(ToastKind::Info, "one");
        let second = toasts.push(ToastKind::Info, "two");
        assert!(toasts.dismiss(second));
        assert_eq!(toasts.messages().len(), 1);
        assert_eq!(toasts.messages()[0].id, first);
        assert!(!toasts.dismiss(second), "dismissing twice is a no-op");
    }

    #[test]
    fn ids_are_unique_across_pushes() {
        let mut toasts = Toasts::default();
        let a = toasts.push(ToastKind::Info, "a");
        let b = toasts.push(ToastKind::Info, "b");
        assert_ne!(a, b);
    }

    #[test]
    fn a_zero_capacity_still_holds_one_message() {
        let mut toasts = Toasts::new(0, Duration::from_secs(1));
        toasts.push(ToastKind::Info, "only");
        assert_eq!(toasts.len(), 1);
    }
}
