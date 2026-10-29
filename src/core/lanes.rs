//! الميزة 7 من `VeloxFetch`: دمج الشبكات المتعددة (`Multi-Network Lane Bonding`)
//! مستلهم من `VeloxFetch/crates/http-client/src/lanes.rs`.
//!
//! يوزع طلبات `Range` الخاصة بالتحميل الواحد عبر عدة شبكات متصلة بالجهاز في آنٍ واحد
//! (مثل `Wi-Fi` + كابل `Ethernet` + هاتف مشترك عبر `USB`) بحيث تتجمع سرعاتها معاً!
//! - كل شبكة (`NetworkLane`) تملك `reqwest::Client` مربوطاً بعنوان IP المحلي لتلك البطاقة (`local_address`).
//! - يوجّه كل طلب جديد إلى الشبكة ذات العبء الأقل بالنسبة لسرعتها المقاسة (`active / rate`).
//! - إذا تعثرت إحدى الشبكات أو كانت أبطأ بـ 8 أضعاف من الشبكة الأسرع (`crawls`)، توضع في حالة راحة (`Resting`)
//!   وتنتقل المقاطع فوراً للشبكات النشطة الأخرى دون فشل التحميل.
//! - للأمان: لا يعمل التوزيع إلا مع روابط `https://` العامة (حيث تثبت شهادة TLS هوية الخادم عبر أي شبكة).

use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{
    atomic::{AtomicU64, AtomicUsize, Ordering},
    Arc, Mutex, RwLock,
};
use std::time::{Duration, Instant};
use url::Url;

