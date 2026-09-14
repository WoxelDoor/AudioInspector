//! Small helpers: time, strings, GUIDs, hex.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use windows::core::{GUID, PWSTR};
use windows::Win32::Foundation::{FILETIME, SYSTEMTIME};
use windows::Win32::System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime};

/// 100-ns intervals between 1601-01-01 and 1970-01-01.
const FILETIME_UNIX_DIFF: u64 = 116_444_736_000_000_000;

pub fn filetime_to_system(ft: u64) -> Option<SystemTime> {
    if ft <= FILETIME_UNIX_DIFF {
        return None;
    }
    Some(UNIX_EPOCH + Duration::from_nanos((ft - FILETIME_UNIX_DIFF).saturating_mul(100)))
}

fn system_to_filetime(t: SystemTime) -> u64 {
    let d = t.duration_since(UNIX_EPOCH).unwrap_or_default();
    FILETIME_UNIX_DIFF + (d.as_nanos() / 100) as u64
}

/// Local wall-clock time, "2026-09-14 16:08:46".
pub fn fmt_time(t: SystemTime) -> String {
    let ft = system_to_filetime(t);
    let f = FILETIME { dwLowDateTime: ft as u32, dwHighDateTime: (ft >> 32) as u32 };
    unsafe {
        let mut utc = SYSTEMTIME::default();
        let mut local = SYSTEMTIME::default();
        if FileTimeToSystemTime(&f, &mut utc).is_err() {
            return "?".into();
        }
        if SystemTimeToTzSpecificLocalTime(None, &utc, &mut local).is_err() {
            local = utc;
        }
        format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
            local.wYear, local.wMonth, local.wDay, local.wHour, local.wMinute, local.wSecond
        )
    }
}

/// "12 s ago", "5 min ago", "3 h ago", "2 d ago".
pub fn fmt_ago(t: SystemTime) -> String {
    let s = SystemTime::now().duration_since(t).map(|d| d.as_secs()).unwrap_or(0);
    match s {
        0..=59 => format!("{s} s ago"),
        60..=3599 => format!("{} min ago", s / 60),
        3600..=172_799 => format!("{} h ago", s / 3600),
        _ => format!("{} d ago", s / 86_400),
    }
}

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn from_wide(w: &[u16]) -> String {
    let end = w.iter().position(|&c| c == 0).unwrap_or(w.len());
    String::from_utf16_lossy(&w[..end])
}

/// Reads a COM-allocated PWSTR and frees it.
pub unsafe fn take_pwstr(p: PWSTR) -> String {
    if p.is_null() {
        return String::new();
    }
    let s = p.to_string().unwrap_or_default();
    windows::Win32::System::Com::CoTaskMemFree(Some(p.0 as *const _));
    s
}

/// `{XXXXXXXX-XXXX-XXXX-XXXX-XXXXXXXXXXXX}`, upper case.
pub fn guid_str(g: &GUID) -> String {
    format!("{{{g:?}}}")
}

/// Parses a GUID with or without braces.
pub fn parse_guid(s: &str) -> Option<GUID> {
    let hex: String = s.trim().trim_start_matches('{').trim_end_matches('}').chars().filter(|c| *c != '-').collect();
    if hex.len() != 32 {
        return None;
    }
    u128::from_str_radix(&hex, 16).ok().map(GUID::from_u128)
}

/// Bluetooth address as `00:00:5E:00:53:01`.
pub fn mac(addr: u64) -> String {
    (0..6).rev().map(|i| format!("{:02X}", (addr >> (i * 8)) & 0xFF)).collect::<Vec<_>>().join(":")
}

pub fn hex_bytes(b: &[u8], max: usize) -> String {
    let mut s = b.iter().take(max).map(|x| format!("{x:02x}")).collect::<Vec<_>>().join(" ");
    if b.len() > max {
        s.push_str(&format!(" ... ({} bytes)", b.len()));
    }
    s
}

/// Like [`hex_bytes`], with every 8-byte little-endian value in the kernel address range
/// (0xFFFF8000_00000000 and up) printed as `PTR`: a shared log must not show kernel layout.
pub fn hex_bytes_masked(b: &[u8], max: usize) -> String {
    let n = b.len().min(max);
    let mut parts: Vec<String> = Vec::new();
    let mut i = 0;
    while i < n {
        if i + 8 <= b.len() && b[i + 7] == 0xFF && b[i + 6] == 0xFF && b[i + 5] >= 0x80 {
            parts.push("PTR".into());
            i += 8;
        } else {
            parts.push(format!("{:02x}", b[i]));
            i += 1;
        }
    }
    let mut s = parts.join(" ");
    if b.len() > max {
        s.push_str(&format!(" ... ({} bytes)", b.len()));
    }
    s
}

/// "0x80070005 Access is denied."
pub fn err_text(e: &windows::core::Error) -> String {
    let msg = e.message();
    let msg = msg.trim();
    if msg.is_empty() {
        format!("0x{:08X}", e.code().0 as u32)
    } else {
        format!("0x{:08X} {}", e.code().0 as u32, msg)
    }
}

pub fn le_u16(b: &[u8], at: usize) -> Option<u16> {
    b.get(at..at + 2).map(|s| u16::from_le_bytes([s[0], s[1]]))
}

pub fn le_u32(b: &[u8], at: usize) -> Option<u32> {
    b.get(at..at + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

pub fn le_u64(b: &[u8], at: usize) -> Option<u64> {
    b.get(at..at + 8).map(|s| u64::from_le_bytes(s.try_into().unwrap()))
}

/// "1 234 567" style thousands for large counters.
pub fn thousands(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(' ');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mac_formats_msb_first() {
        // 00-00-5E-00-53-xx is reserved for documentation (RFC 7042)
        assert_eq!(mac(0x00005E005301), "00:00:5E:00:53:01");
    }

    #[test]
    fn guid_round_trip() {
        let g = parse_guid("{29CE83D4-7A82-4744-BD1D-ABEC85321DD6}").unwrap();
        assert_eq!(guid_str(&g), "{29CE83D4-7A82-4744-BD1D-ABEC85321DD6}");
        assert!(parse_guid("not-a-guid").is_none());
    }

    #[test]
    fn filetime_epoch() {
        let t = filetime_to_system(FILETIME_UNIX_DIFF + 10_000_000).unwrap();
        assert_eq!(t.duration_since(UNIX_EPOCH).unwrap().as_secs(), 1);
    }

    #[test]
    fn kernel_addresses_are_masked_in_logs() {
        // a kernel pointer, then a stream position that must stay readable
        let b = [0x40, 0x49, 0xcb, 0x5d, 0x8c, 0xd0, 0xff, 0xff, 0x80, 0x37, 0x0b, 0x01, 0x00, 0x00, 0x00, 0x00];
        assert_eq!(hex_bytes_masked(&b, 96), "PTR 80 37 0b 01 00 00 00 00");
        assert_eq!(hex_bytes_masked(&[0x13, 0x05], 96), "13 05");
    }

    #[test]
    fn thousands_groups() {
        assert_eq!(thousands(1234567), "1 234 567");
        assert_eq!(thousands(999), "999");
    }
}
