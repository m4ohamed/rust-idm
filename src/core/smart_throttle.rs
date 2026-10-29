//! الميزة 5 من `VeloxFetch`: الخنق الذكي التلقائي ووضع الألعاب (`Smart Throttle & One-Click Game Mode`)
//! مستلهم من `VeloxFetch/crates/download-manager/src/manager/smart_throttle.rs`.
//!
//! يوفّر:
//! 1. قائمة البرامج والألعاب وتطبيقات الاجتماعات ذات الأولوية (`DEFAULT_GAMING_AND_CALL_APPS`).
//! 2. وضع الألعاب بضغطة زر (`Game Mode`) مع مؤقت تنازلي بالدقائق (`0` = حتى يغلقه المستخدم، أو حتى `1440` دقيقة).
//! 3. حساب حركة الشبكة الأخرى في النظام (`measure_other_traffic`) مع تعويض ترويسات TCP/TLS (`HEADER_OVERHEAD = 1.06`)،
//!    بحيث تتراجع سرعة التحميل تلقائياً عند تشغيل لعبة أو مكالمة فيديو أو تصفح مكثف، وتعود للسرعة القصوى فور فراغ الخط!

use serde::{Deserialize, Serialize};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const DEFAULT_GAME_MODE_MINUTES: u32 = 120;
pub const MAX_GAME_MODE_MINUTES: u32 = 24 * 60;
pub const DEFAULT_GAME_LIMIT_KBPS: u64 = 512;
pub const DEFAULT_FLOOR_KBPS: u64 = 256;
pub const HEADER_OVERHEAD: f64 = 1.06;

