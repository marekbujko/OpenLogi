#![allow(
    dead_code,
    reason = "full schema parsed; only a subset is read in v0.0.1 (display name + files-on-disk lookup)"
)]

//! Parses the `index.json` shipped by assets.openlogi.org.
//!
//! Schema mirrors the file the `stage_assets.py` helper emits:
//!
//! ```json
//! {
//!   "schema_version": 1,
//!   "devices": {
//!     "<depot>": {
//!       "modelId": "6b023",
//!       "displayName": "MX Master 3",
//!       "type": "MOUSE",
//!       "asset_path": "v1/devices/mx_master_3/",
//!       "files": [{ "name": "front_core.png", "sha256": "...", "bytes": 388329 }]
//!     }
//!   }
//! }
//! ```

use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Index {
    pub schema_version: u32,
    pub devices: HashMap<String, DeviceEntry>,
}

#[derive(Debug, Deserialize)]
pub struct DeviceEntry {
    #[serde(rename = "modelId")]
    pub model_id: String,
    #[serde(rename = "displayName")]
    pub display_name: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub asset_path: String,
    pub files: Vec<FileEntry>,
}

#[derive(Debug, Deserialize)]
pub struct FileEntry {
    pub name: String,
    pub sha256: String,
    pub bytes: u64,
}

impl Index {
    pub fn load_from(path: &Path) -> anyhow::Result<Self> {
        let bytes = std::fs::read(path)?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    /// Find the depot whose `modelId` matches `model_id` exactly.
    pub fn find_by_model_id(&self, model_id: &str) -> Option<(&str, &DeviceEntry)> {
        self.devices
            .iter()
            .find(|(_, entry)| entry.model_id.eq_ignore_ascii_case(model_id))
            .map(|(depot, entry)| (depot.as_str(), entry))
    }

    /// Find the depot whose `modelId` ends with `suffix` (case-insensitive).
    ///
    /// Used as a fallback when the strict `ext + bolt_pid` formatting
    /// doesn't line up — Logi's registry stores e.g. `"2b042"` for the
    /// MX Master 4 even though HID++ DeviceInformation reports `ext=01`
    /// on the same device. Matching on the trailing bolt PID is still
    /// unambiguous in practice because Logitech reserves PID ranges per
    /// product family.
    pub fn find_by_model_id_suffix(&self, suffix: &str) -> Option<(&str, &DeviceEntry)> {
        let suffix_lower = suffix.to_ascii_lowercase();
        self.devices
            .iter()
            .find(|(_, entry)| {
                entry
                    .model_id
                    .to_ascii_lowercase()
                    .ends_with(&suffix_lower)
            })
            .map(|(depot, entry)| (depot.as_str(), entry))
    }
}
