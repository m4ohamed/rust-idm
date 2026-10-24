use crate::{
    AppConfig, AppState, BatchExpandRequest, BatchExpandResponse, ControlTaskPayload, CrawlConfig,
    CrawlReport, DownloadStateMeta, DownloadStatus, EngineStatistics, FileCategory,
    ImportTasksPayload, InterceptDownloadPayload, PostQueueAction, QueueConfig, QueueSchedule,
    RefreshUrlPayload, RemoteZipPreviewResponse, ScheduleMode, SegmentStatus, SiteLoginCredential,
    ZipPreviewRequestPayload,
};
use anyhow::{anyhow, Context, Result};
use eframe::egui::{self, Align, Color32, Layout, Pos2, Rect, RichText, Sense, Stroke, Ui, Vec2};
use reqwest::blocking::Client;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::mpsc::{self, Receiver, RecvTimeoutError, Sender},
    thread,
    time::{Duration, Instant},
};
use tokio::runtime::Builder;

const API_BASE: &str = "http://127.0.0.1:16888/api/v1";

enum UiCommand {
    AddDownload(InterceptDownloadPayload),
    AddBatch(BatchExpandRequest),
    RefreshUrl(RefreshUrlPayload),
    CrawlSite(CrawlConfig),
    PreviewZip(ZipPreviewRequestPayload),
    ReverifyChecksum(String),
    UpsertQueue(QueueConfig),
    DeleteQueue(String),
    SaveConfig(AppConfig),
    ExportList,
    ImportList(ImportTasksPayload),
    SetSchedule(QueueSchedule),
    DeleteCompleted,
    RemoveDownload(String),
    Control {
        task_id: String,
        action: &'static str,
    },
    SetSpeedLimit(u64),
}

enum UiEvent {
    Downloads(Vec<DownloadStateMeta>),
    Queues(Vec<QueueConfig>),
    Config(AppConfig),
    Stats(EngineStatistics),
    BatchResult(Result<BatchExpandResponse, String>),
    CrawlResult(Result<CrawlReport, String>),
    ZipPreviewResult(Result<RemoteZipPreviewResponse, String>),
    ExportedText(Result<String, String>),
    Schedule(Result<QueueSchedule, String>),
    CompletedNotice(String),
    Status(Result<String, String>),
    EngineConnection(Result<(), String>),
}

