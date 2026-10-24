# Arquitectura

Cómo encaja DanPlay por dentro, y por qué está hecho así. Si solo quieres
usarlo, con el [README](../README.md) tienes de sobra.

---

## Las capas

```text
   Vue 3 + Vite  (interfaz)
        │  invoke() + eventos ← único puente, sin HTTP en el JS
   Tauri / Rust  (proceso principal: cola, audio, bandeja, MPRIS)
        │  en el mismo proceso ← sin socket ni puerto
   Rust: server, library, analysis
                 (núcleo: identificación, índice, IA, etiquetas, el pulso
                  y separar con ONNX Runtime)
```

Hasta la 1.30 el núcleo era Python, un programa aparte (empaquetado con
PyInstaller) al que la app hablaba por un socket Unix `0600`, con el crate
PyO3 debajo para lo que Python hacía lento. Desde la fase 4 de
[la migración](MIGRATION.md) la app lleva el de Rust dentro y no hay Python
en ningún paquete; desde la fase 5 (la 1.40), tampoco en el repositorio:
sus pruebas pasaron a Rust y el núcleo de Python se fue.

El contrato exacto entre capas —qué comandos hay, qué eventos se emiten y qué
nombres usa cada campo— está en [INTERNAL-CONTRACT.md](INTERNAL-CONTRACT.md).

Cada capa está donde está por una razón concreta:

**Vue 3** es interfaz y nada más. No sabe qué es un socket ni hace `fetch`.
Todo lo que necesita del núcleo lo pide con `invoke()` a Rust.

**Rust (Tauri)** es el proceso principal: lleva **el núcleo dentro** (el
crate `server`, que contesta la API en el mismo proceso, `core.rs`), le hace
de puente, mira sus cambios (`revision`) para que la interfaz se refresque
sola, y reproduce el audio nativamente. También guarda **la cola de
reproducción**, dibuja el icono de la bandeja y publica lo que suena al
escritorio. Los procesos aparte del núcleo (el detector de pulso y el que
separa en pistas) son la propia app con un argumento.

**El núcleo** es Rust: la API y sus trabajos (`server/`), la cascada de
identificación, el índice SQLite, las etiquetas, la IA y el asistente
(`library/`), y lo numérico y las redes (`analysis/`).

**El camino fue de dentro afuera**: el núcleo de Python pasó a Rust módulo
a módulo con la misma API, con un crate PyO3 (`core/`) de puente. Lo que
pasó primero a `library/`:

- el índice: la conexión a SQLite (`index.rs`, que Python usaba como si
  fuera `sqlite3`, con sus transacciones), el esquema y sus migraciones,
  buscar, las carpetas y sus exclusiones, el escaneo, cada canción y su
  modo estudio, «Transformar», el historial;
- las listas (con sus «No está»), lo abierto desde fuera, las
  conversaciones y el diario;
- las etiquetas (lofty), los nombres, la teoría y los cifrados, TOON, los
  datos de fuera como dato, la guía hablada, la búsqueda en la web;
- la conversión, la forma de onda, las miniaturas, la huella y los
  duplicados;
- la configuración (`config.rs`) y lo que todo eso comparte: leer lo que
  llega en un JSON (`value.rs`), los textos (`text.rs`), redondear
  (`num.rs`), las rutas guardadas como texto (`paths.rs`) y leer el JSON que
  dejaron las versiones de antes (`lenient_json.rs`).

Mientras tanto Python llamaba al crate con la misma API de antes, y sus
pruebas eran el testigo de que nada cambiaba; además, cada módulo se comparó
con su versión anterior en miles de casos. Después vinieron la API
(`server/`) y la app con el núcleo dentro (fase 4), y por último sus pruebas,
pasadas a Rust una a una (fase 5: [TESTS-PORT.md](TESTS-PORT.md)). Con eso
se fueron el núcleo de Python, `core/` y la versión vieja de las
etiquetas, la de mutagen (GPL).

## Por qué la cola vive en Rust

Estaba en la interfaz, que es donde parecía natural: allí está la lista y allí
se pulsa. Se movió por tres razones concretas.

**Con la ventana escondida, el JS se ralentiza.** Los navegadores frenan los
temporizadores de una página que no se ve; a los pocos minutos, hasta una vez
por minuto. El aviso de «se acabó la canción» salía de un temporizador cada
250 ms, así que con la aplicación en la bandeja podía haber un minuto de
silencio entre canciones.

**La bandeja y el mini reproductor piden «siguiente» sin ventana delante.**
Igual que las teclas multimedia del teclado. Alguien tiene que saber qué es
«siguiente» sin depender de una ventana que puede estar dormida o cerrada.

**Había tres bucles preguntando lo mismo.** El reproductor grande cada 250 ms,
la ventanita cada 400 ms, y la app observando su propio estado para
reenviárselo a la bandeja. Ahora Rust **avisa** (`danplay://state`) y las
ventanas escuchan: ni un sondeo.

La máquina de estados (qué suena al acabarse una canción según el modo de
repetición y el aleatorio) es una función pura, así que se prueba con `cargo
test` sin tarjeta de sonido. La interfaz tiene la misma en
`playback/queueLogic.js` para el modo navegador, con las mismas pruebas.

## Por qué la bandeja no usa la de Tauri

En Linux, Tauri dibuja el icono con `libappindicator`, y esa biblioteca **no
entrega los clics**: solo abre el menú. Lo dice la propia documentación de
Tauri («Linux: unsupported») y lleva años así. Por eso no se podía tener un
mini reproductor al pulsar el icono.

DanPlay habla el protocolo directamente (`StatusNotifierItem` sobre D-Bus, con
el crate `ksni`), que es el mismo que usa Plasma de forma nativa y el que
traduce la extensión AppIndicator en GNOME. Con eso llegan el clic izquierdo,
el central y la rueda.

Fuera de Linux sí se usa la bandeja de Tauri, porque allí los clics llegan y
además el sistema dice **dónde** está el icono, con lo que la ventanita se
pega justo encima. En Linux con Wayland eso no se puede: no existen las
coordenadas globales y una aplicación no puede colocar sus propias ventanas;
la sitúa el escritorio.

Si no hay bandeja donde quedarse (un escritorio sin ella, GNOME sin la
extensión), la aplicación se entera —`spawn` falla— y entonces **cerrar la
ventana cierra la aplicación**. Dejarla viva y escondida sin icono sería
dejarla sin forma de volver ni de salir.

## Por qué no hay puerto TCP

La API de la app no escucha en ningún sitio: el núcleo va dentro y la
interfaz le habla con `invoke()`, sin salir del proceso.

Un puerto en `localhost` no es privado: cualquier proceso de tu equipo puede
hablar con él, y una página web abierta en tu navegador también. Esta API puede
listar tu biblioteca, mandar archivos a la papelera y leer imágenes del disco.
Hasta la 1.30, con el núcleo de Python aparte, la app le hablaba por
`$XDG_RUNTIME_DIR/danplay/core.sock` con permisos `0600` (y `uvicorn` hacía
`chmod 0666` al socket que creaba él, así que se creaba fuera con los
permisos buenos). Ahora no hay ni eso.

Para desarrollo y las pruebas de la interfaz (`danplay-server` sin
`--uds`) sí hay puerto TCP, y ahí se
aplican tres protecciones de navegador: CORS restringido a los orígenes de
Vite, validación de la cabecera `Host` contra el reenlace de DNS, y una
cabecera propia (`X-DanPlay: 1`) en **todas** las peticiones a `/api/`,
también los `GET`. Esto último porque CORS impide **leer** la respuesta pero
no evita el efecto: un `POST` sin cuerpo a `/api/scan` desde cualquier
pestaña abierta arrancaba un escaneo, y hay `GET` que gastan IA. Exigir una
cabecera propia obliga al navegador a preguntar antes, y ahí CORS sí corta.
Solo se libran el audio y las carátulas, que el navegador pide por su cuenta
desde `<audio>` e `<img>` y no pueden llevarla.

Y dos cosas más en ese modo: `--host` con una dirección que no
sea de loopback se niega a arrancar si no hay token (sería abrir la API a la
red), y no existen `/docs`, `/redoc` ni `/openapi.json`, que quedaban fuera de
la comprobación del token y contaban a cualquiera todo lo que sabe hacer.

En Windows, hasta la 1.30, no había sockets Unix que uvicorn supiera
escuchar, así que allí la aplicación levantaba el núcleo en `127.0.0.1` con
un puerto libre y un secreto de un solo arranque (`DANPLAY_TOKEN`: sin él,
`401`). Con el núcleo dentro, Windows queda como los demás: sin puerto.

La prueba de humo verifica que la app de escritorio no abre ningún puerto ni
socket, ni lanza ningún núcleo aparte.

## El audio no pasa por el WebView

Cuando pulsas play, la interfaz pide la ruta del archivo al núcleo y se la
manda a la cola. Rust lo decodifica y lo saca a la tarjeta de sonido con el
**motor de pistas** (`engine/`, el mismo que la app del celular): symphonia
para decodificar, un render propio muestra a muestra y cpal para la salida.
El WebView no toca el audio en ningún momento. Ver «El motor» más abajo.

Las carátulas sí llegan al WebView, por un protocolo propio
(`danplay://cover/<id>?size=N`, en `protocol.rs`) que se las pide al núcleo
sin pasar por el JS. En el modo navegador, el `<audio>` lee
`/api/song/{id}/audio` por el proxy de Vite.

> **Ojo con el protocolo.** Tauri lo expone de forma distinta según el sistema:
> en Linux y Windows es `http://<esquema>.localhost/...`, y solo en macOS/iOS
> es `<esquema>://localhost/...`. Hay pruebas dedicadas a esto.

## Los formatos que el decodificador no conoce

El motor lee con symphonia MP3, Vorbis, FLAC, WAV y AIFF, y todos empiezan en
la muestra exacta: es lo que hace falta para que el clic caiga donde lo
calculó el núcleo. Opus, WMA y compañía no los lee, y el AAC (M4A) lo lee
1024 muestras tarde. Esos se pasan a FLAC con **ffmpeg** una vez, a la caché
(`player/formats.rs`, con tope de 3 GB), y suena el FLAC: la primera vez tarda
un par de segundos; las demás, nada.

## Lo descargado se llama como en YouTube

La cascada de identificación reconoce una canción por cómo suena. Para lo que
entra por `Entrada/` es lo mejor que hay; para una descarga, no: una «Drum
Cam» de «Que se abra el cielo» suena como el original y la huella la
archivaba como «Miel San Marcos - Que Se Abra El Cielo», que no es quien la
toca. El usuario no reconocía lo que acababa de pedir y volvía a bajarlo.

Para las descargas el nombre lo pone YouTube, limpio (`names.from_video`):
los datos de YouTube Music si vienen; si no, un artista que ya tienes y
aparece en el título; si no, el canal cuando aparece en el título («X - Ish
Melton Drum Cam» → Ish Melton); si no, «Artista - Título», el orden habitual;
y sin guion, el título entero con el canal de artista. `ingest.process`
recibe ese nombre ya decidido (`known`) y no llama ni a la huella ni a la IA.
El aviso de fin de descarga dice «pediste X → entró como Y (id N)».

**La carpeta de descargas se pregunta.** Todo lo descargado va a la carpeta
de la biblioteca (`config.LIBRARY`). Si nadie la había elegido, era una
suposición —`~/Musica`, o la de Música del sistema— y lo bajado podía acabar
en otro disco, fuera de las carpetas que la app mira, sin salir en ningún
sitio. Ahora, mientras no esté elegida (`config.LIBRARY_CHOSEN`), ya no
existe o no es de tus carpetas de música, la primera descarga —desde
Descargas o desde el asistente— pregunta dónde (`utils/downloadFolder.js`),
proponiendo tu carpeta de música con más canciones. Si la elegida no es de
tus carpetas de música, se añade (y se analiza) antes. Se guarda en los
ajustes (`DANPLAY_LIBRARY`), y dentro se usan `Artistas/`, `Pistas/`,
`Secuencias/`, `Tutoriales y Play Along/` y `Entrada/`: las que ya haya, y
las que falten se crean. A partir de ahí no se vuelve a preguntar; se ve y
se cambia en Descargas y en Ajustes. El núcleo rechaza la descarga (`409`)
si llega sin carpeta, por si algo no pregunta.

**Su carpeta, sugerida.** Todo lo descargado entra en `Artistas/<artista>/`,
también una Drum Cam o un tutorial, que la persona guarda en `Pistas/` o en
`Tutoriales y Play Along/`. Si el título de YouTube lo dice con palabras que
no dejan duda (`names.video_kind`: Drum Cam, drumless, tutorial, drum cover,
play along, secuencia, multitrack, playback, «PB» suelto y en mayúscula), el
resultado lleva `kind` y la página de Descargas pregunta «Parece una Drum
Cam: ¿moverla a Pistas?». Solo se sugiere: «(Pista)» no cuenta (un disco
trae sus pistas y se quedan con el artista), ni «cover» a secas (casi
siempre es otra voz cantándola). En 288 títulos de una biblioteca real
acierta el 97,9 % sin sacar de su sitio ninguna canción; lo que se le
escapa son secuencias sin la palabra en el nombre. Moverla
(`library.move_to`) cambia la ruta en el índice y deja todo lo demás:
mismo id, estrellas, listas, estudio y pistas separadas.

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
DanPlay. Lo que no cabe en las etiquetas (el orden de las listas, sus notas y
colores, el estudio, los cifrados) va además en el diario, junto a la música
(ver «Las listas no se rompen»).

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

Los ids de las canciones **no se reutilizan nunca**. Sin `AUTOINCREMENT`,
SQLite da a una fila nueva el id más alto que haya *ahora* más uno: si la
última canción salía del índice, la siguiente heredaba su id y, con él, las
listas en las que estaba. El tope histórico vive en `meta` (lo sube un
trigger) y todo id nuevo sale de ahí (`_next_song_id`).

**El esquema tiene versión** (`PRAGMA user_version`): cada cambio de tablas es
una migración numerada que corre una sola vez y dentro de `BEGIN IMMEDIATE`,
así que si algo falla no queda nada a medias. Una base nueva nace ya en la
última versión. Y la base y las carpetas de datos y de ajustes nacen solo para
tu usuario (`0600`/`0700`): el historial del asistente y las claves de IA no
los lee nadie más en el equipo.

## La biblioteca sigue al disco

Lo que pasa por fuera de la app —mover la carpeta de música, mover o borrar
una canción desde el gestor de archivos, un disco que se desmonta— se refleja
solo, sin pulsar «Analizar». Lo lleva el vigilante (`services::watcher`),
un hilo del núcleo:

- **Al arrancar**, antes de contestar la primera petición, las carpetas que
  ya no existen apartan sus canciones (solo mira si cada carpeta existe: es
  instantáneo). Así la interfaz nunca enseña canciones de una carpeta que se
  movió. Después, en segundo plano, un escaneo incremental pone al día el
  resto.
- **Mientras corre**, `notify` (inotify en Linux) avisa de cada cambio
  dentro de las carpetas. Cuando deja de haber movimiento un par de segundos
  —copiar una carpeta son cientos de avisos— se escanea. Abrir o leer un
  archivo no cuenta: el propio reproductor lo hace con cada canción.
- **Cada pocos segundos** se mira si alguna carpeta apareció o desapareció, y
  una carpeta recién añadida en Ajustes se indexa sola. **Cada diez minutos**,
  un escaneo completo por si algún aviso se perdió (unidades de red, un
  desbordamiento de inotify). `DANPLAY_RESCAN_MINUTES` lo cambia (0 = nunca) y
  `DANPLAY_WATCH=0` apaga la vigilancia entera.

El escaneo solo sube `revision` si algo de lo que se enseña cambió: la
mayoría de los avisos (una estrella que la propia app escribió en el archivo)
no cambian nada, y la interfaz no tiene por qué recargar.

**Lo que se va no se borra: se aparta.** Una canción cuyo archivo desaparece
sale de `songs` —de toda la app— y espera en `songs_missing` con su id y la
fila entera. Sus filas en las listas se quedan donde estaban, y las listas
la enseñan como «No está» (ver abajo). Si el archivo vuelve, la canción recupera
su id y, con él, sus listas en su puesto, los acordes, el análisis y el modo
estudio:

