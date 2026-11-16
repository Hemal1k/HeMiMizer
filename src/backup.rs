//! Backup mode – before Windows is reinstalled: the chosen settings of this PC go into the config folder next to
//! HeMimizer.exe, and every installer (the programs, the runtimes, the AMD drivers, HeMiDock) into the folder
//! `installers` next to it – so the setup after the reinstall needs no internet until its very end.
//! The choice can be kept as the user's own default (config\backup-choice.txt).

use crate::i18n::tr;
use crate::{config, drivers, programs, sys};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;

/// The id of the item "download the installers" (the rest are the parts of config.rs).
pub const INSTALLERS: &str = "installers";
/// The own folders chosen in the wizard.
pub const FOLDERS: &str = "folders";

pub struct Item {
    pub id: &'static str,
    pub name: &'static str,
}

/// Everything the backup offers, in its order.
pub fn items() -> Vec<Item> {
    let mut v: Vec<Item> = config::parts().into_iter().map(|(id, name, _)| Item { id, name }).collect();
    v.push(Item { id: INSTALLERS, name: tr("All installers, for the setup offline", "Všechny instalačky, pro instalaci offline") });
    v.push(Item { id: FOLDERS, name: "Your folders" });
    // the settings of the programs of the wizard (shown only for the programs chosen)
    for d in crate::appdata::ALL {
        if let Some(i) = crate::catalog::find(d.id) {
            v.push(Item { id: d.id, name: i.name });
        }
    }
    v
}

fn choice_file() -> PathBuf {
    config::dir().join("backup-choice.txt")
}

/// The user's own default (all when none was saved).
pub fn saved_choice() -> Vec<String> {
    match std::fs::read_to_string(choice_file()) {
        // also a file written by hand (Notepad may add a BOM)
        Ok(t) => t.trim_start_matches('\u{feff}').lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect(),
        Err(_) => items().iter().map(|i| i.id.to_string()).collect(),
    }
}

pub fn save_choice(ids: &[String]) -> Result<(), String> {
    std::fs::create_dir_all(config::dir()).map_err(|e| e.to_string())?;
    std::fs::write(choice_file(), ids.join("\r\n")).map_err(|e| e.to_string())
}

/// The folder of the installers next to HeMimizer.exe.
pub fn installers_dir() -> PathBuf {
    config::dir().parent().map(Path::to_path_buf).unwrap_or_default().join("installers")
}

/// installers.txt: id|file name|version, one installer per line.
fn index() -> PathBuf {
    installers_dir().join("installers.txt")
}

/// The installers of the backup: (id, file, version) – only the files that are there.
pub fn offline() -> Vec<(String, PathBuf, String)> {
    let Ok(t) = std::fs::read_to_string(index()) else { return Vec::new() };
    t.lines()
        .filter_map(|l| {
            let p: Vec<&str> = l.splitn(3, '|').collect();
            let file = installers_dir().join(p.get(1)?);
            (p.len() == 3 && file.is_file()).then(|| (p[0].to_string(), file, p[2].to_string()))
        })
        .collect()
}

pub enum Event {
    Start(usize),
    Log(String),
    Done(usize, Result<String, String>),
    Finished,
}

/// Runs the chosen items (indices of `items()`) on their own thread.
pub fn run(chosen: Vec<usize>, tx: Sender<Event>, repaint: impl Fn() + Send + 'static) {
    std::thread::spawn(move || {
        let all = items();
        let parts = config::parts();
        sys::log(&format!("==== Backup: {} ====", chosen.iter().map(|&i| all[i].id).collect::<Vec<_>>().join(", ")));
        for i in chosen {
            let _ = tx.send(Event::Start(i));
            repaint();
            let id = all[i].id;
            let log = |t: String| {
                sys::log(&t);
                let _ = tx.send(Event::Log(t));
                repaint();
            };
            let r = if id == INSTALLERS {
                download_installers(&log)
            } else if id == FOLDERS {
                copy_folders(&log)
            } else if crate::appdata::find(id).is_some() {
                crate::appdata::capture(id)
            } else {
                parts.iter().find(|p| p.0 == id).map(|p| (p.2)()).unwrap_or_else(|| Err("?".into()))
            };
            sys::log(&format!("backup {id}: {r:?}"));
            let _ = tx.send(Event::Done(i, r));
            repaint();
        }
        let _ = tx.send(Event::Finished);
        repaint();
    });
}

/// The folder of the copies of the own folders in the backup: folders\1, folders\2… in the order of plan.json.
pub fn folders_dir() -> PathBuf {
    config::home().join("folders")
}

/// Copies a folder with robocopy (all files, subfolders, times; codes below 8 are success).
pub fn robocopy(from: &Path, to: &Path) -> Result<(), String> {
    let o = sys::exec("robocopy.exe", &[&from.display().to_string(), &to.display().to_string(), "/E", "/COPY:DAT", "/DCOPY:DAT", "/R:1", "/W:1", "/NFL", "/NDL", "/NJH", "/NP"])?;
    if o.code < 8 { Ok(()) } else { Err(format!("robocopy {}: {}", o.code, o.text.trim().lines().last().unwrap_or(""))) }
}

