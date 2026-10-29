//! مسارات خادم التحكم المحلي المحصّن أمنياً (Hardened Axum IPC Endpoints)، حالة المحرك (`AppState`)،
//! عزل صلاحيات إضافة المتصفح عن المسارات الإدارية الحساسة (`/config` و`/queues`)،
//! التحقق من سلامة ملفات `.part` عند الاستعادة، ودمج تدفقات الفيديو والصوت المتوازية (`direct_mux`).

use crate::{
    core::{
        limiter::SpeedLimiter,
        doctor::{
            diagnose_task_with_doctor, evaluate_link_intelligence, DoctorDiagnosis,
            LinkIntelligenceReport,
        },
        lanes::{LaneStatus, MultiNetworkLanePool, NetworkRouteSpec},
        preview::{cleanup_finished_previews, play_partial_download},
        publisher::{discover_publisher_checksum, PublisherChecksum},
        smart_throttle::{GamePolicy, SmartThrottleMode, SmartThrottleStatus},
        segmenter::{
            build_optimized_http_client, validate_part_file_against_segments, DynamicDownloadJob,
        },
        selfupdate::{check_for_signed_update, download_signed_installer, SignedReleaseManifest},
        torrent::{
            is_magnet, is_torrent_source, magnet_name, parse_torrent, Aria2TorrentEngine,
            TorrentAddOptions, TorrentInfo,
        },
        storage::{
            compute_file_all_digests, sanitize_filename, save_meta_to_disk_atomic, sync_parent_dir,
            ConnectionLogEntry, DownloadStateMeta, DownloadStatus, FileCategory, FileSegment,
            SegmentStatus, MIN_SPLIT_SIZE,
        },
        tools::{
            find_tool, get_tools_status, hide_tokio_console_window, merge_video_and_audio_streams,
            set_system_awake_for_downloads, spawn_background_tools_updater, update_all_tools,
            validate_safe_executable_path, ToolsStatusReport,
        },
    },
    inspector::{
        crawler::{BoundedSiteCrawler, CrawlConfig, CrawlReport},
        zip::{
            BatchExpandRequest, BatchExpandResponse, DownloadListImporter, RemoteZipInspector,
            RemoteZipPreviewResponse, WildcardPatternExpander,
        },
    },
    ipc::native_host::{
        connect_installed_browsers, constant_time_token_eq, generate_bridge_token,
        register_browser_native_host, write_bridge_config, BrowserIntegrationEntry,
        BrowserIntegrationReport, DEFAULT_BRIDGE_PORT,
    },
    media::{
        dash::DashMpdParser,
        hls::MediaStreamEngine,
        ytdlp::{
            audio_track_rank, classify_youtube_error, fetch_playlist_flat,
            global_youtube_probe_cache, is_av1_vcodec, needs_login_or_cookies,
            normalize_sabr_session, remember_site_cookies, remembered_site_cookies, PlaylistInfo,
            SabrSessionPayload,
        },
    },
};
use anyhow::{anyhow, Context, Result};
use axum::{
    body::Body,
    extract::{DefaultBodyLimit, Path as AxumPath, Query, State},
    http::{HeaderMap as AxumHeaderMap, Method, Request, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
    Json, Router,
};
use chrono::{Datelike, Local, NaiveDateTime, NaiveTime, Utc};
use regex::Regex;
use reqwest::{
    header::{HeaderMap, HeaderValue, COOKIE, REFERER, USER_AGENT},
    Client,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    path::{Path, PathBuf},
    process::Command,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tokio::{
    fs::{self, File},
    io::AsyncWriteExt,
    process::Command as TokioCommand,
    sync::{Mutex, RwLock, Semaphore},
    time::{sleep, timeout},
};
use url::Url;
use uuid::Uuid;

/// اسم الترويسة الأمنية الإلزامية للاتصال بخادم التحكم المحلي لمنع هجمات Drive-by CSRF من صفحات الويب
pub const IPC_SECURITY_HEADER: &str = "x-rustidm";

/// فحص ترويسة `Host` لسدّ هجمات DNS Rebinding: الصفحة الخبيثة التي تعيد ربط نطاقها بـ `127.0.0.1`
/// تصبح "نفس المصدر" مع الخادم المحلي، فيرسل المتصفح طلبات GET بلا `Origin` ومعها أي ترويسة مخصصة،
/// وبذلك تتجاوز حارس `Origin` و`X-RustIDM`. لكن ترويسة `Host` تبقى نطاق المهاجم، فنرفضها.
/// المسموح حصراً: `127.0.0.1` و`localhost` و`[::1]` (مع أو بدون منفذ).
pub fn is_allowed_local_host(headers: &AxumHeaderMap) -> bool {
    let Some(raw) = headers.get("host").and_then(|v| v.to_str().ok()) else {
        return false;
    };
    let host = raw.trim().to_ascii_lowercase();
    let name = if let Some(rest) = host.strip_prefix('[') {
        // عنوان IPv6 بين أقواس: [::1] أو [::1]:16888
        match rest.split_once(']') {
            Some((addr, tail)) if tail.is_empty() || is_port_suffix(tail) => format!("[{addr}]"),
            _ => return false,
        }
    } else {
        match host.split_once(':') {
            Some((name, port)) => {
                if !is_port_digits(port) {
                    return false;
                }
                name.to_string()
            }
            None => host.clone(),
        }
    };
    matches!(name.as_str(), "127.0.0.1" | "localhost" | "[::1]")
}

fn is_port_digits(port: &str) -> bool {
    !port.is_empty() && port.len() <= 5 && port.bytes().all(|b| b.is_ascii_digit())
}

fn is_port_suffix(tail: &str) -> bool {
    tail.strip_prefix(':').map(is_port_digits).unwrap_or(false)
}

/// فحص ما إذا كان عنوان `Origin` يعود حصرياً لإضافة متصفح (Chrome / Edge / Brave / Firefox)
pub fn is_allowed_extension_origin(origin: &str) -> bool {
    let lower = origin.trim().to_ascii_lowercase();
    lower.starts_with("chrome-extension://")
        || lower.starts_with("moz-extension://")
        || lower.starts_with("extension://")
}

/// قائمة المسارات المسموح لإضافة المتصفح باستدعائها حصراً (عزل المسارات الإدارية مثل `/config` و`/queues` و`/tools/update`)
pub fn is_browser_extension_allowed_route(method: &Method, path: &str) -> bool {
    match (*method).clone() {
        Method::GET => matches!(path, "/api/v1/health" | "/api/v1/downloads"),
        Method::POST => matches!(
            path,
            "/api/v1/intercept"
                | "/api/v1/video/probe"
                | "/api/v1/video/download"
                | "/api/v1/video/playlist"
                | "/api/v1/torrent/inspect"
                | "/api/v1/torrent/add"
                | "/api/v1/batch"
                | "/api/v1/refresh-url"
                | "/api/v1/zip-preview"
        ),
        _ => false,
    }
}

/// التحقق الأمني الصارم من طلبات خادم الاتصال المحلي مع عزل صلاحيات المسارات الحساسة:
/// 1. رفض كافة طلبات `OPTIONS` (CORS Preflight) فوراً.
/// 2. رفض أي طلب يحمل ترويسة `Origin` لا تنتمي لإضافة المتصفح (`chrome-extension://` أو `moz-extension://`).
/// 3. إذا كان الطلب قادماً من إضافة متصفح (يحمل `Origin`)، يُحصر وصوله في مسارات الاعتراض والفيديو فقط، ويُمنع تماماً من تعديل `/config` أو `/queues` أو حذف الملفات.
/// 4. اشتراط وجود ترويسة `X-RustIDM` المخصصة في جميع الطلبات.
/// استخراج رمز الجلسة المشفر (`Bearer <token>`) من ترويسة `Authorization` أو `X-StormGet-Token`
pub fn extract_bearer_token(headers: &AxumHeaderMap) -> Option<&str> {
    if let Some(auth_str) = headers.get("authorization").and_then(|v| v.to_str().ok()) {
        if let Some(stripped) = auth_str.trim().strip_prefix("Bearer ") {
            let tok = stripped.trim();
            if !tok.is_empty() {
                return Some(tok);
            }
        }
    }
    headers
        .get("x-stormget-token")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

/// التحقق الأمني المزدوج مع مقارنة زمنية ثابتة (`constant_time_token_eq`) لرمز `bridge.json`:
/// - عندما يكون `expected_bridge_token` مفعّلاً (`Some(expected)` غير فارغ)، فإن ترويسة `X-RustIDM: 1` الثابتة
///   وحدها **لا تكفي أبداً** للوصول إلى مسارات سطح المكتب أو الإدارة (`/config`, `/queues`, `DELETE` إلخ)،
///   بل يُشترط تطابق الرمز السري العشوائي (`Bearer <bridge_token>`) في زمن ثابت.
/// - طلبات إضافة المتصفح (`Origin: chrome-extension://` أو `moz-extension://`) محصورة حصرياً في مسارات
///   الاعتراض والفحص المسموحة (`is_browser_extension_allowed_route`)، وإذا أرسلت رمز `Bearer` فيجب أن يتطابق.
pub fn is_authorized_ipc_request_with_token(
    method: &Method,
    path: &str,
    headers: &AxumHeaderMap,
    expected_bridge_token: Option<&str>,
) -> bool {
    if *method == Method::OPTIONS {
        return false;
    }

    let token_required = expected_bridge_token.map(|t| !t.is_empty()).unwrap_or(false);
    let has_valid_bearer = match (extract_bearer_token(headers), expected_bridge_token) {
        (Some(provided), Some(expected)) if !expected.is_empty() => {
            constant_time_token_eq(provided, expected)
        }
        _ => false,
    };

    if extract_bearer_token(headers).is_some() && token_required && !has_valid_bearer {
        return false;
    }

    let is_verified_browser_ext = headers
        .get("origin")
        .and_then(|v| v.to_str().ok())
        .map(|orig| is_allowed_extension_origin(orig) && is_browser_extension_allowed_route(method, path))
        .unwrap_or(false);

    if let Some(origin_val) = headers.get("origin").and_then(|v| v.to_str().ok()) {
        if !is_allowed_extension_origin(origin_val) || !is_browser_extension_allowed_route(method, path) {
            return false;
        }
    }

    if let Some(sec_site) = headers.get("sec-fetch-site").and_then(|v| v.to_str().ok()) {
        if sec_site.eq_ignore_ascii_case("cross-site") && !is_verified_browser_ext {
            return false;
        }
    }

    if token_required {
        // إصلاح أمني شامل: عندما يكون `expected_bridge_token` مفعلاً (في الخادم الفعلي دائماً)،
        // يُشترط تطابق الرمز السري المشفر (`has_valid_bearer` عبر `constant_time_token_eq`) لجميع الطلبات
        // بنسبة 100% دون استثناء، ولا تُقبل ترويسة `X-RustIDM: 1` الثابتة وحدها مطلقاً حتى لو زوّر المهاجم ترويسة Origin.
        if !has_valid_bearer {
            return false;
        }
    } else {
        let has_legacy_header = headers
            .get(IPC_SECURITY_HEADER)
            .and_then(|v| v.to_str().ok())
            .map(|v| !v.trim().is_empty())
            .unwrap_or(false);
        if !has_valid_bearer && !has_legacy_header {
            return false;
        }
    }

    true
}

pub fn is_authorized_ipc_request_for_path(
    method: &Method,
    path: &str,
    headers: &AxumHeaderMap,
) -> bool {
    if !is_authorized_ipc_request_with_token(method, path, headers, None) {
        return false;
    }

    if let Some(origin_val) = headers.get("origin").and_then(|v| v.to_str().ok()) {
        if !is_allowed_extension_origin(origin_val) {
            return false;
        }
        // عزل صلاحيات الإضافة: لا يُسمح لأي إضافة متصفح بالوصول إلى مسارات الإعدادات أو الأوامر الإدارية
        if !is_browser_extension_allowed_route(method, path) {
            return false;
        }
    }

    if let Some(sec_site) = headers.get("sec-fetch-site").and_then(|v| v.to_str().ok()) {
        if sec_site.eq_ignore_ascii_case("cross-site") {
            let has_ext_origin = headers
                .get("origin")
                .and_then(|v| v.to_str().ok())
                .map(is_allowed_extension_origin)
                .unwrap_or(false);
            if !has_ext_origin || !is_browser_extension_allowed_route(method, path) {
                return false;
            }
        }
    }

    true
}

/// دالة التحقق العامة المتوافقة مع الاختبارات للمسارات القياسية
pub fn is_authorized_ipc_request(method: &Method, headers: &AxumHeaderMap) -> bool {
    is_authorized_ipc_request_for_path(method, "/api/v1/intercept", headers)
}

/// وسيط Axum الأمني لحماية المنفذ المحلي وعزل المسارات الحساسة
async fn enforce_local_ipc_security(
    State(state): State<Arc<AppState>>,
    req: Request<Body>,
    next: Next,
) -> Response {
    let path = req.uri().path().to_string();
    if !is_allowed_local_host(req.headers())
        || !is_authorized_ipc_request_with_token(
            req.method(),
            &path,
            req.headers(),
            Some(state.bridge_token.as_str()),
        )
    {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({
                "error": "forbidden",
                "message": "رفض الوصول: غير مصرح لهذا المصدر باستدعاء هذا المسار المحلي"
            })),
        )
            .into_response();
    }
    next.run(req).await
}

// ============================================================================
// 10. مدير الطوابير والجدولة وإجراءات ما بعد الاكتمال واستعادة الجلسة
// ============================================================================

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PostQueueAction {
    None,
    Notify,
    ExitApplication,
    CustomCommand(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SiteLoginCredential {
    pub domain: String,
    pub username: String,
    #[serde(with = "crate::core::secret::string")]
    pub auth_header_or_cookie: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DuplicateFilePolicy {
    AutoRename,
    Overwrite,
    Prompt,
}

impl Default for DuplicateFilePolicy {
    fn default() -> Self {
        Self::AutoRename
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub download_dir: PathBuf,
    pub max_connections_per_file: usize,
    pub global_speed_limit_kbps: u64,
    pub min_split_size_kb: u64,
    pub proxy_url: Option<String>,
    pub custom_user_agent: String,
    pub auto_start_queued_downloads: bool,
    pub enable_checksum_verification: bool,
    pub antivirus_command: Option<String>,
    pub intercept_extensions: Vec<String>,
    pub site_logins: Vec<SiteLoginCredential>,
    #[serde(default)]
    pub temp_download_dir: Option<PathBuf>,
    #[serde(default)]
    pub duplicate_file_policy: DuplicateFilePolicy,
    #[serde(default = "default_connection_timeout_secs")]
    pub connection_timeout_secs: u64,
    #[serde(default = "default_max_retry_attempts")]
    pub max_retry_attempts: u32,
    #[serde(default = "default_true_flag")]
    pub confirm_destructive_actions: bool,
    #[serde(default = "default_true_flag")]
    pub minimize_to_tray_on_close: bool,
    #[serde(default)]
    pub start_with_system: bool,
    #[serde(default = "default_true_flag")]
    pub notify_on_complete: bool,
    #[serde(default = "default_true_flag")]
    pub notify_on_failure: bool,
    #[serde(default = "default_true_flag")]
    pub notify_on_schedule_start: bool,
    #[serde(default)]
    pub sound_notifications_enabled: bool,
    #[serde(default = "default_logging_level")]
    pub logging_level: String,
}

fn default_connection_timeout_secs() -> u64 {
    15
}
fn default_max_retry_attempts() -> u32 {
    5
}
fn default_true_flag() -> bool {
    true
}
fn default_logging_level() -> String {
    "INFO".to_string()
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            download_dir: crate::default_download_directory(),
            max_connections_per_file: 8,
            global_speed_limit_kbps: 0,
            min_split_size_kb: 512,
            proxy_url: None,
            custom_user_agent: "Mozilla/5.0 (Windows NT 10.0; Win64; x64) StormGet/2.3 (RustIDM)".to_string(),
            auto_start_queued_downloads: true,
            enable_checksum_verification: true,
            antivirus_command: None,
            intercept_extensions: vec![
                "zip", "rar", "7z", "tar", "gz", "bz2", "xz", "iso", "exe", "msi", "dmg", "deb",
                "rpm", "apk", "mp4", "mkv", "webm", "avi", "mov", "flv", "mp3", "flac", "wav",
                "m4a", "aac", "ogg", "pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "epub",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
            site_logins: Vec::new(),
            temp_download_dir: None,
            duplicate_file_policy: DuplicateFilePolicy::AutoRename,
            connection_timeout_secs: 15,
            max_retry_attempts: 5,
            confirm_destructive_actions: true,
            minimize_to_tray_on_close: true,
            start_with_system: false,
            notify_on_complete: true,
            notify_on_failure: true,
            notify_on_schedule_start: true,
            sound_notifications_enabled: false,
            logging_level: "INFO".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineStatistics {
    pub total_tasks: usize,
    pub active_downloads: usize,
    pub paused_downloads: usize,
    pub queued_downloads: usize,
    pub completed_downloads: usize,
    pub failed_downloads: usize,
    pub total_bytes_downloaded: u64,
    pub current_aggregate_speed_bps: u64,
    pub peak_aggregate_speed_bps: u64,
    pub speed_limit_kbps: u64,
    pub queues_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueConfig {
    pub name: String,
    pub max_concurrent_downloads: usize,
    pub start_time: Option<String>, // HH:MM:SS
    pub stop_time: Option<String>,  // HH:MM:SS
    pub post_action: PostQueueAction,
    #[serde(default)]
    pub schedule_mode: ScheduleMode,
    #[serde(default)]
    pub start_at: Option<String>,
    #[serde(default)]
    pub stop_at: Option<String>,
    #[serde(default)]
    pub days_of_week: Vec<u8>, // 0=Mon .. 6=Sun (empty = all days)
    #[serde(default)]
    pub speed_limit_kbps: u64,
}

impl QueueConfig {
    pub fn schedule(&self) -> QueueSchedule {
        QueueSchedule {
            mode: self.schedule_mode,
            start_time: self.start_time.clone(),
            stop_time: self.stop_time.clone(),
            start_at: self.start_at.clone(),
            stop_at: self.stop_at.clone(),
            days_of_week: self.days_of_week.clone(),
            speed_limit_kbps: self.speed_limit_kbps,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScheduleMode {
    #[default]
    Disabled,
    Daily,
    Once,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct QueueSchedule {
    pub mode: ScheduleMode,
    pub start_time: Option<String>,
    pub stop_time: Option<String>,
    pub start_at: Option<String>,
    #[serde(default)]
    pub stop_at: Option<String>,
    #[serde(default)]
    pub days_of_week: Vec<u8>,
    #[serde(default)]
    pub speed_limit_kbps: u64,
}

pub struct AppState {
    pub client: Client,
    pub torrent_engine: Arc<Aria2TorrentEngine>,
    pub lane_pool: Arc<MultiNetworkLanePool>,
    pub smart_throttle: Mutex<SmartThrottleStatus>,
    pub base_download_dir: PathBuf,
    pub bridge_token: String,
    pub max_connections_per_file: usize,
    pub speed_limiter: Arc<SpeedLimiter>,
    pub config: RwLock<AppConfig>,
    pub jobs: RwLock<HashMap<String, Arc<DynamicDownloadJob>>>,
    pub detected_videos: RwLock<Vec<DetectedVideoPayload>>,
    pub queues: RwLock<HashMap<String, QueueConfig>>,
    pub recent_intercepts: Mutex<HashMap<String, Instant>>,
    pub pending_intercepts: Mutex<VecDeque<PendingIntercept>>,
    pub queue_semaphores: RwLock<HashMap<String, Arc<Semaphore>>>,
    pub post_action_fired: Mutex<HashSet<String>>,
    pub scheduler_notifications: Mutex<VecDeque<String>>,
    pub peak_speed_bps: AtomicU64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingIntercept {
    pub id: String,
    pub payload: InterceptDownloadPayload,
}

#[derive(Debug, Deserialize)]
struct PendingInterceptDecision {
    action: String,
    save_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InterceptDownloadPayload {
    pub url: String,
    #[serde(default)]
    pub source_page_url: Option<String>,
    #[serde(default)]
    pub filename: Option<String>,
    #[serde(default)]
    pub cookies: Option<String>,
    #[serde(default)]
    pub referer: Option<String>,
    #[serde(default)]
    pub user_agent: Option<String>,
    #[serde(default)]
    pub queue_name: Option<String>,
    #[serde(default)]
    pub expected_sha256: Option<String>,
    #[serde(default)]
    pub expected_md5: Option<String>,
    #[serde(default)]
    pub expected_crc32: Option<String>,
    #[serde(default)]
    pub mirror_urls: Option<Vec<String>>,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefreshUrlPayload {
    pub task_id: String,
    pub new_url: String,
    pub cookies: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectedVideoPayload {
    pub url: String,
    pub page_title: Option<String>,
    pub mime_type: String,
    pub content_length: Option<u64>,
    pub quality_label: Option<String>,
    pub tab_id: Option<i64>,
    pub cookies: Option<String>,
    pub referer: Option<String>,
    pub user_agent: Option<String>,
    pub start_immediately: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControlTaskPayload {
    pub task_id: String,
    pub action: String, // "pause" | "resume" | "cancel" | "speed_limit"
    pub speed_limit_kbps: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProbeVideoPayload {
    pub url: String,
    pub page_url: Option<String>,
    pub page_title: Option<String>,
    pub cookies: Option<String>,
    pub referer: Option<String>,
    pub user_agent: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoQualityOption {
    pub id: String,
    pub label: String,
    pub resolution: Option<String>,
    pub container: String,
    pub bandwidth_bps: Option<u64>,
    pub estimated_size_bytes: Option<u64>,
    pub download_url: String,
    #[serde(default)]
    pub audio_url: Option<String>,
    pub protocol: String, // "hls" | "dash" | "ytdlp" | "sabr" | "direct" | "direct_mux"
    pub format_id: Option<String>,
    #[serde(default)]
    pub sabr_session: Option<SabrSessionPayload>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProbeVideoResponse {
    pub title: String,
    pub drm_protected: bool,
    pub drm_reason: Option<String>,
    pub qualities: Vec<VideoQualityOption>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartVideoDownloadPayload {
    pub url: String,
    #[serde(default)]
    pub audio_url: Option<String>,
    pub page_url: Option<String>,
    pub title: Option<String>,
    pub quality_label: Option<String>,
    pub container: Option<String>,
    pub protocol: String, // "hls" | "dash" | "ytdlp" | "sabr" | "direct" | "direct_mux"
    pub format_id: Option<String>,
    #[serde(default)]
    pub sabr_session: Option<SabrSessionPayload>,
    pub cookies: Option<String>,
    pub referer: Option<String>,
    pub user_agent: Option<String>,
}

impl AppState {
    pub fn new(
        base_download_dir: PathBuf,
        max_connections_per_file: usize,
        speed_limit_kbps: u64,
    ) -> Arc<Self> {
        let client = build_optimized_http_client(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) StormGet/2.3 (RustIDM)",
            15,
        );
        let torrent_engine = Arc::new(Aria2TorrentEngine::new(&base_download_dir.join("torrents_data")));

        let mut default_queues = HashMap::new();
        default_queues.insert(
            "main".to_string(),
            QueueConfig {
                name: "main".to_string(),
                max_concurrent_downloads: 4,
                start_time: None,
                stop_time: None,
                post_action: PostQueueAction::Notify,
                schedule_mode: ScheduleMode::Disabled,
                start_at: None,
                stop_at: None,
                days_of_week: Vec::new(),
                speed_limit_kbps: 0,
            },
        );
        default_queues.insert(
            "sync".to_string(),
            QueueConfig {
                name: "sync".to_string(),
                max_concurrent_downloads: 2,
                start_time: Some("01:00:00".to_string()),
                stop_time: Some("07:00:00".to_string()),
                post_action: PostQueueAction::None,
                schedule_mode: ScheduleMode::Disabled,
                start_at: None,
                stop_at: None,
                days_of_week: Vec::new(),
                speed_limit_kbps: 0,
            },
        );

        let queue_semaphores = default_queues
            .iter()
            .map(|(name, queue)| {
                (
                    name.clone(),
                    Arc::new(Semaphore::new(queue.max_concurrent_downloads.max(1))),
                )
            })
            .collect();

        let mut default_cfg = AppConfig::default();
        default_cfg.download_dir = base_download_dir.clone();
        default_cfg.max_connections_per_file = max_connections_per_file;
        default_cfg.global_speed_limit_kbps = speed_limit_kbps;

        let bridge_token = generate_bridge_token();
        let _ = write_bridge_config(&base_download_dir, DEFAULT_BRIDGE_PORT, &bridge_token);

        let lane_pool = MultiNetworkLanePool::new();
        let smart_throttle = Mutex::new(SmartThrottleStatus::default());

        Arc::new(Self {
            client,
            torrent_engine,
            lane_pool,
            smart_throttle,
            base_download_dir,
            bridge_token,
            max_connections_per_file,
            speed_limiter: SpeedLimiter::new(speed_limit_kbps),
            config: RwLock::new(default_cfg),
            jobs: RwLock::new(HashMap::new()),
            detected_videos: RwLock::new(Vec::new()),
            queues: RwLock::new(default_queues),
            recent_intercepts: Mutex::new(HashMap::new()),
            pending_intercepts: Mutex::new(VecDeque::new()),
            queue_semaphores: RwLock::new(queue_semaphores),
            post_action_fired: Mutex::new(HashSet::new()),
            scheduler_notifications: Mutex::new(VecDeque::new()),
            peak_speed_bps: AtomicU64::new(0),
        })
    }

    pub async fn spawn_download_job(self: &Arc<Self>, job: Arc<DynamicDownloadJob>) {
        if job
            .runner_active
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return;
        }

        let queue_name = job.meta.lock().await.queue_name.clone();
        let semaphore = {
            let mut sems = self.queue_semaphores.write().await;
            if let Some(s) = sems.get(&queue_name) {
                Arc::clone(s)
            } else {
                let s = Arc::new(Semaphore::new(4));
                sems.insert(queue_name.clone(), Arc::clone(&s));
                s
            }
        };

        let state_clone = Arc::clone(self);
        tokio::spawn(async move {
            let permit = match semaphore.acquire_owned().await {
                Ok(permit) => permit,
                Err(error) => {
                    eprintln!("Cannot acquire download queue slot: {error}");
                    job.runner_active.store(false, Ordering::SeqCst);
                    return;
                }
            };
            if !job.cancel_flag.load(Ordering::SeqCst)
                && !job.pause_flag.load(Ordering::SeqCst)
                && !job.uses_external_worker
            {
                set_system_awake_for_downloads(true);
                if let Err(error) = job.execute().await {
                    let status = job.meta.lock().await.status;
                    if status != DownloadStatus::Paused && status != DownloadStatus::Failed {
                        eprintln!("Download job failed: {error:#}");
                    }
                }
            }
            drop(permit);
            job.runner_active.store(false, Ordering::SeqCst);

            let any_active = state_clone.has_any_active_downloads().await;
            set_system_awake_for_downloads(any_active);
        });
    }

    pub async fn has_any_active_downloads(&self) -> bool {
        let jobs = self.jobs.read().await;
        for job in jobs.values() {
            if job.runner_active.load(Ordering::Relaxed) {
                return true;
            }
            let st = job.meta.lock().await.status;
            if matches!(
                st,
                DownloadStatus::Downloading
                    | DownloadStatus::RefreshingUrl
                    | DownloadStatus::Assembling
                    | DownloadStatus::VerifyingChecksum
            ) {
                return true;
            }
        }
        false
    }

    /// استعادة جميع التنزيلات المتوقفة أو غير المكتملة (.idm.json) من القرص مع التحقق من وجود وحجم ملفات `.part` والملفات المكتملة
    pub async fn restore_persisted_tasks_on_startup(self: &Arc<Self>) -> Result<usize> {
        let config_path = self.base_download_dir.join("config.json");
        if let Ok(bytes) = fs::read(&config_path).await {
            if let Ok(saved_cfg) = serde_json::from_slice::<AppConfig>(&bytes) {
                self.speed_limiter
                    .set_limit_kbps(saved_cfg.global_speed_limit_kbps);
                *self.config.write().await = saved_cfg;
            } else {
                eprintln!(
                    "Warning: Failed to parse config.json at {:?}; using defaults",
                    config_path
                );
            }
        }

        let queues_path = self.base_download_dir.join("queues.json");
        if let Ok(bytes) = fs::read(&queues_path).await {
            if let Ok(saved_queues) = serde_json::from_slice::<HashMap<String, QueueConfig>>(&bytes)
            {
                let mut q_guard = self.queues.write().await;
                let mut sem_guard = self.queue_semaphores.write().await;
                for (name, q_cfg) in saved_queues {
                    sem_guard.entry(name.clone()).or_insert_with(|| {
                        Arc::new(Semaphore::new(q_cfg.max_concurrent_downloads.max(1)))
                    });
                    q_guard.insert(name, q_cfg);
                }
            }
        }

        let schedule_path = self.base_download_dir.join("scheduler.json");
        match fs::read(&schedule_path).await {
            Ok(bytes) => {
                if let Ok(saved) = serde_json::from_slice::<HashMap<String, QueueSchedule>>(&bytes)
                {
                    let mut queues = self.queues.write().await;
                    for (name, schedule) in saved {
                        if let Some(queue) = queues.get_mut(&name) {
                            queue.schedule_mode = schedule.mode;
                            queue.start_time = schedule.start_time;
                            queue.stop_time = schedule.stop_time;
                            queue.start_at = schedule.start_at;
                            queue.stop_at = schedule.stop_at;
                            if !schedule.days_of_week.is_empty() {
                                queue.days_of_week = schedule.days_of_week;
                            }
                            if schedule.speed_limit_kbps > 0 {
                                queue.speed_limit_kbps = schedule.speed_limit_kbps;
                            }
                        }
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => eprintln!("Warning: Failed to read scheduler.json: {error}"),
        }

        let mut restored = 0usize;
        let mut search_roots = Vec::new();
        search_roots.push(self.base_download_dir.clone());
        let configured_dir = self.config.read().await.download_dir.clone();
        if !search_roots.contains(&configured_dir) {
            search_roots.push(configured_dir);
        }
        let default_dir = crate::default_download_directory();
        if !search_roots.contains(&default_dir) {
            search_roots.push(default_dir);
        }
        let fallback_downloads = PathBuf::from("./Downloads");
        if !search_roots.contains(&fallback_downloads) {
            search_roots.push(fallback_downloads);
        }

        let mut dirs_to_scan = Vec::new();
        for root in search_roots {
            if !root.exists() {
                continue;
            }
            if !dirs_to_scan.contains(&root) {
                dirs_to_scan.push(root.clone());
            }
            if let Ok(mut entries) = fs::read_dir(&root).await {
                while let Ok(Some(entry)) = entries.next_entry().await {
                    let path = entry.path();
                    if path.is_dir() && !dirs_to_scan.contains(&path) {
                        dirs_to_scan.push(path);
                    }
                }
            }
        }

        for dir in dirs_to_scan {
            let Ok(mut entries) = fs::read_dir(&dir).await else {
                continue;
            };
            while let Ok(Some(entry)) = entries.next_entry().await {
                let path = entry.path();
                if path.to_string_lossy().ends_with(".idm.json") {
                    match fs::read(&path).await {
                        Ok(bytes) => match serde_json::from_slice::<DownloadStateMeta>(&bytes) {
                            Ok(mut meta) => {
                                if matches!(
                                    meta.status,
                                    DownloadStatus::Downloading
                                        | DownloadStatus::Assembling
                                        | DownloadStatus::VerifyingChecksum
                                ) {
                                    meta.status = DownloadStatus::Paused;
                                }

                                // فحص توافق الحالة المحفوظة مع الملفات الفعلية على القرص
                                if meta.status == DownloadStatus::Completed {
                                    if !meta.final_file_path().exists() {
                                        meta.status = DownloadStatus::Paused;
                                        meta.segments.clear();
                                        meta.last_error = Some(
                                            "الملف المكتمل لم يعد موجوداً على القرص؛ يمكن إعادة تحميله"
                                                .to_string(),
                                        );
                                    }
                                } else {
                                    let part_len = std::fs::metadata(meta.part_file_path())
                                        .map(|m| m.len())
                                        .unwrap_or(0);
                                    validate_part_file_against_segments(&mut meta, part_len);
                                }

                                let task_id = meta.task_id.clone();
                                let mut jobs = self.jobs.write().await;
                                if !jobs.contains_key(&task_id) {
                                    let job = Arc::new(DynamicDownloadJob {
                                        meta: Arc::new(Mutex::new(meta)),
                                        pause_flag: Arc::new(AtomicBool::new(true)),
                                        cancel_flag: Arc::new(AtomicBool::new(false)),
                                        resource_mutated_flag: Arc::new(AtomicBool::new(false)),
                                        expired_url_flag: Arc::new(AtomicBool::new(false)),
                                        speed_limiter: Arc::clone(&self.speed_limiter),
                                        client: self.client.clone(),
                                        max_connections: self.max_connections_per_file,
                                        min_split_size: MIN_SPLIT_SIZE,
                                        antivirus_cmd: None,
                                        uses_external_worker: false,
                                        runner_active: AtomicBool::new(false),
                                        schedule_paused: AtomicBool::new(false),
                                    });
                                    jobs.insert(task_id, job);
                                    restored += 1;
                                }
                            }
                            Err(parse_err) => {
                                eprintln!(
                                    "Warning: Corrupt task metadata file {:?} ({parse_err})",
                                    path
                                );
                            }
                        },
                        Err(read_err) => {
                            eprintln!("Warning: Could not read metadata file {:?}: {read_err}", path);
                        }
                    }
                }
            }
        }
        Ok(restored)
    }

    pub fn start_scheduler(self: &Arc<Self>) {
        spawn_background_tools_updater(self.client.clone(), self.base_download_dir.clone());

        // الاكتشاف والربط التلقائي لإضافة المتصفح في Chrome / Edge / Brave / Firefox عند الإقلاع وكل 30 ثانية
        let auto_browser_dir = self.base_download_dir.clone();
        tokio::spawn(async move {
            let exe_path = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("rust_idm"));
            loop {
                let dir_clone = auto_browser_dir.clone();
                let exe_clone = exe_path.clone();
                let _ = tokio::task::spawn_blocking(move || {
                    connect_installed_browsers(&dir_clone, &exe_clone, None)
                })
                .await;
                sleep(Duration::from_secs(30)).await;
            }
        });

        let state = Arc::clone(self);
        tokio::spawn(async move {
            let mut ticks = 0u64;
            loop {
                ticks += 1;
                if ticks % 15 == 0 {
                    cleanup_finished_previews();
                }
                if let Err(error) = apply_scheduled_queues(&state).await {
                    eprintln!("Scheduler tick failed: {error:#}");
                }
                sleep(Duration::from_secs(1)).await;
            }
        });
    }

    pub async fn can_execute_post_queue_action(&self, queue_name: &str) -> bool {
        let jobs = self.jobs.read().await;
        let mut has_tasks = false;

        for job in jobs.values() {
            let m = job.meta.lock().await;
            if m.queue_name == queue_name {
                has_tasks = true;
                if m.status != DownloadStatus::Completed {
                    return false;
                }
            }
        }
        has_tasks
    }

    pub async fn enqueue_download(
        self: &Arc<Self>,
        payload: InterceptDownloadPayload,
    ) -> Result<String> {
        self.enqueue_download_with_options(payload, None, true).await
    }

    pub async fn enqueue_download_with_options(
        self: &Arc<Self>,
        payload: InterceptDownloadPayload,
        save_dir_override: Option<PathBuf>,
        start_immediately: bool,
    ) -> Result<String> {
        {
            let mut guard = self.recent_intercepts.lock().await;
            guard.retain(|_, ts| ts.elapsed() < Duration::from_secs(5));
            if guard.contains_key(&payload.url) {
                return Err(anyhow!(
                    "تم استلام هذا الرابط بالفعل خلال الثواني الخمس الماضية"
                ));
            }
            guard.insert(payload.url.clone(), Instant::now());
        }

        let mut header_map = HeaderMap::new();
        let mut custom_headers = HashMap::new();

        if let Some(ref c) = payload.cookies {
            if !c.is_empty() {
                header_map.insert(COOKIE, HeaderValue::from_str(c)?);
                custom_headers.insert("cookie".to_string(), c.clone());
            }
        }
        if let Some(ref r) = payload.referer {
            if !r.is_empty() {
                header_map.insert(REFERER, HeaderValue::from_str(r)?);
                custom_headers.insert("referer".to_string(), r.clone());
            }
        }
        if let Some(ref ua) = payload.user_agent {
            if !ua.is_empty() {
                header_map.insert(USER_AGENT, HeaderValue::from_str(ua)?);
                custom_headers.insert("user-agent".to_string(), ua.clone());
            }
        }

        let parsed_url = Url::parse(&payload.url).context("صيغة الرابط غير صالحة")?;
        if parsed_url.scheme() != "http" && parsed_url.scheme() != "https" {
            return Err(anyhow!("البروتوكول غير مدعوم ({})؛ يُسمح فقط بـ HTTP/HTTPS", parsed_url.scheme()));
        }
        let (total_size, resumable, probed_name, etag, last_modified) =
            match DynamicDownloadJob::probe_url(&self.client, &payload.url, &header_map).await {
                Ok(res) => res,
                Err(probe_err) => {
                    eprintln!("Warning: Initial probe failed for {} ({}); falling back to direct enqueue", payload.url, probe_err);
                    let fallback_name = crate::core::storage::extract_filename(&payload.url, &HeaderMap::new());
                    (0, false, fallback_name, None, None)
                }
            };

        let raw_filename = payload
            .filename
            .filter(|n| !n.trim().is_empty())
            .map(|n| sanitize_filename(&n))
            .unwrap_or(probed_name);

        let category = FileCategory::from_filename(&raw_filename);
        let save_dir = save_dir_override
            .unwrap_or_else(|| self.base_download_dir.join(category.folder_name()));
        let overwrite_policy = self.config.read().await.duplicate_file_policy == DuplicateFilePolicy::Overwrite;
        let final_filename = crate::core::filename::resolve_unique_filename(&save_dir, &raw_filename, overwrite_policy);
        let queue_name = payload.queue_name.unwrap_or_else(|| "main".to_string());
        let queue = self
            .queues
            .read()
            .await
            .get(&queue_name)
            .map(QueueConfig::schedule)
            .ok_or_else(|| anyhow!("Unknown download queue: {queue_name}"))?;
        let schedule = queue;
        let schedule_allows_start =
            scheduled_queue_should_run(&schedule, Local::now().naive_local())
                .context("Invalid schedule configured for download queue")?
                .unwrap_or(true);
        let should_start = start_immediately && schedule_allows_start;

        let task_id = Uuid::new_v4().to_string();
        let meta = DownloadStateMeta {
            task_id: task_id.clone(),
            url: payload.url.clone(),
            source_page_url: payload.source_page_url.or(payload.referer),
            filename: final_filename,
            save_dir,
            category,
            total_size,
            resumable,
            etag,
            last_modified,
            custom_headers,
            expected_sha256: payload.expected_sha256,
            computed_sha256: None,
            status: if !start_immediately {
                DownloadStatus::Queued
            } else if should_start {
                DownloadStatus::Queued
            } else {
                DownloadStatus::Paused
            },
            last_error: None,
            refresh_attempts: 0,
            segments: Vec::new(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            queue_name: queue_name.clone(),
            mirror_urls: payload.mirror_urls.unwrap_or_default(),
            active_mirror_index: 0,
            expected_md5: payload.expected_md5,
            computed_md5: None,
            expected_crc32: payload.expected_crc32,
            computed_crc32: None,
            description: payload.description,
            speed_history: Vec::new(),
            connection_logs: vec![ConnectionLogEntry {
                timestamp: Utc::now(),
                segment_id: None,
                level: "INFO".to_string(),
                message: format!(
                    "تمت جدولة المهمة في الطابور '{}' (الحجم: {} بايت)",
                    queue_name, total_size
                ),
            }],
            task_speed_limit_kbps: 0,
        };
        fs::create_dir_all(&meta.save_dir).await?;
        save_meta_to_disk_atomic(&meta, &meta.meta_file_path()).await?;

        let pause_flag = Arc::new(AtomicBool::new(!should_start));
        let job = Arc::new(DynamicDownloadJob {
            meta: Arc::new(Mutex::new(meta)),
            pause_flag: Arc::clone(&pause_flag),
            cancel_flag: Arc::new(AtomicBool::new(false)),
            resource_mutated_flag: Arc::new(AtomicBool::new(false)),
            expired_url_flag: Arc::new(AtomicBool::new(false)),
            speed_limiter: Arc::clone(&self.speed_limiter),
            client: self.client.clone(),
            max_connections: self.max_connections_per_file,
            min_split_size: MIN_SPLIT_SIZE,
            antivirus_cmd: None,
            uses_external_worker: false,
            runner_active: AtomicBool::new(false),
            schedule_paused: AtomicBool::new(!should_start),
        });

        self.jobs
            .write()
            .await
            .insert(task_id.clone(), Arc::clone(&job));
        self.post_action_fired.lock().await.remove(&queue_name);

        if should_start {
            self.spawn_download_job(Arc::clone(&job)).await;
        }

        Ok(task_id)
    }
}

pub fn parse_schedule_time(value: &str) -> Result<NaiveTime> {
    NaiveTime::parse_from_str(value, "%H:%M")
        .or_else(|_| NaiveTime::parse_from_str(value, "%H:%M:%S"))
        .map_err(|error| anyhow!("Invalid schedule time '{value}': {error}"))
}

pub fn scheduled_queue_should_run(
    schedule: &QueueSchedule,
    now: NaiveDateTime,
) -> Result<Option<bool>> {
    match schedule.mode {
        ScheduleMode::Disabled => Ok(None),
        ScheduleMode::Once => {
            let Some(start_at) = schedule.start_at.as_deref() else {
                return Ok(None);
            };
            let start_dt = NaiveDateTime::parse_from_str(start_at, "%Y-%m-%d %H:%M")
                .or_else(|_| NaiveDateTime::parse_from_str(start_at, "%Y-%m-%d %H:%M:%S"))
                .map_err(|error| anyhow!("Invalid one-time schedule '{start_at}': {error}"))?;
            if let Some(stop_at) = schedule.stop_at.as_deref().filter(|s| !s.trim().is_empty()) {
                let stop_dt = NaiveDateTime::parse_from_str(stop_at, "%Y-%m-%d %H:%M")
                    .or_else(|_| NaiveDateTime::parse_from_str(stop_at, "%Y-%m-%d %H:%M:%S"))
                    .map_err(|error| anyhow!("Invalid one-time stop schedule '{stop_at}': {error}"))?;
                return Ok(Some(now >= start_dt && now < stop_dt));
            }
            Ok(Some(now >= start_dt))
        }
        ScheduleMode::Daily => {
            let (Some(start), Some(stop)) = (
                schedule.start_time.as_deref(),
                schedule.stop_time.as_deref(),
            ) else {
                return Ok(None);
            };
            let start = parse_schedule_time(start)?;
            let stop = parse_schedule_time(stop)?;
            let current = now.time();
            let today_weekday = now.weekday().num_days_from_monday() as u8; // 0=Mon..6=Sun
            let prev_weekday = (today_weekday + 6) % 7;

            let active = if start < stop {
                let day_ok = schedule.days_of_week.is_empty()
                    || schedule.days_of_week.contains(&today_weekday);
                day_ok && current >= start && current < stop
            } else {
                // الجدولة تعبر منتصف الليل (مثال: 22:00 إلى 06:00)
                if current >= start {
                    schedule.days_of_week.is_empty()
                        || schedule.days_of_week.contains(&today_weekday)
                } else if current < stop {
                    schedule.days_of_week.is_empty()
                        || schedule.days_of_week.contains(&prev_weekday)
                } else {
                    false
                }
            };
            Ok(Some(active))
        }
    }
}

pub async fn persist_queue_schedules(state: &AppState) -> Result<()> {
    let schedules = state
        .queues
        .read()
        .await
        .iter()
        .map(|(name, queue)| (name.clone(), queue.schedule()))
        .collect::<HashMap<_, _>>();
    let path = state.base_download_dir.join("scheduler.json");
    fs::create_dir_all(&state.base_download_dir).await?;
    let tmp_path = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(&schedules)?;
    let mut file = File::create(&tmp_path).await?;
    file.write_all(&bytes).await?;
    file.sync_all().await?;
    drop(file);
    fs::rename(&tmp_path, &path).await?;
    sync_parent_dir(&path).await;
    Ok(())
}

pub async fn apply_scheduled_queues(state: &Arc<AppState>) -> Result<()> {
    let now = Local::now().naive_local();
    let mut desired_states = HashMap::new();
    let mut schedule_changed = false;
    {
        let mut queues = state.queues.write().await;
        for (name, queue) in queues.iter_mut() {
            let schedule = queue.schedule();
            match scheduled_queue_should_run(&schedule, now) {
                Ok(Some(should_run)) => {
                    if schedule.mode == ScheduleMode::Once && should_run {
                        queue.schedule_mode = ScheduleMode::Disabled;
                        queue.start_at = None;
                        schedule_changed = true;
                    }
                    desired_states.insert(name.clone(), should_run);
                }
                Ok(None) => {}
                Err(error) => eprintln!("Invalid schedule for queue '{name}': {error:#}"),
            }
        }
    }

    if schedule_changed {
        persist_queue_schedules(state).await?;
    }

    let jobs = state
        .jobs
        .read()
        .await
        .values()
        .cloned()
        .collect::<Vec<_>>();
    for job in jobs {
        let queue_name = job.meta.lock().await.queue_name.clone();
        let Some(&should_run) = desired_states.get(&queue_name) else {
            continue;
        };

        if should_run {
            let status = job.meta.lock().await.status;
            if status == DownloadStatus::Queued {
                job.pause_flag.store(false, Ordering::SeqCst);
                if !job.uses_external_worker {
                    state.spawn_download_job(Arc::clone(&job)).await;
                }
            } else if status == DownloadStatus::Paused && job.schedule_paused.load(Ordering::SeqCst)
            {
                job.pause_flag.store(false, Ordering::SeqCst);
                if job.uses_external_worker || !job.runner_active.load(Ordering::SeqCst) {
                    job.schedule_paused.store(false, Ordering::SeqCst);
                }
                if !job.uses_external_worker {
                    state.spawn_download_job(Arc::clone(&job)).await;
                }
            }
        } else {
            let mut meta = job.meta.lock().await;
            if matches!(
                meta.status,
                DownloadStatus::Downloading | DownloadStatus::Queued
            ) {
                job.schedule_paused.store(true, Ordering::SeqCst);
                job.pause_flag.store(true, Ordering::SeqCst);
                meta.status = DownloadStatus::Paused;
                meta.updated_at = Utc::now();
                save_meta_to_disk_atomic(&meta, &meta.meta_file_path()).await?;
            }
        }
    }

    let any_active = state.has_any_active_downloads().await;
    set_system_awake_for_downloads(any_active);

    let queue_actions = {
        let queues = state.queues.read().await;
        queues
            .iter()
            .map(|(name, queue)| (name.clone(), queue.post_action.clone()))
            .collect::<Vec<_>>()
    };
    for (queue_name, action) in queue_actions {
        if action == PostQueueAction::None
            || !state.can_execute_post_queue_action(&queue_name).await
        {
            continue;
        }
        let mut fired = state.post_action_fired.lock().await;
        if fired.insert(queue_name.clone()) {
            match action {
                PostQueueAction::None => {}
                PostQueueAction::Notify => {
                    state
                        .scheduler_notifications
                        .lock()
                        .await
                        .push_back(format!(
                            "All downloads in queue '{queue_name}' have completed."
                        ));
                }
                PostQueueAction::ExitApplication => {
                    state
                        .scheduler_notifications
                        .lock()
                        .await
                        .push_back(format!(
                            "Queue '{queue_name}' completed (Post-Action: ExitApplication ready)."
                        ));
                }
                PostQueueAction::CustomCommand(cmd_str) => {
                    // إصلاح أمني محصّن:
                    // 1. عدم تقسيم الأمر على المسافات (split_whitespace) لمنع حقن المعاملات (Argument Injection) ولدعم المسارات ذات المسافات.
                    // 2. عزل مقابض الإدخال والإخراج (Stdio::null) وإخفاء نافذة الطرفية لمنع تسرب واصفات ملفات المحرك.
                    let trimmed_exe = cmd_str.trim().trim_matches('"');
                    if !trimmed_exe.is_empty() && validate_safe_executable_path(trimmed_exe).is_ok() {
                        let mut cmd = Command::new(trimmed_exe);
                        cmd.stdin(std::process::Stdio::null())
                            .stdout(std::process::Stdio::null())
                            .stderr(std::process::Stdio::null());
                        crate::core::tools::hide_console_window(&mut cmd);
                        let _ = cmd.spawn();
                    }
                    state
                        .scheduler_notifications
                        .lock()
                        .await
                        .push_back(format!(
                            "Queue '{queue_name}' completed and executed post-action command."
                        ));
                }
            }
        }
    }
    Ok(())
}

// ============================================================================
// 11. نقاط نهاية خادم الاتصال المحلي (Hardened Axum IPC Endpoints)
// ============================================================================

async fn health_handler(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "online",
        "engine": "StormGet",
        "version": env!("CARGO_PKG_VERSION"),
        "speed_limit_kbps": state.speed_limiter.get_limit_kbps(),
        "capabilities": [
            "atomic_segment_progress_telemetry",
            "strict_rfc9110_206_content_range_validation",
            "truncated_and_empty_stream_guard",
            "conservative_url_refresh_identity_verification",
            "part_file_on_disk_recovery_audit",
            "extension_route_privilege_isolation",
            "staged_ffmpeg_mux_stream_verification",
            "ntfs_sparse_and_positional_write_at",
            "youtube_10mb_range_window_and_yt_core_direct_mux",
            "auto_ytdlp_and_ffmpeg_updater"
        ]
    }))
}

async fn tools_status_handler(State(state): State<Arc<AppState>>) -> Json<ToolsStatusReport> {
    Json(get_tools_status(&state.base_download_dir))
}

async fn tools_update_handler(State(state): State<Arc<AppState>>) -> Json<ToolsStatusReport> {
    let report =
        update_all_tools(state.client.clone(), state.base_download_dir.clone(), true).await;
    Json(report)
}

async fn intercept_download_handler(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<InterceptDownloadPayload>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let mut pending = state.pending_intercepts.lock().await;
    if pending.iter().any(|item| item.payload.url == payload.url) {
        return Err((
            StatusCode::CONFLICT,
            "This download is already waiting for review".to_owned(),
        ));
    }
    let id = Uuid::new_v4().to_string();
    pending.push_back(PendingIntercept {
        id: id.clone(),
        payload,
    });

    Ok(Json(serde_json::json!({
        "status": "pending_review",
        "id": id
    })))
}

async fn pending_intercepts_handler(
    State(state): State<Arc<AppState>>,
) -> Json<Vec<PendingIntercept>> {
    Json(state.pending_intercepts.lock().await.iter().cloned().collect())
}

async fn decide_pending_intercept_handler(
    State(state): State<Arc<AppState>>,
    AxumPath(id): AxumPath<String>,
    Json(decision): Json<PendingInterceptDecision>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let pending_item = {
        let pending = state.pending_intercepts.lock().await;
        pending.iter().find(|item| item.id == id).cloned()
    }
    .ok_or_else(|| (StatusCode::NOT_FOUND, "Pending download not found".to_owned()))?;

    match decision.action.as_str() {
        "cancel" => {
            state
                .pending_intercepts
                .lock()
                .await
                .retain(|item| item.id != id);
            Ok(Json(serde_json::json!({ "status": "cancelled" })))
        }
        "start" | "later" => {
            let start_immediately = decision.action == "start";
            let task_id = state
                .enqueue_download_with_options(
                    pending_item.payload,
                    decision.save_dir,
                    start_immediately,
                )
                .await
                .map_err(|error| (StatusCode::BAD_REQUEST, error.to_string()))?;
            state
                .pending_intercepts
                .lock()
                .await
                .retain(|item| item.id != id);
            Ok(Json(serde_json::json!({
                "status": if start_immediately { "started" } else { "queued" },
                "task_id": task_id
            })))
        }
        _ => Err((
            StatusCode::BAD_REQUEST,
            "Unknown pending download action".to_owned(),
        )),
    }
}

async fn enqueue_download_handler(
    State(state): State<Arc<AppState>>,
    Json(request): Json<EnqueueDownloadRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let task_id = state
        .enqueue_download_with_options(
            request.payload,
            request.save_dir,
            request.start_immediately,
        )
        .await
        .map_err(|error| (StatusCode::BAD_REQUEST, error.to_string()))?;

    Ok(Json(serde_json::json!({
        "status": "accepted",
        "task_id": task_id
    })))
}

#[derive(Debug, Deserialize)]
struct EnqueueDownloadRequest {
    payload: InterceptDownloadPayload,
    save_dir: Option<PathBuf>,
    start_immediately: bool,
}

async fn refresh_url_handler(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<RefreshUrlPayload>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let job = {
        let jobs = state.jobs.read().await;
        jobs.get(&payload.task_id)
            .cloned()
            .ok_or_else(|| (StatusCode::NOT_FOUND, "المهمة غير موجودة".to_string()))?
    };

    if let Some(ref c) = payload.cookies {
        let mut m = job.meta.lock().await;
        m.custom_headers.insert("cookie".to_string(), c.clone());
    }

    let kept_partial = job
        .verify_and_apply_refreshed_url(&payload.new_url)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;

    job.schedule_paused.store(false, Ordering::SeqCst);
    job.pause_flag.store(false, Ordering::SeqCst);
    state.spawn_download_job(Arc::clone(&job)).await;

    Ok(Json(serde_json::json!({
        "status": "refreshed_and_resumed",
        "preserved_partial_bytes": kept_partial
    })))
}

async fn list_downloads_handler(
    State(state): State<Arc<AppState>>,
) -> Json<Vec<DownloadStateMeta>> {
    let jobs = state.jobs.read().await;
    let mut list = Vec::with_capacity(jobs.len());
    for job in jobs.values() {
        let m = job.meta.lock().await;
        list.push(m.redacted_clone());
    }
    Json(list)
}

async fn delete_completed_downloads_handler(
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let completed = {
        let jobs = state.jobs.read().await;
        let mut completed = Vec::new();
        for (task_id, job) in jobs.iter() {
            let meta = job.meta.lock().await;
            if meta.status == DownloadStatus::Completed {
                completed.push((task_id.clone(), meta.meta_file_path()));
            }
        }
        completed
    };

    let mut deleted = 0usize;
    for (task_id, meta_path) in completed {
        match fs::remove_file(&meta_path).await {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err((
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!(
                        "Cannot remove completed task metadata '{}': {error}",
                        meta_path.display()
                    ),
                ));
            }
        }
        let mut jobs = state.jobs.write().await;
        if let Some(job) = jobs.get(&task_id) {
            let meta = job.meta.lock().await;
            let is_completed = meta.status == DownloadStatus::Completed;
            drop(meta);
            if is_completed {
                jobs.remove(&task_id);
                deleted += 1;
            }
        }
    }

    Ok(Json(serde_json::json!({
        "status": "deleted",
        "count": deleted,
        "files_preserved": true
    })))
}

#[derive(Debug, Default, Deserialize)]
struct DeleteTaskQuery {
    #[serde(default)]
    delete_file: bool,
}

async fn delete_download_handler(
    State(state): State<Arc<AppState>>,
    AxumPath(task_id): AxumPath<String>,
    Query(query): Query<DeleteTaskQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let mut jobs = state.jobs.write().await;
    let job = jobs
        .get(&task_id)
        .cloned()
        .ok_or_else(|| (StatusCode::NOT_FOUND, "Download task not found".to_string()))?;
    let meta = job.meta.lock().await;
    if job.runner_active.load(Ordering::SeqCst)
        || !matches!(
            meta.status,
            DownloadStatus::Completed
                | DownloadStatus::Paused
                | DownloadStatus::Failed
                | DownloadStatus::Queued
                | DownloadStatus::NeedsUrlRefresh
        )
    {
        return Err((
            StatusCode::CONFLICT,
            "Pause the download before removing its task record".to_string(),
        ));
    }

    let meta_path = meta.meta_file_path();
    let part_path = meta.part_file_path();
    let final_path = meta.final_file_path();
    drop(meta);

    let _ = fs::remove_file(&meta_path).await;
    let _ = fs::remove_file(&part_path).await;
    if query.delete_file {
        let _ = fs::remove_file(&final_path).await;
    }
    jobs.remove(&task_id);

    Ok(Json(serde_json::json!({
        "status": "deleted",
        "task_id": task_id,
        "file_preserved": !query.delete_file
    })))
}

async fn redownload_task_handler(
    State(state): State<Arc<AppState>>,
    AxumPath(task_id): AxumPath<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let job = state
        .jobs
        .read()
        .await
        .get(&task_id)
        .cloned()
        .ok_or_else(|| (StatusCode::NOT_FOUND, "Download task not found".to_string()))?;

    {
        job.pause_flag.store(true, Ordering::SeqCst);
        let mut m = job.meta.lock().await;
        let part_path = m.part_file_path();
        let final_path = m.final_file_path();
        let _ = fs::remove_file(&part_path).await;
        let _ = fs::remove_file(&final_path).await;
        m.segments.clear();
        m.status = DownloadStatus::Queued;
        m.last_error = None;
        m.updated_at = Utc::now();
        let _ = save_meta_to_disk_atomic(&m, &m.meta_file_path()).await;
    }

    job.cancel_flag.store(false, Ordering::SeqCst);
    job.pause_flag.store(false, Ordering::SeqCst);
    job.schedule_paused.store(false, Ordering::SeqCst);
    state.spawn_download_job(job).await;

    Ok(Json(serde_json::json!({
        "status": "restarted",
        "task_id": task_id
    })))
}

async fn scheduler_notifications_handler(State(state): State<Arc<AppState>>) -> Json<Vec<String>> {
    let mut notifications = state.scheduler_notifications.lock().await;
    Json(notifications.drain(..).collect())
}

async fn control_task_handler(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<ControlTaskPayload>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    if payload.action == "speed_limit" {
        let kbps = payload.speed_limit_kbps.unwrap_or(0);
        state.speed_limiter.set_limit_kbps(kbps);
        return Ok(Json(serde_json::json!({
            "status": "speed_limit_updated",
            "speed_limit_kbps": kbps
        })));
    }

    let job = {
        let jobs = state.jobs.read().await;
        jobs.get(&payload.task_id)
            .cloned()
            .ok_or_else(|| (StatusCode::NOT_FOUND, "مهمة التحميل غير موجودة".to_string()))?
    };

    match payload.action.as_str() {
        "pause" => {
            if matches!(
                job.meta.lock().await.status,
                DownloadStatus::Completed | DownloadStatus::Failed
            ) {
                return Err((
                    StatusCode::CONFLICT,
                    "A finished task cannot be paused".to_string(),
                ));
            }
            job.schedule_paused.store(false, Ordering::SeqCst);
            job.pause_flag.store(true, Ordering::SeqCst);
            let mut meta = job.meta.lock().await;
            meta.status = DownloadStatus::Paused;
            meta.updated_at = Utc::now();
            save_meta_to_disk_atomic(&meta, &meta.meta_file_path())
                .await
                .map_err(|error| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("Could not persist paused task: {error}"),
                    )
                })?;
            Ok(Json(serde_json::json!({ "status": "paused" })))
        }
        "resume" => {
            if job.meta.lock().await.status == DownloadStatus::Completed {
                return Err((
                    StatusCode::CONFLICT,
                    "A completed task cannot be resumed".to_string(),
                ));
            }
            job.schedule_paused.store(false, Ordering::SeqCst);
            job.pause_flag.store(false, Ordering::SeqCst);
            if job.uses_external_worker {
                let mut meta = job.meta.lock().await;
                meta.status = DownloadStatus::Downloading;
                meta.updated_at = Utc::now();
            } else {
                state.spawn_download_job(Arc::clone(&job)).await;
            }
            Ok(Json(serde_json::json!({ "status": "resumed" })))
        }
        "cancel" => {
            if job.meta.lock().await.status == DownloadStatus::Completed {
                return Err((
                    StatusCode::CONFLICT,
                    "A completed task cannot be cancelled".to_string(),
                ));
            }
            job.cancel_flag.store(true, Ordering::SeqCst);
            job.pause_flag.store(true, Ordering::SeqCst);
            let mut meta = job.meta.lock().await;
            meta.status = DownloadStatus::Failed;
            meta.last_error = Some("Cancelled by user".to_string());
            meta.updated_at = Utc::now();
            save_meta_to_disk_atomic(&meta, &meta.meta_file_path())
                .await
                .map_err(|error| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("Could not persist cancelled task: {error}"),
                    )
                })?;
            Ok(Json(serde_json::json!({ "status": "cancelled" })))
        }
        _ => Err((StatusCode::BAD_REQUEST, "أمر تحكم غير معروف".to_string())),
    }
}

async fn crawl_site_handler(
    State(state): State<Arc<AppState>>,
    Json(cfg): Json<CrawlConfig>,
) -> Result<Json<CrawlReport>, (StatusCode, String)> {
    let crawler = BoundedSiteCrawler::new(state.client.clone());
    let report = crawler
        .crawl(&cfg)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    Ok(Json(report))
}

async fn get_queue_schedule_handler(
    State(state): State<Arc<AppState>>,
    AxumPath(queue_name): AxumPath<String>,
) -> Result<Json<QueueSchedule>, (StatusCode, String)> {
    let queues = state.queues.read().await;
    let queue = queues
        .get(&queue_name)
        .ok_or_else(|| (StatusCode::NOT_FOUND, "Queue not found".to_string()))?;
    Ok(Json(queue.schedule()))
}

async fn set_queue_schedule_handler(
    State(state): State<Arc<AppState>>,
    AxumPath(queue_name): AxumPath<String>,
    Json(mut schedule): Json<QueueSchedule>,
) -> Result<Json<QueueSchedule>, (StatusCode, String)> {
    match schedule.mode {
        ScheduleMode::Disabled => {
            schedule.start_time = None;
            schedule.stop_time = None;
            schedule.start_at = None;
        }
        ScheduleMode::Daily => {
            let start = schedule.start_time.as_deref().ok_or_else(|| {
                (
                    StatusCode::BAD_REQUEST,
                    "Daily start time is required".to_string(),
                )
            })?;
            let stop = schedule.stop_time.as_deref().ok_or_else(|| {
                (
                    StatusCode::BAD_REQUEST,
                    "Daily stop time is required".to_string(),
                )
            })?;
            let start_time = parse_schedule_time(start)
                .map_err(|error| (StatusCode::BAD_REQUEST, error.to_string()))?;
            let stop_time = parse_schedule_time(stop)
                .map_err(|error| (StatusCode::BAD_REQUEST, error.to_string()))?;
            if start_time == stop_time {
                return Err((
                    StatusCode::BAD_REQUEST,
                    "Daily start and stop times must differ".to_string(),
                ));
            }
            schedule.start_at = None;
        }
        ScheduleMode::Once => {
            let start_at = schedule.start_at.as_deref().ok_or_else(|| {
                (
                    StatusCode::BAD_REQUEST,
                    "One-time start date is required".to_string(),
                )
            })?;
            NaiveDateTime::parse_from_str(start_at, "%Y-%m-%d %H:%M").map_err(|error| {
                (
                    StatusCode::BAD_REQUEST,
                    format!("Invalid one-time date and time: {error}"),
                )
            })?;
            schedule.start_time = None;
            schedule.stop_time = None;
        }
    }

    let previous_schedule = {
        let mut queues = state.queues.write().await;
        let queue = queues
            .get_mut(&queue_name)
            .ok_or_else(|| (StatusCode::NOT_FOUND, "Queue not found".to_string()))?;
        let previous = queue.schedule();
        queue.schedule_mode = schedule.mode;
        queue.start_time = schedule.start_time.clone();
        queue.stop_time = schedule.stop_time.clone();
        queue.start_at = schedule.start_at.clone();
        queue.stop_at = schedule.stop_at.clone();
        queue.days_of_week = schedule.days_of_week.clone();
        queue.speed_limit_kbps = schedule.speed_limit_kbps;
        previous
    };

    if schedule.speed_limit_kbps > 0 {
        state.speed_limiter.set_limit_kbps(schedule.speed_limit_kbps);
    }

    if let Err(error) = persist_queue_schedules(&state).await {
        if let Some(queue) = state.queues.write().await.get_mut(&queue_name) {
            queue.schedule_mode = previous_schedule.mode;
            queue.start_time = previous_schedule.start_time;
            queue.stop_time = previous_schedule.stop_time;
            queue.start_at = previous_schedule.start_at;
            queue.stop_at = previous_schedule.stop_at;
            queue.days_of_week = previous_schedule.days_of_week;
            queue.speed_limit_kbps = previous_schedule.speed_limit_kbps;
        }
        return Err((StatusCode::INTERNAL_SERVER_ERROR, error.to_string()));
    }
    let _ = persist_all_queues(&state).await;
    Ok(Json(schedule))
}

/// استكشاف جودات الفيديو الحقيقية (HLS m3u8 / DASH mpd / yt-dlp / ملفات مباشرة) لعرضها في قائمة IDM
async fn probe_video_qualities_handler(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<ProbeVideoPayload>,
) -> Result<Json<ProbeVideoResponse>, (StatusCode, String)> {
    let raw_title = payload
        .page_title
        .clone()
        .filter(|t| !t.trim().is_empty())
        .unwrap_or_else(|| "video_stream".to_string());
    let clean_title = sanitize_filename(&raw_title);

    let mut headers = HeaderMap::new();
    if let Some(ref c) = payload.cookies {
        if let Ok(hv) = HeaderValue::from_str(c) {
            headers.insert(COOKIE, hv);
        }
    }
    if let Some(ref r) = payload.referer {
        if let Ok(hv) = HeaderValue::from_str(r) {
            headers.insert(REFERER, hv);
        }
    }
    if let Some(ref ua) = payload.user_agent {
        if let Ok(hv) = HeaderValue::from_str(ua) {
            headers.insert(USER_AGENT, hv);
        }
    }

    let url_lower = payload.url.to_lowercase();

    if url_lower.contains(".m3u8") {
        let text = state
            .client
            .get(&payload.url)
            .headers(headers)
            .send()
            .await
            .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?
            .text()
            .await
            .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;

        if text.contains("SAMPLE-AES")
            || text.contains("com.widevine")
            || text.contains("com.apple.fps")
        {
            return Ok(Json(ProbeVideoResponse {
                title: clean_title,
                drm_protected: true,
                drm_reason: Some(
                    "هذا البث محمي بنظام إدارة الحقوق الرقمية (DRM) ولا يمكن تنزيله.".to_string(),
                ),
                qualities: Vec::new(),
            }));
        }

        let base_url =
            Url::parse(&payload.url).map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;

        let mut qualities = Vec::new();
        if text.contains("#EXT-X-STREAM-INF") {
            let (mut variants, _renditions) =
                MediaStreamEngine::parse_master_playlist(&text, &base_url)
                    .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
            variants.sort_by(|a, b| b.bandwidth.cmp(&a.bandwidth));

            for (idx, v) in variants.into_iter().enumerate() {
                let res_str = v.resolution.clone().unwrap_or_else(|| "Auto".to_string());
                let height_tag = res_str
                    .split('x')
                    .nth(1)
                    .map(|h| format!("{}p", h))
                    .unwrap_or_else(|| res_str.clone());
                let kbps = v.bandwidth / 1000;
                let codec_suffix = v
                    .codecs
                    .as_deref()
                    .map(|c| format!(" [{}]", c))
                    .unwrap_or_default();
                qualities.push(VideoQualityOption {
                    id: format!("hls_{}", idx),
                    label: format!("MP4 / HLS {}{} ({} kbps)", height_tag, codec_suffix, kbps),
                    resolution: Some(res_str),
                    container: "ts".to_string(),
                    bandwidth_bps: Some(v.bandwidth),
                    estimated_size_bytes: None,
                    download_url: v.uri,
                    audio_url: None,
                    protocol: "hls".to_string(),
                    format_id: None,
                    sabr_session: None,
                });
            }
        } else {
            qualities.push(VideoQualityOption {
                id: "hls_single".to_string(),
                label: "MP4 / HLS (الجودة الأصلية)".to_string(),
                resolution: None,
                container: "ts".to_string(),
                bandwidth_bps: None,
                estimated_size_bytes: None,
                download_url: payload.url.clone(),
                audio_url: None,
                protocol: "hls".to_string(),
                format_id: None,
                sabr_session: None,
            });
        }

        return Ok(Json(ProbeVideoResponse {
            title: clean_title,
            drm_protected: false,
            drm_reason: None,
            qualities,
        }));
    }

    if url_lower.contains(".mpd") {
        let xml = state
            .client
            .get(&payload.url)
            .headers(headers)
            .send()
            .await
            .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?
            .text()
            .await
            .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;

        if xml.contains("ContentProtection")
            || xml.contains("cenc:pssh")
            || xml.contains("widevine")
        {
            return Ok(Json(ProbeVideoResponse {
                title: clean_title,
                drm_protected: true,
                drm_reason: Some(
                    "بث MPEG-DASH محمي بنظام DRM (ContentProtection) ولا يدعم التنزيل.".to_string(),
                ),
                qualities: Vec::new(),
            }));
        }

        let base_url =
            Url::parse(&payload.url).map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
        let mut reps = DashMpdParser::parse_mpd(&xml, &base_url)
            .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
        reps.sort_by(|a, b| b.bandwidth.cmp(&a.bandwidth));

        let mut qualities = Vec::new();
        for r in reps {
            let res = match (r.width, r.height) {
                (Some(w), Some(h)) => Some(format!("{}x{}", w, h)),
                _ => None,
            };
            let height_label = r
                .height
                .map(|h| format!("{}p", h))
                .unwrap_or_else(|| r.id.clone());
            let codec_suffix = r
                .codecs
                .as_deref()
                .map(|c| format!(" [{}]", c))
                .unwrap_or_default();
            qualities.push(VideoQualityOption {
                id: format!("dash_{}", r.id),
                label: format!("DASH {}{} ({} kbps)", height_label, codec_suffix, r.bandwidth / 1000),
                resolution: res,
                container: "mp4".to_string(),
                bandwidth_bps: Some(r.bandwidth),
                estimated_size_bytes: r.estimated_size_bytes,
                download_url: payload.url.clone(),
                audio_url: None,
                protocol: "dash".to_string(),
                format_id: Some(r.id),
                sabr_session: None,
            });
        }

        return Ok(Json(ProbeVideoResponse {
            title: clean_title,
            drm_protected: false,
            drm_reason: None,
            qualities,
        }));
    }

    let target_page_validated = crate::core::tools::validate_web_url_for_tools(
        payload.page_url.as_ref().unwrap_or(&payload.url),
    )
    .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    let target_page = &target_page_validated;
    let is_streaming_page = target_page.contains("youtube.com/")
        || target_page.contains("youtu.be/")
        || target_page.contains("vimeo.com/")
        || target_page.contains("dailymotion.com/")
        || target_page.contains("twitch.tv/")
        || target_page.contains("twitter.com/")
        || target_page.contains("x.com/")
        || target_page.contains("tiktok.com/")
        || target_page.contains("facebook.com/");

    if is_streaming_page
        || (!url_lower.contains(".mp4")
            && !url_lower.contains(".webm")
            && !url_lower.contains(".mkv"))
    {
        if let Some(ref c) = payload.cookies {
            if !c.trim().is_empty() {
                remember_site_cookies(target_page, c);
            }
        }
        let ytdlp_bin = find_tool("yt-dlp").unwrap_or_else(|| PathBuf::from("yt-dlp"));

        let run_ytdlp_probe = |use_cookies: Option<String>| {
            let bin = ytdlp_bin.clone();
            let page = target_page.clone();
            let ref_hdr = payload.referer.clone();
            async move {
                let mut cmd = TokioCommand::new(&bin);
                hide_tokio_console_window(&mut cmd);
                if let Some(work_dir) = crate::core::tools::writable_tool_dir("yt-dlp") {
                    cmd.env("TEMP", &work_dir).env("TMP", &work_dir);
                }
                cmd.kill_on_drop(true)
                    .arg("--ignore-config")
                    .arg("-J")
                    .arg("--no-playlist")
                    .arg("--no-warnings")
                    .arg("--no-cache-dir")
                    .arg("--remote-components")
                    .arg("ejs:github")
                    .arg("--js-runtimes")
                    .arg("node");
                if let Some(ref r) = ref_hdr.filter(|r| r.starts_with("http")) {
                    cmd.arg("--referer").arg(r);
                }
                let _cookie_guard = if let Some(ref c) = use_cookies {
                    if let Ok(guard) = crate::media::create_temp_netscape_cookie_file(&page, c) {
                        cmd.arg("--cookies").arg(&guard.0);
                        Some(guard)
                    } else {
                        cmd.arg("--add-header").arg(format!("Cookie: {}", c));
                        None
                    }
                } else {
                    None
                };
                cmd.arg("--").arg(&page);
                timeout(Duration::from_secs(18), cmd.output()).await
            }
        };

        let cached_info = {
            let mut cache = global_youtube_probe_cache().lock().await;
            cache.get_valid(target_page)
        };

        let info_opt: Option<serde_json::Value> = match cached_info {
            Some(Ok(val)) => Some(val),
            Some(Err(rate_err)) => {
                return Err((StatusCode::TOO_MANY_REQUESTS, rate_err));
            }
            None => {
                // المحاولة بدون كوكيز أولاً (لحماية حساب المستخدم على يوتيوب)، وإعادة المحاولة بالكوكيز فقط إذا طلب الموقع تسجيل الدخول!
                let mut first_try = run_ytdlp_probe(None).await;
                if let Ok(Ok(ref out)) = first_try {
                    if !out.status.success() {
                        let stderr_str = String::from_utf8_lossy(&out.stderr);
                        if needs_login_or_cookies(&stderr_str) {
                            let avail_cookies = payload
                                .cookies
                                .clone()
                                .filter(|c| !c.trim().is_empty())
                                .or_else(|| remembered_site_cookies(target_page));
                            if let Some(c) = avail_cookies {
                                first_try = run_ytdlp_probe(Some(c)).await;
                            }
                        }
                    }
                }

                if let Ok(Ok(output)) = first_try {
                    if output.status.success() {
                        if let Ok(parsed_json) =
                            serde_json::from_slice::<serde_json::Value>(&output.stdout)
                        {
                            let mut cache = global_youtube_probe_cache().lock().await;
                            cache.insert_result(target_page.clone(), Ok(parsed_json.clone()));
                            Some(parsed_json)
                        } else {
                            None
                        }
                    } else {
                        let stderr_str = String::from_utf8_lossy(&output.stderr);
                        let (msg, code) = classify_youtube_error(&stderr_str);
                        if code == 429 {
                            let mut cache = global_youtube_probe_cache().lock().await;
                            cache.insert_result(target_page.clone(), Err(msg.clone()));
                            return Err((StatusCode::TOO_MANY_REQUESTS, msg));
                        }
                        None
                    }
                } else {
                    None
                }
            }
        };

        if let Some(info) = info_opt {
            {
                    if info.get("drm").and_then(|v| v.as_bool()).unwrap_or(false) {
                        return Ok(Json(ProbeVideoResponse {
                            title: clean_title,
                            drm_protected: true,
                            drm_reason: Some("الفيديو محمي بـ DRM.".to_string()),
                            qualities: Vec::new(),
                        }));
                    }
                    let real_title = info
                        .get("title")
                        .and_then(|v| v.as_str())
                        .map(sanitize_filename)
                        .unwrap_or_else(|| clean_title.clone());

                    let mut qualities = Vec::new();
                    let mut seen_height_codec = HashSet::new();
                    let duration_secs = info
                        .get("duration")
                        .and_then(|d| d.as_u64().or_else(|| d.as_f64().map(|f| f as u64)))
                        .unwrap_or(0);

                    if let Some(formats) = info.get("formats").and_then(|f| f.as_array()) {
                        // اختيار المسار الصوتي الأعلى رتبة (اللغة الأصلية أولاً عبر audio_track_rank ثم أعلى معدل بت abr)
                        let best_audio_fmt = formats
                            .iter()
                            .filter(|fmt| {
                                let vc = fmt.get("vcodec").and_then(|v| v.as_str()).unwrap_or("none");
                                let ac = fmt.get("acodec").and_then(|v| v.as_str()).unwrap_or("none");
                                vc == "none" && ac != "none"
                            })
                            .max_by_key(|fmt| {
                                let rank = audio_track_rank(fmt);
                                let abr_bps = fmt
                                    .get("abr")
                                    .and_then(|a| a.as_f64())
                                    .map(|a| (a * 1000.0) as i64)
                                    .unwrap_or(0);
                                (rank, abr_bps)
                            });

                        let best_audio_url = best_audio_fmt
                            .and_then(|fmt| fmt.get("url").and_then(|u| u.as_str()).map(String::from));

                        let best_audio_size = formats
                            .iter()
                            .filter(|fmt| {
                                let vc = fmt.get("vcodec").and_then(|v| v.as_str()).unwrap_or("none");
                                let ac = fmt.get("acodec").and_then(|v| v.as_str()).unwrap_or("none");
                                vc == "none" && ac != "none"
                            })
                            .filter_map(|fmt| {
                                fmt.get("filesize")
                                    .and_then(|s| s.as_u64())
                                    .or_else(|| fmt.get("filesize_approx").and_then(|s| s.as_u64()))
                            })
                            .max()
                            .unwrap_or(0);

                        for fmt in formats.iter().rev() {
                            let vcodec =
                                fmt.get("vcodec").and_then(|v| v.as_str()).unwrap_or("none");
                            if vcodec == "none" {
                                continue;
                            }
                            let height = fmt.get("height").and_then(|h| h.as_u64()).unwrap_or(0);
                            let is_av1 = is_av1_vcodec(vcodec);
                            // فصل خيار AV1 عن H.264 لنفس الدقة (كما في Ezoo Download)
                            if height < 144 || !seen_height_codec.insert((height, is_av1)) {
                                continue;
                            }
                            let av1_badge = if is_av1 { " AV1" } else { "" };
                            let av1_id_suffix = if is_av1 { "_av1" } else { "" };
                            let ext = fmt.get("ext").and_then(|e| e.as_str()).unwrap_or("mp4");
                            let acodec =
                                fmt.get("acodec").and_then(|a| a.as_str()).unwrap_or("none");
                            let direct_v_url = fmt.get("url").and_then(|u| u.as_str());
                            let proto = fmt.get("protocol").and_then(|p| p.as_str()).unwrap_or("");
                            let is_direct_http =
                                proto == "https" || proto == "http" || !proto.contains("m3u8");

                            let filesize = fmt
                                .get("filesize")
                                .and_then(|s| s.as_u64())
                                .or_else(|| fmt.get("filesize_approx").and_then(|s| s.as_u64()));
                            let fps = fmt.get("fps").and_then(|f| f.as_u64()).unwrap_or(30);

                            if let (Some(v_url), true) = (direct_v_url, is_direct_http) {
                                if acodec != "none" {
                                    qualities.push(VideoQualityOption {
                                        id: format!("direct_{}p", height),
                                        label: format!(
                                            "MP4 {}p ({}fps - متعدد الاتصالات)",
                                            height, fps
                                        ),
                                        resolution: Some(format!("{}p", height)),
                                        container: ext.to_string(),
                                        bandwidth_bps: None,
                                        estimated_size_bytes: filesize,
                                        download_url: v_url.to_string(),
                                        audio_url: None,
                                        protocol: "direct".to_string(),
                                        format_id: None,
                                        sabr_session: None,
                                    });
                                    continue;
                                } else if best_audio_url.is_some() && find_tool("ffmpeg").is_some()
                                {
                                    qualities.push(VideoQualityOption {
                                        id: format!("mux_{}p{}", height, av1_id_suffix),
                                        label: format!(
                                            "MP4{} {}p ({}fps - تسريع متعدد الاتصالات)",
                                            av1_badge, height, fps
                                        ),
                                        resolution: Some(format!("{}p", height)),
                                        container: "mp4".to_string(),
                                        bandwidth_bps: None,
                                        estimated_size_bytes: filesize.map(|s| s + best_audio_size),
                                        download_url: v_url.to_string(),
                                        audio_url: best_audio_url.clone(),
                                        protocol: "direct_mux".to_string(),
                                        format_id: None,
                                        sabr_session: None,
                                    });
                                    continue;
                                }
                            }

                            let selector = format!(
                                "bestvideo[height<={}]+bestaudio/best[height<={}]",
                                height, height
                            );
                            qualities.push(VideoQualityOption {
                                id: format!("ytdlp_{}p", height),
                                label: format!("MP4 {}p ({}fps)", height, fps),
                                resolution: Some(format!("{}p", height)),
                                container: ext.to_string(),
                                bandwidth_bps: None,
                                estimated_size_bytes: filesize,
                                download_url: target_page.clone(),
                                audio_url: None,
                                protocol: "ytdlp".to_string(),
                                format_id: Some(selector),
                                sabr_session: None,
                            });
                        }
                    }

                    qualities.push(VideoQualityOption {
                        id: "ytdlp_audio".to_string(),
                        label: "Audio Only · M4A (الصوت الأصلي بأعلى جودة)".to_string(),
                        resolution: Some("Audio".to_string()),
                        container: "m4a".to_string(),
                        bandwidth_bps: None,
                        estimated_size_bytes: None,
                        download_url: target_page.clone(),
                        audio_url: None,
                        protocol: "ytdlp".to_string(),
                        format_id: Some("bestaudio/best".to_string()),
                        sabr_session: None,
                    });
                    let mp3_est_size = (duration_secs > 0).then_some(duration_secs * 192 * 1000 / 8);
                    qualities.push(VideoQualityOption {
                        id: "ytdlp_audio_mp3".to_string(),
                        label: "Audio · MP3 192k (متوافق مع جميع المشغلات)".to_string(),
                        resolution: Some("MP3 192k".to_string()),
                        container: "mp3".to_string(),
                        bandwidth_bps: Some(192_000),
                        estimated_size_bytes: mp3_est_size,
                        download_url: target_page.clone(),
                        audio_url: None,
                        protocol: "ytdlp".to_string(),
                        format_id: Some("audio:bestaudio/best".to_string()),
                        sabr_session: None,
                    });

                    if !qualities.is_empty() {
                        return Ok(Json(ProbeVideoResponse {
                            title: real_title,
                            drm_protected: false,
                            drm_reason: None,
                            qualities,
                        }));
                    }
                }
        }

        let presets = [
            ("2160", "MP4 2160p (4K Ultra HD)"),
            ("1440", "MP4 1440p (2K QHD)"),
            ("1080", "MP4 1080p (Full HD)"),
            ("720", "MP4 720p (HD)"),
            ("480", "MP4 480p (SD)"),
            ("360", "MP4 360p (Medium)"),
        ];
        let mut qualities = Vec::new();
        for (h, label) in presets {
            qualities.push(VideoQualityOption {
                id: format!("ytdlp_{}p", h),
                label: label.to_string(),
                resolution: Some(format!("{}p", h)),
                container: "mp4".to_string(),
                bandwidth_bps: None,
                estimated_size_bytes: None,
                download_url: target_page.clone(),
                audio_url: None,
                protocol: "ytdlp".to_string(),
                format_id: Some(format!(
                    "bestvideo[height<={}]+bestaudio/best[height<={}]/best",
                    h, h
                )),
                sabr_session: None,
            });
        }

        return Ok(Json(ProbeVideoResponse {
            title: clean_title,
            drm_protected: false,
            drm_reason: None,
            qualities,
        }));
    }

    let (size, _resumable, probed_name, _etag, _lm) =
        DynamicDownloadJob::probe_url(&state.client, &payload.url, &headers)
            .await
            .unwrap_or((0, true, format!("{}.mp4", clean_title), None, None));

    let ext = Path::new(&probed_name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("mp4")
        .to_lowercase();

    Ok(Json(ProbeVideoResponse {
        title: clean_title,
        drm_protected: false,
        drm_reason: None,
        qualities: vec![VideoQualityOption {
            id: "direct_original".to_string(),
            label: format!("{} (الملف المباشر الأصلي)", ext.to_uppercase()),
            resolution: Some("Original".to_string()),
            container: ext,
            bandwidth_bps: None,
            estimated_size_bytes: if size > 0 { Some(size) } else { None },
            download_url: payload.url,
            audio_url: None,
            protocol: "direct".to_string(),
            format_id: None,
            sabr_session: None,
        }],
    }))
}

async fn start_video_download_handler(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<StartVideoDownloadPayload>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let base_title = payload
        .title
        .clone()
        .filter(|t| !t.trim().is_empty())
        .unwrap_or_else(|| "video".to_string());
    let quality_suffix = payload
        .quality_label
        .as_deref()
        .and_then(|q| {
            let re = Regex::new(r"(\d{3,4}p)").ok()?;
            re.captures(q)
                .and_then(|c| c.get(1))
                .map(|m| m.as_str().to_string())
        })
        .unwrap_or_default();

    let ext = payload
        .container
        .clone()
        .filter(|e| !e.is_empty())
        .unwrap_or_else(|| "mp4".to_string());

    let filename = if quality_suffix.is_empty() {
        sanitize_filename(&format!("{}.{}", base_title, ext))
    } else {
        sanitize_filename(&format!("{}_{}.{}", base_title, quality_suffix, ext))
    };

    if payload.protocol == "direct" && payload.audio_url.is_none() {
        let task_id = state
            .enqueue_download(InterceptDownloadPayload {
                url: payload.url,
                source_page_url: payload.page_url,
                filename: Some(filename),
                cookies: payload.cookies,
                referer: payload.referer,
                user_agent: payload.user_agent,
                queue_name: Some("main".to_string()),
                expected_sha256: None,
                ..Default::default()
            })
            .await
            .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;

        return Ok(Json(serde_json::json!({
            "status": "accepted",
            "task_id": task_id,
            "mode": "direct_segmented"
        })));
    }

    let save_dir = state
        .base_download_dir
        .join(FileCategory::Video.folder_name());
    let _ = fs::create_dir_all(&save_dir).await;
    let overwrite_existing =
        state.config.read().await.duplicate_file_policy == DuplicateFilePolicy::Overwrite;
    let filename = crate::core::filename::resolve_unique_filename(
        &save_dir,
        &filename,
        overwrite_existing,
    );
    let task_id = Uuid::new_v4().to_string();

    let mut custom_headers = HashMap::new();
    let mut header_map = HeaderMap::new();
    if let Some(ref c) = payload.cookies {
        if !c.is_empty() {
            custom_headers.insert("cookie".to_string(), c.clone());
            if let Ok(hv) = HeaderValue::from_str(c) {
                header_map.insert(COOKIE, hv);
            }
        }
    }
    if let Some(ref r) = payload.referer {
        if !r.is_empty() {
            custom_headers.insert("referer".to_string(), r.clone());
            if let Ok(hv) = HeaderValue::from_str(r) {
                header_map.insert(REFERER, hv);
            }
        }
    }
    if let Some(ref ua) = payload.user_agent {
        if !ua.is_empty() {
            custom_headers.insert("user-agent".to_string(), ua.clone());
            if let Ok(hv) = HeaderValue::from_str(ua) {
                header_map.insert(USER_AGENT, hv);
            }
        }
    }

    let schedule = state
        .queues
        .read()
        .await
        .get("main")
        .map(QueueConfig::schedule)
        .unwrap_or_default();
    let should_start = scheduled_queue_should_run(&schedule, Local::now().naive_local())
        .map_err(|error| (StatusCode::BAD_REQUEST, error.to_string()))?
        .unwrap_or(true);

    let validated_sabr = if let Some(ref sabr_raw) = payload.sabr_session {
        Some(
            normalize_sabr_session(sabr_raw)
                .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?,
        )
    } else {
        None
    };

    let initial_sabr_size = validated_sabr
        .as_ref()
        .and_then(|s| s.estimated_total_bytes)
        .unwrap_or(0);

    let meta = DownloadStateMeta {
        task_id: task_id.clone(),
        url: payload.url.clone(),
        source_page_url: payload.page_url.clone(),
        filename: filename.clone(),
        save_dir: save_dir.clone(),
        category: FileCategory::Video,
        total_size: initial_sabr_size,
        resumable: false,
        etag: None,
        last_modified: None,
        custom_headers: custom_headers.clone(),
        expected_sha256: None,
        computed_sha256: None,
        status: if should_start {
            DownloadStatus::Downloading
        } else {
            DownloadStatus::Paused
        },
        last_error: None,
        refresh_attempts: 0,
        segments: vec![FileSegment {
            id: 0,
            start_byte: 0,
            current_offset: 0,
            end_byte: 100,
            status: SegmentStatus::Downloading,
            split_count: 0,
            last_speed_bps: 0,
        }],
        created_at: Utc::now(),
        updated_at: Utc::now(),
        queue_name: "main".to_string(),
        mirror_urls: Vec::new(),
        active_mirror_index: 0,
        expected_md5: None,
        computed_md5: None,
        expected_crc32: None,
        computed_crc32: None,
        description: Some(format!("Video stream ({})", payload.protocol.to_uppercase())),
        speed_history: Vec::new(),
        connection_logs: vec![ConnectionLogEntry {
            timestamp: Utc::now(),
            segment_id: Some(0),
            level: "INFO".to_string(),
            message: format!("بدء تدفق الفيديو عبر بروتوكول {}", payload.protocol),
        }],
        task_speed_limit_kbps: 0,
    };
    let pause_flag = Arc::new(AtomicBool::new(!should_start));
    let cancel_flag = Arc::new(AtomicBool::new(false));
    let meta_arc = Arc::new(Mutex::new(meta));

    let job = Arc::new(DynamicDownloadJob {
        meta: Arc::clone(&meta_arc),
        pause_flag: Arc::clone(&pause_flag),
        cancel_flag: Arc::clone(&cancel_flag),
        resource_mutated_flag: Arc::new(AtomicBool::new(false)),
        expired_url_flag: Arc::new(AtomicBool::new(false)),
        speed_limiter: Arc::clone(&state.speed_limiter),
        client: state.client.clone(),
        max_connections: state.max_connections_per_file,
        min_split_size: MIN_SPLIT_SIZE,
        antivirus_cmd: None,
        uses_external_worker: true,
        runner_active: AtomicBool::new(false),
        schedule_paused: AtomicBool::new(!should_start),
    });

    state
        .jobs
        .write()
        .await
        .insert(task_id.clone(), Arc::clone(&job));
    state.post_action_fired.lock().await.remove("main");

    let client_clone = state.client.clone();
    let limiter_clone = Arc::clone(&state.speed_limiter);
    let max_conns = state.max_connections_per_file;
    let queue_semaphore = state
        .queue_semaphores
        .read()
        .await
        .get("main")
        .cloned()
        .unwrap_or_else(|| Arc::new(Semaphore::new(4)));
    let output_path = save_dir.join(&filename);
    let protocol = payload.protocol.clone();
    let target_url = payload.url.clone();
    let audio_url_opt = payload.audio_url.clone();
    let format_id = payload.format_id.clone();
    let sabr_session = payload.sabr_session.clone();
    let cookies = payload.cookies.clone();
    let referer = payload.referer.clone();
    let user_agent = payload.user_agent.clone();
    let filename_clone = filename.clone();

    job.runner_active.store(true, Ordering::SeqCst);
    tokio::spawn(async move {
        while pause_flag.load(Ordering::SeqCst) && !cancel_flag.load(Ordering::SeqCst) {
            sleep(Duration::from_millis(200)).await;
        }
        let _permit = match queue_semaphore.acquire_owned().await {
            Ok(permit) => permit,
            Err(error) => {
                let mut m = meta_arc.lock().await;
                m.status = DownloadStatus::Failed;
                m.last_error = Some(format!("Cannot acquire queue slot: {error}"));
                job.runner_active.store(false, Ordering::SeqCst);
                return;
            }
        };
        set_system_awake_for_downloads(true);

        let res = if (protocol == "direct_mux" || audio_url_opt.is_some())
            && audio_url_opt.as_deref().map(|s| !s.is_empty()).unwrap_or(false)
        {
            let audio_url = audio_url_opt.unwrap_or_default();
            download_and_mux_dual_streams(
                &client_clone,
                &limiter_clone,
                max_conns,
                &target_url,
                &audio_url,
                &header_map,
                &custom_headers,
                &save_dir,
                &filename_clone,
                &output_path,
                Arc::clone(&meta_arc),
                Arc::clone(&pause_flag),
                Arc::clone(&cancel_flag),
            )
            .await
        } else if protocol == "hls" {
            let engine = MediaStreamEngine::new(client_clone);
            engine
                .download_hls_stream_with_progress(
                    &target_url,
                    &output_path,
                    &header_map,
                    8,
                    Arc::clone(&meta_arc),
                    Arc::clone(&pause_flag),
                    Arc::clone(&cancel_flag),
                )
                .await
                .map(|_| ())
        } else if protocol == "dash" {
            let engine = MediaStreamEngine::new(client_clone);
            engine
                .download_dash_stream_with_progress(
                    &target_url,
                    &output_path,
                    &header_map,
                    format_id.as_deref(),
                    8,
                    Arc::clone(&meta_arc),
                    Arc::clone(&pause_flag),
                    Arc::clone(&cancel_flag),
                )
                .await
                .map(|_| ())
        } else {
            MediaStreamEngine::download_via_ytdlp_or_sabr(
                &target_url,
                &output_path,
                format_id.as_deref(),
                sabr_session.as_ref(),
                cookies.as_deref(),
                referer.as_deref(),
                user_agent.as_deref(),
                limiter_clone.get_limit_kbps(),
                Arc::clone(&pause_flag),
                Arc::clone(&cancel_flag),
            )
            .await
        };

        let mut m = meta_arc.lock().await;
        match res {
            Ok(()) => {
                if let Ok(metadata) = fs::metadata(&output_path).await {
                    let len = metadata.len();
                    m.total_size = len;
                    if let Some(s0) = m.segments.first_mut() {
                        s0.end_byte = len.saturating_sub(1);
                        s0.current_offset = len;
                        s0.status = SegmentStatus::Completed;
                        s0.last_speed_bps = 0;
                    }
                }
                if let Ok((sha256, md5, crc32)) = compute_file_all_digests(&output_path).await {
                    m.computed_sha256 = Some(sha256);
                    m.computed_md5 = Some(md5);
                    m.computed_crc32 = Some(crc32);
                }
                m.status = DownloadStatus::Completed;
                m.last_error = None;
                let _ = save_meta_to_disk_atomic(&m, &m.meta_file_path()).await;
            }
            Err(e) => {
                m.status = DownloadStatus::Failed;
                m.last_error = Some(e.to_string());
                if let Some(s0) = m.segments.first_mut() {
                    s0.status = SegmentStatus::Failed;
                }
                let _ = save_meta_to_disk_atomic(&m, &m.meta_file_path()).await;
            }
        }
        m.updated_at = Utc::now();
        drop(m);
        job.runner_active.store(false, Ordering::SeqCst);
    });

    Ok(Json(serde_json::json!({
        "status": "accepted",
        "task_id": task_id,
        "mode": payload.protocol
    })))
}

#[allow(clippy::too_many_arguments)]
async fn download_and_mux_dual_streams(
    client: &Client,
    limiter: &Arc<SpeedLimiter>,
    max_conns: usize,
    video_url: &str,
    audio_url: &str,
    header_map: &HeaderMap,
    custom_headers: &HashMap<String, String>,
    save_dir: &Path,
    final_filename: &str,
    output_path: &Path,
    parent_meta: Arc<Mutex<DownloadStateMeta>>,
    pause_flag: Arc<AtomicBool>,
    cancel_flag: Arc<AtomicBool>,
) -> Result<()> {
    let (v_size, v_resumable, _, v_etag, v_lm) =
        DynamicDownloadJob::probe_url(client, video_url, header_map)
            .await
            .unwrap_or((0, true, String::from("v.mp4"), None, None));
    let (a_size, a_resumable, _, a_etag, a_lm) =
        DynamicDownloadJob::probe_url(client, audio_url, header_map)
            .await
            .unwrap_or((0, true, String::from("a.m4a"), None, None));

    let combined_total = v_size.saturating_add(a_size);
    {
        let mut pm = parent_meta.lock().await;
        pm.total_size = combined_total;
        pm.resumable = v_resumable && a_resumable;
        pm.push_log(
            None,
            "INFO",
            format!(
                "بدء التحميل المزدوج السريع (فيديو: {} بايت + صوت: {} بايت) مع نوافذ 10MB",
                v_size, a_size
            ),
        );
    }

    let v_temp_name = format!(".{}.video_stream.tmp", final_filename);
    let a_temp_name = format!(".{}.audio_stream.tmp", final_filename);

    let v_meta = Arc::new(Mutex::new(DownloadStateMeta {
        task_id: format!("v_{}", Uuid::new_v4()),
        url: video_url.to_string(),
        source_page_url: None,
        filename: v_temp_name.clone(),
        save_dir: save_dir.to_path_buf(),
        category: FileCategory::Video,
        total_size: v_size,
        resumable: v_resumable,
        etag: v_etag,
        last_modified: v_lm,
        custom_headers: custom_headers.clone(),
        expected_sha256: None,
        computed_sha256: None,
        status: DownloadStatus::Downloading,
        last_error: None,
        refresh_attempts: 0,
        segments: Vec::new(),
        created_at: Utc::now(),
        updated_at: Utc::now(),
        queue_name: "main".to_string(),
        mirror_urls: Vec::new(),
        active_mirror_index: 0,
        expected_md5: None,
        computed_md5: None,
        expected_crc32: None,
        computed_crc32: None,
        description: None,
        speed_history: Vec::new(),
        connection_logs: Vec::new(),
        task_speed_limit_kbps: 0,
    }));

    let v_job = Arc::new(DynamicDownloadJob {
        meta: Arc::clone(&v_meta),
        pause_flag: Arc::clone(&pause_flag),
        cancel_flag: Arc::clone(&cancel_flag),
        resource_mutated_flag: Arc::new(AtomicBool::new(false)),
        expired_url_flag: Arc::new(AtomicBool::new(false)),
        speed_limiter: Arc::clone(limiter),
        client: client.clone(),
        max_connections: max_conns,
        min_split_size: MIN_SPLIT_SIZE,
        antivirus_cmd: None,
        uses_external_worker: false,
        runner_active: AtomicBool::new(false),
        schedule_paused: AtomicBool::new(false),
    });

    let a_meta = Arc::new(Mutex::new(DownloadStateMeta {
        task_id: format!("a_{}", Uuid::new_v4()),
        url: audio_url.to_string(),
        source_page_url: None,
        filename: a_temp_name.clone(),
        save_dir: save_dir.to_path_buf(),
        category: FileCategory::Video,
        total_size: a_size,
        resumable: a_resumable,
        etag: a_etag,
        last_modified: a_lm,
        custom_headers: custom_headers.clone(),
        expected_sha256: None,
        computed_sha256: None,
        status: DownloadStatus::Downloading,
        last_error: None,
        refresh_attempts: 0,
        segments: Vec::new(),
        created_at: Utc::now(),
        updated_at: Utc::now(),
        queue_name: "main".to_string(),
        mirror_urls: Vec::new(),
        active_mirror_index: 0,
        expected_md5: None,
        computed_md5: None,
        expected_crc32: None,
        computed_crc32: None,
        description: None,
        speed_history: Vec::new(),
        connection_logs: Vec::new(),
        task_speed_limit_kbps: 0,
    }));

    let a_job = Arc::new(DynamicDownloadJob {
        meta: Arc::clone(&a_meta),
        pause_flag: Arc::clone(&pause_flag),
        cancel_flag: Arc::clone(&cancel_flag),
        resource_mutated_flag: Arc::new(AtomicBool::new(false)),
        expired_url_flag: Arc::new(AtomicBool::new(false)),
        speed_limiter: Arc::clone(limiter),
        client: client.clone(),
        max_connections: max_conns.min(4).max(1),
        min_split_size: MIN_SPLIT_SIZE,
        antivirus_cmd: None,
        uses_external_worker: false,
        runner_active: AtomicBool::new(false),
        schedule_paused: AtomicBool::new(false),
    });

    // مزامنة تقدم مساري الفيديو والصوت المتوازيين في آنٍ واحد إلى المهمة الأم (على غرار runPairedTracks في Awesome Downloader)
    let pm_sync = Arc::clone(&parent_meta);
    let vm_clone = Arc::clone(&v_meta);
    let am_clone = Arc::clone(&a_meta);
    let sync_both = tokio::spawn(async move {
        loop {
            sleep(Duration::from_millis(120)).await;
            let vm = vm_clone.lock().await;
            let am = am_clone.lock().await;
            let mut pm = pm_sync.lock().await;
            let mut combined_segs = vm.segments.clone();
            let id_offset = combined_segs.len();
            for mut aseg in am.segments.clone() {
                aseg.id += id_offset;
                combined_segs.push(aseg);
            }
            pm.segments = combined_segs;
            let v_done = matches!(vm.status, DownloadStatus::Completed | DownloadStatus::Failed);
            let a_done = matches!(am.status, DownloadStatus::Completed | DownloadStatus::Failed);
            if v_done && a_done {
                break;
            }
        }
    });

    let parallel_res = tokio::try_join!(v_job.execute(), a_job.execute());
    sync_both.abort();
    let _ = fs::remove_file(save_dir.join(format!("{}.idm.json", v_temp_name))).await;
    let _ = fs::remove_file(save_dir.join(format!("{}.idm.json", a_temp_name))).await;
    let (v_final_path, a_final_path) = parallel_res?;

    if cancel_flag.load(Ordering::Relaxed) {
        anyhow::bail!("تم إلغاء المهمة قبل بدء دمج المسارات");
    }

    {
        let mut pm = parent_meta.lock().await;
        pm.status = DownloadStatus::Assembling;
        pm.push_log(
            None,
            "INFO",
            "اكتمل تنزيل الفيديو والصوت؛ جاري الدمج الآمن والتحقق من المسارات عبر FFmpeg...",
        );
    }

    let v_clone = v_final_path.clone();
    let a_clone = a_final_path.clone();
    let out_clone = output_path.to_path_buf();
    let fname_clone = final_filename.to_string();

    let mux_res = tokio::task::spawn_blocking(move || {
        merge_video_and_audio_streams(&v_clone, &a_clone, &out_clone, &fname_clone)
    })
    .await
    .context("فشلت مهمة الدمج الخلفية")?;

    // لا نحذف ملفات الفيديو والصوت المؤقتة إلا بعد التأكد التام من نجاح الدمج واجتياز فحص verify_merged_media_file
    if mux_res.is_ok() && output_path.exists() {
        let _ = fs::remove_file(&v_final_path).await;
        let _ = fs::remove_file(&a_final_path).await;
    }

    mux_res
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZipPreviewRequestPayload {
    pub url: String,
    #[serde(default)]
    pub cookies: Option<String>,
    #[serde(default)]
    pub referer: Option<String>,
    #[serde(default)]
    pub user_agent: Option<String>,
}

async fn zip_preview_handler(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<ZipPreviewRequestPayload>,
) -> Result<Json<RemoteZipPreviewResponse>, (StatusCode, String)> {
    let mut headers = HeaderMap::new();
    if let Some(ref c) = payload.cookies {
        if let Ok(hv) = HeaderValue::from_str(c) {
            headers.insert(COOKIE, hv);
        }
    }
    if let Some(ref r) = payload.referer {
        if let Ok(hv) = HeaderValue::from_str(r) {
            headers.insert(REFERER, hv);
        }
    }
    if let Some(ref ua) = payload.user_agent {
        if let Ok(hv) = HeaderValue::from_str(ua) {
            headers.insert(USER_AGENT, hv);
        }
    }

    let preview = RemoteZipInspector::inspect_remote_zip(&state.client, &payload.url, &headers)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    Ok(Json(preview))
}

async fn batch_enqueue_handler(
    State(state): State<Arc<AppState>>,
    Json(req): Json<BatchExpandRequest>,
) -> Result<Json<BatchExpandResponse>, (StatusCode, String)> {
    let step = req.step.unwrap_or(1).max(1);
    let expanded_urls = WildcardPatternExpander::expand_pattern(&req.pattern_or_urls, step)
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;

    let queue_name = req.queue_name.unwrap_or_else(|| "main".to_string());
    let mut queued_task_ids = Vec::new();
    let mut skipped_duplicates = 0usize;
    let mut errors = Vec::new();

    for url in &expanded_urls {
        let payload = InterceptDownloadPayload {
            url: url.clone(),
            source_page_url: req.referer.clone(),
            filename: None,
            cookies: req.cookies.clone(),
            referer: req.referer.clone(),
            user_agent: None,
            queue_name: Some(queue_name.clone()),
            expected_sha256: None,
            expected_md5: None,
            expected_crc32: None,
            mirror_urls: None,
            description: Some("Batch Download".to_string()),
        };

        match state.enqueue_download(payload).await {
            Ok(tid) => {
                if req.start_immediately == Some(false) {
                    if let Some(job) = state.jobs.read().await.get(&tid).cloned() {
                        job.pause_flag.store(true, Ordering::SeqCst);
                        let mut m = job.meta.lock().await;
                        m.status = DownloadStatus::Paused;
                    }
                }
                queued_task_ids.push(tid);
            }
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("الثواني الخمس") {
                    skipped_duplicates += 1;
                } else {
                    errors.push(format!("{} -> {}", url, msg));
                }
            }
        }
    }

    Ok(Json(BatchExpandResponse {
        expanded_urls,
        queued_task_ids,
        skipped_duplicates,
        errors,
    }))
}

async fn list_queues_handler(State(state): State<Arc<AppState>>) -> Json<Vec<QueueConfig>> {
    let queues = state.queues.read().await;
    let mut list: Vec<QueueConfig> = queues.values().cloned().collect();
    list.sort_by(|a, b| a.name.cmp(&b.name));
    Json(list)
}

async fn upsert_queue_handler(
    State(state): State<Arc<AppState>>,
    Json(mut cfg): Json<QueueConfig>,
) -> Result<Json<QueueConfig>, (StatusCode, String)> {
    cfg.name = cfg.name.trim().to_string();
    if cfg.name.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "اسم الطابور لا يمكن أن يكون فارغاً".to_string(),
        ));
    }
    if let PostQueueAction::CustomCommand(ref cmd_str) = cfg.post_action {
        validate_safe_executable_path(cmd_str)
            .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    }
    cfg.max_concurrent_downloads = cfg.max_concurrent_downloads.clamp(1, 32);

    {
        let mut sems = state.queue_semaphores.write().await;
        sems.insert(
            cfg.name.clone(),
            Arc::new(Semaphore::new(cfg.max_concurrent_downloads)),
        );
    }
    {
        let mut queues = state.queues.write().await;
        queues.insert(cfg.name.clone(), cfg.clone());
    }

    persist_all_queues(&state)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let _ = persist_queue_schedules(&state).await;

    Ok(Json(cfg))
}

async fn delete_queue_handler(
    State(state): State<Arc<AppState>>,
    AxumPath(queue_name): AxumPath<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    if queue_name == "main" {
        return Err((
            StatusCode::BAD_REQUEST,
            "لا يمكن حذف الطابور الرئيسي (main)".to_string(),
        ));
    }

    {
        let mut queues = state.queues.write().await;
        if queues.remove(&queue_name).is_none() {
            return Err((StatusCode::NOT_FOUND, "الطابور غير موجود".to_string()));
        }
    }

    let jobs: Vec<Arc<DynamicDownloadJob>> = state.jobs.read().await.values().cloned().collect();
    for job in jobs {
        let mut m = job.meta.lock().await;
        if m.queue_name == queue_name {
            m.queue_name = "main".to_string();
            let _ = save_meta_to_disk_atomic(&m, &m.meta_file_path()).await;
        }
    }

    persist_all_queues(&state)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(serde_json::json!({
        "status": "deleted",
        "deleted_queue": queue_name,
        "reassigned_to": "main"
    })))
}

async fn persist_all_queues(state: &AppState) -> Result<()> {
    let queues = state.queues.read().await.clone();
    let path = state.base_download_dir.join("queues.json");
    fs::create_dir_all(&state.base_download_dir).await?;
    let tmp_path = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(&queues)?;
    let mut file = File::create(&tmp_path).await?;
    file.write_all(&bytes).await?;
    file.sync_all().await?;
    drop(file);
    fs::rename(&tmp_path, &path).await?;
    sync_parent_dir(&path).await;
    Ok(())
}

async fn get_config_handler(State(state): State<Arc<AppState>>) -> Json<AppConfig> {
    let mut cfg = state.config.read().await.clone();
    for login in &mut cfg.site_logins {
        if !login.auth_header_or_cookie.is_empty() {
            login.auth_header_or_cookie = "[REDACTED_SECRET]".to_string();
        }
    }
    Json(cfg)
}

async fn set_config_handler(
    State(state): State<Arc<AppState>>,
    Json(mut new_cfg): Json<AppConfig>,
) -> Result<Json<AppConfig>, (StatusCode, String)> {
    if let Some(ref av_cmd) = new_cfg.antivirus_command {
        validate_safe_executable_path(av_cmd)
            .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    }

    new_cfg.max_connections_per_file = new_cfg.max_connections_per_file.clamp(1, 64);
    new_cfg.min_split_size_kb = new_cfg.min_split_size_kb.max(64);
    state
        .speed_limiter
        .set_limit_kbps(new_cfg.global_speed_limit_kbps);

    {
        let mut current = state.config.write().await;
        for new_login in &mut new_cfg.site_logins {
            if new_login.auth_header_or_cookie == "[REDACTED_SECRET]" {
                if let Some(existing) = current
                    .site_logins
                    .iter()
                    .find(|l| l.domain == new_login.domain && l.username == new_login.username)
                {
                    new_login.auth_header_or_cookie = existing.auth_header_or_cookie.clone();
                }
            }
        }
        *current = new_cfg.clone();
    }

    let path = state.base_download_dir.join("config.json");
    fs::create_dir_all(&state.base_download_dir)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let tmp_path = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(&new_cfg)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let mut file = File::create(&tmp_path)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    file.write_all(&bytes)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    file.sync_all()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    drop(file);
    fs::rename(&tmp_path, &path)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    sync_parent_dir(&path).await;

    Ok(Json(new_cfg))
}

async fn stats_handler(State(state): State<Arc<AppState>>) -> Json<EngineStatistics> {
    let jobs = state.jobs.read().await;
    let mut active_downloads = 0usize;
    let mut paused_downloads = 0usize;
    let mut queued_downloads = 0usize;
    let mut completed_downloads = 0usize;
    let mut failed_downloads = 0usize;
    let mut total_bytes_downloaded = 0u64;
    let mut current_aggregate_speed_bps = 0u64;

    for job in jobs.values() {
        let m = job.meta.lock().await;
        total_bytes_downloaded = total_bytes_downloaded.saturating_add(m.total_downloaded());
        match m.status {
            DownloadStatus::Downloading
            | DownloadStatus::RefreshingUrl
            | DownloadStatus::Assembling
            | DownloadStatus::VerifyingChecksum => {
                active_downloads += 1;
                let s: u64 = m.segments.iter().map(|seg| seg.last_speed_bps).sum();
                current_aggregate_speed_bps = current_aggregate_speed_bps.saturating_add(s);
            }
            DownloadStatus::Paused => paused_downloads += 1,
            DownloadStatus::Queued => queued_downloads += 1,
            DownloadStatus::Completed => completed_downloads += 1,
            DownloadStatus::Failed | DownloadStatus::NeedsUrlRefresh => failed_downloads += 1,
        }
    }

    set_system_awake_for_downloads(active_downloads > 0);

    let prev_peak = state.peak_speed_bps.load(Ordering::Relaxed);
    let peak = if current_aggregate_speed_bps > prev_peak {
        state
            .peak_speed_bps
            .store(current_aggregate_speed_bps, Ordering::Relaxed);
        current_aggregate_speed_bps
    } else {
        prev_peak
    };

    let queues_count = state.queues.read().await.len();

    Json(EngineStatistics {
        total_tasks: jobs.len(),
        active_downloads,
        paused_downloads,
        queued_downloads,
        completed_downloads,
        failed_downloads,
        total_bytes_downloaded,
        current_aggregate_speed_bps,
        peak_aggregate_speed_bps: peak,
        speed_limit_kbps: state.speed_limiter.get_limit_kbps(),
        queues_count,
    })
}

async fn task_logs_handler(
    State(state): State<Arc<AppState>>,
    AxumPath(task_id): AxumPath<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let job = state
        .jobs
        .read()
        .await
        .get(&task_id)
        .cloned()
        .ok_or_else(|| (StatusCode::NOT_FOUND, "المهمة غير موجودة".to_string()))?;
    let m = job.meta.lock().await;
    Ok(Json(serde_json::json!({
        "task_id": m.task_id,
        "filename": m.filename,
        "logs": m.connection_logs,
        "speed_history": m.speed_history,
        "computed_sha256": m.computed_sha256,
        "computed_md5": m.computed_md5,
        "computed_crc32": m.computed_crc32
    })))
}

async fn reverify_checksum_handler(
    State(state): State<Arc<AppState>>,
    AxumPath(task_id): AxumPath<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let job = state
        .jobs
        .read()
        .await
        .get(&task_id)
        .cloned()
        .ok_or_else(|| (StatusCode::NOT_FOUND, "المهمة غير موجودة".to_string()))?;

    let final_path = {
        let m = job.meta.lock().await;
        if m.status != DownloadStatus::Completed {
            return Err((
                StatusCode::BAD_REQUEST,
                "لا يمكن فحص البصمة النهائية إلا بعد اكتمال التحميل".to_string(),
            ));
        }
        m.final_file_path()
    };

    let (sha256, md5, crc32) = compute_file_all_digests(&final_path)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let mut m = job.meta.lock().await;
    m.computed_sha256 = Some(sha256.clone());
    m.computed_md5 = Some(md5.clone());
    m.computed_crc32 = Some(crc32.clone());
    m.push_log(
        None,
        "INFO",
        format!(
            "تمت إعادة فحص البصمات الرقمية بنجاح: SHA256={} | MD5={} | CRC32={}",
            sha256, md5, crc32
        ),
    );
    let _ = save_meta_to_disk_atomic(&m, &m.meta_file_path()).await;

    Ok(Json(serde_json::json!({
        "status": "verified",
        "sha256": sha256,
        "md5": md5,
        "crc32": crc32
    })))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportTasksPayload {
    pub content: String,
    #[serde(default)]
    pub default_queue: Option<String>,
    #[serde(default)]
    pub start_immediately: Option<bool>,
}

async fn import_tasks_handler(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<ImportTasksPayload>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let items = DownloadListImporter::parse_import_text(&payload.content);
    if items.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "لم يتم العثور على أي روابط صالحة للاستيراد".to_string(),
        ));
    }

    let fallback_queue = payload.default_queue.unwrap_or_else(|| "main".to_string());
    let mut imported_ids = Vec::new();
    let mut errors = Vec::new();

    for item in items {
        let req = InterceptDownloadPayload {
            url: item.url.clone(),
            source_page_url: item.referer.clone(),
            filename: item.filename,
            cookies: item.cookies,
            referer: item.referer,
            user_agent: item.user_agent,
            queue_name: Some(item.queue_name.unwrap_or_else(|| fallback_queue.clone())),
            expected_sha256: None,
            expected_md5: None,
            expected_crc32: None,
            mirror_urls: None,
            description: Some("Imported from list".to_string()),
        };
        match state.enqueue_download(req).await {
            Ok(tid) => {
                if payload.start_immediately == Some(false) {
                    if let Some(job) = state.jobs.read().await.get(&tid).cloned() {
                        job.pause_flag.store(true, Ordering::SeqCst);
                        job.meta.lock().await.status = DownloadStatus::Paused;
                    }
                }
                imported_ids.push(tid);
            }
            Err(e) => errors.push(format!("{} -> {}", item.url, e)),
        }
    }

    Ok(Json(serde_json::json!({
        "status": "imported",
        "imported_count": imported_ids.len(),
        "task_ids": imported_ids,
        "errors": errors
    })))
}

async fn export_tasks_handler(
    State(state): State<Arc<AppState>>,
) -> Result<String, (StatusCode, String)> {
    let jobs = state.jobs.read().await;
    let mut out = String::from("# RustIDM Exported Download List (EF2 Compatible)\n");
    for job in jobs.values() {
        let m = job.meta.lock().await;
        out.push_str("<\n");
        out.push_str(&format!("{}\n", m.url));
        if let Some(ref ref_url) = m.source_page_url {
            out.push_str(&format!("referer: {}\n", ref_url));
        }
        out.push_str(&format!("filename: {}\n", m.filename));
        out.push_str(&format!("queue: {}\n", m.queue_name));
        out.push_str(">\n");
    }
    Ok(out)
}

/// بناء موجه الخادم المحلي المحصّن أمنياً مع عزل مسارات الإضافة وتحديد سقف حجم الطلب (4MB)

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateTaskMetaPayload {
    pub new_filename: Option<String>,
    pub new_save_dir: Option<PathBuf>,
    pub new_queue_name: Option<String>,
    pub new_description: Option<String>,
    pub task_speed_limit_kbps: Option<u64>,
}

async fn update_task_meta_handler(
    State(state): State<Arc<AppState>>,
    AxumPath(task_id): AxumPath<String>,
    Json(payload): Json<UpdateTaskMetaPayload>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let job = state
        .jobs
        .read()
        .await
        .get(&task_id)
        .cloned()
        .ok_or_else(|| (StatusCode::NOT_FOUND, "المهمة غير موجودة".to_string()))?;

    if job.runner_active.load(Ordering::SeqCst)
        && (payload.new_filename.is_some() || payload.new_save_dir.is_some())
    {
        return Err((
            StatusCode::CONFLICT,
            "يرجى إيقاف التنزيل مؤقتاً قبل تغيير اسم الملف أو مجلد الحفظ".to_string(),
        ));
    }

    let mut m = job.meta.lock().await;
    let old_meta_path = m.meta_file_path();
    let old_part_path = m.part_file_path();
    let old_final_path = m.final_file_path();

    if let Some(ref raw_name) = payload.new_filename {
        let clean = sanitize_filename(raw_name.trim());
        if !clean.is_empty() && clean != m.filename {
            m.filename = clean;
        }
    }
    if let Some(ref new_dir) = payload.new_save_dir {
        if !new_dir.as_os_str().is_empty() && *new_dir != m.save_dir {
            fs::create_dir_all(new_dir)
                .await
                .map_err(|e| (StatusCode::BAD_REQUEST, format!("تعذر إنشاء المجلد: {e}")))?;
            m.save_dir = new_dir.clone();
        }
    }
    if let Some(ref q) = payload.new_queue_name {
        if !q.trim().is_empty() {
            m.queue_name = q.trim().to_string();
        }
    }
    if let Some(ref desc) = payload.new_description {
        m.description = Some(desc.clone());
    }
    if let Some(limit) = payload.task_speed_limit_kbps {
        m.task_speed_limit_kbps = limit;
    }

    let new_meta_path = m.meta_file_path();
    let new_part_path = m.part_file_path();
    let new_final_path = m.final_file_path();

    if old_part_path != new_part_path && old_part_path.exists() {
        let _ = fs::rename(&old_part_path, &new_part_path).await;
    }
    if old_final_path != new_final_path && old_final_path.exists() {
        let _ = fs::rename(&old_final_path, &new_final_path).await;
    }
    if old_meta_path != new_meta_path && old_meta_path.exists() {
        let _ = fs::remove_file(&old_meta_path).await;
    }

    m.updated_at = Utc::now();
    save_meta_to_disk_atomic(&m, &new_meta_path)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(serde_json::json!({
        "status": "updated",
        "task_id": m.task_id,
        "filename": m.filename,
        "save_dir": m.save_dir
    })))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProbeUrlRequestPayload {
    pub url: String,
    pub cookies: Option<String>,
    pub referer: Option<String>,
    pub user_agent: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProbeUrlResponsePayload {
    pub url: String,
    pub filename: String,
    pub total_size: u64,
    pub resumable: bool,
    pub category: FileCategory,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub host: Option<String>,
}

async fn probe_url_metadata_handler(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<ProbeUrlRequestPayload>,
) -> Result<Json<ProbeUrlResponsePayload>, (StatusCode, String)> {
    let parsed = Url::parse(payload.url.trim())
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("صيغة الرابط غير صالحة: {e}")))?;
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return Err((
            StatusCode::BAD_REQUEST,
            "يُدعم فقط بروتوكول HTTP أو HTTPS".to_string(),
        ));
    }
    let host = parsed.host_str().map(String::from);

    let mut headers = HeaderMap::new();
    if let Some(ref c) = payload.cookies {
        if let Ok(hv) = HeaderValue::from_str(c) {
            headers.insert(COOKIE, hv);
        }
    }
    if let Some(ref r) = payload.referer {
        if let Ok(hv) = HeaderValue::from_str(r) {
            headers.insert(REFERER, hv);
        }
    }
    if let Some(ref ua) = payload.user_agent {
        if let Ok(hv) = HeaderValue::from_str(ua) {
            headers.insert(USER_AGENT, hv);
        }
    }

    let (total_size, resumable, filename, etag, last_modified) =
        DynamicDownloadJob::probe_url(&state.client, payload.url.trim(), &headers)
            .await
            .map_err(|e| (StatusCode::BAD_GATEWAY, e.to_string()))?;
    let category = FileCategory::from_filename(&filename);

    Ok(Json(ProbeUrlResponsePayload {
        url: payload.url,
        filename,
        total_size,
        resumable,
        category,
        etag,
        last_modified,
        host,
    }))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterBrowserPayload {
    pub browser: String,
    pub extension_id: String,
}

async fn browser_integration_status_handler(
    State(state): State<Arc<AppState>>,
) -> Json<BrowserIntegrationReport> {
    let dir = state.base_download_dir.clone();
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("rust_idm"));
    let report = tokio::task::spawn_blocking(move || connect_installed_browsers(&dir, &exe, None))
        .await
        .unwrap_or_else(|e| BrowserIntegrationReport {
            host_name: "com.rustidm.native_host".to_string(),
            bridge_port: DEFAULT_BRIDGE_PORT,
            extension_directory: None,
            connected_browsers: Vec::new(),
            errors: vec![e.to_string()],
        });
    Json(report)
}

async fn register_browser_integration_handler(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<RegisterBrowserPayload>,
) -> Result<Json<BrowserIntegrationEntry>, (StatusCode, String)> {
    let dir = state.base_download_dir.clone();
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("rust_idm"));
    let browser = payload.browser.trim().to_string();
    let ext_id = payload.extension_id.trim().to_string();
    let entry = tokio::task::spawn_blocking(move || {
        register_browser_native_host(&browser, &[ext_id], &dir, &exe)
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    Ok(Json(entry))
}


// ============================================================================
// مسارات المزايا الجديدة المستلهمة من `Ezoo Download`:
// 1. فحص وإضافة التورنت والروابط المغناطيسية (`/api/v1/torrent/inspect` & `/api/v1/torrent/add`)
// 2. استخراج قوائم التشغيل الكاملة (`/api/v1/video/playlist`)
// 3. المعاينة الحية أثناء التحميل (`/api/v1/downloads/:task_id/preview`)
// 4. فحص التحديث الذاتي الموقّع بـ Ed25519 (`/api/v1/selfupdate/check`)
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TorrentInspectRequest {
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TorrentInspectResponse {
    pub is_magnet: bool,
    pub name: String,
    pub total_size: u64,
    pub files: Vec<crate::core::torrent::TorrentFileEntry>,
}

async fn inspect_torrent_handler(
    Json(payload): Json<TorrentInspectRequest>,
) -> Result<Json<TorrentInspectResponse>, (StatusCode, String)> {
    let src = payload.source.trim();
    if is_magnet(src) {
        let name = magnet_name(src).unwrap_or_else(|| "Magnet Torrent".to_string());
        return Ok(Json(TorrentInspectResponse {
            is_magnet: true,
            name,
            total_size: 0,
            files: Vec::new(),
        }));
    }
    let bytes = fs::read(src)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("تعذر قراءة ملف التورنت: {e}")))?;
    let info: TorrentInfo =
        parse_torrent(&bytes).map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    Ok(Json(TorrentInspectResponse {
        is_magnet: false,
        name: info.name,
        total_size: info.total_size,
        files: info.files,
    }))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartTorrentRequest {
    pub source: String,
    #[serde(default)]
    pub select_files: Option<String>,
    #[serde(default)]
    pub seed_minutes: u32,
    #[serde(default)]
    pub save_dir: Option<PathBuf>,
}

async fn start_torrent_download_handler(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<StartTorrentRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let source = payload.source.trim().to_string();
    if !is_torrent_source(&source) {
        return Err((
            StatusCode::BAD_REQUEST,
            "المصدر ليس رابطاً مغناطيسياً (magnet:?) ولا ملف .torrent صالحاً".to_string(),
        ));
    }

    let initial_name = if is_magnet(&source) {
        magnet_name(&source).unwrap_or_else(|| "Magnet_Download".to_string())
    } else {
        std::fs::read(&source)
            .ok()
            .and_then(|b| parse_torrent(&b).ok())
            .map(|i| i.name)
            .unwrap_or_else(|| "Torrent_Download".to_string())
    };
    let clean_name = sanitize_filename(&initial_name);
    let save_dir = payload
        .save_dir
        .unwrap_or_else(|| state.base_download_dir.join("Torrents"));
    let _ = fs::create_dir_all(&save_dir).await;

    let opts = TorrentAddOptions {
        dir: save_dir.clone(),
        select: payload.select_files.clone(),
        seed_minutes: payload.seed_minutes,
        download_limit_kb: state.speed_limiter.get_limit_kbps(),
        upload_limit_kb: 0,
        check_integrity: false,
    };

    let mut gid = state
        .torrent_engine
        .add(&source, &opts)
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, e.to_string()))?;

    let task_id = Uuid::new_v4().to_string();
    let meta = DownloadStateMeta {
        task_id: task_id.clone(),
        url: source.clone(),
        source_page_url: None,
        filename: clean_name,
        save_dir: save_dir.clone(),
        category: FileCategory::General,
        total_size: 0,
        resumable: true,
        etag: None,
        last_modified: None,
        custom_headers: HashMap::new(),
        expected_sha256: None,
        computed_sha256: None,
        status: DownloadStatus::Downloading,
        last_error: None,
        refresh_attempts: 0,
        segments: vec![FileSegment {
            id: 0,
            start_byte: 0,
            current_offset: 0,
            end_byte: 100,
            status: SegmentStatus::Downloading,
            split_count: 0,
            last_speed_bps: 0,
        }],
        created_at: Utc::now(),
        updated_at: Utc::now(),
        queue_name: "main".to_string(),
        mirror_urls: Vec::new(),
        active_mirror_index: 0,
        expected_md5: None,
        computed_md5: None,
        expected_crc32: None,
        computed_crc32: None,
        description: Some(format!("BitTorrent (GID: {gid})")),
        speed_history: Vec::new(),
        connection_logs: vec![ConnectionLogEntry {
            timestamp: Utc::now(),
            segment_id: Some(0),
            level: "INFO".to_string(),
            message: format!("بدأ تنزيل التورنت عبر aria2c JSON-RPC (GID: {gid})"),
        }],
        task_speed_limit_kbps: 0,
    };

    let pause_flag = Arc::new(AtomicBool::new(false));
    let cancel_flag = Arc::new(AtomicBool::new(false));
    let meta_arc = Arc::new(Mutex::new(meta));

    let job = Arc::new(DynamicDownloadJob {
        meta: Arc::clone(&meta_arc),
        pause_flag: Arc::clone(&pause_flag),
        cancel_flag: Arc::clone(&cancel_flag),
        resource_mutated_flag: Arc::new(AtomicBool::new(false)),
        expired_url_flag: Arc::new(AtomicBool::new(false)),
        speed_limiter: Arc::clone(&state.speed_limiter),
        client: state.client.clone(),
        max_connections: state.max_connections_per_file,
        min_split_size: MIN_SPLIT_SIZE,
        antivirus_cmd: None,
        uses_external_worker: true,
        runner_active: AtomicBool::new(true),
        schedule_paused: AtomicBool::new(false),
    });

    state
        .jobs
        .write()
        .await
        .insert(task_id.clone(), Arc::clone(&job));

    let engine_clone = Arc::clone(&state.torrent_engine);
    let job_clone = Arc::clone(&job);
    tokio::spawn(async move {
        loop {
            sleep(Duration::from_millis(600)).await;
            if cancel_flag.load(Ordering::Relaxed) {
                engine_clone.remove(&gid).await;
                let mut m = meta_arc.lock().await;
                m.status = DownloadStatus::Failed;
                m.last_error = Some("تم إلغاء التورنت بواسطة المستخدم".to_string());
                break;
            }
            if let Ok(st) = engine_clone.status(&gid).await {
                if let Some(next_gid) = st.followed_by {
                    gid = next_gid;
                    continue;
                }
                let mut m = meta_arc.lock().await;
                if let Some(real_name) = st.name {
                    let s = sanitize_filename(&real_name);
                    if !s.is_empty() {
                        m.filename = s;
                    }
                }
                if st.total > 0 {
                    m.total_size = st.total;
                }
                if let Some(seg0) = m.segments.first_mut() {
                    seg0.end_byte = st.total.max(1).saturating_sub(1);
                    seg0.current_offset = st.done;
                    seg0.last_speed_bps = st.speed;
                }
                m.push_speed_sample(st.speed, st.connections as usize);
                if st.status == "complete" || (st.total > 0 && st.done >= st.total && !st.seeding) {
                    m.status = DownloadStatus::Completed;
                    if let Some(seg0) = m.segments.first_mut() {
                        seg0.status = SegmentStatus::Completed;
                        seg0.last_speed_bps = 0;
                    }
                    let _ = save_meta_to_disk_atomic(&m, &m.meta_file_path()).await;
                    break;
                } else if st.status == "error" {
                    m.status = DownloadStatus::Failed;
                    m.last_error = st.error.or_else(|| Some("خطأ في تحميل التورنت".to_string()));
                    let _ = save_meta_to_disk_atomic(&m, &m.meta_file_path()).await;
                    break;
                }
            }
        }
        job_clone.runner_active.store(false, Ordering::SeqCst);
    });

    Ok(Json(serde_json::json!({
        "status": "accepted",
        "task_id": task_id,
        "mode": "bittorrent"
    })))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaylistProbeRequest {
    pub url: String,
    #[serde(default)]
    pub cookies: Option<String>,
}

async fn probe_playlist_handler(
    Json(payload): Json<PlaylistProbeRequest>,
) -> Result<Json<PlaylistInfo>, (StatusCode, String)> {
    let info = fetch_playlist_flat(&payload.url, payload.cookies.as_deref())
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    Ok(Json(info))
}

async fn preview_active_download_handler(
    State(state): State<Arc<AppState>>,
    AxumPath(task_id): AxumPath<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let job = state
        .jobs
        .read()
        .await
        .get(&task_id)
        .cloned()
        .ok_or_else(|| (StatusCode::NOT_FOUND, "المهمة غير موجودة".to_string()))?;

    let (part_path, final_path, filename, status) = {
        let m = job.meta.lock().await;
        (
            m.part_file_path(),
            m.final_file_path(),
            m.filename.clone(),
            m.status,
        )
    };

    let target_to_play = if status == DownloadStatus::Completed && final_path.exists() {
        final_path
    } else {
        part_path
    };

    let preview_path = tokio::task::spawn_blocking(move || {
        play_partial_download(&target_to_play, &filename)
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;

    Ok(Json(serde_json::json!({
        "status": "playing_preview",
        "task_id": task_id,
        "preview_path": preview_path
    })))
}

async fn check_signed_selfupdate_handler(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Option<SignedReleaseManifest>>, (StatusCode, String)> {
    let res = check_for_signed_update(&state.client)
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, e.to_string()))?;
    if let Some(ref manifest) = res {
        if std::env::var("STORMGET_AUTO_STAGE_UPDATE").as_deref() == Ok("1") {
            let stage_dir = std::env::temp_dir().join("StormGet-Updates");
            let _ = download_signed_installer(&state.client, manifest, &stage_dir).await;
        }
    }
    Ok(Json(res))
}


// ============================================================================
// مسارات مزايا `VeloxFetch` المتقدمة:
// 1. الخنق الذكي ووضع الألعاب (`/api/v1/smart-throttle` & `/api/v1/game-mode`)
// 2. ذكاء ما قبل التحميل (`/api/v1/intelligence/check`) وطبيب التحميل (`/api/v1/downloads/:task_id/doctor`)
// 3. دمج الشبكات المتعددة (`/api/v1/network-lanes`)
// 4. البحث الفوري عن بصمة الناشر (`/api/v1/downloads/:task_id/publisher-checksum`)
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateSmartThrottlePayload {
    pub mode: Option<SmartThrottleMode>,
    pub game_policy: Option<GamePolicy>,
    pub game_limit_kbps: Option<u64>,
    pub floor_kbps: Option<u64>,
    pub share_percent: Option<u8>,
    pub watched_apps: Option<Vec<String>>,
}

async fn get_smart_throttle_handler(
    State(state): State<Arc<AppState>>,
) -> Json<SmartThrottleStatus> {
    let mut st = state.smart_throttle.lock().await;
    let peak_kbps = state.peak_speed_bps.load(Ordering::Relaxed) / 1024;
    st.recalculate_effective_limit(peak_kbps, 0);
    Json(st.clone())
}

async fn set_smart_throttle_handler(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<UpdateSmartThrottlePayload>,
) -> Json<SmartThrottleStatus> {
    let mut st = state.smart_throttle.lock().await;
    if let Some(m) = payload.mode {
        st.mode = m;
    }
    if let Some(p) = payload.game_policy {
        st.game_policy = p;
    }
    if let Some(lim) = payload.game_limit_kbps {
        st.game_limit_kbps = lim.max(32);
    }
    if let Some(fl) = payload.floor_kbps {
        st.floor_kbps = fl.max(32);
    }
    if let Some(sh) = payload.share_percent {
        st.share_percent = sh.clamp(5, 90);
    }
    if let Some(apps) = payload.watched_apps {
        st.watched_apps = apps;
    }
    let peak_kbps = state.peak_speed_bps.load(Ordering::Relaxed) / 1024;
    let cap = st.recalculate_effective_limit(peak_kbps, 0);
    if cap > 0 {
        state.speed_limiter.set_limit_kbps(cap);
    } else {
        let cfg_lim = state.config.read().await.global_speed_limit_kbps;
        state.speed_limiter.set_limit_kbps(cfg_lim);
    }
    Json(st.clone())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameModeTogglePayload {
    pub active: bool,
    #[serde(default)]
    pub duration_minutes: Option<u32>,
}

async fn toggle_game_mode_handler(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<GameModeTogglePayload>,
) -> Json<SmartThrottleStatus> {
    let mut st = state.smart_throttle.lock().await;
    let peak_kbps = state.peak_speed_bps.load(Ordering::Relaxed) / 1024;
    st.set_game_mode(payload.active, payload.duration_minutes, peak_kbps);
    let cap = st.effective_speed_limit_kbps;
    if cap > 0 {
        state.speed_limiter.set_limit_kbps(cap);
    } else {
        let cfg_lim = state.config.read().await.global_speed_limit_kbps;
        state.speed_limiter.set_limit_kbps(cfg_lim);
    }
    Json(st.clone())
}

async fn check_link_intelligence_handler(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<ProbeUrlRequestPayload>,
) -> Result<Json<LinkIntelligenceReport>, (StatusCode, String)> {
    let mut headers = HeaderMap::new();
    if let Some(ref c) = payload.cookies {
        if let Ok(hv) = HeaderValue::from_str(c) {
            headers.insert(COOKIE, hv);
        }
    }
    if let Some(ref r) = payload.referer {
        if let Ok(hv) = HeaderValue::from_str(r) {
            headers.insert(REFERER, hv);
        }
    }
    if let Some(ref ua) = payload.user_agent {
        if let Ok(hv) = HeaderValue::from_str(ua) {
            headers.insert(USER_AGENT, hv);
        }
    }

    let (total_size, resumable, _filename, etag, last_modified) =
        DynamicDownloadJob::probe_url(&state.client, payload.url.trim(), &headers)
            .await
            .unwrap_or((0, false, String::from("download"), None, None));

    let existing_tasks = {
        let jobs = state.jobs.read().await;
        let mut list = Vec::with_capacity(jobs.len());
        for j in jobs.values() {
            list.push(j.meta.lock().await.clone());
        }
        list
    };

    let report = evaluate_link_intelligence(
        payload.url.trim(),
        payload.url.trim(),
        None,
        total_size,
        resumable,
        etag.as_deref(),
        last_modified.as_deref(),
        &existing_tasks,
    );
    Ok(Json(report))
}

async fn diagnose_download_doctor_handler(
    State(state): State<Arc<AppState>>,
    AxumPath(task_id): AxumPath<String>,
) -> Result<Json<DoctorDiagnosis>, (StatusCode, String)> {
    let job = state
        .jobs
        .read()
        .await
        .get(&task_id)
        .cloned()
        .ok_or_else(|| (StatusCode::NOT_FOUND, "المهمة غير موجودة".to_string()))?;
    let meta = job.meta.lock().await;
    Ok(Json(diagnose_task_with_doctor(&meta)))
}

async fn get_network_lanes_handler(State(state): State<Arc<AppState>>) -> Json<Vec<LaneStatus>> {
    Json(state.lane_pool.statuses())
}

async fn set_network_lanes_handler(
    State(state): State<Arc<AppState>>,
    Json(routes): Json<Vec<NetworkRouteSpec>>,
) -> Result<Json<Vec<LaneStatus>>, (StatusCode, String)> {
    state
        .lane_pool
        .configure_routes(routes)
        .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    Ok(Json(state.lane_pool.statuses()))
}

async fn lookup_publisher_checksum_handler(
    State(state): State<Arc<AppState>>,
    AxumPath(task_id): AxumPath<String>,
) -> Result<Json<Option<PublisherChecksum>>, (StatusCode, String)> {
    let job = state
        .jobs
        .read()
        .await
        .get(&task_id)
        .cloned()
        .ok_or_else(|| (StatusCode::NOT_FOUND, "المهمة غير موجودة".to_string()))?;
    let url = job.meta.lock().await.url.clone();
    let found = discover_publisher_checksum(&state.client, &url, false).await;
    Ok(Json(found))
}

pub fn build_daemon_router(state: Arc<AppState>) -> Router {
    let security_state = Arc::clone(&state);
    Router::new()
        .route("/api/v1/health", get(health_handler))
        .route("/api/v1/tools/status", get(tools_status_handler))
        .route("/api/v1/tools/update", post(tools_update_handler))
        .route("/api/v1/intercept", post(intercept_download_handler))
        .route("/api/v1/pending-intercepts", get(pending_intercepts_handler))
        .route(
            "/api/v1/pending-intercepts/:id",
            post(decide_pending_intercept_handler),
        )
        .route("/api/v1/enqueue", post(enqueue_download_handler))
        .route("/api/v1/refresh-url", post(refresh_url_handler))
        .route("/api/v1/downloads", get(list_downloads_handler))
        .route(
            "/api/v1/downloads/completed",
            post(delete_completed_downloads_handler),
        )
        .route(
            "/api/v1/downloads/:task_id",
            delete(delete_download_handler),
        )
        .route(
            "/api/v1/downloads/:task_id/redownload",
            post(redownload_task_handler),
        )
        .route("/api/v1/control", post(control_task_handler))
        .route("/api/v1/crawl", post(crawl_site_handler))
        .route(
            "/api/v1/scheduler/notifications",
            get(scheduler_notifications_handler),
        )
        .route(
            "/api/v1/scheduler/:queue_name",
            get(get_queue_schedule_handler).post(set_queue_schedule_handler),
        )
        .route("/api/v1/video/probe", post(probe_video_qualities_handler))
        .route("/api/v1/video/download", post(start_video_download_handler))
        .route("/api/v1/batch", post(batch_enqueue_handler))
        .route("/api/v1/zip-preview", post(zip_preview_handler))
        .route(
            "/api/v1/queues",
            get(list_queues_handler).post(upsert_queue_handler),
        )
        .route("/api/v1/queues/:queue_name", delete(delete_queue_handler))
        .route(
            "/api/v1/config",
            get(get_config_handler).post(set_config_handler),
        )
        .route("/api/v1/stats", get(stats_handler))
        .route("/api/v1/export", get(export_tasks_handler))
        .route("/api/v1/import", post(import_tasks_handler))
        .route("/api/v1/downloads/:task_id/logs", get(task_logs_handler))
        .route(
            "/api/v1/downloads/:task_id/reverify",
            post(reverify_checksum_handler),
        )
        .route(
            "/api/v1/downloads/:task_id/meta",
            post(update_task_meta_handler),
        )
        .route("/api/v1/probe-url", post(probe_url_metadata_handler))
        .route(
            "/api/v1/browser-integration",
            get(browser_integration_status_handler),
        )
        .route(
            "/api/v1/browser-integration/register",
            post(register_browser_integration_handler),
        )
        .route("/api/v1/torrent/inspect", post(inspect_torrent_handler))
        .route("/api/v1/torrent/add", post(start_torrent_download_handler))
        .route("/api/v1/video/playlist", post(probe_playlist_handler))
        .route(
            "/api/v1/downloads/:task_id/preview",
            post(preview_active_download_handler),
        )
        .route(
            "/api/v1/selfupdate/check",
            get(check_signed_selfupdate_handler),
        )
        .route(
            "/api/v1/smart-throttle",
            get(get_smart_throttle_handler).post(set_smart_throttle_handler),
        )
        .route("/api/v1/game-mode", post(toggle_game_mode_handler))
        .route(
            "/api/v1/intelligence/check",
            post(check_link_intelligence_handler),
        )
        .route(
            "/api/v1/downloads/:task_id/doctor",
            get(diagnose_download_doctor_handler),
        )
        .route(
            "/api/v1/network-lanes",
            get(get_network_lanes_handler).post(set_network_lanes_handler),
        )
        .route(
            "/api/v1/downloads/:task_id/publisher-checksum",
            post(lookup_publisher_checksum_handler),
        )
        .layer(DefaultBodyLimit::max(4 * 1024 * 1024))
        .layer(middleware::from_fn_with_state(
            security_state,
            enforce_local_ipc_security,
        ))
        .with_state(state)
}
