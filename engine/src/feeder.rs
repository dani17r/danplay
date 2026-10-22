//! El hilo que decodifica: todas las partes a la par, en bloques, por delante
//! de lo que suena.
//!
//! Cada bloque lleva un trozo de todas las partes juntas (muestra a muestra:
//! `[muestra][parte][canal]`) y de que muestra de la cancion empieza. Los
//! bloques van y vuelven por dos anillos sin candados: el hilo de audio
//! devuelve los usados y este los llena otra vez. Asi el hilo de audio nunca
//! pide ni suelta memoria, ni espera a nadie.
//!
//! Los saltos y los tramos se resuelven aqui: un salto empieza una
//! *generacion* nueva y el hilo de audio tira los bloques de las anteriores;
//! al llegar al final de un tramo se salta al principio del siguiente (o del
//! primero), con un fundido de 4 ms a cada lado de la costura para que no
//! chasque. Al acabar el ultimo, lo que diga `LoopEnd`: volver al primero,
//! pararse ahi mismo (en la muestra exacta, no un tic despues) o seguir.
use crate::decode::Part;
use crate::queue::LoopEnd;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, TryRecvError};
use std::time::Duration;

/// Muestras por canal de cada bloque (23 ms a 44,1 kHz).
pub const BLOCK: usize = 1024;
/// Cuantos bloques hay: lo que se decodifica por delante (1,5 s).
pub const POOL: usize = 64;
/// El fundido de las costuras de los tramos (4 ms).
pub const FADE: u64 = 176;

pub struct Block {
    pub generation: u64,
    /// Muestra de la cancion de la primera de este bloque.
    pub start: u64,
    pub frames: usize,
    /// `frames * partes * 2`, intercalado.
    pub data: Vec<f32>,
    /// Es el ultimo: detras se acaba la cancion.
    pub last: bool,
}

impl Block {
    pub fn new(parts: usize) -> Block {
        Block {
            generation: 0,
            start: 0,
            frames: 0,
            data: vec![0.0; BLOCK * parts * 2],
            last: false,
        }
    }
}

/// Lo que se le pide al hilo.
pub enum Order {
    /// Ir a esa muestra de la cancion.
    Seek(u64),
    /// Los tramos que se repiten, en muestras `[desde, hasta)`, y que pasa
    /// al acabar el ultimo. Vacio: sin tramos, hasta el final.
    Segments(Vec<(u64, u64)>, LoopEnd),
}

/// Lo que se comparte con el hilo de audio y con quien pregunta como va.
#[derive(Default)]
pub struct Shared {
    /// La generacion buena: los bloques de otra se tiran.
    pub generation: AtomicU64,
    /// A donde fue el ultimo salto (para decir donde va mientras no suena).
    pub target: AtomicU64,
    /// Ordenes ya atendidas (quien las manda cuenta las que mando).
    pub orders_done: AtomicU64,
    /// Lo ultimo que tomo el hilo de audio: de que generacion y que muestra.
    pub consumed_gen: AtomicU64,
    pub consumed_frame: AtomicU64,
    /// Lo escrito acaba porque se acabaron los tramos con `LoopEnd::Stop`,
    /// no la cancion: quien lo vea la deja en pausa al principio del primero.
    pub segments_done: AtomicBool,
}

pub struct Feeder {
    parts: Vec<Part>,
    /// Muestras por canal que dura la cancion.
    len: u64,
    /// La siguiente muestra que se escribe.
    pos: u64,
    /// Desde donde lo escrito va seguido, sin saltos, hasta `pos`.
    contiguous_from: u64,
    generation: u64,
    segments: Vec<(u64, u64)>,
    /// Que pasa al acabar el ultimo tramo.
    end: LoopEnd,
    /// Se acaba de saltar a esa muestra: los primeros `FADE` suben.
    landed: Option<u64>,
    ended: bool,
    scratch: Vec<f32>,
    shared: Arc<Shared>,
}

