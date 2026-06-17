use std::sync::atomic::{AtomicU64, Ordering};

/// Latest host timestamp (micros since UNIX epoch) when an H.264 chunk was sent to the client.
#[derive(Default)]
pub struct StreamLatencyState {
    last_send_us: AtomicU64,
}

impl StreamLatencyState {
    pub fn mark_send_now(&self) {
        let now_us = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_micros() as u64;
        self.last_send_us.store(now_us, Ordering::Relaxed);
    }

    pub fn last_send_us(&self) -> u64 {
        self.last_send_us.load(Ordering::Relaxed)
    }
}

pub fn host_now_us() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros() as u64
}
