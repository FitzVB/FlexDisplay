mod input;

use std::sync::Arc;

use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::sync::{watch, RwLock};
use tracing::{error, info};
use warp::ws::Message;
use warp::Filter;

#[cfg(windows)]
fn enable_dpi_awareness() {
    unsafe {
        // Ensure monitor geometry/capture runs in physical pixels, not DPI-virtualized coords.
        // This prevents mirror-mode crops where the bottom taskbar area is missing.
        winapi::um::winuser::SetProcessDPIAware();
    }
}

#[cfg(not(windows))]
fn enable_dpi_awareness() {}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
struct HostSettings {
    preferred_encoder: Option<String>,
    preferred_amf_device: Option<u32>,
    /// Named quality preset — overrides width/height/fps/bitrate when set.
    preferred_preset: Option<String>,
    // Legacy manual overrides (ignored when preferred_preset is set).
    preferred_width: Option<u32>,
    preferred_height: Option<u32>,
    preferred_bitrate_kbps: Option<u32>,
}

#[derive(Clone, Debug, Serialize)]
struct GpuInfo {
    index: usize,
    name: String,
    driver_version: String,
}

#[derive(Clone, Debug, Serialize)]
struct HostCapabilities {
    encoders: Vec<String>,
    gpus: Vec<GpuInfo>,
}

fn canonical_encoder(value: Option<String>) -> Option<String> {
    let raw = value?.trim().to_ascii_lowercase();
    match raw.as_str() {
        "h264_nvenc" | "h264_qsv" | "h264_amf" | "libx264" => Some(raw),
        _ => None,
    }
}

fn canonical_resolution(width: Option<u32>, height: Option<u32>) -> (Option<u32>, Option<u32>) {
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

fn canonical_bitrate_kbps(value: Option<u32>) -> Option<u32> {
    let allowed = [
        3000u32, 5000u32, 10000u32, 15000u32, 25000u32, 30000u32, 50000u32,
    ];
    let v = value?;
    if allowed.contains(&v) {
        Some(v)
    } else {
        None
    }
}

fn canonical_preset(value: Option<String>) -> Option<String> {
    let v = value?.trim().to_ascii_lowercase();
    match v.as_str() {
        "ahorro" | "equilibrado" | "alta_720p" | "fluido_900p" | "full_hd" | "full_hd_max" => {
            Some(v)
        }
        _ => None,
    }
}

/// Returns (width, height, fps, bitrate_kbps) for a named quality preset.
/// Values are already macroblock-aligned (multiples of 16).
fn resolve_preset(name: &str) -> Option<(u32, u32, u32, u32)> {
    match name {
        "ahorro" => Some((960, 544, 30, 5_000)),
        "equilibrado" => Some((1280, 720, 60, 10_000)),
        "alta_720p" => Some((1280, 720, 60, 15_000)),
        "fluido_900p" => Some((1600, 900, 60, 20_000)),
        "full_hd" => Some((1920, 1080, 60, 25_000)),
        "full_hd_max" => Some((1920, 1080, 60, 35_000)),
        _ => None,
    }
}

fn settings_file_path() -> std::path::PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            return dir.join("host-settings.json");
        }
    }
    std::path::PathBuf::from("host-settings.json")
}

fn load_host_settings_from_disk() -> HostSettings {
    let path = settings_file_path();
    let Ok(text) = std::fs::read_to_string(path) else {
        return HostSettings::default();
    };
    serde_json::from_str::<HostSettings>(&text).unwrap_or_default()
}

fn save_host_settings_to_disk(settings: &HostSettings) {
    let path = settings_file_path();
    if let Ok(text) = serde_json::to_string_pretty(settings) {
        let _ = std::fs::write(path, text);
    }
}

fn detect_available_h264_encoders() -> Vec<String> {
    let output = std::process::Command::new(ffmpeg_exe())
        .arg("-hide_banner")
        .arg("-encoders")
        .output();

    let Ok(out) = output else {
        return vec!["libx264".to_string()];
    };

    let text = String::from_utf8_lossy(&out.stdout).to_ascii_lowercase();
    let mut found = Vec::new();
    for enc in ["h264_nvenc", "h264_qsv", "h264_amf", "libx264"] {
        if text.contains(enc) {
            found.push(enc.to_string());
        }
    }
    if found.is_empty() {
        found.push("libx264".to_string());
    }
    found
}

