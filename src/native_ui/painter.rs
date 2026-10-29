//! محرك الرسم المتجه الموحد (Unified Vector Icon & Progress Painter) لواجهة StormGet
//!
//! يطبق متطلبات المرحلة 2.3 والمرحلة 4 والمرحلة 5.2:
//! - مجموعة أيقونات متجهة متناسقة بالكامل (عدم الخلط العشوائي بين رموز نصية وأيقونات متنافرة).
//! - حالات Hover و Pressed و Focused و Disabled واضحة مع Tooltips إلزامية.
//! - شريط تقدم يدعم النسبة المئوية الفعلية أو مؤشر التقدم غير محدد النسبة (Indeterminate) عند جهل الحجم الإجمالي.
//! - رسم شارة الحالة مع أيقونة مخصصة لكل حالة (عدم الاعتماد على اللون وحده).

use super::{
    theme::ThemePalette,
    types::{ToolbarIcon, TreeIconKind},
};
use crate::{DownloadStateMeta, DownloadStatus, FileCategory, SegmentStatus};
use eframe::egui::{self, Color32, Pos2, Rect, RichText, Sense, Stroke, StrokeKind, Ui, Vec2};

#[allow(dead_code)]
pub fn idm_toolbar_button(
    ui: &mut Ui,
    palette: &ThemePalette,
    icon: ToolbarIcon,
    label: impl AsRef<str>,
    enabled: bool,
    has_dropdown: bool,
) -> bool {
    stormget_toolbar_button_with_tooltip(ui, palette, icon, label.as_ref(), "", enabled, has_dropdown)
}

pub fn stormget_toolbar_button_with_tooltip(
    ui: &mut Ui,
    palette: &ThemePalette,
    icon: ToolbarIcon,
    label: &str,
    tooltip_desc: &str,
    enabled: bool,
    has_dropdown: bool,
) -> bool {
    let width = if has_dropdown { 78.0 } else { 70.0 };
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 56.0), sense);

    let response = if !tooltip_desc.is_empty() {
        response.on_hover_text(format!("{} — {}", label, tooltip_desc))
    } else {
        response.on_hover_text(label)
    };

    let painter = ui.painter();
    if enabled {
        if response.is_pointer_button_down_on() {
            painter.rect_filled(rect, 4.0, palette.btn_secondary_active);
            painter.rect_stroke(
                rect,
                4.0,
                Stroke::new(1.2_f32, palette.focus_ring),
                StrokeKind::Inside,
            );
        } else if response.hovered() || response.has_focus() {
            painter.rect_filled(rect, 4.0, palette.btn_secondary_hover);
            painter.rect_stroke(
                rect,
                4.0,
                Stroke::new(1.0_f32, palette.focus_ring),
                StrokeKind::Inside,
            );
        }
    }

    let icon_center = Pos2::new(
        rect.center().x - if has_dropdown { 5.0 } else { 0.0 },
        rect.top() + 20.0,
    );
    draw_toolbar_vector_icon(painter, palette, icon_center, icon, enabled);

    if has_dropdown {
        let ax = rect.right() - 8.0;
        let ay = rect.top() + 20.0;
        let arrow_color = if enabled {
            palette.text_main
        } else {
            palette.text_disabled
        };
        painter.line_segment(
            [Pos2::new(ax - 3.0, ay - 1.0), Pos2::new(ax + 3.0, ay - 1.0)],
            Stroke::new(1.5_f32, arrow_color),
        );
        painter.line_segment(
            [Pos2::new(ax - 2.0, ay), Pos2::new(ax + 2.0, ay)],
            Stroke::new(1.5_f32, arrow_color),
        );
        painter.line_segment(
            [Pos2::new(ax - 1.0, ay + 1.0), Pos2::new(ax + 1.0, ay + 1.0)],
            Stroke::new(1.5_f32, arrow_color),
        );
    }

    let text_color = if enabled {
        palette.text_main
    } else {
        palette.text_disabled
    };
    painter.text(
        Pos2::new(rect.center().x, rect.bottom() - 9.0),
        egui::Align2::CENTER_CENTER,
        label,
        egui::FontId::proportional(11.0),
        text_color,
    );

    enabled && response.clicked()
}

