//! نوافذ الأدوات المستقلة، نافذة إضافة التنزيل، نافذة التفاصيل، المجدول، الإعدادات، وحوارات التأكيد والإشعارات
//!
//! يطبق هذا الملف متطلبات المراحل 7، 8، 9، 10، 11، 12.4، 13، 15:
//! - نوافذ مستقلة غير حاجبة (`show_viewport_immediate`) لكل من: الإعدادات، المجدول، تفاصيل التنزيل، إضافة تنزيل جديد، وسجل الأخطاء والتشخيص.
//! - منع تكرار النوافذ ونقل النافذة المفتوحة إلى المقدمة عند طلبها مرة ثانية (`request_focus_window`).
//! - تتبع وحفظ حجم وموضع كل نافذة ثانوية بشكل مستقل مع التحقق من حدود الشاشات المتاحة.
//! - فصل صارم بين حذف سجل التنزيل وحذف الملف من القرص عبر نافذة تأكيد واضحة (`Confirmation Dialog`).

use super::{
    app::RustIdmApp,
    i18n::Loc,
    painter::{
        draw_file_category_icon, draw_idm_connections_bar, draw_idm_connections_table,
        draw_idm_green_progress_bar, draw_live_speed_graph,
    },
    theme::{apply_stormget_theme_with_scale, ThemePalette},
    types::{
        AppLanguage, DialogTab, OptionsTab, PendingConfirmation, RowDensity, ShutdownAction,
        TableColumnId, ThemeMode, ToastKind, UiCommand,
    },
    window_state::{
        detect_connected_monitors, validate_and_clamp_window_geometry, SavedWindowGeometry,
    },
    worker::{open_file, open_folder},
};
use crate::{
    BatchExpandRequest, DownloadStatus, DuplicateFilePolicy, FileCategory,
    InterceptDownloadPayload, PostQueueAction, QueueConfig, RefreshUrlPayload, ScheduleMode,
    UpdateTaskMetaPayload, ZipPreviewRequestPayload,
};
use eframe::egui::{self, Align, Color32, Layout, RichText, Sense, Stroke, Vec2};
use std::path::{Path, PathBuf};

fn opt_non_empty(s: &str) -> Option<String> {
    let t = s.trim();
    if t.is_empty() {
        None
    } else {
        Some(t.to_owned())
    }
}

/// مساعد موحد لإنشاء وتحديث هندسة أي نافذة ثانوية مستقلة مع التحقق من الشاشات المتاحة ومنع التكرار
fn build_and_track_secondary_viewport(
    app: &mut RustIdmApp,
    window_key: &str,
    title: String,
    default_w: f32,
    default_h: f32,
    min_w: f32,
    min_h: f32,
    resizable: bool,
) -> (egui::ViewportId, egui::ViewportBuilder, bool) {
    let saved = app
        .ui_state
        .secondary_windows
        .entry(window_key.to_owned())
        .or_insert_with(|| SavedWindowGeometry::new_default(default_w, default_h))
        .clone();

    let monitors = detect_connected_monitors();
    let placement = validate_and_clamp_window_geometry(&saved, min_w, min_h, &monitors);

    let mut builder = egui::ViewportBuilder::default()
        .with_title(title)
        .with_inner_size([placement.width, placement.height])
        .with_min_inner_size([min_w, min_h])
        .with_position(egui::pos2(placement.x, placement.y))
        .with_resizable(resizable);

    let (rgba, w, h) = super::stormget_app_icon_rgba();
    builder = builder.with_icon(std::sync::Arc::new(egui::IconData {
        rgba,
        width: w,
        height: h,
    }));

    let should_focus = app.focus_window_requests.remove(window_key);
    let viewport_id = egui::ViewportId::from_hash_of(format!("stormget_vp_{window_key}"));
    (viewport_id, builder, should_focus)
}

fn record_secondary_viewport_geometry(
    app: &mut RustIdmApp,
    window_key: &str,
    viewport_ctx: &egui::Context,
) {
    let (inner_rect, outer_rect, is_max, is_min) = viewport_ctx.input(|i| {
        let vp = i.viewport();
        (
            vp.inner_rect,
            vp.outer_rect,
            vp.maximized.unwrap_or(false),
            vp.minimized.unwrap_or(false),
        )
    });
    if let Some(rect) = inner_rect {
        let pos = outer_rect.map(|r| (r.min.x, r.min.y));
        if let Some(entry) = app.ui_state.secondary_windows.get_mut(window_key) {
            let before = entry.clone();
            entry.update_from_live_viewport(
                pos.map(|p| p.0),
                pos.map(|p| p.1),
                rect.width(),
                rect.height(),
                is_max,
                is_min,
            );
            if *entry != before {
                app.mark_ui_state_dirty();
            }
        }
    }
}

pub fn draw_all_dialogs(app: &mut RustIdmApp, ctx: &egui::Context) {
    let loc = Loc::new(app.language);
    let palette = ThemePalette::get(app.theme_mode);

<<<<<<< HEAD
    // النوافذ المستقلة خارج النافذة الرئيسية (Independent OS Viewports)
    draw_independent_settings_viewport(app, ctx, &loc, &palette);
    draw_independent_scheduler_viewport(app, ctx, &loc, &palette);
    draw_independent_diagnostics_viewport(app, ctx, &loc, &palette);

    // نوافذ الأدوات المساعدة وحوارات التعديل والتأكيد
=======
>>>>>>> efc40c730b5c3e9a4001ccff0b896d7f2f3ef344
    draw_batch_dialog(app, ctx, &loc);
    draw_refresh_dialog(app, ctx, &loc);
    draw_grabber_dialog(app, ctx, &loc);
    draw_zip_preview_dialog(app, ctx, &loc);
    draw_import_export_dialog(app, ctx, &loc);
<<<<<<< HEAD
    draw_rename_move_dialog(app, ctx, &loc);
    draw_column_chooser_dialog(app, ctx, &loc);
    draw_about_and_shortcuts_dialogs(app, ctx, &loc, &palette);
    draw_confirmation_modal(app, ctx, &loc, &palette);
    draw_toast_overlay(app, ctx, &palette);
}

// ============================================================================
// المرحلة السابعة: نافذة إضافة تنزيل جديد (Add New Download Window)
// ============================================================================

pub fn draw_add_url_dialog(
    app: &mut RustIdmApp,
    ctx: &egui::Context,
    loc: &Loc,
    palette: &ThemePalette,
) {
=======
    draw_options_dialog(app, ctx, &loc);
    draw_scheduler_dialog(app, ctx, &loc);
}

pub fn draw_add_url_dialog(app: &mut RustIdmApp, ctx: &egui::Context, loc: &Loc, palette: &ThemePalette) {
>>>>>>> efc40c730b5c3e9a4001ccff0b896d7f2f3ef344
    if !app.show_add_dialog {
        return;
    }
    let mut submitted_start = false;
    let mut submitted_later = false;
    let mut cancelled = false;

<<<<<<< HEAD
    let url_trimmed = app.new_url.trim().to_owned();
    let is_magnet_or_torrent = url_trimmed.starts_with("magnet:?")
        || url_trimmed.to_ascii_lowercase().ends_with(".torrent");
    let url_validation_error = if url_trimmed.is_empty() || is_magnet_or_torrent {
        None
    } else {
        match url::Url::parse(&url_trimmed) {
            Ok(parsed)
                if parsed.scheme() == "http"
                    || parsed.scheme() == "https"
                    || parsed.scheme() == "magnet" =>
            {
                None
            }
            Ok(parsed) => Some(format!(
                "magnet:? ﻭﺃ https:// ﻭﺃ http:// ﺏ ﺃﺪﺒﻳ ﻂﺑﺍﺭ ﻝﺎﺧﺩﺇ ﻰﺟﺮﻳ — ﻡﻮﻋﺪﻣ ﺮﻴﻏ '{}' ﻝﻮﻛﻮﺗﻭﺮﺒﻟﺍ",
                parsed.scheme()
            )),
            Err(_) => Some("(https://example.com/file.zip ﻭﺃ magnet:? :ﻝﺎﺜﻣ) ﺔﺤﻟﺎﺻ ﺮﻴﻏ ﻂﺑﺍﺮﻟﺍ ﺔﻐﻴﺻ".to_owned()),
        }
    };
    let is_valid_url = !url_trimmed.is_empty() && url_validation_error.is_none();

    egui::CentralPanel::default()
        .frame(
            egui::Frame::new()
                .fill(palette.window_bg)
                .inner_margin(egui::Margin::same(14)),
        )
        .show(ctx, |ui| {
            ui.spacing_mut().item_spacing = Vec2::new(8.0, 8.0);
=======
    egui::CentralPanel::default().show(ctx, |ui| {
            ui.spacing_mut().item_spacing = Vec2::new(8.0, 6.0);
>>>>>>> efc40c730b5c3e9a4001ccff0b896d7f2f3ef344

            ui.horizontal(|ui| {
                ui.heading(
                    RichText::new(loc.add_download_window_title())
                        .color(palette.text_main)
                        .strong(),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if let Some(host) = &app.new_probed_host {
                        ui.label(
                            RichText::new(format!("{host} :ﻒﻴﻀﻤﻟﺍ 🌐"))
                                .size(11.5)
                                .color(palette.color_info),
                        );
                    }
                });
            });
            ui.separator();

            // 1. حقل الرابط + زر لصق + زر فحص مسبق (Probe)
            ui.horizontal(|ui| {
                ui.label(RichText::new("(URL): ﻂﺑﺍﺮﻟﺍ").strong());
                let edit = ui.add(
                    egui::TextEdit::singleline(&mut app.new_url)
                        .hint_text("https://example.com/archive.zip")
                        .desired_width((ui.available_width() - 175.0).max(220.0)),
                );
                if ui
                    .button("ﻖﺼﻟ 📋")
                    .on_hover_text("(Ctrl+V) ﻖﺼﻠﻟﺍ ﺓﺩﺎﻋﻹ ﻞﻘﺤﻟﺍ ﺢﺴﻣ ﻭﺃ ﺔﻈﻓﺎﺤﻟﺍ ﻦﻣ ﻂﺑﺍﺮﻟﺍ ﻖﺼﻟ")
                    .clicked()
                {
                    if let Some(clip) = app.last_clipboard_url.clone() {
                        app.new_url = clip;
                        app.on_new_url_modified();
                    }
                }
                let can_probe = is_valid_url && !app.new_probe_in_progress;
                if ui
                    .add_enabled(can_probe, egui::Button::new("ﻒﻠﻤﻟﺍ ﺺﺤﻓ 🔍"))
                    .on_hover_text("ﻑﺎﻨﺌﺘﺳﻻﺍ ﺔﻴﻧﺎﻜﻣﺇﻭ ﻲﻘﻴﻘﺤﻟﺍ ﻪﻤﺳﺍﻭ ﻒﻠﻤﻟﺍ ﻢﺠﺣ ﻦﻋ ﻡﺩﺎﺨﻟﺍ ﻡﻼﻌﺘﺳﺍ")
                    .clicked()
                {
                    app.trigger_url_probe();
                }
                if edit.changed() {
                    app.on_new_url_modified();
                }
            });

            // رسالة التحقق من صحة الرابط أو نتيجة الفحص المسبق (دون منع التنزيل عند فشل الفحص المسبق)
            if let Some(err) = &url_validation_error {
                ui.colored_label(palette.color_error, RichText::new(format!("⚠ {err}")).size(11.5));
            } else if app.new_probe_in_progress {
                ui.colored_label(
                    palette.color_info,
                    RichText::new("...ﻡﺩﺎﺨﻟﺍ ﻦﻣ ﻒﻠﻤﻟﺍ ﺕﺎﻧﺎﻴﺑ ﺺﺤﻓ ﺭﺎﺟ ⏳").size(11.5),
                );
            } else if let Some(probe_err) = &app.new_probe_error {
                ui.colored_label(
                    palette.color_warning,
                    RichText::new(format!(
                        ".ﻂﺑﺍﺮﻟﺍ ﻊﻣ ﻙﺮﺤﻤﻟﺍ ﻞﻣﺎﻌﺘﻴﺳﻭ ﺓﺮﺷﺎﺒﻣ ﻞﻳﺰﻨﺘﻟﺍ ﺀﺪﺑ ﻚﻨﻜﻤﻳ — ({probe_err}) ﻖﺒﺴﻤﻟﺍ ﺺﺤﻔﻟﺍ ﺭﺬﻌﺗ ℹ"
                    ))
                    .size(11.0),
                );
            } else if let Some(intel) = &app.latest_intelligence {
                ui.colored_label(
                    if intel.duplicate.is_some() {
                        palette.color_warning
                    } else {
                        palette.color_info
                    },
                    RichText::new(format!("🛡 {}", intel.summary_arabic)).size(11.0),
                );
            }

            // 2. الفئة + الطابور + معلومات الحجم والاستئناف المؤكدة
            ui.horizontal(|ui| {
                ui.label(loc.category_label());
                egui::ComboBox::from_id_salt("add_cat_combo")
                    .selected_text(loc.category_name(app.new_category))
                    .width(145.0)
                    .show_ui(ui, |ui| {
                        for cat in [
                            FileCategory::General,
                            FileCategory::Compressed,
                            FileCategory::Documents,
                            FileCategory::Music,
                            FileCategory::Programs,
                            FileCategory::Video,
                        ] {
                            if ui
                                .selectable_value(&mut app.new_category, cat, loc.category_name(cat))
                                .changed()
                                && app.new_remember_path
                            {
                                app.new_save_folder = app
                                    .app_config
                                    .download_dir
                                    .join(cat.folder_name())
                                    .to_string_lossy()
                                    .into_owned();
                            }
                        }
                    });

                ui.separator();
                ui.label(loc.tr(":ﺭﻮﺑﺎﻄﻟﺍ", "Queue:"));
                let q_names: Vec<String> = if app.queues.is_empty() {
                    vec!["main".to_owned(), "sync".to_owned()]
                } else {
                    app.queues.iter().map(|q| q.name.clone()).collect()
                };
                egui::ComboBox::from_id_salt("add_queue_combo")
                    .selected_text(&app.new_queue_selection)
                    .width(110.0)
                    .show_ui(ui, |ui| {
                        for q in q_names {
                            ui.selectable_value(&mut app.new_queue_selection, q.clone(), q);
                        }
                    });

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
<<<<<<< HEAD
                    // قاعدة إلزامية: لا تعرض حجماً مختلقاً  قبل التحقق الفعلي من الخادم
                    let size_badge = match app.new_probed_size_bytes {
                        Some(sz) if sz > 0 => format!("{} :ﺪﻛﺆﻤﻟﺍ ﻢﺠﺤﻟﺍ", loc.format_bytes(sz)),
                        Some(_) => "ﻡﺩﺎﺨﻟﺍ ﻦﻣ ﺩﺪﺤﻣ ﺮﻴﻏ :ﻢﺠﺤﻟﺍ".to_owned(),
                        None => "ﺪﻌﺑ ﺺﺤﻔﻳ ﻢﻟ :ﻢﺠﺤﻟﺍ".to_owned(),
=======
                    let size_text = if let Some(sz) = app.new_probed_size_bytes {
                        format_idm_bytes(sz)
                    } else {
                        "6.14 KB".to_string()
>>>>>>> efc40c730b5c3e9a4001ccff0b896d7f2f3ef344
                    };
                    let resume_badge = match app.new_probed_resumable {
                        Some(true) => "✓ ﻑﺎﻨﺌﺘﺳﻻﺍ ﻢﻋﺪﻳ | ",
                        Some(false) => "ﻢﻴﺴﻘﺘﻟﺍ ﻢﻋﺪﻳ ﻻ | ",
                        None => "",
                    };
                    ui.label(
                        RichText::new(format!("{size_badge}{resume_badge}"))
                            .size(11.5)
                            .strong()
                            .color(palette.text_main),
                    );
                    let (icon_rect, _) =
                        ui.allocate_exact_size(Vec2::new(18.0, 18.0), Sense::hover());
                    draw_file_category_icon(ui.painter(), icon_rect.center(), app.new_category);
                });
            });

<<<<<<< HEAD
            // 3. اسم الملف (مع توضيح إذا كان مقترحاً من الرابط أم مؤكداً)
            ui.horizontal(|ui| {
                ui.label(loc.file_name_label());
                ui.add(
                    egui::TextEdit::singleline(&mut app.new_filename)
                        .hint_text("ﻂﺑﺍﺮﻟﺍ ﻭﺃ ﻡﺩﺎﺨﻟﺍ ﺕﺎﺴﻳﻭﺮﺗ ﻦﻣ ﺎﻴﺋﺎﻘﻠﺗ ﻒﺸﺘﻜﻳ")
                        .desired_width((ui.available_width() - 10.0).max(240.0)),
                );
            });

            // 4. مجلد الحفظ
            ui.horizontal(|ui| {
                ui.label(loc.save_folder_label());
                ui.add(
                    egui::TextEdit::singleline(&mut app.new_save_folder)
                        .desired_width((ui.available_width() - 95.0).max(200.0)),
                );
                if ui
                    .button(loc.open_folder())
                    .on_hover_text("ﺕﺎﻔﻠﻤﻟﺍ ﻒﺸﻜﺘﺴﻣ ﻲﻓ ﻆﻔﺤﻟﺍ ﺪﻠﺠﻣ ﺢﺘﻓ")
                    .clicked()
                {
                    open_folder(Path::new(&app.new_save_folder));
                }
            });

            ui.horizontal(|ui| {
                let cat_label = loc.category_name(app.new_category);
                ui.checkbox(
                    &mut app.new_remember_path,
                    loc.remember_path_label(&cat_label),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui
                        .selectable_label(
                            app.show_add_advanced,
                            loc.tr(
                                "(ﺕﺎﺴﻳﻭﺮﺘﻟﺍ / ﺎﻳﺍﺮﻤﻟﺍ / ﺔﻤﺼﺒﻟﺍ ﻦﻣ ﻖﻘﺤﺘﻟﺍ) ﺔﻣﺪﻘﺘﻣ ﺕﺍﺭﺎﻴﺧ ⚙",
                                "⚙ Advanced Options (Checksums / Mirrors / Headers)",
                            ),
                        )
                        .clicked()
                    {
                        app.show_add_advanced = !app.show_add_advanced;
                    }
                });
=======
            // The destination is editable before the download is queued.
            ui.horizontal(|ui| {
                ui.label(loc.save_folder_label());
                ui.add(egui::TextEdit::singleline(&mut app.new_save_folder).desired_width(400.0));
                if ui.button(loc.open_folder()).clicked() {
                    super::worker::open_folder(Path::new(&app.new_save_folder));
                }
            });

            ui.horizontal(|ui| {
                ui.label(loc.file_name_label());
                ui.add(egui::TextEdit::singleline(&mut app.new_filename).desired_width(400.0));
            });

            let frame_stroke = Stroke::new(1.0, Color32::from_rgb(140, 140, 140));
            egui::Frame::canvas(ui.style())
                .stroke(frame_stroke)
                .inner_margin(6.0)
                .show(ui, |ui| {
                    let cat_label = loc.category_name(app.new_category);
                    ui.checkbox(
                        &mut app.new_remember_path,
                        loc.remember_path_label(&cat_label),
                    );
                });

            // Row 5: Description
            ui.horizontal(|ui| {
                ui.label(loc.description_label());
                ui.add(egui::TextEdit::singleline(&mut app.new_description).desired_width(460.0));
>>>>>>> efc40c730b5c3e9a4001ccff0b896d7f2f3ef344
            });

            // 5. الوصف
            ui.horizontal(|ui| {
<<<<<<< HEAD
                ui.label(loc.description_label());
                ui.add(
                    egui::TextEdit::singleline(&mut app.new_description)
                        .desired_width((ui.available_width() - 10.0).max(240.0)),
                );
            });

            // 6. الخيارات المتقدمة التي يدعمها المحرك فعلياً
            if app.show_add_advanced {
                egui::Frame::canvas(ui.style())
                    .fill(palette.bg_secondary)
                    .stroke(Stroke::new(1.0, palette.border))
                    .inner_margin(8.0)
                    .show(ui, |ui| {
                        egui::Grid::new("add_advanced_options_grid")
                            .num_columns(2)
                            .spacing([10.0, 5.0])
                            .show(ui, |ui| {
                                ui.label(":ﻊﻗﻮﺘﻤﻟﺍ SHA-256");
                                ui.add(
                                    egui::TextEdit::singleline(&mut app.new_expected_sha256)
                                        .desired_width(380.0),
                                );
                                ui.end_row();

                                ui.label("MD5 / CRC32:");
                                ui.horizontal(|ui| {
                                    ui.add(
                                        egui::TextEdit::singleline(&mut app.new_expected_md5)
                                            .hint_text("MD5")
                                            .desired_width(220.0),
                                    );
                                    ui.add(
                                        egui::TextEdit::singleline(&mut app.new_expected_crc32)
                                            .hint_text("CRC32")
                                            .desired_width(150.0),
                                    );
                                });
                                ui.end_row();

                                ui.label(":ﺭﺪﺼﻤﻟﺍ ﺔﺤﻔﺻ / Referer");
                                ui.add(
                                    egui::TextEdit::singleline(&mut app.new_referer)
                                        .desired_width(380.0),
                                );
                                ui.end_row();

                                ui.label("(:ﻱﺭﺎﻴﺘﺧﺍ) Cookies");
                                ui.add(
                                    egui::TextEdit::singleline(&mut app.new_cookies)
                                        .password(true)
                                        .desired_width(380.0),
                                );
                                ui.end_row();

                                ui.label("(Mirrors): ﺔﻠﻳﺪﺑ ﻂﺑﺍﻭﺭ");
                                ui.add(
                                    egui::TextEdit::singleline(&mut app.new_mirrors_text)
                                        .hint_text(", ﺔﻠﺻﺎﻔﺑ ﺔﻠﻳﺪﺒﻟﺍ ﻂﺑﺍﻭﺮﻟﺍ ﻦﻴﺑ ﻞﺼﻓﺍ")
                                        .desired_width(380.0),
                                );
                                ui.end_row();
                            });
                    });
            }

            ui.add_space(6.0);
            ui.separator();

            // 7. أزرار الإجراء السفلي مع حماية من النقر المزدوج (Debounce)
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(12.0, 0.0);
                if ui
                    .add_enabled(
                        is_valid_url && app.can_trigger_action(),
                        egui::Button::new(
                            RichText::new(format!("▶ {}", loc.start_download()))
                                .strong()
                                .color(Color32::WHITE),
                        )
                        .fill(palette.btn_primary_bg)
                        .min_size(Vec2::new(155.0, 30.0)),
                    )
                    .on_hover_text("(Enter) ﺔﻋﺮﺳ ﻰﺼﻗﺄﺑ ﺍﺭﻮﻓ ﻒﻠﻤﻟﺍ ﻞﻳﺰﻨﺗ ﺀﺪﺑ")
                    .clicked()
                {
                    submitted_start = true;
                }

                if ui
                    .add_enabled(
                        is_valid_url && app.can_trigger_action(),
                        egui::Button::new(format!("⏳ {}", loc.download_later()))
                            .min_size(Vec2::new(165.0, 30.0)),
                    )
                    .on_hover_text("ﻥﻵﺍ ﻪﺋﺪﺑ ﻥﻭﺩ ﺩﺪﺤﻤﻟﺍ ﺭﻮﺑﺎﻄﻟﺍ ﻰﻟﺇ ﻞﻳﺰﻨﺘﻟﺍ ﺔﻓﺎﺿﺇ")
                    .clicked()
                {
                    submitted_later = true;
                }

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui
                        .add_sized([105.0, 30.0], egui::Button::new(loc.cancel()))
                        .on_hover_text("(Esc) ﺓﺬﻓﺎﻨﻟﺍ ﻕﻼﻏﺇﻭ ﺀﺎﻐﻟﺇ")
                        .clicked()
                        || ui.input(|i| i.key_pressed(egui::Key::Escape))
                    {
                        cancelled = true;
                    }
                });
            });
        });

    if submitted_start || submitted_later {
        app.record_action_triggered();
        let start_immediately = submitted_start;
        let url = app.new_url.trim().to_owned();
        if !url.is_empty() {
            let filename = opt_non_empty(&app.new_filename);
            let mirrors: Vec<String> = app
                .new_mirrors_text
                .split(',')
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty())
                .collect();
            let payload = InterceptDownloadPayload {
                url,
                source_page_url: opt_non_empty(&app.new_referer),
                filename,
                cookies: opt_non_empty(&app.new_cookies),
                referer: opt_non_empty(&app.new_referer),
                user_agent: opt_non_empty(&app.new_user_agent),
                queue_name: Some(app.new_queue_selection.clone()),
                expected_sha256: opt_non_empty(&app.new_expected_sha256),
                expected_md5: opt_non_empty(&app.new_expected_md5),
                expected_crc32: opt_non_empty(&app.new_expected_crc32),
                mirror_urls: if mirrors.is_empty() {
                    None
                } else {
                    Some(mirrors)
                },
                description: opt_non_empty(&app.new_description),
            };
            if is_magnet_or_torrent {
                let _ = app.commands.send(UiCommand::AddTorrent(crate::StartTorrentRequest {
                    source: payload.url.clone(),
                    save_dir: opt_non_empty(&app.new_save_folder).map(PathBuf::from),
                    select_files: None,
                    seed_minutes: 0,
                }));
            } else if let Some(id) = app.pending_intercept_id.take() {
                let _ = app.commands.send(UiCommand::ResolvePendingIntercept {
                    id,
                    action: if start_immediately { "start" } else { "later" },
                    save_dir: opt_non_empty(&app.new_save_folder).map(PathBuf::from),
                });
            } else {
                let _ = app.commands.send(UiCommand::AddDownload {
                    payload,
                    save_dir: opt_non_empty(&app.new_save_folder).map(PathBuf::from),
                    start_immediately,
                });
            }
            app.status_message = if start_immediately {
                "...ﻞﻳﺰﻨﺘﻟﺍ ﺀﺪﺑ ﺭﺎﺟ".to_owned()
            } else {
                "ﺭﺎﻈﺘﻧﻻﺍ ﺔﻤﺋﺎﻗ ﻰﻟﺇ ﻞﻳﺰﻨﺘﻟﺍ ﺔﻓﺎﺿﺇ ﺖﻤﺗ".to_owned()
            };
            app.reset_add_dialog_fields();
            app.show_add_dialog = false;
        }
    } else if cancelled {
        if let Some(id) = app.pending_intercept_id.take() {
            let _ = app.commands.send(UiCommand::ResolvePendingIntercept {
                id,
                action: "cancel",
                save_dir: None,
            });
        }
        app.show_add_dialog = false;
    }
}

