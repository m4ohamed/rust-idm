//! محلل ومحمل قوائم MPEG-DASH (.mpd) المطور:
//! 1. دعم وراثة الخصائص وقوالب المقاطع (`SegmentTemplate`, `mimeType`, `codecs`) من مستوى `<AdaptationSet>` إلى `<Representation>` (على غرار `dlman`).
//! 2. تحليل مدة العرض القياسية ISO-8601 (`mediaPresentationDuration="PT1H2M10.5S"`) لحساب عدد المقاطع والحجم التقديري تلقائياً حتى عند غياب سمة التكرار `r="..."`.
//! 3. تجميع متسلسل مباشر على القرص (`OrderedDiskSpooler`) لمنع استنزاف الذاكرة العشوائية (Zero-OOM).

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
    pub codecs: Option<String>,
    pub bandwidth: u64,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub estimated_size_bytes: Option<u64>,
    pub initialization_url: Option<String>,
    pub segment_urls: Vec<String>,
}

/// تحليل صيغة الوقت القياسية ISO-8601 (مثل `PT1H2M10.5S` أو `PT185.2S`) إلى ثوانٍ عشرية
pub fn parse_iso8601_duration(s: &str) -> f64 {
    let s = s.trim();
    let Some(rest) = s.strip_prefix("PT").or_else(|| s.strip_prefix("P")) else {
        return 0.0;
    };
    let rest = rest.strip_prefix('T').unwrap_or(rest);

    let mut total: f64 = 0.0;
    let mut num_buf = String::new();

    for ch in rest.chars() {
        if ch.is_ascii_digit() || ch == '.' {
            num_buf.push(ch);
        } else {
            let val: f64 = num_buf.parse().unwrap_or(0.0);
            num_buf.clear();
            match ch {
                'H' | 'h' => total += val * 3600.0,
                'M' | 'm' => total += val * 60.0,
                'S' | 's' => total += val,
                _ => {}
            }
        }
    }
    total
}

#[derive(Debug, Default, Clone)]
struct InheritedTemplate {
    mime_type: Option<String>,
    codecs: Option<String>,
    initialization: Option<String>,
    media: Option<String>,
    start_number: Option<u64>,
    timescale: Option<u64>,
    duration: Option<u64>,
    repeat_count: Option<u64>,
}

pub struct DashMpdParser;

