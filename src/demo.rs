//! `--demo`: the window filled with invented devices instead of a scan, so screenshots
//! show the interface without anyone's hardware, addresses or serial numbers.
//! Bluetooth addresses here come from the range reserved for documentation (RFC 7042).

use std::collections::HashMap;
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use crate::model::{AudioFormat, Battery, Device, Endpoint, EpState, Flow, Kind, Row, Section, Snapshot, Tone, Transport};
use crate::worker::{Cmd, Shared};

/// Shared state for the window with no scanner behind it; commands go nowhere.
pub fn shared() -> (Sender<Cmd>, Arc<Mutex<Shared>>) {
    let (tx, _rx) = channel();
    let mut peaks = HashMap::new();
    peaks.insert("demo:ep:headphones".to_string(), 0.42);
    let s = Shared { snap: Arc::new(snapshot()), peaks, ..Default::default() };
    (tx, Arc::new(Mutex::new(s)))
}

fn format(rate: u32, bits: u16, channels: u16) -> AudioFormat {
    AudioFormat { sample_rate: rate, bits, valid_bits: bits, channels, float: false, channel_mask: if channels == 1 { 4 } else { 3 } }
}

fn overview(name: &str, kind: &str, connection: &str, active: usize, total: usize) -> Section {
    let mut s = Section::new("Overview");
    s.add("Name", name);
    s.add("Type", kind);
    s.add("Connection", connection);
    s.add("State", "Active").note(format!("{active}/{total} endpoints active")).tone(Tone::Good);
    s
}

fn endpoint(id: &str, name: &str, flow: Flow, form: &str, f: AudioFormat, default_for: Vec<&'static str>, extra: Vec<Section>) -> Endpoint {
    let mut ep = Section::new("Endpoint");
    ep.add("Direction", flow.label());
    ep.add("State", "Active").tone(Tone::Good);
    ep.add("Form factor", form);
    if !default_for.is_empty() {
        ep.add("Default for", default_for.join(", ")).tone(Tone::Good);
    }
    ep.add("Event-driven mode", "supported");
    let mut fmt = Section::new("Format");
    fmt.add("Shared-mode format", f.short()).note(format!("{:.0} kbit/s PCM", f.kbps()));
    fmt.add("Mixer format", format!("{}, 32-bit float, {} ch", crate::model::khz(f.sample_rate), f.channels)).note("what Windows mixes app audio into");
    fmt.add("Engine period", "10.00 ms").note("minimum 3.00 ms");
    let mut sections = vec![ep, fmt];
    sections.extend(extra);
    Endpoint {
        id: id.into(),
        name: name.into(),
        flow,
        state: EpState::Active,
        form_factor: form.into(),
        default_for,
        format: Some(f),
        function: None,
        filter_interface: None,
        sections,
    }
}

fn headphones() -> Device {
    let mut ov = overview("Wireless Headphones", "Headphones", "Bluetooth · via Bluetooth 5.3 Adapter", 2, 2);
    ov.add("Battery", "80 %").note("reported over Hands-Free · updated 12 s ago").tone(Tone::Good);
    ov.add("Connected since", "2026-01-15 09:41:07").note("25 min ago");
    ov.add("First seen", "2025-11-02 18:20:44");

    let mut bt = Section::new("Bluetooth");
    bt.add("Address", "00:00:5E:00:53:01");
    bt.add("Link", "connected").tone(Tone::Good);
    bt.add("Class of device", "Audio/Video / Headphones").note("0x240418").child("Services", "Rendering, Audio");
    let mut ver = Row::new("Bluetooth version", "5.3").with_note("LMP 12 · read from the radio now");
    ver.child("Controller maker", "Airoha Technology (0x0094)");
    ver.child_list("Features", &["EDR 2 Mb/s", "EDR 3 Mb/s", "eSCO", "AFH", "Secure Simple Pairing", "LE"], 4);
    bt.push(ver);
    bt.add("Pairing", "Secure Simple Pairing").child("MITM protection", "no");
    let mut services = Row::new("Services", "4/4 enabled");
    for name in ["Audio Sink", "A/V Remote Control Target", "A/V Remote Control", "Hands-Free"] {
        services.child(name, "enabled");
    }
    bt.push(services);
    bt.add("Audio connections", "12").note("A2DP ConnectionCount");

    let mut codec = Section::new("Audio codec");
    codec.add("Music (A2DP)", "AAC").note("undocumented Windows property").tone(Tone::Good);
    codec.add("Headset offers", "aptX, AAC, SBC");
    codec.add("Into the encoder", "48 kHz, 16-bit, 2 ch").note("1536 kbit/s PCM before compression");
    codec.add("Bitrate on air", "not reported by Windows").note("no Windows API carries it").tone(Tone::Dim);
    codec.add("Calls (Hands-Free)", "mSBC, wide band").note("inferred from the 16 kHz call endpoint");

    let mut profiles = Section::new("Profiles");
    profiles.add("Hands-Free", "1.8").note("echo cancel/noise reduction · remote volume · wide band speech · RFCOMM 2");
    profiles.add("A2DP sink", "1.3").note("headphone");
    profiles.add("AVRCP target", "1.6").note("category 2 monitor/amplifier (absolute volume)");
    profiles.add("AVRCP controller", "1.6").note("category 1");

    let mut radio = Section::new("Bluetooth adapter");
    radio.add("Adapter", "Bluetooth 5.3 Adapter");
    radio.add("Bluetooth version", "5.3").note("LMP 12");
    radio.add("Address", "00:00:5E:00:53:02");

    let mut vol = Section::new("Volume");
    vol.add("Level", "50 %").note("-10.3 dB");
    vol.add("Muted", "no");
    vol.add("In hardware", "volume, mute");
    let mut apps = Section::new("Applications playing");
    apps.add("Sessions", "1/2 active");
    apps.add("player.exe", "active").note("100 % · pid 4242").tone(Tone::Good);
    apps.add("System sounds", "idle").note("100 %").tone(Tone::Dim);
    let mut spatial = Section::new("Spatial sound");
    spatial.add("Active format", "off");
    spatial.add("Spatial audio", "supported");

    Device {
        key: "demo:headphones".into(),
        name: "Wireless Headphones".into(),
        kind: Kind::Headphones,
        transport: Transport::BluetoothClassic,
        state: EpState::Active,
        battery: Some(Battery { percent: 80, updated: None, source: "reported over Hands-Free".into() }),
        endpoints: vec![
            endpoint(
                "demo:ep:headphones",
                "Headphones (Wireless Headphones)",
                Flow::Render,
                "Headphones",
                format(48000, 16, 2),
                vec!["Default", "Multimedia"],
                vec![vol, apps, spatial],
            ),
            endpoint("demo:ep:headset", "Headset (Wireless Headphones Hands-Free)", Flow::Capture, "Headset", format(16000, 16, 1), vec!["Communications"], vec![]),
        ],
        sections: vec![ov, bt, codec, profiles, radio],
        ..Default::default()
    }
}

