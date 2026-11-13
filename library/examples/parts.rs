//! Las partes de una canción por su letra con tiempos (`library::parts`),
//! para mirarlas de cerca con una LRC de verdad:
//!
//! ```text
//! cargo run -p danplay-library --example parts -- LETRA.lrc [DURACION]
//! ```
#![allow(clippy::print_stdout, clippy::print_stderr)]

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let Some(path) = args.get(1) else {
        eprintln!("uso: parts LETRA.lrc [DURACION]");
        std::process::exit(2);
    };
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("no se lee {path}: {e}");
            std::process::exit(1);
        }
    };
    let duration = args.get(2).and_then(|d| d.parse().ok()).unwrap_or(0.0);
    let lines = library::parts::lines(&text);
    let parts = library::parts::from_lyrics(&lines, duration, &[]);
    let mut next = parts.iter().peekable();
    for line in &lines {
        while let Some((at, label)) = next.next_if(|(at, _)| *at <= line.at + 0.01) {
            println!(
                "--- {:>3.0}:{:04.1}  {label}",
                (at / 60.0).floor(),
                at % 60.0
            );
        }
        println!(
            "    {:>3.0}:{:04.1}  {}",
            (line.at / 60.0).floor(),
            line.at % 60.0,
            line.text
        );
    }
    for (at, label) in next {
        println!(
            "--- {:>3.0}:{:04.1}  {label}",
            (at / 60.0).floor(),
            at % 60.0
        );
    }
}
