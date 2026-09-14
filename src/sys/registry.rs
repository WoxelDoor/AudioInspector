//! Read-only registry access (64-bit view). Nothing here writes.

use windows::core::PCWSTR;
use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Registry::{
    RegCloseKey, RegEnumKeyExW, RegEnumValueW, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CLASSES_ROOT,
    HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_64KEY, REG_VALUE_TYPE,
};

use crate::util::{hex_bytes, wide};

#[derive(Clone, Copy)]
pub enum Hive {
    LocalMachine,
    CurrentUser,
    ClassesRoot,
}

#[derive(Clone, Debug, PartialEq)]
pub enum RegValue {
    Dword(u32),
    Qword(u64),
    Sz(String),
    MultiSz(Vec<String>),
    Binary(Vec<u8>),
    Other(u32, Vec<u8>),
}

impl RegValue {
    pub fn as_u32(&self) -> Option<u32> {
        match *self {
            RegValue::Dword(v) => Some(v),
            RegValue::Qword(v) => Some(v as u32),
            _ => None,
        }
    }

    pub fn as_u64(&self) -> Option<u64> {
        match *self {
            RegValue::Dword(v) => Some(v as u64),
            RegValue::Qword(v) => Some(v),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            RegValue::Sz(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            RegValue::Binary(b) | RegValue::Other(_, b) => Some(b),
            _ => None,
        }
    }

    pub fn display(&self) -> String {
        match self {
            RegValue::Dword(v) => format!("{v} (0x{v:X})"),
            RegValue::Qword(v) => format!("{v} (0x{v:X})"),
            RegValue::Sz(s) => s.clone(),
            RegValue::MultiSz(v) => v.join(" ; "),
            RegValue::Binary(b) => hex_bytes(b, 48),
            RegValue::Other(t, b) => format!("type {t}: {}", hex_bytes(b, 48)),
        }
    }
}

pub struct Key(HKEY);

impl Drop for Key {
    fn drop(&mut self) {
        unsafe {
            let _ = RegCloseKey(self.0);
        }
    }
}

pub fn open(hive: Hive, path: &str) -> Option<Key> {
    let root = match hive {
        Hive::LocalMachine => HKEY_LOCAL_MACHINE,
        Hive::CurrentUser => HKEY_CURRENT_USER,
        Hive::ClassesRoot => HKEY_CLASSES_ROOT,
    };
    open_under(root, path)
}

fn open_under(root: HKEY, path: &str) -> Option<Key> {
    let w = wide(path);
    let mut out = HKEY::default();
    let rc = unsafe { RegOpenKeyExW(root, PCWSTR(w.as_ptr()), 0, KEY_READ | KEY_WOW64_64KEY, &mut out) };
    (rc == ERROR_SUCCESS).then_some(Key(out))
}

fn parse(ty: u32, data: &[u8]) -> RegValue {
    let units = || -> Vec<u16> { data.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect() };
    match ty {
        1 | 2 => RegValue::Sz(String::from_utf16_lossy(&units()).trim_end_matches('\0').to_string()),
        4 if data.len() >= 4 => RegValue::Dword(u32::from_le_bytes(data[..4].try_into().unwrap())),
        7 => RegValue::MultiSz(
            String::from_utf16_lossy(&units()).split('\0').filter(|s| !s.is_empty()).map(String::from).collect(),
        ),
        11 if data.len() >= 8 => RegValue::Qword(u64::from_le_bytes(data[..8].try_into().unwrap())),
        3 => RegValue::Binary(data.to_vec()),
        t => RegValue::Other(t, data.to_vec()),
    }
}

impl Key {
    pub fn open(&self, sub: &str) -> Option<Key> {
        open_under(self.0, sub)
    }

    pub fn subkeys(&self) -> Vec<String> {
        let mut out = Vec::new();
        for i in 0.. {
            let mut name = vec![0u16; 512];
            let mut len = name.len() as u32;
            let rc = unsafe {
                RegEnumKeyExW(self.0, i, windows::core::PWSTR(name.as_mut_ptr()), &mut len, None, windows::core::PWSTR::null(), None, None)
            };
            if rc != ERROR_SUCCESS {
                break;
            }
            out.push(String::from_utf16_lossy(&name[..len as usize]));
        }
        out
    }

    pub fn get(&self, name: &str) -> Option<RegValue> {
        let w = wide(name);
        unsafe {
            let mut ty = REG_VALUE_TYPE(0);
            let mut size = 0u32;
            if RegQueryValueExW(self.0, PCWSTR(w.as_ptr()), None, Some(&mut ty), None, Some(&mut size)) != ERROR_SUCCESS {
                return None;
            }
            let mut buf = vec![0u8; size as usize];
            if RegQueryValueExW(self.0, PCWSTR(w.as_ptr()), None, Some(&mut ty), Some(buf.as_mut_ptr()), Some(&mut size))
                != ERROR_SUCCESS
            {
                return None;
            }
            buf.truncate(size as usize);
            Some(parse(ty.0, &buf))
        }
    }

    /// The unnamed default value.
    pub fn default_value(&self) -> Option<RegValue> {
        self.get("")
    }

    pub fn values(&self) -> Vec<(String, RegValue)> {
        let mut out = Vec::new();
        for i in 0.. {
            let mut name = vec![0u16; 16384];
            let mut name_len = name.len() as u32;
            let mut ty = 0u32;
            let mut size = 0u32;
            let rc = unsafe {
                RegEnumValueW(self.0, i, windows::core::PWSTR(name.as_mut_ptr()), &mut name_len, None, Some(&mut ty), None, Some(&mut size))
            };
            if rc != ERROR_SUCCESS {
                break;
            }
            let mut data = vec![0u8; size as usize];
            let mut name_len2 = name.len() as u32;
            let rc = unsafe {
                RegEnumValueW(
                    self.0,
                    i,
                    windows::core::PWSTR(name.as_mut_ptr()),
                    &mut name_len2,
                    None,
                    Some(&mut ty),
                    Some(data.as_mut_ptr()),
                    Some(&mut size),
                )
            };
            if rc != ERROR_SUCCESS {
                continue;
            }
            data.truncate(size as usize);
            out.push((String::from_utf16_lossy(&name[..name_len2 as usize]), parse(ty, &data)));
        }
        out
    }
}
