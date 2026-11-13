# Plan de mejoras (noviembre de 2026)

Todo lo que se puede mejorar en DanPlay de escritorio, ordenado para irlo
haciendo por fases. Sale de una revisión del 13 de noviembre de 2026 sobre
la 1.40.1 (`fffa1e1`).

**Cómo se hizo.** Leyendo el código de los dos lados de cada cosa (la
interfaz y Rust), el paquete `.deb` de la 1.40.1 tal como se instala, el
registro de la app en este equipo, sus 14 rejillas de clic guardadas y su
base (324 canciones). **No se compiló ni se ejecutó nada**: nada de lo de
abajo está reproducido con la app en marcha.

**Cómo leerlo.** Cada punto lleva su estado y su tamaño:

- **C**, confirmado: se ve en el código o en los datos, leyendo los dos
  lados.
- **S**, sospecha: el mecanismo está, pero hay que reproducirlo antes de
  tocar nada.
- **P**, propuesta: no es un fallo, es algo que se puede hacer mejor.
- Tamaño **S** (un cambio pequeño en un sitio), **M** (varias piezas) o
  **L** (pide diseño y medir antes y después).

**Alcance.** Solo el escritorio, y solo Linux para compilar y probar en
local. Los paquetes de Windows, macOS y Android se lanzan en GitHub al
final, con todo validado (sección «Cómo se valida»).

---

## Fase 1: lo que se pidió arreglar primero

### A. Instalar y que suene

**Qué pasó.** El historial de apt de este equipo lo dice: el 31 de octubre
a las 09:13 se instaló `DanPlay_1.40.1_amd64.deb` y a las 09:22
`pipewire-alsa`. Era eso lo que faltaba.

**Por qué.** DanPlay saca el sonido por ALSA (`engine/src/output.rs`, con
cpal). En un sistema con PipeWire, ALSA solo llega al servidor de sonido si
está instalado su puente, `pipewire-alsa`. Sin él, «la salida del sistema»
de ALSA es la tarjeta 0 a pelo: puede estar ocupada por PipeWire, o ser el
HDMI, y entonces la app «suena» sin error y no se oye nada. Y el paquete no
lo pide:

```text
Depends: ffmpeg, libchromaprint-tools, libayatana-appindicator3-1,
         libwebkit2gtk-4.1-0, libgtk-3-0
```

| Id | Qué hacer | Dónde | Est. | Tam. |
| --- | --- | --- | --- | --- |
| A1 | El `.deb` pide el puente de sonido: `pipewire-alsa` o, como alternativa, `pulseaudio` (los dos paquetes se excluyen entre sí, así que tiene que ser una alternativa y no los dos). Con eso apt lo instala solo | `desktop/src-tauri/tauri.conf.json` (`bundle.linux.deb.depends`) | C | S |
| A2 | Al arrancar, comprobar el sonido como ya se comprueban ffmpeg y fpcalc: si hay servidor de sonido (el socket de PipeWire o de PulseAudio) y ALSA no tiene su puente, decirlo y ofrecer instalarlo con un clic. Es lo que cubre el AppImage y las distribuciones que no son Debian | `desktop/src-tauri/src/dependencies.rs` | C | M |
| A3 | Que nunca falle callado: si la salida abierta no es la del servidor de sonido, o no hay ninguna, un aviso visible en el reproductor con qué hacer, no solo en el cajón de «Salidas» | `desktop/src-tauri/src/player/engine.rs:811`, `Player.vue` | C | S |
| A4 | Con el AppImage se ofrece instalar ffmpeg y fpcalc aunque van dentro: la comprobación mira el PATH y no la carpeta `tools/` | `dependencies.rs:111`, `tools.rs:75` | C | S |
| A5 | Quitar del `.deb` lo que ya viaja dentro: `ffmpeg` y `libchromaprint-tools` van en `tools/` (91 MB y 5 MB). Hoy se instalan dos veces. Antes hay que resolver H5: el ffmpeg de dentro no trae `rubberband` y el del sistema sí | `tauri.conf.json` | C | S |
| A6 | Una prueba de instalación limpia, en local con Docker (ya está en este equipo): en una Debian 13 y una Ubuntu recién hechas, `apt install ./DanPlay.deb`, que no falte ninguna biblioteca (`ldd`), que ALSA tenga su puente y que la app conteste. Entra en `scripts/test.sh` como tanda aparte | `tests/`, `scripts/` | P | M |
| A7 | A más largo plazo, hablar con PipeWire o PulseAudio directamente, sin pasar por ALSA: así no hace falta ningún puente. Hay que mirar qué ofrece la versión de cpal que se use (aquí no se pudo comprobar: sus fuentes no están descargadas) | `engine/Cargo.toml`, `engine/src/output.rs` | P | L |

