//! فاحص أرشيفات ZIP عن بُعد عبر HTTP Range (`RemoteZipInspector`)، موسع الأنماط الدفعية (`WildcardPatternExpander`)،
//! ومستورد قوائم التحميل القياسية (`DownloadListImporter`: `.ef2`, `.txt`, `.m3u`).

use crate::core::{segmenter::DynamicDownloadJob, storage::Crc32Hasher};
use anyhow::{anyhow, Context, Result};
use regex::Regex;
use reqwest::{
    header::{HeaderMap, RANGE},
    Client,
};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::time::timeout;
use url::Url;

// ============================================================================
// 1. محرك التنزيل الدفعي وتوسيع الأنماط الرقمية والحرفية (WildcardPatternExpander)
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchExpandRequest {
    /// نمط الرابط (مثال: `https://cdn.example.com/part_[01-12].zip` أو `https://example.com/file_[a-f].pdf` أو قائمة روابط مفصولة بأسطر)
    pub pattern_or_urls: String,
    #[serde(default)]
    pub step: Option<usize>,
    #[serde(default)]
    pub queue_name: Option<String>,
    #[serde(default)]
    pub cookies: Option<String>,
    #[serde(default)]
    pub referer: Option<String>,
    #[serde(default)]
    pub start_immediately: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchExpandResponse {
    pub expanded_urls: Vec<String>,
    pub queued_task_ids: Vec<String>,
    pub skipped_duplicates: usize,
    pub errors: Vec<String>,
}

pub struct WildcardPatternExpander;

impl WildcardPatternExpander {
    /// الحد الأقصى لعدد الروابط المتولدة في الدفعة الواحدة لمنع استنزاف الذاكرة (2000 رابط)
    pub const MAX_BATCH_URLS: usize = 2000;

    /// توسيع نمط روابط يحتوي على `[start-end]` أو `[start-end:step]` أو `*` مع استبدال حجم الترقيم (`wildcard_pad_width`)
    pub fn expand_pattern(pattern: &str, default_step: usize) -> Result<Vec<String>> {
        let step_override = default_step.max(1);
        let mut results = Vec::new();
        let bracket_re = Regex::new(r"\[([0-9a-zA-Z]+)-([0-9a-zA-Z]+)(?::(\d+))?\]")?;

        for raw_line in pattern.lines() {
            let line = raw_line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            if let Some(caps) = bracket_re.captures(line) {
                let full_match = caps.get(0).unwrap();
                let start_str = caps.get(1).unwrap().as_str();
                let end_str = caps.get(2).unwrap().as_str();
                let step = caps
                    .get(3)
                    .and_then(|m| m.as_str().parse::<usize>().ok())
                    .unwrap_or(step_override)
                    .max(1);

                let prefix = &line[..full_match.start()];
                let suffix = &line[full_match.end()..];

                if let (Ok(s_num), Ok(e_num)) = (start_str.parse::<u64>(), end_str.parse::<u64>()) {
                    if s_num > e_num {
                        return Err(anyhow!(
                            "بداية النطاق الرقمي ({}) أكبر من نهايته ({})",
                            s_num,
                            e_num
                        ));
                    }
                    let pad_width = if start_str.starts_with('0') && start_str.len() > 1 {
                        start_str.len()
                    } else {
                        0
                    };
                    let mut cur = s_num;
                    while cur <= e_num {
                        if results.len() >= Self::MAX_BATCH_URLS {
                            return Err(anyhow!(
                                "تجاوز عدد الروابط المتولدة الحد الأقصى المسموح ({})",
                                Self::MAX_BATCH_URLS
                            ));
                        }
                        let num_formatted = if pad_width > 0 {
                            format!("{:0width$}", cur, width = pad_width)
                        } else {
                            cur.to_string()
                        };
                        let candidate = format!("{}{}{}", prefix, num_formatted, suffix);
                        Url::parse(&candidate).context("الرابط المتولد من النمط غير صالح")?;
                        results.push(candidate);
                        cur = cur.saturating_add(step as u64);
                    }
                    continue;
                }

                if start_str.len() == 1 && end_str.len() == 1 {
                    let s_ch = start_str.chars().next().unwrap();
                    let e_ch = end_str.chars().next().unwrap();
                    if (s_ch.is_ascii_lowercase() && e_ch.is_ascii_lowercase() && s_ch <= e_ch)
                        || (s_ch.is_ascii_uppercase() && e_ch.is_ascii_uppercase() && s_ch <= e_ch)
                    {
                        let mut code_pt = s_ch as u32;
                        let end_pt = e_ch as u32;
                        while code_pt <= end_pt {
                            if results.len() >= Self::MAX_BATCH_URLS {
                                return Err(anyhow!(
                                    "تجاوز عدد الروابط المتولدة الحد الأقصى المسموح ({})",
                                    Self::MAX_BATCH_URLS
                                ));
                            }
                            if let Some(ch) = char::from_u32(code_pt) {
                                let candidate = format!("{}{}{}", prefix, ch, suffix);
                                Url::parse(&candidate)
                                    .context("الرابط المتولد من النمط الحرفي غير صالح")?;
                                results.push(candidate);
                            }
                            code_pt += step as u32;
                        }
                        continue;
                    }
                }

                return Err(anyhow!(
                    "صيغة النطاق داخل الأقواس غير صالحة: {}",
                    full_match.as_str()
                ));
            }

            let parsed = Url::parse(line).with_context(|| format!("رابط غير صالح: {}", line))?;
            if parsed.scheme() != "http" && parsed.scheme() != "https" {
                return Err(anyhow!(
                    "بروتوكول غير مدعوم في الدفعة: {}",
                    parsed.scheme()
                ));
            }
            if results.len() >= Self::MAX_BATCH_URLS {
                return Err(anyhow!(
                    "تجاوز عدد الروابط الحد الأقصى المسموح ({})",
                    Self::MAX_BATCH_URLS
                ));
            }
            results.push(parsed.to_string());
        }

        if results.is_empty() {
            return Err(anyhow!("لم يتم العثور على أي روابط صالحة في المدخلات"));
        }
        Ok(results)
    }
}

// ============================================================================
// 2. فاحص الدليل المركزي لأرشيفات ZIP عن بُعد عبر HTTP Range (RemoteZipInspector)
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZipArchiveEntryInfo {
    pub index: usize,
    pub filename: String,
    pub compressed_size: u64,
    pub uncompressed_size: u64,
    pub compression_method: u16,
    pub crc32_hex: String,
    pub local_header_offset: u64,
    pub is_directory: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteZipPreviewResponse {
    pub url: String,
    pub archive_total_size: u64,
    pub total_entries: usize,
    pub total_uncompressed_bytes: u64,
    pub entries: Vec<ZipArchiveEntryInfo>,
}

pub struct RemoteZipInspector;

impl RemoteZipInspector {
    /// قراءة جدول الدليل المركزي (Central Directory - EOCD) لأرشيف ZIP عن بُعد باستخدام طلب Range لآخر 64 كيلوبايت فقط!
    pub async fn inspect_remote_zip(
        client: &Client,
        url: &str,
        custom_headers: &HeaderMap,
    ) -> Result<RemoteZipPreviewResponse> {
        let (total_size, resumable, _name, _etag, _lm) =
            DynamicDownloadJob::probe_url(client, url, custom_headers).await?;
        if !resumable || total_size < 22 {
            return Err(anyhow!(
                "الخادم لا يدعم طلبات النطاق (HTTP Range) أو أن حجم الملف أصغر من ترويسة ZIP القياسية"
            ));
        }

        let tail_window: u64 = 65_557.min(total_size);
        let range_start = total_size - tail_window;
        let resp = timeout(
            Duration::from_secs(15),
            client
                .get(url)
                .headers(custom_headers.clone())
                .header(RANGE, format!("bytes={}-{}", range_start, total_size - 1))
                .send(),
        )
        .await
        .context("انتهت مهلة جلب ذيل أرشيف ZIP عن بُعد")??
        .error_for_status()?;

        let tail_bytes = resp.bytes().await?;
        let entries = Self::parse_zip_central_directory(
            &tail_bytes,
            range_start,
            total_size,
            client,
            url,
            custom_headers,
        )
        .await?;
        let total_uncompressed_bytes = entries.iter().map(|e| e.uncompressed_size).sum();

        Ok(RemoteZipPreviewResponse {
            url: url.to_string(),
            archive_total_size: total_size,
            total_entries: entries.len(),
            total_uncompressed_bytes,
            entries,
        })
    }

    /// تحليل بايتات End of Central Directory (EOCD) واستخراج قائمة الملفات الداخلية
    pub async fn parse_zip_central_directory(
        tail_bytes: &[u8],
        tail_offset_in_file: u64,
        total_file_size: u64,
        client: &Client,
        url: &str,
        custom_headers: &HeaderMap,
    ) -> Result<Vec<ZipArchiveEntryInfo>> {
        if tail_bytes.len() < 22 {
            return Err(anyhow!("بيانات ذيل ZIP قصيرة جداً"));
        }

        let mut eocd_pos: Option<usize> = None;
        for i in (0..=tail_bytes.len() - 22).rev() {
            if tail_bytes[i..i + 4] == [0x50, 0x4b, 0x05, 0x06] {
                eocd_pos = Some(i);
                break;
            }
        }

        let eocd_idx = eocd_pos.ok_or_else(|| {
            anyhow!(
                "لم يتم العثور على توقيع الدليل المركزي (EOCD)؛ قد لا يكون الملف أرشيف ZIP قياسي"
            )
        })?;

        let eocd = &tail_bytes[eocd_idx..];
        let total_entries_count = u16::from_le_bytes([eocd[10], eocd[11]]) as usize;
        let cd_size = u32::from_le_bytes([eocd[12], eocd[13], eocd[14], eocd[15]]) as u64;
        let cd_offset = u32::from_le_bytes([eocd[16], eocd[17], eocd[18], eocd[19]]) as u64;

        if cd_offset + cd_size > total_file_size {
            return Err(anyhow!(
                "إزاحة الدليل المركزي لملف ZIP تتجاوز حجم الملف الكلي"
            ));
        }

        let cd_buffer: Vec<u8> = if cd_offset >= tail_offset_in_file {
            let local_start = (cd_offset - tail_offset_in_file) as usize;
            let local_end = (local_start + cd_size as usize).min(tail_bytes.len());
            tail_bytes[local_start..local_end].to_vec()
        } else {
            let cd_resp = timeout(
                Duration::from_secs(15),
                client
                    .get(url)
                    .headers(custom_headers.clone())
                    .header(
                        RANGE,
                        format!(
                            "bytes={}-{}",
                            cd_offset,
                            cd_offset + cd_size.saturating_sub(1)
                        ),
                    )
                    .send(),
            )
            .await
            .context("انتهت مهلة جلب الدليل المركزي لأرشيف ZIP")??
            .error_for_status()?;
            cd_resp.bytes().await?.to_vec()
        };

        Self::decode_central_directory_entries(&cd_buffer, total_entries_count)
    }

    /// تحليل متزامن لذاكرة أرشيف ZIP كاملة أو ذيل يحتوي على EOCD والدليل المركزي معاً
    pub fn parse_zip_central_directory_sync(tail_bytes: &[u8]) -> Result<Vec<ZipArchiveEntryInfo>> {
        if tail_bytes.len() < 22 {
            return Err(anyhow!("بيانات ZIP قصيرة جداً"));
        }
        let mut eocd_pos: Option<usize> = None;
        for i in (0..=tail_bytes.len() - 22).rev() {
            if tail_bytes[i..i + 4] == [0x50, 0x4b, 0x05, 0x06] {
                eocd_pos = Some(i);
                break;
            }
        }
        let eocd_idx = eocd_pos.ok_or_else(|| anyhow!("لم يتم العثور على توقيع EOCD"))?;
        let eocd = &tail_bytes[eocd_idx..];
        let count = u16::from_le_bytes([eocd[10], eocd[11]]) as usize;
        let cd_size = u32::from_le_bytes([eocd[12], eocd[13], eocd[14], eocd[15]]) as usize;
        let cd_offset = u32::from_le_bytes([eocd[16], eocd[17], eocd[18], eocd[19]]) as usize;
        if cd_offset + cd_size > tail_bytes.len() {
            return Err(anyhow!(
                "إزاحة الدليل المركزي خارج حدود الذاكرة المتوفرة"
            ));
        }
        Self::decode_central_directory_entries(&tail_bytes[cd_offset..cd_offset + cd_size], count)
    }

    /// فك ترميز سجلات Central Directory File Header (`0x02014b50`)
    pub fn decode_central_directory_entries(
        cd_buf: &[u8],
        expected_count: usize,
    ) -> Result<Vec<ZipArchiveEntryInfo>> {
        if expected_count > 0 && (cd_buf.len() < 46 || cd_buf[0..4] != [0x50, 0x4b, 0x01, 0x02]) {
            return Err(anyhow!(
                "توقيع سجل الدليل المركزي (0x02014b50) غير موجود عند الإزاحة المحددة"
            ));
        }
        let mut entries = Vec::with_capacity(expected_count.min(4096));
        let mut pos = 0usize;
        let mut idx = 0usize;

        while pos + 46 <= cd_buf.len() {
            if cd_buf[pos..pos + 4] != [0x50, 0x4b, 0x01, 0x02] {
                break;
            }
            let compression_method = u16::from_le_bytes([cd_buf[pos + 10], cd_buf[pos + 11]]);
            let crc32_val = u32::from_le_bytes([
                cd_buf[pos + 16],
                cd_buf[pos + 17],
                cd_buf[pos + 18],
                cd_buf[pos + 19],
            ]);
            let compressed_size = u32::from_le_bytes([
                cd_buf[pos + 20],
                cd_buf[pos + 21],
                cd_buf[pos + 22],
                cd_buf[pos + 23],
            ]) as u64;
            let uncompressed_size = u32::from_le_bytes([
                cd_buf[pos + 24],
                cd_buf[pos + 25],
                cd_buf[pos + 26],
                cd_buf[pos + 27],
            ]) as u64;
            let name_len = u16::from_le_bytes([cd_buf[pos + 28], cd_buf[pos + 29]]) as usize;
            let extra_len = u16::from_le_bytes([cd_buf[pos + 30], cd_buf[pos + 31]]) as usize;
            let comment_len = u16::from_le_bytes([cd_buf[pos + 32], cd_buf[pos + 33]]) as usize;
            let local_header_offset = u32::from_le_bytes([
                cd_buf[pos + 42],
                cd_buf[pos + 43],
                cd_buf[pos + 44],
                cd_buf[pos + 45],
            ]) as u64;

            let name_start = pos + 46;
            let name_end = (name_start + name_len).min(cd_buf.len());
            let filename = String::from_utf8_lossy(&cd_buf[name_start..name_end]).to_string();
            let is_directory = filename.ends_with('/');

            entries.push(ZipArchiveEntryInfo {
                index: idx,
                filename,
                compressed_size,
                uncompressed_size,
                compression_method,
                crc32_hex: format!("{:08x}", crc32_val),
                local_header_offset,
                is_directory,
            });

            idx += 1;
            pos = name_start + name_len + extra_len + comment_len;
        }

        Ok(entries)
    }
}

// ============================================================================
// 3. مستورد قوائم التحميل (EF2 / M3U / TXT) وباني أرشيفات الاختبار
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportedTaskItem {
    pub url: String,
    pub referer: Option<String>,
    pub cookies: Option<String>,
    pub user_agent: Option<String>,
    pub filename: Option<String>,
    pub queue_name: Option<String>,
}

pub struct DownloadListImporter;

impl DownloadListImporter {
    /// استيراد ملفات قوائم التحميل القياسية في IDM (`.ef2` و `.txt` و `.m3u`)
    pub fn parse_import_text(content: &str) -> Vec<ImportedTaskItem> {
        let mut items = Vec::new();
        if content.contains("<\n") || content.contains("<\r\n") {
            let mut in_block = false;
            let mut cur_url: Option<String> = None;
            let mut cur_ref: Option<String> = None;
            let mut cur_cookie: Option<String> = None;
            let mut cur_ua: Option<String> = None;
            let mut cur_file: Option<String> = None;
            let mut cur_queue: Option<String> = None;

            for raw_line in content.lines() {
                let line = raw_line.trim();
                if line == "<" {
                    in_block = true;
                    cur_url = None;
                    cur_ref = None;
                    cur_cookie = None;
                    cur_ua = None;
                    cur_file = None;
                    cur_queue = None;
                } else if line == ">" {
                    if in_block {
                        if let Some(u) = cur_url.take() {
                            items.push(ImportedTaskItem {
                                url: u,
                                referer: cur_ref.take(),
                                cookies: cur_cookie.take(),
                                user_agent: cur_ua.take(),
                                filename: cur_file.take(),
                                queue_name: cur_queue.take(),
                            });
                        }
                    }
                    in_block = false;
                } else if in_block {
                    let lower = line.to_lowercase();
                    if lower.starts_with("http://") || lower.starts_with("https://") {
                        if cur_url.is_none() {
                            cur_url = Some(line.to_string());
                        }
                    } else if let Some(rest) = lower.strip_prefix("referer:") {
                        let val = line[line.len() - rest.len()..].trim();
                        if !val.is_empty() {
                            cur_ref = Some(val.to_string());
                        }
                    } else if let Some(rest) = lower.strip_prefix("cookie:") {
                        let val = line[line.len() - rest.len()..].trim();
                        if !val.is_empty() {
                            cur_cookie = Some(val.to_string());
                        }
                    } else if let Some(rest) = lower.strip_prefix("user-agent:") {
                        let val = line[line.len() - rest.len()..].trim();
                        if !val.is_empty() {
                            cur_ua = Some(val.to_string());
                        }
                    } else if let Some(rest) = lower.strip_prefix("filename:") {
                        let val = line[line.len() - rest.len()..].trim();
                        if !val.is_empty() {
                            cur_file = Some(val.to_string());
                        }
                    } else if let Some(rest) = lower.strip_prefix("queue:") {
                        let val = line[line.len() - rest.len()..].trim();
                        if !val.is_empty() {
                            cur_queue = Some(val.to_string());
                        }
                    }
                }
            }
        } else {
            for raw_line in content.lines() {
                let line = raw_line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                if line.starts_with("http://") || line.starts_with("https://") {
                    if Url::parse(line).is_ok() {
                        items.push(ImportedTaskItem {
                            url: line.to_string(),
                            referer: None,
                            cookies: None,
                            user_agent: None,
                            filename: None,
                            queue_name: None,
                        });
                    }
                }
            }
        }
        items
    }

    /// بناء أرشيف ZIP قياسي مصغر في الذاكرة لاختبار محلل الدليل المركزي عن بُعد (RemoteZipInspector)
    pub fn build_synthetic_zip_archive(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut archive = Vec::new();
        let mut central_dir = Vec::new();
        let mut offsets = Vec::with_capacity(files.len());

        for &(name, data) in files {
            let local_offset = archive.len() as u32;
            offsets.push(local_offset);

            let mut crc_hasher = Crc32Hasher::new();
            crc_hasher.update(data);
            let crc = crc_hasher.finalize();
            let size = data.len() as u32;
            let name_bytes = name.as_bytes();
            let name_len = name_bytes.len() as u16;

            // Local File Header (0x04034b50)
            archive.extend_from_slice(&[0x50, 0x4b, 0x03, 0x04]);
            archive.extend_from_slice(&20u16.to_le_bytes());
            archive.extend_from_slice(&0u16.to_le_bytes());
            archive.extend_from_slice(&0u16.to_le_bytes());
            archive.extend_from_slice(&0u16.to_le_bytes());
            archive.extend_from_slice(&0u16.to_le_bytes());
            archive.extend_from_slice(&crc.to_le_bytes());
            archive.extend_from_slice(&size.to_le_bytes());
            archive.extend_from_slice(&size.to_le_bytes());
            archive.extend_from_slice(&name_len.to_le_bytes());
            archive.extend_from_slice(&0u16.to_le_bytes());
            archive.extend_from_slice(name_bytes);
            archive.extend_from_slice(data);

            // Central Directory File Header (0x02014b50)
            central_dir.extend_from_slice(&[0x50, 0x4b, 0x01, 0x02]);
            central_dir.extend_from_slice(&20u16.to_le_bytes());
            central_dir.extend_from_slice(&20u16.to_le_bytes());
            central_dir.extend_from_slice(&0u16.to_le_bytes());
            central_dir.extend_from_slice(&0u16.to_le_bytes());
            central_dir.extend_from_slice(&0u16.to_le_bytes());
            central_dir.extend_from_slice(&0u16.to_le_bytes());
            central_dir.extend_from_slice(&crc.to_le_bytes());
            central_dir.extend_from_slice(&size.to_le_bytes());
            central_dir.extend_from_slice(&size.to_le_bytes());
            central_dir.extend_from_slice(&name_len.to_le_bytes());
            central_dir.extend_from_slice(&0u16.to_le_bytes());
            central_dir.extend_from_slice(&0u16.to_le_bytes());
            central_dir.extend_from_slice(&0u16.to_le_bytes());
            central_dir.extend_from_slice(&0u16.to_le_bytes());
            central_dir.extend_from_slice(&0u32.to_le_bytes());
            central_dir.extend_from_slice(&local_offset.to_le_bytes());
            central_dir.extend_from_slice(name_bytes);
        }

        let cd_offset = archive.len() as u32;
        let cd_size = central_dir.len() as u32;
        let count = files.len() as u16;
        archive.extend_from_slice(&central_dir);

        // End of Central Directory Record (EOCD 0x06054b50)
        archive.extend_from_slice(&[0x50, 0x4b, 0x05, 0x06]);
        archive.extend_from_slice(&0u16.to_le_bytes());
        archive.extend_from_slice(&0u16.to_le_bytes());
        archive.extend_from_slice(&count.to_le_bytes());
        archive.extend_from_slice(&count.to_le_bytes());
        archive.extend_from_slice(&cd_size.to_le_bytes());
        archive.extend_from_slice(&cd_offset.to_le_bytes());
        archive.extend_from_slice(&0u16.to_le_bytes());

        archive
    }
}
