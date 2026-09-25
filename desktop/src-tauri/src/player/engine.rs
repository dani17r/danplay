//! El bucle del hilo de audio: atiende las ordenes, vigila la salida, lleva
//! el bucle A-B y el metronomo, y cuenta lo que pasa.
//!
//! **La salida se abre al primer play y se suelta tras un rato sin sonar.**
//! Antes se abria al arrancar y cpal trabajaba sin parar —pidiendo silencio
//! al mezclador cientos de veces por segundo— aunque DanPlay llevara horas
//! en la bandeja sin sonar nada. Ahora, si no suena ni la cancion ni el
//! metronomo durante `IDLE_RELEASE`, se suelta; la cancion en pausa se
//! recuerda donde iba y, al volver a darle, se abre ahi.
use super::metro::{self, Click, Metro, Replan};
use super::open::{self, Recipe, Song};
use super::output::{self, Output};
use super::state::{MAX_VOLUME, State, lock, nudged_volume};
use super::{Command, Event, Notify};
use crate::{beats::BeatGrid, tools, transcode};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Lo que se dice cuando no hay por donde sacar el sonido.
pub(super) const NO_OUTPUT: &str = "no hay salida de audio en este equipo";

/// Cuanto puede estar la salida sin pedir muestras antes de darla por
/// muerta. Si el servidor de sonido se cae o el aparato desaparece, cpal
/// deja de pedir y no avisa: la cancion se queda «sonando» sin sonar y sin
/// acabarse nunca. Se rehace la salida y se sigue donde estaba.
const STALL: Duration = Duration::from_secs(4);

/// Cuantas veces seguidas se rehace una salida que no llega a latir antes
/// de rendirse: si el sistema da una que nunca pide muestras, rehacerla cada
/// cuatro segundos para siempre no arregla nada.
const MAX_RECOVERIES: u32 = 3;

/// Cuanto se espera a ver latir la salida antes de pedirle un salto.
const RESPONDS_WITHIN: Duration = Duration::from_millis(500);

/// Cuanto se espera a que el mezclador atienda un salto. Con la salida viva
/// son milisegundos; si no llega, es que murio justo ahora.
const SEEK_WITHIN: Duration = Duration::from_millis(1500);

/// Cuanto tiene que estar todo callado para soltar la salida.
#[cfg(not(test))]
const IDLE_RELEASE: Duration = Duration::from_secs(60);
/// En las pruebas, poco: asi se puede probar que se suelta y se recupera.
#[cfg(test)]
const IDLE_RELEASE: Duration = Duration::from_secs(2);

/// Con cuanta antelacion se prepara la vuelta del bucle A-B cuando la
/// cancion pasa por ffmpeg: asi, al llegar a B, el salto es instantaneo.
const LOOP_AHEAD: f64 = 1.5;

/// El tono corrido: una octava arriba o abajo como mucho, al centesimo de
/// semitono (un cent). Lo que no sea un numero, tal cual.
pub(super) fn clean_pitch(semitones: f32) -> f32 {
    if !semitones.is_finite() {
        return 0.0;
    }
    (semitones.clamp(-12.0, 12.0) * 100.0).round() / 100.0
}

/// Cada cuanto se mira el reloj si no llega ninguna orden. Sonando hace falta
/// a menudo (de ahi sale la barra de progreso); parado no se mueve nada, asi
/// que despertar ocho veces por segundo para ver lo mismo solo gasta bateria.
const ACTIVE: Duration = Duration::from_millis(100);
const IDLE: Duration = Duration::from_millis(500);
/// Cada cuanto se avisa de la posicion mientras suena.
const TICK: Duration = Duration::from_millis(250);

