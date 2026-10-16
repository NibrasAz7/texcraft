//! TexCraft Studio — egui front end.
//!
//! LocalLeaf-style three-pane layout: project file tree + document
//! outline on the left, a syntax-highlighted `.tex` editor with gutter
//! and status bar in the center, and a hayro-rendered PDF preview on the
//! right. `document.compile` (crates/engine) shells out to a system TeX
//! engine (never shell-escape) into `.texcraft/`; pages are rasterized
//! on a worker thread.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod files;
mod highlight;
mod theme;

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use egui::text::{CCursor, CCursorRange};
use texcraft_engine::{CompileReport, Session};
use texparse::DocStructure;

const DEFAULT_DOC: &str = "\\documentclass{article}\n\\usepackage{amsmath}\n\n\\title{Untitled}\n\\author{}\n\n\\begin{document}\n\\maketitle\n\n\\section{Introduction}\n\n\\end{document}\n";

/// Render scale for preview pages (roughly 110 dpi on screen).
const PREVIEW_SCALE: f32 = 1.6;
/// Safety cap: never render a page bigger than this in either dimension.
const MAX_PREVIEW_PX: f32 = 4096.0;
/// Safety cap: never render more pages than this in one preview.
const MAX_PREVIEW_PAGES: usize = 128;
/// Editor gutter width in points.
const GUTTER_W: f32 = 40.0;
/// How often the project tree is rescanned, seconds.
const TREE_SCAN_SECS: f64 = 1.5;

/// A rasterized page plus its PDF size in points (top-left origin) —
/// SyncTeX speaks points, so we need the pt dimensions to map clicks.
struct Page {
    img: egui::ColorImage,
    pt: egui::Vec2,
}

/// A finished compile job: the report plus rasterized page images.
#[cfg(not(target_arch = "wasm32"))]
type CompileJob = Result<(CompileReport, Vec<Page>), String>;

/// Preview zoom mode for the PDF pane.
#[derive(Clone, Copy, PartialEq)]
enum Zoom {
    FitWidth,
    FitPage,
    Pct(f32),
}

/// A SyncTeX highlight on a preview page: which page, the box in PDF
/// points, and the `ctx.input` timestamp it appeared at.
struct Flash {
    page: usize,
    rect_pt: egui::Rect,
    at: f64,
}
const FLASH_SECS: f64 = 1.6;

/// What the "new entry" modal is creating, and where.
struct NewEntry {
    dir: PathBuf,
    name: String,
    is_dir: bool,
}

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
    /// PDF point size per page, parallel to `pages`.
    page_pts: Vec<egui::Vec2>,
    pdf_path: Option<PathBuf>,
    page_idx: usize,
    zoom: Zoom,
    flash: Option<Flash>,

    // Sidebar + editor chrome
    tree: Vec<files::Node>,
    tree_scanned_at: f64,
    expanded: HashSet<PathBuf>,
    main_file: Option<PathBuf>,
    outline_open: bool,
    show_logs: bool,
    cursor_lc: (usize, usize),
    /// Pending "jump to byte offset" for the next editor frame (outline
    /// click-through).
    goto: Option<usize>,
    engines: Vec<String>,
    engine_sel: Option<usize>,
    new_entry: Option<NewEntry>,

    // Auto-compile after idle
    autocompile: bool,
    /// `ctx.input` timestamp of the last buffer edit.
    last_edit: f64,
    /// `last_edit` value at the last compile attempt — a failed compile
    /// does not retrigger until the user edits again.
    compiled_edit: f64,
}
const AUTOCOMPILE_IDLE_SECS: f64 = 5.0;

