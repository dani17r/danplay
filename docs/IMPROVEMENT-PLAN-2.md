# Plan de mejoras 2: seguridad, rapidez, tamaño y limpieza (noviembre de 2026)

Revisión completa de DanPlay hecha el 22 de noviembre de 2026 sobre la 1.44.1
(`d6ebfde`, `main`). Sigue a [IMPROVEMENT-PLAN.md](IMPROVEMENT-PLAN.md), que
era sobre fallos de uso y está casi todo hecho; este es sobre lo que no se ve:
que no se corte el sonido, seguridad, rapidez, tamaño de descarga, la CI,
dependencias y limpieza.

Está escrito para que lo aplique un agente sin más contexto que este archivo.
Cada punto dice qué pasa, dónde, qué hacer y cómo se comprueba.

**Cómo se hizo.** Cinco lecturas a fondo en paralelo (seguridad; servidor y
biblioteca; interfaz; motor y análisis; CI, paquetes y guiones), cada hallazgo
cotejado después contra el código, más `cargo audit`, `bun audit`,
`bun outdated`, `cargo tree` y los tamaños del build y del `.deb` de la
1.44.1. **No se compiló ni se ejecutó la app**: los tiempos y ahorros que no
digan «medido» son estimaciones.

**Resultado en una frase.** No hay nada grave: ninguna vulnerabilidad en lo
que va dentro de la app, ningún fallo que hoy corte el sonido en uso normal.
Lo que hay son unos 90 puntos pequeños y medianos; los que más valen están en
el orden de abajo.

---

## 1. Normas para quien lo aplique

### Entorno

- El equipo **no compila Rust en el anfitrión**. Todo `cargo` va dentro del
  contenedor: `.dev/dk.sh sh -c '<orden>'` desde la raíz. Siempre con
  `--release` (es lo que ya está compilado en `target/`). Tiempo de espera
  largo (10 min). Nunca dos `cargo` a la vez, nunca `cargo clean`.
- Si Cargo corre menos pruebas de las esperadas, da por bueno un binario
  viejo: `.dev/fresh.sh 'cargo test --release -p <crate>'`.
- La interfaz corre en el anfitrión, desde `desktop/`:
  `./node_modules/.bin/vp test run`, `vp run lint`, `vp run lint:css`,
  `vp run format:check` (se arregla con `vp run format`), `vp run typecheck`.
- En esta shell `grep` es un envoltorio que se salta archivos: usar
  `/usr/bin/grep -rn` (con `-a` para `player/formats.rs`, que tiene un byte
  nulo).
- Los números de línea son de `d6ebfde`. **Buscar siempre por contenido**: se
  habrán movido.

### Reglas duras

1. Rama nueva desde `main` (`git switch -c fase-11/<lote>`). **Nunca**
   `git push`, etiquetas, PR ni `scripts/github.sh`. Publicar lo decide Daniel.
2. Un commit por punto (o grupo muy pegado), en castellano y con prefijo, como
   `git log --oneline -15`. `git add` con nombres, nunca `-A`. El cuerpo dice
   el porqué.
3. **Cada arreglo, con su prueba, y la prueba tiene que fallar con el código
   de antes.** Comprobarlo de verdad (quitar el arreglo un momento, verla en
   rojo, reponerlo) y decirlo en el informe.
4. **Nadie puede oír nada.** Lo que cambie cómo suena se justifica con una
   medida dentro de una prueba (niveles, muestras, espectro). Si no se puede
   medir, no se hace.
5. Los puntos **S** (sospecha) se reproducen o se miden primero. Si no se
   reproducen, se saltan y se explica. No se fuerza un cambio.
6. Los de rapidez: medir antes y después y apuntar los números.
7. Dependencias: no añadir, quitar ni subir ninguna **salvo las que nombra el
   lote 7**.
8. No tocar `CHANGELOG.md`, `desktop/src/data/releases.js`, la versión ni
   `README.md` (salvo los puntos DOC). En el informe, una línea de novedades
   por cada punto que Daniel notaría.
9. Sin refactorizaciones de paso ni reformatear lo que no se cambia.
   Identificadores en inglés; comentarios y textos en castellano, y los
   comentarios explican el porqué.
10. Nada debe retrasar la entrada de la canción al dar play. Si un cambio
    del motor puede hacerlo, medir con `real_start_latency`
    (`engine/src/real_files.rs`, `--ignored`, con `DANPLAY_LATENCY_DIR`).

### Puerta de cada lote

Antes de dar un lote por cerrado, todo en verde:

```sh
# Rust: los crates tocados y los que dependen de ellos
# (server ← library, analysis; app ← server, engine; mobile ← engine)
.dev/dk.sh sh -c 'cargo fmt --all --check'
.dev/dk.sh sh -c 'cargo clippy --workspace --all-targets --release -- -D warnings 2>&1 | tail -30'
.dev/fresh.sh 'cargo test --workspace --release 2>&1 | tail -40'
# la CI usa un Rust más nuevo: si está instalado, también
# CARGO_TARGET_DIR=.dev/target-1100 cargo +1.100.0 clippy ... (ver memoria del proyecto)

# interfaz
cd desktop && ./node_modules/.bin/vp run lint && ./node_modules/.bin/vp run lint:css \
  && ./node_modules/.bin/vp run format:check && ./node_modules/.bin/vp run typecheck \
  && ./node_modules/.bin/vp test run
```

Línea base de la 1.44.1: vitest 1679, Rust ~1556, e2e 29, humo 18. Las e2e
(`vp run e2e`, pide `target/release/danplay-server` recién compilado), las de
humo y `scripts/test-install.sh` se corren una vez al final de los lotes 3, 4
y 6, no en cada punto.

### Cómo leer cada punto

- **C** confirmado leyendo el código · **S** sospecha (medir antes) · **P**
  propuesta.
- Tamaño **S** (un sitio), **M** (varias piezas), **L** (pide diseño).
- **Cuidado** `bajo` / `medio` / `alto`. Los de cuidado **alto** conviene que
  los revise un modelo grande (o Daniel) antes de fusionar: tocan el hilo de
  audio, la escritura de etiquetas o la publicación.

### Informe final de cada lote

Por cada punto: hecho / en parte / saltado y por qué; archivos; la prueba y
la confirmación de que fallaba antes; las medidas; lo que haya que decidir; y
`git log --oneline main..HEAD`.

---

## 2. Orden

| Lote | Qué | Puntos | Por qué va ahí |
| --- | --- | --- | --- |
| 0 | Medir antes | MED-1…4 | Sin números no se puede decir que algo mejoró |
| 1 | Que no se corte el sonido | AUD-1…17 | Es lo peor que le puede pasar a quien toca en directo |
| 2 | Seguridad | SEG-1…9 | Todo pequeño; nada grave |
| 3 | El núcleo, rápido | NUC-1…14 | Guardados, arranque y disco |
| 4 | La interfaz, fluida | INT-1…15 | El estudio se repinta entero 4 veces por segundo |
| 5 | CI y cadena de suministro | CI-1…13 | Protege la publicación; ahorra minutos |
| 6 | Paquetes más pequeños | PAQ-1…4 | De 145 MB a unos 89 MB el `.deb` |
| 7 | Dependencias y compilación | DEP-1…7 | Lo único que toca `Cargo.toml` y `package.json` |
| 8 | Limpieza y documentación | LIM-1…12, DOC-1…5 | Sin efecto para quien usa la app |
| 9 | Partir `StudyBar.vue` | M5 | El último: es el de más riesgo |

Los lotes 1, 2, 3 y 4 no se pisan entre sí (motor / seguridad / servidor /
interfaz): pueden ir en ramas distintas. El 9 va después del 4.

**Si solo hay tiempo para poco**, lo que más vale por lo que cuesta: AUD-1,
AUD-2, AUD-3, AUD-5, SEG-2, SEG-3, NUC-3, NUC-8, INT-1, CI-1, CI-2, PAQ-1.

---

## Lote 0 — Medir antes

**MED-1 · Línea base.** Correr la puerta entera sobre `main` sin tocar nada y
apuntar cuántas pruebas pasan en cada tanda y cuánto tardan. Es con lo que se
compara todo.

**MED-2 · El estudio sonando no se mide.** `desktop/e2e/perf/startup.perf.js`
mide arranque, desplazamiento, ficha, búsqueda y abrir el estudio, pero nada
con el estudio abierto y sonando. Añadir una sección 7 con el mismo bucle de
fotogramas del desplazamiento (hacia las líneas 144-160): 5 s sonando con el
estudio abierto; 30 `wheel` con Ctrl sobre `.tl-box`; pulsar «Grande».
Apuntar media, p95 y peor fotograma. Se compara antes y después con
`DANPLAY_PERF_DIST`. Sirve a INT-1, INT-2 e INT-3. Tamaño S.

**MED-3 · Lo que cuesta escribir en la base.** Banco `#[ignore]` en
`server/src/app.rs` (junto al que ya hay hacia la línea 295): 200 escrituras
seguidas de una estrella por `db_write`, tiempo total, y si
`danplay.db-wal` existe entre una y otra. Si desaparece tras cada escritura,
SQLite está haciendo punto de control (con `fsync`) cada vez que se cierra la
única conexión, que es lo que sospecha NUC-2. Tamaño S.

**MED-4 · La lista con filas de verdad.** Los bancos de la lista
(`library/src/index/search.rs` hacia 433-445, `server/src/app.rs` hacia
303-316) insertan filas sin letra, cifrado ni estudio. Rehacerlos con 20 000
filas con 2 KB de letra y 3 KB de cifrado, y apuntar la primera página de 400
y la carga entera. Sirve a NUC-9. Tamaño S.

---

## Lote 1 — Que no se corte el sonido

Crates: `engine/`, `desktop/src-tauri/src/player/`, `mobile/src/`. Nada de
este lote cambia el sonido salvo AUD-15, que lleva su medida.

### AUD-1 · Un pánico en el callback deja la salida muda hasta reiniciar

C · M · cuidado **alto**

- **Qué pasa.** `engine/src/output.rs` (hacia 487):
  `if let Ok(mut r) = deck.try_lock() { … } else { xruns.fetch_add(1) }`. Un
  `Mutex` envenenado devuelve error para siempre: esa canción calla y solo
  sube el contador. `broken` solo lo pone el callback de errores de cpal
  (hacia 507-513); si el callback de datos entra en pánico, el hilo de cpal
  muere sin avisar y el bucle de 250 ms (hacia 103-116) nunca reabre. El
  perfil lleva `panic = "unwind"` justo para poder levantarse, y aquí no hay
  quien lo haga. En AAudio y CoreAudio el pánico cruza FFI y aborta la app.
- **Qué hacer.**
  1. Sacar el cuerpo del callback a `fn fill<T>(…)` y envolver la llamada en
     `std::panic::catch_unwind(AssertUnwindSafe(…))`. Si falla: `data` a
     silencio y `broken.store(true, Release)`.
  2. En el `try_lock`, recuperar el envenenado:
     `Err(TryLockError::Poisoned(p)) => p.into_inner()` y seguir.
  3. Un latido: `AtomicU64` que el callback incrementa. El bucle de 250 ms
     marca `broken` si no avanza en 1 s con la salida abierta (cubre también
     una salida Bluetooth colgada).
- **Prueba.** Llamar a `fill` con un `Render` que entre en pánico por un
  gancho `#[cfg(test)]` (como el `break_at` del feeder): sale silencio,
  `broken` queda puesto y la llamada siguiente suena.

### AUD-2 · Un pánico al saltar mata el hilo que decodifica

C · S/M · cuidado medio

- **Qué pasa.** `engine/src/feeder.rs` (hacia 486-491):
  `Order::Seek(frame) => feeder.seek(frame)` y `Segments` van fuera de
  `fill_safely`. `seek` llega a symphonia sobre archivos de fuera
  (`decode.rs` hacia 304-344). Si muere, no llega ningún bloque, `failed` no
  se pone y `orders_done` no sube: «sonando» en silencio.
- **Qué hacer.** `fn handle(feeder, order) -> bool` con `catch_unwind`. Si
  falla, marcar el feeder para que el siguiente bloque libre salga como en la
  rama de error de `fill_safely` (`frames = 0`, `last`, `failed = true`,
  generación actual). `orders_done` sube siempre.
- **Prueba.** Gancho `#[cfg(test)]` que entre en pánico en el salto: llega el
  bloque final con `failed`, y un `seek` posterior revive.

### AUD-3 · En el celular, cada cambio del mezclador hace un hueco

C · S · cuidado bajo · **defecto audible**

- **Qué pasa.** `mobile/src/player.rs` (hacia 314-318): `Command::Stems`
  llama a `apply_mix()` y a `apply_routing()`. `Engine::set_routing`
  (`engine/src/engine.rs` hacia 724-737) manda siempre, sin comparar, y
  `send_routing` crea un `Stretch::new` por bus. Sonando, eso es un
  `Pending::Reroute` (`render.rs` hacia 709-714): envolvente abajo,
  estiradores a cero y arriba: unos 10 ms de bajón por cada toque del
  mezclador, y a 0,7× se pierde además el estado del estirador. El escritorio
  no lo sufre (`player/engine.rs` hacia 846 solo llama a `apply_mix`).
