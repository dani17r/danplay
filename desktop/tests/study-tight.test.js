import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'

// En una ventana baja (menos de 800 px) el panel de estudio aprieta sus
// espacios para que las tarjetas del tono y del clic quepan enteras
// (`.study.tight`, ver study.css y studyFit.js). En la de verdad, 1536×864,
// nada cambia.
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

import StudyBar from '../src/components/StudyBar.vue'
import { resetPlayback } from '../src/composables/usePlayback.js'
import { resetClick } from '../src/composables/useClick.js'
import { resetRouting } from '../src/composables/useRouting.js'
import { song } from './support/backend.js'

const realHeight = Object.getOwnPropertyDescriptor(window, 'innerHeight')
const setHeight = (value) => {
  Object.defineProperty(window, 'innerHeight', { configurable: true, value })
  window.dispatchEvent(new Event('resize'))
}
const mounted = []
beforeEach(() => {
  vi.clearAllMocks()
  localStorage.clear()
  resetPlayback()
  resetClick()
  resetRouting()
  held.playback.reset()
  held.state.songs = [song(7, { title: 'Mi Gozo', study: '' })]
  held.playback.emit({
    track: { id: 7, title: 'Mi Gozo', artist: 'Barak', duration: 200 },
    playing: false,
    position: 0,
    path: '/musica/mi-gozo.mp3'
  })
})
afterEach(() => {
  for (const w of mounted.splice(0)) w.unmount()
  if (realHeight) Object.defineProperty(window, 'innerHeight', realHeight)
})
async function open(height) {
  Object.defineProperty(window, 'innerHeight', { configurable: true, value: height })
  const w = mount(StudyBar, { attachTo: document.body })
  mounted.push(w)
  await flushPromises()
  return w
}

describe('the study panel in a low window', () => {
  it('goes tight below 800 px of window height, and not from there up', async () => {
    const w = await open(768) // 1366×768
    expect(w.find('.study').classes()).toContain('tight')
    // 1536×864 (la ventana de verdad): como siempre
    setHeight(864)
    await flushPromises()
    expect(w.find('.study').classes()).not.toContain('tight')
    setHeight(800)
    await flushPromises()
    expect(w.find('.study').classes()).not.toContain('tight')
    // y encogiendo la ventana a mano, sin recargar
    setHeight(700)
    await flushPromises()
    expect(w.find('.study').classes()).toContain('tight')
  })

  it('«Grande» already has all the height: it is not tight there', async () => {
    const w = await open(768)
    expect(w.find('.study').classes()).toContain('tight')
    await w.find('.study-size').trigger('click')
    expect(w.find('.study').classes()).toContain('big')
    expect(w.find('.study').classes()).not.toContain('tight')
  })

  it('and the window of the person who asked is untouched', async () => {
    const w = await open(864)
    expect(w.find('.study').classes()).toEqual(expect.arrayContaining(['desk']))
    expect(w.find('.study').classes()).not.toContain('tight')
  })
})
