//! الميزة 4 من `VeloxFetch`: المتحكم التكيفي بعدد الاتصالات (`Adaptive Connection Controller 4 → 16 → 64`)
//! مستلهم من `VeloxFetch/crates/downloader-core/src/adaptive.rs`.
//!
//! يبدأ التحميل بـ `4` اتصالات، وكل فترة قياس يراقب السرعة (`throughput`) والأخطاء وزمن الاستجابة (`latency`):
//! - يضاعف عدد الاتصالات (`4 → 16 → 64`) طالما تحقق الزيادة نمواً في السرعة لا يقل عن `5%` (`min_gain`).
//! - يتراجع فوراً إلى النصف عند استلام `HTTP 429 / 503` (`on_throttled`) أو تجاوز عتبة الأخطاء أو تدهور السرعة.
//! - لا يخفض الاتصالات أبداً تحت `COLLAPSE_FLOOR = 4` بسبب تذبذب عابر في الشبكة بدون أخطاء.

use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

pub const COLLAPSE_FLOOR: usize = 4;
const MAX_BACKOFF_EXP: u32 = 5;
const BEST_FADE: f64 = 0.995;
pub const STABLE_FOR: Duration = Duration::from_secs(20);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdaptiveConfig {
    pub enabled: bool,
    pub interval: Duration,
    pub min_gain: f64,
    pub degrade_ratio: f64,
    pub latency_factor: f64,
    pub error_threshold: u32,
    pub cooldown_intervals: u32,
    pub settle_intervals: u32,
    pub degrade_intervals: u32,
    pub growth: usize,
}

