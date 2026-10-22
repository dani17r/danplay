//! Lo de tests/test_separation.py que no necesita ffmpeg, con una red de
//! mentira que devuelve lo que le entra.
use super::*;

/// Estéreo con varios tonos: sin nada en la frecuencia de Nyquist, que el
/// espectrograma de Demucs tira.
fn tonos(largo: usize) -> Vec<f32> {
    let t = |i: usize| i as f64 / TASA as f64;
    let pi2 = 2.0 * std::f64::consts::PI;
    let mut x: Vec<f32> = (0..largo)
        .map(|i| (0.3 * (pi2 * 220.0 * t(i)).sin() + 0.2 * (pi2 * 1375.0 * t(i)).sin()) as f32)
        .collect();
    x.extend((0..largo).map(|i| {
        (0.25 * (pi2 * 330.0 * t(i) + 0.4).sin() + 0.1 * (pi2 * 5000.0 * t(i)).sin()) as f32
    }));
    x
}

#[test]
fn el_espectrograma_y_su_inversa_devuelven_la_senal() {
    let x = tonos(SEGMENTO);
    let s = Stft::demucs();
    let (tramas, mag) = spec(&s, &x, 2);
    assert_eq!(tramas, SEGMENTO.div_ceil(SALTO));
    assert_eq!(mag.len(), 4 * FRECS * tramas);
    let back = ispec(&s, &mag, 1, tramas, SEGMENTO);
    assert_eq!(back.len(), x.len());
    let borde = 3 * NFFT;
    for c in 0..2 {
        for i in borde..SEGMENTO - borde {
            let k = c * SEGMENTO + i;
            assert!(
                (back[k] - x[k]).abs() < 1e-4,
                "{c} {i}: {} {}",
                back[k],
                x[k]
            );
        }
    }
}

#[test]
fn el_relleno_refleja_como_numpy() {
    assert_eq!(
        reflejar(&[1.0, 2.0, 3.0], 2, 2),
        [3.0, 2.0, 1.0, 2.0, 3.0, 2.0, 1.0]
    );
    // más largo que la señal: sigue reflejando
    assert_eq!(reflejar(&[1.0, 2.0], 3, 0), [2.0, 1.0, 2.0, 1.0, 2.0]);
    assert_eq!(reflejar(&[5.0], 1, 2), [5.0; 4]);
}

/// Una «red» que reparte la mezcla entre sus fuentes por la rama temporal
/// (la del espectrograma, a cero). Sumadas, la canción.
struct Identidad {
    partes: Vec<f32>,
    llamadas: usize,
}

impl Red for Identidad {
    fn pasar(&mut self, entradas: Vec<(String, Tensor32)>) -> Result<Vec<Tensor32>, Fallo> {
        self.llamadas += 1;
        let mix = &entradas[0].1;
        let mag = &entradas[1].1;
        assert_eq!(mix.forma, [1, 2, SEGMENTO]);
        assert_eq!(mag.forma, [1, 4, FRECS, 336]);
        let n = self.partes.len();
        let espectro = Tensor32 {
            forma: vec![1, n, 4, FRECS, 336],
            datos: vec![0.0; n * 4 * FRECS * 336],
        };
        let mut onda = Vec::with_capacity(n * 2 * SEGMENTO);
        for p in &self.partes {
            onda.extend(mix.datos.iter().map(|v| v * p));
        }
        Ok(vec![
            espectro,
            Tensor32 {
                forma: vec![1, n, 2, SEGMENTO],
                datos: onda,
            },
        ])
    }
}

fn juntar(bloques: &[Bloque]) -> (usize, Vec<Vec<f32>>) {
    let s = bloques[0].fuentes;
    let mut filas = vec![Vec::new(); s * 2];
    for b in bloques {
        for (f, fila) in filas.iter_mut().enumerate() {
            fila.extend_from_slice(&b.datos[f * b.n..(f + 1) * b.n]);
        }
    }
    (s, filas)
}

