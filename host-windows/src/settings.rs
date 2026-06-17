use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct ProbeCache {
    pub encoder: String,
    pub capture: String,
    pub nvenc_gpu: Option<u32>,
    pub amf_device: Option<u32>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct HostSettings {
    pub preferred_encoder: Option<String>,
    pub preferred_amf_device: Option<u32>,
    /// CUDA device index for NVENC (`-gpu N`). Separate from AMF adapter index.
    pub preferred_nvenc_gpu: Option<u32>,
    /// Named quality preset — overrides width/height/fps/bitrate when set.
    pub preferred_preset: Option<String>,
    // Legacy manual overrides (ignored when preferred_preset is set).
    pub preferred_width: Option<u32>,
    pub preferred_height: Option<u32>,
    pub preferred_bitrate_kbps: Option<u32>,
    /// Last known-good encoder/capture/GPU combination from a successful stream.
    #[serde(default)]
    pub encoder_probe_cache: Option<ProbeCache>,
    /// Keys like "h264_nvenc/Gdigrab/gpu0" that failed recently — skipped on reconnect.
    #[serde(default)]
    pub failed_probe_keys: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct EnvConfig {
    pub listen_host: String,
    pub port: u16,
    pub fps: Option<u32>,
    pub bitrate_kbps: Option<u32>,
    pub hw_encoder: Option<String>,
    pub capture: Option<String>,
    pub capture_width: Option<u32>,
    pub capture_height: Option<u32>,
    pub force_software_encoder: bool,
}

impl EnvConfig {
    pub fn load() -> Self {
        let force_software = std::env::var("FORCE_SOFTWARE_ENCODER")
            .ok()
            .map(|v| {
                let v = v.trim().to_ascii_lowercase();
                v == "1" || v == "true" || v == "yes"
            })
            .unwrap_or(false);

        Self {
            listen_host: std::env::var("FLEXDISPLAY_LISTEN")
                .unwrap_or_else(|_| "0.0.0.0".to_string()),
            port: std::env::var("FLEXDISPLAY_PORT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(9001),
            fps: std::env::var("FLEXDISPLAY_FPS")
                .ok()
                .and_then(|v| v.parse().ok()),
            bitrate_kbps: std::env::var("FLEXDISPLAY_BITRATE")
                .ok()
                .and_then(|v| v.parse().ok()),
            hw_encoder: std::env::var("FLEXDISPLAY_HW_ENCODER").ok(),
            capture: std::env::var("FLEXDISPLAY_CAPTURE").ok(),
            capture_width: std::env::var("FLEXDISPLAY_CAPTURE_WIDTH")
                .ok()
                .and_then(|v| v.parse().ok()),
            capture_height: std::env::var("FLEXDISPLAY_CAPTURE_HEIGHT")
                .ok()
                .and_then(|v| v.parse().ok()),
            force_software_encoder: force_software,
        }
    }
}

pub fn canonical_encoder(value: Option<String>) -> Option<String> {
    let raw = value?.trim().to_ascii_lowercase();
    match raw.as_str() {
        "h264_nvenc" | "h264_qsv" | "h264_amf" | "libx264" => Some(raw),
        _ => None,
    }
}

pub fn canonical_resolution(width: Option<u32>, height: Option<u32>) -> (Option<u32>, Option<u32>) {
    let allowed = [
        (800u32, 600u32),
        (1024u32, 768u32),
        (1280u32, 720u32),
        (1600u32, 900u32),
        (1920u32, 1080u32),
    ];
    match (width, height) {
        (Some(w), Some(h)) if allowed.contains(&(w, h)) => (Some(w), Some(h)),
        _ => (None, None),
    }
}

pub fn canonical_bitrate_kbps(value: Option<u32>) -> Option<u32> {
    let allowed = [
        3000u32, 4000u32, 5000u32, 8000u32, 10000u32, 12000u32, 15000u32, 18000u32, 25000u32,
        30000u32, 35000u32, 50000u32,
    ];
    let v = value?;
    if allowed.contains(&v) {
        Some(v)
    } else {
        None
    }
}

pub fn canonical_preset(value: Option<String>) -> Option<String> {
    let v = value?.trim().to_ascii_lowercase();
    match v.as_str() {
        "ahorro" | "cpu_safe" | "equilibrado" | "alta_720p" | "fluido_900p" | "full_hd"
        | "full_hd_max" => Some(v),
        _ => None,
    }
}

/// Returns (width, height, fps, bitrate_kbps) for a named quality preset.
pub fn resolve_preset(name: &str) -> Option<(u32, u32, u32, u32)> {
    match name {
        "ahorro" | "cpu_safe" => Some((960, 544, 30, 5_000)),
        "equilibrado" => Some((1280, 720, 60, 10_000)),
        "alta_720p" => Some((1280, 720, 60, 15_000)),
        "fluido_900p" => Some((1600, 900, 60, 20_000)),
        "full_hd" => Some((1920, 1080, 60, 25_000)),
        "full_hd_max" => Some((1920, 1080, 60, 35_000)),
        _ => None,
    }
}

pub fn settings_file_path() -> std::path::PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            return dir.join("host-settings.json");
        }
    }
    std::path::PathBuf::from("host-settings.json")
}

pub fn load_host_settings_from_disk() -> HostSettings {
    let path = settings_file_path();
    let Ok(text) = std::fs::read_to_string(path) else {
        return HostSettings::default();
    };
    let mut settings: HostSettings = serde_json::from_str(&text).unwrap_or_default();
    // Legacy migration: NVENC CUDA index was stored in preferred_amf_device.
    if settings.preferred_nvenc_gpu.is_none()
        && settings.preferred_encoder.as_deref() == Some("h264_nvenc")
    {
        settings.preferred_nvenc_gpu = settings.preferred_amf_device;
    }
    // Prefer DXGI capture for hardware encoders — cached gdigrab caused dup/drop at 1080p60.
    if let Some(cache) = settings.encoder_probe_cache.as_mut() {
        if cache.encoder != "libx264" && cache.capture.eq_ignore_ascii_case("gdigrab") {
            cache.capture = "ddagrab".to_string();
        }
    }
    settings
}

pub fn save_host_settings_to_disk(settings: &HostSettings) {
    let path = settings_file_path();
    if let Ok(text) = serde_json::to_string_pretty(settings) {
        let _ = std::fs::write(path, text);
    }
}

pub fn probe_cache_key(
    encoder: &str,
    capture: &str,
    nvenc_gpu: Option<u32>,
    amf_device: Option<u32>,
) -> String {
    let gpu = nvenc_gpu
        .map(|g| format!("gpu{g}"))
        .or_else(|| amf_device.map(|d| format!("amf{d}")))
        .unwrap_or_else(|| "default".to_string());
    format!("{encoder}/{capture}/{gpu}")
}
