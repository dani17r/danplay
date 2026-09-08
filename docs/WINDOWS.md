# DanPlay en Windows

Estado a 8 de septiembre de 2026: **hay una versión portátil compilada desde
Linux y un flujo que genera el instalador.** Nadie lo ha ejecutado todavía en
un Windows de verdad, así que hasta el primer arranque hay que tratarlo como
«debería funcionar», no como «funciona».

---

## Las dos formas, y las dos salen desde Linux

```bash
./scripts/build-windows-cross.sh --zip          # portátil
./scripts/build-windows-cross.sh --instalador   # instalador
./scripts/build-windows-cross.sh --zip --instalador   # los dos
```

Salen **de la misma carpeta**, así que no pueden desincronizarse: el
instalador es esa carpeta comprimida con NSIS, más los accesos directos, las
asociaciones de archivo y el desinstalador.

| | Portátil | Instalador |
| --- | --- | --- |
| **Qué es** | una carpeta que se copia y se ejecuta | un `.exe` que instala |
| **Dónde va** | donde lo dejes | `%LOCALAPPDATA%\Programs\DanPlay` |
| **Administrador** | no | **tampoco**: instala para tu usuario, sin aviso de UAC |
| **Accesos directos** | ninguno | menú Inicio y escritorio (se puede desmarcar) |
| **«Abrir con DanPlay»** | lo activa la propia app desde Ajustes | lo deja puesto el instalador |
| **Desinstalador** | borrar la carpeta | sí, y sale en «Aplicaciones instaladas» |
| **ffmpeg y fpcalc** | solo con `--herramientas` | siempre |
| **Tamaño** | 273 MB (98 MB comprimido) con herramientas; 74 MB (25 MB) sin ellas | 71 MB |

De esos 273 MB, **197 son `ffmpeg.exe` y `ffprobe.exe`**, que van enteros
porque son los binarios oficiales. `ffprobe` solo se usa para medir las
portadas antes de encogerlas; si algún día molesta el tamaño, ahí están los
98 MB más fáciles de quitar.

La primera vez descarga a `~/.cache/danplay-cross` lo que necesita —mingw-w64,
zig, NSIS y el Python embebido de Windows, todos extraídos de sus paquetes sin
instalar nada— y luego ya es rápido. Al final comprueba lo que ha salido: que
los dos ejecutables son PE32+, que están el Python y las dependencias, y que
dentro del instalador va todo lo de la carpeta.

### Lo que el instalador NO puede hacer, y por qué

**Ponerse como reproductor predeterminado.** Desde Windows 8 esa elección vive
en una clave del registro (`UserChoice`) protegida con un hash: si un programa
la escribe, Windows lo detecta y la deshace. Lo que sí hace el instalador es
dejar a DanPlay **registrado** —sale en «Abrir con» y en la lista de
aplicaciones predeterminadas del sistema— y ofrecerte al terminar abrir la
página de Ajustes donde das el último clic. Es lo mismo que hacen VLC, Spotify
y foobar2000. Desde la app, ese botón está en Ajustes → «Abrir canciones con
DanPlay».

**Por qué no usa PyInstaller.** Porque PyInstaller no compila para otro
sistema. En su lugar coge el Python embebido oficial de Windows y le mete las
dependencias como ruedas `win_amd64`, que sí se pueden descargar desde
cualquier parte. Un lanzador de treinta líneas
([`packaging/launcher.c`](../packaging/launcher.c)) hace de `danplay-core.exe`
y arranca ese Python; espera a que termine en vez de sustituirse por él, para
que la aplicación siga vigilando un proceso vivo y el Job Object se lleve a
los dos al cerrar.

**Por qué el enlazador de mingw y no el de zig.** Zig sirve para compilar el
lanzador y para preprocesar los recursos, pero su enlazador no sabe usar las
bibliotecas de importación de Windows que trae Rust (`libwindows.0.52.0.a`):
busca un `.dll` y se para. Con el `ld` de mingw enlaza a la primera.

**Lo que la versión portátil no trae.** Ni desinstalador ni accesos directos;
y las asociaciones de archivo no las deja puestas nadie, así que hay que
pedirlas desde Ajustes → «Abrir canciones con DanPlay» (se registran en HKCU,
las mismas claves que escribe el instalador).

## El instalador desde Windows, o desde la integración continua

Sigue existiendo el camino con PyInstaller, que es el que produce un núcleo de
un solo ejecutable en vez del Python embebido:

### Con la integración continua

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

- **macOS.** Sin compilar ni probar. El transporte por socket vale y la
  aplicación ya sale del Dock al esconderse (`ActivationPolicy::Accessory`);
  falta que alguien con un Mac compruebe la bandeja.
- **Reproducir `.opus` y `.wma` sin ffmpeg.** El decodificador no los conoce,
  así que se pasan por ffmpeg. En Windows va dentro del instalador, pero si
  alguien monta la versión portátil sin él, esos dos formatos no sonarán y la
  aplicación lo dirá.
