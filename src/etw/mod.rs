//! Bluetooth capture (administrator): a real-time ETW session on the Bluetooth stack's
//! providers. Every event goes to a log file next to the exe; payloads that carry HCI
//! packets feed the tracker, which reports the negotiated A2DP configuration and the
//! measured bitrate of the media channel.
//!
//! UNVERIFIED on a live elevated run when written (2026-09-14): the session code follows
//! the documented ETW API, the packet parsers are unit-tested, but the BTHPORT event
//! layout is undocumented. The log file is there to close that gap.

pub mod hci;

use std::ffi::c_void;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime};

use windows::core::{GUID, PCWSTR, PWSTR};
use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, ERROR_SUCCESS, WIN32_ERROR};
use windows::Win32::System::Diagnostics::Etw::{
    CloseTrace, ControlTraceW, EnableTraceEx2, OpenTraceW, ProcessTrace, StartTraceW, CONTROLTRACE_HANDLE, EVENT_CONTROL_CODE_ENABLE_PROVIDER,
    EVENT_RECORD, EVENT_TRACE_CONTROL_STOP, EVENT_TRACE_LOGFILEW, EVENT_TRACE_PROPERTIES, EVENT_TRACE_REAL_TIME_MODE, PROCESSTRACE_HANDLE,
    PROCESS_TRACE_MODE_EVENT_RECORD, PROCESS_TRACE_MODE_REAL_TIME, WNODE_FLAG_TRACED_GUID,
};

use crate::model::{Row, Section, Tone};
use crate::util::{fmt_time, hex_bytes_masked, mac, thousands, wide};

const SESSION: &str = "AudioInspector-Bluetooth";

/// The log stops growing here; events are still counted and fed to the tracker.
const LOG_LIMIT: u64 = 50 * 1024 * 1024;

/// Providers of the Bluetooth stack, from Microsoft's BluetoothStack.wprp (busiotools).
const PROVIDERS: [(&str, u128); 2] = [
    ("BTHPORT (HCI)", 0x8a1f9517_3a8c_4a9e_a018_4f17a200f277),
    ("BthA2dp", 0xddb6da39_08a7_4579_8d0c_68011146e205),
];

struct State {
    started: Instant,
    started_at: SystemTime,
    events: u64,
    by_provider: Vec<(String, u64)>,
    tracker: hci::Tracker,
    log: Option<BufWriter<File>>,
    log_path: Option<PathBuf>,
    log_error: Option<String>,
    logged: u64,
    log_bytes: u64,
    enabled: Vec<(String, Result<(), String>)>,
    ended: Option<String>,
}

pub struct Capture {
    state: Option<Arc<Mutex<State>>>,
    control: CONTROLTRACE_HANDLE,
    trace: PROCESSTRACE_HANDLE,
    thread: Option<std::thread::JoinHandle<()>>,
    props: Vec<u8>,
}

fn properties() -> Vec<u8> {
    let size = std::mem::size_of::<EVENT_TRACE_PROPERTIES>() + 2048;
    let mut buf = vec![0u8; size];
    unsafe {
        let p = buf.as_mut_ptr() as *mut EVENT_TRACE_PROPERTIES;
        (*p).Wnode.BufferSize = size as u32;
        (*p).Wnode.Flags = WNODE_FLAG_TRACED_GUID;
        (*p).Wnode.ClientContext = 1;
        (*p).LogFileMode = EVENT_TRACE_REAL_TIME_MODE;
        (*p).BufferSize = 256;
        (*p).MinimumBuffers = 8;
        (*p).MaximumBuffers = 64;
        (*p).FlushTimer = 1;
        (*p).LoggerNameOffset = std::mem::size_of::<EVENT_TRACE_PROPERTIES>() as u32;
    }
    buf
}

fn capture_dir() -> PathBuf {
    let base = std::env::current_exe().ok().and_then(|e| e.parent().map(|p| p.to_path_buf())).unwrap_or_else(std::env::temp_dir);
    base.join("captures")
}

