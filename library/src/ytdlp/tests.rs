use super::*;

fn nums(v: &str) -> Vec<String> {
    version_tuple(v)
        .iter()
        .map(|n| n.digits().to_owned())
        .collect()
}

#[test]
fn versions_compare_as_numbers() {
    assert_eq!(nums("2026.08.19"), ["2026", "8", "19"]);
    assert_eq!(version_tuple("2026.08.19"), version_tuple("2026.8.19"));
    assert!(newer("2026.9.1", "2026.08.19"));
    assert!(!newer("2026.08.19", "2026.8.19"));
    assert!(nums("").is_empty() && nums("sin cifras").is_empty());
    // como las tuplas: lo que sigue igual y es más largo, es mayor
    assert!(newer("1.2.0", "1.2"));
    // sin tope de tamaño, como los `int` de Python
    assert!(newer("100000000000000000000001", "99999999999999999999999"));
    assert_eq!(nums("v000.0010"), ["0", "10"]);
    // las cifras de Unicode cuentan (es lo que hacen `\d` e `int()`)
    assert_eq!(nums("٢٠٢٦.٨"), ["2026", "8"]);
    // las de matemáticas van en cinco tiras de diez seguidas
    assert_eq!(nums("\u{1d7d1}\u{1d7e1}x٢٠٢٦"), ["39", "2026"]);
    assert_eq!(
        nums("1²3"),
        ["1", "3"],
        "un superíndice no es una cifra decimal"
    );
}

fn spec(name: &str) -> &'static RuntimeSpec {
    runtime_spec(name).expect("conocido")
}

#[test]
fn each_runtime_says_its_version_its_way() {
    let deno = parse_runtime(
        spec("deno"),
        "/x/deno",
        "deno 2.5.1 (stable, release, x86_64)\nv8 13\n",
    );
    assert_eq!((deno.version.as_str(), deno.supported), ("2.5.1", true));
    assert_eq!(deno.minimum, "2.3.0");
    assert!(!parse_runtime(spec("deno"), "d", "deno 2.2.9\n").supported);
    let node = parse_runtime(spec("node"), "n", "v22.3.0\n");
    assert_eq!((node.version.as_str(), node.supported), ("22.3.0", true));
    let old = parse_runtime(spec("node"), "n", "v18.19.0");
    assert_eq!(
        (old.version.as_str(), old.supported, old.minimum.as_str()),
        ("18.19.0", false, "22.0.0")
    );
    // en una línea que no es la primera también (re.MULTILINE), con \r\n
    let late = parse_runtime(spec("node"), "n", "aviso\r\nv23.1.0\r\n");
    assert_eq!(late.version, "23.1.0");
    let qjs = parse_runtime(
        spec("quickjs"),
        "q",
        "QuickJS version 2024-01-13\nusage: qjs",
    );
    assert_eq!((qjs.version.as_str(), qjs.supported), ("2024-01-13", true));
    assert!(!parse_runtime(spec("quickjs"), "q", "QuickJS version 2021-03-27").supported);
    let ng = parse_runtime(spec("quickjs"), "q", "QuickJS-ng version 0.10.1");
    assert_eq!((ng.version.as_str(), ng.supported), ("0.10.1", true));
    assert!(!parse_runtime(spec("quickjs"), "q", "QuickJS-ng version 0").supported);
    // los espacios de `\s+` pueden ser saltos de línea
    assert_eq!(
        parse_runtime(spec("quickjs"), "q", "QuickJS\n version\t2024-01-13").version,
        "2024-01-13"
    );
    assert_eq!(
        parse_runtime(spec("quickjs"), "q", "QuickJSversion 1").version,
        ""
    );
    let bun = parse_runtime(spec("bun"), "b", "  \n1.2.11\n");
    assert_eq!((bun.version.as_str(), bun.supported), ("1.2.11", true));
    let nothing = parse_runtime(spec("deno"), "d", "");
    assert_eq!((nothing.version.as_str(), nothing.supported), ("", false));
}

#[test]
fn the_hint_says_what_to_do() {
    let none = hint(&[]);
    assert!(
        none.contains("Deno") && none.contains("Node.js 22"),
        "{none}"
    );
    let old = parse_runtime(spec("node"), "n", "v18.19.0");
    let h = hint(&[old]);
    assert_eq!(
        h,
        "Tu node (18.19.0) es demasiado antiguo para yt-dlp: hace falta 22.0.0 o posterior. \
         Actualizalo, o instala Deno (https://deno.com)."
    );
    let unknown = parse_runtime(spec("deno"), "d", "algo raro");
    assert!(hint(&[unknown]).contains("(version desconocida)"));
}

