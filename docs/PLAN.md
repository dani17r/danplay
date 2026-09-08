# Plan de evolución de DanPlay

Fecha: 7 de septiembre de 2026. **Actualizado el 8 de septiembre: los bloques
A y D están hechos** (ver «Qué se hizo» más abajo); Android se descartó por
ahora y Windows queda preparado pero sin compilar.

Todo lo que se afirma aquí se comprobó en el repositorio, en las dependencias
descargadas en `~/.cargo`, en tu sesión de escritorio o en la documentación
oficial (las fuentes van al final).

---

## Qué se hizo (8 de septiembre)

**La bandeja, como se pidió.** Icono propio por D-Bus (`ksni`), porque el de
Tauri no entrega los clics en Linux. Clic izquierdo: mini reproductor. Clic
derecho: menú. Clic central: pausa. Rueda: volumen. Comprobado contra la
sesión de Plasma de verdad, no solo en las pruebas: el icono se registra
(`org.kde.StatusNotifierItem`), el clic abre y cierra la ventanita, y «Salir»
cierra la aplicación y el núcleo sin dejar nada.

**Cerrar la ventana la esconde.** La música sigue, el icono sigue, y solo se
sale con «Salir» (o con `Ctrl+Q` desde la ventana). Si el escritorio no tiene
bandeja, cerrar cierra: la aplicación se entera de que no hay dónde quedarse.

**La cola vive en Rust.** Con la ventana escondida el navegador ralentiza sus
temporizadores hasta una vez por minuto, así que la música se paraba entre
canciones. Ahora Rust decide qué suena y lo cuenta por un evento; han
desaparecido los tres bucles de sondeo que había.

**Las teclas multimedia funcionan** (MPRIS), y el escritorio enseña lo que
suena con su carátula.

**Los fallos que encontró la revisión, arreglados**: el reescaneo ya no
reasigna los ids (las listas dejan de apuntar a otras canciones), «resolver
duplicados» solo toca archivos de la biblioteca y manda a la papelera,
exportar una lista no puede escribir fuera, la ventanita tiene sus permisos,
«conservar el original» se manda con el nombre que la API lee, los `.ogg`
suenan, y un `Play` fallido ya no revive la canción anterior.

**El asistente ya no borra por su cuenta**: lo que no tiene vuelta atrás se
pregunta, y lo aprueba una persona.

**Lo que faltaba de herramientas**: ESLint, Prettier, Stylelint,
comprobación de tipos, integración continua, el generador de iconos que el
propio archivo decía que existía, y una biblioteca de prueba que se genera
con ffmpeg en vez de depender de tu música.

De 426 pruebas a **501**, todas en verde.

Lo que **no** se hizo, y por qué:

- **Android**: descartado por ahora a petición tuya. El bloque C se queda como
  estudio para cuando toque.
- **Windows**: el código ya tiene su camino (papelera, rutas, transporte con
  token, Job Object), pero **nadie lo ha compilado ni ejecutado**. Hace falta
  una máquina Windows: PyInstaller no cruza plataformas.
- **Los selectores repetidos del CSS**: la hoja está partida por áreas y las
  reglas muertas fuera, pero quedan unos veinte selectores definidos dos
  veces. Fusionarlos cambia el orden de la cascada y hay que hacerlo mirando
  la aplicación.

---



Cuatro bloques, en el orden en que los pediste:

| Bloque | Qué resuelve | Veredicto corto |
| --- | --- | --- |
| **A. Bandeja y mini reproductor** | Clic en el icono abre el mini reproductor; cerrar la ventana la oculta; salir solo desde la bandeja | Se puede, con bandeja propia por D-Bus (`ksni`) en Linux. Ni parche ni espera: Tauri no lo va a traer pronto. De paso: la ventanita actual está **rota por permisos** (no tiene *capability*), por eso no avanza ni responde a anterior/siguiente |
| **B. Windows** | Compilar y empaquetar para Windows | Se puede. Una veintena de puntos concretos del código son solo-Linux; el más gordo es el socket Unix |
| **C. Android (APK)** | Usar DanPlay desde el móvil | La app tal cual **no** cabe en un teléfono (el núcleo Python no puede ir dentro). Sí cabe una app móvil que hable con tu PC, y más adelante una biblioteca local si se reescribe el núcleo en Rust |
| **D. Mejoras generales** | Seguridad, rendimiento, paquetes, refactor | Base sólida (426 pruebas en verde), pero con **tres fallos graves verificados**: el reescaneo reasigna los ids y las listas acaban apuntando a otras canciones; «resolver duplicados» borra de forma permanente cualquier ruta; exportar una lista puede escribir fuera de la biblioteca. Además: la ventanita sin *capability*; «conservar el original» se ignora al convertir por un nombre mal escrito; los `.ogg` no suenan (falta el demuxer); la API se congela mientras se hashean duplicados (GIL); dos avisos de seguridad en `pyo3`; dos instancias que se pisan el socket; paquetes sin uso; siete desajustes de nombres interfaz/API; y un `App.vue` de 1.072 líneas |

---

## 1. Estado verificado

| Capa | Versión instalada | Última publicada | Nota |
| --- | --- | --- | --- |
| Tauri (Rust) | 2.11.5 | 2.11.5 (jul 2026) | al día |
| tray-icon / libappindicator | 0.24.2 / 0.9.0 | — | sin clics en Linux (ver A) |
| rodio | 0.20.1 | 0.22.2 | dos versiones detrás; cambió la API (`OutputStreamBuilder`) |
| pyo3 (crate `core/`) | 0.23.5 | 0.29.2 | **2 avisos RUSTSEC** (2025-0020, 2026-0177) |
| Vue / Vite / Vitest | 3.5.42 / 6.4.3 / 5.0 | 3.5.x / 8.2.2 / 5.x | Vite dos mayores detrás (Rolldown) |
| Python / FastAPI / uvicorn | 3.13.5 / 0.141 / 0.52 | al día | solo `pydantic_core` desfasado |
| Node / rustc | 24.15 / 1.98.1 | — | bien |

Tamaños de lo que se distribuye: `danplay-app` 8 MB, núcleo PyInstaller 41 MB
(**onefile**: se descomprime en `/tmp` en cada arranque), `.deb` 44 MB, AppImage
144 MB. El `.deb` declara `Depends: ffmpeg, libchromaprint-tools,
libayatana-appindicator3-1, libwebkit2gtk-4.1-0, libgtk-3-0`.

Pruebas hoy: 162 Python + 232 Vitest + 7 de rutas de medios, todas en verde
(la de humo no la lancé: abre la app en tu escritorio). No hay CI ni linter.

Tu entorno: Debian 13, **KDE Plasma 6.3.6 en Wayland**. En la sesión están
vivos `org.kde.StatusNotifierWatcher` (kded6) y el host (plasmashell): la
bandeja por D-Bus que propongo se registra ahí sin nada más. También tienes
GNOME 48 instalado **sin** la extensión AppIndicator: si alguna vez entras por
GNOME, no habrá bandeja de ningún tipo, y el plan lo contempla.

Auditorías: `npm audit` limpio; `cargo audit` en la app solo avisa de crates sin
mantener que arrastra GTK 0.18 (cosa de Tauri, nada que hacer); `cargo audit` en
`core/` da los dos avisos de `pyo3`.

---

## 2. Bloque A: bandeja del sistema y mini reproductor

### 2.1 Diagnóstico

Lo que te molesta no es un fallo tuyo ni de tu escritorio. El propio código lo
documenta en `desktop/src-tauri/src/tray.rs:3-7`: en Linux, Tauri monta la
bandeja con `libappindicator`, y esa biblioteca **no entrega los clics sobre el
icono**, solo el menú. Por eso el mini reproductor hoy es una ventana flotante
aparte (`tray.rs:196-225`), abierta desde una entrada del menú, siempre encima
y colocada a ojo en una esquina (`tray.rs:232-242`).

Comprobado:

- La documentación de Tauri lo dice tal cual: «Linux: unsupported. The event is
  not emitted even though the icon is shown». El issue tray-icon#104 (dic 2023)
  sigue abierto sin actividad.
