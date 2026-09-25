import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import { settle } from './support/pages.js'

// Mientras suena algo, Rust cuenta el estado cuatro veces por segundo, y cada
// vez con objetos nuevos aunque digan lo mismo. Eso despertaba a toda la app:
// la barra lateral se repintaba en cada tick y, con el chat abierto, se
// volvia a pasar todo el markdown de la conversacion (20 ticks con 15
// mensajes eran 300 conversiones). Lo que tiene que moverse es la aguja.
const held = vi.hoisted(() => ({ state: null, api: null, playback: null }))

vi.mock('../src/api.js', async (importOriginal) => {
  const actual = await importOriginal()
  const { createState, buildApiDouble, createPlaybackDouble, createAppDouble } =
    await import('./support/backend.js')
  const { vi: v } = await import('vitest')
  held.state = createState()
  held.api = buildApiDouble(actual, held.state)
  held.api.inTauri = true
  held.playback = createPlaybackDouble()
  return {
    ...actual,
    inTauri: true,
    api: held.api,
    playback: held.playback.bridge,
    app: createAppDouble(),
    core: { onStatus: v.fn(async () => () => {}), onChanged: v.fn(async () => () => {}) }
  }
})
vi.mock('../src/utils/markdown.js', async (importOriginal) => {
  const actual = await importOriginal()
  const { vi: v } = await import('vitest')
  return { ...actual, renderMarkdown: v.fn(actual.renderMarkdown) }
})

import App from '../src/App.vue'
import { renderMarkdown } from '../src/utils/markdown.js'
import { resetPlayback } from '../src/composables/usePlayback.js'
import { resetPreferences } from '../src/composables/usePreferences.js'
import { resetChat } from '../src/composables/useChat.js'
import { song } from './support/backend.js'

beforeEach(() => {
  resetPlayback()
  resetPreferences()
  resetChat()
  held.playback.reset()
  localStorage.clear()
  localStorage.setItem('danplay.default-player-asked', '1')
  vi.clearAllMocks()
})

/** Monta la app contando cuantas veces se repinta cada componente. */
async function montar(counts) {
  const w = mount(App, {
    attachTo: document.body,
    global: {
      mixins: [
        {
          updated() {
            const n = this.$.type.__name || '?'
            counts[n] = (counts[n] || 0) + 1
          }
        }
      ]
    }
  })
  await flushPromises()
  await flushPromises()
  return w
}

describe('lo que suena y lo que se repinta', () => {
  it('cada tick mueve la aguja y nada mas', async () => {
    held.state.status.configured = true
    held.state.songs = [song(1, { title: 'Mi Gozo' }), song(2)]
    held.state.playlists = [{ id: 7, name: 'Domingo', n: 2 }]
    held.state.chats = [
      {
        id: 1,
        title: 'Set del domingo',
        updated: 1,
        messages: Array.from({ length: 15 }, (_, i) =>
          i % 2
            ? { role: 'ai', text: `Te propongo:\n\n1. **Mi Gozo** – Barak\n2. *Shekinah* (${i})` }
            : { role: 'me', text: `pregunta ${i}` }
        )
      }
    ]
    const counts = {}
    const w = await montar(counts)
    await w
      .findAll('.nav-link')
      .find((b) => b.text().includes('Asistente'))
      .trigger('click')
    await settle()
    expect(w.findAll('.chat-msg')).toHaveLength(15)
    // suena una cancion de la lista
    await w
      .findAll('.nav-link')
      .find((b) => b.text().includes('Todas'))
      .trigger('click')
    await flushPromises()
    await w.findAll('tbody tr')[0].find('.row-play').trigger('click')
    await flushPromises()
    await w
      .findAll('.nav-link')
      .find((b) => b.text().includes('Asistente'))
      .trigger('click')
    await settle()

    for (const k of Object.keys(counts)) delete counts[k]
    renderMarkdown.mockClear()
    for (let i = 1; i <= 20; i++) {
      held.playback.emit({ position: i * 0.25 })
      await flushPromises()
    }
    expect(renderMarkdown, 'el chat vuelve a pasar el markdown en cada tick').not.toHaveBeenCalled()
    expect(counts.App || 0, 'la app se repinta en cada tick').toBe(0)
    expect(counts.Sidebar || 0, 'la barra lateral se repinta en cada tick').toBe(0)
    expect(counts.ChatPage || 0, 'el chat se repinta en cada tick').toBe(0)
    // la aguja si se mueve
    expect(counts.Player).toBeGreaterThan(0)
    expect(w.find('.pl-bar .time').text()).toBe('0:05')
  })

  it('cuando cambia algo de verdad, se entera quien lo enseña', async () => {
    held.state.status.configured = true
    held.state.songs = [song(1, { title: 'Mi Gozo' }), song(2, { title: 'Shekinah' })]
    const w = await montar({})
    await w.findAll('tbody tr')[0].find('.row-play').trigger('click')
    await flushPromises()
    expect(w.find('.player .title').text()).toBe('Mi Gozo')
    const dot = () =>
      w
        .findAll('.nav-link')
        .find((b) => b.text().includes('Todas'))
        .find('.now-dot')
    expect(dot().classes()).not.toContain('paused')
    held.playback.emit({ playing: false })
    await flushPromises()
    expect(dot().classes()).toContain('paused')
    held.playback.emit({
      track: { id: 2, title: 'Shekinah', artist: 'Barak', duration: 200, blur: false }
    })
    await flushPromises()
    expect(w.find('.player .title').text()).toBe('Shekinah')
  })
})
