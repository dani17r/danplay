# El núcleo, de Python a Rust

Por qué, en una frase: el núcleo Python (35.000 líneas) se arrastraba en
cada paquete (PyInstaller, unos 150 MB en Windows) y en cada compilación, y
lo lento de verdad (leer etiquetas, recorrer carpetas, buscar) iba más
rápido en Rust. Rust ya era el segundo lenguaje (la app, el motor de audio,
el celular), así que no se sumaba ninguno.

La migración acabó en la 1.40.0: la app no lleva Python desde la fase 4, y
desde la fase 5 tampoco el repositorio (abajo, «La fase 5: fuera Python»).
La fase 6 quitó del código de Rust lo que imitaba a Python para poder
compararse con él (abajo, «La fase 6: Rust de verdad»). Lo que sigue cuenta
cómo se hizo.

## Cómo se hizo: de dentro afuera

La lógica pasaba al crate `library/` (Rust puro, sin saber nada de Python)
o a `analysis/` (lo numérico y ONNX), y el módulo de Python se quedaba como
una puerta fina con **la misma API**: llamaba a `danplay_core` (el crate
PyO3 de `core/`). Así las pruebas de Python (unas 1.700) seguían siendo el
testigo de que nada cambiaba, y además cada módulo se comparó con su versión
anterior (`git show <commit>:danplay/<módulo>.py`) en miles de casos, con
datos de verdad en solo lectura.

Lo que hizo posible ir módulo a módulo:

- **Una sola conexión al índice.** `library.connect()` devolvía la
  conexión de Rust (`core/src/db.rs`), que imitaba lo que se usaba de
  `sqlite3` (filas por nombre, `with`, las transacciones implícitas, las
  mismas excepciones). Lo de Python y lo de Rust escribían en la misma
  transacción: `library::index::begin` antes de escribir, como hacía
  `sqlite3`.
- **Las reglas de Python, escritas en Rust** (`library/src/py.rs`):
  `os.path` de POSIX y de Windows, `fnmatch`, `round`, `repr`, `float()`,
  `int()`, `str()`, `json.dumps` (con `indent` y `sort_keys`), `casefold`,
  la fecha de `os.stat`... `tests/test_compat.py` las comparaba con las de
  verdad. Sin esto, un `round(2.675, 2)` o una fecha con el último bit
  distinto bastaban para que el escaneo releyera toda la biblioteca. La fase
  6 las quitó: de lo que importaba para lo guardado se quedó la regla, con
  nombre propio (`num::round_to`, `paths::mtime`); lo demás pasó a las
  reglas de Rust.
- **Lo que tocaba fuera se quedó en Python mientras hizo falta**: la red
  (las pruebas la cortaban y la sustituían en Python), los hilos, los
  cerrojos y lo que aún no estaba en Rust. Rust recibía la función que pide
  una página, o Python orquestaba alrededor de la llamada.

## Dónde iba (22 de octubre de 2026)

En Rust (con su puerta de Python):

| Fase | Qué |
|---|---|
| 1 | el índice entero (conexión, esquema y migraciones, búsqueda, carpetas, escaneo, canciones y su estudio, «Transformar», historial), listas, lo de fuera, conversaciones, diario, nombres, etiquetas (lofty), configuración (de momento solo comparada con la de Python) |
| 1 | convertir, forma de onda, miniaturas, huella, duplicados |
| 2 | pulso (Beat This! con `ort`), clic, pesos de PyTorch, separar en pistas (Demucs y el Mega53), las pistas separadas, el paquete del celular, las secuencias, la estructura y la guía automática, las guías habladas y sus voces (Supertonic y Piper), la rejilla del clic (lo guardado, leído y escrito como el `json` de Python, y re-analizar) |
| 3 | teoría, cifrados, TOON, los datos de fuera (`chat/safe`), la guía hablada, la web, los proveedores de IA, su catálogo y su cliente, leer listas pegadas, emparejar con la biblioteca, la narración, lo que se le cuenta al modelo en cada turno, la lista contra un repertorio, las confirmaciones con su plan de descarga, las herramientas del asistente y su conversación (el turno entero); de YouTube y yt-dlp, lo que se le pide, lo que contesta, lo bajado, las versiones, el motor de JavaScript y descomprimir la rueda; enriquecer una canción (la letra de LRCLIB, la carátula, la ficha de la IA, su cifrado, transponerlo y rellenar la ficha) e importar la Entrada (la cascada, el nombre, la carpeta y archivar) |

