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

| Paquete | Licencia |
| --- | --- |
| `annotated-doc` | MIT |
| `annotated-types` | MIT |
| `anyio` | MIT |
| `audioop-lts` | PSF-2.0 |
| `audioread` | MIT |
| `certifi` | Mozilla Public License 2.0 (MPL 2.0) |
| `charset-normalizer` | MIT |
| `click` | BSD-3-Clause |
| `fastapi` | MIT |
| `h11` | MIT License |
| `httpcore2` | BSD-3-Clause |
| `httptools` | MIT |
| `httpx2` | BSD-3-Clause |
| `idna` | BSD-3-Clause |
| `jiter` | MIT |
| `maturin` | MIT OR Apache-2.0 |
| `musicbrainzngs` | BSD License |
| `mutagen` | GPL-2.0-or-later |
| `openai` | Apache-2.0 |
| `pyacoustid` | MIT |
| `pydantic` | MIT |
| `pydantic_core` | MIT |
| `python-dotenv` | BSD-3-Clause |
| `python-multipart` | Apache-2.0 |
| `PyYAML` | MIT License |
| `RapidFuzz` | MIT |
| `requests` | Apache Software License |
| `sniffio` | MIT License |
| `standard-aifc` | Python Software Foundation License |
| `standard-chunk` | Python Software Foundation License |
| `standard-sunau` | Python Software Foundation License |
| `starlette` | BSD-3-Clause |
| `truststore` | MIT |
| `typing-inspection` | MIT |
| `typing_extensions` | PSF-2.0 |
| `urllib3` | MIT |
| `uvicorn` | BSD-3-Clause |
| `uvloop` | Apache Software License |
| `watchdog` | Apache Software License |
| `watchfiles` | MIT License |
| `websockets` | BSD-3-Clause |
| `yt-dlp` | Unlicense |

## Rust

Todas permisivas. `symphonia` usa MPL-2.0, que es copyleft por archivo: no
afecta mientras no se modifiquen sus archivos.

| Crate | Licencia |
| --- | --- |
| `tauri`, `tauri-plugin-dialog` | MIT OR Apache-2.0 |
| `rodio` | MIT OR Apache-2.0 |
| `symphonia` | MPL-2.0 |
| `pyo3` | MIT OR Apache-2.0 |
| `rayon` | MIT OR Apache-2.0 |
| `rustfft` | MIT OR Apache-2.0 |
| `md-5` | MIT OR Apache-2.0 |
| `hyper`, `hyper-util`, `hyperlocal` | MIT |
| `serde`, `serde_json` | MIT OR Apache-2.0 |
| `tokio` | MIT |
| `libc` | MIT OR Apache-2.0 |

## Interfaz (npm)

| Paquete | Licencia |
| --- | --- |
| `vue` | MIT |
| `@tauri-apps/api`, `@tauri-apps/plugin-dialog` | MIT OR Apache-2.0 |
| `vite`, `@vitejs/plugin-vue` | MIT |
| `vitest`, `@vue/test-utils`, `jsdom` | MIT |
| `heroicons` (los iconos) | MIT |

## Herramientas externas

No se distribuyen con la app; se invocan si están en el sistema.

| Programa | Licencia | Para qué |
| --- | --- | --- |
| `ffmpeg` | LGPL-2.1+ / GPL-2+ según compilación | convertir formatos, encoger carátulas |
| `fpcalc` (Chromaprint) | LGPL-2.1+ | calcular la huella acústica |
| `yt-dlp` | Unlicense | descargar de YouTube |
| `gio` (GLib) | LGPL-2.1+ | mandar archivos a la papelera |

## Servicios que se consultan

Ninguno es obligatorio; la app funciona sin todos ellos.

| Servicio | Para qué | Requiere clave |
| --- | --- | --- |
| AcoustID + MusicBrainz | identificar por huella acústica | sí (gratuita) |
| DeepInfra | IA y asistente | sí |
| LRCLIB | letras | no |
| iTunes Search / Cover Art Archive | carátulas | no |
| DuckDuckGo (HTML) | comprobar datos en la web | no |

---

Cómo regenerar la tabla de Python:

```bash
.venv/bin/python -m pip list --format=freeze | cut -d= -f1 | while read p; do
  printf '%-24s %s\n' "$p" "$(.venv/bin/python -c "
import importlib.metadata as m,sys
d=m.metadata('$p')
print(d.get('License-Expression') or next((c.split('::')[-1].strip()
  for c in d.get_all('Classifier') or [] if c.startswith('License')), '?'))" 2>/dev/null)"
done
```
