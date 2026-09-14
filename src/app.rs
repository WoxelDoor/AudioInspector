//! The window: device list on the left, the selected device as tables on the right.

use std::collections::HashMap;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use eframe::egui::{self, Align, Color32, FontId, Layout, RichText, ScrollArea, Sense, TextFormat};

use crate::model::{Device, EpState, Row, Section, Snapshot, Tone};
use crate::worker::{Cmd, Shared};
use crate::{report, sys, util, VERSION};

const BG: Color32 = Color32::from_rgb(0x2b, 0x2d, 0x31);
const FIELD: Color32 = Color32::from_rgb(0x23, 0x25, 0x29);
const RULE: Color32 = Color32::from_rgb(0x3a, 0x3e, 0x45);
const TEXT: Color32 = Color32::from_rgb(0xc9, 0xcc, 0xd1);
const VALUE: Color32 = Color32::from_rgb(0xf0, 0xf2, 0xf5);
const GREEN: Color32 = Color32::from_rgb(0x63, 0xc0, 0x7a);
const AMBER: Color32 = Color32::from_rgb(0xe2, 0xb1, 0x3c);
const DIM: Color32 = Color32::from_rgb(0x8b, 0x90, 0x98);
const SELECT: Color32 = Color32::from_rgb(0x3b, 0x4b, 0x62);

const SYSTEM_KEY: &str = "system";

pub struct Screenshot {
    pub path: String,
    pub select: Option<String>,
    pub requested: bool,
    pub first_data: Option<Instant>,
}

pub struct App {
    tx: Sender<Cmd>,
    shared: Arc<Mutex<Shared>>,
    selected: Option<String>,
    show_all: bool,
    message: Option<(String, Instant)>,
    screenshot: Option<Screenshot>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, screenshot: Option<Screenshot>, demo: bool) -> Self {
        style(&cc.egui_ctx);
        let (tx, shared) = if demo { crate::demo::shared() } else { crate::worker::spawn(cc.egui_ctx.clone()) };
        App { tx, shared, selected: None, show_all: false, message: None, screenshot }
    }

    fn say(&mut self, text: impl Into<String>) {
        self.message = Some((text.into(), Instant::now()));
    }
}

fn style(ctx: &egui::Context) {
    ctx.set_theme(egui::Theme::Dark);
    let mut v = egui::Visuals::dark();
    v.panel_fill = BG;
    v.window_fill = BG;
    v.extreme_bg_color = FIELD;
    v.faint_bg_color = FIELD;
    v.widgets.noninteractive.fg_stroke.color = TEXT;
    v.widgets.noninteractive.bg_stroke.color = RULE;
    v.widgets.inactive.bg_fill = Color32::from_rgb(0x35, 0x38, 0x3e);
    v.widgets.inactive.weak_bg_fill = Color32::from_rgb(0x35, 0x38, 0x3e);
    v.widgets.hovered.weak_bg_fill = Color32::from_rgb(0x40, 0x44, 0x4b);
    v.selection.bg_fill = SELECT;
    v.selection.stroke.color = VALUE;
    ctx.set_visuals_of(egui::Theme::Dark, v);
    let mut st = (*ctx.style_of(egui::Theme::Dark)).clone();
    st.spacing.item_spacing = egui::vec2(8.0, 3.0);
    st.spacing.button_padding = egui::vec2(8.0, 3.0);
    for (ts, size) in [
        (egui::TextStyle::Body, 14.0),
        (egui::TextStyle::Button, 14.0),
        (egui::TextStyle::Heading, 20.0),
        (egui::TextStyle::Small, 12.0),
        (egui::TextStyle::Monospace, 13.0),
    ] {
        if let Some(f) = st.text_styles.get_mut(&ts) {
            f.size = size;
        }
    }
    ctx.set_style_of(egui::Theme::Dark, st);
}

fn tone_color(t: Tone) -> Color32 {
    match t {
        Tone::Normal => VALUE,
        Tone::Good => GREEN,
        Tone::Warn => AMBER,
        Tone::Dim => DIM,
    }
}

/// Only active devices by default. A headset switched to its cable, an empty jack and
/// everything Windows remembers wait for the checkbox.
fn visible(d: &Device, show_all: bool) -> bool {
    show_all || d.state == EpState::Active
}

