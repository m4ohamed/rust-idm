use super::{
    app::RustIdmApp,
    i18n::Loc,
    painter::{
        draw_file_category_icon, draw_idm_connections_bar, draw_idm_connections_table,
        draw_idm_green_progress_bar, draw_live_speed_graph, format_idm_bytes,
    },
    theme::ThemePalette,
    types::{DialogTab, OptionsTab, ShutdownAction, UiCommand},
};
use crate::{
    BatchExpandRequest, DownloadStatus, FileCategory, InterceptDownloadPayload, QueueConfig,
    QueueSchedule, RefreshUrlPayload, ScheduleMode, SiteLoginCredential, ZipPreviewRequestPayload,
};
use eframe::egui::{self, Align, Color32, Layout, RichText, Sense, Stroke, Vec2};
use std::path::PathBuf;

fn opt_non_empty(s: &str) -> Option<String> {
    let t = s.trim();
    if t.is_empty() {
        None
    } else {
        Some(t.to_owned())
    }
}

pub fn draw_all_dialogs(app: &mut RustIdmApp, ctx: &egui::Context) {
    let loc = Loc::new(app.language);
    let palette = ThemePalette::get(app.theme_mode);

    draw_add_url_dialog(app, ctx, &loc, &palette);
    draw_batch_dialog(app, ctx, &loc);
    draw_refresh_dialog(app, ctx, &loc);
    draw_grabber_dialog(app, ctx, &loc);
    draw_zip_preview_dialog(app, ctx, &loc);
    draw_import_export_dialog(app, ctx, &loc);
    draw_options_dialog(app, ctx, &loc);
    draw_scheduler_dialog(app, ctx, &loc);
    draw_details_dialog(app, ctx, &loc, &palette);
    draw_notification_dialog(app, ctx);
}

