//! Installs what the wizard chose (plan.json): from the installers of the backup when there are some (offline), else
//! through winget (also the Microsoft Store through its source msstore), the HeMi apps from their releases on GitHub.
//! What is installed is noted in installed.txt in the work folder, so a step that runs again (after a restart, after
//! the internet) does not install it twice.

use crate::catalog::{self, Item, Source};
use crate::i18n::tr;
use crate::steps::{Ctx, NEEDS_INTERNET, Res};
use crate::sys;
use std::path::PathBuf;

/// The plan of the backup (or of the wizard on this PC); none = nothing chosen.
pub fn plan() -> crate::plan::Plan {
    crate::plan::Plan::load().unwrap_or_default()
}

/// The wizard chose it.
pub fn chosen(id: &str) -> bool {
    plan().has(id)
}

/// The result of a step whose program was not chosen.
pub fn not_chosen() -> String {
    "Not chosen in the wizard".into()
}

/// Installers for one user that refuse administrator rights or ask for them again (HeMimizer has them; Discord asked
/// for an administrator during its installation, 2026-11-18): run as the signed-in user.
const AS_USER: &[&str] = &["spotify", "discord"];

/// Programs with their own steps (their settings from the backup come after the installation).
pub const OWN_STEPS: &[&str] = &["7zip", "vlc", "brave", "steam", "qbittorrent", "hemidock", "amd-chipset", "amd-gpu"];

fn done_file() -> PathBuf {
    sys::work_dir().join("installed.txt")
}

fn is_done(id: &str) -> bool {
    std::fs::read_to_string(done_file()).is_ok_and(|t| t.lines().any(|l| l.trim() == id))
}

fn mark_done(id: &str) {
    use std::io::Write;
    let _ = std::fs::create_dir_all(sys::work_dir());
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(done_file()) {
        let _ = writeln!(f, "{id}");
    }
}

// ---------------------------------------------------------------- winget

/// winget is there (a fresh Windows registers App Installer only some time after the first sign-in – it is
/// registered here then; Tiny11 and similar may lack it entirely).
pub fn winget_ready(c: &Ctx) -> Result<(), String> {
    if sys::exec("winget.exe", &["--version"]).is_ok_and(|o| o.code == 0) {
        return Ok(());
    }
    c.log("Preparing winget");
    let _ = sys::ps("Add-AppxPackage -RegisterByFamilyName -MainPackage Microsoft.DesktopAppInstaller_8wekyb3d8bbwe");
    if sys::exec("winget.exe", &["--version"]).is_ok_and(|o| o.code == 0) {
        return Ok(());
    }
    if crate::drivers::online() {
        c.log("Downloading winget (App Installer)");
        let file = sys::work_dir().join("AppInstaller.msixbundle");
        sys::download("https://aka.ms/getwinget", &file, None)?;
        let _ = sys::ps(&format!("Add-AppxPackage -Path '{}'", file.display()));
        if sys::exec("winget.exe", &["--version"]).is_ok_and(|o| o.code == 0) {
            return Ok(());
        }
    }
    Err("winget is missing – install \"App Installer\" from the Microsoft Store and run HeMimizer again".into())
}

/// winget's exit codes that mean "it is there": already installed, no newer version.
fn already(code: i32) -> bool {
    matches!(code as u32, 0x8A15002B | 0x8A150061 | 0x8A15010D | 0x8A150109)
}

fn winget_args<'a>(cmd: &'a str, id: &'a str, store: bool) -> Vec<&'a str> {
    let mut a = vec![cmd, "--id", id, "--exact", "--accept-source-agreements", "--accept-package-agreements", "--disable-interactivity"];
    a.extend(["--source", if store { "msstore" } else { "winget" }]);
    a
}

