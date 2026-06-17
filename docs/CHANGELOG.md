# Changelog

All notable changes to this project are documented in this file.

El formato está basado en [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.4] - 2026-06-17

### Fixed
- **Antivirus false positives**: host binary ships as `FlexDisplay.exe` with updated PE metadata; first-run prefers **winget** (signed ADB/FFmpeg packages) over extracting downloaded tools into `.runtime`.
- **Defender helper**: `DEFENDER-EXCLUSION.bat` adds the install folder to Microsoft Defender exclusions (optional, run once as admin).

### Added
- **SHA256SUMS.txt** in release packages for integrity verification.

---

## [0.2.3] - 2026-06-17

### Fixed
- **Config panel not opening**: startup splash closed before launching Edge/Chrome; browser starts visibly instead of hidden `wscript` after WinForms splash.

### Changed
- **Encoder tips**: GUI, HUD, and release notes refer to hardware encoders in general (NVENC, AMF, QSV), not only NVIDIA.

---

## [0.2.2] - 2026-06-17

### Fixed
- **First-run START.bat failure on Windows**: UTF-8 em dash inside a PowerShell string was misread as a closing quote under the system ANSI code page, so `start-usb.ps1` failed to parse and the host never started.
- **USB auto-install on first run**: with the parser fixed, `FlexDisplay.apk` installs automatically when the app is missing on a connected device.
- **Logcat capture**: detached `adb logcat` process (no `Start-Job` that died when the launcher exited); fixed `Start-Process` flags.
- **Portable ADB**: `Resolve-AdbPath` and host `adb_exe()` prefer `.runtime\adb\platform-tools\adb.exe` from the package folder.
- **CPU preset caps**: `libx264` + named presets no longer always force 30 fps / 1280×704; `full_hd` on CPU caps to 720p60 on USB with HUD note; `equilibrado` allows 720p60 on USB.
- **Hardware encoder latency**: tighter VBV buffer on GPU encoders (`bufsize = bitrate/12`) for lower glass-to-glass delay.

### Added
- **Startup progress window**: silent `START.bat` launches show a small dark splash with progress bar and status text (downloads, USB setup, APK install, server start) so first-run does not look frozen.

---

## [0.2.1] - 2026-06-17

### Fixed
- **START.bat silent launch**: separate launcher/USB logs (no transcript lock), proper silent `Write-Host` shim, detached host process, `FlexDisplay-OpenGui.vbs` opens the desktop panel visibly.
- **CMD window flashing**: subprocesses (`powershell`, `adb`, `ffmpeg`) use `CREATE_NO_WINDOW` on Windows.
- **USB auto-install**: `FlexDisplay.apk` in the package is preferred; if the Android app is missing on a connected USB device, `START.bat` installs it automatically before launching.

### Changed
- **Android client (v0.2.1)**: dark futuristic theme aligned with the PC panel, glass setup panel, portrait layout.

---

## [0.2.0] - 2026-06-17

### Added
- **Desktop app launcher**: `START.bat` / `WIFI.bat` launch via hidden VBS scripts — no console window; host opens in Edge/Chrome app mode.
- **Host status API**: `GET /api/status` (mode, LAN IP, ADB device connected).
- **Silent startup logs**: `logs/flexdisplay-start.log` when using the desktop launcher.

### Changed
- **PC control panel**: new dark futuristic web UI (Inter + Orbitron, glass panels, live status pills).
- **Android UI**: dark futuristic theme aligned with the desktop panel (cyan/violet accents, glass setup panel).
- **Host binary**: release builds use `windows_subsystem = "windows"` (no extra console when launched directly).

---

## [0.1.9] - 2026-06-17

### Fixed
- **NVENC probe blacklist**: stale `failed_probe_keys` no longer block all encoder candidates; invalid GPU indices are cleared on load and at stream start.
- **Android portrait**: dedicated `layout-port`, scrollable setup panel, stream restart and letterbox reflow on rotation.

---

## [0.1.8] - 2026-06-17

### Fixed
- **Release packaging**: Gradle stdout no longer breaks APK resolution; `FlexDisplay.apk` is always included in the lite ZIP.

### Changed
- **Capture**: hardware encoders (NVENC/AMF/QSV) now try **DXGI (`ddagrab`) first** in mirror mode; GDI remains fallback.
- **NVENC tuning**: GOP = 1 s at 60 fps, tighter VBV (`bufsize = bitrate/8`), `-no-scenecut`, `-forced-idr 0`, `-strict_gop 1`.
- **Android input**: touch→mouse throttle reduced from 8 ms to **4 ms** (250 Hz).
- **USB logcat**: `start-usb.ps1` filters `H264Decoder` and `MainActivity` tags (was non-existent `FlexDisplay`).

### Fixed
- **Settings migration**: cached probe combos using `gdigrab` with HW encoders upgrade to `ddagrab` on load.

---

## [0.1.7] - 2026-06-16

