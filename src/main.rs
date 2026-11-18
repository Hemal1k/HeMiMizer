// HeMimizer – gets a freshly installed Windows ready: drivers, programs and settings of the author.
//   HeMimizer.exe            asks for the NAS password, then does everything and shows its progress
//   HeMimizer.exe --resume   continues after the restart (the scheduled task "HeMimizer - continue")
//   HeMimizer.exe --capture  saves the settings of this PC to the folder config next to the exe
//   HeMimizer.exe --check    shows the newest versions of the programs and drivers
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod appdata;
mod backup;
mod catalog;
mod config;
mod crypto;
mod debloat;
mod look;
mod options;
mod downloads;
mod drivers;
mod hardware;
mod i18n;
mod install;
mod plan;
mod programs;
mod steps;
mod shortcuts;
mod sys;
mod theme;
mod tweaks;
mod usb;
mod widgets;
mod windows;

use eframe::egui;
use i18n::tr;

/// A Windows message box (the release has no console).
fn message(title: &str, text: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONINFORMATION, MB_OK, MessageBoxW};
    let (t, m) = (sys::wide(title), sys::wide(text));
    unsafe { MessageBoxW(std::ptr::null_mut(), m.as_ptr(), t.as_ptr(), MB_OK | MB_ICONINFORMATION) };
    if cfg!(debug_assertions) {
        println!("{text}");
    }
}

fn report(rows: Vec<(String, Result<String, String>)>) -> String {
    rows.into_iter()
        .map(|(n, r)| match r {
            Ok(v) => format!("✓ {n}: {v}"),
            Err(e) => format!("✗ {n}: {e}"),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // a packed backup next to the exe: unpacked first (before the log makes the folder)
    backup::unpack_if_packed();
    config::tidy_up();
    sys::log_init();
    let build = sys::get_sz(sys::Root::Hklm, r"SOFTWARE\Microsoft\Windows NT\CurrentVersion", "CurrentBuild").unwrap_or_default();
    let display = sys::get_sz(sys::Root::Hklm, r"SOFTWARE\Microsoft\Windows NT\CurrentVersion", "DisplayVersion").unwrap_or_default();
    let product = sys::get_sz(sys::Root::Hklm, r"SOFTWARE\Microsoft\Windows NT\CurrentVersion", "ProductName").unwrap_or_default();
    sys::log(&format!(
        "======== HeMimizer {} started {:?} | {product} {display} build {build} | PC {} | user {} | exe {} | config {} (exists: {})",
        env!("CARGO_PKG_VERSION"),
        args,
        sys::env("COMPUTERNAME", "?"),
        sys::env("USERNAME", "?"),
        std::env::current_exe().map(|e| e.display().to_string()).unwrap_or_default(),
        config::dir().display(),
        config::dir().exists()
    ));
    std::panic::set_hook(Box::new(|info| sys::log(&format!("PANIC: {info}"))));
    // the wizard needs to know the hardware: found out meanwhile
    hardware::detect_in_background();

    if args.iter().any(|a| a == "--check") {
        let mut text = report(steps::check_links());
        for l in drivers::local_installers() {
            text.push_str(&format!("\n{}: {:?} {} ({})", tr("Next to the exe", "Vedle exe"), l.kind, l.path.display(), l.version));
        }
        message(tr("HeMimizer – newest versions", "HeMimizer – nejnovější verze"), &text);
        return Ok(());
    }
    if args.iter().any(|a| a == "--capture") {
        let text = report(config::capture());
        message(tr("HeMimizer – settings captured", "HeMimizer – nastavení zachyceno"), &text);
        return Ok(());
    }
    // Debug builds only: try the removal at the end on a copy of the exe
    // Debug builds only: make a USB stick without the window (--test-usb <disk> <iso> <out file>), for testing
    if cfg!(debug_assertions) && args.first().map(String::as_str) == Some("--test-usb") && args.len() >= 4 {
        let o = usb::Options {
            disk: args[1].parse().unwrap_or(u32::MAX),
            iso: args[2].clone().into(),
            gpt: true,
            label: "WIN11 TEST".into(),
            bypass_hw: true,
            bypass_msa: true,
            local_user: "user".into(),
            regional: true,
            no_privacy: true,
            no_bitlocker: true,
        };
        let out = std::path::PathBuf::from(&args[3]);
        let lines = std::sync::Mutex::new(Vec::<String>::new());
        let log = |t: String| {
            let mut l = lines.lock().unwrap();
            if l.last() != Some(&t) && !(t.starts_with("Copying") && l.last().is_some_and(|x| x.starts_with("Copying"))) {
                l.push(t);
            }
            let _ = std::fs::write(&out, l.join("
"));
        };
        let r = usb::make(&o, &log);
        log(format!("RESULT: {r:?}"));
        return Ok(());
    }
    if cfg!(debug_assertions) && args.iter().any(|a| a == "--test-remove") {
        steps::remove_myself();
        return Ok(());
    }
    let mode = if args.iter().any(|a| a == "--resume") && !steps::pending_resume().is_empty() { app::Mode::Resume } else { app::Mode::Normal };

    let icon = egui::IconData { rgba: include_bytes!("../assets/HeMimizer-64.rgba").to_vec(), width: 64, height: 64 };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("HeMimizer")
            .with_app_id("HeMimizer")
            .with_inner_size([840.0, 700.0])
            .with_min_inner_size([600.0, 480.0])
            .with_icon(icon)
            // transparent, so the Mica of Windows 11 shows behind the pages
            .with_transparent(true),
        centered: true,
        ..Default::default()
    };
    eframe::run_native("HeMimizer", options, Box::new(move |cc| Ok(Box::new(app::HeMimizerApp::new(cc, mode)))))
}
