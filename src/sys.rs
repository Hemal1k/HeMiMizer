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

/// The full path of a program of Windows given by its name ("curl.exe"): from System32, PowerShell from its own folder
/// there, winget from WindowsApps. Never just the name: Windows would look into the folder of HeMiMizer.exe first (often
/// Downloads), where any "curl.exe" would then run with administrator rights. A path is returned as it is.
pub fn program(exe: &str) -> PathBuf {
    if exe.contains(['\\', '/']) {
        return PathBuf::from(exe);
    }
    let system = {
        use windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW;
        let mut buf = [0u16; 260];
        // SAFETY: the buffer is valid for its length; the call writes at most that many characters
        let n = unsafe { GetSystemDirectoryW(buf.as_mut_ptr(), buf.len() as u32) } as usize;
        if n > 0 && n < buf.len() {
            PathBuf::from(String::from_utf16_lossy(&buf[..n]))
        } else {
            PathBuf::from(env("SystemRoot", r"C:\Windows")).join("System32")
        }
    };
    match exe.to_ascii_lowercase().as_str() {
        "powershell.exe" => system.join(r"WindowsPowerShell\v1.0\powershell.exe"),
        "winget.exe" => PathBuf::from(env("LOCALAPPDATA", "")).join(r"Microsoft\WindowsApps\winget.exe"),
        "explorer.exe" => PathBuf::from(env("SystemRoot", r"C:\Windows")).join("explorer.exe"),
        _ => system.join(exe),
    }
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

/// Every key of the .reg text lies under one of `allowed` (the key itself or below it), else the first one that does
/// not.
pub fn reg_keys_outside<'a>(text: &'a str, allowed: &[&str]) -> Option<&'a str> {
    text.lines().map(str::trim).filter(|l| l.starts_with('[')).find(|l| {
        let key = l.trim_start_matches('[').trim_start_matches('-').trim_end_matches(']').trim().to_lowercase();
        !allowed.iter().any(|a| {
            let a = a.trim_end_matches('\\').to_lowercase();
            key == a || key.starts_with(&format!("{a}\\"))
        })
    })
}

