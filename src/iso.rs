//! "Make a custom Windows ISO" – a Windows 11 ISO by HeMiMizer's own list (no third-party script): the editions chosen
//! are taken from the ISO, apps are removed (Easy: all but a few; Custom: the ones ticked), the registry of each image
//! gets the chosen settings, drivers and apps may be built in (iso_apps.rs), the images are cleaned and
//! compressed, and a new bootable ISO (BIOS + UEFI) is made with oscdimg of Microsoft (downloaded from Microsoft's
//! symbol server, its checksum checked).
//!
//! The work runs in C:\HeMiMizer-ISO (DISM needs a local NTFS folder) and is removed at the end, also after an error
//! (the image is unmounted without saving, the hives unloaded, the ISO closed).

use crate::sys;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};

/// A setting of the image. `easy`: part of the Easy choice (and ticked at first in Custom).
pub struct Item {
    pub id: &'static str,
    pub title: &'static str,
    pub detail: &'static str,
    pub easy: bool,
    pub what: What,
}

pub enum What {
    /// registry values: (hive, key, name, value) – hive SOFTWARE, SYSTEM or DEFAULT (the profile of new users)
    Reg(&'static [(&'static str, &'static str, &'static str, u32)]),
    /// done in its own way (see customize)
    Own,
}

const fn it(id: &'static str, title: &'static str, detail: &'static str, easy: bool, what: What) -> Item {
    Item { id, title, detail, easy, what }
}

/// An app Custom may remove: the beginnings of its package names. `default`: ticked at first.
pub struct App {
    pub id: &'static str,
    pub name: &'static str,
    pub pkgs: &'static [&'static str],
    pub default: bool,
}

const fn app(id: &'static str, name: &'static str, pkgs: &'static [&'static str], default: bool) -> App {
    App { id, name, pkgs, default }
}

/// The apps of Windows 11 Custom offers one by one (Microsoft Store, Windows Security, App Installer and the libraries
/// other apps need are never offered).
pub const APPS: &[App] = &[
    app("news", "News", &["Microsoft.BingNews"], true),
    app("weather", "Weather", &["Microsoft.BingWeather"], true),
    app("tips", "Tips and Get Help", &["Microsoft.Getstarted", "Microsoft.GetHelp"], true),
    app("office", "Microsoft 365 (Office)", &["Microsoft.MicrosoftOfficeHub"], true),
    app("outlook", "Outlook (new)", &["Microsoft.OutlookForWindows"], true),
    app("mail", "Mail, Calendar and People", &["microsoft.windowscommunicationsapps", "Microsoft.People"], true),
    app("todo", "To Do", &["Microsoft.Todos"], true),
    app("automate", "Power Automate", &["Microsoft.PowerAutomateDesktop"], true),
    app("teams", "Teams", &["MSTeams", "MicrosoftTeams"], true),
    app("family", "Family", &["MicrosoftCorporationII.MicrosoftFamily"], true),
    app("quickassist", "Quick Assist", &["MicrosoftCorporationII.QuickAssist"], true),
    app("phone", "Phone Link", &["Microsoft.YourPhone", "MicrosoftWindows.CrossDevice"], true),
    app("clipchamp", "Clipchamp", &["Clipchamp.Clipchamp"], true),
    app("solitaire", "Solitaire", &["Microsoft.MicrosoftSolitaireCollection"], true),
    app("maps", "Maps", &["Microsoft.WindowsMaps"], true),
    app("alarms", "Clock", &["Microsoft.WindowsAlarms"], true),
    app("recorder", "Sound Recorder", &["Microsoft.WindowsSoundRecorder"], true),
    app("feedback", "Feedback Hub", &["Microsoft.WindowsFeedbackHub"], true),
    app("copilot", "Copilot", &["Microsoft.Copilot", "Microsoft.Windows.Ai.Copilot.Provider"], true),
    app("devhome", "Dev Home", &["Microsoft.Windows.DevHome"], true),
    app("cortana", "Cortana and Bing Search", &["Microsoft.549981C3F5F10", "Microsoft.BingSearch"], true),
    app("xbox", "Xbox apps and Game Bar", &["Microsoft.GamingApp", "Microsoft.Xbox.TCUI", "Microsoft.XboxGamingOverlay", "Microsoft.XboxGameOverlay", "Microsoft.XboxSpeechToTextOverlay", "Microsoft.XboxIdentityProvider", "Microsoft.Edge.GameAssist"], true),
    app("movies", "Movies & TV", &["Microsoft.ZuneVideo"], false),
    app("mediaplayer", "Media Player", &["Microsoft.ZuneMusic"], false),
    app("stickynotes", "Sticky Notes", &["Microsoft.MicrosoftStickyNotes"], false),
    app("paint", "Paint", &["Microsoft.Paint"], false),
    app("calculator", "Calculator", &["Microsoft.WindowsCalculator"], false),
    app("camera", "Camera", &["Microsoft.WindowsCamera"], false),
    app("photos", "Photos", &["Microsoft.Windows.Photos"], false),
    app("snipping", "Snipping Tool", &["Microsoft.ScreenSketch"], false),
    app("notepad", "Notepad", &["Microsoft.WindowsNotepad"], false),
    app("terminal", "Terminal", &["Microsoft.WindowsTerminal"], false),
];

