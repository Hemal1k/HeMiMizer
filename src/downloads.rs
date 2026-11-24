//! Everything to install is downloaded first, right after the internet is connected. What fails is shown in the
//! window: the user may try again or choose the installer by hand. Only then are the programs installed.
//! downloads.txt in the work folder (id|file|version) tells the install steps which file to use.

use crate::i18n::tr;
use crate::programs;
use crate::steps::{Ctx, Res};
use crate::sys;
use std::path::PathBuf;
use std::sync::Mutex;

struct Planned {
    id: String,
    name: String,
    /// the installed program to compare with (the download is skipped when it is the newest already)
    installed: &'static str,
    resolve: Box<dyn Fn() -> Result<(String, String), String> + Send>,
}

/// The answer of the window when downloads failed.
pub enum Reply {
    Retry,
    /// (id, file chosen by hand); ids not listed are skipped
    Continue(Vec<(String, PathBuf)>),
}

pub static REPLY: Mutex<Option<Reply>> = Mutex::new(None);

fn manifest() -> PathBuf {
    sys::work_dir().join("downloads.txt")
}

fn record(id: &str, file: &std::path::Path, version: &str) {
    use std::io::Write;
    let _ = std::fs::create_dir_all(sys::work_dir());
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(manifest()) {
        let _ = writeln!(f, "{id}|{}|{version}", file.display());
    }
}

/// The installer prepared for `id` and its version ("" when chosen by hand).
pub fn prepared(id: &str) -> Option<(PathBuf, String)> {
    let text = std::fs::read_to_string(manifest()).ok()?;
    text.lines().rev().find_map(|l| {
        let p: Vec<&str> = l.splitn(3, '|').collect();
        (p.len() == 3 && p[0] == id && std::path::Path::new(p[1]).exists()).then(|| (PathBuf::from(p[1]), p[2].to_string()))
    })
}

fn plan() -> Vec<Planned> {
    // the programs of the wizard are downloaded by winget as they are installed; here only the runtimes
    let mut v: Vec<Planned> = Vec::new();
    for (id, name, url) in programs::runtime_downloads(false) {
        v.push(Planned { id, name, installed: "", resolve: Box::new(move || Ok((url.clone(), String::new()))) });
    }
    v
}

fn file_name(url: &str) -> String {
    url.split(['?', '#']).next().unwrap_or(url).trim_end_matches("/download").rsplit('/').next().unwrap_or("setup.exe").to_string()
}

/// Downloads one; Ok(false) = not needed (the newest is installed).
fn fetch(c: &Ctx, p: &Planned) -> Result<bool, String> {
    if prepared(&p.id).is_some() {
        return Ok(true);
    }
    c.log(format!("{} {}", tr("Downloading", "Stahuji"), p.name));
    let (url, version) = (p.resolve)()?;
    if !p.installed.is_empty() && !version.is_empty() {
        if let Some(have) = programs::installed_version(p.installed) {
            if have == version || have.ends_with(&format!(".{version}")) {
                return Ok(false);
            }
        }
    }
    let name = file_name(&url);
    let file = sys::work_dir().join(format!("{}-{}", p.id, if name.to_lowercase().ends_with(".exe") { name } else { format!("{name}.exe") }));
    sys::download(&url, &file, None)?;
    record(&p.id, &file, &version);
    Ok(true)
}

pub fn download_all(c: &Ctx) -> Res {
    let plan = plan();
    let mut pending: Vec<usize> = (0..plan.len()).collect();
    let (mut done, mut skipped, mut by_hand) = (0, 0, 0);
    loop {
        let mut failed: Vec<(usize, String)> = Vec::new();
        for &i in &pending {
            match fetch(c, &plan[i]) {
                Ok(true) => done += 1,
                Ok(false) => skipped += 1,
                Err(e) => failed.push((i, e)),
            }
        }
        if failed.is_empty() {
            break;
        }
        // the window shows them; the user chooses files by hand, tries again, or goes on without them
        *REPLY.lock().unwrap() = None;
        c.need_files(failed.iter().map(|(i, e)| (plan[*i].id.clone(), plan[*i].name.clone(), e.clone())).collect());
        let reply = loop {
            if let Some(r) = REPLY.lock().unwrap().take() {
                break r;
            }
            std::thread::sleep(std::time::Duration::from_millis(300));
        };
        match reply {
            Reply::Retry => pending = failed.iter().map(|(i, _)| *i).collect(),
            Reply::Continue(files) => {
                for (id, path) in files {
                    let target = sys::work_dir().join(format!("{id}-{}", path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()));
                    if std::fs::copy(&path, &target).is_ok() {
                        record(&id, &target, "");
                        sys::log(&format!("{id}: chosen by hand {}", path.display()));
                        by_hand += 1;
                    }
                }
                let left = failed.len().saturating_sub(by_hand);
                if left > 0 {
                    c.note(format!("{} {left}", tr("Installers not downloaded (their steps try again or fail):", "Nestažené instalační programy (jejich kroky to zkusí znovu, nebo selžou):")));
                }
                break;
            }
        }
    }
    Ok(format!(
        "{} {done}{}{}",
        tr("Downloaded:", "Staženo:"),
        if skipped > 0 { format!(", {} {skipped}", tr("already the newest:", "už nejnovější:")) } else { String::new() },
        if by_hand > 0 { format!(", {} {by_hand}", tr("chosen by hand:", "vybráno ručně:")) } else { String::new() }
    ))
}

/// The ids prepared so far that start with `prefix` (".NET versions of the backup when the PC is offline").
pub fn prepared_ids(prefix: &str) -> Vec<String> {
    let text = std::fs::read_to_string(manifest()).unwrap_or_default();
    let mut ids: Vec<String> = text
        .lines()
        .filter_map(|l| l.split('|').next())
        .filter(|id| id.starts_with(prefix))
        .map(String::from)
        .collect();
    ids.dedup();
    ids
}

/// The step "Installers from the backup": the installers the backup downloaded (the folder `installers` next to
/// HeMiMizer.exe) are copied to this PC – the network drive or the USB stick may be gone after the restart – and the
/// install steps take them instead of downloading. The AMD drivers are taken from that folder directly.
pub fn import_offline(c: &Ctx) -> Res {
    let list = crate::backup::offline();
    if list.is_empty() {
        return Ok(tr("No installers in the backup – they are downloaded later", "V záloze nejsou instalační programy – stáhnou se později").into());
    }
    let mut n = 0;
    for (id, file, version) in list {
        if id.starts_with("amd-") {
            continue;
        }
        let target = sys::work_dir().join(file.file_name().unwrap_or_default());
        c.log(format!("{} {}", tr("Copying", "Kopíruji"), file.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default()));
        if std::fs::copy(&file, &target).is_ok() {
            record(&id, &target, &version);
            n += 1;
        }
    }
    Ok(format!("{} {n}", tr("Installers ready:", "Připravené instalační programy:")))
}