- **Qué hacer.** Al principio de `Engine::set_routing`:
  `if *lock(&self.session).routing == routing { return; }` (`Routing` ya es
  `PartialEq`; comparar antes de envolver en `Arc`). De paso, en el
  escritorio cada carga crea los estiradores dos veces (`start_song`,
  `engine.rs` hacia 393, y `after_open`, `player/engine.rs` hacia 656): con la
  comparación la segunda ya no hace nada.
- **Prueba.** `Engine::offline`, sonar, repetir `set_routing` con el mismo
  enrutado: ninguna ventana de 3 ms baja de RMS frente a una pasada de
  referencia sin la orden.

### AUD-4 · El hilo del reproductor del celular no se vigila

C · S · cuidado bajo

- **Qué pasa.** `mobile/src/player.rs` (hacia 150-152):
  `spawn(move || worker.run(&rx))` sin `catch_unwind`. El escritorio sí lo
  tiene (`desktop/src-tauri/src/player/mod.rs` hacia 257). Un pánico al abrir
  mata el hilo y `send` lo calla.
- **Qué hacer.** El mismo bucle que el escritorio alrededor de `handle` y
  `check_end`; tras caer, `Engine::new()`, `loaded = None` y un `error`
  legible en el estado.
- **Prueba.** La del escritorio adaptada.

### AUD-5 · NaN o infinitos de un archivo llegan al estirador y a la tarjeta

C · S · cuidado bajo

- **Qué pasa.** `engine/src/decode.rs` (hacia 448-460) mete en el `fifo` lo
  que da symphonia sin mirar (WAV/AIFF en coma flotante; paquetes dañados).
  En el limitador (`render.rs` hacia 1884-1890) un `inf` da `inf * 0 = NaN`,
  y `output.rs` (hacia 502) lo entrega con `T::from_sample`. Estirando es
  peor: Signalsmith guarda la fase anterior por banda y se queda mudo o con
  ruido hasta el siguiente `reset`.
- **Qué hacer** (sin tocar el limitador):
  1. En `decode_packet`, tras `copy_to_slice_interleaved`:
     `if !v.is_finite() { *v = 0.0 }`.
  2. En `output.rs`: `T::from_sample(if v.is_finite() { *v } else { 0.0 })`.
- **Prueba.** Un WAV en coma flotante con NaN e `inf` (hace falta un
  ayudante de formato 3: el `wav()` de `testing.rs` escribe 16 bits): lo
  leído es finito. Y un bloque con NaN por `render_into` más `Limiter`:
  salida finita.

### AUD-6 · Tres asignaciones de memoria que quedan en el hilo de audio

C · S · cuidado medio

1. **Cuenta de entrada** (viva en el celular): `render.rs` (hacia 1011-1017)
   hace `.collect()` y el `Vec` se suelta dentro del callback. → Un
   `Vec::with_capacity(1024)` creado en `Render::new` y reutilizado.
2. **Relevo**: `render.rs` (hacia 1539),
   `let Handoff { ring, routing, .. } = *handoff;` libera el `Box` en el
   callback. → `ring: Option<Ring>` con `take()` y mandar el `Box` a `Trash`.
3. **Conversor de tasa**: `resample.rs` (hacia 74) nace con capacidad
   `HALF * channels` y crece (hacia 111) en los primeros callbacks cuando la
   salida no va a 44,1 kHz. →
   `Vec::with_capacity((2 * HALF + 2 + CHUNK) * channels * 2)`.

**Prueba.** Las de cuenta y relevo de `render/tests.rs` dan lo mismo; para el
conversor, `capacity()` igual tras 1 s de `pull`.

### AUD-7 · Tras caerse la salida se tarda casi un segundo en reabrir

C · S · cuidado bajo

- **Qué pasa.** `output.rs`: `broken` se mira cada 250 ms (hacia 104) y
  después se esperan 500 ms antes de reintentar (hacia 122-123), también la
  primera vez. Unos auriculares USB que se desenchufan: hasta 750 ms más la
  apertura, en silencio.
- **Qué hacer.** Reintentar ya la primera vez y esperar 500 ms solo si
  `open` falla. Sacar la decisión a una función pura
  `next_wait(failed_to_open: bool) -> Duration` y probarla.

### AUD-8 · Cambiar de canción espera al hilo de la anterior

C · S · cuidado medio

- **Qué pasa.** `engine/src/engine.rs` (hacia 139): `let _ = t.join();`
  dentro de `close`, llamado al descargar el plato. Con el archivo en un
  disco dormido o de red, bloquea el hilo de órdenes: ni pausa ni siguiente
  responden.
- **Qué hacer.** Soltar el `JoinHandle` sin esperar: el hilo acaba solo al
  cerrarse `orders`. Mirar antes si alguna prueba cuenta con el `join`.
- **Prueba.** Las de carga y cambio de canción siguen en verde; `Engine`
  suelto no deja hilos vivos al acabar la prueba (contarlos por nombre si hay
  ayudante, o confiar en que el proceso de pruebas termina).

### AUD-9 · Se abre y se lee la canción siguiente 25 veces por segundo

C · S · cuidado bajo

- **Qué pasa.** `desktop/src-tauri/src/player/engine.rs` (hacia 1070-1082):
  en `chain()` se calcula `ready` (que llama a `ready_file` → `is_native` →
  `File::open` y 192 bytes, `formats.rs` hacia 161) **antes** de mirar
  `over`. 25 veces por segundo sonando y hasta 500 en los últimos 2 s, justo
  cuando se mide el hueco entre canciones.
- **Qué hacer.** `if !over { return false; }` antes de calcular `ready`. Lo
  cubren las pruebas de encadenado que ya hay.

### AUD-10 · La caché de convertidos tira lo que más se usa

C · S · cuidado bajo

- **Qué pasa.** `player/formats.rs` (hacia 441) ordena por
  `accessed().or_else(modified)`. Con `noatime`, o en NTFS con el último
  acceso apagado, eso es orden de creación. `cached_in` (hacia 240) no marca
  el uso. Pasados 3 GB se va lo convertido primero aunque suene a diario, y
  la próxima vez espera a ffmpeg.
- **Qué hacer.** `formats::mark_used(&flac)` (el `touch` que ya hay, solo si
  lleva más de una hora) en `Engine::playable` (`player/engine.rs` hacia
  465), no en `cached`, que se consulta en cada vuelta. `prune` ordena por
  `modified()`.
- **Prueba.** Carpeta temporal con tres FLAC y fechas puestas a mano: se va
  el menos usado.

### AUD-11 · El celular no suelta la salida y sondea en pausa

C · S/M · cuidado medio · batería

- **Qué pasa.** `release_output` solo se llama en el escritorio
  (`player/engine.rs` hacia 1015). En el celular, en pausa siguen vivos el
  callback de AAudio, el conversor de tasa sobre silencio, `feeder.rs` (hacia
  474) con `recv_timeout(4 ms)` (unos 250 despertares por segundo) y
  `mobile/src/player.rs` (hacia 225) cada 250 ms.
- **Qué hacer.**
  1. En `Worker::run` del celular: pasados ~30 s sin sonar,
     `engine.release_output()` y `recv()` bloqueante.
  2. En `feeder::run`: `recv()` sin plazo si `ended()`; con el anillo lleno,
     alargar el plazo 4 → 8 → … → 64 ms mientras no se libere ningún bloque.
     Hay 1,5 s de colchón y una orden despierta al momento.
- **Prueba.** Unitaria de la función del plazo. Y `real_start_latency`: la
  entrada tras un salto no empeora (regla 10).

### AUD-12 · Prioridad del hilo de audio

Dos partes, de riesgo distinto:

- **a) Los hijos que analizan, a prioridad baja en Windows.** C · S · cuidado
  bajo. `server/src/services/separation.rs` (hacia 51-56) y `beats.rs` (hacia
  1293) hacen `nice(10)` solo en Unix. En Windows, Demucs corre a prioridad
  normal con todos los núcleos contra el callback. → Añadir
  `BELOW_NORMAL_PRIORITY_CLASS` (`0x4000`) al `creation_flags(0x0800_0000)`
  con que se lanzan. Prueba: la función que arma los indicadores, pura.
- **b) Tiempo real para el hilo de audio.** S · S · cuidado **alto** ·
  **espera a Daniel**. En cpal 0.17.3, `boost_current_thread_priority` es
  una función vacía sin el rasgo `audio_thread_priority` (comprobado en
  `host/alsa/mod.rs:879-896`). Activarlo no sube cpal de versión, pero mete
  un crate nuevo (va por rtkit/D-Bus en Linux) y rtkit mata el proceso si un
  hilo de tiempo real acapara la CPU. No hacerlo sin que Daniel lo pruebe
  tocando mientras separa una canción y mirando `OutputStatus.xruns`.

### AUD-13 · La carga abre las partes en serie y las lee enteras

C (la ganancia, S) · M · cuidado medio

- **Qué pasa.** `engine/src/engine.rs` (hacia 822-827) abre las partes una
  tras otra, y `decode.rs` (hacia 94-97) hace `std::fs::read` de cada una si
  pesa ≤ 24 MB. El tope es por parte: 32 partes × 24 MB son 768 MB por
  canción, y puede haber hasta tres juegos vivos (plato, fundido, relevo). En
  un teléfono eso es que el sistema mate la app.
- **Qué hacer.**
  1. `std::thread::scope` en `open_parts`, hasta 4 hilos.
  2. Presupuesto por canción (256 MB en el escritorio, 64 MB en el celular):
     las partes más grandes pasan a `Media::File` (lectura por trozos, que ya
     existe).
- **Medir** con `real_start_latency` (imprime «cargada en X ms») antes y
  después. **Prueba**: como `read_from_the_file_it_is_the_same`, comprobando
  qué partes quedan `Media::File`.

### AUD-14 · El clic se sintetiza muestra a muestra en el callback

C (el coste, estimado) · M · cuidado medio

`engine/src/click.rs` (hacia 138-161): por muestra y por voz, `exp` y `sin`
en f64 (seis parciales en el charles). Estimación: 60-150 µs por trozo de 256
con una voz. → Tabla `Arc<[f32]>` por (sonido, acento), calculada al mandar el
`ClickPlan` con el mismo `click_sample`. **Prueba**:
`tabla[i] == click_sample(i, …)` bit a bit. Hacerlo solo si un banco confirma
el coste.

### AUD-15 · El conversor de tasa se queda corto con fuentes de 96 y 192 kHz

S · S/M · cuidado medio · **cambia el sonido** (solo de fuentes ≥ 88,2 kHz)

`engine/src/resample.rs` (hacia 13-14): `HALF = 16` fijo con
`cutoff = 0.92 / step`. De 96 a 44,1 kHz la caída empieza hacia 12 kHz y
entra alias. → `HALF = ceil(16 * step.max(1.0))`. **La medida que sustituye
al oído**: un tono de 15 kHz a 96 kHz convertido conserva su amplitud a
±0,5 dB; uno de 30 kHz deja el alias por debajo de −60 dB. Escribir primero
la prueba y ver que hoy falla; si no falla, saltar el punto.

### AUD-16 · El C++ del estirador se compila siempre con -O2

S · S · cuidado bajo

`engine/build.rs`: `build.opt_level(2)` fijo, sin `NDEBUG`. →
`opt_level(3)` cuando la variable `OPT_LEVEL` sea `3`, y
`.define("NDEBUG", None)` en release. **Sin `-ffast-math`** (cambiaría bits).
Medir con la prueba de `stretch.rs`; si no gana un 5 %, dejarlo.

### AUD-17 · Copias de más al analizar

C · S cada una · cuidado bajo · todo bit a bit igual

- `analysis/src/separate.rs` (hacia 303 y 341): plan de FFT nuevo en cada
  `Stft::forward`/`inverse`. → Guardar los dos planes y `square` en `Stft`.
- `analysis/src/separate/process.rs` (hacia 175-187): `unpack` tiene vivas
  tres copias de la canción (~318 MB con 5 min). → Desintercalar directo de
  los bytes.
- `process.rs` (hacia 713-724, `run_max`): `melodic = wav.clone()` y dos
  `x.clone()` en `stems`. Si de verdad no se mutan hasta la línea ~800,
  compartir con `Rc<Vec<f32>>` (~212 MB menos).
- `analysis/src/structure.rs` (hacia 276-290) y `key.rs` (hacia 128-129):
  tres o cuatro `Vec` por trama y `fft.process` sin búfer de trabajo.

**Prueba.** Las que ya comparan la salida del separador y de la guía muestra
a muestra.

---

## Lote 2 — Seguridad

