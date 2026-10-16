import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import { nextTick } from 'vue'

// La app del celular (movil.html → src/movil/): su propio armazón para los
// dedos (la lista táctil, el reproductor del celular, el menú de lado) y los
// componentes de la PC donde valen tal cual (el estudio, la ficha). Lo que
// solo se puede en la PC no aparece.
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
import MovilSongs from '../src/movil/MovilSongs.vue'
import MovilPlayer from '../src/movil/MovilPlayer.vue'
import StudyBar from '../src/components/StudyBar.vue'
import StudyTimeline from '../src/components/StudyTimeline.vue'
import DetailsPanel from '../src/components/DetailsPanel.vue'
import { isMobileApp } from '../src/platform.js'
import { resetPlayback } from '../src/composables/usePlayback.js'
import { resetClick } from '../src/composables/useClick.js'
import { useContextMenu, closeMenu } from '../src/composables/useContextMenu.js'
import { useDialog, dialogOk } from '../src/composables/useDialog.js'
import { tapPlays } from '../src/movil/prefs.js'
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

describe('la app del celular', () => {
  it('sabe que es la del celular', () => {
    expect(isMobileApp).toBe(true)
  })

  it('enseña las pistas traídas y lo que se hace con cada una, sin nada de la PC', async () => {
    const w = mount(MovilApp, { attachTo: document.body })
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

  it('un toque elige la canción y enseña sus botones; el doble toque la pone', async () => {
    const w = mount(MovilApp, { attachTo: document.body })
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

  it('con «Un toque pone la canción», basta un toque; y la carátula siempre la pone', async () => {
    tapPlays.value = true
    const w = mount(MovilApp, { attachTo: document.body })
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

  it('mantener pulsada una canción abre sus opciones', async () => {
    vi.useFakeTimers()
    try {
      const w = mount(MovilApp, { attachTo: document.body })
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

  it('«Estudiar» abre el estudio, y «atrás» lo cierra como su botón', async () => {
    const w = mount(MovilApp, { attachTo: document.body })
    await flushPromises()
    await rowOf(w, 'Mi Gozo').trigger('click')
    const estudiar = w.findAll('.m-song-actions button').find((b) => b.text() === 'Estudiar')
    await estudiar.trigger('click')
    await flushPromises()
    expect(held.playback.bridge.setQueue).toHaveBeenCalled()
    expect(w.findComponent(StudyBar).exists()).toBe(true)
    // la cabecera del estudio: el título grande y el artista debajo
    expect(w.find('.study-head-title').text()).toBe('Mi Gozo')
    expect(w.find('.study-head-artist').text()).toContain('Barak')
    expect(w.find('.m-app').classes()).toContain('m-studying')
    // «atrás»: oculta el estudio, sin tocar lo que suena (sigue puesto)
    held.playback.bridge.setSpeed.mockClear()
    expect(window.__danplayAtras()).toBe(true)
    await flushPromises()
    expect(w.findComponent(StudyBar).exists()).toBe(true)
    expect(w.findComponent(StudyBar).isVisible()).toBe(false)
    expect(w.find('.m-app').classes()).not.toContain('m-studying')
    expect(held.playback.bridge.setSpeed, 'nada vuelve a como viene').not.toHaveBeenCalled()
    // luego la canción elegida, y luego ya nada (la app se va al fondo)
    expect(window.__danplayAtras()).toBe(true)
    expect(window.__danplayAtras()).toBe(false)
    w.unmount()
    expect(window.__danplayAtras).toBeUndefined()
  })

  it('sin pistas explica cómo traerlas de la PC', async () => {
    held.state.songs = []
    const w = mount(MovilApp, { attachTo: document.body })
    await flushPromises()
    expect(w.text()).toContain('Todavía no hay pistas en el celular')
    expect(w.text()).toContain('Convertir a DanPista')
    w.unmount()
  })
})

describe('la música del teléfono', () => {
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
    delete window.DanPlayMusica
    delete window.__danplayPermisoMusica
  })
  async function openPhone(w) {
    await w.find('[aria-label="Abrir el menú"]').trigger('click')
    await flushPromises()
    ;[...document.querySelectorAll('.m-nav-item')]
      .find((b) => b.textContent.includes('Música del teléfono'))
      .click()
    await flushPromises()
  }

  it('se ve desde el menú; al poner una suenan detrás las de su carpeta', async () => {
    const w = mount(MovilApp, { attachTo: document.body })
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

  it('sin permiso lo pide, y con él se ve la música', async () => {
    let granted = false
    window.DanPlayMusica = {
      tiene: () => granted,
      pedir: () => {
        granted = true
        setTimeout(() => window.__danplayPermisoMusica(true), 0)
      }
    }
    const w = mount(MovilApp, { attachTo: document.body })
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

  it('las carpetas se eligen como en la PC', async () => {
    const w = mount(MovilApp, { attachTo: document.body })
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

describe('el menú de lado y las listas', () => {
  it('«Acerca de DanPlay» dice quién lo hizo, cómo escribirle y lo nuevo', async () => {
    const w = mount(MovilApp, { attachTo: document.body })
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
    expect(window.__danplayAtras()).toBe(true)
    await flushPromises()
    expect(document.querySelector('.m-about')).toBeNull()
    w.unmount()
  })

  it('abre con ☰ y lleva a las favoritas y a las listas', async () => {
    held.state.songs[1].favorite = 1
    held.state.playlists = [{ id: 7, name: 'Domingo', n: 1, seconds: 245 }]
    held.state.playlistSongs = [held.state.songs[0]]
    const w = mount(MovilApp, { attachTo: document.body })
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
    expect(window.__danplayAtras()).toBe(true)
    expect(window.__danplayAtras()).toBe(true)
    await flushPromises()
    expect(w.find('.m-top').text()).toContain('Mis pistas')
    w.unmount()
  })

  it('una lista nueva se pide por su nombre y se abre', async () => {
    const w = mount(MovilApp, { attachTo: document.body })
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

  it('una canción se añade a una lista desde sus opciones', async () => {
    held.state.playlists = [{ id: 3, name: 'Domingo', n: 0, seconds: 0 }]
    const w = mount(MovilApp, { attachTo: document.body })
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

describe('el reproductor del celular', () => {
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

  it('la barra se sube con el dedo para abrir el estudio; un toque abre el reproductor', async () => {
    sounding()
    const w = mount(MovilPlayer, { props: { full: false }, attachTo: document.body })
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

  it('subirla poco no abre nada', async () => {
    sounding()
    const w = mount(MovilPlayer, { props: { full: false }, attachTo: document.body })
    await flushPromises()
    const bar = w.find('.m-mini-open')
    await pointer(bar, 'pointerdown', 100, 700)
    await pointer(bar, 'pointermove', 100, 688)
    await new Promise((r) => setTimeout(r, 120))
    await pointer(bar, 'pointerup', 100, 688)
    expect(w.emitted('toggleStudy')).toBeUndefined()
    w.unmount()
  })

  it('el reproductor entero: todo grande, con el estudio, la letra y la cola', async () => {
    sounding()
    const w = mount(MovilPlayer, {
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

  it('el volumen de la app, a mano y a la vista: se baja, se silencia y vuelve', async () => {
    sounding()
    const w = mount(MovilPlayer, { props: { full: true }, attachTo: document.body })
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

  it('con el estudio abierto, la barra lleva la hora y los saltos de 10 s', async () => {
    sounding()
    const w = mount(MovilPlayer, { props: { study: true }, attachTo: document.body })
    await flushPromises()
    expect(w.find('.m-player-study').exists()).toBe(true)
    expect(w.text()).toContain('0:30')
    expect(w.text()).toContain('4:05')
    expect(w.find('[aria-label="Retroceder 10 segundos"]').exists()).toBe(true)
    w.unmount()
  })
})

describe('los mandos de la bandeja de Android', () => {
  it('cuenta a Android lo que suena, y otra vez al pausar', async () => {
    const update = vi.fn()
    window.DanPlayAndroid = { update }
    const w = mount(MovilApp, { attachTo: document.body })
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
    // sin «Grande»: en el celular el estudio ya ocupa la pantalla
    expect(w.find('.study-size').exists()).toBe(false)
    w.unmount()
  })

  it('en la onda, «varios» y el candado van en su fila y cada carril lleva su nombre', async () => {
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

describe('las salidas en el celular', () => {
  it('el menú del estudio ofrece «Salidas» y manda el reparto al motor del celular', async () => {
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
    const salidas = menu.value.items.find((i) => i.label?.startsWith('Salidas'))
    expect(salidas).toBeTruthy()
    salidas.action()
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

describe('la lista táctil, sola', () => {
  it('ordenando una lista, cada fila sube, baja o se quita, y no se pone a sonar', async () => {
    const songs = [song(1, { title: 'Uno' }), song(2, { title: 'Dos' })]
    const w = mount(MovilSongs, { props: { songs, editing: true } })
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
