//! Unicode text onto the Windows clipboard, without depending on eframe's clipboard feature.

use windows::Win32::Foundation::{HANDLE, HWND};
use windows::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};

const CF_UNICODETEXT: u32 = 13;

pub fn set_text(text: &str) -> Result<(), String> {
    let w: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        OpenClipboard(HWND::default()).map_err(|e| format!("OpenClipboard: {e}"))?;
        let result = (|| {
            EmptyClipboard().map_err(|e| format!("EmptyClipboard: {e}"))?;
            let h = GlobalAlloc(GMEM_MOVEABLE, w.len() * 2).map_err(|e| format!("GlobalAlloc: {e}"))?;
            let p = GlobalLock(h) as *mut u16;
            if p.is_null() {
                return Err("GlobalLock failed".to_string());
            }
            std::ptr::copy_nonoverlapping(w.as_ptr(), p, w.len());
            let _ = GlobalUnlock(h);
            SetClipboardData(CF_UNICODETEXT, HANDLE(h.0)).map_err(|e| format!("SetClipboardData: {e}"))?;
            Ok(())
        })();
        let _ = CloseClipboard();
        result
    }
}
