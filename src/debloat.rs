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
    Ok("Hibernation off – the space hiberfil.sys took is free".into())
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
    c.log("Removing unneeded Microsoft apps");
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

/// Store apps put in by makers and Microsoft as ads (games, social networks, trials): exact package names, no
/// wildcards (audit 2026-11-24: "*Amazon*" also took the Kindle or Amazon Music someone installed). No antivirus.
const AD_APPS: &[&str] = &[
    "king.com.CandyCrushSaga",
    "king.com.CandyCrushSodaSaga",
    "king.com.CandyCrushFriends",
    "king.com.BubbleWitch3Saga",
    "king.com.FarmHeroesSaga",
    "Disney.37853FC22B2CE",
    "Facebook.Facebook",
    "Facebook.InstagramBeta",
    "BytedancePte.Ltd.TikTok",
    "4DF9E0F8.Netflix",
    "AmazonVideo.PrimeVideo",
    "Amazon.com.Amazon",
    "PricelinePartnerNetwork.Booking.comBigsavingsonhot",
    "7EE7776C.LinkedInforWindows",
    "SpotifyAB.SpotifyMusic",
    "AdobeSystemsIncorporated.AdobePhotoshopExpress",
    "C27EB4BA.DropboxOEM",
];

/// Only what the maker of the PC put into the image of Windows (a provisioned package) – with its copies of the users;
/// the same app installed by a user from the Store stays.
fn remove_provisioned_appx(names: &[&str]) -> Result<(), String> {
    let list = names.iter().map(|n| format!("'{n}'")).collect::<Vec<_>>().join(",");
    sys::ps(&format!(
        "$prov=Get-AppxProvisionedPackage -Online; foreach($n in @({list})) {{ \
         $p = $prov | Where-Object DisplayName -eq $n; if (-not $p) {{ continue }}; \
         try {{ $p | Remove-AppxProvisionedPackage -Online -ErrorAction Stop | Out-Null }} catch {{ \"$n (provisioned): $_\" }}; \
         try {{ Get-AppxPackage -AllUsers -Name $n | Remove-AppxPackage -AllUsers -ErrorAction Stop }} catch {{ \"$n (user): $_\" }} }}"
    ))
    .map(|_| ())
}

/// Programs makers install as trials or ads: by their exact winget id (`--id … --exact`, not a part of a name).
const AD_PROGRAMS: &[(&str, &str)] = &[("ExpressVPN", "ExpressVPN.ExpressVPN"), ("WildTangent Games", "gamigo.WildTangentGamesApp")];

/// Antivirus trials: never uninstalled by HeMiMizer (it may be one the user bought) – only named, to remove by hand.
const ANTIVIRUS: &[&str] = &["McAfee", "Norton", "Avast", "AVG", "Trend Micro", "Bullguard"];

pub fn ad_software(c: &Ctx) -> Res {
    use crate::i18n::tr;
    c.log("Removing trial and advertising software");
    remove_provisioned_appx(AD_APPS)?;
    let mut removed = Vec::new();
    if crate::install::winget_ready(c).is_ok() {
        let args = ["--accept-source-agreements", "--disable-interactivity"];
        for (name, id) in AD_PROGRAMS {
            let installed = sys::exec("winget.exe", &[&["list", "--id", id, "--exact"][..], &args].concat()).is_ok_and(|o| o.code == 0);
            if !installed {
                continue;
            }
            c.log(format!("Uninstalling {name}"));
            let o = sys::exec("winget.exe", &[&["uninstall", "--id", id, "--exact", "--silent", "--all-versions"][..], &args].concat());
            if o.is_ok_and(|o| o.code == 0) {
                removed.push(*name);
            }
        }
        // the antiviruses: only told (the names as winget lists them, case and all)
        let list = sys::exec("winget.exe", &[&["list"][..], &args].concat()).map(|o| o.text).unwrap_or_default();
        let found: Vec<&str> = ANTIVIRUS.iter().copied().filter(|n| list.lines().any(|l| l.contains(n))).collect();
        if !found.is_empty() {
            c.note(format!(
                "{} {}",
                tr(
                    "Antivirus programs are not removed by HeMiMizer – if you do not want them, uninstall them in Settings → Apps:",
                    "Antiviry HeMiMizer neodebírá – pokud je nechceš, odinstaluj je v Nastavení → Aplikace:"
                ),
                found.join(", ")
            ));
        }
    }
    Ok(if removed.is_empty() { "None of it was on this PC".into() } else { format!("Removed: {}", removed.join(", ")) })
}
