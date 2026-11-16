//! Drivers: Windows Update must not install drivers (nor apps such as Razer Synapse that come with them);
//! the AMD chipset and graphics drivers come straight from amd.com, then the settings of the author.

use crate::config;
use crate::i18n::tr;
use crate::programs::installed_version;
use crate::steps::{Ctx, Res};
use crate::sys::{self, Root};
use std::path::PathBuf;

/// One driver for all AM4 / AM5 chipsets and one for all current Radeon cards – these pages list the newest
/// (only for Check links: the drivers themselves are installed offline from the installers next to the exe).
const CHIPSET_PAGE: &str = "https://www.amd.com/en/support/downloads/drivers.html/chipsets/am4/b550.html";
const GPU_PAGE: &str =
    "https://www.amd.com/en/support/downloads/drivers.html/graphics/radeon-rx/radeon-rx-9000-series/amd-radeon-rx-9070-xt.html";
const GPU_CLASS: &str = r"SYSTEM\CurrentControlSet\Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}";

pub fn no_driver_updates(_c: &Ctx) -> Res {
    sys::set_dword(Root::Hklm, r"SOFTWARE\Policies\Microsoft\Windows\WindowsUpdate", "ExcludeWUDriversInQualityUpdate", 1)?;
    sys::set_dword(Root::Hklm, r"SOFTWARE\Microsoft\Windows\CurrentVersion\DriverSearching", "SearchOrderConfig", 0)?;
    sys::set_dword(Root::Hklm, r"SOFTWARE\Policies\Microsoft\Windows\DriverSearching", "SearchOrderConfig", 0)?;
    sys::set_dword(Root::Hklm, r"SOFTWARE\Policies\Microsoft\Windows\Device Metadata", "PreventDeviceMetadataFromNetwork", 1)?;
    sys::set_dword(Root::Hklm, r"SOFTWARE\Microsoft\Windows\CurrentVersion\Device Metadata", "PreventDeviceMetadataFromNetwork", 1)?;
    // co-installers are what installs Razer Synapse (and similar) when a device is plugged in
    sys::set_dword(Root::Hklm, r"SOFTWARE\Microsoft\Windows\CurrentVersion\Device Installer", "DisableCoInstallers", 1)?;
    Ok(tr("Drivers and device apps (Razer Synapse…) no longer come from Windows Update", "Ovladače a aplikace zařízení (Razer Synapse…) už nepřijdou z Windows Update").into())
}

fn gpu_name() -> String {
    sys::ps("(Get-CimInstance Win32_VideoController | ForEach-Object Name) -join ', '").unwrap_or_default().trim().to_string()
}

pub fn url_chipset() -> Result<String, String> {
    let html = sys::fetch(CHIPSET_PAGE, None)?;
    sys::links(&html, "amd_chipset_software_", ".exe").into_iter().next().ok_or_else(|| not_found(tr("chipset driver", "ovladač čipsetu")))
}

pub fn url_gpu() -> Result<String, String> {
    let html = sys::fetch(GPU_PAGE, None)?;
    sys::links(&html, "whql-amd-software-adrenalin-edition-", ".exe")
        .into_iter()
        .next()
        .ok_or_else(|| not_found(tr("graphics driver", "ovladač grafiky")))
}

fn not_found(what: &str) -> String {
    format!("AMD {what}: {}", tr("the download link was not found on amd.com", "odkaz ke stažení nebyl na amd.com nalezen"))
}

// ---------------------------------------------------------------- installers next to HeMimizer.exe (offline)

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind {
    Chipset,
    Graphics,
}

pub struct Local {
    pub kind: Kind,
    pub path: PathBuf,
    pub version: String,
}

/// The digits-and-dots part after `after` in a file name ("…edition-26.10.2-win11-b.exe" → 26.10.2).
fn version_after(name: &str, after: &str) -> Option<String> {
    let i = name.to_lowercase().find(after)? + after.len();
    let v: String = name[i..].chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
    let v = v.trim_end_matches('.').to_string();
    (!v.is_empty()).then_some(v)
}

