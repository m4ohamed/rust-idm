pub mod app;
pub mod dialogs;
pub mod i18n;
pub mod painter;
pub mod theme;
pub mod types;
pub mod worker;
#[cfg(windows)]
mod tray;

pub use app::RustIdmApp;
pub use types::*;

use eframe::egui;

fn configure_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "DejaVuSans".to_owned(),
        egui::FontData::from_static(include_bytes!("../../assets/fonts/DejaVuSans.ttf")).into(),
    );

    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts
            .families
            .entry(family)
            .or_default()
            .push("DejaVuSans".to_owned());
    }

    ctx.set_fonts(fonts);
}

pub fn run() -> eframe::Result {
    worker::start_engine().map_err(|error| {
        eframe::Error::AppCreation(Box::new(std::io::Error::other(error.to_string())))
    })?;

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1040.0, 640.0])
            .with_min_inner_size([840.0, 500.0])
            .with_title("RustIDM v2.0.0 — Pure Rust IDM Desktop Edition"),
        ..Default::default()
    };

    eframe::run_native(
        "RustIDM v2.0.0",
        options,
        Box::new(|context| {
            context.egui_ctx.set_visuals(egui::Visuals::dark());
            configure_fonts(&context.egui_ctx);
            let mut app = app::RustIdmApp::default();
            #[cfg(windows)]
            {
                app.tray = Some(
                    tray::TrayController::new(context.egui_ctx.clone())
                        .map_err(|error| std::io::Error::other(format!(
                            "Could not create system tray icon: {error}"
                        )))?,
                );
            }
            Ok(Box::new(app))
        }),
    )
}

#[cfg(target_os = "windows")]
pub fn show_startup_error(message: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};

    let text = format!("RustIDM could not start:\n\n{message}")
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let title = "RustIDM"
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            text.as_ptr(),
            title.as_ptr(),
            MB_OK | MB_ICONERROR,
        );
    }
}