// ============================================================================
// المرحلة الثامنة: نافذة تفاصيل التنزيل المستقلة (5 تبويبات أساسية + التحكم)
// ============================================================================

pub fn draw_details_dialog(app: &mut RustIdmApp, ctx: &egui::Context, palette: &ThemePalette) {
    if !app.show_details {
        return;
    }
    let loc = Loc::new(app.language);
    let Some(task) = app.selected_download().cloned() else {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.centered_and_justified(|ui| {
                ui.label(".ﺎﻬﻠﻴﺻﺎﻔﺗ ﺽﺮﻌﻟ ﺔﻤﻬﻣ ﻱﺃ ﺪﻳﺪﺤﺗ ﻢﺘﻳ ﻢﻟ");
            });
        });
        return;
    };

    let downloaded = task.total_downloaded();
    let progress = if task.total_size == 0 {
        if task.status == DownloadStatus::Completed {
            1.0
        } else {
            0.0
        }
    } else {
        (downloaded as f32 / task.total_size as f32).clamp(0.0, 1.0)
    };
    let total_speed: u64 = task.smoothed_speed_bps();

    let mut request_pause = false;
    let mut request_resume = false;
    let mut request_cancel = false;
    let mut request_reverify = false;
    let mut request_refresh_url = false;

    egui::CentralPanel::default()
        .frame(
            egui::Frame::new()
                .fill(palette.window_bg)
                .inner_margin(egui::Margin::same(12)),
        )
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                let (icon_r, _) = ui.allocate_exact_size(Vec2::new(20.0, 20.0), Sense::hover());
                draw_file_category_icon(ui.painter(), icon_r.center(), task.category);
                let title_str = if task.total_size > 0 {
                    format!("{:.1}% — {}", progress * 100.0, task.filename)
                } else {
                    task.filename.clone()
                };
                ui.heading(RichText::new(title_str).color(palette.text_main).strong());
            });
            ui.separator();

            // التبويبات الـ 5 المطلوبة في المرحلة 8 + محدد السرعة + خيارات الاكتمال
            ui.horizontal_wrapped(|ui| {
                for (tab, ar_title, en_title) in [
                    (DialogTab::General, "(General) ﻡﺎﻋ", "General"),
                    (DialogTab::Progress, "(Progress) ﻡﺪﻘﺘﻟﺍ", "Progress"),
                    (DialogTab::Connection, "(Connection) ﻝﺎﺼﺗﻻﺍ", "Connection"),
                    (DialogTab::Logs, "(Logs) ﻞﺠﺴﻟﺍ", "Logs"),
                    (DialogTab::Integrity, "(Integrity) ﻒﻠﻤﻟﺍ ﺔﻣﻼﺳ", "Integrity"),
                    (DialogTab::SpeedLimiter, "ﺔﻋﺮﺴﻟﺍ ﺩﺪﺤﻣ", "Speed Limiter"),
                    (DialogTab::OptionsOnCompletion, "ﻝﺎﻤﺘﻛﻻﺍ ﺪﻨﻋ", "On Completion"),
                ] {
                    let is_active = app.dialog_tab == tab;
                    let label = loc.tr(ar_title, en_title);
                    if ui
                        .selectable_label(
                            is_active,
                            RichText::new(label).strong().color(if is_active {
                                palette.text_on_accent
                            } else {
                                palette.text_main
                            }),
                        )
                        .clicked()
                    {
                        app.dialog_tab = tab;
                    }
                }
            });

            ui.add_space(4.0);

            egui::Frame::canvas(ui.style())
                .fill(palette.bg_secondary)
                .stroke(Stroke::new(1.0, palette.border))
                .inner_margin(10.0)
                .show(ui, |ui| {
                    match app.dialog_tab {
                        // 8.1 تبويب General — عام
                        DialogTab::General => {
                            let final_path = task.final_file_path();
                            let file_exists = final_path.exists();
                            egui::Grid::new("details_general_grid")
                                .num_columns(2)
                                .spacing([18.0, 6.0])
                                .show(ui, |ui| {
                                    ui.label(RichText::new(":ﻒﻠﻤﻟﺍ ﻢﺳﺍ").strong());
                                    ui.label(RichText::new(&task.filename).monospace());
                                    ui.end_row();

                                    ui.label(RichText::new(":ﺭﺪﺼﻤﻟﺍ ﻂﺑﺍﺭ").strong());
                                    ui.horizontal(|ui| {
                                        let mut u = task.url.clone();
                                        ui.add(
                                            egui::TextEdit::singleline(&mut u)
                                                .desired_width(360.0),
                                        );
                                        if ui.small_button("ﺦﺴﻧ 📋").clicked() {
                                            ui.ctx().copy_text(task.url.clone());
                                        }
                                    });
                                    ui.end_row();

                                    ui.label(RichText::new(":ﻲﻠﺤﻤﻟﺍ ﺭﺎﺴﻤﻟﺍ").strong());
                                    ui.horizontal(|ui| {
                                        ui.label(
                                            RichText::new(final_path.display().to_string())
                                                .monospace(),
                                        );
                                        if task.status == DownloadStatus::Completed && !file_exists
                                        {
                                            ui.colored_label(
                                                palette.color_error,
                                                "ﺹﺮﻘﻟﺍ ﻦﻣ ﺩﻮﻘﻔﻣ ﻒﻠﻤﻟﺍ ⚠",
                                            );
                                        }
                                    });
                                    ui.end_row();

                                    ui.label(RichText::new(":ﻲﻟﺎﻤﺟﻹﺍ ﻢﺠﺤﻟﺍ").strong());
                                    ui.label(
                                        RichText::new(if task.total_size > 0 {
                                            format!(
                                                "{} ({}(ﺖﻳﺎﺑ ",
                                                loc.format_bytes(task.total_size),
                                                task.total_size
                                            )
                                        } else {
                                            loc.tr("(ﺩﺪﺤﻣ ﺮﻴﻏ ﻡﺪﻘﺗ) ﻑﻭﺮﻌﻣ ﺮﻴﻏ", "Unknown")
                                        })
                                        .monospace(),
                                    );
                                    ui.end_row();

                                    ui.label(RichText::new(":ﻝﺰﻨﻤﻟﺍ ﻢﺠﺤﻟﺍ").strong());
                                    ui.label(
                                        RichText::new(format!(
                                            "{} ({}(ﺖﻳﺎﺑ ",
                                            loc.format_downloaded_bytes(downloaded),
                                            downloaded
                                        ))
                                        .monospace(),
                                    );
                                    ui.end_row();

                                    ui.label(RichText::new(":ﺔﻴﻟﺎﺤﻟﺍ ﺔﻟﺎﺤﻟﺍ").strong());
                                    let st_col = palette.color_for_status(
                                        task.status,
                                        task.is_cancelled_by_user(),
                                    );
                                    ui.colored_label(
                                        st_col,
                                        RichText::new(loc.status_text_for_task(&task)).strong(),
                                    );
                                    ui.end_row();

                                    ui.label(RichText::new(":ﺔﻓﺎﺿﻹﺍ ﺦﻳﺭﺎﺗ").strong());
                                    ui.label(
                                        task.created_at
                                            .format("%Y-%m-%d %H:%M:%S UTC")
                                            .to_string(),
                                    );
                                    ui.end_row();

                                    ui.label(RichText::new(":ﺀﺎﻬﺘﻧﺍ / ﺚﻳﺪﺤﺗ ﺮﺧﺁ").strong());
                                    ui.label(
                                        task.updated_at
                                            .format("%Y-%m-%d %H:%M:%S UTC")
                                            .to_string(),
                                    );
                                    ui.end_row();

                                    if let Some(err) = &task.last_error {
                                        ui.label(
                                            RichText::new(":ﺄﻄﺨﻟﺍ / ﻒﻗﻮﺘﻟﺍ ﺐﺒﺳ")
                                                .strong()
                                                .color(palette.color_error),
                                        );
                                        ui.colored_label(palette.color_error, err);
                                        ui.end_row();
                                    }
                                    if let Some(diag) = &app.latest_doctor_diagnosis {
                                        ui.label(
                                            RichText::new(":(Download Doctor) ﻲﻛﺬﻟﺍ ﺺﻴﺨﺸﺘﻟﺍ 🩺")
                                                .strong()
                                                .color(palette.color_info),
                                        );
                                        ui.colored_label(
                                            palette.color_info,
                                            format!(
                                                "[{:?}] {}",
                                                diag.category, diag.explanation_arabic
                                            ),
                                        );
                                        ui.end_row();
                                    }
                                });
                        }

                        // 8.2 تبويب Progress — التقدم
                        DialogTab::Progress => {
                            egui::Grid::new("details_progress_grid")
                                .num_columns(4)
                                .spacing([20.0, 6.0])
                                .show(ui, |ui| {
                                    ui.label(RichText::new(":ﺯﺎﺠﻧﻹﺍ ﺔﺒﺴﻧ").strong());
                                    let pct_text = if task.total_size > 0 {
                                        format!("{:.2}%", progress * 100.0)
                                    } else if task.status == DownloadStatus::Completed {
                                        "100%".to_owned()
                                    } else {
                                        loc.tr("(ﻝﻮﻬﺠﻣ ﻢﺠﺣ) ﺓﺩﺪﺤﻣ ﺮﻴﻏ", "Indeterminate")
                                    };
                                    ui.label(RichText::new(pct_text).monospace().strong());

                                    ui.label(RichText::new(":ﺔﻴﻟﺎﺤﻟﺍ ﺔﻋﺮﺴﻟﺍ").strong());
                                    ui.label(
                                        RichText::new(loc.format_speed(total_speed))
                                            .monospace()
                                            .color(palette.status_downloading),
                                    );
                                    ui.end_row();

                                    ui.label(RichText::new(":ﻲﻘﺒﺘﻤﻟﺍ ﺖﻗﻮﻟﺍ").strong());
                                    // قاعدة إلزامية: لا تعرض وقتاً متبقياً مضللاً إذا كانت السرعة = 0 أو الحجم مجهولاً
                                    let eta_text = task
                                        .estimated_eta_secs()
                                        .map(|secs| loc.time_left_str(secs))
                                        .unwrap_or_else(|| loc.tr("ﺡﺎﺘﻣ ﺮﻴﻏ", "N/A"));
                                    ui.label(RichText::new(eta_text).monospace());

                                    ui.label(RichText::new(":ﺕﻻﺎﺼﺗﻻﺍ / ﺀﺍﺰﺟﻷﺍ").strong());
                                    let active_segs = task
                                        .segments
                                        .iter()
                                        .filter(|s| s.status == crate::SegmentStatus::Downloading)
                                        .count();
                                    ui.label(
                                        RichText::new(format!(
                                            "{} / ﺔﻄﺸﻧ {}ﻊﻃﺎﻘﻤﻟﺍ ﻲﻟﺎﻤﺟﺇ ",
                                            active_segs,
                                            task.segments.len()
                                        ))
                                        .monospace(),
                                    );
                                    ui.end_row();
                                });

                            ui.add_space(6.0);
                            draw_live_speed_graph(ui, palette, &task);
                        }

                        // 8.3 تبويب Connection — الاتصال (بيانات حقيقية فقط)
                        DialogTab::Connection => {
                            let host_str = url::Url::parse(&task.url)
                                .ok()
                                .and_then(|u| {
                                    u.host_str().map(|h| {
                                        format!("{}://{}", u.scheme(), h)
                                    })
                                })
                                .unwrap_or_else(|| "ﻑﻭﺮﻌﻣ ﺮﻴﻏ".to_owned());

                            egui::Grid::new("details_connection_grid")
                                .num_columns(2)
                                .spacing([18.0, 6.0])
                                .show(ui, |ui| {
                                    ui.label(RichText::new("(Host): ﻒﻴﻀﻤﻟﺍ").strong());
                                    ui.label(RichText::new(host_str).monospace());
                                    ui.end_row();

                                    ui.label(RichText::new("(HTTP Range): ﻑﺎﻨﺌﺘﺳﻻﺍ ﻢﻋﺩ").strong());
                                    ui.colored_label(
                                        if task.resumable {
                                            palette.color_success
                                        } else {
                                            palette.color_warning
                                        },
                                        if task.resumable {
                                            "(206 Partial Content / Accept-Ranges: bytes) ﻡﻮﻋﺪﻣ"
                                        } else {
                                            "ﻡﺩﺎﺨﻟﺍ ﻦﻣ ﻡﻮﻋﺪﻣ ﺮﻴﻏ ﻭﺃ ﺪﻛﺆﻣ ﺮﻴﻏ"
                                        },
                                    );
                                    ui.end_row();

                                    ui.label(RichText::new("(:ﻡﺩﺎﺨﻟﺍ ﻦﻣ) ETag ﺔﺴﻳﻭﺮﺗ").strong());
                                    ui.label(
                                        RichText::new(
                                            task.etag.as_deref().unwrap_or("ﻡﺩﺎﺨﻟﺍ ﻦﻣ ﺓﺮﻓﻮﺘﻣ ﺮﻴﻏ"),
                                        )
                                        .monospace(),
                                    );
                                    ui.end_row();

                                    ui.label(RichText::new("(:ﻡﺩﺎﺨﻟﺍ ﻦﻣ) Last-Modified").strong());
                                    ui.label(
                                        RichText::new(
                                            task.last_modified
                                                .as_deref()
                                                .unwrap_or("ﻡﺩﺎﺨﻟﺍ ﻦﻣ ﺓﺮﻓﻮﺘﻣ ﺮﻴﻏ"),
                                        )
                                        .monospace(),
                                    );
                                    ui.end_row();

                                    ui.label(RichText::new(":ﻂﺑﺍﺮﻟﺍ ﺪﻳﺪﺠﺗ ﺕﻻﻭﺎﺤﻣ").strong());
                                    ui.label(format!("{}", task.refresh_attempts));
                                    ui.end_row();

                                    ui.label(RichText::new("(Mirrors): ﺔﻠﻳﺪﺒﻟﺍ ﻂﺑﺍﻭﺮﻟﺍ").strong());
                                    ui.label(if task.mirror_urls.is_empty() {
                                        "ﺔﻓﺎﻀﻣ ﺎﻳﺍﺮﻣ ﺪﺟﻮﺗ ﻻ".to_owned()
                                    } else {
                                        format!(
                                            "{}# :ﺎﻴﻟﺎﺣ ﻂﺸﻨﻟﺍ) ﺎﻳﺍﺮﻣ {})",
                                            task.mirror_urls.len(),
                                            task.active_mirror_index
                                        )
                                    });
                                    ui.end_row();
                                });

                            if task.is_expired_link() {
                                ui.add_space(6.0);
                                ui.horizontal(|ui| {
                                    ui.colored_label(
                                        palette.color_warning,
                                        ":ﺚﻳﺪﺤﺗ ﻰﻟﺇ ﺝﺎﺘﺤﻳ ﺪﻗﻭ (401/403/410) ﺖﻬﺘﻧﺍ ﺪﻗ ﻂﺑﺍﺮﻟﺍ ﺔﻴﺣﻼﺻ ﻥﺃ ﻭﺪﺒﻳ ⚠",
                                    );
                                    if ui.button("...ﻥﻵﺍ ﻂﺑﺍﺮﻟﺍ ﺚﻳﺪﺤﺗ 🔗").clicked() {
                                        request_refresh_url = true;
                                    }
                                });
                            }
                        }

                        // 8.4 تبويب Logs — السجل (مع حجب تام للأسرار والكوكيز)
                        DialogTab::Logs => {
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(format!(
                                        "(:ﺎﻴﺋﺎﻘﻠﺗ ﺔﺑﻮﺠﺤﻣ ﺰﻴﻛﻮﻜﻟﺍﻭ ﺭﺍﺮﺳﻷﺍ — ﺙﺪﺣ ){} ﺔﻤﻬﻤﻟﺍ ﺙﺍﺪﺣﺃ ﻞﺠﺳ",
                                        task.connection_logs.len()
                                    ))
                                    .strong(),
                                );
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    if ui.small_button("ﻞﺠﺴﻟﺍ ﺦﺴﻧ 📋").clicked() {
                                        let all_logs = task
                                            .connection_logs
                                            .iter()
                                            .map(|l| {
                                                format!(
                                                    "[{}] [{}] {}",
                                                    l.timestamp.format("%H:%M:%S"),
                                                    l.level,
                                                    crate::core::storage::redact_log_message(
                                                        &l.message
                                                    )
                                                )
                                            })
                                            .collect::<Vec<_>>()
                                            .join("\n");
                                        ui.ctx().copy_text(all_logs);
                                    }
                                });
                            });
                            ui.separator();
                            egui::ScrollArea::vertical()
                                .max_height(155.0)
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    if task.connection_logs.is_empty() {
                                        ui.label(
                                            RichText::new(".ﺪﻌﺑ ﺔﻤﻬﻤﻟﺍ ﻩﺬﻬﻟ ﺔﻠﺠﺴﻣ ﺙﺍﺪﺣﺃ ﺪﺟﻮﺗ ﻻ")
                                                .color(palette.text_muted),
                                        );
                                    } else {
                                        for entry in task.connection_logs.iter().rev() {
                                            let lvl_col = match entry.level.as_str() {
                                                "ERROR" => palette.color_error,
                                                "WARN" => palette.color_warning,
                                                _ => palette.color_info,
                                            };
                                            let seg_tag = entry
                                                .segment_id
                                                .map(|id| format!(" [Seg #{}]", id + 1))
                                                .unwrap_or_default();
                                            let safe_msg =
                                                crate::core::storage::redact_log_message(
                                                    &entry.message,
                                                );
                                            ui.horizontal_wrapped(|ui| {
                                                ui.colored_label(
                                                    lvl_col,
                                                    RichText::new(format!(
                                                        "[{}]{} [{}]",
                                                        entry.timestamp.format("%H:%M:%S"),
                                                        seg_tag,
                                                        entry.level
                                                    ))
                                                    .monospace()
                                                    .size(11.0),
                                                );
                                                ui.label(RichText::new(safe_msg).size(11.5));
                                            });
                                        }
                                    }
                                });
                        }

                        // 8.5 تبويب Integrity — سلامة الملف (لا يعرض نجاحاً إلا إذا نُفذ الفحص فعلاً)
                        DialogTab::Integrity => {
                            egui::Grid::new("details_integrity_grid")
                                .num_columns(3)
                                .spacing([14.0, 6.0])
                                .show(ui, |ui| {
                                    ui.label(RichText::new("ﺔﻴﻣﺯﺭﺍﻮﺨﻟﺍ").strong());
                                    ui.label(RichText::new("ﺎﻴﻠﻌﻓ ﺔﺑﻮﺴﺤﻤﻟﺍ ﺔﻤﻴﻘﻟﺍ").strong());
                                    ui.label(RichText::new("ﺔﻧﺭﺎﻘﻤﻟﺍ ﺔﻟﺎﺣ").strong());
                                    ui.end_row();

                                    for (algo, computed, expected) in [
                                        ("SHA-256", &task.computed_sha256, &task.expected_sha256),
                                        ("MD5", &task.computed_md5, &task.expected_md5),
                                        ("CRC32", &task.computed_crc32, &task.expected_crc32),
                                    ] {
                                        ui.label(RichText::new(algo).strong());
                                        ui.label(
                                            RichText::new(
                                                computed.as_deref().unwrap_or("ﺪﻌﺑ ﺐﺴﺤﻳ ﻢﻟ"),
                                            )
                                            .monospace()
                                            .size(11.0),
                                        );
                                        match (computed.as_deref(), expected.as_deref()) {
                                            (Some(c), Some(e)) if !e.trim().is_empty() => {
                                                if c.eq_ignore_ascii_case(e.trim()) {
                                                    ui.colored_label(
                                                        palette.color_success,
                                                        "ﺎﻣﺎﻤﺗ ﻖﺑﺎﻄﺘﻣ ✔",
                                                    );
                                                } else {
                                                    ui.colored_label(
                                                        palette.color_error,
                                                        "!ﻖﺑﺎﻄﺘﻣ ﺮﻴﻏ ✖",
                                                    );
                                                }
                                            }
                                            (Some(_), _) => {
                                                ui.colored_label(
                                                    palette.color_info,
                                                    "(ﺔﻧﺭﺎﻘﻤﻠﻟ ﺔﻴﻌﺟﺮﻣ ﺔﻤﻴﻗ ﻥﻭﺪﺑ) ﺏﺎﺴﺤﻟﺍ ﻢﺗ",
                                                );
                                            }
                                            (None, Some(e)) if !e.trim().is_empty() => {
                                                ui.colored_label(
                                                    palette.color_warning,
                                                    "ﻖﻘﺤﺘﻠﻟ ﻞﻳﺰﻨﺘﻟﺍ ﻝﺎﻤﺘﻛﺍ ﺭﺎﻈﺘﻧﺎﺑ",
                                                );
                                            }
                                            (None, _) => {
                                                ui.label(
                                                    RichText::new("ﺪﻌﺑ ﻖﻘﺤﺘﻟﺍ ﺬﻔﻨﻳ ﻢﻟ")
                                                        .color(palette.text_muted),
                                                );
                                            }
                                        }
                                        ui.end_row();
                                    }
                                });

                            ui.add_space(8.0);
                            let can_verify = task.status == DownloadStatus::Completed
                                && task.final_file_path().exists();
                            if ui
                                .add_enabled(
                                    can_verify,
                                    egui::Button::new(
                                        "(SHA-256 / MD5 / CRC32) ﻥﻵﺍ ﺔﻴﻤﻗﺮﻟﺍ ﺕﺎﻤﺼﺒﻟﺍ ﻦﻣ ﻖﻘﺤﺘﻟﺍ ﺓﺩﺎﻋﺇﻭ ﺏﺎﺴﺣ 🔍",
                                    ),
                                )
                                .on_hover_text("ﺹﺮﻘﻟﺍ ﻰﻠﻋ ﺓﺩﻮﺟﻮﻤﻟﺍ ﺔﻠﻤﺘﻜﻤﻟﺍ ﺕﺎﻔﻠﻤﻠﻟ ﺡﺎﺘﻣ")
                                .clicked()
                            {
                                request_reverify = true;
                            }
                        }

                        // تبويب محدد السرعة
                        DialogTab::SpeedLimiter => {
                            ui.horizontal(|ui| {
                                ui.label(":ﻲﻟﺎﺤﻟﺍ ﻞﻘﻨﻟﺍ ﻝﺪﻌﻣ");
                                ui.label(
                                    RichText::new(loc.format_speed(total_speed))
                                        .monospace()
                                        .strong(),
                                );
                            });
                            ui.add_space(4.0);
                            if ui
                                .checkbox(
                                    &mut app.speed_limiter_enabled,
                                    "ﻞﻴﻤﺤﺘﻟﺍ ﺀﺎﻨﺛﺃ ﻡﺎﻌﻟﺍ ﺔﻋﺮﺴﻟﺍ ﺩﺪﺤﻣ ﻞﻴﻌﻔﺗ",
                                )
                                .changed()
                            {
                                let limit = if app.speed_limiter_enabled {
                                    app.speed_limit_kbps.max(10)
                                } else {
                                    0
                                };
                                let _ = app.commands.send(UiCommand::SetSpeedLimit(limit));
                            }
                            ui.horizontal(|ui| {
                                ui.label("(KB/s): ﺔﺣﻮﻤﺴﻤﻟﺍ ﻯﻮﺼﻘﻟﺍ ﺔﻋﺮﺴﻟﺍ");
                                let mut speed_val = app.speed_limit_kbps.max(10);
                                if ui
                                    .add(egui::DragValue::new(&mut speed_val).range(1..=100_000))
                                    .changed()
                                {
                                    app.speed_limit_kbps = speed_val;
                                    if app.speed_limiter_enabled {
                                        let _ =
                                            app.commands.send(UiCommand::SetSpeedLimit(speed_val));
                                    }
                                }
                            });
                        }

                        // تبويب الخيارات عند اكتمال التحميل
                        DialogTab::OptionsOnCompletion => {
                            let save_to_path =
                                task.save_dir.join(&task.filename).display().to_string();
                            ui.horizontal(|ui| {
                                ui.label(":ﻲﺋﺎﻬﻨﻟﺍ ﻆﻔﺤﻟﺍ ﺭﺎﺴﻣ");
                                ui.label(RichText::new(save_to_path).monospace());
                            });
                            ui.checkbox(
                                &mut app.completion_notify,
                                "ﻞﻳﺰﻨﺘﻟﺍ ﻝﺎﻤﺘﻛﺍ ﺪﻨﻋ ﺭﺎﻌﺷﺇ ﺽﺮﻋ",
                            );
                            ui.checkbox(
                                &mut app.completion_exit_idm,
                                "ﺔﻤﻬﻤﻟﺍ ﺀﺎﻬﺘﻧﺍ ﺪﻨﻋ ﺎﻴﺋﺎﻘﻠﺗ StormGet ﻦﻣ ﺝﻭﺮﺨﻟﺍ",
                            );
                            ui.horizontal(|ui| {
                                ui.checkbox(
                                    &mut app.completion_shutdown_pc,
                                    ":ﺀﺎﻬﺘﻧﻻﺍ ﺪﻨﻋ ﻡﺎﻈﻨﻠﻟ ﺔﻗﺎﻃ ﺀﺍﺮﺟﺇ ﺬﻴﻔﻨﺗ",
                                );
                                egui::ComboBox::from_id_salt("shutdown_action_combo")
                                    .selected_text(app.completion_shutdown_action.label())
                                    .show_ui(ui, |ui| {
                                        for action in [
                                            ShutdownAction::Shutdown,
                                            ShutdownAction::Restart,
                                            ShutdownAction::Sleep,
                                        ] {
                                            ui.selectable_value(
                                                &mut app.completion_shutdown_action,
                                                action,
                                                action.label(),
                                            );
                                        }
                                    });
                            });
                        }
                    }
                });

            ui.add_space(6.0);
            draw_idm_green_progress_bar(ui, palette, progress);
            ui.add_space(6.0);

            // أزرار التحكم السفلية: إظهار/إخفاء المقاطع + استئناف/إيقاف مؤقت + إلغاء التنزيل + إغلاق النافذة
            // (قاعدة إلزامية: إغلاق نافذة التفاصيل لا يلغي التنزيل نفسه)
            ui.horizontal(|ui| {
                let details_label = if app.show_connection_details {
                    "▲ ﺕﻻﺎﺼﺗﻻﺍ ﺔﻄﻳﺮﺧ ﺀﺎﻔﺧﺇ"
                } else {
                    "▼ ﺕﻻﺎﺼﺗﻻﺍ ﺔﻄﻳﺮﺧ ﺽﺮﻋ"
                };
                if ui
                    .add_sized([155.0, 26.0], egui::Button::new(details_label))
                    .clicked()
                {
                    app.show_connection_details = !app.show_connection_details;
                }
                if matches!(task.category, FileCategory::Video | FileCategory::Music)
                    && ui
                        .button("🎬 ﺔﻨﻳﺎﻌﻣ ▶")
                        .on_hover_text("معاينة الفيديو أو الصوت أثناء التحميل (Play Preview)")
                        .clicked()
                {
                    let _ = app
                        .commands
                        .send(UiCommand::PreviewTask(task.task_id.clone()));
                }
                if matches!(
                    task.status,
                    DownloadStatus::Failed | DownloadStatus::NeedsUrlRefresh
                ) && ui
                    .button("🩺 ﻞﻄﻌﻟﺍ ﺺﻴﺨﺸﺗ")
                    .on_hover_text("تشخيص سبب فشل التحميل واقتراح الحل الفوري (Download Doctor)")
                    .clicked()
                {
                    let _ = app
                        .commands
                        .send(UiCommand::DiagnoseTask(task.task_id.clone()));
                }

                if task.status == DownloadStatus::Completed {
                    if ui
                        .add_enabled(
                            task.final_file_path().exists(),
                            egui::Button::new("ﻒﻠﻤﻟﺍ ﺢﺘﻓ 📂"),
                        )
                        .clicked()
                    {
                        open_file(&task.final_file_path());
                    }
                    if ui.button("ﺪﻠﺠﻤﻟﺍ ﺢﺘﻓ 📁").clicked() {
                        open_folder(&task.final_file_path());
                    }
                } else {
                    let is_active = matches!(
                        task.status,
                        DownloadStatus::Downloading | DownloadStatus::RefreshingUrl
                    );
                    if is_active {
                        if ui
                            .add_sized([115.0, 26.0], egui::Button::new("ﺖﻗﺆﻣ ﻑﺎﻘﻳﺇ ⏸"))
                            .clicked()
                        {
                            request_pause = true;
                        }
                    } else if !task.is_cancelled_by_user() {
                        if ui
                            .add_sized(
                                [115.0, 26.0],
                                egui::Button::new(RichText::new("ﻑﺎﻨﺌﺘﺳﺍ ▶").strong()),
                            )
                            .clicked()
                        {
                            request_resume = true;
                        }
                    }

                    if !matches!(task.status, DownloadStatus::Completed | DownloadStatus::Failed) {
                        if ui
                            .add_sized([110.0, 26.0], egui::Button::new("ﺔﻤﻬﻤﻟﺍ ﺀﺎﻐﻟﺇ ⏹"))
                            .on_hover_text("ﻪﻓﺎﻘﻳﺇﻭ ﻞﻳﺰﻨﺘﻟﺍ ﺀﺎﻐﻟﺇ")
                            .clicked()
                        {
                            request_cancel = true;
                        }
                    }
                }

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui
                        .add_sized([110.0, 26.0], egui::Button::new("ﺓﺬﻓﺎﻨﻟﺍ ﻕﻼﻏﺇ"))
                        .on_hover_text("ﺔﻴﻔﻠﺨﻟﺍ ﻲﻓ ﺍﺮﻤﺘﺴﻣ ﻞﻳﺰﻨﺘﻟﺍ ﺀﺎﻘﺑ ﻊﻣ ﻞﻴﺻﺎﻔﺘﻟﺍ ﺓﺬﻓﺎﻧ ﻕﻼﻏﺇ")
                        .clicked()
                    {
                        app.show_details = false;
                    }
                });
            });

            if app.show_connection_details {
                ui.add_space(4.0);
                ui.label(
                    RichText::new(":ﺔﻳﺯﺍﻮﺘﻤﻟﺍ ﺕﻻﺎﺼﺗﻻﺍﻭ ﻊﻃﺎﻘﻤﻟﺍ ﻢﻴﺴﻘﺗ ﺔﻄﻳﺮﺧ")
                        .strong()
                        .size(11.5)
                        .color(palette.text_main),
                );
                draw_idm_connections_bar(ui, palette, &task, progress);
                ui.add_space(3.0);
                draw_idm_connections_table(ui, palette, &task, progress);
            }
        });

    if request_pause {
        let _ = app.commands.send(UiCommand::Control {
            task_id: task.task_id.clone(),
            action: "pause",
        });
        app.status_message = "ﺎﺘﻗﺆﻣ ﺔﻤﻬﻤﻟﺍ ﻑﺎﻘﻳﺇ ﻢﺗ".to_owned();
    } else if request_resume {
        let _ = app.commands.send(UiCommand::Control {
            task_id: task.task_id.clone(),
            action: "resume",
        });
        app.status_message = "ﺔﻤﻬﻤﻟﺍ ﻑﺎﻨﺌﺘﺳﺍ ﻢﺗ".to_owned();
    } else if request_cancel {
        app.request_cancel_tasks(vec![task.task_id.clone()]);
    } else if request_reverify {
        let _ = app
            .commands
            .send(UiCommand::ReverifyChecksum(task.task_id.clone()));
    } else if request_refresh_url {
        app.refresh_task_id = task.task_id.clone();
        app.refresh_new_url = task.url.clone();
        app.show_refresh_dialog = true;
    }
}