/// The AMD installers lying next to HeMimizer.exe, whatever their names: told apart by the product name written
/// in the file ("AMD Chipset Software" / "AMD Software: Adrenalin Edition"), the file name only as a fallback.
/// The small web installer of Adrenalin (minimal setup) needs the internet, so it is not taken.
pub fn local_installers() -> Vec<Local> {
    let Some(dir) = crate::config::dir().parent().map(PathBuf::from) else { return Vec::new() };
    let own = std::env::current_exe().ok().and_then(|e| e.file_name().map(|n| n.to_string_lossy().to_lowercase())).unwrap_or_default();
    // next to the exe, and among the installers the backup downloaded
    let dirs: Vec<String> = [crate::config::exe_dir(), dir, crate::backup::installers_dir()]
        .iter()
        .filter(|d| d.is_dir())
        .map(|d| format!("'{}'", d.display().to_string().replace('\'', "''")))
        .collect();
    if dirs.is_empty() {
        return Vec::new();
    }
    let list = sys::ps(&format!(
        "Get-ChildItem -LiteralPath {} -Filter *.exe -File | ForEach-Object {{ $v = $_.VersionInfo; \
         '{{0}}|{{1}}|{{2}}|{{3}}|{{4}}' -f $_.FullName, $v.ProductName, $v.FileDescription, $v.FileVersion, $_.Length }}",
        dirs.join(",")
    ))
    .unwrap_or_default();
    let mut out = Vec::new();
    for line in list.lines() {
        let p: Vec<&str> = line.trim().split('|').collect();
        if p.len() < 5 {
            continue;
        }
        let path = PathBuf::from(p[0]);
        let file = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if file.to_lowercase() == own {
            continue;
        }
        let text = format!("{} {} {}", p[1], p[2], file).to_lowercase();
        let size: u64 = p[4].parse().unwrap_or(0);
        let kind = if text.contains("chipset") {
            Kind::Chipset
        } else if (text.contains("adrenalin") || text.contains("radeon")) && !text.contains("minimal") && size > 100_000_000 {
            Kind::Graphics
        } else {
            continue;
        };
        let version = match kind {
            Kind::Chipset => version_after(&file, "chipset_software_"),
            Kind::Graphics => version_after(&file, "edition-"),
        }
        .unwrap_or_else(|| p[3].trim().to_string());
        sys::log(&format!("local installer: {kind:?} {} (version {version})", path.display()));
        out.push(Local { kind, path, version });
    }
    out
}

/// The installers chosen on the start page (None = the user cleared the field: the step is skipped).
static CHOSEN: std::sync::Mutex<Vec<(Kind, Option<PathBuf>)>> = std::sync::Mutex::new(Vec::new());

pub fn set_chosen(kind: Kind, path: Option<PathBuf>) {
    let mut c = CHOSEN.lock().unwrap();
    c.retain(|(k, _)| *k != kind);
    c.push((kind, path));
}

/// The newest installer of this kind next to the exe (for the start page).
pub fn found(kind: Kind) -> Option<PathBuf> {
    local_installers().into_iter().filter(|l| l.kind == kind).max_by_key(|l| l.version.split('.').map(|x| x.parse::<u32>().unwrap_or(0)).collect::<Vec<_>>()).map(|l| l.path)
}

fn local(kind: Kind) -> Option<Local> {
    let chosen = CHOSEN.lock().unwrap().iter().find(|(k, _)| *k == kind).map(|(_, p)| p.clone());
    let path = match chosen {
        Some(p) => p.filter(|p| p.exists())?,
        None => found(kind)?,
    };
    let file = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let version = match kind {
        Kind::Chipset => version_after(&file, "chipset_software_"),
        Kind::Graphics => version_after(&file, "edition-"),
    }
    .or_else(|| {
        sys::ps(&format!("(Get-Item -LiteralPath '{}').VersionInfo.FileVersion", path.display().to_string().replace('\'', "''")))
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
    })
    .unwrap_or_default();
    sys::log(&format!("installer for {kind:?}: {} (version {version})", path.display()));
    Some(Local { kind, path, version })
}

// ---------------------------------------------------------------- the internet

/// True when a server on the internet answers (no command is run, so the log is not filled while waiting).
pub fn online() -> bool {
    use std::net::{TcpStream, ToSocketAddrs};
    ("www.msftconnecttest.com", 80)
        .to_socket_addrs()
        .ok()
        .and_then(|mut a| a.next())
        .is_some_and(|a| TcpStream::connect_timeout(&a, std::time::Duration::from_secs(4)).is_ok())
}

/// Waits until the PC is connected to the internet; the window asks the user to connect it meanwhile.
pub fn ensure_internet(c: &Ctx) {
    if online() {
        return;
    }
    sys::log("offline – waiting for the internet");
    c.waiting_for_internet(true);
    while !online() {
        std::thread::sleep(std::time::Duration::from_secs(3));
    }
    sys::log("online");
    c.waiting_for_internet(false);
}

pub fn internet(c: &Ctx) -> Res {
    let was = online();
    ensure_internet(c);
    Ok(if was { tr("Connected", "Připojeno") } else { tr("Connected – continuing", "Připojeno – pokračuji") }.into())
}

// ---------------------------------------------------------------- the drivers

/// The step says it, the end of the run lists it, and HeMimizer goes on (nothing is downloaded).
fn not_next_to_exe(c: &Ctx, what: &str) -> Res {
    let dir = crate::config::dir().parent().map(|d| d.display().to_string()).unwrap_or_default();
    // c.note writes it to the log too
    let _ = dir;
    c.note(format!("{what}: {}", tr("no installer was chosen – install it by hand", "nebyla vybrána instalačka – nainstaluj ho ručně")));
    Ok(tr("Installer not found – skipped", "Instalačka nenalezena – přeskočeno").into())
}

