//! "Make a Windows USB stick" (like Rufus, for Windows 10 / 11): a USB disk is wiped and made bootable from an ISO.
//!
//! - Partition scheme GPT (UEFI) or MBR (BIOS or UEFI).
//! - FAT32: one partition, which every UEFI starts from. FAT32 holds no file over 4 GB: a bigger install.wim is split
//!   by DISM into install.swm parts (Windows Setup reads them); any other file that big (an install.esd) needs NTFS.
//!   Windows formats FAT32 only up to 32 GB, so a bigger stick gets a 32 GB partition.
//! - NTFS: two partitions, as Microsoft describes it for big images – a small FAT32 "BOOT" with everything but the
//!   folder sources (and with sources\boot.wim), which UEFI and BIOS start from, and an NTFS one with all the files;
//!   Windows Setup finds install.wim there.
//! - MBR: the (boot) partition is active and gets the boot code of Windows (bootsect from the ISO) for BIOS.
//! - The ISO is opened without a drive letter (into a hidden folder), so no new drive pops up while it works.
//! - The choices of the installation go into autounattend.xml at the root of the stick (as Rufus does): no TPM 2.0 /
//!   Secure Boot / RAM check, no Microsoft account required, a local account, the region and time zone chosen, no
//!   privacy questions, no automatic BitLocker encryption.
//!
//! Only disks on USB that are neither the boot nor the system disk are offered, and the disk is checked again right
//! before it is wiped.

use crate::sys;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};

const FAT32_MAX: u64 = 4 * 1024 * 1024 * 1024 - 1;

#[derive(Clone, Debug, PartialEq)]
pub struct UsbDisk {
    pub number: u32,
    pub name: String,
    pub size: u64,
}

impl UsbDisk {
    pub fn label(&self) -> String {
        format!("{} ({:.1} GB)", self.name, self.size as f64 / 1e9)
    }
}

/// The USB disks that may be wiped: on USB, not the boot or the system disk.
pub fn disks() -> Vec<UsbDisk> {
    let Ok(t) = sys::ps(
        "Get-Disk | Where-Object { $_.BusType -eq 'USB' -and -not $_.IsBoot -and -not $_.IsSystem -and $_.Size -gt 0 } | \
         ForEach-Object { \"$($_.Number)|$($_.FriendlyName)|$($_.Size)\" }",
    ) else {
        return Vec::new();
    };
    t.lines()
        .filter_map(|l| {
            let p: Vec<&str> = l.trim().splitn(3, '|').collect();
            Some(UsbDisk { number: p.first()?.parse().ok()?, name: p.get(1)?.to_string(), size: p.get(2)?.parse().ok()? })
        })
        .collect()
}

/// The region of the new Windows: a culture (formats, language list) and a time zone; the keyboard is the culture's.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Region {
    pub culture: String,
    pub time_zone: String,
}

/// (name, English name) of every country / region Windows knows, e.g. ("cs-CZ", "Czech (Czechia)").
pub fn cultures() -> Vec<(String, String)> {
    pairs("[Globalization.CultureInfo]::GetCultures('SpecificCultures') | Sort-Object EnglishName | ForEach-Object { \"$($_.Name)|$($_.EnglishName)\" }")
}

/// (id, display name) of every time zone, e.g. ("Central Europe Standard Time", "(UTC+01:00) Belgrade, …").
pub fn time_zones() -> Vec<(String, String)> {
    pairs("[TimeZoneInfo]::GetSystemTimeZones() | ForEach-Object { \"$($_.Id)|$($_.DisplayName)\" }")
}

/// The region and time zone of this PC (the starting choice).
pub fn this_pc() -> Region {
    let t = sys::ps("\"$((Get-Culture).Name)|$((Get-TimeZone).Id)\"").unwrap_or_default();
    let (c, z) = t.trim().split_once('|').unwrap_or(("en-US", "UTC"));
    Region { culture: c.to_string(), time_zone: z.to_string() }
}

fn pairs(script: &str) -> Vec<(String, String)> {
    sys::ps(script).unwrap_or_default().lines().filter_map(|l| l.trim().split_once('|').map(|(a, b)| (a.to_string(), b.to_string()))).collect()
}

