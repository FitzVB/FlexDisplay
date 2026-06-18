use std::sync::atomic::{AtomicU32, AtomicU8, Ordering};
use std::time::{Duration, Instant};

/// Host-side playback tuning — balances glass latency vs video quality.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlaybackTuning {
    /// Desktop / pointer use: lower latency, tighter buffers.
    Interactive,
    /// Video / fast motion: wider VBV, AQ, smoother decode.
    Motion,
}

impl PlaybackTuning {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Interactive => "interactive",
            Self::Motion => "motion",
        }
    }

    pub fn from_str(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "motion" | "video" | "smooth" => Self::Motion,
            _ => Self::Interactive,
        }
    }
}

/// Rolling motion estimate from H.264 chunk throughput (0.0 = static, 1.0 = heavy motion).
pub struct MotionDetector {
    buckets: [u32; 10],
    idx: usize,
    last_rotate: Instant,
}

impl Default for MotionDetector {
    fn default() -> Self {
        Self {
            buckets: [0; 10],
            idx: 0,
            last_rotate: Instant::now(),
        }
    }
}

impl MotionDetector {
    pub fn observe_chunk(&mut self, nbytes: usize) {
        while self.last_rotate.elapsed() >= Duration::from_millis(100) {
            self.idx = (self.idx + 1) % self.buckets.len();
            self.buckets[self.idx] = 0;
            self.last_rotate += Duration::from_millis(100);
        }
        self.buckets[self.idx] = self.buckets[self.idx].saturating_add(nbytes as u32);
    }

    pub fn score(&self) -> f32 {
        let vals: Vec<f32> = self
            .buckets
            .iter()
            .map(|b| *b as f32 / 1024.0)
            .filter(|kb| *kb > 0.5)
            .collect();
        if vals.len() < 3 {
            return 0.0;
        }
        let mean = vals.iter().sum::<f32>() / vals.len() as f32;
        if mean < 8.0 {
            return 0.0;
        }
        let variance = vals.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / vals.len() as f32;
        let cv = (variance.sqrt() / mean).clamp(0.0, 1.5);
        let throughput = (mean / 120.0).clamp(0.0, 1.0);
        (0.55 * cv + 0.45 * throughput).clamp(0.0, 1.0)
    }
}

/// Shared between the H.264 and input WebSocket handlers.
#[derive(Default)]
pub struct AdaptiveStreamState {
    motion_score_bits: AtomicU32, // score * 1000
    client_glass_ms: AtomicU32,
    client_dec_ms: AtomicU32,
    tuning: AtomicU8, // 0 = interactive, 1 = motion
}

impl AdaptiveStreamState {
    pub fn set_motion_score(&self, score: f32) {
        self.motion_score_bits
            .store((score.clamp(0.0, 1.0) * 1000.0) as u32, Ordering::Relaxed);
    }

    pub fn motion_score(&self) -> f32 {
        self.motion_score_bits.load(Ordering::Relaxed) as f32 / 1000.0
    }

    pub fn update_client_stats(&self, glass_ms: u32, dec_ms: u32) {
        if glass_ms > 0 {
            self.client_glass_ms.store(glass_ms.min(5000), Ordering::Relaxed);
        }
        if dec_ms > 0 {
            self.client_dec_ms.store(dec_ms.min(5000), Ordering::Relaxed);
        }
    }

    pub fn client_glass_ms(&self) -> u32 {
        self.client_glass_ms.load(Ordering::Relaxed)
    }

    pub fn current_tuning(&self) -> PlaybackTuning {
        match self.tuning.load(Ordering::Relaxed) {
            1 => PlaybackTuning::Motion,
            _ => PlaybackTuning::Interactive,
        }
    }

    pub fn set_tuning(&self, tuning: PlaybackTuning) {
        self.tuning.store(
            match tuning {
                PlaybackTuning::Motion => 1,
                PlaybackTuning::Interactive => 0,
            },
            Ordering::Relaxed,
        );
    }

    /// Decide tuning from host motion + client glass latency.
    pub fn evaluate_tuning(&self) -> PlaybackTuning {
        let motion = self.motion_score();
        let glass = self.client_glass_ms();

        if motion > 0.42 {
            PlaybackTuning::Motion
        } else if glass > 45 && motion < 0.2 {
            PlaybackTuning::Interactive
        } else if motion > 0.28 {
            PlaybackTuning::Motion
        } else {
            PlaybackTuning::Interactive
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_chunks_score_low() {
        let mut det = MotionDetector::default();
        for _ in 0..20 {
            det.observe_chunk(2048);
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(det.score() < 0.35);
    }

    #[test]
    fn bursty_chunks_score_high() {
        let mut det = MotionDetector::default();
        for i in 0..30 {
            let size = if i % 3 == 0 { 48_000 } else { 4_000 };
            det.observe_chunk(size);
            std::thread::sleep(Duration::from_millis(30));
        }
        assert!(det.score() > 0.25);
    }
}
