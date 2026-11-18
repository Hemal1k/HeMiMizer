//! The settings of Windows as on the PC of the author.

use crate::config;
use crate::i18n::tr;
use crate::steps::{Ctx, Res};
use crate::sys::{self, Root};

const CDM: &str = r"Software\Microsoft\Windows\CurrentVersion\ContentDeliveryManager";

// ---------------------------------------------------------------- Windows features

const FEATURES_ON: &[&str] = &["NetFx3", "NetFx4-AdvSrvs", "SmbDirect"];
const FEATURES_OFF: &[&str] = &[
    "Printing-PrintToPDFServices-Features",
    "Printing-XPSServices-Features",
    "Printing-Foundation-Features",
    "Printing-Foundation-InternetPrinting-Client",
    "MSRDC-Infrastructure",
    "WorkFolders-Client",
    "MediaPlayback",
    "WindowsMediaPlayer",
    "SMB1Protocol",
    "MicrosoftWindowsPowerShellV2Root",
    "MicrosoftWindowsPowerShellV2",
    "Recall",
    // Remote Desktop Connection (mstsc) – an optional feature since Windows 11 24H2
    "Microsoft-RemoteDesktopConnection",
];

pub fn features(c: &Ctx) -> Res {
    let list = sys::run("dism.exe", &["/online", "/get-features", "/format:table", "/English"])?;
    let state = |name: &str| -> Option<bool> {
        list.lines().find_map(|l| {
            let mut p = l.split('|');
            let n = p.next()?.trim();
            (n.eq_ignore_ascii_case(name)).then(|| p.next().map(|s| s.trim().starts_with("Enabled")).unwrap_or(false))
        })
    };
    let mut changed = 0;
    for f in FEATURES_ON {
        if state(f) == Some(false) {
            c.log(format!("{} {f}", tr("Turning on", "Zapínám")));
            sys::run("dism.exe", &["/online", "/enable-feature", &format!("/featurename:{f}"), "/all", "/norestart", "/quiet"])?;
            changed += 1;
        }
    }
    for f in FEATURES_OFF {
        if state(f) == Some(true) {
            c.log(format!("{} {f}", tr("Turning off", "Vypínám")));
            sys::run("dism.exe", &["/online", "/disable-feature", &format!("/featurename:{f}"), "/norestart", "/quiet"])?;
            changed += 1;
        }
    }
    if changed > 0 {
        c.reboot();
    }
    Ok(format!("{} {changed}", tr("Changed:", "Změněno:")))
}

// ---------------------------------------------------------------- wallpaper, Spotlight, lock screen

pub fn wallpaper(_c: &Ctx) -> Res {
    use windows_sys::Win32::UI::WindowsAndMessaging::{SPI_SETDESKWALLPAPER, SPIF_SENDCHANGE, SPIF_UPDATEINIFILE, SystemParametersInfoW};
    let Some((name, data)) = config::wallpaper() else { return Ok(config::not_backed_up()) };
    let file = sys::pictures().join(name);
    std::fs::create_dir_all(sys::pictures()).map_err(|e| e.to_string())?;
    std::fs::write(&file, data).map_err(|e| format!("{}: {e}", file.display()))?;
    let path = file.display().to_string();

    // Spotlight off on the desktop and on the lock screen, no tips on the lock screen
    sys::set_dword(Root::Hkcu, r"Software\Microsoft\Windows\CurrentVersion\DesktopSpotlight\Settings", "EnabledState", 0)?;
    sys::dwords(Root::Hkcu, r"Software\Policies\Microsoft\Windows\CloudContent", &[("DisableSpotlightCollectionOnDesktop", 1), ("DisableWindowsSpotlightOnSettings", 1)])?;
    sys::dwords(Root::Hkcu, CDM, &[("RotatingLockScreenEnabled", 0), ("RotatingLockScreenOverlayEnabled", 0), ("SubscribedContent-338387Enabled", 0)])?;
    sys::dwords(Root::Hkcu, r"Software\Microsoft\Windows\CurrentVersion\Lock Screen", &[("LockScreenWidgetsEnabled", 0)])?;

    // the desktop: Fill
    sys::set_sz(Root::Hkcu, r"Control Panel\Desktop", "WallpaperStyle", "10")?;
    sys::set_sz(Root::Hkcu, r"Control Panel\Desktop", "TileWallpaper", "0")?;
    sys::set_dword(Root::Hkcu, r"Software\Microsoft\Windows\CurrentVersion\Explorer\Wallpapers", "BackgroundType", 0)?;
    let w = sys::wide(&path);
    let ok = unsafe { SystemParametersInfoW(SPI_SETDESKWALLPAPER, 0, w.as_ptr() as _, SPIF_UPDATEINIFILE | SPIF_SENDCHANGE) };
    if ok == 0 {
        return Err(tr("Windows did not accept the wallpaper.", "Windows tapetu nepřijal.").into());
    }
    // the lock screen (the Personalization CSP of Windows Pro)
    let csp = r"SOFTWARE\Microsoft\Windows\CurrentVersion\PersonalizationCSP";
    sys::set_sz(Root::Hklm, csp, "LockScreenImagePath", &path)?;
    sys::set_sz(Root::Hklm, csp, "LockScreenImageUrl", &path)?;
    sys::set_dword(Root::Hklm, csp, "LockScreenImageStatus", 1)?;
    Ok(path)
}