fn draw_add_url_dialog(app: &mut RustIdmApp, ctx: &egui::Context, loc: &Loc, palette: &ThemePalette) {
    if !app.show_add_dialog {
        return;
    }
    let mut open = app.show_add_dialog;
    let mut submitted_start = false;
    let mut submitted_later = false;
    let mut cancelled = false;

    egui::Window::new("معلومات ملف التحميل")
        .collapsible(false)
        .resizable(false)
        .default_width(550.0)
        .open(&mut open)
        .show(ctx, |ui| {
            ui.spacing_mut().item_spacing = Vec2::new(8.0, 6.0);

            // Row 1: URL
            ui.horizontal(|ui| {
                ui.label(RichText::new("URL").strong().monospace());
                let edit = ui.add(
                    egui::TextEdit::singleline(&mut app.new_url)
                        .hint_text("https://example.com/file.zip")
                        .desired_width(460.0),
                );
                if edit.changed() && app.new_filename.is_empty() && !app.new_url.is_empty() {
                    if let Ok(parsed) = url::Url::parse(&app.new_url) {
                        if let Some(segment) = parsed.path_segments().and_then(|mut s| s.next_back()) {
                            let clean = segment.split('?').next().unwrap_or(segment).trim();
                            if !clean.is_empty() {
                                app.new_filename = clean.to_string();
                                app.new_category = FileCategory::from_filename(clean);
                            }
                        }
                    }
                }
            });

            // Row 2: Categories + Icon & Size
            ui.horizontal(|ui| {
                ui.label("الفئات");
                egui::ComboBox::from_id_salt("add_cat_combo")
                    .selected_text(loc.category_name(app.new_category))
                    .width(180.0)
                    .show_ui(ui, |ui| {
                        for cat in [
                            FileCategory::General,
                            FileCategory::Compressed,
                            FileCategory::Documents,
                            FileCategory::Music,
                            FileCategory::Programs,
                            FileCategory::Video,
                        ] {
                            ui.selectable_value(&mut app.new_category, cat, loc.category_name(cat));
                        }
                    });
                if ui.button("+").clicked() {
                    // Category configuration shortcut
                }

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let size_text = if let Some(sz) = app.new_probed_size_bytes {
                        format_idm_bytes(sz)
                    } else {
                        "6.14 كب".to_string()
                    };
                    ui.label(RichText::new(size_text).strong().color(palette.text_main));
                    let (icon_rect, _) = ui.allocate_exact_size(Vec2::new(18.0, 18.0), Sense::hover());
                    draw_file_category_icon(ui.painter(), icon_rect.center(), app.new_category);
                });
            });

            // Row 3: Save as
            ui.horizontal(|ui| {
                ui.label("الحفظ باسم");
                let save_path = if app.new_filename.is_empty() {
                    format!("{}\\", app.new_save_folder)
                } else {
                    format!("{}\\{}", app.new_save_folder.trim_end_matches('\\'), app.new_filename)
                };
                let mut path_display = save_path;
                ui.add(egui::TextEdit::singleline(&mut path_display).desired_width(400.0));
                if ui.button("...").clicked() {
                    let _ = std::process::Command::new("explorer").arg(&app.new_save_folder).spawn();
                }
            });

            // Row 4: "تذكر هذا المسار لمجلد" Group Box
            let frame_stroke = Stroke::new(1.0, Color32::from_rgb(140, 140, 140));
            egui::Frame::canvas(ui.style())
                .stroke(frame_stroke)
                .inner_margin(6.0)
                .show(ui, |ui| {
                    let cat_label = loc.category_name(app.new_category);
                    ui.checkbox(&mut app.new_remember_path, format!("تذكر هذا المسار لمجلد \"{}\"", cat_label));
                    ui.add(egui::TextEdit::singleline(&mut app.new_save_folder).desired_width(480.0));
                });

            // Row 5: Description
            ui.horizontal(|ui| {
                ui.label("الوصف");
                ui.add(egui::TextEdit::singleline(&mut app.new_description).desired_width(460.0));
            });

            ui.add_space(8.0);

            // Bottom Buttons: [ بدء التحميل ] [ التحميل لاحقاً ] [ إلغاء الأمر ]
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(24.0, 0.0);
                if ui.add_sized([120.0, 24.0], egui::Button::new(RichText::new("بدء التحميل").strong())).clicked() {
                    submitted_start = true;
                }
                if ui.add_sized([120.0, 24.0], egui::Button::new("التحميل لاحقاً")).clicked() {
                    submitted_later = true;
                }
                if ui.add_sized([100.0, 24.0], egui::Button::new("إلغاء الأمر")).clicked() {
                    cancelled = true;
                }
            });
        });

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
            let _ = app.commands.send(UiCommand::AddDownload(payload));
            app.status_message = "جاري بدء التحميل...".to_owned();
            app.show_details = true;
            app.show_connection_details = true;
            app.dialog_tab = DialogTab::DownloadStatus;
            app.new_url.clear();
            app.new_filename.clear();
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
            let _ = app.commands.send(UiCommand::AddDownload(payload));
            app.status_message = "تمت إضافة التنزيل للمنتظرة".to_owned();
            app.new_url.clear();
            app.new_filename.clear();
            app.show_add_dialog = false;
        }
    } else if cancelled {
        app.show_add_dialog = false;
    } else {
        app.show_add_dialog = open;
    }
}


