<h1 align="center">DanPlay</h1>

<p align="center">
  <strong>Tu música vuelve a ser tuya.</strong><br>
  Un gestor de biblioteca musical de escritorio que identifica, ordena, etiqueta
  y reproduce lo que tienes en el disco duro. Sin nube, sin cuenta, sin suscripción.
</p>

<p align="center">
  Creado y desarrollado por <strong>Daniel Rangel</strong> ·
  <a href="https://t.me/Dani17r">Telegram @Dani17r</a>
</p>

<p align="center">
  <img alt="Linux" src="https://img.shields.io/badge/Linux-.deb%20%C2%B7%20AppImage-333?logo=linux&logoColor=white">
  <img alt="Windows" src="https://img.shields.io/badge/Windows-portable%20%2B%20instalador-0078d4?logo=windows&logoColor=white">
  <img alt="Vue 3" src="https://img.shields.io/badge/Vue-3-42b883?logo=vue.js&logoColor=white">
  <img alt="Rust" src="https://img.shields.io/badge/Rust-Tauri%202-b7410e?logo=rust&logoColor=white">
  <img alt="Python" src="https://img.shields.io/badge/Python-3.13-3776ab?logo=python&logoColor=white">
  <img alt="Version" src="https://img.shields.io/badge/version-1.30.0-4ade80">
  <img alt="Pruebas" src="https://img.shields.io/badge/pruebas-1.602%20en%20verde-2ea043">
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
| **Músico o equipo de alabanza** | Acordes y tono de cifrados publicados (Ultimate Guitar, LaCuerda), con su enlace, y transposición con cejilla sugerida. Repertorios en el orden que tú decidas (se arrastran las canciones arriba y abajo), exportables a `.m3u` y como **hoja para el atril** (tono, bpm, cejilla, acordes, letra). Letra dentro del archivo, y con tiempos: la ficha sigue la línea que suena, y la **proyección** la pone en grande para la pantalla. **Modo estudio** a media pantalla (o, con «Grande», a todo el alto, y se recuerda): el tramo que se repite se elige arrastrando sobre la forma de onda (con regla de minutos y un candado para que, mientras suena, un clic no la mueva), o varios tramos que se repiten seguidos saltándose lo de en medio; cada tramo tiene sus opciones (sonar ya, repetir cuando acabe la canción, ajustarlo a los pulsos) y se guarda como marcador con nombre y notas; **velocidad** sin cambiar el tono, **tono** en tonos (de cuarto, de medio o de uno entero), y un **metrónomo** que detecta solo el pulso y el «1» de cada compás (con Beat This!, una red que los oye como un músico, compases irregulares incluidos), entra en el «1» y se para o arranca aparte de ella. Con **«Clic automático»** cada canción trae el suyo, también al pasar de una a otra; mientras carga no suena y la onda se va encendiendo con lo que lleva, y sin él el tempo que pongas sigue igual en todas: se dobla para ir a corcheas, admite el tempo a mano con decimales y otros compases (o ninguno acentuado), tiene cuatro sonidos (clásico, madera, baqueta, cencerro), **cuenta uno o dos compases** antes de que entre la canción, y el clic y la canción suben por encima de como vienen sin saturar. **Pistas separadas**: la canción se separa en batería, voces, bajo, guitarra, piano y el resto (en tu equipo, sin subir nada), y en el modo estudio suenan ellas, con un carril por instrumento: callar la batería para tocarla tú, dejar solo el bajo, subir la voz, llevar cada una a un lado; con el bucle, la velocidad, el tono y el metrónomo encima, y al pasar de la canción a sus pistas (o al revés) no se oye ningún corte. La mezcla se guarda en mp3 («sin batería», para el móvil), y si quieres con el clic encima. Todo guardado con la canción y borrado con ella. Lo que bajas de YouTube y es una Drum Cam, un tutorial o una secuencia se ofrece llevarlo a su carpeta. Entre canción y canción, un **fundido** (la que se va baja mientras sube la siguiente). Y **«Transformar»** pone cada canción de tu carpeta en su sitio y con su nombre, una a una y solo si dices que sí, sin que deje de sonar la que suena. |
| **Quien tiene la música en el disco** | Cientos de descargas con nombres imposibles, duplicados y etiquetas rotas. Esto lo ordena. Y en cualquier lista, el título o el nombre completo de una canción se copian de un clic, para buscarla fuera. |
| **Quien no quiere depender de nadie** | Sin cuenta, sin nube, sin telemetría. Funciona con el wifi apagado (salvo lo que por definición necesita internet). |
| **Quien viene de otro reproductor** | Se lee y se escribe ID3 estándar. Kodi, foobar2000 o Rhythmbox verán tus estrellas y tus letras igual. |
| **Quien comparte música** | «Enviar por Telegram» desde el menú de una canción, de varias seleccionadas (con el botón «Seleccionar» y sus casillas, o con Ctrl/Mayús) o de un repertorio entero, si Telegram Desktop está instalado (Linux, Windows y macOS): se abre con los archivos listos y eliges el chat allí. |

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
con permisos `0600`, dentro de una carpeta que solo tú puedes abrir. Ningún
otro proceso de tu equipo —ni una web abierta en el navegador— puede hablar
con la API. Comprobado en las pruebas de humo.

