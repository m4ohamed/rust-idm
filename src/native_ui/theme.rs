use super::types::ThemeMode;
use eframe::egui::{self, Color32, Stroke};

pub struct ThemePalette {
    pub mode: ThemeMode,
    pub window_bg: Color32,
    pub panel_bg: Color32,
    pub toolbar_bg: Color32,
    pub table_bg: Color32,
    pub table_alt_row: Color32,
    pub table_selected_row: Color32,
    pub header_bg: Color32,
    pub text_main: Color32,
    pub text_muted: Color32,
    pub border: Color32,
    pub grid_line: Color32,
    pub status_complete: Color32,
    pub status_downloading: Color32,
    pub status_failed: Color32,
    pub status_paused: Color32,
}

impl ThemePalette {
    pub fn get(mode: ThemeMode) -> Self {
        match mode {
            ThemeMode::Dark => Self {
                mode,
                window_bg: Color32::from_rgb(32, 32, 32),
                panel_bg: Color32::from_rgb(36, 36, 36),
                toolbar_bg: Color32::from_rgb(42, 42, 42),
                table_bg: Color32::from_rgb(28, 28, 28),
                table_alt_row: Color32::from_rgb(35, 35, 35),
                table_selected_row: Color32::from_rgb(27, 77, 121), // IDM 6.43 Deep Navy selection
                header_bg: Color32::from_rgb(45, 45, 45),
                text_main: Color32::from_rgb(235, 235, 235),
                text_muted: Color32::from_rgb(165, 165, 165),
                border: Color32::from_rgb(60, 60, 60),
                grid_line: Color32::from_rgb(48, 48, 48),
                status_complete: Color32::from_rgb(72, 195, 105),
                status_downloading: Color32::from_rgb(85, 165, 235),
                status_failed: Color32::from_rgb(235, 80, 80),
                status_paused: Color32::from_rgb(215, 175, 95),
            },
            ThemeMode::Light => Self {
                mode,
                window_bg: Color32::from_rgb(240, 240, 240),
                panel_bg: Color32::from_rgb(240, 240, 240),
                toolbar_bg: Color32::from_rgb(244, 245, 247),
                table_bg: Color32::WHITE,
                table_alt_row: Color32::from_rgb(250, 251, 253),
                table_selected_row: Color32::from_rgb(205, 232, 255),
                header_bg: Color32::from_rgb(247, 248, 250),
                text_main: Color32::from_rgb(20, 20, 20),
                text_muted: Color32::from_rgb(90, 90, 90),
                border: Color32::from_rgb(180, 180, 180),
                grid_line: Color32::from_rgb(235, 235, 235),
                status_complete: Color32::from_rgb(16, 118, 36),
                status_downloading: Color32::from_rgb(0, 72, 186),
                status_failed: Color32::DARK_RED,
                status_paused: Color32::from_rgb(175, 120, 20),
            },
        }
    }
}

pub fn apply_idm_theme(ctx: &egui::Context, mode: ThemeMode) {
    let p = ThemePalette::get(mode);
    let mut visuals = match mode {
        ThemeMode::Dark => egui::Visuals::dark(),
        ThemeMode::Light => egui::Visuals::light(),
    };

    visuals.panel_fill = p.panel_bg;
    visuals.window_fill = p.window_bg;
    visuals.extreme_bg_color = p.table_bg;
    visuals.faint_bg_color = p.toolbar_bg;

    visuals.widgets.noninteractive.bg_fill = p.panel_bg;
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, p.text_main);
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, p.border);

    visuals.widgets.inactive.bg_fill = match mode {
        ThemeMode::Dark => Color32::from_rgb(46, 46, 46),
        ThemeMode::Light => Color32::from_rgb(228, 228, 228),
    };
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, p.text_main);
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, p.border);

    visuals.widgets.hovered.bg_fill = match mode {
        ThemeMode::Dark => Color32::from_rgb(58, 68, 82),
        ThemeMode::Light => Color32::from_rgb(229, 241, 251),
    };
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, p.text_main);
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, match mode {
        ThemeMode::Dark => Color32::from_rgb(0, 150, 255),
        ThemeMode::Light => Color32::from_rgb(0, 120, 215),
    });

    visuals.widgets.active.bg_fill = match mode {
        ThemeMode::Dark => Color32::from_rgb(35, 75, 115),
        ThemeMode::Light => Color32::from_rgb(204, 228, 247),
    };
    visuals.widgets.active.fg_stroke = Stroke::new(1.0, p.text_main);

    ctx.set_visuals(visuals);
}
