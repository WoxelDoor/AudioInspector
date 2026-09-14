//! Lookup tables: numbers and GUIDs Windows and the Bluetooth/USB specs use, to names.
//! Sources: Windows SDK 10.0.26100 (ksmedia.h, mmdeviceapi.h), Bluetooth Core and
//! profile specifications, USB-IF class codes. Vendor tables list only entries whose
//! identifiers are certain; anything else prints as a hex number.

use windows::core::GUID;

pub fn form_factor(code: u32) -> &'static str {
    match code {
        0 => "Remote network device",
        1 => "Speakers",
        2 => "Line level",
        3 => "Headphones",
        4 => "Microphone",
        5 => "Headset",
        6 => "Handset",
        7 => "Digital passthrough",
        8 => "S/PDIF",
        9 => "Display (HDMI/DP)",
        _ => "Unknown",
    }
}

/// KSNODETYPE_* (ksmedia.h) that endpoints report as their jack subtype.
pub fn node_type(g: &GUID) -> Option<&'static str> {
    Some(match g.to_u128() {
        0xDFF21BE0_F70F_11D0_B917_00A0C9223196 => "Input (undefined)",
        0xDFF21BE1_F70F_11D0_B917_00A0C9223196 => "Microphone",
        0xDFF21BE2_F70F_11D0_B917_00A0C9223196 => "Desktop microphone",
        0xDFF21BE3_F70F_11D0_B917_00A0C9223196 => "Personal microphone",
        0xDFF21BE4_F70F_11D0_B917_00A0C9223196 => "Omnidirectional microphone",
        0xDFF21BE5_F70F_11D0_B917_00A0C9223196 => "Microphone array",
        0xDFF21BE6_F70F_11D0_B917_00A0C9223196 => "Processing microphone array",
        0xDFF21CE0_F70F_11D0_B917_00A0C9223196 => "Output (undefined)",
        0xDFF21CE1_F70F_11D0_B917_00A0C9223196 => "Speaker",
        0xDFF21CE2_F70F_11D0_B917_00A0C9223196 => "Headphones",
        0xDFF21CE3_F70F_11D0_B917_00A0C9223196 => "Head-mounted display audio",
        0xDFF21CE4_F70F_11D0_B917_00A0C9223196 => "Desktop speaker",
        0xDFF21CE5_F70F_11D0_B917_00A0C9223196 => "Room speaker",
        0xDFF21CE6_F70F_11D0_B917_00A0C9223196 => "Communication speaker",
        0xDFF21CE7_F70F_11D0_B917_00A0C9223196 => "LFE speaker",
        0xDFF21DE1_F70F_11D0_B917_00A0C9223196 => "Handset",
        0xDFF21DE2_F70F_11D0_B917_00A0C9223196 => "Headset",
        0xDFF21DE3_F70F_11D0_B917_00A0C9223196 => "Speakerphone",
        0xDFF21DE4_F70F_11D0_B917_00A0C9223196 => "Echo-suppressing speakerphone",
        0xDFF21DE5_F70F_11D0_B917_00A0C9223196 => "Echo-cancelling speakerphone",
        0xDFF21EE1_F70F_11D0_B917_00A0C9223196 => "Phone line",
        0xDFF21EE2_F70F_11D0_B917_00A0C9223196 => "Telephone",
        0xDFF21FE1_F70F_11D0_B917_00A0C9223196 => "Analog connector",
        0xDFF21FE2_F70F_11D0_B917_00A0C9223196 => "Digital audio interface",
        0xDFF21FE3_F70F_11D0_B917_00A0C9223196 => "Line connector",
        0xDFF21FE4_F70F_11D0_B917_00A0C9223196 => "Legacy audio connector",
        0xDFF21FE5_F70F_11D0_B917_00A0C9223196 => "S/PDIF interface",
        0xD1B9CC2A_F519_417F_91C9_55FA65481001 => "HDMI interface",
        0xE47E4031_3EA6_418D_8F9B_B73843CCBA97 => "DisplayPort interface",
        0x8F42C0B2_91CE_4BCF_9CCD_0E599037AB35 => "Loopback",
        0x28E04F87_4DBE_4F8D_8589_025D209DFB4A => "Speakers (static jack)",
        _ => return None,
    })
}

