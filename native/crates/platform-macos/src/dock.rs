//! Dock visibility.
//!
//! NeoHub keeps running with no window (the tray holds it). At that point the Dock icon is
//! misleading — clicking it would raise nothing — so it is switched off with
//! `NSApplicationActivationPolicy::Accessory` and switched back on before the window returns.
//!
//! Both calls must happen on the main thread, which is where the tray and GPUI live; the
//! functions assert that instead of silently doing nothing.

/// Set the Dock icon visibility. Returns the policy that is in force afterwards.
#[cfg(target_os = "macos")]
pub fn set_dock_visible(visible: bool) -> bool {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};

    let Some(marker) = MainThreadMarker::new() else {
        // Never change the activation policy off the main thread: AppKit ignores it and the
        // app can end up with a Dock icon it cannot control.
        eprintln!("[dock] set_dock_visible must run on the main thread");
        return current_dock_visible();
    };
    let app = NSApplication::sharedApplication(marker);
    let policy = if visible {
        NSApplicationActivationPolicy::Regular
    } else {
        NSApplicationActivationPolicy::Accessory
    };
    app.setActivationPolicy(policy);
    current_dock_visible()
}

/// Read the current policy back (so callers can verify the change instead of assuming it).
#[cfg(target_os = "macos")]
pub fn current_dock_visible() -> bool {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};

    let Some(marker) = MainThreadMarker::new() else {
        return true;
    };
    let app = NSApplication::sharedApplication(marker);
    app.activationPolicy() == NSApplicationActivationPolicy::Regular
}

#[cfg(not(target_os = "macos"))]
pub fn set_dock_visible(_visible: bool) -> bool {
    true
}

#[cfg(not(target_os = "macos"))]
pub fn current_dock_visible() -> bool {
    true
}

/// Apply the host's intent, returning whether the Dock is actually showing.
pub fn sync_dock(hidden: bool) -> bool {
    set_dock_visible(!hidden)
}
