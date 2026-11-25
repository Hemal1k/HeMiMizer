//! The page "Make a Windows USB stick" of the window (the work is in usb.rs).

use super::{Action, HeMiMizerApp};
use crate::theme::glyph;
use crate::usb::{self, Region, UsbDisk};
use crate::widgets::{Btn, Tone, card, info_bar, row_layout, section};
use eframe::egui::{self, RichText, vec2};
use std::sync::{Arc, Mutex};

pub type Shared<T> = Arc<Mutex<Option<T>>>;
/// (countries / regions, time zones, the region of this PC)
pub type RegionLists = (Vec<(String, String)>, Vec<(String, String)>, Region);

/// What the page keeps: the choices, and the work while it runs.
#[derive(Default)]
pub struct UsbUi {
    pub disks: Vec<UsbDisk>,
    pub disk: Option<u32>,
    pub iso: String,
    /// the ISO whose label is the name already (a newly chosen one gives its label)
    label_for: String,
    pub gpt: bool,
    pub ntfs: bool,
    pub label: String,
    pub bypass_hw: bool,
    pub bypass_msa: bool,
    pub local: bool,
    pub user: String,
    pub region_on: bool,
    pub culture: String,
    pub time_zone: String,
    /// the countries / regions and time zones of Windows, read in the background
    pub lists: Shared<RegionLists>,
    pub no_privacy: bool,
    pub no_bitlocker: bool,
    /// the question "everything on … is erased" is open
    pub asking: bool,
    /// None = not started; Some((what it does now, None while it runs / the result))
    pub state: Shared<(String, Option<Result<String, String>>)>,
}

impl UsbUi {
    pub fn new() -> UsbUi {
        // GPT with NTFS by default (big images fit), MBR with FAT32
        UsbUi { gpt: true, ntfs: true, label: "WINDOWS".into(), bypass_hw: true, bypass_msa: true, user: "user".into(), no_privacy: true, ..Default::default() }
    }

    pub fn running(&self) -> bool {
        self.state.lock().unwrap().as_ref().is_some_and(|(_, r)| r.is_none())
    }

    pub fn ready(&self) -> bool {
        self.disk.is_some_and(|d| self.disks.iter().any(|x| x.number == d))
            && std::path::Path::new(self.iso.trim().trim_matches('"')).is_file()
            && (!self.local || (!self.user.trim().is_empty() && usb::valid_user(&self.user)))
            && (!self.region_on || (!self.culture.is_empty() && !self.time_zone.is_empty()))
            && !self.running()
    }

    pub fn disk_label(&self) -> String {
        self.disks.iter().find(|d| Some(d.number) == self.disk).map(|d| d.label()).unwrap_or_default()
    }

    pub fn options(&self) -> usb::Options {
        usb::Options {
            disk: self.disk.unwrap_or(u32::MAX),
            iso: self.iso.trim().trim_matches('"').into(),
            gpt: self.gpt,
            ntfs: self.ntfs,
            label: self.label.clone(),
            bypass_hw: self.bypass_hw,
            bypass_msa: self.bypass_msa,
            local_user: if self.local { self.user.trim().to_string() } else { String::new() },
            region: self.region_on.then(|| Region { culture: self.culture.clone(), time_zone: self.time_zone.clone() }),
            no_privacy: self.no_privacy,
            no_bitlocker: self.no_bitlocker,
        }
    }

    /// The lists of regions and time zones, once, in the background (PowerShell takes a moment).
    pub fn load_lists(&mut self, ctx: &egui::Context) {
        if self.lists.lock().unwrap().is_some() || !self.culture.is_empty() {
            return;
        }
        self.culture = "…".into();
        let (slot, ctx) = (self.lists.clone(), ctx.clone());
        std::thread::spawn(move || {
            let r = (usb::cultures(), usb::time_zones(), usb::this_pc());
            *slot.lock().unwrap() = Some(r);
            ctx.request_repaint();
        });
    }

    /// A newly chosen ISO names the stick by its label.
    fn label_from_iso(&mut self) {
        if self.iso == self.label_for {
            return;
        }
        self.label_for = self.iso.clone();
        if let Some(l) = usb::iso_label(std::path::Path::new(self.iso.trim().trim_matches('"'))) {
            self.label = l;
        }
    }
}

