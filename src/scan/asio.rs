//! ASIO drivers: the installed list from the registry, which device each belongs to, and
//! a query of one driver (channels, buffer sizes, sample rates, latencies, clocks).
//!
//! The query loads a third-party driver DLL, so it runs in a child copy of this exe
//! (`--asio-query {CLSID}`): a driver that crashes takes only the child down. It never
//! creates buffers, starts streaming or changes the sample rate.

use std::ffi::c_void;
use std::io::Read;
use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use windows::core::{GUID, PCWSTR};
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
use windows::Win32::UI::WindowsAndMessaging::{CreateWindowExW, DestroyWindow, WINDOW_EX_STYLE, WINDOW_STYLE};

use crate::model::{khz, Device, Row, Section, Tone};
use crate::sys::registry::{self, Hive};
use crate::sys::{devnode, keys};
use crate::util::{parse_guid, wide};

#[derive(Clone, Debug)]
pub struct AsioDriver {
    pub name: String,
    pub clsid: String,
    pub dll: Option<String>,
}

pub fn installed() -> Vec<AsioDriver> {
    let Some(root) = registry::open(Hive::LocalMachine, r"SOFTWARE\ASIO") else { return Vec::new() };
    let mut out = Vec::new();
    for name in root.subkeys() {
        let Some(k) = root.open(&name) else { continue };
        let Some(clsid) = k.get("CLSID").and_then(|v| v.as_str().map(String::from)) else { continue };
        let dll = registry::open(Hive::ClassesRoot, &format!(r"CLSID\{clsid}\InprocServer32"))
            .and_then(|c| c.default_value())
            .and_then(|v| v.as_str().map(String::from));
        let label = k.get("Description").and_then(|v| v.as_str().map(String::from)).filter(|d| !d.is_empty()).unwrap_or(name);
        out.push(AsioDriver { name: label, clsid, dll });
    }
    out
}

/// First meaningful word of a vendor string: "Focusrite Audio Engineering Ltd." -> "focusrite".
fn vendor_word(s: &str) -> Option<String> {
    let w = s.split(|c: char| !c.is_alphanumeric()).find(|w| w.len() >= 4)?.to_lowercase();
    (!matches!(w.as_str(), "microsoft" | "audio" | "device" | "generic")).then_some(w)
}

/// ASIO drivers that name the same vendor as the device's driver, model or name.
pub fn attach(dev: &mut Device) {
    let mut words: Vec<String> = Vec::new();
    if let Some(f) = dev.functions.first() {
        if let Some(p) = devnode::get_str(f, &keys::DRIVER_PROVIDER) {
            words.extend(vendor_word(&p));
        }
    }
    words.extend(dev.manufacturer.as_deref().and_then(vendor_word));
    words.extend(vendor_word(&dev.name));
    words.dedup();
    if words.is_empty() {
        return;
    }
    let drivers: Vec<AsioDriver> = installed().into_iter().filter(|d| words.iter().any(|w| d.name.to_lowercase().contains(w))).collect();
    if drivers.is_empty() {
        return;
    }
    let mut s = Section::new("ASIO");
    for d in drivers {
        let mut row = Row::new(d.name.clone(), "installed");
        row.note = d.clsid.clone();
        if let Some(dll) = &d.dll {
            row.child("DLL", dll.clone());
        }
        s.push(row);
    }
    dev.sections.push(s);
}

pub fn system_sections() -> Vec<Section> {
    let list = installed();
    if list.is_empty() {
        return Vec::new();
    }
    let mut s = Section::new("ASIO drivers");
    for d in list {
        let mut row = Row::new(d.name.clone(), d.clsid.clone());
        if let Some(dll) = &d.dll {
            row.note = dll.clone();
        }
        s.push(row);
    }
    vec![s]
}

fn sample_type(t: i32) -> &'static str {
    match t {
        16 => "16-bit int",
        17 => "24-bit int",
        18 => "32-bit int",
        19 => "32-bit float",
        20 => "64-bit float",
        24 => "32-bit int (16 used)",
        25 => "32-bit int (18 used)",
        26 => "32-bit int (20 used)",
        27 => "32-bit int (24 used)",
        0 => "16-bit int BE",
        1 => "24-bit int BE",
        2 => "32-bit int BE",
        3 => "32-bit float BE",
        4 => "64-bit float BE",
        _ => "other",
    }
}

