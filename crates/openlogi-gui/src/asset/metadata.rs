#![allow(
    dead_code,
    reason = "full schema parsed; label direction codes + extra coords land in later phases"
)]

//! Parses the per-depot `core_metadata.json` shipped by the Logi Options+
//! installer (and re-hosted by assets.openlogi.org).
//!
//! Only the fields OpenLogi actually uses are deserialized — every other
//! field is silently ignored. The schema below is observed-from-the-wild,
//! not derived from any Logitech specification.
//!
//! ```json
//! {
//!   "images": [
//!     {
//!       "key": "device_image",
//!       "origin": { "width": 687, "height": 1024 }
//!     },
//!     {
//!       "key": "device_buttons_image",
//!       "origin": { "width": 687, "height": 1024 },
//!       "assignments": [
//!         { "slotId": "...", "slotName": "SLOT_NAME_MIDDLE_BUTTON",
//!           "marker": { "x": 73, "y": 18 },
//!           "label":  { "x": 1,  "y": 0  } }
//!       ]
//!     }
//!   ]
//! }
//! ```
//!
//! `marker.{x,y}` is a percentage 0..100 of the device image's origin
//! dimensions. `label.{x,y}` is a direction code (-1 = left, 0 = centre,
//! +1 = right; same for y) that hints where the annotation card should sit
//! relative to the marker.

use std::path::Path;

use serde::Deserialize;

use crate::data::mouse_buttons::ButtonId;

#[derive(Debug, Deserialize)]
pub struct Metadata {
    pub images: Vec<ImageEntry>,
}

#[derive(Debug, Deserialize)]
pub struct ImageEntry {
    pub key: String,
    pub origin: Origin,
    #[serde(default)]
    pub assignments: Vec<Assignment>,
}

#[derive(Debug, Deserialize, Clone, Copy)]
pub struct Origin {
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Deserialize)]
pub struct Assignment {
    #[serde(rename = "slotName")]
    pub slot_name: String,
    pub marker: Point,
    #[serde(default)]
    pub label: Direction,
}

#[derive(Debug, Deserialize, Clone, Copy)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Deserialize, Clone, Copy, Default)]
pub struct Direction {
    pub x: i32,
    pub y: i32,
}

impl Metadata {
    pub fn load_from(path: &Path) -> anyhow::Result<Self> {
        let bytes = std::fs::read(path)?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    /// Image dimensions (use the `device_image` entry — both entries always
    /// share the same origin in practice).
    pub fn origin(&self) -> Option<Origin> {
        self.images.first().map(|i| i.origin)
    }

    /// Yields the button hotspot assignments. Slot names without a known
    /// [`ButtonId`] mapping are skipped — they'll come in as we extend the
    /// enum to cover thumbwheels, modeshift buttons, etc.
    pub fn hotspots(&self) -> impl Iterator<Item = HotspotEntry> + '_ {
        self.images
            .iter()
            .find(|i| i.key == "device_buttons_image")
            .into_iter()
            .flat_map(|img| img.assignments.iter())
            .filter_map(|a| {
                Some(HotspotEntry {
                    id: map_slot_name(&a.slot_name)?,
                    marker: a.marker,
                    label: a.label,
                })
            })
    }
}

pub struct HotspotEntry {
    pub id: ButtonId,
    pub marker: Point,
    pub label: Direction,
}

fn map_slot_name(name: &str) -> Option<ButtonId> {
    // Logitech's slot names are stable across devices in the same family.
    // Mapping is intentionally conservative — unknown slots fall through
    // so we can extend ButtonId without breaking older clients.
    match name {
        "SLOT_NAME_LEFT_BUTTON" => Some(ButtonId::LeftClick),
        "SLOT_NAME_RIGHT_BUTTON" => Some(ButtonId::RightClick),
        "SLOT_NAME_MIDDLE_BUTTON" => Some(ButtonId::MiddleClick),
        "SLOT_NAME_BACK_BUTTON" => Some(ButtonId::Back),
        "SLOT_NAME_FORWARD_BUTTON" => Some(ButtonId::Forward),
        "SLOT_NAME_MODESHIFT_BUTTON" => Some(ButtonId::DpiToggle),
        _ => None,
    }
}