fn interface() -> Device {
    let mut ov = overview("USB Audio Interface", "Audio interface", "USB 2.0 High Speed", 2, 2);
    ov.add("Manufacturer", "Example Audio");
    let mut usb = Section::new("USB");
    usb.add("USB version", "2.0").note("from the device descriptor");
    usb.add("Speed", "High Speed (480 Mb/s)");
    usb.add("Power", "500 mA").note("bus-powered");
    let mut ifs = Row::new("Interfaces", "3");
    ifs.child("#0.0", "Audio control");
    ifs.child("#1.1", "24-bit streaming").note("3-byte slots · isochronous out, asynchronous");
    ifs.child("#2.1", "24-bit streaming").note("3-byte slots");
    usb.push(ifs);
    usb.add("Audio class", "UAC 2.0").tone(Tone::Good);
    Device {
        key: "demo:interface".into(),
        name: "USB Audio Interface".into(),
        kind: Kind::Interface,
        transport: Transport::Usb,
        state: EpState::Active,
        endpoints: vec![
            endpoint("demo:ep:iface-out", "Speakers (USB Audio Interface)", Flow::Render, "Speakers", format(48000, 24, 2), vec![], vec![]),
            endpoint("demo:ep:iface-in", "Line In (USB Audio Interface)", Flow::Capture, "Line level", format(48000, 24, 2), vec!["Default"], vec![]),
        ],
        sections: vec![ov, usb],
        ..Default::default()
    }
}

fn onboard() -> Device {
    let ov = overview("Onboard Audio", "Sound card", "HD Audio · PCI Express bus", 1, 3);
    let mut hda = Section::new("HD Audio");
    hda.add("Codec", "Realtek ALC897").note("VEN_10EC DEV_0897");
    Device {
        key: "demo:onboard".into(),
        name: "Onboard Audio".into(),
        kind: Kind::SoundCard,
        transport: Transport::HdAudio,
        state: EpState::Active,
        endpoints: vec![endpoint("demo:ep:onboard", "Speakers (Onboard Audio)", Flow::Render, "Speakers", format(48000, 24, 2), vec![], vec![])],
        sections: vec![ov, hda],
        ..Default::default()
    }
}

fn monitor() -> Device {
    let ov = overview("Monitor", "Display audio", "HDMI / DisplayPort", 1, 1);
    let mut sink = Section::new("Display sink");
    sink.add("Sink", "Monitor");
    sink.add("Link", "HDMI");
    sink.add("HDCP", "capable");
    Device {
        key: "demo:monitor".into(),
        name: "Monitor".into(),
        kind: Kind::Display,
        transport: Transport::Hdmi,
        state: EpState::Active,
        endpoints: vec![endpoint("demo:ep:monitor", "Monitor (HDMI)", Flow::Render, "Display (HDMI/DP)", format(48000, 16, 2), vec![], vec![])],
        sections: vec![ov, sink],
        ..Default::default()
    }
}

pub fn snapshot() -> Snapshot {
    let mut radio = Section::new("Bluetooth");
    radio.add("Adapter", "Bluetooth 5.3 Adapter");
    radio.add("Radio", "on").tone(Tone::Good);
    Snapshot {
        taken: Some(SystemTime::now()),
        took_ms: 640,
        elevated: false,
        devices: vec![headphones(), interface(), onboard(), monitor()],
        system: vec![radio],
        problems: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every `xx:xx:xx:xx:xx:xx` in the text.
    fn macs(text: &str) -> Vec<String> {
        let b = text.as_bytes();
        let mut out = Vec::new();
        for i in 0..b.len().saturating_sub(16) {
            let w = &b[i..i + 17];
            let shaped = (0..17).all(|j| if j % 3 == 2 { w[j] == b':' } else { w[j].is_ascii_hexdigit() });
            if shaped {
                out.push(String::from_utf8_lossy(w).into_owned());
            }
        }
        out
    }

    #[test]
    fn demo_data_holds_only_documentation_addresses() {
        let text = crate::report::full(&snapshot(), true);
        let found = macs(&text);
        assert!(!found.is_empty(), "the demo should show what an address looks like");
        for m in found {
            assert!(m.starts_with("00:00:5E:00:53:"), "{m} is not an RFC 7042 documentation address");
        }
        assert!(!text.contains(r"\\?\"), "no device interface paths");
    }
}
