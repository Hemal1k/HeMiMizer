//! The list of what HeMimizer does, in the order it is done, and the worker that does it.
//! After the graphics driver (when a driver was installed) the PC restarts and HeMimizer continues
//! by itself after the sign-in (a scheduled task "HeMimizer – continue", run as administrator).

use crate::i18n::tr;
use crate::{drivers, programs, sys, windows};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;

pub type Res = Result<String, String>;

/// The error of a step that needs the internet while the PC is still offline (its installer is not in the backup):
/// it runs again after the step "Connect to the internet".
pub const NEEDS_INTERNET: &str = "needs the internet";

/// What a step belongs to (only for reading the list; the window shows one step at a time).
#[derive(Clone, Copy, PartialEq)]
pub enum Group {
    Programs,
    Drivers,
    Windows,
    Cleanup,
}

pub struct Step {
    /// Never changes: the steps left after a restart are saved by it.
    pub id: &'static str,
    #[allow(dead_code)]
    pub group: Group,
    pub title: (&'static str, &'static str),
    pub detail: (&'static str, &'static str),
    pub run: fn(&Ctx) -> Res,
    /// Restart here when this or an earlier step needs it, and continue after the sign-in.
    pub restart_after: bool,
}

impl Step {
    pub fn title(&self) -> &'static str {
        tr(self.title.0, self.title.1)
    }
    pub fn detail(&self) -> &'static str {
        tr(self.detail.0, self.detail.1)
    }
}

macro_rules! step {
    ($id:expr, $g:ident, $te:expr, $tc:expr, $de:expr, $dc:expr, $f:expr) => {
        Step { id: $id, group: Group::$g, title: ($te, $tc), detail: ($de, $dc), run: $f, restart_after: false }
    };
}