fn exe_dir() -> std::path::PathBuf {
    std::env::current_exe().ok().and_then(|e| e.parent().map(|p| p.to_path_buf())).unwrap_or_else(std::env::temp_dir)
}

/// The label column is at least this wide in every section, so sections line up.
const LABEL_W: f32 = 200.0;

fn rows_grid(ui: &mut egui::Ui, rows: &[Row], depth: usize) {
    for r in rows {
        let indent = "      ".repeat(depth);
        let label_color = if depth > 0 { DIM } else { TEXT };
        ui.label(RichText::new(format!("{indent}{}", r.label)).color(label_color));
        ui.label(RichText::new(&r.value).color(tone_color(r.tone)));
        if r.note.is_empty() {
            ui.label("");
        } else {
            ui.label(RichText::new(&r.note).color(DIM).size(12.5));
        }
        ui.end_row();
        if !r.sub.is_empty() {
            rows_grid(ui, &r.sub, depth + 1);
        }
    }
}

fn section(ui: &mut egui::Ui, s: &Section, salt: &str) {
    ui.add_space(8.0);
    ui.label(RichText::new(&s.title).color(VALUE).size(14.5).strong());
    let w = ui.available_width().max(200.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, 1.0), Sense::hover());
    ui.painter().hline(rect.x_range(), rect.center().y, egui::Stroke::new(1.0_f32, RULE));
    ui.add_space(2.0);
    egui::Grid::new(format!("grid:{salt}:{}", s.title)).num_columns(3).min_col_width(LABEL_W).spacing([18.0, 3.0]).show(ui, |ui| {
        rows_grid(ui, &s.rows, 0);
    });
}

