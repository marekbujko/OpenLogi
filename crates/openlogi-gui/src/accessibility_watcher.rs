//! macOS Accessibility-permission watcher.
//!
//! Polls [`openlogi_hook::Hook::has_accessibility`] on a dedicated OS thread
//! and forwards the trust state over an mpsc whenever it changes (plus an
//! initial value). The GUI uses it to (a) show/hide the permission gate,
//! (b) install the OS mouse hook the moment the user grants access (no
//! restart needed), and (c) drop the hook + re-show the gate if access is
//! revoked while running.
//!
//! Non-macOS platforms have no Accessibility concept — `has_accessibility`
//! returns `true` there — so the watcher emits a single `true` and exits.

use std::thread;
use std::time::Duration;

use openlogi_hook::Hook;
use tokio::sync::mpsc;
use tracing::{debug, warn};

/// Start the watcher and return a receiver of trust-state transitions. The
/// initial value is pushed immediately so the consumer doesn't need a
/// separate query.
///
/// Dropping the receiver shuts the watcher down: the next `send` fails and
/// the loop exits.
pub fn spawn(period: Duration) -> mpsc::UnboundedReceiver<bool> {
    let (tx, rx) = mpsc::unbounded_channel();

    // Non-macOS: permission is always "granted"; emit once and stop. The
    // initial send can't fail (receiver is still held by the caller).
    if !cfg!(target_os = "macos") {
        let _ = tx.send(true);
        let _ = period;
        return rx;
    }

    let spawn_result = thread::Builder::new()
        .name("openlogi-accessibility-watcher".into())
        .spawn(move || {
            let mut last: Option<bool> = None;
            loop {
                let granted = Hook::has_accessibility();
                if last != Some(granted) {
                    debug!(granted, "accessibility trust changed");
                    if tx.send(granted).is_err() {
                        debug!("accessibility watcher receiver dropped — exiting");
                        return;
                    }
                    last = Some(granted);
                }
                // Keep polling in *both* directions for the whole session.
                // Revocation does not relaunch the app, so the consumer must
                // hear about it to tear the hook down and re-show the gate;
                // a later re-grant then reinstalls the hook.
                thread::sleep(period);
            }
        });
    if let Err(e) = spawn_result {
        warn!(error = %e, "could not spawn accessibility watcher — gate won't auto-dismiss");
    }
    rx
}