/// The keyboard of a culture as Windows sets it up, e.g. "0405:00000405".
fn keyboard(culture: &str) -> String {
    sys::ps(&format!("(New-WinUserLanguageList {})[0].InputMethodTips[0]", ps_quote(culture)))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| s.contains(':'))
        .unwrap_or_else(|| "0409:00000409".into())
}

/// The label of the ISO (ISO 9660 primary volume descriptor, sector 16) – read from the file, nothing is opened.
pub fn iso_label(iso: &Path) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(iso).ok()?;
    f.seek(SeekFrom::Start(0x8000)).ok()?;
    let mut pvd = [0u8; 72];
    f.read_exact(&mut pvd).ok()?;
    if &pvd[1..6] != b"CD001" {
        return None;
    }
    let label = String::from_utf8_lossy(&pvd[40..72]).trim().to_string();
    (!label.is_empty()).then_some(label)
}

#[derive(Clone, Debug, Default)]
pub struct Options {
    pub disk: u32,
    pub iso: PathBuf,
    /// GPT for UEFI; else MBR for BIOS or UEFI
    pub gpt: bool,
    /// NTFS (a FAT32 boot partition and an NTFS one); else one FAT32 partition
    pub ntfs: bool,
    pub label: String,
    pub bypass_hw: bool,
    pub bypass_msa: bool,
    /// a local account with this name (empty = none)
    pub local_user: String,
    pub region: Option<Region>,
    pub no_privacy: bool,
    pub no_bitlocker: bool,
}

impl Options {
    fn unattended(&self) -> bool {
        self.bypass_hw || self.bypass_msa || !self.local_user.trim().is_empty() || self.region.is_some() || self.no_privacy || self.no_bitlocker
    }
}

/// A name for a FAT32 partition: at most 11 letters, digits, spaces, - and _, in capitals.
pub fn fat_label(s: &str) -> String {
    let t: String = s.chars().filter(|c| c.is_ascii_alphanumeric() || *c == ' ' || *c == '-' || *c == '_').take(11).collect::<String>().to_uppercase();
    if t.trim().is_empty() { "WINDOWS".into() } else { t.trim().to_string() }
}

/// A name for an NTFS partition: at most 32 characters, none Windows refuses.
pub fn ntfs_label(s: &str) -> String {
    let t: String = s.chars().filter(|c| !"\"*/:<>?\\|".contains(*c) && !c.is_control()).take(32).collect();
    if t.trim().is_empty() { "WINDOWS".into() } else { t.trim().to_string() }
}

/// A user name Windows takes for a local account (1–20 characters, none of "/\[]:;|=,+*?<>@ and not only dots or
/// spaces).
pub fn valid_user(s: &str) -> bool {
    let t = s.trim();
    t.chars().count() <= 20 && !t.chars().any(|c| "\"/\\[]:;|=,+*?<>@%".contains(c)) && !t.chars().all(|c| c == '.' || c == ' ')
}

fn ps_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// The hidden folder the ISO is opened into.
fn mount_dir() -> PathBuf {
    std::env::temp_dir().join("HeMiMizer").join("iso")
}

/// Opens the ISO without a drive letter, into a hidden folder (no new drive in File Explorer).
pub fn open_iso(iso: &Path) -> Result<PathBuf, String> {
    let dir = mount_dir();
    close_iso(iso);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let vol = sys::ps(&format!("(Mount-DiskImage -ImagePath {} -NoDriveLetter -PassThru | Get-Volume).Path", ps_quote(&iso.display().to_string())))?;
    let vol = vol.trim();
    if !vol.starts_with(r"\\?\Volume") {
        close_iso(iso);
        return Err("The ISO could not be opened".into());
    }
    let o = sys::exec("mountvol.exe", &[&dir.display().to_string(), vol])?;
    if o.code != 0 {
        close_iso(iso);
        return Err(format!("The ISO could not be opened (mountvol {}: {})", o.code, o.text.trim()));
    }
    Ok(dir)
}