/// AUDIO_SIGNALPROCESSINGMODE_* (ksmedia.h).
pub fn processing_mode(g: &GUID) -> Option<&'static str> {
    Some(match g.to_u128() {
        0xC18E2F7E_933D_4965_B7D1_1EEF228D2AF3 => "Default",
        0x9E90EA20_B493_4FD1_A1A8_7E1361A956CF => "Raw",
        0x98951333_B9CD_48B1_A0A3_FF40682D73F7 => "Communications",
        0xFC1CFC9B_B9D6_4CFA_B5E0_4BB2166878B2 => "Speech",
        0x9CF2A70B_F377_403B_BD6B_360863E0355C => "Notification",
        0x4780004E_7133_41D8_8C74_660DADD2C0EE => "Media",
        0xB26FEB0D_EC94_477C_9494_D1AB8E753F6E => "Movie",
        0x28941CBA_3BE6_4A78_9A76_30FD91559B64 => "Far-field speech",
        _ => return None,
    })
}

pub fn jack_connection(v: i32) -> &'static str {
    match v {
        0 => "Unknown",
        1 => "3.5 mm jack",
        2 => "6.3 mm jack",
        3 => "ATAPI internal",
        4 => "RCA",
        5 => "Optical",
        6 => "Other digital",
        7 => "Other analog",
        8 => "Multichannel analog DIN",
        9 => "XLR",
        10 => "RJ-11 modem",
        11 => "Combination",
        _ => "?",
    }
}

pub fn jack_geo(v: i32) -> &'static str {
    match v {
        1 => "Rear",
        2 => "Front",
        3 => "Left",
        4 => "Right",
        5 => "Top",
        6 => "Bottom",
        7 => "Rear panel",
        8 => "Riser",
        9 => "Inside mobile lid",
        10 => "Drive bay",
        11 => "HDMI",
        12 => "Outside mobile lid",
        13 => "ATAPI",
        14 => "Not applicable",
        _ => "?",
    }
}

pub fn jack_gen(v: i32) -> &'static str {
    match v {
        0 => "Primary box",
        1 => "Internal",
        2 => "Separate",
        3 => "Other",
        _ => "?",
    }
}

pub fn jack_port(v: i32) -> &'static str {
    match v {
        0 => "Jack",
        1 => "Integrated device",
        2 => "Integrated and jack",
        3 => "Unknown",
        _ => "?",
    }
}

/// Bluetooth LMP/HCI version number to the Core specification it stands for.
pub fn bt_version(lmp: u32) -> &'static str {
    match lmp {
        0 => "1.0b",
        1 => "1.1",
        2 => "1.2",
        3 => "2.0 + EDR",
        4 => "2.1 + EDR",
        5 => "3.0 + HS",
        6 => "4.0",
        7 => "4.1",
        8 => "4.2",
        9 => "5.0",
        10 => "5.1",
        11 => "5.2",
        12 => "5.3",
        13 => "5.4",
        14 => "6.0",
        15 => "6.1",
        _ => "?",
    }
}

/// Bluetooth SIG company identifiers - the certain subset.
pub fn bt_company(id: u32) -> Option<&'static str> {
    Some(match id {
        0x0000 => "Ericsson",
        0x0001 => "Nokia",
        0x0002 => "Intel",
        0x0003 => "IBM",
        0x0004 => "Toshiba",
        0x0005 => "3Com",
        0x0006 => "Microsoft",
        0x0007 => "Lucent",
        0x0008 => "Motorola",
        0x0009 => "Infineon",
        0x000A => "Qualcomm Technologies International (CSR)",
        0x000B => "Silicon Wave",
        0x000C => "Digianswer",
        0x000D => "Texas Instruments",
        0x000F => "Broadcom",
        0x0010 => "Mitel",
        0x0011 => "Widcomm",
        0x0012 => "Zeevo",
        0x0013 => "Atmel",
        0x001D => "Qualcomm",
        0x0025 => "NXP Semiconductors",
        0x0030 => "STMicroelectronics",
        0x0046 => "MediaTek",
        0x0048 => "Marvell",
        0x004C => "Apple",
        0x004F => "APT Ltd",
        0x0055 => "Plantronics",
        0x0057 => "Harman International",
        0x0059 => "Nordic Semiconductor",
        0x005D => "Realtek Semiconductor",
        0x0067 => "GN Audio (Jabra)",
        0x0075 => "Samsung Electronics",
        0x0076 => "Creative Technology",
        0x0082 => "Sennheiser Communications",
        0x0087 => "Garmin",
        0x0094 => "Airoha Technology",
        0x009E => "Bose",
        0x00C4 => "LG Electronics",
        0x00CC => "Beats Electronics",
        0x00D2 => "Dialog Semiconductor",
        0x00D7 => "Qualcomm Technologies",
        0x00E0 => "Google",
        0x012D => "Sony",
        0x0131 => "Cypress Semiconductor",
        0x0171 => "Amazon",
        0x01DA => "Logitech",
        0x027D => "Huawei",
        0x02FF => "Silicon Labs",
        0x038F => "Xiaomi",
        _ => return None,
    })
}