fn meter(ui: &mut egui::Ui, peak: f32) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("Level now").color(TEXT));
        let (rect, _) = ui.allocate_exact_size(egui::vec2(220.0, 10.0), Sense::hover());
        ui.painter().rect_filled(rect, 2.0, FIELD);
        let p = peak.clamp(0.0, 1.0);
        let mut fill = rect;
        fill.set_width(rect.width() * p);
        ui.painter().rect_filled(fill, 2.0, if p > 0.9 { AMBER } else { GREEN });
        let db = if peak > 0.00001 { format!("{:.1} dBFS", 20.0 * peak.log10()) } else { "silence".to_string() };
        ui.label(RichText::new(db).color(DIM).size(12.5));
    });
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let (snap, peaks, scanning, asio, asio_running, capture, capture_running, msg) = {
            let mut s = self.shared.lock().unwrap();
            (s.snap.clone(), s.peaks.clone(), s.scanning, s.asio.clone(), s.asio_running.clone(), s.capture.clone(), s.capture_running, s.message.take())
        };
        if let Some(m) = msg {
            self.say(m);
        }
        let has_data = snap.taken.is_some();

        // Keep the selection valid: the first visible device once data exists.
        if has_data {
            let valid = self.selected.as_deref() == Some(SYSTEM_KEY) || snap.devices.iter().any(|d| Some(&d.key) == self.selected.as_ref());
            if !valid {
                self.selected = snap.devices.iter().find(|d| visible(d, self.show_all)).map(|d| d.key.clone());
                let _ = self.tx.send(Cmd::Select(self.selected.clone()));
            }
        }

        let title = format!("AudioInspector {VERSION}{}", if snap.elevated { " — administrator" } else { "" });
        egui::TopBottomPanel::top("bar").frame(egui::Frame::default().fill(FIELD).inner_margin(egui::Margin::symmetric(10, 7))).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("AudioInspector").color(VALUE).size(17.0).strong());
                ui.label(RichText::new(VERSION).color(DIM));
                ui.add_space(12.0);
                if ui.button("Refresh").on_hover_text("Scan everything again, including the raw properties").clicked() {
                    let _ = self.tx.send(Cmd::Refresh);
                }
                ui.checkbox(&mut self.show_all, "Show inactive devices");
                ui.separator();
                if ui.button("Copy device").on_hover_text("Copy the selected device as text").clicked() {
                    let text = match self.selected.as_deref() {
                        Some(SYSTEM_KEY) => report::full(&snap, false),
                        Some(k) => snap.devices.iter().find(|d| d.key == k).map(|d| format!("{}{}", report::header(&snap), report::device(d, false))).unwrap_or_default(),
                        None => String::new(),
                    };
                    match sys::clipboard::set_text(&text) {
                        Ok(()) => self.say("Copied to the clipboard"),
                        Err(e) => self.say(format!("Clipboard: {e}")),
                    }
                }
                if ui.button("Copy all").on_hover_text("Copy every device as text").clicked() {
                    match sys::clipboard::set_text(&report::full(&snap, false)) {
                        Ok(()) => self.say("Copied to the clipboard"),
                        Err(e) => self.say(format!("Clipboard: {e}")),
                    }
                }
                if ui.button("Save report").on_hover_text("Write every device, with raw properties, to a text file next to the program").clicked() {
                    let stamp = util::fmt_time(SystemTime::now()).replace([':', ' '], "-");
                    let saved = sys::files::create_new_in(&exe_dir().join("reports"), &format!("AudioInspector-{stamp}"), ".txt").and_then(|(mut f, path)| {
                        std::io::Write::write_all(&mut f, report::full(&snap, true).as_bytes())?;
                        Ok(path)
                    });
                    match saved {
                        Ok(path) => self.say(format!("Saved {}", path.display())),
                        Err(e) => self.say(format!("Not saved: {e}")),
                    }
                }
                ui.separator();
                if !snap.elevated && has_data {
                    if ui
                        .button("Run as administrator")
                        .on_hover_text("Restart elevated: adds the experimental Bluetooth capture (a log of the Bluetooth stack's trace events)")
                        .clicked()
                    {
                        match sys::elevation::relaunch_elevated("") {
                            Ok(()) => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                            Err(e) => self.say(e),
                        }
                    }
                } else if snap.elevated {
                    let label = if capture_running { "Stop Bluetooth capture" } else { "Start Bluetooth capture" };
                    if ui.button(label).clicked() {
                        let _ = self.tx.send(if capture_running { Cmd::CaptureStop } else { Cmd::CaptureStart });
                        if !capture_running {
                            self.selected = Some(SYSTEM_KEY.into());
                        }
                    }
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let status = if !has_data {
                        "Scanning…".to_string()
                    } else if scanning {
                        format!("Updating… · last scan {:.1} s", snap.took_ms as f64 / 1000.0)
                    } else {
                        format!("Updated {} · {:.1} s", snap.taken.map(util::fmt_time).unwrap_or_default().get(11..).unwrap_or(""), snap.took_ms as f64 / 1000.0)
                    };
                    ui.label(RichText::new(status).color(DIM));
                });
            });
            if let Some((m, t)) = &self.message {
                if t.elapsed() < Duration::from_secs(8) {
                    ui.label(RichText::new(m).color(AMBER));
                }
            }
        });

        egui::SidePanel::left("devices").resizable(true).default_width(300.0).min_width(220.0).frame(egui::Frame::default().fill(BG).inner_margin(egui::Margin::same(8))).show(ctx, |ui| {
            ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                ui.with_layout(Layout::top_down_justified(Align::LEFT), |ui| {
                    let mut clicked: Option<String> = None;
                    for d in snap.devices.iter().filter(|d| visible(d, self.show_all)) {
                        let sel = self.selected.as_ref() == Some(&d.key);
                        let mut job = egui::text::LayoutJob::default();
                        let active = d.state == EpState::Active;
                        job.append(&d.name, 0.0, TextFormat { font_id: FontId::proportional(14.5), color: if active { VALUE } else { TEXT }, ..Default::default() });
                        job.append("\n", 0.0, TextFormat::default());
                        let small = |c: Color32| TextFormat { font_id: FontId::proportional(12.5), color: c, ..Default::default() };
                        job.append(&format!("{} · {}", d.kind.label(), d.transport.label()), 0.0, small(DIM));
                        if let Some(b) = &d.battery {
                            job.append(&format!(" · {} %", b.percent), 0.0, small(if b.percent <= 15 { AMBER } else { GREEN }));
                        }
                        if !active {
                            job.append(&format!(" · {}", d.state_label()), 0.0, small(DIM));
                        }
                        if ui.add(egui::SelectableLabel::new(sel, job)).clicked() {
                            clicked = Some(d.key.clone());
                        }
                        ui.add_space(2.0);
                    }
                    let hidden = snap.devices.iter().filter(|d| !visible(d, self.show_all)).count();
                    if hidden > 0 && !self.show_all {
                        ui.label(RichText::new(format!("{hidden} inactive hidden")).color(DIM).size(12.5));
                    }
                    ui.add_space(6.0);
                    let sel = self.selected.as_deref() == Some(SYSTEM_KEY);
                    let mut job = egui::text::LayoutJob::default();
                    job.append("This PC", 0.0, TextFormat { font_id: FontId::proportional(14.5), color: VALUE, ..Default::default() });
                    job.append("\nBluetooth radios, ASIO drivers, capture", 0.0, TextFormat { font_id: FontId::proportional(12.5), color: DIM, ..Default::default() });
                    if ui.add(egui::SelectableLabel::new(sel, job)).clicked() {
                        clicked = Some(SYSTEM_KEY.into());
                    }
                    if let Some(k) = clicked {
                        self.selected = Some(k.clone());
                        let _ = self.tx.send(Cmd::Select(Some(k)));
                    }
                });
            });
        });

        egui::CentralPanel::default().frame(egui::Frame::default().fill(BG).inner_margin(egui::Margin::symmetric(16, 10))).show(ctx, |ui| {
            ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| {
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                if !has_data {
                    ui.label(RichText::new("Reading the audio devices…").color(DIM));
                    return;
                }
                match self.selected.as_deref() {
                    Some(SYSTEM_KEY) => system_view(ui, &snap, capture.as_deref()),
                    Some(k) => {
                        if let Some(d) = snap.devices.iter().find(|d| d.key == k) {
                            device_view(ui, d, &peaks, &asio, asio_running.as_deref(), capture.as_deref(), &self.tx);
                        }
                    }
                    None => {
                        ui.label(RichText::new("No audio device found").color(DIM));
                    }
                }
            });
        });

        ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));
        ctx.request_repaint_after(Duration::from_millis(500));
        self.handle_screenshot(ctx, &snap);
    }
}