fn winget_install(c: &Ctx, item: &Item, id: &str, store: bool) -> Result<(), String> {
    c.log(format!("{} {}", tr("Installing", "Instaluji"), item.name));
    let mut args = winget_args("install", id, store);
    args.push("--silent");
    let out = if AS_USER.contains(&item.id) { as_user("winget.exe", &args)? } else { sys::exec("winget.exe", &args)? };
    if out.code == 0 || already(out.code) {
        Ok(())
    } else {
        Err(format!("winget {} {:#X}: {}", tr("ended with code", "skončil s kódem"), out.code as u32, last_line(&out.text)))
    }
}

fn last_line(text: &str) -> String {
    text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.chars().all(|c| "-\\|/█▒ ".contains(c))).last().unwrap_or("").to_string()
}

/// Runs a program as the signed-in user without administrator rights (a scheduled task with the limited token)
/// and waits for it; its output is not known, only that it ended.
fn as_user(exe: &str, args: &[&str]) -> Result<sys::Output, String> {
    const TASK: &str = "HeMimizer-as-user";
    let line = std::iter::once(format!("\"{exe}\"")).chain(args.iter().map(|a| if a.contains(' ') { format!("\"{a}\"") } else { a.to_string() })).collect::<Vec<_>>().join(" ");
    // the task of this user (the one signed in) with the limited token: the installer runs without administrator rights
    sys::run("schtasks.exe", &["/create", "/tn", TASK, "/tr", &line, "/sc", "once", "/st", "00:00", "/rl", "LIMITED", "/f"])?;
    sys::run("schtasks.exe", &["/run", "/tn", TASK])?;
    // running until the task is no longer "Running" (at most 20 minutes)
    std::thread::sleep(std::time::Duration::from_secs(3));
    for _ in 0..400 {
        let q = sys::exec("schtasks.exe", &["/query", "/tn", TASK, "/fo", "list"]).map(|o| o.text).unwrap_or_default();
        if !q.contains("Running") && !q.contains("Spuštěno") {
            break;
        }
        std::thread::sleep(std::time::Duration::from_secs(3));
    }
    let _ = sys::exec("schtasks.exe", &["/delete", "/tn", TASK, "/f"]);
    Ok(sys::Output { code: 0, text: String::new() })
}

// ---------------------------------------------------------------- the installers of the backup

/// The folder of the installers of one program in the backup (winget download writes the installer and its manifest).
pub fn offline_dir(id: &str) -> PathBuf {
    crate::backup::installers_dir().join("programs").join(id)
}

/// Downloads the installer of `item` into the backup (for an offline setup); the Store apps cannot be downloaded.
pub fn download(item: &Item, log: &dyn Fn(String)) -> Result<(), String> {
    let dir = offline_dir(item.id);
    match item.source {
        Source::Winget(id) => {
            log(format!("{} {}", tr("Downloading", "Stahuji"), item.name));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            let d = dir.display().to_string();
            let mut args = winget_args("download", id, false);
            args.extend(["--download-directory", &d]);
            let out = sys::exec("winget.exe", &args)?;
            if out.code == 0 { Ok(()) } else { Err(format!("winget {:#X}: {}", out.code as u32, last_line(&out.text))) }
        }
        Source::GitHub(repo, asset) => {
            log(format!("{} {}", tr("Downloading", "Stahuji"), item.name));
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            sys::download(&github_url(repo, asset), &dir.join(asset), None)
        }
        Source::Release(repo, end) => {
            log(format!("{} {}", tr("Downloading", "Stahuji"), item.name));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            let (url, name) = release_asset(repo, end)?;
            sys::download(&url, &dir.join(name), None)
        }
        Source::Store(_) => Err("a Microsoft Store app – it is installed on the new PC from the internet".into()),
        Source::Own => Ok(()),
    }
}

