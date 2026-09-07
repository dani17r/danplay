<h1 align="center">DanPlay</h1>

<p align="center">
  <strong>Tu música vuelve a ser tuya.</strong><br>
  Un gestor de biblioteca musical de escritorio que identifica, ordena, etiqueta
  y reproduce lo que tienes en el disco duro. Sin nube, sin cuenta, sin suscripción.
</p>

<p align="center">
  <img alt="Linux" src="https://img.shields.io/badge/Linux-.deb%20%C2%B7%20AppImage-333?logo=linux&logoColor=white">
  <img alt="Vue 3" src="https://img.shields.io/badge/Vue-3-42b883?logo=vue.js&logoColor=white">
  <img alt="Rust" src="https://img.shields.io/badge/Rust-Tauri%202-b7410e?logo=rust&logoColor=white">
  <img alt="Python" src="https://img.shields.io/badge/Python-3.13-3776ab?logo=python&logoColor=white">
  <img alt="Pruebas" src="https://img.shields.io/badge/pruebas-363%20en%20verde-2ea043">
</p>

<!--
  CAPTURAS: pon aquí dos o tres imágenes de la app (docs/img/…) cuando las tengas.
  <p align="center"><img src="docs/img/biblioteca.png" width="820" alt="La biblioteca"></p>
-->

---

## El problema

Tienes cientos de canciones en una carpeta. Se llaman así:

```text
y2mate.com - BARAK Mi Gozo VIDEO OFICIAL LETRA 320kbps.mp3
01 - pista_2_FINAL (1).mp3
VERSIONButterflyFull.mp3
```

Ningún reproductor sabe qué son. No tienen etiquetas, o las tienen mal. Hay
copias repetidas que no sabes cuál borrar. Y si mañana cambias de programa,
todo el trabajo de ordenarlas se queda dentro de ese programa.

## Qué hace DanPlay

Coge ese montón y lo convierte en una biblioteca:

```text
Artistas/
  Barak/
    Barak - Mi Gozo.mp3
    Barak - Sera Llena La Tierra (feat. Miel San Marcos).mp3
```

Reconoce cada canción **por cómo suena**, no por el nombre del archivo. Limpia
el título, encuentra la letra y la carátula, y lo archiva por artista. Lo que
no logra identificar va a `Revisar/`: **nunca se inventa un artista**.

Y lo más importante: **todo se guarda dentro del propio mp3**. Las estrellas,
el favorito, la letra, la carátula, el tono, el BPM. La base de datos es solo
un índice desechable — si la borras, un escaneo la reconstruye entera desde
tus archivos. Si mañana te vas a otro reproductor, tu trabajo se va contigo.

## Para quién es

| Si eres… | Esto es lo que te da |
| --- | --- |
| **Músico o equipo de alabanza** | Tonos, acordes y transposición con cejilla sugerida. Repertorios exportables a `.m3u`. Letra dentro del archivo. |
| **Quien tiene la música en el disco** | Cientos de descargas con nombres imposibles, duplicados y etiquetas rotas. Esto lo ordena. |
| **Quien no quiere depender de nadie** | Sin cuenta, sin nube, sin telemetría. Funciona con el wifi apagado (salvo lo que por definición necesita internet). |
| **Quien viene de otro reproductor** | Se lee y se escribe ID3 estándar. Kodi, foobar2000 o Rhythmbox verán tus estrellas y tus letras igual. |

## Cómo identifica una canción

En cascada, de más fiable a menos. Se para en el primer escalón que dé una
respuesta con confianza suficiente:

```text
  1  Etiquetas ID3        ¿ya viene bien etiquetada?          confianza 0.95
  2  Huella acústica      Chromaprint → AcoustID → MusicBrainz
                          la reconoce por CÓMO SUENA
  3  Heurístico           ¿coincide con un artista que ya
                          tienes en Artistas/?                 umbral 0.80
  4  IA                   para los casos ambiguos              umbral 0.55
  5  Revisar/             nada de inventar un artista
```

El paso 2 es el que sorprende: DanPlay puede identificar una canción cuyo
archivo se llame `audio_final_2.mp3`, porque calcula su huella acústica y la
busca en la base de datos abierta de MusicBrainz.

