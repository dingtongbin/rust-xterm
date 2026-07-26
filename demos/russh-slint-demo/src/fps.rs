//! FPS 滑动平均（60 帧窗口）
use std::time::{Duration, Instant};

pub(crate) struct FpsTracker {
    samples: Vec<Duration>,
    last: Instant,
}

impl FpsTracker {
    pub(crate) fn new() -> Self {
        Self {
            samples: Vec::with_capacity(60),
            last: Instant::now(),
        }
    }

    pub(crate) fn tick(&mut self) -> f64 {
        let now = Instant::now();
        let dt = now - self.last;
        self.last = now;
        self.samples.push(dt);
        if self.samples.len() > 60 {
            self.samples.remove(0);
        }
        let total: Duration = self.samples.iter().sum();
        if total.as_secs_f64() > 0.0 {
            self.samples.len() as f64 / total.as_secs_f64()
        } else {
            0.0
        }
    }
}

impl Default for FpsTracker {
    fn default() -> Self {
        Self::new()
    }
}
