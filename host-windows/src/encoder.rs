use crate::capture::Capture;
use crate::ffmpeg::ffmpeg_exe;
use crate::settings::{probe_cache_key, HostSettings, ProbeCache};
use serde::Serialize;
use tracing::info;

pub const HW_ENCODERS: [&str; 3] = ["h264_nvenc", "h264_qsv", "h264_amf"];
pub const ALL_ENCODERS: [&str; 4] = ["h264_nvenc", "h264_qsv", "h264_amf", "libx264"];
pub const PROBE_TIMEOUT_MS: u64 = 1500;
pub const MAX_FAILED_PROBE_KEYS: usize = 32;

#[derive(Clone, Debug, Serialize)]
pub struct GpuInfo {
    pub index: usize,
    pub name: String,
    pub driver_version: String,
}

#[derive(Clone, Debug, Default)]
pub struct GpuVendorInfo {
    pub has_nvidia: bool,
    pub has_amd: bool,
    pub has_intel: bool,
}

impl GpuVendorInfo {
    pub fn from_gpus(gpus: &[GpuInfo]) -> Self {
        let mut info = Self::default();
        for gpu in gpus {
            let name = gpu.name.to_ascii_lowercase();
            if name.contains("nvidia")
                || name.contains("geforce")
                || name.contains("rtx")
                || name.contains("gtx")
                || name.contains("quadro")
            {
                info.has_nvidia = true;
            }
            if name.contains("amd") || name.contains("radeon") {
                info.has_amd = true;
            }
            if name.contains("intel") {
                info.has_intel = true;
            }
        }
        info
    }
}

pub fn detect_gpus() -> Vec<GpuInfo> {
    #[derive(Debug, serde::Deserialize)]
    struct PsGpu {
        #[serde(rename = "Name")]
        name: Option<String>,
        #[serde(rename = "DriverVersion")]
        driver_version: Option<String>,
    }

    let ps = r#"Get-CimInstance Win32_VideoController |
Select-Object Name,DriverVersion |
ConvertTo-Json -Compress"#;
    let output = std::process::Command::new("powershell")
        .args(["-NoProfile", "-Command", ps])
        .output();

    let Ok(out) = output else {
        return Vec::new();
    };

    if out.stdout.is_empty() {
        return Vec::new();
    }

    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if text.is_empty() {
        return Vec::new();
    }

    let mut parsed: Vec<PsGpu> = match serde_json::from_str::<Vec<PsGpu>>(&text) {
        Ok(v) => v,
        Err(_) => match serde_json::from_str::<PsGpu>(&text) {
            Ok(one) => vec![one],
            Err(_) => Vec::new(),
        },
    };

    parsed
        .drain(..)
        .enumerate()
        .map(|(idx, g)| GpuInfo {
            index: idx,
            name: g.name.unwrap_or_else(|| "Unknown GPU".to_string()),
            driver_version: g.driver_version.unwrap_or_default(),
        })
        .collect()
}

fn ffmpeg_lists_encoder(text: &str, enc: &str) -> bool {
    text.contains(enc)
}

/// Static FFmpeg encoder list filtered by detected GPU vendors.
pub fn detect_available_h264_encoders(gpus: &[GpuInfo], force_software: bool) -> Vec<String> {
    if force_software {
        return vec!["libx264".to_string()];
    }

    let output = std::process::Command::new(ffmpeg_exe())
        .arg("-hide_banner")
        .arg("-encoders")
        .output();

    let Ok(out) = output else {
        return vec!["libx264".to_string()];
    };

    let text = String::from_utf8_lossy(&out.stdout).to_ascii_lowercase();
    let vendors = GpuVendorInfo::from_gpus(gpus);
    let mut found = Vec::new();

    for enc in ALL_ENCODERS {
        if !ffmpeg_lists_encoder(&text, enc) {
            continue;
        }
        match enc {
            "h264_nvenc" if !vendors.has_nvidia => continue,
            "h264_amf" if !vendors.has_amd => continue,
            "h264_qsv" if !vendors.has_intel => continue,
            _ => {}
        }
        found.push(enc.to_string());
    }

    if found.is_empty() {
        found.push("libx264".to_string());
    }
    found
}

#[derive(Clone, Debug)]
pub struct EncoderCandidate {
    pub encoder: String,
    pub capture: Capture,
    pub pre_input_args: Vec<String>,
    pub nvenc_gpu: Option<u32>,
    pub amf_device: Option<u32>,
}

