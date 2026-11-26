//! The look all HeMi apps in Rust share: the theme (automatic / light / dark), chosen at the top of the settings and
//! kept in %AppData%\HeMi\HeMiMizer\appearance.ini. The language is kept too but not offered (English only); the
//! updates and the log (Logging) are kept there as well.

use std::path::PathBuf;

/// How often the own updates are looked for (Settings → Updates), in hours; 0 = only at the start.
pub const UPDATE_HOURS: [u8; 5] = [0, 1, 3, 6, 24];
/// Every 6 hours, also for an appearance.ini written before the choice existed.
pub const DEFAULT_UPDATE_HOURS: u8 = 6;

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Look {
    /// 0 automatic (HeMiDock, else Windows), 1 light, 2 dark.
    pub theme: u8,
    /// 0 as Windows, 1 English, 2 Czech.
    pub language: u8,
    /// The own updates switched off (Settings → Updates): no question to GitHub at the start.
    pub no_updates: bool,
    /// How often GitHub is asked again while HeMiMizer runs (one of UPDATE_HOURS; 0 = only at the start).
    pub update_hours: u8,
    /// The released HeMiMizer writes its log (Settings → Logging; off by default – a Debug build always writes one).
    pub logging: bool,
}

impl Default for Look {
    fn default() -> Self {
        Look { theme: 0, language: 0, no_updates: false, update_hours: DEFAULT_UPDATE_HOURS, logging: false }
    }
}

/// %AppData%\HeMi\HeMiMizer: appearance.ini, and the log of the released HeMiMizer when Logging is on.
pub fn dir() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("APPDATA")?).join("HeMi").join("HeMiMizer"))
}

fn file() -> Option<PathBuf> {
    Some(dir()?.join("appearance.ini"))
}

/// The hours as saved; anything not offered is the default.
fn parse_update_hours(v: &str) -> u8 {
    v.trim().parse::<u8>().ok().filter(|h| UPDATE_HOURS.contains(h)).unwrap_or(DEFAULT_UPDATE_HOURS)
}

impl Look {
    fn parse(text: &str) -> Look {
        let mut l = Look::default();
        for line in text.lines() {
            match line.split_once('=') {
                Some(("Theme", v)) => l.theme = v.trim().parse::<u8>().unwrap_or(0).min(2),
                Some(("Language", v)) => l.language = v.trim().parse::<u8>().unwrap_or(0).min(2),
                Some(("Updates", v)) => l.no_updates = v.trim() == "0",
                Some(("UpdateHours", v)) => l.update_hours = parse_update_hours(v),
                Some(("Logging", v)) => l.logging = v.trim() == "1",
                _ => {}
            }
        }
        l
    }

    fn text(&self) -> String {
        format!(
            "Theme={}\nLanguage={}\nUpdates={}\nUpdateHours={}\nLogging={}\n",
            self.theme,
            self.language,
            if self.no_updates { 0 } else { 1 },
            self.update_hours,
            u8::from(self.logging)
        )
    }

    pub fn load() -> Look {
        file().and_then(|f| std::fs::read_to_string(f).ok()).map(|t| Look::parse(&t)).unwrap_or_default()
    }

    pub fn save(&self) {
        if let Some(f) = file() {
            if let Some(dir) = f.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(f, self.text());
        }
    }

    /// The pause between two questions to GitHub while HeMiMizer runs; None = only at the start.
    pub fn update_interval(&self) -> Option<std::time::Duration> {
        (self.update_hours > 0).then(|| std::time::Duration::from_secs(u64::from(self.update_hours) * 3600))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_hours_default_and_round_trip() {
        // an appearance.ini from before the choice: every 6 hours
        assert_eq!(Look::parse("Theme=1\nLanguage=0\nUpdates=1\n").update_hours, 6);
        assert_eq!(Look::parse("UpdateHours=5\n").update_hours, 6);
        assert_eq!(Look::parse("UpdateHours=x\n").update_hours, 6);
        for h in UPDATE_HOURS {
            let l = Look { update_hours: h, no_updates: true, ..Look::default() };
            assert_eq!(Look::parse(&l.text()), l);
        }
        assert_eq!(Look { update_hours: 0, ..Look::default() }.update_interval(), None);
        // the log: off when not saved (older appearance.ini), kept both ways
        assert!(!Look::parse("Theme=1\nLanguage=0\nUpdates=1\n").logging);
        for on in [false, true] {
            let l = Look { logging: on, ..Look::default() };
            assert_eq!(Look::parse(&l.text()), l);
        }
        assert_eq!(Look::default().update_interval(), Some(std::time::Duration::from_secs(6 * 3600)));
    }
}