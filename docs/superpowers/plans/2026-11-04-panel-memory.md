# Release panel memory on close

**Goal:** Destroy the panel renderer on close while keeping the menu bar, project processes, logs, and main-process services alive.

**Approved design:** The conversation selected immediate panel release over hiding the window. Reopen through the menu bar or a system launch, restore navigation and bounds, and read live state from the backend. Explicit Quit retains the existing graceful shutdown.

**Architecture:** Allow Electron's normal window close to release the renderer. Hide the Dock only after the main window actually closes and only when a menu bar icon exists. Persist small navigation preferences separately from runtime data.

**Constraints:** No new dependencies; preserve tray-disabled behavior; do not restart services when recreating a window. Work in the current user workspace and leave changes uncommitted for review.

## Task 1: Window lifecycle

- [x] Add regression tests exercising main-process close/reopen, menu bar disabled, and explicit quit.
- [x] Run tests against the old implementation and confirm close still retains the window.
- [x] Replace hide-on-close with normal close, clear renderer subscriptions on destruction, and retain Dock restoration/single-instance entry points.
- [x] Verify lifecycle and existing tray tests.

## Task 2: Panel restoration

- [x] Test restoring project/view/task selection after creating a fresh renderer store, invalid persisted data, and projects/tasks removed while the panel was closed.
- [x] Persist only navigation preferences when they change; retain authoritative runtime snapshots in main.
- [x] Remove the forced dashboard reset at renderer boot. Preserve pending debounced saves when a native window closes.
- [x] Update the README and settings description.
- [x] Run type checking, full tests, lint, build, and attempt an Electron smoke test.

## Review focus

- Closing a full-screen panel must not leave an empty hidden window alive.
- Multiple reopen requests must reuse the newly created window.
- A quit confirmation cancelled by the user must leave close-to-menu-bar working.
- Removed projects/tasks must not restore dangling selections.
- Pending task edits and backend log buffers must survive the close/reopen cycle.

## Verification and review

- Lifecycle and restoration tests failed against the previous hide-only implementation and now pass.
- Independent review identified save ordering, drafts during closure, recovery after a temporary hang, deep-link delivery, tray-quit cancellation, and update-banner restoration. The implementation addresses those findings.
- Task saves are acknowledged before panel destruction. Environment-variable drafts use the existing discard confirmation; cancellation keeps the panel open. The panel is inert while closing, while confirmation dialogs remain interactive through their body portal.
- Menu actions, notification clicks and deep links wait for the renderer's explicit ready acknowledgement instead of a fixed delay. Downloaded updates and current download progress are replayed from main.
- Type checking and lint pass. Production build succeeds.
- The full default suite encountered environment restrictions: `AppOrchestrator.e2e.test.ts` and `HttpReadiness.test.ts` cannot listen on localhost (`EPERM`); `RestartWatcher.test.ts` hits `fs.watch` limits (`EMFILE`) and passes with `CHOKIDAR_USEPOLLING=1`.
- Electron itself exits with `SIGABRT` in this restricted runtime, including `electron --version`; native window behavior and actual memory savings require a local run outside this environment.
