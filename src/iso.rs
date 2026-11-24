//! "Make a smaller Windows" – a lighter Windows 11 ISO in the spirit of Tiny11, by HeMiMizer's own list (no
//! third-party script): one edition is taken from the ISO, the apps and parts chosen are removed, the registry of the
//! image gets the chosen settings, the image is cleaned and compressed, and a new bootable ISO (BIOS + UEFI) is made
//! with oscdimg of Microsoft (downloaded from Microsoft's symbol server, its signature checked).
//!
//! The work runs in C:\HeMiMizer-ISO (DISM needs a local NTFS folder) and is removed at the end, also after an error
//! (the image is unmounted without saving, the hives unloaded, the ISO closed).

use crate::sys;
use std::path::{Path, PathBuf};

/// What can be removed from or set in the image. `simple`: in the simple (recommended) choice.
pub struct Item {
    pub id: &'static str,
    pub title: &'static str,
    pub detail: &'static str,
    pub simple: bool,
    pub what: What,
}

pub enum What {
    /// provisioned Store apps: the beginnings of their names
    Apps(&'static [&'static str]),
    /// files or folders of the image (relative to its root), removed
    Paths(&'static [&'static str]),
    /// registry values: (hive, key, name, value) – hive SOFTWARE, SYSTEM or DEFAULT (the profile of new users)
    Reg(&'static [(&'static str, &'static str, &'static str, u32)]),
    /// done in its own way (see build)
    Own,
}

const fn it(id: &'static str, title: &'static str, detail: &'static str, simple: bool, what: What) -> Item {
    Item { id, title, detail, simple, what }
}

pub const REMOVE: &[Item] = &[
    it("news", "News, Weather, Tips and Get Help", "Bing News, Bing Weather, Tips, Get Help", true, What::Apps(&["Microsoft.BingNews", "Microsoft.BingWeather", "Microsoft.GetHelp", "Microsoft.Getstarted"])),
    it("office", "Microsoft 365, Outlook (new), To Do, Power Automate", "Ads for paid Microsoft apps", true, What::Apps(&["Microsoft.MicrosoftOfficeHub", "Microsoft.OutlookForWindows", "Microsoft.Todos", "Microsoft.PowerAutomateDesktop"])),
    it("teams", "Teams, Family, Quick Assist, Phone Link", "Microsoft chat and remote help", true, What::Apps(&["MSTeams", "MicrosoftTeams", "MicrosoftCorporationII.MicrosoftFamily", "MicrosoftCorporationII.QuickAssist", "Microsoft.YourPhone"])),
    it("clipchamp", "Clipchamp", "Microsoft's video editor", true, What::Apps(&["Clipchamp.Clipchamp"])),
    it("extras", "Solitaire, Maps, Alarms, Sound Recorder, Feedback Hub", "Small apps few people use", true, What::Apps(&["Microsoft.MicrosoftSolitaireCollection", "Microsoft.WindowsMaps", "Microsoft.WindowsAlarms", "Microsoft.WindowsSoundRecorder", "Microsoft.WindowsFeedbackHub", "Microsoft.People", "microsoft.windowscommunicationsapps"])),
    it("ai", "Copilot, Dev Home, Cortana, Bing Search", "AI and search apps", true, What::Apps(&["Microsoft.Copilot", "Microsoft.Windows.DevHome", "Microsoft.549981C3F5F10", "Microsoft.BingSearch"])),
    it("xbox", "Xbox apps and Game Bar", "Keep them for Game Pass and the Xbox app", true, What::Apps(&["Microsoft.GamingApp", "Microsoft.Xbox.TCUI", "Microsoft.XboxGamingOverlay", "Microsoft.XboxGameOverlay", "Microsoft.XboxSpeechToTextOverlay", "Microsoft.XboxIdentityProvider"])),
    it("media", "Media Player and Movies & TV", "VLC or another player instead", false, What::Apps(&["Microsoft.ZuneMusic", "Microsoft.ZuneVideo"])),
    it("onedrive", "OneDrive", "Not installed for new users", true, What::Own),
    it("edge", "Microsoft Edge", "The browser (WebView2, which apps need, stays) – install another one first", false, What::Paths(&[r"Program Files (x86)\Microsoft\Edge", r"Program Files (x86)\Microsoft\EdgeUpdate", r"Program Files (x86)\Microsoft\EdgeCore"])),
];