pub fn draw_toolbar_vector_icon(
    painter: &egui::Painter,
    palette: &ThemePalette,
    c: Pos2,
    icon: ToolbarIcon,
    enabled: bool,
) {
    let disabled = palette.text_disabled;
    let badge_bg = palette.bg_secondary;
    painter.circle_filled(c, 13.5, badge_bg);

    match icon {
        ToolbarIcon::AddUrl => {
            let col = if enabled { palette.color_success } else { disabled };
            painter.circle_stroke(c, 13.5, Stroke::new(1.5_f32, col));
            let s = Stroke::new(2.2_f32, col);
            painter.line_segment([Pos2::new(c.x - 6.0, c.y), Pos2::new(c.x + 6.0, c.y)], s);
            painter.line_segment([Pos2::new(c.x, c.y - 6.0), Pos2::new(c.x, c.y + 6.0)], s);
        }
        ToolbarIcon::PasteClipboard => {
            let col = if enabled { palette.color_info } else { disabled };
            painter.circle_stroke(c, 13.5, Stroke::new(1.5_f32, col));
            let board = Rect::from_center_size(Pos2::new(c.x, c.y + 1.5), Vec2::new(11.0, 12.0));
            painter.rect_stroke(board, 1.5, Stroke::new(1.6_f32, col), StrokeKind::Inside);
            let clip = Rect::from_center_size(Pos2::new(c.x, c.y - 5.0), Vec2::new(6.0, 3.0));
            painter.rect_filled(clip, 1.0, col);
        }
        ToolbarIcon::ImportList => {
            let col = if enabled { palette.color_info } else { disabled };
            painter.circle_stroke(c, 13.5, Stroke::new(1.5_f32, col));
            let s = Stroke::new(1.8_f32, col);
            painter.line_segment([Pos2::new(c.x - 6.0, c.y + 5.0), Pos2::new(c.x + 6.0, c.y + 5.0)], s);
            painter.line_segment([Pos2::new(c.x, c.y - 6.0), Pos2::new(c.x, c.y + 2.5)], s);
            painter.line_segment([Pos2::new(c.x - 4.0, c.y - 1.5), Pos2::new(c.x, c.y + 2.5)], s);
            painter.line_segment([Pos2::new(c.x + 4.0, c.y - 1.5), Pos2::new(c.x, c.y + 2.5)], s);
        }
        ToolbarIcon::StartResume => {
            let col = if enabled { palette.status_complete } else { disabled };
            painter.circle_stroke(c, 13.5, Stroke::new(1.5_f32, col));
            let pts = vec![
                Pos2::new(c.x - 4.0, c.y - 6.0),
                Pos2::new(c.x - 4.0, c.y + 6.0),
                Pos2::new(c.x + 6.0, c.y),
            ];
            painter.add(egui::Shape::convex_polygon(pts, col, Stroke::NONE));
        }
        ToolbarIcon::Pause => {
            let col = if enabled { palette.status_paused } else { disabled };
            painter.circle_stroke(c, 13.5, Stroke::new(1.5_f32, col));
            painter.rect_filled(
                Rect::from_center_size(Pos2::new(c.x - 3.2, c.y), Vec2::new(3.0, 11.0)),
                1.0,
                col,
            );
            painter.rect_filled(
                Rect::from_center_size(Pos2::new(c.x + 3.2, c.y), Vec2::new(3.0, 11.0)),
                1.0,
                col,
            );
        }
        ToolbarIcon::Cancel | ToolbarIcon::StopAll => {
            let col = if enabled { palette.status_failed } else { disabled };
            painter.circle_stroke(c, 13.5, Stroke::new(1.5_f32, col));
            let r = Rect::from_center_size(c, Vec2::new(9.5, 9.5));
            painter.rect_filled(r, 1.5, col);
        }
        ToolbarIcon::RetryFailed => {
            let col = if enabled { palette.status_downloading } else { disabled };
            painter.circle_stroke(c, 13.5, Stroke::new(1.5_f32, col));
            painter.circle_stroke(c, 6.0, Stroke::new(1.8_f32, col));
            painter.line_segment(
                [Pos2::new(c.x + 3.0, c.y - 6.0), Pos2::new(c.x + 7.0, c.y - 3.0)],
                Stroke::new(1.8_f32, col),
            );
        }
        ToolbarIcon::Delete | ToolbarIcon::DeleteCompleted => {
            let is_comp = matches!(icon, ToolbarIcon::DeleteCompleted);
            let col = if !enabled {
                disabled
            } else if is_comp {
                palette.color_warning
            } else {
                palette.color_error
            };
            painter.circle_stroke(c, 13.5, Stroke::new(1.5_f32, col));
            let s = Stroke::new(1.6_f32, col);
            let body = Rect::from_min_max(Pos2::new(c.x - 5.5, c.y - 4.0), Pos2::new(c.x + 5.5, c.y + 6.5));
            painter.rect_stroke(body, 1.0, s, StrokeKind::Inside);
            painter.line_segment([Pos2::new(c.x - 7.0, c.y - 5.5), Pos2::new(c.x + 7.0, c.y - 5.5)], s);
            if is_comp {
                painter.line_segment([Pos2::new(c.x - 3.0, c.y + 1.0), Pos2::new(c.x - 0.5, c.y + 3.5)], s);
                painter.line_segment([Pos2::new(c.x - 0.5, c.y + 3.5), Pos2::new(c.x + 3.5, c.y - 1.5)], s);
            }
        }
        ToolbarIcon::Scheduler => {
            let col = if enabled { palette.status_verifying } else { disabled };
            painter.circle_stroke(c, 13.5, Stroke::new(1.5_f32, col));
            painter.circle_stroke(c, 7.5, Stroke::new(1.6_f32, col));
            painter.line_segment([c, Pos2::new(c.x, c.y - 4.5)], Stroke::new(1.6_f32, col));
            painter.line_segment([c, Pos2::new(c.x + 4.0, c.y + 2.0)], Stroke::new(1.6_f32, col));
        }
        ToolbarIcon::Options => {
            let col = if enabled { palette.text_main } else { disabled };
            painter.circle_stroke(c, 13.5, Stroke::new(1.5_f32, col));
            for i in 0..8 {
                let angle = (i as f32) * std::f32::consts::TAU / 8.0;
                let p1 = Pos2::new(c.x + angle.cos() * 5.0, c.y + angle.sin() * 5.0);
                let p2 = Pos2::new(c.x + angle.cos() * 8.5, c.y + angle.sin() * 8.5);
                painter.line_segment([p1, p2], Stroke::new(2.2_f32, col));
            }
            painter.circle_filled(c, 3.5, badge_bg);
            painter.circle_stroke(c, 3.5, Stroke::new(1.4_f32, col));
        }
        ToolbarIcon::StartQueue | ToolbarIcon::StopQueue => {
            let is_start = matches!(icon, ToolbarIcon::StartQueue);
            let col = if !enabled {
                disabled
            } else if is_start {
                palette.color_info
            } else {
                palette.color_warning
            };
            painter.circle_stroke(c, 13.5, Stroke::new(1.5_f32, col));
            for dy in [-4.0, 0.0, 4.0] {
                painter.line_segment(
                    [Pos2::new(c.x - 5.5, c.y + dy), Pos2::new(c.x + 5.5, c.y + dy)],
                    Stroke::new(1.8_f32, col),
                );
            }
        }
        ToolbarIcon::Grabber => {
            let col = if enabled { palette.color_info } else { disabled };
            painter.circle_stroke(c, 13.5, Stroke::new(1.5_f32, col));
            painter.circle_stroke(c, 7.5, Stroke::new(1.4_f32, col));
            painter.line_segment([Pos2::new(c.x - 7.5, c.y), Pos2::new(c.x + 7.5, c.y)], Stroke::new(1.2_f32, col));
            painter.line_segment([Pos2::new(c.x, c.y - 7.5), Pos2::new(c.x, c.y + 7.5)], Stroke::new(1.2_f32, col));
        }
        ToolbarIcon::Batch => {
            let col = if enabled { palette.status_complete } else { disabled };
            painter.circle_stroke(c, 13.5, Stroke::new(1.5_f32, col));
            for offset in [-2.5_f32, 0.0, 2.5] {
                let r = Rect::from_center_size(Pos2::new(c.x + offset * 0.5, c.y + offset), Vec2::new(10.0, 6.0));
                painter.rect_stroke(r, 1.0, Stroke::new(1.3_f32, col), StrokeKind::Inside);
            }
        }
        ToolbarIcon::ZipPreview => {
            let col = if enabled { palette.color_warning } else { disabled };
            painter.circle_stroke(c, 13.5, Stroke::new(1.5_f32, col));
            let r = Rect::from_center_size(c, Vec2::new(11.0, 11.0));
            painter.rect_stroke(r, 1.5, Stroke::new(1.5_f32, col), StrokeKind::Inside);
            painter.line_segment([Pos2::new(c.x, c.y - 5.0), Pos2::new(c.x, c.y + 5.0)], Stroke::new(1.6_f32, col));
        }
    }
}

