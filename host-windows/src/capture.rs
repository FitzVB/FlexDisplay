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
    let software = encoder == Some("libx264");
    if stream_mode.eq_ignore_ascii_case("mirror") {
        if software {
            // Mirror + CPU encode: try DXGI first to reduce capture CPU, then GDI for accuracy.
            [Capture::Ddagrab, Capture::Gdigrab]
        } else {
            [Capture::Gdigrab, Capture::Ddagrab]
        }
    } else if prefer_capture_env == Some("gdigrab") {
        [Capture::Gdigrab, Capture::Ddagrab]
    } else {
        [Capture::Ddagrab, Capture::Gdigrab]
    }
}