#[derive(Clone, PartialEq, Eq)]
enum DownloadFilter {
    All,
    Unfinished,
    Finished,
    Category(FileCategory),
    Queue(String),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SortColumn {
    Filename,
    Queue,
    Size,
    Status,
    TimeLeft,
    Speed,
    UpdatedAt,
    Category,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum OptionsTab {
    General,
    Connection,
    FileTypes,
    Proxy,
    SiteLogins,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BottomInspectorTab {
    SegmentMap,
    SpeedGraph,
    ConnectionLogs,
    ChecksumAudit,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DialogTab {
    DownloadStatus,
    SpeedLimiter,
    OptionsOnCompletion,
    IntegrityAndMirrors,
}

#[derive(Clone, Copy)]
enum ToolbarIcon {
    AddUrl,
    Resume,
    Stop,
    StopAll,
    Delete,
    DeleteCompleted,
    Options,
    Scheduler,
    StartQueue,
    StopQueue,
    Grabber,
    Batch,
    TellFriend,
}

struct RustIdmApp {
    commands: Sender<UiCommand>,
    events: Receiver<UiEvent>,
    downloads: Vec<DownloadStateMeta>,
    queues: Vec<QueueConfig>,
    app_config: AppConfig,
    engine_stats: Option<EngineStatistics>,
    selected_task: Option<String>,
    filter: DownloadFilter,
    search_query: String,
    sort_column: SortColumn,
    sort_ascending: bool,
    show_categories_panel: bool,
    show_bottom_inspector: bool,
    bottom_inspector_tab: BottomInspectorTab,
    expand_all_downloads: bool,
    expand_unfinished: bool,
    expand_finished: bool,
    expand_queues: bool,
    show_add_dialog: bool,
    show_add_advanced: bool,
    show_batch_dialog: bool,
    show_grabber_dialog: bool,
    show_refresh_dialog: bool,
    show_zip_preview_dialog: bool,
    show_import_export_dialog: bool,
    show_options_dialog: bool,
    options_tab: OptionsTab,
    show_scheduler_dialog: bool,
    selected_queue_name: String,
    new_queue_name_input: String,
    show_details: bool,
    show_connection_details: bool,
    dialog_tab: DialogTab,
    close_on_completion: bool,
    open_on_completion: bool,
    // Add URL Dialog fields
    new_url: String,
    new_filename: String,
    new_referer: String,
    new_cookies: String,
    new_user_agent: String,
    new_expected_sha256: String,
    new_expected_md5: String,
    new_expected_crc32: String,
    new_mirrors_text: String,
    new_description: String,
    new_queue_selection: String,
    // Batch Dialog fields
    batch_pattern_input: String,
    batch_step_input: usize,
    batch_queue_selection: String,
    batch_start_immediately: bool,
    batch_last_response: Option<BatchExpandResponse>,
    // Refresh Expired URL Dialog fields
    refresh_task_id: String,
    refresh_new_url: String,
    refresh_new_cookies: String,
    // Site Grabber (Crawler) Wizard fields
    crawl_config: CrawlConfig,
    crawl_extensions_csv: String,
    crawl_excluded_csv: String,
    crawl_report: Option<CrawlReport>,
    crawl_selected_links: HashMap<String, bool>,
    crawl_in_progress: bool,
    // Remote ZIP Inspector fields
    zip_preview_url: String,
    zip_preview_cookies: String,
    zip_preview_data: Option<RemoteZipPreviewResponse>,
    zip_preview_loading: bool,
    // Import / Export Dialog fields
    import_export_text: String,
    // Site Login Editor fields
    new_login_domain: String,
    new_login_user: String,
    new_login_secret: String,
    // Status & notifications
    error_message: Option<String>,
    status_message: String,
    speed_limit_kbps: u64,
    speed_limiter_enabled: bool,
    schedule: QueueSchedule,
    previous_download_status: HashMap<String, DownloadStatus>,
    has_received_downloads: bool,
    completed_notice: Option<String>,
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
            show_categories_panel: true,
            show_bottom_inspector: true,
            bottom_inspector_tab: BottomInspectorTab::SegmentMap,
            expand_all_downloads: true,
            expand_unfinished: false,
            expand_finished: false,
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
            new_referer: String::new(),
            new_cookies: String::new(),
            new_user_agent: String::new(),
            new_expected_sha256: String::new(),
            new_expected_md5: String::new(),
            new_expected_crc32: String::new(),
            new_mirrors_text: String::new(),
            new_description: String::new(),
            new_queue_selection: "main".to_owned(),
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
            status_message: "Ready".to_owned(),
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
        self.apply_idm_visuals(ctx);
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
        self.draw_dialogs(ctx);
        ctx.request_repaint_after(Duration::from_millis(200));
    }
}

impl RustIdmApp {
    fn apply_idm_visuals(&self, ctx: &egui::Context) {
        let mut visuals = egui::Visuals::light();
        visuals.panel_fill = Color32::from_rgb(240, 240, 240);
        visuals.window_fill = Color32::from_rgb(240, 240, 240);
        visuals.faint_bg_color = Color32::from_rgb(246, 246, 246);
        visuals.extreme_bg_color = Color32::WHITE;
        visuals.widgets.noninteractive.bg_fill = Color32::from_rgb(240, 240, 240);
        visuals.widgets.noninteractive.fg_stroke =
            Stroke::new(1.0_f32, Color32::from_rgb(20, 20, 20));
        visuals.widgets.inactive.bg_fill = Color32::from_rgb(225, 225, 225);
        visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, Color32::from_rgb(173, 173, 173));
        visuals.widgets.hovered.bg_fill = Color32::from_rgb(229, 241, 251);
        visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, Color32::from_rgb(0, 120, 215));
        visuals.widgets.active.bg_fill = Color32::from_rgb(204, 228, 247);
        visuals.widgets.active.bg_stroke = Stroke::new(1.0_f32, Color32::from_rgb(0, 84, 153));
        ctx.set_visuals(visuals);
    }

    fn receive_events(&mut self, ctx: &egui::Context) {
        while let Ok(event) = self.events.try_recv() {
            match event {
                UiEvent::Downloads(mut downloads) => {
                    downloads.sort_by(|a, b| b.created_at.cmp(&a.created_at));
                    if self.has_received_downloads {
                        for task in &downloads {
                            if task.status == DownloadStatus::Completed
                                && self.previous_download_status.get(&task.task_id)
                                    != Some(&DownloadStatus::Completed)
                            {
                                if self.close_on_completion
                                    && self.selected_task.as_deref() == Some(task.task_id.as_str())
                                {
                                    self.show_details = false;
                                }
                                if self.open_on_completion {
                                    self.completed_notice = Some(task.filename.clone());
                                }
                            }
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
                UiEvent::Queues(queues) => {
                    self.queues = queues;
                }
                UiEvent::Config(cfg) => {
                    self.speed_limit_kbps = cfg.global_speed_limit_kbps;
                    self.speed_limiter_enabled = cfg.global_speed_limit_kbps > 0;
                    self.app_config = cfg;
                }
                UiEvent::Stats(stats) => {
                    self.engine_stats = Some(stats);
                }
                UiEvent::BatchResult(Ok(resp)) => {
                    self.status_message = format!(
                        "Batch expanded {} URLs ({} queued)",
                        resp.expanded_urls.len(),
                        resp.queued_task_ids.len()
                    );
                    self.batch_last_response = Some(resp);
                    self.error_message = None;
                }
                UiEvent::BatchResult(Err(err)) => {
                    self.error_message = Some(err);
                }
                UiEvent::CrawlResult(Ok(report)) => {
                    self.crawl_in_progress = false;
                    self.crawl_selected_links.clear();
                    for link in &report.matched_files {
                        self.crawl_selected_links.insert(link.clone(), true);
                    }
                    self.status_message = format!(
                        "Site Grabber visited {} pages, found {} matching files",
                        report.pages_visited,
                        report.matched_files.len()
                    );
                    self.crawl_report = Some(report);
                    self.error_message = None;
                }
                UiEvent::CrawlResult(Err(err)) => {
                    self.crawl_in_progress = false;
                    self.error_message = Some(err);
                }
                UiEvent::ZipPreviewResult(Ok(preview)) => {
                    self.zip_preview_loading = false;
                    self.status_message = format!(
                        "Remote ZIP inspected: {} files ({})",
                        preview.total_entries,
                        format_bytes(preview.archive_total_size)
                    );
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
                    self.status_message = "Exported task list to EF2 format".to_owned();
                }
                UiEvent::ExportedText(Err(err)) => {
                    self.error_message = Some(err);
                }
                UiEvent::Schedule(Ok(schedule)) => {
                    self.schedule = schedule;
                    self.error_message = None;
                }
                UiEvent::Schedule(Err(error)) => {
                    self.error_message = Some(error);
                }
                UiEvent::CompletedNotice(filename) => {
                    self.completed_notice = Some(filename);
                }
                UiEvent::Status(Ok(message)) => {
                    self.error_message = None;
                    self.status_message = message;
                }
                UiEvent::Status(Err(error)) => {
                    self.status_message = "Engine request failed".to_owned();
                    self.error_message = Some(error);
                }
                UiEvent::EngineConnection(Ok(())) => {
                    if self.status_message == "Connecting to RustIDM engine..."
                        || self.status_message.starts_with("Cannot read downloads")
                    {
                        self.status_message = "Ready".to_owned();
                        self.error_message = None;
                    }
                }
                UiEvent::EngineConnection(Err(error)) => {
                    self.status_message = "Engine unavailable".to_owned();
                    self.error_message = Some(error);
                }
            }
            ctx.request_repaint();
        }
    }

    fn draw_menu_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("idm_menu_bar")
            .exact_height(24.0)
            .frame(
                egui::Frame::new()
                    .fill(Color32::WHITE)
                    .inner_margin(egui::Margin::symmetric(6, 2)),
            )
            .show(ctx, |ui| {
                egui::menu::bar(ui, |ui| {
                    ui.menu_button("Tasks", |ui| {
                        if ui.button("Add new download...").clicked() {
                            self.show_add_dialog = true;
                            ui.close_menu();
                        }
                        if ui.button("Add batch download (Wildcard [01-99])...").clicked() {
                            self.show_batch_dialog = true;
                            ui.close_menu();
                        }
                        if ui.button("Run Site Grabber (Bounded Crawler)...").clicked() {
                            self.show_grabber_dialog = true;
                            ui.close_menu();
                        }
                        if ui.button("Inspect Remote ZIP Archive (HTTP Range)...").clicked() {
                            self.show_zip_preview_dialog = true;
                            ui.close_menu();
                        }
                        ui.separator();
                        if ui.button("Export download list (.ef2)...").clicked() {
                            let _ = self.commands.send(UiCommand::ExportList);
                            ui.close_menu();
                        }
                        if ui.button("Import download list (.ef2 / .txt)...").clicked() {
                            self.show_import_export_dialog = true;
                            ui.close_menu();
                        }
                        ui.separator();
                        if ui.button("Stop All").clicked() {
                            self.stop_all_downloads();
                            ui.close_menu();
                        }
                    });
                    ui.menu_button("File", |ui| {
                        if ui
                            .add_enabled(
                                self.has_selected(),
                                egui::Button::new("Download status & progress..."),
                            )
                            .clicked()
                        {
                            self.show_details = true;
                            ui.close_menu();
                        }
                        if ui
                            .add_enabled(
                                self.has_selected(),
                                egui::Button::new("Refresh download address (Expired URL)..."),
                            )
                            .clicked()
                        {
                            if let Some((task_id, url)) = self
                                .selected_download()
                                .map(|task| (task.task_id.clone(), task.url.clone()))
                            {
                                self.refresh_task_id = task_id;
                                self.refresh_new_url = url;
                                self.show_refresh_dialog = true;
                            }
                            ui.close_menu();
                        }
                        if ui
                            .add_enabled(
                                self.has_selected(),
                                egui::Button::new("Re-verify SHA-256 / MD5 / CRC32"),
                            )
                            .clicked()
                        {
                            if let Some(task) = self.selected_download() {
                                let _ = self
                                    .commands
                                    .send(UiCommand::ReverifyChecksum(task.task_id.clone()));
                            }
                            ui.close_menu();
                        }
                        ui.separator();
                        if ui
                            .add_enabled(self.has_selected(), egui::Button::new("Resume"))
                            .clicked()
                        {
                            self.control_selected("resume");
                            ui.close_menu();
                        }
                        if ui
                            .add_enabled(self.has_selected(), egui::Button::new("Stop"))
                            .clicked()
                        {
                            self.control_selected("pause");
                            ui.close_menu();
                        }
                        if ui
                            .add_enabled(self.has_selected(), egui::Button::new("Remove"))
                            .clicked()
                        {
                            self.remove_selected_download();
                            ui.close_menu();
                        }
                    });
                    ui.menu_button("Downloads", |ui| {
                        if ui.button("Pause all").clicked() {
                            self.stop_all_downloads();
                            ui.close_menu();
                        }
                        if ui.button("Start queue").clicked() {
                            self.start_queue_downloads();
                            ui.close_menu();
                        }
                        if ui.button("Stop queue").clicked() {
                            self.stop_all_downloads();
                            ui.close_menu();
                        }
                        ui.separator();
                        if ui.button("Scheduler & Queues...").clicked() {
                            self.show_scheduler_dialog = true;
                            ui.close_menu();
                        }
                        if ui.button("Speed Limiter...").clicked() {
                            self.options_tab = OptionsTab::Connection;
                            self.show_options_dialog = true;
                            ui.close_menu();
                        }
                        if ui.button("Options...").clicked() {
                            self.show_options_dialog = true;
                            ui.close_menu();
                        }
                    });
                    ui.menu_button("View", |ui| {
                        if ui
                            .checkbox(&mut self.show_categories_panel, "Categories Tree")
                            .clicked()
                        {
                            ui.close_menu();
                        }
                        if ui
                            .checkbox(
                                &mut self.show_bottom_inspector,
                                "Bottom Segment & Telemetry Inspector",
                            )
                            .clicked()
                        {
                            ui.close_menu();
                        }
                        ui.separator();
                        if ui.button("All Downloads").clicked() {
                            self.filter = DownloadFilter::All;
                            ui.close_menu();
                        }
                        if ui.button("Unfinished").clicked() {
                            self.filter = DownloadFilter::Unfinished;
                            ui.close_menu();
                        }
                        if ui.button("Finished").clicked() {
                            self.filter = DownloadFilter::Finished;
                            ui.close_menu();
                        }
                    });
                    ui.menu_button("Help", |ui| {
                        ui.label("RustIDM v2.0.0 — Production Desktop Edition");
                        ui.label("Dynamic In-Half Segmentation | HLS AES-128 | MPEG-DASH | Remote ZIP");
                    });

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if !self.search_query.is_empty() && ui.small_button("×").clicked() {
                            self.search_query.clear();
                        }
                        ui.add(
                            egui::TextEdit::singleline(&mut self.search_query)
                                .hint_text("Search files, URLs, queues...")
                                .desired_width(190.0),
                        );
                        ui.label(RichText::new("Filter:").size(11.5).color(Color32::DARK_GRAY));
                    });
                });
            });
    }

    fn draw_toolbar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("idm_toolbar")
            .exact_height(68.0)
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(244, 245, 247))
                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(216, 218, 222)))
                    .inner_margin(egui::Margin::symmetric(6, 3)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing = Vec2::new(2.0, 0.0);

                    let can_resume = self.selected_download().is_some_and(|t| {
                        matches!(
                            t.status,
                            DownloadStatus::Paused
                                | DownloadStatus::Failed
                                | DownloadStatus::Queued
                                | DownloadStatus::NeedsUrlRefresh
                        )
                    });
                    let can_stop = self.selected_download().is_some_and(|t| {
                        matches!(
                            t.status,
                            DownloadStatus::Downloading
                                | DownloadStatus::Queued
                                | DownloadStatus::RefreshingUrl
                        )
                    });
                    let has_sel = self.has_selected();

                    if idm_toolbar_button(ui, ToolbarIcon::AddUrl, "Add URL", true, false) {
                        self.show_add_dialog = true;
                    }
                    if idm_toolbar_button(ui, ToolbarIcon::Batch, "Batch DL", true, false) {
                        self.show_batch_dialog = true;
                    }
                    if idm_toolbar_button(ui, ToolbarIcon::Resume, "Resume", can_resume, false) {
                        self.control_selected("resume");
                    }
                    if idm_toolbar_button(ui, ToolbarIcon::Stop, "Stop", can_stop, false) {
                        self.control_selected("pause");
                    }
                    if idm_toolbar_button(ui, ToolbarIcon::StopAll, "Stop All", true, false) {
                        self.stop_all_downloads();
                    }
                    if idm_toolbar_button(ui, ToolbarIcon::Delete, "Delete", has_sel, false) {
                        self.remove_selected_download();
                    }
                    if idm_toolbar_button(
                        ui,
                        ToolbarIcon::DeleteCompleted,
                        "Del Comp...",
                        true,
                        false,
                    ) {
                        self.delete_completed_downloads();
                    }
                    if idm_toolbar_button(ui, ToolbarIcon::Options, "Options", true, false) {
                        self.show_options_dialog = true;
                    }
                    if idm_toolbar_button(ui, ToolbarIcon::Scheduler, "Scheduler", true, false) {
                        self.show_scheduler_dialog = true;
                    }
                    if idm_toolbar_button(ui, ToolbarIcon::StartQueue, "Start Qu...", true, true) {
                        self.start_queue_downloads();
                    }
                    if idm_toolbar_button(ui, ToolbarIcon::StopQueue, "Stop Qu...", true, true) {
                        self.stop_all_downloads();
                    }
                    if idm_toolbar_button(ui, ToolbarIcon::Grabber, "Grabber", true, false) {
                        self.show_grabber_dialog = true;
                    }
                    if idm_toolbar_button(
                        ui,
                        ToolbarIcon::TellFriend,
                        "ZIP Insp...",
                        true,
                        false,
                    ) {
                        if let Some(url) = self.selected_download().map(|sel| sel.url.clone()) {
                            self.zip_preview_url = url;
                        }
                        self.show_zip_preview_dialog = true;
                    }
                });
            });
    }

    fn draw_status_bar(&self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("idm_status_bar")
            .exact_height(22.0)
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(240, 240, 240))
                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(210, 210, 210)))
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

                    ui.label(
                        RichText::new(format!(
                            "{} items  |  Transfer rate: {}",
                            self.downloads.len(),
                            format_speed(total_speed)
                        ))
                        .size(11.5)
                        .color(Color32::from_rgb(50, 50, 50)),
                    );

                    if let Some(err) = &self.error_message {
                        ui.separator();
                        ui.colored_label(Color32::DARK_RED, RichText::new(err).size(11.5));
                    }

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new(&self.status_message)
                                .size(11.5)
                                .color(Color32::from_rgb(70, 70, 70)),
                        );
                    });
                });
            });
    }

    fn draw_categories(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("idm_categories")
            .resizable(true)
            .default_width(172.0)
            .min_width(145.0)
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(240, 240, 240))
                    .inner_margin(egui::Margin::same(3)),
            )
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(Color32::WHITE)
                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(160, 160, 160)))
                    .show(ui, |ui| {
                        // Classic IDM "Categories [x]" header
                        egui::Frame::new()
                            .fill(Color32::from_rgb(245, 246, 248))
                            .stroke(Stroke::new(0.5_f32, Color32::from_rgb(210, 210, 210)))
                            .inner_margin(egui::Margin::symmetric(6, 2))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new("Categories")
                                            .size(12.0)
                                            .color(Color32::from_rgb(30, 30, 30)),
                                    );
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        if ui
                                            .add(
                                                egui::Button::new(
                                                    RichText::new("×").size(13.0).strong(),
                                                )
                                                .frame(false),
                                            )
                                            .clicked()
                                        {
                                            self.show_categories_panel = false;
                                        }
                                    });
                                });
                            });

                        egui::ScrollArea::vertical()
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                ui.add_space(4.0);
                                ui.spacing_mut().item_spacing = Vec2::new(0.0, 1.0);

                                // Root: All Downloads
                                if tree_root_row(
                                    ui,
                                    &mut self.expand_all_downloads,
                                    "All Downloads",
                                    TreeIconKind::FolderYellow,
                                    self.filter == DownloadFilter::All,
                                ) {
                                    self.filter = DownloadFilter::All;
                                }

                                if self.expand_all_downloads {
                                    for (cat, label, icon_kind) in [
                                        (
                                            FileCategory::Compressed,
                                            "Compressed",
                                            TreeIconKind::Compressed,
                                        ),
                                        (
                                            FileCategory::Documents,
                                            "Documents",
                                            TreeIconKind::Document,
                                        ),
                                        (FileCategory::Music, "Music", TreeIconKind::Music),
                                        (FileCategory::Programs, "Programs", TreeIconKind::Program),
                                        (FileCategory::Video, "Video", TreeIconKind::Video),
                                        (FileCategory::General, "General", TreeIconKind::Document),
                                    ] {
                                        let target = DownloadFilter::Category(cat);
                                        if tree_child_row(
                                            ui,
                                            label,
                                            icon_kind,
                                            self.filter == target,
                                        ) {
                                            self.filter = target;
                                        }
                                    }
                                }

                                // Unfinished
                                if tree_root_row(
                                    ui,
                                    &mut self.expand_unfinished,
                                    "Unfinished",
                                    TreeIconKind::FolderYellow,
                                    self.filter == DownloadFilter::Unfinished,
                                ) {
                                    self.filter = DownloadFilter::Unfinished;
                                }

                                // Finished
                                if tree_root_row(
                                    ui,
                                    &mut self.expand_finished,
                                    "Finished",
                                    TreeIconKind::FolderFinished,
                                    self.filter == DownloadFilter::Finished,
                                ) {
                                    self.filter = DownloadFilter::Finished;
                                }

                                // Queues Tree
                                if tree_root_row(
                                    ui,
                                    &mut self.expand_queues,
                                    "Queues",
                                    TreeIconKind::FolderYellow,
                                    matches!(self.filter, DownloadFilter::Queue(_)),
                                ) {
                                    self.filter = DownloadFilter::Queue("main".to_owned());
                                }

                                if self.expand_queues {
                                    let queue_names: Vec<String> = if self.queues.is_empty() {
                                        vec!["main".to_owned(), "sync".to_owned()]
                                    } else {
                                        self.queues.iter().map(|q| q.name.clone()).collect()
                                    };
                                    for q_name in queue_names {
                                        let label = format!("Queue: {}", q_name);
                                        let target = DownloadFilter::Queue(q_name);
                                        if tree_child_row(
                                            ui,
                                            &label,
                                            TreeIconKind::FolderYellow,
                                            self.filter == target,
                                        ) {
                                            self.filter = target;
                                        }
                                    }
                                }

                                ui.add_space(12.0);
                            });
                    });
            });
    }

    fn draw_download_table(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(240, 240, 240))
                    .inner_margin(egui::Margin::same(3)),
            )
            .show(ctx, |ui| {
                let downloads = self
                    .filtered_downloads()
                    .into_iter()
                    .cloned()
                    .collect::<Vec<_>>();

                egui::Frame::new()
                    .fill(Color32::WHITE)
                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(160, 160, 160)))
                    .show(ui, |ui| {
                        egui::ScrollArea::both()
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                let available = ui.available_width().max(760.0);
                                let columns = [
                                    ("File Name", available * 0.27, SortColumn::Filename),
                                    ("Q", 34.0, SortColumn::Queue),
                                    ("Size", available * 0.10, SortColumn::Size),
                                    ("Status", available * 0.12, SortColumn::Status),
                                    ("Time left", available * 0.10, SortColumn::TimeLeft),
                                    ("Transfer rate", available * 0.12, SortColumn::Speed),
                                    ("Last Try Date", available * 0.13, SortColumn::UpdatedAt),
                                    (
                                        "Description",
                                        (available * 0.16 - 34.0).max(90.0),
                                        SortColumn::Category,
                                    ),
                                ];

                                // Interactive Sortable Table Header
                                egui::Frame::new()
                                    .fill(Color32::from_rgb(247, 248, 250))
                                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(215, 215, 215)))
                                    .show(ui, |ui| {
                                        ui.spacing_mut().item_spacing = Vec2::ZERO;
                                        ui.horizontal(|ui| {
                                            for (title, width, col_id) in columns {
                                                let (rect, resp) = ui.allocate_exact_size(
                                                    Vec2::new(width, 23.0),
                                                    Sense::click(),
                                                );
                                                let painter = ui.painter();
                                                if resp.hovered() {
                                                    painter.rect_filled(
                                                        rect,
                                                        0.0,
                                                        Color32::from_rgb(232, 240, 250),
                                                    );
                                                }
                                                painter.line_segment(
                                                    [rect.right_top(), rect.right_bottom()],
                                                    Stroke::new(
                                                        1.0_f32,
                                                        Color32::from_rgb(220, 220, 220),
                                                    ),
                                                );
                                                let arrow = if self.sort_column == col_id {
                                                    if self.sort_ascending {
                                                        " ▲"
                                                    } else {
                                                        " ▼"
                                                    }
                                                } else {
                                                    ""
                                                };
                                                painter.text(
                                                    Pos2::new(rect.left() + 6.0, rect.center().y),
                                                    egui::Align2::LEFT_CENTER,
                                                    format!("{title}{arrow}"),
                                                    egui::FontId::proportional(12.0),
                                                    Color32::from_rgb(30, 30, 30),
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
                                    let selected =
                                        self.selected_task.as_deref() == Some(&task.task_id);
                                    let row_bg = if selected {
                                        Color32::from_rgb(205, 232, 255)
                                    } else if row_idx % 2 == 1 {
                                        Color32::from_rgb(250, 251, 253)
                                    } else {
                                        Color32::WHITE
                                    };

                                    let total_width: f32 = columns.iter().map(|(_, w, _)| *w).sum();
                                    let (row_rect, response) = ui.allocate_exact_size(
                                        Vec2::new(total_width, 22.0),
                                        Sense::click(),
                                    );

                                    let painter = ui.painter();
                                    painter.rect_filled(row_rect, 0.0, row_bg);
                                    painter.line_segment(
                                        [row_rect.left_bottom(), row_rect.right_bottom()],
                                        Stroke::new(1.0_f32, Color32::from_rgb(238, 238, 238)),
                                    );

                                    let mut x = row_rect.left();
                                    let cy = row_rect.center().y;

                                    // Col 0: File icon + File Name
                                    let w0 = columns[0].1;
                                    draw_file_category_icon(
                                        painter,
                                        Pos2::new(x + 11.0, cy),
                                        task.category,
                                    );
                                    painter.text(
                                        Pos2::new(x + 24.0, cy),
                                        egui::Align2::LEFT_CENTER,
                                        truncate_text(&task.filename, 36),
                                        egui::FontId::proportional(12.0),
                                        Color32::BLACK,
                                    );
                                    x += w0;

                                    // Col 1: Q (Queue badge)
                                    let w1 = columns[1].1;
                                    draw_queue_mini_icon(
                                        painter,
                                        Pos2::new(x + w1 * 0.5, cy),
                                        task.status == DownloadStatus::Completed,
                                    );
                                    x += w1;

                                    // Col 2: Size
                                    let w2 = columns[2].1;
                                    painter.text(
                                        Pos2::new(x + 6.0, cy),
                                        egui::Align2::LEFT_CENTER,
                                        format_bytes(task.total_size),
                                        egui::FontId::proportional(12.0),
                                        Color32::BLACK,
                                    );
                                    x += w2;

                                    // Col 3: Status
                                    let w3 = columns[3].1;
                                    let status_str = table_status_text(task);
                                    let status_col = match task.status {
                                        DownloadStatus::Completed => Color32::from_rgb(16, 118, 36),
                                        DownloadStatus::Downloading => {
                                            Color32::from_rgb(0, 72, 186)
                                        }
                                        DownloadStatus::Failed
                                        | DownloadStatus::NeedsUrlRefresh => Color32::DARK_RED,
                                        _ => Color32::BLACK,
                                    };
                                    painter.text(
                                        Pos2::new(x + 6.0, cy),
                                        egui::Align2::LEFT_CENTER,
                                        status_str,
                                        egui::FontId::proportional(12.0),
                                        status_col,
                                    );
                                    x += w3;

                                    // Col 4: Time left
                                    let w4 = columns[4].1;
                                    painter.text(
                                        Pos2::new(x + 6.0, cy),
                                        egui::Align2::LEFT_CENTER,
                                        time_left(task),
                                        egui::FontId::proportional(12.0),
                                        Color32::BLACK,
                                    );
                                    x += w4;

                                    // Col 5: Transfer rate
                                    let w5 = columns[5].1;
                                    let speed: u64 =
                                        task.segments.iter().map(|s| s.last_speed_bps).sum();
                                    let speed_str = if speed > 0
                                        && task.status == DownloadStatus::Downloading
                                    {
                                        format_speed(speed)
                                    } else {
                                        String::new()
                                    };
                                    painter.text(
                                        Pos2::new(x + 6.0, cy),
                                        egui::Align2::LEFT_CENTER,
                                        speed_str,
                                        egui::FontId::proportional(12.0),
                                        Color32::BLACK,
                                    );
                                    x += w5;

                                    // Col 6: Last Try Date (Real localized timestamp)
                                    let w6 = columns[6].1;
                                    let date_str = format_last_try_date(task.updated_at);
                                    painter.text(
                                        Pos2::new(x + 6.0, cy),
                                        egui::Align2::LEFT_CENTER,
                                        date_str,
                                        egui::FontId::proportional(11.5),
                                        Color32::from_rgb(40, 40, 40),
                                    );
                                    x += w6;

                                    // Col 7: Description
                                    let desc = task
                                        .description
                                        .as_deref()
                                        .filter(|s| !s.is_empty())
                                        .unwrap_or_else(|| category_label(task.category));
                                    painter.text(
                                        Pos2::new(x + 6.0, cy),
                                        egui::Align2::LEFT_CENTER,
                                        truncate_text(desc, 26),
                                        egui::FontId::proportional(11.5),
                                        Color32::from_rgb(70, 70, 70),
                                    );

                                    // Vertical grid lines
                                    let mut gx = row_rect.left();
                                    for (_, cw, _) in columns {
                                        gx += cw;
                                        painter.line_segment(
                                            [
                                                Pos2::new(gx, row_rect.top()),
                                                Pos2::new(gx, row_rect.bottom()),
                                            ],
                                            Stroke::new(1.0_f32, Color32::from_rgb(240, 240, 240)),
                                        );
                                    }

                                    if response.clicked() {
                                        self.selected_task = Some(task.task_id.clone());
                                    }
                                    if response.double_clicked() {
                                        self.selected_task = Some(task.task_id.clone());
                                        self.show_details = true;
                                    }

                                    // Right-click context menu on every download row
                                    response.context_menu(|ui| {
                                        self.selected_task = Some(task.task_id.clone());
                                        ui.label(
                                            RichText::new(truncate_text(&task.filename, 32))
                                                .strong(),
                                        );
                                        ui.separator();
                                        if ui.button("Open Progress & Connections Window").clicked()
                                        {
                                            self.show_details = true;
                                            ui.close_menu();
                                        }
                                        if ui.button("Resume Download").clicked() {
                                            let _ = self.commands.send(UiCommand::Control {
                                                task_id: task.task_id.clone(),
                                                action: "resume",
                                            });
                                            ui.close_menu();
                                        }
                                        if ui.button("Pause / Stop Download").clicked() {
                                            let _ = self.commands.send(UiCommand::Control {
                                                task_id: task.task_id.clone(),
                                                action: "pause",
                                            });
                                            ui.close_menu();
                                        }
                                        if ui
                                            .button("Refresh Download Address (Expired Link)...")
                                            .clicked()
                                        {
                                            self.refresh_task_id = task.task_id.clone();
                                            self.refresh_new_url = task.url.clone();
                                            self.show_refresh_dialog = true;
                                            ui.close_menu();
                                        }
                                        if task.filename.to_lowercase().ends_with(".zip")
                                            && ui
                                                .button("Preview Remote ZIP Contents...")
                                                .clicked()
                                        {
                                            self.zip_preview_url = task.url.clone();
                                            self.show_zip_preview_dialog = true;
                                            ui.close_menu();
                                        }
                                        if task.status == DownloadStatus::Completed
                                            && ui
                                                .button("Verify Integrity (SHA-256 / MD5 / CRC32)")
                                                .clicked()
                                        {
                                            let _ = self.commands.send(
                                                UiCommand::ReverifyChecksum(task.task_id.clone()),
                                            );
                                            ui.close_menu();
                                        }
                                        ui.separator();
                                        if ui.button("Copy Download URL").clicked() {
                                            ui.ctx().copy_text(task.url.clone());
                                            self.status_message =
                                                "Download URL copied to clipboard".to_owned();
                                            ui.close_menu();
                                        }
                                        if ui.button("Copy Local File Path").clicked() {
                                            ui.ctx().copy_text(
                                                task.final_file_path().display().to_string(),
                                            );
                                            self.status_message =
                                                "File path copied to clipboard".to_owned();
                                            ui.close_menu();
                                        }
                                        ui.separator();
                                        if ui.button("Remove Task Record").clicked() {
                                            let _ = self.commands.send(UiCommand::RemoveDownload(
                                                task.task_id.clone(),
                                            ));
                                            ui.close_menu();
                                        }
                                    });
                                }
                            });
                    });
            });
    }

    fn draw_bottom_inspector(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("idm_bottom_inspector")
            .resizable(true)
            .default_height(168.0)
            .min_height(115.0)
            .max_height(300.0)
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(245, 246, 248))
                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(195, 198, 204)))
                    .inner_margin(egui::Margin::symmetric(8, 5)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    for (tab, label) in [
                        (BottomInspectorTab::SegmentMap, "Dynamic Segment Map"),
                        (BottomInspectorTab::SpeedGraph, "Live Transfer Rate Curve"),
                        (BottomInspectorTab::ConnectionLogs, "Worker & HTTP Logs"),
                        (
                            BottomInspectorTab::ChecksumAudit,
                            "Digests & Mirror Failover",
                        ),
                    ] {
                        let selected = self.bottom_inspector_tab == tab;
                        if ui.selectable_label(selected, label).clicked() {
                            self.bottom_inspector_tab = tab;
                        }
                    }

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.small_button("×").clicked() {
                            self.show_bottom_inspector = false;
                        }
                        if let Some(stats) = &self.engine_stats {
                            ui.label(
                                RichText::new(format!(
                                    "Active: {} | Completed: {} | Peak Rate: {}",
                                    stats.active_downloads,
                                    stats.completed_downloads,
                                    format_speed(stats.peak_aggregate_speed_bps)
                                ))
                                .size(11.5)
                                .color(Color32::from_rgb(45, 75, 120)),
                            );
                        }
                    });
                });
                ui.separator();

                let Some(task) = self.selected_download().cloned() else {
                    ui.centered_and_justified(|ui| {
                        ui.label(
                            RichText::new(
                                "Select a download task above to inspect its real-time byte segments, transfer rate curve, worker logs, and SHA-256/MD5/CRC32 digests.",
                            )
                            .color(Color32::GRAY),
                        );
                    });
                    return;
                };

                match self.bottom_inspector_tab {
                    BottomInspectorTab::SegmentMap => {
                        let progress = if task.total_size == 0 {
                            0.0
                        } else {
                            (task.total_downloaded() as f32 / task.total_size as f32)
                                .clamp(0.0, 1.0)
                        };
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(format!(
                                    "File: {}  |  Total Size: {}  |  Segments: {}  |  Queue: {}",
                                    truncate_text(&task.filename, 38),
                                    format_bytes(task.total_size),
                                    task.segments.len(),
                                    task.queue_name
                                ))
                                .size(11.5)
                                .strong(),
                            );
                        });
                        ui.add_space(3.0);
                        draw_idm_connections_bar(ui, &task, progress);
                        ui.add_space(4.0);
                        egui::ScrollArea::vertical()
                            .max_height(82.0)
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                ui.horizontal_wrapped(|ui| {
                                    for seg in &task.segments {
                                        let badge_col = match seg.status {
                                            SegmentStatus::Completed => {
                                                Color32::from_rgb(212, 244, 220)
                                            }
                                            SegmentStatus::Downloading => {
                                                Color32::from_rgb(210, 232, 255)
                                            }
                                            SegmentStatus::Failed => {
                                                Color32::from_rgb(255, 220, 220)
                                            }
                                            _ => Color32::from_rgb(235, 235, 235),
                                        };
                                        egui::Frame::new()
                                            .fill(badge_col)
                                            .stroke(Stroke::new(
                                                1.0_f32,
                                                Color32::from_rgb(175, 185, 198),
                                            ))
                                            .inner_margin(egui::Margin::symmetric(6, 3))
                                            .show(ui, |ui| {
                                                ui.label(
                                                    RichText::new(format!(
                                                        "#{} [{}..{}] {} ({} splits)",
                                                        seg.id + 1,
                                                        format_bytes(seg.start_byte),
                                                        if seg.end_byte == u64::MAX {
                                                            "EOF".to_owned()
                                                        } else {
                                                            format_bytes(seg.end_byte)
                                                        },
                                                        format_speed(seg.last_speed_bps),
                                                        seg.split_count
                                                    ))
                                                    .size(11.0),
                                                );
                                            });
                                    }
                                });
                            });
                    }
                    BottomInspectorTab::SpeedGraph => {
                        draw_live_speed_graph(ui, &task);
                    }
                    BottomInspectorTab::ConnectionLogs => {
                        egui::ScrollArea::vertical()
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                if task.connection_logs.is_empty() {
                                    ui.label("No diagnostic logs recorded for this task yet.");
                                }
                                for entry in task.connection_logs.iter().rev() {
                                    let lvl_color = match entry.level.as_str() {
                                        "ERROR" => Color32::DARK_RED,
                                        "WARN" => Color32::from_rgb(180, 110, 0),
                                        _ => Color32::from_rgb(20, 85, 155),
                                    };
                                    ui.horizontal(|ui| {
                                        ui.label(
                                            RichText::new(
                                                entry
                                                    .timestamp
                                                    .with_timezone(&chrono::Local)
                                                    .format("%H:%M:%S")
                                                    .to_string(),
                                            )
                                            .monospace()
                                            .size(11.0)
                                            .color(Color32::DARK_GRAY),
                                        );
                                        ui.colored_label(
                                            lvl_color,
                                            RichText::new(format!("[{}]", entry.level))
                                                .monospace()
                                                .size(11.0)
                                                .strong(),
                                        );
                                        if let Some(sid) = entry.segment_id {
                                            ui.label(
                                                RichText::new(format!("(Conn #{})", sid + 1))
                                                    .monospace()
                                                    .size(11.0),
                                            );
                                        }
                                        ui.label(RichText::new(&entry.message).size(11.5));
                                    });
                                }
                            });
                    }
                    BottomInspectorTab::ChecksumAudit => {
                        ui.horizontal(|ui| {
                            ui.vertical(|ui| {
                                ui.label(
                                    RichText::new(format!(
                                        "SHA-256: {}",
                                        task.computed_sha256
                                            .as_deref()
                                            .unwrap_or("Pending completion / verification")
                                    ))
                                    .monospace()
                                    .size(11.5),
                                );
                                ui.label(
                                    RichText::new(format!(
                                        "MD5:     {}",
                                        task.computed_md5
                                            .as_deref()
                                            .unwrap_or("Pending completion / verification")
                                    ))
                                    .monospace()
                                    .size(11.5),
                                );
                                ui.label(
                                    RichText::new(format!(
                                        "CRC32:   {}",
                                        task.computed_crc32
                                            .as_deref()
                                            .unwrap_or("Pending completion / verification")
                                    ))
                                    .monospace()
                                    .size(11.5),
                                );
                            });
                            ui.separator();
                            ui.vertical(|ui| {
                                ui.label(
                                    RichText::new(format!(
                                        "ETag: {}  |  Resumable: {}",
                                        task.etag.as_deref().unwrap_or("None"),
                                        if task.resumable { "Yes (RFC 7233)" } else { "No" }
                                    ))
                                    .size(11.5),
                                );
                                ui.label(
                                    RichText::new(format!(
                                        "Active URL: {} (Mirrors configured: {})",
                                        truncate_text(&task.current_effective_url(), 54),
                                        task.mirror_urls.len()
                                    ))
                                    .size(11.5),
                                );
                                if task.status == DownloadStatus::Completed
                                    && ui.button("Re-verify File Digests on Disk").clicked()
                                {
                                    let _ = self
                                        .commands
                                        .send(UiCommand::ReverifyChecksum(task.task_id.clone()));
                                }
                            });
                        });
                    }
                }
            });
    }

    fn draw_dialogs(&mut self, ctx: &egui::Context) {
        // 1. Advanced Add URL Dialog
        if self.show_add_dialog {
            let mut open = self.show_add_dialog;
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
                            ui.add(
                                egui::TextEdit::singleline(&mut self.new_url)
                                    .hint_text("https://example.com/file.zip")
                                    .desired_width(410.0),
                            );
                            ui.end_row();

                            ui.label("Save Filename:");
                            ui.add(
                                egui::TextEdit::singleline(&mut self.new_filename)
                                    .hint_text("Optional custom filename (auto-detected if blank)")
                                    .desired_width(410.0),
                            );
                            ui.end_row();

                            ui.label("Target Queue:");
                            egui::ComboBox::from_id_salt("add_url_queue_combo")
                                .selected_text(&self.new_queue_selection)
                                .show_ui(ui, |ui| {
                                    let q_names: Vec<String> = if self.queues.is_empty() {
                                        vec!["main".to_owned(), "sync".to_owned()]
                                    } else {
                                        self.queues.iter().map(|q| q.name.clone()).collect()
                                    };
                                    for q in q_names {
                                        ui.selectable_value(
                                            &mut self.new_queue_selection,
                                            q.clone(),
                                            q,
                                        );
                                    }
                                });
                            ui.end_row();

                            ui.label("Description:");
                            ui.add(
                                egui::TextEdit::singleline(&mut self.new_description)
                                    .hint_text("Optional note or category description")
                                    .desired_width(410.0),
                            );
                            ui.end_row();
                        });

                    ui.add_space(4.0);
                    ui.checkbox(
                        &mut self.show_add_advanced,
                        "Show Advanced Options (Headers, Cookies, Expected Digests & Mirror URLs)",
                    );

                    if self.show_add_advanced {
                        ui.separator();
                        egui::Grid::new("add_url_adv_grid")
                            .num_columns(2)
                            .spacing([10.0, 5.0])
                            .show(ui, |ui| {
                                ui.label("Referer URL:");
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.new_referer)
                                        .hint_text("https://source-page.example.com")
                                        .desired_width(390.0),
                                );
                                ui.end_row();

                                ui.label("Cookies Header:");
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.new_cookies)
                                        .hint_text("session_id=xyz; token=abc")
                                        .desired_width(390.0),
                                );
                                ui.end_row();

                                ui.label("User-Agent:");
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.new_user_agent)
                                        .hint_text("Custom User-Agent header")
                                        .desired_width(390.0),
                                );
                                ui.end_row();

                                ui.label("Expected SHA-256:");
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.new_expected_sha256)
                                        .hint_text("64-char hex SHA-256 digest")
                                        .desired_width(390.0),
                                );
                                ui.end_row();

                                ui.label("Expected MD5:");
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.new_expected_md5)
                                        .hint_text("32-char hex MD5 digest")
                                        .desired_width(390.0),
                                );
                                ui.end_row();

                                ui.label("Expected CRC32:");
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.new_expected_crc32)
                                        .hint_text("8-char hex CRC32 (e.g. cbf43926)")
                                        .desired_width(390.0),
                                );
                                ui.end_row();

                                ui.label("Mirror URLs:");
                                ui.add(
                                    egui::TextEdit::multiline(&mut self.new_mirrors_text)
                                        .hint_text("One fallback mirror URL per line")
                                        .desired_rows(2)
                                        .desired_width(390.0),
                                );
                                ui.end_row();
                            });
                    }

                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if ui.button("Download Now (OK)").clicked() {
                            let url = self.new_url.trim().to_owned();
                            if !url.is_empty() {
                                let mirrors: Vec<String> = self
                                    .new_mirrors_text
                                    .lines()
                                    .map(|s| s.trim().to_owned())
                                    .filter(|s| !s.is_empty())
                                    .collect();
                                let payload = InterceptDownloadPayload {
                                    url,
                                    source_page_url: opt_non_empty(&self.new_referer),
                                    filename: opt_non_empty(&self.new_filename),
                                    cookies: opt_non_empty(&self.new_cookies),
                                    referer: opt_non_empty(&self.new_referer),
                                    user_agent: opt_non_empty(&self.new_user_agent),
                                    queue_name: Some(self.new_queue_selection.clone()),
                                    expected_sha256: opt_non_empty(&self.new_expected_sha256),
                                    expected_md5: opt_non_empty(&self.new_expected_md5),
                                    expected_crc32: opt_non_empty(&self.new_expected_crc32),
                                    mirror_urls: if mirrors.is_empty() {
                                        None
                                    } else {
                                        Some(mirrors)
                                    },
                                    description: opt_non_empty(&self.new_description),
                                };
                                match self.commands.send(UiCommand::AddDownload(payload)) {
                                    Ok(()) => {
                                        self.status_message = "Adding download...".to_owned();
                                        self.new_url.clear();
                                        self.new_filename.clear();
                                        submitted = true;
                                    }
                                    Err(error) => self.error_message = Some(error.to_string()),
                                }
                            }
                        }
                        if ui.button("Preview Remote ZIP").clicked() {
                            let url = self.new_url.trim().to_owned();
                            if !url.is_empty() {
                                self.zip_preview_url = url;
                                self.show_zip_preview_dialog = true;
                            }
                        }
                        if ui.button("Cancel").clicked() {
                            submitted = true;
                        }
                    });
                });
            self.show_add_dialog = open && !submitted;
        }

        // 2. Batch Wildcard Download Dialog ([01-99] / [a-z] / Multi-line)
        if self.show_batch_dialog {
            let mut open = self.show_batch_dialog;
            egui::Window::new("Add Batch Download (Wildcard Pattern Expander)")
                .collapsible(false)
                .resizable(true)
                .default_width(550.0)
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.label(
                        "Enter a wildcard pattern (e.g. https://cdn.example.com/part_[01-12].zip or [a-f]) or paste multiple URLs (one per line):",
                    );
                    ui.add(
                        egui::TextEdit::multiline(&mut self.batch_pattern_input)
                            .desired_rows(4)
                            .desired_width(520.0),
                    );
                    ui.horizontal(|ui| {
                        ui.label("Step increment:");
                        ui.add(egui::DragValue::new(&mut self.batch_step_input).range(1..=100));
                        ui.separator();
                        ui.label("Target Queue:");
                        egui::ComboBox::from_id_salt("batch_queue_combo")
                            .selected_text(&self.batch_queue_selection)
                            .show_ui(ui, |ui| {
                                let q_names: Vec<String> = if self.queues.is_empty() {
                                    vec!["main".to_owned(), "sync".to_owned()]
                                } else {
                                    self.queues.iter().map(|q| q.name.clone()).collect()
                                };
                                for q in q_names {
                                    ui.selectable_value(
                                        &mut self.batch_queue_selection,
                                        q.clone(),
                                        q,
                                    );
                                }
                            });
                        ui.checkbox(&mut self.batch_start_immediately, "Start immediately");
                    });

                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        if ui.button("Expand & Enqueue Batch").clicked() {
                            let req = BatchExpandRequest {
                                pattern_or_urls: self.batch_pattern_input.clone(),
                                step: Some(self.batch_step_input.max(1)),
                                queue_name: Some(self.batch_queue_selection.clone()),
                                cookies: None,
                                referer: None,
                                start_immediately: Some(self.batch_start_immediately),
                            };
                            let _ = self.commands.send(UiCommand::AddBatch(req));
                            self.status_message = "Expanding batch pattern...".to_owned();
                        }
                        if ui.button("Clear").clicked() {
                            self.batch_pattern_input.clear();
                            self.batch_last_response = None;
                        }
                    });

                    if let Some(resp) = &self.batch_last_response {
                        ui.separator();
                        ui.label(
                            RichText::new(format!(
                                "Expanded {} URLs | Queued: {} | Skipped duplicates: {}",
                                resp.expanded_urls.len(),
                                resp.queued_task_ids.len(),
                                resp.skipped_duplicates
                            ))
                            .strong()
                            .color(Color32::from_rgb(16, 118, 36)),
                        );
                        egui::ScrollArea::vertical()
                            .max_height(130.0)
                            .show(ui, |ui| {
                                for (idx, u) in resp.expanded_urls.iter().enumerate() {
                                    ui.label(
                                        RichText::new(format!("{}. {}", idx + 1, u))
                                            .monospace()
                                            .size(11.0),
                                    );
                                }
                                for err in &resp.errors {
                                    ui.colored_label(Color32::DARK_RED, err);
                                }
                            });
                    }
                });
            self.show_batch_dialog = open;
        }

        // 3. Refresh Expired URL Dialog
        if self.show_refresh_dialog {
            let mut open = self.show_refresh_dialog;
            let mut submitted = false;
            egui::Window::new("Refresh Expired Download Address (Preserve Partial Bytes)")
                .collapsible(false)
                .resizable(false)
                .default_width(510.0)
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.label(
                        "Provide the fresh pre-signed URL or updated session cookies. RustIDM will verify Content-Length and strong ETag before resuming:",
                    );
                    egui::Grid::new("refresh_url_grid")
                        .num_columns(2)
                        .spacing([10.0, 6.0])
                        .show(ui, |ui| {
                            ui.label("Task ID:");
                            ui.label(RichText::new(&self.refresh_task_id).monospace());
                            ui.end_row();

                            ui.label("New URL:");
                            ui.add(
                                egui::TextEdit::singleline(&mut self.refresh_new_url)
                                    .desired_width(380.0),
                            );
                            ui.end_row();

                            ui.label("Updated Cookies:");
                            ui.add(
                                egui::TextEdit::singleline(&mut self.refresh_new_cookies)
                                    .hint_text("Optional new Cookie header")
                                    .desired_width(380.0),
                            );
                            ui.end_row();
                        });
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if ui.button("Verify & Resume Download").clicked() {
                            let payload = RefreshUrlPayload {
                                task_id: self.refresh_task_id.clone(),
                                new_url: self.refresh_new_url.trim().to_owned(),
                                cookies: opt_non_empty(&self.refresh_new_cookies),
                            };
                            let _ = self.commands.send(UiCommand::RefreshUrl(payload));
                            self.status_message = "Verifying refreshed URL...".to_owned();
                            submitted = true;
                        }
                        if ui.button("Cancel").clicked() {
                            submitted = true;
                        }
                    });
                });
            self.show_refresh_dialog = open && !submitted;
        }

        // 4. Site Grabber (Bounded BFS Crawler) Wizard Window
        if self.show_grabber_dialog {
            let mut open = self.show_grabber_dialog;
            egui::Window::new("IDM Site Grabber — Bounded BFS Web Crawler")
                .collapsible(false)
                .resizable(true)
                .default_width(600.0)
                .open(&mut open)
                .show(ctx, |ui| {
                    egui::Grid::new("grabber_cfg_grid")
                        .num_columns(2)
                        .spacing([10.0, 6.0])
                        .show(ui, |ui| {
                            ui.label("Start Page URL:");
                            ui.add(
                                egui::TextEdit::singleline(&mut self.crawl_config.start_url)
                                    .hint_text("https://example.com/archive/")
                                    .desired_width(430.0),
                            );
                            ui.end_row();

                            ui.label("Allowed Extensions:");
                            ui.add(
                                egui::TextEdit::singleline(&mut self.crawl_extensions_csv)
                                    .hint_text("mp4,mkv,pdf,zip,rar,iso")
                                    .desired_width(430.0),
                            );
                            ui.end_row();

                            ui.label("Excluded Paths:");
                            ui.add(
                                egui::TextEdit::singleline(&mut self.crawl_excluded_csv)
                                    .hint_text("/logout,/admin")
                                    .desired_width(430.0),
                            );
                            ui.end_row();
                        });

                    ui.horizontal(|ui| {
                        ui.label("Max Depth:");
                        ui.add(
                            egui::DragValue::new(&mut self.crawl_config.max_depth).range(0..=5),
                        );
                        ui.separator();
                        ui.label("Max Pages:");
                        ui.add(
                            egui::DragValue::new(&mut self.crawl_config.max_pages).range(1..=200),
                        );
                        ui.separator();
                        ui.checkbox(
                            &mut self.crawl_config.same_domain_only,
                            "Stay on same domain only",
                        );
                    });

                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        let can_start =
                            !self.crawl_in_progress && !self.crawl_config.start_url.trim().is_empty();
                        if ui
                            .add_enabled(can_start, egui::Button::new("Start Bounded Crawl"))
                            .clicked()
                        {
                            self.crawl_config.allowed_extensions = self
                                .crawl_extensions_csv
                                .split(',')
                                .map(|s| s.trim().trim_start_matches('.').to_owned())
                                .filter(|s| !s.is_empty())
                                .collect();
                            self.crawl_config.excluded_paths = self
                                .crawl_excluded_csv
                                .split(',')
                                .map(|s| s.trim().to_owned())
                                .filter(|s| !s.is_empty())
                                .collect();
                            self.crawl_in_progress = true;
                            let _ = self
                                .commands
                                .send(UiCommand::CrawlSite(self.crawl_config.clone()));
                            self.status_message = "Crawling site with BFS bounds...".to_owned();
                        }
                        if self.crawl_in_progress {
                            ui.label("Crawling in progress...");
                        }
                    });

                    if let Some(report) = self.crawl_report.clone() {
                        ui.separator();
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(format!(
                                    "Visited {} pages | Discovered {} matched files",
                                    report.pages_visited,
                                    report.matched_files.len()
                                ))
                                .strong(),
                            );
                            if ui.small_button("Select All").clicked() {
                                for link in &report.matched_files {
                                    self.crawl_selected_links.insert(link.clone(), true);
                                }
                            }
                            if ui.small_button("Deselect All").clicked() {
                                for link in &report.matched_files {
                                    self.crawl_selected_links.insert(link.clone(), false);
                                }
                            }
                            if ui.button("Queue Selected Files").clicked() {
                                let selected: Vec<String> = report
                                    .matched_files
                                    .iter()
                                    .filter(|u| {
                                        self.crawl_selected_links.get(*u).copied().unwrap_or(false)
                                    })
                                    .cloned()
                                    .collect();
                                if !selected.is_empty() {
                                    let req = BatchExpandRequest {
                                        pattern_or_urls: selected.join("\n"),
                                        step: Some(1),
                                        queue_name: Some("main".to_owned()),
                                        cookies: None,
                                        referer: Some(self.crawl_config.start_url.clone()),
                                        start_immediately: Some(true),
                                    };
                                    let _ = self.commands.send(UiCommand::AddBatch(req));
                                }
                            }
                        });

                        egui::ScrollArea::vertical()
                            .max_height(180.0)
                            .show(ui, |ui| {
                                for link in &report.matched_files {
                                    let checked = self
                                        .crawl_selected_links
                                        .entry(link.clone())
                                        .or_insert(true);
                                    ui.checkbox(checked, link);
                                }
                                for err in &report.errors {
                                    ui.colored_label(Color32::DARK_RED, err);
                                }
                            });
                    }
                });
            self.show_grabber_dialog = open;
        }

        // 5. Remote ZIP Central Directory Inspector Dialog
        if self.show_zip_preview_dialog {
            let mut open = self.show_zip_preview_dialog;
            egui::Window::new("Remote ZIP Archive Inspector (HTTP Range EOCD Reader)")
                .collapsible(false)
                .resizable(true)
                .default_width(620.0)
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.label(
                        "Inspect the internal files and CRC32 digests of a remote .zip archive by reading only its final 64 KB Central Directory:",
                    );
                    ui.horizontal(|ui| {
                        ui.label("Remote ZIP URL:");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.zip_preview_url)
                                .hint_text("https://example.com/release.zip")
                                .desired_width(380.0),
                        );
                        if ui
                            .add_enabled(
                                !self.zip_preview_loading && !self.zip_preview_url.trim().is_empty(),
                                egui::Button::new("Inspect ZIP"),
                            )
                            .clicked()
                        {
                            self.zip_preview_loading = true;
                            let _ = self.commands.send(UiCommand::PreviewZip(
                                ZipPreviewRequestPayload {
                                    url: self.zip_preview_url.trim().to_owned(),
                                    cookies: opt_non_empty(&self.zip_preview_cookies),
                                    referer: None,
                                    user_agent: None,
                                },
                            ));
                        }
                    });

                    if let Some(preview) = &self.zip_preview_data {
                        ui.separator();
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(format!(
                                    "Total Entries: {}  |  Archive Size: {}  |  Uncompressed Total: {}",
                                    preview.total_entries,
                                    format_bytes(preview.archive_total_size),
                                    format_bytes(preview.total_uncompressed_bytes)
                                ))
                                .strong(),
                            );
                            if ui.button("Download Full ZIP Archive").clicked() {
                                let _ = self.commands.send(UiCommand::AddDownload(
                                    InterceptDownloadPayload {
                                        url: preview.url.clone(),
                                        queue_name: Some("main".to_owned()),
                                        ..Default::default()
                                    },
                                ));
                            }
                        });

                        egui::ScrollArea::vertical()
                            .max_height(210.0)
                            .show(ui, |ui| {
                                egui::Grid::new("remote_zip_entries_grid")
                                    .striped(true)
                                    .num_columns(4)
                                    .spacing([14.0, 4.0])
                                    .show(ui, |ui| {
                                        ui.label(RichText::new("File Path inside ZIP").strong());
                                        ui.label(RichText::new("Compressed").strong());
                                        ui.label(RichText::new("Uncompressed").strong());
                                        ui.label(RichText::new("CRC32").strong());
                                        ui.end_row();

                                        for entry in &preview.entries {
                                            ui.label(&entry.filename);
                                            ui.label(format_bytes(entry.compressed_size));
                                            ui.label(format_bytes(entry.uncompressed_size));
                                            ui.label(
                                                RichText::new(&entry.crc32_hex).monospace(),
                                            );
                                            ui.end_row();
                                        }
                                    });
                            });
                    }
                });
            self.show_zip_preview_dialog = open;
        }

        // 6. Import / Export Download List Dialog (.ef2 / .txt)
        if self.show_import_export_dialog {
            let mut open = self.show_import_export_dialog;
            egui::Window::new("Import / Export Download List (IDM .ef2 & .txt)")
                .collapsible(false)
                .resizable(true)
                .default_width(540.0)
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.label("Paste IDM .ef2 blocks or plain URLs below, or export current tasks:");
                    ui.add(
                        egui::TextEdit::multiline(&mut self.import_export_text)
                            .desired_rows(8)
                            .desired_width(510.0)
                            .code_editor(),
                    );
                    ui.horizontal(|ui| {
                        if ui.button("Import & Queue Tasks").clicked() {
                            if !self.import_export_text.trim().is_empty() {
                                let _ = self.commands.send(UiCommand::ImportList(
                                    ImportTasksPayload {
                                        content: self.import_export_text.clone(),
                                        default_queue: Some("main".to_owned()),
                                        start_immediately: Some(true),
                                    },
                                ));
                            }
                        }
                        if ui.button("Refresh Export from Engine").clicked() {
                            let _ = self.commands.send(UiCommand::ExportList);
                        }
                        if ui.button("Copy to Clipboard").clicked() {
                            ui.ctx().copy_text(self.import_export_text.clone());
                            self.status_message = "Exported list copied to clipboard".to_owned();
                        }
                    });
                });
            self.show_import_export_dialog = open;
        }

        // 7. Multi-Tab Options & Configuration Dialog
        if self.show_options_dialog {
            let mut open = self.show_options_dialog;
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
                            let active = self.options_tab == tab;
                            if ui.selectable_label(active, title).clicked() {
                                self.options_tab = tab;
                            }
                        }
                    });
                    ui.separator();

                    match self.options_tab {
                        OptionsTab::General => {
                            ui.label(RichText::new("Base Download Directory").strong());
                            let mut dir_str =
                                self.app_config.download_dir.display().to_string();
                            if ui.text_edit_singleline(&mut dir_str).changed() {
                                self.app_config.download_dir = PathBuf::from(dir_str);
                            }
                            ui.add_space(6.0);
                            ui.checkbox(
                                &mut self.app_config.auto_start_queued_downloads,
                                "Automatically start newly intercepted downloads",
                            );
                            ui.checkbox(
                                &mut self.app_config.enable_checksum_verification,
                                "Verify SHA-256 / MD5 / CRC32 digests automatically upon completion",
                            );
                            ui.add_space(6.0);
                            ui.label("Antivirus Scanner Command (optional):");
                            let mut av_str = self
                                .app_config
                                .antivirus_command
                                .clone()
                                .unwrap_or_default();
                            if ui.text_edit_singleline(&mut av_str).changed() {
                                self.app_config.antivirus_command = opt_non_empty(&av_str);
                            }
                        }
                        OptionsTab::Connection => {
                            ui.horizontal(|ui| {
                                ui.label("Default Max Connections per File (1..32):");
                                ui.add(
                                    egui::Slider::new(
                                        &mut self.app_config.max_connections_per_file,
                                        1..=32,
                                    ),
                                );
                            });
                            ui.horizontal(|ui| {
                                ui.label("Minimum In-Half Split Size (KB):");
                                ui.add(
                                    egui::Slider::new(
                                        &mut self.app_config.min_split_size_kb,
                                        64..=4096,
                                    ),
                                );
                            });
                            ui.separator();
                            ui.label(RichText::new("Global Token-Bucket Speed Limiter").strong());
                            ui.horizontal(|ui| {
                                let mut limit =
                                    self.app_config.global_speed_limit_kbps as f32;
                                if ui
                                    .add(
                                        egui::Slider::new(&mut limit, 0.0..=51_200.0)
                                            .text("KB/s (0 = Unlimited)"),
                                    )
                                    .changed()
                                {
                                    self.app_config.global_speed_limit_kbps =
                                        limit.round() as u64;
                                    self.speed_limit_kbps =
                                        self.app_config.global_speed_limit_kbps;
                                }
                            });
                        }
                        OptionsTab::FileTypes => {
                            ui.label(
                                "Automatically intercept downloads for these file extensions:",
                            );
                            let mut ext_joined =
                                self.app_config.intercept_extensions.join(", ");
                            if ui
                                .add(
                                    egui::TextEdit::multiline(&mut ext_joined)
                                        .desired_rows(4)
                                        .desired_width(490.0),
                                )
                                .changed()
                            {
                                self.app_config.intercept_extensions = ext_joined
                                    .split(',')
                                    .map(|s| s.trim().trim_start_matches('.').to_lowercase())
                                    .filter(|s| !s.is_empty())
                                    .collect();
                            }
                        }
                        OptionsTab::Proxy => {
                            ui.label("HTTP / SOCKS5 Proxy URL (optional):");
                            let mut proxy_str =
                                self.app_config.proxy_url.clone().unwrap_or_default();
                            if ui
                                .add(
                                    egui::TextEdit::singleline(&mut proxy_str)
                                        .hint_text("http://127.0.0.1:8080")
                                        .desired_width(450.0),
                                )
                                .changed()
                            {
                                self.app_config.proxy_url = opt_non_empty(&proxy_str);
                            }
                            ui.add_space(6.0);
                            ui.label("Default HTTP User-Agent:");
                            ui.add(
                                egui::TextEdit::singleline(
                                    &mut self.app_config.custom_user_agent,
                                )
                                .desired_width(450.0),
                            );
                        }
                        OptionsTab::SiteLogins => {
                            ui.label(
                                "Configure domain credentials (automatically redacted in logs & UI):",
                            );
                            ui.horizontal(|ui| {
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.new_login_domain)
                                        .hint_text("domain.example.com")
                                        .desired_width(140.0),
                                );
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.new_login_user)
                                        .hint_text("username")
                                        .desired_width(110.0),
                                );
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.new_login_secret)
                                        .password(true)
                                        .hint_text("Bearer / Cookie secret")
                                        .desired_width(140.0),
                                );
                                if ui.button("Add").clicked()
                                    && !self.new_login_domain.trim().is_empty()
                                {
                                    self.app_config.site_logins.push(SiteLoginCredential {
                                        domain: self.new_login_domain.trim().to_owned(),
                                        username: self.new_login_user.trim().to_owned(),
                                        auth_header_or_cookie: self
                                            .new_login_secret
                                            .trim()
                                            .to_owned(),
                                        enabled: true,
                                    });
                                    self.new_login_domain.clear();
                                    self.new_login_user.clear();
                                    self.new_login_secret.clear();
                                }
                            });
                            ui.separator();
                            let mut remove_idx = None;
                            for (idx, cred) in self.app_config.site_logins.iter_mut().enumerate() {
                                ui.horizontal(|ui| {
                                    ui.checkbox(&mut cred.enabled, "");
                                    ui.label(RichText::new(&cred.domain).strong());
                                    ui.label(&cred.username);
                                    ui.label(
                                        RichText::new("[REDACTED_SECRET]")
                                            .monospace()
                                            .color(Color32::GRAY),
                                    );
                                    if ui.small_button("Delete").clicked() {
                                        remove_idx = Some(idx);
                                    }
                                });
                            }
                            if let Some(i) = remove_idx {
                                self.app_config.site_logins.remove(i);
                            }
                        }
                    }

                    ui.separator();
                    ui.horizontal(|ui| {
                        if ui.button("Save & Apply Configuration").clicked() {
                            let _ = self
                                .commands
                                .send(UiCommand::SaveConfig(self.app_config.clone()));
                            let _ = self.commands.send(UiCommand::SetSpeedLimit(
                                self.app_config.global_speed_limit_kbps,
                            ));
                        }
                    });
                });
            self.show_options_dialog = open;
        }

        // 8. Multi-Queue Manager & Scheduler Dialog
        if self.show_scheduler_dialog {
            let mut open = self.show_scheduler_dialog;
            let mut schedule = self.schedule.clone();
            egui::Window::new("Scheduler & Multi-Queue Manager")
                .collapsible(false)
                .resizable(true)
                .default_width(510.0)
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Selected Queue:").strong());
                        let q_names: Vec<String> = if self.queues.is_empty() {
                            vec!["main".to_owned(), "sync".to_owned()]
                        } else {
                            self.queues.iter().map(|q| q.name.clone()).collect()
                        };
                        egui::ComboBox::from_id_salt("scheduler_queue_selector")
                            .selected_text(&self.selected_queue_name)
                            .show_ui(ui, |ui| {
                                for q in q_names {
                                    ui.selectable_value(
                                        &mut self.selected_queue_name,
                                        q.clone(),
                                        q,
                                    );
                                }
                            });

                        ui.separator();
                        ui.add(
                            egui::TextEdit::singleline(&mut self.new_queue_name_input)
                                .hint_text("New queue name...")
                                .desired_width(120.0),
                        );
                        if ui.button("+ Create Queue").clicked()
                            && !self.new_queue_name_input.trim().is_empty()
                        {
                            let name = self.new_queue_name_input.trim().to_owned();
                            let new_q = QueueConfig {
                                name: name.clone(),
                                max_concurrent_downloads: 4,
                                start_time: None,
                                stop_time: None,
                                post_action: PostQueueAction::Notify,
                                schedule_mode: ScheduleMode::Disabled,
                                start_at: None,
                            };
                            let _ = self.commands.send(UiCommand::UpsertQueue(new_q));
                            self.selected_queue_name = name;
                            self.new_queue_name_input.clear();
                        }
                        if self.selected_queue_name != "main"
                            && ui.button("Delete Queue").clicked()
                        {
                            let _ = self
                                .commands
                                .send(UiCommand::DeleteQueue(self.selected_queue_name.clone()));
                            self.selected_queue_name = "main".to_owned();
                        }
                    });
                    ui.separator();

                    egui::ComboBox::from_label("Schedule Mode")
                        .selected_text(match schedule.mode {
                            ScheduleMode::Disabled => "Disabled (Manual Start/Stop)",
                            ScheduleMode::Daily => "Daily recurring start and stop",
                            ScheduleMode::Once => "Start once at local timestamp",
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut schedule.mode,
                                ScheduleMode::Disabled,
                                "Disabled (Manual Start/Stop)",
                            );
                            ui.selectable_value(
                                &mut schedule.mode,
                                ScheduleMode::Daily,
                                "Daily recurring start and stop",
                            );
                            ui.selectable_value(
                                &mut schedule.mode,
                                ScheduleMode::Once,
                                "Start once at local timestamp",
                            );
                        });

                    match schedule.mode {
                        ScheduleMode::Disabled => {
                            ui.label("The queue will follow manual Start/Stop controls.");
                        }
                        ScheduleMode::Daily => {
                            let start =
                                schedule.start_time.get_or_insert_with(|| "09:00".into());
                            ui.horizontal(|ui| {
                                ui.label("Start time (local, HH:MM):");
                                ui.text_edit_singleline(start);
                            });
                            let stop =
                                schedule.stop_time.get_or_insert_with(|| "17:00".into());
                            ui.horizontal(|ui| {
                                ui.label("Stop time (local, HH:MM):");
                                ui.text_edit_singleline(stop);
                            });
                        }
                        ScheduleMode::Once => {
                            let start_at = schedule.start_at.get_or_insert_with(|| {
                                chrono::Local::now().format("%Y-%m-%d %H:%M").to_string()
                            });
                            ui.horizontal(|ui| {
                                ui.label("Start at (local, YYYY-MM-DD HH:MM):");
                                ui.text_edit_singleline(start_at);
                            });
                        }
                    }

                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if ui.button("Save Schedule").clicked() {
                            self.schedule = schedule.clone();
                            self.status_message = "Saving schedule...".to_owned();
                            if let Err(error) =
                                self.commands.send(UiCommand::SetSchedule(schedule.clone()))
                            {
                                self.error_message = Some(error.to_string());
                            }
                        }
                        if ui.button("Disable Schedule").clicked() {
                            schedule = QueueSchedule::default();
                            self.schedule = schedule.clone();
                            let _ = self
                                .commands
                                .send(UiCommand::SetSchedule(schedule.clone()));
                        }
                        ui.separator();
                        if ui.button("Start Queue Now").clicked() {
                            self.start_queue_downloads();
                        }
                        if ui.button("Stop Queue Now").clicked() {
                            self.stop_all_downloads();
                        }
                    });
                });
            self.schedule = schedule;
            self.show_scheduler_dialog = open;
        }

        // 9. Classic IDM Download Progress Window with Real Segment Bar & Speed Graph
        if self.show_details {
            if let Some(task) = self.selected_download().cloned() {
                let progress = if task.total_size == 0 {
                    0.0
                } else {
                    (task.total_downloaded() as f32 / task.total_size as f32).clamp(0.0, 1.0)
                };
                let pct_int = (progress * 100.0).floor() as u32;
                let window_title = format!("{}% {}", pct_int, truncate_text(&task.filename, 38));

                let mut open = self.show_details;
                egui::Window::new(window_title)
                    .id(egui::Id::new("idm_download_status_window"))
                    .collapsible(false)
                    .resizable(false)
                    .default_width(545.0)
                    .open(&mut open)
                    .show(ctx, |ui| {
                        ui.set_min_width(530.0);

                        // Top Tabs: Download status | Speed Limiter | Options on completion | Integrity & Mirrors
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing = Vec2::new(2.0, 0.0);
                            for (tab, title) in [
                                (DialogTab::DownloadStatus, "Download status"),
                                (DialogTab::SpeedLimiter, "Speed Limiter"),
                                (DialogTab::OptionsOnCompletion, "Options on completion"),
                                (DialogTab::IntegrityAndMirrors, "Digests & Mirrors"),
                            ] {
                                let is_active = self.dialog_tab == tab;
                                let bg = if is_active {
                                    Color32::WHITE
                                } else {
                                    Color32::from_rgb(230, 230, 230)
                                };
                                let resp = egui::Frame::new()
                                    .fill(bg)
                                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(180, 180, 180)))
                                    .inner_margin(egui::Margin::symmetric(10, 4))
                                    .show(ui, |ui| {
                                        ui.label(
                                            RichText::new(title)
                                                .size(12.0)
                                                .color(Color32::BLACK),
                                        )
                                    });
                                if resp.response.interact(Sense::click()).clicked() {
                                    self.dialog_tab = tab;
                                }
                            }
                        });

                        match self.dialog_tab {
                            DialogTab::DownloadStatus => {
                                egui::Frame::new()
                                    .fill(Color32::WHITE)
                                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(200, 200, 200)))
                                    .inner_margin(egui::Margin::symmetric(14, 10))
                                    .show(ui, |ui| {
                                        ui.set_min_width(ui.available_width());
                                        ui.label(
                                            RichText::new(truncate_text(
                                                &task.current_effective_url(),
                                                68,
                                            ))
                                            .size(12.0)
                                            .color(Color32::BLACK),
                                        );
                                        ui.add_space(2.0);

                                        let speed: u64 = task
                                            .segments
                                            .iter()
                                            .map(|s| s.last_speed_bps)
                                            .sum();

                                        egui::Grid::new("idm_status_grid")
                                            .num_columns(2)
                                            .spacing([28.0, 3.0])
                                            .show(ui, |ui| {
                                                ui.label(RichText::new("Status").size(12.0));
                                                ui.label(
                                                    RichText::new(idm_dialog_status_label(
                                                        task.status,
                                                    ))
                                                    .size(12.0)
                                                    .color(Color32::from_rgb(0, 38, 230)),
                                                );
                                                ui.end_row();

                                                ui.label(RichText::new("File size").size(12.0));
                                                ui.label(
                                                    RichText::new(format_bytes(task.total_size))
                                                        .size(12.0),
                                                );
                                                ui.end_row();

                                                ui.label(RichText::new("Downloaded").size(12.0));
                                                ui.label(
                                                    RichText::new(format!(
                                                        "{} ( {:.2} % )",
                                                        format_bytes(task.total_downloaded()),
                                                        progress * 100.0
                                                    ))
                                                    .size(12.0),
                                                );
                                                ui.end_row();

                                                ui.label(
                                                    RichText::new("Transfer rate").size(12.0),
                                                );
                                                ui.label(
                                                    RichText::new(format_speed(speed)).size(12.0),
                                                );
                                                ui.end_row();

                                                ui.label(RichText::new("Time left").size(12.0));
                                                ui.label(
                                                    RichText::new(time_left(&task)).size(12.0),
                                                );
                                                ui.end_row();

                                                ui.label(
                                                    RichText::new("Resume capability").size(12.0),
                                                );
                                                ui.label(
                                                    RichText::new(if task.resumable {
                                                        "Yes"
                                                    } else {
                                                        "No"
                                                    })
                                                    .size(12.0),
                                                );
                                                ui.end_row();
                                            });
                                    });

                                ui.add_space(6.0);
                                draw_idm_green_progress_bar(ui, progress);
                                ui.add_space(6.0);

                                ui.horizontal(|ui| {
                                    let toggle_label = if self.show_connection_details {
                                        "<< Hide details"
                                    } else {
                                        "Show details >>"
                                    };
                                    if ui
                                        .add_sized([132.0, 24.0], egui::Button::new(toggle_label))
                                        .clicked()
                                    {
                                        self.show_connection_details =
                                            !self.show_connection_details;
                                    }

                                    if ui
                                        .add_sized([108.0, 24.0], egui::Button::new("Refresh URL"))
                                        .clicked()
                                    {
                                        self.refresh_task_id = task.task_id.clone();
                                        self.refresh_new_url = task.url.clone();
                                        self.show_refresh_dialog = true;
                                    }

                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        if ui
                                            .add_sized([86.0, 24.0], egui::Button::new("Cancel"))
                                            .clicked()
                                        {
                                            let _ = self.commands.send(UiCommand::Control {
                                                task_id: task.task_id.clone(),
                                                action: "cancel",
                                            });
                                        }
                                        ui.add_space(6.0);
                                        let is_paused = task.status == DownloadStatus::Paused;
                                        let pause_label =
                                            if is_paused { "Resume" } else { "Pause" };
                                        let pause_action =
                                            if is_paused { "resume" } else { "pause" };
                                        if ui
                                            .add_sized(
                                                [86.0, 24.0],
                                                egui::Button::new(pause_label),
                                            )
                                            .clicked()
                                        {
                                            let _ = self.commands.send(UiCommand::Control {
                                                task_id: task.task_id.clone(),
                                                action: pause_action,
                                            });
                                        }
                                    });
                                });

                                if self.show_connection_details {
                                    ui.add_space(6.0);
                                    ui.vertical_centered(|ui| {
                                        ui.label(
                                            RichText::new(
                                                "Start positions and download progress by connections (Real Byte Offsets)",
                                            )
                                            .size(12.0)
                                            .color(Color32::BLACK),
                                        );
                                    });
                                    ui.add_space(2.0);
                                    draw_idm_connections_bar(ui, &task, progress);
                                    ui.add_space(6.0);
                                    draw_idm_connections_table(ui, &task, progress);
                                }
                            }
                            DialogTab::SpeedLimiter => {
                                egui::Frame::new()
                                    .fill(Color32::WHITE)
                                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(200, 200, 200)))
                                    .inner_margin(egui::Margin::same(14))
                                    .show(ui, |ui| {
                                        ui.set_min_width(ui.available_width());
                                        ui.checkbox(
                                            &mut self.speed_limiter_enabled,
                                            "Use Token-Bucket Speed Limiter",
                                        );
                                        ui.add_space(8.0);
                                        ui.horizontal(|ui| {
                                            ui.label("Maximum download speed (KB/sec):");
                                            let mut limit = self.speed_limit_kbps as f32;
                                            ui.add(
                                                egui::Slider::new(&mut limit, 0.0..=51_200.0)
                                                    .show_value(true),
                                            );
                                            self.speed_limit_kbps = limit.round() as u64;
                                        });
                                        ui.add_space(8.0);
                                        if ui.button("Apply speed limit").clicked() {
                                            let target = if self.speed_limiter_enabled {
                                                self.speed_limit_kbps
                                            } else {
                                                0
                                            };
                                            let _ = self
                                                .commands
                                                .send(UiCommand::SetSpeedLimit(target));
                                        }
                                    });
                            }
                            DialogTab::OptionsOnCompletion => {
                                egui::Frame::new()
                                    .fill(Color32::WHITE)
                                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(200, 200, 200)))
                                    .inner_margin(egui::Margin::same(14))
                                    .show(ui, |ui| {
                                        ui.set_min_width(ui.available_width());
                                        ui.checkbox(
                                            &mut self.close_on_completion,
                                            "Close progress dialog when download completes",
                                        );
                                        ui.checkbox(
                                            &mut self.open_on_completion,
                                            "Show download complete notification",
                                        );
                                        ui.add_space(8.0);
                                        ui.label(format!(
                                            "Save location: {}",
                                            task.final_file_path().display()
                                        ));
                                    });
                            }
                            DialogTab::IntegrityAndMirrors => {
                                egui::Frame::new()
                                    .fill(Color32::WHITE)
                                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(200, 200, 200)))
                                    .inner_margin(egui::Margin::same(12))
                                    .show(ui, |ui| {
                                        ui.set_min_width(ui.available_width());
                                        ui.label(RichText::new("Computed Cryptographic Digests:").strong());
                                        ui.label(
                                            RichText::new(format!(
                                                "SHA-256: {}",
                                                task.computed_sha256.as_deref().unwrap_or("—")
                                            ))
                                            .monospace()
                                            .size(11.5),
                                        );
                                        ui.label(
                                            RichText::new(format!(
                                                "MD5:     {}",
                                                task.computed_md5.as_deref().unwrap_or("—")
                                            ))
                                            .monospace()
                                            .size(11.5),
                                        );
                                        ui.label(
                                            RichText::new(format!(
                                                "CRC32:   {}",
                                                task.computed_crc32.as_deref().unwrap_or("—")
                                            ))
                                            .monospace()
                                            .size(11.5),
                                        );
                                        ui.separator();
                                        draw_live_speed_graph(ui, &task);
                                    });
                            }
                        }
                    });
                self.show_details = open;
            }
        }

        if let Some(message) = self.completed_notice.clone() {
            let mut open = true;
            egui::Window::new("Download notification")
                .collapsible(false)
                .resizable(false)
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.label(message);
                    if ui.button("OK").clicked() {
                        self.completed_notice = None;
                    }
                });
            if !open {
                self.completed_notice = None;
            }
        }
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
                        || task
                            .description
                            .as_deref()
                            .unwrap_or("")
                            .to_lowercase()
                            .contains(&query_lower)
                }
            })
            .collect();

        list.sort_by(|a, b| {
            let cmp = match self.sort_column {
                SortColumn::Filename => a.filename.to_lowercase().cmp(&b.filename.to_lowercase()),
                SortColumn::Queue => a.queue_name.cmp(&b.queue_name),
                SortColumn::Size => a.total_size.cmp(&b.total_size),
                SortColumn::Status => status_label(a.status).cmp(status_label(b.status)),
                SortColumn::TimeLeft => a
                    .total_size
                    .saturating_sub(a.total_downloaded())
                    .cmp(&b.total_size.saturating_sub(b.total_downloaded())),
                SortColumn::Speed => {
                    let sa: u64 = a.segments.iter().map(|s| s.last_speed_bps).sum();
                    let sb: u64 = b.segments.iter().map(|s| s.last_speed_bps).sum();
                    sa.cmp(&sb)
                }
                SortColumn::UpdatedAt => a.updated_at.cmp(&b.updated_at),
                SortColumn::Category => category_label(a.category).cmp(category_label(b.category)),
            };
            if self.sort_ascending {
                cmp
            } else {
                cmp.reverse()
            }
        });
        list
    }

    fn selected_download(&self) -> Option<&DownloadStateMeta> {
        self.downloads
            .iter()
            .find(|task| Some(task.task_id.as_str()) == self.selected_task.as_deref())
    }

    fn has_selected(&self) -> bool {
        self.selected_download().is_some()
    }

    fn control_selected(&mut self, action: &'static str) {
        if let Some(task) = self.selected_download() {
            if let Err(error) = self.commands.send(UiCommand::Control {
                task_id: task.task_id.clone(),
                action,
            }) {
                self.error_message = Some(error.to_string());
            } else {
                self.status_message = format!("{} requested", action);
            }
        }
    }

    fn stop_all_downloads(&mut self) {
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

    fn start_queue_downloads(&mut self) {
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

    fn delete_completed_downloads(&mut self) {
        if let Err(error) = self.commands.send(UiCommand::DeleteCompleted) {
            self.error_message = Some(error.to_string());
        } else {
            self.status_message = "Removing completed download records...".to_owned();
        }
    }

    fn remove_selected_download(&mut self) {
        if let Some(task) = self.selected_download() {
            if let Err(error) = self
                .commands
                .send(UiCommand::RemoveDownload(task.task_id.clone()))
            {
                self.error_message = Some(error.to_string());
            } else {
                self.status_message = "Removing download task record...".to_owned();
            }
        }
    }
}

fn spawn_api_worker(commands: Receiver<UiCommand>, events: Sender<UiEvent>) {
    thread::spawn(move || {
        let client = match Client::builder().timeout(Duration::from_secs(20)).build() {
            Ok(client) => client,
            Err(error) => {
                let _ = events.send(UiEvent::Status(Err(error.to_string())));
                return;
            }
        };
        let schedule_result = client
            .get(format!("{API_BASE}/scheduler/main"))
            .send()
            .and_then(reqwest::blocking::Response::error_for_status)
            .and_then(|response| response.json::<QueueSchedule>())
            .map_err(|error| error.to_string());
        let _ = events.send(UiEvent::Schedule(schedule_result));

        if let Ok(resp) = client.get(format!("{API_BASE}/config")).send() {
            if let Ok(cfg) = resp.json::<AppConfig>() {
                let _ = events.send(UiEvent::Config(cfg));
            }
        }
        if let Ok(resp) = client.get(format!("{API_BASE}/queues")).send() {
            if let Ok(queues) = resp.json::<Vec<QueueConfig>>() {
                let _ = events.send(UiEvent::Queues(queues));
            }
        }

        let mut next_poll = Instant::now();
        let mut next_notification_poll = Instant::now();
        let mut last_error: Option<String> = Some("Connecting to engine".to_owned());

        loop {
            let wait = next_poll
                .saturating_duration_since(Instant::now())
                .min(Duration::from_millis(250));
            match commands.recv_timeout(wait) {
                Ok(command) => process_command(&client, command, &events),
                Err(RecvTimeoutError::Disconnected) => break,
                Err(RecvTimeoutError::Timeout) => {}
            }

            while let Ok(command) = commands.try_recv() {
                process_command(&client, command, &events);
            }

            if Instant::now() >= next_poll {
                match client
                    .get(format!("{API_BASE}/downloads"))
                    .send()
                    .and_then(reqwest::blocking::Response::error_for_status)
                    .and_then(|response| response.json::<Vec<DownloadStateMeta>>())
                {
                    Ok(downloads) => {
                        let _ = events.send(UiEvent::Downloads(downloads));
                        if last_error.take().is_some() {
                            let _ = events.send(UiEvent::EngineConnection(Ok(())));
                        }
                    }
                    Err(error) => {
                        let message = format!("Cannot read downloads from engine: {error}");
                        if last_error.as_deref() != Some(message.as_str()) {
                            let _ = events.send(UiEvent::EngineConnection(Err(message.clone())));
                            last_error = Some(message);
                        }
                    }
                }

                if let Ok(resp) = client.get(format!("{API_BASE}/stats")).send() {
                    if let Ok(stats) = resp.json::<EngineStatistics>() {
                        let _ = events.send(UiEvent::Stats(stats));
                    }
                }

                next_poll = Instant::now() + Duration::from_millis(850);
            }
            if Instant::now() >= next_notification_poll {
                match client
                    .get(format!("{API_BASE}/scheduler/notifications"))
                    .send()
                    .and_then(reqwest::blocking::Response::error_for_status)
                    .and_then(|response| response.json::<Vec<String>>())
                {
                    Ok(notifications) => {
                        for notification in notifications {
                            let _ = events.send(UiEvent::CompletedNotice(notification));
                        }
                    }
                    Err(error) => {
                        let _ = events.send(UiEvent::Status(Err(format!(
                            "Cannot read scheduler notifications: {error}"
                        ))));
                    }
                }
                if let Ok(resp) = client.get(format!("{API_BASE}/queues")).send() {
                    if let Ok(queues) = resp.json::<Vec<QueueConfig>>() {
                        let _ = events.send(UiEvent::Queues(queues));
                    }
                }
                next_notification_poll = Instant::now() + Duration::from_secs(2);
            }
        }
    });
}

fn process_command(client: &Client, command: UiCommand, events: &Sender<UiEvent>) {
    let result = match command {
        UiCommand::AddDownload(payload) => client
            .post(format!("{API_BASE}/intercept"))
            .json(&payload)
            .send()
            .and_then(reqwest::blocking::Response::error_for_status)
            .and_then(|response| response.json::<serde_json::Value>())
            .map(|reply| {
                format!(
                    "Download queued ({})",
                    reply["task_id"].as_str().unwrap_or("accepted")
                )
            })
            .map_err(|error| error.to_string()),
        UiCommand::AddBatch(req) => {
            let res = client
                .post(format!("{API_BASE}/batch"))
                .json(&req)
                .send()
                .and_then(reqwest::blocking::Response::error_for_status)
                .and_then(|r| r.json::<BatchExpandResponse>())
                .map_err(|e| e.to_string());
            let status_res = res
                .as_ref()
                .map(|r| format!("Queued {} batch tasks", r.queued_task_ids.len()))
                .map_err(|e| e.clone());
            let _ = events.send(UiEvent::BatchResult(res));
            status_res
        }
        UiCommand::RefreshUrl(payload) => client
            .post(format!("{API_BASE}/refresh-url"))
            .json(&payload)
            .send()
            .and_then(reqwest::blocking::Response::error_for_status)
            .and_then(|r| r.json::<serde_json::Value>())
            .map(|v| {
                let kept = v["preserved_partial_bytes"].as_bool().unwrap_or(false);
                format!("URL refreshed & resumed (preserved partial bytes: {kept})")
            })
            .map_err(|e| e.to_string()),
        UiCommand::CrawlSite(cfg) => {
            let res = client
                .post(format!("{API_BASE}/crawl"))
                .json(&cfg)
                .send()
                .and_then(reqwest::blocking::Response::error_for_status)
                .and_then(|r| r.json::<CrawlReport>())
                .map_err(|e| e.to_string());
            let status_res = res
                .as_ref()
                .map(|r| format!("Crawled {} pages", r.pages_visited))
                .map_err(|e| e.clone());
            let _ = events.send(UiEvent::CrawlResult(res));
            status_res
        }
        UiCommand::PreviewZip(req) => {
            let res = client
                .post(format!("{API_BASE}/zip-preview"))
                .json(&req)
                .send()
                .and_then(reqwest::blocking::Response::error_for_status)
                .and_then(|r| r.json::<RemoteZipPreviewResponse>())
                .map_err(|e| e.to_string());
            let status_res = res
                .as_ref()
                .map(|r| format!("ZIP inspected: {} entries", r.total_entries))
                .map_err(|e| e.clone());
            let _ = events.send(UiEvent::ZipPreviewResult(res));
            status_res
        }
        UiCommand::ReverifyChecksum(task_id) => client
            .post(format!("{API_BASE}/downloads/{task_id}/reverify"))
            .send()
            .and_then(reqwest::blocking::Response::error_for_status)
            .and_then(|r| r.json::<serde_json::Value>())
            .map(|v| {
                format!(
                    "Verified CRC32={} | SHA256={}..",
                    v["crc32"].as_str().unwrap_or("ok"),
                    &v["sha256"].as_str().unwrap_or("ok")[..8.min(v["sha256"].as_str().unwrap_or("ok").len())]
                )
            })
            .map_err(|e| e.to_string()),
        UiCommand::UpsertQueue(q_cfg) => client
            .post(format!("{API_BASE}/queues"))
            .json(&q_cfg)
            .send()
            .and_then(reqwest::blocking::Response::error_for_status)
            .map(|_| format!("Queue '{}' saved", q_cfg.name))
            .map_err(|e| e.to_string()),
        UiCommand::DeleteQueue(q_name) => client
            .delete(format!("{API_BASE}/queues/{q_name}"))
            .send()
            .and_then(reqwest::blocking::Response::error_for_status)
            .map(|_| format!("Queue '{q_name}' deleted"))
            .map_err(|e| e.to_string()),
        UiCommand::SaveConfig(cfg) => client
            .post(format!("{API_BASE}/config"))
            .json(&cfg)
            .send()
            .and_then(reqwest::blocking::Response::error_for_status)
            .and_then(|r| r.json::<AppConfig>())
            .map(|saved| {
                let _ = events.send(UiEvent::Config(saved));
                "Engine configuration saved to config.json".to_owned()
            })
            .map_err(|e| e.to_string()),
        UiCommand::ExportList => {
            let res = client
                .get(format!("{API_BASE}/export"))
                .send()
                .and_then(reqwest::blocking::Response::error_for_status)
                .and_then(|r| r.text())
                .map_err(|e| e.to_string());
            let status_res = res
                .as_ref()
                .map(|_| "Task list exported".to_owned())
                .map_err(|e| e.clone());
            let _ = events.send(UiEvent::ExportedText(res));
            status_res
        }
        UiCommand::ImportList(payload) => client
            .post(format!("{API_BASE}/import"))
            .json(&payload)
            .send()
            .and_then(reqwest::blocking::Response::error_for_status)
            .and_then(|r| r.json::<serde_json::Value>())
            .map(|v| {
                format!(
                    "Imported {} download tasks",
                    v["imported_count"].as_u64().unwrap_or(0)
                )
            })
            .map_err(|e| e.to_string()),
        UiCommand::SetSchedule(schedule) => {
            let result = client
                .post(format!("{API_BASE}/scheduler/main"))
                .json(&schedule)
                .send()
                .and_then(reqwest::blocking::Response::error_for_status)
                .and_then(|response| response.json::<QueueSchedule>())
                .map_err(|error| error.to_string());
            match result {
                Ok(saved) => {
                    let _ = events.send(UiEvent::Schedule(Ok(saved)));
                    Ok("Schedule saved".to_owned())
                }
                Err(error) => {
                    let _ = events.send(UiEvent::Schedule(Err(error.clone())));
                    Err(error)
                }
            }
        }
        UiCommand::DeleteCompleted => client
            .post(format!("{API_BASE}/downloads/completed"))
            .send()
            .and_then(reqwest::blocking::Response::error_for_status)
            .and_then(|response| response.json::<serde_json::Value>())
            .map(|reply| {
                format!(
                    "Removed {} completed task records; downloaded files were kept",
                    reply["count"].as_u64().unwrap_or(0)
                )
            })
            .map_err(|error| error.to_string()),
        UiCommand::RemoveDownload(task_id) => client
            .delete(format!("{API_BASE}/downloads/{task_id}"))
            .send()
            .and_then(reqwest::blocking::Response::error_for_status)
            .map(|_| "Download task record removed; downloaded file was kept".to_owned())
            .map_err(|error| error.to_string()),
        UiCommand::Control { task_id, action } => client
            .post(format!("{API_BASE}/control"))
            .json(&ControlTaskPayload {
                task_id,
                action: action.to_owned(),
                speed_limit_kbps: None,
            })
            .send()
            .and_then(reqwest::blocking::Response::error_for_status)
            .map(|_| format!("{action} command sent"))
            .map_err(|error| error.to_string()),
        UiCommand::SetSpeedLimit(speed_limit_kbps) => client
            .post(format!("{API_BASE}/control"))
            .json(&ControlTaskPayload {
                task_id: String::new(),
                action: "speed_limit".to_owned(),
                speed_limit_kbps: Some(speed_limit_kbps),
            })
            .send()
            .and_then(reqwest::blocking::Response::error_for_status)
            .map(|_| "Speed limit updated".to_owned())
            .map_err(|error| error.to_string()),
    };
    let _ = events.send(UiEvent::Status(result));
}

fn start_engine() -> Result<()> {
    let (ready_tx, ready_rx) = mpsc::sync_channel(1);
    thread::Builder::new()
        .name("rustidm-engine".to_owned())
        .spawn(move || {
            let result = (|| -> Result<()> {
                let runtime = Builder::new_multi_thread()
                    .enable_all()
                    .build()
                    .context("Failed to create the async runtime")?;
                let (state, listener, restored) = runtime.block_on(async {
                    let state = AppState::new(download_directory(), 8, 0);
                    let restored = state.restore_persisted_tasks_on_startup().await?;
                    let listener = tokio::net::TcpListener::bind("127.0.0.1:16888")
                        .await
                        .context("Cannot start the local engine on 127.0.0.1:16888")?;
                    state.start_scheduler();
                    Ok::<_, anyhow::Error>((state, listener, restored))
                })?;
                ready_tx
                    .send(Ok(restored))
                    .map_err(|_| anyhow!("Desktop window closed before engine startup"))?;
                runtime.block_on(async move {
                    axum::serve(listener, crate::build_daemon_router(state)).await?;
                    Ok::<_, anyhow::Error>(())
                })?;
                Ok(())
            })();
            if let Err(error) = result {
                let message = format!("{error:#}");
                let _ = ready_tx.send(Err(message.clone()));
                eprintln!("RustIDM engine stopped: {message}");
            }
        })
        .context("Failed to start the RustIDM engine thread")?;

    match ready_rx.recv_timeout(Duration::from_secs(20)) {
        Ok(Ok(restored)) => {
            eprintln!("RustIDM engine ready; restored {restored} downloads");
            Ok(())
        }
        Ok(Err(error)) => Err(anyhow!(error)),
        Err(error) => Err(anyhow!(
            "Timed out waiting for local engine startup: {error}"
        )),
    }
}

pub fn run() -> eframe::Result {
    start_engine().map_err(|error| {
        eframe::Error::AppCreation(Box::new(std::io::Error::other(error.to_string())))
    })?;
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1020.0, 620.0])
            .with_min_inner_size([820.0, 480.0])
            .with_title("RustIDM v2.0.0"),
        ..Default::default()
    };
    eframe::run_native(
        "RustIDM v2.0.0",
        options,
        Box::new(|context| {
            context.egui_ctx.set_visuals(egui::Visuals::light());
            Ok(Box::new(RustIdmApp::default()))
        }),
    )
}

