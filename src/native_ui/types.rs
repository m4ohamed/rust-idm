//! أنواع الحالة والأوامر والأحداث الموحدة لواجهة StormGet (RustIDM سابقاً)

use crate::{
    AppConfig, BatchExpandRequest, BatchExpandResponse, CrawlConfig, CrawlReport,
    DoctorDiagnosis, DownloadStateMeta, EngineStatistics, FileCategory, GameModeTogglePayload,
    ImportTasksPayload, InterceptDownloadPayload, LinkIntelligenceReport, PendingIntercept,
    PlaylistInfo, PlaylistProbeRequest, ProbeUrlRequestPayload, ProbeUrlResponsePayload,
    QueueConfig, QueueSchedule, RefreshUrlPayload, RemoteZipPreviewResponse, SmartThrottleStatus,
    StartTorrentRequest, ToolsStatusReport, UpdateTaskMetaPayload, ZipPreviewRequestPayload,
};
use serde::{Deserialize, Serialize};
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemeMode {
    Dark,
    Light,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AppLanguage {
    Arabic,
    English,
}

/// كثافة عرض صفوف جدول التنزيلات (قابلة للتخصيص والحفظ عبر قائمة View ونافذة الإعدادات)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RowDensity {
    Compact,
    Comfortable,
    Spacious,
}

impl RowDensity {
    pub fn row_height(self) -> f32 {
        match self {
            RowDensity::Compact => 22.0,
            RowDensity::Comfortable => 27.0,
            RowDensity::Spacious => 33.0,
        }
    }
}

/// معرفات أعمدة جدول التنزيلات (قابلة للإظهار والإخفاء وإعادة الترتيب وحفظ العرض)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TableColumnId {
    Status,
    Filename,
    TotalSize,
    Downloaded,
    Progress,
    Speed,
    TimeLeft,
    Connections,
    Category,
    CreatedAt,
    SaveFolder,
    Queue,
}

impl TableColumnId {
    pub fn all_columns() -> Vec<Self> {
        vec![
            Self::Filename,
            Self::Status,
            Self::TotalSize,
            Self::Downloaded,
            Self::Progress,
            Self::Speed,
            Self::TimeLeft,
            Self::Connections,
            Self::Category,
            Self::CreatedAt,
            Self::SaveFolder,
            Self::Queue,
        ]
    }

    pub fn default_visible_columns() -> Vec<Self> {
        vec![
            Self::Filename,
            Self::Status,
            Self::TotalSize,
            Self::Downloaded,
            Self::Progress,
            Self::Speed,
            Self::TimeLeft,
            Self::Connections,
            Self::Category,
            Self::CreatedAt,
        ]
    }

    pub fn default_width(self) -> f32 {
        match self {
            Self::Filename => 240.0,
            Self::Status => 125.0,
            Self::TotalSize => 92.0,
            Self::Downloaded => 92.0,
            Self::Progress => 135.0,
            Self::Speed => 105.0,
            Self::TimeLeft => 92.0,
            Self::Connections => 66.0,
            Self::Category => 90.0,
            Self::CreatedAt => 115.0,
            Self::SaveFolder => 165.0,
            Self::Queue => 64.0,
        }
    }

    pub fn min_width(self) -> f32 {
        match self {
            Self::Filename => 140.0,
            Self::Status => 95.0,
            Self::Progress => 100.0,
            Self::Connections | Self::Queue => 48.0,
            _ => 70.0,
        }
    }
}

pub enum UiCommand {
    AddDownload {
        payload: InterceptDownloadPayload,
        save_dir: Option<std::path::PathBuf>,
        start_immediately: bool,
    },
    ProbeUrl(ProbeUrlRequestPayload),
    UpdateTaskMeta {
        task_id: String,
        payload: UpdateTaskMetaPayload,
    },
    ResolvePendingIntercept {
        id: String,
        action: &'static str,
        save_dir: Option<std::path::PathBuf>,
    },
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
    SetSchedule {
        queue_name: String,
        schedule: QueueSchedule,
    },
    DeleteCompleted,
    RemoveDownload(String),
    Redownload(String),
    DeleteWithFile(String),
    Control {
        task_id: String,
        action: &'static str,
    },
    SetSpeedLimit(u64),
    UpdateTools,
    ToggleGameMode(GameModeTogglePayload),
    PreviewTask(String),
    DiagnoseTask(String),
    CheckIntelligence(ProbeUrlRequestPayload),
    AddTorrent(StartTorrentRequest),
    ProbePlaylist(PlaylistProbeRequest),
}

