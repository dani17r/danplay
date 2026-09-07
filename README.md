# DanPlay

Gestor de biblioteca musical de escritorio: identifica, renombra, organiza,
etiqueta, reproduce y enriquece con IA.

## Arquitectura

    Vue 3 + Vite  (interfaz)
         |  invoke()          <- unico puente, sin HTTP en el JS
    Tauri / Rust  (proceso principal)
         |  socket Unix 0600  <- sin puerto TCP; solo tu usuario
    Python        (nucleo: identificacion, indice, IA, etiquetas)
         |
    Rust (PyO3)   (hashes en paralelo, analisis de audio)

La API nunca escucha en un puerto: solo en `$XDG_RUNTIME_DIR/danplay.sock`
con permisos 0600. Ningun otro proceso del equipo puede hablar con ella.
El audio lo sirve Rust leyendo del disco (con soporte de Range para poder
buscar dentro de la cancion), sin pasar por Python.

## Idioma

El **codigo** esta en ingles: carpetas, archivos, funciones, variables, rutas
de la API y columnas de la base. Los **comentarios** y todo lo que ve el
usuario (interfaz, mensajes, el asistente) estan en castellano.

La busqueda acepta los dos: `artista:barak` y `artist:barak` valen igual, y
tambien `tono:`/`key:`, `duracion:`/`duration:`, `anio:`/`year:`.

## Estructura

    danplay/        nucleo Python (indice, IA, etiquetas, descargas)
    core/           crate Rust (PyO3): hashes en paralelo y analisis de audio
    desktop/        interfaz Vue 3 + envoltorio Tauri
      src/          componentes, composables, estilos
      src-tauri/    proceso principal en Rust
      tests/        pruebas de la interfaz
    tests/          pruebas de Python, del frontend y de humo
    packaging/      receta de PyInstaller para el nucleo empaquetado
    scripts/        build.sh y test.sh
    dist/           todo lo que se genera (core/ e installers/)

## Arrancar

    ./danplay-app.sh          # la app de escritorio
    ./danplay.sh <comando>    # la linea de comandos

Tambien queda un acceso directo en el menu de aplicaciones.

## Construir y probar

    ./scripts/build.sh              interfaz + nucleo + app
    ./scripts/build.sh --package    ademas el .deb y el .AppImage
    ./scripts/test.sh               todas las pruebas

Son cuatro capas y el orden importa: si compilas solo una, la app arranca con
la version vieja y no se nota. Por eso hay un script y no cuatro comandos.
Los directorios de trabajo intermedios se borran solos; no queda un `build/`
suelto en la raiz.

## Comandos

    danplay status                       configuracion y resumen
    danplay folder add /ruta             gestionar carpetas indexadas
    danplay exclude add Secuencias       carpetas que se omiten al indexar
    danplay scan                         (re)indexar
    danplay search artista:barak vivo    busqueda avanzada
    danplay import --dry-run             procesar Entrada/
    danplay convert --apply              unificar formatos a mp3
    danplay youtube <url|texto>          descargar de YouTube y archivar
    danplay playlist                     repertorios
    danplay duplicates                   informe (nunca borra solo)
    danplay organize rolas               renombrar una carpeta
    danplay watch                        procesa Entrada/ segun caen archivos
    danplay serve --uds /ruta.sock       levantar la API

## Descargas de YouTube

    danplay youtube https://youtu.be/XXXX          un video
    danplay youtube https://youtube.com/playlist?… una lista entera
    danplay youtube barak sera llena la tierra     busca y baja el primero
    danplay youtube -l barak                       solo enseña que bajaria
    danplay youtube --only-download <url>          lo deja en Entrada/

Baja el mejor audio, lo pasa a mp3 (320 kbps por defecto) con la miniatura
como caratula, y lo suelta en la misma tuberia que Entrada/: se identifica,
se limpia el nombre y se archiva en `Artistas/<Artista>/`.

**No se usa el postprocesador de metadatos de yt-dlp a proposito.** Ese rellena
el artista con el nombre del canal ("Fulanito Music", "… - Topic"), y como las
etiquetas son el primer escalon de la cascada de identificacion se colaria con
0.95 de confianza y archivaria la cancion bajo un artista inventado. Solo se
escriben etiquetas cuando YouTube da datos de musica de verdad (los campos
`track` y `artist`, de YouTube Music). Si no, se deja limpio y deciden la
huella acustica, el heuristico o la IA.

En la app hay una pagina **Descargas** con el mismo trabajo, barra de progreso
y boton de cancelar.

## El asistente

Habla **solo de musica**: canciones, artistas, discos, generos, epocas,
instrumentos, teoria, produccion e historia. Si le preguntas otra cosa lo dice
y no entra al trapo.

Tiene 13 herramientas: consultar la biblioteca, crear listas y añadir temas,
letras (de la biblioteca o de fuera), tonalidades y acordes, transponer,
buscar en YouTube, **descargar**, ver como va la descarga, y buscar en la web
para comprobar datos en vez de suponerlos.

Solo descarga si se lo pides. Si la peticion es ambigua o la lista es larga,
primero enseña que ha encontrado y espera el visto bueno.

## Como identifica una cancion

En cascada, de mas fiable a menos:

1. **Etiquetas ID3** ya presentes
2. **Huella acustica** (Chromaprint -> AcoustID -> MusicBrainz): la reconoce
   por como suena, no por el nombre
3. **Heuristico** contra los artistas que ya existen en `Artistas/`
4. **IA** (DeepInfra) para los casos ambiguos
5. Si nada funciona -> `Revisar/`, nunca se inventa un artista

## Reglas de nombres

- Sin acentos. Unica excepcion: la **ñ** se conserva.
- Nada en MAYUSCULA SOSTENIDA.
- Formato `Artista - Titulo (feat. X).mp3`
- Duplicados: sufijo ` - r`, ` - r2` para compararlos y borrar a mano.

## Que se guarda dentro del archivo

Estrellas (`POPM`), favorito y listas (`TXXX`), letra (`USLT`), portada
(`APIC`), tono (`TKEY`), BPM (`TBPM`). Si pierdes la base de datos no
pierdes nada, y otros reproductores lo leen igual.

## Precision del analisis de audio

Medido contra valores conocidos (8 canciones):

- **BPM**: 5/8 aceptando errores de octava. Usable.
- **Tono**: 2/8. **No es fiable.** Se probaron 60 combinaciones de perfiles
  y ponderaciones y todas se estancan ahi. El tono que muestra la app viene
  de la IA, que si conoce las canciones; el del DSP queda como pista
  secundaria con su confianza a la vista. Se puede corregir a mano y queda
  guardado en el `TKEY`.

## Instalacion desde cero

    python3 -m venv .venv
    .venv/bin/python -m pip install -r requirements.txt
    cp .env.example .env                       # y rellenar las claves
    .venv/bin/maturin develop --release -m core/Cargo.toml
    cd desktop && npm install && cd ..
    ./scripts/build.sh

Para las descargas hace falta `yt-dlp` (ya viene en requirements.txt) y
`ffmpeg`. El nucleo empaquetado del .deb/.AppImage los lleva dentro.

Opcional, para la huella acustica:

    sudo apt install libchromaprint-tools      # ya instalado
    # y una clave gratis en https://acoustid.org/new-application