pub fn close_iso(iso: &Path) {
    let dir = mount_dir();
    if dir.exists() {
        let _ = sys::exec("mountvol.exe", &[&dir.display().to_string(), "/D"]);
        let _ = std::fs::remove_dir(&dir);
    }
    let _ = sys::ps(&format!("Dismount-DiskImage -ImagePath {} -ErrorAction SilentlyContinue | Out-Null", ps_quote(&iso.display().to_string())));
}

/// Wipes the disk and makes it a bootable Windows stick; `log` tells what it does.
pub fn make(o: &Options, log: &dyn Fn(String)) -> Result<String, String> {
    if !o.iso.is_file() {
        return Err(format!("{}: the ISO is not there", o.iso.display()));
    }
    // the disk once more, right before it is wiped: still on USB, not the system
    if !disks().iter().any(|d| d.number == o.disk) {
        return Err("The USB disk is not there any more (or it is not a USB disk).".into());
    }
    log("Opening the ISO".into());
    let src = open_iso(&o.iso)?;
    let r = make_from(o, &src, log);
    close_iso(&o.iso);
    r
}

fn make_from(o: &Options, src: &Path, log: &dyn Fn(String)) -> Result<String, String> {
    if !src.join("sources").is_dir() || !src.join("setup.exe").is_file() {
        return Err("This ISO is not a Windows installation (no setup.exe, no sources).".into());
    }
    // FAT32: every file over 4 GB but install.wim (split by DISM) makes it impossible – said before the disk is wiped
    if !o.ntfs {
        if let Some(big) = big_files(src).into_iter().find(|p| !p.file_name().is_some_and(|n| n.eq_ignore_ascii_case("install.wim"))) {
            return Err(format!(
                "{} is bigger than 4 GB and FAT32 cannot hold it – choose NTFS.",
                big.strip_prefix(src).unwrap_or(&big).display()
            ));
        }
    }
    log("Preparing the USB disk".into());
    let style = if o.gpt { "GPT" } else { "MBR" };
    let active = if o.gpt { "" } else { "-IsActive" };
    let prepare = format!(
        "$d = Get-Disk -Number {n}\n\
         if ($d.BusType -ne 'USB' -or $d.IsBoot -or $d.IsSystem) {{ throw 'Not a USB disk' }}\n\
         Set-Disk -Number {n} -IsReadOnly $false -ErrorAction SilentlyContinue\n\
         Get-Partition -DiskNumber {n} -ErrorAction SilentlyContinue | Where-Object DriveLetter | ForEach-Object {{ Remove-PartitionAccessPath -DiskNumber {n} -PartitionNumber $_.PartitionNumber -AccessPath \"$($_.DriveLetter):\\\" -ErrorAction SilentlyContinue }}\n\
         Clear-Disk -Number {n} -RemoveData -RemoveOEM -Confirm:$false -ErrorAction SilentlyContinue\n\
         $d = Get-Disk -Number {n}\n\
         if ($d.PartitionStyle -eq 'RAW') {{ Initialize-Disk -Number {n} -PartitionStyle {style} }} else {{ Set-Disk -Number {n} -PartitionStyle {style} }}\n",
        n = o.disk
    );
    let (boot, data, label) = if o.ntfs {
        let label = ntfs_label(&o.label);
        let t = sys::ps(&format!(
            "{prepare}\
             $b = New-Partition -DiskNumber {n} -Size 2GB {active} -AssignDriveLetter\n\
             Format-Volume -Partition $b -FileSystem FAT32 -NewFileSystemLabel 'BOOT' -Confirm:$false -Force | Out-Null\n\
             $p = New-Partition -DiskNumber {n} -UseMaximumSize -AssignDriveLetter\n\
             Format-Volume -Partition $p -FileSystem NTFS -NewFileSystemLabel {lab} -Confirm:$false -Force | Out-Null\n\
             \"$((Get-Partition -DiskNumber {n} -PartitionNumber $b.PartitionNumber).DriveLetter)|$((Get-Partition -DiskNumber {n} -PartitionNumber $p.PartitionNumber).DriveLetter)\"",
            n = o.disk,
            lab = ps_quote(&label)
        ))?;
        let (b, d) = t.trim().lines().last().and_then(|l| l.split_once('|')).ok_or("The USB disk got no drive letters")?;
        (drive(b)?, Some(drive(d)?), label)
    } else {
        let label = fat_label(&o.label);
        let t = sys::ps(&format!(
            "{prepare}\
             $size = [Math]::Min((Get-Disk -Number {n}).LargestFreeExtent, 32GB - 1MB)\n\
             $p = New-Partition -DiskNumber {n} -Size $size {active} -AssignDriveLetter\n\
             Format-Volume -Partition $p -FileSystem FAT32 -NewFileSystemLabel {lab} -Confirm:$false -Force | Out-Null\n\
             (Get-Partition -DiskNumber {n} -PartitionNumber $p.PartitionNumber).DriveLetter",
            n = o.disk,
            lab = ps_quote(&label)
        ))?;
        (drive(t.trim().lines().last().unwrap_or(""))?, None, label)
    };
    sys::log(&format!("usb: {style} {} {label} boot {} data {:?}", if o.ntfs { "NTFS" } else { "FAT32" }, boot.display(), data));

    let total = folder_size(src);
    match &data {
        // NTFS: all on the big partition; the start files (no sources but boot.wim) on BOOT
        Some(d) => {
            copy(src, d, &[], &[], total, "Copying the files", log)?;
            log("Copying the start files".into());
            copy(src, &boot, &["sources"], &[], 0, "", log)?;
            std::fs::create_dir_all(boot.join("sources")).map_err(|e| e.to_string())?;
            std::fs::copy(src.join(r"sources\boot.wim"), boot.join(r"sources\boot.wim")).map_err(|e| format!("boot.wim: {e}"))?;
        }
        None => {
            let wim = src.join(r"sources\install.wim");
            let big_wim = std::fs::metadata(&wim).is_ok_and(|m| m.len() > FAT32_MAX);
            let skip: Vec<PathBuf> = if big_wim { vec![wim.clone()] } else { Vec::new() };
            copy(src, &boot, &[], &skip, total, "Copying the files", log)?;
            if big_wim {
                log("Splitting install.wim for FAT32 (a few minutes)".into());
                let o2 = sys::exec(
                    "dism.exe",
                    &["/Split-Image", &format!("/ImageFile:{}", wim.display()), &format!("/SWMFile:{}", boot.join(r"sources\install.swm").display()), "/FileSize:3800"],
                )?;
                if o2.code != 0 {
                    return Err(format!("DISM {}: {}", o2.code, o2.text.trim().lines().last().unwrap_or("")));
                }
            }
        }
    }
    if !o.gpt {
        // BIOS: the boot code of Windows on the (boot) partition, which is active already
        log("Making it start on BIOS".into());
        let bootsect = src.join(r"boot\bootsect.exe");
        if bootsect.is_file() {
            let d = boot.display().to_string().trim_end_matches('\\').to_string();
            let b = sys::exec(&bootsect.display().to_string(), &["/nt60", &d, "/mbr", "/force"])?;
            if b.code != 0 {
                return Err(format!("bootsect {}: {}", b.code, b.text.trim().lines().last().unwrap_or("")));
            }
        }
    }
    if o.unattended() {
        log("Writing your choices for the installation".into());
        let arch = if src.join(r"efi\boot\bootaa64.efi").is_file() { "arm64" } else { "amd64" };
        let xml = autounattend(o, arch);
        for root in std::iter::once(&boot).chain(data.as_ref()) {
            std::fs::write(root.join("autounattend.xml"), &xml).map_err(|e| format!("autounattend.xml: {e}"))?;
        }
    }
    log("Done".into());
    let d = data.as_ref().unwrap_or(&boot).display().to_string();
    Ok(format!(
        "{} {label} – {style}, {}{}",
        d.trim_end_matches('\\'),
        if o.ntfs { "NTFS" } else { "FAT32" },
        if o.unattended() { ", with your choices for the installation" } else { "" }
    ))
}

