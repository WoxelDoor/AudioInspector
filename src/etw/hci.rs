//! Bluetooth HCI traffic out of ETW events: ACL/L2CAP framing, AVDTP stream configuration
//! (codec parameters, SBC bitrate) and the measured bitrate of the A2DP media channel.
//!
//! The BTHPORT trace event layout is not documented, so packets are recognised by their
//! own framing: an ACL header whose length matches the rest of the event payload, with or
//! without a leading H4 packet-type byte. Everything here is pure and unit-tested.

use std::collections::{HashMap, VecDeque};

#[derive(Clone, Debug, PartialEq)]
pub enum Codec {
    Sbc { rate: u32, mode: &'static str, blocks: u32, subbands: u32, allocation: &'static str, min_bitpool: u32, max_bitpool: u32 },
    Aac { object: &'static str, rate: u32, channels: u32, vbr: bool, bitrate: u32 },
    Vendor { vendor: u32, codec: u16, data: Vec<u8> },
    Other { codec_type: u8, data: Vec<u8> },
}

impl Codec {
    pub fn describe(&self) -> (String, Vec<(String, String)>) {
        match self {
            Codec::Sbc { rate, mode, blocks, subbands, allocation, min_bitpool, max_bitpool } => {
                let mut rows = vec![
                    ("Sample rate".into(), crate::model::khz(*rate)),
                    ("Channel mode".into(), mode.to_string()),
                    ("Blocks · subbands".into(), format!("{blocks} · {subbands}")),
                    ("Allocation".into(), allocation.to_string()),
                    ("Bitpool".into(), format!("{min_bitpool}…{max_bitpool}")),
                ];
                if let Some(kbps) = sbc_bitrate(*rate, mode, *blocks, *subbands, *max_bitpool) {
                    rows.push(("Bitrate at max bitpool".into(), format!("{kbps} kbit/s")));
                }
                ("SBC".into(), rows)
            }
            Codec::Aac { object, rate, channels, vbr, bitrate } => (
                "AAC".into(),
                vec![
                    ("Object type".into(), object.to_string()),
                    ("Sample rate".into(), crate::model::khz(*rate)),
                    ("Channels".into(), channels.to_string()),
                    ("Bitrate".into(), if *bitrate == 0 { "not stated".into() } else { format!("{} kbit/s{}", bitrate / 1000, if *vbr { " (VBR allowed)" } else { "" }) }),
                ],
            ),
            Codec::Vendor { vendor, codec, data } => {
                let name = crate::scan::names::a2dp_codec(&[&[0xFF], &vendor.to_le_bytes()[..], &codec.to_le_bytes()[..]].concat()).0;
                let mut rows = vec![("Vendor data".into(), crate::util::hex_bytes(data, 16))];
                if let (Some(b), true) = (data.first(), name.starts_with("aptX")) {
                    let rate = match b >> 4 {
                        x if x & 0x8 != 0 => 16000,
                        x if x & 0x4 != 0 => 32000,
                        x if x & 0x2 != 0 => 44100,
                        x if x & 0x1 != 0 => 48000,
                        _ => 0,
                    };
                    if rate != 0 {
                        rows.insert(0, ("Sample rate".into(), crate::model::khz(rate)));
                    }
                }
                (name, rows)
            }
            Codec::Other { codec_type, data } => (format!("codec type 0x{codec_type:02X}"), vec![("Data".into(), crate::util::hex_bytes(data, 16))]),
        }
    }
}

fn first_bit(v: u8, table: &[(u8, u32)]) -> u32 {
    table.iter().find(|(bit, _)| v & bit != 0).map(|(_, x)| *x).unwrap_or(0)
}

/// Media Codec capability (AVDTP service category 7): media type, codec type, codec info.
pub fn parse_codec(cap: &[u8]) -> Option<Codec> {
    let codec_type = *cap.get(1)?;
    let info = cap.get(2..)?;
    Some(match codec_type {
        0x00 if info.len() >= 4 => {
            let rate = first_bit(info[0] >> 4, &[(8, 16000), (4, 32000), (2, 44100), (1, 48000)]);
            let mode = match info[0] & 0x0F {
                m if m & 1 != 0 => "joint stereo",
                m if m & 2 != 0 => "stereo",
                m if m & 4 != 0 => "dual channel",
                m if m & 8 != 0 => "mono",
                _ => "?",
            };
            let blocks = first_bit(info[1] >> 4, &[(8, 4), (4, 8), (2, 12), (1, 16)]);
            let subbands = first_bit((info[1] >> 2) & 3, &[(2, 4), (1, 8)]);
            let allocation = if info[1] & 1 != 0 { "loudness" } else { "SNR" };
            Codec::Sbc { rate, mode, blocks, subbands, allocation, min_bitpool: info[2] as u32, max_bitpool: info[3] as u32 }
        }
        0x02 if info.len() >= 6 => {
            let object = match info[0] {
                o if o & 0x80 != 0 => "MPEG-2 AAC LC",
                o if o & 0x40 != 0 => "MPEG-4 AAC LC",
                o if o & 0x20 != 0 => "MPEG-4 AAC LTP",
                o if o & 0x10 != 0 => "MPEG-4 AAC scalable",
                o if o & 0x08 != 0 => "MPEG-4 HE-AAC",
                o if o & 0x04 != 0 => "MPEG-4 HE-AAC v2",
                _ => "?",
            };
            let r1 = [8000, 11025, 12000, 16000, 22050, 24000, 32000, 44100];
            let r2 = [48000, 64000, 88200, 96000];
            let mut rate = 0;
            for (i, r) in r1.iter().enumerate() {
                if info[1] & (0x80 >> i) != 0 {
                    rate = *r;
                    break;
                }
            }
            if rate == 0 {
                for (i, r) in r2.iter().enumerate() {
                    if info[2] & (0x80 >> i) != 0 {
                        rate = *r;
                        break;
                    }
                }
            }
            let channels = if info[2] & 0x04 != 0 { 2 } else if info[2] & 0x08 != 0 { 1 } else { 0 };
            let vbr = info[3] & 0x80 != 0;
            let bitrate = ((info[3] as u32 & 0x7F) << 16) | ((info[4] as u32) << 8) | info[5] as u32;
            Codec::Aac { object, rate, channels, vbr, bitrate }
        }
        0xFF if info.len() >= 6 => Codec::Vendor {
            vendor: u32::from_le_bytes(info[0..4].try_into().ok()?),
            codec: u16::from_le_bytes([info[4], info[5]]),
            data: info[6..].to_vec(),
        },
        t => Codec::Other { codec_type: t, data: info.to_vec() },
    })
}

/// A2DP spec SBC frame length and bitrate for one bitpool.
pub fn sbc_bitrate(rate: u32, mode: &str, blocks: u32, subbands: u32, bitpool: u32) -> Option<u32> {
    if rate == 0 || blocks == 0 || subbands == 0 {
        return None;
    }
    let channels = if mode == "mono" { 1 } else { 2 };
    let bits = match mode {
        "mono" | "dual channel" => blocks * channels * bitpool,
        "stereo" => blocks * bitpool,
        _ => subbands + blocks * bitpool,
    };
    let frame = 4 + (4 * subbands * channels) / 8 + bits.div_ceil(8);
    // 327 994 bit/s for the classic high-quality setting: round, do not truncate, to 328.
    Some((8.0 * frame as f64 * rate as f64 / (subbands * blocks) as f64 / 1000.0).round() as u32)
}

/// AVDTP signalling with a Media Codec capability: SET_CONFIGURATION or RECONFIGURE
/// commands, GET_CONFIGURATION accepts. Recognised by structure, not by channel.
pub fn parse_avdtp_config(p: &[u8]) -> Option<(&'static str, Codec)> {
    if p.len() < 4 {
        return None;
    }
    let packet_type = (p[0] >> 2) & 3;
    let message_type = p[0] & 3;
    if packet_type != 0 {
        return None;
    }
    let signal = p[1] & 0x3F;
    let (name, caps) = match (signal, message_type) {
        (0x03, 0) => ("SET_CONFIGURATION", p.get(4..)?),
        (0x05, 0) => ("RECONFIGURE", p.get(3..)?),
        (0x04, 2) => ("GET_CONFIGURATION", p.get(2..)?),
        _ => return None,
    };
    let mut at = 0;
    let mut codec = None;
    while at < caps.len() {
        let category = caps[at];
        let len = *caps.get(at + 1)? as usize;
        if !(1..=8).contains(&category) {
            return None;
        }
        let body = caps.get(at + 2..at + 2 + len)?;
        if category == 7 {
            codec = parse_codec(body);
        }
        at += 2 + len;
    }
    (at == caps.len()).then_some(()).and(codec).map(|c| (name, c))
}

#[derive(Default, Debug)]
pub struct Link {
    pub address: Option<u64>,
    pub config: Option<(&'static str, Codec)>,
    /// cid -> (bytes, packets) over the whole capture.
    pub totals: HashMap<u16, (u64, u64)>,
    /// (ms, cid, bytes) for the sliding window.
    window: VecDeque<(u64, u16, u32)>,
    /// PSM 0x19 connection order: the second AVDTP channel is the media transport.
    avdtp_cids: Vec<u16>,
    pending: HashMap<u8, u16>,
    last_start_cid: Option<u16>,
}

impl Link {
    fn media_cid(&self) -> Option<(u16, &'static str)> {
        if let Some(c) = self.avdtp_cids.get(1) {
            return Some((*c, "AVDTP media channel"));
        }
        // Mid-stream capture: the busiest dynamic channel in the window.
        let mut by_cid: HashMap<u16, u64> = HashMap::new();
        for (_, cid, b) in &self.window {
            if *cid >= 0x40 {
                *by_cid.entry(*cid).or_default() += *b as u64;
            }
        }
        by_cid.into_iter().max_by_key(|(_, b)| *b).map(|(c, _)| (c, "busiest channel"))
    }

