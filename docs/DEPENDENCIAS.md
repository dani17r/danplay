# Dependencias y sus licencias

DanPlay usa código de otra gente. Cada pieza mantiene su propia licencia, que
**no cambia** por usarse aquí. Este documento las lista y señala las dos que
merecen atención.

---

## ⚠️ Aviso importante sobre `mutagen`

`mutagen` es la librería que lee y escribe las etiquetas ID3, y está bajo
**GPL-2.0-or-later**. Es la única dependencia con copyleft fuerte del proyecto,
y tiene una consecuencia práctica:

**La receta de empaquetado ([`packaging/core.spec`](../packaging/core.spec))
mete `mutagen` dentro del binario del núcleo.** Es decir, el `.deb` y el
`.AppImage` contienen código GPL.

La GPL permite redistribuir, pero pone dos condiciones que chocan de frente con
la licencia de este proyecto:

1. Hay que ofrecer el código fuente correspondiente.
2. **No se pueden añadir restricciones** sobre la obra combinada. «Solo uso no
   comercial» y «no se puede modificar» son exactamente eso.

Traducido: *publicar el código de este repositorio* es una cosa, y
*distribuir un binario compilado que lleve `mutagen` dentro* es otra bastante
distinta. Lo segundo, bajo una licencia no comercial y sin derivados, es muy
probablemente incompatible con la GPL.

**Esto no es asesoramiento legal.** Es una advertencia para que la decisión se
tome a sabiendas. Las salidas razonables son tres:

- **No distribuir binarios ya compilados.** Quien quiera usar la app la
  construye desde el código y `pip` le instala `mutagen` por su cuenta. Es lo
  más limpio y lo que hace la mayoría de proyectos en esta situación.
- **Cambiar `mutagen`** por una librería de etiquetas con licencia permisiva.
  Es trabajo de verdad: escribir ID3 está en el corazón del diseño.
- **Distribuir los binarios bajo GPL**, aceptando que eso permite el uso
  comercial y la modificación de la obra combinada.

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
| `mutagen` | GPL-2.0-or-later |
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
| Deno | MIT | el motor de JavaScript que YouTube exige a yt-dlp (o Node, Bun, QuickJS) |
| `yt-dlp`, `yt-dlp-ejs` | Unlicense (y MIT, ISC) | descargar de YouTube |
| `gio` (GLib) | LGPL-2.1+ | papelera de respaldo en Linux, si `send2trash` no puede |

> Repartir el ffmpeg GPL dentro del instalador obliga a ofrecer su código
> fuente (el de esa versión está en la publicación de Gyan y en el repositorio
> de FFmpeg). Como con `mutagen`, arriba: no es asesoramiento legal, es para
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

Para medir qué redes separan mejor, `scripts/evaluar-separador.py` usa las
muestras de 7 segundos de **MUSDB18** (Rafii et al., SigSep), que baja el
paquete `musdb` la primera vez. Solo sirven para medir en la máquina de
quien desarrolla: la app no las lleva ni las usa.

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
