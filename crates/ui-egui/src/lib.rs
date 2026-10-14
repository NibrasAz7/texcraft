//! TexCraft Studio — egui front end.
//!
//! Editor + live `texparse` outline/issues on the left, PDF preview on the
//! right. The preview is produced by `document.compile` (crates/engine):
//! a system TeX engine (xelatex/lualatex/pdflatex on PATH, never
//! shell-escape) builds into a `.texcraft/` dir beside the document and
//! `hayro` rasterizes the PDF to preview textures on a worker thread.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::path::{Path, PathBuf};

use texcraft_engine::{CompileReport, Session};
use texparse::DocStructure;

const DEFAULT_DOC: &str = "\\documentclass{article}\n\\usepackage{amsmath}\n\n\\title{Untitled}\n\\author{}\n\n\\begin{document}\n\\maketitle\n\n\\section{Introduction}\n\n\\end{document}\n";

/// Render scale for preview pages (roughly 110 dpi on screen).
const PREVIEW_SCALE: f32 = 1.6;
/// Safety cap: never render a page bigger than this in either dimension.
const MAX_PREVIEW_PX: f32 = 4096.0;
/// Safety cap: never render more pages than this in one preview.
const MAX_PREVIEW_PAGES: usize = 128;

/// A finished compile job: the report plus rasterized page images.
#[cfg(not(target_arch = "wasm32"))]
type CompileJob = Result<(CompileReport, Vec<egui::ColorImage>), String>;

/// The studio application state.
pub struct TexApp {
    session: Session,
    doc: DocStructure,
    status: String,
    compiling: bool,
    #[cfg(not(target_arch = "wasm32"))]
    compile_rx: Option<std::sync::mpsc::Receiver<CompileJob>>,
    report: Option<CompileReport>,
    pages: Vec<egui::TextureHandle>,
    page_sizes: Vec<[usize; 2]>,
}

impl TexApp {
    /// Create the app; `file` (optional) is opened at startup.
    pub fn new(_cc: &eframe::CreationContext<'_>, file: Option<PathBuf>) -> Self {
        let mut app = TexApp {
            session: Session::new(),
            doc: DocStructure::default(),
            status: String::new(),
            compiling: false,
            #[cfg(not(target_arch = "wasm32"))]
            compile_rx: None,
            report: None,
            pages: Vec::new(),
            page_sizes: Vec::new(),
        };
        app.session = Session::with_source(DEFAULT_DOC.to_string());
        match file {
            Some(p) => app.open_path(p),
            None => app.reparse(),
        }
        app
    }

    fn reparse(&mut self) {
        self.doc = self.session.structure();
    }

    fn open_path(&mut self, path: PathBuf) {
        match self.session.open(&path) {
            Ok(()) => {
                self.status.clear();
                self.pages.clear();
                self.report = None;
                self.reparse();
            }
            Err(e) => self.status = format!("cannot open {}: {e}", path.display()),
        }
    }

    fn save(&mut self) {
        if self.session.path().is_none() {
            self.save_as();
            return;
        }
        match self.session.save() {
            Ok(()) => self.status = format!("saved {}", self.path_display()),
            Err(e) => self.status = e.to_string(),
        }
    }

