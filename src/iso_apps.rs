//! The apps and drivers built into a custom ISO, the way the ISO of a PC maker has them: everything is downloaded while
//! the ISO is made and installed quietly while Windows sets itself up – nothing is downloaded or clicked through after it.
//! The script is started three ways, the first one that comes runs it (it removes itself, so it never runs twice):
//! 1. Windows\Panther\unattend.xml of the image (the specialize pass, RunSynchronous, as SYSTEM) – it works with an OEM
//!    product key ("can be used with OEM product keys", Microsoft), which notebooks with Windows from their maker have in
//!    the firmware;
//! 2. SetupComplete.cmd (as SYSTEM, at the end of the setup) – for when an autounattend.xml of the setup media (Rufus…)
//!    replaced that unattend.xml; Windows skips it with an OEM key ("disabled when using OEM product keys");
//! 3. the autounattend.xml of a USB stick made by HeMiMizer runs it in its specialize pass too (usb.rs).
//!
//! - winget packages: `winget download` gives the installer and its manifest (the type of the installer and its quiet
//!   switches); app packages (MSIX) are provisioned into the image straight away.
//! - the installers of a folder: MSI, Inno Setup, NSIS and WiX bundles are told apart by their content; other .exe files
//!   have no known quiet switches and are refused.
//! - drivers: NVIDIA GeForce (NVIDIA's driver lookup), AMD Radeon and AMD chipset (amd.com); each is installed only when
//!   the PC has that hardware. Intel lets no program download its drivers (its site refuses them), so Intel Driver &
//!   Support Assistant (winget) is offered instead.

use crate::sys;
use std::path::{Path, PathBuf};

/// The most used apps, offered as ticks: (winget id, name). All have an installer for all users.
pub const POPULAR: &[(&str, &str)] = &[
    ("7zip.7zip", "7-Zip"),
    ("VideoLAN.VLC", "VLC media player"),
    ("Google.Chrome", "Google Chrome"),
    ("Mozilla.Firefox", "Mozilla Firefox"),
    ("Brave.Brave", "Brave"),
    ("Notepad++.Notepad++", "Notepad++"),
    ("Adobe.Acrobat.Reader.64-bit", "Adobe Acrobat Reader"),
    ("TheDocumentFoundation.LibreOffice", "LibreOffice"),
    ("Valve.Steam", "Steam"),
    ("Microsoft.PowerToys", "PowerToys"),
    ("voidtools.Everything", "Everything"),
    ("Microsoft.VCRedist.2015+.x64", "Visual C++ 2015-2022"),
];

/// The drivers offered: (id, name, detail).
pub const DRIVERS: &[(&str, &str, &str)] = &[
    ("nvidia", "NVIDIA GeForce", "The newest Game Ready driver for GeForce cards in desktop PCs (about 1 GB)"),
    ("amd-gpu", "AMD Radeon", "The newest AMD Software: Adrenalin Edition for Radeon RX cards (about 900 MB)"),
    ("amd-chipset", "AMD chipset", "For Ryzen PCs (AM4 / AM5)"),
    ("intel", "Intel Driver & Support Assistant", "Intel's site lets no program download its drivers – this app of Intel finds them after the first sign-in"),
];

