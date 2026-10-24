# Contribuir

Gracias por mirar. Aquí está lo que hace falta para montar el entorno, cómo se
prueban las cosas y en qué se puede ayudar.

> **Antes de nada, lee la [licencia](../LICENSE).** Este proyecto es *código a
> la vista*, no código abierto: puedes usar la app y leer el código, pero no
> mantener tu propia versión aparte ni distribuir copias modificadas.
>
> Eso **no impide** proponer cambios sobre este repositorio, y se agradecen.
> Al mandar un pull request cedes los derechos de esa aportación al autor,
> para que pueda seguir bajo la misma licencia que el resto del proyecto. Si
> eso no te encaja, abre un issue describiendo la idea en vez de mandar código.

---

## Montar el entorno

```bash
git clone https://github.com/dani17r/danplay.git && cd danplay

cd desktop && vp install && cd ..
cp .env.example .env
```

La app y su núcleo son Rust: con Node 24, Vite+ y Rust basta para
compilarla y probarla. No hace falta Python: el núcleo de Python de hasta
la 1.30 salió del repositorio en la 1.40, con sus pruebas ya pasadas a Rust
(ver [la migración](MIGRATION.md)).

La interfaz (`desktop/`) va con [Vite+](https://viteplus.dev): `vp` es Vite
(`vp dev`, `vp build`), Vitest (`vp test`), el formateador (`vp fmt`, Oxfmt)
y el que lanza los scripts de `package.json` (`vp run lint`). Se instala una
vez con `curl -fsSL https://vite.plus | bash`. Sus dependencias las gestiona
Bun, que `vp` baja solo si no lo tienes: `package.json` y `bun.lock` las
fijan, y `bunfig.toml` no deja entrar nada con menos de dos semanas.
Añadir o subir una: `vp add paquete` / `vp update paquete`. Vite+, `vite` y
`vitest` van juntos (`vite` es un alias de su núcleo): para subirlos,
`vp migrate` en `desktop/`.

Las de Rust las fija el `Cargo.lock` de la raíz, uno para todo el
workspace.

Los modelos que viajan con la app ya están en `data/`: los grafos del
separador de pistas (`data/separator/`, también el del Mega53) y el de
Beat This! (`data/pulse/`). Se generaron con guiones de Python que
necesitaban PyTorch y el núcleo de Python (`scripts/export-separator.py`,
`export-mega53.py` y `export-pulse.py`); se fueron con él y quedan en el
historial, en `a0e9317^` (`git show a0e9317^:scripts/export-separator.py`).
Solo harían falta si cambia una red o el modo de exportarla. Lo mismo con
los que midieron qué redes usar (`scripts/evaluate-separator.py`, con
MUSDB18, y `evaluate-pulse.py`, con GTZAN): antes de cambiar algo del
separador o del pulso, que lo nuevo gane ahí.

Opcional, para la huella acústica y la conversión de formatos:

```bash
sudo apt install libchromaprint-tools ffmpeg
```

Sin ellos la app funciona igual, solo que con esas dos funciones desactivadas
y diciéndolo en Ajustes.

## Ejecutar

Mientras desarrollas, la app entera en modo desarrollo:

```bash
./scripts/dev.sh                    # con tus datos de siempre
./scripts/dev.sh --sandbox ~/Musica # con datos aparte y una COPIA de esas canciones
```

Es la app de escritorio de verdad (Tauri) con la interfaz servida por Vite:
lo que cambias en `desktop/src` se ve al momento, y el núcleo, que va dentro,
se compila del código del proyecto (`server/`, `library/`, `analysis/`) con
los datos de `data/`: un cambio en Rust se ve cuando `tauri dev` la
vuelve a compilar, sola, al guardar. La compilación de desarrollo usa su
propio identificador y su nombre en MPRIS y en la bandeja: convive con
DanPlay instalado sin pisarle nada.

`--sandbox` guarda ajustes, base e índice en `.dev/sandbox/` (fuera de git) y
copia allí las canciones que le des: la app escribe etiquetas y la importación
mueve archivos, y así tu música no se toca. Sin carpeta, genera unas
canciones con ffmpeg. `--sandbox --clean` empieza de cero.

En un clon recién hecho, `desktop/src-tauri/data/` y `desktop/src-tauri/tools/`
solo traen su LEEME: `./scripts/dev.sh` baja ONNX Runtime la primera vez
(`scripts/data.sh`, a una caché, y de ahí a `desktop/src-tauri/data/`, de
donde lo toman la app y las pruebas) y ffmpeg sale del sistema. Lo que viaja
dentro de los paquetes lo ponen `scripts/data.sh` y `scripts/tools-linux.sh`.

Para la versión que se instala:

```bash
./scripts/build.sh     # la interfaz, los datos y la app, en orden
./danplay-app.sh       # la app de escritorio compilada, con el núcleo dentro
```

Y solo la interfaz en el navegador, con el núcleo por un puerto:

```bash
cargo run -p danplay-server                # el núcleo, en el puerto 8730
cd desktop && vp dev                       # Vite en el 5273, con proxy
```

## La app del celular

`mobile/` es la app de Android (e iOS, cuando se compile en un Mac). Su
interfaz es la de la PC (`desktop/mobile.html` → `desktop/src/mobile/`), así
que se prueba con las mismas herramientas: `vp test` en `desktop/` incluye
`tests/mobile.test.js`, y `cargo test -p danplay-engine -p danplay-mobile`
prueba el motor de pistas y lo que la app contesta en vez del núcleo de la
PC.

Para compilarla hace falta, en `~/Android`:

- un JDK 17 (`~/Android/jdk-17`);
- el SDK con `platform-tools`, una plataforma, `build-tools` y el NDK 28 o más
  nuevo (`sdkmanager "platform-tools" "platforms;android-36"
  "build-tools;36.1.0" "ndk;29.0.14206865"`); Gradle baja la plataforma que
  pida el proyecto;
- los destinos de Rust: `rustup target add aarch64-linux-android
  armv7-linux-androideabi x86_64-linux-android i686-linux-android`.

```bash
./scripts/build-android.sh             # el APK firmado, a dist/installers/
./scripts/build-android.sh --emulator  # el de depuración, para el emulador
cd mobile && bun run tauri android dev      # con el teléfono por USB
```

O sin nada de lo anterior instalado: `./scripts/github.sh packages android`
lo compila y lo firma en GitHub (la llave viaja en dos secretos del
repositorio; cómo se ponen o se cambian, en la cabecera de
`.github/workflows/android.yml`).

La **llave de firma** vive fuera del repositorio, en `~/Android/claves/`
(`danplay-pistas.jks` y `keystore.properties`). Guárdala con las copias de
seguridad: un teléfono solo acepta una actualización firmada con la misma
llave, y si se pierde hay que desinstalar la app en cada teléfono.

Para probarla sin teléfono, un emulador sin ventana y sin sonar por los
altavoces:

```bash
avdmanager create avd -n danplay -k "system-images;android-35;google_apis;x86_64" -d pixel_7
emulator -avd danplay -no-window -gpu swiftshader_indirect -audio none &
adb install -r mobile/gen/android/app/build/outputs/apk/universal/debug/app-universal-debug.apk
```

La versión de depuración deja inspeccionar la página como en Chrome
(`chrome://inspect`, o `adb forward tcp:9222
localabstract:webview_devtools_remote_<pid>` y el protocolo de depuración).

## Las pruebas

```bash
./scripts/test.sh
```

Es lo mismo que comprueba la CI en cada push, y todo tiene que pasar:

| Tanda | Qué cubre |
| --- | --- |
| Interfaz | ESLint, stylelint, Oxfmt (formato), vue-tsc (tipos) y Vitest: componentes, reactividad, contratos, listas grandes |
| Interfaz (URLs) | cómo se construyen las rutas de medios en cada sistema |
| Rust | rustfmt, clippy sin avisos y las pruebas del workspace: el núcleo (`danplay-library`, `danplay-analysis` y `danplay-server`: nombres, etiquetas, índice, la API sobre una biblioteca temporal de verdad, IA, descargas, vigilante de carpetas, el pulso, separar), el motor, la app de escritorio (reproductor, cola, bandeja, permisos por ventana) y la del celular |
| e2e | la interfaz de verdad contra el núcleo de verdad (`danplay-server`, que `scripts/test.sh` compila antes), en Chrome (Playwright) |
| Humo | la app **ya compilada** o empaquetada: el núcleo dentro y sin puertos, sus datos y programas, el pulso, la instancia única, el ciclo de vida |

`./scripts/test.sh --quick` se salta las e2e y las de humo.

Si no quieres cargar el equipo, que las corra GitHub: sube el commit
(`git push`) y

```bash
./scripts/github.sh tests            # las de la CI para ese commit; si fallan, enseña qué
./scripts/github.sh packages         # los siete de una versión: .deb, .AppImage, APK, dos .dmg y los dos de Windows
./scripts/github.sh packages linux   # o solo los de un sistema (linux, android, macos, windows)
```

Mientras tanto aquí solo se espera; los paquetes acaban en `dist/installers/`
como si los hubieras construido tú. Si el commit ya tiene una ejecución (la
del push a main, la de la etiqueta de versión), se usa esa en vez de lanzar
otra.

Las pruebas del núcleo **nunca tocan tus datos**: cada una trabaja en su
propia carpeta temporal, que se borra al acabar (`testkit.rs` de cada
crate), con su configuración y su biblioteca.

Una tanda suelta, mientras trabajas:

```bash
cargo test --workspace --release                 # todo lo de Rust
cargo test --release -p danplay-server watch     # un crate, solo lo que se llame así
cd desktop && vp run check                       # lint + css + formato + pruebas
cd desktop && vp run e2e                         # los flujos en Chrome
```

Las e2e arrancan `danplay-server` (`desktop/e2e/server.mjs` lo busca en
`target/debug` o `target/release`; o el que diga `DANPLAY_E2E_CORE`), así
que antes hace falta `cargo build -p danplay-server`.

### Cómo se escriben aquí las pruebas

Dos costumbres que conviene seguir:

**Una prueba nueva tiene que fallar con el código viejo.** Si no falla, no está
comprobando nada. La forma rápida de verificarlo es revertir el arreglo un
momento y ver que se pone en rojo.

**Se prueba el comportamiento, no cómo está hecho.** Da igual cómo se ponga al
día el índice mientras el resultado sea el correcto. Así las pruebas sobreviven
a que se cambie la implementación por dentro, que es justo para lo que están.

**La biblioteca de prueba se genera.** El `testkit.rs` de cada crate hace mp3
de verdad con ffmpeg (un segundo de silencio, con sus etiquetas), así que las
pruebas comprueban lo mismo en cualquier máquina. Antes dependían de la música
de quien las ejecutara y la mitad se saltaban solas. Sin ffmpeg, las que lo
necesitan lo dicen y vuelven sin comprobar nada.

**Los dobles se construyen a partir del código real.** El de la API
(`desktop/tests/support/backend.js`) recorre el `api` de verdad, así que un
método que la interfaz llame y la prueba no haya programado falla diciendo
cuál es. Cinco fallos de la interfaz eran nombres que dejaron de existir al
pasar el código a inglés y que nadie comparó con el núcleo.

### El banco de pruebas contra un modelo de verdad

Las pruebas de arriba usan un modelo falso: comprueban la mecánica del
asistente (ids que no existen se rechazan, lo destructivo se confirma, la
narración se detecta), no que un modelo real siga el prompt. Para eso
estaba `scripts/evaluate-assistant.py`: montaba una biblioteca sintética,
corría las conversaciones de `tests/evaluation.json` contra el proveedor
configurado en la app y decía qué cumplió cada una (herramientas usadas, lo
que quedó en la biblioteca, lo que dijo), lo que tardó y lo que costó. Se
lanzaba a mano **cada vez que se tocaba el prompt, las herramientas o se
cambiaba de modelo**, y antes de subir versión. La primera pasada encontró
dos fallos de la búsqueda (`Barak - Mi Gozo` devolvía cero por el guion;
`titulo:` no existía como filtro) y una manía del modelo (escribir la
llamada como texto) que ninguna prueba con dobles habría visto.

Era de Python y usaba su núcleo, así que se fue con él en la 1.40. Queda en
el historial, con sus conversaciones (`git show
a0e9317^:scripts/evaluate-assistant.py` y `a0e9317^:tests/evaluation.json`),
para quien lo pase a Rust.

## La versión

**Cada cambio que se entrega sube la versión.** Sin excepción: una corrección
pequeña, una función nueva, un cambio que rompe algo. Es la regla de la casa
porque, sin subirla, un `.deb` con el mismo número no se reinstala con
`apt install`, Windows enseña el mismo número antes y después, y nadie puede
saber qué DanPlay tiene delante.

Cuánto se sube lo decide quien hace el cambio, con versionado semántico
`MAYOR.MENOR.PARCHE`:

| Sube | Cuándo | Ejemplo |
| --- | --- | --- |
| **PARCHE** (1.2.0 → 1.2.1) | arregla algo sin cambiar lo que la app hace | el reproductor se quedaba mudo; el asistente narraba sin actuar |
| **MENOR** (1.2.0 → 1.3.0) | añade algo que antes no se podía hacer | enviar por Telegram, selección múltiple, renombrar listas |
| **MAYOR** (1.2.0 → 2.0.0) | cambia algo de forma incompatible: formato de la base, del `.desktop`, de la API interna, de las etiquetas que escribe | pasar el índice a otro esquema sin migración |

Si un mismo bloque de cambios mezcla arreglos y funciones, manda el mayor.

La versión se escribe en dos sitios (el `Cargo.toml` de la raíz, que heredan
todos los crates y del que la toma Tauri, y `desktop/package.json`) y no se
toca a mano:

```bash
./scripts/bump-version.sh 1.3.0
```

Eso los deja iguales, con el cerrojo de Cargo (el de Bun no la lleva) y la
insignia del README. Después, `./scripts/test.sh --quick` —hay
una prueba que falla si alguno se queda atrás—, un **commit propio**
(`chore: subir a 1.3.0`, sin mezclarlo con el cambio) y, para publicarla,
una etiqueta:
`git tag v1.3.0 && git push --tags`. Con ella la CI construye los siete
paquetes (el `.deb`, el `.AppImage`, el APK, los dos `.dmg` y el instalador y
el portátil de Windows) y los deja en esa versión de GitHub: no hace falta
compilar nada en tu equipo. Si además los quieres en `dist/installers/`,
`./scripts/github.sh packages` espera a esas mismas ejecuciones y te los baja.

## Estilo

- El **código** en inglés: carpetas, archivos, funciones, variables, rutas de
  la API y columnas de la base.
- Los **comentarios** y todo lo que ve el usuario, en castellano.
- Los comentarios explican **por qué**, no qué. Si algo está hecho de una forma
  rara, el comentario cuenta qué pasó cuando se hizo de la forma normal.
- Sin acentos en nombres de archivo generados. La ñ se conserva.
- El formato no se discute: lo ponen `cargo fmt` (Rust) y Oxfmt (`vp fmt`,
  la interfaz), y la CI falla si algo no está formateado.
  `.editorconfig` hace que cualquier editor empiece bien.

## Dónde se puede ayudar

**Detección de tonalidad.** Es la debilidad conocida: 2 aciertos de 8. Se
probaron 60 combinaciones de perfiles y ponderaciones (Krumhansl, Temperley,
pesos por banda) y todas se estancan ahí. Como nadie lo usaba, ese código
se quitó (estaba en `core/src/audio.rs`; queda en el historial, en
`e6a86f0^`), y hoy el tono sale del cifrado o se pone a mano. Si sabes de
esto, es donde más falta hace.

**Windows.** Ya se compila entero desde Linux: `./scripts/build-windows-cross.sh
--zip --installer` saca la versión portátil **y** el instalador. Pero **nadie
lo ha ejecutado en un Windows de verdad**. Falta comprobar lo que solo se ve
al abrirlo: que suene (WASAPI), que las rutas con acentos no rompan el índice,
que la papelera reciba los archivos, que el clic en el icono abra la
ventanita, que las teclas multimedia funcionen, y que el instalador deje los
accesos directos y el «Abrir con» donde debe. Los detalles están en
[Windows](WINDOWS.md).

**macOS.** Sin probar. El núcleo va dentro, como en Linux, y la
política de activación ya está puesta (`tray::dock`: al esconderse pasa a
`Accessory` y sale del Dock, al volver a `Regular`). El paquete sale de la
integración continua: `./scripts/build-macos.sh` lanza `macos.yml`, espera y
deja en `dist/installers/` un `.dmg` para Apple Silicon y otro para Intel, con
ffmpeg, fpcalc, QuickJS, yt-dlp y ONNX Runtime dentro (en los Intel, la 1.23.2,
la última que Microsoft publica para ellos) y firmados solo «ad hoc» (la ejecución guarda
además la `.app` en `.zip`). Pero **nadie lo ha abierto todavía en un Mac
de verdad**. Falta ver si la bandeja de Tauri entrega el clic como en
Windows, si la ventanita queda bien pegada al icono de la barra y si el
cambio de política no deja la aplicación sin foco al reaparecer.

**Traducción.** Todo lo que ve el usuario está en castellano, escrito a mano.
No hay sistema de idiomas.

## Antes de abrir un pull request

- Que `./scripts/test.sh` pase entero.
- Que lo que arregles tenga una prueba que falle sin el arreglo.
- Si cambias algo de rendimiento, mide y di los números. «Va más rápido» no
  vale; «de 890 ms a 185 ms con 20.000 canciones» sí.
- Si algo se queda a medias o tiene una pega, dilo en la descripción. Es más
  útil que descubrirlo después.
- Nada de añadir dependencias con copyleft fuerte (GPL, AGPL) a lo que viaja
  en los paquetes. `mutagen` lo era, y por eso las etiquetas pasaron a Rust
  (lofty); ver [DEPENDENCIES.md](DEPENDENCIES.md).

## Informar de un fallo

Cuenta qué esperabas, qué pasó, y cómo repetirlo. Ayuda mucho el registro
de la app (`danplay.log`, en su carpeta de registros: en Linux,
`~/.local/share/local.danplay.app/logs/`), que dice al arrancar dónde
encontró sus datos, ONNX Runtime y cada programa.

Si es un fallo de identificación, el nombre del archivo que lo provoca vale
más que cualquier descripción.