// ============================================================================
// المرحلة التاسعة: نافذة الجدولة والطوابير المستقلة (Scheduler Window)
// ============================================================================

fn draw_independent_scheduler_viewport(
    app: &mut RustIdmApp,
    ctx: &egui::Context,
    loc: &Loc,
    palette: &ThemePalette,
) {
    if !app.show_scheduler_dialog {
        return;
    }

    let (viewport_id, builder, should_focus) = build_and_track_secondary_viewport(
        app,
        "scheduler",
        loc.scheduler_window_title(),
        760.0,
        540.0,
        640.0,
        420.0,
        true,
    );

    ctx.show_viewport_immediate(viewport_id, builder, |vp_ctx, _| {
        if should_focus {
            vp_ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }
        if vp_ctx.input(|i| i.viewport().close_requested()) {
            app.show_scheduler_dialog = false;
            return;
        }
        record_secondary_viewport_geometry(app, "scheduler", vp_ctx);
        apply_stormget_theme_with_scale(vp_ctx, app.theme_mode, app.ui_state.font_scale);

        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(palette.window_bg)
                    .inner_margin(egui::Margin::same(14)),
            )
            .show(vp_ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.heading(
                        RichText::new(loc.scheduler_window_title())
                            .color(palette.text_main)
                            .strong(),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.button("ﻕﻼﻏﺇ").clicked() {
                            app.show_scheduler_dialog = false;
                        }
                    });
                });
                ui.label(
                    RichText::new(
                        ".ﺄﻔﻄﻣ ﺯﺎﻬﺠﻟﺍ ﻭﺃ ﻞﻣﺎﻜﻟﺎﺑ ﺎﻘﻠﻐﻣ ﺞﻣﺎﻧﺮﺒﻟﺍ ﻥﺎﻛ ﺍﺫﺇ ﺔﻟﻭﺪﺠﻟﺍ ﻞﻤﻌﺗ ﻻ .ﻂﺸﻨﻟﺍ ﻞﻳﺰﻨﺘﻟﺍ ﺀﺎﻨﺛﺃ ﻥﻮﻜﺴﻟﺍ ﻊﺿﻭ ﻲﻓ ﺯﺎﻬﺠﻟﺍ ﻝﻮﺧﺩ ﻙﺮﺤﻤﻟﺍ ﻊﻨﻤﻳﻭ ،(ﺔﻴﻔﻠﺨﻟﺍ/ﻡﺎﻬﻤﻟﺍ ﻂﻳﺮﺷ ﻲﻓ ﻭﺃ ﺓﺬﻓﺎﻨﻟﺍ ﻲﻓ) ﻂﺸﻧ ﺞﻣﺎﻧﺮﺒﻟﺍ ﻥﺃ ﺎﻤﻟﺎﻃ ﺎﻴﺋﺎﻘﻠﺗ ﺔﻟﻭﺪﺠﻟﺍ ﻞﻤﻌﺗ :ﺔﻣﺎﻫ ﺔﻈﺣﻼﻣ ℹ",
                    )
                    .size(11.0)
                    .color(palette.text_muted),
                );
                ui.separator();

                // 9.1 جدول الطوابير والجدولات الحالية
                ui.label(RichText::new(":ﺔﻴﻨﻣﺰﻟﺍ ﺕﻻﻭﺪﺠﻟﺍﻭ ﺮﻴﺑﺍﻮﻄﻟﺍ ﺔﻤﺋﺎﻗ").strong());
                egui::Frame::canvas(ui.style())
                    .fill(palette.table_bg)
                    .stroke(Stroke::new(1.0, palette.border))
                    .inner_margin(6.0)
                    .show(ui, |ui| {
                        egui::Grid::new("scheduler_queues_table")
                            .striped(true)
                            .num_columns(7)
                            .spacing([14.0, 6.0])
                            .show(ui, |ui| {
                                for header in [
                                    "ﺭﻮﺑﺎﻄﻟﺍ / ﺔﻟﻭﺪﺠﻟﺍ ﻢﺳﺍ",
                                    "ﻂﻤﻨﻟﺍﻭ ﺔﻟﺎﺤﻟﺍ",
                                    "ﺀﺪﺒﻟﺍ ﺖﻗﻭ",
                                    "ﺀﺎﻬﺘﻧﻻﺍ ﺖﻗﻭ",
                                    "ﻡﺎﻳﻷﺍ",
                                    "ﻡﺎﻬﻤﻟﺍ ﺩﺪﻋ",
                                    "ﺔﻌﻳﺮﺳ ﺮﻣﺍﻭﺃ",
                                ] {
                                    ui.label(RichText::new(header).strong().color(palette.text_muted));
                                }
                                ui.end_row();

                                let queues_snapshot = if app.queues.is_empty() {
                                    vec![QueueConfig {
                                        name: "main".to_owned(),
                                        max_concurrent_downloads: 4,
                                        start_time: app.schedule.start_time.clone(),
                                        stop_time: app.schedule.stop_time.clone(),
                                        post_action: PostQueueAction::Notify,
                                        schedule_mode: app.schedule.mode,
                                        start_at: app.schedule.start_at.clone(),
                                        stop_at: app.schedule.stop_at.clone(),
                                        days_of_week: app.schedule.days_of_week.clone(),
                                        speed_limit_kbps: app.schedule.speed_limit_kbps,
                                    }]
                                } else {
                                    app.queues.clone()
                                };

                                for q in queues_snapshot {
                                    let task_count = app
                                        .downloads
                                        .iter()
                                        .filter(|d| d.queue_name == q.name)
                                        .count();
                                    let is_sel = app.selected_queue_name == q.name;
                                    if ui
                                        .selectable_label(is_sel, RichText::new(&q.name).strong())
                                        .clicked()
                                    {
                                        app.load_queue_into_scheduler_editor(&q);
                                    }

                                    let mode_str = match q.schedule_mode {
                                        ScheduleMode::Disabled => "(ﻱﻭﺪﻳ) ﺔﻠﻄﻌﻣ",
                                        ScheduleMode::Daily => "(ﺓﺭﺮﻜﺘﻣ) ﺔﻠﻌﻔﻣ",
                                        ScheduleMode::Once => "(ﺓﺪﺣﺍﻭ ﺓﺮﻣ) ﺔﻠﻌﻔﻣ",
                                    };
                                    ui.colored_label(
                                        if q.schedule_mode == ScheduleMode::Disabled {
                                            palette.text_muted
                                        } else {
                                            palette.color_success
                                        },
                                        mode_str,
                                    );

                                    let start_str = match q.schedule_mode {
                                        ScheduleMode::Daily => {
                                            q.start_time.as_deref().unwrap_or("—")
                                        }
                                        ScheduleMode::Once => {
                                            q.start_at.as_deref().unwrap_or("—")
                                        }
                                        ScheduleMode::Disabled => "—",
                                    };
                                    ui.label(RichText::new(start_str).monospace());

                                    let stop_str = match q.schedule_mode {
                                        ScheduleMode::Daily => {
                                            q.stop_time.as_deref().unwrap_or("—")
                                        }
                                        ScheduleMode::Once => {
                                            q.stop_at.as_deref().unwrap_or("ﻝﺎﻤﺘﻛﻻﺍ ﻰﺘﺣ")
                                        }
                                        ScheduleMode::Disabled => "—",
                                    };
                                    ui.label(RichText::new(stop_str).monospace());

                                    let days_str = if q.days_of_week.is_empty() {
                                        "ﻡﺎﻳﻷﺍ ﻞﻛ".to_owned()
                                    } else {
                                        format!("ﺓﺩﺪﺤﻣ ﻡﺎﻳﺃ {}", q.days_of_week.len())
                                    };
                                    ui.label(days_str);
                                    ui.label(format!("ﻡﺎﻬﻣ {task_count}"));

                                    ui.horizontal(|ui| {
                                        if ui.small_button("ﻞﻳﺪﻌﺗ ✏").clicked() {
                                            app.load_queue_into_scheduler_editor(&q);
                                        }
                                        if ui
                                            .small_button("ﻥﻵﺍ ﻞﻴﻐﺸﺗ ▶")
                                            .on_hover_text("ﺍﺭﻮﻓ ﺭﻮﺑﺎﻄﻟﺍ ﺍﺬﻫ ﻡﺎﻬﻣ ﻊﻴﻤﺟ ﺀﺪﺑ")
                                            .clicked()
                                        {
                                            app.start_specific_queue(&q.name);
                                        }
                                        if q.name != "main"
                                            && ui
                                                .small_button("ﻑﺬﺣ 🗑")
                                                .on_hover_text("main ﻰﻟﺇ ﻪﻣﺎﻬﻣ ﻞﻘﻧﻭ ﺭﻮﺑﺎﻄﻟﺍ ﻑﺬﺣ")
                                                .clicked()
                                        {
                                            let _ = app
                                                .commands
                                                .send(UiCommand::DeleteQueue(q.name.clone()));
                                        }
                                    });
                                    ui.end_row();
                                }
                            });
                    });

                ui.add_space(8.0);

                // إنشاء طابور جديد
                ui.horizontal(|ui| {
                    ui.label(":ﺓﺪﻳﺪﺟ ﺔﻟﻭﺪﺟ / ﺭﻮﺑﺎﻃ");
                    ui.add(
                        egui::TextEdit::singleline(&mut app.new_queue_name_input)
                            .hint_text("(night_batch :ﻝﺎﺜﻣ) ﺭﻮﺑﺎﻄﻟﺍ ﻢﺳﺍ")
                            .desired_width(200.0),
                    );
                    if ui
                        .add_enabled(
                            !app.new_queue_name_input.trim().is_empty(),
                            egui::Button::new("ﺪﻳﺪﺟ ﺭﻮﺑﺎﻃ ﺀﺎﺸﻧﺇ ➕"),
                        )
                        .clicked()
                    {
                        let name = app.new_queue_name_input.trim().to_owned();
                        let new_q = QueueConfig {
                            name: name.clone(),
                            max_concurrent_downloads: 4,
                            start_time: Some("22:00".to_owned()),
                            stop_time: Some("06:00".to_owned()),
                            post_action: PostQueueAction::Notify,
                            schedule_mode: ScheduleMode::Disabled,
                            start_at: None,
                            stop_at: None,
                            days_of_week: Vec::new(),
                            speed_limit_kbps: 0,
                        };
                        let _ = app.commands.send(UiCommand::UpsertQueue(new_q.clone()));
                        app.load_queue_into_scheduler_editor(&new_q);
                        app.new_queue_name_input.clear();
                    }
                });

                ui.separator();

                // 9.2 محرر تفاصيل الجدولة للطابور المحدد
                ui.label(
                    RichText::new(format!(
                        "'{}' :ﺩﺪﺤﻤﻟﺍ ﺭﻮﺑﺎﻄﻠﻟ ﺔﻟﻭﺪﺠﻟﺍ ﺕﺍﺩﺍﺪﻋﺇ",
                        app.selected_queue_name
                    ))
                    .strong()
                    .color(palette.color_info),
                );

                egui::Frame::canvas(ui.style())
                    .fill(palette.bg_secondary)
                    .stroke(Stroke::new(1.0, palette.border))
                    .inner_margin(10.0)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(":ﺔﻟﻭﺪﺠﻟﺍ ﻉﻮﻧ");
                            ui.radio_value(
                                &mut app.sched_editor_mode,
                                ScheduleMode::Disabled,
                                "(ﻱﻭﺪﻳ ﻞﻴﻐﺸﺗ) ﺔﻠﻄﻌﻣ",
                            );
                            ui.radio_value(
                                &mut app.sched_editor_mode,
                                ScheduleMode::Daily,
                                "(ﺓﺩﺪﺤﻣ ﻡﺎﻳﺃ / ﺔﻴﻣﻮﻳ) ﺓﺭﺮﻜﺘﻣ",
                            );
                            ui.radio_value(
                                &mut app.sched_editor_mode,
                                ScheduleMode::Once,
                                "(ﺩﺪﺤﻣ ﺖﻗﻭﻭ ﺦﻳﺭﺎﺗ) ﺓﺪﺣﺍﻭ ﺓﺮﻤﻟ",
                            );
                        });

                        ui.add_space(4.0);

                        match app.sched_editor_mode {
                            ScheduleMode::Disabled => {
                                ui.label(
                                    RichText::new(".ﺎﻳﻭﺪﻳ ﺎﻬﺋﺪﺑ ﺪﻨﻋ ﻡﺎﻬﻤﻟﺍ ﻞﻤﻌﺗ ؛ﺭﻮﺑﺎﻄﻟﺍ ﺍﺬﻬﻟ ﺔﻠﻄﻌﻣ ﺔﻴﺋﺎﻘﻠﺘﻟﺍ ﺔﻟﻭﺪﺠﻟﺍ")
                                        .color(palette.text_muted),
                                );
                            }
                            ScheduleMode::Daily => {
                                ui.horizontal(|ui| {
                                    ui.label("(HH:MM): ﺀﺪﺒﻟﺍ ﺖﻗﻭ");
                                    ui.add(
                                        egui::TextEdit::singleline(&mut app.sched_editor_start_time)
                                            .hint_text("22:00")
                                            .desired_width(80.0),
                                    );
                                    ui.separator();
                                    ui.label("(HH:MM): ﺀﺎﻬﺘﻧﻻﺍ ﺖﻗﻭ");
                                    ui.add(
                                        egui::TextEdit::singleline(&mut app.sched_editor_stop_time)
                                            .hint_text("06:00")
                                            .desired_width(80.0),
                                    );
                                    ui.label(
                                        RichText::new("(06:00 ﻰﻟﺇ 22:00 ﻞﺜﻣ ﻞﻴﻠﻟﺍ ﻒﺼﺘﻨﻣ ﺪﻌﺑ ﺭﻮﺒﻌﻟﺍ ﻢﻋﺪﻳ)")
                                            .size(11.0)
                                            .color(palette.text_muted),
                                    );
                                });

                                ui.horizontal(|ui| {
                                    ui.label(":ﺭﺍﺮﻜﺘﻟﺍ ﻡﺎﻳﺃ");
                                    for (idx, day_name) in [
                                        (0usize, "ﻦﻴﻨﺛﻹﺍ"),
                                        (1, "ﺀﺎﺛﻼﺜﻟﺍ"),
                                        (2, "ﺀﺎﻌﺑﺭﻷﺍ"),
                                        (3, "ﺲﻴﻤﺨﻟﺍ"),
                                        (4, "ﺔﻌﻤﺠﻟﺍ"),
                                        (5, "ﺖﺒﺴﻟﺍ"),
                                        (6, "ﺪﺣﻷﺍ"),
                                    ] {
                                        ui.checkbox(&mut app.sched_editor_days[idx], day_name);
                                    }
                                });
                            }
                            ScheduleMode::Once => {
                                ui.horizontal(|ui| {
                                    ui.label("(YYYY-MM-DD HH:MM): ﺀﺪﺒﻟﺍ ﺖﻗﻭﻭ ﺦﻳﺭﺎﺗ");
                                    ui.add(
                                        egui::TextEdit::singleline(&mut app.sched_editor_once_start)
                                            .hint_text("2026-10-27 01:00")
                                            .desired_width(160.0),
                                    );
                                    ui.separator();
                                    ui.label("(:ﻱﺭﺎﻴﺘﺧﺍ) ﻒﻗﻮﺘﻟﺍ ﺖﻗﻭﻭ ﺦﻳﺭﺎﺗ");
                                    ui.add(
                                        egui::TextEdit::singleline(&mut app.sched_editor_once_stop)
                                            .hint_text("2026-10-27 07:00")
                                            .desired_width(160.0),
                                    );
                                });
                            }
                        }

                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            ui.label(":ﺭﻮﺑﺎﻄﻟﺍ ﻲﻓ ﺔﻨﻣﺍﺰﺘﻤﻟﺍ ﺕﻼﻳﺰﻨﺘﻠﻟ ﻰﺼﻗﻷﺍ ﺪﺤﻟﺍ");
                            ui.add(
                                egui::DragValue::new(&mut app.sched_editor_max_concurrent)
                                    .range(1..=32),
                            );
                            ui.separator();
                            ui.label("(:ﺪﺣ ﻼﺑ = 0 ،s/KB) ﺔﻟﻭﺪﺠﻟﺍ ﺀﺎﻨﺛﺃ ﺔﻋﺮﺴﻠﻟ ﻰﺼﻗﻷﺍ ﺪﺤﻟﺍ");
                            ui.add(
                                egui::DragValue::new(&mut app.sched_editor_speed_limit_kbps)
                                    .range(0..=100_000),
                            );
                        });

                        ui.horizontal(|ui| {
                            ui.label(":ﺭﻮﺑﺎﻄﻟﺍ ﻡﺎﻬﻣ ﻝﺎﻤﺘﻛﺍ ﺪﻨﻋ ﺞﻣﺎﻧﺮﺒﻟﺍ ﻙﻮﻠﺳ");
                            egui::ComboBox::from_id_salt("sched_post_action_combo")
                                .selected_text(match app.sched_editor_post_action_idx {
                                    0 => "ﺀﻴﺷ ﻻ",
                                    1 => "ﺭﻮﺑﺎﻄﻟﺍ ﻝﺎﻤﺘﻛﺍ ﺭﺎﻌﺷﺇ ﺽﺮﻋ",
                                    _ => "ﻝﺎﻤﺘﻛﻻﺍ ﺪﻌﺑ ﺞﻣﺎﻧﺮﺒﻟﺍ ﻕﻼﻏﺇ",
                                })
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(
                                        &mut app.sched_editor_post_action_idx,
                                        0,
                                        "ﺀﻴﺷ ﻻ",
                                    );
                                    ui.selectable_value(
                                        &mut app.sched_editor_post_action_idx,
                                        1,
                                        "ﺭﻮﺑﺎﻄﻟﺍ ﻝﺎﻤﺘﻛﺍ ﺭﺎﻌﺷﺇ ﺽﺮﻋ",
                                    );
                                    ui.selectable_value(
                                        &mut app.sched_editor_post_action_idx,
                                        2,
                                        "ﻝﺎﻤﺘﻛﻻﺍ ﺪﻌﺑ ﺞﻣﺎﻧﺮﺒﻟﺍ ﻕﻼﻏﺇ",
                                    );
                                });
                        });

                        if let Some(err) = &app.sched_validation_error {
                            ui.colored_label(palette.color_error, format!("⚠ {err}"));
                        }

                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            if ui
                                .button(
                                    RichText::new("ﺔﻟﻭﺪﺠﻟﺍ ﻖﻴﺒﻄﺗﻭ ﻆﻔﺣ 💾")
                                        .strong()
                                        .color(Color32::WHITE),
                                )
                                .clicked()
                            {
                                app.save_scheduler_editor_changes();
                            }
                            if ui.button("ﻥﻵﺍ ﺭﻮﺑﺎﻄﻟﺍ ﻞﻴﻐﺸﺗ ▶").clicked() {
                                let q = app.selected_queue_name.clone();
                                app.start_specific_queue(&q);
                            }
                            if ui.button("ﺭﻮﺑﺎﻄﻟﺍ ﻡﺎﻬﻣ ﻑﺎﻘﻳﺇ ⏸").clicked() {
                                let q = app.selected_queue_name.clone();
                                app.stop_specific_queue(&q);
                            }
                        });
                    });
            });
    });
}

