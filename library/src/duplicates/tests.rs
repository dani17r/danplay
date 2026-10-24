#![allow(clippy::float_cmp)]

use super::*;

fn scratch(name: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("danplay-duplicados-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("carpeta temporal");
    dir
}

/// Los números de rapidfuzz 3.14 (`ratio`, `token_set_ratio`), hasta el
/// último decimal.
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
    // «barak» solo)
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
        ("Gozo -\tR .mp3", "Gozo.mp3"),
    ] {
        assert_eq!(name_without_suffix(name), clean, "{name:?}");
    }
}

#[test]
fn strip_duplicate_suffix() {
    for (value, expected) in [
        ("Barak - Mi Gozo - r.mp3", "Barak - Mi Gozo.mp3"),
        ("Barak - Mi Gozo - r2.mp3", "Barak - Mi Gozo.mp3"),
        ("Barak - Mi Gozo - R3.mp3", "Barak - Mi Gozo.mp3"),
        ("Barak - Mi Gozo.mp3", "Barak - Mi Gozo.mp3"),
        ("Rey (En Vivo) - r.mp3", "Rey (En Vivo).mp3"),
    ] {
        assert_eq!(name_without_suffix(value), expected, "{value}");
    }
}

/// Los hashes en paralelo dan lo mismo que el de un solo archivo.
#[test]
fn parallel_and_single_hashes_match() {
    let dir = scratch("hashes");
    let f = dir.join("x.mp3");
    // 200 000 bytes que no se repiten (como al azar, pero fijos)
    let mut state = 0x2545_f491_4f6c_dd1d_u64;
    let data: Vec<u8> = (0..200_000)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state >> 24) as u8
        })
        .collect();
    std::fs::write(&f, &data).expect("archivo");
    let many = partial_hashes(vec![f.clone()], PARTIAL_BYTES);
    assert_eq!(many.len(), 1);
    assert_eq!(many[0].1, partial_hash(&f, PARTIAL_BYTES));
    assert_eq!(
        partial_hash(&f, PARTIAL_BYTES).expect("hash"),
        hex(&Md5::digest(&data))
    );
    std::fs::remove_dir_all(&dir).expect("borrar");
}

/// Todos contra todos, sin atajos: la referencia (con `token_set_ratio` en
/// cada pareja).
fn brute_force_groups(paths: &[PathBuf], threshold: f64) -> Vec<Vec<usize>> {
    let mk: Vec<String> = paths
        .iter()
        .map(|p| crate::names::match_key(&p.file_name().expect("nombre").to_string_lossy()))
        .collect();
    let mut by_key: Vec<(String, Vec<usize>)> = Vec::new();
    for (i, k) in mk.iter().enumerate() {
        if k.is_empty() {
            continue;
        }
        match by_key.iter_mut().find(|(key, _)| key == k) {
            Some((_, g)) => g.push(i),
            None => by_key.push((k.clone(), vec![i])),
        }
    }
    let mut groups: Vec<Vec<usize>> = by_key
        .into_iter()
        .map(|(_, g)| g)
        .filter(|g| g.len() > 1)
        .collect();
    let already_there: Vec<usize> = groups.iter().flatten().copied().collect();
    let rest: Vec<usize> = (0..paths.len())
        .filter(|i| !already_there.contains(i) && !mk[*i].is_empty())
        .collect();
    let mut used: Vec<usize> = Vec::new();
    for (n, &a) in rest.iter().enumerate() {
        if used.contains(&a) {
            continue;
        }
        let mut g = vec![a];
        for &b in &rest[n + 1..] {
            if used.contains(&b) {
                continue;
            }
            if token_set_ratio(&mk[a], &mk[b]) / 100.0 >= threshold {
                g.push(b);
            }
        }
        if g.len() > 1 {
            used.extend(&g);
            groups.push(g);
        }
    }
    groups
}

/// Detección de duplicados: el atajo tiene que dar EXACTAMENTE los mismos
/// grupos que comparar todo contra todo. Es lo único que autoriza el atajo.
#[test]
fn the_shortcut_finds_exactly_the_same_duplicate_groups() {
    let mut cases: Vec<(&str, Vec<PathBuf>)> = vec![
        // plurales y erratas: NO comparten ninguna palabra, así que caen por
        // la rama de comparación simple. Son las que rompería un atajo ingenuo.
        (
            "plurales",
            make_paths(&[
                "gozo mio.mp3",
                "gozos mios.mp3",
                "santo fuego.mp3",
                "santos fuegos.mp3",
                "alabanza nueva.mp3",
                "alabanzas nuevas.mp3",
            ]),
        ),
        // uno contenido en otro: la puntuación de conjuntos da 100
        (
            "subconjuntos",
            make_paths(&[
                "barak gozo.mp3",
                "barak gozo vivo estudio.mp3",
                "barak.mp3",
                "gozo.mp3",
                "barak gozo vivo.mp3",
            ]),
        ),
        // claves vacías o de una sola letra (match_key las descarta)
        (
            "raros",
            make_paths(&[".mp3", "a.mp3", "el la de.mp3", "AAA.mp3", "aaa.mp3"]),
        ),
    ];
    // 400 nombres «Artista - tres palabras» con un generador fijo (no hace
    // falta una secuencia concreta, sino muchas combinaciones que se repiten
    // en parte)
    let words = [
        "barak", "gozo", "vivo", "santo", "fuego", "gloria", "rey", "cristo", "amor", "cielo",
        "paz", "luz",
    ];
    let mut state = 4_u64;
    let mut next = |n: usize| {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((state >> 33) as usize) % n
    };
    // (sin repetir ruta: en un disco no hay dos archivos con la misma)
    let mut mixed: Vec<PathBuf> = Vec::new();
    while mixed.len() < 400 {
        let artist = words[next(words.len())];
        let mut picked: Vec<&str> = Vec::new();
        while picked.len() < 3 {
            let w = words[next(words.len())];
            if !picked.contains(&w) {
                picked.push(w);
            }
        }
        let title = artist[..1].to_uppercase() + &artist[1..];
        let path = PathBuf::from(format!("/x/{title} - {}.mp3", picked.join(" ")));
        if !mixed.contains(&path) {
            mixed.push(path);
        }
    }
    cases.push(("mezcla", mixed));
    for (name, paths) in cases {
        let expected = normalized(&paths, brute_force_groups(&paths, 0.88));
        let got = normalized(&paths, same_song(&paths, 0.88));
        assert_eq!(got, expected, "difieren en «{name}»");
    }
}
