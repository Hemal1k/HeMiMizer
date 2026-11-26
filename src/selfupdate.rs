//! HeMiMizer updates itself, like HeMiDock does without HeMi Launcher: at the start it asks GitHub for the newest
//! release; on the first page a newer one is offered, downloaded with its progress, checked against SHA256SUMS.txt of
//! the release, put in place of this HeMiMizer.exe (the running one renamed to .old, removed at the next start) and
//! started instead. Nothing is written anywhere else, nothing is asked while HeMiMizer works (not after a restart).
//! A HeMiMizer installed by its installer (since 1.7.0) gets HeMiMizer-Setup.exe instead: checked the same way and run
//! quietly for the same installation (the user or all users); it puts the new version in place and starts it again.

use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

const REPO: &str = "Hemal1k/HeMiMizer";
/// The portable program (the name stays: the HeMiMizers up to 1.6 update themselves by it).
const ASSET: &str = "HeMiMizer.exe";
/// The installer (since 1.7.0).
const SETUP: &str = "HeMiMizer-Setup.exe";
const UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\HeMiMizer_is1";

#[derive(Clone, Debug, PartialEq)]
pub enum State {
    /// nothing newer (or not known yet / no internet)
    None,
    /// `exe`: the download (the portable exe, or the installer when `installer` is Some – for all users or not)
    Available { version: String, exe: String, sums: String, installer: Option<bool> },
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

/// Installed by its installer: the uninstall entry HeMiMizer_is1 (of the user, else of all users) names the folder of
/// this exe. Some(true) = installed for all users. A portable HeMiMizer.exe elsewhere (a USB stick) is not.
pub fn installed() -> Option<bool> {
    installed_in(std::env::current_exe().ok()?.parent()?)
}

/// The installer put HeMiMizer into `folder` (Some(true): for all users).
pub fn installed_in(folder: &Path) -> Option<bool> {
    [(crate::sys::Root::Hkcu, false), (crate::sys::Root::Hklm, true)]
        .into_iter()
        .find_map(|(root, all)| crate::sys::get_sz(root, UNINSTALL_KEY, "InstallLocation").filter(|l| same_folder(l, folder)).map(|_| all))
}

/// The same folder, whatever the case and a backslash at the end.
fn same_folder(a: &str, b: &Path) -> bool {
    let b = b.display().to_string();
    !a.trim().is_empty() && a.trim().trim_end_matches('\\').eq_ignore_ascii_case(b.trim_end_matches('\\'))
}

/// The installer is downloaded into the protected folder (only administrators may change it before it runs).
fn setup_dir() -> PathBuf {
    crate::sys::state_dir().join("update")
}

/// "1.4.1" as numbers.
fn version_of(text: &str) -> Vec<u32> {
    text.trim().trim_start_matches(['v', 'V']).split('.').map(|p| p.chars().take_while(char::is_ascii_digit).collect::<String>().parse().unwrap_or(0)).collect()
}

/// Asks GitHub in the background (at the start, then again as often as Settings → Updates says); `done` repaints the window.
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
        // installed: its installer (a release without one: the exe in place, as a portable one)
        let installer = installed().filter(|_| asset(SETUP).is_some());
        let (Some(exe), Some(sums)) = (asset(if installer.is_some() { SETUP } else { ASSET }), asset("SHA256SUMS.txt")) else { return };
        // only the files of this repository's releases on GitHub, over HTTPS
        let ours = |u: &str| u.starts_with(&format!("https://github.com/{REPO}/releases/download/"));
        if !ours(&exe) || !ours(&sums) {
            return;
        }
        // a download already running (or a failed one shown) is not replaced by a later check
        {
            let mut s = state.lock().unwrap();
            if !matches!(*s, State::None | State::Available { .. }) {
                return;
            }
            *s = State::Available { version: tag.trim_start_matches(['v', 'V']).to_string(), exe, sums, installer };
        }
        done();
    });
}