// ---------------------------------------------------------------- File Explorer, Recycle Bin

/// The view of File Explorer from the backup (file extensions, hidden files…); the rest is in the single settings.
pub fn explorer(_c: &Ctx) -> Res {
    if !config::has("explorer-view.reg") {
        return Ok(config::not_backed_up());
    }
    sys::reg_import(&config::text("explorer-view.reg", true))?;
    Ok(String::new())
}

// ---------------------------------------------------------------- updates, Game Bar, sign-in

/// The settings of Windows Update, then every update that is not a driver (drivers come only from HeMimizer –
/// the step "No drivers from Windows Update" runs before and the search asks for software only).
pub fn windows_update(c: &Ctx) -> Res {
    // updates for other Microsoft products: on
    sys::set_dword(Root::Hklm, r"SOFTWARE\Microsoft\WindowsUpdate\UX\Settings", "AllowMUUpdateService", 1)?;
    sys::ps(
        "$sm=New-Object -ComObject Microsoft.Update.ServiceManager; $sm.ClientApplicationID='HeMimizer'; \
         $null=$sm.AddService2('7971f918-a847-4430-9279-4a52d1efe18d',7,'')",
    )?;
    // Delivery Optimization (downloads from other PCs): off
    sys::set_dword(Root::Hku, r"S-1-5-20\Software\Microsoft\Windows\CurrentVersion\DeliveryOptimization\Settings", "DownloadMode", 0)?;

    c.log(tr("Looking for updates", "Hledám aktualizace"));
    let out = sys::ps(
        r#"
$s = New-Object -ComObject Microsoft.Update.Session; $s.ClientApplicationID = 'HeMimizer'
$found = $s.CreateUpdateSearcher().Search("IsInstalled=0 and IsHidden=0 and Type='Software'").Updates
$list = New-Object -ComObject Microsoft.Update.UpdateColl
foreach ($u in $found) { if (-not $u.EulaAccepted) { $u.AcceptEula() }; [void]$list.Add($u) }
if ($list.Count -eq 0) { 'COUNT=0'; return }
$d = $s.CreateUpdateDownloader(); $d.Updates = $list; [void]$d.Download()
$i = $s.CreateUpdateInstaller(); $i.Updates = $list; $r = $i.Install()
"COUNT=$($list.Count)"; "REBOOT=$($r.RebootRequired)"; "RESULT=$($r.ResultCode)"
foreach ($u in $list) { "TITLE=$($u.Title)" }
"#,
    )?;
    let get = |k: &str| out.lines().find_map(|l| l.trim().strip_prefix(k)).unwrap_or("").to_string();
    let count = get("COUNT=");
    if count == "0" {
        return Ok(tr("No updates", "Žádné aktualizace").into());
    }
    if get("REBOOT=").eq_ignore_ascii_case("true") {
        c.reboot();
    }
    // ResultCode: 2 = succeeded, 3 = succeeded with errors, 4 = failed
    match get("RESULT=").as_str() {
        "4" | "5" => Err(format!("{} ({count})", tr("The updates failed to install", "Aktualizace se nepodařilo nainstalovat"))),
        code => Ok(format!(
            "{} {count}{}",
            tr("Installed:", "Nainstalováno:"),
            if code == "3" { tr(" (some with errors)", " (některé s chybou)") } else { "" }
        )),
    }
}

// ---------------------------------------------------------------- colours, language, File Explorer view

/// Dark mode and the accent colour as captured, Czech only (keyboard, formats), the time zone of Prague.
pub fn appearance(_c: &Ctx) -> Res {
    if config::has("appearance.reg") {
        sys::reg_import(&config::text("appearance.reg", true))?;
    }
    sys::settings_changed();
    Ok(if config::has("appearance.reg") { tr("Colours from the backup", "Barvy ze zálohy").into() } else { config::not_backed_up() })
}

// ---------------------------------------------------------------- Defender

