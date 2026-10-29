//! دعم ملفات التورنت (`.torrent`) والروابط المغناطيسية (`magnet:?xt=urn:btih:...`)
//! مستلهم من `ezoo-download/crates/engine/src/torrent.rs`.
//!
//! يتضمن:
//! 1. محلل Bencode مدمج بالكامل بلغة Rust الصرفة لقراءة ملفات `.torrent` واستخراج شجرة الملفات وأحجامها مع تجاهل ملفات المحاذاة (`BEP 47` padding).
//! 2. فحص وتحليل الروابط المغناطيسية (`is_magnet`, `magnet_name`, `is_torrent_source`).
//! 3. إدارة خادم `aria2c` محلي مخفي (`127.0.0.1` بكلمة مرور عشوائية لكل جلسة `--rpc-secret` وإغلاق تلقائي `--stop-with-process`) عبر واجهة JSON-RPC 2.0.

use crate::core::tools::{find_tool, hide_console_window};
use anyhow::{anyhow, bail, Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::Duration;

// ============================================================================
// 1. محلل Bencode المدمج لملفات .torrent (Pure-Rust Bencode Parser)
// ============================================================================

#[derive(Debug, Clone, PartialEq)]
enum Ben {
    Int(i64),
    Bytes(Vec<u8>),
    List(Vec<Ben>),
    Dict(Vec<(Vec<u8>, Ben)>),
}

impl Ben {
    fn get(&self, key: &str) -> Option<&Ben> {
        match self {
            Ben::Dict(d) => d
                .iter()
                .find(|(k, _)| k.as_slice() == key.as_bytes())
                .map(|(_, v)| v),
            _ => None,
        }
    }

    fn int(&self) -> Option<i64> {
        match self {
            Ben::Int(i) => Some(*i),
            _ => None,
        }
    }

    fn text(&self) -> Option<String> {
        match self {
            Ben::Bytes(b) => Some(String::from_utf8_lossy(b).into_owned()),
            _ => None,
        }
    }

    fn list(&self) -> &[Ben] {
        match self {
            Ben::List(l) => l,
            _ => &[],
        }
    }
}

struct BencodeReader<'a> {
    data: &'a [u8],
    pos: usize,
    depth: u32,
}

impl BencodeReader<'_> {
    fn peek(&self) -> Result<u8> {
        self.data
            .get(self.pos)
            .copied()
            .ok_or_else(|| anyhow!("ملف التورنت مقطوع أو غير مكتمل"))
    }

    fn number_until(&mut self, end: u8) -> Result<i64> {
        let start = self.pos;
        while self.peek()? != end {
            self.pos += 1;
            if self.pos - start > 20 {
                bail!("ليس ملف تورنت صالحاً (تجاوز طول الرقم)");
            }
        }
        let s = std::str::from_utf8(&self.data[start..self.pos])
            .map_err(|_| anyhow!("ليس ملف تورنت صالحاً"))?;
        self.pos += 1;
        s.parse().map_err(|_| anyhow!("ليس ملف تورنت صالحاً"))
    }

    fn value(&mut self) -> Result<Ben> {
        self.depth += 1;
        if self.depth > 64 {
            bail!("ليس ملف تورنت صالحاً (تجاوز عمق التداخل المسموح)");
        }
        let v = match self.peek()? {
            b'i' => {
                self.pos += 1;
                Ben::Int(self.number_until(b'e')?)
            }
            b'l' => {
                self.pos += 1;
                let mut list = Vec::new();
                while self.peek()? != b'e' {
                    list.push(self.value()?);
                }
                self.pos += 1;
                Ben::List(list)
            }
            b'd' => {
                self.pos += 1;
                let mut dict = Vec::new();
                while self.peek()? != b'e' {
                    let Ben::Bytes(k) = self.value()? else {
                        bail!("ليس ملف تورنت صالحاً (مفتاح القاموس ليس نصاً)");
                    };
                    let val = self.value()?;
                    dict.push((k, val));
                }
                self.pos += 1;
                Ben::Dict(dict)
            }
            b'0'..=b'9' => {
                let len = usize::try_from(self.number_until(b':')?)
                    .map_err(|_| anyhow!("ليس ملف تورنت صالحاً"))?;
                let end = self
                    .pos
                    .checked_add(len)
                    .filter(|e| *e <= self.data.len())
                    .ok_or_else(|| anyhow!("ملف التورنت مقطوع قبل نهاية البيانات"))?;
                let bytes = self.data[self.pos..end].to_vec();
                self.pos = end;
                Ben::Bytes(bytes)
            }
            _ => bail!("ليس ملف تورنت صالحاً"),
        };
        self.depth -= 1;
        Ok(v)
    }
}

