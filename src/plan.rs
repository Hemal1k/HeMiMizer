//! The choices of the wizard: what to install (catalog ids), online or offline, the name of the PC. Saved into the
//! backup (config\plan.json), so the new PC is set up as chosen on the old one; without a backup the wizard runs on
//! the new PC itself.

use serde_json::{Value, json};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Plan {
    /// catalog ids (drivers, maker tools, the browser, programs, HeMi apps)
    pub apps: Vec<String>,
    /// offline: the backup downloads all installers; online: they come from the internet on the new PC
    pub offline: bool,
    /// the new name of the PC; empty = it keeps its name
    pub pc_name: String,
    /// own folders the backup copies and the setup puts back (paths in the profile written with {PROFILE})
    pub folders: Vec<String>,
    /// the licence terms of the programs chosen were accepted in the wizard
    pub accepted: bool,
    /// the steps of Windows switched on (options.rs); None = the defaults
    pub windows: Option<Vec<String>>,
    /// what the user wants for each new program (catalog ids): a shortcut on the desktop, pinned to Start, pinned to
    /// the taskbar, started with Windows – nothing else is done to them
    pub desktop: Vec<String>,
    pub start_pins: Vec<String>,
    pub taskbar: Vec<String>,
    pub startup: Vec<String>,
}

fn file() -> std::path::PathBuf {
    crate::config::dir().join("plan.json")
}

impl Plan {
    /// The plan of the backup, if there is one.
    pub fn load() -> Option<Plan> {
        let text = std::fs::read_to_string(file()).ok()?;
        let v: Value = serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()?;
        Some(Plan {
            apps: v["apps"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).filter(|id| crate::catalog::find(id).is_some()).collect()).unwrap_or_default(),
            offline: v["offline"].as_bool().unwrap_or(false),
            pc_name: v["pc_name"].as_str().unwrap_or("").trim().to_string(),
            folders: v["folders"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default(),
            accepted: v["accepted"].as_bool().unwrap_or(false),
            windows: v["windows"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()),
            desktop: list(&v["desktop"]),
            start_pins: list(&v["start_pins"]),
            taskbar: list(&v["taskbar"]),
            startup: list(&v["startup"]),
        })
    }

    /// Into the backup; the name of the PC also as pc-name.txt (the step that renames the PC reads it).
    pub fn save(&self) -> Result<(), String> {
        std::fs::create_dir_all(crate::config::dir()).map_err(|e| e.to_string())?;
        let v = json!({ "apps": self.apps, "offline": self.offline, "pc_name": self.pc_name.trim(), "folders": self.folders, "accepted": self.accepted, "windows": self.windows, "desktop": self.desktop, "start_pins": self.start_pins, "taskbar": self.taskbar, "startup": self.startup });
        std::fs::write(file(), serde_json::to_string_pretty(&v).unwrap_or_default()).map_err(|e| e.to_string())?;
        let name = crate::config::dir().join("pc-name.txt");
        if self.pc_name.trim().is_empty() {
            let _ = std::fs::remove_file(name);
            Ok(())
        } else {
            std::fs::write(name, self.pc_name.trim()).map_err(|e| e.to_string())
        }
    }

    pub fn has(&self, id: &str) -> bool {
        self.apps.iter().any(|a| a == id)
    }

    pub fn set(&mut self, id: &str, on: bool) {
        self.apps.retain(|a| a != id);
        if on {
            self.apps.push(id.to_string());
        }
    }

    /// A step of Windows is on (the defaults until the user changed one).
    pub fn windows_on(&self, id: &str) -> bool {
        match &self.windows {
            Some(v) => v.iter().any(|w| w == id),
            None => crate::options::defaults().iter().any(|w| w == id),
        }
    }

    pub fn set_windows(&mut self, id: &str, on: bool) {
        let mut v = self.windows.clone().unwrap_or_else(crate::options::defaults);
        v.retain(|w| w != id);
        if on {
            v.push(id.to_string());
        }
        self.windows = Some(v);
    }