Ese día, dentro del núcleo de Python (que desde la fase 4 ya no viajaba en
la app: seguía en el repositorio para sus pruebas y los diferenciales), aún
eran de Python: la API (`api/`) y sus trabajos en segundo plano, la
cola de la rejilla del clic (`beatgrid.py`: su hilo, el proceso del detector
y la batería separada de cada canción), lo del asistente que orquesta
alrededor de Rust: el registro de las confirmaciones (`chat/plan.py`: su
cerrojo, sus topes y los hilos que buscan en YouTube), la validación de los
argumentos de las herramientas (pydantic, en `chat/tools.py`) y, del turno
(`chat/loop.py`), las llamadas a la IA y al juez, leer la lista pegada, lo
que se apunta para el visto bueno y el uso del mes; de YouTube, lo que habla
con yt-dlp en Python (su API, ponerlo al día, el turno de las descargas);
la red de enriquecer (`enrich._get`, con urllib), el `Result` de importar y
lo que toma de `config`; el vigilante de carpetas, la línea de órdenes, y lo
que orquesta alrededor de Rust (los hilos, los cerrojos, la red).

## Los nombres, en inglés

Todo lo de dentro va en inglés (carpetas, crates, módulos, funciones, tipos,
variables, las pruebas); en castellano, los comentarios, los mensajes y lo
que ve quien usa la app. Lo que ya estaba guardado con nombre en castellano
(carpetas de los datos, los manifiestos de las pistas, el diario, las
preferencias de la interfaz) se renombra al arrancar y lo de antes se sigue
leyendo. No cambian: el `.danpista` (`pista.json`, `estudio.json`,
`guias/`: lo leen celulares con la app de antes), los valores guardados en
los datos (la calidad de las pistas `rapida`/`mejor`/`maxima`, los sonidos
del clic, el modelo `secuencia`), los estados del plan del asistente, los
identificadores de las apps (`local.danplay.app`, `local.danplay.pistas`) y
el canal de notificaciones de Android.

## La fase 4: la app sin Python

El plan, para cuando todo lo de arriba estuviera en Rust:

1. **Un enrutador en Rust** que conteste lo mismo que la API de FastAPI
   (las mismas rutas, los mismos cuerpos, los errores con `detail` y los 422
   de validación), con los trabajos en segundo plano (`api/jobs.py`).
2. **En el mismo proceso que la app**: todo pasa ya por un solo punto,
   `core::request_within` en `desktop/src-tauri/src/core.rs` (el puente
   `api`, las carátulas, la cola, abrir archivos). Ese punto deja de hablar
   por el socket y llama al enrutador. Sin proceso aparte, sin socket, sin
   token, sin latido.
3. **Las pruebas de la API** contra el enrutador de Rust: un adaptador ASGI
   que pase cada petición del `TestClient` a `danplay_core`, con la
   configuración de Python de cada prueba. Las que sustituyen funciones de
   Python (clientes de IA falsos, la papelera...) necesitan que esas
   fronteras sean inyectables también desde Rust. Ya son rasgos las del
   asistente (`plan::Index`, `tools::Io`, `conversation::Chat`) y las de
   enriquecer e importar (`library::enrich::Outside`,
   `library::ingest::Outside`): entonces las daba Python; faltaba la
   versión de Rust de cada uno.
4. **Los paquetes sin Python**: fuera PyInstaller y el núcleo empaquetado;
   la app lleva `libonnxruntime` (entonces salía del paquete `onnxruntime`) y
   yt-dlp como su ejecutable oficial, como ffmpeg y QuickJS. Los procesos
   aparte (pulso, separar) pasan a ser la propia app con un argumento.
5. **La CI**: los paquetes sin `uv` ni pruebas de Python; las pruebas de
   Python, solo en `tests.yml`.

### Cómo fue (23 de octubre de 2026)

El enrutador ya existe: el crate `server/` (`danplay-server`). Contesta en
el mismo proceso (`server::handle(&app, &request)`, lo que hace la app) o
por un socket Unix o loopback con hyper (el binario `danplay-server`, con
`--uds` o `--host` y `--port`, como el núcleo empaquetado de Python de
hasta la 1.30: para las pruebas de la interfaz y, mientras hubo Python, los
diferenciales). Cómo se escribe una ruta, cómo se
comprueba lo que llega (lo de pydantic y su 422), los errores, los trabajos
y el estado: en el comentario de `server/src/lib.rs`.

