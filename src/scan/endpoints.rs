//! Core Audio endpoints: identity, formats, volume, sessions, jacks, display sinks,
//! spatial sound, effects, exclusive-mode formats, and the live peak meters.

use std::collections::HashMap;
use std::ffi::c_void;

use windows::core::{Interface, GUID, HSTRING};
use windows::Media::Audio::{SpatialAudioDeviceConfiguration, SpatialAudioFormatSubtype};
use windows::Win32::Devices::Properties::DEVPROPKEY;
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::Media::Audio::Endpoints::{IAudioEndpointVolume, IAudioMeterInformation};
use windows::Win32::Media::Audio::{
    eAll, eCapture, eCommunications, eConsole, eMultimedia, eRender, AudioSessionStateActive, AudioSessionStateExpired,
    IAudioClient, IAudioSessionControl2, IAudioSessionManager2, IDeviceTopology, IMMDevice, IMMDeviceEnumerator,
    IMMEndpoint, IPart, ISimpleAudioVolume, MMDeviceEnumerator, AUDCLNT_SHAREMODE_EXCLUSIVE, DEVICE_STATE,
    DEVICE_STATEMASK_ALL, WAVEFORMATEX,
};
use windows::Win32::Media::KernelStreaming::{
    IKsJackDescription, IKsJackDescription2, IKsJackSinkInformation, KSJACK_DESCRIPTION, KSJACK_SINK_INFORMATION,
};
use windows::Win32::System::Com::{CoCreateInstance, CoTaskMemFree, CLSCTX_ALL, CLSCTX_INPROC_SERVER, STGM_READ};
use windows::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION};
use windows::Win32::UI::Shell::PropertiesSystem::{IPropertyStore, PROPERTYKEY};

use super::names;
use crate::model::{khz, AudioFormat, Endpoint, EpState, Flow, Row, Section, Tone};
use crate::sys::devnode::PropValue;
use crate::sys::registry::{self, Hive};
use crate::sys::{keys, propvar};
use crate::util::{err_text, guid_str, parse_guid, take_pwstr};

/// An endpoint plus what the grouping step needs and the model does not keep.
pub struct EpRaw {
    pub ep: Endpoint,
    pub container: Option<String>,
    pub device_name: Option<String>,
    pub form_code: u32,
    pub jack_subtype: Option<GUID>,
    pub has_sink: bool,
    pub store: Vec<(DEVPROPKEY, PropValue)>,
}

pub struct Audio {
    enumerator: IMMDeviceEnumerator,
    exclusive_cache: HashMap<String, (String, Row)>,
}

fn pk(k: &DEVPROPKEY) -> PROPERTYKEY {
    PROPERTYKEY { fmtid: k.fmtid, pid: k.pid }
}

fn store_get(store: &IPropertyStore, k: &DEVPROPKEY) -> Option<PropValue> {
    let v = unsafe { store.GetValue(&pk(k)) }.ok()?;
    match propvar::decode(&v) {
        PropValue::Empty => None,
        other => Some(other),
    }
}

fn store_all(store: &IPropertyStore) -> Vec<(DEVPROPKEY, PropValue)> {
    let mut out = Vec::new();
    let n = unsafe { store.GetCount() }.unwrap_or(0);
    for i in 0..n {
        let mut key = PROPERTYKEY::default();
        if unsafe { store.GetAt(i, &mut key) }.is_err() {
            continue;
        }
        let dk = DEVPROPKEY { fmtid: key.fmtid, pid: key.pid };
        if let Ok(v) = unsafe { store.GetValue(&key) } {
            out.push((dk, propvar::decode(&v)));
        }
    }
    out
}

/// WAVEFORMATEX or WAVEFORMATEXTENSIBLE bytes.
pub fn parse_waveformat(b: &[u8]) -> Option<AudioFormat> {
    if b.len() < 16 {
        return None;
    }
    let tag = u16::from_le_bytes([b[0], b[1]]);
    let channels = u16::from_le_bytes([b[2], b[3]]);
    let sample_rate = u32::from_le_bytes(b[4..8].try_into().ok()?);
    let bits = u16::from_le_bytes([b[14], b[15]]);
    let mut f = AudioFormat { sample_rate, bits, valid_bits: bits, channels, float: tag == 3, channel_mask: 0 };
    if tag == 0xFFFE && b.len() >= 40 {
        f.valid_bits = u16::from_le_bytes([b[18], b[19]]);
        f.channel_mask = u32::from_le_bytes(b[20..24].try_into().ok()?);
        let sub = u32::from_le_bytes(b[24..28].try_into().ok()?);
        f.float = sub == 3;
    }
    Some(f)
}

