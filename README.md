# FlexDisplay (Windows + Android)

<p align="center">
  <a href="https://ko-fi.com/obsidian_motion" target="_blank">
    <img src="https://ko-fi.com/img/githubbutton_sm.svg" alt="Support me on Ko-fi" />
  </a>
</p>

Turn an Android device into a touch-enabled second monitor for Windows over USB (recommended) or Wi-Fi.

> **New to FlexDisplay?** Read the step-by-step guide for non-technical users:
> [English — Quick Start Guide](QUICK-START.md) · [Español — Guía de inicio rápido](QUICK-START.es.md)

## Minimum Requirements

### Windows PC (host)

| Component | Minimum | Recommended |
|-----------|---------|-------------|
| OS | Windows 10 64-bit (build 1903+) | Windows 10/11 22H2+ |
| CPU | Any x64 with SSE2 | 4-core / 8-thread or better |
| RAM | 4 GB | 8 GB+ |
| GPU | Any (software `libx264` fallback) | NVIDIA GTX 1060+ **or** AMD RX 580+ **or** Intel Gen 6+ iGPU |
| Driver — NVIDIA (NVENC) | Game Ready 452.39+ / Studio 452.06+ | Latest Game Ready |
| Driver — AMD (AMF) | Adrenalin 21.4.1+ | Latest Adrenalin |
| Driver — Intel (QSV) | DCH Graphics 27.20+ | Latest |
| FFmpeg | 6.0+ (bundled automatically) | 7.x release-essentials |
| ADB | Platform-tools 31+ (bundled automatically) | Latest |
| USB cable | USB 2.0 | USB 3.0 (reduces latency to ≤ 5 ms) |
| Visual C++ Runtime | 2019 Redistributable | 2022 Redistributable |

> **Software fallback:** No GPU hardware encoder required. The host automatically falls back to `libx264` (CPU H.264 encoding) on any machine. Hardware encoding is faster and uses less CPU — it is detected and selected automatically at startup.

> **Encoder auto-learn:** On first successful stream the host saves the working encoder/capture/GPU combo to `host-settings.json` (next to the running `host-windows.exe`). Subsequent connections try the cached combo first; failed combos are blacklisted for 1.5s probe timeout cycles.

> **Manual encoder lock:** Selecting an encoder in the host GUI (`http://127.0.0.1:9001`) locks that encoder — no automatic HW fallback afterward.

### Android device (client)

| Component | Minimum |
|-----------|---------|
| Android version | 7.0 Nougat (API 24) |
| H.264 decoder | Hardware AVC decoder (present on virtually all devices since 2012) |
| H.264 profile | Main Profile Level 3.1 (Android CDD requirement — guaranteed on all devices) |
| Network | USB 2.0 cable **or** Wi-Fi 802.11n (5 GHz strongly recommended) |
| Screen | Any resolution — host scales output automatically |
| RAM | 1 GB free | 2 GB+ |

> **Tested devices:** Samsung Galaxy Tab S series, Lenovo Tab P series, Fire HD 10 (with Play Store). Any Android 7+ device with a hardware H.264 decoder will work.

---

### Encoder compatibility matrix

| Encoder | Hardware required | Bitrate used | Typical latency | Auto-selection order |
|---------|------------------|--------------|-----------------|----------------------|
| `h264_nvenc` | NVIDIA GPU (Kepler+, GTX 600 / RTX series) | 8 Mbps cap | ≤ 5 ms encode | 1st — if NVIDIA GPU detected (WMI) |
| `h264_qsv` | Intel CPU with Quick Sync (Haswell 4th gen+) | configurable | ≤ 10 ms encode | 2nd — if Intel iGPU detected |
| `h264_amf` | AMD GPU (GCN 1st gen+, RX 400 series+) | 18 Mbps floor | ≤ 10 ms encode | 3rd — if AMD GPU detected |
| `libx264` | None — pure CPU | 8 Mbps max* | 15–40 ms encode | Always last / guaranteed fallback |

\* Without a named GUI preset, libx264 auto-downgrades to max 1280×720 @ 30fps. Use preset `cpu_safe` or `ahorro` on weak PCs.

**NVENC technical notes (RTX / GTX):**
- Uses H.264 **Main Profile** + AUD NAL units for maximum Android MediaCodec compatibility.
- B-frames disabled (`-bf 0`), CBR mode, zero-latency tuning — optimised for live streaming, not file encoding.
- Bitrate capped at 8 Mbps to stay within Android hardware decoder input buffer limits.

**AMD AMF technical notes:**
- Uses H.264 **Baseline Profile** for broad Android compatibility.
- CBR low-latency mode with async depth 1.

