//! نظام التصميم المرئي الموحد لـ StormGet (Centralized Design System & Theme Tokens)
//!
//! يطبق متطلبات المرحلة الثانية بالكامل:
//! - متغيرات ألوان مركزية (لا توجد قيم ألوان عشوائية موزعة).
//! - دعم الوضع الداكن والفاتح بتباين قياسي واضح (WCAG >= 4.5:1).
//! - حالات تفاعل واضحة (Hover, Pressed/Active, Focused, Disabled).
//! - ألوان حالات التنزيل مقترنة دائماً بأيقونات ونصوص صريحة لعدم الاعتماد على اللون وحده.

use super::types::ThemeMode;
use crate::DownloadStatus;
use eframe::egui::{self, Color32, CornerRadius, Stroke};

#[derive(Debug, Clone, Copy)]
pub struct ThemePalette {
    pub mode: ThemeMode,
    // 1. الأسطح والخلفيات (Surfaces & Backgrounds)
    pub bg_primary: Color32,
    pub bg_secondary: Color32,
    pub sidebar_bg: Color32,
    pub window_bg: Color32,
    pub panel_bg: Color32,
    pub toolbar_bg: Color32,
    pub table_bg: Color32,
    pub table_alt_row: Color32,
    pub table_hover_row: Color32,
    pub table_selected_row: Color32,
    pub header_bg: Color32,
    // 2. النصوص (Typography Colors)
    pub text_main: Color32,
    pub text_muted: Color32,
    pub text_disabled: Color32,
    pub text_on_accent: Color32,
    // 3. الحدود والفواصل والتركيز (Borders, Dividers & Focus)
    pub border: Color32,
    pub divider: Color32,
    pub grid_line: Color32,
    pub focus_ring: Color32,
    // 4. الأزرار والتفاعل (Primary & Interactive Controls)
    pub btn_primary_bg: Color32,
    pub btn_primary_hover: Color32,
    pub btn_primary_active: Color32,
    pub btn_secondary_bg: Color32,
    pub btn_secondary_hover: Color32,
    pub btn_secondary_active: Color32,
    pub btn_disabled_bg: Color32,
    // 5. ألوان الدلالة العامة (Semantic Feedback Colors)
    pub color_success: Color32,
    pub color_warning: Color32,
    pub color_error: Color32,
    pub color_info: Color32,
    // 6. ألوان حالات التنزيل (Download State Colors)
    pub status_complete: Color32,
    pub status_downloading: Color32,
    pub status_paused: Color32,
    pub status_queued: Color32,
    pub status_failed: Color32,
    pub status_verifying: Color32,
    pub status_cancelled: Color32,
}