/// Parent side: runs the child, turns its `key\tvalue` lines into a section.
pub fn query(clsid: &str, name: &str) -> Section {
    let mut s = Section::new(format!("ASIO query · {name}"));
    let exe = match std::env::current_exe() {
        Ok(e) => e,
        Err(e) => {
            s.add("Query", format!("could not find this program's path: {e}")).tone(Tone::Warn);
            return s;
        }
    };
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let child = Command::new(exe).arg("--asio-query").arg(clsid).stdout(Stdio::piped()).stderr(Stdio::null()).creation_flags(CREATE_NO_WINDOW).spawn();
    let mut child = match child {
        Ok(c) => c,
        Err(e) => {
            s.add("Query", format!("could not start: {e}")).tone(Tone::Warn);
            return s;
        }
    };
    let mut out = child.stdout.take().unwrap();
    let reader = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = out.read_to_string(&mut text);
        text
    });
    let deadline = Instant::now() + Duration::from_secs(15);
    let status = loop {
        match child.try_wait() {
            Ok(Some(st)) => break Some(st),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
            _ => {
                let _ = child.kill();
                break None;
            }
        }
    };
    let text = reader.join().unwrap_or_default();
    if status.is_none() {
        s.add("Query", "the driver did not answer within 15 s; the helper process was closed").tone(Tone::Warn);
    } else if status.map(|x| !x.success()).unwrap_or(false) && text.is_empty() {
        s.add("Query", "the helper process ended without output (the driver crashed it)").tone(Tone::Warn);
    }
    let mut inputs = Row::new("Input channels", "");
    let mut outputs = Row::new("Output channels", "");
    let mut clocks = Row::new("Clock sources", "");
    for line in text.lines() {
        let Some((k, v)) = line.split_once('\t') else { continue };
        match k {
            "error" => {
                s.add("Driver says", v).tone(Tone::Warn);
            }
            "driver" => {
                s.add("Driver name", v);
            }
            "version" => {
                s.add("Driver version", v);
            }
            "init" => {
                s.add("Initialised", v).tone(if v == "yes" { Tone::Good } else { Tone::Warn });
            }
            "sample_rate" => {
                s.add("Sample rate", v).tone(Tone::Good);
            }
            "rates" => {
                s.add("Rates it accepts", v);
            }
            "buffer" => {
                s.add("Buffer size", v);
            }
            "latency" => {
                s.add("Latency", v).note("as the driver reports it, before buffers exist");
            }
            "channels" => {
                inputs.value = v.split('/').next().unwrap_or("").to_string();
                outputs.value = v.split('/').nth(1).unwrap_or("").to_string();
            }
            "in" => {
                let mut p = v.splitn(2, '|');
                inputs.child(p.next().unwrap_or(""), p.next().unwrap_or(""));
            }
            "out" => {
                let mut p = v.splitn(2, '|');
                outputs.child(p.next().unwrap_or(""), p.next().unwrap_or(""));
            }
            "clock" => {
                let mut p = v.splitn(2, '|');
                let name = p.next().unwrap_or("").to_string();
                let current = p.next() == Some("current");
                clocks.child(name, if current { "current" } else { "" }).tone(if current { Tone::Good } else { Tone::Normal });
            }
            _ => {}
        }
    }
    for row in [inputs, outputs, clocks] {
        if !row.value.is_empty() || !row.sub.is_empty() {
            if row.value.is_empty() {
                let n = row.sub.len();
                let mut r = row;
                r.value = n.to_string();
                s.push(r);
            } else {
                s.push(row);
            }
        }
    }
    s
}

#[repr(C)]
struct ClockSource {
    index: i32,
    associated_channel: i32,
    associated_group: i32,
    is_current: i32,
    name: [u8; 32],
}

#[repr(C)]
struct ChannelInfo {
    channel: i32,
    is_input: i32,
    is_active: i32,
    group: i32,
    sample_type: i32,
    name: [u8; 32],
}

type P = *mut c_void;

#[repr(C)]
struct AsioVtbl {
    query_interface: usize,
    add_ref: usize,
    release: unsafe extern "system" fn(P) -> u32,
    init: unsafe extern "system" fn(P, P) -> i32,
    get_driver_name: unsafe extern "system" fn(P, *mut u8),
    get_driver_version: unsafe extern "system" fn(P) -> i32,
    get_error_message: unsafe extern "system" fn(P, *mut u8),
    start: usize,
    stop: usize,
    get_channels: unsafe extern "system" fn(P, *mut i32, *mut i32) -> i32,
    get_latencies: unsafe extern "system" fn(P, *mut i32, *mut i32) -> i32,
    get_buffer_size: unsafe extern "system" fn(P, *mut i32, *mut i32, *mut i32, *mut i32) -> i32,
    can_sample_rate: unsafe extern "system" fn(P, f64) -> i32,
    get_sample_rate: unsafe extern "system" fn(P, *mut f64) -> i32,
    set_sample_rate: usize,
    get_clock_sources: unsafe extern "system" fn(P, *mut ClockSource, *mut i32) -> i32,
    set_clock_source: usize,
    get_sample_position: usize,
    get_channel_info: unsafe extern "system" fn(P, *mut ChannelInfo) -> i32,
}