pub fn all() -> Vec<Step> {
    let mut v = vec![
        step!("7zip", Programs, "7-Zip", "7-Zip", "The newest 64-bit version; opens archives", "Nejnovější 64bit verze; otevírá archivy", programs::seven_zip),
        step!(
            "vlc",
            Programs,
            "VLC media player",
            "VLC media player",
            "Without shortcuts, web plug-ins and context menus; the skin and settings from the backup",
            "Bez zástupců, webových modulů a místních nabídek; skin a nastavení ze zálohy",
            programs::vlc
        ),
        step!(
            "brave",
            Programs,
            "Brave",
            "Brave",
            "The settings and extensions from the backup; only ad blocking – Leo AI, crypto, VPN, Tor, News… off",
            "Nastavení a rozšíření ze zálohy; jen blokování reklam – Leo AI, krypto, VPN, Tor, News… vypnuté",
            programs::brave
        ),
        step!("steam", Programs, "Steam", "Steam", "Without a desktop shortcut", "Bez zástupce na ploše", programs::steam),
        step!(
            "qbittorrent",
            Programs,
            "qBittorrent",
            "qBittorrent",
            "Start menu, firewall rule, long paths; the settings from the backup",
            "Nabídka Start, pravidlo firewallu, dlouhé cesty; nastavení ze zálohy",
            programs::qbittorrent
        ),
        step!(
            "runtimes",
            Programs,
            "Runtimes for games and apps",
            "Knihovny pro hry a aplikace",
            "Visual C++ 2005–2026 (x64 + x86), DirectX, .NET Desktop Runtime",
            "Visual C++ 2005–2026 (x64 + x86), DirectX, .NET Desktop Runtime",
            programs::runtimes
        ),
        step!("restore-point", Windows, "Restore point", "Bod obnovení", "A restore point before anything is changed", "Bod obnovení, než se cokoli změní", crate::debloat::restore_point),
        step!(
            "microsoft-apps",
            Cleanup,
            "Apps of Microsoft not needed",
            "Nepotřebné aplikace Microsoftu",
            "Clipchamp, News, Weather, Solitaire, Teams (personal), new Outlook, To Do, Tips, Feedback Hub, Maps, Phone Link…",
            "Clipchamp, Zprávy, Počasí, Solitaire, Teams (osobní), nový Outlook, To Do, Tipy, Centrum Feedback, Mapy, Propojení s telefonem…",
            crate::debloat::microsoft_apps
        ),
        step!("xbox-apps", Cleanup, "Xbox apps", "Aplikace Xbox", "The Xbox app, Game Bar and their helpers – keep them for games from the Xbox app or Game Pass", "Aplikace Xbox, herní panel a jejich pomocníci", crate::debloat::xbox_apps),
        step!(
            "ad-software",
            Cleanup,
            "Trial and advertising software",
            "Zkušební a reklamní software",
            "McAfee, Norton, Avast, AVG, ExpressVPN, WildTangent, Candy Crush, Booking, TikTok and similar, when the maker put them in",
            "McAfee, Norton, Avast, AVG, ExpressVPN, WildTangent, Candy Crush, Booking, TikTok a podobné, když je přidal výrobce",
            crate::debloat::ad_software
        ),
        step!("hibernation", Windows, "Hibernation off", "Vypnout hibernaci", "Frees the space of hiberfil.sys (several GB); Fast startup goes with it", "Uvolní místo souboru hiberfil.sys (několik GB); zmizí i Rychlé spuštění", crate::debloat::hibernation_off),
        step!(
            "folders",
            Windows,
            "Your folders",
            "Tvoje složky",
            "The folders chosen in the backup, put back to their places",
            "Složky vybrané při záloze, vrácené na svá místa",
            restore_folders
        ),
        step!(
            "programs",
            Programs,
            "Programs from the wizard",
            "Programy z průvodce",
            "The drivers, tools, browser, programs and HeMi apps chosen – from the backup or through winget",
            "Ovladače, nástroje, prohlížeč, programy a HeMi aplikace z průvodce – ze zálohy nebo přes winget",
            crate::install::programs
        ),
        step!(
            "drives",
            Programs,
            "Network drives",
            "Síťové disky",
            "The network drives from the backup; the password is asked once and saved",
            "Síťové disky ze zálohy; heslo se zadá jednou a uloží",
            programs::network_drives
        ),
        step!(
            "hemidock",
            Programs,
            "HeMiDock",
            "HeMiDock",
            "The newest version from GitHub (its installer adds the .NET and Windows App Runtime)",
            "Nejnovější verze z GitHubu (instalátor doplní .NET a Windows App Runtime)",
            programs::hemidock
        ),
        step!(
            "no-wu-drivers",
            Drivers,
            "No drivers from Windows Update",
            "Žádné ovladače z Windows Update",
            "Also no Razer Synapse and other device apps installed automatically",
            "Ani Razer Synapse a jiné aplikace zařízení se nenainstalují samy",
            drivers::no_driver_updates
        ),
        step!(
            "chipset",
            Drivers,
            "AMD chipset driver",
            "Ovladač čipsetu AMD",
            "Offline from the installer next to HeMimizer.exe, else the newest from amd.com",
            "Offline z instalačky vedle HeMimizer.exe, jinak nejnovější z amd.com",
            drivers::chipset
        ),
        step!(
            "gpu",
            Drivers,
            "AMD graphics driver",
            "Ovladač grafiky AMD",
            "Offline from the installer next to HeMimizer.exe, else from amd.com (~1 GB); then a restart, HeMimizer continues",
            "Offline z instalačky vedle HeMimizer.exe, jinak z amd.com (~1 GB); pak restart, HeMimizer pokračuje",
            drivers::gpu
        ),
        step!(
            "offline",
            Programs,
            "Installers from the backup",
            "Instalačky ze zálohy",
            "The installers the backup downloaded are copied to this PC – the programs are installed offline",
            "Instalačky stažené zálohou se zkopírují do PC – programy se nainstalují offline",
            crate::downloads::import_offline
        ),
        step!(
            "downloads",
            Programs,
            "Download the installers",
            "Stažení instalaček",
            "Everything to install is downloaded first; what fails can be chosen by hand",
            "Nejdřív se stáhne vše k instalaci; co se nepovede, můžeš vybrat ručně",
            crate::downloads::download_all
        ),
        step!(
            "store",
            Windows,
            "Microsoft Store updates",
            "Aktualizace Microsoft Store",
            "Store looks for updates of its apps and downloads them in the background",
            "Store vyhledá aktualizace svých aplikací a stáhne je na pozadí",
            windows::store_updates
        ),
        step!(
            "internet",
            Drivers,
            "Connect to the internet",
            "Připojení k internetu",
            "Waits until you connect the PC to the internet, then goes on",
            "Počká, až PC připojíš k internetu, a pak pokračuje",
            drivers::internet
        ),
        step!(
            "gpu-settings",
            Drivers,
            "AMD graphics settings",
            "Nastavení grafiky AMD",
            "The Adrenalin settings from the backup; the OC profile is saved to Documents\\HeMimizer",
            "Nastavení Adrenalinu ze zálohy; OC profil se uloží do Dokumenty\\HeMimizer",
            drivers::gpu_settings
        ),
        step!("display", Drivers, "Monitor", "Monitor", "The highest refresh rate of each monitor; the scaling from the backup", "Nejvyšší obnovovací frekvence každého monitoru; měřítko ze zálohy", windows::display),
        step!(
            "features",
            Windows,
            "Windows features",
            "Funkce systému Windows",
            ".NET 3.5, .NET 4.8 Advanced Services, SMB Direct on; the rest off, Remote Desktop Connection uninstalled",
            ".NET 3.5, .NET 4.8 Advanced Services, SMB Direct zapnuto; zbytek vypnut, Připojení ke vzdálené ploše odinstalováno",
            windows::features
        ),
        step!(
            "wallpaper",
            Windows,
            "Wallpaper",
            "Tapeta",
            "Spotlight off; your wallpaper on the desktop and the lock screen, no tips",
            "Spotlight vypnutý; tvoje tapeta na ploše i zamykací obrazovce, bez tipů",
            windows::wallpaper
        ),
        step!(
            "explorer",
            Windows,
            "File Explorer view",
            "Zobrazení Průzkumníka",
            "The view of File Explorer from the backup",
            "Zobrazení Průzkumníka ze zálohy",
            windows::explorer
        ),
        step!(
            "tweaks",
            Windows,
            "Windows settings",
            "Nastavení Windows",
            "The settings of Windows chosen in the wizard",
            "Nastavení Windows zvolená v průvodci",
            crate::tweaks::run
        ),
        step!(
            "app-settings",
            Programs,
            "Settings of your programs",
            "Nastavení tvých programů",
            "The settings of the programs, from the backup",
            "Nastavení programů ze zálohy",
            crate::appdata::restore
        ),
        step!(
            "windows-update",
            Windows,
            "Windows Update",
            "Windows Update",
            "Installs all updates except drivers; other Microsoft products on, downloads from other PCs off",
            "Nainstaluje všechny aktualizace kromě ovladačů; další produkty Microsoftu zapnuté, stahování z jiných PC vypnuté",
            windows::windows_update
        ),
        step!("notepad", Windows, "Notepad", "Poznámkový blok", "The settings from the backup", "Nastavení ze zálohy", windows::notepad),
        step!("calculator", Windows, "Calculator", "Kalkulačka", "Uninstalled", "Odinstalovat", windows::calculator),
        step!(
            "restore",
            Windows,
            "System Restore off",
            "Vypnout obnovu systému",
            "No restore points; the existing ones are deleted",
            "Žádné body obnovení; ty existující se smažou",
            windows::restore_off
        ),
        step!(
            "appearance",
            Windows,
            "Colours and language",
            "Barvy a jazyk",
            "Dark mode and the accent colour from the backup",
            "Tmavý režim a barva zvýraznění ze zálohy",
            windows::appearance
        ),
        step!(
            "defender",
            Windows,
            "Microsoft Defender",
            "Microsoft Defender",
            "The settings from the backup and memory integrity; exclusions for the Steam libraries",
            "Nastavení ze zálohy a integrita paměti; výjimky pro knihovny Steamu",
            windows::defender
        ),
        step!(
            "startup",
            Windows,
            "Startup apps",
            "Aplikace při spuštění",
            "Only HeMiDock starts with Windows; Steam and the others off (can be turned on in Task Manager)",
            "S Windows startuje jen HeMiDock; Steam a ostatní vypnuté (jdou zapnout ve Správci úloh)",
            windows::startup_apps
        ),
        step!(
            "pins",
            Windows,
            "Pinned apps",
            "Připnuté aplikace",
            "Start and the taskbar pinned as in the backup",
            "Start a hlavní panel připnuté jako v záloze",
            windows::pins
        ),
        step!("pc-name", Windows, "PC name", "Název PC", "The name of the PC", "Název počítače", windows::rename_pc),
        step!(
            "cleanup",
            Cleanup,
            "Clean up",
            "Vyčistit",
            "Temporary files, component store, Disk Clean-up with all items, at last the downloaded updates",
            "Dočasné soubory, úložiště součástí, Vyčištění disku se všemi položkami, nakonec stažené aktualizace",
            windows::cleanup
        ),
    ];
    // the order of the work (2026-11-14, the author's wish): the network drives first (the backup is on the NAS);
    // then everything that works offline – the drivers from the backup and a restart, the programs from the
    // installers of the backup, the settings of Windows; only then HeMimizer asks to connect the internet and does
    // what needs it: the installers missing from the backup (their steps were put off), HeMiDock from GitHub, the
    // Microsoft Store, Windows features (.NET 3.5 comes from Windows Update) and Windows Update – again after every
    // restart until nothing is left –, and the clean-up last (it also deletes the downloaded updates)
    const ORDER: &[&str] = &[
        "restore-point", "offline", "drives", "no-wu-drivers", "chipset", "gpu", "gpu-settings", "display", "7zip", "vlc", "brave", "steam", "qbittorrent", "programs",
        "app-settings", "runtimes", "appearance", "wallpaper", "explorer", "tweaks", "defender", "notepad", "calculator",
        "hibernation", "restore", "startup", "pins", "folders", "pc-name", "microsoft-apps", "xbox-apps", "ad-software", "internet", "downloads", "hemidock", "store", "features", "windows-update", "cleanup",
    ];
    v.sort_by_key(|s| ORDER.iter().position(|o| *o == s.id).unwrap_or(usize::MAX));
    for s in v.iter_mut() {
        s.restart_after = s.id == "gpu" || s.id == "windows-update";
    }
    v
}