impl Default for AdaptiveConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            interval: Duration::from_secs(1),
            min_gain: 0.05,
            degrade_ratio: 0.50,
            latency_factor: 3.0,
            error_threshold: 3,
            cooldown_intervals: 15,
            settle_intervals: 1,
            degrade_intervals: 3,
            growth: 4,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct AdaptiveSample {
    pub throughput_bps: f64,
    pub errors: u32,
    pub latency: Option<Duration>,
    pub speed_limited: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdaptivePhase {
    Ramping,
    Holding,
}

#[derive(Debug)]
pub struct AdaptiveController {
    cfg: AdaptiveConfig,
    limit: usize,
    ceiling: usize,
    soft_ceiling: usize,
    phase: AdaptivePhase,
    raised: bool,
    settle: u32,
    probed: bool,
    prev_limit: usize,
    prev_throughput: f64,
    best_throughput: f64,
    cooldown: u32,
    low_streak: u32,
    base_latency: Option<Duration>,
    last_throttle_drop: Option<Instant>,
    throttle_hits: u32,
    probe_fails: u32,
    changed_at: Instant,
    floor: usize,
    last_refusal: Option<Instant>,
}

impl AdaptiveController {
    pub fn new(cfg: AdaptiveConfig, start: usize, ceiling: usize) -> Self {
        let ceiling = ceiling.max(1);
        let start = start.clamp(1, ceiling);
        Self {
            cfg,
            limit: start,
            ceiling,
            soft_ceiling: ceiling,
            phase: AdaptivePhase::Ramping,
            raised: false,
            settle: 0,
            probed: false,
            prev_limit: start,
            prev_throughput: 0.0,
            best_throughput: 0.0,
            cooldown: 0,
            low_streak: 0,
            base_latency: None,
            last_throttle_drop: None,
            throttle_hits: 0,
            probe_fails: 0,
            changed_at: Instant::now(),
            floor: 1,
            last_refusal: None,
        }
    }

    pub fn with_floor(mut self, n: usize) -> Self {
        self.floor = n.clamp(1, self.ceiling);
        self.limit = self.limit.max(self.floor);
        self.prev_limit = self.prev_limit.max(self.floor);
        self
    }

    pub fn limit(&self) -> usize {
        self.limit
    }

    pub fn ceiling(&self) -> usize {
        self.ceiling
    }

    pub fn set_ceiling(&mut self, ceiling: usize) -> Option<usize> {
        let ceiling = ceiling.max(1);
        if ceiling == self.ceiling {
            return None;
        }
        let raised = ceiling > self.ceiling;
        self.ceiling = ceiling;
        if raised {
            self.soft_ceiling = ceiling;
            if self.phase == AdaptivePhase::Holding && self.cooldown > 1 {
                self.cooldown = 1;
            }
            return None;
        }
        self.soft_ceiling = self.soft_ceiling.min(ceiling);
        (self.limit > ceiling).then(|| {
            self.limit = ceiling;
            self.changed_at = Instant::now();
            ceiling
        })
    }

    fn step_down(&mut self) -> usize {
        (self.limit / 2).max(self.floor)
    }

    fn hold(&mut self) {
        self.phase = AdaptivePhase::Holding;
        let backoff = (self.throttle_hits + self.probe_fails).min(MAX_BACKOFF_EXP);
        self.cooldown = self.cfg.cooldown_intervals.saturating_mul(1 << backoff);
        self.raised = false;
        self.settle = 0;
        self.low_streak = 0;
    }

    fn raise(&mut self, throughput: f64, cap: usize, factor: usize) -> Option<usize> {
        self.prev_limit = self.limit;
        self.prev_throughput = throughput;
        self.limit = (self.limit * factor.max(2)).min(cap);
        self.changed_at = Instant::now();
        self.raised = true;
        self.settle = self.cfg.settle_intervals;
        Some(self.limit)
    }

    fn lower_to(&mut self, new_limit: usize, throughput: f64) -> Option<usize> {
        self.probed = true;
        self.soft_ceiling = new_limit;
        self.limit = new_limit;
        self.changed_at = Instant::now();
        self.best_throughput = throughput;
        self.hold();
        Some(new_limit)
    }

    /// استجابة فورية عند رفض الخادم لعدد الاتصالات (`HTTP 429 / 503`)
    pub fn on_throttled(&mut self, now: Instant) -> Option<usize> {
        if self.limit <= 1 {
            return None;
        }
        if self
            .last_throttle_drop
            .is_some_and(|t| now.saturating_duration_since(t) < self.cfg.interval)
        {
            return None;
        }
        let hiccup = !self.raised
            && now.saturating_duration_since(self.changed_at) >= STABLE_FOR
            && self
                .last_refusal
                .is_none_or(|t| now.saturating_duration_since(t) >= STABLE_FOR);
        self.last_refusal = Some(now);
        self.last_throttle_drop = Some(now);
        if hiccup {
            return None;
        }
        self.throttle_hits = self.throttle_hits.saturating_add(1);
        let new_limit = (self.limit / 2).max(1);
        self.lower_to(new_limit, self.best_throughput * 0.5)
    }

    /// تحديث القرار التكيفي بناءً على قراءة الفترة الحالية (`Sample`)
    pub fn on_sample(&mut self, s: AdaptiveSample) -> Option<usize> {
        if !self.cfg.enabled || s.speed_limited {
            return None;
        }
        if let Some(lat) = s.latency {
            self.base_latency = Some(match self.base_latency {
                Some(b) if lat < b => lat,
                Some(b) => b,
                None => lat,
            });
        }

        if s.errors >= self.cfg.error_threshold && self.limit > self.floor {
            let next = self.step_down();
            return self.lower_to(next, s.throughput_bps);
        }

        if let (Some(lat), Some(base)) = (s.latency, self.base_latency) {
            if lat.as_secs_f64() > base.as_secs_f64() * self.cfg.latency_factor
                && self.limit > self.floor
            {
                let next = self.step_down();
                return self.lower_to(next, s.throughput_bps);
            }
        }

        if self.settle > 0 {
            self.settle -= 1;
            return None;
        }

        if self.raised {
            self.raised = false;
            let required = self.prev_throughput * (1.0 + self.cfg.min_gain);
            if self.prev_throughput > 0.0 && s.throughput_bps < required {
                self.probe_fails = self.probe_fails.saturating_add(1);
                let revert = self.prev_limit.max(self.floor);
                self.limit = revert;
                self.soft_ceiling = revert;
                self.changed_at = Instant::now();
                self.hold();
                return Some(revert);
            }
            self.probe_fails = 0;
            if s.throughput_bps > self.best_throughput {
                self.best_throughput = s.throughput_bps;
            }
        }

        match self.phase {
            AdaptivePhase::Ramping => {
                if s.throughput_bps > self.best_throughput {
                    self.best_throughput = s.throughput_bps;
                }
                if self.limit < self.soft_ceiling && s.throughput_bps > 0.0 {
                    let factor = if self.probed { 2 } else { self.cfg.growth };
                    return self.raise(s.throughput_bps, self.soft_ceiling, factor);
                }
                self.probed = true;
                self.hold();
                None
            }
            AdaptivePhase::Holding => {
                self.best_throughput *= BEST_FADE;
                if s.throughput_bps > self.best_throughput {
                    self.best_throughput = s.throughput_bps;
                }
                let floor = self.floor.max(COLLAPSE_FLOOR);
                if self.best_throughput > 0.0
                    && s.throughput_bps < self.best_throughput * self.cfg.degrade_ratio
                    && self.limit > floor
                {
                    self.low_streak += 1;
                    if self.low_streak >= self.cfg.degrade_intervals {
                        let next = (self.limit / 2).max(floor);
                        return self.lower_to(next, s.throughput_bps);
                    }
                } else {
                    self.low_streak = 0;
                }

                if self.cooldown > 0 {
                    self.cooldown -= 1;
                } else if self.limit < self.ceiling {
                    self.soft_ceiling = self.ceiling;
                    self.phase = AdaptivePhase::Ramping;
                    return self.raise(s.throughput_bps, self.ceiling, 2);
                }
                None
            }
        }
    }
}
