//! Receiving `neohub://` links while the app is already running.
//!
//! `Info.plist` registration and `deep_link::parse` cover everything except one step: handing the
//! URL from LaunchServices into this process. On macOS that arrives as an Apple Event
//! (`kInternetEventClass` / `kAEGetURL`), and taking it means registering a target object with
//! `NSAppleEventManager` — which is why this file is unsafe Objective-C interop and nothing else.
//!
//! The probe rule: build and run this with a URL-printing handler *before* wiring it into the app.
//! The failure mode of getting it wrong is a crash the moment someone clicks a link, which no unit
//! test here can catch.

use objc2::rc::Retained;
use objc2::{AllocAnyThread, define_class, msg_send, sel};
use objc2_core_services::{AEEventClass, AEEventID, keyDirectObject};
use objc2_foundation::{NSAppleEventDescriptor, NSAppleEventManager, NSObject, NSString};
use std::sync::{Mutex, OnceLock};

/// `'GURL'` — the event class LaunchServices uses for URL opens.
///
/// The SDK spells these as C macros, which do not appear in the generated bindings, so they are
/// written out. Both are FourCharCode 'GURL'; the class says "internet event", the id says
/// "get URL".
const K_INTERNET_EVENT_CLASS: AEEventClass = u32::from_be_bytes(*b"GURL");
const K_AE_GET_URL: AEEventID = u32::from_be_bytes(*b"GURL");

/// Links received from the system, in arrival order.
static RECEIVED: OnceLock<Mutex<Vec<String>>> = OnceLock::new();

fn received() -> &'static Mutex<Vec<String>> {
    RECEIVED.get_or_init(|| Mutex::new(Vec::new()))
}

define_class!(
    // The handler has no state of its own: everything lands in `RECEIVED`, so the object can be
    // created once and forgotten.
    #[unsafe(super(NSObject))]
    #[name = "NeoHubURLEventHandler"]
    #[derive(Debug)]
    pub struct UrlEventHandler;

    impl UrlEventHandler {
        #[unsafe(method(handleGetURLEvent:withReplyEvent:))]
        fn handle_event(&self, event: &NSAppleEventDescriptor, _reply: &NSAppleEventDescriptor) {
            // `keyDirectObject` carries the URL for a `kAEGetURL` event.
            let Some(direct) = event.paramDescriptorForKeyword(keyDirectObject) else {
                return;
            };
            let Some(text) = direct.stringValue() else {
                return;
            };
            if let Ok(mut list) = received().lock() {
                list.push(text.to_string());
            }
        }
    }
);

impl UrlEventHandler {
    fn new() -> Retained<Self> {
        let this = Self::alloc();
        unsafe { msg_send![this, init] }
    }
}

/// Register the handler. Call once, on the main thread, after the app is up.
///
/// Returns the retained handler: dropping it while the manager still points at it would leave a
/// dangling target, which is exactly the crash this module warns about.
pub fn install() -> Result<Retained<UrlEventHandler>, String> {
    let handler = UrlEventHandler::new();
    let manager = NSAppleEventManager::sharedAppleEventManager();
    unsafe {
        manager.setEventHandler_andSelector_forEventClass_andEventID(
            &handler,
            sel!(handleGetURLEvent:withReplyEvent:),
            K_INTERNET_EVENT_CLASS,
            K_AE_GET_URL,
        );
    }
    Ok(handler)
}

/// Take everything received since the last call.
pub fn drain() -> Vec<String> {
    received()
        .lock()
        .map(|mut list| std::mem::take(&mut *list))
        .unwrap_or_default()
}

/// Only used by tests and the probe to prove the plumbing without a live Apple Event.
pub fn record_for_test(url: &str) {
    if let Ok(mut list) = received().lock() {
        list.push(url.to_string());
    }
}

#[allow(dead_code)]
fn unused_import_marker(_: &NSString) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_handler_class_is_registered_with_the_runtime() {
        // Building the class is the part that can fail silently at runtime rather than at compile
        // time: an unregistered class makes `setEventHandler:` store a nil target.
        let handler = UrlEventHandler::new();
        // Reaching this line at all proves the class registered with the runtime; an
        // unregistered class traps when the object is allocated.
        assert!(!format!("{handler:?}").is_empty());
    }

    #[test]
    fn draining_hands_over_what_arrived_and_empties() {
        let _ = drain(); // start clean; other tests may have pushed
        record_for_test("neohub://open?id=abc");
        assert_eq!(drain(), vec!["neohub://open?id=abc".to_string()]);
        assert!(drain().is_empty(), "a second drain has nothing left");
    }

    #[test]
    fn install_registers_without_crashing() {
        // The real value here is that registering on this machine does not trap; whether the
        // system then calls us can only be checked by clicking a link.
        let handler = install().expect("install");
        assert!(!format!("{handler:?}").is_empty());
    }
}
