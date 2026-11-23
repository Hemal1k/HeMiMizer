//! The optional clean-up of a new Windows: a restore point first, hibernation off, the apps Microsoft and the maker of
//! the PC put in (each switch in the wizard). Only an own, checked list – nothing from scripts of others.

use crate::steps::{Ctx, Res};
use crate::sys;

/// Removes Store apps (for all users and from the image, so new users do not get them); each on its own, errors only
/// logged (a package half gone, as on Tiny11, throws "path not found").
pub fn remove_appx(names: &[&str]) -> Result<(), String> {
    let list = names.iter().map(|n| format!("'{n}'")).collect::<Vec<_>>().join(",");
    sys::ps(&format!(
        "$prov=Get-AppxProvisionedPackage -Online; foreach($n in @({list})) {{ \
         try {{ Get-AppxPackage -AllUsers -Name $n | Remove-AppxPackage -AllUsers -ErrorAction Stop }} catch {{ \"$n (user): $_\" }}; \
         try {{ $prov | Where-Object DisplayName -like $n | Remove-AppxProvisionedPackage -Online -ErrorAction Stop | Out-Null }} catch {{ \"$n (provisioned): $_\" }} }}"
    ))
    .map(|_| ())
}

/// A restore point before HeMiMizer changes anything (System Protection of C: is switched on for it).
pub fn restore_point(c: &Ctx) -> Res {
    c.log("Creating a restore point");
    // Windows makes at most one point a day unless this is 0
    sys::set_dword(sys::Root::Hklm, r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\SystemRestore", "SystemRestorePointCreationFrequency", 0)?;
    sys::ps("Enable-ComputerRestore -Drive \"$env:SystemDrive\\\"; Checkpoint-Computer -Description 'Before HeMiMizer' -RestorePointType MODIFY_SETTINGS")?;
    Ok("Restore point \"Before HeMiMizer\"".into())
}

pub fn hibernation_off(_c: &Ctx) -> Res {
    sys::run("powercfg.exe", &["/hibernate", "off"])?;
    Ok("Hibernation off – the space of hiberfil.sys is free".into())
}

/// The apps of Microsoft few people use (Media Player, Photos, the Store, Calculator… stay).
const MICROSOFT: &[&str] = &[
    "Clipchamp.Clipchamp",
    "Microsoft.BingNews",
    "Microsoft.BingWeather",
    "Microsoft.BingSearch",
    "Microsoft.MicrosoftSolitaireCollection",
    "MSTeams",
    "MicrosoftTeams",
    "Microsoft.OutlookForWindows",
    "Microsoft.Todos",
    "Microsoft.Getstarted",
    "Microsoft.GetHelp",
    "Microsoft.WindowsFeedbackHub",
    "Microsoft.WindowsMaps",
    "Microsoft.People",
    "Microsoft.MicrosoftOfficeHub",
    "Microsoft.PowerAutomateDesktop",
    "Microsoft.549981C3F5F10",
    "Microsoft.MixedReality.Portal",
    "Microsoft.SkypeApp",
    "Microsoft.YourPhone",
    "MicrosoftCorporationII.QuickAssist",
    "MicrosoftCorporationII.MicrosoftFamily",
];

pub fn microsoft_apps(c: &Ctx) -> Res {
    c.log("Removing apps of Microsoft that are not needed");
    // Microsoft Teams stays when it was chosen in the wizard (the same package)
    let keep: &[&str] = if crate::install::chosen("teams") { &["MSTeams", "MicrosoftTeams"] } else { &[] };
    let list: Vec<&str> = MICROSOFT.iter().copied().filter(|n| !keep.contains(n)).collect();
    remove_appx(&list)?;
    Ok(format!("{} apps checked and removed where present", MICROSOFT.len()))
}

const XBOX: &[&str] = &["Microsoft.XboxApp", "Microsoft.GamingApp", "Microsoft.XboxGamingOverlay", "Microsoft.XboxGameOverlay", "Microsoft.XboxIdentityProvider", "Microsoft.XboxSpeechToTextOverlay", "Microsoft.Xbox.TCUI"];

pub fn xbox_apps(c: &Ctx) -> Res {
    c.log("Removing the Xbox apps");
    remove_appx(XBOX)?;
    Ok("Xbox apps removed".into())
}

/// Store apps put in by makers and Microsoft as ads (games, social networks, trials).
const AD_APPS: &[&str] = &[
    "king.com.*",
    "*CandyCrush*",
    "Disney.*",
    "*Facebook*",
    "*Instagram*",
    "*TikTok*",
    "*Netflix*",
    "AmazonVideo.PrimeVideo",
    "*Amazon*",
    "*Booking*",
    "*LinkedIn*",
    "SpotifyAB.SpotifyMusic",
    "AdobeSystemsIncorporated.AdobePhotoshopExpress",
    "*McAfee*",
    "*Norton*",
    "*ExpressVPN*",
    "*WildTangent*",
    "*DropboxOEM*",
];

/// Programs installed by makers as trials or ads (found by name in the installed programs; uninstalled quietly).
const AD_PROGRAMS: &[&str] = &["McAfee", "Norton", "Avast", "AVG", "WildTangent", "ExpressVPN", "Booking.com", "Dropbox promotion", "Trend Micro", "Bullguard"];

pub fn ad_software(c: &Ctx) -> Res {
    c.log("Removing trial and advertising software");
    remove_appx(AD_APPS)?;
    // the programs: by name through winget (it knows how to uninstall them quietly)
    let mut removed = Vec::new();
    if crate::install::winget_ready(c).is_ok() {
        let list = sys::exec("winget.exe", &["list", "--accept-source-agreements", "--disable-interactivity"]).map(|o| o.text).unwrap_or_default();
        for name in AD_PROGRAMS {
            if !list.lines().any(|l| l.to_lowercase().contains(&name.to_lowercase())) {
                continue;
            }
            c.log(format!("Uninstalling {name}"));
            let o = sys::exec("winget.exe", &["uninstall", "--name", name, "--silent", "--all-versions", "--accept-source-agreements", "--disable-interactivity"]);
            if o.is_ok_and(|o| o.code == 0) {
                removed.push(*name);
            }
        }
    }
    Ok(if removed.is_empty() { "Nothing of it was on this PC".into() } else { format!("Removed: {}", removed.join(", ")) })
}