/// رسم أيقونة حالة التنزيل المتجهة (لضمان عدم الاعتماد على اللون وحده في جدول التنزيلات)
pub fn draw_status_vector_icon(
    painter: &egui::Painter,
    c: Pos2,
    status: DownloadStatus,
    is_cancelled: bool,
    color: Color32,
) {
    painter.circle_stroke(c, 6.5, Stroke::new(1.4_f32, color));
    if is_cancelled {
        painter.line_segment([Pos2::new(c.x - 3.0, c.y - 3.0), Pos2::new(c.x + 3.0, c.y + 3.0)], Stroke::new(1.5_f32, color));
        painter.line_segment([Pos2::new(c.x + 3.0, c.y - 3.0), Pos2::new(c.x - 3.0, c.y + 3.0)], Stroke::new(1.5_f32, color));
        return;
    }
    match status {
        DownloadStatus::Completed => {
            painter.line_segment([Pos2::new(c.x - 3.0, c.y + 0.5), Pos2::new(c.x - 0.8, c.y + 3.0)], Stroke::new(1.6_f32, color));
            painter.line_segment([Pos2::new(c.x - 0.8, c.y + 3.0), Pos2::new(c.x + 3.5, c.y - 2.5)], Stroke::new(1.6_f32, color));
        }
        DownloadStatus::Downloading | DownloadStatus::RefreshingUrl => {
            painter.line_segment([Pos2::new(c.x, c.y - 3.5), Pos2::new(c.x, c.y + 3.0)], Stroke::new(1.6_f32, color));
            painter.line_segment([Pos2::new(c.x - 2.5, c.y + 0.5), Pos2::new(c.x, c.y + 3.2)], Stroke::new(1.6_f32, color));
            painter.line_segment([Pos2::new(c.x + 2.5, c.y + 0.5), Pos2::new(c.x, c.y + 3.2)], Stroke::new(1.6_f32, color));
        }
        DownloadStatus::Paused => {
            painter.line_segment([Pos2::new(c.x - 1.8, c.y - 3.0), Pos2::new(c.x - 1.8, c.y + 3.0)], Stroke::new(1.6_f32, color));
            painter.line_segment([Pos2::new(c.x + 1.8, c.y - 3.0), Pos2::new(c.x + 1.8, c.y + 3.0)], Stroke::new(1.6_f32, color));
        }
        DownloadStatus::Queued => {
            painter.line_segment([c, Pos2::new(c.x, c.y - 3.0)], Stroke::new(1.4_f32, color));
            painter.line_segment([c, Pos2::new(c.x + 2.5, c.y + 1.5)], Stroke::new(1.4_f32, color));
        }
        DownloadStatus::Failed | DownloadStatus::NeedsUrlRefresh => {
            painter.line_segment([Pos2::new(c.x, c.y - 3.5), Pos2::new(c.x, c.y + 1.0)], Stroke::new(1.8_f32, color));
            painter.circle_filled(Pos2::new(c.x, c.y + 3.2), 1.1, color);
        }
        DownloadStatus::Assembling | DownloadStatus::VerifyingChecksum => {
            painter.circle_filled(c, 2.5, color);
        }
    }
}

