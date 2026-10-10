//! Abrir una cancion: el sink con su fuente, por el decodificador de
//! siempre (leyendo el archivo a memoria por delante, ver `ahead`) o por
//! ffmpeg.
use super::ahead::{self, Ahead, Gauge};
use super::mix::{StemMix, StemSet};
use crate::{tools, transcode};
use rodio::mixer::Mixer;
use rodio::{Decoder, Player, Source};
use std::fs::File;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

/// Cuanto se espera, antes de un salto, a tener en memoria lo que hace falta
/// para llegar ahi (ver `Song::preload`). Mas, y una pausa pedida mientras
/// tanto tardaria en notarse; si no llega, el salto espera a lo que falte
/// como antes esperaba al disco.
const PRELOAD_WITHIN: Duration = Duration::from_secs(2);

/// Una cancion abierta: su sink y lo que hace falta saber de su fuente.
pub(super) struct Song {
    /// Compartido para poder pedir los saltos desde un hilo aparte (ver
    /// `output::seek_within`).
    pub(super) sink: Arc<Player>,
    /// El mando de la fuente de ffmpeg, si la cancion pasa por el: con el se
    /// preparan los saltos antes de pedirlos.
    pub(super) ffmpeg: Option<transcode::Control>,
    /// El factor que aplica ffmpeg (1.0 si no pasa por el): rodio cuenta en
    /// tiempo de salida y la cancion va en el suyo.
    pub(super) tempo: f32,
    /// La duracion que anuncia el propio archivo, en segundos de cancion.
    pub(super) announced: Option<f64>,
    /// Lo que va por delante el filtro de velocidad o tono de ffmpeg, en
    /// segundos de cancion (`transcode::filter_lead`): sin el, el clic iba
    /// tarde a 0,5x.
    pub(super) lead: f64,
    /// Como va la lectura a memoria, si la cancion va por el decodificador
    /// de siempre y cabe (ver `ahead`).
    pub(super) reading: Option<Gauge>,
}

impl Song {
    /// Donde va la cancion, en sus segundos. rodio cuenta en tiempo de
    /// salida, que a otra velocidad no es el mismo (ver `clock_of`).
    pub(super) fn position(&self, clock: f32) -> f64 {
        self.sink.get_pos().as_secs_f64() * f64::from(clock) + self.lead
    }

    /// Antes de saltar a `from` (segundos de cancion), al reabrirla o dentro
    /// de la que suena: espera, con tope, a que este en memoria lo que hay
    /// que leer para llegar ahi (el mp3 se recorre trama a trama hasta el
    /// punto). El salto lo hace el callback de la tarjeta, y asi no espera al
    /// disco; mientras, lo que suena sigue sonando. Con la cancion ya leida,
    /// no espera nada. `duration` es la que se sepa (0 si ninguna).
    pub(super) fn preload(&self, from: f64, duration: f64) {
        let Some(reading) = &self.reading else {
            return;
        };
        let total = self.announced.filter(|d| *d > 0.0).unwrap_or(duration);
        let share = if total > 0.0 { from / total } else { 1.0 };
        if !reading.wait_for_share(share, PRELOAD_WITHIN) {
            log::info!("el disco va lento: el salto a {from:.1} s esperara a lo que falte");
        }
    }
    pub(super) fn playing(&self) -> bool {
        !self.sink.is_paused() && !self.sink.empty()
    }
    /// Se acabo: no le queda nada que sonar.
    pub(super) fn exhausted(&self) -> bool {
        self.sink.empty()
    }
}

/// Cuantos segundos de cancion caben en cada segundo del reloj de rodio.
///
/// A velocidad normal, uno. Si la velocidad la pone ffmpeg, el stream ya
/// llega acelerado y el factor es su `tempo`. Si la pone rodio
/// (`set_speed`), el factor es la velocidad igualmente: rodio no remuestrea
/// para acelerar, dice que la fuente va a otra tasa, y cuenta la posicion
/// *despues* de aplicarla, asi que `get_pos` devuelve tiempo de salida y no
/// de cancion. Sin esto, a 0,8x sin ffmpeg la aguja —y con ella el clic— se
/// iba quedando un 20 % atras.
pub(super) fn clock_of(tempo: f32, speed: f32) -> f32 {
    if (tempo - 1.0).abs() > 1e-6 { tempo } else { speed }
}

