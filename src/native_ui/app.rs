//! التطبيق الرئيسي لواجهة سطح المكتب StormGet (RustIDM سابقاً)
//!
//! يطبق هذا الملف متطلبات المراحل 3، 4، 5، 6، 11، 12، 13، 14، 15، 16، 17:
//! - شريط قوائم قياسي منظم (File, Downloads, View, Tools, Help) مع الاختصارات وحالات التفعيل والتعطيل.
//! - شريط أدوات مرتب حسب الأولوية مع حماية من النقر المزدوج (Debounce) و Tooltips شاملة.
//! - جدول تنزيلات عالي الأداء يدعم التمرير الافتراضي (`show_rows`)، الأعمدة القابلة للإظهار والإخفاء والترتيب، والبحث السريع، والتحديد المتعدد (`Ctrl`, `Shift`, `Ctrl+A`, عكس التحديد).
//! - قوائم نقر بالزر الأيمن مخصصة لكل حالة سياقية (نشط، متوقف مؤقتاً، مكتمل، فاشل/منتهي، تحديد متعدد، مساحة فارغة).
//! - إدارة النوافذ المستقلة وحفظ/استعادة حالة النافذة الرئيسية والنوافذ الثانوية بتوقيت موثوق (Debounced Save + On Exit).

use super::{
    dialogs::{draw_add_url_dialog, draw_all_dialogs, draw_details_dialog},
    i18n::Loc,
    painter::{
        draw_file_category_icon, draw_idm_connections_bar, draw_live_speed_graph,
        draw_queue_mini_icon, draw_status_vector_icon, draw_table_progress_cell,
        stormget_toolbar_button_with_tooltip, tree_child_row_with_count, tree_root_row_with_count,
    },
    theme::{apply_stormget_theme_with_scale, ThemePalette},
    types::{
        AppLanguage, BottomInspectorTab, DialogTab, DownloadFilter, OptionsTab,
        PendingConfirmation, RowDensity, ShutdownAction, SortColumn, TableColumnId, ThemeMode,
        ToastKind, ToastNotification, ToolbarIcon, TreeIconKind, UiCommand, UiEvent,
    },
<<<<<<< HEAD
    window_state::{
        detect_connected_monitors, validate_and_clamp_window_geometry, PersistedUiState,
        SavedWindowGeometry,
    },
=======
>>>>>>> efc40c730b5c3e9a4001ccff0b896d7f2f3ef344
    worker::{open_file, open_file_with, open_folder, reveal_in_explorer, spawn_api_worker},
};
use crate::{
    AppConfig, BatchExpandResponse, CrawlConfig, CrawlReport, DownloadStateMeta, DownloadStatus,
    EngineStatistics, FileCategory, PostQueueAction, ProbeUrlRequestPayload, QueueConfig,
    QueueSchedule, RemoteZipPreviewResponse, ScheduleMode, SegmentStatus, ToolsStatusReport,
};
use eframe::egui::{self, Align, Color32, Layout, Pos2, Rect, RichText, Sense, Stroke, Vec2};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::mpsc::{self, Receiver, Sender},
    time::{Duration, Instant},
};

pub struct RustIdmApp {
    pub commands: Sender<UiCommand>,
    pub events: Receiver<UiEvent>,
    pub downloads: Vec<DownloadStateMeta>,
    pub queues: Vec<QueueConfig>,
    pub app_config: AppConfig,
    pub engine_stats: Option<EngineStatistics>,
    pub tools_status: Option<ToolsStatusReport>,
    // حالة التحديد الفردي والمتعدد (Ctrl / Shift / Ctrl+A / Invert)
    pub selected_task: Option<String>,
    pub selected_tasks: HashSet<String>,
    pub selection_anchor: Option<String>,
    // التصفية والبحث والترتيب
    pub filter: DownloadFilter,
    pub search_query: String,
    pub sort_column: SortColumn,
    pub sort_ascending: bool,
    // المظهر واللغة وحالة الواجهة المحفوظة
    pub theme_mode: ThemeMode,
    pub language: AppLanguage,
    pub ui_state: PersistedUiState,
    pub ui_state_dirty: bool,
    pub last_ui_state_save: Instant,
    pub show_categories_panel: bool,
    pub show_bottom_inspector: bool,
    pub bottom_inspector_tab: BottomInspectorTab,
    pub expand_all_downloads: bool,
    pub expand_unfinished: bool,
    pub expand_finished: bool,
    pub expand_queues: bool,
    // حالات فتح النوافذ المستقلة والحوارية + منع تكرار النوافذ (Focus Requests)
    pub focus_window_requests: HashSet<String>,
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
    pub show_diagnostics_window: bool,
    pub show_rename_dialog: bool,
    pub show_column_chooser_dialog: bool,
    pub show_shortcuts_dialog: bool,
    pub show_about_dialog: bool,
    // حقول المجدول ومحرر الطوابير
    pub selected_queue_name: String,
    pub new_queue_name_input: String,
    pub sched_editor_mode: ScheduleMode,
    pub sched_editor_start_time: String,
    pub sched_editor_stop_time: String,
    pub sched_editor_once_start: String,
    pub sched_editor_once_stop: String,
    pub sched_editor_days: [bool; 7],
    pub sched_editor_max_concurrent: usize,
    pub sched_editor_speed_limit_kbps: u64,
    pub sched_editor_post_action_idx: usize,
    pub sched_validation_error: Option<String>,
    // حقول نافذة تفاصيل التنزيل
    pub show_details: bool,
    pub show_connection_details: bool,
    pub dialog_tab: DialogTab,
    pub close_on_completion: bool,
    pub open_on_completion: bool,
    // حقول نافذة إضافة تنزيل جديد وفحص الرابط المسبق (Probe)
    pub new_url: String,
    pub new_filename: String,
    pub new_category: FileCategory,
    pub new_save_folder: String,
    pub new_remember_path: bool,
    pub new_probed_size_bytes: Option<u64>,
    pub new_probed_resumable: Option<bool>,
    pub new_probed_host: Option<String>,
    pub new_probe_in_progress: bool,
    pub new_probe_error: Option<String>,
    pub new_referer: String,
    pub new_cookies: String,
    pub new_user_agent: String,
    pub new_expected_sha256: String,
    pub new_expected_md5: String,
    pub new_expected_crc32: String,
    pub new_mirrors_text: String,
    pub new_description: String,
    pub new_queue_selection: String,
    pub last_clipboard_url: Option<String>,
    // حقول نافذة إعادة التسمية وتغيير المجلد
    pub rename_task_id: String,
    pub rename_filename_input: String,
    pub rename_save_dir_input: String,
    // خيارات ما بعد الاكتمال ومحدد السرعة
    pub completion_notify: bool,
    pub completion_hangup: bool,
    pub completion_exit_idm: bool,
    pub completion_shutdown_pc: bool,
    pub completion_shutdown_action: ShutdownAction,
    pub completion_force_kill: bool,
    pub speed_limiter_remember: bool,
    // حقول الأدوات المساعدة (Batch, Refresh, Grabber, ZIP, Import/Export, Site Logins)
    pub batch_pattern_input: String,
    pub batch_step_input: usize,
    pub batch_queue_selection: String,
    pub batch_start_immediately: bool,
    pub batch_last_response: Option<BatchExpandResponse>,
    pub refresh_task_id: String,
    pub refresh_new_url: String,
    pub refresh_new_cookies: String,
    pub crawl_config: CrawlConfig,
    pub crawl_extensions_csv: String,
    pub crawl_excluded_csv: String,
    pub crawl_report: Option<CrawlReport>,
    pub crawl_selected_links: HashMap<String, bool>,
    pub crawl_in_progress: bool,
    pub zip_preview_url: String,
    pub zip_preview_cookies: String,
    pub zip_preview_data: Option<RemoteZipPreviewResponse>,
    pub zip_preview_loading: bool,
    pub import_export_text: String,
    pub new_login_domain: String,
    pub new_login_user: String,
    pub new_login_secret: String,
    // الإشعارات الموحدة وحوارات التأكيد والحماية من النقر المزدوج
    pub error_message: Option<String>,
    pub status_message: String,
    pub speed_limit_kbps: u64,
    pub speed_limiter_enabled: bool,
    pub schedule: QueueSchedule,
    pub previous_download_status: HashMap<String, DownloadStatus>,
    pub has_received_downloads: bool,
    pub completed_notice: Option<String>,
    pub pending_intercept_id: Option<String>,
    pub completed_download: Option<DownloadStateMeta>,
<<<<<<< HEAD
    pub pending_confirmation: Option<PendingConfirmation>,
    pub toasts: Vec<ToastNotification>,
    pub next_toast_id: u64,
    pub last_action_instant: Instant,
    pub allow_exit: bool,
    pub game_mode_active: bool,
    pub latest_intelligence: Option<crate::LinkIntelligenceReport>,
    pub latest_doctor_diagnosis: Option<crate::DoctorDiagnosis>,
    pub playlist_entries: Vec<crate::PlaylistEntry>,
=======
    pub allow_exit: bool,
>>>>>>> efc40c730b5c3e9a4001ccff0b896d7f2f3ef344
    #[cfg(windows)]
    pub tray: Option<super::tray::TrayController>,
}

impl Default for RustIdmApp {
    fn default() -> Self {
        Self::from_persisted_state(PersistedUiState::default(), None)
    }
}

impl RustIdmApp {
    pub fn from_persisted_state(
        ui_state: PersistedUiState,
        load_warning: Option<String>,
    ) -> Self {
        let (command_tx, command_rx) = mpsc::channel();
        let (event_tx, event_rx) = mpsc::channel();
        spawn_api_worker(command_rx, event_tx);

        let mut app = Self {
            commands: command_tx,
            events: event_rx,
            downloads: Vec::new(),
            queues: Vec::new(),
            app_config: AppConfig::default(),
            engine_stats: None,
            tools_status: None,
            selected_task: None,
            selected_tasks: HashSet::new(),
            selection_anchor: None,
            filter: DownloadFilter::All,
            search_query: String::new(),
            sort_column: ui_state.sort_column,
            sort_ascending: ui_state.sort_ascending,
            theme_mode: ui_state.theme_mode,
            language: ui_state.language,
            show_categories_panel: ui_state.show_sidebar,
            show_bottom_inspector: ui_state.show_bottom_inspector,
            bottom_inspector_tab: ui_state.bottom_inspector_tab,
            expand_all_downloads: ui_state.expand_all_downloads,
            expand_unfinished: ui_state.expand_unfinished,
            expand_finished: ui_state.expand_finished,
            expand_queues: ui_state.expand_queues,
            focus_window_requests: HashSet::new(),
            show_add_dialog: false,
            show_add_advanced: false,
            show_batch_dialog: false,
            show_grabber_dialog: false,
            show_refresh_dialog: false,
            show_zip_preview_dialog: false,
            show_import_export_dialog: false,
            show_options_dialog: false,
            options_tab: ui_state.last_options_tab,
            show_scheduler_dialog: false,
            show_diagnostics_window: false,
            show_rename_dialog: false,
            show_column_chooser_dialog: false,
            show_shortcuts_dialog: false,
            show_about_dialog: false,
            selected_queue_name: "main".to_owned(),
            new_queue_name_input: String::new(),
            sched_editor_mode: ScheduleMode::Disabled,
            sched_editor_start_time: "22:00".to_owned(),
            sched_editor_stop_time: "06:00".to_owned(),
            sched_editor_once_start: "2026-10-27 01:00".to_owned(),
            sched_editor_once_stop: String::new(),
            sched_editor_days: [true; 7],
            sched_editor_max_concurrent: 4,
            sched_editor_speed_limit_kbps: 0,
            sched_editor_post_action_idx: 1,
            sched_validation_error: None,
            show_details: false,
            show_connection_details: true,
            dialog_tab: DialogTab::General,
            close_on_completion: false,
            open_on_completion: false,
            new_url: String::new(),
            new_filename: String::new(),
            new_category: FileCategory::General,
            new_save_folder: crate::default_download_directory()
                .to_string_lossy()
                .to_string(),
            new_remember_path: true,
            new_probed_size_bytes: None,
            new_probed_resumable: None,
            new_probed_host: None,
            new_probe_in_progress: false,
            new_probe_error: None,
            new_referer: String::new(),
            new_cookies: String::new(),
            new_user_agent: String::new(),
            new_expected_sha256: String::new(),
            new_expected_md5: String::new(),
            new_expected_crc32: String::new(),
            new_mirrors_text: String::new(),
            new_description: String::new(),
            new_queue_selection: "main".to_owned(),
            last_clipboard_url: None,
            rename_task_id: String::new(),
            rename_filename_input: String::new(),
            rename_save_dir_input: String::new(),
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
            status_message: "ﻂﺸﻧ StormGet ﻙﺮﺤﻣ — ﺰﻫﺎﺟ".to_owned(),
            speed_limit_kbps: 0,
            speed_limiter_enabled: false,
            schedule: QueueSchedule::default(),
            previous_download_status: HashMap::new(),
            has_received_downloads: false,
            completed_notice: None,
            pending_intercept_id: None,
            completed_download: None,
<<<<<<< HEAD
            pending_confirmation: None,
            toasts: Vec::new(),
            next_toast_id: 1,
            last_action_instant: Instant::now() - Duration::from_secs(5),
            ui_state,
            ui_state_dirty: false,
            last_ui_state_save: Instant::now(),
            allow_exit: false,
            game_mode_active: false,
            latest_intelligence: None,
            latest_doctor_diagnosis: None,
            playlist_entries: Vec::new(),
            #[cfg(windows)]
            tray: None,
        };

        if let Some(warn_msg) = load_warning {
            app.push_toast(
                ToastKind::Warning,
                "ﺔﻬﺟﺍﻮﻟﺍ ﺔﻟﺎﺣ ﻞﻴﻤﺤﺗ ﻪﻴﺒﻨﺗ",
                &warn_msg,
                None,
            );
        }
        app
    }

    pub fn push_toast(
        &mut self,
        kind: ToastKind,
        title: impl Into<String>,
        message: impl Into<String>,
        task_id: Option<String>,
    ) {
        let id = self.next_toast_id;
        self.next_toast_id += 1;
        if self.toasts.len() >= 4 {
            self.toasts.remove(0);
        }
        self.toasts.push(ToastNotification {
            id,
            kind,
            title: title.into(),
            message: message.into(),
            task_id,
            created_at: Instant::now(),
            duration_secs: 6,
        });
    }

    pub fn mark_ui_state_dirty(&mut self) {
        self.ui_state.theme_mode = self.theme_mode;
        self.ui_state.language = self.language;
        self.ui_state.show_sidebar = self.show_categories_panel;
        self.ui_state.show_bottom_inspector = self.show_bottom_inspector;
        self.ui_state.bottom_inspector_tab = self.bottom_inspector_tab;
        self.ui_state.last_options_tab = self.options_tab;
        self.ui_state.sort_column = self.sort_column;
        self.ui_state.sort_ascending = self.sort_ascending;
        self.ui_state.expand_all_downloads = self.expand_all_downloads;
        self.ui_state.expand_unfinished = self.expand_unfinished;
        self.ui_state.expand_finished = self.expand_finished;
        self.ui_state.expand_queues = self.expand_queues;
        self.ui_state_dirty = true;
    }

    pub fn flush_ui_state_if_needed(&mut self, force: bool) {
        if !self.ui_state_dirty && !force {
            return;
        }
        if !force && self.last_ui_state_save.elapsed() < Duration::from_millis(850) {
            return;
        }
        let base_dir = super::worker::download_directory();
        if let Err(err) = self.ui_state.save_to_disk_atomic(&base_dir) {
            self.error_message = Some(err);
        } else {
            self.ui_state_dirty = false;
            self.last_ui_state_save = Instant::now();
        }
    }

    /// منع تكرار تنفيذ الأمر عند النقر المزدوج غير المقصود (Debounce 220ms)
    pub fn can_trigger_action(&self) -> bool {
        self.last_action_instant.elapsed() >= Duration::from_millis(220)
    }

    pub fn record_action_triggered(&mut self) {
        self.last_action_instant = Instant::now();
    }

    // دوال فتح النوافذ المستقلة مع نقل التركيز إليها إذا كانت مفتوحة مسبقاً (منع التكرار — المرحلة 11.3)
    pub fn open_or_focus_add_dialog(&mut self) {
        self.show_add_dialog = true;
        self.focus_window_requests.insert("add_download".to_owned());
    }

    pub fn open_or_focus_details_dialog(&mut self) {
        if self.has_selected() {
            self.show_details = true;
            self.focus_window_requests
                .insert("download_details".to_owned());
        }
    }

    pub fn open_or_focus_settings(&mut self) {
        self.show_options_dialog = true;
        self.focus_window_requests.insert("settings".to_owned());
    }

    pub fn open_or_focus_scheduler(&mut self) {
        self.show_scheduler_dialog = true;
        self.focus_window_requests.insert("scheduler".to_owned());
    }

    pub fn open_or_focus_diagnostics(&mut self) {
        self.show_diagnostics_window = true;
        self.focus_window_requests.insert("diagnostics".to_owned());
    }

    pub fn reset_add_dialog_fields(&mut self) {
        self.new_url.clear();
        self.new_filename.clear();
        self.new_probed_size_bytes = None;
        self.new_probed_resumable = None;
        self.new_probed_host = None;
        self.new_probe_in_progress = false;
        self.new_probe_error = None;
        self.pending_intercept_id = None;
    }

    pub fn on_new_url_modified(&mut self) {
        self.new_probed_size_bytes = None;
        self.new_probed_resumable = None;
        self.new_probe_error = None;
        let trimmed = self.new_url.trim();
        if let Ok(parsed) = url::Url::parse(trimmed) {
            self.new_probed_host = parsed.host_str().map(String::from);
            if self.new_filename.is_empty() {
                if let Some(segment) = parsed.path_segments().and_then(|mut s| s.next_back()) {
                    let clean = segment.split('?').next().unwrap_or(segment).trim();
                    if !clean.is_empty() {
                        let decoded = crate::core::storage::percent_decode_str(clean);
                        self.new_filename = crate::core::storage::sanitize_filename(&decoded);
                        self.new_category = FileCategory::from_filename(&self.new_filename);
                        if self.new_remember_path {
                            self.new_save_folder = self
                                .app_config
                                .download_dir
                                .join(self.new_category.folder_name())
                                .to_string_lossy()
                                .into_owned();
                        }
                    }
                }
            }
        } else {
            self.new_probed_host = None;
        }
    }

    pub fn trigger_url_probe(&mut self) {
        let u = self.new_url.trim().to_owned();
        if u.is_empty() {
            return;
        }
        self.new_probe_in_progress = true;
        self.new_probe_error = None;
        let req_payload = ProbeUrlRequestPayload {
            url: u,
            cookies: (!self.new_cookies.trim().is_empty()).then(|| self.new_cookies.trim().to_owned()),
            referer: (!self.new_referer.trim().is_empty()).then(|| self.new_referer.trim().to_owned()),
            user_agent: (!self.new_user_agent.trim().is_empty())
                .then(|| self.new_user_agent.trim().to_owned()),
        };
        let _ = self.commands.send(UiCommand::ProbeUrl(req_payload.clone()));
        let _ = self.commands.send(UiCommand::CheckIntelligence(req_payload));
    }

    pub fn load_queue_into_scheduler_editor(&mut self, q: &QueueConfig) {
        self.selected_queue_name.clone_from(&q.name);
        self.sched_editor_mode = q.schedule_mode;
        self.sched_editor_start_time = q
            .start_time
            .clone()
            .unwrap_or_else(|| "22:00".to_owned());
        self.sched_editor_stop_time = q
            .stop_time
            .clone()
            .unwrap_or_else(|| "06:00".to_owned());
        self.sched_editor_once_start = q
            .start_at
            .clone()
            .unwrap_or_else(|| "2026-10-27 01:00".to_owned());
        self.sched_editor_once_stop = q.stop_at.clone().unwrap_or_default();
        if q.days_of_week.is_empty() {
            self.sched_editor_days = [true; 7];
        } else {
            for d in 0..7 {
                self.sched_editor_days[d] = q.days_of_week.contains(&(d as u8));
            }
        }
        self.sched_editor_max_concurrent = q.max_concurrent_downloads.max(1);
        self.sched_editor_speed_limit_kbps = q.speed_limit_kbps;
        self.sched_editor_post_action_idx = match q.post_action {
            PostQueueAction::None => 0,
            PostQueueAction::Notify => 1,
            PostQueueAction::ExitApplication | PostQueueAction::CustomCommand(_) => 2,
        };
        self.sched_validation_error = None;
    }

