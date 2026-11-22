//! Compila Signalsmith Stretch (solo cabeceras de C++, en `cpp/`) con su
//! envoltorio de C. Solo con el motor entero (el rasgo `engine`, el de por
//! defecto: el de las dos apps); el analisis (`danplay-analysis`) usa este
//! crate sin el, por el pulso y el sonido del clic.
fn main() {
    println!("cargo::rerun-if-changed=cpp");
    #[cfg(feature = "engine")]
    {
        let mut build = cc::Build::new();
        build
            .cpp(true)
            .std("c++14")
            .file("cpp/stretch.cpp")
            .include("cpp")
            .warnings(false);
        // sin optimizar, la FFT va diez veces mas lenta: en las pruebas (que
        // se compilan en modo depuracion) se notaria en el tiempo que tardan.
        // Con el perfil de publicar, lo mismo que el Rust (-O3) y sin los
        // `assert` de C++ (NDEBUG). Sin -ffast-math: cambiaria los numeros
        let level = std::env::var("OPT_LEVEL").unwrap_or_default();
        build.opt_level(if level == "3" { 3 } else { 2 });
        if std::env::var_os("CARGO_CFG_DEBUG_ASSERTIONS").is_none() {
            build.define("NDEBUG", None);
        }
        build.compile("danplay_stretch");
    }
}
