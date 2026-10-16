//! Studio chrome palettes — dark and light variants. All chrome reads
//! colors through `pal()` so a mode toggle restyles every surface on the
//! next frame.

use std::sync::atomic::{AtomicBool, Ordering};

use egui::{Color32, CornerRadius, Stroke, Theme, ThemePreference, Visuals};

/// One theme's full token set.
#[derive(Clone, Copy)]
pub struct Palette {
    pub top_bg: Color32,
    pub panel_bg: Color32,
    pub editor_bg: Color32,
    pub canvas_bg: Color32,
    pub raised: Color32,
    pub selected: Color32,
    pub hovered: Color32,
    pub border: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub faint: Color32,
    pub accent: Color32,
    pub accent_dim: Color32,
    pub warn: Color32,
    pub error: Color32,
    pub syn_cmd: Color32,
    pub syn_math: Color32,
    pub syn_brace: Color32,
    pub syn_comment: Color32,
}

/// Dark — deep charcoal surfaces, single green accent.
const DARK: Palette = Palette {
    top_bg: Color32::from_rgb(0x18, 0x1C, 0x22),
    panel_bg: Color32::from_rgb(0x20, 0x24, 0x2C),
    editor_bg: Color32::from_rgb(0x1A, 0x1E, 0x25),
    canvas_bg: Color32::from_rgb(0x14, 0x17, 0x1D),
    raised: Color32::from_rgb(0x28, 0x2D, 0x37),
    selected: Color32::from_rgb(0x2B, 0x34, 0x42),
    hovered: Color32::from_rgb(0x26, 0x2C, 0x36),
    border: Color32::from_rgb(0x2E, 0x34, 0x3E),
    text: Color32::from_rgb(0xD7, 0xDC, 0xE3),
    muted: Color32::from_rgb(0x8A, 0x91, 0x9C),
    faint: Color32::from_rgb(0x5C, 0x64, 0x70),
    accent: Color32::from_rgb(0x3D, 0xB1, 0x83),
    accent_dim: Color32::from_rgb(0x2C, 0x8A, 0x68),
    warn: Color32::from_rgb(0xE0, 0xA0, 0x50),
    error: Color32::from_rgb(0xE0, 0x6A, 0x6A),
    syn_cmd: Color32::from_rgb(0xB0, 0x8C, 0xD8),
    syn_math: Color32::from_rgb(0x4E, 0xBF, 0xA9),
    syn_brace: Color32::from_rgb(0x7E, 0x9C, 0xB8),
    syn_comment: Color32::from_rgb(0x6E, 0x7A, 0x5E),
};

/// Light — paper-white surfaces, same green accent (darkened for
/// contrast on white), syntax hues tuned for a light editor.
const LIGHT: Palette = Palette {
    top_bg: Color32::from_rgb(0xF2, 0xF4, 0xF7),
    panel_bg: Color32::from_rgb(0xEA, 0xED, 0xF2),
    editor_bg: Color32::from_rgb(0xFF, 0xFF, 0xFF),
    canvas_bg: Color32::from_rgb(0xDE, 0xE2, 0xE8),
    raised: Color32::from_rgb(0xE0, 0xE4, 0xEA),
    selected: Color32::from_rgb(0xD6, 0xE0, 0xEC),
    hovered: Color32::from_rgb(0xDC, 0xE2, 0xEA),
    border: Color32::from_rgb(0xC6, 0xCC, 0xD6),
    text: Color32::from_rgb(0x24, 0x28, 0x30),
    muted: Color32::from_rgb(0x5C, 0x64, 0x70),
    faint: Color32::from_rgb(0x8E, 0x96, 0xA2),
    accent: Color32::from_rgb(0x2A, 0x8F, 0x66),
    accent_dim: Color32::from_rgb(0x37, 0xA0, 0x77),
    warn: Color32::from_rgb(0xB0, 0x72, 0x18),
    error: Color32::from_rgb(0xBE, 0x40, 0x40),
    syn_cmd: Color32::from_rgb(0x7A, 0x4D, 0xB0),
    syn_math: Color32::from_rgb(0x18, 0x86, 0x6E),
    syn_brace: Color32::from_rgb(0x4E, 0x70, 0x94),
    syn_comment: Color32::from_rgb(0x7E, 0x86, 0x6E),
};

static DARK_MODE: AtomicBool = AtomicBool::new(true);

/// Whether the dark theme is active.
pub fn is_dark() -> bool {
    DARK_MODE.load(Ordering::Relaxed)
}

/// The active palette — a small `Copy` struct, fine to call per widget.
pub fn pal() -> Palette {
    if is_dark() {
        DARK
    } else {
        LIGHT
    }
}

fn visuals(p: Palette, dark: bool) -> Visuals {
    let mut v = if dark { Visuals::dark() } else { Visuals::light() };
    v.dark_mode = dark;
    v.override_text_color = Some(p.text);
    v.panel_fill = p.panel_bg;
    v.window_fill = p.top_bg;
    v.extreme_bg_color = p.editor_bg;
    v.faint_bg_color = p.raised;
    v.window_stroke = Stroke::new(1.0, p.border);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, p.text);
    v.widgets.noninteractive.bg_fill = p.panel_bg;
    v.widgets.inactive.bg_fill = p.raised;
    v.widgets.inactive.weak_bg_fill = p.raised;
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, p.text);
    v.widgets.hovered.bg_fill = p.hovered;
    v.widgets.hovered.weak_bg_fill = p.hovered;
    v.widgets.active.bg_fill = p.selected;
    v.widgets.active.weak_bg_fill = p.selected;
    v.selection.bg_fill = if dark { Color32::from_rgb(0x2D, 0x4F, 0x60) } else { Color32::from_rgb(0xC4, 0xD8, 0xEA) };
    v.selection.stroke = Stroke::new(1.0, p.accent);
    v
}

/// Register both palettes and select one. Call once at startup and on
/// every toggle — `set_theme` flips which visuals set egui uses.
pub fn apply(ctx: &egui::Context, dark: bool) {
    DARK_MODE.store(dark, Ordering::Relaxed);
    ctx.set_visuals_of(Theme::Dark, visuals(DARK, true));
    ctx.set_visuals_of(Theme::Light, visuals(LIGHT, false));
    ctx.set_theme(if dark { ThemePreference::Dark } else { ThemePreference::Light });
    for t in [Theme::Dark, Theme::Light] {
        ctx.style_mut_of(t, |style| {
            style.spacing.item_spacing = egui::vec2(6.0, 4.0);
            style.spacing.button_padding = egui::vec2(8.0, 3.0);
            style.visuals.widgets.noninteractive.corner_radius = CornerRadius::same(4);
            style.visuals.widgets.inactive.corner_radius = CornerRadius::same(4);
            style.visuals.widgets.hovered.corner_radius = CornerRadius::same(4);
            style.visuals.widgets.active.corner_radius = CornerRadius::same(4);
        });
    }
}
