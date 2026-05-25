//! Enumerate connected HID++ receivers and their paired devices.

use std::{collections::HashMap, sync::Arc, time::Duration};

use async_hid::HidBackend;
use futures_lite::StreamExt;
use hidpp::{
    channel::HidppChannel,
    device::Device,
    feature::{
        device_information::v0::DeviceInformationFeatureV0,
        unified_battery::v0::{
            BatteryLevel as HidppBatteryLevel, BatteryStatus as HidppBatteryStatus,
            UnifiedBatteryFeatureV0,
        },
    },
    nibble::U4,
    receiver::{
        self, Receiver,
        bolt::{BoltDeviceConnection, BoltDeviceKind, BoltEvent, BoltReceiver},
    },
};
use openlogi_core::device::{
    BatteryInfo, BatteryLevel, BatteryStatus, DeviceInventory, DeviceKind, DeviceModelInfo,
    DeviceTransports, PairedDevice, ReceiverInfo,
};
use thiserror::Error;
use tokio::time::timeout;
use tracing::{debug, warn};

use crate::transport::AsyncHidChannel;

/// Logitech HID vendor ID.
const LOGITECH_VID: u16 = 0x046d;
/// HID++ long-report usage page / usage. Filtering on this pair gives us one
/// HID node per physical HID++ device on every supported OS.
const HIDPP_USAGE_PAGE: u16 = 0xff00;
const HIDPP_LONG_USAGE_ID: u16 = 0x0002;

/// How long to wait for device-arrival event bursts before assuming the
/// receiver has finished reporting. MX Master 4 (and other devices that may
/// be asleep) need a generous window to wake and respond to the arrival
/// ping; we err on the side of waiting.
const ARRIVAL_DRAIN: Duration = Duration::from_millis(1500);

/// Maximum number of pairing slots a Bolt receiver supports. We iterate this
/// range to surface paired-but-offline devices that won't fire arrival events.
const MAX_BOLT_SLOTS: u8 = 6;