| Qué pasó | Cómo se reconoce |
| --- | --- |
| Vuelve a su sitio (el disco se monta otra vez, se restaura de la papelera) | misma ruta; si además tiene el mismo tamaño y fecha, entra sin releer el archivo |
| Se movió de carpeta | mismo tamaño y mismo nombre |
| Se renombró en su sitio | mismo tamaño y misma fecha (renombrar no la cambia) |
| Se movió la carpeta entera | ver abajo |

El escaneo mira primero qué hay en el disco, aparta lo que se fue y solo
después mete lo nuevo: en ese orden, un archivo movido encuentra su fila ya
apartada y recupera el id en la misma pasada. Pasado un mes sin volver, lo
apartado se olvida, pero no sus filas en las listas: antes de borrarla se
guarda su ficha (artista, título, duración, nombre y ruta) en `song_cards`, y
la lista la sigue enseñando como «No está».

**Una carpeta que se mueve entera.** Si todas las canciones de la biblioteca
estaban en ella, la biblioteca queda vacía y la interfaz enseña «No encuentro
tu música» en vez de una lista vacía sin más. Elegir la carpeta en su sitio
nuevo, como quien vuelve a importar, basta: si al menos la mitad de una
muestra de sus canciones aparece allí con la misma ruta relativa
(`Artistas/Barak/…`), el núcleo entiende que es la misma carpeta
(`relocation_for`), cambia su ruta en vez de añadir otra y devuelve cada
canción con su id sin releer nada. Ajustes marca la carpeta como «no está» y
tiene el mismo gesto a mano («¿Dónde está?»).

**La cola también se entera.** Al abrir la app, la sesión guardada vuelve sin
las canciones cuyo archivo ya no está, también la última que sonaba; si no
queda ninguna, el reproductor arranca vacío. Con la app abierta, cada cambio
del núcleo pone la cola al día (`queue::prune`): Rust pregunta de una vez por
las canciones que no están donde creía (`POST /api/songs/locate`); la que se
movió sigue con su ruta nueva y la que se fue sale. La que está sonando se
deja terminar: el sistema sigue leyendo un archivo abierto aunque se mueva.

**Lo hecho sigue a la canción** (`services::reuse` y `library/src/reuse.rs`,
desde 1.28). Pasaba:
se cambiaba la carpeta de música de sitio, o se volvía a bajar una canción
que ya estaba, y DanPlay la trataba como nueva: separar otra vez, el clic
otra vez, sin marcadores ni listas. Ahora, al entrar una canción, se busca su
**gemela**: la misma (artista y título normalizados, o el nombre) y con la
misma duración (±2 s), en la biblioteca, en las apartadas o en los
manifiestos de `Separadas/`. Si es una apartada, el escaneo le devuelve su id
(sus listas). Se mide cuánto está corrido un archivo respecto al otro
(correlación de los golpes cada 10 ms y luego muestra a muestra; contra el
archivo de antes, sus pistas sumadas o lo que oyó la red) y se adoptan, ya
corridos, sus pistas (`adelay`/`atrim` a FLAC), su clic y lo del estudio. Si
no suenan igual (otra versión con el mismo nombre), no se adopta nada. Las
canciones que solo cambiaron de sitio se llevan su clic sin medir nada
(`beatgrid.moved`). Lo que ya se había perdido antes se recupera con una
pasada por toda la biblioteca, una vez, al arrancar (`reuse::sweep`).

### Las listas no se rompen: las «No está» y el diario

En cualquier reproductor, una lista guarda rutas o ids: si la canción se
mueve, se renombra, se vuelve a bajar o la base se pierde, la lista se rompe.
Aquí no, por dos piezas:

**Las «No está»** (`library/src/playlists.rs`). Una fila de una lista cuya
canción ya no está en `songs` no se esconde: `playlists.songs()` la devuelve
en su sitio con `ghost: true` y su ficha (de `songs_missing` o, pasado el mes,
de `song_cards`), y el resumen de cada lista cuenta cuántas faltan
(`missing`). La interfaz las pinta apagadas con la etiqueta «No está», nunca
entran en la cola, y su menú ofrece buscarla en la biblioteca, buscarla para
bajarla (Descargas, con «artista título» ya puesto) o quitarla. Reordenar la
lista las respeta (`set_songs` acepta los ids de sus filas). Cuando entra una
canción nueva, el escaneo mira si es una «No está» que vuelve en otro archivo
(`relink_ghosts`: la misma identidad que las gemelas de `reuse`, artista y
título o nombre, y la misma duración ±2 s) y la pone en su sitio, aunque las
etiquetas del archivo ya la hubieran añadido al final.

Borrar desde DanPlay es distinto de que desaparezca: la papelera pregunta si
sale también de sus listas (`DELETE /api/song/{id}?from_lists=true`,
`playlists.take_out_everywhere`). Si sí, no queda ninguna «No está»: se
quería fuera.

**El diario** (`library/src/journal.rs`). La base es desechable, pero no todo
lo que el usuario hace cabe en las etiquetas del archivo: el orden de las
listas, sus notas y colores, el estudio, los cifrados. Unos segundos después
de cada cambio (un hilo que mira `library.revision()`), se escribe
`.danplay/journal.json` dentro de cada carpeta de música (viaja con ella a
otro disco) y otra copia en los datos de DanPlay. Lleva las listas por ids y
la ficha de cada canción con algo del usuario o en una lista: ruta, carpeta
raíz y ruta relativa, nombre, tamaño, artista, título y duración, y sus campos
(`FIELDS`).

Al arrancar y tras cada escaneo con altas, bajas o vueltas, `reconcile`
compara el diario más reciente con lo que hay. Cada canción del diario se
busca: en su ruta, en la misma ruta relativa dentro de su carpeta (la
carpeta se movió), por nombre y tamaño, o por identidad y duración. Hay dos
casos:

- **La misma base** (el `library_uid` de `meta` coincide): solo se recupera
  lo de las canciones que cambiaron de sitio. Lo demás ya está en la base, y
  lo que no está se quitó a propósito: no se deshace.
- **Una base nueva** (se perdió, otro equipo): se rehacen las listas enteras,
  en su orden, con su nota y su color; lo que no se encuentra entra como «No
  está» con un id nuevo que no es de nadie. Las listas que ya rehízo el
  escaneo desde las etiquetas de los archivos se completan y se ordenan. Al
  acabar se reescribe el diario con el `library_uid` nuevo.

De lo de cada canción solo se rellena lo que falta: lo que hay en la base
manda. Las listas ya exportadas (`Listas/*.m3u8`) se reescriben con las
rutas de ahora, así que siguen valiendo en otros reproductores. Lo
recuperado sale en `/api/status` (`journal`) y la interfaz lo cuenta una vez.
El hilo no escribe hasta que se ha recuperado: una base nueva y vacía no
puede pisar el diario que la va a rellenar.

## La cascada de identificación

```text
  1  Etiquetas ID3        confianza 0.95 si trae artista Y título
  2  Huella acústica      fpcalc → AcoustID → MusicBrainz     mínimo 0.75
  3  Heurístico           contra las carpetas de Artistas/    umbral 0.80
  4  IA (la que elijas)   para los ambiguos                   umbral 0.55
  5  Revisar/             sin artista, nunca inventado
```

Un escalón que falla no tumba la cascada: si AcoustID devuelve un error de
servicio, decide el siguiente. Esto pasó de verdad y dejó la importación
completamente bloqueada, de ahí el envoltorio `_safe`.

La cascada, el nombre y la carpeta de cada archivo y la Entrada entera van en
Rust (`library/src/ingest.rs`). Lo de fuera (las etiquetas, la huella, la
IA, convertir y mover el archivo) se lo da quien llama
(`library::ingest::Outside`): el núcleo, en `services::ingest`, o una prueba
con el suyo. Hasta la 1.40 se lo daba `ingest.py`, el módulo de Python.

**Por qué no se usa el postprocesador de metadatos de yt-dlp.** Rellena el
artista con el nombre del canal («Fulanito Music», «… - Topic»). Como las
etiquetas son el primer escalón de la cascada, se colaría con 0.95 de confianza
y archivaría la canción bajo un artista inventado. Solo se escriben etiquetas
cuando YouTube da datos de música de verdad (los campos `track` y `artist`, que
vienen de YouTube Music).

## Cualquier proveedor de IA

Todos los servicios de IA hablan hoy el mismo protocolo (el *chat
completions* de OpenAI), así que hay **un solo cliente** y lo que cambia es
la URL, si pide clave y qué parámetros tolera. Tres piezas:

- `library::ai::providers`: el catálogo (sesenta y pico servicios en seis grupos:
  laboratorios, plataformas tipo DeepInfra, nubes corporativas, Asia, en tu
  equipo, otro) y los **perfiles guardados** en `~/.config/danplay/ai.json`
  (0600), uno por proveedor configurado, con uno activo. Se recuerdan todos
  para poder saltar de Ollama a OpenRouter y volver sin pegar claves otra
  vez. Las variables `DANPLAY_AI_*` mandan sobre el archivo (línea de
  órdenes, pruebas), y la `DEEPINFRA_API_KEY` de antes se migra sola. El
  archivo se escribe de golpe (un temporal que nace `0600` y se renombra):
  cortado a medias, antes quedaba vacío y el siguiente guardado pisaba las
  claves. Una clave guardada solo viaja a **la URL con la que se guardó**:
  «Probar» con otra dirección en el formulario no la manda a ese servidor. Y
  las cabeceras y campos extra salen enmascarados, como la clave.