    /// (kbit/s, packets/s, average payload bytes, cid, how the channel was chosen) over the window.
    pub fn media_rate(&self, now_ms: u64, window_ms: u64) -> Option<(f64, f64, f64, u16, &'static str)> {
        let (cid, how) = self.media_cid()?;
        let from = now_ms.saturating_sub(window_ms);
        let (mut bytes, mut packets, mut first) = (0u64, 0u64, u64::MAX);
        for (t, c, b) in &self.window {
            if *c == cid && *t >= from {
                bytes += *b as u64;
                packets += 1;
                first = first.min(*t);
            }
        }
        if packets < 2 {
            return None;
        }
        let span = (now_ms.saturating_sub(first)).max(1) as f64 / 1000.0;
        Some((bytes as f64 * 8.0 / 1000.0 / span, packets as f64 / span, bytes as f64 / packets as f64, cid, how))
    }
}

#[derive(Default, Debug)]
pub struct Tracker {
    pub links: HashMap<u16, Link>,
    pub acl_packets: u64,
    pub events_with_hci: u64,
}

impl Tracker {
    /// Looks for one HCI packet in an ETW payload; returns true when one was recognised.
    pub fn feed(&mut self, now_ms: u64, blob: &[u8]) -> bool {
        for off in 0..blob.len().min(24) {
            let rest = &blob[off..];
            if rest.first() == Some(&0x02) && self.acl(now_ms, &rest[1..]) {
                self.events_with_hci += 1;
                return true;
            }
            if rest.first() == Some(&0x04) && self.event(&rest[1..]) {
                self.events_with_hci += 1;
                return true;
            }
        }
        for off in 0..blob.len().min(24) {
            if self.acl(now_ms, &blob[off..]) {
                self.events_with_hci += 1;
                return true;
            }
        }
        false
    }