Modelo de confianza: la interfaz es «el usuario» (CSP estricta, sin XSS
encontrado). Las entradas hostiles reales son tres: archivos de fuera, texto
de la red y lo que decida el modelo de IA.

### SEG-1 · El asistente escribe sin pedir permiso tras leer texto de fuera

C el camino (que un modelo obedezca, S) · M · cuidado medio

- **Qué pasa.** `library/src/assistant/tools.rs:30`:
  `NEEDS_CONFIRMATION = ["delete_song", "delete_playlist", "download_music"]`.
  El resto de `ACTING_TOOLS` se ejecuta directo: `set_playlist_songs`,
  `remove_from_playlist`, `rename_playlist`, `edit_song` (escribe etiquetas
  en el archivo). Y `web_search` (`server/src/services/chat/lookups.rs`,
  hacia 15-26) entrega `title`, `url` y `summary` al modelo sin pasar por
  `library::safe::datum`; el título no tiene tope (`library/src/web.rs`
  hacia 132). Un resultado de búsqueda con una orden dentro podría dejar el
  repertorio del domingo con una sola canción o cambiar el artista de ocho.
- **Qué hacer.**
  1. En `web_search`: `title` por `library::safe::datum(…, 120)` y `summary`
     por `datum(…, 400)`.
  2. En el bucle de `library/src/assistant/conversation.rs` (hacia 597-600),
     una marca `read_outside` que se activa cuando en este turno haya
     contestado `web_search`, `search_youtube`, `get_lyrics`, `get_chords` o
     `find_lyrics_and_cover`.
  3. Con la marca puesta, `set_playlist_songs`, `remove_from_playlist`,
     `rename_playlist` y `edit_song` (si cambia algo más que estrellas o
     favorito) van por el camino de confirmación que ya usan las tres de hoy
     (`server/src/services/chat/mod.rs` hacia 569, `plan::register`).
  Decidido: `edit_song` de solo `year` o `genre` **también** pide permiso
  con la marca puesta (es lo simple y lo seguro; si molesta, se afina).
- **Prueba.** En `library/src/assistant/conversation/tests.rs`, con el `Io`
  de mentira: `web_search` devuelve un resumen con una orden y el modelo
  guionizado llama a `set_playlist_songs` → queda una confirmación registrada
  y la lista sin tocar. Y otra: sin lectura de fuera, la misma llamada se
  ejecuta como hoy.

### SEG-2 · La etiqueta de versión de yt-dlp se usa como carpeta sin validar

C · S · cuidado bajo

- **Qué pasa.** `server/src/services/ytdlp.rs` (hacia 983): `latest` sale
  del `tag_name` del JSON de GitHub, solo con `trim`. Más abajo (hacia
  1025-1032): `root.join(&latest)`, `remove_tree(&final_dir)` y `rename`. Un
  `tag_name` con `../` borraría un árbol fuera de su carpeta. Lo mismo al
  leer `current` (hacia 212, `marked`).
- **Qué hacer.** `valid_tag(&str) -> bool` en `library/src/ytdlp.rs`: solo
  `[0-9A-Za-z._-]`, de 1 a 40 caracteres, sin `..`. Aplicarla en `update`
  tras leer `tag_name` («GitHub dice una versión que no se entiende») y en
  `marked()` antes del `join`.
- **Prueba.** En `server/src/services/ytdlp/tests.rs`: `update` contra el
  servidor falso con `tag_name: "2099.1.1/../x"` falla y no toca nada fuera
  de `root`.

### SEG-3 · Un `.als` preparado puede tumbar la app

C · S · cuidado bajo

- **Qué pasa.** `library/src/sequence.rs` (hacia 256-290): `parse_xml` monta
  el árbol sin tope de profundidad ni de nodos; `descendants` (hacia
  230-236) y el `Drop` del árbol son recursivos. Decenas de miles de
  etiquetas anidadas (unos 100 KB en gzip) desbordan la pila, y el núcleo va
  dentro del proceso: se cae DanPlay entero. 64 MB de `<a/>` son más de 1 GB
  de memoria.
- **Qué hacer.** En `parse_xml`: devolver `broken()` si `stack.len() > 256`
  al hacer `push`, y contar nodos con un tope de 2 000 000.
- **Prueba.** Un XML con 100 000 niveles da el error «dañado» sin caerse;
  otro con 3 millones de `<a/>`, lo mismo.

### SEG-4 · El registro y los datos del WebView los leen otros usuarios

C (en este equipo) · S · cuidado bajo

`~/.local/share/local.danplay.app/logs/danplay.log` está en 664 dentro de
carpetas en 775; lleva rutas de la biblioteca y enlaces de YouTube (no
claves). Lo crea `tauri-plugin-log` con la umask del sistema. → En el `setup`
de `desktop/src-tauri/src/main.rs`, llamar a `library::config::private_dir()`
(ya existe, `config.rs` hacia 143) sobre `app.path().app_log_dir()` y
`app_data_dir()`, y poner 0600 a los `danplay*.log` que haya. **Prueba**
(solo Unix): tras la función, 0700 en la carpeta y 0600 en el archivo.

### SEG-5 · Los archivos con claves solo se cierran al escribirlos

C (en este equipo) · S · cuidado bajo

`~/.config/danplay/ai.json` (las claves de IA) está en **777** desde octubre
(herencia de la copia desde NTFS); hoy solo lo protege la carpeta en 0700.
`write_private` (`library/src/config.rs` hacia 160) corrige el modo solo al
guardar. → En `Config::load`, tras `private_dir`, un `tighten(path)` que
ponga 0600 a `danplay.env`, `ai.json` y `danplay.db` si existen (la misma
llamada que ya hace `migrate`, hacia 196). **Prueba** en
`library/src/config/tests.rs`: crear `ai.json` en 0666, llamar a `load`,
esperar 0600.

### SEG-6 · El servidor de desarrollo lee el cuerpo entero antes del tope

C · S · cuidado bajo · solo `danplay-server`, nunca la app instalada

- `server/src/serve.rs` (hacia 136): `body.collect().await` acumula lo que
  llegue; el tope de 1 MB / 4 MB se mira después (`guard.rs` hacia 78). →
  Rechazar con 413 si `Content-Length` supera `MAX_CHAT_BODY` y envolver el
  cuerpo en `http_body_util::Limited::new(body, MAX_CHAT_BODY)`.
- `serve.rs` (hacia 168): `http1::Builder::new()` sin plazo de cabeceras. →
  `.timer(TokioTimer::new()).header_read_timeout(Duration::from_secs(10))`.
- `guard.rs` (hacia 60): `host.rsplit_once(':')` rompe `Host: [::1]` sin
  puerto. → Quitar el puerto solo si lo de detrás de `:` son cifras y el
  texto no acaba en `]`.

**Prueba**: un POST de 5 MB a `/api/scan` da 413 sin leerlo entero;
`Host: [::1]` pasa.

### SEG-7 · La ventana de proyección tiene el puente entero

C · S · cuidado bajo

`desktop/src-tauri/permissions/windows.toml` (hacia 86) le da `allow-api`
(todos los métodos) y solo lee la letra. → Un comando `api_read` en
`core.rs` que solo admita `GET`, declarado en `build.rs` (`COMMANDS`), y
dárselo a `projection-window` en lugar de `allow-api`. La interfaz de la
proyección llama a `api_read`. **Prueba**: la que ya comprueba permisos por
ventana en `tests/test_frontend.mjs`, más una unitaria de que `api_read`
rechaza `POST`.

### SEG-8 · ffmpeg recibe rutas sin `file:` fuera del escritorio

C · M (mecánico, unos 20 sitios) · cuidado medio

`desktop/src-tauri/src/tools.rs` (hacia 145, `ffmpeg_input`) antepone
`file:`, pero `server`, `library` y `analysis` pasan la ruta tal cual tras
`-i` (p. ej. `server/src/services/cues.rs` hacia 529, `reuse.rs` hacia 249).
Las rutas del índice son absolutas, así que hoy no hay confusión de
protocolo; la excepción es `cues::import_recording` (hacia 603-604), que
acepta del cliente una ruta relativa. → Mover `ffmpeg_input` a
`library::processes` y usarla en todos los `-i`; exigir ruta absoluta en
`import_recording`. **Prueba**: una ruta llamada `concat:a|b` se trata como
archivo; en Windows, una `C:\…` sigue valiendo (la prueba del núcleo en
Windows corre en la CI).

### SEG-9 · Un salto de línea en una etiqueta rompe el `.m3u8` exportado

C · S · cuidado bajo

`server/src/services/playlists.rs` (hacia 250): artista y título van al
`#EXTINF` sin quitar saltos de línea. → `replace(['\r', '\n'], " ")`.
**Prueba**: un título con `\n` no añade líneas al archivo.

### Sin revisar a fondo (siguiente sitio si se quiere cerrar del todo)

- `library/src/html` y `chord_sheets.rs`: `fancy-regex` sobre páginas de
  hasta 64 MB; podría haber retroceso catastrófico. Buscar expresiones con
  cuantificadores anidados y probarlas con entradas largas.
- `library/src/lenient_json.rs`.
- Enlaces simbólicos dentro de las carpetas de música (el vigilante y el
  escaneo: ¿siguen un enlace a `/`?).
- ureq no reenvía `Authorization` a otro dominio al redirigir (es su valor
  por defecto según se recuerda; **no se comprobó** en su código). Las
  cabeceras propias de un perfil de IA sí viajarían.
- La puesta al día de yt-dlp comprueba el sha256 contra el `SHA2-256SUMS`
  de la misma versión: eso es integridad, no autenticidad (no se verifica la
  firma). Aceptable; apuntarlo en `ARCHITECTURE.md`.

---

## Lote 3 — El núcleo, rápido

Crates: `server/`, `library/`, `desktop/src-tauri/src/core.rs` y `queue/`.
Con 324 canciones ninguna consulta es lenta: lo que cuesta hoy son las
escrituras y dos redundancias del arranque. La segunda mitad solo importa a
escala (20 000 canciones).

### NUC-1 · Cada guardado del estudio reescribe el archivo de audio entero

C (el coste, estimado) · M · cuidado **alto**

- **Qué pasa.** `server/src/services/songs.rs` (hacia 227-247, `set_study`):
  `by_id` (`SELECT *`), `update_one`, `tags::write_fields`, `by_id` otra vez.
  `library/src/tags.rs` (hacia 730-739) lee el archivo entero y lo guarda
  entero. Después `carry` y `reindex` (`server/src/services/tags.rs` hacia
  139-170). La interfaz lo llama cada 0,6 s de cambios. Un FLAC de 40 MB o un
  disco USB: de 100 ms a segundos, repetido mientras se arrastra un tramo.
  Estrellas y favorito hacen lo mismo (hacia 252-271), pero son un toque
  suelto.
- **Qué hacer.** La base al momento; la etiqueta del estudio, aplazada.
  1. `pending_study: Mutex<HashMap<i64, (String, Instant)>>` en `State`.
  2. Un hilo (o el del diario) escribe la etiqueta tras ~10 s sin cambios de
     esa canción, y en `startup::stop` se vacía todo.
  3. La respuesta lleva `tags_written: false` y un campo nuevo
     `tags_pending: true`. La interfaz usa `tags_written`/`tags_skipped`
     para avisar: con `tags_pending` no debe avisar de nada.
  4. Cambiar de canción o cerrar el estudio vacía lo pendiente de esa
     canción (que no se quede 10 s en el aire si se apaga el equipo; el
     diario ya cubre la pérdida).
- **Prueba.** Cinco `PUT /study` seguidos: la fecha del archivo cambia una
  sola vez tras el vaciado; `stop()` vacía; la ficha devuelve el estudio
  nuevo desde el primer `PUT`.

### NUC-2 · Una conexión nueva a la base por cada consulta

C (el efecto en WAL, S: medir con MED-3) · S y M · cuidado medio

- **Qué pasa.** `server/src/app.rs` (hacia 157-169): `connect()` abre
  siempre y `db()` cierra al acabar. Cada apertura registra la colación,
  `busy_timeout` y `synchronous` (`library/src/index.rs` hacia 43-62). La
  caché de sentencias preparadas (`set_prepared_statement_cache_capacity`)
  no sirve de nada: muere con la conexión. Y la sospecha: en WAL, al cerrarse
  la **última** conexión SQLite hace punto de control con `fsync` y borra
  `-wal` y `-shm`; como casi nunca hay otra abierta, cada escritura acabaría
  esperando al disco, que es justo lo que `synchronous=NORMAL` quería
  evitar.
- **Qué hacer**, en dos pasos:
  1. (S) Un `keeper: Mutex<Option<Connection>>` en `State`, abierto en
     `startup::prepare_index` y soltado en `stop`: así nunca hay «última
     conexión». Reabrirlo si la base se borró y se recreó. **Ojo en
     Windows**: una base abierta no se puede borrar (la prueba
     `with_the_db_lost_study_comes_back_from_the_file`,
     `server/src/services/library.rs` hacia 453): soltar el `keeper` donde
     el código borre la base.
  2. (M, solo si MED-3 muestra que aún compensa) Conexión por hilo en
     `db`/`db_write` (`thread_local!` con `RefCell<Option<(PathBuf,
     Connection)>>`, y conexión nueva si ya está prestada).
