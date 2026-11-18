//! Small helpers around Windows: the registry, hidden processes, downloads (curl.exe of Windows), known folders.

use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::ptr::null_mut;
use windows_sys::Win32::Foundation::ERROR_SUCCESS;
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, HKEY_USERS, REG_DWORD, REG_SZ, RRF_RT_REG_SZ,
    RegDeleteKeyValueW, RegDeleteTreeW, RegGetValueW, RegSetKeyValueW,
};

/// Removes values of a key (missing ones are fine).
pub fn delete_values(root: Root, key: &str, names: &[&str]) {
    let k = wide(key);
    for n in names {
        let n = wide(n);
        unsafe { RegDeleteKeyValueW(hkey(root), k.as_ptr(), n.as_ptr()) };
    }
}

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
/// Short on purpose: SourceForge answers a full browser string with a bot check (403).
pub const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64)";

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

// ---------------------------------------------------------------- registry

#[derive(Clone, Copy)]
pub enum Root {
    Hklm,
    Hkcu,
    Hku,
}

fn hkey(r: Root) -> HKEY {
    match r {
        Root::Hklm => HKEY_LOCAL_MACHINE,
        Root::Hkcu => HKEY_CURRENT_USER,
        Root::Hku => HKEY_USERS,
    }
}

fn reg_err(code: u32, key: &str, name: &str) -> String {
    format!("{key}\\{name}: {}", std::io::Error::from_raw_os_error(code as i32))
}

fn root_name(r: Root) -> &'static str {
    match r {
        Root::Hklm => "HKLM",
        Root::Hkcu => "HKCU",
        Root::Hku => "HKU",
    }
}

pub fn set_dword(root: Root, key: &str, name: &str, value: u32) -> Result<(), String> {
    log(&format!("reg {}\\{key} {name} = {value}", root_name(root)));
    let (k, n) = (wide(key), wide(name));
    let r = unsafe { RegSetKeyValueW(hkey(root), k.as_ptr(), n.as_ptr(), REG_DWORD, &value as *const u32 as _, 4) };
    if r == ERROR_SUCCESS {
        Ok(())
    } else {
        let e = reg_err(r, key, name);
        log(&format!("  failed: {e}"));
        Err(e)
    }
}

/// A REG_SZ; an empty name sets the default value of the key.
pub fn set_sz(root: Root, key: &str, name: &str, value: &str) -> Result<(), String> {
    log(&format!("reg {}\\{key} {name} = \"{value}\"", root_name(root)));
    let (k, n, v) = (wide(key), wide(name), wide(value));
    let r = unsafe { RegSetKeyValueW(hkey(root), k.as_ptr(), n.as_ptr(), REG_SZ, v.as_ptr() as _, (v.len() * 2) as u32) };
    if r == ERROR_SUCCESS {
        Ok(())
    } else {
        let e = reg_err(r, key, name);
        log(&format!("  failed: {e}"));
        Err(e)
    }
}

pub fn set_binary(root: Root, key: &str, name: &str, value: &[u8]) -> Result<(), String> {
    use windows_sys::Win32::System::Registry::REG_BINARY;
    let (k, n) = (wide(key), wide(name));
    let r = unsafe { RegSetKeyValueW(hkey(root), k.as_ptr(), n.as_ptr(), REG_BINARY, value.as_ptr() as _, value.len() as u32) };
    if r == ERROR_SUCCESS { Ok(()) } else { Err(reg_err(r, key, name)) }
}

/// The names of the values of a key.
pub fn value_names(root: Root, key: &str) -> Vec<String> {
    use windows_sys::Win32::System::Registry::{KEY_READ, RegCloseKey, RegEnumValueW, RegOpenKeyExW};
    let k = wide(key);
    let mut h: HKEY = null_mut();
    if unsafe { RegOpenKeyExW(hkey(root), k.as_ptr(), 0, KEY_READ, &mut h) } != ERROR_SUCCESS {
        return Vec::new();
    }
    let mut out = Vec::new();
    for i in 0.. {
        let mut buf = [0u16; 512];
        let mut len = buf.len() as u32;
        let r = unsafe { RegEnumValueW(h, i, buf.as_mut_ptr(), &mut len, null_mut(), null_mut(), null_mut(), null_mut()) };
        if r != ERROR_SUCCESS {
            break;
        }
        out.push(String::from_utf16_lossy(&buf[..len as usize]));
    }
    unsafe { RegCloseKey(h) };
    out
}

