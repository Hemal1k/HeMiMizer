//! "Download Windows": the ISO of Windows 11 or 10 in the language chosen, straight from Microsoft – the way of its own
//! download page (and of Fido, the script of Rufus): a session is registered (vlscppe + ov-df, else Microsoft's
//! Sentinel rejects the request), the languages come from getskuinformationbyproductedition, the link of the ISO
//! from GetProductDownloadLinksBySku. The link is valid for 24 hours; the file is downloaded by curl with its progress.

use crate::sys;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};

const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0 Safari/537.36";
/// The page of Windows 10 offers the ISO only to a browser that is not on Windows.
const UA_OTHER: &str = "Mozilla/5.0 (X11; Linux x86_64; rv:130.0) Gecko/20100101 Firefox/130.0";
const PROFILE: &str = "606624d44113";
const INSTANCE: &str = "560dc9f3-1aa5-4a2f-b63c-9e18f8d0e175";

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Product {
    Windows11,
    Windows10,
}

impl Product {
    pub fn name(self) -> &'static str {
        match self {
            Product::Windows11 => "Windows 11",
            Product::Windows10 => "Windows 10",
        }
    }

    fn page(self) -> &'static str {
        match self {
            Product::Windows11 => "https://www.microsoft.com/en-us/software-download/windows11",
            Product::Windows10 => "https://www.microsoft.com/en-us/software-download/windows10ISO",
        }
    }
}

