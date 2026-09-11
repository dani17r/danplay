import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { mount } from '@vue/test-utils'

const { api } = vi.hoisted(() => ({
  api: { youtube: vi.fn(async () => ({ available: true, active: false, results: [] })) }
}))
vi.mock('../src/api.js', () => ({ api, inTauri: false }))
import { useDownloads, resetDownloads } from '../src/composables/useDownloads.js'
import Sidebar from '../src/components/Sidebar.vue'

beforeEach(() => { vi.clearAllMocks(); resetDownloads(); vi.useFakeTimers() })
afterEach(() => vi.useRealTimers())

// Un solo sitio consulta al nucleo lo que se esta bajando, y de ahi leen la
// pagina de Descargas, el chat y la barra lateral. Sin sondeo de fondo: se
// despierta cuando alguien arranca una descarga y se duerme al terminar.
describe('useDownloads', () => {
  it('despierta, sigue mientras baja, avisa al terminar y se duerme', async () => {
    const d = useDownloads()
    const done = vi.fn()
    d.onFinished(done)
    api.youtube
      .mockResolvedValueOnce({ active: true, phase: 'downloading', index: 1, total: 2, percent: 10 })
      .mockResolvedValueOnce({ active: true, phase: 'filing', index: 2, total: 2, percent: 100 })
      .mockResolvedValueOnce({ active: false, phase: 'done', results: [{ ok: true, id: 9 }] })
    d.wake()
    await vi.advanceTimersByTimeAsync(10)
    expect(d.state.active).toBe(true)
    expect(d.active.value).toBe(true)
    expect(d.state.total).toBe(2)

    await vi.advanceTimersByTimeAsync(1500)
    expect(d.state.active).toBe(false)
    expect(done).toHaveBeenCalledTimes(1)
    expect(done.mock.calls[0][0].results).toEqual([{ ok: true, id: 9 }])

    // dormido: no sigue preguntando
    const calls = api.youtube.mock.calls.length
    await vi.advanceTimersByTimeAsync(10000)
    expect(api.youtube.mock.calls.length).toBe(calls)
  })

  it('si termino entre dos miradas, se cuenta igual', async () => {
    const d = useDownloads()
    const done = vi.fn()
    d.onFinished(done)
    api.youtube.mockResolvedValueOnce({ active: false, phase: 'done', results: [{ ok: false, reason: 'x' }] })
    d.wake()
    await vi.advanceTimersByTimeAsync(10)
    expect(done).toHaveBeenCalledTimes(1)
  })

  it('una consulta suelta con algo en marcha sigue sola', async () => {
    const d = useDownloads()
    api.youtube
      .mockResolvedValueOnce({ active: true, phase: 'downloading', index: 1, total: 1 })
      .mockResolvedValueOnce({ active: false, phase: 'done', results: [] })
    await d.refresh()
    expect(d.state.active).toBe(true)
    await vi.advanceTimersByTimeAsync(1000)
    expect(d.state.active).toBe(false)
  })

  it('si el nucleo no contesta, insiste un poco y luego para', async () => {
    const d = useDownloads()
    api.youtube.mockRejectedValue(new Error('sin nucleo'))
    d.wake()
    await vi.advanceTimersByTimeAsync(10000)
    expect(api.youtube.mock.calls.length).toBeLessThanOrEqual(6)
    expect(d.state.known).toBe(false)
  })

  it('desapuntarse funciona', async () => {
    const d = useDownloads()
    const done = vi.fn()
    const stop = d.onFinished(done)
    stop()
    api.youtube.mockResolvedValueOnce({ active: false, phase: 'done', results: [] })
    d.wake()
    await vi.advanceTimersByTimeAsync(10)
    expect(done).not.toHaveBeenCalled()
  })
})

const montar = (props = {}) =>
  mount(Sidebar, { props: { view: { kind: 'all' }, playlists: [], stats: { total: 3 }, entrada: 0, ...props },
                   global: { stubs: { TextField: true } } })

describe('la barra lateral cuenta lo que pasa', () => {
  it('mientras baja algo, «Descargas» lleva el numero; si no, nada', () => {
    const w = montar({ download: { active: true, index: 2, total: 3, phase: 'downloading', name: 'X', percent: 40 } })
    const link = w.findAll('.nav-link').find(b => b.text().includes('Descargas'))
    expect(link.find('.count.live').text()).toBe('2/3')
    expect(link.attributes('title')).toContain('Bajando')
    expect(link.attributes('title')).toContain('X')

    const quiet = montar({ download: { active: false } })
    const link2 = quiet.findAll('.nav-link').find(b => b.text().includes('Descargas'))
    expect(link2.find('.count.live').exists()).toBe(false)
  })

  it('con una sola cancion el numero es 1', () => {
    const w = montar({ download: { active: true, index: 1, total: 1, phase: 'filing' } })
    const link = w.findAll('.nav-link').find(b => b.text().includes('Descargas'))
    expect(link.find('.count.live').text()).toBe('1')
  })

  it('la lista de la que sale lo que suena lleva un punto', () => {
    const playlists = [{ id: 1, name: 'domingo', n: 2 }, { id: 2, name: 'Herlin', n: 3 }]
    const w = montar({ playlists, nowPlaying: { kind: 'playlist', id: 2, playing: true } })
    const rows = w.findAll('.nav-playlist')
    expect(rows[0].find('.now-dot').exists()).toBe(false)
    expect(rows[1].find('.now-dot').exists()).toBe(true)
    expect(rows[1].find('.now-dot').classes()).not.toContain('paused')
    expect(rows[1].find('.now-dot').attributes('title')).toBe('Sonando ahora')
  })

  it('en pausa el punto se queda quieto; sin nada sonando no hay punto', () => {
    const w = montar({ nowPlaying: { kind: 'all', id: null, playing: false } })
    const all = w.findAll('.nav-link').find(b => b.text().includes('Todas las canciones'))
    expect(all.find('.now-dot').classes()).toContain('paused')
    expect(all.find('.now-dot').attributes('title')).toBe('En pausa')
    const none = montar({ nowPlaying: null })
    expect(none.find('.now-dot').exists()).toBe(false)
  })
})