### B. «Re-analizar» el clic no hace nada

**Por qué.** El núcleo sí lo hace: recalcula la rejilla y la guarda
(`server/src/services/beats.rs:384`). Pero la interfaz no la recoge. Tras
la respuesta llama a `click.reload()`, que es `load()`, y `load()` sale en
su primera línea si la canción que suena ya tiene rejilla:

```js
if (sameSong(id, path) && (grid.value || loading.value)) return
```

El aviso dice «Clic arreglado», y ni la onda ni el clic cambian hasta
cambiar de canción. No hay ninguna prueba de la interfaz que lo cubra.

| Id | Qué hacer | Dónde | Est. | Tam. |
| --- | --- | --- | --- | --- |
| B1 | Una recarga de verdad: la respuesta ya trae la rejilla nueva; se le da al motor y a la onda. Con su prueba, que falle con el código de hoy | `desktop/src/composables/useClick.js:236,436`, `StudyBar.vue:994-1007` | C | S |
| B2 | En modo «propio» (pulsos ajustados a mano) el re-análisis queda tapado por el propio: decirlo, o preguntar si se quita | `useClick.js:249-257` | C | S |
| B3 | «Con la batería separada» pasa la red por la batería dentro de la petición (15-20 s) sin enseñar avance: que vaya por la cola, como el clic normal | `server/src/services/beats.rs:300-310` | C | M |
| B4 | El aviso final dice cuánto cambió de verdad. La única re-análisis guardada en este equipo dejó `fixed: 0` y aun así diría «analizado otra vez» | `StudyBar.vue:1008-1015` | C | S |

### C. Exportar: dos caminos a lo mismo

**Qué hay.** En el estudio hay dos «Exportar…»: el del «⋯» de la cabecera
(`StudyBar.vue:2083`) y el del «⋯» de las pistas (`StudyBar.vue:2020`).
Abren **la misma ventana**, solo con otra opción marcada, y en la ventana
se elige igualmente entre la canción, la mezcla y el celular. Además,
«Convertir a DanPista» del clic derecho de una canción es un tercer camino
a la opción «Para el celular».

| Id | Qué hacer | Dónde | Est. | Tam. |
| --- | --- | --- | --- | --- |
| C1 | Un solo «Exportar…» en el estudio. Propuesta: el de la cabecera, que abre con «la mezcla» marcada si están sonando las pistas; el del menú de las pistas se quita | `StudyBar.vue:2020,2083` | C | S |
| C2 | Los textos de ayuda de los tres «⋯» nombran opciones que ya no están en esos menús («convertir a DanPista», «Guardar la mezcla»), y el README dice que «Convertir a DanPista» está en el «⋯» del estudio | `StudyBar.vue:2266,2346,2384`, `README.md` | C | S |
| C3 | «Convertir a DanPista» del clic derecho: dejarlo solo donde aporta, con varias canciones elegidas (la ventana de exportar es de una), y con el mismo nombre que en la ventana | `useSongMenus.js:446,574` | P | S |
| C4 | Exportar con el clic por encima del 200 % da error: el mando llega a 3 y la ruta admite hasta 2 | `StudyBar.vue:3059`, `server/src/routes/stems.rs` | C | S |
| C5 | Exportar dos veces seguidas: la segunda dice «Guardada» con el nombre de la primera y no escribe nada (el trabajo ya en marcha se devuelve tal cual) | `StudyBar.vue:1935-1959`, `server/src/jobs.rs:186` | C | S |

---

## Fase 2: lo que «no funciona del todo bien» en el estudio

Todos los botones del estudio llaman a algo que existe: se cruzaron las 122
llamadas de la interfaz con las 121 rutas del núcleo y no hay ninguna
colgando. Los fallos son de estado que se desincroniza, no de cables
sueltos.

### D. El clic