unsafe fn waveformat_bytes(p: *const WAVEFORMATEX) -> Vec<u8> {
    let head = std::slice::from_raw_parts(p as *const u8, 18);
    let extra = u16::from_le_bytes([head[16], head[17]]) as usize;
    std::slice::from_raw_parts(p as *const u8, 18 + extra).to_vec()
}

fn extensible(rate: u32, bits: u16, valid: u16, channels: u16, float: bool, mask: u32) -> Vec<u8> {
    let block = channels * (bits / 8);
    let mut b = Vec::with_capacity(40);
    b.extend_from_slice(&0xFFFEu16.to_le_bytes());
    b.extend_from_slice(&channels.to_le_bytes());
    b.extend_from_slice(&rate.to_le_bytes());
    b.extend_from_slice(&(rate * block as u32).to_le_bytes());
    b.extend_from_slice(&block.to_le_bytes());
    b.extend_from_slice(&bits.to_le_bytes());
    b.extend_from_slice(&22u16.to_le_bytes());
    b.extend_from_slice(&valid.to_le_bytes());
    b.extend_from_slice(&mask.to_le_bytes());
    let sub: u128 = if float { 0x00000003_0000_0010_8000_00aa00389b71 } else { 0x00000001_0000_0010_8000_00aa00389b71 };
    let g = GUID::from_u128(sub);
    b.extend_from_slice(&g.data1.to_le_bytes());
    b.extend_from_slice(&g.data2.to_le_bytes());
    b.extend_from_slice(&g.data3.to_le_bytes());
    b.extend_from_slice(&g.data4);
    b
}

fn speaker_mask(mask: u32) -> String {
    const NAMES: [&str; 18] = ["FL", "FR", "FC", "LFE", "BL", "BR", "FLC", "FRC", "BC", "SL", "SR", "TC", "TFL", "TFC", "TFR", "TBL", "TBC", "TBR"];
    let v: Vec<&str> = NAMES.iter().enumerate().filter(|(i, _)| mask & (1 << i) != 0).map(|(_, n)| *n).collect();
    if v.is_empty() {
        "not specified".into()
    } else {
        v.join(" ")
    }
}

/// The endpoint's device format: live while active, otherwise only a stored setting.
fn format_row(f: &AudioFormat, active: bool) -> Row {
    if active {
        Row::new("Shared-mode format", f.short()).with_note(format!("{:.0} kbit/s PCM", f.kbps()))
    } else {
        Row::new("Stored format", f.short()).with_note("Windows' saved setting; the endpoint is not active").with_tone(Tone::Dim)
    }
}

/// The Bluetooth drivers' KS jack is virtual: its "connected" flag is the radio link, and
/// the Bluetooth section already shows the link. Every other driver describes a real jack.
fn wants_jack_section(function: Option<&str>) -> bool {
    !function.is_some_and(crate::model::is_bluetooth_function)
}

fn map_state(s: u32) -> EpState {
    match s & 0xF {
        1 => EpState::Active,
        2 => EpState::Disabled,
        4 => EpState::NotPresent,
        8 => EpState::Unplugged,
        _ => EpState::Unknown,
    }
}

fn ms(hns: i64) -> String {
    format!("{:.2} ms", hns as f64 / 10_000.0)
}

fn process_name(pid: u32) -> String {
    if pid == 0 {
        return "system".into();
    }
    unsafe {
        let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return format!("pid {pid}");
        };
        let mut buf = vec![0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, windows::core::PWSTR(buf.as_mut_ptr()), &mut len).is_ok();
        let _ = CloseHandle(h);
        if !ok {
            return format!("pid {pid}");
        }
        let full = String::from_utf16_lossy(&buf[..len as usize]);
        full.rsplit('\\').next().unwrap_or(&full).to_string()
    }
}

