# How this app works

This document is the source of truth for understanding, running, debugging, and reconstructing the project from scratch.

## 1. Goal

Tablet Monitor turns an Android tablet into:
- Mirror mode: a low-latency mirror of a Windows monitor.
- Extended mode: a second display with touch input support.

Core design principles in current stable state:
- Windows host is the source of truth for capture geometry and stream profile.
- Android client is primarily a decoder + renderer + input sender.
- USB mode is the default reliable path.

## 2. Repository layout

- `START.bat`: single entry point for users.
- `scripts/launcher.ps1`: asks user for USB or Wi-Fi and dispatches startup.
- `scripts/start-usb.ps1`: configures ADB reverse and starts host.
- `scripts/start-wifi.ps1`: starts host on LAN and prints likely host IP.
- `scripts/STOP.bat` + `scripts/stop-usb.ps1`: stop host and clean ADB reverse.
- `host-windows/`: Rust backend (capture, encode, websocket server, host GUI).
- `android-client/`: Android app (UI, stream decode, touch input, reconnect logic).
- `docs/SETUP.md`: toolchain/environment setup.

## 3. Runtime architecture

### 3.1 Host process (`host-windows`)

The host runs a websocket/http server (port `9001`) and exposes:
- `GET /health`: health probe.
- `GET /api/capabilities`: available encoders and GPUs.
- `GET /api/settings`: current host settings.
- `POST /api/settings`: persist and hot-apply host settings.
- `GET /displays`: monitor list and geometry.
- `WS /h264?...`: video stream channel.
- `WS /input?...`: touch/mouse input channel.

It also serves a small host GUI (HTML) for:
- encoder selection,
- AMF GPU selection,
- quality preset.

### 3.2 Android process (`MainActivity`)

Android app responsibilities:
- let user choose host IP, mode, optional monitor index,
- open websocket stream + input channels,
- decode H.264 with `MediaCodec`,
- render onto `SurfaceView`,
- send normalized touch events (`down/move/up`) to host.

It does not own the final stream profile policy. Host profile/preset wins.

## 4. Startup flow (user path)

### 4.1 USB mode (recommended)

1. User runs `START.bat`.
2. Launcher asks mode and calls `scripts/start-usb.ps1`.
3. USB script:
   - ensures ADB is available,
   - runs `adb reverse tcp:9001 tcp:9001`,
   - kills old host and stale port owner,
   - sets host env (notably listen `127.0.0.1`),
   - starts `host-windows` release binary or fallback `cargo run --release`.
4. Android app uses host `127.0.0.1`.

### 4.2 Wi-Fi mode

1. User runs `START.bat` and selects Wi-Fi.
2. `scripts/start-wifi.ps1` clears localhost binding override.
3. Host listens on `0.0.0.0:9001`.
4. User enters host LAN IPv4 on Android.

## 5. Video stream protocol

Android opens:
- `WS /h264?w=...&h=...&fps=...&bitrate_kbps=...&mode=mirror|extended&display=n`

Host replies with binary H.264 NAL payloads and optional text messages:
- `CFG:encoder=...;capture=...;w=...;h=...;fps=...;bitrate_kbps=...`
- `T:<host_unix_microseconds>` (periodic timestamp)
- `RESET` (host requests reconnect after settings hot-apply)

### 5.1 Current profile priority (stable behavior)

Host resolves stream parameters in this order:
1. host preset (if selected),
2. host manual override values,
3. host mirror display geometry (for mirror mode),
4. client query (`w/h/fps/bitrate`),
5. defaults.

This keeps hot profile switching functional while preserving robust mirror geometry.

## 6. Capture/encode pipeline

### 6.1 Capture backend policy

Host tries candidates by encoder and capture backend.
Current stable policy:
- Mirror mode prefers `Gdigrab` first (coordinate-based, robust for monitor bounds).
- Extended mode can prioritize `Ddagrab` (DXGI path) unless overridden.

### 6.2 DPI awareness (critical mirror fix)

