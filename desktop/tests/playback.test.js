import { describe, it, expect, vi, beforeEach } from 'vitest'
import { flushPromises } from '@vue/test-utils'
import {
  REPEAT_MODES,
  normalizeRepeat,
  randomOther,
  nextIndex,
  afterEnd,
  neighbours
} from '../src/playback/queueLogic.js'

// La máquina de estados de la cola. La de verdad vive en Rust (queue.rs, con
// sus mismas pruebas); esta es la del modo navegador, y tiene que decidir
// exactamente lo mismo o la app y el navegador se comportarían distinto.
describe('the queue state machine', () => {
  const q = (extra) => ({ length: 3, index: 0, repeat: 'list', shuffle: false, ...extra })

  it('the four modes and no more', () => {
    expect(REPEAT_MODES).toEqual(['list', 'one', 'once', 'queue'])
  })

  it('an unknown mode falls back to the default', () => {
    expect(normalizeRepeat('loquesea')).toBe('list')
    expect(normalizeRepeat(null)).toBe('list')
    expect(normalizeRepeat('one')).toBe('one')
  })

  it('manually it always moves, wrapping around at the ends', () => {
    expect(nextIndex(q({ index: 2 }), 1)).toBe(0)
    expect(nextIndex(q({ index: 0 }), -1)).toBe(2)
    expect(nextIndex(q({ index: 0 }), 1)).toBe(1)
  })

  it('with an empty queue there is nowhere to go', () => {
    expect(nextIndex(q({ length: 0 }), 1)).toBe(-1)
    expect(afterEnd(q({ length: 0 }))).toBe(-1)
  })

  it('repeating the list goes back to the start when it ends', () => {
    expect(afterEnd(q({ index: 2, repeat: 'list' }))).toBe(0)
    expect(afterEnd(q({ index: 1, repeat: 'list' }))).toBe(2)
  })

  it('«la lista una vez» stops at the end', () => {
    expect(afterEnd(q({ index: 2, repeat: 'queue' }))).toBe(-1)
    expect(afterEnd(q({ index: 0, repeat: 'queue' }))).toBe(1)
  })

  it('«repetir esta cancion» stays where it is', () => {
    expect(afterEnd(q({ index: 1, repeat: 'one' }))).toBe(1)
    expect(afterEnd(q({ index: 2, repeat: 'one' }))).toBe(2)
  })

  it('«solo esta cancion» always stops', () => {
    for (const index of [0, 1, 2]) expect(afterEnd(q({ index, repeat: 'once' }))).toBe(-1)
  })

  it('shuffle never repeats the one already playing', () => {
    for (let i = 0; i < 200; i++) expect(randomOther(4, 2)).not.toBe(2)
    // con una sola cancion no hay otra
    expect(randomOther(1, 0)).toBe(-1)
  })

  it('with shuffle, the end of the list does not stop the music', () => {
    const next = afterEnd(q({ index: 2, repeat: 'list', shuffle: true }))
    expect(next).toBeGreaterThanOrEqual(0)
    // salvo que se pidiera expresamente parar
    expect(afterEnd(q({ index: 2, repeat: 'once', shuffle: true }))).toBe(-1)
  })

  it('alone in the queue there is no previous or next', () => {
    expect(neighbours({ length: 1, index: 0 })).toEqual({ has_previous: false, has_next: false })
    expect(neighbours({ length: 3, index: 1 })).toEqual({ has_previous: true, has_next: true })
    expect(neighbours({ length: 0, index: -1 })).toEqual({ has_previous: false, has_next: false })
  })
})

// ------------------------------------------------------------ el composable
const held = vi.hoisted(() => ({ playback: null, api: null, state: null }))

vi.mock('../src/api.js', async (importOriginal) => {
  const actual = await importOriginal()
  const { createState, buildApiDouble, createPlaybackDouble } = await import('./support/backend.js')
  held.state = createState()
  held.api = buildApiDouble(actual, held.state)
  held.api.inTauri = true
  held.playback = createPlaybackDouble()
  return { ...actual, inTauri: true, api: held.api, playback: held.playback.bridge }
})

