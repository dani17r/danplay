# Contribuir

Gracias por mirar. Aquí está lo que hace falta para montar el entorno, cómo se
prueban las cosas y en qué se puede ayudar.

> **Antes de nada, lee la [licencia](../LICENSE).** Este proyecto es *código a
> la vista*, no código abierto: puedes usar la app y leer el código, pero no
> mantener tu propia versión aparte ni distribuir copias modificadas.
>
> Eso **no impide** proponer cambios sobre este repositorio, y se agradecen.
> Al mandar un pull request cedes los derechos de esa aportación al autor,
> para que pueda seguir bajo la misma licencia que el resto del proyecto. Si
> eso no te encaja, abre un issue describiendo la idea en vez de mandar código.

---

## Montar el entorno

```bash
git clone https://github.com/dani17r/danplay.git && cd danplay

cd desktop && npm ci && cd ..
cp .env.example .env

# solo para las pruebas del núcleo de Python (la app ya no lo lleva): el
# .venv con TODO lo del cerrojo (uv.lock), con las pruebas, las
# herramientas de estilo y tipos y el crate de Rust que usa Python,
# compilado y editable
uv sync
```

La app y su núcleo son Rust: con Node 24 y Rust basta para compilarla y
probarla. El núcleo de Python (`danplay/`) sigue en el repositorio de
testigo: sus pruebas y los diferenciales comparan el de Rust con él.