/// The apps of the makers of PCs, notebooks and motherboards, and of mice, keyboards and headsets: (winget id, name,
/// detail, the PowerShell condition of the hardware it is for). Each is installed only on a PC with that hardware: the
/// maker of the PC or its board (`$maker`), a device of that maker plugged in (`$usb`: the ids of all devices).
/// Not here: Lenovo Vantage, Dell SupportAssist and MyASUS are only in the Microsoft Store, which winget cannot download
/// from; Razer Synapse has an installer of its own with no quiet switches; MSI Center fails winget's own hash check
/// (checked 2026-11-27).
pub const VENDOR: &[(&str, &str, &str, &str)] = &[
    ("HP.SupportAssistant", "HP Support Assistant", "HP notebooks and PCs", "$maker -match '\\bHP\\b|Hewlett'"),
    ("Lenovo.SystemUpdate", "Lenovo System Update", "Lenovo notebooks and PCs: drivers and BIOS (Vantage is Store-only)", "$maker -match 'Lenovo'"),
    ("Dell.CommandUpdate", "Dell Command | Update", "Dell notebooks and PCs: drivers and BIOS (SupportAssist is Store-only)", "$maker -match 'Dell'"),
    ("Asus.ArmouryCrate", "ASUS Armoury Crate", "ASUS notebooks and motherboards – it downloads its parts, so the PC needs the internet (a cable) while Windows sets itself up", "$maker -match 'ASUS'"),
    ("Logitech.GHUB", "Logitech G HUB", "Logitech G mice, keyboards and headsets – it downloads its parts, so the PC needs the internet (a cable) while Windows sets itself up", "$usb -match 'VID_046D'"),
    ("Corsair.iCUE.5", "Corsair iCUE 5", "Corsair mice, keyboards, headsets and RGB – it downloads its parts, so the PC needs the internet (a cable) while Windows sets itself up", "$usb -match 'VID_1B1C'"),
    ("SteelSeries.GG", "SteelSeries GG", "SteelSeries mice, keyboards and headsets", "$usb -match 'VID_1038'"),
];

/// The hardware condition of an app of [`VENDOR`] (None for the others).
pub fn hardware_check(id: &str) -> Option<&'static str> {
    VENDOR.iter().find(|v| v.0.eq_ignore_ascii_case(id)).map(|v| v.3)
}

/// The winget package behind the Intel choice.
pub const INTEL_DSA: &str = "Intel.IntelDriverAndSupportAssistant";

/// The folder of the apps in the image and in the work folder (in Windows\Setup\Scripts).
pub const DIR: &str = "HeMiMizer-apps";
/// The script that installs them, next to it.
pub const SCRIPT: &str = "HeMiMizer-apps.ps1";

/// Everything downloaded and ready to go into each edition.
#[derive(Default)]
pub struct Prepared {
    /// the folder with the installers (copied into the image as Windows\Setup\Scripts\HeMiMizer-apps)
    pub dir: PathBuf,
    /// the lines of the script that install them, in order
    lines: Vec<String>,
    /// app packages provisioned into the image: (package, its dependencies)
    pub appx: Vec<(PathBuf, Vec<PathBuf>)>,
    /// the names, for the result
    pub names: Vec<String>,
    /// the processor of the image ("x64", "arm64")
    pub arch: String,
}

impl Prepared {
    /// The installing part of the script exists.
    pub fn has_script(&self) -> bool {
        !self.lines.is_empty()
    }
}

/// What a quiet installation needs, by the type of the installer.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Type {
    Msi,
    Inno,
    Nullsoft,
    Burn,
    /// an .exe with its own switches (from the manifest)
    Exe,
    Appx,
}

/// The type of a winget installer ("wix", "inno"…) – None for those that cannot be installed quietly here (zip,
/// portable, a web app…).
fn type_of(name: &str) -> Option<Type> {
    Some(match name.to_lowercase().as_str() {
        "msi" | "wix" => Type::Msi,
        "inno" => Type::Inno,
        "nullsoft" => Type::Nullsoft,
        "burn" => Type::Burn,
        "exe" => Type::Exe,
        "msix" | "appx" => Type::Appx,
        _ => return None,
    })
}

/// The quiet switches when the manifest names none.
fn default_quiet(t: Type) -> Option<&'static str> {
    match t {
        Type::Msi => Some("/qn /norestart"),
        Type::Inno => Some("/VERYSILENT /SUPPRESSMSGBOXES /NORESTART /SP-"),
        Type::Nullsoft => Some("/S"),
        Type::Burn => Some("/quiet /norestart"),
        Type::Exe | Type::Appx => None,
    }
}

/// What a manifest of `winget download` says.
#[derive(Default, Debug, PartialEq)]
struct Manifest {
    kind: Option<String>,
    silent: Option<String>,
    custom: Option<String>,
    /// a zip: the type of the installer in it and its path inside
    nested_kind: Option<String>,
    nested_file: Option<String>,
}

