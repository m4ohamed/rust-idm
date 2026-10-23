use crate::{
    AppState, ControlTaskPayload, DownloadStateMeta, DownloadStatus, FileCategory,
    InterceptDownloadPayload, QueueSchedule, ScheduleMode,
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
    AddDownload(String),
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
    Schedule(Result<QueueSchedule, String>),
    CompletedNotice(String),
    Status(Result<String, String>),
    EngineConnection(Result<(), String>),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DownloadFilter {
    All,
    Unfinished,
    Finished,
    Category(FileCategory),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DialogTab {
    DownloadStatus,
    SpeedLimiter,
    OptionsOnCompletion,
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
    TellFriend,
}

struct RustIdmApp {
    commands: Sender<UiCommand>,
    events: Receiver<UiEvent>,
    downloads: Vec<DownloadStateMeta>,
    selected_task: Option<String>,
    filter: DownloadFilter,
    show_categories_panel: bool,
    expand_all_downloads: bool,
    expand_unfinished: bool,
    expand_finished: bool,
    show_add_dialog: bool,
    show_options_dialog: bool,
    show_scheduler_dialog: bool,
    show_details: bool,
    show_connection_details: bool,
    dialog_tab: DialogTab,
    close_on_completion: bool,
    open_on_completion: bool,
    new_url: String,
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
            selected_task: None,
            filter: DownloadFilter::All,
            show_categories_panel: true,
            expand_all_downloads: true,
            expand_unfinished: false,
            expand_finished: false,
            show_add_dialog: false,
            show_options_dialog: false,
            show_scheduler_dialog: false,
            show_details: false,
            show_connection_details: true,
            dialog_tab: DialogTab::DownloadStatus,
            close_on_completion: false,
            open_on_completion: false,
            new_url: String::new(),
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
                                egui::Button::new("Download status..."),
                            )
                            .clicked()
                        {
                            self.show_details = true;
                            ui.close_menu();
                        }
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
                        if ui.button("Scheduler...").clicked() {
                            self.show_scheduler_dialog = true;
                            ui.close_menu();
                        }
                        if ui.button("Speed Limiter...").clicked() {
                            self.dialog_tab = DialogTab::SpeedLimiter;
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
                            .checkbox(&mut self.show_categories_panel, "Categories")
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
                        ui.label("  RustIDM v1.0.0");
                        ui.label("Powered by RustIDM Engine");
                    });
                    ui.menu_button("Registration", |ui| {
                        ui.label("Registered Edition (Open Source RustIDM)");
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
                        "Delete C...",
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
                    if idm_toolbar_button(
                        ui,
                        ToolbarIcon::TellFriend,
                        "Tell a Frie...",
                        true,
                        false,
                    ) {
                        ui.ctx().copy_text(
                            "Try RustIDM, the Rust download manager: https://github.com/m4ohamed/rust-idm"
                                .to_owned(),
                        );
                        self.status_message = "RustIDM link copied to clipboard".to_owned();
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
                                    ("File Name", available * 0.28),
                                    ("Q", 28.0),
                                    ("Size", available * 0.11),
                                    ("Status", available * 0.12),
                                    ("Time left", available * 0.11),
                                    ("Transfer rate", available * 0.12),
                                    ("Last Try Date", available * 0.12),
                                    ("Description", (available * 0.14 - 28.0).max(80.0)),
                                ];

                                // Table Header
                                egui::Frame::new()
                                    .fill(Color32::from_rgb(247, 248, 250))
                                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(215, 215, 215)))
                                    .show(ui, |ui| {
                                        ui.spacing_mut().item_spacing = Vec2::ZERO;
                                        ui.horizontal(|ui| {
                                            for (title, width) in columns {
                                                let (rect, _) = ui.allocate_exact_size(
                                                    Vec2::new(width, 23.0),
                                                    Sense::hover(),
                                                );
                                                let painter = ui.painter();
                                                painter.line_segment(
                                                    [rect.right_top(), rect.right_bottom()],
                                                    Stroke::new(
                                                        1.0_f32,
                                                        Color32::from_rgb(220, 220, 220),
                                                    ),
                                                );
                                                painter.text(
                                                    Pos2::new(rect.left() + 6.0, rect.center().y),
                                                    egui::Align2::LEFT_CENTER,
                                                    title,
                                                    egui::FontId::proportional(12.0),
                                                    Color32::from_rgb(30, 30, 30),
                                                );
                                            }
                                        });
                                    });

                                // Table Rows
                                ui.spacing_mut().item_spacing = Vec2::ZERO;
                                for task in &downloads {
                                    let selected =
                                        self.selected_task.as_deref() == Some(&task.task_id);
                                    let row_bg = if selected {
                                        Color32::from_rgb(205, 232, 255)
                                    } else {
                                        Color32::WHITE
                                    };

                                    let total_width: f32 = columns.iter().map(|(_, w)| *w).sum();
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
                                        truncate_text(&task.filename, 34),
                                        egui::FontId::proportional(12.0),
                                        Color32::BLACK,
                                    );
                                    x += w0;

                                    // Col 1: Q (Queue icon)
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

                                    // Col 3: Status (IDM shows percentage when active/paused with size)
                                    let w3 = columns[3].1;
                                    let status_str = table_status_text(task);
                                    painter.text(
                                        Pos2::new(x + 6.0, cy),
                                        egui::Align2::LEFT_CENTER,
                                        status_str,
                                        egui::FontId::proportional(12.0),
                                        Color32::BLACK,
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

                                    // Col 6: Last Try Date
                                    let w6 = columns[6].1;
                                    painter.text(
                                        Pos2::new(x + 6.0, cy),
                                        egui::Align2::LEFT_CENTER,
                                        "Today",
                                        egui::FontId::proportional(12.0),
                                        Color32::from_rgb(40, 40, 40),
                                    );
                                    x += w6;

                                    // Col 7: Description
                                    painter.text(
                                        Pos2::new(x + 6.0, cy),
                                        egui::Align2::LEFT_CENTER,
                                        category_label(task.category),
                                        egui::FontId::proportional(12.0),
                                        Color32::from_rgb(70, 70, 70),
                                    );

                                    // Vertical grid lines
                                    let mut gx = row_rect.left();
                                    for (_, cw) in columns {
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
                                }
                            });
                    });
            });
    }

    fn draw_dialogs(&mut self, ctx: &egui::Context) {
        // 1. Add URL Dialog
        if self.show_add_dialog {
            let mut open = self.show_add_dialog;
            let mut submitted = false;
            egui::Window::new("Enter new address to download")
                .collapsible(false)
                .resizable(false)
                .default_width(480.0)
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Address:");
                        let response = ui.add(
                            egui::TextEdit::singleline(&mut self.new_url)
                                .hint_text("https://example.com/file.zip")
                                .desired_width(340.0),
                        );
                        let submit = ui.button("OK").clicked()
                            || (response.lost_focus()
                                && ui.input(|input| input.key_pressed(egui::Key::Enter)));
                        if submit {
                            let url = self.new_url.trim().to_owned();
                            if !url.is_empty() {
                                match self.commands.send(UiCommand::AddDownload(url)) {
                                    Ok(()) => {
                                        self.status_message = "Adding download...".to_owned();
                                        self.new_url.clear();
                                        submitted = true;
                                    }
                                    Err(error) => self.error_message = Some(error.to_string()),
                                }
                            }
                        }
                    });
                });
            self.show_add_dialog = open && !submitted;
        }

        // 2. Options / Speed Limiter Dialog
        if self.show_options_dialog {
            let mut open = self.show_options_dialog;
            egui::Window::new(" RustIDM Configuration")
                .collapsible(false)
                .resizable(false)
                .default_width(420.0)
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.label(RichText::new("Download Directory").strong());
                    ui.label(download_directory().display().to_string());
                    ui.separator();
                    ui.label(RichText::new("Global Speed Limiter").strong());
                    ui.horizontal(|ui| {
                        let mut limit = self.speed_limit_kbps as f32;
                        ui.add(
                            egui::Slider::new(&mut limit, 0.0..=20_480.0)
                                .text("KB/s (0 = Unlimited)"),
                        );
                        self.speed_limit_kbps = limit.round() as u64;
                        if ui.button("Apply").clicked() {
                            let _ = self
                                .commands
                                .send(UiCommand::SetSpeedLimit(self.speed_limit_kbps));
                        }
                    });
                });
            self.show_options_dialog = open;
        }

        // 3. Scheduler Dialog
        if self.show_scheduler_dialog {
            let mut open = self.show_scheduler_dialog;
            let mut schedule = self.schedule.clone();
            egui::Window::new("Scheduler - Main Download Queue")
                .collapsible(false)
                .resizable(false)
                .default_width(420.0)
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.label("Queue: Main Download Queue");
                    ui.separator();
                    egui::ComboBox::from_label("Schedule")
                        .selected_text(match schedule.mode {
                            ScheduleMode::Disabled => "Disabled",
                            ScheduleMode::Daily => "Daily start and stop",
                            ScheduleMode::Once => "Start once",
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut schedule.mode,
                                ScheduleMode::Disabled,
                                "Disabled",
                            );
                            ui.selectable_value(
                                &mut schedule.mode,
                                ScheduleMode::Daily,
                                "Daily start and stop",
                            );
                            ui.selectable_value(
                                &mut schedule.mode,
                                ScheduleMode::Once,
                                "Start once",
                            );
                        });

                    match schedule.mode {
                        ScheduleMode::Disabled => {
                            ui.label("The queue will follow manual Start/Stop controls.");
                        }
                        ScheduleMode::Daily => {
                            let start = schedule.start_time.get_or_insert_with(|| "09:00".into());
                            ui.horizontal(|ui| {
                                ui.label("Start time (local, HH:MM):");
                                ui.text_edit_singleline(start);
                            });
                            let stop = schedule.stop_time.get_or_insert_with(|| "17:00".into());
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

                    ui.horizontal(|ui| {
                        if ui.button("Save schedule").clicked() {
                            self.schedule = schedule.clone();
                            self.status_message = "Saving schedule...".to_owned();
                            if let Err(error) =
                                self.commands.send(UiCommand::SetSchedule(schedule.clone()))
                            {
                                self.error_message = Some(error.to_string());
                            }
                        }
                        if ui.button("Disable").clicked() {
                            schedule = QueueSchedule::default();
                            self.schedule = schedule.clone();
                            if let Err(error) =
                                self.commands.send(UiCommand::SetSchedule(schedule.clone()))
                            {
                                self.error_message = Some(error.to_string());
                            }
                        }
                    });
                    ui.separator();
                    ui.horizontal(|ui| {
                        if ui.button("Start Now").clicked() {
                            self.start_queue_downloads();
                        }
                        if ui.button("Stop Now").clicked() {
                            self.stop_all_downloads();
                        }
                    });
                });
            self.schedule = schedule;
            self.show_scheduler_dialog = open;
        }

        // 4. Classic IDM Download Progress Window (Matches Image 1 & Image 2)
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
                    .default_width(530.0)
                    .open(&mut open)
                    .show(ctx, |ui| {
                        ui.set_min_width(515.0);

                        // Top Tabs: Download status | Speed Limiter | Options on completion
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing = Vec2::new(2.0, 0.0);
                            for (tab, title) in [
                                (DialogTab::DownloadStatus, "Download status"),
                                (DialogTab::SpeedLimiter, "Speed Limiter"),
                                (DialogTab::OptionsOnCompletion, "Options on completion"),
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
                                // White Status Information Box
                                egui::Frame::new()
                                    .fill(Color32::WHITE)
                                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(200, 200, 200)))
                                    .inner_margin(egui::Margin::symmetric(14, 10))
                                    .show(ui, |ui| {
                                        ui.set_min_width(ui.available_width());
                                        ui.label(
                                            RichText::new(truncate_text(&task.url, 68))
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

                                ui.add_space(8.0);

                                // Classic IDM Solid Green Progress Bar
                                draw_idm_green_progress_bar(ui, progress);

                                ui.add_space(8.0);

                                // Action Buttons Row: [<< Hide details / Show details >>] ... [Pause] [Cancel]
                                ui.horizontal(|ui| {
                                    let toggle_label = if self.show_connection_details {
                                        "<< Hide details"
                                    } else {
                                        "Show details >>"
                                    };
                                    if ui
                                        .add_sized([138.0, 24.0], egui::Button::new(toggle_label))
                                        .clicked()
                                    {
                                        self.show_connection_details =
                                            !self.show_connection_details;
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
                                        ui.add_space(8.0);
                                        let is_paused = task.status == DownloadStatus::Paused;
                                        let pause_label = if is_paused { "Resume" } else { "Pause" };
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

                                // Expanded Details Section (Image 2)
                                if self.show_connection_details {
                                    ui.add_space(6.0);
                                    ui.vertical_centered(|ui| {
                                        ui.label(
                                            RichText::new(
                                                "Start positions and download progress by connections",
                                            )
                                            .size(12.0)
                                            .color(Color32::BLACK),
                                        );
                                    });
                                    ui.add_space(2.0);

                                    // Segmented Connections Bar with Red Markers & Glossy Blue Fill
                                    let conn_count = task.segments.len().max(8);
                                    draw_idm_connections_bar(ui, conn_count, progress);

                                    ui.add_space(8.0);

                                    // Connections Table with Classic IDM Slate-Taupe Background
                                    draw_idm_connections_table(ui, &task, conn_count, progress);
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
                                            "Use Speed Limiter",
                                        );
                                        ui.add_space(8.0);
                                        ui.horizontal(|ui| {
                                            ui.label("Maximum download speed (KB/sec):");
                                            let mut limit = self.speed_limit_kbps as f32;
                                            ui.add(
                                                egui::Slider::new(&mut limit, 0.0..=20_480.0)
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
                                            "Show download complete dialog",
                                        );
                                        ui.add_space(8.0);
                                        ui.label(format!(
                                            "Save location: {}",
                                            task.final_file_path().display()
                                        ));
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
        self.downloads
            .iter()
            .filter(|task| match self.filter {
                DownloadFilter::All => true,
                DownloadFilter::Unfinished => task.status != DownloadStatus::Completed,
                DownloadFilter::Finished => task.status == DownloadStatus::Completed,
                DownloadFilter::Category(category) => task.category == category,
            })
            .collect()
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
        let client = match Client::builder().timeout(Duration::from_secs(12)).build() {
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
                next_poll = Instant::now() + Duration::from_secs(1);
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
                next_notification_poll = Instant::now() + Duration::from_secs(2);
            }
        }
    });
}

fn process_command(client: &Client, command: UiCommand, events: &Sender<UiEvent>) {
    let result = match command {
        UiCommand::AddDownload(url) => {
            let payload = InterceptDownloadPayload {
                url,
                source_page_url: None,
                filename: None,
                cookies: None,
                referer: None,
                user_agent: None,
                queue_name: Some("main".to_owned()),
                expected_sha256: None,
            };
            client
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
                .map_err(|error| error.to_string())
        }
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
            .with_title("RustIDM v1.0.0"),
        ..Default::default()
    };
    eframe::run_native(
        "RustIDM v1.0.0",
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

fn draw_idm_connections_bar(ui: &mut Ui, conn_count: usize, overall_progress: f32) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 18.0), Sense::hover());
    let painter = ui.painter();

    painter.rect_filled(rect, 0.0, Color32::from_rgb(238, 240, 245));

    let n = conn_count.max(1);
    let slot_w = rect.width() / (n as f32);

    for i in 0..n {
        let slot_left = rect.left() + (i as f32) * slot_w;
        let seg_prog = overall_progress.clamp(0.0, 1.0);
        let fill_w = (slot_w * seg_prog).max(0.0);

        if fill_w > 0.0 {
            let top_half = Rect::from_min_max(
                Pos2::new(slot_left, rect.top()),
                Pos2::new(slot_left + fill_w, rect.center().y),
            );
            let bot_half = Rect::from_min_max(
                Pos2::new(slot_left, rect.center().y),
                Pos2::new(slot_left + fill_w, rect.bottom()),
            );
            painter.rect_filled(top_half, 0.0, Color32::from_rgb(65, 135, 225));
            painter.rect_filled(bot_half, 0.0, Color32::from_rgb(12, 82, 184));
        }

        // Red start-position vertical divider (exact IDM detail)
        if i > 0 {
            painter.line_segment(
                [
                    Pos2::new(slot_left, rect.top()),
                    Pos2::new(slot_left, rect.bottom()),
                ],
                Stroke::new(1.0_f32, Color32::from_rgb(205, 35, 35)),
            );
        }
    }

    let border = Stroke::new(1.0_f32, Color32::from_rgb(130, 135, 145));
    painter.line_segment([rect.left_top(), rect.right_top()], border);
    painter.line_segment([rect.right_top(), rect.right_bottom()], border);
    painter.line_segment([rect.right_bottom(), rect.left_bottom()], border);
    painter.line_segment([rect.left_bottom(), rect.left_top()], border);
}

fn draw_idm_connections_table(
    ui: &mut Ui,
    task: &DownloadStateMeta,
    conn_count: usize,
    progress: f32,
) {
    egui::Frame::new()
        .fill(Color32::from_rgb(179, 175, 184))
        .stroke(Stroke::new(1.0_f32, Color32::from_rgb(140, 140, 140)))
        .show(ui, |ui| {
            // White Header Row: N. | Downloaded | Info
            egui::Frame::new()
                .fill(Color32::WHITE)
                .inner_margin(egui::Margin::symmetric(6, 3))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.add_sized(
                            [36.0, 18.0],
                            egui::Label::new(RichText::new("N.").size(12.0)),
                        );
                        ui.add_sized(
                            [135.0, 18.0],
                            egui::Label::new(RichText::new("Downloaded").size(12.0)),
                        );
                        ui.add_sized(
                            [280.0, 18.0],
                            egui::Label::new(RichText::new("Info").size(12.0)),
                        );
                    });
                });

            let per_conn_bytes = if conn_count > 0 {
                task.total_downloaded() / (conn_count as u64)
            } else {
                0
            };

            egui::ScrollArea::vertical()
                .max_height(115.0)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing = Vec2::new(0.0, 2.0);
                    for idx in 0..conn_count {
                        let info_text = match task.status {
                            DownloadStatus::Downloading => {
                                if idx == 0 && progress < 0.05 {
                                    "Send GET..."
                                } else {
                                    "Receiving data..."
                                }
                            }
                            DownloadStatus::Completed => "Complete",
                            DownloadStatus::Paused => "Paused",
                            DownloadStatus::Queued => "Connecting...",
                            _ => status_label(task.status),
                        };

                        ui.horizontal(|ui| {
                            ui.add_space(8.0);
                            ui.add_sized(
                                [32.0, 18.0],
                                egui::Label::new(
                                    RichText::new(format!("{}", idx + 1))
                                        .size(12.0)
                                        .color(Color32::BLACK),
                                ),
                            );
                            ui.add_sized(
                                [135.0, 18.0],
                                egui::Label::new(
                                    RichText::new(format_kb(per_conn_bytes))
                                        .size(12.0)
                                        .color(Color32::BLACK),
                                ),
                            );
                            ui.add_sized(
                                [280.0, 18.0],
                                egui::Label::new(
                                    RichText::new(info_text).size(12.0).color(Color32::BLACK),
                                ),
                            );
                        });
                    }
                });
        });
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
