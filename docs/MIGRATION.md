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
| 2 | pulso (Beat This! con `ort`), clic, pesos de PyTorch |
| 3 | teoría, cifrados, TOON, los datos de fuera (`chat/safe`), la guía hablada, la web |

Siguen en Python (o los está portando un ayudante): la API (`api/`), la
orquestación de la fase 2 (pistas separadas, rejilla del clic, reaprovechar,
guías, estructura, secuencias, paquete para el celular), separar en pistas,
el asistente (`chat/*`), la IA y sus proveedores, YouTube y yt-dlp,
importar y enriquecer, el vigilante de carpetas y la línea de órdenes.

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
   fronteras sean inyectables también desde Rust.
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