impl Audio {
    pub fn new() -> windows::core::Result<Self> {
        let enumerator: IMMDeviceEnumerator = unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)? };
        Ok(Audio { enumerator, exclusive_cache: HashMap::new() })
    }

    fn defaults(&self) -> HashMap<String, Vec<&'static str>> {
        let mut map: HashMap<String, Vec<&'static str>> = HashMap::new();
        for flow in [eRender, eCapture] {
            for (role, name) in [(eConsole, "Default"), (eMultimedia, "Multimedia"), (eCommunications, "Communications")] {
                if let Ok(d) = unsafe { self.enumerator.GetDefaultAudioEndpoint(flow, role) } {
                    if let Ok(id) = unsafe { d.GetId() } {
                        map.entry(unsafe { take_pwstr(id) }).or_default().push(name);
                    }
                }
            }
        }
        map
    }

    pub fn endpoints(&mut self, full: bool, problems: &mut Vec<String>) -> Vec<EpRaw> {
        let defaults = self.defaults();
        let coll = match unsafe { self.enumerator.EnumAudioEndpoints(eAll, DEVICE_STATE(DEVICE_STATEMASK_ALL)) } {
            Ok(c) => c,
            Err(e) => {
                problems.push(format!("Core Audio endpoint list: {}", err_text(&e)));
                return Vec::new();
            }
        };
        let n = unsafe { coll.GetCount() }.unwrap_or(0);
        let mut out = Vec::with_capacity(n as usize);
        for i in 0..n {
            let Ok(dev) = (unsafe { coll.Item(i) }) else { continue };
            match self.read(&dev, &defaults, full) {
                Ok(r) => out.push(r),
                Err(e) => problems.push(format!("endpoint {i}: {}", err_text(&e))),
            }
        }
        out
    }

    fn read(&mut self, dev: &IMMDevice, defaults: &HashMap<String, Vec<&'static str>>, full: bool) -> windows::core::Result<EpRaw> {
        let id = unsafe { take_pwstr(dev.GetId()?) };
        let state = map_state(unsafe { dev.GetState()? }.0);
        let flow = match dev.cast::<IMMEndpoint>().and_then(|e| unsafe { e.GetDataFlow() }) {
            Ok(f) if f == eCapture => Flow::Capture,
            _ => Flow::Render,
        };
        let store = unsafe { dev.OpenPropertyStore(STGM_READ)? };
        let s = |k: &DEVPROPKEY| store_get(&store, k).and_then(|v| v.as_str().map(String::from)).filter(|x| !x.is_empty());

        let name = s(&keys::PKEY_FRIENDLY_NAME).unwrap_or_else(|| id.clone());
        let desc = s(&keys::PKEY_DEVICE_DESC);
        let device_name = s(&keys::PKEY_EP_DEVICE_NAME);
        let form_code = store_get(&store, &keys::PKEY_FORM_FACTOR).and_then(|v| v.as_u32()).unwrap_or(10);
        let jack_subtype = s(&keys::PKEY_JACK_SUBTYPE).and_then(|g| parse_guid(&g));
        let container = store_get(&store, &keys::CONTAINER_ID).and_then(|v| v.as_guid()).map(|g| guid_str(&g));
        let function = s(&keys::PKEY_EP_FUNCTION).map(|x| x.trim_start_matches("{1}.").to_string());
        let filter = s(&keys::PKEY_EP_FILTER).map(|x| x.trim_start_matches("{2}.").to_string());
        let interface_id = s(&keys::PKEY_EP_INTERFACE_ID);
        let format = store_get(&store, &keys::PKEY_DEVICE_FORMAT).and_then(|v| v.as_bytes().and_then(parse_waveformat));
        let oem = store_get(&store, &keys::PKEY_OEM_FORMAT).and_then(|v| v.as_bytes().and_then(parse_waveformat));

        let mut ep = Endpoint {
            id: id.clone(),
            name,
            flow,
            state,
            form_factor: names::form_factor(form_code).to_string(),
            default_for: defaults.get(&id).cloned().unwrap_or_default(),
            format,
            function: function.clone(),
            filter_interface: filter.clone(),
            sections: Vec::new(),
        };

        // Endpoint identity.
        let mut sec = Section::new("Endpoint");
        sec.add("Direction", flow.label());
        sec.add("State", ep.state_label()).tone(if state == EpState::Active { Tone::Good } else { Tone::Dim });
        sec.add("Form factor", ep.form_factor.clone());
        if let Some(g) = jack_subtype {
            sec.add("Jack type", names::node_type(&g).unwrap_or("unlisted")).note(guid_str(&g));
        }
        if let Some(d) = &desc {
            sec.add("Description", d.clone());
        }
        if !ep.default_for.is_empty() {
            sec.add("Default for", ep.default_for.join(", ")).tone(Tone::Good);
        }
        if let Some(mask) = store_get(&store, &keys::PKEY_PHYSICAL_SPEAKERS).and_then(|v| v.as_u32()) {
            sec.add("Speaker layout", speaker_mask(mask)).note(format!("0x{mask:X}"));
        }
        if let Some(v) = store_get(&store, &keys::PKEY_EVENT_DRIVEN).and_then(|v| v.as_u32()) {
            sec.add("Event-driven mode", if v != 0 { "supported" } else { "not supported" });
        }
        sec.add("Endpoint id", id.clone()).tone(Tone::Dim);
        ep.sections.push(sec);

        // Formats.
        let active = state == EpState::Active;
        let mut fmt = Section::new("Format");
        if let Some(f) = format {
            fmt.push(format_row(&f, active));
        }
        if let Some(f) = oem {
            fmt.add("Driver default", f.short()).tone(Tone::Dim);
        }

        if active {
            if let Ok(client) = unsafe { dev.Activate::<IAudioClient>(CLSCTX_ALL, None) } {
                unsafe {
                    if let Ok(p) = client.GetMixFormat() {
                        if !p.is_null() {
                            if let Some(m) = parse_waveformat(&waveformat_bytes(p)) {
                                fmt.add("Mixer format", m.short()).note("what Windows mixes app audio into");
                            }
                            CoTaskMemFree(Some(p as *const c_void));
                        }
                    }
                    let (mut def, mut min) = (0i64, 0i64);
                    if client.GetDevicePeriod(Some(&mut def), Some(&mut min)).is_ok() {
                        fmt.add("Engine period", ms(def)).note(format!("minimum {}", ms(min)));
                    }
                }
                if let Some(f) = format {
                    fmt.push(self.exclusive(&id, &client, f, full));
                }
            }
        }
        ep.sections.push(fmt);

        if active {
            if let Some(v) = volume(dev) {
                ep.sections.push(v);
            }
            if let Some(s) = sessions(dev, flow) {
                ep.sections.push(s);
            }
        }
        let (jacks, sink) = if active && wants_jack_section(function.as_deref()) { jacks(dev) } else { (None, None) };
        let has_sink = sink.is_some();
        if let Some(j) = jacks {
            ep.sections.push(j);
        }
        if let Some(s) = sink {
            ep.sections.push(s);
        }
        if active && flow == Flow::Render {
            if let Some(iid) = &interface_id {
                if let Some(s) = spatial(iid) {
                    ep.sections.push(s);
                }
            }
        }
        let sysfx = store_get(&store, &keys::PKEY_DISABLE_SYSFX).and_then(|v| v.as_u32());
        if let Some(e) = effects(&id, sysfx) {
            ep.sections.push(e);
        }

        let all = if full { store_all(&store) } else { Vec::new() };
        Ok(EpRaw { ep, container, device_name, form_code, jack_subtype, has_sink, store: all })
    }

    /// Exclusive-mode format support, asked of the driver without opening a stream.
    fn exclusive(&mut self, id: &str, client: &IAudioClient, shared: AudioFormat, full: bool) -> Row {
        let sig = format!("{}/{}/{}", shared.sample_rate, shared.bits, shared.channels);
        if !full {
            if let Some((s, row)) = self.exclusive_cache.get(id) {
                if *s == sig {
                    return row.clone();
                }
            }
        }
        let channels = shared.channels.max(1);
        let mask = if shared.channel_mask != 0 { shared.channel_mask } else if channels == 1 { 0x4 } else { 0x3 };
        let rates = [8000u32, 16000, 32000, 44100, 48000, 88200, 96000, 176400, 192000, 352800, 384000, 768000];
        let depths: [(u16, u16, bool, &str); 5] =
            [(16, 16, false, "16"), (24, 24, false, "24"), (32, 24, false, "24in32"), (32, 32, false, "32"), (32, 32, true, "32f")];
        let mut row = Row::new("Exclusive mode", "");
        let mut supported = 0;
        let mut blocked: Option<String> = None;
        for rate in rates {
            let mut ok_depths = Vec::new();
            for (bits, valid, float, label) in depths {
                let wf = extensible(rate, bits, valid, channels, float, mask);
                let hr = unsafe { client.IsFormatSupported(AUDCLNT_SHAREMODE_EXCLUSIVE, wf.as_ptr() as *const WAVEFORMATEX, None) };
                match hr.0 as u32 {
                    0 => ok_depths.push(label),
                    0x8889000E => blocked = Some("exclusive mode is switched off for this endpoint".into()),
                    0x8889000A => blocked = Some("the device is in use by another application".into()),
                    _ => {}
                }
            }
            if !ok_depths.is_empty() {
                supported += ok_depths.len();
                row.child(khz(rate), ok_depths.join(" · ")).note("bit depths");
            }
        }
        if let Some(b) = blocked.filter(|_| supported == 0) {
            row.value = b;
            row.tone = Tone::Dim;
        } else if supported == 0 {
            row.value = "no standard PCM format accepted".into();
            row.tone = Tone::Dim;
        } else {
            let n = row.sub.len();
            row.value = format!("{n} sample rate{}", if n == 1 { "" } else { "s" });
            row.note = format!("{channels} ch, asked with IsFormatSupported");
        }
        self.exclusive_cache.insert(id.to_string(), (sig, row.clone()));
        row
    }

    pub fn device(&self, id: &str) -> Option<IMMDevice> {
        unsafe { self.enumerator.GetDevice(&HSTRING::from(id)) }.ok()
    }
}

