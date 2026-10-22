import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import { nextTick } from 'vue'

// La app del celular (mobile.html → src/mobile/): su propio armazón para los
// dedos (la lista táctil, el reproductor del celular, el menú de lado) y los
// componentes de la PC donde valen tal cual (el estudio, la ficha). Lo que
// solo se puede en la PC no aparece.
const held = vi.hoisted(() => {
  // lo marca mobile.html; platform.js lo lee al cargarse
  document.documentElement.dataset.app = 'mobile'
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

import MobileApp from '../src/mobile/MobileApp.vue'
import MobileSongs from '../src/mobile/MobileSongs.vue'
import MobilePlayer from '../src/mobile/MobilePlayer.vue'
import StudyBar from '../src/components/StudyBar.vue'
import StudyTimeline from '../src/components/StudyTimeline.vue'
import DetailsPanel from '../src/components/DetailsPanel.vue'
import { isMobileApp } from '../src/platform.js'
import { resetPlayback } from '../src/composables/usePlayback.js'
import { resetClick } from '../src/composables/useClick.js'
import { useContextMenu, closeMenu } from '../src/composables/useContextMenu.js'
import { useDialog, dialogOk } from '../src/composables/useDialog.js'
import { tapPlays } from '../src/mobile/prefs.js'
import { song, stemsOf } from './support/backend.js'

const { menu } = useContextMenu()
const { dialog } = useDialog()
const labels = () => menu.value.items.filter((i) => !i.separator).map((i) => i.label)
const rowOf = (w, title) => w.findAll('.m-song').find((r) => r.text().includes(title))
// jsdom no trae PointerEvent y test-utils no deja poner coordenadas: un
// MouseEvent con ese nombre lleva las dos cosas
async function pointer(wrapper, type, x = 0, y = 0) {
  wrapper.element.dispatchEvent(
    new MouseEvent(type, { bubbles: true, cancelable: true, clientX: x, clientY: y })
  )
  await nextTick()
}

beforeEach(() => {
  vi.clearAllMocks()
  localStorage.clear()
  resetPlayback()
  resetClick()
  held.playback.reset()
  closeMenu()
  tapPlays.value = false
  held.state.songs = [
    song(1, { title: 'Mi Gozo', artist: 'Barak', has_stems: true, duration: 245 }),
    song(2, { title: 'Dios Poderoso', artist: 'Adoración La IBI', has_stems: true })
  ]
  held.state.stems = { 1: stemsOf(1), 2: stemsOf(2) }
  held.state.playlists = []
  held.state.playlistSongs = []
})
afterEach(() => {
  delete window.DanPlayAndroid
  document.body.innerHTML = ''
})

describe('the phone app', () => {
  it('knows it is the phone one', () => {
    expect(isMobileApp).toBe(true)
  })

  it('shows the imported tracks and what can be done with each, with nothing from the desktop app', async () => {
    const w = mount(MobileApp, { attachTo: document.body })
    await flushPromises()
    expect(w.find('.m-top').text()).toContain('Mis pistas')
    expect(w.text()).toContain('Mi Gozo')
    expect(w.text()).toContain('Dios Poderoso')
    expect(w.find('[aria-label^="Importar pistas"]').exists()).toBe(true)
    // nada de organizar: ni Transformar ni la barra lateral de la biblioteca
    expect(w.text()).not.toContain('Transformar')
    expect(w.find('.sidebar').exists()).toBe(false)
    await rowOf(w, 'Mi Gozo').find('.m-song-more').trigger('click')
    expect(labels()).toEqual([
      'Reproducir',
      'Estudiar',
      'Letra y acordes',
      'Añadir a una lista',
      'Marcar como favorita',
      'Quitar del celular…'
    ])
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

  it('a tap selects the song and shows its buttons; a double tap plays it', async () => {
    const w = mount(MobileApp, { attachTo: document.body })
    await flushPromises()
    await rowOf(w, 'Mi Gozo').trigger('click')
    expect(held.playback.bridge.setQueue).not.toHaveBeenCalled()
    const actions = w.find('.m-song-actions')
    expect(actions.exists()).toBe(true)
    expect(actions.text()).toContain('Reproducir')
    expect(actions.text()).toContain('Estudiar')
    // el segundo toque, enseguida: suena
    await rowOf(w, 'Mi Gozo').trigger('click')
    await flushPromises()
    expect(held.playback.bridge.setQueue).toHaveBeenCalledTimes(1)
    const [items, start, origin] = held.playback.bridge.setQueue.mock.calls[0]
    expect(items.map((t) => t.id)).toEqual([1, 2])
    expect(start).toBe(1)
    expect(origin).toMatchObject({ kind: 'all', label: 'Mis pistas' })
    expect(w.find('.m-player').text()).toContain('Mi Gozo')
    w.unmount()
  })

  it('with «Un toque pone la canción», one tap is enough; and the cover always plays it', async () => {
    tapPlays.value = true
    const w = mount(MobileApp, { attachTo: document.body })
    await flushPromises()
    await rowOf(w, 'Dios Poderoso').trigger('click')
    await flushPromises()
    expect(held.playback.bridge.setQueue).toHaveBeenCalledTimes(1)
    tapPlays.value = false
    await rowOf(w, 'Mi Gozo').find('.m-song-cover').trigger('click')
    await flushPromises()
    expect(held.playback.bridge.setQueue).toHaveBeenCalledTimes(2)
    expect(held.playback.bridge.setQueue.mock.calls[1][1]).toBe(1)
    w.unmount()
  })

  it('long-pressing a song opens its options', async () => {
    vi.useFakeTimers()
    try {
      const w = mount(MobileApp, { attachTo: document.body })
      await vi.runOnlyPendingTimersAsync()
      await flushPromises()
      const row = rowOf(w, 'Mi Gozo')
      await pointer(row, 'pointerdown', 50, 100)
      vi.advanceTimersByTime(600)
      await nextTick()
      expect(menu.value.open).toBe(true)
      expect(menu.value.title).toBe('Mi Gozo')
      // soltar después no la pone a sonar ni la elige
      await pointer(row, 'pointerup', 50, 100)
      await row.trigger('click')
      expect(held.playback.bridge.setQueue).not.toHaveBeenCalled()
      w.unmount()
    } finally {
      vi.useRealTimers()
    }
  })

  it('«Estudiar» opens study mode, and back closes it like its button', async () => {
    const w = mount(MobileApp, { attachTo: document.body })
    await flushPromises()
    await rowOf(w, 'Mi Gozo').trigger('click')
    const study = w.findAll('.m-song-actions button').find((b) => b.text() === 'Estudiar')
    await study.trigger('click')
    await flushPromises()
    expect(held.playback.bridge.setQueue).toHaveBeenCalled()
    expect(w.findComponent(StudyBar).exists()).toBe(true)
    // la cabecera del estudio: el título grande y el artista debajo
    expect(w.find('.study-head-title').text()).toBe('Mi Gozo')
    expect(w.find('.study-head-artist').text()).toContain('Barak')
    expect(w.find('.m-app').classes()).toContain('m-studying')
    // «atrás»: oculta el estudio, sin tocar lo que suena (sigue puesto)
    held.playback.bridge.setSpeed.mockClear()
    expect(window.__danplayBack()).toBe(true)
    await flushPromises()
    expect(w.findComponent(StudyBar).exists()).toBe(true)
    expect(w.findComponent(StudyBar).isVisible()).toBe(false)
    expect(w.find('.m-app').classes()).not.toContain('m-studying')
    expect(held.playback.bridge.setSpeed, 'nada vuelve a como viene').not.toHaveBeenCalled()
    // luego la canción elegida, y luego ya nada (la app se va al fondo)
    expect(window.__danplayBack()).toBe(true)
    expect(window.__danplayBack()).toBe(false)
    w.unmount()
    expect(window.__danplayBack).toBeUndefined()
  })

  it('with no tracks it explains how to bring them from the PC', async () => {
    held.state.songs = []
    const w = mount(MobileApp, { attachTo: document.body })
    await flushPromises()
    expect(w.text()).toContain('Todavía no hay pistas en el celular')
    expect(w.text()).toContain('Convertir a DanPista')
    w.unmount()
  })
})

describe('the phone music', () => {
  const phoneSong = (id, title, artist, path) =>
    song(id, { title, artist, path, phone: true, has_stems: false, duration: 200 })

  beforeEach(() => {
    held.state.phone = [
      phoneSong(-11, 'Coro Uno', 'Grupo', '/sd/Music/Grupo/coro-uno.mp3'),
      phoneSong(-12, 'Coro Dos', 'Grupo', '/sd/Music/Grupo/coro-dos.mp3'),
      phoneSong(-13, 'Otra', 'Alguien', '/sd/Download/otra.mp3')
    ]
    held.state.phoneFolders = [
      { path: '/sd/Music/Grupo', name: 'Music/Grupo', songs: 2, on: true },
      { path: '/sd/Download', name: 'Download', songs: 1, on: true },
      { path: '/sd/Notas', name: 'Notas', songs: 4, on: false }
    ]
  })
  afterEach(() => {
    delete window.DanPlayMusic
    delete window.__danplayMusicPermission
  })
  async function openPhone(w) {
    await w.find('[aria-label="Abrir el menú"]').trigger('click')
    await flushPromises()
    ;[...document.querySelectorAll('.m-nav-item')]
      .find((b) => b.textContent.includes('Música del teléfono'))
      .click()
    await flushPromises()
  }

  it('is reachable from the menu; playing one queues the rest of its folder after it', async () => {
    const w = mount(MobileApp, { attachTo: document.body })
    await flushPromises()
    await openPhone(w)
    expect(w.find('.m-top').text()).toContain('Música del teléfono')
    expect(w.text()).toContain('Coro Uno')
    expect(w.text()).toContain('Otra')
    // su menú: sonar y estudiar, sin lo de las pistas traídas
    await rowOf(w, 'Coro Dos').find('.m-song-more').trigger('click')
    expect(labels()).toEqual(['Reproducir', 'Estudiar'])
    await menu.value.items[0].action()
    await flushPromises()
    const [items, start] = held.playback.bridge.setQueue.mock.calls.at(-1)
    expect(items.map((s) => s.title)).toEqual(['Coro Uno', 'Coro Dos'])
    expect(start).toBe(-12)
    w.unmount()
  })

  it('without permission it asks for it, and with it the music shows', async () => {
    let granted = false
    window.DanPlayMusic = {
      granted: () => granted,
      request: () => {
        granted = true
        setTimeout(() => window.__danplayMusicPermission(true), 0)
      }
    }
    const w = mount(MobileApp, { attachTo: document.body })
    await flushPromises()
    await openPhone(w)
    expect(w.text()).toContain('Música y audio')
    expect(w.text()).not.toContain('Coro Uno')
    await w
      .findAll('button')
      .find((b) => b.text().includes('Dar permiso'))
      .trigger('click')
    await new Promise((r) => setTimeout(r, 5))
    await flushPromises()
    expect(held.api.phoneScan).toHaveBeenCalled()
    expect(w.text()).toContain('Coro Uno')
    w.unmount()
  })

  it('folders and songs come from what Android reports (SD card included)', async () => {
    const media = [
      { path: '/storage/1A2B-3C4D/Musica/coro.mp3', title: 'Coro', artist: 'Grupo' },
      { path: '/storage/emulated/0/Music/Grupo/coro-uno.mp3', title: 'Coro Uno', artist: 'Grupo' }
    ]
    window.DanPlayMusic = { granted: () => true, songs: () => JSON.stringify(media) }
    const w = mount(MobileApp, { attachTo: document.body })
    await flushPromises()
    await openPhone(w)
    await w
      .findAll('button')
      .find((b) => b.text().includes('Carpetas'))
      .trigger('click')
    await flushPromises()
    expect(held.api.phoneMedia).toHaveBeenCalledWith(media)
    expect(held.api.phoneMedia.mock.invocationCallOrder[0]).toBeLessThan(
      held.api.phoneFolders.mock.invocationCallOrder.at(-1)
    )
    // y antes de volver a mirar la música
    held.api.phoneMedia.mockClear()
    const { scanPhone } = await import('../src/mobile/phoneMusic.js')
    await scanPhone()
    expect(held.api.phoneMedia).toHaveBeenCalledTimes(1)
    expect(held.api.phoneMedia.mock.invocationCallOrder[0]).toBeLessThan(
      held.api.phoneScan.mock.invocationCallOrder.at(-1)
    )
    w.unmount()
  })

  it('folders are chosen as on the PC', async () => {
    const w = mount(MobileApp, { attachTo: document.body })
    await flushPromises()
    await openPhone(w)
    await w
      .findAll('button')
      .find((b) => b.text().includes('Carpetas'))
      .trigger('click')
    await flushPromises()
    const boxes = document.querySelectorAll('.m-folder')
    expect([...boxes].map((b) => b.getAttribute('aria-checked'))).toEqual(['true', 'true', 'false'])
    boxes[2].click()
    boxes[1].click()
    await flushPromises()
    expect(document.querySelector('.m-folders-count').textContent).toContain('6 canciones en 2')
    ;[...document.querySelectorAll('.m-folders button')]
      .find((b) => b.textContent.includes('Listo'))
      .click()
    await flushPromises()
    expect(held.api.setPhoneFolders).toHaveBeenCalledWith(['/sd/Music/Grupo', '/sd/Notas'])
    w.unmount()
  })
})

describe('the side menu and the playlists', () => {
  it('«Acerca de DanPlay» says who made it, how to write to them and what is new', async () => {
    const w = mount(MobileApp, { attachTo: document.body })
    await flushPromises()
    await w.find('[aria-label="Abrir el menú"]').trigger('click')
    await flushPromises()
    const nav = document.querySelector('.m-nav')
    expect(nav.querySelector('.m-nav-credit').textContent).toContain('Daniel Rangel')
    ;[...nav.querySelectorAll('.m-nav-item')]
      .find((b) => b.textContent.includes('Acerca de DanPlay'))
      .click()
    await flushPromises()
    const about = document.querySelector('.m-about')
    expect(about.textContent).toContain('DanPlay es obra de Daniel Rangel')
    expect(about.querySelector('a').getAttribute('href')).toBe('https://t.me/Dani17r')
    expect(about.textContent).toContain('@Dani17r')
    expect(about.textContent).toContain('Novedades')
    expect(about.textContent).toContain('Términos y condiciones')
    // «atrás» la cierra
    expect(window.__danplayBack()).toBe(true)
    await flushPromises()
    expect(document.querySelector('.m-about')).toBeNull()
    w.unmount()
  })

  it('opens with ☰ and leads to the favorites and the playlists', async () => {
    held.state.songs[1].favorite = 1
    held.state.playlists = [{ id: 7, name: 'Domingo', n: 1, seconds: 245 }]
    held.state.playlistSongs = [held.state.songs[0]]
    const w = mount(MobileApp, { attachTo: document.body })
    await flushPromises()
    await w.find('[aria-label="Abrir el menú"]').trigger('click')
    await flushPromises()
    const nav = document.querySelector('.m-nav')
    // al abrirse relee las listas (y las favoritas)
    expect(held.api.playlists).toHaveBeenCalledTimes(2)
    expect(nav.textContent).toContain('Favoritas')
    expect(nav.textContent).toContain('Domingo')
    expect(nav.textContent).toContain('Ajustes')
    // las favoritas
    ;[...nav.querySelectorAll('.m-nav-item')]
      .find((b) => b.textContent.includes('Favoritas'))
      .click()
    await flushPromises()
    expect(w.find('.m-top').text()).toContain('Favoritas')
    expect(w.text()).toContain('Dios Poderoso')
    expect(w.text()).not.toContain('Mi Gozo')
    // una lista: se toca entera en su orden
    await w.find('[aria-label="Abrir el menú"]').trigger('click')
    await flushPromises()
    ;[...document.querySelectorAll('.m-nav-item')]
      .find((b) => b.textContent.includes('Domingo'))
      .click()
    await flushPromises()
    expect(w.find('.m-top').text()).toContain('Domingo')
    const play = w.findAll('.m-list-head button').find((b) => b.text().includes('Tocar en orden'))
    await play.trigger('click')
    await flushPromises()
    const [items, start, origin] = held.playback.bridge.setQueue.mock.calls.at(-1)
    expect(items.map((t) => t.id)).toEqual([1])
    expect(start).toBe(1)
    expect(origin).toMatchObject({ kind: 'list', id: 7, label: 'Domingo' })
    // «atrás» desde una lista vuelve a todas (antes cierra el reproductor entero)
    expect(window.__danplayBack()).toBe(true)
    expect(window.__danplayBack()).toBe(true)
    await flushPromises()
    expect(w.find('.m-top').text()).toContain('Mis pistas')
    w.unmount()
  })

  it('a new playlist is asked for by name and opens', async () => {
    const w = mount(MobileApp, { attachTo: document.body })
    await flushPromises()
    await w.find('[aria-label="Abrir el menú"]').trigger('click')
    await flushPromises()
    document.querySelector('.m-nav-add').click()
    await flushPromises()
    expect(dialog.value.open).toBe(true)
    expect(dialog.value.kind).toBe('prompt')
    dialogOk('Ensayo del jueves')
    await flushPromises()
    expect(held.api.createPlaylist).toHaveBeenCalledWith('Ensayo del jueves')
    expect(w.find('.m-top').text()).toContain('Ensayo del jueves')
    expect(w.text()).toContain('La lista está vacía')
    w.unmount()
  })

  it('a song is added to a playlist from its options', async () => {
    held.state.playlists = [{ id: 3, name: 'Domingo', n: 0, seconds: 0 }]
    const w = mount(MobileApp, { attachTo: document.body })
    await flushPromises()
    await rowOf(w, 'Mi Gozo').find('.m-song-more').trigger('click')
    const add = menu.value.items.find((i) => i.label === 'Añadir a una lista')
    expect(add.children.map((c) => c.label)).toEqual(['Domingo', 'Nueva lista…'])
    closeMenu()
    await add.children[0].action()
    await flushPromises()
    expect(held.api.addToPlaylist).toHaveBeenCalledWith(3, [1])
    w.unmount()
  })
})

describe('the phone player', () => {
  const sounding = () =>
    held.playback.emit({
      track: { id: 1, title: 'Mi Gozo', artist: 'Barak', duration: 245, path: '/pistas/mi-gozo' },
      index: 0,
      length: 1,
      duration: 245,
      position: 30,
      playing: true,
      path: '/pistas/mi-gozo'
    })

  it('swiping the bar up opens study mode; a tap opens the player', async () => {
    sounding()
    const w = mount(MobilePlayer, { props: { full: false }, attachTo: document.body })
    await flushPromises()
    const bar = w.find('.m-mini-open')
    await pointer(bar, 'pointerdown', 100, 700)
    await pointer(bar, 'pointermove', 102, 680)
    await pointer(bar, 'pointermove', 103, 630)
    expect(w.find('.m-lift-hint').classes()).toContain('ready')
    await pointer(bar, 'pointerup', 103, 630)
    expect(w.emitted('toggleStudy')).toHaveLength(1)
    // el arrastre no cuenta además como toque
    await bar.trigger('click')
    expect(w.emitted('update:full')).toBeUndefined()
    await new Promise((r) => setTimeout(r, 450))
    await bar.trigger('click')
    expect(w.emitted('update:full')).toEqual([[true]])
    w.unmount()
  })

  it('a short swipe up opens nothing', async () => {
    sounding()
    const w = mount(MobilePlayer, { props: { full: false }, attachTo: document.body })
    await flushPromises()
    const bar = w.find('.m-mini-open')
    await pointer(bar, 'pointerdown', 100, 700)
    await pointer(bar, 'pointermove', 100, 688)
    await new Promise((r) => setTimeout(r, 120))
    await pointer(bar, 'pointerup', 100, 688)
    expect(w.emitted('toggleStudy')).toBeUndefined()
    w.unmount()
  })

  it('the full player: everything large, with study mode, lyrics and queue', async () => {
    sounding()
    const w = mount(MobilePlayer, {
      props: { full: true, song: { ...song(1), title: 'Mi Gozo', key: 'A', bpm: 140 } },
      attachTo: document.body
    })
    await flushPromises()
    const full = w.find('.m-full')
    expect(full.exists()).toBe(true)
    expect(full.text()).toContain('Mi Gozo')
    expect(full.text()).toContain('Tono A')
    expect(full.text()).toContain('140 bpm')
    await full.find('[aria-label="Avanzar 10 segundos"]').trigger('click')
    await full.find('[aria-label="Pausar"]').trigger('click')
    expect(held.playback.bridge.toggle).toHaveBeenCalled()
    await w
      .findAll('.m-tile')
      .find((t) => t.text().includes('Cola'))
      .trigger('click')
    expect(w.find('.m-queue').exists()).toBe(true)
    await w
      .findAll('.m-tile')
      .find((t) => t.text().includes('Estudio'))
      .trigger('click')
    expect(w.emitted('toggleStudy')).toHaveLength(1)
    expect(w.emitted('update:full').at(-1)).toEqual([false])
    w.unmount()
  })

  it('the app volume, at hand and visible: it lowers, mutes and comes back', async () => {
    sounding()
    const w = mount(MobilePlayer, { props: { full: true }, attachTo: document.body })
    await flushPromises()
    const range = w.find('.m-volume input[type="range"]')
    expect(range.exists()).toBe(true)
    expect(w.find('.m-volume-pct').text()).toMatch(/^\d+ %$/)
    await range.setValue('0.6')
    expect(held.playback.bridge.setVolume).toHaveBeenLastCalledWith(0.6)
    held.playback.emit({ volume: 0.6 })
    await flushPromises()
    await w.find('[aria-label="Silenciar la app"]').trigger('click')
    expect(held.playback.bridge.setVolume).toHaveBeenLastCalledWith(0)
    expect(w.find('.m-volume-pct').text()).toBe('0 %')
    await flushPromises()
    await w.find('[aria-label="Volver a oírla"]').trigger('click')
    expect(held.playback.bridge.setVolume).toHaveBeenLastCalledWith(0.6)
    w.unmount()
  })

  it('with study mode open, the bar has the time and the 10 s skips', async () => {
    sounding()
    const w = mount(MobilePlayer, { props: { study: true }, attachTo: document.body })
    await flushPromises()
    expect(w.find('.m-player-study').exists()).toBe(true)
    expect(w.text()).toContain('0:30')
    expect(w.text()).toContain('4:05')
    expect(w.find('[aria-label="Retroceder 10 segundos"]').exists()).toBe(true)
    w.unmount()
  })
})

describe('the Android notification controls', () => {
  it('tells Android what is playing, and again on pause', async () => {
    const update = vi.fn()
    window.DanPlayAndroid = { update }
    const w = mount(MobileApp, { attachTo: document.body })
    await flushPromises()
    held.playback.emit({
      track: { id: 1, title: 'Mi Gozo', artist: 'Barak', duration: 245, path: '/pistas/mi-gozo' },
      index: 0,
      length: 2,
      duration: 245,
      position: 12,
      playing: true,
      has_next: true
    })
    await flushPromises()
    expect(update).toHaveBeenCalled()
    const first = JSON.parse(update.mock.calls.at(-1)[0])
    expect(first).toMatchObject({
      id: 1,
      title: 'Mi Gozo',
      artist: 'Barak',
      folder: '/pistas/mi-gozo',
      duration: 245000,
      position: 12000,
      playing: true,
      hasNext: true
    })
    // un tick normal no se manda (Android avanza solo la barra)
    const calls = update.mock.calls.length
    held.playback.emit({ position: 12.5 })
    await flushPromises()
    expect(update.mock.calls.length).toBe(calls)
    held.playback.emit({ playing: false })
    await flushPromises()
    expect(JSON.parse(update.mock.calls.at(-1)[0]).playing).toBe(false)
    w.unmount()
  })
})

describe('the desktop components, on the phone', () => {
  it('the study stems menu only allows going back to the default mix', async () => {
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
    // sin «Grande»: en el celular el estudio ya ocupa la pantalla
    expect(w.find('.study-size').exists()).toBe(false)
    w.unmount()
  })

  it('cues brought from the PC: they play and mix, but cannot be made or changed', async () => {
    held.state.songs[0].study = JSON.stringify({
      cues: [
        { t: 8, file: 'guias/coro-1.wav', text: 'Intro', lane: 'g1' },
        { t: 52, file: 'guias/grabada-1.wav', lane: 'g2' }
      ],
      cueLanes: [
        { key: 'g1', name: 'Banda' },
        { key: 'g2', name: 'Baterista', gain: 1.2 }
      ]
    })
    held.playback.emit({
      track: { id: 1, title: 'Mi Gozo', artist: 'Barak', duration: 200 },
      index: 0,
      length: 1,
      duration: 200,
      playing: false,
      path: '/pistas/barak-mi-gozo',
      metronome: { on: false, bpm: 100, meter: 4, shift: 0, mult: 0, volume: 0.8 }
    })
    const w = mount(StudyBar, { attachTo: document.body })
    await flushPromises()
    // sus carriles, como los de las pistas: callar, solo y volumen
    const mix = w.find('.tl-guide-mix')
    expect(mix.findAll('.tl-lane-name').map((n) => n.text())).toEqual(['Banda', 'Baterista'])
    const sent = () => held.playback.bridge.setCues.mock.calls.at(-1)
    // las del .danpista, dentro de la carpeta de la pista
    expect(sent()[1].map((c) => c.path)).toEqual(['guias/coro-1.wav', 'guias/grabada-1.wav'])
    await mix.findAll('.tl-lane')[0].find('.tl-lane-btn').trigger('click')
    await flushPromises()
    expect(
      sent()[1].map((c) => c.at),
      'la banda, callada'
    ).toEqual([52])
    // tocar una guía en la onda lleva a ella, sin abrir nada para cambiarla
    await w.findAll('.tl-cue')[0].trigger('click')
    await flushPromises()
    expect(held.playback.bridge.seek).toHaveBeenLastCalledWith(8)
    expect(w.findComponent({ name: 'StudyCue' }).exists()).toBe(false)
    // en la lista, solo escucharla: la guía, desde la carpeta de la pista
    const list = w.findAll('.study-cue')
    expect(list).toHaveLength(2)
    expect(w.find('.study-cue [aria-label="Cambiarla"]').exists()).toBe(false)
    expect(w.find('.study-cue [aria-label="Quitarla"]').exists()).toBe(false)
    await list[0].find('[aria-label="Escucharla"]').trigger('click')
    expect(held.api.previewCue).toHaveBeenCalledWith('/pistas/barak-mi-gozo/guias/coro-1.wav')
    // y nada para poner guías
    expect(w.find('.study-add-cue').exists()).toBe(false)
    expect(w.find('.tl-guide-tracks').exists()).toBe(false)
    w.unmount()
  })

  it('«Salidas» is visible in the study header', async () => {
    const w = mount(StudyBar, { attachTo: document.body })
    await flushPromises()
    const outputs = w.find('.study-head [aria-label="Salidas"]')
    expect(outputs.exists()).toBe(true)
    await outputs.trigger('click')
    await flushPromises()
    expect(document.querySelector('.outs')).not.toBeNull()
    expect(document.body.textContent).toContain('Estéreo')
    w.unmount()
  })

  it('on the waveform, «varios» and the lock sit on their row and each lane has its name', async () => {
    const lanes = [
      { key: 'drums', name: 'Batería', on: true },
      { key: 'vocals', name: 'Voces', on: false, mute: true }
    ]
    const w = mount(StudyTimeline, {
      props: { songId: 1, duration: 200, position: 0, lanes, laneHeight: 42 },
      attachTo: document.body
    })
    await flushPromises()
    // fuera de la onda: no tapan el final, donde a veces va el tramo
    expect(w.find('.tl-box .tl-multi').exists()).toBe(false)
    expect(w.find('.tl-box .tl-lock').exists()).toBe(false)
    const tools = w.find('.tl-tools')
    expect(tools.text()).toContain('Varios tramos')
    expect(tools.text()).toContain('Bloquear la onda')
    await tools.findAll('button')[1].trigger('click')
    expect(w.emitted('update:locked')).toEqual([[true]])
    const tags = w.findAll('.tl-lane-tags span')
    expect(tags.map((t) => t.text())).toEqual(['Batería', 'Voces'])
    expect(tags[1].classes()).toContain('off')
    expect(tags[1].attributes('style')).toContain('top: 42px')
    w.unmount()
  })

  it('the details panel shows lyrics and chords, without searching or editing', async () => {
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

describe('outputs on the phone', () => {
  it('the study menu offers «Salidas» and sends the routing to the phone engine', async () => {
    const { resetRouting } = await import('../src/composables/useRouting.js')
    resetRouting()
    held.playback.emit({
      track: { id: 1, title: 'Mi Gozo', artist: 'Barak', duration: 245, path: '/pistas/mi-gozo' },
      index: 0,
      length: 1,
      duration: 245,
      position: 30,
      playing: true,
      path: '/pistas/mi-gozo'
    })
    const w = mount(StudyBar, { attachTo: document.body })
    await flushPromises()
    await w.find('[aria-label="Más del modo estudio"]').trigger('click')
    const outputs = menu.value.items.find((i) => i.label?.startsWith('Salidas'))
    expect(outputs).toBeTruthy()
    outputs.action()
    await flushPromises()
    const presets = document.querySelectorAll('.outs-preset')
    expect(presets.length).toBe(4)
    presets[1].click()
    await flushPromises()
    expect(held.playback.bridge.setRouting).toHaveBeenLastCalledWith(
      expect.objectContaining({ click: 1, cues: 1, song: 0 })
    )
    w.unmount()
  })
})

describe('the touch list on its own', () => {
  it('while sorting a playlist, each row moves up, down or is removed, and does not start playing', async () => {
    const songs = [song(1, { title: 'Uno' }), song(2, { title: 'Dos' })]
    const w = mount(MobileSongs, { props: { songs, editing: true } })
    const first = w.findAll('.m-song')[0]
    expect(first.find('[aria-label="Subir Uno"]').attributes('disabled')).toBeDefined()
    await first.find('[aria-label="Bajar Uno"]').trigger('click')
    expect(w.emitted('move')).toEqual([[0, 1]])
    await first.find('[aria-label="Quitar de la lista Uno"]').trigger('click')
    expect(w.emitted('remove')[0][0].id).toBe(1)
    await first.trigger('click')
    await first.trigger('click')
    expect(w.emitted('play')).toBeUndefined()
    expect(w.find('.m-song-more').exists()).toBe(false)
  })
})
