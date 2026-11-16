//! The settings of the programs of the wizard that keep them in files or in the registry of the user: the backup
//! copies them (folders with robocopy, single files, `reg export`), the setup puts them back after the programs are
//! installed. Programs that sync their settings through an account (browsers, chat, games) are not listed. VLC,
//! qBittorrent, Brave and Steam have their own parts in config.rs.

use crate::steps::{Ctx, Res};
use crate::sys;
use std::path::PathBuf;

/// Where a program keeps its settings: `{APPDATA}`, `{LOCALAPPDATA}`, `{DOCUMENTS}` or `{PROFILE}` + a folder or a
/// file, or `HKCU\…` for a key of the registry. `skip`: subfolders not worth copying (caches, logs).
pub struct Data {
    pub id: &'static str,
    pub paths: &'static [&'static str],
    pub skip: &'static [&'static str],
    /// what it takes, for the window
    pub what: &'static str,
}

const fn d(id: &'static str, paths: &'static [&'static str], skip: &'static [&'static str], what: &'static str) -> Data {
    Data { id, paths, skip, what }
}

pub const ALL: &[Data] = &[
    d("7zip", &[r"HKCU\Software\7-Zip"], &[], "Options and the context menu"),
    d("winrar", &[r"HKCU\Software\WinRAR"], &[], "Options"),
    d("notepadpp", &[r"{APPDATA}\Notepad++"], &["backup", "session.xml"], "Options, themes and plug-ins"),
    d("powertoys", &[r"{LOCALAPPDATA}\Microsoft\PowerToys"], &["Logs", "Updates", "WinGet"], "The settings of every tool"),
    d("keepassxc", &[r"{APPDATA}\KeePassXC"], &[], "Options (not the database)"),
    d("obs", &[r"{APPDATA}\obs-studio"], &["logs", "crashes", "updates", "plugin_config\\obs-browser\\Cache"], "Scenes, profiles and settings"),
    d("sharex", &[r"{DOCUMENTS}\ShareX"], &["Screenshots", "Logs", "Backup"], "Settings, hotkeys and uploaders"),
    d("handbrake", &[r"{APPDATA}\HandBrake"], &["logs"], "Presets and options"),
    d("audacity", &[r"{APPDATA}\audacity"], &[], "Settings and macros"),
    d("gimp", &[r"{APPDATA}\GIMP"], &[], "Settings, brushes and plug-ins"),
    d("paintnet", &[r"{APPDATA}\paint.net"], &[], "Settings"),
    d("thunderbird", &[r"{APPDATA}\Thunderbird"], &["Crash Reports"], "The profile with the mail – can be large"),
    d("libreoffice", &[r"{APPDATA}\LibreOffice"], &["4\\user\\backup"], "Settings, dictionaries and templates"),
    d("vscode", &[r"{APPDATA}\Code\User"], &["workspaceStorage", "History", "globalStorage"], "Settings, key bindings and snippets"),
    d("git", &[r"{PROFILE}\.gitconfig"], &[], "Your name, e-mail and options"),
];

pub fn find(id: &str) -> Option<&'static Data> {
    ALL.iter().find(|d| d.id == id && !d.paths.is_empty())
}

/// A path of the table on this PC.
fn resolve(p: &str) -> PathBuf {
    PathBuf::from(
        p.replace("{APPDATA}", &sys::env("APPDATA", ""))
            .replace("{LOCALAPPDATA}", &sys::env("LOCALAPPDATA", ""))
            .replace("{DOCUMENTS}", &sys::documents().display().to_string())
            .replace("{PROFILE}", &sys::profile().display().to_string()),
    )
}

fn reg_key(p: &str) -> Option<&str> {
    p.strip_prefix(r"HKCU\")
}

/// The program has settings on this PC to take along.
pub fn present(id: &str) -> bool {
    find(id).is_some_and(|d| {
        d.paths.iter().any(|p| match reg_key(p) {
            Some(k) => sys::key_exists(sys::Root::Hkcu, k),
            None => resolve(p).exists(),
        })
    })
}

/// apps\<id>\<n> in the backup (n = the place in `paths`).
fn dir(id: &str) -> PathBuf {
    crate::config::home().join("apps").join(id)
}

/// Into the backup.
pub fn capture(id: &str) -> Result<String, String> {
    let d = find(id).ok_or("?")?;
    let to = dir(id);
    let _ = std::fs::remove_dir_all(&to);
    std::fs::create_dir_all(&to).map_err(|e| e.to_string())?;
    let mut n = 0;
    for (i, p) in d.paths.iter().enumerate() {
        let dest = to.join(i.to_string());
        if let Some(key) = reg_key(p) {
            if !sys::key_exists(sys::Root::Hkcu, key) {
                continue;
            }
            std::fs::create_dir_all(&dest).map_err(|e| e.to_string())?;
            sys::run("reg.exe", &["export", &format!(r"HKCU\{key}"), &dest.join("settings.reg").display().to_string(), "/y"])?;
            n += 1;
            continue;
        }
        let src = resolve(p);
        if src.is_dir() {
            robocopy(&src, &dest, d.skip)?;
            n += 1;
        } else if src.is_file() {
            std::fs::create_dir_all(&dest).map_err(|e| e.to_string())?;
            std::fs::copy(&src, dest.join(src.file_name().unwrap_or_default())).map_err(|e| e.to_string())?;
            n += 1;
        }
    }
    if n == 0 {
        let _ = std::fs::remove_dir_all(&to);
        return Err("Not found on this PC".into());
    }
    Ok(d.what.to_string())
}

fn robocopy(from: &std::path::Path, to: &std::path::Path, skip: &[&str]) -> Result<(), String> {
    let (from, to) = (from.display().to_string(), to.display().to_string());
    let mut args = vec![from.as_str(), to.as_str(), "/E", "/COPY:DAT", "/DCOPY:DAT", "/R:1", "/W:1", "/NFL", "/NDL", "/NJH", "/NP"];
    let skip_full: Vec<String> = skip.iter().map(|s| format!(r"{from}\{s}")).collect();
    if !skip.is_empty() {
        args.push("/XD");
        args.extend(skip_full.iter().map(String::as_str));
        // a skipped name may also be a file (session.xml)
        args.push("/XF");
        args.extend(skip.iter().copied());
    }
    let o = sys::exec("robocopy.exe", &args)?;
    if o.code < 8 { Ok(()) } else { Err(format!("robocopy {}: {}", o.code, o.text.trim().lines().last().unwrap_or(""))) }
}

/// The step "Settings of your programs": back from the backup, for every program chosen in the wizard.
pub fn restore(c: &Ctx) -> Res {
    let plan = crate::plan::Plan::load().unwrap_or_default();
    let mut done = Vec::new();
    let mut failed = Vec::new();
    for d in ALL.iter().filter(|d| !d.paths.is_empty() && plan.has(d.id) && dir(d.id).is_dir()) {
        let name = crate::catalog::find(d.id).map(|i| i.name).unwrap_or(d.id);
        c.log(name);
        let mut ok = true;
        for (i, p) in d.paths.iter().enumerate() {
            let from = dir(d.id).join(i.to_string());
            if !from.is_dir() {
                continue;
            }
            let r = if reg_key(p).is_some() {
                std::fs::read(from.join("settings.reg")).map_err(|e| e.to_string()).and_then(|raw| {
                    let u: Vec<u16> = raw.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
                    sys::reg_import(String::from_utf16_lossy(&u).trim_start_matches('\u{feff}'))
                })
            } else {
                let to = resolve(p);
                let file = from.join(to.file_name().unwrap_or_default());
                if file.is_file() {
                    // a single file (.gitconfig)
                    std::fs::create_dir_all(to.parent().unwrap_or(&to)).and_then(|()| std::fs::copy(&file, &to).map(|_| ())).map_err(|e| e.to_string())
                } else {
                    crate::backup::robocopy(&from, &to)
                }
            };
            if let Err(e) = r {
                failed.push(format!("{name}: {e}"));
                ok = false;
            }
        }
        if ok {
            done.push(name);
        }
    }
    if done.is_empty() && failed.is_empty() {
        return Ok("Nothing in the backup".into());
    }
    let text = done.join(", ");
    if failed.is_empty() { Ok(text) } else { Err(format!("{text}\n{}", failed.join("\n"))) }
}