/// قائمة الألعاب وبرامج الاجتماعات الافتراضية التي تتنحى لها التحميلات تلقائياً
pub const DEFAULT_GAMING_AND_CALL_APPS: &[&str] = &[
    "League of Legends",
    "VALORANT-Win64-Shipping",
    "cs2",
    "dota2",
    "FortniteClient-Win64-Shipping",
    "r5apex",
    "RocketLeague",
    "GTA5",
    "Overwatch",
    "EA SPORTS FC 25",
    "eFootball",
    "PUBG",
    "TslGame",
    "Zoom",
    "zoom.us",
    "ms-teams",
    "Teams",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum GamePolicy {
    /// الإبقاء على حصة متكيفة من الخط (مثلاً 25%) دون إيقاف التحميل
    #[default]
    Adaptive,
    /// سقف سرعة منخفض ثابت أثناء اللعب (مثلاً 512 KB/s)
    LowBandwidth,
    /// إيقاف طوابير التحميل مؤقتاً طوال فترة عمل اللعبة أو المكالمة
    PauseQueue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum SmartThrottleMode {
    /// السرعة القصوى دائماً
    #[default]
    Fast,
    /// التكيف الذكي التلقائي مع الألعاب والمكالمات واستهلاك الشبكة الآخر
    Smart,
    /// سقف يدوي ثابت
    Manual,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmartThrottleStatus {
    pub mode: SmartThrottleMode,
    pub game_mode_active: bool,
    pub game_mode_until_ms: Option<u64>,
    pub game_policy: GamePolicy,
    pub game_limit_kbps: u64,
    pub floor_kbps: u64,
    pub share_percent: u8,
    pub detected_priority_app: Option<String>,
    pub effective_speed_limit_kbps: u64,
    pub watched_apps: Vec<String>,
}

impl Default for SmartThrottleStatus {
    fn default() -> Self {
        Self {
            mode: SmartThrottleMode::Fast,
            game_mode_active: false,
            game_mode_until_ms: None,
            game_policy: GamePolicy::Adaptive,
            game_limit_kbps: DEFAULT_GAME_LIMIT_KBPS,
            floor_kbps: DEFAULT_FLOOR_KBPS,
            share_percent: 25,
            detected_priority_app: None,
            effective_speed_limit_kbps: 0,
            watched_apps: DEFAULT_GAMING_AND_CALL_APPS
                .iter()
                .map(|s| (*s).to_string())
                .collect(),
        }
    }
}

pub fn current_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_millis() as u64
}

/// فحص ما إذا كان أحد البرامج المراقبة يعمل حالياً ضمن قائمة العمليات الجارية
pub fn match_running_priority_app(
    running_processes: &[String],
    watched_apps: &[String],
) -> Option<String> {
    for proc_name in running_processes {
        let clean_proc = proc_name
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(proc_name)
            .trim()
            .trim_end_matches(".exe")
            .trim_end_matches(".EXE");
        if clean_proc.is_empty() {
            continue;
        }
        for watched in watched_apps {
            let clean_watched = watched
                .rsplit(['/', '\\'])
                .next()
                .unwrap_or(watched)
                .trim()
                .trim_end_matches(".exe")
                .trim_end_matches(".EXE");
            if !clean_watched.is_empty() && clean_proc.eq_ignore_ascii_case(clean_watched) {
                return Some(watched.clone());
            }
        }
    }
    None
}

/// حساب حركة الشبكة الأخرى في الجهاز (بايت/ثانية) بطرح بايتات StormGet مضروبة في `HEADER_OVERHEAD (1.06)` من إجمالي بايتات بطاقة الشبكة
pub fn compute_other_network_traffic_bps(
    nic_delta_bytes: u64,
    stormget_delta_bytes: u64,
    elapsed_secs: f64,
) -> u64 {
    if elapsed_secs <= 0.001 {
        return 0;
    }
    let nic_bps = nic_delta_bytes as f64 / elapsed_secs;
    let own_bps = (stormget_delta_bytes as f64 / elapsed_secs) * HEADER_OVERHEAD;
    (nic_bps - own_bps).max(0.0) as u64
}

impl SmartThrottleStatus {
    /// تفعيل أو إيقاف وضع الألعاب (`Game Mode`) بضغطة زر
    pub fn set_game_mode(&mut self, active: bool, duration_minutes: Option<u32>, peak_kbps: u64) {
        if !active {
            self.game_mode_active = false;
            self.game_mode_until_ms = None;
            self.recalculate_effective_limit(peak_kbps, 0);
            return;
        }
        let mins = duration_minutes
            .unwrap_or(DEFAULT_GAME_MODE_MINUTES)
            .min(MAX_GAME_MODE_MINUTES);
        self.game_mode_active = true;
        self.game_mode_until_ms = if mins == 0 {
            Some(0) // مفتوح حتى يغلقه المستخدم يدوياً
        } else {
            Some(current_unix_ms() + u64::from(mins) * 60 * 1000)
        };
        self.recalculate_effective_limit(peak_kbps, 0);
    }

    /// التحقق مما إذا كان مؤقت وضع الألعاب لا يزال سارياً
    pub fn is_game_mode_currently_on(&mut self) -> bool {
        if !self.game_mode_active {
            return false;
        }
        if let Some(until) = self.game_mode_until_ms {
            if until > 0 && current_unix_ms() >= until {
                self.game_mode_active = false;
                self.game_mode_until_ms = None;
                return false;
            }
        }
        true
    }

    /// حساب سقف السرعة الفعلي (KB/s) بناءً على وضع الألعاب أو البرامج المكتشفة أو استهلاك الشبكة الآخر
    pub fn recalculate_effective_limit(&mut self, peak_kbps: u64, other_traffic_kbps: u64) -> u64 {
        let _other_bps = compute_other_network_traffic_bps(other_traffic_kbps * 1024, 0, 1.0);
        let game_active = self.is_game_mode_currently_on()
            || (self.mode == SmartThrottleMode::Smart && self.detected_priority_app.is_some());

        if game_active {
            let cap = match self.game_policy {
                GamePolicy::LowBandwidth => self.game_limit_kbps.max(self.floor_kbps),
                GamePolicy::Adaptive => {
                    let base = if peak_kbps > 0 { peak_kbps } else { 2048 };
                    ((base * u64::from(self.share_percent.clamp(5, 90))) / 100)
                        .max(self.floor_kbps)
                }
                GamePolicy::PauseQueue => self.floor_kbps.min(64),
            };
            self.effective_speed_limit_kbps = cap;
            return cap;
        }

        if self.mode == SmartThrottleMode::Smart && other_traffic_kbps > 256 {
            let base = if peak_kbps > 0 { peak_kbps } else { 4096 };
            let cap = ((base * 50) / 100).max(self.floor_kbps);
            self.effective_speed_limit_kbps = cap;
            return cap;
        }

        self.effective_speed_limit_kbps = 0; // السرعة القصوى (بدون قيد)
        0
    }
}