- **Portadas**: `system.py`, `playlists.py`, `external.py` y `library.py`,
  con lo que hacían los módulos de Python a su alrededor (`server/src/services/`:
  las listas con su cerrojo y sus etiquetas, el diario, el escaneo con todo
  lo de después, convertir, los duplicados y la papelera, importar la
  Entrada con su `Outside` en Rust, el perfil de IA y `library::ai::call`).
- **Comparadas** con el núcleo de Python: dos cajas de arena hechas con la
  misma biblioteca (mp3 generados y canciones copiadas), las mismas
  peticiones a las dos (también los 404, 405, 409, 422 y los cuerpos rotos),
  y los códigos, los JSON y los archivos exportados iguales en 170 de 170
  (solo se normalizan la ruta de cada caja, las horas y `revision`). La
  guardia por loopback (Host, `X-DanPlay`, el tope, el token, el CORS de
  Vite), igual en 13 de 14: la que falta es el audio, de `songs.py`.
- **En la app, siempre** (los puntos 2 y 4): `core::request_within` le pide
  la API al núcleo de Rust en el mismo proceso. Fuera el núcleo aparte
  (lanzarlo, el socket, el TCP con su token, el latido que lo revivía o lo
  reiniciaba, `sidecar()`, `from_source()`, `DANPLAY_CORE` y
  `DANPLAY_RUST_CORE`) y lo que solo servía para hablarle (hyper,
  hyperlocal, libc, win32job). La vigilancia que queda solo mira
  `/api/status` para los avisos de la interfaz (`danplay://core` y
  `danplay://changed`), y al arrancar deja en el registro dónde encontró
  sus datos, ONNX Runtime y cada programa. La app ya no lanzaba nunca el
  núcleo de Python, que siguió en el repositorio hasta la fase 5 para sus
  pruebas y los diferenciales. Las pruebas de la interfaz, contra el de
  Rust: `vp run e2e` en `desktop/`, con el `danplay-server` de `target/` (o
  el que diga `DANPLAY_E2E_CORE`).
- **Los paquetes, sin Python** (los puntos 4 y 5): en vez de `core/` (el
  núcleo de PyInstaller), un recurso `data/` con lo de `danplay/data` (desde
  la fase 5, `data/` en la raíz: el pulso, el separador, las voces, la foto
  del catálogo) y ONNX Runtime, la biblioteca oficial de
  github.com/microsoft/onnxruntime (la 1.31.0; en los Mac con Intel, la
  1.23.2, la última que hay para ellos: `ort` pide la API 23), con su sha256
  fijado y en la caché `~/.cache/danplay-<sistema>/` (`scripts/data.sh`; en
  Windows, PowerShell en `build-windows.ps1` y `windows.yml`). yt-dlp, su
  ejecutable oficial (2026.08.19), en `tools/` con ffmpeg, fpcalc y QuickJS.
  `services::onnx` la encuentra donde la deja cada paquete
  (`usr/lib/DanPlay/data` en el .deb y el AppImage, `Contents/Resources/data`
  en macOS, junto al ejecutable en Windows), con su prueba. La foto del
  catálogo de modelos la saca la propia app recién compilada
  (`--models-snapshot SALIDA`, sobre `library::ai::catalog`; sin red viaja la
  del repositorio): por eso la app se compila sin empaquetar (`tauri build
  --no-bundle`) y se empaqueta después (`tauri bundle`). Fuera PyInstaller
  (del grupo dev de pyproject.toml, con `uv.lock` y `requirements-dev.txt` al
  día), `packaging/core.spec`, `packaging/core_entry.py`, el lanzador del
  Python embebido de Windows (`packaging/launcher.c`) y
  `scripts/update-models.py`. Los flujos de Linux, Windows y macOS, sin
  `setup-python`, uv, pip ni pruebas de Python (que entonces solo quedaban en
  `tests.yml`): compilan, sacan la foto, prueban el pulso de una canción con
  la propia app (`--pulse`, con su ONNX Runtime y su ffmpeg) y empaquetan; el
  de Linux comprueba además que el .deb no lleva Python. Probado en Linux:
  `scripts/build.sh --package` y `tests/smoke.sh` contra el .deb extraído y
  contra el AppImage (18 de 18 cada uno, en su caja de arena: el núcleo
  contesta dentro, sin núcleo aparte, socket ni puerto, encuentra ONNX Runtime
  y ffmpeg, saca el pulso, la instancia única y nada vivo al cerrarla).
  Windows y macOS, solo los scripts: salen de GitHub.