/// Imports .reg text (written as UTF-16 with a BOM, as regedit writes it) whose keys all lie under `allowed`. A backup
/// is not checked by anyone and HeMiMizer runs as administrator: a .reg with any other key (services, Run, IFEO…) is
/// refused as a whole (audit 2026-11-24).
pub fn reg_import_under(text: &str, allowed: &[&str]) -> Result<(), String> {
    if let Some(key) = reg_keys_outside(text, allowed) {
        log(&format!("reg import refused, key outside {allowed:?}: {key}"));
        return Err(format!("{} {key}", crate::i18n::tr("Not imported – the backup holds a key outside the allowed ones:", "Neimportováno – záloha obsahuje klíč mimo povolené:")));
    }
    log(&format!("reg import:\n{}", text.trim()));
    let file = scripts_dir()?.join(format!("import-{}.reg", std::process::id()));
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
    let out = Command::new(program(exe)).args(args).creation_flags(CREATE_NO_WINDOW).output().map_err(|e| {
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

/// HeMiMizer.log in "HeMiMizer data" next to HeMiMizer.exe; %LOCALAPPDATA%\HeMiMizer when that folder is read-only.
/// Only in Debug builds: the release writes no log at all (the user, 2026-11-18 – nothing of HeMiMizer stays on the PC).
pub fn log_init() {
    if !cfg!(debug_assertions) {
        return;
    }
    let data = state_dir();
    let exe_dir = std::env::current_exe().ok().and_then(|e| e.parent().map(PathBuf::from));
    // after a restart the copy in %ProgramData%\HeMiMizer\app runs: the log stays where HeMiMizer was started first
    let origin = exe_dir
        .as_ref()
        .filter(|d| d.starts_with(data.join("app")))
        .and_then(|_| std::fs::read_to_string(data.join("origin.txt")).ok())
        .map(|s| PathBuf::from(s.trim()))
        .filter(|p| p.exists());
    let next_to_exe = origin.or(exe_dir).map(|d| d.join(crate::config::HOME).join("HeMiMizer.log"));
    let fallback = PathBuf::from(env("LOCALAPPDATA", "")).join(r"HeMiMizer\HeMiMizer.log");
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
    // Defender's machine learning flagged HeMiMizer for it (Trojan:Win32/Bearfoos.A!ml, 2026-11-10)
    let full = format!("$ErrorActionPreference='Stop'; $ProgressPreference='SilentlyContinue'\r\n{script}\r\n");
    static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    // the protected folder: no program of the user can change the script between writing and running it
    let dir = scripts_dir()?;
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
    // HTTPS only, also after a redirect
    let mut args = vec!["-sSL", "--proto", "=https", "--proto-redir", "=https", "--fail", "--retry", "3", "-A", USER_AGENT, "-o", &d];
    if let Some(r) = referer {
        args.extend(["-e", r]);
    }
    args.push(url);
    let o = exec("curl.exe", &args)?;
    if o.code != 0 {
        let _ = std::fs::remove_file(dest);
        return Err(format!("{url}: curl {} {}", o.code, o.text.trim()));
    }
    // every download is a Windows program: it starts with "MZ", an MSI package with the header of a compound file
    // (a web page instead of the file does not)
    let mut head = [0u8; 4];
    let ok = std::fs::File::open(dest).and_then(|mut f| std::io::Read::read_exact(&mut f, &mut head)).is_ok()
        && (head.starts_with(b"MZ") || head == [0xD0, 0xCF, 0x11, 0xE0] || (head.starts_with(b"PK") && is_appx(dest)));
    if !ok {
        let _ = std::fs::remove_file(dest);
        return Err(format!(
            "{url}: {}",
            crate::i18n::tr("the server sent a web page instead of the installer", "server poslal místo instalačního programu webovou stránku")
        ));
    }
    // it runs with administrator rights: checked first (audit 2026-11-24)
    if let Err(e) = verify(url, dest) {
        let _ = std::fs::remove_file(dest);
        return Err(format!("{url}: {e}"));
    }
    Ok(())
}

fn is_appx(file: &Path) -> bool {
    file.extension().is_some_and(|e| ["msix", "msixbundle", "appx", "appxbundle"].iter().any(|x| e.eq_ignore_ascii_case(x)))
}

/// A downloaded program before it is run: from a GitHub release – its SHA256 against SHA256SUMS.txt of that release
/// (the HeMi apps are not signed); from anywhere else (Microsoft, AMD) – a valid Authenticode signature.
fn verify(url: &str, file: &Path) -> Result<(), String> {
    use crate::i18n::tr;
    let name = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    // an app package (winget's App Installer): Windows itself refuses one without a valid signature when installing it
    if is_appx(file) {
        return Ok(());
    }
    // oscdimg.exe from Microsoft's symbol server has no signature of its own (the ADK signs it by a catalog): the
    // SHA-256 of the known file, checked here right after the download
    if url == crate::iso::OSCDIMG_URL {
        let bytes = std::fs::read(file).map_err(|e| e.to_string())?;
        let have: String = sha2::Digest::finalize(<sha2::Sha256 as sha2::Digest>::new_with_prefix(&bytes)).iter().map(|b| format!("{b:02x}")).collect();
        return if have.eq_ignore_ascii_case(crate::iso::OSCDIMG_SHA256) {
            Ok(())
        } else {
            Err(format!("oscdimg.exe from Microsoft is not the expected file (SHA-256 {have})"))
        };
    }
    if url.starts_with("https://github.com/") && url.contains("/releases/") {
        let asset = url.rsplit('/').next().unwrap_or("");
        let sums_url = format!("{}SHA256SUMS.txt", &url[..url.len() - asset.len()]);
        let sums = fetch(&sums_url, None).map_err(|e| format!("SHA256SUMS.txt: {e}"))?;
        let want = sums
            .lines()
            .find_map(|l| l.split_once(char::is_whitespace).filter(|(_, f)| f.trim().trim_start_matches('*').eq_ignore_ascii_case(asset)).map(|(h, _)| h.trim().to_lowercase()))
            .ok_or_else(|| format!("SHA256SUMS.txt {} {asset}", tr("has no line for", "nemá řádek pro")))?;
        let bytes = std::fs::read(file).map_err(|e| e.to_string())?;
        let have: String = sha2::Digest::finalize(<sha2::Sha256 as sha2::Digest>::new_with_prefix(&bytes)).iter().map(|b| format!("{b:02x}")).collect();
        return if have == want {
            Ok(())
        } else {
            Err(tr("the download is damaged or changed (its SHA256 does not match) – not run", "stažený soubor je poškozený nebo změněný (nesedí SHA256) – nespuštěn").into())
        };
    }
    let out = exec("powershell.exe", &[
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        &format!("(Get-AuthenticodeSignature -LiteralPath '{}').Status", file.display().to_string().replace('\'', "''")),
    ])?;
    if out.text.trim() == "Valid" {
        Ok(())
    } else {
        Err(format!(
            "{name}: {} ({})",
            tr("no valid digital signature – not run", "nemá platný digitální podpis – nespuštěn"),
            out.text.trim()
        ))
    }
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

/// Where HeMiMizer downloads the installers: `work` in the protected folder, deleted at the end. Not in the user's
/// Downloads any more: a program of the user could swap an installer or downloads.txt there before HeMiMizer ran it
/// as administrator (audit 2026-11-24).
pub fn work_dir() -> PathBuf {
    // made again (protected) when the end of a run removed it
    if let Err(e) = protect_state_dir() {
        log(&format!("protected folder: {e}"));
    }
    state_dir().join("work")
}

// ---------------------------------------------------------------- the protected folder

/// Owned by the Administrators; full control for them and SYSTEM, read and run for the users; nothing inherited from
/// ProgramData (where every user may create files and folders).
const PROTECTED_SDDL: &str = "O:BAD:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;FRFX;;;BU)";
/// The same permissions as Windows writes them back (FRFX = 0x1200a9; "AI" left out, see [`dacl_of`]).
const PROTECTED_DACL: &str = "D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;0x1200a9;;;BU)";

/// %ProgramData%\HeMiMizer: everything HeMiMizer writes as administrator and reads or runs later – its scripts, the
/// downloaded installers, the copy for the restarts with its config and the state between the restarts. Only
/// administrators may change anything in it, so a program running as the user cannot put its own code in there for the
/// administrator (audit 2026-11-24). ProgramData from the registry of Windows, not from the environment (a user may set
/// his own %ProgramData%).
pub fn state_dir() -> PathBuf {
    known_folder(&windows_sys::Win32::UI::Shell::FOLDERID_ProgramData)
        .unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"))
        .join("HeMiMizer")
}

/// Makes sure [`state_dir`] is the protected folder. One HeMiMizer made is kept (a real folder owned by the
/// Administrators or SYSTEM with exactly these permissions); anything else of that name – a folder a user made, a link –
/// goes, and the folder is made anew with its permissions in the same call (no moment without them). Without
/// administrator rights (a Debug build) the folder is just made: nothing runs elevated then.
pub fn protect_state_dir() -> Result<(), String> {
    let dir = state_dir();
    if !elevated() {
        return std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()));
    }
    if std::fs::symlink_metadata(&dir).is_ok() && is_protected(&dir) {
        return Ok(());
    }
    make_protected_dir(&dir)
}

/// A new folder with the protected permissions: whatever is at `dir` goes first (a link only as the link, not what it
/// points to); the folder is made with its permissions in one call, and that call fails when someone made it again in
/// between. Without administrator rights just a folder.
pub fn make_protected_dir(dir: &Path) -> Result<(), String> {
    use std::os::windows::fs::MetadataExt;
    if !elevated() {
        return std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()));
    }
    if let Ok(m) = std::fs::symlink_metadata(dir) {
        log(&format!("{} is not the protected folder of HeMiMizer: made anew", dir.display()));
        // FILE_ATTRIBUTE_REPARSE_POINT: a junction or a link – removed itself
        let gone = if m.file_type().is_symlink() || m.file_attributes() & 0x400 != 0 {
            std::fs::remove_dir(dir).or_else(|_| std::fs::remove_file(dir))
        } else if m.is_dir() {
            std::fs::remove_dir_all(dir)
        } else {
            std::fs::remove_file(dir)
        };
        gone.map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    if let Some(p) = dir.parent() {
        std::fs::create_dir_all(p).map_err(|e| format!("{}: {e}", p.display()))?;
    }
    create_dir_with(dir, PROTECTED_SDDL)
}

/// Creates one folder with the permissions of `sddl` (CreateDirectoryW with its security descriptor).
fn create_dir_with(dir: &Path, sddl: &str) -> Result<(), String> {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Authorization::{ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1};
    use windows_sys::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
    use windows_sys::Win32::Storage::FileSystem::CreateDirectoryW;
    let (name, text) = (wide(&dir.display().to_string()), wide(sddl));
    // SAFETY: both strings end with a NUL; the descriptor is made by Windows, used during the call and freed after it
    unsafe {
        let mut sd: PSECURITY_DESCRIPTOR = null_mut();
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(text.as_ptr(), SDDL_REVISION_1, &mut sd, null_mut()) == 0 {
            return Err(format!("{}: {}", dir.display(), std::io::Error::last_os_error()));
        }
        let sa = SECURITY_ATTRIBUTES { nLength: size_of::<SECURITY_ATTRIBUTES>() as u32, lpSecurityDescriptor: sd, bInheritHandle: 0 };
        let ok = CreateDirectoryW(name.as_ptr(), &sa) != 0;
        let err = std::io::Error::last_os_error();
        LocalFree(sd);
        if ok { Ok(()) } else { Err(format!("{}: {err}", dir.display())) }
    }
}

/// The permissions (DACL) of a file or folder as SDDL, with "AI" (Windows marks a DACL set afterwards as
/// auto-inherited) left out.
fn dacl_of(path: &Path) -> Option<String> {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Authorization::{ConvertSecurityDescriptorToStringSecurityDescriptorW, GetNamedSecurityInfoW, SDDL_REVISION_1, SE_FILE_OBJECT};
    use windows_sys::Win32::Security::{DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR};
    let name = wide(&path.display().to_string());
    // SAFETY: the out pointers are valid; the descriptor and the string are freed with LocalFree; the string ends with a NUL
    unsafe {
        let mut sd: PSECURITY_DESCRIPTOR = null_mut();
        if GetNamedSecurityInfoW(name.as_ptr(), SE_FILE_OBJECT, DACL_SECURITY_INFORMATION, null_mut(), null_mut(), null_mut(), null_mut(), &mut sd) != ERROR_SUCCESS {
            return None;
        }
        let mut text: *mut u16 = null_mut();
        let ok = ConvertSecurityDescriptorToStringSecurityDescriptorW(sd, SDDL_REVISION_1, DACL_SECURITY_INFORMATION, &mut text, null_mut()) != 0;
        LocalFree(sd);
        if !ok || text.is_null() {
            return None;
        }
        let len = (0..).take_while(|&i| *text.add(i) != 0).count();
        let s = String::from_utf16_lossy(std::slice::from_raw_parts(text, len));
        LocalFree(text.cast());
        Some(s.replacen("D:PAI(", "D:P(", 1))
    }
}

/// A real folder (no link), owned by the Administrators or SYSTEM, with exactly the protected permissions.
fn is_protected(dir: &Path) -> bool {
    use std::os::windows::fs::MetadataExt;
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Authorization::{GetNamedSecurityInfoW, SE_FILE_OBJECT};
    use windows_sys::Win32::Security::{IsWellKnownSid, OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID, WinBuiltinAdministratorsSid, WinLocalSystemSid};
    let real = std::fs::symlink_metadata(dir).is_ok_and(|m| m.is_dir() && !m.file_type().is_symlink() && m.file_attributes() & 0x400 == 0);
    if !real || dacl_of(dir).as_deref() != Some(PROTECTED_DACL) {
        return false;
    }
    let name = wide(&dir.display().to_string());
    // SAFETY: the out pointers are valid; `owner` points into the descriptor, which is freed after its last use
    unsafe {
        let mut owner: PSID = null_mut();
        let mut sd: PSECURITY_DESCRIPTOR = null_mut();
        if GetNamedSecurityInfoW(name.as_ptr(), SE_FILE_OBJECT, OWNER_SECURITY_INFORMATION, &mut owner, null_mut(), null_mut(), null_mut(), &mut sd) != ERROR_SUCCESS {
            return false;
        }
        let ok = !owner.is_null() && (IsWellKnownSid(owner, WinBuiltinAdministratorsSid) != 0 || IsWellKnownSid(owner, WinLocalSystemSid) != 0);
        LocalFree(sd);
        ok
    }
}

/// After the window closed: the protected folder goes when nothing is left in it (an empty folder of scripts too).
pub fn tidy_state_dir() {
    let dir = state_dir();
    let _ = std::fs::remove_dir(dir.join("scripts"));
    let _ = std::fs::remove_dir(&dir);
}

/// The scripts and .reg files HeMiMizer writes and runs at once: in the protected folder, not in %TEMP%.
pub fn scripts_dir() -> Result<PathBuf, String> {
    protect_state_dir()?;
    let dir = state_dir().join("scripts");
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    Ok(dir)
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

/// HeMiMizer runs as administrator (its token is elevated).
pub fn elevated() -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::Security::{GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation};
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    unsafe {
        let mut token: HANDLE = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return false;
        }
        let mut e = TOKEN_ELEVATION { TokenIsElevated: 0 };
        let mut size = 0u32;
        let ok = GetTokenInformation(token, TokenElevation, (&mut e as *mut TOKEN_ELEVATION).cast(), size_of::<TOKEN_ELEVATION>() as u32, &mut size) != 0;
        CloseHandle(token);
        ok && e.TokenIsElevated != 0
    }
}