- De paso (S): `server/src/routes/library.rs` (hacia 237-240) abre dos
  conexiones por búsqueda (`search` y `count`): juntarlas en un solo `db`. Y
  `PRAGMA table_info(songs)` se repite en cada búsqueda (`search.rs` hacia
  138): guardarlo por base en un `OnceLock`, invalidado en `prepare`.
- **Prueba.** MED-3 antes y después, y todas las de `api_tests`.

### NUC-3 · A los 2 s de arrancar, la interfaz lo recarga todo otra vez

C · S · cuidado bajo

- **Qué pasa.** `desktop/src-tauri/src/core.rs` (hacia 274-303): `watch`
  duerme `WATCH_EVERY` (2 s) **antes** de su primera comprobación y entonces
  emite `READY {ready: true}`. En `desktop/src/App.vue` (hacia 440 y
  992-998) `coreWasReady` empieza en `false`, así que ese aviso lanza
  `refreshAll()` y `termsAtStart()` aunque `library.start()` ya cargó:
  estado, Entrada, lista entera y repertorios dos veces.
- **Qué hacer.** En Rust: un `served: AtomicBool` en `Core` que `api` pone a
  `true` en su primera respuesta correcta. En `watch`, si
  `!was_ready && core.served()`: `was_ready = true` y `prune_queue` **sin**
  emitir `READY`. Y mover el `sleep` al final del bucle: un núcleo que tarda
  se anuncia al momento y no a los 2 s.
- **Prueba.** Unitaria de la decisión (como `health_due`). En la interfaz
  (`tests/app.test.js`): contar llamadas a `/search` en el arranque.

### NUC-4 · El catálogo de modelos (2,9 MB) se analiza en cada arranque

C (tamaño medido) · S/M · cuidado bajo

`server/src/startup.rs` (hacia 316-325) llama a `catalog::load`, que lee y
analiza `models-catalog.json` entero (`library/src/ai/catalog.rs` hacia
262-300) y lo retiene para siempre, solo para mirar `checked_at`: 20-40 ms
de CPU compitiendo con el primer pintado y 10-15 MB residentes aunque nunca
se abra la IA (estimación). → Guardar `checked_at`, `etag`, `source` y
`error` en un `models-catalog.meta.json` pequeño que escriben
`store_trimmed` y `mark_checked`; `refresh_catalog_in_background` y
`catalog_status` leen solo eso; el catálogo entero se carga al primer uso
real. Sin el archivo pequeño, el comportamiento de hoy. **Prueba**:
`start()` no llena `LOADED` (hace falta un `catalog::is_loaded()` solo para
pruebas).

### NUC-5 · El diario rehace y reescribe la foto entera tras cualquier cambio

C · S (los dos primeros) y M · cuidado bajo

`library/src/journal.rs` (hacia 173-178) lee `study`, `chords` y
`lyrics_synced` de toda la biblioteca; `body()` y `text()` (hacia 308-317)
clonan el diario entero dos veces; `server/src/services/journal.rs` (`write`)
abre 4 conexiones y guarda el cuerpo anterior entero en `Inner.written`. Hoy
son 44 KB; con 20 000 canciones serían decenas de MB por cada estrella,
sobre el disco de la música.

1. (S) `body(&Dict)` sin clonar (serializar saltando `updated`), `text` con
   `to_writer_pretty`, y guardar en `written` un sha1 en vez del texto.
2. (S) Una sola conexión para `roots`, `uid` y `snapshot`.
3. (M, solo a escala) No releer lo pesado si no cambió: un contador aparte
   para las escrituras de `study`/`chords`/`lyrics_synced`.

**Prueba**: el archivo escrito es idéntico byte a byte al de antes (el
formato no cambia). Banco `#[ignore]` con 20 000 filas para el punto 3.

### NUC-6 · Servir una miniatura ya hecha lee la ficha entera y escribe en disco

C · S · cuidado bajo

- `server/src/routes/songs.rs` (hacia 467): `songs::resolve` hace
  `SELECT *` (con letra, cifrado y estudio) para usar solo `path`; lo mismo
  en `audio_path` (hacia 728). → `songs::path_of(app, id) -> Option<String>`
  (`SELECT path`).
- `server/src/services/media.rs` (hacia 184-188): `private_dir`
  (`create_dir_all` y `chmod`) en cada petición. → Una vez por ruta
  (`OnceLock`), o solo al fallar la lectura.
- `library/src/thumbnails.rs` (hacia 120-131): `set_times` en cada acierto,
  una escritura de inodo por carátula pintada. → Tocar la fecha solo si la
  que tiene pasa de un día.

**Prueba**: ampliar la que usa `cover_reads()`; la fecha no cambia entre dos
lecturas seguidas.

### NUC-7 · `/api/status` lleva los resultados de los trabajos y nadie los lee

C · S · cuidado bajo

`server/src/routes/system.rs` (hacia 67): `"jobs": app.jobs.snapshot_all()`,
que clona el `result` de cada trabajo terminado durante 10 min
(`server/src/jobs.rs` hacia 77-89): el informe de duplicados o de una
importación, entero, en cada estado. En `desktop/src` nadie lee
`status.jobs` (se usa `GET /api/jobs/{name}`). → `snapshot_all` sin `result`
(un `Job::summary()`). Ajustar las dos pruebas que lo miran
(`api_tests/jobs.rs` hacia 20, `lib.rs` hacia 314) y
`docs/INTERNAL-CONTRACT.md` si lo nombra.

### NUC-8 · El escaneo bloquea las escrituras aunque no haya nada que apartar

C · S · cuidado bajo

- **Qué pasa.** `library/src/index.rs` (hacia 239-243): `to_missing` llama a
  `begin(conn)?` (`BEGIN IMMEDIATE`) antes de mirar si `paths` está vacío.
  `scan.rs` (hacia 701) lo llama siempre y el `COMMIT` llega mucho después
  (hacia 846), tras el `stat` de cada archivo. Un escaneo sin cambios (cada
  10 min y al arrancar) tiene cogido el turno de escritura todo el
  recorrido: con el disco frío o USB, poner una estrella espera hasta 5 s y
  falla con «database is locked».
- **Qué hacer.** `if paths.is_empty() { return Ok(0); }` al principio de
  `to_missing`. **No** adelantar el `COMMIT` cuando sí hay apartadas: el
  comentario de `scan.rs` (hacia 692-695) explica por qué va junto con la
  primera tanda. Comprobar que quien viene después abre su transacción él
  mismo cuando `to_missing` no la abrió.
- **Prueba.** Una segunda conexión con `busy_timeout(0)` escribe durante un
  escaneo sin cambios (hace falta un gancho en `progress`).

### NUC-9 · La lista a escala

C (magnitudes estimadas) · por pasos · cuidado medio. Solo se nota con miles
de canciones. **Empezar por MED-4** y decidir con sus números.

1. (S) `desktop/src-tauri/src/core.rs` (hacia 351):
   `String::from_utf8_lossy(&bytes).to_string()` copia toda respuesta. →
   `String::from_utf8(bytes)`, y el `lossy` solo si falla.
2. (S) `light_columns` por base en un `OnceLock` (ver NUC-2).
3. (S, con la interfaz) `desktop/src/composables/useLibrary.js` (hacia 20 y
   43): página de 400 y luego de 5 000: cinco ordenaciones completas por
   carga con 20 000 canciones (`ORDER BY … COLLATE SPANISH LIMIT ? OFFSET ?`
   no tiene índice que sirva, `search.rs` hacia 310-316). → Segunda página
   de 20 000 (el servidor ya lo admite): dos ordenaciones.
4. (S, medir) `PRAGMA temp_store=MEMORY` en `open()`.
5. (M, solo si el banco lo pide) `search_json(conn, …) -> String` que
   escriba el JSON fila a fila desde `rusqlite::Row`, sin el árbol `Value`
   (hoy unas 600 000 `String` de claves por carga, `index.rs` hacia 104-111).

### NUC-10 · Borrar miniaturas es cuadrático

C · S · cuidado bajo

`library/src/thumbnails.rs` (hacia 228-246): `forget` lista la carpeta
entera **una vez por canción**. Un disco con 20 000 canciones que se
desmonta: 20 000 listados de hasta 60 000 archivos, con el cerrojo del
escaneo cogido. `store` (hacia 196-207) lista la carpeta por cada miniatura
nueva. → En `forget`, un solo `entries()` y un `HashSet` de claves. En
`store`, borrar las versiones viejas por nombre solo si `stored()` falló con
otra versión, o dejarlo a `prune`. **Prueba**: ampliar el banco
`pruning_the_caches_of_a_big_library_costs_this_much` con un `forget` de
5 000 rutas; y una normal de que se borran las de esas canciones y solo
esas.

### NUC-11 · Menores, todos C y S

- `library/src/index/songs.rs` (hacia 69-100, `paths_of`): búsqueda lineal
  por id dentro de un bucle, O(n²). → `HashMap<i64, usize>`.
- `desktop/src-tauri/src/queue/worker.rs` (hacia 124-129): compara la cola
  entera (`guard.0 != inner.items`) en cada mensaje, 4 veces por segundo. →
  Comparar `inner.revision`.
- `library/src/index.rs` (hacia 383, `STATS_TTL` de 5 s) con `core.rs`
  (hacia 255, estado entero cada 30 s): el agregado con
  `SUM(lyrics != '')` se recalcula cada 30 s en reposo. → Invalidar por
  `writes()` y subir el TTL.
- `library/src/names.rs` (hacia 573-576, `artist_pattern`): compila tres
  expresiones regulares por archivo sin artista. → Caché
  `HashMap<String, Arc<Regex>>`.
- `MISSING_DAYS` duplicada (`server/src/services/library.rs` hacia 13 e
  `library/src/index/scan.rs` hacia 29): dejar una.

### NUC-12 · Un ffmpeg por miniatura nueva, sin tope

S · S · cuidado bajo

`media::thumbnail` lanza un ffmpeg por cada miniatura que falta. Al
desplazar una biblioteca sin miniaturas pueden ser decenas a la vez. →
Semáforo con el número de núcleos alrededor de `shrink_bytes`. Medir antes:
desplazar una biblioteca de prueba con las miniaturas borradas y contar
procesos.

### NUC-13 · El servidor de desarrollo viaja dentro de la app

C · S/M · cuidado medio (CI)

`server/Cargo.toml`: `hyper`, `hyper-util`, `http-body-util`, `bytes` y
`tokio` con `rt-multi-thread`, `net` y `signal` entran en la app solo por
`serve.rs` y `main.rs`, que la app no llama. → Un rasgo `serve` (por
defecto, y `required-features` del binario) y `default-features = false`
desde `desktop/src-tauri/Cargo.toml`. Sin efecto al ejecutar; ahorra
compilación y tamaño. Comprobar que `cargo test --workspace`, las e2e (que
usan el binario) y `cargo check --target x86_64-pc-windows-gnu` siguen bien.
Medir el tamaño de `danplay-app` antes y después.

### NUC-14 · La sesión entera a disco en cada cambio de canción

C · M · cuidado medio · **solo si se mide**

`queue/worker.rs` (hacia 143-147): cada cambio de canción serializa la cola
entera con `sync_all` (en otro hilo desde la 1.42, así que no retrasa nada).
Con una cola de miles, separar la posición (archivo pequeño) de la lista.
No hacerlo sin una medida que lo justifique.

---

## Lote 4 — La interfaz, fluida

Todo en `desktop/`. La base está cuidada (listas virtualizadas, `shallowRef`
en lo grande, oyentes con su limpieza). Lo que queda es **un patrón repetido
en tres sitios**: la posición del reproductor (el evento `danplay://state`,
cada 250 ms) se lee en plantillas grandes. El arranque tiene poco que rascar:
el JS inicial son 354 KB y lo recortable ronda el 10 %.

Nota de honestidad: el 16-nov se midió la onda a 60 fps con la CPU frenada
×4, así que esto no es un cuelgue que se vea; es trabajo tirado (CPU,
batería en el celular) y una aguja que avanza a saltos de 250 ms.

### INT-1 · El estudio entero se repinta 4 veces por segundo, también oculto

C · M · cuidado **alto** (lo comparte el celular)

- **Qué pasa.** `src/components/StudyBar.vue`: `{{ formatTime(position) }}`
  (hacia 2983) y `:position="position"` a `StudyTimeline` y `BeatTools`
  (hacia 3168 y 3226). `StudyTimeline.vue`: las agujas con
  `:style="{ left: pct(position) }"` (hacia 1954, 2243, 2478, 2517). Cada
  aviso vuelve a ejecutar las ~1 340 líneas de plantilla de StudyBar y las
  ~760 de StudyTimeline (carriles, hasta 600 pulsos, 400 marcas de regla,
  `cueWavePath(c)` por guía sin memorizar). `StudyBar.vue` crea además un
  objeto nuevo por pintada (`:click-lane="{…}"`, hacia 3186) y un `[]`
  (`:hits`, hacia 3180). Y `App.vue` (hacia 1731-1737) lo tiene con
  `v-if="studyActive" v-show="studyOpen"`: tras «Ocultar», el repintado
  sigue toda la sesión.