## Lo que lo hace distinto

**No abre ningún puerto.** La interfaz habla con el núcleo por un socket Unix
con permisos `0600`. Ningún otro proceso de tu equipo —ni una web abierta en
el navegador— puede hablar con la API. Comprobado en las pruebas de humo.

**El audio no pasa por el navegador.** Lo decodifica Rust y sale directo a la
tarjeta de sonido. El WebView no lo toca en ningún momento.

**Listas de cualquier tamaño.** La lista pinta solo las filas que se ven: da
igual que tengas 500 canciones o 20.000, siempre son unos 900 nodos en pantalla
y el mismo tiempo de dibujado.

**Un asistente que solo habla de música.** 23 herramientas: consultar la
biblioteca, armar repertorios, buscar letras, transponer acordes, descargar de
YouTube, comprobar datos en la web. Si le preguntas de política te dice que eso
no es lo suyo. Solo descarga si se lo pides.

## Instalación

Se construye desde el código. Son cuatro comandos y un script:

```bash
git clone https://github.com/dani17r/danplay.git && cd danplay

python3 -m venv .venv
.venv/bin/python -m pip install -r requirements.txt
.venv/bin/maturin develop --release -m core/Cargo.toml   # el crate de Rust
cd desktop && npm install && cd ..

cp .env.example .env        # y rellena las claves que quieras usar
./scripts/build.sh          # interfaz + núcleo + app
./danplay-app.sh
```

Para la conversión de formatos y la huella acústica hacen falta dos programas
del sistema. Si te faltan, la app lo dice y sigue funcionando sin esas dos
funciones:

```bash
sudo apt install ffmpeg libchromaprint-tools
```