/// The checks of the audit of 2026-11-24 (nothing here changes the system: only a folder in %TEMP%).
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reg_only_under_the_allowed_keys() {
        let user = "Windows Registry Editor Version 5.00\r\n\r\n[HKEY_CURRENT_USER\\Software\\7-Zip]\r\n\"Lang\"=\"-\"\r\n[-HKEY_CURRENT_USER\\Software\\7-Zip\\FM]\r\n";
        assert_eq!(reg_keys_outside(user, &[r"HKEY_CURRENT_USER\Software\7-Zip"]), None);
        assert_eq!(reg_keys_outside(user, &["HKEY_CURRENT_USER"]), None);
        let machine = format!("{user}[HKEY_LOCAL_MACHINE\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Run]\r\n\"x\"=\"c:\\\\x.exe\"\r\n");
        assert!(reg_keys_outside(&machine, &[r"HKEY_CURRENT_USER\Software\7-Zip"]).is_some());
        // a key that only starts like the allowed one
        assert!(reg_keys_outside("[HKEY_CURRENT_USER\\Software\\7-ZipEvil]", &[r"HKEY_CURRENT_USER\Software\7-Zip"]).is_some());
    }

    /// The permissions as Windows writes them back match what the check of the protected folder expects (the owner
    /// rights added, so that the test can delete the folder without administrator rights).
    #[test]
    fn protected_permissions_read_back() {
        let dir = std::env::temp_dir().join(format!("hmz-protected-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        create_dir_with(&dir, &format!("{}(A;OICI;FA;;;OW)", PROTECTED_SDDL.trim_start_matches("O:BA"))).unwrap();
        assert_eq!(dacl_of(&dir).unwrap(), format!("{PROTECTED_DACL}(A;OICI;FA;;;OW)"));
        // not owned by the Administrators (made without administrator rights): not trusted
        assert!(!is_protected(&dir));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn font_names_only() {
        assert!(crate::windows::font_file_ok("Inter.ttf"));
        assert!(crate::windows::font_file_ok("x.OTF"));
        for bad in [r"..\System32\x.ttf", r"C:\Windows\x.ttf", "sub/x.ttf", "x.dll", "..", "x.ttf:stream"] {
            assert!(!crate::windows::font_file_ok(bad), "{bad}");
        }
    }

    #[test]
    fn folders_only_in_allowed_places() {
        use crate::plan::safe_folder;
        assert!(safe_folder(r"{PROFILE}\Documents\Projects").is_some());
        assert!(safe_folder(r"D:\Projects\HeMi").is_some());
        let windir = program("x").parent().and_then(|p| p.parent()).unwrap().display().to_string();
        for bad in [
            r"{PROFILE}\..\Other".to_string(),
            r"{PROFILE}".into(),
            r"D:\".into(),
            r"Projects".into(),
            format!(r"{windir}\System32"),
            env("ProgramFiles", r"C:\Program Files") + r"\Something",
            env("ProgramData", r"C:\ProgramData") + r"\Microsoft\Windows\Start Menu\Programs\StartUp",
            format!(r"{}\Users\Someone\Desktop", &windir[..2]),
        ] {
            assert!(safe_folder(&bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn drives_only_plain_shares() {
        let d = |letter: &str, share: &str, ip: &str| crate::config::Drive { letter: letter.into(), share: share.into(), label: String::new(), user: String::new(), ip: ip.into() };
        assert!(crate::config::valid_drive(&d("H", r"\\nas\home", "192.168.1.5")));
        assert!(crate::config::valid_drive(&d("M", r"\\nas.local\Music (old)\Bob's", "")));
        for bad in [d("H", r#"\\nas\home" & calc & ""#, ""), d("H", r"\\nas\a|b", ""), d("HH", r"\\nas\home", ""), d("H", r"\\nas", ""), d("H", r"\\na s\x", ""), d("H", r"\\nas\home", "1.2.3.4; calc")] {
            assert!(!crate::config::valid_drive(&bad), "{}", bad.share);
        }
    }

    #[test]
    fn defender_protection_not_switched_off() {
        use crate::windows::weakens_defender;
        for (n, v) in [("DisableRealtimeMonitoring", "True"), ("DisableBehaviorMonitoring", "true"), ("MAPSReporting", "0"), ("SubmitSamplesConsent", "2"), ("PUAProtection", "0"), ("MemoryIntegrity", "0")] {
            assert!(weakens_defender(n, v), "{n}={v}");
        }
        for (n, v) in [("DisableRealtimeMonitoring", "False"), ("MAPSReporting", "2"), ("PUAProtection", "1"), ("DisableCpuThrottleOnIdleScans", "True"), ("ScanAvgCPULoadFactor", "0"), ("MemoryIntegrity", "1")] {
            assert!(!weakens_defender(n, v), "{n}={v}");
        }
    }

    #[test]
    fn installer_switches_from_the_list() {
        assert!(crate::install::switch_allowed("/verysilent"));
        assert!(crate::install::switch_allowed("/S"));
        for bad in ["/LOADINF=x.inf", "/DIR=C:\\Windows", "INSTALLDIR=C:\\x", "&", "/TASKS=x"] {
            assert!(!crate::install::switch_allowed(bad), "{bad}");
        }
    }
}
