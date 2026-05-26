//! Polling inventory watcher (P1.6 phase 1).
//!
//! Spawns a dedicated OS thread with a one-shot tokio runtime that calls
//! `openlogi_hid::enumerate` every `period` and forwards the result over an
//! unbounded mpsc to the GPUI thread. The GUI applies updates via
//! `AppState::refresh_inventories`.
//!
//! Polling beats hot-plug event registration on simplicity: HID transport
//! crates ship different listener APIs across platforms, and `async-hid 0.4`
//! does not expose any. A 2 s tick is cheap (one HID enumerate per cycle ≤
//! a few hundred milliseconds) and matches the human-perceptible reconnect
//! latency budget in PLAN.md.

use std::thread;
use std::time::Duration;

use openlogi_core::device::DeviceInventory;
use tokio::sync::mpsc;
use tracing::{debug, warn};

/// Spawn the watcher and return a receiver of inventory snapshots. The
/// channel is unbounded so a slow GUI thread cannot back-pressure the HID
/// poll loop into stalling on a real device disconnect.
///
/// Dropping the receiver shuts the watcher down: the next `send` fails and
/// the loop exits cleanly.
pub fn spawn(period: Duration) -> mpsc::UnboundedReceiver<Vec<DeviceInventory>> {
    let (tx, rx) = mpsc::unbounded_channel();
    let spawn_result = thread::Builder::new()
        .name("openlogi-inventory-watcher".into())
        .spawn(move || {
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    warn!(error = %e, "tokio runtime init failed; watcher exiting");
                    return;
                }
            };
            loop {
                let inv = match rt.block_on(openlogi_hid::enumerate()) {
                    Ok(inv) => inv,
                    Err(e) => {
                        warn!(error = ?e, "enumerate failed during watch tick");
                        Vec::new()
                    }
                };
                if tx.send(inv).is_err() {
                    debug!("inventory watcher receiver dropped — exiting");
                    return;
                }
                thread::sleep(period);
            }
        });
    if let Err(e) = spawn_result {
        // OS thread limits / fork failures are non-fatal: the GUI can run
        // with the initial enumeration snapshot, just without hot-plug
        // detection. The dropped sender means the receiver immediately
        // closes on its first recv() and the GUI loop falls through.
        warn!(error = %e, "could not spawn inventory watcher — auto-reconnect disabled");
    }
    rx
}
