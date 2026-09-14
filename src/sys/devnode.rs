//! PnP devnodes and device interfaces through cfgmgr32: properties, parents, lists.
//! Works for present and remembered (phantom) devnodes, unelevated.

use std::time::SystemTime;

use windows::core::{GUID, PCWSTR};
use windows::Win32::Devices::DeviceAndDriverInstallation::{
    CM_Get_DevNode_PropertyW, CM_Get_DevNode_Property_Keys, CM_Get_Device_IDW, CM_Get_Device_ID_ListW,
    CM_Get_Device_ID_List_SizeW, CM_Get_Device_Interface_ListW, CM_Get_Device_Interface_List_SizeW,
    CM_Get_Device_Interface_PropertyW, CM_Get_Device_Interface_Property_KeysW, CM_Get_Parent, CM_Locate_DevNodeW,
    CM_GET_DEVICE_INTERFACE_LIST_ALL_DEVICES, CM_GET_DEVICE_INTERFACE_LIST_PRESENT, CM_LOCATE_DEVNODE_PHANTOM,
    CR_BUFFER_SMALL, CR_SUCCESS,
};
use windows::Win32::Devices::Properties::{DEVPROPKEY, DEVPROPTYPE};

use crate::util::{filetime_to_system, fmt_time, guid_str, hex_bytes, wide};

const CM_GETIDLIST_FILTER_NONE: u32 = 0;
const CM_GETIDLIST_FILTER_PRESENT: u32 = 0x100;

#[derive(Clone, Debug, PartialEq)]
pub enum PropValue {
    Empty,
    Bool(bool),
    I8(i8),
    U8(u8),
    I16(i16),
    U16(u16),
    I32(i32),
    U32(u32),
    I64(i64),
    U64(u64),
    F32(f32),
    F64(f64),
    Guid(GUID),
    FileTime(u64),
    Str(String),
    StrList(Vec<String>),
    Binary(Vec<u8>),
    Other(u32, Vec<u8>),
}

impl PropValue {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            PropValue::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_list(&self) -> Vec<String> {
        match self {
            PropValue::StrList(v) => v.clone(),
            PropValue::Str(s) => vec![s.clone()],
            _ => Vec::new(),
        }
    }

    pub fn as_u64(&self) -> Option<u64> {
        Some(match *self {
            PropValue::U8(v) => v as u64,
            PropValue::U16(v) => v as u64,
            PropValue::U32(v) => v as u64,
            PropValue::U64(v) => v,
            PropValue::I8(v) => v as u64,
            PropValue::I16(v) => v as u64,
            PropValue::I32(v) => v as u64,
            PropValue::I64(v) => v as u64,
            PropValue::Bool(b) => b as u64,
            _ => return None,
        })
    }

    pub fn as_u32(&self) -> Option<u32> {
        self.as_u64().map(|v| v as u32)
    }

    pub fn as_bool(&self) -> Option<bool> {
        match *self {
            PropValue::Bool(b) => Some(b),
            _ => self.as_u64().map(|v| v != 0),
        }
    }

    pub fn as_guid(&self) -> Option<GUID> {
        match self {
            PropValue::Guid(g) => Some(*g),
            _ => None,
        }
    }

    pub fn as_time(&self) -> Option<SystemTime> {
        match *self {
            PropValue::FileTime(ft) => filetime_to_system(ft),
            _ => None,
        }
    }

    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            PropValue::Binary(b) => Some(b),
            PropValue::Other(_, b) => Some(b),
            _ => None,
        }
    }

    pub fn display(&self) -> String {
        match self {
            PropValue::Empty => String::new(),
            PropValue::Bool(b) => b.to_string(),
            PropValue::I8(v) => v.to_string(),
            PropValue::U8(v) => v.to_string(),
            PropValue::I16(v) => v.to_string(),
            PropValue::U16(v) => format!("{v} (0x{v:04X})"),
            PropValue::I32(v) => v.to_string(),
            PropValue::U32(v) => format!("{v} (0x{v:X})"),
            PropValue::I64(v) => v.to_string(),
            PropValue::U64(v) => format!("{v} (0x{v:X})"),
            PropValue::F32(v) => v.to_string(),
            PropValue::F64(v) => v.to_string(),
            PropValue::Guid(g) => guid_str(g),
            PropValue::FileTime(ft) => match filetime_to_system(*ft) {
                Some(t) => fmt_time(t),
                None => format!("FILETIME {ft}"),
            },
            PropValue::Str(s) => s.clone(),
            PropValue::StrList(v) => v.join(" ; "),
            PropValue::Binary(b) => hex_bytes(b, 48),
            PropValue::Other(t, b) => format!("type 0x{t:X}: {}", hex_bytes(b, 48)),
        }
    }
}

