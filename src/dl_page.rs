//! The page "Download Windows": the ISO of Windows 11 or 10 in the language chosen, straight from Microsoft (msiso.rs).

use super::{Action, HeMimizerApp};
use crate::msiso::{self, Language, Product, Session};
use crate::theme::glyph;
use crate::widgets::{Btn, Tone, card, info_bar, section};
use eframe::egui::{self, RichText};
use std::sync::{Arc, Mutex};

/// The languages of a product, with the session they came in (needed for the link).
type Langs = Result<(Arc<Session>, Vec<Language>), String>;

#[derive(Default)]
pub struct DlUi {
    pub product: Option<Product>,
    /// the languages of the product chosen: None = not yet, Some(None) = reading
    pub langs: Arc<Mutex<Option<Option<Langs>>>>,
    pub lang: String,
    pub folder: String,
    /// (bytes done, total, None while it runs / the result: the ISO)
    pub state: Arc<Mutex<Option<(u64, u64, Option<Result<std::path::PathBuf, String>>)>>>,
}

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

    /// A product chosen: its languages from Microsoft, in the background.
    pub fn choose(&mut self, p: Product, ctx: &egui::Context) {
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
        let folder = std::path::PathBuf::from(self.folder.trim());
        *self.state.lock().unwrap() = Some((0, 0, None));
        let (state, ctx) = (self.state.clone(), ctx.clone());
        std::thread::spawn(move || {
            let r = msiso::link(&session, &lang).and_then(|(url, file)| {
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

impl HeMimizerApp {
    pub(super) fn dl_page(&mut self, ui: &mut egui::Ui, acts: &mut Vec<Action>) {
        let p = self.p;
        let state = self.dl.state.lock().unwrap().clone();
        if let Some((done, total, r)) = &state {
            match r {
                None => {
                    let text = if *total > 0 {
                        format!("Downloading – {:.1} of {:.1} GB", *done as f64 / 1e9, *total as f64 / 1e9)
                    } else {
                        "Asking Microsoft for the link…".to_string()
                    };
                    ui.horizontal(|ui| {
                        ui.add(egui::Spinner::new().size(18.0).color(p.accent_text));
                        ui.label(RichText::new(text).size(14.0).color(p.text));
                    });
                    if *total > 0 {
                        ui.add_space(8.0);
                        ui.add(egui::ProgressBar::new(*done as f32 / *total as f32).desired_height(6.0).fill(p.accent));
                    }
                    ui.add_space(6.0);
                    ui.label(RichText::new("From Microsoft's own servers. It may be stopped – started again, it goes on.").size(12.0).color(p.text3));
                    return;
                }
                Some(Ok(iso)) => {
                    info_bar(ui, &p, Tone::Success, &format!("Downloaded: {}", iso.display()), None);
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if Btn::new("Make a USB stick from it").icon(glyph::USB).accent().show(ui, &p).clicked() {
                            acts.push(Action::DlToUsb(iso.display().to_string()));
                        }
                        if Btn::new("Make a smaller Windows from it").icon(glyph::DISC).show(ui, &p).clicked() {
                            acts.push(Action::DlToIso(iso.display().to_string()));
                        }
                    });
                }
                Some(Err(e)) => {
                    info_bar(ui, &p, Tone::Critical, &format!("Not downloaded: {e}"), None);
                }
            };
            ui.add_space(12.0);
        }
        let ctx = ui.ctx().clone();
        let d = &mut self.dl;
        section(ui, &p, "Windows");
        card(ui, &p, |ui| {
            for prod in [Product::Windows11, Product::Windows10] {
                if ui.radio(d.product == Some(prod), RichText::new(prod.name()).size(13.5).color(p.text)).clicked() {
                    d.choose(prod, &ctx);
                }
            }
            ui.label(RichText::new("The 64-bit ISO of all editions (Home, Pro, Education…), as on microsoft.com.").size(11.5).color(p.text3));
        });
        ui.add_space(10.0);
        if d.product.is_some() {
            section(ui, &p, "Language");
            let langs = d.langs.lock().unwrap().clone();
            card(ui, &p, |ui| match &langs {
                Some(None) | None => {
                    ui.horizontal(|ui| {
                        ui.add(egui::Spinner::new().size(14.0).color(p.accent_text));
                        ui.label(RichText::new("Asking Microsoft for the languages…").size(13.0).color(p.text2));
                    });
                }
                Some(Some(Err(e))) => {
                    ui.label(RichText::new(e.as_str()).size(13.0).color(p.critical));
                }
                Some(Some(Ok((_, list)))) => {
                    let shown = list.iter().find(|l| l.sku == d.lang).map(|l| l.name.clone()).unwrap_or_else(|| "Choose…".into());
                    egui::ComboBox::from_id_salt("dl-lang").selected_text(shown).width(260.0).height(400.0).show_ui(ui, |ui| {
                        for l in list {
                            ui.selectable_value(&mut d.lang, l.sku.clone(), l.name.as_str());
                        }
                    });
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
