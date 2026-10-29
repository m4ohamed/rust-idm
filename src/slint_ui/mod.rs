//! واجهة سطح المكتب السلسة المبنية بـ Slint (`--features slint-ui`) — الإصدار الكامل v2.3.6
//!
//! تتضمن هذه النسخة:
//! 1. دعم النوافذ المستقلة الحقيقية خارج النافذة الرئيسية (`MainWindow`, `AddDownloadWindow`, `DetailsWindow`, `SchedulerWindow`, `SettingsWindow`, `DiagnosticsWindow`) مع منع تكرار النوافذ.
//! 2. حفظ واستعادة حجم وموضع النافذة الرئيسية والنوافذ الفرعية وتفضيلات المظهر واللغة في `slint_window_state.json`.
//! 3. ربط مباشر بـ `Arc<AppState>` في الذاكرة (Zero HTTP Polling) بسرعة 60 إطار/ثانية مع دعم كامل للغة العربية (RTL & BiDi الأصلي).
//! 4. قوائم سياقية ذكية (Right-Click) وحوارات تأكيد صريحة تميز بين حذف السجل وحذف الملف من القرص.

use crate::{
    build_daemon_router,
    core::{
        segmenter::DynamicDownloadJob,
        storage::{
            compute_file_all_digests, default_download_directory, redact_log_message,
            save_meta_to_disk_atomic, sync_parent_dir, DownloadStatus, FileCategory, SegmentStatus,
        },
        tools::{get_tools_status, update_all_tools},
    },
    inspector::zip::{RemoteZipInspector, WildcardPatternExpander},
    persist_queue_schedules, AppConfig, AppState, InterceptDownloadPayload, PostQueueAction,
    QueueConfig, ScheduleMode,
};
use anyhow::{Context, Result};
use reqwest::header::HeaderMap;
use serde::{Deserialize, Serialize};
use slint::{
    ComponentHandle, LogicalSize, ModelRc, PhysicalPosition, SharedString, Timer,
    TimerMode, VecModel,
};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    rc::Rc,
    sync::{atomic::Ordering, Arc},
    time::Duration,
};
use tokio::runtime::Builder;
use url::Url;

