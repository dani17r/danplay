//! Compila Signalsmith Stretch (solo cabeceras de C++, en `cpp/`) con su
//! envoltorio de C. Solo con el motor entero: la app de escritorio usa este
//! crate sin el, solo por el sonido del clic.
fn main() {
    println!("cargo::rerun-if-changed=cpp");
    #[cfg(feature = "engine")]
    {
        let mut build = cc::Build::new();
        build
            .cpp(true)
            .std("c++14")
            .file("cpp/estirar.cpp")
            .include("cpp")
            .warnings(false);
        // sin optimizar, la FFT va diez veces mas lenta: en las pruebas (que
        // se compilan en modo depuracion) se notaria en el tiempo que tardan
        build.opt_level(2);
        build.compile("danplay_estirar");
    }
}