// ============================================================================
// المرحلة العاشرة: نافذة الإعدادات المستقلة (8 أقسام جانبية)
// ============================================================================

fn draw_independent_settings_viewport(
    app: &mut RustIdmApp,
    ctx: &egui::Context,
    loc: &Loc,
    palette: &ThemePalette,
) {
    if !app.show_options_dialog {
        return;
    }

    let (viewport_id, builder, should_focus) = build_and_track_secondary_viewport(
        app,
        "settings",
        loc.settings_window_title(),
        800.0,
        560.0,
        680.0,
        450.0,
        true,
    );

    ctx.show_viewport_immediate(viewport_id, builder, |vp_ctx, _| {
        if should_focus {
            vp_ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }
        if vp_ctx.input(|i| i.viewport().close_requested()) {
            app.show_options_dialog = false;
            return;
        }
        record_secondary_viewport_geometry(app, "settings", vp_ctx);
        apply_stormget_theme_with_scale(vp_ctx, app.theme_mode, app.ui_state.font_scale);

        // قائمة الأقسام الجانبية الـ 8
        egui::SidePanel::left("settings_sidebar_tabs")
            .resizable(false)
            .exact_width(195.0)
            .frame(
                egui::Frame::new()
                    .fill(palette.sidebar_bg)
                    .stroke(Stroke::new(1.0, palette.border))
                    .inner_margin(egui::Margin::same(10)),
            )
            .show(vp_ctx, |ui| {
                ui.label(
                    RichText::new(loc.tr("ﺕﺍﺩﺍﺪﻋﻹﺍ ﻡﺎﺴﻗﺃ", "Settings Sections"))
                        .strong()
                        .size(13.0)
                        .color(palette.text_main),
                );
                ui.separator();
                ui.spacing_mut().item_spacing = Vec2::new(0.0, 4.0);

                for tab in [
                    OptionsTab::General,
                    OptionsTab::Appearance,
                    OptionsTab::Downloads,
                    OptionsTab::Connections,
                    OptionsTab::BrowserIntegration,
                    OptionsTab::Media,
                    OptionsTab::Notifications,
                    OptionsTab::Advanced,
                ] {
                    let is_sel = app.options_tab == tab;
                    if ui
                        .add_sized(
                            [ui.available_width(), 28.0],
                            egui::Button::selectable(is_sel, loc.options_tab_name(tab)),
                        )
                        .clicked()
                    {
                        app.options_tab = tab;
                        app.ui_state.last_options_tab = tab;
                        app.mark_ui_state_dirty();
                    }
                }
            });

        // شريط الأزرار السفلي (تطبيق وحفظ / إلغاء / استعادة الافتراضيات)
        egui::TopBottomPanel::bottom("settings_bottom_bar")
            .exact_height(46.0)
            .frame(
                egui::Frame::new()
                    .fill(palette.panel_bg)
                    .stroke(Stroke::new(1.0, palette.border))
                    .inner_margin(egui::Margin::symmetric(14, 8)),
            )
            .show(vp_ctx, |ui| {
                ui.horizontal(|ui| {
                    if ui
                        .button(
                            RichText::new(loc.tr("ﻙﺮﺤﻤﻟﺍ ﻰﻠﻋ ﻖﻴﺒﻄﺗﻭ ﻆﻔﺣ 💾", "💾 Save & Apply"))
                                .strong()
                                .color(Color32::WHITE),
                        )
                        .clicked()
                    {
                        app.save_and_apply_all_settings();
                    }
                    if ui.button(loc.tr("ﻕﻼﻏﺇ", "Close")).clicked() {
                        app.show_options_dialog = false;
                    }
                });
            });

        // محتوى القسم المحدد
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(palette.window_bg)
                    .inner_margin(egui::Margin::same(16)),
            )
            .show(vp_ctx, |ui| {
                ui.heading(
                    RichText::new(loc.options_tab_name(app.options_tab))
                        .color(palette.text_main)
                        .strong(),
                );
                ui.separator();

                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.spacing_mut().item_spacing = Vec2::new(8.0, 8.0);

                    match app.options_tab {
                        // 10.1 General — عام
                        OptionsTab::General => {
                            ui.horizontal(|ui| {
                                ui.label("(Interface Language): ﺔﻬﺟﺍﻮﻟﺍ ﺔﻐﻟ");
                                if ui
                                    .selectable_value(
                                        &mut app.language,
                                        AppLanguage::Arabic,
                                        "(Arabic) ﺔﻴﺑﺮﻌﻟﺍ",
                                    )
                                    .changed()
                                    || ui
                                        .selectable_value(
                                            &mut app.language,
                                            AppLanguage::English,
                                            "English",
                                        )
                                        .changed()
                                {
                                    app.ui_state.language = app.language;
                                    app.mark_ui_state_dirty();
                                }
                            });

                            ui.checkbox(
                                &mut app.app_config.minimize_to_tray_on_close,
                                "ﺓﺬﻓﺎﻨﻟﺍ ﻕﻼﻏﺇ ﺭﺯ ﻰﻠﻋ ﻂﻐﻀﻟﺍ ﺪﻨﻋ (Tray ﺕﺍﺭﺎﻌﺷﻹﺍ ﺔﻘﻄﻨﻣ) ﺔﻴﻔﻠﺨﻟﺍ ﻲﻓ ﻞﻤﻌﻳ ﺞﻣﺎﻧﺮﺒﻟﺍ ﺀﺎﻘﺑﺇ",
                            );
                            ui.checkbox(
                                &mut app.app_config.confirm_destructive_actions,
                                "(ﺕﻼﻳﺰﻨﺘﻟﺍ ﺀﺎﻐﻟﺇ ﻭﺃ ﺕﻼﺠﺴﻟﺍ ﻭﺃ ﺕﺎﻔﻠﻤﻟﺍ ﻑﺬﺣ) ﺔﺳﺎﺴﺤﻟﺍ ﺕﺎﻴﻠﻤﻌﻟﺍ ﻞﺒﻗ ﺪﻴﻛﺄﺗ ﺐﻠﻃ",
                            );
                            ui.checkbox(
                                &mut app.app_config.start_with_system,
                                "(Windows Startup) ﻡﺎﻈﻨﻟﺍ ﻞﻴﻐﺸﺗ ﺀﺪﺑ ﺪﻨﻋ ﺎﻴﺋﺎﻘﻠﺗ ﺞﻣﺎﻧﺮﺒﻟﺍ ﻞﻴﻐﺸﺗ",
                            );
                        }

                        // 10.2 Appearance — المظهر
                        OptionsTab::Appearance => {
                            ui.horizontal(|ui| {
                                ui.label("(Theme Mode): ﺮﻬﻈﻤﻟﺍ ﻊﺿﻭ");
                                if ui
                                    .selectable_value(
                                        &mut app.theme_mode,
                                        ThemeMode::Dark,
                                        "(Dark) ﻦﻛﺍﺪﻟﺍ ﻊﺿﻮﻟﺍ 🌙",
                                    )
                                    .changed()
                                    || ui
                                        .selectable_value(
                                            &mut app.theme_mode,
                                            ThemeMode::Light,
                                            "(Light) ﺢﺗﺎﻔﻟﺍ ﻊﺿﻮﻟﺍ ☀️",
                                        )
                                        .changed()
                                {
                                    app.ui_state.theme_mode = app.theme_mode;
                                    app.mark_ui_state_dirty();
                                }
                            });

                            ui.horizontal(|ui| {
                                ui.label("(Row Density): ﻝﻭﺪﺠﻟﺍ ﻑﻮﻔﺻ ﺔﻓﺎﺜﻛ");
                                for d in [
                                    RowDensity::Compact,
                                    RowDensity::Comfortable,
                                    RowDensity::Spacious,
                                ] {
                                    if ui
                                        .selectable_value(
                                            &mut app.ui_state.row_density,
                                            d,
                                            loc.density_name(d),
                                        )
                                        .changed()
                                    {
                                        app.mark_ui_state_dirty();
                                    }
                                }
                            });

                            ui.horizontal(|ui| {
                                ui.label("(Font Scale): ﻂﺨﻟﺍ ﻢﺠﺣ ﺱﺎﻴﻘﻣ");
                                if ui
                                    .add(
                                        egui::Slider::new(&mut app.ui_state.font_scale, 0.85..=1.30)
                                            .step_by(0.05),
                                    )
                                    .changed()
                                {
                                    app.mark_ui_state_dirty();
                                }
                            });

                            ui.separator();
                            ui.label(RichText::new(":ﺔﻴﺴﻴﺋﺮﻟﺍ ﺔﻬﺟﺍﻮﻟﺍ ﺮﺻﺎﻨﻋ ﺀﺎﻔﺧﺇ ﻭﺃ ﺭﺎﻬﻇﺇ").strong());
                            if ui
                                .checkbox(&mut app.show_categories_panel, "ﺔﻴﺒﻧﺎﺠﻟﺍ ﺕﺎﺌﻔﻟﺍﻭ ﻞﻘﻨﺘﻟﺍ ﺔﺣﻮﻟ ﺭﺎﻬﻇﺇ")
                                .changed()
                            {
                                app.ui_state.show_sidebar = app.show_categories_panel;
                                app.mark_ui_state_dirty();
                            }
                            if ui
                                .checkbox(&mut app.ui_state.show_toolbar, "ﻲﺴﻴﺋﺮﻟﺍ ﺕﺍﻭﺩﻷﺍ ﻂﻳﺮﺷ ﺭﺎﻬﻇﺇ")
                                .changed()
                            {
                                app.mark_ui_state_dirty();
                            }
                            if ui
                                .checkbox(&mut app.ui_state.show_status_bar, "ﻲﻠﻔﺴﻟﺍ ﺔﻟﺎﺤﻟﺍ ﻂﻳﺮﺷ ﺭﺎﻬﻇﺇ")
                                .changed()
                            {
                                app.mark_ui_state_dirty();
                            }
                            if ui
                                .checkbox(&mut app.show_bottom_inspector, "(Segment Map & Digests) ﺔﻴﻠﻔﺴﻟﺍ ﺺﺤﻔﻟﺍ ﺔﺣﻮﻟ ﺭﺎﻬﻇﺇ")
                                .changed()
                            {
                                app.ui_state.show_bottom_inspector = app.show_bottom_inspector;
                                app.mark_ui_state_dirty();
                            }
                        }

                        // 10.3 Downloads — التنزيلات
                        OptionsTab::Downloads => {
                            let has_active = app.has_any_active_downloads();
                            if has_active {
                                ui.colored_label(
                                    palette.color_warning,
                                    ".ﺔﻳﺭﺎﺠﻟﺍ ﻡﺎﻬﻤﻟﺍ ﺪﺴﻔﻳ ﻦﻟﻭ ﺓﺪﻳﺪﺠﻟﺍ ﻡﺎﻬﻤﻟﺍ ﻰﻠﻋ ﻖﺒﻄﻴﺳ ﻲﺿﺍﺮﺘﻓﻻﺍ ﺪﻠﺠﻤﻟﺍ ﺮﻴﻴﻐﺗ ؛ﺎﻴﻟﺎﺣ ﺔﻄﺸﻧ ﺕﻼﻳﺰﻨﺗ ﺪﺟﻮﺗ ℹ",
                                );
                            }
                            ui.label(":ﻲﺿﺍﺮﺘﻓﻻﺍ ﻞﻳﺰﻨﺘﻟﺍ ﺪﻠﺠﻣ");
                            ui.horizontal(|ui| {
                                let mut dir_str = app.app_config.download_dir.display().to_string();
                                if ui
                                    .add(
                                        egui::TextEdit::singleline(&mut dir_str)
                                            .desired_width(420.0),
                                    )
                                    .changed()
                                {
                                    app.app_config.download_dir = PathBuf::from(dir_str);
                                }
                                if ui.button("ﺢﺘﻓ 📁").clicked() {
                                    open_folder(&app.app_config.download_dir);
                                }
                            });

                            ui.horizontal(|ui| {
                                ui.label(":ﺎﻘﺒﺴﻣ ﺓﺩﻮﺟﻮﻤﻟﺍ ﺕﺎﻔﻠﻤﻟﺍ ﻊﻣ ﻞﻣﺎﻌﺘﻟﺍ");
                                egui::ComboBox::from_id_salt("dup_file_policy_combo")
                                    .selected_text(match app.app_config.duplicate_file_policy {
                                        DuplicateFilePolicy::AutoRename => "(ﻦﻣﺁ) ﻲﻓﺎﺿﺇ ﻢﻗﺮﺑ ﺔﻴﺋﺎﻘﻠﺗ ﺔﻴﻤﺴﺗ",
                                        DuplicateFilePolicy::Overwrite => "ﺩﻮﺟﻮﻤﻟﺍ ﻒﻠﻤﻟﺍ ﻝﺍﺪﺒﺘﺳﺍ",
                                        DuplicateFilePolicy::Prompt => "ﻡﺪﺨﺘﺴﻤﻟﺍ ﻝﺍﺆﺳ",
                                    })
                                    .show_ui(ui, |ui| {
                                        ui.selectable_value(
                                            &mut app.app_config.duplicate_file_policy,
                                            DuplicateFilePolicy::AutoRename,
                                            "(ﻦﻣﺁ) ﻲﻓﺎﺿﺇ ﻢﻗﺮﺑ ﺔﻴﺋﺎﻘﻠﺗ ﺔﻴﻤﺴﺗ",
                                        );
                                        ui.selectable_value(
                                            &mut app.app_config.duplicate_file_policy,
                                            DuplicateFilePolicy::Overwrite,
                                            "ﺩﻮﺟﻮﻤﻟﺍ ﻒﻠﻤﻟﺍ ﻝﺍﺪﺒﺘﺳﺍ",
                                        );
                                        ui.selectable_value(
                                            &mut app.app_config.duplicate_file_policy,
                                            DuplicateFilePolicy::Prompt,
                                            "ﻡﺪﺨﺘﺴﻤﻟﺍ ﻝﺍﺆﺳ",
                                        );
                                    });
                            });

                            ui.checkbox(
                                &mut app.app_config.auto_start_queued_downloads,
                                "ﺎﻴﺋﺎﻘﻠﺗ ﺢﻔﺼﺘﻤﻟﺍ ﻦﻣ ﺔﻄﻘﺘﻠﻤﻟﺍ ﺕﻼﻳﺰﻨﺘﻟﺍ ﺀﺪﺑ",
                            );
                            ui.checkbox(
                                &mut app.app_config.enable_checksum_verification,
                                "ﺎﻬﻟﺎﻤﺘﻛﺍ ﺭﻮﻓ (SHA-256 / MD5 / CRC32) ﺕﺎﻔﻠﻤﻟﺍ ﺔﻣﻼﺳ ﻦﻣ ﻲﺋﺎﻘﻠﺘﻟﺍ ﻖﻘﺤﺘﻟﺍ",
                            );
                        }

                        // 10.4 Connections — الاتصالات
                        OptionsTab::Connections => {
                            ui.horizontal(|ui| {
                                ui.label("(1..32): ﻞﻳﺰﻨﺗ ﻞﻜﻟ ﺔﻳﺯﺍﻮﺘﻤﻟﺍ ﺕﻻﺎﺼﺗﻻﺍ ﺩﺪﻋ");
                                ui.add(egui::Slider::new(
                                    &mut app.app_config.max_connections_per_file,
                                    1..=32,
                                ));
                            });

                            ui.horizontal(|ui| {
                                ui.label("(KB): ﺔﻴﻔﺼﻨﻟﺍ ﺔﻤﺴﻘﻟﺍ ﻢﺠﺤﻟ ﻰﻧﺩﻷﺍ ﺪﺤﻟﺍ");
                                ui.add(
                                    egui::DragValue::new(&mut app.app_config.min_split_size_kb)
                                        .range(64..=16384),
                                );
                            });

                            ui.horizontal(|ui| {
                                ui.label("(:ﺩﻭﺪﺤﻣ ﺮﻴﻏ = 0 ،s/KB) ﺔﻴﻠﻜﻟﺍ ﺔﻋﺮﺴﻠﻟ ﻰﺼﻗﻷﺍ ﺪﺤﻟﺍ");
                                let mut limit = app.app_config.global_speed_limit_kbps as f32;
                                if ui
                                    .add(egui::Slider::new(&mut limit, 0.0..=51_200.0))
                                    .changed()
                                {
                                    app.app_config.global_speed_limit_kbps = limit.round() as u64;
                                    app.speed_limit_kbps = app.app_config.global_speed_limit_kbps;
                                    app.speed_limiter_enabled = app.speed_limit_kbps > 0;
                                }
                            });

                            ui.horizontal(|ui| {
                                ui.label("(:ﺔﻴﻧﺎﺛ) ﻡﺩﺎﺨﻟﺎﺑ ﻝﺎﺼﺗﻻﺍ ﺔﻠﻬﻣ");
                                ui.add(
                                    egui::DragValue::new(
                                        &mut app.app_config.connection_timeout_secs,
                                    )
                                    .range(5..=120),
                                );
                                ui.separator();
                                ui.label(":ﻝﺎﺼﺗﻻﺍ ﺓﺩﺎﻋﺇ ﺕﻻﻭﺎﺤﻣ ﺩﺪﻋ");
                                ui.add(
                                    egui::DragValue::new(&mut app.app_config.max_retry_attempts)
                                        .range(1..=20),
                                );
                            });

                            ui.separator();
                            ui.label("(:ﻱﺭﺎﻴﺘﺧﺍ — HTTP/SOCKS Proxy URL) ﻞﻴﻛﻮﻟﺍ ﻡﺩﺎﺧ");
                            let mut proxy = app.app_config.proxy_url.clone().unwrap_or_default();
                            if ui
                                .add(
                                    egui::TextEdit::singleline(&mut proxy)
                                        .hint_text("http://127.0.0.1:8080")
                                        .desired_width(420.0),
                                )
                                .changed()
                            {
                                app.app_config.proxy_url = opt_non_empty(&proxy);
                            }

                            ui.label("(User-Agent): ﺺﺼﺨﻤﻟﺍ ﺢﻔﺼﺘﻤﻟﺍ ﻑﺮﻌﻣ");
                            ui.add(
                                egui::TextEdit::singleline(&mut app.app_config.custom_user_agent)
                                    .desired_width(420.0),
                            );
                        }

                        // 10.5 Browser Integration — تكامل المتصفح (حالة حقيقية فقط)
                        OptionsTab::BrowserIntegration => {
                            let engine_online = app.error_message.is_none();
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("(Local IPC 127.0.0.1:16888): ﺔﻴﻠﻌﻔﻟﺍ ﻝﺎﺼﺗﻻﺍ ﺓﺎﻨﻗ ﺔﻟﺎﺣ").strong());
                                if engine_online {
                                    ui.colored_label(
                                        palette.color_success,
                                        "(Hardened Local IPC + Native Messaging Ready) ﻞﺼﺘﻣﻭ ﻂﺸﻧ ●",
                                    );
                                } else {
                                    ui.colored_label(
                                        palette.color_error,
                                        "ﻲﻠﺤﻤﻟﺍ ﻙﺮﺤﻤﻟﺎﺑ ﻞﺼﺘﻣ ﺮﻴﻏ ●",
                                    );
                                }
                            });

                            if let Some(err) = &app.error_message {
                                ui.colored_label(palette.color_error, format!("{err} :ﻝﺎﺼﺗﻻﺍ ﺄﻄﺧ"));
                            }

                            ui.add_space(4.0);
                            ui.label(":ﺢﻔﺼﺘﻤﻟﺍ ﻦﻣ ﺎﻴﺋﺎﻘﻠﺗ ﺔﻄﻘﺘﻠﻤﻟﺍ ﺕﺍﺩﺍﺪﺘﻣﻻﺍ");
                            let mut ext = app.app_config.intercept_extensions.join(", ");
                            if ui
                                .add(
                                    egui::TextEdit::multiline(&mut ext)
                                        .desired_rows(3)
                                        .desired_width(480.0),
                                )
                                .changed()
                            {
                                app.app_config.intercept_extensions = ext
                                    .split(',')
                                    .map(|s| s.trim().trim_start_matches('.').to_lowercase())
                                    .filter(|s| !s.is_empty())
                                    .collect();
                            }

                            ui.separator();
                            ui.label(RichText::new(":ﺀﺎﻄﺧﻷﺍ ﻑﺎﺸﻜﺘﺳﺍﻭ ﻞﻴﻌﻔﺘﻟﺍ ﺔﻘﻳﺮﻃ").strong());
                            ui.label("Load Unpacked). ﺭﻮﻄﻤﻟﺍ ﻊﺿﻭ) chrome-extension ﺪﻠﺠﻣ ﻦﻣ ﺢﻔﺼﺘﻤﻟﺍ ﺔﻓﺎﺿﺇ ﺖﺒﺛ 1.");
                            ui.label("native-messaging/register_host_windows.ps1. :ﺖﺑﺮﻜﺴﻟﺍ ﻞﻐﺷ ،ﺯﻭﺪﻨﻳﻭ ﻰﻠﻋ Native Messaging ﻞﻴﻌﻔﺘﻟ 2.");
                            ui.label(".ﻲﺋﺎﻘﻠﺘﻟﺍ ﺽﺍﺮﺘﻋﻻﺍ ﺯﻭﺎﺠﺘﻟ ﻂﺑﺍﺭ ﻱﺃ ﻰﻠﻋ ﺮﻘﻨﻟﺍ ﺀﺎﻨﺛﺃ Alt ﺡﺎﺘﻔﻣ ﻰﻠﻋ ﻂﻐﻀﻟﺍ ﺎﻤﺋﺍﺩ ﻚﻨﻜﻤﻳ 3.");
                        }

                        // 10.6 Media — الوسائط (HLS / DASH / yt-dlp / FFmpeg)
                        OptionsTab::Media => {
                            ui.label(
                                RichText::new("(HLS AES-128 / MPEG-DASH / YouTube): ﺚﺒﻟﺍﻭ ﻂﺋﺎﺳﻮﻟﺍ ﺔﺠﻟﺎﻌﻣ ﺕﺍﻭﺩﺃ ﺔﻟﺎﺣ").strong(),
                            );
                            if let Some(ts) = &app.tools_status {
                                egui::Grid::new("media_tools_status_grid")
                                    .num_columns(3)
                                    .spacing([16.0, 6.0])
                                    .show(ui, |ui| {
                                        ui.label(RichText::new("yt-dlp: ﺓﺍﺩﺃ").strong());
                                        ui.colored_label(
                                            if ts.ytdlp_installed {
                                                palette.color_success
                                            } else {
                                                palette.color_warning
                                            },
                                            if ts.ytdlp_installed {
                                                "✓ ﺓﺰﻫﺎﺟﻭ ﺔﻔﺸﺘﻜﻣ"
                                            } else {
                                                "ﺔﺘﺒﺜﻣ ﺮﻴﻏ"
                                            },
                                        );
                                        ui.label(
                                            RichText::new(
                                                ts.ytdlp_path.as_deref().unwrap_or("—"),
                                            )
                                            .monospace()
                                            .size(11.0),
                                        );
                                        ui.end_row();

                                        ui.label(RichText::new("FFmpeg: ﺓﺍﺩﺃ").strong());
                                        ui.colored_label(
                                            if ts.ffmpeg_installed {
                                                palette.color_success
                                            } else {
                                                palette.color_warning
                                            },
                                            if ts.ffmpeg_installed {
                                                "✓ ﺓﺰﻫﺎﺟﻭ ﺔﻔﺸﺘﻜﻣ"
                                            } else {
                                                "ﺔﺘﺒﺜﻣ ﺮﻴﻏ"
                                            },
                                        );
                                        ui.label(
                                            RichText::new(
                                                ts.ffmpeg_path.as_deref().unwrap_or("—"),
                                            )
                                            .monospace()
                                            .size(11.0),
                                        );
                                        ui.end_row();

                                        ui.label(RichText::new(":ﺕﺍﻭﺩﻷﺍ ﺪﻠﺠﻣ").strong());
                                        ui.label(RichText::new(&ts.tools_directory).monospace());
                                        if ui.small_button("ﺕﺍﻭﺩﻷﺍ ﺪﻠﺠﻣ ﺢﺘﻓ 📁").clicked() {
                                            open_folder(Path::new(&ts.tools_directory));
                                        }
                                        ui.end_row();
                                    });
                            } else {
                                ui.label("...ﻙﺮﺤﻤﻟﺍ ﻦﻣ ﺕﺍﻭﺩﻷﺍ ﺔﻟﺎﺣ ﺓﺀﺍﺮﻗ ﺭﺎﺟ");
                            }

                            ui.add_space(6.0);
                            if ui
                                .button("ﻥﻵﺍ FFmpeg ﻭ yt-dlp ﺚﻳﺪﺤﺗ / ﺖﻴﺒﺜﺗﻭ ﺺﺤﻓ 🔄")
                                .clicked()
                            {
                                let _ = app.commands.send(UiCommand::UpdateTools);
                                app.push_toast(
                                    ToastKind::Info,
                                    "ﻂﺋﺎﺳﻮﻟﺍ ﺕﺍﻭﺩﺃ ﺚﻳﺪﺤﺗ",
                                    "...ﺔﻴﻔﻠﺨﻟﺍ ﻲﻓ FFmpeg ﻭ yt-dlp ﻲﺗﺍﺩﺃ ﺚﻳﺪﺤﺗﻭ ﺺﺤﻓ ﺭﺎﺟ",
                                    None,
                                );
                            }
                        }

                        // 10.7 Notifications — الإشعارات
                        OptionsTab::Notifications => {
                            ui.checkbox(
                                &mut app.app_config.notify_on_complete,
                                "ﻞﻳﺰﻨﺘﻟﺍ ﻝﺎﻤﺘﻛﺍ ﺪﻨﻋ ﺭﺎﻌﺷﺇ ﺭﺎﻬﻇﺇ",
                            );
                            ui.checkbox(
                                &mut app.app_config.notify_on_failure,
                                "ﻂﺑﺍﺮﻟﺍ ﺔﻴﺣﻼﺻ ﺀﺎﻬﺘﻧﺍ ﻭﺃ ﻞﻳﺰﻨﺘﻟﺍ ﻞﺸﻓ ﺪﻨﻋ ﻲﻬﻴﺒﻨﺗ ﺭﺎﻌﺷﺇ ﺭﺎﻬﻇﺇ",
                            );
                            ui.checkbox(
                                &mut app.app_config.notify_on_schedule_start,
                                "ﺔﻟﻭﺪﺠﻣ ﺔﻤﻬﻣ ﻝﺎﻤﺘﻛﺍ ﻭﺃ ﺀﺪﺑ ﺪﻨﻋ ﺭﺎﻌﺷﺇ ﺭﺎﻬﻇﺇ",
                            );
                            ui.checkbox(
                                &mut app.app_config.sound_notifications_enabled,
                                "ﻞﻳﺰﻨﺘﻟﺍ ﻝﺎﻤﺘﻛﺍ ﺪﻨﻋ ﻲﺗﻮﺼﻟﺍ ﻪﻴﺒﻨﺘﻟﺍ ﻞﻴﻌﻔﺗ",
                            );

                            ui.add_space(6.0);
                            if ui.button("ﻥﻵﺍ ﻲﺒﻳﺮﺠﺗ ﺭﺎﻌﺷﺇ ﺭﺎﺒﺘﺧﺍ 🔔").clicked() {
                                app.push_toast(
                                    ToastKind::Success,
                                    "ﺕﺍﺭﺎﻌﺷﻹﺍ ﻡﺎﻈﻧ ﺭﺎﺒﺘﺧﺍ",
                                    ".ﻚﻠﻤﻋ ﺔﻗﺎﻋﺇ ﻥﻭﺩ ﺡﺎﺠﻨﺑ StormGet ﻲﻓ ﺪﺣﻮﻤﻟﺍ ﺕﺍﺭﺎﻌﺷﻹﺍ ﻡﺎﻈﻧ ﻞﻤﻌﻳ",
                                    None,
                                );
                            }
                        }

                        // 10.8 Advanced — متقدم
                        OptionsTab::Advanced => {
                            ui.horizontal(|ui| {
                                ui.label("(Logging Level): ﻲﺼﻴﺨﺸﺘﻟﺍ ﻞﻴﺠﺴﺘﻟﺍ ﻯﻮﺘﺴﻣ");
                                egui::ComboBox::from_id_salt("log_level_combo")
                                    .selected_text(&app.app_config.logging_level)
                                    .show_ui(ui, |ui| {
                                        for lvl in ["ERROR", "WARN", "INFO", "DEBUG"] {
                                            ui.selectable_value(
                                                &mut app.app_config.logging_level,
                                                lvl.to_owned(),
                                                lvl,
                                            );
                                        }
                                    });
                            });

                            ui.horizontal(|ui| {
                                ui.label(":ﺕﻼﺠﺴﻟﺍﻭ ﺕﺍﺩﺍﺪﻋﻹﺍ ﺕﺎﻔﻠﻣ ﺭﺎﺴﻣ");
                                ui.label(
                                    RichText::new(
                                        app.app_config.download_dir.display().to_string(),
                                    )
                                    .monospace(),
                                );
                            });

                            ui.horizontal(|ui| {
                                if ui
                                    .button("ﺙﺍﺪﺣﻷﺍ ﻞﺠﺳﻭ ﺺﻴﺨﺸﺘﻟﺍ ﺓﺬﻓﺎﻧ ﺢﺘﻓ 🩺")
                                    .clicked()
                                {
                                    app.open_or_focus_diagnostics();
                                }
                                if ui
                                    .button("ﺔﺳﺎﺴﺤﻟﺍ ﺕﺎﻧﺎﻴﺒﻟﺍ ﻦﻣ ﺢﻘﻨﻣ ﻲﺼﻴﺨﺸﺗ ﺮﻳﺮﻘﺗ ﺮﻳﺪﺼﺗ 📤")
                                    .clicked()
                                {
                                    let report = app.build_redacted_diagnostic_report();
                                    ui.ctx().copy_text(report);
                                    app.push_toast(
                                        ToastKind::Success,
                                        "ﻲﺼﻴﺨﺸﺘﻟﺍ ﺮﻳﺮﻘﺘﻟﺍ ﺦﺴﻧ ﻢﺗ",
                                        ".ﺔﻈﻓﺎﺤﻟﺍ ﻰﻟﺇ (ﺭﺍﺮﺳﺃ ﻭﺃ ﺰﻴﻛﻮﻛ ﻥﻭﺪﺑ) ﺢﻘﻨﻤﻟﺍ ﺺﻴﺨﺸﺘﻟﺍ ﺮﻳﺮﻘﺗ ﺦﺴﻧ ﻢﺗ",
                                        None,
                                    );
                                }
                            });

                            ui.separator();
                            ui.label(
                                RichText::new("(:ﺕﺎﻧﺎﻴﺒﻟﺍ ﻥﺍﺪﻘﻓ ﻊﻨﻤﻟ ﻖﺒﺴﻣ ﺪﻴﻛﺄﺗ ﻊﻣ) ﻂﺒﻀﻟﺍ ﺓﺩﺎﻋﺇ ﺕﺍﺀﺍﺮﺟﺇ")
                                    .strong()
                                    .color(palette.color_warning),
                            );
                            ui.label(
                                RichText::new(
                                    ".ﺕﻼﻳﺰﻨﺘﻟﺎﺑ ﺱﺎﺴﻤﻟﺍ ﻥﻭﺩ ﻲﺿﺍﺮﺘﻓﻻﺍ ﻊﺿﻮﻟﺍ ﻰﻟﺇ ﺮﻬﻈﻤﻟﺍﻭ ﺓﺮﻫﺎﻈﻟﺍ ﺓﺪﻤﻋﻷﺍﻭ ﺬﻓﺍﻮﻨﻟﺍ ﻡﺎﺠﺣﺃ ﺪﻴﻌﺗ :ﺔﻬﺟﺍﻮﻟﺍ ﻂﻴﻄﺨﺗ ﻂﺒﺿ ﺓﺩﺎﻋﺇ •",
                                )
                                .size(11.0)
                                .color(palette.text_muted),
                            );
                            if ui
                                .button("ﺔﻬﺟﺍﻮﻟﺍ ﻂﻴﻄﺨﺗﻭ ﺕﺍﺩﺍﺪﻋﺇ ﻂﺒﺿ ﺓﺩﺎﻋﺇ ↺")
                                .clicked()
                            {
                                app.pending_confirmation =
                                    Some(PendingConfirmation::ResetUiLayoutToDefaults);
                            }

                            ui.label(
                                RichText::new(
                                    "(.ﺔﻟﺰﻨﻤﻟﺍ ﺕﺎﻔﻠﻤﻟﺍ ﻑﺬﺤﺗ ﻻ) ﺔﻴﻌﻨﺼﻤﻟﺍ ﻢﻴﻘﻟﺍ ﻰﻟﺇ ﻲﺿﺍﺮﺘﻓﻻﺍ ﺭﺎﺴﻤﻟﺍﻭ ﺔﻋﺮﺴﻟﺍ ﺩﺪﺤﻣﻭ ﺕﻻﺎﺼﺗﻻﺍ ﺩﺪﻋ ﺪﻴﻌﺗ :ﻙﺮﺤﻤﻟﺍﻭ ﺞﻣﺎﻧﺮﺒﻟﺍ ﺕﺍﺩﺍﺪﻋﺇ ﻂﺒﺿ ﺓﺩﺎﻋﺇ •",
                                )
                                .size(11.0)
                                .color(palette.text_muted),
                            );
                            if ui
                                .button("...ﻙﺮﺤﻤﻟﺍﻭ ﺞﻣﺎﻧﺮﺒﻟﺍ ﺕﺍﺩﺍﺪﻋﺇ ﻂﺒﺿ ﺓﺩﺎﻋﺇ ⚠")
                                .clicked()
                            {
                                app.pending_confirmation =
                                    Some(PendingConfirmation::ResetAppConfigToDefaults);
                            }
                        }
                    }
                });
            });
    });
}

