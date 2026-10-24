use crate::{
    AppConfig, BatchExpandRequest, BatchExpandResponse, CrawlConfig, CrawlReport, DownloadStateMeta,
    EngineStatistics, FileCategory, ImportTasksPayload, InterceptDownloadPayload, QueueConfig,
    QueueSchedule, RefreshUrlPayload, RemoteZipPreviewResponse, ZipPreviewRequestPayload,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeMode {
    Dark,
    Light,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppLanguage {
    Arabic,
    English,
}

pub enum UiCommand {
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
    Redownload(String),
    DeleteWithFile(String),
    Control {
        task_id: String,
        action: &'static str,
    },
    SetSpeedLimit(u64),
}

pub enum UiEvent {
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
pub enum DownloadFilter {
    All,
    Unfinished,
    Finished,
    Category(FileCategory),
    Queue(String),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SortColumn {
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
pub enum OptionsTab {
    General,
    Connection,
    FileTypes,
    Proxy,
    SiteLogins,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum BottomInspectorTab {
    SegmentMap,
    SpeedGraph,
    ConnectionLogs,
    ChecksumAudit,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DialogTab {
    DownloadStatus,
    SpeedLimiter,
    OptionsOnCompletion,
    IntegrityAndMirrors,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ToolbarIcon {
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

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TreeIconKind {
    FolderYellow,
    FolderFinished,
    Compressed,
    Document,
    Music,
    Program,
    Video,
    Grabber,
}
