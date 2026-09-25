<h1 align="center">DanPlay</h1>

<p align="center">
  <strong>Tu música vuelve a ser tuya.</strong><br>
  Un gestor de biblioteca musical de escritorio que identifica, ordena, etiqueta
  y reproduce lo que tienes en el disco duro. Sin nube, sin cuenta, sin suscripción.
</p>

<p align="center">
  <img alt="Linux" src="https://img.shields.io/badge/Linux-.deb%20%C2%B7%20AppImage-333?logo=linux&logoColor=white">
  <img alt="Windows" src="https://img.shields.io/badge/Windows-portable%20%2B%20instalador-0078d4?logo=windows&logoColor=white">
  <img alt="Vue 3" src="https://img.shields.io/badge/Vue-3-42b883?logo=vue.js&logoColor=white">
  <img alt="Rust" src="https://img.shields.io/badge/Rust-Tauri%202-b7410e?logo=rust&logoColor=white">
  <img alt="Python" src="https://img.shields.io/badge/Python-3.13-3776ab?logo=python&logoColor=white">
  <img alt="Version" src="https://img.shields.io/badge/version-1.13.0-4ade80">
  <img alt="Pruebas" src="https://img.shields.io/badge/pruebas-1.429%20en%20verde-2ea043">
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
| **Músico o equipo de alabanza** | Tonos, acordes y transposición con cejilla sugerida. Repertorios en el orden que tú decidas (se arrastran las canciones arriba y abajo), exportables a `.m3u` y como **hoja para el atril** (tono, bpm, cejilla, acordes, letra). Letra dentro del archivo, y con tiempos: la ficha sigue la línea que suena, y la **proyección** la pone en grande para la pantalla. **Modo estudio** a media pantalla: el tramo que se repite se elige arrastrando sobre la forma de onda (con regla de minutos) y se guarda como marcador con nombre y notas; **velocidad** sin cambiar el tono, **tono** en semitonos, y un **metrónomo** que detecta solo el pulso y el compás de la canción, entra en el «1» y se para o arranca aparte de ella. Todo guardado con la canción y borrado con ella. |
| **Quien tiene la música en el disco** | Cientos de descargas con nombres imposibles, duplicados y etiquetas rotas. Esto lo ordena. |
| **Quien no quiere depender de nadie** | Sin cuenta, sin nube, sin telemetría. Funciona con el wifi apagado (salvo lo que por definición necesita internet). |
| **Quien viene de otro reproductor** | Se lee y se escribe ID3 estándar. Kodi, foobar2000 o Rhythmbox verán tus estrellas y tus letras igual. |
| **Quien comparte música** | «Enviar por Telegram» desde el menú de una canción, de varias seleccionadas (Ctrl/Mayús) o de un repertorio entero, si Telegram Desktop está instalado (Linux, Windows y macOS): se abre con los archivos listos y eliges el chat allí. |

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

**Listas de cualquier tamaño.** La lista pinta solo las filas que se ven: da
igual que tengas 500 canciones o 20.000, siempre son unos 900 nodos en pantalla
y el mismo tiempo de dibujado.

**Un asistente que solo habla de música.** 28 herramientas: consultar la
biblioteca, armar repertorios, buscar letras, transponer acordes, descargar de
YouTube, comprobar datos en la web, sacar los tonos vecinos para un set sin
saltos, escribir la hoja para el atril. Si le preguntas de política te dice
que eso no es lo suyo. Solo descarga si se lo pides. Ve lo que tú ves («pon
la segunda», «añade las seleccionadas»), responde en vivo y se puede parar,
guarda las conversaciones y se buscan, enseña lo que cuesta cada respuesta,
y si su proveedor falla responde otro de los que tengas configurados. Corre
con la IA que tú elijas: OpenAI, Claude, Gemini, DeepInfra, OpenRouter… o un
modelo en tu propio equipo.

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
tocar tus datos): ver [Contribuir](docs/CONTRIBUIR.md).

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
   Rust (PyO3)             hashes en paralelo y forma de onda
