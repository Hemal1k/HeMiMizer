//! The step "Shortcuts and start": what the user chose in the wizard for each new program – a shortcut on the desktop,
//! pinned to Start, pinned to the taskbar, started with Windows. Nothing else is touched: a program the wizard did not
//! install, and a pin the user did not tick, stay as they are.
//!
//! The shortcut of a program is found in the Start menu by its name. Start pins go through the "Pin to Start" verb of
//! the shortcut; Windows does not let programs pin to the taskbar, so the taskbar pins go through the taskbar layout of
//! Windows 11 (an XML file with PinListPlacement="Append" – the pins already there stay, the user can unpin them).

use crate::catalog::{self, Group};
use crate::plan::{Plan, in_list};
use crate::steps::{Ctx, Res};
use crate::sys::{self, Root};
use std::path::{Path, PathBuf};

/// Programs that have no shortcut of their own (runtimes, the command line).
const NO_SHORTCUT: &[&str] = &["java", "nodejs"];

/// The new programs the wizard can make shortcuts for: everything chosen except drivers and runtimes.
pub fn programs(plan: &Plan) -> Vec<&'static catalog::Item> {
    plan.apps
        .iter()
        .filter_map(|id| catalog::find(id))
        .filter(|i| matches!(i.group, Group::App | Group::Browser | Group::Hemi) && !NO_SHORTCUT.contains(&i.id))
        .collect()
}

/// The program starts itself with Windows (its Run value is known), so the start can be switched on or off.
pub fn can_start(id: &str) -> bool {
    crate::windows::STARTS_ITSELF.iter().any(|(i, _)| *i == id)
}

fn norm(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

/// Is the shortcut `file` (its name without .lnk) the one of the program `name`?
fn matches(name: &str, file: &str) -> bool {
    let (n, f) = (norm(name), norm(file));
    if f.len() < 2 || ["uninstall", "readme", "help", "website", "documentation", "releasenotes", "manual"].iter().any(|w| f.contains(w)) {
        return false;
    }
    f == n || f.contains(&n) || (f.len() >= 3 && n.contains(&f)) || (f.len() == 2 && n.starts_with(&f))
}

fn lnks(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            lnks(&p, out);
        } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("lnk")) {
            out.push(p);
        }
    }
}

