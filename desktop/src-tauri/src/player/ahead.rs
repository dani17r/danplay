//! La cancion, leida por delante a memoria mientras suena.
//!
//! El decodificador de siempre (symphonia) lee el archivo a trozos de 1 a
//! 32 KiB, y lo hace DENTRO del callback de la tarjeta de sonido, que tiene
//! unos 43 ms de colchon. La musica suele estar en un disco duro; si otro
//! proceso lo esta usando (la separacion de pistas, el analisis, una copia,
//! un escaneo), una sola lectura tarda de 0,1 a 1,8 s y el sonido se cortaba
//! ese rato, clic del metronomo incluido.
//!
//! Ahora la cancion empieza a sonar enseguida y un hilo la va leyendo a
//! memoria por bloques; el decodificador lee de esa memoria y el callback no
//! toca el disco. Solo espera (con un `Condvar`, sin dar vueltas) si pide algo
//! que aun no ha llegado: al abrir la cancion, o en un salto muy adelante
//! justo al empezar. El hilo lee primero lo que va por delante de donde lee
//! el decodificador y despues lo de atras, asi que un salto adelante espera a
//! su bloque y no a todo lo de en medio.
//!
//! - Si el archivo falla a mitad (desaparece, se trunca o se reescribe), lo
//!   ya leido sigue sonando; lo que falta se da como el final del archivo, o
//!   como el error que dio el disco. Nada de esperar para siempre.
//! - Reescribir el archivo que suena (las etiquetas del estudio y de las
//!   listas desplazan todo el audio) no toca lo que ya esta en memoria.
//! - Al cambiar de cancion el decodificador se suelta; el hilo lo ve, suelta
//!   la memoria (en su hilo y no en el de la tarjeta, que no tiene por que
//!   pagar eso) y termina.
use std::io::{self, Read, Seek, SeekFrom};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError, Weak};
use std::time::{Duration, Instant};

/// Cuanto se lee de una vez: lo que ntfs-3g (FUSE) atiende en una sola
/// peticion. El primer sonido espera a un bloque; medido en frio en el disco
/// duro de la musica, en reposo, uno asi tarda unos 14 ms (mediana; 20 ms
/// uno del doble, 12 ms uno de 8 KiB como el que pedia el decodificador).
/// Con el disco ocupado, cuanto mas pequeña la peticion antes se cuela.
pub(super) const BLOCK: usize = 128 * 1024;

/// Hasta que tamaño se lee a memoria. Por encima (un WAV de una hora), el
/// camino de siempre: del disco, a trozos.
pub(super) const MAX_LEN: u64 = 512 * 1024 * 1024;

/// Como acabo la lectura.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum End {
    /// Todo leido.
    Whole,
    /// El archivo se acabo antes de lo que media al abrirlo: se trunco o se
    /// reescribio. Lo que falta es el final del archivo.
    Short,
    /// El disco fallo: lo que falta da ese error.
    Failed(io::ErrorKind),
}

/// Lo leido hasta ahora y lo que se necesita saber para seguir.
struct Store {
    /// Un sitio por bloque; `None` mientras no ha llegado.
    blocks: Vec<Option<Box<[u8]>>>,
    /// Cuantos bloques seguidos, desde el primero, han llegado ya.
    prefix: usize,
    /// El bloque donde esta leyendo el decodificador: el hilo sigue desde
    /// ahi, que es lo que va a hacer falta antes.
    cursor: usize,
    /// La lectura termino (bien o mal): no van a llegar mas bloques.
    end: Option<End>,
    /// El decodificador se solto: el hilo suelta la memoria y termina.
    gone: bool,
}

/// Lo que comparten el lector (dentro del decodificador) y el hilo.
struct Shared {
    /// Lo que media el archivo al abrirlo.
    len: u64,
    block: usize,
    store: Mutex<Store>,
    /// Llego un bloque, termino la lectura o se solto el lector.
    changed: Condvar,
}

