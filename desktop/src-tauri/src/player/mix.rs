//! Las pistas separadas de una cancion sonando juntas, cada una con su
//! volumen y su panorama.
//!
//! ffmpeg las lee todas a la vez y las da en un solo flujo, dos canales por
//! pista (`transcode::merge_filter`): asi no se pueden desfasar, y la
//! velocidad, el tono, los saltos y el bucle les pasan a todas por igual,
//! por el mismo camino que a una cancion normal. Aqui se suman a estereo.
//!
//! El volumen de cada pista vive en atomicos (`Gains`): callar la bateria o
//! subir la voz no reabre nada, el mezclador lo lee sobre la marcha. Y no
//! salta de golpe: se acerca al nuevo en unos milisegundos, que un cambio en
//! seco se oye como un clic.
use crate::transcode::Transcoded;
use rodio::source::SeekError;
use rodio::{ChannelCount, SampleRate, Source};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

/// Hasta donde sube una pista: el doble de como viene.
pub const MAX_GAIN: f32 = 2.0;

/// Cada cuantos instantes se vuelven a leer los volumenes: 1,5 ms.
const RELOAD: u32 = 64;

/// Cuanto se acerca el volumen al pedido en cada instante: una constante de
/// tiempo de unos 8 ms. Callar una pista no hace clic y aun asi es inmediato.
const SMOOTH: f32 = 1.0 / (0.008 * 44_100.0);

/// Una pista en el mezclador, tal como la manda la interfaz.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct StemTrack {
    pub path: String,
    /// 0..`MAX_GAIN`: 1 es como viene.
    #[serde(default = "one")]
    pub gain: f32,
    /// -1 (izquierda) .. 1 (derecha).
    #[serde(default)]
    pub pan: f32,
    /// Si suena: la interfaz ya resolvio aqui lo de callar y dejar sola.
    #[serde(default = "yes")]
    pub on: bool,
}

const fn one() -> f32 {
    1.0
}
const fn yes() -> bool {
    true
}

/// Lo que se aplica a cada canal de una pista segun su volumen y su
/// panorama. Es un balance, como el de un equipo de musica: al centro, los
/// dos canales como vienen; hacia un lado, el otro baja hasta callar. Es la
/// misma cuenta que hace el nucleo al guardar la mezcla (`stems.gains`), para
/// que lo guardado suene como lo que se oia.
pub fn balance(gain: f32, pan: f32, on: bool) -> [f32; 2] {
    if !on || !gain.is_finite() || gain <= 0.0 {
        return [0.0, 0.0];
    }
    let gain = gain.min(MAX_GAIN);
    let pan = if pan.is_finite() { pan.clamp(-1.0, 1.0) } else { 0.0 };
    [gain * (1.0 - pan).min(1.0), gain * (1.0 + pan).min(1.0)]
}

/// Los volumenes de las pistas, compartidos entre el hilo de audio (que los
/// cambia) y el mezclador (que los lee): dos por pista, en bits de `f32`.
pub struct Gains {
    values: Vec<[AtomicU32; 2]>,
}

impl Gains {
    pub fn new(tracks: &[StemTrack]) -> Arc<Self> {
        let values = tracks
            .iter()
            .map(|t| {
                let [l, r] = balance(t.gain, t.pan, t.on);
                [AtomicU32::new(l.to_bits()), AtomicU32::new(r.to_bits())]
            })
            .collect();
        Arc::new(Self { values })
    }

    /// Cambia los volumenes. Las pistas son las mismas, en el mismo orden.
    pub fn set(&self, tracks: &[StemTrack]) {
        for (slot, t) in self.values.iter().zip(tracks) {
            let [l, r] = balance(t.gain, t.pan, t.on);
            slot[0].store(l.to_bits(), Ordering::Relaxed);
            slot[1].store(r.to_bits(), Ordering::Relaxed);
        }
    }

