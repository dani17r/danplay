import { describe, it, expect, vi, beforeEach } from 'vitest'
import { ref } from 'vue'
import { flushPromises } from '@vue/test-utils'

// Lo que se hace con un repertorio: crearlo, llenarlo, renombrarlo, sacarlo
// a un .m3u o a la hoja del atril, y borrarlo. Cada accion se cuenta con un
// aviso: son cosas que la persona pide y espera ver confirmadas.
const held = vi.hoisted(() => ({ state: null, api: null, app: null }))

vi.mock('../src/api.js', async (importOriginal) => {
  const actual = await importOriginal()
  const { createState, buildApiDouble, createAppDouble } = await import('./support/backend.js')
  const { vi: v } = await import('vitest')
  held.state = createState()
  held.api = buildApiDouble(actual, held.state)
  held.app = createAppDouble({
    openHtml: v.fn(async () => {}),
    revealInFolder: v.fn(async () => {})
  })
  return { ...actual, api: held.api, app: held.app }
})

import { usePlaylistActions } from '../src/composables/usePlaylistActions.js'
import { dialogOk, dialogCancel, useDialog } from '../src/composables/useDialog.js'
import { clearNotices, useNotices } from '../src/composables/useNotices.js'
import { song } from './support/backend.js'

const api = held.api
const notices = () => useNotices().notices.value.map((n) => n.message)
const dialog = () => useDialog().dialog.value

let view, reload, actions
beforeEach(() => {
  vi.clearAllMocks()
  clearNotices()
  dialogCancel()
  held.state.playlists = [
    { id: 1, name: 'Domingo', n: 2 },
    { id: 2, name: 'Ensayo', n: 0 }
  ]
  view = ref({ kind: 'playlist', id: 1, name: 'Domingo' })
  reload = vi.fn(async () => {})
  actions = usePlaylistActions({ view, reload })
})

/** Contesta el dialogo que se acaba de abrir. */
async function answer(value) {
  await flushPromises()
  if (value === null) dialogCancel()
  else dialogOk(value)
  await flushPromises()
}

describe('playlists', () => {
  it('are loaded from the core', async () => {
    await actions.load()
    expect(actions.playlists.value.map((l) => l.name)).toEqual(['Domingo', 'Ensayo'])
  })

  it('adding a song says so; if it was already there, too', async () => {
    await actions.addTo(song(7), { id: 2, name: 'Ensayo' })
    expect(api.addToPlaylist).toHaveBeenCalledWith(2, [7])
    expect(notices()).toContain('Añadida a «Ensayo»')
    api.addToPlaylist.mockResolvedValueOnce({ added: 0 })
    await actions.addTo(song(7), { id: 2, name: 'Ensayo' })
    expect(notices()).toContain('Ya estaba en «Ensayo»')
  })

  it('several at once report how many got in', async () => {
    api.addToPlaylist.mockResolvedValueOnce({ added: 3 })
    await actions.addManyTo([song(1), song(2), song(3)], { id: 2, name: 'Ensayo' })
    expect(api.addToPlaylist).toHaveBeenCalledWith(2, [1, 2, 3])
    expect(notices()).toContain('3 añadidas a «Ensayo»')
  })
})

describe('creating a playlist', () => {
  it('asks for the name and, if it comes with songs, adds them', async () => {
    const done = actions.create([song(4), song(5)])
    await flushPromises()
    expect(dialog().kind).toBe('prompt')
    expect(dialog().message).toContain('2 canciones')
    await answer('Viernes')
    const r = await done
    expect(api.createPlaylist).toHaveBeenCalledWith('Viernes')
    expect(api.addToPlaylist).toHaveBeenCalledWith(r.id, [4, 5])
    expect(notices()).toContain('Lista «Viernes» creada con 2 canciones')
    expect(actions.playlists.value.map((l) => l.name)).toContain('Viernes')
  })

  it('with a single song, or with none', async () => {
    let done = actions.create(song(4, { title: 'Mi Gozo' }))
    await flushPromises()
    expect(dialog().message).toContain('«Mi Gozo»')
    await answer('Sabado')
    await done
    expect(notices()).toContain('Lista «Sabado» creada con esa canción')
    done = actions.create()
    await answer('Vacia')
    await done
    expect(notices()).toContain('Lista «Vacia» creada')
  })

  it('if the name already exists, that one is used and it says so', async () => {
    const done = actions.create()
    await answer('Domingo')
    await done
    expect(notices()).toContain('Ya existía una lista «Domingo»: se ha usado esa')
  })

  it('cancelling creates nothing', async () => {
    const done = actions.create()
    await answer(null)
    expect(await done).toBe(null)
    expect(api.createPlaylist).not.toHaveBeenCalled()
  })
})

