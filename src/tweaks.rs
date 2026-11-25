//! The single settings of Windows the wizard offers one by one (the page "Windows"): each can be switched on or off,
//! has a recommended default, and – where Windows keeps it in the registry – can be read from this PC, so the new
//! Windows is set "like this PC". The step "tweaks" applies the ones that are on in the plan.

use crate::steps::{Ctx, Res};
use crate::sys::{self, Root};

const ADV: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced";
const EXPLORER: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer";
const CDM: &str = r"Software\Microsoft\Windows\CurrentVersion\ContentDeliveryManager";
const AI: &str = r"SOFTWARE\Policies\Microsoft\Windows\WindowsAI";
const START: &str = r"Software\Microsoft\Windows\CurrentVersion\Start";
const PERSONALIZE: &str = r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize";
const HIGH_PERFORMANCE: &str = "8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c";
/// Home and Gallery in the navigation pane of File Explorer.
const HOME_GALLERY: [&str; 2] = ["{f874310e-b6b7-47dc-bc84-b9e6b38f5903}", "{e88865ea-0e1c-4e20-9aa6-edcd0212c87c}"];
/// The menu of Windows 10 on a right-click: this key (empty) switches the new one off.
const CLASSIC_MENU: &str = r"Software\Classes\CLSID\{86ca1aa0-34aa-4e8b-a509-50c905bae2a2}\InprocServer32";

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Group {
    Privacy,
    Taskbar,
    Explorer,
    System,
}

pub struct Tweak {
    pub id: &'static str,
    pub group: Group,
    pub title: &'static str,
    pub detail: &'static str,
    /// on in the recommended choice
    pub recommended: bool,
    pub apply: fn(&Ctx) -> Result<(), String>,
    /// Is it so on this PC now? None = Windows does not tell (then "like this PC" keeps the recommended choice).
    pub now: fn() -> Option<bool>,
}

const fn t(id: &'static str, group: Group, title: &'static str, detail: &'static str, recommended: bool, apply: fn(&Ctx) -> Result<(), String>, now: fn() -> Option<bool>) -> Tweak {
    Tweak { id, group, title, detail, recommended, apply, now }
}

fn dw(root: Root, key: &str, name: &str) -> Option<u32> {
    sys::get_dword(root, key, name)
}

/// A value that is set only when someone changed it: missing = the default of Windows.
fn is(root: Root, key: &str, name: &str, value: u32) -> Option<bool> {
    Some(dw(root, key, name) == Some(value))
}

use Group::*;

