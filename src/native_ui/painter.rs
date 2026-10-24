use super::{
    theme::ThemePalette,
    types::{ThemeMode, ToolbarIcon, TreeIconKind},
};
use crate::{DownloadStateMeta, DownloadStatus, FileCategory, SegmentStatus};
use eframe::egui::{self, Color32, Pos2, Rect, RichText, Sense, Stroke, StrokeKind, Ui, Vec2};

pub fn idm_toolbar_button(
    ui: &mut Ui,
    palette: &ThemePalette,
    icon: ToolbarIcon,
    label: impl AsRef<str>,
    enabled: bool,
    has_dropdown: bool,
) -> bool {
    let label = label.as_ref();
    let width = if has_dropdown { 74.0 } else { 64.0 };
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 58.0), Sense::click());

    let painter = ui.painter();
    if enabled && response.hovered() {
        let hover_bg = match palette.mode {
            ThemeMode::Dark => Color32::from_rgb(52, 60, 72),
            ThemeMode::Light => Color32::from_rgb(225, 236, 248),
        };
        painter.rect_filled(rect, 3.0, hover_bg);
    }

    let icon_center = Pos2::new(
        rect.center().x - if has_dropdown { 5.0 } else { 0.0 },
        rect.top() + 21.0,
    );
    draw_toolbar_vector_icon(painter, palette, icon_center, icon, enabled);

    if has_dropdown {
        let ax = rect.right() - 8.0;
        let ay = rect.top() + 22.0;
        let arrow_color = if enabled {
            palette.text_main
        } else {
            palette.text_muted
        };
        painter.line_segment(
            [Pos2::new(ax - 3.0, ay - 1.0), Pos2::new(ax + 3.0, ay - 1.0)],
            Stroke::new(1.5, arrow_color),
        );
        painter.line_segment(
            [Pos2::new(ax - 2.0, ay), Pos2::new(ax + 2.0, ay)],
            Stroke::new(1.5, arrow_color),
        );
        painter.line_segment(
            [Pos2::new(ax - 1.0, ay + 1.0), Pos2::new(ax + 1.0, ay + 1.0)],
            Stroke::new(1.5, arrow_color),
        );
    }

    let text_color = if enabled {
        palette.text_main
    } else {
        palette.text_muted
    };
    painter.text(
        Pos2::new(rect.center().x, rect.bottom() - 9.0),
        egui::Align2::CENTER_CENTER,
        label,
        egui::FontId::proportional(11.5),
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
    let disabled = Color32::from_rgb(130, 130, 130);
    match icon {
        ToolbarIcon::AddUrl => {
            let col = if enabled {
                Color32::from_rgb(92, 194, 136)
            } else {
                disabled
            };
            let s = Stroke::new(1.8, col);
            let fill = match palette.mode {
                ThemeMode::Dark => Color32::from_rgb(32, 54, 40),
                ThemeMode::Light => Color32::from_rgb(230, 246, 236),
            };
            painter.rect_filled(Rect::from_center_size(c, Vec2::new(8.0, 26.0)), 1.5, fill);
            painter.rect_filled(Rect::from_center_size(c, Vec2::new(26.0, 8.0)), 1.5, fill);
            painter.line_segment([Pos2::new(c.x - 4.0, c.y - 13.0), Pos2::new(c.x + 4.0, c.y - 13.0)], s);
            painter.line_segment([Pos2::new(c.x - 4.0, c.y + 13.0), Pos2::new(c.x + 4.0, c.y + 13.0)], s);
            painter.line_segment([Pos2::new(c.x - 13.0, c.y - 4.0), Pos2::new(c.x - 13.0, c.y + 4.0)], s);
            painter.line_segment([Pos2::new(c.x + 13.0, c.y - 4.0), Pos2::new(c.x + 13.0, c.y + 4.0)], s);
            painter.line_segment([Pos2::new(c.x - 4.0, c.y - 13.0), Pos2::new(c.x - 4.0, c.y - 4.0)], s);
            painter.line_segment([Pos2::new(c.x + 4.0, c.y - 13.0), Pos2::new(c.x + 4.0, c.y - 4.0)], s);
            painter.line_segment([Pos2::new(c.x - 4.0, c.y + 4.0), Pos2::new(c.x - 4.0, c.y + 13.0)], s);
            painter.line_segment([Pos2::new(c.x + 4.0, c.y + 4.0), Pos2::new(c.x + 4.0, c.y + 13.0)], s);
            painter.line_segment([Pos2::new(c.x - 13.0, c.y - 4.0), Pos2::new(c.x - 4.0, c.y - 4.0)], s);
            painter.line_segment([Pos2::new(c.x + 4.0, c.y - 4.0), Pos2::new(c.x + 13.0, c.y - 4.0)], s);
            painter.line_segment([Pos2::new(c.x - 13.0, c.y + 4.0), Pos2::new(c.x - 4.0, c.y + 4.0)], s);
            painter.line_segment([Pos2::new(c.x + 4.0, c.y + 4.0), Pos2::new(c.x + 13.0, c.y + 4.0)], s);
        }
        ToolbarIcon::Resume => {
            let col = if enabled {
                Color32::from_rgb(85, 185, 115)
            } else {
                disabled
            };
            let bg = match palette.mode {
                ThemeMode::Dark => Color32::from_rgb(38, 52, 42),
                ThemeMode::Light => Color32::from_rgb(236, 236, 236),
            };
            painter.circle_filled(c, 14.0, bg);
            painter.circle_stroke(c, 14.0, Stroke::new(1.6, col));
            let s = Stroke::new(1.8, col);
            painter.line_segment([Pos2::new(c.x, c.y - 6.0), Pos2::new(c.x, c.y + 6.0)], s);
            painter.line_segment([Pos2::new(c.x - 5.0, c.y + 1.0), Pos2::new(c.x, c.y + 6.0)], s);
            painter.line_segment([Pos2::new(c.x + 5.0, c.y + 1.0), Pos2::new(c.x, c.y + 6.0)], s);
        }
        ToolbarIcon::Stop => {
            let col = if enabled {
                Color32::from_rgb(175, 115, 205)
            } else {
                disabled
            };
            let bg = match palette.mode {
                ThemeMode::Dark => Color32::from_rgb(50, 38, 58),
                ThemeMode::Light => Color32::from_rgb(238, 232, 242),
            };
            painter.circle_filled(c, 14.0, bg);
            painter.circle_stroke(c, 14.0, Stroke::new(1.6, col));
            let r = Rect::from_center_size(c, Vec2::new(9.0, 9.0));
            painter.rect_filled(r, 1.0, Color32::WHITE);
            let s = Stroke::new(1.6, col);
            painter.line_segment([r.left_top(), r.right_top()], s);
            painter.line_segment([r.right_top(), r.right_bottom()], s);
            painter.line_segment([r.right_bottom(), r.left_bottom()], s);
            painter.line_segment([r.left_bottom(), r.left_top()], s);
        }
        ToolbarIcon::StopAll => {
            let col = if enabled {
                Color32::from_rgb(215, 105, 115)
            } else {
                disabled
            };
            let s = Stroke::new(1.6, col);
            let palm = Rect::from_min_max(Pos2::new(c.x - 8.0, c.y - 2.0), Pos2::new(c.x + 8.0, c.y + 12.0));
            let bg = match palette.mode {
                ThemeMode::Dark => Color32::from_rgb(58, 38, 42),
                ThemeMode::Light => Color32::from_rgb(246, 232, 234),
            };
            painter.rect_filled(palm, 3.0, bg);
            for dx in [-6.0, -2.0, 2.0, 6.0] {
                painter.line_segment(
                    [Pos2::new(c.x + dx, c.y - 12.0), Pos2::new(c.x + dx, c.y + 2.0)],
                    Stroke::new(2.4, col),
                );
            }
            painter.line_segment([palm.left_bottom(), palm.right_bottom()], s);
            painter.line_segment([palm.left_top(), palm.left_bottom()], s);
            painter.line_segment([palm.right_top(), palm.right_bottom()], s);
        }
        ToolbarIcon::Delete | ToolbarIcon::DeleteCompleted => {
            let is_comp = matches!(icon, ToolbarIcon::DeleteCompleted);
            let col = if !enabled {
                disabled
            } else if is_comp {
                Color32::from_rgb(210, 95, 155)
            } else {
                Color32::from_rgb(175, 175, 175)
            };
            let s = Stroke::new(1.6, col);
            let body = Rect::from_min_max(Pos2::new(c.x - 9.0, c.y - 7.0), Pos2::new(c.x + 9.0, c.y + 13.0));
            let bg = match palette.mode {
                ThemeMode::Dark => Color32::from_rgb(52, 40, 48),
                ThemeMode::Light => Color32::from_rgb(245, 238, 242),
            };
            painter.rect_filled(body, 1.5, bg);
            painter.line_segment([body.left_top(), body.right_top()], s);
            painter.line_segment([body.left_top(), body.left_bottom()], s);
            painter.line_segment([body.right_top(), body.right_bottom()], s);
            painter.line_segment([body.left_bottom(), body.right_bottom()], s);
            painter.line_segment([Pos2::new(c.x - 11.0, c.y - 9.0), Pos2::new(c.x + 11.0, c.y - 9.0)], s);
            painter.line_segment([Pos2::new(c.x - 4.0, c.y - 12.0), Pos2::new(c.x + 4.0, c.y - 12.0)], s);
            if is_comp {
                painter.line_segment([Pos2::new(c.x - 4.0, c.y + 3.0), Pos2::new(c.x - 1.0, c.y + 7.0)], s);
                painter.line_segment([Pos2::new(c.x - 1.0, c.y + 7.0), Pos2::new(c.x + 5.0, c.y - 2.0)], s);
            } else {
                for dx in [-4.0, 0.0, 4.0] {
                    painter.line_segment([Pos2::new(c.x + dx, c.y - 3.0), Pos2::new(c.x + dx, c.y + 9.0)], s);
                }
            }
        }
        ToolbarIcon::Options => {
            let col = Color32::from_rgb(165, 145, 220);
            for i in 0..8 {
                let angle = (i as f32) * std::f32::consts::TAU / 8.0;
                let p1 = Pos2::new(c.x + angle.cos() * 8.0, c.y + angle.sin() * 8.0);
                let p2 = Pos2::new(c.x + angle.cos() * 14.0, c.y + angle.sin() * 14.0);
                painter.line_segment([p1, p2], Stroke::new(3.2, col));
            }
            painter.circle_filled(c, 10.5, Color32::from_rgb(60, 52, 78));
            painter.circle_stroke(c, 10.5, Stroke::new(1.6, col));
            painter.circle_filled(c, 4.5, palette.panel_bg);
            painter.circle_stroke(c, 4.5, Stroke::new(1.4, col));
        }
        ToolbarIcon::Scheduler => {
            let col = Color32::from_rgb(195, 145, 205);
            painter.circle_filled(c, 13.0, Color32::from_rgb(62, 50, 72));
            painter.circle_stroke(c, 13.0, Stroke::new(1.7, col));
            painter.line_segment([Pos2::new(c.x - 12.0, c.y - 10.0), Pos2::new(c.x - 7.0, c.y - 14.0)], Stroke::new(2.0, col));
            painter.line_segment([Pos2::new(c.x + 7.0, c.y - 14.0), Pos2::new(c.x + 12.0, c.y - 10.0)], Stroke::new(2.0, col));
            painter.line_segment([c, Pos2::new(c.x, c.y - 7.0)], Stroke::new(1.8, col));
            painter.line_segment([c, Pos2::new(c.x + 5.0, c.y + 3.0)], Stroke::new(1.8, col));
        }
        ToolbarIcon::StartQueue | ToolbarIcon::StopQueue => {
            let is_start = matches!(icon, ToolbarIcon::StartQueue);
            let col = if is_start {
                Color32::from_rgb(115, 185, 225)
            } else {
                Color32::from_rgb(175, 135, 210)
            };
            let s = Stroke::new(1.5, col);
            let r1 = Rect::from_min_max(Pos2::new(c.x - 11.0, c.y - 12.0), Pos2::new(c.x + 5.0, c.y + 6.0));
            let r2 = Rect::from_min_max(Pos2::new(c.x - 6.0, c.y - 7.0), Pos2::new(c.x + 10.0, c.y + 11.0));
            painter.rect_filled(r1, 1.5, Color32::from_rgb(45, 60, 75));
            painter.line_segment([r1.left_top(), r1.right_top()], s);
            painter.line_segment([r1.left_top(), r1.left_bottom()], s);
            painter.rect_filled(r2, 1.5, Color32::from_rgb(32, 42, 54));
            painter.line_segment([r2.left_top(), r2.right_top()], s);
            painter.line_segment([r2.right_top(), r2.right_bottom()], s);
            painter.line_segment([r2.left_bottom(), r2.right_bottom()], s);
            painter.line_segment([r2.left_top(), r2.left_bottom()], s);
            if is_start {
                let ac = Pos2::new(c.x + 6.0, c.y + 7.0);
                painter.line_segment([Pos2::new(ac.x, ac.y - 5.0), Pos2::new(ac.x, ac.y + 4.0)], Stroke::new(1.8, Color32::from_rgb(70, 195, 145)));
            } else {
                let sq = Rect::from_center_size(Pos2::new(c.x + 6.0, c.y + 7.0), Vec2::new(8.0, 8.0));
                painter.rect_filled(sq, 1.0, Color32::from_rgb(205, 80, 135));
            }
        }
        ToolbarIcon::Grabber => {
            let col = if enabled {
                Color32::from_rgb(75, 155, 245)
            } else {
                disabled
            };
            painter.circle_filled(c, 11.5, Color32::from_rgb(35, 52, 75));
            painter.circle_stroke(c, 11.5, Stroke::new(1.6, col));
            painter.circle_stroke(c, 6.0, Stroke::new(1.1, col));
            painter.line_segment([Pos2::new(c.x - 11.0, c.y), Pos2::new(c.x + 11.0, c.y)], Stroke::new(1.2, col));
            painter.line_segment([Pos2::new(c.x, c.y - 11.0), Pos2::new(c.x, c.y + 11.0)], Stroke::new(1.2, col));
        }
        ToolbarIcon::Batch => {
            let col = if enabled {
                Color32::from_rgb(60, 175, 105)
            } else {
                disabled
            };
            let s = Stroke::new(1.4, col);
            for offset in [-3.5_f32, 0.0, 3.5] {
                let r = Rect::from_center_size(Pos2::new(c.x + offset * 0.6, c.y + offset), Vec2::new(16.0, 10.0));
                painter.rect_filled(r, 1.5, Color32::from_rgb(35, 62, 45));
                painter.line_segment([r.left_top(), r.right_top()], s);
                painter.line_segment([r.right_top(), r.right_bottom()], s);
                painter.line_segment([r.right_bottom(), r.left_bottom()], s);
                painter.line_segment([r.left_bottom(), r.left_top()], s);
            }
        }
        ToolbarIcon::TellFriend => {
            let col = Color32::from_rgb(165, 155, 235);
            let s = Stroke::new(1.6, col);
            let box_r = Rect::from_min_max(Pos2::new(c.x - 9.0, c.y - 3.0), Pos2::new(c.x + 10.0, c.y + 11.0));
            painter.rect_filled(box_r, 3.0, Color32::from_rgb(52, 48, 72));
            painter.line_segment([box_r.left_top(), box_r.right_top()], s);
            painter.line_segment([box_r.left_bottom(), box_r.right_bottom()], s);
            painter.line_segment([box_r.left_top(), box_r.left_bottom()], s);
            painter.line_segment([box_r.right_top(), box_r.right_bottom()], s);
            painter.line_segment([Pos2::new(c.x - 2.0, c.y - 3.0), Pos2::new(c.x + 1.0, c.y - 11.0)], Stroke::new(3.0, col));
        }
    }
}