#[cfg(target_os = "windows")]
pub fn show_startup_error(message: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};

    let text = format!("RustIDM could not start:\n\n{message}")
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let title = "RustIDM"
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

// ============================================================================
// IDM 6.38 Custom Vector UI Drawing Helpers
// ============================================================================

fn idm_toolbar_button(
    ui: &mut Ui,
    icon: ToolbarIcon,
    label: &str,
    enabled: bool,
    has_dropdown: bool,
) -> bool {
    let width = if has_dropdown { 72.0 } else { 62.0 };
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 58.0), Sense::click());

    let painter = ui.painter();
    if enabled && response.hovered() {
        painter.rect_filled(rect, 3.0, Color32::from_rgb(225, 236, 248));
    }

    let icon_center = Pos2::new(
        rect.center().x - if has_dropdown { 5.0 } else { 0.0 },
        rect.top() + 21.0,
    );
    draw_toolbar_vector_icon(painter, icon_center, icon, enabled);

    if has_dropdown {
        let ax = rect.right() - 8.0;
        let ay = rect.top() + 22.0;
        painter.line_segment(
            [Pos2::new(ax - 3.0, ay - 1.0), Pos2::new(ax + 3.0, ay - 1.0)],
            Stroke::new(1.5_f32, Color32::BLACK),
        );
        painter.line_segment(
            [Pos2::new(ax - 2.0, ay), Pos2::new(ax + 2.0, ay)],
            Stroke::new(1.5_f32, Color32::BLACK),
        );
        painter.line_segment(
            [Pos2::new(ax - 1.0, ay + 1.0), Pos2::new(ax + 1.0, ay + 1.0)],
            Stroke::new(1.5_f32, Color32::BLACK),
        );
    }

    let text_color = if enabled {
        Color32::from_rgb(25, 25, 25)
    } else {
        Color32::from_rgb(175, 175, 175)
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

fn draw_toolbar_vector_icon(painter: &egui::Painter, c: Pos2, icon: ToolbarIcon, enabled: bool) {
    let disabled = Color32::from_rgb(190, 190, 190);
    match icon {
        ToolbarIcon::AddUrl => {
            let col = if enabled {
                Color32::from_rgb(102, 184, 136)
            } else {
                disabled
            };
            let s = Stroke::new(1.8_f32, col);
            // Hollow green cross
            painter.rect_filled(
                Rect::from_center_size(c, Vec2::new(8.0, 26.0)),
                1.5,
                Color32::from_rgb(230, 246, 236),
            );
            painter.rect_filled(
                Rect::from_center_size(c, Vec2::new(26.0, 8.0)),
                1.5,
                Color32::from_rgb(230, 246, 236),
            );
            painter.line_segment(
                [
                    Pos2::new(c.x - 4.0, c.y - 13.0),
                    Pos2::new(c.x + 4.0, c.y - 13.0),
                ],
                s,
            );
            painter.line_segment(
                [
                    Pos2::new(c.x - 4.0, c.y + 13.0),
                    Pos2::new(c.x + 4.0, c.y + 13.0),
                ],
                s,
            );
            painter.line_segment(
                [
                    Pos2::new(c.x - 13.0, c.y - 4.0),
                    Pos2::new(c.x - 13.0, c.y + 4.0),
                ],
                s,
            );
            painter.line_segment(
                [
                    Pos2::new(c.x + 13.0, c.y - 4.0),
                    Pos2::new(c.x + 13.0, c.y + 4.0),
                ],
                s,
            );
            painter.line_segment(
                [
                    Pos2::new(c.x - 4.0, c.y - 13.0),
                    Pos2::new(c.x - 4.0, c.y - 4.0),
                ],
                s,
            );
            painter.line_segment(
                [
                    Pos2::new(c.x + 4.0, c.y - 13.0),
                    Pos2::new(c.x + 4.0, c.y - 4.0),
                ],
                s,
            );
            painter.line_segment(
                [
                    Pos2::new(c.x - 4.0, c.y + 4.0),
                    Pos2::new(c.x - 4.0, c.y + 13.0),
                ],
                s,
            );
            painter.line_segment(
                [
                    Pos2::new(c.x + 4.0, c.y + 4.0),
                    Pos2::new(c.x + 4.0, c.y + 13.0),
                ],
                s,
            );
            painter.line_segment(
                [
                    Pos2::new(c.x - 13.0, c.y - 4.0),
                    Pos2::new(c.x - 4.0, c.y - 4.0),
                ],
                s,
            );
            painter.line_segment(
                [
                    Pos2::new(c.x + 4.0, c.y - 4.0),
                    Pos2::new(c.x + 13.0, c.y - 4.0),
                ],
                s,
            );
            painter.line_segment(
                [
                    Pos2::new(c.x - 13.0, c.y + 4.0),
                    Pos2::new(c.x - 4.0, c.y + 4.0),
                ],
                s,
            );
            painter.line_segment(
                [
                    Pos2::new(c.x + 4.0, c.y + 4.0),
                    Pos2::new(c.x + 13.0, c.y + 4.0),
                ],
                s,
            );
        }
        ToolbarIcon::Resume => {
            let col = if enabled {
                Color32::from_rgb(95, 165, 115)
            } else {
                disabled
            };
            painter.circle_filled(c, 14.0, Color32::from_rgb(236, 236, 236));
            painter.circle_stroke(c, 14.0, Stroke::new(1.6_f32, col));
            let s = Stroke::new(1.8_f32, col);
            painter.line_segment([Pos2::new(c.x, c.y - 6.0), Pos2::new(c.x, c.y + 6.0)], s);
            painter.line_segment(
                [Pos2::new(c.x - 5.0, c.y + 1.0), Pos2::new(c.x, c.y + 6.0)],
                s,
            );
            painter.line_segment(
                [Pos2::new(c.x + 5.0, c.y + 1.0), Pos2::new(c.x, c.y + 6.0)],
                s,
            );
        }
        ToolbarIcon::Stop => {
            let col = if enabled {
                Color32::from_rgb(138, 102, 162)
            } else {
                disabled
            };
            painter.circle_filled(c, 14.0, Color32::from_rgb(238, 232, 242));
            painter.circle_stroke(c, 14.0, Stroke::new(1.6_f32, col));
            let r = Rect::from_center_size(c, Vec2::new(9.0, 9.0));
            painter.rect_filled(r, 1.0, Color32::WHITE);
            let s = Stroke::new(1.6_f32, col);
            painter.line_segment([r.left_top(), r.right_top()], s);
            painter.line_segment([r.right_top(), r.right_bottom()], s);
            painter.line_segment([r.right_bottom(), r.left_bottom()], s);
            painter.line_segment([r.left_bottom(), r.left_top()], s);
        }
        ToolbarIcon::StopAll => {
            let col = if enabled {
                Color32::from_rgb(176, 98, 108)
            } else {
                disabled
            };
            let s = Stroke::new(1.6_f32, col);
            // Palm + fingers (IDM Stop All hand)
            let palm = Rect::from_min_max(
                Pos2::new(c.x - 8.0, c.y - 2.0),
                Pos2::new(c.x + 8.0, c.y + 12.0),
            );
            painter.rect_filled(palm, 3.0, Color32::from_rgb(246, 232, 234));
            for dx in [-6.0, -2.0, 2.0, 6.0] {
                painter.line_segment(
                    [
                        Pos2::new(c.x + dx, c.y - 12.0),
                        Pos2::new(c.x + dx, c.y + 2.0),
                    ],
                    Stroke::new(2.4_f32, col),
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
                Color32::from_rgb(186, 85, 140)
            } else {
                Color32::from_rgb(150, 150, 150)
            };
            let s = Stroke::new(1.6_f32, col);
            let body = Rect::from_min_max(
                Pos2::new(c.x - 9.0, c.y - 7.0),
                Pos2::new(c.x + 9.0, c.y + 13.0),
            );
            painter.rect_filled(body, 1.5, Color32::from_rgb(245, 238, 242));
            painter.line_segment([body.left_top(), body.right_top()], s);
            painter.line_segment([body.left_top(), body.left_bottom()], s);
            painter.line_segment([body.right_top(), body.right_bottom()], s);
            painter.line_segment([body.left_bottom(), body.right_bottom()], s);
            // Lid
            painter.line_segment(
                [
                    Pos2::new(c.x - 11.0, c.y - 9.0),
                    Pos2::new(c.x + 11.0, c.y - 9.0),
                ],
                s,
            );
            painter.line_segment(
                [
                    Pos2::new(c.x - 4.0, c.y - 12.0),
                    Pos2::new(c.x + 4.0, c.y - 12.0),
                ],
                s,
            );
            if is_comp {
                // Checkmark inside trash can
                painter.line_segment(
                    [
                        Pos2::new(c.x - 4.0, c.y + 3.0),
                        Pos2::new(c.x - 1.0, c.y + 7.0),
                    ],
                    s,
                );
                painter.line_segment(
                    [
                        Pos2::new(c.x - 1.0, c.y + 7.0),
                        Pos2::new(c.x + 5.0, c.y - 2.0),
                    ],
                    s,
                );
            } else {
                for dx in [-4.0, 0.0, 4.0] {
                    painter.line_segment(
                        [
                            Pos2::new(c.x + dx, c.y - 3.0),
                            Pos2::new(c.x + dx, c.y + 9.0),
                        ],
                        s,
                    );
                }
            }
        }
        ToolbarIcon::Options => {
            let col = Color32::from_rgb(142, 124, 195);
            for i in 0..8 {
                let angle = (i as f32) * std::f32::consts::TAU / 8.0;
                let p1 = Pos2::new(c.x + angle.cos() * 8.0, c.y + angle.sin() * 8.0);
                let p2 = Pos2::new(c.x + angle.cos() * 14.0, c.y + angle.sin() * 14.0);
                painter.line_segment([p1, p2], Stroke::new(3.2_f32, col));
            }
            painter.circle_filled(c, 10.5, Color32::from_rgb(234, 228, 248));
            painter.circle_stroke(c, 10.5, Stroke::new(1.6_f32, col));
            painter.circle_filled(c, 4.5, Color32::WHITE);
            painter.circle_stroke(c, 4.5, Stroke::new(1.4_f32, col));
        }
        ToolbarIcon::Scheduler => {
            let col = Color32::from_rgb(166, 128, 178);
            painter.circle_filled(c, 13.0, Color32::from_rgb(240, 232, 244));
            painter.circle_stroke(c, 13.0, Stroke::new(1.7_f32, col));
            // Alarm bells
            painter.line_segment(
                [
                    Pos2::new(c.x - 12.0, c.y - 10.0),
                    Pos2::new(c.x - 7.0, c.y - 14.0),
                ],
                Stroke::new(2.0_f32, col),
            );
            painter.line_segment(
                [
                    Pos2::new(c.x + 7.0, c.y - 14.0),
                    Pos2::new(c.x + 12.0, c.y - 10.0),
                ],
                Stroke::new(2.0_f32, col),
            );
            // Clock hands
            painter.line_segment([c, Pos2::new(c.x, c.y - 7.0)], Stroke::new(1.8_f32, col));
            painter.line_segment(
                [c, Pos2::new(c.x + 5.0, c.y + 3.0)],
                Stroke::new(1.8_f32, col),
            );
        }
        ToolbarIcon::StartQueue | ToolbarIcon::StopQueue => {
            let is_start = matches!(icon, ToolbarIcon::StartQueue);
            let col = if is_start {
                Color32::from_rgb(98, 160, 198)
            } else {
                Color32::from_rgb(150, 120, 185)
            };
            let s = Stroke::new(1.5_f32, col);
            let r1 = Rect::from_min_max(
                Pos2::new(c.x - 11.0, c.y - 12.0),
                Pos2::new(c.x + 5.0, c.y + 6.0),
            );
            let r2 = Rect::from_min_max(
                Pos2::new(c.x - 6.0, c.y - 7.0),
                Pos2::new(c.x + 10.0, c.y + 11.0),
            );
            painter.rect_filled(r1, 1.5, Color32::from_rgb(235, 242, 248));
            painter.line_segment([r1.left_top(), r1.right_top()], s);
            painter.line_segment([r1.left_top(), r1.left_bottom()], s);
            painter.rect_filled(r2, 1.5, Color32::WHITE);
            painter.line_segment([r2.left_top(), r2.right_top()], s);
            painter.line_segment([r2.right_top(), r2.right_bottom()], s);
            painter.line_segment([r2.left_bottom(), r2.right_bottom()], s);
            painter.line_segment([r2.left_top(), r2.left_bottom()], s);
            if is_start {
                let ac = Pos2::new(c.x + 6.0, c.y + 7.0);
                painter.line_segment(
                    [Pos2::new(ac.x, ac.y - 5.0), Pos2::new(ac.x, ac.y + 4.0)],
                    Stroke::new(1.8_f32, Color32::from_rgb(60, 165, 130)),
                );
            } else {
                let sq =
                    Rect::from_center_size(Pos2::new(c.x + 6.0, c.y + 7.0), Vec2::new(8.0, 8.0));
                painter.rect_filled(sq, 1.0, Color32::from_rgb(175, 70, 120));
            }
        }
        ToolbarIcon::Grabber => {
            // Spider / Globe Web Crawler icon
            let col = if enabled {
                Color32::from_rgb(56, 132, 215)
            } else {
                disabled
            };
            painter.circle_filled(c, 11.5, Color32::from_rgb(232, 242, 252));
            painter.circle_stroke(c, 11.5, Stroke::new(1.6_f32, col));
            painter.circle_stroke(c, 6.0, Stroke::new(1.1_f32, col));
            painter.line_segment(
                [Pos2::new(c.x - 11.0, c.y), Pos2::new(c.x + 11.0, c.y)],
                Stroke::new(1.2_f32, col),
            );
            painter.line_segment(
                [Pos2::new(c.x, c.y - 11.0), Pos2::new(c.x, c.y + 11.0)],
                Stroke::new(1.2_f32, col),
            );
        }
        ToolbarIcon::Batch => {
            // Stacked Batch Layers + Plus badge
            let col = if enabled {
                Color32::from_rgb(45, 145, 85)
            } else {
                disabled
            };
            let s = Stroke::new(1.4_f32, col);
            for offset in [-3.5_f32, 0.0, 3.5] {
                let r = Rect::from_center_size(
                    Pos2::new(c.x + offset * 0.6, c.y + offset),
                    Vec2::new(16.0, 10.0),
                );
                painter.rect_filled(r, 1.5, Color32::from_rgb(230, 248, 236));
                painter.line_segment([r.left_top(), r.right_top()], s);
                painter.line_segment([r.right_top(), r.right_bottom()], s);
                painter.line_segment([r.right_bottom(), r.left_bottom()], s);
                painter.line_segment([r.left_bottom(), r.left_top()], s);
            }
        }
        ToolbarIcon::TellFriend => {
            let col = Color32::from_rgb(144, 136, 204);
            let s = Stroke::new(1.6_f32, col);
            let box_r = Rect::from_min_max(
                Pos2::new(c.x - 9.0, c.y - 3.0),
                Pos2::new(c.x + 10.0, c.y + 11.0),
            );
            painter.rect_filled(box_r, 3.0, Color32::from_rgb(236, 234, 248));
            painter.line_segment([box_r.left_top(), box_r.right_top()], s);
            painter.line_segment([box_r.left_bottom(), box_r.right_bottom()], s);
            painter.line_segment([box_r.left_top(), box_r.left_bottom()], s);
            painter.line_segment([box_r.right_top(), box_r.right_bottom()], s);
            painter.line_segment(
                [
                    Pos2::new(c.x - 2.0, c.y - 3.0),
                    Pos2::new(c.x + 1.0, c.y - 11.0),
                ],
                Stroke::new(3.0_f32, col),
            );
        }
    }
}

#[derive(Clone, Copy)]
enum TreeIconKind {
    FolderYellow,
    FolderFinished,
    Compressed,
    Document,
    Music,
    Program,
    Video,
}

fn tree_root_row(
    ui: &mut Ui,
    expanded: &mut bool,
    label: &str,
    icon: TreeIconKind,
    selected: bool,
) -> bool {
    let (rect, resp) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 20.0), Sense::click());
    let painter = ui.painter();

    if selected {
        painter.rect_filled(
            Rect::from_min_max(
                Pos2::new(rect.left() + 36.0, rect.top()),
                rect.right_bottom(),
            ),
            1.0,
            Color32::from_rgb(205, 232, 255),
        );
    }

    // [+] / [-] Box
    let box_center = Pos2::new(rect.left() + 12.0, rect.center().y);
    let box_rect = Rect::from_center_size(box_center, Vec2::new(9.0, 9.0));
    painter.rect_filled(box_rect, 0.0, Color32::WHITE);
    let box_s = Stroke::new(1.0_f32, Color32::from_rgb(130, 130, 130));
    painter.line_segment([box_rect.left_top(), box_rect.right_top()], box_s);
    painter.line_segment([box_rect.right_top(), box_rect.right_bottom()], box_s);
    painter.line_segment([box_rect.right_bottom(), box_rect.left_bottom()], box_s);
    painter.line_segment([box_rect.left_bottom(), box_rect.left_top()], box_s);
    painter.line_segment(
        [
            Pos2::new(box_center.x - 2.5, box_center.y),
            Pos2::new(box_center.x + 2.5, box_center.y),
        ],
        Stroke::new(1.0_f32, Color32::BLACK),
    );
    if !*expanded {
        painter.line_segment(
            [
                Pos2::new(box_center.x, box_center.y - 2.5),
                Pos2::new(box_center.x, box_center.y + 2.5),
            ],
            Stroke::new(1.0_f32, Color32::BLACK),
        );
    }

    draw_tree_icon(
        painter,
        Pos2::new(rect.left() + 27.0, rect.center().y),
        icon,
    );

    painter.text(
        Pos2::new(rect.left() + 39.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(12.0),
        Color32::BLACK,
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

fn tree_child_row(ui: &mut Ui, label: &str, icon: TreeIconKind, selected: bool) -> bool {
    let (rect, resp) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 20.0), Sense::click());
    let painter = ui.painter();

    if selected {
        painter.rect_filled(
            Rect::from_min_max(
                Pos2::new(rect.left() + 50.0, rect.top()),
                rect.right_bottom(),
            ),
            1.0,
            Color32::from_rgb(205, 232, 255),
        );
    }

    // Dotted connector line
    let lx = rect.left() + 27.0;
    painter.line_segment(
        [Pos2::new(lx, rect.top()), Pos2::new(lx, rect.bottom())],
        Stroke::new(1.0_f32, Color32::from_rgb(190, 190, 190)),
    );
    painter.line_segment(
        [
            Pos2::new(lx, rect.center().y),
            Pos2::new(lx + 9.0, rect.center().y),
        ],
        Stroke::new(1.0_f32, Color32::from_rgb(190, 190, 190)),
    );

    draw_tree_icon(
        painter,
        Pos2::new(rect.left() + 41.0, rect.center().y),
        icon,
    );

    painter.text(
        Pos2::new(rect.left() + 53.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(12.0),
        Color32::BLACK,
    );

    resp.clicked()
}

fn draw_tree_icon(painter: &egui::Painter, c: Pos2, icon: TreeIconKind) {
    match icon {
        TreeIconKind::FolderYellow | TreeIconKind::FolderFinished => {
            let r = Rect::from_center_size(c, Vec2::new(13.0, 10.0));
            painter.rect_filled(r, 1.0, Color32::from_rgb(245, 206, 98));
            if matches!(icon, TreeIconKind::FolderFinished) {
                painter.circle_filled(
                    Pos2::new(c.x + 2.0, c.y + 1.0),
                    3.5,
                    Color32::from_rgb(55, 130, 210),
                );
            }
        }
        TreeIconKind::Compressed => {
            let r = Rect::from_center_size(c, Vec2::new(11.0, 12.0));
            painter.rect_filled(r, 1.0, Color32::from_rgb(225, 185, 110));
        }
        TreeIconKind::Document => {
            let r = Rect::from_center_size(c, Vec2::new(10.0, 12.0));
            painter.rect_filled(r, 1.0, Color32::from_rgb(220, 230, 242));
        }
        TreeIconKind::Music => {
            painter.circle_filled(
                Pos2::new(c.x - 2.0, c.y + 3.0),
                3.0,
                Color32::from_rgb(65, 120, 215),
            );
            painter.line_segment(
                [Pos2::new(c.x, c.y + 3.0), Pos2::new(c.x, c.y - 5.0)],
                Stroke::new(1.5_f32, Color32::from_rgb(65, 120, 215)),
            );
        }
        TreeIconKind::Program => {
            let r = Rect::from_center_size(c, Vec2::new(12.0, 10.0));
            painter.rect_filled(r, 1.0, Color32::from_rgb(130, 165, 215));
        }
        TreeIconKind::Video => {
            let r = Rect::from_center_size(c, Vec2::new(12.0, 10.0));
            painter.rect_filled(r, 1.0, Color32::from_rgb(100, 175, 215));
        }
    }
}

fn draw_file_category_icon(painter: &egui::Painter, c: Pos2, category: FileCategory) {
    let color = match category {
        FileCategory::Music => Color32::from_rgb(65, 145, 230),
        FileCategory::Video => Color32::from_rgb(135, 95, 205),
        FileCategory::Compressed => Color32::from_rgb(215, 155, 55),
        FileCategory::Programs => Color32::from_rgb(45, 160, 115),
        FileCategory::Documents => Color32::from_rgb(215, 80, 65),
        FileCategory::General => Color32::from_rgb(120, 145, 175),
    };
    let r = Rect::from_center_size(c, Vec2::new(13.0, 14.0));
    painter.rect_filled(r, 1.5, color);
    painter.rect_filled(
        Rect::from_center_size(Pos2::new(c.x, c.y + 1.0), Vec2::new(7.0, 6.0)),
        0.5,
        Color32::WHITE,
    );
}

fn draw_queue_mini_icon(painter: &egui::Painter, c: Pos2, completed: bool) {
    let col = if completed {
        Color32::from_rgb(90, 175, 95)
    } else {
        Color32::from_rgb(225, 180, 75)
    };
    let r = Rect::from_center_size(c, Vec2::new(12.0, 10.0));
    painter.rect_filled(r, 1.0, col);
}

fn draw_idm_green_progress_bar(ui: &mut Ui, progress: f32) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 18.0), Sense::hover());
    let painter = ui.painter();

    painter.rect_filled(rect, 0.0, Color32::from_rgb(230, 230, 230));

    let fill_w = (rect.width() * progress.clamp(0.0, 1.0)).max(0.0);
    if fill_w > 0.0 {
        let fill_rect = Rect::from_min_size(rect.min, Vec2::new(fill_w, rect.height()));
        painter.rect_filled(fill_rect, 0.0, Color32::from_rgb(0, 168, 28));
    }

    let border = Stroke::new(1.0_f32, Color32::from_rgb(160, 160, 160));
    painter.line_segment([rect.left_top(), rect.right_top()], border);
    painter.line_segment([rect.right_top(), rect.right_bottom()], border);
    painter.line_segment([rect.right_bottom(), rect.left_bottom()], border);
    painter.line_segment([rect.left_bottom(), rect.left_top()], border);
}