pub const ALL: &[Tweak] = &[
    // ---------------------------------------------------------------- privacy and AI
    t("copilot", Privacy, "Copilot off", "No Copilot app, button and key", true, copilot, || {
        Some(dw(Root::Hklm, r"SOFTWARE\Policies\Microsoft\Windows\WindowsCopilot", "TurnOffWindowsCopilot") == Some(1)
            || dw(Root::Hkcu, r"Software\Policies\Microsoft\Windows\WindowsCopilot", "TurnOffWindowsCopilot") == Some(1))
    }),
    t("recall", Privacy, "Recall and Click to Do off", "Windows takes no snapshots of the screen", true, recall, || {
        Some(dw(Root::Hklm, AI, "AllowRecallEnablement") == Some(0) || dw(Root::Hklm, AI, "DisableAIDataAnalysis") == Some(1))
    }),
    t("ai-apps", Privacy, "No AI in Paint, Notepad and Settings", "Image Creator, generative fill, rewriting in Notepad, the agent in Settings", true, ai_apps, || {
        is(Root::Hklm, r"SOFTWARE\Policies\WindowsNotepad", "DisableAIFeatures", 1)
    }),
    t("widgets", Privacy, "Widgets off", "No news and weather board on the taskbar", true, widgets, || {
        Some(dw(Root::Hklm, r"SOFTWARE\Policies\Microsoft\Dsh", "AllowNewsAndInterests") == Some(0) || dw(Root::Hkcu, ADV, "TaskbarDa") == Some(0))
    }),
    t("web-search", Privacy, "No web results in Search", "Search finds your apps and files – no Bing, no news of the day", true, web_search, || {
        Some(
            dw(Root::Hkcu, r"Software\Policies\Microsoft\Windows\Explorer", "DisableSearchBoxSuggestions") == Some(1)
                || dw(Root::Hkcu, r"Software\Microsoft\Windows\CurrentVersion\Search", "BingSearchEnabled") == Some(0),
        )
    }),
    t("ads", Privacy, "No ads, tips and suggestions", "In Start, Settings, the lock screen and File Explorer; no \"finish setting up\"", true, ads, || {
        Some(dw(Root::Hkcu, CDM, "SystemPaneSuggestionsEnabled") == Some(0) || dw(Root::Hkcu, CDM, "SubscribedContent-338389Enabled") == Some(0))
    }),
    t("telemetry", Privacy, "Least diagnostic data", "Only the required data goes to Microsoft; no feedback requests", true, telemetry, || {
        Some(
            dw(Root::Hklm, r"SOFTWARE\Policies\Microsoft\Windows\DataCollection", "AllowTelemetry") == Some(0)
                || dw(Root::Hklm, r"SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\DataCollection", "AllowTelemetry").is_some_and(|v| v <= 1),
        )
    }),
    t("activity", Privacy, "No activity history", "Windows does not keep a list of what you did", true, activity, || {
        is(Root::Hklm, r"SOFTWARE\Policies\Microsoft\Windows\System", "PublishUserActivities", 0)
    }),
    t("edge", Privacy, "Edge quieter", "No sidebar, shopping offers and running in the background", true, edge, || {
        is(Root::Hklm, r"SOFTWARE\Policies\Microsoft\Edge", "HubsSidebarEnabled", 0)
    }),
    t("security-nags", Privacy, "No nagging in Windows Security", "Dismisses \"sign in with a Microsoft account\" and \"memory integrity is off\"", true, security_nags, || {
        is(Root::Hkcu, r"Software\Microsoft\Windows Security Health\State", "AccountProtection_MicrosoftAccount_Disconnected", 1)
    }),
    // ---------------------------------------------------------------- taskbar and Start
    t("tb-left", Taskbar, "Taskbar on the left", "Start and the apps at the left edge, as in Windows 10", false, |_| sys::set_dword(Root::Hkcu, ADV, "TaskbarAl", 0), || {
        is(Root::Hkcu, ADV, "TaskbarAl", 0)
    }),
    t("tb-search", Taskbar, "No search box on the taskbar", "Search opens from Start or with Win+S", false, |_| sys::set_dword(Root::Hkcu, r"Software\Microsoft\Windows\CurrentVersion\Search", "SearchboxTaskbarMode", 0), || {
        is(Root::Hkcu, r"Software\Microsoft\Windows\CurrentVersion\Search", "SearchboxTaskbarMode", 0)
    }),
    t("tb-taskview", Taskbar, "No Task view button", "Win+Tab still shows it", false, |_| sys::set_dword(Root::Hkcu, ADV, "ShowTaskViewButton", 0), || {
        is(Root::Hkcu, ADV, "ShowTaskViewButton", 0)
    }),
    t("tb-extras", Taskbar, "No emoji, pen and Resume", "Fewer icons next to the clock; no \"resume on this PC\"", false, taskbar_extras, || {
        is(Root::Hkcu, r"Software\Microsoft\Windows\CurrentVersion\CrossDeviceResume\Configuration", "IsResumeAllowed", 0)
    }),
    t("start-recommended", Taskbar, "No Recommended in Start", "No recent files and suggested apps", false, start_recommended, || {
        is(Root::Hkcu, ADV, "Start_TrackDocs", 0)
    }),
    t("start-apps", Taskbar, "No recent and most used apps in Start", "Only your pins and the list of all apps", false, start_apps, || {
        Some(dw(Root::Hkcu, ADV, "Start_TrackProgs") == Some(0) || dw(Root::Hkcu, START, "ShowRecentList") == Some(0))
    }),
    t("start-account", Taskbar, "No account notifications in Start", "No reminders to back up or to sign in", false, |_| sys::set_dword(Root::Hkcu, ADV, "Start_AccountNotifications", 0), || {
        is(Root::Hkcu, ADV, "Start_AccountNotifications", 0)
    }),
    t("start-pins", Taskbar, "More pins in Start", "One more row for your pins, less for Recommended", false, |_| sys::set_dword(Root::Hkcu, ADV, "Start_Layout", 1), || {
        is(Root::Hkcu, ADV, "Start_Layout", 1)
    }),
    // ---------------------------------------------------------------- File Explorer
    t("fe-thispc", Explorer, "File Explorer opens This PC", "Your drives and folders instead of Home", true, |_| sys::set_dword(Root::Hkcu, ADV, "LaunchTo", 1), || {
        is(Root::Hkcu, ADV, "LaunchTo", 1)
    }),
    t("fe-ext", Explorer, "Show file extensions", "\"photo.jpg\" instead of \"photo\" – safer, a fake document is seen", true, |_| sys::set_dword(Root::Hkcu, ADV, "HideFileExt", 0), || {
        is(Root::Hkcu, ADV, "HideFileExt", 0)
    }),
    t("fe-hidden", Explorer, "Show hidden files", "Also AppData and other hidden folders", false, |_| sys::set_dword(Root::Hkcu, ADV, "Hidden", 1), || {
        is(Root::Hkcu, ADV, "Hidden", 1)
    }),
    t("fe-recent", Explorer, "No recent files and folders", "File Explorer does not list what you opened lately", true, recent_off, || {
        is(Root::Hkcu, EXPLORER, "ShowRecent", 0)
    }),
    t("fe-home", Explorer, "No Home and Gallery", "In the navigation pane on the left", false, home_gallery_off, || {
        is(Root::Hkcu, &format!(r"Software\Classes\CLSID\{}", HOME_GALLERY[0]), "System.IsPinnedToNameSpaceTree", 0)
    }),
    t("fe-menu", Explorer, "The full right-click menu", "All items at once, without \"Show more options\"", false, |_| sys::set_sz(Root::Hkcu, CLASSIC_MENU, "", ""), || {
        Some(sys::key_exists(Root::Hkcu, CLASSIC_MENU))
    }),
    t("fe-recycle", Explorer, "No Recycle Bin", "Deleted files are gone at once – careful", false, recycle_off, || {
        is(Root::Hkcu, r"Software\Microsoft\Windows\CurrentVersion\Policies\Explorer", "NoRecycleFiles", 1)
    }),
    // ---------------------------------------------------------------- system and games
    t("dark", System, "Dark mode", "For Windows and the apps", false, dark, || {
        Some(dw(Root::Hkcu, PERSONALIZE, "AppsUseLightTheme") == Some(0) && dw(Root::Hkcu, PERSONALIZE, "SystemUsesLightTheme") == Some(0))
    }),
    t("power", System, "Full performance", "The High performance plan; the PC never sleeps, the display goes off after 10 min, the power button shuts down", false, power, || {
        Some(sys::run("powercfg.exe", &["/getactivescheme"]).ok()?.to_lowercase().contains(HIGH_PERFORMANCE))
    }),
    t("games", System, "Game Mode and GPU scheduling", "Plus optimizations for windowed games", false, games, || {
        is(Root::Hklm, r"SYSTEM\CurrentControlSet\Control\GraphicsDrivers", "HwSchMode", 2)
    }),
    t("game-dvr", System, "No background recording of games", "Game Bar does not record and does not open from the controller", false, game_dvr, || {
        Some(dw(Root::Hkcu, r"Software\Microsoft\Windows\CurrentVersion\GameDVR", "AppCaptureEnabled") == Some(0) || dw(Root::Hkcu, r"System\GameConfigStore", "GameDVR_Enabled") == Some(0))
    }),
    t("mouse", System, "Enhance pointer precision off", "The pointer moves exactly with the mouse – better for games", false, mouse, || {
        Some(sys::get_sz(Root::Hkcu, r"Control Panel\Mouse", "MouseSpeed").as_deref() == Some("0"))
    }),
    t("sound", System, "Quiet start and calls", "No sound when Windows starts; other sounds are not turned down during calls", false, sound, || {
        is(Root::Hklm, r"SOFTWARE\Microsoft\Windows\CurrentVersion\Authentication\LogonUI\BootAnimation", "DisableStartupSound", 1)
    }),
    t("restart-apps", System, "Apps do not reopen after a restart", "Also no automatic sign-in to finish updates", false, restart_apps, || {
        is(Root::Hkcu, r"Software\Microsoft\Windows NT\CurrentVersion\Winlogon", "RestartApps", 0)
    }),
    t("signin-email", System, "Hide the e-mail on the sign-in screen", "Only your name is shown", false, |_| sys::set_dword(Root::Hklm, r"SOFTWARE\Policies\Microsoft\Windows\System", "BlockUserFromShowingAccountDetailsOnSignin", 1), || {
        is(Root::Hklm, r"SOFTWARE\Policies\Microsoft\Windows\System", "BlockUserFromShowingAccountDetailsOnSignin", 1)
    }),
    t("vlc-lean", System, "VLC without extras", "No web plug-ins, no disc AutoPlay, no VLC items in the right-click menu", false, crate::programs::vlc_lean, || None),
    t("long-paths", System, "Long file paths", "Paths over 260 characters work – torrents and projects with deep folders", false, |_| {
        sys::set_dword(Root::Hklm, r"SYSTEM\CurrentControlSet\Control\FileSystem", "LongPathsEnabled", 1)
    }, || is(Root::Hklm, r"SYSTEM\CurrentControlSet\Control\FileSystem", "LongPathsEnabled", 1)),
    t("network", System, "Home network private", "Every connected network becomes Private – other PCs and the NAS are seen (network discovery)", false, network_private, || None),
    t("dns", System, "Encrypted DNS", "Cloudflare 1.1.1.1 over HTTPS for all adapters – your internet provider does not see the sites you visit", false, dns, || None),
];

