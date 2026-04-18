# FlexDisplay — Guía de inicio rápido

> Esta guía está escrita para personas sin conocimientos técnicos.
> Sigue los pasos en orden y no omitas ninguno.

---

## ¿Qué necesitas?

| Elemento | Detalles |
|---|---|
| PC con Windows 10 u 11 | 64 bits, cualquier marca |
| Dispositivo Android (teléfono o tablet) | Android 8 o superior |
| Cable USB **o** red Wi-Fi | Cualquiera de los dos funciona |
| La aplicación FlexDisplay instalada en Android | El archivo `.apk` está incluido en el paquete |

---

## Paso 1 — Descarga el programa

1. Ve a la página de [Releases de FlexDisplay](https://github.com/FitzVB/tablet-second-monitor/releases).
2. Descarga el archivo que termina en `.zip` (por ejemplo `FlexDisplay-v0.1.0-windows-lite.zip`).
3. Haz clic derecho sobre el archivo ZIP → **Extraer todo…**
4. Elige una carpeta fácil de recordar, como `C:\FlexDisplay`.

---

## Paso 2 — Instala la app en el Android

1. En tu dispositivo Android, ve a **Ajustes → Seguridad** (o *Aplicaciones*) y activa **"Fuentes desconocidas"** (o **"Instalar apps desconocidas"**).
   *(Solo necesitas hacer esto una vez.)*
2. Conecta el dispositivo al PC por USB.
3. Copia el archivo `FlexDisplay.apk` (está dentro del ZIP que extrajiste) al dispositivo Android.
4. Abre el archivo `.apk` desde el dispositivo Android y confirma la instalación.

---

## Paso 3 — Instala el monitor virtual *(solo si quieres pantalla extendida)*

> Si solo quieres **duplicar** tu pantalla, puedes saltar este paso.

El **Virtual Display Driver (VDD)** es un programa gratuito que crea una pantalla virtual en Windows.
Esto permite que tu dispositivo Android funcione como un **monitor extra** (pantalla extendida).

### Cómo instalarlo

1. Ve a: **[github.com/VirtualDrivers/Virtual-Display-Driver/releases](https://github.com/VirtualDrivers/Virtual-Display-Driver/releases)**
2. Descarga el instalador más reciente (el archivo `.exe`, por ejemplo `Virtual-Driver-Control-Installer.exe`).
3. Ejecuta el instalador y haz clic en **Install**.
4. Cuando termine, **reinicia el PC** una vez.

### Verifica que funciona
Después de reiniciar, ve a **Configuración → Sistema → Pantalla** y deberías ver una pantalla extra llamada "Virtual Display" o similar.

---

## Paso 4 — Inicia FlexDisplay

1. Abre la carpeta donde extrajiste el programa.
2. Haz doble clic en **`START.bat`**.
3. Se abrirá una ventana negra (terminal) y te preguntará el modo de conexión:
   - Escribe `1` y pulsa **Enter** para **USB** (recomendado, más estable).
   - Escribe `2` y pulsa **Enter** para **Wi-Fi**.

---

## Paso 5 — Conecta el dispositivo Android

### Modo USB

1. Conecta el dispositivo Android al PC con el cable USB.
2. En el dispositivo Android, aparecerá una ventana que dice **"¿Permitir depuración USB?"** → toca **Aceptar**.
3. El programa configura todo automáticamente.
4. Abre la app **FlexDisplay** en el dispositivo Android.
5. La pantalla debería aparecer de inmediato. Si no aparece, toca el botón **Conectar**.

### Modo Wi-Fi

1. Asegúrate de que el PC y el dispositivo Android están en la **misma red Wi-Fi**.
2. Cuando inicies `START.bat` verás un mensaje como:
   `[OK] Wi-Fi host IP detected: 192.168.1.50`
   Ese número es la IP de tu PC.
3. Abre la app **FlexDisplay** en el dispositivo Android.
4. Escribe esa IP en el campo de dirección y toca **Conectar**.

---

## Paso 6 — Elige el modo de pantalla

Dentro de la app FlexDisplay en el dispositivo Android, verás dos opciones:

| Modo | ¿Cuándo usarlo? |
|---|---|
| **Mirror** (duplicar) | Ver la misma imagen que tu PC en el dispositivo Android |
| **Extended** (extendida) | Usar el dispositivo Android como segundo monitor independiente |

Para el modo **Extended** necesitas tener instalado el VDD (Paso 3).

---

## Cómo cerrar el programa

Cierra la ventana negra (terminal) que se abrió al ejecutar `START.bat`.
El programa se cerrará automáticamente y limpiará todos los procesos en segundo plano.

También puedes hacer doble clic en **`STOP.bat`** (dentro de la carpeta `scripts`) para detenerlo sin cerrar la terminal.

---

## Problemas comunes

### El dispositivo Android no aparece en el PC (modo USB)
- Asegúrate de que el cable USB transfiere datos (no solo carga).
- Ve a **Ajustes → Opciones de desarrollador** en el dispositivo Android y activa **Depuración USB**.
- Acepta el mensaje de autorización en la pantalla del dispositivo Android.

### La app muestra "No se puede conectar" (modo Wi-Fi)
- Verifica que el PC y el dispositivo Android están en la **misma red Wi-Fi**.
- Comprueba que el número IP en la app es exactamente el que mostró el programa al iniciar.
- Desactiva temporalmente el antivirus o firewall del PC para probar.

### La pantalla extendida no aparece en Windows
- Abre **Configuración → Sistema → Pantalla** y haz clic en **Detectar**.
- Si no aparece, reinicia el PC después de instalar el VDD.

### El video se ve cortado o de mala calidad
- Reduce la resolución en la app (prueba con 1280×720).
- En modo Wi-Fi, acerca el dispositivo al router.

### Windows bloqueó el script `.ps1`
Usa **`START_SAFE.bat`** en lugar de `START.bat`. Este modo evita los scripts de PowerShell.

---

## Desinstalar

1. Borra la carpeta donde extrajiste FlexDisplay.
2. Desinstala la app FlexDisplay desde el dispositivo Android (**Ajustes → Aplicaciones**).
3. Si instalaste el VDD y ya no lo necesitas, ábrelo desde el menú de inicio o desde **Panel de control → Programas** y desinstálalo.
