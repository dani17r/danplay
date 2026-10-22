// @ts-check
// Los mandos de fuera de la app: la notificación de la bandeja (con pausa,
// anterior y siguiente), la pantalla de bloqueo, los auriculares y el reloj.
//
// Los dibuja Android con una «sesión multimedia» que lleva la app nativa
// (PlaybackService.kt). La página le cuenta qué suena por `DanPlayAndroid`, el
// objeto que MainActivity pone en el WebView. Lo que se pulsa va de Android
// directo al motor en Rust (mobile/src/controls.rs), y la página se entera por
// el evento de siempre. Fuera del teléfono no hay ese objeto y esto no hace
// nada.
import { watch } from 'vue'
import { usePlayback } from '../composables/usePlayback.js'

/** @returns {{update: (json: string) => void} | null} */
function nativeBridge() {
  const b = /** @type {any} */ (window).DanPlayAndroid
  return b && typeof b.update === 'function' ? b : null
}

export function useMediaSession() {
  const player = usePlayback()

  let sent = ''
  let lastPos = 0
  let lastAt = 0
  /** Le cuenta a Android lo que suena. La posición, solo cuando salta. */
  function send(force = false) {
    // se busca cada vez: Android puede ponerlo después de cargar la página
    const bridge = nativeBridge()
    if (!bridge) return
    const t = player.track.value
    const s = player.state
    const msg = t
      ? {
          id: t.id,
          title: t.title || '',
          artist: t.artist || '',
          // la carpeta de la pista: de ahí saca Android la portada
          folder: typeof t.path === 'string' ? t.path : s.path || '',
          duration: Math.round((player.duration.value || 0) * 1000),
          position: Math.round((s.position || 0) * 1000),
          speed: s.speed || 1,
          playing: !!s.playing,
          hasNext: !!s.has_next,
          hasPrevious: true
        }
      : { id: null }
    // lo que no es la posición: si no cambió, no se manda
    const key = JSON.stringify({ ...msg, position: 0 })
    const now = Date.now()
    const expected = lastPos + (s.playing ? (now - lastAt) * (s.speed || 1) : 0)
    const jumped = t && Math.abs((msg.position || 0) - expected) > 1500
    if (!force && key === sent && !jumped) return
    sent = key
    lastPos = msg.position || 0
    lastAt = now
    try {
      bridge.update(JSON.stringify(msg))
    } catch {
      /* la app nativa no contesta: la bandeja se queda como estaba */
    }
  }

  watch(
    () => [
      player.track.value?.id,
      player.track.value?.title,
      player.state.playing,
      player.state.speed,
      player.duration.value,
      player.state.has_next
    ],
    () => send(true)
  )
  watch(
    () => player.state.position,
    () => send()
  )
}