Las dependencias de Python se gestionan con [uv](https://docs.astral.sh/uv/):
`pyproject.toml` dice qué hace falta, en rangos, y `uv.lock` fija la versión
exacta de todo, también de lo que arrastran. Añadir o subir una:
`uv add paquete` / `uv lock --upgrade-package paquete`, y luego
`.venv/bin/python scripts/export-requirements.py`, que regenera los
`requirements*.txt` (con hashes) para quien use pip:

```bash
pip install -r requirements.txt                          # solo ejecutarlo
pip install -r requirements.txt -r requirements-test.txt # y probarlo, sin Rust
pip install -r requirements-dev.txt                      # todo, como uv sync
```

Los grafos del separador de pistas (`danplay/data/separator/`) se generan
con `scripts/export-separator.py`, que necesita PyTorch y Demucs (la app
no: usa ONNX Runtime). Solo hace falta si cambia la red o el modo de
exportarla; el propio script explica cómo ejecutarlo en un entorno aparte y
comprueba que el grafo da lo mismo que el modelo original (y que el de
htdemucs, con los pesos de cada especialista, da lo mismo que ese
especialista). Qué redes usa la app y qué pistas se quedan se decidió
midiendo con `scripts/evaluate-separator.py` (MUSDB18, sin PyTorch): antes de
cambiar algo del separador, que lo nuevo gane ahí.

Opcional, para la huella acústica y la conversión de formatos:

```bash
sudo apt install libchromaprint-tools ffmpeg
```

Sin ellos la app funciona igual, solo que con esas dos funciones desactivadas
y diciéndolo en Ajustes.

## Ejecutar

Mientras desarrollas, la app entera en modo desarrollo:

```bash
./scripts/dev.sh                    # con tus datos de siempre
./scripts/dev.sh --sandbox ~/Musica # con datos aparte y una COPIA de esas canciones
```

Es la app de escritorio de verdad (Tauri) con la interfaz servida por Vite:
lo que cambias en `desktop/src` se ve al momento, y el núcleo, que va dentro,
se compila del código del proyecto (`server/`, `library/`, `analysis/`) con
los datos de `danplay/data`: un cambio en Rust se ve cuando `tauri dev` la
vuelve a compilar, sola, al guardar. La compilación de desarrollo usa su
propio identificador y su nombre en MPRIS y en la bandeja: convive con
DanPlay instalado sin pisarle nada.

`--sandbox` guarda ajustes, base e índice en `.dev/sandbox/` (fuera de git) y
copia allí las canciones que le des: la app escribe etiquetas y la importación
mueve archivos, y así tu música no se toca. Sin carpeta, genera unas
canciones con ffmpeg. `--sandbox --clean` empieza de cero.

En un clon recién hecho, `desktop/src-tauri/data/` y `desktop/src-tauri/tools/`
solo traen su LEEME: `./scripts/dev.sh` baja ONNX Runtime la primera vez
(`scripts/data.sh`, a una caché) y ffmpeg sale del sistema. Lo que viaja
dentro de los paquetes lo ponen `scripts/data.sh` y `scripts/tools-linux.sh`.

Para la versión que se instala:

```bash
./scripts/build.sh     # la interfaz, los datos y la app, en orden
./danplay-app.sh       # la app de escritorio compilada, con el núcleo dentro
./danplay.sh status    # la línea de comandos del núcleo de Python
```

Y solo la interfaz en el navegador, con el núcleo por un puerto:

```bash
cargo run -p danplay-server                # el de Rust, en el puerto 8730
.venv/bin/python -m danplay.cli serve      # o el de Python, en el mismo
cd desktop && npm run dev                  # Vite en el 5273, con proxy
```

## La app del celular

`mobile/` es la app de Android (e iOS, cuando se compile en un Mac). Su
interfaz es la de la PC (`desktop/mobile.html` → `desktop/src/mobile/`), así
que se prueba con las mismas herramientas: `npm test` en `desktop/` incluye
`tests/mobile.test.js`, y `cargo test -p danplay-engine -p danplay-mobile`
prueba el motor de pistas y lo que la app contesta en vez de Python.

Para compilarla hace falta, en `~/Android`:

- un JDK 17 (`~/Android/jdk-17`);
- el SDK con `platform-tools`, una plataforma, `build-tools` y el NDK 28 o más
  nuevo (`sdkmanager "platform-tools" "platforms;android-36"
  "build-tools;36.1.0" "ndk;29.0.14206865"`); Gradle baja la plataforma que
  pida el proyecto;
- los destinos de Rust: `rustup target add aarch64-linux-android
  armv7-linux-androideabi x86_64-linux-android i686-linux-android`.

```bash
./scripts/build-android.sh             # el APK firmado, a dist/installers/
./scripts/build-android.sh --emulator  # el de depuración, para el emulador
cd mobile && npm run tauri -- android dev   # con el teléfono por USB
```

O sin nada de lo anterior instalado: `./scripts/github.sh packages android`
lo compila y lo firma en GitHub (la llave viaja en dos secretos del
repositorio; cómo se ponen o se cambian, en la cabecera de
`.github/workflows/android.yml`).

La **llave de firma** vive fuera del repositorio, en `~/Android/claves/`
(`danplay-pistas.jks` y `keystore.properties`). Guárdala con las copias de
seguridad: un teléfono solo acepta una actualización firmada con la misma
llave, y si se pierde hay que desinstalar la app en cada teléfono.

Para probarla sin teléfono, un emulador sin ventana y sin sonar por los
altavoces:

```bash
avdmanager create avd -n danplay -k "system-images;android-35;google_apis;x86_64" -d pixel_7
emulator -avd danplay -no-window -gpu swiftshader_indirect -audio none &
adb install -r mobile/gen/android/app/build/outputs/apk/universal/debug/app-universal-debug.apk
```

La versión de depuración deja inspeccionar la página como en Chrome
(`chrome://inspect`, o `adb forward tcp:9222
localabstract:webview_devtools_remote_<pid>` y el protocolo de depuración).

## Las pruebas

```bash
./scripts/test.sh
```

Es lo mismo que comprueba la CI en cada push, y todo tiene que pasar:

| Tanda | Qué cubre |
| --- | --- |
| Núcleo Python | ruff (estilo y formato), basedpyright (tipos) y pytest con cobertura (mínimo 70 %): nombres, etiquetas, índice, API sobre una biblioteca temporal de verdad, IA, descargas, vigilante de carpetas |
| Interfaz | ESLint, stylelint, Prettier, vue-tsc (tipos) y Vitest: componentes, reactividad, contratos, listas grandes |
| Interfaz (URLs) | cómo se construyen las rutas de medios en cada sistema |
| Rust | rustfmt, clippy sin avisos y las pruebas del workspace: hashes, forma de onda, reproductor, cola, bandeja, permisos por ventana |
| e2e | la interfaz de verdad contra el núcleo de verdad, en Chrome (Playwright) |
| Humo | la app **ya compilada** o empaquetada: el núcleo dentro y sin puertos, sus datos y programas, el pulso, la instancia única, el ciclo de vida |

`./scripts/test.sh --quick` se salta las e2e y las de humo.

Si no quieres cargar el equipo, que las corra GitHub: sube el commit
(`git push`) y

```bash
./scripts/github.sh tests            # las de la CI para ese commit; si fallan, enseña qué
./scripts/github.sh packages         # los siete de una versión: .deb, .AppImage, APK, dos .dmg y los dos de Windows
./scripts/github.sh packages linux   # o solo los de un sistema (linux, android, macos, windows)
```

Mientras tanto aquí solo se espera; los paquetes acaban en `dist/installers/`
como si los hubieras construido tú. Si el commit ya tiene una ejecución (la
del push a main, la de la etiqueta de versión), se usa esa en vez de lanzar
otra.

Las pruebas de Python **nunca tocan tus datos**: `tests/conftest.py` pone
HOME y las carpetas XDG en un temporal y corta la red antes de importar nada.

Una tanda suelta, mientras trabajas:

```bash
.venv/bin/python -m pytest                       # todo el núcleo, con cobertura
.venv/bin/python -m pytest tests/test_watch.py --no-cov
cd desktop && npm run check                      # lint + css + formato + pruebas
cd desktop && npm run e2e                        # los flujos en Chrome
PYO3_BUILD_EXTENSION_MODULE=1 cargo test --workspace --release
```

### Cómo se escriben aquí las pruebas

Dos costumbres que conviene seguir:

**Una prueba nueva tiene que fallar con el código viejo.** Si no falla, no está
comprobando nada. La forma rápida de verificarlo es revertir el arreglo un
momento y ver que se pone en rojo.

**Se prueba el comportamiento, no cómo está hecho.** Da igual cómo se ponga al
día el índice mientras el resultado sea el correcto. Así las pruebas sobreviven
a que se cambie la implementación por dentro, que es justo para lo que están.

**La biblioteca de prueba se genera.** `tests/conftest.py` hace mp3 de verdad
con ffmpeg (un segundo de silencio, con sus etiquetas), así que las pruebas
comprueban lo mismo en cualquier máquina. Antes dependían de la música de
quien las ejecutara y la mitad se saltaban solas.

**Los dobles se construyen a partir del código real.** El de la API
(`desktop/tests/support/backend.js`) recorre el `api` de verdad, así que un
método que la interfaz llame y la prueba no haya programado falla diciendo
cuál es. Cinco fallos de la interfaz eran nombres que dejaron de existir al
pasar el código a inglés y que nadie comparó con el núcleo.

### El banco de pruebas contra un modelo de verdad

Las pruebas de arriba usan un modelo falso: comprueban la mecánica del
asistente (ids que no existen se rechazan, lo destructivo se confirma, la
narración se detecta), no que un modelo real siga el prompt. Para eso está

```bash
./.venv/bin/python scripts/evaluate-assistant.py                 # el proveedor activo
./.venv/bin/python scripts/evaluate-assistant.py --proveedor llm7 --repetir 3
```

Monta una biblioteca sintética, corre las conversaciones de
`tests/evaluation.json` contra el proveedor configurado en la app y dice qué
cumplió cada una (herramientas usadas, lo que quedó en la biblioteca, lo que
dijo), lo que tardó y lo que costó; con `--repetir` se ve lo estable que es.
Cuesta dinero (céntimos) y red, así que no va en `scripts/test.sh`: se lanza
a mano **cada vez que se toca el prompt, las herramientas o se cambia de
modelo**, y antes de subir versión. La primera pasada encontró dos fallos de
la búsqueda (`Barak - Mi Gozo` devolvía cero por el guion; `titulo:` no
existía como filtro) y una manía del modelo (escribir la llamada como texto)
que ninguna prueba con dobles habría visto.

## La versión

**Cada cambio que se entrega sube la versión.** Sin excepción: una corrección
pequeña, una función nueva, un cambio que rompe algo. Es la regla de la casa
porque, sin subirla, un `.deb` con el mismo número no se reinstala con
`apt install`, Windows enseña el mismo número antes y después, y nadie puede
saber qué DanPlay tiene delante.

Cuánto se sube lo decide quien hace el cambio, con versionado semántico
`MAYOR.MENOR.PARCHE`:

| Sube | Cuándo | Ejemplo |
| --- | --- | --- |
| **PARCHE** (1.2.0 → 1.2.1) | arregla algo sin cambiar lo que la app hace | el reproductor se quedaba mudo; el asistente narraba sin actuar |
| **MENOR** (1.2.0 → 1.3.0) | añade algo que antes no se podía hacer | enviar por Telegram, selección múltiple, renombrar listas |
| **MAYOR** (1.2.0 → 2.0.0) | cambia algo de forma incompatible: formato de la base, del `.desktop`, de la API interna, de las etiquetas que escribe | pasar el índice a otro esquema sin migración |

Si un mismo bloque de cambios mezcla arreglos y funciones, manda el mayor.

La versión se escribe en tres sitios (`danplay/__init__.py`, el `Cargo.toml`
de la raíz, que heredan los dos crates y del que la toman Tauri y maturin, y
`desktop/package.json`) y no se toca a mano:

```bash
./scripts/bump-version.sh 1.3.0
```

Eso los deja iguales, con los cerrojos (Cargo, npm y uv) y la insignia del
README. Después, `./scripts/test.sh --quick` —hay una prueba que falla si
alguno se queda atrás—, un **commit propio** (`chore: subir a 1.3.0`, sin
mezclarlo con el cambio) y, para publicarla, una etiqueta:
`git tag v1.3.0 && git push --tags`. Con ella la CI construye los siete
paquetes (el `.deb`, el `.AppImage`, el APK, los dos `.dmg` y el instalador y
el portátil de Windows) y los deja en esa versión de GitHub: no hace falta
compilar nada en tu equipo. Si además los quieres en `dist/installers/`,
`./scripts/github.sh packages` espera a esas mismas ejecuciones y te los baja.

## Estilo

- El **código** en inglés: carpetas, archivos, funciones, variables, rutas de
  la API y columnas de la base.
- Los **comentarios** y todo lo que ve el usuario, en castellano.
- Los comentarios explican **por qué**, no qué. Si algo está hecho de una forma
  rara, el comentario cuenta qué pasó cuando se hizo de la forma normal.
- Sin acentos en nombres de archivo generados. La ñ se conserva.
- El formato no se discute: lo ponen `ruff format` (Python), `cargo fmt`
  (Rust) y Prettier (interfaz), y la CI falla si algo no está formateado.
  `.editorconfig` hace que cualquier editor empiece bien.

## Dónde se puede ayudar

**Detección de tonalidad.** Es la debilidad conocida: 2 aciertos de 8. Se
probaron 60 combinaciones de perfiles y ponderaciones (Krumhansl, Temperley,
pesos por banda) y todas se estancan ahí. El código está en
[`core/src/audio.rs`](../core/src/audio.rs). Si sabes de esto, es donde más
falta hace.

**Windows.** Ya se compila entero desde Linux: `./scripts/build-windows-cross.sh
--zip --installer` saca la versión portátil **y** el instalador. Pero **nadie
lo ha ejecutado en un Windows de verdad**. Falta comprobar lo que solo se ve
al abrirlo: que suene (WASAPI), que las rutas con acentos no rompan el índice,
que la papelera reciba los archivos, que el clic en el icono abra la
ventanita, que las teclas multimedia funcionen, y que el instalador deje los
accesos directos y el «Abrir con» donde debe. Los detalles están en
[Windows](WINDOWS.md).

**macOS.** Sin probar. El núcleo va dentro, como en Linux, y la
política de activación ya está puesta (`tray::dock`: al esconderse pasa a
`Accessory` y sale del Dock, al volver a `Regular`). El paquete sale de la
integración continua: `./scripts/build-macos.sh` lanza `macos.yml`, espera y
deja en `dist/installers/` un `.dmg` para Apple Silicon y otro para Intel, con
ffmpeg, fpcalc, QuickJS, yt-dlp y ONNX Runtime dentro (en los Intel, la 1.23.2,
la última que Microsoft publica para ellos) y firmados solo «ad hoc» (la ejecución guarda
además la `.app` en `.zip`). Pero **nadie lo ha abierto todavía en un Mac
de verdad**. Falta ver si la bandeja de Tauri entrega el clic como en
Windows, si la ventanita queda bien pegada al icono de la barra y si el
cambio de política no deja la aplicación sin foco al reaparecer.

**Traducción.** Todo lo que ve el usuario está en castellano, escrito a mano.
No hay sistema de idiomas.

## Antes de abrir un pull request

- Que `./scripts/test.sh` pase entero.
- Que lo que arregles tenga una prueba que falle sin el arreglo.
- Si cambias algo de rendimiento, mide y di los números. «Va más rápido» no
  vale; «de 890 ms a 185 ms con 20.000 canciones» sí.
- Si algo se queda a medias o tiene una pega, dilo en la descripción. Es más
  útil que descubrirlo después.
- Nada de añadir dependencias con copyleft fuerte (GPL, AGPL) a lo que viaja
  en los paquetes. `mutagen` lo era, y por eso las etiquetas pasaron a Rust
  (lofty); ver [DEPENDENCIES.md](DEPENDENCIES.md).

## Informar de un fallo

Cuenta qué esperabas, qué pasó, y cómo repetirlo. Ayuda mucho:

```bash
./danplay.sh status     # configuración y estado, sin claves
```

Si es un fallo de identificación, el nombre del archivo que lo provoca vale
más que cualquier descripción.