The host probes each encoder **per WebSocket connection** (not only at process startup). Vendor-aware filtering skips NVENC on AMD-only PCs, AMF on NVIDIA-only PCs, etc. If an encoder produces zero output within **1.5 seconds**, the next candidate is tried automatically — **no manual action needed.**

---

## Current status

- Touch input on extended monitor: fixed.
- Android UI with menu, HUD, and FAQ: updated.
- Single startup launcher: `START.bat` (downloads ADB/FFmpeg on first run).
- Stop utility: `STOP.bat`.

## Single structure (source of truth)

- `START.bat`: main startup entry point (end users only need this + `STOP.bat`).
- `STOP.bat`: stops host and clears `adb reverse`.
- `scripts\launcher.ps1`: internal launcher (USB/Wi-Fi); not meant for direct use.
- `docs\SETUP.md`: environment setup.
- `host-windows\`: Rust host server.
- `android-client\`: Android app.

## Quick start

### Lightweight precompiled distribution (recommended for end users)

To create a lightweight package with precompiled binaries:

```powershell
.\scripts\release.ps1
```

Default release behavior (recommended for fewer antivirus false positives):

- Does NOT embed ADB/FFmpeg binaries directly in the ZIP.
- Downloads ADB/FFmpeg automatically on first launch from official sources.

If you explicitly want fully bundled runtime in the ZIP:

```powershell
.\scripts\release.ps1 -BundleRuntime
```

Alternative (advanced):

```powershell
.\scripts\package.ps1
```

Output:

- `dist\FlexDisplay-vX.X.X-windows-lite\`
- `dist\FlexDisplay-vX.X.X-windows-lite.zip`

Package characteristics:

- Includes precompiled host binary (`host-windows.exe`)
- Includes runtime bootstrap scripts under `scripts\` (runtime can be bundled or downloaded on first launch)
- Includes startup scripts (`START.bat`, `scripts\*.ps1`)
- Includes APK if available (`FlexDisplay.apk`)
- By default, `release.ps1` builds a fresh Android debug APK before packaging
- Does not require Rust/JDK installation on end-user PCs
- `START.bat` checks VDD and attempts automatic install if missing (including admin retry via UAC)

Target size for end-user package:

- Typical ZIP target: ~5–10 MB (host + APK + scripts; no embedded FFmpeg/ADB)
- First run downloads ~110 MB of tools into `.runtime\` (one time)

### Developer setup (new PC / from source)

If you are building from source on a clean Windows PC:

```bat
scripts\SETUP_DEV.bat
```

What it does automatically:

- Installs required toolchains locally in the project folder (`.runtime`)
   - Rust/Cargo
   - ADB platform-tools
   - Java 17 (JDK)
   - FFmpeg
- Installs virtual display driver (winget)
- Builds host release binary (first run)
- Builds Android debug APK (first run)
- Launches `START.bat`

Notes:

- You may need to accept UAC/admin prompts from Windows.
- First run can take several minutes (toolchains + first builds).
- If virtual display was just installed, one reboot may be required.
- Local tools are kept under `.runtime` so startup does not depend on global PATH.

1. Run `START.bat` from the project root.
2. Choose mode:
   - `1` USB (recommended)
   - `2` Wi-Fi
3. In USB mode, the launcher now does this automatically:
   - Detect/select Android device
   - Install/update Android app (if APK exists)
   - Configure `adb reverse tcp:9001 tcp:9001`
   - Open the Android app
   - Start host server
4. If needed, tap `Connect` in the Android app.

### PowerShell blocked by policy

If Windows blocks `.ps1` scripts, run once in PowerShell (as your user):

```powershell
Set-ExecutionPolicy -Scope CurrentUser RemoteSigned
```

Then run `START.bat` again.

### Recommended USB mode

- Enable USB debugging on Android.
- Connect via USB cable.
- Accept the first ADB authorization prompt on the device.
- Host in app should stay as `127.0.0.1`.

### Wi-Fi

- PC and device must be on the same network.
- The launcher tries to show the PC LAN IP.
- In the app, use that IP and port `9001`.

## Multiple Android Devices and Virtual Displays

Use this when you want 2, 3, or 4 Android devices as different monitors.

### What you need to know first

- Mirror mode duplicates a monitor.
- Extended mode needs extra displays in Windows (physical or virtual).
- Wi-Fi mode does not require ADB between Android devices.

### Step 1. Install virtual display support (one-time)

> **Virtual Display Driver (VDD)** is a free, open-source signed driver maintained by the community.
> Official page: **[github.com/VirtualDrivers/Virtual-Display-Driver](https://github.com/VirtualDrivers/Virtual-Display-Driver)**
> Direct download (installer): **[Releases page](https://github.com/VirtualDrivers/Virtual-Display-Driver/releases)**

You can install it in two ways:

**Option A — Script (recommended):** from the project root, run:

```powershell
.\scripts\install-virtual-display.ps1
```

**Option B — Manual installer:** download `Virtual-Driver-Control-Installer.exe` from the [VDD Releases page](https://github.com/VirtualDrivers/Virtual-Display-Driver/releases), run it, and click **Install**.

### If you need more virtual displays

If you already installed the virtual driver but only see one virtual monitor, do this:

1. Open `C:\VirtualDisplayDriver\vdd_settings.xml` in Notepad.
2. Find:

```xml
<monitors>
   <count>1</count>
