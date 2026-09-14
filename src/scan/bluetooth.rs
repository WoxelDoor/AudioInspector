//! Bluetooth: battery, active A2DP codec, the headset's own Bluetooth version and LMP
//! features, Device ID, pairing, profiles from the cached SDP records, per-service
//! switches, the local radio, and GATT battery / device information on an LE side.
//! Several of these properties are undocumented; each was confirmed on a live headset.

use std::collections::HashMap;
use std::ffi::c_void;
use std::time::{Duration, Instant};

use windows::core::{RuntimeType, GUID};
use windows::Devices::Bluetooth::{BluetoothCacheMode, BluetoothConnectionStatus, BluetoothDevice, BluetoothLEDevice};
use windows::Devices::Radios::{Radio, RadioKind, RadioState};
use windows::Foundation::{AsyncStatus, IAsyncOperation};
use windows::Storage::Streams::DataReader;
use windows::Win32::Devices::Bluetooth::{BluetoothFindFirstRadio, BluetoothFindNextRadio, BluetoothFindRadioClose, BLUETOOTH_FIND_RADIO_PARAMS};
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::IO::DeviceIoControl;

use super::endpoints::EpRaw;
use super::names;
use super::sdp::{self, Record};
use crate::model::{Battery, Device, EpState, Flow, Row, Section, Tone};
use crate::sys::devnode;
use crate::sys::keys;
use crate::sys::registry::{self, Hive};
use crate::util::{filetime_to_system, fmt_ago, fmt_time, le_u16, le_u64, mac};

const IOCTL_BTH_GET_RADIO_INFO: u32 = 0x0041_0004;

#[derive(Default)]
pub struct Cache {
    radios: Vec<String>,
}

