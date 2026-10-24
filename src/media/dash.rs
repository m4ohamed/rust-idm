//! محلل ومحمل قوائم MPEG-DASH (.mpd) مع تجميع متسلسل على القرص (`OrderedDiskSpooler`) لمنع استنزاف الذاكرة (Zero-OOM)

use crate::{
    core::storage::{
        save_meta_to_disk_atomic, DownloadStateMeta, DownloadStatus, FileSegment, SegmentStatus,
    },
    media::hls::{MediaStreamEngine, OrderedDiskSpooler},
};
use anyhow::{anyhow, Result};
use chrono::Utc;
use regex::Regex;
use reqwest::header::HeaderMap;
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tokio::{
    fs::{self, File},
    io::AsyncWriteExt,
    sync::{mpsc, Mutex, Semaphore},
    time::sleep,
};
use url::Url;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashRepresentation {
    pub id: String,
    pub mime_type: String,
    pub bandwidth: u64,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub initialization_url: Option<String>,
    pub segment_urls: Vec<String>,
}

pub struct DashMpdParser;

impl DashMpdParser {
    /// تحليل ملفات MPEG-DASH (.mpd) التي تستخدم BaseURL أو SegmentList أو SegmentTemplate
    pub fn parse_mpd(xml_text: &str, base_url: &Url) -> Result<Vec<DashRepresentation>> {
        let mut reps = Vec::new();
        let rep_re = Regex::new(r"(?s)<Representation\b([^>]*)>(.*?)</Representation>")?;
        let id_re = Regex::new(r#"id="([^"]+)""#)?;
        let bw_re = Regex::new(r#"bandwidth="(\d+)""#)?;
        let mime_re = Regex::new(r#"mimeType="([^"]+)""#)?;
        let w_re = Regex::new(r#"width="(\d+)""#)?;
        let h_re = Regex::new(r#"height="(\d+)""#)?;
        let base_tag_re = Regex::new(r"<BaseURL>([^<]+)</BaseURL>")?;
        let seg_url_re = Regex::new(r#"<SegmentURL\b[^>]*media="([^"]+)""#)?;
        let init_re = Regex::new(r#"initialization="([^"]+)""#)?;
        let media_tmpl_re = Regex::new(r#"media="([^"]+)""#)?;
        let start_num_re = Regex::new(r#"startNumber="(\d+)""#)?;
        let count_re = Regex::new(r#"r="(\d+)""#)?;

        for cap in rep_re.captures_iter(xml_text) {
            let attrs = cap.get(1).map(|m| m.as_str()).unwrap_or("");
            let body = cap.get(2).map(|m| m.as_str()).unwrap_or("");

            let rep_id = id_re
                .captures(attrs)
                .and_then(|c| c.get(1))
                .map(|m| m.as_str().to_string())
                .unwrap_or_else(|| "rep_0".to_string());
            let bandwidth = bw_re
                .captures(attrs)
                .and_then(|c| c.get(1))
                .and_then(|m| m.as_str().parse::<u64>().ok())
                .unwrap_or(0);
            let mime_type = mime_re
                .captures(attrs)
                .and_then(|c| c.get(1))
                .map(|m| m.as_str().to_string())
                .unwrap_or_else(|| "video/mp4".to_string());
            let width = w_re
                .captures(attrs)
                .and_then(|c| c.get(1))
                .and_then(|m| m.as_str().parse::<u32>().ok());
            let height = h_re
                .captures(attrs)
                .and_then(|c| c.get(1))
                .and_then(|m| m.as_str().parse::<u32>().ok());

            let mut initialization_url = None;
            let mut segment_urls = Vec::new();

            if let Some(init_cap) = init_re.captures(body).and_then(|c| c.get(1)) {
                let raw_init = init_cap
                    .as_str()
                    .replace("$RepresentationID$", &rep_id)
                    .replace("$Bandwidth$", &bandwidth.to_string());
                if let Ok(u) = base_url.join(&raw_init) {
                    initialization_url = Some(u.to_string());
                }
            }

            for seg_cap in seg_url_re.captures_iter(body) {
                if let Some(m) = seg_cap.get(1) {
                    if let Ok(u) = base_url.join(m.as_str()) {
                        segment_urls.push(u.to_string());
                    }
                }
            }

            if segment_urls.is_empty() {
                if let Some(tmpl_cap) = media_tmpl_re.captures(body).and_then(|c| c.get(1)) {
                    let start_num = start_num_re
                        .captures(body)
                        .and_then(|c| c.get(1))
                        .and_then(|m| m.as_str().parse::<u64>().ok())
                        .unwrap_or(1);
                    let repeat_count = count_re
                        .captures(body)
                        .and_then(|c| c.get(1))
                        .and_then(|m| m.as_str().parse::<u64>().ok())
                        .unwrap_or(0);

                    for idx in start_num..=(start_num + repeat_count) {
                        let expanded = tmpl_cap
                            .as_str()
                            .replace("$RepresentationID$", &rep_id)
                            .replace("$Bandwidth$", &bandwidth.to_string())
                            .replace("$Number$", &idx.to_string());
                        if let Ok(u) = base_url.join(&expanded) {
                            segment_urls.push(u.to_string());
                        }
                    }
                }
            }

            if segment_urls.is_empty() {
                if let Some(b_cap) = base_tag_re.captures(body).and_then(|c| c.get(1)) {
                    if let Ok(u) = base_url.join(b_cap.as_str().trim()) {
                        segment_urls.push(u.to_string());
                    }
                }
            }

            reps.push(DashRepresentation {
                id: rep_id,
                mime_type,
                bandwidth,
                width,
                height,
                initialization_url,
                segment_urls,
            });
        }

        Ok(reps)
    }
}

impl MediaStreamEngine {
    /// تحميل بث MPEG-DASH (.mpd) أصلياً عبر Rust مع تفريغ المقاطع مباشرةً إلى القرص (`OrderedDiskSpooler` Zero-OOM)
    pub async fn download_dash_stream_with_progress(
        &self,
        mpd_url: &str,
        output_file: &Path,
        headers: &HeaderMap,
        representation_id: Option<&str>,
        concurrency: usize,
        meta: Arc<Mutex<DownloadStateMeta>>,
        pause_flag: Arc<AtomicBool>,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<PathBuf> {
        while pause_flag.load(Ordering::Relaxed) {
            if cancel_flag.load(Ordering::Relaxed) {
                return Err(anyhow!("تم إلغاء تحميل بث MPEG-DASH"));
            }
            sleep(Duration::from_millis(200)).await;
        }
        if cancel_flag.load(Ordering::Relaxed) {
            return Err(anyhow!("تم إلغاء تحميل بث MPEG-DASH"));
        }

        let xml_text = self
            .client
            .get(mpd_url)
            .headers(headers.clone())
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?;

        if xml_text.contains("ContentProtection")
            || xml_text.contains("cenc:pssh")
            || xml_text.contains("widevine")
        {
            return Err(anyhow!(
                "بث MPEG-DASH محمي بنظام DRM (ContentProtection) ولا يمكن فك تشفيره بدون ترخيص"
            ));
        }

        let base_url = Url::parse(mpd_url)?;
        let mut reps = DashMpdParser::parse_mpd(&xml_text, &base_url)?;
        if reps.is_empty() {
            return Err(anyhow!(
                "لم يتم العثور على أي تمثيلات (Representations) داخل ملف MPD"
            ));
        }

        reps.sort_by(|a, b| b.bandwidth.cmp(&a.bandwidth));
        let chosen = if let Some(req_id) = representation_id {
            let clean_id = req_id.strip_prefix("dash_").unwrap_or(req_id);
            reps.iter()
                .find(|r| r.id == clean_id)
                .cloned()
                .unwrap_or_else(|| reps[0].clone())
        } else {
            reps[0].clone()
        };

        if chosen.segment_urls.is_empty() {
            return Err(anyhow!(
                "قائمة مقاطع MPEG-DASH فارغة للتمثيل المختار ({})",
                chosen.id
            ));
        }

        let total_segments_usize = chosen.segment_urls.len();
        let total_segments = total_segments_usize as u64;
        {
            let mut m = meta.lock().await;
            m.status = DownloadStatus::Downloading;
            m.total_size = total_segments * 1024;
            m.push_log(
                Some(0),
                "INFO",
                format!(
                    "بدء تنزيل MPEG-DASH للتمثيل '{}' ({} مقاطع متوازية مع تفريغ مباشر على القرص)",
                    chosen.id, total_segments
                ),
            );
            m.segments = vec![FileSegment {
                id: 0,
                start_byte: 0,
                current_offset: 0,
                end_byte: (total_segments * 1024).saturating_sub(1),
                status: SegmentStatus::Downloading,
                split_count: 0,
                last_speed_bps: 0,
            }];
        }

        if let Some(parent) = output_file.parent() {
            fs::create_dir_all(parent).await?;
        }
        let mut out = File::create(output_file).await?;
        let mut init_written = 0u64;

        if let Some(ref init_url) = chosen.initialization_url {
            let init_bytes = self
                .client
                .get(init_url)
                .headers(headers.clone())
                .send()
                .await?
                .error_for_status()?
                .bytes()
                .await?;
            init_written = init_bytes.len() as u64;
            out.write_all(&init_bytes).await?;
        }

        let spool_dir = OrderedDiskSpooler::create_spool_dir(output_file).await?;
        let (tx, rx) = mpsc::unbounded_channel::<(usize, PathBuf, u64)>();
        let merger_handle = tokio::spawn(async move {
            OrderedDiskSpooler::run_sequential_merger(out, rx, total_segments_usize).await
        });

        let sem = Arc::new(Semaphore::new(concurrency.max(1)));
        let completed_count = Arc::new(AtomicU64::new(0));
        let start_inst = Instant::now();
        let mut tasks = Vec::with_capacity(total_segments_usize);

        for (idx, seg_url) in chosen.segment_urls.into_iter().enumerate() {
            let sem_clone = Arc::clone(&sem);
            let client_clone = self.client.clone();
            let headers_clone = headers.clone();
            let meta_clone = Arc::clone(&meta);
            let done_clone = Arc::clone(&completed_count);
            let pause_clone = Arc::clone(&pause_flag);
            let cancel_clone = Arc::clone(&cancel_flag);
            let spool_clone = spool_dir.clone();
            let tx_clone = tx.clone();

            tasks.push(tokio::spawn(async move {
                let _permit = sem_clone.acquire().await.unwrap();
                while pause_clone.load(Ordering::Relaxed) {
                    if cancel_clone.load(Ordering::Relaxed) {
                        return Err(anyhow!("تم إلغاء تحميل بث MPEG-DASH"));
                    }
                    let mut m = meta_clone.lock().await;
                    m.status = DownloadStatus::Paused;
                    m.updated_at = Utc::now();
                    drop(m);
                    sleep(Duration::from_millis(200)).await;
                }
                if cancel_clone.load(Ordering::Relaxed) {
                    return Err(anyhow!("تم إلغاء تحميل بث MPEG-DASH"));
                }

                let raw_bytes = client_clone
                    .get(&seg_url)
                    .headers(headers_clone)
                    .send()
                    .await?
                    .error_for_status()?
                    .bytes()
                    .await?;

                // كتابة المقطع فوراً إلى ملف مؤقت على القرص وتحرير الذاكرة العشوائية (Zero-OOM)
                let (seg_path, seg_len) =
                    OrderedDiskSpooler::write_segment_to_disk(&spool_clone, idx, &raw_bytes)
                        .await?;
                drop(raw_bytes);
                let _ = tx_clone.send((idx, seg_path, seg_len));

                let done = done_clone.fetch_add(1, Ordering::SeqCst) + 1;
                let elapsed = start_inst.elapsed().as_secs_f64().max(0.001);
                let approx_bps = ((done * 64 * 1024) as f64 / elapsed) as u64;
                {
                    let mut m = meta_clone.lock().await;
                    if let Some(s0) = m.segments.first_mut() {
                        s0.current_offset = done * 1024;
                        s0.last_speed_bps = approx_bps;
                    }
                    m.push_speed_sample(approx_bps, 1);
                }

                Ok::<(), anyhow::Error>(())
            }));
        }
        drop(tx);

        let mut worker_error: Option<anyhow::Error> = None;
        for t in tasks {
            if let Err(e) = t.await? {
                if worker_error.is_none() {
                    worker_error = Some(e);
                }
            }
        }

        if let Some(err) = worker_error {
            merger_handle.abort();
            let _ = fs::remove_dir_all(&spool_dir).await;
            return Err(err);
        }

        let merged_bytes = merger_handle.await??;
        let _ = fs::remove_dir_all(&spool_dir).await;
        let total_written = init_written + merged_bytes;

        {
            let mut m = meta.lock().await;
            m.total_size = total_written;
            if let Some(s0) = m.segments.first_mut() {
                s0.end_byte = total_written.saturating_sub(1);
                s0.current_offset = total_written;
                s0.status = SegmentStatus::Completed;
                s0.last_speed_bps = 0;
            }
            m.status = DownloadStatus::Completed;
            m.updated_at = Utc::now();
            m.push_log(
                Some(0),
                "INFO",
                format!("اكتمل تجميع ملف MPEG-DASH بنجاح ({} بايت)", total_written),
            );
            let meta_path = m.meta_file_path();
            let _ = save_meta_to_disk_atomic(&m, &meta_path).await;
        }

        Ok(output_file.to_path_buf())
    }
}