</monitors>
```

3. Change `count` to the number you want (for example `2`, `3`, or `4`).
4. Save the file.
5. Reload/restart the virtual display driver (or reboot Windows).
6. Open Windows Settings -> System -> Display and click Detect.

Example for two virtual displays:

```xml
<monitors>
   <count>2</count>
</monitors>
```

### Step 2. Create and arrange displays in Windows

1. Open Windows Settings -> System -> Display.
2. Click Detect if Windows does not show new displays yet.
3. Set desktop mode to Extend these displays.
4. Arrange display positions to match your real setup (left/right/top).

### Step 3. Start host in Wi-Fi mode

1. Run START.bat.
2. Select Wi-Fi.
3. Note the host LAN IP shown by the launcher.

### Step 4. Connect each device to a different monitor

1. On each device, use the same host IP.
2. Choose mode for each device: duplicate view = Mirror, extra desktop = Extended.
3. Set a different Target monitor number for each device in Extended mode.
4. Keep Auto only if you connect one extended device.

### Step 5. Verify monitor indexes

Use one of these options:

1. In the app, open Menu -> Select detected monitor.
2. Open http://127.0.0.1:9001/displays on the host PC.

### Remove virtual display driver (optional)

```powershell
.\scripts\remove-virtual-display.ps1
```

### Troubleshooting for non-technical users

1. If Extended looks identical to Primary, Windows is not in Extend mode yet.
2. If two Android devices show the same extended screen, assign different Target monitor numbers.
3. If a device cannot connect in Wi-Fi mode, verify both devices are on the same network and the app uses the PC LAN IP.
4. If you cannot find a VDD app in Start menu, edit `C:\VirtualDisplayDriver\vdd_settings.xml` and change `<count>` as shown above, then reload the driver.

## Stop

To stop everything and clean the USB tunnel:

```bat
STOP.bat
```

## In-app FAQ

From the `FAQ` button you can choose language:

- Spanish
- English

Content is shown in the selected language, independent from system language.

## Is it plug and play for non-technical users?

It is **close**, but still not 100% zero-friction.

### Already solved

- Single startup file (`START.bat`).
- Single stop file (`STOP.bat`).
- Stable USB workflow for daily use.

### Still requires minimal technical steps

- Enable USB debugging on Android.
- Accept ADB authorization on first use.
- Have PC dependencies ready (Rust/cargo/ffmpeg/adb, depending on scenario).

## Next milestone for true non-technical use

- Package a Windows installer with verified dependencies.
- Bundle `adb`/runtime in final distribution.
- Add first-run setup wizard with automatic checks.

## Development

### NVENC / Hardware encoder troubleshooting

If `h264_nvenc` is selected in the GUI but the stream does not start:

1. Check the NVIDIA driver version — NVENC requires **452.39+** (Game Ready) or **452.06+** (Studio).
   Older drivers (e.g., 390.x) expose NVENC in `ffmpeg -encoders` but fail at runtime.
2. Run the diagnostic script to confirm NVENC works outside the app:
   ```powershell
   powershell -ExecutionPolicy Bypass -NoProfile -File .\scripts\collect-nvenc-diagnostics.ps1
   ```
   Output is saved to `logs\nvenc-diagnostic-<timestamp>.txt`.
3. Check `logs\ffmpeg-h264_nvenc-<timestamp>.txt` — every stream attempt writes full ffmpeg stderr there, including the exact error message from NVENC.
4. On multi-GPU systems (e.g., laptop with iGPU + dGPU), the host now tries each GPU index automatically (`-gpu 0`, `-gpu 1`, …). If one index fails, the next is tried before falling back to a software encoder.
5. Force a specific GPU from the GUI: open `http://127.0.0.1:9001`, select the encoder and the GPU adapter, click **Save and apply**.
6. If all NVENC attempts fail, the host falls back to `libx264` transparently — video will still work, just with higher CPU usage.

### Android

```powershell
Set-Location android-client
.\gradlew.bat assembleDebug
```

### Host (Rust)

```powershell
Set-Location host-windows
cargo build --release
```

## Additional documentation

- Detailed setup: `docs\SETUP.md`
- Maintenance utilities: `scripts\health-check.ps1`, `scripts\setup.ps1`
