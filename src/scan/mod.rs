//! One scan of the machine: endpoints -> devices -> hardware, Bluetooth, ASIO -> overview.

pub mod asio;
pub mod bluetooth;
pub mod endpoints;
pub mod hardware;
pub mod names;
pub mod raw;
pub mod sdp;
pub mod usb;

use std::collections::HashMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::time::{Instant, SystemTime};

use crate::model::{Device, EpState, Flow, Kind, Row, Section, Snapshot, Tone, Transport};
use crate::sys::{devnode, elevation, keys};
use crate::util::{fmt_ago, fmt_time};
use endpoints::{Audio, EpRaw, Meters};

const LOCAL_MACHINE_CONTAINER: &str = "{00000000-0000-0000-FFFF-FFFFFFFFFFFF}";

pub struct Scanner {
    audio: Option<Audio>,
    meters: Meters,
    /// container id -> devnodes, rebuilt on full scans.
    containers: HashMap<String, Vec<String>>,
    raw: HashMap<String, Vec<Section>>,
    pub hw: hardware::Cache,
    pub bt: bluetooth::Cache,
    init_error: Option<String>,
}

impl Scanner {
    pub fn new() -> Self {
        let (audio, init_error) = match Audio::new() {
            Ok(a) => (Some(a), None),
            Err(e) => (None, Some(format!("Core Audio is not available: {}", crate::util::err_text(&e)))),
        };
        Scanner {
            audio,
            meters: Meters::new(),
            containers: HashMap::new(),
            raw: HashMap::new(),
            hw: hardware::Cache::default(),
            bt: bluetooth::Cache::default(),
            init_error,
        }
    }

    fn index_containers(&mut self) {
        let mut map: HashMap<String, Vec<String>> = HashMap::new();
        for id in devnode::all_instance_ids(false) {
            if let Some(g) = devnode::get(&id, &keys::CONTAINER_ID).and_then(|v| v.as_guid()) {
                map.entry(crate::util::guid_str(&g)).or_default().push(id);
            }
        }
        self.containers = map;
    }

