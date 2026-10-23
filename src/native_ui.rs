use crate::{
    AppState, ControlTaskPayload, DownloadStateMeta, DownloadStatus, FileCategory,
    InterceptDownloadPayload,
};
use anyhow::{anyhow, Context, Result};
use eframe::egui::{self, Align, Color32, Layout, RichText, Stroke, Vec2};
use reqwest::blocking::Client;
use std::{
    path::PathBuf,
    sync::mpsc::{self, Receiver, RecvTimeoutError, Sender},
    thread,
    time::{Duration, Instant},
};
use tokio::runtime::Builder;

const API_BASE: &str = "http://127.0.0.1:16888/api/v1";

enum UiCommand {
    AddDownload(String),
    Refresh,
    Control {
        task_id: String,
        action: &'static str,
    },
    SetSpeedLimit(u64),
}

enum UiEvent {
    Downloads(Vec<DownloadStateMeta>),
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

struct RustIdmApp {
    commands: Sender<UiCommand>,
    events: Receiver<UiEvent>,
    downloads: Vec<DownloadStateMeta>,
    selected_task: Option<String>,
    filter: DownloadFilter,
    show_add_dialog: bool,
    show_details: bool,
    new_url: String,
    error_message: Option<String>,
    status_message: String,
    speed_limit_kbps: u64,
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
            show_add_dialog: false,
            show_details: false,
            new_url: String::new(),
            error_message: None,
            status_message: "Connecting to RustIDM engine...".to_owned(),
            speed_limit_kbps: 0,
        }
    }
}

impl eframe::App for RustIdmApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.receive_events(ctx);
        self.draw_menu_bar(ctx);
        self.draw_toolbar(ctx);
        self.draw_status_bar(ctx);
        self.draw_categories(ctx);
        self.draw_download_table(ctx);
        self.draw_dialogs(ctx);
        ctx.request_repaint_after(Duration::from_millis(200));
    }
}

impl RustIdmApp {
    fn receive_events(&mut self, ctx: &egui::Context) {
        while let Ok(event) = self.events.try_recv() {
            match event {
                UiEvent::Downloads(mut downloads) => {
                    downloads.sort_by(|a, b| b.created_at.cmp(&a.created_at));
                    self.downloads = downloads;
                    if self
                        .selected_task
                        .as_ref()
                        .is_some_and(|id| !self.downloads.iter().any(|task| &task.task_id == id))
                    {
                        self.selected_task = None;
                    }
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
                        self.status_message = "Engine connected".to_owned();
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
        egui::TopBottomPanel::top("classic_menu")
            .exact_height(27.0)
            .show(ctx, |ui| {
                egui::menu::bar(ui, |ui| {
                    ui.menu_button("Tasks", |ui| {
                        if ui.button("Add URL...").clicked() {
                            self.show_add_dialog = true;
                            ui.close_menu();
                        }
                        if ui.button("Resume selected").clicked() {
                            self.control_selected("resume");
                            ui.close_menu();
                        }
                        if ui.button("Pause selected").clicked() {
                            self.control_selected("pause");
                            ui.close_menu();
                        }
                        if ui.button("Cancel selected").clicked() {
                            self.control_selected("cancel");
                            ui.close_menu();
                        }
                    });
                    ui.menu_button("Downloads", |ui| {
                        ui.label("Download folder");
                        ui.label(
                            RichText::new(download_directory().display().to_string())
                                .small()
                                .color(Color32::GRAY),
                        );
                    });
                    ui.menu_button("View", |ui| {
                        if ui.button("All downloads").clicked() {
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
                        ui.label("RustIDM native desktop");
                        ui.label("Download manager built with Rust and egui");
                    });
                });
            });
    }

    fn draw_toolbar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("classic_toolbar")
            .exact_height(76.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    toolbar_button(ui, "+", "Add URL", true, || self.show_add_dialog = true);
                    toolbar_button(ui, "▶", "Resume", self.has_selected(), || {
                        self.control_selected("resume")
                    });
                    toolbar_button(ui, "Ⅱ", "Stop", self.has_selected(), || {
                        self.control_selected("pause")
                    });
                    toolbar_button(ui, "×", "Cancel", self.has_selected(), || {
                        self.control_selected("cancel")
                    });
                    ui.separator();
                    toolbar_button(ui, "↻", "Refresh", true, || {
                        self.status_message = "Refreshing download list...".to_owned();
                        let _ = self.commands.send(UiCommand::Refresh);
                    });
                    ui.separator();
                    ui.vertical(|ui| {
                        ui.label(RichText::new("Speed limit").small().color(Color32::GRAY));
                        ui.horizontal(|ui| {
                            let mut limit = self.speed_limit_kbps as f32;
                            ui.add(egui::Slider::new(&mut limit, 0.0..=20_480.0).show_value(false));
                            if ui.button("Apply").clicked() {
                                self.speed_limit_kbps = limit.round() as u64;
                                let _ = self
                                    .commands
                                    .send(UiCommand::SetSpeedLimit(self.speed_limit_kbps));
                            }
                        });
                        ui.label(if self.speed_limit_kbps == 0 {
                            "Unlimited".to_owned()
                        } else {
                            format!("{} KB/s", self.speed_limit_kbps)
                        });
                    });
                });
            });
    }