/// Microsoft Defender as on the PC of the author (defender.txt: Get-MpPreference + memory integrity), then the
/// exclusions: the Steam libraries (games load faster) and the files qBittorrent writes while downloading
/// (a downloaded file is still scanned when it is opened).
pub fn defender(c: &Ctx) -> Res {
    let settings = config::defender();
    let mut failed = Vec::new();
    let mut set = 0;
    let quote = |s: &str| format!("'{}'", s.replace('\'', "''"));
    // only what differs from now: a program that rewrites the settings of the antivirus one by one looks like malware
    let now: Vec<(String, String)> = sys::ps(&format!(
        "$m = Get-MpPreference; foreach ($n in '{}'.Split(',')) {{ $v = $m.$n; if ($v -is [array]) {{ $v = $v -join ';' }}; \"$n=$v\" }}",
        config::DEFENDER_SETTINGS.join(",")
    ))
    .unwrap_or_default()
    .lines()
    .filter_map(|l| l.trim().split_once('=').map(|(k, v)| (k.to_string(), v.to_string())))
    .collect();
    let same = |n: &str, v: &str| now.iter().any(|(k, x)| k == n && x.eq_ignore_ascii_case(v));
    let mut script = Vec::new();
    for (name, value) in &settings {
        if name == "MemoryIntegrity" {
            // Core isolation → Memory integrity (applies after a restart)
            let on = (value == "1") as u32;
            let k = r"SYSTEM\CurrentControlSet\Control\DeviceGuard\Scenarios\HypervisorEnforcedCodeIntegrity";
            sys::set_dword(Root::Hklm, k, "Enabled", on)?;
            sys::set_dword(Root::Hklm, k, "WasEnabledBy", 2)?;
            c.reboot();
            continue;
        }
        if !config::DEFENDER_SETTINGS.contains(&name.as_str()) {
            continue;
        }
        let arg = if name.starts_with("Exclusion") {
            if value.is_empty() {
                continue;
            }
            value.split(';').map(|v| quote(v)).collect::<Vec<_>>().join(",")
        } else if value.eq_ignore_ascii_case("true") || value.eq_ignore_ascii_case("false") {
            format!("${}", value.to_lowercase())
        } else if value.parse::<i64>().is_ok() {
            value.clone()
        } else {
            continue;
        };
        if !name.starts_with("Exclusion") && same(name, value) {
            continue;
        }
        let cmd = if name.starts_with("Exclusion") { "Add-MpPreference" } else { "Set-MpPreference" };
        // each on its own: with Tamper Protection on, Windows refuses some of them – the rest still applies
        script.push(format!("try {{ {cmd} -{name} {arg}; 'OK {name}' }} catch {{ 'FAIL {name}' }}"));
    }
    if !script.is_empty() {
        let out = sys::ps(&script.join("\r\n")).unwrap_or_default();
        for l in out.lines() {
            if let Some(n) = l.trim().strip_prefix("FAIL ") {
                failed.push(n.to_string());
            } else if l.trim().starts_with("OK ") {
                set += 1;
            }
        }
    }
    if !failed.is_empty() {
        sys::log(&format!("Defender settings not applied: {}", failed.join(", ")));
    }
    let mut paths: Vec<String> = config::steam_libraries();
    let steam = sys::get_sz(Root::Hkcu, r"Software\Valve\Steam", "SteamPath").map(|p| p.replace('/', "\\"));
    paths.extend(steam);
    paths.sort();
    paths.dedup();
    let list = paths.iter().map(|p| format!("'{}'", p.replace('\'', "''"))).collect::<Vec<_>>().join(",");
    let qbit = format!(r"{}\qBittorrent\qbittorrent.exe", sys::env("ProgramFiles", r"C:\Program Files"));
    sys::ps(&format!(
        "if (-not (Get-Command Add-MpPreference -ErrorAction SilentlyContinue)) {{ 'NODEFENDER'; return }}; \
         Add-MpPreference -ExclusionPath @({list}) ; Add-MpPreference -ExclusionProcess '{qbit}'"
    ))
    .and_then(|o| if o.contains("NODEFENDER") { Err(tr("Defender is not in this Windows.", "Defender v tomto Windows není.").into()) } else { Ok(()) })?;
    let mi = settings.iter().find(|(k, _)| k == "MemoryIntegrity").map(|(_, v)| v == "1");
    Ok(format!(
        "{} {set}{}{}; {}: {}, qBittorrent",
        tr("Settings:", "Nastavení:"),
        if failed.is_empty() { String::new() } else { format!(" ({} {})", tr("refused:", "odmítnuto:"), failed.len()) },
        match mi {
            Some(true) => tr(", memory integrity on", ", integrita paměti zapnutá"),
            Some(false) => tr(", memory integrity off", ", integrita paměti vypnutá"),
            None => "",
        },
        tr("exclusions", "výjimky"),
        paths.join(", ")
    ))
}