impl ThemePalette {
    pub fn get(mode: ThemeMode) -> Self {
        match mode {
            ThemeMode::Dark => Self {
                mode,
                bg_primary: Color32::from_rgb(20, 24, 31),
                bg_secondary: Color32::from_rgb(27, 32, 41),
                sidebar_bg: Color32::from_rgb(23, 28, 36),
                window_bg: Color32::from_rgb(22, 26, 34),
                panel_bg: Color32::from_rgb(27, 32, 41),
                toolbar_bg: Color32::from_rgb(31, 37, 48),
                table_bg: Color32::from_rgb(18, 22, 28),
                table_alt_row: Color32::from_rgb(24, 29, 37),
                table_hover_row: Color32::from_rgb(34, 44, 58),
                table_selected_row: Color32::from_rgb(26, 78, 138),
                header_bg: Color32::from_rgb(33, 40, 52),
                text_main: Color32::from_rgb(238, 242, 248),
                text_muted: Color32::from_rgb(156, 168, 186),
                text_disabled: Color32::from_rgb(102, 112, 128),
                text_on_accent: Color32::WHITE,
                border: Color32::from_rgb(54, 64, 80),
                divider: Color32::from_rgb(44, 52, 66),
                grid_line: Color32::from_rgb(36, 43, 55),
                focus_ring: Color32::from_rgb(56, 162, 255),
                btn_primary_bg: Color32::from_rgb(14, 116, 208),
                btn_primary_hover: Color32::from_rgb(28, 136, 232),
                btn_primary_active: Color32::from_rgb(10, 94, 172),
                btn_secondary_bg: Color32::from_rgb(40, 48, 62),
                btn_secondary_hover: Color32::from_rgb(54, 65, 84),
                btn_secondary_active: Color32::from_rgb(32, 40, 52),
                btn_disabled_bg: Color32::from_rgb(32, 36, 44),
                color_success: Color32::from_rgb(46, 196, 108),
                color_warning: Color32::from_rgb(240, 176, 52),
                color_error: Color32::from_rgb(238, 82, 82),
                color_info: Color32::from_rgb(64, 168, 245),
                status_complete: Color32::from_rgb(46, 196, 108),
                status_downloading: Color32::from_rgb(64, 168, 245),
                status_paused: Color32::from_rgb(235, 175, 65),
                status_queued: Color32::from_rgb(148, 163, 184),
                status_failed: Color32::from_rgb(238, 82, 82),
                status_verifying: Color32::from_rgb(168, 130, 255),
                status_cancelled: Color32::from_rgb(156, 163, 175),
            },
            ThemeMode::Light => Self {
                mode,
                bg_primary: Color32::from_rgb(248, 250, 252),
                bg_secondary: Color32::from_rgb(241, 245, 249),
                sidebar_bg: Color32::from_rgb(243, 246, 250),
                window_bg: Color32::from_rgb(246, 248, 251),
                panel_bg: Color32::from_rgb(241, 245, 249),
                toolbar_bg: Color32::from_rgb(245, 247, 250),
                table_bg: Color32::WHITE,
                table_alt_row: Color32::from_rgb(248, 250, 253),
                table_hover_row: Color32::from_rgb(232, 242, 254),
                table_selected_row: Color32::from_rgb(24, 104, 192),
                header_bg: Color32::from_rgb(236, 241, 247),
                text_main: Color32::from_rgb(18, 24, 34),
                text_muted: Color32::from_rgb(82, 95, 114),
                text_disabled: Color32::from_rgb(148, 158, 172),
                text_on_accent: Color32::WHITE,
                border: Color32::from_rgb(198, 208, 220),
                divider: Color32::from_rgb(214, 222, 232),
                grid_line: Color32::from_rgb(228, 234, 242),
                focus_ring: Color32::from_rgb(0, 112, 210),
                btn_primary_bg: Color32::from_rgb(12, 108, 198),
                btn_primary_hover: Color32::from_rgb(22, 124, 218),
                btn_primary_active: Color32::from_rgb(8, 88, 164),
                btn_secondary_bg: Color32::from_rgb(230, 236, 244),
                btn_secondary_hover: Color32::from_rgb(214, 226, 242),
                btn_secondary_active: Color32::from_rgb(196, 214, 236),
                btn_disabled_bg: Color32::from_rgb(236, 239, 243),
                color_success: Color32::from_rgb(18, 128, 56),
                color_warning: Color32::from_rgb(176, 108, 10),
                color_error: Color32::from_rgb(192, 36, 36),
                color_info: Color32::from_rgb(10, 102, 194),
                status_complete: Color32::from_rgb(18, 128, 56),
                status_downloading: Color32::from_rgb(10, 102, 194),
                status_paused: Color32::from_rgb(176, 108, 10),
                status_queued: Color32::from_rgb(92, 106, 126),
                status_failed: Color32::from_rgb(192, 36, 36),
                status_verifying: Color32::from_rgb(108, 62, 196),
                status_cancelled: Color32::from_rgb(112, 120, 132),
            },
        }
    }

    pub fn color_for_status(&self, status: DownloadStatus, is_cancelled: bool) -> Color32 {
        if is_cancelled {
            return self.status_cancelled;
        }
        match status {
            DownloadStatus::Completed => self.status_complete,
            DownloadStatus::Downloading | DownloadStatus::RefreshingUrl => self.status_downloading,
            DownloadStatus::Paused => self.status_paused,
            DownloadStatus::Queued => self.status_queued,
            DownloadStatus::Failed | DownloadStatus::NeedsUrlRefresh => self.status_failed,
            DownloadStatus::Assembling | DownloadStatus::VerifyingChecksum => self.status_verifying,
        }
    }
}

