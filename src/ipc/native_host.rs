//! بروتوكول Chrome Native Messaging Host القياسي (4-Byte Length-Prefixed Stdio)

use crate::core::storage::MAX_NATIVE_MSG_BYTES;
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    io::{Read, Write},
    path::Path,
};
use url::Url;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NativeIncomingMessage {
    pub action: String, // "PING" | "INTERCEPT_DOWNLOAD" | "VIDEO_STREAM"
    pub url: Option<String>,
    pub filename: Option<String>,
    pub referer: Option<String>,
    pub cookies: Option<String>,
    pub user_agent: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NativeOutgoingMessage {
    pub status: String,
    pub engine: String,
    pub version: String,
    pub task_id: Option<String>,
    pub message: String,
}

/// قراءة رسالة واحدة من Chrome عبر stdin وفق بروتوكول 4 بايت Little-Endian
pub fn read_native_message<R: Read>(reader: &mut R) -> Result<Option<NativeIncomingMessage>> {
    let mut len_bytes = [0u8; 4];
    match reader.read_exact(&mut len_bytes) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e.into()),
    }

    let msg_len = u32::from_le_bytes(len_bytes) as usize;
    if msg_len == 0 || msg_len > MAX_NATIVE_MSG_BYTES {
        return Err(anyhow!(
            "حجم رسالة Native Messaging غير صالح أو يتجاوز الحد المسموح: {} بايت",
            msg_len
        ));
    }

    let mut payload_buf = vec![0u8; msg_len];
    reader.read_exact(&mut payload_buf)?;
    let parsed: NativeIncomingMessage = serde_json::from_slice(&payload_buf)?;
    Ok(Some(parsed))
}

/// كتابة رسالة رد إلى Chrome عبر stdout وفق بروتوكول 4 بايت Little-Endian
pub fn write_native_message<W: Write>(writer: &mut W, msg: &NativeOutgoingMessage) -> Result<()> {
    let json_bytes = serde_json::to_vec(msg)?;
    let len = json_bytes.len() as u32;
    writer.write_all(&len.to_le_bytes())?;
    writer.write_all(&json_bytes)?;
    writer.flush()?;
    Ok(())
}

/// التحقق الأمني من رسالة Chrome ومنع تمرير روابط غير HTTP/HTTPS
pub fn validate_native_incoming(msg: &NativeIncomingMessage) -> Result<()> {
    match msg.action.as_str() {
        "PING" => Ok(()),
        "INTERCEPT_DOWNLOAD" | "VIDEO_STREAM" => {
            let raw_url = msg
                .url
                .as_ref()
                .ok_or_else(|| anyhow!("الرسالة تفتقر إلى حقل الرابط url"))?;
            let parsed = Url::parse(raw_url).context("صيغة الرابط غير صالحة")?;
            if parsed.scheme() != "http" && parsed.scheme() != "https" {
                return Err(anyhow!(
                    "بروتوكول الرابط غير مسموح به عبر Native Messaging: {}",
                    parsed.scheme()
                ));
            }
            Ok(())
        }
        other => Err(anyhow!("إجراء غير معروف في Native Messaging: {}", other)),
    }
}

/// توليد ملف تعريف المضيف الأصلي لمتصفح Google Chrome (`com.rustidm.native_host.json`)
pub fn generate_native_host_manifest(binary_path: &Path, extension_id: &str) -> serde_json::Value {
    serde_json::json!({
        "name": "com.rustidm.native_host",
        "description": "RustIDM Studio Native Messaging Host",
        "path": binary_path.to_string_lossy(),
        "type": "stdio",
        "allowed_origins": [
            format!("chrome-extension://{}/", extension_id.trim_matches('/'))
        ]
    })
}