/// ملف واحد داخل حزمة التورنت
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TorrentFileEntry {
    /// المسار النسبي داخل التورنت مفصولاً بـ `/`
    pub path: String,
    pub size: u64,
    /// رقم الملف (يبدأ من 1، متوافق مع خيار `select-file` في aria2)
    pub index: usize,
}

/// معلومات حزمة التورنت المستخرجة من ملف `.torrent`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TorrentInfo {
    pub name: String,
    pub files: Vec<TorrentFileEntry>,
    pub total_size: u64,
}

/// قراءة وتحليل بيانات ملف `.torrent`
pub fn parse_torrent(data: &[u8]) -> Result<TorrentInfo> {
    let root = BencodeReader {
        data,
        pos: 0,
        depth: 0,
    }
    .value()?;
    let info = root
        .get("info")
        .ok_or_else(|| anyhow!("ملف التورنت يفتقر إلى قسم info"))?;
    let name = info
        .get("name.utf-8")
        .or_else(|| info.get("name"))
        .and_then(Ben::text)
        .unwrap_or_else(|| "torrent".into());

    let mut files = Vec::new();
    if let Some(list) = info.get("files") {
        for (n, f) in list.list().iter().enumerate() {
            let parts = f
                .get("path.utf-8")
                .or_else(|| f.get("path"))
                .map(Ben::list)
                .unwrap_or(&[]);
            let path: Vec<String> = parts.iter().filter_map(Ben::text).collect();
            // تجاهل ملفات المحاذاة الوهمية (BEP 47 padding files)
            let is_padding = f
                .get("attr")
                .and_then(Ben::text)
                .map(|a| a.contains('p'))
                .unwrap_or(false)
                || path.first().map(|p| p.starts_with(".pad")).unwrap_or(false);
            if is_padding {
                continue;
            }
            files.push(TorrentFileEntry {
                path: path.join("/"),
                size: f.get("length").and_then(Ben::int).unwrap_or(0).max(0) as u64,
                index: n + 1,
            });
        }
    } else {
        files.push(TorrentFileEntry {
            path: name.clone(),
            size: info.get("length").and_then(Ben::int).unwrap_or(0).max(0) as u64,
            index: 1,
        });
    }

    if files.is_empty() {
        bail!("لا يحتوي ملف التورنت على أي ملفات صالحة للتنزيل");
    }
    let total_size = files.iter().map(|f| f.size).sum();
    Ok(TorrentInfo {
        name,
        files,
        total_size,
    })
}

/// التحقق مما إذا كان النص رابطاً مغناطيسياً صالحاً (`magnet:?xt=urn:btih:...` أو `btmh`)
pub fn is_magnet(s: &str) -> bool {
    let s = s.trim().to_ascii_lowercase();
    s.len() <= 8192
        && !s.contains(char::is_whitespace)
        && s.starts_with("magnet:?")
        && (s.contains("xt=urn:btih:") || s.contains("xt=urn:btmh:"))
}

/// التحقق مما إذا كان المصدر رابطاً مغناطيسياً أو مسار ملف `.torrent` محلي
pub fn is_torrent_source(s: &str) -> bool {
    is_magnet(s) || (!s.contains("://") && s.trim().to_ascii_lowercase().ends_with(".torrent"))
}