fn volume(dev: &IMMDevice) -> Option<Section> {
    let v = unsafe { dev.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None) }.ok()?;
    let mut s = Section::new("Volume");
    unsafe {
        if let Ok(db) = v.GetMasterVolumeLevel() {
            let pct = v.GetMasterVolumeLevelScalar().map(|x| format!("{:.0} %", x * 100.0)).unwrap_or_default();
            s.add("Level", pct).note(format!("{db:.1} dB"));
        }
        if let Ok(m) = v.GetMute() {
            s.add("Muted", if m.as_bool() { "yes" } else { "no" }).tone(if m.as_bool() { Tone::Warn } else { Tone::Normal });
        }
        let (mut lo, mut hi, mut inc) = (0f32, 0f32, 0f32);
        if v.GetVolumeRange(&mut lo, &mut hi, &mut inc).is_ok() {
            s.add("Range", format!("{lo:.1} … {hi:.1} dB")).note(format!("step {inc:.2} dB"));
        }
        let (mut step, mut count) = (0u32, 0u32);
        if v.GetVolumeStepInfo(&mut step, &mut count).is_ok() {
            s.add("Steps", format!("{step}/{count}"));
        }
        if let Ok(hw) = v.QueryHardwareSupport() {
            let mut parts = Vec::new();
            if hw & 1 != 0 {
                parts.push("volume");
            }
            if hw & 2 != 0 {
                parts.push("mute");
            }
            if hw & 4 != 0 {
                parts.push("meter");
            }
            s.add("In hardware", if parts.is_empty() { "none (software)".to_string() } else { parts.join(", ") });
        }
        if let Ok(c) = v.GetChannelCount() {
            let mut row = Row::new("Channels", c.to_string());
            for ch in 0..c.min(16) {
                if let Ok(x) = v.GetChannelVolumeLevelScalar(ch) {
                    row.child(format!("ch {ch}"), format!("{:.0} %", x * 100.0));
                }
            }
            if c <= 1 {
                row.sub.clear();
            }
            s.push(row);
        }
    }
    Some(s)
}