pub fn find(id: &str) -> Option<&'static Tweak> {
    ALL.iter().find(|t| t.id == id)
}

/// The setting is offered: the ones about a program only when the program is chosen.
pub fn offered(id: &str, plan: &crate::plan::Plan) -> bool {
    match id {
        "vlc-lean" => plan.has("vlc"),
        _ => true,
    }
}

pub const GROUPS: &[(Group, &str, char)] = &[
    (Privacy, "Privacy and AI", '\u{E72E}'),
    (Taskbar, "Taskbar and Start", '\u{E8A9}'),
    (Explorer, "File Explorer", '\u{E8B7}'),
    (System, "System and games", '\u{E7FC}'),
];

/// The step "Windows settings": every tweak that is on in the plan.
pub fn run(c: &Ctx) -> Res {
    let plan = crate::plan::Plan::load().unwrap_or_default();
    let on: Vec<&Tweak> = ALL.iter().filter(|t| plan.windows_on(t.id) && offered(t.id, &plan)).collect();
    if on.is_empty() {
        return Ok("Nothing chosen".into());
    }
    let mut failed = Vec::new();
    for t in &on {
        c.log(t.title);
        if let Err(e) = (t.apply)(c) {
            failed.push(format!("{}: {e}", t.title));
        }
    }
    sys::settings_changed();
    let text = format!("{} of {} set", on.len() - failed.len(), on.len());
    if failed.is_empty() { Ok(text) } else { Err(format!("{text}\n{}", failed.join("\n"))) }
}