| Id | Qué pasa y cómo se nota | Dónde | Est. | Tam. |
| --- | --- | --- | --- | --- |
| D1 | «Salir del modo estudio» no quita el clic: vuelve solo en la canción siguiente, y al entrar otra vez el botón dice «Parar» con el clic callado | `StudyBar.vue:2067`, `useClick.js:86,172` | C | S |
| D2 | Con el clic parado el panel dice «tempo a mano» y ofrece «el de la canción», aunque siga a la canción | `engine/src/metro.rs:114`, `StudyBar.vue:781` | C | S |
| D3 | El tempo a mano se oye multiplicado por la velocidad y la pantalla lo enseña sin multiplicar: a 0,8× con 100 puesto suenan 80 y dice 100 | `engine/src/metro.rs:189`, `StudyBar.vue:760,815` | C | S |
| D4 | La cuenta de entrada, con el clic apagado, no cae en el pulso de la canción: empieza siempre en «1» | `engine/src/metro.rs:173`, `engine/src/render.rs:878` | C | S |
| D5 | Ajustar pulsos a mano con ×2, ÷2 o «el 1 es el siguiente» puestos: Rust los aplica otra vez encima. El clic va al cuádruple o con el «1» corrido dos veces, y la onda pinta lo correcto | `StudyBar.vue:901`, `usePlayback.js:717` | C | S |
| D6 | El volumen del clic no se guarda: cada vez que se abre DanPlay vuelve al 80 % | `StudyBar.vue:888` | C | S |
| D7 | El volumen del clic llega a 150 % en su carril y a 300 % en el panel; con 250 % el carril lo enseña al tope y al tocarlo baja | `StudyTimeline.vue:1352`, `StudyBar.vue:3059` | C | S |
| D8 | Un fallo pasajero del detector se recuerda hasta reiniciar: esa canción se queda con la rejilla de reserva (un solo tempo, el «1» mucho peor) y nada lo dice | `server/src/services/beats.rs:745,810`, `useClick.js:212` | C | S |
| D9 | De dónde sale la rejilla (Beat This!, la de reserva, la propia) no se enseña en ningún sitio | `StudyBar.vue:774` | C | S |
| D10 | Una canción «sin pulso claro» vuelve a pasar la red entera en cada sesión: lo que oyó no se guarda | `analysis/src/pulse.rs:521`, `beats.rs:640` | C | S |
| D11 | La carga del clic no tiene plazo: si el detector se cuelga, el botón se queda en «Cargando…» para siempre | `useClick.js:208-225` | C | S |
| D12 | Con tempo a mano, la onda, el imán de las guías y la guía automática siguen usando la rejilla detectada, que es justo la que falló | `StudyBar.vue:748,1263,1550` | C | M |
| D13 | Un clic suelto a 100 bpm al empezar cada canción, con el clic puesto en automático | `player/engine.rs:464`, `useClick.js:245` | S | M |

### E. Al cambiar de canción

| Id | Qué pasa y cómo se nota | Dónde | Est. | Tam. |
| --- | --- | --- | --- | --- |
| E1 | La siguiente canción puede no traer sus guías, o sonar entera con «Pistas» encendido: Rust anuncia la canción nueva con la ruta de la anterior, y las órdenes que lleva esa ruta se descartan. Las pruebas no lo ven porque su doble emite las dos cosas juntas | `queue/logic.rs:27-62`, `StudyBar.vue:1229`, `usePlayback.js:746` | C (cada cuánto, S) | S |
| E2 | Saltando rápido, la velocidad, el tono y los ajustes de clic de una canción pueden caer sobre la siguiente | `StudyBar.vue:196-259` | S | S |
| E3 | La guía automática puede devolver las partes de otra canción: el trabajo se llama igual para todas y el resultado no dice de cuál es | `server/src/routes/cues.rs:125`, `jobs.rs:193` | C | S |
| E4 | La velocidad puesta en el estudio se queda para todo al reiniciar: se cierra con una canción al 70 % y al día siguiente todo suena al 70 % | `usePlayback.js:504,552` | C | S |
| E5 | Dos mandos de velocidad con listas distintas: desde 0,8× (del estudio), el botón del reproductor salta a 0,5×; y lo cambiado ahí no se guarda con la canción | `Player.vue:64`, `StudyBar.vue:86` | C | S |

### F. Salidas, guardado y la onda