    pub fn save_scheduler_editor_changes(&mut self) {
        self.sched_validation_error = None;
        let mode = self.sched_editor_mode;
        let mut days_vec = Vec::new();
        for (i, &checked) in self.sched_editor_days.iter().enumerate() {
            if checked {
                days_vec.push(i as u8);
            }
        }

        match mode {
            ScheduleMode::Disabled => {}
            ScheduleMode::Daily => {
                let st = crate::parse_schedule_time(self.sched_editor_start_time.trim());
                let sp = crate::parse_schedule_time(self.sched_editor_stop_time.trim());
                match (st, sp) {
                    (Ok(t1), Ok(t2)) => {
                        if t1 == t2 {
                            self.sched_validation_error = Some(
                                ".ﺎﻘﺑﺎﻄﺘﻳ ﻥﺃ ﻦﻜﻤﻳ ﻻ ﺔﻴﻣﻮﻴﻟﺍ ﺔﻟﻭﺪﺠﻟﺍ ﻲﻓ ﺀﺎﻬﺘﻧﻻﺍ ﺖﻗﻭﻭ ﺀﺪﺒﻟﺍ ﺖﻗﻭ"
                                    .to_owned(),
                            );
                            return;
                        }
                    }
                    _ => {
                        self.sched_validation_error = Some(
                            "22:00). :ﻝﺎﺜﻣ) HH:MM ﺔﻐﻴﺼﻟﺍ ﻡﺪﺨﺘﺳﺍ ؛ﺔﺤﻟﺎﺻ ﺮﻴﻏ ﺖﻗﻮﻟﺍ ﺔﻐﻴﺻ".to_owned(),
                        );
                        return;
                    }
                }
                if days_vec.is_empty() {
                    self.sched_validation_error = Some(
                        ".ﺓﺭﺮﻜﺘﻤﻟﺍ ﺔﻟﻭﺪﺠﻟﺍ ﻞﻴﻐﺸﺘﻟ ﻞﻗﻷﺍ ﻰﻠﻋ ﺪﺣﺍﻭ ﻡﻮﻳ ﺭﺎﻴﺘﺧﺍ ﻰﺟﺮﻳ".to_owned(),
                    );
                    return;
                }
            }
            ScheduleMode::Once => {
                if chrono::NaiveDateTime::parse_from_str(
                    self.sched_editor_once_start.trim(),
                    "%Y-%m-%d %H:%M",
                )
                .is_err()
                {
                    self.sched_validation_error = Some(
                        "YYYY-MM-DD HH:MM. ﻡﺪﺨﺘﺳﺍ ؛ﺔﺤﻟﺎﺻ ﺮﻴﻏ ﺀﺪﺒﻟﺍ ﺖﻗﻭﻭ ﺦﻳﺭﺎﺗ ﺔﻐﻴﺻ".to_owned(),
                    );
                    return;
                }
                if !self.sched_editor_once_stop.trim().is_empty()
                    && chrono::NaiveDateTime::parse_from_str(
                        self.sched_editor_once_stop.trim(),
                        "%Y-%m-%d %H:%M",
                    )
                    .is_err()
                {
                    self.sched_validation_error = Some(
                        "YYYY-MM-DD HH:MM. ﻡﺪﺨﺘﺳﺍ ؛ﺔﺤﻟﺎﺻ ﺮﻴﻏ ﻒﻗﻮﺘﻟﺍ ﺖﻗﻭﻭ ﺦﻳﺭﺎﺗ ﺔﻐﻴﺻ".to_owned(),
                    );
                    return;
                }
            }
        }

        let post_action = match self.sched_editor_post_action_idx {
            0 => PostQueueAction::None,
            1 => PostQueueAction::Notify,
            _ => PostQueueAction::ExitApplication,
        };

        let updated_q = QueueConfig {
            name: self.selected_queue_name.clone(),
            max_concurrent_downloads: self.sched_editor_max_concurrent.clamp(1, 32),
            start_time: (mode == ScheduleMode::Daily)
                .then(|| self.sched_editor_start_time.trim().to_owned()),
            stop_time: (mode == ScheduleMode::Daily)
                .then(|| self.sched_editor_stop_time.trim().to_owned()),
            post_action,
            schedule_mode: mode,
            start_at: (mode == ScheduleMode::Once)
                .then(|| self.sched_editor_once_start.trim().to_owned()),
            stop_at: (mode == ScheduleMode::Once && !self.sched_editor_once_stop.trim().is_empty())
                .then(|| self.sched_editor_once_stop.trim().to_owned()),
            days_of_week: if days_vec.len() == 7 {
                Vec::new()
            } else {
                days_vec.clone()
            },
            speed_limit_kbps: self.sched_editor_speed_limit_kbps,
        };

        let sched = updated_q.schedule();
        let _ = self.commands.send(UiCommand::UpsertQueue(updated_q));
        let _ = self.commands.send(UiCommand::SetSchedule {
            queue_name: self.selected_queue_name.clone(),
            schedule: sched,
        });
        self.push_toast(
            ToastKind::Success,
            "ﺔﻟﻭﺪﺠﻟﺍ ﻆﻔﺣ ﻢﺗ",
            format!(
                ".ﺡﺎﺠﻨﺑ '{}' ﺭﻮﺑﺎﻄﻟﺍ ﺔﻟﻭﺪﺟ ﻖﻴﺒﻄﺗﻭ ﻆﻔﺣ ﻢﺗ",
                self.selected_queue_name
            ),
            None,
        );
    }

    pub fn save_and_apply_all_settings(&mut self) {
        if self.app_config.download_dir.as_os_str().is_empty() {
            self.push_toast(
                ToastKind::Error,
                "ﺕﺍﺩﺍﺪﻋﻹﺍ ﻲﻓ ﺄﻄﺧ",
                ".ﺎﻏﺭﺎﻓ ﻥﻮﻜﻳ ﻥﺃ ﻦﻜﻤﻳ ﻻ ﻲﺿﺍﺮﺘﻓﻻﺍ ﻞﻳﺰﻨﺘﻟﺍ ﺪﻠﺠﻣ",
                None,
            );
            return;
        }
        let _ = self
            .commands
            .send(UiCommand::SaveConfig(self.app_config.clone()));
        let _ = self
            .commands
            .send(UiCommand::SetSpeedLimit(self.app_config.global_speed_limit_kbps));
        self.mark_ui_state_dirty();
        self.flush_ui_state_if_needed(true);
        self.push_toast(
            ToastKind::Success,
            "ﺕﺍﺩﺍﺪﻋﻹﺍ ﻆﻔﺣ",
            ".ﺡﺎﺠﻨﺑ ﺔﻬﺟﺍﻮﻟﺍﻭ ﺕﻼﻳﺰﻨﺘﻟﺍ ﻙﺮﺤﻣ ﻰﻠﻋ ﺎﻬﻘﻴﺒﻄﺗﻭ ﺕﺍﺩﺍﺪﻋﻹﺍ ﻆﻔﺣ ﻢﺗ",
            None,
        );
    }

    pub fn build_redacted_diagnostic_report(&self) -> String {
        let mut out = String::new();
        out.push_str("=== StormGet Diagnostic & Activity Report (Redacted) ===\n");
        out.push_str(&format!("Version: v{}\n", env!("CARGO_PKG_VERSION")));
        out.push_str(&format!(
            "Engine Status: {}\n",
            if self.error_message.is_none() {
                "Online (127.0.0.1:16888)"
            } else {
                "Disconnected / Error"
            }
        ));
        if let Some(err) = &self.error_message {
            out.push_str(&format!("Last Engine Error: {err}\n"));
        }
        if let Some(ts) = &self.tools_status {
            out.push_str(&format!(
                "Tools: yt-dlp={} ({:?}) | FFmpeg={} ({:?})\n",
                ts.ytdlp_installed, ts.ytdlp_path, ts.ffmpeg_installed, ts.ffmpeg_path
            ));
        }
        out.push_str(&format!(
            "Tasks Total: {} | Queues: {}\n",
            self.downloads.len(),
            self.queues.len()
        ));
        out.push_str(&format!(
            "Window Normal Geometry: {:.0}x{:.0} at ({:?}, {:?}), Maximized={}\n\n",
            self.ui_state.main_window.normal_width,
            self.ui_state.main_window.normal_height,
            self.ui_state.main_window.normal_x,
            self.ui_state.main_window.normal_y,
            self.ui_state.main_window.maximized
        ));
        out.push_str("--- Recent Task Connection & Error Logs (Secrets Redacted) ---\n");
        for task in self.downloads.iter().take(20) {
            out.push_str(&format!(
                "* Task [{}] {} | Status: {:?} | Size: {} B | Downloaded: {} B\n",
                &task.task_id[..8.min(task.task_id.len())],
                task.filename,
                task.status,
                task.total_size,
                task.total_downloaded()
            ));
            if let Some(err) = &task.last_error {
                out.push_str(&format!(
                    "  Last Error: {}\n",
                    crate::core::storage::redact_log_message(err)
                ));
            }
            for log in task.connection_logs.iter().rev().take(5) {
                out.push_str(&format!(
                    "  [{}] [{}] {}\n",
                    log.timestamp.format("%H:%M:%S"),
                    log.level,
                    crate::core::storage::redact_log_message(&log.message)
                ));
            }
        }
        out
    }

    pub fn has_any_active_downloads(&self) -> bool {
        self.downloads.iter().any(|t| {
            matches!(
                t.status,
                DownloadStatus::Downloading
                    | DownloadStatus::RefreshingUrl
                    | DownloadStatus::Assembling
                    | DownloadStatus::VerifyingChecksum
            )
        })
    }

    #[allow(non_snake_case)]
    pub fn pending_confirmation_execute(&mut self, confirm: PendingConfirmation) {
        match confirm {
            PendingConfirmation::RemoveRecordsOnly { task_ids, .. } => {
                for tid in task_ids {
                    let _ = self.commands.send(UiCommand::RemoveDownload(tid));
                }
                self.selected_tasks.clear();
                self.selected_task = None;
            }
            PendingConfirmation::DeleteFilesFromDisk { task_ids, .. } => {
                for tid in task_ids {
                    let _ = self.commands.send(UiCommand::DeleteWithFile(tid));
                }
                self.selected_tasks.clear();
                self.selected_task = None;
            }
            PendingConfirmation::CancelActiveDownloads { task_ids, .. } => {
                for tid in task_ids {
                    let _ = self.commands.send(UiCommand::Control {
                        task_id: tid,
                        action: "cancel",
                    });
                }
            }
            PendingConfirmation::ClearCompletedRecords { .. } => {
                let _ = self.commands.send(UiCommand::DeleteCompleted);
            }
            PendingConfirmation::ResetUiLayoutToDefaults => {
                self.ui_state.reset_layout_to_defaults();
                self.show_categories_panel = self.ui_state.show_sidebar;
                self.show_bottom_inspector = self.ui_state.show_bottom_inspector;
                self.sort_column = self.ui_state.sort_column;
                self.sort_ascending = self.ui_state.sort_ascending;
                self.mark_ui_state_dirty();
                self.flush_ui_state_if_needed(true);
                self.push_toast(
                    ToastKind::Success,
                    "ﺔﻬﺟﺍﻮﻟﺍ ﻂﻴﻄﺨﺗ ﻂﺒﺿ ﺓﺩﺎﻋﺇ",
                    ".ﺡﺎﺠﻨﺑ ﺓﺮﻫﺎﻈﻟﺍ ﺓﺪﻤﻋﻷﺍﻭ ﻲﺿﺍﺮﺘﻓﻻﺍ ﻂﻴﻄﺨﺘﻟﺍ ﺓﺩﺎﻌﺘﺳﺍ ﺖﻤﺗ",
                    None,
                );
            }
            PendingConfirmation::ResetAppConfigToDefaults => {
                let default_cfg = AppConfig::default();
                self.app_config = default_cfg.clone();
                self.speed_limit_kbps = 0;
                self.speed_limiter_enabled = false;
                let _ = self.commands.send(UiCommand::SaveConfig(default_cfg));
                let _ = self.commands.send(UiCommand::SetSpeedLimit(0));
                self.push_toast(
                    ToastKind::Success,
                    "ﻙﺮﺤﻤﻟﺍ ﺕﺍﺩﺍﺪﻋﺇ ﻂﺒﺿ ﺓﺩﺎﻋﺇ",
                    ".ﺡﺎﺠﻨﺑ ﺎﻬﻘﻴﺒﻄﺗﻭ ﻙﺮﺤﻤﻠﻟ ﺔﻴﺿﺍﺮﺘﻓﻻﺍ ﺕﺍﺩﺍﺪﻋﻹﺍ ﺓﺩﺎﻌﺘﺳﺍ ﺖﻤﺗ",
                    None,
                );
            }
            PendingConfirmation::CloseWhileDownloadsActive { .. } => {}
=======
            allow_exit: false,
            #[cfg(windows)]
            tray: None,
>>>>>>> efc40c730b5c3e9a4001ccff0b896d7f2f3ef344
        }
    }
}

impl eframe::App for RustIdmApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        apply_stormget_theme_with_scale(ctx, self.theme_mode, self.ui_state.font_scale);
        self.receive_events(ctx);
        self.handle_tray_events(ctx);
<<<<<<< HEAD
        self.handle_keyboard_shortcuts(ctx);
        self.track_main_window_geometry(ctx);
=======
>>>>>>> efc40c730b5c3e9a4001ccff0b896d7f2f3ef344

        self.draw_menu_bar(ctx);
        if self.ui_state.show_toolbar {
            self.draw_toolbar(ctx);
        }
        if self.ui_state.show_status_bar {
            self.draw_status_bar(ctx);
        }
        if self.show_bottom_inspector {
            self.draw_bottom_inspector(ctx);
        }
        if self.show_categories_panel {
            self.draw_categories(ctx);
        }

        self.draw_download_table(ctx);
        draw_all_dialogs(self, ctx);
        self.draw_download_review_viewport(ctx);
        self.draw_download_status_viewport(ctx);
        self.draw_completion_viewport(ctx);
        self.handle_main_window_close(ctx);
<<<<<<< HEAD
        self.flush_ui_state_if_needed(false);
=======
>>>>>>> efc40c730b5c3e9a4001ccff0b896d7f2f3ef344

        let has_active = self.has_any_active_downloads();
        ctx.request_repaint_after(if has_active {
            Duration::from_millis(120)
        } else {
            Duration::from_millis(450)
        });
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.flush_ui_state_if_needed(true);
    }
}

impl RustIdmApp {
<<<<<<< HEAD
    fn track_main_window_geometry(&mut self, ctx: &egui::Context) {
        let (inner_rect, outer_rect, is_max, is_min) = ctx.input(|i| {
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
            let before = self.ui_state.main_window.clone();
            self.ui_state.main_window.update_from_live_viewport(
                pos.map(|p| p.0),
                pos.map(|p| p.1),
                rect.width(),
                rect.height(),
                is_max,
                is_min,
            );
            if self.ui_state.main_window != before {
                self.ui_state_dirty = true;
            }
        }
    }

    fn handle_keyboard_shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.wants_keyboard_input() {
            return;
        }
        let (ctrl_n, ctrl_a, del_key, shift_del, enter_key, esc_key) = ctx.input(|i| {
            (
                i.modifiers.ctrl && i.key_pressed(egui::Key::N),
                i.modifiers.ctrl && i.key_pressed(egui::Key::A),
                !i.modifiers.shift && i.key_pressed(egui::Key::Delete),
                i.modifiers.shift && i.key_pressed(egui::Key::Delete),
                i.key_pressed(egui::Key::Enter),
                i.key_pressed(egui::Key::Escape),
            )
        });

        if ctrl_n {
            self.open_or_focus_add_dialog();
        } else if ctrl_a {
            self.select_all_visible();
        } else if shift_del && self.has_selected() {
            self.request_delete_selected(true);
        } else if del_key && self.has_selected() {
            self.request_delete_selected(false);
        } else if enter_key && self.has_selected() {
            self.open_or_focus_details_dialog();
        } else if esc_key {
            self.pending_confirmation = None;
        }
    }

=======
>>>>>>> efc40c730b5c3e9a4001ccff0b896d7f2f3ef344
    fn handle_tray_events(&mut self, ctx: &egui::Context) {
        #[cfg(windows)]
        if let Some(tray) = &self.tray {
            for action in tray.poll_actions() {
                match action {
                    super::tray::TrayAction::Show => {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
<<<<<<< HEAD
                        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                    }
                    super::tray::TrayAction::Exit => {
                        self.flush_ui_state_if_needed(true);
=======
                        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                    }
                    super::tray::TrayAction::Exit => {
>>>>>>> efc40c730b5c3e9a4001ccff0b896d7f2f3ef344
                        self.allow_exit = true;
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                }
            }
        }

        #[cfg(not(windows))]
        let _ = ctx;
    }

<<<<<<< HEAD
    /// التعامل الذكي مع إغلاق النافذة الرئيسية (المرحلة 3.1):
    /// - إذا كانت هناك تنزيلات نشطة، لا يوقفها بصمت بل يعرض حوار خيارات (إبقاء البرنامج في الخلفية أو الإيقاف والخروج).
    /// - إذا لم تكن هناك تنزيلات نشطة وكان خيار التصغير للخلفية مفعلاً في Windows، يخفي النافذة إلى Tray بدون مربع تأكيد غير ضروري.
    fn handle_main_window_close(&mut self, ctx: &egui::Context) {
        if ctx.input(|input| input.viewport().close_requested()) && !self.allow_exit {
            self.flush_ui_state_if_needed(true);
            let active_count = self
                .downloads
                .iter()
                .filter(|t| {
                    matches!(
                        t.status,
                        DownloadStatus::Downloading
                            | DownloadStatus::RefreshingUrl
                            | DownloadStatus::Assembling
                            | DownloadStatus::VerifyingChecksum
                    )
                })
                .count();

            if active_count > 0 {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.pending_confirmation =
                    Some(PendingConfirmation::CloseWhileDownloadsActive { active_count });
                return;
            }

            #[cfg(windows)]
            if self.app_config.minimize_to_tray_on_close && self.tray.is_some() {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            }
        }
=======
    fn handle_main_window_close(&mut self, ctx: &egui::Context) {
        #[cfg(windows)]
        if ctx.input(|input| input.viewport().close_requested()) && !self.allow_exit {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }

        #[cfg(not(windows))]
        let _ = ctx;
>>>>>>> efc40c730b5c3e9a4001ccff0b896d7f2f3ef344
    }

    fn draw_download_review_viewport(&mut self, ctx: &egui::Context) {
        if !self.show_add_dialog {
            return;
        }

<<<<<<< HEAD
        let loc = Loc::new(self.language);
        let saved = self
            .ui_state
            .secondary_windows
            .entry("add_download".to_owned())
            .or_insert_with(|| SavedWindowGeometry::new_default(620.0, 440.0))
            .clone();
        let monitors = detect_connected_monitors();
        let placement = validate_and_clamp_window_geometry(&saved, 540.0, 360.0, &monitors);
        let should_focus = self.focus_window_requests.remove("add_download");

        let viewport_id = egui::ViewportId::from_hash_of("download_review_viewport");
        let builder = egui::ViewportBuilder::default()
            .with_title(loc.add_download_window_title())
            .with_inner_size([placement.width, placement.height])
            .with_min_inner_size([540.0, 360.0])
            .with_position(egui::pos2(placement.x, placement.y))
            .with_resizable(true);

        ctx.show_viewport_immediate(viewport_id, builder, |viewport_ctx, _| {
            if should_focus {
                viewport_ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            }
=======
        let viewport_id = egui::ViewportId::from_hash_of("download_review_viewport");
        let builder = egui::ViewportBuilder::default()
            .with_title(Loc::new(self.language).download_details_title())
            .with_inner_size([580.0, 360.0])
            .with_min_inner_size([520.0, 320.0])
            .with_resizable(false)
            .with_always_on_top();

        ctx.show_viewport_immediate(viewport_id, builder, |viewport_ctx, _| {
>>>>>>> efc40c730b5c3e9a4001ccff0b896d7f2f3ef344
            if viewport_ctx.input(|input| input.viewport().close_requested()) {
                if let Some(id) = self.pending_intercept_id.take() {
                    let _ = self.commands.send(UiCommand::ResolvePendingIntercept {
                        id,
                        action: "cancel",
                        save_dir: None,
                    });
                }
                self.show_add_dialog = false;
                return;
            }

<<<<<<< HEAD
            apply_stormget_theme_with_scale(viewport_ctx, self.theme_mode, self.ui_state.font_scale);
=======
            apply_idm_theme(viewport_ctx, self.theme_mode);
            let loc = Loc::new(self.language);
>>>>>>> efc40c730b5c3e9a4001ccff0b896d7f2f3ef344
            let palette = ThemePalette::get(self.theme_mode);
            draw_add_url_dialog(self, viewport_ctx, &loc, &palette);
        });
    }

    fn draw_completion_viewport(&mut self, ctx: &egui::Context) {
        let Some(message) = self.completed_notice.clone() else {
            return;
        };

        let viewport_id = egui::ViewportId::from_hash_of("download_completion_viewport");
        let loc = Loc::new(self.language);
        let builder = egui::ViewportBuilder::default()
            .with_title(loc.download_complete())
<<<<<<< HEAD
            .with_inner_size([440.0, 185.0])
            .with_resizable(false);
=======
            .with_inner_size([420.0, 180.0])
            .with_resizable(false)
            .with_always_on_top();
>>>>>>> efc40c730b5c3e9a4001ccff0b896d7f2f3ef344

        ctx.show_viewport_immediate(viewport_id, builder, |viewport_ctx, _| {
            if viewport_ctx.input(|input| input.viewport().close_requested()) {
                self.completed_notice = None;
                self.completed_download = None;
                return;
            }

<<<<<<< HEAD
            apply_stormget_theme_with_scale(viewport_ctx, self.theme_mode, self.ui_state.font_scale);
=======
            apply_idm_theme(viewport_ctx, self.theme_mode);
>>>>>>> efc40c730b5c3e9a4001ccff0b896d7f2f3ef344
            egui::CentralPanel::default().show(viewport_ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.heading(loc.download_complete());
                    ui.label(&message);
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        if ui.button(loc.ctx_open()).clicked() {
                            if let Some(task) = &self.completed_download {
                                open_file(&task.final_file_path());
                            }
                            self.completed_notice = None;
                        }
                        if ui.button(loc.open_folder()).clicked() {
                            if let Some(task) = &self.completed_download {
                                open_folder(&task.final_file_path());
                            }
                        }
<<<<<<< HEAD
                        if ui.button(loc.tr("ﺭﺎﻌﺷﻹﺍ ﻕﻼﻏﺇ", "Dismiss")).clicked() {
                            self.completed_notice = None;
=======
                        if ui.button(loc.minimize_to_tray()).clicked() {
                            self.completed_notice = None;
                            viewport_ctx.send_viewport_cmd_to(
                                egui::ViewportId::ROOT,
                                egui::ViewportCommand::Visible(false),
                            );
                        }
                        if ui.button(loc.exit()).clicked() {
                            self.allow_exit = true;
                            viewport_ctx.send_viewport_cmd_to(
                                egui::ViewportId::ROOT,
                                egui::ViewportCommand::Close,
                            );
>>>>>>> efc40c730b5c3e9a4001ccff0b896d7f2f3ef344
                        }
                    });
                });
            });
        });
    }