- **Qué hacer.**
  1. Nuevo `src/composables/usePlayhead.js`: guarda la última posición y el
     instante en que llegó, y pinta por `requestAnimationFrame`:

     ```js
     export function usePlayhead(paint, active = () => true) {
       const { state } = usePlayback()
       let base = 0, stamp = 0, frame = 0
       const now = () => state.playing
         ? Math.min(state.duration || Infinity,
             base + Math.min(0.3, (performance.now() - stamp) / 1000) * state.speed)
         : base
       const step = () => {
         frame = 0
         if (active()) paint(now())
         if (state.playing && active()) frame = requestAnimationFrame(step)
       }
       watch(() => [state.position, state.playing], () => {
         base = state.position; stamp = performance.now()
         if (!frame) frame = requestAnimationFrame(step)
       }, { immediate: true, flush: 'sync' })
       onScopeDispose(() => cancelAnimationFrame(frame))
       return { now, repaint: () => paint(now()) }
     }
     ```

     El tope de 0,3 s evita que la aguja se escape si Rust calla. Con
     tramos, recortar también al final del tramo en curso
     (`segmentAt(state.loops, base)`).
  2. `StudyTimeline`: quitar la prop `position`. Las agujas pasan a
     `useTemplateRef` y `paint(t)` les pone `style.left = pct(t)` y
     `hidden`. Llamar a `repaint()` en el `watch` de `[view.from, view.to]`
     (hacia 590). El «seguir a la aguja» (hacia 323-332) va dentro de
     `paint`. `zoomBy` y `zoomToBeats` usan `now()`. Prop nueva `visible`
     para `active`.
  3. `StudyBar`: el reloj a un `StudyClock.vue` de diez líneas que lee
     `usePlayback()`. Quitar `:position` de `BeatTools` (ya lee
     `player.position.value` en su manejador, `BeatTools.vue` hacia 133).
     `clickLane` a `computed`; el `[]` a una constante de módulo.
  4. Revisar `src/mobile/MobileApp.vue` (hacia 897), que usa lo mismo.
- **Prueba.** `tests/ticks.test.js` ya hace esto para la app y el chat:
  añadir un caso con el estudio abierto que guarde
  `wrapper.findComponent(StudyTimeline).vm.$.subTree`, emita 20 avisos de
  posición y espere la misma identidad; lo mismo para StudyBar. Y MED-2
  antes y después. Al final, las e2e del estudio (`study.spec.js`,
  `study-beats.spec.js`, `beat-edit.spec.js`).

### INT-2 · La onda repite trabajo en cada pintada

C (cuánto, medir con MED-2) · S · cuidado bajo

`StudyTimeline.vue`, función `draw()`:

- (hacia 590-597) un `draw()` por cada cambio de vista; un pellizco manda
  varios `wheel` por fotograma. → `scheduleDraw()` con
  `requestAnimationFrame` para los `watch` y el `ResizeObserver` (hacia
  781).
- (hacia 642-643) `c.width = …; c.height = …` siempre, lo que reasigna el
  lienzo. → Solo si cambió el tamaño.
- (hacia 649 y 754) `getComputedStyle` dos veces por pintada. → Guardar
  `text` y `accent` hasta el `watch([theme, waveStyle])` (hacia 600).
- (hacia 670) `Math.max(1e-6, ...(w.rms || [0]))` en cada pintada. →
  Guardar `rmsTop` al cargar la onda y al cambiar `laneWaves`.
- (hacia 606-630) `columnsOf` crea un array por columna y por carril.

Protegen `tests/wave-paint.test.js` y `tests/timeline-detail.test.js`.

### INT-3 · «Grande» anima la altura y arrastra medir y pintar en cada fotograma

C el mecanismo, S la gravedad · S · cuidado bajo

`src/styles/study.css:36` (`transition: height 0.3s`); el `ResizeObserver`
de StudyBar (hacia 2612-2619) llama a `fitTimeline()` y cambia `waveRoom`,
y el de StudyTimeline repinta: unos 18 ciclos en 300 ms. → No medir mientras
dura la transición (bandera puesta al cambiar `big`, y `fitTimeline()` en
`transitionend`), o quitar la transición de `height`. Protege
`tests/study-fit.test.js`.

### INT-4 · `getBoundingClientRect` en bucle al mover el puntero

C · S · cuidado bajo

`StudyTimeline.vue` (hacia 811-814): `xOf` mide en cada llamada, y `snap`
(hacia 926-931) y `hitAt` (hacia 938-951) lo llaman en bucle desde `onMove`
y `onHover`. → Medir una vez por manejador y pasar `rect` (como ya hace
`begin()`, hacia 1194).

### INT-5 · La cola entera, sin virtualizar, dentro de un reproductor que se repinta

C · S/M · cuidado medio

`src/components/Player.vue` (hacia 291-316): `v-for="(song, i) in queue"`
con una función `:ref` nueva por fila; la cola puede ser la lista entera, y
`Player` lee la posición. Con «Ver la cola entera» abierta y sonando, cada
aviso recrea N filas. → Sacar el panel a `PlayerQueue.vue` (props `queue`,
`currentId`; emite `jump`, `close`), que ya no depende de la posición, y
dentro `useVirtualRows` (o, como mínimo, una ventana de ±150 alrededor de la
que suena). Protege `tests/player.test.js`.

### INT-6 · La ficha se repinta en cada aviso con la letra sincronizada

C · S · cuidado bajo

`src/components/DetailsPanel.vue:775`:
`:position="player.position.value"`. → En `SyncedLyrics.vue`, `position`
opcional y
`const pos = computed(() => props.position ?? player.position.value)`;
`DetailsPanel` deja de pasarla. `tests/lrc.test.js` sigue valiendo con la
prop.

### INT-7 · La lista agrupada se agrupa y ordena dos veces

C el duplicado, S el coste · S · cuidado bajo

`App.vue` (hacia 260, `groupedOrder`) y `components/GroupedSongs.vue`
(hacia 49, `groupSongs`), las dos con `localeCompare(…, 'es')`
(`utils/groups.js` hacia 48), en cada página que llega y en cada
`patchSong`. → `const collator = new Intl.Collator('es')` a nivel de módulo;
`App` calcula `groups` una vez y lo pasa por prop. Protege
`tests/grouped.test.js`.

### INT-8 · Peticiones repetidas al núcleo

C · S · cuidado bajo

- Al poner una canción, `/song/{id}` se pide hasta tres veces
  (`useSelection.js` hacia 93, `useClick.js` hacia 224 y 252, o
  `StudyBar.vue` hacia 226). → En `src/api.js` (hacia 1121), deduplicar en
  vuelo:

  ```js
  const flying = new Map()
  song: (id) => {
    let p = flying.get(id)
    if (!p) { p = GET(`/song/${id}`).finally(() => flying.delete(id)); flying.set(id, p) }
    return p.then((s) => (s ? { ...s } : s))
  }
  ```

- `refreshAll()` (`App.vue` hacia 475) lanza `loadStatus()` y `load(true)`,
  y los dos piden `/status` (`useLibrary.js` hacia 127 y 245). → Un
  `library.refresh()` que pida `status` una vez y haga
  `applyStatus(e); load(true, e)`.

Protegen `tests/api.test.js` y `tests/song-change.test.js`.

### INT-9 · Carga inicial: tres recortes pequeños

Todos C · S · cuidado bajo

- **driver.js viaja en el trozo del estudio.** `StudyBar.vue:45` importa
  `useStudyTour.js`, y este `driver.js` y su CSS: 27,7 KB de los 240 KB,
  para algo que sale una vez. Y `styles/tour.css` (7,4 KB) va en el
  `index.css` global. → `tourSeen()` y `TOUR_KEY` a
  `src/composables/tourSeen.js`; en `tour()` (hacia 2539),
  `const m = await import('../composables/useStudyTour.js')`; mover
  `tour.css` de `index.css` a un import en `useStudyTour.js` después de
  `driver.css`. Vigilar `tests/css-layers.test.js`.
- **Las novedades viajan en el arranque.** `App.vue:70` importa
  `compareVersions` de `data/releases.js` y arrastra las 301 líneas de
  novedades (10,8 KB). → Mover `compareVersions` a `src/utils/version.js`.
  Protege `tests/about.test.js`.
- **Los oyentes del arranque, en fila y detrás de la biblioteca.**
  `App.vue` (hacia 1002-1058): cinco `await` seguidos (`onExternal`,
  `onSequence`, `onBundle`, `takeOpenedBundles`, `onChanged`) después de
  `library.start()`. Un `.danpista` abierto al lanzar espera a que cargue la
  lista. → Registrarlos con un `Promise.all` antes de `library.start()`.
  Protege `tests/app.test.js`.

### INT-10 · Datos grandes en `ref` profundos

C uno, S el otro · S · cuidado medio

- `StudyBar.vue:1611`: `const cueWaves = ref({})` guarda los picos de cada
  guía y se sustituye entero. → `shallowRef`.
- `StudyBar.vue:115`: `study = ref(blank())` hace reactivo
  `metronome.own.beats` (miles de números). → `markRaw(own)` donde se asigna
  (`useBeatEdit.js` hacia 471, `StudyBar.vue` hacia 239 y 1168). **Antes**,
  comprobar que nadie lo muta en sitio (parece que siempre se sustituye); si
  alguien lo muta, saltar.

Protegen `tests/beat-tools.test.js` y `tests/timeline-edit.test.js`.

### INT-11 · Una caché sin tope y una escritura por aviso

C · S · cuidado bajo

- `src/composables/useBeatEdit.js:589`: `hitsCache = new Map()` sin tope
  (los golpes de cada canción editada, miles de pares), y StudyBar vive toda
  la sesión. → Dejar las 4 últimas.
- `src/composables/usePlayback.js` (hacia 555-558): `applyState` escribe
  volumen y repetición en `localStorage` en cada aviso, también en el mini y
  la proyección (contradice su propio comentario, hacia 589-593). →
  Recordar el último valor escrito y exigir `isMainWindow()`.

### INT-12 · El botón «ir a lo que suena» puede quedarse puesto

S · S · cuidado bajo

`src/composables/useHasScroll.js` (hacia 79-85) observa un elemento
`.playing` que la lista virtual recicla; al volver a la vista es otro nodo.
Reproducir primero en `e2e/virtual.spec.js`: bajar, volver, y esperar que
`.jump-fab` desaparezca. Si no falla, saltar.

### INT-13 · El CSS de las páginas perezosas va en el global

C · M · cuidado medio · prioridad baja

De unos 118 KB de `index.css`, unos 43 KB son de páginas que se cargan
después (`ai` 8,1; `list-cards` 8,1; `pocket` 5,6; `about` 5,2; `tidy` 3,1;
`selection-bar` 2,9; `list-rows` 2,6). El orden importa
(`styles/index.css:1-3`): mover solo los que `tests/css-layers.test.js` no
marque con choques. Ganancia de milisegundos: hacerlo solo si sobra tiempo.

Y al revés: `src/mobile/main.js` (hacia 7-14) carga `index.css`,
`study.css`, `mini.css` y `mobile.css`: 186 KB de CSS en el teléfono, y de
`drag`, `selection-bar`, `selection`, `tidy`, `about`, `list-rows` y
`pocket` no usa casi nada. → Un `index-mobile.css` sin ellos. Comprobar con
fotos del celular antes y después.

### INT-14 · Cada build lleva el código del otro

S (la ganancia) · M · cuidado medio

`isMobileApp` es de ejecución (`src/platform.js:9`, lee el `dataset`): 43
ramas en StudyBar y 13 en StudyTimeline que ningún build suelta. → Con
`import.meta.env.MODE === 'mobile'` (dejando el `dataset` solo en modo
`test`) cada build tiraría lo del otro. **Medir primero**: compilar los dos
con y sin el cambio y comparar el tamaño de `StudyBar-*.js`. Si gana menos
de un 10 %, dejarlo.

### INT-15 · vue-router solo hace de cargador perezoso

C · M · cuidado medio · **no recomendado ahora**

26,6 KB (7,5 % del JS inicial) para algo que hace `view` a mano
(`App.vue` hacia 350 y 1407-1415). Se podría quitar, pero toca
`tests/pages.test.js` y `tests/islands.test.js` y gana poco. Se deja
apuntado.

---

## Lote 5 — CI y cadena de suministro