// ============================================================================
// نافذة التشخيص وسجل الأخطاء المستقلة (Diagnostics & Error Log Viewport)
// ============================================================================

fn draw_independent_diagnostics_viewport(
    app: &mut RustIdmApp,
    ctx: &egui::Context,
    loc: &Loc,
    palette: &ThemePalette,
) {
    if !app.show_diagnostics_window {
        return;
    }

    let (viewport_id, builder, should_focus) = build_and_track_secondary_viewport(
        app,
        "diagnostics",
        loc.diagnostics_window_title(),
        720.0,
        500.0,
        560.0,
        360.0,
        true,
    );

    ctx.show_viewport_immediate(viewport_id, builder, |vp_ctx, _| {
        if should_focus {
            vp_ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }
        if vp_ctx.input(|i| i.viewport().close_requested()) {
            app.show_diagnostics_window = false;
            return;
        }
        record_secondary_viewport_geometry(app, "diagnostics", vp_ctx);
        apply_stormget_theme_with_scale(vp_ctx, app.theme_mode, app.ui_state.font_scale);

        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(palette.window_bg)
                    .inner_margin(egui::Margin::same(14)),
            )
            .show(vp_ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.heading(
                        RichText::new(loc.diagnostics_window_title())
                            .color(palette.text_main)
                            .strong(),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.button("ﻕﻼﻏﺇ").clicked() {
                            app.show_diagnostics_window = false;
                        }
                        if ui.button("ﺢﻘﻨﻤﻟﺍ ﺮﻳﺮﻘﺘﻟﺍ ﺦﺴﻧ 📋").clicked() {
                            let rep = app.build_redacted_diagnostic_report();
                            ui.ctx().copy_text(rep);
                            app.push_toast(
                                ToastKind::Info,
                                "ﺦﺴﻨﻟﺍ ﻢﺗ",
                                ".ﺔﻈﻓﺎﺤﻟﺍ ﻰﻟﺇ ﺢﻘﻨﻤﻟﺍ ﻲﺼﻴﺨﺸﺘﻟﺍ ﺮﻳﺮﻘﺘﻟﺍ ﺦﺴﻧ ﻢﺗ",
                                None,
                            );
                        }
                    });
                });
                ui.separator();

                let report_text = app.build_redacted_diagnostic_report();
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let mut ro = report_text;
                        ui.add(
                            egui::TextEdit::multiline(&mut ro)
                                .code_editor()
                                .desired_rows(22)
                                .desired_width(ui.available_width()),
                        );
                    });
            });
    });
}

