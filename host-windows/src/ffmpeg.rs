use crate::capture::Capture;
use crate::process_util;
use crate::encoder::{amf_device_from_pre_args, encoder_extra_args, PROBE_TIMEOUT_MS};
use crate::settings::{save_host_settings_to_disk, HostSettings};
use futures::SinkExt;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::sync::RwLock;
use tracing::{info, warn};
use warp::ws::Message;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamExit {
    Streamed,
    RestartRequested,
    Unavailable,
    SocketClosed,
}

pub struct FfmpegConfig {
    pub out_w: u32,
    pub out_h: u32,
    pub fps: u32,
    pub bitrate_kbps: u32,
    pub mode: String,
    pub fit: String,
    pub display_idx: u32,
    pub encoder: String,
    pub capture: Capture,
    pub pre_input_args: Vec<String>,
    pub nvenc_gpu: Option<u32>,
}

pub async fn stream_with_ffmpeg(
    ws_tx: &mut futures::stream::SplitSink<warp::ws::WebSocket, Message>,
    connection_alive: &Arc<std::sync::atomic::AtomicBool>,
    restart_requested: &Arc<std::sync::atomic::AtomicBool>,
    config: FfmpegConfig,
    amf_settings: Option<Arc<RwLock<HostSettings>>>,
) -> anyhow::Result<StreamExit> {
    let mut args: Vec<String> = Vec::new();
    args.extend(config.pre_input_args.iter().cloned());

    match config.capture {
        Capture::Ddagrab => {
            args.extend([
                "-f".into(),
                "lavfi".into(),
                "-i".into(),
                format!(
                    "ddagrab={}:framerate={},hwdownload,format=bgra",
                    config.display_idx, config.fps
                ),
            ]);
        }
        Capture::Gdigrab => {
            let maybe_target = crate::input::resolve_display_target(config.display_idx).ok();
            args.extend([
                "-probesize".into(),
                "32".into(),
                "-analyzeduration".into(),
                "0".into(),
                "-f".into(),
                "gdigrab".into(),
                "-framerate".into(),
                config.fps.to_string(),
            ]);
            if let Some(target) = maybe_target {
                args.extend([
                    "-offset_x".into(),
                    target.left().to_string(),
                    "-offset_y".into(),
                    target.top().to_string(),
                    "-video_size".into(),
                    format!("{}x{}", target.width(), target.height()),
                ]);
            }
            args.extend(["-i".into(), "desktop".into()]);
        }
    }

    args.extend([
        "-fflags".into(),
        "+nobuffer".into(),
        "-flags".into(),
        "+low_delay".into(),
        "-avioflags".into(),
        "direct".into(),
        "-fps_mode".into(),
        "cfr".into(),
    ]);

    let use_contain = config.mode.eq_ignore_ascii_case("mirror")
        && (config.fit.eq_ignore_ascii_case("contain") || config.fit.is_empty());
    let vf = if use_contain {
        format!(
            "scale={}:{}:force_original_aspect_ratio=decrease,pad={}:{}:(ow-iw)/2:(oh-ih)/2:black,format=yuv420p",
            config.out_w, config.out_h, config.out_w, config.out_h
        )
    } else {
        format!(
            "scale={}:{}:force_original_aspect_ratio=increase,crop={}:{}:(iw-{})/2:(ih-{})/2,format=yuv420p",
            config.out_w, config.out_h, config.out_w, config.out_h, config.out_w, config.out_h
        )
    };
    args.extend(["-vf".into(), vf, "-an".into()]);
    args.extend(["-r".into(), config.fps.to_string()]);
    args.extend(["-c:v".into(), config.encoder.clone()]);
    args.extend(encoder_extra_args(&config.encoder));

    if config.encoder == "h264_nvenc" {
        if let Some(gpu_idx) = config.nvenc_gpu {
            args.extend(["-gpu".into(), gpu_idx.to_string()]);
        }
    }

    let effective_bitrate_kbps = if config.encoder == "libx264" {
        config.bitrate_kbps.min(12000)
    } else if config.encoder == "h264_nvenc" {
        config.bitrate_kbps.min(8000)
    } else if config.encoder == "h264_amf" {
        config.bitrate_kbps.clamp(5_000, 18_000)
    } else {
        config.bitrate_kbps
    };

    let bufsize = match config.encoder.as_str() {
        "h264_amf" => effective_bitrate_kbps / 2,
        // Tighter VBV for NVENC — less end-to-end buffering over USB.
        "h264_nvenc" => effective_bitrate_kbps / 8,
        _ => effective_bitrate_kbps / 4,
    };
    let gop = match config.encoder.as_str() {
        "h264_amf" => config.fps.clamp(30, 60),
        "libx264" => config.fps.clamp(15, 30),
        // One IDR per second at 60 fps — avoids 150 KB keyframes every 0.5 s.
        "h264_nvenc" => config.fps.clamp(30, 120),
        _ => (config.fps / 2).clamp(15, 30),
    };
    args.extend([
        "-g".into(),
        gop.to_string(),
        "-b:v".into(),
        format!("{}k", effective_bitrate_kbps),
        "-maxrate".into(),
        format!("{}k", effective_bitrate_kbps),
        "-bufsize".into(),
        format!("{}k", bufsize),
        "-bsf:v".into(),
        "dump_extra".into(),
        "-f".into(),
        "h264".into(),
        "-".into(),
    ]);

    let mut cmd = Command::new(ffmpeg_exe());
    process_util::hide_tokio_command(&mut cmd);
    cmd.args(&args)
        .kill_on_drop(true)
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped());

    info!(
        encoder = %config.encoder,
        capture = %config.capture,
        nvenc_gpu = ?config.nvenc_gpu,
        cmd = %format!("ffmpeg {}", args.join(" ")),
        "ffmpeg invocation"
    );

    let mut child = cmd.spawn()?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow::anyhow!("ffmpeg stdout unavailable"))?;

    let enc_name_for_log = config.encoder.clone();
    let nvenc_gpu_for_log = config.nvenc_gpu;
    let capture_for_log = config.capture;
    if let Some(stderr) = child.stderr.take() {
        let log_path = ffmpeg_log_path(&config.encoder);
        tokio::spawn(async move {
            use std::io::Write;
            use tokio::io::AsyncBufReadExt;
            let mut log_file: Option<std::fs::File> = tokio::task::block_in_place(|| {
                if let Some(parent) = log_path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&log_path)
                    .ok()
            });
            if let Some(ref mut f) = log_file {
                let _ = writeln!(
                    f,
                    "=== ffmpeg attempt encoder={enc_name_for_log} capture={capture_for_log} nvenc_gpu={nvenc_gpu_for_log:?} ==="
                );
            }
            let mut lines = tokio::io::BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let low = line.to_ascii_lowercase();
                if low.contains("error")
                    || low.contains("invalid")
                    || low.contains("could not")
                    || low.contains("no such")
                    || low.contains("failed")
                {
                    tracing::warn!(encoder = %enc_name_for_log, "ffmpeg: {}", line);
                } else {
                    tracing::debug!(target: "ffmpeg", "{}", line);
                }
                if let Some(ref mut f) = log_file {
                    let _ = writeln!(f, "{line}");
                }
            }
        });
    }

    let mut buf = vec![0u8; 4 * 1024];
    let mut sent_any = false;
    let probe_deadline =
        tokio::time::Instant::now() + tokio::time::Duration::from_millis(PROBE_TIMEOUT_MS);

    let mut tick = tokio::time::interval(tokio::time::Duration::from_millis(100));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut ticker_announced = false;

    while connection_alive.load(Ordering::Relaxed) && !restart_requested.load(Ordering::Relaxed) {
        tokio::select! {
            result = async {
                if !sent_any {
                    let remaining = probe_deadline.saturating_duration_since(tokio::time::Instant::now());
                    if remaining.is_zero() {
                        return Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "probe timeout"));
                    }
                    match tokio::time::timeout(remaining, stdout.read(&mut buf)).await {
                        Ok(r) => r,
                        Err(_) => Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "probe timeout")),
                    }
                } else {
                    stdout.read(&mut buf).await
                }
            } => {
                match result {
                    Ok(0) => break,
                    Ok(n) => {
                        if !sent_any {
                            info!(encoder = %config.encoder, "first H.264 bytes sent to client ({n} bytes)");
                            if config.encoder == "h264_amf" {
                                let successful_device = amf_device_from_pre_args(&config.pre_input_args);
                                if let Some(settings) = amf_settings.clone() {
                                    let maybe_updated = {
                                        let mut write = settings.write().await;
                                        if write.preferred_amf_device != successful_device {
                                            write.preferred_amf_device = successful_device;
                                            Some(write.clone())
                                        } else {
                                            None
                                        }
                                    };
                                    if let Some(updated) = maybe_updated {
                                        let to_save = updated.clone();
                                        tokio::spawn(async move {
                                            let _ = tokio::task::spawn_blocking(move || {
                                                save_host_settings_to_disk(&to_save);
                                            })
                                            .await;
                                            info!(
                                                amf_device = ?updated.preferred_amf_device,
                                                "persisted AMF device from active stream start"
                                            );
                                        });
                                    }
                                }
                            }
                        }
                        sent_any = true;
                        let send_result = tokio::time::timeout(
                            tokio::time::Duration::from_secs(2),
                            ws_tx.send(Message::binary(buf[..n].to_vec())),
                        )
                        .await;
                        match send_result {
                            Ok(Ok(())) => {}
                            Ok(Err(_)) => return Ok(StreamExit::SocketClosed),
                            Err(_elapsed) => {
                                warn!(encoder = %config.encoder, "WebSocket send stalled >2 s — Android decoder overloaded, restarting stream");
                                return Ok(StreamExit::SocketClosed);
                            }
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {
                        warn!(encoder = %config.encoder, timeout_ms = PROBE_TIMEOUT_MS, "encoder probe timed out with zero output");
                        let _ = child.kill().await;
                        return Ok(StreamExit::Unavailable);
                    }
                    Err(e) => return Err(e.into()),
                }
            }
            _ = tick.tick() => {
                let now_us = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_micros();
                if !ticker_announced {
                    info!(encoder = %config.encoder, "e2e ticker active (T: frames every 100ms)");
                    ticker_announced = true;
                }
                let _ = ws_tx.send(Message::text(format!("T:{now_us}"))).await;
            }
        }
    }

    let _ = child.kill().await;

    if !connection_alive.load(Ordering::Relaxed) {
        return Ok(StreamExit::SocketClosed);
    }
    if restart_requested.load(Ordering::Relaxed) {
        return Ok(StreamExit::RestartRequested);
    }
    if sent_any {
        Ok(StreamExit::Streamed)
    } else {
        Ok(StreamExit::Unavailable)
    }
}