/// Lo que sobrevive a un reinicio del bucle de audio.
///
/// El bucle se cae por cosas que no dependen de nosotros, como un archivo que
/// hace panic al decodificarlo. Se vuelve a empezar con el mismo volumen y la
/// misma velocidad, y sin olvidar que habia una cancion puesta.
pub(super) struct Carry {
    pub(super) volume: f32,
    pub(super) speed: f32,
    pub(super) path: String,
    pub(super) duration: f64,
    /// El factor que aplica ffmpeg al sink abierto (1.0 si no pasa por el):
    /// rodio cuenta en tiempo de salida y la cancion va en el suyo.
    pub(super) tempo: f32,
    /// Los tramos que se repiten, en orden y sin pisarse: uno solo es el
    /// bucle A-B de siempre; con varios, al acabar uno se salta al siguiente
    /// y del ultimo al primero.
    pub(super) loops: Vec<(f64, f64)>,
    /// En que tramo va la cancion, si el bucle esta armado: esta (o ha
    /// entrado) en el, y al pasar de su final salta al siguiente. Un tramo
    /// elegido por detras o por delante de la aguja sin saltar a el (la onda
    /// con candado) no hace nada hasta que la cancion entra en el.
    pub(super) loop_at: Option<usize>,
    /// «Repetir cuando acabe la cancion»: aunque la cancion pase por los
    /// tramos, no se arman; al acabarse, vuelve al primero.
    pub(super) loop_defer: bool,
    /// El tono corrido, en semitonos (con fracciones).
    pub(super) pitch: f32,
    /// El metronomo: sus ajustes y la rejilla de la cancion que suena.
    pub(super) metro: Metro,
    /// Una orden que llego mientras no habia salida de audio. Se atiende en
    /// cuanto la haya, en vez de perderse.
    pub(super) pending: Option<Command>,
    /// Donde iba la cancion cuando se solto la salida (tras un rato en
    /// pausa, o porque murio y no se pudo rehacer). Al volver a sonar se
    /// abre ahi.
    pub(super) resume_at: Option<f64>,
    /// Hay salida de audio (o la habia la ultima vez que se miro).
    pub(super) has_output: bool,
}

impl Carry {
    pub(super) fn new() -> Self {
        Self {
            volume: 0.9,
            speed: 1.0,
            path: String::new(),
            duration: 0.0,
            tempo: 1.0,
            loops: Vec::new(),
            loop_at: None,
            loop_defer: false,
            pitch: 0.0,
            metro: Metro::default(),
            pending: None,
            resume_at: None,
            has_output: false,
        }
    }

    /// El factor de `clock_of` para la cancion que suena ahora.
    fn clock(&self) -> f32 {
        open::clock_of(self.tempo, self.speed)
    }
}

/// Lo que ha pasado en una vuelta del bucle y hay que contar.
#[derive(Default)]
struct Step {
    failure: Option<String>,
    clear_error: bool,
    /// Se pidio un salto: la posicion nueva se avisa aunque este en pausa.
    sought: bool,
    /// La cancion se movio de sitio o de marcha: el clic se reengancha.
    replan: Option<Replan>,
}

struct Engine<'a> {
    shared: &'a Arc<Mutex<State>>,
    notify: &'a Notify,
    carry: &'a mut Carry,
    /// `None` mientras no haga falta, o desde que se solto.
    output: Option<Output>,
    song: Option<Song>,
    /// El clic del metronomo: su propio sink, creado cuando haga falta.
    click: Option<Click>,
    was_finished: bool,
    last_sent: State,
    last_tick: Instant,
    /// Desde cuando no se reengancha el clic a la cancion.
    last_resync: Instant,
    /// Desde cuando no suena nada.
    idle_since: Option<Instant>,
    /// Salidas rehechas seguidas sin que ninguna llegue a latir.
    recoveries: u32,
}