describe('renaming', () => {
  it('changes the name and, if it is open, the title follows', async () => {
    api.editPlaylist.mockResolvedValueOnce({ playlist: { id: 1, name: 'Domingo 11h' } })
    const done = actions.rename({ id: 1, name: 'Domingo' })
    await answer(' Domingo 11h ')
    await done
    expect(api.editPlaylist).toHaveBeenCalledWith(1, { name: 'Domingo 11h' })
    expect(view.value.name).toBe('Domingo 11h')
    expect(notices()).toContain('Ahora se llama «Domingo 11h»')
  })

  it('the same name requests nothing from the core', async () => {
    const done = actions.rename({ id: 1, name: 'Domingo' })
    await answer('Domingo')
    await done
    expect(api.editPlaylist).not.toHaveBeenCalled()
  })

  it('if the name belongs to another one, it says so', async () => {
    api.editPlaylist.mockRejectedValueOnce(new Error('ya hay una lista con ese nombre'))
    const done = actions.rename({ id: 2, name: 'Ensayo' })
    await answer('Domingo')
    await done
    expect(notices()).toContain('No se pudo renombrar: ya hay una lista con ese nombre')
  })
})

describe('removing, exporting and the music stand sheet', () => {
  it('removing a song from the open playlist reloads it', async () => {
    await actions.removeSong(song(3))
    expect(api.removeFromPlaylist).toHaveBeenCalledWith(1, 3)
    expect(reload).toHaveBeenCalled()
    expect(notices()).toContain('Quitada de la lista')
  })

  it('exporting says where it was saved; if it fails, why', async () => {
    await actions.exportTo({ id: 1, name: 'Domingo' })
    expect(notices()).toContain('Exportada a /musica/Listas/x.m3u8')
    api.exportPlaylist.mockRejectedValueOnce(new Error('sin permiso'))
    await actions.exportTo({ id: 1, name: 'Domingo' })
    expect(notices()).toContain('No se pudo exportar: sin permiso')
  })

  it('the sheet asks whether to include lyrics and opens in the browser', async () => {
    const done = actions.sheet({ id: 1, name: 'Domingo' })
    await flushPromises()
    expect(dialog().okLabel).toBe('Con letra')
    await answer(true)
    await done
    expect(api.playlistSheet).toHaveBeenCalledWith(1, true)
    expect(held.app.openHtml).toHaveBeenCalledWith('/musica/Listas/lista-1.html')
  })

  it('«solo acordes» requests it without lyrics; if it cannot be opened, it is shown in its folder', async () => {
    held.app.openHtml.mockRejectedValueOnce(new Error('sin navegador'))
    const done = actions.sheet({ id: 1, name: 'Domingo' })
    await answer(null)
    await done
    expect(api.playlistSheet).toHaveBeenCalledWith(1, false)
    expect(held.app.revealInFolder).toHaveBeenCalledWith('/musica/Listas/lista-1.html')
  })

  it('if the sheet cannot be written, it says so', async () => {
    api.playlistSheet.mockRejectedValueOnce(new Error('disco lleno'))
    const done = actions.sheet({ id: 1, name: 'Domingo' })
    await answer(true)
    await done
    expect(notices()).toContain('No se pudo escribir la hoja: disco lleno')
  })
})

describe('deleting a playlist', () => {
  it('asks first, makes clear the songs are not deleted and returns which one it was', async () => {
    const done = actions.remove({ id: 2, name: 'Ensayo' })
    await flushPromises()
    expect(dialog().danger).toBe(true)
    expect(dialog().message).toContain('Las canciones NO se borran')
    await answer(true)
    expect(await done).toBe(2)
    expect(api.deletePlaylist).toHaveBeenCalledWith(2)
    expect(actions.playlists.value.map((l) => l.id)).toEqual([1])
  })

  it('cancelling deletes nothing', async () => {
    const done = actions.remove(1)
    await answer(null)
    expect(await done).toBe(false)
    expect(api.deletePlaylist).not.toHaveBeenCalled()
  })
})