pub fn bt_company_label(id: u32) -> String {
    match bt_company(id) {
        Some(n) => format!("{n} (0x{id:04X})"),
        None => format!("0x{id:04X}"),
    }
}

pub fn cod_major(m: u32) -> &'static str {
    match m {
        0 => "Miscellaneous",
        1 => "Computer",
        2 => "Phone",
        3 => "Network access point",
        4 => "Audio/Video",
        5 => "Peripheral",
        6 => "Imaging",
        7 => "Wearable",
        8 => "Toy",
        9 => "Health",
        31 => "Uncategorized",
        _ => "Reserved",
    }
}

pub fn cod_minor(major: u32, minor: u32) -> Option<&'static str> {
    if major != 4 {
        return None;
    }
    Some(match minor {
        0 => "Uncategorized",
        1 => "Wearable headset",
        2 => "Hands-free",
        4 => "Microphone",
        5 => "Loudspeaker",
        6 => "Headphones",
        7 => "Portable audio",
        8 => "Car audio",
        9 => "Set-top box",
        10 => "HiFi audio",
        11 => "VCR",
        12 => "Video camera",
        13 => "Camcorder",
        14 => "Video monitor",
        15 => "Video display and loudspeaker",
        16 => "Video conferencing",
        18 => "Gaming/toy",
        _ => return None,
    })
}

pub fn cod_services(bits: u32) -> Vec<&'static str> {
    let names = [
        (0, "Limited discoverable"),
        (1, "LE Audio"),
        (3, "Positioning"),
        (4, "Networking"),
        (5, "Rendering"),
        (6, "Capturing"),
        (7, "Object transfer"),
        (8, "Audio"),
        (9, "Telephony"),
        (10, "Information"),
    ];
    names.iter().filter(|(b, _)| bits & (1 << b) != 0).map(|(_, n)| *n).collect()
}

/// A notable subset of LMP feature page 0 (Core Vol 2 Part C 3.3).
pub fn lmp_features(mask: u64) -> Vec<&'static str> {
    let names = [
        (25, "EDR 2 Mb/s"),
        (26, "EDR 3 Mb/s"),
        (31, "eSCO"),
        (45, "EDR eSCO 2 Mb/s"),
        (46, "EDR eSCO 3 Mb/s"),
        (35, "AFH"),
        (41, "Sniff subrating"),
        (51, "Secure Simple Pairing"),
        (38, "LE"),
        (49, "LE + BR/EDR simultaneous"),
        (58, "Enhanced power control"),
        (63, "Extended features"),
    ];
    names.iter().filter(|(b, _)| mask & (1u64 << b) != 0).map(|(_, n)| *n).collect()
}

pub fn a2dp_codec(bytes: &[u8]) -> (String, usize) {
    match bytes.first() {
        None => (String::new(), 0),
        Some(0x00) => ("SBC".into(), 1),
        Some(0x01) => ("MPEG-1,2 Audio (MP3)".into(), 1),
        Some(0x02) => ("AAC".into(), 1),
        Some(0x03) => ("MPEG-D USAC".into(), 1),
        Some(0x04) => ("ATRAC".into(), 1),
        Some(0xFF) if bytes.len() >= 7 => {
            let vendor = u32::from_le_bytes(bytes[1..5].try_into().unwrap());
            let codec = u16::from_le_bytes([bytes[5], bytes[6]]);
            let name = match (vendor, codec) {
                (0x004F, 0x0001) => "aptX".to_string(),
                (0x00D7, 0x0024) => "aptX HD".to_string(),
                (0x00D7, 0x00AD) => "aptX Adaptive".to_string(),
                (0x000A, 0x0002) => "aptX Low Latency".to_string(),
                (0x012D, 0x00AA) => "LDAC".to_string(),
                (0x053A, 0x4C33) => "LHDC V3".to_string(),
                (0x053A, 0x4C35) => "LHDC V5".to_string(),
                (0x00E0, 0x0001) => "Opus".to_string(),
                _ => format!("vendor codec 0x{vendor:08X}/0x{codec:04X}"),
            };
            (name, 7)
        }
        Some(t) => (format!("codec type 0x{t:02X}"), 1),
    }
}