fn draw_idm_connections_bar(ui: &mut Ui, task: &DownloadStateMeta, overall_progress: f32) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 20.0), Sense::hover());
    let painter = ui.painter();

    painter.rect_filled(rect, 0.0, Color32::from_rgb(238, 240, 245));

    if task.total_size > 0 && !task.segments.is_empty() {
        let total_f = task.total_size as f32;
        for (i, seg) in task.segments.iter().enumerate() {
            let start_ratio = (seg.start_byte as f32 / total_f).clamp(0.0, 1.0);
            let end_byte_clamped = seg.end_byte.min(task.total_size.saturating_sub(1));
            let end_ratio = ((end_byte_clamped + 1) as f32 / total_f).clamp(start_ratio, 1.0);
            let cur_ratio =
                (seg.current_offset as f32 / total_f).clamp(start_ratio, end_ratio);

            let slot_left = rect.left() + start_ratio * rect.width();
            let fill_right = rect.left() + cur_ratio * rect.width();

            if fill_right > slot_left {
                let top_half = Rect::from_min_max(
                    Pos2::new(slot_left, rect.top()),
                    Pos2::new(fill_right, rect.center().y),
                );
                let bot_half = Rect::from_min_max(
                    Pos2::new(slot_left, rect.center().y),
                    Pos2::new(fill_right, rect.bottom()),
                );
                painter.rect_filled(top_half, 0.0, Color32::from_rgb(65, 135, 225));
                painter.rect_filled(bot_half, 0.0, Color32::from_rgb(12, 82, 184));
            }

            // Red start-position vertical divider at the exact byte offset where each segment starts
            if i > 0 {
                painter.line_segment(
                    [
                        Pos2::new(slot_left, rect.top()),
                        Pos2::new(slot_left, rect.bottom()),
                    ],
                    Stroke::new(1.2_f32, Color32::from_rgb(205, 35, 35)),
                );
            }
        }
    } else {
        let fill_w = (rect.width() * overall_progress.clamp(0.0, 1.0)).max(0.0);
        if fill_w > 0.0 {
            let fill_rect = Rect::from_min_size(rect.min, Vec2::new(fill_w, rect.height()));
            painter.rect_filled(fill_rect, 0.0, Color32::from_rgb(12, 82, 184));
        }
    }

    let border = Stroke::new(1.0_f32, Color32::from_rgb(130, 135, 145));
    painter.line_segment([rect.left_top(), rect.right_top()], border);
    painter.line_segment([rect.right_top(), rect.right_bottom()], border);
    painter.line_segment([rect.right_bottom(), rect.left_bottom()], border);
    painter.line_segment([rect.left_bottom(), rect.left_top()], border);
}