fn utf16_units(data: &[u8]) -> Vec<u16> {
    data.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect()
}

pub fn decode(ty: u32, d: &[u8]) -> PropValue {
    const ARRAY: u32 = 0x1000;
    const LIST: u32 = 0x2000;
    let fixed = |n: usize| d.len() >= n;
    match ty {
        0x0 | 0x1 => PropValue::Empty,
        0x2 if fixed(1) => PropValue::I8(d[0] as i8),
        0x3 if fixed(1) => PropValue::U8(d[0]),
        0x4 if fixed(2) => PropValue::I16(i16::from_le_bytes([d[0], d[1]])),
        0x5 if fixed(2) => PropValue::U16(u16::from_le_bytes([d[0], d[1]])),
        0x6 if fixed(4) => PropValue::I32(i32::from_le_bytes(d[..4].try_into().unwrap())),
        0x7 | 0x17 | 0x18 if fixed(4) => PropValue::U32(u32::from_le_bytes(d[..4].try_into().unwrap())),
        0x8 if fixed(8) => PropValue::I64(i64::from_le_bytes(d[..8].try_into().unwrap())),
        0x9 if fixed(8) => PropValue::U64(u64::from_le_bytes(d[..8].try_into().unwrap())),
        0xA if fixed(4) => PropValue::F32(f32::from_le_bytes(d[..4].try_into().unwrap())),
        0xB if fixed(8) => PropValue::F64(f64::from_le_bytes(d[..8].try_into().unwrap())),
        // GUID struct layout (Data1..Data3 little-endian, Data4 as bytes), not a u128
        0xD if fixed(16) => PropValue::Guid(GUID::from_values(
            u32::from_le_bytes(d[0..4].try_into().unwrap()),
            u16::from_le_bytes([d[4], d[5]]),
            u16::from_le_bytes([d[6], d[7]]),
            d[8..16].try_into().unwrap(),
        )),
        0x10 if fixed(8) => PropValue::FileTime(u64::from_le_bytes(d[..8].try_into().unwrap())),
        0x11 if fixed(1) => PropValue::Bool(d[0] != 0),
        0x12 | 0x19 => PropValue::Str(String::from_utf16_lossy(&utf16_units(d)).trim_end_matches('\0').to_string()),
        t if t == (0x12 | LIST) => PropValue::StrList(
            String::from_utf16_lossy(&utf16_units(d)).split('\0').filter(|s| !s.is_empty()).map(String::from).collect(),
        ),
        t if t == (0x3 | ARRAY) => PropValue::Binary(d.to_vec()),
        t => PropValue::Other(t, d.to_vec()),
    }
}

/// Devnode handle for an instance id, including remembered (not present) devnodes.
pub fn locate(instance_id: &str) -> Option<u32> {
    let w = wide(instance_id);
    let mut inst = 0u32;
    let cr = unsafe { CM_Locate_DevNodeW(&mut inst, PCWSTR(w.as_ptr()), CM_LOCATE_DEVNODE_PHANTOM) };
    (cr == CR_SUCCESS).then_some(inst)
}

fn read_prop(inst: u32, k: &DEVPROPKEY) -> Option<PropValue> {
    unsafe {
        let mut ty = DEVPROPTYPE(0);
        let mut size = 0u32;
        let cr = CM_Get_DevNode_PropertyW(inst, k, &mut ty, None, &mut size, 0);
        if cr != CR_BUFFER_SMALL && cr != CR_SUCCESS {
            return None;
        }
        let mut buf = vec![0u8; size as usize];
        if size > 0 {
            let cr = CM_Get_DevNode_PropertyW(inst, k, &mut ty, Some(buf.as_mut_ptr()), &mut size, 0);
            if cr != CR_SUCCESS {
                return None;
            }
        }
        buf.truncate(size as usize);
        Some(decode(ty.0, &buf))
    }
}