**Vive en la bandeja del sistema.** Al cerrar la ventana, DanPlay se esconde y
la música sigue. Un clic en el icono saca un mini reproductor; el botón
derecho, el menú; el central pausa; la rueda sube y baja el volumen. Se cierra
del todo con **Salir**. Si tu escritorio no tiene bandeja, cerrar cierra, para
que nunca quede una aplicación viva sin forma de volver a ella.

**Las teclas multimedia funcionan.** DanPlay se anuncia al escritorio (MPRIS en
Linux), así que las teclas de reproducción del teclado y de los auriculares
valen, y el reproductor del sistema y la pantalla de bloqueo enseñan lo que
suena, con su carátula.

**El audio no pasa por el navegador.** Lo decodifica Rust y sale directo a la
tarjeta de sonido. El WebView no lo toca en ningún momento.

**Tus listas no se rompen.** En cualquier reproductor, mover una canción de
carpeta, renombrarla o volver a bajarla rompe las listas donde estaba. DanPlay
guarda lo tuyo (las listas en su orden, las estrellas, el estudio, los
cifrados) también en un diario junto a tu música (`.danplay/journal.json`): si
mueves la música a otro disco o la base se pierde, al abrir todo vuelve a su
sitio, y las listas exportadas a `.m3u8` se reescriben con las rutas nuevas.
Una canción que desaparece se queda en su lista como «No está», para buscarla
o bajarla otra vez, y vuelve sola a su sitio si aparece.

**Listas de cualquier tamaño.** La lista pinta solo las filas que se ven: da
igual que tengas 500 canciones o 20.000, siempre son unos 900 nodos en pantalla
y el mismo tiempo de dibujado.

**Un asistente que solo habla de música.** 27 herramientas: consultar la
biblioteca, armar repertorios, buscar letras, transponer acordes, descargar de
YouTube, comprobar datos en la web, sacar los tonos vecinos para un set sin
saltos, escribir la hoja para el atril. **Pégale la lista del domingo tal como
llega por WhatsApp** y la mete en tu repertorio con lo que ya tienes, en su
orden, mientras te pide permiso en una tarjeta para bajar lo que falta (que al
terminar entra solo en su sitio); «verifica» compara sin tocar nada. Mira
siempre primero tu biblioteca. Si le preguntas de política te dice que eso no
es lo suyo. Solo descarga si se lo pides. Ve lo que tú ves («pon la segunda»,
«añade las seleccionadas»), responde en vivo y se puede parar, guarda las
conversaciones y se buscan, enseña lo que cuesta cada respuesta, y si su
proveedor falla responde otro de los que tengas configurados y te lo dice.
Corre con la IA que tú elijas: OpenAI, Claude, Gemini, DeepInfra,
OpenRouter… o un modelo en tu propio equipo.

