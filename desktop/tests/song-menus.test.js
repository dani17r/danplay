// Los menús de clic derecho y lo que se hace desde ellos (useSongMenus).
// Vivían dentro de App.vue y casi nada de esto tenía prueba: la papelera sin
// confirmar, renombrar, enviar por Telegram, borrar la lista que se está
// viendo…
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { flushPromises } from '@vue/test-utils'
import { ref } from 'vue'

const held = vi.hoisted(() => ({ state: null, api: null, app: null }))
vi.mock('../src/api.js', async (importOriginal) => {
  const actual = await importOriginal()
  const { createState, buildApiDouble, createAppDouble } = await import('./support/backend.js')
  held.state = createState()
  held.api = buildApiDouble(actual, held.state)
  held.app = createAppDouble()
  return { ...actual, api: held.api, app: held.app }
})

import { useSongMenus } from '../src/composables/useSongMenus.js'
import { useSelection } from '../src/composables/useSelection.js'
import { useContextMenu, closeMenu } from '../src/composables/useContextMenu.js'
import { useDialog, dialogOk, dialogCancel } from '../src/composables/useDialog.js'
import { useNotices, clearNotices } from '../src/composables/useNotices.js'
import { song } from './support/backend.js'

const api = () => held.api
const { menu } = useContextMenu()
const { dialog } = useDialog()
const { notices } = useNotices()

/** Los menús con todo lo que necesitan, falso salvo la selección. */
function preparar({ view = { kind: 'all' }, playing = null, detailsInView = true } = {}) {
  const songs = ref([song(1), song(2, { title: 'Otra' }), song(3, { title: 'Tercera' })])
  const selection = useSelection({ shown: songs })
  const ctx = {
    view: ref(view),
    player: {
      track: ref(playing ? { id: playing } : null),
      state: { playing: !!playing },
      stop: vi.fn()
    },
    playlistActions: {
      playlists: ref([{ id: 7, name: 'Domingo', n: 3 }]),
      addTo: vi.fn(),
      addManyTo: vi.fn(),
      create: vi.fn(),
      removeSong: vi.fn(),
      rename: vi.fn(),
      exportTo: vi.fn(),
      sheet: vi.fn(),
      remove: vi.fn(async (pl) => pl.id),
      load: vi.fn(async () => {})
    },
    selection,
    play: vi.fn(),
    onUpdated: vi.fn(),
    refreshAll: vi.fn(async () => {}),
    reload: vi.fn(async () => {}),
    detailsInView: () => detailsInView,
    showDetailsOf: vi.fn()
  }
  return { ctx, songs, menus: useSongMenus(ctx) }
}

const ev = { clientX: 10, clientY: 10 }
const labels = () => menu.value.items.filter((i) => !i.separator).map((i) => i.label)
const item = (label) => menu.value.items.find((i) => i.label === label)
const pick = async (label) => {
  const it = item(label)
  expect(it, `no hay «${label}» en el menú`).toBeTruthy()
  closeMenu()
  await it.action()
  await flushPromises()
}

beforeEach(() => {
  held.state.songs = [song(1), song(2, { title: 'Otra' }), song(3, { title: 'Tercera' })]
  held.state.playlistSongs = held.state.songs.map((s) => ({ ...s }))
  closeMenu()
  dialogCancel()
  clearNotices()
  vi.clearAllMocks()
})