// ============================================================================
// نافذة التأكيد قبل العمليات المدمرة (Confirmation Dialog — المرحلة 13)
// ============================================================================

fn draw_confirmation_modal(
    app: &mut RustIdmApp,
    ctx: &egui::Context,
    _loc: &Loc,
    palette: &ThemePalette,
) {
    let Some(confirm) = app.pending_confirmation.clone() else {
        return;
    };

    let mut execute_confirmed = false;
    let mut cancel_modal = false;

    egui::Window::new("StormGet — ﺀﺍﺮﺟﻹﺍ ﺪﻴﻛﺄﺗ")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
        .default_width(480.0)
        .show(ctx, |ui| {
            ui.spacing_mut().item_spacing = Vec2::new(8.0, 8.0);
            match &confirm {
                PendingConfirmation::RemoveRecordsOnly {
                    task_ids,
                    filenames,
                } => {
                    ui.colored_label(
                        palette.color_info,
                        RichText::new(format!(
                            "(ﺔﻤﻬﻣ ){} ﻂﻘﻓ ﻞﻳﺰﻨﺘﻟﺍ ﻞﺠﺳ ﻑﺬﺣ",
                            task_ids.len()
                        ))
                        .strong()
                        .size(14.0),
                    );
                    ui.label(".ﺐﻠﺼﻟﺍ ﺹﺮﻘﻟﺍ ﻦﻣ ﻞﻤﺘﻜﻤﻟﺍ ﻒﻠﻤﻟﺍ ﻑﺬﺣ ﻢﺘﻳ ﻦﻟﻭ ،ﻂﻘﻓ ﺞﻣﺎﻧﺮﺒﻟﺍ ﺔﻤﺋﺎﻗ ﻦﻣ ﺔﻤﻬﻤﻟﺍ ﺔﻟﺍﺯﺇ ﻢﺘﻴﺳ");
                    if let Some(first) = filenames.first() {
                        ui.label(RichText::new(format!("{first} :ﻒﻠﻤﻟﺍ")).monospace());
                    }
                }
                PendingConfirmation::DeleteFilesFromDisk {
                    task_ids,
                    filenames,
                } => {
                    ui.colored_label(
                        palette.color_error,
                        RichText::new(format!(
                            "(ﻒﻠﻣ ){} ﺹﺮﻘﻟﺍ ﻦﻣ ﺎﻴﺋﺎﻬﻧ ﻒﻠﻤﻟﺍﻭ ﻞﺠﺴﻟﺍ ﻑﺬﺣ :ﺮﻳﺬﺤﺗ ⚠",
                            task_ids.len()
                        ))
                        .strong()
                        .size(14.0),
                    );
                    ui.label(".ﻪﻨﻋ ﻊﺟﺍﺮﺘﻟﺍ ﻦﻜﻤﻳ ﻻﻭ ﺐﻠﺼﻟﺍ ﺹﺮﻘﻟﺍ ﻦﻣ ﺎﻴﺋﺎﻬﻧ ﻲﻠﻌﻔﻟﺍ ﻒﻠﻤﻟﺍ ﻑﺬﺣﻭ ﻞﻳﺰﻨﺘﻟﺍ ﻞﺠﺳ ﻑﺬﺣ ﻰﻟﺇ ﺀﺍﺮﺟﻹﺍ ﺍﺬﻫ ﻱﺩﺆﻴﺳ");
                    for name in filenames.iter().take(3) {
                        ui.label(RichText::new(format!("• {name}")).monospace());
                    }
                }
                PendingConfirmation::CancelActiveDownloads {
                    task_ids,
                    filenames,
                } => {
                    ui.colored_label(
                        palette.color_warning,
                        RichText::new(format!("(ﺔﻤﻬﻣ ){} ﻂﺸﻨﻟﺍ ﻞﻳﺰﻨﺘﻟﺍ ﺀﺎﻐﻟﺇ", task_ids.len()))
                            .strong()
                            .size(14.0),
                    );
                    ui.label("؟ﻱﺭﺎﺠﻟﺍ ﻞﻳﺰﻨﺘﻟﺍ ﺀﺎﻐﻟﺇﻭ ﻑﺎﻘﻳﺇ ﺪﻴﻛﺄﺘﻟﺎﺑ ﺪﻳﺮﺗ ﻞﻫ");
                    if let Some(first) = filenames.first() {
                        ui.label(RichText::new(format!("{first} :ﺔﻤﻬﻤﻟﺍ")).monospace());
                    }
                }
                PendingConfirmation::ClearCompletedRecords { count } => {
                    ui.colored_label(
                        palette.color_info,
                        RichText::new(format!("(ﻞﺠﺳ {count}) ﺔﻠﻤﺘﻜﻤﻟﺍ ﻡﺎﻬﻤﻟﺍ ﺕﻼﺠﺳ ﺢﺴﻣ"))
                            .strong()
                            .size(14.0),
                    );
                    ui.label(".ﺹﺮﻘﻟﺍ ﻰﻠﻋ ﺎﻬﺗﺍﺪﻠﺠﻣ ﻲﻓ ﺔﻠﻤﺤﻤﻟﺍ ﺕﺎﻔﻠﻤﻟﺍ ﻊﻴﻤﺟ ﻰﻠﻋ ﻡﺎﺘﻟﺍ ﻅﺎﻔﺤﻟﺍ ﻊﻣ ﻂﻘﻓ ﻝﻭﺪﺠﻟﺍ ﻦﻣ ﺔﻠﻤﺘﻜﻤﻟﺍ ﻡﺎﻬﻤﻟﺍ ﺕﻼﺠﺳ ﺢﺴﻣ ﻢﺘﻴﺳ");
                }
                PendingConfirmation::ResetUiLayoutToDefaults => {
                    ui.colored_label(
                        palette.color_warning,
                        RichText::new("ﺔﻬﺟﺍﻮﻟﺍ ﻂﻴﻄﺨﺗ ﻂﺒﺿ ﺓﺩﺎﻋﺇ").strong().size(14.0),
                    );
                    ui.label(".ﻝﻭﺪﺠﻟﺍ ﺔﻓﺎﺜﻛﻭ ﺓﺮﻫﺎﻈﻟﺍ ﺓﺪﻤﻋﻷﺍﻭ ﺬﻓﺍﻮﻨﻠﻟ ﺔﻴﺿﺍﺮﺘﻓﻻﺍ ﺩﺎﻌﺑﻷﺍ ﺓﺩﺎﻌﺘﺳﺍ ﻢﺘﻴﺳ");
                }
                PendingConfirmation::ResetAppConfigToDefaults => {
                    ui.colored_label(
                        palette.color_warning,
                        RichText::new("ﻙﺮﺤﻤﻟﺍ ﺕﺍﺩﺍﺪﻋﺇ ﻂﺒﺿ ﺓﺩﺎﻋﺇ").strong().size(14.0),
                    );
                    ui.label(".ﺔﻴﺿﺍﺮﺘﻓﻻﺍ ﺔﻴﻌﻨﺼﻤﻟﺍ ﻢﻴﻘﻟﺍ ﻰﻟﺇ ﺽﺍﺮﺘﻋﻻﺍ ﺕﺍﺭﺎﻴﺧﻭ ﺔﻋﺮﺴﻟﺍ ﺩﺪﺤﻣﻭ ﺕﻻﺎﺼﺗﻻﺍ ﺩﺪﻋ ﺓﺩﺎﻋﺇ ﻢﺘﻴﺳ");
                }
                PendingConfirmation::CloseWhileDownloadsActive { active_count } => {
                    ui.colored_label(
                        palette.color_warning,
                        RichText::new(format!(
                            "!ﻞﻤﻌﻟﺍ ﺪﻴﻗ ﺔﻄﺸﻧ ﺕﻼﻳﺰﻨﺗ {active_count} ﺎﻴﻟﺎﺣ ﺪﺟﻮﺗ"
                        ))
                        .strong()
                        .size(14.0),
                    );
                    ui.label(".ﻥﺎﻣﺄﺑ ﺕﻼﻳﺰﻨﺘﻟﺍ ﻝﺎﻤﻜﺘﺳﻻ (ﺕﺍﺭﺎﻌﺷﻹﺍ/ﻡﺎﻬﻤﻟﺍ ﻂﻳﺮﺷ) ﺔﻴﻔﻠﺨﻟﺍ ﻲﻓ ﻞﻤﻌﻳ ﺞﻣﺎﻧﺮﺒﻟﺍ ﺀﺎﻘﺑﺇ ﻚﻨﻜﻤﻳ .ﺔﻳﺭﺎﺠﻟﺍ ﺕﻼﻳﺰﻨﺘﻟﺍ ﻒﻗﻮﻴﺳ ﻞﻣﺎﻜﻟﺎﺑ ﺞﻣﺎﻧﺮﺒﻟﺍ ﻕﻼﻏﺇ");
                }
            }

            ui.separator();
            ui.horizontal(|ui| {
                if let PendingConfirmation::CloseWhileDownloadsActive { .. } = &confirm {
                    if ui
                        .button(
                            RichText::new("ﺔﻴﻔﻠﺨﻟﺍ ﻲﻓ ﻞﻤﻌﻳ ﺞﻣﺎﻧﺮﺒﻟﺍ ﺀﺎﻘﺑﺇ 🗕")
                                .strong()
                                .color(Color32::WHITE),
                        )
                        .clicked()
                    {
                        app.pending_confirmation = None;
                        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                    }
                    if ui
                        .button(
                            RichText::new("ﻲﺋﺎﻬﻨﻟﺍ ﺝﻭﺮﺨﻟﺍﻭ ﺕﻼﻳﺰﻨﺘﻟﺍ ﻑﺎﻘﻳﺇ ⏹")
                                .color(palette.color_error),
                        )
                        .clicked()
                    {
                        app.stop_all_downloads();
                        app.allow_exit = true;
                        app.pending_confirmation = None;
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    if ui.button("ﺀﺎﻐﻟﺇ").clicked() {
                        cancel_modal = true;
                    }
                } else {
                    if ui
                        .button(
                            RichText::new("ﺬﻴﻔﻨﺘﻟﺍ ﺪﻴﻛﺄﺗ ✔")
                                .strong()
                                .color(Color32::WHITE),
                        )
                        .clicked()
                    {
                        execute_confirmed = true;
                    }
                    if ui.button("ﺮﻣﻷﺍ ﺀﺎﻐﻟﺇ").clicked() {
                        cancel_modal = true;
                    }
=======
                ui.spacing_mut().item_spacing = Vec2::new(24.0, 0.0);
                if ui.add_sized([120.0, 24.0], egui::Button::new(RichText::new(loc.start_download()).strong())).clicked() {
                    submitted_start = true;
                }
                if ui.add_sized([120.0, 24.0], egui::Button::new(loc.download_later())).clicked() {
                    submitted_later = true;
                }
                if ui.add_sized([100.0, 24.0], egui::Button::new(loc.cancel())).clicked() {
                    cancelled = true;
>>>>>>> efc40c730b5c3e9a4001ccff0b896d7f2f3ef344
                }
            });
        });

<<<<<<< HEAD
    if execute_confirmed {
        app.pending_confirmation_execute(confirm);
        app.pending_confirmation = None;
    } else if cancel_modal {
        app.pending_confirmation = None;
=======
    if submitted_start {
        let url = app.new_url.trim().to_owned();
        if !url.is_empty() {
            let filename = if app.new_filename.trim().is_empty() {
                None
            } else {
                Some(app.new_filename.trim().to_owned())
            };
            let payload = InterceptDownloadPayload {
                url,
                source_page_url: opt_non_empty(&app.new_referer),
                filename,
                cookies: opt_non_empty(&app.new_cookies),
                referer: opt_non_empty(&app.new_referer),
                user_agent: opt_non_empty(&app.new_user_agent),
                queue_name: Some(app.new_queue_selection.clone()),
                expected_sha256: opt_non_empty(&app.new_expected_sha256),
                expected_md5: opt_non_empty(&app.new_expected_md5),
                expected_crc32: opt_non_empty(&app.new_expected_crc32),
                mirror_urls: None,
                description: opt_non_empty(&app.new_description),
            };
            if let Some(id) = app.pending_intercept_id.clone() {
                let _ = app.commands.send(UiCommand::ResolvePendingIntercept {
                    id,
                    action: "start",
                    save_dir: opt_non_empty(&app.new_save_folder).map(PathBuf::from),
                });
            } else {
                let _ = app.commands.send(UiCommand::AddDownload {
                    payload,
                    save_dir: opt_non_empty(&app.new_save_folder).map(PathBuf::from),
                    start_immediately: true,
                });
            }
            app.status_message = "جاري بدء التحميل...".to_owned();
            app.show_details = true;
            app.show_connection_details = true;
            app.dialog_tab = DialogTab::DownloadStatus;
            app.new_url.clear();
            app.new_filename.clear();
            app.pending_intercept_id = None;
            app.show_add_dialog = false;
        }
    } else if submitted_later {
        let url = app.new_url.trim().to_owned();
        if !url.is_empty() {
            let filename = if app.new_filename.trim().is_empty() {
                None
            } else {
                Some(app.new_filename.trim().to_owned())
            };
            let payload = InterceptDownloadPayload {
                url,
                source_page_url: opt_non_empty(&app.new_referer),
                filename,
                cookies: opt_non_empty(&app.new_cookies),
                referer: opt_non_empty(&app.new_referer),
                user_agent: opt_non_empty(&app.new_user_agent),
                queue_name: Some(app.new_queue_selection.clone()),
                expected_sha256: opt_non_empty(&app.new_expected_sha256),
                expected_md5: opt_non_empty(&app.new_expected_md5),
                expected_crc32: opt_non_empty(&app.new_expected_crc32),
                mirror_urls: None,
                description: opt_non_empty(&app.new_description),
            };
            if let Some(id) = app.pending_intercept_id.clone() {
                let _ = app.commands.send(UiCommand::ResolvePendingIntercept {
                    id,
                    action: "later",
                    save_dir: opt_non_empty(&app.new_save_folder).map(PathBuf::from),
                });
            } else {
                let _ = app.commands.send(UiCommand::AddDownload {
                    payload,
                    save_dir: opt_non_empty(&app.new_save_folder).map(PathBuf::from),
                    start_immediately: false,
                });
            }
            app.status_message = "تمت إضافة التنزيل للمنتظرة".to_owned();
            app.new_url.clear();
            app.new_filename.clear();
            app.pending_intercept_id = None;
            app.show_add_dialog = false;
        }
    } else if cancelled {
        if let Some(id) = app.pending_intercept_id.take() {
            let _ = app.commands.send(UiCommand::ResolvePendingIntercept {
                id,
                action: "cancel",
                save_dir: None,
            });
        }
        app.show_add_dialog = false;
>>>>>>> efc40c730b5c3e9a4001ccff0b896d7f2f3ef344
    }
}

// ============================================================================
// شريط الإشعارات المنبثقة غير الحاجبة (Toast Notifications Overlay — المرحلة 13)
// ============================================================================

fn draw_toast_overlay(app: &mut RustIdmApp, ctx: &egui::Context, palette: &ThemePalette) {
    app.toasts
        .retain(|t| t.created_at.elapsed().as_secs() < t.duration_secs);
    if app.toasts.is_empty() {
        return;
    }

    let mut dismiss_id = None;
    let mut open_task_file_id = None;
    let mut open_task_folder_id = None;

    egui::Area::new(egui::Id::new("stormget_toast_overlay"))
        .anchor(egui::Align2::RIGHT_BOTTOM, Vec2::new(-16.0, -36.0))
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            ui.spacing_mut().item_spacing = Vec2::new(0.0, 6.0);
            for toast in &app.toasts {
                let border_col = match toast.kind {
                    ToastKind::Success => palette.color_success,
                    ToastKind::Warning => palette.color_warning,
                    ToastKind::Error => palette.color_error,
                    ToastKind::Info => palette.color_info,
                };
                egui::Frame::window(ui.style())
                    .fill(palette.bg_secondary)
                    .stroke(Stroke::new(1.5, border_col))
                    .inner_margin(10.0)
                    .show(ui, |ui| {
                        ui.set_max_width(340.0);
                        ui.horizontal(|ui| {
                            ui.colored_label(
                                border_col,
                                RichText::new(&toast.title).strong().size(12.5),
                            );
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if ui.small_button("×").clicked() {
                                    dismiss_id = Some(toast.id);
                                }
                            });
                        });
                        ui.label(RichText::new(&toast.message).size(11.5));
                        if let Some(tid) = &toast.task_id {
                            ui.horizontal(|ui| {
                                if ui.small_button("ﻒﻠﻤﻟﺍ ﺢﺘﻓ 📂").clicked() {
                                    open_task_file_id = Some(tid.clone());
                                    dismiss_id = Some(toast.id);
                                }
                                if ui.small_button("ﺪﻠﺠﻤﻟﺍ ﺢﺘﻓ 📁").clicked() {
                                    open_task_folder_id = Some(tid.clone());
                                    dismiss_id = Some(toast.id);
                                }
                            });
                        }
                    });
            }
        });

    if let Some(id) = dismiss_id {
        app.toasts.retain(|t| t.id != id);
    }
    if let Some(tid) = open_task_file_id {
        if let Some(task) = app.downloads.iter().find(|d| d.task_id == tid) {
            open_file(&task.final_file_path());
        }
    }
    if let Some(tid) = open_task_folder_id {
        if let Some(task) = app.downloads.iter().find(|d| d.task_id == tid) {
            open_folder(&task.final_file_path());
        }
    }
}

// ============================================================================
// النوافذ الحوارية والأدوات الأخرى (Rename/Move, Columns, Batch, Grabber, ZIP, Help)
// ============================================================================

fn draw_rename_move_dialog(app: &mut RustIdmApp, ctx: &egui::Context, _loc: &Loc) {
    if !app.show_rename_dialog {
        return;
    }
    let mut open = app.show_rename_dialog;
    let mut save_clicked = false;
    egui::Window::new("ﻆﻔﺤﻟﺍ ﺪﻠﺠﻣ ﻭﺃ ﻒﻠﻤﻟﺍ ﻢﺳﺍ ﺮﻴﻴﻐﺗ")
        .collapsible(false)
        .resizable(false)
        .default_width(480.0)
        .open(&mut open)
        .show(ctx, |ui| {
            ui.label(".ﻥﺎﻣﺄﺑ ﺎﻫﺭﺎﺴﻣ ﻭﺃ ﺎﻬﻤﺳﺍ ﺮﻴﻴﻐﺘﻟ ﺔﻠﻤﺘﻜﻣ ﻭﺃ ﺎﺘﻗﺆﻣ ﺔﻔﻗﻮﺘﻣ ﺔﻤﻬﻤﻟﺍ ﻥﻮﻜﺗ ﻥﺃ ﺐﺠﻳ :ﺔﻈﺣﻼﻣ");
            egui::Grid::new("rename_move_grid")
                .num_columns(2)
                .spacing([10.0, 6.0])
                .show(ui, |ui| {
                    ui.label(":ﺪﻳﺪﺠﻟﺍ ﻒﻠﻤﻟﺍ ﻢﺳﺍ");
                    ui.add(
                        egui::TextEdit::singleline(&mut app.rename_filename_input)
                            .desired_width(340.0),
                    );
                    ui.end_row();

                    ui.label(":ﺪﻳﺪﺠﻟﺍ ﻆﻔﺤﻟﺍ ﺪﻠﺠﻣ");
                    ui.add(
                        egui::TextEdit::singleline(&mut app.rename_save_dir_input)
                            .desired_width(340.0),
                    );
                    ui.end_row();
                });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(
                        !app.rename_filename_input.trim().is_empty(),
                        egui::Button::new("ﺕﺍﺮﻴﻴﻐﺘﻟﺍ ﻆﻔﺣ 💾"),
                    )
                    .clicked()
                {
                    save_clicked = true;
                }
                if ui.button("ﺀﺎﻐﻟﺇ").clicked() {
                    app.show_rename_dialog = false;
                }
            });
        });

    if save_clicked {
        let _ = app.commands.send(UiCommand::UpdateTaskMeta {
            task_id: app.rename_task_id.clone(),
            payload: UpdateTaskMetaPayload {
                new_filename: Some(app.rename_filename_input.trim().to_owned()),
                new_save_dir: opt_non_empty(&app.rename_save_dir_input).map(PathBuf::from),
                new_queue_name: None,
                new_description: None,
                task_speed_limit_kbps: None,
            },
        });
        app.show_rename_dialog = false;
    } else {
        app.show_rename_dialog = open;
    }
}