impl Feeder {
    pub fn new(parts: Vec<Part>, len: u64, shared: Arc<Shared>) -> Feeder {
        let generation = shared.generation.load(Ordering::Acquire);
        Feeder {
            parts,
            len,
            pos: 0,
            contiguous_from: 0,
            generation,
            segments: Vec::new(),
            end: LoopEnd::Repeat,
            landed: None,
            ended: false,
            scratch: vec![0.0; BLOCK * 2],
            shared,
        }
    }

    pub fn parts(&self) -> usize {
        self.parts.len()
    }

    /// Ir a `frame`, con una generacion nueva.
    pub fn seek(&mut self, frame: u64) {
        let frame = frame.min(self.len);
        self.generation = self.shared.generation.fetch_add(1, Ordering::AcqRel) + 1;
        self.shared.target.store(frame, Ordering::Release);
        self.jump(frame);
        self.landed = None;
    }

    fn jump(&mut self, frame: u64) {
        for p in &mut self.parts {
            p.seek(frame);
        }
        self.pos = frame;
        self.contiguous_from = frame;
        self.ended = false;
        self.shared.segments_done.store(false, Ordering::Release);
    }

    /// Tramos nuevos. Como en la PC, no mueven la cancion: un tramo entra
    /// cuando la cancion esta (o llega) dentro. Repitiendose, la cancion no
    /// se acaba (al final vuelve al primero). Si lo que ya hay decodificado
    /// por delante sigue valiendo (va seguido desde lo que suena y no cruza
    /// el final de un tramo que ahora salta o para), se sigue sin cortar; si
    /// no, se vuelve a decodificar desde lo que suena.
    pub fn set_segments(&mut self, list: Vec<(u64, u64)>, end: LoopEnd) {
        let mut list: Vec<(u64, u64)> = list
            .into_iter()
            .map(|(a, b)| (a.min(self.len), b.min(self.len)))
            .filter(|(a, b)| b > a)
            .collect();
        list.sort_unstable();
        let played = if self.shared.consumed_gen.load(Ordering::Acquire) == self.generation {
            self.shared.consumed_frame.load(Ordering::Acquire)
        } else {
            self.shared.target.load(Ordering::Acquire)
        };
        self.segments = list;
        self.end = end;
        let crosses = self
            .segments
            .iter()
            .any(|&(_, b)| self.seam_cuts(b) && played < b && b < self.pos);
        let ahead_ok = self.contiguous_from <= played && played <= self.pos;
        let wraps = self.ended && !self.segments.is_empty() && end == LoopEnd::Repeat;
        if crosses || !ahead_ok || wraps {
            self.seek(played);
        }
    }

    /// Al final del tramo que acaba en `b` la cancion no sigue de largo:
    /// salta a otro o se para.
    fn seam_cuts(&self, b: u64) -> bool {
        self.jump_target(b) != b || self.stops_at(b)
    }

    /// El tramo que acaba en `b` es el ultimo y ahi se para.
    fn stops_at(&self, b: u64) -> bool {
        self.end == LoopEnd::Stop && self.segments.last().is_some_and(|s| s.1 == b)
    }

    fn segment_at(&self, frame: u64) -> Option<(u64, u64)> {
        self.segments
            .iter()
            .copied()
            .find(|&(a, b)| a <= frame && frame < b)
    }

    /// El principio del siguiente tramo despues de `frame`, si hay.
    fn next_segment_start(&self, frame: u64) -> Option<u64> {
        self.segments
            .iter()
            .map(|s| s.0)
            .filter(|&a| a > frame)
            .min()
    }

    /// Ya no hay nada que escribir (el final, sin tramos).
    pub fn ended(&self) -> bool {
        self.ended
    }

