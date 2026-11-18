//! The page "Make a Windows USB stick" of the window (the work is in usb.rs).

use super::{Action, HeMimizerApp};
use crate::theme::glyph;
use crate::usb::{self, UsbDisk};
use crate::widgets::{Btn, Tone, card, info_bar, row_layout, section};
use eframe::egui::{self, RichText, vec2};
use std::sync::{Arc, Mutex};

/// What the page keeps: the choices, and the work while it runs.
#[derive(Default)]
pub struct UsbUi {
    pub disks: Vec<UsbDisk>,
    pub disk: Option<u32>,
    pub iso: String,
    pub gpt: bool,
    pub label: String,
    pub bypass_hw: bool,
    pub bypass_msa: bool,
    pub local: bool,
    pub user: String,
    pub regional: bool,
    pub no_privacy: bool,
    pub no_bitlocker: bool,
    /// the user confirmed that the stick is wiped
    pub sure: bool,
    /// None = not started; Some((what it does now, None while it runs / the result))
    pub state: Arc<Mutex<Option<(String, Option<Result<String, String>>)>>>,
}

impl UsbUi {
    pub fn new() -> UsbUi {
        UsbUi { gpt: true, label: "WINDOWS".into(), bypass_hw: true, bypass_msa: true, user: "user".into(), no_privacy: true, ..Default::default() }
    }

    pub fn running(&self) -> bool {
        self.state.lock().unwrap().as_ref().is_some_and(|(_, r)| r.is_none())
    }

    pub fn ready(&self) -> bool {
        self.disk.is_some_and(|d| self.disks.iter().any(|x| x.number == d))
            && std::path::Path::new(self.iso.trim().trim_matches('"')).is_file()
            && (!self.local || (!self.user.trim().is_empty() && usb::valid_user(&self.user)))
            && self.sure
            && !self.running()
    }

    pub fn options(&self) -> usb::Options {
        usb::Options {
            disk: self.disk.unwrap_or(u32::MAX),
            iso: self.iso.trim().trim_matches('"').into(),
            gpt: self.gpt,
            label: self.label.clone(),
            bypass_hw: self.bypass_hw,
            bypass_msa: self.bypass_msa,
            local_user: if self.local { self.user.trim().to_string() } else { String::new() },
            regional: self.regional,
            no_privacy: self.no_privacy,
            no_bitlocker: self.no_bitlocker,
        }
    }
}

impl HeMimizerApp {
    pub(super) fn usb_page(&mut self, ui: &mut egui::Ui, acts: &mut Vec<Action>) {
        let p = self.p;
        let state = self.usb.state.lock().unwrap().clone();
        if let Some((now, r)) = &state {
            match r {
                None => {
                    ui.horizontal(|ui| {
                        ui.add(egui::Spinner::new().size(18.0).color(p.accent_text));
                        ui.label(RichText::new(now.as_str()).size(14.0).color(p.text));
                    });
                    ui.add_space(6.0);
                    ui.label(RichText::new("Do not pull out the stick until it is done.").size(12.0).color(p.text3));
                    return;
                }
                Some(Ok(t)) => info_bar(ui, &p, Tone::Success, &format!("The stick is ready: {t}. Start the PC from it (the boot menu, often F12, F11 or F8)."), None),
                Some(Err(e)) => info_bar(ui, &p, Tone::Critical, &format!("Not made: {e}"), None),
            };
            ui.add_space(10.0);
        }
        let u = &mut self.usb;
        section(ui, &p, "USB disk");
        card(ui, &p, |ui| {
            if u.disks.is_empty() {
                ui.label(RichText::new("No USB disk found – put one in (all on it will be erased).").size(13.0).color(p.text2));
            }
            for d in &u.disks {
                let on = u.disk == Some(d.number);
                if ui.radio(on, RichText::new(d.label()).size(13.5).color(p.text)).clicked() {
                    u.disk = Some(d.number);
                    u.sure = false;
                }
            }
            ui.add_space(4.0);
            if Btn::new("Look again").icon(glyph::SYNC).subtle().show(ui, &p).clicked() {
                acts.push(Action::UsbRefresh);
            }
        });
        ui.add_space(10.0);
        section(ui, &p, "Windows ISO");
        card(ui, &p, |ui| {
            if Self::path_row(ui, &p, &mut u.iso, "The .iso of Windows 10 or 11") {
                acts.push(Action::UsbBrowse);
            }
            ui.add_space(4.0);
            ui.add(egui::Label::new(RichText::new("No ISO yet? Get one from microsoft.com/software-download, or make a smaller one with HeMimizer.").size(12.0).color(p.text3)).wrap());
        });
        ui.add_space(10.0);
        section(ui, &p, "The stick");
        card(ui, &p, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Partition scheme").size(13.5).color(p.text));
                ui.add_space(12.0);
                ui.radio_value(&mut u.gpt, true, "GPT – UEFI (PCs of the last ten years)");
                ui.radio_value(&mut u.gpt, false, "MBR – BIOS or UEFI (older PCs)");
            });
            ui.add_space(6.0);
            row_layout(ui, 30.0, |ui| {
                ui.label(RichText::new("Name").size(13.5).color(p.text));
                ui.add_space(12.0);
                ui.add(egui::TextEdit::singleline(&mut u.label).char_limit(11).desired_width(200.0).margin(vec2(8.0, 5.0)));
                ui.label(RichText::new(format!("as {}", usb::fat_label(&u.label))).size(12.0).color(p.text3));
            });
        });
        ui.add_space(10.0);
        section(ui, &p, "Installation of Windows");
        card(ui, &p, |ui| {
            ui.checkbox(&mut u.bypass_hw, "No need of more than 4 GB RAM, Secure Boot and TPM 2.0");
            ui.checkbox(&mut u.bypass_msa, "No Microsoft account required (an offline account is possible)");
            ui.horizontal(|ui| {
                ui.checkbox(&mut u.local, "A local account named");
                ui.add_enabled(u.local, egui::TextEdit::singleline(&mut u.user).char_limit(20).desired_width(160.0).margin(vec2(6.0, 4.0)));
            });
            if u.local && !usb::valid_user(&u.user) {
                ui.label(RichText::new("This name cannot be used.").size(12.0).color(p.critical));
            }
            ui.checkbox(&mut u.regional, "The regional settings of this PC (keyboard, formats, time zone)");
            ui.checkbox(&mut u.no_privacy, "No data collection (the privacy questions are skipped)");
            ui.checkbox(&mut u.no_bitlocker, "No automatic BitLocker encryption of the disk");
        });
        ui.add_space(10.0);
        if let Some(d) = u.disks.iter().find(|d| Some(d.number) == u.disk) {
            card(ui, &p, |ui| {
                row_layout(ui, 30.0, |ui| {
                    ui.label(RichText::new(glyph::WARNING.to_string()).font(crate::theme::icon(16.0)).color(p.caution));
                    ui.add_space(6.0);
                    ui.checkbox(&mut u.sure, RichText::new(format!("Everything on {} is erased", d.label())).size(13.5).color(p.text));
                });
            });
        }
    }
}