pub const SETTINGS: &[Item] = &[
    it("hw", "No TPM 2.0, Secure Boot and RAM check", "Installs on older PCs too", true, What::Own),
    it("msa", "No Microsoft account required", "A local account at the setup", true, What::Reg(&[("SOFTWARE", r"Microsoft\Windows\CurrentVersion\OOBE", "BypassNRO", 1)])),
    it("ads", "No ads, suggestions and sponsored apps", "No apps installed by themselves, no tips in Start", true, What::Reg(&[
        ("SOFTWARE", r"Policies\Microsoft\Windows\CloudContent", "DisableWindowsConsumerFeatures", 1),
        ("DEFAULT", r"Software\Microsoft\Windows\CurrentVersion\ContentDeliveryManager", "SilentInstalledAppsEnabled", 0),
        ("DEFAULT", r"Software\Microsoft\Windows\CurrentVersion\ContentDeliveryManager", "SystemPaneSuggestionsEnabled", 0),
        ("DEFAULT", r"Software\Microsoft\Windows\CurrentVersion\ContentDeliveryManager", "PreInstalledAppsEnabled", 0),
        ("DEFAULT", r"Software\Microsoft\Windows\CurrentVersion\ContentDeliveryManager", "OemPreInstalledAppsEnabled", 0),
        ("DEFAULT", r"Software\Microsoft\Windows\CurrentVersion\ContentDeliveryManager", "SubscribedContent-338388Enabled", 0),
        ("DEFAULT", r"Software\Microsoft\Windows\CurrentVersion\ContentDeliveryManager", "SubscribedContent-338389Enabled", 0),
    ])),
    it("telemetry", "Least diagnostic data", "Only the required data goes to Microsoft", true, What::Reg(&[
        ("SOFTWARE", r"Policies\Microsoft\Windows\DataCollection", "AllowTelemetry", 0),
        ("DEFAULT", r"Software\Microsoft\Windows\CurrentVersion\AdvertisingInfo", "Enabled", 0),
    ])),
    it("copilot", "Copilot off", "No Copilot button and app", true, What::Reg(&[("SOFTWARE", r"Policies\Microsoft\Windows\WindowsCopilot", "TurnOffWindowsCopilot", 1)])),
    it("autoinstall", "No Outlook, Teams and Dev Home installed later", "Windows Update does not add them by itself", true, What::Own),
    it("bitlocker", "No automatic BitLocker encryption", "The disk is encrypted only if you turn it on", true, What::Reg(&[("SYSTEM", r"ControlSet001\Control\BitLocker", "PreventDeviceEncryption", 1)])),
    it("reserved", "No reserved storage", "About 7 GB more space for you", true, What::Reg(&[("SOFTWARE", r"Microsoft\Windows\CurrentVersion\ReserveManager", "ShippedWithReserves", 0)])),
];

#[derive(Clone, Debug, Default)]
pub struct Options {
    pub iso: PathBuf,
    /// the index of the edition in install.wim / install.esd
    pub index: u32,
    pub out: PathBuf,
    /// ids of REMOVE and SETTINGS that are on
    pub chosen: Vec<String>,
    /// drivers to add (a folder with .inf files), empty = none
    pub drivers: String,
    /// HeMiMizer.exe on the desktop of the new Windows
    pub add_hemimizer: bool,
    /// the image compressed harder (install.esd – smaller, slower to make)
    pub esd: bool,
    /// the component store cleaned (/ResetBase – smaller, takes long)
    pub deep_clean: bool,
}

impl Options {
    fn on(&self, id: &str) -> bool {
        self.chosen.iter().any(|c| c == id)
    }
}

/// The simple choice: everything marked `simple`.
pub fn simple() -> Vec<String> {
    REMOVE.iter().chain(SETTINGS).filter(|i| i.simple).map(|i| i.id.to_string()).collect()
}

fn work() -> PathBuf {
    PathBuf::from(format!(r"{}\HeMiMizer-ISO", sys::env("SystemDrive", "C:")))
}