    /// Llena el bloque con lo que sigue.
    pub fn fill(&mut self, block: &mut Block) {
        let n_parts = self.parts.len();
        // dentro de un tramo, hasta su final (ahi se salta al siguiente);
        // fuera de ellos, seguido hasta donde empieza el proximo, que entra
        // sin saltar
        let mut seam = None;
        let mut limit = self.len;
        if !self.segments.is_empty() {
            if let Some((_, b)) = self.segment_at(self.pos) {
                seam = Some(b);
                limit = b;
            } else if let Some(a) = self.next_segment_start(self.pos) {
                limit = a;
            }
        }
        let limit = limit.min(self.len);
        let frames = (limit.saturating_sub(self.pos) as usize).min(BLOCK);
        block.generation = self.generation;
        block.start = self.pos;
        block.frames = frames;
        block.last = false;
        let data = &mut block.data[..frames * n_parts * 2];
        data.fill(0.0);
        for (p, part) in self.parts.iter_mut().enumerate() {
            let got = part.read(&mut self.scratch[..frames * 2]);
            for (i, frame) in self.scratch[..got * 2]
                .as_chunks::<2>()
                .0
                .iter()
                .enumerate()
            {
                let at = (i * n_parts + p) * 2;
                data[at] = frame[0];
                data[at + 1] = frame[1];
            }
        }
        // las costuras: baja al acercarse al final de un tramo que salta (o
        // donde se para), y sube al empezar el tramo al que se salto
        let jumps_at = seam.filter(|&b| self.seam_cuts(b));
        if jumps_at.is_some() || self.landed.is_some() {
            for i in 0..frames {
                let f = self.pos + i as u64;
                let mut g = 1.0f32;
                if let Some(b) = jumps_at {
                    g = g.min((b - f) as f32 / FADE as f32);
                }
                if let Some(a) = self.landed {
                    g = g.min((f.saturating_sub(a) + 1) as f32 / FADE as f32);
                }
                if g < 1.0 {
                    for v in &mut data[i * n_parts * 2..(i + 1) * n_parts * 2] {
                        *v *= g;
                    }
                }
            }
        }
        self.pos += frames as u64;
        if self.landed.is_some_and(|a| self.pos >= a + FADE) {
            self.landed = None;
        }
        if let Some(b) = seam.filter(|&b| self.pos >= b) {
            if self.stops_at(b) {
                self.finish(block, true);
                return;
            }
            let next = self.jump_target(b);
            if next != b {
                self.jump(next);
                self.landed = Some(next);
            }
        } else if self.pos >= self.len {
            // repitiendose, la cancion no se acaba: al final, al primero;
            // parandose, se para igual (y vuelve al primero quien lo vea)
            match (self.segments.first(), self.end) {
                (Some(&(a, _)), LoopEnd::Repeat) => {
                    self.jump(a);
                    self.landed = Some(a);
                }
                (Some(_), LoopEnd::Stop) => self.finish(block, true),
                _ => self.finish(block, false),
            }
        }
    }

    /// Ya no hay mas que escribir: el bloque es el ultimo. `segments`: se
    /// acaba porque se acabaron los tramos, no la cancion.
    fn finish(&mut self, block: &mut Block, segments: bool) {
        block.last = true;
        self.ended = true;
        self.shared.segments_done.store(segments, Ordering::Release);
    }

    /// A donde se va al acabar el tramo que termina en `b` (`b` si a
    /// ninguno: sigue de largo, o se para).
    fn jump_target(&self, b: u64) -> u64 {
        let Some(i) = self.segments.iter().position(|s| s.1 == b) else {
            return b;
        };
        if i + 1 < self.segments.len() {
            return self.segments[i + 1].0;
        }
        match self.end {
            LoopEnd::Repeat => self.segments[0].0,
            LoopEnd::Stop | LoopEnd::Continue => b,
        }
    }
}