pub fn tree_root_row(
    ui: &mut Ui,
    palette: &ThemePalette,
    expanded: &mut bool,
    label: impl AsRef<str>,
    icon: TreeIconKind,
    selected: bool,
) -> bool {
    let label = label.as_ref();
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 20.0), Sense::click());
    let painter = ui.painter();

    if selected {
        painter.rect_filled(
            Rect::from_min_max(Pos2::new(rect.left() + 36.0, rect.top()), rect.right_bottom()),
            1.0,
            palette.table_selected_row,
        );
    }

    let box_center = Pos2::new(rect.left() + 12.0, rect.center().y);
    let box_rect = Rect::from_center_size(box_center, Vec2::new(9.0, 9.0));
    painter.rect_filled(box_rect, 0.0, palette.panel_bg);
    let box_s = Stroke::new(1.0_f32, palette.border);
    painter.line_segment([box_rect.left_top(), box_rect.right_top()], box_s);
    painter.line_segment([box_rect.right_top(), box_rect.right_bottom()], box_s);
    painter.line_segment([box_rect.right_bottom(), box_rect.left_bottom()], box_s);
    painter.line_segment([box_rect.left_bottom(), box_rect.left_top()], box_s);
    painter.line_segment(
        [Pos2::new(box_center.x - 2.5, box_center.y), Pos2::new(box_center.x + 2.5, box_center.y)],
        Stroke::new(1.0_f32, palette.text_main),
    );
    if !*expanded {
        painter.line_segment(
            [Pos2::new(box_center.x, box_center.y - 2.5), Pos2::new(box_center.x, box_center.y + 2.5)],
            Stroke::new(1.0_f32, palette.text_main),
        );
    }

    draw_tree_icon(painter, Pos2::new(rect.left() + 27.0, rect.center().y), icon);

    let text_col = if selected {
        Color32::WHITE
    } else {
        palette.text_main
    };
    painter.text(
        Pos2::new(rect.left() + 39.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(12.0),
        text_col,
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

pub fn tree_child_row(
    ui: &mut Ui,
    palette: &ThemePalette,
    label: impl AsRef<str>,
    icon: TreeIconKind,
    selected: bool,
) -> bool {
    let label = label.as_ref();
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 20.0), Sense::click());
    let painter = ui.painter();

    if selected {
        painter.rect_filled(
            Rect::from_min_max(Pos2::new(rect.left() + 50.0, rect.top()), rect.right_bottom()),
            1.0,
            palette.table_selected_row,
        );
    }

    let lx = rect.left() + 27.0;
    let dot_s = Stroke::new(1.0, palette.border);
    painter.line_segment([Pos2::new(lx, rect.top()), Pos2::new(lx, rect.bottom())], dot_s);
    painter.line_segment([Pos2::new(lx, rect.center().y), Pos2::new(lx + 9.0, rect.center().y)], dot_s);

    draw_tree_icon(painter, Pos2::new(rect.left() + 41.0, rect.center().y), icon);

    let text_col = if selected {
        Color32::WHITE
    } else {
        palette.text_main
    };
    painter.text(
        Pos2::new(rect.left() + 53.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(12.0),
        text_col,
    );

    resp.clicked()
}

pub fn draw_tree_icon(painter: &egui::Painter, c: Pos2, icon: TreeIconKind) {
    match icon {
        TreeIconKind::FolderYellow | TreeIconKind::FolderFinished => {
            let r = Rect::from_center_size(c, Vec2::new(13.0, 10.0));
            painter.rect_filled(r, 1.0, Color32::from_rgb(245, 206, 98));
            if matches!(icon, TreeIconKind::FolderFinished) {
                painter.circle_filled(Pos2::new(c.x + 2.0, c.y + 1.0), 3.5, Color32::from_rgb(55, 130, 210));
            }
        }
        TreeIconKind::Compressed => {
            let r = Rect::from_center_size(c, Vec2::new(11.0, 12.0));
            painter.rect_filled(r, 1.0, Color32::from_rgb(225, 185, 110));
        }
        TreeIconKind::Document => {
            let r = Rect::from_center_size(c, Vec2::new(10.0, 12.0));
            painter.rect_filled(r, 1.0, Color32::from_rgb(215, 225, 238));
        }
        TreeIconKind::Music => {
            painter.circle_filled(Pos2::new(c.x - 2.0, c.y + 3.0), 3.0, Color32::from_rgb(65, 120, 215));
            painter.line_segment([Pos2::new(c.x, c.y + 3.0), Pos2::new(c.x, c.y - 5.0)], Stroke::new(1.5, Color32::from_rgb(65, 120, 215)));
        }
        TreeIconKind::Program => {
            let r = Rect::from_center_size(c, Vec2::new(12.0, 10.0));
            painter.rect_filled(r, 1.0, Color32::from_rgb(130, 165, 215));
        }
        TreeIconKind::Video => {
            let r = Rect::from_center_size(c, Vec2::new(12.0, 10.0));
            painter.rect_filled(r, 1.0, Color32::from_rgb(100, 175, 215));
        }
        TreeIconKind::Grabber => {
            painter.circle_filled(c, 5.5, Color32::from_rgb(85, 160, 245));
            painter.circle_stroke(c, 5.5, Stroke::new(1.0, Color32::WHITE));
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
    painter.rect_filled(r, 1.5, color);
    painter.rect_filled(Rect::from_center_size(Pos2::new(c.x, c.y + 1.0), Vec2::new(7.0, 6.0)), 0.5, Color32::WHITE);
}

pub fn draw_queue_mini_icon(painter: &egui::Painter, c: Pos2, completed: bool) {
    let col = if completed {
        Color32::from_rgb(90, 195, 105)
    } else {
        Color32::from_rgb(235, 185, 80)
    };
    let r = Rect::from_center_size(c, Vec2::new(12.0, 10.0));
    painter.rect_filled(r, 1.0, col);
}

pub fn draw_idm_green_progress_bar(ui: &mut Ui, palette: &ThemePalette, progress: f32) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 18.0), Sense::hover());
    let painter = ui.painter();

    let bg = match palette.mode {
        ThemeMode::Dark => Color32::from_rgb(45, 45, 45),
        ThemeMode::Light => Color32::from_rgb(230, 230, 230),
    };
    painter.rect_filled(rect, 0.0, bg);

    let fill_w = (rect.width() * progress.clamp(0.0, 1.0)).max(0.0);
    if fill_w > 0.0 {
        let fill_rect = Rect::from_min_size(rect.min, Vec2::new(fill_w, rect.height()));
        painter.rect_filled(fill_rect, 0.0, Color32::from_rgb(0, 185, 35));
    }

    let border = Stroke::new(1.0, palette.border);
    painter.line_segment([rect.left_top(), rect.right_top()], border);
    painter.line_segment([rect.right_top(), rect.right_bottom()], border);
    painter.line_segment([rect.right_bottom(), rect.left_bottom()], border);
    painter.line_segment([rect.left_bottom(), rect.left_top()], border);
}

