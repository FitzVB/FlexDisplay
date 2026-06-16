mod capture;
mod encoder;
mod ffmpeg;
mod input;
mod profile;
mod settings;
mod stream;

use encoder::{detect_available_h264_encoders, detect_gpus, GpuInfo};
use ffmpeg::adb_exe;
use settings::{
    canonical_bitrate_kbps, canonical_encoder, canonical_preset, canonical_resolution,
    load_host_settings_from_disk, save_host_settings_to_disk, EnvConfig, HostSettings,
};
use std::sync::Arc;
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
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A,
        0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
        0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01,
        0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
        0x89, 0x00, 0x00, 0x00, 0x0B, 0x49, 0x44, 0x41,
        0x54, 0x78, 0x9C, 0x62, 0x00, 0x00, 0x00, 0x02,
        0x00, 0x01, 0xE5, 0x27, 0xDE, 0xFC, 0x00, 0x00,
        0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42,
        0x60, 0x82,
    ]
}

fn host_gui_html() -> &'static str {
    r#"<!doctype html>
<html lang="es">
<head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <title>FlexDisplay Host Control</title>
    <style>
        :root {
            --deep-blue:#2A4B8A;
            --vibrant-orange:#F78C00;
            --aqua-cyan:#2EC4B6;
            --panel:#F3F3F3;
            --text:#3A3A3A;
            --muted:#647085;
            --line:#E0E0E0;
            --ok:#11845B;
            --warn:#B02A37;
        }
        body { margin:0; font-family:Roboto, "Segoe UI", Tahoma, sans-serif; color:var(--text);
            background:
                radial-gradient(980px 420px at 8% 0%, rgba(42,75,138,.35) 0%, transparent 62%),
                radial-gradient(820px 360px at 80% 0%, rgba(247,140,0,.18) 0%, transparent 64%),
                radial-gradient(920px 420px at 95% 100%, rgba(46,196,182,.25) 0%, transparent 60%),
                linear-gradient(145deg, #f8fafc, #eef2f7);
            min-height:100vh; display:flex; align-items:center; justify-content:center; }
        .card { width:min(820px, 95vw); background:var(--panel); border-radius:22px; padding:24px;
            border:1px solid #fff;
            border-top:4px solid var(--aqua-cyan);
            box-shadow:0 26px 70px rgba(37,58,96,.16); }
        .brand { display:flex; align-items:center; gap:14px; margin-bottom:12px; }
        .brand img { width:56px; height:56px; border-radius:14px; box-shadow:0 8px 18px rgba(42,75,138,.25); }
        h1 { margin:0 0 3px; font-size:28px; color:#223252; }
        .sub { color:var(--muted); margin:0 0 18px; }
        .grid { display:grid; grid-template-columns:1fr 1fr; gap:14px; }
        .row { display:flex; flex-direction:column; gap:6px; }
        label { font-size:13px; color:#52627e; font-weight:600; }
        select, button { border-radius:12px; border:1px solid #c8d1df; padding:10px 11px; font-size:14px; }
        select {
            background:linear-gradient(180deg, #ffffff, #f7fbff);
            border-color:#bcd0ec;
            color:#25344f;
        }
        select:focus { outline:2px solid rgba(46,196,182,.35); border-color:var(--aqua-cyan); }
        .preset-meta {
            margin-top:10px;
            border-radius:12px;
            border:1px solid #dbe4f2;
            background:linear-gradient(145deg, #ffffff, #eef5ff);
            padding:10px 12px;
            font-size:13px;
            color:#314665;
        }
        button { color:#fff; border:none; font-weight:700; cursor:pointer; letter-spacing:.2px; }
        #save { background:linear-gradient(145deg, var(--vibrant-orange), #e57400); }
        #refresh { background:linear-gradient(145deg, var(--aqua-cyan), #1ea69a); }
        button:hover { filter:brightness(1.05); transform:translateY(-1px); }
        .status { margin-top:14px; min-height:20px; font-weight:600; opacity:0; transition:opacity .2s ease; }
        .status.visible { opacity:1; }
        .status.busy { color:var(--muted); }
        .status.ok { color:var(--ok); }
        .status.error { color:var(--warn); }
        .hint { margin-top:10px; font-size:13px; color:var(--muted); }
        .actions { margin-top:14px; display:flex; gap:8px; }
        @media (max-width: 720px) { .grid { grid-template-columns:1fr; } }
    </style>
</head>
<body>
    <div class="card">
        <div class="brand">
            <img src="/brand/logo.png" alt="FlexDisplay logo" />
            <div>
                <h1>FlexDisplay Host</h1>
                <div class="sub">Control encoder, GPU and quality profile with immediate apply.</div>
            </div>
        </div>
        <div class="grid">
            <div class="row">
                <label for="encoder">Preferred encoder</label>
                <select id="encoder"></select>
            </div>
            <div class="row">
                <label for="gpu">Preferred GPU adapter (optional)</label>
                <select id="gpu"></select>
            </div>
            <div class="row" style="grid-column:span 2;">
                <label for="preset">Quality profile</label>
                <select id="preset"></select>
                <div id="presetMeta" class="preset-meta"></div>
            </div>
        </div>
        <div class="actions">
            <button id="save">Save and apply</button>
            <button id="refresh">Reload detection</button>
        </div>
        <div id="status" class="status"></div>
        <div class="hint">Save locks the selected encoder (no automatic HW fallback). Reload detection refreshes vendor-filtered encoders and GPUs.</div>
    </div>
<script>
let statusTimer = null;
let presetDefs = [];

function setStatus(message, kind = 'ok', autoClearMs = 0){
    const el = document.getElementById('status');
    if (!el) return;
    if (statusTimer) {
        clearTimeout(statusTimer);
        statusTimer = null;
    }
    el.className = 'status visible ' + kind;
    el.textContent = message;
    if (autoClearMs > 0) {
        statusTimer = setTimeout(() => {
            el.textContent = '';
            el.className = 'status';
            statusTimer = null;
        }, autoClearMs);
    }
}

async function loadAll(){
    const [capRes, setRes] = await Promise.all([fetch('/api/capabilities'), fetch('/api/settings')]);
    const cap = await capRes.json();
    const set = await setRes.json();

    const enc = document.getElementById('encoder');
    enc.innerHTML = '';
    const auto = document.createElement('option'); auto.value=''; auto.textContent='auto'; enc.appendChild(auto);
    (cap.encoders || []).forEach(e => { const o=document.createElement('option'); o.value=e; o.textContent=e; enc.appendChild(o); });
    enc.value = set.preferred_encoder || '';

    const gpu = document.getElementById('gpu');
    gpu.innerHTML = '';
    const ga = document.createElement('option'); ga.value=''; ga.textContent='auto'; gpu.appendChild(ga);
    (cap.gpus || []).forEach(g => {
        const o=document.createElement('option');
        o.value=String(g.index);
        o.textContent=`#${g.index} - ${g.name}${g.driver_version ? ' (' + g.driver_version + ')' : ''}`;
        gpu.appendChild(o);
    });
    const gpuPref = set.preferred_nvenc_gpu ?? set.preferred_amf_device ?? '';
    gpu.value = gpuPref === '' ? '' : String(gpuPref);

    const preset = document.getElementById('preset');
    preset.innerHTML = '';
    presetDefs = [
        { value: '',           label: 'Automatic (adaptive to device)', detail: 'Matches tablet native resolution (USB up to 1920×1200@60, Wi-Fi up to 1280×720@30). Encoder caps apply automatically.' },
        { value: 'cpu_safe',    label: 'CPU safe - 960x544 / 30fps / 5 Mbps', detail: 'Optimized for libx264 software encoding on weak PCs.' },
        { value: 'ahorro',      label: 'Power saver - 960x544 / 30fps / 5 Mbps', detail: 'Low bandwidth and battery impact for remote control tasks.' },
        { value: 'equilibrado', label: 'Balanced - 1280x720 / 60fps / 10 Mbps', detail: 'Default smooth profile for most Android devices.' },
        { value: 'alta_720p',   label: 'High quality 720p - 1280x720 / 60fps / 15 Mbps', detail: 'Sharper image while preserving 60fps responsiveness.' },
        { value: 'fluido_900p', label: 'Smooth 900p - 1600x900 / 60fps / 20 Mbps', detail: 'Higher detail profile for stronger GPUs and USB links.' },
        { value: 'full_hd',     label: 'Full HD office - 1920x1080 / 60fps / 25 Mbps', detail: '1080p profile optimized for productivity and text clarity.' },
        { value: 'full_hd_max', label: 'Full HD detail - 1920x1080 / 60fps / 35 Mbps', detail: 'Maximum detail profile. Requires stable encoder and fast link.' },
    ];
    presetDefs.forEach(p => { const o=document.createElement('option'); o.value=p.value; o.textContent=p.label; preset.appendChild(o); });
    preset.value = set.preferred_preset || '';
    renderPresetMeta();
}

function renderPresetMeta(){
    const preset = document.getElementById('preset');
    const meta = document.getElementById('presetMeta');
    const selected = presetDefs.find(p => p.value === preset.value) || presetDefs[0];
    if (!meta || !selected) return;
    meta.textContent = selected.detail;
}

async function save(){
    const saveBtn = document.getElementById('save');
    saveBtn.disabled = true;
    setStatus('Saving configuration...', 'busy');
    const gpuVal = document.getElementById('gpu').value;
    const encVal = document.getElementById('encoder').value || null;
    const payload = {
        preferred_encoder: encVal,
        preferred_amf_device: (encVal === 'h264_amf' && gpuVal !== '') ? Number(gpuVal) : null,
        preferred_nvenc_gpu: (encVal === 'h264_nvenc' && gpuVal !== '') ? Number(gpuVal) : null,
        preferred_preset: document.getElementById('preset').value || null,
        preferred_width: null,
        preferred_height: null,
        preferred_bitrate_kbps: null,
    };
    const controller = new AbortController();
    const timeoutId = setTimeout(() => controller.abort(), 8000);
    try {
        const res = await fetch('/api/settings', {
            method:'POST',
            headers:{'Content-Type':'application/json'},
            body: JSON.stringify(payload),
            signal: controller.signal,
        });
        if (res.ok) {
            await loadAll();
            setStatus('Configuration saved and applied', 'ok', 2600);
        } else {
            setStatus('Could not save settings', 'error', 5000);
        }
    } catch (_e) {
        setStatus('Timed out while saving', 'error', 5000);
    } finally {
        clearTimeout(timeoutId);
        saveBtn.disabled = false;
    }
}

document.getElementById('save').addEventListener('click', save);
document.getElementById('refresh').addEventListener('click', loadAll);
document.getElementById('preset').addEventListener('change', renderPresetMeta);
loadAll();
</script>
</body>
</html>"#
}

fn maybe_open_gui(listen_ip: std::net::Ipv4Addr, port: u16) -> Option<std::sync::mpsc::Receiver<()>> {
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

    let h264_route = warp::path("h264")
        .and(warp::ws())
        .and(stream_query_filter)
        .and(host_settings_filter.clone())
        .and(settings_reload_rx_filter.clone())
        .and(env_filter.clone())
        .map(
            |ws: warp::ws::Ws,
             query: StreamQuery,
             settings,
             reload_rx,
             env| {
                ws.on_upgrade(move |socket| handle_h264_stream(socket, query, settings, reload_rx, env))
            },
        );

    let input_query_filter = warp::query::<InputQuery>()
        .or(warp::any().map(InputQuery::default))
        .unify();

    let input_route = warp::path("input")
        .and(warp::ws())
        .and(input_query_filter)
        .map(|ws: warp::ws::Ws, query: InputQuery| {
            ws.on_upgrade(move |socket| handle_input_socket(socket, query))
        });

    let ui_route = warp::path::end()
        .and(warp::get())
        .map(|| warp::reply::html(host_gui_html()));

    let logo_route = warp::path!("brand" / "logo.png").and(warp::get()).map(|| {
        warp::http::Response::builder()
            .header("content-type", "image/png")
            .header("cache-control", "public, max-age=3600")
            .body(logo_png_bytes().to_vec())
            .expect("build png response")
    });

    let capabilities_route = warp::path!("api" / "capabilities").and(warp::get()).map(|| {
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
        match std::process::Command::new(&adb)
            .args(["reverse", &format!("tcp:{listen_port}"), &format!("tcp:{listen_port}")])
            .output()
        {
            Ok(out) if out.status.success() => {
                info!(port = listen_port, "adb reverse OK — USB mode active");
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

async fn handle_input_socket(socket: warp::ws::WebSocket, query: InputQuery) {
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
                        let pong = format!(r#"{{"type":"pong","ts_ms":{ts_ms}}}"#);
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
