//! Lo de tests/test_compat.py: las reglas de Python que imita `library::py`,
//! comparadas con las de verdad (`posixpath`, `ntpath`, `fnmatch`, `round`,
//! `repr`, `float`, `int` y `json`). En Python se comparaban en vivo, con
//! Hypothesis; aquí, con los casos que se generaron una vez con Python 3.13
//! (Unicode 15.1) y lo que respondía cada regla: compat_cases.json. Los
//! casos tienen la forma de los de Python (rutas hechas de lo que las rompe,
//! patrones y nombres con los caracteres especiales del glob, números con
//! guiones bajos, espacios e «inf»…) y llevan también sus `@example`. Los
//! decimales van como sus bits, para que nada se redondee por el camino.
use super::*;
use serde_json::Value;
use std::sync::LazyLock;

static CASES: LazyLock<Value> =
    LazyLock::new(|| serde_json::from_str(include_str!("compat_cases.json")).expect("los casos"));

fn cases(name: &str) -> &'static [Value] {
    CASES[name]
        .as_array()
        .unwrap_or_else(|| panic!("sin casos de {name}"))
}

fn text(v: &Value) -> &str {
    v.as_str().expect("texto")
}

fn pair(v: &Value) -> (String, String) {
    (text(&v[0]).to_owned(), text(&v[1]).to_owned())
}

fn yes(v: &Value) -> bool {
    v.as_bool().expect("sí o no")
}

/// Un decimal guardado como sus bits.
fn float(v: &Value) -> f64 {
    f64::from_bits(v.as_u64().expect("bits"))
}

#[test]
fn posix_paths_like_posixpath() {
    for c in cases("posix") {
        let p = text(&c["p"]);
        assert_eq!(posix_normpath(p), text(&c["normpath"]), "normpath({p:?})");
        assert_eq!(posix_split(p), pair(&c["split"]), "split({p:?})");
    }
}

#[test]
fn join_and_commonpath_like_posixpath() {
    for c in cases("posix_pairs") {
        let (a, b) = (text(&c["a"]), text(&c["b"]));
        assert_eq!(posix_join(a, b), text(&c["join"]), "join({a:?}, {b:?})");
        assert_eq!(
            posix_commonpath(&[a, b]).as_deref(),
            c["commonpath"].as_str(),
            "commonpath([{a:?}, {b:?}])"
        );
        if let Some(rel) = c.get("relpath") {
            let (na, nb) = (posix_normpath(a), posix_normpath(b));
            assert_eq!(
                posix_relpath_abs(&na, &nb),
                text(rel),
                "relpath({na:?}, {nb:?})"
            );
        }
    }
}

#[test]
fn windows_paths_like_ntpath() {
    for c in cases("nt") {
        let p = text(&c["p"]);
        let root = &c["splitroot"];
        assert_eq!(
            nt_splitroot(p),
            (text(&root[0]), text(&root[1]), text(&root[2])),
            "splitroot({p:?})"
        );
        assert_eq!(nt_normpath(p), text(&c["normpath"]), "normpath({p:?})");
        assert_eq!(nt_normcase(p), text(&c["normcase"]), "normcase({p:?})");
        assert_eq!(nt_split(p), pair(&c["split"]), "split({p:?})");
        assert_eq!(nt_isabs(p), yes(&c["isabs"]), "isabs({p:?})");
    }
}

#[test]
fn join_and_commonpath_like_ntpath() {
    for c in cases("nt_pairs") {
        let (a, b) = (text(&c["a"]), text(&c["b"]));
        assert_eq!(nt_join(a, b), text(&c["join"]), "join({a:?}, {b:?})");
        assert_eq!(
            nt_commonpath(&[a, b]).as_deref(),
            c["commonpath"].as_str(),
            "commonpath([{a:?}, {b:?}])"
        );
    }
}

