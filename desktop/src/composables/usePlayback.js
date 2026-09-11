// Lo que suena y lo que viene, compartido por toda la interfaz.
//
// Es un singleton, como useDragSong: el reproductor grande, el mini y la app
// leen el MISMO estado y mandan las órdenes al mismo sitio. Dentro de Tauri
// ese sitio es Rust, que tiene la cola y cuenta cada cambio por el evento
// `danplay://state` (docs/CONTRATO-INTERNO.md §1); aquí no hay ningún sondeo.
// Fuera de Tauri (navegador y pruebas) hay un reproductor web con <audio>
// que aplica exactamente la misma máquina de estados (playback/queueLogic).
import { reactive, shallowRef, computed } from 'vue'
import { api, playback as bridge } from '../api.js'
import { afterEnd, nextIndex, neighbours, normalizeRepeat, REPEAT_MODES } from '../playback/queueLogic.js'

/** @typedef {import('../api.js').PlaybackState} PlaybackState */
/** @typedef {import('../api.js').Track} Track */

const VOLUME_KEY = 'danplay.vol'
const SPEED_KEY = 'danplay.vel'
const REPEAT_KEY = 'danplay.repeat'

/** @type {PlaybackState} */
const EMPTY = {
  track: null,
  index: -1,
  length: 0,
  playing: false,
  position: 0,
  duration: 0,
  volume: 0.9,
  speed: 1,
  repeat: 'list',
  shuffle: false,
  has_previous: false,
  has_next: false,
  error: '',
  has_output: true,
  origin: null,
  // modo estudio: la velocidad conserva el tono, y el bucle A-B (0,0 = sin bucle)
  pitch_preserved: true,
  loop_a: 0,
  loop_b: 0
}

/**
 * Lo mínimo que Rust necesita de una canción.
 *
 * La ruta va también: la interfaz la conoce del índice y con ella Rust puede
 * ponerla a sonar sin preguntarle nada al núcleo. Cuando el núcleo iba lento
 * o aún se estaba levantando, esa pregunta era lo que dejaba el reproductor
 * mudo («le doy y no suena»). Si el archivo ya no está ahí, Rust vuelve a
 * preguntar al núcleo, que es quien sabe si se movió.
 * @param {import('../api.js').Song} s
 * @returns {Track}
 */
export function toTrack(s) {
  const t = {
    id: s.id,
    title: s.title || s.file || '',
    artist: s.artist || '',
    duration: Number(s.duration) || 0,
    blur: !!s.blur
  }
  if (typeof s.path === 'string' && s.path) t.path = s.path
  return t
}

const clamp = (v, lo, hi) => Math.min(hi, Math.max(lo, v))

// ------------------------------------------------------------ reproductor web
/**
 * El mismo contrato que Rust, con un <audio> del navegador. Solo se usa
 * fuera de la app (npm run dev en un navegador, y las pruebas).
 */
