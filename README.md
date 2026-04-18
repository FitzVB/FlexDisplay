# FlexDisplay (Windows + Android)

Turn an Android device into a touch-enabled second monitor for Windows over USB (recommended) or Wi-Fi.

## Current status

- Touch input on extended monitor: fixed.
- Android UI with menu, HUD, and FAQ: updated.
- Single startup launcher: `START.bat`.
- Stop utility: `scripts\STOP.bat`.

## Single structure (source of truth)

- `START.bat`: main startup entry point.
- `scripts\STOP.bat`: stops host and clears `adb reverse`.
- `scripts\launcher.ps1`: launcher logic (USB/Wi-Fi).
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

- Typical ZIP target: ~250MB to 450MB
- Exact size depends mainly on FFmpeg build and APK size

### New PC (first-time setup in one click)

If this is a new Windows PC with no dependencies installed yet, run:

```bat
SETUP_AND_START.bat
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

### Antivirus fallback (no PowerShell)

If your antivirus blocks `.ps1` scripts, use the safe batch-only path:

1. Run `START_SAFE.bat`.
2. For USB setup help (no PowerShell), run `USB_SAFE.bat`.
3. For Wi-Fi IP guidance (no PowerShell), run `WIFI_SAFE.bat`.

Notes:

- `START_SAFE.bat` starts the host without invoking PowerShell.
- In USB mode you may still need to run ADB commands manually.

### Recommended USB mode

- Enable USB debugging on Android.
- Connect via USB cable.
- Accept the first ADB authorization prompt on the device.
- Host in app should stay as `127.0.0.1`.

### Wi-Fi

- PC and device must be on the same network.
- The launcher tries to show the PC LAN IP.
- In the app, use that IP and port `9001`.

## Multiple Tablets and Virtual Displays

Use this when you want 2, 3, or 4 Android devices as different monitors.

### What you need to know first

- Mirror mode duplicates a monitor.
- Extended mode needs extra displays in Windows (physical or virtual).
- Wi-Fi mode does not require ADB between tablets.

### Step 1. Install virtual display support (one-time)

From the project root, run:

```powershell
.\scripts\install-virtual-display.ps1
```

This installs a signed virtual display driver using winget.

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
2. If two tablets show the same extended screen, assign different Target monitor numbers.
3. If a device cannot connect in Wi-Fi mode, verify both devices are on the same network and the app uses the PC LAN IP.
4. If you cannot find a VDD app in Start menu, edit `C:\VirtualDisplayDriver\vdd_settings.xml` and change `<count>` as shown above, then reload the driver.

## Stop

To stop everything and clean the USB tunnel:

```bat
scripts\STOP.bat
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
- Single stop file (`scripts\STOP.bat`).
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