#[test]
fn fnmatch_like_fnmatchcase() {
    for c in cases("fnmatch") {
        let (name, pat) = (text(&c[0]), text(&c[1]));
        assert_eq!(fnmatchcase(name, pat), yes(&c[2]), "({name:?}, {pat:?})");
    }
}

/// El mismo número, también en el signo del cero (Python comparaba con
/// `==`, que no lo mira; aquí se mira).
#[test]
fn round_like_python() {
    for c in cases("round") {
        let x = float(&c[0]);
        let n = usize::try_from(c[1].as_u64().expect("decimales")).expect("decimales");
        let (got, want) = (round(x, n), float(&c[2]));
        assert_eq!(
            got.to_bits(),
            want.to_bits(),
            "round({x:?}, {n}): {got:?}, Python {want:?}"
        );
    }
}

#[test]
fn float_repr_like_python() {
    for c in cases("repr_float") {
        let x = float(&c[0]);
        assert_eq!(repr_float(x), text(&c[1]), "repr({x:?})");
    }
}

#[test]
fn float_and_int_from_text_like_python() {
    for c in cases("numbers") {
        let s = text(&c[0]);
        let got = parse_float(s);
        match &c[1] {
            Value::Null => assert_eq!(got, None, "float({s:?})"),
            Value::String(nan) if nan == "nan" => {
                assert!(got.is_some_and(f64::is_nan), "float({s:?}): {got:?}");
            }
            bits => assert_eq!(
                got.map(f64::to_bits),
                Some(float(bits).to_bits()),
                "float({s:?})"
            ),
        }
        assert_eq!(
            int_of(&Value::String(s.to_owned())),
            c[2].as_i64(),
            "int({s:?})"
        );
    }
}

#[test]
fn repr_like_python() {
    for c in cases("repr") {
        assert_eq!(repr(&c[0]), text(&c[1]), "repr({})", c[0]);
    }
}

#[test]
fn json_dumps_like_python() {
    for c in cases("dumps") {
        let indent = c[1].as_u64().map(|n| usize::try_from(n).expect("sangría"));
        let sort_keys = yes(&c[2]);
        assert_eq!(
            dumps_with(&c[0], indent, sort_keys),
            text(&c[3]),
            "dumps({}, indent={indent:?}, sort_keys={sort_keys})",
            c[0]
        );
    }
}

#[test]
fn isidentifier_like_python() {
    for c in cases("isidentifier") {
        let s = text(&c[0]);
        assert_eq!(isidentifier(s), yes(&c[1]), "{s:?}.isidentifier()");
    }
}

/// Los tramos `[desde, hasta]` de una lista de los casos.
fn spans(name: &str) -> Vec<(u32, u32)> {
    cases(name)
        .iter()
        .map(|r| {
            let at = |i: usize| u32::try_from(r[i].as_u64().expect("punto")).expect("punto");
            (at(0), at(1))
        })
        .collect()
}

/// Todos los caracteres que ese Python conoce (en Rust, con un Unicode más
/// nuevo, lo que allí no está asignado ya es una letra: esos no cuentan).
#[test]
fn isprintable_like_python() {
    let printable = spans("printable");
    let inside = |cp: u32| {
        let i = printable.partition_point(|&(_, hi)| hi < cp);
        printable.get(i).is_some_and(|&(lo, _)| lo <= cp)
    };
    let mut checked = 0;
    for (lo, hi) in spans("assigned") {
        for cp in lo..=hi {
            let c = char::from_u32(cp).expect("un carácter");
            assert_eq!(isprintable(c), inside(cp), "{c:?} (U+{cp:04X})");
            checked += 1;
        }
    }
    assert!(checked > 280_000, "{checked}");
}

/// La carpeta temporal de `real_filesystem_paths`, para la prueba que
/// trabaja desde ella en otro proceso.
#[cfg(unix)]
const PROBE: &str = "DANPLAY_PATHS_PROBE";

