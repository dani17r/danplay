# Cambios

Lo que cambia en cada versión que se entrega. El detalle de por qué está
hecho así, en [docs/ARQUITECTURA.md](docs/ARQUITECTURA.md).

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
