//! USB identity from the parent hub: connection info, speed, Type-C connector flag,
//! device/configuration/string descriptors and the audio class details in them.

use std::ffi::c_void;

use windows::core::PCWSTR;
use windows::Win32::Devices::Usb::{
    GUID_DEVINTERFACE_USB_HUB, IOCTL_USB_GET_DESCRIPTOR_FROM_NODE_CONNECTION, IOCTL_USB_GET_NODE_CONNECTION_INFORMATION_EX,
    IOCTL_USB_GET_NODE_CONNECTION_INFORMATION_EX_V2, IOCTL_USB_GET_PORT_CONNECTOR_PROPERTIES,
};
use windows::Win32::Foundation::{CloseHandle, GENERIC_WRITE, HANDLE};
use windows::Win32::Storage::FileSystem::{CreateFileW, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING};
use windows::Win32::System::IO::DeviceIoControl;

use super::names;
use crate::model::{khz, Row, Section, Tone};
use crate::sys::{devnode, keys};
use crate::util::{le_u16, le_u32, wide};

#[derive(Clone, Debug)]
pub struct UsbInfo {
    pub section: Section,
    pub connection: String,
    pub manufacturer: Option<String>,
    pub product: Option<String>,
}

struct Hub(HANDLE);

impl Drop for Hub {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

fn open_hub(path: &str, access: u32) -> Option<Hub> {
    let w = wide(path);
    unsafe {
        CreateFileW(
            PCWSTR(w.as_ptr()),
            access,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_FLAGS_AND_ATTRIBUTES(0),
            None,
        )
        .ok()
        .map(Hub)
    }
}

fn ioctl(hub: &Hub, code: u32, buf: &mut [u8]) -> Result<usize, windows::core::Error> {
    let mut ret = 0u32;
    let len = buf.len() as u32;
    unsafe {
        DeviceIoControl(
            hub.0,
            code,
            Some(buf.as_ptr() as *const c_void),
            len,
            Some(buf.as_mut_ptr() as *mut c_void),
            len,
            Some(&mut ret),
            None,
        )?;
    }
    Ok(ret as usize)
}

fn descriptor(hub: &Hub, port: u32, kind: u8, index: u8, lang: u16, len: usize) -> Option<Vec<u8>> {
    let mut buf = vec![0u8; 12 + len];
    buf[0..4].copy_from_slice(&port.to_le_bytes());
    buf[4] = 0x80; // device-to-host, standard, device
    buf[5] = 6; // GET_DESCRIPTOR
    buf[6..8].copy_from_slice(&(((kind as u16) << 8) | index as u16).to_le_bytes());
    buf[8..10].copy_from_slice(&lang.to_le_bytes());
    buf[10..12].copy_from_slice(&(len as u16).to_le_bytes());
    let n = ioctl(hub, IOCTL_USB_GET_DESCRIPTOR_FROM_NODE_CONNECTION, &mut buf).ok()?;
    if n <= 12 {
        return None;
    }
    Some(buf[12..n].to_vec())
}

fn string(hub: &Hub, port: u32, index: u8, lang: u16) -> Option<String> {
    if index == 0 {
        return None;
    }
    let d = descriptor(hub, port, 3, index, lang, 255)?;
    let n = (d.first().copied()? as usize).min(d.len());
    if n < 2 {
        return None;
    }
    let units: Vec<u16> = d[2..n].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
    let s = String::from_utf16_lossy(&units).trim().to_string();
    (!s.is_empty()).then_some(s)
}

fn bcd(v: u16) -> String {
    let major = (v >> 8) as u8;
    let minor = ((v >> 4) & 0xF) as u8;
    let sub = (v & 0xF) as u8;
    if sub == 0 {
        format!("{:X}.{:X}", major, minor)
    } else {
        format!("{:X}.{:X}.{:X}", major, minor, sub)
    }
}

pub fn describe(instance: &str) -> Option<UsbInfo> {
    let vid = devnode::id_field(instance, "VID")?;
    let pid = devnode::id_field(instance, "PID").unwrap_or(0);
    let mut s = Section::new("USB");
    let vendor_name = names::usb_vendor(vid);
    s.add("Vendor : product", format!("{vid:04X}:{pid:04X}")).note(vendor_name.unwrap_or(""));
    let mut connection = String::from("USB");
    let mut manufacturer = vendor_name.map(String::from);
    let mut product = devnode::get_str(instance, &keys::BUS_REPORTED_DESC);

    let hub_id = devnode::parent(instance);
    let port = devnode::get(instance, &keys::ADDRESS).and_then(|v| v.as_u32());
    let hub_path = hub_id.as_deref().and_then(|h| devnode::interfaces(&GUID_DEVINTERFACE_USB_HUB, Some(h), true).into_iter().next());
    let present = devnode::get(instance, &keys::IS_PRESENT).and_then(|v| v.as_bool()).unwrap_or(false);

    let (Some(hub_path), Some(port), true) = (hub_path, port, present) else {
        if !present {
            s.add("Connection details", "the device is not connected now").tone(Tone::Dim);
        } else {
            s.add("Connection details", "parent hub not found").tone(Tone::Dim);
        }
        return Some(UsbInfo { section: s, connection, manufacturer, product });
    };

    // GENERIC_WRITE is what descriptor requests need; connection info also works with 0.
    let hub = open_hub(&hub_path, GENERIC_WRITE.0).or_else(|| open_hub(&hub_path, 0));
    let Some(hub) = hub else {
        s.add("Connection details", "the hub could not be opened").tone(Tone::Dim);
        return Some(UsbInfo { section: s, connection, manufacturer, product });
    };

    let mut info = vec![0u8; 512];
    info[0..4].copy_from_slice(&port.to_le_bytes());
    let mut speed_label = String::new();
    let mut desc = [0u8; 18];
    if ioctl(&hub, IOCTL_USB_GET_NODE_CONNECTION_INFORMATION_EX, &mut info).is_ok() {
        desc.copy_from_slice(&info[4..22]);
        let speed = info[23];
        speed_label = match speed {
            0 => "Low Speed (1.5 Mb/s)",
            1 => "Full Speed (12 Mb/s)",
            2 => "High Speed (480 Mb/s)",
            3 => "SuperSpeed (5 Gb/s or more)",
            _ => "unknown",
        }
        .to_string();
    }
    let mut v2 = vec![0u8; 16];
    v2[0..4].copy_from_slice(&port.to_le_bytes());
    v2[4..8].copy_from_slice(&16u32.to_le_bytes());
    v2[8..12].copy_from_slice(&7u32.to_le_bytes()); // USB 1.1 | 2.0 | 3.0 understood
    let v2_flags = ioctl(&hub, IOCTL_USB_GET_NODE_CONNECTION_INFORMATION_EX_V2, &mut v2).ok().and_then(|_| le_u32(&v2, 12));
    if let Some(f) = v2_flags {
        if f & 4 != 0 {
            speed_label = "SuperSpeed+ (10 Gb/s or more)".into();
        } else if f & 1 != 0 && !speed_label.starts_with("SuperSpeed") {
            speed_label = "SuperSpeed (5 Gb/s)".into();
        }
    }
    let bcd_usb = u16::from_le_bytes([desc[2], desc[3]]);
    if bcd_usb != 0 {
        s.add("USB version", bcd(bcd_usb)).note("from the device descriptor");
    }
    if !speed_label.is_empty() {
        let mut row = Row::new("Speed", speed_label.clone());
        if let Some(f) = v2_flags {
            if f & 2 != 0 && f & 1 == 0 {
                row.note = "device is SuperSpeed-capable, running slower".into();
                row.tone = Tone::Warn;
            }
        }
        s.push(row);
    }

    let mut props = vec![0u8; 256];
    props[0..4].copy_from_slice(&port.to_le_bytes());
    let port_flags = ioctl(&hub, IOCTL_USB_GET_PORT_CONNECTOR_PROPERTIES, &mut props).ok().and_then(|_| le_u32(&props, 8));
    let hub_name = hub_id.as_deref().and_then(|h| devnode::get_str(h, &keys::FRIENDLY_NAME).or_else(|| devnode::get_str(h, &keys::DEVICE_DESC))).unwrap_or_default();
    let mut port_row = Row::new("Port", format!("{port}"));
    port_row.note = hub_name.clone();
    if let Some(f) = port_flags {
        port_row.child("Connector", if f & 8 != 0 { "USB Type-C" } else { "not Type-C (or not described by firmware)" });
        port_row.child("User-connectable", if f & 1 != 0 { "yes" } else { "no (internal)" });
    }
    s.push(port_row);
    if let Some(loc) = devnode::get_str(instance, &keys::LOCATION_INFO) {
        s.add("Location", loc).tone(Tone::Dim);
    }

    let short_speed = speed_label.split(" (").next().unwrap_or("").to_string();
    connection = format!("USB {} {}", if bcd_usb != 0 { bcd(bcd_usb) } else { String::new() }, short_speed).split_whitespace().collect::<Vec<_>>().join(" ");
    if port_flags.map(|f| f & 8 != 0).unwrap_or(false) {
        connection.push_str(", Type-C port");
    }

    if desc[0] == 18 {
        let class = desc[4];
        s.add("Device class", names::usb_class(class)).note(format!("0x{class:02X}/0x{:02X}/0x{:02X}", desc[5], desc[6]));
        let rel = u16::from_le_bytes([desc[12], desc[13]]);
        s.add("Device release", bcd(rel));
        let langs = descriptor(&hub, port, 3, 0, 0, 255);
        let lang = langs.as_ref().and_then(|l| le_u16(l, 2)).unwrap_or(0x0409);
        if let Some(m) = string(&hub, port, desc[14], lang) {
            s.add("Manufacturer", m.clone()).note("string descriptor");
            manufacturer = Some(m);
        }
        if let Some(p) = string(&hub, port, desc[15], lang) {
            s.add("Product", p.clone()).note("string descriptor");
            product = Some(p);
        }
        if let Some(sn) = string(&hub, port, desc[16], lang) {
            s.add("Serial number", sn);
        }
        if let Some(cfg) = descriptor(&hub, port, 2, 0, 0, 4096) {
            config(&cfg, bcd_usb >= 0x0300, &mut s);
        } else {
            s.add("Configuration", "descriptor not readable from this process").tone(Tone::Dim);
        }
    } else {
        s.add("Descriptors", "not readable from this process").tone(Tone::Dim);
    }
    Some(UsbInfo { section: s, connection, manufacturer, product })
}

fn sync_type(attr: u8) -> &'static str {
    match (attr >> 2) & 3 {
        1 => "asynchronous",
        2 => "adaptive",
        3 => "synchronous",
        _ => "no sync",
    }
}

