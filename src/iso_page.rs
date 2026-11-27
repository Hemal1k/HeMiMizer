//! The page "Make a custom Windows ISO" of the window (the work is in iso.rs).

use super::{Action, HeMiMizerApp};
use crate::iso::{self, Compress};
use crate::theme::glyph;
use crate::widgets::{Btn, Tone, card, dropdown, info_bar, section};
use eframe::egui::{self, RichText, vec2};
use std::sync::{Arc, Mutex};

pub type Shared<T> = Arc<Mutex<Option<T>>>;

/// The compressions offered in Custom, with their names.
const COMPRESS: [(Compress, &str); 3] = [
    (Compress::Recovery, "Smallest – install.esd (slowest)"),
    (Compress::Max, "Small – install.wim"),
    (Compress::Fast, "Quick – install.wim (biggest)"),
];

#[derive(Default)]
pub struct IsoUi {
    pub iso: String,
    /// the editions of the ISO read: None = not yet, Some(Err) = not readable
    pub editions: Shared<Result<Vec<(u32, String)>, String>>,
    /// the ISO whose editions are read (or being read)
    pub read_for: String,
    /// the indexes of the editions chosen (empty = the default one is chosen when they are read)
    pub picked: Vec<u32>,
    pub custom: bool,
    /// Easy: Microsoft Edge is removed too
    pub easy_no_edge: bool,
    /// Custom: the ids of the settings on, and of the apps removed
    pub chosen: Vec<String>,
    pub apps: Vec<String>,
    pub compress: Compress,
    pub deep_clean: bool,
    pub drivers: String,
    /// winget ids, as typed
    pub winget: String,
    pub installers: String,
    pub add_hemimizer: bool,
    pub out: String,
    /// None = not started; Some((what it does now, None while it runs / the result))
    pub state: Shared<(String, Option<Result<String, String>>)>,
}

impl IsoUi {
    pub fn new() -> IsoUi {
        let out = crate::sys::known_folder(&windows_sys::Win32::UI::Shell::FOLDERID_Downloads)
            .unwrap_or_else(crate::sys::profile)
            .join("Windows 11 HeMiMizer.iso");
        IsoUi { chosen: iso::easy(), apps: iso::default_apps(), deep_clean: true, out: out.display().to_string(), ..Default::default() }
    }

    pub fn running(&self) -> bool {
        self.state.lock().unwrap().as_ref().is_some_and(|(_, r)| r.is_none())
    }

    fn iso_path(&self) -> std::path::PathBuf {
        self.iso.trim().trim_matches('"').into()
    }

    /// The editions chosen that the ISO has: (index, name).
    fn picked_editions(&self) -> Vec<(u32, String)> {
        match self.editions.lock().unwrap().as_ref() {
            Some(Ok(v)) => v.iter().filter(|(i, _)| self.picked.contains(i)).cloned().collect(),
            _ => Vec::new(),
        }
    }

    fn bad_winget(&self) -> Vec<String> {
        iso::winget_ids(&self.winget).into_iter().filter(|i| !iso::valid_winget(i)).collect()
    }

    pub fn ready(&self) -> bool {
        self.iso_path().is_file()
            && !self.picked_editions().is_empty()
            && !self.out.trim().is_empty()
            && self.bad_winget().is_empty()
            && !self.running()
            && iso::space_problem(&self.options()).is_none()
    }

    pub fn options(&self) -> iso::Options {
        let (chosen, apps, compress, deep_clean) = if self.custom {
            (self.chosen.clone(), self.apps.clone(), self.compress, self.deep_clean)
        } else {
            // Easy: the recommended settings, as small as it gets (cleaned, install.esd)
            let mut c = iso::easy();
            if self.easy_no_edge {
                c.push("edge".into());
            }
            (c, Vec::new(), Compress::Recovery, true)
        };
        iso::Options {
            iso: self.iso_path(),
            editions: self.picked_editions(),
            out: self.out.trim().trim_matches('"').into(),
            easy: !self.custom,
            apps,
            chosen,
            drivers: self.drivers.trim().to_string(),
            winget: iso::winget_ids(&self.winget),
            installers: self.installers.trim().to_string(),
            add_hemimizer: self.add_hemimizer,
            compress,
            deep_clean,
        }
    }

    /// The editions of a newly chosen ISO are read in the background.
    pub fn read_editions(&mut self, ctx: &egui::Context) {
        let path = self.iso_path();
        if self.iso == self.read_for || !path.is_file() {
            return;
        }
        self.read_for = self.iso.clone();
        self.picked.clear();
        *self.editions.lock().unwrap() = None;
        let (slot, ctx) = (self.editions.clone(), ctx.clone());
        std::thread::spawn(move || {
            let r = iso::editions(&path);
            *slot.lock().unwrap() = Some(r);
            ctx.request_repaint();
        });
    }
}

