use super::*;
use std::path::PathBuf;

/// Los dos de tests/fixtures los hizo `torch.save` (2.14): uno de verdad y
/// uno con un pickle que ejecutaría una orden al abrirlo.
fn fixture(nombre: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../tests/fixtures")
        .join(nombre)
}

#[test]
fn los_pesos_salen_como_los_guardo_torch() {
    let pesos = leer(&fixture("pesos.ckpt"), None).expect("se lee");
    let nombres: Vec<&str> = pesos.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(
        nombres,
        ["a", "b", "c", "d", "e", "layer.weight"],
        "sin el «epoch» de fuera"
    );
    let tensor = |n: &str| {
        pesos
            .iter()
            .find(|(k, _)| k == n)
            .map(|(_, t)| Rc::clone(t))
            .expect(n)
    };
    let (a, c) = (tensor("a"), tensor("c"));
    assert_eq!(
        (a.forma.as_slice(), a.pasos.as_slice()),
        (&[2, 3][..], &[3, 1][..])
    );
    // una vista traspuesta del mismo almacén
    assert!(Rc::ptr_eq(&a.almacen, &c.almacen));
    assert_eq!(
        (c.forma.as_slice(), c.pasos.as_slice()),
        (&[3, 2][..], &[1, 3][..])
    );
    assert_eq!(a.almacen.tipo, Tipo::F32);
    assert_eq!(a.almacen.elementos, 6);
    assert_eq!(tensor("b").almacen.tipo, Tipo::F16);
    assert_eq!(tensor("d").almacen.tipo, Tipo::BF16);
    let escalar = tensor("e");
    assert!(escalar.forma.is_empty(), "un escalar");
    assert_eq!(escalar.almacen.tipo, Tipo::I64);
    // los números, donde dice: el primero de «a»
    let crudo = std::fs::read(fixture("pesos.ckpt")).expect("leer");
    let i = usize::try_from(a.almacen.inicio).expect("cabe");
    let primero = f32::from_le_bytes(crudo[i..i + 4].try_into().expect("4 bytes"));
    assert!((primero - 1.540_996).abs() < 1e-6);
}

#[test]
fn un_checkpoint_que_ejecuta_algo_no_se_abre() {
    match leer(&fixture("trampa.ckpt"), None) {
        Err(Fallo::Checkpoint(m)) => assert!(m.contains("no es un peso"), "{m}"),
        otro => panic!("{otro:?}"),
    }
    let este = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/pesos.rs");
    assert!(matches!(leer(&este, None), Err(Fallo::Checkpoint(_))));
    assert!(matches!(
        leer(Path::new("/no/existe.ckpt"), None),
        Err(Fallo::Archivo(std::io::ErrorKind::NotFound, _))
    ));
}

#[test]
fn la_maquina_de_pickle_entiende_lo_de_siempre() {
    // un dict con texto, enteros grandes y negativos, una lista y un real,
    // en protocolo 2 (lo de `pickle.dumps(..., protocol=2)`)
    let datos = b"\x80\x02}q\x00(X\x01\x00\x00\x00aq\x01J\xff\xff\xff\xffX\x01\x00\x00\x00bq\x02\x8a\x05\x00\x00\x00\x00\x01X\x01\x00\x00\x00cq\x03]q\x04(K\x01G?\xf8\x00\x00\x00\x00\x00\x00eu.";
    let f = File::open(fixture("pesos.ckpt")).expect("abrir");
    let mut zip = Zip {
        archivo: zip::ZipArchive::new(f).expect("zip"),
        prefijo: String::new(),
        leidos: HashMap::new(),
    };
    let V::Dict(d) = deshacer(datos, &mut zip).expect("se lee") else {
        panic!("un dict")
    };
    let d = d.borrow();
    assert!(matches!(d[0], (V::Texto(ref k), V::Entero(-1)) if &**k == "a"));
    assert!(matches!(d[1], (V::Texto(_), V::Entero(4_294_967_296))));
    let V::Lista(ref l) = d[2].1 else {
        panic!("una lista")
    };
    assert!(matches!(l.borrow()[..], [V::Entero(1), V::Real(r)] if (r - 1.5).abs() < f64::EPSILON));
    // y lo que no conoce, lo rechaza
    assert!(matches!(
        deshacer(b"\x80\x02cos\nsystem\n.", &mut zip),
        Err(Fallo::Checkpoint(_))
    ));
    assert!(matches!(
        deshacer(b"\x80\x02\x8c\x02os\x8c\x06system\x93.", &mut zip),
        Err(Fallo::Checkpoint(_))
    ));
    assert!(matches!(
        deshacer(b"\x80\x02h\x05.", &mut zip),
        Err(Fallo::Pickle(_))
    ));
    assert!(matches!(
        deshacer(b"\x80\x02", &mut zip),
        Err(Fallo::Pickle(_))
    ));
}