fn sessions(dev: &IMMDevice, flow: Flow) -> Option<Section> {
    let mgr = unsafe { dev.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None) }.ok()?;
    let en = unsafe { mgr.GetSessionEnumerator() }.ok()?;
    let n = unsafe { en.GetCount() }.unwrap_or(0);
    let mut s = Section::new(if flow == Flow::Render { "Applications playing" } else { "Applications recording" });
    let mut active = 0;
    let mut total = 0;
    for i in 0..n {
        let Ok(ctl) = (unsafe { en.GetSession(i) }) else { continue };
        let Ok(ctl2) = ctl.cast::<IAudioSessionControl2>() else { continue };
        let st = unsafe { ctl2.GetState() }.unwrap_or(AudioSessionStateExpired);
        if st == AudioSessionStateExpired {
            continue;
        }
        total += 1;
        let system = unsafe { ctl2.IsSystemSoundsSession() }.0 == 0;
        let pid = unsafe { ctl2.GetProcessId() }.unwrap_or(0);
        let label = if system { "System sounds".to_string() } else { process_name(pid) };
        let state = if st == AudioSessionStateActive {
            active += 1;
            "active"
        } else {
            "idle"
        };
        let vol = ctl
            .cast::<ISimpleAudioVolume>()
            .ok()
            .and_then(|v| unsafe { v.GetMasterVolume() }.ok().map(|x| format!("{:.0} %", x * 100.0)))
            .unwrap_or_default();
        let muted = ctl.cast::<ISimpleAudioVolume>().ok().and_then(|v| unsafe { v.GetMute() }.ok()).map(|m| m.as_bool()).unwrap_or(false);
        let mut row = Row::new(label, state);
        row.note = if muted { format!("{vol}, muted") } else { vol };
        row.tone = if st == AudioSessionStateActive { Tone::Good } else { Tone::Dim };
        if pid != 0 && !system {
            row.note = format!("{} · pid {pid}", row.note);
        }
        s.push(row);
    }
    if total == 0 {
        s.add("Sessions", "none").tone(Tone::Dim);
    } else {
        s.rows.insert(0, Row::new("Sessions", format!("{active}/{total} active")));
    }
    Some(s)
}

unsafe fn part_activate<T: Interface>(part: &IPart) -> Option<T> {
    let mut p: *mut c_void = std::ptr::null_mut();
    part.Activate(CLSCTX_INPROC_SERVER.0, &T::IID, Some(&mut p)).ok()?;
    if p.is_null() {
        None
    } else {
        Some(T::from_raw(p))
    }
}

fn color_name(rgb: u32) -> Option<&'static str> {
    Some(match rgb & 0xFFFFFF {
        0x000000 => return None,
        0x00FF00 | 0x00B050 | 0x008000 => "green",
        0xFF0000 => "red",
        0x0000FF => "blue",
        0xFF00FF | 0xFFC0CB => "pink",
        0xFFFFFF => "white",
        0xFFFF00 => "yellow",
        0x808080 => "grey",
        0xFFA500 => "orange",
        _ => return None,
    })
}