    fn path_display(&self) -> String {
        self.session.path().map(|p| p.display().to_string()).unwrap_or_else(|| "(untitled)".into())
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
            match self.session.save_as(&p) {
                Ok(()) => self.status = format!("saved {}", p.display()),
                Err(e) => self.status = e.to_string(),
            }
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

    /// Kick off `document.compile` + PDF rasterization on a worker thread.
    #[cfg(not(target_arch = "wasm32"))]
    fn start_compile(&mut self) {
        if self.compiling {
            return;
        }
        if self.session.path().is_none() {
            self.save_as();
        }
        let Some(path) = self.session.path().map(Path::to_path_buf) else {
            self.status = "save the document to compile it".into();
            return;
        };
        if let Err(e) = self.session.save() {
            self.status = format!("cannot save before compile: {e}");
            return;
        }
        self.compiling = true;
        self.status = "compiling…".into();
        let (tx, rx) = std::sync::mpsc::channel();
        self.compile_rx = Some(rx);
        std::thread::spawn(move || {
            let _ = tx.send(compile_and_render(&path));
        });
    }

    #[cfg(target_arch = "wasm32")]
    fn start_compile(&mut self) {
        self.status = "compile is not available on the web build".into();
    }

    /// Collect a finished compile: store the report and build textures.
    #[cfg(not(target_arch = "wasm32"))]
    fn poll_compile(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.compile_rx else { return };
        match rx.try_recv() {
            Ok(Ok((report, images))) => {
                self.status = match (report.pdf.is_some(), report.errors.len()) {
                    (true, 0) => format!("{} — {} page(s)", report.engine, report.pages.unwrap_or(0)),
                    (true, n) => format!("{} — {} page(s), {n} error(s)", report.engine, report.pages.unwrap_or(0)),
                    (false, _) => format!("{} failed — {} error(s)", report.engine, report.errors.len()),
                };
                self.pages = images
                    .iter()
                    .enumerate()
                    .map(|(i, img)| ctx.load_texture(format!("page-{i}"), img.clone(), egui::TextureOptions::LINEAR))
                    .collect();
                self.page_sizes = images.iter().map(|i| i.size).collect();
                self.report = Some(report);
                self.compiling = false;
                self.compile_rx = None;
            }
            Ok(Err(e)) => {
                self.status = e;
                self.compiling = false;
                self.compile_rx = None;
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => ctx.request_repaint(),
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.status = "compile worker stopped unexpectedly".into();
                self.compiling = false;
                self.compile_rx = None;
            }
        }
    }
}

/// Run a system-TeX compile of `path`, then rasterize every produced PDF
/// page into an `egui::ColorImage` with `hayro`.
#[cfg(not(target_arch = "wasm32"))]
fn compile_and_render(path: &Path) -> CompileJob {
    let report = texcraft_engine::compile_file(path, None).map_err(|e| e.to_string())?;
    let Some(pdf_path) = &report.pdf else { return Ok((report, Vec::new())) };

    let data = std::fs::read(pdf_path).map_err(|e| format!("cannot read {}: {e}", pdf_path.display()))?;
    let pdf = hayro::hayro_syntax::Pdf::new(data).map_err(|e| format!("cannot parse PDF: {e:?}"))?;

    let cache = hayro::RenderCache::new();
    let interp = hayro::hayro_interpret::InterpreterSettings::default();
    let settings = hayro::RenderSettings::default();

    let mut images = Vec::new();
    for page in pdf.pages().iter().take(MAX_PREVIEW_PAGES) {
        let (w, h) = page.render_dimensions();
        let scale = PREVIEW_SCALE.min(MAX_PREVIEW_PX / w.max(h).max(1.0));
        let pixmap_settings = hayro::PixmapSettings { x_scale: scale, y_scale: scale, bg_color: hayro::vello_cpu::color::palette::css::WHITE };
        let pixmap = hayro::render(page, &cache, &interp, &settings, &pixmap_settings);
        images
            .push(egui::ColorImage::from_rgba_premultiplied([usize::from(pixmap.width()), usize::from(pixmap.height())], pixmap.data_as_u8_slice()));
    }
    Ok((report, images))
}

const LEVEL_NAMES: [&str; 7] = ["part", "chapter", "section", "subsection", "subsubsection", "paragraph", "subparagraph"];

impl eframe::App for TexApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        // Ctrl+S saves.
        if ctx.input(|i| i.key_pressed(egui::Key::S) && i.modifiers.ctrl) {
            self.save();
        }
        #[cfg(not(target_arch = "wasm32"))]
        self.poll_compile(&ctx);

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
                let compile_label = if self.compiling { "compiling…" } else { "▶ Compile" };
                if ui.add_enabled(!self.compiling, egui::Button::new(compile_label)).clicked() {
                    self.start_compile();
                }
                if self.compiling {
                    ui.spinner();
                }
                ui.separator();
                let dark = ui.visuals().dark_mode;
                if ui.button(if dark { "☀ light" } else { "☾ dark" }).clicked() {
                    ctx.set_visuals(if dark { egui::Visuals::light() } else { egui::Visuals::dark() });
                }
                ui.separator();
                ui.label(format!("{}{}", self.path_display(), if self.session.dirty() { " •" } else { "" }));
            });
        });

        egui::Panel::left("outline").default_size(240.0).show(ui, |ui| {
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

        egui::Panel::right("preview").default_size(460.0).show(ui, |ui| {
            ui.heading("Preview");
            ui.separator();
            if let Some(report) = &self.report {
                if !report.errors.is_empty() {
                    for e in &report.errors {
                        match e.line {
                            Some(l) => ui.colored_label(ui.visuals().error_fg_color, format!("{l}: {}", e.message)),
                            None => ui.colored_label(ui.visuals().error_fg_color, &e.message),
                        };
                    }
                    ui.separator();
                }
                if !report.warnings.is_empty() {
                    for w in &report.warnings {
                        ui.colored_label(ui.visuals().warn_fg_color, format!("warning: {}", w.message));
                    }
                    ui.separator();
                }
            }
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                if self.pages.is_empty() {
                    ui.weak(if self.compiling { "compiling…" } else { "no PDF yet — press Compile" });
                } else {
                    for tex in &self.pages {
                        ui.add(egui::Image::new((tex.id(), tex.size_vec2())).max_width(ui.available_width() - 12.0));
                        ui.add_space(8.0);
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
            let mut src = self.session.source().to_string();
            let out = egui::TextEdit::multiline(&mut src).font(egui::TextStyle::Monospace).desired_width(f32::INFINITY).lock_focus(true).show(ui);
            if out.response.changed() {
                self.session.set_source(src);
                self.reparse();
            }
        });
    }
}
