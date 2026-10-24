//! محرك الوسائط المتعددة والبث الحي (Media & Streaming Engine Module)
//! يضم محلل ومحمل HLS مع فك تشفير AES-128 (`hls`)، محلل ومحمل MPEG-DASH (`dash`)،
//! وجسر `yt-dlp` القابل للإلغاء ومحول الترجمات (`ytdlp`)، مع نظام تجميع متسلسل على القرص (`OrderedDiskSpooler`) لمنع استنزاف الذاكرة (Zero-OOM).

pub mod dash;
pub mod hls;
pub mod ytdlp;

pub use dash::*;
pub use hls::*;
pub use ytdlp::*;