fn read_manifest(yaml: &str) -> Manifest {
    let value = |key: &str| {
        yaml.lines()
            .map(|l| l.trim().trim_start_matches("- "))
            .find_map(|l| l.strip_prefix(key))
            .map(|v| v.trim().trim_matches(['\'', '"']).to_string())
            .filter(|v| !v.is_empty())
    };
    Manifest {
        kind: value("InstallerType:"),
        silent: value("Silent:"),
        custom: value("Custom:"),
        nested_kind: value("NestedInstallerType:"),
        nested_file: value("RelativeFilePath:"),
    }
}

/// The type of an installer from a folder, by its content.
fn type_of_file(file: &Path) -> Option<Type> {
    let ext = file.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    if ext == "msi" {
        return Some(Type::Msi);
    }
    if ext != "exe" {
        return None;
    }
    let bytes = std::fs::read(file).ok()?;
    let has = |s: &[u8]| bytes.windows(s.len()).any(|w| w == s);
    if has(b"Inno Setup") {
        Some(Type::Inno)
    } else if has(b"Nullsoft") {
        Some(Type::Nullsoft)
    } else if has(b".wixburn") {
        Some(Type::Burn)
    } else {
        None
    }
}

/// A PowerShell string in single quotes.
fn q(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// The line of the script that installs a file of the apps folder (`rel` inside it) quietly.
fn install_line(title: &str, rel: &str, t: Type, quiet: &str, custom: Option<&str>) -> String {
    let args = match custom {
        Some(c) => format!("{quiet} {c}"),
        None => quiet.to_string(),
    };
    format!("Install {} {} {} {}", q(title), q(rel), q(&args), q(if t == Type::Msi { "msi" } else { "exe" }))
}

/// The processors tried for an image, best first: Windows on Arm runs x64 and x86 apps too, x64 Windows x86 ones;
/// then "neutral" (a zip of ASUS…) and, last, none named (some manifests are found only so – HP Support Assistant).
fn archs(arch: &str) -> &'static [&'static str] {
    if arch == "arm64" { &["arm64", "x64", "x86", "neutral", ""] } else { &["x64", "x86", "neutral", ""] }
}

/// winget download into an empty `dir` for that processor (for all users, or any scope).
fn winget_download(id: &str, arch: &str, machine: bool, dir: &Path) -> Result<(), String> {
    let d = dir.display().to_string();
    let _ = std::fs::remove_dir_all(dir);
    std::fs::create_dir_all(dir).map_err(|e| format!("{d}: {e}"))?;
    let mut args = vec![
        "download",
        "--id",
        id,
        "--exact",
        "--source",
        "winget",
        "--download-directory",
        &d,
        "--accept-source-agreements",
        "--accept-package-agreements",
        "--disable-interactivity",
    ];
    if !arch.is_empty() {
        args.extend(["--architecture", arch]);
    }
    if machine {
        args.extend(["--scope", "machine"]);
    }
    let o = sys::exec("winget.exe", &args)?;
    if o.code == 0 {
        return Ok(());
    }
    let last = o.text.lines().map(str::trim).rfind(|l| !l.is_empty() && !l.chars().all(|c| "-\\|/█▒ ".contains(c))).unwrap_or("").to_string();
    Err(format!("{id}: winget download – {last}"))
}

/// The files of a folder (and its subfolders) whose extension is one of `exts`.
fn files_in(dir: &Path, exts: &[&str]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(files_in(&p, exts));
        } else if p.extension().is_some_and(|x| exts.iter().any(|e| x.eq_ignore_ascii_case(e))) {
            out.push(p);
        }
    }
    out.sort();
    out
}

