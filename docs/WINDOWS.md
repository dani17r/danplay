# DanPlay en Windows

Estado a 23 de octubre de 2026: **el portátil y el instalador salen de la
integración continua** (`windows.yml`, en un Windows de GitHub), y desde la
fase 4 de la migración ([MIGRATION.md](MIGRATION.md)) **sin Python**: el
núcleo va dentro de `danplay-app.exe`, como en Linux y macOS. Nadie los ha
ejecutado todavía en un Windows de verdad, así que hasta el primer arranque
hay que tratarlo como «debería funcionar», no como «funciona».

---

## Las dos formas salen de la integración continua

```bash
./scripts/github.sh packages windows   # tras git push: los dos, a dist/installers/
```

Quedan en `dist/installers/` con la versión en el nombre
(`DanPlay-X.Y.Z-windows-portatil.zip` y `DanPlay_X.Y.Z_x64-setup.exe`), junto
al `.deb`, el `.AppImage` y los `.dmg` de Mac.

Salen **de la misma compilación**, así que no pueden ser distintos: el
portátil es la aplicación (con el núcleo dentro), sus datos (`data\`: los
modelos del pulso y del separador, las voces y ONNX Runtime) y sus
herramientas (`tools\`) en un `.zip`; el instalador, lo mismo con NSIS, más
los accesos directos, las asociaciones de archivo y el desinstalador.

También salen **desde Linux** (`scripts/build-windows-cross.sh`): sin Python
ya no hay nada que no se pueda compilar para Windows desde aquí. La app se
compila con mingw-w64 (`x86_64-pc-windows-gnu`), y `data\` y `tools\` se bajan
igual que en la CI; el instalador es el suyo
([`packaging/windows/installer.nsi`](../packaging/windows/installer.nsi)), no
el de Tauri. Viaja la foto del catálogo de modelos del repositorio: la de hoy
la saca la propia app, y un `.exe` no se puede ejecutar en Linux.

| | Portátil | Instalador |
| --- | --- | --- |
| **Qué es** | una carpeta que se copia y se ejecuta | un `.exe` que instala |
| **Dónde va** | donde lo dejes | `%LOCALAPPDATA%\Programs\DanPlay` |
| **Administrador** | no | **tampoco**: instala para tu usuario, sin aviso de UAC |
| **Accesos directos** | ninguno | menú Inicio y escritorio (se puede desmarcar) |
| **«Abrir con DanPlay»** | lo activa la propia app desde Ajustes | lo deja puesto el instalador |
| **Desinstalador** | borrar la carpeta | sí, y sale en «Aplicaciones instaladas» |
| **ffmpeg, fpcalc, QuickJS y yt-dlp** | solo con `--with-tools` | siempre (el de la CI; ver abajo) |
| **Tamaño** | por medir sin Python (en la 1.30, con él: 217 MB comprimido) | por medir (en la 1.30: 191 MB) |

## Actualizar

Volver a ejecutar el instalador nuevo **actualiza**: va a la misma carpeta
(la recuerda del registro), a la misma ficha de «Aplicaciones instaladas»,
quita lo que dejó la versión anterior y copia la nueva. Nunca quedan dos
DanPlay. Si DanPlay está abierto, lo dice y pide cerrarlo (bandeja → Salir).
La biblioteca (`%LOCALAPPDATA%\danplay`) y los ajustes (`%APPDATA%\danplay`)
no se tocan ni al actualizar ni al desinstalar. Con la portátil, se borra la
carpeta vieja y se descomprime la nueva: los datos tampoco viven ahí.

En Linux pasa lo mismo con el `.deb` (mismo paquete, se sustituye en su
sitio; con `sudo dpkg -i` se reinstala aunque la versión no haya cambiado,
mientras que `apt install ./…deb` con la **misma** versión puede no hacer
nada), y el AppImage es un archivo: se sustituye y ya.

Lo que más pesa es `ffmpeg.exe` (100 MB), que va entero porque es el binario
oficial. Dos que iban y ya no: `ffprobe.exe` (otros 100 MB), que solo medía
las portadas antes de encogerlas y la duración de las secuencias, cosas que
salen ahora de la cabecera del propio archivo; y `deno.exe` (93 MB), el motor
de JavaScript de YouTube, que ahora es QuickJS (`qjs.exe`, 2 MB). Deno
resolvía los retos algo antes: con QuickJS cada descarga tarda unos 5
segundos más.

La primera vez descarga a `~/.cache/danplay-windows` lo que necesita
—mingw-w64 y NSIS, extraídos de sus paquetes sin instalar nada; ONNX Runtime
y las herramientas, con su SHA-256 comprobado— y luego ya es rápido. Al final
comprueba lo que ha salido: que la app y `onnxruntime.dll` son PE32+, que
están los datos y las herramientas, que no queda nada de Python y que dentro
del instalador va todo lo de la carpeta.

### Lo que el instalador NO puede hacer, y por qué

**Ponerse como reproductor predeterminado.** Desde Windows 8 esa elección vive
en una clave del registro (`UserChoice`) protegida con un hash: si un programa
la escribe, Windows lo detecta y la deshace. Lo que sí hace el instalador es
dejar a DanPlay **registrado** —sale en «Abrir con» y en la lista de
aplicaciones predeterminadas del sistema— y ofrecerte al terminar abrir la
página de Ajustes donde das el último clic. Es lo mismo que hacen VLC, Spotify
y foobar2000. Desde la app, ese botón está en Ajustes → «Abrir canciones con
DanPlay».

**Por qué mingw-w64.** El motor de audio lleva C++ (el estirado de
Signalsmith) y Rust enlaza para `x86_64-pc-windows-gnu` con las bibliotecas
de importación de Windows que trae (`libwindows.0.52.0.a`): el `ld` y los
compiladores de mingw-w64 lo hacen a la primera. (Hasta la 1.30 hacía falta
además zig, para el lanzador del Python embebido que hacía de núcleo.)

**Lo que la versión portátil no trae.** Ni desinstalador ni accesos directos;
y las asociaciones de archivo no las deja puestas nadie, así que hay que
pedirlas desde Ajustes → «Abrir canciones con DanPlay» (se registran en HKCU,
las mismas claves que escribe el instalador).

## El instalador desde Windows, o desde la integración continua

### Con la integración continua

`.github/workflows/windows.yml` lo hace entero en `windows-latest`, sin
Python:

1. Descarga `ffmpeg` 9.0.2, `fpcalc` 1.6.1, QuickJS-NG 0.17.0 (el motor de
   JavaScript que YouTube exige a yt-dlp) y yt-dlp 2026.08.19 (su ejecutable
   oficial), **con versión fija y comprobando su SHA-256**. Antes se bajaba
   «la última» sin comprobar nada: dos instaladores de la misma versión
   podían llevar un ffmpeg distinto. Para subir una, se cambian la URL y la
   suma en el propio flujo.
2. Los datos: ONNX Runtime 1.31.0 (`onnxruntime.dll`, el oficial de
   Microsoft, también con su SHA-256) y lo de `danplay/data`, en `data\`.
3. Compila la interfaz y la aplicación, sin empaquetar todavía.
4. Con ella misma saca la foto del catálogo de modelos de hoy
   (`--models-snapshot`) y **el pulso de una canción de prueba** (`--pulse`,
   con su ONNX Runtime y su ffmpeg).
5. Genera el instalador NSIS, con las herramientas dentro (salvo que se
   lance sin ellas), y el portátil.
6. Lo deja como artefacto descargable; con una etiqueta `v*`, lo publica.

Las pruebas del núcleo de Python **en Windows** no van ahí: las pasa
`tests.yml` en cada push (el trabajo «núcleo Python en Windows»).

Se lanza a mano desde la pestaña Actions («Run workflow») o subiendo una
etiqueta:

```bash
git tag v1.1.0 && git push origin v1.1.0
```

### En una máquina con Windows

```powershell
.\scripts\build-windows.ps1 -Installer
```

Hace lo mismo que el flujo de arriba. Necesita Node 24, Rust (MSVC) y las
herramientas de compilación de Visual Studio; Python, ya no.

## Qué cambia respecto a Linux

| | Linux y macOS | Windows |
| --- | --- | --- |
| **Cómo hablan la app y el núcleo** | en el mismo proceso: sin socket ni puerto | igual: sin puerto ni secreto (hasta la 1.30, con el núcleo de Python aparte, era un puerto de loopback con un secreto de un solo arranque) |
| **Dónde van los datos** | `~/.local/share/danplay`, `~/.config/danplay` | `%LOCALAPPDATA%\danplay`, `%APPDATA%\danplay` (como decía `platformdirs`) |
| **ONNX Runtime** (el pulso, el separador, las voces) | en `data/`, el de Microsoft para Linux (o para macOS) | en `data\`, `onnxruntime.dll` |
| **ffmpeg, fpcalc, yt-dlp y el motor de JS** | dentro de los paquetes (`tools/`; el `.deb` declara además ffmpeg y fpcalc); para YouTube, Deno, Node ≥ 22 o QuickJS si están | van **dentro** del instalador, en `tools\` (QuickJS y yt-dlp incluidos); la app se los da al núcleo, que los usa antes que los del PATH |
| **Bandeja** | protocolo D-Bus propio (`ksni`), porque la de Tauri no entrega clics | la de Tauri, que ahí sí los entrega y además dice dónde está el icono |
| **Mini reproductor** | lo coloca el escritorio en Wayland | pegado al icono, con `tauri-plugin-positioner` |
| **Mandos del sistema** | MPRIS | SMTC (los dos con `souvlaki`) |

## Por qué ya no hay puerto abierto, tampoco en Windows

Un puerto en `localhost` no es privado: cualquier proceso del equipo puede
hablar con él. Esta API lista tu biblioteca, manda archivos a la papelera y lee
imágenes del disco. Hasta la 1.30, con el núcleo de Python como programa
aparte, en Linux y macOS se usaba un socket Unix con permisos `0600`, y en
Windows (donde `uvicorn` no sabía escuchar en sockets Unix ni en *named
pipes*) un puerto de loopback con un secreto de un solo arranque.

Ahora el núcleo va dentro de la app: la interfaz le habla sin salir del
proceso, y no hay ni socket ni puerto que proteger, en ningún sistema.

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

- **macOS.** Se compila en la integración continua (`./scripts/build-macos.sh`
  la lanza y deja en `dist/installers/` un `.dmg` para Apple Silicon y otro
  para Intel, sin firma de Apple), pero sin probar en un Mac de verdad. El
  núcleo va dentro, como aquí, y la aplicación ya sale del Dock al esconderse
  (`ActivationPolicy::Accessory`); falta que alguien con un Mac compruebe la
  bandeja.
- **Reproducir `.opus` y `.wma` sin ffmpeg.** El decodificador no los conoce,
  así que se pasan por ffmpeg. En Windows va dentro del instalador, pero si
  alguien monta la versión portátil sin él, esos dos formatos no sonarán y la
  aplicación lo dirá.
