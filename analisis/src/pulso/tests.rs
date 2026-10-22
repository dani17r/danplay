//! Las mismas comprobaciones que tests/test_pulse.py, sin la red.
use super::rejilla::{Opciones, rejilla, terminar};
use super::*;

#[test]
fn el_espectrograma_tiene_cincuenta_tramas_por_segundo_y_oye_la_altura() {
    let seno = |hz: f64| -> Vec<f32> {
        (0..2 * TASA as usize)
            .map(|i| {
                (0.5 * (2.0 * std::f64::consts::PI * hz * i as f64 / f64::from(TASA)).sin()) as f32
            })
            .collect()
    };
    let (grave, agudo) = (espectrograma(&seno(110.0)), espectrograma(&seno(4000.0)));
    assert_eq!(grave.tramas, 1 + 2 * TASA as usize / SALTO);
    let banda_fuerte = |e: &Espectro| {
        let medio = &e.datos[(e.tramas / 2) * MELS..(e.tramas / 2 + 1) * MELS];
        medio
            .iter()
            .enumerate()
            .fold(
                (0, f32::MIN),
                |m, (i, v)| if *v > m.1 { (i, *v) } else { m },
            )
            .0
    };
    assert!(banda_fuerte(&grave) < banda_fuerte(&agudo));
    assert_eq!(espectrograma(&[0.0; 100]).tramas, 2);
}

#[test]
fn los_filtros_van_de_30_hz_a_11_khz() {
    let f = filtros_mel();
    assert_eq!(f.len(), (NFFT / 2 + 1) * MELS);
    let hz = np::linspace(0.0, f64::from(TASA) / 2.0, NFFT / 2 + 1);
    let usados: Vec<f64> = (0..=NFFT / 2)
        .filter(|i| (0..MELS).any(|j| f[i * MELS + j] > 0.0))
        .map(|i| hz[i])
        .collect();
    assert!(usados[0] >= 30.0 && usados[0] < 60.0);
    assert!(usados[usados.len() - 1] > 10_500.0 && usados[usados.len() - 1] <= 11_000.0);
}

/// Una «red» que dice pulso cada `cada` tramas (y «1» cada cuatro pulsos),
/// contando desde el principio de la canción: la posición viaja en la
/// primera banda.
type Logits = Result<(Vec<f32>, Vec<f32>), String>;

fn red_de_mentira(cada: i64) -> impl FnMut(&[f32], usize) -> Logits {
    move |datos, filas| {
        let pos: Vec<i64> = (0..filas)
            .map(|i| datos[i * MELS].round_ties_even() as i64)
            .collect();
        let b = pos
            .iter()
            .map(|p| if p.rem_euclid(cada) == 0 { 5.0 } else { -5.0 })
            .collect();
        let d = pos
            .iter()
            .map(|p| {
                if p.rem_euclid(4 * cada) == 0 {
                    5.0
                } else {
                    -5.0
                }
            })
            .collect();
        Ok((b, d))
    }
}

fn cancion(tramas: usize) -> Espectro {
    let mut datos = vec![0f32; tramas * MELS];
    for t in 0..tramas {
        datos[t * MELS] = t as f32;
    }
    Espectro { tramas, datos }
}

#[test]
fn los_trozos_se_juntan_trama_a_trama() {
    for tramas in [300, 1500, 4000, 9999] {
        let mut vistos = Vec::new();
        let (beat, down) = logits(&cancion(tramas), red_de_mentira(25), |h, t| {
            vistos.push((h, t));
        })
        .expect("la red");
        assert_eq!(beat.len(), tramas);
        for t in 0..tramas {
            assert_eq!(beat[t] > 0.0, t % 25 == 0, "trama {t} de {tramas}");
            assert_eq!(down[t] > 0.0, t % 100 == 0);
        }
        let total = inicios(tramas).len();
        assert_eq!(vistos, (1..=total).map(|h| (h, total)).collect::<Vec<_>>());
    }
}

#[test]
fn los_pulsos_son_los_picos_y_cada_uno_va_a_su_pulso() {
    let mut beat = vec![-3.0; 500];
    for (i, v) in [(50, 2.0), (100, 1.0), (101, 1.0), (150, 3.0), (200, -0.5)] {
        beat[i] = v; // 100 y 101 pegados; 200 no llega
    }
    let mut down = vec![-3.0; 500];
    down[52] = 1.0;
    down[149] = 2.0; // un poco movidos: van al pulso de al lado
    let (pulsos, unos) = pulsos_y_unos(&beat, &down);
    assert_eq!(pulsos, [1.0, 2.01, 3.0]);
    assert_eq!(unos, [1.0, 3.0]);
}

