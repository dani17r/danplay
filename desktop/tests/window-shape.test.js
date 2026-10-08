import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
// La ventana en otras formas (pantalla completa, media pantalla, columna,
// cuadrito, barra) y, cuando es tan pequeña que la biblioteca no cabe, el
// reproductor solo. Y en estrecho, lo que no cabe en la barra va en «⋯».
const held = vi.hoisted(() => ({ playback: null, app: null }))
vi.mock('../src/api.js', async (importOriginal) => {
  const actual = await importOriginal()
  const { createPlaybackDouble, createAppDouble } = await import('./support/backend.js')
  held.playback = createPlaybackDouble()
  held.app = createAppDouble()
  return {
    ...actual,
    inTauri: true,
    api: { ...actual.api, inTauri: true },
    playback: held.playback.bridge,
    app: held.app,
    projection: { show: vi.fn(async () => {}) }
  }
})
import CompactPlayer from '../src/components/CompactPlayer.vue'
import Player from '../src/components/Player.vue'
import { resetPlayback, usePlayback } from '../src/composables/usePlayback.js'
import { useContextMenu, closeMenu } from '../src/composables/useContextMenu.js'
import { useWindowShape, SHAPES } from '../src/composables/useWindowShape.js'
import { useViewport, BAR_MAX_H, SQUARE_MAX_W } from '../src/composables/useViewport.js'
import { defineComponent, h } from 'vue'

const PLAYING = {
  track: {
    id: 7,
    title: 'Mi Gozo Es Saber Que Tu Me Amas Y Que Nunca Me Dejaras Solo (En Vivo)',
    artist: 'Barak',
    duration: 200,
    blur: false
  },
  index: 0,
  length: 3,
  playing: true,
  position: 30,
  duration: 200,
  has_previous: true,
  has_next: true,
  error: '',
  has_output: true,
  speed: 1,
  volume: 0.9
}

let w = null
beforeEach(async () => {
  vi.clearAllMocks()
  resetPlayback()
  held.playback.reset()
  held.playback.emit(PLAYING)
  await useWindowShape().setShape('normal')
  closeMenu()
})
afterEach(() => {
  w?.unmount()
  w = null
})

describe('el reproductor solo (cuadrito y barra)', () => {
  it('dice lo que suena, entero al pasar por encima, y se maneja', async () => {
    w = mount(CompactPlayer, { props: { variant: 'cuadro' }, attachTo: document.body })
    await flushPromises()
    const title = w.find('.pocket-title')
    expect(title.text()).toBe(PLAYING.track.title)
    expect(title.attributes('title')).toBe(PLAYING.track.title)
    await w.find('.pocket-play').trigger('click')
    expect(held.playback.bridge.toggle).toHaveBeenCalled()
    await w.find('button[title="Siguiente"]').trigger('click')
    expect(held.playback.bridge.next).toHaveBeenCalled()
    // en el cuadrito estan los saltos de 10 s; en la barra no
    expect(w.find('button[title="Avanzar 10 s"]').exists()).toBe(true)
    w.unmount()
    w = mount(CompactPlayer, { props: { variant: 'barra', roomy: true } })
    expect(w.find('button[title="Avanzar 10 s"]').exists()).toBe(false)
    expect(w.find('.pocket-volume').exists()).toBe(true)
  })

  it('se arrastra desde lo que no es un boton (va sin marco)', () => {
    w = mount(CompactPlayer, { props: { variant: 'barra' } })
    expect(w.find('.pocket').attributes('data-tauri-drag-region')).toBeDefined()
    for (const b of w.findAll('button')) {
      expect(b.attributes('data-tauri-drag-region')).toBeUndefined()
    }
  })

  it('las formas, en un panel que tapa el reproductor: cabe aunque la barra sea baja', async () => {
    w = mount(CompactPlayer, { props: { variant: 'barra' }, attachTo: document.body })
    await w.find('button[aria-label="Forma de la ventana"]').trigger('click')
    const names = w.findAll('.pocket-shape').map((b) => b.text())
    for (const s of SHAPES) expect(names).toContain(s.n)
    expect(names).toContain('Siempre encima')
    // la de ahora, marcada
    expect(w.find('.pocket-shape.on').text()).toBe('Normal')
    await w
      .findAll('.pocket-shape')
      .find((b) => b.text() === 'Cuadrito')
      .trigger('click')
    await flushPromises()
    expect(held.app.setWindowShape).toHaveBeenCalledWith('cuadrito')
    expect(w.find('.pocket-shapes').exists()).toBe(false)
    // Escape cierra el panel sin hacer nada mas
    await w.find('button[aria-label="Forma de la ventana"]').trigger('click')
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }))
    await flushPromises()
    expect(w.find('.pocket-shapes').exists()).toBe(false)
  })

  it('«volver a la ventana normal» la deja como estaba', async () => {
    w = mount(CompactPlayer, { props: { variant: 'cuadro' } })
    await w.find('button[aria-label="Volver a la ventana normal"]').trigger('click')
    await flushPromises()
    expect(held.app.setWindowShape).toHaveBeenLastCalledWith('normal')
  })
})