/// A Defender exclusion for HeMimizer itself while it works (the user's wish): its folder, its work folders and
/// its process. Defender's machine learning stopped it in the middle of a run (Trojan:Win32/Bearfoos.A!ml) and
/// left Windows half set up. Removed again at the end of the run (kept while a restart is pending).
pub fn self_exclusion(on: bool) {
    let exe = std::env::current_exe().map(|e| e.display().to_string()).unwrap_or_default();
    let mut paths = vec![sys::work_dir().display().to_string(), std::env::temp_dir().join("HeMimizer").display().to_string()];
    if let Some(d) = std::env::current_exe().ok().and_then(|e| e.parent().map(|p| p.display().to_string())) {
        paths.push(d);
    }
    paths.push(std::path::PathBuf::from(sys::env("LOCALAPPDATA", "")).join(r"HeMimizer\app").display().to_string());
    let list = paths.iter().map(|p| format!("'{}'", p.replace('\'', "''"))).collect::<Vec<_>>().join(",");
    let verb = if on { "Add" } else { "Remove" };
    let _ = sys::ps(&format!(
        "if (Get-Command {verb}-MpPreference -ErrorAction SilentlyContinue) {{ \
         try {{ {verb}-MpPreference -ExclusionPath @({list}) -ExclusionProcess '{}' }} catch {{ \"failed: $_\" }} }}",
        exe.replace('\'', "''")
    ));
}

// ---------------------------------------------------------------- Microsoft Store

/// Asks Microsoft Store to look for updates of its apps and download them – in the background, HeMimizer does not
/// wait for it. Through the MDM bridge (as "Get updates" in the Store); winget is the fallback.
pub fn store_updates(c: &Ctx) -> Res {
    let has_store = sys::ps("@(Get-AppxPackage -Name Microsoft.WindowsStore).Count").is_ok_and(|n| n.trim() != "0");
    if !has_store {
        return Ok(tr("Microsoft Store is not in this Windows", "Microsoft Store v tomto Windows není").into());
    }
    c.log(tr("Asking Microsoft Store for updates", "Žádám Microsoft Store o aktualizace"));
    let out = sys::ps(
        "try { Get-CimInstance -Namespace 'Root\\cimv2\\mdm\\dmmap' -ClassName 'MDM_EnterpriseModernAppManagement_AppManagement01' -ErrorAction Stop | \
         Invoke-CimMethod -MethodName UpdateScanMethod -ErrorAction Stop | Out-Null; 'STARTED' } catch { \"CIM: $_\" }",
    )
    .unwrap_or_default();
    if out.contains("STARTED") {
        return Ok(tr("Store updates run in the background", "Aktualizace Storu běží na pozadí").into());
    }
    // winget (App Installer) – started and left running
    let winget = std::path::PathBuf::from(sys::env("LOCALAPPDATA", "")).join(r"Microsoft\WindowsApps\winget.exe");
    if winget.exists() {
        use std::os::windows::process::CommandExt;
        let r = std::process::Command::new(&winget)
            .args(["upgrade", "--all", "--source", "msstore", "--silent", "--accept-source-agreements", "--accept-package-agreements", "--disable-interactivity"])
            .creation_flags(0x0800_0000)
            .spawn();
        sys::log(&format!("winget upgrade (msstore) started: {}", r.is_ok()));
        if r.is_ok() {
            return Ok(tr("Store updates run in the background (winget)", "Aktualizace Storu běží na pozadí (winget)").into());
        }
    }
    Err(format!("{}: {}", tr("Microsoft Store did not start the updates", "Microsoft Store nespustil aktualizace"), out.trim()))
}

// ---------------------------------------------------------------- pinned in Start and on the taskbar

/// The pins as captured: start2.bin of Start (with Start closed) and the taskbar (its shortcuts + Taskband). Then
/// File Explorer is ended – Windows starts it again by itself, as the user – so that the taskbar reads them.
pub fn pins(c: &Ctx) -> Res {
    let tar = config::bytes("pins.tar");
    if tar.is_empty() {
        return Ok(tr("Nothing captured", "Nic zachyceno").into());
    }
    let tmp = sys::work_dir().join("pins");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
    let file = sys::work_dir().join("pins.tar");
    std::fs::write(&file, tar).map_err(|e| e.to_string())?;
    sys::run("tar.exe", &["-xf", &file.display().to_string(), "-C", &tmp.display().to_string()])?;
    sys::remove_file(&file);

    c.log(tr("Start", "Start"));
    let _ = sys::exec("taskkill.exe", &["/f", "/im", "StartMenuExperienceHost.exe"]);
    std::thread::sleep(std::time::Duration::from_secs(1));
    let start = config::start_bin_dir();
    std::fs::create_dir_all(&start).map_err(|e| e.to_string())?;
    std::fs::copy(tmp.join("start2.bin"), start.join("start2.bin")).map_err(|e| format!("start2.bin: {e}"))?;

    c.log(tr("Taskbar", "Hlavní panel"));
    let bar = config::taskbar_dir();
    std::fs::create_dir_all(&bar).map_err(|e| e.to_string())?;
    if let Ok(rd) = std::fs::read_dir(&bar) {
        for e in rd.flatten() {
            if e.path().extension().is_some_and(|x| x.eq_ignore_ascii_case("lnk")) {
                sys::remove_file(e.path());
            }
        }
    }
    let mut n = 0;
    if let Ok(rd) = std::fs::read_dir(tmp.join("taskbar")) {
        for e in rd.flatten() {
            if std::fs::copy(e.path(), bar.join(e.file_name())).is_ok() {
                n += 1;
            }
        }
    }
    crate::programs::fix_shortcuts(&bar)?;
    sys::delete_tree(Root::Hkcu, r"Software\Microsoft\Windows\CurrentVersion\Explorer\Taskband");
    if let Ok(t) = std::fs::read_to_string(tmp.join("taskband.reg")) {
        sys::reg_import(&t)?;
    }
    let _ = std::fs::remove_dir_all(&tmp);
    // Windows restarts File Explorer by itself (AutoRestartShell), as the user – never start it from here (elevated)
    let _ = sys::exec("taskkill.exe", &["/f", "/im", "explorer.exe"]);
    std::thread::sleep(std::time::Duration::from_secs(4));
    Ok(format!("Start + {n} {}", tr("on the taskbar", "na hlavním panelu")))
}