fn draw_batch_dialog(app: &mut RustIdmApp, ctx: &egui::Context, _loc: &Loc) {
    if !app.show_batch_dialog {
        return;
    }
    let mut open = app.show_batch_dialog;
    egui::Window::new("Add Batch Download (Wildcard Pattern Expander)")
        .collapsible(false)
        .resizable(true)
        .default_width(550.0)
        .open(&mut open)
        .show(ctx, |ui| {
            ui.label("Enter wildcard pattern (e.g. https://cdn.example.com/part_[01-10].zip) or multiple URLs:");
            ui.add(egui::TextEdit::multiline(&mut app.batch_pattern_input).desired_rows(4).desired_width(520.0));
            ui.horizontal(|ui| {
                ui.label("Step increment:");
                ui.add(egui::DragValue::new(&mut app.batch_step_input).range(1..=100));
                ui.separator();
                ui.label("Target Queue:");
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
                ui.checkbox(&mut app.batch_start_immediately, "Start immediately");
            });

            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if ui.button("Expand & Enqueue Batch").clicked() {
                    let req = BatchExpandRequest {
                        pattern_or_urls: app.batch_pattern_input.clone(),
                        step: Some(app.batch_step_input.max(1)),
                        queue_name: Some(app.batch_queue_selection.clone()),
                        cookies: None,
                        referer: None,
                        start_immediately: Some(app.batch_start_immediately),
                    };
                    let _ = app.commands.send(UiCommand::AddBatch(req));
                    app.status_message = "Expanding batch pattern...".to_owned();
                }
                if ui.button("Clear").clicked() {
                    app.batch_pattern_input.clear();
                    app.batch_last_response = None;
                }
            });

            if let Some(resp) = &app.batch_last_response {
                ui.separator();
                ui.label(RichText::new(format!("Expanded {} URLs | Queued: {} | Skipped duplicates: {}", resp.expanded_urls.len(), resp.queued_task_ids.len(), resp.skipped_duplicates)).strong().color(Color32::from_rgb(16, 118, 36)));
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
    egui::Window::new("Refresh Expired Download Address")
        .collapsible(false)
        .resizable(false)
        .default_width(510.0)
        .open(&mut open)
        .show(ctx, |ui| {
            ui.label("Provide fresh URL or updated cookies. RustIDM will verify Content-Length & ETag:");
            egui::Grid::new("refresh_url_grid").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                ui.label("Task ID:");
                ui.label(RichText::new(&app.refresh_task_id).monospace());
                ui.end_row();

                ui.label("New URL:");
                ui.add(egui::TextEdit::singleline(&mut app.refresh_new_url).desired_width(380.0));
                ui.end_row();

                ui.label("Updated Cookies:");
                ui.add(egui::TextEdit::singleline(&mut app.refresh_new_cookies).desired_width(380.0));
                ui.end_row();
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.button("Verify & Resume Download").clicked() {
                    let payload = RefreshUrlPayload {
                        task_id: app.refresh_task_id.clone(),
                        new_url: app.refresh_new_url.trim().to_owned(),
                        cookies: opt_non_empty(&app.refresh_new_cookies),
                    };
                    let _ = app.commands.send(UiCommand::RefreshUrl(payload));
                    app.status_message = "Verifying refreshed URL...".to_owned();
                    submitted = true;
                }
                if ui.button("Cancel").clicked() {
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
    egui::Window::new("Site Grabber — Bounded BFS Web Crawler")
        .collapsible(false)
        .resizable(true)
        .default_width(600.0)
        .open(&mut open)
        .show(ctx, |ui| {
            egui::Grid::new("grabber_cfg_grid").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                ui.label("Start Page URL:");
                ui.add(egui::TextEdit::singleline(&mut app.crawl_config.start_url).desired_width(430.0));
                ui.end_row();

                ui.label("Allowed Extensions:");
                ui.add(egui::TextEdit::singleline(&mut app.crawl_extensions_csv).desired_width(430.0));
                ui.end_row();
            });

            ui.horizontal(|ui| {
                ui.label("Max Depth:");
                ui.add(egui::DragValue::new(&mut app.crawl_config.max_depth).range(0..=5));
                ui.separator();
                ui.label("Max Pages:");
                ui.add(egui::DragValue::new(&mut app.crawl_config.max_pages).range(1..=200));
            });

            ui.add_space(4.0);
            if ui.add_enabled(!app.crawl_in_progress && !app.crawl_config.start_url.trim().is_empty(), egui::Button::new("Start Bounded Crawl")).clicked() {
                app.crawl_config.allowed_extensions = app.crawl_extensions_csv.split(',').map(|s| s.trim().trim_start_matches('.').to_owned()).filter(|s| !s.is_empty()).collect();
                app.crawl_in_progress = true;
                let _ = app.commands.send(UiCommand::CrawlSite(app.crawl_config.clone()));
            }
        });
    app.show_grabber_dialog = open;
}

fn draw_zip_preview_dialog(app: &mut RustIdmApp, ctx: &egui::Context, loc: &Loc) {
    if !app.show_zip_preview_dialog {
        return;
    }
    let mut open = app.show_zip_preview_dialog;
    egui::Window::new("Remote ZIP Archive Inspector (HTTP Range EOCD Reader)")
        .collapsible(false)
        .resizable(true)
        .default_width(620.0)
        .open(&mut open)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Remote ZIP URL:");
                ui.add(egui::TextEdit::singleline(&mut app.zip_preview_url).desired_width(380.0));
                if ui.add_enabled(!app.zip_preview_loading && !app.zip_preview_url.trim().is_empty(), egui::Button::new("Inspect ZIP")).clicked() {
                    app.zip_preview_loading = true;
                    let _ = app.commands.send(UiCommand::PreviewZip(ZipPreviewRequestPayload {
                        url: app.zip_preview_url.trim().to_owned(),
                        cookies: opt_non_empty(&app.zip_preview_cookies),
                        referer: None,
                        user_agent: None,
                    }));
                }
            });

            if let Some(preview) = &app.zip_preview_data {
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("Total Entries: {}  |  Archive Size: {}", preview.total_entries, loc.format_bytes(preview.archive_total_size))).strong());
                    if ui.button("Download Full ZIP Archive").clicked() {
                        let _ = app.commands.send(UiCommand::AddDownload(InterceptDownloadPayload {
                            url: preview.url.clone(),
                            queue_name: Some("main".to_owned()),
                            ..Default::default()
                        }));
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
    egui::Window::new("Import / Export Download List (IDM .ef2 & .txt)")
        .collapsible(false)
        .resizable(true)
        .default_width(540.0)
        .open(&mut open)
        .show(ctx, |ui| {
            ui.add(egui::TextEdit::multiline(&mut app.import_export_text).desired_rows(8).desired_width(510.0).code_editor());
            ui.horizontal(|ui| {
                if ui.button("Import & Queue Tasks").clicked() && !app.import_export_text.trim().is_empty() {
                    let _ = app.commands.send(UiCommand::ImportList(crate::ImportTasksPayload {
                        content: app.import_export_text.clone(),
                        default_queue: Some("main".to_owned()),
                        start_immediately: Some(true),
                    }));
                }
                if ui.button("Refresh Export").clicked() {
                    let _ = app.commands.send(UiCommand::ExportList);
                }
            });
        });
    app.show_import_export_dialog = open;
}

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

fn draw_details_dialog(app: &mut RustIdmApp, ctx: &egui::Context, _loc: &Loc, palette: &ThemePalette) {
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
        let window_title = format!("{}% {}", pct_int, task.filename);

        let mut open = app.show_details;
        let mut request_pause = false;
        let mut request_resume = false;
        let mut request_cancel = false;

        egui::Window::new(window_title)
            .id(egui::Id::new("idm_download_status_window"))
            .collapsible(false)
            .resizable(false)
            .default_width(560.0)
            .open(&mut open)
            .show(ctx, |ui| {
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
        } else {
            app.show_details = open;
        }
    }
}

fn draw_notification_dialog(app: &mut RustIdmApp, ctx: &egui::Context) {
    if let Some(msg) = app.completed_notice.clone() {
        let mut open = true;
        egui::Window::new("Download notification")
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.label(msg);
                if ui.button("OK").clicked() {
                    app.completed_notice = None;
                }
            });
        if !open {
            app.completed_notice = None;
        }
    }
}