unsafe extern "system" fn on_event(record: *mut EVENT_RECORD) {
    let r = &*record;
    if r.UserContext.is_null() {
        return;
    }
    let state = &*(r.UserContext as *const Mutex<State>);
    let Ok(mut s) = state.lock() else { return };
    s.events += 1;
    let provider = r.EventHeader.ProviderId.to_u128();
    let name = PROVIDERS.iter().find(|(_, g)| *g == provider).map(|(n, _)| *n).unwrap_or("other");
    match s.by_provider.iter_mut().find(|(n, _)| n == name) {
        Some(e) => e.1 += 1,
        None => s.by_provider.push((name.to_string(), 1)),
    }
    let data: &[u8] = if r.UserData.is_null() || r.UserDataLength == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(r.UserData as *const u8, r.UserDataLength as usize)
    };
    let now_ms = s.started.elapsed().as_millis() as u64;
    let hci = s.tracker.feed(now_ms, data);
    // Everything from BthA2dp and the first 20000 BTHPORT events go to the log, up to
    // LOG_LIMIT; kernel addresses in the payloads are masked.
    if s.log_bytes < LOG_LIMIT && (s.logged < 20_000 || name == "BthA2dp") {
        let d = &r.EventHeader.EventDescriptor;
        let line = format!(
            "{:>8} ms  {:<14} id={:<5} op={:<3} task={:<5} len={:<5} {}{}\n",
            now_ms,
            name,
            d.Id,
            d.Opcode,
            d.Task,
            data.len(),
            if hci { "[hci] " } else { "" },
            hex_bytes_masked(data, 96)
        );
        s.logged += 1;
        s.log_bytes += line.len() as u64;
        let full = s.log_bytes >= LOG_LIMIT;
        if let Some(w) = s.log.as_mut() {
            let _ = w.write_all(line.as_bytes());
            if full {
                let _ = w.write_all(b"log limit of 50 MB reached; later events are counted, not logged\n");
            }
        }
    }
}

impl Capture {
    pub fn idle() -> Self {
        Capture { state: None, control: CONTROLTRACE_HANDLE::default(), trace: PROCESSTRACE_HANDLE { Value: u64::MAX }, thread: None, props: Vec::new() }
    }

    pub fn running(&self) -> bool {
        self.thread.is_some()
    }

    pub fn start(&mut self) -> Result<(), String> {
        if self.running() {
            return Ok(());
        }
        if !crate::sys::elevation::is_elevated() {
            return Err("Bluetooth capture needs AudioInspector running as administrator".into());
        }
        let name = wide(SESSION);
        let mut props = properties();
        unsafe {
            // A session left over from a crashed run keeps the name; stop it first.
            let mut stale = properties();
            let _ = ControlTraceW(CONTROLTRACE_HANDLE::default(), PCWSTR(name.as_ptr()), stale.as_mut_ptr() as *mut _, EVENT_TRACE_CONTROL_STOP);
            let mut control = CONTROLTRACE_HANDLE::default();
            let rc: WIN32_ERROR = StartTraceW(&mut control, PCWSTR(name.as_ptr()), props.as_mut_ptr() as *mut EVENT_TRACE_PROPERTIES);
            if rc == ERROR_ALREADY_EXISTS {
                return Err("a trace session with this name is still running; close other AudioInspector windows".into());
            }
            if rc != ERROR_SUCCESS {
                return Err(format!("the trace session did not start (error {})", rc.0));
            }
            self.control = control;
        }

        let stamp = fmt_time(SystemTime::now()).replace([':', ' '], "-");
        let (log, log_path, log_error) = match crate::sys::files::create_new_in(&capture_dir(), &format!("bt-capture-{stamp}"), ".txt") {
            Ok((f, p)) => (Some(BufWriter::new(f)), Some(p), None),
            Err(e) => (None, None, Some(e.to_string())),
        };
        let mut state = State {
            started: Instant::now(),
            started_at: SystemTime::now(),
            events: 0,
            by_provider: Vec::new(),
            tracker: hci::Tracker::default(),
            log,
            log_path,
            log_error,
            logged: 0,
            log_bytes: 0,
            enabled: Vec::new(),
            ended: None,
        };
        if let Some(w) = state.log.as_mut() {
            let _ = writeln!(w, "AudioInspector {} Bluetooth capture, started {}", crate::VERSION, fmt_time(state.started_at));
        }
        for (label, g) in PROVIDERS {
            let guid = GUID::from_u128(g);
            let rc = unsafe { EnableTraceEx2(self.control, &guid, EVENT_CONTROL_CODE_ENABLE_PROVIDER.0, 5, u64::MAX, 0, 0, None) };
            state.enabled.push((label.to_string(), if rc == ERROR_SUCCESS { Ok(()) } else { Err(format!("error {}", rc.0)) }));
        }
        let shared = Arc::new(Mutex::new(state));
        let ctx = Arc::into_raw(shared.clone()) as *mut c_void;

        let mut logfile = EVENT_TRACE_LOGFILEW::default();
        let mut logger_name = wide(SESSION);
        logfile.LoggerName = PWSTR(logger_name.as_mut_ptr());
        logfile.Anonymous1.ProcessTraceMode = PROCESS_TRACE_MODE_REAL_TIME | PROCESS_TRACE_MODE_EVENT_RECORD;
        logfile.Anonymous2.EventRecordCallback = Some(on_event);
        logfile.Context = ctx;
        let trace = unsafe { OpenTraceW(&mut logfile) };
        if trace.Value == u64::MAX {
            unsafe {
                drop(Arc::from_raw(ctx as *const Mutex<State>));
                let _ = ControlTraceW(self.control, PCWSTR::null(), props.as_mut_ptr() as *mut _, EVENT_TRACE_CONTROL_STOP);
            }
            return Err("the trace could not be opened for reading".into());
        }
        self.trace = trace;
        let handle = trace;
        let thread_state = shared.clone();
        let ctx_addr = ctx as usize;
        self.thread = Some(std::thread::spawn(move || {
            let rc = unsafe { ProcessTrace(&[handle], None, None) };
            if let Ok(mut s) = thread_state.lock() {
                s.ended = Some(format!("the session stopped (code {})", rc.0));
            }
            unsafe { drop(Arc::from_raw(ctx_addr as *const Mutex<State>)) };
            let _ = logger_name.len();
        }));
        self.props = props;
        self.state = Some(shared);
        Ok(())
    }