pub fn format_idm_bytes(bytes: u64) -> String {
    if bytes == 0 {
        return String::new();
    }
    if bytes >= 1024 * 1024 * 1024 {
        format!("{:.3} غب", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    } else if bytes >= 1024 * 1024 {
        format!("{:.3} مب", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.2} كب", bytes as f64 / 1024.0)
    } else {
        format!("{} بايت", bytes)
    }
}

pub fn draw_idm_connections_bar(ui: &mut Ui, palette: &ThemePalette, task: &DownloadStateMeta, _overall_progress: f32) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 18.0), Sense::hover());
    let painter = ui.painter();

    let bg = match palette.mode {
        ThemeMode::Dark => Color32::from_rgb(26, 32, 40),
        ThemeMode::Light => Color32::from_rgb(238, 240, 245),
    };
    painter.rect_filled(rect, 0.0, bg);

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
                painter.rect_filled(top_half, 0.0, Color32::from_rgb(85, 205, 255));
                painter.rect_filled(bot_half, 0.0, Color32::from_rgb(0, 130, 230));
            }

            if i > 0 {
                let sep_x = slot_left;
                painter.line_segment([Pos2::new(sep_x, rect.top()), Pos2::new(sep_x, rect.bottom())], Stroke::new(1.0, Color32::from_rgb(180, 210, 240)));
            }
        }
    } else {
        let step = rect.width() / 6.0;
        for i in 1..6 {
            let x = rect.left() + (i as f32) * step;
            painter.line_segment([Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())], Stroke::new(1.0, Color32::from_rgb(150, 180, 210)));
        }
    }

    let border = Stroke::new(1.0, palette.border);
    painter.line_segment([rect.left_top(), rect.right_top()], border);
    painter.line_segment([rect.right_top(), rect.right_bottom()], border);
    painter.line_segment([rect.right_bottom(), rect.left_bottom()], border);
    painter.line_segment([rect.left_bottom(), rect.left_top()], border);
}

