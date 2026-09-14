//! PROPVARIANT (Core Audio property stores) into the common [`PropValue`].

use windows::core::PROPVARIANT;

use super::devnode::PropValue;

pub fn decode(pv: &PROPVARIANT) -> PropValue {
    unsafe {
        let raw = pv.as_raw();
        let vt = raw.Anonymous.Anonymous.vt;
        let u = &raw.Anonymous.Anonymous.Anonymous;
        match vt {
            0 | 1 => PropValue::Empty,
            2 => PropValue::I16(u.iVal),
            3 | 22 => PropValue::I32(u.lVal),
            4 => PropValue::F32(u.fltVal),
            5 => PropValue::F64(u.dblVal),
            8 => PropValue::Str(wide_z(u.bstrVal)),
            11 => PropValue::Bool(u.boolVal != 0),
            16 => PropValue::I8(u.cVal),
            17 => PropValue::U8(u.bVal),
            18 => PropValue::U16(u.uiVal),
            19 | 23 => PropValue::U32(u.ulVal),
            20 => PropValue::I64(u.hVal),
            21 => PropValue::U64(u.uhVal),
            30 => {
                let p = u.pszVal;
                if p.is_null() {
                    PropValue::Str(String::new())
                } else {
                    PropValue::Str(std::ffi::CStr::from_ptr(p as *const i8).to_string_lossy().into_owned())
                }
            }
            31 => PropValue::Str(wide_z(u.pwszVal)),
            64 => PropValue::FileTime(((u.filetime.dwHighDateTime as u64) << 32) | u.filetime.dwLowDateTime as u64),
            65 => {
                let b = &u.blob;
                if b.pBlobData.is_null() {
                    PropValue::Binary(Vec::new())
                } else {
                    PropValue::Binary(std::slice::from_raw_parts(b.pBlobData, b.cbSize as usize).to_vec())
                }
            }
            72 => {
                if u.puuid.is_null() {
                    PropValue::Empty
                } else {
                    // the raw PROPVARIANT carries windows-core's internal GUID layout
                    let g = &*u.puuid;
                    PropValue::Guid(windows::core::GUID::from_values(g.data1, g.data2, g.data3, g.data4))
                }
            }
            0x101F => {
                let ca = &u.calpwstr;
                let mut v = Vec::new();
                for i in 0..ca.cElems as usize {
                    v.push(wide_z(*ca.pElems.add(i)));
                }
                PropValue::StrList(v)
            }
            0x1011 => {
                let ca = &u.caub;
                PropValue::Binary(std::slice::from_raw_parts(ca.pElems, ca.cElems as usize).to_vec())
            }
            other => PropValue::Other(other as u32, Vec::new()),
        }
    }
}

unsafe fn wide_z(p: *const u16) -> String {
    if p.is_null() {
        return String::new();
    }
    let mut n = 0usize;
    while *p.add(n) != 0 {
        n += 1;
    }
    String::from_utf16_lossy(std::slice::from_raw_parts(p, n))
}