impl TexApp {
    /// Create the app; `file` (optional) is opened at startup.
    pub fn new(_cc: &eframe::CreationContext<'_>, file: Option<PathBuf>) -> Self {
        theme::apply(&_cc.egui_ctx, true);
        let mut app = TexApp {
            session: Session::new(),
            doc: DocStructure::default(),
            status: String::new(),
            compiling: false,
            #[cfg(not(target_arch = "wasm32"))]
            compile_rx: None,
            report: None,
            pages: Vec::new(),
            page_pts: Vec::new(),
            pdf_path: None,
            page_idx: 0,
            zoom: Zoom::FitWidth,
            flash: None,
            tree: Vec::new(),
            tree_scanned_at: f64::NEG_INFINITY,
            expanded: HashSet::new(),
            main_file: None,
            outline_open: true,
            show_logs: false,
            cursor_lc: (1, 1),
            goto: None,
            engines: Vec::new(),
            engine_sel: None,
            new_entry: None,
            autocompile: false,
            last_edit: f64::NEG_INFINITY,
            compiled_edit: f64::NEG_INFINITY,
        };
        app.session = Session::with_source(DEFAULT_DOC.to_string());
        #[cfg(not(target_arch = "wasm32"))]
        {
            app.engines = texcraft_engine::detect_engines();
        }
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
        // Switching files never loses work: a dirty buffer with a path is
        // auto-saved first; an untitled dirty buffer refuses to switch.
        if self.session.dirty() && self.session.path() != Some(path.as_path()) {
            if self.session.path().is_some() {
                let _ = self.session.save();
            } else {
                self.status = "unsaved buffer — save it before switching files".into();
                return;
            }
        }
        match self.session.open(&path) {
            Ok(()) => {
                self.status.clear();
                self.pages.clear();
                self.page_pts.clear();
                self.pdf_path = None;
                self.report = None;
                self.page_idx = 0;
                self.flash = None;
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

    /// The project root: the open file's directory, else the cwd.
    fn project_root(&self) -> PathBuf {
        self.session
            .path()
            .and_then(|p| p.parent().map(Path::to_path_buf))
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("."))
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
        self.compiled_edit = self.last_edit;
        self.status = "compiling…".into();
        let engine = self.engine_sel.and_then(|i| self.engines.get(i).cloned());
        let (tx, rx) = std::sync::mpsc::channel();
        self.compile_rx = Some(rx);
        std::thread::spawn(move || {
            let _ = tx.send(compile_and_render(&path, engine.as_deref()));
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
                self.page_pts = images.iter().map(|p| p.pt).collect();
                self.pages =
                    images.into_iter().enumerate().map(|(i, p)| ctx.load_texture(format!("page-{i}"), p.img, egui::TextureOptions::LINEAR)).collect();
                self.pdf_path = report.pdf.clone();
                self.page_idx = self.page_idx.min(self.pages.len().saturating_sub(1));
                self.flash = None;
                self.report = Some(report);
                self.compiling = false;
                self.compile_rx = None;
                // Surface failures without making the user open Logs.
                if self.report.as_ref().is_some_and(|r| !r.errors.is_empty()) {
                    self.show_logs = true;
                }
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

    /// Rescan the project tree at most every TREE_SCAN_SECS.
    fn refresh_tree(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        if now - self.tree_scanned_at < TREE_SCAN_SECS {
            return;
        }
        self.tree_scanned_at = now;
        let root = self.project_root();
        let expanded = &self.expanded;
        self.tree = files::scan(&root, &|p| expanded.contains(p));
        // The "main document" marker: first .tex file that declares a
        // documentclass (bounded read), else the open file.
        self.main_file = None;
        for n in &self.tree {
            if n.kind == files::Kind::Tex && file_declares_document(&n.path) {
                self.main_file = Some(n.path.clone());
                break;
            }
        }
    }

    /// Ctrl+click in the editor → SyncTeX forward: flash the matching box
    /// in the PDF preview and scroll to it.
    #[cfg(not(target_arch = "wasm32"))]
    fn forward_sync(&mut self, now: f64) {
        let (Some(pdf), Some(src)) = (self.pdf_path.clone(), self.session.path().map(Path::to_path_buf)) else {
            self.status = "compile first — no SyncTeX data".into();
            return;
        };
        // synctex records absolute source paths.
        let src = src.canonicalize().unwrap_or(src);
        let (line, col) = self.cursor_lc;
        // synctex columns are byte offsets; our cursor_lc is char-based.
        let text = self.session.source();
        let byte_col = text
            .split('\n')
            .nth(line.saturating_sub(1))
            .map(|l| l.chars().take(col.saturating_sub(1)).map(|c| c.len_utf8()).sum::<usize>() + 1)
            .unwrap_or(col);
        match texcraft_engine::synctex::forward(&pdf, &src, line as u32, byte_col as u32) {
            Ok(locs) => match locs.first() {
                Some(loc) => {
                    self.page_idx = (loc.page as usize).saturating_sub(1).min(self.pages.len().saturating_sub(1));
                    let r = egui::Rect::from_min_size(
                        egui::pos2(loc.h as f32, loc.v as f32),
                        egui::vec2(loc.width.max(4.0) as f32, loc.height.max(6.0) as f32),
                    );
                    self.flash = Some(Flash { page: self.page_idx, rect_pt: r, at: now });
                }
                None => self.status = format!("no SyncTeX record at line {line}"),
            },
            Err(e) => self.status = format!("synctex: {e}"),
        }
    }

    /// Click on a PDF page → SyncTeX inverse: move the editor caret to the
    /// source position (opening the file if the hit lands elsewhere).
    #[cfg(not(target_arch = "wasm32"))]
    fn inverse_sync(&mut self, page_rect: egui::Rect, pos: egui::Pos2) {
        let (Some(pdf), Some(pt)) = (self.pdf_path.clone(), self.page_pts.get(self.page_idx).copied()) else { return };
        let frac = (pos - page_rect.min) / page_rect.size();
        let (x, y) = ((frac.x * pt.x) as f64, (frac.y * pt.y) as f64);
        match texcraft_engine::synctex::inverse(&pdf, (self.page_idx + 1) as u32, x, y) {
            Ok(Some(loc)) => {
                // synctex reports absolute paths; compare canonicalized.
                let cur = self.session.path().and_then(|p| p.canonicalize().ok());
                if cur.as_deref() != Some(loc.file.as_path()) {
                    self.open_path(loc.file.clone());
                }
                let line = loc.line as usize;
                let col = loc.column.unwrap_or(1) as usize;
                self.goto = Some(offset_of_line_col(self.session.source(), line, col));
                self.status = format!(
                    "jumped to {}:{line}",
                    loc.file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| loc.file.display().to_string())
                );
            }
            Ok(None) => self.status = "no SyncTeX record at that point".into(),
            Err(e) => self.status = format!("synctex: {e}"),
        }
    }

    /// Auto-compile: recompile once the buffer has been idle for
    /// AUTOCOMPILE_IDLE_SECS. A failed compile marks the edit as
    /// "compiled" so we don't hot-loop — the next edit retriggers.
    #[cfg(not(target_arch = "wasm32"))]
    fn tick_autocompile(&mut self, ctx: &egui::Context) {
        if !self.autocompile || self.compiling || self.last_edit <= self.compiled_edit || self.session.path().is_none() {
            return;
        }
        let idle = ctx.input(|i| i.time) - self.last_edit;
        if idle >= AUTOCOMPILE_IDLE_SECS {
            self.start_compile();
        } else {
            ctx.request_repaint_after_secs((AUTOCOMPILE_IDLE_SECS - idle) as f32);
        }
    }

    // ---------- chrome ----------

    /// Top bar: project identity, save state, compile + toggles.
    fn top_bar(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("topbar").frame(egui::Frame::new().fill(theme::pal().top_bg).inner_margin(egui::Margin::symmetric(12, 7))).show(ui, |ui| {
            ui.horizontal(|ui| {
                // Leaf-ish mark + project name + path.
                ui.label(egui::RichText::new("❖").size(16.0).color(theme::pal().accent));
                let root = self.project_root();
                let title = self
                    .session
                    .path()
                    .and_then(|p| p.parent().and_then(|d| d.file_name()).map(|d| d.to_string_lossy().into_owned()))
                    .unwrap_or_else(|| "untitled".into());
                ui.label(egui::RichText::new(title).strong());
                ui.label(egui::RichText::new(format!("~/…/{}", root.display())).small().color(theme::pal().muted));
                let (chip, chip_col) =
                    if self.session.dirty() { ("●  unsaved changes", theme::pal().warn) } else { ("✓  saved locally", theme::pal().accent_dim) };
                ui.label(egui::RichText::new(chip).small().color(chip_col));

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // Engine pick rides on Recompile's chevron.
                    let recompile =
                        egui::Button::new(egui::RichText::new("⟳  Recompile").color(egui::Color32::WHITE).strong()).fill(theme::pal().accent);
                    let enabled = !self.compiling;
                    if ui.add_enabled(enabled, recompile).clicked() {
                        self.start_compile();
                    }
                    let menu_resp = ui
                        .add_enabled(enabled, egui::Button::new(egui::RichText::new("⌄").color(egui::Color32::WHITE)).fill(theme::pal().accent_dim))
                        .on_hover_text("compile options");
                    egui::Popup::from_toggle_button_response(&menu_resp).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
                        ui.set_min_width(140.0);
                        if ui.selectable_label(self.engine_sel.is_none(), "auto").clicked() {
                            self.engine_sel = None;
                        }
                        for (i, e) in self.engines.iter().enumerate() {
                            if ui.selectable_label(self.engine_sel == Some(i), e).clicked() {
                                self.engine_sel = Some(i);
                            }
                        }
                        ui.separator();
                        ui.checkbox(&mut self.autocompile, "auto-compile after 5 s idle");
                    });
                    if self.compiling {
                        ui.spinner();
                    }
                    ui.separator();
                    if ui.selectable_label(self.show_logs, "Logs").clicked() {
                        self.show_logs = !self.show_logs;
                    }
                    if ui.selectable_label(self.outline_open, "Outline").clicked() {
                        self.outline_open = !self.outline_open;
                    }
                    ui.separator();
                    let mode = if theme::is_dark() { "Dark" } else { "Light" };
                    if ui.button(mode).on_hover_text("switch theme").clicked() {
                        theme::apply(ui.ctx(), !theme::is_dark());
                    }
                });
            });
        });
    }