/// Easy: the apps that stay (Microsoft Store, App Installer, Windows Security, Terminal, Notepad, Photos, Media Player,
/// Snipping Tool, Calculator); every other app goes, except [`FRAMEWORKS`] (and Edge, which has its own choice).
pub const EASY_KEEP: &[&str] = &[
    "Microsoft.WindowsStore",
    "Microsoft.StorePurchaseApp",
    "Microsoft.DesktopAppInstaller",
    "Microsoft.SecHealthUI",
    "Microsoft.WindowsTerminal",
    "Microsoft.WindowsNotepad",
    "Microsoft.Windows.Photos",
    "Microsoft.ZuneMusic",
    "Microsoft.ScreenSketch",
    "Microsoft.WindowsCalculator",
];

/// Parts of the names of packages that other apps need (libraries, runtimes, codecs, language packs): never removed.
const FRAMEWORKS: &[&str] = &["vclibs", "ui.xaml", "net.native", "runtime", "services.store", "extension", "winget", "languageexperiencepack", "applicationcompatibility"];

/// The Edge package (its files are removed with the "edge" setting).
const EDGE_PKG: &str = "microsoft.microsoftedge";
const EDGE_PATHS: &[&str] = &[r"Program Files (x86)\Microsoft\Edge", r"Program Files (x86)\Microsoft\EdgeUpdate", r"Program Files (x86)\Microsoft\EdgeCore"];
const WIDGETS_PKG: &str = "microsoftwindows.client.webexperience";

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
    it("websearch", "No web results in the search", "Start searches only the PC, not Bing", true, What::Reg(&[("DEFAULT", r"Software\Policies\Microsoft\Windows\Explorer", "DisableSearchBoxSuggestions", 1)])),
    it("telemetry", "Least diagnostic data", "Only the required data goes to Microsoft", true, What::Reg(&[
        ("SOFTWARE", r"Policies\Microsoft\Windows\DataCollection", "AllowTelemetry", 0),
        ("DEFAULT", r"Software\Microsoft\Windows\CurrentVersion\AdvertisingInfo", "Enabled", 0),
    ])),
    it("ai", "AI off", "Copilot, Recall, Click to Do, the AI in Paint and Notepad, AI models for apps (Photos)", true, What::Reg(&[
        ("SOFTWARE", r"Policies\Microsoft\Windows\WindowsCopilot", "TurnOffWindowsCopilot", 1),
        ("DEFAULT", r"Software\Policies\Microsoft\Windows\WindowsCopilot", "TurnOffWindowsCopilot", 1),
        ("DEFAULT", r"Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced", "ShowCopilotButton", 0),
        ("SOFTWARE", r"Policies\Microsoft\Windows\WindowsAI", "DisableAIDataAnalysis", 1),
        ("SOFTWARE", r"Policies\Microsoft\Windows\WindowsAI", "AllowRecallEnablement", 0),
        ("SOFTWARE", r"Policies\Microsoft\Windows\WindowsAI", "DisableClickToDo", 1),
        ("DEFAULT", r"Software\Policies\Microsoft\Windows\WindowsAI", "DisableAIDataAnalysis", 1),
        ("DEFAULT", r"Software\Policies\Microsoft\Windows\WindowsAI", "DisableClickToDo", 1),
        ("SOFTWARE", r"Microsoft\Windows\CurrentVersion\Policies\Paint", "DisableCocreator", 1),
        ("SOFTWARE", r"Microsoft\Windows\CurrentVersion\Policies\Paint", "DisableGenerativeFill", 1),
        ("SOFTWARE", r"Microsoft\Windows\CurrentVersion\Policies\Paint", "DisableImageCreator", 1),
        ("SOFTWARE", r"Microsoft\Windows\CurrentVersion\Policies\Paint", "DisableGenerativeErase", 1),
        ("SOFTWARE", r"Microsoft\Windows\CurrentVersion\Policies\Paint", "DisableRemoveBackground", 1),
        ("SOFTWARE", r"Policies\WindowsNotepad", "DisableAIFeatures", 1),
        // 2 = apps are denied the AI models of Windows (the generative features of Photos and others)
        ("SOFTWARE", r"Policies\Microsoft\Windows\AppPrivacy", "LetAppsAccessSystemAIModels", 2),
        ("SOFTWARE", r"Policies\Microsoft\Edge", "HubsSidebarEnabled", 0),
    ])),
    it("widgets", "No widgets", "No news board on the taskbar (Widgets removed)", true, What::Reg(&[("SOFTWARE", r"Policies\Microsoft\Dsh", "AllowNewsAndInterests", 0)])),
    it("onedrive", "No OneDrive", "Not installed for new users", true, What::Own),
    it("edge", "Remove Microsoft Edge", "WebView2, which apps need, stays – have another browser ready", false, What::Own),
    it("autoinstall", "No Outlook, Teams and Dev Home installed later", "Windows Update does not add them by itself", true, What::Own),
    it("bitlocker", "No automatic BitLocker encryption", "The disk is encrypted only if you turn it on", true, What::Reg(&[("SYSTEM", r"ControlSet001\Control\BitLocker", "PreventDeviceEncryption", 1)])),
    it("reserved", "No reserved storage", "About 7 GB more space for you", true, What::Reg(&[("SOFTWARE", r"Microsoft\Windows\CurrentVersion\ReserveManager", "ShippedWithReserves", 0)])),
];