fn q(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

fn dism(args: &[&str]) -> Result<String, String> {
    let o = sys::exec("dism.exe", args)?;
    if o.code == 0 || o.code == 3010 {
        Ok(o.text)
    } else {
        Err(format!("DISM {:#X}: {}", o.code as u32, o.text.trim().lines().filter(|l| !l.trim().is_empty()).last().unwrap_or("")))
    }
}

/// Opened without a drive letter, into a hidden folder (the same as the USB stick: no new drive pops up).
fn mount_iso(iso: &Path) -> Result<PathBuf, String> {
    crate::usb::open_iso(iso)
}

fn dismount_iso(iso: &Path) {
    crate::usb::close_iso(iso);
}

/// The image in an ISO: install.wim or install.esd.
fn image_in(root: &Path) -> Option<PathBuf> {
    ["install.wim", "install.esd"].iter().map(|n| root.join("sources").join(n)).find(|p| p.is_file())
}

/// The editions of the ISO: (index, name).
pub fn editions(iso: &Path) -> Result<Vec<(u32, String)>, String> {
    let root = mount_iso(iso)?;
    let r = (|| {
        let img = image_in(&root).ok_or("This ISO has no install.wim or install.esd – is it Windows?")?;
        let t = dism(&["/English", "/Get-WimInfo", &format!("/WimFile:{}", img.display())])?;
        let mut v = Vec::new();
        let mut idx = None;
        for l in t.lines() {
            if let Some(n) = l.trim().strip_prefix("Index : ") {
                idx = n.trim().parse().ok();
            } else if let (Some(i), Some(name)) = (idx, l.trim().strip_prefix("Name : ")) {
                v.push((i, name.trim().to_string()));
                idx = None;
            }
        }
        if v.is_empty() { Err("No editions found in the ISO".to_string()) } else { Ok(v) }
    })();
    dismount_iso(iso);
    r
}

/// Makes the ISO; `log` tells what it does.
pub fn build(o: &Options, log: &dyn Fn(String)) -> Result<String, String> {
    let w = work();
    cleanup(&w);
    // made anew with permissions only for administrators: a folder in the root of C: may otherwise be changed by every
    // user (oscdimg.exe is run from it, ownership is taken of what is in it) – audit 2026-11-24
    sys::make_protected_dir(&w)?;
    std::fs::create_dir_all(w.join("mount")).map_err(|e| format!("{}: {e}", w.display()))?;
    let r = run(o, &w, log);
    if r.is_err() {
        log("Cleaning up after the error".into());
    }
    cleanup(&w);
    dismount_iso(&o.iso);
    r
}

/// Everything of an earlier or failed run goes: mounted images without saving, loaded hives, the work folder.
fn cleanup(w: &Path) {
    for h in ["zSOFTWARE", "zSYSTEM", "zDEFAULT"] {
        let _ = sys::exec("reg.exe", &["unload", &format!(r"HKLM\{h}")]);
    }
    for m in ["mount", "bootmount"] {
        let dir = w.join(m);
        if dir.join("Windows").exists() {
            let _ = sys::exec("dism.exe", &["/Unmount-Image", &format!("/MountDir:{}", dir.display()), "/Discard"]);
        }
    }
    let _ = sys::exec("dism.exe", &["/Cleanup-Wim"]);
    let _ = std::fs::remove_dir_all(w);
}

fn run(o: &Options, w: &Path, log: &dyn Fn(String)) -> Result<String, String> {
    let iso_dir = w.join("iso");
    let mount = w.join("mount");
    log("Copying the ISO".into());
    let src = mount_iso(&o.iso)?;
    let img = image_in(&src).ok_or("This ISO has no install.wim or install.esd – is it Windows?")?;
    let rc = sys::exec(
        "robocopy.exe",
        &[&src.display().to_string(), &iso_dir.display().to_string(), "/E", "/R:1", "/W:1", "/NFL", "/NDL", "/NJH", "/NJS", "/NP", "/A-:R", "/XF", "install.wim", "install.esd"],
    )?;
    if rc.code >= 8 {
        return Err(format!("Copying the ISO failed (robocopy {})", rc.code));
    }
    log("Taking the edition out of the image".into());
    let wim = w.join("install.wim");
    dism(&[
        "/Export-Image",
        &format!("/SourceImageFile:{}", img.display()),
        &format!("/SourceIndex:{}", o.index),
        &format!("/DestinationImageFile:{}", wim.display()),
        "/Compress:max",
        "/CheckIntegrity",
    ])?;
    dismount_iso(&o.iso);

    log("Opening the image (a few minutes)".into());
    dism(&["/Mount-Image", &format!("/ImageFile:{}", wim.display()), "/Index:1", &format!("/MountDir:{}", mount.display())])?;

    // the apps
    let pkgs = dism(&["/English", &format!("/Image:{}", mount.display()), "/Get-ProvisionedAppxPackages"])?;
    let names: Vec<&str> = pkgs.lines().filter_map(|l| l.trim().strip_prefix("PackageName : ")).map(str::trim).collect();
    let mut removed = 0;
    for item in REMOVE.iter().filter(|i| o.on(i.id)) {
        match &item.what {
            What::Apps(list) => {
                log(format!("Removing {}", item.title));
                for n in names.iter().filter(|n| list.iter().any(|p| n.to_lowercase().starts_with(&p.to_lowercase()))) {
                    if dism(&[&format!("/Image:{}", mount.display()), "/Remove-ProvisionedAppxPackage", &format!("/PackageName:{n}")]).is_ok() {
                        removed += 1;
                    }
                }
            }
            What::Paths(list) => {
                log(format!("Removing {}", item.title));
                for p in *list {
                    take_and_delete(&mount.join(p));
                }
            }
            What::Own if item.id == "onedrive" => {
                log("Removing OneDrive".into());
                take_and_delete(&mount.join(r"Windows\System32\OneDriveSetup.exe"));
                take_and_delete(&mount.join(r"Windows\SysWOW64\OneDriveSetup.exe"));
            }
            _ => {}
        }
    }

    // the registry of the image
    log("Setting up the registry of the image".into());
    let hive = |name: &str, file: &Path| sys::exec("reg.exe", &["load", &format!(r"HKLM\z{name}"), &file.display().to_string()]).map(|o| o.code == 0);
    hive("SOFTWARE", &mount.join(r"Windows\System32\config\SOFTWARE"))?;
    hive("SYSTEM", &mount.join(r"Windows\System32\config\SYSTEM"))?;
    hive("DEFAULT", &mount.join(r"Users\Default\ntuser.dat"))?;
    let set = |hive: &str, key: &str, name: &str, v: u32| {
        sys::exec("reg.exe", &["add", &format!(r"HKLM\z{hive}\{key}"), "/v", name, "/t", "REG_DWORD", "/d", &v.to_string(), "/f"]).map(|_| ())
    };
    let mut reg_fail = Vec::new();
    for item in SETTINGS.iter().filter(|i| o.on(i.id)) {
        match &item.what {
            What::Reg(list) => {
                for (h, k, n, v) in *list {
                    if let Err(e) = set(h, k, n, *v) {
                        reg_fail.push(e);
                    }
                }
            }
            What::Own if item.id == "hw" => {
                for n in ["BypassTPMCheck", "BypassSecureBootCheck", "BypassRAMCheck", "BypassCPUCheck", "BypassStorageCheck"] {
                    let _ = set("SYSTEM", r"Setup\LabConfig", n, 1);
                }
                let _ = set("SYSTEM", r"Setup\MoSetup", "AllowUpgradesWithUnsupportedTPMOrCPU", 1);
                let _ = set("DEFAULT", r"Control Panel\UnsupportedHardwareNotificationCache", "SV1", 0);
                let _ = set("DEFAULT", r"Control Panel\UnsupportedHardwareNotificationCache", "SV2", 0);
            }
            What::Own if item.id == "autoinstall" => {
                for k in ["OutlookUpdate", "DevHomeUpdate"] {
                    let _ = sys::exec("reg.exe", &["delete", &format!(r"HKLM\zSOFTWARE\Microsoft\WindowsUpdate\Orchestrator\UScheduler_Oobe\{k}"), "/f"]);
                }
                let _ = set("SOFTWARE", r"Microsoft\Windows\CurrentVersion\Communications", "ConfigureChatAutoInstall", 0);
            }
            _ => {}
        }
    }
    if o.on("onedrive") {
        let _ = sys::exec("reg.exe", &["delete", r"HKLM\zDEFAULT\Software\Microsoft\Windows\CurrentVersion\Run", "/v", "OneDriveSetup", "/f"]);
    }
    for h in ["zSOFTWARE", "zSYSTEM", "zDEFAULT"] {
        let _ = sys::exec("reg.exe", &["unload", &format!(r"HKLM\{h}")]);
    }

    // additions
    if !o.drivers.trim().is_empty() {
        log("Adding the drivers".into());
        dism(&[&format!("/Image:{}", mount.display()), "/Add-Driver", &format!("/Driver:{}", o.drivers.trim().trim_matches('"')), "/Recurse"])?;
    }
    if o.add_hemimizer {
        if let Ok(exe) = std::env::current_exe() {
            let desk = mount.join(r"Users\Public\Desktop");
            let _ = std::fs::create_dir_all(&desk);
            let _ = std::fs::copy(exe, desk.join("HeMiMizer.exe"));
        }
    }
    if o.deep_clean {
        log("Cleaning the image (this takes long)".into());
        dism(&[&format!("/Image:{}", mount.display()), "/Cleanup-Image", "/StartComponentCleanup", "/ResetBase"])?;
    }
    log("Saving the image".into());
    dism(&["/Unmount-Image", &format!("/MountDir:{}", mount.display()), "/Commit"])?;
    log(if o.esd { "Compressing the image (install.esd, this takes long)".into() } else { "Compressing the image".into() });
    let dest = iso_dir.join("sources").join(if o.esd { "install.esd" } else { "install.wim" });
    dism(&[
        "/Export-Image",
        &format!("/SourceImageFile:{}", wim.display()),
        "/SourceIndex:1",
        &format!("/DestinationImageFile:{}", dest.display()),
        if o.esd { "/Compress:recovery" } else { "/Compress:max" },
    ])?;
    let _ = std::fs::remove_file(&wim);

    // the setup itself skips the check too (boot.wim, its 2nd image is Windows Setup)
    if o.on("hw") {
        log("Setup without the hardware check".into());
        let bm = w.join("bootmount");
        std::fs::create_dir_all(&bm).map_err(|e| e.to_string())?;
        let boot = iso_dir.join(r"sources\boot.wim");
        let _ = sys::exec("attrib.exe", &["-r", &boot.display().to_string()]);
        dism(&["/Mount-Image", &format!("/ImageFile:{}", boot.display()), "/Index:2", &format!("/MountDir:{}", bm.display())])?;
        let loaded = hive("SYSTEM", &bm.join(r"Windows\System32\config\SYSTEM"));
        if loaded.is_ok() {
            for n in ["BypassTPMCheck", "BypassSecureBootCheck", "BypassRAMCheck", "BypassCPUCheck", "BypassStorageCheck"] {
                let _ = set("SYSTEM", r"Setup\LabConfig", n, 1);
            }
            let _ = sys::exec("reg.exe", &["unload", r"HKLM\zSYSTEM"]);
        }
        dism(&["/Unmount-Image", &format!("/MountDir:{}", bm.display()), "/Commit"])?;
    }

    log("Making the ISO".into());
    let oscdimg = oscdimg(w)?;
    let etfs = iso_dir.join(r"boot\etfsboot.com");
    let efi = iso_dir.join(r"efi\microsoft\boot\efisys.bin");
    let label = "HEMIMIZER_WIN11";
    let _ = std::fs::remove_file(&o.out);
    let mk = sys::exec(
        &oscdimg.display().to_string(),
        &[
            "-m",
            "-o",
            "-u2",
            "-udfver102",
            &format!("-l{label}"),
            &format!("-bootdata:2#p0,e,b{}#pEF,e,b{}", etfs.display(), efi.display()),
            &iso_dir.display().to_string(),
            &o.out.display().to_string(),
        ],
    )?;
    if mk.code != 0 || !o.out.is_file() {
        return Err(format!("oscdimg {}: {}", mk.code, mk.text.trim().lines().last().unwrap_or("")));
    }
    let size = std::fs::metadata(&o.out).map(|m| m.len()).unwrap_or(0);
    let mut text = format!("{} ({:.1} GB), {} removed", o.out.display(), size as f64 / 1e9, crate::i18n::count(removed, ("app", "apps"), ("aplikace", "aplikace", "aplikací")));
    if !reg_fail.is_empty() {
        text.push_str(&format!("; not set: {}", reg_fail.join("; ")));
    }
    Ok(text)
}

/// oscdimg.exe of Microsoft (version 2.56, part of the Windows ADK), from Microsoft's symbol server. The files there carry
/// no Authenticode signature of their own (the ADK signs them by a catalog), so the download is checked against the SHA-256
/// of the known file instead – anything else is refused.
pub const OSCDIMG_SHA256: &str = "F5129F313ED7EB46F2677CF522E64264A225F226307ED0DDB52BB14C46E7CFDD";
/// `sys::download` checks a download from this address against [`OSCDIMG_SHA256`] (instead of a signature).
pub const OSCDIMG_URL: &str = "https://msdl.microsoft.com/download/symbols/oscdimg.exe/3D44737265000/oscdimg.exe";

fn oscdimg(w: &Path) -> Result<PathBuf, String> {
    let f = w.join("oscdimg.exe");
    sys::download(OSCDIMG_URL, &f, None)?;
    let hash = sys::ps(&format!("(Get-FileHash -LiteralPath {} -Algorithm SHA256).Hash", q(&f.display().to_string())))?;
    if !hash.trim().eq_ignore_ascii_case(OSCDIMG_SHA256) {
        let _ = std::fs::remove_file(&f);
        return Err(format!("oscdimg.exe from Microsoft is not the expected file (SHA-256 {})", hash.trim()));
    }
    Ok(f)
}

/// Takes a file or folder of the image (owned by TrustedInstaller) and deletes it.
fn take_and_delete(path: &Path) {
    if !path.exists() {
        return;
    }
    crate::ownership::take(path);
    if path.is_dir() {
        let _ = std::fs::remove_dir_all(path);
    } else {
        let _ = std::fs::remove_file(path);
    }
}
