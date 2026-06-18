#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

mod adaptive;
mod capture;
mod encoder;
mod ffmpeg;
mod gui;
mod latency;
mod process_util;
mod input;
mod profile;
mod settings;
mod stream;

use encoder::{detect_available_h264_encoders, detect_gpus, GpuInfo};
use ffmpeg::adb_exe;
use process_util::hidden_command;
use settings::{
    canonical_bitrate_kbps, canonical_encoder, canonical_preset, canonical_resolution,
    load_host_settings_from_disk, save_host_settings_to_disk, EnvConfig, HostSettings,
};
use std::sync::Arc;
use adaptive::AdaptiveStreamState;
use latency::StreamLatencyState;
use stream::{handle_h264_stream, StreamQuery};
use tokio::sync::{watch, RwLock};
use tracing::info;
use warp::Filter;

#[cfg(windows)]
fn enable_dpi_awareness() {
    unsafe {
        winapi::um::winuser::SetProcessDPIAware();
    }
}

#[cfg(not(windows))]
fn enable_dpi_awareness() {}

#[derive(Clone, Debug, serde::Serialize)]
struct HostCapabilities {
    encoders: Vec<String>,
    gpus: Vec<GpuInfo>,
}

fn logo_png_bytes() -> &'static [u8] {
    &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F,
        0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0B, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x62, 0x00,
        0x00, 0x00, 0x02, 0x00, 0x01, 0xE5, 0x27, 0xDE, 0xFC, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45,
        0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ]
}

#[derive(Clone, Debug, serde::Serialize)]
struct HostStatus {
    listen_host: String,
    port: u16,
    mode: String,
    lan_ip: Option<String>,
    adb_device_connected: bool,
}

fn detect_lan_ipv4() -> Option<String> {
    #[cfg(windows)]
    {
        let ps = r#"(Get-NetIPAddress -AddressFamily IPv4 | Where-Object {
            $_.IPAddress -notlike '127.*' -and $_.IPAddress -notlike '169.254*' -and
            $_.InterfaceAlias -notmatch 'Loopback|vEthernet|Virtual|Hyper-V|VPN|Tailscale'
        } | Sort-Object InterfaceMetric | Select-Object -First 1 -ExpandProperty IPAddress)"#;
        let out = hidden_command("powershell")
            .args(["-NoProfile", "-Command", ps])
            .output()
            .ok()?;
        let ip = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if ip.is_empty() || ip.contains(' ') {
            return None;
        }
        Some(ip)
    }
    #[cfg(not(windows))]
    {
        None
    }
}

fn adb_device_connected() -> bool {
    let adb = adb_exe();
    let out = hidden_command(&adb).args(["devices"]).output();
    let Ok(out) = out else {
        return false;
    };
    if !out.status.success() {
        return false;
    }
    for line in String::from_utf8_lossy(&out.stdout).lines().skip(1) {
        let trimmed = line.trim();
        if trimmed.ends_with("\tdevice") || trimmed.ends_with(" device") {
            return true;
        }
    }
    false
}

fn maybe_open_gui(
    listen_ip: std::net::Ipv4Addr,
    port: u16,
) -> Option<std::sync::mpsc::Receiver<()>> {
    if std::env::var("FLEXDISPLAY_DISABLE_AUTO_GUI")
        .ok()
        .as_deref()
        == Some("1")
    {
        return None;
    }
    if !cfg!(windows) {
        return None;
    }

    let host = if listen_ip == std::net::Ipv4Addr::UNSPECIFIED {
        "127.0.0.1".to_string()
    } else {
        listen_ip.to_string()
    };
    let url = format!("http://{host}:{port}");

    let edge_paths = [
        "C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe",
        "C:\\Program Files\\Microsoft\\Edge\\Application\\msedge.exe",
        "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe",
        "C:\\Program Files (x86)\\Google\\Chrome\\Application\\chrome.exe",
    ];
    for edge in edge_paths {
        if std::path::Path::new(edge).exists() {
            let child = std::process::Command::new(edge)
                .arg(format!("--app={url}"))
                .spawn();
            if let Ok(mut child) = child {
                let (tx, rx) = std::sync::mpsc::channel::<()>();
                std::thread::spawn(move || {
                    let started = std::time::Instant::now();
                    let _ = child.wait();
                    if started.elapsed() >= std::time::Duration::from_secs(3) {
                        let _ = tx.send(());
                    }
                });
                return Some(rx);
            }
            return None;
        }
    }

    let _ = std::process::Command::new("cmd")
        .args(["/C", "start", "", &url])
        .spawn();
    None
}