    pub fn scan(&mut self, full: bool) -> Snapshot {
        let t0 = Instant::now();
        let mut snap = Snapshot { elevated: elevation::is_elevated(), ..Default::default() };
        if let Some(e) = &self.init_error {
            snap.problems.push(e.clone());
        }
        if full || self.containers.is_empty() {
            self.index_containers();
        }
        let eps = match self.audio.as_mut() {
            Some(a) => a.endpoints(full, &mut snap.problems),
            None => Vec::new(),
        };

        let mut devices = group(eps, &self.containers);
        for (dev, eps) in devices.iter_mut() {
            let key = dev.key.clone();
            let result = catch_unwind(AssertUnwindSafe(|| {
                hardware::enrich(dev, eps, &mut self.hw, full);
                if dev.bt_address.is_some() || matches!(dev.transport, Transport::BluetoothClassic | Transport::BluetoothLe) {
                    bluetooth::enrich(dev, eps, &mut self.bt, full);
                }
                asio::attach(dev);
            }));
            if result.is_err() {
                snap.problems.push(format!("{}: a scanner stopped on this device; the rows shown are partial", dev.name));
            }
            if full {
                self.raw.insert(key.clone(), raw::sections(dev, eps));
            }
            finish(dev, eps);
            if let Some(r) = self.raw.get(&key) {
                dev.sections.extend(r.iter().cloned());
            }
        }

        let mut devices: Vec<Device> = devices.into_iter().map(|(d, _)| d).collect();
        devices.sort_by(|a, b| {
            a.state.cmp(&b.state).then(kind_rank(a.kind).cmp(&kind_rank(b.kind))).then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        snap.devices = devices;

        let r = catch_unwind(AssertUnwindSafe(|| {
            let mut sys = bluetooth::system_sections(&mut self.bt);
            sys.extend(asio::system_sections());
            sys
        }));
        match r {
            Ok(s) => snap.system = s,
            Err(_) => snap.problems.push("system sections: a scanner stopped; the rows shown are partial".into()),
        }
        snap.taken = Some(SystemTime::now());
        snap.took_ms = t0.elapsed().as_millis() as u64;
        snap
    }

    pub fn sample_meters(&mut self, ids: &[String]) -> HashMap<String, f32> {
        match &self.audio {
            Some(a) => self.meters.sample(a, ids),
            None => HashMap::new(),
        }
    }
}

fn kind_rank(k: Kind) -> u8 {
    match k {
        Kind::Headphones => 0,
        Kind::Headset => 1,
        Kind::Interface => 2,
        Kind::Speakers => 3,
        Kind::Microphone => 4,
        Kind::SoundCard => 5,
        Kind::Display => 6,
        Kind::DigitalOut => 7,
        Kind::Other => 8,
        Kind::Virtual => 9,
    }
}

/// Endpoints to devices: by container, or by KS function inside the PC's own container.
fn group(eps: Vec<EpRaw>, containers: &HashMap<String, Vec<String>>) -> Vec<(Device, Vec<EpRaw>)> {
    let mut order: Vec<String> = Vec::new();
    let mut groups: HashMap<String, (Device, Vec<EpRaw>)> = HashMap::new();
    for raw in eps {
        // Onboard codecs, display audio and software devices hand every jack or monitor its
        // own container, so those group by their KS function; Bluetooth, USB and vendor
        // buses group by container, which spans a headset's A2DP and Hands-Free functions.
        let by_function = raw
            .ep
            .function
            .as_deref()
            .map(|f| matches!(devnode::enumerator(f).as_str(), "HDAUDIO" | "INTELAUDIO" | "ROOT" | "SW" | "SWD" | "ACPI" | "PCI"))
            .unwrap_or(false);
        let container = raw.container.clone().filter(|c| !c.eq_ignore_ascii_case(LOCAL_MACHINE_CONTAINER) && !by_function);
        let key = match (&container, &raw.ep.function) {
            (Some(c), _) => format!("container:{}", c.to_uppercase()),
            (None, Some(f)) => format!("function:{}", f.to_uppercase()),
            (None, None) => format!("endpoint:{}", raw.ep.id),
        };
        let entry = groups.entry(key.clone()).or_insert_with(|| {
            order.push(key.clone());
            let mut d = Device { key: key.clone(), container_id: container.clone(), ..Default::default() };
            if let Some(c) = &container {
                d.devnodes = containers.get(&c.to_uppercase()).cloned().unwrap_or_default();
            }
            (d, Vec::new())
        });
        if let Some(f) = &raw.ep.function {
            if !entry.0.functions.iter().any(|x| x.eq_ignore_ascii_case(f)) {
                entry.0.functions.push(f.clone());
            }
        }
        entry.1.push(raw);
    }
    let mut out = Vec::new();
    for key in order {
        let (mut dev, eps) = groups.remove(&key).unwrap();
        if dev.container_id.is_none() {
            for f in dev.functions.clone() {
                dev.devnodes.push(f.clone());
                dev.devnodes.extend(devnode::ancestors(&f));
            }
        }
        dev.devnodes.dedup();
        dev.state = eps.iter().map(|e| e.ep.state).min().unwrap_or_default();
        dev.name = device_name(&dev, &eps);
        dev.bt_address = dev.devnodes.iter().find_map(|id| {
            devnode::get_str(id, &keys::BT_ADDRESS).and_then(|s| u64::from_str_radix(&s, 16).ok()).filter(|a| *a != 0)
        });
        let mut sorted = eps;
        sorted.sort_by(|a, b| {
            a.ep.state.cmp(&b.ep.state).then((a.ep.flow == Flow::Capture).cmp(&(b.ep.flow == Flow::Capture))).then(a.ep.name.cmp(&b.ep.name))
        });
        out.push((dev, sorted));
    }
    out
}

fn device_name(dev: &Device, eps: &[EpRaw]) -> String {
    let mut counts: Vec<(String, usize)> = Vec::new();
    for e in eps {
        if let Some(n) = &e.device_name {
            match counts.iter_mut().find(|(x, _)| x == n) {
                Some(c) => c.1 += 1,
                None => counts.push((n.clone(), 1)),
            }
        }
    }
    counts.sort_by(|a, b| b.1.cmp(&a.1));
    if let Some((n, _)) = counts.first() {
        // "Acme Buds Hands-Free" loses to "Acme Buds" when both exist.
        let shortest = counts.iter().map(|(n, _)| n).filter(|x| n.starts_with(x.as_str()) || x.starts_with(n.as_str())).min_by_key(|x| x.len());
        return shortest.cloned().unwrap_or_else(|| n.clone());
    }
    for f in &dev.functions {
        if let Some(n) = devnode::get_str(f, &keys::FRIENDLY_NAME).or_else(|| devnode::get_str(f, &keys::DEVICE_DESC)) {
            return n;
        }
    }
    eps.first().map(|e| e.ep.name.clone()).unwrap_or_else(|| "Audio device".into())
}

fn classify_kind(dev: &Device, eps: &[EpRaw]) -> Kind {
    if dev.transport == Transport::Virtual {
        return Kind::Virtual;
    }
    let category: Vec<String> = dev.devnodes.iter().flat_map(|id| devnode::get(id, &keys::CONTAINER_CATEGORY).map(|v| v.as_list()).unwrap_or_default()).collect();
    if category.iter().any(|c| c.eq_ignore_ascii_case("Audio.Headphone")) {
        return Kind::Headphones;
    }
    if category.iter().any(|c| c.eq_ignore_ascii_case("Audio.Headset")) {
        return Kind::Headset;
    }
    let has = |code: u32| eps.iter().any(|e| e.form_code == code);
    let renders = eps.iter().any(|e| e.ep.flow == Flow::Render);
    let captures = eps.iter().any(|e| e.ep.flow == Flow::Capture);
    if dev.transport == Transport::Hdmi || has(9) || eps.iter().any(|e| e.has_sink) {
        return Kind::Display;
    }
    if dev.transport == Transport::HdAudio {
        return Kind::SoundCard;
    }
    if has(3) {
        return if captures { Kind::Headset } else { Kind::Headphones };
    }
    if has(5) {
        return Kind::Headset;
    }
    if renders && captures {
        return Kind::Interface;
    }
    if captures && !renders {
        return Kind::Microphone;
    }
    if has(1) || has(2) {
        return Kind::Speakers;
    }
    if has(8) || has(7) {
        return Kind::DigitalOut;
    }
    Kind::Other
}

/// Kind, state and the Overview section, composed after every enricher ran.
fn finish(dev: &mut Device, eps: &mut [EpRaw]) {
    dev.kind = classify_kind(dev, eps);
    let mut ov = Section::new("Overview");
    ov.add("Name", dev.name.clone());
    ov.add("Type", dev.kind.label());
    let label = dev.transport.label();
    let conn = match &dev.connection {
        Some(c) if c.starts_with(&label) => c.clone(),
        Some(c) => format!("{label} · {c}"),
        None => label,
    };
    ov.add("Connection", conn);
    let active = eps.iter().filter(|e| e.ep.state == EpState::Active).count();
    ov.add("State", dev.state_label())
        .note(format!("{active}/{} endpoints active", eps.len()))
        .tone(if dev.state == EpState::Active { Tone::Good } else { Tone::Dim });
    if let Some(b) = &dev.battery {
        let mut row = Row::new("Battery", format!("{} %", b.percent));
        row.tone = if b.percent <= 15 { Tone::Warn } else { Tone::Good };
        row.note = match b.updated {
            Some(t) => format!("{} · updated {}", b.source, fmt_ago(t)),
            None => b.source.clone(),
        };
        ov.push(row);
    }
    if let Some(m) = &dev.manufacturer {
        ov.add("Manufacturer", m.clone());
    }
    if let Some(m) = &dev.model {
        ov.add("Model", m.clone());
    }
    if let Some(t) = dev.connected_since {
        // The devnode of a remembered or switched-off device keeps its last arrival date.
        let label = if dev.state == EpState::Active { "Connected since" } else { "Last connected" };
        ov.add(label, fmt_time(t)).note(fmt_ago(t));
    }
    if let Some(t) = dev.first_seen {
        ov.add("First seen", fmt_time(t));
    }
    if let Some(c) = &dev.container_id {
        ov.add("Container", c.clone()).tone(Tone::Dim);
    }
    dev.sections.insert(0, ov);
    dev.endpoints = eps.iter_mut().map(|e| std::mem::take(&mut e.ep)).collect();
}
