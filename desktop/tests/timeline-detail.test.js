import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'

// Con las pistas sonando, cada carril llevaba la onda gruesa de cuando se
// separó: al acercar, a bloques, justo al ajustar guías (F12 del plan). Ahora
// el carril pide el trozo de su pista, acotado: solo lo que se ve, solo lo que
// suena, con un poco de espera, de tres en tres y recordando lo ya pedido.
const held = vi.hoisted(() => ({ api: null, state: null, paint: [] }))

vi.mock('../src/api.js', async (importOriginal) => {
  const actual = await importOriginal()
  const { createState, buildApiDouble } = await import('./support/backend.js')
  held.state = createState()
  held.api = buildApiDouble(actual, held.state)
  return { ...actual, api: held.api }
})
// lo que se pinta de cada carril: las columnas que recibe `paintWave`
vi.mock('../src/utils/wavePaint.js', async (importOriginal) => {
  const actual = await importOriginal()
  return {
    ...actual,
    paintWave: (ctx, data, opts) => {
      held.paint.push({ top: opts.top, data })
      return actual.paintWave(ctx, data, opts)
    }
  }
})

import StudyTimeline from '../src/components/StudyTimeline.vue'

const DURATION = 200
/** La onda gruesa de una pista: 800 columnas, todas iguales (para ver lo que cambia). */
const coarse = () => ({
  peaks: Array.from({ length: 800 }, () => 0.5),
  rms: Array.from({ length: 800 }, () => 0.25)
})
const lane = (key, over = {}) => ({
  key,
  name: key,
  on: true,
  mute: false,
  solo: false,
  gain: 1,
  pan: 0,
  wave: coarse(),
  ...over
})
const props = (lanes) => ({
  songId: 7,
  duration: DURATION,
  position: 0,
  lanes,
  laneHeight: 34,
  locked: false
})

const mounted = []
afterEach(() => {
  for (const w of mounted.splice(0)) w.unmount()
  vi.useRealTimers()
})
beforeEach(() => {
  vi.useFakeTimers()
  vi.clearAllMocks()
  held.paint = []
  // las ondas de toda la canción: lo que pide la propia línea de tiempo
  held.api.waveform.mockResolvedValue({
    peaks: [0.5, 1, 0.5, 1],
    rms: [0.2, 0.5, 0.2, 0.5],
    buckets: 4
  })
})

async function mountLanes(lanes, over = {}) {
  const w = mount(StudyTimeline, { props: { ...props(lanes), ...over }, attachTo: document.body })
  mounted.push(w)
  // jsdom no maqueta: la caja de la onda mide 800 px
  Object.defineProperty(w.find('.tl-box').element, 'clientWidth', {
    configurable: true,
    value: 800
  })
  await flushPromises()
  return w
}
/** Pasa lo que se espera antes de pedir y deja contestar. */
async function settled() {
  await vi.advanceTimersByTimeAsync(200)
  await flushPromises()
}
const asked = () =>
  held.api.stemWaveformRange.mock.calls.map(([id, stem, start, end, buckets]) => ({
    id,
    stem,
    start,
    end,
    buckets
  }))

