use crate::encoder::{
    build_encoder_candidates, detect_available_h264_encoders, detect_gpus, record_probe_failure,
    record_probe_success, CandidateBuildOptions, EncoderCandidate,
};
use crate::ffmpeg::{stream_with_ffmpeg, FfmpegConfig, StreamExit};
use crate::profile::{
    apply_encoder_profile_caps, resolve_base_profile, BaseProfileRequest, StreamProfile,
    TransportKind,
};
use crate::settings::{canonical_encoder, resolve_preset, save_host_settings_to_disk, HostSettings};
use futures::{SinkExt, StreamExt};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tokio::sync::{watch, RwLock};
use tracing::{error, info, warn};
use warp::ws::Message;

#[derive(Clone, Debug, serde::Deserialize, Default)]
pub struct StreamQuery {
    pub w: Option<u32>,
    pub h: Option<u32>,
    pub fps: Option<u32>,
    pub bitrate_kbps: Option<u32>,
    pub encoder: Option<String>,
    pub mode: Option<String>,
    pub display: Option<u32>,
    pub fit: Option<String>,
    /// `usb` or `wifi` — set by Android for adaptive profile selection.
    pub transport: Option<String>,
    /// `auto` (default) or `manual` — when `manual`, legacy host-first geometry applies.
    pub profile: Option<String>,
}