fn drive(letter: &str) -> Result<PathBuf, String> {
    let l = letter.trim().chars().last().filter(|c| c.is_ascii_alphabetic()).ok_or("The USB disk got no drive letter")?;
    Ok(PathBuf::from(format!("{l}:\\")))
}

/// Files of the ISO over 4 GB.
fn big_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir) else { return out };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(big_files(&p));
        } else if e.metadata().is_ok_and(|m| m.len() > FAT32_MAX) {
            out.push(p);
        }
    }
    out
}

/// robocopy of `from` into `to` without the folders `skip_dirs` and the files `skip_files`; with `total`, the progress
/// from what is on the drive.
fn copy(from: &Path, to: &Path, skip_dirs: &[&str], skip_files: &[PathBuf], total: u64, what: &str, log: &dyn Fn(String)) -> Result<(), String> {
    let mut args: Vec<String> = vec![from.display().to_string(), to.display().to_string(), "/E".into(), "/R:1".into(), "/W:1".into(), "/NFL".into(), "/NDL".into(), "/NJH".into(), "/NJS".into(), "/NP".into(), "/A-:R".into()];
    if !skip_dirs.is_empty() {
        args.push("/XD".into());
        args.extend(skip_dirs.iter().map(|d| from.join(d).display().to_string()));
    }
    if !skip_files.is_empty() {
        args.push("/XF".into());
        args.extend(skip_files.iter().map(|f| f.display().to_string()));
    }
    let before = used(to);
    let mut child = std::process::Command::new(crate::sys::program("robocopy.exe")).args(&args).creation_flags(0x0800_0000).spawn().map_err(|e| format!("robocopy: {e}"))?;
    let code = loop {
        if let Some(s) = child.try_wait().map_err(|e| e.to_string())? {
            break s.code().unwrap_or(16);
        }
        if total > 0 {
            log(format!("{what} – {:.1} of {:.1} GB", used(to).saturating_sub(before) as f64 / 1e9, total as f64 / 1e9));
        }
        std::thread::sleep(std::time::Duration::from_secs(1));
    };
    if code >= 8 {
        return Err(format!("Copying failed (robocopy {code}) – is the stick big enough?"));
    }
    Ok(())
}