function createWebBackend() {
  const audio = document.createElement('audio')
  audio.preload = 'metadata'
  audio.dataset.danplay = 'audio'
  const listeners = new Set()
  const q = { items: /** @type {Track[]} */ ([]), index: -1, repeat: 'list', shuffle: false, origin: null }
  const s = { ...EMPTY }
  let ended = false // la pista acabó y se paró: play la vuelve a empezar

  const logic = () => ({ length: q.items.length, index: q.index, repeat: q.repeat, shuffle: q.shuffle })
  function snapshot() {
    return {
      ...s,
      index: q.index,
      length: q.items.length,
      repeat: q.repeat,
      shuffle: q.shuffle,
      origin: q.origin,
      ...neighbours(logic())
    }
  }
  const push = () => {
    const snap = snapshot()
    for (const fn of listeners) fn(snap)
  }

  function start() {
    s.playing = true
    s.error = ''
    Promise.resolve(audio.play()).catch(() => {})
    push()
  }
  function load(i, play = true) {
    const item = q.items[i]
    if (!item) return stop()
    q.index = i
    ended = false
    s.track = { ...item }
    s.position = 0
    s.duration = item.duration || 0
    s.error = ''
    audio.src = api.audioUrl(item.id)
    audio.volume = s.volume
    audio.playbackRate = s.speed
    if (play) start()
    else push()
  }
  function stop() {
    s.playing = false
    s.track = null
    s.position = 0
    s.duration = 0
    q.index = -1
    ended = false
    audio.pause()
    audio.removeAttribute('src')
    push()
  }
  function seekTo(seconds) {
    const v = clamp(seconds, 0, s.duration || seconds)
    try {
      audio.currentTime = v
    } catch {
      /* sin metadatos aún: se queda en la posición pedida */
    }
    s.position = v
    if (v < (s.duration || Infinity)) ended = false
    push()
  }
  function onEnded() {
    const next = afterEnd(logic())
    if (next < 0) {
      ended = true
      s.playing = false
      s.position = s.duration
      push()
      return
    }
    load(next)
  }

  audio.addEventListener('timeupdate', () => {
    s.position = audio.currentTime || 0
    // bucle A-B: al pasar de B, vuelta a A (Rust hace lo mismo en la app)
    if (s.loop_b > s.loop_a && s.position >= s.loop_b) {
      audio.currentTime = s.loop_a
      s.position = s.loop_a
    }
    push()
  })
  audio.addEventListener('durationchange', () => {
    if (Number.isFinite(audio.duration) && audio.duration > 0) s.duration = audio.duration
    push()
  })
  audio.addEventListener('play', () => {
    s.playing = true
    s.error = ''
    push()
  })
  audio.addEventListener('pause', () => {
    s.playing = false
    push()
  })
  audio.addEventListener('error', () => {
    s.playing = false
    s.error = 'No se pudo reproducir este archivo'
    push()
  })
  audio.addEventListener('ended', onEnded)

  return {
    available: true,
    audio,
    setQueue: async (items, start, origin) => {
      q.items = items.slice()
      q.origin = origin ?? null
      const i = start == null ? 0 : q.items.findIndex((x) => x.id === start)
      load(i < 0 ? 0 : i)
    },
    next: async () => {
      const i = nextIndex(logic(), 1)
      if (i >= 0) load(i)
    },
    previous: async () => {
      const i = nextIndex(logic(), -1)
      if (i >= 0) load(i)
    },
    jump: async (id) => {
      const i = q.items.findIndex((x) => x.id === id)
      if (i >= 0) load(i)
    },
    setRepeat: async (mode) => {
      q.repeat = normalizeRepeat(mode)
      push()
    },
    setShuffle: async (on) => {
      q.shuffle = !!on
      push()
    },
    toggle: async () => {
      if (!s.track) return
      if (ended) return seekTo(0), start()
      if (s.playing) {
        s.playing = false
        audio.pause()
        push()
      } else start()
    },
    stop: async () => stop(),
    seek: async (seconds) => seekTo(seconds),
    setVolume: async (value) => {
      s.volume = clamp(Number(value) || 0, 0, 1)
      audio.volume = s.volume
      push()
    },
    setSpeed: async (value) => {
      s.speed = clamp(Number(value) || 1, 0.25, 3)
      // el navegador conserva el tono al cambiar playbackRate
      audio.playbackRate = s.speed
      push()
    },
    setLoop: async (a, b) => {
      const ok = Number.isFinite(a) && Number.isFinite(b) && b > a
      s.loop_a = ok ? a : 0
      s.loop_b = ok ? b : 0
      push()
    },
    state: async () => snapshot(),
    queueItems: async () => ({ items: q.items.slice(), origin: q.origin }),
    onState: async (fn) => {
      listeners.add(fn)
      return () => listeners.delete(fn)
    }
  }
}

// ------------------------------------------------------------- estado único
const state = reactive({ ...EMPTY })
/** Espejo local de la cola: los objetos completos que se pasaron a setQueue. */
const queue = shallowRef(/** @type {Array<Track & Record<string, any>>} */ ([]))
let backend = null
let booting = null
let ready = false
let stopListening = null
/** La revisión de cola que ya conocemos. Ver `applyState`. */
let seenRevision = -1

const isMiniWindow = () =>
  typeof location !== 'undefined' && new URLSearchParams(location.search).has('mini')

function readNumber(key) {
  try {
    const v = localStorage.getItem(key)
    return v == null || v === '' ? null : Number(v)
  } catch {
    return null
  }
}
function remember(key, value) {
  try {
    localStorage.setItem(key, String(value))
  } catch {
    /* modo privado */
  }
}

