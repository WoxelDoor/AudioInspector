//! The hardware behind the endpoints: bus and transport, USB / HD Audio / PCI / software
//! identity, times, and the driver of every devnode involved.

use std::collections::HashMap;

use super::endpoints::EpRaw;
use super::{names, usb};
use crate::model::{Device, Row, Section, Tone, Transport};
use crate::sys::devnode;
use crate::sys::keys;

#[derive(Default)]
pub struct Cache {
    usb: HashMap<String, Option<usb::UsbInfo>>,
    /// Present USB and PCI device nodes with their driver provider, for vendor-bus linking.
    hardware_nodes: Vec<(String, String)>,
}

impl Cache {
    fn hardware_nodes(&mut self, full: bool) -> &[(String, String)] {
        if full || self.hardware_nodes.is_empty() {
            self.hardware_nodes = devnode::all_instance_ids(true)
                .into_iter()
                .filter(|id| {
                    let e = devnode::enumerator(id);
                    (e == "USB" && !id.to_uppercase().contains("&MI_")) || e == "PCI"
                })
                .filter_map(|id| devnode::get_str(&id, &keys::DRIVER_PROVIDER).map(|p| (id, p)))
                .collect();
        }
        &self.hardware_nodes
    }
}

fn subsystem_vendor(v: u32) -> Option<&'static str> {
    Some(match v {
        0x1028 => "Dell",
        0x1025 => "Acer",
        0x103C => "HP",
        0x1043 => "ASUS",
        0x10DE => "NVIDIA",
        0x144D => "Samsung",
        0x1458 => "Gigabyte",
        0x1462 => "MSI",
        0x17AA => "Lenovo",
        0x1849 => "ASRock",
        0x3842 => "EVGA",
        0x8086 => "Intel",
        _ => return None,
    })
}

fn is_interface_node(id: &str) -> bool {
    id.to_uppercase().contains("&MI_")
}

/// The function devnodes and their ancestors, nearest first, without repeats.
fn chain(dev: &Device) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for f in &dev.functions {
        for id in std::iter::once(f.clone()).chain(devnode::ancestors(f)) {
            if !out.iter().any(|x| x.eq_ignore_ascii_case(&id)) {
                out.push(id);
            }
        }
    }
    out
}