/// Lo que diría la red: un pico de pulso en cada `pulsos` (en segundos) y uno
/// de «1» en cada `unos`.
fn red(pulsos: &[f64], unos: &[f64], segundos: f64) -> (Vec<f64>, Vec<f64>) {
    let tramas = (segundos * FPS) as usize;
    let pico = |marcas: &[f64]| -> Vec<f64> {
        (0..tramas)
            .map(|i| {
                let t = i as f64 / FPS;
                marcas
                    .iter()
                    .fold(-6.0f64, |m, b| m.max(4.0 - 0.5 * ((t - b) * FPS).powi(2)))
            })
            .collect()
    };
    (pico(pulsos), pico(unos))
}

#[test]
fn la_rejilla_va_en_un_solo_nivel_de_principio_a_fin() {
    // lo que pasó en canciones de verdad: un tramo en que la red oye el pulso
    // al doble, un golpe suelto de más y un pulso que no oye
    let periodo = 0.6;
    let pulsos: Vec<f64> = (0..60).map(|i| 1.0 + periodo * f64::from(i)).collect();
    let mut oidos: Vec<f64> = pulsos
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != 20)
        .map(|(_, b)| *b)
        .collect();
    oidos.extend(pulsos[30..42].iter().map(|b| b + periodo / 2.0));
    oidos.push(pulsos[50] + 0.17);
    oidos.sort_by(f64::total_cmp);
    let unos: Vec<f64> = pulsos.iter().step_by(4).copied().collect();
    let (b, d) = red(&oidos, &unos, 60.0);
    let g = rejilla(&b, &d, Opciones::default()).expect("rejilla");
    assert_eq!(g.fin.pulsos.len(), pulsos.len());
    assert!(
        g.fin
            .pulsos
            .iter()
            .zip(&pulsos)
            .all(|(a, b)| (a - b).abs() < 0.004)
    );
    assert!((g.fin.bpm - 100.0).abs() < 0.2, "{}", g.fin.bpm);
    assert_eq!(g.fin.compas, 4);
    assert_eq!(g.fin.compases, (0..60).step_by(4).collect::<Vec<i64>>());
}

#[test]
fn sin_pulso_lo_dice() {
    let (b, d) = red(&[1.0, 2.0], &[], 60.0);
    assert_eq!(
        rejilla(&b, &d, Opciones::default()).map(|_| ()),
        Err("la cancion no tiene pulso claro".into())
    );
    assert!(terminar(&[1.0], &[], None).is_err());
}

#[test]
fn las_claves_son_las_de_python() {
    // hashlib.sha1(b"/m/a.mp3|10|5").hexdigest()
    assert_eq!(
        clave("/m/a.mp3", 10, 5),
        "68a796d3c01a56c441b4c1dd857a90b7a006cd73"
    );
    // como Python: la suma en float ya llega al segundo siguiente
    assert_eq!(mtime_python(1_700_000_000, 999_999_999), 1_700_000_001);
    assert_eq!(mtime_python(1_700_000_000, 999_000_000), 1_700_000_000);
    assert_eq!(mtime_python(5, 500_000_000), 5);
}

#[test]
fn lo_que_dijo_la_red_vuelve_igual() {
    let dir = std::env::temp_dir().join(format!("danplay-crudo-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("carpeta");
    let ruta = dir.join("x.npz");
    let beat = [0.5, -1.25, 3.0, 65504.0];
    crudo::guardar(&ruta, &beat, &[1.0], &[], 100.0, 12.5).expect("guardar");
    let c = crudo::leer(&ruta).expect("leer");
    assert_eq!(c.beat, [0.5, -1.25, 3.0, 65504.0]);
    assert_eq!(
        (c.down.as_slice(), c.low.len(), c.low_fps, c.seconds),
        (&[1.0f32][..], 0, 100.0, 12.5)
    );
    // uno roto se borra
    std::fs::write(&ruta, b"no es un zip").expect("escribir");
    assert_eq!(crudo::leer(&ruta), None);
    assert!(!ruta.exists());
    assert_eq!(crudo::leer(&ruta), None);
    std::fs::remove_dir_all(&dir).expect("borrar");
}
