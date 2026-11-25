//! A backup protected with a password: the parts of the backup that are personal (config – the settings, Wi-Fi
//! passwords, network drives –, the settings of the programs and your own folders) are packed and encrypted into
//! "sealed.hmz" in the folder of the backup; the installers stay as they are (nothing secret in them).
//!
//! AES-256-GCM (RustCrypto) with a key from the password through PBKDF2-HMAC-SHA256 (600 000 rounds, a random salt),
//! in chunks of 1 MiB – each with its own nonce (a random prefix + the number of the chunk) and its own tag, the last
//! one marked, so nothing can be changed, reordered or cut off unnoticed. Without the password nothing can be read.
//!
//! The file: "HMZ1" | salt (16) | rounds (u32 LE) | nonce prefix (8) | chunks: length (u32 LE, top bit = the last
//! chunk) + ciphertext with its tag.

use crate::{config, sys};
use aes_gcm::aead::rand_core::RngCore;
use aes_gcm::aead::{Aead, KeyInit, OsRng, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

const MAGIC: &[u8; 4] = b"HMZ1";
const ROUNDS: u32 = 600_000;
const CHUNK: usize = 1 << 20;
const LAST: u32 = 0x8000_0000;

/// The encrypted part of the backup, in "HeMiMizer data".
pub const SEALED: &str = "sealed.hmz";
/// What is encrypted (folders in "HeMiMizer data").
const PARTS: &[&str] = &["config", "apps", "folders"];

pub fn sealed_file() -> PathBuf {
    config::home().join(SEALED)
}

/// A protected backup that is not unlocked yet (its settings are not there in the open).
pub fn locked() -> bool {
    sealed_file().is_file() && !config::dir().exists()
}

/// The rounds a backup may say it has: fewer would be too weak, more would block the window for hours.
const ROUNDS_OK: std::ops::RangeInclusive<u32> = 100_000..=10_000_000;

fn key(password: &str, salt: &[u8], rounds: u32) -> Aes256Gcm {
    let mut k = [0u8; 32];
    pbkdf2::pbkdf2_hmac::<sha2::Sha256>(password.as_bytes(), salt, rounds, &mut k);
    let cipher = Aes256Gcm::new_from_slice(&k).expect("32 bytes");
    wipe_bytes(&mut k);
    cipher
}

/// Overwrites the bytes with zeros (volatile, so the compiler does not leave it out).
fn wipe_bytes(b: &mut [u8]) {
    for x in b.iter_mut() {
        // SAFETY: `x` is a valid, aligned, exclusive reference to one byte.
        unsafe { std::ptr::write_volatile(x, 0) };
    }
    std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
}

/// A password no longer needed: its bytes overwritten in memory, then emptied.
pub fn wipe(s: &mut String) {
    // SAFETY: only zeros are written, and a string of zero bytes is valid UTF-8.
    wipe_bytes(unsafe { s.as_bytes_mut() });
    s.clear();
}

fn nonce(prefix: &[u8; 8], n: u32) -> [u8; 12] {
    let mut v = [0u8; 12];
    v[..8].copy_from_slice(prefix);
    v[8..].copy_from_slice(&n.to_be_bytes());
    v
}

/// Reads as much as fits into `buf` (less only at the end of the file).
fn fill(r: &mut impl Read, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut n = 0;
    while n < buf.len() {
        match r.read(&mut buf[n..])? {
            0 => break,
            k => n += k,
        }
    }
    Ok(n)
}

fn encrypt_file(from: &Path, to: &Path, password: &str) -> Result<(), String> {
    let mut salt = [0u8; 16];
    let mut prefix = [0u8; 8];
    OsRng.fill_bytes(&mut salt);
    OsRng.fill_bytes(&mut prefix);
    let cipher = key(password, &salt, ROUNDS);
    let mut r = std::io::BufReader::new(std::fs::File::open(from).map_err(|e| e.to_string())?);
    let mut w = std::io::BufWriter::new(std::fs::File::create(to).map_err(|e| e.to_string())?);
    let io = |e: std::io::Error| e.to_string();
    w.write_all(MAGIC).map_err(io)?;
    w.write_all(&salt).map_err(io)?;
    w.write_all(&ROUNDS.to_le_bytes()).map_err(io)?;
    w.write_all(&prefix).map_err(io)?;
    let mut cur = vec![0u8; CHUNK];
    let mut next = vec![0u8; CHUNK];
    let mut len = fill(&mut r, &mut cur).map_err(io)?;
    let mut n = 0u32;
    loop {
        // the next chunk is read first: the current one is the last when nothing follows
        let next_len = if len == CHUNK { fill(&mut r, &mut next).map_err(io)? } else { 0 };
        let last = next_len == 0;
        let aad = [last as u8];
        let ct = cipher.encrypt(Nonce::from_slice(&nonce(&prefix, n)), Payload { msg: &cur[..len], aad: &aad }).map_err(|_| "encryption failed".to_string())?;
        let head = ct.len() as u32 | if last { LAST } else { 0 };
        w.write_all(&head.to_le_bytes()).map_err(io)?;
        w.write_all(&ct).map_err(io)?;
        if last {
            break;
        }
        std::mem::swap(&mut cur, &mut next);
        len = next_len;
        n = n.checked_add(1).ok_or("too large")?;
    }
    w.flush().map_err(io)
}

fn decrypt_file(from: &Path, to: &Path, password: &str) -> Result<(), String> {
    let io = |e: std::io::Error| e.to_string();
    let mut r = std::io::BufReader::new(std::fs::File::open(from).map_err(io)?);
    let mut head = [0u8; 32];
    r.read_exact(&mut head).map_err(|_| "Not a backup of HeMiMizer".to_string())?;
    if &head[..4] != MAGIC {
        return Err("Not a backup of HeMiMizer".into());
    }
    let salt = &head[4..20];
    let rounds = u32::from_le_bytes(head[20..24].try_into().unwrap_or_default());
    let prefix: [u8; 8] = head[24..32].try_into().unwrap_or_default();
    if !ROUNDS_OK.contains(&rounds) {
        return Err("The backup is damaged".into());
    }
    let cipher = key(password, salt, rounds);
    let mut w = std::io::BufWriter::new(std::fs::File::create(to).map_err(io)?);
    let mut n = 0u32;
    loop {
        let mut lb = [0u8; 4];
        r.read_exact(&mut lb).map_err(|_| "The backup is incomplete".to_string())?;
        let l = u32::from_le_bytes(lb);
        let last = l & LAST != 0;
        let size = (l & !LAST) as usize;
        if size > CHUNK + 16 {
            return Err("The backup is damaged".into());
        }
        let mut ct = vec![0u8; size];
        r.read_exact(&mut ct).map_err(|_| "The backup is incomplete".to_string())?;
        let aad = [last as u8];
        let pt = cipher.decrypt(Nonce::from_slice(&nonce(&prefix, n)), Payload { msg: &ct, aad: &aad }).map_err(|_| {
            if n == 0 { "Wrong password".to_string() } else { "The backup is damaged".to_string() }
        })?;
        w.write_all(&pt).map_err(io)?;
        if last {
            break;
        }
        n = n.checked_add(1).ok_or("The backup is damaged")?;
    }
    w.flush().map_err(io)
}

fn system_tar() -> String {
    format!(r"{}\System32\tar.exe", sys::env("SystemRoot", r"C:\Windows"))
}

/// Encrypts the personal parts of the backup into sealed.hmz and removes them in the open.
pub fn seal(password: &str, log: &dyn Fn(String)) -> Result<String, String> {
    let home = config::home();
    let parts: Vec<&str> = PARTS.iter().copied().filter(|p| home.join(p).exists()).collect();
    if parts.is_empty() {
        return Err("Nothing to encrypt".into());
    }
    log("Encrypting the backup".into());
    let tar = home.join("sealed.tar");
    let (t, h) = (tar.display().to_string(), home.display().to_string());
    let mut args = vec!["-c", "-f", t.as_str(), "-C", h.as_str()];
    args.extend(parts.iter().copied());
    let o = sys::exec(&system_tar(), &args)?;
    if o.code != 0 {
        let _ = std::fs::remove_file(&tar);
        return Err(format!("tar {}: {}", o.code, o.text.trim().lines().last().unwrap_or("")));
    }
    let sealed = sealed_file();
    let r = encrypt_file(&tar, &sealed, password);
    let _ = std::fs::remove_file(&tar);
    if let Err(e) = r {
        let _ = std::fs::remove_file(&sealed);
        return Err(e);
    }
    // a check: the sealed file opens with the password, before the open copy goes
    log("Checking the encrypted backup".into());
    let check = home.join("check.tar");
    let ok = decrypt_file(&sealed, &check, password);
    let _ = std::fs::remove_file(&check);
    if let Err(e) = ok {
        let _ = std::fs::remove_file(&sealed);
        return Err(format!("The check failed: {e}"));
    }
    for p in &parts {
        let _ = std::fs::remove_dir_all(home.join(p));
    }
    let size = std::fs::metadata(&sealed).map(|m| m.len()).unwrap_or(0);
    Ok(format!("{SEALED} ({:.1} MB) – only with the password", size as f64 / 1e6))
}

/// Opens a protected backup: its parts back into "HeMiMizer data" (sealed.hmz stays).
pub fn unseal(password: &str) -> Result<(), String> {
    let home = config::home();
    let tar = home.join("sealed.tar");
    let r = decrypt_file(&sealed_file(), &tar, password);
    if let Err(e) = r {
        let _ = std::fs::remove_file(&tar);
        return Err(e);
    }
    let (t, h) = (tar.display().to_string(), home.display().to_string());
    let o = sys::exec(&system_tar(), &["-x", "-f", &t, "-C", &h]);
    let _ = std::fs::remove_file(&tar);
    match o {
        Ok(o) if o.code == 0 => Ok(()),
        Ok(o) => Err(format!("tar {}: {}", o.code, o.text.trim().lines().last().unwrap_or(""))),
        Err(e) => Err(e),
    }
}

/// The open copy of a protected backup goes (at the end, on a USB stick: only the sealed file stays there).
pub fn remove_open_parts(home: &Path) {
    if home.join(SEALED).is_file() {
        for p in PARTS {
            let _ = std::fs::remove_dir_all(home.join(p));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let dir = std::env::temp_dir().join(format!("hmz-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for size in [0usize, 5, CHUNK, CHUNK + 7, 3 * CHUNK] {
            let data: Vec<u8> = (0..size).map(|i| (i * 31 % 251) as u8).collect();
            let (a, b, c) = (dir.join("a"), dir.join("b"), dir.join("c"));
            std::fs::write(&a, &data).unwrap();
            encrypt_file(&a, &b, "secret").unwrap();
            decrypt_file(&b, &c, "secret").unwrap();
            assert_eq!(std::fs::read(&c).unwrap(), data, "size {size}");
            assert_eq!(decrypt_file(&b, &c, "wrong").unwrap_err(), "Wrong password");
            // cut off: the last chunk is missing
            if size > CHUNK {
                let enc = std::fs::read(&b).unwrap();
                std::fs::write(&b, &enc[..32 + 4 + CHUNK + 16]).unwrap();
                assert!(decrypt_file(&b, &c, "secret").is_err());
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rounds_out_of_range() {
        let dir = std::env::temp_dir().join(format!("hmz-rounds-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (a, b, c) = (dir.join("a"), dir.join("b"), dir.join("c"));
        std::fs::write(&a, b"data").unwrap();
        encrypt_file(&a, &b, "secret").unwrap();
        let mut enc = std::fs::read(&b).unwrap();
        for bad in [u32::MAX, 1, 0] {
            enc[20..24].copy_from_slice(&bad.to_le_bytes());
            std::fs::write(&b, &enc).unwrap();
            assert_eq!(decrypt_file(&b, &c, "secret").unwrap_err(), "The backup is damaged");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn wipe_empties() {
        let mut s = String::from("secret");
        wipe(&mut s);
        assert!(s.is_empty());
    }
}
