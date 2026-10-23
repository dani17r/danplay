# Cambios

Lo que cambia en cada versión que se entrega. El detalle de por qué está
hecho así, en [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

## 1.40.0 — 2026-10-23

El núcleo, en Rust y dentro de la app: lo que hacía en Python, ahora en
Rust (ver [docs/MIGRATION.md](docs/MIGRATION.md)). Por fuera todo sigue
igual; por dentro va más rápido y gasta menos.

- **Sin Python**: el núcleo va dentro de la propia app. Ya no hay un
  programa aparte que arrancar y vigilar: la app no espera a que se
  levante, gasta menos memoria (un proceso en vez de dos) y no abre ningún
  socket ni puerto, tampoco en Windows. El paquete pesa menos (el `.deb`, de
  166 a 145 MB, y una vez instalado, de 357 a 257 MB) aunque lleva más
  dentro: yt-dlp, su programa oficial, que se pone al día solo desde
  Descargas, y ONNX Runtime, que mueve el clic, separar en pistas y las
  voces de las guías.
- **Arranca más ligera**: la interfaz se carga por partes, mientras se usa:
  al abrir la app, solo lo que se ve (la mitad de código y un 40 % menos
  de estilos); el modo estudio, la ficha, las otras vistas de la lista o
  los ajustes llegan al pasar el ratón por su botón o en un rato libre, y
  ya están cuando se abren. La ventanita del reproductor y la proyección se
  crean la primera vez que se usan: con la app recién abierta son 3
  procesos en vez de 5 y unos 90 MB menos de memoria.
- **El índice en Rust**: la base se abre con SQLite desde Rust, y buscar,
  recorrer las carpetas, escanear, cada canción, las listas, lo abierto
  desde fuera, las conversaciones y el diario van en Rust. Recorrer las
  carpetas, de 1,5 a 4 veces más rápido; leer las etiquetas del escaneo, en
  paralelo.
- **El análisis en Rust**: el clic (Beat This!) y separar en pistas (Demucs
  y el Mega53) corren en ONNX Runtime desde Rust; separar gasta unos 300 MB
  menos de memoria.
- **Y más**: los nombres, la teoría y los cifrados, la conversión, la forma
  de onda, las miniaturas, los duplicados, la guía hablada, la rejilla del
  clic guardada y «Re-analizar el clic», enriquecer una canción (la letra,
  la carátula, la ficha de la IA, su cifrado y rellenar lo que falta) e
  importar lo que hay en Entrada/.
- **Arreglado**: el segundo intento de convertir un archivo (sin la
  carátula) no funcionaba nunca; un tiempo «inf» en un marcador se guardaba
  como algo que la interfaz no podía leer; si algo escribía en la base a la
  vez que un escaneo (el diario, una descarga), podían quedarse sin entrar
  las canciones de esa tanda; y una base borrada con la app abierta volvía
  vacía, sin poder guardar nada, hasta reiniciar. Y lo que destaparon las
  pruebas al pasar a Rust: un error de yt-dlp escrito con color perdía su
  motivo («yt-dlp terminó con el código 1»); el clic de una canción se podía
  calcular dos veces si se pedía justo cuando salía de la cola; y los textos
  que llegan de fuera (títulos de YouTube, páginas) dejaban pasar algunos
  caracteres de formato invisibles.
- **Lo de dentro, en inglés**: carpetas, crates, módulos, funciones y
  variables (lo que se ve sigue en castellano). Lo guardado con nombre en
  castellano se renombra solo al arrancar, sin perder nada: en los datos de
  DanPlay, `pulso/`, `separador/`, `guias/`, `voces/`, `diario.json` y
  `terminos.json` (y si conviven el nombre de antes y el de ahora, se
  juntan, con lo más reciente de cada cosa); en las carpetas de música, `.danplay-pistas.json`,
  `.danplay-ondas.json` y `.danplay/diario.json` (los de antes se siguen
  leyendo); y las preferencias de la interfaz. El `.danpista` del celular no
  cambia.
- **Para quien compila**: Python ya no está en el repositorio. Sus casi
  1.000 pruebas, pasadas a Rust una a una con los mismos valores esperados
  (dónde está cada una, en [docs/TESTS-PORT.md](docs/TESTS-PORT.md)); las de
  la interfaz de extremo a extremo, contra `danplay-server`; los datos que
  viajan con la app, de `danplay/data` a `data/`. Los paquetes, sin
  PyInstaller ni uv; `scripts/data.sh` deja los datos y ONNX Runtime en
  `desktop/src-tauri/data/`, la foto del
  catálogo de modelos la saca la propia app (`--models-snapshot`) y
  `build-windows-cross.sh` ya no lleva el Python embebido ni pide
  `--no-crate`. QuickJS-NG en vez de Deno para los retos de YouTube
  (−93 MB). Las órdenes de los scripts, en inglés:
  `./scripts/github.sh tests|packages|download`, `./scripts/test.sh
  --quick`, `./scripts/dev.sh --sandbox [--clean]`, `build-android.sh
  --emulator`, `tools-macos.sh --with-ffprobe` y `build-windows-cross.sh
  --installer --with-tools`. La interfaz se instala con Bun y se compila,
  se prueba y se formatea con Vite+ (`vp`, con Oxfmt en vez de Prettier).

## 1.30.0 — 2026-10-17

El modo estudio como una secuencia: cada cosa por su lado desde su carril,
la guía automática que anuncia las partes de la canción, exportar en una
ventana clara y un recorrido que lo enseña todo con animaciones.

### Canales, en cada carril

- **L y R en cada carril** (la canción, cada pista, el clic y cada carril de
  guías): la canción por la derecha y las guías por la izquierda, o cualquier
  instrumento a un lado. Con una interfaz de más salidas, un botón por
  salida (1, 2, 3…). **Est / Mono** al lado: por un solo canal va en mono;
  por dos, se elige.
- **El clic tiene su carril** debajo de la onda, con sus golpes, callarlo,
  su volumen y sus canales.
- «Salidas» queda para la salida de audio y los repartos de siempre; cada
  cosa por su sitio se elige en su carril (en el celular, en «Salidas»).
- **«Mezcla» y «Canales»** en la barra de la onda ocultan esos mandos de los
  carriles: más sitio para ver la canción y las guías.

### Guías

- **Guía automática** (✨, junto a «Guía»): DanPlay busca las partes de la
  canción (intro, estrofas, coros, pre coro, puente, solos de guitarra,
  piano o batería, final) y las dice justo antes de cada una, en su carril
  «Partes». Con las pistas separadas sabe dónde canta la voz y quién hace un
  solo. Si ya tienes una voz elegida, la hace sin preguntar; otra vez, la
  rehace.
- **Cambiar la voz** de las guías de un carril (en su «⋯»): se vuelven a
  decir con la voz nueva.

### Exportar

- **Un solo «Exportar…»** que abre una ventana: la canción, la mezcla de las
  pistas o para el celular, y con qué (las guías, el clic, la velocidad y el
  tono, los canales), cada cosa explicada. Lo marcado se recuerda.
- **Con los canales:** el archivo suena como lo oyes, cada cosa por su lado
  (L o R) y en estéreo o en mono, para usarlo como una secuencia.

### Estudio

- **Con la onda bloqueada, «ir con un clic»** (junto al candado): un clic en
  la onda lleva la canción ahí, también sonando, sin quitar el tramo.
- **Un recorrido** por el modo estudio, con animaciones que se mueven solas
  (arrastrar un tramo, poner una guía, mandar cada cosa a su lado): sale la
  primera vez y con «?».
- **Los menús, cortos:** el nombre de cada opción se ve entero, lo largo va
  en un aviso y la opción puesta (el estilo de la onda, «al acabar») va
  marcada en color, en vez de decir «puesto».
- Velocidad y tono, en una columna más estrecha: más sitio para el clic, los
  marcadores y las notas.

## 1.28.3 — 2026-10-17

El celular: su modo estudio a la vista, las guías solo para oírlas, las
salidas a mano y la música del teléfono, también la de la tarjeta SD.

### Arreglos

- **«Se pausa solo»**: con un tramo elegido, «Al acabar» viene en «parar», y
  al final del tramo la canción se para y vuelve a su principio. En el
  teléfono esa opción quedaba tapada y no se veía. Ahora está a la vista y,
  cuando se para así, se dice (también en la PC).
- **En el teléfono, la cabecera del estudio y la barra del tramo se
  aplastaban** y se montaban unas sobre otras.
- **«Salidas» no se podía desplazar**: con varias pistas no se llegaba al
  clic ni a las guías.
- **La música del teléfono no encontraba tus carpetas**: solo miraba la
  memoria interna (sin la tarjeta SD ni las carpetas de Telegram o
  WhatsApp) y salían los tonos del sistema. Ahora lo dice Android.
- Escuchar una guía en el teléfono daba error.

### Celular

- **El tramo, en dos filas:** su tiempo con «Ahora», guardar y quitar; debajo,
  «Al acabar el tramo: repetir · parar · seguir».
- **«Salidas» en la cabecera del estudio**, junto a «⋯» y ocultar: el clic y
  las guías a un lado y la música al otro, como una secuencia, cada cosa en
  estéreo o en mono y en su tarjeta.
- **Las guías vienen de la PC:** en el teléfono se oyen y cada carril se
  calla, se deja solo o cambia de volumen, como las pistas; se hacen y se
  cambian en la PC.
- Los paneles (carpetas, salidas), con su margen.

## 1.28.2 — 2026-10-16

El modo estudio del escritorio, más a mano: los mandos encima de la onda,
iconos con su aviso en vez de textos largos, y las guías con su onda, un
editor, importar audios y el imán a los pulsos.

### Arreglos

- **Escuchar una guía daba un error** en Linux (y no sonaba): la escuchaba
  el WebView, que depende de los complementos de GStreamer. Ahora la escucha
  el motor de audio, por la salida del sistema.

### Estudio

- **Los mandos de la canción encima de la onda:** al principio, 5 s atrás,
  sonar o pausar, detener, 5 s adelante y dónde va, sin bajar al reproductor.
- **Menos texto y más iconos:** la cabecera y la barra de la onda son
  iconos; al dejar el ratón encima un momento sale un aviso con lo que hace
  cada uno. Lo que se hace con el ratón sobre la onda va en su botón de
  ayuda.
- **Más sitio para la onda:** sin pistas separadas, la columna de la
  izquierda se estrecha y la onda se llama «Canción».
- Movimiento y transiciones: los iconos que cambian, las guías que
  aparecen, la tarjeta de la guía y la lista.

### Guías

- **Se llaman «Guías»**: en ellas va cualquier cosa que ayude a tocar la
  canción, no solo frases habladas.
- **Importar un audio corto** del equipo (WAV, MP3, FLAC, OGG, M4A…), que no
  dure más que la canción, como mucho cinco minutos: en el «⋯» de cada carril
  o con el clic derecho sobre él, y en la tarjeta de la guía.
- **Un editor para lo grabado o importado:** su onda, recortar los bordes
  arrastrándolos, fundido de entrada y de salida, quitar el ruido de fondo,
  quitar los graves y golpes del micrófono e igualar el volumen. Se escucha
  con los cambios antes de guardar, y se guarda como una guía nueva (la de
  antes no se toca).
- **Cada guía con su onda** en su carril, y lo que dura de verdad.
- **El imán:** con él puesto, las guías se pegan a los pulsos del clic al
  ponerlas o moverlas (y antes y después van de pulso en pulso). Usa los
  pulsos de la canción, así que sigue sus cambios de tempo.
- **Clic derecho en una guía:** escucharla, editarla, ir a ella, pasarla a
  otro carril o quitarla.
- **En la lista**, en orden: pulsar una lleva a ella en su carril, que
  destella.

## 1.28.1 — 2026-10-16

Arreglos de la 1.28 en Linux, las guías en sus carriles, exportar con o sin
guías, la onda a tu gusto y las salidas más claras.

### Arreglos

- **En el paquete de Linux no se veía la onda ni se podía separar** («ffmpeg
  no pudo leer la canción … CXXABI_1.3.15 not found»): el núcleo empaquetado
  hacía que ffmpeg cargara bibliotecas viejas que lleva dentro. Las ondas y
  el clic que ya estaban hechos se veían; lo nuevo, no.
- Sin onda, se dice por qué (antes, siempre «hace falta ffmpeg»).
- En «Salidas», mandar una pista a su propia salida podía hacerla sonar por
  la principal.
- Una mezcla con más de 8 pistas (la calidad máxima, una secuencia) no se
  podía guardar.

### Todo dentro

- **Los paquetes de Linux traen ffmpeg, fpcalc y Deno**, como ya los de
  Windows y macOS: no hay que instalar nada aparte, y descargar de YouTube
  funciona sin más.

### Guías habladas

- **En sus carriles**, debajo de la onda, como las pistas separadas: hasta
  seis, cada uno con su nombre, callar, solo, volumen y su salida (uno a los
  cascos del baterista y otro a la banda). Con la canción o con sus pistas.
- **Solo con la canción parada** se ponen (doble clic en el carril), se
  mueven (arrastrándolas) y se cambian (pulsándolas).
- **Exportar** la canción tal cual o las pistas como suenan, con las guías o
  sin ellas: en el menú del estudio.

### Estudio

- **Seis estilos para la onda:** barras, bloques, rayitas, suave, contorno y
  con reflejo. En el menú del estudio o con el clic derecho sobre la onda.
- **La rueda, como en un editor de música:** Ctrl + rueda (o pellizcar)
  acerca, Mayús + rueda recorre la canción y la rueda sola baja el panel.
- **Estéreo o mono**, cada cosa en «Salidas» (para una mesa o una sala en
  mono).
- **Solo las salidas que tienes:** en Linux salían unas 35 (complementos de
  ALSA y cada tarjeta con todos sus nombres); ahora, una por salida de
  verdad. Si el sistema tiene ocupada la elegida, suena por la del sistema
  y se avisa.

## 1.28.0 — 2026-10-16

El fuerte de DanPlay, la pista y el metrónomo, rehecho para tocar en directo:
un motor de audio nuevo sin cortes, el clic arreglado (sin silencios ni
saltos), las salidas a la mesa como una secuencia, guías habladas, más
instrumentos separados, abrir secuencias descargadas, listas que no se
rompen aunque muevas la música, y «Acerca de DanPlay» con las novedades, los
términos y los créditos.

### El reproductor y el modo estudio

- **Motor nuevo, sin cortes:** los tramos se repiten a la muestra exacta, el
  clic va pegado a la música a cualquier velocidad, y pasar de la canción a
  sus pistas (o cambiar una pista) entra a la par, sin salto.
- **Varios tramos:** se tocan uno detrás de otro hasta el final, o en bucle.
  «Al acabar: Repetir · Parar · Seguir» funciona (antes se repetían siempre).
  Quitar «varios» deja uno solo.
- **Marcadores:** se eligen uno o varios y se tocan en bucle o una vez.
- **Ocultar el estudio ya no quita nada:** el clic, los tramos, la velocidad
  y el tono siguen sonando. Solo «Salir del modo estudio» vuelve a la canción
  tal cual.
- **El tono en palabras:** «medio tono», «1 tono», «1 tono y medio», «¼ de
  tono».
- **La onda se acerca con la rueda del ratón:** se ven los clics y se ponen
  marcadores y guías donde van.
- **Diseño del estudio** más ordenado, en pestañas cuando no cabe.

### El metrónomo

- **Más fuerte**, y dos sonidos nuevos: claves y charles.
- **El clic propio:** los pulsos se mueven a mano sobre la onda y se guardan
  con la canción. Clic automático, el propio o uno a mano para todas.
- **Sin silencios ni saltos:** el análisis nuevo repara los saltos de fase
  (el clic que se iba a contratiempo unos compases), rellena los huecos donde
  dejaba de sonar y, si la canción se grabó a clic, lo lleva a tempo fijo. Con
  la batería separada, se apoya en ella.
- **«Re-analizar…»:** de toda la canción o solo del tramo elegido: arreglar
  huecos y saltos, tempo fijo, con la batería separada o rellenar silencios.
  Tarda nada: se guarda lo que oyó la red.
- Se pasó el análisis nuevo por toda la biblioteca.

### Las salidas: como una secuencia en directo

- **«Salidas»:** por dónde suena la música, cada pista, el clic y las guías.
  Todo junto; clic y guías a la izquierda y música a la derecha (un cable en
  Y a la mesa y a los cascos); clic y guías por 1-2 y música por 3-4 (una
  interfaz de 4 salidas); o cada cosa a medida, en estéreo o en mono. Se
  cambia en directo, sin cortar. También en el celular.

### Guías habladas

- Un punto de la canción dice «Coro», «Puente, cuatro tiempos», «Sube medio
  tono»: **escrito** y dicho por una voz (diez voces naturales y voces de
  España, México y Argentina, en el equipo y sin internet salvo para bajarlas
  una vez) o **grabado** con tu micrófono. Suena justo ahí, por la salida de
  las guías, a cualquier velocidad.

### Separar en pistas

- Mientras se separa una canción, la siguiente se pone en la cola y lo dice.
- **Más instrumentos (máxima calidad):** la percusión aparte de la batería
  (congas, pandereta…), la voz principal y los coros, guitarra eléctrica y
  acústica, piano, órgano, teclados, cuerdas, metales y vientos, los que
  suenen. Es lenta (unos 15-25 min por canción) y pide unos 5 GB de memoria
  libre; se baja su separador la primera vez (1,4 GB).
- «Llevar al celular» se llama ahora **«Convertir a DanPista»**.

### Secuencias descargadas

- **«Abrir secuencia»:** la carpeta de una secuencia (MultiTracks, Loop
  Community: sus pistas, el «Click Track», la «Guide» y el proyecto de
  Ableton) queda lista para tocar: la canción en «Secuencias», sus pistas en
  el estudio (el clic y la guía a la salida del clic), sus partes como
  marcadores y el clic exacto del proyecto. Abrir con DanPlay una de sus
  pistas también la ofrece.

### Tu música, donde esté

- **Mover la carpeta de música o volver a bajar una canción ya no pierde lo
  hecho:** se reconoce la misma canción (aunque sea otro archivo) y recupera
  sus pistas separadas, su clic, sus marcadores y sus listas, corrigiendo lo
  que traiga de más o de menos la descarga nueva. Lo que ya se había perdido
  se recupera solo al abrir la app.
- **Abrir un archivo pone detrás los de su carpeta**, como cualquier
  reproductor.

### Las listas ya no se rompen

- **Un diario junto a tu música:** tus listas (en su orden, con su nota y su
  color), las estrellas, las favoritas, lo del modo estudio y los cifrados se
  guardan también en `.danplay/diario.json`, dentro de tu carpeta de música,
  unos segundos después de cada cambio. Si mueves la música a otro disco,
  renombras carpetas o canciones, o la base de DanPlay se pierde, al abrir se
  compara con lo que hay y todo vuelve a su sitio. Las listas exportadas
  (`Listas/*.m3u8`) se reescriben con las rutas nuevas.
- **Una canción que desaparece sigue en su lista** como «No está», en su
  sitio y con su nombre: su menú la busca en tu biblioteca, la busca para
  bajarla otra vez o la quita. Si vuelve (con otro nombre o en otra carpeta),
  la lista la recupera sola. La barra lateral dice cuántas faltan en cada
  lista.
- **Al mandar una canción a la papelera desde DanPlay**, se pregunta si sale
  también de sus listas (marcado por defecto): así no se queda como «No
  está» si de verdad la querías fuera.
- Al abrir, un aviso dice lo que se recuperó.

### Acerca de DanPlay

- **Novedades:** lo de cada versión, corto. Tras una actualización, la barra
  lateral dice «Nuevo» y un aviso lo cuenta al abrir.
- **Términos y condiciones:** salen la primera vez que se abre DanPlay; para
  aceptarlos hay que leerlos hasta el final y marcar la casilla. Sin
  aceptarlos se puede escuchar y ordenar la música, pero no separar en
  pistas, usar el asistente de IA ni «Transformar»: al intentarlo, salen.
- **Créditos:** DanPlay es obra de Daniel Rangel, y se dice en la página,
  en la bienvenida y al pie de la barra lateral, con su Telegram
  ([@Dani17r](https://t.me/Dani17r)) para quien quiera colaborar.

### En el celular

- **Música del teléfono:** cualquier canción del teléfono (MP3, M4A, FLAC…),
  no solo las pistas traídas, eligiendo las carpetas como en la PC. Al poner
  una, siguen las de su carpeta. Con su modo estudio y su clic.
- **El volumen de la app**, a mano y a la vista en el reproductor.
- Las salidas y las guías habladas, como en la PC.
- «Acerca de DanPlay» en el menú: los créditos, las novedades y los términos.

## 1.27.0 — 2026-10-14

La app del celular se rehace para los dedos: todo un poco más grande y con
más aire, un menú de lado con tus listas y los ajustes, un reproductor nuevo,
el modo estudio a la medida de la pantalla y los mandos en la bandeja de
Android. Y se llama como la de la PC: **DanPlay**. Solo cambia el celular.

### En el celular

- **Poner una canción:** doble toque en ella. Con un toque se elige y abre
  debajo sus botones grandes (Reproducir, Estudiar, Letra); la carátula
  también la pone, y mantenerla pulsada (o su ⋮) da el resto de opciones. En
  Ajustes se puede hacer que un toque baste.
- **El reproductor:** abajo, una barra baja con la carátula, el título, sonar
  y la siguiente. Tocándola se abre entero: la carátula grande, la barra de
  tiempo para el dedo, saltos de 10 s, aleatorio, repetir, y el estudio, la
  letra y la cola a mano. Se cierra con la flecha, con «atrás» o bajándolo.
- **El modo estudio, más fácil:** desliza hacia arriba la barra del
  reproductor y se abre; bájalo por su cabecera y se cierra. Ocupa toda la
  pantalla, con la onda a todo lo ancho y el nombre de cada instrumento sobre
  su carril, y debajo cada pista con M, S y su volumen, grandes. «Varios
  tramos» y el candado van encima de la onda, no sobre ella: ya no tapan el
  final de la canción. Con el estudio abierto, la barra de abajo lleva la
  hora, la barra de tiempo y los saltos de 10 s.
- **El menú de lado (☰):** Mis pistas, Favoritas, **tus listas** (la del
  domingo, la del ensayo: se crean, se ordenan subiendo y bajando canciones y
  se tocan en orden o al azar), Importar, Ajustes y cómo traer pistas de la
  PC.
- **Los mandos en la bandeja de Android y en la pantalla de bloqueo:** pausa,
  anterior, siguiente y la barra de tiempo, con la portada. Funcionan con la
  app en segundo plano, y también los botones de los auriculares.
- **Ajustes** con botones grandes: el tamaño de la letra, el tema, filas más
  bajas, el fundido entre canciones y si la pantalla se queda encendida.
- **Letra y acordes** a pantalla completa y con letra de leer.
- **Los nombres largos** se cortan con cuidado (dos líneas en el reproductor
  y en la ficha, una en las listas) y nada queda pegado; probado también en
  teléfonos de 360 px.
- Fuera el recuadro azul que Android pintaba al tocar los botones: ahora cada
  botón responde con un pequeño hundimiento, y las transiciones son solo de
  movimiento, fluidas en cualquier teléfono.
- **Al instalarla:** sale el logo de DanPlay (antes, el de Tauri), la versión
  de verdad (antes decía 1.0) y va firmada con las tres firmas de Android
  (v1, v2 y v3). La pantalla de arranque ya no da un fogonazo blanco.
- El APK se llama ahora `DanPlay-X.Y.Z-android.apk`. Se instala encima de la
  1.26 sin perder las pistas.

## 1.26.0 — 2026-10-14

Lo que separas en la PC se estudia ahora en el celular: una app nueva para
Android, **DanPlay Pistas**, con la misma cara que DanPlay y nada de
convertir ni ordenar. Solo importa las canciones ya separadas y las deja
estudiar, para ti y para tu banda.

### En el celular

- **Cada parte con su volumen:** callar la batería para tocar encima, dejar
  sola la voz para aprenderla, subir el bajo. Las mismas M y S que en la PC.
- **El clic en el pulso de la canción**, el que calculó la PC, con su «1»,
  su compás y la cuenta para entrar. También a menos velocidad y en la vuelta
  de un tramo.
- **Velocidad sin cambiar el tono y tono sin cambiar la velocidad**, los
  **tramos** que se repiten, la **letra** y los **acordes** (también en otro
  tono).
- **Lo que dejaste puesto en el estudio de la PC viaja con la canción**: la
  mezcla, los tramos, los marcadores. Lo que cambies en el teléfono se queda
  en el teléfono.
- **Abrir con DanPlay Pistas:** toca el archivo en WhatsApp, Descargas o
  Drive y se importa solo; o desde la app, con «Importar» (varios a la vez).
- Mientras suena, la pantalla no se apaga; y con «atrás» la app pasa a
  segundo plano sin cortar la música.
- El APK está en `dist/installers/` (Android 8 o más nuevo). La de iPhone
  queda preparada: falta compilarla en un Mac y firmarla con Apple.

### En la PC

- **Llevar al celular**, en el menú de las pistas de una canción (el «⋯» del
  estudio y el clic derecho; con varias elegidas, todas): un archivo
  `.danpista` por canción en `Celular/`, junto a `Separadas/`, de 20 a 50 MB.
  La carpeta se abre sola para arrastrarlo a WhatsApp o al teléfono.
- El modo estudio en una ventana estrecha (700 px o menos) ya no monta las
  secciones unas encima de otras.

## 1.25.0 — 2026-10-11

El clic ya no se hace esperar a ciegas y sigue a cada canción, pasar de la
canción a sus pistas no se nota, las canciones se funden al pasar de una a
otra, «Transformar» ordena tu carpeta canción por canción, el estudio puede
ocupar todo el alto y todo se mueve con suavidad.

### Clic

- **«Clic automático».** Cada canción trae su clic y sus ajustes, también al
  pasar de una a otra y con el estudio cerrado (antes, con el estudio
  cerrado, seguía el de la anterior, suelto). La primera vez de cada canción
  tarda unos segundos: mientras tanto el clic no suena (si venía sonando, se
  calla), su botón no deja ponerlo y la onda se va encendiendo con lo que
  lleva; al llegar, vuelve solo. Con el clic puesto, las siguientes de la
  cola se preparan por adelantado.
- **Sin él, a mano:** el tempo que pongas sigue igual en todas las
  canciones, sin esperar a nada.
- **Lo calculado ya no se pierde.** Al guardar algo del estudio en la
  canción se perdía su clic y había que volver a calcularlo (15-20 s); en tu
  biblioteca, 11 de los 19 guardados ya no valían. Ahora se conserva, también
  al mover la canción.

### Reproductor

- **De la canción a sus pistas sin corte**, y al revés, y al cambiar la
  velocidad o el tono: la nueva entra a la par justo donde iba la otra.
- **Fundido entre canciones:** la que se va baja mientras sube la siguiente.
  Se elige en Ajustes › Reproducción (de 0 a 12 s; de fábrica, 4). Al cambiar
  tú de canción, como mucho 2 s; con el clic o tramos puestos, sin fundido.
- Si se mueve la canción que suena, sigue sonando y lo que la vuelve a abrir
  (un salto, las pistas, otra velocidad) la abre de su sitio nuevo.

### Transformar

- Un botón **«Transformar»** en la barra de arriba pone cada canción en su
  sitio y con su nombre (`Artistas/Artista/Artista - Título`), **una a una**:
  dónde está, dónde iría y por qué. Se aplica, se corrige el artista o el
  título antes, se salta o se deshace; «Aplicar las seguras» hace las
  seguras seguidas. «¿Qué hace?» lo explica.
- Es la misma canción (estrellas, listas, estudio, pistas y clic van con
  ella) y, si está sonando, sigue sonando. Tus carpetas propias (Pistas,
  Tutoriales…) no se tocan, y lo que solo dicen las etiquetas se propone para
  revisar: en una biblioteca de verdad, fiarse de ellas dejaba peor muchos
  nombres.

### Estudio e interfaz

- **«Grande»**: el estudio a todo el alto, sin tapar el reproductor; si lo
  cierras grande, se vuelve a abrir grande.
- **Movimiento** en ventanas, menús, botones, campos, el interruptor, las
  pestañas y al cambiar de página, corto y suave (y nada si el sistema pide
  movimiento reducido).

## 1.24.1 — 2026-10-11

Dos arreglos del asistente, y los paquetes de Mac, también para Intel.

### Arreglos

- **La tabla de la lista ya no dice «falta» de lo que acaba de entrar.** Al
  terminar la descarga, cada canción que entró en la lista aparece en su
  fila como «bajada, en la lista», también al volver a abrir la
  conversación.
- **Cuando YouTube frena, se deja de preguntar.** Al primer «demasiadas
  peticiones» o «no eres un robot», las demás búsquedas de la tarjeta se
  paran en vez de insistir.

### Para quien desarrolla

- El paquete de Mac Intel lleva onnxruntime 1.23.2, la última versión que
  existe para ese procesador (los tres modelos cargan con ella).
- `scripts/build-macos.sh` lanza la compilación de Mac y deja los `.dmg` en
  `dist/installers/`. El portátil de Windows también sale ahí, con la
  versión en el nombre: todos los instaladores en una sola carpeta.
- Las pruebas con tope de tiempo se ajustan a la velocidad de la máquina, y
  la del socket usa una ruta corta (en macOS se pasaba del límite).

## 1.24.0 — 2026-10-10

El asistente mira primero tu biblioteca y entiende la lista del domingo tal
como llega por WhatsApp; elegir varias canciones a la vez tiene botón,
casillas y barra; y el reproductor ya no se corta cuando el disco está
ocupado.

### Asistente

- **Entiende la lista de WhatsApp.** Pégala tal cual (negritas, emojis,
  @menciones, «Voz», enlaces de YouTube) y la app la lee por su cuenta: las
  @menciones son quien canta o toca, nunca el artista, y el orden numerado
  manda. «Banda:» o «Ensayo:» no se toman por canciones (y te lo dice).
- **Biblioteca primero, siempre.** Cada canción se busca en tu biblioteca
  antes de pensar en YouTube, sin tildes ni mayúsculas, con erratas y aunque
  le sobre el nombre de quien la canta. Si la tienes en varias versiones, usa
  la que ya está en tus listas. Pedir bajar algo que ya tienes no pide
  permiso: te dice dónde está.
- **Arma la lista con lo que hay mientras te pide permiso para lo demás.** Lo
  que tienes entra al momento, en el orden de tu lista; lo que falta aparece
  en una **tarjeta** bajo la respuesta, con lo que encontró en YouTube (se va
  rellenando sola), casillas y «Descargar». Al terminar, cada canción entra
  en su sitio de la lista, sin que tengas que pedirlo otra vez. Nada se queda
  esperando: puedes seguir escribiendo.
- **Más rápido.** Meter la lista del domingo es una sola llamada al modelo y
  1–2 segundos (con tu conversación real: antes, unas 6 llamadas y 20 s, y a
  veces «Me he enredado»). El primer mensaje tras abrir la app ya no espera
  a cargar la IA.
- **Hace lo que pides, y nada más.** «Verifica» compara y no cambia nada;
  dejar la lista exacta (quitando lo que sobra) solo si lo dices; una lista
  nueva solo si la pides. La tabla de la lista trae «Dejar solo estas»,
  «Ordenar como mi lista» y «Deshacer».
- **Un «sí» o un «no»** escrito contesta la tarjeta, como el botón.
- **Escribir.** La caja es de varias líneas (Shift+Enter baja de línea, pegar
  conserva las líneas), tus mensajes se ven con su formato (negritas, listas)
  y lo que escribes mientras responde queda en cola.
- **Quién contesta, a la vista.** Si la IA que elegiste falla (sin saldo,
  clave rechazada, saturada), una franja lo dice y quién contesta en su lugar,
  con «Reintentar con el principal» y «Usar el respaldo como principal» (que
  pregunta antes de cambiar nada). Una IA sin saldo ya no gasta una llamada
  fallida en cada mensaje.
- Una insignia en «Asistente» cuando una propuesta espera tu respuesta y
  estás en otra página.

### Elegir varias canciones

- **Botón «Seleccionar»**: casillas en cada canción, en todas las vistas (y
  la de «todas» en la cabecera de la tabla).
- **Barra** con cuántas son y qué hacer con ellas: añadir a una lista,
  favoritas, Telegram, quitar de la lista, papelera y el menú completo.
- **Ctrl+A** elige todas, **Supr** quita de la lista (en un repertorio) o
  manda a la papelera (con confirmación), y **Escape** suelta la selección.
- **Arrastrar varias** a la vez a un repertorio, o reordenar un bloque dentro
  de uno.
- **En el buscador** también: «Seleccionar» en el desplegable, Ctrl/Mayús +
  clic y Ctrl+Espacio desde la caja; añadir a una lista sin que se cierre.

### Buscar

- La búsqueda trata los nombres sin tildes ni símbolos y en minúsculas, y la
  ñ se respeta.
- Cada resultado del desplegable tiene sus opciones (clic derecho o «⋯»):
  añadir a una lista, favorita, letra, papelera… y el desplegable sigue
  abierto para hacer varias cosas seguidas.

### Reproductor

- **Ya no se corta cuando el disco está ocupado.** La canción se lee a
  memoria por delante mientras suena: separar pistas, analizar, descargar o
  escribir etiquetas a la vez ya no la interrumpe (medido: de cortes de hasta
  1,4 s a ninguno).
- Con velocidad, tono o pistas separadas, más colchón (8 s), y si ffmpeg se
  retrasa, la aguja y el clic esperan en vez de adelantarse.

### Arreglos

- Las listas ya no se pisan cuando se tocan a la vez (el asistente, una
  descarga y un arrastre): sin posiciones repetidas ni canciones perdidas.
- Una descarga que falla dice por qué (YouTube pide verificar, demasiadas
  peticiones, sin red). Un enlace compartido desde un mix ya no consulta la
  lista entera.
- En WebKit (la app en Linux) no se veían la raya de dónde cae una canción
  arrastrada ni la marca de la fila elegida en la tabla.
- La cabecera de un grupo en la tabla agrupada ya no deja columnas fantasma;
  las estrellas de la lista fina se esconden en ventanas estrechas.
- Un Enter pulsado fuera de un campo ya no acepta el diálogo que se abre.
- El título de una conversación sale sin el formato de lo pegado.

### Para quien desarrolla

- `macos.yml`: el paquete de macOS (Apple Silicon e Intel) desde la
  integración continua, sin firma de Apple.
- El banco del asistente con un modelo de verdad vuelve a funcionar y tiene
  los casos de la lista del domingo; las pruebas tienen además un servidor
  OpenAI de mentira para el chat con el SDK real.

## 1.23.0 — 2026-10-07

La ventana se adapta a cualquier tamaño, y el reproductor tiene formas:
pantalla completa, media pantalla, columna, cuadrito y barra.

### Ventana

- **Cualquier tamaño.** La ventana ya se puede hacer pequeña de verdad, y
  cada tamaño tiene su diseño en vez de apretar el de siempre. En estrecho la
  barra de abajo se pisaba (la hora encima del título, la aguja cruzando los
  botones) y en mediano la aguja se quedaba sin sitio y los botones de la
  derecha se salían: ahora la aguja va arriba a todo lo ancho y, cuando no
  cabe todo, lo que falta está en **⋯**.
- **Estrecho, como en el móvil**: la canción a todo lo ancho, la aguja y
  los mandos debajo. En la lista, el artista va en la línea del título, en
  gris.
- **Formas de la ventana**, en Vista, en el botón de la barra o en ⋯:
  normal, pantalla completa (**F11**), maximizada, media pantalla izquierda
  o derecha, **columna** (estrecha y alta, a la derecha), **cuadrito**
  (la carátula y los mandos en una esquina) y **barra** (el reproductor en
  una tira abajo). El cuadrito y la barra van sin marco y siempre encima, y
  se arrastran desde el propio reproductor. **Siempre encima** se puede
  poner en cualquier forma. «Normal» la deja como estaba.
- Lo largo (títulos, artistas, repertorios) se corta con «…» y se lee
  entero al pasar por encima. Con la ventana baja, el menú de la izquierda
  se desplaza entero: los repertorios ya no se montan encima de «Gestión».

## 1.22.0 — 2026-10-07

El metrónomo, revisado canción por canción sobre una biblioteca de verdad:
ya no se acelera a ratos, no mete golpes donde no van, el «1» cae donde
toca y el clic está más sentado.

### Metrónomo

- **Un solo pulso de principio a fin.** La red que saca el pulso lo oía, a
  tramos, al doble (el «y» de cada tiempo) o a la mitad, y el clic iba a 68
  y de pronto a 136 dentro de la misma canción; con ×2 encima, esos tramos
  iban a cuatro veces el tempo. Ahora se elige el pulso que se mantiene
  estable, y los golpes sueltos de más o de menos desaparecen.
- **El «1» donde toca.** Los compases salían de 1, 2, 5 u 8 tiempos cuando
  la red no oía un «1»; ahora el compás se mantiene, y solo cambia (el 2/4
  antes del coro) si la canción lo marca de verdad. El acento vuelve a
  servir.
- **Más preciso.** Cada clic caía hasta 10 ms antes o después de su sitio (la
  red mira cada 20 ms); ahora, a 1–2 ms.
- **Sin clic donde no hay pulso**: una intro libre, un final en vivo sin
  tempo o un acorde que se apaga ya no llevan clics a trompicones. Un trozo a
  tempo en el que la banda calla sigue con clic.
- **A otra velocidad, a tiempo.** Despacio (y con el tono corrido), el clic
  llegaba tarde: unos 20 ms a 0,5×. Corregido.
- Las rejillas guardadas se vuelven a calcular solas la próxima vez.
- `scripts/check-metronome.py` pasa por toda tu biblioteca y dice, canción
  por canción, qué hace mal la red y cómo queda el clic.

## 1.21.0 — 2026-10-07

Los acordes y el tono salen de cifrados publicados por músicos, con su
enlace, y no de la memoria de la IA. Si no se encuentran, se dice.

### Acordes

- **Buscar acordes**, en la ficha de la canción: se buscan a la vez en
  Ultimate Guitar y en LaCuerda, cada una con su propio buscador (sin claves,
  y sin buscadores generales, que cortan si se les pregunta seguido). Tarda
  unos 5 segundos y no necesita IA.
- **Leídos tal cual.** Los acordes son los que la página marca como acordes,
  por partes (intro, verso, coro…). Con **Cifrado entero** se ven sobre la
  letra. Siempre se dice de dónde salen («De Ultimate Guitar: … · 157
  votos»), y un clic abre la página para comprobarlo.
- **El tono, solo si el cifrado lo dice.** Si no lo dice, pone «El cifrado
  no dice el tono» y se puede subir o bajar por semitonos. Con tono, se
  transpone a otro como antes. Entre varias versiones de la misma canción
  gana la que dice su tono y han votado otros.
- **Si no está, no se inventa.** «No hay cifrado de esta canción en Ultimate
  Guitar ni LaCuerda», y **Buscar otra vez** cuando quieras. Las versiones de
  otros, las drum cams y los popurrís no se buscan: no se sabe de quién es la
  original.
- Medido en 80 canciones de una biblioteca de alabanza: encuentra 58 (72 %;
  82 % sin versiones ni popurrís), todas la canción correcta, y 28 traen su
  tono. `scripts/evaluate-chords.py` lo mide sobre la tuya.

### Asistente e IA

- La IA **ya no da tono ni acordes**: antes los decía de memoria, como
  aproximación. «Ver detalles IA» queda para el álbum, los artistas y el
  contexto.
- El asistente tiene `get_chords`: busca el cifrado, da la fuente y el enlace,
  y si no lo encuentra tiene dicho que no los saque de memoria.
- **Rellenar información** pone el tono desde el cifrado, solo si lo dice y se
  toca sin cejilla (con cejilla, lo escrito no es lo que suena). Eso ya no
  necesita IA.
- La hoja para el atril lleva los acordes del cifrado por partes, y de dónde
  salen.

## 1.20.3 — 2026-10-07

Mantenimiento: las dependencias al día y las pruebas en verde otra vez. No
cambia nada de lo que ves.

### Por dentro

- **Tauri 2.12** y sus plugins (diálogos, notificaciones, bandeja, registro).
  La interfaz y la parte de Rust van en la misma versión, como pide Tauri, y
  Rust necesita como mínimo la 1.90.
- **Python**: el SDK de OpenAI 3.26, platformdirs, python-dotenv y
  charset-normalizer al día. FastAPI se queda en la 0.141: la 0.142 solo
  añade OpenTelemetry, que una app local no usa, y necesitó dos arreglos
  seguidos.
- **Interfaz**: vite, vitest, eslint, stylelint, vue-tsc y jsdom al día, y
  `source-map-js` 1.2.2 por un aviso de seguridad.
- **Pruebas y CI**: la CI fallaba desde la 1.20.1 por dos pruebas mal
  tipadas y por una regla nueva de Rust 1.99. Además, la prueba de las
  consolas de Windows daba por hecho que corría en Linux. Dependabot vuelve a
  proponer las actualizaciones de Rust, ya no propone TypeScript 7 (vue-tsc
  todavía no lo admite), y la auditoría de npm mira lo que va dentro de la
  app.

## 1.20.2 — 2026-10-03

En Windows ya no se abren ventanas negras al separar pistas, descargar o
analizar canciones.

### Windows

- **Sin ventanas negras.** Cada vez que DanPlay usaba ffmpeg, fpcalc o
  yt-dlp por debajo (separar pistas, sacar la onda, convertir, buscar el
  pulso, descargar), Windows abría una consola negra para ese proceso, y al
  separar una canción salían varias a la vez. Ahora todos los procesos que
  lanza el núcleo van sin ventana.
- En la versión hecha con el Python embebido, también había una consola
  negra detrás de la app mientras estaba abierta: ya no.

## 1.20.1 — 2026-10-03

Al elegir el modelo de IA solo salen los que tu clave puede usar de verdad.

### Asistente e IA

- **Solo los modelos que funcionan con tu clave.** Lo que lista el proveedor
  no es lo que tu clave puede usar: Google enseña unos sesenta modelos a una
  clave gratuita y muchos dan error (retirados para claves nuevas, sin cuota
  en el nivel gratuito, o que no son de chat). Ahora se prueba cada modelo
  con una petición de un token y solo salen los que contestan. Los ocultos
  se cuentan en la nota de la lista, y si tenías elegido uno de ellos, el
  selector avisa de por qué no sirve. Lo probado se recuerda un día, así que
  abrir el modal no gasta cuota; «Cargar la lista» vuelve a probarlos.
- Ya no salen modelos de voz, imagen, vídeo, embeddings o «live» de Google.
- Lo recomendado se elige entre los modelos que funcionan, no del catálogo.
- No se prueban modelo a modelo los servidores locales (Ollama: lo que lista
  es lo descargado) ni las plataformas con cientos de modelos (OpenRouter,
  DeepInfra).

## 1.20.0 — 2026-09-26

La primera vez que descargas algo, DanPlay te pregunta dónde guardarlo, y
a partir de ahí todo lo que bajes va a esa carpeta.

### Descargas

- **¿Dónde guardo lo que descargues?** Antes de la primera descarga, desde
  Descargas o desde el asistente, se pregunta la carpeta, y propone tu
  carpeta de música. Dentro van Artistas, Pistas, Secuencias, Tutoriales y
  Play Along y Entrada: se usan las que ya haya y se crean las que falten.
  Si no era de tus carpetas de música, se añade, para que lo que bajes salga
  en la app. No se vuelve a preguntar: la carpeta se ve (y se cambia) en
  Descargas y en Ajustes → Carpeta de descargas.
- Antes, sin haberla elegido, lo descargado iba a una carpeta supuesta (la
  de Música del sistema) que podía estar en otro disco y fuera de tu
  biblioteca: se bajaba y no aparecía en ningún sitio. Si la carpeta de
  descargas ya no está (un disco sin conectar), se vuelve a preguntar.

## 1.19.0 — 2026-09-26

Lo que bajas de YouTube y no es la canción sin más —una Drum Cam, un
tutorial, una secuencia— se ofrece llevarlo a su carpeta.

### Descargas

- **¿A su carpeta?** Si el título de YouTube dice que es una Drum Cam o una
  pista sin batería, un tutorial, un drum cover o un play along, o una
  secuencia (multitrack, playback, «PB»), al terminar Descargas pregunta
  «Parece una Drum Cam: ¿moverla a Pistas?». Con un clic va a Pistas, a
  Tutoriales y Play Along o a Secuencias, y sigue siendo la misma canción:
  sus estrellas, sus listas, su estudio y sus pistas separadas van con ella.
  Si no, se queda en Artistas, como hasta ahora. Solo pregunta cuando el
  título no deja duda: en una biblioteca real acertó el 97,9 % sin señalar
  ni una canción normal.

### Por dentro

- **Laya, medido y descartado.** Se probó Laya, un modelo que contesta
  preguntas cerradas en tu equipo, en cuatro de los juicios pequeños de la
  app: si el asistente dice haber hecho algo sin hacerlo, el tipo de vídeo,
  el «sí» a una oferta y las órdenes escondidas en letras y páginas. Sin
  afinar no mejora nada de lo que ya funciona, y pesaría 680 MB: no entra.
  El script que lo mide queda en `scripts/evaluate-laya.py`.

## 1.18.0 — 2026-09-26

El metrónomo del modo estudio, para tocar encima: acierta el «1» casi el
doble de veces, cuenta antes de que entre la canción, tiene cuatro sonidos
y va en la mezcla que te llevas.

### Metrónomo

- **El pulso y el «1» los saca Beat This!**, una red que los oye como un
  músico (de la Universidad de Linz, MIT), en tu equipo y sin internet: viaja
  con la app. Medido en GTZAN (diez estilos anotados a mano), el «1» acierta
  0,72 frente a 0,39 del análisis de antes, y en pop y rock 0,88 frente a
  0,56. Y sigue los compases irregulares (el 2/4 antes del coro). La primera
  vez que abres una canción tarda unos segundos; luego es al momento. Si
  algo falla, sigue el análisis de siempre.
- **La cuenta**: uno o dos compases de clic antes de que entre la canción al
  darle a play (en el modo estudio, con la canción en pausa), al tempo y en
  el compás de la canción. Entra justo en su pulso, aunque la pararas a
  mitad de compás. Pausa en plena cuenta la deja en pausa.
- **Cuatro sonidos** de clic: clásico, madera, baqueta (corto, se cuela
  entre los platos) y cencerro (se oye con la banda a tope). Se recuerdan,
  como la cuenta.
- **El clic en la mezcla que guardas**: «Guardarla como suena…» lleva el
  clic si está puesto, con su sonido y su volumen, a la velocidad y en el
  compás que se oyen. Para ensayar con el móvil como en el estudio.

### Por dentro

- `scripts/evaluate-pulse.py` mide cada detector de pulso sobre GTZAN, y
  `scripts/export-pulse.py` regenera el de la app (comprueba que da los
  mismos pulsos que el original).

## 1.17.0 — 2026-09-26

Separar en pistas con la mejor calidad que se ha medido, y con un solo
botón: solo salen los instrumentos que tiene la canción, y juntos suenan
exactamente como ella.

### Pistas separadas

- **Un solo botón, «Separar pistas»**, en el modo estudio y en los menús:
  ya no se elige entre seis o cuatro pistas ni en Ajustes. Sale siempre la
  mejor combinación de separadores, medida (ver abajo).
- **En dos pasadas.** En unos minutos ya tienes las pistas y puedes usarlas
  (unos tres para una canción de cuatro y media); luego la batería y
  el bajo se vuelven a sacar, cada uno con un separador especializado en
  él, y se cambian solos sin cortar lo que suena. El bajo sale más de un dB
  más limpio, y la batería un tercio de dB.
- **Solo lo que está**: una canción sin piano (o sin voz, como una pista de
  acompañamiento) no trae esa pista; lo poco que la red le atribuía va a
  «Otros».
- **«Otros» es lo que queda de la canción**, así que todas juntas suenan
  exactamente como ella: callar la batería deja lo demás entero (más de un
  dB mejor que antes, medido).
- **Un repertorio entero**, desde su menú: «Separar las pistas del
  repertorio» (sin repetir las que ya tienen las mejores). En la cola van
  primero las pasadas rápidas de todas y luego las mejoras, para poder
  ensayarlo cuanto antes.
- **Las separadas con 1.16.0** siguen sonando igual; su menú ofrece
  «Separar otra vez con la mejor calidad». Las que se quedaron sin mejorar
  (la app se cerró a mitad) solo se mejoran.
- **En Opus**, si lo eliges en Ajustes: cuatro veces menos que en FLAC.
- La primera vez se baja más: 223 MB (la red rápida y los dos
  especialistas, de su autor y comprobados). Ajustes lo enseña como un solo
  separador, que se puede borrar.

### Por dentro

- `scripts/evaluate-separator.py` mide cada combinación de separadores con
  las muestras de MUSDB18 (lo que se parece cada pista a la de verdad, y lo
  que tarda); de ahí salen las decisiones de arriba. Cuantizar las redes a 8
  bits se midió y no compensa.
- El reproductor reabre las pistas cuando se rehacen con los mismos nombres
  (al mejorarlas o al separar otra vez), donde iban.

## 1.16.0 — 2026-09-26

Separar una canción en pistas —batería, voces, bajo, guitarra, piano y el
resto— y oírlas en el modo estudio, cada una con su volumen: callar la
batería para tocarla tú, dejar solo el bajo, subir la voz.

### Pistas separadas

- **Separar en pistas**, desde el modo estudio o desde el menú de una
  canción (o de varias elegidas, que van a una cola). Lo hace Demucs, el
  separador de Meta, en tu equipo: sin subir nada ni límites. Seis pistas
  (batería, voces, bajo, guitarra, piano y el resto) o cuatro, algo más
  limpias, sin guitarra ni piano. La primera vez se baja el separador (55 u
  84 MB, del repositorio de su autor, comprobado); luego no hace falta
  internet. Tarda en torno a lo que dura la canción, en segundo plano y con
  poca prioridad, así que se sigue usando la app (y la música no se corta).
- **El mezclador**: con las pistas sonando, la onda del estudio es un carril
  por instrumento, cada uno con su onda. **M** calla una pista, **S** la deja
  sola (o varias a la vez), y cada una tiene su volumen (hasta el doble) y
  su panorama. Los cambios se oyen al momento y sin clics. El bucle, la
  velocidad, el tono y el metrónomo van igual que con la canción, y las
  pistas nunca se desfasan entre sí. Lo que se deja puesto se guarda con la
  canción y vuelve al abrirla.
- **Guardar la mezcla** en mp3, flac o wav: la canción sin batería (o solo
  con la voz y el bajo) para practicar fuera, tal cual o como suena, a otra
  velocidad y tono. Si la guardas dentro de tu biblioteca, entra como una
  canción más.
- **Dónde quedan**: en `Separadas/<canción>/` dentro de tu biblioteca, en
  FLAC (`Bateria.flac`, `Voces.flac`…), archivos que abre cualquier
  programa. No aparecen como canciones sueltas, un escaneo las vuelve a unir
  con su canción si el índice se pierde, y se van a la papelera con ella.
- **Ajustes** dice qué separadores hay bajados y deja borrarlos.

## 1.15.0 — 2026-09-25

El metrónomo, a tiempo en las canciones con síncopa; en el modo estudio,
varios tramos seguidos y opciones para cada uno; y copiar el nombre de una
canción desde cualquier lista.

### Modo estudio

- **Varios tramos que se repiten seguidos**: con «varios» (junto al candado
  de la onda) cada arrastre añade un tramo, y al acabar uno la canción salta
  al siguiente, y del último al primero, saltándose lo de en medio. Un clic
  sobre un tramo lo quita. Se guardan con la canción y, como marcador, van
  juntos: un solo marcador que los vuelve a poner todos.
- **Opciones del tramo**, en su botón ⋯ o con el clic derecho en la onda:
  reproducir ahora, **repetir cuando acabe la canción** (sigue hasta el
  final y entonces vuelve al tramo), ir aquí, ajustar los bordes a los
  pulsos del metrónomo, guardar como marcador, añadir más tramos y quitar
  ese tramo o todos. «Ahora» y «Al acabar» también están junto al tramo.
- **Un clic en la onda quita la selección**; sin candado, o en pausa, además
  lleva la canción a ese punto.

### Listas

- **Copiar el nombre** en las cuatro vistas (tabla, lista fina, fichas y
  cuadrícula): el icono junto al título copia el título, y «nombre», al
  final, el nombre completo como el del archivo pero sin el `.mp3` («Barak -
  Mi Gozo»), para buscarla fuera. Un aviso dice qué se copió.

### Corregido

- **El metrónomo a contratiempo** en canciones con la síncopa 3+3+2 tan
  marcada de mucha alabanza en directo: el análisis tomaba por pulso el golpe
  cada tres corcheas, y en una canción a 138 el clic iba a 92, la mitad del
  tiempo fuera. Ahora reconoce ese patrón y se queda con el pulso de verdad.
  Es un control estrecho: el resto de canciones da el mismo tempo que antes.
- **La tabla agrupada** («Artistas», o agrupar por álbum o por tono)
  repartía el ancho a partes iguales entre todas las columnas, y el título
  quedaba tan estrecho como el número. Ahora cada columna tiene su ancho,
  como en la tabla sin agrupar.

## 1.14.0 — 2026-09-25

El modo estudio, más cómodo para tocar encima de la canción; la letra con
tiempos, que se quedaba pegada, arreglada; y el `.deb` de Linux, igual que el
`.AppImage`.

### Modo estudio

- **La onda con candado**, puesto de entrada: mientras suena, un clic en la
  onda no mueve la canción y elegir un tramo (o la banderita de un marcador)
  no salta a él. El tramo entra cuando la canción llega; si va por detrás,
  la canción sigue y al acabarse vuelve a él en vez de pasar a la siguiente.
  En pausa, la onda coloca como siempre; las teclas A y B y la lista de
  marcadores siguen moviendo la canción.
- **El tono en tonos**, como lo cuenta un músico (medio tono es un
  semitono): de cuarto en cuarto de tono, de medio en medio o de tono en
  tono. El cuarto de tono sirve para ponerse a la par de una grabación que
  no está afinada a 440 («G +¼»).
- **Metrónomo**: el tempo se escribe a mano con decimales (90.7) y las
  flechas lo mueven de décima en décima; con la canción sonando, el clic a
  mano entra en su pulso. Compases 2/4, 3/4, 4/4, 6/8 y **sin acento**
  (todos los clics iguales). El clic sube hasta el doble y la canción hasta
  un 150 %, sin saturar: todo pasa por un limitador que a volumen normal no
  toca nada.

### Corregido

- **Letra con tiempos**: al seguir la canción, la ficha entera iba subiendo
  línea a línea mientras la letra se quedaba parada en medio (y con las dos
  cosas moviéndose se llegaba a ver letra sobre letra). Ahora solo se mueve
  la caja de la letra, y si se mueve a mano se la deja estar un momento.
- **×2 y ÷2 del metrónomo**: con un tempo puesto a mano no hacían nada, no
  se veían puestos (tampoco la velocidad elegida) y la onda seguía pintando
  la rejilla de antes. Ahora doblan también el tempo a mano, se ven
  encendidos y la onda enseña los pulsos tal como suenan.

### Escritorio en Linux

- El `.deb` se ve como el `.AppImage`: en una sesión Wayland, si hay
  XWayland, la barra de título la pone el escritorio y no GTK (la de GNOME
  desentonaba en KDE), y la ventanita puede colocarse junto al icono de la
  bandeja.
- Las ventanas recuerdan su tamaño y su sitio, pero ya no si estaban
  abiertas: la proyección que se quedó abierta no vuelve a salir al arrancar,
  y la ventana principal cerrada a la bandeja no arranca escondida.
- El DanPlay instalado con el `.deb` quita al arrancar el lanzador suelto (de
  un `.AppImage` o del binario) que lo tapaba en el menú y al abrir
  canciones con doble clic, y esas canciones pasan a abrirse con él. La
  compilación de desarrollo lleva su propio lanzador, «DanPlay (desarrollo)».

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
  entera en memoria (320 MB por hora; ahora 9); y en Windows el núcleo
  empaquetado se caía nada más arrancar (sin consola, uvicorn no podía
  configurar sus avisos): el instalador no llegaba a funcionar.
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
- 1.429 pruebas (eran 783).
