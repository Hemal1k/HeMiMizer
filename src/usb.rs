//! "Make a Windows USB stick" (like Rufus, for Windows 10 / 11): a USB disk is wiped and made bootable from an ISO.
//!
//! - Partition scheme GPT (UEFI) or MBR (BIOS or UEFI); one FAT32 partition, which every UEFI starts from. FAT32 holds
//!   no file over 4 GB: a bigger install.wim is split by DISM into install.swm parts (Windows Setup reads them).
//!   Windows formats FAT32 only up to 32 GB, so a bigger stick gets a 32 GB partition.
//! - MBR: the partition is active and gets the boot code of Windows (bootsect from the ISO) for BIOS.
//! - The choices of the installation go into autounattend.xml at the root of the stick (as Rufus does): no TPM 2.0 /
//!   Secure Boot / RAM check, no Microsoft account required, a local account, the regional settings of this PC, no
//!   privacy questions, no automatic BitLocker encryption.
//!
//! Only disks on USB that are neither the boot nor the system disk are offered, and the disk is checked again right
//! before it is wiped.

use crate::sys;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};

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

#[derive(Clone, Debug, Default)]
pub struct Options {
    pub disk: u32,
    pub iso: PathBuf,
    /// GPT for UEFI; else MBR for BIOS or UEFI
    pub gpt: bool,
    pub label: String,
    pub bypass_hw: bool,
    pub bypass_msa: bool,
    /// a local account with this name (empty = none)
    pub local_user: String,
    pub regional: bool,
    pub no_privacy: bool,
    pub no_bitlocker: bool,
}

impl Options {
    fn unattended(&self) -> bool {
        self.bypass_hw || self.bypass_msa || !self.local_user.trim().is_empty() || self.regional || self.no_privacy || self.no_bitlocker
    }
}

