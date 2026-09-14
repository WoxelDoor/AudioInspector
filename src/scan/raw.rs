//! Raw dumps: every property of every devnode of a device and every endpoint property
//! store value, named where the SDK or the hand-written key table names the key.

use super::endpoints::EpRaw;
use crate::model::{Device, Row, Section, Tone};
use crate::sys::devnode::{self, PropValue};
use crate::sys::keys::key_label;

fn value_text(v: &PropValue) -> String {
    let mut s = v.display();
    if s.len() > 400 {
        s.truncate(400);
        s.push_str(" …");
    }
    s
}

pub fn sections(dev: &Device, eps: &[EpRaw]) -> Vec<Section> {
    let mut out = Vec::new();
    for e in eps {
        if e.store.is_empty() {
            continue;
        }
        let mut s = Section::new(format!("Raw · endpoint · {}", e.ep.name));
        for (k, v) in &e.store {
            s.push(Row::new(key_label(k), value_text(v)));
        }
        out.push(s);
    }
    for id in &dev.devnodes {
        if id.to_uppercase().starts_with("SWD\\MMDEVAPI") {
            continue;
        }
        let props = devnode::all(id);
        if props.is_empty() {
            continue;
        }
        let name = devnode::get_str(id, &crate::sys::keys::FRIENDLY_NAME)
            .or_else(|| devnode::get_str(id, &crate::sys::keys::DEVICE_DESC))
            .unwrap_or_default();
        let mut s = Section::new(format!("Raw · devnode · {name}"));
        s.push(Row::new("Instance id", id.clone()).with_tone(Tone::Dim));
        for (k, v) in props {
            s.push(Row::new(key_label(&k), value_text(&v)));
        }
        out.push(s);
    }
    for e in eps {
        let Some(path) = &e.ep.filter_interface else { continue };
        let props = devnode::interface_all(path);
        if props.is_empty() {
            continue;
        }
        let mut s = Section::new(format!("Raw · KS filter interface · {}", e.ep.name));
        s.push(Row::new("Interface", path.clone()).with_tone(Tone::Dim));
        for (k, v) in props {
            s.push(Row::new(key_label(&k), value_text(&v)));
        }
        out.push(s);
    }
    out
}
