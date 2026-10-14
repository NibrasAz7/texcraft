//! TexCraft Studio — egui front end.
//!
//! M0 scope: open/save a `.tex` file, edit it, and show the live
//! `texparse` structure (headings, labels, citations) and issues in a
//! side panel. The PDF preview lands with the typesetting engine; the
//! command layer (`engine`) and TCP control channel land with M2.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::path::PathBuf;

use texparse::{parse_structure, DocStructure};

const DEFAULT_DOC: &str = "\\documentclass{article}\n\\usepackage{amsmath}\n\n\\title{Untitled}\n\\author{}\n\n\\begin{document}\n\\maketitle\n\n\\section{Introduction}\n\n\\end{document}\n";

/// The studio application state.
pub struct TexApp {
    path: Option<PathBuf>,
    src: String,
    doc: DocStructure,
    dirty: bool,
    status: String,
}

impl TexApp {
    /// Create the app; `file` (optional) is opened at startup.
    pub fn new(_cc: &eframe::CreationContext<'_>, file: Option<PathBuf>) -> Self {
        let mut app = TexApp { path: None, src: DEFAULT_DOC.to_string(), doc: DocStructure::default(), dirty: false, status: String::new() };
        match file {
            Some(p) => app.open_path(p),
            None => app.reparse(),
        }
        app
    }

    fn reparse(&mut self) {
        self.doc = parse_structure(&self.src);
    }

    fn open_path(&mut self, path: PathBuf) {
        match std::fs::read(&path) {
            Ok(bytes) => {
                self.src = String::from_utf8_lossy(&bytes).into_owned();
                self.path = Some(path);
                self.dirty = false;
                self.status.clear();
                self.reparse();
            }
            Err(e) => self.status = format!("cannot open {}: {e}", path.display()),
        }
    }

    fn save(&mut self) {
        let Some(path) = self.path.clone() else {
            self.save_as();
            return;
        };
        match std::fs::write(&path, self.src.as_bytes()) {
            Ok(()) => {
                self.dirty = false;
                self.status = format!("saved {}", path.display());
            }
            Err(e) => self.status = format!("cannot save {}: {e}", path.display()),
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn open_dialog(&mut self) {
        if let Some(p) = rfd::FileDialog::new().add_filter("LaTeX", &["tex", "sty", "cls"]).pick_file() {
            self.open_path(p);
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn save_as(&mut self) {
        if let Some(p) = rfd::FileDialog::new().add_filter("LaTeX", &["tex"]).save_file() {
            self.path = Some(p);
            self.save();
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn open_dialog(&mut self) {
        self.status = "open is not available on the web build".into();
    }

    #[cfg(target_arch = "wasm32")]
    fn save_as(&mut self) {
        self.status = "save-as is not available on the web build".into();
    }
}

const LEVEL_NAMES: [&str; 7] = ["part", "chapter", "section", "subsection", "subsubsection", "paragraph", "subparagraph"];

impl eframe::App for TexApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        // Ctrl+S saves.
        if ctx.input(|i| i.key_pressed(egui::Key::S) && i.modifiers.ctrl) {
            self.save();
        }

        egui::Panel::top("toolbar").show(ui, |ui| {
            ui.horizontal(|ui| {
                if ui.button("Open…").clicked() {
                    self.open_dialog();
                }
                if ui.button("Save").clicked() {
                    self.save();
                }
                if ui.button("Save as…").clicked() {
                    self.save_as();
                }
                ui.separator();
                let dark = ui.visuals().dark_mode;
                if ui.button(if dark { "☀ light" } else { "☾ dark" }).clicked() {
                    ctx.set_visuals(if dark { egui::Visuals::light() } else { egui::Visuals::dark() });
                }
                ui.separator();
                let name = self.path.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "(untitled)".into());
                ui.label(format!("{name}{}", if self.dirty { " •" } else { "" }));
            });
        });

        egui::Panel::right("outline").default_size(280.0).show(ui, |ui| {
            ui.heading("Outline");
            ui.separator();
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                if let Some((class, _)) = &self.doc.class {
                    ui.monospace(format!("class: {class}"));
                }
                for h in &self.doc.headings {
                    let name = LEVEL_NAMES.get(h.level as usize).copied().unwrap_or("?");
                    let indent = f32::from(h.level) * 10.0;
                    ui.horizontal(|ui| {
                        ui.add_space(indent);
                        ui.label(format!("{}:{}", h.pos.line, h.pos.col));
                        ui.label(&h.title).on_hover_text(name);
                    });
                }
                if self.doc.headings.is_empty() {
                    ui.weak("(no headings)");
                }

                ui.separator();
                ui.heading("Issues");
                if self.doc.issues.is_empty() {
                    ui.weak("none");
                } else {
                    for i in &self.doc.issues {
                        ui.colored_label(ui.visuals().error_fg_color, format!("{}:{}  {}", i.pos.line, i.pos.col, i.message));
                    }
                }

                if !self.doc.labels.is_empty() {
                    ui.separator();
                    ui.heading("Labels");
                    for (l, p) in &self.doc.labels {
                        ui.monospace(format!("{}:{}  {}", p.line, p.col, l));
                    }
                }
                if !self.doc.cite_keys.is_empty() {
                    ui.separator();
                    ui.heading("Citations");
                    for (k, p) in &self.doc.cite_keys {
                        ui.monospace(format!("{}:{}  \\cite{{{}}}", p.line, p.col, k));
                    }
                }
                if !self.doc.packages.is_empty() {
                    ui.separator();
                    ui.heading("Packages");
                    for (p, _) in &self.doc.packages {
                        ui.monospace(p);
                    }
                }
            });
        });

        egui::Panel::bottom("status").show(ui, |ui| {
            ui.horizontal(|ui| {
                let n = self.doc.issues.len();
                ui.label(if n == 0 { "no issues".into() } else { format!("{n} issue{}", if n == 1 { "" } else { "s" }) });
                ui.separator();
                ui.weak(&self.status);
            });
        });

        egui::CentralPanel::default().show(ui, |ui| {
            let out =
                egui::TextEdit::multiline(&mut self.src).font(egui::TextStyle::Monospace).desired_width(f32::INFINITY).lock_focus(true).show(ui);
            if out.response.changed() {
                self.dirty = true;
                self.reparse();
            }
        });
    }
}