impl App {
    fn handle_screenshot(&mut self, ctx: &egui::Context, snap: &Snapshot) {
        let Some(shot) = self.screenshot.as_mut() else { return };
        if snap.taken.is_none() {
            return;
        }
        let first = *shot.first_data.get_or_insert_with(Instant::now);
        if let Some(sel) = shot.select.take() {
            let wanted = sel.to_lowercase();
            if wanted == SYSTEM_KEY {
                self.selected = Some(SYSTEM_KEY.into());
            } else if let Some(d) = snap.devices.iter().find(|d| d.name.to_lowercase().contains(&wanted)) {
                self.selected = Some(d.key.clone());
                self.show_all = true;
            }
            let _ = self.tx.send(Cmd::Select(self.selected.clone()));
        }
        let Some(shot) = self.screenshot.as_mut() else { return };
        if !shot.requested && first.elapsed() > Duration::from_millis(1500) {
            shot.requested = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
        }
        let image = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(img) = image {
            let path = shot.path.clone();
            let _ = save_png(&path, &img);
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        ctx.request_repaint_after(Duration::from_millis(100));
    }
}

fn save_png(path: &str, img: &egui::ColorImage) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), img.size[0] as u32, img.size[1] as u32);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut w = enc.write_header().map_err(|e| e.to_string())?;
    let bytes: Vec<u8> = img.pixels.iter().flat_map(|p| p.to_array()).collect();
    w.write_image_data(&bytes).map_err(|e| e.to_string())
}

