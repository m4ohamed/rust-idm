//! محرك بث HLS (RFC 8216 + AES-128-CBC) مع مجمع المقاطع المتسلسل على القرص (`OrderedDiskSpooler`) لمنع استنزاف الذاكرة (Zero-OOM)

use crate::core::storage::{DownloadStateMeta, DownloadStatus, FileSegment, SegmentStatus};
use aes::cipher::{block_padding::Pkcs7, BlockDecryptMut, KeyIvInit};
use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use regex::Regex;
use reqwest::{
    header::{HeaderMap, RANGE},
    Client,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap},
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
use uuid::Uuid;

type Aes128CbcDec = cbc::Decryptor<aes::Aes128>;

// ============================================================================
// 1. مجمع المقاطع المتسلسل على القرص لمنع استنزاف الذاكرة (Zero-OOM OrderedDiskSpooler)
// ============================================================================

/// يكتب كل مقطع فيديو فور تنزيله وفك تشفيره إلى ملف مؤقت مستقل على القرص (`temp_seg_XXXXXX.part`)،
/// ويدمج المقاطع الجاهزة بالترتيب المتسلسل (`0, 1, 2, ...`) مباشرةً داخل الملف النهائي مع حذف كل ملف مؤقت فور دمجه!
/// هذا التصميم يجعل استهلاك الذاكرة العشوائية (RAM) ثابتاً ومحدوداً بعدد الاتصالات المتزامنة فقط حتى لو كان حجم الفيلم 50GB.
pub struct OrderedDiskSpooler;

impl OrderedDiskSpooler {
    /// إنشاء مجلد تخزين مؤقت فريد بجانب الملف النهائي لتفريغ المقاطع المنزلة فيه
    pub async fn create_spool_dir(output_file: &Path) -> Result<PathBuf> {
        let parent = output_file.parent().unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent).await?;
        let stem = output_file
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("stream");
        let spool_dir = parent.join(format!(
            ".{}.spool_{}",
            stem,
            &Uuid::new_v4().to_string()[..8]
        ));
        fs::create_dir_all(&spool_dir).await?;
        Ok(spool_dir)
    }

    /// كتابة مقطع واحد إلى القرص الصلب وتحرير ذاكرته العشوائية فوراً
    pub async fn write_segment_to_disk(
        spool_dir: &Path,
        idx: usize,
        bytes: &[u8],
    ) -> Result<(PathBuf, u64)> {
        let seg_path = spool_dir.join(format!("temp_seg_{:06}.ts", idx));
        let mut f = File::create(&seg_path).await?;
        f.write_all(bytes).await?;
        f.flush().await?;
        Ok((seg_path, bytes.len() as u64))
    }

    /// تشغيل حلقة دمج متسلسلة غير متزامنة تقرأ المقاطع الجاهزة بالترتيب وتفرغها في الملف النهائي ثم تحذف الملف المؤقت فوراً
    pub async fn run_sequential_merger(
        mut out_file: File,
        mut rx: mpsc::UnboundedReceiver<(usize, PathBuf, u64)>,
        total_segments: usize,
    ) -> Result<u64> {
        let mut pending_on_disk: BTreeMap<usize, (PathBuf, u64)> = BTreeMap::new();
        let mut next_expected_idx = 0usize;
        let mut total_written = 0u64;

        while next_expected_idx < total_segments {
            let Some((idx, seg_path, seg_len)) = rx.recv().await else {
                break;
            };
            pending_on_disk.insert(idx, (seg_path, seg_len));

            while let Some((ready_path, _ready_len)) = pending_on_disk.remove(&next_expected_idx) {
                let mut seg_file = File::open(&ready_path)
                    .await
                    .with_context(|| format!("فشل فتح ملف المقطع المؤقت {:?}", ready_path))?;
                let copied = tokio::io::copy(&mut seg_file, &mut out_file).await?;
                total_written += copied;
                drop(seg_file);
                let _ = fs::remove_file(&ready_path).await;
                next_expected_idx += 1;
            }
        }

        out_file.flush().await?;
        out_file.sync_all().await?;
        Ok(total_written)
    }
}

