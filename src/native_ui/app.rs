use super::{
    dialogs::draw_all_dialogs,
    i18n::Loc,
    painter::{
        draw_file_category_icon, draw_idm_connections_bar, draw_live_speed_graph,
        draw_queue_mini_icon, idm_toolbar_button, tree_child_row, tree_root_row,
    },
    theme::{apply_idm_theme, ThemePalette},
    types::{
        AppLanguage, BottomInspectorTab, DialogTab, DownloadFilter, OptionsTab,
        ShutdownAction, SortColumn, ThemeMode, ToolbarIcon, TreeIconKind, UiCommand, UiEvent,
    },
    worker::{open_file, open_file_with, reveal_in_explorer, spawn_api_worker},
};
use crate::{
    AppConfig, BatchExpandResponse, CrawlConfig, CrawlReport, DownloadStateMeta, DownloadStatus,
    EngineStatistics, FileCategory, QueueConfig, QueueSchedule, RemoteZipPreviewResponse,
    SegmentStatus,
};
use eframe::egui::{self, Align, Color32, Layout, Pos2, Rect, RichText, Sense, Stroke, Vec2};
use std::{
    collections::HashMap,
    sync::mpsc::{self, Receiver, Sender},
    time::Duration,
};

pub struct RustIdmApp {
    pub commands: Sender<UiCommand>,
    pub events: Receiver<UiEvent>,
    pub downloads: Vec<DownloadStateMeta>,
    pub queues: Vec<QueueConfig>,
    pub app_config: AppConfig,
    pub engine_stats: Option<EngineStatistics>,
    pub selected_task: Option<String>,
    pub filter: DownloadFilter,
    pub search_query: String,
    pub sort_column: SortColumn,
    pub sort_ascending: bool,
    pub theme_mode: ThemeMode,
    pub language: AppLanguage,
    pub show_categories_panel: bool,
    pub show_bottom_inspector: bool,
    pub bottom_inspector_tab: BottomInspectorTab,
    pub expand_all_downloads: bool,
    pub expand_unfinished: bool,
    pub expand_finished: bool,
    pub expand_queues: bool,
    pub show_add_dialog: bool,
    pub show_add_advanced: bool,
    pub show_batch_dialog: bool,
    pub show_grabber_dialog: bool,
    pub show_refresh_dialog: bool,
    pub show_zip_preview_dialog: bool,
    pub show_import_export_dialog: bool,
    pub show_options_dialog: bool,
    pub options_tab: OptionsTab,
    pub show_scheduler_dialog: bool,
    pub selected_queue_name: String,
    pub new_queue_name_input: String,
    pub show_details: bool,
    pub show_connection_details: bool,
    pub dialog_tab: DialogTab,
    pub close_on_completion: bool,
    pub open_on_completion: bool,
    // Add URL Dialog fields (معلومات ملف التحميل)
    pub new_url: String,
    pub new_filename: String,
    pub new_category: FileCategory,
    pub new_save_folder: String,
    pub new_remember_path: bool,
    pub new_probed_size_bytes: Option<u64>,
    pub new_referer: String,
    pub new_cookies: String,
    pub new_user_agent: String,
    pub new_expected_sha256: String,
    pub new_expected_md5: String,
    pub new_expected_crc32: String,
    pub new_mirrors_text: String,
    pub new_description: String,
    pub new_queue_selection: String,
    // Completion Options (الخيارات عند إكتمال التحميل)
    pub completion_notify: bool,
    pub completion_hangup: bool,
    pub completion_exit_idm: bool,
    pub completion_shutdown_pc: bool,
    pub completion_shutdown_action: ShutdownAction,
    pub completion_force_kill: bool,
    pub speed_limiter_remember: bool,
    // Batch Dialog fields
    pub batch_pattern_input: String,
    pub batch_step_input: usize,
    pub batch_queue_selection: String,
    pub batch_start_immediately: bool,
    pub batch_last_response: Option<BatchExpandResponse>,
    // Refresh Expired URL Dialog fields
    pub refresh_task_id: String,
    pub refresh_new_url: String,
    pub refresh_new_cookies: String,
    // Site Grabber (Crawler) Wizard fields
    pub crawl_config: CrawlConfig,
    pub crawl_extensions_csv: String,
    pub crawl_excluded_csv: String,
    pub crawl_report: Option<CrawlReport>,
    pub crawl_selected_links: HashMap<String, bool>,
    pub crawl_in_progress: bool,
    // Remote ZIP Inspector fields
    pub zip_preview_url: String,
    pub zip_preview_cookies: String,
    pub zip_preview_data: Option<RemoteZipPreviewResponse>,
    pub zip_preview_loading: bool,
    // Import / Export Dialog fields
    pub import_export_text: String,
    // Site Login Editor fields
    pub new_login_domain: String,
    pub new_login_user: String,
    pub new_login_secret: String,
    // Status & notifications
    pub error_message: Option<String>,
    pub status_message: String,
    pub speed_limit_kbps: u64,
    pub speed_limiter_enabled: bool,
    pub schedule: QueueSchedule,
    pub previous_download_status: HashMap<String, DownloadStatus>,
    pub has_received_downloads: bool,
    pub completed_notice: Option<String>,
}