/// شريط تقدم ذكي داخل جدول التنزيلات ونافذة التفاصيل:
/// يعرض نسبة حقيقية إذا كان الحجم معروفا، أو شريط تقدم غير محدد النسبة (Indeterminate) إذا كان الحجم مجهولاً
pub fn draw_table_progress_cell(
    painter: &egui::Painter,
    rect: Rect,
    palette: &ThemePalette,
    task: &DownloadStateMeta,
    selected: bool,
) {
    let bar_rect = Rect::from_center_size(
        rect.center(),
        Vec2::new((rect.width() - 12.0).max(40.0), 14.0),
    );
    painter.rect_filled(bar_rect, 3.0, palette.bg_primary);
    painter.rect_stroke(
        bar_rect,
        3.0,
        Stroke::new(1.0_f32, palette.border),
        StrokeKind::Inside,
    );

    let downloaded = task.total_downloaded();
    let status_col = palette.color_for_status(task.status, task.is_cancelled_by_user());

    if task.total_size > 0 {
        let ratio = (downloaded as f32 / task.total_size as f32).clamp(0.0, 1.0);
        let fill_w = (bar_rect.width() * ratio).max(0.0);
        if fill_w > 0.0 {
            let fill_rect = Rect::from_min_size(bar_rect.min, Vec2::new(fill_w, bar_rect.height()));
            painter.rect_filled(fill_rect, 3.0, status_col);
        }
        let pct_str = format!("{:.1}%", ratio * 100.0);
        painter.text(
            bar_rect.center(),
            egui::Align2::CENTER_CENTER,
            pct_str,
            egui::FontId::proportional(10.5),
            if selected || ratio > 0.45 {
                Color32::WHITE
            } else {
                palette.text_main
            },
        );
    } else if task.status == DownloadStatus::Completed {
        painter.rect_filled(bar_rect, 3.0, palette.status_complete);
        painter.text(
            bar_rect.center(),
            egui::Align2::CENTER_CENTER,
            "100%",
            egui::FontId::proportional(10.5),
            Color32::WHITE,
        );
    } else if matches!(
        task.status,
        DownloadStatus::Downloading
            | DownloadStatus::RefreshingUrl
            | DownloadStatus::Assembling
            | DownloadStatus::VerifyingChecksum
    ) {
        // مؤشر تقدم غير محدد النسبة (Indeterminate) عند كون الحجم الإجمالي غير معروف
        let pulse_w = (bar_rect.width() * 0.35).max(16.0);
        let t = (std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() % 1400)
            .unwrap_or(400) as f32)
            / 1400.0;
        let start_x = bar_rect.left() + t * (bar_rect.width() - pulse_w);
        let ind_rect = Rect::from_min_size(
            Pos2::new(start_x, bar_rect.top()),
            Vec2::new(pulse_w, bar_rect.height()),
        );
        painter.rect_filled(ind_rect, 3.0, status_col);
        painter.text(
            bar_rect.center(),
            egui::Align2::CENTER_CENTER,
            "•••",
            egui::FontId::proportional(10.0),
            palette.text_main,
        );
    } else {
        painter.text(
            bar_rect.center(),
            egui::Align2::CENTER_CENTER,
            "—",
            egui::FontId::proportional(10.5),
            palette.text_muted,
        );
    }
}

