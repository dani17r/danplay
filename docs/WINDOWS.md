# DanPlay en Windows

Estado a 8 de septiembre de 2026: **el código está listo y compila para
Windows; el instalador lo produce la integración continua.** Nadie lo ha
ejecutado todavía en un Windows de verdad, así que hasta el primer arranque
hay que tratarlo como «debería funcionar», no como «funciona».

---

## Por qué no se puede compilar entero desde Linux

Una sola razón, y no tiene vuelta: **PyInstaller no compila para otro
sistema.** El núcleo Python (`danplay-core.exe`) tiene que empaquetarse
ejecutando PyInstaller **en Windows**. No hay opción de cruzar.

Lo demás sí se podría: la interfaz es JavaScript (igual en todas partes) y la
aplicación de Rust se puede compilar cruzada. Pero una aplicación sin núcleo
arranca, no encuentra con quién hablar y enseña un error. No sirve de nada.

Lo que **sí** se hace desde Linux, y se hizo:

```bash
cargo check --target x86_64-pc-windows-msvc      # o -gnu
```

Eso comprueba de verdad el código de Windows —los bloques `#[cfg(windows)]`
que en Linux nunca se compilan— y sale limpio. Hace falta un compilador de
recursos (`llvm-rc` para MSVC, `x86_64-w64-mingw32-windres` para GNU); sin él,
el script de compilación de Tauri se para antes de llegar al código.

Enlazar un `.exe` desde Linux es otra historia: Tauri no da soporte al destino
`-gnu`, y el `-msvc` necesita las bibliotecas de Microsoft. No merece la pena
pelearlo cuando de todos modos faltaría el núcleo.

## Cómo se consigue el instalador

### Con la integración continua (lo normal)

`.github/workflows/windows.yml` lo hace entero en `windows-latest`:

1. Compila la interfaz.
2. Ejecuta las pruebas del núcleo **en Windows**.
3. Empaqueta el núcleo con PyInstaller y **comprueba que arranca y contesta**,
   y que sin el token no contesta.
4. Descarga `ffmpeg` y `fpcalc` y los mete dentro.
5. Compila la aplicación y genera el instalador NSIS.
6. Lo deja como artefacto descargable; con una etiqueta `v*`, lo publica.

Se lanza a mano desde la pestaña Actions («Run workflow») o subiendo una
etiqueta:

```bash
git tag v1.1.0 && git push origin v1.1.0
```

### En una máquina con Windows

```powershell
.\scripts\build-windows.ps1 -Instalador
```

Hace lo mismo que el flujo de arriba. Necesita Python 3.13, Node 24, Rust
(MSVC) y las herramientas de compilación de Visual Studio.

## Qué cambia respecto a Linux

| | Linux y macOS | Windows |
| --- | --- | --- |
| **Cómo hablan la app y el núcleo** | socket Unix `0600` en una carpeta privada; sin puerto | `127.0.0.1` con un puerto libre y un secreto de un solo arranque (`DANPLAY_TOKEN`). Sin él, la API responde `401` |
| **Que el núcleo muera con la app** | `PR_SET_PDEATHSIG` | Job Object con `KILL_ON_JOB_CLOSE` |
| **Dónde van los datos** | `~/.local/share/danplay`, `~/.config/danplay` | `%LOCALAPPDATA%\danplay`, `%APPDATA%\danplay` (lo decide `platformdirs`) |
| **Papelera** | `send2trash`, y `gio trash` si falla | `send2trash` |
| **ffmpeg y fpcalc** | los instala el gestor de paquetes; el `.deb` los declara | van **dentro** del instalador, en `tools\`; la app se lo dice al núcleo con `DANPLAY_TOOLS_DIR` |
| **Bandeja** | protocolo D-Bus propio (`ksni`), porque la de Tauri no entrega clics | la de Tauri, que ahí sí los entrega y además dice dónde está el icono |
| **Mini reproductor** | lo coloca el escritorio en Wayland | pegado al icono, con `tauri-plugin-positioner` |
| **Mandos del sistema** | MPRIS | SMTC (los dos con `souvlaki`) |

## Por qué no hay puerto abierto en Linux y sí en Windows

Un puerto en `localhost` no es privado: cualquier proceso del equipo puede
hablar con él. Esta API lista tu biblioteca, manda archivos a la papelera y lee
imágenes del disco, así que en Linux y macOS se usa un socket Unix con
permisos `0600` y no hay puerto que valga.

En Windows no existe esa opción: `uvicorn` no sabe escuchar en sockets Unix ni
en *named pipes*. Lo más cerca que se puede estar es un puerto de loopback con
un secreto que solo conocen la aplicación y el núcleo, y que viaja por el
entorno (no por la línea de órdenes, que es pública). Otro programa del equipo
puede llegar al puerto, pero sin el secreto se lleva un `401`.

## Lo que falta por comprobar cuando alguien lo ejecute

Cosas que solo se ven al abrirlo de verdad:

- Que el audio suene (WASAPI a través de `cpal`).
- Que la ruta con acentos o con `ñ` no rompa el índice.
- Que la papelera de Windows reciba los archivos.
- Que el icono de la bandeja aparezca y el clic abra el mini reproductor
  pegado a él.
- Que las teclas multimedia funcionen (SMTC).
- Que el instalador no dispare SmartScreen de forma que asuste: **sin
  certificado de firma, Windows avisará** de que el editor es desconocido. Es
  normal en un proyecto personal; se quita comprando un certificado.

## Lo que no está preparado

- **macOS.** El transporte por socket ya vale, pero falta que la aplicación
  desaparezca del Dock al esconderse y probar la bandeja.
- **Reproducir `.opus` y `.wma`** en cualquier sistema: el decodificador no los
  trae. Se organizan igual; para escucharlos hay que convertirlos.