pub fn enrich(dev: &mut Device, eps: &[EpRaw], cache: &mut Cache, full: bool) {
    let chain = chain(dev);
    let enums: Vec<String> = chain.iter().chain(dev.devnodes.iter()).map(|id| devnode::enumerator(id)).collect();
    let function = dev.functions.first().cloned();
    let function_enum = function.as_deref().map(devnode::enumerator).unwrap_or_default();
    let provider = function.as_deref().and_then(|f| devnode::get_str(f, &keys::DRIVER_PROVIDER));

    // USB device behind the endpoints: on the parent chain, in the container, or - for
    // vendor drivers that hang their audio function off a software bus (Focusrite) -
    // the present USB/PCI device with the same driver provider.
    let mut usb_node = chain.iter().chain(dev.devnodes.iter()).find(|id| devnode::enumerator(id) == "USB" && !is_interface_node(id)).cloned();
    let mut linked: Option<String> = None;
    let root_bus = chain.last().map(|id| devnode::enumerator(id) == "ROOT").unwrap_or(false);
    if usb_node.is_none() && root_bus && function_enum != "ROOT" && function_enum != "SWD" {
        if let Some(p) = provider.as_ref().filter(|p| !p.eq_ignore_ascii_case("Microsoft")) {
            if let Some((id, _)) = cache.hardware_nodes(full).iter().find(|(_, prov)| prov.eq_ignore_ascii_case(p)) {
                linked = Some(id.clone());
                if devnode::enumerator(id) == "USB" {
                    usb_node = Some(id.clone());
                }
            }
        }
    }

    let bt = enums.iter().any(|e| e == "BTHENUM" || e == "BTHHFENUM" || e.starts_with("BTHLE"));
    dev.transport = if enums.iter().any(|e| e.starts_with("BTHLE")) {
        Transport::BluetoothLe
    } else if bt {
        Transport::BluetoothClassic
    } else if usb_node.is_some() {
        Transport::Usb
    } else if function_enum == "HDAUDIO" {
        let vendor = function.as_deref().and_then(|f| devnode::id_field(f, "VEN")).unwrap_or(0);
        if matches!(vendor, 0x10DE | 0x1002 | 0x8086) || eps.iter().any(|e| e.has_sink || e.form_code == 9) {
            Transport::Hdmi
        } else {
            Transport::HdAudio
        }
    } else if enums.iter().any(|e| e == "INTELAUDIO" || e == "ACP") {
        Transport::HdAudio
    } else if linked.is_some() || enums.iter().any(|e| e == "PCI") {
        Transport::Pci
    } else if chain.iter().all(|id| matches!(devnode::enumerator(id).as_str(), "ROOT" | "SWD" | "SW")) && !chain.is_empty() {
        Transport::Virtual
    } else {
        match function_enum.as_str() {
            "" => Transport::Unknown,
            e => Transport::Other(e.to_string()),
        }
    };

    // Times: the node a person would call "the device".
    let main = dev
        .devnodes
        .iter()
        .find(|id| id.to_uppercase().starts_with("BTHENUM\\DEV_"))
        .cloned()
        .or_else(|| usb_node.clone())
        .or_else(|| function.clone());
    if let Some(m) = &main {
        // A software device never "connects": its arrival date is a driver install or a boot.
        if dev.transport != Transport::Virtual {
            dev.connected_since = devnode::get(m, &keys::LAST_ARRIVAL_DATE).and_then(|v| v.as_time());
        }
        dev.first_seen = devnode::get(m, &keys::FIRST_INSTALL_DATE).and_then(|v| v.as_time());
    }

    match dev.transport {
        Transport::Usb => {
            if let Some(u) = usb_node.clone() {
                let info = cache.usb.entry(u.clone()).or_insert_with(|| usb::describe(&u));
                if full {
                    *info = usb::describe(&u);
                }
                if let Some(info) = info.clone() {
                    if info.connection != "USB" {
                        dev.connection = Some(info.connection.clone());
                    }
                    dev.manufacturer = info.manufacturer.clone().or(dev.manufacturer.take());
                    dev.model = info.product.clone().or(dev.model.take());
                    let mut sec = info.section.clone();
                    if linked.is_some() {
                        sec.rows.insert(
                            0,
                            Row::new("Link to endpoints", "same driver vendor")
                                .with_note(format!("the audio function sits on {}'s software bus", provider.clone().unwrap_or_default()))
                                .with_tone(Tone::Dim),
                        );
                    }
                    dev.sections.push(sec);
                }
            }
        }
        Transport::HdAudio | Transport::Hdmi => {
            if let Some(f) = &function {
                let sec = hd_audio(f, &chain, dev);
                dev.sections.push(sec);
            }
        }
        Transport::Pci => {
            if let Some(p) = linked.as_ref().or_else(|| chain.iter().find(|id| devnode::enumerator(id) == "PCI")) {
                dev.sections.push(pci(p));
            }
        }
        Transport::Virtual => {
            let mut s = Section::new("Software device");
            s.add("Hardware", "none - a driver creates this device").tone(Tone::Dim);
            if let Some(p) = &provider {
                s.add("Driver vendor", p.clone());
                dev.manufacturer.get_or_insert(p.clone());
            }
            if let Some(f) = &function {
                if let Some(svc) = devnode::get_str(f, &keys::SERVICE) {
                    s.add("Service", svc);
                }
            }
            dev.sections.push(s);
        }
        _ => {}
    }

    let mut nodes: Vec<String> = chain.clone();
    if let Some(l) = &linked {
        nodes.push(l.clone());
    }
    for id in &dev.devnodes {
        if !nodes.iter().any(|x| x.eq_ignore_ascii_case(id)) && devnode::get_str(id, &keys::SERVICE).is_some() {
            nodes.push(id.clone());
        }
    }
    dev.sections.push(drivers(&nodes));
}

