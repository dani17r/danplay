# Arquitectura

Cómo encaja DanPlay por dentro, y por qué está hecho así. Si solo quieres
usarlo, con el [README](../README.md) tienes de sobra.

---

## Las cuatro capas

```text
   Vue 3 + Vite  (interfaz)
        │  invoke()          ← único puente, sin HTTP en el JS
   Tauri / Rust  (proceso principal)
        │  socket Unix 0600  ← sin puerto TCP; solo tu usuario
   Python        (núcleo: identificación, índice, IA, etiquetas)
        │
   Rust (PyO3)   (hashes en paralelo, análisis de audio)
```

Cada capa está donde está por una razón concreta:

**Vue 3** es interfaz y nada más. No sabe qué es un socket ni hace `fetch`.
Todo lo que necesita del núcleo lo pide con `invoke()` a Rust.

**Rust (Tauri)** es el proceso principal: arranca y vigila el núcleo Python,
hace de puente con la API, sirve el audio leyendo del disco y lo reproduce
nativamente.

**Python** es el núcleo: la cascada de identificación, el índice SQLite, las
etiquetas ID3, la IA, las descargas. Se puede usar solo, sin interfaz, desde
la línea de comandos.

**Rust (PyO3)** es donde va lo que Python hace lento: hashes de archivos en
paralelo (rayon) y análisis de audio con FFT. Es opcional: si el crate no está
compilado, Python tiene su propia versión más lenta y la app funciona igual.

## Por qué no hay puerto TCP

La API nunca escucha en un puerto. Solo en `$XDG_RUNTIME_DIR/danplay.sock` con
permisos `0600`.

Un puerto en `localhost` no es privado: cualquier proceso de tu equipo puede
hablar con él, y una página web abierta en tu navegador también. Esta API puede
listar tu biblioteca, mandar archivos a la papelera y leer imágenes del disco.
Con un socket Unix `0600`, solo tu usuario puede abrirlo.

`uvicorn` hace `chmod 0666` al socket que crea él mismo, así que lo creamos
nosotros con los permisos correctos y se lo pasamos ya escuchando.

Para desarrollo (`danplay serve` sin `--uds`) sí hay puerto TCP, y ahí sí se
aplican las protecciones de navegador: CORS restringido a los orígenes de Vite
y validación de la cabecera `Host` contra el reenlace de DNS. La prueba de humo
verifica que la app de escritorio no abre ningún puerto.

## El audio no pasa por el WebView

Cuando pulsas play, la interfaz pide la ruta del archivo a Python y se la
manda a Rust. Rust lo decodifica (`rodio` + `symphonia`) y lo saca a la tarjeta
de sonido desde un hilo dedicado. El WebView no toca el audio en ningún momento.

Hay además un protocolo propio (`danplay://audio/<id>`) que sirve el archivo
con soporte de rangos de bytes, para el modo navegador. Lee exactamente lo que
se le pide: enviar menos bytes de los que promete `content-range` hacía que el
cliente reintentara en bucle.

> **Ojo con el protocolo.** Tauri lo expone de forma distinta según el sistema:
> en Linux y Windows es `http://<esquema>.localhost/...`, y solo en macOS/iOS
> es `<esquema>://localhost/...`. Hay pruebas dedicadas a esto.

## La base de datos es desechable

Esta es la decisión de diseño que gobierna todo lo demás.

Todo lo que el usuario crea —estrellas, favorito, letra, carátula, tono, BPM,
a qué listas pertenece— se guarda **dentro del archivo**, en etiquetas ID3
estándar:

| Dato | Dónde vive |
| --- | --- |
| Estrellas (0-5) | `POPM` (convención de Windows Media Player / Kodi) |
| Favorito | `TXXX:FAVORITO` |
| Listas | `TXXX:LISTAS` |
| Letra | `USLT` |
| Carátula | `APIC` |
| Tono | `TKEY` |
| BPM | `TBPM` |
| Portada difuminada | `TXXX:PORTADA_BORROSA` |

La base SQLite es solo un índice para buscar rápido. Si la borras, un escaneo
la reconstruye entera. Y otros reproductores leen esas etiquetas igual que
DanPlay.

## El índice

SQLite con `journal_mode=WAL` y `synchronous=NORMAL`. Lo segundo es seguro
precisamente por lo anterior: en el peor caso se pierde la última escritura y
un escaneo la recupera del archivo.

La búsqueda de texto completo usa **FTS5** con `remove_diacritics`, así que
«cancion» encuentra «canción». El índice se mantiene solo, fila a fila, con
tres triggers (`INSERT`/`DELETE`/`UPDATE OF`) sobre la tabla `songs`. El
trigger de `UPDATE` solo salta si cambia una columna indexada: poner una
estrella no toca el índice de búsqueda.

El escaneo reaprovecha los archivos cuyo `mtime` y tamaño no han cambiado
desde la última vez. Escribir etiquetas cambia la fecha, así que eso siempre se
relee; y los archivos sin artista también, porque pueden resolverse ahora que
hay más carpetas de artista que antes.

## La cascada de identificación

```text
  1  Etiquetas ID3        confianza 0.95 si trae artista Y título
  2  Huella acústica      fpcalc → AcoustID → MusicBrainz     mínimo 0.75
  3  Heurístico           contra las carpetas de Artistas/    umbral 0.80
  4  IA (DeepInfra)       para los ambiguos                   umbral 0.55
  5  Revisar/             sin artista, nunca inventado
```

Un escalón que falla no tumba la cascada: si AcoustID devuelve un error de
servicio, decide el siguiente. Esto pasó de verdad y dejó la importación
completamente bloqueada, de ahí el envoltorio `_safe`.