    /// Left sidebar: file toolbar, project tree, FILE OUTLINE.
    fn sidebar(&mut self, ui: &mut egui::Ui) {
        egui::Panel::left("sidebar")
            .resizable(true)
            .default_size(230.0)
            .size_range(160.0..=420.0)
            .frame(egui::Frame::new().fill(theme::pal().panel_bg).inner_margin(egui::Margin::symmetric(6, 6)))
            .show(ui, |ui| {
                // Icon row: new file / new folder / refresh / open.
                ui.horizontal(|ui| {
                    let root = self.project_root();
                    if ui.small_button("📄+").on_hover_text("new file").clicked() {
                        self.new_entry = Some(NewEntry { dir: root.clone(), name: String::new(), is_dir: false });
                    }
                    if ui.small_button("📁+").on_hover_text("new folder").clicked() {
                        self.new_entry = Some(NewEntry { dir: root.clone(), name: String::new(), is_dir: true });
                    }
                    if ui.small_button("⟳").on_hover_text("refresh tree").clicked() {
                        self.tree_scanned_at = f64::NEG_INFINITY;
                    }
                    if ui.small_button("📂").on_hover_text("open file…").clicked() {
                        self.open_dialog();
                    }
                    ui.add_space(4.0);
                });
                ui.separator();

                let tree_h = ui.available_height() * if self.outline_open { 0.55 } else { 1.0 };
                egui::ScrollArea::vertical().id_salt("tree").max_height(tree_h).auto_shrink([false, false]).show(ui, |ui| {
                    let mut open: Option<PathBuf> = None;
                    let mut toggle: Option<PathBuf> = None;
                    for n in &self.tree {
                        let is_cur = self.session.path() == Some(n.path.as_path());
                        let is_main = self.main_file.as_ref() == Some(&n.path);
                        let resp = tree_row(ui, n, is_cur, is_main, self.expanded.contains(&n.path));
                        if resp.clicked() {
                            if n.kind == files::Kind::Dir {
                                toggle = Some(n.path.clone());
                            } else {
                                open = Some(n.path.clone());
                            }
                        }
                        if resp.double_clicked() && n.kind == files::Kind::Tex && n.kind != files::Kind::Dir {
                            open = Some(n.path.clone());
                        }
                    }
                    if self.tree.is_empty() {
                        ui.weak("(empty folder)");
                    }
                    if let Some(p) = toggle {
                        if !self.expanded.remove(&p) {
                            self.expanded.insert(p);
                        }
                        self.tree_scanned_at = f64::NEG_INFINITY;
                    }
                    if let Some(p) = open {
                        self.open_path(p);
                    }
                });

                if self.outline_open {
                    ui.separator();
                    ui.label(egui::RichText::new("FILE OUTLINE").size(10.0).color(theme::pal().faint).strong());
                    egui::ScrollArea::vertical().id_salt("outline").auto_shrink([false, true]).show(ui, |ui| {
                        if self.doc.headings.is_empty() {
                            ui.weak("(no headings)");
                        }
                        let mut goto = None;
                        for h in &self.doc.headings {
                            let col = outline_color(h.level);
                            let indent = f32::from(h.level) * 10.0;
                            let resp = ui
                                .horizontal(|ui| {
                                    ui.add_space(indent);
                                    ui.add(egui::Label::new(egui::RichText::new(&h.title).color(col).size(12.0)).sense(egui::Sense::click()));
                                })
                                .response;
                            if resp.hovered() {
                                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                            }
                            if resp.clicked() {
                                goto = Some(h.pos.offset);
                            }
                        }
                        if let Some(off) = goto {
                            self.goto = Some(off);
                        }
                    });
                }
            });
    }

