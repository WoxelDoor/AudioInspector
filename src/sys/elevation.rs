//! Is this process elevated, and relaunching it elevated (UAC prompt).

use std::ffi::c_void;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

use crate::util::wide;

pub fn is_elevated() -> bool {
    unsafe {
        let mut token = HANDLE::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
            return false;
        }
        let mut elev = TOKEN_ELEVATION::default();
        let mut len = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            Some(&mut elev as *mut _ as *mut c_void),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut len,
        )
        .is_ok();
        let _ = CloseHandle(token);
        ok && elev.TokenIsElevated != 0
    }
}

/// Starts this exe again through the UAC prompt. Ok means the new process started;
/// the caller then closes its own window.
pub fn relaunch_elevated(args: &str) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let exe_w = wide(&exe.to_string_lossy());
    let verb = wide("runas");
    let args_w = wide(args);
    let r = unsafe {
        ShellExecuteW(None, PCWSTR(verb.as_ptr()), PCWSTR(exe_w.as_ptr()), PCWSTR(args_w.as_ptr()), PCWSTR::null(), SW_SHOWNORMAL)
    };
    if r.0 as isize > 32 {
        Ok(())
    } else {
        Err(format!("the elevated start did not happen (code {})", r.0 as isize))
    }
}
