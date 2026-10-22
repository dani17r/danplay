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
import { song, stemsOf } from './support/backend.js'
import { useSeparation, resetSeparation } from '../src/composables/useSeparation.js'

const api = () => held.api
const { menu } = useContextMenu()
const { dialog } = useDialog()
const { notices } = useNotices()

/** Los menús con todo lo que necesitan, falso salvo la selección. */
function prepare({ view = { kind: 'all' }, playing = null, detailsInView = true } = {}) {
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

beforeEach(async () => {
  resetSeparation()
  held.state.separation.current = null
  held.state.separation.queue = []
  held.state.separation.events = []
  held.state.separation.pending = 0
  held.state.stems = {}
  await useSeparation().refresh()
  held.state.songs = [song(1), song(2, { title: 'Otra' }), song(3, { title: 'Tercera' })]
  held.state.playlistSongs = held.state.songs.map((s) => ({ ...s }))
  closeMenu()
  dialogCancel()
  clearNotices()
  vi.clearAllMocks()
})

describe('el menú de una canción', () => {
  it('ofrece lo de siempre y la elige', async () => {
    const { menus, ctx } = prepare()
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
      'Separar pistas',
      'Abrir la carpeta',
      'Mandar a la papelera…'
    ])
    expect(ctx.selection.selectedIds.value).toEqual([1])
    await flushPromises()
    expect(ctx.selection.selected.value).toBe(1)
  })

  it('sobre la que suena ofrece pausar; en un repertorio, quitarla; sin panel, la ficha', async () => {
    const { menus, ctx } = prepare({
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
    const { menus, ctx } = prepare()
    menus.songMenu(ev, song(1, { stars: 3 }))
    const rate = item('Valorar')
    expect(rate.note).toBe('3 de 5')
    expect(rate.children.map((c) => c.label)).toEqual([
      '5 estrellas',
      '4 estrellas',
      '3 estrellas',
      '2 estrellas',
      '1 estrella',
      'Sin valorar'
    ])
    expect(rate.children.find((c) => c.note === 'ahora').label).toBe('3 estrellas')
    await rate.children[0].action()
    expect(api().setStars).toHaveBeenCalledWith(1, 5)
    expect(ctx.onUpdated).toHaveBeenCalledWith(expect.objectContaining({ id: 1, stars: 5 }))
  })

  it('la papelera pregunta antes, y cancelar no borra nada', async () => {
    const { menus, ctx } = prepare({ playing: 1 })
    menus.songMenu(ev, held.state.songs[0])
    const done = pick('Mandar a la papelera…')
    await flushPromises()
    expect(dialog.value.open).toBe(true)
    expect(dialog.value.danger).toBe(true)
    dialogCancel()
    await done
    expect(api().deleteSong).not.toHaveBeenCalled()

    menus.songMenu(ev, held.state.songs[0])
    const other = pick('Mandar a la papelera…')
    await flushPromises()
    dialogOk()
    await other
    expect(api().deleteSong).toHaveBeenCalledWith(1, { fromLists: false })
    // era la que sonaba: se para
    expect(ctx.player.stop).toHaveBeenCalled()
    expect(ctx.refreshAll).toHaveBeenCalled()
  })

  it('si está en listas, pregunta si sale también de ellas', async () => {
    const { menus } = prepare()
    // sin listas, ni se pregunta
    menus.songMenu(ev, held.state.songs[0])
    let done = pick('Mandar a la papelera…')
    await flushPromises()
    expect(dialog.value.check).toBeUndefined()
    dialogCancel()
    await done

    held.state.songs[0].playlists = ['Domingo', 'Ensayo']
    held.state.playlistSongs = [held.state.songs[0]]
    menus.songMenu(ev, held.state.songs[0])
    done = pick('Mandar a la papelera…')
    await flushPromises()
    expect(dialog.value.message).toContain('«No está»')
    expect(dialog.value.check).toEqual({
      label: 'Quitarla también de sus 2 listas («Domingo», «Ensayo»)',
      value: true
    })
    dialogOk({ checked: true })
    await done
    expect(api().deleteSong).toHaveBeenCalledWith(1, { fromLists: true })
    expect(notices.value.at(-1).message).toContain('fuera de su lista')
  })

  it('dejarla en sus listas la deja como «No está»', async () => {
    const { menus } = prepare()
    held.state.songs[0].playlists = ['Domingo']
    held.state.playlistSongs = [held.state.songs[0]]
    menus.songMenu(ev, held.state.songs[0])
    const done = pick('Mandar a la papelera…')
    await flushPromises()
    expect(dialog.value.check.label).toBe('Quitarla también de la lista («Domingo»)')
    dialogOk({ checked: false })
    await done
    expect(api().deleteSong).toHaveBeenCalledWith(1, { fromLists: false })
    expect(held.state.playlistSongs[0].ghost).toBe(true)
  })

  it('renombrar, buscar la letra y difuminar cambian la canción en todas partes', async () => {
    const { menus, ctx } = prepare()
    menus.songMenu(ev, held.state.songs[0])
    const rename = pick('Renombrar…')
    await flushPromises()
    expect(dialog.value.kind).toBe('prompt')
    dialogOk('Mi Gozo')
    await rename
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
    const { menus } = prepare()
    await menus.loadShareTargets()
    const noPath = { ...held.state.songs[0], path: '' }
    menus.songMenu(ev, noPath)
    await pick('Abrir la carpeta')
    expect(held.app.revealInFolder).toHaveBeenCalledWith('/musica/cancion-1.mp3')
    menus.songMenu(ev, noPath)
    await pick('Enviar por Telegram')
    expect(held.app.sendToTelegram).toHaveBeenCalledWith(['/musica/cancion-1.mp3'])
    // si el explorador no se puede abrir, se dice
    held.app.revealInFolder.mockRejectedValueOnce(new Error('no hay explorador'))
    menus.songMenu(ev, noPath)
    await pick('Abrir la carpeta')
    expect(notices.value.at(-1).message).toContain('no hay explorador')
  })
})

describe('las «No está» de una lista', () => {
  const list = { kind: 'playlist', id: 7, name: 'Domingo' }
  const gone = () => song(9, { title: 'Se Fue', artist: 'Barak', ghost: true })

  it('tienen su menú: buscarla en lo que tienes, bajarla o quitarla', async () => {
    const { menus, ctx } = prepare({ view: list })
    ctx.findInLibrary = vi.fn()
    ctx.findToDownload = vi.fn()
    menus.songMenu(ev, gone())
    expect(menu.value.title).toBe('Se Fue · No está')
    expect(labels()).toEqual([
      'Buscar en tu biblioteca',
      'Buscar y descargar…',
      'Quitar de esta lista'
    ])
    await pick('Buscar en tu biblioteca')
    expect(ctx.findInLibrary).toHaveBeenCalledWith('Barak Se Fue')
    menus.songMenu(ev, gone())
    await pick('Buscar y descargar…')
    expect(ctx.findToDownload).toHaveBeenCalledWith('Barak Se Fue')
    menus.songMenu(ev, gone())
    await pick('Quitar de esta lista')
    expect(api().removeFromPlaylist).toHaveBeenCalledWith(7, 9)
  })

  it('entre varias elegidas, solo cuentan para quitarlas de la lista', async () => {
    const { menus } = prepare({ view: list })
    const several = [held.state.songs[0], gone()]
    menus.groupMenu(ev, several)
    expect(menu.value.title).toBe('Cancion 1')
    expect(labels()).toContain('Quitar 2 de esta lista')
    await pick('Quitar 2 de esta lista')
    expect(api().removeFromPlaylist.mock.calls).toEqual([
      [7, 1],
      [7, 9]
    ])
    // la papelera, las favoritas y Telegram no las tocan
    await menus.trash([gone()])
    expect(dialog.value.open).toBe(false)
    await menus.favorites([gone()])
    expect(api().toggleFavorite).not.toHaveBeenCalled()
  })

  it('solo «No está» elegidas: solo se pueden quitar', () => {
    const { menus } = prepare({ view: list })
    menus.groupMenu(ev, [gone(), song(10, { ghost: true })])
    expect(menu.value.title).toBe('2 que no están')
    expect(labels()).toEqual(['Quitar 2 de esta lista'])
  })
})

describe('el menú de varias', () => {
  it('sobre una de las elegidas, el menú es el de todas', async () => {
    const { menus, ctx } = prepare({ view: { kind: 'playlist', id: 7, name: 'Domingo' } })
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
      'Separar las pistas de 3',
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
    const { menus, ctx } = prepare({ view: { kind: 'playlist', id: 7, name: 'Domingo' } })
    const three = held.state.songs.map((s) => ({ ...s }))
    menus.groupMenu(ev, three)
    await pick('Marcar 3 como favoritas')
    expect(api().toggleFavorite).toHaveBeenCalledTimes(3)
    menus.groupMenu(ev, three)
    await pick('Quitar 3 de esta lista')
    expect(api().removeFromPlaylist).toHaveBeenCalledTimes(3)
    expect(ctx.reload).toHaveBeenCalledWith(true)
    menus.groupMenu(ev, three)
    const trash = pick('Mandar 3 a la papelera…')
    await flushPromises()
    expect(dialog.value.title).toBe('Mandar 3 canciones a la papelera')
    dialogOk()
    await trash
    expect(api().deleteSong).toHaveBeenCalledTimes(3)
    expect(ctx.selection.selectedIds.value).toEqual([])
  })

  it('la papelera desde el buscador no suelta lo elegido en la lista', async () => {
    const { menus, ctx } = prepare()
    const [a, b, c] = held.state.songs
    ctx.selection.selectedIds.value = [c.id]
    menus.groupMenu(ev, [a, b], { fromSearch: true })
    const trash = pick('Mandar 2 a la papelera…')
    await flushPromises()
    dialogOk()
    await trash
    expect(api().deleteSong).toHaveBeenCalledTimes(2)
    expect(ctx.selection.selectedIds.value).toEqual([c.id])
  })

  it('las listas a las que mandarlas, y una nueva', async () => {
    const { menus, ctx } = prepare()
    const two = held.state.songs.slice(0, 2)
    menus.groupMenu(ev, two)
    const targets = item('Añadir 2 a una lista').children
    expect(targets.filter((d) => !d.separator).map((d) => d.label)).toEqual([
      'Domingo',
      'Nueva lista…'
    ])
    await targets[0].action()
    expect(ctx.playlistActions.addManyTo).toHaveBeenCalledWith(
      two,
      expect.objectContaining({ id: 7 })
    )
    await targets.at(-1).action()
    expect(ctx.playlistActions.create).toHaveBeenCalledWith(two)
  })
})

describe('el menú de un repertorio', () => {
  it('abrir, renombrar, exportar, la hoja y borrarlo', async () => {
    const { menus, ctx } = prepare({ view: { kind: 'playlist', id: 7, name: 'Domingo' } })
    const pl = { id: 7, name: 'Domingo' }
    await menus.loadShareTargets()
    menus.playlistMenu(ev, pl)
    expect(labels()).toEqual([
      'Abrir',
      'Renombrar…',
      'Exportar a .m3u',
      'Hoja para el atril…',
      'Separar las pistas del repertorio',
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
    const { menus } = prepare()
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

describe('las pistas separadas', () => {
  it('una sin pistas se separa con un solo clic, sin elegir nada', async () => {
    const { menus } = prepare()
    menus.songMenu(ev, held.state.songs[0])
    expect(item('Separar pistas').children).toBeUndefined()
    await pick('Separar pistas')
    expect(api().separate).toHaveBeenCalledWith(1)
    expect(dialog.value.open).toBeFalsy()
  })

  it('bajar el separador se pregunta antes, con lo que pesa', async () => {
    held.state.separation.pending = 307000000
    await useSeparation().refresh()
    const { menus } = prepare()
    menus.songMenu(ev, held.state.songs[0])
    expect(item('Separar pistas').note).toBe('bajar 307 MB')
    const asked = pick('Separar pistas')
    await flushPromises()
    expect(dialog.value.open).toBe(true)
    expect(dialog.value.message).toContain('307 MB')
    dialogCancel()
    await asked
    expect(api().separate).not.toHaveBeenCalled()
  })

  it('varias a la vez van a la cola', async () => {
    const { menus, ctx } = prepare()
    await ctx.selection.select(1)
    await ctx.selection.select(3, { shiftKey: true })
    menus.songMenu(ev, held.state.songs[0])
    await pick('Separar las pistas de 3')
    expect(api().separate.mock.calls.map((c) => c[0])).toEqual([1, 2, 3])
    expect(useSeparation().state.queue.map((q) => q.id)).toEqual([2, 3])
  })

  it('el repertorio entero, sin repetir las que ya tienen las mejores', async () => {
    const { menus } = prepare()
    held.state.playlistSongs[1] = {
      ...held.state.playlistSongs[1],
      has_stems: true,
      stems_best: true
    }
    held.state.playlistSongs[2] = { ...held.state.playlistSongs[2], has_stems: true }
    menus.playlistMenu(ev, { id: 7, name: 'Domingo' })
    await pick('Separar las pistas del repertorio')
    expect(api().separate.mock.calls.map((c) => c[0])).toEqual([1, 3])
    held.state.playlistSongs = held.state.playlistSongs.map((s) => ({
      ...s,
      has_stems: true,
      stems_best: true
    }))
    menus.playlistMenu(ev, { id: 7, name: 'Domingo' })
    await pick('Separar las pistas del repertorio')
    expect(notices.value.at(-1).message).toBe(
      'Todas las canciones de esa lista ya tienen sus pistas'
    )
  })

  it('la que se está separando o espera se puede parar o quitar', async () => {
    const { menus } = prepare()
    await useSeparation().request([held.state.songs[0], held.state.songs[1]])
    menus.songMenu(ev, held.state.songs[1])
    await pick('Quitar de la cola de separar')
    expect(api().unqueueSeparation).toHaveBeenCalledWith(2)
    menus.songMenu(ev, held.state.songs[0])
    await pick('Parar la separación')
    expect(api().cancelSeparation).toHaveBeenCalled()
  })

  it('con pistas: abrir su carpeta, separarla otra vez o borrarlas', async () => {
    const { menus, ctx } = prepare()
    held.state.stems[1] = stemsOf(1)
    const withStems = { ...held.state.songs[0], has_stems: true, stems_best: true }
    menus.songMenu(ev, withStems)
    expect(labels()).toContain('Separar otra vez')
    expect(labels()).not.toContain('Separar pistas')
    // las rápidas, o las de antes, se ofrecen hacer con la mejor calidad
    menus.songMenu(ev, { ...withStems, stems_best: false })
    expect(labels()).toContain('Separar otra vez con la mejor calidad')
    closeMenu()
    await pick('Abrir la carpeta de sus pistas')
    expect(held.app.revealInFolder).toHaveBeenCalledWith('/musica/Separadas/cancion-1/Bateria.flac')
    menus.songMenu(ev, withStems)
    const deleting = pick('Borrar sus pistas…')
    await flushPromises()
    expect(dialog.value.danger).toBe(true)
    dialogOk()
    await deleting
    expect(api().deleteStems).toHaveBeenCalledWith(1)
    expect(ctx.refreshAll).toHaveBeenCalled()
  })

  it('al celular: una desde su menú, y de varias solo las que tienen pistas', async () => {
    const { menus, ctx, songs } = prepare()
    held.state.stems[1] = stemsOf(1)
    held.state.stems[3] = stemsOf(3)
    const withStems = { ...held.state.songs[0], has_stems: true, stems_best: true }
    menus.songMenu(ev, withStems)
    await pick('Convertir a DanPista (para el celular)')
    expect(api().toPhone).toHaveBeenCalledWith([1])
    expect(notices.value.at(-1).message).toBe('Lista para el celular: «cancion-1.danpista» (23 MB)')
    // la carpeta se abre con el archivo marcado: de ahí se arrastra a WhatsApp
    expect(held.app.revealInFolder).toHaveBeenCalledWith('/musica/Celular/cancion-1.danpista')
    songs.value[0].has_stems = true
    songs.value[2].has_stems = true
    await ctx.selection.select(1)
    await ctx.selection.select(3, { shiftKey: true })
    menus.songMenu(ev, held.state.songs[0])
    expect(item('Convertir 2 a DanPista (para el celular)').note).toBe('1 sin pistas')
    await pick('Convertir 2 a DanPista (para el celular)')
    expect(api().toPhone).toHaveBeenLastCalledWith([1, 3])
    expect(notices.value.at(-1).message).toBe(
      '2 listas para el celular, en la carpeta «Celular»; 1 sin clic'
    )
  })

  it('sin separador en este equipo no se ofrece', async () => {
    held.state.separation.ok = false
    held.state.separation.reason = 'falta onnxruntime'
    await useSeparation().refresh()
    const { menus } = prepare()
    menus.songMenu(ev, held.state.songs[0])
    expect(labels().some((l) => l.startsWith('Separar'))).toBe(false)
    held.state.separation.ok = true
  })
})