/**
 * Lo que Rust (o el reproductor web) acaba de contar.
 * @param {PlaybackState} s
 */
function applyState(s) {
  if (!s || typeof s !== 'object') return
  for (const k of Object.keys(EMPTY)) if (k in s) state[k] = s[k]
  state.repeat = normalizeRepeat(state.repeat)
  // La cola la cambió alguien que no es esta ventana: el mini, la bandeja, o
  // el sistema al abrir una canción con DanPlay. Se vuelve a pedir.
  //
  // Se mira la revisión que trae Rust, y no el número de canciones: abrir una
  // canción desde el explorador deja una cola de UNA, y si ya había una cola
  // de una los números coinciden, así que la pantalla se quedaba con la
  // canción anterior mientras sonaba la nueva. Por el id tampoco vale: dos
  // archivos sueltos distintos pueden llevar el mismo.
  const changed = 'revision' in s && s.revision !== seenRevision
  if (changed) seenRevision = s.revision
  if (ready && backend && (changed || state.length !== queue.value.length)) refreshQueue(changed)
  if (ready) {
    remember(VOLUME_KEY, state.volume)
    remember(SPEED_KEY, state.speed)
    remember(REPEAT_KEY, state.repeat)
  }
}

/**
 * Vuelve a traerse la cola de Rust.
 *
 * Sin `force` se conforma con comparar los ids, que es barato y evita
 * repintar la lista por nada. Con `force` se queda con lo que diga Rust: los
 * ids no siempre distinguen dos canciones —los archivos abiertos desde fuera
 * de la biblioteca no tienen id propio— y ahí la comparación decía «es la
 * misma» sobre dos canciones distintas.
 */
async function refreshQueue(force = false) {
  try {
    const r = await backend.queueItems()
    const items = r?.items || []
    const same =
      !force &&
      items.length === queue.value.length &&
      items.every((t, i) => t.id === queue.value[i]?.id)
    if (!same) queue.value = items
    if (r && 'origin' in r && r.origin != null) state.origin = r.origin
  } catch {
    /* Rust aún no responde: la próxima orden lo vuelve a intentar */
  }
}

async function boot() {
  backend = api.inTauri ? bridge : createWebBackend()
  try {
    stopListening = await backend.onState(applyState)
    // Lo guardado se aplica solo desde la ventana principal: el mini nace
    // después y no debe pisar lo que el usuario cambió desde la bandeja.
    if (!isMiniWindow()) {
      const vol = readNumber(VOLUME_KEY)
      const vel = readNumber(SPEED_KEY)
      let repeat = null
      try {
        repeat = localStorage.getItem(REPEAT_KEY)
      } catch {
        /* sin almacenamiento */
      }
      if (vol != null && Number.isFinite(vol)) await backend.setVolume(clamp(vol, 0, 1))
      if (vel != null && Number.isFinite(vel) && vel > 0) await backend.setSpeed(vel)
      if (REPEAT_MODES.includes(repeat)) await backend.setRepeat(repeat)
    }
    applyState(await backend.state())
    await refreshQueue()
  } catch (e) {
    // Sin Rust detrás (o con una versión que aún no trae la cola) la
    // interfaz sigue pintándose; solo no sonará nada.
    state.error = state.error || ''
    console.warn('[danplay] reproducción no disponible:', e)
  }
  ready = true
}

function ensureBooted() {
  if (!booting) booting = boot()
  return booting
}

/** Vuelve al estado inicial. Solo para pruebas y recarga en caliente. */
export function resetPlayback() {
  if (typeof stopListening === 'function') stopListening()
  stopListening = null
  backend = null
  booting = null
  ready = false
  seenRevision = -1
  queue.value = []
  Object.assign(state, { ...EMPTY })
}

// ------------------------------------------------------------------ órdenes
/** Manda una orden en cuanto el puente esté listo. */
async function send(fn) {
  await ensureBooted()
  if (!backend) return
  try {
    await fn(backend)
  } catch (e) {
    state.error = String(e?.message || e)
  }
}