impl Default for RustIdmApp {
    fn default() -> Self {
        let (command_tx, command_rx) = mpsc::channel();
        let (event_tx, event_rx) = mpsc::channel();
        spawn_api_worker(command_rx, event_tx);
        Self {
            commands: command_tx,
            events: event_rx,
            downloads: Vec::new(),
            queues: Vec::new(),
            app_config: AppConfig::default(),
            engine_stats: None,
            selected_task: None,
            filter: DownloadFilter::All,
            search_query: String::new(),
            sort_column: SortColumn::UpdatedAt,
            sort_ascending: false,
            theme_mode: ThemeMode::Dark, // Default to Dark mode matching IDM 6.43 screenshots
            language: AppLanguage::Arabic, // Default to Arabic localization matching IDM 6.43 screenshots
            show_categories_panel: true,
            show_bottom_inspector: true,
            bottom_inspector_tab: BottomInspectorTab::SegmentMap,
            expand_all_downloads: true,
            expand_unfinished: true,
            expand_finished: true,
            expand_queues: true,
            show_add_dialog: false,
            show_add_advanced: false,
            show_batch_dialog: false,
            show_grabber_dialog: false,
            show_refresh_dialog: false,
            show_zip_preview_dialog: false,
            show_import_export_dialog: false,
            show_options_dialog: false,
            options_tab: OptionsTab::General,
            show_scheduler_dialog: false,
            selected_queue_name: "main".to_owned(),
            new_queue_name_input: String::new(),
            show_details: false,
            show_connection_details: true,
            dialog_tab: DialogTab::DownloadStatus,
            close_on_completion: false,
            open_on_completion: false,
            new_url: String::new(),
            new_filename: String::new(),
            new_category: FileCategory::General,
            new_save_folder: crate::default_download_directory().to_string_lossy().to_string(),
            new_remember_path: true,
            new_probed_size_bytes: None,
            new_referer: String::new(),
            new_cookies: String::new(),
            new_user_agent: String::new(),
            new_expected_sha256: String::new(),
            new_expected_md5: String::new(),
            new_expected_crc32: String::new(),
            new_mirrors_text: String::new(),
            new_description: String::new(),
            new_queue_selection: "main".to_owned(),
            completion_notify: true,
            completion_hangup: false,
            completion_exit_idm: false,
            completion_shutdown_pc: false,
            completion_shutdown_action: ShutdownAction::Shutdown,
            completion_force_kill: false,
            speed_limiter_remember: false,
            batch_pattern_input: "https://cdn.example.com/archive_part_[01-10].zip".to_owned(),
            batch_step_input: 1,
            batch_queue_selection: "main".to_owned(),
            batch_start_immediately: true,
            batch_last_response: None,
            refresh_task_id: String::new(),
            refresh_new_url: String::new(),
            refresh_new_cookies: String::new(),
            crawl_config: CrawlConfig::default(),
            crawl_extensions_csv: "mp4,mkv,pdf,zip,rar,iso".to_owned(),
            crawl_excluded_csv: "/logout,/admin,/cart".to_owned(),
            crawl_report: None,
            crawl_selected_links: HashMap::new(),
            crawl_in_progress: false,
            zip_preview_url: String::new(),
            zip_preview_cookies: String::new(),
            zip_preview_data: None,
            zip_preview_loading: false,
            import_export_text: String::new(),
            new_login_domain: String::new(),
            new_login_user: String::new(),
            new_login_secret: String::new(),
            error_message: None,
            status_message: "جاهز".to_owned(),
            speed_limit_kbps: 0,
            speed_limiter_enabled: false,
            schedule: QueueSchedule::default(),
            previous_download_status: HashMap::new(),
            has_received_downloads: false,
            completed_notice: None,
        }
    }
}

impl eframe::App for RustIdmApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        apply_idm_theme(ctx, self.theme_mode);
        self.receive_events(ctx);

        self.draw_menu_bar(ctx);
        self.draw_toolbar(ctx);
        self.draw_status_bar(ctx);

        if self.show_bottom_inspector {
            self.draw_bottom_inspector(ctx);
        }
        if self.show_categories_panel {
            self.draw_categories(ctx);
        }

        self.draw_download_table(ctx);
        draw_all_dialogs(self, ctx);

        ctx.request_repaint_after(Duration::from_millis(250));
    }
}

impl RustIdmApp {
    pub fn selected_download(&self) -> Option<&DownloadStateMeta> {
        self.downloads
            .iter()
            .find(|task| Some(task.task_id.as_str()) == self.selected_task.as_deref())
    }

    pub fn has_selected(&self) -> bool {
        self.selected_download().is_some()
    }

    pub fn control_selected(&mut self, action: &'static str) {
        if let Some(task) = self.selected_download() {
            let _ = self.commands.send(UiCommand::Control {
                task_id: task.task_id.clone(),
                action,
            });
            self.status_message = format!("{} requested", action);
        }
    }

    pub fn stop_all_downloads(&mut self) {
        for task in &self.downloads {
            if task.status == DownloadStatus::Downloading || task.status == DownloadStatus::Queued {
                let _ = self.commands.send(UiCommand::Control {
                    task_id: task.task_id.clone(),
                    action: "pause",
                });
            }
        }
        self.status_message = "Stopped active downloads".to_owned();
    }

    pub fn start_queue_downloads(&mut self) {
        for task in &self.downloads {
            if task.status == DownloadStatus::Paused || task.status == DownloadStatus::Queued {
                let _ = self.commands.send(UiCommand::Control {
                    task_id: task.task_id.clone(),
                    action: "resume",
                });
            }
        }
        self.status_message = "Queue started".to_owned();
    }

    pub fn delete_completed_downloads(&mut self) {
        let _ = self.commands.send(UiCommand::DeleteCompleted);
        self.status_message = "Removing completed records...".to_owned();
    }

    pub fn remove_selected_download(&mut self) {
        if let Some(task) = self.selected_download() {
            let _ = self.commands.send(UiCommand::RemoveDownload(task.task_id.clone()));
            self.status_message = "Removing download task...".to_owned();
        }
    }

