// @ts-check
// Los menús de clic derecho (el de una canción, el de varias elegidas y el de
// un repertorio) y lo que se hace desde ellos con las canciones: valorar,
// favoritas, difuminar la portada, renombrar, buscar letra, separarlas en
// pistas, la papelera, abrir su carpeta o mandarlas por Telegram. Estaba
// dentro de App.vue.
import { ref } from 'vue'
import { api, app as tauriApp, errorMessage } from '../api.js'
import { notify } from './useNotices.js'
import { ask } from './useDialog.js'
import { openMenu } from './useContextMenu.js'
import { useSeparation } from './useSeparation.js'

/** @typedef {import('../api.js').Song} Song */
/** @typedef {import('./useContextMenu.js').MenuItem} MenuItem */

/**
 * @typedef {Object} SongMenusContext
 * @property {import('vue').Ref<{kind: string, id?: number, name?: string}>} view
 * @property {ReturnType<typeof import('./usePlayback.js').usePlayback>} player
 * @property {ReturnType<typeof import('./usePlaylistActions.js').usePlaylistActions>} playlistActions
 * @property {ReturnType<typeof import('./useSelection.js').useSelection>} selection
 * @property {(song: Song, list?: Song[]|null, origin?: any) => any} play
 * @property {(song: Song) => void} onUpdated  una canción cambió (en la lista, la ficha y la cola)
 * @property {() => Promise<any>} refreshAll
 * @property {(quiet?: boolean) => Promise<any>} reload  la lista de la vista
 * @property {() => boolean} detailsInView  la ficha ya se ve en el panel lateral
 * @property {(song: Song) => any} showDetailsOf  abre la ficha (en el cajón o en una ventana)
 */