| Id | Qué pasa y cómo se nota | Dónde | Est. | Tam. |
| --- | --- | --- | --- | --- |
| F1 | La salida de audio y el reparto guardados no se aplican hasta abrir el estudio: con una interfaz elegida, cada arranque suena por la del sistema, y al abrir el estudio la salida se reabre a mitad de canción | `useRouting.js:315` | C | S |
| F2 | Tras «Salir del estudio» el reparto sigue puesto: con «clic a la izquierda», toda la música sale por la derecha en mono el resto de la sesión | `StudyBar.vue:2067` | C | S |
| F3 | Un reparto a canales que la salida no tiene deja la música muda, y solo lo avisa el cajón de «Salidas». El reparto se guarda uno para todas las salidas, no por salida: lo de la interfaz de la iglesia se aplica a los cascos de casa | `engine/src/render.rs:1691`, `useRouting.js:14` | C | M |
| F4 | Notas de más de 4.000 caracteres: desde ahí no se guarda nada del estudio (tramos, marcadores, mezcla) | `server/src/routes/songs.rs:859` | C | S |
| F5 | La ruta admite 200 marcadores y el índice guarda 50: del 51 en adelante desaparecen al recargar | `library/src/index/songs.rs:574` | C | S |
| F6 | Los errores de las órdenes del estudio duran un cuarto de segundo: el siguiente estado los borra. Si falta el archivo de una pista, «Pistas» queda encendido y suena la canción | `usePlayback.js:600,474` | C | S |
| F7 | Guardar el estudio no dice si la etiqueta del archivo se escribió | `server/src/services/songs.rs:209` | C | S |
| F8 | Ajustando pulsos, el doble clic que añade uno también quita el tramo y mueve la canción | `StudyTimeline.vue:1425,723` | C | S |
| F9 | La tecla B sin A crea el tramo de 0 a ahora y manda la canción al principio | `StudyBar.vue:558` | C | S |
| F10 | Los tramos se redondean a la centésima, también en «Ajustar a los pulsos»: cada borde puede quedar hasta 5 ms fuera del pulso | `utils/segments.js:8` | C | S |
| F11 | Una guía cuyo archivo no se puede leer se salta sin decir nada | `engine/src/spec.rs:106` | C | S |
| F12 | Con las pistas sonando no se pide onda de detalle al acercar: los carriles se ven a bloques justo al ajustar guías | `StudyTimeline.vue:330` | C | M |

---

## Fase 3: que el reproductor no se quede callado

| Id | Qué pasa y cómo se nota | Dónde | Est. | Tam. |
| --- | --- | --- | --- | --- |
| G1 | Un archivo que falla al decodificarse a mitad de canción deja el reproductor «sonando» en silencio, con la aguja parada y la lista sin avanzar. El hilo que decodifica no está vigilado; el que sí lo está es otro | `engine/src/engine.rs:750` | C | M |
| G2 | Un archivo que existe pero no abre (dañado, de 0 bytes) para la lista: no se pasa a la siguiente | `player/engine.rs:476`, `queue/worker.rs:156` | C | S |
| G3 | Formatos que pasan por ffmpeg (m4a, opus, wma, aac): si se cambia de canción mientras se prepara, al acabar suena la de antes y la pantalla dice la nueva; con «Parar», arranca sola | `player/engine.rs:212,308` | C | M |
| G4 | Si ffmpeg falla al prepararlo, se reintenta sin fin: se queda en «preparando» sin error | `player/engine.rs:342` | C | S |
| G5 | Con fundido, si la siguiente necesita prepararse se corta la que suena hasta 12 s antes de su final | `player/engine.rs:414` | C | M |
| G6 | Cada estrella o guardado del estudio en un m4a u opus obliga a convertirlo otra vez y deja huérfano el FLAC anterior | `player/formats.rs:45` | C | M |
| G7 | «Pistas en Opus» se deshace al sonar: cada pista se reinfla a FLAC en la caché, cuatro veces más grande que lo ahorrado | `player/formats.rs:16` | C | M |
| G8 | 18 de los 319 `.mp3` de esta biblioteca son en realidad MP4. No admiten etiquetas (estrellas, listas y estudio viven solo en la base) y nada lo dice. Además el motor los toma por mp3, y el AAC entra 23 ms tarde: el clic iría corrido | `library/src/tags.rs:825`, `player/formats.rs:16` | C (lo del clic, S) | M |
| G9 | En esos 18 (y en WMA), si la fecha del archivo cambia por fuera, un escaneo les pone las estrellas a 0 | `library/src/index/scan.rs:667` | C | S |
| G10 | Un error a mitad de archivo se toma por el final: silencio hasta la duración nominal. Y un paquete que no decodifica adelanta esa pista respecto a las demás y al clic | `engine/src/decode.rs:297` | S | M |
| G11 | El final lo decide la duración que diga el archivo: en un mp3 sin cabecera puede cortarse el final o quedar silencio antes de la siguiente | `engine/src/engine.rs:736` | S | S |
| G12 | Con más de dos canales (un FLAC 5.1) solo suenan los dos primeros: se pierde el central, que suele ser la voz | `engine/src/decode.rs:323` | C | S |
| G13 | Empezar a grabar una guía puede congelar la ventana hasta 5 s | `src-tauri/src/record.rs:13` | C | S |
| G14 | Instalar ffmpeg con la app abierta no vale hasta reiniciar: se busca una sola vez | `src-tauri/src/tools.rs:78` | C | S |
| G15 | Las veces que el audio no llegó a tiempo se cuentan y nadie las enseña | `engine/src/render.rs:1133` | C | S |

