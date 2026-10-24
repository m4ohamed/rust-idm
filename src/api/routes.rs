//! مسارات خادم التحكم المحلي (Axum IPC Endpoints)، حالة المحرك (`AppState`)، والجدولة الزمنية للطوابير

use crate::{
    core::{
        limiter::SpeedLimiter,
        segmenter::DynamicDownloadJob,
        storage::{
            compute_file_all_digests, sanitize_filename, save_meta_to_disk_atomic, sync_parent_dir,
            ConnectionLogEntry, DownloadStateMeta, DownloadStatus, FileCategory, FileSegment,
            SegmentStatus, MIN_SPLIT_SIZE,
        },
    },
    inspector::{
        crawler::{BoundedSiteCrawler, CrawlConfig, CrawlReport},
        zip::{
            BatchExpandRequest, BatchExpandResponse, DownloadListImporter, RemoteZipInspector,
            RemoteZipPreviewResponse, WildcardPatternExpander,
        },
    },
    media::{dash::DashMpdParser, hls::MediaStreamEngine},
};
use anyhow::{anyhow, Context, Result};
use axum::{
    extract::{Path as AxumPath, State},
    http::StatusCode,
    routing::{delete, get, post},
    Json, Router,
};
use chrono::{Local, NaiveDateTime, NaiveTime, Utc};
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
use tower_http::cors::{Any, CorsLayer};
use url::Url;
use uuid::Uuid;

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
    pub auth_header_or_cookie: String,
    pub enabled: bool,
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
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            download_dir: crate::default_download_directory(),
            max_connections_per_file: 8,
            global_speed_limit_kbps: 0,
            min_split_size_kb: 512,
            proxy_url: None,
            custom_user_agent: "Mozilla/5.0 (Windows NT 10.0; Win64; x64) RustIDM/2.0".to_string(),
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
}

impl QueueConfig {
    fn schedule(&self) -> QueueSchedule {
        QueueSchedule {
            mode: self.schedule_mode,
            start_time: self.start_time.clone(),
            stop_time: self.stop_time.clone(),
            start_at: self.start_at.clone(),
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
}

pub struct AppState {
    pub client: Client,
    pub base_download_dir: PathBuf,
    pub max_connections_per_file: usize,
    pub speed_limiter: Arc<SpeedLimiter>,
    pub config: RwLock<AppConfig>,
    pub jobs: RwLock<HashMap<String, Arc<DynamicDownloadJob>>>,
    pub detected_videos: RwLock<Vec<DetectedVideoPayload>>,
    pub queues: RwLock<HashMap<String, QueueConfig>>,
    pub recent_intercepts: Mutex<HashMap<String, Instant>>,
    pub queue_semaphores: RwLock<HashMap<String, Arc<Semaphore>>>,
    pub post_action_fired: Mutex<HashSet<String>>,
    pub scheduler_notifications: Mutex<VecDeque<String>>,
    pub peak_speed_bps: AtomicU64,
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
    pub protocol: String, // "hls" | "dash" | "ytdlp" | "direct"
    pub format_id: Option<String>,
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
    pub page_url: Option<String>,
    pub title: Option<String>,
    pub quality_label: Option<String>,
    pub container: Option<String>,
    pub protocol: String, // "hls" | "dash" | "ytdlp" | "direct"
    pub format_id: Option<String>,
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
        let client = Client::builder()
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) RustIDM/1.1")
            .connect_timeout(Duration::from_secs(15))
            .build()
            .expect("فشل إنشاء عميل HTTP");

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

        Arc::new(Self {
            client,
            base_download_dir,
            max_connections_per_file,
            speed_limiter: SpeedLimiter::new(speed_limit_kbps),
            config: RwLock::new(default_cfg),
            jobs: RwLock::new(HashMap::new()),
            detected_videos: RwLock::new(Vec::new()),
            queues: RwLock::new(default_queues),
            recent_intercepts: Mutex::new(HashMap::new()),
            queue_semaphores: RwLock::new(queue_semaphores),
            post_action_fired: Mutex::new(HashSet::new()),
            scheduler_notifications: Mutex::new(VecDeque::new()),
            peak_speed_bps: AtomicU64::new(0),
        })
    }