/// One winget package downloaded into `dir` (`rel` = its name inside the apps folder) and turned into a line of the
/// script or an app package.
/// Tries the processors and scopes in turn and takes the first installer that can be installed quietly.
fn winget_app(id: &str, arch: &str, dir: &Path, rel: &str, p: &mut Prepared) -> Result<(), String> {
    let mut last = String::new();
    for a in archs(arch) {
        for machine in [true, false] {
            match winget_download(id, a, machine, dir).and_then(|()| winget_take(id, dir, rel, p)) {
                Ok(()) => return Ok(()),
                Err(e) => last = e,
            }
        }
    }
    let _ = std::fs::remove_dir_all(dir);
    Err(last)
}

/// The package downloaded into `dir`: a line of the script, or an app package to provision.
fn winget_take(id: &str, dir: &Path, rel: &str, p: &mut Prepared) -> Result<(), String> {
    // its own manifest (next to it, not one of its Dependencies)
    let yaml = files_in(dir, &["yaml"]).into_iter().find(|y| y.parent() == Some(dir)).ok_or_else(|| format!("{id}: winget gave no manifest"))?;
    let m = read_manifest(&std::fs::read_to_string(&yaml).unwrap_or_default());
    let (silent, custom) = (m.silent.clone(), m.custom.clone());
    let mut kind_name = m.kind.clone().unwrap_or_default();
    // a zip: unpacked now (tar.exe of Windows), the installer in it is what runs
    let mut nested: Option<String> = None;
    if kind_name.eq_ignore_ascii_case("zip") {
        let (Some(nk), Some(nf)) = (m.nested_kind.clone(), m.nested_file.clone()) else {
            return Err(format!("{id}: its zip names no installer inside"));
        };
        let zip = std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).find(|f| f.extension().is_some_and(|x| x.eq_ignore_ascii_case("zip"))).ok_or_else(|| format!("{id}: winget gave no zip"))?;
        let into = dir.join("x");
        std::fs::create_dir_all(&into).map_err(|e| e.to_string())?;
        let o = sys::exec("tar.exe", &["-xf", &zip.display().to_string(), "-C", &into.display().to_string()])?;
        let _ = std::fs::remove_file(&zip);
        if o.code != 0 || !into.join(&nf).is_file() {
            return Err(format!("{id}: its zip could not be unpacked"));
        }
        kind_name = nk;
        nested = Some(format!(r"x\{}", nf.replace('/', "\\")));
    }
    let t = type_of(&kind_name).ok_or_else(|| format!("{id}: its installer ({kind_name}) cannot be installed from the image – leave it out"))?;
    let name = if id.eq_ignore_ascii_case(INTEL_DSA) {
        "Intel Driver & Support Assistant"
    } else if let Some(v) = VENDOR.iter().find(|v| v.0.eq_ignore_ascii_case(id)) {
        v.1
    } else {
        POPULAR.iter().find(|(i, _)| i.eq_ignore_ascii_case(id)).map_or(id, |(_, n)| n)
    };
    if t == Type::Appx {
        let mut pkgs = files_in(dir, &["msix", "msixbundle", "appx", "appxbundle"]);
        let main = pkgs.iter().position(|f| f.parent() == Some(dir)).ok_or_else(|| format!("{id}: winget gave no app package"))?;
        let main = pkgs.remove(main);
        p.appx.push((main, pkgs));
        p.names.push(name.to_string());
        return Ok(());
    }
    let fname = match nested {
        Some(n) => n,
        None => std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .find(|f| f.is_file() && f.extension().is_some_and(|x| x.eq_ignore_ascii_case("exe") || x.eq_ignore_ascii_case("msi")))
            .and_then(|f| f.file_name().map(|n| n.to_string_lossy().into_owned()))
            .ok_or_else(|| format!("{id}: winget gave no installer"))?,
    };
    let quiet = silent.or_else(|| default_quiet(t).map(str::to_string)).ok_or_else(|| format!("{id}: its installer has no quiet switches – leave it out"))?;
    let _ = std::fs::remove_file(&yaml);
    let line = install_line(name, &format!(r"{rel}\{fname}"), t, &quiet, custom.as_deref());
    // made for some hardware only (a maker, a board, a mouse…): installed only on a PC that has it
    p.lines.push(match hardware_check(id) {
        Some(check) => format!("if ({check}) {{ {line} }}"),
        None => line,
    });
    p.names.push(name.to_string());
    Ok(())
}