fn draw_idm_connections_table(ui: &mut Ui, task: &DownloadStateMeta, _progress: f32) {
    egui::Frame::new()
        .fill(Color32::from_rgb(179, 175, 184))
        .stroke(Stroke::new(1.0_f32, Color32::from_rgb(140, 140, 140)))
        .show(ui, |ui| {
            // White Header Row: N. | Byte Range | Downloaded | Rate | Info
            egui::Frame::new()
                .fill(Color32::WHITE)
                .inner_margin(egui::Margin::symmetric(6, 3))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.add_sized(
                            [28.0, 18.0],
                            egui::Label::new(RichText::new("N.").size(11.5).strong()),
                        );
                        ui.add_sized(
                            [135.0, 18.0],
                            egui::Label::new(RichText::new("Byte Range").size(11.5).strong()),
                        );
                        ui.add_sized(
                            [95.0, 18.0],
                            egui::Label::new(RichText::new("Downloaded").size(11.5).strong()),
                        );
                        ui.add_sized(
                            [90.0, 18.0],
                            egui::Label::new(RichText::new("Rate").size(11.5).strong()),
                        );
                        ui.add_sized(
                            [145.0, 18.0],
                            egui::Label::new(RichText::new("Status / Info").size(11.5).strong()),
                        );
                    });
                });

            egui::ScrollArea::vertical()
                .max_height(125.0)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing = Vec2::new(0.0, 2.0);
                    if task.segments.is_empty() {
                        ui.horizontal(|ui| {
                            ui.add_space(8.0);
                            ui.label(
                                RichText::new("1    0..EOF    0 KB    0 KB/sec    Initializing connection...")
                                    .size(11.5)
                                    .color(Color32::BLACK),
                            );
                        });
                        return;
                    }

                    for seg in &task.segments {
                        let range_str = if seg.end_byte == u64::MAX {
                            format!("{}..EOF", format_bytes(seg.start_byte))
                        } else {
                            format!(
                                "{}..{}",
                                format_bytes(seg.start_byte),
                                format_bytes(seg.end_byte)
                            )
                        };
                        let dl_bytes = seg.downloaded_bytes();
                        let info_text = match seg.status {
                            SegmentStatus::Downloading => "Receiving data...",
                            SegmentStatus::Completed => "Complete",
                            SegmentStatus::Paused => "Paused",
                            SegmentStatus::Pending => "Queued / Splitting...",
                            SegmentStatus::Failed => "Retrying / Error",
                        };

                        ui.horizontal(|ui| {
                            ui.add_space(6.0);
                            ui.add_sized(
                                [28.0, 18.0],
                                egui::Label::new(
                                    RichText::new(format!("{}", seg.id + 1))
                                        .size(11.5)
                                        .color(Color32::BLACK),
                                ),
                            );
                            ui.add_sized(
                                [135.0, 18.0],
                                egui::Label::new(
                                    RichText::new(range_str).size(11.0).color(Color32::BLACK),
                                ),
                            );
                            ui.add_sized(
                                [95.0, 18.0],
                                egui::Label::new(
                                    RichText::new(format_kb(dl_bytes))
                                        .size(11.5)
                                        .color(Color32::BLACK),
                                ),
                            );
                            ui.add_sized(
                                [90.0, 18.0],
                                egui::Label::new(
                                    RichText::new(format_speed(seg.last_speed_bps))
                                        .size(11.5)
                                        .color(Color32::BLACK),
                                ),
                            );
                            ui.add_sized(
                                [145.0, 18.0],
                                egui::Label::new(
                                    RichText::new(info_text).size(11.5).color(Color32::BLACK),
                                ),
                            );
                        });
                    }
                });
        });
}