impl HeMiMizerApp {
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
        self.usb.load_lists(ui.ctx());
        self.usb.label_from_iso();
        let lists = self.usb.lists.lock().unwrap().clone();
        let u = &mut self.usb;
        if u.culture == "…"
            && let Some((_, _, here)) = &lists
        {
            u.culture = here.culture.clone();
            u.time_zone = here.time_zone.clone();
        }
        section(ui, &p, "USB disk");
        card(ui, &p, |ui| {
            if u.disks.is_empty() {
                ui.label(RichText::new("No USB disk found – put one in (all on it will be erased).").size(13.0).color(p.text2));
            }
            for d in &u.disks {
                let on = u.disk == Some(d.number);
                if ui.radio(on, RichText::new(d.label()).size(13.5).color(p.text)).clicked() {
                    u.disk = Some(d.number);
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
            ui.horizontal(|ui| {
                ui.label(RichText::new("No ISO yet?").size(12.0).color(p.text3));
                if Btn::new("Download Windows").icon(glyph::DOWNLOAD).subtle().show(ui, &p).clicked() {
                    acts.push(Action::DlOpen);
                }
            });
        });
        ui.add_space(10.0);
        section(ui, &p, "The stick");
        card(ui, &p, |ui| {
            row_layout(ui, 30.0, |ui| {
                ui.label(RichText::new("Partition scheme").size(13.5).color(p.text));
                ui.add_space(12.0);
                // the file system follows the scheme (it can still be changed): NTFS with GPT, FAT32 with MBR
                if ui.radio(u.gpt, "GPT – UEFI").clicked() && !u.gpt {
                    u.gpt = true;
                    u.ntfs = true;
                }
                if ui.radio(!u.gpt, "MBR – BIOS or UEFI").clicked() && u.gpt {
                    u.gpt = false;
                    u.ntfs = false;
                }
            });
            row_layout(ui, 30.0, |ui| {
                ui.label(RichText::new("File system").size(13.5).color(p.text));
                ui.add_space(12.0);
                ui.radio_value(&mut u.ntfs, true, "NTFS");
                ui.radio_value(&mut u.ntfs, false, "FAT32");
            });
            ui.label(
                RichText::new(if u.ntfs {
                    "NTFS: an image of any size fits; a small FAT32 partition starts the PC."
                } else {
                    "FAT32: install.wim is split; an image with a file over 4 GB needs NTFS."
                })
                .size(11.5)
                .color(p.text3),
            );
            ui.add_space(4.0);
            row_layout(ui, 30.0, |ui| {
                ui.label(RichText::new("Name").size(13.5).color(p.text));
                ui.add_space(12.0);
                ui.add(egui::TextEdit::singleline(&mut u.label).char_limit(32).desired_width(260.0).margin(vec2(8.0, 5.0)));
                let shown = if u.ntfs { usb::ntfs_label(&u.label) } else { usb::fat_label(&u.label) };
                if shown != u.label {
                    ui.label(RichText::new(format!("as {shown}")).size(12.0).color(p.text3));
                }
            });
        });
        ui.add_space(10.0);
        section(ui, &p, "Installation of Windows");
        card(ui, &p, |ui| {
            ui.checkbox(&mut u.bypass_hw, "No check for 4 GB RAM, Secure Boot and TPM 2.0");
            ui.checkbox(&mut u.bypass_msa, "No Microsoft account required");
            ui.horizontal(|ui| {
                ui.checkbox(&mut u.local, "A local account named");
                ui.add_enabled(u.local, egui::TextEdit::singleline(&mut u.user).char_limit(20).desired_width(160.0).margin(vec2(6.0, 4.0)));
            });
            if u.local && !usb::valid_user(&u.user) {
                ui.label(RichText::new("This name cannot be used.").size(12.0).color(p.critical));
            }
            ui.checkbox(&mut u.region_on, "Region and time zone");
            if u.region_on {
                match &lists {
                    None => {
                        ui.horizontal(|ui| {
                            ui.add_space(24.0);
                            ui.add(egui::Spinner::new().size(12.0).color(p.accent_text));
                            ui.label(RichText::new("Reading the Windows regions…").size(12.0).color(p.text3));
                        });
                    }
                    Some((cultures, zones, _)) => {
                        ui.horizontal(|ui| {
                            ui.add_space(24.0);
                            let shown = cultures.iter().find(|(n, _)| *n == u.culture).map(|(_, e)| e.clone()).unwrap_or_else(|| u.culture.clone());
                            egui::ComboBox::from_id_salt("usb-region").selected_text(shown).width(300.0).height(400.0).show_ui(ui, |ui| {
                                for (n, e) in cultures {
                                    ui.selectable_value(&mut u.culture, n.clone(), e.as_str());
                                }
                            });
                        });
                        ui.horizontal(|ui| {
                            ui.add_space(24.0);
                            let shown = zones.iter().find(|(id, _)| *id == u.time_zone).map(|(_, d)| d.clone()).unwrap_or_else(|| u.time_zone.clone());
                            egui::ComboBox::from_id_salt("usb-zone").selected_text(shown).width(300.0).height(400.0).show_ui(ui, |ui| {
                                for (id, d) in zones {
                                    ui.selectable_value(&mut u.time_zone, id.clone(), d.as_str());
                                }
                            });
                        });
                        ui.horizontal(|ui| {
                            ui.add_space(24.0);
                            ui.label(RichText::new("The keyboard and the formats of the region; the Windows language is the one in the ISO.").size(11.5).color(p.text3));
                        });
                    }
                }
            }
            ui.checkbox(&mut u.no_privacy, "No data collection (the privacy questions are skipped)");
            ui.checkbox(&mut u.no_bitlocker, "No automatic BitLocker encryption of the disk");
        });
        // the question before the disk is wiped
        if u.asking {
            let label = u.disk_label();
            egui::Modal::new(egui::Id::unique("usb-erase")).show(ui.ctx(), |ui| {
                ui.set_width(420.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(glyph::WARNING.to_string()).font(crate::theme::icon(20.0)).color(p.caution));
                    ui.add_space(6.0);
                    ui.label(RichText::new("Erase the USB disk?").font(crate::theme::semibold(16.0)).color(p.text));
                });
                ui.add_space(8.0);
                ui.add(egui::Label::new(RichText::new(format!("Everything on {label} is erased, then the Windows stick is made.")).size(13.0).color(p.text2)).wrap());
                ui.add_space(14.0);
                ui.horizontal(|ui| {
                    if Btn::new("Erase and make").accent().show(ui, &p).clicked() {
                        acts.push(Action::UsbMake);
                    }
                    if Btn::new("Cancel").show(ui, &p).clicked() {
                        acts.push(Action::UsbCancel);
                    }
                });
            });
        }
    }
}
