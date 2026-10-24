#![allow(
    clippy::float_cmp,
    reason = "los redondeos tienen que dar ese número exacto"
)]
use super::*;
use serde_json::json;

mod compat;

#[test]
fn rounds_like_python() {
    assert_eq!(round(2.675, 2), 2.67);
    assert_eq!(round(0.125, 2), 0.12);
    assert_eq!(round(0.375, 2), 0.38);
    assert_eq!(round(1.005, 2), 1.0);
    assert_eq!(round(-0.5, 0), -0.0);
    assert_eq!(round_int(2.5), 2);
    assert_eq!(round_int(3.5), 4);
    assert_eq!(round_int(-2.5), -2);
}

#[test]
fn writes_floats_like_python() {
    for (x, s) in [
        (1.0, "1.0"),
        (0.1, "0.1"),
        (1e-5, "1e-05"),
        (0.0001, "0.0001"),
        (1e16, "1e+16"),
        (1e15, "1000000000000000.0"),
        (123.456, "123.456"),
        (-2.5e-7, "-2.5e-07"),
        (f64::MAX, "1.7976931348623157e+308"),
        (f64::INFINITY, "inf"),
        (-0.0, "-0.0"),
    ] {
        assert_eq!(repr_float(x), s, "{x}");
    }
}

#[test]
fn reads_numbers_as_float_and_int() {
    assert_eq!(parse_float(" 1_000.5 "), Some(1000.5));
    assert_eq!(parse_float("1__0"), None);
    assert_eq!(parse_float("_1"), None);
    assert_eq!(parse_float("-inf"), Some(f64::NEG_INFINITY));
    assert_eq!(parse_float("+-1"), None);
    assert_eq!(parse_float(""), None);
    assert_eq!(float_of(&json!(true)), Some(1.0));
    assert_eq!(float_of(&json!(null)), None);
    assert_eq!(float_of(&json!("3")), Some(3.0));
    assert_eq!(int_of(&json!(3.9)), Some(3));
    assert_eq!(int_of(&json!(-3.9)), Some(-3));
    assert_eq!(int_of(&json!("3.5")), None);
    assert_eq!(int_of(&json!(" -12 ")), Some(-12));
    assert_eq!(int_of(&json!("1_2")), Some(12));
    assert_eq!(int_of(&json!(false)), Some(0));
}

#[test]
fn str_and_repr_like_python() {
    assert_eq!(str_of(&json!(null)), "None");
    assert_eq!(str_of(&json!(1.0)), "1.0");
    assert_eq!(str_of(&json!(["a", 1, true])), "['a', 1, True]");
    assert_eq!(str_of(&json!({"k": "it's"})), "{'k': \"it's\"}");
    assert_eq!(repr_str("a\nb\u{7}"), "'a\\nb\\x07'");
}

#[test]
fn posix_paths() {
    assert_eq!(posix_normpath("//a//b/../c/."), "//a/c");
    assert_eq!(posix_normpath("///a"), "/a");
    assert_eq!(posix_normpath("../../x"), "../../x");
    assert_eq!(posix_normpath(""), ".");
    assert_eq!(posix_join("/a", "/b"), "/b");
    assert_eq!(posix_split("/a/b/"), ("/a/b".into(), String::new()));
    assert_eq!(posix_split("//x"), ("//".into(), "x".into()));
    assert_eq!(posix_commonpath(&["/a/b/c", "/a/b"]), Some("/a/b".into()));
    assert_eq!(posix_commonpath(&["/a", "b"]), None);
    assert_eq!(posix_relpath_abs("/a/b/c", "/a/d"), "../b/c");
}

#[test]
fn windows_paths() {
    assert_eq!(nt_splitroot(r"C:\x\y"), ("C:", "\\", r"x\y"));
    assert_eq!(nt_splitroot(r"\\srv\share\x"), (r"\\srv\share", "\\", "x"));
    assert_eq!(nt_splitroot("C:x"), ("C:", "", "x"));
    assert_eq!(nt_normpath("C:/a/../b/./c"), r"C:\b\c");
    assert_eq!(nt_normpath(r"\..\a"), r"\a");
    assert_eq!(nt_join(r"C:\a", "b"), r"C:\a\b");
    assert_eq!(nt_join(r"C:\a", r"D:b"), "D:b");
    assert_eq!(nt_join(r"\\srv\share", "x"), r"\\srv\share\x");
    assert_eq!(
        nt_commonpath(&[r"C:\Musica\A", r"c:\musica\b"]),
        Some(r"C:\Musica".into())
    );
    assert!(nt_isabs(r"C:\x") && !nt_isabs("C:x") && nt_isabs(r"\\srv\s"));
}

#[test]
fn fnmatch_like_python() {
    assert!(fnmatchcase(".git", ".*"));
    assert!(fnmatchcase("$RECYCLE.BIN", "$RECYCLE.BIN"));
    assert!(!fnmatchcase("xRECYCLE.BIN", "$RECYCLE.BIN"));
    assert!(fnmatchcase("a/b/c", "*/b/*"));
    assert!(fnmatchcase("b", "[a-c]"));
    assert!(!fnmatchcase("b", "[!a-c]"));
    assert!(fnmatchcase("[", "["));
    assert!(fnmatchcase("-", "[a-]"));
    assert!(fnmatchcase("x\ny", "x*y"));
}
