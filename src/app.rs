//! The window of HeMiMizer, as simple as the author wants it: it asks for the password of the NAS (which is also
//! the start – nothing runs before Run is clicked), then shows the step it is on, out of how many, what it is doing
//! and what failed; at the end what is left to do by hand and a restart.

use crate::i18n::tr;
use crate::steps::{self, Event, Step};
use crate::theme::{self, Palette, glyph};
use crate::widgets::{Btn, Tone, card, info_bar, row_layout, section, two_lines};
use crate::{config, downloads, drivers, programs, sys};
use eframe::egui::{self, Align, Color32, CornerRadius, Id, Layout, Margin, RichText, Stroke, pos2, vec2};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

#[path = "usb_page.rs"]
mod usb_page;
#[path = "iso_page.rs"]
mod iso_page;
#[path = "dl_page.rs"]
mod dl_page;

const REPO: &str = "https://github.com/Hemal1k/HeMiMizer";
const RESTART_DELAY: u64 = 20;

/// How HeMiMizer was started.
#[derive(Clone, Copy, PartialEq)]
pub enum Mode {
    /// Asks for the NAS password and runs everything.
    Normal,
    /// `--resume`: the steps left before the restart, at once.
    Resume,
}

/// The pages of the wizard, in order.
#[derive(Clone, Copy, PartialEq)]
enum Page {
    Hardware,
    Browser,
    Apps,
    Windows,
    Settings,
    Folders,
    Save,
}

#[derive(Clone, Copy, PartialEq)]
enum Phase {
    /// "Make a Windows USB stick".
    Usb,
    /// "Make a smaller Windows".
    Iso,
    /// "Download Windows".
    Download,
    /// A backup protected with a password: the password first.
    Unlock,
    /// The first page: back up this PC (before a reinstall) or set it up (after one).
    Choose,
    /// The wizard: what the new PC gets (before the backup, or on the new PC without a backup).
    Wizard(Page),
    /// The backup runs, then its result.
    BackingUp,
    BackedUp,
    /// The start dialog of the setup (drivers, NAS password, Run / Close).
    Start,
    Running,
    Finished,
}

#[derive(Clone, PartialEq)]
enum Dialog {
    Reboot,
}