/// El bucle del hilo de audio. Vuelve solo cuando se cierra el canal; si hace
/// panic, `Handle::new` lo vuelve a lanzar.
pub(super) fn run(rx: &Receiver<Command>, shared: &Arc<Mutex<State>>, notify: &Notify, carry: &mut Carry) {
    let mut engine = Engine {
        shared,
        notify,
        carry,
        output: None,
        song: None,
        click: None,
        was_finished: false,
        last_sent: State::default(),
        last_tick: Instant::now(),
        last_resync: Instant::now(),
        idle_since: None,
        recoveries: 0,
    };
    // Se mira si hay salida, pero no se abre: eso se hace al primer play.
    engine.carry.has_output = output::probe();
    lock(shared).has_output = engine.carry.has_output;
    let mut first = Step::default();
    if !engine.carry.has_output {
        first.failure = Some(NO_OUTPUT.into());
    }
    if !engine.publish(first) {
        return;
    }

    loop {
        let next = match engine.take_pending() {
            Some(command) => Ok(command),
            None => match engine.wait() {
                Some(wait) => rx.recv_timeout(wait),
                // nada suena ni puede sonar: se duerme hasta la siguiente orden
                None => rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
            },
        };
        let mut step = Step::default();
        match next {
            Ok(command) => engine.handle(command, &mut step),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return, // el canal se cerro: fin
        }
        engine.watchdog(&mut step);
        engine.ab_loop(&mut step);
        engine.metronome(&mut step);
        engine.release_when_idle();
        if !engine.publish(step) {
            return;
        }
    }
}