/// The own folders of the backup back to their places (files there already are overwritten by the backup).
fn restore_folders(c: &Ctx) -> Res {
    let plan = crate::plan::Plan::load().unwrap_or_default();
    if plan.folders.is_empty() {
        return Ok("None chosen".into());
    }
    let mut failed = Vec::new();
    for (n, f) in plan.folders.iter().enumerate() {
        let from = crate::backup::folders_dir().join((n + 1).to_string());
        let to = crate::plan::resolve_folder(f);
        if !from.is_dir() {
            failed.push(format!("{}: not in the backup", to.display()));
            continue;
        }
        c.log(format!("Copying back {}", to.display()));
        if let Err(e) = crate::backup::robocopy(&from, &to) {
            failed.push(format!("{}: {e}", to.display()));
        }
    }
    let text = format!("{} folders", plan.folders.len() - failed.len());
    if failed.is_empty() { Ok(text) } else { Err(format!("{text}\n{}", failed.join("\n"))) }
}

/// Windows Update runs again after its restart (until it finds nothing); at most this many rounds.
const WU_ROUNDS: u32 = 6;

// ---------------------------------------------------------------- the worker

pub enum Event {
    Start(usize),
    Log(String),
    Note(String),
    Done(usize, Res),
    /// The step needs the internet: it runs again after it is connected (it is not counted as done).
    Later(usize),
    /// HeMimizer waits for the internet (true) / is connected again (false).
    Internet(bool),
    /// Downloads failed: (id, name, error) – the window asks for the files or another try.
    NeedFiles(Vec<(String, String, String)>),
    /// `resume`: the rest continues after the restart (the PC should restart now).
    Finished { reboot: bool, resume: bool },
}