fn draw_live_speed_graph(ui: &mut Ui, task: &DownloadStateMeta) {
    let width = ui.available_width().max(260.0);
    let height = 88.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    let painter = ui.painter();

    painter.rect_filled(rect, 2.0, Color32::from_rgb(18, 26, 36));
    let border_s = Stroke::new(1.0_f32, Color32::from_rgb(70, 85, 105));
    painter.line_segment([rect.left_top(), rect.right_top()], border_s);
    painter.line_segment([rect.right_top(), rect.right_bottom()], border_s);
    painter.line_segment([rect.right_bottom(), rect.left_bottom()], border_s);
    painter.line_segment([rect.left_bottom(), rect.left_top()], border_s);

    // Subtle horizontal grid lines
    for i in 1..4 {
        let y = rect.top() + (rect.height() * (i as f32) / 4.0);
        painter.line_segment(
            [Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)],
            Stroke::new(0.5_f32, Color32::from_rgb(40, 55, 72)),
        );
    }

    if task.speed_history.is_empty() {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "Live speed telemetry curve activates during transfer",
            egui::FontId::proportional(11.5),
            Color32::from_rgb(145, 165, 185),
        );
        return;
    }

    let max_speed = task
        .speed_history
        .iter()
        .map(|p| p.speed_bps)
        .max()
        .unwrap_or(1024)
        .max(1024) as f32;

    let count = task.speed_history.len();
    let step_x = if count > 1 {
        rect.width() / ((count - 1) as f32)
    } else {
        rect.width()
    };

    let mut prev_pt: Option<Pos2> = None;
    for (idx, sample) in task.speed_history.iter().enumerate() {
        let x = rect.left() + (idx as f32) * step_x;
        let norm = (sample.speed_bps as f32 / max_speed).clamp(0.0, 1.0);
        let y = rect.bottom() - 4.0 - norm * (rect.height() - 10.0);
        let pt = Pos2::new(x, y);
        if let Some(prev) = prev_pt {
            painter.line_segment(
                [prev, pt],
                Stroke::new(1.8_f32, Color32::from_rgb(72, 219, 134)),
            );
        }
        prev_pt = Some(pt);
    }

    painter.text(
        Pos2::new(rect.left() + 8.0, rect.top() + 10.0),
        egui::Align2::LEFT_CENTER,
        format!("Peak window: {}", format_speed(max_speed as u64)),
        egui::FontId::monospace(10.5),
        Color32::from_rgb(175, 225, 195),
    );
}