slint::include_modules!();

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SlintSavedWindowBounds {
    x: i32,
    y: i32,
    width: f32,
    height: f32,
    maximized: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SlintPersistedUiState {
    arabic: bool,
    dark_mode: bool,
    show_sidebar: bool,
    show_toolbar: bool,
    show_status_bar: bool,
    show_bottom_inspector: bool,
    main_window: SlintSavedWindowBounds,
    secondary_windows: HashMap<String, SlintSavedWindowBounds>,
}

impl Default for SlintPersistedUiState {
    fn default() -> Self {
        Self {
            arabic: true,
            dark_mode: true,
            show_sidebar: true,
            show_toolbar: true,
            show_status_bar: true,
            show_bottom_inspector: true,
            main_window: SlintSavedWindowBounds {
                x: 120,
                y: 80,
                width: 1140.0,
                height: 680.0,
                maximized: false,
            },
            secondary_windows: HashMap::new(),
        }
    }
}

impl SlintPersistedUiState {
    fn file_path(base_dir: &Path) -> PathBuf {
        base_dir.join("slint_window_state.json")
    }

    fn load(base_dir: &Path) -> Self {
        let p = Self::file_path(base_dir);
        if let Ok(bytes) = fs::read(&p) {
            if let Ok(mut st) = serde_json::from_slice::<Self>(&bytes) {
                // Clamp off-screen coordinates safely into visible work area
                st.main_window.width = st.main_window.width.clamp(880.0, 2560.0);
                st.main_window.height = st.main_window.height.clamp(520.0, 1440.0);
                if st.main_window.x < -100 || st.main_window.x > 3400 {
                    st.main_window.x = 120;
                }
                if st.main_window.y < -50 || st.main_window.y > 1800 {
                    st.main_window.y = 80;
                }
                return st;
            }
        }
        Self::default()
    }

    fn save(&self, base_dir: &Path) {
        let _ = fs::create_dir_all(base_dir);
        let p = Self::file_path(base_dir);
        let tmp = p.with_extension("json.tmp");
        if let Ok(bytes) = serde_json::to_vec_pretty(self) {
            if fs::write(&tmp, bytes).is_ok() {
                let _ = fs::rename(&tmp, &p);
            }
        }
    }
}

fn format_bytes(bytes: u64) -> String {
    if bytes == 0 {
        return "غير معروف".to_string();
    }
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let b = bytes as f64;
    if b >= GB {
        format!("{:.2} GB", b / GB)
    } else if b >= MB {
        format!("{:.1} MB", b / MB)
    } else if b >= KB {
        format!("{:.0} KB", b / KB)
    } else {
        format!("{bytes} B")
    }
}

fn format_downloaded_bytes(bytes: u64) -> String {
    if bytes == 0 {
        return "0 B".to_string();
    }
    format_bytes(bytes)
}

fn format_speed(bps: u64) -> String {
    if bps == 0 {
        return "—".to_string();
    }
    format!("{}/s", format_bytes(bps))
}

fn format_eta(secs: u64) -> String {
    if secs >= 3600 {
        format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
    } else if secs >= 60 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else {
        format!("{}s", secs)
    }
}

fn open_file_os(path: &Path) {
    #[cfg(target_os = "windows")]
    let _ = std::process::Command::new("cmd")
        .args(["/c", "start", "", &path.to_string_lossy()])
        .spawn();

    #[cfg(not(target_os = "windows"))]
    let _ = std::process::Command::new("xdg-open").arg(path).spawn();
}

fn reveal_in_explorer_os(path: &Path) {
    #[cfg(target_os = "windows")]
    {
        let arg = format!("/select,\"{}\"", path.display());
        let _ = std::process::Command::new("explorer").arg(arg).spawn();
    }

    #[cfg(not(target_os = "windows"))]
    if let Some(parent) = path.parent() {
        let _ = std::process::Command::new("xdg-open").arg(parent).spawn();
    }
}

fn open_folder_os(path: &Path) {
    let folder = if path.is_dir() {
        path
    } else {
        path.parent().unwrap_or(path)
    };
    #[cfg(target_os = "windows")]
    let _ = std::process::Command::new("explorer").arg(folder).spawn();

    #[cfg(not(target_os = "windows"))]
    let _ = std::process::Command::new("xdg-open").arg(folder).spawn();
}

/// تشغيل واجهة Slint الأصلية الكاملة مع النوافذ الـ 6 المستقلة والربط المباشر بـ `Arc<AppState>`
pub fn run() -> Result<()> {
    let runtime = Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("تعذر إنشاء بيئة التشغيل غير المتزامنة Tokio")?;

    let base_dir = default_download_directory();
    let ui_prefs = Arc::new(std::sync::Mutex::new(SlintPersistedUiState::load(&base_dir)));

    let state = runtime.block_on(async {
        let st = AppState::new(base_dir.clone(), 8, 0);
        let _ = st.restore_persisted_tasks_on_startup().await;
        st.start_scheduler();

        if let Ok(listener) = tokio::net::TcpListener::bind("127.0.0.1:16888").await {
            let _ = crate::ipc::native_host::write_bridge_config(
                &base_dir,
                crate::ipc::native_host::DEFAULT_BRIDGE_PORT,
                &st.bridge_token,
            );
            let router_state = Arc::clone(&st);
            tokio::spawn(async move {
                let _ = axum::serve(listener, build_daemon_router(router_state)).await;
            });
        }
        let bdir_for_reg = base_dir.clone();
        std::thread::spawn(move || {
            if let Ok(exe_path) = std::env::current_exe() {
                let _ = crate::ipc::native_host::connect_installed_browsers(&bdir_for_reg, &exe_path, None);
            }
        });
        Ok::<_, anyhow::Error>(st)
    })?;

    let rt_handle = runtime.handle().clone();

    // تهيئة النافذة الرئيسية والنوافذ الـ 5 المستقلة (Singletons لا تتكرر عند إعادة الطلب)
    let main_win = MainWindow::new().context("تعذر تهيئة نافذة Slint الرئيسية")?;
    let add_win = AddDownloadWindow::new().context("تعذر تهيئة نافذة إضافة التنزيل المستقلة")?;
    let details_win = DetailsWindow::new().context("تعذر تهيئة نافذة تفاصيل التنزيل المستقلة")?;
    let scheduler_win = SchedulerWindow::new().context("تعذر تهيئة نافذة الجدولة المستقلة")?;
    let settings_win = SettingsWindow::new().context("تعذر تهيئة نافذة الإعدادات المستقلة")?;
    let diag_win = DiagnosticsWindow::new().context("تعذر تهيئة نافذة التشخيص المستقلة")?;

    // استعادة حجم وموضع النافذة الرئيسية المحفوظة
    {
        let p = ui_prefs.lock().unwrap_or_else(|p| p.into_inner());
        main_win.set_arabic(p.arabic);
        main_win.set_dark_mode(p.dark_mode);
        main_win.set_show_sidebar(p.show_sidebar);
        main_win.set_show_toolbar(p.show_toolbar);
        main_win.set_show_status_bar(p.show_status_bar);
        main_win.set_show_bottom_inspector(p.show_bottom_inspector);
        main_win.window().set_size(LogicalSize::new(
            p.main_window.width,
            p.main_window.height,
        ));
        main_win
            .window()
            .set_position(PhysicalPosition::new(p.main_window.x, p.main_window.y));
        if p.main_window.maximized {
            main_win.window().set_maximized(true);
        }
        add_win.set_save_dir_input(SharedString::from(base_dir.display().to_string()));
    }

    let rows_model = Rc::new(VecModel::<SlintDownloadRow>::default());
    main_win.set_downloads(ModelRc::from(rows_model.clone()));

    let zip_model = Rc::new(VecModel::<SlintZipEntryRow>::default());
    main_win.set_zip_entries(ModelRc::from(zip_model.clone()));

    let queues_model = Rc::new(VecModel::<SlintQueueRow>::default());
    scheduler_win.set_queues(ModelRc::from(queues_model.clone()));

    let segments_model = Rc::new(VecModel::<SlintSegmentRow>::default());
    details_win.set_segments(ModelRc::from(segments_model.clone()));

    // ========================================================================
    // 1. ربط فتح النوافذ المستقلة (Single-Instance Independent OS Windows)
    // ========================================================================
    {
        let add_weak = add_win.as_weak();
        let main_weak = main_win.as_weak();
        main_win.on_open_add_window(move || {
            if let (Some(aw), Some(mw)) = (add_weak.upgrade(), main_weak.upgrade()) {
                aw.set_arabic(mw.get_arabic());
                aw.set_dark_mode(mw.get_dark_mode());
                let _ = aw.show();
            }
        });
    }
    {
        let add_weak = add_win.as_weak();
        add_win.on_cancel_window(move || {
            if let Some(aw) = add_weak.upgrade() {
                let _ = aw.hide();
            }
        });
    }
    {
        add_win.on_open_save_dir(move |dir_str| {
            open_folder_os(Path::new(dir_str.as_str()));
        });
    }

    {
        let det_weak = details_win.as_weak();
        let main_weak = main_win.as_weak();
        main_win.on_open_details_window(move |task_id| {
            if let (Some(dw), Some(mw)) = (det_weak.upgrade(), main_weak.upgrade()) {
                mw.set_selected_task_id(task_id.clone());
                dw.set_task_id(task_id);
                dw.set_arabic(mw.get_arabic());
                dw.set_dark_mode(mw.get_dark_mode());
                let _ = dw.show();
            }
        });
    }
    {
        let det_weak = details_win.as_weak();
        details_win.on_close_window(move || {
            if let Some(dw) = det_weak.upgrade() {
                // إغلاق نافذة التفاصيل لا يلغي أو يوقف التنزيل الجاري
                let _ = dw.hide();
            }
        });
    }

    {
        let sch_weak = scheduler_win.as_weak();
        let main_weak = main_win.as_weak();
        main_win.on_open_scheduler_window(move || {
            if let (Some(sw), Some(mw)) = (sch_weak.upgrade(), main_weak.upgrade()) {
                sw.set_arabic(mw.get_arabic());
                sw.set_dark_mode(mw.get_dark_mode());
                let _ = sw.show();
            }
        });
    }
    {
        let sch_weak = scheduler_win.as_weak();
        scheduler_win.on_close_window(move || {
            if let Some(sw) = sch_weak.upgrade() {
                let _ = sw.hide();
            }
        });
    }

    {
        let set_weak = settings_win.as_weak();
        let main_weak = main_win.as_weak();
        let st_clone = Arc::clone(&state);
        let rt = rt_handle.clone();
        main_win.on_open_settings_window(move |sec| {
            if let (Some(sw), Some(mw)) = (set_weak.upgrade(), main_weak.upgrade()) {
                sw.set_active_section(sec);
                sw.set_arabic(mw.get_arabic());
                sw.set_dark_mode(mw.get_dark_mode());
                let cfg = rt.block_on(async { st_clone.config.read().await.clone() });
                sw.set_download_dir_input(SharedString::from(cfg.download_dir.display().to_string()));
                sw.set_max_connections_input(SharedString::from(cfg.max_connections_per_file.to_string()));
                sw.set_speed_limit_input(SharedString::from(cfg.global_speed_limit_kbps.to_string()));
                sw.set_proxy_url_input(SharedString::from(cfg.proxy_url.unwrap_or_default()));
                sw.set_user_agent_input(SharedString::from(cfg.custom_user_agent));
                sw.set_extensions_input(SharedString::from(cfg.intercept_extensions.join(", ")));
                sw.set_auto_start_downloads(cfg.auto_start_queued_downloads);
                sw.set_verify_checksums(cfg.enable_checksum_verification);
                sw.set_confirm_destructive(cfg.confirm_destructive_actions);
                sw.set_minimize_to_tray(cfg.minimize_to_tray_on_close);
                sw.set_notify_complete(cfg.notify_on_complete);
                sw.set_notify_failure(cfg.notify_on_failure);
                let _ = sw.show();
            }
        });
    }
    {
        let set_weak = settings_win.as_weak();
        settings_win.on_close_window(move || {
            if let Some(sw) = set_weak.upgrade() {
                let _ = sw.hide();
            }
        });
    }

    {
        let diag_weak = diag_win.as_weak();
        let main_weak = main_win.as_weak();
        let st_clone = Arc::clone(&state);
        let rt = rt_handle.clone();
        let open_diag_fn = move || {
            if let (Some(dw), Some(mw)) = (diag_weak.upgrade(), main_weak.upgrade()) {
                dw.set_arabic(mw.get_arabic());
                dw.set_dark_mode(mw.get_dark_mode());
                let rep = build_slint_diagnostic_report(&st_clone, &rt);
                dw.set_diagnostic_report(SharedString::from(rep));
                let _ = dw.show();
            }
        };
        let open_diag_clone = open_diag_fn.clone();
        main_win.on_open_diagnostics_window(open_diag_fn);
        settings_win.on_open_diagnostics(open_diag_clone);
    }
    {
        let diag_weak = diag_win.as_weak();
        let st_clone = Arc::clone(&state);
        let rt = rt_handle.clone();
        diag_win.on_refresh_report(move || {
            if let Some(dw) = diag_weak.upgrade() {
                let rep = build_slint_diagnostic_report(&st_clone, &rt);
                dw.set_diagnostic_report(SharedString::from(rep));
            }
        });
    }
    {
        let diag_weak = diag_win.as_weak();
        diag_win.on_close_window(move || {
            if let Some(dw) = diag_weak.upgrade() {
                let _ = dw.hide();
            }
        });
    }

    // ========================================================================
    // 2. ربط فحص الرابط (Probe) وإضافة التنزيلات من نافذة AddDownloadWindow المستقلة
    // ========================================================================
    {
        let st = Arc::clone(&state);
        let rt = rt_handle.clone();
        let add_weak = add_win.as_weak();
        add_win.on_probe_url(move |url, _referer, _cookies| {
            let u_str = url.trim().to_string();
            if u_str.is_empty() {
                return;
            }
            if let Some(aw) = add_weak.upgrade() {
                aw.set_probe_busy(true);
                aw.set_validation_message(SharedString::from(""));
            }
            let client = st.client.clone();
            let aw_clone = add_weak.clone();
            rt.spawn(async move {
                let host = Url::parse(&u_str)
                    .ok()
                    .and_then(|u| u.host_str().map(String::from))
                    .unwrap_or_default();
                let res = DynamicDownloadJob::probe_url(&client, &u_str, &HeaderMap::new()).await;
                let _ = slint::invoke_from_event_loop(move || {
                    let Some(aw) = aw_clone.upgrade() else { return };
                    aw.set_probe_busy(false);
                    if !host.is_empty() {
                        aw.set_probed_host_text(SharedString::from(format!("🌐 {host}")));
                    }
                    match res {
                        Ok((size, resumable, name, _, _)) => {
                            if aw.get_filename_input().trim().is_empty() && !name.is_empty() {
                                aw.set_filename_input(SharedString::from(name));
                            }
                            aw.set_probed_size_text(SharedString::from(format!(
                                "الحجم المؤكد: {}",
                                format_bytes(size)
                            )));
                            aw.set_probed_resumable_text(SharedString::from(if resumable {
                                " | يدعم الاستئناف ✓"
                            } else {
                                " | لا يدعم التقسيم"
                            }));
                        }
                        Err(e) => {
                            aw.set_validation_message(SharedString::from(format!(
                                "ℹ تعذر الفحص المسبق ({e}) — يمكنك بدء التنزيل مباشرة."
                            )));
                        }
                    }
                });
            });
        });
    }

    let enqueue_from_add_window = {
        let st = Arc::clone(&state);
        let rt = rt_handle.clone();
        let add_weak = add_win.as_weak();
        let main_weak = main_win.as_weak();
        move |start_immediately: bool| {
            let Some(aw) = add_weak.upgrade() else { return };
            let url_str = aw.get_url_input().trim().to_string();
            if url_str.is_empty() {
                return;
            }
            let fn_opt = {
                let s = aw.get_filename_input().trim().to_string();
                (!s.is_empty()).then_some(s)
            };
            let save_dir_opt = {
                let s = aw.get_save_dir_input().trim().to_string();
                (!s.is_empty()).then(|| PathBuf::from(s))
            };
            let q_name = {
                let s = aw.get_queue_input().trim().to_string();
                if s.is_empty() {
                    "main".to_string()
                } else {
                    s
                }
            };
            let sha_opt = {
                let s = aw.get_sha256_input().trim().to_string();
                (!s.is_empty()).then_some(s)
            };
            let md5_opt = {
                let s = aw.get_md5_input().trim().to_string();
                (!s.is_empty()).then_some(s)
            };
            let crc_opt = {
                let s = aw.get_crc32_input().trim().to_string();
                (!s.is_empty()).then_some(s)
            };
            let ref_opt = {
                let s = aw.get_referer_input().trim().to_string();
                (!s.is_empty()).then_some(s)
            };
            let desc_opt = {
                let s = aw.get_description_input().trim().to_string();
                (!s.is_empty()).then_some(s)
            };
            let mirrors: Vec<String> = aw
                .get_mirrors_input()
                .split(',')
                .map(|m| m.trim().to_string())
                .filter(|m| !m.is_empty())
                .collect();

            aw.set_url_input(SharedString::from(""));
            aw.set_filename_input(SharedString::from(""));
            aw.set_probed_size_text(SharedString::from("الحجم: لم يُفحص بعد"));
            aw.set_probed_resumable_text(SharedString::from(""));
            let _ = aw.hide();

            let st_clone = Arc::clone(&st);
            let mw_weak = main_weak.clone();
            rt.spawn(async move {
                if url_str.starts_with("magnet:?")
                    || url_str.to_ascii_lowercase().ends_with(".torrent")
                {
                    let save_dir = save_dir_opt.unwrap_or_else(|| {
                        st_clone.base_download_dir.join("Torrents")
                    });
                    let _ = tokio::fs::create_dir_all(&save_dir).await;
                    let opts = crate::core::torrent::TorrentAddOptions {
                        dir: save_dir,
                        select: None,
                        seed_minutes: 0,
                        download_limit_kb: st_clone.speed_limiter.get_limit_kbps(),
                        upload_limit_kb: 0,
                        check_integrity: false,
                    };
                    let msg = match st_clone.torrent_engine.add(&url_str, &opts).await {
                        Ok(gid) => format!("🧲 تم بدء تحميل التورنت بنجاح (GID: {gid})"),
                        Err(e) => format!("خطأ في التورنت: {e}"),
                    };
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(mw) = mw_weak.upgrade() {
                            mw.set_status_banner(SharedString::from(msg));
                        }
                    });
                    return;
                }
                let res = st_clone
                    .enqueue_download_with_options(
                        InterceptDownloadPayload {
                            url: url_str,
                            source_page_url: ref_opt.clone(),
                            filename: fn_opt,
                            referer: ref_opt,
                            queue_name: Some(q_name),
                            expected_sha256: sha_opt,
                            expected_md5: md5_opt,
                            expected_crc32: crc_opt,
                            mirror_urls: (!mirrors.is_empty()).then_some(mirrors),
                            description: desc_opt,
                            ..Default::default()
                        },
                        save_dir_opt,
                        start_immediately,
                    )
                    .await;
                let msg = match res {
                    Ok(tid) => format!(
                        "تمت جدولة المهمة بنجاح ({})",
                        &tid[..8.min(tid.len())]
                    ),
                    Err(e) => format!("خطأ: {e}"),
                };
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(mw) = mw_weak.upgrade() {
                        mw.set_status_banner(SharedString::from(msg));
                    }
                });
            });
        }
    };
    {
        let f_now = enqueue_from_add_window.clone();
        add_win.on_start_download_now(move || f_now(true));
        let f_later = enqueue_from_add_window;
        add_win.on_add_to_queue_later(move || f_later(false));
    }

    // ========================================================================
    // 3. ربط نافذة الجدولة والطوابير المستقلة (SchedulerWindow Callbacks)
    // ========================================================================
    {
        let st = Arc::clone(&state);
        let rt = rt_handle.clone();
        let sch_weak = scheduler_win.as_weak();
        scheduler_win.on_create_queue(move |name| {
            let q_name = name.trim().to_string();
            if q_name.is_empty() {
                return;
            }
            let st_clone = Arc::clone(&st);
            let sw = sch_weak.clone();
            rt.spawn(async move {
                let cfg = QueueConfig {
                    name: q_name.clone(),
                    max_concurrent_downloads: 4,
                    start_time: Some("22:00".to_string()),
                    stop_time: Some("06:00".to_string()),
                    post_action: PostQueueAction::Notify,
                    schedule_mode: ScheduleMode::Disabled,
                    start_at: None,
                    stop_at: None,
                    days_of_week: Vec::new(),
                    speed_limit_kbps: 0,
                };
                st_clone.queues.write().await.insert(q_name.clone(), cfg);
                let _ = persist_queue_schedules(&st_clone).await;
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(w) = sw.upgrade() {
                        w.set_status_feedback(SharedString::from(format!(
                            "✔ تم إنشاء الطابور '{q_name}' بنجاح"
                        )));
                    }
                });
            });
        });
    }
    {
        let st = Arc::clone(&state);
        let rt = rt_handle.clone();
        scheduler_win.on_delete_queue(move |name| {
            let q_name = name.trim().to_string();
            if q_name == "main" || q_name.is_empty() {
                return;
            }
            let st_clone = Arc::clone(&st);
            rt.spawn(async move {
                st_clone.queues.write().await.remove(&q_name);
                let _ = persist_queue_schedules(&st_clone).await;
            });
        });
    }
    {
        let st = Arc::clone(&state);
        let rt = rt_handle.clone();
        scheduler_win.on_start_queue_now(move |name| {
            let q_name = name.to_string();
            let st_clone = Arc::clone(&st);
            rt.spawn(async move {
                let jobs: Vec<_> = st_clone.jobs.read().await.values().cloned().collect();
                for job in jobs {
                    let (q, status, cancelled) = {
                        let m = job.meta.lock().await;
                        (m.queue_name.clone(), m.status, m.is_cancelled_by_user())
                    };
                    if q == q_name
                        && (status == DownloadStatus::Paused || status == DownloadStatus::Queued)
                        && !cancelled
                    {
                        job.pause_flag.store(false, Ordering::SeqCst);
                        job.schedule_paused.store(false, Ordering::SeqCst);
                        st_clone.spawn_download_job(job).await;
                    }
                }
            });
        });
    }
    {
        let st = Arc::clone(&state);
        let rt = rt_handle.clone();
        scheduler_win.on_stop_queue_now(move |name| {
            let q_name = name.to_string();
            let st_clone = Arc::clone(&st);
            rt.spawn(async move {
                let jobs: Vec<_> = st_clone.jobs.read().await.values().cloned().collect();
                for job in jobs {
                    let mut m = job.meta.lock().await;
                    if m.queue_name == q_name
                        && (m.status == DownloadStatus::Downloading
                            || m.status == DownloadStatus::Queued)
                    {
                        job.pause_flag.store(true, Ordering::SeqCst);
                        m.status = DownloadStatus::Paused;
                        let _ = save_meta_to_disk_atomic(&m, &m.meta_file_path()).await;
                    }
                }
            });
        });
    }
    {
        let st = Arc::clone(&state);
        let rt = rt_handle.clone();
        let sch_weak = scheduler_win.as_weak();
        scheduler_win.on_save_schedule(
            move |q_name, mode_code, start_t, stop_t, once_dt, max_conns_s, speed_lim_s| {
                let q_str = q_name.trim().to_string();
                let mode = match mode_code {
                    1 => ScheduleMode::Daily,
                    2 => ScheduleMode::Once,
                    _ => ScheduleMode::Disabled,
                };
                let max_c = max_conns_s.trim().parse::<usize>().unwrap_or(4).clamp(1, 32);
                let spd_kbps = speed_lim_s.trim().parse::<u64>().unwrap_or(0);
                let st_clone = Arc::clone(&st);
                let sw = sch_weak.clone();
                let start_str = start_t.trim().to_string();
                let stop_str = stop_t.trim().to_string();
                let once_str = once_dt.trim().to_string();

                rt.spawn(async move {
                    {
                        let mut queues = st_clone.queues.write().await;
                        if let Some(q) = queues.get_mut(&q_str) {
                            q.schedule_mode = mode;
                            q.max_concurrent_downloads = max_c;
                            q.speed_limit_kbps = spd_kbps;
                            q.start_time = (mode == ScheduleMode::Daily).then_some(start_str);
                            q.stop_time = (mode == ScheduleMode::Daily).then_some(stop_str);
                            q.start_at = (mode == ScheduleMode::Once).then_some(once_str);
                        }
                    }
                    if spd_kbps > 0 {
                        st_clone.speed_limiter.set_limit_kbps(spd_kbps);
                    }
                    let _ = persist_queue_schedules(&st_clone).await;
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(w) = sw.upgrade() {
                            w.set_status_feedback(SharedString::from(format!(
                                "✔ تم حفظ وتطبيق جدولة الطابور '{q_str}' بنجاح"
                            )));
                        }
                    });
                });
            },
        );
    }

    // ========================================================================
    // 4. ربط نافذة الإعدادات المستقلة (SettingsWindow Callbacks)
    // ========================================================================
    {
        let st = Arc::clone(&state);
        let rt = rt_handle.clone();
        let set_weak = settings_win.as_weak();
        let main_weak = main_win.as_weak();
        let prefs_arc = Arc::clone(&ui_prefs);
        let bdir = base_dir.clone();

        settings_win.on_save_all_settings(move || {
            let Some(sw) = set_weak.upgrade() else { return };
            let dir_str = sw.get_download_dir_input().trim().to_string();
            let max_c = sw
                .get_max_connections_input()
                .trim()
                .parse::<usize>()
                .unwrap_or(8)
                .clamp(1, 32);
            let spd = sw
                .get_speed_limit_input()
                .trim()
                .parse::<u64>()
                .unwrap_or(0);
            let proxy = {
                let s = sw.get_proxy_url_input().trim().to_string();
                (!s.is_empty()).then_some(s)
            };
            let ua = sw.get_user_agent_input().trim().to_string();
            let exts: Vec<String> = sw
                .get_extensions_input()
                .split(',')
                .map(|s| s.trim().trim_start_matches('.').to_lowercase())
                .filter(|s| !s.is_empty())
                .collect();

            let auto_st = sw.get_auto_start_downloads();
            let ver_chk = sw.get_verify_checksums();
            let conf_dest = sw.get_confirm_destructive();
            let min_tray = sw.get_minimize_to_tray();
            let not_comp = sw.get_notify_complete();
            let not_fail = sw.get_notify_failure();
            let is_ar = sw.get_arabic();
            let is_dark = sw.get_dark_mode();

            if let Some(mw) = main_weak.upgrade() {
                mw.set_arabic(is_ar);
                mw.set_dark_mode(is_dark);
            }
            {
                let mut p = prefs_arc.lock().unwrap_or_else(|p| p.into_inner());
                p.arabic = is_ar;
                p.dark_mode = is_dark;
                p.save(&bdir);
            }

            let st_clone = Arc::clone(&st);
            let sw_clone = set_weak.clone();
            rt.spawn(async move {
                st_clone.speed_limiter.set_limit_kbps(spd);
                let mut cfg = st_clone.config.write().await;
                if !dir_str.is_empty() {
                    cfg.download_dir = PathBuf::from(dir_str);
                }
                cfg.max_connections_per_file = max_c;
                cfg.global_speed_limit_kbps = spd;
                cfg.proxy_url = proxy;
                if !ua.is_empty() {
                    cfg.custom_user_agent = ua;
                }
                if !exts.is_empty() {
                    cfg.intercept_extensions = exts;
                }
                cfg.auto_start_queued_downloads = auto_st;
                cfg.enable_checksum_verification = ver_chk;
                cfg.confirm_destructive_actions = conf_dest;
                cfg.minimize_to_tray_on_close = min_tray;
                cfg.notify_on_complete = not_comp;
                cfg.notify_on_failure = not_fail;

                let path = st_clone.base_download_dir.join("config.json");
                if let Ok(bytes) = serde_json::to_vec_pretty(&*cfg) {
                    let _ = tokio::fs::write(&path, bytes).await;
                    sync_parent_dir(&path).await;
                }
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(w) = sw_clone.upgrade() {
                        w.set_save_feedback(SharedString::from(
                            "✔ تم حفظ الإعدادات وتطبيقها على المحرك والواجهة بنجاح",
                        ));
                    }
                });
            });
        });
    }
    {
        let main_weak = main_win.as_weak();
        let prefs_arc = Arc::clone(&ui_prefs);
        let bdir = base_dir.clone();
        settings_win.on_reset_ui_layout(move || {
            let def = SlintPersistedUiState::default();
            if let Some(mw) = main_weak.upgrade() {
                mw.set_show_sidebar(true);
                mw.set_show_toolbar(true);
                mw.set_show_status_bar(true);
                mw.set_show_bottom_inspector(true);
                mw.window()
                    .set_size(LogicalSize::new(def.main_window.width, def.main_window.height));
            }
            let mut p = prefs_arc.lock().unwrap_or_else(|p| p.into_inner());
            *p = def;
            p.save(&bdir);
        });
    }
    {
        let st = Arc::clone(&state);
        let rt = rt_handle.clone();
        let set_weak = settings_win.as_weak();
        settings_win.on_reset_engine_defaults(move || {
            let st_clone = Arc::clone(&st);
            let sw = set_weak.clone();
            rt.spawn(async move {
                let def = AppConfig::default();
                st_clone.speed_limiter.set_limit_kbps(0);
                *st_clone.config.write().await = def.clone();
                let path = st_clone.base_download_dir.join("config.json");
                if let Ok(bytes) = serde_json::to_vec_pretty(&def) {
                    let _ = tokio::fs::write(&path, bytes).await;
                }
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(w) = sw.upgrade() {
                        w.set_save_feedback(SharedString::from(
                            "✔ تمت استعادة الإعدادات الافتراضية للمحرك بنجاح",
                        ));
                    }
                });
            });
        });
    }

    // ========================================================================
    // 5. ربط أوامر التحكم في المهام (MainWindow & DetailsWindow)
    // ========================================================================
    let control_task_fn = {
        let st = Arc::clone(&state);
        let rt = rt_handle.clone();
        move |task_id: SharedString, action: SharedString| {
            let tid = task_id.to_string();
            let act = action.to_string();
            let st_clone = Arc::clone(&st);
            rt.spawn(async move {
                let job_opt = st_clone.jobs.read().await.get(&tid).cloned();
                if let Some(job) = job_opt {
                    if act == "pause" {
                        job.pause_flag.store(true, Ordering::SeqCst);
                        let mut m = job.meta.lock().await;
                        m.status = DownloadStatus::Paused;
                        let _ = save_meta_to_disk_atomic(&m, &m.meta_file_path()).await;
                    } else if act == "resume" {
                        job.pause_flag.store(false, Ordering::SeqCst);
                        job.schedule_paused.store(false, Ordering::SeqCst);
                        st_clone.spawn_download_job(job).await;
                    } else if act == "cancel" {
                        job.cancel_flag.store(true, Ordering::SeqCst);
                        job.pause_flag.store(true, Ordering::SeqCst);
                        let mut m = job.meta.lock().await;
                        m.status = DownloadStatus::Failed;
                        m.last_error = Some("Cancelled by user".to_string());
                        let _ = save_meta_to_disk_atomic(&m, &m.meta_file_path()).await;
                    }
                }
            });
        }
    };
    {
        let c1 = control_task_fn.clone();
        main_win.on_control_task(c1);
        let c_pause = control_task_fn.clone();
        details_win.on_pause_task(move |tid| c_pause(tid, SharedString::from("pause")));
        let c_res = control_task_fn.clone();
        details_win.on_resume_task(move |tid| c_res(tid, SharedString::from("resume")));
        let c_can = control_task_fn;
        details_win.on_cancel_task(move |tid| c_can(tid, SharedString::from("cancel")));
    }

    {
        let st = Arc::clone(&state);
        let rt = rt_handle.clone();
        main_win.on_redownload_task(move |task_id| {
            let tid = task_id.to_string();
            let st_clone = Arc::clone(&st);
            rt.spawn(async move {
                if let Some(job) = st_clone.jobs.read().await.get(&tid).cloned() {
                    job.pause_flag.store(true, Ordering::SeqCst);
                    {
                        let mut m = job.meta.lock().await;
                        let _ = tokio::fs::remove_file(m.part_file_path()).await;
                        let _ = tokio::fs::remove_file(m.final_file_path()).await;
                        m.segments.clear();
                        m.status = DownloadStatus::Queued;
                        m.last_error = None;
                        let _ = save_meta_to_disk_atomic(&m, &m.meta_file_path()).await;
                    }
                    job.cancel_flag.store(false, Ordering::SeqCst);
                    job.pause_flag.store(false, Ordering::SeqCst);
                    job.schedule_paused.store(false, Ordering::SeqCst);
                    st_clone.spawn_download_job(job).await;
                }
            });
        });
    }

    // التنزيل الدفعي وفاحص ZIP
    {
        let st = Arc::clone(&state);
        let rt = rt_handle.clone();
        let win_weak = main_win.as_weak();
        main_win.on_add_batch(move |pattern, step_str| {
            let step = step_str.trim().parse::<usize>().unwrap_or(1).max(1);
            let pat = pattern.to_string();
            let st_clone = Arc::clone(&st);
            let w = win_weak.clone();
            rt.spawn(async move {
                let msg = match WildcardPatternExpander::expand_pattern(&pat, step) {
                    Ok(urls) => {
                        let mut count = 0usize;
                        for u in urls {
                            if st_clone
                                .enqueue_download(InterceptDownloadPayload {
                                    url: u,
                                    queue_name: Some("main".to_string()),
                                    ..Default::default()
                                })
                                .await
                                .is_ok()
                            {
                                count += 1;
                            }
                        }
                        format!("تمت جدولة {count} رابط من النمط الدفعي")
                    }
                    Err(e) => format!("خطأ في توسيع النمط: {e}"),
                };
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(win) = w.upgrade() {
                        win.set_status_banner(SharedString::from(msg));
                    }
                });
            });
        });
    }
    {
        let st = Arc::clone(&state);
        let rt = rt_handle.clone();
        let win_weak = main_win.as_weak();
        main_win.on_inspect_remote_zip(move |url| {
            let target = url.trim().to_string();
            if target.is_empty() {
                return;
            }
            let client = st.client.clone();
            let w = win_weak.clone();
            rt.spawn(async move {
                let res =
                    RemoteZipInspector::inspect_remote_zip(&client, &target, &HeaderMap::new())
                        .await;
                let _ = slint::invoke_from_event_loop(move || {
                    let Some(win) = w.upgrade() else { return };
                    match res {
                        Ok(preview) => {
                            let rows: Vec<SlintZipEntryRow> = preview
                                .entries
                                .into_iter()
                                .map(|e| SlintZipEntryRow {
                                    filename: SharedString::from(e.filename),
                                    compressed_size: SharedString::from(format_bytes(
                                        e.compressed_size,
                                    )),
                                    uncompressed_size: SharedString::from(format_bytes(
                                        e.uncompressed_size,
                                    )),
                                    crc32_hex: SharedString::from(e.crc32_hex),
                                })
                                .collect();
                            win.set_zip_entries(ModelRc::from(Rc::new(VecModel::from(rows))));
                            win.set_zip_summary_text(SharedString::from(format!(
                                "إجمالي الملفات: {} | حجم الأرشيف: {}",
                                preview.total_entries,
                                format_bytes(preview.archive_total_size)
                            )));
                        }
                        Err(e) => {
                            win.set_zip_summary_text(SharedString::from(format!(
                                "تعذر فحص الأرشيف: {e}"
                            )));
                        }
                    }
                });
            });
        });
    }

    // إيقاف الكل / استئناف الكل / مسح المكتمل / حذف مهمة
    {
        let st = Arc::clone(&state);
        let rt = rt_handle.clone();
        main_win.on_stop_all_tasks(move || {
            let st_clone = Arc::clone(&st);
            rt.spawn(async move {
                let jobs: Vec<_> = st_clone.jobs.read().await.values().cloned().collect();
                for job in jobs {
                    job.pause_flag.store(true, Ordering::SeqCst);
                    let mut m = job.meta.lock().await;
                    if m.status == DownloadStatus::Downloading || m.status == DownloadStatus::Queued
                    {
                        m.status = DownloadStatus::Paused;
                    }
                }
            });
        });
    }
    {
        let st = Arc::clone(&state);
        let rt = rt_handle.clone();
        main_win.on_resume_all_tasks(move || {
            let st_clone = Arc::clone(&st);
            rt.spawn(async move {
                let jobs: Vec<_> = st_clone.jobs.read().await.values().cloned().collect();
                for job in jobs {
                    let (st, cancelled) = {
                        let m = job.meta.lock().await;
                        (m.status, m.is_cancelled_by_user())
                    };
                    if (st == DownloadStatus::Paused || st == DownloadStatus::Queued) && !cancelled {
                        job.pause_flag.store(false, Ordering::SeqCst);
                        st_clone.spawn_download_job(job).await;
                    }
                }
            });
        });
    }
    {
        let st = Arc::clone(&state);
        let rt = rt_handle.clone();
        main_win.on_delete_completed_tasks(move || {
            let st_clone = Arc::clone(&st);
            rt.spawn(async move {
                let mut to_remove = Vec::new();
                {
                    let jobs = st_clone.jobs.read().await;
                    for (tid, job) in jobs.iter() {
                        let m = job.meta.lock().await;
                        if m.status == DownloadStatus::Completed {
                            to_remove.push((tid.clone(), m.meta_file_path()));
                        }
                    }
                }
                let mut jobs = st_clone.jobs.write().await;
                for (tid, meta_path) in to_remove {
                    let _ = tokio::fs::remove_file(&meta_path).await;
                    jobs.remove(&tid);
                }
            });
        });
    }
    {
        let st = Arc::clone(&state);
        let rt = rt_handle.clone();
        main_win.on_remove_task(move |task_id, delete_file| {
            let tid = task_id.to_string();
            let st_clone = Arc::clone(&st);
            rt.spawn(async move {
                let job_opt = st_clone.jobs.write().await.remove(&tid);
                if let Some(job) = job_opt {
                    job.cancel_flag.store(true, Ordering::SeqCst);
                    job.pause_flag.store(true, Ordering::SeqCst);
                    let m = job.meta.lock().await;
                    let _ = tokio::fs::remove_file(m.meta_file_path()).await;
                    let _ = tokio::fs::remove_file(m.part_file_path()).await;
                    if delete_file {
                        let _ = tokio::fs::remove_file(m.final_file_path()).await;
                    }
                }
            });
        });
    }

    // إعادة فحص البصمات وتحديث الأدوات وفتح الملفات
    let reverify_fn = {
        let st = Arc::clone(&state);
        let rt = rt_handle.clone();
        let win_weak = main_win.as_weak();
        move |task_id: SharedString| {
            let tid = task_id.to_string();
            let st_clone = Arc::clone(&st);
            let w = win_weak.clone();
            rt.spawn(async move {
                let Some(job) = st_clone.jobs.read().await.get(&tid).cloned() else {
                    return;
                };
                let final_path = job.meta.lock().await.final_file_path();
                if let Ok((sha256, md5, crc32)) = compute_file_all_digests(&final_path).await {
                    let mut m = job.meta.lock().await;
                    m.computed_sha256 = Some(sha256.clone());
                    m.computed_md5 = Some(md5);
                    m.computed_crc32 = Some(crc32.clone());
                    let _ = save_meta_to_disk_atomic(&m, &m.meta_file_path()).await;
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(win) = w.upgrade() {
                            win.set_status_banner(SharedString::from(format!(
                                "تم التحقق من البصمة: CRC32={crc32} | SHA256={}..",
                                &sha256[..10.min(sha256.len())]
                            )));
                        }
                    });
                }
            });
        }
    };
    main_win.on_reverify_checksum(reverify_fn.clone());
    details_win.on_reverify_task(reverify_fn);

    let update_tools_fn = {
        let st = Arc::clone(&state);
        let rt = rt_handle.clone();
        let win_weak = main_win.as_weak();
        let set_weak = settings_win.as_weak();
        move || {
            let client = st.client.clone();
            let dir = st.base_download_dir.clone();
            let w = win_weak.clone();
            let sw = set_weak.clone();
            if let Some(win) = w.upgrade() {
                win.set_status_banner(SharedString::from(
                    "جاري فحص وتحديث أداتي yt-dlp و FFmpeg من GitHub Releases...",
                ));
            }
            rt.spawn(async move {
                let report = update_all_tools(client, dir, true).await;
                let _ = slint::invoke_from_event_loop(move || {
                    let y = if report.ytdlp_installed { "✓" } else { "✗" };
                    let f = if report.ffmpeg_installed { "✓" } else { "✗" };
                    let badge = format!("yt-dlp: {y} | FFmpeg: {f}");
                    if let Some(win) = w.upgrade() {
                        win.set_tools_badge_text(SharedString::from(&badge));
                        win.set_status_banner(SharedString::from(
                            "اكتمل فحص وتحديث الأدوات المساعدة (yt-dlp & FFmpeg)",
                        ));
                    }
                    if let Some(s_win) = sw.upgrade() {
                        s_win.set_tools_status_summary(SharedString::from(badge));
                    }
                });
            });
        }
    };
    main_win.on_update_external_tools(update_tools_fn.clone());
    settings_win.on_update_media_tools(update_tools_fn);

    let open_file_cb = {
        let st = Arc::clone(&state);
        let rt = rt_handle.clone();
        move |task_id: SharedString| {
            let tid = task_id.to_string();
            let st_clone = Arc::clone(&st);
            rt.spawn(async move {
                if let Some(job) = st_clone.jobs.read().await.get(&tid).cloned() {
                    let path = job.meta.lock().await.final_file_path();
                    if path.exists() {
                        open_file_os(&path);
                    }
                }
            });
        }
    };
    main_win.on_open_task_file(open_file_cb.clone());
    details_win.on_open_file(open_file_cb);

    let open_folder_cb = {
        let st = Arc::clone(&state);
        let rt = rt_handle.clone();
        move |task_id: SharedString| {
            let tid = task_id.to_string();
            let st_clone = Arc::clone(&st);
            rt.spawn(async move {
                if let Some(job) = st_clone.jobs.read().await.get(&tid).cloned() {
                    let path = job.meta.lock().await.final_file_path();
                    if path.exists() {
                        reveal_in_explorer_os(&path);
                    } else {
                        open_folder_os(&job.meta.lock().await.save_dir);
                    }
                }
            });
        }
    };
    main_win.on_open_task_folder(open_folder_cb.clone());
    details_win.on_open_folder(open_folder_cb);

    // تفعيل وضع الألعاب (Game Mode) والخنق الذكي من شريط أدوات Slint
    {
        let st = Arc::clone(&state);
        let rt = rt_handle.clone();
        let win_weak = main_win.as_weak();
        main_win.on_toggle_game_mode(move || {
            let st_clone = Arc::clone(&st);
            let w = win_weak.clone();
            rt.spawn(async move {
                let mut sm = st_clone.smart_throttle.lock().await;
                let next_active = !sm.game_mode_active;
                let peak_kbps = st_clone.peak_speed_bps.load(Ordering::Relaxed) / 1024;
                sm.set_game_mode(next_active, Some(60), peak_kbps);
                let cap = sm.effective_speed_limit_kbps;
                drop(sm);
                if cap > 0 {
                    st_clone.speed_limiter.set_limit_kbps(cap);
                } else {
                    let cfg_lim = st_clone.config.read().await.global_speed_limit_kbps;
                    st_clone.speed_limiter.set_limit_kbps(cfg_lim);
                }
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(win) = w.upgrade() {
                        win.set_game_mode_active(next_active);
                        win.set_status_banner(SharedString::from(if next_active {
                            "🎮 تم تفعيل وضع الألعاب (Game Mode) وتخفيض استهلاك الشبكة تلقائياً"
                        } else {
                            "⚡ تم إيقاف وضع الألعاب واستعادة السرعة القصوى للتحميل"
                        }));
                    }
                });
            });
        });
    }

    // معاينة الفيديو أو الصوت أثناء التحميل (Play Preview)
    let preview_cb = {
        let st = Arc::clone(&state);
        let rt = rt_handle.clone();
        let win_weak = main_win.as_weak();
        move |task_id: SharedString| {
            let tid = task_id.to_string();
            let st_clone = Arc::clone(&st);
            let w = win_weak.clone();
            rt.spawn(async move {
                let Some(job) = st_clone.jobs.read().await.get(&tid).cloned() else {
                    return;
                };
                let (part_path, final_path, filename, status) = {
                    let m = job.meta.lock().await;
                    (
                        m.part_file_path(),
                        m.final_file_path(),
                        m.filename.clone(),
                        m.status,
                    )
                };
                let target_path = if status == DownloadStatus::Completed && final_path.exists() {
                    final_path
                } else {
                    part_path
                };
                let msg = match crate::core::preview::play_partial_download(
                    &target_path,
                    &filename,
                ) {
                    Ok(preview_p) => format!("🎬 تم تشغيل المعاينة الحية: {}", preview_p.display()),
                    Err(e) => format!("⚠ تعذر تشغيل المعاينة: {e}"),
                };
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(win) = w.upgrade() {
                        win.set_status_banner(SharedString::from(msg));
                    }
                });
            });
        }
    };
    main_win.on_preview_task(preview_cb.clone());
    details_win.on_preview_task(preview_cb);

    // تشخيص العطل التلقائي (Download Doctor)
    let doctor_cb = {
        let st = Arc::clone(&state);
        let rt = rt_handle.clone();
        let win_weak = main_win.as_weak();
        let det_weak = details_win.as_weak();
        move |task_id: SharedString| {
            let tid = task_id.to_string();
            let st_clone = Arc::clone(&st);
            let w = win_weak.clone();
            let dw = det_weak.clone();
            rt.spawn(async move {
                let Some(job) = st_clone.jobs.read().await.get(&tid).cloned() else {
                    return;
                };
                let diag = {
                    let m = job.meta.lock().await;
                    crate::core::doctor::diagnose_task_with_doctor(&m)
                };
                let banner = format!(
                    "🩺 تشخيص العطل ({:?}): {}",
                    diag.category, diag.explanation_arabic
                );
                let actions_str = diag
                    .recommended_actions
                    .iter()
                    .map(|a| format!("{a:?}"))
                    .collect::<Vec<_>>()
                    .join(" | ");
                let full_err = format!(
                    "[{:?}] {}\nالإجراء العلاجي المقترح: {}",
                    diag.category, diag.explanation_arabic, actions_str
                );
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(win) = w.upgrade() {
                        win.set_status_banner(SharedString::from(banner));
                    }
                    if let Some(d_win) = dw.upgrade() {
                        d_win.set_error_text(SharedString::from(full_err));
                    }
                });
            });
        }
    };
    main_win.on_diagnose_task(doctor_cb.clone());
    details_win.on_diagnose_task(doctor_cb);

    // حفظ تفضيلات الواجهة وحجم/موضع النافذة الرئيسية
    {
        let main_weak = main_win.as_weak();
        let prefs_arc = Arc::clone(&ui_prefs);
        let bdir = base_dir.clone();
        main_win.on_persist_ui_preferences(move || {
            if let Some(mw) = main_weak.upgrade() {
                let mut p = prefs_arc.lock().unwrap_or_else(|p| p.into_inner());
                p.arabic = mw.get_arabic();
                p.dark_mode = mw.get_dark_mode();
                p.show_sidebar = mw.get_show_sidebar();
                p.show_toolbar = mw.get_show_toolbar();
                p.show_status_bar = mw.get_show_status_bar();
                p.show_bottom_inspector = mw.get_show_bottom_inspector();
                p.save(&bdir);
            }
        });
    }

    // ========================================================================
    // 6. مؤقت التحديث اللحظي من الذاكرة (150ms) لجميع النوافذ المفتوحة
    // ========================================================================
    let timer = Timer::default();
    {
        let st = Arc::clone(&state);
        let rt = rt_handle.clone();
        let win_weak = main_win.as_weak();
        let det_weak = details_win.as_weak();
        let sch_weak = scheduler_win.as_weak();
        let rows_model_clone = rows_model.clone();
        let queues_model_clone = queues_model.clone();
        let segs_model_clone = segments_model.clone();
        let prefs_arc = Arc::clone(&ui_prefs);
        let bdir = base_dir.clone();
        let mut tick_counter = 0u64;
        let mut file_exists_cache: HashMap<String, bool> = HashMap::new();

        timer.start(TimerMode::Repeated, Duration::from_millis(150), move || {
            tick_counter += 1;
            let Some(win) = win_weak.upgrade() else { return };
            let filter = win.get_active_filter().to_string();
            let search = win.get_search_text().to_string().to_lowercase();
            let selected_id = win.get_selected_task_id().to_string();
            let is_ar = win.get_arabic();

            // إصلاح الاختناق #2: لا نحفظ slint_window_state.json على القرص إلا إذا تغيرت إحداثيات أو حالة النافذة فعلياً (Dirty-Check)
            if tick_counter % 20 == 0 && !win.window().is_minimized() {
                let pos = win.window().position();
                let sz = win.window().size();
                let is_max = win.window().is_maximized();
                let scale = win.window().scale_factor().max(1.0);
                let mut p = prefs_arc.lock().unwrap_or_else(|p| p.into_inner());
                let mut changed = p.main_window.maximized != is_max;
                p.main_window.maximized = is_max;
                if !is_max && sz.width > 400 && sz.height > 300 {
                    let new_w = (sz.width as f32) / scale;
                    let new_h = (sz.height as f32) / scale;
                    if p.main_window.x != pos.x
                        || p.main_window.y != pos.y
                        || (p.main_window.width - new_w).abs() > 0.5
                        || (p.main_window.height - new_h).abs() > 0.5
                    {
                        p.main_window.x = pos.x;
                        p.main_window.y = pos.y;
                        p.main_window.width = new_w;
                        p.main_window.height = new_h;
                        changed = true;
                    }
                }
                if changed {
                    p.save(&bdir);
                }
            }

            let (snapshots, queues_list) = rt.block_on(async {
                let pending_items: Vec<_> = {
                    let mut pend = st.pending_intercepts.lock().await;
                    pend.drain(..).collect()
                };
                for item in pending_items {
                    let auto_start = st.config.read().await.auto_start_queued_downloads;
                    let _ = st.enqueue_download_with_options(item.payload, None, auto_start).await;
                }
                let jobs = st.jobs.read().await;
                let mut list = Vec::with_capacity(jobs.len());
                for job in jobs.values() {
                    list.push(job.meta.lock().await.clone());
                }
                let qs: Vec<QueueConfig> = st.queues.read().await.values().cloned().collect();
                (list, qs)
            });

            let mut sorted = snapshots;
            sorted.sort_by(|a, b| b.created_at.cmp(&a.created_at));

            let mut total_bps = 0u64;
            let mut active_cnt = 0i32;
            let mut queued_cnt = 0i32;
            let mut ui_rows = Vec::new();
            let mut selected_meta = None;

            for m in &sorted {
                let task_bps: u64 = m.smoothed_speed_bps();
                if matches!(
                    m.status,
                    DownloadStatus::Downloading
                        | DownloadStatus::RefreshingUrl
                        | DownloadStatus::Assembling
                        | DownloadStatus::VerifyingChecksum
                ) {
                    active_cnt += 1;
                    total_bps = total_bps.saturating_add(task_bps);
                } else if m.status == DownloadStatus::Queued {
                    queued_cnt += 1;
                }

                if m.task_id == selected_id {
                    selected_meta = Some(m.clone());
                }

                let matches_filter = match filter.as_str() {
                    "active" => matches!(
                        m.status,
                        DownloadStatus::Downloading
                            | DownloadStatus::RefreshingUrl
                            | DownloadStatus::Assembling
                            | DownloadStatus::VerifyingChecksum
                    ),
                    "paused" => m.status == DownloadStatus::Paused || m.status == DownloadStatus::Queued,
                    "unfinished" => m.status != DownloadStatus::Completed,
                    "completed" => m.status == DownloadStatus::Completed,
                    "failed" => matches!(
                        m.status,
                        DownloadStatus::Failed | DownloadStatus::NeedsUrlRefresh
                    ),
                    "video" => m.category == FileCategory::Video,
                    "compressed" => m.category == FileCategory::Compressed,
                    "programs" => m.category == FileCategory::Programs,
                    "documents" => m.category == FileCategory::Documents,
                    "music" => m.category == FileCategory::Music,
                    _ => true,
                };
                if !matches_filter {
                    continue;
                }
                if !search.is_empty()
                    && !m.filename.to_lowercase().contains(&search)
                    && !m.url.to_lowercase().contains(&search)
                    && !m.category.folder_name().to_lowercase().contains(&search)
                {
                    continue;
                }

                let downloaded = m.total_downloaded();
                let indeterminate = m.total_size == 0
                    && matches!(
                        m.status,
                        DownloadStatus::Downloading
                            | DownloadStatus::RefreshingUrl
                            | DownloadStatus::Assembling
                            | DownloadStatus::VerifyingChecksum
                    );
                let pct = if m.total_size > 0 {
                    ((downloaded as f64 / m.total_size as f64) * 100.0).clamp(0.0, 100.0) as f32
                } else if m.status == DownloadStatus::Completed {
                    100.0
                } else {
                    0.0
                };

                // إصلاح الاختناق #1: تجنب استدعاء exists() على القرص في كل دورة 150ms؛ يُفحص فقط للمهام المكتملة ويُخزن في الكاش (يُحدث كل ~5 ثوانٍ)
                let file_exists = if m.status == DownloadStatus::Completed {
                    let refresh_disk = tick_counter % 34 == 1 || !file_exists_cache.contains_key(&m.task_id);
                    if refresh_disk {
                        let ex = m.final_file_path().exists();
                        file_exists_cache.insert(m.task_id.clone(), ex);
                        ex
                    } else {
                        *file_exists_cache.get(&m.task_id).unwrap_or(&true)
                    }
                } else {
                    file_exists_cache.remove(&m.task_id);
                    false
                };
                let is_cancelled = m.is_cancelled_by_user();
                let is_expired = m.is_expired_link();

                let (status_str, state_code) = if is_cancelled {
                    (
                        (if is_ar { "ملغى" } else { "Cancelled" }).to_string(),
                        4,
                    )
                } else {
                    match m.status {
                        DownloadStatus::Downloading => (
                            if m.total_size > 0 {
                                format!(
                                    "{} {:.1}%",
                                    if is_ar { "جارٍ التنزيل" } else { "Downloading" },
                                    pct
                                )
                            } else {
                                (if is_ar { "جارٍ التنزيل •••" } else { "Downloading •••" })
                                    .to_string()
                            },
                            0,
                        ),
                        DownloadStatus::Paused => (
                            (if is_ar { "متوقف مؤقتاً" } else { "Paused" }).to_string(),
                            1,
                        ),
                        DownloadStatus::Queued => (
                            (if is_ar { "في الانتظار" } else { "Queued" }).to_string(),
                            1,
                        ),
                        DownloadStatus::Completed => (
                            if file_exists {
                                (if is_ar { "مكتمل ✓" } else { "Completed ✓" }).to_string()
                            } else {
                                (if is_ar {
                                    "مكتمل (الملف مفقود)"
                                } else {
                                    "Completed (Missing)"
                                })
                                .to_string()
                            },
                            2,
                        ),
                        DownloadStatus::Assembling | DownloadStatus::VerifyingChecksum => (
                            (if is_ar {
                                "جارٍ الدمج/التحقق"
                            } else {
                                "Verifying/Merging"
                            })
                            .to_string(),
                            5,
                        ),
                        DownloadStatus::NeedsUrlRefresh => (
                            (if is_ar {
                                "انتهت صلاحية الرابط"
                            } else {
                                "Link Expired"
                            })
                            .to_string(),
                            3,
                        ),
                        _ => ((if is_ar { "فشل" } else { "Failed" }).to_string(), 3),
                    }
                };

                let eta_str = m
                    .estimated_eta_secs()
                    .map(format_eta)
                    .unwrap_or_else(|| "—".to_string());

                let active_conns = m
                    .segments
                    .iter()
                    .filter(|s| s.status == SegmentStatus::Downloading)
                    .count() as i32;

                ui_rows.push(SlintDownloadRow {
                    task_id: SharedString::from(&m.task_id),
                    filename: SharedString::from(&m.filename),
                    category: SharedString::from(m.category.folder_name()),
                    queue_name: SharedString::from(&m.queue_name),
                    size_text: SharedString::from(format_bytes(m.total_size)),
                    downloaded_text: SharedString::from(format_downloaded_bytes(downloaded)),
                    status_text: SharedString::from(status_str),
                    progress_pct: pct,
                    indeterminate,
                    speed_text: SharedString::from(format_speed(task_bps)),
                    eta_text: SharedString::from(eta_str),
                    segments_count: m.segments.len() as i32,
                    active_conns,
                    created_at_text: SharedString::from(
                        m.created_at.format("%Y-%m-%d %H:%M").to_string(),
                    ),
                    save_dir_text: SharedString::from(m.save_dir.display().to_string()),
                    sha256: SharedString::from(m.computed_sha256.clone().unwrap_or_default()),
                    crc32: SharedString::from(m.computed_crc32.clone().unwrap_or_default()),
                    state_code,
                    file_exists,
                    is_expired,
                });
            }

            rows_model_clone.set_vec(ui_rows);
            win.set_active_count(active_cnt);
            win.set_queued_count(queued_cnt);

            // تحديث شريط التقدم على أيقونة البرنامج في شريط مهام ويندوز (ITaskbarList3)
            {
                let mut active_done = 0u64;
                let mut active_total = 0u64;
                let mut has_failed = false;
                for m in &sorted {
                    if matches!(
                        m.status,
                        DownloadStatus::Downloading
                            | DownloadStatus::RefreshingUrl
                            | DownloadStatus::Assembling
                            | DownloadStatus::VerifyingChecksum
                    ) && m.total_size > 0
                    {
                        active_done = active_done.saturating_add(m.total_downloaded());
                        active_total = active_total.saturating_add(m.total_size);
                    } else if matches!(
                        m.status,
                        DownloadStatus::Failed | DownloadStatus::NeedsUrlRefresh
                    ) && !m.is_cancelled_by_user()
                    {
                        has_failed = true;
                    }
                }
                let tb_state = if active_cnt > 0 {
                    if active_total > 0 {
                        crate::core::winfx::TaskbarProgressState::Normal(active_done, active_total)
                    } else {
                        crate::core::winfx::TaskbarProgressState::Busy
                    }
                } else if has_failed {
                    crate::core::winfx::TaskbarProgressState::Error(1, 1)
                } else {
                    crate::core::winfx::TaskbarProgressState::Off
                };
                crate::core::winfx::update_process_taskbar_progress(tb_state);
            }
            win.set_aggregate_speed_text(SharedString::from(if total_bps > 0 {
                format_speed(total_bps)
            } else {
                "0 KB/s".to_string()
            }));

            // تحديث جدول الطوابير في نافذة المجدول المستقلة
            if sch_weak.upgrade().is_some() {
                let mut q_rows = Vec::new();
                for q in &queues_list {
                    let t_cnt = sorted.iter().filter(|m| m.queue_name == q.name).count() as i32;
                    let mode_t = match q.schedule_mode {
                        ScheduleMode::Disabled => "معطّلة (يدوي)",
                        ScheduleMode::Daily => "مفعّلة (يومية)",
                        ScheduleMode::Once => "مفعّلة (مرة واحدة)",
                    };
                    q_rows.push(SlintQueueRow {
                        name: SharedString::from(&q.name),
                        mode_text: SharedString::from(mode_t),
                        start_time: SharedString::from(q.start_time.as_deref().unwrap_or("22:00")),
                        stop_time: SharedString::from(q.stop_time.as_deref().unwrap_or("06:00")),
                        days_text: SharedString::from(if q.days_of_week.is_empty() {
                            "كل الأيام".to_string()
                        } else {
                            format!("{} أيام", q.days_of_week.len())
                        }),
                        tasks_count: t_cnt,
                        max_concurrent: q.max_concurrent_downloads as i32,
                        speed_limit_kbps: q.speed_limit_kbps as i32,
                    });
                }
                queues_model_clone.set_vec(q_rows);
            }

            // تحديث لوحة الفحص السفلية ونافذة التفاصيل المستقلة
            if let Some(sel) = selected_meta {
                win.set_selected_filename(SharedString::from(&sel.filename));
                win.set_selected_url(SharedString::from(&sel.url));
                win.set_selected_sha256(SharedString::from(
                    sel.computed_sha256.as_deref().unwrap_or("لم يُحسب بعد"),
                ));
                win.set_selected_md5(SharedString::from(
                    sel.computed_md5.as_deref().unwrap_or("لم يُحسب بعد"),
                ));
                win.set_selected_crc32(SharedString::from(
                    sel.computed_crc32.as_deref().unwrap_or("لم يُحسب بعد"),
                ));
                let p = if sel.total_size > 0 {
                    (sel.total_downloaded() as f32 / sel.total_size as f32).clamp(0.0, 1.0)
                } else if sel.status == DownloadStatus::Completed {
                    1.0
                } else {
                    0.0
                };
                win.set_selected_progress(p);
                let last_log = sel
                    .connection_logs
                    .last()
                    .map(|l| format!("[{}] {}", l.level, redact_log_message(&l.message)))
                    .unwrap_or_else(|| sel.url.clone());
                win.set_selected_logs(SharedString::from(last_log));

                if let Some(dw) = det_weak.upgrade() {
                    dw.set_task_id(SharedString::from(&sel.task_id));
                    dw.set_window_title(SharedString::from(format!(
                        "StormGet — {:.1}% {}",
                        p * 100.0,
                        sel.filename
                    )));
                    dw.set_filename(SharedString::from(&sel.filename));
                    dw.set_url(SharedString::from(&sel.url));
                    dw.set_local_path(SharedString::from(
                        sel.final_file_path().display().to_string(),
                    ));
                    dw.set_total_size_text(SharedString::from(format_bytes(sel.total_size)));
                    dw.set_downloaded_text(SharedString::from(format_downloaded_bytes(
                        sel.total_downloaded(),
                    )));
                    dw.set_progress_ratio(p);
                    let spd: u64 = sel.smoothed_speed_bps();
                    dw.set_speed_text(SharedString::from(format_speed(spd)));
                    let eta = sel
                        .estimated_eta_secs()
                        .map(format_eta)
                        .unwrap_or_else(|| "غير متاح".to_string());
                    dw.set_eta_text(SharedString::from(eta));
                    let host = Url::parse(&sel.url)
                        .ok()
                        .and_then(|u| u.host_str().map(|h| format!("{}://{}", u.scheme(), h)))
                        .unwrap_or_else(|| "غير معروف".to_string());
                    dw.set_host_text(SharedString::from(host));
                    dw.set_resumable_text(SharedString::from(if sel.resumable {
                        "مدعوم (206 Partial Content)"
                    } else {
                        "غير مؤكد"
                    }));
                    dw.set_etag_text(SharedString::from(
                        sel.etag.as_deref().unwrap_or("غير متوفر"),
                    ));
                    dw.set_last_modified_text(SharedString::from(
                        sel.last_modified.as_deref().unwrap_or("غير متوفر"),
                    ));
                    dw.set_created_at_text(SharedString::from(
                        sel.created_at.format("%Y-%m-%d %H:%M:%S").to_string(),
                    ));
                    dw.set_updated_at_text(SharedString::from(
                        sel.updated_at.format("%Y-%m-%d %H:%M:%S").to_string(),
                    ));
                    dw.set_error_text(SharedString::from(
                        sel.last_error.clone().unwrap_or_default(),
                    ));
                    dw.set_sha256_text(SharedString::from(
                        sel.computed_sha256.as_deref().unwrap_or("لم يُحسب بعد"),
                    ));
                    dw.set_md5_text(SharedString::from(
                        sel.computed_md5.as_deref().unwrap_or("لم يُحسب بعد"),
                    ));
                    dw.set_crc32_text(SharedString::from(
                        sel.computed_crc32.as_deref().unwrap_or("لم يُحسب بعد"),
                    ));
                    let verdict = match (&sel.computed_sha256, &sel.expected_sha256) {
                        (Some(c), Some(e)) if !e.trim().is_empty() => {
                            if c.eq_ignore_ascii_case(e.trim()) {
                                "✔ البصمة متطابقة تماماً مع القيمة المرجعية"
                            } else {
                                "✖ البصمة غير متطابقة!"
                            }
                        }
                        (Some(_), _) => "تم حساب البصمات الرقمية بنجاح",
                        _ => "لم يُنفذ التحقق بعد",
                    };
                    dw.set_integrity_verdict(SharedString::from(verdict));

                    let all_logs = sel
                        .connection_logs
                        .iter()
                        .rev()
                        .map(|l| {
                            format!(
                                "[{}] [{}] {}",
                                l.timestamp.format("%H:%M:%S"),
                                l.level,
                                redact_log_message(&l.message)
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    dw.set_logs_text(SharedString::from(all_logs));

                    let seg_rows: Vec<SlintSegmentRow> = sel
                        .segments
                        .iter()
                        .map(|s| {
                            let dl = s.downloaded_bytes();
                            let tot = (s.end_byte.saturating_sub(s.start_byte)).max(1);
                            SlintSegmentRow {
                                seg_id: (s.id + 1) as i32,
                                downloaded_text: SharedString::from(format_downloaded_bytes(dl)),
                                speed_text: SharedString::from(format_speed(s.last_speed_bps)),
                                status_text: SharedString::from(match s.status {
                                    SegmentStatus::Downloading => "جارٍ التنزيل",
                                    SegmentStatus::Completed => "مكتمل ✓",
                                    SegmentStatus::Paused => "متوقف مؤقتاً",
                                    SegmentStatus::Failed => "فشل",
                                    SegmentStatus::Pending => "في الانتظار",
                                }),
                                progress_ratio: (dl as f32 / tot as f32).clamp(0.0, 1.0),
                            }
                        })
                        .collect();
                    segs_model_clone.set_vec(seg_rows);
                }
            }
        });
    }

    main_win.run().context("انتهت حلقة أحداث واجهة Slint بخطأ")
}

fn build_slint_diagnostic_report(
    state: &Arc<AppState>,
    rt: &tokio::runtime::Handle,
) -> String {
    rt.block_on(async {
        let mut out = String::new();
        out.push_str("=== StormGet v2.3.6 Diagnostic Report (Slint UI — Redacted) ===\n");
        let tools = get_tools_status(&state.base_download_dir);
        out.push_str(&format!(
            "Tools: yt-dlp={} ({:?}) | FFmpeg={} ({:?})\n",
            tools.ytdlp_installed, tools.ytdlp_path, tools.ffmpeg_installed, tools.ffmpeg_path
        ));
        let jobs = state.jobs.read().await;
        out.push_str(&format!("Total Tasks in Engine: {}\n\n", jobs.len()));
        for job in jobs.values().take(20) {
            let m = job.meta.lock().await;
            out.push_str(&format!(
                "* [{}] {} | Status: {:?} | Size: {} B | Downloaded: {} B\n",
                &m.task_id[..8.min(m.task_id.len())],
                m.filename,
                m.status,
                m.total_size,
                m.total_downloaded()
            ));
            for log in m.connection_logs.iter().rev().take(4) {
                out.push_str(&format!(
                    "  [{}] [{}] {}\n",
                    log.timestamp.format("%H:%M:%S"),
                    log.level,
                    redact_log_message(&log.message)
                ));
            }
        }
        out
    })
}