// ---------------------------------------------------------------- the changes

fn copilot(_c: &Ctx) -> Result<(), String> {
    sys::set_dword(Root::Hklm, r"SOFTWARE\Policies\Microsoft\Windows\WindowsCopilot", "TurnOffWindowsCopilot", 1)?;
    sys::set_dword(Root::Hkcu, r"Software\Policies\Microsoft\Windows\WindowsCopilot", "TurnOffWindowsCopilot", 1)?;
    sys::set_dword(Root::Hkcu, ADV, "ShowCopilotButton", 0)?;
    crate::debloat::remove_appx(&["Microsoft.Copilot", "Microsoft.Windows.Ai.Copilot.Provider"])
}

fn recall(_c: &Ctx) -> Result<(), String> {
    sys::dwords(Root::Hklm, AI, &[("DisableAIDataAnalysis", 1), ("AllowRecallEnablement", 0), ("TurnOffSavingSnapshots", 1), ("DisableClickToDo", 1)])?;
    sys::dwords(Root::Hkcu, r"Software\Policies\Microsoft\Windows\WindowsAI", &[("DisableAIDataAnalysis", 1), ("DisableClickToDo", 1)])
}

fn ai_apps(_c: &Ctx) -> Result<(), String> {
    sys::dwords(
        Root::Hklm,
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\Paint",
        &[("DisableCocreator", 1), ("DisableGenerativeFill", 1), ("DisableImageCreator", 1), ("DisableGenerativeErase", 1), ("DisableRemoveBackground", 1)],
    )?;
    sys::set_dword(Root::Hklm, r"SOFTWARE\Policies\WindowsNotepad", "DisableAIFeatures", 1)?;
    sys::set_dword(Root::Hklm, AI, "DisableSettingsAgent", 1)
}

