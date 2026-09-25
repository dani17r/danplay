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

  // Con la ruta puesta, Rust arranca sin esperar al nucleo. Cuando iba lento
  // o aun se estaba levantando, esa espera era «le doy y no suena».
  it('la ruta viaja con la cancion cuando se conoce', () => {
    expect(toTrack({ id: 1, title: 'x', path: '/musica/x.mp3' }).path).toBe('/musica/x.mp3')
    expect('path' in toTrack({ id: 1, title: 'x', path: '' })).toBe(false)
    expect('path' in toTrack({ id: 1, title: 'x' })).toBe(false)
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

  it('lo guardado lo aplica la ventana principal, no el mini ni la proyección', async () => {
    const guardado = () => {
      localStorage.setItem('danplay.vol', '0.3')
      localStorage.setItem('danplay.vel', '0.75')
      localStorage.setItem('danplay.repeat', 'one')
    }
    for (const search of ['?mini=1', '?projection=1']) {
      resetPlayback()
      held.playback.reset()
      vi.clearAllMocks()
      guardado()
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
    guardado()
    window.history.replaceState(null, '', '/')
    await usePlayback().ready()
    expect(held.playback.bridge.setVolume).toHaveBeenCalledWith(0.3)
    expect(held.playback.bridge.setSpeed).toHaveBeenCalledWith(0.75)
    expect(held.playback.bridge.setRepeat).toHaveBeenCalledWith('one')
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

  // Al pedir un salto, la aguja se pinta ya donde se pidio. Un tick que venia
  // en camino con la posicion vieja no la devuelve atras; en cuanto Rust
  // confirma (o pasa el plazo), vuelve a mandar lo que diga Rust.
  it('un salto se ve al momento y no lo pisa un tick viejo', async () => {
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

  it('si Rust no confirma el salto, pasado el plazo manda su posicion', async () => {
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

  it('un salto no se sale de la cancion', async () => {
    const player = usePlayback()
    await player.setQueue([{ id: 1, title: 'A', artist: 'X', duration: 200 }], 1, null)
    await player.seek(999)
    expect(held.playback.bridge.seek).toHaveBeenLastCalledWith(200)
    await player.seek(-4)
    expect(held.playback.bridge.seek).toHaveBeenLastCalledWith(0)
  })

  it('al tanto por ciento: las teclas 1-9', async () => {
    const player = usePlayback()
    await player.setQueue([{ id: 1, title: 'A', artist: 'X', duration: 200 }], 1, null)
    await player.seekPercent(50)
    expect(held.playback.bridge.seek).toHaveBeenLastCalledWith(100)
    await player.seekPercent(90)
    expect(held.playback.bridge.seek).toHaveBeenLastCalledWith(180)
  })

  it('desde el principio: vuelve a 0 y, en pausa, arranca (en ese orden)', async () => {
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

  it('sin nada cargado, «desde el principio» no manda nada', async () => {
    const player = usePlayback()
    await player.ready()
    vi.clearAllMocks()
    await player.restart()
    expect(held.playback.bridge.seek).not.toHaveBeenCalled()
    expect(held.playback.bridge.toggle).not.toHaveBeenCalled()
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