fn push_encoder_matrix(
    out: &mut Vec<EncoderCandidate>,
    encoder: &str,
    capture_order: [Capture; 2],
    amf_pre_args: &[Vec<String>],
    nvenc_gpu_candidates: &[Option<u32>],
) {
    if encoder == "h264_amf" {
        for cap in capture_order {
            for pre in amf_pre_args {
                out.push(EncoderCandidate {
                    encoder: encoder.into(),
                    capture: cap,
                    pre_input_args: pre.clone(),
                    nvenc_gpu: None,
                    amf_device: amf_device_from_pre_args(pre),
                });
            }
        }
    } else if encoder == "h264_nvenc" {
        for cap in capture_order {
            for &gpu in nvenc_gpu_candidates {
                out.push(EncoderCandidate {
                    encoder: encoder.into(),
                    capture: cap,
                    pre_input_args: vec![],
                    nvenc_gpu: gpu,
                    amf_device: None,
                });
            }
        }
    } else {
        for cap in capture_order {
            out.push(EncoderCandidate {
                encoder: encoder.into(),
                capture: cap,
                pre_input_args: vec![],
                nvenc_gpu: None,
                amf_device: None,
            });
        }
    }
}

pub fn amf_device_from_pre_args(pre_args: &[String]) -> Option<u32> {
    if pre_args.len() < 2 || pre_args[0] != "-init_hw_device" {
        return None;
    }
    let spec = pre_args[1].strip_prefix("d3d11va=amf_dx:")?;
    spec.parse::<u32>().ok()
}

pub struct CandidateBuildOptions<'a> {
    pub available_encoders: &'a [String],
    pub preferred_encoder: Option<&'a str>,
    pub manual_encoder_lock: bool,
    pub stream_mode: &'a str,
    pub capture_env: Option<&'a str>,
    pub settings: &'a HostSettings,
    pub gpus: &'a [GpuInfo],
    pub force_software: bool,
}

/// Build ordered encoder×capture×GPU trial list with cache fast-path and manual lock.
pub fn build_encoder_candidates(opts: &CandidateBuildOptions<'_>) -> Vec<EncoderCandidate> {
    let vendors = GpuVendorInfo::from_gpus(opts.gpus);
    let gpu_count = opts.gpus.len().min(10) as u32;

    let mut amf_device_order: Vec<u32> = Vec::new();
    if let Some(idx) = opts.settings.preferred_amf_device {
        amf_device_order.push(idx);
    }
    for idx in 0..gpu_count {
        if Some(idx) != opts.settings.preferred_amf_device {
            amf_device_order.push(idx);
        }
    }
    let mut amf_pre_args: Vec<Vec<String>> = amf_device_order
        .iter()
        .map(|idx| vec!["-init_hw_device".into(), format!("d3d11va=amf_dx:{idx}")])
        .collect();
    amf_pre_args.push(vec![]);

    let mut nvenc_gpu_candidates: Vec<Option<u32>> = Vec::new();
    if let Some(preferred) = opts.settings.preferred_nvenc_gpu {
        nvenc_gpu_candidates.push(Some(preferred));
    }
    for idx in 0..gpu_count.max(1) {
        if Some(idx) != opts.settings.preferred_nvenc_gpu {
            nvenc_gpu_candidates.push(Some(idx));
        }
    }
    nvenc_gpu_candidates.push(None);

    let hw: Vec<&str> = HW_ENCODERS
        .iter()
        .copied()
        .filter(|e| opts.available_encoders.iter().any(|a| a == e))
        .collect();

    let skip_hw = opts.force_software
        || (!vendors.has_nvidia && !vendors.has_amd && !vendors.has_intel)
        || hw.is_empty();

    let preferred = opts.preferred_encoder;
    let capture_order_for = |enc: Option<&str>| {
        crate::capture::capture_order(opts.stream_mode, opts.capture_env, enc)
    };

    let mut candidates: Vec<EncoderCandidate> = Vec::new();
    let failed: std::collections::HashSet<String> = opts
        .settings
        .failed_probe_keys
        .iter()
        .cloned()
        .collect();

    let filter_failed = |list: Vec<EncoderCandidate>| -> Vec<EncoderCandidate> {
        list.into_iter()
            .filter(|c| {
                let key = probe_cache_key(
                    &c.encoder,
                    &c.capture.to_string(),
                    c.nvenc_gpu,
                    c.amf_device,
                );
                !failed.contains(&key)
            })
            .collect()
    };

    // Fast-path: cached working combo first.
    if let Some(cache) = &opts.settings.encoder_probe_cache {
        if let Some(cap) = Capture::from_str_loose(&cache.capture) {
            let nvenc = cache.nvenc_gpu;
            let amf_pre = cache
                .amf_device
                .map(|d| vec!["-init_hw_device".into(), format!("d3d11va=amf_dx:{d}")])
                .unwrap_or_default();
            let key = probe_cache_key(&cache.encoder, &cache.capture, nvenc, cache.amf_device);
            if !failed.contains(&key) {
                candidates.push(EncoderCandidate {
                    encoder: cache.encoder.clone(),
                    capture: cap,
                    pre_input_args: amf_pre,
                    nvenc_gpu: nvenc,
                    amf_device: cache.amf_device,
                });
                info!(encoder = %cache.encoder, capture = %cache.capture, "using cached encoder probe result");
            }
        }
    }

    let add_encoder = |out: &mut Vec<EncoderCandidate>, enc: &str| {
        let order = capture_order_for(Some(enc));
        push_encoder_matrix(out, enc, order, &amf_pre_args, &nvenc_gpu_candidates);
    };

    if let Some(pref) = preferred {
        if pref == "libx264" || hw.contains(&pref) {
            add_encoder(&mut candidates, pref);
        }
        if opts.manual_encoder_lock {
            return dedupe_candidates(filter_failed(candidates));
        }
    }

    if !skip_hw {
        for enc in hw.iter().copied() {
            if preferred != Some(enc) {
                add_encoder(&mut candidates, enc);
            }
        }
    }

    if preferred != Some("libx264") {
        add_encoder(&mut candidates, "libx264");
    }

    dedupe_candidates(filter_failed(candidates))
}