    fn draw_status_bar(&self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("status_bar")
            .exact_height(26.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let active = self
                        .downloads
                        .iter()
                        .filter(|task| task.status == DownloadStatus::Downloading)
                        .count();
                    let downloaded: u64 = self
                        .downloads
                        .iter()
                        .map(DownloadStateMeta::total_downloaded)
                        .sum();
                    ui.label(format!(
                        "{} downloads  |  {} active  |  {} transferred",
                        self.downloads.len(),
                        active,
                        format_bytes(downloaded)
                    ));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new(&self.status_message)
                                .small()
                                .color(Color32::DARK_GRAY),
                        );
                    });
                });
            });
    }

    fn draw_categories(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("categories")
            .resizable(true)
            .default_width(190.0)
            .min_width(150.0)
            .show(ctx, |ui| {
                ui.heading("Categories");
                ui.separator();
                filter_item(
                    ui,
                    "All downloads",
                    self.count_filter(DownloadFilter::All),
                    self.filter == DownloadFilter::All,
                    || self.filter = DownloadFilter::All,
                );
                filter_item(
                    ui,
                    "Unfinished",
                    self.count_filter(DownloadFilter::Unfinished),
                    self.filter == DownloadFilter::Unfinished,
                    || self.filter = DownloadFilter::Unfinished,
                );
                filter_item(
                    ui,
                    "Finished",
                    self.count_filter(DownloadFilter::Finished),
                    self.filter == DownloadFilter::Finished,
                    || self.filter = DownloadFilter::Finished,
                );
                ui.add_space(12.0);
                ui.label(RichText::new("File categories").strong());
                for (category, label) in [
                    (FileCategory::Compressed, "Compressed"),
                    (FileCategory::Documents, "Documents"),
                    (FileCategory::Music, "Music"),
                    (FileCategory::Programs, "Programs"),
                    (FileCategory::Video, "Video"),
                    (FileCategory::General, "General"),
                ] {
                    let filter = DownloadFilter::Category(category);
                    filter_item(
                        ui,
                        label,
                        self.count_filter(filter),
                        self.filter == filter,
                        || self.filter = filter,
                    );
                }
                ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
                    ui.separator();
                    ui.label(
                        RichText::new(format!("Save to\n{}", download_directory().display()))
                            .small()
                            .color(Color32::GRAY),
                    );
                });
            });
    }

    fn draw_download_table(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            let downloads = self
                .filtered_downloads()
                .into_iter()
                .cloned()
                .collect::<Vec<_>>();
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Download queue").heading());
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(format!("{} items", downloads.len()));
                    });
                });
            });

            ui.add_space(5.0);
            egui::ScrollArea::both()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let available = ui.available_width().max(660.0);
                    let columns = [
                        ("File name", available * 0.31),
                        ("Size", available * 0.11),
                        ("Status", available * 0.15),
                        ("Progress", available * 0.19),
                        ("Time left", available * 0.11),
                        ("Category", available * 0.13),
                    ];
                    egui::Frame::new()
                        .fill(Color32::from_rgb(235, 239, 244))
                        .stroke(Stroke::new(1.0_f32, Color32::from_rgb(210, 216, 224)))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                for (title, width) in columns {
                                    ui.add_sized(
                                        [width, 25.0],
                                        egui::Label::new(RichText::new(title).strong()),
                                    );
                                }
                            });
                        });

                    for task in &downloads {
                        let selected = self.selected_task.as_deref() == Some(&task.task_id);
                        let background = if selected {
                            Color32::from_rgb(211, 230, 251)
                        } else {
                            Color32::WHITE
                        };
                        let clicked = egui::Frame::new()
                            .fill(background)
                            .stroke(Stroke::new(0.5_f32, Color32::from_rgb(225, 229, 235)))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    let filename = ui.add_sized(
                                        [columns[0].1, 27.0],
                                        egui::Button::new(&task.filename).selected(selected),
                                    );
                                    ui.add_sized(
                                        [columns[1].1, 27.0],
                                        egui::Label::new(format_bytes(task.total_size)),
                                    );
                                    ui.add_sized(
                                        [columns[2].1, 27.0],
                                        egui::Label::new(status_label(task.status)),
                                    );
                                    let progress = if task.total_size == 0 {
                                        0.0
                                    } else {
                                        (task.total_downloaded() as f32 / task.total_size as f32)
                                            .clamp(0.0, 1.0)
                                    };
                                    ui.allocate_ui_with_layout(
                                        Vec2::new(columns[3].1, 27.0),
                                        Layout::left_to_right(Align::Center),
                                        |ui| {
                                            ui.add(
                                                egui::ProgressBar::new(progress)
                                                    .desired_width(columns[3].1 - 8.0)
                                                    .text(format!("{:.0}%", progress * 100.0)),
                                            );
                                        },
                                    );
                                    ui.add_sized(
                                        [columns[4].1, 27.0],
                                        egui::Label::new(time_left(task)),
                                    );
                                    ui.add_sized(
                                        [columns[5].1, 27.0],
                                        egui::Label::new(category_label(task.category)),
                                    );
                                    (filename.clicked(), filename.double_clicked())
                                })
                                .inner
                            })
                            .inner;
                        if clicked.0 {
                            self.selected_task = Some(task.task_id.clone());
                        }
                        if clicked.1 {
                            self.show_details = true;
                        }
                    }
                    if downloads.is_empty() {
                        ui.add_space(20.0);
                        ui.vertical_centered(|ui| {
                            ui.label(
                                "No downloads in this view. Choose Add URL to start a download.",
                            );
                        });
                    }
                });

            if let Some(error) = &self.error_message {
                ui.add_space(4.0);
                ui.colored_label(Color32::DARK_RED, error);
            }
        });
    }

    fn draw_dialogs(&mut self, ctx: &egui::Context) {
        if self.show_add_dialog {
            let mut open = self.show_add_dialog;
            let mut submitted = false;
            egui::Window::new("Add URL")
                .collapsible(false)
                .resizable(false)
                .default_width(480.0)
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.label("Enter the URL of the file to download:");
                    let response = ui.add(
                        egui::TextEdit::singleline(&mut self.new_url)
                            .hint_text("https://example.com/file.zip")
                            .desired_width(440.0),
                    );
                    let submit = ui.button("Start download").clicked()
                        || response.lost_focus()
                            && ui.input(|input| input.key_pressed(egui::Key::Enter));
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
            self.show_add_dialog = open && !submitted;
        }

        if self.show_details {
            if let Some(task) = self.selected_download().cloned() {
                let mut open = self.show_details;
                egui::Window::new(format!("Download status - {}", task.filename))
                    .collapsible(false)
                    .resizable(true)
                    .default_width(540.0)
                    .open(&mut open)
                    .show(ctx, |ui| {
                        ui.label(RichText::new(&task.url).small().color(Color32::DARK_GRAY));
                        ui.separator();
                        egui::Grid::new("download_details")
                            .num_columns(2)
                            .spacing([20.0, 8.0])
                            .show(ui, |ui| {
                                detail_row(ui, "Status", status_label(task.status));
                                detail_row(ui, "File size", format_bytes(task.total_size));
                                detail_row(ui, "Downloaded", format_bytes(task.total_downloaded()));
                                detail_row(
                                    ui,
                                    "Resume capability",
                                    if task.resumable { "Yes" } else { "No" },
                                );
                                detail_row(ui, "Connections", task.segments.len().to_string());
                                detail_row(
                                    ui,
                                    "Save location",
                                    task.final_file_path().display().to_string(),
                                );
                            });
                        let progress = if task.total_size == 0 {
                            0.0
                        } else {
                            (task.total_downloaded() as f32 / task.total_size as f32)
                                .clamp(0.0, 1.0)
                        };
                        ui.add(
                            egui::ProgressBar::new(progress)
                                .desired_width(ui.available_width())
                                .text(format!("{:.1}%", progress * 100.0)),
                        );
                        if let Some(error) = task.last_error {
                            ui.colored_label(Color32::DARK_RED, error);
                        }
                        ui.horizontal(|ui| {
                            if ui.button("Resume").clicked() {
                                let _ = self.commands.send(UiCommand::Control {
                                    task_id: task.task_id.clone(),
                                    action: "resume",
                                });
                            }
                            if ui.button("Pause").clicked() {
                                let _ = self.commands.send(UiCommand::Control {
                                    task_id: task.task_id.clone(),
                                    action: "pause",
                                });
                            }
                            if ui.button("Cancel").clicked() {
                                let _ = self.commands.send(UiCommand::Control {
                                    task_id: task.task_id.clone(),
                                    action: "cancel",
                                });
                            }
                        });
                    });
                self.show_details = open;
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

    fn count_filter(&self, filter: DownloadFilter) -> usize {
        self.downloads
            .iter()
            .filter(|task| match filter {
                DownloadFilter::All => true,
                DownloadFilter::Unfinished => task.status != DownloadStatus::Completed,
                DownloadFilter::Finished => task.status == DownloadStatus::Completed,
                DownloadFilter::Category(category) => task.category == category,
            })
            .count()
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
        let mut next_poll = Instant::now();
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
        UiCommand::Refresh => client
            .get(format!("{API_BASE}/downloads"))
            .send()
            .and_then(reqwest::blocking::Response::error_for_status)
            .and_then(|response| response.json::<Vec<DownloadStateMeta>>())
            .map(|downloads| {
                let _ = events.send(UiEvent::Downloads(downloads));
                "Download list refreshed".to_owned()
            })
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
            .with_inner_size([1160.0, 760.0])
            .with_min_inner_size([850.0, 560.0])
            .with_title("RustIDM Download Manager"),
        ..Default::default()
    };
    eframe::run_native(
        "RustIDM Download Manager",
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

fn toolbar_button(
    ui: &mut egui::Ui,
    icon: &str,
    label: &str,
    enabled: bool,
    mut action: impl FnMut(),
) {
    ui.add_enabled_ui(enabled, |ui| {
        if ui
            .add_sized(
                [78.0, 62.0],
                egui::Button::new(
                    egui::RichText::new(format!("{icon}\n{label}"))
                        .size(13.0)
                        .color(Color32::from_rgb(28, 62, 103)),
                ),
            )
            .clicked()
        {
            action();
        }
    });
}

fn filter_item(
    ui: &mut egui::Ui,
    label: &str,
    count: usize,
    selected: bool,
    mut select: impl FnMut(),
) {
    let response = ui.add_sized(
        [ui.available_width(), 25.0],
        egui::Button::new(format!(
            "{}{}{}",
            if selected { "▸ " } else { "  " },
            label,
            format!(" ({count})")
        ))
        .selected(selected),
    );
    if response.clicked() {
        select();
    }
}

fn detail_row(ui: &mut egui::Ui, label: &str, value: impl ToString) {
    ui.label(RichText::new(label).strong());
    ui.label(value.to_string());
    ui.end_row();
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