fn detect_gpus() -> Vec<GpuInfo> {
    #[derive(Debug, Deserialize)]
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

fn logo_png_bytes() -> &'static [u8] {
    include_bytes!("../../FlexDisplay_Logo.png")
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
        <div class="hint">Save and apply persists the selected profile and restarts active stream safely. Preferred GPU adapter is only used when a hardware encoder path needs explicit device selection. Reload detection refreshes available encoders/GPUs.</div>
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
    gpu.value = (set.preferred_amf_device ?? '') === '' ? '' : String(set.preferred_amf_device);

    const preset = document.getElementById('preset');
    preset.innerHTML = '';
    presetDefs = [
        { value: '',           label: 'Auto (from client)', detail: 'Host follows device stream request. Best for quick testing.' },
        { value: 'ahorro',     label: 'Power saver - 960x544 / 30fps / 5 Mbps', detail: 'Low bandwidth and battery impact for remote control tasks.' },
        { value: 'equilibrado',label: 'Balanced - 1280x720 / 60fps / 10 Mbps', detail: 'Default smooth profile for most Android devices.' },
        { value: 'alta_720p',  label: 'High quality 720p - 1280x720 / 60fps / 15 Mbps', detail: 'Sharper image while preserving 60fps responsiveness.' },
        { value: 'fluido_900p',label: 'Smooth 900p - 1600x900 / 60fps / 20 Mbps', detail: 'Higher detail profile for stronger GPUs and USB links.' },
        { value: 'full_hd',    label: 'Full HD office - 1920x1080 / 60fps / 25 Mbps', detail: '1080p profile optimized for productivity and text clarity.' },
        { value: 'full_hd_max',label: 'Full HD detail - 1920x1080 / 60fps / 35 Mbps', detail: 'Maximum detail profile. Requires stable encoder and fast link.' },
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
    const payload = {
        preferred_encoder: document.getElementById('encoder').value || null,
        preferred_amf_device: document.getElementById('gpu').value === '' ? null : Number(document.getElementById('gpu').value),
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

fn maybe_open_gui(listen_ip: std::net::Ipv4Addr) -> Option<std::sync::mpsc::Receiver<()>> {
    if std::env::var("TABLET_MONITOR_DISABLE_AUTO_GUI")
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
    let url = format!("http://{host}:9001");

    // Prefer Edge app mode for an app-like desktop window (no tabs/address bar).
    // Fallback to default browser if Edge is unavailable.
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
                    // Some systems briefly spawn/tear-down the Edge app process while
                    // transferring to an existing browser instance. Ignore those transient
                    // exits so host does not stop immediately on startup.
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

#[derive(Clone, Debug, Deserialize, Default)]
struct StreamQuery {
    w: Option<u32>,
    h: Option<u32>,
    fps: Option<u32>,
    bitrate_kbps: Option<u32>,
    encoder: Option<String>,
    mode: Option<String>,
    display: Option<u32>,
}

#[derive(Clone, Debug, Deserialize, Default)]
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

    let host_settings = Arc::new(RwLock::new(load_host_settings_from_disk()));
    let host_settings_filter = warp::any().map({
        let settings = host_settings.clone();
        move || settings.clone()
    });

    // Incremented whenever GUI settings are saved so active streams can restart in-place.
    let (settings_reload_tx, settings_reload_rx) = watch::channel(0u64);
    let settings_reload_tx_filter = warp::any().map({
        let tx = settings_reload_tx.clone();
        move || tx.clone()
    });
    let settings_reload_rx_filter = warp::any().map({
        let rx = settings_reload_rx.clone();
        move || rx.clone()
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
        .map(
            |ws: warp::ws::Ws, query: StreamQuery, settings, reload_rx| {
                ws.on_upgrade(move |socket| handle_h264_stream(socket, query, settings, reload_rx))
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

    let logo_route = warp::path!("brand" / "logo.png")
        .and(warp::get())
        .map(|| {
            warp::http::Response::builder()
                .header("content-type", "image/png")
                .header("cache-control", "public, max-age=3600")
                .body(logo_png_bytes().to_vec())
                .expect("build png response")
        });

    let capabilities_route = warp::path!("api" / "capabilities")
        .and(warp::get())
        .map(|| {
            let caps = HostCapabilities {
                encoders: detect_available_h264_encoders(),
                gpus: detect_gpus(),
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
                // When a named preset is chosen, clear manual overrides so they don't
                // shadow the preset on the next load.
                let (eff_width, eff_height, eff_bitrate) = if preferred_preset.is_some() {
                    (None, None, None)
                } else {
                    (
                        preferred_width,
                        preferred_height,
                        canonical_bitrate_kbps(incoming.preferred_bitrate_kbps),
                    )
                };
                let normalized = HostSettings {
                    preferred_encoder: canonical_encoder(incoming.preferred_encoder),
                    preferred_amf_device: incoming.preferred_amf_device,
                    preferred_preset,
                    preferred_width: eff_width,
                    preferred_height: eff_height,
                    preferred_bitrate_kbps: eff_bitrate,
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
                        amf_device = ?normalized_for_background.preferred_amf_device,
                        preset = ?normalized_for_background.preferred_preset,
                        width = ?normalized_for_background.preferred_width,
                        height = ?normalized_for_background.preferred_height,
                        bitrate_kbps = ?normalized_for_background.preferred_bitrate_kbps,
                        "host settings updated via GUI"
                    );
                });
                Ok::<_, warp::Rejection>(warp::reply::json(&normalized))
            },
        );

    // Default to 0.0.0.0 so Wi-Fi connections work without setting the env var.
    // USB-only mode: set TABLET_MONITOR_LISTEN=127.0.0.1 before starting.
    let listen_host =
        std::env::var("TABLET_MONITOR_LISTEN").unwrap_or_else(|_| "0.0.0.0".to_string());
    let listen_ip: std::net::Ipv4Addr =
        listen_host.parse().unwrap_or(std::net::Ipv4Addr::LOCALHOST);

    info!(%listen_ip, "host server listening on {}:9001", listen_ip);

    // Auto-configure USB tunnel so the Android app works plug-and-play over USB
    // without the user needing to run adb reverse manually.
    {
        let adb = adb_exe();
        match std::process::Command::new(&adb)
            .args(["reverse", "tcp:9001", "tcp:9001"])
            .output()
        {
            Ok(out) if out.status.success() => {
                info!("adb reverse tcp:9001 tcp:9001 OK — USB mode active");
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

    let gui_closed_rx = maybe_open_gui(listen_ip);
    let exit_on_gui_close = std::env::var("TABLET_MONITOR_EXIT_ON_GUI_CLOSE")
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
        .bind_with_graceful_shutdown((listen_ip.octets(), 9001), shutdown_signal)
        .1
        .await;

    Ok(())
}

/// Capture backend passed to `stream_with_ffmpeg`.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Capture {
    /// DXGI Desktop Duplication via lavfi/ddagrab — GPU-accelerated, ~1 ms capture latency,
    /// very low CPU cost.  Requires Windows 8 + WDDM driver (all modern machines).
    Ddagrab,
    /// Legacy GDI screen grab — universally compatible, higher CPU utilisation.
    Gdigrab,
}

/// Per-encoder low-latency arguments.
/// Each HW encoder has its own flag vocabulary; wrong flags cause silent startup failures.
fn encoder_extra_args(encoder: &str) -> Vec<String> {
    match encoder {
        "h264_nvenc" => vec![
            // p1 = fastest NVENC preset (replaces deprecated "ll" in FFmpeg 6+)
            "-preset".into(),
            "p1".into(),
            "-tune".into(),
            "ll".into(),
            // cbr: constant bitrate — lowest encode latency; encoder outputs frames immediately
            // without lookahead buffering (VBR mode adds 50-100 ms of lookahead delay).
            "-rc".into(),
            "cbr".into(),
            // no B-frames: eliminates 2-frame encode delay
            "-bf".into(),
            "0".into(),
            // zerolatency: disables NVENC picture-reorder buffer (hardware lookahead)
            "-zerolatency".into(),
            "1".into(),
            // Level 5.1: 1890×1080@60fps = ~485k macroblocks/sec which exceeds Level 4.1
            // (245k limit). Without this, NVENC may write Level 4.1 in the SPS and Qualcomm
            // decoders enforce that limit, throttling output to ~50fps for complex content.
            "-level".into(),
            "5.1".into(),
        ],
        "h264_qsv" => vec![
            "-preset".into(),
            "veryfast".into(),
            // async_depth 1: reduce QSV internal pipeline length (fewer frames queued)
            "-async_depth".into(),
            "1".into(),
            "-bf".into(),
            "0".into(),
        ],
        "h264_amf" => vec![
            // Keep low-latency transport while avoiding motion-analysis spikes that can
            // show up as microstutter during sustained video playback.
            "-usage".into(), "lowlatency".into(),
            "-quality".into(), "balanced".into(),
            "-latency".into(), "true".into(),
            "-rc".into(), "cbr".into(),
            "-async_depth".into(), "1".into(),
            "-profile".into(), "high".into(),
            "-coder".into(), "cabac".into(),
            "-bf".into(), "0".into(),
            "-max_b_frames".into(), "0".into(),
            "-vbaq".into(), "true".into(),
        ],
        "libx264" => vec![
            "-preset".into(),
            "ultrafast".into(),
            // zerolatency: disables lookahead + sets bf=0 + rc_lookahead=0
            "-tune".into(),
            "zerolatency".into(),
            // Constrained baseline improves compatibility with mobile AVC decoders.
            "-profile:v".into(),
            "baseline".into(),
            // 1600x900@60 can exceed Level 4.1 macroblock rate limits.
            // Declare 5.1 to avoid decoder-side throttling behavior.
            "-level".into(),
            "5.1".into(),
            // Disable CABAC/B-frames/high refs to avoid pink/green macroblock artifacts.
            "-x264-params".into(),
            "bframes=0:scenecut=0:ref=1:cabac=0:rc-lookahead=0:sync-lookahead=0:repeat-headers=1:aud=1:sliced-threads=0".into(),
        ],
        _ => vec!["-bf".into(), "0".into()],
    }
}

fn amf_device_from_pre_args(pre_args: &[String]) -> Option<u32> {
    if pre_args.len() < 2 || pre_args[0] != "-init_hw_device" {
        return None;
    }
    let spec = pre_args[1].strip_prefix("d3d11va=amf_dx:")?;
    spec.parse::<u32>().ok()
}

async fn handle_h264_stream(
    socket: warp::ws::WebSocket,
    query: StreamQuery,
    settings: Arc<RwLock<HostSettings>>,
    mut reload_rx: watch::Receiver<u64>,
) {
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };

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

    while connection_alive.load(Ordering::Relaxed) {
        restart_requested.store(false, Ordering::Relaxed);

        let settings_snapshot = settings.read().await.clone();

        let stream_mode = query
            .mode
            .as_deref()
            .map(|m| m.to_ascii_lowercase())
            .unwrap_or_else(|| "mirror".to_string());

        // Display index: for "mirror" mode prefer explicit selection.
        // If not set by client, keep 0 as safe fallback.
        let default_display_idx = if stream_mode.eq_ignore_ascii_case("extended") {
            input::default_display_for_mode(&stream_mode).unwrap_or(0)
        } else {
            0
        };
        let display_idx = query.display.unwrap_or(default_display_idx).clamp(0, 9);

        // Resolve active (width, height, fps, bitrate).
        // Priority: user-set preset > manual w/h override > host display size (mirror) > client query > defaults.
        // In mirror mode the host is source-of-truth for geometry so Android only renders.
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

        let (mirror_host_w, mirror_host_h) = if stream_mode.eq_ignore_ascii_case("mirror") {
            if let Ok(target) = input::resolve_display_target(display_idx) {
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

        // Width/height resolution priority: preset > manual > host mirror display > client query > defaults.
        let rw = preset_w
            .or(settings_snapshot.preferred_width)
            .or(mirror_host_w)
            .or(query.w)
            .unwrap_or(960);
        let rh = preset_h
            .or(settings_snapshot.preferred_height)
            .or(mirror_host_h)
            .or(query.h)
            .unwrap_or(540);

        let out_w = rw.clamp(320, 3840) & !15;
        let out_h = rh.clamp(240, 2160) & !15;
        // FPS/bitrate priority: preset > query > defaults.
        let fps = preset_fps.or(query.fps).unwrap_or(60).clamp(10, 60);
        let bitrate = preset_bitrate
            .or(settings_snapshot.preferred_bitrate_kbps)
            .or(query.bitrate_kbps)
            .unwrap_or(4000)
            .clamp(1000, 50000);
        let preferred = settings_snapshot
            .preferred_encoder
            .clone()
            .or_else(|| query.encoder.clone())
            .or_else(|| std::env::var("TABLET_MONITOR_HW_ENCODER").ok());
        let manual_encoder_selected = preferred.is_some();

        let selected_amf_device = settings_snapshot.preferred_amf_device;
        // Build AMF device fallbacks from detected GPUs instead of hardcoding a single index.
        // Priority: selected GPU first (if any), then remaining GPUs, then implicit AMF default.
        let gpu_count = detect_gpus().len().min(10) as u32;
        let mut amf_device_order: Vec<u32> = Vec::new();
        if let Some(idx) = selected_amf_device {
            amf_device_order.push(idx);
        }
        for idx in 0..gpu_count {
            if Some(idx) != selected_amf_device {
                amf_device_order.push(idx);
            }
        }
        // Prefer the last known-good AMF GPU first, then try the remaining GPUs,
        // and only then fall back to ffmpeg's implicit default device selection.
        let mut amf_pre_args_candidates: Vec<Vec<String>> = amf_device_order
            .into_iter()
            .map(|idx| vec!["-init_hw_device".into(), format!("d3d11va=amf_dx:{idx}")])
            .collect();
        amf_pre_args_candidates.push(vec![]);

        let hw: &[&str] = &["h264_nvenc", "h264_qsv", "h264_amf"];
        let mut candidates: Vec<(String, Capture, Vec<String>)> = Vec::new();
        // Capture backend priority: mirror mode prefers GDIGRAB (coordinate-based capture)
        // over DDAGRAB (index-based capture) to avoid cropping issues on non-standard resolutions.
        // Extended mode can use DDAGRAB (DXGI) since it targets a virtual display.
        let prefer_capture_env = std::env::var("TABLET_MONITOR_CAPTURE")
            .ok()
            .map(|v| v.trim().to_ascii_lowercase());
        let capture_order: [Capture; 2] = if stream_mode.eq_ignore_ascii_case("mirror") {
            // Mirror: try GDI (coordinate-based) first for exact monitor capture
            [Capture::Gdigrab, Capture::Ddagrab]
        } else if prefer_capture_env.as_deref() == Some("gdigrab") {
            // Extended with explicit GDI preference
            [Capture::Gdigrab, Capture::Ddagrab]
        } else if prefer_capture_env.as_deref() == Some("ddagrab") {
            // Extended with explicit DXGI preference
            [Capture::Ddagrab, Capture::Gdigrab]
        } else {
            // Extended default: DXGI (GPU-accelerated) first for performance
            [Capture::Ddagrab, Capture::Gdigrab]
        };

        if let Some(pref) = preferred.as_deref() {
            if hw.contains(&pref) {
                if pref == "h264_amf" {
                    for cap in capture_order {
                        for pre in &amf_pre_args_candidates {
                            candidates.push((pref.into(), cap, pre.clone()));
                        }
                    }
                } else {
                    for cap in capture_order {
                        candidates.push((pref.into(), cap, vec![]));
                    }
                }
            } else if pref == "libx264" {
                for cap in capture_order {
                    candidates.push((pref.into(), cap, vec![]));
                }
            }
        }

        if manual_encoder_selected {
            info!(preferred = ?preferred, "manual encoder requested: priority mode with fallback");
        }

        {
            for &enc in hw {
                if preferred.as_deref() != Some(enc) {
                    if enc == "h264_amf" {
                        for cap in capture_order {
                            for pre in &amf_pre_args_candidates {
                                candidates.push((enc.into(), cap, pre.clone()));
                            }
                        }
                    } else {
                        for cap in capture_order {
                            candidates.push((enc.into(), cap, vec![]));
                        }
                    }
                }
            }

            if preferred.as_deref() != Some("libx264") {
                for cap in capture_order {
                    candidates.push(("libx264".into(), cap, vec![]));
                }
            }
        }

        info!(out_w, out_h, fps, bitrate, "h264 stream active profile");

        let mut started = false;
        for (encoder, capture, pre_args) in &candidates {
            if !connection_alive.load(Ordering::Relaxed) {
                break;
            }
            if restart_requested.load(Ordering::Relaxed) {
                break;
            }

            let profile_msg = format!(
                "CFG:encoder={};capture={:?};w={};h={};fps={};bitrate_kbps={}",
                encoder, capture, out_w, out_h, fps, bitrate
            );
            let _ = ws_tx.send(Message::text(profile_msg)).await;

            let amf_device = if encoder == "h264_amf" {
                amf_device_from_pre_args(pre_args)
            } else {
                None
            };
            info!(encoder = %encoder, capture = ?capture, amf_device = ?amf_device, "trying encoder/capture combination");

            match stream_with_ffmpeg(
                &mut ws_tx,
                &connection_alive,
                &restart_requested,
                FfmpegConfig {
                    out_w,
                    out_h,
                    fps,
                    bitrate_kbps: bitrate,
                    mode: stream_mode.clone(),
                    display_idx,
                    encoder: encoder.to_string(),
                    capture: *capture,
                    pre_input_args: pre_args.clone(),
                },
                if encoder == "h264_amf" {
                    Some(settings.clone())
                } else {
                    None
                },
            )
            .await
            {
                Ok(exit) => {
                    match exit {
                        StreamExit::Streamed => {
                            started = true;
                            break;
                        }
                        StreamExit::RestartRequested => {
                            started = true;
                            break;
                        }
                        StreamExit::Unavailable => {
                            // If this was the user-preferred encoder, escalate to WARN so it's
                            // visible in the console and the user knows why we fell back.
                            if preferred.as_deref() == Some(encoder.as_str()) {
                                tracing::warn!(encoder = %encoder, capture = ?capture, amf_device = ?amf_device, "preferred encoder unavailable, trying next candidate");
                            } else {
                                info!(encoder = %encoder, capture = ?capture, amf_device = ?amf_device, "not available, trying next");
                            }
                        }
                        StreamExit::SocketClosed => {
                            break;
                        }
                    }
                }
                Err(e) => {
                    error!(encoder = %encoder, "ffmpeg stream error: {e}");
                    if e.to_string()
                        .to_ascii_lowercase()
                        .contains("program not found")
                    {
                        let _ = ws_tx
                            .send(Message::text(
                                "{\"type\":\"error\",\"message\":\"FFmpeg not found. Install ffmpeg or set TABLET_MONITOR_FFMPEG\"}".to_string(),
                            ))
                            .await;
                        break;
                    }
                }
            }
        }

        if !connection_alive.load(Ordering::Relaxed) {
            break;
        }

        if restart_requested.load(Ordering::Relaxed) {
            let _ = ws_tx.send(Message::text("RESET")).await;
            info!(
                "hot settings apply: closing current h264 stream so client can reconnect cleanly"
            );
            break;
        }

        if !started {
            let requested = query.encoder.as_deref().unwrap_or("auto");
            let attempted = candidates
                .iter()
                .map(|(enc, cap, _)| format!("{enc}/{cap:?}"))
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
                    msg
                )))
                .await;
            break;
        }
    }

    info!("h264 stream client disconnected");
    ws_close_task.abort();
    reload_watch_task.abort();
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StreamExit {
    Streamed,
    RestartRequested,
    Unavailable,
    SocketClosed,
}

async fn handle_input_socket(socket: warp::ws::WebSocket, query: InputQuery) {
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

                // Ping/pong for input RTT measurement.
                // Client sends {"type":"ping","ts_ms":N}, host echoes {"type":"pong","ts_ms":N}.
                // This lets the Android HUD display the full input round-trip time so the
                // user can distinguish "input channel lag" from "video pipeline lag".
                if let Ok(obj) = serde_json::from_str::<serde_json::Value>(text) {
                    if obj.get("type").and_then(|v| v.as_str()) == Some("ping") {
                        let ts_ms = obj.get("ts_ms").and_then(|v| v.as_i64()).unwrap_or(0);
                        let pong = format!(r#"{{"type":"pong","ts_ms":{ts_ms}}}"#);
                        let _ = ws_tx.send(Message::text(pong)).await;
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

struct FfmpegConfig {
    out_w: u32,
    out_h: u32,
    fps: u32,
    bitrate_kbps: u32,
    mode: String,
    display_idx: u32,
    encoder: String,
    capture: Capture,
    /// Arguments inserted before the first -f/-i input specifier.
    /// Used to pre-initialise a specific HW device (e.g. for multi-GPU routing).
    pre_input_args: Vec<String>,
}

async fn stream_with_ffmpeg(
    ws_tx: &mut futures::stream::SplitSink<warp::ws::WebSocket, Message>,
    connection_alive: &std::sync::Arc<std::sync::atomic::AtomicBool>,
    restart_requested: &std::sync::Arc<std::sync::atomic::AtomicBool>,
    config: FfmpegConfig,
    amf_settings: Option<Arc<RwLock<HostSettings>>>,
) -> anyhow::Result<StreamExit> {
    let mut args: Vec<String> = Vec::new();

    // Pre-input args (e.g. "-init_hw_device d3d11va=amf_dx:1") must precede -f/-i.
    args.extend(config.pre_input_args.iter().cloned());

    // --- Input source ---
    match config.capture {
        Capture::Ddagrab => {
            // DXGI Desktop Duplication: captures directly from GPU framebuffer.
            // hwdownload copies from GPU VRAM → CPU RAM; format=bgra gives explicit pixel fmt.
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
            // Disable input probing: gdigrab doesn't need it and skipping saves ~500 ms.
            let maybe_target = input::resolve_display_target(config.display_idx).ok();
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

    // --- Global low-latency flags ---
    // nobuffer : disable FFmpeg output buffering between demuxer and encoder
    // low_delay: propagate low-delay hint through the codec pipeline
    // fps_mode cfr: enforce constant framerate — duplicate frames if ddagrab delivers
    //   fewer than `fps` (e.g. 59.94 Hz display gives 59fps with passthrough).
    //   Frame duplication is imperceptible at 60fps and guarantees the decoder always
    //   has exactly 60 frames per second to consume.
    // avioflags direct: bypass the AVIOContext output write-buffer and write encoded
    //   bytes directly to the pipe fd. Eliminates the avio layer's internal 32 KB buffer
    //   that can hold up a partial NAL unit before flushing.
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

    // --- Video filter ---
    // No fps filter here: the fps=fps=N filter holds a one-frame FIFO and adds 16ms of
    // guaranteed latency per-frame. Instead, -r {fps} on the output forces exactly {fps}
    // timestamps via the cfr muxer without any per-frame buffering in the filtergraph.
    // format=yuv420p: convert bgr0 (gdigrab) and bgra (ddagrab) to planar YUV.
    let vf = if config.mode.eq_ignore_ascii_case("mirror") {
        // Mirror mode: fit-center. Scale down to preserve aspect, then pad to fill with black,
        // centering the content. After decoding, TextureView applies a transform matrix for final centering.
        format!(
            "scale={}:{}:force_original_aspect_ratio=decrease,pad={}:{}:(ow-iw)/2:(oh-ih)/2:black,format=yuv420p",
            config.out_w, config.out_h, config.out_w, config.out_h
        )
    } else {
        // Extended mode: fill-crop. Scale up to preserve aspect, then center-crop to target resolution.
        format!(
            "scale={}:{}:force_original_aspect_ratio=increase,crop={}:{}:(iw-{})/2:(ih-{})/2,format=yuv420p",
            config.out_w, config.out_h, config.out_w, config.out_h, config.out_w, config.out_h
        )
    };
    args.extend(["-vf".into(), vf, "-an".into()]);

    // Force exactly fps output frames/sec via muxer timestamp assignment (no FIFO).
    args.extend(["-r".into(), config.fps.to_string()]);

    // --- Encoder + encoder-specific low-latency flags ---
    args.extend(["-c:v".into(), config.encoder.clone()]);
    args.extend(encoder_extra_args(&config.encoder));

    // libx264 on CPU has higher encode cost at very high bitrates; capping bitrate
    // reduces encode queueing and usually lowers interaction latency on CPU-only hosts.
    let effective_bitrate_kbps = if config.encoder == "libx264" {
        config.bitrate_kbps.min(12000)
    } else if config.encoder == "h264_amf" {
        // AMF shows fewer macroblock artifacts in fast motion with a slightly higher floor.
        config.bitrate_kbps.max(18000)
    } else {
        config.bitrate_kbps
    };

    // --- Bitrate / GOP ---
    // GOP = 1 second: avoids large IDR bursts while keeping seek/reconnect latency low.
    // VBV = bitrate/4: gives the encoder ~2x the per-frame budget for I-frames so they
    // are never under-constrained (which causes the blocky flicker every GOP interval).
    // bitrate/8 would save ~1 frame but I-frames become visibly compressed on motion.
    // AMF benefits from a roomier VBV buffer in 1080p motion scenes; it reduces visible
    // blockiness and pacing spikes without reintroducing deep buffering.
    let bufsize = if config.encoder == "h264_amf" {
        effective_bitrate_kbps / 2
    } else {
        effective_bitrate_kbps / 4
    };
    // AMF looks smoother in video playback with a 1-second GOP; other encoders keep the
    // shorter half-second GOP that improves rapid full-scene refresh.
    let gop = if config.encoder == "h264_amf" {
        config.fps.clamp(30, 60)
    } else {
        (config.fps / 2).clamp(15, 30)
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
        // dump_extra: Repeat the SPS+PPS parameter sets before every IDR frame.
        // Without this, parameter sets only appear once at stream start. If the decoder
        // initialises a fraction of a second late (surface not ready, OkHttp delay, etc.)
        // it misses them and produces a black screen until the next reconnect.
        "-bsf:v".into(),
        "dump_extra".into(),
        "-f".into(),
        "h264".into(),
        "-".into(),
    ]);

    let mut cmd = Command::new(ffmpeg_exe());
    cmd.args(&args)
        .kill_on_drop(true)
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped());

    let mut child = cmd.spawn()?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow::anyhow!("ffmpeg stdout unavailable"))?;

    // Drain stderr in background so it never blocks the child process.
    // Lines that look like errors are elevated to WARN so encoder failures are visible
    // in the console without flooding it with normal ffmpeg statistics.
    let enc_name_for_log = config.encoder.clone();
    if let Some(stderr) = child.stderr.take() {
        tokio::spawn(async move {
            use tokio::io::AsyncBufReadExt;
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
            }
        });
    }

    // 4 KB chunks: at 12 Mbps each 16 KB buffer holds ~11 ms of encoded video before
    // it's sent. Cutting to 4 KB reduces that to ~2.7 ms — a real latency reduction
    // without changing any codec or transport parameter.
    let mut buf = vec![0u8; 4 * 1024];
    let mut sent_any = false;

    // Timestamp ticker: every 100 ms send a text frame "T:<host_microseconds_utc>".
    // The Android client computes real end-to-end latency as:
    //   e2e_ms = android_receive_ms - (host_us / 1000)
    // Both devices sync to NTP so clocks agree within ~5 ms, making this a useful
    // display even though it is not a true round-trip measurement.
    let mut tick = tokio::time::interval(tokio::time::Duration::from_millis(100));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut ticker_announced = false;

    while connection_alive.load(std::sync::atomic::Ordering::Relaxed)
        && !restart_requested.load(std::sync::atomic::Ordering::Relaxed)
    {
        tokio::select! {
            result = stdout.read(&mut buf) => {
                let n = result?;
                if n == 0 {
                    break; // ffmpeg exited (encoder unavailable or finished)
                }
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
                if ws_tx
                    .send(Message::binary(buf[..n].to_vec()))
                    .await
                    .is_err()
                {
                    return Ok(StreamExit::SocketClosed);
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
                // Ignore send errors — client may not be listening for text frames.
                let _ = ws_tx.send(Message::text(format!("T:{now_us}"))).await;
            }
        }
    }

    let _ = child.kill().await;

    if !connection_alive.load(std::sync::atomic::Ordering::Relaxed) {
        return Ok(StreamExit::SocketClosed);
    }
    if restart_requested.load(std::sync::atomic::Ordering::Relaxed) {
        return Ok(StreamExit::RestartRequested);
    }
    if sent_any {
        Ok(StreamExit::Streamed)
    } else {
        Ok(StreamExit::Unavailable)
    }
}

/// Resolve the path to `ffmpeg(.exe)`.
///
/// Search order:
/// 1. Same directory as the running executable — bundled distribution places
///    `host-windows.exe` and `ffmpeg.exe` side-by-side.
/// 2. A `bin/` subdirectory next to the executable.
/// 3. Fall back to the name `ffmpeg` and let the OS search PATH.
fn ffmpeg_exe() -> std::path::PathBuf {
    // Explicit override for portable/custom deployments.
    if let Ok(custom) = std::env::var("TABLET_MONITOR_FFMPEG") {
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
            // Prefer repository/package-local runtime ffmpeg so behavior is
            // deterministic across machines and independent from globally
            // installed WinGet ffmpeg builds.
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

    // WinGet (Gyan.FFmpeg) common location on Windows user profiles:
    // %LOCALAPPDATA%\Microsoft\WinGet\Packages\Gyan.FFmpeg_*\ffmpeg-*\bin\ffmpeg.exe
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

/// Same bundled-first resolution for `adb(.exe)`.
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