/// La muestra de la cancion a la que se llega tras sonar `ahead` muestras
/// desde `from`, por el camino que siguen los tramos (como `Feeder::fill`):
/// donde entran las partes de un relevo.
pub fn advance(segments: &[(u64, u64)], end: LoopEnd, from: u64, ahead: u64, len: u64) -> u64 {
    let mut pos = from.min(len);
    let mut left = ahead;
    for _ in 0..1_000 {
        if left == 0 {
            break;
        }
        if let Some(i) = segments.iter().position(|&(a, b)| a <= pos && pos < b) {
            let b = segments[i].1;
            let room = b - pos;
            if left < room {
                return pos + left;
            }
            left -= room;
            pos = if i + 1 < segments.len() {
                segments[i + 1].0
            } else {
                match end {
                    LoopEnd::Repeat => segments[0].0,
                    LoopEnd::Stop => return b,
                    LoopEnd::Continue => b,
                }
            };
        } else {
            let next = segments
                .iter()
                .map(|s| s.0)
                .filter(|&a| a > pos)
                .min()
                .unwrap_or(len);
            let room = next.saturating_sub(pos);
            if left < room || next >= len {
                return (pos + left).min(len);
            }
            left -= room;
            pos = next;
        }
    }
    pos.min(len)
}

/// El hilo: atiende las ordenes y mantiene llenos los bloques. Acaba cuando
/// se suelta quien manda las ordenes.
pub fn run(
    mut feeder: Feeder,
    mut free: rtrb::Consumer<Box<Block>>,
    mut full: rtrb::Producer<Box<Block>>,
    orders: &Receiver<Order>,
) {
    let shared = Arc::clone(&feeder.shared);
    loop {
        let idle = feeder.ended() || free.is_empty() || full.is_full();
        let order = if idle {
            match orders.recv_timeout(Duration::from_millis(4)) {
                Ok(o) => Some(o),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => return,
            }
        } else {
            match orders.try_recv() {
                Ok(o) => Some(o),
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => return,
            }
        };
        if let Some(order) = order {
            match order {
                Order::Seek(frame) => feeder.seek(frame),
                Order::Segments(list, end) => feeder.set_segments(list, end),
            }
            shared.orders_done.fetch_add(1, Ordering::AcqRel);
            continue; // todas las que haya, antes de decodificar nada
        }
        if feeder.ended() || full.is_full() {
            continue;
        }
        if let Ok(mut block) = free.pop() {
            feeder.fill(&mut block);
            // cabe: los dos anillos tienen sitio para todos los bloques
            let _ = full.push(block);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::fixture;

    fn feeder(parts: usize) -> Feeder {
        let (bytes, _) = fixture("ogg");
        let parts = (0..parts)
            .map(|_| Part::open(Arc::clone(&bytes), "ogg").expect("ogg"))
            .collect();
        Feeder::new(parts, 88_200, Arc::new(Shared::default()))
    }

    /// Lo que se escribe, bloque a bloque: (generacion, desde, muestras).
    fn run_blocks(f: &mut Feeder, n: usize) -> Vec<(u64, u64, usize, Vec<f32>)> {
        let mut out = Vec::new();
        let mut b = Block::new(f.parts());
        for _ in 0..n {
            if f.ended() {
                break;
            }
            f.fill(&mut b);
            out.push((
                b.generation,
                b.start,
                b.frames,
                b.data[..b.frames * f.parts() * 2].to_vec(),
            ));
        }
        out
    }

    #[test]
    fn the_parts_go_side_by_side_to_the_end() {
        let mut f = feeder(2);
        let blocks = run_blocks(&mut f, 200);
        let total: usize = blocks.iter().map(|b| b.2).sum();
        assert_eq!(total, 88_200);
        assert!(f.ended());
        // las dos partes son el mismo archivo: cada muestra igual en las dos
        for (_, _, frames, data) in &blocks {
            for i in 0..*frames {
                assert_eq!(data[i * 4].to_bits(), data[i * 4 + 2].to_bits());
                assert_eq!(data[i * 4 + 1].to_bits(), data[i * 4 + 3].to_bits());
            }
        }
        // y seguidos: cada bloque empieza donde acabo el anterior
        for w in blocks.windows(2) {
            assert_eq!(w[1].1, w[0].1 + w[0].2 as u64);
        }
    }

    #[test]
    fn a_seek_starts_a_new_generation_where_it_was_asked() {
        let mut f = feeder(1);
        run_blocks(&mut f, 3);
        f.seek(40_000);
        let b = run_blocks(&mut f, 1);
        assert_eq!((b[0].0, b[0].1), (1, 40_000));
        assert_eq!(f.shared.target.load(Ordering::Acquire), 40_000);
    }

    #[test]
    fn a_segment_repeats_with_a_fade_at_the_seam() {
        let mut f = feeder(1);
        f.set_segments(vec![(10_000, 20_000)], LoopEnd::Repeat);
        // antes del tramo al pedirlo: no salta, llega a el sonando
        let blocks = run_blocks(&mut f, 40);
        assert_eq!(blocks[0].1, 0);
        let entered = blocks.iter().position(|b| b.1 == 10_000).expect("entra");
        assert!(blocks[..entered].iter().all(|b| b.1 + b.2 as u64 <= 10_000));
        let blocks = &blocks[entered..];
        let starts: Vec<u64> = blocks.iter().map(|b| b.1).collect();
        // llega al final del tramo y vuelve a empezar, sin pasarse
        assert!(
            blocks
                .iter()
                .all(|b| b.1 >= 10_000 && b.1 + b.2 as u64 <= 20_000)
        );
        let again = starts
            .iter()
            .skip(1)
            .position(|&s| s == 10_000)
            .expect("vuelve");
        let before = &blocks[again]; // el ultimo antes de volver
        assert_eq!(before.1 + before.2 as u64, 20_000);
        // la ultima muestra antes de la costura, casi en silencio
        let last = before.3[(before.2 - 1) * 2].abs();
        assert!(last < 0.01, "{last}");
    }

    #[test]
    fn segments_in_a_row_play_one_after_the_other() {
        let mut f = feeder(1);
        f.seek(5_000);
        f.set_segments(vec![(5_000, 8_000), (30_000, 32_000)], LoopEnd::Repeat);
        let blocks = run_blocks(&mut f, 12);
        let mut ranges: Vec<(u64, u64)> = Vec::new();
        for b in &blocks {
            match ranges.last_mut() {
                Some(r) if r.1 == b.1 => r.1 += b.2 as u64,
                _ => ranges.push((b.1, b.1 + b.2 as u64)),
            }
        }
        assert_eq!(
            ranges[..3],
            [(5_000, 8_000), (30_000, 32_000), (5_000, 8_000)]
        );
    }

    #[test]
    fn a_new_segment_around_what_plays_does_not_cut() {
        let mut f = feeder(1);
        run_blocks(&mut f, 4); // escrito hasta 4096
        f.shared.consumed_frame.store(1_000, Ordering::Release);
        let generation = f.generation;
        f.set_segments(vec![(0, 60_000)], LoopEnd::Repeat);
        assert_eq!(f.generation, generation, "no hacia falta saltar");
        // uno que acaba dentro de lo ya escrito obliga a decodificar otra vez
        // desde lo que suena (lo escrito pasaba de largo su final)
        f.set_segments(vec![(0, 2_000)], LoopEnd::Repeat);
        assert_eq!(f.generation, generation + 1);
        assert_eq!(f.shared.target.load(Ordering::Acquire), 1_000);
    }

    #[test]
    fn past_the_segments_the_song_comes_back_to_the_first_at_the_end() {
        let mut f = feeder(1);
        f.seek(80_000);
        f.set_segments(vec![(10_000, 20_000)], LoopEnd::Repeat);
        // sigue sonando lo que sonaba, hasta el final, y vuelve al tramo
        let blocks = run_blocks(&mut f, 12);
        assert_eq!(blocks[0].1, 80_000);
        assert!(blocks.iter().any(|b| b.1 + b.2 as u64 == 88_200));
        assert!(blocks.iter().any(|b| b.1 == 10_000));
        assert!(!f.ended());
    }

    /// Los tramos por los que pasa lo escrito, juntando los bloques seguidos.
    fn ranges(blocks: &[(u64, u64, usize, Vec<f32>)]) -> Vec<(u64, u64)> {
        let mut out: Vec<(u64, u64)> = Vec::new();
        for b in blocks {
            match out.last_mut() {
                Some(r) if r.1 == b.1 => r.1 += b.2 as u64,
                _ => out.push((b.1, b.1 + b.2 as u64)),
            }
        }
        out
    }

    #[test]
    fn stopping_plays_each_segment_once_and_stops_on_the_sample() {
        let mut f = feeder(1);
        f.seek(5_000);
        f.set_segments(vec![(5_000, 8_000), (30_000, 32_000)], LoopEnd::Stop);
        let blocks = run_blocks(&mut f, 40);
        // uno detras de otro, una sola vez, y en la ultima muestra del ultimo
        assert_eq!(ranges(&blocks), [(5_000, 8_000), (30_000, 32_000)]);
        assert!(f.ended());
        assert!(f.shared.segments_done.load(Ordering::Acquire));
        // la costura del final, bajando: no chasca al pararse
        let last = blocks.last().expect("bloques");
        assert!(last.3[(last.2 - 1) * 2].abs() < 0.01);
    }

    #[test]
    fn continuing_plays_the_segments_once_and_then_the_rest_of_the_song() {
        let mut f = feeder(1);
        f.seek(5_000);
        f.set_segments(vec![(5_000, 8_000), (30_000, 32_000)], LoopEnd::Continue);
        let blocks = run_blocks(&mut f, 200);
        assert_eq!(ranges(&blocks), [(5_000, 8_000), (30_000, 88_200)]);
        assert!(f.ended(), "la cancion se acaba como siempre");
        assert!(!f.shared.segments_done.load(Ordering::Acquire));
    }

    #[test]
    fn stopping_past_the_segments_stops_at_the_end_of_the_song() {
        let mut f = feeder(1);
        f.seek(80_000);
        f.set_segments(vec![(10_000, 20_000)], LoopEnd::Stop);
        let blocks = run_blocks(&mut f, 12);
        assert_eq!(ranges(&blocks), [(80_000, 88_200)]);
        assert!(f.ended());
        assert!(
            f.shared.segments_done.load(Ordering::Acquire),
            "vuelve al primero quien lo vea"
        );
    }

    #[test]
    fn a_seek_after_stopping_plays_again() {
        let mut f = feeder(1);
        f.seek(5_000);
        f.set_segments(vec![(5_000, 8_000)], LoopEnd::Stop);
        run_blocks(&mut f, 10);
        assert!(f.ended());
        f.seek(5_000);
        assert!(!f.ended());
        assert!(!f.shared.segments_done.load(Ordering::Acquire));
        assert_eq!(ranges(&run_blocks(&mut f, 10)), [(5_000, 8_000)]);
    }

    #[test]
    fn advancing_follows_the_segments() {
        let segs = [(100, 200), (500, 600)];
        // fuera de los tramos, seguido hasta el primero
        assert_eq!(advance(&segs, LoopEnd::Repeat, 0, 50, 1_000), 50);
        // dentro de uno, salta al siguiente
        assert_eq!(advance(&segs, LoopEnd::Repeat, 150, 80, 1_000), 530);
        // del ultimo, al primero
        assert_eq!(advance(&segs, LoopEnd::Repeat, 580, 40, 1_000), 120);
        // o sigue, o se para
        assert_eq!(advance(&segs, LoopEnd::Continue, 580, 40, 1_000), 620);
        assert_eq!(advance(&segs, LoopEnd::Stop, 580, 40, 1_000), 600);
        // sin tramos, hasta el final como mucho
        assert_eq!(advance(&[], LoopEnd::Repeat, 900, 500, 1_000), 1_000);
    }
}
