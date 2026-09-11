// Lo que se puede hacer con un repertorio: crearlo, añadirle canciones,
// quitarlas, exportarlo y borrarlo. Estaba dentro de App.vue.
//
// Cada acción se explica sola con un aviso flotante: son cosas que el usuario
// pide y espera ver confirmadas.
import { ref } from 'vue'
import { api, errorMessage } from '../api.js'
import { notify } from './useNotices.js'
import { ask } from './useDialog.js'

/**
 * @param {{
 *   view: import('vue').Ref<any>,
 *   reload: () => any,       vuelve a pedir la lista de la página
 * }} context
 */
export function usePlaylistActions(context) {
  const playlists = ref([])

  async function load() {
    playlists.value = (await api.playlists()).playlists
  }

  async function addTo(song, playlist) {
    const r = await api.addToPlaylist(playlist.id, [song.id])
    notify(
      r.added ? `Añadida a «${playlist.name}»` : `Ya estaba en «${playlist.name}»`,
      r.added ? 'ok' : 'info'
    )
    load()
  }

  /** Varias canciones a una lista de una vez. */
  async function addManyTo(list, playlist) {
    const r = await api.addToPlaylist(playlist.id, list.map((s) => s.id))
    notify(
      r.added ? `${r.added} añadidas a «${playlist.name}»` : `Ya estaban en «${playlist.name}»`,
      r.added ? 'ok' : 'info'
    )
    load()
  }

  /** Crea una lista, y si se pasa una canción (o varias) las mete dentro. */
  async function create(songOrList = null) {
    const list = Array.isArray(songOrList) ? songOrList : songOrList ? [songOrList] : []
    const song = list[0] || null
    const name = await ask({
      kind: 'prompt',
      title: 'Nueva lista',
      message:
        list.length > 1
          ? `Se creará la lista y se añadirán ${list.length} canciones.`
          : song
            ? `Se creará la lista y se añadirá «${song.title || song.file}».`
            : 'Cómo se va a llamar el repertorio.',
      placeholder: 'Domingo por la mañana',
      okLabel: 'Crear'
    })
    if (!name) return null
    const r = await api.createPlaylist(name)
    if (list.length && r?.id) await api.addToPlaylist(r.id, list.map((s) => s.id))
    await load()
    // El núcleo devuelve la que ya había si el nombre se repite: decirlo es
    // más honrado que fingir que se ha creado una.
    if (r?.created === false) notify(`Ya existía una lista «${name}»: se ha usado esa`, 'info')
    else if (list.length > 1) notify(`Lista «${name}» creada con ${list.length} canciones`, 'ok')
    else notify(song ? `Lista «${name}» creada con esa canción` : `Lista «${name}» creada`, 'ok')
    return r
  }

  /** Cambia el nombre de una lista. Si está abierta, el título la sigue. */
  async function rename(playlist) {
    const name = await ask({
      kind: 'prompt',
      title: 'Renombrar la lista',
      message: 'Nuevo nombre del repertorio.',
      value: playlist.name,
      placeholder: playlist.name,
      okLabel: 'Guardar'
    })
    if (!name || name.trim() === playlist.name) return
    try {
      const r = await api.editPlaylist(playlist.id, { name: name.trim() })
      const v = context.view.value
      if (v?.kind === 'playlist' && v.id === playlist.id) {
        context.view.value = { ...v, name: r.playlist?.name || name.trim() }
      }
      await load()
      notify(`Ahora se llama «${r.playlist?.name || name.trim()}»`, 'ok')
    } catch (e) {
      notify('No se pudo renombrar: ' + errorMessage(e))
    }
  }

  async function removeSong(song) {
    await api.removeFromPlaylist(context.view.value.id, song.id)
    notify('Quitada de la lista', 'ok')
    await Promise.all([context.reload(), load()])
  }

  async function exportTo(playlist) {
    try {
      const r = await api.exportPlaylist(playlist.id)
      notify(`Exportada a ${r.path || r.file || 'la biblioteca'}`, 'ok')
    } catch (e) {
      notify('No se pudo exportar: ' + errorMessage(e))
    }
  }

  async function remove(playlist) {
    const id = typeof playlist === 'object' ? playlist.id : playlist
    const name = typeof playlist === 'object' ? playlist.name : ''
    const ok = await ask({
      kind: 'confirm',
      title: 'Borrar la lista',
      danger: true,
      message:
        `Se borrará la lista${name ? ` «${name}»` : ''}.\n` +
        'Las canciones NO se borran: siguen en tu biblioteca.',
      okLabel: 'Borrar la lista'
    })
    if (!ok) return false
    await api.deletePlaylist(id)
    await load()
    return id
  }

  return { playlists, load, addTo, addManyTo, create, rename, removeSong, exportTo, remove }
}