    /// Editor column: tab strip, guttered code editor, status strip.
    fn editor_pane(&mut self, ui: &mut egui::Ui) {
        egui::CentralPanel::default().frame(egui::Frame::new().fill(theme::pal().editor_bg)).show(ui, |ui| {
            // Tab strip
            egui::Frame::new().fill(theme::pal().top_bg).inner_margin(egui::Margin::symmetric(8, 0)).show(ui, |ui| {
                ui.horizontal(|ui| {
                    let name = self
                        .session
                        .path()
                        .and_then(|p| p.file_name())
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| "untitled.tex".into());
                    let tab_frame = egui::Frame::new()
                        .fill(theme::pal().selected)
                        .inner_margin(egui::Margin::symmetric(10, 5))
                        .stroke(egui::Stroke::new(1.0, theme::pal().border));
                    tab_frame.show(ui, |ui| {
                        ui.label(egui::RichText::new(&name).size(12.0));
                        if self.main_file.as_deref() == self.session.path() {
                            ui.label(egui::RichText::new("main document").size(9.5).color(theme::pal().accent));
                        }
                        if self.session.dirty() {
                            ui.label(egui::RichText::new("●").size(9.0).color(theme::pal().warn));
                        }
                    });
                });
            });

            // Editor body
            let mut src = self.session.source().to_string();
            let editor_id = ui.id().with("tex-editor");
            if let Some(off) = self.goto.take() {
                let mut st = egui::text_edit::TextEditState::load(ui.ctx(), editor_id).unwrap_or_default();
                let idx = src.get(..off).map(|s| s.chars().count()).unwrap_or_else(|| src.chars().count());
                st.cursor.set_char_range(Some(CCursorRange::one(CCursor { index: egui::text::CharIndex(idx), prefer_next_row: false })));
                st.store(ui.ctx(), editor_id);
                ui.ctx().memory_mut(|m| m.request_focus(editor_id));
            }
            let mut layouter = |ui: &egui::Ui, s: &dyn egui::TextBuffer, w: f32| highlight::layout(ui, s, w);
            let avail = ui.available_size();
            let out = egui::TextEdit::multiline(&mut src)
                .id(editor_id)
                .layouter(&mut layouter)
                .font(egui::TextStyle::Monospace)
                .desired_width(f32::INFINITY)
                .min_size(avail)
                .lock_focus(true)
                .frame(egui::Frame::new().fill(theme::pal().editor_bg).inner_margin(egui::Margin {
                    left: GUTTER_W as i8,
                    right: 8,
                    top: 4,
                    bottom: 4,
                }))
                .show(ui);
            if out.response.changed() {
                self.session.set_source(src.clone());
                self.last_edit = ui.ctx().input(|i| i.time);
                self.reparse();
            }
            self.paint_gutter(ui, &out, &src);
            self.track_cursor(ui.ctx(), editor_id, &src);
            #[cfg(not(target_arch = "wasm32"))]
            if out.response.clicked() && ui.ctx().input(|i| i.modifiers.command) {
                let now = ui.ctx().input(|i| i.time);
                self.forward_sync(now);
            }
        });
    }

    /// Line-number gutter + active-line band, painted into the editor's
    /// left margin. Rows come from the live galley so they track scroll.
    fn paint_gutter(&self, ui: &egui::Ui, out: &egui::text_edit::TextEditOutput, _src: &str) {
        let rect = out.response.rect;
        let band = egui::Rect::from_min_max(rect.min, egui::pos2(rect.left() + GUTTER_W, rect.bottom()));
        let painter = ui.painter().with_clip_rect(rect);
        painter.rect_filled(band, 0.0, theme::pal().editor_bg);
        painter.vline(band.right(), band.y_range(), egui::Stroke::new(1.0, theme::pal().border));

        let mono = egui::TextStyle::Monospace.resolve(ui.style());
        let (cur_line, _) = self.cursor_lc;
        for (i, placed) in out.galley.rows.iter().enumerate() {
            let y = out.galley_pos.y + placed.pos.y;
            if y < rect.top() - 4.0 || y > rect.bottom() {
                continue;
            }
            let cur = i + 1 == cur_line;
            let color = if cur { theme::pal().text } else { theme::pal().faint };
            painter.text(egui::pos2(band.right() - 8.0, y), egui::Align2::RIGHT_TOP, format!("{}", i + 1), mono.clone(), color);
        }
        // Active line wash (after the gutter so it never hides numbers).
        if let Some(placed) = out.galley.rows.get(cur_line.saturating_sub(1)) {
            let r = egui::Rect::from_min_max(
                egui::pos2(band.right(), out.galley_pos.y + placed.pos.y),
                egui::pos2(rect.right(), out.galley_pos.y + placed.pos.y + placed.row.size.y),
            );
            if r.intersects(rect) {
                painter.rect_filled(r, 0.0, egui::Color32::from_white_alpha(9));
            }
        }
    }

    /// Read the editor's caret into `cursor_lc` for the status bar.
    fn track_cursor(&mut self, ctx: &egui::Context, editor_id: egui::Id, src: &str) {
        let Some(st) = egui::text_edit::TextEditState::load(ctx, editor_id) else { return };
        let Some(range) = st.cursor.char_range() else { return };
        let char_idx = range.primary.index.0;
        // char index → (line, col), both 1-based
        let mut line = 1usize;
        let mut col = 1usize;
        for (i, c) in src.chars().enumerate() {
            if i >= char_idx {
                break;
            }
            if c == '\n' {
                line += 1;
                col = 1;
            } else {
                col += 1;
            }
        }
        self.cursor_lc = (line, col);
    }

    /// Editor-column status strip: language, caret, issues.
    fn status_bar(&mut self, ui: &mut egui::Ui) {
        egui::Panel::bottom("statusbar").frame(egui::Frame::new().fill(theme::pal().top_bg).inner_margin(egui::Margin::symmetric(10, 3))).show(
            ui,
            |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("TeX").size(10.5).color(theme::pal().syn_cmd).strong());
                    ui.separator();
                    let n = self.doc.issues.len();
                    let msg = if n == 0 { "no issues".to_string() } else { format!("⚠ {n} issue{}", if n == 1 { "" } else { "s" }) };
                    ui.label(egui::RichText::new(msg).size(10.5).color(if n == 0 { theme::pal().muted } else { theme::pal().warn }));
                    ui.separator();
                    ui.weak(&self.status);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            egui::RichText::new(format!("Ln {}, Col {}", self.cursor_lc.0, self.cursor_lc.1)).size(10.5).color(theme::pal().muted),
                        );
                    });
                });
            },
        );
    }

    /// Right pane: page navigation toolbar + single-page canvas.
    fn preview_pane(&mut self, ui: &mut egui::Ui) {
        egui::Panel::right("preview")
            .resizable(true)
            .default_size(460.0)
            .size_range(240.0..=900.0)
            .frame(egui::Frame::new().fill(theme::pal().canvas_bg))
            .show(ui, |ui| {
                // Toolbar: ‹ n/m ›  ·  - zoom% +  fit ▾
                egui::Frame::new().fill(theme::pal().top_bg).inner_margin(egui::Margin::symmetric(8, 4)).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let n = self.pages.len();
                        let has = n > 0;
                        if ui.add_enabled(has && self.page_idx > 0, egui::Button::new("‹").min_size(egui::vec2(22.0, 18.0))).clicked() {
                            self.page_idx = self.page_idx.saturating_sub(1);
                        }
                        ui.label(
                            egui::RichText::new(if has { format!("{} / {}", self.page_idx + 1, n) } else { "– / –".into() })
                                .size(11.0)
                                .color(theme::pal().muted)
                                .monospace(),
                        );
                        if ui.add_enabled(has && self.page_idx + 1 < n, egui::Button::new("›").min_size(egui::vec2(22.0, 18.0))).clicked() {
                            self.page_idx += 1;
                        }
                        ui.separator();
                        if ui.small_button("−").clicked() {
                            self.zoom = Zoom::Pct(self.zoom_pct(ui) * 0.9);
                        }
                        let pct = self.zoom_label(ui);
                        ui.label(egui::RichText::new(pct).size(11.0).color(theme::pal().muted).monospace());
                        if ui.small_button("+").clicked() {
                            self.zoom = Zoom::Pct(self.zoom_pct(ui) * 1.1);
                        }
                        egui::ComboBox::from_id_salt("fit")
                            .selected_text(match self.zoom {
                                Zoom::FitWidth => "fit width",
                                Zoom::FitPage => "fit page",
                                Zoom::Pct(_) => "custom",
                            })
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut self.zoom, Zoom::FitWidth, "fit width");
                                ui.selectable_value(&mut self.zoom, Zoom::FitPage, "fit page");
                            });
                    });
                });
                ui.separator();

                // Canvas: dark backdrop, one page centered with a shadow.
                egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| {
                    let area = ui.available_rect_before_wrap();
                    ui.painter().rect_filled(area, 0.0, theme::pal().canvas_bg);
                    let Some(tex) = self.pages.get(self.page_idx) else {
                        ui.allocate_rect(area, egui::Sense::hover());
                        ui.painter().text(
                            area.center(),
                            egui::Align2::CENTER_CENTER,
                            if self.compiling { "compiling…" } else { "no PDF yet — press Recompile" },
                            egui::FontId::proportional(13.0),
                            theme::pal().faint,
                        );
                        return;
                    };
                    let tsize = tex.size_vec2();
                    let zoom = match self.zoom {
                        Zoom::Pct(p) => p,
                        Zoom::FitWidth => (area.width() - 32.0).max(40.0) / tsize.x,
                        Zoom::FitPage => ((area.width() - 32.0) / tsize.x).min((area.height() - 32.0) / tsize.y).max(0.01),
                    };
                    let size = tsize * zoom;
                    let needed = egui::vec2(size.x + 48.0, size.y + 48.0);
                    let (rect, page_resp) =
                        ui.allocate_exact_size(egui::vec2(needed.x.max(area.width()), needed.y.max(area.height())), egui::Sense::hover());
                    let page = egui::Rect::from_center_size(rect.center(), size);
                    let painter = ui.painter();
                    painter.rect_filled(page.translate(egui::vec2(2.0, 4.0)), 2.0, egui::Color32::from_black_alpha(120));
                    painter.image(tex.id(), page, egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1.0, 1.0)), egui::Color32::WHITE);
                    painter.rect_stroke(page, 0.0, egui::Stroke::new(1.0, theme::pal().border), egui::StrokeKind::Outside);

                    // SyncTeX flash: highlight the forward-search box, fade
                    // out over FLASH_SECS, and scroll it into view once.
                    #[cfg(not(target_arch = "wasm32"))]
                    if let Some(f) = &self.flash {
                        if f.page == self.page_idx {
                            let age = ui.ctx().input(|i| i.time) - f.at;
                            let pt = self.page_pts.get(self.page_idx).copied().unwrap_or(tsize);
                            let scale = egui::vec2(page.width() / pt.x.max(1.0), page.height() / pt.y.max(1.0));
                            let r = egui::Rect::from_min_size(
                                page.min + egui::vec2(f.rect_pt.min.x * scale.x, f.rect_pt.min.y * scale.y),
                                egui::vec2(f.rect_pt.width() * scale.x, f.rect_pt.height() * scale.y),
                            );
                            if age < FLASH_SECS {
                                let a = (1.0 - (age / FLASH_SECS) as f32).clamp(0.0, 1.0);
                                painter.rect_filled(r, 2.0, theme::pal().accent.gamma_multiply(0.25 * a));
                                painter.rect_stroke(r, 2.0, egui::Stroke::new(1.5, theme::pal().accent.gamma_multiply(a)), egui::StrokeKind::Inside);
                                ui.ctx().request_repaint();
                            }
                            if age < 0.3 {
                                ui.scroll_to_rect(r.expand(90.0), Some(egui::Align::Center));
                            }
                        }
                    }

                    // SyncTeX inverse: click on the page jumps the caret.
                    #[cfg(not(target_arch = "wasm32"))]
                    {
                        let hit = ui.interact(page, ui.id().with("pdf-hit"), egui::Sense::click());
                        if hit.clicked() {
                            if let Some(pos) = hit.interact_pointer_pos() {
                                self.inverse_sync(page, pos);
                            }
                        }
                        let _ = page_resp;
                    }
                });
            });
    }

    fn zoom_pct(&self, ui: &egui::Ui) -> f32 {
        match self.zoom {
            Zoom::Pct(p) => p,
            _ => {
                // Resolve current fit into a percentage baseline.
                let Some(tex) = self.pages.get(self.page_idx) else { return 1.0 };
                let w = ui.available_width() - 32.0;
                (w.max(40.0) / tex.size_vec2().x).clamp(0.05, 8.0)
            }
        }
    }

    fn zoom_label(&self, ui: &egui::Ui) -> String {
        format!("{:.0}%", self.zoom_pct(ui) * 100.0)
    }

    /// Bottom logs sheet: errors, warnings, log tail. Declared before the
    /// preview panel so it spans the editor + preview columns.
    fn logs_panel(&mut self, ui: &mut egui::Ui) {
        if !self.show_logs {
            return;
        }
        egui::Panel::bottom("logs")
            .resizable(true)
            .default_size(130.0)
            .size_range(64.0..=400.0)
            .frame(egui::Frame::new().fill(theme::pal().top_bg).inner_margin(egui::Margin::symmetric(10, 4)))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("LOGS").size(10.0).color(theme::pal().faint).strong());
                    if let Some(r) = &self.report {
                        ui.weak(format!("{} · {} error(s) · {} warning(s)", r.engine, r.errors.len(), r.warnings.len()));
                    }
                });
                ui.separator();
                egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| {
                    if let Some(r) = &self.report {
                        for e in &r.errors {
                            let msg = match e.line {
                                Some(l) => format!("l.{l}  {}", e.message),
                                None => e.message.clone(),
                            };
                            ui.label(egui::RichText::new(msg).color(theme::pal().error).size(11.5));
                        }
                        for w in &r.warnings {
                            ui.label(egui::RichText::new(format!("warning: {}", w.message)).color(theme::pal().warn).size(11.5));
                        }
                        if !r.log_tail.is_empty() {
                            ui.add_space(4.0);
                            ui.label(egui::RichText::new(&r.log_tail).monospace().size(10.5).color(theme::pal().muted));
                        }
                    } else {
                        ui.weak("(no compile yet)");
                    }
                });
            });
    }

    /// Modal: create a file or folder in the project tree.
    fn new_entry_modal(&mut self, ctx: &egui::Context) {
        let Some(entry) = &mut self.new_entry else { return };
        let title = if entry.is_dir { "New folder" } else { "New file" };
        let mut create = false;
        let mut cancel = false;
        egui::Window::new(title).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0)).show(ctx, |ui| {
            ui.label(format!("in {}", entry.dir.display()));
            ui.add(egui::TextEdit::singleline(&mut entry.name).hint_text(if entry.is_dir { "folder name" } else { "name.tex" }).desired_width(220.0));
            ui.horizontal(|ui| {
                create = ui.button("Create").clicked();
                cancel = ui.button("Cancel").clicked();
            });
        });
        let entry = self.new_entry.take().unwrap_or_else(|| NewEntry { dir: PathBuf::new(), name: String::new(), is_dir: false });
        if cancel || entry.name.trim().is_empty() && create {
            return;
        }
        if create {
            let path = entry.dir.join(entry.name.trim());
            let res = if entry.is_dir {
                std::fs::create_dir(&path).map_err(|e| e.to_string())
            } else {
                std::fs::write(&path, "").map_err(|e| e.to_string())
            };
            match res {
                Ok(()) => {
                    self.tree_scanned_at = f64::NEG_INFINITY;
                    self.status = format!("created {}", path.display());
                }
                Err(e) => self.status = format!("cannot create {}: {e}", path.display()),
            }
        } else {
            self.new_entry = Some(entry);
        }
    }
}