pub fn get(instance_id: &str, k: &DEVPROPKEY) -> Option<PropValue> {
    read_prop(locate(instance_id)?, k)
}

pub fn get_str(instance_id: &str, k: &DEVPROPKEY) -> Option<String> {
    get(instance_id, k).and_then(|v| v.as_str().map(String::from)).filter(|s| !s.is_empty())
}

/// Every property on a devnode, in the order cfgmgr32 lists the keys.
pub fn all(instance_id: &str) -> Vec<(DEVPROPKEY, PropValue)> {
    let Some(inst) = locate(instance_id) else { return Vec::new() };
    unsafe {
        let mut count = 0u32;
        let _ = CM_Get_DevNode_Property_Keys(inst, None, &mut count, 0);
        if count == 0 {
            return Vec::new();
        }
        let mut keys = vec![DEVPROPKEY::default(); count as usize];
        if CM_Get_DevNode_Property_Keys(inst, Some(keys.as_mut_ptr()), &mut count, 0) != CR_SUCCESS {
            return Vec::new();
        }
        keys.truncate(count as usize);
        keys.into_iter().filter_map(|k| read_prop(inst, &k).map(|v| (k, v))).collect()
    }
}

fn instance_id_of(inst: u32) -> Option<String> {
    let mut buf = vec![0u16; 512];
    let cr = unsafe { CM_Get_Device_IDW(inst, &mut buf, 0) };
    (cr == CR_SUCCESS).then(|| crate::util::from_wide(&buf))
}

pub fn parent(instance_id: &str) -> Option<String> {
    let inst = locate(instance_id)?;
    let mut p = 0u32;
    let cr = unsafe { CM_Get_Parent(&mut p, inst, 0) };
    if cr == CR_SUCCESS {
        return instance_id_of(p);
    }
    // Remembered devnodes have no live parent link; the property still holds it.
    get_str(instance_id, &super::keys::PARENT)
}

/// Parents from the nearest up, stopping before the root of the tree.
pub fn ancestors(instance_id: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = instance_id.to_string();
    while let Some(p) = parent(&cur) {
        if p.to_uppercase().starts_with("HTREE\\ROOT") || out.len() > 16 || out.contains(&p) {
            break;
        }
        out.push(p.clone());
        cur = p;
    }
    out
}

pub fn all_instance_ids(present_only: bool) -> Vec<String> {
    let flags = if present_only { CM_GETIDLIST_FILTER_PRESENT } else { CM_GETIDLIST_FILTER_NONE };
    unsafe {
        let mut len = 0u32;
        if CM_Get_Device_ID_List_SizeW(&mut len, PCWSTR::null(), flags) != CR_SUCCESS {
            return Vec::new();
        }
        let mut buf = vec![0u16; len as usize];
        if CM_Get_Device_ID_ListW(PCWSTR::null(), &mut buf, flags) != CR_SUCCESS {
            return Vec::new();
        }
        split_multi(&buf)
    }
}

fn split_multi(buf: &[u16]) -> Vec<String> {
    buf.split(|&c| c == 0).filter(|s| !s.is_empty()).map(String::from_utf16_lossy).collect()
}

/// Device interface paths of a class, optionally only those of one devnode.
pub fn interfaces(class: &GUID, device: Option<&str>, present_only: bool) -> Vec<String> {
    let dev_w = device.map(wide);
    let dev = dev_w.as_ref().map(|w| PCWSTR(w.as_ptr())).unwrap_or(PCWSTR::null());
    let flags = if present_only { CM_GET_DEVICE_INTERFACE_LIST_PRESENT } else { CM_GET_DEVICE_INTERFACE_LIST_ALL_DEVICES };
    unsafe {
        let mut len = 0u32;
        if CM_Get_Device_Interface_List_SizeW(&mut len, class, dev, flags) != CR_SUCCESS {
            return Vec::new();
        }
        let mut buf = vec![0u16; len as usize];
        if CM_Get_Device_Interface_ListW(class, dev, &mut buf, flags) != CR_SUCCESS {
            return Vec::new();
        }
        split_multi(&buf)
    }
}

