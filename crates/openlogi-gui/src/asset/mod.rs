//! On-disk device asset cache.
//!
//! v0.0.1 is "bring-your-own-cache" — the OpenLogi GUI reads from
//! `~/Library/Application Support/dev.OpenLogi.openlogi/assets/<depot>/`
//! and falls back to the synthetic silhouette when files are missing.
//! Population is the user's problem (rsync from the assets repo, or wait
//! for the HTTP fetch that ships in a later phase).

pub mod index;
pub mod metadata;

use std::path::{Path, PathBuf};

use directories::ProjectDirs;
use openlogi_core::device::DeviceModelInfo;
use tracing::{debug, warn};

use self::index::{DeviceEntry, Index};
use self::metadata::Metadata;

/// Default asset registry filename inside the cache root.
const INDEX_FILE: &str = "index.json";

/// Resolved set of files for one device. Either both `image_path` and
/// `metadata` were loadable from cache, or the cache hit failed and the
/// caller falls back to its synthetic art.
pub struct ResolvedAsset {
    pub depot: String,
    pub display_name: String,
    pub image_path: PathBuf,
    pub metadata: Metadata,
}

pub struct AssetCache {
    root: PathBuf,
    index: Option<Index>,
}

impl AssetCache {
    pub fn new() -> Self {
        let root = cache_root();
        let index = load_index(&root);
        Self { root, index }
    }

    /// Where on disk per-device files live. Public so the user can
    /// populate it from the assets repo.
    pub fn cache_root(&self) -> &Path {
        &self.root
    }

    /// Look up the connected device's depot via its HID++ model info, then
    /// load the cached `front_core.png` + `core_metadata.json` if present.
    ///
    /// Honours `OPENLOGI_FORCE_DEPOT=<depot>` for development — useful when
    /// the physically connected device isn't in the registry yet but you
    /// still want to exercise the asset path.
    pub fn resolve(&self, model: &DeviceModelInfo) -> Option<ResolvedAsset> {
        let index = self.index.as_ref()?;
        if let Ok(forced) = std::env::var("OPENLOGI_FORCE_DEPOT")
            && let Some(entry) = index.devices.get(forced.as_str())
        {
            debug!(depot = %forced, "OPENLOGI_FORCE_DEPOT override active");
            return self.load_files(&forced, entry);
        }
        let candidates = format_candidates(model);
        let (depot, entry) = candidates.iter().find_map(|m| index.find_by_model_id(m))?;
        self.load_files(depot, entry)
    }

    fn load_files(&self, depot: &str, entry: &DeviceEntry) -> Option<ResolvedAsset> {
        let dir = self.root.join(depot);
        let image_path = dir.join("front_core.png");
        let meta_path = dir.join("core_metadata.json");
        if !image_path.exists() || !meta_path.exists() {
            debug!(depot, "asset cache miss — files not populated locally");
            return None;
        }
        let metadata = match Metadata::load_from(&meta_path) {
            Ok(m) => m,
            Err(e) => {
                warn!(depot, error = ?e, "failed to parse core_metadata.json");
                return None;
            }
        };
        Some(ResolvedAsset {
            depot: depot.to_string(),
            display_name: entry.display_name.clone(),
            image_path,
            metadata,
        })
    }
}

impl Default for AssetCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Cache root resolution. Mirrors `openlogi_core::paths::config_dir` but
/// nested under `assets/` to keep it separate from user config files.
fn cache_root() -> PathBuf {
    ProjectDirs::from("dev", "OpenLogi", "openlogi").map_or_else(
        || PathBuf::from("./assets"),
        |d| d.data_dir().join("assets"),
    )
}

fn load_index(root: &Path) -> Option<Index> {
    let path = root.join(INDEX_FILE);
    if !path.exists() {
        debug!(
            ?path,
            "no asset index — using synthetic silhouette for all devices"
        );
        return None;
    }
    match Index::load_from(&path) {
        Ok(idx) => {
            debug!(devices = idx.devices.len(), "asset index loaded");
            Some(idx)
        }
        Err(e) => {
            warn!(error = ?e, "failed to parse asset index");
            None
        }
    }
}

/// Format every non-zero `model_ids[i]` as Logi's registry-style string,
/// e.g. `extended_model_id=0x06 + model_ids[?]=0xb023 → "6b023"`.
/// Returns candidates in array order — the first index that resolves wins.
fn format_candidates(model: &DeviceModelInfo) -> Vec<String> {
    model
        .model_ids
        .iter()
        .filter(|id| **id != 0)
        .map(|id| format!("{:x}{:04x}", model.extended_model_id, id))
        .collect()
}
