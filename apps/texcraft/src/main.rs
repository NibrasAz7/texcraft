//! TexCraft Studio — `texcraft [file.tex]`.
//!
//! M0 surface: editor + live outline. The `--control PORT` JSON-lines
//! channel lands with the engine layer (see the sibling apps).
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![cfg_attr(all(target_os = "windows", not(debug_assertions)), windows_subsystem = "windows")]

use texcraft_ui_egui::TexApp;

const USAGE: &str = "TexCraft Studio\n\nusage: texcraft [file.tex]\n";

fn main() -> eframe::Result<()> {
    let mut file: Option<std::path::PathBuf> = None;
    for a in std::env::args().skip(1) {
        match a.as_str() {
            "-h" | "--help" => {
                print!("{USAGE}");
                return Ok(());
            }
            s if !s.starts_with('-') && file.is_none() => file = Some(std::path::PathBuf::from(s)),
            s => eprintln!("unknown argument `{s}`"),
        }
    }
    let native = eframe::NativeOptions { viewport: egui::ViewportBuilder::default().with_inner_size([1400.0, 900.0]), ..Default::default() };
    eframe::run_native("TexCraft", native, Box::new(move |cc| Ok(Box::new(TexApp::new(cc, file.clone())))))
}
