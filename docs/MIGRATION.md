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
| 2 | pulso (Beat This! con `ort`), clic, pesos de PyTorch, separar en pistas (Demucs y el Mega53), las pistas separadas, el paquete del celular, las secuencias, la estructura y la guía automática, las guías habladas y sus voces (Supertonic y Piper) |
| 3 | teoría, cifrados, TOON, los datos de fuera (`chat/safe`), la guía hablada, la web, los proveedores de IA, su catálogo y su cliente, leer listas pegadas, emparejar con la biblioteca, la narración, lo que se le cuenta al modelo en cada turno, la lista contra un repertorio, las confirmaciones con su plan de descarga, las herramientas del asistente y su conversación (el turno entero) |

Siguen en Python: la API (`api/`) y sus trabajos en segundo plano, la
rejilla del clic (`beatgrid.py`: hay un borrador probado), el resto del
asistente que orquesta alrededor de Rust: el registro de las confirmaciones
(`chat/plan.py`: su cerrojo, sus topes y los hilos que buscan en YouTube), la
validación de los argumentos de las herramientas (pydantic, en
`chat/tools.py`) y, del turno (`chat/loop.py`), las llamadas a la IA y al
juez, leer la lista pegada, lo que se apunta para el visto bueno y el uso del
mes,
YouTube y yt-dlp,
importar y enriquecer, el vigilante de carpetas, la línea de órdenes, y lo
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
   fronteras sean inyectables también desde Rust. Las del asistente ya son
   rasgos (`plan::Index`, `tools::Io`, `conversation::Chat`): hoy los
   implementa Python; falta la versión de Rust de cada uno.
4. **Los paquetes sin Python**: fuera PyInstaller y el núcleo empaquetado;
   la app lleva `libonnxruntime` (hoy sale del paquete `onnxruntime`) y
   yt-dlp como su ejecutable oficial, como ffmpeg y QuickJS. Los procesos
   aparte (pulso, separar) pasan a ser la propia app con un argumento.
5. **La CI**: los paquetes sin `uv` ni pruebas de Python; las pruebas de
   Python, solo en `tests.yml`.

Medido en la fase 0, lo que se espera ganar: unos 150 MB y miles de
archivos menos en el paquete de Windows, varios minutos menos por paquete en
la CI, el arranque sin esperar a que Python importe nada, y menos memoria
(un proceso en vez de dos).