/// El nombre del archivo, para los mensajes.
fn name_of(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map_or_else(|| path.to_string(), |n| n.to_string_lossy().into_owned())
}

/// No se pudo abrir el archivo: si es que ya no esta, se dice asi.
fn unreachable_file(path: &str, e: &std::io::Error) -> String {
    let name = name_of(path);
    if e.kind() == std::io::ErrorKind::NotFound {
        format!("No encuentro «{name}»: puede que se haya movido o borrado.")
    } else {
        format!("No se pudo abrir «{name}»: {e}")
    }
}

/// Ese formato necesita ffmpeg y no lo hay.
pub(super) fn no_ffmpeg(path: &str) -> String {
    let extension = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("ese")
        .to_lowercase();
    format!("Para reproducir .{extension} hace falta ffmpeg, y no lo encuentro. Instalalo y vuelve a intentarlo.")
}

/// Los errores del decodificador vienen en ingles y no dicen nada al
/// usuario. Se mira que error es y no su texto: el texto cambia de una
/// version de rodio a otra («Unrecognized format» paso a ser «The format of
/// the data has not been recognized.») y dejaba de traducirse.
pub(super) fn readable(error: &rodio::decoder::DecoderError, path: &str) -> String {
    use rodio::decoder::DecoderError;
    let extension = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    let what = if extension.is_empty() {
        "este archivo".to_string()
    } else {
        format!("este .{extension}")
    };
    match error {
        DecoderError::IoError(_) => format!("No se pudo leer {what}: fallo la lectura del disco."),
        // Los formatos que el decodificador no conoce ya no llegan aqui: los
        // manda a ffmpeg `open_song`. Si aun asi cae uno, o el archivo esta
        // roto, se dice en castellano.
        _ => format!("No se pudo leer {what}: puede estar dañado o usar una variante que DanPlay no conoce."),
    }
}

/// Lo que hace falta para abrir una cancion.
pub(super) struct Recipe<'a> {
    pub(super) path: &'a str,
    pub(super) volume: f32,
    pub(super) speed: f32,
    /// El tono corrido, en semitonos (con fracciones).
    pub(super) pitch: f32,
    /// La duracion que sabe el indice; solo se usa para los formatos que
    /// pasan por ffmpeg, donde no hay de donde sacarla.
    pub(super) hint: f64,
    /// Desde donde se abre, en segundos de cancion. ffmpeg arranca ya ahi;
    /// al decodificador de siempre se le pide el salto despues.
    pub(super) from: f64,
    /// Las pistas separadas que suenan en vez del archivo, si las hay.
    pub(super) stems: Option<&'a StemSet>,
}