/// The asset of the newest release of `repo` whose name ends with `end`: (download link, file name).
fn release_asset(repo: &str, end: &str) -> Result<(String, String), String> {
    let text = sys::fetch(&format!("https://api.github.com/repos/{repo}/releases/latest"), None)?;
    let v: serde_json::Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    v["assets"]
        .as_array()
        .into_iter()
        .flatten()
        .find_map(|a| {
            let name = a["name"].as_str()?;
            name.ends_with(end).then(|| (a["browser_download_url"].as_str().unwrap_or("").to_string(), name.to_string()))
        })
        .filter(|(u, _)| !u.is_empty())
        .ok_or_else(|| format!("{repo}: no *{end} in the newest release"))
}

fn github_url(repo: &str, asset: &str) -> String {
    format!("https://github.com/Hemal1k/{repo}/releases/latest/download/{asset}")
}

/// Installs from the installer the backup downloaded; Ok(false) = there is none (or it cannot be run offline).
fn install_offline(c: &Ctx, item: &Item) -> Result<bool, String> {
    let dir = offline_dir(item.id);
    let Ok(rd) = std::fs::read_dir(&dir) else { return Ok(false) };
    let files: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
    let yaml = files.iter().find(|f| f.extension().is_some_and(|x| x.eq_ignore_ascii_case("yaml")));
    let installer = files.iter().find(|f| f.extension().is_some_and(|x| ["exe", "msi", "msix", "msixbundle", "appx"].iter().any(|e| x.eq_ignore_ascii_case(e))));
    let Some(installer) = installer else { return Ok(false) };
    c.log(format!("{} {} ({})", tr("Installing", "Instaluji"), item.name, tr("from the backup", "ze zálohy")));
    let manifest = yaml.and_then(|y| std::fs::read_to_string(y).ok()).unwrap_or_default();
    let kind = field(&manifest, "InstallerType").unwrap_or_else(|| if matches!(item.source, Source::GitHub(..)) { "inno".into() } else { String::new() });
    let file = installer.display().to_string();
    let ext = installer.extension().map(|x| x.to_string_lossy().to_lowercase()).unwrap_or_default();
    let switches = |default: &str| {
        let mut s = field(&manifest, "Silent").unwrap_or_else(|| default.to_string());
        if let Some(custom) = field(&manifest, "Custom") {
            s.push(' ');
            s.push_str(&custom);
        }
        s
    };
    let split = |s: String| s.split_whitespace().map(String::from).collect::<Vec<_>>();
    let r = match (kind.as_str(), ext.as_str()) {
        (_, "msix" | "msixbundle" | "appx") | ("msix" | "appx", _) => sys::ps(&format!("Add-AppxPackage -Path '{file}'")).map(|_| ()),
        ("msi" | "wix", _) | (_, "msi") => {
            let mut a = vec!["/i".to_string(), file.clone()];
            a.extend(split(switches("/quiet /norestart")));
            let a: Vec<&str> = a.iter().map(String::as_str).collect();
            sys::run("msiexec.exe", &a).map(|_| ())
        }
        (k, _) => {
            let default = match k {
                "inno" => "/VERYSILENT /SUPPRESSMSGBOXES /NORESTART /SP-",
                "nullsoft" => "/S",
                "burn" => "/quiet /norestart",
                _ => "",
            };
            let s = switches(default);
            if s.trim().is_empty() {
                return Ok(false);
            }
            let mut a = split(s);
            // the HeMi apps for all users (HeMimizer runs as administrator)
            if matches!(item.source, Source::GitHub(..)) {
                a.push("/ALLUSERS".into());
            }
            let a: Vec<&str> = a.iter().map(String::as_str).collect();
            if AS_USER.contains(&item.id) { as_user(&file, &a).map(|_| ()) } else { sys::run(&file, &a).map(|_| ()) }
        }
    };
    r.map(|()| true)
}

/// The first value of `name:` in a manifest of winget (quotes removed).
fn field(yaml: &str, name: &str) -> Option<String> {
    yaml.lines().find_map(|l| {
        let t = l.trim_start().trim_start_matches("- ");
        t.strip_prefix(&format!("{name}:")).map(|v| v.trim().trim_matches(['\'', '"']).to_string()).filter(|v| !v.is_empty())
    })
}