fn opt_non_empty(s: &str) -> Option<String> {
    let t = s.trim();
    if t.is_empty() {
        None
    } else {
        Some(t.to_owned())
    }
}

fn format_last_try_date(dt: chrono::DateTime<chrono::Utc>) -> String {
    let local = dt.with_timezone(&chrono::Local);
    let today = chrono::Local::now().date_naive();
    if local.date_naive() == today {
        format!("Today {}", local.format("%H:%M:%S"))
    } else {
        local.format("%Y-%m-%d %H:%M").to_string()
    }
}

fn table_status_text(task: &DownloadStateMeta) -> String {
    match task.status {
        DownloadStatus::Completed => "Complete".to_owned(),
        DownloadStatus::Downloading | DownloadStatus::Paused if task.total_size > 0 => {
            let pct =
                (task.total_downloaded() as f64 / task.total_size as f64 * 100.0).clamp(0.0, 100.0);
            format!("{pct:.2}%")
        }
        other => status_label(other).to_owned(),
    }
}

fn idm_dialog_status_label(status: DownloadStatus) -> &'static str {
    match status {
        DownloadStatus::Queued => "Connecting...",
        DownloadStatus::Downloading => "Receiving data...",
        DownloadStatus::Paused => "Paused",
        DownloadStatus::NeedsUrlRefresh => "Link expired",
        DownloadStatus::RefreshingUrl => "Refreshing link...",
        DownloadStatus::Assembling => "Building file...",
        DownloadStatus::VerifyingChecksum => "Verifying checksum...",
        DownloadStatus::Completed => "Complete",
        DownloadStatus::Failed => "Error downloading",
    }
}

