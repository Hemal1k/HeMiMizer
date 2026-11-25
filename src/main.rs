// HeMiMizer – gets a freshly installed Windows ready: drivers, programs and the settings of the old PC.
//   HeMiMizer.exe            the window: back up this PC, set it up, the tools
//   HeMiMizer.exe --resume   continues after the restart (the scheduled task "HeMiMizer - continue")
//   HeMiMizer.exe --capture  saves the settings of this PC to the folder config next to the exe (no Wi-Fi passwords)
//   HeMiMizer.exe --check    shows the newest versions of the programs and drivers
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
// every unsafe block says why it is sound (audit 2026-11-24)
#![warn(clippy::undocumented_unsafe_blocks)]

mod app;
mod appdata;
mod backup;
mod catalog;
mod config;
mod crypto;
mod debloat;
mod look;
mod options;
mod ownership;
mod downloads;
mod iso;
mod msiso;
mod drivers;
mod hardware;
mod i18n;
mod install;
mod plan;
mod printscreen;
mod selfupdate;
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
    // SAFETY: both strings end with a NUL and live until the call returns
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
        "======== HeMiMizer {} started {:?} | {product} {display} build {build} | PC {} | user {} | exe {} | config {} (exists: {})",
        env!("CARGO_PKG_VERSION"),
        args,
        sys::env("COMPUTERNAME", "?"),
        sys::env("USERNAME", "?"),
        std::env::current_exe().map(|e| e.display().to_string()).unwrap_or_default(),
        config::dir().display(),
        config::dir().exists()
    ));
    std::panic::set_hook(Box::new(|info| sys::log(&format!("PANIC: {info}"))));
    // what HeMiMizer runs as administrator lies only in its protected folder (%ProgramData%\HeMiMizer); without it
    // nothing is run (audit 2026-11-24)
    if let Err(e) = sys::protect_state_dir() {
        sys::log(&format!("protected folder: {e}"));
        message(
            "HeMiMizer",
            &format!(
                "{}\n\n{e}",
                tr(
                    "HeMiMizer cannot prepare its protected work folder. Delete the folder HeMiMizer in C:\\ProgramData and start HeMiMizer again.",
                    "HeMiMizer nemůže připravit svou chráněnou pracovní složku. Smaž složku HeMiMizer v C:\\ProgramData a spusť HeMiMizer znovu."
                )
            ),
        );
        return Ok(());
    }
    // the wizard needs to know the hardware: found out meanwhile
    hardware::detect_in_background();

    if args.iter().any(|a| a == "--check") {
        let mut text = report(steps::check_links());
        for l in drivers::local_installers() {
            text.push_str(&format!("\n{}: {:?} {} ({})", tr("Next to the exe", "Vedle exe"), l.kind, l.path.display(), l.version));
        }
        sys::tidy_state_dir();
        message(tr("HeMiMizer – newest versions", "HeMiMizer – nejnovější verze"), &text);
        return Ok(());
    }
    if args.iter().any(|a| a == "--capture") {
        let text = report(config::capture());
        sys::tidy_state_dir();
        message(tr("HeMiMizer – settings captured", "HeMiMizer – nastavení zachyceno"), &text);
        return Ok(());
    }
    // Debug builds only: the download links of Microsoft without the window (--test-msiso)
    if cfg!(debug_assertions) && args.first().map(String::as_str) == Some("--test-msiso") {
        let r = msiso::start(msiso::Product::Windows11).and_then(|s| {
            let langs = msiso::languages(&s)?;
            let cz = langs.iter().find(|l| l.name == "Czech").cloned().ok_or("no Czech")?;
            let (url, file) = msiso::link(&s, &cz)?;
            Ok(format!("{} languages; {file}; {} bytes; {}", langs.len(), msiso::size(&url), &url[..60]))
        });
        println!("{r:?}");
        return Ok(());
    }
    // Debug builds only: make a smaller ISO without the window (--test-iso <iso> <out iso> <out log>), the Pro edition
    if cfg!(debug_assertions) && args.first().map(String::as_str) == Some("--test-iso") && args.len() >= 4 {
        let out = std::path::PathBuf::from(&args[3]);
        let lines = std::sync::Mutex::new(Vec::<String>::new());
        let log = |t: String| {
            let mut l = lines.lock().unwrap();
            l.push(t);
            let _ = std::fs::write(&out, l.join("
"));
        };
        let iso = std::path::PathBuf::from(&args[1]);
        let eds = iso::editions(&iso);
        log(format!("editions: {eds:?}"));
        let index = eds.ok().and_then(|v| v.iter().find(|(_, n)| n.ends_with(" Pro")).map(|(i, _)| *i)).unwrap_or(1);
        let o = iso::Options { iso, index, out: args[2].clone().into(), chosen: iso::simple(), add_hemimizer: true, ..Default::default() };
        let r = iso::build(&o, &log);
        log(format!("RESULT: {r:?}"));
        return Ok(());
    }
    // Debug builds only: make a USB stick without the window (--test-usb <disk> <iso> <out file> [fat32]), for testing
    if cfg!(debug_assertions) && args.first().map(String::as_str) == Some("--test-usb") && args.len() >= 4 {
        let o = usb::Options {
            disk: args[1].parse().unwrap_or(u32::MAX),
            iso: args[2].clone().into(),
            gpt: true,
            label: usb::iso_label(std::path::Path::new(&args[2])).unwrap_or_else(|| "WIN11 TEST".into()),
            bypass_hw: true,
            bypass_msa: true,
            local_user: "user".into(),
            region: Some(usb::this_pc()),
            ntfs: args.get(4).is_none_or(|a| a != "fat32"),
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
    // the exe the last own update replaced
    selfupdate::remove_old();
    let mode = if args.iter().any(|a| a == "--resume") && !steps::pending_resume().is_empty() { app::Mode::Resume } else { app::Mode::Normal };

    let icon = egui::IconData { rgba: include_bytes!("../assets/HeMiMizer-64.rgba").to_vec(), width: 64, height: 64 };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("HeMiMizer")
            .with_app_id("HeMiMizer")
            .with_inner_size([840.0, 700.0])
            .with_min_inner_size([600.0, 480.0])
            .with_icon(icon)
            // transparent, so the Mica of Windows 11 shows behind the pages
            .with_transparent(true),
        centered: true,
        ..Default::default()
    };
    // Print Screen in the window of HeMiMizer (as administrator Windows does not pass it to the screen capture)
    printscreen::watch();
    let r = eframe::run_native("HeMiMizer", options, Box::new(move |cc| Ok(Box::new(app::HeMiMizerApp::new(cc, mode)))));
    // the protected folder does not stay when nothing is in it (nothing of HeMiMizer stays on the PC)
    sys::tidy_state_dir();
    r
}
