//! The page "Download Windows": the ISO of Windows 11 or 10 in the language chosen (msiso.rs).

use super::{Action, HeMiMizerApp};
use crate::msiso::{self, Arch, Language, Product, Session};
use crate::theme::glyph;
use crate::widgets::{Btn, Tone, card, dropdown, info_bar, section};
use eframe::egui::{self, RichText};
use std::sync::{Arc, Mutex};

/// The languages of a product, with the session they came in (needed for the link).
type Langs = Result<(Arc<Session>, Vec<Language>), String>;

#[derive(Default)]
pub struct DlUi {
    pub product: Option<Product>,
    /// the processor: Windows 11 x64 / Arm64, Windows 10 64-bit / 32-bit
    pub arch: Option<Arch>,
    /// the languages of the product chosen: None = not yet, Some(None) = reading
    pub langs: Arc<Mutex<Option<Option<Langs>>>>,
    pub lang: String,
    pub folder: String,
    /// (bytes done, total, None while it runs / the result: the ISO)
    pub state: Arc<Mutex<Option<Progress>>>,
}

/// (bytes done, total, None while it runs / the result: the ISO)
pub type Progress = (u64, u64, Option<Result<std::path::PathBuf, String>>);

impl DlUi {
    pub fn new() -> DlUi {
        let folder = crate::sys::known_folder(&windows_sys::Win32::UI::Shell::FOLDERID_Downloads).unwrap_or_else(crate::sys::profile);
        DlUi { folder: folder.display().to_string(), ..Default::default() }
    }

    pub fn running(&self) -> bool {
        self.state.lock().unwrap().as_ref().is_some_and(|s| s.2.is_none())
    }

    pub fn ready(&self) -> bool {
        !self.running() && !self.lang.is_empty() && std::path::Path::new(self.folder.trim()).is_dir() && matches!(*self.langs.lock().unwrap(), Some(Some(Ok(_))))
    }

    /// Windows and its processor chosen: the languages of that product from Microsoft, in the background (Windows 11
    /// Arm64 is a product of its own; Windows 10 32-bit is another link of the same one).
    pub fn choose(&mut self, p: Product, arch: Arch, ctx: &egui::Context) {
        self.arch = Some(arch);
        if self.product == Some(p) {
            return;
        }
        self.product = Some(p);
        self.lang.clear();
        *self.langs.lock().unwrap() = Some(None);
        let (slot, ctx) = (self.langs.clone(), ctx.clone());
        std::thread::spawn(move || {
            let r = msiso::start(p).and_then(|s| msiso::languages(&s).map(|l| (Arc::new(s), l)));
            *slot.lock().unwrap() = Some(Some(r));
            ctx.request_repaint();
        });
    }

    /// The download, in the background.
    pub fn start(&mut self, ctx: &egui::Context) {
        let Some(Some(Ok((session, langs)))) = self.langs.lock().unwrap().clone() else { return };
        let Some(lang) = langs.iter().find(|l| l.sku == self.lang).cloned() else { return };
        let arch = self.arch.unwrap_or(Arch::X64);
        let folder = std::path::PathBuf::from(self.folder.trim());
        *self.state.lock().unwrap() = Some((0, 0, None));
        let (state, ctx) = (self.state.clone(), ctx.clone());
        std::thread::spawn(move || {
            let r = msiso::link(&session, &lang, arch).and_then(|(url, file)| {
                let dest = folder.join(&file);
                let progress = |done: u64, total: u64| {
                    *state.lock().unwrap() = Some((done, total, None));
                    ctx.request_repaint();
                };
                msiso::download(&url, &dest, &progress).map(|()| dest)
            });
            crate::sys::log(&format!("download Windows: {r:?}"));
            let (d, t) = state.lock().unwrap().as_ref().map(|s| (s.0, s.1)).unwrap_or((0, 0));
            *state.lock().unwrap() = Some((d, t, Some(r)));
            ctx.request_repaint();
        });
    }
}

