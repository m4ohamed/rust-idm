//! محدد السرعة القياسي العادل (Fair Token-Bucket Speed Limiter)

use std::{
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tokio::{sync::Mutex, time::sleep};

pub struct SpeedLimiter {
    rate_bytes_per_sec: AtomicU64,
    state: Mutex<(f64, Instant)>,
}

impl SpeedLimiter {
    pub fn new(limit_kbps: u64) -> Arc<Self> {
        let rate = limit_kbps * 1024;
        Arc::new(Self {
            rate_bytes_per_sec: AtomicU64::new(rate),
            state: Mutex::new((rate as f64, Instant::now())),
        })
    }

    pub fn set_limit_kbps(&self, limit_kbps: u64) {
        self.rate_bytes_per_sec
            .store(limit_kbps * 1024, Ordering::SeqCst);
    }

    pub fn get_limit_kbps(&self) -> u64 {
        self.rate_bytes_per_sec.load(Ordering::Relaxed) / 1024
    }

    pub async fn acquire(&self, bytes: usize) {
        let rate = self.rate_bytes_per_sec.load(Ordering::Relaxed);
        if rate == 0 {
            return;
        }
        let rate_f = rate as f64;
        let needed = bytes as f64;

        let wait_duration = {
            let mut guard = self.state.lock().await;
            let (ref mut tokens, ref mut last_instant) = *guard;
            let elapsed = last_instant.elapsed().as_secs_f64();
            *last_instant = Instant::now();
            *tokens = (*tokens + elapsed * rate_f).min(rate_f);

            if *tokens >= needed {
                *tokens -= needed;
                Duration::ZERO
            } else {
                let deficit = needed - *tokens;
                *tokens = 0.0;
                Duration::from_secs_f64(deficit / rate_f)
            }
        };

        if !wait_duration.is_zero() {
            sleep(wait_duration).await;
        }
    }
}