const DSH: &str = r"SOFTWARE\Policies\Microsoft\Dsh";

fn widgets(c: &Ctx) -> Result<(), String> {
    // Windows may refuse the value of the user (it guards it) – the policy is enough
    let _ = sys::set_dword(Root::Hkcu, ADV, "TaskbarDa", 0);
    let Err(e) = sys::set_dword(Root::Hklm, DSH, "AllowNewsAndInterests", 0) else { return Ok(()) };
    // some Windows lock the key of the policy even for administrators (access denied, 2026-11-18): the system account
    // writes it, and when even that fails the Widgets app goes
    sys::log(&format!("widgets policy: {e}"));
    c.log("Widgets off (as the system)");
    let _ = as_system(&format!(r"reg.exe add HKLM\{DSH} /v AllowNewsAndInterests /t REG_DWORD /d 0 /f"));
    if dw(Root::Hklm, DSH, "AllowNewsAndInterests") == Some(0) {
        return Ok(());
    }
    c.log("Removing the Widgets app");
    crate::debloat::remove_appx(&["MicrosoftWindows.Client.WebExperience"]).map_err(|e2| format!("{e}; {e2}"))
}

/// Runs a command line as the system account (a scheduled task) and waits for it (at most 30 s).
fn as_system(line: &str) -> Result<(), String> {
    const TASK: &str = "HeMiMizer-as-system";
    sys::run("schtasks.exe", &["/create", "/tn", TASK, "/tr", line, "/sc", "once", "/st", "00:00", "/ru", "SYSTEM", "/rl", "HIGHEST", "/f"])?;
    let r = sys::run("schtasks.exe", &["/run", "/tn", TASK]).map(|_| ());
    for _ in 0..30 {
        std::thread::sleep(std::time::Duration::from_secs(1));
        let q = sys::exec("schtasks.exe", &["/query", "/tn", TASK, "/fo", "list"]).map(|o| o.text).unwrap_or_default();
        if !q.contains("Running") && !q.contains("Spuštěno") {
            break;
        }
    }
    let _ = sys::exec("schtasks.exe", &["/delete", "/tn", TASK, "/f"]);
    r
}

/// The warnings of Windows Security that only ask for a choice the user made: "sign in with a Microsoft account" and
/// "memory integrity is off" – dismissed as with their Dismiss link.
fn security_nags(_c: &Ctx) -> Result<(), String> {
    sys::set_dword(Root::Hkcu, r"Software\Microsoft\Windows Security Health\State", "AccountProtection_MicrosoftAccount_Disconnected", 1)?;
    sys::set_dword(Root::Hklm, r"SOFTWARE\Microsoft\Windows Security Health\State", "Hardware_HVCI_Off", 1)
}

fn web_search(_c: &Ctx) -> Result<(), String> {
    sys::set_dword(Root::Hkcu, r"Software\Policies\Microsoft\Windows\Explorer", "DisableSearchBoxSuggestions", 1)?;
    sys::dwords(
        Root::Hklm,
        r"SOFTWARE\Policies\Microsoft\Windows\Windows Search",
        &[("AllowCortana", 0), ("ConnectedSearchUseWeb", 0), ("DisableWebSearch", 1), ("EnableDynamicContentInWSB", 0)],
    )?;
    sys::set_dword(Root::Hkcu, r"Software\Microsoft\Windows\CurrentVersion\SearchSettings", "IsDynamicSearchBoxEnabled", 0)?;
    sys::set_dword(Root::Hkcu, r"Software\Microsoft\Windows\CurrentVersion\Search", "BingSearchEnabled", 0)
}