/// How install.wim / install.esd is compressed (DISM /Export-Image /Compress).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Compress {
    /// install.wim, quick to make, bigger
    Fast,
    /// install.wim, smaller
    Max,
    /// install.esd, the smallest, slow to make
    #[default]
    Recovery,
}

impl Compress {
    fn arg(self) -> &'static str {
        match self {
            Compress::Fast => "/Compress:fast",
            Compress::Max => "/Compress:max",
            Compress::Recovery => "/Compress:recovery",
        }
    }

    fn file(self) -> &'static str {
        if self == Compress::Recovery { "install.esd" } else { "install.wim" }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Options {
    pub iso: PathBuf,
    /// the editions: (index in install.wim / install.esd, name)
    pub editions: Vec<(u32, String)>,
    pub out: PathBuf,
    /// Easy: only [`EASY_KEEP`] stays; else the [`APPS`] in `apps` go
    pub easy: bool,
    /// ids of APPS removed (Custom)
    pub apps: Vec<String>,
    /// ids of SETTINGS that are on
    pub chosen: Vec<String>,
    /// drivers to add (a folder with .inf files), empty = none
    pub drivers: String,
    /// winget ids built into the image (installed quietly while Windows sets itself up)
    pub winget: Vec<String>,
    /// a folder whose installers (.msi, and .exe of Inno Setup / NSIS / WiX) are built in too, empty = none
    pub installers: String,
    /// the drivers downloaded and built in (ids of iso_apps::DRIVERS)
    pub driver_dl: Vec<String>,
    /// HeMiMizer.exe on the desktop of the new Windows
    pub add_hemimizer: bool,
    pub compress: Compress,
    /// the component store cleaned (/ResetBase – smaller, takes long)
    pub deep_clean: bool,
}

impl Options {
    fn on(&self, id: &str) -> bool {
        self.chosen.iter().any(|c| c == id)
    }

    /// Whether a provisioned app package goes.
    fn goes(&self, package: &str) -> bool {
        let n = package.to_lowercase();
        let starts = |p: &&str| n.starts_with(&p.to_lowercase());
        if FRAMEWORKS.iter().any(|f| n.contains(f)) {
            return false;
        }
        if n.starts_with(EDGE_PKG) {
            return self.on("edge");
        }
        if n.starts_with(WIDGETS_PKG) && self.on("widgets") {
            return true;
        }
        if self.easy {
            !EASY_KEEP.iter().any(starts)
        } else {
            APPS.iter().filter(|a| self.apps.iter().any(|x| x == a.id)).any(|a| a.pkgs.iter().any(starts))
        }
    }

    /// The installers of the folder chosen (.exe and .msi), sorted by name.
    fn installer_files(&self) -> Vec<PathBuf> {
        let dir = self.installers.trim().trim_matches('"');
        if dir.is_empty() {
            return Vec::new();
        }
        let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_file() && p.extension().is_some_and(|x| x.eq_ignore_ascii_case("exe") || x.eq_ignore_ascii_case("msi")))
            .collect();
        v.sort();
        v
    }
}

/// The settings of the Easy choice (Edge stays; the page adds "edge" when it should go).
pub fn easy() -> Vec<String> {
    SETTINGS.iter().filter(|i| i.easy).map(|i| i.id.to_string()).collect()
}

/// The apps Custom removes at first.
pub fn default_apps() -> Vec<String> {
    APPS.iter().filter(|a| a.default).map(|a| a.id.to_string()).collect()
}

/// A winget package id as winget prints it ("Mozilla.Firefox", "7zip.7zip"): letters, digits and . - _ + only.
pub fn valid_winget(id: &str) -> bool {
    !id.is_empty() && id.len() <= 128 && id.chars().all(|c| c.is_ascii_alphanumeric() || ".-_+".contains(c))
}

/// The winget ids of a text (separated by spaces, commas, semicolons or new lines).
pub fn winget_ids(text: &str) -> Vec<String> {
    text.split(|c: char| c.is_whitespace() || c == ',' || c == ';').filter(|s| !s.is_empty()).map(str::to_string).collect()
}