    fn get(&self, i: usize) -> [f32; 2] {
        self.values.get(i).map_or([0.0, 0.0], |slot| {
            [
                f32::from_bits(slot[0].load(Ordering::Relaxed)),
                f32::from_bits(slot[1].load(Ordering::Relaxed)),
            ]
        })
    }

    pub fn len(&self) -> usize {
        self.values.len()
    }
}

/// Las pistas de una cancion que estan sonando: de donde se leen y con que
/// volumen.
#[derive(Clone)]
pub struct StemSet {
    pub paths: Vec<PathBuf>,
    pub gains: Arc<Gains>,
    /// Como estaba cada archivo al abrirlo (`stamp`): el separador rehace
    /// las pistas con los mismos nombres (al mejorarlas, o al separar otra
    /// vez), y entonces hay que reabrirlas aunque las rutas no cambien.
    pub stamps: Vec<Option<Stamp>>,
}

/// La fecha de modificacion y el tamaño de un archivo.
pub type Stamp = (std::time::SystemTime, u64);

/// Como esta ahora el archivo, o None si no se puede mirar.
pub fn stamp(path: &std::path::Path) -> Option<Stamp> {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.modified().ok()?, meta.len()))
}

impl StemSet {
    pub fn new(tracks: &[StemTrack]) -> Self {
        let paths: Vec<PathBuf> = tracks.iter().map(|t| PathBuf::from(&t.path)).collect();
        let stamps = paths.iter().map(|p| stamp(p)).collect();
        Self {
            paths,
            gains: Gains::new(tracks),
            stamps,
        }
    }

    /// Son las mismas pistas, en el mismo orden, y nadie las ha rehecho: basta
    /// con cambiar el volumen de cada una.
    pub fn same(&self, tracks: &[StemTrack]) -> bool {
        self.gains.len() == tracks.len()
            && self
                .paths
                .iter()
                .zip(tracks)
                .zip(&self.stamps)
                .all(|((p, t), s)| p.as_os_str() == t.path.as_str() && stamp(p) == *s)
    }
}

/// El flujo de ffmpeg con todas las pistas, sumado a estereo.
pub struct StemMix {
    inner: Transcoded,
    gains: Arc<Gains>,
    tracks: usize,
    /// Lo que se aplica ahora a cada canal, camino del pedido.
    current: Vec<[f32; 2]>,
    target: Vec<[f32; 2]>,
    until_reload: u32,
    /// El canal derecho del instante ya sumado: sale en el siguiente `next`.
    right: Option<f32>,
}

impl StemMix {
    pub fn new(inner: Transcoded, gains: Arc<Gains>) -> Self {
        let tracks = inner.frame() / 2;
        let target: Vec<[f32; 2]> = (0..tracks).map(|i| gains.get(i)).collect();
        Self {
            inner,
            gains,
            tracks,
            current: target.clone(),
            target,
            until_reload: RELOAD,
            right: None,
        }
    }
}

impl Iterator for StemMix {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        if let Some(right) = self.right.take() {
            return Some(right);
        }
        if self.until_reload == 0 {
            for (i, slot) in self.target.iter_mut().enumerate() {
                *slot = self.gains.get(i);
            }
            self.until_reload = RELOAD;
        }
        self.until_reload -= 1;
        let (mut left, mut right) = (0.0f32, 0.0f32);
        for i in 0..self.tracks {
            let l = self.inner.next()?;
            let r = self.inner.next()?;
            let [tl, tr] = self.target[i];
            let gain = &mut self.current[i];
            gain[0] += (tl - gain[0]) * SMOOTH;
            gain[1] += (tr - gain[1]) * SMOOTH;
            left += l * gain[0];
            right += r * gain[1];
        }
        self.right = Some(right);
        Some(left)
    }
}

