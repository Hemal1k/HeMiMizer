//! The settings HeMiMizer applies: what "Back up this PC" wrote to the folder `config` next to the exe (nothing is
//! built in). {PROFILE} in the files stands for the profile folder.

use crate::i18n::tr;
use crate::sys::{self, Root};
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};

/// The file is in the backup.
pub fn has(name: &str) -> bool {
    dir().join(name).is_file()
}

/// The result of a step whose settings are not in the backup.
pub fn not_backed_up() -> String {
    tr("Skipped – not in the backup", "Přeskočeno – není v záloze").into()
}

pub fn start_bin_dir() -> PathBuf {
    PathBuf::from(sys::env("LOCALAPPDATA", "")).join(r"Packages\Microsoft.Windows.StartMenuExperienceHost_cw5n1h2txyewy\LocalState")
}

pub fn taskbar_dir() -> PathBuf {
    PathBuf::from(sys::env("APPDATA", "")).join(r"Microsoft\Internet Explorer\Quick Launch\User Pinned\TaskBar")
}

pub const TASKBAND: &str = r"HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Explorer\Taskband";

/// The settings of Microsoft Defender (Get-MpPreference) as captured: name=value, lists joined by ';'.
pub const DEFENDER_SETTINGS: &[&str] = &[
    "DisableRealtimeMonitoring", "MAPSReporting", "SubmitSamplesConsent", "PUAProtection", "EnableControlledFolderAccess",
    "EnableNetworkProtection", "CloudBlockLevel", "CloudExtendedTimeout", "DisableBehaviorMonitoring", "DisableIOAVProtection",
    "DisableScriptScanning", "DisableArchiveScanning", "DisableRemovableDriveScanning", "DisableEmailScanning", "ScanScheduleDay",
    "ScanParameters", "SignatureScheduleDay", "RemediationScheduleDay", "ScanAvgCPULoadFactor", "EnableLowCpuPriority",
    "DisableCatchupFullScan", "DisableCatchupQuickScan", "DisableScanningNetworkFiles", "ScanOnlyIfIdleEnabled",
    "DisableCpuThrottleOnIdleScans", "DisableBlockAtFirstSeen", "CheckForSignaturesBeforeRunningScan", "SignatureUpdateInterval",
    "ExclusionPath", "ExclusionProcess", "ExclusionExtension",
];

pub fn defender() -> Vec<(String, String)> {
    text("defender.txt", false)
        .lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .filter_map(|l| l.split_once('=').map(|(k, v)| (k.trim().to_string(), v.trim().to_string())))
        .filter(|(k, _)| !k.is_empty())
        .collect()
}

/// The Steam library folders (for the Defender exclusions).
pub fn steam_libraries() -> Vec<String> {
    text("steam-libraries.txt", false).lines().map(|l| l.split('#').next().unwrap_or("").trim().to_string()).filter(|l| !l.is_empty()).collect()
}

/// `config` next to HeMiMizer.exe.
/// The folder of HeMiMizer.exe.
pub fn exe_dir() -> PathBuf {
    std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)).unwrap_or_default()
}

/// Everything HeMiMizer keeps next to its exe, in one folder (the user, 2026-11-15): config, installers, the log and the
/// to-do file.
pub const HOME: &str = "HeMiMizer data";

/// The folder the backup goes to when the user chose another one than the folder of HeMiMizer.exe (a USB stick, the
/// NAS…); HeMiMizer.exe is copied there too, so the setup runs from it later.
static SAVE_TO: std::sync::Mutex<Option<PathBuf>> = std::sync::Mutex::new(None);

pub fn set_save_to(folder: Option<PathBuf>) {
    *SAVE_TO.lock().unwrap() = folder;
}

/// The folder of the backup chosen (else the folder of HeMiMizer.exe).
pub fn save_to() -> PathBuf {
    SAVE_TO.lock().unwrap().clone().unwrap_or_else(exe_dir)
}

pub fn home() -> PathBuf {
    save_to().join(HOME)
}

pub fn dir() -> PathBuf {
    home().join("config")
}

/// The folders and the log of older versions (config, installers, HeMiMizer.log right next to the exe) go into the folder.
pub fn tidy_up() {
    let (exe, home) = (exe_dir(), home());
    for name in ["config", "installers", "HeMiMizer.log"] {
        let (old, new) = (exe.join(name), home.join(name));
        if old.exists() && !new.exists() {
            let _ = std::fs::create_dir_all(&home);
            let _ = std::fs::rename(&old, &new);
        }
    }
}

pub fn bytes(name: &str) -> Vec<u8> {
    std::fs::read(dir().join(name))
        .ok()
        .unwrap_or_default()
}

/// A text file with {PROFILE} replaced; `escaped` doubles the backslashes (for .ini / .reg).
pub fn text(name: &str, escaped: bool) -> String {
    let t = String::from_utf8_lossy(&bytes(name)).trim_start_matches('\u{feff}').to_string();
    let profile = sys::profile().display().to_string();
    t.replace("{PROFILE}", &if escaped { profile.replace('\\', r"\\") } else { profile })
}

