#![allow(clippy::float_cmp)]

use super::*;

fn scratch(name: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("danplay-duplicados-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("carpeta temporal");
    dir
}

/// Los números de rapidfuzz 3.14 (`fuzz.ratio`, `fuzz.token_set_ratio`),
/// hasta el último decimal.
#[test]
fn rapidfuzz_scores() {
    for (a, b, r, t) in [
        (
            "this is a test",
            "this is a test!",
            96.551_724_137_931_03,
            96.551_724_137_931_03,
        ),
        (
            "gozo mio",
            "gozos mios",
            88.888_888_888_888_89,
            88.888_888_888_888_89,
        ),
        (
            "barak gozo",
            "gozo vivo",
            42.105_263_157_894_73,
            61.538_461_538_461_54,
        ),
        ("abc", "xyz", 0.0, 0.0),
        (
            "ñandú pájaro",
            "ñandu pajaro",
            83.333_333_333_333_34,
            83.333_333_333_333_33,
        ),
    ] {
        assert_eq!(ratio(a, b), r, "ratio {a} / {b}");
        assert_eq!(token_set_ratio(a, b), t, "token_set_ratio {a} / {b}");
    }
    assert_eq!(
        token_set_ratio(
            "fuzzy was a bear but not a dog",
            "fuzzy was a bear but not a cat"
        ),
        92.307_692_307_692_3
    );
    assert_eq!(
        token_set_ratio("fuzzy was a bear", "fuzzy fuzzy was a bear"),
        100.0
    );
    // más de 64 caracteres: el LCS va en varios bloques de bits
    let long_a = "a".repeat(70) + &"b".repeat(70);
    let long_b = "b".repeat(70) + &"a".repeat(65);
    assert_eq!(ratio(&long_a, &long_b), 50.909_090_909_090_91);
}

#[test]
fn ratio_cutoff_like_process_extract() {
    let p = Pattern::new(&"gozo mio".chars().collect::<Vec<_>>());
    let b: Vec<char> = "gozos mios".chars().collect();
    assert!(ratio_passes(&p, &b, 88.0));
    assert!(!ratio_passes(&p, &b, 89.0));
    // por la diferencia de largos ya no llega
    assert!(!ratio_passes(&p, &"gozo".chars().collect::<Vec<_>>(), 90.0));
    // con umbral 0 entra todo
    assert!(ratio_passes(&p, &"xyz".chars().collect::<Vec<_>>(), 0.0));
}

fn make_paths(names: &[&str]) -> Vec<PathBuf> {
    names
        .iter()
        .map(|n| PathBuf::from(format!("/x/{n}")))
        .collect()
}

fn normalized(paths: &[PathBuf], groups: Vec<Vec<usize>>) -> Vec<Vec<PathBuf>> {
    let mut out: Vec<Vec<PathBuf>> = groups
        .into_iter()
        .map(|g| {
            let mut g: Vec<PathBuf> = g.into_iter().map(|i| paths[i].clone()).collect();
            g.sort();
            g
        })
        .collect();
    out.sort();
    out
}

#[test]
fn same_song_in_another_file() {
    // plurales y erratas: NO comparten ninguna palabra, así que caen por la
    // rama de comparación simple
    let paths = make_paths(&[
        "gozo mio.mp3",
        "gozos mios.mp3",
        "santo fuego.mp3",
        "santos fuegos.mp3",
        "Barak - Mi Gozo.mp3",
        "Barak - Mi Gozo - r.mp3",
    ]);
    let groups = normalized(&paths, same_song(&paths, 0.88));
    assert_eq!(
        groups,
        vec![
            vec![paths[5].clone(), paths[4].clone()],
            vec![paths[0].clone(), paths[1].clone()],
            vec![paths[2].clone(), paths[3].clone()],
        ]
    );
    // uno contenido en otro: la puntuación de conjuntos da 100 (también
    // «barak» solo, como en Python)
    let paths = make_paths(&[
        "barak gozo.mp3",
        "barak gozo vivo estudio.mp3",
        "barak.mp3",
        "otra cosa.mp3",
    ]);
    assert_eq!(
        normalized(&paths, same_song(&paths, 0.88)),
        vec![vec![paths[1].clone(), paths[0].clone(), paths[2].clone()]]
    );
    // claves vacías o de una sola letra: fuera
    let paths = make_paths(&[".mp3", "a.mp3", "el la de.mp3"]);
    assert!(same_song(&paths, 0.88).is_empty());
}

#[test]
fn byte_identical_files() {
    let dir = scratch("identicos");
    let a = dir.join("a.mp3");
    let b = dir.join("b.mp3");
    let c = dir.join("c.mp3");
    let empty1 = dir.join("vacio1.mp3");
    let empty2 = dir.join("vacio2.mp3");
    std::fs::write(&a, vec![b'x'; 5000]).expect("a");
    std::fs::write(&b, vec![b'x'; 5000]).expect("b");
    std::fs::write(&c, vec![b'y'; 5000]).expect("c");
    std::fs::write(&empty1, b"").expect("vacío");
    std::fs::write(&empty2, b"").expect("vacío");
    let paths = vec![a, b, c, empty1, empty2, dir.join("no-esta.mp3")];
    assert_eq!(identical(&paths), vec![vec![0, 1]]);
    std::fs::remove_dir_all(&dir).expect("borrar");
}

#[cfg(unix)]
#[test]
fn suffix_of_a_non_utf8_name() {
    use std::os::unix::ffi::OsStrExt;
    let raw = OsStr::from_bytes(b"Gozo \xff - r2.mp3");
    assert_eq!(
        name_without_suffix(raw),
        OsStr::from_bytes(b"Gozo \xff.mp3")
    );
}

#[test]
fn duplicate_suffix() {
    for (name, clean) in [
        ("Barak - Mi Gozo - r2.mp3", "Barak - Mi Gozo.mp3"),
        ("Barak - Mi Gozo - r.mp3", "Barak - Mi Gozo.mp3"),
        ("Barak - Mi Gozo-R12.mp3", "Barak - Mi Gozo.mp3"),
        ("Barak - Mi Gozo.mp3", "Barak - Mi Gozo.mp3"),
        ("- r.mp3", "- r.mp3"),
        ("Gozo - r2\n.mp3", "Gozo.mp3"),
        ("Gozo - r٣.mp3", "Gozo.mp3"),
        (".bashrc - r", ".bashrc"),
        ("  - r2.mp3", "  - r2.mp3"),
        ("Gozo -\tR .mp3", "Gozo -\tR.mp3"),
    ] {
        assert_eq!(
            name_without_suffix(OsStr::new(name)),
            OsStr::new(clean),
            "{name:?}"
        );
    }
}
