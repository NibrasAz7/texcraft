//! TexCraft Studio — `texcraft [file.tex]`.
//!
//! M0 surface: editor + live outline. `--control FILE` (native only)
//! starts the opt-in TCP control channel: the app binds a random
//! loopback port and writes `{"port","token","pid"}` to FILE so an
//! agent can drive the engine session (`texcraft_ui_egui::control`).
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![cfg_attr(all(target_os = "windows", not(debug_assertions)), windows_subsystem = "windows")]

use texcraft_ui_egui::TexApp;

const USAGE: &str = "TexCraft Studio\n\nusage: texcraft [--control FILE] [file.tex]\n";

fn main() -> eframe::Result<()> {
    let mut file: Option<std::path::PathBuf> = None;
    let mut control_file: Option<std::path::PathBuf> = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "-h" | "--help" => {
                print!("{USAGE}");
                return Ok(());
            }
            "--control" => match args.next() {
                Some(f) => control_file = Some(std::path::PathBuf::from(f)),
                None => {
                    eprintln!("--control needs a control file path\n\n{USAGE}");
                    std::process::exit(1);
                }
            },
            s if !s.starts_with('-') && file.is_none() => file = Some(std::path::PathBuf::from(s)),
            s => eprintln!("unknown argument `{s}`"),
        }
    }
    // Start the channel before the GUI: `--control` was asked for, so
    // carrying on without it would leave a script driving nothing.
    let control = match &control_file {
        Some(path) => match texcraft_ui_egui::control::Control::start(path) {
            // Never the token: it stays in the owner-only file.
            Ok(c) => {
                eprintln!("control channel on 127.0.0.1:{} (connection details in {})", c.port(), path.display());
                Some(c)
            }
            Err(e) => {
                eprintln!("--control {}: {e}. Use a control file in a folder only you can write.", path.display());
                std::process::exit(1);
            }
        },
        None => None,
    };
    let native = eframe::NativeOptions { viewport: egui::ViewportBuilder::default().with_inner_size([1400.0, 900.0]), ..Default::default() };
    eframe::run_native(
        "TexCraft",
        native,
        Box::new(move |cc| {
            let mut app = TexApp::new(cc, file.clone());
            if let Some(c) = control {
                app.set_control(&cc.egui_ctx, c);
            }
            Ok(Box::new(app))
        }),
    )
}