```

- **Vue 3** para la interfaz: temas claros y oscuros, densidad ajustable,
  responsive hasta tamaño móvil.
- **Rust (Tauri)** para el proceso principal: la cola de reproducción, el
  icono de la bandeja, el mini reproductor y los mandos del sistema. Reproduce
  el audio nativamente (rodio), y detecta el pulso para el metrónomo.
- **Python** para el núcleo: la cascada de identificación, el índice SQLite con
  búsqueda de texto completo (FTS5), la IA, las etiquetas, y el vigilante que
  mantiene el índice al día con lo que cambia en el disco.
- **Rust (PyO3)** para lo que Python hace lento: hashes en paralelo con rayon y
  la forma de onda.

El código está en inglés; los comentarios y todo lo que ve el usuario, en
castellano.

## Portadas que prefieres no ver

Algunas descargas traen carátulas desagradables. En el menú de la canción (clic
derecho) o en su ficha hay **Difuminar la portada**: se sigue viendo que hay
algo, pero no qué. Se puede quitar cuando quieras.

**La imagen no se toca.** Lo que se guarda es una marca dentro del propio mp3,
así que la decisión viaja con la canción y sobrevive a perder el índice,
igual que las estrellas.

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
  ponderaciones y todas se estancan ahí. Por eso el tono que muestra la app
  viene de la IA (que sí conoce las canciones) y el del DSP queda como pista
  secundaria, con su confianza a la vista. Se puede corregir a mano y queda
  guardado en el `TKEY` del archivo.
- **El BPM es usable, no exacto.** 5 de 8 aceptando errores de octava.
- **Los acordes que da la IA son aproximados.** La app lo avisa. La
  transposición sobre ellos sí es determinista y exacta.
- **Solo Linux probado.** Para Windows salen las dos formas desde Linux
  (`scripts/build-windows-cross.sh --zip --instalador`) y también desde la
  integración continua, pero **nadie las ha ejecutado todavía en un Windows de
  verdad**: hasta el primer arranque, es un «debería funcionar». macOS ni
  siquiera se ha compilado. Los detalles, en
  [docs/WINDOWS.md](docs/WINDOWS.md).
- **El mini reproductor aparece donde puede.** Con X11 y con el AppImage sale
  pegado al icono de la bandeja. En Wayland lo coloca el escritorio: no existen
  las coordenadas globales y una aplicación no puede situar sus ventanas.
- **Un solo usuario, una sola máquina.** No hay sincronización entre equipos.

## Documentación

| Documento | Qué encontrarás |
| --- | --- |
| [Arquitectura](docs/ARQUITECTURA.md) | Cómo encaja todo por dentro, decisiones de diseño y por qué. |
| [Contrato interno](docs/CONTRATO-INTERNO.md) | Qué se dicen las capas: comandos, eventos y nombres de cada campo. |
| [Windows](docs/WINDOWS.md) | Las dos formas (portátil e instalador), cómo se construyen desde Linux y qué cambia respecto a Linux. |
| [Contribuir](docs/CONTRIBUIR.md) | Cómo montar el entorno, ejecutar las pruebas y en qué se puede ayudar. |
| [Cambios](CHANGELOG.md) | Qué trae cada versión. |
| [Dependencias](docs/DEPENDENCIAS.md) | Qué usa DanPlay de otros y con qué licencia. |

## Pruebas

```bash
./scripts/test.sh
```

1.429 pruebas repartidas así (la CI las pasa todas menos las de humo, que
necesitan la app ya compilada):

| Tanda | Pruebas |
| --- | --- |
| Núcleo Python: nombres, etiquetas (mp3, flac, ogg, m4a…), índice y esquema, duplicados, teoría, la API entera sobre una biblioteca temporal de verdad, el vigilante de carpetas (mover, borrar, renombrar, carpetas que se van y vuelven), IA con cualquier proveedor, asistente, descargas y yt-dlp, CLI, privacidad | 533 |
| Interfaz: componentes, páginas, reactividad, teclado y accesibilidad, listas grandes y agrupadas, temas, contratos, chat, modo estudio, proyección | 727 |
| Interfaz: rutas de medios en cada sistema | 7 |
| Rust: reproductor (con audio de verdad), cola, sesión, bandeja, núcleo, permisos por ventana, metrónomo | 116 |
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

Los derechos son del autor. Para cualquier otro uso, incluido el comercial,
escribe y lo hablamos.

[Resumen en castellano](docs/LICENCIA-RESUMEN.md) · [Licencias de las
dependencias](docs/DEPENDENCIAS.md)

---

<p align="center">
  Hecho para ordenar una biblioteca de verdad, no para una demo.
</p>