**Por qué no se usa el postprocesador de metadatos de yt-dlp.** Rellena el
artista con el nombre del canal («Fulanito Music», «… - Topic»). Como las
etiquetas son el primer escalón de la cascada, se colaría con 0.95 de confianza
y archivaría la canción bajo un artista inventado. Solo se escriben etiquetas
cuando YouTube da datos de música de verdad (los campos `track` y `artist`, que
vienen de YouTube Music).

## Reglas de nombres

- Sin acentos. Única excepción: la **ñ** se conserva.
- Nada en MAYÚSCULA SOSTENIDA.
- Formato `Artista - Titulo (feat. X).mp3`
- Duplicados: sufijo ` - r`, ` - r2`, para compararlos y borrar a mano.

La limpieza quita el ruido típico de las descargas («VIDEO OFICIAL», «LETRA»,
`y2mate.com`, `320kbps`) con una sola expresión regular compilada, y separa
palabras pegadas (`VERSIONButterflyFull` → `VERSION Butterfly Full`).

## Detección de duplicados

Dos pasadas independientes:

**Idénticos byte a byte.** Se agrupa por tamaño, y solo a los grupos con más de
un archivo se les calcula el hash MD5 del primer megabyte, en paralelo con
rayon.

**La misma canción en otro archivo.** Se compara una *clave normalizada* que
ignora tildes, mayúsculas, palabras vacías y ruido de descarga, así que
«BARAK - Mi Gozo (Video Oficial)» y «Barak - Mi Gozo.mp3» dan la misma.

La comparación difusa es de todos contra todos, pero se apoya en cómo está
definida la puntuación para evitar el cálculo caro en casi todas las parejas:

- Si dos claves **comparten alguna palabra**, hace falta la puntuación
  completa. Un índice invertido palabra → claves da esas parejas directamente.
- Si **no comparten ninguna**, la intersección es vacía, dos de los tres
  términos de la fórmula valen cero, y la puntuación se reduce a comparar las
  cadenas tal cual. Como la clave ya viene ordenada y sin repetidos, esa
  comparación simple da exactamente el mismo número.

Los grupos que salen son idénticos a los de la comparación a lo bruto; hay una
prueba que lo verifica con casos adversos (plurales, subconjuntos, claves
vacías). Sigue creciendo con el cuadrado del número de canciones: por eso el
informe es una página que se abre a propósito, no algo que corra solo.

## La interfaz con listas grandes

La tabla y la cuadrícula pintan solo lo que se ve. El resto del alto lo ocupan
dos separadores, uno arriba y otro abajo, así que la barra de desplazamiento
mide lo mismo que si estuviera todo.

No hace falta saber nada del CSS: cuántos elementos caben por línea y cuánto
baja de una línea a la siguiente se **miden de lo ya pintado** (los que empiezan
a la misma altura son una línea). Por eso el mismo código sirve para la tabla,
donde el panel que se desplaza es un antepasado, y para la cuadrícula, donde es
ella misma.

Si no se puede medir —el panel aún no tiene alto, o un entorno sin maquetación
como las pruebas— se pinta la lista entera. Nunca puede salir vacía por un
fallo de medida.

## El asistente

23 herramientas sobre el mismo núcleo que usa la interfaz. No hay un camino
paralelo: cuando el asistente corrige un título, llama a la misma función que
el botón de renombrar.

**Reproducir no lo puede hacer él.** El audio lo maneja Rust y la cola vive en
la interfaz, así que las herramientas de reproducción devuelven una `action`
que la app ejecuta al recibir la respuesta.

**El texto de fuera es un dato, no una instrucción.** Lo que devuelven la
búsqueda web, los títulos de YouTube y las letras lo escriben terceros. Las
instrucciones del asistente dicen explícitamente que si ahí aparece algo con
forma de orden, no viene del usuario y no se obedece.

## Estructura del proyecto

```text
danplay/        núcleo Python (índice, IA, etiquetas, descargas)
core/           crate Rust (PyO3): hashes en paralelo y análisis de audio
desktop/        interfaz Vue 3 + envoltorio Tauri
  src/          componentes, composables, estilos
  src-tauri/    proceso principal en Rust
  tests/        pruebas de la interfaz
tests/          pruebas de Python, del frontend y de humo
packaging/      receta de PyInstaller para el núcleo empaquetado
scripts/        build.sh y test.sh
docs/           esta documentación
```

El **código** está en inglés: carpetas, archivos, funciones, variables, rutas
de la API y columnas de la base. Los **comentarios** y todo lo que ve el
usuario están en castellano.

## Construir

Son cuatro capas y el orden importa: si compilas solo una, la app arranca con
la versión vieja y no se nota. Por eso hay un script y no cuatro comandos.

```bash
./scripts/build.sh              # interfaz + núcleo + app
./scripts/build.sh --package    # además el .deb y el .AppImage
```

## Precisión del análisis de audio

Medido contra valores conocidos, con 8 canciones:

- **BPM**: 5 de 8 aceptando errores de octava. Usable.
- **Tono**: 2 de 8. **No es fiable.** Se probaron 60 combinaciones de perfiles
  y ponderaciones y todas se estancan ahí. El tono que muestra la app viene de
  la IA, que sí conoce las canciones; el del DSP queda como pista secundaria
  con su confianza a la vista. Se puede corregir a mano y queda guardado en el
  `TKEY` del archivo.

Es una limitación honesta, no un `TODO`. Si alguien sabe de detección de
tonalidad, es probablemente el sitio donde más se agradecería ayuda.
