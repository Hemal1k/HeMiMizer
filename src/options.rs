//! The steps of Windows the wizard lets the user switch on or off (the page "Windows"), with their defaults: what most
//! people want is on, the bigger changes of taste are off. A step listed here runs only when it is on in the plan;
//! steps not listed (drivers, programs, the settings of the backup) run as they are. The single settings of Windows
//! (tweaks.rs) are kept in the same list of the plan.

/// (id of the step, on by default)
pub const OPTIONS: &[(&str, bool)] = &[
    ("restore-point", true),
    ("microsoft-apps", true),
    ("ad-software", true),
    ("xbox-apps", false),
    ("hibernation", false),
    ("startup", false),
    ("no-wu-drivers", false),
    ("features", false),
    ("runtimes", true),
    ("store", true),
    ("windows-update", true),
    ("cleanup", true),
];

/// Steps of the author's own taste that the public HeMimizer never runs.
pub const NEVER: &[&str] = &["restore", "calculator"];

pub fn is_option(id: &str) -> bool {
    OPTIONS.iter().any(|(o, _)| *o == id)
}

/// The recommended choice: the steps and the single settings that are on by default.
pub fn defaults() -> Vec<String> {
    OPTIONS
        .iter()
        .filter(|(_, on)| *on)
        .map(|(id, _)| id.to_string())
        .chain(crate::tweaks::ALL.iter().filter(|t| t.recommended).map(|t| t.id.to_string()))
        .collect()
}

/// The step runs (by the plan of the wizard; no plan = the defaults).
pub fn runs(id: &str) -> bool {
    if NEVER.contains(&id) {
        return false;
    }
    if !is_option(id) {
        return true;
    }
    let plan = crate::plan::Plan::load().unwrap_or_default();
    plan.windows.unwrap_or_else(defaults).iter().any(|w| w == id)
}
