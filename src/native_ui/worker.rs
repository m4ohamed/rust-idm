use super::types::{UiCommand, UiEvent};
use crate::{
    AppConfig, AppState, BatchExpandResponse, ControlTaskPayload, CrawlReport, DownloadStateMeta,
    EngineStatistics, QueueConfig, QueueSchedule, RemoteZipPreviewResponse,
};
use anyhow::{anyhow, Context, Result};
use reqwest::blocking::Client;
use std::{
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, RecvTimeoutError, Sender},
    thread,
    time::{Duration, Instant},
};
use tokio::runtime::Builder;

const API_BASE: &str = "http://127.0.0.1:16888/api/v1";

pub fn open_file(path: &Path) {
    #[cfg(target_os = "windows")]
    let _ = std::process::Command::new("cmd")
        .args(["/c", "start", "", &path.to_string_lossy()])
        .spawn();

    #[cfg(not(target_os = "windows"))]
    let _ = std::process::Command::new("xdg-open").arg(path).spawn();
}

pub fn open_file_with(path: &Path) {
    #[cfg(target_os = "windows")]
    let _ = std::process::Command::new("rundll32")
        .args(["shell32.dll,OpenAs_RunDLL", &path.to_string_lossy()])
        .spawn();
}

pub fn reveal_in_explorer(path: &Path) {
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

pub fn spawn_api_worker(commands: Receiver<UiCommand>, events: Sender<UiEvent>) {
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
                    .get(format!("{API_BASE}/pending-intercepts"))
                    .send()
                    .and_then(reqwest::blocking::Response::error_for_status)
                    .and_then(|response| response.json::<Vec<crate::PendingIntercept>>())
                {
                    Ok(pending) => {
                        let _ = events.send(UiEvent::PendingIntercepts(pending));
                    }
                    Err(error) => {
                        let _ = events.send(UiEvent::Status(Err(format!(
                            "Cannot read pending downloads: {error}"
                        ))));
                    }
                }
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
        UiCommand::AddDownload {
            payload,
            save_dir,
            start_immediately,
        } => client
            .post(format!("{API_BASE}/enqueue"))
            .json(&serde_json::json!({
                "payload": payload,
                "save_dir": save_dir,
                "start_immediately": start_immediately
            }))
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
        UiCommand::ResolvePendingIntercept {
            id,
            action,
            save_dir,
        } => client
            .post(format!("{API_BASE}/pending-intercepts/{id}"))
            .json(&serde_json::json!({ "action": action, "save_dir": save_dir }))
            .send()
            .and_then(reqwest::blocking::Response::error_for_status)
            .map(|_| format!("Pending download {action}"))
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
                    "Verified CRC32={} | SHA256={}",
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
                "Engine configuration saved".to_owned()
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
                    "Removed {} completed task records",
                    reply["count"].as_u64().unwrap_or(0)
                )
            })
            .map_err(|error| error.to_string()),
        UiCommand::RemoveDownload(task_id) => client
            .delete(format!("{API_BASE}/downloads/{task_id}"))
            .send()
            .and_then(reqwest::blocking::Response::error_for_status)
            .map(|_| "Download task record removed".to_owned())
            .map_err(|error| error.to_string()),
        UiCommand::Redownload(task_id) => client
            .post(format!("{API_BASE}/downloads/{task_id}/redownload"))
            .send()
            .and_then(reqwest::blocking::Response::error_for_status)
            .map(|_| "Download restarted from beginning".to_owned())
            .map_err(|error| error.to_string()),
        UiCommand::DeleteWithFile(task_id) => client
            .delete(format!("{API_BASE}/downloads/{task_id}?delete_file=true"))
            .send()
            .and_then(reqwest::blocking::Response::error_for_status)
            .map(|_| "Task record and local file deleted".to_owned())
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

pub fn start_engine() -> Result<()> {
    // التحقق أولاً إذا كان المحرك قيد التشغيل بالفعل في الخلفية أو عبر جلسة أخرى
    let check_client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_millis(500))
        .build()
        .unwrap_or_else(|_| reqwest::blocking::Client::new());
    if check_client.get(format!("{API_BASE}/stats")).send().is_ok() {
        eprintln!("RustIDM engine is already active on 127.0.0.1:16888; attaching desktop UI");
        return Ok(());
    }

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
        Err(error) => Err(anyhow!("Timed out waiting for local engine startup: {error}")),
    }
}

pub fn download_directory() -> PathBuf {
    crate::default_download_directory()
}
