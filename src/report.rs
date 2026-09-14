//! Plain-text report: the same table the window shows, for the clipboard and files.

use crate::model::{Device, Row, Section, Snapshot};
use crate::util::fmt_time;
use crate::VERSION;

fn rows(out: &mut String, rows: &[Row], indent: usize) {
    let width = rows.iter().map(|r| r.label.chars().count()).max().unwrap_or(0).min(34);
    for r in rows {
        let pad = " ".repeat(indent);
        let label = format!("{:<width$}", r.label, width = width);
        let mut line = format!("{pad}{label}  {}", r.value);
        if !r.note.is_empty() {
            line.push_str(&format!("   ({})", r.note));
        }
        out.push_str(line.trim_end());
        out.push('\n');
        if !r.sub.is_empty() {
            self::rows(out, &r.sub, indent + 4);
        }
    }
}

fn section(out: &mut String, s: &Section, indent: usize) {
    out.push_str(&format!("{}{}\n", " ".repeat(indent), s.title));
    rows(out, &s.rows, indent + 2);
}

pub fn device(d: &Device, with_raw: bool) -> String {
    let mut out = String::new();
    out.push_str(&format!("== {} ==\n", d.name));
    for s in d.sections.iter().filter(|s| !s.title.starts_with("Raw") && s.title != "Drivers") {
        section(&mut out, s, 0);
    }
    for e in &d.endpoints {
        out.push_str(&format!("Endpoint: {}\n", e.name));
        for s in &e.sections {
            section(&mut out, s, 2);
        }
    }
    for s in d.sections.iter().filter(|s| s.title == "Drivers") {
        section(&mut out, s, 0);
    }
    if with_raw {
        for s in d.sections.iter().filter(|s| s.title.starts_with("Raw")) {
            section(&mut out, s, 0);
        }
    }
    out
}

pub fn header(snap: &Snapshot) -> String {
    format!(
        "AudioInspector {VERSION} · {} · {}\n",
        snap.taken.map(fmt_time).unwrap_or_default(),
        if snap.elevated { "administrator" } else { "standard user" }
    )
}

pub fn full(snap: &Snapshot, with_raw: bool) -> String {
    let mut out = header(snap);
    out.push('\n');
    for d in &snap.devices {
        out.push_str(&device(d, with_raw));
        out.push('\n');
    }
    if !snap.system.is_empty() {
        out.push_str("== System ==\n");
        for s in &snap.system {
            section(&mut out, s, 0);
        }
    }
    if !snap.problems.is_empty() {
        out.push_str("\n== Not read ==\n");
        for p in &snap.problems {
            out.push_str(&format!("  {p}\n"));
        }
    }
    out
}