/// Waits for a WinRT operation with a deadline, so one slow radio call never stalls a scan.
fn wait<T: RuntimeType + 'static>(op: windows::core::Result<IAsyncOperation<T>>, ms: u64) -> Option<T> {
    let op = op.ok()?;
    let deadline = Instant::now() + Duration::from_millis(ms);
    loop {
        match op.Status().ok()? {
            AsyncStatus::Completed => return op.GetResults().ok(),
            AsyncStatus::Started => {}
            _ => return None,
        }
        if Instant::now() > deadline {
            let _ = op.Cancel();
            return None;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn version(v: u16) -> String {
    format!("{}.{}", v >> 8, v & 0xFF)
}

struct LmpInfo {
    features: u64,
    manufacturer: u32,
    subversion: u32,
    version: u32,
    live: bool,
}

/// The remote device's LMP data: live from the radio while connected, else BTHPORT's cache.
fn lmp(addr: u64, cache_key: Option<&registry::Key>) -> Option<LmpInfo> {
    unsafe {
        let params = BLUETOOTH_FIND_RADIO_PARAMS { dwSize: std::mem::size_of::<BLUETOOTH_FIND_RADIO_PARAMS>() as u32 };
        let mut first = HANDLE::default();
        if let Ok(find) = BluetoothFindFirstRadio(&params, &mut first) {
            let mut handles = vec![first];
            loop {
                let mut h = HANDLE::default();
                if BluetoothFindNextRadio(find, &mut h).is_err() {
                    break;
                }
                handles.push(h);
            }
            let _ = BluetoothFindRadioClose(find);
            let mut found = None;
            for h in handles {
                if found.is_none() {
                    let input = addr.to_le_bytes();
                    let mut out = [0u8; 32];
                    let mut ret = 0u32;
                    let ok = DeviceIoControl(
                        h,
                        IOCTL_BTH_GET_RADIO_INFO,
                        Some(input.as_ptr() as *const c_void),
                        8,
                        Some(out.as_mut_ptr() as *mut c_void),
                        out.len() as u32,
                        Some(&mut ret),
                        None,
                    )
                    .is_ok();
                    if ok && ret >= 13 {
                        found = Some(LmpInfo {
                            features: le_u64(&out, 0).unwrap_or(0),
                            manufacturer: le_u16(&out, 8).unwrap_or(0) as u32,
                            subversion: le_u16(&out, 10).unwrap_or(0) as u32,
                            version: out[12] as u32,
                            live: true,
                        });
                    }
                }
                let _ = CloseHandle(h);
            }
            if found.is_some() {
                return found;
            }
        }
    }
    let k = cache_key?;
    Some(LmpInfo {
        features: k.get("LMPFeatures").and_then(|v| v.as_u64()).unwrap_or(0),
        manufacturer: k.get("ManufacturerId").and_then(|v| v.as_u32())?,
        subversion: k.get("LmpSubversion").and_then(|v| v.as_u32()).unwrap_or(0),
        version: k.get("LmpVersion").and_then(|v| v.as_u32())?,
        live: false,
    })
}

fn io_capability(v: u32) -> &'static str {
    match v {
        0 => "display only",
        1 => "display yes/no",
        2 => "keyboard only",
        3 => "no input, no output",
        4 => "keyboard and display",
        _ => "?",
    }
}

/// The link is up when audio flows (an active endpoint) or WinRT says it is connected.
fn link_up(state: EpState, winrt_connected: Option<bool>) -> bool {
    state == EpState::Active || winrt_connected == Some(true)
}

pub fn enrich(dev: &mut Device, eps: &[EpRaw], cache: &mut Cache, _full: bool) {
    let _ = cache;
    let addr = dev.bt_address;
    let winrt = addr.and_then(|a| wait(BluetoothDevice::FromBluetoothAddressAsync(a), 1500));
    let connected = winrt.as_ref().map(|d| d.ConnectionStatus().map(|s| s == BluetoothConnectionStatus::Connected).unwrap_or(false));

    // Battery: Windows keeps it on the Hands-Free AG devnode. A value left
    // from an earlier connection is not the battery now, so it shows only while connected.
    if link_up(dev.state, connected) {
        for id in &dev.devnodes {
            if let Some(pct) = devnode::get(id, &keys::BT_BATTERY).and_then(|v| v.as_u32()) {
                let updated = devnode::get(id, &keys::BT_BATTERY_UPDATED).and_then(|v| v.as_time());
                dev.battery = Some(Battery { percent: pct.min(100) as u8, updated, source: "reported over Hands-Free".into() });
                break;
            }
        }
    }

    let mut bt = Section::new("Bluetooth");
    let hex = addr.map(|a| format!("{a:012x}"));
    let bthport = hex.as_ref().and_then(|h| registry::open(Hive::LocalMachine, &format!(r"SYSTEM\CurrentControlSet\Services\BTHPORT\Parameters\Devices\{h}")));
    if let Some(a) = addr {
        bt.add("Address", mac(a));
    }
    if let Some(c) = connected {
        bt.add("Link", if c { "connected" } else { "not connected" }).tone(if c { Tone::Good } else { Tone::Dim });
    }

    let dev_node = dev.devnodes.iter().find(|id| id.to_uppercase().starts_with("BTHENUM\\DEV_")).cloned();
    let cod = dev_node
        .as_ref()
        .and_then(|n| devnode::get(n, &keys::BT_CLASS_OF_DEVICE).and_then(|v| v.as_u32()))
        .or_else(|| winrt.as_ref().and_then(|d| d.ClassOfDevice().and_then(|c| c.RawValue()).ok()));
    if let Some(raw) = cod {
        let (major, minor, services) = ((raw >> 8) & 0x1F, (raw >> 2) & 0x3F, raw >> 13);
        let mut row = Row::new("Class of device", format!("{} / {}", names::cod_major(major), names::cod_minor(major, minor).unwrap_or("other")));
        row.note = format!("0x{raw:06X}");
        let svc = names::cod_services(services);
        if !svc.is_empty() {
            row.child("Services", svc.join(", "));
        }
        bt.push(row);
    }

    if let Some(a) = addr {
        if let Some(l) = lmp(a, bthport.as_ref()) {
            let mut row = Row::new("Bluetooth version", names::bt_version(l.version).to_string());
            row.note = format!("LMP {} · {}", l.version, if l.live { "read from the radio now" } else { "from Windows' pairing cache" });
            row.child("Controller maker", names::bt_company_label(l.manufacturer));
            row.child("LMP subversion", format!("0x{:04X}", l.subversion));
            let f = names::lmp_features(l.features);
            if !f.is_empty() {
                row.child("Feature mask", format!("0x{:016X}", l.features));
                row.child_list("Features", &f, 4);
            }
            bt.push(row);
        }
    }

    if let Some(n) = &dev_node {
        let src = devnode::get(n, &keys::BT_VID_SOURCE).and_then(|v| v.as_u32());
        let vid = devnode::get(n, &keys::BT_VID).and_then(|v| v.as_u32());
        let pid = devnode::get(n, &keys::BT_PID).and_then(|v| v.as_u32());
        if let (Some(src), Some(vid)) = (src, vid) {
            let vendor = if src == 1 { names::bt_company_label(vid) } else { format!("USB vendor 0x{vid:04X}") };
            let mut row = Row::new("Device ID", vendor);
            row.note = format!("product 0x{:04X} · {}", pid.unwrap_or(0), if src == 1 { "Bluetooth SIG id" } else { "USB-IF id" });
            if let Some(v) = devnode::get(n, &keys::BT_PRODUCT_VERSION).and_then(|v| v.as_u32()) {
                row.child("Version", format!("0x{v:04X}"));
            }
            bt.push(row);
        }
    }

    // Pairing and services, from the Bluetooth port driver's cache.
    if let Some(k) = &bthport {
        for sub in k.subkeys().iter().filter(|s| s.starts_with("ServicesFor")) {
            let Some(sk) = k.open(sub) else { continue };
            let flag = |name: &str| sk.get(name).and_then(|v| v.as_u32());
            let mut row = Row::new("Pairing", if flag("SSP Paired") == Some(1) { "Secure Simple Pairing" } else { "legacy" });
            if let Some(m) = flag("SSP MITM Protected") {
                row.child("MITM protection", if m == 1 { "yes" } else { "no" });
            }
            if let Some(io) = flag("IoCapability") {
                row.child("Headset I/O", io_capability(io));
            }
            if let Some(d) = &winrt {
                if let Ok(sc) = d.WasSecureConnectionUsedForPairing() {
                    row.child("Secure Connections", if sc { "used" } else { "not used" });
                }
            }
            bt.push(row);
            let mut services = Row::new("Services", "");
            let (mut on, mut total) = (0, 0);
            for svc in sk.subkeys() {
                let Some(inst) = sk.open(&format!(r"{svc}\C00000000")) else { continue };
                let enabled = inst.get("Enabled").and_then(|v| v.as_u32()).unwrap_or(0) == 1;
                let uuid = crate::util::parse_guid(&svc);
                let short = uuid.map(|g| (g.to_u128() >> 96) as u32);
                let label = short
                    .filter(|_| uuid.map(|g| g.to_u128() & 0xFFFF_FFFF_FFFF_FFFF_FFFF_FFFF == 0x0000_1000_8000_0080_5F9B_34FB).unwrap_or(false))
                    .and_then(sdp::uuid_name)
                    .map(String::from)
                    .unwrap_or_else(|| {
                        inst.get("PriLangServiceName")
                            .and_then(|v| v.as_bytes().map(|b| String::from_utf8_lossy(b).trim_end_matches('\0').to_string()))
                            .filter(|s| !s.is_empty())
                            .unwrap_or_else(|| svc.clone())
                    });
                total += 1;
                if enabled {
                    on += 1;
                }
                services.child(label, if enabled { "enabled" } else { "disabled" }).tone(if enabled { Tone::Normal } else { Tone::Warn });
            }
            services.value = format!("{on}/{total} enabled");
            if total > 0 {
                bt.push(services);
            }
            break;
        }
        if let Some(t) = k.get("LastConnected").and_then(|v| v.as_u64()).and_then(filetime_to_system) {
            bt.add("Last connection record", fmt_time(t)).note("pairing cache; not rewritten on every connection").tone(Tone::Dim);
        }
    }

    let a2dp_count = dev
        .devnodes
        .iter()
        .find(|id| id.to_uppercase().contains("{0000110B-"))
        .and_then(|id| registry::open(Hive::LocalMachine, &format!(r"SYSTEM\CurrentControlSet\Enum\{id}\Device Parameters")))
        .and_then(|k| k.get("ConnectionCount").and_then(|v| v.as_u32()));
    if let Some(c) = a2dp_count {
        bt.add("Audio connections", c.to_string()).note("A2DP ConnectionCount");
    }
    dev.connection = radio_name(dev).map(|r| format!("via {r}"));
    dev.sections.push(bt);

    if let Some(s) = codec_section(dev, eps) {
        dev.sections.push(s);
    }
    if let Some(d) = &winrt {
        if let Some(s) = profiles_section(d) {
            dev.sections.push(s);
        }
    }
    if let Some(a) = addr {
        if let Some(s) = le_section(a, dev) {
            dev.sections.push(s);
        }
    }
    if let Some(s) = radio_section(dev) {
        dev.sections.push(s);
    }
}

fn codec_section(dev: &Device, eps: &[EpRaw]) -> Option<Section> {
    let mut s = Section::new("Audio codec");
    let a2dp = eps.iter().find(|e| {
        e.ep.flow == Flow::Render && e.ep.function.as_deref().map(|f| f.to_uppercase().contains("{0000110B-")).unwrap_or(false)
    });
    let hfp = eps.iter().find(|e| e.ep.function.as_deref().map(|f| f.to_uppercase().starts_with("BTHHFENUM")).unwrap_or(false));
    if let Some(e) = a2dp {
        // Stream rows only while the A2DP endpoint is active: the format Windows keeps
        // for a disconnected headset is the last connection's, not a stream.
        let live = e.ep.state == EpState::Active;
        let path = e.ep.filter_interface.as_deref();
        if !live {
            s.add("Music (A2DP)", "not connected").tone(Tone::Dim);
        } else if let Some(path) = path {
            let active = devnode::interface_get(path, &keys::A2DP_CODEC_ACTIVE).and_then(|v| v.as_bytes().map(|b| b.to_vec()));
            match active {
                Some(b) if !b.is_empty() => {
                    s.add("Music (A2DP)", names::a2dp_codec(&b).0).note("undocumented Windows property").tone(Tone::Good);
                }
                _ => {
                    s.add("Music (A2DP)", "not reported").note("Windows sets it while the headset is connected").tone(Tone::Dim);
                }
            }
        }
        if let Some(list) = path.and_then(|p| devnode::interface_get(p, &keys::A2DP_CODEC_LIST)).and_then(|v| v.as_bytes().map(|b| b.to_vec())) {
            let l = names::a2dp_codec_list(&list);
            if !l.is_empty() {
                s.add("Headset offers", l.join(", "));
            }
        }
        if live {
            if let Some(f) = e.ep.format {
                s.add("Into the encoder", f.short()).note(format!("{:.0} kbit/s PCM before compression", f.kbps()));
            }
            s.add("Bitrate on air", "not reported by Windows").note("no Windows API carries it").tone(Tone::Dim);
        }
    }
    if let Some(e) = hfp.filter(|e| e.ep.state == EpState::Active) {
        if let Some(f) = e.ep.format {
            let codec = match f.sample_rate {
                8000 => "CVSD, narrow band",
                16000 => "mSBC, wide band",
                32000 => "LC3-SWB, super wide band",
                _ => "unlisted rate",
            };
            s.add("Calls (Hands-Free)", codec).note(format!("inferred from the {} call endpoint", crate::model::khz(f.sample_rate)));
        }
    }
    let _ = dev;
    (!s.is_empty()).then_some(s)
}

fn profiles_section(d: &BluetoothDevice) -> Option<Section> {
    let recs = d.SdpRecords().ok()?;
    let mut s = Section::new("Profiles");
    for buf in recs {
        let Ok(n) = buf.Length() else { continue };
        let mut bytes = vec![0u8; n as usize];
        let Ok(reader) = DataReader::FromBuffer(&buf) else { continue };
        if reader.ReadBytes(&mut bytes).is_err() {
            continue;
        }
        let Some(r) = Record::parse(&bytes) else { continue };
        let classes: Vec<u32> = r.service_classes().iter().filter_map(|c| c.short_uuid()).collect();
        let profiles = r.profiles();
        let ver = profiles.first().map(|(_, v)| version(*v)).unwrap_or_default();
        let feats = r.features().unwrap_or(0);
        let first = classes.first().copied();
        let (label, note): (String, Vec<&str>) = match first {
            Some(0x110B) => ("A2DP sink".into(), sdp::a2dp_sink_features(feats)),
            Some(0x110A) => ("A2DP source".into(), sdp::a2dp_source_features(feats)),
            Some(0x110C) => ("AVRCP target".into(), sdp::avrcp_target_features(feats)),
            Some(0x110E) | Some(0x110F) => ("AVRCP controller".into(), sdp::avrcp_controller_features(feats)),
            Some(0x111E) => ("Hands-Free".into(), sdp::hfp_features(feats)),
            Some(0x111F) => ("Hands-Free gateway".into(), Vec::new()),
            Some(0x1108) | Some(0x1131) => {
                let vol = matches!(r.get(0x0302), Some(sdp::De::Bool(true)));
                ("Headset (HSP)".into(), if vol { vec!["remote volume"] } else { Vec::new() })
            }
            Some(0x1200) => {
                let vid = r.get(0x0201).and_then(|v| v.uint()).unwrap_or(0) as u32;
                let pid = r.get(0x0202).and_then(|v| v.uint()).unwrap_or(0);
                let v = r.get(0x0203).and_then(|v| v.uint()).unwrap_or(0);
                let src = r.get(0x0205).and_then(|v| v.uint()).unwrap_or(1);
                let vendor = if src == 1 { names::bt_company_label(vid) } else { format!("USB vendor 0x{vid:04X}") };
                s.push(Row::new("PnP information", vendor).with_note(format!("product 0x{pid:04X}, version 0x{v:04X}")));
                continue;
            }
            Some(0x1800) | Some(0x1801) => ("GATT over BR/EDR".into(), Vec::new()),
            Some(u) => (sdp::uuid_name(u).unwrap_or("Service").to_string(), Vec::new()),
            None => {
                let uuid = r.service_classes().first().map(|c| c.uuid_text()).unwrap_or_default();
                let mut row = Row::new(r.name().unwrap_or_else(|| "Vendor service".into()), "vendor");
                row.note = match r.rfcomm_channel() {
                    Some(ch) => format!("{uuid} · RFCOMM {ch}"),
                    None => uuid,
                };
                row.tone = Tone::Dim;
                s.push(row);
                continue;
            }
        };
        let mut row = Row::new(label, if ver.is_empty() { "-".to_string() } else { ver });
        let mut parts: Vec<String> = note.iter().map(|x| x.to_string()).collect();
        if let Some(ch) = r.rfcomm_channel() {
            parts.push(format!("RFCOMM {ch}"));
        }
        row.note = parts.join(" · ");
        s.push(row);
    }
    (!s.is_empty()).then_some(s)
}

fn gatt_read(dev: &BluetoothLEDevice, service: u32, characteristic: u32, mode: BluetoothCacheMode) -> Option<Vec<u8>> {
    let base = |short: u32| GUID::from_u128(((short as u128) << 96) | 0x0000_1000_8000_0080_5F9B_34FB);
    let services = wait(dev.GetGattServicesForUuidWithCacheModeAsync(base(service), mode), 1200)?;
    let svc = services.Services().ok()?.into_iter().next()?;
    let chars = wait(svc.GetCharacteristicsForUuidWithCacheModeAsync(base(characteristic), mode), 1200)?;
    let ch = chars.Characteristics().ok()?.into_iter().next()?;
    let result = wait(ch.ReadValueWithCacheModeAsync(mode), 1200)?;
    let buf = result.Value().ok()?;
    let n = buf.Length().ok()?;
    let mut bytes = vec![0u8; n as usize];
    DataReader::FromBuffer(&buf).ok()?.ReadBytes(&mut bytes).ok()?;
    Some(bytes)
}

/// GATT Battery Service and Device Information, when the device has an LE side.
fn le_section(addr: u64, dev: &mut Device) -> Option<Section> {
    let le = wait(BluetoothLEDevice::FromBluetoothAddressAsync(addr), 1200)?;
    let connected = le.ConnectionStatus().map(|s| s == BluetoothConnectionStatus::Connected).unwrap_or(false);
    let mode = if connected { BluetoothCacheMode::Uncached } else { BluetoothCacheMode::Cached };
    let mut s = Section::new("Bluetooth LE");
    s.add("LE link", if connected { "connected" } else { "not connected" });
    // A cached battery level is an old one; device information strings do not age.
    if let Some(b) = gatt_read(&le, 0x180F, 0x2A19, mode).and_then(|b| b.first().copied()).filter(|_| connected) {
        s.add("Battery (GATT)", format!("{b} %")).tone(Tone::Good);
        if dev.battery.is_none() {
            dev.battery = Some(Battery { percent: b.min(100), updated: None, source: "GATT Battery Service".into() });
        }
    }
    for (ch, label) in [(0x2A29, "Manufacturer"), (0x2A24, "Model"), (0x2A25, "Serial number"), (0x2A26, "Firmware"), (0x2A27, "Hardware"), (0x2A28, "Software")] {
        if let Some(b) = gatt_read(&le, 0x180A, ch, mode) {
            let text = String::from_utf8_lossy(&b).trim_end_matches('\0').trim().to_string();
            if !text.is_empty() {
                match ch {
                    0x2A29 => {
                        dev.manufacturer.get_or_insert(text.clone());
                    }
                    0x2A24 => {
                        dev.model.get_or_insert(text.clone());
                    }
                    _ => {}
                }
                s.add(label, text).note("GATT Device Information");
            }
        }
    }
    (s.rows.len() > 1).then_some(s)
}

/// The local adapter the device hangs off: the nearest ancestor that carries radio keys.
fn radio_node(dev: &Device) -> Option<String> {
    let start = dev.devnodes.iter().find(|id| devnode::enumerator(id).starts_with("BTH"))?;
    devnode::ancestors(start).into_iter().find(|a| devnode::get(a, &keys::RADIO_ADDRESS).is_some())
}

fn radio_name(dev: &Device) -> Option<String> {
    let r = radio_node(dev)?;
    devnode::get_str(&r, &keys::FRIENDLY_NAME).or_else(|| devnode::get_str(&r, &keys::DEVICE_DESC))
}

fn describe_radio(node: &str, title: &str) -> Section {
    let mut s = Section::new(title);
    let name = devnode::get_str(node, &keys::FRIENDLY_NAME).or_else(|| devnode::get_str(node, &keys::DEVICE_DESC)).unwrap_or_default();
    s.add("Adapter", name);
    let u = |k| devnode::get(node, k).and_then(|v| v.as_u64());
    if let Some(v) = u(&keys::RADIO_LMP_VERSION) {
        let mut row = Row::new("Bluetooth version", names::bt_version(v as u32));
        row.note = format!("LMP {v}");
        if let Some(h) = u(&keys::RADIO_HCI_VERSION) {
            row.child("HCI version", format!("{} ({h})", names::bt_version(h as u32)));
        }
        if let Some(r) = u(&keys::RADIO_HCI_REVISION) {
            row.child("HCI revision", format!("0x{r:04X}"));
        }
        if let Some(sv) = u(&keys::RADIO_LMP_SUBVERSION) {
            row.child("LMP subversion", format!("0x{sv:04X}"));
        }
        s.push(row);
    }
    if let Some(m) = u(&keys::RADIO_MANUFACTURER) {
        s.add("Chip maker", names::bt_company_label(m as u32));
    }
    if let Some(a) = u(&keys::RADIO_ADDRESS) {
        s.add("Address", mac(a));
    }
    if let Some(f) = u(&keys::RADIO_LMP_FEATURES) {
        let list = names::lmp_features(f);
        if !list.is_empty() {
            let row = s.add("Features", format!("0x{f:016X}"));
            row.child_list("", &list, 4);
        }
    }
    let provider = devnode::get_str(node, &keys::DRIVER_PROVIDER).unwrap_or_default();
    let version = devnode::get_str(node, &keys::DRIVER_VERSION).unwrap_or_default();
    if !provider.is_empty() || !version.is_empty() {
        s.add("Driver", format!("{provider} {version}").trim().to_string());
    }
    s
}

fn radio_section(dev: &Device) -> Option<Section> {
    radio_node(dev).map(|r| describe_radio(&r, "Bluetooth adapter"))
}

pub fn system_sections(cache: &mut Cache) -> Vec<Section> {
    if cache.radios.is_empty() {
        cache.radios = devnode::all_instance_ids(true)
            .into_iter()
            .filter(|id| matches!(devnode::enumerator(id).as_str(), "USB" | "PCI" | "ACPI" | "SDIO"))
            .filter(|id| devnode::get(id, &keys::RADIO_ADDRESS).is_some())
            .collect();
    }
    let mut out = Vec::new();
    let states: HashMap<String, RadioState> = wait(Radio::GetRadiosAsync(), 1500)
        .map(|v| {
            v.into_iter()
                .filter(|r| r.Kind().map(|k| k == RadioKind::Bluetooth).unwrap_or(false))
                .filter_map(|r| Some((r.Name().ok()?.to_string(), r.State().ok()?)))
                .collect()
        })
        .unwrap_or_default();
    for (i, id) in cache.radios.clone().iter().enumerate() {
        let mut s = describe_radio(id, if cache.radios.len() > 1 { "Bluetooth radio" } else { "Bluetooth" });
        if i == 0 {
            if let Some((_, st)) = states.iter().next() {
                let label = match *st {
                    RadioState::On => "on",
                    RadioState::Off => "off",
                    RadioState::Disabled => "disabled",
                    _ => "unknown",
                };
                s.rows.insert(1, Row::new("Radio", label).with_tone(if *st == RadioState::On { Tone::Good } else { Tone::Warn }));
            }
        }
        out.push(s);
    }
    let _ = fmt_ago;
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AudioFormat, Endpoint};

    fn ep(function: &str, flow: Flow, state: EpState, rate: u32, channels: u16) -> EpRaw {
        EpRaw {
            ep: Endpoint {
                flow,
                state,
                function: Some(function.into()),
                format: Some(AudioFormat { sample_rate: rate, bits: 16, valid_bits: 16, channels, float: false, channel_mask: 0 }),
                ..Default::default()
            },
            container: None,
            device_name: None,
            form_code: 3,
            jack_subtype: None,
            has_sink: false,
            store: Vec::new(),
        }
    }

    fn a2dp(state: EpState) -> EpRaw {
        ep(r"BTHENUM\{0000110B-0000-1000-8000-00805F9B34FB}_VID&0001000A_PID&FFFF\7&0&0&0123456789AB_C00000000", Flow::Render, state, 48000, 2)
    }

    fn hfp(state: EpState) -> EpRaw {
        ep(r"BTHHFENUM\BthHFPAudio\8&0&0&97", Flow::Capture, state, 16000, 1)
    }

    fn labels(s: &Section) -> Vec<&str> {
        s.rows.iter().map(|r| r.label.as_str()).collect()
    }

    #[test]
    fn a_disconnected_headset_shows_no_stream_rows() {
        let s = codec_section(&Device::default(), &[a2dp(EpState::Unplugged), hfp(EpState::Unplugged)]).unwrap();
        assert_eq!((s.rows[0].label.as_str(), s.rows[0].value.as_str()), ("Music (A2DP)", "not connected"));
        for stale in ["Into the encoder", "Bitrate on air", "Calls (Hands-Free)"] {
            assert!(!labels(&s).contains(&stale), "{stale} shown for a disconnected headset");
        }
    }

    #[test]
    fn a_connected_headset_shows_the_stream_rows() {
        let s = codec_section(&Device::default(), &[a2dp(EpState::Active), hfp(EpState::Active)]).unwrap();
        for live in ["Into the encoder", "Bitrate on air", "Calls (Hands-Free)"] {
            assert!(labels(&s).contains(&live), "{live} missing");
        }
    }

    #[test]
    fn battery_is_read_only_while_the_link_is_up() {
        assert!(link_up(EpState::Active, None));
        assert!(link_up(EpState::Active, Some(false)));
        assert!(link_up(EpState::Unplugged, Some(true)));
        assert!(!link_up(EpState::Unplugged, Some(false)));
        assert!(!link_up(EpState::Unplugged, None));
    }
}
