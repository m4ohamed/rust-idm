//! الوحدة الأساسية لمحرك التحميل (Core Engine Module)
//! تضم:
//! - `limiter`: محدد السرعة القياسي.
//! - `storage`: إدارة التخزين الذري والبصمات الرقمية.
//! - `filename`: استخلاص وتطهير أسماء الملفات الذكية واستنتاج امتدادات MIME.
//! - `segmenter`: محرك التقسيم الديناميكي النصفي الخالي من التنافس على الأقفال.
//! - `tools`: التثبيت والتحديث التلقائي للأدوات ووسم الأمان `:Zone.Identifier`.
//! - `secret`: تشفير الكوكيز وبيانات الدخول عبر Windows DPAPI.
//! - `torrent`: محرك التورنت والروابط المغناطيسية ومحلل Bencode.
//! - `selfupdate`: التحديث الذاتي الموقّع رقمياً بـ Ed25519.
//! - `preview`: المعاينة الحية أثناء التحميل.
//! - `winfx`: تكامل شريط مهام ويندوز وسحب الملفات.
//! - `resume_verify`: الاستئناف بالعينات (`Sample-Based Resume`) وبصمة `SHA-256` لكل قطاع مكتمل.
//! - `publisher`: الاكتشاف التلقائي لبصمة الناشر (`SHA256SUMS` / `SHA512SUMS`).
//! - `adaptive`: المتحكم التكيفي بعدد الاتصالات (`4 → 16 → 64`).
//! - `smart_throttle`: الخنق الذكي التلقائي ووضع الألعاب بضغطة زر (`Game Mode`).
//! - `doctor`: طبيب التحميل (`Download Doctor`) وذكاء ما قبل التحميل (`Pre-Download Intelligence`).
//! - `lanes`: دمج الشبكات المتعددة (`Multi-Network Lane Bonding`).
//! - `path_safety`: حارس أمان المسارات ومنع الخداع البصري `RTLO` والروابط الرمزية.

#[doc(hidden)]
pub use ::core::{
    clone, cmp, convert, default, fmt, future, hash, hint, iter, marker, mem, ops, option, panic,
    pin, primitive, ptr, result, slice, str, task,
};

pub mod adaptive;
pub mod doctor;
pub mod filename;
pub mod lanes;
pub mod limiter;
pub mod path_safety;
pub mod preview;
pub mod publisher;
pub mod resume_verify;
pub mod secret;
pub mod segmenter;
pub mod selfupdate;
pub mod smart_throttle;
pub mod storage;
pub mod tools;
pub mod torrent;
pub mod winfx;

pub use adaptive::*;
pub use doctor::*;
pub use lanes::*;
pub use limiter::*;
pub use path_safety::*;
pub use preview::*;
pub use publisher::*;
pub use resume_verify::*;
pub use secret::*;
pub use segmenter::*;
pub use selfupdate::*;
pub use smart_throttle::*;
pub use storage::*;
pub use tools::*;
pub use torrent::*;
pub use winfx::*;