#[allow(dead_code)]
pub fn apply_idm_theme(ctx: &egui::Context, mode: ThemeMode) {
    apply_stormget_theme_with_scale(ctx, mode, 1.0);
}

thread_local! {
    static LAST_APPLIED_THEME_KEY: std::cell::Cell<Option<(ThemeMode, u32)>> = const { std::cell::Cell::new(None) };
}

pub fn apply_stormget_theme_with_scale(ctx: &egui::Context, mode: ThemeMode, font_scale: f32) {
    let scale = font_scale.clamp(0.85, 1.35);
    let key = (mode, scale.to_bits());
    // إصلاح الاختناق #4: عدم إعادة بناء ونسخ egui::Visuals و egui::Style في كل إطار رسم (60 FPS) إذا لم يتغير المظهر أو مقياس الخط
    let already_applied = LAST_APPLIED_THEME_KEY.with(|cell| cell.get() == Some(key))
        && ctx.style().visuals.dark_mode == (mode == ThemeMode::Dark);
    if already_applied {
        return;
    }
    LAST_APPLIED_THEME_KEY.with(|cell| cell.set(Some(key)));

    let p = ThemePalette::get(mode);
    let mut visuals = match mode {
        ThemeMode::Dark => egui::Visuals::dark(),
        ThemeMode::Light => egui::Visuals::light(),
    };

    visuals.panel_fill = p.panel_bg;
    visuals.window_fill = p.window_bg;
    visuals.extreme_bg_color = p.table_bg;
    visuals.faint_bg_color = p.toolbar_bg;
    visuals.window_stroke = Stroke::new(1.0_f32, p.border);
    visuals.window_corner_radius = CornerRadius::same(4);

    visuals.widgets.noninteractive.bg_fill = p.panel_bg;
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, p.text_main);
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, p.border);
    visuals.widgets.noninteractive.corner_radius = CornerRadius::same(3);

    visuals.widgets.inactive.bg_fill = p.btn_secondary_bg;
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, p.text_main);
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, p.border);
    visuals.widgets.inactive.corner_radius = CornerRadius::same(3);

    visuals.widgets.hovered.bg_fill = p.btn_secondary_hover;
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, p.text_main);
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.2_f32, p.focus_ring);
    visuals.widgets.hovered.corner_radius = CornerRadius::same(3);

    visuals.widgets.active.bg_fill = p.btn_secondary_active;
    visuals.widgets.active.fg_stroke = Stroke::new(1.2_f32, p.text_main);
    visuals.widgets.active.bg_stroke = Stroke::new(1.5_f32, p.focus_ring);
    visuals.widgets.active.corner_radius = CornerRadius::same(3);

    visuals.widgets.open.bg_fill = p.btn_secondary_active;
    visuals.widgets.open.fg_stroke = Stroke::new(1.0_f32, p.text_main);
    visuals.widgets.open.bg_stroke = Stroke::new(1.2_f32, p.focus_ring);

    visuals.selection.bg_fill = p.table_selected_row;
    visuals.selection.stroke = Stroke::new(1.0_f32, p.focus_ring);

    ctx.set_visuals(visuals);

    let scale = font_scale.clamp(0.85, 1.35);
    let mut style = (*ctx.style()).clone();
    style.text_styles.insert(
        egui::TextStyle::Small,
        egui::FontId::proportional(11.0 * scale),
    );
    style.text_styles.insert(
        egui::TextStyle::Body,
        egui::FontId::proportional(12.5 * scale),
    );
    style.text_styles.insert(
        egui::TextStyle::Button,
        egui::FontId::proportional(12.0 * scale),
    );
    style.text_styles.insert(
        egui::TextStyle::Heading,
        egui::FontId::proportional(15.5 * scale),
    );
    style.text_styles.insert(
        egui::TextStyle::Monospace,
        egui::FontId::monospace(12.0 * scale),
    );
    ctx.set_style(style);
}
