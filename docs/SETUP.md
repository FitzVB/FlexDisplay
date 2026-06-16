# Setup Guide - FlexDisplay

Complete installation and configuration guide for this project.

## Recommended workflow

For daily use, avoid running the host manually unless you are debugging.

- Start: `START.bat`
- Stop and clean USB tunnel: `STOP.bat`
- USB is the recommended mode.

---

## System Requirements

### Windows Host (PC)
- **OS**: Windows 10/11
- **Architecture**: x64 (64-bit)
- **Disk Space**: ~2GB (Rust toolchain + Android SDK)
- **RAM**: 4GB minimum (8GB recommended)
- **Network**: USB cable for device connection

### Android Device
- **Version**: Android 7.0+ (API level 24+)
- **Features**: USB debugging capability
- **Connection**: USB Type-C or Micro-USB data cable

---

## Prerequisites Installation

### 1. Rust Toolchain (Required)

**Status**: [Required for host compilation]

**Why**: Host application is written in Rust

**Installation**:

```powershell
# Option A: Using Rust installer (recommended)
Invoke-WebRequest -Uri https://forge.rust-lang.org/infra/other-installation-methods.html -OutFile $(Join-Path $env:TEMP RustSetup.exe)
& $(Join-Path $env:TEMP RustSetup.exe)

# Option B: Using Scoop package manager
scoop install rustup
rustup default stable

# Option C: Using Chocolatey
choco install rustup.install
rustup default stable
```

**Verification**:
```powershell
rustc --version
cargo --version
```

**Expected Output**:
```
rustc 1.78.0 (9b3a5a0d5 2024-04-29)
cargo 1.78.0 (54d8815b0 2024-04-26)
```

---

### 2. Android SDK Platform-Tools (Required for USB)

**Status**: [Required for ADB]

**Why**: ADB (Android Debug Bridge) enables USB tunneling to Android device

**Installation**:

```powershell
# Option A: Via Scoop
scoop install adb

# Option B: Via Chocolatey
choco install adb

# Option C: Manual setup
$sdkPath = "$env:LOCALAPPDATA\Android\Sdk"
# Download from: https://developer.android.com/tools/releases/platform-tools
# Extract to: C:\Users\<Username>\AppData\Local\Android\Sdk\platform-tools
# Add to PATH manually
```

**Verification**:
```powershell
adb version
```

**Expected Output**:
```
Android Debug Bridge version 1.0.41
Version 35.0.1
```

**PATH Configuration** (if manual):
```powershell
$env:Path += ";$env:LOCALAPPDATA\Android\Sdk\platform-tools"
[Environment]::SetEnvironmentVariable("Path", $env:Path, "User")
```

---

### 3. Java Development Kit (JDK) for Android Build (Required)

**Status**: [Required for APK compilation]

**Why**: Android Gradle build requires Java compiler

**Installation**:

```powershell
# Option A: Using Scoop
scoop install openjdk17

# Option B: Using Chocolatey
choco install openjdk17

# Option C: Using Microsoft Build of OpenJDK (built-in to VS 2022)
# Check: C:\Program Files\Microsoft\jdk-17.0.x
```

**Verification**:
```powershell
java -version
javac -version
```

**Expected Output**:
```
openjdk version "17.0.x" 2021-09-14
OpenJDK Runtime Environment ...
```

**JAVA_HOME Configuration** (if needed):
```powershell
$env:JAVA_HOME = "C:\Program Files\OpenJDK\jdk-17.0.6"
[Environment]::SetEnvironmentVariable("JAVA_HOME", $env:JAVA_HOME, "User")
```

---

### 4. Android SDK (Gradle Managed - Semi-Automatic)

**Status**: [Auto-downloaded by Gradle]

**Why**: Required for APK compilation

**Automatic Setup**:
```powershell
cd android-client
.\gradlew assembleDebug  # First run downloads SDK automatically
```

**Manual Setup** (if needed):
```powershell
$sdkRoot = "$env:LOCALAPPDATA\Android\Sdk"
& "$sdkRoot\cmdline-tools\latest\bin\sdkmanager.bat" `
    --sdk_root=$sdkRoot `
    "platforms;android-33" `
    "build-tools;34.0.0" `
    "android-sdk-platform-tools"
```