- **Las descargas** (`downloads.py`, con el turno, `STATE`, `run_many` y
  `on_done` de `youtube.py` en `services::youtube`, que es también la API de
  Rust para el asistente): yt-dlp ya sin Python, como su ejecutable oficial
  (`services::ytdlp`). Las opciones de la API de Python van como argumentos
  (`--ignore-config` delante), lo que devolvía `extract_info` sale de `-J`,
  lo que recogía el `logger` de los «WARNING:» y «ERROR:» de stderr, y el
  avance de una plantilla `--progress-template` en JSON; cancelar es matar
  el proceso con lo que lanzó (primero por las buenas, para que el de
  PyInstaller borre lo que descomprimió). Ponerlo al día baja de GitHub el
  ejecutable de este sistema (`yt-dlp_linux`...), comprobado con su
  SHA2-256SUMS, a `DATA_DIR/tools/yt-dlp/<versión>/` con su marca `current`.
  Comparadas con el núcleo de Python (un yt-dlp de mentira como módulo en un
  lado, y como ejecutable en el otro detrás del lector de opciones del yt-dlp
  de verdad): 161 de 163 iguales, con lo bajado, sus etiquetas y el
  historial; las dos que no, el paso «consultando PyPI» frente a «consultando
  GitHub». Probadas de verdad: actualizar desde GitHub, consultar, bajar un
  vídeo con licencia Creative Commons y cancelar otro a medias.
- **Las canciones, las pistas y las guías** (`songs.py`, `stems.py`,
  `cues.py`): la rejilla del clic con el detector en su proceso, la cola de
  separar con el separador en el suyo, las voces, la mezcla, el celular y
  las secuencias. Comparadas: 229 de 229 y 98 de 98 iguales.
- **La IA y el asistente** (`ai.py`, `assistant.py`): los rasgos del
  asistente (`plan::Index`, `tools::Io`, `conversation::Chat`) ya tienen su
  versión de Rust (`services/chat/`). Comparados contra un modelo falso
  compatible con OpenAI: 210 de 210 respuestas iguales, y las 127 peticiones
  al modelo, también.
- **Los procesos aparte** (`--pulse`, `--separate`) son el mismo programa con
  un argumento (`server::child_process`): `danplay-server` y la app de
  escritorio, que lleva el núcleo dentro.
- **La app instalada** usa `App::packaged`: los datos que viajan con ella
  (`data/`) y nada del proyecto aunque el código esté en el mismo equipo (lo
  que en Python era `sys.frozen`).
- **El vigilante de carpetas y «aprovechar»** (`watcher.py`, `reuse.py`):
  `notify` (inotify, FSEvents, ReadDirectoryChangesW) en vez de watchdog,
  con los mismos avisos que cuentan y los que no, la misma calma antes de
  escanear y el repaso periódico (`services::watcher`, que arranca y para
  con el núcleo); y lo hecho para otro archivo de la misma canción
  (`services::reuse`): las gemelas, el desfase, las pistas, la rejilla, el
  estudio, las listas de la apartada y la pasada de una vez. El desfase
  (`analysis::align`) hace las cuentas de numpy en sus tipos: contra numpy,
  a 4e-10 s y 7e-7 de parecido en 27 parejas de audio de verdad (todas
  iguales redondeadas como se guardan). Comparados con el núcleo de Python
  en seis escenas (mover una canción, otra descarga con silencio delante o
  sin lo de delante, separar una copia, volver en otro archivo, la pasada de
  una vez y el vigilante viendo llegar, moverse y borrarse canciones y una
  carpeta entera): 196 de 196 iguales, con la base, reuse.json, las
  rejillas, las pistas adoptadas y los archivos de audio byte a byte.
- **Las pruebas de la API de Python, contra el núcleo de Rust**:
  `./scripts/test.sh api` (o pytest con `DANPLAY_API_CORE=rust`) le daba a
  cada cliente un `danplay-server` en caja de arena y sin red, con la
  configuración de Python de esa prueba (`tests/api_core.py`). Pasaban 123;
  se saltaban 36, diciendo por qué, las que sustituían funciones de Python o
  miraban su estado en memoria (`python_core`).