### Changed
- **Simpler end-user UX**: only `START.bat` and `STOP.bat` at package root; removed `START_SAFE`, `USB_SAFE`, `WIFI_SAFE`.
- **Runtime on first start**: `START.bat` downloads ADB/FFmpeg via `ensure-runtime.ps1`; release ZIP no longer embeds `.runtime/` by default (~5 MB vs ~46 MB).
- **Quality presets**: GUI reduced to Automatic / Low / Balanced / High (legacy preset names still load from settings).
- Developer bootstrap moved to `scripts/SETUP_DEV.bat`.

### Fixed
- **package.ps1**: inverted `SkipBundledRuntime` logic corrected; bundling requires `-BundleRuntime` explicitly.

---

## [0.1.6] - 2026-06-16

### Fixed
- **CI Android job**: use `./gradlew` on Ubuntu (was `gradlew.bat`, which only works on Windows).
- **CI Rust job**: added `clippy` and `cargo test` steps; install `rustfmt` + `clippy` components.
- **Clippy**: simplified `capture_order`, removed unnecessary `unwrap` in profile resolution, collapsed nested `if` in settings migration.
- **rustfmt**: formatted host crate sources.

---

## [0.1.5] - 2026-06-16

### Added
- Vector `flexdisplay_logo` drawable so Android builds work without the gitignored PNG asset.
- Release package includes `scripts/lib/Common.ps1` and `encoder-smoke-test.ps1`.

### Changed
- Version bump to **0.1.5** (host + Android).
- Android Gradle uses Windows certificate store for SSL on local builds.
- Release packaging sets `GRADLE_OPTS` for reliable APK builds on Windows.

Includes adaptive streaming profiles (1.3.0): USB/Wi-Fi auto caps, tablet-native resolution, encoder-aware bitrate/fps.

---

## [1.3.0] - 2026-06-16

### Added
- **Adaptive stream profile (default)**: Android sends native device resolution + `transport=usb|wifi`; host adapts fps/bitrate and applies encoder-specific caps.
- **`profile.rs`**: `resolve_base_profile()` + `apply_encoder_profile_caps()` with unit tests.

### Changed
- **Mirror mode** in adaptive profile: output resolution follows the **tablet**, not the PC monitor (avoids upscaling blur on 1200p devices).
- **Android**: USB up to 1920×1200@60 / 8 Mbps; Wi-Fi up to 1280×720@30 / 5 Mbps.
- **AMF bitrate**: clamp 5–18 Mbps (removed 18 Mbps floor on low profiles).
- Host GUI preset default label: "Automatic (adaptive to device)".

---

## [1.2.0] - 2026-06-16

### Added
- **Host modular architecture**: `encoder.rs`, `capture.rs`, `ffmpeg.rs`, `settings.rs`, `stream.rs`.
- **Vendor-aware encoder detection**: NVENC/AMF/QSV filtered by WMI GPU vendor.
- **Probe timeout (1.5s)**: failed encoders advance quickly instead of hanging.
- **Encoder probe cache** in `host-settings.json` with failed-combo blacklist.
- **Manual encoder lock**: GUI-selected encoder skips automatic HW fallback.
- **libx264 adaptive profile**: auto-caps to 1280×720@30 / 4–8 Mbps without a named preset.
- **`preferred_nvenc_gpu`** setting separate from AMF adapter index.
- **`cpu_safe` quality preset** for software encoding.
- **`scripts/lib/Common.ps1`**, `encoder-smoke-test.ps1`, combined CI workflow.
- **Env vars wired**: `FLEXDISPLAY_FPS`, `FLEXDISPLAY_BITRATE`, `FLEXDISPLAY_PORT`, `FORCE_SOFTWARE_ENCODER`, capture size hints.

### Changed
- **Android HUD** shows active encoder + capture backend from `CFG:` frames.
- **Unified `tryStartCodec`** with 1280×720 compatibility fallback.
- **health-check.ps1** uses `ffmpeg -encoders` with vendor filtering.

### Fixed
- Documentation drift around probe timing, settings path, and env configuration.

---

## [1.1.0] - 2026-04-14

### Fixed
- **Invisible Android UI**: The transparent theme background (`windowBackground=transparent`) made text fields and labels invisible on black backgrounds. Explicit dark backgrounds were added in `activity_main.xml` (`#121212` on root layout, `#1E1E1E` on top panel) and white/gray text colors were applied to controls.
- **Black screen on Wi-Fi connect**: The server listened only on `127.0.0.1`, rejecting incoming Wi-Fi clients. Changed default to `0.0.0.0` (configurable with `FLEXDISPLAY_LISTEN`). USB still works through ADB reverse tunnel.

### Changed

#### Host (Rust)
- **H.264 encoder level**: Added `-level 5.1` to NVENC to support 1890x1080 @ 60 fps (Level 4.1 limited throughput to around 245 MB/s).
- **VBV buffer**: Reverted to `bitrate/4` (250ms) from `/8` to remove compression artifacts.
- **FPS filter**: Removed `fps=fps=N` from the filtergraph (it added a ~16ms latency FIFO). Replaced with output `-r {fps}` + `fps_mode cfr`.
- **Listen address**: Default changed to `0.0.0.0` instead of `127.0.0.1`.