/// The newest GeForce Game Ready driver for desktop PCs (DCH, WHQL, Windows 10/11 64-bit), from NVIDIA's lookup.
fn nvidia_url() -> Result<String, String> {
    // psid/pfid of a GeForce RTX 40 card: the Game Ready driver is one package for all GeForce desktop cards
    let t = sys::fetch(
        "https://gfwsl.geforce.com/services_toolkit/services/com/nvidia/services/AjaxDriverService.php?func=DriverManualLookup&psid=127&pfid=1022&osID=57&languageCode=1033&beta=0&isWHQL=1&dltype=-1&dch=1&upCRD=0&qnf=0&sort1=0&numberOfResults=1",
        None,
    )?;
    let v: serde_json::Value = serde_json::from_str(&t).map_err(|e| format!("NVIDIA: {e}"))?;
    let url = v["IDS"][0]["downloadInfo"]["DownloadURL"].as_str().unwrap_or("");
    if url.starts_with("https://") && url.to_lowercase().ends_with(".exe") {
        Ok(url.to_string())
    } else {
        Err("NVIDIA: the download link was not found".into())
    }
}

/// Downloads a driver into `dir` (signature checked by `sys::download`); returns its file name.
fn driver_file(url: &str, dir: &Path, referer: Option<&str>) -> Result<String, String> {
    let name = url.rsplit('/').next().unwrap_or("driver.exe").split('?').next().unwrap_or("driver.exe").to_string();
    sys::download(url, &dir.join(&name), referer)?;
    Ok(name)
}

