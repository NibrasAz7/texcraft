//! Extra UI fonts: every `fonts/*.ttf` found is registered with egui as a
//! per-glyph *fallback* in the Proportional and Monospace families — egui's
//! built-in fonts have no Arabic coverage, so `scripts/fetch-fonts.ps1`
//! fetches Amiri (SIL OFL; the same family the `arabdoc` template
//! typesets with) and its glyphs serve Arabic-script text everywhere.
//!
//! Discovery: `$WARRAQCRAFT_FONTS`, then `$CRAFT_FONTS_DIR`, then `fonts/`
//! next to the exe and in every ancestor dir (dev checkouts: the
//! workspace-root `fonts/`). Missing dirs/files are not an error — the
//! app runs, non-Latin glyphs just degrade to tofu as before.

use std::path::PathBuf;

/// Register all discovered `.ttf` files as fallback fonts. Call once at
/// app start, before the first frame.
pub fn install(ctx: &egui::Context) {
    let files = font_files();
    if files.is_empty() {
        return;
    }
    let mut fonts = egui::FontDefinitions::default();
    for path in files {
        let Some(name) = path.file_stem().and_then(|s| s.to_str()) else { continue };
        let Ok(bytes) = std::fs::read(&path) else { continue };
        fonts.font_data.insert(name.to_owned(), egui::FontData::from_owned(bytes).into());
        // Push at the END of each family: Latin glyphs still resolve to
        // the bundled fonts first; missing glyphs fall through to ours.
        if let Some(list) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
            list.push(name.to_owned());
        }
        if let Some(list) = fonts.families.get_mut(&egui::FontFamily::Monospace) {
            list.push(name.to_owned());
        }
    }
    ctx.set_fonts(fonts);
}

/// `*.ttf` files in every candidate dir that exists (later entries may
/// shadow earlier names — first dir wins on stem collision via `insert`).
fn font_files() -> Vec<PathBuf> {
    candidate_dirs()
        .into_iter()
        .filter(|d| d.is_dir())
        .flat_map(|d| {
            std::fs::read_dir(&d)
                .into_iter()
                .flatten()
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("ttf")))
                .collect::<Vec<_>>()
        })
        .collect()
}

fn candidate_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    for var in ["WARRAQCRAFT_FONTS", "CRAFT_FONTS_DIR"] {
        if let Some(env) = std::env::var_os(var) {
            dirs.push(PathBuf::from(env));
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            dirs.extend(dir.ancestors().map(|a| a.join("fonts")));
        }
    }
    dirs
}

#[cfg(test)]
mod tests {
    #[test]
    fn candidate_dirs_never_panics() {
        // With no env vars and a normal exe path this is just a vec of
        // paths — existence is not required.
        let _ = super::candidate_dirs();
    }
}