/**
 * Sustituye la cola y empieza por `startId` (o por el primero).
 * @param {Array<import('../api.js').Song>} items
 * @param {number|null} [startId]
 * @param {any} [origin]  de dónde salió, para poder volver
 */
function setQueue(items, startId = null, origin = null) {
  const list = (items || []).filter((s) => s && s.id != null)
  queue.value = list.slice()
  state.origin = origin
  return send((b) => b.setQueue(list.map(toTrack), startId, origin))
}
const next = () => send((b) => b.next())
const previous = () => send((b) => b.previous())
const jump = (id) => send((b) => b.jump(id))
const toggle = () => send((b) => b.toggle())
const stop = () => send((b) => b.stop())
const seek = (seconds) => send((b) => b.seek(Math.max(0, Number(seconds) || 0)))
function setVolume(value) {
  const v = clamp(Number(value) || 0, 0, 1)
  remember(VOLUME_KEY, v)
  return send((b) => b.setVolume(v))
}
/** Repetir de A a B (segundos de la canción). Sin valores, se quita. */
function setLoop(a = null, b = null) {
  const ok = Number.isFinite(a) && Number.isFinite(b) && b > a
  return send((bk) => bk.setLoop(ok ? a : null, ok ? b : null))
}
const clearLoop = () => setLoop(null, null)

function setSpeed(value) {
  const v = clamp(Number(value) || 1, 0.25, 3)
  remember(SPEED_KEY, v)
  return send((b) => b.setSpeed(v))
}
function setRepeat(mode) {
  const m = normalizeRepeat(mode)
  remember(REPEAT_KEY, m)
  return send((b) => b.setRepeat(m))
}
const setShuffle = (on) => send((b) => b.setShuffle(!!on))
/** Adelanta o atrasa `seconds` desde donde va. */
function nudge(seconds) {
  const target = clamp(state.position + seconds, 0, state.duration || Infinity)
  return seek(Number.isFinite(target) ? target : 0)
}
function cycleRepeat() {
  const i = REPEAT_MODES.indexOf(state.repeat)
  return setRepeat(REPEAT_MODES[(i + 1) % REPEAT_MODES.length])
}
const toggleShuffle = () => setShuffle(!state.shuffle)

/**
 * Una canción cambió (estrellas, título, portada…): se refresca en el
 * espejo de la cola, que es de donde el reproductor saca los datos.
 * @param {import('../api.js').Song} song
 */
function patchItem(song) {
  if (!song || song.id == null) return
  const i = queue.value.findIndex((x) => x.id === song.id)
  if (i < 0) return
  const list = queue.value.slice()
  list[i] = { ...list[i], ...song }
  queue.value = list
}

// La pista con todo lo que sabemos de ella: lo que Rust cuenta (id, título,
// artista, duración, blur) más lo que el espejo local tenga de más (tono,
// bpm, carpeta…). Lo local manda: es más fresco y más rico.
const track = computed(() => {
  const t = state.track
  if (!t) return null
  const local = queue.value.find((x) => x.id === t.id)
  return local ? { ...t, ...local } : t
})

export function usePlayback() {
  ensureBooted()
  return {
    state,
    track,
    queue,
    origin: computed(() => state.origin),
    playing: computed(() => state.playing),
    position: computed(() => state.position),
    duration: computed(() => state.duration || track.value?.duration || 0),
    volume: computed(() => state.volume),
    speed: computed(() => state.speed),
    repeat: computed(() => state.repeat),
    shuffle: computed(() => state.shuffle),
    hasPrevious: computed(() => state.has_previous),
    hasNext: computed(() => state.has_next),
    error: computed(() => state.error),
    hasOutput: computed(() => state.has_output),
    ready: ensureBooted,
    setQueue,
    next,
    previous,
    jump,
    toggle,
    stop,
    seek,
    nudge,
    setVolume,
    setSpeed,
    setLoop,
    clearLoop,
    loopA: computed(() => state.loop_a),
    loopB: computed(() => state.loop_b),
    pitchPreserved: computed(() => state.pitch_preserved !== false),
    setRepeat,
    cycleRepeat,
    setShuffle,
    toggleShuffle,
    patchItem,
    /** El <audio> del reproductor web, o null dentro de la app. Para pruebas. */
    audioElement: () => backend?.audio || null
  }
}