describe('the stems lanes, zoomed in', () => {
  it('ask for the detail of what plays, only around what is visible, once the view stops moving', async () => {
    const w = await mountLanes([lane('drums'), lane('bass'), lane('vocals', { on: false })])
    // de lejos, la gruesa basta: nada que pedir
    await settled()
    expect(asked()).toEqual([])
    // acercada la vista a 25 s alrededor del 100 (a ráfagas, como con la rueda)
    w.vm.zoomBy(2, 100)
    w.vm.zoomBy(2, 100)
    w.vm.zoomBy(2, 100)
    await vi.advanceTimersByTimeAsync(60)
    expect(asked(), 'con la vista aún moviéndose, nada').toEqual([])
    await settled()
    const calls = asked()
    // una sola tanda: el carril callado (vocals) se queda con la gruesa
    expect(calls.map((c) => c.stem).sort()).toEqual(['bass', 'drums'])
    const { view } = w.vm
    for (const c of calls) {
      expect(c.id).toBe(7)
      // el trozo cubre la vista, y no pasa de ella más de media vista por lado
      const span = view.to - view.from
      expect(c.start).toBeLessThanOrEqual(view.from)
      expect(c.end).toBeGreaterThanOrEqual(view.to)
      expect(c.start).toBeGreaterThanOrEqual(view.from - span * 0.5 - span / 2)
      expect(c.end).toBeLessThanOrEqual(view.to + span * 0.5 + span / 2)
      expect(c.buckets).toBeGreaterThanOrEqual(64)
      expect(c.buckets).toBeLessThanOrEqual(4000)
    }
    // los dos, el mismo trozo
    expect(calls[0].start).toBe(calls[1].start)
    expect(calls[0].end).toBe(calls[1].end)
  })

  it('what is painted is the detail, at the scale of the coarse wave', async () => {
    const w = await mountLanes([lane('drums')])
    /** Lo último que se pintó en el carril: sus picos, columna a columna. */
    const painted = () =>
      held.paint
        .filter((p) => p.top === 0)
        .at(-1)
        .data.map(([peak]) => peak)
    w.vm.zoomBy(8, 100)
    await vi.advanceTimersByTimeAsync(10)
    // antes de que llegue, la gruesa: todas las columnas iguales (a bloques)
    expect(new Set(painted()).size).toBe(1)
    await settled()
    // con el detalle (un diente de sierra, en el doble), columnas distintas, y a
    // la escala de la gruesa: sin pasar de lo que ella tenía en ese trozo (0,5)
    const peaks = painted()
    expect(new Set(peaks).size).toBeGreaterThan(2)
    expect(Math.max(...peaks)).toBeCloseTo(0.5, 5)
    expect(Math.min(...peaks)).toBeGreaterThan(0)
  })

  it('a view already seen is not asked for again', async () => {
    const w = await mountLanes([lane('drums'), lane('bass')])
    w.vm.zoomBy(8, 100)
    await settled()
    const first = asked().length
    expect(first).toBe(2)
    // todo, y otra vez lo mismo: la misma vista, el mismo trozo
    w.vm.zoomFit()
    await settled()
    w.vm.zoomBy(8, 100)
    await settled()
    expect(asked()).toHaveLength(first)
  })

  it('a view that has not moved does not ask again for what it already has', async () => {
    const w = await mountLanes([lane('drums')])
    w.vm.zoomBy(8, 100)
    await settled()
    expect(asked()).toHaveLength(1)
    // un poco más allá, dentro de lo ya traído (la mitad de vista de cada lado)
    const { view } = w.vm
    const span = view.to - view.from
    w.vm.view.from += span * 0.1
    w.vm.view.to += span * 0.1
    await settled()
    expect(asked()).toHaveLength(1)
  })

  it('at most a few lanes at a time, and not more than twelve at all', async () => {
    let running = 0
    let most = 0
    held.api.stemWaveformRange.mockImplementation(async (_id, stem, start, end, buckets) => {
      running++
      most = Math.max(most, running)
      await new Promise((r) => setTimeout(r, 30))
      running--
      const peaks = Array.from({ length: buckets }, (_, i) => 0.2 + 0.8 * ((i % 8) / 7))
      return { peaks, rms: peaks, stem }
    })
    const lanes = Array.from({ length: 20 }, (_, i) => lane('t' + i))
    const w = await mountLanes(lanes)
    w.vm.zoomBy(8, 100)
    await vi.advanceTimersByTimeAsync(130)
    for (let i = 0; i < 8; i++) await vi.advanceTimersByTimeAsync(40)
    await flushPromises()
    expect(asked()).toHaveLength(12)
    expect(most).toBe(3)
  })

  it('a new view cuts short the round that was still asking', async () => {
    let release
    const gate = new Promise((r) => (release = r))
    held.api.stemWaveformRange.mockImplementation(async (_id, stem, _s, _e, buckets) => {
      await gate
      return { peaks: Array.from({ length: buckets }, () => 1), rms: [], stem }
    })
    const w = await mountLanes(Array.from({ length: 9 }, (_, i) => lane('t' + i)))
    w.vm.zoomBy(8, 100)
    await vi.advanceTimersByTimeAsync(130)
    expect(asked()).toHaveLength(3) // la primera tanda, esperando
    w.vm.zoomBy(2, 150) // otra vista, con la primera aún sin contestar
    release()
    await flushPromises()
    await settled()
    // la tanda vieja no siguió con las seis que le quedaban (serían 3 + 6 + 9):
    // solo las tres que ya había pedido, y las nueve de la vista nueva
    expect(asked()).toHaveLength(3 + 9)
  })
})

describe('with a core that does not know the stems', () => {
  it('a reply that does not say whose it is is not painted, and it is not asked again', async () => {
    // un núcleo anterior: ignora `stem` y contesta con la mezcla entera
    held.api.stemWaveformRange.mockImplementation(async (_id, _stem, _s, _e, buckets) => ({
      peaks: Array.from({ length: buckets }, (_, i) => (i % 2 ? 1 : 0.9)),
      rms: []
    }))
    const w = await mountLanes([lane('drums'), lane('bass')])
    w.vm.zoomBy(8, 100)
    await settled()
    expect(asked().length).toBeGreaterThan(0)
    held.paint = []
    // la gruesa de siempre
    w.vm.zoomBy(1.5, 100)
    await settled()
    const last = held.paint.filter((p) => p.top === 0).at(-1)
    expect(new Set(last.data.map(([peak]) => peak)).size).toBeLessThanOrEqual(2)
    // y no se insiste: ni en esta vista ni en otras
    const before = asked().length
    w.vm.zoomBy(2, 40)
    await settled()
    expect(asked()).toHaveLength(before)
  })

  it('a failure keeps the coarse wave and does not retry the same piece in a loop', async () => {
    held.api.stemWaveformRange.mockRejectedValue(new Error('no existe'))
    const w = await mountLanes([lane('drums')])
    w.vm.zoomBy(8, 100)
    await settled()
    expect(asked()).toHaveLength(1)
    w.vm.zoomFit()
    await settled()
    w.vm.zoomBy(8, 100)
    await settled()
    // la misma vista: lo que falló quedó apuntado
    expect(asked()).toHaveLength(1)
  })
})

describe('the lanes appear or change', () => {
  it('stems turned on while zoomed in ask for their detail, and new waves forget the old ones', async () => {
    const w = await mountLanes([])
    w.vm.zoomBy(8, 100)
    await settled()
    expect(asked()).toEqual([])
    await w.setProps({ lanes: [lane('drums')] })
    await settled()
    expect(asked().map((c) => c.stem)).toEqual(['drums'])
    // una separación nueva con los mismos nombres: sus ondas son otras
    await w.setProps({
      lanes: [lane('drums', { wave: { peaks: coarse().peaks.slice(0, 600), rms: [] } })]
    })
    await settled()
    expect(asked().map((c) => c.stem)).toEqual(['drums', 'drums'])
    // una pista que se calla no pide más; al volver a sonar, la que ya tenía no se repite
    await w.setProps({
      lanes: [lane('drums', { on: false, wave: { peaks: coarse().peaks.slice(0, 600), rms: [] } })]
    })
    await settled()
    expect(asked()).toHaveLength(2)
  })
})