#[derive(Clone, Debug, serde::Deserialize, Default)]
struct InputQuery {
    mode: Option<String>,
    display: Option<u32>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    enable_dpi_awareness();

    tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_target(false)
        .init();

    let env_config = EnvConfig::load();
    let host_settings = Arc::new(RwLock::new(load_host_settings_from_disk()));
    let host_settings_filter = warp::any().map({
        let settings = host_settings.clone();
        move || settings.clone()
    });

    let (settings_reload_tx, settings_reload_rx) = watch::channel(0u64);
    let settings_reload_tx_filter = warp::any().map({
        let tx = settings_reload_tx.clone();
        move || tx.clone()
    });
    let settings_reload_rx_filter = warp::any().map({
        let rx = settings_reload_rx.clone();
        move || rx.clone()
    });

    let env_filter = warp::any().map({
        let env = env_config.clone();
        move || env.clone()
    });

    let health = warp::path("health").map(|| "ok");

    let stream_query_filter = warp::query::<StreamQuery>()
        .or(warp::any().map(StreamQuery::default))
        .unify();

    let stream_latency = Arc::new(StreamLatencyState::default());
    let adaptive_stream = Arc::new(AdaptiveStreamState::default());

    let stream_latency_h264 = stream_latency.clone();
    let adaptive_h264 = adaptive_stream.clone();
    let h264_route = warp::path("h264")
        .and(warp::ws())
        .and(stream_query_filter)
        .and(host_settings_filter.clone())
        .and(settings_reload_rx_filter.clone())
        .and(env_filter.clone())
        .map(
            move |ws: warp::ws::Ws, query: StreamQuery, settings, reload_rx, env| {
                let latency = stream_latency_h264.clone();
                let adaptive = adaptive_h264.clone();
                ws.on_upgrade(move |socket| {
                    handle_h264_stream(socket, query, settings, reload_rx, env, latency, adaptive)
                })
            },
        );

    let stream_latency_input = stream_latency.clone();
    let adaptive_input = adaptive_stream.clone();
    let input_query_filter = warp::query::<InputQuery>()
        .or(warp::any().map(InputQuery::default))
        .unify();

    let input_route = warp::path("input")
        .and(warp::ws())
        .and(input_query_filter)
        .map(move |ws: warp::ws::Ws, query: InputQuery| {
            let latency = stream_latency_input.clone();
            let adaptive = adaptive_input.clone();
            ws.on_upgrade(move |socket| handle_input_socket(socket, query, latency, adaptive))
        });

    let ui_route = warp::path::end()
        .and(warp::get())
        .map(|| warp::reply::html(gui::host_gui_html()));

    let status_env = env_config.clone();
    let status_route = warp::path!("api" / "status").and(warp::get()).map(move || {
        let listen = status_env.listen_host.clone();
        let mode = if listen == "127.0.0.1" || listen == "localhost" {
            "usb"
        } else {
            "wifi"
        };
        warp::reply::json(&HostStatus {
            listen_host: listen,
            port: status_env.port,
            mode: mode.to_string(),
            lan_ip: detect_lan_ipv4(),
            adb_device_connected: adb_device_connected(),
        })
    });

    let logo_route = warp::path!("brand" / "logo.png").and(warp::get()).map(|| {
        warp::http::Response::builder()
            .header("content-type", "image/png")
            .header("cache-control", "public, max-age=3600")
            .body(logo_png_bytes().to_vec())
            .expect("build png response")
    });