    async fn spawn_download_job(self: &Arc<Self>, job: Arc<DynamicDownloadJob>) {
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
                if let Err(error) = job.execute().await {
                    let status = job.meta.lock().await.status;
                    if status != DownloadStatus::Paused && status != DownloadStatus::Failed {
                        eprintln!("Download job failed: {error:#}");
                    }
                }
            }
            drop(permit);
            job.runner_active.store(false, Ordering::SeqCst);
        });
    }

    /// استعادة جميع التنزيلات المتوقفة أو غير المكتملة (.idm.json) من القرص عند إعادة تشغيل التطبيق
    pub async fn restore_persisted_tasks_on_startup(self: &Arc<Self>) -> Result<usize> {
        let config_path = self.base_download_dir.join("config.json");
        if let Ok(bytes) = fs::read(&config_path).await {
            if let Ok(saved_cfg) = serde_json::from_slice::<AppConfig>(&bytes) {
                self.speed_limiter
                    .set_limit_kbps(saved_cfg.global_speed_limit_kbps);
                *self.config.write().await = saved_cfg;
            }
        }

        let queues_path = self.base_download_dir.join("queues.json");
        if let Ok(bytes) = fs::read(&queues_path).await {
            if let Ok(saved_queues) = serde_json::from_slice::<HashMap<String, QueueConfig>>(&bytes)
            {
                let mut q_guard = self.queues.write().await;
                let mut sem_guard = self.queue_semaphores.write().await;
                for (name, q_cfg) in saved_queues {
                    sem_guard
                        .entry(name.clone())
                        .or_insert_with(|| Arc::new(Semaphore::new(q_cfg.max_concurrent_downloads.max(1))));
                    q_guard.insert(name, q_cfg);
                }
            }
        }

        let schedule_path = self.base_download_dir.join("scheduler.json");
        match fs::read(&schedule_path).await {
            Ok(bytes) => {
                let saved: HashMap<String, QueueSchedule> = serde_json::from_slice(&bytes)
                    .context("Failed to parse saved scheduler configuration")?;
                let mut queues = self.queues.write().await;
                for (name, schedule) in saved {
                    if let Some(queue) = queues.get_mut(&name) {
                        queue.schedule_mode = schedule.mode;
                        queue.start_time = schedule.start_time;
                        queue.stop_time = schedule.stop_time;
                        queue.start_at = schedule.start_at;
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error).context("Failed to read scheduler configuration"),
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
                    if let Ok(bytes) = fs::read(&path).await {
                        if let Ok(mut meta) = serde_json::from_slice::<DownloadStateMeta>(&bytes) {
                            if matches!(
                                meta.status,
                                DownloadStatus::Downloading
                                    | DownloadStatus::Assembling
                                    | DownloadStatus::VerifyingChecksum
                            ) {
                                meta.status = DownloadStatus::Paused;
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
                    }
                }
            }
        }
        Ok(restored)
    }

    pub fn start_scheduler(self: &Arc<Self>) {
        let state = Arc::clone(self);
        tokio::spawn(async move {
            loop {
                if let Err(error) = apply_scheduled_queues(&state).await {
                    eprintln!("Scheduler tick failed: {error:#}");
                }
                sleep(Duration::from_secs(1)).await;
            }
        });
    }

    /// التحقق من شروط الأمان الصارمة قبل تنفيذ إجراء ما بعد اكتمال الطابور (مثل إغلاق التطبيق أو إيقاف الجهاز)
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
        // منع تكرار تسجيل نفس رابط التحميل خلال 5 ثوانٍ (Deduplication)
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

        let (total_size, resumable, probed_name, etag, last_modified) =
            DynamicDownloadJob::probe_url(&self.client, &payload.url, &header_map).await?;

        let final_filename = payload
            .filename
            .filter(|n| !n.trim().is_empty())
            .unwrap_or(probed_name);

        let category = FileCategory::from_filename(&final_filename);
        let save_dir = self.base_download_dir.join(category.folder_name());
        let queue_name = payload.queue_name.unwrap_or_else(|| "main".to_string());
        let queue = self
            .queues
            .read()
            .await
            .get(&queue_name)
            .map(QueueConfig::schedule)
            .ok_or_else(|| anyhow!("Unknown download queue: {queue_name}"))?;
        let schedule = queue;
        let should_start = scheduled_queue_should_run(&schedule, Local::now().naive_local())
            .context("Invalid schedule configured for download queue")?
            .unwrap_or(true);

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
            status: if should_start {
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
                message: format!("تمت جدولة المهمة في الطابور '{}' (الحجم: {} بايت)", queue_name, total_size),
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
            let start_at = NaiveDateTime::parse_from_str(start_at, "%Y-%m-%d %H:%M")
                .map_err(|error| anyhow!("Invalid one-time schedule '{start_at}': {error}"))?;
            Ok(Some(now >= start_at))
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
            let active = if start < stop {
                current >= start && current < stop
            } else {
                current >= start || current < stop
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
                    if !cmd_str.trim().is_empty() {
                        let parts: Vec<&str> = cmd_str.split_whitespace().collect();
                        if let Some((prog, args)) = parts.split_first() {
                            let _ = Command::new(prog).args(args).spawn();
                        }
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
// 11. نقاط نهاية خادم الاتصال المحلي (Axum IPC Endpoints)
// ============================================================================

async fn health_handler(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "online",
        "engine": "RustIDM",
        "version": "1.1.0",
        "speed_limit_kbps": state.speed_limiter.get_limit_kbps(),
        "capabilities": [
            "race_free_in_half_dynamic_segmentation",
            "range_ignore_and_mutation_guard",
            "expired_url_refresh_providers",
            "chrome_native_messaging_host",
            "hls_aes128_cbc_and_dash_mpd",
            "bounded_bfs_site_crawler"
        ]
    }))
}

async fn intercept_download_handler(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<InterceptDownloadPayload>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let task_id = state
        .enqueue_download(payload)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;

    Ok(Json(serde_json::json!({
        "status": "accepted",
        "task_id": task_id
    })))
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
        // إرجاع نسخة محمية خالية من الكوكيز والترويسات السرية
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

async fn delete_download_handler(
    State(state): State<Arc<AppState>>,
    AxumPath(task_id): AxumPath<String>,
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
        )
    {
        return Err((
            StatusCode::CONFLICT,
            "Pause the download before removing its task record".to_string(),
        ));
    }

    let meta_path = meta.meta_file_path();
    drop(meta);
    match fs::remove_file(&meta_path).await {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                format!(
                    "Cannot remove task metadata '{}': {error}",
                    meta_path.display()
                ),
            ));
        }
    }
    jobs.remove(&task_id);

    Ok(Json(serde_json::json!({
        "status": "deleted",
        "task_id": task_id,
        "file_preserved": true
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
        previous
    };

    if let Err(error) = persist_queue_schedules(&state).await {
        if let Some(queue) = state.queues.write().await.get_mut(&queue_name) {
            queue.schedule_mode = previous_schedule.mode;
            queue.start_time = previous_schedule.start_time;
            queue.stop_time = previous_schedule.stop_time;
            queue.start_at = previous_schedule.start_at;
        }
        return Err((StatusCode::INTERNAL_SERVER_ERROR, error.to_string()));
    }
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

    // 1. إذا كان الرابط قائمة بث HLS (.m3u8)
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
                qualities.push(VideoQualityOption {
                    id: format!("hls_{}", idx),
                    label: format!("MP4 / HLS {} ({} kbps)", height_tag, kbps),
                    resolution: Some(res_str),
                    container: "ts".to_string(),
                    bandwidth_bps: Some(v.bandwidth),
                    estimated_size_bytes: None,
                    download_url: v.uri,
                    protocol: "hls".to_string(),
                    format_id: None,
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
                protocol: "hls".to_string(),
                format_id: None,
            });
        }

        return Ok(Json(ProbeVideoResponse {
            title: clean_title,
            drm_protected: false,
            drm_reason: None,
            qualities,
        }));
    }

    // 2. إذا كان الرابط قائمة MPEG-DASH (.mpd)
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
            qualities.push(VideoQualityOption {
                id: format!("dash_{}", r.id),
                label: format!("DASH {} ({} kbps)", height_label, r.bandwidth / 1000),
                resolution: res,
                container: "mp4".to_string(),
                bandwidth_bps: Some(r.bandwidth),
                estimated_size_bytes: None,
                download_url: payload.url.clone(),
                protocol: "dash".to_string(),
                format_id: Some(r.id),
            });
        }

        return Ok(Json(ProbeVideoResponse {
            title: clean_title,
            drm_protected: false,
            drm_reason: None,
            qualities,
        }));
    }

    // 3. إذا كان صفحة يوتيوب أو موقع فيديو مدعوم عبر yt-dlp
    let target_page = payload.page_url.as_ref().unwrap_or(&payload.url);
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
        // محاولة استخراج الجودات الحقيقية عبر yt-dlp -J
        let mut cmd = TokioCommand::new("yt-dlp");
        cmd.kill_on_drop(true)
            .arg("-J")
            .arg("--no-playlist")
            .arg("--no-warnings")
            .arg("--remote-components")
            .arg("ejs:github")
            .arg("--js-runtimes")
            .arg("node");

        let _cookie_guard = if let Some(ref c) = payload.cookies {
            if !c.trim().is_empty() {
                if let Ok(guard) = crate::media::create_temp_netscape_cookie_file(target_page, c) {
                    cmd.arg("--cookies").arg(&guard.0);
                    Some(guard)
                } else {
                    cmd.arg("--add-header").arg(format!("Cookie: {}", c));
                    None
                }
            } else {
                None
            }
        } else {
            None
        };
        cmd.arg(target_page);

        if let Ok(Ok(output)) = timeout(Duration::from_secs(12), cmd.output()).await {
            if output.status.success() {
                if let Ok(info) = serde_json::from_slice::<serde_json::Value>(&output.stdout) {
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
                    let mut seen_heights = HashSet::new();

                    if let Some(formats) = info.get("formats").and_then(|f| f.as_array()) {
                        for fmt in formats.iter().rev() {
                            let vcodec =
                                fmt.get("vcodec").and_then(|v| v.as_str()).unwrap_or("none");
                            if vcodec == "none" {
                                continue;
                            }
                            let height = fmt.get("height").and_then(|h| h.as_u64()).unwrap_or(0);
                            if height < 144 || !seen_heights.insert(height) {
                                continue;
                            }
                            let ext = fmt.get("ext").and_then(|e| e.as_str()).unwrap_or("mp4");
                            let filesize = fmt
                                .get("filesize")
                                .and_then(|s| s.as_u64())
                                .or_else(|| fmt.get("filesize_approx").and_then(|s| s.as_u64()));
                            let fps = fmt.get("fps").and_then(|f| f.as_u64()).unwrap_or(30);

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
                                protocol: "ytdlp".to_string(),
                                format_id: Some(selector),
                            });
                        }
                    }

                    // إضافة خيار الصوت فقط (M4A / MP3)
                    qualities.push(VideoQualityOption {
                        id: "ytdlp_audio".to_string(),
                        label: "Audio Only (أعلى جودة صوت M4A)".to_string(),
                        resolution: Some("Audio".to_string()),
                        container: "m4a".to_string(),
                        bandwidth_bps: None,
                        estimated_size_bytes: None,
                        download_url: target_page.clone(),
                        protocol: "ytdlp".to_string(),
                        format_id: Some("bestaudio/best".to_string()),
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
        }

        // خيارات قياسية عبر yt-dlp في حال عدم استجابة الاستعلام السريع
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
                protocol: "ytdlp".to_string(),
                format_id: Some(format!(
                    "bestvideo[height<={}]+bestaudio/best[height<={}]/best",
                    h, h
                )),
            });
        }

        return Ok(Json(ProbeVideoResponse {
            title: clean_title,
            drm_protected: false,
            drm_reason: None,
            qualities,
        }));
    }

    // 4. رابط فيديو مباشر (MP4 / WebM / MKV)
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
            protocol: "direct".to_string(),
            format_id: None,
        }],
    }))
}