enum Action {
    Go(Phase),
    /// the wizard from the first page: true = before a backup, false = on the new PC
    StartWizard(bool),
    /// the next / previous page of the wizard
    Next,
    Prev,
    /// a program of the wizard on or off
    Pick(&'static str, bool),
    /// the browser: one or none
    Browser(Option<&'static str>),
    Offline(bool),
    /// the licence terms accepted or not (at the setup)
    Accept(bool),
    /// the programs with licence terms left out (at the setup)
    DropLicensed,
    /// a page of the wizard already visited (the steps on top)
    GoPage(Page),
    /// a section folded or unfolded (by its title)
    Fold(String),
    /// a starting point of the page Windows: 0 = recommended, 1 = like this PC, 2 = nothing
    Preset(u8),
    /// a step of Windows on or off
    WinOpt(&'static str, bool),
    AddFolder,
    RemoveFolder(usize),
    BrowseSaveTo,
    ToggleBackup(usize),
    /// The theme changed.
    SetTheme(u8),
    /// The end: HeMiMizer removes itself and its data, then closes.
    Done,
    RunBackup,
    OpenFolder,
    Pack,
    Unlock,
    Retry,
    UsbOpen,
    UsbRefresh,
    UsbBrowse,
    UsbMake,
    UsbAsk,
    UsbCancel,
    DlOpen,
    DlBrowse,
    DlStart,
    DlToUsb(String),
    DlToIso(String),
    IsoOpen,
    IsoBrowse,
    IsoDrivers,
    IsoMake,
    Start,
    Quit,
    AskReboot,
    About(bool),
    Open(String),
    OpenLicences,
    OpenLog,
    DefaultApps(&'static str),
    RestartNow,
    CancelRestart,
    /// browse for the installer of a driver: 0 = chipset, 1 = graphics
    BrowseDriver(usize),
    /// browse for a file that failed to download (index in need_files)
    BrowseFile(usize),
    /// choose the OC profile exported in Adrenalin (index of the backup item)
    PickOc(usize),
    RetryDownloads,
    ContinueDownloads,
}

pub struct HeMiMizerApp {
    steps: Vec<Step>,
    /// The indices of the steps of this run, in order.
    chosen: Vec<usize>,
    phase: Phase,
    /// How many of `chosen` are finished.
    done: usize,
    current: Option<usize>,
    log_line: String,
    failures: Vec<(String, String)>,
    /// the steps that failed (indices), for "Try again"
    failed_steps: Vec<usize>,
    /// the overview of the start page: the steps that will do something, for this plan
    overview: Option<(crate::plan::Plan, Vec<usize>)>,
    notes: Vec<String>,
    reboot: bool,
    restart_at: Option<Instant>,
    waiting_internet: bool,
    /// (server, user, password) typed in the start dialog
    passwords: Vec<(String, String, String)>,
    tx: Sender<Event>,
    rx: Receiver<Event>,
    p: Palette,
    source: theme::Source,
    theme_checked: Instant,
    hwnd: isize,
    /// Mica of Windows 11 behind the window (transparent background)
    mica: bool,
    /// The theme chosen in the settings.
    look: crate::look::Look,
    about: bool,
    dialog: Option<Dialog>,
    close_warned: bool,
    /// the run ended before a restart – HeMiMizer continues later, so it must not remove itself
    resume_pending: bool,
    removed: bool,
    fresh: bool,
    /// the installers of the drivers (chipset, graphics) chosen on the start page
    driver_paths: [String; 2],
    /// downloads that failed: (id, name, error, file chosen by hand)
    need_files: Option<Vec<(String, String, String, String)>>,
    /// The backup: the items, which are on, their results, what it does now.
    backup_items: Vec<crate::backup::Item>,
    /// programs HeMiMizer does not install whose settings are on this PC (Adobe, DaVinci Resolve…)
    found_apps: Vec<&'static str>,
    /// packing the backup into a .zip: None = not asked, Some(None) = packing, Some(Some(result)) = done
    pack: std::sync::Arc<std::sync::Mutex<Option<Option<Result<String, String>>>>>,
    /// the password of the backup, typed twice on its last page (empty = not protected), and how the encryption went
    seal_pw: String,
    seal_pw2: String,
    sealed: Option<Result<String, String>>,
    /// a protected backup at the start: the password typed, and the unlocking (None = not yet, Some(None) = running)
    unlock_pw: String,
    unlock: std::sync::Arc<std::sync::Mutex<Option<Option<Result<(), String>>>>>,
    /// this PC has Wi-Fi networks saved (the backup offers them)
    has_wifi: bool,
    /// the page "Make a Windows USB stick"
    usb: usb_page::UsbUi,
    /// the page "Make a smaller Windows"
    iso: iso_page::IsoUi,
    /// the page "Download Windows"
    dl: dl_page::DlUi,
    backup_on: Vec<bool>,
    backup_done: Vec<Option<Result<String, String>>>,
    backup_now: Option<usize>,
    backup_log: String,
    btx: Sender<crate::backup::Event>,
    brx: Receiver<crate::backup::Event>,
    /// The installers of the backup (for the setup page).
    offline: usize,
    /// The choices of the wizard; `plan_fresh`: not yet filled with what was found on this PC.
    plan: crate::plan::Plan,
    plan_fresh: bool,
    /// the wizard runs before a backup (else on the new PC)
    wizard_backup: bool,
    /// the start page was reached through the wizard (Back returns to it)
    start_via_wizard: bool,
    /// the network drives of this PC, shown in the backup (found when the backup page opens)
    found_drives: Vec<(String, String)>,
    /// the folder the backup goes to (empty = the folder of HeMiMizer.exe)
    save_to: String,
    /// the search on the page of the programs
    search: String,
    /// the sections folded (by their titles)
    folded: std::collections::HashSet<String>,
    /// how the single settings of Windows are on this PC (tweaks::ALL in order), read once before a backup
    this_pc: Option<Vec<Option<bool>>>,
    /// the furthest page of the wizard reached (its steps on top go back to any of them)
    reached: usize,
}

impl HeMiMizerApp {
    pub fn new(cc: &eframe::CreationContext<'_>, mode: Mode) -> Self {
        let look = crate::look::Look::load();
        let source = theme::read_source(look.theme);
        let p = theme::palette(&source);
        theme::install_fonts(&cc.egui_ctx);
        theme::apply(&cc.egui_ctx, &p);
        let hwnd = match cc.window_handle().map(|h| h.as_raw()) {
            Ok(RawWindowHandle::Win32(w)) => w.hwnd.get(),
            _ => 0,
        };
        let mica = theme::style_window(hwnd, &p);
        let steps = steps::all();
        let chosen: Vec<usize> = if mode == Mode::Resume {
            let ids = steps::pending_resume();
            (0..steps.len()).filter(|&i| ids.iter().any(|id| id == steps[i].id)).collect()
        } else {
            (0..steps.len()).collect()
        };
        // the NAS: one row per server, the user as captured (or as saved in Windows)
        let mut passwords: Vec<(String, String, String)> = Vec::new();
        for d in config::drives() {
            let s = d.server();
            if !passwords.iter().any(|(x, _, _)| *x == s) {
                let user = programs::saved_user(&s).filter(|u| !u.is_empty()).unwrap_or(d.user.clone());
                passwords.push((s, user, String::new()));
            }
        }
        let (btx, brx) = channel();
        let (tx, rx) = channel();
        let mut app = HeMiMizerApp {
            steps,
            chosen,
            phase: if crate::crypto::locked() { Phase::Unlock } else { Phase::Choose },
            done: 0,
            current: None,
            log_line: String::new(),
            failures: Vec::new(),
            failed_steps: Vec::new(),
            overview: None,
            notes: Vec::new(),
            reboot: false,
            restart_at: None,
            waiting_internet: false,
            passwords,
            tx,
            rx,
            p,
            source,
            theme_checked: Instant::now(),
            hwnd,
            mica,
            look,
            about: false,
            dialog: None,
            close_warned: false,
            resume_pending: false,
            removed: false,
            fresh: mode == Mode::Normal,
            driver_paths: [
                drivers::found(drivers::Kind::Chipset).map(|p| p.display().to_string()).unwrap_or_default(),
                drivers::found(drivers::Kind::Graphics).map(|p| p.display().to_string()).unwrap_or_default(),
            ],
            need_files: None,
            backup_on: { let saved = crate::backup::saved_choice(); crate::backup::items().iter().map(|i| saved.iter().any(|s| s == i.id)).collect() },
            backup_done: crate::backup::items().iter().map(|_| None).collect(),
            backup_items: crate::backup::items(),
            found_apps: crate::appdata::ALL.iter().filter(|d| d.found_only() && crate::appdata::present(d.id)).map(|d| d.id).collect(),
            pack: Default::default(),
            seal_pw: String::new(),
            seal_pw2: String::new(),
            sealed: None,
            unlock_pw: String::new(),
            unlock: Default::default(),
            has_wifi: config::has_wifi(),
            usb: usb_page::UsbUi::new(),
            iso: iso_page::IsoUi::new(),
            dl: dl_page::DlUi::new(),
            backup_now: None,
            backup_log: String::new(),
            btx,
            brx,
            offline: crate::backup::offline().len(),
            plan: crate::plan::Plan::load().unwrap_or_default(),
            plan_fresh: crate::plan::Plan::load().is_none(),
            wizard_backup: true,
            start_via_wizard: false,
            found_drives: Vec::new(),
            save_to: config::exe_dir().display().to_string(),
            search: String::new(),
            folded: ["Privacy and AI", "Taskbar and Start", "File Explorer", "System and games", "Updates and runtimes", "For developers", "ArtCraft – free creative apps"]
                .into_iter()
                .map(String::from)
                .collect(),
            this_pc: None,
            reached: 0,
        };
        // the settings of the programs: on when they are on this PC (they are offered only for the programs chosen)
        for (i, it) in app.backup_items.iter().enumerate() {
            // on when its settings are on this PC (a program not on it has nothing to take)
            if crate::appdata::find(it.id).is_some() {
                app.backup_on[i] = crate::appdata::present(it.id);
            }
        }
        if mode == Mode::Resume {
            app.start(&cc.egui_ctx);
        }
        app
    }

    fn start(&mut self, ctx: &egui::Context) {
        // a password left empty: the one saved in Windows is used (if there is one)
        let passwords: Vec<_> = self.passwords.iter().filter(|(_, u, p)| !u.is_empty() && !p.is_empty()).cloned().collect();
        if self.fresh {
            steps::fresh_start();
            let path = |s: &String| (!s.trim().is_empty()).then(|| std::path::PathBuf::from(s.trim().trim_matches('"')));
            drivers::set_chosen(drivers::Kind::Chipset, path(&self.driver_paths[0]));
            drivers::set_chosen(drivers::Kind::Graphics, path(&self.driver_paths[1]));
        }
        self.phase = Phase::Running;
        self.done = 0;
        self.failures.clear();
        self.failed_steps.clear();
        self.notes.clear();
        let ctx = ctx.clone();
        steps::run(self.chosen.clone(), passwords, self.tx.clone(), move || ctx.request_repaint());
    }

    fn poll(&mut self) {
        while let Ok(ev) = self.brx.try_recv() {
            match ev {
                crate::backup::Event::Start(i) => {
                    self.backup_now = Some(i);
                    self.backup_log.clear();
                }
                crate::backup::Event::Log(t) => self.backup_log = t,
                crate::backup::Event::Done(i, r) => self.backup_done[i] = Some(r),
                crate::backup::Event::Sealed(r) => self.sealed = Some(r),
                crate::backup::Event::Finished => {
                    self.backup_now = None;
                    self.phase = Phase::BackedUp;
                    // a backup in another folder: HeMiMizer goes along, the setup is started from there
                    let to = config::save_to();
                    if to != config::exe_dir() {
                        if let Ok(exe) = std::env::current_exe() {
                            if let Err(e) = std::fs::copy(&exe, to.join("HeMiMizer.exe")) {
                                sys::log(&format!("copy HeMiMizer.exe to {}: {e}", to.display()));
                            }
                        }
                    }
                    self.offline = crate::backup::offline().len();
                }
            }
        }
        while let Ok(ev) = self.rx.try_recv() {
            match ev {
                Event::Start(i) => {
                    self.current = Some(i);
                    self.log_line.clear();
                }
                Event::Log(t) => self.log_line = t,
                Event::Note(t) => self.notes.push(t),
                Event::Internet(on) => self.waiting_internet = on,
                Event::NeedFiles(list) => self.need_files = Some(list.into_iter().map(|(i, n, e)| (i, n, e, String::new())).collect()),
                Event::Done(i, r) => {
                    self.done += 1;
                    if let Err(e) = r {
                        self.failures.push((self.steps[i].title().to_string(), e));
                        self.failed_steps.push(i);
                    }
                }
                // put off until the internet: counted when it runs again
                Event::Later(i) => self.log_line = format!("{} – {}", self.steps[i].title(), tr("after the internet is connected", "až po připojení k internetu")),
                Event::Finished { reboot, resume } => {
                    self.phase = Phase::Finished;
                    self.reboot = reboot;
                    self.current = None;
                    self.log_line.clear();
                    self.waiting_internet = false;
                    self.resume_pending = resume;
                    if resume {
                        self.restart_at = Some(Instant::now() + Duration::from_secs(RESTART_DELAY));
                    }
                }
            }
        }
    }

    /// Light / dark or the accent colour changed (in Windows or in HeMiDock): the new colours at once.
    fn follow_theme(&mut self, ctx: &egui::Context) {
        if self.theme_checked.elapsed() < Duration::from_secs(2) {
            return;
        }
        self.theme_checked = Instant::now();
        let source = theme::read_source(self.look.theme);
        if source != self.source {
            self.source = source;
            self.p = theme::palette(&source);
            theme::apply(ctx, &self.p);
            self.mica = theme::style_window(self.hwnd, &self.p);
        }
    }

    // ------------------------------------------------------------------ the window

    fn header(&self, ui: &mut egui::Ui, acts: &mut Vec<Action>) {
        let p = self.p;
        ui.horizontal(|ui| {
            ui.set_min_height(40.0);
            // back to the first page from the backup and from the start of the setup
            let back_to_choose = !self.about && ((matches!(self.phase, Phase::BackedUp | Phase::Start | Phase::Wizard(_)) && self.fresh) || (self.phase == Phase::Usb && !self.usb.running()) || (self.phase == Phase::Iso && !self.iso.running()) || (self.phase == Phase::Download && !self.dl.running()));
            if self.about || back_to_choose {
                if Btn::new("").icon(glyph::BACK).subtle().show(ui, &p).clicked() {
                    acts.push(match self.phase {
                        _ if self.about => Action::About(false),
                        // the wizard page by page; the start goes back to its last page
                        Phase::Wizard(_) | Phase::Start => Action::Prev,
                        _ => Action::Go(Phase::Choose),
                    });
                }
                ui.add_space(4.0);
            }
            let title = if self.about {
                tr("Settings", "Nastavení")
            } else {
                match self.phase {
                    Phase::Wizard(page) => self.page_title(page),
                    Phase::BackingUp | Phase::BackedUp => tr("Back up this PC", "Zálohovat toto PC"),
                    Phase::Start => tr("Set up this PC", "Nastavit toto PC"),
                    Phase::Unlock => "Protected backup",
                    Phase::Usb => "Windows USB stick",
                    Phase::Iso => "A smaller Windows",
                    Phase::Download => "Download Windows",
                    _ => "HeMiMizer",
                }
            };
            ui.label(RichText::new(title).font(theme::display(28.0)).color(p.text));
            if !self.about {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if Btn::new("").icon(glyph::SETTINGS).subtle().show(ui, &p).clicked() {
                        acts.push(Action::About(true));
                    }
                });
            }
        });
        if let (Phase::Wizard(page), false) = (self.phase, self.about) {
            ui.add_space(10.0);
            self.stepper(ui, page, acts);
            ui.add_space(4.0);
        } else {
            ui.add_space(18.0);
        }
    }

    /// Settings, as in the other HeMi apps: the theme, then About with the links side by side.
    fn about_page(&self, ui: &mut egui::Ui, acts: &mut Vec<Action>) {
        let p = self.p;
        ui.spacing_mut().item_spacing.y = 4.0;
        section(ui, &p, "Appearance");
        card(ui, &p, |ui| {
            row_layout(ui, 44.0, |ui| {
                ui.label(RichText::new("Theme").size(14.0).color(p.text));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if let Some(i) = crate::widgets::dropdown(ui, &p, self.look.theme as usize, &["Automatic", "Light", "Dark"], 220.0) {
                        acts.push(Action::SetTheme(i as u8));
                    }
                });
            });
        });
        ui.add_space(14.0);
        section(ui, &p, "About");
        let value = |ui: &mut egui::Ui, label: &str, value: &str| {
            card(ui, &p, |ui| {
                row_layout(ui, 44.0, |ui| {
                    ui.label(RichText::new(label).size(14.0).color(p.text));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(RichText::new(value).size(14.0).color(p.text2));
                    });
                });
            });
        };
        value(ui, "Author", "Richard Hemala");
        value(ui, "Version", env!("CARGO_PKG_VERSION"));
        let items = [("Project on GitHub", REPO), ("Report a problem", ""), ("Log", "HeMiMizer.log"), ("Licences", "")];
        if let Some(i) = card(ui, &p, |ui| crate::widgets::button_row(ui, &p, &items)) {
            acts.push(match i {
                0 => Action::Open(REPO.into()),
                1 => Action::Open(format!("{REPO}/issues")),
                2 => Action::OpenLog,
                _ => Action::OpenLicences,
            });
        }
    }

    /// A path field with Browse (the installers of the drivers, files that failed to download).
    fn path_row(ui: &mut egui::Ui, p: &Palette, value: &mut String, hint: &str) -> bool {
        let mut browse = false;
        ui.horizontal(|ui| {
            let w = ui.available_width() - 120.0;
            ui.add(egui::TextEdit::singleline(value).hint_text(hint).desired_width(w.max(100.0)).margin(vec2(8.0, 6.0)));
            if Btn::new(tr("Browse…", "Procházet…")).show(ui, p).clicked() {
                browse = true;
            }
        });
        browse
    }

    /// The first page: before a reinstall (backup) or after it (setup).
    fn choose_page(&self, ui: &mut egui::Ui, acts: &mut Vec<Action>) {
        let p = self.p;
        let big = |ui: &mut egui::Ui, g: char, title: &str, detail: &str, go: Action, acts: &mut Vec<Action>| {
            let resp = egui::Frame::new()
                .fill(p.card)
                .stroke(Stroke::new(1.0, p.card_stroke))
                .corner_radius(crate::widgets::CARD_RADIUS)
                .inner_margin(Margin { left: 20, right: 18, top: 14, bottom: 14 })
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    row_layout(ui, 64.0, |ui| {
                        ui.label(RichText::new(g.to_string()).font(theme::icon(28.0)).color(p.accent_text));
                        ui.add_space(16.0);
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 3.0;
                            ui.label(RichText::new(title).font(theme::semibold(17.0)).color(p.text));
                            ui.add(egui::Label::new(RichText::new(detail).size(13.0).color(p.text2)).wrap());
                        });
                    });
                })
                .response
                .interact(egui::Sense::click());
            // the whole card is the button
            if resp.hovered() {
                ui.painter().rect_stroke(resp.rect, crate::widgets::CARD_RADIUS, Stroke::new(1.0, p.accent), egui::StrokeKind::Inside);
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if resp.clicked() {
                acts.push(go);
            }
        };
        big(
            ui,
            glyph::SAVE,
            "Back up this PC",
            "Before reinstalling: choose what the new Windows gets, and save your settings (and the installers, if you like) into the folder of HeMiMizer.",
            Action::StartWizard(true),
            acts,
        );
        ui.add_space(6.0);
        // with a backup the choices are made already; without one the wizard asks here
        let planned = crate::plan::Plan::load().is_some();
        big(
            ui,
            glyph::PC,
            "Set up this PC",
            if planned {
                "After reinstalling: drivers, programs and settings as chosen in the backup."
            } else {
                "After reinstalling, without a backup: choose drivers and programs, then HeMiMizer installs them and sets up Windows."
            },
            if planned { Action::Go(Phase::Start) } else { Action::StartWizard(false) },
            acts,
        );
        ui.add_space(14.0);
        section(ui, &p, "Tools");
        big(
            ui,
            glyph::USB,
            "Make a Windows USB stick",
            "From a Windows 10 or 11 ISO, for UEFI or BIOS – with your choices for the installation (no TPM check, a local account…).",
            Action::UsbOpen,
            acts,
        );
        ui.add_space(6.0);
        big(
            ui,
            glyph::DISC,
            "Make a smaller Windows",
            "A lighter Windows 11 ISO like Tiny11: without ads, Teams, Xbox, OneDrive… – the simple choice, or choose yourself what goes and what stays.",
            Action::IsoOpen,
            acts,
        );
        ui.add_space(6.0);
        big(
            ui,
            glyph::DOWNLOAD,
            "Download Windows",
            "The ISO of Windows 11 or 10 in your language, straight from Microsoft's servers.",
            Action::DlOpen,
            acts,
        );
    }

    /// A small rounded tile with an icon in the accent colour.
    fn icon_tile(ui: &mut egui::Ui, p: &Palette, g: char, size: f32) {
        let (r, _) = ui.allocate_exact_size(vec2(size, size), egui::Sense::hover());
        ui.painter().rect_filled(r, 7, theme::mix(p.card, p.accent, 0.20));
        ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, g, theme::icon(size * 0.5), p.accent_text);
    }

    /// A card that switches something on or off – the whole card is the switch: an optional icon, the title with
    /// small tags, a line under it and the toggle on the right. Returns true when clicked.
    fn choice_card(ui: &mut egui::Ui, p: &Palette, icon: Option<char>, title: &str, line: &str, tags: &[(String, Color32)], on: bool) -> bool {
        let resp = egui::Frame::new()
            .fill(if on { theme::mix(p.card, p.accent, 0.07) } else { p.card })
            .stroke(Stroke::new(1.0, if on { theme::mix(p.card_stroke, p.accent, 0.40) } else { p.card_stroke }))
            .corner_radius(crate::widgets::CARD_RADIUS)
            .inner_margin(Margin { left: 12, right: 14, top: 7, bottom: 7 })
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                row_layout(ui, 36.0, |ui| {
                    if let Some(g) = icon {
                        Self::icon_tile(ui, p, g, 32.0);
                        ui.add_space(8.0);
                    }
                    let text_w = (ui.available_width() - 52.0).max(60.0);
                    ui.allocate_ui_with_layout(vec2(text_w, 36.0), Layout::top_down(Align::Min), |ui| {
                        ui.spacing_mut().item_spacing.y = 1.0;
                        ui.add_space(if line.is_empty() { 9.0 } else { 0.0 });
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 6.0;
                            ui.add(egui::Label::new(RichText::new(title).size(14.0).color(p.text)).truncate());
                            for (t, c) in tags {
                                ui.label(RichText::new(t.as_str()).size(11.5).color(*c));
                            }
                        });
                        if !line.is_empty() {
                            ui.add(egui::Label::new(RichText::new(line).size(12.0).color(p.text2)).truncate());
                        }
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        crate::widgets::toggle(ui, p, on);
                    });
                });
            })
            .response
            .interact(egui::Sense::click());
        if resp.hovered() {
            ui.painter().rect_stroke(resp.rect, crate::widgets::CARD_RADIUS, Stroke::new(1.0, p.accent), egui::StrokeKind::Inside);
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        resp.clicked()
    }

    /// The head of a section that folds: an arrow, an icon, the title and how many are on; a click folds or unfolds it.
    /// Returns whether it is open.
    fn fold_head(&self, ui: &mut egui::Ui, icon: Option<char>, title: &str, on: usize, all: usize, force_open: bool, acts: &mut Vec<Action>) -> bool {
        let p = self.p;
        let open = force_open || !self.folded.contains(title);
        let resp = ui
            .horizontal(|ui| {
                ui.set_min_height(30.0);
                let arrow = if open { glyph::CHEVRON_DOWN } else { glyph::CHEVRON_RIGHT };
                ui.label(RichText::new(arrow.to_string()).font(theme::icon(10.0)).color(p.text2));
                ui.add_space(2.0);
                if let Some(g) = icon {
                    ui.label(RichText::new(g.to_string()).font(theme::icon(15.0)).color(p.accent_text));
                    ui.add_space(2.0);
                }
                ui.label(RichText::new(title).font(theme::semibold(14.0)).color(p.text));
                ui.add_space(4.0);
                // how many are on, as a small pill
                let text = format!("{on} of {all}");
                let galley = ui.painter().layout_no_wrap(text, theme::body(11.5), if on > 0 { p.on_accent } else { p.text2 });
                let (r, _) = ui.allocate_exact_size(galley.size() + vec2(14.0, 4.0), egui::Sense::hover());
                ui.painter().rect_filled(r, 9, if on > 0 { p.accent } else { theme::mix(p.card, p.text3, 0.25) });
                ui.painter().galley(r.center() - galley.size() / 2.0, galley, p.text);
                ui.allocate_space(vec2(ui.available_width(), 1.0));
            })
            .response
            .interact(egui::Sense::click());
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if resp.clicked() && !force_open {
            acts.push(Action::Fold(title.to_string()));
        }
        open
    }

    /// Cards in two columns (a long list fits on one screen).
    fn pick_grid(&self, ui: &mut egui::Ui, items: &[&'static crate::catalog::Item], icon: Option<char>, acts: &mut Vec<Action>) {
        let p = self.p;
        for pair in items.chunks(2) {
            ui.columns(2, |cols| {
                for (col, item) in cols.iter_mut().zip(pair) {
                    let on = self.plan.has(item.id);
                    if Self::choice_card(col, &p, icon, item.name, item.line, &[], on) {
                        acts.push(Action::Pick(item.id, !on));
                    }
                }
            });
        }
    }

    /// The pages of this wizard: the settings and own folders only before a backup. (The licence terms are accepted
    /// at the setup, when the programs are installed.)
    fn pages(&self) -> Vec<Page> {
        let mut v = vec![Page::Hardware, Page::Browser, Page::Apps, Page::Windows];
        if self.wizard_backup {
            v.push(Page::Settings);
            v.push(Page::Folders);
        }
        v.push(Page::Save);
        v
    }

    fn page_title(&self, page: Page) -> &'static str {
        match page {
            Page::Hardware => "Drivers and the software of the maker",
            Page::Browser => "Browser",
            Page::Apps => "Programs",
            Page::Windows => "What changes in Windows",
            Page::Settings => "Settings to take along",
            Page::Folders => "Your folders",
            Page::Save if self.wizard_backup => "Where to save the backup",
            Page::Save => "Last details",
        }
    }

    /// The name of a page in the steps on top.
    fn page_short(&self, page: Page) -> &'static str {
        match page {
            Page::Hardware => "Drivers",
            Page::Browser => "Browser",
            Page::Apps => "Programs",
            Page::Windows => "Windows",
            Page::Settings => "Take along",
            Page::Folders => "Folders",
            Page::Save if self.wizard_backup => "Save",
            Page::Save => "Finish",
        }
    }

    /// The Wi-Fi networks go into the backup (their passwords: only into a protected one).
    fn wifi_on(&self) -> bool {
        self.backup_items.iter().position(|it| it.id == "wifi").is_some_and(|i| self.backup_on[i] && self.backup_visible("wifi"))
    }

    /// The password of the backup: typed the same twice (or none); the Wi-Fi passwords need one.
    fn seal_ok(&self) -> bool {
        self.seal_pw == self.seal_pw2 && (!self.wifi_on() || !self.seal_pw.is_empty())
    }

    /// A protected backup: the password, then its settings are opened (HeMiMizer starts again with them).
    fn unlock_page(&mut self, ui: &mut egui::Ui, acts: &mut Vec<Action>) {
        let p = self.p;
        let state = self.unlock.lock().unwrap().clone();
        info_bar(ui, &p, Tone::Caution, "This backup is protected with a password. Type it to set up this PC with it.", None);
        ui.add_space(10.0);
        card(ui, &p, |ui| {
            ui.add_space(4.0);
            ui.label(RichText::new("Password of the backup").size(14.0).color(p.text));
            ui.add_space(6.0);
            let r = ui.add_enabled(
                state != Some(None),
                egui::TextEdit::singleline(&mut self.unlock_pw).password(true).desired_width(f32::INFINITY).margin(vec2(8.0, 6.0)),
            );
            if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) && !self.unlock_pw.is_empty() {
                acts.push(Action::Unlock);
            }
            ui.add_space(4.0);
        });
        match &state {
            Some(None) => {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.add(egui::Spinner::new().size(16.0).color(p.accent_text));
                    ui.label(RichText::new("Opening the backup…").size(13.0).color(p.text2));
                });
            }
            Some(Some(Err(e))) => {
                ui.add_space(6.0);
                ui.label(RichText::new(e.as_str()).size(13.0).color(p.critical));
            }
            _ => {}
        }
        ui.add_space(10.0);
        ui.add(
            egui::Label::new(RichText::new("Forgot it? The backup cannot be opened without it – go on without the backup and choose everything again in the wizard.").size(12.0).color(p.text3))
                .wrap(),
        );
    }

    /// Can Next / Back up go on from this page?
    fn page_ok(&self, page: Page) -> bool {
        match page {
            Page::Save => crate::plan::valid_pc_name(&self.plan.pc_name) && (!self.wizard_backup || (!self.save_to.trim().is_empty() && self.seal_ok())),
            _ => true,
        }
    }

    /// The steps of the wizard as a line of circles: done ones with a tick, the current one bigger; a visited page is
    /// one click away.
    fn stepper(&self, ui: &mut egui::Ui, page: Page, acts: &mut Vec<Action>) {
        let p = self.p;
        let pages = self.pages();
        let cur = pages.iter().position(|x| *x == page).unwrap_or(0);
        let n = pages.len();
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 50.0), egui::Sense::hover());
        let step = rect.width() / n as f32;
        let cy = rect.top() + 13.0;
        let x = |i: usize| rect.left() + step * (i as f32 + 0.5);
        for i in 0..n.saturating_sub(1) {
            let c = if i < cur { p.accent } else { p.card_stroke };
            ui.painter().line_segment([pos2(x(i) + 15.0, cy), pos2(x(i + 1) - 15.0, cy)], Stroke::new(2.0, c));
        }
        for (i, pg) in pages.iter().enumerate() {
            let center = pos2(x(i), cy);
            let reach = i <= self.reached && i != cur;
            let hit = egui::Rect::from_center_size(pos2(x(i), rect.top() + 25.0), vec2(step.min(110.0), 50.0));
            let resp = ui.interact(hit, ui.scope_id().with(("wizard-step", i)), if reach { egui::Sense::click() } else { egui::Sense::hover() });
            let hover = reach && resp.hovered();
            let painter = ui.painter();
            if i == cur {
                painter.circle_filled(center, 15.0, theme::mix(p.bg, p.accent, 0.25));
            }
            let fill = if i <= cur { p.accent } else if i <= self.reached { theme::mix(p.bg, p.accent, 0.30) } else { p.bg };
            painter.circle_filled(center, 11.0, fill);
            if i > cur {
                painter.circle_stroke(center, 11.0, Stroke::new(1.5, if hover { p.accent } else { p.card_stroke }));
            }
            if i < cur {
                painter.text(center, egui::Align2::CENTER_CENTER, glyph::CHECK, theme::icon(11.0), p.on_accent);
            } else {
                let c = if i == cur { p.on_accent } else { p.text2 };
                painter.text(center, egui::Align2::CENTER_CENTER, (i + 1).to_string(), theme::semibold(12.0), c);
            }
            let (font, color) = if i == cur { (theme::semibold(12.0), p.text) } else { (theme::body(12.0), if hover { p.accent_text } else { p.text3 }) };
            painter.text(pos2(x(i), rect.top() + 40.0), egui::Align2::CENTER_CENTER, self.page_short(*pg), font, color);
            if hover {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if resp.clicked() && reach {
                acts.push(Action::GoPage(*pg));
            }
        }
        ui.add_space(6.0);
    }

    /// The programs chosen that are games (for the texts that depend on them).
    fn chosen_names(&self, ids: impl Iterator<Item = &'static str>) -> Vec<&'static str> {
        ids.filter(|id| self.plan.has(id)).filter_map(crate::catalog::find).map(|i| i.name).collect()
    }

    fn games_chosen(&self) -> Vec<&'static str> {
        self.chosen_names(crate::catalog::of_kind(crate::catalog::Kind::Games).map(|i| i.id))
    }

    /// The text of a step of Windows by what was chosen to install (else the text of the step).
    fn option_detail(&self, id: &str, base: &'static str) -> String {
        match id {
            "startup" => {
                "OneDrive and the other programs already in the startup list – the new programs start as ticked on the last page; all can be switched back on in Task Manager".into()
            }
            "runtimes" => {
                let games = self.games_chosen();
                if games.is_empty() {
                    "Visual C++, DirectX and .NET – many programs and games need them".into()
                } else {
                    format!("Visual C++, DirectX and .NET – {} and their games need them", games.join(", "))
                }
            }
            "microsoft-apps" if self.plan.has("teams") => format!("{base} – Microsoft Teams stays, you chose it"),
            "xbox-apps" if !self.games_chosen().is_empty() => "The Xbox app and Game Bar – keep them if you play games from the Xbox app or Game Pass".into(),
            _ => base.to_string(),
        }
    }

    /// The tags of a single setting: good for the programs chosen, and how it is on this PC now (before a backup).
    fn tweak_tags(&self, index: usize, id: &str) -> Vec<(String, Color32)> {
        let p = self.p;
        let mut tags = Vec::new();
        if ["games", "game-dvr", "mouse"].contains(&id) && !self.games_chosen().is_empty() {
            tags.push(("Good for games".to_string(), p.accent_text));
        }
        if let Some(Some(now)) = self.this_pc.as_ref().and_then(|v| v.get(index)) {
            tags.push(if *now { ("This PC: on".to_string(), p.success) } else { ("This PC: off".to_string(), p.text3) });
        }
        tags
    }

    /// The ids switched on by a preset of the page Windows: 0 = recommended, 1 = like this PC, 2 = nothing.
    fn preset_ids(&self, preset: u8) -> Vec<String> {
        match preset {
            0 => crate::options::defaults(),
            1 => {
                let mut v = crate::options::defaults();
                if let Some(now) = &self.this_pc {
                    for (t, n) in crate::tweaks::ALL.iter().zip(now) {
                        if let Some(on) = n {
                            v.retain(|x| x != t.id);
                            if *on {
                                v.push(t.id.to_string());
                            }
                        }
                    }
                }
                v
            }
            _ => Vec::new(),
        }
    }

    fn active_preset(&self) -> Option<u8> {
        let mut now = self.plan.windows.clone().unwrap_or_else(crate::options::defaults);
        now.sort();
        let max = if self.wizard_backup { 3 } else { 2 };
        (0..3u8).filter(|i| *i < max || *i == 2).find(|i| {
            if *i == 1 && !self.wizard_backup {
                return false;
            }
            let mut v = self.preset_ids(*i);
            v.sort();
            v == now
        })
    }

    /// The pages of the wizard.
    fn wizard_page(&mut self, ui: &mut egui::Ui, page: Page, acts: &mut Vec<Action>) {
        use crate::catalog::{self, Group};
        let p = self.p;
        // a new plan: the drivers and the software of the maker found on this PC are ticked once the hardware is known
        if self.plan_fresh {
            if let Some(h) = crate::hardware::ready() {
                for id in h.suggested() {
                    self.plan.set(id, true);
                }
                self.plan_fresh = false;
            } else {
                ui.ctx().request_repaint_after(Duration::from_millis(300));
            }
        }
        // before a backup: how the single settings of Windows are on this PC (read once)
        if page == Page::Windows && self.wizard_backup && self.this_pc.is_none() {
            self.this_pc = Some(crate::tweaks::ALL.iter().map(|t| (t.now)()).collect());
        }
        let intro = |ui: &mut egui::Ui, text: &str| {
            ui.add(egui::Label::new(RichText::new(text).size(13.0).color(p.text2)).wrap());
            ui.add_space(10.0);
        };
        match page {
            Page::Hardware => {
                let hw = crate::hardware::ready();
                match hw {
                    Some(h) => {
                        let text = if h.summary.is_empty() { "This PC".to_string() } else { format!("This PC: {}", h.summary) };
                        card(ui, &p, |ui| {
                            row_layout(ui, 36.0, |ui| {
                                Self::icon_tile(ui, &p, glyph::PC, 32.0);
                                ui.add_space(8.0);
                                ui.add(egui::Label::new(RichText::new(text).size(13.0).color(p.text)).wrap());
                            });
                        });
                        ui.add_space(10.0);
                    }
                    None => {
                        ui.horizontal(|ui| {
                            ui.add(egui::Spinner::new().size(14.0).color(p.accent_text));
                            ui.label(RichText::new("Finding out the hardware of this PC…").size(13.0).color(p.text2));
                        });
                        ui.add_space(10.0);
                    }
                }
                section(ui, &p, "Drivers");
                let suggested = hw.map(|h| h.suggested()).unwrap_or_default();
                for item in catalog::of(Group::Driver) {
                    let tags: Vec<(String, Color32)> = if suggested.contains(&item.id) { vec![("Found on this PC".into(), p.accent_text)] } else { Vec::new() };
                    if Self::choice_card(ui, &p, Some(glyph::DRIVER), item.name, item.line, &tags, self.plan.has(item.id)) {
                        acts.push(Action::Pick(item.id, !self.plan.has(item.id)));
                    }
                }
                // the software of the maker of this laptop / PC / motherboard (and what was chosen before)
                let tools = hw.map(|h| h.maker_tools()).unwrap_or_default();
                let shown: Vec<_> = catalog::of(Group::Maker).filter(|i| tools.contains(&i.id) || self.plan.has(i.id)).collect();
                if !shown.is_empty() {
                    ui.add_space(14.0);
                    let laptop = hw.is_some_and(|h| h.laptop);
                    section(ui, &p, if laptop { "Software of the maker of this laptop" } else { "Software of the maker" });
                    for item in shown {
                        let tags: Vec<(String, Color32)> = if tools.contains(&item.id) { vec![("For this PC".into(), p.accent_text)] } else { Vec::new() };
                        if Self::choice_card(ui, &p, Some(glyph::PC), item.name, item.line, &tags, self.plan.has(item.id)) {
                            acts.push(Action::Pick(item.id, !self.plan.has(item.id)));
                        }
                    }
                }
            }
            Page::Browser => {
                intro(ui, "Microsoft Edge stays in Windows either way. Choose one more browser, or none.");
                let none = self.plan.browser().is_none();
                static EDGE: catalog::Item = catalog::Item { id: "", name: "Only Microsoft Edge", line: "No other browser", group: Group::Browser, kind: catalog::Kind::Other, source: catalog::Source::Own, license: "" };
                let all: Vec<&catalog::Item> = std::iter::once(&EDGE).chain(catalog::of(Group::Browser)).collect();
                for pair in all.chunks(2) {
                    ui.columns(2, |cols| {
                        for (col, item) in cols.iter_mut().zip(pair) {
                            let on = if item.id.is_empty() { none } else { self.plan.has(item.id) };
                            if Self::choice_card(col, &p, Some(glyph::GLOBE), item.name, item.line, &[], on) {
                                acts.push(Action::Browser(if item.id.is_empty() || on { None } else { Some(item.id) }));
                            }
                        }
                    });
                }
            }
            Page::Apps => {
                let chosen = self.plan.apps.iter().filter_map(|id| catalog::find(id)).filter(|i| matches!(i.group, Group::App | Group::Hemi)).count();
                ui.horizontal(|ui| {
                    ui.label(RichText::new("The newest versions, installed quietly through winget.").size(13.0).color(p.text2));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(RichText::new(format!("Chosen: {chosen}")).font(theme::semibold(13.0)).color(if chosen > 0 { p.accent_text } else { p.text3 }));
                    });
                });
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(glyph::SEARCH.to_string()).font(theme::icon(14.0)).color(p.text2));
                    ui.add(egui::TextEdit::singleline(&mut self.search).hint_text("Search the programs…").desired_width(f32::INFINITY).margin(vec2(10.0, 7.0)));
                });
                ui.add_space(10.0);
                let q = self.search.trim().to_lowercase();
                let fits = |i: &catalog::Item| q.is_empty() || i.name.to_lowercase().contains(&q) || i.line.to_lowercase().contains(&q);
                let mut any = false;
                for (kind, title) in catalog::KINDS {
                    let items: Vec<&'static catalog::Item> = catalog::of_kind(*kind).filter(|i| fits(i)).collect();
                    if items.is_empty() {
                        continue;
                    }
                    any = true;
                    let on = catalog::of_kind(*kind).filter(|i| self.plan.has(i.id)).count();
                    let g = catalog::kind_glyph(*kind);
                    if self.fold_head(ui, Some(g), title, on, catalog::of_kind(*kind).count(), !q.is_empty(), acts) {
                        let note = catalog::kind_note(*kind);
                        if !note.is_empty() {
                            ui.add(egui::Label::new(RichText::new(note).size(12.0).color(p.text3)).wrap());
                            ui.add_space(4.0);
                        }
                        self.pick_grid(ui, &items, Some(g), acts);
                    }
                    ui.add_space(8.0);
                }
                let hemi: Vec<&'static catalog::Item> = catalog::of(Group::Hemi).filter(|i| fits(i)).collect();
                if !hemi.is_empty() {
                    any = true;
                    let on = hemi.iter().filter(|i| self.plan.has(i.id)).count();
                    if self.fold_head(ui, Some(glyph::HEMI), "HeMi apps", on, hemi.len(), !q.is_empty(), acts) {
                        for item in hemi {
                            if Self::choice_card(ui, &p, Some(glyph::HEMI), item.name, item.line, &[], self.plan.has(item.id)) {
                                acts.push(Action::Pick(item.id, !self.plan.has(item.id)));
                            }
                        }
                    }
                }
                if !any {
                    card(ui, &p, |ui| {
                        row_layout(ui, 32.0, |ui| {
                            ui.label(RichText::new(format!("No program fits “{}”.", self.search.trim())).size(13.0).color(p.text2));
                        });
                    });
                }
            }
            Page::Windows => self.windows_page(ui, acts),
            Page::Settings => {
                intro(ui, "What the new PC gets from this one. Everything is put back in its place after the reinstall.");
                self.backup_page(ui, acts);
            }
            Page::Folders => {
                intro(ui, "Folders of yours to copy into the backup – they are put back to the same place on the new PC. The folders of the profile (Documents, Pictures…) go into the profile of the new user.");
                let mut remove = None;
                for (i, f) in self.plan.folders.iter().enumerate() {
                    card(ui, &p, |ui| {
                        row_layout(ui, 32.0, |ui| {
                            Self::icon_tile(ui, &p, glyph::FOLDER, 28.0);
                            ui.add_space(8.0);
                            ui.label(RichText::new(crate::plan::show_folder(f)).size(14.0).color(p.text));
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if Btn::new("").icon(glyph::CLOSE).subtle().tip("Remove").show(ui, &p).clicked() {
                                    remove = Some(i);
                                }
                            });
                        });
                    });
                }
                if let Some(i) = remove {
                    acts.push(Action::RemoveFolder(i));
                }
                if self.plan.folders.is_empty() {
                    card(ui, &p, |ui| {
                        row_layout(ui, 32.0, |ui| {
                            ui.label(RichText::new("No folders – nothing extra is copied.").size(13.0).color(p.text2));
                        });
                    });
                }
                ui.add_space(8.0);
                if Btn::new("Add a folder…").icon(glyph::FOLDER).show(ui, &p).clicked() {
                    acts.push(Action::AddFolder);
                }
            }
            Page::Save => self.save_page(ui, acts),
        }
    }

    /// The page "What changes in Windows": a starting point (recommended, like this PC, nothing), then the groups
    /// of single changes to fine-tune.
    fn windows_page(&mut self, ui: &mut egui::Ui, acts: &mut Vec<Action>) {
        let p = self.p;
        ui.add(egui::Label::new(RichText::new("Each change on its own. Pick a starting point, then open a group to fine-tune it.").size(13.0).color(p.text2)).wrap());
        ui.add_space(10.0);
        let active = self.active_preset();
        let presets: Vec<(u8, &str, char, &str)> = [
            (0u8, "Recommended", glyph::LIKE, "What most people want: clean-up, privacy and updates on – the look of Windows stays as it comes."),
            (1, "Like this PC", glyph::PC, "Every setting Windows lets HeMiMizer read is set as it is on this PC now – the new Windows looks and behaves the same."),
            (2, "Change nothing", glyph::CLOSE, "Windows stays as it comes – HeMiMizer only installs."),
        ]
        .into_iter()
        .filter(|(i, ..)| *i != 1 || self.wizard_backup)
        .collect();
        ui.columns(presets.len(), |cols| {
            for (col, (i, name, g, _)) in cols.iter_mut().zip(&presets) {
                let on = active == Some(*i);
                let resp = egui::Frame::new()
                    .fill(if on { theme::mix(p.card, p.accent, 0.14) } else { p.card })
                    .stroke(Stroke::new(if on { 1.5 } else { 1.0 }, if on { p.accent } else { p.card_stroke }))
                    .corner_radius(crate::widgets::CARD_RADIUS)
                    .inner_margin(Margin::symmetric(12, 10))
                    .show(col, |ui| {
                        ui.set_width(ui.available_width());
                        ui.vertical_centered(|ui| {
                            ui.label(RichText::new(g.to_string()).font(theme::icon(20.0)).color(if on { p.accent_text } else { p.text2 }));
                            ui.add_space(2.0);
                            ui.label(RichText::new(*name).font(theme::semibold(13.5)).color(p.text));
                        });
                    })
                    .response
                    .interact(egui::Sense::click());
                if resp.hovered() {
                    col.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    if !on {
                        col.painter().rect_stroke(resp.rect, crate::widgets::CARD_RADIUS, Stroke::new(1.0, p.accent), egui::StrokeKind::Inside);
                    }
                }
                if resp.clicked() {
                    acts.push(Action::Preset(*i));
                }
            }
        });
        ui.add_space(6.0);
        let explain = match active {
            Some(i) => presets.iter().find(|x| x.0 == i).map(|x| x.3).unwrap_or(""),
            None => "Your own choice – the groups below show what is on.",
        };
        ui.add(egui::Label::new(RichText::new(explain).size(12.0).color(p.text3)).wrap());
        ui.add_space(12.0);

        // the groups: the steps that are options, the single settings of tweaks.rs
        enum Row {
            Step(&'static str),
            Tweak(usize),
        }
        let steps = |ids: &[&'static str]| ids.iter().map(|id| Row::Step(id)).collect::<Vec<_>>();
        let plan = &self.plan;
        let tweaks_of = |g: crate::tweaks::Group| crate::tweaks::ALL.iter().enumerate().filter(move |(_, t)| t.group == g && crate::tweaks::offered(t.id, plan)).map(|(i, _)| Row::Tweak(i));
        let mut groups: Vec<(&str, char, Vec<Row>)> = vec![("Clean up", glyph::BROOM, steps(&["restore-point", "microsoft-apps", "ad-software", "xbox-apps", "hibernation", "cleanup"]))];
        for (g, title, icon) in crate::tweaks::GROUPS {
            let mut rows: Vec<Row> = tweaks_of(*g).collect();
            if *g == crate::tweaks::Group::System {
                rows.push(Row::Step("startup"));
            }
            groups.push((title, *icon, rows));
        }
        groups.push(("Updates and runtimes", glyph::SYNC, steps(&["windows-update", "store", "runtimes", "no-wu-drivers", "features"])));
        for (title, icon, rows) in groups {
            let id_of = |r: &Row| match r {
                Row::Step(id) => *id,
                Row::Tweak(i) => crate::tweaks::ALL[*i].id,
            };
            let on = rows.iter().filter(|r| self.plan.windows_on(id_of(r))).count();
            if self.fold_head(ui, Some(icon), title, on, rows.len(), false, acts) {
                for r in &rows {
                    let id = id_of(r);
                    let (name, detail, tags) = match r {
                        Row::Step(id) => {
                            let Some(s) = self.steps.iter().find(|s| s.id == *id) else { continue };
                            (s.title.0, self.option_detail(id, s.detail.0), Vec::new())
                        }
                        Row::Tweak(i) => {
                            let t = &crate::tweaks::ALL[*i];
                            (t.title, t.detail.to_string(), self.tweak_tags(*i, t.id))
                        }
                    };
                    let on = self.plan.windows_on(id);
                    if Self::choice_card(ui, &p, None, name, &detail, &tags, on) {
                        acts.push(Action::WinOpt(id, !on));
                    }
                }
                ui.add_space(4.0);
            }
            ui.add_space(4.0);
        }
    }

    /// Shown in the part "Take along" only when the program it belongs to was chosen: (part of config.rs, catalog id).
    const PROGRAM_PARTS: &'static [(&'static str, &'static str)] = &[("vlc", "vlc"), ("qbittorrent", "qbittorrent"), ("brave", "brave"), ("steam", "steam"), ("amd", "amd-gpu"), ("oc", "amd-gpu")];

    /// The item of the backup is offered: the settings of a program only when it was chosen; the installers and the
    /// own folders have pages of their own; HeMiDock is not installed by the public HeMiMizer.
    fn backup_visible(&self, id: &str) -> bool {
        if id == crate::backup::INSTALLERS || id == crate::backup::FOLDERS || id == "hemidock" {
            return false;
        }
        if id == "wifi" {
            return self.has_wifi;
        }
        if let Some((_, app)) = Self::PROGRAM_PARTS.iter().find(|(part, _)| *part == id) {
            return self.plan.has(app);
        }
        if let Some(d) = crate::appdata::find(id) {
            return if d.found_only() { self.found_apps.contains(&id) } else { self.plan.has(id) };
        }
        true
    }

    fn is_program_part(id: &str) -> bool {
        Self::PROGRAM_PARTS.iter().any(|(part, _)| *part == id) || crate::appdata::find(id).is_some()
    }

    /// A line under a part of the backup.
    fn part_detail(id: &str) -> &'static str {
        match id {
            "wallpaper" => "On the desktop and the lock screen",
            "pins" => "Start and the taskbar",
            "defender" => "The protection settings and exclusions",
            "appearance" => "Dark mode and the accent colour",
            "explorer" => "File extensions, hidden files and the view",
            "mouse" => "Pointer speed, scrolling and double-click speed",
            "wifi" => "The saved networks with their passwords – only into a backup with a password",
            "fonts" => "Fonts you installed – for Adobe, DaVinci Resolve, Office…",
            "scaling" => "The size of text and apps, per monitor",
            "steam" => "Where your games are (Defender leaves them out)",
            "vlc" => "Settings and skin",
            "qbittorrent" => "Settings and layout",
            "brave" => "Look, settings and extensions – no history or passwords",
            "amd" => "3D settings and Adrenalin",
            "oc" => "The tuning profile exported in Adrenalin",
            _ => "",
        }
    }

    fn backup_glyph(id: &str) -> char {
        match id {
            "wallpaper" => glyph::PICTURE,
            "pins" => glyph::PIN,
            "defender" => glyph::SHIELD,
            "appearance" => glyph::COLOR,
            "explorer" => glyph::FOLDER,
            "mouse" => glyph::MOUSE,
            "scaling" => glyph::MONITOR,
            "drives" => glyph::DRIVE,
            "wifi" => glyph::GLOBE,
            "fonts" => glyph::COLOR,
            _ => crate::catalog::find(id).map(|i| crate::catalog::kind_glyph(i.kind)).unwrap_or(glyph::SETTINGS),
        }
    }

    /// What the backup takes – before it: Windows and the programs chosen, with switches; while it runs: its state.
    fn backup_page(&mut self, ui: &mut egui::Ui, acts: &mut Vec<Action>) {
        let p = self.p;
        let running = matches!(self.phase, Phase::BackingUp | Phase::BackedUp);
        if running {
            section(ui, &p, tr("What is backed up", "Co se zálohuje"));
            for (i, item) in self.backup_items.iter().enumerate() {
                if !self.backup_on[i] {
                    continue;
                }
                card(ui, &p, |ui| {
                    row_layout(ui, 36.0, |ui| {
                        let state = &self.backup_done[i];
                        match state {
                            Some(Ok(_)) => {
                                ui.label(RichText::new(glyph::COMPLETED.to_string()).font(theme::icon(16.0)).color(p.success));
                            }
                            Some(Err(_)) => {
                                ui.label(RichText::new(glyph::ERROR.to_string()).font(theme::icon(16.0)).color(p.critical));
                            }
                            None if self.backup_now == Some(i) => {
                                ui.add(egui::Spinner::new().size(16.0).color(p.accent_text));
                            }
                            None => {
                                ui.add_space(20.0);
                            }
                        }
                        ui.add_space(6.0);
                        let detail = match state {
                            Some(Ok(t)) | Some(Err(t)) => t.lines().next().unwrap_or("").to_string(),
                            None if self.backup_now == Some(i) => self.backup_log.clone(),
                            None => String::new(),
                        };
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 1.0;
                            ui.add_space(if detail.is_empty() { 8.0 } else { 0.0 });
                            ui.label(RichText::new(item.name).size(14.0).color(p.text));
                            if !detail.is_empty() {
                                let color = if matches!(state, Some(Err(_))) { p.critical } else { p.text2 };
                                ui.add(egui::Label::new(RichText::new(detail).size(12.0).color(color)).wrap());
                            }
                        });
                        // the OC profile cannot be read from Adrenalin: the file exported there is chosen here
                        if item.id == "oc" && matches!(state, Some(Err(_))) {
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if Btn::new("").icon(glyph::OPEN_FILE).subtle().tip("Choose the exported profile (.xml)").show(ui, &p).clicked() {
                                    acts.push(Action::PickOc(i));
                                }
                            });
                        }
                    });
                });
            }
            ui.add_space(6.0);
            let where_ = format!("{} {}", tr("Into", "Do"), config::dir().parent().map(|d| d.display().to_string()).unwrap_or_default());
            ui.label(RichText::new(where_).size(12.0).color(p.text3));
            if let Some(r) = &self.sealed {
                ui.add_space(10.0);
                match r {
                    Ok(t) => info_bar(ui, &p, Tone::Success, &format!("Protected with the password: {t}"), None),
                    Err(e) => info_bar(ui, &p, Tone::Critical, &format!("Not encrypted – the backup is NOT protected: {e}"), None),
                };
            }
            // at the end: one .zip is easier to move than the folder – asked, not done by itself
            if self.phase == Phase::BackedUp {
                ui.add_space(14.0);
                let state = self.pack.lock().unwrap().clone();
                card(ui, &p, |ui| {
                    row_layout(ui, 36.0, |ui| {
                        Self::icon_tile(ui, &p, glyph::SAVE, 32.0);
                        ui.add_space(8.0);
                        let (title, line, color) = match &state {
                            None => ("Pack it into one .zip?", "One file is easier to copy to another drive. HeMiMizer unpacks it by itself when it lies next to it.".to_string(), p.text2),
                            Some(None) => ("Packing…", String::new(), p.text2),
                            Some(Some(Ok(t))) => ("Packed", t.clone(), p.text2),
                            Some(Some(Err(e))) => ("Not packed", e.clone(), p.critical),
                        };
                        let w = (ui.available_width() - 150.0).max(80.0);
                        ui.allocate_ui_with_layout(vec2(w, 36.0), Layout::top_down(Align::Min), |ui| {
                            ui.spacing_mut().item_spacing.y = 1.0;
                            ui.label(RichText::new(title).size(14.0).color(p.text));
                            if !line.is_empty() {
                                ui.add(egui::Label::new(RichText::new(line).size(12.0).color(color)).wrap());
                            }
                        });
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| match &state {
                            Some(None) => {
                                ui.add(egui::Spinner::new().size(16.0).color(p.accent_text));
                            }
                            Some(Some(Ok(_))) => {}
                            _ => {
                                if Btn::new("Pack into .zip").show(ui, &p).clicked() {
                                    acts.push(Action::Pack);
                                }
                            }
                        });
                    });
                });
            }
            return;
        }
        let row = |this: &Self, ui: &mut egui::Ui, i: usize, acts: &mut Vec<Action>| {
            let item = &this.backup_items[i];
            let mut tags: Vec<(String, Color32)> = Vec::new();
            let detail = if item.id == "drives" {
                if this.found_drives.is_empty() {
                    "No network drives are mapped on this PC".to_string()
                } else {
                    let letters: Vec<String> = this.found_drives.iter().map(|(l, _)| format!("{l}:")).collect();
                    format!("All drives mapped on this PC ({}) – the passwords are asked on the new PC", letters.join(", "))
                }
            } else if let Some(d) = crate::appdata::find(item.id) {
                if d.found_only() {
                    tags.push(("Found on this PC".into(), p.accent_text));
                } else if !crate::appdata::present(item.id) {
                    tags.push(("Not on this PC".into(), p.text3));
                }
                d.what.to_string()
            } else {
                Self::part_detail(item.id).to_string()
            };
            if Self::choice_card(ui, &p, Some(Self::backup_glyph(item.id)), item.name, &detail, &tags, this.backup_on[i]) {
                acts.push(Action::ToggleBackup(i));
            }
        };
        let windows: Vec<usize> = (0..self.backup_items.len()).filter(|&i| self.backup_visible(self.backup_items[i].id) && !Self::is_program_part(self.backup_items[i].id)).collect();
        let programs: Vec<usize> = (0..self.backup_items.len()).filter(|&i| self.backup_visible(self.backup_items[i].id) && Self::is_program_part(self.backup_items[i].id)).collect();
        let on = |v: &[usize]| v.iter().filter(|&&i| self.backup_on[i]).count();
        if self.fold_head(ui, Some(glyph::WINDOWS), "Windows", on(&windows), windows.len(), false, acts) {
            for i in &windows {
                row(self, ui, *i, acts);
            }
        }
        ui.add_space(8.0);
        if self.fold_head(ui, Some(glyph::APPS), "Your programs", on(&programs), programs.len(), false, acts) {
            if programs.is_empty() {
                card(ui, &p, |ui| {
                    ui.add(
                        egui::Label::new(
                            RichText::new("None of the programs you chose keeps settings HeMiMizer can take along – browsers, chat and game launchers bring them back when you sign in.")
                                .size(13.0)
                                .color(p.text2),
                        )
                        .wrap(),
                    );
                });
            }
            for i in &programs {
                row(self, ui, *i, acts);
            }
            // the programs chosen whose settings stay in their account
            let rest = crate::appdata::not_covered(&self.plan.apps);
            if !rest.is_empty() && !programs.is_empty() {
                ui.add_space(2.0);
                ui.add(
                    egui::Label::new(RichText::new(format!("{} – their settings come back when you sign in, or they have none to take along.", rest.join(", "))).size(12.0).color(p.text3))
                        .wrap(),
                );
            }
        }
    }

    /// A tile of the summary: an icon, a big number, what it counts.
    fn tile(ui: &mut egui::Ui, p: &Palette, g: char, n: usize, label: &str) {
        egui::Frame::new()
            .fill(p.card)
            .stroke(Stroke::new(1.0, p.card_stroke))
            .corner_radius(crate::widgets::CARD_RADIUS)
            .inner_margin(Margin::symmetric(12, 10))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    Self::icon_tile(ui, p, g, 30.0);
                    ui.add_space(6.0);
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        ui.label(RichText::new(n.to_string()).font(theme::display(22.0)).color(p.text));
                        ui.label(RichText::new(label).size(12.0).color(p.text2));
                    });
                });
            });
    }

    /// The last page: a summary, where the backup goes (before a backup), the name of the PC, the list to install.
    fn save_page(&mut self, ui: &mut egui::Ui, acts: &mut Vec<Action>) {
        use crate::catalog::{self, Group};
        let p = self.p;
        let items: Vec<&'static catalog::Item> = self.plan.apps.iter().filter_map(|id| catalog::find(id)).collect();
        let drivers = items.iter().filter(|i| matches!(i.group, Group::Driver | Group::Maker)).count();
        let programs = items.len() - drivers;
        let effective = self.plan.windows.clone().unwrap_or_else(crate::options::defaults);
        let changes = effective.iter().filter(|id| crate::options::is_option(id) || crate::tweaks::find(id).is_some()).count();
        let along = (0..self.backup_items.len()).filter(|&i| self.backup_on[i] && self.backup_visible(self.backup_items[i].id)).count();
        let mut tiles: Vec<(char, usize, &str)> = vec![(glyph::DRIVER, drivers, "Drivers and tools"), (glyph::APPS, programs, "Programs"), (glyph::WINDOWS, changes, "Changes in Windows")];
        if self.wizard_backup {
            tiles.push((glyph::SAVE, along + usize::from(!self.plan.folders.is_empty()), "Taken along"));
        }
        ui.columns(tiles.len(), |cols| {
            for (col, (g, n, label)) in cols.iter_mut().zip(&tiles) {
                Self::tile(col, &p, *g, *n, label);
            }
        });
        ui.add_space(14.0);
        if self.wizard_backup {
            section(ui, &p, "Save to");
            card(ui, &p, |ui| {
                ui.add(egui::Label::new(RichText::new("The backup and HeMiMizer itself go into this folder – a USB stick or a network drive is best. Start HeMiMizer from there after reinstalling.").size(12.0).color(p.text2)).wrap());
                ui.add_space(6.0);
                if Self::path_row(ui, &p, &mut self.save_to, "Folder") {
                    acts.push(Action::BrowseSaveTo);
                }
            });
            ui.add_space(14.0);
            section(ui, &p, "Installers");
            if Self::choice_card(ui, &p, Some(glyph::DOWNLOAD), "Download all installers now", "The new PC then installs everything without the internet (needs more space)", &[], self.plan.offline) {
                acts.push(Action::Offline(!self.plan.offline));
            }
            ui.add_space(14.0);
            section(ui, &p, "Password");
            card(ui, &p, |ui| {
                ui.add(
                    egui::Label::new(
                        RichText::new("Encrypts your settings, Wi-Fi passwords, the settings of programs and your folders (AES-256). HeMiMizer asks for it on the new PC. Empty: not protected.")
                            .size(12.0)
                            .color(p.text2),
                    )
                    .wrap(),
                );
                ui.add_space(6.0);
                ui.add(egui::TextEdit::singleline(&mut self.seal_pw).password(true).hint_text("Password").desired_width(f32::INFINITY).margin(vec2(8.0, 6.0)));
                ui.add_space(4.0);
                ui.add(egui::TextEdit::singleline(&mut self.seal_pw2).password(true).hint_text("The same once more").desired_width(f32::INFINITY).margin(vec2(8.0, 6.0)));
            });
            if self.seal_pw != self.seal_pw2 && !self.seal_pw2.is_empty() {
                ui.label(RichText::new("The two passwords are not the same.").size(12.0).color(p.critical));
            } else if self.wifi_on() && self.seal_pw.is_empty() {
                ui.label(RichText::new("The Wi-Fi passwords go only into a protected backup – set a password, or leave out Wi-Fi networks on the page Take along.").size(12.0).color(p.caution));
            } else if !self.seal_pw.is_empty() {
                ui.label(RichText::new("Keep the password safe – without it the backup cannot be opened.").size(12.0).color(p.text3));
            }
            ui.add_space(14.0);
        }
        section(ui, &p, "Name of the PC");
        card(ui, &p, |ui| {
            row_layout(ui, 36.0, |ui| {
                ui.vertical(|ui| {
                    ui.label(RichText::new("New name").size(14.0).color(p.text));
                    ui.label(RichText::new("Empty: Windows keeps its name. Letters, digits and hyphens, at most 15.").size(12.0).color(p.text2));
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.plan.pc_name).hint_text("e.g. HOME-PC").desired_width(200.0).margin(vec2(8.0, 6.0)));
                });
            });
        });
        if !crate::plan::valid_pc_name(&self.plan.pc_name) {
            ui.label(RichText::new("This name cannot be used.").size(12.0).color(p.critical));
        }
        ui.add_space(14.0);
        section(ui, &p, "To install");
        if items.is_empty() {
            card(ui, &p, |ui| {
                ui.label(RichText::new("Nothing – only the settings of Windows.").size(13.0).color(p.text2));
            });
        } else {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
                for i in &items {
                    egui::Frame::new().fill(p.card).stroke(Stroke::new(1.0, p.card_stroke)).corner_radius(12).inner_margin(Margin::symmetric(10, 4)).show(ui, |ui| {
                        ui.label(RichText::new(i.name).size(12.5).color(p.text));
                    });
                }
            });
        }
        if !self.plan.licensed().is_empty() {
            ui.add_space(8.0);
            let names: Vec<&str> = self.plan.licensed().iter().map(|i| i.name).collect();
            ui.add(egui::Label::new(RichText::new(format!("Licence terms of {} are shown at the setup, before they are installed.", names.join(", "))).size(12.0).color(p.text3)).wrap());
        }
        // where each new program goes: only what is ticked here is done
        let progs = crate::shortcuts::programs(&self.plan);
        if !progs.is_empty() {
            ui.add_space(14.0);
            section(ui, &p, "Shortcuts and start");
            ui.add(
                egui::Label::new(
                    RichText::new("Where the new programs go. Desktop not ticked: the shortcut its installer puts there is removed. Start with Windows not ticked: it does not start by itself.")
                        .size(12.0)
                        .color(p.text2),
                )
                .wrap(),
            );
            ui.add_space(6.0);
            let plan = &mut self.plan;
            card(ui, &p, |ui| {
                egui::Grid::new("shortcuts").num_columns(5).spacing(vec2(22.0, 8.0)).min_col_width(64.0).show(ui, |ui| {
                    ui.label(RichText::new("Program").size(12.0).color(p.text2));
                    for h in ["Desktop", "Start", "Taskbar", "With Windows"] {
                        ui.label(RichText::new(h).size(12.0).color(p.text2));
                    }
                    ui.end_row();
                    for item in &progs {
                        ui.label(RichText::new(item.name).size(13.0).color(p.text));
                        for list in [&mut plan.desktop, &mut plan.start_pins, &mut plan.taskbar] {
                            let on = crate::plan::in_list(list, item.id);
                            if Self::check(ui, &p, on) {
                                crate::plan::set_in_list(list, item.id, !on);
                            }
                        }
                        if crate::shortcuts::can_start(item.id) {
                            let on = crate::plan::in_list(&plan.startup, item.id);
                            if Self::check(ui, &p, on) {
                                crate::plan::set_in_list(&mut plan.startup, item.id, !on);
                            }
                        } else {
                            ui.label(RichText::new("–").size(13.0).color(p.text3)).on_hover_text("It does not start with Windows by itself");
                        }
                        ui.end_row();
                    }
                });
            });
        }
    }

    /// A small tick box (true when clicked).
    fn check(ui: &mut egui::Ui, p: &Palette, on: bool) -> bool {
        let (rect, resp) = ui.allocate_exact_size(vec2(20.0, 20.0), egui::Sense::click());
        let fill = if on { p.accent } else { p.card };
        let stroke = if on { p.accent } else if resp.hovered() { p.text2 } else { p.text3 };
        ui.painter().rect(rect, 4.0, fill, Stroke::new(1.0, stroke), egui::StrokeKind::Inside);
        if on {
            ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, glyph::CHECK.to_string(), theme::icon(12.0), p.on_accent);
        }
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        resp.clicked()
    }

    /// Before the run: the licence terms of the programs, the installers of the drivers, the password of the NAS, Run.
    fn start_page(&mut self, ui: &mut egui::Ui, acts: &mut Vec<Action>) {
        let p = self.p;
        let (tone, text) = if self.offline > 0 {
            (Tone::Success, tr("Installers from the backup: {n} – programs are installed offline.", "Instalačky ze zálohy: {n} – programy se nainstalují offline.").replace("{n}", &self.offline.to_string()))
        } else {
            (Tone::Caution, tr("No installers from a backup – the programs are downloaded after the internet is connected.", "Žádné instalačky ze zálohy – programy se stáhnou po připojení k internetu.").to_string())
        };
        info_bar(ui, &p, tone, &text, None);
        ui.add_space(10.0);
        // the overview: what the run will do, step by step (nothing else is done)
        if self.overview.as_ref().is_none_or(|(plan, _)| *plan != self.plan) {
            let todo = self.chosen.iter().copied().filter(|&i| steps::applies(self.steps[i].id)).collect();
            self.overview = Some((self.plan.clone(), todo));
        }
        let todo = self.overview.as_ref().map(|(_, t)| t.clone()).unwrap_or_default();
        if self.fold_head(ui, Some(glyph::INFO), "What HeMiMizer will do", todo.len(), todo.len(), false, acts) {
            card(ui, &p, |ui| {
                for (n, &i) in todo.iter().enumerate() {
                    let s = &self.steps[i];
                    ui.horizontal_top(|ui| {
                        ui.add_sized(vec2(22.0, 16.0), egui::Label::new(RichText::new(format!("{}.", n + 1)).size(12.5).color(p.text3)));
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 0.0;
                            ui.label(RichText::new(s.title()).size(13.0).color(p.text));
                            ui.add(egui::Label::new(RichText::new(tr(s.detail.0, s.detail.1)).size(11.5).color(p.text2)).wrap());
                        });
                    });
                    ui.add_space(3.0);
                }
            });
            ui.add_space(10.0);
        }
        let licensed = self.plan.licensed();
        if !licensed.is_empty() {
            section(ui, &p, "Licence terms");
            ui.add(egui::Label::new(RichText::new("These programs have their own licence terms. HeMiMizer installs them only when you accept them – or leave them out.").size(12.0).color(p.text2)).wrap());
            ui.add_space(6.0);
            for item in &licensed {
                card(ui, &p, |ui| {
                    row_layout(ui, 30.0, |ui| {
                        ui.label(RichText::new(item.name).size(14.0).color(p.text));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if Btn::new("Licence terms").icon(glyph::OPEN_LINK).subtle().show(ui, &p).clicked() {
                                acts.push(Action::Open(item.license.to_string()));
                            }
                        });
                    });
                });
            }
            ui.add_space(6.0);
            if Self::choice_card(ui, &p, None, "I accept the licence terms of all programs above", "Needed to install them", &[], self.plan.accepted) {
                acts.push(Action::Accept(!self.plan.accepted));
            }
            ui.add_space(4.0);
            if !self.plan.accepted && Btn::new("Leave them out").show(ui, &p).clicked() {
                acts.push(Action::DropLicensed);
            }
            ui.add_space(14.0);
        }
        if self.plan.has("amd-chipset") || self.plan.has("amd-gpu") || !self.driver_paths.iter().all(|d| d.is_empty()) {
            section(ui, &p, tr("AMD drivers (offline, right after the network drives)", "Ovladače AMD (offline, hned po síťových discích)"));
            let labels = [tr("AMD chipset driver", "Ovladač čipsetu AMD"), tr("AMD graphics driver (Adrenalin)", "Ovladač grafiky AMD (Adrenalin)")];
            for (i, label) in labels.iter().enumerate() {
                card(ui, &p, |ui| {
                    ui.add_space(4.0);
                    ui.label(RichText::new(*label).font(theme::semibold(14.0)).color(p.text));
                    ui.add_space(8.0);
                    if Self::path_row(ui, &p, &mut self.driver_paths[i], tr("Empty = skipped", "Prázdné = přeskočit")) {
                        acts.push(Action::BrowseDriver(i));
                    }
                    ui.add_space(4.0);
                });
            }
            ui.add_space(14.0);
        }
        if !self.passwords.is_empty() {
            section(ui, &p, tr("Password for the network drives", "Heslo k síťovým diskům"));
            for (server, user, pw) in self.passwords.iter_mut() {
                let saved = programs::saved_user(server).is_some();
                card(ui, &p, |ui| {
                    ui.add_space(4.0);
                    ui.label(RichText::new(server.as_str()).font(theme::semibold(14.0)).color(p.text));
                    ui.add_space(8.0);
                    ui.add(egui::TextEdit::singleline(user).hint_text(tr("User", "Uživatel")).desired_width(f32::INFINITY).margin(vec2(8.0, 6.0)));
                    ui.add_space(6.0);
                    let hint = if saved { tr("Saved in Windows – may stay empty", "Uložené ve Windows – může zůstat prázdné") } else { tr("Password", "Heslo") };
                    let r = ui.add(egui::TextEdit::singleline(pw).password(true).hint_text(hint).desired_width(f32::INFINITY).margin(vec2(8.0, 6.0)));
                    if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        acts.push(Action::Start);
                    }
                    ui.add_space(4.0);
                });
            }
            ui.add_space(4.0);
            ui.label(
                RichText::new(tr(
                    "Windows saves it in Credential Manager; HeMiMizer does not keep it.",
                    "Windows ho uloží do Správce přihlašovacích údajů; HeMiMizer si ho nenechává.",
                ))
                .size(12.0)
                .color(p.text3),
            );
        }
    }

    /// Run can start: no licence terms to accept, or accepted.
    fn licences_ok(&self) -> bool {
        self.plan.licensed().is_empty() || self.plan.accepted
    }

    /// Downloads that failed: choose the files by hand, try again, or go on.
    fn need_files_page(&mut self, ui: &mut egui::Ui, acts: &mut Vec<Action>) {
        let p = self.p;
        let Some(list) = self.need_files.as_mut() else { return };
        info_bar(
            ui,
            &p,
            Tone::Caution,
            tr("These installers could not be downloaded.", "Tyto instalačky se nepodařilo stáhnout."),
            None,
        );
        ui.add_space(8.0);
        for (i, (_, name, err, path)) in list.iter_mut().enumerate() {
            card(ui, &p, |ui| {
                ui.add_space(4.0);
                ui.label(RichText::new(name.as_str()).font(theme::semibold(14.0)).color(p.text));
                ui.add(egui::Label::new(RichText::new(err.as_str()).size(12.0).color(p.text2)).wrap());
                ui.add_space(6.0);
                if Self::path_row(ui, &p, path, tr("The installer downloaded by hand", "Instalačka stažená ručně")) {
                    acts.push(Action::BrowseFile(i));
                }
                ui.add_space(4.0);
            });
        }
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if Btn::new(tr("Try again", "Zkusit znovu")).icon(glyph::RESTART).show(ui, &p).clicked() {
                acts.push(Action::RetryDownloads);
            }
            let t = tr("Continue (without the empty ones)", "Pokračovat (bez prázdných)");
            if Btn::new(t).icon(glyph::PLAY).accent().show(ui, &p).clicked() {
                acts.push(Action::ContinueDownloads);
            }
        });
        ui.add_space(14.0);
    }

    /// During the run: the step, out of how many, what it does; what failed so far.
    fn progress_page(&self, ui: &mut egui::Ui) {
        let p = self.p;
        let total = self.chosen.len().max(1);
        let n = (self.done + 1).min(total);
        card(ui, &p, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.add(egui::Spinner::new().size(18.0).color(p.accent_text));
                ui.add_space(6.0);
                let head = tr("Step {i} of {n}", "Krok {i} z {n}").replace("{i}", &n.to_string()).replace("{n}", &total.to_string());
                ui.label(RichText::new(head).font(theme::semibold(14.0)).color(p.text2));
            });
            ui.add_space(6.0);
            let title = self.current.map(|c| self.steps[c].title()).unwrap_or(tr("Starting…", "Začínám…"));
            ui.label(RichText::new(title).font(theme::display(22.0)).color(p.text));
            ui.add_space(4.0);
            let what = if self.log_line.is_empty() { self.current.map(|c| self.steps[c].detail()).unwrap_or("").to_string() } else { format!("{}…", self.log_line) };
            ui.add(egui::Label::new(RichText::new(what).size(13.0).color(p.text2)).wrap());
            ui.add_space(12.0);
            ui.add(egui::ProgressBar::new(self.done as f32 / total as f32).desired_height(6.0).fill(p.accent).corner_radius(3));
            ui.add_space(6.0);
        });
        self.failure_list(ui);
    }

    fn failure_list(&self, ui: &mut egui::Ui) {
        let p = self.p;
        if self.failures.is_empty() {
            return;
        }
        ui.add_space(14.0);
        section(ui, &p, tr("Failed", "Selhalo"));
        for (title, err) in &self.failures {
            card(ui, &p, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(glyph::ERROR.to_string()).font(theme::icon(16.0)).color(p.critical));
                    ui.add_space(4.0);
                    ui.label(RichText::new(title).font(theme::semibold(14.0)).color(p.text));
                });
                ui.add(egui::Label::new(RichText::new(err).size(12.0).color(p.text2)).wrap());
            });
        }
    }

    /// After the run: how it went, what failed, what is left to do by hand.
    fn result_page(&self, ui: &mut egui::Ui, acts: &mut Vec<Action>) {
        let p = self.p;
        let total = self.chosen.len();
        let (tone, text) = if self.restart_at.is_some() {
            (
                Tone::Caution,
                tr(
                    "The drivers are installed – the PC restarts and HeMiMizer continues after the sign-in.",
                    "Ovladače jsou nainstalované – PC se restartuje a HeMiMizer po přihlášení pokračuje.",
                )
                .to_string(),
            )
        } else if self.failures.is_empty() {
            (Tone::Success, tr("Done – all {n} steps went well.", "Hotovo – všech {n} kroků proběhlo v pořádku.").replace("{n}", &total.to_string()))
        } else {
            (
                Tone::Caution,
                tr("Done – {f} of {n} steps failed.", "Hotovo – {f} z {n} kroků selhalo.")
                    .replace("{f}", &self.failures.len().to_string())
                    .replace("{n}", &total.to_string()),
            )
        };
        info_bar(ui, &p, tone, &text, None);
        self.failure_list(ui);
        if self.restart_at.is_some() {
            return;
        }
        ui.add_space(8.0);
        let bye = if steps::removes_data() {
            "When you press Done, HeMiMizer removes itself – nothing of it stays on this PC (HeMiMizer.exe goes at the next restart). Note what is left to do below first."
        } else {
            "When you press Done, HeMiMizer removes everything it put on this PC. It stays on its drive with the backup, for the next time."
        };
        ui.add(egui::Label::new(RichText::new(bye).size(12.0).color(p.text3)).wrap());
        ui.add_space(14.0);
        section(ui, &p, tr("Left to do by hand", "Zbývá udělat ručně"));
        // the default apps of the programs that were installed
        let apps = crate::catalog::default_apps(&self.plan);
        if !apps.is_empty() {
            card(ui, &p, |ui| {
                two_lines(ui, &p, "Default apps", "Windows does not let programs choose them – open each one and click Set default.");
                ui.add_space(6.0);
                ui.horizontal_wrapped(|ui| {
                    for app in apps {
                        if Btn::new(app).icon(glyph::OPEN_LINK).show(ui, &p).clicked() {
                            acts.push(Action::DefaultApps(app));
                        }
                    }
                });
            });
        }
        for n in &self.notes {
            card(ui, &p, |ui| {
                row_layout(ui, 44.0, |ui| {
                    ui.label(RichText::new(glyph::INFO.to_string()).font(theme::icon(16.0)).color(p.accent_text));
                    ui.add_space(6.0);
                    ui.add(egui::Label::new(RichText::new(n).size(13.0).color(p.text)).wrap());
                });
            });
        }
    }

    fn footer(&self, ui: &mut egui::Ui, acts: &mut Vec<Action>) {
        let p = self.p;
        if self.waiting_internet && self.phase == Phase::Running {
            egui::Frame::new()
                .fill(p.caution_bg)
                .stroke(Stroke::new(1.0, p.caution))
                .corner_radius(8)
                .inner_margin(Margin { left: 14, right: 14, top: 10, bottom: 10 })
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    row_layout(ui, 40.0, |ui| {
                        ui.add(egui::Spinner::new().size(20.0).color(p.caution));
                        ui.add_space(8.0);
                        two_lines(
                            ui,
                            &p,
                            tr("Connect the PC to the internet", "Připoj PC k internetu"),
                            tr("The drivers are installed. HeMiMizer goes on by itself as soon as the PC is online.", "Ovladače jsou nainstalované. HeMiMizer sám pokračuje, jakmile bude PC online."),
                        );
                    });
                });
            ui.add_space(8.0);
        }
        if let Some(at) = self.restart_at {
            let left = at.saturating_duration_since(Instant::now()).as_secs();
            egui::Frame::new()
                .fill(p.caution_bg)
                .stroke(Stroke::new(1.0, p.card_stroke))
                .corner_radius(8)
                .inner_margin(Margin { left: 14, right: 8, top: 6, bottom: 6 })
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    row_layout(ui, 32.0, |ui| {
                        ui.label(RichText::new(glyph::RESTART.to_string()).font(theme::icon(16.0)).color(p.caution));
                        ui.add_space(4.0);
                        let t = tr("The PC restarts in {s} s, then HeMiMizer continues.", "PC se restartuje za {s} s, pak HeMiMizer pokračuje.").replace("{s}", &left.to_string());
                        ui.label(RichText::new(t).size(13.0).color(p.text));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if Btn::new(tr("Not now", "Teď ne")).show(ui, &p).clicked() {
                                acts.push(Action::CancelRestart);
                            }
                            if Btn::new(tr("Restart now", "Restartovat hned")).accent().show(ui, &p).clicked() {
                                acts.push(Action::RestartNow);
                            }
                        });
                    });
                });
            ui.add_space(8.0);
        }
        ui.horizontal(|ui| {
            ui.set_min_height(32.0);
            // the log exists only in Debug builds
            if cfg!(debug_assertions)
                && matches!(self.phase, Phase::Running | Phase::Finished)
                && Btn::new(tr("Log", "Záznam")).subtle().tip(tr("What HeMiMizer did, step by step", "Co HeMiMizer dělal, krok po kroku")).show(ui, &p).clicked()
            {
                acts.push(Action::OpenLog);
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| match self.phase {
                Phase::Download => {
                    if Btn::new("Download").icon(glyph::DOWNLOAD).accent().enabled(self.dl.ready()).show(ui, &p).clicked() {
                        acts.push(Action::DlStart);
                    }
                }
                Phase::Iso => {
                    if Btn::new("Make the ISO").icon(glyph::DISC).accent().enabled(self.iso.ready()).show(ui, &p).clicked() {
                        acts.push(Action::IsoMake);
                    }
                }
                Phase::Usb => {
                    if Btn::new("Make the stick").icon(glyph::USB).accent().enabled(self.usb.ready()).show(ui, &p).clicked() {
                        acts.push(Action::UsbAsk);
                    }
                }
                Phase::Unlock => {
                    let busy = self.unlock.lock().unwrap().as_ref().is_some_and(|s| s.is_none());
                    if Btn::new("Open").accent().enabled(!busy && !self.unlock_pw.is_empty()).show(ui, &p).clicked() {
                        acts.push(Action::Unlock);
                    }
                    if Btn::new(tr("Close", "Zavřít")).show(ui, &p).clicked() {
                        acts.push(Action::Quit);
                    }
                    if Btn::new("Without the backup").enabled(!busy).tip("Choose everything again in the wizard").show(ui, &p).clicked() {
                        acts.push(Action::Go(Phase::Choose));
                    }
                }
                Phase::Choose => {
                    if Btn::new(tr("Close", "Zavřít")).show(ui, &p).clicked() {
                        acts.push(Action::Quit);
                    }
                }
                Phase::Wizard(page) => {
                    let ok = self.page_ok(page);
                    // the last page of the backup: Back up; else Next
                    if page == Page::Save && self.wizard_backup {
                        if Btn::new(tr("Back up", "Zálohovat")).icon(glyph::SAVE).accent().enabled(ok).show(ui, &p).clicked() {
                            acts.push(Action::RunBackup);
                        }
                    } else if Btn::new("Next").accent().enabled(ok).show(ui, &p).clicked() {
                        acts.push(Action::Next);
                    }
                }
                Phase::BackedUp => {
                    if Btn::new(tr("Close", "Zavřít")).accent().show(ui, &p).clicked() {
                        acts.push(Action::Quit);
                    }
                    if Btn::new(tr("Open the folder", "Otevřít složku")).icon(glyph::FOLDER).show(ui, &p).clicked() {
                        acts.push(Action::OpenFolder);
                    }
                }
                Phase::Start => {
                    if Btn::new(tr("Run", "Spustit")).icon(glyph::PLAY).accent().enabled(self.licences_ok()).show(ui, &p).clicked() {
                        acts.push(Action::Start);
                    }
                    if Btn::new(tr("Close", "Zavřít")).show(ui, &p).clicked() {
                        acts.push(Action::Quit);
                    }
                }
                // the end: Done closes HeMiMizer and removes it (nothing of it stays); Try again runs the failed steps once more
                Phase::Finished if self.restart_at.is_none() && !self.resume_pending => {
                    if Btn::new("Done").icon(glyph::COMPLETED).accent().tip("Closes HeMiMizer and removes it – nothing of it stays on the PC").show(ui, &p).clicked() {
                        acts.push(Action::Done);
                    }
                    if !self.failed_steps.is_empty() && Btn::new("Try again").icon(glyph::RESTART).tip("Runs only the steps that failed once more").show(ui, &p).clicked() {
                        acts.push(Action::Retry);
                    }
                    if self.reboot && Btn::new("Restart PC").icon(glyph::RESTART).show(ui, &p).clicked() {
                        acts.push(Action::AskReboot);
                    }
                }
                _ => {}
            });
        });
    }

    fn dialog(&mut self, ctx: &egui::Context) {
        let Some(Dialog::Reboot) = self.dialog.clone() else { return };
        let p = self.p;
        let mut choice: Option<bool> = None;
        let frame = egui::Frame::new().fill(p.dialog).stroke(Stroke::new(1.0, p.card_stroke)).corner_radius(8).shadow(ctx.global_style().visuals.window_shadow);
        let modal = egui::Modal::new(Id::unique("dialog")).backdrop_color(p.smoke).frame(frame).show(ctx, |ui| {
            let width = 460.0;
            ui.set_width(width);
            ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
            egui::Frame::new().inner_margin(Margin { left: 24, right: 24, top: 22, bottom: 24 }).show(ui, |ui| {
                ui.set_width(width - 48.0);
                ui.label(RichText::new(tr("Restart the PC?", "Restartovat PC?")).font(theme::semibold(20.0)).color(p.text));
                ui.add_space(12.0);
                let t = tr(
                    "Some changes (the name of the PC, Windows features, scaling, memory integrity) take effect after a restart.",
                    "Některé změny (název PC, funkce Windows, měřítko, integrita paměti) se projeví až po restartu.",
                );
                ui.add(egui::Label::new(RichText::new(t).size(14.0).color(p.text)).wrap());
            });
            egui::Frame::new().fill(p.dialog_foot).corner_radius(CornerRadius { nw: 0, ne: 0, sw: 8, se: 8 }).inner_margin(Margin::same(24)).show(ui, |ui| {
                ui.set_width(width - 48.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    let half = (width - 48.0 - 8.0) / 2.0;
                    if Btn::new(tr("Restart", "Restartovat")).accent().width(half).show(ui, &p).clicked() {
                        choice = Some(true);
                    }
                    if Btn::new(tr("Cancel", "Zrušit")).width(half).show(ui, &p).clicked() {
                        choice = Some(false);
                    }
                });
            });
        });
        if modal.should_close() {
            choice = choice.or(Some(false));
        }
        match choice {
            Some(true) => {
                self.dialog = None;
                let _ = sys::exec("shutdown.exe", &["/r", "/t", "0"]);
            }
            Some(false) => self.dialog = None,
            None => {}
        }
    }

    fn act(&mut self, ctx: &egui::Context, acts: Vec<Action>) {
        for a in acts {
            match a {
                Action::Go(phase) => {
                    self.phase = phase;
                    if phase == Phase::Start {
                        self.start_via_wizard = false;
                    }
                }
                Action::StartWizard(backup) => {
                    self.wizard_backup = backup;
                    self.phase = Phase::Wizard(Page::Hardware);
                    self.reached = 0;
                    self.search.clear();
                    if backup {
                        self.backup_done.iter_mut().for_each(|d| *d = None);
                        // the network drives found now: on when there are some, off when none
                        self.found_drives = config::mapped_drives();
                        if let Some(i) = self.backup_items.iter().position(|it| it.id == "drives") {
                            self.backup_on[i] = !self.found_drives.is_empty();
                        }
                    }
                }
                Action::Next => {
                    if let Phase::Wizard(page) = self.phase {
                        let pages = self.pages();
                        let i = pages.iter().position(|x| *x == page).unwrap_or(0);
                        if i + 1 < pages.len() {
                            self.phase = Phase::Wizard(pages[i + 1]);
                            self.reached = self.reached.max(i + 1);
                        } else {
                            // the end of the wizard on the new PC: the choices for this run, then the start page
                            if let Err(e) = self.plan.save() {
                                sys::log(&format!("plan.json: {e}"));
                            }
                            self.start_via_wizard = true;
                            self.phase = Phase::Start;
                        }
                    }
                }
                Action::Prev => {
                    self.phase = match self.phase {
                        Phase::Wizard(page) => {
                            let pages = self.pages();
                            match pages.iter().position(|x| *x == page).unwrap_or(0) {
                                0 => Phase::Choose,
                                i => Phase::Wizard(pages[i - 1]),
                            }
                        }
                        Phase::Start if self.start_via_wizard => Phase::Wizard(Page::Save),
                        _ => Phase::Choose,
                    };
                }
                Action::GoPage(page) => self.phase = Phase::Wizard(page),
                Action::Fold(title) => {
                    if !self.folded.remove(&title) {
                        self.folded.insert(title);
                    }
                }
                Action::Preset(i) => {
                    self.plan.windows = if i == 0 { None } else { Some(self.preset_ids(i)) };
                }
                Action::DropLicensed => {
                    self.plan.apps.retain(|id| crate::catalog::find(id).is_none_or(|i| i.license.is_empty()));
                    if let Err(e) = self.plan.save() {
                        sys::log(&format!("plan.json: {e}"));
                    }
                }
                Action::Pick(id, on) => {
                    self.plan.set(id, on);
                    // a program with licence terms added: they are to be accepted again
                    if on && crate::catalog::find(id).is_some_and(|i| !i.license.is_empty()) {
                        self.plan.accepted = false;
                    }
                }
                Action::Browser(choice) => {
                    for b in crate::catalog::of(crate::catalog::Group::Browser) {
                        self.plan.set(b.id, Some(b.id) == choice);
                    }
                    if choice.and_then(crate::catalog::find).is_some_and(|i| !i.license.is_empty()) {
                        self.plan.accepted = false;
                    }
                }
                Action::Offline(on) => self.plan.offline = on,
                Action::Accept(on) => {
                    // at the setup: saved at once, the steps read the plan from its file
                    self.plan.accepted = on;
                    if let Err(e) = self.plan.save() {
                        sys::log(&format!("plan.json: {e}"));
                    }
                }
                Action::WinOpt(id, on) => self.plan.set_windows(id, on),
                Action::AddFolder => {
                    if let Some(d) = rfd::FileDialog::new().set_title("A folder to back up").pick_folder() {
                        let f = crate::plan::store_folder(&d);
                        if !self.plan.folders.contains(&f) {
                            self.plan.folders.push(f);
                        }
                    }
                }
                Action::RemoveFolder(i) => {
                    if i < self.plan.folders.len() {
                        self.plan.folders.remove(i);
                    }
                }
                Action::BrowseSaveTo => {
                    let mut d = rfd::FileDialog::new().set_title("Where to save the backup");
                    if std::path::Path::new(self.save_to.trim()).is_dir() {
                        d = d.set_directory(self.save_to.trim());
                    }
                    if let Some(dir) = d.pick_folder() {
                        self.save_to = dir.display().to_string();
                    }
                }
                Action::ToggleBackup(i) => {
                    self.backup_on[i] = !self.backup_on[i];
                }
                Action::SetTheme(m) => {
                    self.look.theme = m;
                    self.look.save();
                    self.theme_checked = Instant::now().checked_sub(Duration::from_secs(5)).unwrap_or_else(Instant::now);
                }
                Action::Done => {
                    if !self.resume_pending && !self.removed {
                        self.removed = true;
                        steps::remove_myself();
                    }
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                Action::RunBackup => {
                    // the folder chosen for the backup (HeMiMizer.exe is copied there at the end)
                    let to = std::path::PathBuf::from(self.save_to.trim().trim_matches('"'));
                    config::set_save_to((!to.as_os_str().is_empty() && to != config::exe_dir()).then_some(to));
                    // what the page did not offer (the settings of programs not chosen) stays out
                    for i in 0..self.backup_items.len() {
                        if !self.backup_visible(self.backup_items[i].id) {
                            self.backup_on[i] = false;
                        }
                    }
                    // the wizard decides the installers and the own folders
                    if let Some(i) = self.backup_items.iter().position(|it| it.id == crate::backup::INSTALLERS) {
                        self.backup_on[i] = self.plan.offline;
                    }
                    if let Some(i) = self.backup_items.iter().position(|it| it.id == crate::backup::FOLDERS) {
                        self.backup_on[i] = !self.plan.folders.is_empty();
                    }
                    if let Err(e) = self.plan.save() {
                        sys::log(&format!("plan.json: {e}"));
                    }
                    // the choice is ticked again the next time
                    let ids: Vec<String> = self.backup_items.iter().zip(&self.backup_on).filter(|(_, on)| **on).map(|(i, _)| i.id.to_string()).collect();
                    let _ = crate::backup::save_choice(&ids);
                    let chosen: Vec<usize> = (0..self.backup_items.len()).filter(|&i| self.backup_on[i]).collect();
                    self.backup_done.iter_mut().for_each(|d| *d = None);
                    self.phase = Phase::BackingUp;
                    let ctx = ctx.clone();
                    crate::backup::run(chosen, Some(self.seal_pw.clone()).filter(|p| !p.is_empty()), self.btx.clone(), move || ctx.request_repaint());
                }
                Action::OpenFolder => {
                    if let Some(d) = config::dir().parent() {
                        sys::shell_open(&d.display().to_string());
                    }
                }
                Action::IsoOpen => self.phase = Phase::Iso,
                Action::IsoBrowse => {
                    if let Some(f) = rfd::FileDialog::new().set_title("Windows 11 ISO").add_filter("ISO", &["iso"]).pick_file() {
                        self.iso.iso = f.display().to_string();
                    }
                }
                Action::IsoDrivers => {
                    if let Some(d) = rfd::FileDialog::new().set_title("Drivers").pick_folder() {
                        self.iso.drivers = d.display().to_string();
                    }
                }
                Action::IsoMake => {
                    let o = self.iso.options();
                    *self.iso.state.lock().unwrap() = Some(("Starting".into(), None));
                    let (state, ctx) = (self.iso.state.clone(), ctx.clone());
                    std::thread::spawn(move || {
                        let log = |t: String| {
                            if let Some(s) = state.lock().unwrap().as_mut() {
                                s.0 = t;
                            }
                            ctx.request_repaint();
                        };
                        let r = crate::iso::build(&o, &log);
                        sys::log(&format!("iso: {r:?}"));
                        *state.lock().unwrap() = Some((String::new(), Some(r)));
                        ctx.request_repaint();
                    });
                }
                Action::UsbOpen => {
                    self.phase = Phase::Usb;
                    self.usb.disks = crate::usb::disks();
                    if self.usb.disks.len() == 1 {
                        self.usb.disk = Some(self.usb.disks[0].number);
                    }
                }
                Action::UsbAsk => self.usb.asking = true,
                Action::UsbCancel => self.usb.asking = false,
                Action::DlOpen => self.phase = Phase::Download,
                Action::DlBrowse => {
                    if let Some(d) = rfd::FileDialog::new().set_title("Save the ISO to").pick_folder() {
                        self.dl.folder = d.display().to_string();
                    }
                }
                Action::DlStart => self.dl.start(ctx),
                Action::DlToUsb(iso) => {
                    self.usb.iso = iso;
                    self.phase = Phase::Usb;
                    self.usb.disks = crate::usb::disks();
                    if self.usb.disks.len() == 1 {
                        self.usb.disk = Some(self.usb.disks[0].number);
                    }
                }
                Action::DlToIso(iso) => {
                    self.iso.iso = iso;
                    self.phase = Phase::Iso;
                }
                Action::UsbRefresh => {
                    self.usb.disks = crate::usb::disks();
                    self.usb.asking = false;
                }
                Action::UsbBrowse => {
                    if let Some(f) = rfd::FileDialog::new().set_title("Windows ISO").add_filter("ISO", &["iso"]).pick_file() {
                        self.usb.iso = f.display().to_string();
                    }
                }
                Action::UsbMake => {
                    let o = self.usb.options();
                    *self.usb.state.lock().unwrap() = Some(("Starting".into(), None));
                    self.usb.asking = false;
                    let (state, ctx) = (self.usb.state.clone(), ctx.clone());
                    std::thread::spawn(move || {
                        let log = |t: String| {
                            if let Some(s) = state.lock().unwrap().as_mut() {
                                s.0 = t;
                            }
                            ctx.request_repaint();
                        };
                        let r = crate::usb::make(&o, &log);
                        sys::log(&format!("usb: {r:?}"));
                        *state.lock().unwrap() = Some((String::new(), Some(r)));
                        ctx.request_repaint();
                    });
                }
                Action::Retry => {
                    // only the failed steps, in their order; what is done (installed.txt) is not done twice
                    self.chosen = std::mem::take(&mut self.failed_steps);
                    self.fresh = false;
                    self.start(ctx);
                }
                Action::Unlock => {
                    *self.unlock.lock().unwrap() = Some(None);
                    let (state, pw, ctx) = (self.unlock.clone(), self.unlock_pw.clone(), ctx.clone());
                    std::thread::spawn(move || {
                        let r = crate::crypto::unseal(&pw);
                        let ok = r.is_ok();
                        *state.lock().unwrap() = Some(Some(r));
                        // opened: HeMiMizer starts again, now with the settings of the backup
                        if ok {
                            if let Ok(exe) = std::env::current_exe() {
                                let _ = std::process::Command::new(exe).spawn();
                            }
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                        ctx.request_repaint();
                    });
                }
                Action::Pack => {
                    *self.pack.lock().unwrap() = Some(None);
                    let (state, ctx) = (self.pack.clone(), ctx.clone());
                    std::thread::spawn(move || {
                        let r = crate::backup::pack();
                        sys::log(&format!("pack: {r:?}"));
                        *state.lock().unwrap() = Some(Some(r));
                        ctx.request_repaint();
                    });
                }
                Action::Start => {
                    if self.phase == Phase::Start {
                        self.start(ctx);
                    }
                }
                Action::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                Action::AskReboot => self.dialog = Some(Dialog::Reboot),
                Action::About(on) => self.about = on,
                Action::Open(t) => sys::shell_open(&t),
                Action::OpenLog => {
                    let log = sys::log_path();
                    if log.exists() {
                        sys::shell_open(&log.display().to_string());
                    }
                }
                Action::OpenLicences => {
                    let file = std::env::temp_dir().join("HeMiMizer-ThirdPartyNotices.txt");
                    if std::fs::write(&file, include_str!("../assets/ThirdPartyNotices.txt")).is_ok() {
                        sys::shell_open(&file.display().to_string());
                    }
                }
                Action::DefaultApps(app) => programs::open_default_apps(app),
                Action::RestartNow => {
                    let _ = sys::exec("shutdown.exe", &["/r", "/t", "0"]);
                }
                Action::CancelRestart => {
                    self.restart_at = None;
                    self.notes.push(tr("HeMiMizer continues by itself after the next restart and sign-in.", "HeMiMizer pokračuje sám po příštím restartu a přihlášení.").into());
                }
                Action::BrowseDriver(i) => {
                    if let Some(f) = pick_exe(&self.driver_paths[i]) {
                        self.driver_paths[i] = f;
                    }
                }
                Action::BrowseFile(i) => {
                    if let Some(list) = self.need_files.as_mut() {
                        if let Some(f) = pick_exe(&list[i].3) {
                            list[i].3 = f;
                        }
                    }
                }
                Action::PickOc(i) => {
                    let docs = crate::sys::documents();
                    let picked = rfd::FileDialog::new().set_title("Choose the OC profile exported in Adrenalin").add_filter("XML", &["xml"]).set_directory(&docs).pick_file();
                    if let Some(f) = picked {
                        let to = config::dir().join("OC.xml");
                        let r = std::fs::create_dir_all(config::dir())
                            .and_then(|()| std::fs::copy(&f, &to))
                            .map(|_| format!("{} → OC.xml", f.display()))
                            .map_err(|e| e.to_string());
                        self.backup_done[i] = Some(r);
                    }
                }
                Action::RetryDownloads => {
                    self.need_files = None;
                    *downloads::REPLY.lock().unwrap() = Some(downloads::Reply::Retry);
                }
                Action::ContinueDownloads => {
                    let files = self
                        .need_files
                        .take()
                        .unwrap_or_default()
                        .into_iter()
                        .filter_map(|(id, _, _, path)| {
                            let p = std::path::PathBuf::from(path.trim().trim_matches('"'));
                            p.is_file().then_some((id, p))
                        })
                        .collect();
                    *downloads::REPLY.lock().unwrap() = Some(downloads::Reply::Continue(files));
                }
            }
        }
    }
}

/// The Open dialog of Windows for an installer (.exe / .msi), starting in the folder of `current`.
fn pick_exe(current: &str) -> Option<String> {
    let mut d = rfd::FileDialog::new().set_title(tr("Choose the installer", "Vyber instalačku")).add_filter(tr("Installers", "Instalačky"), &["exe", "msi"]);
    if let Some(dir) = std::path::Path::new(current.trim()).parent().filter(|d| d.is_dir()) {
        d = d.set_directory(dir);
    } else if let Some(dir) = config::dir().parent() {
        d = d.set_directory(dir);
    }
    d.pick_file().map(|p| p.display().to_string())
}

impl eframe::App for HeMiMizerApp {
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        // a download of Windows that runs ends with the window
        crate::msiso::stop();
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.poll();
        self.follow_theme(&ctx);
        ctx.request_repaint_after(Duration::from_secs(1));

        if self.restart_at.is_some_and(|t| Instant::now() >= t) {
            self.restart_at = None;
            let _ = sys::exec("shutdown.exe", &["/r", "/t", "0"]);
        }
        // closing in the middle of the work would leave a half-installed program
        // the last run is over: HeMiMizer removes itself when the window closes (only the log and the to-do file stay)
        if ctx.input(|i| i.viewport().close_requested()) && self.phase == Phase::Finished && !self.resume_pending && !self.removed {
            self.removed = true;
            steps::remove_myself();
        }
        if ctx.input(|i| i.viewport().close_requested()) && self.phase == Phase::Running && !self.close_warned {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.close_warned = true;
        }

        let p = self.p;
        // transparent where Windows 11 shows Mica, else the colour of the theme
        let bg = if self.mica { egui::Color32::TRANSPARENT } else { p.bg };
        let mut acts = Vec::new();
        if !self.about {
            egui::Panel::bottom("footer")
                .frame(egui::Frame::new().fill(bg).inner_margin(Margin { left: 28, right: 28, top: 10, bottom: 16 }))
                .show_separator_line(false)
                .show(ui, |ui| {
                    if self.close_warned && self.phase == Phase::Running {
                        info_bar(
                            ui,
                            &p,
                            Tone::Caution,
                            tr(
                                "HeMiMizer is still working – closing it now could leave a program half installed. Close once more to quit anyway.",
                                "HeMiMizer ještě pracuje – zavřením by mohl zůstat program napůl nainstalovaný. Zavři ještě jednou, pokud přesto chceš skončit.",
                            ),
                            None,
                        );
                        ui.add_space(8.0);
                    }
                    self.footer(ui, &mut acts)
                });
        }
        egui::CentralPanel::default().frame(egui::Frame::new().fill(bg).inner_margin(Margin { left: 28, right: 28, top: 14, bottom: 4 })).show(ui, |ui| {
            self.header(ui, &mut acts);
            if self.about {
                self.about_page(ui, &mut acts);
                return;
            }
            // every page starts at its top
            let page_key = match self.phase {
                Phase::Wizard(page) => 10 + page as usize,
                Phase::Choose | Phase::Unlock => 0,
                Phase::Usb => 5,
                Phase::Iso => 6,
                Phase::Download => 7,
                Phase::BackingUp | Phase::BackedUp => 1,
                Phase::Start => 2,
                Phase::Running => 3,
                Phase::Finished => 4,
            };
            egui::ScrollArea::vertical().id_salt(page_key).auto_shrink([false, false]).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                match self.phase {
                    Phase::Choose => self.choose_page(ui, &mut acts),
                    Phase::Unlock => self.unlock_page(ui, &mut acts),
                    Phase::Usb => self.usb_page(ui, &mut acts),
                    Phase::Iso => self.iso_page(ui, &mut acts),
                    Phase::Download => self.dl_page(ui, &mut acts),
                    Phase::Wizard(page) => self.wizard_page(ui, page, &mut acts),
                    Phase::BackingUp | Phase::BackedUp => self.backup_page(ui, &mut acts),
                    Phase::Start => self.start_page(ui, &mut acts),
                    Phase::Running => {
                        self.need_files_page(ui, &mut acts);
                        self.progress_page(ui)
                    }
                    Phase::Finished => self.result_page(ui, &mut acts),
                }
            });
        });
        self.dialog(&ctx);
        self.act(&ctx, acts);
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        if self.mica { [0.0; 4] } else { self.p.bg.to_normalized_gamma_f32() }
    }
}
