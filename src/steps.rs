//! The list of what HeMiMizer does, in the order it is done, and the worker that does it.
//! After the graphics driver (when a driver was installed) the PC restarts and HeMiMizer continues
//! by itself after the sign-in (a scheduled task "HeMiMizer - continue", run as administrator).

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
            "Unneeded Microsoft apps",
            "Nepotřebné aplikace Microsoftu",
            "Clipchamp, News, Weather, Solitaire, Teams (personal), new Outlook, To Do, Tips, Feedback Hub, Maps, Phone Link…",
            "Clipchamp, Zprávy, Počasí, Solitaire, Teams (osobní), nový Outlook, To Do, Tipy, Centrum Feedback Hub, Mapy, Propojení s telefonem…",
            crate::debloat::microsoft_apps
        ),
        step!("xbox-apps", Cleanup, "Xbox apps", "Aplikace Xbox", "The Xbox app, Game Bar and their helpers – keep them for games from the Xbox app or Game Pass", "Aplikace Xbox, Herní panel a jejich doplňky – nech je, pokud hraješ hry z aplikace Xbox nebo Game Pass", crate::debloat::xbox_apps),
        step!(
            "ad-software",
            Cleanup,
            "Trial and advertising software",
            "Zkušební a reklamní software",
            "Candy Crush, TikTok, Booking, Netflix and similar apps only when the maker put them into Windows (yours from the Store stay); ExpressVPN and WildTangent by their exact winget id; antivirus programs are only named, never removed",
            "Candy Crush, TikTok, Booking, Netflix a podobné aplikace, jen když je do Windows dal výrobce (tvoje ze Storu zůstanou); ExpressVPN a WildTangent podle přesného ID ve wingetu; antiviry se jen vypíšou, nikdy neodeberou",
            crate::debloat::ad_software
        ),
        step!("hibernation", Windows, "Hibernation off", "Vypnout hibernaci", "Frees the space hiberfil.sys takes (several GB); Fast startup goes with it", "Uvolní místo souboru hiberfil.sys (několik GB); zmizí i Rychlé spuštění", crate::debloat::hibernation_off),
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
            "The network drives from the backup; the password is asked once and saved. Their servers (only those) go into the Local intranet zone, so files from them open without a security warning; programs run as administrator see the drives too (EnableLinkedConnections)",
            "Síťové disky ze zálohy; heslo se zadá jednou a uloží. Jejich servery (jen ty) se přidají do zóny Místní intranet, takže se soubory z nich otevřou bez bezpečnostního upozornění; disky uvidí i programy spuštěné jako správce (EnableLinkedConnections)",
            programs::network_drives
        ),
        step!(
            "hemidock",
            Programs,
            "HeMiDock",
            "HeMiDock",
            "The newest version from GitHub (its installer adds .NET and the Windows App Runtime)",
            "Nejnovější verze z GitHubu (instalátor doplní .NET a Windows App Runtime)",
            programs::hemidock
        ),
        step!(
            "no-wu-drivers",
            Drivers,
            "No drivers from Windows Update",
            "Žádné ovladače z Windows Update",
            "Razer Synapse and other device apps are not installed automatically either",
            "Ani Razer Synapse a jiné aplikace zařízení se nenainstalují samy",
            drivers::no_driver_updates
        ),
        step!(
            "chipset",
            Drivers,
            "AMD chipset driver",
            "Ovladač čipsetu AMD",
            "Offline from the installer next to HeMiMizer.exe, otherwise the newest from amd.com",
            "Offline z instalačního programu vedle HeMiMizer.exe, jinak nejnovější z amd.com",
            drivers::chipset
        ),
        step!(
            "gpu",
            Drivers,
            "AMD graphics driver",
            "Ovladač grafiky AMD",
            "Offline from the installer next to HeMiMizer.exe, otherwise from amd.com (~1 GB); then a restart, HeMiMizer continues",
            "Offline z instalačního programu vedle HeMiMizer.exe, jinak z amd.com (~1 GB); pak restart, HeMiMizer pokračuje",
            drivers::gpu
        ),
        step!(
            "offline",
            Programs,
            "Installers from the backup",
            "Instalační programy ze zálohy",
            "The installers the backup downloaded are copied to this PC – the programs are installed offline",
            "Instalační programy stažené zálohou se zkopírují do PC – programy se nainstalují offline",
            crate::downloads::import_offline
        ),
        step!(
            "downloads",
            Programs,
            "Download the installers",
            "Stažení instalačních programů",
            "Everything to install is downloaded first; anything that fails can be chosen by hand",
            "Nejdřív se stáhne vše k instalaci; co se nepovede, můžeš vybrat ručně",
            crate::downloads::download_all
        ),
        step!(
            "store",
            Windows,
            "Microsoft Store updates",
            "Aktualizace Microsoft Store",
            "Microsoft Store looks for updates to its apps and downloads them in the background",
            "Microsoft Store vyhledá aktualizace svých aplikací a stáhne je na pozadí",
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
            "The Adrenalin settings from the backup; the OC profile is saved to Documents\\HeMiMizer",
            "Nastavení Adrenalinu ze zálohy; OC profil se uloží do Dokumenty\\HeMiMizer",
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
            "The File Explorer view from the backup",
            "Zobrazení Průzkumníka ze zálohy",
            windows::explorer
        ),
        step!(
            "tweaks",
            Windows,
            "Windows settings",
            "Nastavení Windows",
            "The Windows settings chosen in the wizard",
            "Nastavení Windows zvolená v průvodci",
            crate::tweaks::run
        ),
        step!(
            "app-settings",
            Programs,
            "Settings of your programs",
            "Nastavení tvých programů",
            "Program settings from the backup",
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
        step!("mouse-settings", Windows, "Mouse", "Myš", "Pointer speed, scrolling and double-click speed from the backup", "Rychlost ukazatele, posouvání a dvojklik ze zálohy", windows::mouse),
        step!("wifi", Windows, "Wi-Fi networks", "Sítě Wi-Fi", "The saved networks from the backup, with their passwords", "Uložené sítě ze zálohy i s hesly", windows::wifi),
        step!("fonts", Windows, "Fonts", "Písma", "The fonts installed on the old PC, from the backup", "Písma nainstalovaná na starém PC, ze zálohy", windows::fonts),
        step!(
            "appearance",
            Windows,
            "Colors",
            "Barvy",
            "Dark mode and the accent color from the backup",
            "Tmavý režim a barva zvýraznění ze zálohy",
            windows::appearance
        ),
        step!(
            "defender",
            Windows,
            "Microsoft Defender",
            "Microsoft Defender",
            "The settings from the backup and memory integrity – not the ones that would switch protection off; exclusions for the Steam libraries and, when chosen, qBittorrent",
            "Nastavení ze zálohy a integrita paměti – ne ta, která by ochranu vypnula; výjimky pro knihovny Steamu a qBittorrent, pokud je vybraný",
            windows::defender
        ),
        step!(
            "startup",
            Windows,
            "Other programs do not start with Windows",
            "Ostatní programy se nespouštějí s Windows",
            "OneDrive and what else is in the startup list – the new programs follow the table on the last page (they can be turned on in Task Manager)",
            "OneDrive a další v seznamu spouštění – nové programy podle tabulky na poslední stránce (jdou zapnout ve Správci úloh)",
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
            "shortcuts",
            Programs,
            "Shortcuts and startup",
            "Zástupci a spouštění",
            "Desktop, Start, taskbar and starting with Windows for the new programs, as chosen",
            "Plocha, Start, hlavní panel a spouštění nových programů podle volby",
            crate::shortcuts::run
        ),
        step!(
            "cleanup",
            Cleanup,
            "Clean up",
            "Vyčistit",
            "Temporary files, component store, Disk Cleanup (not Downloads, the Recycle Bin, the previous Windows or file versions), and finally the downloaded updates",
            "Dočasné soubory, úložiště součástí, Vyčištění disku (bez Stažených souborů, Koše, předchozích Windows a verzí souborů), nakonec stažené aktualizace",
            windows::cleanup
        ),
    ];
    // the order of the work (2026-11-14, the author's wish): the network drives first (the backup is on the NAS);
    // then everything that works offline – the drivers from the backup and a restart, the programs from the
    // installers of the backup, the settings of Windows; only then HeMiMizer asks to connect the internet and does
    // what needs it: the installers missing from the backup (their steps were put off), HeMiDock from GitHub, the
    // Microsoft Store, Windows features (.NET 3.5 comes from Windows Update) and Windows Update – again after every
    // restart until nothing is left –, and the clean-up last (it also deletes the downloaded updates)
    const ORDER: &[&str] = &[
        "restore-point", "offline", "wifi", "drives", "no-wu-drivers", "chipset", "gpu", "gpu-settings", "display", "7zip", "vlc", "brave", "steam", "qbittorrent", "programs",
        "app-settings", "fonts", "runtimes", "appearance", "wallpaper", "explorer", "tweaks", "mouse-settings", "defender",
        "hibernation", "startup", "folders", "pc-name", "microsoft-apps", "xbox-apps", "ad-software", "internet", "downloads", "hemidock", "store", "pins", "shortcuts", "features", "windows-update", "cleanup",
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
        let Some(to) = crate::plan::safe_folder(f) else {
            failed.push(format!(
                "{}: {}",
                crate::plan::show_folder(f),
                tr("not put back – a place of Windows or of the programs, or not a plain folder path", "nevráceno – místo Windows nebo programů, nebo to není prostá cesta ke složce")
            ));
            continue;
        };
        if !from.is_dir() {
            failed.push(format!("{}: not in the backup", to.display()));
            continue;
        }
        c.log(format!("Copying back {}", to.display()));
        if let Err(e) = crate::backup::robocopy(&from, &to) {
            failed.push(format!("{}: {e}", to.display()));
        }
    }
    let text = crate::i18n::count(plan.folders.len() - failed.len(), ("folder", "folders"), ("složka", "složky", "složek"));
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
    /// HeMiMizer waits for the internet (true) / is connected again (false).
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

