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
const avisos = () => useNotices().notices.value.map((n) => n.message)
const dialogo = () => useDialog().dialog.value

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
async function responder(valor) {
  await flushPromises()
  if (valor === null) dialogCancel()
  else dialogOk(valor)
  await flushPromises()
}

describe('las listas', () => {
  it('se cargan del nucleo', async () => {
    await actions.load()
    expect(actions.playlists.value.map((l) => l.name)).toEqual(['Domingo', 'Ensayo'])
  })

  it('añadir una cancion lo dice; si ya estaba, tambien', async () => {
    await actions.addTo(song(7), { id: 2, name: 'Ensayo' })
    expect(api.addToPlaylist).toHaveBeenCalledWith(2, [7])
    expect(avisos()).toContain('Añadida a «Ensayo»')
    api.addToPlaylist.mockResolvedValueOnce({ added: 0 })
    await actions.addTo(song(7), { id: 2, name: 'Ensayo' })
    expect(avisos()).toContain('Ya estaba en «Ensayo»')
  })

  it('varias de una vez cuentan cuantas entraron', async () => {
    api.addToPlaylist.mockResolvedValueOnce({ added: 3 })
    await actions.addManyTo([song(1), song(2), song(3)], { id: 2, name: 'Ensayo' })
    expect(api.addToPlaylist).toHaveBeenCalledWith(2, [1, 2, 3])
    expect(avisos()).toContain('3 añadidas a «Ensayo»')
  })
})

describe('crear una lista', () => {
  it('pide el nombre y, si viene con canciones, las mete dentro', async () => {
    const hecho = actions.create([song(4), song(5)])
    await flushPromises()
    expect(dialogo().kind).toBe('prompt')
    expect(dialogo().message).toContain('2 canciones')
    await responder('Viernes')
    const r = await hecho
    expect(api.createPlaylist).toHaveBeenCalledWith('Viernes')
    expect(api.addToPlaylist).toHaveBeenCalledWith(r.id, [4, 5])
    expect(avisos()).toContain('Lista «Viernes» creada con 2 canciones')
    expect(actions.playlists.value.map((l) => l.name)).toContain('Viernes')
  })

  it('con una sola cancion, o con ninguna', async () => {
    let hecho = actions.create(song(4, { title: 'Mi Gozo' }))
    await flushPromises()
    expect(dialogo().message).toContain('«Mi Gozo»')
    await responder('Sabado')
    await hecho
    expect(avisos()).toContain('Lista «Sabado» creada con esa canción')
    hecho = actions.create()
    await responder('Vacia')
    await hecho
    expect(avisos()).toContain('Lista «Vacia» creada')
  })

  it('si el nombre ya existe, se usa esa y se dice', async () => {
    const hecho = actions.create()
    await responder('Domingo')
    await hecho
    expect(avisos()).toContain('Ya existía una lista «Domingo»: se ha usado esa')
  })

  it('cancelar no crea nada', async () => {
    const hecho = actions.create()
    await responder(null)
    expect(await hecho).toBe(null)
    expect(api.createPlaylist).not.toHaveBeenCalled()
  })
})

describe('renombrar', () => {
  it('cambia el nombre y, si esta abierta, el titulo la sigue', async () => {
    api.editPlaylist.mockResolvedValueOnce({ playlist: { id: 1, name: 'Domingo 11h' } })
    const hecho = actions.rename({ id: 1, name: 'Domingo' })
    await responder(' Domingo 11h ')
    await hecho
    expect(api.editPlaylist).toHaveBeenCalledWith(1, { name: 'Domingo 11h' })
    expect(view.value.name).toBe('Domingo 11h')
    expect(avisos()).toContain('Ahora se llama «Domingo 11h»')
  })

  it('el mismo nombre no pide nada al nucleo', async () => {
    const hecho = actions.rename({ id: 1, name: 'Domingo' })
    await responder('Domingo')
    await hecho
    expect(api.editPlaylist).not.toHaveBeenCalled()
  })

  it('si el nombre ya es de otra, lo dice', async () => {
    api.editPlaylist.mockRejectedValueOnce(new Error('ya hay una lista con ese nombre'))
    const hecho = actions.rename({ id: 2, name: 'Ensayo' })
    await responder('Domingo')
    await hecho
    expect(avisos()).toContain('No se pudo renombrar: ya hay una lista con ese nombre')
  })
})

describe('quitar, exportar y la hoja del atril', () => {
  it('quitar una cancion de la lista abierta recarga la lista', async () => {
    await actions.removeSong(song(3))
    expect(api.removeFromPlaylist).toHaveBeenCalledWith(1, 3)
    expect(reload).toHaveBeenCalled()
    expect(avisos()).toContain('Quitada de la lista')
  })

  it('exportar dice donde quedo; si falla, por que', async () => {
    await actions.exportTo({ id: 1, name: 'Domingo' })
    expect(avisos()).toContain('Exportada a /musica/Listas/x.m3u8')
    api.exportPlaylist.mockRejectedValueOnce(new Error('sin permiso'))
    await actions.exportTo({ id: 1, name: 'Domingo' })
    expect(avisos()).toContain('No se pudo exportar: sin permiso')
  })

  it('la hoja pregunta si lleva letra y se abre en el navegador', async () => {
    const hecho = actions.sheet({ id: 1, name: 'Domingo' })
    await flushPromises()
    expect(dialogo().okLabel).toBe('Con letra')
    await responder(true)
    await hecho
    expect(api.playlistSheet).toHaveBeenCalledWith(1, true)
    expect(held.app.openHtml).toHaveBeenCalledWith('/musica/Listas/lista-1.html')
  })

  it('«solo acordes» la pide sin letra; si no se puede abrir, se enseña en su carpeta', async () => {
    held.app.openHtml.mockRejectedValueOnce(new Error('sin navegador'))
    const hecho = actions.sheet({ id: 1, name: 'Domingo' })
    await responder(null)
    await hecho
    expect(api.playlistSheet).toHaveBeenCalledWith(1, false)
    expect(held.app.revealInFolder).toHaveBeenCalledWith('/musica/Listas/lista-1.html')
  })

  it('si no se puede escribir la hoja, lo dice', async () => {
    api.playlistSheet.mockRejectedValueOnce(new Error('disco lleno'))
    const hecho = actions.sheet({ id: 1, name: 'Domingo' })
    await responder(true)
    await hecho
    expect(avisos()).toContain('No se pudo escribir la hoja: disco lleno')
  })
})

describe('borrar una lista', () => {
  it('pregunta antes, deja claro que las canciones no se borran y devuelve cual era', async () => {
    const hecho = actions.remove({ id: 2, name: 'Ensayo' })
    await flushPromises()
    expect(dialogo().danger).toBe(true)
    expect(dialogo().message).toContain('Las canciones NO se borran')
    await responder(true)
    expect(await hecho).toBe(2)
    expect(api.deletePlaylist).toHaveBeenCalledWith(2)
    expect(actions.playlists.value.map((l) => l.id)).toEqual([1])
  })

  it('cancelar no borra nada', async () => {
    const hecho = actions.remove(1)
    await responder(null)
    expect(await hecho).toBe(false)
    expect(api.deletePlaylist).not.toHaveBeenCalled()
  })
})