#[derive(Debug, Error)]
pub enum InventoryError {
    #[error("HID transport error")]
    Hid(#[from] async_hid::HidError),
}

/// Enumerate all Logitech HID++ receivers visible to the current process and
/// the devices paired to each.
///
/// Combines two data sources per receiver:
///
/// - `trigger_device_arrival` events — the only path to a device's wireless
///   PID in hidpp 0.2 (the `wpid` field on `BoltDevicePairingInformation` is
///   private). Only online, responsive devices show up here.
/// - `get_device_pairing_information` polled per slot — covers paired-but-
///   offline devices (sleeping mice, devices on a different host) that the
///   arrival ping doesn't wake. No wpid for these.
///
/// We merge the two so an MX Master that's been asleep still shows up with
/// its codename and kind even before you click it.
pub async fn enumerate() -> Result<Vec<DeviceInventory>, InventoryError> {
    let backend = HidBackend::default();
    let candidates: Vec<async_hid::Device> = backend
        .enumerate()
        .await?
        .filter(|d| {
            d.vendor_id == LOGITECH_VID
                && d.usage_page == HIDPP_USAGE_PAGE
                && d.usage_id == HIDPP_LONG_USAGE_ID
        })
        .collect()
        .await;

    debug!(count = candidates.len(), "HID++ candidate interfaces");

    let mut inventories = Vec::new();
    for dev in candidates {
        match probe_one(dev).await {
            Ok(Some(inv)) => inventories.push(inv),
            Ok(None) => {}
            Err(e) => warn!(error = ?e, "skipping device that failed to probe"),
        }
    }

    Ok(inventories)
}

async fn probe_one(dev: async_hid::Device) -> Result<Option<DeviceInventory>, InventoryError> {
    // `Device: Deref<Target = DeviceInfo>` — clone the deref'd value so we
    // can keep using `dev` (which `to_device_info` would consume).
    let info: async_hid::DeviceInfo = (*dev).clone();
    let (reader, writer) = dev.open().await?;
    let raw = AsyncHidChannel::new(reader, writer, info.clone());

    let channel = match HidppChannel::from_raw_channel(raw).await {
        Ok(c) => Arc::new(c),
        Err(e) => {
            debug!(name = %info.name, error = ?e, "not a HID++ channel");
            return Ok(None);
        }
    };

    let Some(Receiver::Bolt(bolt)) = receiver::detect(Arc::clone(&channel)) else {
        debug!(
            vid = format_args!("{:04x}", info.vendor_id),
            pid = format_args!("{:04x}", info.product_id),
            "no supported receiver (hidpp 0.2 only recognises Logi Bolt)"
        );
        return Ok(None);
    };

    let unique_id = bolt.get_unique_id().await.ok();
    let pairing_count = bolt.count_pairings().await.ok();
    debug!(?pairing_count, "receiver reports pairing count");

    let connections = drain_device_arrival(&bolt).await;
    debug!(events = connections.len(), "drained device-arrival events");
    let by_slot: HashMap<u8, BoltDeviceConnection> =
        connections.into_iter().map(|c| (c.index, c)).collect();

    let mut paired = Vec::new();
    for slot in 1u8..=MAX_BOLT_SLOTS {
        let pairing = match bolt.get_device_pairing_information(U4::from_lo(slot)).await {
            Ok(p) => p,
            Err(e) => {
                debug!(slot, error = ?e, "slot empty or unreadable");
                continue;
            }
        };

        let codename = read_codename(&channel, slot).await;
        let event = by_slot.get(&slot);
        // Prefer event data when present — it's a live response. Fall back to
        // the pairing register for sleeping devices that didn't reply.
        let online = event.map_or(pairing.online, |c| c.online);
        let kind = event.map_or(pairing.kind, |c| c.kind);
        let wpid = event.map(|c| c.wpid);
        debug!(
            slot,
            online,
            ?wpid,
            ?kind,
            has_event = event.is_some(),
            codename = ?codename,
            "paired slot"
        );

        let (battery, model_info) = if online {
            probe_features(&channel, slot).await
        } else {
            (None, None)
        };
        paired.push(PairedDevice {
            slot,
            codename,
            wpid,
            kind: map_kind(kind),
            online,
            battery,
            model_info,
        });
    }

    if let Some(count) = pairing_count
        && paired.len() != usize::from(count)
    {
        warn!(
            expected = count,
            found = paired.len(),
            "paired-device count mismatch — some slots may be unreadable"
        );
    }

    Ok(Some(DeviceInventory {
        receiver: ReceiverInfo {
            name: "Logi Bolt Receiver".to_string(),
            vendor_id: info.vendor_id,
            product_id: info.product_id,
            unique_id,
        },
        paired,
    }))
}

async fn drain_device_arrival(bolt: &BoltReceiver) -> Vec<BoltDeviceConnection> {
    let rx = bolt.listen();
    if let Err(e) = bolt.trigger_device_arrival().await {
        debug!(error = ?e, "trigger_device_arrival failed; receiver may report no devices");
        return Vec::new();
    }

    let mut out = Vec::new();
    loop {
        match timeout(ARRIVAL_DRAIN, rx.recv()).await {
            Ok(Ok(BoltEvent::DeviceConnection(c))) => out.push(c),
            Ok(Ok(_)) => {} // BoltEvent is non_exhaustive; ignore future variants
            Ok(Err(_)) | Err(_) => break,
        }
    }
    out
}

/// Reads a paired device's codename, working around a slicing bug in
/// `hidpp 0.2`'s `BoltReceiver::get_device_codename` that truncates names
/// longer than 8 characters (it treats `response[2]` as an end-index when it
/// is actually the byte length — see Solaar's `device_codename` for the
/// correct slice). 16-byte long-register response is `[sub, chunk, len,
/// data..13]`; we cap at 13 to stay in-bounds. Long names (>13 chars) would
/// need multi-chunk reads with chunk param > 0x01; not needed for v0.0.x.
async fn read_codename(channel: &HidppChannel, slot: u8) -> Option<String> {
    // 0xFF = receiver device index, 0xB5 = ReceiverInfo register,
    // 0x60+slot = DeviceCodename sub-register, 0x01 = first chunk.
    let response = channel
        .read_long_register(0xFF, 0xB5, [0x60 + slot, 0x01, 0x00])
        .await
        .ok()?;
    let len = usize::from(response[2]).min(13);
    core::str::from_utf8(&response[3..3 + len])
        .ok()
        .map(str::to_string)
}

/// Open a HID++ session for `slot` and query the two features we care about
/// (battery, device-information) in one shot. Returns `(battery, model)` —
/// either side may be `None` if the device doesn't expose that feature or
/// the read fails. Device sessions are expensive (multi-round-trip) so we
/// fold both reads through the same `Device::new` + `enumerate_features`.
async fn probe_features(
    channel: &Arc<HidppChannel>,
    slot: u8,
) -> (Option<BatteryInfo>, Option<DeviceModelInfo>) {
    let mut device = match Device::new(Arc::clone(channel), slot).await {
        Ok(d) => d,
        Err(e) => {
            debug!(slot, error = ?e, "Device::new failed");
            return (None, None);
        }
    };
    if let Err(e) = device.enumerate_features().await {
        debug!(slot, error = ?e, "enumerate_features failed");
        return (None, None);
    }

    let battery = match device.get_feature::<UnifiedBatteryFeatureV0>() {
        Some(feature) => feature
            .get_battery_info()
            .await
            .ok()
            .map(|info| BatteryInfo {
                percentage: info.charging_percentage,
                level: map_battery_level(info.level),
                status: map_battery_status(info.status),
            }),
        None => None,
    };

    let model_info = match device.get_feature::<DeviceInformationFeatureV0>() {
        Some(feature) => match feature.get_device_info().await {
            Ok(info) => Some(DeviceModelInfo {
                entity_count: info.entity_count,
                unit_id: info.unit_id,
                transports: DeviceTransports {
                    usb: info.transport.usb,
                    equad: info.transport.e_quad,
                    btle: info.transport.btle,
                    bluetooth: info.transport.bluetooth,
                },
                model_ids: info.model_id,
                extended_model_id: info.extended_model_id,
            }),
            Err(e) => {
                debug!(slot, error = ?e, "DeviceInformation read failed");
                None
            }
        },
        None => None,
    };

    (battery, model_info)
}

fn map_kind(k: BoltDeviceKind) -> DeviceKind {
    match k {
        BoltDeviceKind::Keyboard => DeviceKind::Keyboard,
        BoltDeviceKind::Mouse => DeviceKind::Mouse,
        BoltDeviceKind::Numpad => DeviceKind::Numpad,
        BoltDeviceKind::Presenter => DeviceKind::Presenter,
        BoltDeviceKind::Remote => DeviceKind::Remote,
        BoltDeviceKind::Trackball => DeviceKind::Trackball,
        BoltDeviceKind::Touchpad => DeviceKind::Touchpad,
        BoltDeviceKind::Tablet => DeviceKind::Tablet,
        BoltDeviceKind::Gamepad => DeviceKind::Gamepad,
        BoltDeviceKind::Joystick => DeviceKind::Joystick,
        BoltDeviceKind::Headset => DeviceKind::Headset,
        _ => DeviceKind::Unknown,
    }
}

fn map_battery_level(level: HidppBatteryLevel) -> BatteryLevel {
    match level {
        HidppBatteryLevel::Critical => BatteryLevel::Critical,
        HidppBatteryLevel::Low => BatteryLevel::Low,
        HidppBatteryLevel::Good => BatteryLevel::Good,
        HidppBatteryLevel::Full => BatteryLevel::Full,
        _ => BatteryLevel::Unknown,
    }
}

fn map_battery_status(status: HidppBatteryStatus) -> BatteryStatus {
    match status {
        HidppBatteryStatus::Discharging => BatteryStatus::Discharging,
        HidppBatteryStatus::Charging => BatteryStatus::Charging,
        HidppBatteryStatus::ChargingSlow => BatteryStatus::ChargingSlow,
        HidppBatteryStatus::Full => BatteryStatus::Full,
        HidppBatteryStatus::Error => BatteryStatus::Error,
        _ => BatteryStatus::Unknown,
    }
}