pub fn ffmpeg_log_path(encoder: &str) -> std::path::PathBuf {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let secs_per_day = 86400u64;
    let secs_per_hour = 3600u64;
    let secs_per_min = 60u64;
    let days = now / secs_per_day;
    let rem = now % secs_per_day;
    let hh = rem / secs_per_hour;
    let mm = (rem % secs_per_hour) / secs_per_min;
    let ss = rem % secs_per_min;
    let (mut year, mut month, mut day_of_month) = (1970u64, 1u64, 1u64);
    let mut d = days;
    loop {
        let leap = if year % 400 == 0 || (year % 4 == 0 && year % 100 != 0) {
            366
        } else {
            365
        };
        if d < leap {
            break;
        }
        d -= leap;
        year += 1;
    }
    let leap = if year % 400 == 0 || (year % 4 == 0 && year % 100 != 0) {
        1
    } else {
        0
    };
    let months = [31u64, 28 + leap, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    for &m in &months {
        if d < m {
            break;
        }
        d -= m;
        month += 1;
    }
    day_of_month += d;
    let ts = format!("{year:04}{month:02}{day_of_month:02}-{hh:02}{mm:02}{ss:02}");
    let safe_enc = encoder.replace([':', '/'], "_");
    let filename = format!("ffmpeg-{safe_enc}-{ts}.txt");
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            return dir.join("logs").join(filename);
        }
    }
    std::path::PathBuf::from("logs").join(filename)
}

