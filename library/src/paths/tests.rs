use super::*;

/// La ruta con el separador del sistema.
fn native(p: &str) -> String {
    if cfg!(windows) {
        p.replace('/', "\\")
    } else {
        p.to_owned()
    }
}

#[test]
fn normalized_without_the_disk() {
    assert_eq!(normalize("/a//b/../c/."), native("/a/c"));
    assert_eq!(normalize("../../x"), native("../../x"));
    assert_eq!(normalize("a/../.."), native(".."));
    assert_eq!(normalize("/.."), native("/"));
    assert_eq!(normalize(""), ".");
    assert_eq!(normalize("./"), ".");
    assert_eq!(clean("./a//b/"), native("a/b"));
    assert_eq!(clean(""), ".");
}

#[test]
fn joined_split_and_named() {
    // unir y subir no tocan los separadores que ya hay: añaden el del sistema
    let sep = std::path::MAIN_SEPARATOR;
    assert_eq!(join("/a", "b"), format!("/a{sep}b"));
    assert_eq!(join("/a", "/b"), "/b");
    assert_eq!(file_name("/a/b.mp3"), "b.mp3");
    assert_eq!(file_name("/"), "");
    assert_eq!(parent("/a/b.mp3"), "/a");
    assert_eq!(parent("/a"), "/");
    assert_eq!(parent("b.mp3"), "");
    assert_eq!(parent("/"), "/");
    assert_eq!(stem(Path::new("a/Mi Gozo.mp3")), "Mi Gozo");
    assert_eq!(suffix(Path::new("a/Mi Gozo.MP3")), ".MP3");
    assert_eq!(suffix(Path::new(".flac")), "");
    assert_eq!(suffix(Path::new("x.")), "");
    assert_eq!(
        with_suffix(Path::new("a/b.wav"), ".mp3"),
        Path::new("a/b.mp3")
    );
    assert_eq!(
        with_suffix(Path::new("a/b.tar.gz"), ".mp3"),
        Path::new("a/b.tar.mp3")
    );
    assert_eq!(with_suffix(Path::new("a/b.wav"), ""), Path::new("a/b"));
}

#[test]
fn absolute_relative_and_common() {
    let cwd = std::env::current_dir().expect("carpeta de trabajo");
    assert_eq!(absolute("x/../y"), lossy(&cwd.join("y")));
    assert_eq!(absolute(""), lossy(&cwd));
    let root = lossy(cwd.ancestors().last().expect("la raíz"));
    let under = |p: &str| join(&root, &native(p));
    assert_eq!(
        relative(&under("a/b/c"), &under("a/d")),
        Some(native("../b/c"))
    );
    assert_eq!(relative(&under("a"), &under("a")), Some(".".into()));
    assert_eq!(relative("", &under("a")), None);
    assert_eq!(
        common(&[&under("a/b/c"), &under("a/b"), &under("a/b/d")]),
        Some(under("a/b"))
    );
    assert_eq!(common(&[&under("a"), "b"]), None);
    assert_eq!(common(&[]), None);
}

#[cfg(windows)]
#[test]
fn on_windows_the_drive_and_case_do_not_count() {
    assert_eq!(relative(r"D:\x", r"C:\y"), None);
    assert_eq!(relative(r"C:\Musica\A", r"c:\musica"), Some("A".into()));
    assert_eq!(
        common(&[r"C:\Musica\A", r"c:\musica\b"]),
        Some(r"C:\Musica".into())
    );
    assert_eq!(comparable("C:/Música/A"), r"c:\música\a");
}

#[test]
fn home_is_expanded() {
    let home = std::env::home_dir().expect("casa");
    let home = lossy(&home);
    let home = home.trim_end_matches(['/', '\\']);
    assert_eq!(expand_home("~/x"), format!("{home}/x"));
    assert_eq!(expand_home("~"), if home.is_empty() { "/" } else { home });
    assert_eq!(expand_home("~otro/x"), "~otro/x");
    assert_eq!(expand_home("/a/~"), "/a/~");
}

/// La ruta de verdad resuelve los enlaces de lo que existe y deja lo que no.
#[cfg(unix)]
#[test]
fn the_real_path_follows_links() {
    let dir = tempfile::tempdir().expect("temporal");
    let base = real_path(dir.path());
    std::fs::create_dir_all(base.join("de verdad/dentro")).expect("carpetas");
    std::os::unix::fs::symlink(base.join("de verdad"), base.join("enlace")).expect("enlace");
    assert_eq!(
        real_path(&base.join("enlace/dentro")),
        base.join("de verdad/dentro")
    );
    assert_eq!(
        real_path(&base.join("enlace/no/esta/..")),
        base.join("de verdad/no")
    );
    assert_eq!(real_path(&base.join("enlace/..")), base.clone());
    assert_eq!(real(&lossy(&base.join("x/./y"))), lossy(&base.join("x/y")));
}

#[test]
fn trees_are_removed_without_following_links() {
    let dir = tempfile::tempdir().expect("temporal");
    let tree = dir.path().join("arbol");
    std::fs::create_dir_all(tree.join("a/b")).expect("carpetas");
    std::fs::write(tree.join("a/b/c.txt"), "x").expect("archivo");
    let keep = dir.path().join("fuera.txt");
    std::fs::write(&keep, "x").expect("archivo");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&keep, tree.join("a/enlace")).expect("enlace");
    remove_tree(&tree);
    assert!(!tree.exists() && keep.is_file());
    remove_tree(&dir.path().join("no-esta"));
    assert!(same_file(&keep, &dir.path().join("./fuera.txt")));
    assert!(!same_file(&keep, &dir.path().join("no-esta")));
}

#[test]
fn patterns_like_the_settings_use() {
    assert!(glob_match_case(".git", ".*"));
    assert!(glob_match_case("$RECYCLE.BIN", "$RECYCLE.BIN"));
    assert!(!glob_match_case("xRECYCLE.BIN", "$RECYCLE.BIN"));
    assert!(glob_match_case("a/b/c", "*/b/*"));
    assert!(glob_match_case("b", "[a-c]"));
    assert!(!glob_match_case("b", "[!a-c]"));
    assert!(glob_match_case("[", "["));
    assert!(glob_match_case("-", "[a-]"));
    assert!(glob_match_case("x\ny", "x*y"));
    assert!(!glob_match_case("b", "[z-a]"), "un rango al revés no casa");
    assert!(glob_match_case("Proyecto.als", "*.als"));
    assert_eq!(glob_match("Proyecto.ALS", "*.als"), cfg!(windows));
}