/// A name for the stick that FAT32 takes: at most 11 letters, digits, spaces, - and _, in capitals.
pub fn fat_label(s: &str) -> String {
    let t: String = s.chars().filter(|c| c.is_ascii_alphanumeric() || *c == ' ' || *c == '-' || *c == '_').take(11).collect::<String>().to_uppercase();
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

/// Wipes the disk and makes it a bootable Windows stick; `log` tells what it does.
pub fn make(o: &Options, log: &dyn Fn(String)) -> Result<String, String> {
    if !o.iso.is_file() {
        return Err(format!("{}: the ISO is not there", o.iso.display()));
    }
    // the disk once more, right before it is wiped: still on USB, not the system
    if !disks().iter().any(|d| d.number == o.disk) {
        return Err("The USB disk is not there any more (or it is not a USB disk).".into());
    }
    log("Preparing the USB disk".into());
    let style = if o.gpt { "GPT" } else { "MBR" };
    let label = fat_label(&o.label);
    let letter = sys::ps(&format!(
        "$d = Get-Disk -Number {n}\n\
         if ($d.BusType -ne 'USB' -or $d.IsBoot -or $d.IsSystem) {{ throw 'Not a USB disk' }}\n\
         Set-Disk -Number {n} -IsReadOnly $false -ErrorAction SilentlyContinue\n\
         Get-Partition -DiskNumber {n} -ErrorAction SilentlyContinue | Where-Object DriveLetter | ForEach-Object {{ Remove-PartitionAccessPath -DiskNumber {n} -PartitionNumber $_.PartitionNumber -AccessPath \"$($_.DriveLetter):\\\" -ErrorAction SilentlyContinue }}\n\
         Clear-Disk -Number {n} -RemoveData -RemoveOEM -Confirm:$false -ErrorAction SilentlyContinue\n\
         $d = Get-Disk -Number {n}\n\
         if ($d.PartitionStyle -eq 'RAW') {{ Initialize-Disk -Number {n} -PartitionStyle {style} }} else {{ Set-Disk -Number {n} -PartitionStyle {style} }}\n\
         $size = [Math]::Min((Get-Disk -Number {n}).LargestFreeExtent, 32GB - 1MB)\n\
         $p = New-Partition -DiskNumber {n} -Size $size {active} -AssignDriveLetter\n\
         Format-Volume -Partition $p -FileSystem FAT32 -NewFileSystemLabel {label} -Confirm:$false -Force | Out-Null\n\
         (Get-Partition -DiskNumber {n} -PartitionNumber $p.PartitionNumber).DriveLetter",
        n = o.disk,
        active = if o.gpt { "" } else { "-IsActive" },
        label = ps_quote(&label),
    ))?;
    let letter = letter.trim().chars().last().filter(|c| c.is_ascii_alphabetic()).ok_or("The USB disk got no drive letter")?;
    let usb = PathBuf::from(format!("{letter}:\\"));
    sys::log(&format!("usb: {style} FAT32 {label} as {letter}:"));

    log("Opening the ISO".into());
    let iso = ps_quote(&o.iso.display().to_string());
    let src = sys::ps(&format!("$i = Mount-DiskImage -ImagePath {iso} -PassThru\n($i | Get-Volume).DriveLetter"))?;
    let src = src.trim().chars().last().filter(|c| c.is_ascii_alphabetic()).ok_or("The ISO could not be opened")?;
    let src = PathBuf::from(format!("{src}:\\"));
    let r = copy_windows(&src, &usb, o, log);
    let _ = sys::ps(&format!("Dismount-DiskImage -ImagePath {iso} | Out-Null"));
    r?;
    log("Done".into());
    Ok(format!("{letter}: {label} – {style}, FAT32{}", if o.unattended() { ", with your choices for the installation" } else { "" }))
}

fn copy_windows(src: &Path, usb: &Path, o: &Options, log: &dyn Fn(String)) -> Result<(), String> {
    if !src.join("sources").is_dir() || !src.join("setup.exe").is_file() {
        return Err("This ISO is not a Windows installation (no setup.exe, no sources).".into());
    }
    let wim = src.join(r"sources\install.wim");
    let big_wim = std::fs::metadata(&wim).is_ok_and(|m| m.len() >= 4 * 1024 * 1024 * 1024 - 1);
    let total: u64 = folder_size(src);
    // the files, all but a too big install.wim; the progress from what is on the stick
    let mut args: Vec<String> = vec![src.display().to_string(), usb.display().to_string(), "/E".into(), "/R:1".into(), "/W:1".into(), "/NFL".into(), "/NDL".into(), "/NJH".into(), "/NJS".into(), "/NP".into(), "/A-:R".into()];
    if big_wim {
        args.extend(["/XF".into(), wim.display().to_string()]);
    }
    let mut child = std::process::Command::new("robocopy.exe").args(&args).creation_flags(0x0800_0000).spawn().map_err(|e| format!("robocopy: {e}"))?;
    let code = loop {
        if let Some(s) = child.try_wait().map_err(|e| e.to_string())? {
            break s.code().unwrap_or(16);
        }
        log(format!("Copying the files – {:.1} of {:.1} GB", used(usb) as f64 / 1e9, total as f64 / 1e9));
        std::thread::sleep(std::time::Duration::from_secs(1));
    };
    if code >= 8 {
        return Err(format!("Copying failed (robocopy {code}) – is the stick big enough?"));
    }
    if big_wim {
        log("Splitting install.wim for FAT32 (a few minutes)".into());
        let o2 = sys::exec(
            "dism.exe",
            &["/Split-Image", &format!("/ImageFile:{}", wim.display()), &format!("/SWMFile:{}", usb.join(r"sources\install.swm").display()), "/FileSize:3800"],
        )?;
        if o2.code != 0 {
            return Err(format!("DISM {}: {}", o2.code, o2.text.trim().lines().last().unwrap_or("")));
        }
    }
    if !o.gpt {
        // BIOS: the boot code of Windows on the stick (the partition is active already)
        log("Making it start on BIOS".into());
        let bootsect = src.join(r"boot\bootsect.exe");
        if bootsect.is_file() {
            let drive = usb.display().to_string().trim_end_matches('\\').to_string();
            let b = sys::exec(&bootsect.display().to_string(), &["/nt60", &drive, "/mbr", "/force"])?;
            if b.code != 0 {
                return Err(format!("bootsect {}: {}", b.code, b.text.trim().lines().last().unwrap_or("")));
            }
        }
    }
    if o.unattended() {
        log("Writing your choices for the installation".into());
        let arch = if src.join(r"efi\boot\bootaa64.efi").is_file() { "arm64" } else { "amd64" };
        std::fs::write(usb.join("autounattend.xml"), autounattend(o, arch)).map_err(|e| format!("autounattend.xml: {e}"))?;
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

/// The language, keyboard and time zone of this PC: (input locale, system locale, user locale, time zone).
fn regional() -> (String, String, String, String) {
    let t = sys::ps(
        "$k = (Get-WinUserLanguageList)[0].InputMethodTips[0]\n\
         \"$k|$((Get-WinSystemLocale).Name)|$((Get-Culture).Name)|$((Get-TimeZone).Id)\"",
    )
    .unwrap_or_default();
    let p: Vec<String> = t.trim().split('|').map(String::from).collect();
    let get = |i: usize, d: &str| p.get(i).filter(|s| !s.is_empty()).cloned().unwrap_or_else(|| d.into());
    (get(0, "0409:00000409"), get(1, "en-US"), get(2, "en-US"), get(3, "UTC"))
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
    if o.regional {
        let (input, system, user_locale, tz) = regional();
        oobe_parts.push_str(&comp(
            "Microsoft-Windows-International-Core",
            &format!(
                "      <InputLocale>{}</InputLocale>\n      <SystemLocale>{}</SystemLocale>\n      <UserLocale>{}</UserLocale>\n",
                xml_escape(&input),
                xml_escape(&system),
                xml_escape(&user_locale)
            ),
        ));
        shell.push_str(&format!("      <TimeZone>{}</TimeZone>\n", xml_escape(&tz)));
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
        assert!(valid_user("user"));
        assert!(!valid_user("a/b"));
        assert!(!valid_user("..."));
    }

    #[test]
    fn unattend_without_regional() {
        let o = Options { bypass_hw: true, bypass_msa: true, local_user: "user".into(), no_privacy: true, no_bitlocker: true, ..Default::default() };
        let x = autounattend(&o, "amd64");
        assert!(x.contains("BypassTPMCheck") && x.contains("BypassNRO") && x.contains("PreventDeviceEncryption"));
        assert!(x.contains("<Name>user</Name>") && x.contains("<ProtectYourPC>3</ProtectYourPC>"));
        assert_eq!(x.matches("Microsoft-Windows-Shell-Setup").count(), 1);
    }
}
