//! La salida de audio: abrirla, saber si sigue viva y buscar sin colgarse.
//!
//! Cuando la salida muere (se desenchufa el DAC USB o la HDMI, se cae el
//! servidor de sonido), cpal deja de pedir muestras sin avisar. Eso tenia dos
//! consecuencias. Sonando, la cancion se quedaba «sonando» quieta; de eso ya
//! se encargaba un vigilante. Pero en pausa, o si se pedia un salto antes de
//! que el vigilante saltara, era peor: `Player::try_seek` deja la orden y
//! espera, sin tope, a que el mezclador la atienda, y el mezclador ya no iba
//! a pedir nada nunca mas. El hilo de audio se quedaba colgado para siempre
//! y DanPlay dejaba de sonar hasta reiniciarlo.
//!
//! Ahora hay un **latido**: una fuente muda en el mezclador que cuenta cada
//! vez que la salida le pide muestras. Asi se sabe si la salida esta viva
//! sin preguntarle a ella, este sonando la cancion o no. Antes de buscar se
//! mira el latido; y el salto en si se pide desde un hilo desechable con un
//! tope, por si la salida muriera justo entre mirar y pedir. Y cuando cpal
//! si avisa (el aparato ya no esta), no hace falta esperar al latido.
use rodio::cpal::traits::{DeviceTrait, HostTrait};
use rodio::mixer::Mixer;
use rodio::{ChannelCount, DeviceSinkBuilder, MixerDeviceSink, Player, SampleRate, Source};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Cada cuantas muestras late la fuente muda: unos 5 ms.
const BEAT_EVERY: u64 = 256;

/// Si el latido se movio hace menos que esto, la salida esta viva sin mas
/// comprobaciones. Es generoso: PipeWire, para ahorrar, llega a pedir las
/// muestras en tandas de 170 ms.
const FRESH: Duration = Duration::from_millis(250);

/// Si hay por donde sacar el sonido, sin abrir nada que se quede abierto.
pub(super) fn probe() -> bool {
    rodio::cpal::default_host()
        .default_output_device()
        .and_then(|d| d.default_output_config().ok())
        .is_some()
}

/// El contador del latido y cuando se le vio moverse por ultima vez.
pub(super) struct Pulse {
    beat: Arc<AtomicU64>,
    seen: u64,
    seen_at: Instant,
}

impl Pulse {
    pub(super) fn new(beat: Arc<AtomicU64>) -> Self {
        let seen = beat.load(Ordering::Relaxed);
        Self {
            beat,
            seen,
            seen_at: Instant::now(),
        }
    }

    /// Mira el contador; devuelve si se ha movido desde la ultima vez.
    fn observe(&mut self) -> bool {
        let now = self.beat.load(Ordering::Relaxed);
        if now == self.seen {
            return false;
        }
        self.seen = now;
        self.seen_at = Instant::now();
        true
    }

    /// Cuanto lleva la salida sin pedir muestras.
    pub(super) fn silent_for(&mut self) -> Duration {
        self.observe();
        self.seen_at.elapsed()
    }

    /// Si la salida esta pidiendo muestras. Si hace un rato que no se la ve
    /// latir, se espera como mucho `within` a que lo haga.
    pub(super) fn responds(&mut self, within: Duration) -> bool {
        if self.observe() || self.seen_at.elapsed() < FRESH {
            return true;
        }
        let started = Instant::now();
        while started.elapsed() < within {
            std::thread::sleep(Duration::from_millis(2));
            if self.observe() {
                return true;
            }
        }
        false
    }
}

/// La fuente muda del latido. Sale por el mezclador como cualquier otra,
/// asi que solo cuenta si de verdad se estan pidiendo muestras.
struct Heartbeat {
    beat: Arc<AtomicU64>,
    rate: SampleRate,
    count: u64,
}

impl Iterator for Heartbeat {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        self.count += 1;
        if self.count.is_multiple_of(BEAT_EVERY) {
            self.beat.fetch_add(1, Ordering::Relaxed);
        }
        Some(0.0)
    }
}

impl Source for Heartbeat {
    fn current_span_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> ChannelCount {
        ChannelCount::MIN
    }
    fn sample_rate(&self) -> SampleRate {
        self.rate
    }
    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

/// La salida abierta, con su latido.
pub(super) struct Output {
    /// Mientras viva esto, suena: dentro van el flujo de cpal y el mezclador.
    sink: MixerDeviceSink,
    /// A la que abrio: el clic se genera a esta para no pasar por el
    /// conversor de tasa del mezclador.
    pub(super) rate: u32,
    pub(super) pulse: Pulse,
    /// cpal aviso de que la salida ya no vale (se fue el aparato, cambio la
    /// configuracion): se rehace sin esperar a que el latido se pare.
    lost: Arc<AtomicBool>,
}

impl Output {
    pub(super) fn open() -> Result<Self, String> {
        let lost = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&lost);
        let on_error = move |e: rodio::cpal::StreamError| {
            log::warn!("la salida de audio fallo: {e}");
            if matches!(
                e,
                rodio::cpal::StreamError::DeviceNotAvailable | rodio::cpal::StreamError::StreamInvalidated
            ) {
                flag.store(true, Ordering::Relaxed);
            }
        };
        // La de por defecto, con su configuracion o con otra que admita; si
        // no, cualquier otra salida del equipo, como hacia rodio 0.20. Todas
        // con nuestro aviso de errores: el de rodio solo escribe en la
        // consola y no se enteraria nadie de que el aparato se ha ido.
        let open = |builder: DeviceSinkBuilder| builder.with_error_callback(on_error.clone()).open_sink_or_fallback();
        let mut sink = DeviceSinkBuilder::from_default_device()
            .and_then(open)
            .or_else(|first| {
                let host = rodio::cpal::default_host();
                let Ok(devices) = host.output_devices() else {
                    return Err(first);
                };
                devices
                    // el dispositivo nulo de ALSA acepta el audio y lo tira:
                    // «sonaria» sin sonar
                    .filter(|device| {
                        device
                            .description()
                            .is_ok_and(|d| d.driver().is_none_or(|driver| driver != "null"))
                    })
                    .find_map(|device| DeviceSinkBuilder::from_device(device).and_then(open).ok())
                    .ok_or(first)
            })
            .map_err(|e| e.to_string())?;
        // soltarla es cosa nuestra (tras un rato en pausa): nada de avisos
        sink.log_on_drop(false);
        let rate = sink.config().sample_rate();
        let beat = Arc::new(AtomicU64::new(0));
        sink.mixer().add(Heartbeat {
            beat: Arc::clone(&beat),
            rate,
            count: 0,
        });
        Ok(Self {
            sink,
            rate: rate.get(),
            pulse: Pulse::new(beat),
            lost,
        })
    }