/// The state between the restarts (resume.txt, origin.txt…) and the copy for them: the protected folder
/// %ProgramData%\HeMiMizer, not %LOCALAPPDATA% – the scheduled task runs that copy as administrator, so no program of
/// the user may change it (audit 2026-11-24).
fn data_dir() -> PathBuf {
    if let Err(e) = sys::protect_state_dir() {
        sys::log(&format!("protected folder: {e}"));
    }
    sys::state_dir()
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

/// The folder HeMiMizer.exe was started from at first (kept over the restarts, when the copy in
/// %ProgramData%\HeMiMizer\app runs): a Debug build keeps its log and the to-do file there.
pub fn origin_file() -> PathBuf {
    data_dir().join("origin.txt")
}

/// At the very end: "HeMiMizer - to do by hand.txt" next to the log – only in a Debug build (the release writes no log,
/// so `log_path()` is empty and no file is written; the window shows what is left to do).
fn write_todo() -> Option<PathBuf> {
    let dir = sys::log_path().parent()?.to_path_buf();
    let mut t = String::new();
    t.push_str(tr("HeMiMizer – left to do by hand\r\n\r\n", "HeMiMizer – zbývá udělat ručně\r\n\r\n"));
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
    // only when a log is written (Debug, or Settings → Logging on)
    let log = sys::log_path();
    if !log.as_os_str().is_empty() {
        t.push_str(&format!("{} {}\r\n", tr("\r\nThe details are in", "\r\nPodrobnosti jsou v"), log.display()));
    }
    let file = dir.join(tr("HeMiMizer - to do by hand.txt", "HeMiMizer - udělat ručně.txt"));
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
    // SAFETY: `w` ends with a NUL
    unsafe { GetDriveTypeW(w.as_ptr()) == 3 }
}

/// The folder HeMiMizer was started from at first (also after the restarts, when its copy runs).
fn original_dir() -> PathBuf {
    let origin = std::fs::read_to_string(origin_file()).ok().map(|s| PathBuf::from(s.trim())).filter(|p| p.exists());
    origin.unwrap_or_else(crate::config::exe_dir)
}

/// The folder "HeMiMizer data" goes at the end (not on a USB stick or a network drive: there it is the kit for the
/// next installation).
pub fn removes_data() -> bool {
    on_fixed_disk(&original_dir())
}

/// When the window closes after the last run, nothing of HeMiMizer stays on the PC (the user, 2026-11-18): its folder
/// "HeMiMizer data", %ProgramData%\HeMiMizer (the copy for the restarts, the state between them, the downloads) and its
/// temporary files go right away. The exe that runs cannot be deleted while it runs: Windows deletes it at the next
/// restart (as installers do – no helper process waits for it). On a USB stick the exe and its folder stay (the kit for
/// the next installation). A Debug build keeps its log in %ProgramData%\HeMiMizer (the release writes none).
pub fn remove_myself() {
    let exe = std::env::current_exe().unwrap_or_default();
    let original = original_dir();
    let local = data_dir();
    let fixed = on_fixed_disk(&original);
    // installed by its installer: the program stays (it is removed by uninstalling it), only its data goes
    let installed = crate::selfupdate::installed_in(&original).is_some();
    sys::log(&format!("removing HeMiMizer (its folder too: {fixed}, installed: {installed})"));
    let home = original.join(crate::config::HOME);
    if cfg!(debug_assertions) && fixed {
        // Debug: the log into %ProgramData%\HeMiMizer before its folder goes
        sys::log_close();
        let _ = std::fs::create_dir_all(&local);
        if let Ok(rd) = std::fs::read_dir(&home) {
            for e in rd.flatten().filter(|e| e.path().extension().is_some_and(|x| x.eq_ignore_ascii_case("log") || x.eq_ignore_ascii_case("txt"))) {
                let _ = std::fs::copy(e.path(), local.join(e.file_name()));
            }
        }
    }
    // %LOCALAPPDATA%\HeMiMizer: where HeMiMizer up to 1.5.0 kept the copy for the restarts and its state
    let old_local = PathBuf::from(sys::env("LOCALAPPDATA", "")).join("HeMiMizer");
    for d in [std::env::temp_dir().join("HeMiMizer"), sys::work_dir(), local.join("app"), old_local.join("app")] {
        let _ = std::fs::remove_dir_all(d);
    }
    if !cfg!(debug_assertions) {
        let _ = std::fs::remove_dir_all(&old_local);
    }
    let _ = std::fs::remove_file(std::env::temp_dir().join("HeMiMizer-ThirdPartyNotices.txt"));
    if cfg!(debug_assertions) {
        for f in [todo_file(), origin_file(), resume_file(), local.join("wu-rounds.txt")] {
            let _ = std::fs::remove_file(f);
        }
    } else {
        let _ = std::fs::remove_dir_all(&local);
    }
    // a protected backup on a USB stick: only the sealed file stays there, not the opened copy
    if !fixed {
        crate::crypto::remove_open_parts(&home);
    }
    if fixed {
        let _ = std::fs::remove_dir_all(&home);
        let file = original.join("HeMiMizer.exe");
        if !installed && std::fs::remove_file(&file).is_err() && file.exists() {
            delete_at_restart(&file);
        }
    }
    // the copy for the restarts (or the exe itself) is still running: at the next restart, with its folders
    if exe.exists() && ((fixed && !(installed && exe.starts_with(&original))) || exe.starts_with(&local)) {
        delete_at_restart(&exe);
        if exe.starts_with(local.join("app")) {
            delete_at_restart(&local.join("app"));
            if !cfg!(debug_assertions) {
                delete_at_restart(&local);
            }
        }
    }
}

/// Windows deletes the file (or the empty folder) at the next restart.
fn delete_at_restart(path: &std::path::Path) {
    use windows_sys::Win32::Storage::FileSystem::{MOVEFILE_DELAY_UNTIL_REBOOT, MoveFileExW};
    let w = sys::wide(&path.display().to_string());
    // SAFETY: `w` ends with a NUL; a null target means "delete at the restart"
    unsafe { MoveFileExW(w.as_ptr(), std::ptr::null(), MOVEFILE_DELAY_UNTIL_REBOOT) };
}

const TASK: &str = "HeMiMizer - continue";

/// The ids of the steps left for after the restart (written by the run before it).
pub fn pending_resume() -> Vec<String> {
    std::fs::read_to_string(resume_file()).map(|t| t.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect()).unwrap_or_default()
}

/// Err when the task is there and could not be deleted (it would start HeMiMizer as administrator at a sign-in).
pub fn clear_resume() -> Result<(), String> {
    let _ = std::fs::remove_file(resume_file());
    if !sys::exec("schtasks.exe", &["/query", "/tn", TASK]).is_ok_and(|o| o.code == 0) {
        return Ok(());
    }
    sys::run("schtasks.exe", &["/delete", "/tn", TASK, "/f"]).map(|_| ())
}

/// Saves the steps that are left and schedules HeMiMizer to continue after the next sign-in. HeMiMizer and its
/// config folder are copied to %ProgramData%\HeMiMizer\app first – a USB stick or a network drive may be gone then.
/// The task runs that copy as administrator: so it lies in the protected folder, which only administrators may
/// change (in %LOCALAPPDATA% any program of the user could have replaced it – audit 2026-11-24).
fn schedule_resume(ids: &[&str]) -> Result<(), String> {
    sys::protect_state_dir()?;
    let app = data_dir().join("app");
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let target = app.join("HeMiMizer.exe");
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
    if !origin_file().exists() && let Some(d) = exe.parent() {
        let _ = std::fs::write(origin_file(), d.display().to_string());
    }
    // an interactive task of this user needs no password; "highest" = as administrator without UAC
    sys::ps(&format!(
        "$u=[Security.Principal.WindowsIdentity]::GetCurrent().Name; \
         $a=New-ScheduledTaskAction -Execute '{exe}' -Argument '--resume'; \
         $t=New-ScheduledTaskTrigger -AtLogOn -User $u; \
         $p=New-ScheduledTaskPrincipal -UserId $u -LogonType Interactive -RunLevel Highest; \
         $s=New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -ExecutionTimeLimit (New-TimeSpan -Hours 0); \
         Register-ScheduledTask -TaskName '{TASK}' -Action $a -Trigger $t -Principal $p -Settings $s -Force | Out-Null",
        // a ' in the path (C:\Users\O'Neil) doubled for PowerShell
        exe = target.display().to_string().replace('\'', "''")
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
        if let Err(e) = clear_resume() {
            ctx.note(format!("The scheduled task \"{TASK}\" could not be deleted – delete it in Task Scheduler: {e}"));
        }
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
            // catches a panic only in builds with unwinding (Debug, tests); the release has panic = "abort", so its
            // steps must not panic at all (no unwrap on data from files or from the user)
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (s.run)(&ctx)))
                .unwrap_or_else(|_| Err(tr("The step failed unexpectedly.", "Krok neočekávaně selhal.").into()));
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
            if !windows::self_exclusion(false) {
                ctx.note("The Microsoft Defender exclusions of HeMiMizer could not be removed – remove them in Windows Security (Virus & threat protection settings → Exclusions)");
            }
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
    type Link = fn() -> Result<String, String>;
    let checks: [(&str, Link); 7] = [
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

/// Will the step do something with this plan and backup? For the overview before the run (a step that is not
/// needed only says so when it runs).
pub fn applies(id: &str) -> bool {
    use crate::config;
    if !crate::options::runs(id) {
        return false;
    }
    let plan = crate::plan::Plan::load().unwrap_or_default();
    let has_file = |prefix: &str| std::fs::read_dir(config::dir()).is_ok_and(|r| r.flatten().any(|e| e.file_name().to_string_lossy().to_lowercase().starts_with(prefix)));
    match id {
        "7zip" | "vlc" | "brave" | "steam" | "qbittorrent" | "hemidock" => plan.has(id),
        "chipset" => plan.has("amd-chipset"),
        "gpu" | "gpu-settings" => plan.has("amd-gpu"),
        "programs" => plan.apps.iter().any(|a| !crate::install::OWN_STEPS.contains(&a.as_str()) && crate::catalog::find(a).is_some_and(|i| i.group != crate::catalog::Group::Driver || a == "nvidia" || a == "intel")),
        "pc-name" => !config::text("pc-name.txt", false).trim().is_empty(),
        "wifi" => config::dir().join("wifi").is_dir(),
        "fonts" => config::dir().join("fonts").join("fonts.txt").is_file(),
        "mouse-settings" => config::has("mouse.txt"),
        "pins" => config::has("pins.tar"),
        "drives" => !config::drives().is_empty(),
        "folders" => !plan.folders.is_empty(),
        "app-settings" => config::home().join("apps").is_dir(),
        "shortcuts" => !crate::shortcuts::programs(&plan).is_empty(),
        "tweaks" => crate::tweaks::ALL.iter().any(|t| plan.windows_on(t.id) && crate::tweaks::offered(t.id, &plan)),
        "wallpaper" => has_file("wallpaper."),
        "explorer" => config::has("explorer-view.reg"),
        "appearance" => config::has("appearance.reg"),
        "display" => config::has("display-scaling.reg"),
        "offline" => !crate::backup::offline().is_empty(),
        _ => true,
    }
}
