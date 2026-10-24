# Dependencias y sus licencias

DanPlay usa código de otra gente. Cada pieza mantiene su propia licencia, que
**no cambia** por usarse aquí. Este documento las lista y señala lo que
merece atención.

---

## `mutagen`: ya no va en los paquetes

`mutagen` es la librería que leía y escribía las etiquetas, y está bajo
**GPL-2.0-or-later**: era la única dependencia con copyleft fuerte del
proyecto. Mientras iba dentro del núcleo empaquetado, el `.deb`, el
`.AppImage` y los instaladores llevaban código GPL, y la GPL no permite
añadir restricciones a la obra combinada («solo uso no comercial», «no se
puede modificar»), que es justo lo que dice la licencia de DanPlay.

Desde entonces **las etiquetas las lee y las escribe el núcleo de Rust**
(con [lofty](https://github.com/Serial-ATA/lofty-rs), MIT o Apache-2.0: ver
`library/src/tags.rs`). Desde la fase 4 de la migración la app no lleva
Python, y desde la 1.40 tampoco el repositorio: el núcleo de Python, que
aún instalaba mutagen (lo pedía `yt-dlp[default]`) y lo usaba en sus
pruebas de testigo, se fue (ver [la migración](MIGRATION.md)). Lo que esas
pruebas comparaban con mutagen quedó guardado como casos fijos junto a las
de Rust (`library/src/tags/mutagen/`).

mutagen solo sigue dentro del ejecutable oficial de yt-dlp, que va como
programa aparte, igual que ffmpeg (abajo, en «Herramientas externas»).

**Esto no es asesoramiento legal.** Es para que la decisión se tome a
sabiendas.

## Lo que era de Python

Hasta la 1.40 el núcleo de Python tenía sus propias dependencias (FastAPI,
uvicorn, pydantic, openai, watchdog, mutagen, numpy, onnxruntime…) y las de
desarrollo (pytest, ruff, basedpyright, maturin; PyInstaller, hasta la
1.30). Se fueron con él: hoy no hay ninguna. Su tabla de licencias queda en
el historial de este archivo.

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
| `winreg` (solo Windows) | MIT |
| `cpal` (la salida de audio) | Apache-2.0 |
| `symphonia` | MPL-2.0 |
| `lofty` (las etiquetas de cada canción) | MIT OR Apache-2.0 |
| `rayon` | MIT OR Apache-2.0 |
| `rustfft` (el pulso del metrónomo, en la app) | MIT OR Apache-2.0 |
| `md-5` | MIT OR Apache-2.0 |
| `hyper`, `hyper-util` (`danplay-server`, el núcleo por un puerto para las pruebas) | MIT |
| `serde`, `serde_json` | MIT OR Apache-2.0 |
| `tokio` | MIT |
| `libc` | MIT OR Apache-2.0 |
| `rusqlite`, `libsqlite3-sys` (el índice; SQLite va dentro, y es de dominio público) | MIT |
| `fancy-regex`, `regex` (las expresiones de los nombres, con la semántica del `re` de Python, de donde vienen) | MIT / MIT OR Apache-2.0 |
| `unicode-normalization`, `unicode-properties`, `unicode-ident` | (MIT OR Apache-2.0) AND Unicode-3.0 |
| `ort`, `ort-sys` (ONNX Runtime desde Rust: el pulso, separar y las voces; carga la biblioteca oficial que viaja con la app, ver abajo) | MIT OR Apache-2.0 |
| `realfft` (los espectrogramas del pulso y del separador) | MIT |
| `half` (los pesos en float16), `zip` (los checkpoints de PyTorch) | MIT OR Apache-2.0 / MIT |
| `ureq` (lo que el núcleo pide a la red) | MIT OR Apache-2.0 |
| `rustls`, `ring` (su TLS, sin OpenSSL) | Apache-2.0 OR ISC OR MIT / Apache-2.0 AND ISC |
| `webpki-roots` (los certificados raíz de Mozilla, para `rustls`) | CDLA-Permissive-2.0 |
| `sha1`, `tempfile`, `dirs` | MIT OR Apache-2.0 |

## Interfaz (npm, instalada con Bun)

| Paquete | Licencia |
| --- | --- |
| `vue`, `vue-router` | MIT |
| `@tauri-apps/api`, `@tauri-apps/plugin-dialog` | MIT OR Apache-2.0 |
| `driver.js` (el recorrido del modo estudio) | MIT |
| `heroicons` (los iconos, copiados a `src/icons.js`) | MIT |

Para desarrollar, además (no viajan en la app): `vite-plus` (Vite+: Vite,
Vitest y Oxfmt; `vite` y `vitest` son los suyos), `@vitejs/plugin-vue`,
`vite-plugin-vue-devtools`, `@vitest/coverage-v8`, `@vue/test-utils`,
`jsdom`, `eslint` y sus configuraciones (`@eslint/js`, `eslint-plugin-vue`,
`eslint-config-prettier`, `eslint-plugin-vuejs-accessibility`), `stylelint`,
`vue-tsc`, `@types/node` (MIT); `typescript` y `@playwright/test`
(Apache-2.0); `@tauri-apps/cli` (MIT OR Apache-2.0). Y para instalarlas,
Bun (MIT).

## Herramientas externas

**Viajan dentro de los paquetes** (`tools/` y, ONNX Runtime, `data/`), con
versión fija y su SHA-256 comprobado al construirlos; compilada desde el
código, la app usa las del sistema si están (el `.deb` declara además ffmpeg
y fpcalc).

| Programa | Licencia | Para qué |
| --- | --- | --- |
| `ffmpeg` | LGPL-2.1+ / GPL según compilación (la de Windows, la *essentials* de Gyan, es **GPL-3.0**) | convertir formatos, encoger carátulas, opus/wma, velocidad y tono |
| `fpcalc` (Chromaprint) | MIT / LGPL-2.1+ | calcular la huella acústica |
| QuickJS-NG (`qjs`) | MIT | el motor de JavaScript que YouTube exige a yt-dlp (o Deno, Node, Bun, si están en el sistema) |
| `yt-dlp` | Unlicense | descargar de YouTube. Es su ejecutable oficial: un PyInstaller con su propio Python y sus dependencias dentro (entre ellas `mutagen`, GPL, y `yt-dlp-ejs`), que va como programa aparte, igual que ffmpeg |
| ONNX Runtime (`libonnxruntime`, `onnxruntime.dll`) | MIT (viaja con su licencia y sus avisos de terceros, `ThirdPartyNotices`) | mover las redes: el pulso, separar en pistas y las voces de las guías |
| `gio` (GLib) | LGPL-2.1+ | papelera de respaldo en Linux, si no se puede usar directamente la de freedesktop.org |

> Repartir el ffmpeg GPL dentro del instalador obliga a ofrecer su código
> fuente (el de esa versión está en la publicación de Gyan y en el repositorio
> de FFmpeg). Como lo de `mutagen`, arriba: no es asesoramiento legal, es para
> decidirlo a sabiendas. Una compilación LGPL de ffmpeg evitaría la obligación,
> pero sin `librubberband` el cambio de tono del modo estudio caería al método
> peor (`asetrate`+`atempo`).

## El separador de pistas: Demucs

Separar una canción en pistas usa **Demucs v4** (HTDemucs, de Meta; autor
Alexandre Défossez), **MIT**. Viajan con la app los grafos de la red
exportados a ONNX (`data/separator/`, generados a partir del código de Demucs
con `scripts/export-separator.py`, un guion de Python que queda en el
historial, en `a0e9317^`), **sin los pesos**: esos se bajan del repositorio
del autor en HuggingFace (`adefossez/HTDemucs-6s`, y los especialistas de
batería y bajo de `adefossez/HTDemucs-ft`) la primera vez que se separa algo,
y se comprueba su SHA-256. PyTorch y el paquete `demucs` solo harían falta
para regenerar los grafos, no para usar la app.

## La separación de máxima calidad: Mega53

La calidad «máxima» (percusión aparte, voz principal y coros, guitarras
eléctrica y acústica, piano, órgano, teclados, cuerdas, metales y vientos)
usa el **MVSep Mega 53 Stems** (un BS-RoFormer) de **ZFTurbo**, publicado en
[Music-Source-Separation-Training](https://github.com/ZFTurbo/Music-Source-Separation-Training)
(release v1.0.21), **MIT** el código y los pesos. Viaja con la app el grafo
exportado a ONNX con solo las cabezas que usa (`data/separator/mega53.onnx`,
generado con `scripts/export-mega53.py`, en el historial como el de
Demucs), **sin los pesos**: el checkpoint
original (1,4 GB) se baja de esa release la primera vez y se comprueba su
SHA-256; los pesos se leen de él sin ejecutar nada de su interior
(`analysis/src/weights.rs`).

## Las voces de las guías habladas

Las guías escritas las dice una voz sintética que funciona en tu equipo. Se
bajan una vez, cada una cuando se elige, desde una revisión fija de su
repositorio y con su SHA-256 (`data/voices.json`):

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
(`data/pulse/beat_this.onnx`, generado con `scripts/export-pulse.py`, en
el historial como los otros), **con sus pesos** en float16 (42 MB): la
licencia lo permite, y así funciona sin internet desde el primer día. El
original está en `cloud.cp.jku.at` (el sha256 del checkpoint del que sale va
en `beat_this.json`). PyTorch, torchaudio y el paquete `beat_this` solo
harían falta para regenerarlo.

Para medir qué detector acertaba más, `scripts/evaluate-pulse.py` usaba
**GTZAN** (Tzanetakis y Cook, 2002; lo bajaba de HuggingFace) con las
anotaciones de pulso y compás de **GTZAN-Rhythm** (Marchand y Peeters,
2015). Solo para medir: la app no los lleva ni los usa.

Para medir qué redes separaban mejor, `scripts/evaluate-separator.py` usaba
las muestras de 7 segundos de **MUSDB18** (Rafii et al., SigSep), que bajaba
el paquete `musdb` la primera vez. Solo servían para medir en la máquina de
quien desarrolla: la app no las lleva ni las usa.

Para medir si **Laya** (`convaiinnovations/laya-multilingual`, Convai, 2026,
**Apache-2.0**) decidía mejor que las reglas de la app,
`scripts/evaluate-laya.py` usaba el paquete `laya` con PyTorch. No mejoró
nada (ver docs/ARCHITECTURE.md, «Laya, medido y descartado»): la app no lo
lleva ni lo baja.

Los tres eran guiones de Python: quedan en el historial, en `a0e9317^`.

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
| GitHub (las publicaciones de yt-dlp) | «Actualizar yt-dlp»: el ejecutable de la última versión, comprobado con su `SHA2-256SUMS` | no |
| HuggingFace (el repositorio del autor de Demucs) | los pesos del separador de pistas, una sola vez y comprobados con su `sha256` | no |
| GitHub (la release de ZFTurbo) | los pesos de la separación de máxima calidad, una sola vez y comprobados con su `sha256` | no |
| HuggingFace (Supertone y Rhasspy) | las voces de las guías habladas, cada una cuando se elige y comprobadas con su `sha256` | no |

