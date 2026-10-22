use super::*;

#[test]
fn typos_are_near() {
    assert!(!near("mañana", "manana"));
    assert!(near("hosanna", "hosnna") && near("hosanna", "hosanan"));
    assert!(near("hosanna", "hosannaa") && near("grita", "grit"));
    assert!(!near("rey", "rei"), "las palabras cortas no se tocan");
}

#[test]
fn parses_the_query() {
    let q = parse_query("*3.-Digno De Adorar* ✅ @Fulano (En Su Presencia)", "", &[]);
    assert_eq!(q.core, ["digno", "de", "adorar"]);
    assert_eq!(
        title_key("*3.-Digno De Adorar* ✅ @Fulano (En Su Presencia)", ""),
        "digno de adorar"
    );
    assert_eq!(
        parse_query("Lucero D'Alba", "", &[]).core,
        parse_query("Lucero DAlba", "", &[]).core
    );
}

#[test]
fn python_set_order() {
    // `list({0, 8, 16, 1})` en CPython: la tabla de 8 los deja así
    let mut s = IntSet::new();
    for k in [0, 8, 16, 1] {
        s.add(k);
    }
    assert_eq!(s.iter().collect::<Vec<_>>(), [0, 8, 16, 1]);
    let mut s = IntSet::new();
    for k in (0..40).rev() {
        s.add(k);
    }
    assert_eq!(s.iter().collect::<Vec<_>>(), (0..40).collect::<Vec<_>>());
    let mut s = IntSet::new();
    for k in [5000, 3, 4100, 9, 2048, 7, 6000, 13] {
        s.add(k);
    }
    assert_eq!(
        s.iter().collect::<Vec<_>>(),
        [2048, 3, 4100, 7, 5000, 9, 13, 6000]
    );
}