/// `realpath`, `abspath`, `relpath` y `expanduser` de verdad, con enlaces
/// (uno a una carpeta y otro que se apunta a sí mismo). Lo esperado es lo que
/// daba `os.path` con esta misma carpeta. Va en otro proceso (esta misma
/// prueba, otra vez), que trabaja desde la carpeta temporal y con su propio
/// HOME: el directorio de trabajo y el entorno de este no se pueden cambiar
/// desde una prueba.
#[cfg(unix)]
#[test]
fn real_filesystem_paths() {
    let dir = crate::testkit::Scratch::new("rutas-de-verdad");
    let tmp = std::fs::canonicalize(dir.path()).expect("la carpeta");
    std::fs::create_dir_all(tmp.join("real/sub")).expect("carpetas");
    std::os::unix::fs::symlink(tmp.join("real"), tmp.join("enlace")).expect("enlace");
    std::os::unix::fs::symlink(tmp.join("vuelta"), tmp.join("vuelta")).expect("bucle");
    let out = std::process::Command::new(std::env::current_exe().expect("la prueba"))
        .args([
            "py::tests::compat::real_paths_probe",
            "--exact",
            "--ignored",
        ])
        .args(["--nocapture", "--test-threads=1"])
        .current_dir(&tmp)
        .env_clear()
        .env("HOME", "/home/prueba")
        .env(PROBE, &tmp)
        .output()
        .expect("la prueba en otro proceso");
    let said = String::from_utf8_lossy(&out.stdout) + String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{said}");
    assert!(said.contains("1 passed"), "no se llegó a probar: {said}");
}

#[cfg(unix)]
#[test]
#[ignore = "la llama real_filesystem_paths en otro proceso"]
fn real_paths_probe() {
    let Some(tmp) = std::env::var_os(PROBE) else {
        return;
    };
    let t = tmp.to_string_lossy().into_owned();
    let name = std::path::Path::new(&t)
        .file_name()
        .expect("nombre")
        .to_string_lossy()
        .into_owned();
    let depth = t.split('/').filter(|c| !c.is_empty()).count();
    let up_to_root = vec![".."; depth].join("/");
    // (ruta, realpath, abspath, relpath desde la carpeta)
    let cases: [(String, String, String, Option<String>); 8] = [
        (
            format!("{t}/enlace/sub/../x"),
            format!("{t}/real/x"),
            format!("{t}/enlace/x"),
            Some("enlace/x".into()),
        ),
        (
            format!("{t}/enlace/no-existe/y"),
            format!("{t}/real/no-existe/y"),
            format!("{t}/enlace/no-existe/y"),
            Some("enlace/no-existe/y".into()),
        ),
        (
            format!("{t}/vuelta/z"),
            format!("{t}/vuelta/z"),
            format!("{t}/vuelta/z"),
            Some("vuelta/z".into()),
        ),
        (
            "enlace/sub".into(),
            format!("{t}/real/sub"),
            format!("{t}/enlace/sub"),
            Some("enlace/sub".into()),
        ),
        (
            format!("../{name}/enlace"),
            format!("{t}/real"),
            format!("{t}/enlace"),
            Some("enlace".into()),
        ),
        ("/".into(), "/".into(), "/".into(), Some(up_to_root)),
        (String::new(), t.clone(), t.clone(), None),
        (".".into(), t.clone(), t.clone(), Some(".".into())),
    ];
    for (p, real, abs, rel) in cases {
        assert_eq!(realpath(&p), real, "realpath({p:?})");
        assert_eq!(abspath(&p), abs, "abspath({p:?})");
        assert_eq!(relpath(&p, &t), rel, "relpath({p:?})");
    }
    for (p, home) in [
        ("~", "/home/prueba"),
        ("~/Musica", "/home/prueba/Musica"),
        ("~otro/x", "~otro/x"),
        ("a/~", "a/~"),
        ("~/", "/home/prueba/"),
    ] {
        assert_eq!(expanduser(p), home, "expanduser({p:?})");
    }
}