    pub(super) fn mixer(&self) -> &Mixer {
        self.sink.mixer()
    }

    /// cpal ya dijo que esta salida no vale.
    pub(super) fn lost(&self) -> bool {
        self.lost.load(Ordering::Relaxed)
    }
}

/// Pide el salto desde un hilo aparte y espera como mucho `within`.
///
/// `Player::try_seek` deja la orden y espera sin tope a que el mezclador la
/// atienda. Si la salida muriera justo entre mirar el latido y pedir el
/// salto, quien se queda esperando es un hilo desechable y no el de audio,
/// que rehace la salida y sigue. `None`: no contesto a tiempo.
pub(super) fn seek_within(sink: &Arc<Player>, pos: Duration, within: Duration) -> Option<Result<(), String>> {
    let (tx, rx) = std::sync::mpsc::channel();
    let sink = Arc::clone(sink);
    std::thread::Builder::new()
        .name("danplay-seek".into())
        .spawn(move || {
            let _ = tx.send(sink.try_seek(pos).map_err(|e| e.to_string()));
        })
        .ok()?;
    rx.recv_timeout(within).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Con la salida parada, el latido no se mueve y se nota; latiendo, si.
    #[test]
    fn the_pulse_tells_a_live_output_from_a_dead_one() {
        let beat = Arc::new(AtomicU64::new(0));
        let mut pulse = Pulse::new(Arc::clone(&beat));
        // recien abierta, se le da el beneficio de la duda
        assert!(pulse.responds(Duration::from_millis(10)));
        std::thread::sleep(FRESH + Duration::from_millis(50));
        assert!(!pulse.responds(Duration::from_millis(60)), "parada y dice que responde");
        assert!(pulse.silent_for() > FRESH);

        // un latido que no para hasta que se le diga: con la maquina cargada,
        // uno que parase solo podia acabar antes de que se le mirase
        let alive = Arc::clone(&beat);
        let stop = Arc::new(AtomicBool::new(false));
        let stopper = Arc::clone(&stop);
        let ticking = std::thread::spawn(move || {
            while !stopper.load(Ordering::Relaxed) {
                alive.fetch_add(1, Ordering::Relaxed);
                std::thread::sleep(Duration::from_millis(5));
            }
        });
        assert!(pulse.responds(Duration::from_secs(2)), "late y dice que no responde");
        assert!(pulse.silent_for() < FRESH);
        stop.store(true, Ordering::Relaxed);
        ticking.join().unwrap();
    }

    /// El caso que colgaba el hilo de audio: un salto que nadie va a atender
    /// (un sink cuya salida no pide muestras). Tiene que volver, y a tiempo.
    #[test]
    fn a_seek_nobody_answers_comes_back() {
        let (sink, _queue) = Player::new();
        sink.append(rodio::source::SineWave::new(440.0).take_duration(Duration::from_secs(10)));
        let sink = Arc::new(sink);
        let started = Instant::now();
        let answer = seek_within(&sink, Duration::from_secs(3), Duration::from_millis(300));
        assert!(answer.is_none(), "nadie tira de ese sink: no puede haber contestado");
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "tardo {:?}",
            started.elapsed()
        );
    }

    /// Con la salida viva, el salto se atiende y se nota en la posicion.
    #[test]
    fn a_seek_on_a_live_output_is_answered() {
        let Ok(mut output) = Output::open() else {
            eprintln!("sin salida de audio; se omite");
            return;
        };
        assert!(
            output.pulse.responds(Duration::from_secs(1)),
            "la salida recien abierta no late"
        );
        let sink = Player::connect_new(output.mixer());
        sink.set_volume(0.0);
        sink.append(rodio::source::SineWave::new(440.0).take_duration(Duration::from_secs(20)));
        let sink = Arc::new(sink);
        let answer = seek_within(&sink, Duration::from_secs(5), Duration::from_secs(2));
        assert_eq!(answer, Some(Ok(())));
        assert!(
            sink.get_pos() >= Duration::from_secs(5),
            "posicion {:?}",
            sink.get_pos()
        );
    }
}
