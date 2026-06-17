use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Capture {
    /// DXGI Desktop Duplication — GPU-accelerated, lower CPU cost.
    Ddagrab,
    /// GDI screen grab — coordinate-accurate for mirror mode.
    Gdigrab,
}

use serde::{Deserialize, Serialize};

impl fmt::Display for Capture {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Capture::Ddagrab => write!(f, "ddagrab"),
            Capture::Gdigrab => write!(f, "gdigrab"),
        }
    }
}

impl Capture {
    pub fn from_str_loose(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "ddagrab" => Some(Capture::Ddagrab),
            "gdigrab" => Some(Capture::Gdigrab),
            _ => None,
        }
    }
}

/// Capture backend priority order for encoder trials.
pub fn capture_order(
    stream_mode: &str,
    prefer_capture_env: Option<&str>,
    encoder: Option<&str>,
) -> [Capture; 2] {
    if let Some(pref) = prefer_capture_env.and_then(Capture::from_str_loose) {
        let other = if pref == Capture::Ddagrab {
            Capture::Gdigrab
        } else {
            Capture::Ddagrab
        };
        return [pref, other];
    }

    let software = encoder == Some("libx264");
    if stream_mode.eq_ignore_ascii_case("mirror") {
        if software {
            // Mirror + CPU encode: try DXGI first to reduce capture CPU, then GDI for accuracy.
            [Capture::Ddagrab, Capture::Gdigrab]
        } else {
            // Mirror + HW encode: DXGI avoids gdigrab dup/drop at 1080p60; GDI remains fallback.
            [Capture::Ddagrab, Capture::Gdigrab]
        }
    } else {
        [Capture::Ddagrab, Capture::Gdigrab]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mirror_hw_prefers_ddagrab() {
        let order = capture_order("mirror", None, Some("h264_nvenc"));
        assert_eq!(order[0], Capture::Ddagrab);
        assert_eq!(order[1], Capture::Gdigrab);
    }

    #[test]
    fn env_override_wins() {
        let order = capture_order("mirror", Some("gdigrab"), Some("h264_nvenc"));
        assert_eq!(order[0], Capture::Gdigrab);
    }
}
