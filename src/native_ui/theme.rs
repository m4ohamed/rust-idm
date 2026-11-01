//! نظام التصميم المرئي الموحد لـ StormGet (Centralized Design System & Theme Tokens — Phase 3)
//!
//! يطبق متطلبات المرحلة الثالثة بالكامل:
//! - نظام `DesignTokens` مركزي يضم الرموز الـ 14 القياسية:
//!   `background`, `surface`, `surface_elevated`, `text_primary`, `text_secondary`, `text_disabled`,
//!   `border`, `accent`, `success`, `warning`, `error`, `selection`, `hover`, `focus`.
//! - تباين قياسي واضح متوافق مع معيار `WCAG AA` (>= 4.5:1) في الوضعين الفاتح والداكن.
//! - إصلاح شامل للثيم الفاتح (`Light Theme`) لمنع ظهور أي نص بلون قريب من الخلفية في النصوص الأساسية والثانوية،
//!   عناوين النوافذ، الأزرار، القوائم المنسدلة والسياقية، حقول الإدخال، العناصر المعطلة، الجداول، التلميحات، والنوافذ الثانوية.

use super::types::ThemeMode;
use crate::{
    core::desktop_services::{DesignTokens, RgbaToken},
    DownloadStatus,
};
use eframe::egui::{self, Color32, CornerRadius, Stroke};

#[inline]
pub const fn to_color32(token: RgbaToken) -> Color32 {
    Color32::from_rgba_premultiplied(token.r, token.g, token.b, token.a)
}

#[derive(Debug, Clone, Copy)]
pub struct ThemePalette {
    pub mode: ThemeMode,
    pub tokens: DesignTokens,
    // الرموز الـ 14 المركزية المباشرة (Design Tokens)
    pub background: Color32,
    pub surface: Color32,
    pub surface_elevated: Color32,
    pub text_primary: Color32,
    pub text_secondary: Color32,
    pub text_disabled: Color32,
    pub border: Color32,
    pub accent: Color32,
    pub success: Color32,
    pub warning: Color32,
    pub error: Color32,
    pub selection: Color32,
    pub hover: Color32,
    pub focus: Color32,
    //1. الأسطح والخلفيات المشتقة من الرموز المركزية (Surfaces & Backgrounds)
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
    pub text_on_accent: Color32,
    pub text_on_selection: Color32,
    // 3. الحدود والفواصل والتركيز (Borders, Dividers & Focus)
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
        let tokens = match mode {
            ThemeMode::Dark => DesignTokens::dark(),
            ThemeMode::Light => DesignTokens::light(),
        };

        let background = to_color32(tokens.background);
        let surface = to_color32(tokens.surface);
        let surface_elevated = to_color32(tokens.surface_elevated);
        let text_primary = to_color32(tokens.text_primary);
        let text_secondary = to_color32(tokens.text_secondary);
        let text_disabled = to_color32(tokens.text_disabled);
        let border = to_color32(tokens.border);
        let accent = to_color32(tokens.accent);
        let success = to_color32(tokens.success);
        let warning = to_color32(tokens.warning);
        let error = to_color32(tokens.error);
        let selection = to_color32(tokens.selection);
        let hover = to_color32(tokens.hover);
        let focus = to_color32(tokens.focus);