/// EDID manufacturer id: three 5-bit letters. Drivers store the two EDID bytes in
/// either order, so both are tried; a decode with a letter outside A-Z is rejected.
fn pnp_id(code: u16) -> String {
    let letters = |c: u16| -> Option<String> {
        let parts = [(c >> 10) & 0x1F, (c >> 5) & 0x1F, c & 0x1F];
        if c & 0x8000 != 0 || parts.iter().any(|p| *p == 0 || *p > 26) {
            return None;
        }
        Some(parts.iter().map(|p| (b'A' + *p as u8 - 1) as char).collect())
    };
    letters(code.swap_bytes()).or_else(|| letters(code)).unwrap_or_else(|| "?".into())
}

/// Jack descriptions and HDMI/DisplayPort sink information from the KS topology.
fn jacks(dev: &IMMDevice) -> (Option<Section>, Option<Section>) {
    let Ok(topo) = (unsafe { dev.Activate::<IDeviceTopology>(CLSCTX_ALL, None) }) else { return (None, None) };
    let mut jack_sec = Section::new("Jack");
    let mut sink_sec: Option<Section> = None;
    let count = unsafe { topo.GetConnectorCount() }.unwrap_or(0);
    let mut reported = false;
    for c in 0..count {
        let Ok(conn) = (unsafe { topo.GetConnector(c) }) else { continue };
        let Ok(other) = (unsafe { conn.GetConnectedTo() }) else { continue };
        let Ok(part) = other.cast::<IPart>() else { continue };
        unsafe {
            if let Some(jd) = part_activate::<IKsJackDescription>(&part) {
                let n = jd.GetJackCount().unwrap_or(0);
                let jd2 = part_activate::<IKsJackDescription2>(&part);
                for j in 0..n {
                    let mut d = KSJACK_DESCRIPTION::default();
                    if jd.GetJackDescription(j, &mut d).is_err() {
                        continue;
                    }
                    reported = true;
                    let title = if n > 1 { format!("Jack {}", j + 1) } else { "Connector".to_string() };
                    let mut row = Row::new(title, names::jack_connection(d.ConnectionType.0));
                    row.child("Location", format!("{}, {}", names::jack_geo(d.GeoLocation.0), names::jack_gen(d.GenLocation.0)));
                    row.child("Port", names::jack_port(d.PortConnection.0));
                    if d.Color != 0 {
                        let rgb = d.Color & 0xFFFFFF;
                        row.child("Color", color_name(rgb).map(|n| format!("{n} (#{rgb:06X})")).unwrap_or(format!("#{rgb:06X}")));
                    }
                    let presence = jd2.as_ref().and_then(|x| x.GetJackDescription2(j).ok());
                    let detects = presence.map(|p| p.JackCapabilities & 1 != 0);
                    match detects {
                        Some(false) => {
                            row.child("Plugged in", "not detectable on this jack").tone(Tone::Dim);
                        }
                        _ => {
                            row.child("Plugged in", if d.IsConnected.as_bool() { "yes" } else { "no" })
                                .tone(if d.IsConnected.as_bool() { Tone::Good } else { Tone::Warn });
                        }
                    }
                    if let Some(p) = presence {
                        if p.JackCapabilities & 2 != 0 {
                            row.child("Format change", "dynamic");
                        }
                    }
                    jack_sec.push(row);
                }
            }
            if let Some(si) = part_activate::<IKsJackSinkInformation>(&part) {
                let mut info = KSJACK_SINK_INFORMATION::default();
                if si.GetJackSinkInformation(&mut info).is_ok() {
                    let mut s = Section::new("Display sink");
                    let len = (info.SinkDescriptionLength as usize).min(32);
                    let desc = String::from_utf16_lossy(&info.SinkDescription[..len]);
                    s.add("Sink", if desc.is_empty() { "unnamed".to_string() } else { desc });
                    s.add("Link", if info.ConnType.0 == 1 { "DisplayPort" } else { "HDMI" });
                    if info.ManufacturerId != 0 {
                        s.add("Manufacturer id", pnp_id(info.ManufacturerId)).note(format!("0x{:04X}", info.ManufacturerId));
                    }
                    if info.ProductId != 0 {
                        s.add("Product id", format!("0x{:04X}", info.ProductId));
                    }
                    if info.AudioLatency != 0 {
                        s.add("Audio latency", format!("{} ms", info.AudioLatency)).note("reported by the display");
                    }
                    s.add("HDCP", if info.HDCPCapable.as_bool() { "capable" } else { "no" });
                    s.add("ACP/ISRC", if info.AICapable.as_bool() { "capable" } else { "no" });
                    sink_sec = Some(s);
                }
            }
        }
    }
    if !reported {
        jack_sec.add("Jack information", "not reported by the driver").tone(Tone::Dim);
    }
    (Some(jack_sec), sink_sec)
}