/// Abre el archivo y deja un sink **en pausa**, listo para sonar: nada se
/// oye hasta que se diga, asi se puede colocar antes donde toque.
///
/// La duracion que anuncia el archivo sale gratis aqui: antes se volvia a
/// abrir y decodificar el archivo solo para medirla.
pub(super) fn open_song(mixer: &Mixer, recipe: &Recipe<'_>) -> Result<Song, String> {
    let Recipe {
        path,
        volume,
        speed,
        pitch,
        hint,
        from,
        stems,
    } = *recipe;
    let sink = Player::connect_new(mixer);
    // antes de darle la fuente: asi no se le escapa ni una muestra de donde
    // no era, y un salto pendiente se atiende sin que se oiga
    sink.pause();
    sink.set_volume(volume);
    let slowed = (speed - 1.0).abs() > 1e-4;
    let pitched = pitch.abs() > transcode::NO_PITCH;

    // Las pistas separadas van siempre por ffmpeg, que las lee todas a la
    // vez y juntas: asi no se desfasan (ver `mix`).
    if let Some(stems) = stems {
        let Some(ffmpeg) = tools::ffmpeg() else {
            return Err("Para oír las pistas separadas hace falta ffmpeg, y no lo encuentro.".into());
        };
        for file in &stems.paths {
            std::fs::metadata(file).map_err(|e| unreachable_file(&file.to_string_lossy(), &e))?;
        }
        let tempo = if slowed { speed } else { 1.0 };
        let (source, control) =
            transcode::Transcoded::open_inputs(ffmpeg, &stems.paths, Some(hint), tempo, pitch, from)?;
        let announced = source
            .total_duration()
            .map(|d| d.as_secs_f64() * f64::from(source.tempo()));
        sink.append(StemMix::new(source, std::sync::Arc::clone(&stems.gains)));
        return Ok(Song {
            sink: Arc::new(sink),
            ffmpeg: Some(control),
            tempo,
            announced,
            lead: transcode::filter_lead(tempo, pitch, transcode::has_rubberband(ffmpeg)),
            reading: None,
        });
    }

    // A otra velocidad, ffmpeg (`atempo`) la cambia SIN mover el tono, que
    // es lo que se quiere para estudiar un trozo: pasa por el cualquier
    // formato. El tono corrido tambien es cosa de ffmpeg. Sin ffmpeg, rodio
    // cambia la velocidad a la antigua (con el tono) y el tono no se toca.
    let by_ffmpeg = transcode::is_handled(path) || ((slowed || pitched) && tools::ffmpeg().is_some());
    if by_ffmpeg {
        let Some(ffmpeg) = tools::ffmpeg() else {
            return Err(no_ffmpeg(path));
        };
        std::fs::metadata(path).map_err(|e| unreachable_file(path, &e))?;
        let tempo = if slowed { speed } else { 1.0 };
        let (source, control) =
            transcode::Transcoded::open_with(ffmpeg, Path::new(path), Some(hint), tempo, pitch, from)?;
        // la duracion anunciada se devuelve en tiempo de la cancion
        let announced = source
            .total_duration()
            .map(|d| d.as_secs_f64() * f64::from(source.tempo()));
        sink.append(source);
        return Ok(Song {
            sink: Arc::new(sink),
            ffmpeg: Some(control),
            tempo,
            announced,
            lead: transcode::filter_lead(tempo, pitch, transcode::has_rubberband(ffmpeg)),
            reading: None,
        });
    }
    sink.set_speed(speed);
    let (announced, reading) = native(&sink, path)?;
    Ok(Song {
        sink: Arc::new(sink),
        ffmpeg: None,
        tempo: 1.0,
        announced,
        lead: 0.0,
        reading,
    })
}