#[link(name = "ole32")]
extern "system" {
    fn CoCreateInstance(rclsid: *const GUID, outer: P, context: u32, riid: *const GUID, ppv: *mut P) -> i32;
}

fn cstr(b: &[u8]) -> String {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    String::from_utf8_lossy(&b[..end]).trim().to_string()
}

/// Child side of the query. Prints `key\tvalue` lines; exit code 0 when the driver loaded.
pub fn run_query(clsid_text: &str) -> i32 {
    let Some(clsid) = parse_guid(clsid_text) else {
        println!("error\tnot a CLSID: {clsid_text}");
        return 2;
    };
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let mut obj: P = std::ptr::null_mut();
        let hr = CoCreateInstance(&clsid, std::ptr::null_mut(), 1, &clsid, &mut obj);
        if hr < 0 || obj.is_null() {
            println!("error\tthe driver could not be loaded (0x{:08X})", hr as u32);
            return 3;
        }
        let vt = &**(obj as *mut *const AsioVtbl);
        let class = wide("STATIC");
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            PCWSTR(class.as_ptr()),
            PCWSTR::null(),
            WINDOW_STYLE(0),
            0,
            0,
            0,
            0,
            HWND::default(),
            None,
            None,
            None,
        )
        .unwrap_or_default();
        let ok = (vt.init)(obj, hwnd.0 as P) != 0;
        println!("init\t{}", if ok { "yes" } else { "no" });
        let mut name = [0u8; 128];
        (vt.get_driver_name)(obj, name.as_mut_ptr());
        println!("driver\t{}", cstr(&name));
        println!("version\t{}", (vt.get_driver_version)(obj));
        if !ok {
            let mut msg = [0u8; 256];
            (vt.get_error_message)(obj, msg.as_mut_ptr());
            println!("error\t{}", cstr(&msg));
        } else {
            let (mut ins, mut outs) = (0i32, 0i32);
            if (vt.get_channels)(obj, &mut ins, &mut outs) == 0 {
                println!("channels\t{ins}/{outs}");
            }
            let (mut min, mut max, mut pref, mut gran) = (0i32, 0i32, 0i32, 0i32);
            let mut rate = 0f64;
            let have_rate = (vt.get_sample_rate)(obj, &mut rate) == 0 && rate > 0.0;
            if have_rate {
                println!("sample_rate\t{}", khz(rate as u32));
            }
            if (vt.get_buffer_size)(obj, &mut min, &mut max, &mut pref, &mut gran) == 0 {
                let ms = |n: i32| if have_rate { format!(" ({:.1} ms)", n as f64 * 1000.0 / rate) } else { String::new() };
                println!("buffer\t{pref} samples{} · range {min}…{max}, step {gran}", ms(pref));
            }
            let rates: Vec<String> = [44100.0, 48000.0, 88200.0, 96000.0, 176400.0, 192000.0, 352800.0, 384000.0]
                .iter()
                .filter(|r| (vt.can_sample_rate)(obj, **r) == 0)
                .map(|r| khz(*r as u32))
                .collect();
            if !rates.is_empty() {
                println!("rates\t{}", rates.join(", "));
            }
            let (mut lin, mut lout) = (0i32, 0i32);
            if (vt.get_latencies)(obj, &mut lin, &mut lout) == 0 {
                let ms = |n: i32| if have_rate { format!(" ({:.1} ms)", n as f64 * 1000.0 / rate) } else { String::new() };
                println!("latency\tin {lin} samples{} · out {lout} samples{}", ms(lin), ms(lout));
            }
            for (is_input, count) in [(1, ins), (0, outs)] {
                for ch in 0..count.min(64) {
                    let mut info = ChannelInfo { channel: ch, is_input, is_active: 0, group: 0, sample_type: 0, name: [0; 32] };
                    if (vt.get_channel_info)(obj, &mut info) == 0 {
                        println!("{}\t{}|{}", if is_input == 1 { "in" } else { "out" }, cstr(&info.name), sample_type(info.sample_type));
                    }
                }
            }
            let mut clocks: Vec<ClockSource> =
                (0..16).map(|_| ClockSource { index: 0, associated_channel: 0, associated_group: 0, is_current: 0, name: [0; 32] }).collect();
            let mut n = clocks.len() as i32;
            if (vt.get_clock_sources)(obj, clocks.as_mut_ptr(), &mut n) == 0 {
                for c in clocks.iter().take(n.clamp(0, 16) as usize) {
                    println!("clock\t{}|{}", cstr(&c.name), if c.is_current != 0 { "current" } else { "" });
                }
            }
        }
        (vt.release)(obj);
        if !hwnd.0.is_null() {
            let _ = DestroyWindow(hwnd);
        }
    }
    0
}