pub struct Ctx {
    tx: Sender<Event>,
    reboot: AtomicBool,
    /// (server, user, password) typed in at the start – only kept in memory.
    passwords: Vec<(String, String, String)>,
}

impl Ctx {
    pub fn log(&self, text: impl Into<String>) {
        let text = text.into();
        self.write(&text);
        let _ = self.tx.send(Event::Log(text));
    }
    /// Something the user must do by hand, shown at the end (and kept for the to-do file over the restarts).
    pub fn note(&self, text: impl Into<String>) {
        let text = text.into();
        self.write(&text);
        todo_add(&format!("- {text}"));
        let _ = self.tx.send(Event::Note(text));
    }
    pub fn need_files(&self, failed: Vec<(String, String, String)>) {
        let _ = self.tx.send(Event::NeedFiles(failed));
    }
    pub fn waiting_for_internet(&self, on: bool) {
        let _ = self.tx.send(Event::Internet(on));
    }
    pub fn reboot(&self) {
        self.reboot.store(true, Ordering::Relaxed);
    }
    pub fn passwords(&self) -> Vec<(String, String, String)> {
        self.passwords.clone()
    }
    fn write(&self, text: &str) {
        sys::log(text);
    }
}

fn data_dir() -> PathBuf {
    PathBuf::from(sys::env("LOCALAPPDATA", "")).join("HeMimizer")
}