pub fn get_sz(root: Root, key: &str, name: &str) -> Option<String> {
    let (k, n) = (wide(key), wide(name));
    let mut buf = vec![0u16; 2048];
    let mut size = (buf.len() * 2) as u32;
    let r = unsafe { RegGetValueW(hkey(root), k.as_ptr(), n.as_ptr(), RRF_RT_REG_SZ, null_mut(), buf.as_mut_ptr() as _, &mut size) };
    if r != ERROR_SUCCESS {
        return None;
    }
    let len = (size as usize / 2).saturating_sub(1);
    Some(String::from_utf16_lossy(&buf[..len]))
}

pub fn get_dword(root: Root, key: &str, name: &str) -> Option<u32> {
    use windows_sys::Win32::System::Registry::RRF_RT_REG_DWORD;
    let (k, n) = (wide(key), wide(name));
    let mut v: u32 = 0;
    let mut size = 4u32;
    let r = unsafe { RegGetValueW(hkey(root), k.as_ptr(), n.as_ptr(), RRF_RT_REG_DWORD, null_mut(), &mut v as *mut u32 as _, &mut size) };
    (r == ERROR_SUCCESS).then_some(v)
}

/// The key is there.
pub fn key_exists(root: Root, key: &str) -> bool {
    use windows_sys::Win32::System::Registry::{KEY_READ, RegCloseKey, RegOpenKeyExW};
    let k = wide(key);
    let mut h: HKEY = null_mut();
    let ok = unsafe { RegOpenKeyExW(hkey(root), k.as_ptr(), 0, KEY_READ, &mut h) } == ERROR_SUCCESS;
    if ok {
        unsafe { RegCloseKey(h) };
    }
    ok
}

pub fn delete_tree(root: Root, key: &str) {
    let k = wide(key);
    unsafe { RegDeleteTreeW(hkey(root), k.as_ptr()) };
}

/// Many DWORDs under one key; the first failure is returned but all are tried.
pub fn dwords(root: Root, key: &str, values: &[(&str, u32)]) -> Result<(), String> {
    let mut first = Ok(());
    for (n, v) in values {
        if let Err(e) = set_dword(root, key, n, *v) {
            if first.is_ok() {
                first = Err(e);
            }
        }
    }
    first
}

/// Imports .reg text (written as UTF-16 with a BOM, as regedit writes it).
pub fn reg_import(text: &str) -> Result<(), String> {
    log(&format!("reg import:\n{}", text.trim()));
    let file = std::env::temp_dir().join(format!("HeMimizer-{}.reg", std::process::id()));
    let mut bytes = vec![0xFF, 0xFE];
    for u in text.encode_utf16() {
        bytes.extend_from_slice(&u.to_le_bytes());
    }
    std::fs::write(&file, bytes).map_err(|e| e.to_string())?;
    let r = run("reg.exe", &["import", &file.display().to_string()]);
    let _ = std::fs::remove_file(&file);
    r.map(|_| ())
}

// ---------------------------------------------------------------- processes

pub struct Output {
    pub code: i32,
    pub text: String,
}

/// Console tools write in the OEM code page (852 for Czech); UTF-8 is taken as it is.
fn decode(bytes: &[u8]) -> String {
    use windows_sys::Win32::Globalization::{CP_OEMCP, MultiByteToWideChar};
    if let Ok(s) = std::str::from_utf8(bytes) {
        return s.to_string();
    }
    let n = unsafe { MultiByteToWideChar(CP_OEMCP, 0, bytes.as_ptr(), bytes.len() as i32, null_mut(), 0) };
    let mut w = vec![0u16; n.max(0) as usize];
    unsafe { MultiByteToWideChar(CP_OEMCP, 0, bytes.as_ptr(), bytes.len() as i32, w.as_mut_ptr(), n) };
    String::from_utf16_lossy(&w)
}

