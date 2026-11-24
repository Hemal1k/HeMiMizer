//! Print Screen in the window of HeMiMizer. HeMiMizer runs as administrator, and Windows does not pass the keys of a
//! window with higher rights to the screen capture of the user (Snipping Tool) – Print Screen did nothing while HeMiMizer
//! was in front (the user, 2026-11-23). A keyboard hook of HeMiMizer sees the key then and opens the screen capture
//! itself, as the user (through File Explorer). The key goes on to Windows unchanged; nothing happens while another app is
//! in front (Windows does it there), nor when "Use the Print screen key to open screen capture" is off.

use std::sync::atomic::{AtomicBool, Ordering};
use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetForegroundWindow, GetMessageW, GetWindowThreadProcessId, KBDLLHOOKSTRUCT, MSG, SetWindowsHookExW, TranslateMessage,
    WH_KEYBOARD_LL, WM_KEYUP, WM_SYSKEYUP,
};

const VK_SNAPSHOT: u32 = 0x2C;
static STARTED: AtomicBool = AtomicBool::new(false);

/// Watches Print Screen while HeMiMizer runs (only when it runs as administrator – else Windows does it itself).
pub fn watch() {
    if STARTED.swap(true, Ordering::SeqCst) || !crate::sys::elevated() || !snipping_on() {
        return;
    }
    std::thread::spawn(|| unsafe {
        let hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(on_key), std::ptr::null_mut(), 0);
        if hook.is_null() {
            return;
        }
        // the hook needs a message loop on its thread
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    });
}

/// "Use the Print screen key to open screen capture" (Settings → Accessibility → Keyboard); on when not set.
fn snipping_on() -> bool {
    crate::sys::get_dword(crate::sys::Root::Hkcu, r"Control Panel\Keyboard", "PrintScreenKeyForSnippingEnabled") != Some(0)
}

unsafe extern "system" fn on_key(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        if code >= 0 && (wparam as u32 == WM_KEYUP || wparam as u32 == WM_SYSKEYUP) {
            let k = &*(lparam as *const KBDLLHOOKSTRUCT);
            if k.vkCode == VK_SNAPSHOT && ours_in_front() {
                // as the user, not as administrator: File Explorer opens the link
                let _ = std::process::Command::new(crate::sys::program("explorer.exe")).arg("ms-screenclip:").spawn();
            }
        }
        CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam)
    }
}

fn ours_in_front() -> bool {
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(GetForegroundWindow(), &mut pid) };
    pid == std::process::id()
}
