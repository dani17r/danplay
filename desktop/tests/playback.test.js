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
describe('la maquina de estados de la cola', () => {
  const q = (extra) => ({ length: 3, index: 0, repeat: 'list', shuffle: false, ...extra })

  it('los cuatro modos y ninguno mas', () => {
    expect(REPEAT_MODES).toEqual(['list', 'one', 'once', 'queue'])
  })

  it('un modo que no existe cae en el de siempre', () => {
    expect(normalizeRepeat('loquesea')).toBe('list')
    expect(normalizeRepeat(null)).toBe('list')
    expect(normalizeRepeat('one')).toBe('one')
  })

  it('a mano siempre se mueve, dando la vuelta en los extremos', () => {
    expect(nextIndex(q({ index: 2 }), 1)).toBe(0)
    expect(nextIndex(q({ index: 0 }), -1)).toBe(2)
    expect(nextIndex(q({ index: 0 }), 1)).toBe(1)
  })

  it('con la cola vacia no hay a donde ir', () => {
    expect(nextIndex(q({ length: 0 }), 1)).toBe(-1)
    expect(afterEnd(q({ length: 0 }))).toBe(-1)
  })

  it('repetir la lista vuelve al principio al acabarse', () => {
    expect(afterEnd(q({ index: 2, repeat: 'list' }))).toBe(0)
    expect(afterEnd(q({ index: 1, repeat: 'list' }))).toBe(2)
  })

  it('«la lista una vez» se para al final', () => {
    expect(afterEnd(q({ index: 2, repeat: 'queue' }))).toBe(-1)
    expect(afterEnd(q({ index: 0, repeat: 'queue' }))).toBe(1)
  })

  it('«repetir esta cancion» se queda donde esta', () => {
    expect(afterEnd(q({ index: 1, repeat: 'one' }))).toBe(1)
    expect(afterEnd(q({ index: 2, repeat: 'one' }))).toBe(2)
  })

  it('«solo esta cancion» se para siempre', () => {
    for (const index of [0, 1, 2]) expect(afterEnd(q({ index, repeat: 'once' }))).toBe(-1)
  })

  it('el azar nunca repite la que ya suena', () => {
    for (let i = 0; i < 200; i++) expect(randomOther(4, 2)).not.toBe(2)
    // con una sola cancion no hay otra
    expect(randomOther(1, 0)).toBe(-1)
  })

  it('con aleatorio, el final de la lista no para la musica', () => {
    const next = afterEnd(q({ index: 2, repeat: 'list', shuffle: true }))
    expect(next).toBeGreaterThanOrEqual(0)
    // salvo que se pidiera expresamente parar
    expect(afterEnd(q({ index: 2, repeat: 'once', shuffle: true }))).toBe(-1)
  })

  it('sola en la cola no hay anterior ni siguiente', () => {
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

  it('a Rust solo se le manda lo que necesita de cada cancion', () => {
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

  it('sin titulo se usa el nombre del archivo', () => {
    expect(toTrack({ id: 1, file: 'pista.mp3' }).title).toBe('pista.mp3')
  })

  it('pone la cola y refleja lo que Rust contesta', async () => {
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

  it('si Rust cambia la cola por otra del mismo tamaño, la interfaz se entera', async () => {
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

  it('el volumen y la velocidad se recuerdan entre sesiones', async () => {
    const player = usePlayback()
    await player.setVolume(0.42)
    await player.setSpeed(1.25)
    expect(localStorage.getItem('danplay.vol')).toBe('0.42')
    expect(localStorage.getItem('danplay.vel')).toBe('1.25')
  })

  it('el volumen no se sale de su rango', async () => {
    const player = usePlayback()
    await player.setVolume(9)
    await flushPromises()
    expect(player.volume.value).toBe(1)
    await player.setVolume(-3)
    await flushPromises()
    expect(player.volume.value).toBe(0)
  })

  it('adelantar no se pasa del final ni del principio', async () => {
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

  it('cambiar una cancion se ve en lo que suena sin volver a pedir la cola', async () => {
    const player = usePlayback()
    await player.setQueue([{ id: 1, title: 'A', artist: 'X', duration: 100, stars: 0 }], 1, null)
    await flushPromises()
    player.patchItem({ id: 1, stars: 5 })
    await flushPromises()
    expect(player.track.value.stars).toBe(5)
  })

  it('el modo de repeticion cicla en orden', async () => {
    const player = usePlayback()
    for (const esperado of ['one', 'once', 'queue', 'list']) {
      await player.cycleRepeat()
      await flushPromises()
      expect(player.repeat.value).toBe(esperado)
    }
  })
})