#[test]
fn los_trozos_se_juntan_en_la_cancion_entera() {
    for segundos in [3.0, 7.8, 21.3] {
        let largo = (segundos * TASA as f64) as usize;
        let x = tonos(largo);
        let mut red = Identidad {
            partes: vec![0.25, 0.75],
            llamadas: 0,
        };
        let mut bloques = Vec::new();
        let mut vistos = Vec::new();
        separar(
            &mut red,
            x.clone(),
            &mut |b| {
                bloques.push(b);
                Ok(())
            },
            &mut |d, t| {
                vistos.push((d, t));
                Ok(())
            },
            &mut || Ok(false),
        )
        .expect("separa");
        let (s, filas) = juntar(&bloques);
        assert_eq!(s, 2);
        assert_eq!(filas[0].len(), largo);
        assert_eq!(red.llamadas, largo.div_ceil(PASO));
        assert_eq!(vistos.last(), Some(&(red.llamadas, red.llamadas)));
        for c in 0..2 {
            for i in 0..largo {
                let v = x[c * largo + i];
                assert!((filas[c][i] - 0.25 * v).abs() < 1e-4);
                assert!((filas[c][i] + filas[2 + c][i] - v).abs() < 1e-4);
            }
        }
    }
}

#[test]
fn se_para_si_se_cancela_y_un_fallo_al_juntar_no_se_pierde() {
    let mut red = Identidad {
        partes: vec![0.5, 0.5],
        llamadas: 0,
    };
    let mut llamadas = 0;
    separar(
        &mut red,
        tonos(20 * TASA),
        &mut |_| Ok(()),
        &mut |_, _| Ok(()),
        &mut || {
            llamadas += 1;
            Ok(llamadas > 1)
        },
    )
    .expect("separa");
    assert_eq!(red.llamadas, 1);
    let mut red = Identidad {
        partes: vec![0.5, 0.5],
        llamadas: 0,
    };
    let r = separar(
        &mut red,
        tonos(12 * TASA),
        &mut |_| Err("disco lleno".into()),
        &mut |_, _| Ok(()),
        &mut || Ok(false),
    );
    assert_eq!(r.map_err(|e| e.to_string()), Err("disco lleno".to_owned()));
}

#[test]
fn cada_pista_tiene_su_onda_en_una_escala_comun() {
    let mut ondas = Ondas::nueva(vec!["drums".into(), "vocals".into()], 1000, 10);
    let mut fuerte = vec![0f32; 2 * 2 * 600];
    fuerte[50] = 0.8; // la batería pega fuerte en la primera columna
    fuerte[600 + 50] = 0.8;
    fuerte[3 * 600 + 250] = 0.2; // la voz, flojita, en la tercera
    ondas.sumar(&Bloque {
        fuentes: 2,
        n: 600,
        datos: fuerte,
    });
    let mut resto = vec![0f32; 2 * 2 * 400];
    resto[399] = -0.4; // en la última, un golpe en negativo
    ondas.sumar(&Bloque {
        fuentes: 2,
        n: 400,
        datos: resto,
    });
    let r = ondas.resultado(None);
    let (bat, voz) = (&r[0].picos, &r[1].picos);
    assert_eq!(bat[0], 1.0);
    assert_eq!(bat[9], 0.5);
    assert_eq!(bat[1..9].iter().sum::<f64>(), 0.0);
    // la escala es la de la pista más alta: la voz se ve pequeña
    assert_eq!(voz[2], 0.25);
    assert_eq!(voz[0], 0.0);
    assert!(r[0].rms[0] > 0.0 && r[0].rms[0] < bat[0]);
}

#[test]
fn una_fuente_que_solo_suena_un_momento_se_queda() {
    let mezcla = vec![1.0; 100];
    let mut piano = vec![0.0; 100];
    piano[40..45].fill(0.5); // un 5 % de la canción, a -3 dB de ella
    let n = presencia(&piano, &mezcla);
    assert!(n.db < CALLADA_DB + 20.0 && (n.activa - 0.05).abs() < 1e-9);
    assert!(se_queda(&n));
    assert!(!se_queda(&presencia(&[1e-4; 100], &mezcla)));
    assert!(!se_queda(&presencia(&[0.0; 100], &mezcla)));
    assert_eq!(
        presencia(&[0.0; 100], &[0.0; 100]),
        Nivel {
            db: -120.0,
            activa: 0.0
        }
    );
}

#[test]
fn traspuesto_como_numpy() {
    let (f, d) = traspuesto(&[2, 3], &[0.0, 1.0, 2.0, 3.0, 4.0, 5.0]);
    assert_eq!((f, d), (vec![3, 2], vec![0.0, 3.0, 1.0, 4.0, 2.0, 5.0]));
}
