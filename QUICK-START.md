# FlexDisplay — Quick Start Guide

> This guide is written for non-technical users.
> Follow the steps in order and do not skip any of them.

---

## What you need

| Item | Details |
|---|---|
| Windows 10 or 11 PC | 64-bit, any brand |
| Android device (phone or tablet) | Android 8 or later |
| USB cable **or** Wi-Fi network | Either one works |
| FlexDisplay app installed on Android | The `.apk` file is included in the download package |

---

## Step 1 — Download the program

1. Go to the [FlexDisplay Releases page](https://github.com/FitzVB/tablet-second-monitor/releases).
2. Download the file ending in `.zip` (for example `FlexDisplay-v0.1.0-windows-lite.zip`).
3. Right-click the ZIP file → **Extract All…**
4. Choose a folder you will remember, such as `C:\FlexDisplay`.

---

## Step 2 — Install the app on Android

1. On your Android device, go to **Settings → Security** (or *Apps*) and enable **"Unknown sources"** (or **"Install unknown apps"**).
   *(You only need to do this once.)*
2. Connect the device to the PC via USB.
3. Copy the `FlexDisplay.apk` file (found inside the ZIP you extracted) to the Android device.
4. Open the `.apk` file from the Android device and confirm the installation.

---

## Step 3 — Install the virtual display driver *(only if you want an extended display)*

> If you only want to **mirror** your screen, you can skip this step.

The **Virtual Display Driver (VDD)** is a free, open-source program that creates a virtual monitor in Windows.
This allows your Android device to act as an **extra monitor** (extended display).

- Official page: **[github.com/VirtualDrivers/Virtual-Display-Driver](https://github.com/VirtualDrivers/Virtual-Display-Driver)**
- Direct download: **[VDD Releases page](https://github.com/VirtualDrivers/Virtual-Display-Driver/releases)**

### How to install it


3. Run the installer and click **Install**.
4. When it finishes, **restart your PC** once.

### Verify it works
After restarting, go to **Settings → System → Display** and you should see an extra screen called "Virtual Display" or similar.

---

## Step 4 — Start FlexDisplay

1. Open the folder where you extracted the program.
2. Double-click **`START.bat`**.
3. A black window (terminal) will open and ask for the connection mode:
   - Type `1` and press **Enter** for **USB** (recommended — more stable).
   - Type `2` and press **Enter** for **Wi-Fi**.

---

## Step 5 — Connect the Android device

### USB mode

1. Connect the Android device to the PC with the USB cable.
2. A prompt will appear on the Android device saying **"Allow USB debugging?"** → tap **Allow**.
3. The program configures everything automatically.
4. Open the **FlexDisplay** app on the Android device.
5. The screen should appear immediately. If it does not, tap the **Connect** button.

### Wi-Fi mode

1. Make sure the PC and Android device are on the **same Wi-Fi network**.
2. When you start `START.bat` you will see a message like:
   `[OK] Wi-Fi host IP detected: 192.168.1.50`
   That number is your PC's IP address.
3. Open the **FlexDisplay** app on the Android device.
4. Enter that IP in the address field and tap **Connect**.

---

## Step 6 — Choose the display mode

Inside the FlexDisplay app on the Android device you will see two options:

| Mode | When to use it |
|---|---|
| **Mirror** | Show the same image as your PC on the Android device |
| **Extended** | Use the Android device as an independent second monitor |

**Extended** mode requires the VDD to be installed (Step 3).

---

## How to close the program

Close the black window (terminal) that opened when you ran `START.bat`.
The program will stop automatically and clean up all background processes.

You can also double-click **`STOP.bat`** (inside the `scripts` folder) to stop it without closing the terminal.

---

## Common problems

### The Android device does not appear on the PC (USB mode)
- Make sure the USB cable transfers data (not charge-only).
- Go to **Settings → Developer options** on the Android device and enable **USB debugging**.
- Accept the authorization prompt on the Android device screen.

### The app shows "Cannot connect" (Wi-Fi mode)
- Verify that the PC and Android device are on the **same Wi-Fi network**.
- Check that the IP number in the app matches exactly what the program showed at startup.
- Temporarily disable antivirus or firewall on the PC to test.

### The extended display does not appear in Windows
- Open **Settings → System → Display** and click **Detect**.
- If it still does not appear, restart the PC after installing VDD.

### The video looks cropped or poor quality
- Lower the resolution in the app (try 1280×720).
- In Wi-Fi mode, move the device closer to the router.

### Windows blocked the `.ps1` script
Use **`START_SAFE.bat`** instead of `START.bat`. This mode avoids PowerShell scripts.

---

## Uninstall

1. Delete the folder where you extracted FlexDisplay.
2. Uninstall the FlexDisplay app from the Android device (**Settings → Apps**).
3. If you installed VDD and no longer need it, open it from the Start menu or **Control Panel → Programs** and uninstall it.