fn resume_file() -> PathBuf {
    data_dir().join("resume.txt")
}

/// What is left to do by hand, collected over all the runs between the restarts.
fn todo_file() -> PathBuf {
    data_dir().join("todo.txt")
}

fn todo_add(line: &str) {
    use std::io::Write;
    let _ = std::fs::create_dir_all(data_dir());
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(todo_file()) {
        let _ = writeln!(f, "{line}");
    }
}

/// The folder HeMimizer.exe was started from at first (kept over the restarts, when the copy in
/// %LOCALAPPDATA%\HeMimizer\app runs): the log and the to-do file stay there.
pub fn origin_file() -> PathBuf {
    data_dir().join("origin.txt")
}

/// At the very end: "HeMimizer - udělat ručně.txt" next to the log.
fn write_todo() -> Option<PathBuf> {
    let dir = sys::log_path().parent()?.to_path_buf();
    let mut t = String::new();
    t.push_str(tr("HeMimizer – left to do by hand\r\n\r\n", "HeMimizer – zbývá udělat ručně\r\n\r\n"));
    let apps = crate::catalog::default_apps(&crate::plan::Plan::load().unwrap_or_default());
    if !apps.is_empty() {
        t.push_str(&format!(
            "- Default apps: Settings → Apps → Default apps → {} → Set default (Windows does not let programs choose them).\r\n",
            apps.join(", ")
        ));
    }
    if let Ok(s) = std::fs::read_to_string(todo_file()) {
        for l in s.lines().filter(|l| !l.trim().is_empty()) {
            t.push_str(l);
            t.push_str("\r\n");
        }
    }
    t.push_str(tr("\r\nThe details are in HeMimizer.log.\r\n", "\r\nPodrobnosti jsou v HeMimizer.log.\r\n"));
    let file = dir.join(tr("HeMimizer - to do by hand.txt", "HeMimizer - udělat ručně.txt"));
    let mut bytes = vec![0xEF, 0xBB, 0xBF];
    bytes.extend_from_slice(t.as_bytes());
    std::fs::write(&file, bytes).ok()?;
    Some(file)
}

/// A new run from the start (not after a restart): nothing left over from an earlier, unfinished one.
pub fn fresh_start() {
    for f in [todo_file(), origin_file(), data_dir().join("wu-rounds.txt")] {
        let _ = std::fs::remove_file(f);
    }
}

/// Is `path` on a fixed disk (not a USB stick, not a network drive)?
fn on_fixed_disk(path: &std::path::Path) -> bool {
    use windows_sys::Win32::Storage::FileSystem::GetDriveTypeW;
    let s = path.display().to_string();
    let Some(root) = s.get(..3).filter(|r| r.ends_with(":\\")) else { return false };
    let w = sys::wide(root);
    unsafe { GetDriveTypeW(w.as_ptr()) == 3 }
}

/// When the window closes after the last run: HeMimizer deletes itself, its config, its copy for the restarts and
/// its temporary files. Only HeMimizer.log and the to-do file stay. On a USB stick the exe stays (it is the kit for
/// the next installation). Done by a hidden cmd a few seconds after HeMimizer has ended.
pub fn remove_myself() {
    let exe = std::env::current_exe().unwrap_or_default();
    let origin = std::fs::read_to_string(origin_file()).ok().map(|s| PathBuf::from(s.trim())).filter(|p| p.exists());
    let mut del: Vec<String> = Vec::new();
    let mut dirs: Vec<PathBuf> = vec![std::env::temp_dir().join("HeMimizer"), sys::work_dir()];
    let original = origin.clone().unwrap_or_else(|| exe.parent().map(PathBuf::from).unwrap_or_default());
    if on_fixed_disk(&original) {
        del.push(original.join("HeMimizer.exe").display().to_string());
        // the folder of HeMimizer except the log and the to-do file
        dirs.push(original.join(crate::config::HOME).join("config"));
        dirs.push(original.join(crate::config::HOME).join("installers"));
    }
    // the folder in %LOCALAPPDATA% (resume, origin, to-do, the copy for the restarts) – unless the log is in it
    if !sys::log_path().starts_with(data_dir()) {
        dirs.push(data_dir());
    } else {
        dirs.push(data_dir().join("app"));
    }
    let mut cmd = String::from("ping -n 4 127.0.0.1 >nul");
    for f in del {
        cmd.push_str(&format!(" & del /f /q \"{f}\" 2>nul"));
    }
    for d in dirs {
        cmd.push_str(&format!(" & rmdir /s /q \"{}\" 2>nul", d.display()));
    }
    sys::log(&format!("removing HeMimizer: {cmd}"));
    use std::os::windows::process::CommandExt;
    let _ = std::process::Command::new("cmd.exe").raw_arg(format!("/c {cmd}")).creation_flags(0x0800_0000 | 0x0000_0008).spawn();
}