fn spatial(interface_id: &str) -> Option<Section> {
    let cfg = SpatialAudioDeviceConfiguration::GetForDeviceId(&HSTRING::from(interface_id)).ok()?;
    let mut s = Section::new("Spatial sound");
    let supported = cfg.IsSpatialAudioSupported().unwrap_or(false);
    let active = cfg.ActiveSpatialAudioFormat().map(|h| h.to_string()).unwrap_or_default();
    let name = |g: &str| -> String {
        let table: [(&str, fn() -> windows::core::Result<HSTRING>); 6] = [
            ("Windows Sonic for Headphones", SpatialAudioFormatSubtype::WindowsSonic),
            ("Dolby Atmos for Headphones", SpatialAudioFormatSubtype::DolbyAtmosForHeadphones),
            ("Dolby Atmos for Home Theater", SpatialAudioFormatSubtype::DolbyAtmosForHomeTheater),
            ("Dolby Atmos for Speakers", SpatialAudioFormatSubtype::DolbyAtmosForSpeakers),
            ("DTS:X Ultra", SpatialAudioFormatSubtype::DTSXUltra),
            ("DTS:X for Home Theater", SpatialAudioFormatSubtype::DTSXForHomeTheater),
        ];
        for (n, f) in table {
            if f().map(|h| h.to_string().eq_ignore_ascii_case(g)).unwrap_or(false) {
                return n.to_string();
            }
        }
        g.to_string()
    };
    if active.is_empty() || active.trim_matches(|c| c == '{' || c == '}').chars().all(|c| c == '0' || c == '-') {
        s.add("Active format", "off");
    } else {
        s.add("Active format", name(&active)).tone(Tone::Good);
    }
    s.add("Spatial audio", if supported { "supported" } else { "not supported" });
    Some(s)
}

fn effects(id: &str, disable_sysfx: Option<u32>) -> Option<Section> {
    let flow_dir = if id.starts_with("{0.0.1") { "Capture" } else { "Render" };
    let guid = id.split("}.").nth(1)?;
    let path = format!(r"SOFTWARE\Microsoft\Windows\CurrentVersion\MMDevices\Audio\{flow_dir}\{guid}\FxProperties");
    let mut s = Section::new("Enhancements");
    match disable_sysfx {
        Some(1) => s.add("Audio enhancements", "off").note("switched off in Sound settings"),
        Some(_) => s.add("Audio enhancements", "on"),
        None => s.add("Audio enhancements", "not reported").tone(Tone::Dim),
    };
    let Some(key) = registry::open(Hive::LocalMachine, &path) else { return Some(s) };
    const FX: u128 = 0xd04e05a6_594b_4fb6_a80d_01af5eed7d1d;
    const MODES: u128 = 0xd3993a3f_99c2_4402_b5ec_a92a0367664b;
    let slot = |pid: u32| -> Option<&'static str> {
        Some(match pid {
            1 => "Pre-mix effect",
            2 => "Post-mix effect",
            5 => "Stream effect",
            6 => "Mode effect",
            7 => "Endpoint effect",
            11 => "Offload stream effect",
            12 => "Offload mode effect",
            13 => "Stream effects",
            14 => "Mode effects",
            15 => "Endpoint effects",
            19 => "Offload stream effects",
            20 => "Offload mode effects",
            _ => return None,
        })
    };
    let mut apo_rows: Vec<Row> = Vec::new();
    let mut mode_rows: Vec<Row> = Vec::new();
    for (name, value) in key.values() {
        let Some((g, pid)) = name.split_once(',') else { continue };
        let (Some(g), Ok(pid)) = (parse_guid(g), pid.trim().parse::<u32>()) else { continue };
        let items: Vec<String> = match &value {
            registry::RegValue::Sz(x) => vec![x.clone()],
            registry::RegValue::MultiSz(v) => v.clone(),
            _ => continue,
        };
        if g.to_u128() == FX {
            if let Some(label) = slot(pid) {
                for clsid in items.iter().filter_map(|c| parse_guid(c)) {
                    if clsid.to_u128() == 0 {
                        continue;
                    }
                    let cls = guid_str(&clsid);
                    let friendly = registry::open(Hive::ClassesRoot, &format!(r"CLSID\{cls}"))
                        .and_then(|k| k.default_value())
                        .and_then(|v| v.as_str().map(String::from))
                        .unwrap_or_else(|| "unnamed".into());
                    apo_rows.push(Row::new(label, friendly).with_note(cls));
                }
            } else if pid == 4 {
                if let Some(n) = items.first() {
                    apo_rows.push(Row::new("Effects package", n.clone()));
                }
            }
        } else if g.to_u128() == MODES && (5..=7).contains(&pid) {
            let which = ["Stream", "Mode", "Endpoint"][(pid - 5) as usize];
            let modes: Vec<&str> = items.iter().filter_map(|m| parse_guid(m)).map(|m| names::processing_mode(&m).unwrap_or("custom")).collect();
            mode_rows.push(Row::new(format!("{which} modes"), modes.join(", ")));
        }
    }
    if apo_rows.is_empty() {
        s.add("Effect modules", "none registered").tone(Tone::Dim);
    } else {
        s.rows.extend(apo_rows);
    }
    s.rows.extend(mode_rows);
    Some(s)
}