    pub fn stop(&mut self) {
        let Some(t) = self.thread.take() else { return };
        unsafe {
            let _ = CloseTrace(self.trace);
            let _ = ControlTraceW(self.control, PCWSTR::null(), self.props.as_mut_ptr() as *mut _, EVENT_TRACE_CONTROL_STOP);
        }
        let _ = t.join();
        if let Some(s) = &self.state {
            if let Ok(mut s) = s.lock() {
                if let Some(w) = s.log.as_mut() {
                    let _ = w.flush();
                }
            }
        }
    }

    pub fn report(&self) -> Vec<Section> {
        let Some(state) = &self.state else { return Vec::new() };
        let Ok(mut s) = state.lock() else { return Vec::new() };
        if let Some(w) = s.log.as_mut() {
            let _ = w.flush();
        }
        let mut out = Vec::new();
        let mut sec = Section::new("Bluetooth capture");
        let running = self.thread.is_some();
        sec.add("State", if running { "recording" } else { "stopped" }).note(format!("started {}", fmt_time(s.started_at))).tone(if running {
            Tone::Good
        } else {
            Tone::Dim
        });
        for (label, r) in &s.enabled {
            match r {
                Ok(()) => sec.add(format!("Provider {label}"), "enabled"),
                Err(e) => sec.add(format!("Provider {label}"), e.clone()).tone(Tone::Warn),
            };
        }
        let mut ev = Row::new("Events", thousands(s.events));
        for (n, c) in &s.by_provider {
            ev.child(n.clone(), thousands(*c));
        }
        sec.push(ev);
        sec.add("HCI packets recognised", thousands(s.tracker.acl_packets))
            .note(if s.events > 200 && s.tracker.events_with_hci == 0 { "none found in the event payloads - see the log file" } else { "" })
            .tone(if s.events > 200 && s.tracker.events_with_hci == 0 { Tone::Warn } else { Tone::Normal });
        if let Some(p) = &s.log_path {
            sec.add("Log file", p.display().to_string()).tone(Tone::Dim);
        }
        if let Some(e) = &s.log_error {
            sec.add("Log file", "not written").note(e.clone()).tone(Tone::Warn);
        }
        if let Some(e) = &s.ended {
            sec.add("Session", e.clone()).tone(Tone::Dim);
        }
        out.push(sec);

        let now_ms = s.started.elapsed().as_millis() as u64;
        let mut handles: Vec<u16> = s.tracker.links.keys().copied().collect();
        handles.sort();
        for h in handles {
            let link = &s.tracker.links[&h];
            let title = match link.address {
                Some(a) => format!("Bluetooth link {}", mac(a)),
                None => format!("Bluetooth link, handle 0x{h:03X}"),
            };
            let mut ls = Section::new(title);
            if let Some((msg, codec)) = &link.config {
                let (name, rows) = codec.describe();
                let mut row = Row::new("Stream codec", name);
                row.note = format!("from AVDTP {msg}");
                row.tone = Tone::Good;
                for (k, v) in rows {
                    row.child(k, v);
                }
                ls.push(row);
            } else {
                ls.add("Stream codec", "not seen yet").note("configuration is sent when the headset connects or starts a stream").tone(Tone::Dim);
            }
            match link.media_rate(now_ms, 5000) {
                Some((kbps, pps, avg, cid, how)) => {
                    let mut row = Row::new("Measured bitrate", format!("{kbps:.0} kbit/s"));
                    row.note = format!("last 5 s, incl. RTP headers · {how} 0x{cid:04X}");
                    row.tone = Tone::Good;
                    row.child("Packets", format!("{pps:.1}/s"));
                    row.child("Average packet", format!("{avg:.0} bytes"));
                    ls.push(row);
                }
                None => {
                    ls.add("Measured bitrate", "no media traffic in the last 5 s").tone(Tone::Dim);
                }
            }
            let total: u64 = link.totals.values().map(|(b, _)| *b).sum();
            ls.add("Bytes seen", thousands(total));
            out.push(ls);
        }
        out
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        self.stop();
    }
}
