//! SDP records (Bluetooth Core Vol 3 Part B) decoded into profiles, versions and features.
//! The records are Windows' cache of what the device announced when it was paired.

#[derive(Clone, Debug, PartialEq)]
pub enum De {
    Nil,
    UInt(u64),
    Int(i64),
    Uuid(u32),
    Uuid128([u8; 16]),
    Str(String),
    Bool(bool),
    Seq(Vec<De>),
}

fn be(d: &[u8]) -> u64 {
    d.iter().take(8).fold(0u64, |a, &x| (a << 8) | x as u64)
}

/// Nesting deeper than this is not a real record: a device could otherwise send
/// sequences nested deep enough to overflow the scanner thread's stack.
const MAX_DEPTH: usize = 16;

pub fn parse(b: &[u8], p: &mut usize) -> Option<De> {
    parse_nested(b, p, 0)
}

fn parse_nested(b: &[u8], p: &mut usize, depth: usize) -> Option<De> {
    let h = *b.get(*p)?;
    *p += 1;
    let t = h >> 3;
    let len = if t == 0 {
        0
    } else {
        match h & 7 {
            0 => 1,
            1 => 2,
            2 => 4,
            3 => 8,
            4 => 16,
            5 => {
                let l = *b.get(*p)? as usize;
                *p += 1;
                l
            }
            6 => {
                let l = be(b.get(*p..*p + 2)?) as usize;
                *p += 2;
                l
            }
            _ => {
                let l = be(b.get(*p..*p + 4)?) as usize;
                *p += 4;
                l
            }
        }
    };
    let d = b.get(*p..*p + len)?;
    *p += len;
    Some(match t {
        0 => De::Nil,
        1 => De::UInt(be(d)),
        2 => De::Int(be(d) as i64),
        3 => match len {
            16 => De::Uuid128(d.try_into().ok()?),
            _ => De::Uuid(be(d) as u32),
        },
        4 | 8 => De::Str(String::from_utf8_lossy(d).trim_end_matches('\0').to_string()),
        5 => De::Bool(d.first().copied().unwrap_or(0) != 0),
        6 | 7 => {
            if depth >= MAX_DEPTH {
                return None;
            }
            let mut v = Vec::new();
            let mut q = 0usize;
            while q < d.len() {
                v.push(parse_nested(d, &mut q, depth + 1)?);
            }
            De::Seq(v)
        }
        _ => return None,
    })
}

const BASE_TAIL: [u8; 12] = [0x00, 0x00, 0x10, 0x00, 0x80, 0x00, 0x00, 0x80, 0x5F, 0x9B, 0x34, 0xFB];

impl De {
    /// 16/32-bit UUID value, also from a 128-bit UUID on the Bluetooth base.
    pub fn short_uuid(&self) -> Option<u32> {
        match self {
            De::Uuid(u) => Some(*u),
            De::Uuid128(b) if b[4..] == BASE_TAIL => Some(be(&b[..4]) as u32),
            _ => None,
        }
    }

    pub fn uuid_text(&self) -> String {
        match self {
            De::Uuid128(b) if b[4..] != BASE_TAIL => {
                let h: Vec<String> = b.iter().map(|x| format!("{x:02x}")).collect();
                format!("{}-{}-{}-{}-{}", h[..4].concat(), h[4..6].concat(), h[6..8].concat(), h[8..10].concat(), h[10..].concat())
            }
            _ => self.short_uuid().map(|u| format!("0x{u:04X}")).unwrap_or_default(),
        }
    }

    pub fn uint(&self) -> Option<u64> {
        match self {
            De::UInt(v) => Some(*v),
            _ => None,
        }
    }

    pub fn seq(&self) -> &[De] {
        match self {
            De::Seq(v) => v,
            _ => &[],
        }
    }
}

/// A decoded record: attribute id -> value.
pub struct Record(pub Vec<(u16, De)>);

impl Record {
    pub fn parse(bytes: &[u8]) -> Option<Record> {
        let mut p = 0;
        let De::Seq(items) = parse(bytes, &mut p)? else { return None };
        let mut out = Vec::new();
        for pair in items.chunks(2) {
            if let [De::UInt(id), v] = pair {
                out.push((*id as u16, v.clone()));
            }
        }
        Some(Record(out))
    }

    pub fn get(&self, id: u16) -> Option<&De> {
        self.0.iter().find(|(a, _)| *a == id).map(|(_, v)| v)
    }

    pub fn service_classes(&self) -> Vec<&De> {
        self.get(0x0001).map(|v| v.seq().iter().collect()).unwrap_or_default()
    }