    fn receive_events(&mut self, ctx: &egui::Context) {
        while let Ok(event) = self.events.try_recv() {
            match event {
                UiEvent::Downloads(mut downloads) => {
                    downloads.sort_by(|a, b| b.created_at.cmp(&a.created_at));
                    if self.has_received_downloads {
                        for task in &downloads {
                            // Check for newly intercepted or started download
                            if !self.previous_download_status.contains_key(&task.task_id) {
                                self.selected_task = Some(task.task_id.clone());
                                self.show_details = true;
                                self.show_connection_details = true;
                                self.dialog_tab = DialogTab::DownloadStatus;
                            }

                            if task.status == DownloadStatus::Completed
                                && self.previous_download_status.get(&task.task_id)
                                    != Some(&DownloadStatus::Completed)
                            {
                                if self.close_on_completion
                                    && self.selected_task.as_deref() == Some(task.task_id.as_str())
                                {
                                    self.show_details = false;
                                }
                                if self.completion_notify || self.open_on_completion {
                                    self.completed_notice = Some(format!("اكتمل تحميل الملف:\n{}", task.filename));
                                }
                                if self.completion_exit_idm {
                                    std::process::exit(0);
                                }
                                if self.completion_shutdown_pc {
                                    execute_shutdown(self.completion_shutdown_action, self.completion_force_kill);
                                }
                            }
                        }
                    } else if let Some(active) = downloads.iter().find(|t| t.status == DownloadStatus::Downloading) {
                        if self.selected_task.is_none() {
                            self.selected_task = Some(active.task_id.clone());
                        }
                    }
                    self.previous_download_status = downloads
                        .iter()
                        .map(|task| (task.task_id.clone(), task.status))
                        .collect();
                    self.has_received_downloads = true;
                    self.downloads = downloads;
                    if self
                        .selected_task
                        .as_ref()
                        .is_some_and(|id| !self.downloads.iter().any(|task| &task.task_id == id))
                    {
                        self.selected_task = None;
                    }
                }
                UiEvent::Queues(queues) => self.queues = queues,
                UiEvent::Config(cfg) => {
                    self.speed_limit_kbps = cfg.global_speed_limit_kbps;
                    self.speed_limiter_enabled = cfg.global_speed_limit_kbps > 0;
                    self.app_config = cfg;
                }
                UiEvent::Stats(stats) => self.engine_stats = Some(stats),
                UiEvent::BatchResult(Ok(resp)) => {
                    self.status_message = format!("Batch expanded {} URLs", resp.expanded_urls.len());
                    self.batch_last_response = Some(resp);
                    self.error_message = None;
                }
                UiEvent::BatchResult(Err(err)) => self.error_message = Some(err),
                UiEvent::CrawlResult(Ok(report)) => {
                    self.crawl_in_progress = false;
                    self.crawl_selected_links.clear();
                    for link in &report.matched_files {
                        self.crawl_selected_links.insert(link.clone(), true);
                    }
                    self.status_message = format!("Found {} matching files", report.matched_files.len());
                    self.crawl_report = Some(report);
                    self.error_message = None;
                }
                UiEvent::CrawlResult(Err(err)) => {
                    self.crawl_in_progress = false;
                    self.error_message = Some(err);
                }
                UiEvent::ZipPreviewResult(Ok(preview)) => {
                    self.zip_preview_loading = false;
                    self.zip_preview_data = Some(preview);
                    self.error_message = None;
                }
                UiEvent::ZipPreviewResult(Err(err)) => {
                    self.zip_preview_loading = false;
                    self.error_message = Some(err);
                }
                UiEvent::ExportedText(Ok(text)) => {
                    self.import_export_text = text;
                    self.show_import_export_dialog = true;
                }
                UiEvent::ExportedText(Err(err)) => self.error_message = Some(err),
                UiEvent::Schedule(Ok(schedule)) => {
                    self.schedule = schedule;
                    self.error_message = None;
                }
                UiEvent::Schedule(Err(error)) => self.error_message = Some(error),
                UiEvent::CompletedNotice(filename) => self.completed_notice = Some(filename),
                UiEvent::Status(Ok(msg)) => {
                    self.error_message = None;
                    self.status_message = msg;
                }
                UiEvent::Status(Err(err)) => {
                    self.status_message = "Engine error".to_owned();
                    self.error_message = Some(err);
                }
                UiEvent::EngineConnection(Ok(())) => self.error_message = None,
                UiEvent::EngineConnection(Err(err)) => self.error_message = Some(err),
            }
            ctx.request_repaint();
        }
    }

