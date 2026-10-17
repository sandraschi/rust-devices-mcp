//! L3 desktop app: egui immediate-mode GUI over `devices-core`, in-process.
//!
//! ArtCraft native stack — no WebView2, no JavaScript, one `cargo` toolchain.
//! Async drivers run on a shared Tokio runtime; results cross into the UI
//! thread over an mpsc channel and the UI repaints while jobs are pending.
//! `--smoke` runs the headless parity path (for CI) without opening a window.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![deny(clippy::panic)]
#![deny(clippy::unimplemented)]
#![deny(clippy::todo)]

use eframe::egui;

mod app;
mod registry;
mod vault;

fn main() -> eframe::Result<()> {
    if std::env::args().any(|arg| arg == "--smoke") {
        return app::smoke();
    }
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(runtime) => std::sync::Arc::new(runtime),
        Err(e) => {
            eprintln!("failed to start async runtime: {e}");
            std::process::exit(1);
        }
    };
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Devices (Rust) v0.4")
        .with_inner_size([1280.0, 860.0])
        .with_min_inner_size([900.0, 600.0]);
    if let Some(icon) = load_icon() {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        "Devices (Rust)",
        options,
        Box::new(move |ctx| Ok(Box::new(app::DevicesApp::new(ctx, runtime)))),
    )
}

fn load_icon() -> Option<egui::IconData> {
    let bytes = include_bytes!("../icons/icon.png");
    let image = image::load_from_memory(bytes).ok()?;
    let rgba = image.to_rgba8();
    let (width, height) = (rgba.width(), rgba.height());
    Some(egui::IconData {
        rgba: rgba.into_raw(),
        width,
        height,
    })
}