/// Walks a configuration descriptor: power, interfaces, audio class version and the
/// streaming formats the descriptors themselves state.
fn config(cfg: &[u8], superspeed: bool, s: &mut Section) {
    if cfg.len() < 9 {
        return;
    }
    let attrs = cfg[7];
    let power = cfg[8] as u32 * if superspeed { 8 } else { 2 };
    s.add("Power", format!("{power} mA")).note(if attrs & 0x40 != 0 { "self-powered" } else { "bus-powered" });

    let mut interfaces = Row::new("Interfaces", "");
    let mut uac: Option<String> = None;
    let mut at = 0usize;
    let mut current: Option<(u8, u8, u8, u8)> = None; // number, alt, class, subclass
    let mut count = 0;
    let mut last_label = String::new();
    while at + 2 <= cfg.len() {
        let len = cfg[at] as usize;
        if len < 2 || at + len > cfg.len() {
            break;
        }
        let d = &cfg[at..at + len];
        match d[1] {
            4 if len >= 9 => {
                let (num, alt, class, sub, proto) = (d[2], d[3], d[5], d[6], d[7]);
                current = Some((num, alt, class, sub));
                let what = match (class, sub) {
                    (1, 1) => "Audio control".to_string(),
                    (1, 2) => "Audio streaming".to_string(),
                    (1, 3) => "MIDI streaming".to_string(),
                    (c, _) => names::usb_class(c).to_string(),
                };
                if class == 1 {
                    let v = match proto {
                        0x20 => "UAC 2.0",
                        0x30 => "UAC 3.0",
                        _ => "UAC 1.0",
                    };
                    uac.get_or_insert(v.to_string());
                }
                last_label = format!("#{num}.{alt}");
                if alt == 0 || class != 1 || sub != 2 {
                    interfaces.child(last_label.clone(), what);
                    count += 1;
                }
            }
            0x24 if len >= 4 => {
                if let Some((_, alt, 1, sub)) = current {
                    match (sub, d[2]) {
                        (1, 1) if len >= 5 => {
                            let adc = u16::from_le_bytes([d[3], d[4]]);
                            uac = Some(format!("UAC {}", bcd(adc)));
                        }
                        (2, 2) if len >= 6 && alt != 0 => {
                            // FORMAT_TYPE: UAC2 carries subslot size and bit resolution;
                            // UAC1 Type I also lists its sample rates.
                            let format_type = d[3];
                            if format_type == 1 && len >= 8 && uac.as_deref() == Some("UAC 1.0") {
                                let (ch, sub_bytes, bits, nrates) = (d[4], d[5], d[6], d[7]);
                                let rates: Vec<String> = (0..nrates as usize)
                                    .filter_map(|i| d.get(8 + i * 3..11 + i * 3))
                                    .map(|r| khz(r[0] as u32 | (r[1] as u32) << 8 | (r[2] as u32) << 16))
                                    .collect();
                                interfaces
                                    .child(last_label.clone(), format!("{bits}-bit, {ch} ch"))
                                    .note(format!("{} · {} byte slots", rates.join(", "), sub_bytes));
                                count += 1;
                            } else if len >= 6 {
                                let (sub_bytes, bits) = (d[4], d[5]);
                                interfaces.child(last_label.clone(), format!("{bits}-bit streaming")).note(format!("{sub_bytes}-byte slots"));
                                count += 1;
                            }
                        }
                        _ => {}
                    }
                }
            }
            5 if len >= 7 => {
                if let Some((_, alt, 1, 2)) = current {
                    let attr = d[3];
                    if attr & 3 == 1 && alt != 0 && d[2] & 0x80 == 0 {
                        if let Some(last) = interfaces.sub.last_mut() {
                            if !last.note.contains("sync") {
                                last.note = format!("{}{}isochronous out, {}", last.note, if last.note.is_empty() { "" } else { " · " }, sync_type(attr));
                            }
                        }
                    }
                }
            }
            _ => {}
        }
        at += len;
    }
    if let Some(u) = uac {
        s.add("Audio class", u).tone(Tone::Good);
    }
    interfaces.value = format!("{count}");
    s.push(interfaces);
}
