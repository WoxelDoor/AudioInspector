//! AudioInspector - everything Windows can tell about every audio device.
//!
//! Run without arguments for the window. Other modes:
//!   --report <file> [--raw]                write the full text report and exit
//!   --screenshot <file.png> [--select X]   open, select a device by name, save a PNG, exit
//!   --demo                                 invented devices instead of a scan, for screenshots
//!   --asio-query {CLSID}                   internal: query one ASIO driver in a child process

#![windows_subsystem = "windows"]
#![allow(dead_code)]

mod app;
mod demo;
mod etw;
mod model;
mod report;
mod scan;
mod sys;
mod util;
mod worker;

use windows::Win32::Foundation::RECT;
use windows::Win32::UI::WindowsAndMessaging::{SystemParametersInfoW, SPI_GETWORKAREA, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

fn arg_after(args: &[String], flag: &str) -> Option<String> {
    args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1).cloned())
}

/// Window size as a share of the work area of the primary screen, never a fixed size.
fn window_size() -> [f32; 2] {
    let mut r = RECT::default();
    let ok = unsafe { SystemParametersInfoW(SPI_GETWORKAREA, 0, Some(&mut r as *mut _ as *mut _), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0)) }.is_ok();
    let (w, h) = if ok { ((r.right - r.left) as f32, (r.bottom - r.top) as f32) } else { (1600.0, 900.0) };
    [(w * 0.72).clamp(760.0f32.min(w * 0.9), w * 0.9), (h * 0.82).clamp(520.0f32.min(h * 0.9), h * 0.9)]
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if let Some(clsid) = arg_after(&args, "--asio-query") {
        std::process::exit(scan::asio::run_query(&clsid));
    }
    if let Some(path) = arg_after(&args, "--bench") {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED);
        }
        let mut scanner = scan::Scanner::new();
        let mut out = String::new();
        for (i, full) in [true, false, false, false, true].iter().enumerate() {
            let s = scanner.scan(*full);
            out.push_str(&format!("scan {i} full={full}: {} ms, {} devices\n", s.took_ms, s.devices.len()));
        }
        let _ = std::fs::write(path, out);
        std::process::exit(0);
    }
    if args.iter().any(|a| a == "--report") {
        let path = arg_after(&args, "--report").unwrap_or_else(|| "AudioInspector-report.txt".into());
        let raw = args.iter().any(|a| a == "--raw");
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED);
        }
        let mut scanner = scan::Scanner::new();
        let snap = scanner.scan(true);
        let code = if std::fs::write(&path, report::full(&snap, raw)).is_ok() { 0 } else { 1 };
        std::process::exit(code);
    }

    let screenshot = arg_after(&args, "--screenshot").map(|path| app::Screenshot { path, select: arg_after(&args, "--select"), requested: false, first_data: None });
    let demo = args.iter().any(|a| a == "--demo");
    let size = window_size();
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title(format!("AudioInspector {VERSION}"))
            .with_inner_size(size)
            .with_min_inner_size([640.0, 420.0]),
        centered: true,
        ..Default::default()
    };
    let _ = eframe::run_native("AudioInspector", options, Box::new(move |cc| Ok(Box::new(app::App::new(cc, screenshot, demo)))));
}