/// Downloads everything chosen into `w\HeMiMizer-apps` for an image of `arch` ("x64" / "arm64"): the drivers first
/// (they go first in the script too), then the apps of winget, then the installers of the folder.
pub fn prepare(winget: &[String], drivers: &[String], installers: &[PathBuf], arch: &str, w: &Path, log: &dyn Fn(String)) -> Result<Prepared, String> {
    let dir = w.join(DIR);
    let _ = std::fs::remove_dir_all(&dir);
    let mut p = Prepared { dir: dir.clone(), arch: arch.to_string(), ..Default::default() };
    let on = |id: &str| drivers.iter().any(|d| d == id);
    // the drivers of NVIDIA and AMD are for x64 PCs only
    if arch == "x64" {
        if on("amd-chipset") {
            log("Downloading the AMD chipset driver".into());
            let d = dir.join("amd-chipset");
            let f = driver_file(&crate::drivers::url_chipset()?, &d, Some("https://www.amd.com/"))?;
            p.lines.push(format!("if ((Get-CimInstance Win32_Processor).Name -match 'AMD') {{ {} }}", install_line("AMD chipset driver", &format!(r"amd-chipset\{f}"), Type::Nullsoft, "/S", None)));
            p.names.push("AMD chipset driver".into());
        }
        if on("nvidia") {
            log("Downloading the NVIDIA driver (about 1 GB)".into());
            let d = dir.join("nvidia");
            let f = driver_file(&nvidia_url()?, &d, None)?;
            p.lines.push(format!("if ($gpu -match 'VEN_10DE|NVIDIA') {{ Unpacked 'NVIDIA driver' {} 'setup.exe' '-s -noreboot -noeula' }}", q(&format!(r"nvidia\{f}"))));
            p.names.push("NVIDIA driver".into());
        }
        if on("amd-gpu") {
            log("Downloading the AMD Radeon driver (about 900 MB)".into());
            let d = dir.join("amd-gpu");
            let f = driver_file(&crate::drivers::url_gpu()?, &d, Some("https://www.amd.com/"))?;
            p.lines.push(format!("if ($gpu -match 'VEN_1002|Radeon') {{ Unpacked 'AMD Radeon driver' {} 'Setup.exe' '-INSTALL' }}", q(&format!(r"amd-gpu\{f}"))));
            p.names.push("AMD Radeon driver".into());
        }
    }
    let mut ids: Vec<String> = winget.to_vec();
    if on("intel") && !ids.iter().any(|i| i.eq_ignore_ascii_case(INTEL_DSA)) {
        ids.insert(0, INTEL_DSA.to_string());
    }
    for (k, id) in ids.iter().enumerate() {
        crate::iso::go_on()?;
        log(format!("Downloading the apps – {id} ({} of {})", k + 1, ids.len()));
        let rel = format!("w{}", k + 1);
        winget_app(id, arch, &dir.join(&rel), &rel, &mut p)?;
    }
    // the installers of the folder: all checked before anything is copied
    let mut bad = Vec::new();
    let typed: Vec<(PathBuf, Option<Type>)> = installers.iter().map(|f| (f.clone(), type_of_file(f))).collect();
    for (f, t) in &typed {
        if t.is_none() {
            bad.push(f.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
        }
    }
    if !bad.is_empty() {
        return Err(format!(
            "These installers have no known quiet switches, so they cannot be installed from the image: {} (use an .msi or their winget id instead)",
            bad.join(", ")
        ));
    }
    for (k, (f, t)) in typed.iter().enumerate() {
        let t = t.unwrap_or(Type::Msi);
        let rel = format!("u{}", k + 1);
        let fname = f.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        log(format!("Adding {fname}"));
        let d = dir.join(&rel);
        std::fs::create_dir_all(&d).map_err(|e| format!("{}: {e}", d.display()))?;
        std::fs::copy(f, d.join(&fname)).map_err(|e| format!("{}: {e}", f.display()))?;
        let title = f.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        p.lines.push(install_line(&title, &format!(r"{rel}\{fname}"), t, default_quiet(t).unwrap_or(""), None));
        p.names.push(title);
    }
    Ok(p)
}

/// The script run by unattend.xml in the specialize pass (Windows PowerShell 5.1, as SYSTEM; saved as UTF-8 with its mark): installs everything quietly, one
/// after another (at most 45 minutes each), writes what it did to Windows\Logs\HeMiMizer-apps.log and removes itself
/// and the installers.
pub fn script(p: &Prepared) -> String {
    let head = r#"# HeMiMizer: installs the apps and drivers built into this Windows ISO, quietly, while Windows sets itself up.
param([string]$From = '?')
$ErrorActionPreference = 'Continue'
$root = Join-Path $PSScriptRoot '@@DIR@@'
Start-Transcript -Path (Join-Path $env:SystemRoot 'Logs\HeMiMizer-apps.log') -Append | Out-Null
# before its driver a card is "Microsoft Basic Display Adapter": told apart by the vendor in its id too (PCI\VEN_10DE NVIDIA, VEN_1002 AMD)
$gpu = (Get-CimInstance Win32_VideoController | ForEach-Object { "$($_.Name) $($_.PNPDeviceID)" }) -join ', '
# the maker of the PC and of its board, and every device (USB ids of mice, keyboards...), for the apps made for them
$maker = "$((Get-CimInstance Win32_ComputerSystem).Manufacturer) $((Get-CimInstance Win32_BaseBoard).Manufacturer)"
$usb = (Get-CimInstance Win32_PnPEntity | ForEach-Object DeviceID) -join ' '
Write-Host "Started by: $From"
Write-Host "Maker: $maker"
Write-Host "Graphics: $gpu"
function Wait-Ended($p, $what) {
    if (-not $p.WaitForExit(45 * 60 * 1000)) { try { $p.Kill() } catch { }; Write-Host "   $what took too long - stopped"; return }
    Write-Host "   $what ended with code $($p.ExitCode)"
}
function Install($title, $rel, $quiet, $kind) {
    $file = Join-Path $root $rel
    Write-Host "== $title"
    if ($kind -eq 'msi') { $p = Start-Process -FilePath "$env:SystemRoot\System32\msiexec.exe" -ArgumentList "/i `"$file`" $quiet" -PassThru }
    else { $p = Start-Process -FilePath $file -ArgumentList $quiet -PassThru }
    Wait-Ended $p $title
}
function Unpacked($title, $rel, $setup, $quiet) {
    $file = Join-Path $root $rel
    $to = Join-Path $env:SystemRoot "Temp\HeMiMizer-$([IO.Path]::GetFileNameWithoutExtension($file))"
    Write-Host "== $title"
    New-Item -ItemType Directory -Force -Path $to | Out-Null
    & "$env:SystemRoot\System32\tar.exe" -xf $file -C $to 2>$null
    $exe = Join-Path $to $setup
    if (Test-Path -LiteralPath $exe) { $p = Start-Process -FilePath $exe -ArgumentList $quiet -WorkingDirectory $to -PassThru }
    else { $p = Start-Process -FilePath $file -ArgumentList $quiet -PassThru }
    Wait-Ended $p $title
    Remove-Item -LiteralPath $to -Recurse -Force -ErrorAction SilentlyContinue
}
"#;
    let tail = r#"Remove-Item -LiteralPath $root -Recurse -Force -ErrorAction SilentlyContinue
Stop-Transcript | Out-Null
Remove-Item -LiteralPath $PSCommandPath -Force -ErrorAction SilentlyContinue
"#;
    let body: String = p.lines.iter().map(|l| format!("{l}\n")).collect();
    format!("{}{body}{tail}", head.replace("@@DIR@@", DIR)).replace('\n', "\r\n")
}

/// The command that runs the script when it is there (`from` goes to its log: which way started it).
pub fn command(from: &str) -> String {
    format!(
        r"cmd.exe /c if exist %SystemRoot%\Setup\Scripts\{SCRIPT} %SystemRoot%\System32\WindowsPowerShell\v1.0\powershell.exe -NoProfile -ExecutionPolicy Bypass -File %SystemRoot%\Setup\Scripts\{SCRIPT} -From {from}"
    )
}

/// The line of SetupComplete.cmd (added to one the image may already have).
pub fn setup_complete_line() -> String {
    format!("{}\r\n", command("SetupComplete").trim_start_matches("cmd.exe /c "))
}

/// Windows\Panther\unattend.xml of the image: the script in the specialize pass (RunSynchronous, as SYSTEM, before the
/// first sign-in). The processor in it must be the one of the image.
pub fn unattend(arch: &str) -> String {
    let arch = if arch == "arm64" { "arm64" } else { "amd64" };
    let cmd = command("unattend");
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<unattend xmlns="urn:schemas-microsoft-com:unattend">
  <settings pass="specialize">
    <component name="Microsoft-Windows-Deployment" processorArchitecture="{arch}" publicKeyToken="31bf3856ad364e35" language="neutral" versionScope="nonSxS" xmlns:wcm="http://schemas.microsoft.com/WMIConfig/2002/State">
      <RunSynchronous>
        <RunSynchronousCommand wcm:action="add">
          <Order>1</Order>
          <Description>HeMiMizer apps and drivers</Description>
          <Path>{cmd}</Path>
        </RunSynchronousCommand>
      </RunSynchronous>
    </component>
  </settings>
</unattend>
"#
    )
    .replace('\n', "\r\n")
}

/// Copies a folder into another (both made as needed).
pub fn copy_dir(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::create_dir_all(to).map_err(|e| format!("{}: {e}", to.display()))?;
    for e in std::fs::read_dir(from).map_err(|e| format!("{}: {e}", from.display()))?.flatten() {
        let (src, dst) = (e.path(), to.join(e.file_name()));
        if src.is_dir() {
            copy_dir(&src, &dst)?;
        } else {
            std::fs::copy(&src, &dst).map_err(|e| format!("{}: {e}", src.display()))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_manifest() {
        let y = "PackageIdentifier: 7zip.7zip\nInstallers:\n- Architecture: x64\n  InstallerType: wix\n  InstallerSwitches:\n    Silent: /quiet /norestart\n    SilentWithProgress: /passive /norestart\n";
        let m = read_manifest(y);
        assert_eq!(m.kind.as_deref(), Some("wix"));
        assert_eq!(m.silent.as_deref(), Some("/quiet /norestart"));
        assert_eq!(m.custom, None);
        let y = "  NestedInstallerType: inno\n  NestedInstallerFiles:\n  - RelativeFilePath: MSI Center Installer.exe\n  InstallerType: zip\n    Custom: '/MERGETASKS=!runcode'\n";
        let m = read_manifest(y);
        assert_eq!((m.kind.as_deref(), m.silent, m.custom.as_deref()), (Some("zip"), None, Some("/MERGETASKS=!runcode")));
        assert_eq!((m.nested_kind.as_deref(), m.nested_file.as_deref()), (Some("inno"), Some("MSI Center Installer.exe")));
    }

    #[test]
    fn makes_the_script() {
        let mut p = Prepared::default();
        p.lines.push(install_line("7-Zip", r"w1\7z.msi", Type::Msi, "/quiet /norestart", None));
        assert_eq!(hardware_check("logitech.ghub"), Some("$usb -match 'VID_046D'"));
        p.lines.push(install_line("Bob's app", r"u1\a.exe", Type::Inno, "/VERYSILENT", Some("/X")));
        let s = script(&p);
        assert!(s.contains("Install '7-Zip' 'w1\\7z.msi' '/quiet /norestart' 'msi'"));
        assert!(s.contains("Install 'Bob''s app' 'u1\\a.exe' '/VERYSILENT /X' 'exe'"));
        assert!(s.contains("Join-Path $PSScriptRoot 'HeMiMizer-apps'") && s.is_ascii());
        assert_eq!(type_of("WIX"), Some(Type::Msi));
        assert_eq!(type_of("zip"), None);
    }
}

#[cfg(test)]
mod live {
    /// By hand only (downloads ~300 MB): `cargo test winget_live -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn winget_live() {
        let w = std::env::temp_dir().join("hemimizer-apps-test");
        for (id, arch) in [
            ("7zip.7zip", "x64"),
            ("7zip.7zip", "arm64"),
            ("Notepad++.Notepad++", "x64"),
            ("Mozilla.Firefox", "x64"),
            ("Brave.Brave", "x64"),
            ("voidtools.Everything", "arm64"),
            ("Valve.Steam", "x64"),
            ("Intel.IntelDriverAndSupportAssistant", "x64"),
            ("Microsoft.PowerToys", "x64"),
        ]
        .into_iter()
        .filter(|_| std::env::var("VENDOR_ONLY").is_err())
        .chain(super::VENDOR.iter().map(|v| (v.0, "x64")).filter(|_| std::env::var("VENDOR_ONLY").is_ok()))
        {
            let mut p = super::Prepared::default();
            let r = super::winget_app(id, arch, &w.join("x"), "x", &mut p);
            println!("{id} {arch}: {:?} {:?} appx {}", r, p.lines, p.appx.len());
        }
        let _ = std::fs::remove_dir_all(&w);
    }

    /// By hand only: writes a sample script to %TEMP%\hemimizer-sample.ps1 (to be parsed by PowerShell).
    #[test]
    #[ignore]
    fn sample_script() {
        let mut p = super::Prepared::default();
        p.lines.push(super::install_line("Intel Driver & Support Assistant", r"w1\Intel® DSA.exe", super::Type::Burn, "/quiet /norestart", None));
        p.lines.push(format!("if ($gpu -match 'VEN_10DE|NVIDIA') {{ Unpacked 'NVIDIA driver' {} 'setup.exe' '-s -noreboot -noeula' }}", super::q(r"nvidia\621.40.exe")));
        p.lines.push(format!("if ((Get-CimInstance Win32_Processor).Name -match 'AMD') {{ {} }}", super::install_line("AMD chipset driver", r"amd-chipset\a.exe", super::Type::Nullsoft, "/S", None)));
        std::fs::write(std::env::temp_dir().join("hemimizer-sample.ps1"), format!("\u{feff}{}", super::script(&p))).unwrap();
        std::fs::write(std::env::temp_dir().join("hemimizer-unattend.xml"), super::unattend("arm64")).unwrap();
    }
}
