//! The page "Make a smaller Windows" of the window (the work is in iso.rs).

use super::{Action, HeMimizerApp};
use crate::iso;
use crate::theme::glyph;
use crate::widgets::{Btn, Tone, card, info_bar, section};
use eframe::egui::{self, RichText, vec2};
use std::sync::{Arc, Mutex};

pub type Shared<T> = Arc<Mutex<Option<T>>>;

#[derive(Default)]
pub struct IsoUi {
    pub iso: String,
    /// the editions of the ISO read: None = not yet, Some(Err) = not readable
    pub editions: Shared<Result<Vec<(u32, String)>, String>>,
    /// the ISO whose editions are read (or being read)
    pub read_for: String,
    pub index: u32,
    pub custom: bool,
    pub chosen: Vec<String>,
    pub drivers: String,
    pub add_hemimizer: bool,
    pub esd: bool,
    pub deep_clean: bool,
    pub out: String,
    /// None = not started; Some((what it does now, None while it runs / the result))
    pub state: Shared<(String, Option<Result<String, String>>)>,
}

impl IsoUi {
    pub fn new() -> IsoUi {
        let out = crate::sys::known_folder(&windows_sys::Win32::UI::Shell::FOLDERID_Downloads)
            .unwrap_or_else(crate::sys::profile)
            .join("Windows 11 HeMimizer.iso");
        IsoUi { chosen: iso::simple(), deep_clean: true, esd: true, out: out.display().to_string(), ..Default::default() }
    }

    pub fn running(&self) -> bool {
        self.state.lock().unwrap().as_ref().is_some_and(|(_, r)| r.is_none())
    }

    fn iso_path(&self) -> std::path::PathBuf {
        self.iso.trim().trim_matches('"').into()
    }

    pub fn ready(&self) -> bool {
        self.iso_path().is_file()
            && self.editions.lock().unwrap().as_ref().is_some_and(|r| r.as_ref().is_ok_and(|v| v.iter().any(|(i, _)| *i == self.index)))
            && !self.out.trim().is_empty()
            && !self.running()
    }

    pub fn options(&self) -> iso::Options {
        iso::Options {
            iso: self.iso_path(),
            index: self.index,
            out: self.out.trim().trim_matches('"').into(),
            chosen: if self.custom { self.chosen.clone() } else { iso::simple() },
            drivers: if self.custom { self.drivers.clone() } else { String::new() },
            add_hemimizer: self.add_hemimizer,
            // the simple choice as small as Tiny11: cleaned and compressed harder (one edition ~5 GB instead of ~7 GB)
            esd: !self.custom || self.esd,
            deep_clean: !self.custom || self.deep_clean,
        }
    }

    /// The editions of a newly chosen ISO are read in the background.
    pub fn read_editions(&mut self, ctx: &egui::Context) {
        let path = self.iso_path();
        if self.iso == self.read_for || !path.is_file() {
            return;
        }
        self.read_for = self.iso.clone();
        *self.editions.lock().unwrap() = None;
        let (slot, ctx) = (self.editions.clone(), ctx.clone());
        std::thread::spawn(move || {
            let r = iso::editions(&path);
            *slot.lock().unwrap() = Some(r);
            ctx.request_repaint();
        });
    }
}