/// استخراج اسم الحزمة (`dn=`) من الرابط المغناطيسي مع فك ترميز النسبة المئوية (`%XX` و `+`)
pub fn magnet_name(uri: &str) -> Option<String> {
    let query = uri.trim().split_once('?')?.1;
    query
        .split('&')
        .find_map(|kv| {
            let (k, v) = kv.split_once('=')?;
            if !k.eq_ignore_ascii_case("dn") {
                return None;
            }
            let plus_fixed = v.replace('+', " ");
            let decoded = crate::core::filename::percent_decode_str(&plus_fixed);
            let trimmed = decoded.trim().to_string();
            (!trimmed.is_empty()).then_some(trimmed)
        })
}

pub fn encode_base64_torrent(data: &[u8]) -> String {
    crate::media::ytdlp::encode_base64_standard(data)
}

// ============================================================================
// 2. مدير خادم التورنت الخلفي (Aria2 JSON-RPC 2.0 Manager)
// ============================================================================

struct Aria2DaemonProcess {
    port: u16,
    secret: String,
    child: std::process::Child,
}

pub struct Aria2TorrentEngine {
    client: Client,
    daemon: tokio::sync::Mutex<Option<Aria2DaemonProcess>>,
    data_dir: PathBuf,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TorrentStatus {
    /// "active", "waiting", "paused", "error", "complete", "removed"
    pub status: String,
    pub total: u64,
    pub done: u64,
    pub speed: u64,
    pub upload_speed: u64,
    pub connections: u32,
    pub seeders: u32,
    pub seeding: bool,
    pub name: Option<String>,
    pub followed_by: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TorrentAddOptions {
    pub dir: PathBuf,
    /// أرقام الملفات المطلوبة مفصولة بفواصل ("1,3-5")؛ `None` = جميع الملفات
    pub select: Option<String>,
    /// دقائق المشاركة (Seeding) بعد الاكتمال (`0` = إيقاف المشاركة فور الاكتمال)
    pub seed_minutes: u32,
    pub download_limit_kb: u64,
    pub upload_limit_kb: u64,
    pub check_integrity: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TorrentFileState {
    pub path: PathBuf,
    pub size: u64,
    pub done: u64,
    pub selected: bool,
}

fn json_u64(v: &Value) -> u64 {
    v.as_str()
        .and_then(|s| s.parse().ok())
        .or_else(|| v.as_u64())
        .unwrap_or(0)
}

impl Aria2TorrentEngine {
    pub fn new(data_dir: &Path) -> Self {
        let client = Client::builder()
            .no_proxy()
            .http1_only()
            .tcp_nodelay(true)
            .timeout(Duration::from_secs(20))
            .build()
            .unwrap_or_default();
        Self {
            client,
            daemon: tokio::sync::Mutex::new(None),
            data_dir: data_dir.to_path_buf(),
        }
    }

    async fn ensure_daemon(&self) -> Result<(u16, String)> {
        let mut guard = self.daemon.lock().await;
        if let Some(d) = guard.as_mut() {
            if matches!(d.child.try_wait(), Ok(None)) {
                return Ok((d.port, d.secret.clone()));
            }
        }
        let tool = find_tool("aria2c").ok_or_else(|| {
            anyhow!("أداة التورنت aria2c غير موجودة في مجلد tools أو مسار النظام PATH؛ يرجى وضع aria2c.exe في مجلد tools")
        })?;
        let port = std::net::TcpListener::bind("127.0.0.1:0")?
            .local_addr()?
            .port();
        let secret = crate::ipc::native_host::generate_bridge_token();
        let _ = std::fs::create_dir_all(&self.data_dir);
        let dht = self.data_dir.join("dht.dat");

        let mut cmd = std::process::Command::new(&tool);
        cmd.args([
            "--enable-rpc=true",
            "--rpc-listen-all=false",
            "--rpc-allow-origin-all=false",
            "--rpc-max-request-size=32M",
            "--continue=true",
            "--file-allocation=none",
            "--seed-time=0",
            "--bt-save-metadata=false",
            "--follow-torrent=mem",
            "--follow-metalink=false",
            "--auto-file-renaming=false",
            "--allow-overwrite=false",
            "--max-concurrent-downloads=64",
            "--enable-dht=true",
            "--bt-enable-lpd=true",
            "--enable-peer-exchange=true",
            "--bt-max-peers=80",
            "--console-log-level=error",
            "--summary-interval=0",
            "--quiet=true",
            "--check-certificate=true",
            "--disk-cache=32M",
        ])
        .arg(format!("--rpc-listen-port={port}"))
        .arg(format!("--rpc-secret={secret}"))
        .arg(format!("--stop-with-process={}", std::process::id()))
        .arg(format!("--dht-file-path={}", dht.display()))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
        hide_console_window(&mut cmd);

        let child = cmd.spawn().context("تعذر تشغيل محرك التورنت aria2c")?;
        *guard = Some(Aria2DaemonProcess {
            port,
            secret: secret.clone(),
            child,
        });
        drop(guard);

        for _ in 0..50 {
            if self
                .raw_call(port, &secret, "aria2.getVersion", vec![])
                .await
                .is_ok()
            {
                return Ok((port, secret));
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        bail!("لم يستجب محرك التورنت aria2c على المنفذ المحلي")
    }

    async fn raw_call(
        &self,
        port: u16,
        secret: &str,
        method: &str,
        mut params: Vec<Value>,
    ) -> Result<Value> {
        params.insert(0, json!(format!("token:{secret}")));
        let body = json!({
            "jsonrpc": "2.0",
            "id": "stormget",
            "method": method,
            "params": params,
        });
        let bytes = self
            .client
            .post(format!("http://127.0.0.1:{port}/jsonrpc"))
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(serde_json::to_vec(&body)?)
            .send()
            .await?
            .bytes()
            .await?;
        let r: Value =
            serde_json::from_slice(&bytes).context("استجابة JSON-RPC غير متوقعة من aria2c")?;
        if let Some(e) = r.get("error") {
            bail!("{}", e["message"].as_str().unwrap_or("خطأ في محرك aria2"));
        }
        Ok(r["result"].clone())
    }

    async fn call(&self, method: &str, params: Vec<Value>) -> Result<Value> {
        let (port, secret) = self.ensure_daemon().await?;
        self.raw_call(port, &secret, method, params).await
    }

    /// إضافة ملف `.torrent` أو رابط `magnet:?` وإرجاع معرف المهمة (`GID`) في aria2
    pub async fn add(&self, source: &str, opts: &TorrentAddOptions) -> Result<String> {
        let mut rpc_opts = json!({
            "dir": opts.dir.to_string_lossy(),
            "seed-time": opts.seed_minutes.to_string(),
            "max-download-limit": format!("{}K", opts.download_limit_kb),
            "max-upload-limit": format!("{}K", opts.upload_limit_kb),
            "bt-remove-unselected-file": "true",
            "pause-metadata": "false",
        });
        if opts.check_integrity {
            rpc_opts["check-integrity"] = json!("true");
        }
        if let Some(sel) = opts.select.as_deref().filter(|s| !s.is_empty()) {
            rpc_opts["select-file"] = json!(sel);
        }
        let gid = if is_magnet(source) {
            self.call("aria2.addUri", vec![json!([source.trim()]), rpc_opts])
                .await?
        } else {
            let data = std::fs::read(source).context("تعذر قراءة ملف التورنت من القرص")?;
            parse_torrent(&data)?;
            self.call(
                "aria2.addTorrent",
                vec![json!(encode_base64_torrent(&data)), json!([]), rpc_opts],
            )
            .await?
        };
        gid.as_str()
            .map(String::from)
            .ok_or_else(|| anyhow!("لم يُرجع محرك التورنت معرف مهمة صالح"))
    }

    pub async fn status(&self, gid: &str) -> Result<TorrentStatus> {
        let keys = json!([
            "status",
            "totalLength",
            "completedLength",
            "downloadSpeed",
            "uploadSpeed",
            "connections",
            "numSeeders",
            "seeder",
            "bittorrent",
            "followedBy",
            "errorMessage",
            "errorCode"
        ]);
        let r = self
            .call("aria2.tellStatus", vec![json!(gid), keys])
            .await?;
        let name = r["bittorrent"]["info"]["name"]
            .as_str()
            .map(String::from)
            .filter(|n| !n.is_empty());
        Ok(TorrentStatus {
            status: r["status"].as_str().unwrap_or("").to_string(),
            total: json_u64(&r["totalLength"]),
            done: json_u64(&r["completedLength"]),
            speed: json_u64(&r["downloadSpeed"]),
            upload_speed: json_u64(&r["uploadSpeed"]),
            connections: json_u64(&r["connections"]) as u32,
            seeders: json_u64(&r["numSeeders"]) as u32,
            seeding: r["seeder"].as_str() == Some("true"),
            name,
            followed_by: r["followedBy"]
                .as_array()
                .and_then(|a| a.first())
                .and_then(|g| g.as_str())
                .map(String::from),
            error: r["errorMessage"]
                .as_str()
                .filter(|m| !m.is_empty())
                .map(String::from),
        })
    }

    pub async fn remove(&self, gid: &str) {
        if self.daemon.lock().await.is_none() {
            return;
        }
        let _ = self.call("aria2.forceRemove", vec![json!(gid)]).await;
        for _ in 0..20 {
            if self
                .call("aria2.removeDownloadResult", vec![json!(gid)])
                .await
                .is_ok()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    pub async fn files(&self, gid: &str) -> Result<Vec<TorrentFileState>> {
        let r = self.call("aria2.getFiles", vec![json!(gid)]).await?;
        Ok(r.as_array()
            .map(|arr| {
                arr.iter()
                    .map(|f| TorrentFileState {
                        path: PathBuf::from(f["path"].as_str().unwrap_or("")),
                        size: json_u64(&f["length"]),
                        done: json_u64(&f["completedLength"]),
                        selected: f["selected"].as_str() == Some("true"),
                    })
                    .collect()
            })
            .unwrap_or_default())
    }
}

impl Drop for Aria2TorrentEngine {
    fn drop(&mut self) {
        if let Some(d) = self.daemon.get_mut().as_mut() {
            let _ = d.child.kill();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ben_str(s: &str) -> String {
        format!("{}:{s}", s.len())
    }

    #[test]
    fn reads_multi_file_torrent_and_skips_bep47_padding() {
        let t = format!(
            "d8:announce{}4:infod5:filesld6:lengthi100e4:pathl{}{}eed6:lengthi5e4:pathl{}eed4:attr1:p6:lengthi7e4:pathl{}eee4:name{}12:piece lengthi16384e6:pieces0:ee",
            ben_str("http://t/a"),
            ben_str("Season 1"),
            ben_str("ep1.mkv"),
            ben_str("readme.txt"),
            ben_str(".pad"),
            ben_str("My Show")
        );
        let i = parse_torrent(t.as_bytes()).expect("verified in test");
        assert_eq!(i.name, "My Show");
        assert_eq!(
            i.files,
            vec![
                TorrentFileEntry {
                    path: "Season 1/ep1.mkv".into(),
                    size: 100,
                    index: 1
                },
                TorrentFileEntry {
                    path: "readme.txt".into(),
                    size: 5,
                    index: 2
                },
            ]
        );
        assert_eq!(i.total_size, 105);
    }

    #[test]
    fn reads_single_file_torrent_and_rejects_malformed() {
        let t = format!("d4:infod6:lengthi42e4:name{}ee", ben_str("film.mp4"));
        let i = parse_torrent(t.as_bytes()).expect("verified in test");
        assert_eq!(
            i.files,
            vec![TorrentFileEntry {
                path: "film.mp4".into(),
                size: 42,
                index: 1
            }]
        );
        assert!(parse_torrent(b"<html>").is_err());
        assert!(parse_torrent(b"d4:infod6:lengthi42e4:name99:x").is_err());
        assert!(parse_torrent(&b"l".repeat(200)).is_err());
    }

    #[test]
    fn parses_magnet_links_and_display_names() {
        let m = "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=Big+Film%20(2026)&tr=udp%3A%2F%2Ft";
        assert!(is_magnet(m));
        assert!(is_torrent_source(m));
        assert!(is_torrent_source(r"C:\Users\a\Downloads\ubuntu.torrent"));
        assert!(!is_torrent_source("https://site/x.torrent"));
        assert!(!is_magnet("magnet:?xt=urn:sha1:abc"));
        assert_eq!(magnet_name(m).as_deref(), Some("Big Film (2026)"));
    }
}
