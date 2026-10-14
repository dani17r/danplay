import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'

// La app del celular (movil.html → src/movil/): la misma interfaz que la de
// la PC, con su armazón. Las pistas que se trajeron son la biblioteca, se
// estudian con el mismo estudio, y lo que solo se puede en la PC no aparece.
const held = vi.hoisted(() => {
  // lo marca movil.html; platform.js lo lee al cargarse
  document.documentElement.dataset.app = 'movil'
  return { playback: null, api: null, state: null }
})

vi.mock('../src/api.js', async (importOriginal) => {
  const actual = await importOriginal()
  const { createState, buildApiDouble, createPlaybackDouble } = await import('./support/backend.js')
  held.state = createState()
  held.api = buildApiDouble(actual, held.state)
  held.api.inTauri = true
  held.playback = createPlaybackDouble()
  return { ...actual, inTauri: true, api: held.api, playback: held.playback.bridge }
})

import MovilApp from '../src/movil/MovilApp.vue'
import StudyBar from '../src/components/StudyBar.vue'
import DetailsPanel from '../src/components/DetailsPanel.vue'
import { isMobileApp } from '../src/platform.js'
import { resetPlayback } from '../src/composables/usePlayback.js'
import { resetClick } from '../src/composables/useClick.js'
import { useContextMenu, closeMenu } from '../src/composables/useContextMenu.js'
import { useDialog, dialogOk } from '../src/composables/useDialog.js'
import { song, stemsOf } from './support/backend.js'

const { menu } = useContextMenu()
const { dialog } = useDialog()
const labels = () => menu.value.items.filter((i) => !i.separator).map((i) => i.label)

beforeEach(() => {
  vi.clearAllMocks()
  localStorage.clear()
  resetPlayback()
  resetClick()
  held.playback.reset()
  closeMenu()
  held.state.songs = [
    song(1, { title: 'Mi Gozo', artist: 'Barak', has_stems: true }),
    song(2, { title: 'Dios Poderoso', artist: 'Adoración La IBI', has_stems: true })
  ]
  held.state.stems = { 1: stemsOf(1), 2: stemsOf(2) }
})

describe('la app del celular', () => {
  it('sabe que es la del celular', () => {
    expect(isMobileApp).toBe(true)
  })

  it('enseña las pistas traídas y lo que se hace con cada una, sin nada de la PC', async () => {
    const w = mount(MovilApp, { attachTo: document.body })
    await flushPromises()
    expect(w.text()).toContain('Mis pistas')
    expect(w.text()).toContain('Mi Gozo')
    expect(w.text()).toContain('Dios Poderoso')
    expect(w.text()).toContain('Importar')
    // nada de organizar: ni Transformar ni la barra lateral de la biblioteca
    expect(w.text()).not.toContain('Transformar')
    expect(w.find('.sidebar').exists()).toBe(false)
    const row = w.findAll('tr, .row, .card').find((r) => r.text().includes('Mi Gozo'))
    expect(row, 'la fila de Mi Gozo').toBeTruthy()
    await row.trigger('contextmenu')
    expect(labels()).toEqual(['Reproducir', 'Estudiar', 'Letra y acordes', 'Quitar del celular…'])
    // quitarla pregunta antes, y luego ya no está
    const item = menu.value.items.find((i) => i.label === 'Quitar del celular…')
    closeMenu()
    const removing = item.action()
    await flushPromises()
    expect(dialog.value.open).toBe(true)
    expect(dialog.value.message).toContain('En la PC no se toca')
    dialogOk()
    await removing
    await flushPromises()
    expect(held.api.deleteSong).toHaveBeenCalledWith(1)
    expect(w.text()).not.toContain('Mi Gozo')
    w.unmount()
  })

  it('darle a sonar no tapa la lista con la ficha, y «atrás» cierra lo abierto', async () => {
    const w = mount(MovilApp, { attachTo: document.body })
    await flushPromises()
    const row = w.findAll('tr, .row, .card').find((r) => r.text().includes('Mi Gozo'))
    await row.trigger('contextmenu')
    const item = (label) => menu.value.items.find((i) => i.label === label)
    // con el menú abierto, «atrás» lo cierra
    expect(window.__danplayAtras()).toBe(true)
    expect(menu.value.open).toBe(false)
    await row.trigger('contextmenu')
    const estudiar = item('Estudiar')
    closeMenu()
    await estudiar.action()
    await flushPromises()
    expect(held.playback.bridge.setQueue).toHaveBeenCalled()
    expect(w.find('.drawer.open, .drawer-panel.open').exists()).toBe(false)
    expect(w.findComponent(StudyBar).exists()).toBe(true)
    // «atrás»: primero el estudio; luego ya no queda nada (la app se va al fondo)
    expect(window.__danplayAtras()).toBe(true)
    await flushPromises()
    expect(w.findComponent(StudyBar).exists()).toBe(false)
    expect(window.__danplayAtras()).toBe(false)
    w.unmount()
    expect(window.__danplayAtras).toBeUndefined()
  })

  it('sin pistas explica cómo traerlas de la PC', async () => {
    held.state.songs = []
    const w = mount(MovilApp, { attachTo: document.body })
    await flushPromises()
    expect(w.text()).toContain('Todavía no hay pistas en el celular')
    expect(w.text()).toContain('Llevar al celular')
    w.unmount()
  })
})

describe('los componentes de la PC, en el celular', () => {
  it('el menú de las pistas del estudio solo deja volver a la mezcla de fábrica', async () => {
    held.state.songs[0].study = ''
    held.playback.emit({
      track: { id: 1, title: 'Mi Gozo', artist: 'Barak', duration: 200 },
      index: 0,
      length: 1,
      duration: 200,
      path: '/pistas/barak-mi-gozo',
      metronome: { on: false, bpm: 100, meter: 4, shift: 0, mult: 0, volume: 0.8 }
    })
    const w = mount(StudyBar, { attachTo: document.body })
    await flushPromises()
    const more = w.find('.study-stems-more')
    expect(more.exists()).toBe(true)
    await more.trigger('click')
    expect(labels()).toEqual(['Que suenen todas, como vienen'])
    w.unmount()
  })

  it('la ficha enseña la letra y los acordes, sin buscar ni editar', async () => {
    const s = {
      ...song(1, { title: 'Mi Gozo', artist: 'Barak' }),
      lyrics: 'Mi gozo está en ti',
      chords: JSON.stringify({
        sheet: {
          source: 'LaCuerda',
          url: 'https://acordes.lacuerda.net/x',
          artist: 'Barak',
          title: 'Mi Gozo',
          key: 'G',
          sections: [{ name: 'Coro', chords: ['G', 'D'], lines: [{ t: 'G   D', c: true }] }]
        }
      })
    }
    const w = mount(DetailsPanel, { props: { song: s }, attachTo: document.body })
    await flushPromises()
    expect(w.text()).toContain('Mi gozo está en ti')
    expect(w.text()).toContain('LaCuerda')
    for (const gone of [
      'Buscar letra',
      'Buscar portada',
      'Buscar acordes',
      'Ver detalles IA',
      'Proyectar',
      'Buscar otra vez'
    ]) {
      expect(w.text(), gone).not.toContain(gone)
    }
    expect(w.find('[title="Editar la ficha (se guarda en el mp3)"]').exists()).toBe(false)
    w.unmount()
  })
})
