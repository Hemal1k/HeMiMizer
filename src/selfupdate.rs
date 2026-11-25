//! HeMiMizer updates itself, like HeMiDock does without HeMi Launcher: at the start it asks GitHub for the newest
//! release; on the first page a newer one is offered, downloaded with its progress, checked against SHA256SUMS.txt of
//! the release, put in place of this HeMiMizer.exe (the running one renamed to .old, removed at the next start) and
//! started instead. Nothing is written anywhere else, nothing is asked while HeMiMizer works (not after a restart).

use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

const REPO: &str = "Hemal1k/HeMiMizer";
const ASSET: &str = "HeMiMizer.exe";

#[derive(Clone, Debug, PartialEq)]
pub enum State {
    /// nothing newer (or not known yet / no internet)
    None,
    Available { version: String, exe: String, sums: String },
    /// downloading: done / total bytes
    Downloading(u64, u64),
    Failed(String),
}

pub type Shared = Arc<Mutex<State>>;

pub fn new() -> Shared {
    Arc::new(Mutex::new(State::None))
}

/// HeMi Launcher on this PC: it keeps HeMiMizer up to date, so HeMiMizer does not ask itself.
pub fn launcher_present() -> bool {
    crate::sys::get_sz(crate::sys::Root::Hkcu, r"Software\HeMi\HeMiLauncher", "Path").is_some_and(|p| Path::new(&p).exists())
}

/// "1.4.1" as numbers.
fn version_of(text: &str) -> Vec<u32> {
    text.trim().trim_start_matches(['v', 'V']).split('.').map(|p| p.chars().take_while(char::is_ascii_digit).collect::<String>().parse().unwrap_or(0)).collect()
}

/// Asks GitHub in the background; `done` repaints the window.
pub fn check(state: Shared, done: impl Fn() + Send + 'static) {
    std::thread::spawn(move || {
        let Ok(text) = crate::sys::fetch(&format!("https://api.github.com/repos/{REPO}/releases/latest"), None) else { return };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else { return };
        let tag = v["tag_name"].as_str().unwrap_or("").to_string();
        if version_of(&tag) <= version_of(env!("CARGO_PKG_VERSION")) {
            return;
        }
        let asset = |name: &str| {
            v["assets"].as_array().into_iter().flatten().find(|a| a["name"].as_str().is_some_and(|n| n.eq_ignore_ascii_case(name))).and_then(|a| a["browser_download_url"].as_str()).map(String::from)
        };
        let (Some(exe), Some(sums)) = (asset(ASSET), asset("SHA256SUMS.txt")) else { return };
        // only the files of this repository's releases on GitHub, over HTTPS
        let ours = |u: &str| u.starts_with(&format!("https://github.com/{REPO}/releases/download/"));
        if !ours(&exe) || !ours(&sums) {
            return;
        }
        *state.lock().unwrap() = State::Available { version: tag.trim_start_matches(['v', 'V']).to_string(), exe, sums };
        done();
    });
}

/// Downloads the new version, checks it, puts it in place of this exe and starts it; the caller closes the window.
pub fn update(state: Shared, done: impl Fn() + Send + Sync + 'static, quit: impl FnOnce() + Send + 'static) {
    let State::Available { exe: url, sums, .. } = state.lock().unwrap().clone() else { return };
    *state.lock().unwrap() = State::Downloading(0, 0);
    std::thread::spawn(move || {
        let r = (|| -> Result<PathBuf, String> {
            let me = std::env::current_exe().map_err(|e| e.to_string())?;
            let new = me.with_extension("exe.new");
            let st = state.clone();
            let progress = |d: u64, t: u64| {
                *st.lock().unwrap() = State::Downloading(d, t);
                done();
            };
            // a part left by an interrupted download (maybe of another version) is not continued
            let _ = std::fs::remove_file(new.with_extension("iso.part"));
            crate::msiso::download(&url, &new, &progress)?;
            let list = crate::sys::fetch(&sums, None)?;
            let want = list
                .lines()
                .find_map(|l| l.split_once(char::is_whitespace).filter(|(_, f)| f.trim().trim_start_matches('*').eq_ignore_ascii_case(ASSET)).map(|(h, _)| h.trim().to_lowercase()))
                .ok_or("SHA256SUMS.txt has no HeMiMizer.exe")?;
            if sha256(&new)? != want {
                let _ = std::fs::remove_file(&new);
                return Err("The download is damaged (its checksum does not match) – try again later.".into());
            }
            // the running exe can be renamed, not overwritten
            let old = me.with_extension("exe.old");
            let _ = std::fs::remove_file(&old);
            std::fs::rename(&me, &old).map_err(|e| e.to_string())?;
            if let Err(e) = std::fs::rename(&new, &me) {
                let _ = std::fs::rename(&old, &me);
                return Err(e.to_string());
            }
            Ok(me)
        })();
        match r {
            Ok(me) => match std::process::Command::new(&me).arg("--updated").current_dir(me.parent().unwrap_or(Path::new("."))).spawn() {
                Ok(_) => quit(),
                // the new version is in place but did not start: the window stays and says so
                Err(e) => {
                    *state.lock().unwrap() = State::Failed(format!("Updated, but the new version did not start ({e}) – start HeMiMizer again"));
                    done();
                }
            },
            Err(e) => {
                *state.lock().unwrap() = State::Failed(e);
                done();
            }
        }
    });
}

fn sha256(file: &Path) -> Result<String, String> {
    let bytes = std::fs::read(file).map_err(|e| e.to_string())?;
    Ok(Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect())
}

/// The exe left by the last update.
pub fn remove_old() {
    if let Ok(me) = std::env::current_exe() {
        let old = me.with_extension("exe.old");
        if old.exists() {
            // the old one may still be ending
            for _ in 0..20 {
                if std::fs::remove_file(&old).is_ok() {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(250));
            }
        }
    }
}