impl HeMiMizerApp {
    pub(super) fn dl_page(&mut self, ui: &mut egui::Ui, acts: &mut Vec<Action>) {
        let p = self.p;
        let state = self.dl.state.lock().unwrap().clone();
        let running = matches!(&state, Some((_, _, None)));
        // the page stays as it is; its state (with the progress) is on top
        if let Some((done, total, r)) = &state {
            match r {
                None => {
                    let text = if *total > 0 {
                        format!("Downloading – {:.1} of {:.1} GB ({}%)", *done as f64 / 1e9, *total as f64 / 1e9, done * 100 / (*total).max(1))
                    } else {
                        "Getting the download link…".to_string()
                    };
                    card(ui, &p, |ui| {
                        ui.horizontal(|ui| {
                            ui.add(egui::Spinner::new().size(16.0).color(p.accent_text));
                            ui.label(RichText::new(text).size(14.0).color(p.text));
                        });
                        if *total > 0 {
                            ui.add_space(6.0);
                            ui.add(egui::ProgressBar::new(*done as f32 / *total as f32).desired_height(6.0).fill(p.accent));
                        }
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("You can stop it and go on later from where it stopped.").size(12.0).color(p.text3));
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if Btn::new("Stop").show(ui, &p).clicked() {
                                    acts.push(Action::DlStop);
                                }
                            });
                        });
                    });
                }
                Some(Ok(iso)) => {
                    info_bar(ui, &p, Tone::Success, &format!("Downloaded: {}", iso.display()), None);
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if Btn::new("Make a USB stick from it").icon(glyph::USB).accent().show(ui, &p).clicked() {
                            acts.push(Action::DlToUsb(iso.display().to_string()));
                        }
                        if Btn::new("Make a custom ISO from it").icon(glyph::DISC).show(ui, &p).clicked() {
                            acts.push(Action::DlToIso(iso.display().to_string()));
                        }
                    });
                }
                Some(Err(e)) if e == msiso::STOPPED_TEXT => {
                    info_bar(ui, &p, Tone::Caution, e, None);
                }
                Some(Err(e)) => {
                    info_bar(ui, &p, Tone::Critical, &format!("Not downloaded: {e}"), None);
                }
            };
            ui.add_space(12.0);
        }
        ui.add_enabled_ui(!running, |ui| self.dl_form(ui, acts));
    }

    fn dl_form(&mut self, ui: &mut egui::Ui, acts: &mut Vec<Action>) {
        let p = self.p;
        let ctx = ui.ctx().clone();
        let d = &mut self.dl;
        section(ui, &p, "Windows");
        card(ui, &p, |ui| {
            // Windows, and next to it the processor (x64 the first, the one most PCs have)
            let win11 = matches!(d.product, Some(Product::Windows11 | Product::Windows11Arm));
            let win10 = d.product == Some(Product::Windows10);
            type Choice = (&'static str, Product, Arch);
            let choices: [(&str, bool, [Choice; 2]); 2] = [
                ("Windows 11", win11, [("x64", Product::Windows11, Arch::X64), ("Arm64", Product::Windows11Arm, Arch::Arm64)]),
                ("Windows 10", win10, [("64-bit", Product::Windows10, Arch::X64), ("32-bit", Product::Windows10, Arch::X86)]),
            ];
            for (name, on, archs) in choices {
                ui.horizontal(|ui| {
                    if ui.radio(on, RichText::new(name).size(13.5).color(p.text)).clicked() && !on {
                        let (_, prod, arch) = archs[0];
                        d.choose(prod, arch, &ctx);
                    }
                    if on {
                        ui.add_space(16.0);
                        for (label, prod, arch) in archs {
                            if ui.radio(d.product == Some(prod) && d.arch == Some(arch), RichText::new(label).size(13.0).color(p.text2)).clicked() {
                                d.choose(prod, arch, &ctx);
                            }
                        }
                    }
                });
            }
            ui.label(RichText::new("The ISO straight from Microsoft.").size(11.5).color(p.text3));
        });
        ui.add_space(10.0);
        if d.product.is_some() {
            section(ui, &p, "Language");
            let langs = d.langs.lock().unwrap().clone();
            card(ui, &p, |ui| match &langs {
                Some(None) | None => {
                    ui.horizontal(|ui| {
                        ui.add(egui::Spinner::new().size(14.0).color(p.accent_text));
                        ui.label(RichText::new("Getting the languages…").size(13.0).color(p.text2));
                    });
                }
                Some(Some(Err(e))) => {
                    ui.label(RichText::new(e.as_str()).size(13.0).color(p.critical));
                }
                Some(Some(Ok((_, list)))) => {
                    // "Choose…" first until a language is chosen; a typed letter jumps to the languages with it
                    let mut names: Vec<&str> = list.iter().map(|l| l.name.as_str()).collect();
                    let now = list.iter().position(|l| l.sku == d.lang);
                    let offset = usize::from(now.is_none());
                    if now.is_none() {
                        names.insert(0, "Choose…");
                    }
                    if let Some(i) = dropdown(ui, &p, now.unwrap_or(0), &names, 260.0)
                        && let Some(l) = i.checked_sub(offset).and_then(|i| list.get(i))
                    {
                        d.lang = l.sku.clone();
                    }
                }
            });
            ui.add_space(10.0);
            section(ui, &p, "Save to");
            card(ui, &p, |ui| {
                if Self::path_row(ui, &p, &mut d.folder, "Folder") {
                    acts.push(Action::DlBrowse);
                }
                ui.label(RichText::new("About 6–9 GB.").size(11.5).color(p.text3));
            });
        }
    }
}