    let capabilities_route = warp::path!("api" / "capabilities")
        .and(warp::get())
        .map(|| {
            let gpus = detect_gpus();
            let caps = HostCapabilities {
                encoders: detect_available_h264_encoders(&gpus, false),
                gpus,
            };
            warp::reply::json(&caps)
        });

    let displays_route = warp::path("displays").and(warp::get()).map(|| {
        let displays = input::list_displays().unwrap_or_else(|_| Vec::new());
        warp::reply::json(&displays)
    });

    let settings_get_route = warp::path!("api" / "settings")
        .and(warp::get())
        .and(host_settings_filter.clone())
        .and_then(|settings: Arc<RwLock<HostSettings>>| async move {
            let snapshot = settings.read().await.clone();
            Ok::<_, warp::Rejection>(warp::reply::json(&snapshot))
        });

    let settings_post_route = warp::path!("api" / "settings")
        .and(warp::post())
        .and(warp::body::json())
        .and(host_settings_filter.clone())
        .and(settings_reload_tx_filter)
        .and_then(
            |incoming: HostSettings,
             settings: Arc<RwLock<HostSettings>>,
             reload_tx: watch::Sender<u64>| async move {
                let (preferred_width, preferred_height) =
                    canonical_resolution(incoming.preferred_width, incoming.preferred_height);
                let preferred_preset = canonical_preset(incoming.preferred_preset.clone());
                let (eff_width, eff_height, eff_bitrate) = if preferred_preset.is_some() {
                    (None, None, None)
                } else {
                    (
                        preferred_width,
                        preferred_height,
                        canonical_bitrate_kbps(incoming.preferred_bitrate_kbps),
                    )
                };
                let existing = settings.read().await.clone();
                let normalized = HostSettings {
                    preferred_encoder: canonical_encoder(incoming.preferred_encoder),
                    preferred_amf_device: incoming.preferred_amf_device,
                    preferred_nvenc_gpu: incoming.preferred_nvenc_gpu,
                    preferred_preset,
                    preferred_width: eff_width,
                    preferred_height: eff_height,
                    preferred_bitrate_kbps: eff_bitrate,
                    encoder_probe_cache: incoming
                        .encoder_probe_cache
                        .or(existing.encoder_probe_cache),
                    failed_probe_keys: existing.failed_probe_keys,
                };
                {
                    let mut write = settings.write().await;
                    *write = normalized.clone();
                }
                let normalized_for_background = normalized.clone();
                let next_reload_version = *reload_tx.borrow() + 1;
                tokio::spawn(async move {
                    let to_save = normalized_for_background.clone();
                    let _ = tokio::task::spawn_blocking(move || {
                        save_host_settings_to_disk(&to_save);
                    })
                    .await;
                    let _ = reload_tx.send(next_reload_version);
                    info!(
                        encoder = ?normalized_for_background.preferred_encoder,
                        nvenc_gpu = ?normalized_for_background.preferred_nvenc_gpu,
                        amf_device = ?normalized_for_background.preferred_amf_device,
                        preset = ?normalized_for_background.preferred_preset,
                        "host settings updated via GUI"
                    );
                });
                Ok::<_, warp::Rejection>(warp::reply::json(&normalized))
            },
        );

    let listen_ip: std::net::Ipv4Addr = env_config
        .listen_host
        .parse()
        .unwrap_or(std::net::Ipv4Addr::UNSPECIFIED);
    let listen_port = env_config.port;

    info!(%listen_ip, port = listen_port, "host server listening");

    {
        let adb = adb_exe();
        match hidden_command(&adb)
            .args([
                "reverse",
                &format!("tcp:{listen_port}"),
                &format!("tcp:{listen_port}"),
            ])
            .output()
        {
            Ok(out) if out.status.success() => {
                info!(port = listen_port, "adb reverse OK â€” USB mode active");
            }
            Ok(out) => {
                let msg = String::from_utf8_lossy(&out.stderr);
                info!(
                    "adb reverse skipped (no Android device connected): {}",
                    msg.trim()
                );
            }
            Err(e) => {
                info!("adb not found, USB mode unavailable: {e}");
            }
        }
    }