impl Source for StemMix {
    fn current_span_len(&self) -> Option<usize> {
        // lo de ffmpeg, en instantes, pasado a estereo; y el canal derecho que
        // espera, si lo hay (asi no se parte un instante)
        let pending = usize::from(self.right.is_some());
        let frame = (self.tracks * 2).max(1);
        self.inner.current_span_len().map(|n| n / frame * 2 + pending)
    }
    fn channels(&self) -> ChannelCount {
        rodio::math::nz!(2)
    }
    fn sample_rate(&self) -> SampleRate {
        self.inner.sample_rate()
    }
    fn total_duration(&self) -> Option<Duration> {
        self.inner.total_duration()
    }
    fn try_seek(&mut self, pos: Duration) -> Result<(), SeekError> {
        self.right = None;
        self.inner.try_seek(pos)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools;

    fn track(path: &str, gain: f32, pan: f32, on: bool) -> StemTrack {
        StemTrack {
            path: path.into(),
            gain,
            pan,
            on,
        }
    }

    #[test]
    #[expect(clippy::float_cmp, reason = "son cuentas exactas: 1 por 1, la mitad de 1")]
    fn the_balance_matches_what_the_core_saves() {
        assert_eq!(balance(1.0, 0.0, true), [1.0, 1.0]);
        assert_eq!(balance(0.5, -1.0, true), [0.5, 0.0]);
        let [l, r] = balance(1.2, 0.5, true);
        assert!((l - 0.6).abs() < 1e-6 && (r - 1.2).abs() < 1e-6);
        assert_eq!(balance(1.0, 0.0, false), [0.0, 0.0], "callada");
        assert_eq!(balance(5.0, 0.0, true), [MAX_GAIN, MAX_GAIN], "con tope");
        assert_eq!(balance(f32::NAN, 0.0, true), [0.0, 0.0]);
        assert_eq!(balance(1.0, f32::NAN, true), [1.0, 1.0]);
    }

    /// Una pista de prueba hecha con ffmpeg: lo que diga cada canal (una
    /// expresion de `aevalsrc`, como `0.8*sin(2*PI*440*t)`, o `0`).
    fn stem(name: &str, left: &str, right: &str) -> PathBuf {
        let ffmpeg = tools::ffmpeg().expect("hace falta ffmpeg en el PATH para estas pruebas");
        let file = std::env::temp_dir().join(format!("danplay-pista-{}-{name}.flac", std::process::id()));
        let ok = tools::command(ffmpeg)
            .args(["-y", "-hide_banner", "-loglevel", "error", "-f", "lavfi", "-i"])
            .arg(format!("aevalsrc='{left}|{right}':s=44100:d=2"))
            .args(["-c:a", "flac"])
            .arg(&file)
            .status()
            .is_ok_and(|s| s.success());
        assert!(ok, "ffmpeg no pudo hacer la pista {name}");
        file
    }

    fn open(paths: &[PathBuf], tracks: &[StemTrack]) -> (StemMix, Arc<Gains>) {
        let ffmpeg = tools::ffmpeg().expect("hace falta ffmpeg");
        let (source, _control) = Transcoded::open_inputs(ffmpeg, paths, Some(2.0), 1.0, 0.0, 0.0).unwrap();
        let gains = Gains::new(tracks);
        (StemMix::new(source.patient(), Arc::clone(&gains)), gains)
    }

    fn energy(samples: &[f32]) -> (f32, f32) {
        let (mut l, mut r) = (0.0, 0.0);
        for pair in samples.as_chunks::<2>().0 {
            l += pair[0] * pair[0];
            r += pair[1] * pair[1];
        }
        (l, r)
    }

    /// Dos pistas: una solo por la izquierda, otra solo por la derecha. Cada
    /// una sale por su lado, y callar una la quita sin tocar la otra.
    #[test]
    fn each_stem_sounds_where_it_should_and_can_be_muted_live() {
        let a = stem("izquierda", "0.8*sin(2*PI*440*t)", "0");
        let b = stem("derecha", "0", "0.8*sin(2*PI*660*t)");
        let paths = [a.clone(), b.clone()];
        let tracks = [track("a", 1.0, 0.0, true), track("b", 1.0, 0.0, true)];
        let (mut mix, gains) = open(&paths, &tracks);
        assert_eq!(mix.channels().get(), 2);
        let first: Vec<f32> = mix.by_ref().take(44_100).collect();
        let (l, r) = energy(&first);
        assert!(l > 1000.0 && r > 1000.0, "las dos suenan: {l} {r}");

        // se calla la de la izquierda: en unos milisegundos, fuera
        gains.set(&[track("a", 1.0, 0.0, false), track("b", 1.0, 0.0, true)]);
        let _fade: Vec<f32> = mix.by_ref().take(8_820).collect();
        let after: Vec<f32> = mix.by_ref().take(22_050).collect();
        let (l, r) = energy(&after);
        assert!(l < 1e-3, "la callada sigue sonando: {l}");
        assert!(r > 500.0, "la otra se fue con ella: {r}");
        for file in [a, b] {
            let _ = std::fs::remove_file(file);
        }
    }

    /// El panorama lleva una pista a un lado; el volumen la sube o la baja.
    #[test]
    fn pan_and_gain_move_a_stem() {
        let a = stem("centro", "0.8*sin(2*PI*440*t)", "0.8*sin(2*PI*440*t)");
        let paths = [a.clone()];
        let (mut mix, _) = open(&paths, &[track("a", 0.5, -1.0, true)]);
        let got: Vec<f32> = mix.by_ref().take(44_100).collect();
        let (_, r) = energy(&got);
        assert!(r < 1e-3, "todo a la izquierda y la derecha suena: {r}");
        let peak = got.iter().step_by(2).fold(0f32, |m, v| m.max(v.abs()));
        assert!((0.38..0.42).contains(&peak), "a la mitad de volumen: {peak}");
        let _ = std::fs::remove_file(a);
    }

    /// Todas en la misma muestra: una pista y su contraria (la misma onda con
    /// el signo cambiado) se anulan del todo. Con una sola muestra de desfase
    /// ya no, y se oiria.
    #[test]
    fn the_stems_stay_sample_aligned() {
        let a = stem("fase", "0.8*sin(2*PI*330*t)", "0.8*sin(2*PI*330*t)");
        let b = stem("contrafase", "-0.8*sin(2*PI*330*t)", "-0.8*sin(2*PI*330*t)");
        let paths = [a.clone(), b.clone()];
        let tracks = [track("a", 1.0, 0.0, true), track("b", 1.0, 0.0, true)];
        let (mut mix, _) = open(&paths, &tracks);
        let sum: Vec<f32> = mix.by_ref().take(80_000).collect();
        let worst = sum.iter().fold(0f32, |m, v| m.max(v.abs()));
        assert!(worst < 1e-3, "no se anulan: se han desfasado ({worst})");
        for file in [a, b] {
            let _ = std::fs::remove_file(file);
        }
    }

    /// Las mismas rutas con otro contenido (el separador rehizo las pistas)
    /// no son las mismas pistas: hay que reabrirlas. Cambiar solo el volumen
    /// si lo es.
    #[test]
    fn stems_rewritten_in_place_are_not_the_same() {
        let a = std::env::temp_dir().join(format!("danplay-rehecha-{}.flac", std::process::id()));
        std::fs::write(&a, b"antes").unwrap();
        let path = a.to_string_lossy().into_owned();
        let set = StemSet::new(&[track(&path, 1.0, 0.0, true)]);
        assert!(set.same(&[track(&path, 0.5, 0.3, false)]), "solo cambio la mezcla");
        assert!(!set.same(&[track("/otra/Bateria.flac", 1.0, 0.0, true)]));
        assert!(!set.same(&[]));
        std::fs::write(&a, b"las pistas mejoradas").unwrap();
        assert!(!set.same(&[track(&path, 1.0, 0.0, true)]), "se rehizo y no lo vio");
        let _ = std::fs::remove_file(a);
    }
}