## Instalación

Se construye desde el código. Hacen falta [uv](https://docs.astral.sh/uv/)
(Python), Node 24 y Rust:

```bash
git clone https://github.com/dani17r/danplay.git && cd danplay

uv sync                     # el .venv, con el crate de Rust ya compilado
cd desktop && npm ci && cd ..

cp .env.example .env        # y rellena las claves que quieras usar
./scripts/build.sh          # interfaz + núcleo + app
./danplay-app.sh
```

Para probarla sin compilar la versión final, `./scripts/dev.sh` la abre en
modo desarrollo (y `--prueba CARPETA`, con una copia de esas canciones y sin
tocar tus datos): ver [Contribuir](docs/CONTRIBUTING.md).

Compilada desde el código, para la conversión de formatos y la huella
acústica hacen falta dos programas del sistema (los paquetes `.deb`,
`.AppImage`, de Windows y de macOS ya los traen dentro, con QuickJS para
descargar de YouTube). Si te faltan, la app lo dice y sigue funcionando sin
esas dos funciones:

```bash
sudo apt install ffmpeg libchromaprint-tools
```

`scripts/build.sh --package` genera además un `.deb` y un `.AppImage`. Ya no
llevan `mutagen` (GPL): las etiquetas las escribe el crate de Rust del núcleo,
con lofty — [por qué importa](docs/DEPENDENCIES.md#mutagen-ya-no-va-en-los-paquetes).

> **Nota sobre las claves.** DanPlay funciona sin ninguna clave: escanea,
> organiza, reproduce y busca. Las claves solo activan extras: la de un
> proveedor de IA para la identificación difícil y el asistente, y
> `ACOUSTID_API_KEY` (gratis) para la huella acústica. Se guardan en
> `~/.config/danplay/` con permisos `0600` y nunca salen de tu equipo salvo
> hacia el servicio al que pertenecen.

### La IA que tú quieras

En **Ajustes → Inteligencia artificial** eliges el proveedor y pegas su
clave; la app te dice si funciona y si el modelo sabe usar herramientas
(lo que necesita el asistente). Vale cualquiera, porque todos hablan el
mismo protocolo:

| | |
| --- | --- |
| **Gratis, sin clave** | Para probar sin registrarte: LLM7, las rutas `:free` de Kilo, OpenCode Zen. Un botón, **«Probar gratis, sin clave»**, activa el primero que responda. Tienen límites (LLM7: 10 peticiones por minuto) y son servicios de terceros: tus preguntas pasan por ellos. |
| **Grandes laboratorios** | OpenAI, Anthropic (Claude), Google Gemini, Mistral, xAI, DeepSeek, Cohere, Perplexity… |
| **Plataformas de inferencia** | DeepInfra, OpenRouter, Together, Fireworks, Groq, Cerebras, SambaNova, Hugging Face, NVIDIA, Novita, Nebius, Hyperbolic, GitHub Models… |
| **Nubes corporativas** | Azure OpenAI, Amazon Bedrock, Cloudflare Workers AI, Databricks |
| **Asia** | Qwen (Alibaba), Kimi (Moonshot), GLM (Z.ai), MiniMax, StepFun, SiliconFlow… |
| **En tu equipo, sin clave** | Ollama, LM Studio, llama.cpp, vLLM, Jan, LocalAI, GPT4All, KoboldCpp… |
| **Otro** | cualquier URL compatible con OpenAI: un proxy, LiteLLM, un servidor propio |

La lista de modelos no está escrita en el programa —caduca en meses—:
sale del propio proveedor (lo que tu clave puede usar) y de
[models.dev](https://models.dev), un catálogo abierto que la app consulta
cada vez que abres el selector, con precio, contexto, si usa herramientas y
si está obsoleto. Se recuerdan todos los proveedores que configures, para
cambiar de uno a otro con un clic.

### Que las canciones se abran con DanPlay

La primera vez, DanPlay te lo pregunta. Si dijiste que no y te arrepientes,
está en **Ajustes → Abrir canciones con DanPlay**.

Lo hace la aplicación y no el instalador porque en Linux el reproductor
predeterminado es un ajuste **tuyo** —vive en tu `~/.config/mimeapps.list`—, y
un paquete que se instala como root no puede ponerlo sin decidir por ti. En
Windows es aún más estricto: desde Windows 8 ningún programa puede elegirse a
sí mismo, así que DanPlay se registra y te abre la página de Ajustes donde das
el último clic.

Funciona igual con el `.deb`, con el `.AppImage` y con la versión portátil de
Windows: si no hay un `.desktop` instalado, la app escribe el suyo en
`~/.local/share/applications` apuntando a donde esté.

## Primeros pasos

1. Abre DanPlay y elige tu carpeta de música. Te avisa si la carpeta que
   añades se solapa con otra que ya tenías.
2. Deja que escanee. Lee las etiquetas de cada archivo y construye el índice.
3. Suelta música nueva en `~/Musica/Entrada` y pulsa **Importar**: se
   identifica, se limpia el nombre y se archiva por artista.

Los reescaneos posteriores no vuelven a abrir los archivos que no han
cambiado, así que son casi instantáneos.

### Teclado

Con el foco fuera de un campo de texto:

| Tecla | Hace |
| --- | --- |
| **Espacio** | Pausa / reanuda |
| **0** o **Inicio** | Desde el principio (en pausa, además arranca) |
| **1**–**9** | Al 10 %, 20 %… 90 % de la canción |
| **←** / **→** | 5 s atrás / adelante |
| **Ctrl+←** / **Ctrl+→** | 10 s |
| **Mayús+←** / **Mayús+→** | 30 s |
| **↑** / **↓** | Volumen |
| **M** | Silencio |
| **N** / **P** | Siguiente / anterior |
| **S** / **R** | Aleatorio / modo de repetición |
| **A** / **B** | En modo estudio: aquí empieza / aquí acaba el tramo que se repite |
| **Ctrl+A** | En una lista: elegir todas las canciones |
| **Supr** | En una lista: quitar las elegidas del repertorio, o mandarlas a la papelera (con confirmación) |
| **Escape** | Soltar las elegidas |
| **Ctrl+Q** | Salir |

La aguja de la barra de tiempo se arrastra; un clic en la barra también vale.

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
danplay pulse                        el clic de toda la biblioteca, otra vez
danplay reuse                        recuperar pistas, clic y listas de otra copia
```

## Cómo está hecho

Cuatro capas, cada una en lo que mejor se le da:

```text
   Vue 3 + Vite            interfaz
        │  invoke()        único puente; el JS no hace HTTP
   Tauri / Rust            proceso principal, cola, audio, bandeja, MPRIS
        │  socket Unix     0600; sin puerto TCP
   Python                  identificación, índice, IA, etiquetas
        │
   Rust (PyO3)             etiquetas (lofty), hashes en paralelo y onda
```

- **Vue 3** para la interfaz: temas claros y oscuros, densidad ajustable,
  responsive hasta tamaño móvil.
- **Rust (Tauri)** para el proceso principal: la cola de reproducción, el
  icono de la bandeja, el mini reproductor y los mandos del sistema. Reproduce
  el audio con su propio motor (el mismo del celular): tramos a la muestra,
  el clic y las guías en el pulso, las pistas sin cortes y cada cosa por la
  salida que elijas.
- **Python** para el núcleo: la cascada de identificación, el índice SQLite con
  búsqueda de texto completo (FTS5), la IA, las etiquetas, y el vigilante que
  mantiene el índice al día con lo que cambia en el disco. Y dos redes con
  ONNX Runtime, sin PyTorch: Demucs, que separa las pistas, y Beat This!, que
  saca el pulso y el «1» para el metrónomo.
- **Rust (PyO3)** para lo que Python hace lento: las etiquetas de cada canción
  (con lofty, en paralelo al escanear), hashes en paralelo con rayon y la forma
  de onda. Es el principio del núcleo en Rust: `library/` es lo que la app
  usará directamente cuando no quede Python.

El código está en inglés; los comentarios y todo lo que ve el usuario, en
castellano.

## Portadas que prefieres no ver

Algunas descargas traen carátulas desagradables. En el menú de la canción (clic
derecho) o en su ficha hay **Difuminar la portada**: se sigue viendo que hay
algo, pero no qué. Se puede quitar cuando quieras.

**La imagen no se toca.** Lo que se guarda es una marca dentro del propio mp3,
así que la decisión viaja con la canción y sobrevive a perder el índice,
igual que las estrellas.

## Separar una canción en pistas

En el modo estudio, **Separar pistas** parte la canción en sus
instrumentos: batería, voces, bajo, guitarra, piano y el resto. Solo salen
los que tiene (una canción sin piano no trae pista de piano), y «Otros» es
lo que queda de la canción, así que todas juntas suenan exactamente como
ella: callar la batería deja lo demás intacto. Lo hace **Demucs**, el
separador de Meta, en tu equipo: no se sube nada, no hay límite de canciones
ni trozos de quince segundos. La primera vez se baja el separador (223 MB)
del repositorio de su autor, y ya no hace falta internet.

Va en dos pasadas, sin que elijas nada: en unos minutos ya tienes las
pistas y puedes usarlas; luego la batería y el bajo se vuelven a sacar, cada
uno con un separador especializado en él, más limpios, y se cambian solos
sin cortar lo que suena.

Luego suenan las pistas en vez de la canción, cada una en su carril con su
onda: **M** la calla, **S** la deja sola (o varias), y cada una tiene su
volumen y su panorama. El bucle, la velocidad sin cambiar el tono, el tono y
el metrónomo siguen funcionando igual, y las pistas nunca se desfasan entre
sí. Lo que se deja puesto se guarda con la canción.

```text
Separadas/
  Miel San Marcos - Que Se Abra El Cielo/
    Bateria.flac  Voces.flac  Bajo.flac  Guitarra.flac  Piano.flac  Otros.flac
```

Las pistas quedan en `Separadas/`, dentro de tu biblioteca, en FLAC (o en
Opus, cuatro veces menos, si lo eliges en Ajustes): archivos normales que
abre cualquier programa (un DAW, Ableton, el reproductor del sistema). No aparecen como canciones sueltas, y si pierdes
el índice un escaneo las vuelve a unir con su canción. La mezcla que suena
se guarda en mp3, flac o wav —la canción sin batería—, tal cual o a la
velocidad y el tono a los que la estás oyendo; para estudiarla en el
teléfono con todo, mejor la app del celular (abajo).

Se pueden separar varias de una vez desde el menú de clic derecho, o un
repertorio entero desde el suyo: van a una cola y se separan una detrás de
otra mientras sigues usando la app. Primero la pasada rápida de todas, así
el repertorio se puede ensayar cuanto antes, y luego las mejoras.

**Más instrumentos (máxima calidad):** una pasada más, opcional y lenta (unos
15-25 minutos por canción en un portátil), que saca la **percusión aparte de
la batería** (congas, pandereta…), la voz principal y los coros, la guitarra
eléctrica y la acústica, piano, órgano, teclados, cuerdas, metales y vientos,
los que suenen. La primera vez baja su separador (1,4 GB, el Mega53 de
ZFTurbo).

**Secuencias descargadas:** «Abrir secuencia» (en la barra de la izquierda)
abre la carpeta de una secuencia de MultiTracks o Loop Community con su
proyecto de Ableton: la canción queda en tu biblioteca con todas sus pistas,
las partes como marcadores, el clic exacto, y el «Click Track» y la «Guide»
por la salida del clic.

**Como una secuencia en directo:** cada carril (la canción, cada pista, el
clic y cada carril de guías) tiene sus botones **L** y **R**: el clic y las
guías a la izquierda y la música a la derecha con un cable en Y, o cada cosa
por una salida de tu interfaz, en estéreo o en mono. Se cambia sonando, sin
cortar, y se exporta igual: un archivo que suena como una secuencia.

**La guía automática:** DanPlay busca las partes de la canción (intro,
estrofas, coros, puente, solos…) y te las anuncia con una voz justo antes de
cada una, para estudiarla sin perderte. La voz se cambia cuando quieras.

**Guías habladas, en sus carriles:** debajo de la onda, hasta seis carriles
de guías (cada uno con su volumen y su salida: uno a los cascos del
baterista, otro a la banda). En cada uno pones guías escritas, que dice una
voz, o grabadas con tu micrófono, con la canción parada. Luego exportas la
canción o sus pistas, con las guías o sin ellas.

**Un editor para tus guías:** importa un audio corto del equipo o graba tu
voz, y arréglalo ahí mismo: recorta los bordes, ponle fundidos, quita el
ruido de fondo y los graves, iguala el volumen. Cada guía se ve con su onda,
y con el imán se pegan a los pulsos del clic (también si la canción cambia
de tempo).

**La onda a tu gusto:** seis estilos (barras, bloques, rayitas, suave,
contorno, con reflejo), y la rueda como en un editor de música: Ctrl + rueda
acerca, Mayús + rueda recorre la canción.

## En el celular

Lo que separaste se estudia también en el teléfono, con la misma cara de
DanPlay: cada parte con su volumen (callar la batería para tocar encima,
dejar sola la voz para aprenderla), el clic en el pulso de la canción con su
cuenta, la velocidad sin cambiar el tono, el tono, los tramos que se
repiten, la letra y los acordes. La app del celular **no separa ni ordena
nada**: trae lo que ya hiciste en la PC.

1. En DanPlay de la PC, en el menú de las pistas de una canción (el «⋯» del
   estudio, o el clic derecho sobre ella), **Convertir a DanPista**. Con varias
   elegidas, se llevan todas: un repertorio para la banda.
2. Queda un archivo `.danpista` por canción en `Celular/`, junto a
   `Separadas/` (entre 20 y 50 MB). La carpeta se abre sola.
3. Mándalo al teléfono como quieras: WhatsApp, Drive, un cable.
4. En el teléfono, tócalo y elige **DanPlay**, o ábrelo desde la app con
   **Importar** (en el menú ☰).

Viaja todo: las partes, el clic que calculó la PC, lo que dejaste puesto en
el estudio (la mezcla, los tramos, los marcadores), la letra y el cifrado.
Lo que cambies en el teléfono se queda en el teléfono, y si vuelves a traer
la misma canción se actualiza sin perderlo.

En el teléfono todo va con el dedo: **doble toque** en una canción la pone
(un toque la elige y abre sus botones grandes), **deslizar hacia arriba** la
barra del reproductor abre el modo estudio, y en el menú de lado (☰) están
las favoritas, **tus listas** (la del domingo, en su orden) y los ajustes.
Mientras suena, los mandos están en la **bandeja de Android** y en la
pantalla de bloqueo, también con la app en segundo plano.

Las guías que hiciste en la PC viajan con la pista: en el teléfono se oyen,
y cada carril se calla, se deja solo o cambia de volumen. Con **Salidas**
(en la cabecera del estudio) mandas el clic y las guías a un lado y la
música al otro, como una secuencia, en estéreo o en mono.

Y no solo pistas: en **Música del teléfono** suena cualquier canción que
tengas en él (MP3, M4A, FLAC…), también en la tarjeta SD o en las carpetas
de Telegram y WhatsApp, eligiendo las carpetas, con su modo estudio y su
clic.

Por ahora hay app para **Android** (8 o más nuevo). La de iPhone está
preparada pero falta compilarla en un Mac y firmarla con Apple.

**Instalarla** (también en los teléfonos de la banda): manda
`DanPlay-X.Y.Z-android.apk` por WhatsApp o Drive, tócalo en el
teléfono y acepta. Android pregunta la primera vez si dejas instalar apps
desde esa aplicación («Instalar apps desconocidas»): sí, solo para esa. Las
versiones nuevas se instalan igual, encima, sin perder las pistas. Como no
viene de Google Play, Play Protect puede avisar de que no conoce al
desarrollador: «Más detalles» › «Instalar de todas formas». La app va
firmada siempre con la misma llave; ese aviso solo desaparece publicándola
en Google Play.

## Formatos

Lee, organiza y **reproduce** `.mp3`, `.flac`, `.m4a`, `.wav`, `.ogg`,
`.opus`, `.aac` y `.wma`. Los dos últimos no los conoce el decodificador, así
que los pasa por ffmpeg sin tocar el archivo: suenan igual y siguen siendo lo
que eran. Puede unificar a mp3 con ffmpeg conservando etiquetas y carátula,
respetando las carpetas que marques como intocables (`Secuencias`, `Pistas`,
`Multitracks`…), donde comprimir sería perder calidad.

## Lo que NO hace bien

Prefiero decirlo aquí que en un issue:

- **La detección de tono por DSP no es fiable.** Medido contra valores
  conocidos: 2 aciertos de 8. Se probaron 60 combinaciones de perfiles y
  ponderaciones y todas se estancan ahí. Por eso el tono viene del cifrado
  publicado de la canción, cuando lo dice, o se pone a mano, y queda guardado
  en el `TKEY` del archivo.
- **El BPM es usable, no exacto.** 5 de 8 aceptando errores de octava.
- **Los acordes salen de cifrados publicados** (Ultimate Guitar, LaCuerda), con
  su enlace: los escribió alguien que toca la canción y no siempre son la
  grabación nota a nota. En una biblioteca de alabanza se encuentran en 7 de
  cada 10 canciones, y el tono en 1 de cada 3 (solo si el cifrado lo dice). Lo
  que no se encuentra no se inventa. La transposición es determinista y exacta.
- **Solo Linux probado.** Para Windows salen las dos formas desde Linux
  (`scripts/build-windows-cross.sh --zip --instalador`) y también desde la
  integración continua, pero **nadie las ha ejecutado todavía en un Windows de
  verdad**: hasta el primer arranque, es un «debería funcionar». macOS se
  compila en la integración continua (`macos.yml`) pero tampoco se ha abierto
  en un Mac de verdad. Los detalles, en
  [docs/WINDOWS.md](docs/WINDOWS.md).
- **El mini reproductor aparece donde puede.** Con X11 y con el AppImage sale
  pegado al icono de la bandeja. En Wayland lo coloca el escritorio: no existen
  las coordenadas globales y una aplicación no puede situar sus ventanas.
- **Separar en pistas tarda.** Va por el procesador, no por la gráfica: en
  un portátil corriente (un Ryzen 7 de 15 W), unos tres minutos hasta tener
  las pistas de una canción de cuatro y media, y unos seis y medio más hasta
  que la batería y el bajo quedan mejorados; usa hasta 3 GB de memoria
  mientras tanto. Y es un separador, no los multitracks del estudio: se
  cuela algo de un instrumento en otro, y el **piano** es la pista más floja
  (lo que no reconoce acaba en «Otros»). La **máxima calidad** separa más
  instrumentos, pero en mezclas muy cargadas (varios teclados, pads, loops)
  la guitarra y los teclados siguen saliendo con algo de lo demás; tarda unos
  15-25 minutos por canción y usa unos 4,5 GB de memoria.
- **Un solo usuario, una sola máquina.** No hay sincronización entre equipos.

## Documentación

| Documento | Qué encontrarás |
| --- | --- |
| [Arquitectura](docs/ARCHITECTURE.md) | Cómo encaja todo por dentro, decisiones de diseño y por qué. |
| [Contrato interno](docs/INTERNAL-CONTRACT.md) | Qué se dicen las capas: comandos, eventos y nombres de cada campo. |
| [Windows](docs/WINDOWS.md) | Las dos formas (portátil e instalador), cómo se construyen desde Linux y qué cambia respecto a Linux. |
| [Contribuir](docs/CONTRIBUTING.md) | Cómo montar el entorno, ejecutar las pruebas y en qué se puede ayudar. |
| [Cambios](CHANGELOG.md) | Qué trae cada versión. |
| [Dependencias](docs/DEPENDENCIES.md) | Qué usa DanPlay de otros y con qué licencia. |

## Pruebas

```bash
./scripts/test.sh
```

1.602 pruebas repartidas así (la CI las pasa todas menos las de humo, que
necesitan la app ya compilada):

| Tanda | Pruebas |
| --- | --- |
| Núcleo Python: nombres, etiquetas (mp3, flac, ogg, m4a…), índice y esquema, duplicados, teoría, la API entera sobre una biblioteca temporal de verdad, el vigilante de carpetas (mover, borrar, renombrar, carpetas que se van y vuelven), IA con cualquier proveedor, asistente, descargas y yt-dlp, separar en pistas (el motor contra una red de mentira, la cola, las carpetas, la mezcla), el pulso de Beat This! (también contra una red de mentira) y el clic de la mezcla, CLI, privacidad | 597 |
| Interfaz: componentes, páginas, reactividad, teclado y accesibilidad, listas grandes y agrupadas, temas, contratos, chat, modo estudio y su mezclador, proyección | 808 |
| Interfaz: rutas de medios en cada sistema | 7 |
| Rust: reproductor (con audio de verdad), cola, sesión, bandeja, núcleo, permisos por ventana, metrónomo (la cuenta y sus sonidos), limitador, las pistas separadas sonando juntas | 144 |
| Rust: hashes y forma de onda | 14 |
| De punta a punta: la interfaz contra el núcleo de verdad, en Chrome (Playwright) | 12 |
| Humo sobre la app **ya compilada** | 20 |

La biblioteca de prueba **se genera**: mp3 de verdad hechos con ffmpeg. Antes
hacía falta la música de quien ejecutara las pruebas y en cualquier otra
máquina la mitad se saltaban solas.

Las de humo arrancan la app de verdad, con sus datos aparte y una biblioteca
generada, y comprueban, entre otras cosas, que el socket es privado (`0600`),
que no queda ningún puerto TCP abierto, que el icono se registra en la bandeja
del escritorio y que el núcleo muere con la app sin dejar procesos huérfanos.

Además, en cada push: estilo y formato (ruff, ESLint, stylelint, Prettier,
rustfmt, clippy sin avisos), tipos (basedpyright, vue-tsc), cobertura mínima
(núcleo 70 %, interfaz 84 %), que el código solo-Windows compila, que
`src/icons.js` sigue siendo lo que genera `npm run icons` y los avisos de
seguridad de todas las dependencias.

## Licencia

**[PolyForm Strict 1.0.0](LICENSE)** — código a la vista, no código abierto.

| | |
| --- | --- |
| ✅ | Usar la app para lo tuyo: tu música, tus proyectos, tu iglesia, tu ONG, tu colegio |
| ✅ | Leer el código |
| ❌ | Usarla con fines comerciales |
| ❌ | Redistribuirla, modificarla o hacer obras derivadas |

Los derechos son de su autor, **Daniel Rangel**. Para cualquier otro uso,
incluido el comercial, escribe y lo hablamos.

Al abrir DanPlay por primera vez se piden los términos y condiciones (están
también en la app, en «Acerca de DanPlay»): sin aceptarlos se puede escuchar y
ordenar la música, pero no separar en pistas, usar el asistente ni
«Transformar».

## Autor y colaboración

DanPlay es obra de **Daniel Rangel**, que lo creó y lo desarrolla. Si quieres
colaborar —ideas, errores, canciones para probar—, escríbele por Telegram:
**[@Dani17r](https://t.me/Dani17r)**. Toda colaboración se hace con él y con
el crédito a su autor.

[Resumen en castellano](docs/LICENSE-SUMMARY.md) · [Licencias de las
dependencias](docs/DEPENDENCIES.md)

---

<p align="center">
  Hecho por Daniel Rangel para ordenar una biblioteca de verdad, no para una demo.
</p>