pub enum UiEvent {
    Downloads(Vec<DownloadStateMeta>),
    PendingIntercepts(Vec<PendingIntercept>),
    Queues(Vec<QueueConfig>),
    Config(AppConfig),
    Stats(EngineStatistics),
    ToolsStatus(ToolsStatusReport),
    ProbeResult(Result<ProbeUrlResponsePayload, String>),
    BatchResult(Result<BatchExpandResponse, String>),
    CrawlResult(Result<CrawlReport, String>),
    ZipPreviewResult(Result<RemoteZipPreviewResponse, String>),
    ExportedText(Result<String, String>),
    Schedule {
        queue_name: String,
        result: Result<QueueSchedule, String>,
    },
    CompletedNotice(String),
    Status(Result<String, String>),
    EngineConnection(Result<(), String>),
    SmartThrottleUpdated(SmartThrottleStatus),
    DoctorDiagnosisResult(Result<DoctorDiagnosis, String>),
    IntelligenceResult(Result<LinkIntelligenceReport, String>),
    PlaylistResult(Result<PlaylistInfo, String>),
}

/// مرشحات العرض الجانبية والسريعة
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DownloadFilter {
    All,
    Active,
    Queued,
    Paused,
    Unfinished,
    Finished,
    Failed,
    Category(FileCategory),
    Queue(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SortColumn {
    Filename,
    Queue,
    Size,
    Downloaded,
    Progress,
    Status,
    TimeLeft,
    Speed,
    Connections,
    UpdatedAt,
    CreatedAt,
    Category,
    SaveFolder,
}

/// أقسام نافذة الإعدادات الـ 8 المطلوبة في المرحلة العاشرة
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OptionsTab {
    General,
    Appearance,
    Downloads,
    Connections,
    BrowserIntegration,
    Media,
    Notifications,
    Advanced,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BottomInspectorTab {
    SegmentMap,
    SpeedGraph,
    ConnectionLogs,
    ChecksumAudit,
}

/// تبويبات نافذة تفاصيل التنزيل الـ 5 المطلوبة في المرحلة الثامنة + خيارات السرعة وما بعد الاكتمال
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DialogTab {
    General,
    Progress,
    Connection,
    Logs,
    Integrity,
    SpeedLimiter,
    OptionsOnCompletion,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShutdownAction {
    Shutdown,
    Restart,
    Sleep,
}

impl ShutdownAction {
    pub fn label(&self) -> &'static str {
        match self {
            ShutdownAction::Shutdown => "ﺮﺗﻮﻴﺒﻤﻜﻟﺍ ﻕﻼﻏﺇ",
            ShutdownAction::Restart => "ﻞﻴﻐﺸﺘﻟﺍ ﺓﺩﺎﻋﺇ",
            ShutdownAction::Sleep => "ﻥﻮﻜﺴﻟﺍ ﻊﺿﻭ",
        }
    }
}

/// أيقونات شريط الأدوات الرئيسي الموحدة (المرحلة الرابعة)
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ToolbarIcon {
    AddUrl,
    PasteClipboard,
    ImportList,
    StartResume,
    Pause,
    Cancel,
    RetryFailed,
    Delete,
    DeleteCompleted,
    Scheduler,
    Options,
    StartQueue,
    StopQueue,
    StopAll,
    Grabber,
    Batch,
    ZipPreview,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TreeIconKind {
    FolderYellow,
    FolderFinished,
    FolderActive,
    FolderQueued,
    FolderPaused,
    FolderFailed,
    Compressed,
    Document,
    Music,
    Program,
    Video,
    Grabber,
}

/// مستوى إشعار Toast القصير (المرحلة 13)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    Info,
    Success,
    Warning,
    Error,
}

/// إشعار منبثق قصير غير حاجب (Toast Notification)
#[derive(Debug, Clone)]
pub struct ToastNotification {
    pub id: u64,
    pub kind: ToastKind,
    pub title: String,
    pub message: String,
    pub task_id: Option<String>,
    pub created_at: Instant,
    pub duration_secs: u64,
}

/// حوار التأكيد قبل العمليات المدمرة (المرحلة 3.2 و 6.3 و 10.8 و 13)
#[derive(Debug, Clone)]
pub enum PendingConfirmation {
    /// حذف سجل التنزيل فقط مع بقاء الملف النهائي على القرص
    RemoveRecordsOnly {
        task_ids: Vec<String>,
        filenames: Vec<String>,
    },
    /// حذف سجل التنزيل مع حذف الملف الفعلي نهائياً من القرص
    DeleteFilesFromDisk {
        task_ids: Vec<String>,
        filenames: Vec<String>,
    },
    /// إلغاء تنزيل نشط
    CancelActiveDownloads {
        task_ids: Vec<String>,
        filenames: Vec<String>,
    },
    /// مسح جميع سجلات التنزيلات المكتملة
    ClearCompletedRecords {
        count: usize,
    },
    /// إعادة ضبط إعدادات البرنامج إلى القيم الافتراضية
    ResetAppConfigToDefaults,
    /// إعادة ضبط تخطيط الواجهة والأعمدة إلى القيم الافتراضية
    ResetUiLayoutToDefaults,
    /// إغلاق البرنامج أثناء وجود تنزيلات نشطة (خيار التصغير للخلفية أو الإيقاف والخروج)
    CloseWhileDownloadsActive {
        active_count: usize,
    },
}
