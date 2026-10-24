use super::{
    app::RustIdmApp,
    i18n::Loc,
    painter::{draw_idm_connections_bar, draw_idm_connections_table, draw_idm_green_progress_bar, draw_live_speed_graph},
    theme::ThemePalette,
    types::{DialogTab, OptionsTab, UiCommand},
};
use crate::{
    BatchExpandRequest, DownloadStatus, InterceptDownloadPayload, QueueConfig, QueueSchedule,
    RefreshUrlPayload, ScheduleMode, SiteLoginCredential, ZipPreviewRequestPayload,
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

    draw_add_url_dialog(app, ctx, &loc);
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

fn draw_add_url_dialog(app: &mut RustIdmApp, ctx: &egui::Context, _loc: &Loc) {
    if !app.show_add_dialog {
        return;
    }
    let mut open = app.show_add_dialog;
    let mut submitted = false;

    egui::Window::new("Enter new address to download")
        .collapsible(false)
        .resizable(true)
        .default_width(560.0)
        .open(&mut open)
        .show(ctx, |ui| {
            egui::Grid::new("add_url_main_grid")
                .num_columns(2)
                .spacing([10.0, 6.0])
                .show(ui, |ui| {
                    ui.label("Address (URL):");
                    ui.add(egui::TextEdit::singleline(&mut app.new_url).hint_text("https://example.com/file.zip").desired_width(410.0));
                    ui.end_row();

                    ui.label("Save Filename:");
                    ui.add(egui::TextEdit::singleline(&mut app.new_filename).hint_text("Auto-detected if blank").desired_width(410.0));
                    ui.end_row();

                    ui.label("Target Queue:");
                    egui::ComboBox::from_id_salt("add_url_queue_combo")
                        .selected_text(&app.new_queue_selection)
                        .show_ui(ui, |ui| {
                            let q_names: Vec<String> = if app.queues.is_empty() {
                                vec!["main".to_owned(), "sync".to_owned()]
                            } else {
                                app.queues.iter().map(|q| q.name.clone()).collect()
                            };
                            for q in q_names {
                                ui.selectable_value(&mut app.new_queue_selection, q.clone(), q);
                            }
                        });
                    ui.end_row();

                    ui.label("Description:");
                    ui.add(egui::TextEdit::singleline(&mut app.new_description).desired_width(410.0));
                    ui.end_row();
                });

            ui.add_space(4.0);
            ui.checkbox(&mut app.show_add_advanced, "Advanced (Headers, Cookies, Checksums & Mirrors)");

            if app.show_add_advanced {
                ui.separator();
                egui::Grid::new("add_url_adv_grid")
                    .num_columns(2)
                    .spacing([10.0, 5.0])
                    .show(ui, |ui| {
                        ui.label("Referer:");
                        ui.add(egui::TextEdit::singleline(&mut app.new_referer).desired_width(390.0));
                        ui.end_row();

                        ui.label("Cookies:");
                        ui.add(egui::TextEdit::singleline(&mut app.new_cookies).desired_width(390.0));
                        ui.end_row();

                        ui.label("Expected SHA-256:");
                        ui.add(egui::TextEdit::singleline(&mut app.new_expected_sha256).desired_width(390.0));
                        ui.end_row();

                        ui.label("Expected MD5:");
                        ui.add(egui::TextEdit::singleline(&mut app.new_expected_md5).desired_width(390.0));
                        ui.end_row();

                        ui.label("Expected CRC32:");
                        ui.add(egui::TextEdit::singleline(&mut app.new_expected_crc32).desired_width(390.0));
                        ui.end_row();

                        ui.label("Mirror URLs:");
                        ui.add(egui::TextEdit::multiline(&mut app.new_mirrors_text).desired_rows(2).desired_width(390.0));
                        ui.end_row();
                    });
            }

            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.button("Download Now (OK)").clicked() {
                    let url = app.new_url.trim().to_owned();
                    if !url.is_empty() {
                        let mirrors: Vec<String> = app
                            .new_mirrors_text
                            .lines()
                            .map(|s| s.trim().to_owned())
                            .filter(|s| !s.is_empty())
                            .collect();
                        let payload = InterceptDownloadPayload {
                            url,
                            source_page_url: opt_non_empty(&app.new_referer),
                            filename: opt_non_empty(&app.new_filename),
                            cookies: opt_non_empty(&app.new_cookies),
                            referer: opt_non_empty(&app.new_referer),
                            user_agent: opt_non_empty(&app.new_user_agent),
                            queue_name: Some(app.new_queue_selection.clone()),
                            expected_sha256: opt_non_empty(&app.new_expected_sha256),
                            expected_md5: opt_non_empty(&app.new_expected_md5),
                            expected_crc32: opt_non_empty(&app.new_expected_crc32),
                            mirror_urls: if mirrors.is_empty() { None } else { Some(mirrors) },
                            description: opt_non_empty(&app.new_description),
                        };
                        let _ = app.commands.send(UiCommand::AddDownload(payload));
                        app.status_message = "Adding download...".to_owned();
                        app.new_url.clear();
                        app.new_filename.clear();
                        submitted = true;
                    }
                }
                if ui.button("Preview Remote ZIP").clicked() {
                    let url = app.new_url.trim().to_owned();
                    if !url.is_empty() {
                        app.zip_preview_url = url;
                        app.show_zip_preview_dialog = true;
                    }
                }
                if ui.button("Cancel").clicked() {
                    submitted = true;
                }
            });
        });
    app.show_add_dialog = open && !submitted;
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