/// Every codec in a packed list: type byte, plus vendor id and codec id after 0xFF.
pub fn a2dp_codec_list(bytes: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        let (name, used) = a2dp_codec(&bytes[at..]);
        if used == 0 {
            break;
        }
        out.push(name);
        at += used;
    }
    out
}

pub fn usb_class(c: u8) -> &'static str {
    match c {
        0x00 => "Defined per interface",
        0x01 => "Audio",
        0x02 => "Communications",
        0x03 => "HID",
        0x05 => "Physical",
        0x06 => "Image",
        0x07 => "Printer",
        0x08 => "Mass storage",
        0x09 => "Hub",
        0x0A => "CDC data",
        0x0B => "Smart card",
        0x0D => "Content security",
        0x0E => "Video",
        0x0F => "Personal healthcare",
        0x10 => "Audio/Video",
        0x11 => "Billboard",
        0x12 => "USB Type-C bridge",
        0xDC => "Diagnostic",
        0xE0 => "Wireless controller",
        0xEF => "Miscellaneous",
        0xFE => "Application specific",
        0xFF => "Vendor specific",
        _ => "Reserved",
    }
}

pub fn usb_vendor(vid: u32) -> Option<&'static str> {
    Some(match vid {
        0x03F0 => "HP",
        0x045E => "Microsoft",
        0x046D => "Logitech",
        0x0499 => "Yamaha",
        0x04E8 => "Samsung",
        0x054C => "Sony",
        0x0582 => "Roland",
        0x05AC => "Apple",
        0x05E3 => "Genesys Logic",
        0x0763 => "M-Audio",
        0x07FD => "MOTU",
        0x0951 => "Kingston (HyperX)",
        0x0B05 => "ASUS",
        0x0BDA => "Realtek",
        0x0D8C => "C-Media",
        0x1038 => "SteelSeries",
        0x1235 => "Focusrite-Novation",
        0x1395 => "Sennheiser / EPOS",
        0x1397 => "Behringer",
        0x1532 => "Razer",
        0x17CC => "Native Instruments",
        0x18D1 => "Google",
        0x19B5 => "Bowers & Wilkins",
        0x20B1 => "XMOS",
        0x2109 => "VIA Labs",
        0x2357 => "TP-Link",
        0x262A => "Savitech",
        0x8087 => "Intel",
        _ => return None,
    })
}

pub fn pci_vendor(vid: u32) -> Option<&'static str> {
    Some(match vid {
        0x1002 => "AMD (ATI)",
        0x1013 => "Cirrus Logic",
        0x1022 => "AMD",
        0x10DE => "NVIDIA",
        0x10EC => "Realtek",
        0x1102 => "Creative Labs",
        0x1106 => "VIA",
        0x111D => "IDT",
        0x13F6 => "C-Media",
        0x14F1 => "Conexant",
        0x15AD => "VMware",
        0x8086 => "Intel",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codec_list_of_an_aptx_adaptive_headset() {
        // pid 4 of {29CE83D4-...} as stored for a headset offering aptX Adaptive, aptX, AAC, SBC
        let raw = [0xff, 0xd7, 0x00, 0x00, 0x00, 0xad, 0x00, 0xff, 0x4f, 0x00, 0x00, 0x00, 0x01, 0x00, 0x02, 0x00];
        assert_eq!(a2dp_codec_list(&raw), vec!["aptX Adaptive", "aptX", "AAC", "SBC"]);
        assert_eq!(a2dp_codec(&[0x02]).0, "AAC");
    }

    #[test]
    fn class_of_device_of_headphones() {
        let raw = 0x240418u32;
        assert_eq!(cod_major((raw >> 8) & 0x1F), "Audio/Video");
        assert_eq!(cod_minor(4, (raw >> 2) & 0x3F), Some("Headphones"));
        assert_eq!(cod_services(raw >> 13), vec!["Rendering", "Audio"]);
    }

    #[test]
    fn lmp_features_of_a_bluetooth_5_2_headset() {
        let f = lmp_features(0x875BFFDBFE8FFEFF);
        for want in ["EDR 2 Mb/s", "EDR 3 Mb/s", "eSCO", "Secure Simple Pairing", "LE"] {
            assert!(f.contains(&want), "{want} missing from {f:?}");
        }
    }
}
