//! RTT and clock-baseline state shared by control and input paths.
//!
//! These are pure, time-injected helpers: callers pass a local monotonic
//! millisecond timestamp and the sample values, so the behaviour is
//! deterministic in tests and independent of a system clock.

/// Tracks the minimum and EWMA RTT, in milliseconds.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RttEstimator {
    base_ms: Option<u32>,
    avg_ms: Option<u32>,
    samples: u64,
}

impl RttEstimator {
    /// Integer EWMA with alpha = 1/8, avoiding a float in protocol state.
    pub const EWMA_SHIFT: u32 = 3;

    pub fn new() -> Self {
        Self::default()
    }

    /// Add one measured RTT sample. Negative or absurd values are caller bugs;
    /// keep the API total by accepting any `u32`.
    pub fn sample(&mut self, rtt_ms: u32) {
        self.base_ms = Some(match self.base_ms {
            Some(base) => base.min(rtt_ms),
            None => rtt_ms,
        });
        self.avg_ms = Some(match self.avg_ms {
            Some(avg) if rtt_ms >= avg => {
                avg.saturating_add((rtt_ms - avg).div_ceil(1 << Self::EWMA_SHIFT))
            }
            Some(avg) => avg.saturating_sub((avg - rtt_ms) / (1 << Self::EWMA_SHIFT)),
            None => rtt_ms,
        });
        self.samples = self.samples.saturating_add(1);
    }

    pub fn base_ms(&self) -> Option<u32> {
        self.base_ms
    }

    pub fn avg_ms(&self) -> Option<u32> {
        self.avg_ms
    }

    pub fn samples(&self) -> u64 {
        self.samples
    }

    pub fn is_empty(&self) -> bool {
        self.samples == 0
    }
}

/// Input clock baseline: `min(recv_ts_us - ts_us)`, allowing at most +1 ms/s
/// upward adjustment so the baseline follows modest clock drift without
/// letting a single late burst raise it sharply.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClockBaseline {
    baseline_us: Option<i128>,
    last_adjust_ms: u64,
}

impl ClockBaseline {
    pub const MAX_ADJUST_US_PER_SEC: i128 = 1_000;

    pub fn new() -> Self {
        Self::default()
    }

    /// Observe one input timestamp pair. `now_ms` is used only to rate-limit
    /// the upward drift adjustment; the latency itself uses microseconds.
    pub fn observe(&mut self, recv_ts_us: u64, ts_us: u64, now_ms: u64) -> i128 {
        let delta = i128::from(recv_ts_us) - i128::from(ts_us);
        match self.baseline_us {
            None => {
                self.baseline_us = Some(delta);
                self.last_adjust_ms = now_ms;
            }
            Some(mut baseline) => {
                if delta < baseline {
                    baseline = delta;
                    self.last_adjust_ms = now_ms;
                } else if now_ms.saturating_sub(self.last_adjust_ms) >= 1_000 {
                    let elapsed_s = now_ms.saturating_sub(self.last_adjust_ms) / 1_000;
                    let allowance = Self::MAX_ADJUST_US_PER_SEC
                        .saturating_mul(i128::from(elapsed_s.min(u64::from(u32::MAX))));
                    let raise = (delta - baseline).min(allowance).max(0);
                    baseline += raise;
                    self.last_adjust_ms = now_ms;
                }
                self.baseline_us = Some(baseline);
            }
        }
        delta
    }

    pub fn baseline_us(&self) -> Option<i128> {
        self.baseline_us
    }

    /// One-input relative latency after the baseline is removed.
    pub fn latency_us(&self, recv_ts_us: u64, ts_us: u64) -> Option<u32> {
        let baseline = self.baseline_us?;
        let raw = i128::from(recv_ts_us) - i128::from(ts_us) - baseline;
        Some(raw.clamp(0, i128::from(u32::MAX)) as u32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rtt_tracks_minimum_and_ewma() {
        let mut rtt = RttEstimator::new();
        assert_eq!(rtt.base_ms(), None);
        rtt.sample(100);
        rtt.sample(200);
        assert_eq!(rtt.base_ms(), Some(100));
        assert_eq!(rtt.samples(), 2);
        assert_eq!(rtt.avg_ms(), Some(113)); // 100 + 0.125*(200-100) = 112.5
        rtt.sample(20);
        assert_eq!(rtt.base_ms(), Some(20));
        assert!(rtt.avg_ms().unwrap() < 113);
    }

    #[test]
    fn clock_baseline_uses_fastest_sample_and_rate_limits_drift() {
        let mut clock = ClockBaseline::new();
        assert_eq!(clock.observe(10_000, 1_000, 0), 9_000);
        assert_eq!(clock.baseline_us(), Some(9_000));
        assert_eq!(clock.latency_us(10_000, 1_000), Some(0));

        // A later, slower sample raises the baseline by at most 1 ms/s.
        assert_eq!(clock.observe(15_000, 1_000, 1_000), 14_000);
        assert_eq!(clock.baseline_us(), Some(10_000));
        assert_eq!(clock.latency_us(15_000, 1_000), Some(4_000));

        // Two seconds later the remaining 4 ms can be absorbed.
        clock.observe(15_000, 1_000, 3_000);
        assert_eq!(clock.baseline_us(), Some(12_000));
        assert_eq!(clock.latency_us(15_000, 1_000), Some(2_000));
    }

    #[test]
    fn clock_baseline_fastest_sample_resets_without_negative_latency() {
        let mut clock = ClockBaseline::new();
        clock.observe(20_000, 1_000, 0);
        clock.observe(5_000, 1_000, 10);
        assert_eq!(clock.baseline_us(), Some(4_000));
        assert_eq!(clock.latency_us(5_000, 1_000), Some(0));
    }
}
