# El núcleo, de Python a Rust

Por qué, en una frase: el núcleo Python (35.000 líneas) se arrastra en cada
paquete (PyInstaller, unos 150 MB en Windows) y en cada compilación, y lo
lento de verdad (leer etiquetas, recorrer carpetas, buscar) va más rápido en
Rust. Rust ya era el segundo lenguaje (la app, el motor de audio, el
celular), así que no se suma ninguno.

## Cómo se hace: de dentro afuera

La lógica pasa al crate `library/` (Rust puro, sin saber nada de Python)
o a `analysis/` (lo numérico y ONNX), y el módulo de Python se queda como
una puerta fina con **la misma API**: llama a `danplay_core` (el crate
PyO3 de `core/`). Así las pruebas de Python (unas 1.700) siguen siendo el
testigo de que nada cambia, y además cada módulo se compara con su versión
anterior (`git show <commit>:danplay/<módulo>.py`) en miles de casos, con
datos de verdad en solo lectura.

Lo que hace posible ir módulo a módulo:

- **Una sola conexión al índice.** `library.connect()` devuelve la
  conexión de Rust (`core/src/db.rs`), que imita lo que se usaba de
  `sqlite3` (filas por nombre, `with`, las transacciones implícitas, las
  mismas excepciones). Lo de Python y lo de Rust escriben en la misma
  transacción: `library::index::begin` antes de escribir, como hacía
  `sqlite3`.
- **Las reglas de Python, escritas en Rust** (`library/src/py.rs`):
  `os.path` de POSIX y de Windows, `fnmatch`, `round`, `repr`, `float()`,
  `int()`, `str()`, `json.dumps` (con `indent` y `sort_keys`), `casefold`,
  la fecha de `os.stat`... `tests/test_compat.py` las compara con las de
  verdad. Sin esto, un `round(2.675, 2)` o una fecha con el último bit
  distinto bastaban para que el escaneo releyera toda la biblioteca.
- **Lo que toca fuera se queda en Python mientras haga falta**: la red (las
  pruebas la cortan y la sustituyen en Python), los hilos, los cerrojos y lo
  que aún no está en Rust. Rust recibe la función que pide una página, o
  Python orquesta alrededor de la llamada.

## Dónde va (22 de octubre de 2026)

En Rust (con su puerta de Python):

| Fase | Qué |
|---|---|
| 1 | el índice entero (conexión, esquema y migraciones, búsqueda, carpetas, escaneo, canciones y su estudio, «Transformar», historial), listas, lo de fuera, conversaciones, diario, nombres, etiquetas (lofty), configuración (de momento solo comparada con la de Python) |
| 1 | convertir, forma de onda, miniaturas, huella, duplicados |
| 2 | pulso (Beat This! con `ort`), clic, pesos de PyTorch, separar en pistas (Demucs y el Mega53), las pistas separadas, el paquete del celular, las secuencias, la estructura y la guía automática, las guías habladas y sus voces (Supertonic y Piper), la rejilla del clic (lo guardado, leído y escrito como el `json` de Python, y re-analizar) |
| 3 | teoría, cifrados, TOON, los datos de fuera (`chat/safe`), la guía hablada, la web, los proveedores de IA, su catálogo y su cliente, leer listas pegadas, emparejar con la biblioteca, la narración, lo que se le cuenta al modelo en cada turno, la lista contra un repertorio, las confirmaciones con su plan de descarga, las herramientas del asistente y su conversación (el turno entero); de YouTube y yt-dlp, lo que se le pide, lo que contesta, lo bajado, las versiones, el motor de JavaScript y descomprimir la rueda; enriquecer una canción (la letra de LRCLIB, la carátula, la ficha de la IA, su cifrado, transponerlo y rellenar la ficha) e importar la Entrada (la cascada, el nombre, la carpeta y archivar) |

Siguen en Python: la API (`api/`) y sus trabajos en segundo plano, la
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

## La fase 4: sin Python

Cuando todo lo de arriba esté en Rust:

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
   `library::ingest::Outside`): hoy las da Python; falta la versión de Rust
   de cada uno.
4. **Los paquetes sin Python**: fuera PyInstaller y el núcleo empaquetado;
   la app lleva `libonnxruntime` (hoy sale del paquete `onnxruntime`) y
   yt-dlp como su ejecutable oficial, como ffmpeg y QuickJS. Los procesos
   aparte (pulso, separar) pasan a ser la propia app con un argumento.
5. **La CI**: los paquetes sin `uv` ni pruebas de Python; las pruebas de
   Python, solo en `tests.yml`.

### Cómo va (23 de octubre de 2026)

El enrutador ya existe: el crate `server/` (`danplay-server`). Contesta en
el mismo proceso (`server::handle(&app, &request)`) o por un socket Unix o
loopback con hyper (el binario `danplay-server`, con `--uds` o `--host` y
`--port`, como `packaging/core_entry.py`). Cómo se escribe una ruta, cómo se
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
- **En la app**: con `DANPLAY_RUST_CORE=1`, `core::request_within` le pide
  la API al núcleo de Rust en el mismo proceso (sin proceso aparte, sin
  socket, sin token); sin la variable, todo sigue como antes. Las pruebas de
  la interfaz contra él: `DANPLAY_E2E_CORE=<ruta de danplay-server> npm run e2e`.
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
  `./scripts/test.sh api` (o pytest con `DANPLAY_API_CORE=rust`) le da a cada
  cliente un `danplay-server` en caja de arena y sin red, con la
  configuración de Python de esa prueba (`tests/api_core.py`). Pasan 123; se
  saltan 36, diciendo por qué, las que sustituyen funciones de Python o miran
  su estado en memoria (`python_core`).
- **Faltan**: los paquetes sin Python y la CI.

Medido en la fase 0, lo que se espera ganar: unos 150 MB y miles de
archivos menos en el paquete de Windows, varios minutos menos por paquete en
la CI, el arranque sin esperar a que Python importe nada, y menos memoria
(un proceso en vez de dos).