/** @param {SongMenusContext} ctx */
export function useSongMenus(ctx) {
  const { view, player, playlistActions, selection } = ctx
  const { playlists } = playlistActions

  // A dónde se puede enviar una canción desde este equipo (Telegram, si está
  // instalado). Se mira una vez al arrancar; sin destino, la opción no aparece.
  const shareTargets = ref({ telegram: false })
  function loadShareTargets() {
    return tauriApp
      .shareTargets()
      .then((t) => (shareTargets.value = t || { telegram: false }))
      .catch(() => {})
  }

  const isPlaying = (/** @type {Song} */ song) => player.track.value?.id === song.id
  const separation = useSeparation()

  // -------------------------------------------------------------- acciones
  /** @param {Song} song @param {number} n */
  async function setStars(song, n) {
    ctx.onUpdated(await api.setStars(song.id, n))
  }
  /** @param {Song} song */
  async function toggleFavorite(song) {
    ctx.onUpdated(await api.toggleFavorite(song.id, !song.favorite))
  }

  /** Difumina la portada, o le quita el difuminado. La imagen no se toca. @param {Song} song */
  async function toggleBlur(song) {
    const c = await api.setBlur(song.id, !song.blur)
    ctx.onUpdated(c)
    notify(c.blur ? 'Portada difuminada' : 'Portada a la vista', 'ok', 3)
  }

  /** @param {Song} song */
  async function renameSong(song) {
    const title = await ask({
      kind: 'prompt',
      title: 'Renombrar',
      message: 'Título de la canción.',
      value: song.title || '',
      placeholder: 'Título',
      okLabel: 'Guardar'
    })
    if (!title || title === song.title) return
    ctx.onUpdated(await api.edit(song.id, { title }))
    notify('Renombrada', 'ok')
  }

  /** @param {Song} song */
  async function enrichSong(song) {
    notify('Buscando letra y portada…', 'info', 3)
    try {
      ctx.onUpdated((await api.enrich(song.id, { lyrics: true, cover: true, details: false })).song)
      notify('Listo', 'ok')
    } catch (e) {
      notify('No se pudo: ' + errorMessage(e))
    }
  }

  /** @param {Song} song */
  async function trashSong(song) {
    const ok = await ask({
      kind: 'confirm',
      title: 'Mandar a la papelera',
      danger: true,
      message:
        'El archivo va a la papelera del sistema, así que puedes recuperarlo desde ahí. ' +
        'También sale de la biblioteca.',
      detail: song.path || song.file,
      okLabel: 'A la papelera'
    })
    if (!ok) return
    try {
      const r = await api.deleteSong(song.id)
      if (isPlaying(song)) player.stop()
      notify(`«${r.name}» está en la papelera`, 'ok')
      await ctx.refreshAll()
    } catch (e) {
      notify('No se pudo borrar: ' + errorMessage(e))
    }
  }

  /** Varias a la papelera, con una sola confirmación que dice cuántas. @param {Song[]} list */
  async function trashSongs(list) {
    const n = list.length
    const ok = await ask({
      kind: 'confirm',
      title: `Mandar ${n} canciones a la papelera`,
      danger: true,
      message:
        'Los archivos van a la papelera del sistema, así que puedes recuperarlos desde ahí. ' +
        'También salen de la biblioteca.',
      detail:
        list
          .slice(0, 6)
          .map((s) => s.title || s.file)
          .join('\n') + (n > 6 ? `\n… y ${n - 6} más` : ''),
      okLabel: `A la papelera (${n})`
    })
    if (!ok) return
    let done = 0
    for (const s of list) {
      try {
        await api.deleteSong(s.id)
        if (isPlaying(s)) player.stop()
        done++
      } catch (e) {
        notify(`No se pudo borrar «${s.title || s.file}»: ${errorMessage(e)}`)
      }
    }
    selection.clear()
    if (done) notify(`${done} en la papelera`, 'ok')
    await ctx.refreshAll()
  }

  /** Abre el explorador del sistema señalando el archivo de la canción. @param {Song} song */
  async function revealSong(song) {
    const path = song?.path || (await api.song(song.id).catch(() => null))?.path
    if (!path) return notify('No sé dónde está ese archivo')
    try {
      await tauriApp.revealInFolder(path)
    } catch (e) {
      notify(errorMessage(e))
    }
  }

  /** Abre Telegram con los archivos de esas canciones listos para enviar. @param {Song[]} list */
  async function sendToTelegram(list) {
    const paths = []
    for (const song of list) {
      const path = song?.path || (await api.song(song.id).catch(() => null))?.path
      if (path) paths.push(path)
    }
    if (!paths.length) return notify('No sé dónde están esos archivos')
    try {
      await tauriApp.sendToTelegram(paths)
      notify(
        paths.length > 1
          ? `Telegram se ha abierto con ${paths.length} canciones: elige ahí a quién se las mandas`
          : 'Telegram se ha abierto: elige ahí a quién se la mandas',
        'ok'
      )
    } catch (e) {
      notify(errorMessage(e))
    }
  }

  /** Un repertorio entero a Telegram: todas sus canciones. @param {{id: number}} pl */
  async function sendPlaylistToTelegram(pl) {
    const list = (await api.playlistSongs(pl.id)).songs || []
    if (!list.length) return notify('Esa lista está vacía')
    await sendToTelegram(list)
  }

  /** Abre la carpeta de las pistas separadas de la canción. @param {Song} song */
  async function revealStems(song) {
    try {
      const first = (await api.stems(song.id))?.tracks?.[0]?.path
      if (!first) return notify('Esa canción ya no tiene sus pistas')
      await tauriApp.revealInFolder(first)
    } catch (e) {
      notify(errorMessage(e))
    }
  }

  /** Sus pistas separadas, a la papelera (la canción no se toca). @param {Song} song */
  async function deleteStems(song) {
    const ok = await ask({
      kind: 'confirm',
      title: 'Borrar las pistas separadas',
      danger: true,
      message: 'Las pistas van a la papelera del sistema. La canción no se toca.',
      detail: song.title || song.file,
      okLabel: 'A la papelera'
    })
    if (!ok) return
    try {
      await api.deleteStems(song.id)
      notify('Pistas en la papelera', 'ok')
      await ctx.refreshAll()
    } catch (e) {
      notify('No se pudieron borrar: ' + errorMessage(e))
    }
  }

  /**
   * Separar en pistas: un submenú con los modelos (6 o 4 pistas). Nada si
   * en este equipo no se puede.
   * @param {Song[]} list
   * @param {string} label
   * @returns {MenuItem[]}
   */
  function separateItems(list, label) {
    if (!separation.state.ok) return []
    return [
      {
        label,
        icon: 'mixer',
        children: separation.state.models.map((m) => ({
          label: `En ${m.label}`,
          note: m.installed === false ? `bajar ${separation.megas(m.bytes)}` : '',
          action: () => separation.request(list, m.id)
        }))
      }
    ]
  }

  /** Lo de las pistas de una canción: separarla, o lo que se hace con las suyas. @param {Song} song */
  function stemsItems(song) {
    const queued = separation.progressOf(song.id)
    if (queued) {
      return [
        {
          label: queued.waiting ? 'Quitar de la cola de separar' : 'Parar la separación',
          icon: 'mixer',
          action: () => (queued.waiting ? separation.unqueue(song.id) : separation.cancel())
        }
      ]
    }
    if (!song.has_stems) return separateItems([song], 'Separar en pistas')
    return [
      { label: 'Abrir la carpeta de sus pistas', icon: 'mixer', action: () => revealStems(song) },
      ...separateItems([song], 'Separar otra vez'),
      { label: 'Borrar sus pistas…', icon: 'trash', action: () => deleteStems(song) }
    ]
  }

  /** Borrar una lista puede dejarte mirando una vista que ya no existe. @param {any} pl */
  async function deletePlaylist(pl) {
    const id = await playlistActions.remove(pl)
    if (id && view.value.id === id) view.value = { kind: 'all' }
  }

  // ----------------------------------------------------------------- menús
  /**
   * Las listas a las que se puede mandar la canción, más «crear una nueva».
   * @param {Song|Song[]} songOrList
   * @returns {MenuItem[]}
   */
  function playlistTargets(songOrList) {
    const many = Array.isArray(songOrList) ? songOrList : null
    const song = many ? many[0] : /** @type {Song} */ (songOrList)
    /** @type {MenuItem[]} */
    const kids = playlists.value.map((l) => ({
      label: l.name,
      icon: 'list',
      note: String(l.n ?? ''),
      action: () => (many ? playlistActions.addManyTo(many, l) : playlistActions.addTo(song, l))
    }))
    if (kids.length) kids.push({ separator: true })
    kids.push({
      label: 'Nueva lista…',
      icon: 'plus',
      action: () => playlistActions.create(many || song)
    })
    return kids
  }

  /**
   * El menú sobre varias canciones seleccionadas: actúa sobre todas.
   * @param {MouseEvent|{clientX: number, clientY: number}} ev
   * @param {Song[]} list
   */
  function groupMenu(ev, list) {
    const n = list.length
    /** @type {MenuItem[]} */
    const items = [
      {
        label: `Reproducir estas ${n}`,
        icon: 'play',
        action: () => ctx.play(list[0], list, { ...view.value, label: 'la selección' })
      },
      { separator: true },
      { label: `Añadir ${n} a una lista`, icon: 'list', children: playlistTargets(list) },
      {
        label: `Marcar ${n} como favoritas`,
        icon: 'heart',
        action: async () => {
          for (const s of list) {
            if (!s.favorite) ctx.onUpdated(await api.toggleFavorite(s.id, true))
          }
          notify(`${n} favoritas`, 'ok')
        }
      }
    ]
    if (view.value.kind === 'playlist') {
      const listId = /** @type {number} */ (view.value.id)
      items.push({
        label: `Quitar ${n} de esta lista`,
        icon: 'close',
        action: async () => {
          for (const s of list) await api.removeFromPlaylist(listId, s.id)
          notify(`${n} quitadas de la lista`, 'ok')
          await Promise.all([ctx.reload(true), playlistActions.load()])
        }
      })
    }
    items.push(...separateItems(list, `Separar ${n} en pistas`))
    items.push({ separator: true })
    if (shareTargets.value.telegram) {
      items.push({
        label: `Enviar ${n} por Telegram`,
        icon: 'send',
        action: () => sendToTelegram(list)
      })
    }
    items.push({
      label: `Mandar ${n} a la papelera…`,
      icon: 'trash',
      danger: true,
      action: () => trashSongs(list)
    })
    openMenu(ev, items, `${n} canciones`)
  }

  /**
   * El menú de una canción (o el de todas, si es una de las elegidas).
   * @param {MouseEvent|{clientX: number, clientY: number}} ev
   * @param {Song} song
   */
  function songMenu(ev, song) {
    const chosen = selection.selectedSongs.value
    if (chosen.length > 1 && selection.selectedIds.value.includes(song.id)) {
      return groupMenu(ev, chosen)
    }
    selection.only(song.id)
    const current = isPlaying(song)
    /** @type {MenuItem[]} */
    const items = [
      {
        label: current ? (player.state.playing ? 'Pausar' : 'Reanudar') : 'Reproducir',
        icon: current && player.state.playing ? 'pause' : 'play',
        action: () => ctx.play(song)
      },
      {
        label: song.favorite ? 'Quitar de favoritos' : 'Marcar como favorito',
        icon: song.favorite ? 'heartFull' : 'heart',
        action: () => toggleFavorite(song)
      },
      // las estrellas de la fila no son parada del tabulador: con el teclado
      // se valora desde aquí (Mayús+F10 sobre la fila)
      {
        label: 'Valorar',
        icon: 'star',
        note: song.stars ? `${song.stars} de 5` : '',
        children: [5, 4, 3, 2, 1, 0].map((n) => ({
          label: n ? `${n} ${n === 1 ? 'estrella' : 'estrellas'}` : 'Sin valorar',
          note: n === (song.stars || 0) ? 'ahora' : '',
          action: () => setStars(song, n)
        }))
      },
      { separator: true },
      { label: 'Añadir a una lista', icon: 'list', children: playlistTargets(song) }
    ]
    if (view.value.kind === 'playlist') {
      items.push({
        label: 'Quitar de esta lista',
        icon: 'close',
        action: () => playlistActions.removeSong(song)
      })
    }
    items.push({ separator: true })
    items.push({ label: 'Buscar letra y portada', icon: 'lyrics', action: () => enrichSong(song) })
    items.push({
      label: song.blur ? 'Ver la portada' : 'Difuminar la portada',
      icon: song.blur ? 'eye' : 'eyeOff',
      action: () => toggleBlur(song)
    })
    items.push({ label: 'Renombrar…', icon: 'pencil', action: () => renameSong(song) })
    items.push(...stemsItems(song))
    items.push({ separator: true })
    // Con el panel lateral a la vista la ficha ya se ve; si no, se ofrece
    if (!ctx.detailsInView()) {
      items.push({ label: 'Ver detalles', icon: 'eye', action: () => ctx.showDetailsOf(song) })
    }
    items.push({ label: 'Abrir la carpeta', icon: 'folderOpen', action: () => revealSong(song) })
    if (shareTargets.value.telegram) {
      items.push({
        label: 'Enviar por Telegram',
        icon: 'send',
        action: () => sendToTelegram([song])
      })
    }
    items.push({
      label: 'Mandar a la papelera…',
      icon: 'trash',
      danger: true,
      action: () => trashSong(song)
    })
    selection.select(song.id)
    openMenu(ev, items, song.title || song.file)
  }

  /**
   * El menú de un repertorio de la barra lateral.
   * @param {MouseEvent|{clientX: number, clientY: number}} ev
   * @param {{id: number, name: string}} pl
   */
  function playlistMenu(ev, pl) {
    openMenu(
      ev,
      [
        {
          label: 'Abrir',
          icon: 'list',
          action: () => {
            view.value = { kind: 'playlist', id: pl.id, name: pl.name }
          }
        },
        { label: 'Renombrar…', icon: 'pencil', action: () => playlistActions.rename(pl) },
        { label: 'Exportar a .m3u', icon: 'download', action: () => playlistActions.exportTo(pl) },
        { label: 'Hoja para el atril…', icon: 'chords', action: () => playlistActions.sheet(pl) },
        ...(shareTargets.value.telegram
          ? [
              {
                label: 'Enviar por Telegram',
                icon: 'send',
                action: () => sendPlaylistToTelegram(pl)
              }
            ]
          : []),
        { separator: true },
        {
          label: 'Borrar la lista…',
          icon: 'trash',
          danger: true,
          action: () => deletePlaylist(pl)
        }
      ],
      pl.name
    )
  }

  return {
    shareTargets,
    loadShareTargets,
    setStars,
    toggleFavorite,
    toggleBlur,
    renameSong,
    enrichSong,
    trashSong,
    trashSongs,
    revealSong,
    revealStems,
    deleteStems,
    sendToTelegram,
    deletePlaylist,
    songMenu,
    groupMenu,
    playlistMenu
  }
}