/// F12 → capture the window to `texcraft-shot.png` in the cwd (this is
/// also the future `ui.screenshot` control-channel implementation).
#[cfg(not(target_arch = "wasm32"))]
fn handle_screenshot(ctx: &egui::Context) {
    use egui::{Event, ViewportCommand};
    if ctx.input(|i| i.key_pressed(egui::Key::F12)) {
        ctx.send_viewport_cmd(ViewportCommand::Screenshot(Default::default()));
    }
    let shots: Vec<egui::ColorImage> = ctx.input(|i| {
        i.events
            .iter()
            .filter_map(|e| match e {
                Event::Screenshot { image, .. } => Some(image.as_ref().clone()),
                _ => None,
            })
            .collect()
    });
    for img in shots {
        let path = std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")).join("texcraft-shot.png");
        let [w, h] = img.size;
        if let Some(buf) = image::RgbaImage::from_raw(w as u32, h as u32, img.as_raw().to_vec()) {
            let _ = buf.save(&path);
            eprintln!("screenshot -> {}", path.display());
        }
    }
}

/// Byte offset of (1-based `line`, 1-based byte `col`) in `src`.
/// Input-derived values: clamps instead of panicking.
fn offset_of_line_col(src: &str, line: usize, col: usize) -> usize {
    let mut off = 0usize;
    for (i, l) in src.split('\n').enumerate() {
        if i + 1 == line {
            return (off + col.saturating_sub(1)).min(off + l.len());
        }
        off = off.saturating_add(l.len()).saturating_add(1);
    }
    src.len()
}

