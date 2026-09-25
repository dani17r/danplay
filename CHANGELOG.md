# Cambios

Lo que cambia en cada versión que se entrega. El detalle de por qué está
hecho así, en [docs/ARQUITECTURA.md](docs/ARQUITECTURA.md).

## 1.13.0 — 2026-09-25

Una revisión entera del proyecto: la biblioteca sigue al disco, el modo
desarrollo funciona, la CI pasa por primera vez y todas las dependencias,
herramientas y capas están al día.

### La biblioteca sigue al disco

- Lo que se mueve, se borra o se renombra por fuera de la app (el gestor de
  archivos, un disco que se desmonta) se ve solo en 2–3 s. Una carpeta que
  desaparece o vuelve, en menos de 5 s; y un repaso completo cada 10 minutos.
- Al abrir, las carpetas que ya no están apartan sus canciones antes de
  enseñar nada, y la sesión guardada vuelve sin lo que ya no existe (tampoco
  la última canción). Sin nada que enseñar: «No encuentro tu música» o «Tu
  biblioteca está vacía», con la forma de elegir dónde está ahora.
- Nada se pierde: lo que se va se aparta 30 días con su id. Si vuelve (a su
  sitio o a otro), recupera listas, acordes, análisis y notas; y elegir la
  carpeta movida la reconoce y la vuelve a enlazar sin releer nada.
- La cola del reproductor se entera: lo movido sigue sonando desde su sitio
  nuevo y lo borrado sale de la cola.
- Los ids ya no se reutilizan (una canción nueva heredaba las listas de la
  última borrada), y un nombre que no es UTF-8 ya no tumba el escaneo.

### Corregido

- **Núcleo**: el modo estudio no se recuperaba en FLAC/OGG/Opus; las claves de
  IA se escribían sin atomicidad (cortado a medias, se perdían) y podían viajar
  a otra URL al pulsar «Probar»; la base y el historial eran legibles por
  otros usuarios del equipo; respuestas del modelo que no eran un objeto
  daban un 500; varias carreras entre hilos; la letra con marcas de tiempo
  acababa en el USLT y en la hoja del atril; la migración desde «melodia»
  copiaba claves en entornos aislados; consultar la Entrada hacía reaparecer
  vacía una carpeta de música movida.
- **Escritorio**: el audio se colgaba para siempre al saltar con la salida
  muerta (un DAC desenchufado); en Windows, un `&` en una URL o en el nombre
  de una lista ejecutaba órdenes (`cmd /C start`); el núcleo revivido quedaba
  fuera del Job Object; un argumento que no es UTF-8 impedía arrancar; siete
  comandos congelaban la ventana; la transcodificación leía la tubería dentro
  del callback de audio (cortes); el pulso del metrónomo cargaba la canción
  entera en memoria (320 MB por hora; ahora 9).
- **Interfaz**: el arranque con el núcleo lento dejaba la app vacía para
  siempre; pulsar una fila mientras cargaba descartaba la lista; las notas del
  estudio podían guardarse en otra canción; Enter en «Cancelar» confirmaba
  borrar; la selección múltiple no iba en vistas agrupadas; la lista se
  cortaba en 1000 sin avisar; se repintaba todo 4 veces por segundo mientras
  sonaba; las órdenes del asistente se perdían al cambiar de página.

### Cambiado

- Las tareas largas (escanear, importar, convertir, duplicados, actualizar
  yt-dlp) corren en segundo plano con su progreso: el puente cortaba a los
  60 s y la interfaz daba error con el trabajo aún en marcha.
- yt-dlp se actualiza desde la app (PyPI, con `sha256`) y usa un motor de
  JavaScript (Deno, Node, QuickJS o Bun), que YouTube ya exige.
- Las listas llegan enteras (`count`) y ligeras (sin letras ni acordes).
- Un archivo «Artista - Título» sin etiquetas entra con ese artista.
- Accesibilidad: la biblioteca se recorre con el teclado y los diálogos
  atrapan y devuelven el foco.
- Cada ventana solo puede invocar lo que usa (permisos por ventana), y la
  salida de audio se suelta tras un minuto sin sonar.

### Herramientas y estructura

- **Modo desarrollo**: `./scripts/dev.sh [--prueba CARPETA]`, con el núcleo
  desde el código y datos aislados; convive con DanPlay instalado.
- **CI**: pasa por primera vez; estilo, tipos y cobertura en las tres capas,
  Playwright, comprobación del código de Windows y auditoría de
  dependencias; paquetes de Linux y de Windows al subir una etiqueta, con las
  herramientas del instalador fijadas por versión y `sha256`; Dependabot.
- **Python**: uv con `uv.lock` (y `requirements*.txt` generados, con hashes);
  ruff, basedpyright, pytest-cov e Hypothesis; `api/`, `chat/` y `library/`
  pasan a paquetes; esquema versionado; registro a archivo.
- **Rust**: workspace de Cargo (versión en un sitio), edición 2024, clippy
  pedantic sin avisos, rodio 0.22, `tauri-plugin-opener` y `-log`; `player/` y
  `queue/` en módulos.
- **Interfaz**: vue-router con carga diferida, composables (`useLibrary`,
  `useSelection`, `useSongMenus`, `useChat`…), una sola lista virtual para
  las vistas agrupadas, ESLint 10 sin avisos, Prettier, vue-tsc, Playwright.
- 1.426 pruebas (eran 783).