/// Runs a program without a window and waits for it; the command, its exit code and its output go to the log.
pub fn exec(exe: &str, args: &[&str]) -> Result<Output, String> {
    let started = std::time::Instant::now();
    if exe == "powershell.exe" {
        log("> powershell (script above)");
    } else {
        log(&format!("> {exe} {}", args.iter().map(|a| if a.contains(' ') { format!("\"{a}\"") } else { a.to_string() }).collect::<Vec<_>>().join(" ")));
    }
    let out = Command::new(exe).args(args).creation_flags(CREATE_NO_WINDOW).output().map_err(|e| {
        log(&format!("< cannot start: {e}"));
        format!("{exe}: {e}")
    })?;
    let mut text = decode(&out.stdout);
    let err = decode(&out.stderr);
    if !err.trim().is_empty() {
        text.push('\n');
        text.push_str(&err);
    }
    let code = out.status.code().unwrap_or(-1);
    // a web page read by curl (no -o) is long and says nothing – only its size goes to the log
    let page = exe == "curl.exe" && !args.contains(&"-o") && !args.contains(&"-sI");
    let shown: String = if page && code == 0 { format!("({} bytes)", text.len()) } else { text.trim().chars().take(3000).collect() };
    log(&format!("< code {code} after {:.1} s{}", started.elapsed().as_secs_f32(), if shown.is_empty() { String::new() } else { format!("\n{shown}") }));
    Ok(Output { code, text })
}

// ---------------------------------------------------------------- the log

static LOG: std::sync::Mutex<Option<(PathBuf, std::fs::File)>> = std::sync::Mutex::new(None);

/// HeMimizer.log in "HeMimizer data" next to HeMimizer.exe; %LOCALAPPDATA%\HeMimizer when that folder is read-only.
/// Only in Debug builds: the release writes no log at all (the user, 2026-11-18 – nothing of HeMimizer stays on the PC).
pub fn log_init() {
    if !cfg!(debug_assertions) {
        return;
    }
    let data = PathBuf::from(env("LOCALAPPDATA", "")).join("HeMimizer");
    let exe_dir = std::env::current_exe().ok().and_then(|e| e.parent().map(PathBuf::from));
    // after a restart the copy in %LOCALAPPDATA%\HeMimizer\app runs: the log stays where HeMimizer was started first
    let origin = exe_dir
        .as_ref()
        .filter(|d| d.starts_with(data.join("app")))
        .and_then(|_| std::fs::read_to_string(data.join("origin.txt")).ok())
        .map(|s| PathBuf::from(s.trim()))
        .filter(|p| p.exists());
    let next_to_exe = origin.or(exe_dir).map(|d| d.join(crate::config::HOME).join("HeMimizer.log"));
    let fallback = PathBuf::from(env("LOCALAPPDATA", "")).join(r"HeMimizer\HeMimizer.log");
    for p in next_to_exe.into_iter().chain([fallback]) {
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        if let Ok(f) = std::fs::OpenOptions::new().create(true).append(true).open(&p) {
            *LOG.lock().unwrap() = Some((p, f));
            return;
        }
    }
}

/// Closes the log file (it can be moved then); later lines are not written.
pub fn log_close() {
    *LOG.lock().unwrap() = None;
}

pub fn log_path() -> PathBuf {
    LOG.lock().unwrap().as_ref().map(|(p, _)| p.clone()).unwrap_or_default()
}

/// A line of the log with the local time.
pub fn log(text: &str) {
    use std::io::Write;
    use windows_sys::Win32::System::SystemInformation::GetLocalTime;
    let t = unsafe {
        let mut st = std::mem::zeroed();
        GetLocalTime(&mut st);
        st
    };
    let stamp = format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}", t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond);
    if let Some((_, f)) = LOG.lock().unwrap().as_mut() {
        let _ = writeln!(f, "{stamp}  {}", text.replace('\n', "\n                     "));
    }
}