impl HeMimizerApp {
    pub(super) fn iso_page(&mut self, ui: &mut egui::Ui, acts: &mut Vec<Action>) {
        let p = self.p;
        let state = self.iso.state.lock().unwrap().clone();
        if let Some((now, r)) = &state {
            match r {
                None => {
                    ui.horizontal(|ui| {
                        ui.add(egui::Spinner::new().size(18.0).color(p.accent_text));
                        ui.label(RichText::new(now.as_str()).size(14.0).color(p.text));
                    });
                    ui.add_space(6.0);
                    ui.label(RichText::new("It takes 20–60 minutes. The PC can be used meanwhile; do not close HeMimizer.").size(12.0).color(p.text3));
                    return;
                }
                Some(Ok(t)) => info_bar(ui, &p, Tone::Success, &format!("Done: {t}. Make a USB stick from it on the first page."), None),
                Some(Err(e)) => info_bar(ui, &p, Tone::Critical, &format!("Not made: {e}"), None),
            };
            ui.add_space(10.0);
        }
        self.iso.read_editions(ui.ctx());
        let s = &mut self.iso;
        section(ui, &p, "Windows 11 ISO");
        card(ui, &p, |ui| {
            if Self::path_row(ui, &p, &mut s.iso, "The .iso of Windows 11 from Microsoft") {
                acts.push(Action::IsoBrowse);
            }
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("No ISO yet?").size(12.0).color(p.text3));
                if Btn::new("Download from Microsoft").icon(glyph::OPEN_LINK).subtle().tip("The page of Microsoft: choose the language there, then choose the .iso here").show(ui, &p).clicked() {
                    acts.push(Action::Open("https://www.microsoft.com/software-download/windows11".into()));
                }
            });
        });
        ui.add_space(10.0);
        let eds = s.editions.lock().unwrap().clone();
        if !s.iso.trim().is_empty() {
            section(ui, &p, "Edition");
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
                    if !v.iter().any(|(i, _)| *i == s.index) {
                        s.index = v.iter().find(|(_, n)| n.ends_with(" Pro")).or(v.first()).map(|(i, _)| *i).unwrap_or(1);
                    }
                    let shown = v.iter().find(|(i, _)| *i == s.index).map(|(_, n)| n.clone()).unwrap_or_default();
                    egui::ComboBox::from_id_salt("edition").selected_text(shown).width(320.0).show_ui(ui, |ui| {
                        for (i, n) in v {
                            ui.selectable_value(&mut s.index, *i, n.as_str());
                        }
                    });
                }
            });
            ui.add_space(10.0);
        }
        section(ui, &p, "What changes");
        card(ui, &p, |ui| {
            ui.radio_value(&mut s.custom, false, RichText::new("Simple – the recommended choice").size(13.5).color(p.text));
            ui.label(RichText::new("    Without ads, Teams, Clipchamp, Xbox, OneDrive and other extras; no TPM check, no Microsoft account needed; cleaned and compressed (about 5 GB, 40–60 minutes).").size(12.0).color(p.text2));
            ui.add_space(4.0);
            ui.radio_value(&mut s.custom, true, RichText::new("Custom – choose yourself").size(13.5).color(p.text));
        });
        if s.custom {
            ui.add_space(8.0);
            for (title, list) in [("Remove", iso::REMOVE), ("Set", iso::SETTINGS)] {
                section(ui, &p, title);
                card(ui, &p, |ui| {
                    for item in list {
                        let mut on = s.chosen.iter().any(|c| c == item.id);
                        if ui.checkbox(&mut on, RichText::new(item.title).size(13.5).color(p.text)).changed() {
                            s.chosen.retain(|c| c != item.id);
                            if on {
                                s.chosen.push(item.id.to_string());
                            }
                        }
                        ui.label(RichText::new(format!("      {}", item.detail)).size(11.5).color(p.text3));
                    }
                });
                ui.add_space(8.0);
            }
            section(ui, &p, "Add");
            card(ui, &p, |ui| {
                ui.label(RichText::new("Drivers (a folder with .inf files, e.g. of a network card or a disk controller)").size(13.0).color(p.text));
                if Self::path_row(ui, &p, &mut s.drivers, "Empty = none") {
                    acts.push(Action::IsoDrivers);
                }
                ui.add_space(4.0);
                ui.checkbox(&mut s.add_hemimizer, "HeMimizer on the desktop of the new Windows");
                ui.checkbox(&mut s.deep_clean, "Clean the image (smaller, adds ~15 minutes)");
                ui.checkbox(&mut s.esd, "Compress harder (install.esd – smaller, adds ~20 minutes)");
            });
        } else {
            ui.add_space(4.0);
            ui.checkbox(&mut s.add_hemimizer, "HeMimizer on the desktop of the new Windows");
        }
        ui.add_space(10.0);
        section(ui, &p, "Save as");
        card(ui, &p, |ui| {
            ui.add(egui::TextEdit::singleline(&mut s.out).desired_width(f32::INFINITY).margin(vec2(8.0, 6.0)));
        });
        ui.add_space(4.0);
        ui.label(RichText::new("Needs about 25 GB free on the system disk while it works.").size(12.0).color(p.text3));
    }
}