fn device_view(
    ui: &mut egui::Ui,
    d: &Device,
    peaks: &HashMap<String, f32>,
    asio: &HashMap<String, Section>,
    asio_running: Option<&str>,
    capture: Option<&[Section]>,
    tx: &Sender<Cmd>,
) {
    ui.label(RichText::new(&d.name).color(VALUE).size(21.0).strong());
    ui.label(RichText::new(format!("{} · {} · {}", d.kind.label(), d.transport.label(), d.state_label())).color(DIM));

    for s in d.sections.iter().filter(|s| !s.title.starts_with("Raw") && s.title != "Drivers") {
        if s.title == "ASIO" {
            section(ui, s, &d.key);
            for r in &s.rows {
                let clsid = r.note.clone();
                ui.horizontal(|ui| {
                    let busy = asio_running == Some(clsid.as_str());
                    let label = if busy { format!("Querying {}…", r.label) } else { format!("Query {}", r.label) };
                    if ui
                        .add_enabled(asio_running.is_none(), egui::Button::new(label))
                        .on_hover_text("Loads the ASIO driver in a helper process and asks for channels, buffer sizes, sample rates and latency. Does not start streaming.")
                        .clicked()
                    {
                        let _ = tx.send(Cmd::AsioQuery { clsid: clsid.clone(), name: r.label.clone() });
                    }
                });
                if let Some(res) = asio.get(&clsid) {
                    section(ui, res, &format!("{}:{clsid}", d.key));
                }
            }
            continue;
        }
        section(ui, s, &d.key);
    }

    if let (Some(addr), Some(cap)) = (d.bt_address, capture) {
        let wanted = util::mac(addr);
        for s in cap.iter().filter(|s| s.title.contains(&wanted)) {
            section(ui, s, &d.key);
        }
    }

    ui.add_space(10.0);
    for e in &d.endpoints {
        let head = RichText::new(format!("{} · {} · {}", e.flow.label(), e.name, e.state_label())).color(if e.state == EpState::Active { VALUE } else { TEXT }).size(15.0);
        egui::CollapsingHeader::new(head).id_salt(format!("ep:{}", e.id)).default_open(e.state == EpState::Active).show(ui, |ui| {
            if let Some(p) = peaks.get(&e.id) {
                meter(ui, *p);
            }
            for s in &e.sections {
                section(ui, s, &e.id);
            }
        });
    }

    ui.add_space(6.0);
    if let Some(drv) = d.sections.iter().find(|s| s.title == "Drivers") {
        egui::CollapsingHeader::new(RichText::new(format!("Drivers · {}", drv.rows.len())).color(TEXT).size(15.0)).id_salt(format!("drv:{}", d.key)).show(ui, |ui| {
            section(ui, drv, &d.key);
        });
    }
    let raw: Vec<&Section> = d.sections.iter().filter(|s| s.title.starts_with("Raw")).collect();
    if !raw.is_empty() {
        egui::CollapsingHeader::new(RichText::new(format!("Raw properties · {} lists", raw.len())).color(TEXT).size(15.0)).id_salt(format!("raw:{}", d.key)).show(ui, |ui| {
            for s in raw {
                egui::CollapsingHeader::new(RichText::new(s.title.trim_start_matches("Raw · ")).color(TEXT)).id_salt(format!("raw:{}:{}", d.key, s.title)).show(ui, |ui| {
                    egui::Grid::new(format!("rawgrid:{}:{}", d.key, s.title)).num_columns(3).spacing([18.0, 2.0]).show(ui, |ui| {
                        rows_grid(ui, &s.rows, 0);
                    });
                });
            }
        });
    }
}

fn system_view(ui: &mut egui::Ui, snap: &Snapshot, capture: Option<&[Section]>) {
    ui.label(RichText::new("This PC").color(VALUE).size(21.0).strong());
    ui.label(RichText::new(format!("{} audio devices · {}", snap.devices.len(), if snap.elevated { "administrator" } else { "standard user" })).color(DIM));
    if let Some(cap) = capture {
        for s in cap {
            section(ui, s, "capture");
        }
    } else if snap.elevated {
        let mut s = Section::new("Bluetooth capture");
        s.add("State", "not started").tone(Tone::Dim);
        s.add("How", "Start Bluetooth capture, then switch the headset off and on or start playback").tone(Tone::Dim);
        section(ui, &s, "capture");
    }
    for s in &snap.system {
        section(ui, s, "system");
    }
    if !snap.problems.is_empty() {
        let mut s = Section::new("Not read");
        for p in &snap.problems {
            s.push(Row::new("", p.clone()).with_tone(Tone::Warn));
        }
        section(ui, &s, "problems");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Transport;

    fn dev(state: EpState, transport: Transport) -> Device {
        Device { state, transport, ..Default::default() }
    }

    #[test]
    fn only_active_devices_are_listed_by_default() {
        assert!(visible(&dev(EpState::Active, Transport::BluetoothClassic), false));
        assert!(visible(&dev(EpState::Active, Transport::HdAudio), false));
        // A headset switched to its cable, an empty onboard jack, a remembered USB DAC.
        assert!(!visible(&dev(EpState::Unplugged, Transport::BluetoothClassic), false));
        assert!(!visible(&dev(EpState::Unplugged, Transport::HdAudio), false));
        assert!(!visible(&dev(EpState::NotPresent, Transport::Usb), false));
        assert!(visible(&dev(EpState::Unplugged, Transport::BluetoothClassic), true));
    }
}