fn folder_size(p: &Path) -> u64 {
    let Ok(rd) = std::fs::read_dir(p) else { return 0 };
    rd.flatten().map(|e| if e.path().is_dir() { folder_size(&e.path()) } else { e.metadata().map(|m| m.len()).unwrap_or(0) }).sum()
}

/// What is on the drive now (its size less the free space).
fn used(root: &Path) -> u64 {
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let w = sys::wide(&root.display().to_string());
    let (mut free, mut total) = (0u64, 0u64);
    unsafe { GetDiskFreeSpaceExW(w.as_ptr(), std::ptr::null_mut(), &mut total, &mut free) };
    total.saturating_sub(free)
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// autounattend.xml with the choices: everything else of Setup stays as it is (the user still chooses the edition,
/// the disk and the rest).
fn autounattend(o: &Options, arch: &str) -> String {
    let comp = |name: &str, body: &str| {
        format!(
            "    <component name=\"{name}\" processorArchitecture=\"{arch}\" publicKeyToken=\"31bf3856ad364e35\" language=\"neutral\" versionScope=\"nonSxS\" xmlns:wcm=\"http://schemas.microsoft.com/WMIConfig/2002/State\">\n{body}    </component>\n"
        )
    };
    let runs = |cmds: &[String]| -> String {
        let mut s = String::from("      <RunSynchronous>\n");
        for (i, c) in cmds.iter().enumerate() {
            s.push_str(&format!(
                "        <RunSynchronousCommand wcm:action=\"add\">\n          <Order>{}</Order>\n          <Path>{}</Path>\n        </RunSynchronousCommand>\n",
                i + 1,
                xml_escape(c)
            ));
        }
        s.push_str("      </RunSynchronous>\n");
        s
    };
    let mut xml = String::from("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<unattend xmlns=\"urn:schemas-microsoft-com:unattend\">\n");
    if o.bypass_hw {
        let cmds: Vec<String> = ["BypassTPMCheck", "BypassSecureBootCheck", "BypassRAMCheck"]
            .iter()
            .map(|v| format!(r"reg add HKLM\SYSTEM\Setup\LabConfig /v {v} /t REG_DWORD /d 1 /f"))
            .collect();
        xml.push_str(&format!("  <settings pass=\"windowsPE\">\n{}  </settings>\n", comp("Microsoft-Windows-Setup", &runs(&cmds))));
    }
    let mut special = Vec::new();
    if o.bypass_msa {
        special.push(r"reg add HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\OOBE /v BypassNRO /t REG_DWORD /d 1 /f".to_string());
    }
    if o.no_bitlocker {
        special.push(r"reg add HKLM\SYSTEM\CurrentControlSet\Control\BitLocker /v PreventDeviceEncryption /t REG_DWORD /d 1 /f".to_string());
    }
    if !special.is_empty() {
        xml.push_str(&format!("  <settings pass=\"specialize\">\n{}  </settings>\n", comp("Microsoft-Windows-Deployment", &runs(&special))));
    }
    let user = o.local_user.trim();
    let mut oobe_parts = String::new();
    // Shell-Setup once: the time zone, then the OOBE part
    let mut shell = String::new();
    if let Some(r) = &o.region {
        let kb = keyboard(&r.culture);
        oobe_parts.push_str(&comp(
            "Microsoft-Windows-International-Core",
            &format!(
                "      <InputLocale>{}</InputLocale>\n      <SystemLocale>{c}</SystemLocale>\n      <UserLocale>{c}</UserLocale>\n",
                xml_escape(&kb),
                c = xml_escape(&r.culture)
            ),
        ));
        shell.push_str(&format!("      <TimeZone>{}</TimeZone>\n", xml_escape(&r.time_zone)));
    }
    if o.no_privacy || !user.is_empty() {
        let mut body = String::from("      <OOBE>\n");
        if o.no_privacy {
            body.push_str("        <ProtectYourPC>3</ProtectYourPC>\n");
        }
        if !user.is_empty() {
            body.push_str("        <HideOnlineAccountScreens>true</HideOnlineAccountScreens>\n        <HideLocalAccountScreen>true</HideLocalAccountScreen>\n");
        }
        body.push_str("      </OOBE>\n");
        if !user.is_empty() {
            // no password: Windows asks for one later, or the user sets it in Settings
            body.push_str(&format!(
                "      <UserAccounts>\n        <LocalAccounts>\n          <LocalAccount wcm:action=\"add\">\n            <Name>{u}</Name>\n            <DisplayName>{u}</DisplayName>\n            <Group>Administrators</Group>\n            <Password>\n              <Value></Value>\n              <PlainText>true</PlainText>\n            </Password>\n          </LocalAccount>\n        </LocalAccounts>\n      </UserAccounts>\n",
                u = xml_escape(user)
            ));
        }
        shell.push_str(&body);
    }
    if !shell.is_empty() {
        oobe_parts.push_str(&comp("Microsoft-Windows-Shell-Setup", &shell));
    }
    if !oobe_parts.is_empty() {
        xml.push_str(&format!("  <settings pass=\"oobeSystem\">\n{oobe_parts}  </settings>\n"));
    }
    xml.push_str("</unattend>\n");
    xml
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_and_users() {
        assert_eq!(fat_label("Win 11 Pro 26H2!"), "WIN 11 PRO");
        assert_eq!(fat_label("€€"), "WINDOWS");
        assert_eq!(ntfs_label("CCCOMA_X64FRE_CS-CZ_DV9"), "CCCOMA_X64FRE_CS-CZ_DV9");
        assert!(valid_user("user"));
        assert!(!valid_user("a/b"));
        assert!(!valid_user("..."));
    }

    #[test]
    fn unattend_without_region() {
        let o = Options { bypass_hw: true, bypass_msa: true, local_user: "user".into(), no_privacy: true, no_bitlocker: true, ..Default::default() };
        let x = autounattend(&o, "amd64");
        assert!(x.contains("BypassTPMCheck") && x.contains("BypassNRO") && x.contains("PreventDeviceEncryption"));
        assert!(x.contains("<Name>user</Name>") && x.contains("<ProtectYourPC>3</ProtectYourPC>"));
        assert_eq!(x.matches("Microsoft-Windows-Shell-Setup").count(), 1);
    }
}