describe('las formas de la ventana', () => {
  it('el menu marca la de ahora, y siempre encima va aparte', async () => {
    const shape = useWindowShape()
    await shape.setShape('barra')
    const items = shape.menuItems()
    const barra = items.find((i) => i.label === 'Barra')
    expect(barra.icon).toBe('check')
    expect(items.find((i) => i.label === 'Normal').icon).toBe('shapeNormal')
    const pin = items.find((i) => i.label === 'Siempre encima')
    await pin.action()
    expect(held.app.setWindowOnTop).toHaveBeenCalledWith(true)
    expect(shape.menuItems().find((i) => i.icon === 'pin').label).toBe('Quitar «siempre encima»')
    await shape.toggleOnTop()
  })

  it('F11: pantalla completa, y otra vez, como estaba', async () => {
    const shape = useWindowShape()
    await shape.toggleFullscreen()
    expect(held.app.setWindowShape).toHaveBeenLastCalledWith('completa')
    await shape.toggleFullscreen()
    expect(held.app.setWindowShape).toHaveBeenLastCalledWith('normal')
  })
})

describe('en estrecho, «⋯» lleva lo que no cabe en la barra', () => {
  it('velocidad, volumen, estudio, cola y forma de la ventana', async () => {
    w = mount(Player, { attachTo: document.body })
    await flushPromises()
    await w.find('.pl-more').trigger('click')
    const { menu } = useContextMenu()
    expect(menu.value.open).toBe(true)
    const labels = menu.value.items.filter((i) => i.label).map((i) => i.label)
    for (const l of [
      'Velocidad',
      'Volumen',
      'Modo estudio',
      'Cola de reproducción',
      'Forma de la ventana'
    ]) {
      expect(labels).toContain(l)
    }
    const speed = menu.value.items.find((i) => i.label === 'Velocidad')
    await speed.children.find((c) => c.label === '0.75×').action()
    await flushPromises()
    const { speed: now, volume: vol } = usePlayback()
    expect(now.value).toBe(0.75)
    const volume = menu.value.items.find((i) => i.label === 'Volumen')
    await volume.children.find((c) => c.label === '50 %').action()
    await flushPromises()
    expect(vol.value).toBe(0.5)
    const shapes = menu.value.items.find((i) => i.label === 'Forma de la ventana')
    expect(shapes.children.map((c) => c.label)).toContain('Columna')
  })
})

describe('el tamaño decide el diseño', () => {
  const Probe = defineComponent({
    setup() {
      const v = useViewport()
      return () => h('i', v.pocket.value || 'biblioteca')
    }
  })
  async function at(width, height) {
    window.innerWidth = width
    window.innerHeight = height
    window.dispatchEvent(new Event('resize'))
    await flushPromises()
  }
  it('barra si es muy baja, cuadrito si es pequeña por los dos lados', async () => {
    w = mount(Probe)
    await at(1280, 800)
    expect(w.text()).toBe('biblioteca')
    await at(656, BAR_MAX_H)
    expect(w.text()).toBe('barra')
    await at(SQUARE_MAX_W, 400)
    expect(w.text()).toBe('cuadro')
    // estrecha pero alta (la columna): la biblioteca, con el diseño de movil
    await at(336, 800)
    expect(w.text()).toBe('biblioteca')
    await at(1280, 800)
  })
})