---

### 5. Gradle (Gradle Wrapper - Automatic)

**Status**: [Wrapped - No separate installation needed]

**Why**: Gradle Wrapper ensures consistent build environment

**Note**: First build downloads Gradle wrapper automatically

---

## Project-Specific Setup

### Clone Repository

```powershell
git clone https://github.com/FitzVB/tablet-second-monitor.git
cd tablet-second-monitor
```

### Verify All Tools

```powershell
$tools = @(
    "rustc",
    "cargo",
    "adb",
    "java",
    "javac"
)

foreach ($tool in $tools) {
    try {
        $version = & $tool --version 2>&1
        Write-Host "✓ $tool installed: $($version[0])"
    } catch {
        Write-Host "✗ $tool NOT found - Please install it"
    }
}
```

---

## Build Instructions

### Host (Windows)

**Build Debug**:
```powershell
cd host-windows
cargo build
```

**Run Debug**:
```powershell
$env:FLEXDISPLAY_LISTEN = "127.0.0.1"
cargo run
```

**Build Release** (optimized):
```powershell
cargo build --release
.\target\release\host-windows.exe
```

**Environment Variables** (optional):
```powershell
$env:FLEXDISPLAY_HW_ENCODER = "h264_nvenc"  # GPU encoder: h264_nvenc, h264_qsv, h264_amf, libx264
$env:FLEXDISPLAY_LISTEN = "127.0.0.1"      # Listen address
$env:FLEXDISPLAY_FPS = "60"                 # Target FPS
$env:FLEXDISPLAY_BITRATE = "3500"           # Bitrate in kbps
$env:FLEXDISPLAY_MODE = "mirror"            # mirror | extended
$env:FLEXDISPLAY_EXTENDED_DISPLAY = "1"     # Optional display index override
```

**Virtual Display Driver**:
```powershell
# Install a signed virtual display driver from winget
.\scripts\install-virtual-display.ps1

# Install from a bundled INF package instead
.\scripts\install-virtual-display.ps1 -Provider inf -InfPath .\drivers\virtual-display\YourDriver.inf

# Remove drivers matching the default filter
.\scripts\remove-virtual-display.ps1
```

Notes:
- `extended` only becomes a true external monitor when Windows has a non-primary display to extend onto.
- For a software-only monitor, that means installing a signed indirect display driver.
- After installation, the host exposes `http://127.0.0.1:9001/displays` so you can inspect detected display indexes.
- If you need more than one virtual monitor, edit `C:\VirtualDisplayDriver\vdd_settings.xml` and set `<monitors><count>N</count></monitors>` (for example `2`, `3`, or `4`), then reload the driver or reboot.

---

### Android Client

**Build Debug APK**:
```powershell
cd android-client
.\gradlew assembleDebug
# Output: app\build\outputs\apk\debug\app-debug.apk
```

**Build Release APK** (requires signing):
```powershell
.\gradlew assembleRelease
# Requires keystore configuration
```

**Run Tests**:
```powershell
.\gradlew test
```

---

## Device Setup

### Enable USB Debugging on Android Device

1. **Developer Options**:
   - Settings → About → Tap "Build Number" 7 times
   - Developer Options appears in Settings menu

2. **Enable USB Debugging**:
   - Settings → Developer Options → USB Debugging → ON

3. **Connect Cable**:
  - Plug USB data cable into Android device and Windows PC
  - Device may show "Allow USB debugging?" -> Tap "Allow"

### Verify ADB Connection

```powershell
adb devices
```

**Expected Output**:
```
List of attached devices
XXXXXXXXXXXXXXXX device
```

---

## First Run Workflow

### 1. Build Everything

```powershell
# Terminal 1: Build Host
cd host-windows
cargo build

# Terminal 2: Build Android
cd android-client
.\gradlew assembleDebug
```

### 2. Install APK on Device

```powershell
adb install -r android-client\app\build\outputs\apk\debug\app-debug.apk
```

### 3. Setup USB Reverse Tunnel

```powershell
adb reverse tcp:9001 tcp:9001
```

### 4. Start Host Server