/// The last percentage in a text ("[====  42.0%  ]" of DISM, "45% complete" of oscdimg, "… (33%)"), 0–100.
pub fn percent(s: &[u8]) -> Option<f32> {
    let end = s.iter().rposition(|&b| b == b'%')?;
    let start = s[..end].iter().rposition(|b| !(b.is_ascii_digit() || *b == b'.' || *b == b',')).map_or(0, |i| i + 1);
    let t = std::str::from_utf8(&s[start..end]).ok()?.replace(',', ".");
    t.parse::<f32>().ok().filter(|p| (0.0..=100.0).contains(p))
}

/// Cancel was clicked (the page "Make a custom Windows ISO"); the program that works now (DISM, robocopy, oscdimg).
static CANCEL: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static CHILD: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
/// The result of a cancelled run.
pub const CANCELLED: &str = "Cancelled – nothing was made and the work folder is cleaned up.";

/// Cancels the ISO being made: the program working now ends (with its own programs, DismHost), the next step does not
/// start, and `build` cleans up as after an error.
pub fn cancel() {
    use std::sync::atomic::Ordering;
    CANCEL.store(true, Ordering::SeqCst);
    let pid = CHILD.swap(0, Ordering::SeqCst);
    if pid != 0 {
        let _ = sys::exec("taskkill.exe", &["/PID", &pid.to_string(), "/T", "/F"]);
    }
}

pub fn cancelled() -> bool {
    CANCEL.load(std::sync::atomic::Ordering::SeqCst)
}

pub(crate) fn go_on() -> Result<(), String> {
    if cancelled() { Err(CANCELLED.into()) } else { Ok(()) }
}

/// A program of the run started (0 = it ended): Cancel ends it.
pub(crate) fn working(pid: u32) {
    CHILD.store(pid, std::sync::atomic::Ordering::SeqCst);
    // cancelled while it was starting
    if pid != 0 && cancelled() {
        cancel();
    }
}

/// The free space the work folder needs on the system disk (the copy of the ISO, the editions, one of them opened).
pub const WORK_SPACE: u64 = 25_000_000_000;

/// What is missing for making the ISO: too little space on the system disk, or for the new ISO on its own drive.
pub fn space_problem(o: &Options) -> Option<String> {
    let w = work();
    let sys_drive = w.ancestors().last().unwrap_or(&w).to_path_buf();
    let have = crate::usb::free(&sys_drive);
    let iso = std::fs::metadata(&o.iso).map(|m| m.len()).unwrap_or(0);
    let gb = |b: u64| b as f64 / 1e9;
    let out_root = o.out.ancestors().last().map(Path::to_path_buf).unwrap_or_default();
    let same = out_root.display().to_string().eq_ignore_ascii_case(&sys_drive.display().to_string());
    // the new ISO is about as big as the one of Microsoft, at most
    let need = WORK_SPACE + if same { iso } else { 0 };
    if have > 0 && have < need {
        return Some(format!("Only {:.0} GB free on {} – it needs about {:.0} GB while it works.", gb(have), sys_drive.display(), gb(need)));
    }
    if !same && out_root.exists() {
        let out_free = crate::usb::free(&out_root);
        if out_free > 0 && out_free < iso {
            return Some(format!("Only {:.1} GB free on {} for the new ISO – it needs about {:.1} GB.", gb(out_free), out_root.display(), gb(iso)));
        }
    }
    None
}

fn work() -> PathBuf {
    PathBuf::from(format!(r"{}\HeMiMizer-ISO", sys::env("SystemDrive", "C:")))
}

fn q(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// DISM (in English, so that its output can be read), waited for.
fn dism(args: &[&str]) -> Result<String, String> {
    let all: Vec<&str> = std::iter::once("/English").chain(args.iter().copied()).collect();
    let o = sys::exec("dism.exe", &all)?;
    if o.code == 0 || o.code == 3010 { Ok(o.text) } else { Err(dism_error(o.code, &o.text)) }
}

/// DISM with its progress: `what – NN%` goes to `log` while it works.
fn dism_p(args: &[&str], what: &str, log: &dyn Fn(String)) -> Result<String, String> {
    let all: Vec<&str> = std::iter::once("/English").chain(args.iter().copied()).collect();
    let o = run_progress("dism.exe", &all, what, log)?;
    if o.code == 0 || o.code == 3010 { Ok(o.text) } else { Err(dism_error(o.code, &o.text)) }
}

/// The message of a failed DISM: the line after "Error: …" (else the last line), without the progress bars.
fn dism_error(code: i32, text: &str) -> String {
    let lines: Vec<&str> = text.split(['\r', '\n']).map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('[')).collect();
    let msg = lines.iter().position(|l| l.starts_with("Error")).and_then(|i| lines.get(i + 1).or(lines.get(i))).or(lines.last()).copied().unwrap_or("");
    format!("DISM {:#X}: {msg}", code as u32)
}