---

## Fase 4: sonar mejor

### H. Velocidad y tono

Lo que hay de verdad hoy: todo suena por el motor propio (symphonia, el
render, Signalsmith Stretch y cpal). ffmpeg ya no toca la velocidad ni el
tono al sonar; solo convierte los formatos que el motor no lee y hace la
exportación. La velocidad va de 0,25× a 3× y el tono, una octava arriba o
abajo.

| Id | Qué mejorar | Dónde | Est. | Tam. |
| --- | --- | --- | --- | --- |
| H1 | **Tono con formantes**: hoy se transpone a secas, y la voz suena a «ardilla» o a «gigante» desde 2 o 3 semitonos. El estirador ya trae la compensación de formantes; falta una función en el envoltorio | `engine/cpp/stretch.cpp:54` | P | S |
| H2 | **Estirar según lo que es cada pista**: hoy todo va con la misma ventana de 120 ms. Con las pistas separadas, la batería con ventana corta y sin cambio de tono (golpes nítidos a 0,6-0,8×) y lo tonal con la larga. Es lo que más notaría un baterista practicando lento | `engine/src/render.rs:1012` | P | L |
| H3 | Un preajuste «nítido / suave» para cuando no hay pistas | `engine/cpp/stretch.cpp:38` | P | M |
| H4 | La entrada tras un salto, la cuenta o un cambio de velocidad sale blanda en sus primeros 90 ms (el estirador se calienta con lo que va a sonar y no con lo de antes): afecta al «1» después de la cuenta | `engine/src/render.rs:1017` | S | M |
| H5 | **Exportar «como suena» no usa el mismo estirador**: se oye Signalsmith y se exporta con ffmpeg. Y el ffmpeg que viaja en el paquete no trae `rubberband` (comprobado en el de la 1.40.1), así que se exporta por el camino pobre (`asetrate` y `atempo`, con 44.100 fijo). Exportar con el propio motor daría exactamente lo que suena | `library/src/stems.rs:612` | C | L |
| H6 | El limitador entra sin anticipación: al estirar, los picos crecen 1-3 dB y bombea a volumen normal | `engine/src/render.rs:1698` | P | M |
| H7 | Al estirar, las pistas que entran por relevo a una salida propia pueden sonar unos 60 ms tarde | `engine/src/render.rs:1383` | S | S |
| H8 | El final de la canción (o del último tramo con «Parar») pierde unos 150 ms al estirar: no se vacía el estirador | `engine/src/render.rs:1203` | S | S |
| H9 | Sin hueco entre canciones: hoy, sin fundido, hay un silencio corto en cada cambio (se nota en directos y popurrís). Precargar la siguiente; de paso se preparan por adelantado las que pasan por ffmpeg (G5) | `player/engine.rs`, `queue/worker.rs` | P | M |
| H10 | El metrónomo solo, sin canción o con ella en pausa | `engine/src/render.rs:801` | P | M |
| H11 | M4A, AAC, ALAC y Opus sin pasar por ffmpeg ni por la caché: quita la espera de la primera vez y casi todo G3-G7. Pide medir el retardo de cada archivo | `engine/src/decode.rs`, `player/formats.rs` | P | L |
| H12 | La aguja, la letra y el clic que se ve van por delante de lo que se oye cuando la salida tiene un búfer grande | `engine/src/output.rs` | S | M |
| H13 | Los sonidos de cencerro, claves y charles acaban cortados en seco | `engine/src/click.rs:135` | S | S |

### I. El pulso

Datos de este equipo: de 14 rejillas, 3 tienen huecos sin clic de 7, 10 y
26 s **donde la red sí oyó pulso** (13, 35 y 44 picos fuertes dentro), y 3
se quedan sin clic los últimos 19 a 27 s con pulso sonando.

