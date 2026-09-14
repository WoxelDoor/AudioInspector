//! The data model: plain data shared by the scanners and the UI. No Windows types.
//!
//! A scan produces a [`Snapshot`]: a list of [`Device`]s, each made of the audio
//! endpoints Windows exposes for it plus sections of rows the UI prints as a table.

use std::collections::HashMap;
use std::time::SystemTime;

/// Everything one scan produced.
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub taken: Option<SystemTime>,
    pub took_ms: u64,
    pub elevated: bool,
    pub devices: Vec<Device>,
    /// Machine-wide sections: Bluetooth radios, ASIO drivers, audio service.
    pub system: Vec<Section>,
    /// Failures not tied to one device.
    pub problems: Vec<String>,
}

/// One physical or virtual audio device as a person thinks of it: the endpoints
/// that share a device container, or - for devices inside the PC's own container
/// (onboard HD Audio, HDMI, virtual drivers) - the endpoints of one KS function.
#[derive(Clone, Debug, Default)]
pub struct Device {
    /// Stable across scans: `container:{GUID}` or `function:<instance id>`.
    pub key: String,
    pub name: String,
    pub kind: Kind,
    pub transport: Transport,
    pub state: EpState,
    pub container_id: Option<String>,
    /// KS function devnodes of the endpoints (`HDAUDIO\FUNC_01...`, `BTHENUM\{0000110B...}...`).
    pub functions: Vec<String>,
    /// Every devnode that belongs to the device.
    pub devnodes: Vec<String>,
    pub bt_address: Option<u64>,

    pub manufacturer: Option<String>,
    pub model: Option<String>,
    /// One line, e.g. "USB 2.0 High Speed, port 9 of hub 1".
    pub connection: Option<String>,
    pub connected_since: Option<SystemTime>,
    pub first_seen: Option<SystemTime>,
    pub battery: Option<Battery>,

    pub endpoints: Vec<Endpoint>,
    /// Device-level sections in display order.
    pub sections: Vec<Section>,
}

impl Device {
    pub fn active_endpoints(&self) -> usize {
        self.endpoints.iter().filter(|e| e.state == EpState::Active).count()
    }

    pub fn state_label(&self) -> &'static str {
        self.state.label_for(self.transport.is_bluetooth())
    }
}