fn lock(mutex: &Mutex<Store>) -> MutexGuard<'_, Store> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Shared {
    fn wait<'a>(&self, store: MutexGuard<'a, Store>) -> MutexGuard<'a, Store> {
        self.changed.wait(store).unwrap_or_else(PoisonError::into_inner)
    }

    /// Lo que mide de verdad el bloque `index` (el ultimo puede ser mas
    /// corto).
    fn expected(&self, index: usize) -> usize {
        let offset = index as u64 * self.block as u64;
        self.len.saturating_sub(offset).min(self.block as u64) as usize
    }

    /// El siguiente bloque que leer: el primero que falte desde donde lee el
    /// decodificador y, si ya esta todo lo de delante, el primero que falte
    /// desde el principio. `None` si ya no hay que leer nada.
    fn next_block(&self) -> Option<usize> {
        let mut store = lock(&self.store);
        if store.gone || store.end.is_some() {
            return None;
        }
        let total = store.blocks.len();
        let from = store.cursor.min(total);
        let pick = (from..total)
            .chain(0..from)
            .find(|&i| store.blocks.get(i).is_some_and(Option::is_none));
        if pick.is_none() {
            store.end = Some(End::Whole);
            self.changed.notify_all();
        }
        pick
    }

    /// Deja un bloque leido (o el fallo al leerlo). Devuelve si hay que
    /// seguir leyendo.
    fn deliver(&self, index: usize, read: io::Result<Box<[u8]>>) -> bool {
        let expected = self.expected(index);
        let mut store = lock(&self.store);
        if store.gone {
            return false;
        }
        match read {
            Ok(data) => {
                if data.len() < expected {
                    store.end = Some(End::Short);
                }
                if let Some(slot) = store.blocks.get_mut(index) {
                    *slot = Some(data);
                }
                while store.blocks.get(store.prefix).is_some_and(Option::is_some) {
                    store.prefix += 1;
                }
            }
            Err(e) => store.end = Some(End::Failed(e.kind())),
        }
        self.changed.notify_all();
        store.end.is_none()
    }

    /// Espera a que se suelte el lector y libera la memoria aqui.
    fn free_when_released(&self) {
        let blocks = {
            let mut store = lock(&self.store);
            while !store.gone {
                store = self.wait(store);
            }
            std::mem::take(&mut store.blocks)
        };
        drop(blocks);
    }
}

/// Si el hilo se cayera (no deberia), quien espera un bloque no se queda
/// esperando para siempre: la lectura se da por fallida.
struct Ending<'a>(&'a Shared);

impl Drop for Ending<'_> {
    fn drop(&mut self) {
        let mut store = lock(&self.0.store);
        if store.end.is_none() && !store.gone {
            store.end = Some(End::Failed(io::ErrorKind::Other));
        }
        self.0.changed.notify_all();
    }
}

/// Lee `size` bytes desde `offset`. Si el archivo se acaba antes, lo que
/// haya. `at` es donde esta la fuente, para no mover el cabezal sin motivo.
fn read_block<S: Read + Seek>(source: &mut S, at: &mut Option<u64>, offset: u64, size: usize) -> io::Result<Vec<u8>> {
    if *at != Some(offset) {
        *at = None;
        source.seek(SeekFrom::Start(offset))?;
    }
    let mut data = vec![0u8; size];
    let mut filled = 0;
    while filled < size {
        match source.read(&mut data[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    *at = Some(offset + filled as u64);
    data.truncate(filled);
    Ok(data)
}

/// El hilo: lee bloques hasta tenerlos todos (o hasta que falle el archivo
/// o se suelte el lector) y despues espera a que se suelte para liberar la
/// memoria. El archivo se cierra en cuanto deja de hacer falta.
fn load<S: Read + Seek>(shared: &Shared, mut source: S) {
    let ending = Ending(shared);
    let mut at = Some(0);
    while let Some(index) = shared.next_block() {
        let offset = index as u64 * shared.block as u64;
        let read = read_block(&mut source, &mut at, offset, shared.expected(index));
        // fuera del candado: encoger un bloque corto puede mover memoria
        if !shared.deliver(index, read.map(Vec::into_boxed_slice)) {
            break;
        }
    }
    drop(source);
    drop(ending);
    shared.free_when_released();
}

/// Lo que recibe el decodificador: lee de la memoria que va llenando el
/// hilo. `Read + Seek`, con su tamaño y buscable (ver `open::native`).
pub(super) struct Ahead {
    shared: Arc<Shared>,
    pos: u64,
}

impl Ahead {
    /// Empieza a leer `source` (que mide `len`) por delante, en un hilo.
    /// Vuelve enseguida: el primer bloque lo espera quien lo pida.
    pub(super) fn start<S: Read + Seek + Send + 'static>(source: S, len: u64) -> io::Result<Self> {
        Self::start_with(source, len, BLOCK)
    }

    fn start_with<S: Read + Seek + Send + 'static>(source: S, len: u64, block: usize) -> io::Result<Self> {
        let block = block.max(1);
        let count = usize::try_from(len.div_ceil(block as u64))
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "archivo demasiado grande"))?;
        let shared = Arc::new(Shared {
            len,
            block,
            store: Mutex::new(Store {
                blocks: vec![None; count],
                prefix: 0,
                cursor: 0,
                end: None,
                gone: false,
            }),
            changed: Condvar::new(),
        });
        let loader = Arc::clone(&shared);
        std::thread::Builder::new()
            .name("danplay-lectura".into())
            .spawn(move || load(&loader, source))?;
        Ok(Self { shared, pos: 0 })
    }

    /// Para mirar como va la lectura sin retenerla.
    pub(super) fn gauge(&self) -> Gauge {
        Gauge(Arc::downgrade(&self.shared))
    }

    /// Lo que no esta ni va a llegar: el final del archivo, o el error que
    /// dio el disco.
    fn missing(end: Option<End>) -> io::Result<usize> {
        match end {
            Some(End::Failed(kind)) => {
                // `Interrupted` haria que quien lee lo intentara sin fin
                let kind = if kind == io::ErrorKind::Interrupted {
                    io::ErrorKind::Other
                } else {
                    kind
                };
                Err(io::Error::new(kind, "no se pudo leer el archivo del disco"))
            }
            _ => Ok(0),
        }
    }
}

