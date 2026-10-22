use super::*;
use std::path::PathBuf;

/// Los dos de tests/fixtures los hizo `torch.save` (2.14): uno de verdad y
/// uno con un pickle que ejecutaría una orden al abrirlo.
fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../tests/fixtures")
        .join(name)
}

#[test]
fn weights_come_out_as_torch_saved_them() {
    let weights = read(&fixture("weights.ckpt"), None).expect("se lee");
    let names: Vec<&str> = weights.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(
        names,
        ["a", "b", "c", "d", "e", "layer.weight"],
        "sin el «epoch» de fuera"
    );
    let tensor = |n: &str| {
        weights
            .iter()
            .find(|(k, _)| k == n)
            .map(|(_, t)| Rc::clone(t))
            .expect(n)
    };
    let (a, c) = (tensor("a"), tensor("c"));
    assert_eq!(
        (a.shape.as_slice(), a.steps.as_slice()),
        (&[2, 3][..], &[3, 1][..])
    );
    // una vista traspuesta del mismo almacén
    assert!(Rc::ptr_eq(&a.storage, &c.storage));
    assert_eq!(
        (c.shape.as_slice(), c.steps.as_slice()),
        (&[3, 2][..], &[1, 3][..])
    );
    assert_eq!(a.storage.kind, Type::F32);
    assert_eq!(a.storage.elements, 6);
    assert_eq!(tensor("b").storage.kind, Type::F16);
    assert_eq!(tensor("d").storage.kind, Type::BF16);
    let scalar = tensor("e");
    assert!(scalar.shape.is_empty(), "un escalar");
    assert_eq!(scalar.storage.kind, Type::I64);
    // los números, donde dice: el primero de «a»
    let raw = std::fs::read(fixture("weights.ckpt")).expect("leer");
    let i = usize::try_from(a.storage.start).expect("cabe");
    let first = f32::from_le_bytes(raw[i..i + 4].try_into().expect("4 bytes"));
    assert!((first - 1.540_996).abs() < 1e-6);
}

#[test]
fn checkpoint_that_runs_code_is_refused() {
    match read(&fixture("malicious.ckpt"), None) {
        Err(Error::Checkpoint(m)) => assert!(m.contains("no es un peso"), "{m}"),
        other => panic!("{other:?}"),
    }
    let this_file = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/weights.rs");
    assert!(matches!(read(&this_file, None), Err(Error::Checkpoint(_))));
    assert!(matches!(
        read(Path::new("/no/existe.ckpt"), None),
        Err(Error::Io(std::io::ErrorKind::NotFound, _))
    ));
}

#[test]
fn pickle_machine_handles_the_usual_cases() {
    // un dict con texto, enteros grandes y negativos, una lista y un real,
    // en protocolo 2 (lo de `pickle.dumps(..., protocol=2)`)
    let data = b"\x80\x02}q\x00(X\x01\x00\x00\x00aq\x01J\xff\xff\xff\xffX\x01\x00\x00\x00bq\x02\x8a\x05\x00\x00\x00\x00\x01X\x01\x00\x00\x00cq\x03]q\x04(K\x01G?\xf8\x00\x00\x00\x00\x00\x00eu.";
    let f = File::open(fixture("weights.ckpt")).expect("abrir");
    let mut zip = Zip {
        file: zip::ZipArchive::new(f).expect("zip"),
        prefix: String::new(),
        already_read: HashMap::new(),
    };
    let V::Dict(d) = unpack(data, &mut zip).expect("se lee") else {
        panic!("un dict")
    };
    let d = d.borrow();
    assert!(matches!(d[0], (V::Text(ref k), V::Integer(-1)) if &**k == "a"));
    assert!(matches!(d[1], (V::Text(_), V::Integer(4_294_967_296))));
    let V::List(ref l) = d[2].1 else {
        panic!("una lista")
    };
    assert!(
        matches!(l.borrow()[..], [V::Integer(1), V::Real(r)] if (r - 1.5).abs() < f64::EPSILON)
    );
    // y lo que no conoce, lo rechaza
    assert!(matches!(
        unpack(b"\x80\x02cos\nsystem\n.", &mut zip),
        Err(Error::Checkpoint(_))
    ));
    assert!(matches!(
        unpack(b"\x80\x02\x8c\x02os\x8c\x06system\x93.", &mut zip),
        Err(Error::Checkpoint(_))
    ));
    assert!(matches!(
        unpack(b"\x80\x02h\x05.", &mut zip),
        Err(Error::Pickle(_))
    ));
    assert!(matches!(
        unpack(b"\x80\x02", &mut zip),
        Err(Error::Pickle(_))
    ));
}