/// Live peak meters for active endpoints, sampled between scans.
pub struct Meters {
    meters: HashMap<String, IAudioMeterInformation>,
}

impl Meters {
    pub fn new() -> Self {
        Meters { meters: HashMap::new() }
    }

    pub fn sample(&mut self, audio: &Audio, ids: &[String]) -> HashMap<String, f32> {
        self.meters.retain(|k, _| ids.contains(k));
        let mut out = HashMap::new();
        for id in ids {
            if !self.meters.contains_key(id) {
                if let Some(d) = audio.device(id) {
                    if let Ok(m) = unsafe { d.Activate::<IAudioMeterInformation>(CLSCTX_ALL, None) } {
                        self.meters.insert(id.clone(), m);
                    }
                }
            }
            if let Some(m) = self.meters.get(id) {
                match unsafe { m.GetPeakValue() } {
                    Ok(v) => {
                        out.insert(id.clone(), v);
                    }
                    Err(_) => {
                        self.meters.remove(id);
                    }
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_bluetooth_shared_format() {
        // PKEY_AudioEngine_DeviceFormat blob without the registry's 8-byte header
        let b = [
            0xfe, 0xff, 0x02, 0x00, 0x80, 0xbb, 0x00, 0x00, 0x00, 0xee, 0x02, 0x00, 0x04, 0x00, 0x10, 0x00, 0x16, 0x00, 0x10, 0x00, 0x03,
            0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x10, 0x00, 0x80, 0x00, 0x00, 0xaa, 0x00, 0x38, 0x9b, 0x71,
        ];
        let f = parse_waveformat(&b).unwrap();
        assert_eq!((f.sample_rate, f.bits, f.channels, f.float), (48000, 16, 2, false));
        assert_eq!(f.kbps(), 1536.0);
        assert_eq!(f.short(), "48 kHz, 16-bit, 2 ch");
    }

    #[test]
    fn built_extensible_format_parses_back() {
        let b = extensible(96000, 32, 24, 2, false, 3);
        let f = parse_waveformat(&b).unwrap();
        assert_eq!((f.sample_rate, f.bits, f.valid_bits, f.channels, f.float), (96000, 32, 24, 2, false));
        assert_eq!(b.len(), 40);
    }

    #[test]
    fn edid_manufacturer_id() {
        // "GSM" (LG) is 0x1E6D big-endian in the EDID; the sink info stores it little-endian.
        assert_eq!(pnp_id(0x6D1E), "GSM");
    }

    #[test]
    fn a_stored_format_is_not_presented_as_live() {
        let f = AudioFormat { sample_rate: 48000, bits: 16, valid_bits: 16, channels: 2, float: false, channel_mask: 3 };
        let live = format_row(&f, true);
        assert_eq!((live.label.as_str(), live.tone), ("Shared-mode format", Tone::Normal));
        let stored = format_row(&f, false);
        assert_eq!((stored.label.as_str(), stored.tone), ("Stored format", Tone::Dim));
        assert!(!stored.note.contains("kbit/s"));
    }

    #[test]
    fn bluetooth_endpoints_get_no_jack_section() {
        // The Bluetooth drivers' KS jack is virtual: its IsConnected is the radio link.
        assert!(!wants_jack_section(Some(r"BTHENUM\{0000110B-0000-1000-8000-00805F9B34FB}_VID&0001000A_PID&FFFF\7&0&0&0123456789AB_C00000000")));
        assert!(!wants_jack_section(Some(r"BTHHFENUM\BthHFPAudio\8&0&0&97")));
        assert!(wants_jack_section(Some(r"HDAUDIO\FUNC_01&VEN_10EC&DEV_1220\4&0&0&0001")));
        assert!(wants_jack_section(Some(r"ROOT\FOCUSRITEUSBNEW\0000")));
        assert!(wants_jack_section(None));
    }
}
