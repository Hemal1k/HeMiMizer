//! The programs: always the newest version straight from its maker, installed silently (skipped when the newest
//! is already installed), then set up as on the PC of the author (components, shortcuts, settings).

use crate::config;
use crate::i18n::tr;
use crate::steps::{Ctx, Res};
use crate::sys::{self, Root};
use std::path::{Path, PathBuf};

fn program_files() -> String {
    sys::env("ProgramFiles", r"C:\Program Files")
}

pub fn public_desktop() -> PathBuf {
    PathBuf::from(sys::env("PUBLIC", r"C:\Users\Public")).join("Desktop")
}

pub fn start_menu() -> PathBuf {
    PathBuf::from(sys::env("ProgramData", r"C:\ProgramData")).join(r"Microsoft\Windows\Start Menu\Programs")
}


/// The version of an installed program (Apps → Installed apps) whose name starts with `name`.
pub fn installed_version(name: &str) -> Option<String> {
    let script = format!(
        "Get-ItemProperty 'HKLM:\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\*','HKLM:\\Software\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\*','HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\*' -ErrorAction SilentlyContinue | \
         Where-Object {{ $_.DisplayName -like '{name}*' }} | Select-Object -First 1 -ExpandProperty DisplayVersion"
    );
    sys::ps(&script).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

// ---------------------------------------------------------------- the newest versions

fn not_found(what: &str) -> String {
    format!("{what}: {}", tr("the download link was not found on the page of the maker", "odkaz ke stažení nebyl na stránce výrobce nalezen"))
}

pub fn url_7zip() -> Result<String, String> {
    let page = "https://www.7-zip.org/download.html";
    let html = sys::fetch(page, None)?;
    let l = sys::links(&html, "7z", "-x64.exe").into_iter().next().ok_or_else(|| not_found("7-Zip"))?;
    Ok(sys::join_url(page, &l))
}

/// download.videolan.org redirects to a mirror over HTTP; get.videolan.org answers with a web page instead.
pub fn url_vlc() -> Result<String, String> {
    let base = "https://download.videolan.org/pub/videolan/vlc/last/win64/";
    let html = sys::fetch(base, None)?;
    let l = sys::links(&html, "vlc-", "-win64.exe").into_iter().next().ok_or_else(|| not_found("VLC"))?;
    Ok(sys::join_url(base, &l))
}

pub fn url_brave() -> Result<String, String> {
    Ok("https://github.com/brave/brave-browser/releases/latest/download/BraveBrowserStandaloneSilentSetup.exe".into())
}

pub fn url_steam() -> Result<String, String> {
    Ok("https://cdn.akamai.steamstatic.com/client/installer/SteamSetup.exe".into())
}

/// The page of qBittorrent links to SourceForge; its download page holds the direct link with a token.
pub fn url_qbittorrent() -> Result<String, String> {
    let html = sys::fetch("https://www.qbittorrent.org/download", None)?;
    let page = sys::links(&html, "qbittorrent_", "_x64_setup.exe/download")
        .into_iter()
        .find(|l| !l.contains("_lt"))
        .ok_or_else(|| not_found("qBittorrent"))?;
    let sf = sys::fetch(&page, None)?;
    let start = sf.find("https://downloads.sourceforge.net/").ok_or_else(|| not_found("qBittorrent (SourceForge)"))?;
    let end = sf[start..].find(['"', '\'', '<', ' ']).map(|e| start + e).unwrap_or(sf.len());
    Ok(sf[start..end].replace("&amp;", "&"))
}

// ---------------------------------------------------------------- link + version of each program (used by the
// download step at the start and, when something was not downloaded then, by the install steps)


/// The version the GitHub link of Brave leads to ("1.99.55").
pub fn brave_version() -> Option<String> {
    let o = sys::exec("curl.exe", &["-sI", "-A", sys::USER_AGENT, &url_brave().ok()?]).ok()?;
    let line = o.text.lines().find(|l| l.to_lowercase().starts_with("location:"))?;
    let i = line.find("/download/v")? + "/download/v".len();
    Some(line[i..].split('/').next()?.to_string())
}

// ---------------------------------------------------------------- 7-Zip

const ARCHIVES: &[(&str, u32)] = &[
    ("7z", 0), ("zip", 1), ("bz2", 2), ("rar", 3), ("arj", 4), ("lzh", 6), ("cab", 7), ("tar", 13), ("gz", 14), ("tgz", 14), ("wim", 15),
    ("xz", 23), ("txz", 23),
];

pub fn seven_zip(c: &Ctx) -> Res {
    if !crate::install::chosen("7zip") {
        return Ok(crate::install::not_chosen());
    }
    crate::install::ensure(c, "7zip")?;
    let version = installed_version("7-Zip").unwrap_or_default();
    let dir = format!(r"{}\7-Zip", program_files());
    if !Path::new(&format!(r"{dir}\7zFM.exe")).exists() {
        return Err(tr("7-Zip is not in Program Files after the installation.", "7-Zip po instalaci není v Program Files.").into());
    }
    // the file types: the classes 7-Zip itself writes, plus "Capabilities" so that it is listed in Default apps
    let caps = r"Software\7-Zip\Capabilities";
    sys::set_sz(Root::Hklm, caps, "ApplicationName", "7-Zip")?;
    sys::set_sz(Root::Hklm, caps, "ApplicationDescription", "7-Zip File Manager")?;
    for (ext, icon) in ARCHIVES {
        let prog = format!("7-Zip.{ext}");
        let cls = format!(r"Software\Classes\{prog}");
        sys::set_sz(Root::Hklm, &cls, "", &format!("{ext} Archive"))?;
        sys::set_sz(Root::Hklm, &format!(r"{cls}\DefaultIcon"), "", &format!(r"{dir}\7z.dll,{icon}"))?;
        sys::set_sz(Root::Hklm, &format!(r"{cls}\shell"), "", "")?;
        sys::set_sz(Root::Hklm, &format!(r"{cls}\shell\open\command"), "", &format!(r#""{dir}\7zFM.exe" "%1""#))?;
        let e = format!(r"Software\Classes\.{ext}");
        sys::set_sz(Root::Hklm, &e, "", &prog)?;
        sys::set_sz(Root::Hklm, &format!(r"{e}\OpenWithProgids"), &prog, "")?;
        sys::set_sz(Root::Hklm, &format!(r"{caps}\FileAssociations"), &format!(".{ext}"), &prog)?;
    }
    sys::set_sz(Root::Hklm, r"Software\RegisteredApplications", "7-Zip", caps)?;
    sys::assoc_changed();
    Ok(format!("7-Zip {version}"))
}

// ---------------------------------------------------------------- VLC

pub fn vlc_dir() -> String {
    sys::get_sz(Root::Hklm, r"Software\VideoLAN\VLC", "InstallDir").unwrap_or_else(|| format!(r"{}\VideoLAN\VLC", program_files()))
}

pub fn vlc(c: &Ctx) -> Res {
    if !crate::install::chosen("vlc") {
        return Ok(crate::install::not_chosen());
    }
    if crate::install::ensure(c, "vlc")? {
        let dir = vlc_dir();
        c.log(tr("Removing the parts of VLC that are not wanted", "Odebírám nechtěné součásti VLC"));
        // no web plug-ins, no disc playback (AutoPlay), no context menus; the file types of audio and video stay (the
        // shortcuts and the start with Windows follow the choice of the wizard: the step "shortcuts")
        let ax = format!(r"{dir}\axvlc.dll");
        if Path::new(&ax).exists() {
            let _ = sys::run("regsvr32.exe", &["/u", "/s", &ax]);
            sys::remove_file(&ax);
        }
        sys::remove_file(format!(r"{dir}\npvlc.dll"));
        sys::delete_tree(Root::Hklm, r"Software\MozillaPlugins\@videolan.org/vlc,version=3.0.0");
        sys::ps(
            r#"
$c='HKLM:\SOFTWARE\Classes'
foreach($k in 'Directory\shell\AddToPlaylistVLC','Directory\shell\PlayWithVLC'){ Remove-Item "$c\$k" -Recurse -Force -ErrorAction SilentlyContinue }
Get-ChildItem $c -ErrorAction SilentlyContinue | Where-Object { $_.PSChildName -like 'VLC.*' } | ForEach-Object {
  foreach($v in 'AddToPlaylistVLC','PlayWithVLC','Enqueue'){ Remove-Item "$($_.PSPath)\shell\$v" -Recurse -Force -ErrorAction SilentlyContinue } }
$a='HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\AutoplayHandlers'
Get-ChildItem "$a\Handlers" -ErrorAction SilentlyContinue | Where-Object { $_.PSChildName -like 'VLC*' } | Remove-Item -Recurse -Force -ErrorAction SilentlyContinue
Get-ChildItem "$a\EventHandlers" -ErrorAction SilentlyContinue | ForEach-Object {
  $k=$_; ($k.GetValueNames() | Where-Object { $_ -like 'VLC*' }) | ForEach-Object { Remove-ItemProperty -Path $k.PSPath -Name $_ -Force -ErrorAction SilentlyContinue } }
"#,
        )?;
        sys::assoc_changed();
    }
    vlc_settings(c)?;
    Ok(format!("VLC {}", installed_version("VLC media player").unwrap_or_default()))
}

/// The skin and the settings of VLC (the skin interface, no tray icon, no update / privacy questions).
fn vlc_settings(c: &Ctx) -> Result<(), String> {
    c.log(tr("Setting up VLC (skin)", "Nastavuji VLC (skin)"));
    let dir = vlc_dir();
    let skins = PathBuf::from(&dir).join("skins");
    if let Some((name, data)) = config::vlc_skin() {
        std::fs::create_dir_all(&skins).map_err(|e| e.to_string())?;
        std::fs::write(skins.join(name), data).map_err(|e| format!("{}: {e}", skins.display()))?;
    }
    if !config::has("vlcrc") {
        return Ok(());
    }
    let cfg = PathBuf::from(sys::env("APPDATA", "")).join("vlc");
    std::fs::create_dir_all(&cfg).map_err(|e| e.to_string())?;
    let rc = cfg.join("vlcrc");
    if rc.exists() {
        let _ = std::fs::rename(&rc, cfg.join("vlcrc.bak"));
    }
    std::fs::write(&rc, config::text("vlcrc", false).replace("{VLC}", &dir)).map_err(|e| format!("{}: {e}", rc.display()))
}

// ---------------------------------------------------------------- Brave

/// Only the ad blocking (Shields) stays: Leo AI, Rewards (BAT), Wallet (crypto), VPN, Tor, News, Talk, Playlist,
/// Wayback Machine, shopping, surveys and the telemetry are off.
const BRAVE_POLICIES: &[(&str, u32)] = &[
    ("BraveAIChatEnabled", 0),
    ("BraveRewardsDisabled", 1),
    ("BraveWalletDisabled", 1),
    ("BraveVPNDisabled", 1),
    ("BraveNewsDisabled", 1),
    ("BraveTalkDisabled", 1),
    ("BravePlaylistEnabled", 0),
    ("BraveStatsPingEnabled", 0),
    ("BraveP3AEnabled", 0),
    ("BraveWebDiscoveryEnabled", 0),
    ("MetricsReportingEnabled", 0),
    ("BackgroundModeEnabled", 0),
    ("PromotionalTabsEnabled", 0),
    ("SearchSuggestEnabled", 0),
    ("TorDisabled", 1),
    ("BraveWaybackMachineEnabled", 0),
    ("ShoppingListEnabled", 0),
    ("FeedbackSurveysEnabled", 0),
    ("UserFeedbackAllowed", 0),
    ("DefaultBrowserSettingEnabled", 0),
    ("SpellCheckServiceEnabled", 0),
    ("UrlKeyedAnonymizedDataCollectionEnabled", 0),
];
const BRAVE_POLICY_KEY: &str = r"Software\Policies\BraveSoftware\Brave";

pub fn brave(c: &Ctx) -> Res {
    if !crate::install::chosen("brave") {
        return Ok(crate::install::not_chosen());
    }
    crate::install::ensure(c, "brave")?;
    let version = installed_version("Brave").unwrap_or_default();
    c.log(tr("Setting up Brave", "Nastavuji Brave"));
    sys::dwords(Root::Hklm, BRAVE_POLICY_KEY, BRAVE_POLICIES)?;
    // the extensions: installed by Brave itself from the Chrome Web Store
    let ext = config::brave_extensions();
    if !ext.is_empty() {
        let entries: Vec<String> = ext
            .iter()
            .map(|id| format!(r#""{id}":{{"installation_mode":"normal_installed","update_url":"https://clients2.google.com/service/update2/crx"}}"#))
            .collect();
        sys::set_sz(Root::Hklm, BRAVE_POLICY_KEY, "ExtensionSettings", &format!("{{{}}}", entries.join(",")))?;
    }
    let app = PathBuf::from(program_files()).join(r"BraveSoftware\Brave-Browser\Application");
    if app.exists() && config::has("brave-initial_preferences.json") {
        std::fs::write(app.join("initial_preferences"), config::bytes("brave-initial_preferences.json")).map_err(|e| format!("{}: {e}", app.display()))?;
    }
    let profile = PathBuf::from(sys::env("LOCALAPPDATA", "")).join(r"BraveSoftware\Brave-Browser\User Data\Default\Preferences");
    if profile.exists() {
        c.note(tr(
            "Brave already had a profile – your look applies only to a new profile; the policies and extensions apply now.",
            "Brave už měl profil – tvůj vzhled se použije jen pro nový profil, zásady a rozšíření platí hned.",
        ));
    }
    Ok(format!("Brave {version}, {} {}", ext.len(), tr("extensions", "rozšíření")))
}

// ---------------------------------------------------------------- Steam

pub fn steam(c: &Ctx) -> Res {
    if !crate::install::chosen("steam") {
        return Ok(crate::install::not_chosen());
    }
    // Steam updates itself – it is only installed when missing
    if installed_version("Steam").is_some() {
        return Ok(tr("Already installed (updates itself)", "Už je nainstalovaný (aktualizuje se sám)").into());
    }
    crate::install::ensure(c, "steam")?;
    Ok("Steam".into())
}

// ---------------------------------------------------------------- qBittorrent

pub fn qbittorrent(c: &Ctx) -> Res {
    if !crate::install::chosen("qbittorrent") {
        return Ok(crate::install::not_chosen());
    }
    crate::install::ensure(c, "qbittorrent")?;
    let version = installed_version("qBittorrent").unwrap_or_default();
    let exe = format!(r"{}\qBittorrent\qbittorrent.exe", program_files());
    // a firewall rule, long paths (the shortcuts and the start with Windows follow the choice of the wizard)
    sys::set_dword(Root::Hklm, r"SYSTEM\CurrentControlSet\Control\FileSystem", "LongPathsEnabled", 1)?;
    let has_rule = sys::exec("netsh.exe", &["advfirewall", "firewall", "show", "rule", "name=qBittorrent"]).map(|o| o.code == 0).unwrap_or(false);
    if !has_rule {
        sys::run("netsh.exe", &["advfirewall", "firewall", "add", "rule", "name=qBittorrent", "dir=in", "action=allow", &format!("program={exe}"), "enable=yes"])?;
    }
    c.log(tr("Setting up qBittorrent", "Nastavuji qBittorrent"));
    if sys::is_running("qbittorrent.exe") {
        return Err(tr("Close qBittorrent first – it would overwrite the settings.", "Nejdřív zavři qBittorrent – přepsal by nastavení.").into());
    }
    if config::has("qBittorrent.ini") {
        let dir = PathBuf::from(sys::env("APPDATA", "")).join("qBittorrent");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        std::fs::write(dir.join("qBittorrent.ini"), config::text("qBittorrent.ini", true)).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    Ok(format!("qBittorrent {version}"))
}

// ---------------------------------------------------------------- runtimes for games and apps

struct Redist {
    name: &'static str,
    installed: &'static str,
    url: &'static str,
    args: &'static [&'static str],
}

const VC: &[Redist] = &[
    Redist { name: "Visual C++ 2005 x64", installed: "Microsoft Visual C++ 2005 Redistributable (x64)", url: "https://download.microsoft.com/download/8/B/4/8B42259F-5D70-43F4-AC2E-4B208FD8D66A/vcredist_x64.EXE", args: &["/q"] },
    Redist { name: "Visual C++ 2005 x86", installed: "Microsoft Visual C++ 2005 Redistributable", url: "https://download.microsoft.com/download/8/B/4/8B42259F-5D70-43F4-AC2E-4B208FD8D66A/vcredist_x86.EXE", args: &["/q"] },
    Redist { name: "Visual C++ 2008 x64", installed: "Microsoft Visual C++ 2008 Redistributable - x64", url: "https://download.microsoft.com/download/5/D/8/5D8C65CB-C849-4025-8E95-C3966CAFD8AE/vcredist_x64.exe", args: &["/q"] },
    Redist { name: "Visual C++ 2008 x86", installed: "Microsoft Visual C++ 2008 Redistributable - x86", url: "https://download.microsoft.com/download/5/D/8/5D8C65CB-C849-4025-8E95-C3966CAFD8AE/vcredist_x86.exe", args: &["/q"] },
    Redist { name: "Visual C++ 2010 x64", installed: "Microsoft Visual C++ 2010  x64 Redistributable", url: "https://download.microsoft.com/download/1/6/5/165255E7-1014-4D0A-B094-B6A430A6BFFC/vcredist_x64.exe", args: &["/q", "/norestart"] },
    Redist { name: "Visual C++ 2010 x86", installed: "Microsoft Visual C++ 2010  x86 Redistributable", url: "https://download.microsoft.com/download/1/6/5/165255E7-1014-4D0A-B094-B6A430A6BFFC/vcredist_x86.exe", args: &["/q", "/norestart"] },
    Redist { name: "Visual C++ 2012 x64", installed: "Microsoft Visual C++ 2012 Redistributable (x64)", url: "https://download.microsoft.com/download/1/6/B/16B06F60-3B20-4FF2-B699-5E9B7962F9AE/VSU_4/vcredist_x64.exe", args: &["/install", "/quiet", "/norestart"] },
    Redist { name: "Visual C++ 2012 x86", installed: "Microsoft Visual C++ 2012 Redistributable (x86)", url: "https://download.microsoft.com/download/1/6/B/16B06F60-3B20-4FF2-B699-5E9B7962F9AE/VSU_4/vcredist_x86.exe", args: &["/install", "/quiet", "/norestart"] },
    Redist { name: "Visual C++ 2013 x64", installed: "Microsoft Visual C++ 2013 Redistributable (x64)", url: "https://aka.ms/highdpimfc2013x64enu", args: &["/install", "/quiet", "/norestart"] },
    Redist { name: "Visual C++ 2013 x86", installed: "Microsoft Visual C++ 2013 Redistributable (x86)", url: "https://aka.ms/highdpimfc2013x86enu", args: &["/install", "/quiet", "/norestart"] },
    // 2015–2026 (v14): always the newest; the installer itself does nothing when it is installed already
    Redist { name: "Visual C++ v14 x64", installed: "", url: "https://aka.ms/vs/17/release/vc_redist.x64.exe", args: &["/install", "/quiet", "/norestart"] },
    Redist { name: "Visual C++ v14 x86", installed: "", url: "https://aka.ms/vs/17/release/vc_redist.x86.exe", args: &["/install", "/quiet", "/norestart"] },
];

const DIRECTX_URL: &str = "https://download.microsoft.com/download/8/4/A/84A35BF1-DAFE-4AE8-82AF-AD2AE20B6B14/directx_Jun2010_redist.exe";

fn vc_id(name: &str) -> String {
    name.replace([' ', '+'], "-").to_lowercase()
}

fn directx_missing() -> bool {
    // d3dx9_43.dll is the last file of the June 2010 runtime
    !Path::new(&format!(r"{}\System32\d3dx9_43.dll", sys::env("SystemRoot", r"C:\Windows"))).exists()
}

/// What the Runtimes step needs to download on this PC: (id, name, link); `all`: everything (the backup for another
/// installation), not only what this PC is missing.
pub fn runtime_downloads(all: bool) -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    for r in VC {
        if all || r.installed.is_empty() || installed_version(r.installed).is_none() {
            out.push((vc_id(r.name), r.name.to_string(), r.url.to_string()));
        }
    }
    if all || directx_missing() {
        out.push(("directx".into(), "DirectX".into(), DIRECTX_URL.into()));
    }
    // offline: the .NET versions the backup brought
    let versions = dotnet_versions()
        .ok()
        .or_else(|| Some(crate::downloads::prepared_ids("dotnet-").into_iter().map(|id| id["dotnet-".len()..].to_string()).collect()));
    if let Some(vs) = versions {
        for v in vs {
            if all || !PathBuf::from(program_files()).join(r"dotnet\shared\Microsoft.WindowsDesktop.App").join(&v).exists() {
                out.push((
                    format!("dotnet-{v}"),
                    format!(".NET Desktop Runtime {v}"),
                    format!("https://builds.dotnet.microsoft.com/dotnet/WindowsDesktop/{v}/windowsdesktop-runtime-{v}-win-x64.exe"),
                ));
            }
        }
    }
    out
}

/// The installer of `id` (downloaded at the start or chosen by hand, else now).
fn runtime_file(c: &Ctx, id: &str, name: &str, url: &str) -> Result<PathBuf, String> {
    if let Some((f, _)) = crate::downloads::prepared(id) {
        return Ok(f);
    }
    if !crate::drivers::online() {
        return Err(crate::steps::NEEDS_INTERNET.into());
    }
    let file = sys::work_dir().join(format!("{id}.exe"));
    c.log(format!("{} {name}", tr("Downloading", "Stahuji")));
    sys::download(url, &file, None)?;
    Ok(file)
}

/// Runs an installer; "a newer version is installed" (1638, 5100) and "restart needed" (3010) count as done.
fn run_installer(c: &Ctx, id: &str, name: &str, url: &str, args: &[&str]) -> Result<(), String> {
    let file = runtime_file(c, id, name, url)?;
    c.log(format!("{} {name}", tr("Installing", "Instaluji")));
    match sys::exec(&file.display().to_string(), args)?.code {
        0 | 3010 | 1638 | 5100 => Ok(()),
        code => Err(format!("{name}: {} {code}", tr("ended with code", "skončil s kódem"))),
    }
}

pub fn runtimes(c: &Ctx) -> Res {
    let mut done = Vec::new();
    let mut errors = Vec::new();
    let mut later = false;
    for (id, name, url) in runtime_downloads(false) {
        let r = if id == "directx" {
            (|| -> Result<(), String> {
                let file = runtime_file(c, &id, &name, &url)?;
                c.log(tr("Installing DirectX (June 2010)", "Instaluji DirectX (červen 2010)"));
                let dir = sys::work_dir().join("DirectX");
                sys::run(&file.display().to_string(), &["/Q", &format!("/T:{}", dir.display())])?;
                let r = sys::run(&dir.join("DXSETUP.exe").display().to_string(), &["/silent"]);
                let _ = std::fs::remove_dir_all(&dir);
                r.map(|_| ())
            })()
        } else if id.starts_with("dotnet-") {
            run_installer(c, &id, &name, &url, &["/install", "/quiet", "/norestart"])
        } else {
            let args = VC.iter().find(|r| vc_id(r.name) == id).map(|r| r.args).unwrap_or(&["/install", "/quiet", "/norestart"]);
            run_installer(c, &id, &name, &url, args)
        };
        match r {
            Ok(()) => done.push(name),
            // not in the backup and no internet yet: the step runs again later for these
            Err(e) if e == crate::steps::NEEDS_INTERNET => later = true,
            Err(e) => errors.push(e),
        }
    }
    if !errors.is_empty() {
        return Err(errors.join("; "));
    }
    if later {
        return Err(crate::steps::NEEDS_INTERNET.into());
    }
    Ok(if done.is_empty() { tr("Everything was installed already", "Vše už bylo nainstalované").into() } else { done.join(", ") })
}

/// The newest runtime of each supported .NET (support phase active or maintenance).
pub fn dotnet_versions() -> Result<Vec<String>, String> {
    let idx = sys::fetch("https://builds.dotnet.microsoft.com/dotnet/release-metadata/releases-index.json", None)?;
    let v: serde_json::Value = serde_json::from_str(&idx).map_err(|e| e.to_string())?;
    Ok(v["releases-index"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter(|r| matches!(r["support-phase"].as_str(), Some("active") | Some("maintenance")))
                .filter_map(|r| r["latest-runtime"].as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default())
}

// ---------------------------------------------------------------- network drives

/// The user saved for a server in Credential Manager.
pub fn saved_user(server: &str) -> Option<String> {
    use windows_sys::Win32::Security::Credentials::{CREDENTIALW, CRED_TYPE_DOMAIN_PASSWORD, CredFree, CredReadW};
    let t = sys::wide(server);
    let mut p: *mut CREDENTIALW = std::ptr::null_mut();
    if unsafe { CredReadW(t.as_ptr(), CRED_TYPE_DOMAIN_PASSWORD, 0, &mut p) } == 0 || p.is_null() {
        return None;
    }
    let name = unsafe {
        let u = (*p).UserName;
        let s = if u.is_null() { String::new() } else { String::from_utf16_lossy(std::slice::from_raw_parts(u, (0..).take_while(|&i| *u.add(i) != 0).count())) };
        CredFree(p as _);
        s
    };
    Some(name)
}

/// Saves the user and password for a server (as "Remember my credentials" does).
pub fn save_credential(server: &str, user: &str, password: &str) -> Result<(), String> {
    use windows_sys::Win32::Security::Credentials::{CREDENTIALW, CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_DOMAIN_PASSWORD, CredWriteW};
    let (mut t, mut u) = (sys::wide(server), sys::wide(user));
    let blob: Vec<u16> = password.encode_utf16().collect();
    let mut cred: CREDENTIALW = unsafe { std::mem::zeroed() };
    cred.Type = CRED_TYPE_DOMAIN_PASSWORD;
    cred.TargetName = t.as_mut_ptr();
    cred.UserName = u.as_mut_ptr();
    cred.CredentialBlob = blob.as_ptr() as *mut u8;
    cred.CredentialBlobSize = (blob.len() * 2) as u32;
    cred.Persist = CRED_PERSIST_LOCAL_MACHINE;
    if unsafe { CredWriteW(&cred, 0) } == 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    Ok(())
}

pub fn network_drives(c: &Ctx) -> Res {
    let drives = config::drives();
    // the NAS not reachable yet (no network, no driver of the network card yet): the drives wait for the end, after
    // the internet is connected (step "Connect to the internet")
    let reachable = drives.iter().any(|d| {
        use std::net::{TcpStream, ToSocketAddrs};
        [d.server(), d.ip.clone()].into_iter().filter(|h| !h.is_empty()).any(|h| {
            (h.as_str(), 445)
                .to_socket_addrs()
                .ok()
                .and_then(|mut a| a.next())
                .is_some_and(|a| TcpStream::connect_timeout(&a, std::time::Duration::from_secs(3)).is_ok())
        })
    });
    if !drives.is_empty() && !reachable {
        c.log(tr("The NAS is not reachable yet – the drives are mapped at the end", "NAS zatím není dostupný – disky se připojí na konci"));
        return Err(crate::steps::NEEDS_INTERNET.into());
    }
    // the password for the name, the short name and the IP of the server (the IP is the way when the name is not found)
    for (server, user, password) in c.passwords() {
        let short = server.split('.').next().unwrap_or(&server).to_string();
        let mut targets = vec![server.clone(), short];
        targets.extend(drives.iter().filter(|d| d.server().eq_ignore_ascii_case(&server) && !d.ip.is_empty()).map(|d| d.ip.clone()));
        targets.dedup();
        for t in targets {
            save_credential(&t, &user, &password)?;
        }
    }
    // drives mapped by an elevated program are visible to File Explorer too (after a restart)
    sys::set_dword(Root::Hklm, r"SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\System", "EnableLinkedConnections", 1)?;
    let mut done = Vec::new();
    let mut errors = Vec::new();
    let mut mapped: Vec<(String, String)> = Vec::new();
    for d in &drives {
        let letter = format!("{}:", d.letter);
        let _ = sys::exec("net.exe", &["use", &letter, "/delete", "/y"]);
        let mut share = d.share.clone();
        c.log(format!("{letter} → {share}"));
        let mut r = sys::run("net.exe", &["use", &letter, &share, "/persistent:yes"]);
        // 53 / 67: the name of the server is not found (no mDNS / LLMNR, e.g. a VM behind NAT) → by its IP
        if r.as_ref().is_err_and(|e| e.contains(" 2:") || e.contains("53") || e.contains("67")) && !d.ip.is_empty() {
            share = d.share.replacen(&d.server(), &d.ip, 1);
            c.log(format!("{letter} → {share}"));
            sys::log(&format!("{} not found by name, trying the IP {}", d.server(), d.ip));
            r = sys::run("net.exe", &["use", &letter, &share, "/persistent:yes"]);
        }
        if let Err(e) = r {
            errors.push(format!("{letter} {}: {e}", d.share));
            continue;
        }
        if !d.label.is_empty() {
            let mp = format!(r"Software\Microsoft\Windows\CurrentVersion\Explorer\MountPoints2\{}", share.replace('\\', "#"));
            sys::set_sz(Root::Hkcu, &mp, "_LabelFromReg", &d.label)?;
        }
        done.push(format!("{letter} {}", if d.label.is_empty() { &share } else { &d.label }));
        mapped.push((letter, share));
    }
    map_for_user(c, &mapped);
    if !errors.is_empty() {
        return Err(errors.join("; "));
    }
    trust_servers(&drives)?;
    Ok(done.join(", "))
}

/// HeMimizer runs as administrator, and Windows keeps the drives of the administrator token apart from the
/// normal one: File Explorer would not see them before the next sign-in. So the same `net use` runs once more
/// as the user without elevation – a scheduled task with the limited token, started at once and deleted.
fn map_for_user(c: &Ctx, mapped: &[(String, String)]) {
    if mapped.is_empty() {
        return;
    }
    c.log(tr("Connecting the drives for File Explorer", "Připojuji disky pro Průzkumník"));
    let cmd = mapped.iter().map(|(l, s)| format!("net use {l} /delete /y & net use {l} \"{s}\" /persistent:yes")).collect::<Vec<_>>().join(" & ");
    let r = sys::ps(&format!(
        "$u = [Security.Principal.WindowsIdentity]::GetCurrent().Name\r\n\
         $a = New-ScheduledTaskAction -Execute 'cmd.exe' -Argument '/c {cmd}'\r\n\
         $p = New-ScheduledTaskPrincipal -UserId $u -LogonType Interactive -RunLevel Limited\r\n\
         Register-ScheduledTask -TaskName 'HeMimizer - drives' -Action $a -Principal $p -Force | Out-Null\r\n\
         Start-ScheduledTask -TaskName 'HeMimizer - drives'\r\n\
         for ($i = 0; $i -lt 30 -and (Get-ScheduledTask -TaskName 'HeMimizer - drives').State -eq 'Running'; $i++) {{ Start-Sleep 1 }}\r\n\
         (Get-ScheduledTaskInfo -TaskName 'HeMimizer - drives').LastTaskResult\r\n\
         Unregister-ScheduledTask -TaskName 'HeMimizer - drives' -Confirm:$false",
        cmd = cmd.replace('\'', "''")
    ));
    if let Err(e) = r {
        sys::log(&format!("mapping for the user failed: {e}"));
    }
}

/// Every network drive and \\server path – also ones added later – in the zone Local intranet, where opening and
/// running files is allowed: no "Opening these files might be harmful" for exe / .reg / …, and copied files get no
/// Mark of the Web – network drives behave like C:. `ProtocolDefaults\file = 1` makes Local intranet the zone of
/// every file:// address no other rule matches (Windows has Internet, 3, there); the servers of the drives are
/// listed by name too.
pub fn trust_servers(drives: &[config::Drive]) -> Result<(), String> {
    let inet = r"Software\Microsoft\Windows\CurrentVersion\Internet Settings";
    let mut hosts: Vec<String> = Vec::new();
    for d in drives {
        let s = d.server().to_lowercase();
        let short = s.split('.').next().unwrap_or(&s).to_string();
        for h in [s, short] {
            if !h.is_empty() && !hosts.contains(&h) {
                hosts.push(h);
            }
        }
    }
    for h in &hosts {
        // 1 = Local intranet, for file:// (UNC paths and mapped drives)
        sys::set_dword(Root::Hkcu, &format!(r"{inet}\ZoneMap\Domains\{h}"), "file", 1)?;
    }
    sys::dwords(Root::Hkcu, &format!(r"{inet}\ZoneMap"), &[("UNCAsIntranet", 1), ("IntranetName", 1)])?;
    sys::set_dword(Root::Hkcu, &format!(r"{inet}\ZoneMap\ProtocolDefaults"), "file", 1)?;
    // Local intranet: launching programs and unsafe files = enable (0)
    sys::set_dword(Root::Hkcu, &format!(r"{inet}\Zones\1"), "1806", 0)?;
    Ok(())
}

// ---------------------------------------------------------------- HeMiDock

pub const HEMIDOCK_SETUP: &str = "https://github.com/Hemal1k/HeMiDock/releases/latest/download/HeMiDock-Setup.exe";

/// Where HeMiDock is: the folder of its installation (Apps & features), else where its installer puts it for the user
/// (%LOCALAPPDATA%\Programs\HeMi\HeMiDock; before 1.8.5 it was Programs\HeMiDock).
pub fn hemidock_install_dir() -> PathBuf {
    let key = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\HeMiDock_is1";
    if let Some(dir) = sys::get_sz(sys::Root::Hkcu, key, "InstallLocation").or_else(|| sys::get_sz(sys::Root::Hklm, key, "InstallLocation")) {
        let dir = PathBuf::from(dir.trim_end_matches('\\'));
        if dir.join("HeMiDock.exe").exists() {
            return dir;
        }
    }
    PathBuf::from(sys::env("LOCALAPPDATA", "")).join(r"Programs\HeMi\HeMiDock")
}

/// The newest HeMiDock from its GitHub releases, through its installer (into %LOCALAPPDATA%\Programs\HeMi\HeMiDock; the
/// installer adds the .NET Runtime and the Windows App Runtime when they are missing); then the captured settings,
/// and the dock started as the user (not as administrator).
pub fn hemidock(c: &Ctx) -> Res {
    if !crate::install::chosen("hemidock") {
        return Ok(crate::install::not_chosen());
    }
    let latest = sys::ps("(Invoke-RestMethod 'https://api.github.com/repos/Hemal1k/HeMiDock/releases/latest' -UseBasicParsing).tag_name")
        .map(|v| v.trim().trim_start_matches('v').to_string())
        .unwrap_or_default();
    let installed = sys::ps("(Get-ItemProperty 'HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\HeMiDock_is1' -ErrorAction SilentlyContinue).DisplayVersion")
        .map(|v| v.trim().to_string())
        .unwrap_or_default();
    let exe = hemidock_install_dir().join("HeMiDock.exe");
    let fresh = !latest.is_empty() && latest == installed && exe.exists();
    if !fresh {
        let name = if latest.is_empty() { "HeMiDock".to_string() } else { format!("HeMiDock {latest}") };
        run_installer(c, "hemidock", &name, HEMIDOCK_SETUP, &["/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART"])?;
        if !exe.exists() {
            return Err(tr("HeMiDock.exe is missing after the installation.", "Po instalaci chybí HeMiDock.exe.").into());
        }
    }
    // the installer starts the dock at its end – with the rights of HeMimizer (administrator): stopped, started again below
    std::thread::sleep(std::time::Duration::from_secs(2));
    if sys::is_running("HeMiDock.exe") {
        c.log(tr("Closing the running HeMiDock", "Zavírám spuštěný HeMiDock"));
        let _ = sys::exec(&exe.display().to_string(), &["--exit"]);
        std::thread::sleep(std::time::Duration::from_secs(3));
    }
    hemidock_settings(c, &exe)?;
    // started through File Explorer so that it runs as the user, not as administrator
    let _ = sys::exec("explorer.exe", &[&exe.display().to_string()]);
    let version = sys::ps("(Get-ItemProperty 'HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\HeMiDock_is1' -ErrorAction SilentlyContinue).DisplayVersion")
        .map(|v| v.trim().to_string())
        .unwrap_or_default();
    Ok(if fresh {
        format!("HeMiDock {version} – {}", tr("already the newest, settings applied", "už je nejnovější, nastavení použito"))
    } else {
        format!("HeMiDock {version}, {}", tr("your settings", "tvoje nastavení"))
    })
}
/// The files of HeMiDock that hold its settings (not the backups, logs and statistics).
pub const HEMIDOCK_FILES: &[&str] = &["settings.json", "pinned.json", "startpins.json", "tray.json", "workspaces.json"];

/// The settings of HeMiDock as captured (hemidock.tar: the files above + the folder Pinned with the shortcuts of
/// the dock) into %AppData%\HeMiDock, with the dock stopped. A pinned shortcut whose program is somewhere else
/// now (Brave was per-user, HeMimizer installs it for all users) is replaced by the shortcut of the same name
/// in the Start menu.
fn hemidock_settings(c: &Ctx, exe: &Path) -> Result<(), String> {
    let tar = config::bytes("hemidock.tar");
    if tar.is_empty() {
        return Ok(());
    }
    c.log(tr("Applying the settings of HeMiDock", "Nastavuji HeMiDock"));
    let tmp = sys::work_dir().join("HeMiDock-settings");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
    let file = sys::work_dir().join("hemidock.tar");
    std::fs::write(&file, tar).map_err(|e| e.to_string())?;
    sys::run("tar.exe", &["-xf", &file.display().to_string(), "-C", &tmp.display().to_string()])?;
    sys::remove_file(&file);

    if exe.exists() && sys::is_running("HeMiDock.exe") {
        let _ = sys::exec(&exe.display().to_string(), &["--exit"]);
        std::thread::sleep(std::time::Duration::from_secs(3));
    }
    // HeMiDock 1.7 and newer keep it in %AppData%HeMiHeMiDock
    let data = PathBuf::from(sys::env("APPDATA", "")).join("HeMi").join("HeMiDock");
    std::fs::create_dir_all(data.join("Pinned")).map_err(|e| e.to_string())?;
    let profile = sys::profile().display().to_string().replace('\\', r"\\");
    for f in HEMIDOCK_FILES {
        if let Ok(t) = std::fs::read_to_string(tmp.join(f)) {
            std::fs::write(data.join(f), t.replace("{PROFILE}", &profile)).map_err(|e| format!("{f}: {e}"))?;
        }
    }
    if let Ok(rd) = std::fs::read_dir(tmp.join("Pinned")) {
        for e in rd.flatten() {
            let _ = std::fs::copy(e.path(), data.join("Pinned").join(e.file_name()));
        }
    }
    fix_shortcuts(&data.join("Pinned"))?;
    let _ = std::fs::remove_dir_all(&tmp);
    Ok(())
}

/// Shortcuts in `dir` that lead nowhere now (Brave was per-user on the author's PC, HeMimizer installs it for all
/// users) → the shortcut of the same name from the Start menu.
pub fn fix_shortcuts(dir: &Path) -> Result<(), String> {
    let start = [start_menu(), PathBuf::from(sys::env("APPDATA", "")).join(r"Microsoft\Windows\Start Menu\Programs")];
    let starts = start.iter().map(|p| format!("'{}'", p.display().to_string().replace('\'', "''"))).collect::<Vec<_>>().join(",");
    sys::ps(&format!(
        "$sh = New-Object -ComObject WScript.Shell; \
         Get-ChildItem -LiteralPath '{pinned}' -Filter *.lnk | ForEach-Object {{ \
           $t = $sh.CreateShortcut($_.FullName).TargetPath; \
           if ($t -and -not (Test-Path -LiteralPath $t)) {{ \
             $new = Get-ChildItem -LiteralPath @({starts}) -Filter $_.Name -Recurse -ErrorAction SilentlyContinue | Select-Object -First 1; \
             if ($new) {{ Copy-Item -LiteralPath $new.FullName -Destination $_.FullName -Force; \"fixed $($_.Name)\" }} else {{ Remove-Item -LiteralPath $_.FullName -Force; \"left out $($_.Name) (not installed)\" }} }} }}",
        pinned = dir.display().to_string().replace('\'', "''")
    ))
    .map(|_| ())
}

// ---------------------------------------------------------------- default apps

/// The name under which an app is listed in Default apps (HKCU or HKLM RegisteredApplications).
pub fn registered_app(part: &str) -> Option<(bool, String)> {
    for (user, root) in [(true, "HKCU"), (false, "HKLM")] {
        let o = sys::exec("reg.exe", &["query", &format!(r"{root}\Software\RegisteredApplications")]).ok()?;
        for line in o.text.lines() {
            let name = line.trim().split("    ").next().unwrap_or("").trim();
            if name.to_lowercase().contains(&part.to_lowercase()) {
                return Some((user, name.to_string()));
            }
        }
    }
    None
}

/// Opens the page of the app in Settings → Default apps (Windows does not let programs choose defaults).
pub fn open_default_apps(part: &str) {
    match registered_app(part) {
        Some((true, n)) => sys::shell_open(&format!("ms-settings:defaultapps?registeredAppUser={n}")),
        Some((false, n)) => sys::shell_open(&format!("ms-settings:defaultapps?registeredAppMachine={n}")),
        None => sys::shell_open("ms-settings:defaultapps"),
    }
}