| Id | Qué mejorar | Dónde | Est. | Tam. |
| --- | --- | --- | --- | --- |
| I1 | **Tramos con pulso que se quedan sin clic**: un tramo a tresillos o a otro tempo no se enlaza con la cadena y no se rellena. Sacar una mini rejilla propia de cada tramo sin cubrir y coserla | `analysis/src/pulse/grid.rs:176,241,300` | C en los datos | M |
| I2 | El umbral de «grabada a clic» es absoluto (15 ms): una canción lenta a 64 bpm se queda fuera por 16,7 ms y de ahí su hueco de 26 s. Que sea relativo al pulso | `grid.rs:59,960` | C en un caso | M |
| I3 | A tempo fijo no hay clic antes del primer pulso ni después del último: las 5 de tempo fijo dejan de 2 a 31 s finales sin clic. Extender, al menos hacia el principio | `grid.rs:972` | C | S |
| I4 | **La batería separada nunca mejora el clic sola**: solo si se pide a mano. Al terminar de separar, pasar la red por la batería en segundo plano y quedarse con la rejilla nueva solo si es mejor | `server/src/services/beats.rs:300,361` | C | M |
| I5 | El arreglo de saltos de fase puede mover medio pulso un tramo bien detectado, entre dos huecos largos | `grid.rs:417,484` | S | M |
| I6 | El compás solo puede ser 3 o 4: un 6/8 sale como 4+2 alternados, y al elegirlo a mano el «1» acierta una de cada dos veces | `grid.rs:785,1049` | S | M |
| I7 | «Sin acento» está puesto en 16 de las 22 canciones con ajustes, y ×2 en 7: una preferencia general («sin acento por defecto», «a corcheas por debajo de tantos bpm») ahorra ponerlo canción a canción | `StudyBar.vue`, `useClick.js` | P | S |
| I8 | **La espera de 15-20 s**: dos pasadas, como con las pistas. Primero el Beat This! pequeño y después el grande. Pide exportar otra vez el grafo con el guion del historial | `analysis/src/pulse.rs`, `data/pulse/` | P | M |
| I9 | **Preparar el clic de un repertorio entero**: hoy la cola es de 6 y tira lo que no cabe sin avisar; solo se preparan las 3 siguientes | `beats.rs:35,755`, `useClick.js:263` | C | M |
| I10 | Mientras carga, cada 700 ms se mira dos veces si están ONNX, ffmpeg y el grafo, y se lee dos veces el JSON | `server/src/routes/songs.rs:175` | C | S |
| I11 | La carpeta del pulso no se limpia nunca (aquí hay 2 análisis de baterías sin su rejilla) | `server/src/services/library.rs:98` | C | S |

### J. La guía automática

| Id | Qué mejorar | Dónde | Est. | Tam. |
| --- | --- | --- | --- | --- |
| J1 | **Con «sin acento», la guía recibe compases inventados** (uno de cada cuatro pulsos, empiece donde empiece): las partes caen hasta 3 pulsos fuera del «1». Afecta al uso real: 16 de 22. Los compases tienen que salir del análisis, no de cómo se acentúa el clic | `StudyBar.vue:1550`, `utils/beats.js:161` | C | S |
| J2 | La guía no espera al clic: pulsada antes de que cargue, parte la canción en trozos de 2 s y sale distinta, y peor | `StudyBar.vue:1591`, `analysis/src/structure.rs:346` | C | S |
| J3 | Una secuencia que ya trae sus partes exactas se vuelve a adivinar del audio: la interfaz no manda las partes aunque la ruta las acepta | `StudyBar.vue:1589`, `server/src/routes/cues.rs:98` | C | S |
| J4 | La voz no entra a tempo: se coloca por segundos y no en un pulso del compás anterior, como las guías de una secuencia | `structure.rs:1029` | C | S |
| J5 | **Los nombres de las partes** se apoyan en supuestos frágiles: «Coro» es lo que más se repite y suena más fuerte (en alabanza suele serlo el puente); estrofa y coro sobre los mismos acordes solo se distinguen por el timbre. La única guía guardada aquí anuncia «Coro 1, Puente 1, Coro 2, Puente 2» cada 7 s | `structure.rs:541,659,929,977` | S con un dato | L |
| J6 | Usar lo que ya se sabe: el cifrado trae las partes con nombre y en orden, y la letra con tiempos dice cuándo se canta cada línea. Casando los dos salen los inicios con sus nombres de verdad | `library/src/chord_sheets.rs`, `structure.rs` | P | L |
| J7 | Avisos de arreglo, que son los que este músico escribe a mano («entra bombo», «bajar», «entra batería»): se detectan con el volumen de cada pista por compás, que ya se calcula | `server/src/services/structure.rs:83` | P | M |
| J8 | La guía vuelve a decodificar todas las pistas cada vez, una detrás de otra, y no guarda nada | `server/src/services/structure.rs:83,146` | C | S |
| J9 | **Un banco para medirla**: 15-20 canciones con sus partes anotadas (las secuencias de MultiTracks ya las traen exactas), midiendo fronteras a un compás y nombres. Sin él, J5 y J6 son a ojo | `analysis/` | P | M |

### K. El tono de la canción y los acordes

En esta biblioteca, 1 de 324 canciones tiene tono.