On Windows startup the host enables process DPI awareness.
Why this matters:
- Without it, monitor geometry may be DPI-virtualized (scaled values).
- Virtualized geometry caused mirror cropping/missing taskbar on some setups.
- With DPI awareness, geometry aligns with physical pixels and mirror framing is correct.

### 6.3 FFmpeg video filter

- Mirror mode: `scale(...,force_original_aspect_ratio=decrease) + pad(...)` (contain/letterbox).
- Extended mode: `scale(...,force_original_aspect_ratio=increase) + crop(...)` (fill/crop).

Encoder fallback chain includes hardware options and software fallback (`libx264`).

## 7. Android decode/render pipeline

### 7.1 Surface + decoder

- `SurfaceView` + `SurfaceHolder.Callback` controls surface readiness.
- `H264Decoder` wraps `MediaCodec` and starts after SPS/PPS (with guarded fallback).
- output format changes provide buffer/crop/picture dimensions.

### 7.2 Aspect handling

Current stable visual behavior:
- stream uses contain strategy (no geometric stretching),
- client computes destination size preserving aspect ratio,
- `SurfaceView` is centered to keep bars balanced.

### 7.3 Reconnect/watchdog

- stream stall watchdog triggers reconnect if no video chunks in threshold window,
- host `RESET` and websocket failures trigger controlled reconnect backoff.

## 8. Input pipeline (touch to Windows mouse)

Android sends normalized coordinates (`x_norm`, `y_norm`) over `WS /input`.
Host maps normalized values into selected monitor desktop rectangle and injects mouse events via `SendInput` with `MOUSEEVENTF_VIRTUALDESK`.

This keeps input aligned with selected display in both mirror and extended modes.

## 9. Persistence and hot-apply

Host settings are stored in:
- `host-windows/target/release/host-settings.json` (runtime path for release run)

When host GUI saves settings:
- file is persisted,
- running stream receives reload signal,
- host asks client to reconnect (`RESET`) to apply new profile safely.

## 10. Localization model

Android resources are split by locale:
- `android-client/app/src/main/res/values/` (default locale)
- `android-client/app/src/main/res/values-es/` (Spanish locale)

Language button in app toggles language preference and recreates activity.

## 11. Known-good behavior checklist

A session is considered healthy when:
- host logs show mirror geometry in physical pixels (for example `1920x1080`),
- host logs show active stream profile and first H.264 bytes sent,
- Android logs show repeated `VideoTransform` entries with valid picture dimensions,
- tablet displays full desktop content (taskbar visible in mirror when expected),
- profile changes in host GUI trigger reconnect and apply new profile.

## 12. Rebuild from zero (disaster recovery)

### 12.1 Toolchain prerequisites

Install and verify:
- Rust (`rustc`, `cargo`),
- Android Platform-Tools (`adb`),
- JDK 17,
- Android SDK/build-tools (via Gradle wrapper).

### 12.2 Build host

From `host-windows/`:
- `cargo build --release`

### 12.3 Build Android app

From `android-client/`:
- `gradlew.bat assembleDebug`

### 12.4 Install app

- `adb install -r android-client/app/build/outputs/apk/debug/app-debug.apk`

### 12.5 Run system

- Start host with `START.bat` (USB preferred).
- On Android set host to `127.0.0.1` in USB mode.
- Tap Connect.

### 12.6 Validate quickly

- `adb logcat -s VideoTransform -d`
- confirm host terminal logs stream profile and byte flow.

## 13. If mirror looks cropped again

Run this decision tree:
1. Confirm host DPI awareness is active in code path (startup call present).
2. Confirm host logs mirror geometry matches expected physical monitor size.
3. Confirm mirror capture path uses coordinate-based backend first.
4. Confirm Android is in contain mode (not forced stretch).
5. Confirm no stale host binary is running (kill old process, restart).

## 14. Practical maintenance notes

- Keep screenshot artifacts out of commits unless explicitly needed.
- Prefer USB mode for deterministic testing.
- Preserve host-first profile ownership; avoid making client query override host presets.
- After changes affecting geometry, always validate with both logs and screenshot.

---

This document is intentionally operational and reconstruction-oriented so any AI or developer can recover a known-good state quickly.