/// The SHA-256 of `name` in the text of SHA256SUMS.txt ("<hash>  <name>", a "*" before the name allowed).
fn sum_of(list: &str, name: &str) -> Option<String> {
    list.lines().find_map(|l| l.split_once(char::is_whitespace).filter(|(_, f)| f.trim().trim_start_matches('*').eq_ignore_ascii_case(name)).map(|(h, _)| h.trim().to_lowercase()))
}

/// Downloads `url` to `to` with its progress and checks it against SHA256SUMS.txt of the release (a damaged one is
/// deleted).
fn fetch_checked(url: &str, sums: &str, name: &str, to: &Path, progress: &dyn Fn(u64, u64)) -> Result<(), String> {
    // a part left by an interrupted download (maybe of another version) is not continued
    let _ = std::fs::remove_file(to.with_extension("iso.part"));
    crate::msiso::download(url, to, progress)?;
    let list = crate::sys::fetch(sums, None)?;
    let want = sum_of(&list, name).ok_or_else(|| format!("SHA256SUMS.txt has no {name}"))?;
    if sha256(to)? != want {
        let _ = std::fs::remove_file(to);
        return Err("The download is damaged (its checksum does not match) – try again later.".into());
    }
    Ok(())
}

/// Downloads the new version, checks it, puts it in place of this exe and starts it; the caller closes the window.
/// Installed: the installer runs quietly for the same installation and starts the new HeMiMizer itself.
pub fn update(state: Shared, done: impl Fn() + Send + Sync + 'static, quit: impl FnOnce() + Send + 'static) {
    let State::Available { exe: url, sums, installer, .. } = state.lock().unwrap().clone() else { return };
    *state.lock().unwrap() = State::Downloading(0, 0);
    std::thread::spawn(move || {
        let st = state.clone();
        let progress = |d: u64, t: u64| {
            *st.lock().unwrap() = State::Downloading(d, t);
            done();
        };
        if let Some(all_users) = installer {
            let r = (|| -> Result<(), String> {
                crate::sys::protect_state_dir()?;
                let dir = setup_dir();
                std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
                let setup = dir.join(SETUP);
                fetch_checked(&url, &sums, SETUP, &setup, &progress)?;
                // /OPEN=1: the installer starts HeMiMizer again at its end (HeMi Launcher's quiet updates do not)
                std::process::Command::new(&setup)
                    .args(["/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/UPDATE", "/OPEN=1", if all_users { "/ALLUSERS" } else { "/CURRENTUSER" }])
                    .current_dir(&dir)
                    .spawn()
                    .map_err(|e| format!("The installer did not start ({e})"))?;
                Ok(())
            })();
            match r {
                // the installer ends this HeMiMizer if it is still running when it gets to its files
                Ok(()) => quit(),
                Err(e) => {
                    *state.lock().unwrap() = State::Failed(e);
                    done();
                }
            }
            return;
        }
        let r = (|| -> Result<PathBuf, String> {
            let me = std::env::current_exe().map_err(|e| e.to_string())?;
            let new = me.with_extension("exe.new");
            fetch_checked(&url, &sums, ASSET, &new, &progress)?;
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

/// The exe left by the last update (and the installer of the last one, when it has ended).
pub fn remove_old() {
    let dir = setup_dir();
    if dir.exists() {
        let _ = std::fs::remove_dir_all(&dir);
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_checksums_and_the_folder() {
        let list = "ABC123  HeMiMizer-Setup.exe\ndef456 *HeMiMizer.exe\n";
        assert_eq!(sum_of(list, SETUP).as_deref(), Some("abc123"));
        assert_eq!(sum_of(list, ASSET).as_deref(), Some("def456"));
        assert_eq!(sum_of(list, "SHA256SUMS.txt"), None);
        let here = Path::new(r"C:\Program Files\HeMi\HeMiMizer");
        assert!(same_folder(r"C:\Program Files\HeMi\HeMiMizer\", here));
        assert!(same_folder(r"c:\program files\hemi\hemimizer", here));
        assert!(!same_folder(r"C:\Program Files\HeMi\HeMiMizer2", here));
        assert!(!same_folder("", here));
    }
}
