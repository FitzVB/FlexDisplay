# Changelog

Todos los cambios significativos en este proyecto están documentados en este archivo.

El formato está basado en [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
y este proyecto sigue [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.1.0] - 2026-04-14

### Fixed
- **UI invisible en Android**: El fondo transparente del tema (`windowBackground=transparent`) hacía que los campos de texto y textos fueran invisibles sobre fondo negro. Se añadieron fondos oscuros explícitos en `activity_main.xml` (`#121212` en el layout raíz, `#1E1E1E` en el panel superior) y colores de texto blancos/grises en todos los controles.
- **Pantalla negra al conectar por Wi-Fi**: El servidor escuchaba solo en `127.0.0.1`, rechazando conexiones entrantes Wi-Fi. Cambiado a `0.0.0.0` (configurable con la variable de entorno `TABLET_MONITOR_LISTEN`). USB sigue funcionando a través del túnel ADB reverse.

### Changed

#### Host (Rust)
- **Encoder nivel H.264**: Añadido `-level 5.1` al encoder NVENC para soportar 1890×1080 @ 60 fps (Level 4.1 limitaba a ~245 MB/s).
- **VBV buffer**: Revertido a `bitrate/4` (250ms) desde `/8` para eliminar artefactos de compresión.
- **Filtro de FPS**: Eliminado el filtro `fps=fps=N` del filtergraph (añadía un FIFO de ~16ms de latencia). Reemplazado con `-r {fps}` en el output + `fps_mode cfr`.
- **Dirección de escucha**: Por defecto `0.0.0.0` en lugar de `127.0.0.1`.

#### Android Client
- **Render path**: Reemplazado `TextureView` por `SurfaceView` para aprovechar el overlay HWC directo (sin copia GPU intermedia), reduciendo latencia y uso de CPU.
- **Drain thread**: Cambiado de `HandlerThread+postDelayed(4ms)` a thread dedicado con `dequeueOutputBuffer(4000µs)` bloqueante + prioridad `THREAD_PRIORITY_URGENT_DISPLAY`.
- **Submit thread**: Prioridad `THREAD_PRIORITY_URGENT_AUDIO`, `nalQueue.take()` bloqueante en lugar de `poll(1ms)` para wakeup instantáneo.
- **MediaFormat**: Añadidos `KEY_PRIORITY=0` y `KEY_OPERATING_RATE=60f` (float, no int) para que el codec reserve el máximo throughput del hardware.
- **HUD / FPS display**: Suavizado EMA (alpha=0.25) sobre ventana de 1 segundo para evitar jitter visual en el contador.

### Added

#### Android Client
- **Auto-reconexión**: Backoff exponencial automático al perder la conexión (1s → 2s → 4s → 8s → 16s → 30s máximo).
- **Selección de display**: Campo `Display` en la UI; se pasa como `?display=N` → `ddagrab=N` en FFmpeg para seleccionar el monitor a capturar.
- **Campo IP del servidor**: Campo `PC IP` reemplaza la sala de señalización; acepta `127.0.0.1` (USB vía ADB) o una IP LAN para Wi-Fi.
- **Manejo de orientación**: `onConfigurationChanged` cierra el socket y programa reconexión automática al rotar el dispositivo, evitando el restart de Activity.
- **Tema oscuro UI**: `#121212` / `#1E1E1E` con texto blanco, compatible con el `windowBackground=transparent` necesario para el SurfaceView.

### Technical Details (updated)
- **Latencia medida**: 14–18 ms (USB)
- **FPS**: 60 estables (Level 5.1 + KEY_OPERATING_RATE)
- **Wi-Fi**: Funcional; servidor escucha en `0.0.0.0:9001`
- **USB**: ADB reverse tunnel `tcp:9001 → tcp:9001` sin cambios

---

## [1.0.0] - 2026-04-14

### Added

#### Host (Rust)
- ✨ H.264 hardware encoding con FFmpeg
- 🎬 Soporte para múltiples encoders GPU: h264_nvenc, h264_qsv, h264_amf
- 📹 Fallback automático a libx264 (software)
- 🔄 WebSocket servidor con Warp
- 📊 GDI screen capture con escalado dinámico
- 🎛️ Query parameters: resolución, FPS, bitrate, fit mode
- 📝 Logging con tracing

#### Android Client
- 📱 MediaCodec hardware H.264 decoder
- 🎨 SurfaceView para renderizado de video
- 📡 WebSocket client con OkHttp3
- 🔍 Annex-B H.264 NAL unit parser
- 🔗 Signaling con room-based coordination
- 📊 Logs en tiempo real en UI
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

## Notas de Versión Futura

### [1.1.0] - Roadmap

- [ ] Audio streaming (AAC codec)
- [ ] WiFi direct support (fallback)
- [ ] Cursor tracking overlay
- [ ] Mouse/keyboard input from tablet
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
- [ ] Web client (browser-based tablet)
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