fn stem(p: &Path) -> String {
    p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

/// The shortcut of the program in the Start menu (of all users or of this user); the shortest name that fits.
fn find_lnk(name: &str, all: &[PathBuf]) -> Option<PathBuf> {
    all.iter().filter(|p| matches(name, &stem(p))).min_by_key(|p| stem(p).len()).cloned()
}

fn user_desktop() -> PathBuf {
    sys::known_folder(&windows_sys::Win32::UI::Shell::FOLDERID_Desktop).unwrap_or_else(|| sys::profile().join("Desktop"))
}

fn user_start_menu() -> PathBuf {
    PathBuf::from(sys::env("APPDATA", "")).join(r"Microsoft\Windows\Start Menu\Programs")
}

/// The layout with the taskbar pins (it must stay: Windows reads it at every sign-in).
fn layout_file() -> PathBuf {
    PathBuf::from(sys::env("ProgramData", r"C:\ProgramData")).join(r"HeMimizer\taskbar.xml")
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('"', "&quot;").replace('<', "&lt;").replace('>', "&gt;")
}

pub fn run(c: &Ctx) -> Res {
    let plan = Plan::load().unwrap_or_default();
    let progs = programs(&plan);
    if progs.is_empty() {
        return Ok("Nothing chosen".into());
    }
    let mut all = Vec::new();
    lnks(&crate::programs::start_menu(), &mut all);
    lnks(&user_start_menu(), &mut all);
    let mut desk = Vec::new();
    for d in [user_desktop(), crate::programs::public_desktop()] {
        if let Ok(rd) = std::fs::read_dir(&d) {
            desk.extend(rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("lnk"))));
        }
    }
    let (mut done, mut missing, mut failed) = (Vec::new(), Vec::new(), Vec::new());
    let mut taskbar = Vec::new();
    for item in &progs {
        let id = item.id;
        let lnk = find_lnk(item.name, &all);
        // the desktop: the shortcut there when ticked, the one of its installer removed when not
        let on_desk: Vec<&PathBuf> = desk.iter().filter(|p| matches(item.name, &stem(p))).collect();
        if in_list(&plan.desktop, id) {
            if on_desk.is_empty() {
                match &lnk {
                    Some(l) => {
                        if let Err(e) = std::fs::copy(l, user_desktop().join(l.file_name().unwrap_or_default())) {
                            failed.push(format!("{}: {e}", item.name));
                        }
                    }
                    None => missing.push(item.name),
                }
            }
        } else {
            for p in on_desk {
                sys::remove_file(p);
            }
        }
        if in_list(&plan.start_pins, id) || in_list(&plan.taskbar, id) {
            let Some(l) = &lnk else {
                missing.push(item.name);
                continue;
            };
            if in_list(&plan.start_pins, id) {
                c.log(format!("Pinning {} to Start", item.name));
                let dir = l.parent().map(|d| d.display().to_string()).unwrap_or_default().replace('\'', "''");
                let file = l.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default().replace('\'', "''");
                if let Err(e) = sys::ps(&format!("(New-Object -ComObject Shell.Application).Namespace('{dir}').ParseName('{file}').InvokeVerb('pintostartscreen')")) {
                    failed.push(format!("{} (Start): {e}", item.name));
                }
            }
            if in_list(&plan.taskbar, id) {
                taskbar.push(l.clone());
            }
        }
        done.push(item.name);
    }
    if !taskbar.is_empty() {
        c.log("Pinning to the taskbar");
        let pins: String = taskbar.iter().map(|l| format!("        <taskbar:DesktopApp DesktopApplicationLinkPath=\"{}\"/>\r\n", xml_escape(&l.display().to_string()))).collect();
        let xml = format!(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\r\n\
<LayoutModificationTemplate xmlns=\"http://schemas.microsoft.com/Start/2014/LayoutModification\" xmlns:defaultlayout=\"http://schemas.microsoft.com/Start/2014/FullDefaultLayout\" xmlns:start=\"http://schemas.microsoft.com/Start/2014/StartLayout\" xmlns:taskbar=\"http://schemas.microsoft.com/Start/2014/TaskbarLayout\" Version=\"1\">\r\n\
  <CustomTaskbarLayoutCollection PinListPlacement=\"Append\">\r\n\
    <defaultlayout:TaskbarLayout>\r\n\
      <taskbar:TaskbarPinList>\r\n{pins}      </taskbar:TaskbarPinList>\r\n\
    </defaultlayout:TaskbarLayout>\r\n\
  </CustomTaskbarLayoutCollection>\r\n\
</LayoutModificationTemplate>\r\n"
        );
        let f = layout_file();
        let r = std::fs::create_dir_all(f.parent().unwrap_or(&f))
            .and_then(|()| std::fs::write(&f, xml))
            .map_err(|e| e.to_string())
            .and_then(|()| sys::set_sz(Root::Hkcu, r"Software\Policies\Microsoft\Windows\Explorer", "StartLayoutFile", &f.display().to_string()))
            .and_then(|()| sys::set_dword(Root::Hkcu, r"Software\Policies\Microsoft\Windows\Explorer", "LockedStartLayout", 1));
        match r {
            // the taskbar reads the layout when File Explorer starts (Windows starts it again by itself, as the user)
            Ok(()) => {
                let _ = sys::exec("taskkill.exe", &["/f", "/im", "explorer.exe"]);
                std::thread::sleep(std::time::Duration::from_secs(10));
                // the pins are the taskbar's own now: the policy and its file go (nothing of HeMimizer stays)
                let pol = r"Software\Policies\Microsoft\Windows\Explorer";
                sys::delete_values(Root::Hkcu, pol, &["StartLayoutFile", "LockedStartLayout"]);
                let _ = std::fs::remove_dir_all(f.parent().unwrap_or(&f));
            }
            Err(e) => failed.push(format!("Taskbar: {e}")),
        }
    }
    startup(&plan);
    let mut text = done.join(", ");
    if !missing.is_empty() {
        text.push_str(&format!("\nNo shortcut found: {}", missing.join(", ")));
    }
    if failed.is_empty() { Ok(text) } else { Err(format!("{text}\n{}", failed.join("\n"))) }
}

/// The programs that start themselves with Windows: on when ticked, off when not – as in Task Manager → Startup apps
/// (StartupApproved; set ahead, before a program adds itself at its first start).
fn startup(plan: &Plan) {
    let approved = r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved";
    for (id, run) in crate::windows::STARTS_ITSELF.iter().filter(|(id, _)| plan.has(id)) {
        let state = if in_list(&plan.startup, id) { 2u8 } else { 3u8 };
        let v = [state, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let _ = sys::set_binary(Root::Hkcu, &format!(r"{approved}\Run"), run, &v);
        // a program for all users has its Run value in HKLM, approved there
        for (key, appr) in [(r"SOFTWARE\Microsoft\Windows\CurrentVersion\Run", "Run"), (r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Run", "Run32")] {
            if sys::value_names(Root::Hklm, key).iter().any(|n| n.eq_ignore_ascii_case(run)) {
                let _ = sys::set_binary(Root::Hklm, &format!(r"{approved}\{appr}"), run, &v);
            }
        }
    }
}