impl Engine<'_> {
    fn sounding(&self) -> bool {
        self.song.as_ref().is_some_and(Song::playing)
    }

    /// Cuanto esperar a la siguiente orden; `None` si no hay nada que mirar
    /// mientras tanto.
    fn wait(&self) -> Option<Duration> {
        // una cancion con tramo que se acaba vuelve al tramo: cuanto antes
        let back_to_loop = !self.carry.loops.is_empty() && self.song.as_ref().is_some_and(Song::exhausted);
        if self.sounding() || back_to_loop {
            Some(ACTIVE)
        } else if self.output.is_some() {
            Some(IDLE)
        } else {
            None
        }
    }

    /// La orden que esperaba a la salida, si ya la hay.
    fn take_pending(&mut self) -> Option<Command> {
        if self.output.is_some() {
            self.carry.pending.take()
        } else {
            None
        }
    }

    /// Si la orden necesita la salida abierta para hacer algo.
    fn needs_output(&self, command: &Command) -> bool {
        match command {
            Command::Play { .. } => true,
            Command::Toggle | Command::Resume | Command::Seek(_) => !self.carry.path.is_empty(),
            _ => false,
        }
    }

    /// Abre la salida si no lo esta. Si no la hay, se dice.
    fn ensure_output(&mut self, step: &mut Step) -> bool {
        if self.output.is_some() {
            return true;
        }
        match Output::open() {
            Ok(opened) => {
                self.output = Some(opened);
                self.carry.has_output = true;
                true
            }
            Err(e) => {
                log::warn!("no se pudo abrir la salida de audio: {e}");
                self.carry.has_output = false;
                step.failure = Some(NO_OUTPUT.into());
                false
            }
        }
    }

    fn handle(&mut self, command: Command, step: &mut Step) {
        if self.needs_output(&command) {
            if !self.ensure_output(step) {
                // se atiende en cuanto haya salida; la ultima manda
                self.carry.pending = Some(command);
                return;
            }
            self.carry.pending = None;
        }
        step.clear_error = matches!(command, Command::Play { .. } | Command::Load { .. } | Command::Stop);
        match command {
            Command::Play { path, duration } => self.play(path, duration, step),
            Command::Load { path, duration } => self.load(path, duration),
            Command::Toggle => self.toggle(None, step),
            Command::Resume => self.toggle(Some(true), step),
            Command::Pause => self.toggle(Some(false), step),
            Command::Stop => self.forget_song(),
            Command::Fail(reason) => {
                // La cola no pudo ni localizar el archivo: se para lo que
                // hubiera y se cuenta el motivo. Antes se mandaba un Play con
                // la ruta vacia y el error que salia era «No such file or
                // directory (os error 2)».
                self.forget_song();
                step.failure = Some(reason);
            }
            Command::Seek(seconds) => self.seek(seconds, step),
            Command::Loops { segments, defer } => self.set_loops(segments, defer),
            Command::Metronome { settings, grid } => self.metronome_settings(settings, grid, step),
            Command::Volume(value) => self.volume(value),
            Command::NudgeVolume(delta) => self.volume(nudged_volume(self.carry.volume, delta)),
            Command::Speed(value) => {
                let clock = self.carry.clock();
                self.carry.speed = value.clamp(0.25, 3.0);
                self.retune(clock, step);
            }
            Command::Pitch(semitones) => {
                let clock = self.carry.clock();
                self.carry.pitch = clean_pitch(semitones);
                self.retune(clock, step);
            }
        }
        // una orden que esperaba a la salida tiene otra oportunidad con cada
        // orden nueva: puede que ya se hayan conectado los auriculares
        if self.output.is_none() && self.carry.pending.is_some() {
            let _ = self.ensure_output(step);
        }
    }

    /// Abre la cancion que hay puesta desde `from`, en pausa.
    fn open_current(&mut self, from: f64) -> Result<Song, String> {
        let Some(output) = self.output.as_mut() else {
            return Err(NO_OUTPUT.into());
        };
        let recipe = Recipe {
            path: &self.carry.path,
            volume: self.carry.volume,
            speed: self.carry.speed,
            pitch: self.carry.pitch,
            hint: self.carry.duration,
            from,
        };
        let song = open::open_song(output.mixer(), &recipe)?;
        if from > 0.0 {
            // ffmpeg ya arranco ahi y el salto no cuesta nada; al
            // decodificador de siempre se lo pide ahora, sin que se oiga
            if !output.pulse.responds(RESPONDS_WITHIN) {
                return Err("La salida de audio no responde.".into());
            }
            let clock = open::clock_of(song.tempo, self.carry.speed);
            let pos = Duration::from_secs_f64(from / f64::from(clock.max(0.01)));
            match output::seek_within(&song.sink, pos, SEEK_WITHIN) {
                Some(Ok(())) => {}
                Some(Err(e)) => log::warn!("no se pudo colocar la cancion en {from:.1} s: {e}"),
                None => return Err("La salida de audio no responde.".into()),
            }
        }
        Ok(song)
    }

    /// Pone la cancion abierta como la que suena (y a sonar, si `play`).
    fn adopt(&mut self, song: Song, play: bool) {
        if play {
            song.sink.play();
        }
        self.carry.tempo = song.tempo;
        if let Some(old) = self.song.replace(song) {
            old.sink.stop();
        }
        self.was_finished = false;
    }

    fn play(&mut self, path: String, hint: f64, step: &mut Step) {
        if let Some(old) = self.song.take() {
            old.sink.stop();
        }
        self.carry.path = path;
        self.carry.duration = hint;
        self.carry.resume_at = None;
        match self.open_current(0.0) {
            Ok(song) => {
                // la del indice manda: en mp3 de bitrate variable sin
                // cabecera Xing la del archivo se inventa
                if hint <= 0.0 {
                    self.carry.duration = song.announced.unwrap_or(0.0);
                }
                self.adopt(song, true);
                self.rearm(0.0);
                // otra cancion: su rejilla, o ninguna
                let path = self.carry.path.clone();
                self.carry.metro.rebuild(&path);
                step.replan = Some(Replan::Song);
            }
            Err(e) => {
                // Sin limpiar la ruta, el siguiente play o un salto en la
                // barra reproducian la cancion ANTERIOR, que es de las cosas
                // mas raras que puede hacer un reproductor.
                self.carry.path.clear();
                self.carry.duration = 0.0;
                step.failure = Some(e);
            }
        }
    }

    /// Deja una cancion preparada pero en silencio (ver `Command::Load`).
    fn load(&mut self, path: String, hint: f64) {
        if let Some(old) = self.song.take() {
            old.sink.stop();
        }
        self.carry.path = path;
        self.carry.duration = hint;
        self.carry.resume_at = None;
        self.was_finished = false;
        self.rearm(0.0);
    }

    /// Stop, o la cola que no encontro el archivo: nada puesto.
    fn forget_song(&mut self) {
        if let Some(old) = self.song.take() {
            old.sink.stop();
        }
        self.carry.path.clear();
        self.carry.duration = 0.0;
        self.carry.resume_at = None;
        self.was_finished = false;
    }

    /// Toggle (`None`), Resume (`Some(true)`) o Pause (`Some(false)`).
    fn toggle(&mut self, want: Option<bool>, step: &mut Step) {
        let exhausted = self.song.as_ref().is_none_or(Song::exhausted);
        // Toggle: lo contrario de lo que hay
        let wants_play = want.unwrap_or_else(|| self.song.as_ref().is_none_or(|s| s.sink.is_paused() || s.exhausted()));
        if wants_play && exhausted && !self.carry.path.is_empty() {
            // la pista acabo (o la salida se solto en pausa): se vuelve a
            // abrir, donde se quedo si se sabe, y suena
            let from = self.carry.resume_at.take().unwrap_or(0.0);
            match self.open_current(from) {
                Ok(song) => {
                    self.adopt(song, true);
                    self.rearm(from);
                    step.replan = Some(Replan::Song);
                }
                Err(e) => {
                    self.carry.resume_at = Some(from).filter(|f| *f > 0.0);
                    step.failure = Some(e);
                }
            }
        } else if let Some(song) = &self.song {
            if wants_play {
                song.sink.play();
                step.replan = Some(Replan::Song);
            } else {
                song.sink.pause();
            }
        }
    }

    fn seek(&mut self, seconds: f64, step: &mut Step) {
        step.sought = true;
        step.replan = Some(Replan::Song);
        let seconds = seconds.max(0.0);
        // saltar a proposito dentro de un tramo («reproducir ahora») lo arma
        // ya, aunque esperara al final de la cancion
        if self.segment_at(seconds).is_some() {
            self.carry.loop_defer = false;
        }
        self.rearm(seconds);
        let exhausted = self.song.as_ref().is_none_or(Song::exhausted);
        if exhausted && !self.carry.path.is_empty() {
            // Nada abierto: la pista acabo (se vuelve a abrir y suena, como
            // siempre) o la salida se solto en pausa (se abre donde se pide,
            // pero sigue en pausa: la pausa era de quien escucha).
            let play = self.carry.resume_at.take().is_none();
            match self.open_current(seconds) {
                Ok(song) => self.adopt(song, play),
                Err(e) => step.failure = Some(e),
            }
            return;
        }
        if self.song.is_some() {
            self.seek_song(seconds, step);
        }
    }

    /// El salto dentro de la cancion que suena, sin quedarse colgado nunca.
    fn seek_song(&mut self, target: f64, step: &mut Step) {
        let clock = self.carry.clock();
        let (Some(output), Some(song)) = (self.output.as_mut(), self.song.as_ref()) else {
            return;
        };
        // 1) La salida tiene que estar pidiendo muestras: si no, el salto no
        //    lo atenderia nadie. Se rehace antes.
        if !output.pulse.responds(RESPONDS_WITHIN) {
            self.recover(Some(target), step);
            return;
        }
        // 2) ffmpeg: la tirada nueva, lista antes de pedir el salto.
        if let Some(control) = &song.ffmpeg
            && let Err(e) = control.prepare(target, transcode::READY_WITHIN)
        {
            step.failure = Some(format!("no se puede buscar aqui: {e}"));
            return;
        }
        // 3) El salto, con tope por si la salida muere justo ahora.
        let pos = Duration::from_secs_f64(target / f64::from(clock.max(0.01)));
        match output::seek_within(&song.sink, pos, SEEK_WITHIN) {
            Some(Ok(())) => {}
            Some(Err(e)) => step.failure = Some(format!("no se puede buscar aqui: {e}")),
            None => self.recover(Some(target), step),
        }
    }

    /// La salida no responde: se rehace y la cancion sigue donde iba (o
    /// donde se pidio), sonando o en pausa como estaba.
    fn recover(&mut self, at: Option<f64>, step: &mut Step) {
        let clock = self.carry.clock();
        let resume = self
            .song
            .take()
            .filter(|s| !s.exhausted())
            .map(|s| (at.unwrap_or_else(|| s.position(clock)), s.playing()));
        self.click = None;
        self.output = None;
        match Output::open() {
            Ok(opened) => {
                log::info!("salida de audio rehecha");
                self.output = Some(opened);
                self.carry.has_output = true;
                // la salida es otra: el clic se rehace en ella
                step.replan = Some(Replan::Settings);
                if let Some((from, playing)) = resume.filter(|_| !self.carry.path.is_empty()) {
                    match self.open_current(from) {
                        Ok(song) => self.adopt(song, playing),
                        Err(e) => {
                            self.carry.resume_at = Some(from);
                            step.failure = Some(e);
                        }
                    }
                }
            }
            Err(e) => {
                log::warn!("la salida de audio murio y no se pudo rehacer: {e}");
                self.carry.has_output = false;
                self.carry.resume_at = resume.map(|(from, _)| from);
                step.failure = Some(
                    "La salida de audio dejo de responder y no se pudo recuperar. \
                     Pulsa play para volver a intentarlo."
                        .into(),
                );
            }
        }
    }

    fn metronome_settings(
        &mut self,
        settings: super::MetronomeSettings,
        grid: Option<(String, Arc<BeatGrid>)>,
        step: &mut Step,
    ) {
        let on = settings.on;
        let path = self.carry.path.clone();
        self.carry.metro.set(settings, grid, &path);
        step.replan = Some(Replan::Settings);
        // el clic puede sonar solo, sin cancion: necesita la salida
        if on {
            let _ = self.ensure_output(step);
        }
    }

    fn volume(&mut self, value: f32) {
        self.carry.volume = if value.is_finite() {
            value.clamp(0.0, MAX_VOLUME)
        } else {
            self.carry.volume
        };
        if let Some(song) = &self.song {
            song.sink.set_volume(self.carry.volume);
        }
    }

    /// La velocidad o el tono cambiaron. `clock` es el factor de antes del
    /// cambio: la cancion va donde va segun el sink que suena, no segun el
    /// que se va a abrir (con el nuevo, de 1x a 0,8x la cancion saltaba
    /// hacia atras un 20 %).
    fn retune(&mut self, clock: f32, step: &mut Step) {
        step.replan = Some(Replan::Song);
        let Some(song) = &self.song else {
            return;
        };
        // Con ffmpeg la velocidad (y el tono) se aplican al decodificar: hay
        // que reabrir la cancion donde iba. Sin el, rodio cambia la
        // velocidad al vuelo (y el tono con ella), y el tono corrido no se
        // puede.
        let reopen = tools::ffmpeg().is_some() && !self.carry.path.is_empty();
        if !reopen || song.exhausted() {
            song.sink.set_speed(self.carry.speed);
            return;
        }
        let at = song.position(clock);
        let playing = !song.sink.is_paused();
        match self.open_current(at) {
            Ok(fresh) => self.adopt(fresh, playing),
            Err(e) => step.failure = Some(e),
        }
    }

    // ------------------------------------------- vigilante de la salida
    /// Si la salida lleva STALL sin pedir muestras, ha muerto por debajo: se
    /// rehace y la cancion sigue donde estaba. Se mira siempre, suene o no:
    /// en pausa tambien se puede desenchufar el aparato.
    fn watchdog(&mut self, step: &mut Step) {
        let Some(output) = self.output.as_mut() else {
            return;
        };
        let silent = output.pulse.silent_for();
        if silent < STALL && !output.lost() {
            if silent < Duration::from_secs(1) {
                self.recoveries = 0;
            }
            return;
        }
        self.recoveries += 1;
        if self.recoveries > MAX_RECOVERIES {
            // Ninguna de las salidas que da el sistema llega a sonar: se
            // suelta y se dice. Play lo vuelve a intentar desde cero.
            log::warn!("la salida de audio no llega a sonar tras {MAX_RECOVERIES} intentos: se deja");
            let clock = self.carry.clock();
            if let Some(song) = self.song.take()
                && !song.exhausted()
            {
                self.carry.resume_at = Some(song.position(clock));
            }
            self.click = None;
            self.output = None;
            self.recoveries = 0;
            self.carry.has_output = false;
            step.failure = Some(NO_OUTPUT.into());
            return;
        }
        log::warn!("la salida de audio ha dejado de pedir muestras: se rehace");
        self.recover(None, step);
    }

    // ------------------------------------------- bucle A-B
    /// Donde va la cancion ahora (o donde se quedo), en sus segundos.
    fn position_now(&self) -> f64 {
        let clock = self.carry.clock();
        self.song
            .as_ref()
            .filter(|s| !s.exhausted())
            .map(|s| s.position(clock))
            .or(self.carry.resume_at)
            .unwrap_or(0.0)
    }

    /// En que tramo cae `at`, si cae en alguno.
    fn segment_at(&self, at: f64) -> Option<usize> {
        self.carry.loops.iter().position(|&(a, b)| at >= a - 0.01 && at < b)
    }

    /// Arma el bucle si la cancion esta dentro de un tramo en `at` (y no
    /// espera al final de la cancion).
    fn rearm(&mut self, at: f64) {
        self.carry.loop_at = if self.carry.loop_defer {
            None
        } else {
            self.segment_at(at)
        };
    }

    /// Tramos nuevos: en orden, y los que se pisan, juntos.
    fn set_loops(&mut self, segments: Vec<(f64, f64)>, defer: bool) {
        let mut clean: Vec<(f64, f64)> = segments
            .into_iter()
            .filter(|&(a, b)| a.is_finite() && b.is_finite() && a >= 0.0 && b > a + 0.2)
            .take(64)
            .collect();
        clean.sort_by(|x, y| x.0.total_cmp(&y.0));
        let mut merged: Vec<(f64, f64)> = Vec::with_capacity(clean.len());
        for (a, b) in clean {
            match merged.last_mut() {
                Some(last) if a <= last.1 => last.1 = last.1.max(b),
                _ => merged.push((a, b)),
            }
        }
        self.carry.loops = merged;
        self.carry.loop_defer = defer && !self.carry.loops.is_empty();
        self.rearm(self.position_now());
    }

    /// Al pasar del final del tramo en que va, la cancion salta al principio
    /// del siguiente (con uno solo, vuelve a A: el bucle A-B). Sirve para
    /// machacar un trozo, o varios seguidos saltandose lo de entre medias.
    ///
    /// Solo con el bucle armado: la cancion tiene que haber estado dentro de
    /// un tramo. Uno elegido mientras suena sin saltar a el (la onda con
    /// candado) espera: si va por delante, entra al llegar la cancion; si va
    /// por detras, la cancion sigue hasta el final y entonces vuelve al
    /// primero, en vez de pasar a la siguiente. Con `loop_defer` pasa lo
    /// mismo aunque la cancion atraviese los tramos.
    fn ab_loop(&mut self, step: &mut Step) {
        if self.carry.loops.is_empty() {
            return;
        }
        let Some(song) = &self.song else {
            return;
        };
        if song.exhausted() {
            let first = self.carry.loops[0].0;
            self.carry.loop_defer = false;
            self.seek(first, step);
            if step.failure.is_some() {
                // no se pudo volver a abrir: se deja acabar, sin insistir
                self.carry.loops.clear();
                self.carry.loop_at = None;
            }
            return;
        }
        if !song.playing() {
            return;
        }
        let position = song.position(self.carry.clock());
        let at = match self.carry.loop_at {
            Some(i) => i,
            None if self.carry.loop_defer => return,
            None => match self.segment_at(position) {
                Some(i) => {
                    self.carry.loop_at = Some(i);
                    i
                }
                None => return,
            },
        };
        let Some(&(_, end)) = self.carry.loops.get(at) else {
            self.carry.loop_at = None;
            return;
        };
        let next = (at + 1) % self.carry.loops.len();
        let start = self.carry.loops[next].0;
        if let Some(control) = &song.ffmpeg
            && end - position < LOOP_AHEAD
        {
            control.prepare_soon(start);
        }
        if position >= end {
            self.seek_song(start, step);
            self.carry.loop_at = Some(next);
            step.replan = Some(Replan::Song);
        }
    }

    // ------------------------------------------- metronomo
    /// Se planifica DESPUES de mover la cancion, con su posicion de ahora.
    /// Y aunque no se haya movido nada, cada RESYNC se reengancha mientras
    /// suena, para que ningun desvio se vaya acumulando.
    fn metronome(&mut self, step: &mut Step) {
        if step.replan.is_none()
            && self.sounding()
            && self.carry.metro.follows()
            && self.last_resync.elapsed() >= metro::RESYNC
        {
            step.replan = Some(Replan::Resync);
        }
        let Some(why) = step.replan else {
            return;
        };
        self.last_resync = Instant::now();
        if !self.carry.metro.settings.on && why != Replan::Settings {
            return;
        }
        let Some(output) = &self.output else {
            return;
        };
        if let Some(shared) = metro::ensure_click(output, &mut self.click) {
            let position = self.song.as_ref().map_or(0.0, |s| s.position(self.carry.clock()));
            let playing = self.sounding();
            metro::plan(&self.carry.metro, self.carry.speed, position, playing, &shared, why);
        }
    }

    // ------------------------------------------- soltar la salida
    fn release_when_idle(&mut self) {
        let busy = self.sounding() || self.carry.metro.settings.on;
        if busy || self.output.is_none() {
            self.idle_since = None;
            return;
        }
        let since = *self.idle_since.get_or_insert_with(Instant::now);
        if since.elapsed() < IDLE_RELEASE {
            return;
        }
        let clock = self.carry.clock();
        if let Some(song) = self.song.take() {
            // la que acabo no se recuerda: play la empieza otra vez
            if !song.exhausted() {
                self.carry.resume_at = Some(song.position(clock));
            }
            song.sink.stop();
        }
        self.click = None;
        self.output = None;
        self.idle_since = None;
        log::info!(
            "salida de audio suelta: nada suena desde hace {} s",
            IDLE_RELEASE.as_secs()
        );
    }

    // ------------------------------------------- estado nuevo
    /// Cuenta lo que haya cambiado. Devuelve false si ya no escucha nadie.
    fn publish(&mut self, step: Step) -> bool {
        let clock = self.carry.clock();
        let finished = self.song.as_ref().is_some_and(Song::exhausted);
        let current = {
            let mut e = lock(self.shared);
            if let Some(reason) = step.failure {
                e.error = reason;
            } else if step.clear_error || (self.carry.has_output && e.error == NO_OUTPUT) {
                e.error.clear();
            }
            if let Some(song) = &self.song {
                e.playing = song.playing();
                e.position = song.position(clock);
            } else {
                e.playing = false;
                e.position = self.carry.resume_at.unwrap_or(0.0);
            }
            e.has_output = self.carry.has_output;
            e.path.clone_from(&self.carry.path);
            e.duration = self.carry.duration;
            e.volume = self.carry.volume;
            e.speed = self.carry.speed;
            e.pitch_preserved = (self.carry.speed - 1.0).abs() < 1e-4 || (self.carry.tempo - 1.0).abs() > 1e-6;
            let loops = &self.carry.loops;
            e.loop_a = loops.first().map_or(0.0, |s| s.0);
            e.loop_b = loops.last().map_or(0.0, |s| s.1);
            e.loops = loops.iter().map(|&(a, b)| [a, b]).collect();
            e.loop_defer = self.carry.loop_defer;
            e.pitch = self.carry.pitch;
            e.metronome = self.carry.metro.state();
            e.clone()
        };

        // Fin de pista: se avisa UNA vez. Quien decide que pasa luego
        // (repetir, avanzar, pararse) es la cola. Con un tramo puesto no se
        // acaba: vuelve a A (ver `ab_loop`).
        if finished && !self.was_finished && !self.carry.path.is_empty() && self.carry.loops.is_empty() {
            self.was_finished = true;
            if !(self.notify)(Event::Finished) {
                return false;
            }
        }
        if !finished {
            self.was_finished = false;
        }

        // Se avisa cuando cambia algo que se ve, y mientras suena tambien
        // cada 250 ms para mover la barra de progreso. Y tras un salto
        // siempre: en pausa no hay tick, y la barra se quedaba donde estaba
        // aunque hubieras pinchado en otro minuto.
        let changed = step.sought || current.differs_from(&self.last_sent);
        let due = current.playing && self.last_tick.elapsed() >= TICK;
        if changed || due {
            self.last_sent = current.clone();
            self.last_tick = Instant::now();
            if !(self.notify)(Event::Changed(current)) {
                return false;
            }
        }
        true
    }
}