Solo `.github/` y `scripts/`. **Nada de este lote se puede validar en
local**: se prepara en una rama y Daniel decide cuándo subirla. Lo que toca
la publicación (CI-2) se prueba primero con una etiqueta de prueba en un
repositorio aparte o con `workflow_dispatch`, nunca con una `v*` de verdad.

### CI-1 · 44 de 59 acciones van por etiqueta móvil

C · S · cuidado medio

- **Qué pasa.** Fijadas por SHA solo `oven-sh/setup-bun` y
  `voidzero-dev/setup-vp`. Sin fijar: `actions/checkout@v7`,
  `dtolnay/rust-toolchain@stable`, `Swatinem/rust-cache@v2`,
  `actions/cache@v6`, `actions/upload-artifact@v7`,
  `taiki-e/install-action@v2`, `softprops/action-gh-release@v3`,
  `actions/setup-java@v6`, en los cinco flujos. Lo más serio es
  `android.yml`: la llave de firma se escribe en
  `$RUNNER_TEMP/keys/danplay.jks` (hacia la línea 91) y sigue en disco
  mientras corren acciones de terceros con etiqueta móvil.
- **Qué hacer.**
  1. Cada `uses:` a `owner/repo@<sha de 40> # vX.Y.Z`. El SHA se saca con
     `gh api repos/OWNER/REPO/commits/TAG --jq .sha` (pide red: lo hace
     quien suba la rama, o `pinact run`). Dependabot ya vigila
     `github-actions` y mantiene los SHA con su comentario.
  2. `persist-credentials: false` en todos los `actions/checkout`.
  3. En `android.yml`, tras construir el APK, un paso `if: always()` con
     `rm -rf "$RUNNER_TEMP/keys"`.
- **Comprobar.**
  `/usr/bin/grep -n "uses:" .github/workflows/*.yml | /usr/bin/grep -vE "@[0-9a-f]{40}"`
  no devuelve nada.

### CI-2 · Permiso de escritura para todo el flujo de paquetes

C · M · cuidado **alto** (es la publicación)

- **Qué pasa.** `linux.yml:20-21`, `windows.yml:24-25`, `macos.yml:27-28`,
  `android.yml:25-26`: `permissions: contents: write` a nivel de flujo, con
  un solo trabajo. Con ese permiso corren `vp install`, los `build.rs`, los
  binarios bajados y las acciones de CI-1. No se publican sumas ni
  procedencia.
- **Qué hacer.** `permissions: contents: read` en los cuatro, y un trabajo
  aparte que solo publica:

  ```yaml
  publish:
    needs: packages
    if: startsWith(github.ref, 'refs/tags/v')
    runs-on: ubuntu-latest
    timeout-minutes: 10
    permissions: { contents: write, id-token: write, attestations: write }
    steps:
      - uses: actions/download-artifact@<sha>
        with: { name: danplay-linux, path: out }
      - run: cd out && find . -type f -exec mv {} . \; && sha256sum * > SHA256SUMS-linux.txt
      - uses: actions/attest-build-provenance@<sha>
        with: { subject-path: 'out/*' }
      - env: { GH_TOKEN: '${{ github.token }}' }
        run: |
          gh release view "$GITHUB_REF_NAME" -R "$GITHUB_REPOSITORY" >/dev/null 2>&1 \
            || gh release create "$GITHUB_REF_NAME" -R "$GITHUB_REPOSITORY" --generate-notes
          gh release upload "$GITHUB_REF_NAME" out/* -R "$GITHUB_REPOSITORY" --clobber
  ```

  Con eso sobra `softprops/action-gh-release`. Mejor aún (M): un
  `release.yml` con `on: push: tags` que llame a los cuatro como
  `workflow_call` y un único `publish` con un solo `SHA256SUMS` de los siete
  archivos; evita que cuatro flujos creen la misma versión a la vez.
- **Comprobar** tras una publicación:
  `gh attestation verify DanPlay_X_amd64.deb -R dani17r/danplay`.

### CI-3 · La etiqueta no se coteja con la versión

C · S · cuidado bajo

`git tag v1.44.2` sobre un árbol en 1.44.1 publicaría en «v1.44.2» archivos
llamados 1.44.1. Primer paso de cada flujo de paquetes:

```yaml
- name: la etiqueta es la versión
  if: startsWith(github.ref, 'refs/tags/v')
  shell: bash
  run: |
    v=$(sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml | head -1)
    [ "v$v" = "$GITHUB_REF_NAME" ] || { echo "::error::etiqueta $GITHUB_REF_NAME, Cargo.toml $v"; exit 1; }
```

### CI-4 · Sin `rust-toolchain.toml`: la CI usa la estable del día

C · S · cuidado bajo

Ya rompió una vez por un lint nuevo de clippy (el contenedor en 1.99.1, la
CI en 1.100.0). → `rust-toolchain.toml` en la raíz:

```toml
[toolchain]
channel = "1.NN.0"      # la que se decida (el 14-nov la CI iba con 1.100.0 y el contenedor con 1.99.1)
components = ["clippy", "rustfmt"]
```

En los flujos, sustituir `dtolnay/rust-toolchain@stable` por
`run: rustup show active-toolchain || rustup toolchain install` (más
`rustup target add …` donde haga falta: `android.yml` y el `desktop` de
`tests.yml`). Subirla a propósito, una vez al mes. **Ojo local**: el
contenedor `danplay-dev` monta `~/.rustup`; con el archivo, `cargo` pedirá
esa versión exacta: instalarla antes
(`rustup toolchain install 1.NN.0 -c clippy,rustfmt`) o la compilación local
intentará bajarla.

### CI-5 · Un commit de documentación lanza los siete trabajos

C · S · cuidado bajo

`tests.yml` no tiene `paths-ignore`. **No vale ignorar `**/*.md`**:
`desktop/tests/about.test.js` lee `CHANGELOG.md` y
`library/src/repo_checks.rs` lee `README.md` y `docs/CONTRIBUTING.md`.

```yaml
on:
  push:
    branches: [main]
    paths-ignore: ['docs/**', '!docs/CONTRIBUTING.md', 'LICENSE', '.markdownlint.jsonc']
  pull_request:
    paths-ignore: ['docs/**', '!docs/CONTRIBUTING.md', 'LICENSE', '.markdownlint.jsonc']
```

### CI-6 · El núcleo se compila ocho veces por subida, en cuatro perfiles

C · S y M · cuidado medio

`core`: clippy en dev y pruebas en release. `desktop`: clippy ×3, pruebas
release ×2 y `check` para Windows. `e2e`: build en debug. Más Windows y
macOS.

1. (S) Clippy con el mismo perfil que las pruebas (`--release`, o
   `--profile ci` si se hace DEP-3), como ya hace `scripts/test.sh`: los
   `build.rs` (SQLite, el C++ del estirador) y las macros se hacen una vez.
2. (S) `cargo build --profile ci -p danplay-server` en `e2e`, y añadir
   `target/ci` a lo que busca `desktop/e2e/server.mjs` (hacia 33-34).
3. (M) Fundir `core` y `desktop` de Linux en un trabajo con `--workspace`,
   más un `cargo check -p danplay-analysis` para no perder la configuración
   de `engine` sin su rasgo.

Ganancia estimada: 25-40 % del tiempo de Rust por subida. Medir en «Usage»
de una ejecución antes y después.

### CI-7 · Las cachés de Rust se guardan donde no se reutilizan

C · S · cuidado bajo

Los flujos de paquetes corren en etiquetas y guardan caché; una caché de
`refs/tags/vX` no la restaura ni otra etiqueta ni `main`: ocupa parte de los
10 GB y desaloja las útiles. → En todos los `Swatinem/rust-cache`:
`save-if: ${{ github.ref == 'refs/heads/main' }}`.

### CI-8 · Sin plazos ni concurrencia en los paquetes

C · S · cuidado bajo

Ningún `timeout-minutes` (por defecto, 6 h). → 30 en los de pruebas (45 en
Windows y macOS), 90 en los de paquetes, y en estos:

```yaml
concurrency:
  group: ${{ github.workflow }}-${{ github.ref }}
  cancel-in-progress: ${{ !startsWith(github.ref, 'refs/tags/') }}
```

### CI-9 · La comprobación «arrancan de verdad» no comprueba casi nada

C · S · cuidado bajo

`scripts/tools-linux.sh` (hacia 74-77) y `tools-macos.sh` (hacia 94-97):
con `set -eu` sin `pipefail`, `"$DEST/ffmpeg" -version | head -1` devuelve
el estado de `head`. Solo `fpcalc` puede fallar.

```sh
v=$("$DEST/ffmpeg" -hide_banner -version) || die "ffmpeg no arranca aquí"
printf '%s\n' "$v" | head -1
```

(igual para `qjs` y `yt-dlp`). Otros de guiones:
`build-windows-cross.sh:121` (`curl` sin `--retry 3`), `:129` y `:248`
(`mktemp -d` sin `trap`), `:32` lee `DANPLAY_CACHE_WIN` y `data.sh:78`
`DANPLAY_CACHE_WINDOWS`; `build-windows.ps1` (hacia 122-124) el `catch` se
traga también una suma que no coincide; `build.sh:83,100`,
`build-android.sh:30,63`, `test-install.sh:17` (`ls`/`find` sin comillas).

### CI-10 · Descargas sin fijar en las pruebas, y la foto del catálogo

C · S · cuidado bajo

- `tests.yml` (hacia 72) `choco install ffmpeg` y (hacia 225-228)
  `apt-get install … ffmpeg mingw-w64`, sin versión ni suma (y contradice el
  comentario de las líneas 37-39). → En `desktop`, `./scripts/tools-linux.sh`
  con su `actions/cache` como en `core`; en `core-windows`, el zip con la URL
  y la suma de `windows.yml:34-35`.
- La foto del catálogo de modelos se baja **al construir** cada paquete, con
  `|| true` (`linux.yml` hacia 77, `macos.yml` hacia 96, `windows.yml` hacia
  156-162, `scripts/build.sh` hacia 56): cada sistema puede llevar una
  distinta y el paquete no es reproducible. → Regenerarla al subir versión,
  dejarla en `data/models-snapshot.json` (ya está en git) y quitar el paso.
- ffmpeg de Linux y macOS sale de un servidor personal
  (`ffmpeg.martin-riedl.de`). La integridad está cubierta por el sha256; si
  la URL desaparece, se rompen paquetes y pruebas. → Subir esos zips a una
  versión propia del repositorio (`tools-ffmpeg-<versión>`) y usarla como
  segunda URL. Pide a Daniel (es publicar un archivo).
- Las versiones y sumas de Windows están triplicadas (`windows.yml`,
  `build-windows-cross.sh`, `build-windows.ps1`) y las de ONNX Runtime
  también (`data.sh`, `windows.yml`, `build-windows.ps1`). → (M) un
  `scripts/tools.lock.json` que lean los tres.

### CI-11 · Los avisos de seguridad solo se miran al subir algo

C · S y M · cuidado bajo

- (S) `schedule: - cron: '17 6 * * 1'` en `tests.yml`, con
  `if: github.event_name != 'schedule'` en los otros seis trabajos: un aviso
  nuevo aparece el lunes aunque no se haya subido nada.
- (M) `cargo deny check advisories licenses sources bans` con
  `unknown-registry = "deny"` (hoy todas las fuentes son crates.io) y la
  lista de licencias de `docs/DEPENDENCIES.md`. La primera pasada de
  licencias pide afinar.

### CI-12 · Faltan `shellcheck`, `cargo doc` y la prueba del paquete

P · S/M · cuidado bajo

- `shellcheck -S warning scripts/*.sh tests/smoke.sh packaging/deb/post* danplay-app.sh`
  en el trabajo `frontend` (viene en `ubuntu-latest`).
- `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps` (puede dar
  avisos la primera vez: arreglarlos).
- `linux.yml` nunca abre la ventana del paquete que publica. → Tras
  empaquetar: `sudo apt-get install -y xvfb`, `dpkg-deb -x` y
  `xvfb-run tests/smoke.sh ext/usr/bin/danplay-app`. **S**: no se sabe si
  WebKitGTK arranca bajo Xvfb sin retoques; probarlo en una rama con
  `workflow_dispatch`.
- `scripts/test.sh:2` dice «las mismas que la CI» y le faltan
  `node scripts/icons.mjs --check`, la cobertura, `vp build` y el audit:
  añadirlos o corregir la frase.

### CI-13 · Dos pruebas sensibles a la carga

S · S · cuidado bajo

`desktop/src-tauri/src/queue/tests.rs` (hacia 734, `< 50 ms`) y
`server/src/services/watcher/tests.rs` (hacia 200, `< 1500 ms`). Ya falló
una vez un test con el equipo ocupado. → Subir el primero a 500 ms (lo que
prueba es «no espera al disco», no una cifra).

---

## Lote 6 — Paquetes más pequeños

Medido sobre `DanPlay_1.44.1_amd64.deb` (145 MB; el AppImage, 244 MB):

