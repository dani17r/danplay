import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'

// La ventanita de la bandeja no comparte nada con la app: lo que suena se lo
// cuentan, y el avance lo pregunta al hilo de audio. Estas pruebas fijan lo
// que el usuario pidio: que los botones esten en gris cuando no se pueden
// pulsar, segun el estado real del reproductor.
const { api, native, tray, escuchas } = vi.hoisted(() => {
  const escuchas = { cambios: null }
  return {
    api: { inTauri: true, coverUrl: (id) => `/cover/${id}`, coverUrlAlt: () => null },
    native: {
      available: true,
      status: vi.fn(async () => ({ playing: false, position: 0, duration: 0 })),
      togglePlay: vi.fn(async () => {}),
      seek: vi.fn(async () => {})
    },
    tray: {
      available: false,             // sin ventana de verdad que colocar
      nowPlaying: vi.fn(async () => null),
      onChanged: vi.fn(async (fn) => { escuchas.cambios = fn; return () => {} }),
      send: vi.fn(async () => {}),
      showApp: vi.fn(async () => {}),
      closeMini: vi.fn(async () => {})
    },
    escuchas
  }
})
vi.mock('../src/api.js', () => ({ api, native, tray }))
import MiniPlayer from '../src/MiniPlayer.vue'

const SONANDO = {
  id: 7, title: 'Mi Gozo', artist: 'Barak',
  playing: true, has_previous: true, has_next: true
}

let w = null
beforeEach(() => {
  vi.clearAllMocks()
  native.status.mockResolvedValue({ playing: false, position: 0, duration: 0 })
  tray.nowPlaying.mockResolvedValue(null)
})
afterEach(() => { if (w) { w.unmount(); w = null } })

async function montar (now = null, audio = null) {
  if (now) tray.nowPlaying.mockResolvedValue(now)
  if (audio) native.status.mockResolvedValue(audio)
  w = mount(MiniPlayer)
  await flushPromises(); await flushPromises()
  return w
}

const botones = (w) => w.findAll('.mini-btn')
const [ANTERIOR, PLAY, SIGUIENTE] = [0, 1, 2]

describe('el mini reproductor', () => {
  it('sin nada cargado no deja pulsar nada y lo dice', async () => {
    const w = await montar()
    expect(w.find('.mini-title').text()).toBe('Nada sonando')
    for (const b of botones(w)) {
      expect(b.attributes('disabled'), 'un boton quedo habilitado sin cancion')
        .toBeDefined()
    }
  })

  it('con una cancion cargada se habilitan los mandos', async () => {
    const w = await montar(SONANDO, { playing: true, position: 30, duration: 200 })
    expect(w.find('.mini-title').text()).toBe('Mi Gozo')
    expect(w.find('.mini-artist').text()).toBe('Barak')
    for (const b of botones(w)) expect(b.attributes('disabled')).toBeUndefined()
  })

  it('sola en la cola, anterior y siguiente siguen en gris', async () => {
    const w = await montar({ ...SONANDO, has_previous: false, has_next: false },
                           { playing: true, position: 1, duration: 100 })
    expect(botones(w)[ANTERIOR].attributes('disabled')).toBeDefined()
    expect(botones(w)[SIGUIENTE].attributes('disabled')).toBeDefined()
    expect(botones(w)[PLAY].attributes('disabled'),
           'sonando se tiene que poder pausar').toBeUndefined()
  })

  it('pausar es una orden directa al audio, sin pasar por la app', async () => {
    const w = await montar(SONANDO, { playing: true, position: 5, duration: 100 })
    await botones(w)[PLAY].trigger('click')
    await flushPromises()
    expect(native.togglePlay).toHaveBeenCalled()
    expect(tray.send).not.toHaveBeenCalled()
  })

  it('cambiar de cancion se le pide a la app, que es quien tiene la cola', async () => {
    const w = await montar(SONANDO, { playing: true, position: 5, duration: 100 })
    await botones(w)[SIGUIENTE].trigger('click')
    await botones(w)[ANTERIOR].trigger('click')
    await flushPromises()
    expect(tray.send).toHaveBeenCalledWith('next')
    expect(tray.send).toHaveBeenCalledWith('previous')
  })

  it('el boton ofrece pausar o reproducir segun este el audio de verdad', async () => {
    // lo que manda es el hilo de audio, no lo que diga la ventana principal
    const sonando = await montar(SONANDO, { playing: true, position: 5, duration: 100 })
    expect(botones(sonando)[PLAY].attributes('title')).toBe('Pausar')
    sonando.unmount()

    w = null
    const parado = await montar({ ...SONANDO, playing: true },
                                { playing: false, position: 5, duration: 100 })
    expect(botones(parado)[PLAY].attributes('title')).toBe('Reproducir')
  })

  it('se entera de que ha cambiado la cancion sin preguntar', async () => {
    const w = await montar()
    expect(w.find('.mini-title').text()).toBe('Nada sonando')
    escuchas.cambios({ ...SONANDO, title: 'Shekinah', artist: '' })
    await flushPromises()
    expect(w.find('.mini-title').text()).toBe('Shekinah')
    expect(w.find('.mini-artist').text()).toBe('Sin artista')
  })

  it('espacio pausa y Escape la quita de en medio', async () => {
    await montar(SONANDO, { playing: true, position: 5, duration: 100 })
    window.dispatchEvent(new KeyboardEvent('keydown', { key: ' ' }))
    await flushPromises()
    expect(native.togglePlay).toHaveBeenCalled()
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }))
    await flushPromises()
    expect(tray.closeMini).toHaveBeenCalled()
  })

  it('la barra no se puede mover si no hay nada sonando', async () => {
    const w = await montar()
    expect(w.find('.mini-seek').classes()).toContain('off')
    await w.find('.mini-seek').trigger('click')
    expect(native.seek).not.toHaveBeenCalled()
  })

  it('la barra avanza con lo que dice el audio, no con lo que creemos', async () => {
    const w = await montar(SONANDO, { playing: true, position: 50, duration: 200 })
    expect(w.find('.mini-seek-fill').attributes('style')).toContain('width: 25%')
    expect(w.find('.mini-times').text()).toContain('0:50')
  })
})