```powershell
cd host-windows
$env:FLEXDISPLAY_LISTEN = "127.0.0.1"
cargo run
```

### 5. Launch App on Device

```powershell
adb shell am start -n com.flexdisplay.android/.MainActivity
```

### 6. Connect on Device UI

- Tap "Connect" in the device app
- Video stream should appear on SurfaceView
- View connection logs in app UI

---

## Automated Setup Script

For CI/CD or rapid setup, use automation-ready commands:

```json
{
  "dependencies": {
    "rust": {
      "name": "Rust Toolchain",
      "command": "rustup show",
      "check": "rustc --version",
      "url": "https://rustup.rs"
    },
    "adb": {
      "name": "Android Debug Bridge",
      "command": "adb version",
      "check": "adb --version",
      "url": "https://developer.android.com/tools/releases/platform-tools"
    },
    "java": {
      "name": "Java Development Kit",
      "command": "java -version",
      "check": "java -version",
      "url": "https://adoptium.net"
    }
  },
  "build_steps": {
    "host": {
      "dir": "host-windows",
      "command": "cargo build"
    },
    "android": {
      "dir": "android-client",
      "command": ".\\gradlew assembleDebug"
    }
  },
  "deployment": {
    "adb_reverse": "adb reverse tcp:9001 tcp:9001",
    "install_apk": "adb install -r android-client\\app\\build\\outputs\\apk\\debug\\app-debug.apk",
    "start_host": "cd host-windows && cargo run",
    "start_app": "adb shell am start -n com.flexdisplay.android/.MainActivity"
  }
}
```

---

## Troubleshooting

### ADB Device Not Found

```powershell
# Check USB connection
adb devices

# Reconnect device
adb disconnect
adb connect <device_ip>  # If using network

# Check drivers on Windows
# Device Manager → Look for Android devices
```

### Rust Compilation Errors

```powershell
# Update Rust
rustup update stable

# Clear cargo cache
cargo clean

# Rebuild
cargo build
```

### Gradle Build Fails

```powershell
# Clear Gradle cache
.\gradlew clean

# Rebuild
.\gradlew assembleDebug

# Check Java version
java -version
```

### WebSocket Connection Refused

```powershell
# Verify port 9001 is not in use
netstat -ano | findstr :9001

# Verify ADB reverse is active
adb reverse -l

# Reestablish reverse tunnel
adb reverse tcp:9001 tcp:9001
```

### MediaCodec Errors on Android

```powershell
# Check device API level
adb shell getprop ro.build.version.sdk

# Check available codecs
adb shell dumpsys media.codec
```

---

## Development Workflow

### Local Testing Loop

```powershell
# Terminal 1: Monitor host logs
cd host-windows
cargo run

# Terminal 2: Monitor device logs
$env:Path += ";$env:LOCALAPPDATA\Android\Sdk\platform-tools"
adb logcat -s "FlexDisplay"

# Terminal 3: Rebuild/redeploy on change
cd android-client
.\gradlew assembleDebug
adb install -r app\build\outputs\apk\debug\app-debug.apk
adb shell am start -n com.flexdisplay.android/.MainActivity
```

### Git Workflow

```powershell
# After making changes
git add .
git commit -m "your message"
git push origin master
```

---

## Performance Notes

- **FFmpeg Encoders** (auto-detected fallback):
  - `h264_nvenc` (NVIDIA GPU) - fastest
  - `h264_qsv` (Intel GPU) - fast
  - `h264_amf` (AMD GPU) - fast
  - `libx264` (CPU software) - slower fallback

- **Recommended Settings**:
  - Resolution: 960×540
  - FPS: 60
  - Bitrate: 3500 kbps
  - Codec: H.264 hardware

---

## FAQ

**Q: Can I use WiFi instead of USB?**
A: Yes, modify `startH264Stream()` URL from `ws://127.0.0.1:9001/h264` to device IP

**Q: FFmpeg not found**
A: Install via `winget install ffmpeg` or `choco install ffmpeg`

**Q: How to revert to JPEG streaming?**
A: Android URL provides both `/stream` (JPEG) and `/h264` (video) endpoints

**Q: Can I build on Linux/Mac?**
A: Yes, modify scripts for Bash and use appropriate build commands