/// A file found by its extension in the config folder (the name varies, e.g. the wallpaper or the VLC skin).
fn find(prefix: &str, ext: &str) -> Option<PathBuf> {
    std::fs::read_dir(dir()).ok()?.flatten().map(|e| e.path()).find(|p| {
        let n = p.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
        n.starts_with(prefix) && n.ends_with(ext)
    })
}

/// The wallpaper: (file name, data); None when the backup has none.
pub fn wallpaper() -> Option<(String, Vec<u8>)> {
    for ext in [".jpg", ".jpeg", ".png", ".bmp"] {
        if let Some(p) = find("wallpaper", ext)
            && let Ok(d) = std::fs::read(&p)
        {
            return Some((format!("wallpaper{ext}"), d));
        }
    }
    has("wallpaper.jpg").then(|| ("wallpaper.jpg".into(), bytes("wallpaper.jpg")))
}

/// The VLC skin: (file name, data); None when the backup has none.
pub fn vlc_skin() -> Option<(String, Vec<u8>)> {
    if let Some(p) = find("", ".vlt")
        && let Ok(d) = std::fs::read(&p)
    {
        let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "skin.vlt".into());
        return Some((name, d));
    }
    has("HeMi-Dark-blue-140.vlt").then(|| ("HeMi-Dark-blue-140.vlt".into(), bytes("HeMi-Dark-blue-140.vlt")))
}

pub struct Drive {
    pub letter: String,
    pub share: String,
    pub label: String,
    pub user: String,
    /// The IPv4 address of the server when captured – used when its name is not found (e.g. no mDNS in a VM).
    pub ip: String,
}

impl Drive {
    pub fn server(&self) -> String {
        self.share.trim_start_matches('\\').split('\\').next().unwrap_or("").to_string()
    }
}

pub fn drives() -> Vec<Drive> {
    text("network-drives.txt", false)
        .lines()
        .map(|l| l.split('#').next().unwrap_or("").trim())
        .filter(|l| !l.is_empty() && !l.starts_with("//"))
        .filter_map(|l| {
            let p: Vec<&str> = l.split('|').map(str::trim).collect();
            (p.len() >= 2 && !p[0].is_empty()).then(|| Drive {
                letter: p[0].trim_end_matches(':').to_uppercase(),
                share: p[1].to_string(),
                label: p.get(2).unwrap_or(&"").to_string(),
                user: p.get(3).unwrap_or(&"").to_string(),
                ip: p.get(4).unwrap_or(&"").to_string(),
            })
        })
        // the share goes into net use and a command line of cmd: a line that is not a plain drive is left out
        .filter(|d| {
            let ok = valid_drive(d);
            if !ok {
                sys::log(&format!("network-drives.txt: left out, not a valid drive: {}|{}|{}", d.letter, d.share, d.ip));
            }
            ok
        })
        .collect()
}

/// A drive of network-drives.txt that is safe to map: one letter, a share `\\server\folder…` with a server name of
/// letters, digits, dots, hyphens and underscores and no character that means something to cmd or PowerShell, and an
/// IPv4 address (or none).
pub fn valid_drive(d: &Drive) -> bool {
    let letter = d.letter.len() == 1 && d.letter.chars().all(|c| c.is_ascii_uppercase());
    let share = d.share.strip_prefix(r"\\").and_then(|r| r.split_once('\\')).is_some_and(|(server, path)| {
        !server.is_empty()
            && server.chars().all(|c| c.is_ascii_alphanumeric() || ".-_".contains(c))
            && !path.trim_matches('\\').is_empty()
            && !path.chars().any(|c| c.is_control() || "\"&|<>^%!".contains(c))
    });
    let ip = d.ip.is_empty() || d.ip.parse::<std::net::Ipv4Addr>().is_ok();
    letter && share && ip
}

pub fn brave_extensions() -> Vec<String> {
    text("brave-extensions.txt", false)
        .lines()
        .map(|l| l.split('#').next().unwrap_or("").trim().to_string())
        .filter(|l| l.len() == 32 && l.chars().all(|c| c.is_ascii_lowercase()))
        .collect()
}

// ---------------------------------------------------------------- capture

fn write(name: &str, data: &[u8]) -> Result<(), String> {
    std::fs::create_dir_all(dir()).map_err(|e| format!("{}: {e}", dir().display()))?;
    std::fs::write(dir().join(name), data).map_err(|e| format!("{name}: {e}"))
}

fn with_profile(t: &str, escaped: bool) -> String {
    let p = sys::profile().display().to_string();
    if escaped { t.replace(&p.replace('\\', r"\\"), "{PROFILE}") } else { t.replace(&p, "{PROFILE}") }
}

/// A part of the settings that can be backed up: (id, name, capture).
pub type Part = (&'static str, &'static str, fn() -> Result<String, String>);