// ---------------------------------------------------------------- startup apps

/// Kept starting with Windows; everything else in Run is switched off as in Task Manager → Startup apps
/// (StartupApproved, so it can be switched on there again).
/// (The updater of Brave is no browser window – without it Brave would not update.)
const STARTUP_KEEP: &[&str] = &["HeMiDock", "HeMiLauncher", "SecurityHealth", "BraveSoftware Update"];
/// The programs of the wizard that add themselves to the start of Windows: (catalog id, the name of their Run value).
/// They are switched off ahead, before the programs add themselves (Steam does at its first start).
pub const STARTS_ITSELF: &[(&str, &str)] = &[
    ("steam", "Steam"),
    ("discord", "Discord"),
    ("epic", "EpicGamesLauncher"),
    ("spotify", "Spotify"),
    ("gog", "GogGalaxy"),
    ("ubisoft", "Ubisoft Connect"),
    ("ea", "EADM"),
    ("teams", "com.squirrel.Teams.Teams"),
    ("slack", "com.squirrel.slack.slack"),
    ("zoom", "Zoom"),
    ("telegram", "Telegram Desktop"),
    ("qbittorrent", "qBittorrent"),
];

/// The option "Other programs do not start with Windows": everything in Run is switched off – except the programs of
/// the wizard, whose start is chosen in the table of the last page (the step "shortcuts"), and what must run.
pub fn startup_apps(_c: &Ctx) -> Res {
    let plan = crate::plan::Plan::load().unwrap_or_default();
    let chosen: Vec<&str> = STARTS_ITSELF.iter().filter(|(id, _)| plan.has(id)).map(|(_, run)| *run).collect();
    let ahead: Vec<&str> = vec!["OneDrive"];
    // 03 = disabled, then the time of the change (zero is fine)
    let off = [3u8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    let approved = r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved";
    let mut done = Vec::new();
    for (root, run, appr) in [
        (Root::Hkcu, r"Software\Microsoft\Windows\CurrentVersion\Run", format!(r"{approved}\Run")),
        (Root::Hklm, r"SOFTWARE\Microsoft\Windows\CurrentVersion\Run", format!(r"{approved}\Run")),
        (Root::Hklm, r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Run", format!(r"{approved}\Run32")),
    ] {
        let mut names = sys::value_names(root, run);
        if matches!(root, Root::Hkcu) {
            names.extend(ahead.iter().map(|s| s.to_string()));
        }
        for n in names {
            if STARTUP_KEEP.iter().chain(&chosen).any(|k| n.eq_ignore_ascii_case(k)) {
                continue;
            }
            // HKLM Run entries are approved under HKLM, HKCU ones under HKCU
            sys::set_binary(root, &appr, &n, &off)?;
            if !done.contains(&n) {
                done.push(n);
            }
        }
    }
    Ok(done.join(", "))
}

// ---------------------------------------------------------------- System Restore

/// System Restore off on every drive and its restore points deleted; the policy keeps it off.
pub fn restore_off(c: &Ctx) -> Res {
    c.log(tr("Turning off System Restore", "Vypínám obnovu systému"));
    sys::ps("Get-CimInstance Win32_LogicalDisk -Filter 'DriveType=3' | ForEach-Object { Disable-ComputerRestore -Drive ($_.DeviceID + '\\') -ErrorAction SilentlyContinue }")?;
    sys::dwords(Root::Hklm, r"SOFTWARE\Policies\Microsoft\Windows NT\SystemRestore", &[("DisableSR", 1), ("DisableConfig", 1)])?;
    c.log(tr("Deleting the restore points", "Mažu body obnovení"));
    // vssadmin ends with an error when there is nothing to delete
    let _ = sys::exec("vssadmin.exe", &["delete", "shadows", "/all", "/quiet"]);
    let on = sys::ps("(Get-ItemProperty 'HKLM:\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\SystemRestore').RPSessionInterval").unwrap_or_default();
    if on.trim() == "1" {
        return Err(tr("Windows still reports System Restore as on.", "Windows stále hlásí obnovu systému jako zapnutou.").into());
    }
    Ok(tr("Off, restore points deleted", "Vypnuto, body obnovení smazány").into())
}

// ---------------------------------------------------------------- clean-up

fn free_c() -> u64 {
    sys::ps("(Get-PSDrive C).Free").ok().and_then(|s| s.trim().parse().ok()).unwrap_or(0)
}

/// Temporary files, the update and delivery caches, leftovers of the AMD installer, then the component store.
pub fn cleanup(c: &Ctx) -> Res {
    let before = free_c();
    c.log(tr("Deleting temporary files", "Mažu dočasné soubory"));
    sys::ps(
        r#"
$paths = "$env:TEMP\*", "$env:SystemRoot\Temp\*", "$env:LOCALAPPDATA\CrashDumps\*", "$env:SystemDrive\AMD\*", "$env:ProgramData\Microsoft\Windows\WER\ReportArchive\*", "$env:ProgramData\Microsoft\Windows\WER\ReportQueue\*"
foreach ($p in $paths) { Remove-Item $p -Recurse -Force -ErrorAction SilentlyContinue }
Delete-DeliveryOptimizationCache -Force -ErrorAction SilentlyContinue
"#,
    )?;
    c.log(tr("Cleaning up the component store (takes a few minutes)", "Čistím úložiště součástí (trvá pár minut)"));
    sys::run("dism.exe", &["/online", "/cleanup-image", "/startcomponentcleanup", "/quiet"])?;

    // Disk Clean-up with every item ticked (incl. the system files): each handler under VolumeCaches gets the
    // flag of the set 99, which cleanmgr /sagerun:99 then runs on every drive (HeMimizer runs as administrator)
    c.log(tr("Disk Clean-up with all items (can take long)", "Vyčištění disku se všemi položkami (může trvat dlouho)"));
    sys::ps(
        "Get-ChildItem 'HKLM:\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Explorer\\VolumeCaches' | \
         ForEach-Object { New-ItemProperty -Path $_.PSPath -Name StateFlags0099 -Value 2 -PropertyType DWord -Force | Out-Null }",
    )?;
    let _ = sys::exec("cleanmgr.exe", &["/sagerun:99"]);
    // cleanmgr hands part of the work to child processes – wait until none is left
    for _ in 0..360 {
        if !sys::is_running("cleanmgr.exe") {
            break;
        }
        std::thread::sleep(std::time::Duration::from_secs(5));
    }

    // the very last thing: the downloaded updates (Windows Update must be stopped for it)
    c.log(tr("Deleting the downloaded updates", "Mažu stažené aktualizace"));
    sys::ps(
        r#"
Stop-Service wuauserv, bits, dosvc -Force -ErrorAction SilentlyContinue
Remove-Item "$env:SystemRoot\SoftwareDistribution\Download\*" -Recurse -Force -ErrorAction SilentlyContinue
Start-Service bits, wuauserv -ErrorAction SilentlyContinue
"#,
    )?;
    let left = std::fs::read_dir(format!(r"{}\SoftwareDistribution\Download", sys::env("SystemRoot", r"C:\Windows"))).map(|d| d.count()).unwrap_or(0);
    if left > 0 {
        sys::log(&format!("SoftwareDistribution\\Download: {left} items could not be deleted (in use)"));
    }
    let freed = free_c().saturating_sub(before);
    Ok(format!("{} {:.1} GB", tr("Freed", "Uvolněno"), freed as f64 / 1e9))
}

// ---------------------------------------------------------------- display

/// Every monitor at the highest refresh rate of its resolution, and the scaling from the backup.
pub fn display(c: &Ctx) -> Res {
    use std::mem::{size_of, zeroed};
    use windows_sys::Win32::Graphics::Gdi::{
        CDS_UPDATEREGISTRY, ChangeDisplaySettingsExW, DEVMODEW, DISP_CHANGE_SUCCESSFUL, DISPLAY_DEVICE_ATTACHED_TO_DESKTOP, DISPLAY_DEVICEW,
        DM_DISPLAYFREQUENCY, ENUM_CURRENT_SETTINGS, EnumDisplayDevicesW, EnumDisplaySettingsW,
    };
    let mut done = Vec::new();
    for i in 0.. {
        let mut dd: DISPLAY_DEVICEW = unsafe { zeroed() };
        dd.cb = size_of::<DISPLAY_DEVICEW>() as u32;
        if unsafe { EnumDisplayDevicesW(std::ptr::null(), i, &mut dd, 0) } == 0 {
            break;
        }
        if dd.StateFlags & DISPLAY_DEVICE_ATTACHED_TO_DESKTOP == 0 {
            continue;
        }
        let name = dd.DeviceName;
        let mut cur: DEVMODEW = unsafe { zeroed() };
        cur.dmSize = size_of::<DEVMODEW>() as u16;
        if unsafe { EnumDisplaySettingsW(name.as_ptr(), ENUM_CURRENT_SETTINGS, &mut cur) } == 0 {
            continue;
        }
        let mut best = cur.dmDisplayFrequency;
        for j in 0.. {
            let mut m: DEVMODEW = unsafe { zeroed() };
            m.dmSize = size_of::<DEVMODEW>() as u16;
            if unsafe { EnumDisplaySettingsW(name.as_ptr(), j, &mut m) } == 0 {
                break;
            }
            if m.dmPelsWidth == cur.dmPelsWidth && m.dmPelsHeight == cur.dmPelsHeight && m.dmDisplayFrequency > best {
                best = m.dmDisplayFrequency;
            }
        }
        if best > cur.dmDisplayFrequency {
            c.log(format!("{}×{} → {best} Hz", cur.dmPelsWidth, cur.dmPelsHeight));
            cur.dmDisplayFrequency = best;
            cur.dmFields = DM_DISPLAYFREQUENCY;
            let r = unsafe { ChangeDisplaySettingsExW(name.as_ptr(), &cur, std::ptr::null_mut(), CDS_UPDATEREGISTRY, std::ptr::null()) };
            if r != DISP_CHANGE_SUCCESSFUL {
                return Err(format!("{} {best} Hz ({r})", tr("The monitor did not accept", "Monitor nepřijal")));
            }
        }
        done.push(format!("{}×{} @ {best} Hz", cur.dmPelsWidth, cur.dmPelsHeight));
    }
    // the scaling (Settings → Display → Scale) per monitor; Windows applies it at the next sign-in
    if config::has("display-scaling.reg") {
        sys::reg_import(&config::text("display-scaling.reg", true))?;
        c.reboot();
    }
    Ok(done.join(", "))
}

// ---------------------------------------------------------------- apps

pub fn calculator(_c: &Ctx) -> Res {
    sys::ps(
        "try { Get-AppxPackage -AllUsers -Name Microsoft.WindowsCalculator | Remove-AppxPackage -AllUsers -ErrorAction Stop } catch { \"user: $_\" }; \
         try { Get-AppxProvisionedPackage -Online | Where-Object DisplayName -eq 'Microsoft.WindowsCalculator' | \
         Remove-AppxProvisionedPackage -Online -ErrorAction Stop | Out-Null } catch { \"provisioned: $_\" }",
    )?;
    Ok(String::new())
}

// ---------------------------------------------------------------- mouse

/// The settings of the mouse the backup takes along.
pub struct Mouse {
    /// pointer speed 1–20 (10 = the default of Windows)
    pub speed: u32,
    /// lines a wheel notch scrolls (u32::MAX = a page)
    pub lines: u32,
    /// characters a sideways notch scrolls
    pub chars: u32,
    /// the double-click time in ms
    pub double: u32,
}

impl Mouse {
    pub fn now() -> Mouse {
        use windows_sys::Win32::UI::WindowsAndMessaging::{SPI_GETMOUSESPEED, SPI_GETWHEELSCROLLCHARS, SPI_GETWHEELSCROLLLINES, SystemParametersInfoW};
        let get = |action, default: u32| {
            let mut v = 0u32;
            if unsafe { SystemParametersInfoW(action, 0, (&mut v as *mut u32).cast(), 0) } != 0 { v } else { default }
        };
        let double = unsafe { windows_sys::Win32::UI::Input::KeyboardAndMouse::GetDoubleClickTime() };
        Mouse { speed: get(SPI_GETMOUSESPEED, 10), lines: get(SPI_GETWHEELSCROLLLINES, 3), chars: get(SPI_GETWHEELSCROLLCHARS, 3), double }
    }

    pub fn to_text(&self) -> String {
        format!("speed={}\r\nlines={}\r\nchars={}\r\ndouble={}\r\n", self.speed, self.lines, self.chars, self.double)
    }

    fn from_text(t: &str) -> Option<Mouse> {
        let get = |name: &str| t.lines().find_map(|l| l.trim().strip_prefix(&format!("{name}=")).and_then(|v| v.trim().parse::<u32>().ok()));
        Some(Mouse { speed: get("speed")?.clamp(1, 20), lines: get("lines").unwrap_or(3), chars: get("chars").unwrap_or(3), double: get("double").unwrap_or(500) })
    }
}

/// The step "Mouse": the pointer speed, scrolling and double-click time from the backup (saved for the user, at once).
pub fn mouse(_c: &Ctx) -> Res {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SPI_SETDOUBLECLICKTIME, SPI_SETMOUSESPEED, SPI_SETWHEELSCROLLCHARS, SPI_SETWHEELSCROLLLINES, SPIF_SENDCHANGE, SPIF_UPDATEINIFILE, SystemParametersInfoW,
    };
    let Some(m) = Mouse::from_text(&config::text("mouse.txt", false)) else {
        return Ok("Not in the backup".into());
    };
    let save = SPIF_UPDATEINIFILE | SPIF_SENDCHANGE;
    unsafe {
        // the speed goes in pvParam itself, the others in uiParam
        SystemParametersInfoW(SPI_SETMOUSESPEED, 0, m.speed as usize as _, save);
        SystemParametersInfoW(SPI_SETWHEELSCROLLLINES, m.lines, std::ptr::null_mut(), save);
        SystemParametersInfoW(SPI_SETWHEELSCROLLCHARS, m.chars, std::ptr::null_mut(), save);
        SystemParametersInfoW(SPI_SETDOUBLECLICKTIME, m.double, std::ptr::null_mut(), save);
    }
    Ok(format!("speed {} of 20, {} lines per notch", m.speed, m.lines))
}

// ---------------------------------------------------------------- the name of the PC

/// The name the PC gets: chosen in the wizard of the backup (pc-name.txt); empty = it keeps its name.
fn pc_name() -> String {
    config::text("pc-name.txt", false).trim().to_string()
}

pub fn rename_pc(c: &Ctx) -> Res {
    use windows_sys::Win32::System::SystemInformation::{ComputerNamePhysicalDnsHostname, SetComputerNameExW};
    let name = pc_name();
    let name = name.as_str();
    if name.is_empty() {
        return Ok(tr("Skipped – no name chosen", "Přeskočeno – není zvolený název").into());
    }
    if sys::env("COMPUTERNAME", "").eq_ignore_ascii_case(name) {
        return Ok(format!("{} {name}", tr("The PC is already called", "PC se už jmenuje")));
    }
    let n = sys::wide(name);
    if unsafe { SetComputerNameExW(ComputerNamePhysicalDnsHostname, n.as_ptr()) } == 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    c.reboot();
    Ok(format!("{name} {}", tr("after a restart", "po restartu")))
}

// ---------------------------------------------------------------- Wi-Fi and fonts from the backup

/// The Wi-Fi networks of the backup, with their passwords, for all users of the PC.
pub fn wifi(c: &Ctx) -> Res {
    let dir = config::dir().join("wifi");
    let files: Vec<std::path::PathBuf> = std::fs::read_dir(&dir)
        .map(|r| r.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("xml"))).collect())
        .unwrap_or_default();
    if files.is_empty() {
        return Ok(config::not_backed_up());
    }
    c.log("Wi-Fi networks");
    let mut n = 0;
    let mut last = String::new();
    for f in &files {
        match sys::exec("netsh.exe", &["wlan", "add", "profile", &format!("filename={}", f.display()), "user=all"]) {
            Ok(o) if o.code == 0 => n += 1,
            Ok(o) => last = o.text.trim().lines().last().unwrap_or("").to_string(),
            Err(e) => last = e,
        }
    }
    if n == 0 {
        // a PC without Wi-Fi (or without its driver yet) has no Wi-Fi service: nothing to do
        return Ok(format!("Not added – {last}"));
    }
    Ok(format!("{n} of {}", files.len()))
}

