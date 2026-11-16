//! The window of HeMimizer, as simple as the author wants it: it asks for the password of the NAS (which is also
//! the start – nothing runs before Run is clicked), then shows the step it is on, out of how many, what it is doing
//! and what failed; at the end what is left to do by hand and a restart.

use crate::i18n::tr;
use crate::steps::{self, Event, Step};
use crate::theme::{self, Palette, glyph};
use crate::widgets::{Btn, Tone, card, info_bar, row_layout, section, two_lines};
use crate::{config, downloads, drivers, programs, sys};
use eframe::egui::{self, Align, CornerRadius, Id, Layout, Margin, RichText, Stroke, vec2};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

const REPO: &str = "https://github.com/Hemal1k/HeMimizer";
const RESTART_DELAY: u64 = 20;

/// How HeMimizer was started.
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
    Licences,
    Windows,
    Settings,
    Folders,
    Save,
}

#[derive(Clone, Copy, PartialEq)]
enum Phase {
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
    /// the licence terms accepted or not
    Accept(bool),
    /// a step of Windows on or off
    WinOpt(&'static str, bool),
    AddFolder,
    RemoveFolder(usize),
    BrowseSaveTo,
    ToggleBackup(usize),
    /// The theme changed.
    SetTheme(u8),
    /// The end: HeMimizer removes itself and its data, then closes.
    Done,
    RunBackup,
    OpenFolder,
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

pub struct HeMimizerApp {
    steps: Vec<Step>,
    /// The indices of the steps of this run, in order.
    chosen: Vec<usize>,
    phase: Phase,
    /// How many of `chosen` are finished.
    done: usize,
    current: Option<usize>,
    log_line: String,
    failures: Vec<(String, String)>,
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
    /// The theme chosen in the settings.
    look: crate::look::Look,
    about: bool,
    dialog: Option<Dialog>,
    close_warned: bool,
    /// the run ended before a restart – HeMimizer continues later, so it must not remove itself
    resume_pending: bool,
    removed: bool,
    fresh: bool,
    /// the installers of the drivers (chipset, graphics) chosen on the start page
    driver_paths: [String; 2],
    /// downloads that failed: (id, name, error, file chosen by hand)
    need_files: Option<Vec<(String, String, String, String)>>,
    /// The backup: the items, which are on, their results, what it does now.
    backup_items: Vec<crate::backup::Item>,
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
    /// the folder the backup goes to (empty = the folder of HeMimizer.exe)
    save_to: String,
}

impl HeMimizerApp {
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
        theme::style_window(hwnd, &p);
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
        let mut app = HeMimizerApp {
            steps,
            chosen,
            phase: Phase::Choose,
            done: 0,
            current: None,
            log_line: String::new(),
            failures: Vec::new(),
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
        };
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
                crate::backup::Event::Finished => {
                    self.backup_now = None;
                    self.phase = Phase::BackedUp;
                    // a backup in another folder: HeMimizer goes along, the setup is started from there
                    let to = config::save_to();
                    if to != config::exe_dir() {
                        if let Ok(exe) = std::env::current_exe() {
                            if let Err(e) = std::fs::copy(&exe, to.join("HeMimizer.exe")) {
                                sys::log(&format!("copy HeMimizer.exe to {}: {e}", to.display()));
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
            theme::style_window(self.hwnd, &self.p);
        }
    }

    // ------------------------------------------------------------------ the window

    fn header(&self, ui: &mut egui::Ui, acts: &mut Vec<Action>) {
        let p = self.p;
        ui.horizontal(|ui| {
            ui.set_min_height(40.0);
            // back to the first page from the backup and from the start of the setup
            let back_to_choose = !self.about && matches!(self.phase, Phase::BackedUp | Phase::Start | Phase::Wizard(_)) && self.fresh;
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
                    _ => "HeMimizer",
                }
            };
            ui.label(RichText::new(title).font(theme::display(28.0)).color(p.text));
            if let Phase::Wizard(page) = self.phase {
                let pages = self.pages();
                let n = pages.iter().position(|x| *x == page).unwrap_or(0) + 1;
                ui.add_space(8.0);
                ui.label(RichText::new(format!("Step {n} of {}", pages.len())).size(13.0).color(p.text3));
            }
            if !self.about {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if Btn::new("").icon(glyph::SETTINGS).subtle().show(ui, &p).clicked() {
                        acts.push(Action::About(true));
                    }
                });
            }
        });
        ui.add_space(18.0);
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
        let items = [("Project on GitHub", REPO), ("Report a problem", ""), ("Log", "HeMimizer.log"), ("Licences", "")];
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
            "Before reinstalling: choose what the new Windows gets, and save your settings (and the installers, if you like) into the folder of HeMimizer.",
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
                "After reinstalling, without a backup: choose drivers and programs, then HeMimizer installs them and sets up Windows."
            },
            if planned { Action::Go(Phase::Start) } else { Action::StartWizard(false) },
            acts,
        );
    }

    /// A program of the wizard: its name, a line, an optional tag ("Found on this PC") and a switch.
    fn pick_row(ui: &mut egui::Ui, p: &Palette, item: &crate::catalog::Item, on: bool, tag: Option<&str>) -> bool {
        let mut clicked = false;
        card(ui, p, |ui| {
            row_layout(ui, 36.0, |ui| {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 1.0;
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(item.name).size(14.0).color(p.text));
                        if let Some(t) = tag {
                            ui.add_space(6.0);
                            ui.label(RichText::new(t).size(12.0).color(p.accent_text));
                        }
                    });
                    if !item.line.is_empty() {
                        ui.add(egui::Label::new(RichText::new(item.line).size(12.0).color(p.text2)).truncate());
                    }
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if crate::widgets::toggle(ui, p, on).clicked() {
                        clicked = true;
                    }
                });
            });
        });
        clicked
    }

    /// Programs as cards in two columns (a long list fits on one screen).
    fn pick_grid(&self, ui: &mut egui::Ui, items: &[&'static crate::catalog::Item], acts: &mut Vec<Action>) {
        let p = self.p;
        for pair in items.chunks(2) {
            ui.columns(2, |cols| {
                for (col, item) in cols.iter_mut().zip(pair) {
                    let on = self.plan.has(item.id);
                    if Self::pick_row(col, &p, item, on, None) {
                        acts.push(Action::Pick(item.id, !on));
                    }
                }
            });
        }
    }

    /// The pages of this wizard: the licences only when a program chosen has some; the settings and own folders only
    /// before a backup.
    fn pages(&self) -> Vec<Page> {
        let mut v = vec![Page::Hardware, Page::Browser, Page::Apps];
        if !self.plan.licensed().is_empty() {
            v.push(Page::Licences);
        }
        v.push(Page::Windows);
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
            Page::Licences => "Licence terms",
            Page::Windows => "Windows",
            Page::Settings => "Settings to take along",
            Page::Folders => "Your folders",
            Page::Save if self.wizard_backup => "Where to save the backup",
            Page::Save => "Last details",
        }
    }

    /// Can Next / Back up go on from this page?
    fn page_ok(&self, page: Page) -> bool {
        match page {
            Page::Licences => self.plan.accepted,
            Page::Save => crate::plan::valid_pc_name(&self.plan.pc_name) && (!self.wizard_backup || !self.save_to.trim().is_empty()),
            _ => true,
        }
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
                        intro(ui, &text);
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
                    let tag = suggested.contains(&item.id).then_some("Found on this PC");
                    if Self::pick_row(ui, &p, item, self.plan.has(item.id), tag) {
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
                        let tag = tools.contains(&item.id).then_some("For this PC");
                        if Self::pick_row(ui, &p, item, self.plan.has(item.id), tag) {
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
                            if Self::pick_row(col, &p, item, on, None) {
                                acts.push(Action::Browser(if item.id.is_empty() || on { None } else { Some(item.id) }));
                            }
                        }
                    });
                }
            }
            Page::Apps => {
                intro(ui, "The newest versions, installed quietly through winget.");
                for (kind, title) in catalog::KINDS {
                    let items: Vec<&'static catalog::Item> = catalog::of_kind(*kind).collect();
                    section(ui, &p, title);
                    self.pick_grid(ui, &items, acts);
                    ui.add_space(12.0);
                }
                section(ui, &p, "HeMi apps");
                let hemi: Vec<&'static catalog::Item> = catalog::of(Group::Hemi).collect();
                for item in hemi {
                    if Self::pick_row(ui, &p, item, self.plan.has(item.id), None) {
                        acts.push(Action::Pick(item.id, !self.plan.has(item.id)));
                    }
                }
            }
            Page::Licences => {
                intro(ui, "These programs have their own licence terms. HeMimizer installs them only when you accept them.");
                for item in self.plan.licensed() {
                    card(ui, &p, |ui| {
                        row_layout(ui, 32.0, |ui| {
                            ui.label(RichText::new(item.name).size(14.0).color(p.text));
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if Btn::new("Licence terms").icon(glyph::OPEN_LINK).subtle().show(ui, &p).clicked() {
                                    acts.push(Action::Open(item.license.to_string()));
                                }
                            });
                        });
                    });
                }
                ui.add_space(10.0);
                let accept = catalog::Item {
                    id: "",
                    name: "I accept the licence terms of all programs above",
                    line: "Without it, go back and leave them out.",
                    group: Group::App,
                    kind: catalog::Kind::Other,
                    source: catalog::Source::Own,
                    license: "",
                };
                if Self::pick_row(ui, &p, &accept, self.plan.accepted, None) {
                    acts.push(Action::Accept(!self.plan.accepted));
                }
            }
            Page::Windows => {
                intro(ui, "What HeMimizer changes in Windows. Most people want what is on; the rest changes how Windows looks and behaves.");
                let groups: [(&str, &[&str]); 3] = [
                    ("Clean up", &["restore-point", "ai", "microsoft-apps", "ad-software", "xbox-apps", "cleanup"]),
                    ("Updates and runtimes", &["windows-update", "store", "runtimes", "no-wu-drivers", "features"]),
                    ("Look and behaviour", &["explorer", "taskbar", "start", "games", "power", "hibernation", "startup", "sign-in", "sound-mouse", "network"]),
                ];
                for (title, ids) in groups {
                    section(ui, &p, title);
                    for id in ids {
                        let Some(s) = self.steps.iter().find(|s| s.id == *id) else { continue };
                        let row = catalog::Item { id: "", name: s.title.0, line: s.detail.0, group: Group::App, kind: catalog::Kind::Other, source: catalog::Source::Own, license: "" };
                        let on = self.plan.windows_on(id);
                        if Self::pick_row(ui, &p, &row, on, None) {
                            acts.push(Action::WinOpt(s.id, !on));
                        }
                    }
                    ui.add_space(12.0);
                }
            }
            Page::Settings => {
                intro(ui, "The settings of this PC and of its programs, put back on the new one.");
                self.backup_page(ui, acts);
            }
            Page::Folders => {
                intro(ui, "Folders of yours to copy into the backup – they are put back to the same place on the new PC. The folders of the profile (Documents, Pictures…) go into the profile of the new user.");
                let mut remove = None;
                for (i, f) in self.plan.folders.iter().enumerate() {
                    card(ui, &p, |ui| {
                        row_layout(ui, 32.0, |ui| {
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
            Page::Save => {
                if self.wizard_backup {
                    section(ui, &p, "Save to");
                    card(ui, &p, |ui| {
                        ui.add(egui::Label::new(RichText::new("The backup and HeMimizer itself go into this folder – a USB stick or a network drive is best. Start HeMimizer from there after reinstalling.").size(12.0).color(p.text2)).wrap());
                        ui.add_space(6.0);
                        if Self::path_row(ui, &p, &mut self.save_to, "Folder") {
                            acts.push(Action::BrowseSaveTo);
                        }
                    });
                    ui.add_space(14.0);
                    section(ui, &p, "Installers");
                    let offline = catalog::Item {
                        id: "",
                        name: "Download all installers now",
                        line: "The new PC then installs everything without the internet (needs more space)",
                        group: Group::App,
                        kind: catalog::Kind::Other,
                        source: catalog::Source::Own,
                        license: "",
                    };
                    if Self::pick_row(ui, &p, &offline, self.plan.offline, None) {
                        acts.push(Action::Offline(!self.plan.offline));
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
                let chosen: Vec<&str> = self.plan.apps.iter().filter_map(|id| catalog::find(id)).map(|i| i.name).collect();
                section(ui, &p, "To install");
                card(ui, &p, |ui| {
                    let text = if chosen.is_empty() { "Nothing – only the settings of Windows.".to_string() } else { chosen.join(", ") };
                    ui.add(egui::Label::new(RichText::new(text).size(13.0).color(p.text2)).wrap());
                });
            }
        }
    }

    /// What the backup takes; the user's own default can be saved.
    fn backup_page(&mut self, ui: &mut egui::Ui, acts: &mut Vec<Action>) {
        let p = self.p;
        let running = matches!(self.phase, Phase::BackingUp | Phase::BackedUp);
        section(ui, &p, tr("What to back up", "Co zálohovat"));
        for (i, item) in self.backup_items.iter().enumerate() {
            if running && !self.backup_on[i] {
                continue;
            }
            // the installers and the own folders have their own pages in the wizard
            if !running && (item.id == crate::backup::INSTALLERS || item.id == crate::backup::FOLDERS) {
                continue;
            }
            card(ui, &p, |ui| {
                row_layout(ui, 36.0, |ui| {
                    // the state while it runs, the switch before
                    let state = &self.backup_done[i];
                    if running {
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
                    }
                    let detail = match state {
                        Some(Ok(t)) | Some(Err(t)) => t.lines().next().unwrap_or("").to_string(),
                        None if self.backup_now == Some(i) => self.backup_log.clone(),
                        // before the backup: the network drives it found (the passwords are asked on the new PC)
                        None if item.id == "drives" && !running => {
                            if self.found_drives.is_empty() {
                                "No network drives on this PC".into()
                            } else {
                                let list: Vec<String> = self.found_drives.iter().map(|(l, s)| format!("{l}: {s}")).collect();
                                format!("{} – the password is asked on the new PC", list.join(", "))
                            }
                        }
                        None => String::new(),
                    };
                    if detail.is_empty() {
                        ui.label(RichText::new(item.name).size(14.0).color(p.text));
                    } else {
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 1.0;
                            ui.label(RichText::new(item.name).size(14.0).color(p.text));
                            let color = if matches!(state, Some(Err(_))) { p.critical } else { p.text2 };
                            ui.add(egui::Label::new(RichText::new(detail).size(12.0).color(color)).wrap());
                        });
                    }
                    // the OC profile cannot be read from Adrenalin: the file exported there is chosen here
                    if running && item.id == "oc" && matches!(state, Some(Err(_))) {
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if Btn::new("").icon(glyph::OPEN_FILE).subtle().tip("Choose the exported profile (.xml)").show(ui, &p).clicked() {
                                acts.push(Action::PickOc(i));
                            }
                        });
                    }
                    if !running {
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if crate::widgets::toggle(ui, &p, self.backup_on[i]).clicked() {
                                acts.push(Action::ToggleBackup(i));
                            }
                        });
                    }
                });
            });
        }
        ui.add_space(6.0);
        let where_ = format!(
            "{} {}",
            tr("Into", "Do"),
            config::dir().parent().map(|d| d.display().to_string()).unwrap_or_default()
        );
        ui.label(RichText::new(where_).size(12.0).color(p.text3));
    }

    /// Before the run: the installers of the drivers, the password of the NAS and Run.
    fn start_page(&mut self, ui: &mut egui::Ui, acts: &mut Vec<Action>) {
        let p = self.p;
        let (tone, text) = if self.offline > 0 {
            (Tone::Success, tr("Installers from the backup: {n} – programs are installed offline.", "Instalačky ze zálohy: {n} – programy se nainstalují offline.").replace("{n}", &self.offline.to_string()))
        } else {
            (Tone::Caution, tr("No installers from a backup – the programs are downloaded after the internet is connected.", "Žádné instalačky ze zálohy – programy se stáhnou po připojení k internetu.").to_string())
        };
        info_bar(ui, &p, tone, &text, None);
        ui.add_space(10.0);
        section(ui, &p, tr("Drivers (offline, right after the network drives)", "Ovladače (offline, hned po síťových discích)"));
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
        if !self.passwords.is_empty() {
            section(ui, &p, tr("Password for the NAS", "Heslo k NAS"));
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
                    "Windows saves it in Credential Manager; HeMimizer does not keep it.",
                    "Windows ho uloží do Správce přihlašovacích údajů; HeMimizer si ho nenechává.",
                ))
                .size(12.0)
                .color(p.text3),
            );
        }
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
                    "The drivers are installed – the PC restarts and HeMimizer continues after the sign-in.",
                    "Ovladače jsou nainstalované – PC se restartuje a HeMimizer po přihlášení pokračuje.",
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
        let dir = sys::log_path().parent().map(|d| d.display().to_string()).unwrap_or_default();
        let bye = tr(
            "When you close this window, HeMimizer removes itself and its temporary files. HeMimizer.log and the to-do file stay in {dir}.",
            "Po zavření okna se HeMimizer smaže i se svými dočasnými soubory. HeMimizer.log a soubor s ručními úkoly zůstanou v {dir}.",
        )
        .replace("{dir}", &dir);
        ui.add(egui::Label::new(RichText::new(bye).size(12.0).color(p.text3)).wrap());
        ui.add_space(14.0);
        section(ui, &p, tr("Left to do by hand", "Zbývá udělat ručně"));
        card(ui, &p, |ui| {
            row_layout(ui, 44.0, |ui| {
                two_lines(
                    ui,
                    &p,
                    tr("Default apps", "Výchozí aplikace"),
                    tr("Windows does not let programs choose them – click Set default on each page.", "Windows nedovolí programům je nastavit – na každé stránce klikni na Nastavit jako výchozí."),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    for app in ["7-Zip", "qBittorrent", "VLC", "Brave"] {
                        if Btn::new(app).show(ui, &p).clicked() {
                            acts.push(Action::DefaultApps(app));
                        }
                    }
                });
            });
        });
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
                            tr("The drivers are installed. HeMimizer goes on by itself as soon as the PC is online.", "Ovladače jsou nainstalované. HeMimizer sám pokračuje, jakmile bude PC online."),
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
                        let t = tr("The PC restarts in {s} s, then HeMimizer continues.", "PC se restartuje za {s} s, pak HeMimizer pokračuje.").replace("{s}", &left.to_string());
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
            if matches!(self.phase, Phase::Running | Phase::Finished)
                && Btn::new(tr("Log", "Záznam")).subtle().tip(tr("What HeMimizer did, step by step", "Co HeMimizer dělal, krok po kroku")).show(ui, &p).clicked()
            {
                acts.push(Action::OpenLog);
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| match self.phase {
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
                    if Btn::new(tr("Run", "Spustit")).icon(glyph::PLAY).accent().show(ui, &p).clicked() {
                        acts.push(Action::Start);
                    }
                    if Btn::new(tr("Close", "Zavřít")).show(ui, &p).clicked() {
                        acts.push(Action::Quit);
                    }
                }
                // the end: Done closes HeMimizer and removes it with its data (the log and the to-do file stay)
                Phase::Finished if self.restart_at.is_none() && !self.resume_pending => {
                    if Btn::new("Done").icon(glyph::COMPLETED).accent().tip("Closes HeMimizer and removes it with its data; the log and the to-do file stay").show(ui, &p).clicked() {
                        acts.push(Action::Done);
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
                }                Action::Pick(id, on) => {
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
                Action::Accept(on) => self.plan.accepted = on,
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
                    // the folder chosen for the backup (HeMimizer.exe is copied there at the end)
                    let to = std::path::PathBuf::from(self.save_to.trim().trim_matches('"'));
                    config::set_save_to((!to.as_os_str().is_empty() && to != config::exe_dir()).then_some(to));
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
                    crate::backup::run(chosen, self.btx.clone(), move || ctx.request_repaint());
                }
                Action::OpenFolder => {
                    if let Some(d) = config::dir().parent() {
                        sys::shell_open(&d.display().to_string());
                    }
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
                    let file = std::env::temp_dir().join("HeMimizer-ThirdPartyNotices.txt");
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
                    self.notes.push(tr("HeMimizer continues by itself after the next restart and sign-in.", "HeMimizer pokračuje sám po příštím restartu a přihlášení.").into());
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

impl eframe::App for HeMimizerApp {
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
        // the last run is over: HeMimizer removes itself when the window closes (only the log and the to-do file stay)
        if ctx.input(|i| i.viewport().close_requested()) && self.phase == Phase::Finished && !self.resume_pending && !self.removed {
            self.removed = true;
            steps::remove_myself();
        }
        if ctx.input(|i| i.viewport().close_requested()) && self.phase == Phase::Running && !self.close_warned {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.close_warned = true;
        }

        let p = self.p;
        let mut acts = Vec::new();
        if !self.about {
            egui::Panel::bottom("footer")
                .frame(egui::Frame::new().fill(p.bg).inner_margin(Margin { left: 28, right: 28, top: 10, bottom: 16 }))
                .show_separator_line(false)
                .show(ui, |ui| {
                    if self.close_warned && self.phase == Phase::Running {
                        info_bar(
                            ui,
                            &p,
                            Tone::Caution,
                            tr(
                                "HeMimizer is still working – closing it now could leave a program half installed. Close once more to quit anyway.",
                                "HeMimizer ještě pracuje – zavřením by mohl zůstat program napůl nainstalovaný. Zavři ještě jednou, pokud přesto chceš skončit.",
                            ),
                            None,
                        );
                        ui.add_space(8.0);
                    }
                    self.footer(ui, &mut acts)
                });
        }
        egui::CentralPanel::default().frame(egui::Frame::new().fill(p.bg).inner_margin(Margin { left: 28, right: 28, top: 14, bottom: 4 })).show(ui, |ui| {
            self.header(ui, &mut acts);
            if self.about {
                self.about_page(ui, &mut acts);
                return;
            }
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                match self.phase {
                    Phase::Choose => self.choose_page(ui, &mut acts),
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
        self.p.bg.to_normalized_gamma_f32()
    }
}