        match mode {
            ThemeMode::Dark => Self {
                mode,
                tokens,
                background,
                surface,
                surface_elevated,
                text_primary,
                text_secondary,
                text_disabled,
                border,
                accent,
                success,
                warning,
                error,
                selection,
                hover,
                focus,
                bg_primary: background,
                bg_secondary: surface,
                sidebar_bg: Color32::from_rgb(22, 27, 35),
                window_bg: background,
                panel_bg: surface,
                toolbar_bg: surface_elevated,
                table_bg: Color32::from_rgb(16, 20, 26),
                table_alt_row: Color32::from_rgb(23, 28, 36),
                table_hover_row: hover,
                table_selected_row: selection,
                header_bg: surface_elevated,
                text_main: text_primary,
                text_muted: text_secondary,
                text_on_accent: Color32::WHITE,
                text_on_selection: Color32::WHITE,
                divider: Color32::from_rgb(44, 52, 66),
                grid_line: Color32::from_rgb(36, 43, 55),
                focus_ring: focus,
                btn_primary_bg: accent,
                btn_primary_hover: Color32::from_rgb(28, 136, 232),
                btn_primary_active: Color32::from_rgb(10, 94, 172),
                btn_secondary_bg: surface_elevated,
                btn_secondary_hover: hover,
                btn_secondary_active: Color32::from_rgb(30, 38, 50),
                btn_disabled_bg: Color32::from_rgb(28, 33, 41),
                color_success: success,
                color_warning: warning,
                color_error: error,
                color_info: Color32::from_rgb(64, 168, 245),
                status_complete: success,
                status_downloading: Color32::from_rgb(64, 168, 245),
                status_paused: warning,
                status_queued: text_secondary,
                status_failed: error,
                status_verifying: Color32::from_rgb(168, 130, 255),
                status_cancelled: Color32::from_rgb(156, 163, 175),
            },
            ThemeMode::Light => Self {
                mode,
                tokens,
                background,
                surface,
                surface_elevated,
                text_primary,
                text_secondary,
                text_disabled,
                border,
                accent,
                success,
                warning,
                error,
                selection,
                hover,
                focus,
                bg_primary: background,
                bg_secondary: surface_elevated,
                sidebar_bg: Color32::from_rgb(238, 242, 248),
                window_bg: background,
                panel_bg: surface,
                toolbar_bg: surface_elevated,
                table_bg: surface,
                table_alt_row: Color32::from_rgb(246, 249, 253),
                table_hover_row: hover,
                table_selected_row: selection,
                header_bg: surface_elevated,
                text_main: text_primary,
                text_muted: text_secondary,
                text_on_accent: Color32::WHITE,
                text_on_selection: text_primary,
                divider: Color32::from_rgb(203, 213, 225),
                grid_line: Color32::from_rgb(218, 226, 236),
                focus_ring: focus,
                btn_primary_bg: accent,
                btn_primary_hover: Color32::from_rgb(14, 116, 208),
                btn_primary_active: Color32::from_rgb(8, 82, 156),
                btn_secondary_bg: surface_elevated,
                btn_secondary_hover: hover,
                btn_secondary_active: Color32::from_rgb(206, 218, 234),
                btn_disabled_bg: Color32::from_rgb(236, 240, 245),
                color_success: success,
                color_warning: warning,
                color_error: error,
                color_info: accent,
                status_complete: success,
                status_downloading: accent,
                status_paused: warning,
                status_queued: text_secondary,
                status_failed: error,
                status_verifying: Color32::from_rgb(109, 40, 217),
                status_cancelled: Color32::from_rgb(71, 85, 105),
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

    visuals.dark_mode = mode == ThemeMode::Dark;
    visuals.override_text_color = None;
    visuals.panel_fill = p.panel_bg;
    visuals.window_fill = p.window_bg;
    visuals.extreme_bg_color = p.table_bg;
    visuals.faint_bg_color = p.table_alt_row;
    visuals.code_bg_color = p.bg_secondary;
    visuals.warn_fg_color = p.color_warning;
    visuals.error_fg_color = p.color_error;
    visuals.hyperlink_color = p.accent;
    visuals.window_stroke = Stroke::new(1.0_f32, p.border);
    visuals.window_corner_radius = CornerRadius::same(6);
    visuals.menu_corner_radius = CornerRadius::same(6);

    // عناصر غير تفاعلية (عناوين، تسميات، فواصل، جداول)
    visuals.widgets.noninteractive.bg_fill = p.panel_bg;
    visuals.widgets.noninteractive.weak_bg_fill = p.bg_secondary;
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, p.text_primary);
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, p.border);
    visuals.widgets.noninteractive.corner_radius = CornerRadius::same(4);

    // عناصر تفاعلية في الوضع العادي (أزرار، قوائم منسدلة، مربعات اختيار، حقول إدخال)
    visuals.widgets.inactive.bg_fill = p.btn_secondary_bg;
    visuals.widgets.inactive.weak_bg_fill = p.btn_secondary_bg;
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, p.text_primary);
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, p.border);
    visuals.widgets.inactive.corner_radius = CornerRadius::same(4);

    // عناصر عند التحويم (Hover)
    visuals.widgets.hovered.bg_fill = p.hover;
    visuals.widgets.hovered.weak_bg_fill = p.hover;
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.2_f32, p.text_primary);
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.2_f32, p.focus);
    visuals.widgets.hovered.corner_radius = CornerRadius::same(4);

    // عناصر مضغوطة أو نشطة (Active / Pressed)
    visuals.widgets.active.bg_fill = p.btn_secondary_active;
    visuals.widgets.active.weak_bg_fill = p.btn_secondary_active;
    visuals.widgets.active.fg_stroke = Stroke::new(1.3_f32, p.text_primary);
    visuals.widgets.active.bg_stroke = Stroke::new(1.5_f32, p.focus);
    visuals.widgets.active.corner_radius = CornerRadius::same(4);

    // القوائم المنسدلة والقوائم السياقية المفتوحة (Open Menus & ComboBoxes)
    visuals.widgets.open.bg_fill = p.surface_elevated;
    visuals.widgets.open.weak_bg_fill = p.surface_elevated;
    visuals.widgets.open.fg_stroke = Stroke::new(1.1_f32, p.text_primary);
    visuals.widgets.open.bg_stroke = Stroke::new(1.2_f32, p.focus);
    visuals.widgets.open.corner_radius = CornerRadius::same(4);

    // التحديد (Selection in Tables, Selectable Labels, TextEdits)
    // في الوضع الفاتح تكون خلفية التحديد فاتحة (#d6e8ff) والنص داكناً (#0f172a) لضمان قراءة كافة التبويبات والصفوف بوضوح تام!
    visuals.selection.bg_fill = p.selection;
    visuals.selection.stroke = Stroke::new(1.2_f32, p.text_on_selection);

    ctx.set_visuals(visuals);

    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = egui::vec2(6.0, 5.0);
    style.spacing.button_padding = egui::vec2(8.0, 4.0);
    style.text_styles.insert(
        egui::TextStyle::Small,
        egui::FontId::proportional(11.2 * scale),
    );
    style.text_styles.insert(
        egui::TextStyle::Body,
        egui::FontId::proportional(12.8 * scale),
    );
    style.text_styles.insert(
        egui::TextStyle::Button,
        egui::FontId::proportional(12.2 * scale),
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