    let gui_closed_rx = maybe_open_gui(listen_ip, listen_port);
    let exit_on_gui_close = std::env::var("FLEXDISPLAY_EXIT_ON_GUI_CLOSE")
        .ok()
        .as_deref()
        == Some("1");

    let routes = ui_route
        .or(status_route)
        .or(logo_route)
        .or(health)
        .or(capabilities_route)
        .or(displays_route)
        .or(settings_get_route)
        .or(settings_post_route)
        .or(h264_route)
        .or(input_route);

    let shutdown_signal = async move {
        if exit_on_gui_close {
            if let Some(rx) = gui_closed_rx {
                let _ = tokio::task::spawn_blocking(move || rx.recv()).await;
                info!("GUI window closed, stopping host process");
            } else {
                std::future::pending::<()>().await;
            }
        } else {
            std::future::pending::<()>().await;
        }
    };

    warp::serve(routes)
        .bind_with_graceful_shutdown((listen_ip.octets(), listen_port), shutdown_signal)
        .1
        .await;

    Ok(())
}

async fn handle_input_socket(
    socket: warp::ws::WebSocket,
    query: InputQuery,
    stream_latency: Arc<StreamLatencyState>,
    adaptive: Arc<AdaptiveStreamState>,
) {
    use futures::{SinkExt, StreamExt};
    use tracing::error;

    let (mut ws_tx, mut ws_rx) = socket.split();
    let mode = query.mode.unwrap_or_else(|| "mirror".to_string());
    let default_display_idx = input::default_display_for_mode(&mode).unwrap_or(0);
    let display_idx = query.display.unwrap_or(default_display_idx).clamp(0, 9);
    let display_target = match input::resolve_display_target(display_idx) {
        Ok(target) => Some(target),
        Err(e) => {
            error!(display_idx, "failed to resolve input display target: {e}");
            None
        }
    };

    info!(mode, display_idx, "input channel connected");

    while let Some(result) = ws_rx.next().await {
        match result {
            Ok(msg) if msg.is_text() => {
                let text = match msg.to_str() {
                    Ok(v) => v,
                    Err(_) => continue,
                };

                if let Ok(obj) = serde_json::from_str::<serde_json::Value>(text) {
                    if obj.get("type").and_then(|v| v.as_str()) == Some("ping") {
                        let ts_ms = obj.get("ts_ms").and_then(|v| v.as_i64()).unwrap_or(0);
                        let glass_ms = obj
                            .get("glass_ms")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0) as u32;
                        let dec_ms = obj
                            .get("dec_ms")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0) as u32;
                        adaptive.update_client_stats(glass_ms, dec_ms);

                        let host_us = latency::host_now_us();
                        let stream_send_us = stream_latency.last_send_us();
                        let tuning = adaptive.evaluate_tuning().as_str();
                        let pong = format!(
                            r#"{{"type":"pong","ts_ms":{ts_ms},"host_us":{host_us},"stream_send_us":{stream_send_us},"tuning":"{tuning}"}}"#
                        );
                        let _ = ws_tx.send(warp::ws::Message::text(pong)).await;
                        continue;
                    }
                }

                match serde_json::from_str::<input::PointerInputEvent>(text) {
                    Ok(event) => {
                        let Some(target) = display_target.as_ref() else {
                            error!("input dropped: no display target available");
                            continue;
                        };
                        if let Err(e) = input::inject_pointer_event(&event, target) {
                            error!("input inject error: {e}");
                        }
                    }
                    Err(e) => {
                        error!("invalid input payload: {e}");
                    }
                }
            }
            Ok(msg) if msg.is_close() => break,
            Ok(_) => {}
            Err(e) => {
                error!("input websocket error: {e}");
                break;
            }
        }
    }

    info!("input channel disconnected");
}