pub fn interface_get(path: &str, k: &DEVPROPKEY) -> Option<PropValue> {
    let w = wide(path);
    unsafe {
        let p = PCWSTR(w.as_ptr());
        let mut ty = DEVPROPTYPE(0);
        let mut size = 0u32;
        let cr = CM_Get_Device_Interface_PropertyW(p, k, &mut ty, None, &mut size, 0);
        if cr != CR_BUFFER_SMALL && cr != CR_SUCCESS {
            return None;
        }
        let mut buf = vec![0u8; size as usize];
        if size > 0 && CM_Get_Device_Interface_PropertyW(p, k, &mut ty, Some(buf.as_mut_ptr()), &mut size, 0) != CR_SUCCESS {
            return None;
        }
        buf.truncate(size as usize);
        Some(decode(ty.0, &buf))
    }
}

pub fn interface_all(path: &str) -> Vec<(DEVPROPKEY, PropValue)> {
    let w = wide(path);
    unsafe {
        let p = PCWSTR(w.as_ptr());
        let mut count = 0u32;
        let _ = CM_Get_Device_Interface_Property_KeysW(p, None, &mut count, 0);
        if count == 0 {
            return Vec::new();
        }
        let mut keys = vec![DEVPROPKEY::default(); count as usize];
        if CM_Get_Device_Interface_Property_KeysW(p, Some(keys.as_mut_ptr()), &mut count, 0) != CR_SUCCESS {
            return Vec::new();
        }
        keys.truncate(count as usize);
        keys.into_iter().filter_map(|k| interface_get(path, &k).map(|v| (k, v))).collect()
    }
}

/// `USB\VID_1235&PID_8211\Y78...` -> "USB".
pub fn enumerator(instance_id: &str) -> String {
    instance_id.split('\\').next().unwrap_or("").to_uppercase()
}

/// A hex field such as `VID_1235` or `VEN_10EC` from an instance or hardware id.
pub fn id_field(id: &str, field: &str) -> Option<u32> {
    let up = id.to_uppercase();
    let tag = format!("{field}_");
    let start = up.find(&tag)? + tag.len();
    let hex: String = up[start..].chars().take_while(|c| c.is_ascii_hexdigit()).collect();
    u32::from_str_radix(&hex, 16).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_fields_parse() {
        assert_eq!(id_field(r"USB\VID_1235&PID_8211\Y783", "VID"), Some(0x1235));
        assert_eq!(id_field(r"USB\VID_1235&PID_8211\Y783", "PID"), Some(0x8211));
        assert_eq!(id_field(r"HDAUDIO\FUNC_01&VEN_10EC&DEV_1220&SUBSYS_1458A0C3", "DEV"), Some(0x1220));
        assert_eq!(id_field(r"ROOT\MEDIA\0000", "VID"), None);
    }

    #[test]
    fn decodes_string_lists_and_binary() {
        let bytes: Vec<u8> = "a\0bc\0\0".encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
        assert_eq!(decode(0x2012, &bytes), PropValue::StrList(vec!["a".into(), "bc".into()]));
        assert_eq!(decode(0x1003, &[1, 2]), PropValue::Binary(vec![1, 2]));
        assert_eq!(decode(0x3, &[70]), PropValue::U8(70));
    }

    #[test]
    fn decodes_guids_in_struct_layout() {
        // KSCATEGORY_AUDIO {6994AD04-93EF-11D0-A3CC-00A0C9223196} as stored in memory
        let b = [0x04, 0xad, 0x94, 0x69, 0xef, 0x93, 0xd0, 0x11, 0xa3, 0xcc, 0x00, 0xa0, 0xc9, 0x22, 0x31, 0x96];
        assert_eq!(decode(0xD, &b), PropValue::Guid(GUID::from_u128(0x6994AD04_93EF_11D0_A3CC_00A0C9223196)));
    }
}