    /// (profile UUID, version) pairs from BluetoothProfileDescriptorList.
    pub fn profiles(&self) -> Vec<(u32, u16)> {
        self.get(0x0009)
            .map(|v| {
                v.seq()
                    .iter()
                    .filter_map(|p| {
                        let s = p.seq();
                        Some((s.first()?.short_uuid()?, s.get(1)?.uint()? as u16))
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn name(&self) -> Option<String> {
        match self.get(0x0100) {
            Some(De::Str(s)) if !s.is_empty() => Some(s.clone()),
            _ => None,
        }
    }

    pub fn features(&self) -> Option<u32> {
        self.get(0x0311).and_then(|v| v.uint()).map(|v| v as u32)
    }

    pub fn rfcomm_channel(&self) -> Option<u64> {
        self.get(0x0004)?.seq().iter().find_map(|p| {
            let s = p.seq();
            (s.first()?.short_uuid()? == 0x0003).then(|| s.get(1)?.uint()).flatten()
        })
    }
}

pub fn uuid_name(u: u32) -> Option<&'static str> {
    Some(match u {
        0x1101 => "Serial Port",
        0x1108 => "Headset",
        0x110A => "Audio Source",
        0x110B => "Audio Sink",
        0x110C => "A/V Remote Control Target",
        0x110D => "A2DP",
        0x110E => "A/V Remote Control",
        0x110F => "A/V Remote Control Controller",
        0x1112 => "Headset Audio Gateway",
        0x111E => "Hands-Free",
        0x111F => "Hands-Free Audio Gateway",
        0x1124 => "HID",
        0x112F => "Phonebook Access",
        0x1131 => "Headset (HS)",
        0x1200 => "PnP Information",
        0x1203 => "Generic Audio",
        0x1800 => "Generic Access",
        0x1801 => "Generic Attribute",
        _ => return None,
    })
}

fn bits(mask: u32, names: &[(u32, &'static str)]) -> Vec<&'static str> {
    names.iter().filter(|(b, _)| mask & (1 << b) != 0).map(|(_, n)| *n).collect()
}

pub fn hfp_features(f: u32) -> Vec<&'static str> {
    bits(
        f,
        &[
            (0, "echo cancel/noise reduction"),
            (1, "three-way calling"),
            (2, "caller id"),
            (3, "voice recognition"),
            (4, "remote volume"),
            (5, "wide band speech"),
            (6, "enhanced voice recognition"),
            (7, "voice recognition text"),
            (8, "super wide band speech"),
        ],
    )
}

pub fn a2dp_sink_features(f: u32) -> Vec<&'static str> {
    bits(f, &[(0, "headphone"), (1, "speaker"), (2, "recorder"), (3, "amplifier")])
}

pub fn a2dp_source_features(f: u32) -> Vec<&'static str> {
    bits(f, &[(0, "player"), (1, "microphone"), (2, "tuner"), (3, "mixer")])
}

pub fn avrcp_target_features(f: u32) -> Vec<&'static str> {
    bits(
        f,
        &[
            (0, "category 1 player"),
            (1, "category 2 monitor/amplifier (absolute volume)"),
            (2, "category 3 tuner"),
            (3, "category 4 menu"),
            (4, "player settings"),
            (5, "group navigation"),
            (6, "browsing"),
            (7, "multiple players"),
            (8, "cover art"),
        ],
    )
}

pub fn avrcp_controller_features(f: u32) -> Vec<&'static str> {
    bits(f, &[(0, "category 1"), (1, "category 2"), (2, "category 3"), (3, "category 4"), (6, "browsing"), (7, "cover art")])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_a_hands_free_record() {
        // A headset's Hands-Free record: HFP 1.7, RFCOMM channel 10, features 0x00FF.
        let rec: Vec<u8> = vec![
            0x35, 0x2F, // DES, 47 bytes
            0x09, 0x00, 0x01, 0x35, 0x06, 0x19, 0x11, 0x1E, 0x19, 0x12, 0x03, // ServiceClassIDList
            0x09, 0x00, 0x04, 0x35, 0x0C, 0x35, 0x03, 0x19, 0x01, 0x00, 0x35, 0x05, 0x19, 0x00, 0x03, 0x08, 0x0A, // protocols
            0x09, 0x00, 0x09, 0x35, 0x08, 0x35, 0x06, 0x19, 0x11, 0x1E, 0x09, 0x01, 0x07, // profile list
            0x09, 0x03, 0x11, 0x09, 0x00, 0xFF, // SupportedFeatures
        ];
        let r = Record::parse(&rec).unwrap();
        assert_eq!(r.service_classes()[0].short_uuid(), Some(0x111E));
        assert_eq!(r.profiles(), vec![(0x111E, 0x0107)]);
        assert_eq!(r.rfcomm_channel(), Some(10));
        assert_eq!(r.features(), Some(0xFF));
        assert!(hfp_features(0xFF).contains(&"wide band speech"));
        assert!(!hfp_features(0xFF).contains(&"super wide band speech"));
    }

    #[test]
    fn deeply_nested_sequences_are_rejected_not_recursed() {
        // 40 sequences inside each other around one uint8
        let mut rec = vec![0x08, 0x01];
        for _ in 0..40 {
            let mut outer = vec![0x35, rec.len() as u8];
            outer.extend_from_slice(&rec);
            rec = outer;
        }
        assert!(parse(&rec, &mut 0).is_none());
        // a shallow one still parses
        assert_eq!(parse(&[0x35, 0x02, 0x08, 0x07], &mut 0), Some(De::Seq(vec![De::UInt(7)])));
    }
}