pub fn ffmpeg_exe() -> std::path::PathBuf {
    if let Ok(custom) = std::env::var("FLEXDISPLAY_FFMPEG") {
        let p = std::path::PathBuf::from(custom);
        if p.exists() {
            return p;
        }
    }

    if let Ok(exe) = std::env::current_exe() {
        let name = if cfg!(windows) {
            "ffmpeg.exe"
        } else {
            "ffmpeg"
        };
        let sibling = exe.with_file_name(name);
        if sibling.exists() {
            return sibling;
        }
        if let Some(dir) = exe.parent() {
            let in_runtime = dir.join(".runtime").join("ffmpeg").join("bin").join(name);
            if in_runtime.exists() {
                return in_runtime;
            }
            let in_bin = dir.join("bin").join(name);
            if in_bin.exists() {
                return in_bin;
            }
        }
    }

    if cfg!(windows) {
        if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
            let root = std::path::Path::new(&local_app_data)
                .join("Microsoft")
                .join("WinGet")
                .join("Packages");
            if let Ok(entries) = std::fs::read_dir(root) {
                for entry in entries.flatten() {
                    let package_dir = entry.path();
                    let Some(name) = package_dir.file_name().and_then(|n| n.to_str()) else {
                        continue;
                    };
                    if !name.starts_with("Gyan.FFmpeg_") {
                        continue;
                    }
                    if let Ok(children) = std::fs::read_dir(&package_dir) {
                        for child in children.flatten() {
                            let ffmpeg_bin = child.path().join("bin").join("ffmpeg.exe");
                            if ffmpeg_bin.exists() {
                                return ffmpeg_bin;
                            }
                        }
                    }
                }
            }
        }
    }

    std::path::PathBuf::from(if cfg!(windows) {
        "ffmpeg.exe"
    } else {
        "ffmpeg"
    })
}

pub fn adb_exe() -> std::path::PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        let name = if cfg!(windows) { "adb.exe" } else { "adb" };
        let sibling = exe.with_file_name(name);
        if sibling.exists() {
            return sibling;
        }
        if let Some(dir) = exe.parent() {
            let in_bin = dir.join("bin").join(name);
            if in_bin.exists() {
                return in_bin;
            }
        }
    }
    std::path::PathBuf::from(if cfg!(windows) { "adb.exe" } else { "adb" })
}