fn dedupe_candidates(list: Vec<EncoderCandidate>) -> Vec<EncoderCandidate> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for c in list {
        let key = probe_cache_key(
            &c.encoder,
            &c.capture.to_string(),
            c.nvenc_gpu,
            c.amf_device,
        );
        if seen.insert(key) {
            out.push(c);
        }
    }
    out
}

pub fn cpu_thread_count() -> u32 {
    std::thread::available_parallelism()
        .map(|n| n.get() as u32)
        .unwrap_or(4)
        .max(1)
}

/// Per-encoder low-latency arguments.
pub fn encoder_extra_args(encoder: &str) -> Vec<String> {
    let sliced_threads = if cpu_thread_count() >= 4 { "1" } else { "0" };
    match encoder {
        "h264_nvenc" => vec![
            "-preset".into(),
            "p1".into(),
            "-tune".into(),
            "ll".into(),
            "-rc".into(),
            "cbr".into(),
            "-bf".into(),
            "0".into(),
            "-zerolatency".into(),
            "1".into(),
            "-profile:v".into(),
            "main".into(),
            "-aud".into(),
            "1".into(),
            "-level".into(),
            "5.1".into(),
        ],
        "h264_qsv" => vec![
            "-preset".into(),
            "veryfast".into(),
            "-async_depth".into(),
            "1".into(),
            "-bf".into(),
            "0".into(),
        ],
        "h264_amf" => vec![
            "-usage".into(),
            "lowlatency".into(),
            "-quality".into(),
            "balanced".into(),
            "-latency".into(),
            "true".into(),
            "-rc".into(),
            "cbr".into(),
            "-async_depth".into(),
            "1".into(),
            "-profile".into(),
            "baseline".into(),
            "-coder".into(),
            "cavlc".into(),
            "-bf".into(),
            "0".into(),
            "-max_b_frames".into(),
            "0".into(),
            "-vbaq".into(),
            "true".into(),
        ],
        "libx264" => vec![
            "-preset".into(),
            "ultrafast".into(),
            "-tune".into(),
            "zerolatency".into(),
            "-profile:v".into(),
            "baseline".into(),
            "-level".into(),
            "5.1".into(),
            "-x264-params".into(),
            format!(
                "bframes=0:scenecut=0:ref=1:cabac=0:rc-lookahead=0:sync-lookahead=0:repeat-headers=1:aud=1:sliced-threads={sliced_threads}"
            ),
        ],
        _ => vec!["-bf".into(), "0".into()],
    }
}

pub fn record_probe_success(settings: &mut HostSettings, candidate: &EncoderCandidate) {
    settings.encoder_probe_cache = Some(ProbeCache {
        encoder: candidate.encoder.clone(),
        capture: candidate.capture.to_string(),
        nvenc_gpu: candidate.nvenc_gpu,
        amf_device: candidate.amf_device,
    });
    let key = probe_cache_key(
        &candidate.encoder,
        &candidate.capture.to_string(),
        candidate.nvenc_gpu,
        candidate.amf_device,
    );
    settings.failed_probe_keys.retain(|k| k != &key);
}

pub fn record_probe_failure(settings: &mut HostSettings, candidate: &EncoderCandidate) {
    let key = probe_cache_key(
        &candidate.encoder,
        &candidate.capture.to_string(),
        candidate.nvenc_gpu,
        candidate.amf_device,
    );
    if !settings.failed_probe_keys.contains(&key) {
        settings.failed_probe_keys.push(key);
        if settings.failed_probe_keys.len() > MAX_FAILED_PROBE_KEYS {
            let drain = settings.failed_probe_keys.len() - MAX_FAILED_PROBE_KEYS;
            settings.failed_probe_keys.drain(0..drain);
        }
    }
}