/// The `pos`/`title` heading row colour, by level.
fn outline_color(level: u8) -> egui::Color32 {
    match level {
        0 | 1 => theme::pal().text,
        2 => theme::pal().accent,
        3 => theme::pal().warn,
        _ => theme::pal().muted,
    }
}

/// One file-tree row: chevron (dirs), kind icon, name, main-doc marker.
/// Returns the row response.
fn tree_row(ui: &mut egui::Ui, n: &files::Node, current: bool, is_main: bool, open: bool) -> egui::Response {
    let h = 20.0;
    let indent = n.depth as f32 * 14.0;
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), h), egui::Sense::click());
    if current || resp.hovered() {
        ui.painter().rect_filled(rect, 0.0, if current { theme::pal().selected } else { theme::pal().hovered });
    }
    if current {
        ui.painter().rect_filled(egui::Rect::from_min_size(rect.min, egui::vec2(2.5, h)), 0.0, theme::pal().accent);
    }
    let mut x = rect.left() + 6.0 + indent;
    let painter = ui.painter();
    if n.kind == files::Kind::Dir {
        // chevron
        let cy = rect.center().y;
        let pts = if open {
            vec![egui::pos2(x, cy - 2.0), egui::pos2(x + 7.0, cy - 2.0), egui::pos2(x + 3.5, cy + 3.0)]
        } else {
            vec![egui::pos2(x, cy - 3.5), egui::pos2(x, cy + 3.5), egui::pos2(x + 6.0, cy)]
        };
        painter.add(egui::Shape::convex_polygon(pts, theme::pal().muted, egui::Stroke::NONE));
        x += 12.0;
        // folder glyph: tab + body
        let (fx, fy) = (x, cy - 4.5);
        painter.rect_filled(egui::Rect::from_min_size(egui::pos2(fx, fy), egui::vec2(5.0, 3.0)), 1.0, theme::pal().warn);
        painter.rect_filled(egui::Rect::from_min_size(egui::pos2(fx, fy + 2.0), egui::vec2(11.0, 8.0)), 1.0, theme::pal().warn);
        x += 15.0;
    } else {
        x += 4.0;
        // document glyph: page with folded corner
        let (fx, fy) = (x, rect.center().y - 5.0);
        let col = match n.kind {
            files::Kind::Tex => theme::pal().syn_math,
            files::Kind::Bib => theme::pal().warn,
            files::Kind::Style => theme::pal().syn_cmd,
            files::Kind::Image => theme::pal().syn_brace,
            _ => theme::pal().muted,
        };
        let body = egui::Rect::from_min_size(egui::pos2(fx, fy), egui::vec2(9.0, 11.0));
        painter.rect_filled(body, 1.0, col);
        painter.rect_filled(egui::Rect::from_min_size(egui::pos2(fx + 6.0, fy), egui::vec2(3.0, 3.0)), 0.0, theme::pal().panel_bg);
        x += 13.0;
    }
    painter.text(
        egui::pos2(x, rect.center().y),
        egui::Align2::LEFT_CENTER,
        &n.name,
        egui::FontId::proportional(11.5),
        if current { theme::pal().text } else { theme::pal().muted },
    );
    if is_main {
        painter.text(
            egui::pos2(rect.right() - 8.0, rect.center().y),
            egui::Align2::RIGHT_CENTER,
            "★",
            egui::FontId::proportional(10.0),
            theme::pal().accent,
        );
    }
    resp.on_hover_text(n.path.display().to_string())
}

