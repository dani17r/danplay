# Arquitectura

Cómo encaja DanPlay por dentro, y por qué está hecho así. Si solo quieres
usarlo, con el [README](../README.md) tienes de sobra.

---

## Las cuatro capas

```text
   Vue 3 + Vite  (interfaz)
        │  invoke() + eventos ← único puente, sin HTTP en el JS
   Tauri / Rust  (proceso principal: cola, audio, bandeja, MPRIS)
        │  socket Unix 0600   ← sin puerto TCP; solo tu usuario
   Python        (núcleo: identificación, índice, IA, etiquetas)
        │
   Rust (PyO3)   (hashes en paralelo, análisis de audio)
```

El contrato exacto entre capas —qué comandos hay, qué eventos se emiten y qué
nombres usa cada campo— está en [CONTRATO-INTERNO.md](CONTRATO-INTERNO.md).

Cada capa está donde está por una razón concreta:

**Vue 3** es interfaz y nada más. No sabe qué es un socket ni hace `fetch`.
Todo lo que necesita del núcleo lo pide con `invoke()` a Rust.

**Rust (Tauri)** es el proceso principal: arranca y **vigila** el núcleo
Python (si se muere, lo levanta otra vez y avisa), hace de puente con la API,
sirve el audio leyendo del disco y lo reproduce nativamente. También guarda
**la cola de reproducción**, dibuja el icono de la bandeja y publica lo que
suena al escritorio.

**Python** es el núcleo: la cascada de identificación, el índice SQLite, las
etiquetas ID3, la IA, las descargas. Se puede usar solo, sin interfaz, desde
la línea de comandos.

**Rust (PyO3)** es donde va lo que Python hace lento: hashes de archivos en
paralelo (rayon) y análisis de audio con FFT. Es opcional: si el crate no está
compilado, Python tiene su propia versión más lenta y la app funciona igual.

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

La API nunca escucha en un puerto. Solo en `$XDG_RUNTIME_DIR/danplay.sock` con
permisos `0600`.

Un puerto en `localhost` no es privado: cualquier proceso de tu equipo puede
hablar con él, y una página web abierta en tu navegador también. Esta API puede
listar tu biblioteca, mandar archivos a la papelera y leer imágenes del disco.
Con un socket Unix `0600`, solo tu usuario puede abrirlo.

`uvicorn` hace `chmod 0666` al socket que crea él mismo, así que lo creamos
nosotros con los permisos correctos y se lo pasamos ya escuchando.

Para desarrollo (`danplay serve` sin `--uds`) sí hay puerto TCP, y ahí se
aplican tres protecciones de navegador: CORS restringido a los orígenes de
Vite, validación de la cabecera `Host` contra el reenlace de DNS, y una
cabecera propia (`X-DanPlay: 1`) en **todas** las peticiones a `/api/`,
también los `GET`. Esto último porque CORS impide **leer** la respuesta pero
no evita el efecto: un `POST` sin cuerpo a `/api/scan` desde cualquier
pestaña abierta arrancaba un escaneo, y hay `GET` que gastan IA. Exigir una
cabecera propia obliga al navegador a preguntar antes, y ahí CORS sí corta.
Solo se libran el audio y las carátulas, que el navegador pide por su cuenta
desde `<audio>` e `<img>` y no pueden llevarla.

Y dos cosas más en ese modo: `danplay serve --host` con una dirección que no
sea de loopback se niega a arrancar si no hay token (sería abrir la API a la
red), y no existen `/docs`, `/redoc` ni `/openapi.json`, que quedaban fuera de
la comprobación del token y contaban a cualquiera todo lo que sabe hacer.

En Windows no hay sockets Unix que uvicorn sepa escuchar, así que allí la
aplicación levanta el núcleo en `127.0.0.1` con un puerto libre y un secreto
de un solo arranque que le pasa por el entorno. Sin ese secreto, la API
responde `401` a todo. Es lo más cerca de «solo la aplicación habla con el
núcleo» que permite ese sistema.