fn draw_details_dialog(app: &mut RustIdmApp, ctx: &egui::Context, loc: &Loc, palette: &ThemePalette) {
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
        egui::Window::new(window_title)
            .id(egui::Id::new("idm_download_status_window"))
            .collapsible(false)
            .resizable(false)
            .default_width(545.0)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    for (tab, title) in [
                        (DialogTab::DownloadStatus, "Download status"),
                        (DialogTab::SpeedLimiter, "Speed Limiter"),
                        (DialogTab::OptionsOnCompletion, "Options on completion"),
                        (DialogTab::IntegrityAndMirrors, "Digests & Mirrors"),
                    ] {
                        if ui.selectable_label(app.dialog_tab == tab, title).clicked() {
                            app.dialog_tab = tab;
                        }
                    }
                });
                ui.separator();

                match app.dialog_tab {
                    DialogTab::DownloadStatus => {
                        ui.label(RichText::new(&task.url).monospace());
                        draw_idm_green_progress_bar(ui, palette, progress);
                        ui.label(format!("Size: {} | Status: {}", loc.format_bytes(task.total_size), loc.status_text(task.status)));

                        if app.show_connection_details {
                            ui.separator();
                            draw_idm_connections_bar(ui, palette, &task, progress);
                            draw_idm_connections_table(ui, palette, &task, progress);
                        }
                    }
                    DialogTab::SpeedLimiter => {
                        ui.checkbox(&mut app.speed_limiter_enabled, "Use Speed Limiter");
                    }
                    DialogTab::OptionsOnCompletion => {
                        ui.checkbox(&mut app.close_on_completion, "Close progress dialog when download completes");
                        ui.checkbox(&mut app.open_on_completion, "Show notification on completion");
                    }
                    DialogTab::IntegrityAndMirrors => {
                        ui.label(format!("SHA-256: {}", task.computed_sha256.as_deref().unwrap_or("—")));
                        ui.label(format!("MD5:     {}", task.computed_md5.as_deref().unwrap_or("—")));
                        ui.label(format!("CRC32:   {}", task.computed_crc32.as_deref().unwrap_or("—")));
                        draw_live_speed_graph(ui, palette, &task);
                    }
                }
            });
        app.show_details = open;
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