/// The own folders of the wizard into the backup.
fn copy_folders(log: &dyn Fn(String)) -> Result<String, String> {
    let plan = crate::plan::Plan::load().unwrap_or_default();
    let dir = folders_dir();
    let _ = std::fs::remove_dir_all(&dir);
    let mut failed = Vec::new();
    for (n, f) in plan.folders.iter().enumerate() {
        let src = crate::plan::resolve_folder(f);
        log(format!("Copying {}", src.display()));
        if let Err(e) = robocopy(&src, &dir.join((n + 1).to_string())) {
            failed.push(format!("{}: {e}", src.display()));
        }
    }
    let text = format!("{} folders", plan.folders.len() - failed.len());
    if failed.is_empty() { Ok(text) } else { Err(format!("{text}\n{}", failed.join("\n"))) }
}

/// One installer to keep: id, name, how to find its link and version, a Referer when the server wants one.
struct Wanted {
    id: String,
    name: String,
    resolve: Box<dyn Fn() -> Result<(String, String), String>>,
    referer: Option<&'static str>,
}

fn file_name(url: &str) -> String {
    url.split(['?', '#']).next().unwrap_or(url).trim_end_matches("/download").rsplit('/').next().unwrap_or("setup.exe").to_string()
}

/// Downloads the newest installer of everything HeMimizer installs into `installers`; a file of the same name that is
/// there already is kept (the backup before the next reinstall only adds what is new) and older ones are removed.
pub fn download_installers(log: &dyn Fn(String)) -> Result<String, String> {
    let dir = installers_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    // the programs chosen in the wizard: through winget download (with their manifests) / from GitHub
    let (programs_new, mut failed) = crate::install::download_chosen(log);
    let mut want: Vec<Wanted> = Vec::new();
    for (id, name, url) in programs::runtime_downloads(true) {
        want.push(Wanted { id, name, resolve: Box::new(move || Ok((url.clone(), String::new()))), referer: None });
    }
    if crate::install::chosen("hemidock") {
        want.push(Wanted { id: "hemidock".into(), name: "HeMiDock".into(), resolve: Box::new(|| Ok((programs::HEMIDOCK_SETUP.to_string(), String::new()))), referer: None });
    }
    // the AMD drivers keep their own names: HeMimizer tells them apart by them (and by the product name inside)
    if crate::install::chosen("amd-chipset") {
        want.push(Wanted { id: "amd-chipset".into(), name: tr("AMD chipset driver", "Ovladač čipsetu AMD").into(), resolve: Box::new(|| drivers::url_chipset().map(|u| (u, String::new()))), referer: Some("https://www.amd.com/") });
    }
    if crate::install::chosen("amd-gpu") {
        want.push(Wanted { id: "amd-graphics".into(), name: tr("AMD graphics driver (~1 GB)", "Ovladač grafiky AMD (~1 GB)").into(), resolve: Box::new(|| drivers::url_gpu().map(|u| (u, String::new()))), referer: Some("https://www.amd.com/") });
    }

    let old: Vec<(String, PathBuf, String)> = offline();
    let mut lines: Vec<String> = Vec::new();
    let (mut new, mut kept) = (programs_new, 0);
    for w in &want {
        log(format!("{} {}", tr("Checking", "Kontroluji"), w.name));
        let (url, version) = match (w.resolve)() {
            Ok(x) => x,
            Err(e) => {
                // offline or the page changed: the one from the last backup stays
                if let Some((_, f, v)) = old.iter().find(|(id, _, _)| *id == w.id) {
                    lines.push(format!("{}|{}|{v}", w.id, f.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()));
                    kept += 1;
                }
                failed.push(format!("{}: {e}", w.name));
                continue;
            }
        };
        let mut name = file_name(&url);
        if !name.to_lowercase().ends_with(".exe") && !name.to_lowercase().ends_with(".msi") {
            name.push_str(".exe");
        }
        let file_name = if w.id.starts_with("amd-") { name } else { format!("{}-{name}", w.id) };
        let file = dir.join(&file_name);
        // the same file name = the same version (the links carry it); HeMiDock and Visual C++ 2015–2026 keep one name
        // for every version, so they are downloaded again (they are small)
        let always = w.id == "hemidock" || url.contains("aka.ms");
        let same = file.is_file() && !always;
        if same {
            kept += 1;
        } else {
            log(format!("{} {}", tr("Downloading", "Stahuji"), w.name));
            if let Err(e) = sys::download(&url, &file, w.referer) {
                failed.push(format!("{}: {e}", w.name));
                if let Some((_, f, v)) = old.iter().find(|(id, _, _)| *id == w.id) {
                    lines.push(format!("{}|{}|{v}", w.id, f.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()));
                }
                continue;
            }
            new += 1;
        }
        // an older file of this id goes
        for (id, f, _) in &old {
            if *id == w.id && *f != file {
                let _ = std::fs::remove_file(f);
            }
        }
        lines.push(format!("{}|{file_name}|{version}", w.id));
    }
    std::fs::write(index(), lines.join("\r\n")).map_err(|e| e.to_string())?;
    let size: u64 = std::fs::read_dir(&dir).map(|r| r.flatten().filter_map(|e| e.metadata().ok()).map(|m| m.len()).sum()).unwrap_or(0);
    let text = format!(
        "{} {new}, {} {kept} · {:.1} GB · {}",
        tr("downloaded", "staženo"),
        tr("kept", "ponecháno"),
        size as f64 / 1e9,
        dir.display()
    );
    if failed.is_empty() { Ok(text) } else { Err(format!("{text}\n{}", failed.join("\n"))) }
}