fn format_speed(bps: u64) -> String {
    if bps == 0 {
        return "0 KB/sec".to_owned();
    }
    if bps >= 1024 * 1024 {
        format!("{:.3} MB/sec", bps as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.1} KB/sec", bps as f64 / 1024.0)
    }
}

fn format_kb(bytes: u64) -> String {
    let kb = bytes / 1024;
    let s = kb.to_string();
    let mut out = String::new();
    for (count, ch) in s.chars().rev().enumerate() {
        if count > 0 && count % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    let formatted: String = out.chars().rev().collect();
    format!("{formatted} KB")
}

fn truncate_text(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        text.to_owned()
    } else {
        let prefix: String = text.chars().take(max_chars.saturating_sub(3)).collect();
        format!("{prefix}...")
    }
}

fn status_label(status: DownloadStatus) -> &'static str {
    match status {
        DownloadStatus::Queued => "Queued",
        DownloadStatus::Downloading => "Receiving data",
        DownloadStatus::Paused => "Paused",
        DownloadStatus::NeedsUrlRefresh => "Link expired",
        DownloadStatus::RefreshingUrl => "Refreshing link",
        DownloadStatus::Assembling => "Assembling",
        DownloadStatus::VerifyingChecksum => "Verifying",
        DownloadStatus::Completed => "Complete",
        DownloadStatus::Failed => "Failed",
    }
}

fn category_label(category: FileCategory) -> &'static str {
    match category {
        FileCategory::Video => "Video",
        FileCategory::Music => "Music",
        FileCategory::Compressed => "Compressed",
        FileCategory::Documents => "Documents",
        FileCategory::Programs => "Programs",
        FileCategory::General => "General",
    }
}

fn time_left(task: &DownloadStateMeta) -> String {
    if task.status == DownloadStatus::Completed {
        return "—".to_owned();
    }
    if task.status != DownloadStatus::Downloading {
        return "—".to_owned();
    }
    let remaining = task.total_size.saturating_sub(task.total_downloaded());
    let speed: u64 = task
        .segments
        .iter()
        .map(|segment| segment.last_speed_bps)
        .sum();
    if speed == 0 {
        return "Calculating".to_owned();
    }
    let seconds = remaining / speed;
    if seconds >= 3600 {
        format!("{} h {} m", seconds / 3600, (seconds % 3600) / 60)
    } else if seconds >= 60 {
        format!("{} m {} s", seconds / 60, seconds % 60)
    } else {
        format!("{seconds} sec")
    }
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.2} {}", UNITS[unit])
    }
}

fn download_directory() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Downloads")
        .join("RustIDM")
}