const TASK: &str = "HeMimizer - continue";

/// The ids of the steps left for after the restart (written by the run before it).
pub fn pending_resume() -> Vec<String> {
    std::fs::read_to_string(resume_file()).map(|t| t.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect()).unwrap_or_default()
}

pub fn clear_resume() {
    let _ = std::fs::remove_file(resume_file());
    let _ = sys::exec("schtasks.exe", &["/delete", "/tn", TASK, "/f"]);
}

/// Saves the steps that are left and schedules HeMimizer to continue after the next sign-in. HeMimizer and its
/// config folder are copied to %LOCALAPPDATA%\HeMimizer\app first – a USB stick or a network drive may be gone then.
fn schedule_resume(ids: &[&str]) -> Result<(), String> {
    let app = data_dir().join("app");
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let target = app.join("HeMimizer.exe");
    if exe != target {
        let _ = std::fs::remove_dir_all(&app);
        std::fs::create_dir_all(app.join(crate::config::HOME).join("config")).map_err(|e| e.to_string())?;
        std::fs::copy(&exe, &target).map_err(|e| e.to_string())?;
        if let Ok(rd) = std::fs::read_dir(crate::config::dir()) {
            for e in rd.flatten() {
                let _ = std::fs::copy(e.path(), app.join(crate::config::HOME).join("config").join(e.file_name()));
            }
        }
    }
    std::fs::write(resume_file(), ids.join("\r\n")).map_err(|e| e.to_string())?;
    if !origin_file().exists() {
        if let Some(d) = exe.parent() {
            let _ = std::fs::write(origin_file(), d.display().to_string());
        }
    }
    // an interactive task of this user needs no password; "highest" = as administrator without UAC
    sys::ps(&format!(
        "$u=[Security.Principal.WindowsIdentity]::GetCurrent().Name; \
         $a=New-ScheduledTaskAction -Execute '{exe}' -Argument '--resume'; \
         $t=New-ScheduledTaskTrigger -AtLogOn -User $u; \
         $p=New-ScheduledTaskPrincipal -UserId $u -LogonType Interactive -RunLevel Highest; \
         $s=New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -ExecutionTimeLimit (New-TimeSpan -Hours 0); \
         Register-ScheduledTask -TaskName '{TASK}' -Action $a -Trigger $t -Principal $p -Settings $s -Force | Out-Null",
        exe = target.display()
    ))?;
    Ok(())
}