fn ads(_c: &Ctx) -> Result<(), String> {
    sys::set_dword(Root::Hklm, r"SOFTWARE\Policies\Microsoft\Windows\AdvertisingInfo", "DisabledByGroupPolicy", 1)?;
    sys::set_dword(Root::Hkcu, r"Software\Microsoft\Windows\CurrentVersion\AdvertisingInfo", "Enabled", 0)?;
    sys::set_dword(Root::Hkcu, r"Software\Microsoft\Windows\CurrentVersion\Privacy", "TailoredExperiencesWithDiagnosticDataEnabled", 0)?;
    sys::dwords(
        Root::Hklm,
        r"SOFTWARE\Policies\Microsoft\Windows\CloudContent",
        &[("DisableWindowsConsumerFeatures", 1), ("DisableConsumerAccountStateContent", 1), ("DisableCloudOptimizedContent", 1)],
    )?;
    sys::dwords(Root::Hkcu, r"Software\Policies\Microsoft\Windows\CloudContent", &[("DisableTailoredExperiencesWithDiagnosticData", 1), ("DisableThirdPartySuggestions", 1)])?;
    sys::dwords(
        Root::Hkcu,
        CDM,
        &[
            ("ContentDeliveryAllowed", 0),
            ("FeatureManagementEnabled", 0),
            ("OemPreInstalledAppsEnabled", 0),
            ("PreInstalledAppsEnabled", 0),
            ("PreInstalledAppsEverEnabled", 0),
            ("SilentInstalledAppsEnabled", 0),
            ("SoftLandingEnabled", 0),
            ("SystemPaneSuggestionsEnabled", 0),
            ("SubscribedContentEnabled", 0),
            ("RotatingLockScreenOverlayEnabled", 0),
            ("SubscribedContent-310093Enabled", 0),
            ("SubscribedContent-338387Enabled", 0),
            ("SubscribedContent-338388Enabled", 0),
            ("SubscribedContent-338389Enabled", 0),
            ("SubscribedContent-338393Enabled", 0),
            ("SubscribedContent-353694Enabled", 0),
            ("SubscribedContent-353696Enabled", 0),
            ("SubscribedContent-353698Enabled", 0),
        ],
    )?;
    sys::set_dword(Root::Hkcu, r"Software\Microsoft\Windows\CurrentVersion\UserProfileEngagement", "ScoobeSystemSettingEnabled", 0)?;
    sys::set_dword(Root::Hkcu, ADV, "ShowSyncProviderNotifications", 0)
}

fn telemetry(_c: &Ctx) -> Result<(), String> {
    sys::dwords(
        Root::Hklm,
        r"SOFTWARE\Policies\Microsoft\Windows\DataCollection",
        &[("AllowTelemetry", 0), ("DoNotShowFeedbackNotifications", 1), ("AllowDeviceNameInTelemetry", 0)],
    )?;
    sys::set_dword(Root::Hkcu, r"Software\Microsoft\Siuf\Rules", "NumberOfSIUFInPeriod", 0)?;
    sys::set_dword(Root::Hkcu, r"Software\Microsoft\input\Settings", "InsightsEnabled", 0)
}

fn activity(_c: &Ctx) -> Result<(), String> {
    sys::dwords(Root::Hklm, r"SOFTWARE\Policies\Microsoft\Windows\System", &[("PublishUserActivities", 0), ("UploadUserActivities", 0), ("EnableActivityFeed", 0)])
}

fn edge(_c: &Ctx) -> Result<(), String> {
    sys::dwords(
        Root::Hklm,
        r"SOFTWARE\Policies\Microsoft\Edge",
        &[
            ("HubsSidebarEnabled", 0),
            ("CopilotPageContext", 0),
            ("CopilotCDPPageContext", 0),
            ("EdgeShoppingAssistantEnabled", 0),
            ("ShowRecommendationsEnabled", 0),
            ("PersonalizationReportingEnabled", 0),
            ("StartupBoostEnabled", 0),
            ("BackgroundModeEnabled", 0),
        ],
    )
}