impl DashMpdParser {
    /// تحليل ملفات MPEG-DASH (.mpd) مع وراثة `<AdaptationSet>` وحساب المدة والحجم التقديري
    pub fn parse_mpd(xml_text: &str, base_url: &Url) -> Result<Vec<DashRepresentation>> {
        let mpd_dur_re = Regex::new(r#"mediaPresentationDuration="([^"]+)""#)?;
        let presentation_duration_secs = mpd_dur_re
            .captures(xml_text)
            .and_then(|c| c.get(1))
            .map(|m| parse_iso8601_duration(m.as_str()))
            .unwrap_or(0.0);

        let adapt_re = Regex::new(r"(?s)<AdaptationSet\b([^>]*)>(.*?)</AdaptationSet>")?;
        let rep_re = Regex::new(r"(?s)<Representation\b([^>]*)>(.*?)</Representation>")?;
        let rep_empty_re = Regex::new(r#"<Representation\b([^/>]*)/>"#)?;

        let mut reps = Vec::new();

        if adapt_re.is_match(xml_text) {
            for adapt_cap in adapt_re.captures_iter(xml_text) {
                let adapt_attrs = adapt_cap.get(1).map(|m| m.as_str()).unwrap_or("");
                let adapt_body = adapt_cap.get(2).map(|m| m.as_str()).unwrap_or("");

                // استخراج SegmentTemplate على مستوى AdaptationSet خارج Representation
                let body_without_reps = rep_re.replace_all(adapt_body, "");
                let inherited = Self::extract_inherited_template(adapt_attrs, &body_without_reps);

                for rep_cap in rep_re.captures_iter(adapt_body) {
                    let attrs = rep_cap.get(1).map(|m| m.as_str()).unwrap_or("");
                    let body = rep_cap.get(2).map(|m| m.as_str()).unwrap_or("");
                    if let Ok(r) = Self::parse_single_representation(
                        attrs,
                        body,
                        &inherited,
                        presentation_duration_secs,
                        base_url,
                    ) {
                        reps.push(r);
                    }
                }

                for rep_cap in rep_empty_re.captures_iter(adapt_body) {
                    let attrs = rep_cap.get(1).map(|m| m.as_str()).unwrap_or("");
                    if let Ok(r) = Self::parse_single_representation(
                        attrs,
                        "",
                        &inherited,
                        presentation_duration_secs,
                        base_url,
                    ) {
                        reps.push(r);
                    }
                }
            }
        } else {
            let empty_inherited = InheritedTemplate::default();
            for rep_cap in rep_re.captures_iter(xml_text) {
                let attrs = rep_cap.get(1).map(|m| m.as_str()).unwrap_or("");
                let body = rep_cap.get(2).map(|m| m.as_str()).unwrap_or("");
                if let Ok(r) = Self::parse_single_representation(
                    attrs,
                    body,
                    &empty_inherited,
                    presentation_duration_secs,
                    base_url,
                ) {
                    reps.push(r);
                }
            }
        }

        Ok(reps)
    }

    fn extract_inherited_template(attrs: &str, body: &str) -> InheritedTemplate {
        let mime_re = Regex::new(r#"mimeType="([^"]+)""#).ok();
        let codecs_re = Regex::new(r#"codecs="([^"]+)""#).ok();
        let init_re = Regex::new(r#"initialization="([^"]+)""#).ok();
        let media_re = Regex::new(r#"<SegmentTemplate\b[^>]*\bmedia="([^"]+)""#).ok();
        let start_re = Regex::new(r#"startNumber="(\d+)""#).ok();
        let scale_re = Regex::new(r#"timescale="(\d+)""#).ok();
        let dur_re = Regex::new(r#"duration="(\d+)""#).ok();
        let r_re = Regex::new(r#"\br="(\d+)""#).ok();

        InheritedTemplate {
            mime_type: mime_re
                .and_then(|re| re.captures(attrs))
                .and_then(|c| c.get(1))
                .map(|m| m.as_str().to_string()),
            codecs: codecs_re
                .and_then(|re| re.captures(attrs))
                .and_then(|c| c.get(1))
                .map(|m| m.as_str().to_string()),
            initialization: init_re
                .and_then(|re| re.captures(body))
                .and_then(|c| c.get(1))
                .map(|m| m.as_str().to_string()),
            media: media_re
                .and_then(|re| re.captures(body))
                .and_then(|c| c.get(1))
                .map(|m| m.as_str().to_string()),
            start_number: start_re
                .and_then(|re| re.captures(body))
                .and_then(|c| c.get(1))
                .and_then(|m| m.as_str().parse::<u64>().ok()),
            timescale: scale_re
                .and_then(|re| re.captures(body))
                .and_then(|c| c.get(1))
                .and_then(|m| m.as_str().parse::<u64>().ok()),
            duration: dur_re
                .and_then(|re| re.captures(body))
                .and_then(|c| c.get(1))
                .and_then(|m| m.as_str().parse::<u64>().ok()),
            repeat_count: r_re
                .and_then(|re| re.captures(body))
                .and_then(|c| c.get(1))
                .and_then(|m| m.as_str().parse::<u64>().ok()),
        }
    }

    fn parse_single_representation(
        attrs: &str,
        body: &str,
        inherited: &InheritedTemplate,
        presentation_duration_secs: f64,
        base_url: &Url,
    ) -> Result<DashRepresentation> {
        let id_re = Regex::new(r#"id="([^"]+)""#)?;
        let bw_re = Regex::new(r#"bandwidth="(\d+)""#)?;
        let mime_re = Regex::new(r#"mimeType="([^"]+)""#)?;
        let codecs_re = Regex::new(r#"codecs="([^"]+)""#)?;
        let w_re = Regex::new(r#"width="(\d+)""#)?;
        let h_re = Regex::new(r#"height="(\d+)""#)?;
        let base_tag_re = Regex::new(r"<BaseURL>([^<]+)</BaseURL>")?;
        let seg_url_re = Regex::new(r#"<SegmentURL\b[^>]*media="([^"]+)""#)?;
        let init_re = Regex::new(r#"initialization="([^"]+)""#)?;
        let media_tmpl_re = Regex::new(r#"media="([^"]+)""#)?;
        let start_num_re = Regex::new(r#"startNumber="(\d+)""#)?;
        let scale_re = Regex::new(r#"timescale="(\d+)""#)?;
        let dur_re = Regex::new(r#"duration="(\d+)""#)?;
        let count_re = Regex::new(r#"\br="(\d+)""#)?;

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
            .or_else(|| inherited.mime_type.clone())
            .unwrap_or_else(|| "video/mp4".to_string());
        let codecs = codecs_re
            .captures(attrs)
            .and_then(|c| c.get(1))
            .map(|m| m.as_str().to_string())
            .or_else(|| inherited.codecs.clone());
        let width = w_re
            .captures(attrs)
            .and_then(|c| c.get(1))
            .and_then(|m| m.as_str().parse::<u32>().ok());
        let height = h_re
            .captures(attrs)
            .and_then(|c| c.get(1))
            .and_then(|m| m.as_str().parse::<u32>().ok());

        let effective_base = base_tag_re
            .captures(body)
            .and_then(|c| c.get(1))
            .and_then(|m| base_url.join(m.as_str().trim()).ok())
            .unwrap_or_else(|| base_url.clone());

        let mut initialization_url = None;
        let mut segment_urls = Vec::new();

        let init_pattern = init_re
            .captures(body)
            .and_then(|c| c.get(1))
            .map(|m| m.as_str().to_string())
            .or_else(|| inherited.initialization.clone());

        if let Some(raw_init_tpl) = init_pattern {
            let raw_init = raw_init_tpl
                .replace("$RepresentationID$", &rep_id)
                .replace("$Bandwidth$", &bandwidth.to_string());
            if let Ok(u) = effective_base.join(&raw_init) {
                initialization_url = Some(u.to_string());
            }
        }

        for seg_cap in seg_url_re.captures_iter(body) {
            if let Some(m) = seg_cap.get(1) {
                if let Ok(u) = effective_base.join(m.as_str()) {
                    segment_urls.push(u.to_string());
                }
            }
        }

        if segment_urls.is_empty() {
            let media_tpl = media_tmpl_re
                .captures(body)
                .and_then(|c| c.get(1))
                .map(|m| m.as_str().to_string())
                .or_else(|| inherited.media.clone());

            if let Some(tmpl) = media_tpl {
                let start_num = start_num_re
                    .captures(body)
                    .and_then(|c| c.get(1))
                    .and_then(|m| m.as_str().parse::<u64>().ok())
                    .or(inherited.start_number)
                    .unwrap_or(1);

                let timescale = scale_re
                    .captures(body)
                    .and_then(|c| c.get(1))
                    .and_then(|m| m.as_str().parse::<u64>().ok())
                    .or(inherited.timescale)
                    .unwrap_or(1);

                let seg_duration = dur_re
                    .captures(body)
                    .and_then(|c| c.get(1))
                    .and_then(|m| m.as_str().parse::<u64>().ok())
                    .or(inherited.duration)
                    .unwrap_or(0);

                let explicit_repeat = count_re
                    .captures(body)
                    .and_then(|c| c.get(1))
                    .and_then(|m| m.as_str().parse::<u64>().ok())
                    .or(inherited.repeat_count);

                let repeat_count = if let Some(r) = explicit_repeat {
                    r
                } else if seg_duration > 0 && presentation_duration_secs > 0.0 {
                    let seg_dur_secs = seg_duration as f64 / timescale.max(1) as f64;
                    let total_segs = (presentation_duration_secs / seg_dur_secs).ceil() as u64;
                    total_segs.saturating_sub(1).min(10_000)
                } else {
                    0
                };

                for offset in 0..=repeat_count {
                    let idx = start_num + offset;
                    let time_val = offset * seg_duration;
                    let expanded = tmpl
                        .replace("$RepresentationID$", &rep_id)
                        .replace("$Bandwidth$", &bandwidth.to_string())
                        .replace("$Number$", &idx.to_string())
                        .replace("$Time$", &time_val.to_string());
                    if let Ok(u) = effective_base.join(&expanded) {
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

        let estimated_size_bytes = if bandwidth > 0 && presentation_duration_secs > 0.0 {
            Some(((bandwidth as f64 * presentation_duration_secs) / 8.0) as u64)
        } else {
            None
        };

        Ok(DashRepresentation {
            id: rep_id,
            mime_type,
            codecs,
            bandwidth,
            width,
            height,
            estimated_size_bytes,
            initialization_url,
            segment_urls,
        })
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
            m.total_size = chosen.estimated_size_bytes.unwrap_or(total_segments * 1024);
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
                let _permit = sem_clone.acquire().await.map_err(|e| anyhow!("تم إغلاق منظم التوازي: {e}"))?;
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