pub fn tree_root_row_with_count(
    ui: &mut Ui,
    palette: &ThemePalette,
    expanded: &mut bool,
    label: impl AsRef<str>,
    count: usize,
    icon: TreeIconKind,
    selected: bool,
) -> bool {
    let label = label.as_ref();
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 24.0), Sense::click());
    let painter = ui.painter();

    if selected {
        painter.rect_filled(rect, 3.0, palette.table_selected_row);
    } else if resp.hovered() {
        painter.rect_filled(rect, 3.0, palette.table_hover_row);
    }

    let box_center = Pos2::new(rect.left() + 12.0, rect.center().y);
    let box_rect = Rect::from_center_size(box_center, Vec2::new(10.0, 10.0));
    painter.rect_filled(box_rect, 1.5, palette.panel_bg);
    painter.rect_stroke(box_rect, 1.5, Stroke::new(1.0_f32, palette.border), StrokeKind::Inside);
    painter.line_segment(
        [Pos2::new(box_center.x - 2.5, box_center.y), Pos2::new(box_center.x + 2.5, box_center.y)],
        Stroke::new(1.2_f32, palette.text_main),
    );
    if !*expanded {
        painter.line_segment(
            [Pos2::new(box_center.x, box_center.y - 2.5), Pos2::new(box_center.x, box_center.y + 2.5)],
            Stroke::new(1.2_f32, palette.text_main),
        );
    }

    draw_tree_icon(painter, Pos2::new(rect.left() + 28.0, rect.center().y), icon);

    let text_col = if selected {
        palette.text_on_accent
    } else {
        palette.text_main
    };
    painter.text(
        Pos2::new(rect.left() + 40.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(12.0),
        text_col,
    );

    painter.text(
        Pos2::new(rect.right() - 8.0, rect.center().y),
        egui::Align2::RIGHT_CENTER,
        format!("{count}"),
        egui::FontId::proportional(11.0),
        if selected {
            palette.text_on_accent
        } else {
            palette.text_muted
        },
    );

    if resp.clicked() {
        if let Some(pos) = resp.interact_pointer_pos() {
            if pos.x <= rect.left() + 20.0 {
                *expanded = !*expanded;
                return false;
            }
        }
        return true;
    }
    false
}

pub fn tree_child_row_with_count(
    ui: &mut Ui,
    palette: &ThemePalette,
    label: impl AsRef<str>,
    count: usize,
    icon: TreeIconKind,
    selected: bool,
) -> bool {
    let label = label.as_ref();
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 22.0), Sense::click());
    let painter = ui.painter();

    if selected {
        painter.rect_filled(rect, 3.0, palette.table_selected_row);
    } else if resp.hovered() {
        painter.rect_filled(rect, 3.0, palette.table_hover_row);
    }

    let lx = rect.left() + 22.0;
    let dot_s = Stroke::new(1.0_f32, palette.divider);
    painter.line_segment([Pos2::new(lx, rect.top()), Pos2::new(lx, rect.bottom())], dot_s);
    painter.line_segment([Pos2::new(lx, rect.center().y), Pos2::new(lx + 8.0, rect.center().y)], dot_s);

    draw_tree_icon(painter, Pos2::new(rect.left() + 36.0, rect.center().y), icon);

    let text_col = if selected {
        palette.text_on_accent
    } else {
        palette.text_main
    };
    painter.text(
        Pos2::new(rect.left() + 48.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(11.5),
        text_col,
    );

    painter.text(
        Pos2::new(rect.right() - 8.0, rect.center().y),
        egui::Align2::RIGHT_CENTER,
        format!("{count}"),
        egui::FontId::proportional(10.5),
        if selected {
            palette.text_on_accent
        } else {
            palette.text_muted
        },
    );

    resp.clicked()
}