// ============================================================================
// 2. هياكل ومحلل قوائم HLS (RFC 8216 + AES-128-CBC)
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HlsVariant {
    pub bandwidth: u64,
    pub resolution: Option<String>,
    pub audio_group: Option<String>,
    pub uri: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HlsMediaRendition {
    pub media_type: String, // AUDIO | SUBTITLES
    pub group_id: String,
    pub name: String,
    pub uri: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HlsEncryptionMethod {
    None,
    Aes128 {
        key_uri: String,
        iv: Option<[u8; 16]>,
    },
    UnsupportedDrm(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HlsSegmentItem {
    pub sequence_number: u64,
    pub duration_secs: f64,
    pub uri: String,
    pub byte_range: Option<(u64, u64)>, // (length, offset)
    pub encryption: HlsEncryptionMethod,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedMediaPlaylist {
    pub init_map_uri: Option<String>,
    pub media_sequence_start: u64,
    pub is_live_stream: bool,
    pub segments: Vec<HlsSegmentItem>,
}

pub struct MediaStreamEngine {
    pub client: Client,
}

impl MediaStreamEngine {
    pub fn new(client: Client) -> Self {
        Self { client }
    }

    /// فك تشفير مقطع HLS مشفر بمعيار AES-128-CBC وفق RFC 8216 القسم 5.2
    /// إذا لم يُذكر IV صراحةً في #EXT-X-KEY، يُشتق كرقم تسلسل المقطع (Media Sequence Number) بصيغة 128-bit Big-Endian!
    pub fn decrypt_hls_aes128_segment(
        ciphertext: &[u8],
        key: &[u8; 16],
        explicit_iv: Option<[u8; 16]>,
        sequence_number: u64,
    ) -> Result<Vec<u8>> {
        let iv = explicit_iv.unwrap_or_else(|| {
            let mut buf = [0u8; 16];
            buf[8..16].copy_from_slice(&sequence_number.to_be_bytes());
            buf
        });

        let mut buffer = ciphertext.to_vec();
        let decrypted = Aes128CbcDec::new(key.into(), (&iv).into())
            .decrypt_padded_mut::<Pkcs7>(&mut buffer)
            .map_err(|_| {
                anyhow!("فشل فك تشفير مقطع HLS AES-128-CBC: المفتاح غير صالح أو البيانات تالفة")
            })?;

        Ok(decrypted.to_vec())
    }

    /// تحليل قائمة HLS الرئيسية (Master Playlist) بما في ذلك #EXT-X-STREAM-INF و #EXT-X-MEDIA
    pub fn parse_master_playlist(
        text: &str,
        base_url: &Url,
    ) -> Result<(Vec<HlsVariant>, Vec<HlsMediaRendition>)> {
        let mut variants = Vec::new();
        let mut renditions = Vec::new();

        let bw_re = Regex::new(r"BANDWIDTH=(\d+)")?;
        let res_re = Regex::new(r"RESOLUTION=([0-9]+x[0-9]+)")?;
        let audio_re = Regex::new(r#"AUDIO="([^"]+)""#)?;
        let type_re = Regex::new(r"TYPE=([A-Z]+)")?;
        let group_re = Regex::new(r#"GROUP-ID="([^"]+)""#)?;
        let name_re = Regex::new(r#"NAME="([^"]+)""#)?;
        let uri_re = Regex::new(r#"URI="([^"]+)""#)?;

        let mut lines = text.lines().peekable();
        while let Some(line) = lines.next() {
            let trimmed = line.trim();
            if trimmed.starts_with("#EXT-X-MEDIA:") {
                let media_type = type_re
                    .captures(trimmed)
                    .and_then(|c| c.get(1))
                    .map(|m| m.as_str().to_string())
                    .unwrap_or_default();
                let group_id = group_re
                    .captures(trimmed)
                    .and_then(|c| c.get(1))
                    .map(|m| m.as_str().to_string())
                    .unwrap_or_default();
                let name = name_re
                    .captures(trimmed)
                    .and_then(|c| c.get(1))
                    .map(|m| m.as_str().to_string())
                    .unwrap_or_default();
                let uri = uri_re
                    .captures(trimmed)
                    .and_then(|c| c.get(1))
                    .and_then(|m| base_url.join(m.as_str()).ok())
                    .map(|u| u.to_string());

                renditions.push(HlsMediaRendition {
                    media_type,
                    group_id,
                    name,
                    uri,
                });
            } else if trimmed.starts_with("#EXT-X-STREAM-INF:") {
                let bandwidth = bw_re
                    .captures(trimmed)
                    .and_then(|c| c.get(1))
                    .and_then(|m| m.as_str().parse::<u64>().ok())
                    .unwrap_or(0);
                let resolution = res_re
                    .captures(trimmed)
                    .and_then(|c| c.get(1))
                    .map(|m| m.as_str().to_string());
                let audio_group = audio_re
                    .captures(trimmed)
                    .and_then(|c| c.get(1))
                    .map(|m| m.as_str().to_string());

                while let Some(next_line) = lines.next() {
                    let next_trimmed = next_line.trim();
                    if !next_trimmed.is_empty() && !next_trimmed.starts_with('#') {
                        let resolved = base_url.join(next_trimmed)?.to_string();
                        variants.push(HlsVariant {
                            bandwidth,
                            resolution,
                            audio_group,
                            uri: resolved,
                        });
                        break;
                    }
                }
            }
        }
        Ok((variants, renditions))
    }

    /// تحليل قائمة وسائط HLS (Media Playlist) مع دعم #EXT-X-MAP و #EXT-X-BYTERANGE و #EXT-X-KEY و #EXT-X-ENDLIST
    pub fn parse_media_playlist(text: &str, base_url: &Url) -> Result<ParsedMediaPlaylist> {
        let mut init_map_uri = None;
        let mut media_seq: u64 = 0;
        let mut has_endlist = false;
        let mut current_enc = HlsEncryptionMethod::None;
        let mut pending_duration = 0.0f64;
        let mut pending_byterange: Option<(u64, u64)> = None;
        let mut last_byterange_end: u64 = 0;
        let mut segments = Vec::new();

        let map_re = Regex::new(r#"#EXT-X-MAP:.*URI="([^"]+)""#)?;
        let seq_re = Regex::new(r"#EXT-X-MEDIA-SEQUENCE:(\d+)")?;
        let inf_re = Regex::new(r"#EXTINF:([0-9.]+)")?;
        let br_re = Regex::new(r"#EXT-X-BYTERANGE:(\d+)(?:@(\d+))?")?;
        let method_re = Regex::new(r"METHOD=([A-Z0-9-]+)")?;
        let uri_re = Regex::new(r#"URI="([^"]+)""#)?;
        let iv_re = Regex::new(r"IV=0[xX]([0-9a-fA-F]{32})")?;

        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if trimmed.starts_with("#EXT-X-MAP:") {
                if let Some(cap) = map_re.captures(trimmed).and_then(|c| c.get(1)) {
                    init_map_uri = Some(base_url.join(cap.as_str())?.to_string());
                }
            } else if trimmed.starts_with("#EXT-X-MEDIA-SEQUENCE:") {
                if let Some(seq) = seq_re
                    .captures(trimmed)
                    .and_then(|c| c.get(1))
                    .and_then(|m| m.as_str().parse::<u64>().ok())
                {
                    media_seq = seq;
                }
            } else if trimmed.starts_with("#EXTINF:") {
                pending_duration = inf_re
                    .captures(trimmed)
                    .and_then(|c| c.get(1))
                    .and_then(|m| m.as_str().parse::<f64>().ok())
                    .unwrap_or(0.0);
            } else if trimmed.starts_with("#EXT-X-BYTERANGE:") {
                if let Some(caps) = br_re.captures(trimmed) {
                    let len = caps
                        .get(1)
                        .and_then(|m| m.as_str().parse::<u64>().ok())
                        .unwrap_or(0);
                    let offset = caps
                        .get(2)
                        .and_then(|m| m.as_str().parse::<u64>().ok())
                        .unwrap_or(last_byterange_end);
                    last_byterange_end = offset + len;
                    pending_byterange = Some((len, offset));
                }
            } else if trimmed.starts_with("#EXT-X-KEY:") {
                let method = method_re
                    .captures(trimmed)
                    .and_then(|c| c.get(1))
                    .map(|m| m.as_str())
                    .unwrap_or("NONE");

                match method {
                    "NONE" => current_enc = HlsEncryptionMethod::None,
                    "AES-128" => {
                        let key_uri = uri_re
                            .captures(trimmed)
                            .and_then(|c| c.get(1))
                            .ok_or_else(|| anyhow!("وسم #EXT-X-KEY يفتقر إلى URI المفتاح"))?;
                        let resolved_key_uri = base_url.join(key_uri.as_str())?.to_string();

                        let iv =
                            if let Some(iv_hex) = iv_re.captures(trimmed).and_then(|c| c.get(1)) {
                                let decoded = hex::decode(iv_hex.as_str())?;
                                let mut arr = [0u8; 16];
                                arr.copy_from_slice(&decoded[..16]);
                                Some(arr)
                            } else {
                                None
                            };

                        current_enc = HlsEncryptionMethod::Aes128 {
                            key_uri: resolved_key_uri,
                            iv,
                        };
                    }
                    other => {
                        current_enc = HlsEncryptionMethod::UnsupportedDrm(other.to_string());
                    }
                }
            } else if trimmed == "#EXT-X-ENDLIST" {
                has_endlist = true;
            } else if !trimmed.starts_with('#') {
                if let HlsEncryptionMethod::UnsupportedDrm(ref drm_name) = current_enc {
                    return Err(anyhow!(
                        "قائمة البث محمية بنظام تشفير غير مدعوم أو DRM ({})؛ لا يمكن فك تشفيرها بدون تصريح قياسي",
                        drm_name
                    ));
                }
                let resolved = base_url.join(trimmed)?.to_string();
                let seq_num = media_seq + segments.len() as u64;
                segments.push(HlsSegmentItem {
                    sequence_number: seq_num,
                    duration_secs: pending_duration,
                    uri: resolved,
                    byte_range: pending_byterange.take(),
                    encryption: current_enc.clone(),
                });
                pending_duration = 0.0;
            }
        }

        Ok(ParsedMediaPlaylist {
            init_map_uri,
            media_sequence_start: media_seq,
            is_live_stream: !has_endlist,
            segments,
        })
    }

    /// تحميل بث HLS (.m3u8) عبر `OrderedDiskSpooler` (تفريغ فوري للمقاطع على القرص ودمج تتابعي لمنع استنزاف الذاكرة OOM)
    pub async fn download_hls_stream(
        &self,
        m3u8_url: &str,
        output_file: &Path,
        headers: &HeaderMap,
        concurrency: usize,
    ) -> Result<PathBuf> {
        let playlist_text = self
            .client
            .get(m3u8_url)
            .headers(headers.clone())
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?;

        let base_url = Url::parse(m3u8_url)?;

        if playlist_text.contains("#EXT-X-STREAM-INF") {
            let (variants, _renditions) = Self::parse_master_playlist(&playlist_text, &base_url)?;
            let best_variant = variants
                .into_iter()
                .max_by_key(|v| v.bandwidth)
                .ok_or_else(|| anyhow!("لم يتم العثور على جودات صالحة في ملف m3u8 الرئيسي"))?;

            return Box::pin(self.download_hls_stream(
                &best_variant.uri,
                output_file,
                headers,
                concurrency,
            ))
            .await;
        }

        let parsed = Self::parse_media_playlist(&playlist_text, &base_url)?;
        if parsed.segments.is_empty() {
            return Err(anyhow!("قائمة مقاطع الفيديو فارغة"));
        }

        if let Some(parent) = output_file.parent() {
            fs::create_dir_all(parent).await?;
        }
        let mut out = File::create(output_file).await?;

        if let Some(ref map_uri) = parsed.init_map_uri {
            let init_bytes = self
                .client
                .get(map_uri)
                .headers(headers.clone())
                .send()
                .await?
                .error_for_status()?
                .bytes()
                .await?;
            out.write_all(&init_bytes).await?;
        }

        let total_segments = parsed.segments.len();
        let spool_dir = OrderedDiskSpooler::create_spool_dir(output_file).await?;
        let (tx, rx) = mpsc::unbounded_channel::<(usize, PathBuf, u64)>();

        // تشغيل المدمج التتابعي على القرص بالتوازي مع عمال التحميل
        let merger_handle = tokio::spawn(async move {
            OrderedDiskSpooler::run_sequential_merger(out, rx, total_segments).await
        });

        let key_cache: Arc<Mutex<HashMap<String, [u8; 16]>>> = Arc::new(Mutex::new(HashMap::new()));
        let sem = Arc::new(Semaphore::new(concurrency.max(1)));
        let mut tasks = Vec::with_capacity(total_segments);

        for (idx, seg) in parsed.segments.into_iter().enumerate() {
            let sem_clone = Arc::clone(&sem);
            let client_clone = self.client.clone();
            let headers_clone = headers.clone();
            let cache_clone = Arc::clone(&key_cache);
            let spool_clone = spool_dir.clone();
            let tx_clone = tx.clone();

            tasks.push(tokio::spawn(async move {
                let _permit = sem_clone.acquire().await.unwrap();
                let mut req = client_clone.get(&seg.uri).headers(headers_clone.clone());
                if let Some((len, offset)) = seg.byte_range {
                    req = req.header(RANGE, format!("bytes={}-{}", offset, offset + len - 1));
                }

                let raw_bytes = req.send().await?.error_for_status()?.bytes().await?;

                let final_bytes = match seg.encryption {
                    HlsEncryptionMethod::None => raw_bytes.to_vec(),
                    HlsEncryptionMethod::Aes128 { key_uri, iv } => {
                        let key_arr = {
                            let mut guard = cache_clone.lock().await;
                            if let Some(k) = guard.get(&key_uri) {
                                *k
                            } else {
                                let k_bytes = client_clone
                                    .get(&key_uri)
                                    .headers(headers_clone)
                                    .send()
                                    .await?
                                    .error_for_status()?
                                    .bytes()
                                    .await?;
                                if k_bytes.len() != 16 {
                                    return Err(anyhow!(
                                        "طول مفتاح HLS AES-128 غير صالح ({} بايت، المتوقع 16 بايت)",
                                        k_bytes.len()
                                    ));
                                }
                                let mut arr = [0u8; 16];
                                arr.copy_from_slice(&k_bytes[..16]);
                                guard.insert(key_uri, arr);
                                arr
                            }
                        };
                        Self::decrypt_hls_aes128_segment(
                            &raw_bytes,
                            &key_arr,
                            iv,
                            seg.sequence_number,
                        )?
                    }
                    HlsEncryptionMethod::UnsupportedDrm(d) => {
                        return Err(anyhow!("مقطع محمي بـ DRM غير مدعوم: {}", d));
                    }
                };

                // كتابة المقطع فوراً إلى ملف مؤقت على القرص وتحرير الذاكرة العشوائية (Zero-OOM)
                let (seg_path, seg_len) =
                    OrderedDiskSpooler::write_segment_to_disk(&spool_clone, idx, &final_bytes)
                        .await?;
                drop(final_bytes);
                let _ = tx_clone.send((idx, seg_path, seg_len));
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

        let merge_res = merger_handle.await?;
        let _ = fs::remove_dir_all(&spool_dir).await;
        merge_res?;

        Ok(output_file.to_path_buf())
    }

    /// تحميل بث HLS مع تحديث حالة المهمة لحظياً وتفريغ المقاطع على القرص (`OrderedDiskSpooler` لمنع استنزاف الذاكرة OOM)
    pub async fn download_hls_stream_with_progress(
        &self,
        m3u8_url: &str,
        output_file: &Path,
        headers: &HeaderMap,
        concurrency: usize,
        meta: Arc<Mutex<DownloadStateMeta>>,
        pause_flag: Arc<AtomicBool>,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<PathBuf> {
        while pause_flag.load(Ordering::Relaxed) {
            if cancel_flag.load(Ordering::Relaxed) {
                return Err(anyhow!("تم إلغاء تحميل بث HLS"));
            }
            sleep(Duration::from_millis(200)).await;
        }
        if cancel_flag.load(Ordering::Relaxed) {
            return Err(anyhow!("تم إلغاء تحميل بث HLS"));
        }

        let playlist_text = self
            .client
            .get(m3u8_url)
            .headers(headers.clone())
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?;

        let base_url = Url::parse(m3u8_url)?;

        if playlist_text.contains("#EXT-X-STREAM-INF") {
            let (variants, _renditions) = Self::parse_master_playlist(&playlist_text, &base_url)?;
            let best_variant = variants
                .into_iter()
                .max_by_key(|v| v.bandwidth)
                .ok_or_else(|| anyhow!("لم يتم العثور على جودات صالحة في ملف m3u8 الرئيسي"))?;

            return Box::pin(self.download_hls_stream_with_progress(
                &best_variant.uri,
                output_file,
                headers,
                concurrency,
                meta,
                pause_flag,
                cancel_flag,
            ))
            .await;
        }

        let parsed = Self::parse_media_playlist(&playlist_text, &base_url)?;
        if parsed.segments.is_empty() {
            return Err(anyhow!("قائمة مقاطع الفيديو فارغة"));
        }

        let total_segments_usize = parsed.segments.len();
        let total_segments = total_segments_usize as u64;
        {
            let mut m = meta.lock().await;
            m.status = DownloadStatus::Downloading;
            m.total_size = total_segments * 1024;
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

        if let Some(ref map_uri) = parsed.init_map_uri {
            let init_bytes = self
                .client
                .get(map_uri)
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

        let key_cache: Arc<Mutex<HashMap<String, [u8; 16]>>> = Arc::new(Mutex::new(HashMap::new()));
        let sem = Arc::new(Semaphore::new(concurrency.max(1)));
        let completed_count = Arc::new(AtomicU64::new(0));
        let start_inst = Instant::now();
        let mut tasks = Vec::with_capacity(total_segments_usize);

        for (idx, seg) in parsed.segments.into_iter().enumerate() {
            let sem_clone = Arc::clone(&sem);
            let client_clone = self.client.clone();
            let headers_clone = headers.clone();
            let cache_clone = Arc::clone(&key_cache);
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
                        return Err(anyhow!("تم إلغاء تحميل بث HLS"));
                    }
                    let mut m = meta_clone.lock().await;
                    m.status = DownloadStatus::Paused;
                    m.updated_at = Utc::now();
                    drop(m);
                    sleep(Duration::from_millis(200)).await;
                }
                if cancel_clone.load(Ordering::Relaxed) {
                    return Err(anyhow!("تم إلغاء تحميل بث HLS"));
                }
                {
                    let mut m = meta_clone.lock().await;
                    m.status = DownloadStatus::Downloading;
                    m.updated_at = Utc::now();
                }

                let mut req = client_clone.get(&seg.uri).headers(headers_clone.clone());
                if let Some((len, offset)) = seg.byte_range {
                    req = req.header(RANGE, format!("bytes={}-{}", offset, offset + len - 1));
                }

                let raw_bytes = req.send().await?.error_for_status()?.bytes().await?;

                let final_bytes = match seg.encryption {
                    HlsEncryptionMethod::None => raw_bytes.to_vec(),
                    HlsEncryptionMethod::Aes128 { key_uri, iv } => {
                        let key_arr = {
                            let mut guard = cache_clone.lock().await;
                            if let Some(k) = guard.get(&key_uri) {
                                *k
                            } else {
                                let k_bytes = client_clone
                                    .get(&key_uri)
                                    .headers(headers_clone)
                                    .send()
                                    .await?
                                    .error_for_status()?
                                    .bytes()
                                    .await?;
                                if k_bytes.len() != 16 {
                                    return Err(anyhow!(
                                        "طول مفتاح HLS AES-128 غير صالح ({} بايت)",
                                        k_bytes.len()
                                    ));
                                }
                                let mut arr = [0u8; 16];
                                arr.copy_from_slice(&k_bytes[..16]);
                                guard.insert(key_uri, arr);
                                arr
                            }
                        };
                        Self::decrypt_hls_aes128_segment(
                            &raw_bytes,
                            &key_arr,
                            iv,
                            seg.sequence_number,
                        )?
                    }
                    HlsEncryptionMethod::UnsupportedDrm(d) => {
                        return Err(anyhow!("مقطع محمي بـ DRM غير مدعوم: {}", d));
                    }
                };

                // تفريغ المقطع فوراً على القرص وإرساله للمدمج التتابعي لتحرير الذاكرة العشوائية (Zero-OOM)
                let (seg_path, seg_len) =
                    OrderedDiskSpooler::write_segment_to_disk(&spool_clone, idx, &final_bytes)
                        .await?;
                drop(final_bytes);
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
        }

        Ok(output_file.to_path_buf())
    }
}