/// Runs a program; a non-zero exit code is an error (3010 = done, a restart is needed).
pub fn run(exe: &str, args: &[&str]) -> Result<String, String> {
    let o = exec(exe, args)?;
    if o.code == 0 || o.code == 3010 {
        Ok(o.text)
    } else {
        let name = Path::new(exe).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| exe.into());
        Err(format!("{name} {} {}: {}", crate::i18n::tr("ended with code", "skončil s kódem"), o.code, o.text.trim()))
    }
}

/// A PowerShell script (Windows PowerShell 5.1, part of Windows).
pub fn ps(script: &str) -> Result<String, String> {
    log(&format!("PowerShell:\n{}", script.trim()));
    // a readable .ps1 file instead of -EncodedCommand: an encoded command line is what malware does, and Microsoft
    // Defender's machine learning flagged HeMimizer for it (Trojan:Win32/Bearfoos.A!ml, 2026-11-10)
    let full = format!("$ErrorActionPreference='Stop'; $ProgressPreference='SilentlyContinue'\r\n{script}\r\n");
    static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let dir = std::env::temp_dir().join("HeMimizer");
    let _ = std::fs::create_dir_all(&dir);
    let file = dir.join(format!("step-{}-{}.ps1", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
    // UTF-8 with a BOM, so that Windows PowerShell 5.1 reads Czech letters right
    let mut bytes = vec![0xEF, 0xBB, 0xBF];
    bytes.extend_from_slice(full.as_bytes());
    std::fs::write(&file, bytes).map_err(|e| format!("{}: {e}", file.display()))?;
    let r = run("powershell.exe", &["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File", &file.display().to_string()]);
    let _ = std::fs::remove_file(&file);
    r
}

pub fn is_running(exe_name: &str) -> bool {
    exec("tasklist.exe", &["/FI", &format!("IMAGENAME eq {exe_name}"), "/NH"])
        .map(|o| o.text.to_lowercase().contains(&exe_name.to_lowercase()))
        .unwrap_or(false)
}

// ---------------------------------------------------------------- web

/// A web page as text.
pub fn fetch(url: &str, referer: Option<&str>) -> Result<String, String> {
    let mut args = vec!["-sSL", "--fail", "--retry", "2", "-A", USER_AGENT];
    if let Some(r) = referer {
        args.extend(["-e", r]);
    }
    args.push(url);
    let o = exec("curl.exe", &args)?;
    if o.code != 0 {
        return Err(format!("{url}: curl {} {}", o.code, o.text.trim()));
    }
    Ok(o.text)
}

/// Downloads a file to `dest`.
pub fn download(url: &str, dest: &Path, referer: Option<&str>) -> Result<(), String> {
    if let Some(d) = dest.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    let d = dest.display().to_string();
    let mut args = vec!["-sSL", "--fail", "--retry", "3", "-A", USER_AGENT, "-o", &d];
    if let Some(r) = referer {
        args.extend(["-e", r]);
    }
    args.push(url);
    let o = exec("curl.exe", &args)?;
    if o.code != 0 {
        let _ = std::fs::remove_file(dest);
        return Err(format!("{url}: curl {} {}", o.code, o.text.trim()));
    }
    // every download is a Windows program: it starts with "MZ" (a web page instead of the file does not)
    let mut head = [0u8; 2];
    let ok = std::fs::File::open(dest).and_then(|mut f| std::io::Read::read_exact(&mut f, &mut head)).is_ok() && &head == b"MZ";
    if !ok {
        let _ = std::fs::remove_file(dest);
        return Err(format!(
            "{url}: {}",
            crate::i18n::tr("the server sent a web page instead of the program", "server poslal místo programu webovou stránku")
        ));
    }
    Ok(())
}

/// All links in a page that end with `ends` and contain `contains` (the text between the quotes before it).
pub fn links(html: &str, contains: &str, ends: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut from = 0;
    while let Some(i) = html[from..].find(ends) {
        let end = from + i + ends.len();
        from = end;
        let start = html[..end].rfind(['"', '\'', '>', ' ', '(', '=']).map(|s| s + 1).unwrap_or(0);
        let l = html[start..end].replace("&amp;", "&");
        if l.contains(contains) && !out.contains(&l) {
            out.push(l);
        }
    }
    out
}

/// "https://site/dir/page" + "file" -> "https://site/dir/file"
pub fn join_url(base: &str, link: &str) -> String {
    if link.starts_with("http") {
        link.into()
    } else if let Some(rest) = link.strip_prefix('/') {
        let host_end = base.find("://").map(|i| i + 3).and_then(|i| base[i..].find('/').map(|j| i + j)).unwrap_or(base.len());
        format!("{}/{rest}", &base[..host_end])
    } else {
        format!("{}/{link}", base.trim_end_matches(|c| c != '/').trim_end_matches('/'))
    }
}

// ---------------------------------------------------------------- folders

pub fn known_folder(id: &windows_sys::core::GUID) -> Option<PathBuf> {
    use windows_sys::Win32::System::Com::CoTaskMemFree;
    use windows_sys::Win32::UI::Shell::SHGetKnownFolderPath;
    let mut p: windows_sys::core::PWSTR = null_mut();
    let r = unsafe { SHGetKnownFolderPath(id, 0, null_mut(), &mut p) };
    if r != 0 || p.is_null() {
        return None;
    }
    let len = (0..).take_while(|&i| unsafe { *p.add(i) } != 0).count();
    let s = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(p, len) });
    unsafe { CoTaskMemFree(p as _) };
    Some(PathBuf::from(s))
}