#[cfg(unix)]
#[test]
fn runtimes_are_asked_all_at_once() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().expect("en la prueba");
    let tool = |name: &str, body: &str| {
        let p = dir.path().join(name);
        std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).expect("en la prueba");
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).expect("en la prueba");
        p.to_string_lossy().into_owned()
    };
    let found = [
        (spec("deno"), tool("deno", "sleep 0.4; echo 'deno 2.5.1'")),
        (spec("node"), tool("node", "sleep 0.4; echo v18.0.0 >&2")),
        (
            spec("quickjs"),
            dir.path().join("no-esta").to_string_lossy().into_owned(),
        ),
    ];
    let t = std::time::Instant::now();
    let answers = probe_all(&found);
    assert!(
        t.elapsed() < std::time::Duration::from_millis(750),
        "a la vez: {:?}",
        t.elapsed()
    );
    assert_eq!(answers[0].as_ref().expect("en la prueba").version, "2.5.1");
    let node = answers[1].as_ref().expect("en la prueba");
    assert_eq!(
        (node.version.as_str(), node.supported),
        ("18.0.0", false),
        "lo de stderr también"
    );
    assert!(answers[2].is_err(), "no arranca");
    let ok: Vec<Runtime> = answers.into_iter().flatten().collect();
    assert_eq!(choose(&ok), Some(0));
}

#[test]
fn the_downloaded_version_is_used_only_if_newer() {
    let dir = tempfile::tempdir().expect("en la prueba");
    let root = dir.path().join("yt-dlp");
    let root_s = root.to_string_lossy().into_owned();
    assert_eq!(marked_version(&root), Ok(None), "sin carpeta");
    std::fs::create_dir_all(root.join("2099.1.1")).expect("en la prueba");
    std::fs::write(root.join("current"), " 2099.1.1\r\n").expect("en la prueba");
    assert_eq!(marked_version(&root), Ok(Some("2099.1.1".into())));
    assert!(usable("2099.1.1", ""), "sin ninguna en la app");
    assert!(usable("2099.1.1", "2026.8.19"));
    assert!(!usable("2099.1.1", "2099.01.01"), "la de la app es igual");
    assert!(!usable("2099.1.1", "2100.1"), "la de la app es más nueva");
    // una marca que no es UTF-8 no se lee como otra cosa
    std::fs::write(root.join("current"), b"\xff").expect("en la prueba");
    assert_eq!(marked_version(&root), Err(NotUtf8(vec![0xff])));
    // apartarla: fuera la carpeta y, si la nombra, la marca
    std::fs::write(root.join("current"), "2099.1.1").expect("en la prueba");
    std::fs::create_dir_all(root.join("2001.1.1")).expect("en la prueba");
    discard(&root_s, &format!("{root_s}/2001.1.1")).expect("en la prueba");
    assert!(root.join("current").exists() && !root.join("2001.1.1").exists());
    discard(&root_s, &format!("{root_s}/2099.1.1")).expect("en la prueba");
    assert!(!root.join("current").exists() && !root.join("2099.1.1").exists());
}

#[test]
fn pruning_keeps_only_the_new_one() {
    let dir = tempfile::tempdir().expect("en la prueba");
    let root = dir.path();
    for v in ["2099.1.1", "2026.8.19", ".nueva-1-2"] {
        std::fs::create_dir_all(root.join(v).join("yt_dlp")).expect("en la prueba");
    }
    std::fs::write(root.join("current"), "2099.1.1").expect("en la prueba");
    #[cfg(unix)]
    std::os::unix::fs::symlink(root.join("2099.1.1"), root.join("enlace")).expect("en la prueba");
    prune(root, "2099.1.1").expect("en la prueba");
    let mut left: Vec<String> = std::fs::read_dir(root)
        .expect("en la prueba")
        .map(|e| {
            e.expect("en la prueba")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    left.sort();
    let mut expected = vec!["2099.1.1", "current"];
    if cfg!(unix) {
        // un enlace no se borra (rmtree se niega) ni se sigue
        expected.push("enlace");
        expected.sort_unstable();
    }
    assert_eq!(left, expected);
    assert!(root.join("2099.1.1").join("yt_dlp").is_dir());
}

#[test]
fn network_failures_in_one_sentence() {
    assert_eq!(
        why(Some("404"), "x"),
        "el servidor contesto con un error 404"
    );
    assert_eq!(
        why(None, "<urlopen error timed out>"),
        "tarda demasiado en responder"
    );
    assert_eq!(
        why(None, "<urlopen error [Errno -2] Name or service not known>"),
        "sin conexion a internet"
    );
    // «resolution» no es «resolve»: se dice tal cual, como en Python
    let dns = "<urlopen error [Errno -3] Temporary failure in name resolution>";
    assert_eq!(why(None, dns), dns);
    let long = "x".repeat(300);
    assert_eq!(why(None, &long).len(), 160);
    assert_eq!(why(None, &"ñ".repeat(200)).chars().count(), 160);
}
