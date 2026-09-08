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

python3 -m venv .venv
# `requirements-dev.txt` trae también pytest, maturin y PyInstaller
.venv/bin/python -m pip install -r requirements-dev.txt

# el crate de Rust que usa Python (hashes y análisis de audio)
.venv/bin/maturin develop --release -m core/Cargo.toml

cd desktop && npm install && cd ..
cp .env.example .env
```

Para usar DanPlay sin tocarlo basta con `requirements.txt`, que solo lleva lo
necesario para ejecutarlo (sin compilador de Rust).

Opcional, para la huella acústica y la conversión de formatos:

```bash
sudo apt install libchromaprint-tools ffmpeg
```

Sin ellos la app funciona igual, solo que con esas dos funciones desactivadas
y diciéndolo en Ajustes.

## Ejecutar

```bash
./scripts/build.sh     # compila las cuatro capas, en orden
./danplay-app.sh       # la app de escritorio
./danplay.sh status    # la línea de comandos
```

Para trastear solo con la interfaz, sin recompilar Rust:

```bash
.venv/bin/python -m danplay.cli serve      # API en el puerto 8730
cd desktop && npm run dev                  # Vite en el 5273, con proxy
```

## Las pruebas

```bash
./scripts/test.sh
```

Son siete tandas y todas tienen que pasar:

| Tanda | Qué cubre |
| --- | --- |
| Núcleo Python | nombres, etiquetas, duplicados, teoría musical, índice |
| API | los endpoints sobre una biblioteca temporal de verdad |
| Frontend (URLs) | cómo se construyen las rutas de medios en cada sistema |
| Frontend (estilo) | ESLint y stylelint |
| Frontend (vitest) | componentes, reactividad, temas, listas grandes, contratos |
| Rust | hashes, análisis de audio, reproductor, cola y bandeja |
| Humo | la app **ya compilada**: socket, permisos, bandeja, ciclo de vida |

`./scripts/test.sh --rapido` se salta las de humo, que arrancan la aplicación
de verdad.

Una tanda suelta, mientras trabajas:

```bash
.venv/bin/python -m pytest tests/ -q
cd desktop && npm run check      # lint + css + pruebas
cd core && cargo test --release
```

### Cómo se escriben aquí las pruebas

Dos costumbres que conviene seguir:

**Una prueba nueva tiene que fallar con el código viejo.** Si no falla, no está
comprobando nada. La forma rápida de verificarlo es revertir el arreglo un
momento y ver que se pone en rojo.

**Se prueba el comportamiento, no cómo está hecho.** Da igual cómo se ponga al
día el índice mientras el resultado sea el correcto. Así las pruebas sobreviven
a que se cambie la implementación por dentro, que es justo para lo que están.

**La biblioteca de prueba se genera.** `tests/conftest.py` hace mp3 de verdad
con ffmpeg (un segundo de silencio, con sus etiquetas), así que las pruebas
comprueban lo mismo en cualquier máquina. Antes dependían de la música de
quien las ejecutara y la mitad se saltaban solas.

**Los dobles se construyen a partir del código real.** El de la API
(`desktop/tests/support/backend.js`) recorre el `api` de verdad, así que un
método que la interfaz llame y la prueba no haya programado falla diciendo
cuál es. Cinco fallos de la interfaz eran nombres que dejaron de existir al
pasar el código a inglés y que nadie comparó con el núcleo.

## Estilo

- El **código** en inglés: carpetas, archivos, funciones, variables, rutas de
  la API y columnas de la base.
- Los **comentarios** y todo lo que ve el usuario, en castellano.
- Los comentarios explican **por qué**, no qué. Si algo está hecho de una forma
  rara, el comentario cuenta qué pasó cuando se hizo de la forma normal.
- Sin acentos en nombres de archivo generados. La ñ se conserva.

## Dónde se puede ayudar

**Detección de tonalidad.** Es la debilidad conocida: 2 aciertos de 8. Se
probaron 60 combinaciones de perfiles y ponderaciones (Krumhansl, Temperley,
pesos por banda) y todas se estancan ahí. El código está en
[`core/src/audio.rs`](../core/src/audio.rs). Si sabes de esto, es donde más
falta hace.

**Windows.** Ya se compila —`./scripts/build-windows-cross.sh` saca una
versión portátil desde Linux y `.github/workflows/windows.yml` el instalador—
pero **nadie lo ha ejecutado en un Windows de verdad**. Falta comprobar lo que
solo se ve al abrirlo: que suene (WASAPI), que las rutas con acentos no rompan
el índice, que la papelera reciba los archivos, que el clic en el icono abra
la ventanita y que las teclas multimedia funcionen. Los detalles están en
[Windows](WINDOWS.md).

**macOS.** Sin probar. El transporte por socket Unix vale tal cual y la
política de activación ya está puesta (`tray::dock`: al esconderse pasa a
`Accessory` y sale del Dock, al volver a `Regular`), pero **nadie lo ha
compilado ahí**. Falta ver si la bandeja de Tauri entrega el clic como en
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
- Nada de añadir dependencias con copyleft fuerte (GPL, AGPL). Ya hay una
  —`mutagen`— y complica la distribución;
  ver [DEPENDENCIAS.md](DEPENDENCIAS.md).

## Informar de un fallo

Cuenta qué esperabas, qué pasó, y cómo repetirlo. Ayuda mucho:

```bash
./danplay.sh status     # configuración y estado, sin claves
```

Si es un fallo de identificación, el nombre del archivo que lo provoca vale
más que cualquier descripción.