pub const FIRST_LANE_REST: Duration = Duration::from_secs(5);
pub const MAX_LANE_REST: Duration = Duration::from_secs(5 * 60);
pub const REFUSAL_REST: Duration = Duration::from_secs(10 * 60);
const MIN_LANE_RATE_BPS: f64 = 16.0 * 1024.0;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkRouteSpec {
    /// اسم الشبكة كما يظهر للمستخدم ("Wi-Fi", "Ethernet", "USB Tethering")
    pub name: String,
    pub interface: Option<String>,
    pub local_ip: IpAddr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LaneState {
    Ready,
    Working,
    Resting,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LaneStatus {
    pub name: String,
    pub local_ip: String,
    pub active_connections: usize,
    pub total_bytes: u64,
    pub smoothed_speed_bps: u64,
    pub state: LaneState,
}

/// هل الشبكة بطيئة جداً (تزحف بأقل من ثُمن سرعة الشبكة الأسرع)؟
pub fn lane_crawls(speed: f64, fastest: f64) -> bool {
    fastest > 0.0 && speed * 8.0 < fastest
}

/// للأمان: لا يُسمح بتوزيع الطلبات عبر عدة شبكات إلا للروابط المحمية بـ `https://` على عناوين عامة
pub fn is_spreadable_https_url(raw_url: &str) -> bool {
    let Ok(u) = Url::parse(raw_url.trim()) else {
        return false;
    };
    if u.scheme() != "https" {
        return false;
    }
    let Some(host) = u.host_str() else {
        return false;
    };
    if host.eq_ignore_ascii_case("localhost") || host.ends_with(".local") {
        return false;
    }
    if let Ok(ip) = host.trim_matches(['[', ']']).parse::<IpAddr>() {
        return match ip {
            IpAddr::V4(v4) => {
                !v4.is_loopback()
                    && !v4.is_private()
                    && !v4.is_link_local()
                    && !v4.is_unspecified()
            }
            IpAddr::V6(v6) => !v6.is_loopback() && !v6.is_unspecified(),
        };
    }
    true
}

#[derive(Default)]
struct LaneHealth {
    down_until: Option<Instant>,
    strikes: u32,
    worked: bool,
    rate_bps: f64,
    measured: Option<(Instant, u64, usize)>,
    refused_hosts: HashMap<String, Instant>,
}

pub struct NetworkLane {
    pub spec: NetworkRouteSpec,
    pub client: Client,
    active: AtomicUsize,
    bytes: AtomicU64,
    health: Mutex<LaneHealth>,
}

impl NetworkLane {
    pub fn new(spec: NetworkRouteSpec, timeout_secs: u64) -> Result<Arc<Self>, String> {
        let client = Client::builder()
            .local_address(spec.local_ip)
            .http1_only()
            .tcp_nodelay(true)
            .connect_timeout(Duration::from_secs(timeout_secs.max(5)))
            .build()
            .map_err(|e| format!("تعذر ربط عميل الشبكة بالواجهة {}: {e}", spec.name))?;
        Ok(Arc::new(Self {
            spec,
            client,
            active: AtomicUsize::new(0),
            bytes: AtomicU64::new(0),
            health: Mutex::new(LaneHealth::default()),
        }))
    }

    pub fn is_usable_for(&self, host: &str, now: Instant) -> bool {
        let h = self.health.lock().unwrap_or_else(|p| p.into_inner());
        h.down_until.is_none_or(|t| now >= t)
            && h.refused_hosts.get(host).is_none_or(|t| now >= *t)
    }

    pub fn measure_rate(&self, now: Instant) -> f64 {
        let total_bytes = self.bytes.load(Ordering::Relaxed);
        let active = self.active.load(Ordering::Relaxed);
        let mut h = self.health.lock().unwrap_or_else(|p| p.into_inner());
        match h.measured {
            Some((at, before, was))
                if active > 0 && now.duration_since(at) >= Duration::from_secs(1) =>
            {
                let delta =
                    total_bytes.saturating_sub(before) as f64 / now.duration_since(at).as_secs_f64();
                let per_conn = delta / ((was + active) as f64 / 2.0).max(1.0);
                h.rate_bps = if h.rate_bps == 0.0 {
                    per_conn
                } else {
                    0.5 * h.rate_bps + 0.5 * per_conn
                };
                h.measured = Some((now, total_bytes, active));
            }
            _ if active == 0 => h.measured = Some((now, total_bytes, 0)),
            None => h.measured = Some((now, total_bytes, active)),
            _ => {}
        }
        h.rate_bps
    }

    pub fn begin_connection(&self) {
        self.active.fetch_add(1, Ordering::Relaxed);
    }

    pub fn end_connection(&self) {
        let mut cur = self.active.load(Ordering::Relaxed);
        while cur > 0 {
            match self.active.compare_exchange_weak(
                cur,
                cur - 1,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(actual) => cur = actual,
            }
        }
    }

    pub fn record_bytes(&self, n: usize) {
        self.bytes.fetch_add(n as u64, Ordering::Relaxed);
        let mut h = self.health.lock().unwrap_or_else(|p| p.into_inner());
        h.worked = true;
        h.strikes = 0;
        h.down_until = None;
    }

    pub fn record_failure(&self) {
        let mut h = self.health.lock().unwrap_or_else(|p| p.into_inner());
        if h.down_until.is_some_and(|t| Instant::now() < t) {
            return;
        }
        let rest = FIRST_LANE_REST
            .saturating_mul(1u32 << h.strikes.min(6))
            .min(MAX_LANE_REST);
        h.strikes = h.strikes.saturating_add(1);
        h.down_until = Some(Instant::now() + rest);
    }

    pub fn record_host_refusal(&self, host: &str) {
        let mut h = self.health.lock().unwrap_or_else(|p| p.into_inner());
        h.refused_hosts
            .insert(host.to_ascii_lowercase(), Instant::now() + REFUSAL_REST);
    }

    pub fn status(&self) -> LaneStatus {
        let now = Instant::now();
        let rate = self.measure_rate(now);
        let h = self.health.lock().unwrap_or_else(|p| p.into_inner());
        let state = if h.down_until.is_some_and(|t| now < t) {
            LaneState::Resting
        } else if h.worked {
            LaneState::Working
        } else {
            LaneState::Ready
        };
        LaneStatus {
            name: self.spec.name.clone(),
            local_ip: self.spec.local_ip.to_string(),
            active_connections: self.active.load(Ordering::Relaxed),
            total_bytes: self.bytes.load(Ordering::Relaxed),
            smoothed_speed_bps: rate as u64,
            state,
        }
    }
}

/// مجمع مسارات الشبكات المتعددة (`MultiNetworkLanePool`)
#[derive(Default)]
pub struct MultiNetworkLanePool {
    lanes: RwLock<Vec<Arc<NetworkLane>>>,
}

impl MultiNetworkLanePool {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            lanes: RwLock::new(Vec::new()),
        })
    }

    pub fn configure_routes(&self, specs: Vec<NetworkRouteSpec>) -> Result<usize, String> {
        let mut new_lanes = Vec::with_capacity(specs.len());
        for spec in specs {
            new_lanes.push(NetworkLane::new(spec, 15)?);
        }
        let count = new_lanes.len();
        *self.lanes.write().unwrap_or_else(|p| p.into_inner()) = new_lanes;
        Ok(count)
    }

    /// اختيار الشبكة الأنسب للطلب الجديد بناءً على نسبة الاتصالات النشطة إلى السرعة المقاسة
    pub fn pick_lane_for_url(&self, url: &str) -> Option<Arc<NetworkLane>> {
        if !is_spreadable_https_url(url) {
            return None;
        }
        let host = Url::parse(url)
            .ok()?
            .host_str()?
            .to_ascii_lowercase();
        let guard = self.lanes.read().unwrap_or_else(|p| p.into_inner());
        if guard.is_empty() {
            return None;
        }
        let now = Instant::now();
        let usable: Vec<&Arc<NetworkLane>> =
            guard.iter().filter(|l| l.is_usable_for(&host, now)).collect();
        if usable.is_empty() {
            return None;
        }
        let fastest = usable
            .iter()
            .map(|l| l.measure_rate(now))
            .fold(0.0f64, f64::max);
        let floor_rate = (fastest * 0.1).max(MIN_LANE_RATE_BPS);

        usable
            .into_iter()
            .min_by(|a, b| {
                let ra = a.measure_rate(now).max(floor_rate);
                let rb = b.measure_rate(now).max(floor_rate);
                let score_a = (a.active.load(Ordering::Relaxed) + 1) as f64 / ra;
                let score_b = (b.active.load(Ordering::Relaxed) + 1) as f64 / rb;
                score_a
                    .partial_cmp(&score_b)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .cloned()
    }

    pub fn statuses(&self) -> Vec<LaneStatus> {
        self.lanes
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .iter()
            .map(|l| l.status())
            .collect()
    }
}


/// المجمع العام لمسارات الشبكات المتعددة المشترك بين حالة المحرك (`AppState`) وعمال التحميل (`DynamicDownloadJob`)
pub fn global_network_lane_pool() -> &'static Arc<MultiNetworkLanePool> {
    static POOL: std::sync::OnceLock<Arc<MultiNetworkLanePool>> = std::sync::OnceLock::new();
    POOL.get_or_init(MultiNetworkLanePool::new)
}