/// Runs the chosen steps one after another on its own thread.
pub fn run(chosen: Vec<usize>, passwords: Vec<(String, String, String)>, tx: Sender<Event>, repaint: impl Fn() + Send + 'static) {
    std::thread::spawn(move || {
        let steps = all();
        let _ = std::fs::create_dir_all(data_dir());
        let ctx = Ctx { tx: tx.clone(), reboot: AtomicBool::new(false), passwords };
        ctx.write(&format!("==== Run: {} ====", chosen.iter().map(|&i| steps[i].id).collect::<Vec<_>>().join(", ")));
        clear_resume();
        windows::self_exclusion(true);
        let mut resume = false;
        let mut chosen = chosen;
        let mut k = 0;
        while k < chosen.len() {
            let i = chosen[k];
            let s = &steps[i];
            let _ = tx.send(Event::Start(i));
            ctx.write(&format!("---------- STEP {} ({})", s.id, s.title.0));
            repaint();
            let started = std::time::Instant::now();
            // a step of Windows switched off in the wizard: nothing to do
            if !crate::options::runs(s.id) {
                ctx.write(&format!("---------- SKIP {} (not chosen)", s.id));
                let _ = tx.send(Event::Done(i, Ok("Not chosen".into())));
                repaint();
                k += 1;
                continue;
            }
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (s.run)(&ctx)))
                .unwrap_or_else(|_| Err(tr("The step crashed.", "Krok spadl.").into()));
            let secs = started.elapsed().as_secs();
            // offline and its installer is not in the backup: again after the internet (and the downloads) – when
            // that is still ahead; else it is an error like any other
            if r.as_ref().err().is_some_and(|e| e == NEEDS_INTERNET) {
                let ahead = |id: &str| chosen[k + 1..].iter().position(|&j| steps[j].id == id).map(|p| k + 1 + p);
                if let Some(at) = ahead("downloads").or_else(|| ahead("internet")) {
                    ctx.write(&format!("---------- LATER {} (offline, after the internet)", s.id));
                    chosen.insert(at + 1, i);
                    chosen.remove(k);
                    let _ = tx.send(Event::Later(i));
                    repaint();
                    continue;
                }
            }
            match &r {
                Ok(m) => ctx.write(&format!("---------- OK {} in {secs} s: {m}", s.id)),
                Err(e) => {
                    ctx.write(&format!("---------- ERROR {} in {secs} s: {e}", s.id));
                    todo_add(&format!("- {} {}: {e}", tr("Failed:", "Selhalo:"), s.title()));
                }
            }
            let _ = tx.send(Event::Done(i, r));
            repaint();
            let mut rest: Vec<&str> = chosen[k + 1..].iter().map(|&j| steps[j].id).collect();
            // Windows Update again after its restart, until it finds nothing (and nothing waits for a restart)
            if s.id == "windows-update" && ctx.reboot.load(Ordering::Relaxed) {
                let rounds_file = data_dir().join("wu-rounds.txt");
                let rounds: u32 = std::fs::read_to_string(&rounds_file).ok().and_then(|t| t.trim().parse().ok()).unwrap_or(0) + 1;
                let _ = std::fs::write(&rounds_file, rounds.to_string());
                if rounds < WU_ROUNDS {
                    rest.insert(0, s.id);
                }
            }
            if s.restart_after && ctx.reboot.load(Ordering::Relaxed) && !rest.is_empty() {
                match schedule_resume(&rest) {
                    Ok(()) => {
                        ctx.write("   restart, then continue");
                        resume = true;
                        break;
                    }
                    Err(e) => ctx.note(format!("{}: {e}", tr("Continuing after a restart could not be set up", "Pokračování po restartu nešlo nastavit"))),
                }
            }
            k += 1;
        }
        if !resume {
            let _ = std::fs::remove_dir_all(sys::work_dir());
            windows::self_exclusion(false);
            let _ = std::fs::remove_file(data_dir().join("wu-rounds.txt"));
            if let Some(f) = write_todo() {
                ctx.write(&format!("to-do file: {}", f.display()));
            }
            let _ = std::fs::remove_file(todo_file());
        }
        let _ = tx.send(Event::Finished { reboot: ctx.reboot.load(Ordering::Relaxed), resume });
        repaint();
    });
}

/// The newest download link of every program and driver: (name, link or error).
pub fn check_links() -> Vec<(String, Result<String, String>)> {
    let checks: [(&str, fn() -> Result<String, String>); 7] = [
        ("7-Zip", programs::url_7zip),
        ("VLC", programs::url_vlc),
        ("Brave", || programs::brave_version().map(|v| format!("Brave {v}")).ok_or_else(|| "GitHub".to_string())),
        ("Steam", programs::url_steam),
        ("qBittorrent", programs::url_qbittorrent),
        ("AMD chipset", drivers::url_chipset),
        ("AMD Adrenalin", drivers::url_gpu),
    ];
    let mut out: Vec<(String, Result<String, String>)> = std::thread::scope(|s| {
        let hs: Vec<_> = checks.iter().map(|(n, f)| (n.to_string(), s.spawn(f))).collect();
        hs.into_iter().map(|(n, h)| (n, h.join().unwrap_or_else(|_| Err("?".into())))).collect()
    });
    out.push((".NET Desktop Runtime".into(), programs::dotnet_versions().map(|v| v.join(", "))));
    // the link itself is long; the file name (with the version) says enough
    for (_, r) in out.iter_mut() {
        if let Ok(u) = r {
            let name = u.split('?').next().unwrap_or(u).trim_end_matches("/download").rsplit('/').next().unwrap_or(u).to_string();
            *u = name;
        }
    }
    out
}