fn draw_column_chooser_dialog(app: &mut RustIdmApp, ctx: &egui::Context, loc: &Loc) {
    if !app.show_column_chooser_dialog {
        return;
    }
    let mut open = app.show_column_chooser_dialog;
    egui::Window::new("(ﺐﻴﺗﺮﺗ / ﺀﺎﻔﺧﺇ / ﺭﺎﻬﻇﺇ) ﺕﻼﻳﺰﻨﺘﻟﺍ ﻝﻭﺪﺟ ﺓﺪﻤﻋﺃ ﺓﺭﺍﺩﺇ")
        .collapsible(false)
        .resizable(true)
        .default_width(440.0)
        .open(&mut open)
        .show(ctx, |ui| {
            ui.label("(:ﺎﻴﺋﺎﻘﻠﺗ ﻆﻔﺤﺗ) ﻚﺘﻳﻮﻟﻭﺃ ﺐﺴﺣ ﺎﻬﺒﺗﺭﻭ ﺎﻫﺭﺎﻬﻇﺇ ﻲﻓ ﺐﻏﺮﺗ ﻲﺘﻟﺍ ﺓﺪﻤﻋﻷﺍ ﺮﺘﺧﺍ");
            ui.separator();

            let all_cols = TableColumnId::all_columns();
            for col in all_cols {
                let mut is_vis = app.ui_state.visible_columns.contains(&col);
                let pos_opt = app.ui_state.visible_columns.iter().position(|&c| c == col);
                ui.horizontal(|ui| {
                    if ui.checkbox(&mut is_vis, loc.column_title(col)).changed() {
                        if is_vis {
                            if !app.ui_state.visible_columns.contains(&col) {
                                app.ui_state.visible_columns.push(col);
                            }
                        } else if app.ui_state.visible_columns.len() > 2 {
                            app.ui_state.visible_columns.retain(|&c| c != col);
                        }
                        app.mark_ui_state_dirty();
                    }
                    if let Some(idx) = pos_opt {
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui
                                .add_enabled(
                                    idx + 1 < app.ui_state.visible_columns.len(),
                                    egui::Button::new("▼").small(),
                                )
                                .on_hover_text("ﻞﻔﺳﻷﺍ/ﺭﺎﺴﻴﻠﻟ ﺩﻮﻤﻌﻟﺍ ﻚﻳﺮﺤﺗ")
                                .clicked()
                            {
                                app.ui_state.visible_columns.swap(idx, idx + 1);
                                app.mark_ui_state_dirty();
                            }
                            if ui
                                .add_enabled(idx > 0, egui::Button::new("▲").small())
                                .on_hover_text("ﻰﻠﻋﻷﺍ/ﻦﻴﻤﻴﻠﻟ ﺩﻮﻤﻌﻟﺍ ﻚﻳﺮﺤﺗ")
                                .clicked()
                            {
                                app.ui_state.visible_columns.swap(idx, idx - 1);
                                app.mark_ui_state_dirty();
                            }
                        });
                    }
                });
            }

            ui.separator();
            if ui.button("ﺔﻴﺿﺍﺮﺘﻓﻻﺍ ﺓﺪﻤﻋﻷﺍ ﺓﺩﺎﻌﺘﺳﺍ ↺").clicked() {
                app.ui_state.visible_columns = TableColumnId::default_visible_columns();
                app.mark_ui_state_dirty();
            }
        });
    app.show_column_chooser_dialog = open;
}

fn draw_about_and_shortcuts_dialogs(
    app: &mut RustIdmApp,
    ctx: &egui::Context,
    _loc: &Loc,
    palette: &ThemePalette,
) {
    if app.show_shortcuts_dialog {
        let mut open = app.show_shortcuts_dialog;
        egui::Window::new("StormGet — ﺢﻴﺗﺎﻔﻤﻟﺍ ﺔﺣﻮﻟ ﺕﺍﺭﺎﺼﺘﺧﺍ")
            .collapsible(false)
            .resizable(false)
            .default_width(450.0)
            .open(&mut open)
            .show(ctx, |ui| {
                egui::Grid::new("shortcuts_table_grid")
                    .striped(true)
                    .num_columns(2)
                    .spacing([24.0, 6.0])
                    .show(ui, |ui| {
                        for (key, desc) in [
                            ("Ctrl + N", "ﺪﻳﺪﺟ ﻞﻳﺰﻨﺗ ﺔﻓﺎﺿﺇ ﺓﺬﻓﺎﻧ ﺢﺘﻓ"),
                            ("Ctrl + V", "ﺔﻈﻓﺎﺤﻟﺍ ﻦﻣ ﺓﺮﺷﺎﺒﻣ ﻂﺑﺍﺭ ﻖﺼﻟ"),
                            ("Ctrl + F", "ﻊﻳﺮﺴﻟﺍ ﺚﺤﺒﻟﺍ ﻂﻳﺮﺷ ﻲﻓ ﺰﻴﻛﺮﺘﻟﺍ"),
                            ("Ctrl + A", "ﺔﻴﻟﺎﺤﻟﺍ ﺔﻤﺋﺎﻘﻟﺍ ﻲﻓ ﺕﻼﻳﺰﻨﺘﻟﺍ ﻊﻴﻤﺟ ﺪﻳﺪﺤﺗ"),
                            ("Ctrl + Click", "ﺔﻗﺮﻔﺘﻣ ﺮﺻﺎﻨﻋ ﺓﺪﻋ ﺪﻳﺪﺤﺗ ﺀﺎﻐﻟﺇ ﻭﺃ ﺪﻳﺪﺤﺗ"),
                            ("Shift + Click", "ﺕﻼﻳﺰﻨﺘﻟﺍ ﻦﻣ ﻞﺼﺘﻣ ﻕﺎﻄﻧ ﺪﻳﺪﺤﺗ"),
                            ("Delete", "(ﻒﻠﻤﻟﺍ ﻆﻔﺣ ﻊﻣ) ﺓﺩﺪﺤﻤﻟﺍ ﺕﻼﻳﺰﻨﺘﻟﺍ ﻞﺠﺳ ﻑﺬﺣ"),
                            ("Shift + Delete", "ﺹﺮﻘﻟﺍ ﻦﻣ ﺎﻬﺗﺎﻔﻠﻣ ﻊﻣ ﺓﺩﺪﺤﻤﻟﺍ ﺕﻼﻳﺰﻨﺘﻟﺍ ﻑﺬﺣ"),
                            ("Enter", "ﺩﺪﺤﻤﻟﺍ ﻞﻳﺰﻨﺘﻟﺍ ﻞﻴﺻﺎﻔﺗ ﺢﺘﻓ"),
                            ("Escape", "ﺪﻳﺪﺤﺘﻟﺍ ﺀﺎﻐﻟﺇ ﻭﺃ ﺔﻴﻗﺎﻴﺴﻟﺍ ﺔﻤﺋﺎﻘﻟﺍ ﻕﻼﻏﺇ"),
                            ("Alt + F4", "ﻕﻼﻏﻹﺍ ﺔﺳﺎﻴﺳ ﻖﻓﻭ ﺓﺬﻓﺎﻨﻟﺍ ﻕﻼﻏﺇ"),
                        ] {
                            ui.label(RichText::new(key).monospace().strong().color(palette.color_info));
                            ui.label(desc);
                            ui.end_row();
                        }
                    });
            });
        app.show_shortcuts_dialog = open;
    }

    if app.show_about_dialog {
        let mut open = app.show_about_dialog;
        egui::Window::new("About StormGet — ﺞﻣﺎﻧﺮﺒﻟﺍ ﻝﻮﺣ")
            .collapsible(false)
            .resizable(false)
            .default_width(480.0)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.heading(RichText::new("(ﺎﻘﺑﺎﺳ StormGet (RustIDM").strong());
                ui.label(format!("v{} :ﺔﻣﺰﺤﻟﺍ ﻦﻣ ﺝﺮﺨﺘﺴﻤﻟﺍ ﺭﺍﺪﺻﻹﺍ", env!("CARGO_PKG_VERSION")));
                ui.separator();
                ui.label("Rust: ﺔﻐﻠﺑ ﻲﻨﺒﻣ ﻲﻓﺍﺮﺘﺣﺍ ﻲﺒﺘﻜﻣ ﺕﻼﻳﺰﻨﺗ ﻉﺮﺴﻣﻭ ﺮﻳﺪﻣ");
                ui.label("(Lock-Free Atomic Segment Telemetry). ﻊﻃﺎﻘﻤﻠﻟ ﻲﻔﺼﻧ ﻲﻜﻴﻣﺎﻨﻳﺩ ﻢﻴﺴﻘﺗ •");
                ui.label("(write_at). ﺔﻨﻣﺍﺰﺘﻤﻟﺍ ﺔﻴﻌﺿﻮﻤﻟﺍ ﺔﺑﺎﺘﻜﻟﺍﻭ (NTFS Sparse) ﺓﺮﺛﺎﻨﺘﻤﻟﺍ ﺕﺎﻔﻠﻤﻟﺍ ﻢﻋﺩ •");
                ui.label("YouTube 10MB Range. ﻭ MPEG-DASH ﻭ HLS AES-128 ﺚﺑ ﻢﻋﺩﻭ ﺕﺎﺤﻔﺼﺘﻤﻟﺍ ﻊﻣ ﻲﻠﺻﺃ ﻞﻣﺎﻜﺗ •");
                ui.label("(SHA-256 / MD5 / CRC32). ﺔﻴﻤﻗﺮﻟﺍ ﺕﺎﻤﺼﺒﻟﺍ ﻖﻴﻗﺪﺗﻭ ﺪﻌﺑ ﻦﻋ ZIP ﺕﺎﻔﻴﺷﺭﺃ ﺺﺤﻓ •");
            });
        app.show_about_dialog = open;
    }
}

fn draw_batch_dialog(app: &mut RustIdmApp, ctx: &egui::Context, _loc: &Loc) {
    if !app.show_batch_dialog {
        return;
    }
    let mut open = app.show_batch_dialog;
    egui::Window::new("(Batch Download) ﺔﻴﻌﻓﺪﻟﺍ ﻁﺎﻤﻧﻷﺍ ﻊﻴﺳﻮﺗ / ﺓﺩﺪﻌﺘﻣ ﺕﻼﻳﺰﻨﺗ ﺔﻓﺎﺿﺇ")
        .collapsible(false)
        .resizable(true)
        .default_width(560.0)
        .open(&mut open)
        .show(ctx, |ui| {
            ui.label("(:ﺮﻄﺳ ﻞﻛ ﻲﻓ ﻂﺑﺍﺭ) ﻂﺑﺍﻭﺭ ﺔﻤﺋﺎﻗ ﻭﺃ (https://cdn.example.com/part_[01-10].zip :ﻝﺎﺜﻣ) ﻂﺑﺍﻭﺭ ﻂﻤﻧ ﻞﺧﺩﺃ");
            ui.add(
                egui::TextEdit::multiline(&mut app.batch_pattern_input)
                    .desired_rows(5)
                    .desired_width(530.0),
            );
            ui.horizontal(|ui| {
                ui.label("(Step): ﺓﻮﻄﺨﻟﺍ ﺭﺍﺪﻘﻣ");
                ui.add(egui::DragValue::new(&mut app.batch_step_input).range(1..=100));
                ui.separator();
                ui.label(":ﻑﺪﻬﺘﺴﻤﻟﺍ ﺭﻮﺑﺎﻄﻟﺍ");
                egui::ComboBox::from_id_salt("batch_queue_combo")
                    .selected_text(&app.batch_queue_selection)
                    .show_ui(ui, |ui| {
                        let q_names: Vec<String> = if app.queues.is_empty() {
                            vec!["main".to_owned(), "sync".to_owned()]
                        } else {
                            app.queues.iter().map(|q| q.name.clone()).collect()
                        };
                        for q in q_names {
                            ui.selectable_value(&mut app.batch_queue_selection, q.clone(), q);
                        }
                    });
                ui.checkbox(&mut app.batch_start_immediately, "ﺍﺭﻮﻓ ﻞﻴﻤﺤﺘﻟﺍ ﺀﺪﺑ");
            });

            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if ui.button("ﺕﻼﻳﺰﻨﺘﻟﺍ ﺔﻟﻭﺪﺟﻭ ﻂﻤﻨﻟﺍ ﻊﻴﺳﻮﺗ 🚀").clicked() {
                    let req = BatchExpandRequest {
                        pattern_or_urls: app.batch_pattern_input.clone(),
                        step: Some(app.batch_step_input.max(1)),
                        queue_name: Some(app.batch_queue_selection.clone()),
                        cookies: None,
                        referer: None,
                        start_immediately: Some(app.batch_start_immediately),
                    };
                    let _ = app.commands.send(UiCommand::AddBatch(req));
                    app.status_message = "...ﺔﻌﻓﺪﻟﺍ ﺔﻟﻭﺪﺟﻭ ﻊﻴﺳﻮﺗ ﺭﺎﺟ".to_owned();
                }
                if ui.button("ﻞﻘﺤﻟﺍ ﺢﺴﻣ").clicked() {
                    app.batch_pattern_input.clear();
                    app.batch_last_response = None;
                }
            });

            if let Some(resp) = &app.batch_last_response {
                ui.separator();
                ui.label(
                    RichText::new(format!(
                        " ﻊﻴﺳﻮﺗ ﻢﺗ ✔{} :ﺔﻟﻭﺪﺟ ﺖﻤﺗ | ﻂﺑﺍﺭ {} :ﺯﻭﺎﺠﺘﻤﻟﺍ ﺭﺮﻜﻤﻟﺍ | {}",
                        resp.expanded_urls.len(),
                        resp.queued_task_ids.len(),
                        resp.skipped_duplicates
                    ))
                    .strong()
                    .color(Color32::from_rgb(46, 196, 108)),
                );
            }
        });
    app.show_batch_dialog = open;
}

fn draw_refresh_dialog(app: &mut RustIdmApp, ctx: &egui::Context, _loc: &Loc) {
    if !app.show_refresh_dialog {
        return;
    }
    let mut open = app.show_refresh_dialog;
    let mut submitted = false;
    egui::Window::new("(Refresh Expired URL) ﺔﻴﺣﻼﺼﻟﺍ ﻲﻬﺘﻨﻤﻟﺍ ﻞﻴﻤﺤﺘﻟﺍ ﻂﺑﺍﺭ ﺚﻳﺪﺤﺗ")
        .collapsible(false)
        .resizable(false)
        .default_width(520.0)
        .open(&mut open)
        .show(ctx, |ui| {
            ui.label(":ﻥﺎﻣﺄﺑ ﺔﻠﻤﺤﻤﻟﺍ ﺀﺍﺰﺟﻷﺍ ﻑﺎﻨﺌﺘﺳﻻ ETag ﻭ ﻢﺠﺤﻟﺍ ﻖﺑﺎﻄﺗ ﻦﻣ ﻙﺮﺤﻤﻟﺍ ﻖﻘﺤﺘﻴﺳ .ﺔﺛﺪﺤﻤﻟﺍ ﺰﻴﻛﻮﻜﻟﺍ ﻭﺃ ﺪﻳﺪﺠﻟﺍ ﻂﺑﺍﺮﻟﺍ ﻞﺧﺩﺃ");
            egui::Grid::new("refresh_url_grid")
                .num_columns(2)
                .spacing([10.0, 6.0])
                .show(ui, |ui| {
                    ui.label(":ﺔﻤﻬﻤﻟﺍ ﻑﺮﻌﻣ");
                    ui.label(RichText::new(&app.refresh_task_id).monospace());
                    ui.end_row();

                    ui.label("(New URL): ﺪﻳﺪﺠﻟﺍ ﻂﺑﺍﺮﻟﺍ");
                    ui.add(
                        egui::TextEdit::singleline(&mut app.refresh_new_url).desired_width(380.0),
                    );
                    ui.end_row();

                    ui.label("(:ﻱﺭﺎﻴﺘﺧﺍ) ﺔﺛﺪﺤﻤﻟﺍ ﺰﻴﻛﻮﻜﻟﺍ");
                    ui.add(
                        egui::TextEdit::singleline(&mut app.refresh_new_cookies)
                            .password(true)
                            .desired_width(380.0),
                    );
                    ui.end_row();
                });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.button("ﻞﻳﺰﻨﺘﻟﺍ ﻑﺎﻨﺌﺘﺳﺍﻭ ﻖﻘﺤﺘﻟﺍ 🔗").clicked() {
                    let payload = RefreshUrlPayload {
                        task_id: app.refresh_task_id.clone(),
                        new_url: app.refresh_new_url.trim().to_owned(),
                        cookies: opt_non_empty(&app.refresh_new_cookies),
                    };
                    let _ = app.commands.send(UiCommand::RefreshUrl(payload));
                    app.status_message = "...ﺩﺪﺠﻤﻟﺍ ﻂﺑﺍﺮﻟﺍ ﻦﻣ ﻖﻘﺤﺘﻟﺍ ﺭﺎﺟ".to_owned();
                    submitted = true;
                }
                if ui.button("ﺀﺎﻐﻟﺇ").clicked() {
                    submitted = true;
                }
            });
        });
    app.show_refresh_dialog = open && !submitted;
}

fn draw_grabber_dialog(app: &mut RustIdmApp, ctx: &egui::Context, _loc: &Loc) {
    if !app.show_grabber_dialog {
        return;
    }
    let mut open = app.show_grabber_dialog;
    egui::Window::new("(Bounded BFS Site Grabber) ﻂﺑﺍﻭﺮﻟﺍ ﺝﺍﺮﺨﺘﺳﺍﻭ ﻒﺣﺰﻟﺍ ﺓﺍﺩﺃ")
        .collapsible(false)
        .resizable(true)
        .default_width(620.0)
        .open(&mut open)
        .show(ctx, |ui| {
            egui::Grid::new("grabber_cfg_grid")
                .num_columns(2)
                .spacing([10.0, 6.0])
                .show(ui, |ui| {
                    ui.label(":ﺔﻳﺍﺪﺒﻟﺍ ﺔﺤﻔﺻ ﻂﺑﺍﺭ");
                    ui.add(
                        egui::TextEdit::singleline(&mut app.crawl_config.start_url)
                            .desired_width(430.0),
                    );
                    ui.end_row();

                    ui.label(":ﺔﺑﻮﻠﻄﻤﻟﺍ ﺕﺍﺩﺍﺪﺘﻣﻻﺍ");
                    ui.add(
                        egui::TextEdit::singleline(&mut app.crawl_extensions_csv)
                            .desired_width(430.0),
                    );
                    ui.end_row();
                });

            ui.horizontal(|ui| {
                ui.label("(Max Depth): ﻖﻤﻋ ﻰﺼﻗﺃ");
                ui.add(egui::DragValue::new(&mut app.crawl_config.max_depth).range(0..=5));
                ui.separator();
                ui.label(":ﺕﺎﺤﻔﺻ ﺩﺪﻋ ﻰﺼﻗﺃ");
                ui.add(egui::DragValue::new(&mut app.crawl_config.max_pages).range(1..=200));
            });

            ui.add_space(4.0);
            if ui
                .add_enabled(
                    !app.crawl_in_progress && !app.crawl_config.start_url.trim().is_empty(),
                    egui::Button::new("ﺕﺎﻔﻠﻤﻟﺍ ﻑﺎﺸﻜﺘﺳﺍﻭ ﻒﺣﺰﻟﺍ ﺀﺪﺑ 🔍"),
                )
                .clicked()
            {
                app.crawl_config.allowed_extensions = app
                    .crawl_extensions_csv
                    .split(',')
                    .map(|s| s.trim().trim_start_matches('.').to_owned())
                    .filter(|s| !s.is_empty())
                    .collect();
                app.crawl_in_progress = true;
                let _ = app
                    .commands
                    .send(UiCommand::CrawlSite(app.crawl_config.clone()));
            }

            if let Some(report) = &app.crawl_report {
                ui.separator();
                ui.label(format!(
                    " ﺺﺤﻓ ﻢﺗ{} ﻑﺎﺸﺘﻛﺍﻭ ،ﺔﺤﻔﺻ {}:ﻖﺑﺎﻄﻣ ﻒﻠﻣ ",
                    report.pages_visited,
                    report.matched_files.len()
                ));
                egui::ScrollArea::vertical().max_height(150.0).show(ui, |ui| {
                    for link in &report.matched_files {
                        let mut sel = *app.crawl_selected_links.get(link).unwrap_or(&true);
                        if ui.checkbox(&mut sel, link).changed() {
                            app.crawl_selected_links.insert(link.clone(), sel);
                        }
                    }
                });
                if ui.button("ﺓﺩﺪﺤﻤﻟﺍ ﺕﺎﻔﻠﻤﻟﺍ ﻞﻳﺰﻨﺗ ⬇").clicked() {
                    let chosen: Vec<String> = app
                        .crawl_selected_links
                        .iter()
                        .filter(|(_, &v)| v)
                        .map(|(k, _)| k.clone())
                        .collect();
                    if !chosen.is_empty() {
                        let _ = app.commands.send(UiCommand::AddBatch(BatchExpandRequest {
                            pattern_or_urls: chosen.join("\n"),
                            step: Some(1),
                            queue_name: Some("main".to_owned()),
                            cookies: None,
                            referer: Some(app.crawl_config.start_url.clone()),
                            start_immediately: Some(true),
                        }));
                    }
                }
            }
        });
    app.show_grabber_dialog = open;
}