pub fn downloads() -> PathBuf {
    known_folder(&windows_sys::Win32::UI::Shell::FOLDERID_Downloads).unwrap_or_else(|| profile().join("Downloads"))
}

pub fn pictures() -> PathBuf {
    known_folder(&windows_sys::Win32::UI::Shell::FOLDERID_Pictures).unwrap_or_else(|| profile().join("Pictures"))
}

pub fn documents() -> PathBuf {
    known_folder(&windows_sys::Win32::UI::Shell::FOLDERID_Documents).unwrap_or_else(|| profile().join("Documents"))
}

pub fn profile() -> PathBuf {
    PathBuf::from(std::env::var("USERPROFILE").unwrap_or_else(|_| r"C:\Users\Public".into()))
}

pub fn env(name: &str, fallback: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| fallback.into())
}

/// Where HeMimizer downloads the installers: a folder in the user's Downloads, deleted at the end.
pub fn work_dir() -> PathBuf {
    downloads().join("HeMimizer")
}

pub fn shell_open(target: &str) {
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let (op, t) = (wide("open"), wide(target));
    unsafe { ShellExecuteW(null_mut(), op.as_ptr(), t.as_ptr(), std::ptr::null(), std::ptr::null(), SW_SHOWNORMAL) };
}

/// Tells Explorer that file associations changed.
pub fn assoc_changed() {
    use windows_sys::Win32::UI::Shell::{SHCNE_ASSOCCHANGED, SHCNF_IDLIST, SHChangeNotify};
    unsafe { SHChangeNotify(SHCNE_ASSOCCHANGED as i32, SHCNF_IDLIST, std::ptr::null(), std::ptr::null()) };
}

/// Tells running programs that settings changed (Explorer re-reads some of them).
pub fn settings_changed() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{HWND_BROADCAST, SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_SETTINGCHANGE};
    let s = wide("ImmersiveColorSet");
    let p = wide("Policy");
    unsafe {
        SendMessageTimeoutW(HWND_BROADCAST, WM_SETTINGCHANGE, 0, s.as_ptr() as isize, SMTO_ABORTIFHUNG, 2000, null_mut());
        SendMessageTimeoutW(HWND_BROADCAST, WM_SETTINGCHANGE, 0, p.as_ptr() as isize, SMTO_ABORTIFHUNG, 2000, null_mut());
    }
}

pub fn remove_file(p: impl AsRef<Path>) {
    let _ = std::fs::remove_file(p);
}
