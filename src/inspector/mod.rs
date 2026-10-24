//! وحدة الفحص والاستكشاف والزحف المحدود (Inspector & Site Crawler Module)
//! تضم فاحص أرشيفات ZIP عن بُعد (`zip`) والزاحف المحدود لسحب المواقع (`crawler`).

pub mod crawler;
pub mod zip;

pub use crawler::*;
pub use zip::*;