/// Runs a program without a window and reads its output while it runs (stdout and stderr in one pipe): each new
/// percentage it prints goes to `log` as "`what` – NN%" (DISM, oscdimg). The command, its exit code and its output
/// (without the progress bars) go to the log of HeMiMizer, as `sys::exec` does.
pub fn run_progress(exe: &str, args: &[&str], what: &str, log: &dyn Fn(String)) -> Result<sys::Output, String> {
    use std::io::Read;
    let started = std::time::Instant::now();
    sys::log(&format!("> {exe} {}", args.iter().map(|a| if a.contains(' ') { format!("\"{a}\"") } else { a.to_string() }).collect::<Vec<_>>().join(" ")));
    log(what.to_string());
    let (mut reader, writer) = std::io::pipe().map_err(|e| e.to_string())?;
    let writer2 = writer.try_clone().map_err(|e| e.to_string())?;
    let mut cmd = std::process::Command::new(sys::program(exe));
    cmd.args(args).stdin(std::process::Stdio::null()).stdout(writer).stderr(writer2).creation_flags(0x0800_0000);
    let child = cmd.spawn();
    // the write ends of this process closed, so that the reading ends when the program does
    drop(cmd);
    let mut child = child.map_err(|e| format!("{exe}: {e}"))?;
    working(child.id());
    let mut all = Vec::new();
    let mut buf = [0u8; 4096];
    let mut last = -1i32;
    loop {
        let n = match reader.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        all.extend_from_slice(&buf[..n]);
        let tail = &all[all.len().saturating_sub(96)..];
        if let Some(p) = percent(tail) {
            let p = p as i32;
            if p != last {
                last = p;
                log(format!("{what} – {p}%"));
            }
        }
    }
    let code = child.wait().map_err(|e| e.to_string())?.code().unwrap_or(-1);
    working(0);
    let text = String::from_utf8_lossy(&all).into_owned();
    let shown: String = text.split(['\r', '\n']).map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('[')).collect::<Vec<_>>().join("\n").chars().take(3000).collect();
    sys::log(&format!("< code {code} after {:.1} s\n{shown}", started.elapsed().as_secs_f32()));
    go_on()?;
    Ok(sys::Output { code, text })
}

