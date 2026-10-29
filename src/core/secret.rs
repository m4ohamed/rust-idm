//! تشفير الكوكيز وكلمات المرور وبيانات المصادقة على القرص باستخدام مفتاح المستخدم في ويندوز (`DPAPI` / `CryptProtectData`)
//! مستلهم من `ezoo-download/crates/engine/src/secret.rs`.
//!
//! على القرص تُحفظ النصوص المحمية بالبادئة `"dpapi:<hex>"`.
//! لا يمكن لأي مستخدم آخر أو جهاز آخر فك تشفيرها، كما تُقرأ الملفات القديمة غير المشفرة
//! تلقائياً وتُشفَّر عند الحفظ التالي دون كسر التوافق الخلفي.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::HashMap;
use std::sync::Mutex;

pub const DPAPI_PREFIX: &str = "dpapi:";

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn from_hex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok())
        .collect()
}

#[cfg(windows)]
mod os {
    use std::ffi::c_void;

    #[repr(C)]
    struct CryptIntegerBlob {
        cb_data: u32,
        pb_data: *mut u8,
    }

    const CRYPTPROTECT_UI_FORBIDDEN: u32 = 0x1;

    #[link(name = "crypt32")]
    extern "system" {
        fn CryptProtectData(
            p_data_in: *const CryptIntegerBlob,
            sz_data_descr: *const u16,
            p_optional_entropy: *const CryptIntegerBlob,
            pv_reserved: *mut c_void,
            p_prompt_struct: *const c_void,
            dw_flags: u32,
            p_data_out: *mut CryptIntegerBlob,
        ) -> i32;

        fn CryptUnprotectData(
            p_data_in: *const CryptIntegerBlob,
            ppsz_data_descr: *mut *mut u16,
            p_optional_entropy: *const CryptIntegerBlob,
            pv_reserved: *mut c_void,
            p_prompt_struct: *const c_void,
            dw_flags: u32,
            p_data_out: *mut CryptIntegerBlob,
        ) -> i32;
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn LocalFree(h_mem: *mut c_void) -> *mut c_void;
    }

    fn run(data: &[u8], protect: bool) -> Option<Vec<u8>> {
        let input = CryptIntegerBlob {
            cb_data: u32::try_from(data.len()).ok()?,
            pb_data: data.as_ptr() as *mut u8,
        };
        let mut out = CryptIntegerBlob {
            cb_data: 0,
            pb_data: std::ptr::null_mut(),
        };
        // SAFETY: `input` points to `data` which outlives the Win32 call; on success
        // Windows allocates `out.pb_data` via LocalAlloc, which we copy and free once via LocalFree.
        unsafe {
            let ok = if protect {
                CryptProtectData(
                    &input,
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null_mut(),
                    std::ptr::null(),
                    CRYPTPROTECT_UI_FORBIDDEN,
                    &mut out,
                )
            } else {
                CryptUnprotectData(
                    &input,
                    std::ptr::null_mut(),
                    std::ptr::null(),
                    std::ptr::null_mut(),
                    std::ptr::null(),
                    CRYPTPROTECT_UI_FORBIDDEN,
                    &mut out,
                )
            };
            if ok == 0 || out.pb_data.is_null() {
                return None;
            }
            let bytes = std::slice::from_raw_parts(out.pb_data, out.cb_data as usize).to_vec();
            LocalFree(out.pb_data as *mut c_void);
            Some(bytes)
        }
    }

    pub fn protect(data: &[u8]) -> Option<Vec<u8>> {
        run(data, true)
    }

    pub fn unprotect(data: &[u8]) -> Option<Vec<u8>> {
        run(data, false)
    }
}

#[cfg(not(windows))]
mod os {
    pub fn protect(_data: &[u8]) -> Option<Vec<u8>> {
        None
    }
    pub fn unprotect(_data: &[u8]) -> Option<Vec<u8>> {
        None
    }
}

/// ذاكرة تخزين مؤقت للنصوص المشفرة خلال الجلسة لتفادي تكرار استدعاء النظام عند الحفظ الدوري
static SEALED_CACHE: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);

/// تشفير النص السري لحفظه على القرص ببادئة `dpapi:`
pub fn seal(plain: &str) -> String {
    if plain.is_empty() || plain.starts_with(DPAPI_PREFIX) {
        return plain.to_string();
    }
    let mut cache = SEALED_CACHE.lock().unwrap_or_else(|p| p.into_inner());
    let map = cache.get_or_insert_with(HashMap::new);
    if let Some(cached) = map.get(plain) {
        return cached.clone();
    }
    let sealed = match os::protect(plain.as_bytes()) {
        Some(enc) => format!("{DPAPI_PREFIX}{}", to_hex(&enc)),
        None => plain.to_string(),
    };
    if map.len() > 1000 {
        map.clear();
    }
    map.insert(plain.to_string(), sealed.clone());
    sealed
}