#[allow(dead_code)]
pub fn tree_root_row(
    ui: &mut Ui,
    palette: &ThemePalette,
    expanded: &mut bool,
    label: impl AsRef<str>,
    icon: TreeIconKind,
    selected: bool,
) -> bool {
    tree_root_row_with_count(ui, palette, expanded, label, 0, icon, selected)
}

#[allow(dead_code)]
pub fn tree_child_row(
    ui: &mut Ui,
    palette: &ThemePalette,
    label: impl AsRef<str>,
    icon: TreeIconKind,
    selected: bool,
) -> bool {
    tree_child_row_with_count(ui, palette, label, 0, icon, selected)
}

pub fn draw_tree_icon(painter: &egui::Painter, c: Pos2, icon: TreeIconKind) {
    match icon {
        TreeIconKind::FolderYellow | TreeIconKind::FolderFinished => {
            let r = Rect::from_center_size(c, Vec2::new(13.0, 10.0));
            painter.rect_filled(r, 1.5, Color32::from_rgb(245, 206, 98));
            if matches!(icon, TreeIconKind::FolderFinished) {
                painter.circle_filled(Pos2::new(c.x + 2.0, c.y + 1.0), 3.5, Color32::from_rgb(46, 196, 108));
            }
        }
        TreeIconKind::FolderActive => {
            painter.circle_filled(c, 5.5, Color32::from_rgb(64, 168, 245));
        }
        TreeIconKind::FolderQueued => {
            painter.circle_filled(c, 5.5, Color32::from_rgb(148, 163, 184));
        }
        TreeIconKind::FolderPaused => {
            painter.circle_filled(c, 5.5, Color32::from_rgb(235, 175, 65));
        }
        TreeIconKind::FolderFailed => {
            painter.circle_filled(c, 5.5, Color32::from_rgb(238, 82, 82));
        }
        TreeIconKind::Compressed => {
            let r = Rect::from_center_size(c, Vec2::new(11.0, 12.0));
            painter.rect_filled(r, 1.5, Color32::from_rgb(225, 185, 110));
        }
        TreeIconKind::Document => {
            let r = Rect::from_center_size(c, Vec2::new(10.0, 12.0));
            painter.rect_filled(r, 1.5, Color32::from_rgb(215, 225, 238));
        }
        TreeIconKind::Music => {
            painter.circle_filled(Pos2::new(c.x - 2.0, c.y + 3.0), 3.0, Color32::from_rgb(65, 120, 215));
            painter.line_segment([Pos2::new(c.x, c.y + 3.0), Pos2::new(c.x, c.y - 5.0)], Stroke::new(1.5_f32, Color32::from_rgb(65, 120, 215)));
        }
        TreeIconKind::Program => {
            let r = Rect::from_center_size(c, Vec2::new(12.0, 10.0));
            painter.rect_filled(r, 1.5, Color32::from_rgb(55, 180, 125));
        }
        TreeIconKind::Video => {
            let r = Rect::from_center_size(c, Vec2::new(12.0, 10.0));
            painter.rect_filled(r, 1.5, Color32::from_rgb(155, 105, 225));
        }
        TreeIconKind::Grabber => {
            painter.circle_filled(c, 5.5, Color32::from_rgb(85, 160, 245));
            painter.circle_stroke(c, 5.5, Stroke::new(1.0_f32, Color32::WHITE));
        }
    }
}

pub fn draw_file_category_icon(painter: &egui::Painter, c: Pos2, category: FileCategory) {
    let color = match category {
        FileCategory::Music => Color32::from_rgb(65, 145, 230),
        FileCategory::Video => Color32::from_rgb(155, 105, 225),
        FileCategory::Compressed => Color32::from_rgb(225, 165, 65),
        FileCategory::Programs => Color32::from_rgb(55, 180, 125),
        FileCategory::Documents => Color32::from_rgb(230, 85, 75),
        FileCategory::General => Color32::from_rgb(135, 155, 185),
    };
    let r = Rect::from_center_size(c, Vec2::new(13.0, 14.0));
    painter.rect_filled(r, 2.0, color);
    painter.rect_filled(Rect::from_center_size(Pos2::new(c.x, c.y + 1.0), Vec2::new(7.0, 6.0)), 0.5, Color32::WHITE);
}