/// " (2 of 3)" when there is more than one edition.
fn of(k: usize, n: usize) -> String {
    if n > 1 { format!(" ({} of {n})", k + 1) } else { String::new() }
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
        let t = dism(&["/Get-WimInfo", &format!("/WimFile:{}", img.display())])?;
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
    CANCEL.store(false, std::sync::atomic::Ordering::SeqCst);
    if let Some(p) = space_problem(o) {
        return Err(p);
    }
    let w = work();
    cleanup(&w);
    // made anew with permissions only for administrators: a folder in the root of C: may otherwise be changed by every
    // user (oscdimg.exe is run from it, ownership is taken of what is in it) – audit 2026-11-24
    sys::make_protected_dir(&w)?;
    std::fs::create_dir_all(w.join("mount")).map_err(|e| format!("{}: {e}", w.display()))?;
    let r = run(o, &w, log);
    if r.is_err() {
        log(if cancelled() { "Cancelled – cleaning up" } else { "Cleaning up after the error" }.into());
    }
    let r = if cancelled() && r.is_err() { Err(CANCELLED.to_string()) } else { r };
    // a USB stick made later runs DISM through run_progress too: not cancelled by this
    CANCEL.store(false, std::sync::atomic::Ordering::SeqCst);
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

fn hive(name: &str, file: &Path) -> Result<bool, String> {
    sys::exec("reg.exe", &["load", &format!(r"HKLM\z{name}"), &file.display().to_string()]).map(|o| o.code == 0)
}

fn set(hive: &str, key: &str, name: &str, v: u32) -> Result<(), String> {
    sys::exec("reg.exe", &["add", &format!(r"HKLM\z{hive}\{key}"), "/v", name, "/t", "REG_DWORD", "/d", &v.to_string(), "/f"]).map(|_| ())
}

fn run(o: &Options, w: &Path, log: &dyn Fn(String)) -> Result<String, String> {
    if o.editions.is_empty() {
        return Err("No edition chosen".into());
    }
    let iso_dir = w.join("iso");
    let mount = w.join("mount");
    log("Copying the ISO".into());
    let src = mount_iso(&o.iso)?;
    let img = image_in(&src).ok_or("This ISO has no install.wim or install.esd – is it Windows?")?;
    // the apps and drivers first: a download that fails stops it before the long work
    let installers = o.installer_files();
    let prep = if o.winget.is_empty() && o.driver_dl.is_empty() && installers.is_empty() {
        crate::iso_apps::Prepared::default()
    } else {
        let arch = image_arch(&img, o.editions[0].0);
        crate::iso_apps::prepare(&o.winget, &o.driver_dl, &installers, &arch, w, log)?
    };
    // made before the copy: its progress is measured on the disk of the folder
    std::fs::create_dir_all(&iso_dir).map_err(|e| format!("{}: {e}", iso_dir.display()))?;
    let total = crate::usb::folder_size(&src).saturating_sub(std::fs::metadata(&img).map(|m| m.len()).unwrap_or(0));
    crate::usb::copy(&src, &iso_dir, &[], &[PathBuf::from("install.wim"), PathBuf::from("install.esd")], total, "Copying the ISO", log)?;

    // the editions chosen into one image (index 1, 2 …, files shared between them are stored once)
    let wim = w.join("install.wim");
    let n = o.editions.len();
    for (k, (index, name)) in o.editions.iter().enumerate() {
        dism_p(
            &[
                "/Export-Image",
                &format!("/SourceImageFile:{}", img.display()),
                &format!("/SourceIndex:{index}"),
                &format!("/DestinationImageFile:{}", wim.display()),
                "/Compress:max",
                "/CheckIntegrity",
            ],
            &format!("Taking {name} out of the ISO{}", of(k, n)),
            log,
        )?;
    }
    dismount_iso(&o.iso);

    let mut removed = 0;
    let mut reg_fail = Vec::new();
    for (k, (_, name)) in o.editions.iter().enumerate() {
        let tag = format!("{name}{}", of(k, n));
        dism_p(&["/Mount-Image", &format!("/ImageFile:{}", wim.display()), &format!("/Index:{}", k + 1), &format!("/MountDir:{}", mount.display())], &format!("Opening {tag}"), log)?;
        let r = customize(o, &prep, &mount, &tag, log, &mut reg_fail)?;
        removed = removed.max(r);
        dism_p(&["/Unmount-Image", &format!("/MountDir:{}", mount.display()), "/Commit"], &format!("Saving {tag}"), log)?;
    }

    let dest = iso_dir.join("sources").join(o.compress.file());
    for (k, (_, name)) in o.editions.iter().enumerate() {
        dism_p(
            &[
                "/Export-Image",
                &format!("/SourceImageFile:{}", wim.display()),
                &format!("/SourceIndex:{}", k + 1),
                &format!("/DestinationImageFile:{}", dest.display()),
                o.compress.arg(),
            ],
            &format!("Compressing {name}{}{}", of(k, n), if o.compress == Compress::Recovery { " (install.esd, this takes long)" } else { "" }),
            log,
        )?;
    }
    let _ = std::fs::remove_file(&wim);

    // the setup itself skips the check too (boot.wim, its 2nd image is Windows Setup)
    if o.on("hw") {
        let bm = w.join("bootmount");
        std::fs::create_dir_all(&bm).map_err(|e| e.to_string())?;
        let boot = iso_dir.join(r"sources\boot.wim");
        let _ = sys::exec("attrib.exe", &["-r", &boot.display().to_string()]);
        dism_p(&["/Mount-Image", &format!("/ImageFile:{}", boot.display()), "/Index:2", &format!("/MountDir:{}", bm.display())], "Setup without the hardware check: opening it", log)?;
        let loaded = hive("SYSTEM", &bm.join(r"Windows\System32\config\SYSTEM"));
        if loaded.is_ok() {
            for n in ["BypassTPMCheck", "BypassSecureBootCheck", "BypassRAMCheck", "BypassCPUCheck", "BypassStorageCheck"] {
                let _ = set("SYSTEM", r"Setup\LabConfig", n, 1);
            }
            let _ = sys::exec("reg.exe", &["unload", r"HKLM\zSYSTEM"]);
        }
        dism_p(&["/Unmount-Image", &format!("/MountDir:{}", bm.display()), "/Commit"], "Setup without the hardware check: saving it", log)?;
    }

    go_on()?;
    log("Making the ISO".into());
    let oscdimg = oscdimg(w)?;
    let etfs = iso_dir.join(r"boot\etfsboot.com");
    let efi = iso_dir.join(r"efi\microsoft\boot\efisys.bin");
    let label = "HEMIMIZER_WIN11";
    let _ = std::fs::remove_file(&o.out);
    let mk = run_progress(
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
        "Making the ISO",
        log,
    )?;
    if mk.code != 0 || !o.out.is_file() {
        let last = mk.text.split(['\r', '\n']).map(str::trim).rfind(|l| !l.is_empty()).unwrap_or("");
        return Err(format!("oscdimg {}: {last}", mk.code));
    }
    let size = std::fs::metadata(&o.out).map(|m| m.len()).unwrap_or(0);
    let mut text = format!("{} ({:.1} GB), {} removed", o.out.display(), size as f64 / 1e9, crate::i18n::count(removed, ("app", "apps"), ("aplikace", "aplikace", "aplikací")));
    if n > 1 {
        text.push_str(&format!(" from each of {n} editions"));
    }
    if !prep.names.is_empty() {
        text.push_str(&format!("; built in: {} (installed while Windows sets itself up)", prep.names.join(", ")));
    }
    reg_fail.sort();
    reg_fail.dedup();
    if !reg_fail.is_empty() {
        text.push_str(&format!("; not set: {}", reg_fail.join("; ")));
    }
    Ok(text)
}

/// The processor of an image ("x64", "arm64"; x64 when DISM does not say).
fn image_arch(img: &Path, index: u32) -> String {
    dism(&["/Get-WimInfo", &format!("/WimFile:{}", img.display()), &format!("/Index:{index}")])
        .ok()
        .and_then(|t| t.lines().find_map(|l| l.trim().strip_prefix("Architecture : ").map(|a| a.trim().to_lowercase())))
        .unwrap_or_else(|| "x64".into())
}

/// One mounted image: its apps removed, its registry set, drivers, apps and drivers built in, cleaned.
/// Returns how many apps were removed.
fn customize(o: &Options, prep: &crate::iso_apps::Prepared, mount: &Path, tag: &str, log: &dyn Fn(String), reg_fail: &mut Vec<String>) -> Result<usize, String> {
    let image = format!("/Image:{}", mount.display());
    let pkgs = dism(&[&image, "/Get-ProvisionedAppxPackages"])?;
    let names: Vec<&str> = pkgs.lines().filter_map(|l| l.trim().strip_prefix("PackageName : ")).map(str::trim).filter(|n| o.goes(n)).collect();
    let mut removed = 0;
    for (k, n) in names.iter().enumerate() {
        go_on()?;
        log(format!("Removing the apps of {tag} – {} of {} ({}%)", k + 1, names.len(), k * 100 / names.len()));
        if dism(&[&image, "/Remove-ProvisionedAppxPackage", &format!("/PackageName:{n}")]).is_ok() {
            removed += 1;
        }
    }
    if o.on("edge") {
        log(format!("Removing Microsoft Edge from {tag}"));
        for p in EDGE_PATHS {
            take_and_delete(&mount.join(p));
        }
    }
    if o.on("onedrive") {
        log(format!("Removing OneDrive from {tag}"));
        take_and_delete(&mount.join(r"Windows\System32\OneDriveSetup.exe"));
        take_and_delete(&mount.join(r"Windows\SysWOW64\OneDriveSetup.exe"));
    }

    // the registry of the image
    go_on()?;
    log(format!("Setting up the registry of {tag}"));
    hive("SOFTWARE", &mount.join(r"Windows\System32\config\SOFTWARE"))?;
    hive("SYSTEM", &mount.join(r"Windows\System32\config\SYSTEM"))?;
    hive("DEFAULT", &mount.join(r"Users\Default\ntuser.dat"))?;
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
        log(format!("Adding the drivers to {tag}"));
        dism(&[&image, "/Add-Driver", &format!("/Driver:{}", o.drivers.trim().trim_matches('"')), "/Recurse"])?;
    }
    // app packages (MSIX) straight into the image
    for (pkg, deps) in &prep.appx {
        go_on()?;
        log(format!("Adding {} to {tag}", pkg.file_name().unwrap_or_default().to_string_lossy()));
        let mut args = vec![image.clone(), "/Add-ProvisionedAppxPackage".into(), format!("/PackagePath:{}", pkg.display()), "/SkipLicense".into()];
        args.extend(deps.iter().map(|d| format!("/DependencyPackagePath:{}", d.display())));
        dism(&args.iter().map(String::as_str).collect::<Vec<_>>())?;
    }
    // the installers and their script (Windows\Setup\Scripts: only administrators may change it), started by
    // Windows\Panther\unattend.xml (works with an OEM key) and by SetupComplete.cmd (when an autounattend.xml of the setup
    // media replaced that file) – see iso_apps.rs
    if prep.has_script() {
        log(format!("Adding your apps and drivers to {tag}"));
        let scripts = mount.join(r"Windows\Setup\Scripts");
        crate::iso_apps::copy_dir(&prep.dir, &scripts.join(crate::iso_apps::DIR))?;
        // UTF-8 with its mark: Windows PowerShell 5.1 reads the names of the installers right (e.g. "Intel®")
        std::fs::write(scripts.join(crate::iso_apps::SCRIPT), format!("\u{feff}{}", crate::iso_apps::script(prep))).map_err(|e| format!("{}: {e}", crate::iso_apps::SCRIPT))?;
        let panther = mount.join(r"Windows\Panther");
        let file = panther.join("unattend.xml");
        // an answer file of the image itself (rare in an ISO of Microsoft) is never overwritten
        if file.exists() {
            return Err(format!("{tag} already has Windows\\Panther\\unattend.xml – the apps cannot be added to it"));
        }
        std::fs::create_dir_all(&panther).map_err(|e| format!("{}: {e}", panther.display()))?;
        std::fs::write(&file, crate::iso_apps::unattend(&prep.arch)).map_err(|e| format!("unattend.xml: {e}"))?;
        let cmd = scripts.join("SetupComplete.cmd");
        let mut text = std::fs::read_to_string(&cmd).unwrap_or_else(|_| "@echo off\r\n".into());
        if !text.ends_with('\n') {
            text.push_str("\r\n");
        }
        text.push_str(&crate::iso_apps::setup_complete_line());
        std::fs::write(&cmd, text).map_err(|e| format!("SetupComplete.cmd: {e}"))?;
    }
    if o.add_hemimizer && let Ok(exe) = std::env::current_exe() {
        let desk = mount.join(r"Users\Public\Desktop");
        let _ = std::fs::create_dir_all(&desk);
        let _ = std::fs::copy(exe, desk.join("HeMiMizer.exe"));
    }
    if o.deep_clean {
        dism_p(&[&image, "/Cleanup-Image", "/StartComponentCleanup", "/ResetBase"], &format!("Cleaning {tag} (this takes long)"), log)?;
    }
    Ok(removed)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentages() {
        assert_eq!(percent(b"[=====                      10.0%                          ]"), Some(10.0));
        assert_eq!(percent(b"\r[==========================100.0%==========================] "), Some(100.0));
        assert_eq!(percent(b"Scanning source tree\r  45% complete"), Some(45.0));
        assert_eq!(percent(b"Removing the apps - 3 of 9 (33%)"), Some(33.0));
        assert_eq!(percent(b"Copying the ISO"), None);
        assert_eq!(percent(b"%"), None);
    }

    #[test]
    fn easy_keeps_the_few_apps_and_the_libraries() {
        let o = Options { easy: true, chosen: easy(), ..Default::default() };
        for keep in [
            "Microsoft.WindowsStore_22410.1401.5.0_neutral_~_8wekyb3d8bbwe",
            "Microsoft.SecHealthUI_1000.26100.1.0_x64__8wekyb3d8bbwe",
            "Microsoft.WindowsTerminal_3001.18.10301.0_neutral_~_8wekyb3d8bbwe",
            "Microsoft.WindowsNotepad_11.2410.21.0_neutral_~_8wekyb3d8bbwe",
            "Microsoft.Windows.Photos_24.24010.29003.0_neutral_~_8wekyb3d8bbwe",
            "Microsoft.ZuneMusic_11.2408.12.0_neutral_~_8wekyb3d8bbwe",
            "Microsoft.ScreenSketch_2022.2409.25.0_neutral_~_8wekyb3d8bbwe",
            "Microsoft.WindowsCalculator_2021.2409.0.0_neutral_~_8wekyb3d8bbwe",
            "Microsoft.DesktopAppInstaller_2024.709.2344.0_neutral_~_8wekyb3d8bbwe",
            "Microsoft.VCLibs.140.00_14.0.33519.0_x64__8wekyb3d8bbwe",
            "Microsoft.HEIFImageExtension_1.0.63001.0_x64__8wekyb3d8bbwe",
            "Microsoft.WindowsAppRuntime.1.5_5001.178.1908.0_x64__8wekyb3d8bbwe",
            "Microsoft.MicrosoftEdge.Stable_130.0.2849.68_neutral__8wekyb3d8bbwe",
        ] {
            assert!(!o.goes(keep), "{keep}");
        }
        for gone in [
            "Clipchamp.Clipchamp_2.2.8.0_neutral_~_yxz26nhyzhsrt",
            "Microsoft.BingNews_4.55.62231.0_x64__8wekyb3d8bbwe",
            "Microsoft.Copilot_1.0.3.0_neutral_~_8wekyb3d8bbwe",
            "Microsoft.Paint_11.2410.28.0_neutral_~_8wekyb3d8bbwe",
            "MicrosoftWindows.Client.WebExperience_424.1301.270.9_neutral_~_cw5n1h2txyewy",
        ] {
            assert!(o.goes(gone), "{gone}");
        }
        // Edge goes only when chosen
        let mut no_edge = o.clone();
        no_edge.chosen.push("edge".into());
        assert!(no_edge.goes("Microsoft.MicrosoftEdge.Stable_130.0.2849.68_neutral__8wekyb3d8bbwe"));
    }

    #[test]
    fn custom_removes_only_the_ticked() {
        let o = Options { apps: vec!["teams".into(), "paint".into()], ..Default::default() };
        assert!(o.goes("MSTeams_24243.1309.3132.617_x64__8wekyb3d8bbwe"));
        assert!(o.goes("Microsoft.Paint_11.2410.28.0_neutral_~_8wekyb3d8bbwe"));
        assert!(!o.goes("Microsoft.BingNews_4.55.62231.0_x64__8wekyb3d8bbwe"));
        assert!(!o.goes("Microsoft.WindowsStore_22410.1401.5.0_neutral_~_8wekyb3d8bbwe"));
    }

    #[test]
    fn winget_ids_are_read() {
        assert_eq!(winget_ids("Mozilla.Firefox, VideoLAN.VLC;7zip.7zip\n"), vec!["Mozilla.Firefox", "VideoLAN.VLC", "7zip.7zip"]);
        assert!(valid_winget("Notepad++.Notepad++"));
        assert!(!valid_winget("a'; rm"));
    }

    #[test]
    fn dism_errors() {
        let t = "Deployment Image Servicing and Management tool\r\n[==   5.0%  ]\r\nError: 87\r\n\r\nThe export-image option is unknown.\r\n\r\nThe DISM log file can be found at C:\\Windows\\Logs\\DISM\\dism.log\r\n";
        assert_eq!(dism_error(87, t), "DISM 0x57: The export-image option is unknown.");
    }
}
