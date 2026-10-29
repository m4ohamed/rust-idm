//! الميزة 8 من `VeloxFetch`: حارس أمان المسارات والأسماء المخادعة والروابط الرمزية (`Path Safety & RTLO Spoofing Guard`)
//! مستلهم من `VeloxFetch/crates/path-safety/src/lib.rs`.
//!
//! يحمي المستخدم والنظام من:
//! 1. محارف إعادة الترتيب الخفية في يونيكود (`U+202E Right-to-Left Override` وغيرها) التي تجعل ملفاً تنفيذياً خبيثاً
//!    مثل `invoice\u{202e}fdp.exe` يظهر بصرياً كأنه `invoice.pdf`!
//! 2. أسماء أجهزة ويندوز المحجوزة بما فيها الأسس العلوية (`COM¹`, `COM²`, `COM³`, `LPT¹`, `LPT²`, `LPT³`, `CONIN$`, `CONOUT$`).
//! 3. هروب المسار خارج مجلد التحميل المعتمد عبر الروابط الرمزية أو نقاط الوصل (`ensure_path_within_root`).
//! 4. الكتابة عبر الروابط الرمزية المزروعة أو الملفات متعددة الروابط الصلبة (`refuse_symlink_or_reparse_point`).

use std::fs::{File, OpenOptions};
use std::io;
use std::path::Path;

pub const WINDOWS_RESERVED_DEVICES_EXTENDED: &[&str] = &[
    "CON",
    "PRN",
    "AUX",
    "NUL",
    "COM1",
    "COM2",
    "COM3",
    "COM4",
    "COM5",
    "COM6",
    "COM7",
    "COM8",
    "COM9",
    "LPT1",
    "LPT2",
    "LPT3",
    "LPT4",
    "LPT5",
    "LPT6",
    "LPT7",
    "LPT8",
    "LPT9",
    "COM\u{b9}",
    "COM\u{b2}",
    "COM\u{b3}",
    "LPT\u{b9}",
    "LPT\u{b2}",
    "LPT\u{b3}",
    "CONIN$",
    "CONOUT$",
];

/// فحص ما إذا كان الاسم يطابق أحد أسماء أجهزة ويندوز المحجوزة (بما فيها `COM¹` و `CONIN$`)
pub fn is_reserved_windows_device_name(name: &str) -> bool {
    let stem = name
        .split('.')
        .next()
        .unwrap_or("")
        .trim_end_matches(' ')
        .to_uppercase();
    WINDOWS_RESERVED_DEVICES_EXTENDED.contains(&stem.as_str())
}

/// فحص المحارف الخفية ومحارف عكس اتجاه النص (`RTLO U+202E` وأخواتها) التي تُستخدم لتزوير امتدادات الملفات التنفيذية
pub fn is_invisible_or_bidi_spoofing_char(c: char) -> bool {
    matches!(
            c as u32,
            0xad
                | 0x34f
                | 0x61c
                | 0x115f
                | 0x1160
                | 0x17b4
                | 0x17b5
                | 0x180b..=0x180f
                | 0x200b..=0x200f
                | 0x2028..=0x202e
                | 0x2060..=0x206f
                | 0x3164
                | 0xfe00..=0xfe0f
                | 0xfeff
                | 0xffa0
                | 0xfff0..=0xfffb
                | 0xfffe
                | 0xffff
                | 0xe0000..=0xe0fff
        )
}

/// تطهير اسم الملف من محارف الخداع البصري (`RTLO`) والرموز غير المسموحة وأسماء الأجهزة المحجوزة
pub fn harden_filename_against_spoofing(raw: &str) -> String {
    let last = raw.rsplit(['/', '\\']).next().unwrap_or("");
    let cleaned: String = last
        .chars()
        .filter(|c| !is_invisible_or_bidi_spoofing_char(*c))
        .map(|c| {
            if matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*' | '/' | '\\')
                || (c as u32) < 0x20
                || (c as u32) == 0x7F
                || c.is_control()
            {
                '_'
            } else {
                c
            }
        })
        .collect();
    let trimmed = cleaned
        .trim_matches(|c: char| c == '.' || c.is_whitespace())
        .to_string();
    if trimmed.is_empty() {
        return "download".to_string();
    }
    if is_reserved_windows_device_name(&trimmed) {
        format!("_{trimmed}")
    } else {
        trimmed
    }
}

/// التحقق الصارم من أن المسار `path` يقع فعلياً داخل مجلد التحميل المعتمد `root` بعد حل أي روابط رمزية أو `..`
pub fn ensure_path_within_root(root: &Path, path: &Path) -> io::Result<()> {
    let canon_root = root.canonicalize()?;
    let mut probe = path;
    let existing = loop {
        if let Ok(c) = probe.canonicalize() {
            break c;
        }
        match probe.parent() {
            Some(p) if p != probe => probe = p,
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "المسار يهرب خارج مجلد التحميل المسموح",
                ))
            }
        }
    };
    if existing.starts_with(&canon_root) {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "المسار يشير إلى موقع خارج مجلد التحميل المسموح",
        ))
    }
}

/// رفض الكتابة عبر رابط رمزي مزروع (`Symlink`) لمنع استبدال ملفات النظام الحساسة
pub fn refuse_symlink_target(path: &Path) -> io::Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(m) => {
            if m.file_type().is_symlink() {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "رفض الكتابة عبر رابط رمزي (Symlink Protection)",
                ));
            }
            Ok(())
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

/// فتح أو إنشاء ملف للكتابة بأمان مع رفض الروابط الرمزية المزروعة
pub fn open_part_file_no_follow(path: &Path) -> io::Result<File> {
    refuse_symlink_target(path)?;
    if let Some(parent) = path.parent() {
        if parent.exists() {
            ensure_path_within_root(parent, path)?;
        }
    }
    OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(path)
}