impl Read for Ahead {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let shared = &*self.shared;
        if buf.is_empty() || self.pos >= shared.len {
            return Ok(0);
        }
        let index = (self.pos / shared.block as u64) as usize;
        let within = (self.pos % shared.block as u64) as usize;
        let mut store = lock(&shared.store);
        store.cursor = index;
        loop {
            match store.blocks.get(index) {
                Some(Some(data)) => {
                    // un bloque mas corto de lo que tocaba: el archivo se
                    // acabo antes, y ahi esta su final
                    let Some(rest) = data.get(within..).filter(|rest| !rest.is_empty()) else {
                        return Self::missing(store.end);
                    };
                    let n = rest.len().min(buf.len());
                    buf[..n].copy_from_slice(&rest[..n]);
                    drop(store);
                    self.pos += n as u64;
                    return Ok(n);
                }
                Some(None) if store.end.is_none() => {
                    // aun no ha llegado: el hilo va a por el (es el del
                    // cursor) y avisa al dejarlo
                    store = shared.wait(store);
                }
                _ => return Self::missing(store.end),
            }
        }
    }
}

impl Seek for Ahead {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        let target = match to {
            SeekFrom::Start(n) => Some(n),
            SeekFrom::End(delta) => self.shared.len.checked_add_signed(delta),
            SeekFrom::Current(delta) => self.pos.checked_add_signed(delta),
        };
        let Some(target) = target else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "posicion antes del principio del archivo",
            ));
        };
        self.pos = target;
        Ok(target)
    }
}

impl Drop for Ahead {
    fn drop(&mut self) {
        let mut store = lock(&self.shared.store);
        store.gone = true;
        self.shared.changed.notify_all();
    }
}

/// Como va la lectura de una cancion, visto desde fuera. No la retiene: con
/// el lector suelto, la memoria se libera igual.
#[derive(Clone)]
pub(super) struct Gauge(Weak<Shared>);