/// What the backup can take from this PC into the config folder.
pub fn parts() -> Vec<Part> {
    vec![
        ("wallpaper", tr("Wallpaper", "Tapeta"), cap_wallpaper),
        ("pins", tr("Pinned in Start and on the taskbar", "Připnuté ve Startu a na hlavním panelu"), cap_pins),
        ("hemidock", "HeMiDock", cap_hemidock),
        ("defender", "Microsoft Defender", cap_defender),
        ("appearance", tr("Colors (dark mode, accent)", "Barvy (tmavý režim, zvýraznění)"), cap_appearance),
        ("explorer", tr("File Explorer view", "Zobrazení Průzkumníka"), cap_explorer),
        ("steam", tr("Steam libraries", "Knihovny Steamu"), cap_steam),
        ("vlc", "VLC", cap_vlc),
        ("qbittorrent", "qBittorrent", cap_qbittorrent),
        ("brave", "Brave", cap_brave),
        ("mouse", "Mouse", cap_mouse),
        ("amd", tr("AMD graphics settings", "Nastavení grafiky AMD"), cap_amd),
        ("scaling", tr("Display scaling", "Měřítko displeje"), cap_scaling),
        ("drives", "Mapped network drives", cap_drives),
        ("wifi", "Wi-Fi networks", cap_wifi),
        ("fonts", "Fonts", cap_fonts),
        ("oc", tr("OC profile", "OC profil"), cap_oc),
    ]
}

// ---------------------------------------------------------------- Wi-Fi

/// This PC has Wi-Fi networks saved (a line "<kind> : <name>" per network; no Wi-Fi service = none).
pub fn has_wifi() -> bool {
    sys::exec("netsh.exe", &["wlan", "show", "profiles"]).is_ok_and(|o| o.code == 0 && o.text.lines().any(|l| l.contains(" : ") && !l.trim_end().ends_with(':')))
}

/// The saved Wi-Fi networks with their passwords (netsh writes one XML per network) – only into a backup protected
/// with a password (the window asks for one).
fn cap_wifi() -> Result<String, String> {
    let to = dir().join("wifi");
    let _ = std::fs::remove_dir_all(&to);
    std::fs::create_dir_all(&to).map_err(|e| e.to_string())?;
    let o = sys::exec("netsh.exe", &["wlan", "export", "profile", "key=clear", &format!("folder={}", to.display())])?;
    let n = std::fs::read_dir(&to).map(|r| r.flatten().filter(|e| e.path().extension().is_some_and(|x| x.eq_ignore_ascii_case("xml"))).count()).unwrap_or(0);
    if n == 0 {
        let _ = std::fs::remove_dir_all(&to);
        return Err(o.text.trim().lines().last().unwrap_or("No Wi-Fi networks on this PC").to_string());
    }
    Ok(crate::i18n::count(n, ("network", "networks"), ("síť", "sítě", "sítí")))
}

// ---------------------------------------------------------------- fonts

const FONTS_KEY: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Fonts";
/// The account that owns the fonts of Windows itself (a font installed later belongs to someone else).
const TRUSTED_INSTALLER: &str = "S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464";

fn owner_sid(path: &Path) -> Option<String> {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Authorization::{ConvertSidToStringSidW, GetNamedSecurityInfoW, SE_FILE_OBJECT};
    use windows_sys::Win32::Security::{OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID};
    let w = sys::wide(&path.display().to_string());
    // SAFETY: `w` ends with a NUL; the out pointers are valid locals; `owner` points into `sd`, which is freed after
    // its last use; the SID string ends with a NUL and is freed with LocalFree
    unsafe {
        let mut owner: PSID = std::ptr::null_mut();
        let mut sd: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
        if GetNamedSecurityInfoW(w.as_ptr(), SE_FILE_OBJECT, OWNER_SECURITY_INFORMATION, &mut owner, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut(), &mut sd) != 0 {
            return None;
        }
        let mut s: *mut u16 = std::ptr::null_mut();
        let r = if ConvertSidToStringSidW(owner, &mut s) != 0 {
            let len = (0..).take_while(|&i| *s.add(i) != 0).count();
            let t = String::from_utf16_lossy(std::slice::from_raw_parts(s, len));
            LocalFree(s.cast());
            Some(t)
        } else {
            None
        };
        LocalFree(sd);
        r
    }
}

/// The fonts installed after Windows: the ones of this user, and the ones in Windows\Fonts that Windows did not bring
/// (their owner is not TrustedInstaller). Copied into config\fonts with fonts.txt: name|file.
fn cap_fonts() -> Result<String, String> {
    let win_fonts = PathBuf::from(sys::env("SystemRoot", r"C:\Windows")).join("Fonts");
    let mut found: Vec<(String, PathBuf)> = Vec::new();
    for (root, machine) in [(Root::Hkcu, false), (Root::Hklm, true)] {
        for name in sys::value_names(root, FONTS_KEY) {
            let Some(v) = sys::get_sz(root, FONTS_KEY, &name) else { continue };
            let path = if Path::new(&v).is_absolute() { PathBuf::from(&v) } else { win_fonts.join(&v) };
            if !path.is_file() || (machine && owner_sid(&path).is_none_or(|s| s == TRUSTED_INSTALLER)) {
                continue;
            }
            found.push((name, path));
        }
    }
    let to = dir().join("fonts");
    let _ = std::fs::remove_dir_all(&to);
    if found.is_empty() {
        return Err("No fonts installed besides those of Windows".into());
    }
    std::fs::create_dir_all(&to).map_err(|e| e.to_string())?;
    let mut lines = Vec::new();
    let mut size = 0;
    for (name, path) in &found {
        let file = path.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
        if std::fs::copy(path, to.join(&file)).is_ok() {
            size += std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
            lines.push(format!("{name}|{file}"));
        }
    }
    std::fs::write(to.join("fonts.txt"), lines.join("\r\n")).map_err(|e| e.to_string())?;
    Ok(format!("{} ({} MB)", crate::i18n::count(lines.len(), ("font", "fonts"), ("písmo", "písma", "písem")), crate::i18n::dec1(size as f64 / 1e6)))
}