pub fn draw_queue_mini_icon(painter: &egui::Painter, c: Pos2, completed: bool) {
    let col = if completed {
        Color32::from_rgb(46, 196, 108)
    } else {
        Color32::from_rgb(235, 185, 80)
    };
    let r = Rect::from_center_size(c, Vec2::new(12.0, 10.0));
    painter.rect_filled(r, 1.5, col);
}

pub fn draw_idm_green_progress_bar(ui: &mut Ui, palette: &ThemePalette, progress: f32) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 18.0), Sense::hover());
    let painter = ui.painter();

    painter.rect_filled(rect, 3.0, palette.bg_primary);
    let fill_w = (rect.width() * progress.clamp(0.0, 1.0)).max(0.0);
    if fill_w > 0.0 {
        let fill_rect = Rect::from_min_size(rect.min, Vec2::new(fill_w, rect.height()));
        painter.rect_filled(fill_rect, 3.0, palette.status_complete);
    }
    painter.rect_stroke(rect, 3.0, Stroke::new(1.0_f32, palette.border), StrokeKind::Inside);
}

pub fn format_idm_bytes(bytes: u64) -> String {
    if bytes == 0 {
        return "ﻑﻭﺮﻌﻣ ﺮﻴﻏ".to_string();
    }
    if bytes >= 1024 * 1024 * 1024 {
        format!("ﺏ.ﻍ {:.3}", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    } else if bytes >= 1024 * 1024 {
        format!("ﺏ.ﻡ {:.2}", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("ﺏ.ﻙ {:.2}", bytes as f64 / 1024.0)
    } else {
        format!("ﺖﻳﺎﺑ {}", bytes)
    }
}

pub fn draw_idm_connections_bar(ui: &mut Ui, palette: &ThemePalette, task: &DownloadStateMeta, _overall_progress: f32) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 18.0), Sense::hover());
    let painter = ui.painter();

    painter.rect_filled(rect, 3.0, palette.bg_primary);

    if task.total_size > 0 && !task.segments.is_empty() {
        let total_f = task.total_size as f32;
        for (i, seg) in task.segments.iter().enumerate() {
            let start_ratio = (seg.start_byte as f32 / total_f).clamp(0.0, 1.0);
            let end_byte_clamped = seg.end_byte.min(task.total_size.saturating_sub(1));
            let end_ratio = ((end_byte_clamped + 1) as f32 / total_f).clamp(start_ratio, 1.0);
            let cur_ratio = (seg.current_offset as f32 / total_f).clamp(start_ratio, end_ratio);

            let slot_left = rect.left() + start_ratio * rect.width();
            let fill_right = rect.left() + cur_ratio * rect.width();

            if fill_right > slot_left {
                let top_half = Rect::from_min_max(Pos2::new(slot_left, rect.top()), Pos2::new(fill_right, rect.center().y));
                let bot_half = Rect::from_min_max(Pos2::new(slot_left, rect.center().y), Pos2::new(fill_right, rect.bottom()));
                painter.rect_filled(top_half, 0.0, palette.color_info);
                painter.rect_filled(bot_half, 0.0, palette.btn_primary_bg);
            }

            if i > 0 {
                let sep_x = slot_left;
                painter.line_segment([Pos2::new(sep_x, rect.top()), Pos2::new(sep_x, rect.bottom())], Stroke::new(1.0_f32, palette.border));
            }
        }
    } else {
        let step = rect.width() / 6.0;
        for i in 1..6 {
            let x = rect.left() + (i as f32) * step;
            painter.line_segment([Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())], Stroke::new(1.0_f32, palette.divider));
        }
    }

    painter.rect_stroke(rect, 3.0, Stroke::new(1.0_f32, palette.border), StrokeKind::Inside);
}