impl Gauge {
    /// Espera, como mucho `within`, a que este en memoria la parte del
    /// archivo que lleva hasta la fraccion `share` (de 0 a 1) y un poco mas,
    /// o a que la lectura termine. Para saltar a un punto al reabrir una
    /// cancion: el salto lo hace el callback, y asi no espera al disco.
    /// Devuelve si llego a tiempo.
    pub(super) fn wait_for_share(&self, share: f64, within: Duration) -> bool {
        let Some(shared) = self.0.upgrade() else {
            return true;
        };
        let share = if share.is_finite() { share.clamp(0.0, 1.0) } else { 1.0 };
        // los mp3 de bitrate variable no reparten los bytes por igual: un
        // margen de un 5 % del archivo y un bloque
        let margin = shared.len / 20 + shared.block as u64;
        let target = ((shared.len as f64 * share) as u64)
            .saturating_add(margin)
            .min(shared.len);
        let deadline = Instant::now() + within;
        let mut store = lock(&shared.store);
        loop {
            let loaded = (store.prefix as u64 * shared.block as u64).min(shared.len);
            if loaded >= target || store.end.is_some() || store.gone {
                return true;
            }
            let Some(left) = deadline.checked_duration_since(Instant::now()).filter(|d| !d.is_zero()) else {
                return false;
            };
            store = shared
                .changed
                .wait_timeout(store, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }

    /// Espera, como mucho `within`, a que el archivo este entero en memoria.
    #[cfg(test)]
    pub(super) fn wait_whole(&self, within: Duration) -> bool {
        let Some(shared) = self.0.upgrade() else {
            return false;
        };
        let deadline = Instant::now() + within;
        let mut store = lock(&shared.store);
        while store.end != Some(End::Whole) {
            let Some(left) = deadline.checked_duration_since(Instant::now()).filter(|d| !d.is_zero()) else {
                return false;
            };
            store = shared
                .changed
                .wait_timeout(store, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
        true
    }

    /// El lector se solto y el hilo termino: no queda nada de esta cancion.
    #[cfg(test)]
    pub(super) fn released(&self) -> bool {
        self.0.strong_count() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools;
    use rodio::{Decoder, Source};
    use std::io::Cursor;
    use std::sync::mpsc;

    /// Lo que se espera como mucho a que pase algo que tiene que pasar.
    const PATIENCE: Duration = Duration::from_secs(10);

    /// Unos bytes que no se repiten (cada posicion se sabe cual es).
    fn bytes(n: usize) -> Vec<u8> {
        (0..n).map(|i| (i * 7 + i / 251) as u8).collect()
    }

    /// Una fuente en memoria que apunta lo que le piden y puede ir lenta,
    /// quedarse parada a partir de un sitio o fallar a partir de otro.
    struct Probe {
        data: Cursor<Vec<u8>>,
        /// Los desplazamientos de cada lectura, en orden.
        reads: Arc<Mutex<Vec<u64>>>,
        delay: Duration,
        /// A partir de aqui no da nada hasta que llegue algo por el canal.
        gate: Option<(u64, mpsc::Receiver<()>)>,
        /// A partir de aqui falla.
        broken_from: Option<u64>,
    }

    impl Probe {
        fn new(data: Vec<u8>) -> (Self, Arc<Mutex<Vec<u64>>>) {
            let reads = Arc::new(Mutex::new(Vec::new()));
            let probe = Self {
                data: Cursor::new(data),
                reads: Arc::clone(&reads),
                delay: Duration::ZERO,
                gate: None,
                broken_from: None,
            };
            (probe, reads)
        }
    }

    impl Read for Probe {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let at = self.data.position();
            self.reads.lock().unwrap().push(at);
            if let Some((from, open)) = &self.gate
                && at >= *from
            {
                let _ = open.recv();
            }
            if self.broken_from.is_some_and(|from| at >= from) {
                return Err(io::Error::other("el disco ha fallado"));
            }
            std::thread::sleep(self.delay);
            self.data.read(buf)
        }
    }

    impl Seek for Probe {
        fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
            self.data.seek(to)
        }
    }

    /// Lee del lector hasta el final, de `chunk` en `chunk` bytes.
    fn drain(reader: &mut Ahead, chunk: usize) -> io::Result<Vec<u8>> {
        let mut all = Vec::new();
        let mut buf = vec![0u8; chunk];
        loop {
            match reader.read(&mut buf)? {
                0 => return Ok(all),
                n => all.extend_from_slice(&buf[..n]),
            }
        }
    }

    /// Lo que se lee es lo que hay, se lea como se lea y desde donde se lea.
    #[test]
    fn it_reads_back_exactly_what_the_source_holds() {
        let data = bytes(10 * 1000 + 123);
        let (probe, _) = Probe::new(data.clone());
        let mut reader = Ahead::start_with(probe, data.len() as u64, 1000).unwrap();
        assert_eq!(drain(&mut reader, 777).unwrap(), data);
        // y por saltos
        reader.seek(SeekFrom::Start(4321)).unwrap();
        let mut buf = [0u8; 100];
        reader.read_exact(&mut buf).unwrap();
        assert_eq!(&buf[..], &data[4321..4421]);
        assert_eq!(reader.seek(SeekFrom::End(-23)).unwrap(), data.len() as u64 - 23);
        assert_eq!(drain(&mut reader, 64).unwrap(), &data[data.len() - 23..]);
        assert_eq!(reader.seek(SeekFrom::Current(-50)).unwrap(), data.len() as u64 - 50);
        assert!(reader.seek(SeekFrom::Current(-(data.len() as i64) - 1)).is_err());
        // pasado el final, el final
        reader.seek(SeekFrom::Start(data.len() as u64 + 10)).unwrap();
        assert_eq!(reader.read(&mut buf).unwrap(), 0);
    }

    /// Con los datos ya en memoria, leer no espera a nadie: ni al hilo, que
    /// esta parado a mitad de leer otro bloque.
    #[test]
    fn the_reader_never_waits_for_what_is_already_there() {
        let data = bytes(8 * 1000);
        let (mut probe, _) = Probe::new(data.clone());
        let (open, gate) = mpsc::channel();
        // del tercer bloque en adelante, la fuente se queda colgada
        probe.gate = Some((2000, gate));
        let mut reader = Ahead::start_with(probe, data.len() as u64, 1000).unwrap();
        let gauge = reader.gauge();
        assert!(gauge.wait_for_share(0.0, PATIENCE), "no llego el primer bloque");
        let mut buf = [0u8; 300];
        // espera a los dos primeros bloques (el hilo ya esta colgado en el
        // tercero): de ahi se lee una y otra vez sin esperar
        let deadline = Instant::now() + PATIENCE;
        while lock(&reader.shared.store).prefix < 2 {
            assert!(Instant::now() < deadline, "no llegaron los dos primeros bloques");
            std::thread::sleep(Duration::from_millis(5));
        }
        let started = Instant::now();
        for round in 0..200 {
            reader.seek(SeekFrom::Start((round * 37) % 1700)).unwrap();
            let n = reader.read(&mut buf).unwrap();
            assert!(n > 0);
        }
        assert!(
            started.elapsed() < Duration::from_millis(500),
            "doscientas lecturas de memoria tardaron {:?}",
            started.elapsed()
        );
        // y lo que no ha llegado se espera: llega al abrir la fuente
        let waiting = std::thread::spawn(move || {
            reader.seek(SeekFrom::Start(5000)).unwrap();
            let mut buf = [0u8; 10];
            reader.read_exact(&mut buf).unwrap();
            buf
        });
        std::thread::sleep(Duration::from_millis(100));
        assert!(!waiting.is_finished(), "dio datos que no habian llegado");
        // la fuente atiende una lectura por mensaje
        for _ in 0..64 {
            let _ = open.send(());
        }
        assert_eq!(&waiting.join().unwrap()[..], &data[5000..5010]);
    }

    /// Un salto muy adelante al empezar no espera a que se lea todo lo de en
    /// medio: el hilo va a por ese bloque en cuanto acaba el que tenia entre
    /// manos.
    #[test]
    fn a_jump_ahead_is_read_next() {
        let data = bytes(40 * 1000);
        let (mut probe, reads) = Probe::new(data.clone());
        // lento: aunque esta prueba tarde en llegar a leer, el hilo no se le
        // adelanta mas de un par de bloques
        probe.delay = Duration::from_millis(50);
        let mut reader = Ahead::start_with(probe, data.len() as u64, 1000).unwrap();
        reader.seek(SeekFrom::Start(35_500)).unwrap();
        let mut buf = [0u8; 200];
        reader.read_exact(&mut buf).unwrap();
        assert_eq!(&buf[..], &data[35_500..35_700]);
        let order = reads.lock().unwrap().clone();
        let at = order
            .iter()
            .position(|&o| o == 35_000)
            .expect("nunca leyo el bloque del salto");
        assert!(at <= 3, "leyo {at} bloques antes que el del salto: {order:?}");
        // y despues sigue por delante de donde se lee, luego lo de atras
        drop(reader);
    }

    /// Si el archivo falla a mitad, lo leido se sigue dando entero y lo que
    /// falta da el error del disco, sin quedarse esperando.
    #[test]
    fn a_failing_file_keeps_what_was_read_and_does_not_hang() {
        let data = bytes(6 * 1000);
        let (mut probe, _) = Probe::new(data.clone());
        probe.broken_from = Some(3000);
        let mut reader = Ahead::start_with(probe, data.len() as u64, 1000).unwrap();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut got = vec![0u8; 3000];
            let first = reader.read_exact(&mut got).map(|()| got);
            let mut more = [0u8; 10];
            let after = reader.read(&mut more);
            let _ = tx.send((first, after.map_err(|e| e.kind())));
        });
        let (first, after) = rx.recv_timeout(PATIENCE).expect("se quedo esperando para siempre");
        assert_eq!(first.unwrap(), &data[..3000]);
        assert_eq!(after, Err(io::ErrorKind::Other));
    }

    /// Un archivo que se acorta mientras se lee: lo que hay hasta el corte, y
    /// despues el final.
    #[test]
    fn a_shrunk_file_ends_where_it_was_cut() {
        let data = bytes(2500);
        let (probe, _) = Probe::new(data.clone());
        // dice medir mas de lo que mide (se trunco entre medir y leer)
        let mut reader = Ahead::start_with(probe, 5000, 1000).unwrap();
        assert_eq!(drain(&mut reader, 300).unwrap(), data);
        reader.seek(SeekFrom::Start(4200)).unwrap();
        assert_eq!(reader.read(&mut [0u8; 10]).unwrap(), 0);
    }

    /// Cien canciones seguidas, soltando cada una: ni un hilo ni un byte de
    /// ninguna queda vivo.
    #[test]
    fn a_hundred_songs_in_a_row_leave_nothing_behind() {
        let data = bytes(64 * 1000);
        let gauges: Vec<Gauge> = (0..100)
            .map(|i| {
                let (probe, _) = Probe::new(data.clone());
                let mut reader = Ahead::start_with(probe, data.len() as u64, 4096).unwrap();
                // unas se sueltan sin leer nada, otras a medias, otras enteras
                let mut buf = vec![0u8; 1000 * (i % 3)];
                let _ = reader.read(&mut buf);
                if i % 5 == 0 {
                    assert_eq!(drain(&mut reader, 8192).unwrap().len(), data.len() - buf.len());
                }
                reader.gauge()
            })
            .collect();
        let deadline = Instant::now() + PATIENCE;
        while gauges.iter().any(|g| !g.released()) {
            assert!(
                Instant::now() < deadline,
                "quedan {} lecturas vivas",
                gauges.iter().filter(|g| !g.released()).count()
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// Un mp3 de verdad, hecho con ffmpeg: un barrido de tono, para que cada
    /// instante sea distinto y un salto mal hecho se note.
    fn sweep_mp3(name: &str, seconds: u32) -> std::path::PathBuf {
        let file = std::env::temp_dir().join(format!("{name}-{}.mp3", std::process::id()));
        let ffmpeg = tools::ffmpeg().expect("hace falta ffmpeg para esta prueba");
        let made = tools::command(ffmpeg)
            .args(["-y", "-hide_banner", "-loglevel", "error", "-f", "lavfi"])
            .arg("-i")
            .arg(format!("aevalsrc=0.5*sin(2*PI*(200+20*t)*t):s=44100:d={seconds}"))
            .args(["-ac", "2", "-c:a", "libmp3lame", "-b:a", "128k"])
            .arg(&file)
            .status()
            .is_ok_and(|s| s.success());
        assert!(made, "ffmpeg no pudo hacer el mp3 de la prueba");
        file
    }

    fn decoder(reader: Ahead, len: u64) -> Decoder<Ahead> {
        Decoder::builder()
            .with_data(reader)
            .with_byte_len(len)
            .with_seekable(true)
            .build()
            .expect("el mp3 de la prueba tiene que abrirse")
    }

    /// Un salto adelante nada mas abrir (con el disco lento: el bloque aun
    /// no ha llegado) da las mismas muestras que leyendo del archivo como
    /// siempre.
    #[test]
    fn a_jump_ahead_at_the_start_gives_the_same_samples_as_the_file() {
        let file = sweep_mp3("danplay-prueba-salto-adelante", 40);
        let bytes = std::fs::read(&file).unwrap();
        let len = bytes.len() as u64;
        let mut direct = Decoder::try_from(std::fs::File::open(&file).unwrap()).unwrap();
        direct.try_seek(Duration::from_secs(31)).unwrap();
        let expected: Vec<f32> = direct.take(44_100).collect();

        let (mut probe, _) = Probe::new(bytes);
        // un disco que da 32 KiB cada 20 ms: todo el archivo tardaria mas de
        // un segundo
        probe.delay = Duration::from_millis(20);
        let reader = Ahead::start_with(probe, len, 32 * 1024).unwrap();
        let mut ahead = decoder(reader, len);
        ahead.try_seek(Duration::from_secs(31)).unwrap();
        let got: Vec<f32> = ahead.take(44_100).collect();
        assert_eq!(got.len(), expected.len());
        assert!(expected.iter().any(|s| s.abs() > 0.1), "la prueba tiene que sonar");
        assert!(got == expected, "tras el salto salieron otras muestras");
        let _ = std::fs::remove_file(&file);
    }

    /// El archivo se trunca a cero justo despues de abrirlo (y de leerlo a
    /// memoria): siguen saliendo todas las muestras. Leyendo del archivo como
    /// antes, se cortaba ahi.
    #[test]
    fn truncating_the_file_after_opening_it_loses_nothing() {
        let file = sweep_mp3("danplay-prueba-truncado", 30);
        let whole: Vec<f32> = Decoder::try_from(std::fs::File::open(&file).unwrap())
            .unwrap()
            .collect();
        assert!(whole.len() > 44_100 * 2 * 25, "el mp3 salio corto: {}", whole.len());

        // como antes, del archivo: se corta en cuanto se acaba lo que el
        // decodificador ya tenia en su bufer
        let copy = std::env::temp_dir().join(format!("danplay-prueba-truncado-b-{}.mp3", std::process::id()));
        std::fs::copy(&file, &copy).unwrap();
        let old = Decoder::try_from(std::fs::File::open(&copy).unwrap()).unwrap();
        std::fs::OpenOptions::new()
            .write(true)
            .open(&copy)
            .unwrap()
            .set_len(0)
            .unwrap();
        let old_count = old.count();
        assert!(
            old_count < whole.len() / 2,
            "leyendo del archivo tendria que cortarse: {old_count} de {}",
            whole.len()
        );

        // ahora: de memoria
        let opened = std::fs::File::open(&file).unwrap();
        let len = opened.metadata().unwrap().len();
        let reader = Ahead::start(opened, len).unwrap();
        let gauge = reader.gauge();
        let ahead = decoder(reader, len);
        assert!(gauge.wait_whole(PATIENCE), "no termino de leerlo");
        std::fs::OpenOptions::new()
            .write(true)
            .open(&file)
            .unwrap()
            .set_len(0)
            .unwrap();
        let got: Vec<f32> = ahead.collect();
        assert_eq!(got.len(), whole.len(), "se perdieron muestras al truncar el archivo");
        assert!(got == whole, "salieron otras muestras");
        for f in [file, copy] {
            let _ = std::fs::remove_file(f);
        }
    }

    /// Lo que espera al reabrir una cancion en un punto: llega cuando esta en
    /// memoria lo que hace falta, y si el disco no da, vuelve a su hora.
    #[test]
    fn waiting_for_a_share_of_the_file_is_bounded() {
        let data = bytes(100 * 1000);
        let (mut probe, _) = Probe::new(data.clone());
        let (open, gate) = mpsc::channel();
        probe.gate = Some((50_000, gate));
        let reader = Ahead::start_with(probe, data.len() as u64, 1000).unwrap();
        let gauge = reader.gauge();
        // hasta el 30 % (con su margen) llega
        assert!(gauge.wait_for_share(0.3, PATIENCE));
        // el 90 % no, que el disco esta parado en la mitad: vuelve a su hora
        let started = Instant::now();
        assert!(!gauge.wait_for_share(0.9, Duration::from_millis(150)));
        assert!(started.elapsed() < Duration::from_secs(2));
        drop(open);
        drop(reader);
        // soltado el lector, no hay nada que esperar
        assert!(gauge.wait_for_share(1.0, Duration::from_millis(10)));
    }
}