    /// The programs chosen whose licence terms have to be accepted.
    pub fn licensed(&self) -> Vec<&'static crate::catalog::Item> {
        self.apps.iter().filter_map(|id| crate::catalog::find(id)).filter(|i| !i.license.is_empty()).collect()
    }

    /// The browser chosen (one), if any.
    pub fn browser(&self) -> Option<&'static str> {
        crate::catalog::of(crate::catalog::Group::Browser).map(|i| i.id).find(|id| self.has(id))
    }
}

/// A folder for plan.json: one in the profile with {PROFILE} (the new user may have another profile folder).
pub fn store_folder(path: &std::path::Path) -> String {
    let p = path.display().to_string();
    let profile = crate::sys::profile().display().to_string();
    match p.get(..profile.len()) {
        Some(head) if head.eq_ignore_ascii_case(&profile) => format!("{{PROFILE}}{}", &p[profile.len()..]),
        _ => p,
    }
}

/// The folder on this PC.
pub fn resolve_folder(stored: &str) -> std::path::PathBuf {
    std::path::PathBuf::from(stored.replace("{PROFILE}", &crate::sys::profile().display().to_string()))
}

/// A folder of plan.json that may be written to when the backup goes back (robocopy as administrator): in the profile,
/// or anywhere else on a drive except the places of Windows and of the programs (the Windows folder, Program Files,
/// ProgramData, the other profiles, the root of a drive); a full path without "." or ".." and without a link on the
/// way (a junction would lead somewhere else). None = refused (audit 2026-11-24: any path of plan.json was taken).
pub fn safe_folder(stored: &str) -> Option<std::path::PathBuf> {
    use std::path::Component;
    let path = resolve_folder(stored);
    let mut parts = path.components();
    let (Some(Component::Prefix(_)), Some(Component::RootDir)) = (parts.next(), parts.next()) else { return None };
    let rest: Vec<Component> = parts.collect();
    if rest.is_empty() || !rest.iter().all(|c| matches!(c, Component::Normal(_))) {
        return None;
    }
    let norm = |p: &std::path::Path| p.display().to_string().trim_end_matches('\\').to_lowercase();
    let me = norm(&path);
    let inside = |base: &str| !base.is_empty() && (me == base || me.starts_with(&format!("{base}\\")));
    let profile = norm(&crate::sys::profile());
    let in_profile = inside(&profile) && me != profile;
    if !in_profile {
        let system = crate::sys::program("x").parent().and_then(|p| p.parent()).map(norm).unwrap_or_default();
        let drive = system.get(..2).unwrap_or("c:").to_string();
        let env = |n: &str| std::env::var_os(n).map(|v| norm(std::path::Path::new(&v))).unwrap_or_default();
        let blocked = [system, env("ProgramFiles"), env("ProgramFiles(x86)"), env("ProgramW6432"), env("ProgramData"), format!("{drive}\\users")];
        if blocked.iter().any(|b| inside(b)) {
            return None;
        }
    }
    // no link anywhere on the way down to the folder
    let mut at = std::path::PathBuf::new();
    for c in path.components() {
        at.push(c);
        if std::fs::symlink_metadata(&at).is_ok_and(|m| m.file_type().is_symlink()) {
            return None;
        }
    }
    Some(path)
}

/// For the window: {PROFILE} as "Your profile".
pub fn show_folder(stored: &str) -> String {
    stored.replace("{PROFILE}", "Your profile")
}

/// A valid name for a PC: 1–15 letters, digits and hyphens, not only digits.
pub fn valid_pc_name(name: &str) -> bool {
    let n = name.trim();
    n.is_empty() || (n.len() <= 15 && n.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') && !n.chars().all(|c| c.is_ascii_digit()) && !n.starts_with('-'))
}

fn list(v: &Value) -> Vec<String> {
    v.as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default()
}

/// One of the lists of the plan: is `id` in it, put it in or out.
pub fn in_list(l: &[String], id: &str) -> bool {
    l.iter().any(|x| x == id)
}

pub fn set_in_list(l: &mut Vec<String>, id: &str, on: bool) {
    l.retain(|x| x != id);
    if on {
        l.push(id.to_string());
    }
}