fn taskbar_extras(_c: &Ctx) -> Result<(), String> {
    sys::set_dword(Root::Hkcu, r"Software\Microsoft\Windows\CurrentVersion\CrossDeviceResume\Configuration", "IsResumeAllowed", 0)?;
    sys::set_dword(Root::Hkcu, r"Software\Microsoft\TabletTip\1.7", "EmojiAndMoreIconVisibilityState", 0)?;
    sys::set_dword(Root::Hkcu, r"Software\Microsoft\Windows\CurrentVersion\PenWorkspace", "PenWorkspaceButtonDesiredVisibility", 0)?;
    sys::set_dword(Root::Hkcu, ADV, "TaskbarMn", 0)
}

fn start_recommended(_c: &Ctx) -> Result<(), String> {
    sys::dwords(Root::Hkcu, ADV, &[("Start_TrackDocs", 0), ("Start_IrisRecommendations", 0)])
}

fn start_apps(_c: &Ctx) -> Result<(), String> {
    sys::set_dword(Root::Hkcu, ADV, "Start_TrackProgs", 0)?;
    sys::dwords(Root::Hkcu, START, &[("ShowRecentList", 0), ("ShowFrequentList", 0)])
}

fn recent_off(_c: &Ctx) -> Result<(), String> {
    sys::dwords(Root::Hkcu, EXPLORER, &[("ShowRecent", 0), ("ShowFrequent", 0)])
}

fn home_gallery_off(_c: &Ctx) -> Result<(), String> {
    for clsid in HOME_GALLERY {
        sys::set_dword(Root::Hkcu, &format!(r"Software\Classes\CLSID\{clsid}"), "System.IsPinnedToNameSpaceTree", 0)?;
    }
    Ok(())
}

fn recycle_off(_c: &Ctx) -> Result<(), String> {
    sys::set_dword(Root::Hkcu, r"Software\Microsoft\Windows\CurrentVersion\Policies\Explorer", "NoRecycleFiles", 1)?;
    let bin = "{645FF040-5081-101B-9F08-00AA002F954E}";
    sys::set_dword(Root::Hkcu, &format!(r"{EXPLORER}\HideDesktopIcons\NewStartPanel"), bin, 1)?;
    sys::set_dword(Root::Hkcu, &format!(r"{EXPLORER}\HideDesktopIcons\ClassicStartMenu"), bin, 1)
}

fn dark(_c: &Ctx) -> Result<(), String> {
    sys::dwords(Root::Hkcu, PERSONALIZE, &[("AppsUseLightTheme", 0), ("SystemUsesLightTheme", 0)])
}

fn power(_c: &Ctx) -> Result<(), String> {
    let pc = "powercfg.exe";
    if sys::run(pc, &["/setactive", HIGH_PERFORMANCE]).is_err() {
        // Windows 11 hides the plan on some PCs – it is made from its template
        sys::run(pc, &["/duplicatescheme", HIGH_PERFORMANCE, HIGH_PERFORMANCE])?;
        sys::run(pc, &["/setactive", HIGH_PERFORMANCE])?;
    }
    // on a laptop the battery keeps its sleep
    for (what, v) in [("monitor-timeout-ac", "10"), ("standby-timeout-ac", "0")] {
        sys::run(pc, &["/change", what, v])?;
    }
    for set in ["/setacvalueindex", "/setdcvalueindex"] {
        sys::run(pc, &[set, "SCHEME_CURRENT", "SUB_BUTTONS", "PBUTTONACTION", "3"])?;
    }
    sys::run(pc, &["/setactive", "SCHEME_CURRENT"]).map(|_| ())
}

fn games(_c: &Ctx) -> Result<(), String> {
    sys::dwords(Root::Hkcu, r"Software\Microsoft\GameBar", &[("AutoGameModeEnabled", 1), ("AllowAutoGameMode", 1)])?;
    sys::set_dword(Root::Hklm, r"SYSTEM\CurrentControlSet\Control\GraphicsDrivers", "HwSchMode", 2)?;
    sys::set_sz(Root::Hkcu, r"Software\Microsoft\DirectX\UserGpuPreferences", "DirectXUserGlobalSettings", "SwapEffectUpgradeEnable=1;")
}