const { usePlayback, resetPlayback, toTrack } = await import('../src/composables/usePlayback.js')

describe('usePlayback', () => {
  beforeEach(() => {
    resetPlayback()
    held.playback.reset()
    localStorage.clear()
    vi.clearAllMocks()
  })

  it('Rust only gets what it needs from each song', () => {
    const t = toTrack({
      id: 3,
      title: 'Mi Gozo',
      artist: 'Barak',
      duration: '210',
      blur: 1,
      lyrics: 'una letra muy larga'
    })
    expect(t).toEqual({ id: 3, title: 'Mi Gozo', artist: 'Barak', duration: 210, blur: true })
  })

  it('without a title the file name is used', () => {
    expect(toTrack({ id: 1, file: 'pista.mp3' }).title).toBe('pista.mp3')
  })

  // Con la ruta puesta, Rust arranca sin esperar al nucleo. Cuando iba lento
  // o aun se estaba levantando, esa espera era «le doy y no suena».
  it('the path travels with the song when known', () => {
    expect(toTrack({ id: 1, title: 'x', path: '/musica/x.mp3' }).path).toBe('/musica/x.mp3')
    expect('path' in toTrack({ id: 1, title: 'x', path: '' })).toBe(false)
    expect('path' in toTrack({ id: 1, title: 'x' })).toBe(false)
  })

  it('sets the queue and reflects what Rust answers', async () => {
    const player = usePlayback()
    await player.setQueue(
      [
        { id: 1, title: 'A', artist: 'X', duration: 100 },
        { id: 2, title: 'B', artist: 'X', duration: 120 }
      ],
      2,
      { kind: 'all', label: 'Todas' }
    )
    await flushPromises()
    expect(held.playback.bridge.setQueue).toHaveBeenCalled()
    expect(player.track.value.id).toBe(2)
    expect(player.playing.value).toBe(true)
    expect(player.hasNext.value).toBe(true)
  })

  it('the «No está» songs of a playlist do not enter the queue', async () => {
    const player = usePlayback()
    await player.setQueue(
      [
        { id: 1, title: 'A', artist: 'X', duration: 100 },
        { id: 9, title: 'Se fue', artist: 'X', duration: 100, ghost: true },
        { id: 2, title: 'B', artist: 'X', duration: 120 }
      ],
      1,
      { kind: 'playlist', id: 3, label: 'Domingo' }
    )
    await flushPromises()
    expect(player.queue.value.map((s) => s.id)).toEqual([1, 2])
  })

  it('if Rust swaps the queue for another of the same size, the UI notices', async () => {
    // El caso de verdad: abrir una canción con DanPlay desde el explorador
    // deja una cola de UNA. Si ya había una cola de una, los dos números
    // coinciden, y antes de esto la interfaz seguía enseñando la canción
    // anterior mientras sonaba la nueva.
    const player = usePlayback()
    await player.setQueue([{ id: -1, title: 'La primera', artist: '', duration: 100 }])
    await flushPromises()
    expect(player.track.value.title).toBe('La primera')

    held.playback.replaceQueueFromRust([
      { id: -1, title: 'La segunda', artist: 'Alguien', duration: 200 }
    ])
    await flushPromises()
    expect(player.track.value.title, 'se quedó con la anterior').toBe('La segunda')
    expect(player.queue.value[0].title).toBe('La segunda')
  })

  it('volume and speed are remembered between sessions', async () => {
    const player = usePlayback()
    await player.setVolume(0.42)
    await player.setSpeed(1.25)
    expect(localStorage.getItem('danplay.vol')).toBe('0.42')
    expect(localStorage.getItem('danplay.speed')).toBe('1.25')
  })

  it('saved settings are applied by the main window, not the mini player or the projection', async () => {
    const saved = () => {
      localStorage.setItem('danplay.vol', '0.3')
      localStorage.setItem('danplay.speed', '0.75')
      localStorage.setItem('danplay.repeat', 'one')
    }
    for (const search of ['?mini=1', '?projection=1']) {
      resetPlayback()
      held.playback.reset()
      vi.clearAllMocks()
      saved()
      window.history.replaceState(null, '', '/' + search)
      await usePlayback().ready()
      const b = held.playback.bridge
      expect(b.setVolume, search).not.toHaveBeenCalled()
      expect(b.setSpeed, search).not.toHaveBeenCalled()
      expect(b.setRepeat, search).not.toHaveBeenCalled()
    }
    resetPlayback()
    held.playback.reset()
    vi.clearAllMocks()
    saved()
    window.history.replaceState(null, '', '/')
    await usePlayback().ready()
    expect(held.playback.bridge.setVolume).toHaveBeenCalledWith(0.3)
    expect(held.playback.bridge.setSpeed).toHaveBeenCalledWith(0.75)
    expect(held.playback.bridge.setRepeat).toHaveBeenCalledWith('one')
  })

  it('the crossfade between songs is chosen, remembered and sent on startup', async () => {
    let player = usePlayback()
    await player.ready()
    // de fábrica: 4 s, mandado a Rust nada más arrancar
    expect(player.crossfade.value).toBe(4)
    expect(held.playback.bridge.setCrossfade).toHaveBeenCalledWith(4)
    await player.setCrossfade(6)
    expect(held.playback.bridge.setCrossfade).toHaveBeenLastCalledWith(6)
    expect(localStorage.getItem('danplay.crossfade')).toBe('6')
    await player.setCrossfade(99)
    expect(player.crossfade.value, 'hasta 12 s').toBe(12)
    await player.setCrossfade(0)
    expect(held.playback.bridge.setCrossfade).toHaveBeenLastCalledWith(0)
    // la sesión siguiente arranca con el elegido
    resetPlayback()
    held.playback.reset()
    vi.clearAllMocks()
    player = usePlayback()
    await player.ready()
    expect(player.crossfade.value).toBe(0)
    expect(held.playback.bridge.setCrossfade).toHaveBeenCalledWith(0)
  })

  it('volume stays in range: up to 50 % above the original level', async () => {
    const player = usePlayback()
    await player.setVolume(9)
    await flushPromises()
    expect(player.volume.value).toBe(1.5)
    await player.setVolume(-3)
    await flushPromises()
    expect(player.volume.value).toBe(0)
  })

  it('seeking does not go past the end or the start', async () => {
    const player = usePlayback()
    await player.setQueue([{ id: 1, title: 'A', artist: 'X', duration: 100 }], 1, null)
    await flushPromises()
    held.playback.emit({ position: 95, duration: 100 })
    await flushPromises()
    await player.nudge(30)
    expect(held.playback.bridge.seek).toHaveBeenLastCalledWith(100)
    held.playback.emit({ position: 3 })
    await flushPromises()
    await player.nudge(-30)
    expect(held.playback.bridge.seek).toHaveBeenLastCalledWith(0)
  })

  // Al pedir un salto, la aguja se pinta ya donde se pidio. Un tick que venia
  // en camino con la posicion vieja no la devuelve atras; en cuanto Rust
  // confirma (o pasa el plazo), vuelve a mandar lo que diga Rust.
  it('a seek shows right away and an old tick does not overwrite it', async () => {
    const player = usePlayback()
    await player.setQueue([{ id: 1, title: 'A', artist: 'X', duration: 200 }], 1, null)
    await flushPromises()
    // el doble confirma al instante; aqui interesa lo que pasa ANTES de eso
    held.playback.bridge.seek.mockImplementationOnce(async () => {})
    held.playback.emit({ position: 12, playing: true })
    const p = player.seek(100)
    expect(player.position.value, 'la aguja no se movio al pedirlo').toBe(100)
    await p
    held.playback.emit({ position: 12.3 }) // el tick que ya venia de camino
    expect(player.position.value).toBe(100)
    held.playback.emit({ position: 100.4 }) // Rust confirma
    expect(player.position.value).toBe(100.4)
    held.playback.emit({ position: 20 }) // y a partir de ahi, lo que diga Rust
    expect(player.position.value).toBe(20)
  })

  it('if Rust does not confirm the seek, after the timeout its position wins', async () => {
    vi.useFakeTimers()
    try {
      const player = usePlayback()
      await player.setQueue([{ id: 1, title: 'A', artist: 'X', duration: 200 }], 1, null)
      held.playback.bridge.seek.mockImplementationOnce(async () => {})
      await player.seek(100)
      held.playback.emit({ position: 12 })
      expect(player.position.value).toBe(100)
      vi.advanceTimersByTime(900)
      held.playback.emit({ position: 12.5 })
      expect(player.position.value).toBe(12.5)
    } finally {
      vi.useRealTimers()
    }
  })

  it('a seek does not leave the song', async () => {
    const player = usePlayback()
    await player.setQueue([{ id: 1, title: 'A', artist: 'X', duration: 200 }], 1, null)
    await player.seek(999)
    expect(held.playback.bridge.seek).toHaveBeenLastCalledWith(200)
    await player.seek(-4)
    expect(held.playback.bridge.seek).toHaveBeenLastCalledWith(0)
  })

  it('to a percentage: keys 1-9', async () => {
    const player = usePlayback()
    await player.setQueue([{ id: 1, title: 'A', artist: 'X', duration: 200 }], 1, null)
    await player.seekPercent(50)
    expect(held.playback.bridge.seek).toHaveBeenLastCalledWith(100)
    await player.seekPercent(90)
    expect(held.playback.bridge.seek).toHaveBeenLastCalledWith(180)
  })

  it('from the start: goes back to 0 and, if paused, starts playing (in that order)', async () => {
    const player = usePlayback()
    await player.setQueue([{ id: 1, title: 'A', artist: 'X', duration: 200 }], 1, null)
    held.playback.emit({ position: 80, playing: true })
    vi.clearAllMocks()
    await player.restart()
    expect(held.playback.bridge.seek).toHaveBeenCalledWith(0)
    expect(held.playback.bridge.toggle, 'sonando no hay que tocar play').not.toHaveBeenCalled()

    held.playback.emit({ position: 80, playing: false })
    vi.clearAllMocks()
    const order = []
    held.playback.bridge.toggle.mockImplementationOnce(async () => order.push('toggle'))
    held.playback.bridge.seek.mockImplementationOnce(async () => order.push('seek'))
    await player.restart()
    // primero arranca y luego salta: si la cancion habia acabado, el salto ya
    // la pone en marcha y un «reanudar» detras la dejaria otra vez en pausa
    expect(order).toEqual(['toggle', 'seek'])
  })

  it('with nothing loaded, «desde el principio» sends nothing', async () => {
    const player = usePlayback()
    await player.ready()
    vi.clearAllMocks()
    await player.restart()
    expect(held.playback.bridge.seek).not.toHaveBeenCalled()
    expect(held.playback.bridge.toggle).not.toHaveBeenCalled()
  })

  it('editing a song shows in what is playing without fetching the queue again', async () => {
    const player = usePlayback()
    await player.setQueue([{ id: 1, title: 'A', artist: 'X', duration: 100, stars: 0 }], 1, null)
    await flushPromises()
    player.patchItem({ id: 1, stars: 5 })
    await flushPromises()
    expect(player.track.value.stars).toBe(5)
  })

  it('the repeat mode cycles in order', async () => {
    const player = usePlayback()
    for (const expected of ['one', 'once', 'queue', 'list']) {
      await player.cycleRepeat()
      await flushPromises()
      expect(player.repeat.value).toBe(expected)
    }
  })
})
