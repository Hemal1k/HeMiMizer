//! Print Screen in the window of HeMiMizer. HeMiMizer runs as administrator, and Windows does not pass the keys of a
//! window with higher rights to the screen capture of the user (Snipping Tool) – Print Screen did nothing while HeMiMizer
//! was in front (the user, 2026-11-23). A keyboard hook of HeMiMizer sees the key then and opens the screen capture
//! itself, as the user (through File Explorer). The key goes on to Windows unchanged; nothing happens while another app is
//! in front (Windows does it there), nor when "Use the Print screen key to open screen capture" is off.
//! Fixed 2026-11-26 (it still did nothing): the hook was set without the module of HeMiMizer (a global hook needs it,
//! SetWindowsHookExW may refuse it then – and that failure was silent), and the screen capture was started inside the
//! hook itself – a hook that takes too long is removed by Windows without a word. Now the hook gets the module, a
//! failure goes to the log, and the hook only notes the key: the screen capture starts on a thread of its own.

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
    // SAFETY: `on_key` has the signature of a LowLevelKeyboardProc and lies in the module passed (GetModuleHandleW of
    // null is the exe itself, valid while it runs); MSG is plain data (zeroed is valid) and the
    // message loop runs on the thread that set the hook, as Windows requires
    std::thread::spawn(|| unsafe {
        // a hook for every thread (0) names the module its procedure lies in: HeMiMizer.exe itself
        let module = windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(std::ptr::null());
        let hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(on_key), module, 0);
        if hook.is_null() {
            crate::sys::log(&format!("Print Screen: the keyboard hook was refused ({})", std::io::Error::last_os_error()));
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
    // SAFETY: for code >= 0 and a key message, Windows passes a valid KBDLLHOOKSTRUCT in `lparam`; the arguments
    // go on to CallNextHookEx unchanged. Only the key code is read, nothing is kept or written down.
    unsafe {
        if code >= 0 && (wparam as u32 == WM_KEYUP || wparam as u32 == WM_SYSKEYUP) {
            let k = &*(lparam as *const KBDLLHOOKSTRUCT);
            if k.vkCode == VK_SNAPSHOT && ours_in_front() {
                // not here: the hook returns at once (Windows removes a slow one)
                std::thread::spawn(open_screen_capture);
            }
        }
        CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam)
    }
}

/// The screen capture of Windows (Snipping Tool), as the user, not as administrator: File Explorer opens the link.
fn open_screen_capture() {
    if let Err(e) = std::process::Command::new(crate::sys::program("explorer.exe")).arg("ms-screenclip:").spawn() {
        crate::sys::log(&format!("Print Screen: the screen capture did not start ({e})"));
    }
}

fn ours_in_front() -> bool {
    let mut pid = 0u32;
    // SAFETY: `pid` is a valid out pointer; a null window (none in front) only gives 0
    unsafe { GetWindowThreadProcessId(GetForegroundWindow(), &mut pid) };
    pid == std::process::id()
}