`scripts/build.sh --package` genera además un `.deb` y un `.AppImage` para tu
propio uso. **No los publiques**: llevan `mutagen` dentro, que es GPL, y eso
choca con la licencia de este proyecto —
[por qué](docs/DEPENDENCIAS.md#️-aviso-importante-sobre-mutagen).

> **Nota sobre las claves.** DanPlay funciona sin ninguna clave: escanea,
> organiza, reproduce y busca. Las claves solo activan extras:
> `DEEPINFRA_API_KEY` para la IA y el asistente, `ACOUSTID_API_KEY` (gratis)
> para la huella acústica. Se guardan en `~/.config/danplay/danplay.env` con
> permisos `0600` y nunca salen de tu equipo salvo hacia el servicio al que
> pertenecen.

## Primeros pasos

1. Abre DanPlay y elige tu carpeta de música. Te avisa si la carpeta que
   añades se solapa con otra que ya tenías.
2. Deja que escanee. Lee las etiquetas de cada archivo y construye el índice.
3. Suelta música nueva en `~/Musica/Entrada` y pulsa **Importar**: se
   identifica, se limpia el nombre y se archiva por artista.

Los reescaneos posteriores no vuelven a abrir los archivos que no han
cambiado, así que son casi instantáneos.

## La búsqueda

Texto normal, o filtros escritos en la propia caja. Acepta español e inglés
indistintamente:

```text
barak gozo                    texto libre (busca en artista, título, álbum, archivo…)
artista:barak tono:Bb         filtros por campo
bpm>100 duracion<300          comparaciones numéricas
genero:alabanza anio:2019     todo combinable
```

`artista:` y `artist:`, `tono:` y `key:`, `duracion:` y `duration:` valen igual.

## Desde la terminal

Todo lo que hace la app se puede hacer sin ella:

```bash
danplay status                       configuración y resumen
danplay scan                         (re)indexar
danplay search artista:barak vivo    búsqueda avanzada
danplay import --dry-run             ver qué haría con Entrada/ sin tocar nada
danplay youtube <url|texto>          descargar de YouTube y archivar
danplay duplicates                   informe (nunca borra solo)
danplay playlist                     repertorios
danplay organize rolas               renombrar una carpeta existente
danplay watch                        procesa Entrada/ según caen archivos
```

## Cómo está hecho

Cuatro capas, cada una en lo que mejor se le da:

```text
   Vue 3 + Vite            interfaz
        │  invoke()        único puente; el JS no hace HTTP
   Tauri / Rust            proceso principal, reproducción de audio
        │  socket Unix     0600; sin puerto TCP
   Python                  identificación, índice, IA, etiquetas
        │
   Rust (PyO3)             hashes en paralelo y análisis de audio
```

- **Vue 3** para la interfaz: temas claros y oscuros, densidad ajustable,
  responsive hasta tamaño móvil, mini reproductor en la bandeja del sistema.
- **Rust (Tauri)** para el proceso principal: sirve el audio leyendo del disco
  con soporte de rangos, y lo reproduce nativamente.
- **Python** para el núcleo: la cascada de identificación, el índice SQLite con
  búsqueda de texto completo (FTS5), la IA y las etiquetas.
- **Rust (PyO3)** para lo que Python hace lento: hashes en paralelo con rayon y
  análisis de audio (BPM y tono) con FFT.

El código está en inglés; los comentarios y todo lo que ve el usuario, en
castellano.

## Formatos

Lee `.mp3`, `.flac`, `.m4a`, `.wav`, `.ogg`, `.opus`, `.aac` y `.wma`. Puede
unificar a mp3 con ffmpeg conservando etiquetas y carátula, respetando las
carpetas que marques como intocables (`Secuencias`, `Pistas`, `Multitracks`…),
donde comprimir sería perder calidad.

## Lo que NO hace bien

Prefiero decirlo aquí que en un issue:

- **La detección de tono por DSP no es fiable.** Medido contra valores
  conocidos: 2 aciertos de 8. Se probaron 60 combinaciones de perfiles y
  ponderaciones y todas se estancan ahí. Por eso el tono que muestra la app
  viene de la IA (que sí conoce las canciones) y el del DSP queda como pista
  secundaria, con su confianza a la vista. Se puede corregir a mano y queda
  guardado en el `TKEY` del archivo.
- **El BPM es usable, no exacto.** 5 de 8 aceptando errores de octava.
- **Los acordes que da la IA son aproximados.** La app lo avisa. La
  transposición sobre ellos sí es determinista y exacta.
- **Solo Linux, de momento.** El empaquetado, la papelera del sistema y los
  diálogos usan herramientas de escritorio Linux. El núcleo es portable; el
  envoltorio no lo he probado en otros sistemas.
- **Un solo usuario, una sola máquina.** No hay sincronización entre equipos.

## Documentación

| Documento | Qué encontrarás |
| --- | --- |
| [Arquitectura](docs/ARQUITECTURA.md) | Cómo encaja todo por dentro, decisiones de diseño y por qué. |
| [Contribuir](docs/CONTRIBUIR.md) | Cómo montar el entorno, ejecutar las pruebas y en qué se puede ayudar. |

## Pruebas

```bash
./scripts/test.sh
```

363 pruebas repartidas así:

| Tanda | Pruebas |
| --- | --- |
| Núcleo Python (nombres, etiquetas, duplicados, teoría, índice) | 87 |
| API sobre una biblioteca temporal de verdad | 66 |
| Interfaz: componentes, reactividad, temas, listas grandes | 169 |
| Interfaz: rutas de medios en cada sistema | 7 |
| Rust: análisis de audio y reproductor nativo | 22 |
| Humo sobre la app **ya compilada** | 12 |

Las de humo arrancan la app de verdad y comprueban, entre otras cosas, que el
socket es privado (`0600`), que no queda ningún puerto TCP abierto y que el
núcleo muere con la app sin dejar procesos huérfanos.

## Licencia

**[PolyForm Strict 1.0.0](LICENSE)** — código a la vista, no código abierto.

| | |
| --- | --- |
| ✅ | Usar la app para lo tuyo: tu música, tus proyectos, tu iglesia, tu ONG, tu colegio |
| ✅ | Leer el código |
| ❌ | Usarla con fines comerciales |
| ❌ | Redistribuirla, modificarla o hacer obras derivadas |

Los derechos son del autor. Para cualquier otro uso, incluido el comercial,
escribe y lo hablamos.

[Resumen en castellano](docs/LICENCIA-RESUMEN.md) · [Licencias de las
dependencias](docs/DEPENDENCIAS.md)

---

<p align="center">
  Hecho para ordenar una biblioteca de verdad, no para una demo.
</p>