- **Quedaban** esas 36 (el punto 3). Las resolvió la fase 5: al pasar las
  pruebas a Rust, lo que sustituían se sustituye con costuras de Rust solo
  de pruebas (`#[cfg(test)]`).

Medido en la fase 0, lo que se esperaba ganar: unos 150 MB y miles de
archivos menos en el paquete de Windows, varios minutos menos por paquete en
la CI, el arranque sin esperar a que Python importe nada, y menos memoria
(un proceso en vez de dos). Medido ya en Linux, con la misma versión: el
.deb pasa de 166 a 145 MB (instalado, de 357 a 257 MB, y de 334 archivos a
42), y el AppImage, de 251 a 246 MB. Pesa menos de lo que se quita porque
ahora viajan aparte ONNX Runtime (29 MB) y yt-dlp (40 MB, un ejecutable que
lleva su propio Python dentro), y la app lleva el núcleo (de 20 a 40 MB).
Con la 1.40.0, ya sin Python en el repositorio: el .deb, 139 MB, y el
AppImage, 235 MB.

El núcleo, frente al de Python de la 1.30.0 (el mismo equipo, la misma
muestra de 120 canciones copiadas, la mediana de tres pasadas):

| | 1.30.0 | fase 4 |
|---|---|---|
| Arrancar el núcleo | 0,67 s | 0,13 s |
| Memoria en reposo | 66 MB | 30 MB |
| Escanear 120 canciones | 0,19 s | 0,13 s |
| Buscar (mediana / p95) | 2,4 / 3,2 ms | 1,4 / 1,6 ms |
| Ficha de una canción | 1,8 ms | 0,6 ms |
| Carátula | 1,7 ms | 0,7 ms |
| Forma de onda, la primera vez | 365 ms | 358 ms |
| Pulso y «1» de una canción | 18,0 s | 12,7 s |
| Memoria máxima del pulso | 1944 MB | 1658 MB |
| Separar en pistas una canción | 129 s | 107 s |
| Memoria máxima al separar | 3214 MB | 2823 MB |

La app entera en reposo pesa lo mismo (unos 1070 MB, casi todo de WebKit),
pero con un proceso menos.

## La fase 5: fuera Python

Con la app ya sin Python, el núcleo de Python solo quedaba de testigo. La
fase 5 cerró la migración, en la 1.40.0:

1. **Sus pruebas, en Rust.** Las 944 pruebas de Python (359 de la parte del
   servidor A, 137 de la B y 448 de la biblioteca) se pasaron una a una, con
   las mismas comprobaciones y los mismos valores esperados. Las que solo
   tenían sentido en Python se dicen, con su porqué. Prueba a prueba, en
   [TESTS-PORT.md](TESTS-PORT.md). Al pasarlas salieron diferencias de
   verdad, arregladas en Rust: la cola del pulso, que dejaba una canción en
   ningún sitio; `safe::datum`, que dejaba pasar caracteres de formato; un
   «ERROR:» de yt-dlp con color que no se reconocía; una base borrada por
   fuera que se volvía a crear vacía.
2. **Los datos, a `data/`** (b7f888c): lo que viaja con la app (el pulso, el
   separador, las voces y la foto del catálogo de modelos) estaba en
   `danplay/data`.
3. **Fuera Python** (a0e9317): `danplay/` (el núcleo), `core/` (su módulo
   PyO3), `tests/*.py`, `pyproject.toml`, `uv.lock`, los
   `requirements*.txt`, `danplay.sh`, `.basedpyright/` y todos los guiones
   de Python de `scripts/`. Quedan en el historial: `git show
   a0e9317^:<ruta>`.
4. **Los guiones que exportaban los modelos** (`scripts/export-pulse.py`,
   `export-separator.py` y `export-mega53.py`) dependían del núcleo de
   Python, y se fueron con él. Los modelos ya están en `data/`; si un día
   hay que rehacerlos, los guiones están en `a0e9317^`. Igual los que
   medían (`evaluate-pulse.py`, `evaluate-separator.py`,
   `evaluate-chords.py`, `evaluate-laya.py`, `check-metronome.py`) y el banco
   del asistente contra un modelo de verdad (`evaluate-assistant.py`, con
   `tests/evaluation.json`).
