# Dependencias y sus licencias

DanPlay usa código de otra gente. Cada pieza mantiene su propia licencia, que
**no cambia** por usarse aquí. Este documento las lista y señala las dos que
merecen atención.

---

## `mutagen`: ya no va en los paquetes

`mutagen` es la librería que leía y escribía las etiquetas, y está bajo
**GPL-2.0-or-later**: la única dependencia con copyleft fuerte del proyecto.
Mientras iba dentro del núcleo empaquetado, el `.deb`, el `.AppImage` y los
instaladores llevaban código GPL, y la GPL no permite añadir restricciones a
la obra combinada («solo uso no comercial», «no se puede modificar»), que es
justo lo que dice la licencia de DanPlay.

Desde la versión que trae este cambio, **las etiquetas las lee y las escribe
el crate de Rust del núcleo** (`danplay_core`, con
[lofty](https://github.com/Serial-ATA/lofty-rs), MIT o Apache-2.0: ver
`biblioteca/src/etiquetas.rs`), y el núcleo empaquetado **excluye** mutagen
(`packaging/core.spec`; la compilación comprueba que no se cuela). Se sigue
instalando al compilar desde el código, porque lo pide `yt-dlp[default]`
(yt-dlp solo lo usaría para meter la carátula en un m4a, un ogg o un opus, y
DanPlay baja siempre mp3, cosa que hace con ffmpeg), y lo usan las pruebas,
de testigo: comparan lo que leen y escriben las dos.
`danplay/tags_mutagen.py` es la versión de antes, para cuando no está el
crate (en desarrollo o en las pruebas sin Rust).

**Esto no es asesoramiento legal.** Es para que la decisión se tome a
sabiendas.

## `certifi` (MPL-2.0)

Copyleft por archivo, no por proyecto. Mientras no se modifiquen sus archivos,
se puede combinar con código propietario sin problema. No requiere ninguna
acción.

## Núcleo y app (Python)

Lo que lleva la app al ejecutarse (`requirements.txt`, que sale de `uv.lock`
con todas las versiones fijadas). Algunos solo se instalan en otros sistemas
o intérpretes (`cffi`, `brotlicffi`, `pycparser`, `httpx2-jsfetch`).

| Paquete | Licencia |
| --- | --- |
| `annotated-doc` | MIT |
| `annotated-types` | MIT |
| `anyio` | MIT |
| `audioop-lts` | PSF-2.0 |
| `audioread` | MIT |
| `brotli` | ? |
| `brotlicffi` | MIT |
| `certifi` | Mozilla Public License 2.0 (MPL 2.0) |
| `cffi` | MIT-0 |
| `charset-normalizer` | ? |
| `click` | BSD-3-Clause |
| `fastapi` | MIT |
| `flatbuffers` (lo pide ONNX Runtime) | Apache-2.0 |
| `h11` | MIT License |
| `httpcore2` | BSD-3-Clause |
| `httpx2` | BSD-3-Clause |
| `httpx2-jsfetch` | BSD-3-Clause |
| `idna` | BSD-3-Clause |
| `jiter` | MIT |
| `mutagen` (solo al compilar y en las pruebas: el núcleo empaquetado no lo lleva) | GPL-2.0-or-later |
| `numpy` (el separador de pistas) | BSD-3-Clause (lleva OpenBLAS, BSD-3-Clause) |
| `onnxruntime` (el separador de pistas) | MIT |
| `openai` | Apache-2.0 |
| `packaging` (lo pide ONNX Runtime) | Apache-2.0 OR BSD-2-Clause |
| `platformdirs` | MIT |
| `protobuf` (lo pide ONNX Runtime) | BSD-3-Clause |
| `pyacoustid` | MIT |
| `pycparser` | BSD-3-Clause |
| `pycryptodomex` | BSD License |
| `pydantic` | MIT |
| `pydantic-core` | MIT |
| `python-dotenv` | ? |
| `rapidfuzz` | MIT |
| `requests` | Apache Software License |
| `send2trash` | BSD-3-Clause |
| `sniffio` | MIT License |
| `standard-aifc` | Python Software Foundation License |
| `standard-chunk` | Python Software Foundation License |
| `standard-sunau` | Python Software Foundation License |
| `starlette` | BSD-3-Clause |
| `truststore` | MIT |
| `typing-extensions` | PSF-2.0 |
| `typing-inspection` | MIT |
| `urllib3` | MIT |
| `uvicorn` | BSD-3-Clause |
| `watchdog` | Apache Software License |
| `websockets` | BSD-3-Clause |
| `yt-dlp` | Unlicense |
| `yt-dlp-ejs` | Unlicense AND MIT AND ISC |

Para desarrollar, además (no viajan en la app): `pytest`, `pytest-cov`,
`hypothesis` y `httpx` (pruebas; MIT/BSD/MPL-2.0), `ruff` (MIT),
`basedpyright` (MIT), `maturin` (MIT OR Apache-2.0) y `pyinstaller`
(GPL-2.0 con excepción para lo que empaqueta: no afecta a la licencia de la
app).

## Rust

Todas permisivas. `symphonia` usa MPL-2.0, que es copyleft por archivo: no
afecta mientras no se modifiquen sus archivos.

| Crate | Licencia |
| --- | --- |
| `tauri` y sus plugins (dialog, single-instance, window-state, notification, positioner, opener, log) | MIT OR Apache-2.0 |
| `ksni` (bandeja por D-Bus en Linux) | Unlicense |
| `souvlaki` (MPRIS / SMTC / Now Playing) | MIT |
| `zbus`, `dbus` | MIT |
| `log` | MIT OR Apache-2.0 |
| `rtrb` (el anillo del audio que decodifica ffmpeg) | MIT OR Apache-2.0 |
| `rand` | MIT OR Apache-2.0 |
| `win32job`, `winreg` (solo Windows) | MIT OR Apache-2.0 / MIT |
| `rodio` | MIT OR Apache-2.0 |
| `symphonia` | MPL-2.0 |
| `pyo3` | MIT OR Apache-2.0 |
| `lofty` (las etiquetas de cada canción) | MIT OR Apache-2.0 |
| `rayon` | MIT OR Apache-2.0 |
| `rustfft` (el pulso del metrónomo, en la app) | MIT OR Apache-2.0 |
| `md-5` | MIT OR Apache-2.0 |
| `hyper`, `hyper-util`, `hyperlocal` | MIT |
| `serde`, `serde_json` | MIT OR Apache-2.0 |
| `tokio` | MIT |
| `libc` | MIT OR Apache-2.0 |

## Interfaz (npm)

| Paquete | Licencia |
| --- | --- |
| `vue`, `vue-router` | MIT |
| `@tauri-apps/api`, `@tauri-apps/plugin-dialog` | MIT OR Apache-2.0 |
| `heroicons` (los iconos, copiados a `src/icons.js`) | MIT |

Para desarrollar, además (no viajan en la app): `vite`, `@vitejs/plugin-vue`,
`vite-plugin-vue-devtools`, `vitest`, `@vitest/coverage-v8`, `@vue/test-utils`,
`jsdom`, `eslint` y sus configuraciones (`@eslint/js`, `eslint-plugin-vue`,
`eslint-config-prettier`, `eslint-plugin-vuejs-accessibility`), `prettier`,
`stylelint`, `vue-tsc`, `@types/node` (MIT); `typescript` y
`@playwright/test` (Apache-2.0); `@tauri-apps/cli` (MIT OR Apache-2.0).

## Herramientas externas

En Linux no se distribuyen con la app: se invocan si están en el sistema (el
`.deb` declara ffmpeg y fpcalc). **El instalador de Windows sí las lleva
dentro** (`tools\`), con versión fija y su SHA-256 comprobado al construirlo.

| Programa | Licencia | Para qué |
| --- | --- | --- |
| `ffmpeg` | LGPL-2.1+ / GPL según compilación (la de Windows, la *essentials* de Gyan, es **GPL-3.0**) | convertir formatos, encoger carátulas, opus/wma, velocidad y tono |
| `fpcalc` (Chromaprint) | MIT / LGPL-2.1+ | calcular la huella acústica |
| QuickJS-NG (`qjs`) | MIT | el motor de JavaScript que YouTube exige a yt-dlp (o Deno, Node, Bun, si están en el sistema) |
| `yt-dlp`, `yt-dlp-ejs` | Unlicense (y MIT, ISC) | descargar de YouTube |
| `gio` (GLib) | LGPL-2.1+ | papelera de respaldo en Linux, si `send2trash` no puede |

> Repartir el ffmpeg GPL dentro del instalador obliga a ofrecer su código
> fuente (el de esa versión está en la publicación de Gyan y en el repositorio
> de FFmpeg). Como lo de `mutagen`, arriba: no es asesoramiento legal, es para
> decidirlo a sabiendas. Una compilación LGPL de ffmpeg evitaría la obligación,
> pero sin `librubberband` el cambio de tono del modo estudio caería al método
> peor (`asetrate`+`atempo`).

## El separador de pistas: Demucs

Separar una canción en pistas usa **Demucs v4** (HTDemucs, de Meta; autor
Alexandre Défossez), **MIT**. Viajan con la app los grafos de la red
exportados a ONNX (`danplay/data/separador/`, generados con
`scripts/exportar-separador.py` a partir del código de Demucs), **sin los
pesos**: esos se bajan del repositorio del autor en HuggingFace
(`adefossez/HTDemucs-6s`, y los especialistas de batería y bajo de
`adefossez/HTDemucs-ft`) la primera vez que se separa algo, y se comprueba
su SHA-256. PyTorch y el paquete `demucs` solo hacen falta para regenerar
los grafos, no para usar la app.

## La separación de máxima calidad: Mega53

La calidad «máxima» (percusión aparte, voz principal y coros, guitarras
eléctrica y acústica, piano, órgano, teclados, cuerdas, metales y vientos)
usa el **MVSep Mega 53 Stems** (un BS-RoFormer) de **ZFTurbo**, publicado en
[Music-Source-Separation-Training](https://github.com/ZFTurbo/Music-Source-Separation-Training)
(release v1.0.21), **MIT** el código y los pesos. Viaja con la app el grafo
exportado a ONNX con solo las cabezas que usa (`danplay/data/separador/mega53.onnx`,
generado con `scripts/exportar-mega53.py`), **sin los pesos**: el checkpoint
original (1,4 GB) se baja de esa release la primera vez y se comprueba su
SHA-256; los pesos se leen de él sin ejecutar nada de su interior
(`danplay/torchzip.py`).

## Las voces de las guías habladas

Las guías escritas las dice una voz sintética que funciona en tu equipo. Se
bajan una vez, cada una cuando se elige, desde una revisión fija de su
repositorio y con su SHA-256 (`danplay/data/voces.json`):

| Voz | De | Licencia |
| --- | --- | --- |
| Supertonic 3 (diez voces) | Supertone, `supertone-oss-archive/supertonic-3` | BigScience OpenRAIL-M |
| David · España (`es_ES-davefx`) | Piper, `rhasspy/piper-voices` | CC0 |
| Sharvard · España (`es_ES-sharvard`, dos voces) | Piper | CC BY 3.0 |
| Carl · España (`es_ES-carlfm`) | Piper | dominio público |
| Ald · México (`es_MX-ald`) | Piper | Unlicense |
| Claude · México (`es_MX-claude`) | Piper | Apache-2.0 |
| Daniela · Argentina (`es_AR-daniela`) | Piper | CC BY-SA 4.0 |

El motor de Piper es MIT. La licencia OpenRAIL-M de Supertonic prohíbe,
entre otras cosas, usar la voz para suplantar a una persona o para engañar:
los términos de DanPlay lo recogen. Las voces con CC BY y CC BY-SA piden
citar su origen: cada una trae su `MODEL_CARD` con sus autores, que se
guarda junto a ella.

## El detector de pulso: Beat This!

El pulso y el «1» del metrónomo los saca **Beat This!** (Foscarin, Schlüter
y Widmer, CPJKU, Universidad Johannes Kepler de Linz, 2024), **MIT** el
código y los pesos. Viaja con la app el modelo `final0` exportado a ONNX
(`danplay/data/pulso/beat_this.onnx`, generado con
`scripts/exportar-pulso.py`), **con sus pesos** en float16 (42 MB): la
licencia lo permite, y así funciona sin internet desde el primer día. El
original está en `cloud.cp.jku.at` (el sha256 del checkpoint del que sale va
en `beat_this.json`). PyTorch, torchaudio y el paquete `beat_this` solo hacen
falta para regenerarlo.

Para medir qué detector acierta más, `scripts/evaluar-pulso.py` usa **GTZAN**
(Tzanetakis y Cook, 2002; lo baja de HuggingFace) con las anotaciones de
pulso y compás de **GTZAN-Rhythm** (Marchand y Peeters, 2015). Solo para
medir: la app no los lleva ni los usa.

Para medir qué redes separan mejor, `scripts/evaluar-separador.py` usa las
muestras de 7 segundos de **MUSDB18** (Rafii et al., SigSep), que baja el
paquete `musdb` la primera vez. Solo sirven para medir en la máquina de
quien desarrolla: la app no las lleva ni las usa.

Para medir si **Laya** (`convaiinnovations/laya-multilingual`, Convai, 2026,
**Apache-2.0**) decide mejor que las reglas de la app,
`scripts/evaluar-laya.py` usa el paquete `laya` con PyTorch. No mejoró nada
(ver docs/ARQUITECTURA.md, «Laya, medido y descartado»): la app no lo lleva
ni lo baja.

## Servicios que se consultan

Ninguno es obligatorio; la app funciona sin todos ellos.

| Servicio | Para qué | Requiere clave |
| --- | --- | --- |
| AcoustID + MusicBrainz | identificar por huella acústica | sí (gratuita) |
| El proveedor de IA que elijas (OpenAI, Anthropic, Google, DeepInfra, OpenRouter… o un Ollama en tu equipo) | IA y asistente | según el proveedor; los locales no |
| models.dev | el catálogo de modelos de IA (qué modelos existen, precio, si usan herramientas); petición anónima y condicional | no |
| LLM7, Kilo (rutas `:free`), OpenCode Zen | «Probar gratis, sin clave»: IA de prueba, solo si el usuario la elige; servicios de terceros con límites | no |
| LRCLIB | letras | no |
| iTunes Search / Cover Art Archive | carátulas | no |
| DuckDuckGo (HTML) | comprobar datos en la web | no |
| PyPI | «Actualizar yt-dlp»: la última versión, comprobada con su `sha256` | no |
| HuggingFace (el repositorio del autor de Demucs) | los pesos del separador de pistas, una sola vez y comprobados con su `sha256` | no |
| GitHub (la release de ZFTurbo) | los pesos de la separación de máxima calidad, una sola vez y comprobados con su `sha256` | no |
| HuggingFace (Supertone y Rhasspy) | las voces de las guías habladas, cada una cuando se elige y comprobadas con su `sha256` | no |

---

Cómo regenerar la tabla de Python (la de todo el `.venv`, desarrollo
incluido):

```bash
uv pip list --format=freeze | cut -d= -f1 | while read p; do
  printf '%-24s %s\n' "$p" "$(.venv/bin/python -c "
import importlib.metadata as m,sys
d=m.metadata('$p')
print(d.get('License-Expression') or next((c.split('::')[-1].strip()
  for c in d.get_all('Classifier') or [] if c.startswith('License')), '?'))" 2>/dev/null)"
done
```