describe('el menú de una canción', () => {
  it('ofrece lo de siempre y la elige', async () => {
    const { menus, ctx } = preparar()
    menus.songMenu(ev, held.state.songs[0])
    expect(menu.value.open).toBe(true)
    expect(labels()).toEqual([
      'Reproducir',
      'Marcar como favorito',
      'Valorar',
      'Añadir a una lista',
      'Buscar letra y portada',
      'Difuminar la portada',
      'Renombrar…',
      'Abrir la carpeta',
      'Mandar a la papelera…'
    ])
    expect(ctx.selection.selectedIds.value).toEqual([1])
    await flushPromises()
    expect(ctx.selection.selected.value).toBe(1)
  })

  it('sobre la que suena ofrece pausar; en un repertorio, quitarla; sin panel, la ficha', async () => {
    const { menus, ctx } = preparar({
      view: { kind: 'playlist', id: 7, name: 'Domingo' },
      playing: 1,
      detailsInView: false
    })
    await menus.loadShareTargets()
    menus.songMenu(ev, held.state.songs[0])
    expect(labels()).toContain('Pausar')
    expect(labels()).toContain('Quitar de esta lista')
    expect(labels()).toContain('Ver detalles')
    expect(labels()).toContain('Enviar por Telegram')
    await pick('Quitar de esta lista')
    expect(ctx.playlistActions.removeSong).toHaveBeenCalled()
    menus.songMenu(ev, held.state.songs[0])
    await pick('Ver detalles')
    expect(ctx.showDetailsOf).toHaveBeenCalled()
  })

  it('valorar desde el teclado: un submenú con las estrellas', async () => {
    const { menus, ctx } = preparar()
    menus.songMenu(ev, song(1, { stars: 3 }))
    const valorar = item('Valorar')
    expect(valorar.note).toBe('3 de 5')
    expect(valorar.children.map((c) => c.label)).toEqual([
      '5 estrellas',
      '4 estrellas',
      '3 estrellas',
      '2 estrellas',
      '1 estrella',
      'Sin valorar'
    ])
    expect(valorar.children.find((c) => c.note === 'ahora').label).toBe('3 estrellas')
    await valorar.children[0].action()
    expect(api().setStars).toHaveBeenCalledWith(1, 5)
    expect(ctx.onUpdated).toHaveBeenCalledWith(expect.objectContaining({ id: 1, stars: 5 }))
  })

  it('la papelera pregunta antes, y cancelar no borra nada', async () => {
    const { menus, ctx } = preparar({ playing: 1 })
    menus.songMenu(ev, held.state.songs[0])
    const hecho = pick('Mandar a la papelera…')
    await flushPromises()
    expect(dialog.value.open).toBe(true)
    expect(dialog.value.danger).toBe(true)
    dialogCancel()
    await hecho
    expect(api().deleteSong).not.toHaveBeenCalled()

    menus.songMenu(ev, held.state.songs[0])
    const otra = pick('Mandar a la papelera…')
    await flushPromises()
    dialogOk()
    await otra
    expect(api().deleteSong).toHaveBeenCalledWith(1)
    // era la que sonaba: se para
    expect(ctx.player.stop).toHaveBeenCalled()
    expect(ctx.refreshAll).toHaveBeenCalled()
  })

  it('renombrar, buscar la letra y difuminar cambian la canción en todas partes', async () => {
    const { menus, ctx } = preparar()
    menus.songMenu(ev, held.state.songs[0])
    const renombrar = pick('Renombrar…')
    await flushPromises()
    expect(dialog.value.kind).toBe('prompt')
    dialogOk('Mi Gozo')
    await renombrar
    expect(api().edit).toHaveBeenCalledWith(1, { title: 'Mi Gozo' })
    expect(ctx.onUpdated).toHaveBeenLastCalledWith(expect.objectContaining({ title: 'Mi Gozo' }))

    menus.songMenu(ev, held.state.songs[1])
    await pick('Buscar letra y portada')
    expect(api().enrich).toHaveBeenCalledWith(2, expect.objectContaining({ lyrics: true }))
    expect(notices.value.at(-1).message).toBe('Listo')

    api().enrich.mockRejectedValueOnce(new Error('sin red'))
    menus.songMenu(ev, held.state.songs[1])
    await pick('Buscar letra y portada')
    expect(notices.value.at(-1).message).toContain('sin red')

    menus.songMenu(ev, held.state.songs[2])
    await pick('Difuminar la portada')
    expect(api().setBlur).toHaveBeenCalledWith(3, true)
    expect(notices.value.at(-1).message).toBe('Portada difuminada')
  })

  it('abrir la carpeta y enviar por Telegram buscan la ruta si no la tienen', async () => {
    const { menus } = preparar()
    await menus.loadShareTargets()
    const sinRuta = { ...held.state.songs[0], path: '' }
    menus.songMenu(ev, sinRuta)
    await pick('Abrir la carpeta')
    expect(held.app.revealInFolder).toHaveBeenCalledWith('/musica/cancion-1.mp3')
    menus.songMenu(ev, sinRuta)
    await pick('Enviar por Telegram')
    expect(held.app.sendToTelegram).toHaveBeenCalledWith(['/musica/cancion-1.mp3'])
    // si el explorador no se puede abrir, se dice
    held.app.revealInFolder.mockRejectedValueOnce(new Error('no hay explorador'))
    menus.songMenu(ev, sinRuta)
    await pick('Abrir la carpeta')
    expect(notices.value.at(-1).message).toContain('no hay explorador')
  })
})