/// فك تشفير النص المحفوظ على القرص؛ إذا تعذر فك التشفير (مستخدم آخر أو بيانات تالفة) يُرجع نصاً فارغاً بأمان
pub fn open(stored: &str) -> String {
    match stored.strip_prefix(DPAPI_PREFIX) {
        Some(hex_str) => from_hex(hex_str)
            .and_then(|b| os::unprotect(&b))
            .and_then(|b| String::from_utf8(b).ok())
            .unwrap_or_default(),
        None => stored.to_string(),
    }
}

pub fn is_sensitive_header_name(name: &str) -> bool {
    let lower = name.trim().to_ascii_lowercase();
    lower.contains("cookie")
        || lower.contains("authorization")
        || lower.contains("token")
        || lower.contains("secret")
        || lower.contains("password")
        || lower.contains("api-key")
        || lower.contains("apikey")
}

pub mod string {
    use super::*;
    pub fn serialize<S: Serializer>(v: &str, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&seal(v))
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
        Ok(open(&String::deserialize(d)?))
    }
}

pub mod opt {
    use super::*;
    pub fn serialize<S: Serializer>(v: &Option<String>, s: S) -> Result<S::Ok, S::Error> {
        v.as_deref().map(seal).serialize(s)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
        Ok(Option::<String>::deserialize(d)?
            .map(|x| open(&x))
            .filter(|x| !x.is_empty()))
    }
}

pub mod pair {
    use super::*;
    pub fn serialize<S: Serializer>(v: &Option<(String, String)>, s: S) -> Result<S::Ok, S::Error> {
        v.as_ref()
            .map(|(u, p)| (u.clone(), seal(p)))
            .serialize(s)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(
        d: D,
    ) -> Result<Option<(String, String)>, D::Error> {
        Ok(Option::<(String, String)>::deserialize(d)?.map(|(u, p)| (u, open(&p))))
    }
}

/// تشفير تلقائي لترويسات الكوكيز والمصادقة داخل `HashMap<String, String>` عند الحفظ على القرص
pub mod header_map {
    use super::*;
    pub fn serialize<S: Serializer>(
        map: &HashMap<String, String>,
        s: S,
    ) -> Result<S::Ok, S::Error> {
        let protected: HashMap<String, String> = map
            .iter()
            .map(|(k, v)| {
                if is_sensitive_header_name(k) {
                    (k.clone(), seal(v))
                } else {
                    (k.clone(), v.clone())
                }
            })
            .collect();
        protected.serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        d: D,
    ) -> Result<HashMap<String, String>, D::Error> {
        let raw = HashMap::<String, String>::deserialize(d)?;
        let mut out = HashMap::with_capacity(raw.len());
        for (k, v) in raw {
            let plain = if v.starts_with(DPAPI_PREFIX) || is_sensitive_header_name(&k) {
                open(&v)
            } else {
                v
            };
            if !plain.is_empty() {
                out.insert(k, plain);
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Serialize, Deserialize, PartialEq, Debug)]
    struct SampleSecretConfig {
        #[serde(with = "string")]
        pub password: String,
        #[serde(default, with = "opt")]
        pub cookies: Option<String>,
        #[serde(default, with = "pair")]
        pub auth: Option<(String, String)>,
        #[serde(default, with = "header_map")]
        pub headers: HashMap<String, String>,
    }

    #[test]
    fn plain_text_backward_compatibility_and_roundtrip() {
        let raw_json = r#"{"password":"pw123","cookies":"SID=abc","auth":["admin","sec"],"headers":{"cookie":"SID=xyz","user-agent":"StormGet"}}"#;
        let parsed: SampleSecretConfig = serde_json::from_str(raw_json).expect("verified in test");
        assert_eq!(parsed.password, "pw123");
        assert_eq!(parsed.cookies.as_deref(), Some("SID=abc"));
        assert_eq!(parsed.auth, Some(("admin".into(), "sec".into())));
        assert_eq!(parsed.headers.get("cookie").map(String::as_str), Some("SID=xyz"));

        let serialized = serde_json::to_string(&parsed).expect("verified in test");
        let restored: SampleSecretConfig = serde_json::from_str(&serialized).expect("verified in test");
        assert_eq!(restored, parsed);
    }

    #[test]
    fn unreadable_dpapi_blobs_become_empty_safely() {
        assert_eq!(open("dpapi:zz"), "");
        assert_eq!(open("dpapi:00ff"), "");
        assert_eq!(seal(""), "");
    }
}