fn game_dvr(_c: &Ctx) -> Result<(), String> {
    sys::dwords(Root::Hkcu, r"Software\Microsoft\GameBar", &[("UseNexusForGameBarEnabled", 0), ("GamepadNexusChordEnabled", 0)])?;
    sys::set_dword(Root::Hkcu, r"Software\Microsoft\Windows\CurrentVersion\GameDVR", "AppCaptureEnabled", 0)?;
    sys::set_dword(Root::Hkcu, r"System\GameConfigStore", "GameDVR_Enabled", 0)
}

fn mouse(_c: &Ctx) -> Result<(), String> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{SPI_SETMOUSE, SPIF_SENDCHANGE, SPIF_UPDATEINIFILE, SystemParametersInfoW};
    let mut m = [0i32; 3];
    // SAFETY: SPI_SETMOUSE reads an array of three ints, `m` is one
    if unsafe { SystemParametersInfoW(SPI_SETMOUSE, 0, m.as_mut_ptr() as _, SPIF_UPDATEINIFILE | SPIF_SENDCHANGE) } == 0 {
        for (n, v) in [("MouseSpeed", "0"), ("MouseThreshold1", "0"), ("MouseThreshold2", "0")] {
            sys::set_sz(Root::Hkcu, r"Control Panel\Mouse", n, v)?;
        }
    }
    Ok(())
}

fn sound(_c: &Ctx) -> Result<(), String> {
    sys::set_dword(Root::Hklm, r"SOFTWARE\Microsoft\Windows\CurrentVersion\Authentication\LogonUI\BootAnimation", "DisableStartupSound", 1)?;
    sys::set_dword(Root::Hklm, r"SOFTWARE\Microsoft\Windows\CurrentVersion\EditionOverrides", "UserSetting_DisableStartupSound", 1)?;
    sys::set_dword(Root::Hkcu, r"Software\Microsoft\Multimedia\Audio", "UserDuckingPreference", 3)
}

fn restart_apps(_c: &Ctx) -> Result<(), String> {
    sys::set_dword(Root::Hkcu, r"Software\Microsoft\Windows NT\CurrentVersion\Winlogon", "RestartApps", 0)?;
    sys::set_dword(Root::Hklm, r"SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\System", "DisableAutomaticRestartSignOn", 1)
}

fn network_private(c: &Ctx) -> Result<(), String> {
    c.log("Private network, network discovery");
    sys::ps(
        "Get-NetConnectionProfile | Where-Object NetworkCategory -ne 'DomainAuthenticated' | Set-NetConnectionProfile -NetworkCategory Private; \
         Get-NetFirewallRule -Group '@FirewallAPI.dll,-32752' | Where-Object { $_.Profile -match 'Private|Any' } | Enable-NetFirewallRule",
    )
    .map(|_| ())
}

fn dns(c: &Ctx) -> Result<(), String> {
    c.log("Encrypted DNS (Cloudflare)");
    sys::ps(
        r#"
foreach ($ip in '1.1.1.1','1.0.0.1','2606:4700:4700::1111','2606:4700:4700::1001') {
  if (Get-DnsClientDohServerAddress -ServerAddress $ip -ErrorAction SilentlyContinue) {
    Set-DnsClientDohServerAddress -ServerAddress $ip -AutoUpgrade $true -AllowFallbackToUdp $false | Out-Null
  } else {
    Add-DnsClientDohServerAddress -ServerAddress $ip -DohTemplate 'https://cloudflare-dns.com/dns-query' -AutoUpgrade $true -AllowFallbackToUdp $false | Out-Null
  }
}
Get-NetAdapter -Physical | Where-Object Status -eq 'Up' | ForEach-Object {
  Set-DnsClientServerAddress -InterfaceIndex $_.ifIndex -ServerAddresses '1.1.1.1','1.0.0.1','2606:4700:4700::1111','2606:4700:4700::1001'
}
Clear-DnsClientCache
"#,
    )
    .map(|_| ())
}
