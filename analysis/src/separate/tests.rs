//! Lo de tests/test_separation.py que no necesita ffmpeg, con una red de
//! mentira que devuelve lo que le entra.
use super::*;

/// Estéreo con varios tonos: sin nada en la frecuencia de Nyquist, que el
/// espectrograma de Demucs tira.
fn tones(length: usize) -> Vec<f32> {
    let t = |i: usize| i as f64 / RATE as f64;
    let pi2 = 2.0 * std::f64::consts::PI;
    let mut x: Vec<f32> = (0..length)
        .map(|i| (0.3 * (pi2 * 220.0 * t(i)).sin() + 0.2 * (pi2 * 1375.0 * t(i)).sin()) as f32)
        .collect();
    x.extend((0..length).map(|i| {
        (0.25 * (pi2 * 330.0 * t(i) + 0.4).sin() + 0.1 * (pi2 * 5000.0 * t(i)).sin()) as f32
    }));
    x
}

#[test]
fn spectrogram_and_inverse_give_back_the_signal() {
    let x = tones(SEGMENT);
    let s = Stft::demucs();
    let (frames, mag) = spec(&s, &x, 2);
    assert_eq!(frames, SEGMENT.div_ceil(HOP));
    assert_eq!(mag.len(), 4 * FREQS * frames);
    let back = ispec(&s, &mag, 1, frames, SEGMENT);
    assert_eq!(back.len(), x.len());
    let border = 3 * NFFT;
    for c in 0..2 {
        for i in border..SEGMENT - border {
            let k = c * SEGMENT + i;
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
fn padding_reflects_like_numpy() {
    assert_eq!(
        reflect(&[1.0, 2.0, 3.0], 2, 2),
        [3.0, 2.0, 1.0, 2.0, 3.0, 2.0, 1.0]
    );
    // más largo que la señal: sigue reflejando
    assert_eq!(reflect(&[1.0, 2.0], 3, 0), [2.0, 1.0, 2.0, 1.0, 2.0]);
    assert_eq!(reflect(&[5.0], 1, 2), [5.0; 4]);
}

/// Una «red» que reparte la mezcla entre sus fuentes por la rama temporal
/// (la del espectrograma, a cero). Sumadas, la canción.
struct Identity {
    parts: Vec<f32>,
    calls: usize,
}

impl Net for Identity {
    fn forward(&mut self, inputs: Vec<(String, Tensor32)>) -> Result<Vec<Tensor32>, Error> {
        self.calls += 1;
        let mix = &inputs[0].1;
        let mag = &inputs[1].1;
        assert_eq!(mix.shape, [1, 2, SEGMENT]);
        assert_eq!(mag.shape, [1, 4, FREQS, 336]);
        let n = self.parts.len();
        let spectrum = Tensor32 {
            shape: vec![1, n, 4, FREQS, 336],
            data: vec![0.0; n * 4 * FREQS * 336],
        };
        let mut waveform = Vec::with_capacity(n * 2 * SEGMENT);
        for p in &self.parts {
            waveform.extend(mix.data.iter().map(|v| v * p));
        }
        Ok(vec![
            spectrum,
            Tensor32 {
                shape: vec![1, n, 2, SEGMENT],
                data: waveform,
            },
        ])
    }
}

fn join_blocks(blocks: &[Block]) -> (usize, Vec<Vec<f32>>) {
    let s = blocks[0].sources;
    let mut rows = vec![Vec::new(); s * 2];
    for b in blocks {
        for (f, row) in rows.iter_mut().enumerate() {
            row.extend_from_slice(&b.data[f * b.n..(f + 1) * b.n]);
        }
    }
    (s, rows)
}

#[test]
fn chunks_join_into_the_whole_song() {
    for segundos in [3.0, 7.8, 21.3] {
        let length = (segundos * RATE as f64) as usize;
        let x = tones(length);
        let mut net = Identity {
            parts: vec![0.25, 0.75],
            calls: 0,
        };
        let mut blocks = Vec::new();
        let mut seen = Vec::new();
        separate(
            &mut net,
            x.clone(),
            &mut |b| {
                blocks.push(b);
                Ok(())
            },
            &mut |d, t| {
                seen.push((d, t));
                Ok(())
            },
            &mut || Ok(false),
        )
        .expect("separa");
        let (s, rows) = join_blocks(&blocks);
        assert_eq!(s, 2);
        assert_eq!(rows[0].len(), length);
        assert_eq!(net.calls, length.div_ceil(STRIDE));
        assert_eq!(seen.last(), Some(&(net.calls, net.calls)));
        for c in 0..2 {
            for i in 0..length {
                let v = x[c * length + i];
                assert!((rows[c][i] - 0.25 * v).abs() < 1e-4);
                assert!((rows[c][i] + rows[2 + c][i] - v).abs() < 1e-4);
            }
        }
    }
}

#[test]
fn stops_on_cancel_and_a_join_error_is_not_lost() {
    let mut net = Identity {
        parts: vec![0.5, 0.5],
        calls: 0,
    };
    let mut calls = 0;
    separate(
        &mut net,
        tones(20 * RATE),
        &mut |_| Ok(()),
        &mut |_, _| Ok(()),
        &mut || {
            calls += 1;
            Ok(calls > 1)
        },
    )
    .expect("separa");
    assert_eq!(net.calls, 1);
    let mut net = Identity {
        parts: vec![0.5, 0.5],
        calls: 0,
    };
    let r = separate(
        &mut net,
        tones(12 * RATE),
        &mut |_| Err("disco lleno".into()),
        &mut |_, _| Ok(()),
        &mut || Ok(false),
    );
    assert_eq!(r.map_err(|e| e.to_string()), Err("disco lleno".to_owned()));
}

#[test]
fn each_stem_waveform_shares_one_scale() {
    let mut waveforms = Waveforms::new(vec!["drums".into(), "vocals".into()], 1000, 10);
    let mut loud = vec![0f32; 2 * 2 * 600];
    loud[50] = 0.8; // la batería pega fuerte en la primera columna
    loud[600 + 50] = 0.8;
    loud[3 * 600 + 250] = 0.2; // la voz, flojita, en la tercera
    waveforms.add(&Block {
        sources: 2,
        n: 600,
        data: loud,
    });
    let mut rest = vec![0f32; 2 * 2 * 400];
    rest[399] = -0.4; // en la última, un golpe en negativo
    waveforms.add(&Block {
        sources: 2,
        n: 400,
        data: rest,
    });
    let r = waveforms.result(None);
    let (bat, voice) = (&r[0].peaks, &r[1].peaks);
    assert_eq!(bat[0], 1.0);
    assert_eq!(bat[9], 0.5);
    assert_eq!(bat[1..9].iter().sum::<f64>(), 0.0);
    // la escala es la de la pista más alta: la voz se ve pequeña
    assert_eq!(voice[2], 0.25);
    assert_eq!(voice[0], 0.0);
    assert!(r[0].rms[0] > 0.0 && r[0].rms[0] < bat[0]);
}

#[test]
fn a_source_heard_only_briefly_is_kept() {
    let mix = vec![1.0; 100];
    let mut piano = vec![0.0; 100];
    piano[40..45].fill(0.5); // un 5 % de la canción, a -3 dB de ella
    let n = presence(&piano, &mix);
    assert!(n.db < QUIET_DB + 20.0 && (n.active - 0.05).abs() < 1e-9);
    assert!(keeps(&n));
    assert!(!keeps(&presence(&[1e-4; 100], &mix)));
    assert!(!keeps(&presence(&[0.0; 100], &mix)));
    assert_eq!(
        presence(&[0.0; 100], &[0.0; 100]),
        Level {
            db: -120.0,
            active: 0.0
        }
    );
}

#[test]
fn transposed_like_numpy() {
    let (f, d) = transposed(&[2, 3], &[0.0, 1.0, 2.0, 3.0, 4.0, 5.0]);
    assert_eq!((f, d), (vec![3, 2], vec![0.0, 3.0, 1.0, 4.0, 2.0, 5.0]));
}