| Id | Qué mejorar | Dónde | Est. | Tam. |
| --- | --- | --- | --- | --- |
| K1 | **Transponer no deja elegir Bb, Eb ni Ab**: la lista solo tiene sostenidos, y elegir «A#» escribe `A# D# Gm`. Ofrecer los doce tonos con su nombre de siempre | `DetailsPanel.vue:194`, `library/src/enrich.rs:682` | C | S |
| K2 | Un cifrado en menor pierde el modo al transponer: Em llevado «a G» queda en Gm y se titula «G» | `library/src/theory.rs:106` | C | S |
| K3 | La cejilla sugerida mezcla formas mayores y menores («traste 1 con formas de Am» para Bb mayor suena Bbm) y no sale en tonos menores | `theory.rs:178`, `enrich.rs:729` | C | S |
| K4 | **Sacar el tono de los acordes del cifrado** cuando la fuente no lo dice (encaje en los 24 tonos, con peso al primer acorde, al último y al más repetido), y sumarle la cejilla. Marcado como «deducido». Se mide contra los cifrados que sí dicen su tono | `library/src/chord_sheets.rs:608` | P | M |
| K5 | **Tono por señal** para lo que no tiene cifrado. Lo descartado fue un perfil sobre la mezcla entera (2 de 8). Hoy hay más: las pistas sin batería, el bajo aparte y los «1» del compás. Versión barata: con cifrado, decidir solo entre las 12 rotaciones de sus acordes, lo que además detecta que la grabación va en otro tono | `analysis/src/structure.rs:204` | P | L |

---

## Fase 5: que vaya más rápido

Con 324 canciones el índice no es el problema (la consulta de la lista
tarda 1,5 ms). Lo que se nota hoy va primero; lo que solo se notará con
miles de canciones, al final.

| Id | Qué pasa | Dónde | Est. | Tam. |
| --- | --- | --- | --- | --- |
| L1 | **Preguntar la versión de yt-dlp bloquea el estado de la app y, detrás, la lista**: 1,35 s con el cerrojo cogido. Cambiar de vista en ese rato se queda esperando | `server/src/services/ytdlp.rs:261` | C | S |
| L2 | El 1 y 2 de noviembre el registro tiene 28 «el núcleo no contesta» (la peor racha, de seis minutos) sin causa apuntada. Registrar toda petición de más de medio segundo | `src-tauri/src/core.rs:138` | S la causa | S |
| L3 | **Cada escritura propia provoca una recarga entera y un escaneo**: una estrella o un guardado del estudio (cada 0,6 s de cambios) reescribe el archivo, a los 2 s la interfaz recarga todo, y el vigilante escanea la biblioteca y relee ese archivo | `server/src/services/songs.rs:193`, `watcher.rs:342`, `App.vue:457` | C | M |
| L4 | Las 12 canciones sin artista se releen enteras en cada escaneo (cada 10 min y en cada arranque) | `library/src/index/scan.rs:588` | C | S |
| L5 | Rust pide el estado entero cada 2 s para leer un contador que tiene en su propio proceso | `src-tauri/src/core.rs:246` | C | S |
| L6 | Al arrancar, tres idas y vueltas en serie antes de pedir la lista | `App.vue:983`, `useLibrary.js:119` | C | S |
| L7 | Servir una miniatura lee antes la carátula original del archivo; una canción sin carátula se pide dos veces cada vez que entra en pantalla | `server/src/services/media.rs:146`, `CoverArt.vue:56` | C | M |
| L8 | Una migración de una sola vez (de la 1.30 a la 1.40) recorre la base hasta 8 veces en cada arranque | `server/src/startup.rs:123` | C | S |
| L9 | El 90 % del registro es ruido de dos bibliotecas (7.360 de 8.123 líneas): con rotación a 1 MB, lo útil se va antes | `src-tauri/src/main.rs:39` | C | S |
| L10 | La cola guarda la sesión entera a disco, esperando al disco, antes de avisar del cambio de canción | `queue/worker.rs:86`, `queue/session.rs:72` | C | S |
| L11 | El diario despierta cada 100 ms y el vigilante cada segundo, con la app en reposo | `server/src/services/journal.rs:204`, `watcher.rs:240` | C | S |
| L12 | Con más de 400 canciones, cada refresco de fondo recorta la lista a 400 y la rellena: el desplazamiento salta | `useLibrary.js:162` | C, sin reproducir | M |
| L13 | A escala: la búsqueda recorre y ordena toda la tabla en cada página; cada escaneo poda todas las cachés; una conexión nueva a la base en cada consulta; los resultados de los trabajos terminados viajan en cada estado durante 10 min | `library/src/index/search.rs:122`, `server/src/services/library.rs:168`, `app.rs:153`, `jobs.rs:76` | C | M |
| L14 | La lista sin agrupar ordena en binario y la agrupada en castellano: «Álvaro» iría después de la Z | `search.rs:71`, `utils/groups.js:48` | C | S |
| L15 | El detector de pulso no se va si la app muere, y salir no para ni el pulso ni el separador (el comentario dice que sí) | `server/src/startup.rs:334` | C | S |