| Pieza | En disco | gzip | xz |
| --- | --- | --- | --- |
| `tools/ffmpeg` | 87,1 MB | 31,9 | 23,5 |
| `tools/yt-dlp` | 38,6 | 38,1 | 38,0 |
| `data/pulse/beat_this.onnx` | 40,7 | 35,9 | 34,9 |
| `usr/bin/danplay-app` | 38,9 | 15,4 | 10,6 |
| `libonnxruntime.so` | 28,0 | 10,6 | 7,9 |
| `mega53.onnx` | 7,8 | 0,4 | 0,2 |
| `fpcalc` | 5,1 | 2,3 | 1,8 |

### PAQ-1 · El `.deb` va en gzip: con xz, 17,7 MB menos (medido)

C · S · cuidado bajo

`ar t` del paquete da `control.tar.gz` y `data.tar.gz`. Paso tras
`tauri bundle` en `linux.yml` (hacia 99) y `scripts/build.sh` (hacia 74):

```sh
deb=$(ls target/release/bundle/deb/*.deb)
rm -rf pkg && dpkg-deb -R "$deb" pkg && dpkg-deb -Zxz -z6 --root-owner-group -b pkg "$deb"
```

xz y no zstd: el `dpkg` de Debian 11 no abre zstd. **Validar** con
`.dev/dkpkg.sh sh -c './scripts/build.sh --package'` y
`scripts/test-install.sh` (Debian 13 y Ubuntu 24.04).

### PAQ-2 · yt-dlp bajo demanda: 38 MB menos por paquete (medido)

C · M · cuidado medio · **cambia algo que Daniel nota** (ver «Decisiones»)

Es un binario ya comprimido: 38 de los 145 MB. Solo sirve para YouTube, que
ya pide red, y la app ya sabe bajarlo de GitHub comprobando su suma
(`server/src/services/ytdlp.rs`). → No meterlo en los paquetes
(`tools-linux.sh` hacia 61-65, `tools-macos.sh` hacia 75-79, `windows.yml`
hacia 103, y las comprobaciones de `linux.yml` hacia 106, `macos.yml` hacia
127, `build.sh` hacia 104), y que la primera descarga de YouTube lance el
trabajo «yt-dlp» si no hay ejecutable, con su avance en la página de
Descargas y un mensaje claro si no hay red. **Prueba**: en
`server/src/api_tests/youtube.rs`, sin ejecutable, la descarga arranca la
instalación (contra el servidor falso) y después baja. Con PAQ-1 y PAQ-2:
de 145 a unos 89 MB (estimado a partir de las medidas).

### PAQ-3 · ffmpeg recortado

P · L · cuidado alto · **no recomendado ahora**

El código solo usa audio: decodificar a PCM; los codificadores
`libmp3lame`, `flac`, `libvorbis`, `libopus`; los filtros `amix`,
`alimiter`, `pan`, `aformat`, `atempo`, `asetrate`, `aresample`,
`rubberband` (opcional), `scale` para carátulas; y el muxer `md5`. Una
compilación propia rondaría 10-15 MB (estimación), pero habría que
compilarla en la CI para cuatro destinos y yt-dlp también lo usa para
remezclar. Después de PAQ-1 y PAQ-2, y solo si el tamaño sigue importando.

### PAQ-4 · Menores

- AppImage: no se pudo ver la compresión de su squashfs. Mirar con
  `--appimage-offset` y `unsquashfs -s`; si es gzip, probar zstd.
- `beat_this.onnx` ya va en float16: no hay recorte sin cambiar de modelo.

---

## Lote 7 — Dependencias y compilación

El único lote que toca `Cargo.toml`, `Cargo.lock` y `package.json`.
Estado medido el 22-nov-2026: `cargo audit`, **0 vulnerabilidades** y 10
avisos (sin mantener, incorrección, retirados); `bun audit --prod`, **0**;
`bun audit` completo, 9 moderadas y bajas, todas en herramientas de
desarrollo.

### DEP-1 · Avisos de Rust que se arreglan con `cargo update`

C · S · cuidado bajo

- `rustix 1.1.5` (RUSTSEC-2026-0486, incorrección en funciones `xattr`, que
  DanPlay no usa; es dependencia directa de `server`).
- `async-executor 1.14.0` y `yoke-derive 0.8.3`: versiones retiradas.

→ `cargo update -p rustix -p async-executor -p yoke-derive` dentro del
contenedor, y `cargo audit` después. Los demás avisos (`glib 0.18`,
`proc-macro-error`, `paste` por `lofty`, `zvariant`/`zbus_names`, `chrono`)
vienen de Tauri, `ksni` y `lofty`: no están en nuestra mano; apuntarlos en
`docs/DEPENDENCIES.md` y revisar cuando esas bibliotecas saquen versión.

### DEP-2 · `sha2` duplica media familia de crates

C · S · cuidado bajo

`library/Cargo.toml`: `sha2 = "0.10.9"` junto a `sha1` y `md-5` en 0.11:
arrastra `digest 0.10`, `block-buffer`, `crypto-common` y `cpufeatures`
viejos. → Subir `sha2` a `0.11` (mirar si cambia la API de `Digest`) y
comprobar con `cargo tree -d`.

### DEP-3 · Solo existe el perfil de release

C · S/M · cuidado medio

Sin `[profile.dev]` ni perfil de pruebas: cada binario de prueba se enlaza
con LTO «thin» y `opt-level = 3`.

```toml
[profile.dev]
debug = "line-tables-only"
[profile.dev.package."*"]          # symphonia, rustfft, realfft, rusqlite…
opt-level = 2
[profile.dev.package.danplay-engine]
opt-level = 2
[profile.ci]                       # pruebas: optimizadas, sin LTO ni strip
inherits = "release"
lto = false
strip = false
```

y `--profile ci` en las pruebas de `tests.yml` y de `scripts/test.sh`.
**Ojo**: deja los binarios en `target/ci/`: revisar `tests/smoke.sh:19`,
`scripts/which-version.sh:20`, `desktop/e2e/server.mjs` y los guiones de
`.dev/` (que dan por hecho `--release`). Medir el tiempo de
`cargo test --workspace` antes y después.

### DEP-4 · `symphonia` con rasgos de más en `library`

C · S · cuidado bajo

`library/Cargo.toml`: `symphonia` sin `default-features = false` (el motor
sí lo pone) arrastra `mkv`, `adpcm` y `pcm`. → Añadirlo y dejar solo los que
usa. Correr las pruebas de `library` que decodifican (onda, huella,
duplicados).

### DEP-5 · Herramientas de desarrollo de la interfaz con avisos

C · S/M · cuidado medio

`vite-plus 1.1.0` y `vitest 5.0.3` tienen un aviso moderado (el servidor de
desarrollo sirve archivos que debería negar; arreglado en `vite-plus ≥ 1.3.0`
y `vitest ≥ 5.0.5`). Solo afecta mientras corre `vp dev` o las pruebas en
modo navegador, nunca a la app. → Con el PR de Dependabot de `vite-plus`,
`vp migrate` en `desktop/` (sube `vite`, `vitest` y la cobertura juntos; ver
el comentario de `.github/dependabot.yml`). `fast-uri`, `glob` y `minimatch`
se arreglan con `bun update` de quien los trae (`eslint`, `stylelint`,
`@vue/test-utils`). `bunfig.toml` impide instalar nada con menos de 14 días:
respetarlo. `typescript` se queda en la 6 (`vue-tsc` no arranca con la 7).

### DEP-6 · El MSRV no lo prueba nadie

C · S · cuidado bajo

`rust-version = "1.90"` en `Cargo.toml` y ningún trabajo compila con esa
versión. O se prueba (`cargo +1.90.0 check --workspace` en un trabajo
semanal) o se sube a la versión de `rust-toolchain.toml` (CI-4), que es la
que de verdad se usa. Recomendado: lo segundo.

### DEP-7 · Tres funciones de resumen para nombres de caché

C · S · **no recomendado**: sha1, sha2 y md5 a la vez. Unificar cambiaría los
nombres de todas las cachés ya escritas. Se deja apuntado.

---

## Lote 8 — Limpieza y documentación

Sin efecto para quien usa la app. Cada punto es S y de cuidado bajo salvo
que diga otra cosa.

### Código

- **LIM-1 · Ramas muertas en la interfaz.** `StudyBar.vue` (hacia 3229): un
  `<p v-if="isMobileApp">` con `v-else-if`/`v-else` dentro (hacia 3242-3259)
  que nunca se alcanzan. El emit `'remove-cue-lane'` se declara
  (`StudyTimeline.vue` hacia 211) y se escucha (`StudyBar.vue` hacia 3206) y
  nadie lo emite.
- **LIM-2 · Exportaciones sin uso.** `FOLDER_ACTIONS` (`utils/folders.js`),
  `THEMES` (`themes.js` hacia 333), `newSince` (`data/releases.js`),
  `semitoneLabel` y `toneUnit` (`utils/theory.js`): las tres últimas solo las
  usan sus pruebas. Otras 17 llevan un `export` que no hace falta
  (`JOB_POLL_MS`, `sameOwn`, `combPoints`, `tableIds`, `FOCUSABLE`,
  `PAGE_ROUTES`, `whenIdle`…). Comprobar cada una con
  `/usr/bin/grep -rn` en `src/` **y** `tests/` antes de quitar.
- **LIM-3 · Utilidades repetidas.** `clamp` ×7, `round2` ×4, `round3` ×5
  (dos variantes, con y sin `+ 0`: no son iguales, mirar cuál usa quién),
  `readPref`/`savePref` ×3, `pctText` ×3, `MAX_CUE_LANES` ×2. → `utils/num.js`
  y `utils/prefs.js`. **Ojo**: `partsOf` está dos veces con reglas distintas
  (`StudyBar.vue` hacia 557 usa `length > 1`; `StudyTimeline.vue` hacia 923,
  `length`): averiguar cuál es la buena antes de unificar; puede ser un
  fallo.
- **LIM-4 · Dos listas de velocidades.** `usePlayback.js` (hacia 36-41)
  dice que son «las mismas en el reproductor y en el estudio», y
  `StudyBar.vue:98` tiene su propia `SPEEDS`, distinta. Es el E5 del plan
  anterior a medias: unificar o corregir el comentario. Comentarios
  huérfanos en `usePlayback.js` (hacia 763-766 y 1032).
- **LIM-5 · Tipos.** `jsconfig.json` tiene `checkJs: false`; 60 de 66 `.js`
  llevan `// @ts-check`; faltan `useClick.js` y `useBeatEdit.js`. Añadirlo a
  los dos y arreglar lo que salga. El script `check` de `package.json` no
  corre `typecheck`: añadirlo.
- **LIM-6 · Reglas de ESLint que habrían cazado lo de arriba.**
  `vue/no-unused-emit-declarations`, `vue/no-unused-refs`,
  `vue/no-setup-props-reactivity-loss`, `vue/no-ref-object-reactivity-loss`.
  Encenderlas de una en una y arreglar.
- **LIM-7 · CSS sin uso.** Candidatas reales: `study-metro-auto` y
  `cue-lead` (`study.css` hacia 289 y 1894), `empty-state` y `toggle-field`
  (`mobile.css` hacia 236 y 1339). Comprobar con grep antes de borrar.
  `vite.config.js`: `build.reportCompressedSize: false`.
- **LIM-8 · Solo para pruebas dentro del motor.** `Render::render`,
  `output_rate`, `set_out_rate` y el campo `limiter` de `Render`
  (`render.rs` hacia 561, 655, 792-804): pasarlos a `#[cfg(test)]`.
- **LIM-9 · Duplicados en Rust.** `analysis/src/pulse/net.rs` duplica
  `onnx::Net`. El parámetro `is_cancelled` de `separate::separate` y
  `mega::separate` siempre es `|| Ok(false)` en producción (se cancela
  matando el proceso): quitarlo. `compose`, `StemTrack`, `random_other`,
  `advance` y el texto de «está dañado» están en `mobile/src/player.rs` y en
  `desktop/src-tauri/src/queue/logic.rs`: llevarlos a `engine/` (M, cuidado
  medio). `mobile/src/player.rs` (hacia 414-427) relee el manifiesto que
  `Engine::load` ya leyó: devolverlo en `Loaded`.
- **LIM-10 · Pequeños fallos del motor que no cortan nada.**
  `engine/src/preview.rs` (hacia 25-27) usa `default_output_device()` y no
  `system_devices()`: sin el puente de ALSA la escucha previa puede ir a la
  tarjeta 0 (el mismo fallo que arregló A1-A3 del plan anterior, en otro
  sitio). `spec.rs` (hacia 76-110): `Clips` cuenta 64 entradas y no bytes
  (una guía puede pesar 106 MB): tope por bytes. `Mailbox::empty` solo corre
  en `send`: llamarlo también en `reap()`. `output.rs:130` y
  `engine.rs:869`: `spawn(..).ok()` calla el fallo al crear el hilo:
  registrarlo. `engine/src/record.rs` (hacia 187): un `Vec` y un `send` por
  bloque en el callback de entrada.