    fn event(&mut self, e: &[u8]) -> bool {
        if e.len() < 2 || e[1] as usize + 2 != e.len() {
            return false;
        }
        // Connection Complete: status, handle, BD_ADDR, link type, encryption.
        if e[0] == 0x03 && e.len() >= 13 && e[2] == 0 {
            let handle = u16::from_le_bytes([e[3], e[4]]) & 0x0FFF;
            let mut addr = [0u8; 8];
            addr[..6].copy_from_slice(&e[5..11]);
            self.links.entry(handle).or_default().address = Some(u64::from_le_bytes(addr));
        }
        true
    }

    fn acl(&mut self, now_ms: u64, a: &[u8]) -> bool {
        if a.len() < 4 {
            return false;
        }
        let hdr = u16::from_le_bytes([a[0], a[1]]);
        let handle = hdr & 0x0FFF;
        let boundary = (hdr >> 12) & 0x3;
        let len = u16::from_le_bytes([a[2], a[3]]) as usize;
        if len + 4 != a.len() || len == 0 || handle > 0x0EFF {
            return false;
        }
        let payload = &a[4..];
        let link = self.links.entry(handle).or_default();
        let cid = if boundary == 0b01 {
            match link.last_start_cid {
                Some(c) => c,
                None => return true,
            }
        } else {
            if payload.len() < 4 {
                return false;
            }
            let l2len = u16::from_le_bytes([payload[0], payload[1]]) as usize;
            if l2len + 4 < payload.len() {
                return false;
            }
            let cid = u16::from_le_bytes([payload[2], payload[3]]);
            link.last_start_cid = Some(cid);
            let body = &payload[4..];
            if cid == 0x0001 {
                signalling(link, body);
            } else if let Some(cfg) = parse_avdtp_config(body) {
                link.config = Some(cfg);
            }
            cid
        };
        self.acl_packets += 1;
        let t = link.totals.entry(cid).or_default();
        t.0 += len as u64;
        t.1 += 1;
        link.window.push_back((now_ms, cid, len as u32));
        while link.window.front().map(|(t, _, _)| now_ms.saturating_sub(*t) > 10_000).unwrap_or(false) {
            link.window.pop_front();
        }
        true
    }
}

/// L2CAP signalling: connection requests and responses for PSM 0x0019 (AVDTP).
fn signalling(link: &mut Link, body: &[u8]) {
    let mut at = 0;
    while at + 4 <= body.len() {
        let code = body[at];
        let ident = body[at + 1];
        let len = u16::from_le_bytes([body[at + 2], body[at + 3]]) as usize;
        let data = match body.get(at + 4..at + 4 + len) {
            Some(d) => d,
            None => return,
        };
        match code {
            0x02 if data.len() >= 4 => {
                let psm = u16::from_le_bytes([data[0], data[1]]);
                if psm == 0x0019 {
                    link.pending.insert(ident, u16::from_le_bytes([data[2], data[3]]));
                }
            }
            0x03 if data.len() >= 8 => {
                let result = u16::from_le_bytes([data[4], data[5]]);
                if result == 0 {
                    if let Some(scid) = link.pending.remove(&ident) {
                        if !link.avdtp_cids.contains(&scid) {
                            link.avdtp_cids.push(scid);
                        }
                        let dcid = u16::from_le_bytes([data[0], data[1]]);
                        if !link.avdtp_cids.contains(&dcid) {
                            link.avdtp_cids.push(dcid);
                        }
                    }
                }
            }
            _ => {}
        }
        at += 4 + len;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sbc_high_quality_bitrate() {
        // 44.1 kHz joint stereo, 16 blocks, 8 subbands, bitpool 53: the well-known 328 kbit/s.
        assert_eq!(sbc_bitrate(44100, "joint stereo", 16, 8, 53), Some(328));
        assert_eq!(sbc_bitrate(48000, "joint stereo", 16, 8, 51), Some(345));
    }

    #[test]
    fn parses_sbc_set_configuration() {
        // AVDTP single packet, command, SET_CONFIGURATION, ACP SEID 1, INT SEID 1,
        // Media Transport cap, Media Codec: audio, SBC, 44.1 kHz joint, 16/8 loudness, 2..53
        let p = [0x10, 0x03, 0x04, 0x04, 0x01, 0x00, 0x07, 0x06, 0x00, 0x00, 0x21, 0x15, 0x02, 0x35];
        let (name, codec) = parse_avdtp_config(&p).unwrap();
        assert_eq!(name, "SET_CONFIGURATION");
        assert_eq!(
            codec,
            Codec::Sbc { rate: 44100, mode: "joint stereo", blocks: 16, subbands: 8, allocation: "loudness", min_bitpool: 2, max_bitpool: 53 }
        );
    }

    #[test]
    fn parses_aac_configuration() {
        // MPEG-2 AAC LC, 44.1 kHz, 2 channels, VBR, 320000 bit/s
        let cap = [0x00, 0x02, 0x80, 0x01, 0x04, 0x84, 0xE2, 0x00];
        match parse_codec(&cap).unwrap() {
            Codec::Aac { object, rate, channels, vbr, bitrate } => {
                assert_eq!((object, rate, channels, vbr, bitrate), ("MPEG-2 AAC LC", 44100, 2, true, 320000));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn tracks_media_channel_bitrate_through_acl() {
        let mut t = Tracker::default();
        // H4 ACL packets of 600 L2CAP bytes on CID 0x0041, handle 0x000B, every 20 ms.
        for i in 0..50u64 {
            let mut pkt = vec![0x02, 0x0B, 0x20];
            let l2: Vec<u8> = [&600u16.to_le_bytes()[..], &0x0041u16.to_le_bytes()[..], &vec![0u8; 600][..]].concat();
            pkt.extend_from_slice(&(l2.len() as u16).to_le_bytes());
            pkt.extend_from_slice(&l2);
            let mut blob = vec![0xAA; 7];
            blob.extend_from_slice(&pkt);
            assert!(t.feed(i * 20, &blob));
        }
        let (kbps, pps, avg, cid, _) = t.links[&0x000B].media_rate(49 * 20, 5000).unwrap();
        assert_eq!(cid, 0x0041);
        assert!((pps - 51.0).abs() < 1.5, "{pps}");
        assert!((avg - 604.0).abs() < 0.1);
        assert!(kbps > 240.0 && kbps < 250.0, "{kbps}");
    }
}
