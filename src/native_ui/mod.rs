pub mod app;
pub mod arabic;
pub mod dialogs;
pub mod i18n;
pub mod painter;
pub mod theme;
pub mod types;
pub mod window_state;
pub mod worker;
#[cfg(windows)]
mod tray;

#[allow(unused_imports)]
pub use app::RustIdmApp;
#[allow(unused_imports)]
pub use types::*;
#[allow(unused_imports)]
pub use window_state::*;

use eframe::egui;

fn configure_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();

    // تحميل خط Segoe UI أو Tahoma من الويندوز لعرض واضح ومتناسق للعربية والإنجليزية
    let mut loaded_system_arabic = false;
    #[cfg(target_os = "windows")]
    for path in [
        "C:\\Windows\\Fonts\\segoeui.ttf",
        "C:\\Windows\\Fonts\\tahoma.ttf",
        "C:\\Windows\\Fonts\\arial.ttf",
    ] {
        if let Ok(data) = std::fs::read(path) {
            fonts.font_data.insert(
                "IDM_ArabicFont".to_owned(),
                egui::FontData::from_owned(data).into(),
            );
            loaded_system_arabic = true;
            break;
        }
    }

    // إدراج خط DejaVuSans كخط احتياطي مدمج
    fonts.font_data.insert(
        "DejaVuSans".to_owned(),
        egui::FontData::from_static(include_bytes!("../../assets/fonts/DejaVuSans.ttf")).into(),
    );

    let primary_font = if loaded_system_arabic {
        "IDM_ArabicFont"
    } else {
        "DejaVuSans"
    };

    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts
            .families
            .entry(family)
            .or_default()
            .insert(0, primary_font.to_owned());
    }

    ctx.set_fonts(fonts);
}

/// أيقونة StormGet الرسمية (64x64 RGBA) المستخرجة من الشعار الرسمي لشريط العنوان وصينية النظام
pub fn stormget_app_icon_rgba() -> (Vec<u8>, u32, u32) {
    const OFFICIAL_ICON_RGBA: &[u8] = include_bytes!("../../assets/icon_64.rgba");
    if OFFICIAL_ICON_RGBA.len() == 64 * 64 * 4 {
        return (OFFICIAL_ICON_RGBA.to_vec(), 64, 64);
    }
    (vec![0u8; 32 * 32 * 4], 32, 32)
}

pub fn run() -> eframe::Result {
    worker::start_engine().map_err(|error| {
        eframe::Error::AppCreation(Box::new(std::io::Error::other(error.to_string())))
    })?;

    let base_dir = worker::download_directory();
    let (ui_state, load_warning) = match window_state::PersistedUiState::load_from_disk(&base_dir) {
        Ok(st) => (st, None),
        Err(err) => (window_state::PersistedUiState::default(), Some(err)),
    };

    let monitors = window_state::detect_connected_monitors();
    let placement = window_state::validate_and_clamp_window_geometry(
        &ui_state.main_window,
        880.0,
        520.0,
        &monitors,
    );

    let (icon_rgba, icon_w, icon_h) = stormget_app_icon_rgba();
    let icon_data = egui::IconData {
        rgba: icon_rgba,
        width: icon_w,
        height: icon_h,
    };

    // بناء النافذة الرئيسية مع استعادة الحجم والموضع وحالة التكبير، وعدم استعادة حالة التصغير تلقائياً
    let viewport = egui::ViewportBuilder::default()
        .with_title("StormGet v2.3.6 — Professional Desktop Download Manager")
        .with_inner_size([placement.width, placement.height])
        .with_min_inner_size([880.0, 520.0])
        .with_position(egui::pos2(placement.x, placement.y))
        .with_maximized(placement.maximized)
        .with_icon(std::sync::Arc::new(icon_data));

    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        "StormGet v2.3.6",
        options,
        Box::new(move |context| {
            configure_fonts(&context.egui_ctx);
            let mut app = app::RustIdmApp::from_persisted_state(ui_state, load_warning);
            if placement.relocated_to_available_monitor {
                app.push_toast(
                    types::ToastKind::Info,
                    "استعادة موضع النافذة",
                    "تم نقل النافذة الرئيسية تلقائياً إلى الشاشة المتاحة لأن الشاشة السابقة غير متصلة.",
                    None,
                );
            }
            theme::apply_stormget_theme_with_scale(
                &context.egui_ctx,
                app.theme_mode,
                app.ui_state.font_scale,
            );
            #[cfg(windows)]
            {
                app.tray = Some(
                    tray::TrayController::new(context.egui_ctx.clone())
                        .map_err(|error| {
                            std::io::Error::other(format!(
                                "Could not create system tray icon: {error}"
                            ))
                        })?,
                );
            }
            Ok(Box::new(app))
        }),
    )
}

#[cfg(target_os = "windows")]
pub fn show_startup_error(message: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};

    let text = format!("StormGet could not start:\n\n{message}")
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let title = "StormGet"
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