/// A started session with Microsoft: its id and the cookies of curl.
#[derive(Debug)]
pub struct Session {
    product: Product,
    edition: String,
    id: String,
    jar: PathBuf,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Language {
    pub sku: String,
    /// as Microsoft names it (English), e.g. "Czech"
    pub name: String,
}

fn curl(args: &[&str]) -> Result<String, String> {
    let o = std::process::Command::new(crate::sys::program("curl.exe")).args(args).creation_flags(0x0800_0000).output().map_err(|e| format!("curl: {e}"))?;
    if !o.status.success() {
        return Err(format!("curl {}: {}", o.status.code().unwrap_or(-1), String::from_utf8_lossy(&o.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&o.stdout).into_owned())
}

fn get(s: &Session, url: &str, ua: &str, referer: Option<&str>) -> Result<String, String> {
    let jar = s.jar.display().to_string();
    let mut args = vec!["-sSL", "--fail", "-A", ua, "-b", &jar, "-c", &jar];
    if let Some(r) = referer {
        args.extend(["-e", r]);
    }
    args.push(url);
    curl(&args)
}

/// A new session: the product's edition from its page, then the session registered the way the page does it.
pub fn start(product: Product) -> Result<Session, String> {
    let dir = std::env::temp_dir().join("HeMiMizer");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let id = sys::ps("[guid]::NewGuid().ToString()")?.trim().to_string();
    let mut s = Session { product, edition: String::new(), id: id.clone(), jar: dir.join(format!("ms-{id}.cookies")) };
    let page = get(&s, product.page(), if product == Product::Windows10 { UA_OTHER } else { UA }, None)?;
    // <option value="3813">Windows 11 (multi-edition ISO for x64 devices)</option>
    s.edition = page
        .split("option value=\"")
        .skip(1)
        .filter_map(|p| p.split_once('"'))
        .find(|(v, rest)| v.chars().all(|c| c.is_ascii_digit()) && !v.is_empty() && rest.contains("ISO"))
        .map(|(v, _)| v.to_string())
        .ok_or("Microsoft's page offers no ISO now (it may have changed)")?;
    get(&s, &format!("https://vlscppe.microsoft.com/tags?org_id=y6jn8c31&session_id={id}"), UA, None)?;
    let js = get(&s, &format!("https://ov-df.microsoft.com/mdt.js?instanceId={INSTANCE}&PageId=si&session_id={id}"), UA, None)?;
    if let Some(u) = js.split("url:\"").nth(1).and_then(|r| r.split('"').next()) {
        let _ = get(&s, &u.replace("&amp;", "&"), UA, None);
    }
    Ok(s)
}

/// The languages of the product.
pub fn languages(s: &Session) -> Result<Vec<Language>, String> {
    let t = get(
        s,
        &format!(
            "https://www.microsoft.com/software-download-connector/api/getskuinformationbyproductedition?profile={PROFILE}&ProductEditionId={}&SKU=undefined&friendlyFileName=undefined&Locale=en-US&sessionID={}",
            s.edition, s.id
        ),
        UA,
        Some(s.product.page()),
    )?;
    let v: serde_json::Value = serde_json::from_str(&t).map_err(|e| e.to_string())?;
    if let Some(e) = v["Errors"].as_array().and_then(|a| a.first()) {
        return Err(format!("Microsoft: {}", e["Value"].as_str().unwrap_or("refused")));
    }
    let mut out: Vec<Language> = v["Skus"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|k| Some(Language { sku: k["Id"].as_str()?.to_string(), name: k["Language"].as_str()?.to_string() }))
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    if out.is_empty() { Err("Microsoft sent no languages".into()) } else { Ok(out) }
}

/// The link of the 64-bit ISO in that language (valid for 24 hours).
pub fn link(s: &Session, lang: &Language) -> Result<(String, String), String> {
    let t = get(
        s,
        &format!(
            "https://www.microsoft.com/software-download-connector/api/GetProductDownloadLinksBySku?profile={PROFILE}&productEditionId=undefined&SKU={}&friendlyFileName=undefined&Locale=en-US&sessionID={}",
            lang.sku, s.id
        ),
        UA,
        Some(s.product.page()),
    )?;
    let v: serde_json::Value = serde_json::from_str(&t).map_err(|e| e.to_string())?;
    if let Some(e) = v["Errors"].as_array().and_then(|a| a.first()) {
        return Err(format!("Microsoft refused it ({}) – try again in a while", e["Value"].as_str().unwrap_or("rejected")));
    }
    let opts: Vec<(String, String)> = v["ProductDownloadOptions"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|o| Some((o["Name"].as_str().unwrap_or("").to_string(), o["Uri"].as_str()?.to_string())))
        .collect();
    let pick = opts.iter().find(|(n, u)| n.contains("64") || u.contains("x64")).or(opts.first()).cloned().ok_or("Microsoft sent no link")?;
    let file = pick.1.split('?').next().and_then(|p| p.rsplit('/').next()).filter(|f| f.to_lowercase().ends_with(".iso")).unwrap_or("Windows.iso").to_string();
    Ok((pick.1, file))
}

/// The size of the file behind the link (0 when unknown).
pub fn size(url: &str) -> u64 {
    curl(&["-sIL", "-A", UA, url])
        .ok()
        .and_then(|h| h.lines().filter_map(|l| l.to_lowercase().strip_prefix("content-length:").map(|v| v.trim().to_string())).next_back())
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
}

/// Downloads to `dest` (a .part file first), telling the progress (done, total bytes).
pub fn download(url: &str, dest: &Path, progress: &dyn Fn(u64, u64)) -> Result<(), String> {
    let total = size(url);
    let part = dest.with_extension("iso.part");
    let p = part.display().to_string();
    let mut child = std::process::Command::new(crate::sys::program("curl.exe"))
        .args(["-sSL", "--proto", "=https", "--proto-redir", "=https", "--fail", "--retry", "3", "-C", "-", "-A", UA, "-o", &p, url])
        .creation_flags(0x0800_0000)
        .spawn()
        .map_err(|e| format!("curl: {e}"))?;
    STOPPED.store(false, std::sync::atomic::Ordering::SeqCst);
    RUNNING.store(child.id(), std::sync::atomic::Ordering::SeqCst);
    let code = loop {
        if let Some(st) = child.try_wait().map_err(|e| e.to_string())? {
            break st.code().unwrap_or(-1);
        }
        progress(std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0), total);
        std::thread::sleep(std::time::Duration::from_millis(700));
    };
    RUNNING.store(0, std::sync::atomic::Ordering::SeqCst);
    if STOPPED.load(std::sync::atomic::Ordering::SeqCst) {
        return Err(STOPPED_TEXT.into());
    }
    if code != 0 {
        return Err(format!("The download stopped (curl {code}) – start it again, it goes on where it stopped"));
    }
    let _ = std::fs::remove_file(dest);
    std::fs::rename(&part, dest).map_err(|e| e.to_string())
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.jar);
    }
}

/// The curl of a download that runs (0 = none): stopped when HeMiMizer closes, else it went on without the window.
static RUNNING: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// Stop was clicked (not an error of the download).
static STOPPED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
pub const STOPPED_TEXT: &str = "Stopped. Download again to go on where it stopped.";

/// Stops a download that runs (the .part file stays: the next start goes on from it).
pub fn stop() {
    let pid = RUNNING.swap(0, std::sync::atomic::Ordering::SeqCst);
    if pid != 0 {
        STOPPED.store(true, std::sync::atomic::Ordering::SeqCst);
        let _ = std::process::Command::new(crate::sys::program("taskkill.exe")).args(["/PID", &pid.to_string(), "/F"]).creation_flags(0x0800_0000).status();
    }
}