/// Takes the current settings of this PC into the config folder; one line per part (✓ or ✗). For `--capture`: no
/// Wi-Fi – its passwords would lie readable next to the exe; they go only into a backup with a password, made in the
/// window (audit 2026-11-24).
pub fn capture() -> Vec<(String, Result<String, String>)> {
    let mut out: Vec<(String, Result<String, String>)> = parts()
        .iter()
        .map(|(id, n, f)| {
            if *id == "wifi" {
                (n.to_string(), Err(tr("not saved – the Wi-Fi passwords go only into a protected backup (in the window)", "neuloženo – hesla Wi-Fi jdou jen do chráněné zálohy (v okně)").to_string()))
            } else {
                (n.to_string(), f())
            }
        })
        .collect();
    out.push((tr("Folder", "Složka").to_string(), Ok(dir().display().to_string())));
    out
}

fn cap_wallpaper() -> Result<String, String> {
    let k = r"Software\Microsoft\Windows\CurrentVersion\Explorer\Wallpapers";
    let src = sys::get_sz(Root::Hkcu, k, "BackgroundHistoryPath0")
        .filter(|p| Path::new(p).exists())
        .or_else(|| sys::get_sz(Root::Hkcu, r"Control Panel\Desktop", "WallPaper"))
        .ok_or_else(|| tr("No wallpaper is set.", "Není nastavená tapeta.").to_string())?;
    let ext = Path::new(&src).extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_else(|| "jpg".into());
    let data = std::fs::read(&src).map_err(|e| format!("{src}: {e}"))?;
    if let Ok(rd) = std::fs::read_dir(dir()) {
        for e in rd.flatten() {
            if e.file_name().to_string_lossy().to_lowercase().starts_with("wallpaper.") {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    write(&format!("wallpaper.{ext}"), &data)?;
    Ok(src)
}

fn cap_vlc() -> Result<String, String> {
    let rc = PathBuf::from(sys::env("APPDATA", "")).join(r"vlc\vlcrc");
    let text = std::fs::read_to_string(&rc).map_err(|e| format!("{}: {e}", rc.display()))?;
    let vlc = crate::programs::vlc_dir();
    let mut out = String::new();
    let mut section = String::new();
    let mut skin = None;
    for line in text.lines() {
        if line.starts_with('[') {
            section = line.to_string();
        } else if !line.starts_with('#') && line.contains('=') {
            if !section.is_empty() {
                out.push_str(&format!("\n{section}\n"));
                section.clear();
            }
            if let Some(p) = line.strip_prefix("skins2-last=") {
                skin = Some(p.to_string());
            }
            out.push_str(&line.replace(&vlc, "{VLC}"));
            out.push('\n');
        }
    }
    // only a skin file (not a folder such as "C:\")
    if let Some((s, name)) = skin.filter(|s| Path::new(s).is_file()).and_then(|s| {
        let name = Path::new(&s).file_name()?.to_string_lossy().into_owned();
        Some((s, name))
    }) {
        if let Ok(rd) = std::fs::read_dir(dir()) {
            for e in rd.flatten() {
                if e.file_name().to_string_lossy().to_lowercase().ends_with(".vlt") {
                    let _ = std::fs::remove_file(e.path());
                }
            }
        }
        write(&name, &std::fs::read(&s).map_err(|e| e.to_string())?)?;
        out = out.replace(&s, &format!(r"{{VLC}}\skins\{name}"));
    }
    write("vlcrc", out.trim_start().as_bytes())?;
    Ok(tr("settings and skin", "nastavení a skin").into())
}

fn cap_qbittorrent() -> Result<String, String> {
    let ini = PathBuf::from(sys::env("APPDATA", "")).join(r"qBittorrent\qBittorrent.ini");
    let t = std::fs::read_to_string(&ini).map_err(|e| format!("{}: {e}", ini.display()))?;
    write("qBittorrent.ini", with_profile(&t, true).as_bytes())?;
    Ok(tr("settings and layout", "nastavení a rozložení").into())
}

fn brave_profile() -> PathBuf {
    PathBuf::from(sys::env("LOCALAPPDATA", "")).join(r"BraveSoftware\Brave-Browser\User Data\Default")
}

/// Only the look and the switches of Brave – no history, accounts, statistics or keys.
fn cap_brave() -> Result<String, String> {
    let read = |f: &str| -> Option<Value> { serde_json::from_str(&std::fs::read_to_string(brave_profile().join(f)).ok()?).ok() };
    let pref = read("Preferences").ok_or_else(|| tr("Brave has no profile here.", "Brave tu nemá profil.").to_string())?;
    let pick = |v: &Value, keys: &[&str]| -> Value {
        let mut m = Map::new();
        for k in keys {
            if let Some(x) = v.get(*k) {
                m.insert(k.to_string(), x.clone());
            }
        }
        Value::Object(m)
    };
    let b = pref.get("brave").cloned().unwrap_or(json!({}));
    let mut brave = pick(
        &b,
        &[
            "ai_chat", "brave_search", "brave_vpn", "enable_media_router_on_restart", "enable_window_closing_confirm", "location_bar_is_wide",
            "pin_share_menu_button", "shields", "show_bookmarks_button", "show_fullscreen_reminder", "show_side_panel_button", "speedreader",
            "tabs", "web_view_rounded_corners",
        ],
    );
    let m = brave.as_object_mut().unwrap();
    m.insert("has_seen_brave_welcome_page".into(), json!(true));
    if let Some(ads) = b.get("brave_ads") {
        m.insert("brave_ads".into(), pick(ads, &["should_allow_ads_subdivision_targeting", "sponsored"]));
    }
    if let Some(ntp) = b.get("new_tab_page").and_then(Value::as_object) {
        let clean: Map<String, Value> = ntp.iter().filter(|(k, _)| !k.starts_with("p3a")).map(|(k, v)| (k.clone(), v.clone())).collect();
        m.insert("new_tab_page".into(), Value::Object(clean));
    }
    for (k, keep) in [("rewards", "show_brave_rewards_button_in_location_bar"), ("today", "should_show_toolbar_button"), ("wallet", "show_wallet_icon_on_toolbar")] {
        if let Some(x) = b.get(k).and_then(|o| o.get(keep)) {
            m.insert(k.into(), json!({ keep: x }));
        }
    }
    if let Some(s) = b.get("sidebar") {
        m.insert("sidebar".into(), pick(s, &["sidebar_items", "hidden_built_in_items"]));
    }
    let mut out = json!({
        "distribution": {
            "skip_first_run_ui": true, "show_welcome_page": false, "import_bookmarks": false, "import_history": false,
            "import_home_page": false, "import_search_engine": false, "make_chrome_default": false,
            "make_chrome_default_for_user": false, "suppress_first_run_default_browser_prompt": true
        },
        "background_mode": { "enabled": false },
        "brave": brave,
    });
    if let Some(x) = pref.get("bookmark_bar") {
        out["bookmark_bar"] = x.clone();
    }
    if let Some(x) = pref.pointer("/browser/pin_pwa_install_button") {
        out["browser"] = json!({ "pin_pwa_install_button": x });
    }
    if let Some(x) = pref.pointer("/ntp/shortcust_visible") {
        out["ntp"] = json!({ "shortcust_visible": x });
    }
    write("brave-initial_preferences.json", serde_json::to_string_pretty(&out).unwrap_or_default().as_bytes())?;

    // the extensions from the Chrome Web Store (Secure Preferences holds them in newer versions)
    let mut lines = vec!["# Chrome Web Store IDs of the extensions Brave installs".to_string()];
    for f in ["Secure Preferences", "Preferences"] {
        if let Some(s) = read(f).and_then(|v| v.pointer("/extensions/settings").cloned()).and_then(|s| s.as_object().cloned()) {
            for (id, e) in s {
                let store = e.get("from_webstore").and_then(Value::as_bool) == Some(true) && e.get("location").and_then(Value::as_i64) == Some(1);
                if store && !lines.iter().any(|l| l.starts_with(&id)) {
                    let name = e.pointer("/manifest/name").and_then(Value::as_str).unwrap_or("");
                    lines.push(format!("{id} # {name}"));
                }
            }
        }
    }
    let n = lines.len() - 1;
    write("brave-extensions.txt", (lines.join("\r\n") + "\r\n").as_bytes())?;
    Ok(format!("{} {n}", tr("look and settings, extensions:", "vzhled a nastavení, rozšíření:")))
}

/// The pointer speed (1–20), the lines and characters a wheel notch scrolls, the double-click time (ms) – as
/// mouse.txt (name=value).
fn cap_mouse() -> Result<String, String> {
    let m = crate::windows::Mouse::now();
    write("mouse.txt", m.to_text().as_bytes())?;
    Ok(format!("speed {} of 20, {} lines per notch", m.speed, m.lines))
}

/// `reg export` of a key as text.
fn reg_export(key: &str) -> Result<String, String> {
    let f = sys::scripts_dir()?.join(format!("export-{}.reg", std::process::id()));
    sys::run("reg.exe", &["export", key, &f.display().to_string(), "/y"])?;
    let raw = std::fs::read(&f).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(&f);
    let u: Vec<u16> = raw.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
    Ok(String::from_utf16_lossy(&u).trim_start_matches('\u{feff}').to_string())
}

/// The values (whole logical lines) right under [key] whose names pass `keep`.
fn reg_values(text: &str, key: &str, keep: impl Fn(&str) -> bool) -> Vec<String> {
    let mut out = Vec::new();
    let mut inside = false;
    let mut buf = String::new();
    for line in text.lines() {
        if line.starts_with('[') {
            inside = line.trim() == format!("[{key}]");
            continue;
        }
        if !inside {
            continue;
        }
        buf.push_str(line);
        if line.ends_with('\\') {
            buf.push_str("\r\n");
            continue;
        }
        if let Some(name) = buf.strip_prefix('"').and_then(|r| r.split('"').next())
            && keep(name)
        {
            out.push(buf.clone());
        }
        buf.clear();
    }
    out
}

fn cap_amd() -> Result<String, String> {
    let key = crate::drivers::gpu_key().ok_or_else(|| tr("No AMD Radeon driver here.", "Není tu ovladač AMD Radeon.").to_string())?;
    let full = format!(r"HKEY_LOCAL_MACHINE\{key}\UMD");
    let umd = reg_values(&reg_export(&full)?, &full, |n| {
        !(n.ends_with("_DEF") || n.ends_with("_SET") || n.ends_with("_NA") || n.ends_with("_TBL"))
            && !["FsrOvrDLLPath", "AppGpuId", "DisableGeometricScaling", "PowerState"].contains(&n)
    });
    let mut t = format!("Windows Registry Editor Version 5.00\r\n\r\n[{{GPU}}\\UMD]\r\n{}\r\n", umd.join("\r\n"));
    write("amd-umd.reg", t.as_bytes())?;

    const CN: &[&str] = &[
        "WizardProfile", "LastSavedExpID", "RSXColorScheme", "SystemTray", "AllowWebContent", "AnimationEffect", "LanguageOverride",
        "GameConfigNotificationEnabled", "EulaAccepted", "FeedbackNotification", "CN_Hide_FeatureData", "UserTypeWizardShown",
        "ResumeLastPage", "ChillHk", "DelagHk", "BoostHk", "DelagBoostIndicatorHk",
    ];
    const DVR: &[&str] = &[
        "DvrEnabled", "FileEncoderType", "HotkeysDisabled", "ShowRSOverlay", "CaptureFileOutput", "PrevInstantReplayEnable",
        "PrevInGameReplayEnabled", "PrevInstantGifEnabled",
    ];
    t = "Windows Registry Editor Version 5.00\r\n".into();
    for (key, keep) in [
        (r"HKEY_CURRENT_USER\Software\AMD\CN", Some(CN)),
        (r"HKEY_CURRENT_USER\Software\AMD\CN\R3DBk", None),
        (r"HKEY_CURRENT_USER\Software\AMD\DVR", Some(DVR)),
    ] {
        let Ok(text) = reg_export(key) else { continue };
        let v = reg_values(&text, key, |n| keep.is_none_or(|k| k.contains(&n)));
        t.push_str(&format!("\r\n[{key}]\r\n{}\r\n", v.join("\r\n")));
    }
    write("amd-user.reg", with_profile(&t, true).as_bytes())?;
    Ok(format!("{} {}", umd.len(), tr("3D settings + Adrenalin", "3D nastavení + Adrenalin")))
}

fn cap_scaling() -> Result<String, String> {
    let t = reg_export(r"HKEY_CURRENT_USER\Control Panel\Desktop\PerMonitorSettings")?;
    write("display-scaling.reg", t.as_bytes())?;
    Ok(String::new())
}

/// The network drives of this user now: (letter, share), e.g. ("H", "\\nas\home").
pub fn mapped_drives() -> Vec<(String, String)> {
    let Ok(o) = sys::exec("reg.exe", &["query", r"HKCU\Network", "/s"]) else { return Vec::new() };
    let mut v = Vec::new();
    let mut letter = String::new();
    for l in o.text.lines() {
        if let Some(k) = l.trim().strip_prefix(r"HKEY_CURRENT_USER\Network\") {
            letter = k.to_string();
        } else if l.trim().starts_with("RemotePath") {
            v.push((letter.clone(), l.split("REG_SZ").nth(1).unwrap_or("").trim().to_string()));
        }
    }
    v
}

fn cap_drives() -> Result<String, String> {
    let o = sys::exec("reg.exe", &["query", r"HKCU\Network", "/s"])?;
    let mut lines = vec!["# letter|share|name in File Explorer|user|IP of the server (used when the name is not found)".to_string()];
    let mut letter = String::new();
    for l in o.text.lines() {
        if let Some(k) = l.trim().strip_prefix(r"HKEY_CURRENT_USER\Network\") {
            letter = k.to_string();
        } else if l.trim().starts_with("RemotePath") {
            let share = l.split("REG_SZ").nth(1).unwrap_or("").trim().to_string();
            let mp = format!(r"Software\Microsoft\Windows\CurrentVersion\Explorer\MountPoints2\{}", share.replace('\\', "#"));
            let label = sys::get_sz(Root::Hkcu, &mp, "_LabelFromReg").unwrap_or_default();
            let server = share.trim_start_matches('\\').split('\\').next().unwrap_or("").to_string();
            let user = crate::programs::saved_user(&server).unwrap_or_default();
            let ip = std::net::ToSocketAddrs::to_socket_addrs(&(server.as_str(), 445))
                .ok()
                .and_then(|mut a| a.find(|x| x.is_ipv4()))
                .map(|a| a.ip().to_string())
                .unwrap_or_default();
            lines.push(format!("{letter}|{share}|{label}|{user}|{ip}"));
        }
    }
    let n = lines.len() - 1;
    write("network-drives.txt", (lines.join("\r\n") + "\r\n").as_bytes())?;
    Ok(format!("{n}"))
}

const PERSONALIZE: &str = r"HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize";
const DWM: &str = r"HKEY_CURRENT_USER\Software\Microsoft\Windows\DWM";
const ACCENT: &str = r"HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Explorer\Accent";
const ADVANCED: &str = r"HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced";

/// Values of several keys as one .reg text.
fn reg_pick(keys: &[(&str, &[&str])]) -> Result<(String, usize), String> {
    let mut t = String::from("Windows Registry Editor Version 5.00\r\n");
    let mut n = 0;
    for (key, names) in keys {
        let v = reg_values(&reg_export(key)?, key, |x| names.contains(&x));
        n += v.len();
        t.push_str(&format!("\r\n[{key}]\r\n{}\r\n", v.join("\r\n")));
    }
    Ok((t, n))
}

pub const APPEARANCE_VALUES: &[(&str, &[&str])] = &[
    (PERSONALIZE, &["AppsUseLightTheme", "SystemUsesLightTheme", "EnableTransparency", "ColorPrevalence"]),
    (DWM, &["AccentColor", "ColorPrevalence", "ColorizationColor", "ColorizationAfterglow", "AccentColorInactive"]),
    (ACCENT, &["AccentPalette", "AccentColorMenu", "StartColorMenu"]),
];

pub const EXPLORER_VALUES: &[(&str, &[&str])] = &[(
    ADVANCED,
    &[
        "HideFileExt", "Hidden", "ShowSuperHidden", "ShowStatusBar", "ShowInfoTip", "ShowTypeOverlay", "IconsOnly", "AutoCheckSelect",
        "SeparateProcess", "ShowCompColor", "HideDrivesWithNoMedia", "NavPaneShowAllFolders", "NavPaneExpandToCurrentFolder",
        "HideMergeConflicts", "PersistBrowsers", "DontPrettyPath", "ShowEncryptCompressedColor", "SharingWizardOn", "ListviewAlphaSelect",
        "ListviewShadow", "WebView", "Filter", "MapNetDrvBtn",
    ],
)];

fn cap_appearance() -> Result<String, String> {
    let (t, n) = reg_pick(APPEARANCE_VALUES)?;
    write("appearance.reg", t.as_bytes())?;
    Ok(format!("{n}"))
}

fn cap_explorer() -> Result<String, String> {
    let (t, n) = reg_pick(EXPLORER_VALUES)?;
    write("explorer-view.reg", t.as_bytes())?;
    Ok(format!("{n}"))
}

fn cap_steam() -> Result<String, String> {
    let steam = sys::get_sz(Root::Hkcu, r"Software\Valve\Steam", "SteamPath").unwrap_or_else(|| r"C:\Program Files (x86)\Steam".into());
    let vdf = PathBuf::from(steam.replace('/', "\\")).join(r"steamapps\libraryfolders.vdf");
    let t = std::fs::read_to_string(&vdf).map_err(|e| format!("{}: {e}", vdf.display()))?;
    let mut lines = vec!["# Steam library folders (Defender skips them)".to_string()];
    for l in t.lines() {
        let l = l.trim();
        if let Some(rest) = l.strip_prefix("\"path\"") {
            lines.push(rest.trim().trim_matches('"').replace(r"\\", r"\"));
        }
    }
    let n = lines.len() - 1;
    write("steam-libraries.txt", (lines.join("\r\n") + "\r\n").as_bytes())?;
    Ok(format!("{n}"))
}

/// Get-MpPreference (the exclusions only when HeMiMizer runs as administrator – the release always does),
/// and whether memory integrity (Core isolation) is on.
fn cap_defender() -> Result<String, String> {
    let names = DEFENDER_SETTINGS.join(",");
    let out = sys::ps(&format!(
        "$m = Get-MpPreference; foreach ($n in '{names}'.Split(',')) {{ $v = $m.$n; \
         if ($v -is [array]) {{ $v = ($v | Where-Object {{ $_ -notlike 'N/A*' }}) -join ';' }} \
         elseif ($v -like 'N/A*') {{ continue }}; \"$n=$v\" }}; \
         $dg = Get-CimInstance -Namespace root\\Microsoft\\Windows\\DeviceGuard -ClassName Win32_DeviceGuard -ErrorAction SilentlyContinue; \
         \"MemoryIntegrity=$([int]($dg.SecurityServicesConfigured -contains 2))\""
    ))?;
    let mut lines = vec!["# Microsoft Defender as on the PC this backup was made on (Get-MpPreference); MemoryIntegrity = Core isolation".to_string()];
    lines.extend(out.lines().map(|l| l.trim().to_string()).filter(|l| l.contains('=')));
    let n = lines.len() - 1;
    write("defender.txt", (lines.join("\r\n") + "\r\n").as_bytes())?;
    let admin = out.lines().any(|l| l.starts_with("ExclusionPath=") && l.len() > "ExclusionPath=".len());
    Ok(format!("{n}{}", if admin { "" } else { tr(" (exclusions only when run as administrator)", " (výjimky jen při spuštění jako správce)") }))
}

/// The settings of HeMiDock (%AppData%\HeMiDock: settings, pins, Start pins, tray, workspaces + the folder Pinned)
/// into one hemidock.tar; the profile folder in them becomes {PROFILE}.
fn cap_hemidock() -> Result<String, String> {
    // the folder of HeMiDock 1.7 and newer, else the one of the older versions
    let roaming = PathBuf::from(sys::env("APPDATA", ""));
    let data = [roaming.join("HeMi").join("HeMiDock"), roaming.join("HeMiDock")].into_iter().find(|d| d.is_dir()).unwrap_or_else(|| roaming.join("HeMi").join("HeMiDock"));
    if !data.join("settings.json").exists() {
        return Err(tr("HeMiDock has no settings here.", "HeMiDock tu nemá nastavení.").into());
    }
    let stage = std::env::temp_dir().join(format!("HeMiMizer-hemidock-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&stage);
    std::fs::create_dir_all(stage.join("Pinned")).map_err(|e| e.to_string())?;
    let mut n = 0;
    for f in crate::programs::HEMIDOCK_FILES {
        if let Ok(t) = std::fs::read_to_string(data.join(f)) {
            std::fs::write(stage.join(f), with_profile(&t, true)).map_err(|e| e.to_string())?;
            n += 1;
        }
    }
    let mut pins = 0;
    if let Ok(rd) = std::fs::read_dir(data.join("Pinned")) {
        for e in rd.flatten() {
            if std::fs::copy(e.path(), stage.join("Pinned").join(e.file_name())).is_ok() {
                pins += 1;
            }
        }
    }
    std::fs::create_dir_all(dir()).map_err(|e| e.to_string())?;
    let tar = dir().join("hemidock.tar");
    let r = sys::run("tar.exe", &["-cf", &tar.display().to_string(), "-C", &stage.display().to_string(), "."]);
    let _ = std::fs::remove_dir_all(&stage);
    r?;
    Ok(format!(
        "{}, {}",
        crate::i18n::count(n, ("file", "files"), ("soubor", "soubory", "souborů")),
        crate::i18n::count(pins, ("pinned shortcut", "pinned shortcuts"), ("připnutý zástupce", "připnutí zástupci", "připnutých zástupců"))
    ))
}

/// The pins of Start (start2.bin) and of the taskbar (its shortcuts + the order in Taskband) into pins.tar.
fn cap_pins() -> Result<String, String> {
    let stage = std::env::temp_dir().join(format!("HeMiMizer-pins-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&stage);
    std::fs::create_dir_all(stage.join("taskbar")).map_err(|e| e.to_string())?;
    std::fs::copy(start_bin_dir().join("start2.bin"), stage.join("start2.bin")).map_err(|e| format!("start2.bin: {e}"))?;
    let mut n = 0;
    if let Ok(rd) = std::fs::read_dir(taskbar_dir()) {
        for e in rd.flatten() {
            if e.path().extension().is_some_and(|x| x.eq_ignore_ascii_case("lnk")) && std::fs::copy(e.path(), stage.join("taskbar").join(e.file_name())).is_ok() {
                n += 1;
            }
        }
    }
    let reg = reg_values(&reg_export(TASKBAND)?, TASKBAND, |v| v.starts_with("Favorites"));
    std::fs::write(stage.join("taskband.reg"), format!("Windows Registry Editor Version 5.00\r\n\r\n[{TASKBAND}]\r\n{}\r\n", reg.join("\r\n")))
        .map_err(|e| e.to_string())?;
    std::fs::create_dir_all(dir()).map_err(|e| e.to_string())?;
    let r = sys::run("tar.exe", &["-cf", &dir().join("pins.tar").display().to_string(), "-C", &stage.display().to_string(), "."]);
    let _ = std::fs::remove_dir_all(&stage);
    r?;
    Ok(format!("Start + {n} {}", tr("on the taskbar", "na hlavním panelu")))
}

fn cap_oc() -> Result<String, String> {
    if dir().join("OC.xml").exists() {
        Ok(tr("OC.xml in the folder is used", "použije se OC.xml ze složky").into())
    } else {
        Err(tr(
            "Adrenalin cannot be read from outside – export the profile in Adrenalin (Performance → Tuning → Export) and choose the file with the button on the right.",
            "Adrenalin nejde přečíst zvenku – vyexportuj profil v Adrenalinu (Výkon → Ladění → Exportovat) a vyber soubor tlačítkem vpravo.",
        )
        .into())
    }
}