- `library::ai::catalog`: **qué modelos existen**, sin escribirlos en el código.
  Los nombres caducan en meses (OpenAI cambió toda su nomenclatura, Mistral
  retiró los alias `-latest`), así que se consulta
  [models.dev](https://models.dev/api.json), una base de datos abierta con
  200+ proveedores y miles de modelos: el id exacto de cada proveedor, si el
  modelo usa herramientas, si acepta JSON y temperatura, precio, contexto,
  fecha y si está obsoleto. Se pide con **petición condicional (ETag)** cada
  vez que se abre el selector y al arrancar: sin cambios, el servidor
  responde 304 y cero bytes, así que se puede comprobar siempre. La copia
  vive en la carpeta de datos; la app lleva dentro una foto para el primer
  arranque sin red, regenerada en cada paquete por la propia app recién
  compilada (`--models-snapshot`). Y lo que el usuario puede usar **de
  verdad** con su clave lo dice el propio proveedor (`/models`), cruzado con
  el catálogo.
- **Gratis, sin clave**: el grupo `free` del catálogo (LLM7, las rutas
  `:free` de Kilo, OpenCode Zen) son servicios de terceros que hoy contestan
  a peticiones anónimas, comprobados en vivo el 11-09-2026 con herramientas
  incluidas. «Probar gratis, sin clave» (`ai.try_free`) los prueba por orden
  y activa el primero que responda: cambian sin avisar (Pollinations dejó de
  servir anónimos ese mismo mes), así que se prueban en el momento y no se
  da ninguno por vivo. Sin clave, la lista de modelos se filtra a lo que
  sirven a anónimos. No se incrusta ninguna clave «de cortesía» en la app:
  sería extraíble, compartiría el límite entre todos los usuarios y va
  contra las condiciones de los proveedores.
- `library::ai::call`: el cliente, que **tolera**. Anthropic ignora
  `response_format`, muchos servidores locales rechazan
  `tool_choice="required"`, los razonadores de OpenAI no admiten `temperature`
  y quieren `max_completion_tokens`. En vez de fallar en seco, `complete()`
  quita lo que el servidor rechaza, reintenta y se acuerda por (proveedor,
  modelo); lo que el catálogo ya dice que no se admite ni se manda. Si el
  modelo no sabe usar herramientas, el asistente lo cuenta en castellano en
  vez de dar un error genérico. Siguen siendo dos modelos con papeles
  distintos: el **rápido** (identificar nombres sucios, rellenar fichas,
  letras, el juez de narración) y el de **conversación** (el asistente, que
  necesita herramientas). «Probar» hace la llamada más barata posible con cada
  uno y una tercera con una herramienta de prueba.

## Modo estudio

Para machacar un trozo: **bucle A-B**, **velocidad sin cambiar el tono**,
marcadores con nombre y notas por canción.

El tramo se elige sobre la **forma de onda** (`StudyTimeline.vue`): se
arrastra de donde a donde, se cogen sus bordes, se lleva entero, o se marca
con las teclas A y B mientras suena; un clic sin arrastrar quita el tramo
(y, si el candado lo deja, lleva la canción a ese punto), y cerca de un
marcador el borde se pega a él. Debajo va una regla con los minutos (el paso
se elige para que quepan ~70 px entre números).

**Varios tramos.** Con «varios» puesto (junto al candado), cada arrastre
añade un tramo en vez de sustituir el que había, y un clic sobre uno lo
quita. Se repiten seguidos saltándose lo de en medio: al acabar uno, el
siguiente, y del último vuelta al primero. Rust los lleva en orden y juntando
los que se pisan, con el índice del que está **armado** (`ab_loop`); el
`<audio>` del navegador hace lo mismo (`utils/segments.js`). Se guardan con
la canción como `loops`; uno solo sigue siendo `loop`, que es lo que
entienden las versiones de antes. Guardados como marcador van juntos: un
solo marcador con sus `parts`, que al elegirlo los vuelve a poner todos.

**Las opciones de un tramo**, en su botón ⋯ o con el clic derecho en la
onda: reproducir ahora, **repetir cuando acabe la canción**, ir aquí,
ajustar los bordes a los pulsos del metrónomo (a la rejilla tal como suena),
guardarlo como marcador, pasar a varios tramos o volver a uno, y quitar ese
tramo o todos. Repetir al acabar es `defer` en `set_loop`: la canción sigue
hasta el final aunque pase por los tramos, y entonces vuelve al primero y ya
se repiten; saltar a propósito dentro de uno lo arma ya, y mover un borde o
ajustarlo a los pulsos no lo quita. Las opciones que mueven la canción son
órdenes a propósito, así que valen también con el candado puesto.

**El candado de la onda.** Arriba a la derecha, puesto de entrada (se
recuerda en el navegador): mientras suena, un clic en la onda no mueve la
canción —tocando encima de la grabación, uno sin querer la mandaba a otro
sitio— y elegir un tramo, o pulsar la banderita de un marcador, no salta a
él. El tramo queda puesto y Rust lo **arma** cuando la canción entra en él
(`ab_loop`): si va por delante, al llegar; si va por detrás, la canción
sigue y al acabarse vuelve a A en vez de pasar a la siguiente (con un tramo
puesto la canción no se acaba). Un clic parado sacude el candado para decir
por qué no pasó nada. Las teclas A y B y la lista de marcadores sí mueven
la canción, que son órdenes a propósito; y en pausa la onda coloca como
siempre. El reproductor del navegador sigue la misma regla.

Un **marcador** es un tramo guardado con nombre (inicio Y final) y sus
propias notas, aparte de las notas generales de la canción; también puede
ser un instante suelto. Pulsarlo vuelve a poner ese bucle y coloca la
canción al principio del tramo: si sonaba sigue sonando desde ahí, y si no,
queda lista para play. Con el marcador elegido, mover los bordes del tramo
lo cambia a él; dibujar un tramo nuevo de cero no lo toca. Renombrar es un
botón aparte, para que pulsar el nombre no abra nada. En la onda un tramo se
ve como banda y un instante como raya; la banderita con el nombre es lo que
se pulsa, y la banda no recibe el puntero para poder seguir arrastrando por
encima.

La barra es una **capa fija** que sube desde el reproductor con una
transición corta (`<transition name="study">`) y ocupa media pantalla, en
cuatro columnas (reproducción, metrónomo, marcadores, notas): no le quita
alto a la lista de detrás, que antes se recolocaba entera al abrirla y al
cerrarla. Queda por debajo de la cola del reproductor y por encima del
botón flotante. Las notas van en un solo cuadro con dos pestañas (las de la
canción y las del marcador elegido).

**Tono.** Se cuenta en tonos, como lo cuenta un músico —medio tono es un
semitono— y se corre de cuarto en cuarto de tono, de medio en medio o de
tono en tono, hasta una octava arriba o abajo. El cuarto de tono (medio
semitono) sirve para ponerse a la par de una grabación que no está afinada
a 440. Va a Rust en semitonos con fracciones y por el mismo camino que la
velocidad: ffmpeg reabre la canción donde iba con `rubberband=tempo:pitch`
(tiempo y tono a la vez, buena calidad; lo traen el ffmpeg de Debian y el
que va en el paquete de Windows) o, si no está, `asetrate`+`atempo`; los
dos admiten cualquier factor. Se guarda con la canción y se enseña el tono
resultante («G → A») si el índice sabe el tono original; con un cuarto de
tono no hay nombre de tonalidad y se dice de cuál se parte, «G +¼».

**Metrónomo.** Un clic sintetizado en un sink aparte del mezclador de rodio
(`metronome.rs`): tiene su volumen y su play/pausa, independientes de la
canción. El pulso y el «1» los saca **Beat This!** (CPJKU, 2024, MIT) en el
núcleo (`analysis/src/pulse.rs`, `analysis/src/beatgrid.rs`): una red que
mira el espectrograma y dice,
cada 50 avos de segundo, lo probable que es un pulso y que sea el «1». Va
en ONNX Runtime como Demucs, sin PyTorch: el grafo con sus pesos en float16
viaja con la app (42 MB), y el espectrograma, los trozos de 30 s y los
máximos se hacen igual que su código (`export-pulse.py`, el guion que
exportó la red, lo comprobaba: los mismos pulsos). Desde la fase 2 todo eso
está en Rust (`analysis/src/pulse`): la red corre en ONNX Runtime, cargado
desde Rust con `ort`, y la rejilla es la que se hacía con numpy, cuenta a
cuenta (sale lo mismo, bit a bit, en las 283 canciones con lo que
oyó la red guardado). La primera vez tarda (15-20 s por canción en
un portátil: casi todo es la red), en un proceso aparte con prioridad baja
y cuatro hilos; se guarda en `DATA_DIR/beats/` y luego sale al momento. La
interfaz se la pasa a Rust (`set_beat_grid`), que la usa como la suya. Si el
núcleo no puede, Rust la saca con lo de siempre (`beats.rs`, abajo).

Esa espera es la que se notaba («el clic tarda en cargar»), así que nadie
la hace a ciegas. El núcleo la calcula en un hilo propio (`?wait=false`
contesta al momento; lo que se pide para ya va delante, lo de después
detrás) y dice por dónde va. Con **«Clic automático»** (`useClick`, que vive
fuera del panel de estudio), cada canción trae su clic y sus ajustes;
mientras carga, el clic no suena (si venía sonando, se calla), su botón no
deja ponerlo y la onda se va encendiendo con lo que lleva; al llegar, vuelve
solo. Con el clic puesto se preparan las tres siguientes de la cola. A mano,
un tempo para todas, que sigue igual al pasar de canción. Y lo guardado no
se pierde: se reconocía por la fecha del archivo, y DanPlay la cambiaba él
mismo al escribir cualquier etiqueta (el estudio guarda a menudo): ahora,
al reescribir una etiqueta o mover el archivo, la rejilla y la onda pasan a
la clave nueva (`services::tags::retagged`, `services::songs::relocate`).

Medido con `scripts/evaluate-pulse.py` (un guion de Python que queda en el
historial, en `a0e9317^`) en 300 fragmentos de GTZAN (diez
estilos, pulso y compás anotados a mano, y que Beat This! no vio al
entrenar), la F del MIREX (un pulso cuenta si cae a menos de 70 ms del
anotado):

| | Pulso | «1» | Pulso (pop, rock, country, disco) | «1» (ídem) |
| --- | --- | --- | --- | --- |
| `beats.rs` | 0,779 | 0,388 | 0,879 | 0,559 |
| Beat This! pequeño (2 M de parámetros) | 0,854 | 0,701 | 0,931 | 0,868 |
| **Beat This! (20 M), el de la app** | **0,862** | **0,721** | **0,940** | **0,881** |

El «1» casi se duplica.

**La rejilla que suena** (`pulse.grid`, desde 1.22) no es lo que dice la red
tal cual. Pasando la red canción por canción por una biblioteca de alabanza
de verdad (`scripts/check-metronome.py`, también en el historial), en tres
de cada cuatro canciones
la red oye el pulso **a otro nivel durante unos compases** (el «y» de cada
tiempo, o la mitad) y deja golpes sueltos de más o de menos; la rejilla de
antes los llevaba tal cual, rellenaba los huecos con el tempo de toda la
canción (que, con tramos al doble, salía al doble) y el clic se aceleraba
y frenaba dentro de la canción. Con ×2 encima, esos tramos iban a cuatro
veces el tempo. Además la red mira cada 20 ms, y el clic bailaba ±10 ms de
golpe a golpe (un tempo de 148 salía a ratos 143 y 157), y el «1» salía en
compases de 1, 2, 5 u 8 tiempos: quien lo usaba acababa quitando el acento o
poniendo el tempo a mano. Ahora:

- **Un solo nivel de pulso.** Entre los máximos de la red (también los
  flojos, de más del 20 %) se elige por programación dinámica la cadena con
  más probabilidad y menos tirones respecto al tempo de alrededor (llevado
  al nivel del dominante, así que acelerar o un popurrí cambian el tempo y
  no el nivel; hasta un 28 %, que es un popurrí de 87 a 111: más lejos es la
  red confundiendo un tresillo). Saltarse un candidato no cuesta; un pulso
  que falta, sí; después de un hueco largo se puede empezar otra cadena. Dos
  pasadas: el tempo de alrededor de la segunda sale de la primera cadena, que
  ya va en un nivel (con el de la red tal cual, un popurrí que empezaba más
  rápido que el resto se quedaba con un tempo atrasado y la rejilla
  inventaba los pulsos en vez de seguir los de la red). El tempo dominante
  es el que más se repite, intervalo a intervalo, como la mediana de antes:
  ante la duda, el nivel rápido, y el ×2 que ya hubiera puesto alguien sigue
  valiendo lo mismo.
- **Afinado.** Cada pulso en el vértice de la parábola por su trama y las
  dos de al lado, y alisado con la recta de sus cuatro vecinos de cada lado
  donde el tramo va regular: en golpes sintéticos a instantes exactos, de
  ±6,3 ms a ±1,1 ms. (La red llega unos 5 ms tarde a esos golpes, pero
  corregirlo empeora frente a las anotaciones de GTZAN: no se corrige.)
- **El «1» con un modelo de compás** (Viterbi): el tiempo del compás como
  estado; cortar un compás o alargarlo un tiempo cuesta, así que solo pasa
  si los «1» de la red lo piden una y otra vez (el 2/4 antes del coro sigue
  saliendo). Un «1» que la red no oye ya no rompe el compás. 4/4 salvo que el
  3 cuadre claramente mejor.
- **Sin clic donde no hay pulso**: antes del primero (una intro libre),
  después del último (un final en vivo, un acorde que se apaga: la rejilla
  va `closed` y Rust no sigue al tempo) y en un hueco que no es un número
  entero de pulsos (una parte libre). Un hueco a tempo (un puente en el que
  la banda calla) se rellena. El doble no mete golpes en los huecos y la
  mitad se queda con el 1 y el 3 de cada compás, en Rust y en
  `utils/beats.js`.

Medido otra vez con `evaluate-pulse.py` en los mismos 300 fragmentos de GTZAN
(con ±25 ms, lo fino; CMLt y AMLt, la continuidad: que el pulso siga en el
mismo nivel):

| | Pulso | ±25 ms | CMLt | AMLt | «1» |
| --- | --- | --- | --- | --- | --- |
| La rejilla de antes | 0,861 | 0,681 | 0,760 | 0,893 | 0,721 |
| **La de 1.22** | 0,858 | **0,702** | **0,762** | **0,905** | 0,720 |

En fragmentos de 30 s hay pocos cambios de nivel y el silencio de los
extremos cuenta en contra (los fragmentos se cortan en mitad de la
canción); donde se nota es en canciones enteras. En las 280 de la biblioteca
(31,8 horas): los tramos a otro nivel pasan del 72 % de las canciones al
10 % (lo que queda son cambios de tempo de verdad, popurrís y finales en
vivo: un 1,5 % del audio, y se siguen), los golpes sueltos del 76 % al 5 %,
los compases irregulares del 70 % al 1 % y el baile del clic de 5,9 a 1,5 ms. Aflojar la cadena (menos castigo a los tirones)
daba +0,004 en GTZAN y doblaba los tramos a otro nivel en la biblioteca: no.
Contra los golpes de la propia música (el flujo espectral, calibrado con
golpes sintéticos) el clic cae a −1 ms de mediana: no había un retraso
general.

El análisis de reserva, en Rust (`beats.rs`), sin modelos: envolvente de
ataques por FFT (flujo espectral), tempo por autocorrelación con un prior
alrededor de 120 (o del bpm del índice), rejilla de pulsos por programación
dinámica (Ellis 2007, lo que hace librosa) afinada a subtrama, y el «1» por
dónde caen el bombo y los cambios de acorde, probando 4/4 y 3/4. Un par de
segundos por canción. La rejilla se guarda por ruta durante la sesión y se
pinta sobre la onda. Sonando con la canción, el clic se reengancha a la
rejilla en cada play, salto, cambio de velocidad y vuelta del bucle, y sigue
la velocidad del estudio; con la canción parada sigue solo al mismo tempo.

**La cuenta.** Con uno o dos compases de cuenta puestos, play con la canción
en pausa no la arranca: el clic cuenta esos compases al tempo y en el compás
de la rejilla, y la canción entra justo donde caería el golpe siguiente, que
es su próximo pulso (así, parada a mitad de compás, la cuenta va «3, 4, 1, 2»
y entra en el 3). El clic va a su tiempo de muestra en el mezclador; la
canción la arranca el hilo de audio cuando toca, con unos milisegundos de
margen, y en cuanto suena el clic se reengancha a ella. Pausa en plena
cuenta la deja en pausa. Es cosa del estudio: al cerrarlo, play vuelve a ser
play.

**Cuatro sonidos**: clásico (un pitido), madera, baqueta (corto y brillante,
se cuela entre platos) y cencerro (se oye aunque la banda suene fuerte).
Cada uno son unos pocos parciales con su caída (`metronome.rs`, `Sound`), y
el «1» va más fuerte. La misma tabla (la del motor, `engine/src/click.rs`)
la usa `analysis/src/click.rs`, que pone el clic en la
**mezcla que se guarda** «como suena»: los golpes de la
rejilla tal como suena (o del tempo a mano), pasados al tiempo de la mezcla
y sumados después de la velocidad y el tono (que si no lo estirarían). Las
pruebas de los dos lados comprueban las mismas muestras.

La **síncopa 3+3+2**, la de tanta alabanza en directo, engañaba al tempo: el
golpe cada tres corcheas (pulso y medio) sale casi tan fuerte como el pulso,
y con el prior en 120 ganaba, así que una canción a 138 salía a 92 y el clic
iba la mitad del tiempo a contratiempo. Tiene una firma: el compás (lo que
más se repite entre 1,2 y 4,5 s) mide 4, 8, 16 o 32 **tercios** del periodo
elegido, y no un número entero de periodos. Solo entonces el tempo pasa a
dos tercios de ese periodo, en la octava que más se repita. Se probó un
control más ancho («que el periodo quepa en el compás»): arreglaba estas
pero, medido con cientos de canciones, doblaba otras en las que el compás
no sale limpio; el estrecho solo cambia las que tienen esa firma.

Se ajusta a mano, y se guarda con la canción: el compás (2/4, 3/4, 4/4,
6/8, o **sin acento**: todos los clics iguales; el «1» de los que no salen
del análisis se saca de donde caía en 3 o en 4), «el 1 es el siguiente», el
**doble o la mitad** de clics (una lenta a corcheas: 68 → 136) y el **tempo
a mano, con decimales** (90.7). El doble y la mitad valen también sobre el
tempo a mano: antes solo cambiaban la rejilla y, con un tempo puesto, ×2 no
hacía nada (y, como el botón no se veía puesto ni la onda cambiaba, parecía
roto). Con tempo a mano el clic va a su aire, pero entra con la canción: al
ponerlo y cada vez que la canción se mueve (play, salto, vuelta del bucle)
su primer golpe cae en el siguiente pulso de la rejilla y en su tiempo del
compás (`metro::plan`); así un tempo ajustado con decimales se queda con la
canción y en un bucle entra igual en cada vuelta. La onda pinta la rejilla
tal como suena —con el doble, el compás y el «1» corrido—, que la interfaz
rehace igual que Rust (`utils/beats.js`).

**Volumen.** La canción sube hasta un 150 % (también desde el reproductor)
y el clic hasta el doble: para las grabaciones flojas y para oír el clic
por encima de una mezcla cargada. Todo lo que suena pasa por un
**limitador** (`player/output.rs`: la canción y el clic van a un mezclador
propio que sale por el `Limit` de rodio): con el umbral pegado al tope, a
volumen normal no toca ni una muestra; lo que se pasaría baja en esa misma
muestra en vez de recortarse en seco en la salida, y se recupera en 80 ms,
así que con el clic al doble la canción cede un instante en cada golpe. La
rueda de la bandeja y las flechas se paran en el 100 %: pasar de ahí es a
propósito.

Para el pulso se miró primero madmom (muerto desde 2018) y Beat This! sobre
PyTorch (200–550 MB), y se quedó en lo propio. Con Demucs ya en ONNX Runtime
para separar las pistas, Beat This! entró igual, sin PyTorch, y medido.

La onda la
calcula el núcleo en Rust recorriendo el archivo por bloques —no hace falta
tener la canción entera en memoria—, con pico y RMS por columna; lo que
symphonia no lee (opus, wma) lo saca ffmpeg a PCM crudo y lo resume el
núcleo. Se
guarda en disco: la segunda vez es gratis. Para pintarla, el RMS se estira a
su propio máximo y se le da una curva suave, porque en un mp3 de hoy los
picos van todos a tope y el RMS ronda el tercio: tal cual, no se
distinguía el verso del estribillo. La onda guardada **se va con la
canción**: al mandarla a la papelera, y al escanear si desapareció por
fuera (el escaneo poda además las que no correspondan a ninguna canción del
índice). El estudio en sí es una columna de la fila, así que se va solo.

La velocidad la aplica ffmpeg
(`atempo`) al decodificar —cualquier formato pasa por él cuando no es 1×—,
así que el tono no se mueve; rodio cuenta entonces en tiempo de salida y el
reproductor convierte a segundos de la canción en las posiciones, las
búsquedas y el bucle. `atempo` y `rubberband` estiran por ventanas y lo que
suena sale unos milisegundos antes de esa cuenta: medido con golpes a
instantes exactos, unos 7 ms de canción entre 0,7× y 0,95×, 11 ms más
despacio y 13 ms con el tono corrido. El clic llegaba tarde (20 ms de reloj
a 0,5×); la posición suma ese adelanto (`transcode::filter_lead`). A 1× la
canción va por rodio y coincide muestra a muestra con lo que se analizó. Sin ffmpeg, rodio cambia la velocidad a la antigua y
la barra avisa de que el tono cambia. Los tramos viven en el hilo de audio
(al pasar del final de uno, al principio del siguiente), y en el navegador
los recorre el `<audio>`. Lo que se
marca se guarda con la canción, en el índice y en una etiqueta del archivo
(`ESTUDIO`, JSON), y se recupera al escanear si el índice se pierde, como
las estrellas y las listas. Al cerrar la barra, la canción vuelve a sonar
normal.

### El clic sin silencios ni saltos (1.28)

En canciones de verdad el clic tenía dos fallos que se oían: **silencios**
(segundos o minutos sin clic donde la red no oía el pulso, en las partes
suaves) y **saltos de fase** (unos compases a contratiempo y vuelta). Medido
contra la batería separada de esas mismas canciones, los saltos eran tramos
en que la red se iba al «y» y volvía. Ahora `pulse.grid`:

- **repara los saltos de fase**: un tramo que se desvía medio pulso y vuelve
  se lleva a la fase de los lados, votando con dónde golpean el bombo y el
  bajo (el flujo por debajo de 150 Hz, cada 10 ms: el de toda la banda no
  distingue, los platos van a corcheas);
- **rellena los huecos** con pulsos anclados a los dos lados (los que quepan
  enteros, siguiendo a la red donde aún oye algo), sin inventar donde no
  cabe un número entero de pulsos;
- **lleva a tempo fijo lo grabado a clic**: si los intervalos casan con un
  solo tempo (periodograma de Rayleigh) dentro de unos milisegundos, la
  rejilla es ese tempo exacto de principio a fin;
- con la **batería separada**, la red se pasa también por ella y en cada
  instante manda la más segura (`pulse.combine`).

Lo que dijo la red se guarda (`DATA_DIR/beats/<clave>.npz`, float16, unos
100 KB), así que cambiar la forma de sacar la rejilla o **re-analizar**
(`POST /api/song/{id}/beats/reanalyze`: arreglar, tempo fijo, rellenar, con la
batería; de toda la canción o de un tramo, cosido a lo de los lados) tarda
milisegundos. Lo re-analizado a mano se guarda y se vuelve a aplicar si
cambia la versión. (Hasta la 1.40, `danplay pulse`, la línea de órdenes
del núcleo de Python, pasaba el análisis por toda la biblioteca.) Todo eso
va en Rust (`analysis/src/beatgrid.rs`, y lo guardado en
`library/src/beatgrid.rs`, leído y escrito byte a byte como lo escribía el
`json` de Python), y también la cola, su hilo y el proceso del detector
(`services::beats`).

## Separar en pistas

Una canción se separa en sus instrumentos —batería, voces, bajo, guitarra,
piano y el resto, los que tenga— y en el modo estudio suenan ellas en vez de
la canción, cada una con su volumen, su panorama, y callada o sola. Un solo
botón: sin elegir modelo ni calidad, sale siempre lo mejor que se ha medido. Es lo que hacen las webs de *stems*, pero en el equipo: sin
subir nada, sin límites ni trozos de quince segundos, y con el bucle, la
velocidad, el tono y el metrónomo de siempre encima.

**El motor es Demucs v4 (HTDemucs) sin PyTorch.** PyTorch pesa cientos de
megas instalado y no cabe en el paquete; ONNX Runtime pesa 17. La red se
exportó a ONNX (con `scripts/export-separator.py`, un guion de Python que
queda en el historial, en `a0e9317^`) y todo lo que Demucs hace alrededor de
ella con PyTorch se hace en Rust (`analysis/src/separate.rs`, lo que era
`separation.py`, con numpy, cuenta a cuenta igual): el
espectrograma y su inversa (las mismas ventanas, los mismos rellenos, los
dos marcos que se tiran de cada borde), y la canción en trozos de 7,8 s que
se solapan un cuarto y se funden con un peso en triángulo. Sin los
desplazamientos al azar: el mismo archivo da siempre las mismas pistas.
Medido contra Demucs sobre una canción de verdad, la diferencia es ruido
numérico (de 57 a 85 dB por debajo de cada pista).

**Los grafos viajan sin pesos.** Cada peso del grafo es una referencia
externa, y un pequeño manifiesto dice de qué tensor del archivo oficial sale
(tal cual, traspuesto —el exportador guarda así los de las capas lineales—
o con otra forma: el optimizador pliega el `unsqueeze` de las normas y las
escalas). Todos, también los pequeños: así un mismo grafo sirve para varios
juegos de pesos, y el de htdemucs es también el de sus cuatro especialistas
afinados (htdemucs_ft), que el exportador comprueba uno por uno contra
Demucs. Los archivos oficiales son los del autor en HuggingFace, en
float16; se bajan la primera vez que se separa algo (223 MB: la red rápida,
55, y los especialistas de batería y bajo, 84 cada uno), se comprueba su
sha256 y se le dan a ONNX Runtime al cargar (`add_external_initializers`),
pasados a float32, que es exacto. Así la app no reparte pesos que no todos
usarán, y el repositorio no carga con ellos.

**Qué redes, medido.** `scripts/evaluate-separator.py` (en el historial,
como los otros guiones de Python) separaba con las mismas piezas que la app
las 50 canciones de prueba de MUSDB18 (sus muestras de 7 segundos, que
ningún modelo de Demucs vio al entrenar; solo para medir, la app no las
lleva) y daba el SDR de cada fuente: lo que se parece la pista a
la de verdad, en dB. Y el de la canción «sin batería», que es lo que oye un
baterista al callarla. En este portátil:

| Combinación | Batería | Bajo | Voces | Sin batería | Sin voces | × canción |
| --- | --- | --- | --- | --- | --- | --- |
| htdemucs_6s (1.16.0) | 9,21 | 7,66 | 8,52 | 12,38 | 12,05 | 0,57 |
| htdemucs_6s, «Otros» lo que queda | 9,21 | 7,66 | 8,52 | 13,44 | 13,26 | 0,57 |
| htdemucs (el de cuatro), igual | 9,55 | 8,40 | 8,38 | 13,78 | 13,12 | 0,61 |
| los especialistas de batería, bajo y voz | 9,54 | 8,84 | 8,51 | 13,77 | 13,26 | 1,71 |
| **la de la app**: la de seis, y batería y bajo de sus especialistas | 9,54 | 8,84 | 8,52 | 13,77 | 13,26 | 1,71 |

(`× canción`: lo que tarda por segundo de canción.) De ahí sale todo lo
demás: que «Otros» sea lo que queda gana más de un dB en lo que suena al
callar una pista; los especialistas sacan mejor la batería y sobre todo el
bajo; el de la voz no mejora a la red de seis, así que no se usa (un tercio
menos de tiempo). Cuantizar las redes a 8 bits no cambia el SDR pero solo
ahorra un 5 % del tiempo: el trabajo está repartido entre convoluciones,
multiplicaciones y operaciones elemento a elemento, y no compensa llevar
otro grafo.

**Dos pasadas.** La mejor combinación pasa tres redes por la canción: de
punta a punta, unos diez minutos para una de 4:31 en este portátil. Pero su
primera red es la de seis, que hace falta igual (la guitarra y el piano
solo los saca ella): con lo que da ya se escriben todas las pistas y se
pueden usar, a los tres minutos. La mejora se lleva los otros seis y medio. Luego los especialistas rehacen la batería y el
bajo (y «Otros») y se cambian de golpe, con los mismos nombres; el
reproductor ve que cambiaron (fecha y tamaño de cada archivo) y las reabre
donde iba, sin cortar. En la cola, las pasadas rápidas van antes que
cualquier mejora: un repertorio entero se puede ensayar cuanto antes. Si la
app se cierra a mitad de una mejora, las pistas se quedan en «rápidas» y
pedir la canción otra vez solo las mejora.

**Solo lo que está.** Una fuente que la red no encuentra no sale callada:
sale con lo poco que se le cuela de las demás. Con la red rápida se mide lo
que suena cada una frente a la canción, por tramos de un cuarto de segundo:
se queda si en conjunto está a menos de 30 dB de la canción, o si en al
menos un 2 % de lo que suena llega a 15 dB de ella (un piano que solo entra
en el puente). Si no, no llega a pista, y lo suyo acaba en «Otros». El
manifiesto guarda lo medido de cada una y cuáles se quitaron. Medido con
`evaluate-separator.py --presencia`: en canciones de verdad, lo que no está
sale de −44 a −63 dB (el piano de una canción sin piano, el bajo de una
pista de acompañamiento que lo deja para el bajista, la batería y la voz de
un preludio de piano) y lo que está, por encima de −20; el umbral queda en
medio, con margen. En MUSDB18 se quitaron 2 de 150 fuentes que sí sonaban:
la red rápida no las encontró (−49 y −36 dB), y los especialistas tampoco
(−55 y −68), así que decidirlo con la rápida no pierde nada.

**«Otros» es lo que queda.** No es la salida de la red para «el resto»,
sino la canción menos todas las demás pistas tal y como quedaron guardadas:
juntas suenan exactamente como la canción, y al callar una, lo demás está
entero. Se calcula al final, leyendo las pistas ya escritas por tramos.

**La atención, por bloques.** ONNX Runtime guarda entera la matriz de la
atención del transformer (8 cabezas × 2688 × 2688, 230 MB) donde PyTorch
no; el grafo la calcula por bloques de 512 filas, que da exactamente lo
mismo. Con eso y sin el plan de memoria de ORT (reserva de golpe todo lo que
la red tendrá vivo en algún momento), el pico baja de casi 5 GB a menos de 3.

**En un proceso aparte, con prioridad baja** (la propia app con
`--separate`, o `danplay-server --separate`). La red usa todos los núcleos
durante un par de minutos: así la música y la interfaz no lo notan, la
memoria se devuelve al acabar, y parar es matar el proceso. Si el núcleo se
muere, el proceso se da cuenta (cambia de padre) y se va. Cuenta cómo va
por la salida estándar, una línea JSON por aviso, y escribe cada pista en
FLAC según sale (seis pistas de cinco minutos en memoria serían 640 MB),
con su forma de onda: todas a la misma escala, para que en el mezclador se
vea qué instrumento suena más y dónde entra cada uno.

**Tarda**, en un portátil corriente (un Ryzen 7 de 15 W), unos tres minutos
hasta tener las pistas de una canción de cuatro y media, y unos seis y
medio más hasta mejorarlas. Por eso hay una cola: se piden varias (del menú
de una canción, de varias elegidas o de un repertorio) y se separan una
detrás de otra mientras se sigue usando la app.

**Dónde quedan.** En `Separadas/<nombre del archivo>/` dentro de la carpeta
de la biblioteca de la que cuelga la canción: `Bateria.flac`, `Voces.flac`…
archivos normales que abre cualquier programa. En FLAC, o en Opus a 128
kbps si se elige en Ajustes (cuatro veces menos; a 96 serían cinco, pero una
pista sola deja oír más lo que quita el códec). El Opus se hace al final de
la mejora: hasta entonces todo va en FLAC, para que «Otros» salga de pistas
sin pérdida. Se separa en una carpeta
oculta al lado (el escaneo se salta las ocultas) y al acabar se pone en su
sitio de golpe; si la app se cerró a mitad, la siguiente separación limpia
lo que quedó. Junto a las pistas va `.danplay-stems.json`, que dice de qué
canción son: con él, el escaneo no mete las pistas en la biblioteca como
canciones sueltas y, si el índice se pierde, las vuelve a unir con su
canción (`stems.relink`). En el índice solo va dónde están (`songs.stems`,
de la migración 4). Se van a la papelera con la canción.

**Sonar juntas sin desfasarse.** Un solo ffmpeg lee todas las pistas a la
vez y las junta (`amerge`) en un flujo de dos canales por pista: empiezan en
la misma muestra, y la velocidad, el tono, los saltos y el bucle les pasan a
todas por igual, por el mismo camino que a una canción que pasa por ffmpeg.
El render del motor las suma con el volumen y el panorama de cada una, que
le llegan por su buzón: callar la batería no reabre nada, y el cambio se hace
en unos milisegundos (en seco se oye un clic). La
prueba de que no se desfasan es una pista y su contraria, que se anulan del
todo.

**La mezcla, a un archivo.** Lo que suena se guarda en mp3, flac o wav —la
canción sin batería para practicar con el móvil—, tal cual o como suena (a
otra velocidad y tono). El volumen y el panorama se aplican con la misma
cuenta que el reproductor, y un limitador al final evita que la suma se
recorte. Solo dentro de la biblioteca y sin pisar una canción suya; si no
cae dentro de una carpeta de pistas, entra en la biblioteca como una más.

### Más instrumentos: la máxima calidad

Una tercera pasada opcional, lenta, sobre las pistas buenas: el **Mega53**
(«MVSep Mega 53 Stems», BS-RoFormer de ZFTurbo, MIT) oye 53 instrumentos con
un cuerpo común y una cabeza por instrumento; DanPlay exporta el cuerpo y 15
cabezas (con `scripts/export-mega53.py`, en el historial) y reparte con
máscaras suaves (`analysis/src/mega.rs`, lo que era `danplay/mega.py`):

- la **batería** (de los especialistas) en el kit y la **percusión** de
  aparte (congas, pandereta, shakers): kit y percusión suman exactamente la
  batería;
- las **voces** en la principal y los coros;
- lo **melódico** que queda (la canción menos batería, bajo y voces) entre
  guitarra eléctrica, guitarra acústica, piano, órgano, teclados (con los
  sintes), cuerdas, metales y vientos: cada uno, lo que oye su cabeza (con su
  fase) sin pasar en ningún punto de lo que hay; lo que nadie reclama, a
  «Otros». Medido contra la verdad de una secuencia de MultiTracks (sus
  pistas sueltas, `.dev/eval`): en el primer minuto, repartir en proporción
  daba 1,3 dB en las guitarras y 4,5 en los teclados; así, 2,2 y 5,3. En la
  canción entera, las guitarras pasan de 0,5 dB (la pista «Guitarra» de
  siempre) a 2,3, y los teclados de nada (no salía pista) a 2,9. Es una
  mezcla difícil (cinco teclados, pads, loops): en las pruebas de su autor,
  la guitarra eléctrica llega a 8.

Solo sale lo que suena (`presence`), y «Otros» sigue siendo la canción menos
todo lo demás. Los pesos son el checkpoint original del autor (1,4 GB, con su
sha256), leído **sin PyTorch** por un lector propio que no ejecuta el pickle
(`analysis/src/weights.rs`, lo que era `danplay/torchzip.py`: una máquina de
pickle propia que solo reconstruye tensores, y no deja que uno se salga de su
almacén). Trozos de 5 s que se solapan un cuarto; la red no reserva memoria de
más (sin la *arena* de ORT) y antes de empezar se mira que haya memoria libre.
Medido en un portátil (Ryzen 7 5825U, 14 GB): con trozos de 10 s y 20 cabezas
llegaba a 8 GB y el sistema mató el proceso; con 5 s y 15 cabezas, 4,5 GB de
pico y 14 s por trozo (unos 15 minutos para una canción de 4).

## Secuencias descargadas

`library/src/sequence.rs` abre la carpeta de una secuencia (MultiTracks, Loop
Community): las pistas (`MultiTracks/*.wav`), el «Click Track», la «Guide» y
el proyecto de Ableton (`.als`: XML comprimido, leído sin DTD). Del proyecto
salen el tempo, los cambios de compás (Ableton guarda el compás como
`99·log2(denominador) + numerador − 1`) y los localizadores en pulsos; del
nombre de la carpeta, «Título-Artista-Tono-BPM». Queda: la canción mezclada
sin clic ni guía en `Secuencias/`, las pistas en FLAC como sus pistas
separadas con su nombre y su papel (el clic y la guía van a la salida del
clic), las partes como marcadores y la rejilla **exacta** del tempo del
proyecto, que no se recalcula aunque cambie la versión del análisis. Abrir con
DanPlay una pista de una secuencia (`open.rs`) ofrece abrirla entera.

## Para el atril

Un repertorio se exporta también como **hoja para el atril**: un HTML en
`Listas/` con cada canción, su tono (americano y latino), bpm, cejilla
sugerida y los acordes por partes si se encontró su cifrado (con la fuente), y la letra si se
pide. Se abre con el navegador y se imprime o se guarda como PDF desde ahí:
sin ninguna librería de PDF que empaquetar. `theory.related_keys` da los
tonos vecinos de uno (relativo, dominante, subdominante) para armar un set
sin saltos, y el asistente lo tiene como herramienta.

La **letra con tiempos** de LRCLIB se guarda en `lyrics_synced` (una
caché: si se pierde, se vuelve a pedir) y, con la canción sonando, la ficha
resalta la línea que va y salta al pulsar otra. Para tenerla a la vista se
desplaza **solo la caja de la letra**: con `scrollIntoView`, que mueve todo
lo que la contiene, WebKitGTK subía la ficha entera línea a línea mientras
la letra se quedaba parada en medio. Si se mueve la letra a mano, se la deja
estar unos segundos. En el USLT del archivo va la
letra limpia: antes iba con las marcas `[01:23.45]`, y tras un escaneo la hoja
del atril las imprimía (ahora además las quita si le llegan).

## Reglas de nombres

- Sin acentos. Única excepción: la **ñ** se conserva.
- Nada en MAYÚSCULA SOSTENIDA.
- Formato `Artista - Titulo (feat. X).mp3`
- Duplicados: el nombre acaba en `- r`, `- r2`… (`Artista - Titulo - r.mp3`),
  para compararlos y borrar a mano.

La limpieza quita el ruido típico de las descargas («VIDEO OFICIAL», «LETRA»,
«Official Music Video», «(Audio)», «Visualizer», `y2mate.com`, `320kbps`) con
una sola expresión regular compilada, quita los caracteres de control, y
separa palabras pegadas (`VERSIONButterflyFull` → `VERSION Butterfly Full`).

Y al revés, al escanear: un archivo que ya sigue la convención («Palisades -
Personal.mp3») pero no trae etiquetas ni cuelga de `Artistas/` entra en el
índice con ese artista. No es inventar: es lo que dice su nombre. El archivo
no se toca.

**«Transformar»** (`library/src/index/tidy.rs`) aplica estas reglas a lo que
ya está en la biblioteca, de una en una y solo cuando se pide. Se midió
primero en una biblioteca de verdad (281 canciones): con el artista y el
título de las etiquetas, 118 «no seguían la regla», pero renombrarlas así las
dejaba peor muchas veces (canales de YouTube como artista, webs pegadas al
título, el «(ft. X)» perdido). Así que el nombre que ya tiene la forma
«Artista - Título» manda, una canción de `Artistas/<X>/` es de X, lo que solo
dicen las etiquetas va para revisar, y solo se mira lo suelto en la raíz y
`Artistas/` (las carpetas propias, nunca). Salen 19, todas con sentido. Es la
misma canción (`services::songs::relocate`: mismo id, con todo lo suyo) y, si
es la que suena, sigue sonando: la cola le dice al reproductor su ruta nueva.

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

## La ventana a cualquier tamaño

La ventana se puede hacer tan pequeña como se quiera (260×90 como mínimo;
antes 800×600) y la interfaz cambia de diseño según lo que queda, en
píxeles de la interfaz (con el «tamaño de la app» al 125 %, una ventana de
820 son 656):

- **ancho** (más de 1.100): todo en una línea, como siempre;
- **mediano** (hasta 1.100): la barra del reproductor en dos pisos, la aguja
  arriba a todo lo ancho; por debajo de 900 se van la velocidad y los saltos
  de 10 s (siguen en el estudio y en las flechas);
- **estrecho** (hasta 700, el lateral y la ficha ya se abren encima): la
  aguja arriba y debajo la canción, anterior, play, siguiente y «⋯», que
  lleva todo lo demás (aleatorio, repetir, ±10 s, velocidad, volumen,
  estudio, proyectar, cola, forma de la ventana); en la lista se van la
  columna «nombre» y, por debajo de 560, la del artista, que pasa a la línea
  del título en gris;
- **columna** (hasta 460): el reproductor en tres pisos, como en un móvil:
  la canción a todo lo ancho, la aguja y los mandos;
- **barra** (menos de 200 de alto) y **cuadrito** (menos de 440×460): solo el
  reproductor (`CompactPlayer.vue`), la biblioteca escondida pero montada.

Antes, por debajo de 700 la aguja se colocaba encima de todo y se pisaban la
hora, el título y los botones; entre 700 y 1.000 la aguja se quedaba sin
ancho y los botones de la derecha se salían. Lo largo (títulos, artistas,
repertorios) se corta con «…» y se lee entero al pasar por encima; en la
ficha y en el encabezado de un repertorio, en varias líneas. Con poco alto
el lateral se desplaza entero (los repertorios se montaban encima de
«Gestión»). Un detalle de WebKitGTK: una columna escondida le sigue
guardando el ancho a su `<col>`; van a ancho 0. Probado en el motor de verdad
(WebKitGTK sin ventana) a diez tamaños, con un título de 130 caracteres,
buscando lo que se pisa o se sale.

**Las formas** (`shape.rs`, en el menú Vista, en el botón de la barra o en
«⋯»): normal (como estaba), pantalla completa (F11), maximizada, media
pantalla izquierda o derecha, columna (estrecha y alta, a la derecha),
cuadrito (en una esquina) y barra (abajo). Cada una es un tamaño y un sitio
en la zona útil de la pantalla donde está la ventana (sin la barra de
tareas), y el tamaño activa su diseño. El cuadrito y la barra van sin marco
y siempre encima, para dejarlos a un lado mientras se toca; se arrastran
desde el propio reproductor (`data-tauri-drag-region`) y llevan un panel con
las formas, porque en una barra de cien píxeles un menú de diez opciones no
cabe. «Siempre encima» se puede poner en cualquiera. Al salir de la normal
se apunta dónde estaba y «normal» la deja igual. En Wayland el compositor no
deja colocar ventanas: el tamaño cambia, pero el sitio lo decide él.

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
fallo de medida. Y se mide con decimales: con posiciones enteras, el redondeo
se acumulaba en una lista larga y las últimas filas no se alcanzaban nunca.

Una vista **agrupada** («Artistas», o agrupar por álbum o por tono) es también
una sola lista virtual, con las cabeceras de grupo como filas: antes era una
lista por grupo, y con grupos de menos de ochenta canciones se pintaba todo.
La selección con Mayús, el orden de la cola y las flechas siguen el orden que
se ve, grupo a grupo (`utils/groups.js`). La tabla agrupada no lleva cabecera
y su primera fila es la del grupo, una celda que abarca todas las columnas:
con `table-layout: fixed` eso las repartía a partes iguales, y el título
quedaba tan estrecho como el número. Por eso los anchos van en un
`<colgroup>` con la clase de cada columna, en las dos tablas; y las columnas
que se esconden al estrechar la ventana se esconden por su clase, para que se
vaya también su `col`.

En las cuatro vistas cada canción lleva **copiar**: un icono junto al título
copia el título, y «nombre», al final, el nombre completo tal como el del
archivo sin la extensión (o «Artista - Título» si no hay archivo), que es lo
que se pega para buscarla fuera. Van tenues hasta pasar por la fila, no
eligen la fila ni la ponen a sonar, y un aviso dice qué se copió.

La biblioteca se recorre también **con el teclado**: Tab entra en la lista,
las flechas se mueven, Enter pone la canción, Mayús+F10 abre su menú. Tras un
clic con el ratón la fila suelta el foco, para que el espacio y las flechas
sigan siendo del reproductor. Los diálogos atrapan el foco y lo devuelven al
cerrarse, y Enter con el foco en «Cancelar» cancela (antes aceptaba: con
«Mandar a la papelera» delante).

Y la lista llega **entera y ligera**. `/api/search` dice cuántas canciones
cumplen la consulta (`count`) y la interfaz trae una primera página rápida y
el resto por detrás: antes se cortaba en 1000 sin avisar. Las filas no llevan
la letra, los acordes ni el modo estudio (con mil canciones eran megas que no
pintaba nadie), sino `has_lyrics`, `has_chords`… para los iconos; la ficha
entera sale de `/api/song/{id}` al abrirla.

## Arrastrar canciones

No se usa el arrastre nativo del navegador: dentro del WebView se comporta
distinto según el sistema y pinta un fantasma que aquí no sirve. Va con
eventos de puntero (`useDragSong`), con un umbral de unos píxeles para que un
clic tembloroso no mueva nada, y sin tocar nada con el dedo, que ahí la lista
tiene que desplazarse.

Quien recibe no se registra en ningún sitio: basta un `data-drop` en el
elemento. Los repertorios y Favoritos del menú lateral lo llevan siempre. Las
filas de un **repertorio** lo llevan también (`sort:id`), y así la misma
canción se puede dejar sobre otra lista o **recolocar dentro de la suya**: el
composable mira por qué mitad del destino va el puntero (arriba/abajo en las
filas, izquierda/derecha en las fichas, que van en cuadrícula) y la app manda
el orden entero a `POST /playlists/{id}/order`, la misma ruta que ya usaba el
asistente. La lista se recoloca al momento y vuelve atrás si el núcleo no
puede. Solo en los repertorios que uno crea: en «Todas» o Favoritos el orden lo
dan las columnas, y en la lista del reproductor, cuándo se abrió cada archivo.

Mientras se arrastra por el borde de una lista ordenable (`data-sort-list`),
el panel se desplaza solo, por cuadros de animación y no por eventos: con el
puntero quieto en el borde no llega ningún `pointermove`, y sin eso no habría
forma de llevar una canción del final de un repertorio largo al principio.

## El asistente

27 herramientas sobre el mismo núcleo que usa la interfaz. No hay un camino
paralelo: cuando el asistente corrige un título, llama a la misma función que
el botón de renombrar.

**Biblioteca primero, y lo mecánico lo hace el código.** Cada semana llega por
WhatsApp la lista del domingo (negritas, emojis, @menciones de quien canta,
«Voz», enlaces de YouTube) y se le pega al asistente. Con el modelo haciéndolo
todo, eran 3–6 llamadas y 20 s por mensaje: tomaba las @menciones por
artistas, buscaba con todas las palabras obligatorias (y no encontraba lo que
había), bajaba canciones que ya estaban y reescribía la lista cuando solo se le
pedía verificarla. Ahora:

- `library::assistant::intake` lee la lista pegada por código
  (`parse_setlist`): título, número, enlace de un vídeo en forma canónica, las
  personas aparte (nunca son el artista) y lo que no es una canción («Banda:»,
  «Ensayo:»), para poder decirlo. `read_intent` lee lo que la persona pide con
  sus palabras: verificar, no descargar, dejarla exacta, una lista nueva, esa
  versión.
- `library::assistant::matching` empareja cada canción con la biblioteca en la
  forma de buscar de la casa (`names.search_form`, en los dos lados), por
  cobertura de palabras, con erratas, ñ≈n como segunda pasada y el artista
  solo sumando. Un enlace ya bajado manda si la canción sigue siendo esa;
  entre varias versiones gana la que ya está en alguna lista.
- `library::assistant::setlist` (con `services::chat::setlist`) es lo que hay
  detrás de `fill_playlist` y `find_songs`: mete lo encontrado de una vez y en
  el orden de la lista pegada (`playlists.place`, bajo el cerrojo de las
  listas), y lo que falta lo propone en **una** tarjeta de permiso que sabe
  meter cada descarga en su sitio al terminar. Las guardas son de las palabras
  de la persona, no del modelo: «verifica» nunca cambia nada; dejar la lista
  exacta solo si lo dijo (y con lo de antes para «Deshacer»); una lista nueva
  solo si la pidió.
- Si en una vuelta todo fue meter o comprobar listas y salió bien, **el informe
  lo escribe la app** (en pasado, con nombres, y prometiendo solo la tarjeta
  que de verdad hay): una lista pegada cuesta una llamada al modelo y 1–2 s.
- El modelo ve la lista ya leída en una nota de sistema, **como datos**
  (`safe.data_block`): un título de vídeo o una línea pegada con forma de orden
  no se lee como una orden.
- Tope de 8 llamadas al modelo por turno (vueltas, reintentos, el empujón y el
  juez); al agotarse contesta la app con lo que llevaba hecho.

**En la interfaz nada bloquea.** La caja de escribir es un textarea (Enter
envía, Shift+Enter baja de línea, pegar conserva las líneas) y lo que se
escribe mientras el asistente responde queda en cola. El permiso para
descargar es una **tarjeta en línea** bajo la respuesta, no un diálogo dentro
del turno: filas con lo pedido junto a lo encontrado, casillas, «Descargar» y
«No»; confirma por id (`POST /api/chat/confirm {id, selection}`) y su estado
(aceptada, rechazada, sustituida, caducada) se deduce de los mensajes de la
app que llevan su `ref`, y de `GET /api/chat/confirm/{id}` al reabrir. Un «sí»
o un «no» corto escrito contesta la tarjeta viva sin llamar al modelo. Borrar
sigue preguntando en un diálogo, fuera del turno, y los diálogos van en cola.
La tabla de una lista trae «Dejar solo estas», «Ordenar como mi lista» y
«Deshacer», que van directos a la lista. Una franja bajo la cabecera dice
cuándo el proveedor elegido falla y quién contesta en su lugar.

**Reproducir no lo puede hacer él.** El audio y la cola los maneja Rust, así
que las herramientas de reproducción devuelven una `action` que la app ejecuta
al recibir la respuesta.

**Lo que no tiene vuelta atrás lo aprueba una persona.** Borrar una canción,
borrar un repertorio y descargar no se ejecutan dentro de la conversación: el
núcleo devuelve lo que *iba* a hacer y la interfaz lo pregunta. El texto del
sistema ya pedía consultar antes, pero un texto no es una barrera: por la
búsqueda web, por los títulos de YouTube y por las letras entra contenido que
escribe cualquiera, y bastaría una línea bien puesta para disparar un borrado.
La propuesta se **registra en el servidor** (`services::chat::plan`) con un id que
genera él; la interfaz solo puede decir que sí a ese id, y lo que se ejecuta
sale del registro, no de lo que mande el cliente ni de lo que escriba el
modelo. Una llamada escrita como texto en vez de hecha («búsqueda:
search_songs …») solo se ejecuta si es una consulta o meter/comprobar una
lista: nunca borrar, bajar, crear ni dejar una lista exacta.

**El texto de fuera es un dato, no una instrucción.** Lo que devuelven la
búsqueda web, los títulos de YouTube y las letras lo escriben terceros. Las
instrucciones del asistente dicen explícitamente que si ahí aparece algo con
forma de orden, no viene del usuario y no se obedece.

**Los ids no se adivinan.** El modelo se inventaba ids de canciones y de
listas: creó un repertorio con tres canciones ajenas y pidió borrar «domingo»
queriendo borrar «Herlin». Ahora las herramientas de listas rechazan
cualquier id que no exista (todo o nada), aceptan el **nombre** del
repertorio, devuelven qué canciones quedaron de verdad, y hay una para ver
una lista (`playlist_songs`) y otra para dejarla exactamente como debe
(`set_playlist_songs`). El aviso de fin de descarga le da los ids exactos de
lo que entró.

**Narrar no es hacer.** El modelo puede escribir «Descargando… la app te
avisará» o «Añadida a la lista (id: 278)» sin haber llamado a nada, y al turno
siguiente leer esa frase suya como un hecho. Llegó a imitar la marca que la
app añadía al historial. Las defensas, en `chat.reply`:

- **El estado real** de la app al final de cada turno (qué repertorios
  existen, si hay descarga en marcha, si su último mensaje fue solo texto).
- **Notas de sistema** junto a cada mensaje suyo anterior: qué herramientas
  usó **y qué devolvieron** (ids y nombres, en corto: es su memoria entre
  turnos, lo que le permite entender «esa» o «la segunda»), o que no usó
  ninguna y lo que dice haber hecho no ocurrió, o que ese mensaje lo escribió
  la app. Van como `system`, no pegadas a su texto, para que no las copie; si
  copia una, se borra y cuenta como afirmación falsa. La ventana de historial
  se mide por mensajes y por tamaño y empieza siempre en un mensaje del
  usuario: los mensajes de la app no se comen la petición original.
- **Detector de narración**: si contesta solo texto y no ha llamado a ninguna
  herramienta que *haga* algo (`ACTING_TOOLS`), se mira si el texto afirma
  una acción —una lista amplia de frases y, si no salta, el propio modelo como
  **juez** de una palabra— y en ese caso se le devuelve la pelota una vez con
  `tool_choice="required"`. Tras solo consultar («busco y digo "añadida"»)
  decide únicamente el juez: «ya está en tu biblioteca» tras buscar es un
  dato.
- **Un sí a una pregunta suya** («¿la bajo?» — «dale») fuerza herramientas
  en la primera vuelta: es donde más narraba.

Cuando termina una descarga que se propuso desde una lista, el propio turno
de descarga mete cada canción en su sitio antes de soltarse
(`youtube.run_many(on_done=…)` → `plan.apply_after`, una sola escritura, sin
duplicar y comprobando que la lista sigue siendo esa), y la interfaz cuenta lo
que entró sin llamar al modelo. Solo si eso no llega (una descarga pedida
suelta, o un fallo), la app le pasa el turno al modelo para que remate lo que
quedara, con los ids exactos de lo que entró.

**El asistente ve lo que tú ves.** Con cada mensaje viaja lo que hay en
pantalla (la vista y sus primeras veinte canciones en orden, la selección,
lo que suena) y entra en el estado real del turno: «pon la segunda», «esta»,
«las seleccionadas» dejan de ser adivinanzas. Se le dice que esa lista es
solo lo visible: para contar, buscar u ordenar sigue usando `search_songs`.

**La respuesta llega en vivo y se puede parar.** El núcleo pide al modelo
la respuesta en trozos (`stream`), junta las llamadas a herramientas que
llegan partidas y va entregando el texto según sale; si tras un texto
aparece una herramienta, ese texto era un preámbulo y se retira. La
interfaz lo lee sondeando `/api/chat/poll` cada 250 ms por el puente de
siempre: sin flujo abierto ni cambios en Rust, y vale igual en el
navegador. «Parar» corta el flujo y deja lo escrito, señalado.

**Si el proveedor falla, responde otro, y se dice.** Caído, sin crédito,
saturado o con la clave rechazada, `ai.complete` pasa al siguiente perfil
configurado (cada uno con su modelo) y la respuesta dice quién contestó y por
qué (`via.reason`). `classify_failure` mira primero el texto inequívoco y
después el código (un 429 de OpenAI sin saldo no es «espera»; un 403 de
moderación no es una clave mala), y el elegido se da por caído según el motivo,
contado desde el fallo: 20 min sin saldo o con la clave mala (no se arregla
solo), 2 min por límite de peticiones, 1 min caído. Así, un principal sin
saldo cuesta una llamada fallida y no una por mensaje. `ai.status()` lo cuenta
a la franja del chat sin repetir ni una palabra del proveedor (puede llevar
trozos de la clave: `sanitize_provider_message` limpia todo lo que va a
pantalla o al registro). Un error del mensaje no dispara el respaldo: no lo
arreglaría.

**Lo que cuesta, a la vista.** Cada llamada se apunta en `ai_usage`
(proveedor, modelo, tokens y el coste según el precio del catálogo); cada
respuesta enseña sus tokens y su coste, y Ajustes lo de hoy y lo del mes.

**Las conversaciones se guardan en la base** (`library/src/chats.rs`): varias, con
título, y se busca en todas. Lo que había en el `localStorage` de versiones
anteriores pasa a la base una vez.

**Cada token se paga, y el prompt viaja en cada llamada.** El texto del
sistema se escribe una regla por fallo real y sin adornos (unos 1.250 tokens;
era 2.077), las herramientas se declaran con una fábrica que no repite el
andamiaje y describe cada una en una frase; las que hacían lo mismo se
fusionaron (`edit_song` puntúa y marca favorito; `get_lyrics` vale por id o
por nombre; `play` pone una canción o un repertorio; los nombres viejos siguen
valiendo como alias), y las de descargar, las de músico y la de letra+carátula
solo se declaran cuando la conversación habla de eso (un turno normal lleva 17
herramientas, unos 1.700 tokens; eran 2.764), y **los resultados de las
herramientas van en TOON** (`library/src/toon.rs`), no en JSON: una lista de
canciones se manda como tabla, con las claves una sola vez en la cabecera y
una fila por canción. Medido sobre resultados reales, la mitad de tokens en
una búsqueda (54 %) y un 47 % en conjunto. El modelo recibe una explicación de
dos líneas del formato; lo que él devuelve sigue siendo JSON, que es lo que
garantizan los modos JSON de los proveedores. Y una ficha de IA guardada con
confianza baja (la dio un modelo que no conocía la canción) no se reutiliza:
se vuelve a preguntar, que con otro modelo puede salir. El tono y los acordes
no se le preguntan a la IA (§ Acordes y tono, de un cifrado publicado).

**Descargar se pide, no se espera.** Antes de pedir permiso, el núcleo mira
la biblioteca: lo que ya está no se propone, se dice dónde está. La tarjeta
sale al momento y sus filas se rellenan solas: un hilo aparte busca en YouTube
lo que falta (dos consultas a la vez como mucho en toda la app, sin
reintentos, con caché de 10 min, cortando a la primera señal de bloqueo o de
red); lo dudoso (no se parece, duración rara, una lista entera) sale sin
marcar o rechazado. Una descarga tarda minutos y la conversación no puede
quedarse colgada: al aprobarla, el núcleo la arranca en segundo plano bajo el
mismo turno que usa la página de Descargas (una a la vez, reservado con
`youtube.claim()`, que devuelve un token para que una descarga vieja no pise a
la nueva). Si el usuario la quiere igualmente como otra versión, el asistente
repite la petición con `force`.

## Acordes y tono, de un cifrado publicado

Hasta 1.20 la IA daba el tono y los acordes de memoria, como «aproximación».
Desde 1.21 salen de cifrados que alguien publicó
(`library/src/chord_sheets.rs`), y si no se encuentran no hay. La IA no
escribe ni un acorde.

**Las fuentes, cada una con su buscador.** Los buscadores generales gratuitos
cortan si se les pregunta seguido (DuckDuckGo devolvía 0 resultados a la
segunda consulta), y no hay ninguno sin límites que no pida clave. Así que se
pregunta a las páginas de acordes directamente, las dos a la vez:

- **Ultimate Guitar**: `search.php` y la página del cifrado. Los datos van en
  JSON dentro de la página (`js-store`): el tono que puso quien lo escribió
  (`tonality_name`), la cejilla, los votos, las partes (`[Coro]`) y cada
  acorde marcado (`[ch]G[/ch]`). Su buscador responde 404 cuando no hay nada:
  eso es «no está», no una caída, y se prueba con el título solo. Solo los
  cifrados de acordes públicos (no los «Pro» ni los «Official», de pago).
- **LaCuerda**: la página del artista lista todas sus canciones; cada versión
  marca los acordes con `<A>`, y la primera de la lista es la mejor valorada.
  No guarda el tono.

Cifra Club tiene buscador abierto pero bloquea las páginas a lo que no sea un
navegador (Akamai), y AcordesWeb no guarda el tono: no están.

**Qué se lee.** Los acordes son los que la página marca como acordes; las
partes, sus corchetes o una línea que es solo «Coro:», «VERSO 2» o «Pre-Coro»
(«Coro de ángeles» es letra). El tono, de los datos de la fuente o de una
línea «Tono: A» en la cabecera del propio texto, y solo si es un tono que se
puede transponer. Entre versiones gana la que dice su tono y han votado al
menos tres; si no, la más votada. Cada línea guarda si es de acordes, porque al
transponer solo se tocan esas: en la letra, «Dios» no es un Re.

**Qué cuenta como la misma canción** (`same_song`). El título, con las mismas
palabras exactas, sin ruido («en Español», «Live», lo que va entre paréntesis)
y sin el artista que los canales meten delante: «Hay libertad» no es «Hay
libertad en la casa de Dios». El artista, si comparte una palabra, si se
escribe igual sin espacios («SovereignGraceMusic») o si va dentro del título.
Una versión de otro («(cover)»), una drum cam o un popurrí no se buscan: no se
sabe de quién es la original, y el mismo título de otro artista suele ser otra
canción.

**Medido** con `scripts/evaluate-chords.py` (en el historial, en
`a0e9317^`) sobre 80 canciones al azar de una biblioteca de alabanza (solo
lectura, un segundo entre una y otra):

| | |
| --- | --- |
| Encontradas | 58 de 80 (72 %); 58 de 71 sin versiones, drum cams ni popurrís (82 %) |
| La canción equivocada | 0 de 58, revisadas a mano |
| Con tono de la fuente | 28 de 80 (35 %) |
| De dónde | 50 de Ultimate Guitar, 8 de LaCuerda |
| Tiempo | mediana 5,3 s, peor 9,3 s |

**Se guarda** en la columna `chords`, junto a la ficha de la IA y sin pisarla
(`sheet`, `sheet_checked`, `sheet_tried`). Que no esté también se guarda (no
se vuelve a buscar sola cada vez que se abre la ficha); que una fuente no
respondiera, no, porque es la red y no la canción. `has_chords` es que hay un
cifrado, no que la columna tenga algo. «Rellenar información» pone el tono de
la ficha desde el cifrado solo si lo dice y se toca sin cejilla: con cejilla
lo escrito no es lo que suena.

Guardarlo, pasarlo a otro tono y rellenar la ficha van en Rust
(`library/src/enrich.rs`), con la letra de LRCLIB, la carátula de iTunes o
Cover Art Archive y la ficha de la IA. Lo de fuera (la red, la IA, el
índice, las etiquetas) se lo da `services::enrich` (`library::enrich::Outside`);
hasta la 1.40 se lo daba `enrich.py`, como hacía Python con sus globales.

## Laya, medido y descartado

Laya (Convai, 2026, Apache-2.0) es un modelo de decisiones: se le da un
texto y una pregunta cerrada —sí o no, una opción entre varias— y contesta
con probabilidades en una pasada, sin generar texto. Parecía hecho para los
juicios pequeños que hoy hacen reglas o una llamada a la IA: el juez de
narración (§ El asistente), el «sí» a una oferta, marcar órdenes escondidas
en textos de fuera, el tipo de vídeo de una descarga. Se midió
`laya-multilingual` (mmBERT, 322 M de parámetros, 644 MB; unos 100 ms por
pregunta en la CPU) tal como sale, con `scripts/evaluate-laya.py` (en el
historial, en `a0e9317^`):

| Juicio | Lo de hoy | Laya |
| --- | --- | --- |
| Narración (`library/src/assistant/narration/corpus.json`, 327 frases) | reglas + la IA | AUC 0,49–0,65 en cinco formas de preguntarlo |
| Tipo de vídeo (288 títulos) | `names.video_kind`: 97,9 % | 87,2 %; 0 de 19 secuencias |
| «Sí» a una oferta (40 respuestas) | `_YES`: 87,5 % | AUC 0,63 |
| Órdenes escondidas (40 textos) | nada | AUC 0,93 |

Sin afinar es casi tirar una moneda en lo que la app ya resuelve; su propia
ficha lo avisa (0,34 en decisiones tipadas, frente a 0,32 al azar). Solo da
señal con las órdenes escondidas, y ahí los 40 ejemplos los escribió quien
medía: no basta para fiarse, y lo que protege de verdad ya está (lo que no
tiene vuelta atrás lo confirma la persona). No compensa bajar 680 MB y
tener 1 GB de memoria ocupado para eso. Afinarlo pediría ejemplos
etiquetados que no hay. Si sale otra versión, el guion del historial diría
en unos minutos si ya gana.

## La interfaz se entera de todo

Nada de lo que se enseña puede quedarse congelado porque el cambio lo hiciera
otro: el asistente, una descarga que termina, el vigilante de carpetas. El núcleo
lleva un contador (`revision`) que sube con cada cambio del índice, de las
listas o de las estrellas; Rust, que ya consulta `/api/status` cada dos
segundos para vigilar que el núcleo vive, emite `danplay://changed` cuando
se mueve, y la ventana refresca todo lo que tiene en memoria. Los cambios que
hace la propia ventana siguen refrescando al momento; esto cubre el resto. En
el modo navegador, sin Rust, la interfaz hace lo mismo por su cuenta: consulta
`/api/status` cada dos segundos y compara `revision`.

**El asistente no se pierde al cambiar de página.** La conversación en curso
vive en un composable (`useChat`), no en la página del chat: si le pides «pon
la lista X» y te vas a otra pantalla mientras contesta, la orden llega igual
(antes Vue descartaba lo que emitía una página ya desmontada).

Las descargas tienen un solo seguimiento (`useDownloads`) que comparten la
página de Descargas, el chat y la barra lateral, donde «Descargas» lleva el
número (`2/3`) mientras algo baja. Y la entrada de la que salió lo que suena
—una lista, Todas las canciones, Favoritos— lleva un punto que late.

## Las tareas largas no esperan

Escanear, importar, convertir y buscar duplicados pueden tardar minutos, y el
puente de Rust corta una petición al minuto: antes la interfaz daba error
mientras el trabajo seguía (y una conversión seguía borrando originales).
Ahora arrancan en un hilo (`server/src/jobs.rs`) y la petición contesta al
momento (`202`) con el trabajo; la interfaz consulta `GET /api/jobs/{nombre}`
cada medio segundo y enseña cuánto lleva (`done`/`total`/`message`) hasta
tener el resultado. Pedir otra vez uno que ya está en marcha no arranca otro:
devuelve el que corre. Una conversión de prueba (`dry_run`) sigue siendo
inmediata.

## yt-dlp al día

YouTube cambia a menudo y yt-dlp saca versión cada pocas semanas; la que viaja
con la app (su ejecutable oficial, en `tools/`) se quedaría congelada hasta la
siguiente versión de DanPlay. «Actualizar yt-dlp» (Descargas) baja de GitHub
el ejecutable de la última versión para este sistema (`yt-dlp_linux`,
`yt-dlp_macos`, `yt-dlp.exe`), lo comprueba contra su `SHA2-256SUMS`, lo deja
en la carpeta de datos (`tools/yt-dlp/<versión>/`, con su marca `current`) y
desde ahí se usa (`services::ytdlp`). Si el descargado no contesta, se vuelve
al de la app; y no se cambia de versión a mitad de una descarga. (Hasta la
1.30 era el paquete de Python dentro del núcleo de PyInstaller, y se ponía
al día con la rueda de PyPI.)

YouTube exige además un **motor de JavaScript** para resolver sus retos (sin él
yt-dlp avisa de que «faltarán formatos»). Se busca, con las versiones mínimas
que pide yt-dlp, Deno, Node, QuickJS o Bun, en la carpeta de herramientas y en
el `PATH`, y se le pasa a yt-dlp; si no hay ninguno, Descargas dice qué
instalar. Los paquetes llevan QuickJS-NG dentro (`qjs`, 2 MB), como ffmpeg:
antes era Deno, que resolvía unos 5 segundos antes por descarga pero pesaba
93 MB en cada paquete. Si el sistema tiene Deno o Node, se prefieren.

## Fundidos y cambios sin corte

Todo pasa por un solo render (`engine/src/render.rs`), muestra a muestra, con
un buzón sin candados para las órdenes: el hilo de audio nunca espera.

- **Los tramos** saltan en su muestra exacta con un fundido de 4 ms (antes se
  vigilaban cada 100 ms y se pasaba de largo hasta una décima). Varios tramos
  van uno detrás de otro; «Al acabar» dice si se repiten, se para o se sigue.
- **Entre canciones**, dos «decks»: la que se va baja mientras sube la
  siguiente, con la misma potencia. Con el clic, la cuenta o tramos puestos,
  no se funden.
- **De la canción a sus pistas** (o al cambiar una pista): el relevo
  (`handoff`) abre el conjunto nuevo y lo cruza a la par, en la misma
  muestra, antes del estirador de tiempo: no hay salto ni eco.
- **El clic y las guías habladas** se renderizan en tiempo de canción, sin
  estirar: caen en el pulso también a 0,7× y en la vuelta de un tramo.

## El hilo de audio no se rinde

El hilo que lleva el reproductor es uno solo, y si se cae no hay música:

- **Un archivo que hace *panic* al decodificarse.** El bucle corre bajo
  `catch_unwind`: se avisa de qué archivo era y se vuelve a empezar con el
  mismo volumen y la misma velocidad (el binario va con `panic = "unwind"`).
- **Sin salida de audio, o la elegida desenchufada.** Se intenta abrir cada
  vez que alguien pide sonido; si la interfaz elegida no está, suena por la
  del sistema y se dice (en directo, sin sonido es peor que por otro sitio).
- **La salida muere sonando.** El hilo de la salida (`engine/src/output.rs`)
  la vuelve a abrir solo, a la tasa que tenga entonces.

La salida se abre al primer play y se suelta tras un minuto sin sonar; al
volver, sigue donde iba. Y la cola (`queue/`) no depende del núcleo para
empezar a sonar: la interfaz manda la ruta con cada canción.

## Las salidas: como una secuencia

«Salidas» (`StudyOutputs.vue`, `useRouting.js`) dice por dónde suena la
canción, cada pista, el clic y las guías. El motor tiene **buses** (cada uno,
dos canales de la salida o uno en mono) y cada parte, el clic y las guías van
a uno; un limitador por pareja de canales evita que se sature. Con tres
maneras de siempre (todo junto; clic y guías a la izquierda y música a la
derecha, en mono; clic y guías por 1-2 y música por 3-4 con una interfaz) y
«a medida». Se cambia en directo, sin cortar, y se recuerda por salida de
audio. Las pistas con papel (el «Click Track» y la «Guide» de una secuencia)
van con el clic salvo que se diga otra cosa. La descripción que manda la
interfaz y cómo se convierte vive en el motor (`engine/src/spec.rs`): la misma
en la PC y en el celular.

Cada cosa puede ir en **estéreo o en mono**: un destino de dos canales con
`mono` junta los dos lados y saca lo mismo por los dos (`Target::joins`); por
un solo canal va en mono siempre. Se guarda por cosa y vale con cualquier
reparto. La lista de pistas que reparte es una para toda la interfaz: el
panel de «Salidas» tiene su propio `useRouting()`, y con una lista suya
mandaba al motor un reparto sin los buses de las pistas.

**Las salidas de audio, las de verdad.** En Linux, ALSA enseña además de las
tarjetas los complementos (null, jack, oss, pulse, convertidores…) y cada
salida con muchos nombres (hw, plughw, front, surround51, dmix, hdmi, la
misma con número). `output::devices` deja solo `hw:CARD=<nombre>,DEV=<n>`
(una por salida física); la del sistema (PipeWire o PulseAudio) es la
primera opción de la interfaz. Abrir cada una para contar sus canales dice
también si está libre: la que el sistema tiene ocupada se marca, y elegida,
suena por la del sistema (`fallback`) en vez de quedarse callada
reintentando.

## Guías habladas

Un punto de la canción dice «Coro» o «Puente, cuatro tiempos»: escrito, y lo
dice una voz, o grabado con el micrófono (`engine/src/record.rs`, cpal de
entrada, un WAV temporal que el núcleo recorta y nivela).

Las voces (`analysis/src/voices.rs`, con `services::cues`) corren en el equipo
con ONNX Runtime, sin PyTorch y sin internet salvo para bajarlas, con sha256 y
revisión fijados (`data/voices.json`): **Supertonic 3** (diez voces,
OpenRAIL-M, lee el texto tal cual) y voces **Piper** de España, México y
Argentina, que se entrenaron con los fonemas de espeak-ng. espeak es GPL y no
viaja: sus reglas para el castellano están reescritas en
`library/src/speech.rs` (coincide palabra por palabra en un 96-97 % de un
corpus normal). Los números y las abreviaturas se dicen en palabras antes
(«x2» es «dos veces»). La misma frase con la misma voz es el mismo archivo.
Las guías se guardan con el estudio; solo se aceptan archivos de la carpeta de
guías de DanPlay.

**Los carriles de guías** (1.28.1): hasta seis (`cueLanes` en el estudio:
clave, nombre, volumen, callado, solo), y cada guía dice el suyo (`lane`; sin
él, el primero). En la interfaz van debajo de la onda, a su misma escala
(`StudyTimeline`), con la canción o con sus pistas. El volumen, el callar y
el solo de cada carril se aplican al mandarlas (`audibleCues`); su salida, en
«Salidas», como una pista con papel de guía (`guias_<clave>`), y el motor
manda cada guía por el bus de su carril (`CueSpec.bus`). Se ponen, se mueven
y se cambian solo con la canción parada.

**Exportar** la canción tal cual (`source: "song"`) o la mezcla de las pistas,
con las guías o sin ellas: el núcleo monta las que suenan en un WAV aparte
(cada una en su segundo, al tiempo de la mezcla y sin estirar) y lo suma,
como el clic, después de la velocidad y el tono (`stems.export_mix`).

**El editor de guías** (1.28.2). Lo grabado o importado se recorta y se
limpia en el núcleo (`cues.edit_clip`): de `start` a `end`, por ffmpeg con
`highpass=f=100` (los graves y golpes del micrófono) y `afftdn=nf=-25` (el
ruido de fondo), con fundidos lineales y el pico a -1 dBFS si se pide.
Siempre en un archivo nuevo (`editada-<hash>.wav`, el mismo para los mismos
cambios), y con `preview` en la carpeta de pruebas, que se barre sola. La
interfaz pinta la onda con `cues.wave_of` (el pico de cada trozo y lo que
dura), también en los bloques de los carriles. Un audio del equipo se importa
si no dura más que la canción (la ruta la recorta a lo que dice la base,
aunque la app pida más), como mucho cinco minutos: lo que el motor lee entero
de una guía (`Clips`, `preview`).

**El imán** pega las guías a los pulsos que se ven en la onda (la rejilla
con el doble o la mitad y el «1» corrido; con el tempo a mano, uno cada
tanto): `nearestBeat`, el más cercano, al ponerlas y al arrastrarlas, y
`stepBeat` para antes y después en la tarjeta. Como son los pulsos
detectados de la canción y no una cuadrícula fija, siguen sus cambios de
tempo.

**Escuchar una guía** lo hace el motor (`engine/src/preview.rs`, por la
salida del sistema, en su hilo): el `<audio>` del WebView depende en Linux
de los complementos de GStreamer y fallaba.

**Los avisos de los iconos** (`useTooltips.js`, directiva `v-tips` en el
contenedor del estudio): por delegación, cualquier cosa con `title` saca un
globo propio al dejar el ratón 450 ms o al llegar con el teclado; mientras
se ve, el `title` se guarda en `data-tip` para que no salga a la vez el del
sistema, y vuelve al irse.

### Desde 1.30: cada cosa por su lado, la guía automática

**Los canales, en cada carril.** Cada carril de la onda (la canción, cada
pista, el clic y cada carril de guías) lleva sus botones de canal
(`ChannelPicker.vue`): L y R, o 1, 2, 3… con una interfaz de más salidas
(los canales de la salida elegida, de `output_devices`). Uno encendido: en
mono por él; dos: el primero a la izquierda y el segundo a la derecha
(`destOfChannels`, que da los destinos de siempre —`main`, `L`, `p3`, `m3`—
o `x2-3`, dos cualesquiera); un tercero apaga el más antiguo. «Est / Mono»
es el `mono` de siempre. Tocar uno desde un reparto de fábrica lo pasa a «a
medida» con cada cosa donde estaba (`asCustom`), para que no salte todo a
los dos lados. «Salidas» queda para la salida de audio y los repartos; en
el celular, que no tiene cabeceras de carril, sigue con cada cosa en su
tarjeta. El clic tiene su carril (sus golpes, callarlo, su volumen y sus
canales). «Mezcla» y «Canales» en la barra ocultan esos mandos y la columna
de las cabeceras se estrecha (`--tl-heads`).

**Exportar con los canales** pasa cada cosa por su lado (`sideOf`: los dos,
L o R; de una interfaz, las impares a la izquierda y las pares a la
derecha) al núcleo, que monta la matriz de cada pista como el motor:
primero su balance y, por un lado o en mono, sus dos canales juntos a la
mitad (`stems.matrix`). Las guías y el clic van en su lado del WAV aparte.

**La guía automática** (`analysis/src/structure.rs`, tarea `guia-automatica`)
encuentra las partes de la canción con ffmpeg y sus propias cuentas, sin
modelos: por
compás (los «1» que manda la interfaz; sin ellos, ventanas de 2 s), un
cromagrama y un timbre (bandas log y una DCT), su autosemejanza, la
novedad con un núcleo de tablero de ajedrez y los picos (mejor si caen cada
4 compases). Con las pistas separadas sabe dónde canta la voz (y sus
cambios son fronteras fuertes) y quién lleva un solo. Los trozos se agrupan
por parecido y se nombran: lo de antes y después de la voz, «Intro» y
«Final»; lo que más se repite y suena más fuerte, «Coro»; otra cosa que se
repite, «Estrofa»; un trozo corto justo antes del coro, «Pre coro»; algo
cantado una sola vez tras un coro, «Puente»; sin voz y con un instrumento
mandando, «Solo de guitarra» (o el que sea), si no «Instrumental». Cada
parte se dice con la voz elegida justo antes de que empiece (lo que dura la
frase y un pulso antes, sin pisar la anterior) y va al carril «Partes»
(`auto: true`), que se rehace entero. Es aproximado: con las pistas
separadas y el clic de la canción acierta mucho más.

**Los menús, cortos.** El nombre de una opción se ve siempre entero; lo que
no cabe es su nota, y lo largo va en `hint` (el aviso al dejar el ratón).
La opción puesta va con `active` (en el color de la app y su marca), no con
una nota. Lo que tiene muchas formas (exportar) va en una ventana
(`WideDialog.vue`), con sitio para explicar cada una.

**El recorrido** (`useStudyTour.js`, driver.js 1.8): cada cosa resaltada
con una frase y, donde ayuda, una animación de CSS que se mueve sola
(`tour.css`). Sale la primera vez que se abre el estudio en el escritorio
(si tiene medidas: en las pruebas no) y con «?». Los pasos sin sitio a la
vista se saltan.

## La app del celular

Pedido el 14 de octubre: el reproductor en el teléfono, **sin** convertir,
separar, organizar ni renombrar; solo importar lo que se separó en la PC y
estudiarlo (cada parte, el clic, la velocidad, los tramos). Para quien lo
pidió y para su banda. Android primero; iOS, después (ver abajo).

### Un archivo por canción: `.danpista`

La carpeta de `Separadas/` no basta: ahí solo están las partes. El pulso del
clic (Beat This!, en la caché de la PC), lo guardado del estudio, la letra y
el cifrado viven en la base. «Llevar al celular» (`library/src/bundle.rs`) arma un
ZIP con todo, en `Celular/` junto a `Separadas/`:

```text
pista.json       la canción, sus partes, el pulso, el estudio, la letra,
                 el cifrado, las ondas de cada parte y la de la canción
partes/*.ogg     cada parte, en Ogg Vorbis (calidad 6)
portada.jpg      si la canción la tiene
```

Por qué Vorbis, medido con el decodificador del celular (symphonia 0.6) sobre
golpes en muestras conocidas: Vorbis y MP3 (con su cabecera LAME) empiezan en
la muestra exacta; **AAC llega 1024 muestras tarde** (23 ms: el clic se
iría). Con una canción de 4:54 y cinco partes: FLAC 86 MB, MP3 a 256 kbps
45 MB, Vorbis 23 MB; y Vorbis se decodifica antes (1,0 s frente a 1,55 s las
cinco). Opus queda fuera porque symphonia no lo decodifica. Si el ffmpeg de la
PC no sabe hacer Vorbis, van en MP3.

### El motor: `engine/`

En la PC, mezclar las partes y cambiar la velocidad sin cambiar el tono lo
hace ffmpeg, que en el teléfono no existe. El motor es Rust puro, salvo el
estirador:

- **Decodificar** (`decode.rs`): cada parte desde memoria (los bytes se leen
  enteros al abrir: nunca se lee del disco en el hilo de audio). Lo que sale se
  coloca por la **marca de tiempo de cada paquete**: medido, el primer paquete
  de Vorbis tras un salto no da audio y, contando desde donde dejaba el salto,
  todo salía 1024 muestras corrido. Y se salta 4096 muestras antes de lo
  pedido: un MP3 que empezaba justo ahí salía con diferencias de 0,76 en las
  primeras muestras (su reserva de bits está en los cuadros de antes).
- **Alimentar** (`feeder.rs`): un hilo decodifica todas las partes a la par
  en bloques de 1024 muestras, 1,5 s por delante. Los bloques van y vuelven
  por dos anillos sin candados: el hilo de audio no pide memoria ni espera. Un
  salto empieza una «generación» nueva y los bloques viejos se tiran. Los
  tramos se resuelven aquí, **como en la PC**: un tramo nuevo no mueve la
  canción (entra cuando llega a él) y con tramos la canción no se acaba;
  4 ms de fundido a cada lado de la costura.
- **Sonar** (`render.rs`): suma las partes con su volumen y su panorama
  (rampas a lo largo del trozo), y si la velocidad o el tono no son los
  normales pasa la suma por **Signalsmith Stretch** (C++, MIT, en
  `engine/cpp/` con sus enlaces escritos a mano: el envoltorio publicado
  necesita bindgen y libclang, que no estaban aquí y que en Android e iOS
  piden apuntar el sysroot a mano). A 1× y sin tono no pasa: suena tal cual.
  Encima, el clic: cada trozo sabe qué muestras de la canción se oyen
  (descontando lo que retrasa el estirador y siguiendo los saltos de los
  tramos), así que el golpe cae en su pulso a 0,7× y en la vuelta del tramo;
  las pruebas lo miden contra el golpe de la música (a 1×, a 0,8×, a 48 kHz,
  en un tramo). La cuenta, la envolvente de 3 ms al pausar o saltar y un
  limitador cierran el camino.
- **Salir** (`output.rs`): cpal (AAudio en Android, CoreAudio en iOS). Se
  pide 44,1 kHz: AAudio la acepta y convierte el sistema (el emulador va a
  48 kHz); si una salida no la acepta, convierte el motor (`resample.rs`,
  sinc polifase).

Medido con «Dios Poderoso» (cinco partes, 4:54) en un núcleo de la PC:
decodificar va a 180 veces el tiempo real; mezclar a velocidad normal, a
2.500 veces; estirando a 0,8×, a 70 veces, y el peor trozo de 11,6 ms tarda 0,34 ms con el cálculo repartido
de Signalsmith (0,97 ms sin repartir). Un teléfono es varias veces más lento:
sobra margen.

Lo que la PC y el celular tienen que hacer igual vive también en `engine/`:
la rejilla del pulso y su análisis de Rust, los ajustes del metrónomo, los
tipos de la cola y el sonido del clic. La app de escritorio los usa de ahí
(sin la función `motor`, que trae el resto).

### La misma interfaz

El celular usa **la interfaz de la PC**, con otra entrada
(`desktop/mobile.html` → `src/mobile/`) y, desde 1.27, su propio armazón para
los dedos: la cabecera con el menú de lado (`MobileMenu.vue`), la lista táctil
(`MobileSongs.vue`: doble toque para sonar, un toque para elegir y enseñar los
botones, mantener pulsado para las opciones; el doble toque se cuenta a mano
con los `click`, porque el WebView de Android no siempre manda `dblclick`), el
reproductor (`MobilePlayer.vue`: la barra baja, que se sube con el dedo para
abrir el estudio, y el reproductor entero) y los ajustes (`MobileSettings.vue`).
El estudio (`StudyBar`, `StudyTimeline`) y la ficha (`DetailsPanel`) son los
de la PC, con ramas `isMobileApp` donde el teléfono cambia el contenido (la
cabecera del estudio, «varios» y el candado fuera de la onda, el nombre de
cada pista sobre su carril) y `styles/mobile.css`, que solo carga esta app,
para lo demás. Lo que solo se puede en la PC (separar, guardar mezclas, buscar
letra o acordes, editar el archivo, proyectar, las formas de la ventana) se
esconde con `platform.js`.

La interfaz del celular se prueba en Chrome a tamaño de teléfono con un
puente de Tauri falso (las órdenes del reproductor y el `api` en JS), y al
final en el emulador con el APK de verdad.

Para no tocar esa interfaz, la app (`mobile/`) le contesta lo mismo que la de
escritorio:

- el **reproductor** con las mismas órdenes y el mismo evento
  (`danplay://state`, §1 de INTERNAL-CONTRACT.md), sobre el motor. En el
  celular no hay canción entera: «la canción» son todas sus partes a 1 (la
  suma es la canción: «Otros» es lo que queda) y «las pistas», con la mezcla
  del estudio;
- el puente **`api`**, que en la PC lleva al núcleo, lo contesta la app del
  celular (`mobile/src/api.rs`) con las pistas importadas, con la misma forma
  que el núcleo; lo que en el celular no existe contesta 404 con el motivo;
- las **listas** (`mobile/src/playlists.rs`): `.listas.json` junto a las pistas,
  cada lista con las carpetas de sus canciones (estables: una pista que se
  quita y se vuelve a traer vuelve a su sitio) y las mismas rutas y campos
  que el núcleo (`n`, `seconds`);
- el **cambio de tono de los acordes** (`theory.rs`, el mismo que el de la
  PC, que era `theory.py` y `enrich.transpose_sheet`, con las mismas
  pruebas);
- el **pulso**: el de Beat This! que trae el `.danpista`; sin él, el análisis
  de Rust de la PC sobre la suma de las partes.

### Android

- Desde Android 15 la app se pinta por debajo de las barras del sistema; la
  actividad aparta el contenido (`MainActivity.kt`).
- **«Abrir con» y «Compartir con»**: un `.danpista` que llega de WhatsApp,
  Descargas o Drive (como binario o ZIP: Android no conoce la extensión) se le
  pasa a la página, que lo importa como con el botón.
- **«Atrás»** cierra lo abierto (menú, ficha, estudio); si no queda nada, la
  app pasa a segundo plano y la música sigue.
- Mientras suena, la pantalla no se apaga (la API Wake Lock del WebView; se
  puede apagar en Ajustes).
- **Los mandos de la bandeja** (y de la pantalla de bloqueo, los auriculares
  y el reloj): una sesión multimedia en un servicio en primer plano de tipo
  reproducción (`PlaybackService.kt`). La página le cuenta qué suena por
  `DanPlayAndroid.update` (`src/mobile/mediaSession.js`, solo cuando cambia
  algo o la posición salta; Android avanza la barra solo). Lo que se pulsa no
  pasa por la página: va directo al reproductor en Rust por JNI
  (`mobile/src/controls.rs`), así funciona con el WebView dormido, y la página se
  entera por `danplay://state`. Quitar la app de recientes para la música y
  quita la notificación.
- **El icono** es adaptable: las barras de la onda en vector, dentro de la
  zona segura de 66 dp, sobre el negro del logo, y la misma capa sirve de
  icono temático. El tema arranca ya oscuro (sin fogonazo blanco).
- **La versión** sale de `desktop/package.json` (`version` en
  `mobile/tauri.conf.json`): el código de versión es `mayor·10⁶ + menor·10³ +
  parche`. Antes el APK decía 1.0 y código 1.
- La versión que se entrega va **firmada siempre con la misma llave**
  (`~/Android/claves/`, fuera del repositorio) y con las tres firmas, v1, v2
  y v3 (la v1 la miran algunos instaladores de marca): un teléfono solo acepta una
  actualización firmada como la que tiene. Si se pierde, la banda tendría que
  desinstalar y volver a instalar.

Probado en el emulador (Pixel 7, Android 15) con siete canciones separadas
de verdad: importar con el selector del sistema y con «Abrir con», sonar, la
velocidad (a 0,8× avanza 0,798 s por segundo), un tramo, el clic con el pulso
de la PC, la ficha, los acordes y su tono, «atrás» y la música en segundo
plano.

### iOS

El código está listo (cpal tiene CoreAudio y Tauri compila para iOS), pero no
se ha compilado: hace falta un Mac (en GitHub, como el `.dmg`). Dos cosas
pendientes: la sesión de audio en «reproducción», para que suene con el
interruptor de silencio puesto, y la firma. Para la banda, la cuenta de
desarrollador de Apple con TestFlight; con un Apple ID gratis habría que
volver a firmarla cada 7 días en cada iPhone.

### La música del teléfono

Desde 1.28 el celular también toca cualquier canción del teléfono, no solo
las pistas traídas (`mobile/src/music.rs`): se eligen las carpetas (se buscan
solas; Música y Descargas vienen elegidas), el índice se guarda y se pone al
día por tamaño y fecha, y el título, el artista y la duración se leen con
symphonia. En Android se abren con su ruta: con el permiso de «música y
audio», que se pide al ir a verla (`MusicPermission.kt`), el sistema deja leer
así los archivos de audio. Los ids son negativos, como los archivos sueltos de
la PC. Al poner una, la cola son las de su carpeta. El motor carga el archivo
como una sola parte, con su clic, sus tramos y sus salidas.

**Qué hay lo dice Android** (1.28.3). Recorrer la memoria no ve la tarjeta SD
(`/storage/<tarjeta>`) ni las carpetas de las apps (los audios de Telegram o
WhatsApp, en `Android/media/<app>/…`, que se saltaban), y sí encuentra los
tonos del sistema. El índice de música de Android (MediaStore, todas las
memorias) lo da todo: `DanPlayMusica.canciones()` devuelve cada canción con
su ruta, título, artista, álbum, duración, tamaño y fecha, sin tonos de
llamada, avisos, alarmas ni grabaciones; la página se lo pasa a la app
(`PUT /api/phone/media`) antes de enseñar las carpetas o mirar la música, y
`Musica` agrupa por carpeta y hace las canciones con eso, sin abrir los
archivos. Sin esa lista (en la PC, en las pruebas) se recorren las carpetas.

**En el teléfono las guías son de lectura**: se hacen en la PC y viajan en el
.danpista (`guias/…`, dentro de la pista). El estudio del celular enseña sus
carriles como filas de mezcla (callar, solo, volumen) y nada para ponerlas o
cambiarlas; tocar una lleva a ella, y escucharla usa `preview_clip` del
motor, como en la PC.

## Términos, novedades y créditos

DanPlay es obra de Daniel Rangel, y la app lo dice: en «Acerca de DanPlay»
(`AboutPage.vue`, una página del enrutador con tres pestañas), en la
bienvenida, al pie de la barra lateral y en el menú del celular, con su
Telegram para colaborar. Los textos viven en la interfaz:
`src/data/terms.js` (los términos, su versión y el autor) y
`src/data/releases.js` (lo de cada versión, corto; una prueba comprueba que
cada versión del CHANGELOG desde la 1.20.0 está, con su fecha).

**Los términos** salen solos la primera vez (y otra vez si sube
`TERMS_VERSION`). La casilla no se puede marcar hasta bajar al final del
texto, y «Aceptar» pide la casilla. Lo aceptado lo guarda el núcleo
(`library/src/terms.rs`: `terms.json` en sus datos; `GET /api/terms`, `POST
/api/terms/accept`), no el WebView. Sin aceptarlos se puede escuchar y
ordenar la música, pero `requireTerms()` (`useTerms.js`) para tres cosas:
separar en pistas (`useSeparation.request`), el asistente (`useChat.send`) y
«Transformar». Al intentarlo sale la ventana con lo que se quería hacer, y la
acción sigue si se aceptan.

**Las novedades** se marcan como leídas al abrir su pestaña
(`localStorage`). Tras una actualización, «Acerca de DanPlay» dice «Nuevo» y
un aviso lo cuenta al arrancar.

## Estructura del proyecto

```text
server/         crate Rust (danplay-server): la API del núcleo, en el mismo
                proceso que la app o por un socket (el binario, main.rs)
  src/
    router.rs     qué función contesta cada ruta; routes/ (un módulo por
                  dominio), validate.rs (lo que llega), jobs.rs (tareas
                  largas), guard.rs (la guardia de cada petición)
    services/     lo que hace el núcleo alrededor de library y analysis: el
                  escaneo, el vigilante (watcher), las descargas (youtube,
                  ytdlp), separar en pistas (stems, separation), el pulso y
                  su cola (beats), las guías habladas (cues), lo hecho para
                  otra copia (reuse), las secuencias, el celular (bundle),
                  el asistente (chat/), la papelera (trash)…
    api_tests/    las pruebas de la API, sobre bibliotecas temporales
  tests/          la de integración: el núcleo por un socket (serve.rs)
library/        crate Rust (danplay-library): el índice (index/: esquema y
                migraciones, escaneo, búsqueda, canciones, carpetas,
                historial, «Transformar»), las etiquetas (lofty), los
                nombres, las listas, el diario, la IA (ai/: proveedores,
                catálogo de modelos, cliente) y el asistente (assistant/:
                leer listas pegadas, emparejar, el plan, las herramientas, la
                narración), los cifrados, la teoría, TOON, la configuración
analysis/       crate Rust (danplay-analysis): el pulso (Beat This!), el
                separador (Demucs y el Mega53), las redes en ONNX Runtime
                (ort), el clic de la mezcla, la guía automática, las voces y
                los pesos de PyTorch y safetensors (sin PyTorch)
data/           lo que viaja con la app: el grafo de Beat This! con sus
                pesos (pulse/), los del separador sin pesos (separator/,
                también el Mega53), el catálogo de voces (voices.json) y la
                foto del catálogo de modelos
engine/          el motor de audio de la PC y del celular (decode, feeder,
                render, stretch, output, engine, bundle, record: grabar del
                micrófono, spec: salidas y guías como las manda la interfaz)
                y beats (el pulso), metro, queue y click; cpp/ Signalsmith
mobile/          la app del celular (Tauri): el reproductor con el contrato de
                la PC, el puente api con sus pistas, la biblioteca de pistas, la
                música del teléfono (music.rs); gen/android el proyecto de
                Android
desktop/        interfaz Vue 3 + envoltorio Tauri (también la del celular:
                mobile.html y src/mobile/)
  src/
    App.vue       la ventana principal (la ventanita y la proyección, aparte)
    islands.js    lo que no se ve al arrancar (el estudio, la ficha,
                  Transformar, tres de las cuatro vistas…): cada parte es su
                  trozo y se carga al abrirla, en tiempo libre o al pasar el
                  ratón por su botón (`v-preload`)
    router.js     las páginas (Ajustes, Asistente, Descargas, Entrada,
                  Duplicados, Acerca de), con carga diferida y el chat en
                  <KeepAlive>
    data/         los textos de la app: términos y autor (terms.js) y las
                  novedades de cada versión (releases.js)
    composables/  estado compartido: useLibrary (lista y estado), useSelection,
                  useSongMenus, useSongList, usePlayback, useClick (el clic de
                  cada canción, con el estudio abierto o cerrado), useChat
                  (y chatLink, lo poco de él que la app usa sin cargarlo)…
    playback/     la máquina de estados de la cola, en JS y sin dependencias
    components/   la interfaz, una página por archivo; ui/ los controles base
    styles/       los estilos, por áreas (base, listas, ui, responsive…); los
                  del estudio, el chat, la ventanita y la proyección van con
                  su componente
    utils/        formato, teclas, grupos, carpetas, markdown del chat
  tests/        pruebas de la interfaz (Vitest, con dobles del núcleo y de Rust)
  e2e/          los flujos enteros en Chrome contra el núcleo de verdad;
                  perf/, lo que tarda la compilada en WebKit (`npm run perf`)
  src-tauri/src/
    core.rs       el núcleo (server), en este mismo proceso: el puente de la
                  API y el aviso de sus cambios a la interfaz
    queue/        la cola: qué suena y qué viene (model, session, logic,
                  resolve, worker, commands)
    player/       el audio sobre el motor (engine: una función por orden;
                  formats: lo que el motor no lee, a FLAC; state)
    record.rs     grabar una guía hablada con el micrófono
    open.rs       abrir desde fuera: la carpeta a la cola, las secuencias
    protocol.rs   danplay://cover, las carátulas
    tools.rs      dónde están ffmpeg y los demás programas, y data/
    tray/         bandeja: linux.rs (ksni) y desktop.rs (Windows/macOS)
    lazy_window.rs  la ventanita y la proyección, creadas al abrirlas
    media.rs      MPRIS / SMTC
  src-tauri/permissions/  qué comandos puede invocar cada ventana
tests/          las rutas de medios (test_frontend.mjs) y las de humo
                (smoke.sh)
packaging/      lo del escritorio de Linux (danplay.desktop, deb/) y el
                instalador de Windows desde Linux (windows/installer.nsi)
scripts/        build.sh, data.sh, tools-*.sh, test.sh e icons.mjs
docs/           esta documentación
```

El **código** está en inglés: carpetas, archivos, funciones, variables, rutas
de la API y columnas de la base. Los **comentarios** y todo lo que ve el
usuario están en castellano.

## Construir

Son tres capas y el orden importa: si compilas solo una, la app arranca con
la versión vieja y no se nota. Por eso hay un script y no tres comandos.

```bash
./scripts/build.sh              # interfaz + datos + app (con el núcleo dentro)
./scripts/build.sh --package    # además el .deb y el .AppImage
```

**Sin Python.** El núcleo va dentro de la app (el crate `server`, en el
mismo proceso: ver [la migración](MIGRATION.md)). Hasta la 1.30 era el de
Python, empaquetado con PyInstaller en una carpeta (`core/`) que la app
arrancaba aparte y a la que hablaba por un socket. Con `--package` la app se
compila sin empaquetar (`tauri build --no-bundle`), saca con ella misma la
foto del catálogo de modelos (`--models-snapshot`) y luego se empaqueta
(`tauri bundle`); el script comprueba que el `.deb` lleva el núcleo, sus
datos y sus programas, y nada de Python.

**Los programas de fuera, dentro de los paquetes.** ffmpeg, fpcalc,
QuickJS-NG y yt-dlp (su ejecutable oficial) viajan en la carpeta `tools/` de
cada paquete (en Linux y macOS los ponen `scripts/tools-linux.sh` y
`tools-macos.sh`, con versiones y sha256 fijados; en Windows, su CI). La app
se la da al núcleo, que la mira antes que el sistema.

**Los datos, en `data/`.** Lo de `data/` del repositorio (el grafo y los pesos de
Beat This!, los grafos del separador, el catálogo de voces y la foto del
catálogo de modelos) y ONNX Runtime, la biblioteca oficial de Microsoft (la
1.31.0; en los Mac con Intel, la 1.23.2, la última que hay para ellos: `ort`
pide la API 23), con su licencia: lo pone `scripts/data.sh`, comprobado con
su sha256 y en una caché (`~/.cache/danplay-<sistema>/`). La app se lo da al
núcleo (`App::packaged`), y el núcleo busca ahí ONNX Runtime
(`services::onnx`): en `usr/lib/DanPlay/data` en el `.deb` y el AppImage, en
`Contents/Resources/data` en macOS y junto al ejecutable en Windows
(`tools::bundled`).

Los crates de Rust (`analysis`, `engine`, `library`, `server`, la app de
escritorio y la del celular) son un **workspace** (`Cargo.toml` de la raíz):
un solo `Cargo.lock`, una sola carpeta `target/` y la versión de DanPlay en
un solo sitio, que heredan todos y que Tauri lee de ahí.

Para desarrollar no hace falta nada de eso: `./scripts/dev.sh` abre la app
entera con la interfaz en caliente y el núcleo compilado del código, con los
datos de `data/` y el ONNX Runtime que deja `scripts/data.sh` en
`desktop/src-tauri/data/` (ver [Contribuir](CONTRIBUTING.md)).

## El tono y el tempo

El tono y los bpm de una canción salen de sus etiquetas (`TKEY`, `TBPM`), de
la ficha de la IA, que sí conoce las canciones, o de lo que corrijas a mano,
que se guarda en el archivo.

El crate del núcleo llegó a estimarlos por señal (cromagramas y FFT), pero
medido contra valores conocidos acertaba el tono en 2 de 8 canciones y el
tempo en 5 de 8, y ya nadie lo usaba: se quitó, y con él `rustfft` del crate.
Lo que sí se analiza por señal es **el pulso** para el metrónomo del modo
estudio (`beats.rs`, en la app de escritorio): por bloques, sin cargar la
canción entera en memoria (unos 9 MB por hora de audio; antes 320).

Si alguien sabe de detección de tonalidad, sigue siendo probablemente el sitio
donde más se agradecería ayuda.