fn draw_zip_preview_dialog(app: &mut RustIdmApp, ctx: &egui::Context, loc: &Loc) {
    if !app.show_zip_preview_dialog {
        return;
    }
    let mut open = app.show_zip_preview_dialog;
    egui::Window::new("(Remote ZIP Central Directory Inspector) ﺪﻌﺑ ﻦﻋ ZIP ﺕﺎﻔﻴﺷﺭﺃ ﺺﺣﺎﻓ")
        .collapsible(false)
        .resizable(true)
        .default_width(640.0)
        .open(&mut open)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("ZIP: ﻒﻴﺷﺭﺃ ﻂﺑﺍﺭ");
                ui.add(
                    egui::TextEdit::singleline(&mut app.zip_preview_url).desired_width(390.0),
                );
                if ui
                    .add_enabled(
                        !app.zip_preview_loading && !app.zip_preview_url.trim().is_empty(),
                        egui::Button::new("ﻒﻴﺷﺭﻷﺍ ﺺﺤﻓ 🔍"),
                    )
                    .clicked()
                {
                    app.zip_preview_loading = true;
                    let _ = app.commands.send(UiCommand::PreviewZip(
                        ZipPreviewRequestPayload {
                            url: app.zip_preview_url.trim().to_owned(),
                            cookies: opt_non_empty(&app.zip_preview_cookies),
                            referer: None,
                            user_agent: None,
                        },
                    ));
                }
            });

            if let Some(preview) = &app.zip_preview_data {
                ui.separator();
                ui.horizontal(|ui| {
<<<<<<< HEAD
                    ui.label(
                        RichText::new(format!(
                            "{} :ﺕﺎﻔﻠﻤﻟﺍ ﻲﻟﺎﻤﺟﺇ  |  {} :ﻒﻴﺷﺭﻷﺍ ﻢﺠﺣ",
                            preview.total_entries,
                            loc.format_bytes(preview.archive_total_size)
                        ))
                        .strong(),
                    );
                    if ui.button("ﻞﻣﺎﻜﻟﺎﺑ ﻒﻴﺷﺭﻷﺍ ﻞﻳﺰﻨﺗ ⬇").clicked() {
=======
                    ui.label(RichText::new(format!("Total Entries: {}  |  Archive Size: {}", preview.total_entries, loc.format_bytes(preview.archive_total_size))).strong());
                    if ui.button("Download Full ZIP Archive").clicked() {
>>>>>>> efc40c730b5c3e9a4001ccff0b896d7f2f3ef344
                        let _ = app.commands.send(UiCommand::AddDownload {
                            payload: InterceptDownloadPayload {
                                url: preview.url.clone(),
                                queue_name: Some("main".to_owned()),
                                ..Default::default()
                            },
                            save_dir: None,
                            start_immediately: true,
                        });
<<<<<<< HEAD
                    }
                });
                egui::ScrollArea::vertical().max_height(180.0).show(ui, |ui| {
                    for entry in &preview.entries {
                        ui.label(
                            RichText::new(format!(
                                "• {} ({} -> {} | CRC32: {})",
                                entry.filename,
                                loc.format_bytes(entry.compressed_size),
                                loc.format_bytes(entry.uncompressed_size),
                                entry.crc32_hex
                            ))
                            .monospace()
                            .size(11.0),
                        );
=======
>>>>>>> efc40c730b5c3e9a4001ccff0b896d7f2f3ef344
                    }
                });
            }
        });
    app.show_zip_preview_dialog = open;
}

fn draw_import_export_dialog(app: &mut RustIdmApp, ctx: &egui::Context, _loc: &Loc) {
    if !app.show_import_export_dialog {
        return;
    }
    let mut open = app.show_import_export_dialog;
    egui::Window::new("(IDM .ef2 & .txt) ﺕﻼﻳﺰﻨﺘﻟﺍ ﺔﻤﺋﺎﻗ ﺮﻳﺪﺼﺗﻭ ﺩﺍﺮﻴﺘﺳﺍ")
        .collapsible(false)
        .resizable(true)
        .default_width(560.0)
        .open(&mut open)
        .show(ctx, |ui| {
            ui.label(":ﺔﻴﻟﺎﺤﻟﺍ ﺔﻤﺋﺎﻘﻟﺍ ﺮﻳﺪﺼﺗ ﻰﻠﻋ ﻂﻐﺿﺍ ﻭﺃ ،ﺩﺍﺮﻴﺘﺳﻼﻟ ﺔﻴﺼﻧ ﻂﺑﺍﻭﺭ ﺔﻤﺋﺎﻗ ﻭﺃ .ef2 ﻒﻠﻣ ﻯﻮﺘﺤﻣ ﻖﺼﻟﺍ");
            ui.add(
                egui::TextEdit::multiline(&mut app.import_export_text)
                    .desired_rows(9)
                    .desired_width(530.0)
                    .code_editor(),
            );
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(
                        !app.import_export_text.trim().is_empty(),
                        egui::Button::new("ﻡﺎﻬﻤﻟﺍ ﺔﻟﻭﺪﺟﻭ ﺩﺍﺮﻴﺘﺳﺍ 📥"),
                    )
                    .clicked()
                {
                    let _ = app.commands.send(UiCommand::ImportList(
                        crate::ImportTasksPayload {
                            content: app.import_export_text.clone(),
                            default_queue: Some("main".to_owned()),
                            start_immediately: Some(true),
                        },
                    ));
                }
                if ui.button("(.ef2) ﺔﻴﻟﺎﺤﻟﺍ ﺔﻤﺋﺎﻘﻟﺍ ﺮﻳﺪﺼﺗ 📤").clicked() {
                    let _ = app.commands.send(UiCommand::ExportList);
                }
                if ui.button("ﺔﻈﻓﺎﺤﻟﺍ ﻰﻟﺇ ﺦﺴﻧ 📋").clicked() {
                    ui.ctx().copy_text(app.import_export_text.clone());
                }
            });
        });
    app.show_import_export_dialog = open;
}
<<<<<<< HEAD
=======

fn draw_options_dialog(app: &mut RustIdmApp, ctx: &egui::Context, _loc: &Loc) {
    if !app.show_options_dialog {
        return;
    }
    let mut open = app.show_options_dialog;
    egui::Window::new("RustIDM Options & Engine Configuration")
        .collapsible(false)
        .resizable(true)
        .default_width(540.0)
        .open(&mut open)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                for (tab, title) in [
                    (OptionsTab::General, "General"),
                    (OptionsTab::Connection, "Connections & Speed"),
                    (OptionsTab::FileTypes, "File Types"),
                    (OptionsTab::Proxy, "Proxy / UA"),
                    (OptionsTab::SiteLogins, "Site Logins"),
                ] {
                    if ui.selectable_label(app.options_tab == tab, title).clicked() {
                        app.options_tab = tab;
                    }
                }
            });
            ui.separator();

            match app.options_tab {
                OptionsTab::General => {
                    ui.label("Base Download Directory:");
                    let mut dir_str = app.app_config.download_dir.display().to_string();
                    if ui.text_edit_singleline(&mut dir_str).changed() {
                        app.app_config.download_dir = PathBuf::from(dir_str);
                    }
                    ui.checkbox(&mut app.app_config.auto_start_queued_downloads, "Auto start intercepted downloads");
                    ui.checkbox(&mut app.app_config.enable_checksum_verification, "Verify SHA-256 / MD5 / CRC32 automatically");
                }
                OptionsTab::Connection => {
                    ui.horizontal(|ui| {
                        ui.label("Default Max Connections per File (1..32):");
                        ui.add(egui::Slider::new(&mut app.app_config.max_connections_per_file, 1..=32));
                    });
                    ui.horizontal(|ui| {
                        ui.label("Speed limit (KB/s):");
                        let mut limit = app.app_config.global_speed_limit_kbps as f32;
                        if ui.add(egui::Slider::new(&mut limit, 0.0..=51_200.0)).changed() {
                            app.app_config.global_speed_limit_kbps = limit.round() as u64;
                            app.speed_limit_kbps = app.app_config.global_speed_limit_kbps;
                        }
                    });
                }
                OptionsTab::FileTypes => {
                    ui.label("Intercept extensions:");
                    let mut ext = app.app_config.intercept_extensions.join(", ");
                    if ui.text_edit_multiline(&mut ext).changed() {
                        app.app_config.intercept_extensions = ext.split(',').map(|s| s.trim().trim_start_matches('.').to_lowercase()).filter(|s| !s.is_empty()).collect();
                    }
                }
                OptionsTab::Proxy => {
                    let mut proxy = app.app_config.proxy_url.clone().unwrap_or_default();
                    if ui.text_edit_singleline(&mut proxy).changed() {
                        app.app_config.proxy_url = opt_non_empty(&proxy);
                    }
                }
                OptionsTab::SiteLogins => {
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(&mut app.new_login_domain).hint_text("domain.com"));
                        ui.add(egui::TextEdit::singleline(&mut app.new_login_user).hint_text("user"));
                        ui.add(egui::TextEdit::singleline(&mut app.new_login_secret).password(true));
                        if ui.button("Add").clicked() && !app.new_login_domain.trim().is_empty() {
                            app.app_config.site_logins.push(SiteLoginCredential {
                                domain: app.new_login_domain.trim().to_owned(),
                                username: app.new_login_user.trim().to_owned(),
                                auth_header_or_cookie: app.new_login_secret.trim().to_owned(),
                                enabled: true,
                            });
                            app.new_login_domain.clear();
                            app.new_login_user.clear();
                            app.new_login_secret.clear();
                        }
                    });
                }
            }

            ui.separator();
            if ui.button("Save & Apply Configuration").clicked() {
                let _ = app.commands.send(UiCommand::SaveConfig(app.app_config.clone()));
                let _ = app.commands.send(UiCommand::SetSpeedLimit(app.app_config.global_speed_limit_kbps));
            }
        });
    app.show_options_dialog = open;
}

fn draw_scheduler_dialog(app: &mut RustIdmApp, ctx: &egui::Context, _loc: &Loc) {
    if !app.show_scheduler_dialog {
        return;
    }
    let mut open = app.show_scheduler_dialog;
    let mut schedule = app.schedule.clone();
    egui::Window::new("Scheduler & Multi-Queue Manager")
        .collapsible(false)
        .resizable(true)
        .default_width(510.0)
        .open(&mut open)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Selected Queue:");
                let q_names: Vec<String> = if app.queues.is_empty() {
                    vec!["main".to_owned(), "sync".to_owned()]
                } else {
                    app.queues.iter().map(|q| q.name.clone()).collect()
                };
                egui::ComboBox::from_id_salt("scheduler_queue_selector")
                    .selected_text(&app.selected_queue_name)
                    .show_ui(ui, |ui| {
                        for q in q_names {
                            ui.selectable_value(&mut app.selected_queue_name, q.clone(), q);
                        }
                    });

                if ui.button("+ Create Queue").clicked() && !app.new_queue_name_input.trim().is_empty() {
                    let name = app.new_queue_name_input.trim().to_owned();
                    let new_q = QueueConfig {
                        name: name.clone(),
                        max_concurrent_downloads: 4,
                        start_time: None,
                        stop_time: None,
                        post_action: crate::PostQueueAction::Notify,
                        schedule_mode: ScheduleMode::Disabled,
                        start_at: None,
                    };
                    let _ = app.commands.send(UiCommand::UpsertQueue(new_q));
                    app.selected_queue_name = name;
                    app.new_queue_name_input.clear();
                }
            });

            ui.separator();
            if ui.button("Save Schedule").clicked() {
                app.schedule = schedule.clone();
                let _ = app.commands.send(UiCommand::SetSchedule(schedule));
            }
        });
    app.show_scheduler_dialog = open;
}

pub fn draw_details_dialog(app: &mut RustIdmApp, ctx: &egui::Context, palette: &ThemePalette) {
    if !app.show_details {
        return;
    }
    if let Some(task) = app.selected_download().cloned() {
        let progress = if task.total_size == 0 {
            0.0
        } else {
            (task.total_downloaded() as f32 / task.total_size as f32).clamp(0.0, 1.0)
        };
        let pct_int = (progress * 100.0).floor() as u32;
        let mut request_pause = false;
        let mut request_resume = false;
        let mut request_cancel = false;

        egui::CentralPanel::default().show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.heading(format!("{pct_int}% {}", task.filename));
                });
                ui.separator();
                // Top Tab Bar matching Screenshots 1, 2, 3
                ui.horizontal(|ui| {
                    for (tab, title) in [
                        (DialogTab::DownloadStatus, "حالة التحميل"),
                        (DialogTab::SpeedLimiter, "محدد السرعة"),
                        (DialogTab::OptionsOnCompletion, "الخيارات عند إكتمال التحميل"),
                    ] {
                        let is_active = app.dialog_tab == tab;
                        let text = if is_active {
                            RichText::new(title).strong().color(Color32::WHITE)
                        } else {
                            RichText::new(title).color(palette.text_muted)
                        };
                        if ui.selectable_label(is_active, text).clicked() {
                            app.dialog_tab = tab;
                        }
                    }
                });

                ui.add_space(2.0);

                // Framed Box with thin light border matching Screenshots 1, 2, 3
                let box_border = Stroke::new(1.0, Color32::from_rgb(180, 180, 180));
                egui::Frame::canvas(ui.style())
                    .stroke(box_border)
                    .inner_margin(8.0)
                    .show(ui, |ui| {
                        match app.dialog_tab {
                            DialogTab::DownloadStatus => {
                                // Screenshot 1: URL at top of box
                                let mut url_copy = task.url.clone();
                                ui.add(
                                    egui::TextEdit::singleline(&mut url_copy)
                                        .desired_width(ui.available_width()),
                                );

                                ui.add_space(6.0);

                                egui::Grid::new("details_status_grid")
                                    .num_columns(2)
                                    .spacing([24.0, 5.0])
                                    .show(ui, |ui| {
                                        ui.label(":الحالة");
                                        let status_str = match task.status {
                                            DownloadStatus::Downloading => "جاري التحميل",
                                            DownloadStatus::Paused => "إيقاف",
                                            DownloadStatus::Completed => "مكتمل",
                                            DownloadStatus::Failed => "فشل التحميل",
                                            DownloadStatus::Queued => "في الانتظار",
                                            DownloadStatus::NeedsUrlRefresh => "رابط منتهي الصلاحية",
                                            DownloadStatus::RefreshingUrl => "جاري تحديث الرابط...",
                                            DownloadStatus::Assembling => "جاري تجميع الأجزاء...",
                                            DownloadStatus::VerifyingChecksum => "جاري فحص البصمة...",
                                        };
                                        ui.label(RichText::new(status_str).strong());
                                        ui.end_row();

                                        ui.label("حجم الملف");
                                        let size_str = if task.total_size > 0 {
                                            format_idm_bytes(task.total_size)
                                        } else {
                                            "(مجهول)".to_string()
                                        };
                                        ui.label(RichText::new(size_str).monospace());
                                        ui.end_row();

                                        ui.label("المحمل");
                                        let downloaded = task.total_downloaded();
                                        let dl_str = if task.total_size > 0 {
                                            let pct = (downloaded as f64 / task.total_size as f64) * 100.0;
                                            format!("{} ( {:.2} %)", format_idm_bytes(downloaded), pct)
                                        } else {
                                            format_idm_bytes(downloaded)
                                        };
                                        ui.label(RichText::new(dl_str).monospace());
                                        ui.end_row();

                                        ui.label("معدل النقل");
                                        let total_speed: u64 = task.segments.iter().map(|s| s.last_speed_bps).sum();
                                        let spd_str = if task.status == DownloadStatus::Downloading && total_speed > 0 {
                                            if total_speed >= 1024 * 1024 {
                                                format!("{:.2} مب/ثانية", total_speed as f64 / (1024.0 * 1024.0))
                                            } else if total_speed >= 1024 {
                                                format!("{:.1} كب/ثانية", total_speed as f64 / 1024.0)
                                            } else {
                                                format!("{} بايت/ثانية", total_speed)
                                            }
                                        } else {
                                            "0 بايت/ثانية".to_string()
                                        };
                                        ui.label(RichText::new(spd_str).monospace());
                                        ui.end_row();

                                        ui.label("الوقت المتبقي");
                                        let time_str = if task.status == DownloadStatus::Downloading && total_speed > 0 && task.total_size > downloaded {
                                            let rem_bytes = task.total_size - downloaded;
                                            let rem_secs = rem_bytes / total_speed;
                                            let h = rem_secs / 3600;
                                            let m = (rem_secs % 3600) / 60;
                                            let s = rem_secs % 60;
                                            format!("{:02}:{:02}:{:02}", h, m, s)
                                        } else {
                                            "(مجهول)".to_string()
                                        };
                                        ui.label(time_str);
                                        ui.end_row();

                                        ui.label("إمكانية الإستئناف");
                                        ui.label(if task.resumable { "نعم" } else { "لا" });
                                        ui.end_row();
                                    });
                            }
                            DialogTab::SpeedLimiter => {
                                // Screenshot 2: Speed Limiter
                                let total_speed: u64 = task.segments.iter().map(|s| s.last_speed_bps).sum();
                                let spd_str = if task.status == DownloadStatus::Downloading && total_speed > 0 {
                                    if total_speed >= 1024 * 1024 {
                                        format!("{:.2} مب/ثانية", total_speed as f64 / (1024.0 * 1024.0))
                                    } else {
                                        format!("{:.1} كب/ثانية", total_speed as f64 / 1024.0)
                                    }
                                } else {
                                    "0 بايت/ثانية".to_string()
                                };
                                ui.horizontal(|ui| {
                                    ui.label("معدل النقل");
                                    ui.label(RichText::new(spd_str).monospace());
                                });

                                ui.add_space(4.0);

                                if ui.checkbox(&mut app.speed_limiter_enabled, "إستخدام محدد السرعة").changed() {
                                    let limit = if app.speed_limiter_enabled {
                                        app.speed_limit_kbps.max(10)
                                    } else {
                                        0
                                    };
                                    let _ = app.commands.send(UiCommand::SetSpeedLimit(limit));
                                }

                                ui.add_space(2.0);
                                ui.label(":السرعة القصوى للتحميل");

                                ui.horizontal(|ui| {
                                    let mut speed_val = app.speed_limit_kbps.max(10);
                                    let edit = ui.add(egui::DragValue::new(&mut speed_val).range(1..=100_000));
                                    if edit.changed() {
                                        app.speed_limit_kbps = speed_val;
                                        if app.speed_limiter_enabled {
                                            let _ = app.commands.send(UiCommand::SetSpeedLimit(speed_val));
                                        }
                                    }
                                    ui.label("ك.بايت/ث");

                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        if ui.button("إخفاء التبويب").clicked() {
                                            app.dialog_tab = DialogTab::DownloadStatus;
                                        }
                                    });
                                });

                                ui.add_space(4.0);
                                ui.checkbox(&mut app.speed_limiter_remember, "تذكر إعدادات محدد التحميل لهذا الملف عند إيقاف/إستئناف التحميل");
                            }
                            DialogTab::OptionsOnCompletion => {
                                // Screenshot 3: Options on completion
                                let save_to_path = task.save_dir.join(&task.filename).display().to_string();
                                ui.horizontal(|ui| {
                                    ui.label(":حفظ إلى");
                                    ui.label(RichText::new(save_to_path).monospace());
                                });

                                ui.checkbox(&mut app.completion_notify, "الإشعار بإكتمال التحميل");
                                ui.separator();

                                ui.label(RichText::new("هذه الإعدادات غير متوفرة عند يكون \"الإشعار بإكتمال التحميل\" قيد التشغيل").color(Color32::from_rgb(160, 160, 160)));

                                ui.checkbox(&mut app.completion_hangup, "قطع الإتصال عند الانتهاء");
                                ui.checkbox(&mut app.completion_exit_idm, "الخروج من Internet Download Manager عند الإنتهاء");

                                ui.horizontal(|ui| {
                                    ui.checkbox(&mut app.completion_shutdown_pc, "إيقاف تشغيل الجهاز عند الانتهاء");
                                    egui::ComboBox::from_id_salt("shutdown_action_combo")
                                        .selected_text(app.completion_shutdown_action.label())
                                        .show_ui(ui, |ui| {
                                            for action in [
                                                ShutdownAction::Shutdown,
                                                ShutdownAction::Restart,
                                                ShutdownAction::Sleep,
                                            ] {
                                                ui.selectable_value(&mut app.completion_shutdown_action, action, action.label());
                                            }
                                        });

                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        if ui.button("إخفاء التبويب").clicked() {
                                            app.dialog_tab = DialogTab::DownloadStatus;
                                        }
                                    });
                                });

                                ui.horizontal(|ui| {
                                    ui.add_space(16.0);
                                    ui.checkbox(&mut app.completion_force_kill, "إنهاء العمليات");
                                });
                            }
                            DialogTab::IntegrityAndMirrors => {
                                ui.label(format!("SHA-256: {}", task.computed_sha256.as_deref().unwrap_or("—")));
                                ui.label(format!("MD5:     {}", task.computed_md5.as_deref().unwrap_or("—")));
                                ui.label(format!("CRC32:   {}", task.computed_crc32.as_deref().unwrap_or("—")));
                                draw_live_speed_graph(ui, palette, &task);
                            }
                        }
                    });

                ui.add_space(6.0);

                // IDM Green Progress Bar
                draw_idm_green_progress_bar(ui, palette, progress);

                ui.add_space(6.0);

                // 3 Action Buttons matching Screenshots 1, 2, 3:
                // [ إخفاء التفاصيل >> ]       [ بدء / إيقاف ]        [ إلغاء الأمر ]
                ui.horizontal(|ui| {
                    let details_label = if app.show_connection_details {
                        "إخفاء التفاصيل >>"
                    } else {
                        "عرض التفاصيل <<"
                    };
                    if ui.add_sized([120.0, 24.0], egui::Button::new(details_label)).clicked() {
                        app.show_connection_details = !app.show_connection_details;
                    }

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.add_sized([110.0, 24.0], egui::Button::new("إلغاء الأمر")).clicked() {
                            request_cancel = true;
                        }

                        let start_pause_label = if task.status == DownloadStatus::Downloading {
                            "إيقاف"
                        } else {
                            "بدء"
                        };
                        if ui.add_sized([110.0, 24.0], egui::Button::new(RichText::new(start_pause_label).strong())).clicked() {
                            if task.status == DownloadStatus::Downloading {
                                request_pause = true;
                            } else {
                                request_resume = true;
                            }
                        }
                    });
                });

                // Connection Details (when enabled)
                if app.show_connection_details {
                    ui.add_space(4.0);
                    ui.vertical_centered(|ui| {
                        ui.label(RichText::new("نقاط البداية لكل اتصال وحالة تقدمه").strong().color(palette.text_main));
                    });
                    ui.add_space(2.0);
                    draw_idm_connections_bar(ui, palette, &task, progress);
                    ui.add_space(2.0);
                    draw_idm_connections_table(ui, palette, &task, progress);
                }
            });

        if request_pause {
            let _ = app.commands.send(UiCommand::Control {
                task_id: task.task_id.clone(),
                action: "pause",
            });
            app.status_message = "تم إيقاف التحميل".to_owned();
        } else if request_resume {
            let _ = app.commands.send(UiCommand::Control {
                task_id: task.task_id.clone(),
                action: "resume",
            });
            app.status_message = "تم استئناف التحميل".to_owned();
        } else if request_cancel {
            let _ = app.commands.send(UiCommand::Control {
                task_id: task.task_id.clone(),
                action: "pause",
            });
            app.show_details = false;
        }
    }
}
>>>>>>> efc40c730b5c3e9a4001ccff0b896d7f2f3ef344
