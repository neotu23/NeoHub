//! One process-wide event router for every `muda` menu.
//!
//! `muda` keeps a **single** `MenuEvent` handler for the whole process. The status bar item and
//! the application menu are two different menus in the same process, so installing "one handler
//! per menu" would leave whichever was installed last as the only working one — the other's
//! clicks would vanish silently.
//!
//! This module owns that single handler. Each menu registers its rows under a source, and a
//! rebuild replaces only its own rows. The receiving end is one channel, so the caller drains
//! both menus from one place.

use crate::tray::TrayAction;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Mutex, OnceLock};

/// Which menu a row belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuSource {
    /// The status bar item.
    Tray,
    /// The application (top of screen) menu.
    AppMenu,
}

struct Router {
    sender: Sender<TrayAction>,
    routes: Vec<(MenuSource, muda::MenuId, TrayAction)>,
}

static ROUTER: OnceLock<Mutex<Router>> = OnceLock::new();
static RECEIVER: OnceLock<Mutex<Option<Receiver<TrayAction>>>> = OnceLock::new();

fn router() -> &'static Mutex<Router> {
    ROUTER.get_or_init(|| {
        let (sender, receiver) = channel();
        // The receiver is handed to the first caller via `take_receiver`; owned here until then.
        let _ = RECEIVER.set(Mutex::new(Some(receiver)));
        Mutex::new(Router {
            sender,
            routes: Vec::new(),
        })
    })
}

/// Take the receiving end. The first caller gets it; later callers get `None`.
///
/// Only one consumer is correct: actions from both menus arrive on the same channel, and two
/// consumers would steal clicks from each other.
pub fn take_receiver() -> Option<Receiver<TrayAction>> {
    let _ = router();
    RECEIVER
        .get()
        .and_then(|slot| slot.lock().ok())
        .and_then(|mut guard| guard.take())
}

/// The sending end, for menus that need to feed actions back.
pub fn sender() -> Sender<TrayAction> {
    router().lock().expect("menu router").sender.clone()
}

/// Replace the rows contributed by one source.
pub fn register(source: MenuSource, routing: Vec<(muda::MenuId, TrayAction)>) {
    let mut router = router().lock().expect("menu router");
    router.routes.retain(|(existing, _, _)| *existing != source);
    for (id, action) in routing {
        router.routes.push((source, id, action));
    }
    drop(router);
    ensure_handler();
}

/// Install the process-wide handler exactly once.
fn ensure_handler() {
    if !ROUTER.get().is_some_and(|_| true) {
        return;
    }
    // `set_event_handler` cannot tell us whether one is already installed, so a marker in the
    // router decides. Re-installing would be harmless but would re-allocate the closure.
    static INSTALLED: OnceLock<()> = OnceLock::new();
    INSTALLED.get_or_init(|| {
        muda::MenuEvent::set_event_handler(Some(|event: muda::MenuEvent| {
            let Some(router) = ROUTER.get() else { return };
            let Ok(router) = router.lock() else { return };
            if let Some((_, _, action)) = router.routes.iter().find(|(_, id, _)| *id == event.id) {
                let _ = router.sender.send(action.clone());
            }
        }));
    });
}

/// Rows currently routable (used by tests to prove a rebuild replaced rather than duplicated).
pub fn routes_for(source: MenuSource) -> Vec<TrayAction> {
    router()
        .lock()
        .expect("menu router")
        .routes
        .iter()
        .filter(|(candidate, _, _)| *candidate == source)
        .map(|(_, _, action)| action.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registering_a_source_replaces_only_its_own_rows() {
        let tray_id = muda::MenuId("tray-open".into());
        let app_id = muda::MenuId("app-settings".into());

        register(
            MenuSource::Tray,
            vec![(tray_id.clone(), TrayAction::Open(None))],
        );
        register(MenuSource::AppMenu, vec![(app_id, TrayAction::Quit)]);

        assert_eq!(routes_for(MenuSource::Tray).len(), 1);
        assert_eq!(routes_for(MenuSource::AppMenu), vec![TrayAction::Quit]);

        // A tray rebuild must not disturb the application menu.
        register(MenuSource::Tray, vec![(tray_id, TrayAction::StopAll)]);
        assert_eq!(routes_for(MenuSource::Tray), vec![TrayAction::StopAll]);
        assert_eq!(
            routes_for(MenuSource::AppMenu),
            vec![TrayAction::Quit],
            "the other menu keeps its rows"
        );
    }
}
