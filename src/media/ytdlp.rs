//! جسر التكامل غير المتزامن مع `yt-dlp` (القابل للإلغاء والإيقاف المؤقت) ومحول ترجمات الفيديو (`WebVTT -> SRT`)

use crate::media::hls::MediaStreamEngine;
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::{process::Command as TokioCommand, time::sleep};

impl MediaStreamEngine {
    /// جسر تكامل غير متزامن مع yt-dlp يدعم الإلغاء الفوري (kill_on_drop) واختيار الجودة (format_id)
    #[allow(clippy::too_many_arguments)]
    pub async fn download_via_ytdlp_with_format(
        url: &str,
        output_file: &Path,
        format_selector: Option<&str>,
        cookies_header: Option<&str>,
        referer: Option<&str>,
        user_agent: Option<&str>,
        pause_flag: Arc<AtomicBool>,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<()> {
        let fmt = format_selector
            .filter(|s| !s.trim().is_empty())
            .unwrap_or("bestvideo+bestaudio/best");

        loop {
            while pause_flag.load(Ordering::Relaxed) {
                if cancel_flag.load(Ordering::Relaxed) {
                    return Err(anyhow!("تم إلغاء عملية yt-dlp"));
                }
                sleep(Duration::from_millis(200)).await;
            }

            if cancel_flag.load(Ordering::Relaxed) {
                return Err(anyhow!("تم إلغاء عملية yt-dlp"));
            }

            let mut cmd = TokioCommand::new("yt-dlp");
            cmd.kill_on_drop(true)
                .arg("--no-playlist")
                .arg("-f")
                .arg(fmt)
                .arg("--merge-output-format")
                .arg("mp4")
                .arg("-o")
                .arg(output_file);

            if let Some(cookie) = cookies_header.filter(|value| !value.is_empty()) {
                cmd.arg("--add-header").arg(format!("Cookie: {cookie}"));
            }
            if let Some(ref_url) = referer.filter(|value| !value.is_empty()) {
                cmd.arg("--referer").arg(ref_url);
            }
            if let Some(ua) = user_agent.filter(|value| !value.is_empty()) {
                cmd.arg("--user-agent").arg(ua);
            }

            let mut child = cmd
                .arg(url)
                .spawn()
                .context("تعذر تشغيل yt-dlp؛ تأكد من تثبيت yt-dlp و ffmpeg في مسار النظام")?;

            loop {
                if cancel_flag.load(Ordering::Relaxed) {
                    let _ = child.kill().await;
                    return Err(anyhow!(
                        "تم إلغاء عملية yt-dlp وإنهاء العملية الفرعية بأمان"
                    ));
                }
                if pause_flag.load(Ordering::Relaxed) {
                    child.kill().await?;
                    break;
                }
                if let Some(status) = child.try_wait()? {
                    if status.success() {
                        return Ok(());
                    }
                    return Err(anyhow!("انتهت عملية yt-dlp برمز خطأ: {:?}", status.code()));
                }
                sleep(Duration::from_millis(200)).await;
            }
        }
    }

    /// جسر تكامل غير متزامن مع yt-dlp يدعم الإلغاء الفوري (kill_on_drop) ومنع بقاء عمليات يتيمة
    pub async fn download_via_ytdlp_cancellable(
        url: &str,
        output_dir: &Path,
        cookies_header: Option<&str>,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<()> {
        let mut cmd = TokioCommand::new("yt-dlp");
        cmd.kill_on_drop(true)
            .arg("-f")
            .arg("bestvideo+bestaudio/best")
            .arg("--merge-output-format")
            .arg("mp4")
            .arg("-P")
            .arg(output_dir);

        if let Some(cookie) = cookies_header {
            if !cookie.is_empty() {
                cmd.arg("--add-header").arg(format!("Cookie: {}", cookie));
            }
        }

        let mut child = cmd
            .arg(url)
            .spawn()
            .context("تعذر تشغيل yt-dlp؛ تأكد من تثبيت yt-dlp و ffmpeg في مسار النظام")?;

        loop {
            if cancel_flag.load(Ordering::Relaxed) {
                let _ = child.kill().await;
                return Err(anyhow!(
                    "تم إلغاء عملية yt-dlp وإنهاء العملية الفرعية بأمان"
                ));
            }
            if let Some(status) = child.try_wait()? {
                if status.success() {
                    return Ok(());
                } else {
                    return Err(anyhow!("انتهت عملية yt-dlp برمز خطأ: {:?}", status.code()));
                }
            }
            sleep(Duration::from_millis(200)).await;
        }
    }
}

// ============================================================================
// محول ملفات الترجمة المدمج (WebVTT to SubRip .srt Converter)
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubtitleCue {
    pub index: usize,
    pub start_time_ms: u64,
    pub end_time_ms: u64,
    pub text: String,
}

pub struct SubtitleConverter;

impl SubtitleConverter {
    /// تحويل ملفات الترجمة بصيغة WebVTT (`.vtt`) المصاحبة لبث HLS/DASH إلى صيغة SubRip القياسية (`.srt`)
    pub fn convert_webvtt_to_srt(vtt_text: &str) -> String {
        let mut srt_out = String::new();
        let mut cue_idx = 1usize;
        let mut lines = vtt_text.lines().peekable();

        while let Some(raw_line) = lines.next() {
            let line = raw_line.trim();
            if line.is_empty()
                || line.starts_with("WEBVTT")
                || line.starts_with("NOTE")
                || line.starts_with("STYLE")
                || line.starts_with("Kind:")
                || line.starts_with("Language:")
            {
                continue;
            }

            if line.contains("-->") {
                let timing_parts: Vec<&str> = line.split("-->").collect();
                if timing_parts.len() >= 2 {
                    let start_raw = timing_parts[0].trim();
                    let end_raw = timing_parts[1]
                        .split_whitespace()
                        .next()
                        .unwrap_or("")
                        .trim();

                    let start_srt = Self::normalize_vtt_timestamp(start_raw);
                    let end_srt = Self::normalize_vtt_timestamp(end_raw);

                    let mut text_lines = Vec::new();
                    while let Some(&next_line) = lines.peek() {
                        if next_line.trim().is_empty() {
                            lines.next();
                            break;
                        }
                        text_lines.push(lines.next().unwrap().trim());
                    }

                    if !text_lines.is_empty() {
                        srt_out.push_str(&format!(
                            "{}\n{} --> {}\n{}\n\n",
                            cue_idx,
                            start_srt,
                            end_srt,
                            text_lines.join("\n")
                        ));
                        cue_idx += 1;
                    }
                }
            }
        }

        srt_out
    }

    pub fn normalize_vtt_timestamp(ts: &str) -> String {
        let replaced = ts.replace('.', ",");
        let colon_count = replaced.chars().filter(|&c| c == ':').count();
        if colon_count == 1 {
            format!("00:{}", replaced)
        } else {
            replaced
        }
    }
}