pub fn chipset(c: &Ctx) -> Res {
    if !crate::install::chosen("amd-chipset") {
        return Ok(crate::install::not_chosen());
    }
    let Some(l) = local(Kind::Chipset) else { return not_next_to_exe(c, tr("AMD chipset driver", "Ovladač čipsetu AMD")) };
    let cpu = sys::ps("(Get-CimInstance Win32_Processor).Name").unwrap_or_default();
    if !cpu.contains("AMD") {
        return Ok(tr("Skipped – the processor is not AMD", "Přeskočeno – procesor není AMD").into());
    }
    let version = l.version.clone();
    if installed_version("AMD Chipset Software").as_deref() == Some(version.as_str()) {
        return Ok(format!("{version} – {}", tr("already installed", "už je nainstalovaný")));
    }
    c.log(format!("{} {} ({version})", tr("Installing", "Instaluji"), l.path.file_name().unwrap().to_string_lossy()));
    sys::run(&l.path.display().to_string(), &["/S"])?;
    c.reboot();
    Ok(version)
}

/// Unpacks a 7-Zip archive or self-extracting exe: with tar.exe of Windows (libarchive reads 7z, also inside an
/// exe – checked 2026-11-10), or with 7-Zip when it is there.
fn unpack(file: &std::path::Path, dir: &std::path::Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let tar = sys::run("tar.exe", &["-xf", &file.display().to_string(), "-C", &dir.display().to_string()]);
    if tar.is_ok() && dir.join("Setup.exe").exists() {
        return Ok(());
    }
    let seven = format!(r"{}\7-Zip\7z.exe", sys::env("ProgramFiles", r"C:\Program Files"));
    if std::path::Path::new(&seven).exists() {
        sys::run(&seven, &["x", &file.display().to_string(), &format!("-o{}", dir.display()), "-y"])?;
        return Ok(());
    }
    tar.and_then(|_| Err(tr("Setup.exe is not in the unpacked driver.", "V rozbaleném ovladači není Setup.exe.").into()))
}

pub fn gpu(c: &Ctx) -> Res {
    if !crate::install::chosen("amd-gpu") {
        return Ok(crate::install::not_chosen());
    }
    let Some(l) = local(Kind::Graphics) else { return not_next_to_exe(c, tr("AMD graphics driver", "Ovladač grafiky AMD")) };
    if !gpu_name().contains("Radeon") {
        return Ok(tr("Skipped – no AMD Radeon in the PC", "Přeskočeno – v PC není AMD Radeon").into());
    }
    let version = l.version.clone();
    if installed_version("AMD Software").as_deref() == Some(version.as_str()) {
        return Ok(format!("Adrenalin {version} – {}", tr("already installed", "už je nainstalovaný")));
    }
    let dir = sys::work_dir().join("AMD-Adrenalin");
    let _ = std::fs::remove_dir_all(&dir);
    c.log(format!("{} {}", tr("Unpacking", "Rozbaluji"), l.path.file_name().unwrap().to_string_lossy()));
    unpack(&l.path, &dir)?;
    c.log(tr("Installing the driver – the screen may go black for a moment", "Instaluji ovladač – obrazovka může na chvíli zčernat"));
    let setup = dir.join("Setup.exe");
    let r = sys::run(&setup.display().to_string(), &["-INSTALL"]);
    let _ = std::fs::remove_dir_all(&dir);
    r?;
    c.reboot();
    Ok(format!("Adrenalin {version}"))
}

/// The class key of the Radeon (…\Class\{display}\000N).
pub fn gpu_key() -> Option<String> {
    (0..16).map(|i| format!(r"{GPU_CLASS}\{i:04}")).find(|k| sys::get_sz(Root::Hklm, k, "DriverDesc").is_some_and(|d| d.contains("Radeon")))
}

/// The graphics settings of Adrenalin, its own settings and the tuning (OC) profile.
pub fn gpu_settings(c: &Ctx) -> Res {
    let Some(key) = gpu_key() else {
        return Ok(tr("Skipped – the AMD driver is not installed", "Přeskočeno – ovladač AMD není nainstalován").into());
    };
    if !config::has("amd-umd.reg") && !config::has("amd-user.reg") && !config::has("OC.xml") {
        return Ok(config::not_backed_up());
    }
    c.log(tr("Applying the graphics settings", "Nastavuji grafiku"));
    if config::has("amd-umd.reg") {
        sys::reg_import(&config::text("amd-umd.reg", true).replace("{GPU}", &format!(r"HKEY_LOCAL_MACHINE\{key}")))?;
    }
    if config::has("amd-user.reg") {
        sys::reg_import(&config::text("amd-user.reg", true))?;
    }
    if !config::has("OC.xml") {
        return Ok(tr("Graphics settings applied", "Nastavení grafiky použito").into());
    }
    // Adrenalin has no way to import a tuning profile from outside – it is saved for Import in Adrenalin
    let dir: PathBuf = sys::documents().join("HeMimizer");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let oc = dir.join("OC.xml");
    std::fs::write(&oc, config::bytes("OC.xml")).map_err(|e| e.to_string())?;
    c.note(format!(
        "{} {}",
        tr(
            "Import the OC profile in AMD Software: Performance → Tuning → Import:",
            "OC profil načti v AMD Software: Výkon → Ladění → Importovat:"
        ),
        oc.display()
    ));
    Ok(tr("Graphics settings applied, OC profile saved", "Nastavení grafiky použito, OC profil uložen").into())
}