---

## Fase 6: mantenimiento

| Id | Qué hacer | Est. | Tam. |
| --- | --- | --- | --- |
| M1 | `ARCHITECTURE.md` describe el reproductor de antes en cuatro secciones (el tono con rubberband, el metrónomo y el limitador de rodio, la velocidad con `atempo`, las pistas con `amerge`) y cita archivos que ya no existen. Reescribirlas con el motor de hoy | C | M |
| M2 | Comentarios y restos del camino viejo: «solo con ffmpeg» en la cola y el estado, el crate del motor descrito como «del celular», el campo `pitch_preserved` que siempre es verdadero y sus ramas en la interfaz que no se ven nunca | C | S |
| M3 | Los bancos de medida se fueron con Python (el pulso, el separador, los acordes, el asistente contra un modelo de verdad). Sin ellos, las fases 4 y 5 no se pueden medir: pasar a Rust al menos el del pulso y hacer el de la guía (J9) | C | L |
| M4 | El doble de las pruebas de la interfaz emite la canción y su ruta juntas, y Rust no (E1): que se parezca a lo de verdad | C | S |
| M5 | Archivos demasiado grandes: `StudyBar.vue` (3.460 líneas), `useChat.js` (2.161), `StudyTimeline.vue` (1.842), `App.vue` (1.705). Partir `StudyBar` por áreas (el clic, las pistas, las guías, exportar) antes de seguir añadiéndole cosas | C | L |
| M6 | Unas 30 ramas locales de fases ya fusionadas y de agentes | C | S |
| M7 | El archivo `.env` del proyecto, con claves, tiene permisos para todos (viene de la copia desde el disco NTFS) | C | S |

---

## Orden propuesto

1. **Fase 1** entera (A, B, C): es lo pedido, y casi todo es pequeño.
2. **Los fallos confirmados y pequeños** de las fases 2 y 3: D1-D11,
   E1-E5, F1-F11, G1-G4, J1-J4, K1-K3. Son los que explican «varias cosas
   no funcionan del todo bien».
3. **Medir antes de mejorar** (M3, J9): el banco del pulso y el de la guía.
4. **Calidad**, con sus números: el pulso (I1-I4), la velocidad y el tono
   (H1, H2, H5), la guía (J5-J7), el tono (K4, K5).
5. **Rapidez** que se nota hoy: L1-L9.
6. Lo que queda de las fases 3 a 6.

## Cómo se valida

- **Todo en Linux y en local.** `./scripts/test.sh --quick` tras cada
  bloque, y entero (con las e2e y las de humo) antes de cerrar una fase.
- **Cada arreglo, con su prueba**, que falle con el código de antes.
- **Las sospechas (S) se reproducen primero.** Si no se reproducen, se
  tachan aquí con el motivo.
- **Las mejoras de calidad, con números** antes y después (segundos sin
  clic con pulso sonando, acierto de fronteras de la guía, acierto de
  tono).
- **Sin compilar en GitHub hasta el final.** Las pruebas de la CI saltan
  con cada subida a `main` y con cada pull request; los paquetes, solo con
  una etiqueta `v*`. Se trabaja en una rama local y no se sube a `main` ni
  se etiqueta hasta tener la fase validada aquí. Entonces sí: etiqueta, y
  los paquetes de Windows, macOS y Android salen de GitHub.
- **La versión** sube con cada entrega, como siempre
  (`scripts/bump-version.sh`).

## Lo que hay que decidir

1. **Exportar** (C1, C3): ¿se queda solo el de la cabecera del estudio, y
   «Convertir a DanPista» del clic derecho solo con varias elegidas?
2. **«Sin acento» y ×2** (I7): en 16 de 22 canciones está sin acento. ¿Es
   gusto, o es que el «1» no se fía? Si es lo segundo, la mejora es el «1»
   y no una preferencia.
3. **Salir del estudio** (F2): ¿el reparto de salidas vuelve a «todo
   junto», o se queda como estaba?
4. **La velocidad** (E4): ¿se guarda solo con la canción, y al abrir
   DanPlay todo suena a su velocidad?
5. **Pistas en Opus** (G7): ¿se quita la opción, o se hace que el motor
   las lea sin convertir (H11)?