/// A checkbox with a line of detail under it; returns the new state when it was clicked.
fn check(ui: &mut egui::Ui, p: &crate::theme::Palette, on: bool, title: &str, detail: &str) -> Option<bool> {
    let mut v = on;
    let changed = ui.checkbox(&mut v, RichText::new(title).size(13.5).color(p.text)).changed();
    if !detail.is_empty() {
        ui.label(RichText::new(format!("      {detail}")).size(11.5).color(p.text3));
    }
    changed.then_some(v)
}

fn flip(list: &mut Vec<String>, id: &str, on: bool) {
    list.retain(|c| c != id);
    if on {
        list.push(id.to_string());
    }
}

impl HeMiMizerApp {
    pub(super) fn iso_page(&mut self, ui: &mut egui::Ui, acts: &mut Vec<Action>) {
        let p = self.p;
        let state = self.iso.state.lock().unwrap().clone();
        let running = matches!(&state, Some((_, None)));
        // the state stays on top of the page (with its progress), the choices below it
        if let Some((now, r)) = &state {
            match r {
                None => {
                    card(ui, &p, |ui| {
                        ui.horizontal(|ui| {
                            ui.add(egui::Spinner::new().size(16.0).color(p.accent_text));
                            ui.label(RichText::new(now.as_str()).size(14.0).color(p.text));
                        });
                        if let Some(pc) = iso::percent(now.as_bytes()) {
                            ui.add_space(6.0);
                            ui.add(egui::ProgressBar::new(pc / 100.0).desired_height(6.0).fill(p.accent));
                        }
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("It takes 20–60 minutes. The PC can be used meanwhile; do not close HeMiMizer.").size(12.0).color(p.text3));
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                let busy = iso::cancelled();
                                if Btn::new(if busy { "Cancelling…" } else { "Cancel" }).enabled(!busy).show(ui, &p).clicked() {
                                    acts.push(Action::IsoCancel);
                                }
                            });
                        });
                    });
                }
                Some(Ok(t)) => {
                    info_bar(ui, &p, Tone::Success, &format!("Done: {t}. Make a USB stick from it on the first page."), None);
                }
                Some(Err(e)) if e == iso::CANCELLED => {
                    info_bar(ui, &p, Tone::Caution, e, None);
                }
                Some(Err(e)) => {
                    info_bar(ui, &p, Tone::Critical, &format!("Not made: {e}"), None);
                }
            };
            ui.add_space(10.0);
        }
        self.iso.read_editions(ui.ctx());
        ui.add_enabled_ui(!running, |ui| self.iso_form(ui, acts));
    }

    fn iso_form(&mut self, ui: &mut egui::Ui, acts: &mut Vec<Action>) {
        let p = self.p;
        let s = &mut self.iso;
        section(ui, &p, "Windows 11 ISO");
        card(ui, &p, |ui| {
            if Self::path_row(ui, &p, &mut s.iso, "The Windows 11 .iso") {
                acts.push(Action::IsoBrowse);
            }
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("No ISO yet?").size(12.0).color(p.text3));
                if Btn::new("Download Windows").icon(glyph::DOWNLOAD).subtle().show(ui, &p).clicked() {
                    acts.push(Action::DlOpen);
                }
            });
        });
        ui.add_space(10.0);
        let eds = s.editions.lock().unwrap().clone();
        if !s.iso.trim().is_empty() {
            section(ui, &p, "Editions");
            card(ui, &p, |ui| match &eds {
                None => {
                    ui.horizontal(|ui| {
                        ui.add(egui::Spinner::new().size(14.0).color(p.accent_text));
                        ui.label(RichText::new("Reading the ISO…").size(13.0).color(p.text2));
                    });
                }
                Some(Err(e)) => {
                    ui.label(RichText::new(e.as_str()).size(13.0).color(p.critical));
                }
                Some(Ok(v)) => {
                    if s.picked.is_empty()
                        && let Some((i, _)) = v.iter().find(|(_, n)| n.ends_with(" Pro")).or(v.first())
                    {
                        s.picked.push(*i);
                    }
                    for (i, n) in v {
                        let mut on = s.picked.contains(i);
                        if ui.checkbox(&mut on, RichText::new(n.as_str()).size(13.5).color(p.text)).changed() {
                            s.picked.retain(|x| x != i);
                            if on {
                                s.picked.push(*i);
                            }
                        }
                    }
                    ui.label(RichText::new("More editions in one ISO: Windows Setup asks which one to install. Each adds some time, little size.").size(11.5).color(p.text3));
                }
            });
            ui.add_space(10.0);
        }
        section(ui, &p, "What changes");
        card(ui, &p, |ui| {
            ui.radio_value(&mut s.custom, false, RichText::new("Easy – recommended").size(13.5).color(p.text));
            ui.label(
                RichText::new("      Only Store, Windows Security, Terminal, Notepad, Photos, Media Player, Snipping Tool and Calculator stay. No ads, AI or OneDrive; installs without TPM and a Microsoft account.")
                    .size(12.0)
                    .color(p.text2),
            );
            if !s.custom {
                ui.horizontal(|ui| {
                    ui.add_space(18.0);
                    ui.checkbox(&mut s.easy_no_edge, RichText::new("Remove Microsoft Edge too").size(13.0).color(p.text));
                });
            }
            ui.add_space(4.0);
            ui.radio_value(&mut s.custom, true, RichText::new("Custom – choose yourself").size(13.5).color(p.text));
        });
        if s.custom {
            ui.add_space(8.0);
            section(ui, &p, "Settings");
            card(ui, &p, |ui| {
                for item in iso::SETTINGS {
                    if let Some(on) = check(ui, &p, s.chosen.iter().any(|c| c == item.id), item.title, item.detail) {
                        flip(&mut s.chosen, item.id, on);
                    }
                }
            });
            ui.add_space(8.0);
            section(ui, &p, "Remove apps");
            card(ui, &p, |ui| {
                ui.horizontal(|ui| {
                    if Btn::new("All").subtle().show(ui, &p).clicked() {
                        s.apps = iso::APPS.iter().map(|a| a.id.to_string()).collect();
                    }
                    if Btn::new("Recommended").subtle().show(ui, &p).clicked() {
                        s.apps = iso::default_apps();
                    }
                    if Btn::new("None").subtle().show(ui, &p).clicked() {
                        s.apps.clear();
                    }
                });
                ui.add_space(4.0);
                ui.columns(2, |cols| {
                    let half = iso::APPS.len().div_ceil(2);
                    for (k, a) in iso::APPS.iter().enumerate() {
                        let ui = &mut cols[usize::from(k >= half)];
                        let mut on = s.apps.iter().any(|x| x == a.id);
                        if ui.checkbox(&mut on, RichText::new(a.name).size(13.5).color(p.text)).changed() {
                            flip(&mut s.apps, a.id, on);
                        }
                    }
                });
                ui.label(RichText::new("Microsoft Store, Windows Security, App Installer and the libraries apps need always stay.").size(11.5).color(p.text3));
            });
            ui.add_space(8.0);
            section(ui, &p, "Size");
            card(ui, &p, |ui| {
                let names: Vec<&str> = COMPRESS.iter().map(|(_, n)| *n).collect();
                let now = COMPRESS.iter().position(|(c, _)| *c == s.compress).unwrap_or(0);
                if let Some(i) = dropdown(ui, &p, now, &names, 320.0) {
                    s.compress = COMPRESS[i].0;
                }
                ui.add_space(6.0);
                ui.checkbox(&mut s.deep_clean, RichText::new("Clean the image (smaller, adds about 15 minutes)").size(13.5).color(p.text));
            });
        }
        ui.add_space(8.0);
        section(ui, &p, "Add (optional)");
        card(ui, &p, |ui| {
            ui.label(RichText::new("Drivers – a folder with .inf files (network, storage, graphics…)").size(13.0).color(p.text));
            if Self::path_row(ui, &p, &mut s.drivers, "Empty = none") {
                acts.push(Action::IsoDrivers);
            }
            ui.add_space(8.0);
            ui.label(RichText::new("Apps from winget, installed at the first sign-in (ids, e.g. Mozilla.Firefox 7zip.7zip)").size(13.0).color(p.text));
            ui.add(egui::TextEdit::singleline(&mut s.winget).desired_width(f32::INFINITY).margin(vec2(8.0, 6.0)).hint_text("Empty = none"));
            let bad = s.bad_winget();
            if !bad.is_empty() {
                ui.label(RichText::new(format!("Not a winget id: {}", bad.join(", "))).size(12.0).color(p.critical));
            }
            ui.add_space(8.0);
            ui.label(RichText::new("Installers – a folder whose .exe and .msi files run one by one at the first sign-in").size(13.0).color(p.text));
            if Self::path_row(ui, &p, &mut s.installers, "Empty = none") {
                acts.push(Action::IsoInstallers);
            }
            ui.label(RichText::new("The apps install after the first sign-in, with the internet on; Windows asks for administrator rights once.").size(11.5).color(p.text3));
            ui.add_space(8.0);
            ui.checkbox(&mut s.add_hemimizer, RichText::new("HeMiMizer on the desktop of the new Windows").size(13.5).color(p.text));
        });
        ui.add_space(10.0);
        section(ui, &p, "Save as");
        card(ui, &p, |ui| {
            ui.add(egui::TextEdit::singleline(&mut s.out).desired_width(f32::INFINITY).margin(vec2(8.0, 6.0)));
        });
        ui.add_space(4.0);
        match iso::space_problem(&s.options()) {
            Some(problem) if s.iso_path().is_file() => {
                ui.label(RichText::new(problem).size(12.0).color(p.critical));
            }
            _ => {
                ui.label(RichText::new("Needs about 25 GB free on the system disk while it works.").size(12.0).color(p.text3));
            }
        }
    }
}