La prueba de humo verifica que la app de escritorio no abre ningún puerto.

## El audio no pasa por el WebView

Cuando pulsas play, la interfaz pide la ruta del archivo a Python y se la
manda a Rust. Rust lo decodifica (`rodio` + `symphonia`) y lo saca a la tarjeta
de sonido desde un hilo dedicado. El WebView no toca el audio en ningún momento.

Las carátulas sí llegan al WebView, por un protocolo propio
(`danplay://cover/<id>?size=N`, en `protocol.rs`) que se las pide al núcleo
sin pasar por el JS. Había también una rama `audio` con rangos de bytes, pero
dentro de Tauri el audio lo suena Rust y nadie la usaba: se quitó. En el modo
navegador, el `<audio>` lee `/api/song/{id}/audio` por el proxy de Vite.

> **Ojo con el protocolo.** Tauri lo expone de forma distinta según el sistema:
> en Linux y Windows es `http://<esquema>.localhost/...`, y solo en macOS/iOS
> es `<esquema>://localhost/...`. Hay pruebas dedicadas a esto.

## Los formatos que el decodificador no conoce

`symphonia` —lo que usa rodio por debajo— no trae ni opus ni wma, y los dos
están en la lista de lo que DanPlay organiza. La salida no es pedirle al
usuario que convierta su archivo para poder oírlo: se le pide a **ffmpeg** que
lo decodifique y mande el audio crudo por una tubería, y eso se le da a rodio
como una fuente más (`transcode.rs`).

ffmpeg ya era dependencia del programa (convierte formatos y encoge
carátulas), así que no se añade nada. Buscar dentro de la canción vuelve a
lanzar ffmpeg desde el segundo pedido: una tubería no se rebobina.

Todo lo demás sigue yendo por el decodificador nativo, que es más rápido y no
depende de nada externo.

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
solo, sin pulsar «Analizar». Lo lleva `watcher.py`, un hilo del núcleo:

- **Al arrancar**, antes de contestar la primera petición, las carpetas que
  ya no existen apartan sus canciones (solo mira si cada carpeta existe: es
  instantáneo). Así la interfaz nunca enseña canciones de una carpeta que se
  movió. Después, en segundo plano, un escaneo incremental pone al día el
  resto.
- **Mientras corre**, `watchdog` (inotify en Linux) avisa de cada cambio
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
fila entera. Sus filas en las listas se quedan donde estaban (las listas solo
enseñan lo que está en `songs`). Si el archivo vuelve, la canción recupera
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
apartado se olvida del todo (con sus filas en las listas).

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

