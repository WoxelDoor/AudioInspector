//! The background thread that owns COM and the scanner: periodic scans, live meters,
//! ASIO queries and the Bluetooth capture, published into shared state for the UI.

use std::collections::HashMap;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::model::{EpState, Section, Snapshot};
use crate::scan::{asio, Scanner};

pub enum Cmd {
    Refresh,
    Select(Option<String>),
    AsioQuery { clsid: String, name: String },
    CaptureStart,
    CaptureStop,
}

#[derive(Default)]
pub struct Shared {
    pub snap: Arc<Snapshot>,
    pub peaks: HashMap<String, f32>,
    pub scanning: bool,
    pub asio: HashMap<String, Section>,
    pub asio_running: Option<String>,
    pub capture: Option<Vec<Section>>,
    pub capture_running: bool,
    pub message: Option<String>,
}

pub const QUICK_EVERY: Duration = Duration::from_secs(3);
pub const FULL_EVERY: Duration = Duration::from_secs(60);
const METER_EVERY: Duration = Duration::from_millis(80);

pub fn spawn(ctx: eframe::egui::Context) -> (Sender<Cmd>, Arc<Mutex<Shared>>) {
    let (tx, rx) = channel::<Cmd>();
    let shared = Arc::new(Mutex::new(Shared::default()));
    let out = shared.clone();
    std::thread::Builder::new()
        .name("scanner".into())
        .spawn(move || run(rx, out, ctx))
        .expect("scanner thread");
    (tx, shared)
}

fn run(rx: Receiver<Cmd>, shared: Arc<Mutex<Shared>>, ctx: eframe::egui::Context) {
    unsafe {
        let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED);
    }
    let mut scanner = Scanner::new();
    let mut selected: Option<String> = None;
    let mut last_quick = Instant::now() - QUICK_EVERY;
    let mut last_full: Option<Instant> = None;
    let mut last_meter = Instant::now();
    let mut capture = crate::etw::Capture::idle();

    loop {
        let mut force_full = false;
        loop {
            match rx.try_recv() {
                Ok(Cmd::Refresh) => force_full = true,
                Ok(Cmd::Select(k)) => selected = k,
                Ok(Cmd::AsioQuery { clsid, name }) => {
                    shared.lock().unwrap().asio_running = Some(clsid.clone());
                    ctx.request_repaint();
                    let section = asio::query(&clsid, &name);
                    let mut s = shared.lock().unwrap();
                    s.asio.insert(clsid, section);
                    s.asio_running = None;
                    ctx.request_repaint();
                }
                Ok(Cmd::CaptureStart) => {
                    let r = capture.start();
                    let mut s = shared.lock().unwrap();
                    s.capture_running = capture.running();
                    if let Err(e) = r {
                        s.message = Some(e);
                    }
                    ctx.request_repaint();
                }
                Ok(Cmd::CaptureStop) => {
                    capture.stop();
                    let mut s = shared.lock().unwrap();
                    s.capture_running = false;
                    s.capture = Some(capture.report());
                    ctx.request_repaint();
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    capture.stop();
                    return;
                }
            }
        }

        let due_full = force_full || last_full.map(|t| t.elapsed() >= FULL_EVERY).unwrap_or(true);
        if due_full || last_quick.elapsed() >= QUICK_EVERY {
            shared.lock().unwrap().scanning = true;
            ctx.request_repaint();
            let snap = scanner.scan(due_full);
            if due_full {
                last_full = Some(Instant::now());
            }
            last_quick = Instant::now();
            let mut s = shared.lock().unwrap();
            s.snap = Arc::new(snap);
            s.scanning = false;
            drop(s);
            ctx.request_repaint();
        }

        if capture.running() {
            let mut s = shared.lock().unwrap();
            s.capture = Some(capture.report());
        }

        if last_meter.elapsed() >= METER_EVERY {
            last_meter = Instant::now();
            let ids: Vec<String> = {
                let s = shared.lock().unwrap();
                s.snap
                    .devices
                    .iter()
                    .filter(|d| Some(&d.key) == selected.as_ref())
                    .flat_map(|d| d.endpoints.iter().filter(|e| e.state == EpState::Active).map(|e| e.id.clone()))
                    .collect()
            };
            let peaks = scanner.sample_meters(&ids);
            let changed = {
                let mut s = shared.lock().unwrap();
                let changed = s.peaks != peaks;
                s.peaks = peaks;
                changed
            };
            if changed {
                ctx.request_repaint();
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}