describe('el menú de varias', () => {
  it('sobre una de las elegidas, el menú es el de todas', async () => {
    const { menus, ctx } = preparar({ view: { kind: 'playlist', id: 7, name: 'Domingo' } })
    await menus.loadShareTargets()
    await ctx.selection.select(1)
    await ctx.selection.select(3, { shiftKey: true })
    menus.songMenu(ev, held.state.songs[1])
    expect(menu.value.title).toBe('3 canciones')
    expect(labels()).toEqual([
      'Reproducir estas 3',
      'Añadir 3 a una lista',
      'Marcar 3 como favoritas',
      'Quitar 3 de esta lista',
      'Enviar 3 por Telegram',
      'Mandar 3 a la papelera…'
    ])
    await pick('Reproducir estas 3')
    expect(ctx.play).toHaveBeenCalledWith(
      expect.objectContaining({ id: 1 }),
      expect.arrayContaining([expect.objectContaining({ id: 3 })]),
      expect.objectContaining({ label: 'la selección' })
    )
  })

  it('favoritas, quitarlas de la lista y la papelera, de una vez', async () => {
    const { menus, ctx } = preparar({ view: { kind: 'playlist', id: 7, name: 'Domingo' } })
    const tres = held.state.songs.map((s) => ({ ...s }))
    menus.groupMenu(ev, tres)
    await pick('Marcar 3 como favoritas')
    expect(api().toggleFavorite).toHaveBeenCalledTimes(3)
    menus.groupMenu(ev, tres)
    await pick('Quitar 3 de esta lista')
    expect(api().removeFromPlaylist).toHaveBeenCalledTimes(3)
    expect(ctx.reload).toHaveBeenCalledWith(true)
    menus.groupMenu(ev, tres)
    const papelera = pick('Mandar 3 a la papelera…')
    await flushPromises()
    expect(dialog.value.title).toBe('Mandar 3 canciones a la papelera')
    dialogOk()
    await papelera
    expect(api().deleteSong).toHaveBeenCalledTimes(3)
    expect(ctx.selection.selectedIds.value).toEqual([])
  })

  it('las listas a las que mandarlas, y una nueva', async () => {
    const { menus, ctx } = preparar()
    const dos = held.state.songs.slice(0, 2)
    menus.groupMenu(ev, dos)
    const destinos = item('Añadir 2 a una lista').children
    expect(destinos.filter((d) => !d.separator).map((d) => d.label)).toEqual([
      'Domingo',
      'Nueva lista…'
    ])
    await destinos[0].action()
    expect(ctx.playlistActions.addManyTo).toHaveBeenCalledWith(
      dos,
      expect.objectContaining({ id: 7 })
    )
    await destinos.at(-1).action()
    expect(ctx.playlistActions.create).toHaveBeenCalledWith(dos)
  })
})

describe('el menú de un repertorio', () => {
  it('abrir, renombrar, exportar, la hoja y borrarlo', async () => {
    const { menus, ctx } = preparar({ view: { kind: 'playlist', id: 7, name: 'Domingo' } })
    const pl = { id: 7, name: 'Domingo' }
    await menus.loadShareTargets()
    menus.playlistMenu(ev, pl)
    expect(labels()).toEqual([
      'Abrir',
      'Renombrar…',
      'Exportar a .m3u',
      'Hoja para el atril…',
      'Enviar por Telegram',
      'Borrar la lista…'
    ])
    for (const l of ['Renombrar…', 'Exportar a .m3u', 'Hoja para el atril…']) {
      menus.playlistMenu(ev, pl)
      await pick(l)
    }
    expect(ctx.playlistActions.rename).toHaveBeenCalledWith(pl)
    expect(ctx.playlistActions.exportTo).toHaveBeenCalledWith(pl)
    expect(ctx.playlistActions.sheet).toHaveBeenCalledWith(pl)
    // borrar la que se está viendo devuelve a «Todas»
    menus.playlistMenu(ev, pl)
    await pick('Borrar la lista…')
    expect(ctx.playlistActions.remove).toHaveBeenCalledWith(pl)
    expect(ctx.view.value).toEqual({ kind: 'all' })
    menus.playlistMenu(ev, pl)
    await pick('Abrir')
    expect(ctx.view.value).toEqual({ kind: 'playlist', id: 7, name: 'Domingo' })
  })

  it('enviarla por Telegram manda todas sus canciones; vacía, lo dice', async () => {
    const { menus } = preparar()
    await menus.loadShareTargets()
    menus.playlistMenu(ev, { id: 7, name: 'Domingo' })
    await pick('Enviar por Telegram')
    expect(held.app.sendToTelegram.mock.calls[0][0]).toHaveLength(3)
    held.state.playlistSongs = []
    menus.playlistMenu(ev, { id: 7, name: 'Domingo' })
    await pick('Enviar por Telegram')
    expect(notices.value.at(-1).message).toBe('Esa lista está vacía')
  })
})
