//! Lo de tests/test_mega.py que no necesita ffmpeg: el espectrograma, el
//! orden de la red y el reparto con una red de mentira.
use super::*;

#[test]
#[allow(clippy::many_single_char_names)]
fn el_espectrograma_va_y_vuelve() {
    // un ruido de mentira (sin rand): una suma de senos de frecuencias raras
    let n = 50_000;
    let x: Vec<f32> = (0..2 * n)
        .map(|i| {
            let t = i as f64;
            ((t * 0.731).sin() * 0.5 + (t * 0.093).sin() * 0.3 + (t * 1.97).cos() * 0.2) as f32
        })
        .collect();
    let s = Stft::mega();
    let z = stft(&s, &x);
    assert_eq!((z[0].bins, z[0].tramas), (FRECS, n / SALTO + 1));
    let back = istft(&s, &z, n);
    assert!(back.iter().zip(&x).all(|(a, b)| (a - b).abs() < 2e-5));
    // el orden de la red: por frecuencia, cada canal, real e imaginaria
    let red = a_la_red(&z);
    assert_eq!(red.forma, [1, z[0].tramas, FRECS * 4]);
    assert_eq!(red.datos[3 * FRECS * 4 + 4 * 10 + 2], z[1].en(10, 3).re);
    assert_eq!(red.datos[3 * FRECS * 4 + 4 * 10 + 1], z[0].en(10, 3).im);
    // una máscara con el mismo orden vuelve a su sitio
    let mut dos = red.datos.clone();
    dos.extend(red.datos.iter().map(|v| 2.0 * v));
    let m = de_la_red(&Tensor32 {
        forma: vec![1, 2, z[0].tramas, FRECS * 4],
        datos: dos,
    });
    assert_eq!(m[0][1].datos, z[1].datos);
    assert_eq!(m[1][0].en(7, 2), z[0].en(7, 2) * 2.0);
}

#[test]
fn el_fundido_y_los_trozos() {
    assert_eq!(
        fundido(6, 2),
        [1.0 / 3.0, 2.0 / 3.0, 1.0, 1.0, 2.0 / 3.0, 1.0 / 3.0]
    );
    assert_eq!(trozos_de(10, 4, 0.25), 3);
    assert_eq!(trozos_de(0, 4, 0.25), 1);
    assert_eq!(tramas_de(44_100), 87);
}

/// Una red de mentira: cada cabeza oye una franja de frecuencias.
struct Franjas {
    cabezas: Vec<String>,
    franjas: Vec<(String, (usize, usize))>,
}

impl Red for Franjas {
    fn pasar(&mut self, entradas: Vec<(String, Tensor32)>) -> Result<Vec<Tensor32>, Fallo> {
        let t = entradas[0].1.forma[1];
        let n = self.cabezas.len();
        let mut datos = vec![0f32; n * t * FRECS * 4];
        for (i, h) in self.cabezas.iter().enumerate() {
            let (a, b) = self
                .franjas
                .iter()
                .find(|(k, _)| k == h)
                .map_or((0, 0), |(_, r)| *r);
            for ti in 0..t {
                for j in (4 * a..4 * b).step_by(2) {
                    datos[(i * t + ti) * FRECS * 4 + j] = 1.0;
                }
            }
        }
        Ok(vec![Tensor32 {
            forma: vec![1, n, t, FRECS * 4],
            datos,
        }])
    }
}

#[test]
fn el_reparto_suma_y_sigue_lo_que_oye_la_red() {
    let cabezas: Vec<String> = [
        "drums",
        "percussion",
        "congas",
        "tambourine",
        "lead-vocal",
        "back-vocal",
        "electric-guitar",
        "acoustic-guitar",
        "piano",
        "keys",
        "organ",
        "synth",
        "strings",
        "brass",
        "wind",
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .collect();
    let rate = TASA as f64;
    let n = TASA * 3;
    let tono = |hz: f64, a: f64| {
        (0..n).map(move |i| (a * (2.0 * std::f64::consts::PI * hz * i as f64 / rate).sin()) as f32)
    };
    let bajo: Vec<f32> = tono(200.0, 0.3).collect(); // «congas»
    let alto: Vec<f32> = tono(3000.0, 0.3).collect(); // «el kit»
    let bat1: Vec<f32> = bajo.iter().zip(&alto).map(|(a, b)| a + b).collect();
    let guit1: Vec<f32> = tono(700.0, 0.2).collect();
    let estereo = |v: &[f32]| [v, v].concat();
    let (bat, guit) = (estereo(&bat1), estereo(&guit1));
    let mezcla: Vec<f32> = bat.iter().zip(&guit).map(|(a, b)| a + b).collect();
    let hz = FRECS as f64 / (rate / 2.0);
    let fr = |a: f64, b: f64| ((a * hz) as usize, (b * hz) as usize);
    let mut red = Franjas {
        cabezas: cabezas.clone(),
        franjas: vec![
            ("congas".into(), fr(150.0, 260.0)),
            ("drums".into(), fr(2500.0, 3500.0)),
            ("electric-guitar".into(), fr(600.0, 800.0)),
        ],
    };
    let mut got: Vec<(String, Vec<f32>)> = Vec::new();
    separar(
        &mut red,
        &cabezas,
        44_100,
        &mezcla,
        &[
            ("drums".into(), bat.clone()),
            ("melodic".into(), guit.clone()),
        ],
        &mut |partes| {
            for (k, v) in partes {
                let n = v.len() / 2;
                if let Some((_, g)) = got.iter_mut().find(|(x, _)| *x == k) {
                    let (izq, der) = v.split_at(n);
                    let m = g.len() / 2;
                    let mut nuevo = g[..m].to_vec();
                    nuevo.extend_from_slice(izq);
                    nuevo.extend_from_slice(&g[m..]);
                    nuevo.extend_from_slice(der);
                    *g = nuevo;
                } else {
                    got.push((k, v));
                }
            }
            Ok(())
        },
        &mut |_, _| Ok(()),
        &mut || Ok(false),
        0.25,
    )
    .expect("separa");
    let de = |k: &str| {
        got.iter()
            .find(|(x, _)| x == k)
            .map(|(_, v)| v.clone())
            .expect(k)
    };
    let (kit, perc) = (de("drums"), de("percussion"));
    assert_eq!(kit.len(), bat.len());
    assert!(
        kit.iter()
            .zip(&perc)
            .zip(&bat)
            .all(|((a, b), c)| (a + b - c).abs() < 1e-4)
    );
    let core = TASA / 2..n - TASA / 2; // lejos de los bordes
    assert!(core.clone().all(|i| (perc[i] - bajo[i]).abs() < 0.02));
    assert!(core.clone().all(|i| (kit[i] - alto[i]).abs() < 0.02));
    let eg = de("eguitar");
    assert!(core.clone().all(|i| (eg[i] - guit1[i]).abs() < 0.02));
    assert!(
        de("piano").iter().all(|v| v.abs() < 1e-3),
        "lo que la red no oye no se reparte"
    );
}