pub fn draw_live_speed_graph(ui: &mut Ui, palette: &ThemePalette, task: &DownloadStateMeta) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 95.0), Sense::hover());
    let painter = ui.painter();

    painter.rect_filled(rect, 3.0, palette.bg_primary);
    painter.rect_stroke(rect, 3.0, Stroke::new(1.0_f32, palette.border), StrokeKind::Inside);

    for i in 1..4 {
        let y = rect.top() + (i as f32) * (rect.height() / 4.0);
        painter.line_segment([Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)], Stroke::new(1.0_f32, palette.grid_line));
    }

    let speeds: Vec<f32> = if !task.speed_history.is_empty() {
        task.speed_history.iter().map(|p| p.speed_bps as f32).collect()
    } else {
        task.segments.iter().map(|s| s.last_speed_bps as f32).collect()
    };

    if speeds.is_empty() {
        return;
    }

    let max_speed = speeds.iter().copied().fold(1024.0_f32, f32::max);
    let n = speeds.len();
    if n > 1 {
        let step = rect.width() / ((n - 1) as f32);
        let mut prev = Pos2::new(rect.left(), rect.bottom() - (speeds[0] / max_speed) * (rect.height() - 8.0) - 4.0);

        for (idx, &spd) in speeds.iter().enumerate().skip(1) {
            let cur = Pos2::new(rect.left() + (idx as f32) * step, rect.bottom() - (spd / max_speed) * (rect.height() - 8.0) - 4.0);
            painter.line_segment([prev, cur], Stroke::new(2.0_f32, palette.status_downloading));
            painter.circle_filled(cur, 2.5, palette.color_info);
            prev = cur;
        }
    }
}

pub fn draw_idm_connections_table(ui: &mut Ui, palette: &ThemePalette, task: &DownloadStateMeta, _overall_progress: f32) {
    let border_stroke = Stroke::new(1.0_f32, palette.border);
    egui::Frame::canvas(ui.style())
        .fill(palette.table_bg)
        .stroke(border_stroke)
        .inner_margin(4.0)
        .show(ui, |ui| {
            egui::ScrollArea::vertical().max_height(130.0).show(ui, |ui| {
                egui::Grid::new("idm_connections_table_exact")
                    .striped(true)
                    .num_columns(4)
                    .spacing([18.0, 4.0])
                    .min_col_width(30.0)
                    .show(ui, |ui| {
                        ui.label(RichText::new("#").strong().color(palette.text_muted));
                        ui.label(RichText::new("ﻪﻠﻳﺰﻨﺗ ﻢﺗ").strong().color(palette.text_muted));
                        ui.label(RichText::new("ﺔﻋﺮﺴﻟﺍ").strong().color(palette.text_muted));
                        ui.label(RichText::new("ﻝﺎﺼﺗﻻﺍ ﺔﻟﺎﺣ").strong().color(palette.text_muted));
                        ui.end_row();

                        let is_paused = matches!(
                            task.status,
                            DownloadStatus::Paused | DownloadStatus::Failed | DownloadStatus::Queued
                        );

                        if !task.segments.is_empty() {
                            for seg in &task.segments {
                                ui.label(RichText::new(format!("{}", seg.id + 1)).monospace());

                                let downloaded = seg.downloaded_bytes();
                                let downloaded_str = if downloaded > 0 {
                                    format_idm_bytes(downloaded)
                                } else {
                                    "ﺖﻳﺎﺑ 0".to_string()
                                };
                                ui.label(RichText::new(downloaded_str).monospace());

                                let spd_str = if !is_paused && seg.last_speed_bps > 0 {
                                    format!("ﺙ{}/", format_idm_bytes(seg.last_speed_bps))
                                } else {
                                    "—".to_string()
                                };
                                ui.label(RichText::new(spd_str).monospace());

                                let info_text = if is_paused {
                                    "ﺎﺘﻗﺆﻣ ﻒﻗﻮﺘﻣ"
                                } else {
                                    match seg.status {
                                        SegmentStatus::Downloading => "...ﺕﺎﻧﺎﻴﺒﻟﺍ ﻡﻼﺘﺳﺍ ﺭﺎﺟ",
                                        SegmentStatus::Pending => "ﺭﺎﻈﺘﻧﻻﺍ ﻲﻓ",
                                        SegmentStatus::Completed => "✓ ﻞﻤﺘﻜﻣ",
                                        SegmentStatus::Failed => "ﻝﺎﺼﺗﻻﺍ ﻞﺸﻓ",
                                        SegmentStatus::Paused => "ﺎﺘﻗﺆﻣ ﻒﻗﻮﺘﻣ",
                                    }
                                };
                                ui.label(RichText::new(info_text).color(palette.text_main));
                                ui.end_row();
                            }
                        } else {
                            ui.label(RichText::new("1").monospace());
                            ui.label("—");
                            ui.label("—");
                            ui.label(RichText::new("ﻊﻃﺎﻘﻤﻟﺍ ﻢﻴﺴﻘﺗ ﺀﺪﺑ ﺭﺎﻈﺘﻧﺎﺑ").color(palette.text_muted));
                            ui.end_row();
                        }
                    });
            });
        });
}
