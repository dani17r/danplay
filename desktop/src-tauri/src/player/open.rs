//! Abrir una cancion: el sink con su fuente, por el decodificador de
//! siempre o por ffmpeg.
use crate::{tools, transcode};
use rodio::mixer::Mixer;
use rodio::{Decoder, Player, Source};
use std::fs::File;
use std::path::Path;
use std::sync::Arc;

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
}

impl Song {
    /// Donde va la cancion, en sus segundos. rodio cuenta en tiempo de
    /// salida, que a otra velocidad no es el mismo (ver `clock_of`).
    pub(super) fn position(&self, clock: f32) -> f64 {
        self.sink.get_pos().as_secs_f64() * f64::from(clock)
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
    /// El tono corrido, en semitonos.
    pub(super) pitch: i32,
    /// La duracion que sabe el indice; solo se usa para los formatos que
    /// pasan por ffmpeg, donde no hay de donde sacarla.
    pub(super) hint: f64,
    /// Desde donde se abre, en segundos de cancion. ffmpeg arranca ya ahi;
    /// al decodificador de siempre se le pide el salto despues.
    pub(super) from: f64,
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
    } = *recipe;
    let sink = Player::connect_new(mixer);
    // antes de darle la fuente: asi no se le escapa ni una muestra de donde
    // no era, y un salto pendiente se atiende sin que se oiga
    sink.pause();
    sink.set_volume(volume);
    let slowed = (speed - 1.0).abs() > 1e-4;
    let pitched = pitch != 0;

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
        });
    }
    sink.set_speed(speed);

    let file = File::open(path).map_err(|e| unreachable_file(path, &e))?;
    // con el tamaño del archivo, symphonia calcula bien la duracion y busca
    // por el indice (lo que hace `try_from`)
    let source = Decoder::try_from(file).map_err(|e| readable(&e, path))?;
    let announced = source.total_duration().map(|d| d.as_secs_f64());
    sink.append(source);
    Ok(Song {
        sink: Arc::new(sink),
        ffmpeg: None,
        tempo: 1.0,
        announced,
    })
}