/// بدء تحميل الفيديو بالجودة التي اختارها المستخدم وإدراجه في قائمة مهام RustIDM
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

    // إذا كان ملف فيديو مباشر (MP4/WebM) نستخدم محرك التقسيم الديناميكي متعدد الاتصالات مباشرة
    if payload.protocol == "direct" {
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

    // إنشاء مهمة مرئية في واجهة RustIDM لبث HLS أو yt-dlp
    let save_dir = state
        .base_download_dir
        .join(FileCategory::Video.folder_name());
    let _ = fs::create_dir_all(&save_dir).await;
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

    let meta = DownloadStateMeta {
        task_id: task_id.clone(),
        url: payload.url.clone(),
        source_page_url: payload.page_url.clone(),
        filename: filename.clone(),
        save_dir: save_dir.clone(),
        category: FileCategory::Video,
        total_size: 0,
        resumable: false,
        etag: None,
        last_modified: None,
        custom_headers,
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
    let format_id = payload.format_id.clone();
    let cookies = payload.cookies.clone();
    let referer = payload.referer.clone();
    let user_agent = payload.user_agent.clone();

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
        let res = if protocol == "hls" {
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
            MediaStreamEngine::download_via_ytdlp_with_format(
                &target_url,
                &output_path,
                format_id.as_deref(),
                cookies.as_deref(),
                referer.as_deref(),
                user_agent.as_deref(),
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
                m.status = DownloadStatus::Completed;
                m.last_error = None;
            }
            Err(e) => {
                m.status = DownloadStatus::Failed;
                m.last_error = Some(e.to_string());
                if let Some(s0) = m.segments.first_mut() {
                    s0.status = SegmentStatus::Failed;
                }
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

/// معاينة محتويات أرشيف ZIP عن بُعد عبر HTTP Range دون تحميل الملف كاملاً
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

/// توسيع نمط روابط دفعي (Batch Wildcard Pattern) وجدولة جميع الروابط المتولدة في الطابور المختار
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

/// استرجاع كافة طوابير التحميل المسجلة مع إعداداتها وجداولها الزمنية
async fn list_queues_handler(State(state): State<Arc<AppState>>) -> Json<Vec<QueueConfig>> {
    let queues = state.queues.read().await;
    let mut list: Vec<QueueConfig> = queues.values().cloned().collect();
    list.sort_by(|a, b| a.name.cmp(&b.name));
    Json(list)
}

/// إنشاء أو تحديث طابور تحميل مخصص مع حفظه ذرياً في queues.json
async fn upsert_queue_handler(
    State(state): State<Arc<AppState>>,
    Json(mut cfg): Json<QueueConfig>,
) -> Result<Json<QueueConfig>, (StatusCode, String)> {
    cfg.name = cfg.name.trim().to_string();
    if cfg.name.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "اسم الطابور لا يمكن أن يكون فارغاً".to_string()));
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

/// حذف طابور مخصص ونقل مهامه إلى الطابور الرئيسي (main)
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

    let jobs: Vec<Arc<DynamicDownloadJob>> =
        state.jobs.read().await.values().cloned().collect();
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

/// قراءة إعدادات المحرك الشاملة (AppConfig) مع حجب كلمات المرور السرية
async fn get_config_handler(State(state): State<Arc<AppState>>) -> Json<AppConfig> {
    let mut cfg = state.config.read().await.clone();
    for login in &mut cfg.site_logins {
        if !login.auth_header_or_cookie.is_empty() {
            login.auth_header_or_cookie = "[REDACTED_SECRET]".to_string();
        }
    }
    Json(cfg)
}

/// تحديث إعدادات المحرك الشاملة وحفظها ذرياً في config.json
async fn set_config_handler(
    State(state): State<Arc<AppState>>,
    Json(mut new_cfg): Json<AppConfig>,
) -> Result<Json<AppConfig>, (StatusCode, String)> {
    new_cfg.max_connections_per_file = new_cfg.max_connections_per_file.clamp(1, 32);
    new_cfg.min_split_size_kb = new_cfg.min_split_size_kb.max(64);
    state
        .speed_limiter
        .set_limit_kbps(new_cfg.global_speed_limit_kbps);

    {
        let mut current = state.config.write().await;
        // الحفاظ على أسرار تسجيل الدخول إذا كانت القيمة المرسلة محجوبة
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

/// إحصائيات المحرك الحية (Engine Telemetry & Aggregate Transfer Metrics)
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

/// استرجاع سجل الاتصالات التشخيصي ومنحنى السرعة لمهمة محددة
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

/// إعادة فحص البصمات الرقمية (SHA-256 / MD5 / CRC32) لملف مكتمل على القرص عند الطلب
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

/// استيراد قائمة مهام من نص EF2 أو TXT وجدولتها في المحرك
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

/// تصدير قائمة المهام بصيغة نصية (.ef2 / .txt) أو استيرادها
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


pub fn build_daemon_router(state: Arc<AppState>) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        .route("/api/v1/health", get(health_handler))
        .route("/api/v1/intercept", post(intercept_download_handler))
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
        .layer(cors)
        .with_state(state)
}
