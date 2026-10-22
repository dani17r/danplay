//! Lo de tests/test_mega.py que no necesita ffmpeg: el espectrograma, el
//! orden de la red y el reparto con una red de mentira.
use super::*;

#[test]
#[allow(clippy::many_single_char_names)]
fn spectrogram_round_trip() {
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
    assert_eq!((z[0].bins, z[0].frames), (FREQS, n / HOP + 1));
    let back = istft(&s, &z, n);
    assert!(back.iter().zip(&x).all(|(a, b)| (a - b).abs() < 2e-5));
    // el orden de la red: por frecuencia, cada canal, real e imaginaria
    let net = to_net(&z);
    assert_eq!(net.shape, [1, z[0].frames, FREQS * 4]);
    assert_eq!(net.data[3 * FREQS * 4 + 4 * 10 + 2], z[1].en(10, 3).re);
    assert_eq!(net.data[3 * FREQS * 4 + 4 * 10 + 1], z[0].en(10, 3).im);
    // una máscara con el mismo orden vuelve a su sitio
    let mut two_heads = net.data.clone();
    two_heads.extend(net.data.iter().map(|v| 2.0 * v));
    let m = from_net(&Tensor32 {
        shape: vec![1, 2, z[0].frames, FREQS * 4],
        data: two_heads,
    });
    assert_eq!(m[0][1].data, z[1].data);
    assert_eq!(m[1][0].en(7, 2), z[0].en(7, 2) * 2.0);
}

#[test]
fn crossfade_and_chunk_counts() {
    assert_eq!(
        crossfade(6, 2),
        [1.0 / 3.0, 2.0 / 3.0, 1.0, 1.0, 2.0 / 3.0, 1.0 / 3.0]
    );
    assert_eq!(chunks_for(10, 4, 0.25), 3);
    assert_eq!(chunks_for(0, 4, 0.25), 1);
    assert_eq!(frames_for(44_100), 87);
}

/// Una red de mentira: cada cabeza oye una franja de frecuencias.
struct Bands {
    heads: Vec<String>,
    bands: Vec<(String, (usize, usize))>,
}

impl Net for Bands {
    fn forward(&mut self, inputs: Vec<(String, Tensor32)>) -> Result<Vec<Tensor32>, Error> {
        let t = inputs[0].1.shape[1];
        let n = self.heads.len();
        let mut data = vec![0f32; n * t * FREQS * 4];
        for (i, h) in self.heads.iter().enumerate() {
            let (a, b) = self
                .bands
                .iter()
                .find(|(k, _)| k == h)
                .map_or((0, 0), |(_, r)| *r);
            for ti in 0..t {
                for j in (4 * a..4 * b).step_by(2) {
                    data[(i * t + ti) * FREQS * 4 + j] = 1.0;
                }
            }
        }
        Ok(vec![Tensor32 {
            shape: vec![1, n, t, FREQS * 4],
            data,
        }])
    }
}

#[test]
fn split_adds_up_and_follows_what_the_net_hears() {
    let heads: Vec<String> = [
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
    let rate = RATE as f64;
    let n = RATE * 3;
    let tone = |hz: f64, a: f64| {
        (0..n).map(move |i| (a * (2.0 * std::f64::consts::PI * hz * i as f64 / rate).sin()) as f32)
    };
    let bass: Vec<f32> = tone(200.0, 0.3).collect(); // «congas»
    let high: Vec<f32> = tone(3000.0, 0.3).collect(); // «el kit»
    let bat1: Vec<f32> = bass.iter().zip(&high).map(|(a, b)| a + b).collect();
    let guitar1: Vec<f32> = tone(700.0, 0.2).collect();
    let stereo = |v: &[f32]| [v, v].concat();
    let (bat, guitar) = (stereo(&bat1), stereo(&guitar1));
    let mix: Vec<f32> = bat.iter().zip(&guitar).map(|(a, b)| a + b).collect();
    let hz = FREQS as f64 / (rate / 2.0);
    let fr = |a: f64, b: f64| ((a * hz) as usize, (b * hz) as usize);
    let mut net = Bands {
        heads: heads.clone(),
        bands: vec![
            ("congas".into(), fr(150.0, 260.0)),
            ("drums".into(), fr(2500.0, 3500.0)),
            ("electric-guitar".into(), fr(600.0, 800.0)),
        ],
    };
    let mut got: Vec<(String, Vec<f32>)> = Vec::new();
    separate(
        &mut net,
        &heads,
        44_100,
        &mix,
        &[
            ("drums".into(), bat.clone()),
            ("melodic".into(), guitar.clone()),
        ],
        &mut |parts| {
            for (k, v) in parts {
                let n = v.len() / 2;
                if let Some((_, g)) = got.iter_mut().find(|(x, _)| *x == k) {
                    let (left, der) = v.split_at(n);
                    let m = g.len() / 2;
                    let mut new = g[..m].to_vec();
                    new.extend_from_slice(left);
                    new.extend_from_slice(&g[m..]);
                    new.extend_from_slice(der);
                    *g = new;
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
    let core = RATE / 2..n - RATE / 2; // lejos de los bordes
    assert!(core.clone().all(|i| (perc[i] - bass[i]).abs() < 0.02));
    assert!(core.clone().all(|i| (kit[i] - high[i]).abs() < 0.02));
    let eg = de("eguitar");
    assert!(core.clone().all(|i| (eg[i] - guitar1[i]).abs() < 0.02));
    assert!(
        de("piano").iter().all(|v| v.abs() < 1e-3),
        "lo que la red no oye no se reparte"
    );
}