#### Android Client
- **Render path**: Reemplazado `TextureView` por `SurfaceView` para aprovechar el overlay HWC directo (sin copia GPU intermedia), reduciendo latencia y uso de CPU.
- **Drain thread**: Cambiado de `HandlerThread+postDelayed(4ms)` a thread dedicado con `dequeueOutputBuffer(4000µs)` bloqueante + prioridad `THREAD_PRIORITY_URGENT_DISPLAY`.
- **Submit thread**: Prioridad `THREAD_PRIORITY_URGENT_AUDIO`, `nalQueue.take()` bloqueante en lugar de `poll(1ms)` para wakeup instantáneo.
- **MediaFormat**: Añadidos `KEY_PRIORITY=0` y `KEY_OPERATING_RATE=60f` (float, no int) para que el codec reserve el máximo throughput del hardware.
- **HUD / FPS display**: Suavizado EMA (alpha=0.25) sobre ventana de 1 segundo para evitar jitter visual en el contador.

### Added

#### Android Client
- **Auto-reconnect**: Automatic exponential backoff on connection loss (1s -> 2s -> 4s -> 8s -> 16s -> 30s max).
- **Display selection**: `Display` field in UI; sent as `?display=N` -> `ddagrab=N` in FFmpeg to choose capture monitor.
- **Server IP field**: `PC IP` replaces room signaling; accepts `127.0.0.1` (USB over ADB) or LAN IPv4 for Wi-Fi.
- **Orientation handling**: `onConfigurationChanged` closes sockets and schedules automatic reconnect on rotation, avoiding Activity restart.
- **Dark UI theme**: `#121212` / `#1E1E1E` with white text, compatible with required `windowBackground=transparent` for SurfaceView.

### Technical Details (updated)
- **Latencia medida**: 14–18 ms (USB)
- **FPS**: 60 estables (Level 5.1 + KEY_OPERATING_RATE)
- **Wi-Fi**: Working; server listens on `0.0.0.0:9001`
- **USB**: ADB reverse tunnel `tcp:9001 -> tcp:9001` unchanged

---

## [1.0.0] - 2026-04-14

### Added

#### Host (Rust)
- ✨ H.264 hardware encoding with FFmpeg
- 🎬 Multi-GPU encoder support: h264_nvenc, h264_qsv, h264_amf
- 📹 Automatic fallback to libx264 (software)
- 🔄 Warp-based WebSocket server
- 📊 GDI screen capture with dynamic scaling
- 🎛️ Query parameters: resolution, FPS, bitrate, fit mode
- 📝 Logging with tracing

#### Android Client
- 📱 MediaCodec hardware H.264 decoder
- 🎨 SurfaceView video rendering
- 📡 WebSocket client with OkHttp3
- 🔍 Annex-B H.264 NAL unit parser
- 🔗 Room-based signaling coordination
- 📊 Real-time UI logs
- ⬜ Fullscreen landscape immersive mode

#### Infrastructure
- 🔌 ADB reverse tunnel USB
- 📋 Setup automation script (PowerShell)
- 📖 Comprehensive SETUP.md documentation
- 🤖 Machine-readable requirements.json
- 📝 README with quick start

### Technical Details

- **Transport**: WebSocket binary frames over USB/ADB
- **Codec**: H.264 Annex-B NAL units
- **Target Performance**: 60 FPS, 3500 kbps, <100ms latency
- **Tested On**: Windows 11, Android 7.0+

### Known Limitations

- ⚠️ Single display capture (primary monitor)
- ⚠️ No audio support yet
- ⚠️ No cursor tracking
- ⚠️ USB-only (WiFi planned)

---

## Future Release Notes

### [1.1.0] - Roadmap

- [ ] Audio streaming (AAC codec)
- [ ] WiFi direct support (fallback)
- [ ] Cursor tracking overlay
- [ ] Mouse/keyboard input from device
- [ ] Settings persistence on Android
- [ ] Recording mode (save stream to file)

### [1.2.0] - Roadmap

- [ ] Linux host support
- [ ] macOS host support
- [ ] Multiple display support
- [ ] Adaptive bitrate based on network
- [ ] Motion detection optimization
- [ ] Thermal throttling protection

### [2.0.0] - Roadmap (Long term)

- [ ] HEVC (H.265) codec support
- [ ] RTMP/RTSP streaming output
- [ ] Web client (browser-based device)
- [ ] Cloud streaming (low latency WebRTC)
- [ ] Network resilience (auto-reconnect)
- [ ] 4K streaming support

---

## Cómo Reportar Cambios

Al hacer commits, usa conventional commits:

```
feat: add feature description
fix: fix bug description
docs: update documentation
test: add tests
perf: improve performance
chore: maintenance tasks
```

Ejemplo:
```
feat: add adaptive bitrate streaming

- Monitorea ancho de banda
- Reduce bitrate if congestion detected
- Recovers to target bitrate when stable

Fixes #123
```
