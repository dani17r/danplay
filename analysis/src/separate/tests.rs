//! Lo de separar que no necesita ffmpeg, con una red de mentira que devuelve
//! lo que le entra.
use super::*;

/// Estéreo con varios tonos: sin nada en la frecuencia de Nyquist, que el
/// espectrograma de Demucs tira.
pub(super) fn tones(length: usize) -> Vec<f32> {
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
fn padding_mirrors_without_repeating_the_edge() {
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
pub(super) struct Identity {
    pub(super) parts: Vec<f32>,
    pub(super) calls: usize,
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
fn transposed_reverses_the_axes() {
    let (f, d) = transposed(&[2, 3], &[0.0, 1.0, 2.0, 3.0, 4.0, 5.0]);
    assert_eq!((f, d), (vec![3, 2], vec![0.0, 3.0, 1.0, 4.0, 2.0, 5.0]));
}

/// La mezcla se normaliza antes de la red (media y desviación, como
/// Demucs) y se deshace después: una señal alta y descentrada vuelve igual.
#[test]
fn a_song_with_an_offset_and_a_loud_mix_comes_back_the_same() {
    let length = 9 * RATE;
    let x: Vec<f32> = tones(length).iter().map(|v| v * 2.5 + 0.05).collect();
    let mut net = Identity {
        parts: vec![1.0],
        calls: 0,
    };
    let mut blocks = Vec::new();
    separate(
        &mut net,
        x.clone(),
        &mut |b| {
            blocks.push(b);
            Ok(())
        },
        &mut |_, _| Ok(()),
        &mut || Ok(false),
    )
    .expect("separa");
    let (s, rows) = join_blocks(&blocks);
    assert_eq!(s, 1);
    for (c, row) in rows.iter().enumerate() {
        assert_eq!(row.len(), length);
        for (i, got) in row.iter().enumerate() {
            let want = x[c * length + i];
            assert!((got - want).abs() < 1e-4, "{c} {i}: {got} {want}");
        }
    }
}

/// Un `.safetensors` a mano: el largo de la cabecera, la cabecera en JSON y
/// los números detrás. Cada tensor: (nombre, tipo, forma, bytes).
pub(super) fn write_safetensors(path: &Path, tensors: &[(&str, &str, Vec<usize>, Vec<u8>)]) {
    let mut header = serde_json::Map::new();
    let mut at = 0usize;
    for (name, kind, shape, raw) in tensors {
        header.insert(
            (*name).to_owned(),
            serde_json::json!({"dtype": kind, "shape": shape, "data_offsets": [at, at + raw.len()]}),
        );
        at += raw.len();
    }
    let head = serde_json::to_vec(&serde_json::Value::Object(header)).expect("cabecera");
    let mut out = (head.len() as u64).to_le_bytes().to_vec();
    out.extend(head);
    for (_, _, _, raw) in tensors {
        out.extend(raw);
    }
    std::fs::write(path, out).expect("escribir");
}

fn f16_bytes(values: impl IntoIterator<Item = f32>) -> Vec<u8> {
    values
        .into_iter()
        .flat_map(|v| half::f16::from_f32(v).to_le_bytes())
        .collect()
}

/// Los pesos del separador se leen sin la biblioteca de safetensors; lo que
/// no lo es, no se lee.
#[test]
fn safetensors_are_read_without_the_library() {
    let dir = crate::testkit::Scratch::new("safetensors");
    let path = dir.join("w.safetensors");
    let a = f16_bytes((0..6u8).map(f32::from));
    let b: Vec<u8> = [1.5f32, -2.0]
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .collect();
    write_safetensors(
        &path,
        &[("a", "F16", vec![2, 3], a), ("b", "F32", vec![2], b)],
    );
    let got = weights::safetensors(&path).expect("se lee");
    let mut file = File::open(&path).expect("abrir");
    let values: HashMap<String, (Vec<usize>, Vec<f32>)> = got
        .iter()
        .map(|t| {
            let v = weights::materialize_st(&mut file, t).expect("los números");
            (t.name.clone(), v)
        })
        .collect();
    assert_eq!(
        values["a"],
        (vec![2, 3], vec![0.0, 1.0, 2.0, 3.0, 4.0, 5.0])
    );
    assert_eq!(values["b"], (vec![2], vec![1.5, -2.0]));
    let broken = dir.join("roto.safetensors");
    std::fs::write(&broken, [0xff; 16]).expect("escribir");
    assert!(weights::safetensors(&broken).is_err());
}

/// El grafo de verdad se carga con pesos inventados con los nombres y formas
/// del archivo del autor: ONNX Runtime comprueba al cargar que cada peso casa
/// en nombre, tipo y forma. Así se sabe que la app sabe rellenar cada peso
/// sin bajarse los 55 MB (ni los 84 del de cuatro pistas).
#[test]
fn the_real_graph_loads_with_weights_laid_out_like_the_official_ones() {
    let Some(runtime) = crate::testkit::onnx_runtime() else {
        println!("sin ONNX Runtime: el grafo de verdad no se comprueba");
        return;
    };
    let folder = crate::testkit::bundled_data("separator");
    let dir = crate::testkit::Scratch::new("grafo");
    for graph in ["htdemucs_6s", "htdemucs"] {
        let text =
            std::fs::read_to_string(folder.join(format!("{graph}.json"))).expect("manifiesto");
        let manifest = Manifest::de_json(&text).expect("manifiesto");
        let raw: serde_json::Value = serde_json::from_str(&text).expect("manifiesto");
        let mut seen = std::collections::HashSet::new();
        let mut tensors = Vec::new();
        for item in raw["initializers"].as_array().expect("pesos") {
            let source = item["source"].as_str().expect("fuente");
            if !seen.insert(source) {
                continue;
            }
            let shape: Vec<usize> = item["shape"]
                .as_array()
                .expect("forma")
                .iter()
                .map(|d| usize::try_from(d.as_u64().expect("dimensión")).expect("dimensión"))
                .collect();
            let n: usize = shape.iter().product();
            // pequeños y distintos
            let values = (0..n).map(|i| ((i * 7919) % 2001) as f32 / 100_000.0 - 0.01);
            tensors.push((source, "F16", shape, f16_bytes(values)));
        }
        let weights = dir.join(raw["weights"]["file"].as_str().expect("archivo"));
        write_safetensors(&weights, &tensors);
        drop(tensors);
        let net = load_net(
            &runtime,
            &folder.join(format!("{graph}.onnx")),
            &manifest,
            &weights,
            Some(2),
        )
        .unwrap_or_else(|e| panic!("{graph}: {e}"));
        std::fs::remove_file(&weights).expect("borrar los pesos");
        let fixed = |d: &[usize]| -> Vec<onnx::Dim> {
            d.iter()
                .map(|n| onnx::Dim::Fixed(i64::try_from(*n).expect("cabe")))
                .collect()
        };
        let inputs: HashMap<String, Vec<onnx::Dim>> = net.inputs().into_iter().collect();
        let outputs: HashMap<String, Vec<onnx::Dim>> = net.outputs().into_iter().collect();
        let n = manifest.sources.len();
        assert_eq!(
            inputs,
            HashMap::from([
                ("mix".to_owned(), fixed(&[1, 2, SEGMENT])),
                ("mag".to_owned(), fixed(&[1, 4, FREQS, 336])),
            ]),
            "{graph}"
        );
        assert_eq!(
            outputs,
            HashMap::from([
                ("spec".to_owned(), fixed(&[1, n, 4, FREQS, 336])),
                ("wave".to_owned(), fixed(&[1, n, 2, SEGMENT])),
            ]),
            "{graph}"
        );
    }
}