/// Cheap main-document probe: does this `.tex` contain `\documentclass`?
/// Reads at most 64 KiB — enough for any sane preamble.
fn file_declares_document(path: &Path) -> bool {
    use std::io::Read;
    let mut buf = String::new();
    let Ok(f) = std::fs::File::open(path) else { return false };
    let mut take = f.take(65_536);
    if take.read_to_string(&mut buf).is_err() {
        return false;
    }
    buf.contains("\\documentclass")
}

/// Run a system-TeX compile of `path`, then rasterize every produced PDF
/// page into an `egui::ColorImage` with `hayro`.
#[cfg(not(target_arch = "wasm32"))]
fn compile_and_render(path: &Path, engine: Option<&str>) -> CompileJob {
    let report = texcraft_engine::compile_file_with(path, None, engine).map_err(|e| e.to_string())?;
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
        images.push(Page {
            img: egui::ColorImage::from_rgba_premultiplied([usize::from(pixmap.width()), usize::from(pixmap.height())], pixmap.data_as_u8_slice()),
            pt: egui::vec2(w, h),
        });
    }
    Ok((report, images))
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    /// End-to-end: compile examples/minimal.tex via system TeX and
    /// rasterize the PDF. Skips silently when no TeX engine is installed.
    #[test]
    fn compile_and_render_minimal() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/minimal.tex");
        match compile_and_render(&path, None) {
            Ok((report, pages)) => {
                assert!(report.errors.is_empty(), "compile errors: {:?}", report.errors);
                assert!(!pages.is_empty(), "no pages rendered");
                assert!(pages.iter().all(|p| p.img.size[0] > 0 && p.img.size[1] > 0 && p.pt.x > 0.0 && p.pt.y > 0.0));
            }
            Err(e) => {
                assert!(e.contains("no TeX engine"), "unexpected failure: {e}");
            }
        }
    }
}

impl eframe::App for TexApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if ctx.input(|i| i.key_pressed(egui::Key::S) && i.modifiers.ctrl) {
            self.save();
        }
        handle_screenshot(&ctx);
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.poll_compile(&ctx);
            self.tick_autocompile(&ctx);
        }
        self.refresh_tree(&ctx);

        self.top_bar(ui);
        self.sidebar(ui);
        self.logs_panel(ui);
        self.preview_pane(ui);
        self.status_bar(ui);
        self.editor_pane(ui);
        self.new_entry_modal(&ctx);
    }
}