- Hay un PR en Tauri (tauri#12319) para cambiar `libappindicator` por el crate
  `ksni`, con una feature `linux-ksni`. **Sigue abierto y sin versión.** Lo
  verifiqué en los `Cargo.toml` descargados: ni tauri 2.11.5 ni tray-icon
  0.24.2 tienen esa feature. No es una opción hoy.
- `libayatana-appindicator` está además dando problemas nuevos en Plasma 6
  sobre Wayland (tray-icon#336, jul 2026): el icono a veces ni se registra.
- **La ventanita actual está rota por permisos** (lo encontraron por separado
  los dos revisores y lo confirmé en `gen/schemas`): la única *capability*
  (`capabilities/default.json:5`) cubre solo `"windows": ["main"]`. La ventana
  `mini` no está en ninguna, y en Tauri 2 eso significa que se le deniegan
  `listen`, `emit` y `setPosition` (`core:window:default` ni siquiera incluye
  `allow-set-position` ni `allow-start-dragging`). Como `await tray.onChanged`
  rechaza en `MiniPlayer.vue:91`, el `refresh()` y el bucle de las líneas
  92-93 no llegan a ejecutarse: la ventanita enseña el título inicial pero la
  barra no avanza, el botón siempre dice «Reproducir», anterior/siguiente no
  hacen nada y la posición no se recuerda. Las pruebas no lo ven porque
  `mini-player.test.js:19` pone `tray.available = false`. Es un arreglo de
  cinco líneas (`capabilities/mini.json`) y explica parte de la sensación de
  «librería cerrada».

### 2.2 Alternativas evaluadas

| Opción | Qué da | Qué cuesta | Veredicto |
| --- | --- | --- | --- |
| **1. Bandeja propia con `ksni`** (StatusNotifierItem por D-Bus) | Clic izquierdo, clic central, rueda, menú contextual, tooltip con título y subtítulo, icono dinámico. Es el protocolo nativo de Plasma; GNOME lo usa con la extensión AppIndicator | Escribir el módulo (~200 líneas de Rust) y mantener dos implementaciones: `ksni` en Linux, la de Tauri en Windows/macOS (donde sí llegan los clics) | **Recomendada** |
| 2. Esperar `linux-ksni` en Tauri | Cero código propio | Sin fecha; el PR lleva desde enero de 2025 | No |
| 3. GTK `StatusIcon` / XEmbed | Clics | API obsoleta, solo X11, Plasma la emula mal | No |
| 4. Solo MPRIS | Plasma pinta su propio reproductor en la bandeja (carátula, play, siguiente) y las teclas multimedia funcionan | No es tu diseño: no puedes poner lo que quieras | **Además**, no en lugar de: es gratis con `souvlaki` (ver 2.7) |
| 5. `tauri-plugin-positioner` (colocar el popup junto al icono) | Posición exacta respecto al icono | Solo Windows/macOS; en Linux no hay eventos de bandeja y en Wayland no existe posicionamiento global | Solo para Windows/macOS |

`ksni` 0.3.6 (julio 2026, MIT/Apache). Métodos del trait `Tray` que usaremos,
confirmados en su documentación: `activate(x, y)` (clic izquierdo),
`secondary_activate(x, y)` (clic central), `scroll(delta, orientation)`,
`menu()` (clic derecho), `tool_tip()`, `icon_pixmap()` (ARGB32),
`watcher_offline()` (para saber si la bandeja desaparece) y la constante
`MENU_ON_ACTIVATE` (la dejamos en `false`: el clic izquierdo es nuestro).
Errores al arrancar: `Error::Watcher` y `Error::WontShow` = no hay bandeja en
este escritorio.

### 2.3 Diseño propuesto

```text
                 ┌────────────────────────────────────────┐
   clic izq ───► │ tray::linux (ksni, D-Bus)              │ ◄── Linux
   clic izq ───► │ tray::desktop (tauri tray)             │ ◄── Windows / macOS
                 └──────────────┬─────────────────────────┘
                                │ toggle_popup()
                 ┌──────────────▼─────────────────────────┐
                 │ ventana "mini" (WebviewWindow)          │  mismo MiniPlayer.vue
                 │ sin marco, sin barra de tareas, pequeña │  se oculta al perder el foco
                 └──────────────┬─────────────────────────┘
                                │ escucha eventos, manda órdenes
                 ┌──────────────▼─────────────────────────┐
                 │ player + queue (Rust)                   │  estado único: qué suena,
                 │ emite "danplay://state"                 │  cola, repeat, shuffle
                 └────────────────────────────────────────┘
                                ▲
                 ventana principal (App.vue) · MPRIS/SMTC (souvlaki) · menú bandeja
```

Comportamiento del icono:

| Gesto | Acción |
| --- | --- |
| Clic izquierdo | Abre el mini reproductor pegado a la bandeja; otro clic (o perder el foco, o Escape) lo cierra |
| Clic derecho | Menú: lo que suena · Mostrar DanPlay · Salir |
| Clic central | Pausar / seguir |
| Rueda | Volumen ±5 % |
| Pasar el ratón | Tooltip «Título — Artista» (en Plasma sale con carátula si se la damos) |
| Icono | El de la app; con un punto de «sonando» superpuesto cuando hay música (ksni admite `overlay_icon_pixmap`) |

El mini reproductor deja de ser una ventana que vive «por fuera»: es un popup
que solo existe mientras lo miras. Se reutiliza `MiniPlayer.vue` casi entero;
lo que cambia es quién lo abre y cuándo se cierra.

### 2.4 Boceto del módulo Linux

Es un boceto para leer, no está compilado. Nombres de API verificados en la
documentación de `ksni` 0.3.

```rust
// desktop/src-tauri/src/tray/linux.rs
// Bandeja por StatusNotifierItem (D-Bus). Nada de libappindicator: esta si
// entrega el clic izquierdo, el central y la rueda.
use ksni::{menu::*, Icon, ToolTip, Tray, TrayMethods};
use tauri::AppHandle;

pub struct DanTray {
    app: AppHandle,
    now: super::NowPlaying,   // copia de lo que suena, para el tooltip y el menu
    icon: Icon,               // ARGB32 del icono de la app
}

impl Tray for DanTray {
    fn id(&self) -> String { "danplay".into() }
    fn title(&self) -> String { "DanPlay".into() }
    fn icon_pixmap(&self) -> Vec<Icon> { vec![self.icon.clone()] }

    fn tool_tip(&self) -> ToolTip {
        let l = super::labels(&self.now);
        ToolTip { title: l.song, description: "DanPlay".into(), ..Default::default() }
    }
    // clic izquierdo: el mini reproductor, que es lo que se pidio
    fn activate(&mut self, _x: i32, _y: i32) { super::toggle_popup(&self.app) }
    // clic central: pausar o seguir, orden directa al hilo de audio
    fn secondary_activate(&mut self, _x: i32, _y: i32) { crate::player::toggle(&self.app) }
    // rueda: volumen
    fn scroll(&mut self, delta: i32, _o: ksni::Orientation) {
        crate::player::nudge_volume(&self.app, if delta > 0 { 0.05 } else { -0.05 })
    }
    // clic derecho: el menu de siempre, sin los mandos (ya estan en el popup)
    fn menu(&self) -> Vec<MenuItem<Self>> {
        vec![
            StandardItem { label: super::labels(&self.now).song, enabled: false,
                           ..Default::default() }.into(),
            MenuItem::Separator,
            StandardItem { label: "Mostrar DanPlay".into(),
                           activate: Box::new(|t: &mut Self| super::show_main(&t.app)),
                           ..Default::default() }.into(),
            StandardItem { label: "Salir".into(), icon_name: "application-exit".into(),
                           activate: Box::new(|t: &mut Self| t.app.exit(0)),
                           ..Default::default() }.into(),
        ]
    }
    // si Plasma reinicia su bandeja, nos quedamos esperando a que vuelva
    fn watcher_offline(&self, _r: ksni::OfflineReason) -> bool { true }
}

/// Se llama desde `setup`. Si falla, no hay bandeja en este escritorio y la
/// app lo tiene que saber (ver 2.5): entonces cerrar la ventana cierra la app.
pub async fn install(app: AppHandle) -> Result<ksni::Handle<DanTray>, ksni::Error> {
    DanTray::new(app).spawn().await
}
// Cuando cambia lo que suena:  handle.update(|t| t.now = data.clone()).await;
```

Detalles que hay que hacer bien:

- **El icono.** `app.default_window_icon()` da RGBA; ksni quiere ARGB32 en
  orden de red. Son cuatro líneas de conversión, pero si se olvida el icono
  sale con los colores cambiados.
- **El runtime.** `ksni` usa tokio; Tauri también (`tauri::async_runtime`).
  Se lanza con `tauri::async_runtime::spawn`, sin montar otro runtime.
- **`Cargo.toml`.** `ksni` solo en Linux y la feature `tray-icon` de Tauri solo
  fuera de Linux, con dependencias por destino:

  ```toml
  [dependencies]
  tauri = { version = "2" }
  [target.'cfg(target_os = "linux")'.dependencies]
  ksni = "0.3"
  [target.'cfg(not(target_os = "linux"))'.dependencies]
  tauri = { version = "2", features = ["tray-icon"] }
  ```

  Con el resolver 2 (edición 2021) las features de una dependencia por destino
  no se contagian a los demás destinos, así que el binario Linux deja de
  enlazar `libappindicator`. Hay que comprobar después que el `.deb` ya no
  declare `libayatana-appindicator3-1`; si el empaquetador lo sigue metiendo,
  se fija la lista en `bundle.linux.deb.depends` (donde ya están `ffmpeg` y
  `libchromaprint-tools`).
- **Windows/macOS.** Se queda el `tray.rs` actual reducido: el evento
  `TrayIconEvent::Click` trae `position` y `rect`, con eso el popup se coloca
  justo encima del icono. Ahí `tauri-plugin-positioner` (`Position::TrayCenter`)
  ahorra el cálculo. Hace falta `.show_menu_on_left_click(false)` en el
  builder: por defecto es `true`, y hoy el clic izquierdo abriría el menú **y**
  la ventanita a la vez.

### 2.5 Cerrar la ventana = ocultar; salir solo desde la bandeja

Hoy `main.rs:257-272` deja que la ventana se cierre y de paso cierra la mini;
al no quedar ventanas, el runtime emite `ExitRequested { code: None }` y la
app termina. El cambio, con los matices que encontró la revisión de Rust:

```rust
// en setup, sobre la ventana principal
main.on_window_event(move |event| {
    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
        if tray_available(&handle) {          // hay bandeja donde quedarse
            api.prevent_close();
            let _ = handle.get_webview_window("main").map(|w| w.hide());
            let _ = handle.get_webview_window("mini").map(|w| w.hide());
        }
        // sin bandeja se cierra de verdad, como hasta ahora: si no, la app
        // quedaria viva sin ninguna forma de volver a verla ni de salir
    }
});

// en run
.run(|app, event| match event {
    // solo «se destruyo la ultima ventana»; app.exit(0) llega con Some(0)
    tauri::RunEvent::ExitRequested { api, code: None, .. } if tray_available(app) => api.prevent_exit(),
    tauri::RunEvent::Exit => { if let Some(n) = app.try_state::<Core>() { cleanup(&n) } }
    #[cfg(target_os = "macos")]
    tauri::RunEvent::Reopen { has_visible_windows: false, .. } => tray::show_main(app),
    _ => {}
})
```

Tres detalles que no son opcionales:

- **`cleanup` hoy corre también en `ExitRequested`** (`main.rs:399-404`). Es
  inofensivo mientras la app muere justo después, pero en cuanto exista
  `prevent_exit` mataría el núcleo con la app viva. Pasa a `RunEvent::Exit`
  solo.
- **`tray_available` tiene que ser de verdad.** En Windows/macOS basta
  `app.tray_by_id("danplay").is_some()`. En Linux con `ksni` la señal es que
  `spawn()` haya devuelto `Ok` (falla con `Error::Watcher`/`WontShow` si no hay
  bandeja). Con el appindicator actual no habría forma: `app_indicator_new`
  nunca falla aunque nadie pinte el icono.
- **Matar el núcleo con SIGTERM, no SIGKILL** (`main.rs:226-234` usa
  `kill()`): así uvicorn ejecuta su `finally` y SQLite cierra limpio; SIGKILL
  como respaldo a los dos segundos.

- **Salir**: la entrada «Salir» de la bandeja llama a `app.exit(0)`, que dispara
  `RunEvent::Exit`, donde se mata el núcleo (con SIGTERM, ver arriba) y se
  borra el socket (`main.rs:399-404`).
- **Sin bandeja** (GNOME sin extensión, un escritorio raro, o `ksni` falla):
  `available()` es `false`, cerrar cierra. Además conviene una entrada «Salir»
  dentro de la app (en el menú de vista, con `Ctrl+Q`) para que siempre haya
  salida aunque el icono no esté.
- **Primera vez**: al ocultarse por primera vez, un aviso de una sola vez
  («DanPlay sigue en la bandeja; para cerrarla del todo usa Salir»). Es lo que
  hacen Telegram o Spotify y evita el «¿dónde se fue?».
- **Segunda instancia**: hoy, si abres DanPlay dos veces, la segunda **borra el
  socket de la primera** (`main.rs:184` y `api.py:670` hacen `remove_file` del
  mismo `danplay.sock`) y al salir se lo lleva por delante. Con la ventana
  oculta esto va a pasar más (la gente vuelve a pulsar el icono del menú). La
  solución es `tauri-plugin-single-instance` como **primer** plugin: la segunda
  instancia no arranca y la primera enseña su ventana.
- **Vigilar al núcleo**: hoy `child` nunca se sondea y `core_ready`
  (`main.rs:131-133`) solo mira si existe el fichero del socket. Si el núcleo
  muere (OOM, fallo), la interfaz cree que sigue ahí. Con la app viviendo horas
  en la bandeja hace falta `try_wait()` periódico, reinicio con espera creciente
  y un aviso a la interfaz.
- **Lo que suena sigue sonando**: el audio vive en el hilo de Rust, no en la
  ventana, así que ocultarla no lo corta. Pero la **cola** (qué va después) vive
  hoy en `App.vue` (`step`, `onTrackEnded`, líneas 338-371), y eso es un
  problema con la ventana oculta; ver 2.6.
- **macOS** (si algún día): al ocultar la última ventana conviene pasar a
  `ActivationPolicy::Accessory` para que desaparezca del Dock. Fuera de alcance
  ahora.

### 2.6 La cola de reproducción pasa a Rust

Es el cambio de fondo que hace que todo lo demás sea sólido, y lo recomiendo
en la misma tanda que la bandeja.

Por qué:

1. **Con la ventana oculta, el JS se ralentiza.** Chromium (y por tanto
   WebView2 en Windows) aplica «intensive wake-up throttling»: a los cinco
   minutos de estar oculta, los temporizadores encadenados de una página
   despiertan **una vez por minuto**. El aviso de «fin de pista» sale hoy de un
   `setTimeout` cada 250 ms en `Player.vue:140-147`. Resultado: hasta un minuto
   de silencio entre canciones cuando la app lleva un rato en la bandeja.
   WebKitGTK no documenta el mismo comportamiento, pero no hay garantía.
2. **El mini reproductor y la bandeja piden «siguiente» a la ventana**
   (`tray.rs:172-174`, `MiniPlayer.vue:103-104`) porque solo ella sabe la cola.
   Con MPRIS pasa igual: Plasma manda «Next» y alguien tiene que saber qué es
   «next» sin depender de una ventana que puede estar dormida.
3. Hoy hay **tres bucles de polling** preguntando lo mismo (`Player.vue` a 250
   ms, `MiniPlayer.vue` a 400 ms, y `App.vue` observa `trayState` en profundidad
   para reenviarlo a Rust). Con el estado en Rust, Rust lo **emite** y las
   ventanas escuchan.

Diseño (`desktop/src-tauri/src/queue.rs`):

```text
Queue { items: Vec<Track>, index, repeat: List|One|Once|Queue, shuffle: bool, origin }
Track { id, path, title, artist, duration, blur }

comandos (invoke):  set_queue(items, start_id, origin) · next() · previous() ·
                    jump(id) · set_repeat(mode) · set_shuffle(bool) · toggle() · seek() · volume()
evento (emit):      "danplay://state" { track, playing, position, duration, volume,
                                        speed, repeat, shuffle, has_previous, has_next }
                    cada 250 ms mientras suena; al instante en cada cambio; nada en reposo
```

El hilo de audio (`player.rs`) ya sabe cuándo se agota el `Sink`
(`player.rs:178`); en vez de anotarlo en `finished` para que el JS lo descubra,
avisa a la cola y la cola decide (repetir, avanzar o parar) con la misma lógica
que hoy tiene `onTrackEnded`. Esa lógica es pura y se puede probar con `cargo
test` sin sonido, cosa que hoy no se puede.

Qué cambia en Vue: `App.vue` deja de tener `queue`, `step`, `onTrackEnded`,
`cycleRepeat`, `trayState` y su `watch` profundo (unas 120 líneas); `play(c)`
pasa a ser `invoke('set_queue', ...)`. `Player.vue` y `MiniPlayer.vue`
comparten un composable `usePlayback()` que escucha `danplay://state` y expone
`track, playing, position, duration` y las órdenes. Desaparecen los tres
polls y la mitad del código duplicado entre ambos reproductores
(`fmt`/`tt`, `seek`, `refresh`).

Lo que **no** cambia: la lista visible, la búsqueda, la selección, el detalle,
todo sigue en Vue. Rust solo guarda «qué está sonando y qué viene».

### 2.7 MPRIS (Linux) y SMTC (Windows) con `souvlaki`

Con el estado en Rust, publicarlo al sistema son 40 líneas. `souvlaki` 0.8.3
(MIT) cubre MPRIS por D-Bus en Linux (backend `dbus-crossroads`; la app ya
enlaza `libdbus-1`), SMTC en Windows (pide el `HWND`, que `WebviewWindow::hwnd()`
da) y Now Playing en macOS. Eventos que llegan: Play, Pause, Toggle, Next,
Previous, Seek, SetPosition, SetVolume, Raise, Quit.

Lo que se gana sin diseñar nada: las **teclas multimedia** del teclado y de los
auriculares, el reproductor de Plasma en la bandeja y en la pantalla de bloqueo
(con carátula si le pasamos `cover_url` como `file://` a una miniatura en
caché), y que «Raise» abra tu ventana. En tu sesión ahora mismo hay dos
reproductores MPRIS registrados (Chrome y `mpris-proxy`); DanPlay sería el
tercero y Plasma lo trata igual.

Alternativa mirada: `tauri-plugin-media` 0.1.1. Muy nuevo, callbacks marcados
como parciales. `souvlaki` es más maduro y ya lo usa al menos un reproductor
Tauri 2 en producción (RustMusic).

### 2.8 Dónde aparece el popup, según dónde corra

Esto es lo único que no se puede hacer perfecto en todas partes, y prefiero
decirlo antes:

| Entorno | Posición del popup | Por qué |
| --- | --- | --- |
| Windows | Encima del icono, exacto | el clic trae `position` y `rect` |
| macOS | Debajo del icono de la barra | ídem |
| Linux X11 y **AppImage** | Esquina inferior derecha del monitor, y recuerda dónde la dejes (lo actual, `corner()` + `onMoved`) | el AppImage de Tauri fuerza `GDK_BACKEND=x11` (tauri#15781), así que corre en XWayland y `set_position` funciona |
| Linux **Wayland nativo** (`.deb`, binario de desarrollo, tu caso al depurar) | La coloca el compositor (Plasma: según su política de colocación); se puede arrastrar por la cabecera | Wayland no tiene coordenadas globales; `set_position`, `outer_position` y `always_on_top` son no-ops (tauri#14913). Nadie puede hacerlo mejor sin protocolos que Tauri no expone |

Opción para Wayland si te molesta: fijar `GDK_BACKEND=x11` desde `main()` (la
app entera pasa a XWayland; el AppImage ya vive así). Coste: escalado
fraccional algo borroso. Lo dejaría como ajuste opcional, no como valor por
defecto. Y `ksni` da `activate(x, y)` como pista de posición: en X11 sirve
para poner el popup junto al puntero; en Wayland llega a cero.

### 2.9 Cambios, archivo por archivo

| Archivo | Cambio |
| --- | --- |
| `desktop/src-tauri/Cargo.toml` | `ksni` (Linux), `souvlaki`, `tauri-plugin-single-instance`, `tauri-plugin-positioner` (no Linux); `tray-icon` solo fuera de Linux |
| `desktop/src-tauri/src/tray/mod.rs` | `NowPlaying`, `labels()` (se conserva con sus pruebas), `toggle_popup`, `show_main`, estado `Tray { available }` |
| `desktop/src-tauri/src/tray/linux.rs` | nuevo: `ksni` (boceto 2.4) |
| `desktop/src-tauri/src/tray/desktop.rs` | lo que queda del `tray.rs` actual para Windows/macOS |
| `desktop/src-tauri/src/queue.rs` | nuevo: cola, modos, emisión de `danplay://state` |
| `desktop/src-tauri/src/player.rs` | avisa a la cola al acabar la pista; `nudge_volume`; sin `finished` como bandera |
| `desktop/src-tauri/src/media.rs` | nuevo: `souvlaki` |
| `desktop/src-tauri/src/main.rs` | `on_window_event` y `run` (2.5), `cleanup` solo en `Exit` y con SIGTERM, vigilancia del núcleo, plugin single-instance, comandos de cola, sidecar por `TAURI_ENV_TARGET_TRIPLE`, quitar `zenity` a favor del diálogo de Tauri |
| `desktop/src-tauri/tauri.conf.json` | la ventana `mini` declarada en `app.windows` con `visible:false` (evita construirla en caliente) |
| `desktop/src-tauri/capabilities/mini.json` | **nuevo y urgente**: `"windows": ["mini"]` con `core:event:default`, `core:window:default`, `core:window:allow-set-position`, `core:window:allow-start-dragging` |
| `desktop/src-tauri/capabilities/default.json` | enumerar solo lo que usa el JS (`core:event`, `core:window`, `core:app`, `core:path`) en vez de `core:default`, que arrastra `core:tray`; quitar `dialog:allow-message`, que nadie usa |
| `desktop/src/composables/usePlayback.js` | nuevo: escucha `danplay://state`, expone órdenes |
| `desktop/src/App.vue` | fuera cola/repeat/shuffle/trayState; botón «Mini reproductor» desaparece del menú (ya no hace falta) |
| `desktop/src/components/Player.vue`, `desktop/src/MiniPlayer.vue` | usan `usePlayback`; sin polling |
| `desktop/src/api.js` | `tray.*` se reduce; `native.*` gana las órdenes de cola |
| `tests/smoke.sh` | comprobar que el icono se registra: `busctl --user list \| grep StatusNotifierItem-$PID` |
| `README.md`, `docs/ARQUITECTURA.md` | «mini reproductor en la bandeja» pasa a ser verdad en Linux; explicar Wayland |

### 2.10 Pruebas

- Rust: la máquina de estados de la cola (avanzar, repetir uno, repetir lista,
  «una vez», aleatorio, fin de cola) con `cargo test`, sin audio.
- Rust: `labels()` se conserva tal cual con sus cinco pruebas.
- Vitest: `MiniPlayer` con `usePlayback` simulado (las pruebas actuales de
  `mini-player.test.js` se adaptan; el comportamiento es el mismo).
- Humo: icono registrado en D-Bus; el núcleo sigue vivo con la ventana oculta;
  `Salir` mata al núcleo (ya se comprueba).
- A mano en Plasma: clic, clic central, rueda, tooltip, teclas multimedia,
  pantalla de bloqueo, y en GNOME sin extensión que cerrar cierra.

### 2.11 Riesgos

- `ksni` en GNOME **con** extensión: funciona, pero la extensión traduce a su
  manera (el tooltip rico no se ve, el clic izquierdo abre el menú en algunas
  versiones). Se prueba y, si hace falta, en GNOME se pone el mini reproductor
  como primera entrada del menú, como ahora.
- Dos implementaciones de bandeja son más código que mantener. Se mitiga con
  una interfaz común pequeña (`install`, `update`, `available`).
- Mover la cola a Rust toca el corazón de la reproducción. Por eso va con
  pruebas de la máquina de estados antes de conectar el audio.

---

## 3. Bloque B: Windows

### 3.1 Lo que es solo-Linux hoy

| Dónde | Qué | Alternativa |
| --- | --- | --- |
| `main.rs:15-16, 38-41, 50-51` | `hyperlocal` / `Client::unix` / `UnixUri`: HTTP por socket Unix | En Windows, TCP en `127.0.0.1` con token (3.2). `hyperlocal` solo en `cfg(unix)` |
| `main.rs:27-30` | `XDG_RUNTIME_DIR` para el socket | no aplica en Windows |
| `main.rs:186-199, 208-221` | `--uds`, `prctl(PR_SET_PDEATHSIG)` | ya está en `cfg(target_os = "linux")`. En Windows hace falta un **Job Object** con `KILL_ON_JOB_CLOSE` (`windows-sys`): el vigilante del núcleo (`api.py:640-656`) mira si cambia `os.getppid()`, y en Windows el ppid **no cambia** cuando muere el padre, así que allí no se dispara nunca |
| `main.rs:245-247` | `zenity` para el error de arranque | `tauri-plugin-dialog` bloqueante (ya está instalado) |
| `dependencies.rs` entero | `apt`/`dnf`/`pacman`/`zypper` + `pkexec` + `zenity`/`kdialog` | en Windows no existe: `ffmpeg.exe` y `fpcalc.exe` van **dentro** del instalador (`bundle.resources`) y el núcleo los busca junto al ejecutable |
| `tauri.conf.json` `externalBin`, `build.sh:34-35`, `main.rs:174-175` | el sidecar con triple fijo `x86_64-unknown-linux-gnu` en tres sitios | en Rust, `env!("TAURI_ENV_TARGET_TRIPLE")` (lo define `tauri-build`); el empaquetado espera `danplay-core-x86_64-pc-windows-msvc.exe` |
| `tauri.conf.json:5` | `identifier` `local.danplay.app`: `.local` es el dominio de mDNS y ese nombre bautiza carpetas de datos y el bundle id | `com.github.dani17r.danplay`; cambiarlo **antes** de Windows/Android, después cuesta migrar datos |
| `tauri.conf.json` `bundle.targets`, `icon` | solo `deb`/`appimage`; solo `icon.png` | añadir `nsis`; generar `icon.ico` con `npx tauri icon`; `bundle.windows.webviewInstallMode` |
| `packaging/core.spec` | `console=True` | en Windows abre una consola negra: `console=False` |
| `danplay/api.py:659-686` | `AF_UNIX` en `serve()` | rama TCP con token; uvicorn no sabe de named pipes |
| `danplay/config.py:16-22, 86` | directorios XDG; `chmod 0600` | `platformdirs` (MIT) para `%APPDATA%`/`%LOCALAPPDATA%`; `chmod` es no-op allí |
| `danplay/library.py:716-733` | papelera con `gio trash` | `send2trash` (BSD-3): Windows, macOS y Linux con la misma llamada |
| `danplay/library.py:249, 252, 368, 371, 694`, `cli.py:127` | «está dentro de la biblioteca» se calcula pegando `/` a mano | `os.path.commonpath` sobre `realpath` (también cierra los huecos de seguridad de 5.2) |
| `danplay/names.py:24, 91-93` | `sanitize` no conoce los nombres reservados de Windows (`CON`, `PRN`, `AUX`, `NUL`, `COM1-9`, `LPT1-9`) ni los puntos y espacios finales | añadirlos: un artista «Con» (existe en las pruebas: «Con Poder») no podría tener carpeta |
| `danplay/convert.py`, `fingerprint.py` | `ffmpeg`/`fpcalc` por `PATH` | buscar también junto al ejecutable (`sys.executable` cuando `FROZEN`) |
| `requirements.txt` | `uvloop` no existe en Windows | quitarlo (uvicorn cae a asyncio) o marcarlo `; sys_platform != "win32"` |
| `tests/smoke.sh` | `ss`, `curl --unix-socket`, `pgrep` | versión PowerShell o probar solo en Linux por ahora |

Lo que **no** es problema: `rodio`/`cpal` (WASAPI), `symphonia`, el crate PyO3,
`yt-dlp`, `mutagen`, la interfaz (`api.js` ya usa `http://danplay.localhost`
en Windows).

### 3.2 IPC en Windows: decisión

Windows no tiene sockets Unix utilizables desde uvicorn. Opciones:

- **TCP en `127.0.0.1`, puerto efímero, con token** (recomendada). Rust genera
  32 bytes aleatorios al arrancar, se los pasa al núcleo por variable de
  entorno, y toda petición lleva `Authorization: Bearer <token>`; el núcleo
  rechaza sin él y sigue comprobando `Host`. Es lo más cerca de «solo la app
  habla con el núcleo» que permite Windows, y el mismo código sirve para el
  modo desarrollo en Linux (que hoy va sin token).
- Named pipes con `hyper-named-pipe`: el lado Rust es viable, el lado Python
  no (uvicorn no escucha en pipes). Descartada.

En Linux y macOS se queda el socket Unix `0600` tal cual.

### 3.3 Empaquetado

- **PyInstaller no cruza plataformas**: el `danplay-core.exe` hay que
  construirlo en Windows. Sin máquina Windows, la vía es GitHub Actions
  (`windows-latest`), que compila el núcleo, la app y el instalador NSIS en
  cada etiqueta de versión.
- Compilar la app Rust para Windows **desde Linux** sí se puede (`cargo-xwin` +
  `nsis`, documentado como experimental: solo NSIS, sin MSI, sin firma), pero
  como el sidecar ya obliga a Windows, no compensa mantener dos caminos.
- `ffmpeg.exe` y `fpcalc.exe` dentro del instalador: unos 100 MB más. Alternativa
  ligera: descargarlos al primer uso con verificación de hash. Decisión tuya.
- Pasar el núcleo a **onedir** (hoy es onefile): arranca más rápido (no
  descomprime 41 MB en cada inicio) y en Windows evita el problema conocido de
  que Tauri solo conoce el PID del cargador y no del Python real. Vale también
  para Linux.
- Firma de código: sin certificado, SmartScreen avisará. Es normal en un
  proyecto personal; se documenta.

### 3.4 CI (que hoy no existe)

Un workflow con matriz `ubuntu-latest` + `windows-latest`: pruebas Python,
Vitest, `cargo test`, y en etiquetas de versión los paquetes (`.deb`,
`.AppImage`, `-setup.exe`). Es la primera aportación que pide
`docs/CONTRIBUIR.md`.

### 3.5 Esfuerzo y riesgos

Una a dos semanas de trabajo real más el tiempo de probar en Windows. Riesgo
principal: no tener un Windows a mano para depurar lo que CI no enseña (audio,
rutas con acentos, la papelera). Si tienes una máquina o una VM, baja mucho.

---

## 4. Bloque C: Android (APK)

### 4.1 Lo que Tauri permite y lo que no (verificado)

- Tauri 2 compila para Android (mínimo Android 7, SDK 24; NDK 28+ para las
  páginas de 16 KB que exige Google Play). El WebView es el del sistema
  (Chromium) y los protocolos propios se sirven como `http://danplay.localhost`,
  igual que en Linux: `api.js` ya lo hace bien.
- **No hay sidecar en móvil.** Está confirmado en el issue tauri#9774 y la
  discusión #11454: `externalBin` no funciona en Android ni iOS. El núcleo
  Python (identificación, índice, IA, yt-dlp, mutagen) **no puede ir dentro de
  la APK**. Tampoco hay `ffmpeg`/`fpcalc` que llamar.
- `rodio` funciona en Android (cpal con AAudio), pero un reproductor de verdad
  necesita **servicio en primer plano** para seguir sonando con la pantalla
  apagada y controles en la pantalla de bloqueo; eso lo da
  `tauri-plugin-native-audio` 1.0.5 (Media3/ExoPlayer, Android 8+, admite rutas
  locales y URLs remotas, MIT/Apache).
- Acceso a la música del teléfono: con el almacenamiento por ámbitos de Android
  no se ven rutas, se ven `content://`. Existen `tauri-plugin-android-mediastore`
  (consulta de audio con permisos `READ_MEDIA_AUDIO`) y
  `tauri-plugin-android-fs`. **Escribir etiquetas dentro de los mp3** del
  teléfono (la base del diseño de DanPlay) es posible pero incómodo: Android 11+
  pide confirmación por archivo para modificar medios que la app no creó.
- Plugins oficiales sin móvil: `global-shortcut` y `positioner`. Los demás que
  usas o usarías (`dialog` parcial, `fs`, `notification`, `store`,
  `single-instance`, `window-state`) sí.

Conclusión honesta: **«la misma app en el móvil» no existe con esta
arquitectura.** Lo que sí existe son dos productos posibles.

### 4.2 Opción A: la app móvil habla con tu PC (recomendada primero)

```text
   móvil (Tauri Android: Vue + shell Rust mínimo)
        │  HTTP en la red local, con token de emparejamiento
   PC con DanPlay abierto (núcleo Python + audio servido con Range)
```

- El PC activa en Ajustes «Compartir con el móvil»: el núcleo escucha **también**
  en la red local (puerto fijo, `0.0.0.0`), solo con token. Emparejar = escanear
  un QR en pantalla (dirección + token) o teclear un PIN. Descubrimiento
  automático con mDNS (`mdns-sd`) es un extra.
- El móvil reutiliza **la misma interfaz Vue** (ya es responsive: `useViewport`
  tiene `isPhone`/`isCompact`), con `api.js` apuntando al PC en vez de a
  `invoke`. La búsqueda, las listas, estrellas, favoritos, letras, el asistente
  y hasta lanzar descargas funcionan porque corren en el PC.
- El audio se reproduce en el móvil con `tauri-plugin-native-audio` desde
  `http://pc:puerto/api/song/{id}/audio` (el endpoint existe; `FileResponse` de
  Starlette sirve rangos). Segundo plano y pantalla de bloqueo incluidos.
- Lo que no hay: música sin el PC encendido. Un caché de descargas para
  escuchar sin conexión es viable después (guardar los mp3 elegidos en el
  almacenamiento privado de la app).
- Seguridad: esto abre un puerto, que va contra la filosofía actual. Por eso es
  **opt-in**, apagado por defecto, con token, límite de intentos y aviso visible
  mientras está activo. TLS con certificado autofirmado anclado al emparejar es
  la versión seria; para una red doméstica, token sobre HTTP es aceptable si se
  explica.
- De regalo: con eso mismo, cualquier navegador de la casa puede abrir DanPlay
  (modo web), sin instalar nada.

Esfuerzo: dos a tres semanas, más lo que cueste la primera compilación
Android (4.4).

### 4.3 Opción B: biblioteca local en el teléfono

Requiere un **núcleo en Rust** dentro de la app: índice con `rusqlite`
(`bundled`, que trae FTS5), etiquetas con `lofty` 0.25 (MIT/Apache: lee y
escribe APIC, USLT, POPM, TXXX, TKEY, TBPM y conserva los frames desconocidos),
archivos por MediaStore, reproducción nativa. Sin identificación acústica, sin
IA, sin YouTube en el teléfono (o vía el PC, Opción A).

Tiene un efecto colateral valioso: **el mismo núcleo Rust sustituiría a
`mutagen` en el escritorio** y se acabaría el problema de licencia (GPL dentro
del binario) que documenta `docs/DEPENDENCIAS.md`. Pero es reescribir la parte
más delicada del proyecto (los 352 líneas de `tags.py` y 856 de `library.py`,
con sus 162 pruebas como red). Meses, no semanas. Solo después de A y solo si
quieres ir en esa dirección.

### 4.4 Lo que falta en esta máquina para compilar Android

Tienes `ANDROID_HOME=~/Android/Sdk` y `adb`. Falta:

- JDK (Android Studio trae uno en `jbr/`; `JAVA_HOME` apuntando ahí).
- NDK (Side by side) 28 o superior y `NDK_HOME`.
- SDK Platform, Platform-Tools, Build-Tools y Command-line Tools desde el SDK
  Manager.
- `rustup target add aarch64-linux-android armv7-linux-androideabi
  i686-linux-android x86_64-linux-android`.

Después: `npx tauri android init` (genera `src-tauri/gen/android`), `npx tauri
android dev` con el móvil por USB, y `npx tauri android build --apk` (APK
universal por defecto; se firma con una clave propia para instalarla fuera de
Google Play). `minSdkVersion` 26 por `native-audio`.

### 4.5 iOS

Necesita un Mac con Xcode. Fuera de alcance mientras no lo haya.

---

## 5. Bloque D: mejoras generales

Recoge lo que encontré yo más tres revisiones a fondo por capa (núcleo Python,
interfaz Vue, Rust). Lo marcado **[verificado]** se reprodujo ejecutando código
contra una biblioteca temporal en un directorio de trabajo, sin tocar el repo.

### 5.1 Fallos verificados que van primero

1. **El reescaneo reasigna los ids y las listas se corrompen.** `library.py:443-491`
   hace `DELETE FROM songs` y reinserta todo sin `id`; con `INTEGER PRIMARY KEY`
   sobre tabla vacía los ids vuelven a empezar en 1 en el orden de `os.walk`.
   Cualquier archivo nuevo, renombrado o borrado desplaza los ids de los
   siguientes, y `playlist_songs.song_id` y `downloads.song_id` pasan a apuntar
   a **otras canciones**. **[verificado]**: lista con «Shekinah», se añade
   «Aleluya», reescaneo, la lista contiene «Aleluya». Además `TXXX:LISTAS` se
   escribe en el mp3 (`playlists.py:162`) pero **nadie lo lee** (`tags.py:106-114`),
   así que las listas tampoco sobreviven a perder la base, en contra de lo que
   promete `docs/ARQUITECTURA.md`. Arreglo: escaneo incremental por `path`
   (`UPDATE` si cambió mtime/tamaño, `INSERT` si es nuevo, `DELETE` de lo que ya
   no está) con ids estables, quitar el `rebuild` de FTS (los triggers ya lo
   mantienen) y restaurar las listas desde `TXXX:LISTAS` al reindexar. Con
   prueba: «las listas sobreviven a un reescaneo con un archivo nuevo».
2. **«Resolver duplicados» borra permanentemente cualquier ruta.**
   `api.py:505-530` + `duplicates.py:139-152`: `keep`/`remove` absolutos pasan
   tal cual, `dry_run` es `False` por defecto y se hace `os.remove` (no
   papelera) sin comprobar que la ruta esté bajo una carpeta gestionada ni que
   sea audio. **[verificado]** borrando un `.txt` fuera de la biblioteca con un
   POST. Arreglo: exigir que todas las rutas estén bajo `_roots()`
   (`os.path.commonpath` tras `realpath`), solo `config.EXTENSIONS`, papelera
   con la misma `trash()`, y `dry_run=True` por defecto como en `/api/convert`.
3. **Exportar una lista escribe fuera de la biblioteca.** `playlists.py:200-211`
   monta `LIBRARY/Listas/{name}.m3u8` con el nombre sin sanear; `../../x`
   escribe donde quiera **[verificado]**, y el nombre lo puede fijar el
   asistente. Arreglo: `names.sanitize(name)` y rechazar vacío o `..`, o nombrar
   por `id`.

Y cinco más, pequeños de arreglar y con efecto visible:

4. **La ventanita sin *capability*** (2.1): `capabilities/mini.json`.
5. **«Conservar el archivo original» se ignora al convertir.**
   `SettingsPage.vue:98` manda `keepOne` y la API lee `body.get("keep", False)`
   (`api.py:500`): siempre llega «no conservar». Riesgo de perder originales.
   Un nombre.
6. **Añadir carpeta desde Ajustes falla en silencio.** `SettingsPage.vue:71-78`
   compara con `'confirmar' | 'ya_estaba' | 'reemplaza'`; la API devuelve
   `'confirm' | 'already_there' | 'replaced'` (`App.vue` sí usa los buenos).
   El caso «parece una copia, confirma» vacía el campo y no añade nada. Del
   mismo estilo: `status.ia` (la API dice `ai`, `SettingsPage.vue:283`: la
   insignia siempre dice «sin clave»), calidad `'alta'|'media'` (la API espera
   `high|medium`, `:223-225`: elegir «Media» acaba en 320k en silencio), el
   struct `NowPlaying` sin `blur` (`tray.rs:26-33`: la ventanita nunca difumina),
   `ThemeEditor.vue:14-19` (`CATALOG.noche`, `'oscuro'` y una lista `BASICOS`
   que solo acierta 2 de 6 campos), y `Player.vue:237` que pone clases
   `antes/ahora/luego` mientras el CSS estiliza `.before/.now`
   (`style.css:734-741`): la fila «sonando ahora» no se resalta. Todos son
   restos del paso a inglés; la cura de fondo está en 5.5.
7. **Los `.ogg` no suenan.** `rodio` se compila con `symphonia-vorbis`, que es
   el códec, pero no con el demuxer `ogg`: en el `Cargo.lock` no hay
   `symphonia-format-ogg`. Resultado: «Unrecognized format». `.opus` y `.wma`
   tampoco suenan (symphonia 0.5 no los tiene) aunque el README los lista como
   admitidos. Arreglo: dependencia directa `symphonia` con `features = ["ogg"]`
   (más `alac`/`aiff`, baratos); para opus/wma, mensaje claro en castellano o
   encaminarlos por la conversión con ffmpeg que ya existe; corregir el README.
8. **Tras un `Play` fallido, pausar o buscar reproduce la canción anterior.**
   `player.rs:117-128` hace `sink.take()` pero deja `path`/`duration` de la
   pista de antes; `Toggle`/`Seek` ven «agotada con ruta» y la reabren.
   Actualizar o vaciar `path` en el error.

### 5.2 Seguridad

| Sev. | Dónde | Qué | Arreglo |
| --- | --- | --- | --- |
| Alta | `core/Cargo.toml` (`pyo3 0.23`) | RUSTSEC-2025-0020 (desbordamiento en `PyString::from_object`) y RUSTSEC-2026-0177 | subir a `pyo3 0.29` (la API `Bound` ya se usa; cambio pequeño) |
| Media-alta | `chat.py:524-535, 466-475, 358-392` | Las herramientas destructivas del asistente (`delete_song`, `delete_playlist`, `download_music` hasta 25 archivos, `edit_song`…) se ejecutan en el acto; la única barrera es el texto del `SYSTEM_PROMPT`. Texto no confiable entra por `search_web`, `search_youtube` y letras | reutilizar el mecanismo de `actions` (`chat.py:500-503`, que ya delega en la interfaz): lo destructivo devuelve `needs_confirmation` y solo se ejecuta cuando la interfaz reenvía el sí; límite de llamadas por turno; delimitar el texto externo |
| Media | `main.rs:184`, `api.py:670` | Dos instancias se pisan el socket | `tauri-plugin-single-instance` (2.5); antes de borrar el socket, intentar conectar: si responde, hay otra instancia viva |
| Media | `main.rs:79-94` | El comando `api` acepta cualquier método y ruta bajo `/api/` y **sin timeout**: una petición colgada en Python cuelga la promesa JS para siempre. El WebView es de confianza (no hay ACL propia), así que un XSS equivaldría a la API entera | validar `method` contra `GET/POST/PATCH/DELETE`, `tokio::time::timeout` alrededor de `request()`, y la ACL mínima de 2.9 |
| Media | `main.rs:332-390` | La E/S del protocolo `danplay://` es síncrona dentro del runtime async (`open`, `seek`, `read_exact`) | `tokio::fs` o `spawn_blocking`; cachear id→ruta para no repetir `GET /path` en cada trozo de 2 MB; `cache-control` en portadas |
| Media | `api.py:382-399` | `POST /song/{id}/cover` acepta cualquier archivo legible, lo pasa por `ffmpeg` y luego `GET /cover` lo devuelve: lectura arbitraria encadenada (el comentario de `api.py:26-31` ya lo reconocía) | solo extensiones de imagen, comprobar magic bytes antes de `ffmpeg`, y en modo TCP rechazar rutas fuera de `~`/biblioteca |
| Media | `api.py:203-213, 310, 374, 402, 471` (modo TCP) | Los `POST` sin cuerpo (`/scan`, `/import`, `/song/{id}/blur`, `/enrich`, `/autofill`) son «peticiones simples»: CORS impide leer la respuesta pero no evita el efecto. **[verificado]** desde otro origen | exigir una cabecera propia (`X-DanPlay: 1`, que fuerza preflight) o rechazar `Sec-Fetch-Site: cross-site`; o usar socket Unix también en desarrollo (Vite acepta `socketPath`) |
| Media | `cli.py:344` + `api.py:686` | `serve --host 0.0.0.0` expone la API a la LAN sin autenticación; el chequeo de `Host` lo salta cualquier cliente no navegador | el token de 3.2 resuelve esto y prepara el modo remoto de 4.2 |
| Media | `enrich.py:17-20, 94, 103-104, 246` | Carátulas descargadas sin tope de tamaño ni comprobar `Content-Type`, incrustadas **sin** pasar por `shrink_image` (solo las del usuario se encogen); una página de error acaba dentro del mp3 como `image/jpeg` | `r.read(LIMIT+1)`, exigir `image/*`, magic bytes, encoger antes de incrustar; validar que el id de MusicBrainz sea UUID |
| Media | `youtube.py:98-100, 108-109, 143, 294` | Cualquier `http(s)://` no-YouTube se pasa a yt-dlp (su extractor genérico descarga de cualquier host, incluidos internos); `is_url` compara por subcadena; `info()` no pone `noplaylist` y `download` recorre **toda** la lista; sin `max_filesize` ni filtro de duración | lista blanca por `urlsplit().hostname`, `noplaylist` también en `info`, `playlist_items: "1:50"`, `max_filesize`, `match_filter` por duración |
| Media | `main.rs:368, 384`, `api.py:283, 576` | El audio se sirve como `audio/mpeg` sea cual sea el formato | tipo por extensión (`mime_guess` en Rust, `mimetypes` en Python) |
| Media | `main.rs:376-378` | Sin cabecera `Range` se lee **el archivo entero** en memoria | responder siempre por trozos |
| Baja | `api.py:256-261` + `library.py:646-655` | `PATCH /song/{id}` pasa el cuerpo entero a `edit(**body)`: se pueden fijar `stars`/`favorite` en la base sin escribir el POPM; una clave inesperada da 500 | conjunto `EDITABLE` y modelo pydantic |
| Baja | `api.py:670-677`, `main.rs:28` | El socket nace con la umask del proceso y se hace `chmod` después; sin `XDG_RUNTIME_DIR` cae a `/tmp/danplay.sock` (compartido, predecible) | `os.umask(0o077)` alrededor del `bind`; respaldo en `~/.cache/danplay/` |
| Baja | `convert.py:138` | Imagen temporal con nombre predecible en `/tmp` | `NamedTemporaryFile` / `mkdtemp` |
| Media | `dependencies.rs:71-127` | La app instala paquetes del sistema con `pkexec sh -c "<cadena>"` en **cada** arranque (también en el `.deb`, donde `Depends` ya lo garantiza); los nombres son constantes (no hay inyección hoy), pero es un shell root con una cadena, falla en Fedora/openSUSE sin repos extra y no recuerda «Ahora no» | como mínimo `pkexec apt-get install …` sin `sh -c` y saltarlo bajo `/usr`; mejor: `ffmpeg`/`fpcalc` dentro del AppImage (`bundle.resources`) o `rusty-chromaprint` en el crate PyO3 y retirar el módulo |
| Baja | `tauri.conf.json:26` | CSP correcta en lo esencial (inline/eval bloqueados, `connect-src` hereda `'self'`), pero sin `object-src`, `base-uri`, `frame-src`, `form-action`; `img-src data:` no lo usa nadie | `script-src 'self'; object-src 'none'; base-uri 'none'; frame-src 'none'; form-action 'none'`; probar sin `'unsafe-inline'` en estilos |
| Baja | `ChatPage.vue:83` | Los últimos 60 mensajes del chat se guardan en claro en `localStorage` | guardarlos en el índice SQLite del núcleo, o al menos limitar y ofrecer borrado (ya existe) |
| Baja | cuerpos JSON sin modelo ni tamaño (20 endpoints) | `lyrics` de 200 MB acaba en el USLT; `/chat` reenvía `messages` sin recortar | modelos pydantic con `max_length`; middleware que limite `Content-Length` |
| Bien | `tauri.conf.json` CSP, `Icon.vue`, protocolo `danplay://` | CSP sin inline ni `eval`, el único `v-html` pinta constantes propias, el id del protocolo se valida como dígitos y el 416 de rangos es correcto | — |
| Bien | SQL, subprocesos, yt-dlp, secretos | Sin inyección SQL (identificadores de constantes, valores parametrizados, FTS escapado con prueba); subprocesos siempre con lista, sin `shell`, con `timeout`; yt-dlp con `outtmpl` por id en temporal privado y `noplaylist` en descarga; `danplay.env` 0600 y la clave nunca sale entera | — |

### 5.3 Rendimiento

- **La API entera se congela mientras se hashean duplicados.** En
  `core/src/lib.rs` solo `analyze_many` suelta el GIL (`py.allow_threads`,
  línea 69); `hashes`, `full_hashes`, `analyze` y `chromagrams` lo retienen
  durante todo el trabajo de rayon. Se llaman desde una petición
  (`duplicates.py:27`): mientras dura, ningún hilo Python responde. Un
  `allow_threads` por función (los datos ya son `Vec<String>` propios).
- **`/api/status` en cada sondeo hace un agregado sobre toda la tabla**
  (`library.py:848-856`) evaluando `lyrics != ''`, la columna más pesada; la
  interfaz lo consulta continuamente. Cachear unos segundos o invalidar al
  escribir; a medio plazo, las letras a una tabla aparte.
- **El escaneo reescribe la tabla entera** (5.1) con triple trabajo FTS
  (trigger por fila borrada, por fila insertada y `rebuild` final) y una
  transacción de escritura abierta todo el tiempo: puntuar mientras escanea
  falla con «database is locked». El escaneo incremental lo arregla de raíz.
- **Dos `/api/scan` a la vez** compiten por el `DELETE` (`api.py:53-58,
  203-213`); `JOBS` nunca se limpia. `Lock` por trabajo y 409 si ya corre.
  Lo mismo en `youtube.STATE["active"]` (`api.py:604` vs `youtube.py:364-367`).
- **`download_music` del asistente descarga hasta 25 temas en serie dentro de
  la petición `/api/chat`** (`chat.py:370-375`): minutos de espera. Encolar con
  `run_job`, que ya publica en `STATE`, y devolver «en marcha».
- **Polling → eventos** (2.6): quita tres bucles y el `watch` profundo de
  `App.vue:415`.
- **`duration_of` decodifica el archivo dos veces** (`player.rs:64-70, 122`):
  la duración ya está en el índice; pásala con la orden `Play`.
- **Añadir N canciones a una lista reescribe N mp3** (`playlists.py:81-98,
  138-162`) y cambia sus mtime, con lo que el siguiente escaneo los relee; como
  `LISTAS` no se lee nunca, hoy es coste sin beneficio.
- `index_file` reabre conexión y relista `Artistas/` por archivo
  (`library.py:693-698`), y `/api/import` y las descargas lo llaman en bucle;
  cuatro parseos de etiquetas por archivo importado (`ingest.py:42, 65, 165`).
  Pasar `roots`/vocabulario ya calculados y un solo `read_all`.
- **Núcleo onefile** (3.3): 41 MB descomprimidos en cada arranque. Onedir.
- **AppImage de 144 MB**: el grueso es el núcleo y GTK; quitar de `core.spec`
  lo que no se usa (5.4) lo recorta.
- **Vista agrupada**: `GroupedSongs.vue:45-74` monta **una lista virtual por
  grupo** (cada una con su `scroll`, `ResizeObserver` y rAF sobre el mismo
  panel) y como casi ningún grupo pasa del mínimo de 80 filas, todo se pinta
  entero: «Por artista» con 200 artistas son 200 observadores y la biblioteca
  completa en el DOM. Virtualizar la lista aplanada (cabeceras como filas) o,
  como paso barato, `content-visibility: auto` por grupo.
- **Miniaturas a tamaño completo**: cada portada de 30-48 px decodifica la
  APIC entera (a menudo 0,5-2 MB) y el protocolo `danplay://cover` la reenvía
  sin redimensionar ni cabeceras de caché. Un endpoint de miniatura
  (`?size=96`) con `Cache-Control` y `decoding="async"` en el `<img>`.
- **Respuestas que se pisan**: `load()`, `select()` y `play()`
  (`App.vue:218-241, 284, 293`) no secuencian; con el debounce de 180 ms
  todavía puede llegar «bar» después de «barak» y pisar la lista. Un contador
  de petición y descartar las viejas.
- `App.vue` guarda hasta 1.000 canciones en un `ref` profundo; `shallowRef`
  para la lista, pero solo después de que todas las actualizaciones pasen por
  sustituir el elemento (hoy hay `Object.assign` in situ en `:423-429, 578`).
  `nowPlaying` (`App.vue:392-393`) hace `queue.concat(songs).find()` sobre
  hasta 2.000 elementos y se recalcula al tocar cualquier canción.
- **`style.css` (1.245 líneas)** crece por parches al final: `thead th`,
  `td`, `.stars`, `.queue`, `.queue-row`, `.pl-btn` definidos tres veces cada
  uno, y reglas muertas (`.searchbox*`, `.pl-speed`, `.pl-volume`,
  `.queue-row.before/.now`). Partir por áreas, `@layer` y `stylelint`.
- Menores: caché de carátulas con entradas `None` que nunca se desalojan
  (`tags.py:316-343`); `fingerprint.AVAILABLE` calculado al importar (instalar
  `fpcalc` exige reiniciar); `process.extract` compara cada clave contra las n
  (`duplicates.py:109-110`, la mitad redundantes).

### 5.4 Paquetes

Python. Importaciones reales del núcleo (AST + grep): `fastapi`, `uvicorn`,
`openai`, `mutagen`, `rapidfuzz`, `yt_dlp`, `acoustid`, `dotenv`, `watchdog`
(solo `cli watch`) y `danplay_core`. Todo lo demás:

| Paquete | Situación | Qué hacer |
| --- | --- | --- |
| `musicbrainzngs` | **no se importa en ningún sitio** (solo figura como import oculto en `core.spec:13`) | quitar de ambos |
| `maturin`, `-e ./core` | herramienta de build y crate «opcional» como dependencia de ejecución: `pip install -r` exige toolchain Rust | a `requirements-dev.txt` |
| `websockets`, `watchfiles`, `PyYAML`, `python-multipart`, `uvloop`, `httptools` | extras de `uvicorn[standard]` sin uso (no hay WS, ni `--reload`, ni formularios); `uvloop` no existe en Windows | quitar los seis y el import oculto `uvicorn.protocols.websockets.websockets_impl` de `core.spec:16` |
| `httpx2`, `httpcore2`, `truststore` | los exige `openai==3.8.0`; no es typosquatting (`pip show` confirma el origen) | mantener |
| `audioread`, `audioop-lts`, `standard-*` | los exige `pyacoustid` en Python 3.13, pero el código solo usa `fpcalc`: peso muerto | excluirlos en `core.spec` (`excludes`) |
| `pytest` | lo usa `scripts/test.sh` y **no está declarado** | a `requirements-dev.txt` |
| Versión de Python | `theory.py:24` usa `re.NOFLAG` (3.11+) pero `core/pyproject.toml` declara `>=3.9` y `danplay/` no declara nada | `requires-python = ">=3.13"` |

Rust: `rodio` 0.20 → 0.22 (API nueva `OutputStreamBuilder`, cambio mecánico;
0.21+ trae además `with_error_callback` para enterarse de que se fue el
dispositivo de audio), `symphonia` con `ogg`/`alac`/`aiff` (5.1.7), `pyo3` 0.23
→ 0.29, `tokio` con `features = ["full"]` cuando basta `rt` y `net`. Plugins
que faltan: `single-instance` (imprescindible con la app en bandeja),
`window-state` (tamaño/posición de la principal, que hoy abre siempre a
1440×900), `positioner` (Windows/macOS), `notification` (aviso de «sigo en la
bandeja»), `os` (sustituye el sniffing de `navigator.platform` en
`api.js:45-46`), `opener` cuando haya enlaces, `clipboard-manager`
(`CopyButton.vue:36-44` usa `execCommand('copy')`, obsoleto), `autostart`
(arrancar en bandeja) y, para MPRIS/SMTC, `souvlaki`.

Frontend: Vite 6 → 8 (Rolldown) con `@vitejs/plugin-vue` 6 a la vez. Este
proyecto no usa `rollupOptions` ni plugins raros, así que la migración es
cambiar versiones y probar. Faltan las herramientas que habrían cazado los
fallos de 5.1.6: ESLint + `eslint-plugin-vue`, Prettier, `stylelint`
(selectores duplicados), tipos (`jsconfig.json` con `checkJs` y JSDoc en
`api.js`, o TypeScript con `vue-tsc`) y `@vitest/coverage-v8` (el «232 en
verde» esconde que los flujos grandes de `App.vue` no se prueban). `heroicons`
está en `package.json` pero el script que «genera `icons.js`» no existe:
añadir `scripts/icons.mjs` o quitar la dependencia. Declarar `engines`
(jsdom 30 exige Node ≥ 24.15, justo lo instalado) y un script `test`.

### 5.5 Refactor y calidad

- **Ediciones en archivos que no son mp3 se pierden en silencio**
  (`tags.py:33-45` + `library.py:668-679`): `_id3` solo sabe de `MP3()`; en
  `.flac/.m4a/.ogg/.wav` (todos admitidos) `write`/`rate`/`set_favorite`
  devuelven `False`, `library.edit` **ignora el retorno**, el índice cambia y
  el archivo no. Usar `mutagen.File`/`FLAC`/`MP4` según formato y propagar el
  fallo a la respuesta.
- `enrich(details=True)` guarda `album/year/genre` de la IA **sin pasar por
  `_dato_util`** (`enrich.py:250-259`), que solo usa `autofill`: el bug de
  «desconocido» sigue vivo por esa puerta.
- **Cero `logging` en el núcleo**: `_safe` de `ingest.py:75-85` traga hasta
  `TypeError`; `enrich`, `youtube`, `tags` devuelven `False` sin rastro. Un
  `logging.getLogger("danplay")` con `warning(..., exc_info=True)`.
- **Una sola fuente de verdad para los nombres de la API.** Cinco de los
  fallos de 5.1.6 son «un nombre que dejó de existir al pasar a inglés», y los
  mocks de las pruebas (`reactivity.test.js:5-84`, `menus.test.js:9-41`) están
  escritos a mano con nombres muertos (`quitarCarpeta`, `estadoFalso.ia`), así
  que nunca fallan. Generar los tipos del OpenAPI de FastAPI (o JSDoc +
  `checkJs`) y construir los mocks recorriendo el `api` real.
- `App.vue` (1.072 líneas): con la cola fuera (2.6), extraer además
  `useNotices` (hoy los hijos reemiten `@notice` en cadena), `useDialog`
  (`ask`; por no tenerlo, `SettingsPage.vue:38,99` y `ChatPage.vue:124` siguen
  usando `confirm()`/`alert()` nativos), `useContextMenu`, `useSearch`
  (`App.vue:613-689`), `usePreferences` (tema, densidad, tamaño, vista) y sacar
  a componentes las páginas que están inline (Bienvenida, Entrada, Duplicados,
  ~150 líneas de plantilla). `columns` (`App.vue:209`) es siempre `null` y
  `SongTable.vue:50-57` lo consume: código muerto. Objetivo: menos de 400
  líneas de orquestación. `DetailsPanel.vue` (395) también pide partirse en
  ficha, edición y acordes.
- **Atajos globales que pisan controles** (`Player.vue:119-134`): Espacio con
  un botón enfocado alterna la reproducción y anula el botón (el diálogo enfoca
  el botón principal, así que Espacio ya no confirma); las flechas dentro de
  `SelectField` mueven además el volumen; `n/p/s/r/m` actúan con un modal o un
  menú abiertos. Un `useHotkeys` con ámbitos.
- **Accesibilidad**: estrellas, corazón, filas y azulejos no son enfocables ni
  operables con teclado; `Drawer`/`ModalDialog` no atrapan ni restauran el
  foco; el menú contextual no tiene `role="menu"` ni flechas; los avisos no
  llevan `aria-live`. Un `useFocusTrap` compartido y `tabindex`/roles.
- **Texto visible sin tildes** («Busqueda», «Titulo», «Cancion», «Genero»…):
  la regla «sin acentos» es de los nombres de archivo, pero se coló en la
  interfaz y se lee como faltas de ortografía. Una pasada, o un `strings.js`.
- **`minWidth: 1080`** (`tauri.conf.json:15-16`) hace inalcanzable dentro de
  la app toda la capa responsive (`≤1000 px`), y a `≤700 px` el reproductor
  desborda porque `style.css:1104` apunta a clases que no existen. Decidir:
  bajar el mínimo (720, para ventanas en mosaico) o dejar esa capa solo para
  el modo remoto/móvil de 4.2, donde sí hace falta.
- **Crate PyO3**: rutas como `Vec<String>` (un nombre no UTF-8 tumba el lote
  entero: `Vec<PathBuf>`), fallos como `None` sin motivo, y `analyze`,
  `analyze_many`, `chromagrams` **no se llaman desde ningún módulo Python**
  (solo los hashes tienen fallback): o se usan o se retiran.
- **Reproductor**: `panic = "abort"` (`Cargo.toml:33`) hace que un archivo
  corrupto que provoque un panic en el hilo de audio tumbe la app entera
  (valorar `unwind` + `catch_unwind` en ese hilo); si se desconecta el
  dispositivo, el stream muere en silencio con `playing = true` y la posición
  congelada (detectar estancamiento y recrear, o el callback de rodio 0.21+).
  Gapless sale casi gratis encolando la siguiente pista en el mismo `Sink`.
- `Player.vue` y `MiniPlayer.vue`: `usePlayback` compartido (2.6). El
  formateador de duración está copiado nueve veces y el
  `String(e).replace(/^Error:\s*/, '')` diez: `utils/format.js` y normalizar
  el error en `api.js`.
- `chat.py:287-485` (`run_tool`, 200 líneas de `if name ==`) y `:550-603`:
  un diccionario `{nombre: (handler, resumen)}` junto a cada herramienta; ya se
  desincronizó una vez (`chat.py:273` cita `buscar_letra`, que no existe).
- Duplicación: `api.py:409-423` y `chat.py:329-342` (acordes cacheados);
  `api.py:382-399` reimplementa `tags.cover_from_file`.
- Estado global mutable: `config` con `setattr` en caliente (`api.py:108-119`),
  `JOBS`, `youtube.STATE`, `ai._client`, cachés sin lock. Un objeto `Settings`
  que se reemplaza atómicamente y un registro `Jobs` con lock.
- Validación: 20 endpoints con `body: dict`; `int(body.get(...))` puede dar 500
  (`api.py:597, 616`); `api.py:349-350` guarda `ids` como texto y revienta
  **después** del commit. Modelos pydantic.
- `str(frame)` de mutagen une valores múltiples con NUL (`tags.py:87`): un
  TPE1 multivalor rompe `shutil.move` con «embedded null byte».
- `playlists.create` con `INSERT OR IGNORE` sobre `name UNIQUE` devuelve la
  lista existente: el chat «crea» y en realidad añade a otra.
- Imports sin uso en `api.py:6-16` (`io`, `shutil`, `subprocess`, `Any`,
  `StreamingResponse`, `names`, `web`); `core_entry.py:11-13` `--uds` sin
  valor → `IndexError`.
- Español e inglés mezclados en identificadores contra la regla del proyecto:
  `sonando`, `agotada`, `espera` (`player.rs`), `conservar_original`
  (`convert.py:52`), `escribir_tags` (`ingest.py:105`), `permitir_ia`
  (`enrich.py:49`), `semitonos` y `existe` en la API (`api.py:452, 252`),
  `_CANCELAR`, `bucle`/`intervalo` (`api.py:640-656`).

### 5.6 Pruebas que faltan

Frontend: 47 de las 63 pruebas de `design.test.js` leen el código fuente con
`readFileSync` y expresiones regulares (exigen que aparezca literalmente
`sortDesc.value = !sortDesc.value`, o la cadena `<SongRows`): cualquier
refactor de los propuestos las rompe sin cambiar el comportamiento. Conservar
las guardas que aportan (props inexistentes, iconos, colores fijos) y convertir
el resto en pruebas montadas. `Player.vue` no tiene pruebas directas (el fin de
pista se simula emitiendo `trackEnded` a mano), y `api.js`, `SearchPanel`
(la gramática de búsqueda es pura y no tiene ni una prueba), `SettingsPage`,
`DownloadsPage`, `ThemeEditor` y `ContextMenu` no tienen ninguna. Para la
bandeja nueva, una prueba de integración con ventana real (`tauri-driver`) es
lo único que habría detectado el problema de la *capability*.

Python: las suites dependen de `~/Musica/Artistas/Barak` o `DANPLAY_TEST_MUSIC`: en CI
se saltarían casi todas. Generar mp3 sintéticos con `ffmpeg -f lavfi -i
anullsrc` en un fixture las hace fiables en cualquier máquina. Y faltan pruebas
para lo crítico: ids estables tras reescanear (habría cazado 5.1.1),
contención de rutas en `/duplicates/resolve` y `/song/{id}/cover`, saneado en
`export_m3u`, el 421 del `Host` y el comportamiento sin cuerpo en modo TCP,
`chat.reply` con un cliente OpenAI falso (bucle de herramientas, argumentos
malformados), topes de `youtube.download` e `enrich.cover` con red simulada,
escritura de etiquetas en no-mp3, `names.sanitize` con nombres reservados de
Windows. `test_api.py:92` pide `/api/cancion/999999` (ruta que ya no existe):
el 404 no prueba nada.

## 6. Orden propuesto

| Fase | Contenido | Tamaño | Depende de |
| --- | --- | --- | --- |
| **0. Higiene** | `capabilities/mini.json`; los siete nombres desajustados de 5.1.5-6; demuxer OGG; `Play` fallido; single-instance; `pyo3` 0.29 y `allow_threads`; quitar paquetes sin uso; CI con las pruebas; Vite 8 | 2-3 días | — |
| **0b. Datos a salvo** | escaneo incremental con ids estables (5.1.1), contención de rutas en duplicados y exportación (5.1.2-3), confirmación del lado servidor para el asistente, límites en carátulas y yt-dlp | 3-4 días | — |
| **1. Bandeja** | `ksni` en Linux, popup, cerrar=ocultar, salir desde la bandeja, vigilancia del núcleo, aviso de primera vez, MPRIS | 1 semana | 0 |
| **2. Cola en Rust** | `queue.rs`, eventos, `usePlayback`, adelgazar `App.vue`, pruebas de la máquina de estados | 1 semana | 1 (se pueden solapar) |
| **3. Windows** | TCP+token, `platformdirs`, `send2trash`, recursos, NSIS, CI en Windows | 1-2 semanas + pruebas en Windows | 0 |
| **4. Android, modo remoto** | compartir en LAN con token y QR, app móvil, `native-audio`, APK firmada | 2-3 semanas | 2, 3 (la parte del token) |
| **5. Largo plazo (opcional)** | núcleo Rust (`lofty` + `rusqlite`), biblioteca local en el móvil, adiós `mutagen` | meses | 4 |

Lo que **no** haría: esperar a `linux-ksni` de Tauri; intentar meter Python en
la APK (Chaquopy/BeeWare no encajan con Tauri); usar `tauri-plugin-positioner`
en Linux; cambiar a GTK4 o a otro framework.

## 7. Decisiones que necesito de ti antes de tocar nada

1. **Móvil**: ¿te vale empezar por el modo remoto (el móvil necesita el PC
   encendido), o solo te interesa si la música vive en el teléfono? Cambia
   todo el bloque C.
2. **Popup en Wayland**: ¿aceptas que en `.deb`/desarrollo lo coloque Plasma y
   que solo el AppImage y Windows lo peguen al icono, o prefieres que la app
   fuerce X11 por defecto (posición exacta a cambio de un escalado menos fino)?
3. **La cola a Rust** en la misma tanda que la bandeja (recomendado) o después.
4. **Windows**: ¿hay una máquina o VM Windows para probar, o vamos solo con CI?
5. **`ffmpeg`/`fpcalc` en Windows**: ¿dentro del instalador (+100 MB) o
   descarga al primer uso?
6. **Rumbo a largo plazo**: ¿te interesa la Opción B (núcleo Rust) también por
   lo de la licencia de `mutagen`, o el escritorio se queda en Python?
7. **Instalar paquetes del sistema al arrancar** (`dependencies.rs`): ¿lo
   retiramos y metemos `ffmpeg`/`fpcalc` en el AppImage, o se queda?
8. **Ventana mínima**: ¿bajar `minWidth` a 720 para que la capa compacta sirva
   en el escritorio, o reservarla para el móvil?

## 8. Fuentes

- Tauri, bandeja: <https://v2.tauri.app/learn/system-tray/> («Linux: unsupported»)
- tray-icon#104, clics en Linux: <https://github.com/tauri-apps/tray-icon/issues/104>
- tray-icon#336, Plasma 6/Wayland: <https://github.com/tauri-apps/tray-icon/issues/336>
- PR `linux-ksni` (abierto): <https://github.com/tauri-apps/tauri/pull/12319> · issue <https://github.com/tauri-apps/tauri/issues/11293>
- `ksni` 0.3.6: <https://docs.rs/ksni/latest/ksni/> · <https://github.com/iovxw/ksni>
- Wayland sin posicionamiento: <https://github.com/tauri-apps/tauri/issues/14913>
- AppImage fuerza X11: <https://github.com/tauri-apps/tauri/issues/15781>
- Cerrar a la bandeja: <https://github.com/orgs/tauri-apps/discussions/2684>
- single-instance: <https://v2.tauri.app/plugin/single-instance/>
- positioner: <https://docs.rs/crate/tauri-plugin-positioner/latest>
- `souvlaki`: <https://docs.rs/souvlaki> · `tauri-plugin-media`: <https://github.com/Taiizor/tauri-plugin-media>
- Throttling de temporizadores en Chromium: <https://developer.chrome.com/blog/timer-throttling-in-chrome-88> · <https://chromestatus.com/feature/5580139453743104>
- Sidecar no disponible en móvil: <https://github.com/tauri-apps/tauri/issues/9774> · <https://github.com/orgs/tauri-apps/discussions/11454>
- Requisitos Android: <https://v2.tauri.app/start/prerequisites/> · Google Play/16 KB: <https://v2.tauri.app/distribute/google-play/>
- Tabla de plugins por plataforma: <https://v2.tauri.app/plugin/>
- `tauri-plugin-native-audio`: <https://github.com/uvarov-frontend/tauri-plugin-native-audio>
- MediaStore en Android: <https://lib.rs/crates/tauri-plugin-android-mediastore> · <https://github.com/aiueo13/tauri-plugin-android-fs>
- Windows desde Linux (experimental): <https://v2.tauri.app/distribute/windows-installer/>
- `hyperlocal` (solo Unix): <https://github.com/softprops/hyperlocal>
- `lofty` 0.25: <https://docs.rs/lofty> · `rusqlite`: <https://github.com/rusqlite/rusqlite>
- `pyo3` 0.29: <https://github.com/pyo3/pyo3/releases> · `rodio` 0.22: <https://docs.rs/crate/rodio/latest>
- Vite 8 / Rolldown: <https://vite.dev/guide/migration>