pub fn draw_live_speed_graph(ui: &mut Ui, palette: &ThemePalette, task: &DownloadStateMeta) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 95.0), Sense::hover());
    let painter = ui.painter();

    let bg = match palette.mode {
        ThemeMode::Dark => Color32::from_rgb(22, 28, 35),
        ThemeMode::Light => Color32::from_rgb(240, 245, 252),
    };
    painter.rect_filled(rect, 2.0, bg);
    painter.rect_stroke(rect, 2.0, Stroke::new(1.0, palette.border), StrokeKind::Inside);

    // Grid lines
    for i in 1..4 {
        let y = rect.top() + (i as f32) * (rect.height() / 4.0);
        painter.line_segment([Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)], Stroke::new(1.0, palette.grid_line));
    }

    if task.segments.is_empty() {
        return;
    }

    let speeds: Vec<f32> = task.segments.iter().map(|s| s.last_speed_bps as f32).collect();
    let max_speed = speeds.iter().copied().fold(1024.0_f32, f32::max);

    let n = speeds.len();
    if n > 1 {
        let step = rect.width() / ((n - 1) as f32);
        let mut prev = Pos2::new(rect.left(), rect.bottom() - (speeds[0] / max_speed) * (rect.height() - 8.0) - 4.0);

        for (idx, &spd) in speeds.iter().enumerate().skip(1) {
            let cur = Pos2::new(rect.left() + (idx as f32) * step, rect.bottom() - (spd / max_speed) * (rect.height() - 8.0) - 4.0);
            painter.line_segment([prev, cur], Stroke::new(2.0, Color32::from_rgb(45, 145, 235)));
            painter.circle_filled(cur, 2.5, Color32::from_rgb(0, 185, 255));
            prev = cur;
        }
    }
}

