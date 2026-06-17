pub fn host_gui_html() -> &'static str {
    r#"<!doctype html>
<html lang="es">
<head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <title>FlexDisplay</title>
    <link rel="preconnect" href="https://fonts.googleapis.com" />
    <link rel="preconnect" href="https://fonts.gstatic.com" crossorigin />
    <link href="https://fonts.googleapis.com/css2?family=Inter:wght@400;500;600;700&family=Orbitron:wght@500;700&display=swap" rel="stylesheet" />
    <style>
        :root {
            --bg-0:#05070d;
            --bg-1:#0b1020;
            --bg-2:#121a2e;
            --panel:rgba(14,22,40,.82);
            --line:rgba(0,229,255,.22);
            --cyan:#00e5ff;
            --violet:#8b5cf6;
            --orange:#ff7a18;
            --text:#e8f0ff;
            --muted:#8fa3c7;
            --ok:#34d399;
            --warn:#f87171;
        }
        * { box-sizing:border-box; }
        body {
            margin:0; min-height:100vh; color:var(--text);
            font-family:Inter,"Segoe UI",sans-serif;
            background:
                radial-gradient(900px 500px at 10% -10%, rgba(0,229,255,.18), transparent 60%),
                radial-gradient(700px 420px at 95% 0%, rgba(139,92,246,.2), transparent 55%),
                radial-gradient(800px 500px at 50% 110%, rgba(255,122,24,.12), transparent 60%),
                linear-gradient(160deg, var(--bg-0), var(--bg-1) 45%, #070b14);
            display:flex; align-items:center; justify-content:center; padding:24px;
        }
        .shell { width:min(920px, 100%); display:grid; gap:16px; }
        .card {
            background:var(--panel);
            border:1px solid var(--line);
            border-radius:20px;
            padding:22px 24px;
            box-shadow:0 24px 80px rgba(0,0,0,.45), inset 0 1px 0 rgba(255,255,255,.04);
            backdrop-filter:blur(12px);
        }
        .hero { display:flex; align-items:center; gap:16px; margin-bottom:8px; }
        .logo {
            width:58px; height:58px; border-radius:16px;
            box-shadow:0 0 24px rgba(0,229,255,.35);
            border:1px solid rgba(0,229,255,.35);
        }
        h1 {
            margin:0; font-family:Orbitron,Inter,sans-serif; font-size:26px;
            letter-spacing:.04em; font-weight:700;
            background:linear-gradient(90deg, #fff, var(--cyan));
            -webkit-background-clip:text; background-clip:text; color:transparent;
        }
        .sub { margin:4px 0 0; color:var(--muted); font-size:14px; }
        .status-row { display:flex; flex-wrap:wrap; gap:10px; margin:14px 0 6px; }
        .pill {
            display:inline-flex; align-items:center; gap:8px;
            padding:8px 12px; border-radius:999px;
            border:1px solid rgba(255,255,255,.08);
            background:rgba(255,255,255,.03); font-size:12px; color:var(--muted);
        }
        .dot { width:8px; height:8px; border-radius:50%; background:var(--ok); box-shadow:0 0 10px var(--ok); }
        .dot.warn { background:var(--warn); box-shadow:0 0 10px var(--warn); }
        .grid { display:grid; grid-template-columns:1fr 1fr; gap:14px; margin-top:10px; }
        .row { display:flex; flex-direction:column; gap:7px; }
        label { font-size:12px; color:var(--muted); font-weight:600; letter-spacing:.03em; text-transform:uppercase; }
        select, button {
            border-radius:12px; border:1px solid rgba(255,255,255,.1);
            padding:11px 12px; font-size:14px; font-family:inherit;
        }
        select {
            background:linear-gradient(180deg, rgba(255,255,255,.06), rgba(255,255,255,.02));
            color:var(--text);
        }
        select:focus { outline:2px solid rgba(0,229,255,.35); border-color:var(--cyan); }
        .preset-meta {
            margin-top:8px; border-radius:12px;
            border:1px solid rgba(0,229,255,.15);
            background:rgba(0,229,255,.05);
            padding:10px 12px; font-size:13px; color:#b8c9e8;
        }
        button { color:#fff; border:none; font-weight:700; cursor:pointer; letter-spacing:.2px; transition:transform .15s ease, filter .15s ease; }
        #save { background:linear-gradient(135deg, var(--orange), #ff5f1a); }
        #refresh { background:linear-gradient(135deg, var(--cyan), #00a8c7); color:#041018; }
        button:hover { filter:brightness(1.06); transform:translateY(-1px); }
        button:disabled { opacity:.55; cursor:wait; transform:none; }
        .actions { margin-top:16px; display:flex; gap:10px; flex-wrap:wrap; }
        .status { margin-top:12px; min-height:20px; font-weight:600; opacity:0; transition:opacity .2s ease; }
        .status.visible { opacity:1; }
        .status.busy { color:var(--muted); }
        .status.ok { color:var(--ok); }
        .status.error { color:var(--warn); }
        .hint { margin-top:10px; font-size:12px; color:var(--muted); line-height:1.5; }
        .wifi-box {
            margin-top:6px; padding:12px 14px; border-radius:14px;
            border:1px dashed rgba(139,92,246,.35);
            background:rgba(139,92,246,.08); font-size:13px; color:#c4b5fd;
        }
        .wifi-box strong { color:#e9d5ff; }
        @media (max-width: 720px) { .grid { grid-template-columns:1fr; } .row.span-2 { grid-column:span 1; } }
        .row.span-2 { grid-column:span 2; }
    </style>
</head>
<body>
    <div class="shell">
        <div class="card">
            <div class="hero">
                <img class="logo" src="/brand/logo.png" alt="FlexDisplay" />
                <div>
                    <h1>FlexDisplay</h1>
                    <div class="sub">Second monitor control — encoder, GPU and streaming profile</div>
                </div>
            </div>
            <div class="status-row">
                <span class="pill"><span id="srvDot" class="dot"></span><span id="srvText">Host online</span></span>
                <span class="pill" id="modePill">Mode —</span>
                <span class="pill" id="adbPill">ADB —</span>
            </div>
            <div id="wifiHint" class="wifi-box" hidden></div>
            <div class="grid">
                <div class="row">
                    <label for="encoder">Preferred encoder</label>
                    <select id="encoder"></select>
                </div>
                <div class="row">
                    <label for="gpu">GPU adapter (optional)</label>
                    <select id="gpu"></select>
                </div>
                <div class="row span-2">
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
            <div class="hint">Closing this window stops the host. Use STOP.bat if the window was closed unexpectedly. USB: connect the tablet and open the Android app — IP is 127.0.0.1.</div>
        </div>
    </div>
<script>
let statusTimer = null;
let presetDefs = [];

function setStatus(message, kind = 'ok', autoClearMs = 0){
    const el = document.getElementById('status');
    if (!el) return;
    if (statusTimer) { clearTimeout(statusTimer); statusTimer = null; }
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

async function loadStatus(){
    try {
        const res = await fetch('/api/status');
        const st = await res.json();
        document.getElementById('modePill').textContent = `Mode ${st.mode.toUpperCase()} · ${st.listen_host}:${st.port}`;
        const adbPill = document.getElementById('adbPill');
        const adbDot = document.createElement('span');
        adbDot.className = 'dot' + (st.adb_device_connected ? '' : ' warn');
        adbPill.textContent = '';
        adbPill.appendChild(adbDot);
        adbPill.append(st.adb_device_connected ? ' Android USB ready' : ' No USB device');
        const wifi = document.getElementById('wifiHint');
        if (st.mode === 'usb' && st.lan_ip) {
            wifi.hidden = false;
            wifi.innerHTML = `<strong>Wi-Fi tip:</strong> to connect over LAN use <strong>${st.lan_ip}</strong> in the tablet app (run WIFI.bat on the PC for Wi-Fi host mode).`;
        } else if (st.mode === 'wifi' && st.lan_ip) {
            wifi.hidden = false;
            wifi.innerHTML = `<strong>Wi-Fi active:</strong> enter <strong>${st.lan_ip}</strong> on the tablet.`;
        } else {
            wifi.hidden = true;
        }
    } catch (_e) {}
}

async function loadAll(){
    await Promise.all([loadStatus(), (async () => {
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
            { value: '', label: 'Automatic (recommended)', detail: 'Adapts to tablet and link: USB up to 1920×1200@60, Wi-Fi up to 1280×720@30.' },
            { value: 'cpu_safe', label: 'Low — 960×544 / 30 fps / 5 Mbps', detail: 'For weak PCs or software encoding.' },
            { value: 'equilibrado', label: 'Balanced — 1280×720 / 60 fps / 10 Mbps', detail: 'Fixed 720p when you want more control than automatic.' },
            { value: 'full_hd', label: 'High — 1920×1080 / 60 fps / 25 Mbps', detail: 'Fixed 1080p for strong GPUs and USB.' },
        ];
        presetDefs.forEach(p => { const o=document.createElement('option'); o.value=p.value; o.textContent=p.label; preset.appendChild(o); });
        preset.value = set.preferred_preset || '';
        renderPresetMeta();
    })()]);
}

function renderPresetMeta(){
    const preset = document.getElementById('preset');
    const enc = document.getElementById('encoder');
    const meta = document.getElementById('presetMeta');
    const selected = presetDefs.find(p => p.value === preset.value) || presetDefs[0];
    if (!meta || !selected) return;
    let detail = selected.detail;
    const encVal = enc ? enc.value : '';
    if (selected.value === 'full_hd' && encVal === 'libx264') {
        detail += ' CPU encoding cannot reach 1080p60 — use NVENC or Automatic for full resolution.';
    } else if (selected.value === 'full_hd' && encVal === '') {
        detail += ' Uses hardware encoding when available.';
    }
    meta.textContent = detail;
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
document.getElementById('encoder').addEventListener('change', renderPresetMeta);
loadAll();
setInterval(loadStatus, 5000);
</script>
</body>
</html>"#
}