5. **Al desarrollar ya no hace falta Python.** `scripts/test.sh` y la CI
   (`tests.yml`) prueban el núcleo con `cargo test` (también en Windows:
   `danplay-library`, `danplay-analysis` y `danplay-server`), y las e2e van
   contra `danplay-server`. `scripts/bump-version.sh` lee la versión del
   `Cargo.toml`. En desarrollo, ONNX Runtime sale de
   `desktop/src-tauri/data` (lo deja `scripts/data.sh`), no del `.venv`. La
   línea de órdenes de Python (`danplay status`, `scan`, `import`…) se fue
   sin sustituta: todo se hace desde la app.

Las pruebas, el día que se fue Python:

| Crate o tanda | Pruebas |
| --- | --- |
| `danplay-library` | 543 |
| `danplay-server` | 541 (538, 2 del binario y 1 de integración, por el socket) |
| `danplay-analysis` | 61 |
| `danplay-engine` | 69 |
| `danplay-app` (la de escritorio) | 102 |
| `danplay-mobile` | 16 |
| La interfaz (Vitest) | 1.288 |
| De punta a punta (Playwright, contra `danplay-server`) | 26 |

## La fase 6: Rust de verdad

Para compararse con Python, el código de Rust había copiado sus reglas: las
rutas de `os.path` (las de POSIX y las de Windows, escritas a mano), el
`json.dumps` con sus espacios, `repr`, `float()`, `int()`, `str()` (que
daba «None» y «True»), las clases de caracteres de `str` (2.905 líneas de
tablas) y hasta sus excepciones (`ValueError`, `KeyError`, `TypeError`...,
con sus mensajes en inglés). Sin Python, eso ya no protegía nada y
estorbaba: el código decía lo que hacía Python en vez de lo que hacía él, y
las rutas de Windows salían de una copia de `ntpath` en vez de las de Rust.
La fase 6 lo quitó (en la rama `fase-6/native-rust`):

1. **Enriquecer e importar**, que llevaban un JSON propio con textos como
   puntos de código, pasan a `serde_json`, `String` y `PathBuf`, y a errores
   de Rust. Fuera `py/json.rs`, `py/value.rs`, `py/text.rs` y
   `services/pyjson.rs`.
2. **Fuera `py` y `parity`**. En su lugar, módulos propios con las reglas de
   Rust: `value` (leer lo que llega en un JSON), `text` (los primeros
   caracteres, el plegado de mayúsculas de Unicode, las líneas), `num`
   (redondear a decimales) y `paths` (las rutas con `std::path`:
   normalizar, absoluta, la de verdad con `canonicalize`, relativa...). El
   JSON se escribe con `serde_json`.
3. **Lo que se guarda se sigue leyendo**. Lo que dejaron las versiones de
   antes puede llevar `NaN` (el tempo de una rejilla de un solo pulso) o un
   nombre que no era UTF-8 (`\udcff`): `lenient_json` lo lee. Lo que se
   compara con lo guardado mantiene su cuenta (la fecha de modificación,
   el nombre de la caché de la forma de onda y de las guías editadas), para
   no releer ni recalcular nada.
4. **El asistente**, sin las tablas de caracteres de Python (las categorías
   de Unicode) y con un error de tres casos en castellano en vez de las
   excepciones de Python. El análisis, la rejilla del clic, YouTube y el
   lector de pesos, igual.
5. **Lo que ya no se usaba**, fuera: la rueda de PyPI de yt-dlp
   (`wheel_zip`, desde la fase 4 viaja el ejecutable oficial), el filtro de
   duración en Rust (lo hace el `--match-filter` de yt-dlp).
6. **Los comentarios** ya no hablan de Python: dicen lo que hace el código y,
   donde importa, por qué (compatibilidad con lo guardado, las cuentas con
   las que se exportaron los modelos).

Por fuera casi nada cambia: el JSON guardado va sin espacios, lo que se le
cuenta a la IA ya no lleva «None», y los mensajes de error que antes
salían en inglés de Python salen en castellano. Al hacerlo salió un fallo de
verdad que hacía fallar a veces las pruebas de YouTube: lanzar un programa
recién escrito podía dar «text file busy»; ahora se reintenta
(`processes::spawn`), lo que también vale para yt-dlp recién puesto al día.