pub fn draw_idm_connections_table(ui: &mut Ui, palette: &ThemePalette, task: &DownloadStateMeta, _overall_progress: f32) {
    let table_bg = match palette.mode {
        ThemeMode::Dark => Color32::from_rgb(28, 30, 34),
        ThemeMode::Light => Color32::from_rgb(248, 249, 250),
    };

    let border_stroke = Stroke::new(1.0, palette.border);
    egui::Frame::canvas(ui.style())
        .fill(table_bg)
        .stroke(border_stroke)
        .inner_margin(4.0)
        .show(ui, |ui| {
            egui::ScrollArea::vertical().max_height(120.0).show(ui, |ui| {
                egui::Grid::new("idm_connections_table_exact")
                    .striped(true)
                    .num_columns(3)
                    .spacing([24.0, 4.0])
                    .min_col_width(30.0)
                    .show(ui, |ui| {
                        ui.label(RichText::new("N.").strong().color(palette.text_muted));
                        ui.label(RichText::new("المحمل").strong().color(palette.text_muted));
                        ui.label(RichText::new("معلومات").strong().color(palette.text_muted));
                        ui.end_row();

                        let is_paused = matches!(
                            task.status,
                            DownloadStatus::Paused | DownloadStatus::Failed | DownloadStatus::Queued
                        );

                        if !task.segments.is_empty() {
                            for seg in &task.segments {
                                ui.label(RichText::new(format!("{}", seg.id + 1)).monospace());

                                let downloaded = seg.current_offset.saturating_sub(seg.start_byte);
                                let downloaded_str = if downloaded > 0 {
                                    format_idm_bytes(downloaded)
                                } else {
                                    String::new()
                                };
                                ui.label(RichText::new(downloaded_str).monospace());

                                let info_text = if is_paused {
                                    "قطع الإتصال"
                                } else {
                                    match seg.status {
                                        SegmentStatus::Downloading => "استلام البيانات...",
                                        SegmentStatus::Pending => "في الانتظار",
                                        SegmentStatus::Completed => "اكتمل",
                                        SegmentStatus::Failed => "فشل الاتصال",
                                        _ => "قطع الإتصال",
                                    }
                                };
                                ui.label(RichText::new(info_text).color(palette.text_main));
                                ui.end_row();
                            }
                        } else {
                            for i in 1..=6 {
                                ui.label(RichText::new(format!("{}", i)).monospace());
                                ui.label("");
                                let info = if is_paused {
                                    "قطع الإتصال"
                                } else if i == 1 {
                                    "جاري الإتصال..."
                                } else {
                                    "في الانتظار"
                                };
                                ui.label(RichText::new(info).color(palette.text_main));
                                ui.end_row();
                            }
                        }
                    });
            });
        });
}