- `providers.py`: el catálogo (sesenta y pico servicios en seis grupos:
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
- `model_catalog.py`: **qué modelos existen**, sin escribirlos en el código.
  Los nombres caducan en meses (OpenAI cambió toda su nomenclatura, Mistral
  retiró los alias `-latest`), así que se consulta
  [models.dev](https://models.dev/api.json), una base de datos abierta con
  200+ proveedores y miles de modelos: el id exacto de cada proveedor, si el
  modelo usa herramientas, si acepta JSON y temperatura, precio, contexto,
  fecha y si está obsoleto. Se pide con **petición condicional (ETag)** cada
  vez que se abre el selector y al arrancar: sin cambios, el servidor
  responde 304 y cero bytes, así que se puede comprobar siempre. La copia
  vive en la carpeta de datos; la app lleva dentro una foto para el primer
  arranque sin red, regenerada en cada compilación
  (`scripts/actualizar-modelos.py`). Y lo que el usuario puede usar **de
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
- `ai.py`: el cliente, que **tolera**. Anthropic ignora `response_format`,
  muchos servidores locales rechazan `tool_choice="required"`, los
  razonadores de OpenAI no admiten `temperature` y quieren
  `max_completion_tokens`. En vez de fallar en seco, `complete()` quita lo
  que el servidor rechaza, reintenta y se acuerda por (proveedor, modelo);
  lo que el catálogo ya dice que no se admite ni se manda. Si el modelo no
  sabe usar herramientas, el asistente lo cuenta en castellano en vez de dar
  un error genérico. Siguen siendo dos modelos con papeles distintos: el
  **rápido** (identificar nombres sucios, rellenar fichas, letras, el juez
  de narración) y el de **conversación** (el asistente, que necesita
  herramientas). «Probar» hace la llamada más barata posible con cada uno y
  una tercera con una herramienta de prueba.

## Modo estudio

Para machacar un trozo: **bucle A-B**, **velocidad sin cambiar el tono**,
marcadores con nombre y notas por canción.

El tramo se elige sobre la **forma de onda** (`StudyTimeline.vue`): se
arrastra de donde a donde, se cogen sus bordes, se lleva entero, o se marca
con las teclas A y B mientras suena; un clic sin arrastrar va a ese punto,
y cerca de un marcador el borde se pega a él. Debajo va una regla con los
minutos (el paso se elige para que quepan ~70 px entre números).

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

**Tono.** Se corre en semitonos (−12..12) por el mismo camino que la
velocidad: ffmpeg reabre la canción donde iba con `rubberband=tempo:pitch`
(tiempo y tono a la vez, buena calidad; lo traen el ffmpeg de Debian y el
que va en el paquete de Windows) o, si no está, `asetrate`+`atempo`. Se
guarda con la canción y se enseña el tono resultante («G → A») si el índice
sabe el tono original.

**Metrónomo.** Un clic sintetizado en un sink aparte del mezclador de rodio
(`metronome.rs`): tiene su volumen y su play/pausa, independientes de la
canción. El pulso y el compás los detecta la propia app en Rust
(`beats.rs`), sin modelos ni dependencias: envolvente de ataques por FFT
(flujo espectral), tempo por autocorrelación con un prior alrededor de 120
(o del bpm del índice), rejilla de pulsos por programación dinámica (Ellis
2007, lo que hace librosa) afinada a subtrama, y el «1» por dónde caen el
bombo y los cambios de acorde, probando 4/4 y 3/4. Un par de segundos por
canción; la rejilla se guarda por ruta durante la sesión y se pinta sobre
la onda. Sonando con la canción, el clic se reengancha a la rejilla en
cada play, salto, cambio de velocidad y vuelta del bucle, y sigue la
velocidad del estudio; con la canción parada sigue solo al mismo tempo. Se
puede corregir a mano (compás, «el 1 es el siguiente», ×2/÷2, tempo libre)
y eso se guarda con la canción. Se miró usar madmom (muerto desde 2018),
Beat This! o Demucs (los dos sobre torch, 200–550 MB): quedan como posible
«paquete de IA local» opcional más adelante.

La onda la
calcula el núcleo en Rust recorriendo el archivo por bloques —no hace falta
tener la canción entera en memoria—, con pico y RMS por columna; sin el
núcleo compilado (Windows) la saca ffmpeg a PCM crudo y la resume Python. Se
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
búsquedas y el bucle. Sin ffmpeg, rodio cambia la velocidad a la antigua y
la barra avisa de que el tono cambia. El bucle vive en el hilo de audio (al
pasar de B, vuelta a A), y en el navegador lo hace el `<audio>`. Lo que se
marca se guarda con la canción, en el índice y en una etiqueta del archivo
(`ESTUDIO`, JSON), y se recupera al escanear si el índice se pierde, como
las estrellas y las listas. Al cerrar la barra, la canción vuelve a sonar
normal.

## Para el atril

Un repertorio se exporta también como **hoja para el atril**: un HTML en
`Listas/` con cada canción, su tono (americano y latino), bpm, cejilla
sugerida y los acordes por secciones si la IA los dio, y la letra si se
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
fallo de medida. Y se mide con decimales: con posiciones enteras, el redondeo
se acumulaba en una lista larga y las últimas filas no se alcanzaban nunca.

Una vista **agrupada** («Artistas», o agrupar por álbum o por tono) es también
una sola lista virtual, con las cabeceras de grupo como filas: antes era una
lista por grupo, y con grupos de menos de ochenta canciones se pintaba todo.
La selección con Mayús, el orden de la cola y las flechas siguen el orden que
se ve, grupo a grupo (`utils/groups.js`).

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

23 herramientas sobre el mismo núcleo que usa la interfaz. No hay un camino
paralelo: cuando el asistente corrige un título, llama a la misma función que
el botón de renombrar.

**Reproducir no lo puede hacer él.** El audio y la cola los maneja Rust, así
que las herramientas de reproducción devuelven una `action` que la app ejecuta
al recibir la respuesta.

**Lo que no tiene vuelta atrás lo aprueba una persona.** Borrar una canción,
borrar un repertorio y descargar no se ejecutan dentro de la conversación: el
núcleo devuelve lo que *iba* a hacer y la interfaz lo pregunta. El texto del
sistema ya pedía consultar antes, pero un texto no es una barrera: por la
búsqueda web, por los títulos de YouTube y por las letras entra contenido que
escribe cualquiera, y bastaría una línea bien puesta para disparar un borrado.

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

Cuando termina una descarga pedida desde el chat, la app le pasa el turno
para que remate lo que quedara («…y ármame una lista»), con los ids exactos
de lo que entró.

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

**Si el proveedor falla, responde otro.** Caído, sin crédito, saturado o
con la clave rechazada, `ai.complete` pasa al siguiente perfil configurado
(cada uno con su modelo), marca al caído durante un minuto para no volver a
esperar su tiempo límite, y la respuesta dice quién contestó (`via`). Un
error del mensaje (un 400) no dispara el respaldo: no lo arreglaría.

**Lo que cuesta, a la vista.** Cada llamada se apunta en `ai_usage`
(proveedor, modelo, tokens y el coste según el precio del catálogo); cada
respuesta enseña sus tokens y su coste, y Ajustes lo de hoy y lo del mes.

**Las conversaciones se guardan en la base** (`chats.py`): varias, con
título, y se busca en todas. Lo que había en el `localStorage` de versiones
anteriores pasa a la base una vez.

**Cada token se paga, y el prompt viaja en cada llamada.** El texto del sistema
se escribe una regla por fallo real y sin adornos (1.305 tokens; era 2.077),
las herramientas se declaran con una fábrica que no repite el andamiaje y
describe cada una en una frase; las que hacían lo mismo se fusionaron
(`edit_song` puntúa y marca favorito; `get_lyrics` vale por id o por nombre;
`play` pone una canción o un repertorio; los nombres viejos siguen valiendo
como alias), y las de descargar, las de músico y la de letra+carátula solo se
declaran cuando la conversación habla de eso (un turno normal lleva 17
herramientas, unos 1.700 tokens; eran 2.764), y **los resultados de las
herramientas van en TOON** (`toon.py`), no en JSON: una lista de canciones se
manda como tabla, con las claves una sola vez en la cabecera y una fila por
canción. Medido sobre resultados reales, la mitad de tokens en una búsqueda
(54 %) y un 47 % en conjunto. El modelo recibe una explicación de dos líneas del
formato; lo que él devuelve sigue siendo JSON, que es lo que garantizan los
modos JSON de los proveedores. Y una ficha de IA guardada sin tono ni acordes
(la dio un modelo que no conocía la canción) no se reutiliza: se vuelve a
preguntar, que con otro modelo puede salir.

**Descargar se pide, no se espera.** Una descarga tarda minutos y la
conversación no puede quedarse colgada: al aprobarla, el núcleo la arranca en
segundo plano bajo el mismo turno que usa la página de Descargas (una a la
vez, reservado con `youtube.claim()`), y el chat la sigue y cuenta el
resultado cuando acaba. Si algo ya estaba en la biblioteca no se baja, se
dice; y si el usuario la quiere igualmente como otra versión, el asistente
repite la petición con `force`.

## La interfaz se entera de todo

Nada de lo que se enseña puede quedarse congelado porque el cambio lo hiciera
otro: el asistente, una descarga que termina, la línea de órdenes. El núcleo
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
mientras el trabajo seguía (y una conversión seguía borrando originales). Ahora
arrancan en un hilo (`api/jobs.py`) y la petición contesta al momento (`202`)
con el trabajo; la interfaz consulta `GET /api/jobs/{nombre}` cada medio
segundo y enseña cuánto lleva (`done`/`total`/`message`) hasta tener el
resultado. Pedir otra vez uno que ya está en marcha no arranca otro: devuelve
el que corre. Una conversión de prueba (`dry_run`) sigue siendo inmediata.

## yt-dlp al día

YouTube cambia a menudo y yt-dlp saca versión cada pocas semanas; la que viaja
dentro del núcleo empaquetado se quedaba congelada hasta la siguiente versión
de DanPlay. «Actualizar yt-dlp» (Descargas) baja de PyPI la última rueda
universal y la de `yt-dlp-ejs` que esa versión pide, comprueba su `sha256`, la
deja en la carpeta de datos y desde ahí se usa (`ytdlp.py`): un buscador propio
al principio de `sys.meta_path` hace que se carguen ella y sus submódulos
también dentro del binario de PyInstaller, donde el importador congelado va
antes que `sys.path`. Si la descargada no importa, se vuelve a la empaquetada;
y no se cambia de versión a mitad de una descarga.

YouTube exige además un **motor de JavaScript** para resolver sus retos (sin él
yt-dlp avisa de que «faltarán formatos»). Se busca, con las versiones mínimas
que pide yt-dlp, Deno, Node, QuickJS o Bun, en la carpeta de herramientas y en
el `PATH`, y se le pasa a yt-dlp; si no hay ninguno, Descargas dice qué
instalar. El instalador de Windows lleva Deno dentro, como ffmpeg.

## El hilo de audio no se rinde

El hilo que decodifica y saca el sonido es uno solo, y si se cae no hay
música. Cuatro cosas lo dejaban mudo hasta reiniciar DanPlay, y las cuatro se
tratan ahora dentro del propio reproductor (`player/`):

- **Un archivo que hace *panic* al decodificarse.** El bucle corre bajo
  `catch_unwind`: se avisa de qué archivo era y se vuelve a empezar con el
  mismo volumen y la misma velocidad. Es para lo que el binario se compila
  con `panic = "unwind"`.
- **Sin salida de audio al arrancar** (el servidor de sonido aún no estaba,
  unos auriculares sin conectar). Antes se tragaba las órdenes para siempre;
  ahora, cada vez que alguien pide sonido, se vuelve a intentar abrirla.
- **La salida muere sonando** (se cae el servidor, desaparece el aparato).
  cpal deja de pedir muestras sin avisar y la canción se queda «sonando»
  quieta. En el mezclador hay una fuente muda que cuenta cada vez que la
  salida pide muestras (el **latido**), y un vigilante lo mira también en
  pausa: si deja de latir, se rehace la salida y se sigue donde estaba.
- **Saltar con la salida muerta.** Buscar en rodio espera la respuesta del
  hilo de audio sin tope: con un DAC USB desenchufado, arrastrar la barra
  colgaba el reproductor para siempre. Ahora se mira el latido antes de
  buscar y la búsqueda va en un hilo aparte con plazo; si no contesta, se
  rehace la salida.

La salida se abre al primer play y se suelta tras un minuto sin sonar (con la
app en la bandeja, cpal no trabaja para nada); al volver, sigue donde iba. Si
ninguna salida llega a sonar tras tres intentos, se dice en vez de insistir.
El audio que decodifica ffmpeg (opus, wma, velocidad y tono) llega por una
tubería que lee otro hilo y deja en un anillo sin candados: antes se leía
dentro del propio callback de audio, y un tirón de disco era un corte.

Y la cola (`queue/`) ya no depende del núcleo para empezar a sonar: la
interfaz manda la ruta con cada canción, y solo si el archivo no está ahí se
le pregunta al núcleo. Cuando ni así se localiza, el error se dice en
castellano y, si la canción se acabó sola, se pasa a la siguiente.

## Estructura del proyecto

```text
danplay/        núcleo Python (índice, IA, etiquetas, descargas)
  api/            la API: app, guardia y arranque; routes/ (un router por
                  dominio), models.py (los cuerpos), jobs.py (tareas largas)
  library/        el índice: db (conexión, esquema y migraciones), scan,
                  search, songs, folders (carpetas, las que se mueven), history
  chat/           el asistente: tools (una por herramienta), loop, context,
                  narration (el detector de acciones narradas)
  providers.py    catálogo de proveedores de IA y perfiles guardados
  watcher.py      vigila las carpetas y mantiene el índice al día solo
  ytdlp.py        yt-dlp: el motor de JavaScript y la actualización en caliente
  thumbnails.py   las miniaturas de las carátulas, con tope y poda
  logs.py         el registro (a un archivo rotativo al servir)
  toon.py         resultados de herramientas en TOON: la mitad de tokens que JSON
  chats.py        las conversaciones con el asistente, guardadas y buscables
  model_catalog.py  el catálogo de modelos (models.dev), siempre al día
  data/           la foto del catálogo que viaja con la app
core/           crate Rust (PyO3): hashes en paralelo y forma de onda
desktop/        interfaz Vue 3 + envoltorio Tauri
  src/
    App.vue       la ventana principal (la ventanita y la proyección, aparte)
    router.js     las páginas (Ajustes, Asistente, Descargas, Entrada,
                  Duplicados), con carga diferida y el chat en <KeepAlive>
    composables/  estado compartido: useLibrary (lista y estado), useSelection,
                  useSongMenus, useSongList, usePlayback, useChat, useDownloads…
    playback/     la máquina de estados de la cola, en JS y sin dependencias
    components/   la interfaz, una página por archivo; ui/ los controles base
    styles/       los estilos, por áreas (base, listas, ui, responsive…)
    utils/        formato, teclas, grupos, carpetas, markdown del chat
  tests/        pruebas de la interfaz (Vitest, con dobles del núcleo y de Rust)
  e2e/          los flujos enteros en Chrome contra el núcleo de verdad
  src-tauri/src/
    core.rs       arrancar, vigilar y hablar con el núcleo Python
    queue/        la cola: qué suena y qué viene (model, session, logic,
                  resolve, worker, commands)
    player/       el audio (engine, output con el latido, open, metro, state)
    protocol.rs   danplay://cover, las carátulas
    tools.rs      dónde están ffmpeg y los demás programas
    tray/         bandeja: linux.rs (ksni) y desktop.rs (Windows/macOS)
    media.rs      MPRIS / SMTC
  src-tauri/permissions/  qué comandos puede invocar cada ventana
tests/          pruebas de Python, de las rutas de medios y de humo
packaging/      receta de PyInstaller para el núcleo empaquetado
scripts/        build.sh, test.sh e icons.mjs
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

Los dos crates de Rust son un **workspace** (`Cargo.toml` de la raíz): un solo
`Cargo.lock`, una sola carpeta `target/` y la versión de DanPlay en un solo
sitio, que heredan los dos y que Tauri y maturin leen de ahí.

Para desarrollar no hace falta nada de eso: `./scripts/dev.sh` abre la app
entera con la interfaz en caliente y el núcleo desde el código (ver
[Contribuir](CONTRIBUIR.md)).

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
