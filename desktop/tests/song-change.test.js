import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'

// Al cambiar de canción, Rust cuenta primero la canción (`track`, de la cola)
// y la ruta de su archivo (`path`, del audio) después, en otro estado. El doble
// de las pruebas las entregaba juntas y por eso no veía lo que pasa entre los
// dos (E1 del plan): lo que la interfaz manda con la ruta de la anterior, el
// motor lo descarta. Aquí se prueba que el doble se comporta como Rust, y que
// el estudio acaba mandando lo de la canción nueva con SU ruta.
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
import { resetPlayback, usePlayback } from '../src/composables/usePlayback.js'
import { resetClick } from '../src/composables/useClick.js'
import { resetRouting } from '../src/composables/useRouting.js'
import { song, stemsOf } from './support/backend.js'

const bridge = () => held.playback.bridge
const A = song(7, { title: 'Mi Gozo', study: '' })
const COROS = [{ t: 12, file: '/guias/coro.wav', text: 'Coro' }]
const B = song(8, { title: 'Otra', study: JSON.stringify({ cues: COROS }) })

beforeEach(() => {
  vi.clearAllMocks()
  localStorage.clear()
  resetPlayback()
  resetClick()
  resetRouting()
  held.playback.reset()
  held.state.songs = [A, B]
})
afterEach(() => held.playback.reset())

/** Los estados que va contando el doble, como los recibe la interfaz. */
async function recording() {
  const seen = []
  await bridge().onState((s) => seen.push({ id: s.track?.id ?? null, path: s.path }))
  return seen
}

describe('the playback double tells the song before its path, as Rust does', () => {
  it('a patch that changes both comes out as two states: the song first, then its path', async () => {
    const seen = await recording()
    held.playback.emit({ track: { id: 7, title: 'Mi Gozo' }, path: '/musica/mi-gozo.mp3' })
    // en el primer estado la canción ya es la nueva y la ruta aún no
    expect(seen).toEqual([{ id: 7, path: '' }])
    await flushPromises()
    expect(seen).toEqual([
      { id: 7, path: '' },
      { id: 7, path: '/musica/mi-gozo.mp3' }
    ])
    // de una canción a otra: la nueva, con la ruta de la anterior, y luego la suya
    held.playback.emit({ track: { id: 8, title: 'Otra' }, path: '/musica/otra.mp3' })
    expect(seen.at(-1)).toEqual({ id: 8, path: '/musica/mi-gozo.mp3' })
    await flushPromises()
    expect(seen.at(-1)).toEqual({ id: 8, path: '/musica/otra.mp3' })
    // lo que no cambia de canción va tal cual, de una vez
    held.playback.emit({ path: '/musica/otra-version.mp3', position: 3 })
    expect(seen.at(-1)).toEqual({ id: 8, path: '/musica/otra-version.mp3' })
  })

  it('the queue does the same: choosing a song announces it with the path of the one before', async () => {
    await usePlayback().setQueue([A, B], 7)
    await flushPromises()
    const seen = await recording()
    await usePlayback().setQueue([A, B], 8)
    await flushPromises()
    expect(seen[0]).toEqual({ id: 8, path: '/musica/cancion-7.mp3' })
    expect(seen.at(-1)).toEqual({ id: 8, path: '/musica/cancion-8.mp3' })
  })

  it('the path can take longer than the song, and then it arrives', async () => {
    const seen = await recording()
    held.playback.holdPaths()
    held.playback.emit({ track: { id: 7, title: 'Mi Gozo' }, path: '/musica/mi-gozo.mp3' })
    await flushPromises()
    expect(seen.at(-1)).toEqual({ id: 7, path: '' })
    held.playback.releasePaths()
    expect(seen.at(-1)).toEqual({ id: 7, path: '/musica/mi-gozo.mp3' })
  })
})

describe('changing song with the study open', () => {
  const mounted = []
  afterEach(() => {
    for (const w of mounted.splice(0)) w.unmount()
  })
  const mountView = async () => {
    const w = mount(StudyBar, { attachTo: document.body })
    mounted.push(w)
    await flushPromises()
    await flushPromises()
    return w
  }
  const coro = () => [expect.objectContaining({ at: 12, path: '/guias/coro.wav' })]

  it('the cues of the new song reach the engine even if its path arrives after the study loaded', async () => {
    // abierta la 7, sonando
    await usePlayback().setQueue([A, B], 7)
    await flushPromises()
    await mountView()
    // la cola elige la 8, y su ruta tarda más que el estudio de la 8 en cargarse
    held.playback.holdPaths()
    await usePlayback().setQueue([A, B], 8)
    await flushPromises()
    await flushPromises()
    // el motor ya es el de la 8, y lo que la interfaz mandó con la ruta
    // publicada (aún la de la 7) lo descartó: no hay guías
    expect(held.playback.engine.path).toBe('/musica/cancion-8.mp3')
    expect(held.playback.engine.cues).toBeNull()
    // llega su ruta: la interfaz las manda otra vez, ahora a su canción
    held.playback.releasePaths()
    await flushPromises()
    await flushPromises()
    expect(held.playback.engine.cues).toEqual({
      song: '/musica/cancion-8.mp3',
      cues: coro(),
      volume: expect.any(Number)
    })
  })

  it('and when the path arrives first, the cues also go to their song', async () => {
    await usePlayback().setQueue([A, B], 7)
    await flushPromises()
    await mountView()
    await usePlayback().setQueue([A, B], 8)
    await flushPromises()
    await flushPromises()
    expect(held.playback.engine.cues).toEqual({
      song: '/musica/cancion-8.mp3',
      cues: coro(),
      volume: expect.any(Number)
    })
  })

  it('the mix of the stems of the new song also waits for its path', async () => {
    held.state.stems[8] = stemsOf(8)
    held.state.songs = [
      A,
      { ...B, has_stems: true, study: JSON.stringify({ mixer: { on: true } }) }
    ]
    await usePlayback().setQueue([A, B], 7)
    await flushPromises()
    await mountView()
    held.playback.holdPaths()
    await usePlayback().setQueue([A, B], 8)
    await flushPromises()
    await flushPromises()
    // con la ruta de la 7 publicada, la mezcla de la 8 se descarta
    expect(held.playback.engine.stems).toBeNull()
    held.playback.releasePaths()
    await flushPromises()
    await flushPromises()
    expect(held.playback.engine.stems?.song).toBe('/musica/cancion-8.mp3')
    expect(held.playback.engine.stems?.tracks?.length).toBeGreaterThan(1)
  })
})