/// El decodificador de siempre para `path`, en el sink. Devuelve la duracion
/// que anuncia el archivo y, si se lee a memoria, como va esa lectura.
///
/// El decodificador lee de la memoria que va llenando un hilo (ver `ahead`):
/// el callback de la tarjeta no toca el disco. Lo que no cabe (mas de
/// `ahead::MAX_LEN`), o si no se puede crear el hilo, como siempre: del
/// archivo. Los errores son los de siempre.
fn native(sink: &Player, path: &str) -> Result<(Option<f64>, Option<Gauge>), String> {
    let file = File::open(path).map_err(|e| unreachable_file(path, &e))?;
    // con el tamaño del archivo, symphonia calcula bien la duracion y busca
    // por el indice (lo que hace `Decoder::try_from` con un `File`)
    let len = file
        .metadata()
        .map_err(|e| readable(&rodio::decoder::DecoderError::IoError(e.to_string()), path))?
        .len();
    if len <= ahead::MAX_LEN {
        match Ahead::start(file, len) {
            Ok(reader) => {
                let gauge = reader.gauge();
                let source = Decoder::builder()
                    .with_data(reader)
                    .with_byte_len(len)
                    .with_seekable(true)
                    .build()
                    .map_err(|e| readable(&e, path))?;
                let announced = source.total_duration().map(|d| d.as_secs_f64());
                sink.append(source);
                return Ok((announced, Some(gauge)));
            }
            Err(e) => log::warn!("no se pudo leer «{}» por delante: {e}", name_of(path)),
        }
    }
    let file = File::open(path).map_err(|e| unreachable_file(path, &e))?;
    let source = Decoder::try_from(file).map_err(|e| readable(&e, path))?;
    let announced = source.total_duration().map(|d| d.as_secs_f64());
    sink.append(source);
    Ok((announced, None))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rodio::mixer::MixerSource;
    use rodio::{ChannelCount, SampleRate};
    use std::time::Instant;

    /// Un mezclador del que tira la prueba, sin tarjeta de sonido.
    fn mixer() -> (Mixer, MixerSource) {
        rodio::mixer::mixer(
            ChannelCount::new(2).expect("dos canales"),
            SampleRate::new(44_100).expect("una tasa de verdad"),
        )
    }

    fn pull(out: &mut MixerSource, samples: usize) {
        for _ in 0..samples {
            let _ = out.next();
        }
    }

    fn recipe(path: &str) -> Recipe<'_> {
        Recipe {
            path,
            volume: 1.0,
            speed: 1.0,
            pitch: 0.0,
            hint: 0.0,
            from: 0.0,
            stems: None,
        }
    }

    /// Un archivo de prueba hecho con ffmpeg, en la carpeta temporal.
    fn made(name: &str, codec: &[&str]) -> String {
        let file = std::env::temp_dir().join(format!("{}-{name}", std::process::id()));
        let ffmpeg = tools::ffmpeg().expect("hace falta ffmpeg para esta prueba");
        let ok = tools::command(ffmpeg)
            .args(["-y", "-hide_banner", "-loglevel", "error", "-f", "lavfi"])
            .args(["-i", "sine=frequency=440:duration=8:sample_rate=44100", "-ac", "2"])
            .args(codec)
            .arg(&file)
            .status()
            .is_ok_and(|s| s.success());
        assert!(ok, "ffmpeg no pudo hacer {name}");
        file.to_string_lossy().into_owned()
    }

    /// Lo que decia antes `open_song` de un archivo que no se puede leer.
    fn as_before(path: &str) -> String {
        match File::open(path) {
            Err(e) => unreachable_file(path, &e),
            Ok(file) => Decoder::try_from(file).map_or_else(|e| readable(&e, path), |_| String::new()),
        }
    }

    /// Un archivo que no esta, que no se puede abrir o que no es audio da el
    /// mismo error que antes de leerlo a memoria.
    #[test]
    fn unreadable_files_give_the_same_errors_as_before() {
        let dir = std::env::temp_dir().join(format!("danplay-prueba-errores-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let junk = dir.join("texto.mp3");
        std::fs::write(&junk, b"no soy audio, soy texto").unwrap();
        let empty = dir.join("vacio.flac");
        std::fs::write(&empty, b"").unwrap();
        let folder = dir.join("carpeta.mp3");
        std::fs::create_dir_all(&folder).unwrap();
        let mut cases = vec![dir.join("no-esta.mp3"), junk.clone(), empty.clone(), folder.clone()];
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let locked = dir.join("cerrado.mp3");
            std::fs::write(&locked, b"ID3").unwrap();
            std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
            cases.push(locked);
        }
        let (mix, _out) = mixer();
        for case in &cases {
            let path = case.to_string_lossy();
            let expected = as_before(&path);
            assert!(!expected.is_empty(), "{path} se podia leer");
            let got = open_song(&mix, &recipe(&path)).err();
            assert_eq!(got.as_deref(), Some(expected.as_str()), "{path}");
        }
        assert!(
            as_before(&cases[0].to_string_lossy()).starts_with("No encuentro «no-esta.mp3»"),
            "el que no esta lo dice asi"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Cien cambios de cancion seguidos: cada una se suelta y su lectura
    /// termina, sin dejar hilos ni memoria.
    #[test]
    fn a_hundred_song_changes_leave_no_reading_behind() {
        let song = made("danplay-prueba-cambios.mp3", &["-c:a", "libmp3lame", "-b:a", "128k"]);
        let (mix, mut out) = mixer();
        let mut gauges = Vec::new();
        for _ in 0..100 {
            let opened = open_song(&mix, &recipe(&song)).expect("la cancion de la prueba se abre");
            assert!(opened.announced.is_some_and(|d| (d - 8.0).abs() < 0.2));
            gauges.push(opened.reading.clone().expect("se lee a memoria"));
            opened.sink.play();
            // suena un poco y se cambia de cancion: el mezclador la suelta
            pull(&mut out, 4_410);
            opened.sink.stop();
            drop(opened);
            pull(&mut out, 4_410);
        }
        let deadline = Instant::now() + Duration::from_secs(10);
        while gauges.iter().any(|g| !g.released()) {
            assert!(
                Instant::now() < deadline,
                "quedan {} lecturas vivas",
                gauges.iter().filter(|g| !g.released()).count()
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        let _ = std::fs::remove_file(&song);
    }
}