/// Bluetooth audio functions: `BTHENUM\...`, `BTHHFENUM\...`, `BTHLEENUM\...`.
pub fn is_bluetooth_function(instance_id: &str) -> bool {
    instance_id.get(..3).is_some_and(|p| p.eq_ignore_ascii_case("BTH"))
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Kind {
    Headphones,
    Headset,
    Speakers,
    Microphone,
    Interface,
    SoundCard,
    Display,
    DigitalOut,
    Virtual,
    #[default]
    Other,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::Headphones => "Headphones",
            Kind::Headset => "Headset",
            Kind::Speakers => "Speakers",
            Kind::Microphone => "Microphone",
            Kind::Interface => "Audio interface",
            Kind::SoundCard => "Sound card",
            Kind::Display => "Display audio",
            Kind::DigitalOut => "Digital output",
            Kind::Virtual => "Virtual device",
            Kind::Other => "Audio device",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum Transport {
    BluetoothClassic,
    BluetoothLe,
    Usb,
    HdAudio,
    Hdmi,
    Pci,
    Virtual,
    #[default]
    Unknown,
    Other(String),
}

impl Transport {
    pub fn label(&self) -> String {
        match self {
            Transport::BluetoothClassic => "Bluetooth".into(),
            Transport::BluetoothLe => "Bluetooth LE".into(),
            Transport::Usb => "USB".into(),
            Transport::HdAudio => "HD Audio".into(),
            Transport::Hdmi => "HDMI / DisplayPort".into(),
            Transport::Pci => "PCI".into(),
            Transport::Virtual => "Software".into(),
            Transport::Unknown => "Unknown bus".into(),
            Transport::Other(s) => s.clone(),
        }
    }

    pub fn is_bluetooth(&self) -> bool {
        matches!(self, Transport::BluetoothClassic | Transport::BluetoothLe)
    }
}

/// Endpoint state, ordered from most to least present.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum EpState {
    Active,
    Unplugged,
    Disabled,
    NotPresent,
    #[default]
    Unknown,
}

impl EpState {
    pub fn label(self) -> &'static str {
        match self {
            EpState::Active => "Active",
            EpState::Unplugged => "Unplugged",
            EpState::Disabled => "Disabled",
            EpState::NotPresent => "Not connected",
            EpState::Unknown => "Unknown",
        }
    }

    /// A Bluetooth driver reports a dropped link as an unplugged jack; nothing was unplugged.
    pub fn label_for(self, bluetooth: bool) -> &'static str {
        match self {
            EpState::Unplugged if bluetooth => "Not connected",
            s => s.label(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Battery {
    pub percent: u8,
    pub updated: Option<SystemTime>,
    pub source: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Flow {
    #[default]
    Render,
    Capture,
}

impl Flow {
    pub fn label(self) -> &'static str {
        match self {
            Flow::Render => "Output",
            Flow::Capture => "Input",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AudioFormat {
    pub sample_rate: u32,
    pub bits: u16,
    pub valid_bits: u16,
    pub channels: u16,
    pub float: bool,
    pub channel_mask: u32,
}

impl AudioFormat {
    /// PCM data rate in kbit/s, counting the container bits the stream carries.
    pub fn kbps(&self) -> f64 {
        self.sample_rate as f64 * self.bits as f64 * self.channels as f64 / 1000.0
    }

    /// "48 kHz, 16-bit, 2 ch" - float and padded formats say so.
    pub fn short(&self) -> String {
        let depth = if self.float {
            format!("{}-bit float", self.bits)
        } else if self.valid_bits != 0 && self.valid_bits != self.bits {
            format!("{}-bit in {}", self.valid_bits, self.bits)
        } else {
            format!("{}-bit", self.bits)
        };
        format!("{}, {}, {} ch", khz(self.sample_rate), depth, self.channels)
    }
}

/// 44100 -> "44.1 kHz", 48000 -> "48 kHz".
pub fn khz(rate: u32) -> String {
    if rate % 1000 == 0 {
        format!("{} kHz", rate / 1000)
    } else {
        let s = format!("{:.1}", rate as f64 / 1000.0);
        format!("{} kHz", s.trim_end_matches('0').trim_end_matches('.'))
    }
}

#[derive(Clone, Debug, Default)]
pub struct Endpoint {
    /// IMMDevice id, e.g. `{0.0.0.00000000}.{40faccf2-57ea-...}`.
    pub id: String,
    pub name: String,
    pub flow: Flow,
    pub state: EpState,
    pub form_factor: String,
    pub default_for: Vec<&'static str>,
    pub format: Option<AudioFormat>,
    pub function: Option<String>,
    pub filter_interface: Option<String>,
    pub sections: Vec<Section>,
}

impl Endpoint {
    pub fn state_label(&self) -> &'static str {
        self.state.label_for(self.function.as_deref().is_some_and(is_bluetooth_function))
    }
}

#[derive(Clone, Debug, Default)]
pub struct Section {
    pub title: String,
    pub rows: Vec<Row>,
}

impl Section {
    pub fn new(title: impl Into<String>) -> Self {
        Section { title: title.into(), rows: Vec::new() }
    }

    pub fn add(&mut self, label: impl Into<String>, value: impl Into<String>) -> &mut Row {
        self.rows.push(Row::new(label, value));
        self.rows.last_mut().unwrap()
    }

    pub fn push(&mut self, row: Row) {
        self.rows.push(row);
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

#[derive(Clone, Debug, Default)]
pub struct Row {
    pub label: String,
    pub value: String,
    /// Dim detail on the same line.
    pub note: String,
    pub tone: Tone,
    /// Short rows underneath, one entity per row.
    pub sub: Vec<Row>,
}

impl Row {
    pub fn new(label: impl Into<String>, value: impl Into<String>) -> Self {
        Row { label: label.into(), value: value.into(), ..Default::default() }
    }

    pub fn note(&mut self, note: impl Into<String>) -> &mut Self {
        self.note = note.into();
        self
    }

    pub fn tone(&mut self, tone: Tone) -> &mut Self {
        self.tone = tone;
        self
    }

    pub fn child(&mut self, label: impl Into<String>, value: impl Into<String>) -> &mut Row {
        self.sub.push(Row::new(label, value));
        self.sub.last_mut().unwrap()
    }

    /// A long list as sub-rows of `per_line` items, so one row never widens the table.
    pub fn child_list(&mut self, label: impl Into<String>, items: &[&str], per_line: usize) -> &mut Row {
        let label = label.into();
        for (i, chunk) in items.chunks(per_line.max(1)).enumerate() {
            let l = if i == 0 { label.clone() } else { String::new() };
            self.sub.push(Row::new(l, chunk.join(", ")));
        }
        self
    }

    /// Owned-builder forms, for `Row::new(..).with_note(..)` chains.
    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.note = note.into();
        self
    }

    pub fn with_tone(mut self, tone: Tone) -> Self {
        self.tone = tone;
        self
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tone {
    #[default]
    Normal,
    Good,
    Warn,
    Dim,
}

/// Values sampled between scans.
#[derive(Clone, Debug, Default)]
pub struct Live {
    pub peaks: HashMap<String, f32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bluetooth_functions_are_recognised() {
        assert!(is_bluetooth_function(r"BTHENUM\{0000110B-0000-1000-8000-00805F9B34FB}_VID&0001000A_PID&FFFF\7&0&0&0123456789AB_C00000000"));
        assert!(is_bluetooth_function(r"BTHHFENUM\BthHFPAudio\8&0&0&97"));
        assert!(is_bluetooth_function(r"BTHLEENUM\{00001850-0000-1000-8000-00805F9B34FB}_0123456789AB\8&0&0&0"));
        assert!(!is_bluetooth_function(r"HDAUDIO\FUNC_01&VEN_10EC&DEV_1220\4&0&0&0001"));
        assert!(!is_bluetooth_function(r"USB\VID_1235&PID_8211&MI_00\7&0&0&0000"));
        assert!(!is_bluetooth_function(""));
    }

    #[test]
    fn a_bluetooth_link_is_never_called_unplugged() {
        assert_eq!(EpState::Unplugged.label_for(true), "Not connected");
        assert_eq!(EpState::Unplugged.label_for(false), "Unplugged");
        assert_eq!(EpState::Active.label_for(true), "Active");

        let headset = Device { state: EpState::Unplugged, transport: Transport::BluetoothClassic, ..Default::default() };
        assert_eq!(headset.state_label(), "Not connected");
        let jack = Device { state: EpState::Unplugged, transport: Transport::HdAudio, ..Default::default() };
        assert_eq!(jack.state_label(), "Unplugged");

        let ep = Endpoint { state: EpState::Unplugged, function: Some(r"BTHHFENUM\BthHFPAudio\8&0&0&97".into()), ..Default::default() };
        assert_eq!(ep.state_label(), "Not connected");
    }
}