    fn draw_download_status_viewport(&mut self, ctx: &egui::Context) {
        if !self.show_details {
            return;
        }
        let Some(task) = self.selected_download() else {
            self.show_details = false;
            return;
        };
        let progress = if task.total_size == 0 {
            0.0
        } else {
            (task.total_downloaded() as f32 / task.total_size as f32).clamp(0.0, 1.0)
        };
<<<<<<< HEAD
        let title = if task.total_size > 0 {
            format!("StormGet — {}% {}", (progress * 100.0).floor() as u32, task.filename)
        } else {
            format!("StormGet — {}", task.filename)
        };

        let saved = self
            .ui_state
            .secondary_windows
            .entry("download_details".to_owned())
            .or_insert_with(|| SavedWindowGeometry::new_default(660.0, 560.0))
            .clone();
        let monitors = detect_connected_monitors();
        let placement = validate_and_clamp_window_geometry(&saved, 560.0, 420.0, &monitors);
        let should_focus = self.focus_window_requests.remove("download_details");

        let viewport_id = egui::ViewportId::from_hash_of("download_status_viewport");
        let builder = egui::ViewportBuilder::default()
            .with_title(title)
            .with_inner_size([placement.width, placement.height])
            .with_min_inner_size([560.0, 420.0])
            .with_position(egui::pos2(placement.x, placement.y))
            .with_resizable(true);

        ctx.show_viewport_immediate(viewport_id, builder, |viewport_ctx, _| {
            if should_focus {
                viewport_ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            }
            if viewport_ctx.input(|input| input.viewport().close_requested()) {
                // إغلاق نافذة تفاصيل التنزيل لا يلغي التنزيل نفسه أبداً
=======
        let title = format!("{}% {}", (progress * 100.0).floor() as u32, task.filename);
        let viewport_id = egui::ViewportId::from_hash_of("download_status_viewport");
        let builder = egui::ViewportBuilder::default()
            .with_title(title)
            .with_inner_size([580.0, 560.0])
            .with_min_inner_size([520.0, 400.0])
            .with_always_on_top();

        ctx.show_viewport_immediate(viewport_id, builder, |viewport_ctx, _| {
            if viewport_ctx.input(|input| input.viewport().close_requested()) {
>>>>>>> efc40c730b5c3e9a4001ccff0b896d7f2f3ef344
                self.show_details = false;
                return;
            }

<<<<<<< HEAD
            apply_stormget_theme_with_scale(viewport_ctx, self.theme_mode, self.ui_state.font_scale);
=======
            apply_idm_theme(viewport_ctx, self.theme_mode);
>>>>>>> efc40c730b5c3e9a4001ccff0b896d7f2f3ef344
            let palette = ThemePalette::get(self.theme_mode);
            draw_details_dialog(self, viewport_ctx, &palette);
        });
    }

<<<<<<< HEAD
    // ========================================================================
    // إدارة التحديد الفردي والمتعدد والأوامر الجماعية (Section 4 & 5.4)
    // ========================================================================

    pub fn effective_selected_ids(&self) -> Vec<String> {
        if !self.selected_tasks.is_empty() {
            let mut ids: Vec<String> = self
                .downloads
                .iter()
                .filter(|d| self.selected_tasks.contains(&d.task_id))
                .map(|d| d.task_id.clone())
                .collect();
            if ids.is_empty() {
                if let Some(sel) = &self.selected_task {
                    ids.push(sel.clone());
                }
            }
            ids
        } else if let Some(sel) = &self.selected_task {
            vec![sel.clone()]
        } else {
            Vec::new()
        }
    }

=======
>>>>>>> efc40c730b5c3e9a4001ccff0b896d7f2f3ef344
    pub fn selected_download(&self) -> Option<&DownloadStateMeta> {
        if let Some(primary) = &self.selected_task {
            if let Some(found) = self.downloads.iter().find(|t| &t.task_id == primary) {
                return Some(found);
            }
        }
        self.downloads
            .iter()
            .find(|t| self.selected_tasks.contains(&t.task_id))
    }

    pub fn selected_downloads_list(&self) -> Vec<&DownloadStateMeta> {
        let ids = self.effective_selected_ids();
        self.downloads
            .iter()
            .filter(|t| ids.contains(&t.task_id))
            .collect()
    }

    pub fn has_selected(&self) -> bool {
        self.selected_download().is_some()
    }

    pub fn select_single_task(&mut self, task_id: String) {
        self.selected_tasks.clear();
        self.selected_tasks.insert(task_id.clone());
        self.selected_task = Some(task_id.clone());
        self.selection_anchor = Some(task_id);
    }

    pub fn select_all_visible(&mut self) {
        let visible_ids: Vec<String> = self
            .filtered_downloads()
            .into_iter()
            .map(|t| t.task_id.clone())
            .collect();
        self.selected_tasks.clear();
        for id in &visible_ids {
            self.selected_tasks.insert(id.clone());
        }
        if let Some(first) = visible_ids.first() {
            self.selected_task = Some(first.clone());
        }
    }

    pub fn invert_visible_selection(&mut self) {
        let visible_ids: Vec<String> = self
            .filtered_downloads()
            .into_iter()
            .map(|t| t.task_id.clone())
            .collect();
        let mut new_set = HashSet::new();
        for id in visible_ids {
            if !self.selected_tasks.contains(&id) {
                new_set.insert(id);
            }
        }
        self.selected_task = new_set.iter().next().cloned();
        self.selected_tasks = new_set;
    }

    /// شروط التفاعل الصارمة للأزرار السياقية والجماعية (المرحلة 4 و 5.4):
    /// - زر البدء/الاستئناف يتفعل فقط إذا كان هناك عنصر محدد واحد على الأقل قابل للاستئناف (متوقف مؤقتاً أو في الانتظار، وليس ملغى أو مكتملاً).
    pub fn can_resume_selected(&self) -> bool {
        self.selected_downloads_list().iter().any(|t| {
            matches!(t.status, DownloadStatus::Paused | DownloadStatus::Queued)
                && !t.is_cancelled_by_user()
        })
    }

    /// زر الإيقاف المؤقت لا يعمل على تنزيل مكتمل أو فاشل؛ يتفعل فقط للتنزيلات النشطة أو المنتظرة
    pub fn can_pause_selected(&self) -> bool {
        self.selected_downloads_list().iter().any(|t| {
            matches!(
                t.status,
                DownloadStatus::Downloading
                    | DownloadStatus::Queued
                    | DownloadStatus::RefreshingUrl
            )
        })
    }

    /// زر الإلغاء يتفعل للتنزيلات النشطة أو المتوقفة أو المنتظرة (وليس للمكتملة أو الملغاة مسبقاً)
    pub fn can_cancel_selected(&self) -> bool {
        self.selected_downloads_list().iter().any(|t| {
            !matches!(t.status, DownloadStatus::Completed | DownloadStatus::Failed)
        })
    }

    /// زر إعادة المحاولة يتفعل للتنزيلات الفاشلة أو المنتهية الصلاحية أو الملغاة
    pub fn can_retry_selected(&self) -> bool {
        self.selected_downloads_list().iter().any(|t| {
            matches!(
                t.status,
                DownloadStatus::Failed | DownloadStatus::NeedsUrlRefresh
            )
        })
    }

    pub fn resume_selected_valid_tasks(&mut self) {
        if !self.can_trigger_action() {
            return;
        }
        self.record_action_triggered();
        let ids: Vec<String> = self
            .selected_downloads_list()
            .into_iter()
            .filter(|t| {
                matches!(t.status, DownloadStatus::Paused | DownloadStatus::Queued)
                    && !t.is_cancelled_by_user()
            })
            .map(|t| t.task_id.clone())
            .collect();
        for tid in &ids {
            let _ = self.commands.send(UiCommand::Control {
                task_id: tid.clone(),
                action: "resume",
            });
        }
        if !ids.is_empty() {
            self.status_message = format!("ﺔﻤﻬﻣ {} ﻝ ﻑﺎﻨﺌﺘﺳﻻﺍ ﺮﻣﺃ ﻝﺎﺳﺭﺇ ﻢﺗ", ids.len());
        }
    }

    pub fn pause_selected_valid_tasks(&mut self) {
        if !self.can_trigger_action() {
            return;
        }
        self.record_action_triggered();
        let ids: Vec<String> = self
            .selected_downloads_list()
            .into_iter()
            .filter(|t| {
                matches!(
                    t.status,
                    DownloadStatus::Downloading
                        | DownloadStatus::Queued
                        | DownloadStatus::RefreshingUrl
                )
            })
            .map(|t| t.task_id.clone())
            .collect();
        for tid in &ids {
            let _ = self.commands.send(UiCommand::Control {
                task_id: tid.clone(),
                action: "pause",
            });
        }
        if !ids.is_empty() {
            self.status_message = format!("ﺎﺘﻗﺆﻣ ﺔﻤﻬﻣ {} ﻑﺎﻘﻳﺇ ﻢﺗ", ids.len());
        }
    }

    pub fn retry_selected_failed_tasks(&mut self) {
        if !self.can_trigger_action() {
            return;
        }
        self.record_action_triggered();
        let tasks_to_retry: Vec<(String, bool)> = self
            .selected_downloads_list()
            .into_iter()
            .filter(|t| {
                matches!(
                    t.status,
                    DownloadStatus::Failed | DownloadStatus::NeedsUrlRefresh
                )
            })
            .map(|t| (t.task_id.clone(), t.is_cancelled_by_user()))
            .collect();

        for (tid, was_cancelled) in &tasks_to_retry {
            if *was_cancelled {
                let _ = self.commands.send(UiCommand::Redownload(tid.clone()));
            } else {
                let _ = self.commands.send(UiCommand::Control {
                    task_id: tid.clone(),
                    action: "resume",
                });
            }
        }
        if !tasks_to_retry.is_empty() {
            self.status_message =
                format!("ﺔﻤﻬﻣ {} ﻝ ﺔﻟﻭﺎﺤﻤﻟﺍ ﺓﺩﺎﻋﺇ ﺖﻤﺗ", tasks_to_retry.len());
        }
    }

    pub fn request_cancel_tasks(&mut self, task_ids: Vec<String>) {
        let valid: Vec<(String, String)> = self
            .downloads
            .iter()
            .filter(|t| {
                task_ids.contains(&t.task_id)
                    && !matches!(t.status, DownloadStatus::Completed | DownloadStatus::Failed)
            })
            .map(|t| (t.task_id.clone(), t.filename.clone()))
            .collect();
        if valid.is_empty() {
            return;
        }
        let ids: Vec<String> = valid.iter().map(|(i, _)| i.clone()).collect();
        let names: Vec<String> = valid.iter().map(|(_, n)| n.clone()).collect();
        if self.app_config.confirm_destructive_actions {
            self.pending_confirmation = Some(PendingConfirmation::CancelActiveDownloads {
                task_ids: ids,
                filenames: names,
            });
        } else {
            for tid in ids {
                let _ = self.commands.send(UiCommand::Control {
                    task_id: tid,
                    action: "cancel",
                });
            }
        }
    }

    pub fn request_delete_selected(&mut self, delete_from_disk: bool) {
        let selected = self.selected_downloads_list();
        if selected.is_empty() {
            return;
        }
        let task_ids: Vec<String> = selected.iter().map(|t| t.task_id.clone()).collect();
        let filenames: Vec<String> = selected.iter().map(|t| t.filename.clone()).collect();

        if delete_from_disk {
            // حذف الملف من القرص يتطلب دائماً تأكيداً صريحاً (المرحلة 6.3 و 13)
            self.pending_confirmation = Some(PendingConfirmation::DeleteFilesFromDisk {
                task_ids,
                filenames,
            });
        } else if self.app_config.confirm_destructive_actions {
            self.pending_confirmation = Some(PendingConfirmation::RemoveRecordsOnly {
                task_ids,
                filenames,
            });
        } else {
            for tid in task_ids {
                let _ = self.commands.send(UiCommand::RemoveDownload(tid));
            }
            self.selected_tasks.clear();
            self.selected_task = None;
        }
    }

    #[allow(dead_code)]
    pub fn control_selected(&mut self, action: &'static str) {
        match action {
            "resume" => self.resume_selected_valid_tasks(),
            "pause" => self.pause_selected_valid_tasks(),
            "cancel" => {
                let ids = self.effective_selected_ids();
                self.request_cancel_tasks(ids);
            }
            _ => {
                if let Some(task) = self.selected_download() {
                    let _ = self.commands.send(UiCommand::Control {
                        task_id: task.task_id.clone(),
                        action,
                    });
                }
            }
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
        self.status_message = "ﺎﺘﻗﺆﻣ ﺔﻄﺸﻨﻟﺍ ﺕﻼﻳﺰﻨﺘﻟﺍ ﻊﻴﻤﺟ ﻑﺎﻘﻳﺇ ﻢﺗ".to_owned();
    }

    #[allow(dead_code)]
    pub fn start_queue_downloads(&mut self) {
        self.start_specific_queue("main");
    }

    pub fn start_specific_queue(&mut self, queue_name: &str) {
        for task in &self.downloads {
            if task.queue_name == queue_name
                && (task.status == DownloadStatus::Paused || task.status == DownloadStatus::Queued)
                && !task.is_cancelled_by_user()
            {
                let _ = self.commands.send(UiCommand::Control {
                    task_id: task.task_id.clone(),
                    action: "resume",
                });
            }
        }
        self.status_message = format!("'{queue_name}' ﺭﻮﺑﺎﻄﻟﺍ ﻡﺎﻬﻣ ﺀﺪﺑ ﻢﺗ");
    }

    pub fn stop_specific_queue(&mut self, queue_name: &str) {
        for task in &self.downloads {
            if task.queue_name == queue_name
                && (task.status == DownloadStatus::Downloading
                    || task.status == DownloadStatus::Queued)
            {
                let _ = self.commands.send(UiCommand::Control {
                    task_id: task.task_id.clone(),
                    action: "pause",
                });
            }
        }
        self.status_message = format!("'{queue_name}' ﺭﻮﺑﺎﻄﻟﺍ ﻡﺎﻬﻣ ﻑﺎﻘﻳﺇ ﻢﺗ");
    }

    pub fn delete_completed_downloads(&mut self) {
        let completed_count = self
            .downloads
            .iter()
            .filter(|t| t.status == DownloadStatus::Completed)
            .count();
        if completed_count == 0 {
            self.push_toast(
                ToastKind::Info,
                "ﺔﻠﻤﺘﻜﻣ ﺕﻼﺠﺳ ﺪﺟﻮﺗ ﻻ",
                ".ﺎﻬﺤﺴﻤﻟ ﺔﻠﻤﺘﻜﻣ ﻞﻳﺰﻨﺗ ﺕﻼﺠﺳ ﻰﻠﻋ ﺎﻴﻟﺎﺣ ﻱﻮﺘﺤﺗ ﻻ ﺔﻤﺋﺎﻘﻟﺍ",
                None,
            );
            return;
        }
        if self.app_config.confirm_destructive_actions {
            self.pending_confirmation = Some(PendingConfirmation::ClearCompletedRecords {
                count: completed_count,
            });
        } else {
            let _ = self.commands.send(UiCommand::DeleteCompleted);
        }
    }

    #[allow(dead_code)]
    pub fn remove_selected_download(&mut self) {
        self.request_delete_selected(false);
    }

    pub fn open_rename_dialog_for_task(&mut self, task: &DownloadStateMeta) {
        self.rename_task_id.clone_from(&task.task_id);
        self.rename_filename_input.clone_from(&task.filename);
        self.rename_save_dir_input = task.save_dir.display().to_string();
        self.show_rename_dialog = true;
    }

    // ========================================================================
    // استقبال ومعالجة أحداث المحرك الخلفي (Engine Events Receiver)
    // ========================================================================

    fn receive_events(&mut self, ctx: &egui::Context) {
        while let Ok(event) = self.events.try_recv() {
            match event {
                UiEvent::Downloads(mut downloads) => {
                    downloads.sort_by(|a, b| b.created_at.cmp(&a.created_at));
                    if self.has_received_downloads {
                        for task in &downloads {
                            if !self.previous_download_status.contains_key(&task.task_id) {
                                self.select_single_task(task.task_id.clone());
                            }

                            let prev_st = self.previous_download_status.get(&task.task_id).copied();
                            if task.status == DownloadStatus::Completed
                                && prev_st != Some(DownloadStatus::Completed)
                            {
<<<<<<< HEAD
                                if self.app_config.notify_on_complete {
                                    self.push_toast(
                                        ToastKind::Success,
                                        Loc::new(self.language).download_complete(),
                                        format!("{} :ﻒﻠﻤﻟﺍ ﻞﻳﺰﻨﺗ ﻞﻤﺘﻛﺍ", task.filename),
                                        Some(task.task_id.clone()),
                                    );
                                }
=======
                                self.show_details = false;
                                self.completed_notice = Some(format!(
                                    "{}: {}",
                                    Loc::new(self.language).download_complete(),
                                    task.filename
                                ));
                                self.completed_download = Some(task.clone());
>>>>>>> efc40c730b5c3e9a4001ccff0b896d7f2f3ef344
                                if self.completion_exit_idm {
                                    self.flush_ui_state_if_needed(true);
                                    std::process::exit(0);
                                }
                                if self.completion_shutdown_pc {
                                    execute_shutdown(
                                        self.completion_shutdown_action,
                                        self.completion_force_kill,
                                    );
                                }
                            } else if matches!(
                                task.status,
                                DownloadStatus::Failed | DownloadStatus::NeedsUrlRefresh
                            ) && prev_st != Some(task.status)
                                && !task.is_cancelled_by_user()
                                && self.app_config.notify_on_failure
                            {
                                let reason = if task.is_expired_link() {
                                    ".ﻂﺑﺍﺮﻟﺍ ﺚﻳﺪﺤﺗ ﻰﻟﺇ ﺝﺎﺘﺤﻳ — (401/403/410) ﻂﺑﺍﺮﻟﺍ ﺔﻴﺣﻼﺻ ﺖﻬﺘﻧﺍ"
                                } else {
                                    task.last_error
                                        .as_deref()
                                        .unwrap_or("ﻡﺩﺎﺨﻟﺎﺑ ﻝﺎﺼﺗﻻﺍ ﻝﺎﻤﻜﺘﺳﺍ ﺭﺬﻌﺗ")
                                };
                                self.push_toast(
                                    ToastKind::Error,
                                    format!("{} :ﻞﻳﺰﻨﺘﻟﺍ ﻞﺸﻓ", task.filename),
                                    reason,
                                    None,
                                );
                            }
                        }
                    } else if let Some(active) = downloads
                        .iter()
                        .find(|t| t.status == DownloadStatus::Downloading)
                    {
                        if self.selected_task.is_none() {
                            self.select_single_task(active.task_id.clone());
                        }
                    }
                    self.previous_download_status = downloads
                        .iter()
                        .map(|task| (task.task_id.clone(), task.status))
                        .collect();
                    self.has_received_downloads = true;
                    self.downloads = downloads;
                    self.sync_windows_taskbar_progress();

                    let valid_ids: HashSet<String> =
                        self.downloads.iter().map(|t| t.task_id.clone()).collect();
                    self.selected_tasks.retain(|id| valid_ids.contains(id));
                    if self
                        .selected_task
                        .as_ref()
                        .is_some_and(|id| !valid_ids.contains(id))
                    {
                        self.selected_task = self.selected_tasks.iter().next().cloned();
                    }
                }
                UiEvent::Queues(queues) => {
                    if self.queues.is_empty() {
                        if let Some(main_q) = queues.iter().find(|q| q.name == "main").cloned() {
                            self.load_queue_into_scheduler_editor(&main_q);
                        }
                    }
                    self.queues = queues;
                }
                UiEvent::PendingIntercepts(pending) => {
                    if !self.show_add_dialog {
                        if let Some(item) = pending.first() {
                            self.pending_intercept_id = Some(item.id.clone());
                            self.new_url.clone_from(&item.payload.url);
                            self.new_filename = item.payload.filename.clone().unwrap_or_default();
                            self.new_category = FileCategory::from_filename(&self.new_filename);
                            self.new_save_folder = self
                                .app_config
                                .download_dir
                                .join(self.new_category.folder_name())
                                .to_string_lossy()
                                .into_owned();
                            self.new_referer = item.payload.referer.clone().unwrap_or_default();
                            self.new_cookies = item.payload.cookies.clone().unwrap_or_default();
                            self.new_user_agent =
                                item.payload.user_agent.clone().unwrap_or_default();
                            self.new_description =
                                item.payload.description.clone().unwrap_or_default();
                            self.new_queue_selection = item
                                .payload
                                .queue_name
                                .clone()
                                .unwrap_or_else(|| "main".to_owned());
                            self.on_new_url_modified();
                            self.open_or_focus_add_dialog();
                        }
                    }
                }
<<<<<<< HEAD
=======
                UiEvent::Queues(queues) => self.queues = queues,
                UiEvent::PendingIntercepts(pending) => {
                    if !self.show_add_dialog {
                        if let Some(item) = pending.first() {
                            self.pending_intercept_id = Some(item.id.clone());
                            self.new_url.clone_from(&item.payload.url);
                            self.new_filename =
                                item.payload.filename.clone().unwrap_or_default();
                            self.new_category = FileCategory::from_filename(&self.new_filename);
                            self.new_save_folder = self
                                .app_config
                                .download_dir
                                .join(self.new_category.folder_name())
                                .to_string_lossy()
                                .into_owned();
                            self.new_referer =
                                item.payload.referer.clone().unwrap_or_default();
                            self.new_cookies =
                                item.payload.cookies.clone().unwrap_or_default();
                            self.new_user_agent =
                                item.payload.user_agent.clone().unwrap_or_default();
                            self.new_description =
                                item.payload.description.clone().unwrap_or_default();
                            self.new_queue_selection = item
                                .payload
                                .queue_name
                                .clone()
                                .unwrap_or_else(|| "main".to_owned());
                            self.show_add_dialog = true;
                        }
                    }
                }
>>>>>>> efc40c730b5c3e9a4001ccff0b896d7f2f3ef344
                UiEvent::Config(cfg) => {
                    self.speed_limit_kbps = cfg.global_speed_limit_kbps;
                    self.speed_limiter_enabled = cfg.global_speed_limit_kbps > 0;
                    if self.new_save_folder == crate::default_download_directory().to_string_lossy()
                    {
                        self.new_save_folder = cfg.download_dir.to_string_lossy().into_owned();
                    }
                    self.app_config = cfg;
                }
                UiEvent::Stats(stats) => self.engine_stats = Some(stats),
                UiEvent::ToolsStatus(report) => self.tools_status = Some(report),
                UiEvent::ProbeResult(Ok(probe)) => {
                    self.new_probe_in_progress = false;
                    self.new_probe_error = None;
                    self.new_probed_size_bytes = Some(probe.total_size);
                    self.new_probed_resumable = Some(probe.resumable);
                    self.new_probed_host = probe.host;
                    if !probe.filename.is_empty() {
                        self.new_filename = probe.filename;
                        self.new_category = probe.category;
                        if self.new_remember_path {
                            self.new_save_folder = self
                                .app_config
                                .download_dir
                                .join(self.new_category.folder_name())
                                .to_string_lossy()
                                .into_owned();
                        }
                    }
                }
                UiEvent::ProbeResult(Err(err)) => {
                    self.new_probe_in_progress = false;
                    self.new_probe_error = Some(err);
                }
                UiEvent::BatchResult(Ok(resp)) => {
                    self.status_message =
                        format!("ﺔﻴﻌﻓﺩ ﺔﻤﻬﻣ {} ﺔﻟﻭﺪﺟ ﺖﻤﺗ", resp.queued_task_ids.len());
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
                    self.status_message =
                        format!("ﻖﺑﺎﻄﻣ ﻒﻠﻣ {} ﻰﻠﻋ ﺭﻮﺜﻌﻟﺍ ﻢﺗ", report.matched_files.len());
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
                UiEvent::Schedule { queue_name, result } => match result {
                    Ok(schedule) => {
                        if queue_name == "main" {
                            self.schedule = schedule;
                        }
                        self.error_message = None;
                    }
                    Err(error) => self.error_message = Some(error),
                },
                UiEvent::CompletedNotice(msg) => {
                    if self.app_config.notify_on_schedule_start {
                        self.push_toast(ToastKind::Info, "ﺮﻴﺑﺍﻮﻄﻟﺍﻭ ﻝﻭﺪﺠﻤﻟﺍ ﺭﺎﻌﺷﺇ", &msg, None);
                    }
                    self.status_message = msg;
                }
<<<<<<< HEAD
=======
                UiEvent::Schedule(Err(error)) => self.error_message = Some(error),
                UiEvent::CompletedNotice(filename) => {
                    self.status_message = filename;
                }
>>>>>>> efc40c730b5c3e9a4001ccff0b896d7f2f3ef344
                UiEvent::Status(Ok(msg)) => {
                    self.error_message = None;
                    self.status_message = msg;
                }
                UiEvent::Status(Err(err)) => {
                    self.status_message = "ﻙﺮﺤﻤﻟﺍ ﻦﻣ ﻪﻴﺒﻨﺗ".to_owned();
                    self.push_toast(ToastKind::Error, "ﺔﻴﻠﻤﻌﻟﺍ ﻲﻓ ﺄﻄﺧ", &err, None);
                    self.error_message = Some(err);
                }
                UiEvent::EngineConnection(Ok(())) => self.error_message = None,
                UiEvent::EngineConnection(Err(err)) => self.error_message = Some(err),
                UiEvent::SmartThrottleUpdated(st) => {
                    self.game_mode_active = st.game_mode_active;
                }
                UiEvent::DoctorDiagnosisResult(Ok(diag)) => {
                    self.push_toast(
                        ToastKind::Info,
                        format!("🩺 تشخيص العطل ({:?})", diag.category),
                        &diag.explanation_arabic,
                        None,
                    );
                    self.latest_doctor_diagnosis = Some(diag);
                }
                UiEvent::DoctorDiagnosisResult(Err(err)) => {
                    self.error_message = Some(err);
                }
                UiEvent::IntelligenceResult(Ok(rep)) => {
                    self.latest_intelligence = Some(rep);
                }
                UiEvent::IntelligenceResult(Err(_)) => {}
                UiEvent::PlaylistResult(Ok(info)) => {
                    self.playlist_entries = info.entries;
                }
                UiEvent::PlaylistResult(Err(err)) => {
                    self.error_message = Some(err);
                }
            }
            ctx.request_repaint();
        }
    }

    // ========================================================================
    // المرحلة 3.2: شريط القوائم الرئيسي (File, Downloads, View, Tools, Help)
    // ========================================================================

    fn draw_menu_bar(&mut self, ctx: &egui::Context) {
        let loc = Loc::new(self.language);
        let palette = ThemePalette::get(self.theme_mode);

        egui::TopBottomPanel::top("stormget_menu_bar")
            .exact_height(28.0)
            .frame(
                egui::Frame::new()
                    .fill(palette.header_bg)
                    .stroke(Stroke::new(1.0, palette.border))
                    .inner_margin(egui::Margin::symmetric(8, 3)),
            )
            .show(ctx, |ui| {
                egui::menu::bar(ui, |ui| {
                    // 1. القائمة File — ملف
                    ui.menu_button(loc.menu_file(), |ui| {
                        if ui
                            .button(loc.tr("(Ctrl+N) ...ﺪﻳﺪﺟ ﻞﻳﺰﻨﺗ ﺔﻓﺎﺿﺇ ➕", "➕ Add New Download... (Ctrl+N)"))
                            .clicked()
                        {
                            self.open_or_focus_add_dialog();
                            ui.close_menu();
                        }
                        if ui
                            .button(loc.tr("(Batch)... ﺓﺩﺪﻌﺘﻣ ﺕﻼﻳﺰﻨﺗ ﺔﻓﺎﺿﺇ 📦", "📦 Add Batch Downloads..."))
                            .clicked()
                        {
                            self.show_batch_dialog = true;
                            ui.close_menu();
                        }
                        ui.separator();
                        if ui
                            .button(loc.tr("(.ef2 / .txt)... ﺕﻼﻳﺰﻨﺗ ﺔﻤﺋﺎﻗ ﺩﺍﺮﻴﺘﺳﺍ 📥", "📥 Import Download List..."))
                            .clicked()
                        {
                            self.show_import_export_dialog = true;
                            ui.close_menu();
                        }
                        if ui
                            .button(loc.tr("(.ef2)... ﺕﻼﻳﺰﻨﺘﻟﺍ ﺔﻤﺋﺎﻗ ﺮﻳﺪﺼﺗ 📤", "📤 Export Download List..."))
                            .clicked()
                        {
                            let _ = self.commands.send(UiCommand::ExportList);
                            ui.close_menu();
                        }
                        ui.separator();
                        if ui
                            .button(loc.tr("ﻲﺿﺍﺮﺘﻓﻻﺍ ﺕﻼﻳﺰﻨﺘﻟﺍ ﺪﻠﺠﻣ ﺢﺘﻓ 📁", "📁 Open Default Downloads Folder"))
                            .clicked()
                        {
                            open_folder(&self.app_config.download_dir);
                            ui.close_menu();
                        }
                        ui.separator();
                        if ui
                            .button(loc.tr("ﺔﻴﻔﻠﺨﻠﻟ ﺓﺬﻓﺎﻨﻟﺍ ﺀﺎﻔﺧﺇ / ﻕﻼﻏﺇ 🗕", "🗕 Close Window to Tray"))
                            .clicked()
                        {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                            ui.close_menu();
                        }
                        if ui
                            .button(loc.tr("(Alt+F4) ﺞﻣﺎﻧﺮﺒﻟﺍ ﻦﻣ ﻲﺋﺎﻬﻨﻟﺍ ﺝﻭﺮﺨﻟﺍ 🚪", "🚪 Exit StormGet (Alt+F4)"))
                            .clicked()
                        {
                            ui.close_menu();
                            let active_count = self
                                .downloads
                                .iter()
                                .filter(|t| {
                                    matches!(
                                        t.status,
                                        DownloadStatus::Downloading
                                            | DownloadStatus::RefreshingUrl
                                            | DownloadStatus::Assembling
                                            | DownloadStatus::VerifyingChecksum
                                    )
                                })
                                .count();
                            if active_count > 0 {
                                self.pending_confirmation =
                                    Some(PendingConfirmation::CloseWhileDownloadsActive {
                                        active_count,
                                    });
                            } else {
                                self.flush_ui_state_if_needed(true);
                                self.allow_exit = true;
                                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                            }
                        }
                    });

                    // 2. القائمة Downloads — التنزيلات
                    ui.menu_button(loc.menu_downloads(), |ui| {
                        let has_sel = self.has_selected();
                        let can_res = self.can_resume_selected();
                        let can_pau = self.can_pause_selected();
                        let can_can = self.can_cancel_selected();
                        let can_ret = self.can_retry_selected();
                        let sel_completed_exists = self
                            .selected_download()
                            .is_some_and(|t| t.status == DownloadStatus::Completed && t.final_file_path().exists());

                        if ui.button(loc.tb_add_url()).clicked() {
                            self.open_or_focus_add_dialog();
                            ui.close_menu();
                        }
                        ui.separator();
                        if ui
                            .add_enabled(can_res, egui::Button::new(loc.ctx_resume()))
                            .clicked()
                        {
                            self.resume_selected_valid_tasks();
                            ui.close_menu();
                        }
                        if ui
                            .add_enabled(can_pau, egui::Button::new(loc.ctx_pause()))
                            .clicked()
                        {
                            self.pause_selected_valid_tasks();
                            ui.close_menu();
                        }
                        if ui
                            .add_enabled(can_can, egui::Button::new(loc.ctx_cancel()))
                            .clicked()
                        {
                            let ids = self.effective_selected_ids();
                            self.request_cancel_tasks(ids);
                            ui.close_menu();
                        }
                        if ui
                            .add_enabled(can_ret, egui::Button::new(loc.ctx_retry()))
                            .clicked()
                        {
                            self.retry_selected_failed_tasks();
                            ui.close_menu();
                        }
                        if ui
                            .add_enabled(has_sel, egui::Button::new(loc.ctx_redownload()))
                            .clicked()
                        {
                            if let Some(tid) = self.selected_download().map(|t| t.task_id.clone()) {
                                let _ = self.commands.send(UiCommand::Redownload(tid));
                            }
                            ui.close_menu();
                        }
                        ui.separator();
                        if ui
                            .add_enabled(sel_completed_exists, egui::Button::new(loc.ctx_open()))
                            .clicked()
                        {
                            if let Some(p) = self.selected_download().map(|t| t.final_file_path()) {
                                open_file(&p);
                            }
                            ui.close_menu();
                        }
                        if ui
                            .add_enabled(has_sel, egui::Button::new(loc.ctx_open_folder()))
                            .clicked()
                        {
                            if let Some(p) = self.selected_download().map(|t| t.final_file_path()) {
                                reveal_in_explorer(&p);
                            }
                            ui.close_menu();
                        }
                        if ui
                            .add_enabled(has_sel, egui::Button::new(loc.ctx_copy_url()))
                            .clicked()
                        {
                            if let Some(u) = self.selected_download().map(|t| t.url.clone()) {
                                ui.ctx().copy_text(u);
                            }
                            ui.close_menu();
                        }
                        ui.separator();
                        if ui
                            .add_enabled(has_sel, egui::Button::new(loc.ctx_remove()))
                            .clicked()
                        {
                            self.request_delete_selected(false);
                            ui.close_menu();
                        }
                        if ui
                            .add_enabled(has_sel, egui::Button::new(loc.ctx_delete_with_file()))
                            .clicked()
                        {
                            self.request_delete_selected(true);
                            ui.close_menu();
                        }
                    });

                    // 3. القائمة View — عرض (مع حفظ دائم للتفضيلات)
                    ui.menu_button(loc.menu_view(), |ui| {
                        if ui
                            .checkbox(
                                &mut self.show_categories_panel,
                                loc.tr("(Categories) ﺔﻴﺒﻧﺎﺠﻟﺍ ﺔﺣﻮﻠﻟﺍ ﺭﺎﻬﻇﺇ", "Show Sidebar"),
                            )
                            .changed()
                        {
                            self.ui_state.show_sidebar = self.show_categories_panel;
                            self.mark_ui_state_dirty();
                        }
                        if ui
                            .checkbox(
                                &mut self.ui_state.show_toolbar,
                                loc.tr("(Toolbar) ﺕﺍﻭﺩﻷﺍ ﻂﻳﺮﺷ ﺭﺎﻬﻇﺇ", "Show Toolbar"),
                            )
                            .changed()
                        {
                            self.mark_ui_state_dirty();
                        }
                        if ui
                            .checkbox(
                                &mut self.ui_state.show_status_bar,
                                loc.tr("(Status Bar) ﻲﻠﻔﺴﻟﺍ ﺔﻟﺎﺤﻟﺍ ﻂﻳﺮﺷ ﺭﺎﻬﻇﺇ", "Show Status Bar"),
                            )
                            .changed()
                        {
                            self.mark_ui_state_dirty();
                        }
                        if ui
                            .checkbox(
                                &mut self.show_bottom_inspector,
                                loc.tr("(Inspector) ﻲﻠﻔﺴﻟﺍ ﺺﺤﻔﻟﺍ ﺔﺣﻮﻟ ﺭﺎﻬﻇﺇ", "Show Bottom Inspector"),
                            )
                            .changed()
                        {
                            self.ui_state.show_bottom_inspector = self.show_bottom_inspector;
                            self.mark_ui_state_dirty();
                        }
                        ui.separator();
                        ui.menu_button(
                            loc.tr("(Row Density) ﻑﻮﻔﺼﻟﺍ ﺽﺮﻋ ﺔﻓﺎﺜﻛ", "Row Density"),
                            |ui| {
                                for d in [
                                    RowDensity::Compact,
                                    RowDensity::Comfortable,
                                    RowDensity::Spacious,
                                ] {
                                    if ui
                                        .selectable_value(
                                            &mut self.ui_state.row_density,
                                            d,
                                            loc.density_name(d),
                                        )
                                        .changed()
                                    {
                                        self.mark_ui_state_dirty();
                                        ui.close_menu();
                                    }
                                }
                            },
                        );
                        if ui
                            .button(loc.tr("...ﺎﻬﺒﻴﺗﺮﺗﻭ ﺓﺮﻫﺎﻈﻟﺍ ﺓﺪﻤﻋﻷﺍ ﻲﻓ ﻢﻜﺤﺘﻟﺍ 📊", "📊 Configure Visible Columns..."))
                            .clicked()
                        {
                            self.show_column_chooser_dialog = true;
                            ui.close_menu();
                        }
                        ui.separator();
                        if ui
                            .button(if self.theme_mode == ThemeMode::Dark {
                                "(Light Mode) ﺢﺗﺎﻔﻟﺍ ﺮﻬﻈﻤﻟﺍ ﻰﻟﺇ ﻞﻳﺪﺒﺘﻟﺍ ☀️"
                            } else {
                                "(Dark Mode) ﻦﻛﺍﺪﻟﺍ ﺮﻬﻈﻤﻟﺍ ﻰﻟﺇ ﻞﻳﺪﺒﺘﻟﺍ 🌙"
                            })
                            .clicked()
                        {
                            self.theme_mode = match self.theme_mode {
                                ThemeMode::Dark => ThemeMode::Light,
                                ThemeMode::Light => ThemeMode::Dark,
                            };
                            self.mark_ui_state_dirty();
                            ui.close_menu();
                        }
                        if ui
                            .button(if self.language == AppLanguage::Arabic {
                                "🌐 Switch to English"
                            } else {
                                "ﺔﻴﺑﺮﻌﻟﺍ ﺔﻐﻠﻟﺍ ﻰﻟﺇ ﻞﻳﺪﺒﺘﻟﺍ 🌐"
                            })
                            .clicked()
                        {
                            self.language = match self.language {
                                AppLanguage::Arabic => AppLanguage::English,
                                AppLanguage::English => AppLanguage::Arabic,
                            };
                            self.mark_ui_state_dirty();
                            ui.close_menu();
                        }
                        ui.separator();
                        if ui
                            .button(loc.tr("...ﻲﺿﺍﺮﺘﻓﻻﺍ ﺔﻬﺟﺍﻮﻟﺍ ﻂﻴﻄﺨﺗ ﺓﺩﺎﻌﺘﺳﺍ ↺", "↺ Restore Default Layout..."))
                            .clicked()
                        {
                            self.pending_confirmation =
                                Some(PendingConfirmation::ResetUiLayoutToDefaults);
                            ui.close_menu();
                        }
                    });

                    // 4. القائمة Tools — أدوات
                    ui.menu_button(loc.menu_tools(), |ui| {
                        if ui
                            .button(loc.tr("...ﺮﻴﺑﺍﻮﻄﻟﺍ ﺮﻳﺪﻣﻭ ﺕﻼﻳﺰﻨﺘﻟﺍ ﺔﻟﻭﺪﺟ ⏰", "⏰ Scheduler & Queue Manager..."))
                            .clicked()
                        {
                            self.open_or_focus_scheduler();
                            ui.close_menu();
                        }
                        if ui
                            .button(loc.tr("...ﺔﻋﺮﺴﻟﺍﻭ ﻝﺎﺼﺗﻻﺍ ﻦﻳﻮﻜﺗﻭ ﺕﺍﺩﺍﺪﻋﻹﺍ ⚙", "⚙ Settings & Connection Config..."))
                            .clicked()
                        {
                            self.open_or_focus_settings();
                            ui.close_menu();
                        }
                        if ui
                            .button(loc.tr("...ﺢﻔﺼﺘﻤﻟﺍ ﻊﻣ ﻞﻣﺎﻜﺘﻟﺍ ﺕﺍﺩﺍﺪﻋﺇ 🌐", "🌐 Browser Integration Settings..."))
                            .clicked()
                        {
                            self.options_tab = OptionsTab::BrowserIntegration;
                            self.open_or_focus_settings();
                            ui.close_menu();
                        }
                        if ui
                            .button(loc.tr("(yt-dlp & FFmpeg)... ﻂﺋﺎﺳﻮﻟﺍ ﺕﺍﻭﺩﺃ 🎬", "🎬 Media Tools (yt-dlp & FFmpeg)..."))
                            .clicked()
                        {
                            self.options_tab = OptionsTab::Media;
                            self.open_or_focus_settings();
                            ui.close_menu();
                        }
                        ui.separator();
                        if ui
                            .button(loc.tr("(Site Grabber)... ﻊﻗﺍﻮﻤﻟﺍ ﻑﺎﺸﻜﺘﺳﺍﻭ ﻒﺣﺰﻟﺍ ﺓﺍﺩﺃ 🕷", "🕷 Site Grabber..."))
                            .clicked()
                        {
                            self.show_grabber_dialog = true;
                            ui.close_menu();
                        }
                        if ui
                            .button(loc.tr("...ﺪﻌﺑ ﻦﻋ ZIP ﺕﺎﻔﻴﺷﺭﺃ ﺺﺣﺎﻓ 🗜", "🗜 Remote ZIP Inspector..."))
                            .clicked()
                        {
                            if let Some(url) = self.selected_download().map(|sel| sel.url.clone()) {
                                self.zip_preview_url = url;
                            }
                            self.show_zip_preview_dialog = true;
                            ui.close_menu();
                        }
                        ui.separator();
                        if ui
                            .button(loc.tr("...ﺺﻴﺨﺸﺘﻟﺍ ﺕﺎﻣﻮﻠﻌﻣﻭ ﻁﺎﺸﻨﻟﺍﻭ ﺀﺎﻄﺧﻷﺍ ﻞﺠﺳ 🩺", "🩺 Error Log & Diagnostics..."))
                            .clicked()
                        {
                            self.open_or_focus_diagnostics();
                            ui.close_menu();
                        }
                    });

                    // 5. القائمة Help — مساعدة
                    ui.menu_button(loc.menu_help(), |ui| {
                        if ui
                            .button(loc.tr("...ﺢﻴﺗﺎﻔﻤﻟﺍ ﺔﺣﻮﻟ ﺕﺍﺭﺎﺼﺘﺧﺍ ﺽﺮﻋ ⌨", "⌨ Keyboard Shortcuts..."))
                            .clicked()
                        {
                            self.show_shortcuts_dialog = true;
                            ui.close_menu();
                        }
                        if ui
                            .button(loc.tr("...ﺕﺍﻭﺩﻷﺍ ﺔﻟﺎﺣﻭ ﺺﻴﺨﺸﺘﻟﺍ ﺕﺎﻣﻮﻠﻌﻣ ﺽﺮﻋ 🩺", "🩺 View Diagnostic Info..."))
                            .clicked()
                        {
                            self.open_or_focus_diagnostics();
                            ui.close_menu();
                        }
                        if ui
                            .button(loc.tr("ﻲﻠﺤﻤﻟﺍ ﻖﻴﺛﻮﺘﻟﺍ ﺪﻠﺠﻣ ﺢﺘﻓ 📖", "📖 Open Local Documentation Folder"))
                            .clicked()
                        {
                            open_folder(&PathBuf::from("docs"));
                            ui.close_menu();
                        }
                        ui.separator();
                        if ui
                            .button(loc.tr(
                                &format!("(v{} ﺭﺍﺪﺻﻹﺍ) StormGet ﻝﻮﺣ ℹ", env!("CARGO_PKG_VERSION")),
                                "ℹ About StormGet",
                            ))
                            .clicked()
                        {
                            self.show_about_dialog = true;
                            ui.close_menu();
                        }
                    });

                    // شريط البحث السريع وأزرار تبديل المظهر واللغة في يمين شريط القوائم
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let lang_badge = if self.language == AppLanguage::Arabic {
                            "EN"
                        } else {
                            "AR"
                        };
                        if ui
                            .small_button(lang_badge)
                            .on_hover_text("(ﺔﻳﺰﻴﻠﺠﻧﻹﺍ / ﺔﻴﺑﺮﻌﻟﺍ) ﺔﻬﺟﺍﻮﻟﺍ ﺔﻐﻟ ﻞﻳﺪﺒﺗ")
                            .clicked()
                        {
                            self.language = match self.language {
                                AppLanguage::Arabic => AppLanguage::English,
                                AppLanguage::English => AppLanguage::Arabic,
                            };
                            self.mark_ui_state_dirty();
                        }

                        let theme_icon = if self.theme_mode == ThemeMode::Dark {
                            "☀️"
                        } else {
                            "🌙"
                        };
                        if ui
                            .small_button(theme_icon)
                            .on_hover_text("ﻦﻛﺍﺪﻟﺍ / ﺢﺗﺎﻔﻟﺍ ﺮﻬﻈﻤﻟﺍ ﻞﻳﺪﺒﺗ")
                            .clicked()
                        {
                            self.theme_mode = match self.theme_mode {
                                ThemeMode::Dark => ThemeMode::Light,
                                ThemeMode::Light => ThemeMode::Dark,
                            };
                            self.mark_ui_state_dirty();
                        }

                        if !self.search_query.is_empty()
                            && ui
                                .small_button("×")
                                .on_hover_text("ﺚﺤﺒﻟﺍ ﺺﻧ ﺢﺴﻣ")
                                .clicked()
                        {
                            self.search_query.clear();
                        }
                        ui.add(
                            egui::TextEdit::singleline(&mut self.search_query)
                                .hint_text(loc.tr("...ﺔﺌﻔﻟﺍ ﻭﺃ ﻂﺑﺍﺮﻟﺍ ﻭﺃ ﻢﺳﻻﺎﺑ ﺚﺤﺑ 🔍", "🔍 Search name, URL, category..."))
                                .desired_width(210.0),
                        );
                    });
                });
            });
    }

    // ========================================================================
    // المرحلة الرابعة: شريط الأدوات الرئيسي (Main Toolbar)
    // ========================================================================

    fn draw_toolbar(&mut self, ctx: &egui::Context) {
        let loc = Loc::new(self.language);
        let palette = ThemePalette::get(self.theme_mode);

        egui::TopBottomPanel::top("stormget_toolbar")
            .exact_height(64.0)
            .frame(
                egui::Frame::new()
                    .fill(palette.toolbar_bg)
                    .stroke(Stroke::new(1.0, palette.border))
                    .inner_margin(egui::Margin::symmetric(8, 3)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing = Vec2::new(3.0, 0.0);

                    let can_resume = self.can_resume_selected();
                    let can_pause = self.can_pause_selected();
                    let can_cancel = self.can_cancel_selected();
                    let can_retry = self.can_retry_selected();
                    let has_sel = self.has_selected();

                    // 1. تنزيل جديد
                    if stormget_toolbar_button_with_tooltip(
                        ui,
                        &palette,
                        ToolbarIcon::AddUrl,
                        &loc.tb_add_url(),
                        "(Ctrl+N) ﺪﻳﺪﺟ ﻞﻳﺰﻨﺗ ﻂﺑﺍﺭ ﺔﻓﺎﺿﺇ ﺓﺬﻓﺎﻧ ﺢﺘﻓ",
                        true,
                        false,
                    ) {
                        self.open_or_focus_add_dialog();
                    }

                    // 2. إضافة دفعة / من الحافظة
                    if stormget_toolbar_button_with_tooltip(
                        ui,
                        &palette,
                        ToolbarIcon::PasteClipboard,
                        &loc.tb_paste_url(),
                        "(Ctrl+V) ﻊﻳﺮﺴﻟﺍ ﻖﺼﻠﻟﺍ ﺰﻴﻬﺠﺗ ﻊﻣ ﻂﺑﺍﺭ ﺔﻓﺎﺿﺇ ﺓﺬﻓﺎﻧ ﺢﺘﻓ",
                        true,
                        false,
                    ) {
                        self.open_or_focus_add_dialog();
                    }

                    // 3. استيراد قائمة تنزيلات
                    if stormget_toolbar_button_with_tooltip(
                        ui,
                        &palette,
                        ToolbarIcon::ImportList,
                        &loc.tb_import(),
                        "(.ef2 / .txt) ﻂﺑﺍﻭﺭ ﺔﻤﺋﺎﻗ ﺮﻳﺪﺼﺗ ﻭﺃ ﺩﺍﺮﻴﺘﺳﺍ",
                        true,
                        false,
                    ) {
                        self.show_import_export_dialog = true;
                    }

                    ui.separator();

                    // 4. بدء / استئناف
                    if stormget_toolbar_button_with_tooltip(
                        ui,
                        &palette,
                        ToolbarIcon::StartResume,
                        &loc.tb_resume(),
                        "ﻑﺎﻨﺌﺘﺳﻼﻟ ﺔﻠﺑﺎﻘﻟﺍ ﺓﺩﺪﺤﻤﻟﺍ ﺕﻼﻳﺰﻨﺘﻟﺍ ﻑﺎﻨﺌﺘﺳﺍ ﻭﺃ ﺀﺪﺑ",
                        can_resume,
                        false,
                    ) {
                        self.resume_selected_valid_tasks();
                    }

                    // 5. إيقاف مؤقت
                    if stormget_toolbar_button_with_tooltip(
                        ui,
                        &palette,
                        ToolbarIcon::Pause,
                        &loc.tb_stop(),
                        "ﻊﻃﺎﻘﻤﻟﺍ ﻡﺪﻘﺗ ﻆﻔﺣ ﻊﻣ ﺎﺘﻗﺆﻣ ﺓﺩﺪﺤﻤﻟﺍ ﺔﻄﺸﻨﻟﺍ ﺕﻼﻳﺰﻨﺘﻟﺍ ﻑﺎﻘﻳﺇ",
                        can_pause,
                        false,
                    ) {
                        self.pause_selected_valid_tasks();
                    }

                    // 6. إلغاء التنزيل
                    if stormget_toolbar_button_with_tooltip(
                        ui,
                        &palette,
                        ToolbarIcon::Cancel,
                        &loc.tb_cancel(),
                        "ﻥﺎﻣﺄﺑ ﻪﺗﻻﺎﺼﺗﺍ ﻑﺎﻘﻳﺇﻭ ﺩﺪﺤﻤﻟﺍ ﻞﻳﺰﻨﺘﻟﺍ ﺀﺎﻐﻟﺇ",
                        can_cancel,
                        false,
                    ) {
                        let ids = self.effective_selected_ids();
                        self.request_cancel_tasks(ids);
                    }

                    // 7. إعادة المحاولة
                    if stormget_toolbar_button_with_tooltip(
                        ui,
                        &palette,
                        ToolbarIcon::RetryFailed,
                        &loc.tb_retry(),
                        "ﺄﻄﺧ ﺐﺒﺴﺑ ﺔﻔﻗﻮﺘﻤﻟﺍ ﻭﺃ ﺔﻠﺷﺎﻔﻟﺍ ﺕﻼﻳﺰﻨﺘﻟﺍ ﺔﻟﻭﺎﺤﻣ ﺓﺩﺎﻋﺇ",
                        can_retry,
                        false,
                    ) {
                        self.retry_selected_failed_tasks();
                    }

                    // 8. حذف السجل
                    if stormget_toolbar_button_with_tooltip(
                        ui,
                        &palette,
                        ToolbarIcon::Delete,
                        &loc.tb_delete(),
                        "(Shift+Delete) ﻡﺪﺨﺘﺳﺍ ﻲﻠﻌﻔﻟﺍ ﻒﻠﻤﻟﺍ ﻑﺬﺤﻟ — (Delete) ﺓﺩﺪﺤﻤﻟﺍ ﺔﻤﻬﻤﻟﺍ ﻞﺠﺳ ﻑﺬﺣ",
                        has_sel,
                        false,
                    ) {
                        self.request_delete_selected(false);
                    }

                    // 9. مسح المكتمل
                    if stormget_toolbar_button_with_tooltip(
                        ui,
                        &palette,
                        ToolbarIcon::DeleteCompleted,
                        &loc.tb_del_completed(),
                        "ﺹﺮﻘﻟﺍ ﻰﻠﻋ ﺎﻬﺗﺎﻔﻠﻣ ﺀﺎﻘﺑ ﻊﻣ ﺔﻤﺋﺎﻘﻟﺍ ﻦﻣ ﺔﻠﻤﺘﻜﻤﻟﺍ ﺕﻼﻳﺰﻨﺘﻟﺍ ﺕﻼﺠﺳ ﺢﺴﻣ",
                        true,
                        false,
                    ) {
                        self.delete_completed_downloads();
                    }

                    ui.separator();

                    // 10. المجدول والطوابير
                    if stormget_toolbar_button_with_tooltip(
                        ui,
                        &palette,
                        ToolbarIcon::Scheduler,
                        &loc.tb_scheduler(),
                        "ﺔﻠﻘﺘﺴﻤﻟﺍ ﺮﻴﺑﺍﻮﻄﻟﺍﻭ ﺔﻴﻨﻣﺰﻟﺍ ﺔﻟﻭﺪﺠﻟﺍ ﺓﺭﺍﺩﺇ ﺓﺬﻓﺎﻧ ﺢﺘﻓ",
                        true,
                        false,
                    ) {
                        self.open_or_focus_scheduler();
                    }

                    // 11. الإعدادات
                    if stormget_toolbar_button_with_tooltip(
                        ui,
                        &palette,
                        ToolbarIcon::Options,
                        &loc.tb_options(),
                        "ﺔﻠﻘﺘﺴﻤﻟﺍ ﻙﺮﺤﻤﻟﺍﻭ ﺞﻣﺎﻧﺮﺒﻟﺍ ﺕﺍﺩﺍﺪﻋﺇ ﺓﺬﻓﺎﻧ ﺢﺘﻓ",
                        true,
                        false,
                    ) {
                        self.open_or_focus_settings();
                    }

                    ui.separator();

                    // 12. زر وضع الألعاب الذكي (Game Mode & Smart Throttle)
                    let gm_label = if self.game_mode_active {
                        loc.tr("🎮 (ﻂﺸﻧ) ﺏﺎﻌﻟﻷﺍ ﻊﺿﻭ", "🎮 Game Mode (ON)")
                    } else {
                        loc.tr("🎮 ﺏﺎﻌﻟﻷﺍ ﻊﺿﻭ", "🎮 Game Mode")
                    };
                    if ui
                        .selectable_label(self.game_mode_active, RichText::new(gm_label).strong())
                        .on_hover_text("تفعيل/إيقاف وضع الألعاب والخنق الذكي لمنع ارتفاع الـ Ping أثناء اللعب")
                        .clicked()
                    {
                        let next_state = !self.game_mode_active;
                        self.game_mode_active = next_state;
                        let _ = self.commands.send(UiCommand::ToggleGameMode(
                            crate::GameModeTogglePayload {
                                active: next_state,
                                duration_minutes: Some(60),
                            },
                        ));
                    }
                });
            });
    }

    pub fn sync_windows_taskbar_progress(&self) {
        let mut active_done = 0u64;
        let mut active_total = 0u64;
        let mut has_active = false;
        let mut has_indeterminate = false;
        let mut has_failed = false;
        let mut paused_done = 0u64;
        let mut paused_total = 0u64;
        let mut has_paused = false;

        for t in &self.downloads {
            match t.status {
                DownloadStatus::Downloading
                | DownloadStatus::RefreshingUrl
                | DownloadStatus::Assembling
                | DownloadStatus::VerifyingChecksum => {
                    has_active = true;
                    if t.total_size > 0 {
                        active_done = active_done.saturating_add(t.total_downloaded());
                        active_total = active_total.saturating_add(t.total_size);
                    } else {
                        has_indeterminate = true;
                    }
                }
                DownloadStatus::Paused if t.total_size > 0 => {
                    has_paused = true;
                    paused_done = paused_done.saturating_add(t.total_downloaded());
                    paused_total = paused_total.saturating_add(t.total_size);
                }
                DownloadStatus::Failed | DownloadStatus::NeedsUrlRefresh
                    if !t.is_cancelled_by_user() =>
                {
                    has_failed = true;
                }
                _ => {}
            }
        }

        let state = if has_active {
            if active_total > 0 {
                crate::core::winfx::TaskbarProgressState::Normal(active_done, active_total)
            } else if has_indeterminate {
                crate::core::winfx::TaskbarProgressState::Busy
            } else {
                crate::core::winfx::TaskbarProgressState::Off
            }
        } else if has_failed {
            crate::core::winfx::TaskbarProgressState::Error(1, 1)
        } else if has_paused && paused_total > 0 {
            crate::core::winfx::TaskbarProgressState::Paused(paused_done, paused_total)
        } else {
            crate::core::winfx::TaskbarProgressState::Off
        };

        crate::core::winfx::update_process_taskbar_progress(state);
    }

    // ========================================================================
    // المرحلة 5.5: شريط الحالة السفلي (Bottom Status Bar)
    // ========================================================================

    fn draw_status_bar(&mut self, ctx: &egui::Context) {
        let loc = Loc::new(self.language);
        let palette = ThemePalette::get(self.theme_mode);

        egui::TopBottomPanel::bottom("stormget_status_bar")
            .exact_height(26.0)
            .frame(
                egui::Frame::new()
                    .fill(palette.header_bg)
                    .stroke(Stroke::new(1.0, palette.border))
                    .inner_margin(egui::Margin::symmetric(10, 3)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let mut active_count = 0usize;
                    let mut queued_count = 0usize;
                    let mut total_speed: u64 = 0;

                    for t in &self.downloads {
                        match t.status {
                            DownloadStatus::Downloading
                            | DownloadStatus::RefreshingUrl
                            | DownloadStatus::Assembling
                            | DownloadStatus::VerifyingChecksum => {
                                active_count += 1;
                                let s: u64 = t.smoothed_speed_bps();
                                total_speed = total_speed.saturating_add(s);
                            }
                            DownloadStatus::Queued => queued_count += 1,
                            _ => {}
                        }
                    }

                    let engine_dot = if self.error_message.is_none() {
                        ("ﻞﺼﺘﻣ ﻙﺮﺤﻤﻟﺍ ●", palette.color_success)
                    } else {
                        ("ﻙﺮﺤﻤﻟﺎﺑ ﻪﻴﺒﻨﺗ ●", palette.color_error)
                    };
                    ui.colored_label(engine_dot.1, RichText::new(engine_dot.0).size(11.5).strong());
                    ui.separator();

                    let summary_text = if self.language == AppLanguage::Arabic {
                        loc.ar(&format!(
                            "{} :ﺔﻄﺸﻨﻟﺍ  |  {} :ﺭﺎﻈﺘﻧﻻﺍ ﻲﻓ  |  {} :ﻲﻟﺎﻤﺟﻹﺍ  |  {} :ﺔﻴﻠﻜﻟﺍ ﺔﻋﺮﺴﻟﺍ",
                            active_count,
                            queued_count,
                            self.downloads.len(),
                            loc.format_speed(total_speed)
                        ))
                    } else {
                        format!(
                            "Active: {}  |  Queued: {}  |  Total: {}  |  Speed: {}",
                            active_count,
                            queued_count,
                            self.downloads.len(),
                            loc.format_speed(total_speed)
                        )
                    };
                    ui.label(RichText::new(summary_text).size(11.5).color(palette.text_main));

                    if let Some(ref ts) = self.tools_status {
                        ui.separator();
                        let y = if ts.ytdlp_installed { "✓" } else { "✗" };
                        let f = if ts.ffmpeg_installed { "✓" } else { "✗" };
                        ui.label(
                            RichText::new(format!("yt-dlp {y} | FFmpeg {f}"))
                                .size(11.0)
                                .color(palette.text_muted),
                        );
                    }

                    if let Some(err) = self.error_message.clone() {
                        ui.separator();
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new(format!("⚠ {err}"))
                                        .size(11.0)
                                        .color(palette.color_error),
                                )
                                .frame(false),
                            )
                            .on_hover_text("ﺀﺎﻄﺧﻷﺍ ﻞﺠﺳﻭ ﺺﻴﺨﺸﺘﻟﺍ ﺓﺬﻓﺎﻧ ﺢﺘﻔﻟ ﺮﻘﻧﺍ")
                            .clicked()
                        {
                            self.open_or_focus_diagnostics();
                        }
                    }

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new(&self.status_message)
                                .size(11.5)
                                .color(palette.text_muted),
                        );
                    });
                });
            });
    }

    // ========================================================================
    // المرحلة الرابعة عشرة: لوحة التنقل والفئات الجانبية مع أعداد المهام الحقيقية
    // ========================================================================

    fn draw_categories(&mut self, ctx: &egui::Context) {
        let loc = Loc::new(self.language);
        let palette = ThemePalette::get(self.theme_mode);

        let all_count = self.downloads.len();
        let active_count = self
            .downloads
            .iter()
            .filter(|t| {
                matches!(
                    t.status,
                    DownloadStatus::Downloading
                        | DownloadStatus::RefreshingUrl
                        | DownloadStatus::Assembling
                        | DownloadStatus::VerifyingChecksum
                )
            })
            .count();
        let queued_count = self
            .downloads
            .iter()
            .filter(|t| t.status == DownloadStatus::Queued)
            .count();
        let paused_count = self
            .downloads
            .iter()
            .filter(|t| t.status == DownloadStatus::Paused)
            .count();
        let finished_count = self
            .downloads
            .iter()
            .filter(|t| t.status == DownloadStatus::Completed)
            .count();
        let failed_count = self
            .downloads
            .iter()
            .filter(|t| {
                matches!(
                    t.status,
                    DownloadStatus::Failed | DownloadStatus::NeedsUrlRefresh
                )
            })
            .count();

        let panel_resp = egui::SidePanel::left("stormget_categories")
            .resizable(true)
            .default_width(self.ui_state.sidebar_width)
            .min_width(155.0)
            .max_width(340.0)
            .frame(
                egui::Frame::new()
                    .fill(palette.sidebar_bg)
                    .inner_margin(egui::Margin::same(4)),
            )
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(palette.table_bg)
                    .stroke(Stroke::new(1.0, palette.border))
                    .show(ui, |ui| {
                        egui::Frame::new()
                            .fill(palette.header_bg)
                            .stroke(Stroke::new(0.5, palette.border))
                            .inner_margin(egui::Margin::symmetric(8, 4))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(loc.cat_title())
                                            .size(12.0)
                                            .strong()
                                            .color(palette.text_main),
                                    );
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        if ui
                                            .add(
                                                egui::Button::new(
                                                    RichText::new("×").size(13.0).strong(),
                                                )
                                                .frame(false),
                                            )
                                            .on_hover_text("(View ﺔﻤﺋﺎﻗ ﻦﻣ ﺎﻬﺗﺩﺎﻋﺇ ﻦﻜﻤﻳ) ﺔﻴﺒﻧﺎﺠﻟﺍ ﺔﺣﻮﻠﻟﺍ ﺀﺎﻔﺧﺇﻭ ﻲﻃ")
                                            .clicked()
                                        {
                                            self.show_categories_panel = false;
                                            self.ui_state.show_sidebar = false;
                                            self.mark_ui_state_dirty();
                                        }
                                    });
                                });
                            });

                        egui::ScrollArea::vertical()
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                ui.add_space(4.0);
                                ui.spacing_mut().item_spacing = Vec2::new(0.0, 2.0);

                                // 1. جميع التنزيلات والفئات الفرعية
                                if tree_root_row_with_count(
                                    ui,
                                    &palette,
                                    &mut self.expand_all_downloads,
                                    loc.cat_all(),
                                    all_count,
                                    TreeIconKind::FolderYellow,
                                    self.filter == DownloadFilter::All,
                                ) {
                                    self.filter = DownloadFilter::All;
                                }
                                if self.expand_all_downloads {
                                    for (cat, label, icon) in [
                                        (
                                            FileCategory::Video,
                                            loc.cat_video(),
                                            TreeIconKind::Video,
                                        ),
                                        (
                                            FileCategory::Compressed,
                                            loc.cat_compressed(),
                                            TreeIconKind::Compressed,
                                        ),
                                        (
                                            FileCategory::Programs,
                                            loc.cat_programs(),
                                            TreeIconKind::Program,
                                        ),
                                        (
                                            FileCategory::Documents,
                                            loc.cat_documents(),
                                            TreeIconKind::Document,
                                        ),
                                        (
                                            FileCategory::Music,
                                            loc.cat_music(),
                                            TreeIconKind::Music,
                                        ),
                                    ] {
                                        let cat_cnt = self
                                            .downloads
                                            .iter()
                                            .filter(|t| t.category == cat)
                                            .count();
                                        let target = DownloadFilter::Category(cat);
                                        if tree_child_row_with_count(
                                            ui,
                                            &palette,
                                            label,
                                            cat_cnt,
                                            icon,
                                            self.filter == target,
                                        ) {
                                            self.filter = target;
                                        }
                                    }
                                }

                                // 2. الحالات السريعة (النشطة، في الانتظار، المتوقفة، المكتملة، الفاشلة)
                                if tree_root_row_with_count(
                                    ui,
                                    &palette,
                                    &mut self.expand_unfinished,
                                    loc.tr("ﺔﻟﺎﺤﻟﺍ ﺐﺴﺣ", "By Status"),
                                    all_count,
                                    TreeIconKind::FolderActive,
                                    matches!(
                                        self.filter,
                                        DownloadFilter::Active
                                            | DownloadFilter::Queued
                                            | DownloadFilter::Paused
                                            | DownloadFilter::Finished
                                            | DownloadFilter::Failed
                                    ),
                                ) {
                                    self.filter = DownloadFilter::Active;
                                }
                                if self.expand_unfinished {
                                    for (flt, label, cnt, icon) in [
                                        (
                                            DownloadFilter::Active,
                                            loc.cat_active(),
                                            active_count,
                                            TreeIconKind::FolderActive,
                                        ),
                                        (
                                            DownloadFilter::Queued,
                                            loc.cat_queued(),
                                            queued_count,
                                            TreeIconKind::FolderQueued,
                                        ),
                                        (
                                            DownloadFilter::Paused,
                                            loc.cat_paused(),
                                            paused_count,
                                            TreeIconKind::FolderPaused,
                                        ),
                                        (
                                            DownloadFilter::Finished,
                                            loc.cat_finished(),
                                            finished_count,
                                            TreeIconKind::FolderFinished,
                                        ),
                                        (
                                            DownloadFilter::Failed,
                                            loc.cat_failed(),
                                            failed_count,
                                            TreeIconKind::FolderFailed,
                                        ),
                                    ] {
                                        if tree_child_row_with_count(
                                            ui,
                                            &palette,
                                            label,
                                            cnt,
                                            icon,
                                            self.filter == flt,
                                        ) {
                                            self.filter = flt;
                                        }
                                    }
                                }

                                // 3. الطوابير الفعلية الموجودة في المحرك
                                if tree_root_row_with_count(
                                    ui,
                                    &palette,
                                    &mut self.expand_queues,
                                    loc.cat_queues(),
                                    self.queues.len().max(2),
                                    TreeIconKind::FolderYellow,
                                    matches!(self.filter, DownloadFilter::Queue(_)),
                                ) {
                                    self.filter = DownloadFilter::Queue("main".to_owned());
                                }
                                if self.expand_queues {
                                    let q_names: Vec<String> = if self.queues.is_empty() {
                                        vec!["main".to_owned(), "sync".to_owned()]
                                    } else {
                                        self.queues.iter().map(|q| q.name.clone()).collect()
                                    };
                                    for q_name in q_names {
                                        let q_cnt = self
                                            .downloads
                                            .iter()
                                            .filter(|t| t.queue_name == q_name)
                                            .count();
                                        let display_lbl = if q_name == "main" {
                                            loc.cat_main_queue()
                                        } else if q_name == "sync" {
                                            loc.cat_sync_queue()
                                        } else {
                                            q_name.clone()
                                        };
                                        let target = DownloadFilter::Queue(q_name);
                                        if tree_child_row_with_count(
                                            ui,
                                            &palette,
                                            display_lbl,
                                            q_cnt,
                                            TreeIconKind::FolderYellow,
                                            self.filter == target,
                                        ) {
                                            self.filter = target;
                                        }
                                    }
                                }
                            });
                    });
            });

        let new_w = panel_resp.response.rect.width();
        if (new_w - self.ui_state.sidebar_width).abs() > 2.0 {
            self.ui_state.sidebar_width = new_w;
            self.mark_ui_state_dirty();
        }
    }

    // ========================================================================
    // المرحلة الخامسة والسادسة: جدول التنزيلات العالي الأداء + القوائم السياقية
    // ========================================================================

    fn draw_download_table(&mut self, ctx: &egui::Context) {
        let loc = Loc::new(self.language);
        let palette = ThemePalette::get(self.theme_mode);

        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(palette.panel_bg)
                    .inner_margin(egui::Margin::same(4)),
            )
            .show(ctx, |ui| {
                // 5.3 شريط المرشحات السريعة وأدوات التحديد المتعدد أعلى الجدول
                egui::Frame::new()
                    .fill(palette.bg_secondary)
                    .stroke(Stroke::new(1.0, palette.border))
                    .inner_margin(egui::Margin::symmetric(8, 4))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(loc.tr(":ﺔﻌﻳﺮﺳ ﺔﻴﻔﺼﺗ", "Quick Filter:"))
                                    .size(11.5)
                                    .color(palette.text_muted),
                            );
                            for (flt, ar_lbl, en_lbl) in [
                                (DownloadFilter::All, "ﻞﻜﻟﺍ", "All"),
                                (DownloadFilter::Active, "ﺔﻄﺸﻨﻟﺍ", "Active"),
                                (DownloadFilter::Finished, "ﺔﻠﻤﺘﻜﻤﻟﺍ", "Completed"),
                                (DownloadFilter::Paused, "ﺎﺘﻗﺆﻣ ﺔﻔﻗﻮﺘﻤﻟﺍ", "Paused"),
                                (DownloadFilter::Failed, "ﺔﻠﺷﺎﻔﻟﺍ", "Failed"),
                            ] {
                                let active = self.filter == flt;
                                if ui
                                    .selectable_label(active, loc.tr(ar_lbl, en_lbl))
                                    .clicked()
                                {
                                    self.filter = flt;
                                }
                            }

                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if ui
                                    .small_button(loc.tr("ﺓﺪﻤﻋﻷﺍ ⚙", "⚙ Columns"))
                                    .on_hover_text("ﻝﻭﺪﺠﻟﺍ ﺓﺪﻤﻋﺃ ﺐﻴﺗﺮﺗ ﺓﺩﺎﻋﺇ ﻭﺃ ﺀﺎﻔﺧﺇ ﻭﺃ ﺭﺎﻬﻇﺇ")
                                    .clicked()
                                {
                                    self.show_column_chooser_dialog = true;
                                }
                                if ui
                                    .small_button(loc.tr("ﺪﻳﺪﺤﺘﻟﺍ ﺲﻜﻋ ⇄", "⇄ Invert"))
                                    .on_hover_text("ﻝﻭﺪﺠﻟﺍ ﻲﻓ ﺓﺮﻫﺎﻈﻟﺍ ﺮﺻﺎﻨﻌﻟﺍ ﺪﻳﺪﺤﺗ ﺲﻜﻋ")
                                    .clicked()
                                {
                                    self.invert_visible_selection();
                                }
                                if ui
                                    .small_button(loc.tr("ﻞﻜﻟﺍ ﺪﻳﺪﺤﺗ ☑", "☑ Select All"))
                                    .on_hover_text("(Ctrl+A) ﺓﺮﻫﺎﻈﻟﺍ ﺮﺻﺎﻨﻌﻟﺍ ﻊﻴﻤﺟ ﺪﻳﺪﺤﺗ")
                                    .clicked()
                                {
                                    self.select_all_visible();
                                }
                                if self.selected_tasks.len() > 1 {
                                    ui.colored_label(
                                        palette.color_info,
                                        RichText::new(format!(
                                            "{} :ﺩﺪﺤﻣ",
                                            self.selected_tasks.len()
                                        ))
                                        .size(11.5)
                                        .strong(),
                                    );
                                }
                            });
                        });
                    });

                ui.add_space(3.0);

                let downloads: Vec<DownloadStateMeta> =
                    self.filtered_downloads().into_iter().cloned().collect();

                egui::Frame::new()
                    .fill(palette.table_bg)
                    .stroke(Stroke::new(1.0, palette.border))
                    .show(ui, |ui| {
<<<<<<< HEAD
                        // 15. حالات القائمة الفارغة أو عدم وجود نتائج بحث
                        if self.downloads.is_empty() {
                            let rect = ui.available_rect_before_wrap();
                            let resp = ui.allocate_rect(rect, Sense::click());
                            resp.context_menu(|ui| {
                                self.draw_empty_space_context_menu(ui, &loc);
                            });
                            ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
                                ui.centered_and_justified(|ui| {
                                    ui.vertical_centered(|ui| {
                                        ui.add_space(rect.height() * 0.30);
                                        ui.heading(
                                            RichText::new(loc.tr(
                                                "ﺎﻴﻟﺎﺣ ﺔﻏﺭﺎﻓ ﺕﻼﻳﺰﻨﺘﻟﺍ ﺔﻤﺋﺎﻗ",
                                                "Your download list is currently empty",
                                            ))
                                            .color(palette.text_main),
                                        );
                                        ui.label(
                                            RichText::new(loc.tr(
                                                ".ﺀﺪﺒﻠﻟ ﺢﻔﺼﺘﻤﻟﺍ ﻦﻣ ﺎﻴﺋﺎﻘﻠﺗ ﺕﻼﻳﺰﻨﺘﻟﺍ ﻂﻘﺘﻟﺍ ﻭﺃ ﺍﺪﻳﺪﺟ ﺎﻄﺑﺍﺭ ﻒﺿﺃ",
                                                "Add a new URL or intercept downloads from your browser to get started.",
                                            ))
                                            .color(palette.text_muted),
                                        );
                                        ui.add_space(10.0);
                                        if ui
                                            .button(
                                                RichText::new(loc.tr(
                                                    "(Ctrl+N) ﻥﻵﺍ ﻞﻳﺰﻨﺗ ﻝﻭﺃ ﺔﻓﺎﺿﺇ ➕",
                                                    "➕ Add First Download Now (Ctrl+N)",
                                                ))
                                                .strong()
                                                .color(Color32::WHITE),
                                            )
                                            .clicked()
                                        {
                                            self.open_or_focus_add_dialog();
                                        }
                                    });
                                });
                            });
                            return;
                        } else if downloads.is_empty() {
                            let rect = ui.available_rect_before_wrap();
                            let resp = ui.allocate_rect(rect, Sense::click());
                            resp.context_menu(|ui| {
                                self.draw_empty_space_context_menu(ui, &loc);
                            });
                            ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
                                ui.centered_and_justified(|ui| {
                                    ui.vertical_centered(|ui| {
                                        ui.add_space(rect.height() * 0.30);
                                        ui.heading(
                                            RichText::new(loc.tr(
                                                "ﻲﻟﺎﺤﻟﺍ ﺢﺷﺮﻤﻟﺍ ﻭﺃ ﺚﺤﺒﻠﻟ ﺔﻘﺑﺎﻄﻣ ﺞﺋﺎﺘﻧ ﺪﺟﻮﺗ ﻻ",
                                                "No downloads match the current search or filter",
                                            ))
                                            .color(palette.text_main),
                                        );
                                        ui.add_space(8.0);
                                        if ui
                                            .button(loc.tr(
                                                "ﻞﻜﻟﺍ ﺽﺮﻋﻭ ﺢﺷﺮﻤﻟﺍﻭ ﺚﺤﺒﻟﺍ ﺢﺴﻣ ↺",
                                                "↺ Clear Search & Filter",
                                            ))
                                            .clicked()
                                        {
                                            self.search_query.clear();
                                            self.filter = DownloadFilter::All;
=======
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
                                        for (title, width, col_id) in &columns {
                                            let (rect, resp) = ui.allocate_exact_size(Vec2::new(*width, 24.0), Sense::click());
                                            let painter = ui.painter();
                                            if resp.hovered() {
                                                painter.rect_filled(rect, 0.0, palette.toolbar_bg);
                                            }
                                            painter.line_segment([rect.right_top(), rect.right_bottom()], Stroke::new(1.0, palette.grid_line));
                                            let arrow = if self.sort_column == *col_id {
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
                                                if self.sort_column == *col_id {
                                                    self.sort_ascending = !self.sort_ascending;
                                                } else {
                                                    self.sort_column = *col_id;
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
                                let desc = task
                                    .description
                                    .clone()
                                    .unwrap_or_else(|| loc.category_name(task.category));
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
>>>>>>> efc40c730b5c3e9a4001ccff0b896d7f2f3ef344
                                        }
                                    });
                                });
                            });
                            return;
                        }

                        // حساب عرض الأعمدة الظاهرة
                        let visible_cols = self.ui_state.visible_columns.clone();
                        let available_w = ui.available_width().max(680.0);
                        let base_sum: f32 = visible_cols
                            .iter()
                            .map(|c| {
                                self.ui_state
                                    .column_widths
                                    .get(c)
                                    .copied()
                                    .unwrap_or_else(|| c.default_width())
                            })
                            .sum();
                        let scale_factor = if base_sum > 0.0 && available_w > base_sum {
                            available_w / base_sum
                        } else {
                            1.0
                        };

                        let col_specs: Vec<(TableColumnId, String, f32, SortColumn)> = visible_cols
                            .iter()
                            .map(|&col| {
                                let w = self
                                    .ui_state
                                    .column_widths
                                    .get(&col)
                                    .copied()
                                    .unwrap_or_else(|| col.default_width())
                                    * scale_factor;
                                let sort_col = match col {
                                    TableColumnId::Filename => SortColumn::Filename,
                                    TableColumnId::Status => SortColumn::Status,
                                    TableColumnId::TotalSize => SortColumn::Size,
                                    TableColumnId::Downloaded => SortColumn::Downloaded,
                                    TableColumnId::Progress => SortColumn::Progress,
                                    TableColumnId::Speed => SortColumn::Speed,
                                    TableColumnId::TimeLeft => SortColumn::TimeLeft,
                                    TableColumnId::Connections => SortColumn::Connections,
                                    TableColumnId::Category => SortColumn::Category,
                                    TableColumnId::CreatedAt => SortColumn::CreatedAt,
                                    TableColumnId::SaveFolder => SortColumn::SaveFolder,
                                    TableColumnId::Queue => SortColumn::Queue,
                                };
                                (col, loc.column_title(col), w.max(col.min_width()), sort_col)
                            })
                            .collect();

                        let total_row_width: f32 = col_specs.iter().map(|(_, _, w, _)| *w).sum();

                        // ترويسة الجدول (تدعم الترتيب والنقر بالزر الأيمن لإدارة الأعمدة)
                        egui::Frame::new()
                            .fill(palette.header_bg)
                            .stroke(Stroke::new(1.0, palette.border))
                            .show(ui, |ui| {
                                ui.spacing_mut().item_spacing = Vec2::ZERO;
                                ui.horizontal(|ui| {
                                    for (_col_id, title, width, sort_id) in &col_specs {
                                        let (rect, resp) = ui.allocate_exact_size(
                                            Vec2::new(*width, 26.0),
                                            Sense::click(),
                                        );
                                        let painter = ui.painter();
                                        if resp.hovered() {
                                            painter.rect_filled(rect, 0.0, palette.toolbar_bg);
                                        }
                                        painter.line_segment(
                                            [rect.right_top(), rect.right_bottom()],
                                            Stroke::new(1.0, palette.grid_line),
                                        );
                                        let arrow = if self.sort_column == *sort_id {
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
                                            egui::FontId::proportional(11.5),
                                            palette.text_main,
                                        );
                                        if resp.clicked() {
                                            if self.sort_column == *sort_id {
                                                self.sort_ascending = !self.sort_ascending;
                                            } else {
                                                self.sort_column = *sort_id;
                                                self.sort_ascending = true;
                                            }
                                            self.mark_ui_state_dirty();
                                        }
                                        resp.context_menu(|ui| {
                                            ui.label(RichText::new("ﺓﺪﻤﻋﻷﺍ ﺀﺎﻔﺧﺇ / ﺭﺎﻬﻇﺇ").strong());
                                            ui.separator();
                                            for c in TableColumnId::all_columns() {
                                                let mut vis =
                                                    self.ui_state.visible_columns.contains(&c);
                                                if ui
                                                    .checkbox(&mut vis, loc.column_title(c))
                                                    .changed()
                                                {
                                                    if vis {
                                                        self.ui_state.visible_columns.push(c);
                                                    } else if self.ui_state.visible_columns.len()
                                                        > 2
                                                    {
                                                        self.ui_state
                                                            .visible_columns
                                                            .retain(|&x| x != c);
                                                    }
                                                    self.mark_ui_state_dirty();
                                                }
                                            }
                                            ui.separator();
                                            if ui.button("...ﺓﺪﻤﻋﻷﺍ ﺐﻴﺗﺮﺗ ﺓﺬﻓﺎﻧ ﺢﺘﻓ ⚙").clicked()
                                            {
                                                self.show_column_chooser_dialog = true;
                                                ui.close_menu();
                                            }
                                        });
                                    }
                                });
                            });

                        // المرحلة 17: التمرير الافتراضي (Virtual Row Scrolling) للتعامل السلس مع مئات أو آلاف التنزيلات
                        let row_h = self.ui_state.row_density.row_height();
                        egui::ScrollArea::both()
                            .auto_shrink([false, false])
                            .show_rows(ui, row_h, downloads.len(), |ui, row_range| {
                                ui.spacing_mut().item_spacing = Vec2::ZERO;
                                for row_idx in row_range {
                                    let task = &downloads[row_idx];
                                    let selected = self.selected_tasks.contains(&task.task_id)
                                        || self.selected_task.as_deref()
                                            == Some(task.task_id.as_str());

                                    let (row_rect, response) = ui.allocate_exact_size(
                                        Vec2::new(total_row_width, row_h),
                                        Sense::click(),
                                    );

                                    let row_bg = if selected {
                                        palette.table_selected_row
                                    } else if response.hovered() {
                                        palette.table_hover_row
                                    } else if row_idx % 2 == 1 {
                                        palette.table_alt_row
                                    } else {
                                        palette.table_bg
                                    };

                                    let painter = ui.painter();
                                    painter.rect_filled(row_rect, 0.0, row_bg);
                                    painter.line_segment(
                                        [row_rect.left_bottom(), row_rect.right_bottom()],
                                        Stroke::new(1.0, palette.grid_line),
                                    );

                                    let mut x = row_rect.left();
                                    let cy = row_rect.center().y;
                                    let row_text_col = if selected {
                                        palette.text_on_accent
                                    } else {
                                        palette.text_main
                                    };
                                    let muted_col = if selected {
                                        palette.text_on_accent
                                    } else {
                                        palette.text_muted
                                    };

                                    let downloaded_bytes = task.total_downloaded();
                                    let speed: u64 = task.smoothed_speed_bps();

                                    for (col_id, _, col_w, _) in &col_specs {
                                        let cell_rect = Rect::from_min_size(
                                            Pos2::new(x, row_rect.top()),
                                            Vec2::new(*col_w, row_h),
                                        );
                                        let cell_painter = painter.with_clip_rect(cell_rect);

                                        match col_id {
                                            TableColumnId::Filename => {
                                                draw_file_category_icon(
                                                    &cell_painter,
                                                    Pos2::new(x + 12.0, cy),
                                                    task.category,
                                                );
                                                cell_painter.text(
                                                    Pos2::new(x + 24.0, cy),
                                                    egui::Align2::LEFT_CENTER,
                                                    &super::arabic::shape_arabic(&task.filename),
                                                    egui::FontId::proportional(12.0),
                                                    row_text_col,
                                                );
                                            }
                                            TableColumnId::Status => {
                                                let st_col = if selected {
                                                    Color32::WHITE
                                                } else {
                                                    palette.color_for_status(
                                                        task.status,
                                                        task.is_cancelled_by_user(),
                                                    )
                                                };
                                                draw_status_vector_icon(
                                                    &cell_painter,
                                                    Pos2::new(x + 11.0, cy),
                                                    task.status,
                                                    task.is_cancelled_by_user(),
                                                    st_col,
                                                );
                                                cell_painter.text(
                                                    Pos2::new(x + 22.0, cy),
                                                    egui::Align2::LEFT_CENTER,
                                                    loc.status_text_for_task(task),
                                                    egui::FontId::proportional(11.5),
                                                    st_col,
                                                );
                                            }
                                            TableColumnId::TotalSize => {
                                                cell_painter.text(
                                                    Pos2::new(x + 6.0, cy),
                                                    egui::Align2::LEFT_CENTER,
                                                    loc.format_bytes(task.total_size),
                                                    egui::FontId::proportional(11.5),
                                                    row_text_col,
                                                );
                                            }
                                            TableColumnId::Downloaded => {
                                                cell_painter.text(
                                                    Pos2::new(x + 6.0, cy),
                                                    egui::Align2::LEFT_CENTER,
                                                    loc.format_downloaded_bytes(downloaded_bytes),
                                                    egui::FontId::proportional(11.5),
                                                    row_text_col,
                                                );
                                            }
                                            TableColumnId::Progress => {
                                                draw_table_progress_cell(
                                                    &cell_painter,
                                                    cell_rect,
                                                    &palette,
                                                    task,
                                                    selected,
                                                );
                                            }
                                            TableColumnId::Speed => {
                                                let spd_str = if speed > 0 {
                                                    loc.format_speed(speed)
                                                } else {
                                                    "—".to_owned()
                                                };
                                                cell_painter.text(
                                                    Pos2::new(x + 6.0, cy),
                                                    egui::Align2::LEFT_CENTER,
                                                    spd_str,
                                                    egui::FontId::proportional(11.5),
                                                    row_text_col,
                                                );
                                            }
                                            TableColumnId::TimeLeft => {
                                                // لا نعرض وقتاً متبقياً مضللاً إذا كانت السرعة = 0 أو الحجم مجهولاً
                                                let t_left = task
                                                    .estimated_eta_secs()
                                                    .map(|secs| loc.time_left_str(secs))
                                                    .unwrap_or_else(|| "—".to_owned());
                                                cell_painter.text(
                                                    Pos2::new(x + 6.0, cy),
                                                    egui::Align2::LEFT_CENTER,
                                                    t_left,
                                                    egui::FontId::proportional(11.5),
                                                    muted_col,
                                                );
                                            }
                                            TableColumnId::Connections => {
                                                let active_c = task
                                                    .segments
                                                    .iter()
                                                    .filter(|s| {
                                                        s.status == SegmentStatus::Downloading
                                                    })
                                                    .count();
                                                let conn_str = if task.segments.is_empty() {
                                                    "—".to_owned()
                                                } else {
                                                    format!("{}/{}", active_c, task.segments.len())
                                                };
                                                cell_painter.text(
                                                    Pos2::new(x + 8.0, cy),
                                                    egui::Align2::LEFT_CENTER,
                                                    conn_str,
                                                    egui::FontId::monospace(11.0),
                                                    muted_col,
                                                );
                                            }
                                            TableColumnId::Category => {
                                                cell_painter.text(
                                                    Pos2::new(x + 6.0, cy),
                                                    egui::Align2::LEFT_CENTER,
                                                    loc.category_name(task.category),
                                                    egui::FontId::proportional(11.5),
                                                    muted_col,
                                                );
                                            }
                                            TableColumnId::CreatedAt => {
                                                let date_str = task
                                                    .created_at
                                                    .format("%Y-%m-%d %H:%M")
                                                    .to_string();
                                                cell_painter.text(
                                                    Pos2::new(x + 6.0, cy),
                                                    egui::Align2::LEFT_CENTER,
                                                    date_str,
                                                    egui::FontId::proportional(11.0),
                                                    muted_col,
                                                );
                                            }
                                            TableColumnId::SaveFolder => {
                                                cell_painter.text(
                                                    Pos2::new(x + 6.0, cy),
                                                    egui::Align2::LEFT_CENTER,
                                                    task.save_dir.display().to_string(),
                                                    egui::FontId::proportional(11.0),
                                                    muted_col,
                                                );
                                            }
                                            TableColumnId::Queue => {
                                                draw_queue_mini_icon(
                                                    &cell_painter,
                                                    Pos2::new(x + 12.0, cy),
                                                    task.status == DownloadStatus::Completed,
                                                );
                                                cell_painter.text(
                                                    Pos2::new(x + 22.0, cy),
                                                    egui::Align2::LEFT_CENTER,
                                                    &task.queue_name,
                                                    egui::FontId::proportional(11.0),
                                                    muted_col,
                                                );
                                            }
                                        }
                                        x += *col_w;
                                    }

                                    // Tooltip للاسم الكامل والمسار والخطأ عند التحويم
                                    let hover_info = if let Some(err) = &task.last_error {
                                        format!(
                                            "{}\n :ﺭﺎﺴﻤﻟﺍ{}\n :ﻂﺑﺍﺮﻟﺍ{}\n :ﺄﻄﺨﻟﺍ{}",
                                            task.filename,
                                            task.final_file_path().display(),
                                            task.url,
                                            err
                                        )
                                    } else {
                                        format!(
                                            "{}\n :ﺭﺎﺴﻤﻟﺍ{}\n :ﻂﺑﺍﺮﻟﺍ{}",
                                            task.filename,
                                            task.final_file_path().display(),
                                            task.url
                                        )
                                    };
                                    let response = response.on_hover_text(hover_info);

                                    // التعامل مع النقر الفردي والمتعدد (Ctrl / Shift)
                                    if response.clicked() {
                                        let mods = ui.input(|i| i.modifiers);
                                        if mods.ctrl {
                                            if self.selected_tasks.contains(&task.task_id) {
                                                self.selected_tasks.remove(&task.task_id);
                                            } else {
                                                self.selected_tasks.insert(task.task_id.clone());
                                                self.selected_task = Some(task.task_id.clone());
                                            }
                                            self.selection_anchor = Some(task.task_id.clone());
                                        } else if mods.shift {
                                            if let Some(anchor) = self.selection_anchor.clone() {
                                                let pos_a = downloads
                                                    .iter()
                                                    .position(|d| d.task_id == anchor);
                                                if let Some(a_idx) = pos_a {
                                                    let low = a_idx.min(row_idx);
                                                    let high = a_idx.max(row_idx);
                                                    self.selected_tasks.clear();
                                                    for item in &downloads[low..=high] {
                                                        self.selected_tasks
                                                            .insert(item.task_id.clone());
                                                    }
                                                    self.selected_task =
                                                        Some(task.task_id.clone());
                                                } else {
                                                    self.select_single_task(task.task_id.clone());
                                                }
                                            } else {
                                                self.select_single_task(task.task_id.clone());
                                            }
                                        } else {
                                            self.select_single_task(task.task_id.clone());
                                        }
                                    }

                                    if response.double_clicked() {
                                        self.select_single_task(task.task_id.clone());
                                        self.open_or_focus_details_dialog();
                                    }

                                    // المرحلة السادسة: القائمة السياقية المخصصة لكل حالة
                                    let task_clone = task.clone();
                                    response.context_menu(|ui| {
                                        if !self.selected_tasks.contains(&task_clone.task_id) {
                                            self.select_single_task(task_clone.task_id.clone());
                                        }
                                        if self.selected_tasks.len() > 1 {
                                            self.draw_multi_select_context_menu(ui, &loc);
                                        } else {
                                            self.draw_single_task_context_menu(
                                                ui,
                                                &loc,
                                                &palette,
                                                &task_clone,
                                            );
                                        }
                                    });
                                }

                                // مساحة فارغة أسفل الصفوف للنقر بالزر الأيمن على مساحة فارغة (المرحلة 6.5)
                                let rem_h = ui.available_height();
                                if rem_h > 20.0 {
                                    let (empty_rect, empty_resp) = ui.allocate_exact_size(
                                        Vec2::new(total_row_width, rem_h),
                                        Sense::click(),
                                    );
                                    let _ = empty_rect;
                                    if empty_resp.clicked() {
                                        self.selected_tasks.clear();
                                        self.selected_task = None;
                                    }
                                    empty_resp.context_menu(|ui| {
                                        self.draw_empty_space_context_menu(ui, &loc);
                                    });
                                }
                            });
                    });
            });
    }

    // ========================================================================
    // المرحلة السادسة: القوائم السياقية الـ 6 (Active, Paused, Completed, Failed, Multi, Empty)
    // ========================================================================

    fn draw_single_task_context_menu(
        &mut self,
        ui: &mut egui::Ui,
        loc: &Loc,
        palette: &ThemePalette,
        task: &DownloadStateMeta,
    ) {
        let path = task.final_file_path();
        let file_exists = path.exists();
        let is_cancelled = task.is_cancelled_by_user();

        ui.label(RichText::new(&task.filename).strong());
        ui.label(
            RichText::new(loc.status_text_for_task(task))
                .size(11.0)
                .color(palette.color_for_status(task.status, is_cancelled)),
        );
        ui.separator();

        match task.status {
            // 6.1 عند النقر على تنزيل نشط
            DownloadStatus::Downloading
            | DownloadStatus::RefreshingUrl
            | DownloadStatus::Assembling
            | DownloadStatus::VerifyingChecksum => {
                // فتح الملف معطّل لأنه لم يكتمل بعد، مع إتاحة المعاينة الحية أثناء التحميل للفيديو والصوت
                ui.add_enabled(false, egui::Button::new(format!("(ﻞﻤﺘﻜﻣ ﺮﻴﻏ) {} 📂", loc.ctx_open())));
                if matches!(task.category, FileCategory::Video | FileCategory::Music)
                    && ui
                        .button("🎬 (Play Preview) ﻞﻴﻤﺤﺘﻟﺍ ﺀﺎﻨﺛﺃ ﺔﻨﻳﺎﻌﻣ ▶")
                        .clicked()
                {
                    let _ = self
                        .commands
                        .send(UiCommand::PreviewTask(task.task_id.clone()));
                    ui.close_menu();
                }
                if ui.button(format!("📁 {}", loc.ctx_open_folder())).clicked() {
                    open_folder(&task.save_dir);
                    ui.close_menu();
                }
                ui.separator();
                if ui.button(format!("⏸ {}", loc.ctx_pause())).clicked() {
                    let _ = self.commands.send(UiCommand::Control {
                        task_id: task.task_id.clone(),
                        action: "pause",
                    });
                    ui.close_menu();
                }
                if ui.button(format!("⏹ {}", loc.ctx_cancel())).clicked() {
                    self.request_cancel_tasks(vec![task.task_id.clone()]);
                    ui.close_menu();
                }
                ui.separator();
                if ui.button(format!("📋 {}", loc.ctx_copy_url())).clicked() {
                    ui.ctx().copy_text(task.url.clone());
                    ui.close_menu();
                }
                if ui
                    .button(format!("📋 {}", loc.ctx_copy_filename()))
                    .clicked()
                {
                    ui.ctx().copy_text(task.filename.clone());
                    ui.close_menu();
                }
                ui.separator();
                if ui.button(format!("🔍 {}", loc.ctx_properties())).clicked() {
                    self.dialog_tab = DialogTab::Progress;
                    self.open_or_focus_details_dialog();
                    ui.close_menu();
                }
                if ui
                    .button("🩺 (Download Doctor) ﻲﻛﺬﻟﺍ ﻞﻄﻌﻟﺍ ﺺﻴﺨﺸﺗ")
                    .clicked()
                {
                    let _ = self
                        .commands
                        .send(UiCommand::DiagnoseTask(task.task_id.clone()));
                    self.dialog_tab = DialogTab::Logs;
                    self.open_or_focus_details_dialog();
                    ui.close_menu();
                }
                if ui
                    .button(format!("📜 {}", loc.ctx_view_error_log()))
                    .clicked()
                {
                    self.dialog_tab = DialogTab::Logs;
                    self.open_or_focus_details_dialog();
                    ui.close_menu();
                }
            }

            // 6.2 عند النقر على تنزيل متوقف مؤقتاً أو في الانتظار (لا يعرض أمر الإيقاف المؤقت كأنه صالح)
            DownloadStatus::Paused | DownloadStatus::Queued => {
                if ui
                    .button(RichText::new(format!("▶ {}", loc.ctx_resume())).strong())
                    .clicked()
                {
                    let _ = self.commands.send(UiCommand::Control {
                        task_id: task.task_id.clone(),
                        action: "resume",
                    });
                    ui.close_menu();
                }
                if ui.button(format!("⏹ {}", loc.ctx_cancel())).clicked() {
                    self.request_cancel_tasks(vec![task.task_id.clone()]);
                    ui.close_menu();
                }
                if ui.button(format!("🔄 {}", loc.ctx_redownload())).clicked() {
                    let _ = self
                        .commands
                        .send(UiCommand::Redownload(task.task_id.clone()));
                    ui.close_menu();
                }
                ui.separator();
                if ui.button(format!("📁 {}", loc.ctx_open_folder())).clicked() {
                    open_folder(&task.save_dir);
                    ui.close_menu();
                }
                if ui
                    .button(format!("✏ {}", loc.ctx_rename_file()))
                    .clicked()
                {
                    self.open_rename_dialog_for_task(task);
                    ui.close_menu();
                }
                if ui.button(format!("📋 {}", loc.ctx_copy_url())).clicked() {
                    ui.ctx().copy_text(task.url.clone());
                    ui.close_menu();
                }
                ui.separator();
                if ui.button(format!("🔍 {}", loc.ctx_properties())).clicked() {
                    self.dialog_tab = DialogTab::General;
                    self.open_or_focus_details_dialog();
                    ui.close_menu();
                }
                ui.separator();
                if ui.button(format!("🗑 {}", loc.ctx_remove())).clicked() {
                    self.request_delete_selected(false);
                    ui.close_menu();
                }
            }

            // 6.3 عند النقر على تنزيل مكتمل
            DownloadStatus::Completed => {
                if !file_exists {
                    ui.colored_label(
                        palette.color_error,
                        "!ﺹﺮﻘﻟﺍ ﻰﻠﻋ ﺩﻮﺟﻮﻣ ﺮﻴﻏ ﻲﺋﺎﻬﻨﻟﺍ ﻒﻠﻤﻟﺍ ⚠",
                    );
                }
                if ui
                    .add_enabled(file_exists, egui::Button::new(format!("📂 {}", loc.ctx_open())))
                    .clicked()
                {
                    open_file(&path);
                    ui.close_menu();
                }
                if ui
                    .add_enabled(
                        file_exists,
                        egui::Button::new(format!("📂 {}", loc.ctx_open_with())),
                    )
                    .clicked()
                {
                    open_file_with(&path);
                    ui.close_menu();
                }
                if ui.button(format!("📁 {}", loc.ctx_open_folder())).clicked() {
                    if file_exists {
                        reveal_in_explorer(&path);
                    } else {
                        open_folder(&task.save_dir);
                    }
                    ui.close_menu();
                }
                ui.separator();
                if ui.button(format!("📋 {}", loc.ctx_copy_path())).clicked() {
                    ui.ctx().copy_text(path.display().to_string());
                    ui.close_menu();
                }
                if ui
                    .button(format!("📋 {}", loc.ctx_copy_filename()))
                    .clicked()
                {
                    ui.ctx().copy_text(task.filename.clone());
                    ui.close_menu();
                }
                if ui.button(format!("📋 {}", loc.ctx_copy_url())).clicked() {
                    ui.ctx().copy_text(task.url.clone());
                    ui.close_menu();
                }
                if ui
                    .add_enabled(
                        file_exists,
                        egui::Button::new(format!("✏ {}", loc.ctx_rename_file())),
                    )
                    .clicked()
                {
                    self.open_rename_dialog_for_task(task);
                    ui.close_menu();
                }
                if ui
                    .add_enabled(
                        file_exists,
                        egui::Button::new(format!("✔ {}", loc.ctx_reverify())),
                    )
                    .clicked()
                {
                    let _ = self
                        .commands
                        .send(UiCommand::ReverifyChecksum(task.task_id.clone()));
                    ui.close_menu();
                }
                if !file_exists
                    && ui
                        .button(format!("🔄 {}", loc.ctx_redownload()))
                        .clicked()
                {
                    let _ = self
                        .commands
                        .send(UiCommand::Redownload(task.task_id.clone()));
                    ui.close_menu();
                }
                ui.separator();
                if ui.button(format!("🔍 {}", loc.ctx_properties())).clicked() {
                    self.dialog_tab = DialogTab::Integrity;
                    self.open_or_focus_details_dialog();
                    ui.close_menu();
                }
                ui.separator();
                if ui.button(format!("🗑 {}", loc.ctx_remove())).clicked() {
                    self.request_delete_selected(false);
                    ui.close_menu();
                }
                if ui
                    .add_enabled(
                        file_exists,
                        egui::Button::new(
                            RichText::new(format!("🔥 {}", loc.ctx_delete_with_file()))
                                .color(palette.color_error),
                        ),
                    )
                    .clicked()
                {
                    self.request_delete_selected(true);
                    ui.close_menu();
                }
            }

            // 6.4 عند النقر على تنزيل فاشل أو منتهي الصلاحية أو ملغى
            DownloadStatus::Failed | DownloadStatus::NeedsUrlRefresh => {
                if task.is_expired_link() {
                    ui.colored_label(
                        palette.color_warning,
                        ":ﺚﻳﺪﺤﺗ ﻰﻟﺇ ﻂﺑﺍﺮﻟﺍ ﺝﺎﺘﺤﻳ ﺪﻗ — ﻂﺑﺍﺮﻟﺍ ﺔﻴﺣﻼﺻ ﺖﻬﺘﻧﺍ ⚠",
                    );
                } else if let Some(err) = &task.last_error {
                    ui.colored_label(palette.color_error, format!("{err} :ﺐﺒﺴﻟﺍ"));
                }

                if !is_cancelled {
                    if ui
                        .button(RichText::new(format!("🔄 {}", loc.ctx_retry())).strong())
                        .clicked()
                    {
                        let _ = self.commands.send(UiCommand::Control {
                            task_id: task.task_id.clone(),
                            action: "resume",
                        });
                        ui.close_menu();
                    }
                }
                if ui
                    .button(format!("🔁 {}", loc.ctx_redownload()))
                    .clicked()
                {
                    let _ = self
                        .commands
                        .send(UiCommand::Redownload(task.task_id.clone()));
                    ui.close_menu();
                }
                if ui
                    .button(format!("🔗 {}", loc.ctx_refresh_url()))
                    .clicked()
                {
                    self.refresh_task_id.clone_from(&task.task_id);
                    self.refresh_new_url.clone_from(&task.url);
                    self.show_refresh_dialog = true;
                    ui.close_menu();
                }
                if ui
                    .button(format!("📜 {}", loc.ctx_view_error_log()))
                    .clicked()
                {
                    self.dialog_tab = DialogTab::Logs;
                    self.open_or_focus_details_dialog();
                    ui.close_menu();
                }
                ui.separator();
                if ui.button(format!("📋 {}", loc.ctx_copy_url())).clicked() {
                    ui.ctx().copy_text(task.url.clone());
                    ui.close_menu();
                }
                if ui.button(format!("📁 {}", loc.ctx_open_folder())).clicked() {
                    open_folder(&task.save_dir);
                    ui.close_menu();
                }
                ui.separator();
                if ui.button(format!("🗑 {}", loc.ctx_remove())).clicked() {
                    self.request_delete_selected(false);
                    ui.close_menu();
                }
            }
        }
    }

    fn draw_multi_select_context_menu(&mut self, ui: &mut egui::Ui, loc: &Loc) {
        ui.label(
            RichText::new(format!(
                "(ﺓﺩﺪﺤﻣ ﺮﺻﺎﻨﻋ ){} ﺔﻴﻋﺎﻤﺟ ﺮﻣﺍﻭﺃ",
                self.selected_tasks.len()
            ))
            .strong(),
        );
        ui.separator();

        if ui
            .add_enabled(
                self.can_resume_selected(),
                egui::Button::new("ﻑﺎﻨﺌﺘﺳﻼﻟ ﺔﻠﺑﺎﻘﻟﺍ ﺮﺻﺎﻨﻌﻟﺍ ﻑﺎﻨﺌﺘﺳﺍ ▶"),
            )
            .clicked()
        {
            self.resume_selected_valid_tasks();
            ui.close_menu();
        }
        if ui
            .add_enabled(
                self.can_pause_selected(),
                egui::Button::new("ﺎﺘﻗﺆﻣ ﺔﻄﺸﻨﻟﺍ ﺮﺻﺎﻨﻌﻟﺍ ﻑﺎﻘﻳﺇ ⏸"),
            )
            .clicked()
        {
            self.pause_selected_valid_tasks();
            ui.close_menu();
        }
        if ui
            .add_enabled(
                self.can_retry_selected(),
                egui::Button::new("ﺔﻠﺷﺎﻔﻟﺍ ﺮﺻﺎﻨﻌﻟﺍ ﺔﻟﻭﺎﺤﻣ ﺓﺩﺎﻋﺇ 🔄"),
            )
            .clicked()
        {
            self.retry_selected_failed_tasks();
            ui.close_menu();
        }
        if ui
            .add_enabled(
                self.can_cancel_selected(),
                egui::Button::new("ﺔﻠﻤﺘﻜﻤﻟﺍ ﺮﻴﻏ ﺮﺻﺎﻨﻌﻟﺍ ﺀﺎﻐﻟﺇ ⏹"),
            )
            .clicked()
        {
            let ids = self.effective_selected_ids();
            self.request_cancel_tasks(ids);
            ui.close_menu();
        }
        ui.separator();
        if ui.button("ﺓﺩﺪﺤﻤﻟﺍ ﻂﺑﺍﻭﺮﻟﺍ ﻊﻴﻤﺟ ﺦﺴﻧ 📋").clicked() {
            let urls = self
                .selected_downloads_list()
                .into_iter()
                .map(|t| t.url.clone())
                .collect::<Vec<_>>()
                .join("\n");
            ui.ctx().copy_text(urls);
            ui.close_menu();
        }
        ui.separator();
        if ui.button(format!("🗑 {}", loc.ctx_remove())).clicked() {
            self.request_delete_selected(false);
            ui.close_menu();
        }
        if ui
            .button(format!("🔥 {}", loc.ctx_delete_with_file()))
            .clicked()
        {
            self.request_delete_selected(true);
            ui.close_menu();
        }
    }

    // 6.5 عند النقر على مساحة فارغة في الجدول
    fn draw_empty_space_context_menu(&mut self, ui: &mut egui::Ui, loc: &Loc) {
        if ui
            .button(loc.tr("...ﺪﻳﺪﺟ ﻞﻳﺰﻨﺗ ﺔﻓﺎﺿﺇ ➕", "➕ Add New Download..."))
            .clicked()
        {
            self.open_or_focus_add_dialog();
            ui.close_menu();
        }
        if ui
            .button(loc.tr("...ﻂﺑﺍﻭﺭ ﺔﻌﻓﺩ ﺔﻓﺎﺿﺇ 📦", "📦 Add Batch Downloads..."))
            .clicked()
        {
            self.show_batch_dialog = true;
            ui.close_menu();
        }
        ui.separator();
        if ui
            .button(loc.tr("(Ctrl+A) ﻞﻜﻟﺍ ﺪﻳﺪﺤﺗ ☑", "☑ Select All (Ctrl+A)"))
            .clicked()
        {
            self.select_all_visible();
            ui.close_menu();
        }
        if ui
            .button(loc.tr("ﻲﻟﺎﺤﻟﺍ ﺚﺤﺒﻟﺍﻭ ﺢﺷﺮﻤﻟﺍ ﺢﺴﻣ ↺", "↺ Clear Current Filter & Search"))
            .clicked()
        {
            self.filter = DownloadFilter::All;
            self.search_query.clear();
            ui.close_menu();
        }
        ui.separator();
        if ui
            .button(loc.tr("...ﺎﻬﺒﻴﺗﺮﺗﻭ ﺓﺮﻫﺎﻈﻟﺍ ﺓﺪﻤﻋﻷﺍ ﺩﺍﺪﻋﺇ 📊", "📊 Configure Columns..."))
            .clicked()
        {
            self.show_column_chooser_dialog = true;
            ui.close_menu();
        }
    }

    fn draw_bottom_inspector(&mut self, ctx: &egui::Context) {
        let loc = Loc::new(self.language);
        let palette = ThemePalette::get(self.theme_mode);

        egui::TopBottomPanel::bottom("stormget_bottom_inspector")
            .resizable(true)
            .default_height(155.0)
            .min_height(110.0)
            .frame(
                egui::Frame::new()
                    .fill(palette.panel_bg)
                    .stroke(Stroke::new(1.0, palette.border))
                    .inner_margin(egui::Margin::symmetric(8, 5)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    for (tab, label) in [
                        (
                            BottomInspectorTab::SegmentMap,
                            loc.tr("(Segments) ﻊﻃﺎﻘﻤﻟﺍ ﺔﻄﻳﺮﺧ", "Segment Map"),
                        ),
                        (
                            BottomInspectorTab::SpeedGraph,
                            loc.tr("(Speed Graph) ﺔﻋﺮﺴﻟﺍ ﻰﻨﺤﻨﻣ", "Speed Graph"),
                        ),
                        (
                            BottomInspectorTab::ConnectionLogs,
                            loc.tr("(Logs) ﺙﺍﺪﺣﻷﺍ ﻞﺠﺳ", "Logs"),
                        ),
                        (
                            BottomInspectorTab::ChecksumAudit,
                            loc.tr("(Integrity) ﺔﻴﻤﻗﺮﻟﺍ ﺕﺎﻤﺼﺒﻟﺍ", "Digests"),
                        ),
                    ] {
                        if ui
                            .selectable_label(self.bottom_inspector_tab == tab, label)
                            .clicked()
                        {
                            self.bottom_inspector_tab = tab;
                            self.mark_ui_state_dirty();
                        }
                    }

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .small_button("×")
                            .on_hover_text("ﺔﻴﻠﻔﺴﻟﺍ ﺺﺤﻔﻟﺍ ﺔﺣﻮﻟ ﺀﺎﻔﺧﺇ")
                            .clicked()
                        {
                            self.show_bottom_inspector = false;
                            self.ui_state.show_bottom_inspector = false;
                            self.mark_ui_state_dirty();
                        }
                        if self.has_selected()
                            && ui
                                .small_button("ﺔﻠﻘﺘﺴﻣ ﻞﻴﺻﺎﻔﺗ ﺓﺬﻓﺎﻧ 🗗")
                                .on_hover_text("ﺔﻠﻘﺘﺴﻣ ﺔﻴﺟﺭﺎﺧ ﺓﺬﻓﺎﻧ ﻲﻓ ﺔﻤﻬﻤﻟﺍ ﻞﻴﺻﺎﻔﺗ ﺢﺘﻓ")
                                .clicked()
                        {
                            self.open_or_focus_details_dialog();
                        }
                    });
                });
                ui.separator();

                let Some(task) = self.selected_download().cloned() else {
                    ui.centered_and_justified(|ui| {
                        ui.label(
                            RichText::new(loc.tr(
                                "ﺔﻴﻤﻗﺮﻟﺍ ﺕﺎﻤﺼﺒﻟﺍﻭ ﺔﻋﺮﺴﻟﺍ ﻰﻨﺤﻨﻣﻭ ﻊﻃﺎﻘﻤﻟﺍ ﺔﻄﻳﺮﺧ ﺺﺤﻔﻟ ﻝﻭﺪﺠﻟﺍ ﻦﻣ ﺔﻤﻬﻣ ﺩﺪﺣ",
                                "Select a download task to inspect segments, speed curve, and checksums",
                            ))
                            .color(palette.text_muted),
                        );
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
                                ui.colored_label(
                                    badge_col,
                                    format!(
                                        "#{} [{}..{}] ({})",
                                        seg.id + 1,
                                        loc.format_downloaded_bytes(seg.start_byte),
                                        if seg.end_byte == u64::MAX {
                                            "EOF".into()
                                        } else {
                                            loc.format_downloaded_bytes(seg.end_byte)
                                        },
                                        loc.format_speed(seg.last_speed_bps)
                                    ),
                                );
                            }
                        });
                    }
                    BottomInspectorTab::SpeedGraph => {
                        draw_live_speed_graph(ui, &palette, &task);
                    }
                    BottomInspectorTab::ConnectionLogs => {
                        egui::ScrollArea::vertical().show(ui, |ui| {
                            for entry in task.connection_logs.iter().rev() {
                                let safe_msg =
                                    crate::core::storage::redact_log_message(&entry.message);
                                ui.label(format!(
                                    "[{}] [{}] {}",
                                    entry.timestamp.format("%H:%M:%S"),
                                    entry.level,
                                    safe_msg
                                ));
                            }
                        });
                    }
                    BottomInspectorTab::ChecksumAudit => {
                        ui.label(format!(
                            "SHA-256: {}",
                            task.computed_sha256.as_deref().unwrap_or("ﺪﻌﺑ ﺐﺴﺤﻳ ﻢﻟ")
                        ));
                        ui.label(format!(
                            "MD5:     {}",
                            task.computed_md5.as_deref().unwrap_or("ﺪﻌﺑ ﺐﺴﺤﻳ ﻢﻟ")
                        ));
                        ui.label(format!(
                            "CRC32:   {}",
                            task.computed_crc32.as_deref().unwrap_or("ﺪﻌﺑ ﺐﺴﺤﻳ ﻢﻟ")
                        ));
                    }
                }
            });
    }

    pub fn filtered_downloads(&self) -> Vec<&DownloadStateMeta> {
        let query_lower = self.search_query.trim().to_lowercase();
        let loc = Loc::new(self.language);
        let mut list: Vec<&DownloadStateMeta> = self
            .downloads
            .iter()
            .filter(|task| match &self.filter {
                DownloadFilter::All => true,
                DownloadFilter::Active => matches!(
                    task.status,
                    DownloadStatus::Downloading
                        | DownloadStatus::RefreshingUrl
                        | DownloadStatus::Assembling
                        | DownloadStatus::VerifyingChecksum
                ),
                DownloadFilter::Queued => task.status == DownloadStatus::Queued,
                DownloadFilter::Paused => task.status == DownloadStatus::Paused,
                DownloadFilter::Unfinished => task.status != DownloadStatus::Completed,
                DownloadFilter::Finished => task.status == DownloadStatus::Completed,
                DownloadFilter::Failed => matches!(
                    task.status,
                    DownloadStatus::Failed | DownloadStatus::NeedsUrlRefresh
                ),
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
                            .category
                            .folder_name()
                            .to_lowercase()
                            .contains(&query_lower)
                        || loc
                            .status_text_for_task(task)
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
                SortColumn::Downloaded => a.total_downloaded().cmp(&b.total_downloaded()),
                SortColumn::Progress => {
                    let pa = if a.total_size > 0 {
                        (a.total_downloaded() * 10000) / a.total_size
                    } else {
                        0
                    };
                    let pb = if b.total_size > 0 {
                        (b.total_downloaded() * 10000) / b.total_size
                    } else {
                        0
                    };
                    pa.cmp(&pb)
                }
                SortColumn::Status => (a.status as u8).cmp(&(b.status as u8)),
                SortColumn::TimeLeft => a
                    .total_size
                    .saturating_sub(a.total_downloaded())
                    .cmp(&b.total_size.saturating_sub(b.total_downloaded())),
                SortColumn::Speed => {
                    let sa: u64 = a.smoothed_speed_bps();
                    let sb: u64 = b.smoothed_speed_bps();
                    sa.cmp(&sb)
                }
                SortColumn::Connections => a.segments.len().cmp(&b.segments.len()),
                SortColumn::UpdatedAt => a.updated_at.cmp(&b.updated_at),
                SortColumn::CreatedAt => a.created_at.cmp(&b.created_at),
                SortColumn::Category => (a.category as u8).cmp(&(b.category as u8)),
                SortColumn::SaveFolder => a.save_dir.cmp(&b.save_dir),
            };
            if self.sort_ascending {
                cmp
            } else {
                cmp.reverse()
            }
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
                cmd.args(["/s", "/t", "30"]);
                if force {
                    cmd.arg(force_flag);
                }
                let _ = cmd.spawn();
            }
            ShutdownAction::Restart => {
                let mut cmd = std::process::Command::new("shutdown");
                cmd.args(["/r", "/t", "30"]);
                if force {
                    cmd.arg(force_flag);
                }
                let _ = cmd.spawn();
            }
            ShutdownAction::Sleep => {
                let _ = std::process::Command::new("rundll32.exe")
                    .args(["powrprof.dll,SetSuspendState", "0,1,0"])
                    .spawn();
            }
        }
    }
}
