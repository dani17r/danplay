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
import DetailsPanel from '../src/components/DetailsPanel.vue'
import StudyBar from '../src/components/StudyBar.vue'
import StudyTimeline from '../src/components/StudyTimeline.vue'
import StudyNotes from '../src/components/StudyNotes.vue'
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

/** Cuenta cuántas veces se repinta cada componente (por su nombre). */
const counting = (counts) => ({
  updated() {
    const n = this.$.type.__name || '?'
    counts[n] = (counts[n] || 0) + 1
  }
})

/** Monta la app contando cuantas veces se repinta cada componente. */
async function mountView(counts) {
  const w = mount(App, {
    attachTo: document.body,
    global: { mixins: [counting(counts)] }
  })
  await flushPromises()
  await flushPromises()
  return w
}

describe('what plays and what gets repainted', () => {
  it('each tick moves the playhead and nothing else', async () => {
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
    const w = await mountView(counts)
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

  it('when something really changes, whoever displays it finds out', async () => {
    held.state.status.configured = true
    held.state.songs = [song(1, { title: 'Mi Gozo' }), song(2, { title: 'Shekinah' })]
    const w = await mountView({})
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

  it('the details panel with synced lyrics: the lyrics follow, the panel is not repainted', async () => {
    const lrc = '[00:01.00]Mi gozo\n[00:03.00]es el Señor\n[00:04.50]coro'
    held.playback.emit({
      track: { id: 1, title: 'Mi Gozo', artist: 'Barak', duration: 200, blur: false },
      playing: true,
      position: 0
    })
    await flushPromises()
    const counts = {}
    const w = mount(DetailsPanel, {
      attachTo: document.body,
      props: { song: song(1, { title: 'Mi Gozo', lyrics_synced: lrc }), aiReady: false },
      global: { mixins: [counting(counts)] }
    })
    await flushPromises()
    for (const k of Object.keys(counts)) delete counts[k]
    for (let i = 1; i <= 20; i++) {
      held.playback.emit({ position: i * 0.25 })
      await flushPromises()
    }
    expect(counts.DetailsPanel || 0, 'la ficha entera se repinta en cada tick').toBe(0)
    // la letra sí sigue a la canción: a los 5 s va por el coro
    expect(w.find('.lrc-line.current').text()).toBe('coro')
    expect(counts.SyncedLyrics).toBeGreaterThan(0)
    w.unmount()
  })

  it('with the study open, StudyBar and its timeline are not repainted: the clock and the playhead move', async () => {
    vi.useFakeTimers({ toFake: ['requestAnimationFrame', 'cancelAnimationFrame'] })
    try {
      held.state.songs = [song(7, { title: 'Mi Gozo', study: '' })]
      held.playback.emit({
        track: { id: 7, title: 'Mi Gozo', artist: 'Barak', duration: 200, blur: false },
        playing: true,
        position: 10,
        duration: 200,
        path: '/musica/cancion-7.mp3'
      })
      const counts = {}
      const w = mount(StudyBar, { attachTo: document.body, global: { mixins: [counting(counts)] } })
      await flushPromises()
      await flushPromises()
      vi.advanceTimersByTime(20)
      const bar = w.vm.$.subTree
      const timeline = w.findComponent(StudyTimeline).vm.$.subTree
      // y las partes en que se partió StudyBar (lote 9): tampoco
      const parts = [StudyNotes]
      const partTree = (C) => w.findComponent(C).vm.$.subTree
      const partsBefore = parts.map(partTree)
      const head = () => w.find('.tl-head').element.style.left
      const before = head()
      for (const k of Object.keys(counts)) delete counts[k]
      for (let i = 1; i <= 20; i++) {
        held.playback.emit({ position: 10 + i * 0.25 })
        await flushPromises()
        vi.advanceTimersByTime(16)
      }
      expect(w.vm.$.subTree, 'StudyBar se repinta en cada aviso').toBe(bar)
      expect(w.findComponent(StudyTimeline).vm.$.subTree, 'la línea de tiempo también').toBe(
        timeline
      )
      expect(counts.StudyBar || 0).toBe(0)
      expect(counts.StudyTimeline || 0).toBe(0)
      expect(counts.BeatTools || 0).toBe(0)
      parts.forEach((C, i) => {
        expect(partTree(C), `${C.__name} se repinta en cada aviso`).toBe(partsBefore[i])
      })
      // lo que tiene que moverse, se mueve
      expect(w.find('.study-time').text()).toBe('0:15 / 3:20')
      expect(head()).not.toBe(before)
      // a los 15 s (más lo poco que la adelanta el fotograma: 0,3 s como mucho)
      expect(parseFloat(head())).toBeGreaterThanOrEqual(7.5)
      expect(parseFloat(head())).toBeLessThanOrEqual(7.65)
      w.unmount()
    } finally {
      vi.useRealTimers()
    }
  })

  it('the playhead only touches the page when it changes pixel', async () => {
    vi.useFakeTimers({ toFake: ['requestAnimationFrame', 'cancelAnimationFrame'] })
    try {
      held.state.songs = [song(7, { title: 'Mi Gozo', study: '' })]
      held.playback.emit({
        track: { id: 7, title: 'Mi Gozo', artist: 'Barak', duration: 200, blur: false },
        playing: false,
        position: 10,
        duration: 200,
        path: '/musica/cancion-7.mp3'
      })
      const w = mount(StudyBar, { attachTo: document.body })
      await flushPromises()
      // 200 s en 200 px: un píxel por segundo
      const box = w.find('.tl-box').element
      Object.defineProperty(box, 'clientWidth', { configurable: true, value: 200 })
      const head = w.find('.tl-head').element
      held.playback.emit({ position: 50 })
      await flushPromises()
      vi.advanceTimersByTime(20)
      const at50 = head.style.left
      expect(parseFloat(at50)).toBeCloseTo(25, 5)
      // un cuarto de segundo: el mismo píxel, no se toca
      held.playback.emit({ position: 50.25 })
      await flushPromises()
      vi.advanceTimersByTime(20)
      expect(head.style.left).toBe(at50)
      // un segundo: otro píxel
      held.playback.emit({ position: 51 })
      await flushPromises()
      vi.advanceTimersByTime(20)
      expect(parseFloat(head.style.left)).toBeCloseTo(25.5, 5)
      w.unmount()
    } finally {
      vi.useRealTimers()
    }
  })
})
