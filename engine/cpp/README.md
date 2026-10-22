# Signalsmith Stretch, dentro del repositorio

La velocidad sin cambiar el tono (y el tono sin cambiar la velocidad) del
motor del celular es [Signalsmith Stretch](https://github.com/Signalsmith-Audio/signalsmith-stretch)
1.3.1, con la parte de [Signalsmith Linear](https://github.com/Signalsmith-Audio/linear)
que usa (la FFT y la STFT). Las dos son de Signalsmith Audio, con licencia MIT
(`LICENSE.txt` de cada carpeta). Son solo cabeceras de C++.

Van copiadas aquí, sin tocar, y no como dependencia de crates.io, porque el
envoltorio publicado (`signalsmith-stretch` 0.1.3) genera sus enlaces con
bindgen al compilar: necesita libclang con sus cabeceras, que en esta máquina
no estaban, y para Android e iOS hay que apuntarle a mano el sysroot de cada
plataforma. `stretch.cpp` es un envoltorio de C de seis funciones, y los
enlaces de Rust están escritos a mano en `src/stretch.rs`: así basta con el
compilador de C++ que traen el NDK de Android y Xcode.

Se quitaron las carpetas que no hacen falta para compilar (las pruebas, la
demo web, la línea de órdenes y los atajos para Accelerate e IPP, que solo se
usan si se definen sus macros).