    fn draw_menu_bar(&mut self, ctx: &egui::Context) {
        let loc = Loc::new(self.language);
        let palette = ThemePalette::get(self.theme_mode);

        egui::TopBottomPanel::top("idm_menu_bar")
            .exact_height(24.0)
            .frame(egui::Frame::new().fill(palette.header_bg).inner_margin(egui::Margin::symmetric(6, 2)))
            .show(ctx, |ui| {
                egui::menu::bar(ui, |ui| {
                    ui.menu_button(loc.menu_tasks(), |ui| {
                        if ui.button(loc.tb_add_url()).clicked() {
                            self.show_add_dialog = true;
                            ui.close_menu();
                        }
                        if ui.button(loc.tb_batch()).clicked() {
                            self.show_batch_dialog = true;
                            ui.close_menu();
                        }
                        if ui.button(loc.tb_grabber()).clicked() {
                            self.show_grabber_dialog = true;
                            ui.close_menu();
                        }
                        ui.separator();
                        if ui.button(loc.tb_stop_all()).clicked() {
                            self.stop_all_downloads();
                            ui.close_menu();
                        }
                    });

                    ui.menu_button(loc.menu_file(), |ui| {
                        if ui.add_enabled(self.has_selected(), egui::Button::new(loc.ctx_properties())).clicked() {
                            self.show_details = true;
                            ui.close_menu();
                        }
                        if ui.add_enabled(self.has_selected(), egui::Button::new(loc.ctx_resume())).clicked() {
                            self.control_selected("resume");
                            ui.close_menu();
                        }
                        if ui.add_enabled(self.has_selected(), egui::Button::new(loc.ctx_pause())).clicked() {
                            self.control_selected("pause");
                            ui.close_menu();
                        }
                        if ui.add_enabled(self.has_selected(), egui::Button::new(loc.ctx_remove())).clicked() {
                            self.remove_selected_download();
                            ui.close_menu();
                        }
                    });

                    ui.menu_button(loc.menu_downloads(), |ui| {
                        if ui.button(loc.tb_start_queue()).clicked() {
                            self.start_queue_downloads();
                            ui.close_menu();
                        }
                        if ui.button(loc.tb_stop_queue()).clicked() {
                            self.stop_all_downloads();
                            ui.close_menu();
                        }
                        ui.separator();
                        if ui.button(loc.tb_scheduler()).clicked() {
                            self.show_scheduler_dialog = true;
                            ui.close_menu();
                        }
                        if ui.button(loc.tb_options()).clicked() {
                            self.show_options_dialog = true;
                            ui.close_menu();
                        }
                    });

                    ui.menu_button(loc.menu_view(), |ui| {
                        ui.checkbox(&mut self.show_categories_panel, loc.cat_title());
                        ui.checkbox(&mut self.show_bottom_inspector, "Inspector");
                        ui.separator();
                        if ui.button(if self.theme_mode == ThemeMode::Dark { "☀️ Light Theme" } else { "🌙 Dark Theme" }).clicked() {
                            self.theme_mode = match self.theme_mode {
                                ThemeMode::Dark => ThemeMode::Light,
                                ThemeMode::Light => ThemeMode::Dark,
                            };
                            ui.close_menu();
                        }
                        if ui.button(if self.language == AppLanguage::Arabic { "English Language" } else { "اللغة العربية" }).clicked() {
                            self.language = match self.language {
                                AppLanguage::Arabic => AppLanguage::English,
                                AppLanguage::English => AppLanguage::Arabic,
                            };
                            ui.close_menu();
                        }
                    });

                    ui.menu_button(loc.menu_help(), |ui| {
                        ui.label("RustIDM v2.0.0 — Pure Rust IDM 6.43 Desktop Edition");
                        ui.label("Dynamic In-Half Segmentation | HLS AES-128 | MPEG-DASH | EJS Solver");
                    });

                    // Search box & Theme/Lang Quick Toggles
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if !self.search_query.is_empty() && ui.small_button("×").clicked() {
                            self.search_query.clear();
                        }
                        ui.add(egui::TextEdit::singleline(&mut self.search_query).hint_text("Search...").desired_width(160.0));

                        let theme_icon = if self.theme_mode == ThemeMode::Dark { "🌙" } else { "☀️" };
                        if ui.small_button(theme_icon).clicked() {
                            self.theme_mode = match self.theme_mode {
                                ThemeMode::Dark => ThemeMode::Light,
                                ThemeMode::Light => ThemeMode::Dark,
                            };
                        }

                        let lang_badge = if self.language == AppLanguage::Arabic { "AR" } else { "EN" };
                        if ui.small_button(lang_badge).clicked() {
                            self.language = match self.language {
                                AppLanguage::Arabic => AppLanguage::English,
                                AppLanguage::English => AppLanguage::Arabic,
                            };
                        }
                    });
                });
            });
    }

    fn draw_toolbar(&mut self, ctx: &egui::Context) {
        let loc = Loc::new(self.language);
        let palette = ThemePalette::get(self.theme_mode);

        egui::TopBottomPanel::top("idm_toolbar")
            .exact_height(68.0)
            .frame(
                egui::Frame::new()
                    .fill(palette.toolbar_bg)
                    .stroke(Stroke::new(1.0, palette.border))
                    .inner_margin(egui::Margin::symmetric(6, 3)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing = Vec2::new(2.0, 0.0);

                    let can_resume = self.selected_download().is_some_and(|t| {
                        matches!(t.status, DownloadStatus::Paused | DownloadStatus::Failed | DownloadStatus::Queued | DownloadStatus::NeedsUrlRefresh)
                    });
                    let can_stop = self.selected_download().is_some_and(|t| {
                        matches!(t.status, DownloadStatus::Downloading | DownloadStatus::Queued | DownloadStatus::RefreshingUrl)
                    });
                    let has_sel = self.has_selected();

                    if idm_toolbar_button(ui, &palette, ToolbarIcon::AddUrl, loc.tb_add_url(), true, false) {
                        self.show_add_dialog = true;
                    }
                    if idm_toolbar_button(ui, &palette, ToolbarIcon::Batch, loc.tb_batch(), true, false) {
                        self.show_batch_dialog = true;
                    }
                    if idm_toolbar_button(ui, &palette, ToolbarIcon::Resume, loc.tb_resume(), can_resume, false) {
                        self.control_selected("resume");
                    }
                    if idm_toolbar_button(ui, &palette, ToolbarIcon::Stop, loc.tb_stop(), can_stop, false) {
                        self.control_selected("pause");
                    }
                    if idm_toolbar_button(ui, &palette, ToolbarIcon::StopAll, loc.tb_stop_all(), true, false) {
                        self.stop_all_downloads();
                    }
                    if idm_toolbar_button(ui, &palette, ToolbarIcon::Delete, loc.tb_delete(), has_sel, false) {
                        self.remove_selected_download();
                    }
                    if idm_toolbar_button(ui, &palette, ToolbarIcon::DeleteCompleted, loc.tb_del_completed(), true, false) {
                        self.delete_completed_downloads();
                    }
                    if idm_toolbar_button(ui, &palette, ToolbarIcon::Options, loc.tb_options(), true, false) {
                        self.show_options_dialog = true;
                    }
                    if idm_toolbar_button(ui, &palette, ToolbarIcon::Scheduler, loc.tb_scheduler(), true, false) {
                        self.show_scheduler_dialog = true;
                    }
                    if idm_toolbar_button(ui, &palette, ToolbarIcon::StartQueue, loc.tb_start_queue(), true, true) {
                        self.start_queue_downloads();
                    }
                    if idm_toolbar_button(ui, &palette, ToolbarIcon::StopQueue, loc.tb_stop_queue(), true, true) {
                        self.stop_all_downloads();
                    }
                    if idm_toolbar_button(ui, &palette, ToolbarIcon::Grabber, loc.tb_grabber(), true, false) {
                        self.show_grabber_dialog = true;
                    }
                    if idm_toolbar_button(ui, &palette, ToolbarIcon::TellFriend, loc.tb_tell_friend(), true, false) {
                        if let Some(url) = self.selected_download().map(|sel| sel.url.clone()) {
                            self.zip_preview_url = url;
                        }
                        self.show_zip_preview_dialog = true;
                    }
                });
            });
    }

    fn draw_status_bar(&self, ctx: &egui::Context) {
        let loc = Loc::new(self.language);
        let palette = ThemePalette::get(self.theme_mode);

        egui::TopBottomPanel::bottom("idm_status_bar")
            .exact_height(22.0)
            .frame(
                egui::Frame::new()
                    .fill(palette.panel_bg)
                    .stroke(Stroke::new(1.0, palette.border))
                    .inner_margin(egui::Margin::symmetric(8, 2)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let total_speed: u64 = self
                        .downloads
                        .iter()
                        .filter(|t| t.status == DownloadStatus::Downloading)
                        .flat_map(|t| t.segments.iter().map(|s| s.last_speed_bps))
                        .sum();

                    let items_label = if self.language == AppLanguage::Arabic {
                        format!("{} عناصر  |  معدل النقل: {}", self.downloads.len(), loc.format_speed(total_speed))
                    } else {
                        format!("{} items  |  Transfer rate: {}", self.downloads.len(), loc.format_speed(total_speed))
                    };

                    ui.label(RichText::new(items_label).size(11.5).color(palette.text_main));

                    if let Some(err) = &self.error_message {
                        ui.separator();
                        ui.colored_label(palette.status_failed, RichText::new(err).size(11.5));
                    }

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(RichText::new(&self.status_message).size(11.5).color(palette.text_muted));
                    });
                });
            });
    }

    fn draw_categories(&mut self, ctx: &egui::Context) {
        let loc = Loc::new(self.language);
        let palette = ThemePalette::get(self.theme_mode);

        egui::SidePanel::left("idm_categories")
            .resizable(true)
            .default_width(178.0)
            .min_width(145.0)
            .frame(egui::Frame::new().fill(palette.panel_bg).inner_margin(egui::Margin::same(3)))
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(palette.table_bg)
                    .stroke(Stroke::new(1.0, palette.border))
                    .show(ui, |ui| {
                        // Categories Header
                        egui::Frame::new()
                            .fill(palette.header_bg)
                            .stroke(Stroke::new(0.5, palette.border))
                            .inner_margin(egui::Margin::symmetric(6, 2))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(loc.cat_title()).size(12.0).strong().color(palette.text_main));
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        if ui.add(egui::Button::new(RichText::new("×").size(13.0).strong()).frame(false)).clicked() {
                                            self.show_categories_panel = false;
                                        }
                                    });
                                });
                            });

                        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                            ui.add_space(4.0);
                            ui.spacing_mut().item_spacing = Vec2::new(0.0, 1.0);

                            // All Downloads
                            if tree_root_row(ui, &palette, &mut self.expand_all_downloads, loc.cat_all(), TreeIconKind::FolderYellow, self.filter == DownloadFilter::All) {
                                self.filter = DownloadFilter::All;
                            }
                            if self.expand_all_downloads {
                                for (cat, label, icon) in [
                                    (FileCategory::Compressed, loc.cat_compressed(), TreeIconKind::Compressed),
                                    (FileCategory::Documents, loc.cat_documents(), TreeIconKind::Document),
                                    (FileCategory::Music, loc.cat_music(), TreeIconKind::Music),
                                    (FileCategory::Programs, loc.cat_programs(), TreeIconKind::Program),
                                    (FileCategory::Video, loc.cat_video(), TreeIconKind::Video),
                                ] {
                                    let target = DownloadFilter::Category(cat);
                                    if tree_child_row(ui, &palette, label, icon, self.filter == target) {
                                        self.filter = target;
                                    }
                                }
                            }

                            // Unfinished
                            if tree_root_row(ui, &palette, &mut self.expand_unfinished, loc.cat_unfinished(), TreeIconKind::FolderYellow, self.filter == DownloadFilter::Unfinished) {
                                self.filter = DownloadFilter::Unfinished;
                            }

                            // Finished
                            if tree_root_row(ui, &palette, &mut self.expand_finished, loc.cat_finished(), TreeIconKind::FolderFinished, self.filter == DownloadFilter::Finished) {
                                self.filter = DownloadFilter::Finished;
                            }

                            // Queues Tree
                            if tree_root_row(ui, &palette, &mut self.expand_queues, loc.cat_queues(), TreeIconKind::FolderYellow, matches!(self.filter, DownloadFilter::Queue(_))) {
                                self.filter = DownloadFilter::Queue("main".to_owned());
                            }
                            if self.expand_queues {
                                for (q_name, label) in [("main", loc.cat_main_queue()), ("sync", loc.cat_sync_queue())] {
                                    let target = DownloadFilter::Queue(q_name.to_owned());
                                    if tree_child_row(ui, &palette, label, TreeIconKind::FolderYellow, self.filter == target) {
                                        self.filter = target;
                                    }
                                }
                            }
                        });
                    });
            });
    }

    fn draw_download_table(&mut self, ctx: &egui::Context) {
        let loc = Loc::new(self.language);
        let palette = ThemePalette::get(self.theme_mode);

        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(palette.panel_bg).inner_margin(egui::Margin::same(3)))
            .show(ctx, |ui| {
                let downloads: Vec<DownloadStateMeta> =
                    self.filtered_downloads().into_iter().cloned().collect();

                egui::Frame::new()
                    .fill(palette.table_bg)
                    .stroke(Stroke::new(1.0, palette.border))
                    .show(ui, |ui| {
                        egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| {
                            let available = ui.available_width().max(760.0);
                            let columns = [
                                (loc.col_filename(), available * 0.27, SortColumn::Filename),
                                (loc.col_queue(), 34.0, SortColumn::Queue),
                                (loc.col_size(), available * 0.10, SortColumn::Size),
                                (loc.col_status(), available * 0.12, SortColumn::Status),
                                (loc.col_time_left(), available * 0.10, SortColumn::TimeLeft),
                                (loc.col_transfer_rate(), available * 0.12, SortColumn::Speed),
                                (loc.col_last_try(), available * 0.13, SortColumn::UpdatedAt),
                                (loc.col_description(), (available * 0.16 - 34.0).max(90.0), SortColumn::Category),
                            ];

                            // Table Header
                            egui::Frame::new()
                                .fill(palette.header_bg)
                                .stroke(Stroke::new(1.0, palette.border))
                                .show(ui, |ui| {
                                    ui.spacing_mut().item_spacing = Vec2::ZERO;
                                    ui.horizontal(|ui| {
                                        for (title, width, col_id) in columns {
                                            let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, 24.0), Sense::click());
                                            let painter = ui.painter();
                                            if resp.hovered() {
                                                painter.rect_filled(rect, 0.0, palette.toolbar_bg);
                                            }
                                            painter.line_segment([rect.right_top(), rect.right_bottom()], Stroke::new(1.0, palette.grid_line));
                                            let arrow = if self.sort_column == col_id {
                                                if self.sort_ascending { " ▲" } else { " ▼" }
                                            } else {
                                                ""
                                            };
                                            painter.text(
                                                Pos2::new(rect.left() + 6.0, rect.center().y),
                                                egui::Align2::LEFT_CENTER,
                                                format!("{title}{arrow}"),
                                                egui::FontId::proportional(12.0),
                                                palette.text_main,
                                            );
                                            if resp.clicked() {
                                                if self.sort_column == col_id {
                                                    self.sort_ascending = !self.sort_ascending;
                                                } else {
                                                    self.sort_column = col_id;
                                                    self.sort_ascending = true;
                                                }
                                            }
                                        }
                                    });
                                });

                            // Table Rows
                            ui.spacing_mut().item_spacing = Vec2::ZERO;
                            for (row_idx, task) in downloads.iter().enumerate() {
                                let selected = self.selected_task.as_deref() == Some(&task.task_id);
                                let row_bg = if selected {
                                    palette.table_selected_row
                                } else if row_idx % 2 == 1 {
                                    palette.table_alt_row
                                } else {
                                    palette.table_bg
                                };

                                let total_width: f32 = columns.iter().map(|(_, w, _)| *w).sum();
                                let (row_rect, response) = ui.allocate_exact_size(Vec2::new(total_width, 23.0), Sense::click());

                                let painter = ui.painter();
                                painter.rect_filled(row_rect, 0.0, row_bg);
                                painter.line_segment([row_rect.left_bottom(), row_rect.right_bottom()], Stroke::new(1.0, palette.grid_line));

                                let mut x = row_rect.left();
                                let cy = row_rect.center().y;
                                let row_text_col = if selected { Color32::WHITE } else { palette.text_main };

                                // Col 0: Icon + Filename
                                let w0 = columns[0].1;
                                draw_file_category_icon(painter, Pos2::new(x + 11.0, cy), task.category);
                                painter.text(Pos2::new(x + 24.0, cy), egui::Align2::LEFT_CENTER, &task.filename, egui::FontId::proportional(12.0), row_text_col);
                                x += w0;

                                // Col 1: Queue Mini Badge
                                let w1 = columns[1].1;
                                draw_queue_mini_icon(painter, Pos2::new(x + w1 * 0.5, cy), task.status == DownloadStatus::Completed);
                                x += w1;

                                // Col 2: Size
                                let w2 = columns[2].1;
                                painter.text(Pos2::new(x + 6.0, cy), egui::Align2::LEFT_CENTER, loc.format_bytes(task.total_size), egui::FontId::proportional(12.0), row_text_col);
                                x += w2;

                                // Col 3: Status
                                let w3 = columns[3].1;
                                let status_col = if selected {
                                    Color32::WHITE
                                } else {
                                    match task.status {
                                        DownloadStatus::Completed => palette.status_complete,
                                        DownloadStatus::Downloading => palette.status_downloading,
                                        DownloadStatus::Failed | DownloadStatus::NeedsUrlRefresh => palette.status_failed,
                                        _ => palette.status_paused,
                                    }
                                };
                                painter.text(Pos2::new(x + 6.0, cy), egui::Align2::LEFT_CENTER, loc.status_text(task.status), egui::FontId::proportional(12.0), status_col);
                                x += w3;

                                // Col 4: Time left
                                let w4 = columns[4].1;
                                let rem_sec = if task.status == DownloadStatus::Downloading {
                                    let rem = task.total_size.saturating_sub(task.total_downloaded());
                                    let spd: u64 = task.segments.iter().map(|s| s.last_speed_bps).sum();
                                    if spd > 0 { Some(rem / spd) } else { None }
                                } else {
                                    None
                                };
                                let t_left = rem_sec.map(|s| loc.time_left_str(s)).unwrap_or_else(|| "—".to_owned());
                                painter.text(Pos2::new(x + 6.0, cy), egui::Align2::LEFT_CENTER, t_left, egui::FontId::proportional(12.0), row_text_col);
                                x += w4;

                                // Col 5: Speed
                                let w5 = columns[5].1;
                                let speed: u64 = task.segments.iter().map(|s| s.last_speed_bps).sum();
                                let spd_str = if speed > 0 && task.status == DownloadStatus::Downloading {
                                    loc.format_speed(speed)
                                } else {
                                    String::new()
                                };
                                painter.text(Pos2::new(x + 6.0, cy), egui::Align2::LEFT_CENTER, spd_str, egui::FontId::proportional(12.0), row_text_col);
                                x += w5;

                                // Col 6: Last Try Date
                                let w6 = columns[6].1;
                                let date_str = task.updated_at.format("%b %d, %H:%M").to_string();
                                painter.text(Pos2::new(x + 6.0, cy), egui::Align2::LEFT_CENTER, date_str, egui::FontId::proportional(11.5), if selected { Color32::WHITE } else { palette.text_muted });
                                x += w6;

                                // Col 7: Description
                                let desc = task.description.as_deref().unwrap_or_else(|| loc.category_name(task.category));
                                painter.text(Pos2::new(x + 6.0, cy), egui::Align2::LEFT_CENTER, desc, egui::FontId::proportional(11.5), if selected { Color32::WHITE } else { palette.text_muted });

                                if response.clicked() {
                                    self.selected_task = Some(task.task_id.clone());
                                }
                                if response.double_clicked() {
                                    self.selected_task = Some(task.task_id.clone());
                                    self.show_details = true;
                                }

                                // Complete IDM 6.43 Right-Click Context Menu
                                response.context_menu(|ui| {
                                    self.selected_task = Some(task.task_id.clone());
                                    ui.label(RichText::new(&task.filename).strong());
                                    ui.separator();

                                    let path = task.final_file_path();
                                    let file_exists = path.exists();

                                    // 1. فتح (Open file)
                                    if ui.add_enabled(file_exists, egui::Button::new(loc.ctx_open())).clicked() {
                                        open_file(&path);
                                        ui.close_menu();
                                    }

                                    // 2. فتح بواسطة... (Open with...)
                                    if ui.add_enabled(file_exists, egui::Button::new(loc.ctx_open_with())).clicked() {
                                        open_file_with(&path);
                                        ui.close_menu();
                                    }

                                    // 3. فتح المجلد (Open folder)
                                    if ui.button(loc.ctx_open_folder()).clicked() {
                                        reveal_in_explorer(&path);
                                        ui.close_menu();
                                    }

                                    ui.separator();

                                    // 4. إعادة تحميل (Redownload)
                                    if ui.button(loc.ctx_redownload()).clicked() {
                                        let _ = self.commands.send(UiCommand::Redownload(task.task_id.clone()));
                                        ui.close_menu();
                                    }

                                    // 5. إستئناف التحميل (Resume)
                                    if task.status != DownloadStatus::Downloading && task.status != DownloadStatus::Completed {
                                        if ui.button(loc.ctx_resume()).clicked() {
                                            let _ = self.commands.send(UiCommand::Control { task_id: task.task_id.clone(), action: "resume" });
                                            ui.close_menu();
                                        }
                                    }

                                    // 6. إيقاف التحميل (Pause)
                                    if task.status == DownloadStatus::Downloading {
                                        if ui.button(loc.ctx_pause()).clicked() {
                                            let _ = self.commands.send(UiCommand::Control { task_id: task.task_id.clone(), action: "pause" });
                                            ui.close_menu();
                                        }
                                    }

                                    // 7. تحديث عنوان التحميل (Refresh URL)
                                    if ui.button(loc.ctx_refresh_url()).clicked() {
                                        self.refresh_task_id = task.task_id.clone();
                                        self.refresh_new_url = task.url.clone();
                                        self.show_refresh_dialog = true;
                                        ui.close_menu();
                                    }

                                    // 8. فحص البصمة
                                    if task.status == DownloadStatus::Completed {
                                        if ui.button(loc.ctx_reverify()).clicked() {
                                            let _ = self.commands.send(UiCommand::ReverifyChecksum(task.task_id.clone()));
                                            ui.close_menu();
                                        }
                                    }

                                    ui.separator();

                                    // 9. نسخ الروابط
                                    if ui.button(loc.ctx_copy_url()).clicked() {
                                        ui.ctx().copy_text(task.url.clone());
                                        ui.close_menu();
                                    }
                                    if ui.button(loc.ctx_copy_path()).clicked() {
                                        ui.ctx().copy_text(path.display().to_string());
                                        ui.close_menu();
                                    }

                                    ui.separator();

                                    // 10. إزالة السجل
                                    if ui.button(loc.ctx_remove()).clicked() {
                                        let _ = self.commands.send(UiCommand::RemoveDownload(task.task_id.clone()));
                                        ui.close_menu();
                                    }

                                    // 11. حذف مع الملف
                                    if ui.button(loc.ctx_delete_with_file()).clicked() {
                                        let _ = self.commands.send(UiCommand::DeleteWithFile(task.task_id.clone()));
                                        ui.close_menu();
                                    }

                                    ui.separator();

                                    // 12. خصائص
                                    if ui.button(loc.ctx_properties()).clicked() {
                                        self.show_details = true;
                                        ui.close_menu();
                                    }
                                });
                            }
                        });
                    });
            });
    }

    fn draw_bottom_inspector(&mut self, ctx: &egui::Context) {
        let loc = Loc::new(self.language);
        let palette = ThemePalette::get(self.theme_mode);

        egui::TopBottomPanel::bottom("idm_bottom_inspector")
            .resizable(true)
            .default_height(168.0)
            .min_height(115.0)
            .frame(
                egui::Frame::new()
                    .fill(palette.panel_bg)
                    .stroke(Stroke::new(1.0, palette.border))
                    .inner_margin(egui::Margin::symmetric(8, 5)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    for (tab, label) in [
                        (BottomInspectorTab::SegmentMap, "Segment Map"),
                        (BottomInspectorTab::SpeedGraph, "Speed Graph"),
                        (BottomInspectorTab::ConnectionLogs, "Logs"),
                        (BottomInspectorTab::ChecksumAudit, "Digests"),
                    ] {
                        if ui.selectable_label(self.bottom_inspector_tab == tab, label).clicked() {
                            self.bottom_inspector_tab = tab;
                        }
                    }

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.small_button("×").clicked() {
                            self.show_bottom_inspector = false;
                        }
                    });
                });
                ui.separator();

                let Some(task) = self.selected_download().cloned() else {
                    ui.centered_and_justified(|ui| {
                        ui.label(RichText::new("Select a task to inspect byte segments, transfer rate curve & digests").color(palette.text_muted));
                    });
                    return;
                };

                let progress = if task.total_size == 0 {
                    0.0
                } else {
                    (task.total_downloaded() as f32 / task.total_size as f32).clamp(0.0, 1.0)
                };

                match self.bottom_inspector_tab {
                    BottomInspectorTab::SegmentMap => {
                        draw_idm_connections_bar(ui, &palette, &task, progress);
                        ui.add_space(4.0);
                        ui.horizontal_wrapped(|ui| {
                            for seg in &task.segments {
                                let badge_col = match seg.status {
                                    SegmentStatus::Completed => palette.status_complete,
                                    SegmentStatus::Downloading => palette.status_downloading,
                                    SegmentStatus::Failed => palette.status_failed,
                                    _ => palette.status_paused,
                                };
                                ui.colored_label(badge_col, format!("#{} [{}..{}] ({})", seg.id + 1, loc.format_bytes(seg.start_byte), if seg.end_byte == u64::MAX { "EOF".into() } else { loc.format_bytes(seg.end_byte) }, loc.format_speed(seg.last_speed_bps)));
                            }
                        });
                    }
                    BottomInspectorTab::SpeedGraph => {
                        draw_live_speed_graph(ui, &palette, &task);
                    }
                    BottomInspectorTab::ConnectionLogs => {
                        egui::ScrollArea::vertical().show(ui, |ui| {
                            for entry in task.connection_logs.iter().rev() {
                                ui.label(format!("[{}] {}", entry.level, entry.message));
                            }
                        });
                    }
                    BottomInspectorTab::ChecksumAudit => {
                        ui.label(format!("SHA-256: {}", task.computed_sha256.as_deref().unwrap_or("—")));
                        ui.label(format!("MD5:     {}", task.computed_md5.as_deref().unwrap_or("—")));
                        ui.label(format!("CRC32:   {}", task.computed_crc32.as_deref().unwrap_or("—")));
                    }
                }
            });
    }

    fn filtered_downloads(&self) -> Vec<&DownloadStateMeta> {
        let query_lower = self.search_query.trim().to_lowercase();
        let mut list: Vec<&DownloadStateMeta> = self
            .downloads
            .iter()
            .filter(|task| match &self.filter {
                DownloadFilter::All => true,
                DownloadFilter::Unfinished => task.status != DownloadStatus::Completed,
                DownloadFilter::Finished => task.status == DownloadStatus::Completed,
                DownloadFilter::Category(category) => task.category == *category,
                DownloadFilter::Queue(q_name) => &task.queue_name == q_name,
            })
            .filter(|task| {
                if query_lower.is_empty() {
                    true
                } else {
                    task.filename.to_lowercase().contains(&query_lower)
                        || task.url.to_lowercase().contains(&query_lower)
                        || task.queue_name.to_lowercase().contains(&query_lower)
                }
            })
            .collect();

        list.sort_by(|a, b| {
            let cmp = match self.sort_column {
                SortColumn::Filename => a.filename.to_lowercase().cmp(&b.filename.to_lowercase()),
                SortColumn::Queue => a.queue_name.cmp(&b.queue_name),
                SortColumn::Size => a.total_size.cmp(&b.total_size),
                SortColumn::Status => (a.status as u8).cmp(&(b.status as u8)),
                SortColumn::TimeLeft => a.total_size.saturating_sub(a.total_downloaded()).cmp(&b.total_size.saturating_sub(b.total_downloaded())),
                SortColumn::Speed => {
                    let sa: u64 = a.segments.iter().map(|s| s.last_speed_bps).sum();
                    let sb: u64 = b.segments.iter().map(|s| s.last_speed_bps).sum();
                    sa.cmp(&sb)
                }
                SortColumn::UpdatedAt => a.updated_at.cmp(&b.updated_at),
                SortColumn::Category => (a.category as u8).cmp(&(b.category as u8)),
            };
            if self.sort_ascending { cmp } else { cmp.reverse() }
        });
        list
    }
}

#[allow(unused_variables)]
fn execute_shutdown(action: ShutdownAction, force: bool) {
    #[cfg(target_os = "windows")]
    {
        let force_flag = if force { "/f" } else { "" };
        match action {
            ShutdownAction::Shutdown => {
                let mut cmd = std::process::Command::new("shutdown");
                cmd.args(&["/s", "/t", "30"]);
                if force {
                    cmd.arg(force_flag);
                }
                let _ = cmd.spawn();
            }
            ShutdownAction::Restart => {
                let mut cmd = std::process::Command::new("shutdown");
                cmd.args(&["/r", "/t", "30"]);
                if force {
                    cmd.arg(force_flag);
                }
                let _ = cmd.spawn();
            }
            ShutdownAction::Sleep => {
                let _ = std::process::Command::new("rundll32.exe")
                    .args(&["powrprof.dll,SetSuspendState", "0,1,0"])
                    .spawn();
            }
        }
    }
}