// ---------------------------------------------------------------- one program

/// Installs `id` (once): Ok(true) = installed now, Ok(false) = it was done already. Offline without its installer
/// in the backup: NEEDS_INTERNET (the step runs again after the internet is connected).
pub fn ensure(c: &Ctx, id: &str) -> Result<bool, String> {
    if is_done(id) {
        return Ok(false);
    }
    let item = catalog::find(id).ok_or_else(|| format!("unknown program {id}"))?;
    if install_offline(c, item)? {
        mark_done(id);
        return Ok(true);
    }
    if !crate::drivers::online() {
        return Err(NEEDS_INTERNET.into());
    }
    match item.source {
        Source::Winget(wid) => {
            winget_ready(c)?;
            winget_install(c, item, wid, false)?;
        }
        Source::Store(sid) => {
            winget_ready(c)?;
            winget_install(c, item, sid, true)?;
        }
        Source::GitHub(repo, asset) => {
            c.log(format!("{} {}", tr("Downloading", "Stahuji"), item.name));
            let file = sys::work_dir().join(asset);
            sys::download(&github_url(repo, asset), &file, None)?;
            c.log(format!("{} {}", tr("Installing", "Instaluji"), item.name));
            sys::run(&file.display().to_string(), &["/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/SP-", "/ALLUSERS"])?;
        }
        Source::Release(repo, end) => {
            c.log(format!("{} {}", tr("Downloading", "Stahuji"), item.name));
            let (url, name) = release_asset(repo, end)?;
            let file = sys::work_dir().join(&name);
            sys::download(&url, &file, None)?;
            c.log(format!("{} {}", tr("Installing", "Instaluji"), item.name));
            sys::run("msiexec.exe", &["/i", &file.display().to_string(), "/quiet", "/norestart"])?;
            sys::remove_file(&file);
        }
        Source::Own => return Ok(false),
    }
    mark_done(id);
    Ok(true)
}

/// The step "Programs from the wizard": everything chosen that has no step of its own.
pub fn programs(c: &Ctx) -> Res {
    let plan = plan();
    let todo: Vec<&Item> = plan.apps.iter().filter_map(|id| catalog::find(id)).filter(|i| !OWN_STEPS.contains(&i.id)).collect();
    if todo.is_empty() {
        return Ok("Nothing chosen".into());
    }
    let (mut new, mut had, mut later, mut failed) = (0, 0, 0, Vec::new());
    for item in &todo {
        match ensure(c, item.id) {
            Ok(true) => new += 1,
            Ok(false) => had += 1,
            Err(e) if e == NEEDS_INTERNET => later += 1,
            Err(e) => failed.push(format!("{}: {e}", item.name)),
        }
    }
    // some wait for the internet: the whole step again later (what is done is not installed twice)
    if later > 0 {
        return Err(NEEDS_INTERNET.into());
    }
    let text = format!("{} {new}{}", "Installed:", if had > 0 { format!(", already there: {had}") } else { String::new() });
    if failed.is_empty() { Ok(text) } else { Err(format!("{text}\n{}", failed.join("\n"))) }
}

/// The backup (offline): the installers of everything chosen into the backup.
pub fn download_chosen(log: &dyn Fn(String)) -> (usize, Vec<String>) {
    let plan = plan();
    let (mut n, mut failed) = (0, Vec::new());
    for item in plan.apps.iter().filter_map(|id| catalog::find(id)) {
        // the AMD drivers and HeMiDock: the backup keeps their installers its own way
        if matches!(item.source, Source::Own | Source::Store(_)) || item.id == "hemidock" {
            continue;
        }
        match download(item, log) {
            Ok(()) => n += 1,
            Err(e) => failed.push(format!("{}: {e}", item.name)),
        }
    }
    (n, failed)
}