/// The fonts of the backup into Windows\Fonts, registered for all users.
pub fn fonts(c: &Ctx) -> Res {
    use windows_sys::Win32::Graphics::Gdi::AddFontResourceW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{HWND_BROADCAST, SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_FONTCHANGE};
    let dir = config::dir().join("fonts");
    let Ok(list) = std::fs::read_to_string(dir.join("fonts.txt")) else { return Ok(config::not_backed_up()) };
    c.log("Fonts");
    let win_fonts = std::path::PathBuf::from(sys::env("SystemRoot", r"C:\Windows")).join("Fonts");
    let key = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Fonts";
    let (mut n, mut failed) = (0, Vec::new());
    for line in list.lines().filter(|l| !l.trim().is_empty()) {
        let Some((name, file)) = line.split_once('|') else { continue };
        let to = win_fonts.join(file);
        if !to.exists() {
            if let Err(e) = std::fs::copy(dir.join(file), &to) {
                failed.push(format!("{file}: {e}"));
                continue;
            }
        }
        if let Err(e) = sys::set_sz(Root::Hklm, key, name, file) {
            failed.push(e);
            continue;
        }
        let w = sys::wide(&to.display().to_string());
        unsafe { AddFontResourceW(w.as_ptr()) };
        n += 1;
    }
    unsafe { SendMessageTimeoutW(HWND_BROADCAST, WM_FONTCHANGE, 0, 0, SMTO_ABORTIFHUNG, 2000, std::ptr::null_mut()) };
    let text = format!("{n} fonts");
    if failed.is_empty() { Ok(text) } else { Err(format!("{text}\n{}", failed.join("\n"))) }
}