fn hd_audio(function: &str, chain: &[String], dev: &mut Device) -> Section {
    let mut s = Section::new("HD Audio");
    let hwid = devnode::get(function, &keys::HARDWARE_IDS).map(|v| v.as_list()).unwrap_or_default();
    let id = hwid.first().cloned().unwrap_or_else(|| function.to_string());
    let ven = devnode::id_field(&id, "VEN").unwrap_or(0);
    let codec = devnode::id_field(&id, "DEV").unwrap_or(0);
    let vendor = names::pci_vendor(ven).map(String::from).unwrap_or(format!("0x{ven:04X}"));
    let model = if ven == 0x10EC && codec >= 0x200 { format!("ALC{codec:X}") } else { format!("codec 0x{codec:04X}") };
    s.add("Codec", format!("{vendor} {model}")).note(format!("VEN_{ven:04X} DEV_{codec:04X}"));
    dev.manufacturer.get_or_insert(vendor.clone());
    dev.model.get_or_insert(model.clone());
    if let Some(sub) = id.to_uppercase().find("SUBSYS_").map(|i| &id[i + 7..]) {
        let hex: String = sub.chars().take(8).collect();
        if let Ok(v) = u32::from_str_radix(&hex, 16) {
            let board = v >> 16;
            let label = subsystem_vendor(board).map(|n| format!("{n} 0x{:04X}", v & 0xFFFF)).unwrap_or(format!("0x{v:08X}"));
            s.add("Subsystem", label).note(format!("SUBSYS_{hex}"));
        }
    }
    if let Some(r) = devnode::id_field(&id, "REV") {
        s.add("Revision", format!("0x{r:04X}"));
    }
    if let Some(ctrl) = chain.iter().find(|x| devnode::enumerator(x) == "PCI") {
        let name = devnode::get_str(ctrl, &keys::FRIENDLY_NAME).or_else(|| devnode::get_str(ctrl, &keys::DEVICE_DESC)).unwrap_or_default();
        let mut row = Row::new("Controller", name);
        if let Some(loc) = devnode::get_str(ctrl, &keys::LOCATION_INFO) {
            row.note = loc;
        }
        s.push(row);
        dev.connection = Some(format!("{} bus", "PCI Express"));
    }
    s
}

fn pci(id: &str) -> Section {
    let mut s = Section::new("PCI");
    let ven = devnode::id_field(id, "VEN").unwrap_or(0);
    let d = devnode::id_field(id, "DEV").unwrap_or(0);
    s.add("Device", devnode::get_str(id, &keys::FRIENDLY_NAME).or_else(|| devnode::get_str(id, &keys::DEVICE_DESC)).unwrap_or_default());
    s.add("Vendor", names::pci_vendor(ven).map(String::from).unwrap_or(format!("0x{ven:04X}"))).note(format!("VEN_{ven:04X} DEV_{d:04X}"));
    if let Some(loc) = devnode::get_str(id, &keys::LOCATION_INFO) {
        s.add("Location", loc);
    }
    s
}

/// One row per devnode: name, driver vendor and version; the details underneath.
fn drivers(nodes: &[String]) -> Section {
    let mut s = Section::new("Drivers");
    for id in nodes {
        let name = devnode::get_str(id, &keys::FRIENDLY_NAME)
            .or_else(|| devnode::get_str(id, &keys::DEVICE_DESC))
            .unwrap_or_else(|| id.clone());
        let provider = devnode::get_str(id, &keys::DRIVER_PROVIDER).unwrap_or_default();
        let version = devnode::get_str(id, &keys::DRIVER_VERSION).unwrap_or_default();
        let mut row = Row::new(name, format!("{provider} {version}").trim().to_string());
        if row.value.is_empty() {
            row.value = "no driver".into();
            row.tone = Tone::Dim;
        }
        if let Some(d) = devnode::get_str(id, &keys::DRIVER_DESC) {
            row.child("Driver", d);
        }
        if let Some(t) = devnode::get(id, &keys::DRIVER_DATE).and_then(|v| v.as_time()) {
            row.child("Date", crate::util::fmt_time(t).chars().take(10).collect::<String>());
        }
        if let Some(inf) = devnode::get_str(id, &keys::DRIVER_INF_PATH) {
            let section = devnode::get_str(id, &keys::DRIVER_INF_SECTION).map(|x| format!(" [{x}]")).unwrap_or_default();
            row.child("INF", format!("{inf}{section}"));
        }
        if let Some(svc) = devnode::get_str(id, &keys::SERVICE) {
            row.child("Service", svc);
        }
        let stack = devnode::get(id, &keys::STACK).map(|v| v.as_list()).unwrap_or_default();
        if !stack.is_empty() {
            row.child("Stack", stack.iter().map(|x| x.trim_start_matches("\\Driver\\")).collect::<Vec<_>>().join(" → "));
        }
        for (k, label) in [(&keys::UPPER_FILTERS, "Upper filters"), (&keys::LOWER_FILTERS, "Lower filters")] {
            let v = devnode::get(id, k).map(|v| v.as_list()).unwrap_or_default();
            if !v.is_empty() {
                row.child(label, v.join(", "));
            }
        }
        if let Some(p) = devnode::get(id, &keys::PROBLEM_CODE).and_then(|v| v.as_u32()).filter(|p| *p != 0) {
            row.child("Problem code", p.to_string()).tone(Tone::Warn);
        }
        let present = devnode::get(id, &keys::IS_PRESENT).and_then(|v| v.as_bool()).unwrap_or(true);
        if !present {
            row.child("Present", "no").tone(Tone::Dim);
        }
        row.child("Instance", id.clone()).tone(Tone::Dim);
        s.push(row);
    }
    s
}
