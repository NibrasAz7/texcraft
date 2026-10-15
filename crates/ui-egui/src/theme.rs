//! Studio chrome palette — dark theme matching the reference editor
//! look: deep charcoal surfaces, a single green accent, quiet borders.

use egui::{Color32, CornerRadius, Stroke, Visuals};

/// Surfaces.
pub const TOP_BG: Color32 = Color32::from_rgb(0x18, 0x1C, 0x22);
pub const PANEL_BG: Color32 = Color32::from_rgb(0x20, 0x24, 0x2C);
pub const EDITOR_BG: Color32 = Color32::from_rgb(0x1A, 0x1E, 0x25);
pub const CANVAS_BG: Color32 = Color32::from_rgb(0x14, 0x17, 0x1D);
pub const RAISED: Color32 = Color32::from_rgb(0x28, 0x2D, 0x37);
pub const SELECTED: Color32 = Color32::from_rgb(0x2B, 0x34, 0x42);
pub const HOVERED: Color32 = Color32::from_rgb(0x26, 0x2C, 0x36);
pub const BORDER: Color32 = Color32::from_rgb(0x2E, 0x34, 0x3E);

/// Text.
pub const TEXT: Color32 = Color32::from_rgb(0xD7, 0xDC, 0xE3);
pub const MUTED: Color32 = Color32::from_rgb(0x8A, 0x91, 0x9C);
pub const FAINT: Color32 = Color32::from_rgb(0x5C, 0x64, 0x70);

/// Accent + semantic.
pub const ACCENT: Color32 = Color32::from_rgb(0x3D, 0xB1, 0x83);
pub const ACCENT_DIM: Color32 = Color32::from_rgb(0x2C, 0x8A, 0x68);
pub const WARN: Color32 = Color32::from_rgb(0xE0, 0xA0, 0x50);
pub const ERROR: Color32 = Color32::from_rgb(0xE0, 0x6A, 0x6A);

/// Syntax colors for LaTeX source.
pub const SYN_CMD: Color32 = Color32::from_rgb(0xB0, 0x8C, 0xD8);
pub const SYN_MATH: Color32 = Color32::from_rgb(0x4E, 0xBF, 0xA9);
pub const SYN_BRACE: Color32 = Color32::from_rgb(0x7E, 0x9C, 0xB8);
pub const SYN_COMMENT: Color32 = Color32::from_rgb(0x6E, 0x7A, 0x5E);

/// Apply the studio dark theme. Only dark mode exists for now — the
/// light variant lands with the theme system (ROADMAP M4).
pub fn apply(ctx: &egui::Context) {
    let mut v = Visuals::dark();
    v.dark_mode = true;
    v.override_text_color = Some(TEXT);
    v.panel_fill = PANEL_BG;
    v.window_fill = TOP_BG;
    v.extreme_bg_color = EDITOR_BG;
    v.faint_bg_color = RAISED;
    v.window_stroke = Stroke::new(1.0, BORDER);
    v.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, TEXT);
    v.widgets.noninteractive.bg_fill = PANEL_BG;
    v.widgets.inactive.bg_fill = RAISED;
    v.widgets.inactive.weak_bg_fill = RAISED;
    v.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, TEXT);
    v.widgets.hovered.bg_fill = HOVERED;
    v.widgets.hovered.weak_bg_fill = HOVERED;
    v.widgets.active.bg_fill = SELECTED;
    v.widgets.active.weak_bg_fill = SELECTED;
    v.selection.bg_fill = Color32::from_rgb(0x2D, 0x4F, 0x60);
    v.selection.stroke = egui::Stroke::new(1.0, ACCENT);
    ctx.set_theme(egui::ThemePreference::Dark);
    ctx.set_visuals_of(egui::Theme::Dark, v);

    ctx.style_mut_of(egui::Theme::Dark, |style| {
        style.spacing.item_spacing = egui::vec2(6.0, 4.0);
        style.spacing.button_padding = egui::vec2(8.0, 3.0);
        style.visuals.widgets.noninteractive.corner_radius = CornerRadius::same(4);
        style.visuals.widgets.inactive.corner_radius = CornerRadius::same(4);
        style.visuals.widgets.hovered.corner_radius = CornerRadius::same(4);
        style.visuals.widgets.active.corner_radius = CornerRadius::same(4);
    });
}