pub async fn handle_h264_stream(
    socket: warp::ws::WebSocket,
    query: StreamQuery,
    settings: Arc<RwLock<HostSettings>>,
    mut reload_rx: watch::Receiver<u64>,
    env: crate::settings::EnvConfig,
) {
    let (mut ws_tx, mut ws_rx) = socket.split();
    let _ = *reload_rx.borrow_and_update();
    let connection_alive = Arc::new(AtomicBool::new(true));
    let connection_alive_watcher = connection_alive.clone();
    let restart_requested = Arc::new(AtomicBool::new(false));
    let restart_requested_watcher = restart_requested.clone();

    let ws_close_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = ws_rx.next().await {
            if msg.is_close() {
                break;
            }
        }
        connection_alive_watcher.store(false, Ordering::Relaxed);
    });

    let reload_watch_task = tokio::spawn(async move {
        while reload_rx.changed().await.is_ok() {
            info!("settings changed, requesting active h264 stream restart");
            restart_requested_watcher.store(true, Ordering::Relaxed);
        }
    });

    let gpus = detect_gpus();
    let available_encoders =
        detect_available_h264_encoders(&gpus, env.force_software_encoder);

    while connection_alive.load(Ordering::Relaxed) {
        restart_requested.store(false, Ordering::Relaxed);

        let settings_snapshot = settings.read().await.clone();

        let stream_mode = query
            .mode
            .as_deref()
            .map(|m| m.to_ascii_lowercase())
            .unwrap_or_else(|| "mirror".to_string());

        let default_display_idx = if stream_mode.eq_ignore_ascii_case("extended") {
            crate::input::default_display_for_mode(&stream_mode).unwrap_or(0)
        } else {
            0
        };
        let display_idx = query.display.unwrap_or(default_display_idx).clamp(0, 9);

        let (preset_w, preset_h, preset_fps, preset_bitrate) =
            if let Some(ref pname) = settings_snapshot.preferred_preset {
                if let Some((pw, ph, pfps, pbr)) = resolve_preset(pname) {
                    (Some(pw), Some(ph), Some(pfps), Some(pbr))
                } else {
                    (None, None, None, None)
                }
            } else {
                (None, None, None, None)
            };

        let preset_active = settings_snapshot.preferred_preset.is_some();
        let adaptive = !preset_active
            && query
                .profile
                .as_deref()
                .map(|p| p.eq_ignore_ascii_case("manual"))
                != Some(true);
        let transport = TransportKind::parse(query.transport.as_deref());

        let (mirror_host_w, mirror_host_h) = if stream_mode.eq_ignore_ascii_case("mirror") {
            if let Ok(target) = crate::input::resolve_display_target(display_idx) {
                (
                    Some(target.width().max(1) as u32),
                    Some(target.height().max(1) as u32),
                )
            } else {
                (None, None)
            }
        } else {
            (None, None)
        };

        if stream_mode.eq_ignore_ascii_case("mirror") {
            info!(display_idx, mirror_host_w = ?mirror_host_w, mirror_host_h = ?mirror_host_h, "mirror host display geometry resolved");
        }

        let env_w = env.capture_width;
        let env_h = env.capture_height;

        let base_profile = resolve_base_profile(&BaseProfileRequest {
            preset_active,
            preset_w,
            preset_h,
            preset_fps,
            preset_bitrate,
            manual_w: settings_snapshot.preferred_width,
            manual_h: settings_snapshot.preferred_height,
            manual_bitrate: settings_snapshot.preferred_bitrate_kbps,
            env_w,
            env_h,
            env_fps: env.fps,
            env_bitrate: env.bitrate_kbps,
            client_w: query.w,
            client_h: query.h,
            client_fps: query.fps,
            client_bitrate: query.bitrate_kbps,
            mirror_host_w,
            mirror_host_h,
            transport,
            adaptive,
        });

        let out_w = base_profile.w;
        let out_h = base_profile.h;
        let fps = base_profile.fps;
        let bitrate = base_profile.bitrate_kbps;

        let profile_label = if preset_active {
            settings_snapshot
                .preferred_preset
                .clone()
                .unwrap_or_else(|| "preset".into())
        } else if adaptive {
            "auto".into()
        } else {
            "manual".into()
        };
        let transport_label = match transport {
            TransportKind::Usb => "usb",
            TransportKind::Wifi => "wifi",
            TransportKind::Unknown => "unknown",
        };

        let preferred = settings_snapshot
            .preferred_encoder
            .clone()
            .or_else(|| query.encoder.clone())
            .or_else(|| env.hw_encoder.clone())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .and_then(|s| canonical_encoder(Some(s)));
        let manual_encoder_lock = preferred.is_some();

        let fit = query
            .fit
            .as_deref()
            .map(|f| f.to_ascii_lowercase())
            .unwrap_or_else(|| {
                if stream_mode.eq_ignore_ascii_case("mirror") {
                    "contain".into()
                } else {
                    "cover".into()
                }
            });

        let capture_env_owned = env
            .capture
            .clone()
            .or_else(|| std::env::var("FLEXDISPLAY_CAPTURE").ok());
        let capture_env_ref = capture_env_owned.as_deref();

        let candidates = build_encoder_candidates(&CandidateBuildOptions {
            available_encoders: &available_encoders,
            preferred_encoder: preferred.as_deref(),
            manual_encoder_lock,
            stream_mode: &stream_mode,
            capture_env: capture_env_ref,
            settings: &settings_snapshot,
            gpus: &gpus,
            force_software: env.force_software_encoder,
        });

        if manual_encoder_lock {
            info!(preferred = ?preferred, "manual encoder lock: no HW fallback after preferred");
        }

        info!(
            out_w,
            out_h,
            fps,
            bitrate,
            adaptive,
            transport = transport_label,
            profile = %profile_label,
            candidates = candidates.len(),
            "h264 stream active profile"
        );

        let mut started = false;
        let mut winning_candidate: Option<EncoderCandidate> = None;

        for candidate in &candidates {
            if !connection_alive.load(Ordering::Relaxed) {
                break;
            }
            if restart_requested.load(Ordering::Relaxed) {
                break;
            }

            let base = StreamProfile {
                w: out_w,
                h: out_h,
                fps,
                bitrate_kbps: bitrate,
            };
            let eff = apply_encoder_profile_caps(&candidate.encoder, transport, base);
            let (eff_w, eff_h, eff_fps, eff_bitrate) =
                (eff.w, eff.h, eff.fps, eff.bitrate_kbps);

            let profile_msg = format!(
                "CFG:encoder={};capture={};w={};h={};fps={};bitrate_kbps={};profile={};transport={}",
                candidate.encoder,
                candidate.capture,
                eff_w,
                eff_h,
                eff_fps,
                eff_bitrate,
                profile_label,
                transport_label
            );
            let _ = ws_tx.send(Message::text(profile_msg)).await;

            info!(
                encoder = %candidate.encoder,
                capture = %candidate.capture,
                amf_device = ?candidate.amf_device,
                nvenc_gpu = ?candidate.nvenc_gpu,
                eff_w,
                eff_h,
                eff_fps,
                "trying encoder/capture combination"
            );

            match stream_with_ffmpeg(
                &mut ws_tx,
                &connection_alive,
                &restart_requested,
                FfmpegConfig {
                    out_w: eff_w,
                    out_h: eff_h,
                    fps: eff_fps,
                    bitrate_kbps: eff_bitrate,
                    mode: stream_mode.clone(),
                    fit: fit.clone(),
                    display_idx,
                    encoder: candidate.encoder.clone(),
                    capture: candidate.capture,
                    pre_input_args: candidate.pre_input_args.clone(),
                    nvenc_gpu: candidate.nvenc_gpu,
                },
                if candidate.encoder == "h264_amf" {
                    Some(settings.clone())
                } else {
                    None
                },
            )
            .await
            {
                Ok(exit) => match exit {
                    StreamExit::Streamed => {
                        winning_candidate = Some(candidate.clone());
                        if !manual_encoder_lock {
                            let current_saved = settings.read().await.preferred_encoder.clone();
                            if current_saved.as_deref() != Some(candidate.encoder.as_str()) {
                                let learned = candidate.encoder.clone();
                                let learned_nvenc = candidate.nvenc_gpu;
                                let learned_candidate = candidate.clone();
                                let settings_for_learn = settings.clone();
                                tokio::spawn(async move {
                                    let maybe_updated = {
                                        let mut w = settings_for_learn.write().await;
                                        if w.preferred_encoder.is_none() {
                                            w.preferred_encoder = Some(learned.clone());
                                            if learned == "h264_nvenc" {
                                                w.preferred_nvenc_gpu = learned_nvenc;
                                            }
                                            record_probe_success(&mut w, &learned_candidate);
                                            Some(w.clone())
                                        } else {
                                            record_probe_success(&mut w, &learned_candidate);
                                            Some(w.clone())
                                        }
                                    };
                                    if let Some(to_save) = maybe_updated {
                                        let _ = tokio::task::spawn_blocking(move || {
                                            save_host_settings_to_disk(&to_save);
                                        })
                                        .await;
                                        info!(encoder = %learned, nvenc_gpu = ?learned_nvenc, "auto-learned encoder persisted for this machine");
                                    }
                                });
                            } else {
                                let settings_for_cache = settings.clone();
                                let cand = candidate.clone();
                                tokio::spawn(async move {
                                    let to_save = {
                                        let mut w = settings_for_cache.write().await;
                                        record_probe_success(&mut w, &cand);
                                        w.clone()
                                    };
                                    let _ = tokio::task::spawn_blocking(move || {
                                        save_host_settings_to_disk(&to_save);
                                    })
                                    .await;
                                });
                            }
                        } else {
                            let settings_for_cache = settings.clone();
                            let cand = candidate.clone();
                            tokio::spawn(async move {
                                let to_save = {
                                    let mut w = settings_for_cache.write().await;
                                    record_probe_success(&mut w, &cand);
                                    w.clone()
                                };
                                let _ = tokio::task::spawn_blocking(move || {
                                    save_host_settings_to_disk(&to_save);
                                })
                                .await;
                            });
                        }
                        started = true;
                        break;
                    }
                    StreamExit::RestartRequested => {
                        started = true;
                        break;
                    }
                    StreamExit::Unavailable => {
                        let settings_for_fail = settings.clone();
                        let cand = candidate.clone();
                        tokio::spawn(async move {
                            let to_save = {
                                let mut w = settings_for_fail.write().await;
                                record_probe_failure(&mut w, &cand);
                                w.clone()
                            };
                            let _ = tokio::task::spawn_blocking(move || {
                                save_host_settings_to_disk(&to_save);
                            })
                            .await;
                        });
                        if preferred.as_deref() == Some(candidate.encoder.as_str()) {
                            warn!(
                                encoder = %candidate.encoder,
                                capture = %candidate.capture,
                                "preferred encoder unavailable, trying next candidate"
                            );
                        } else {
                            info!(
                                encoder = %candidate.encoder,
                                capture = %candidate.capture,
                                "not available, trying next"
                            );
                        }
                    }
                    StreamExit::SocketClosed => break,
                },
                Err(e) => {
                    error!(encoder = %candidate.encoder, "ffmpeg stream error: {e}");
                    if e.to_string()
                        .to_ascii_lowercase()
                        .contains("program not found")
                    {
                        let _ = ws_tx
                            .send(Message::text(
                                "{\"type\":\"error\",\"message\":\"FFmpeg not found. Install ffmpeg or set FLEXDISPLAY_FFMPEG\"}".to_string(),
                            ))
                            .await;
                        break;
                    }
                }
            }
        }

        if let Some(winner) = winning_candidate {
            info!(encoder = %winner.encoder, capture = %winner.capture, "active stream ended");
        }

        if !connection_alive.load(Ordering::Relaxed) {
            break;
        }

        if restart_requested.load(Ordering::Relaxed) {
            let _ = ws_tx.send(Message::text("RESET")).await;
            info!("hot settings apply: closing current h264 stream so client can reconnect cleanly");
            break;
        }

        if !started {
            let requested = query.encoder.as_deref().unwrap_or("auto");
            let attempted = candidates
                .iter()
                .map(|c| format!("{}/{}", c.encoder, c.capture))
                .collect::<Vec<_>>()
                .join(", ");
            let msg = if attempted.is_empty() {
                format!("No H.264 encoder available (requested={requested}, attempted=none)")
            } else {
                format!("No H.264 encoder available (requested={requested}, attempted={attempted})")
            };
            error!(%msg, "h264 setup failed");
            let _ = ws_tx
                .send(Message::text(format!(
                    "{{\"type\":\"error\",\"message\":\"{}\"}}",
                    msg.replace('"', "\\\"")
                )))
                .await;
            break;
        }
    }

    info!("h264 stream client disconnected");
    ws_close_task.abort();
    reload_watch_task.abort();
}