- **LIM-11 · Funciones de más de 200 líneas** (partir solo al tocarlas por
  otro motivo): `server/src/services/sequence.rs` `import_sequence` (357),
  `library/src/journal.rs` `reconcile` (288),
  `library/src/assistant/intake.rs` `scan_line` (284),
  `assistant/conversation.rs` `reply` (229), `assistant/matching.rs`
  `eval_tokens` (214); en el motor, `pull` (`render.rs` hacia 1210) y
  `run`/`run_max` (`process.rs` hacia 500 y 692).
- **LIM-12 · Permisos de la copia local.** `danplay-app.sh`,
  `tests/smoke.sh`, once guiones de `scripts/` y `.git/objects/pack/*`
  tienen escritura para todos (herencia de NTFS). →
  `chmod 755 danplay-app.sh scripts/*.sh tests/smoke.sh && chmod -R go-w .git`.
  No es un cambio de git: en el repositorio ya son 100755/100644.

### Documentación y repositorio

- **DOC-1 · Comentarios que ya no son verdad** (no hay Python ni PyO3 desde
  la 1.40, y el escritorio usa el motor entero):
  - `Cargo.toml` raíz: líneas 49-50, 55 y 78-79 (PyO3); «motor de pistas de
    la app del celular».
  - `server/Cargo.toml:7` («que la de FastAPI (danplay/api/)») y `:31`.
  - `analysis/Cargo.toml:21-22` («el mismo que Python… la 1.30»;
    `data.sh:49` usa la 1.31.0).
  - `engine/build.rs:2-3` («la app de escritorio usa este crate sin el»),
    `engine/Cargo.toml:11-14`, `engine/src/lib.rs` (la «funcion `motor`»: el
    rasgo se llama `engine`), `cpp/stretch.cpp:1`, `cpp/README.md`.
  - rodio en `click.rs:2`, `metro.rs:6` y `beats.rs:20`; `render.rs:979`
    cita `player/metro.rs`, que no existe; el comentario de `render()` quedó
    sobre `blocks_ready` (`render.rs` hacia 784-786).
  - mutagen, watchdog y platformdirs en `library/src/tags.rs`,
    `server/src/services/watcher.rs` y `library/src/config.rs`.
  - `.github/workflows/windows.yml:6-7`,
    `desktop/src-tauri/data/LEEME.txt:23`, `scripts/github.sh:166`,
    `docs/WINDOWS.md:45` y `:62`, `.editorconfig:2` (ruff) y `:20` (un
    `Makefile` que no existe).
- **DOC-2 · `.env.example` desfasado.** Documenta `DANPLAY_PYPI_URL`, que
  ningún código lee (la vigente es `DANPLAY_YTDLP_RELEASES`), y le faltan
  las que sí lee `library/src/config.rs` (hacia 404-430):
  `DANPLAY_TOOLS_DIR`, `DANPLAY_AI`, `DANPLAY_CONVERT`, `DANPLAY_QUALITY`,
  `DANPLAY_KEEP_ORIGINAL`, `DANPLAY_NEVER_CONVERT`, `DANPLAY_TAGS`,
  `DANPLAY_STEMS_FORMAT`, `DANPLAY_DUP_THRESHOLD`, `DANPLAY_INBOX`.
- **DOC-3 · Históricos a `docs/historia/`.** `TESTS-PORT.md` (1 475
  líneas), `MIGRATION.md` (375) y `PLAN.md` (1 073) describen cómo se llegó
  aquí, no el presente. `git mv` y actualizar los enlaces: `README.md` (4),
  `docs/ARCHITECTURE.md` (3), `CONTRIBUTING.md`, `DEPENDENCIES.md`,
  `WINDOWS.md`, `server/src/lib.rs` (1) y `CHANGELOG.md` (3). Ninguna prueba
  los lee. `ARCHITECTURE.md` (2 904 líneas) mezcla presente e historia:
  separarlo es L y no urge.
- **DOC-4 · Generados y `.gitignore`.**
  `desktop/src-tauri/gen/schemas/` y `mobile/gen/schemas/` (~1,7 MB) los
  reescribe `tauri-build` en cada compilación: dejar de versionarlos
  (`git rm --cached` y `.gitignore`) **solo si** `capabilities/*.json` sigue
  validando sin ellos; si no, marcarlos en `.gitattributes`
  (`linguist-generated=true`) junto con
  `desktop/src-tauri/permissions/autogenerated/**`. Añadir `*.onnx binary`.
  Pasar a `.gitignore` lo que hoy vive en `.git/info/exclude`
  (`.claude/worktrees/`), más `.idea/`, `.vscode/` y `Cargo.lock.tmp`.
  `.markdownlint.jsonc` existe y nada lo ejecuta: usarlo o quitarlo.
- **DOC-5 · `bump-version.sh` no avisa de lo que se le escapa.** Toca
  `Cargo.toml`, `desktop/package.json`, `README.md` y `Cargo.lock`; se le
  escapan `CHANGELOG.md` y `desktop/src/data/releases.js` (lo caza
  `about.test.js`, pero tarde). Añadir al final:

  ```sh
  grep -q "^## $new_version " CHANGELOG.md || echo "falta la entrada de $new_version en CHANGELOG.md y en desktop/src/data/releases.js"
  ```

  Y las etiquetas: 15 de 19 son ligeras; usar `git tag -a` (ya se hizo con
  la `v1.44.1`).

---

## Lote 9 — Partir `StudyBar.vue` (M5 del plan anterior)

C · L · cuidado **alto**. 3 974 líneas. **Hacer INT-1 antes**: si no, cada
componente nuevo hereda el repintado por aviso. Y hacerlo cuando Daniel haya
usado la 1.44.x unos días (lo dejó dicho: «con riesgo de romper lo que acaba
de quedar bien»).

Las pruebas montan StudyBar entero y miran clases CSS (`tests/study.test.js`,
`click.test.js`, `cue.test.js`, `beat-tools.test.js`; e2e `study.spec.js`,
`study-beats.spec.js`, `beat-edit.spec.js`): el corte es seguro **si el DOM
y las clases no cambian**. El estado compartido va por un solo
`provide(STUDY, ctx)` armado en StudyBar con los composables; los hijos hacen
`inject` (con props serían más de cuarenta).

Un paso por commit, con toda la puerta de la interfaz en verde tras cada
uno, en este orden (las líneas son de `d6ebfde`):

1. `composables/useStudyDoc.js`: script 104-115, 154, 160, 171-209, 292-404
   (`blank`, `study`, `loadedFor`, `studyFor`, `saving`, `payload`,
   `mixerPayload`, `scheduleSave`, `flushSave`, `warnIfOnlyHere`). `loadFor`
   (212-290) se queda en StudyBar: orquesta.
2. `StudyNotes.vue`: plantilla 3849-3914, script 168 y 2101-2105.
3. `StudyTempo.vue`: plantilla 3282-3387, script 98, 719-737, 753-770.
4. `useStudyMarkers.js` (script 161-164, 1383-1497) y `StudyMarkers.vue`
   (plantilla 3659-3722).
5. `useStudyCues.js` (script 1498-2100) y `StudyCueList.vue` (plantilla
   3723-3847). El `watch` de `state.path` (1567-1576) se queda en StudyBar.
6. `useStudyStems.js` (script 145-153 y 2106-2451, menos el encaje,
   2114-2139) y `StudyStems.vue` (plantilla 2710-2914).
7. `useStudyClick.js` (script 771-1382) y `StudyClick.vue` (plantilla
   3389-3657). `soundingGrid`, `edit` y `clickBeats` salen por el contexto:
   los usan la onda y las guías.
8. `useStudyLoops.js` (script 406-717) y `StudyWaveBar.vue` (plantilla
   2915-3162). El último: es el que más cruza.

Quedan en StudyBar la cabecera, `loadFor`, ocultar y salir, las pestañas, el
recorrido, el encaje, los cuatro diálogos y `<StudyTimeline>`: unas 700
líneas. Al final, las e2e enteras y fotos a 1536×864 (la ventana real de
Daniel) antes y después: tienen que salir iguales.

---

## 3. Decisiones

**Tomadas en este plan** (se pueden cambiar; van dichas para que quien lo
aplique no pare a preguntar):

- SEG-1: con texto de fuera leído en el turno, `edit_song` pide permiso
  también para `year` y `genre`.
- PAQ-1 con xz y no zstd (por Debian 11).
- CI-4 y DEP-6: fijar la versión de Rust y subir el MSRV a esa misma.
- No se hacen: PAQ-3 (ffmpeg recortado), INT-15 (quitar vue-router), DEP-7
  (unificar resúmenes), Git LFS para los modelos (48,5 de los 68 MB
  versionados son dos `.onnx`; LFS gasta cuota en cada descarga y hoy un
  solo commit toca el grande).

**Esperan a Daniel:**

1. **PAQ-2**: sacar yt-dlp del paquete ahorra 38 MB, pero la primera
   descarga de YouTube tarda unos segundos más y sin red no hay yt-dlp
   (tampoco YouTube). Recomendado: sí.
2. **AUD-12 b**: prioridad de tiempo real para el hilo de audio. Hay que
   probarlo tocando mientras se separa una canción.
3. **AUD-15**: cambia cómo suenan los archivos de 88,2 kHz o más (mejor,
   según la medida). ¿Tiene alguno en su biblioteca?
4. **CI-10**: subir los zips de ffmpeg a una versión propia del repositorio.
5. **Publicar**: todo este plan se queda en ramas locales. Subir a `main` y
   etiquetar `v*` lo decide él.

**Siguen como estaban** (del plan anterior, no se tocan aquí): H2, H3, H5,
H12, I8, subir `cpal` de versión y el limitador con anticipación.

---

## 4. Revisado y está bien

Para que nadie lo vuelva a mirar.

**Seguridad.** CSP estricta sin `withGlobalTauri`; comandos por ventana (la
ventanita no tiene `api`); `danplay://cover` solo admite cifras; el puente
limita método y prefijo `/api/`; abrir enlaces solo http(s);
`pkexec` sin `sh -c` y con paquetes constantes; el celular con `core:default`
y `dialog:allow-open`. Markdown que escapa antes de marcar; los únicos
`v-html` son ese y los iconos. Token en tiempo constante, validación de
`Host`, `X-DanPlay`, CORS de dos orígenes; sin token no escucha fuera de
loopback; socket Unix 0600. Ajustes por lista cerrada. yt-dlp con
`--ignore-config`, `--` antes del enlace y sin shell; solo seis dominios de
YouTube. `.danpista`: nombres por lista cerrada, sin zip-slip, topes medidos
sobre lo escrito. Tiempo límite y tope de 64 MB por respuesta de red.
Carpeta 0700, archivo 0600, escritura atómica; claves enmascaradas hacia la
interfaz. SQL con listas blancas y FTS5 con comillas dobladas. Las tres
acciones sin vuelta atrás del asistente se confirman en el servidor. `.env`
fuera de git y en 0600.

**Núcleo.** Arranque síncrono ligero; migraciones una vez por base; todas
las expresiones regulares en `LazyLock` salvo `artist_pattern`; escaneo
incremental con rayon en tandas; poda de cachés solo si algo cambió;
vigilante y diario sin sondeo; `/api/status` no lanza yt-dlp; filas ligeras
en la lista; listas sin N+1; duplicados en paralelo. 0 `unwrap()` y 33
`expect()` fuera de pruebas, todos sobre invariantes locales.

**Interfaz.** Los 103 iconos se usan; tabla, filas, tarjetas y rejilla
virtualizadas; `songs` y `queue` en `shallowRef`; topes de nodos por pulso;
todos los oyentes y temporizadores con su limpieza; ningún sondeo de fondo
en Tauri; carreras al cambiar de canción cubiertas; sin `console.log`; todas
las dependencias se usan; `cssCodeSplit` funciona; animaciones de
`opacity`/`transform`.

**Motor.** Sin cerrojos disputados en el callback; buzón y basura por
`rtrb`; el estirador no asigna y a 1× y tono 0 se salta de verdad; `unsafe`
solo en el FFI del estirador, el `no_mangle` de JNI y un `set_var` antes de
crear hilos; `mp4.rs` con accesos comprobados; el anillo (1,49 s) cuenta los
cortes y se recupera; `decode.rs` aguanta tasa 0 y 0 canales; el evento de
estado va a 4 Hz con un JSON pequeño.

**CI y paquetes.** Sin `pull_request_target`; sin expresiones de eventos en
`run:`; los secretos de Android por `env:` de un solo paso; todo lo que
viaja en los paquetes con versión fija, HTTPS y sha256; Dependabot con 14
días de espera; `--frozen-lockfile`; todas las fuentes de `Cargo.lock` son
crates.io; el `CHANGELOG.md` no se empaqueta; no hay modelos duplicados en
el paquete; umbral de cobertura en `vitest.config.js`; las 15 pruebas
`#[ignore]` están justificadas.
