// @ts-check
// Los menús de clic derecho (el de una canción, el de varias elegidas y el de
// un repertorio) y lo que se hace desde ellos con las canciones: valorar,
// favoritas, difuminar la portada, renombrar, buscar letra, separarlas en
// pistas, la papelera, abrir su carpeta o mandarlas por Telegram. Estaba
// dentro de App.vue. Las acciones sobre varias (`groupItems`) son también las
// de la barra de la selección (SelectionBar).
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
 * @property {(song: Song) => any} [previewSong]  elige la canción para que su ficha se vea en el panel de al lado (sin ventana)
 * @property {(ids: number[]) => void} [onRemoved]  esas canciones ya no están (a la papelera)
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
      ctx.onRemoved?.([song.id])
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
    ctx.onRemoved?.(list.map((s) => s.id))
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

  // --------------------------------------------- sobre varias (o una) a la vez
  // Lo que hacen el menú de grupo y la barra de la selección: las dos cosas
  // salen de aquí para no repetir la lógica (ni los avisos).

  /** El nombre con el que se nombra una canción en un aviso. @param {Song} s */
  const nameOf = (s) => s.title || s.file || 'la canción'

  /**
   * Las que no lo eran, a favoritas. Un solo aviso para todas.
   * @param {Song[]} list
   */
  async function favorites(list) {
    const todo = list.filter((s) => !s.favorite)
    if (!todo.length) {
      return notify(list.length === 1 ? 'Ya era favorita' : 'Ya eran todas favoritas', 'info')
    }
    let done = 0
    for (const s of todo) {
      try {
        ctx.onUpdated(await api.toggleFavorite(s.id, true))
        done++
      } catch (e) {
        notify(`No se pudo marcar «${nameOf(s)}»: ${errorMessage(e)}`)
      }
    }
    if (done) {
      notify(
        done === 1 ? `«${nameOf(todo[0])}» a favoritos` : `${done} marcadas como favoritas`,
        'ok'
      )
    }
  }

  /** Las que eran favoritas dejan de serlo. @param {Song[]} list */
  async function unfavorite(list) {
    const todo = list.filter((s) => s.favorite)
    let done = 0
    for (const s of todo) {
      try {
        ctx.onUpdated(await api.toggleFavorite(s.id, false))
        done++
      } catch (e) {
        notify(`No se pudo quitar «${nameOf(s)}»: ${errorMessage(e)}`)
      }
    }
    if (done) {
      notify(
        done === 1 ? `«${nameOf(todo[0])}» ya no es favorita` : `${done} quitadas de favoritas`,
        'ok'
      )
    }
  }

  /**
   * A la papelera con la confirmación de siempre: la de una (que enseña la
   * ruta del archivo) si es una, la de varias (que dice cuántas) si son más.
   * @param {Song[]} list
   */
  function trash(list) {
    if (!list.length) return
    return list.length === 1 ? trashSong(list[0]) : trashSongs(list)
  }

  /**
   * Fuera del repertorio que se está viendo. Los archivos no se tocan, así
   * que no se pregunta: se avisa de cuántas y de qué lista.
   * @param {Song[]} list
   */
  async function removeFromView(list) {
    const v = view.value
    if (v.kind !== 'playlist' || !list.length) return
    const listId = /** @type {number} */ (v.id)
    const listName = v.name || 'la lista'
    const gone = []
    for (const s of list) {
      try {
        await api.removeFromPlaylist(listId, s.id)
        gone.push(s.id)
      } catch (e) {
        notify(`No se pudo quitar «${nameOf(s)}»: ${errorMessage(e)}`)
      }
    }
    if (gone.length) {
      selection.forget(gone)
      notify(
        gone.length === 1
          ? `«${nameOf(list.find((s) => s.id === gone[0]) || list[0])}» quitada de «${listName}»`
          : `${gone.length} quitadas de «${listName}»`,
        'ok'
      )
    }
    await Promise.all([ctx.reload(true), playlistActions.load()])
  }

  /** Por Telegram (si está instalado: `shareTargets.telegram`). @param {Song[]} list */
  const sendTelegram = (list) => sendToTelegram(list)

  /**
   * Dónde abrir un menú que sale de un botón: debajo de él, alineado a su
   * izquierda, y no donde cayó el puntero. Solo para un clic (con el ratón o
   * con Enter); un clic derecho o un sitio que ya viene dado se quedan como
   * están. Sigue siendo un MouseEvent con su `detail`: así el menú sabe si
   * se abrió con el teclado (y entonces le da el foco y lo devuelve).
   * @param {MouseEvent|{clientX: number, clientY: number}} ev
   */
  function below(ev) {
    if (!(ev instanceof MouseEvent) || ev.type !== 'click') return ev
    const target = /** @type {any} */ (ev.currentTarget || ev.target)
    const button = target?.closest?.('button')
    if (!button) return ev
    const r = button.getBoundingClientRect()
    return new MouseEvent('click', { clientX: r.left, clientY: r.bottom + 4, detail: ev.detail })
  }

  /**
   * El menú de las listas para esas canciones, debajo del botón que lo abre
   * («Añadir a una lista ▾» de la barra de la selección).
   * @param {MouseEvent|{clientX: number, clientY: number}} ev
   * @param {Song[]} list
   */
  function addToList(ev, list) {
    if (!list.length) return
    const n = list.length
    openMenu(
      below(ev),
      playlistTargets(list),
      n === 1 ? 'Añadir a una lista' : `Añadir ${n} a una lista`
    )
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
   * Separar en pistas, con la mejor calidad. Nada si en este equipo no se
   * puede.
   * @param {Song[]} list
   * @param {string} label
   * @returns {MenuItem[]}
   */
  function separateItems(list, label) {
    if (!separation.state.ok) return []
    const note = separation.state.pending
      ? `bajar ${separation.megas(separation.state.pending)}`
      : ''
    return [{ label, icon: 'mixer', note, action: () => separation.request(list) }]
  }

  /** Lo de las pistas de una canción: separarla, o lo que se hace con las suyas. @param {Song} song */
  function stemsItems(song) {
    const queued = separation.progressOf(song.id)
    if (queued) {
      const what = queued.stage === 'refine' ? 'mejorar' : 'separar'
      return [
        {
          label: queued.waiting ? `Quitar de la cola de ${what}` : 'Parar la separación',
          icon: 'mixer',
          action: () => (queued.waiting ? separation.unqueue(song.id) : separation.cancel())
        }
      ]
    }
    if (!song.has_stems) return separateItems([song], 'Separar pistas')
    return [
      { label: 'Abrir la carpeta de sus pistas', icon: 'mixer', action: () => revealStems(song) },
      ...separateItems(
        [song],
        song.stems_best ? 'Separar otra vez' : 'Separar otra vez con la mejor calidad'
      ),
      { label: 'Borrar sus pistas…', icon: 'trash', action: () => deleteStems(song) }
    ]
  }

  /**
   * Todas las canciones de un repertorio a la cola de separar: las que ya
   * tienen las mejores pistas no se repiten. @param {{id: number}} pl
   */
  async function separatePlaylist(pl) {
    let list
    try {
      list = (await api.playlistSongs(pl.id)).songs || []
    } catch (e) {
      return notify(errorMessage(e))
    }
    const todo = list.filter((s) => !s.has_stems || !s.stems_best)
    if (!list.length) return notify('Esa lista está vacía')
    if (!todo.length) return notify('Todas las canciones de esa lista ya tienen sus pistas', 'ok')
    await separation.request(todo)
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
    // una lista de una sola es esa canción: su aviso habla de ella, no de «1 añadidas»
    const many = Array.isArray(songOrList) && songOrList.length > 1 ? songOrList : null
    const song = Array.isArray(songOrList) ? songOrList[0] : songOrList
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
   * Las opciones sobre varias canciones (o una, la elegida en el modo
   * «Seleccionar»): actúan sobre todas. Son las del menú de grupo y las del
   * «⋯» de la barra de la selección.
   *
   * Con `fromSearch` son las de lo elegido en el desplegable del buscador:
   * no se ofrece «quitar de esta lista» (habla de otra lista) y «Reproducir»
   * lo decide quien llama (`play`), que sabe qué cola le toca.
   * @param {Song[]} list
   * @param {{fromSearch?: boolean, play?: (list: Song[]) => any}} [opts]
   * @returns {MenuItem[]}
   */
  function groupItems(list, opts = {}) {
    const n = list.length
    const one = n === 1
    /** @param {string} single @param {string} several */
    const say = (single, several) => (one ? single : several)
    const allFavorite = list.every((s) => s.favorite)
    /** @type {MenuItem[]} */
    const items = [
      {
        label: say('Reproducir', `Reproducir estas ${n}`),
        icon: 'play',
        action: () =>
          opts.play
            ? opts.play(list)
            : ctx.play(list[0], list, { ...view.value, label: 'la selección' })
      },
      { separator: true },
      {
        label: say('Añadir a una lista', `Añadir ${n} a una lista`),
        icon: 'list',
        children: playlistTargets(list)
      },
      allFavorite
        ? {
            label: say('Quitar de favoritas', `Quitar ${n} de favoritas`),
            icon: 'heartFull',
            action: () => unfavorite(list)
          }
        : {
            label: say('Marcar como favorita', `Marcar ${n} como favoritas`),
            icon: 'heart',
            action: () => favorites(list)
          }
    ]
    if (view.value.kind === 'playlist' && !opts.fromSearch) {
      items.push({
        label: say('Quitar de esta lista', `Quitar ${n} de esta lista`),
        icon: 'close',
        action: () => removeFromView(list)
      })
    }
    items.push(...separateItems(list, say('Separar pistas', `Separar las pistas de ${n}`)))
    items.push({ separator: true })
    if (shareTargets.value.telegram) {
      items.push({
        label: say('Enviar por Telegram', `Enviar ${n} por Telegram`),
        icon: 'send',
        action: () => sendTelegram(list)
      })
    }
    items.push({
      label: say('Mandar a la papelera…', `Mandar ${n} a la papelera…`),
      icon: 'trash',
      danger: true,
      action: () => trash(list)
    })
    return items
  }

  /**
   * El menú sobre las canciones elegidas: actúa sobre todas. Desde un botón
   * (el «⋯» de la barra) se abre debajo de él.
   * @param {MouseEvent|{clientX: number, clientY: number}} ev
   * @param {Song[]} list
   * @param {{fromSearch?: boolean, play?: (list: Song[]) => any}} [opts]
   */
  function groupMenu(ev, list, opts = {}) {
    if (!list.length) return
    const n = list.length
    openMenu(below(ev), groupItems(list, opts), n === 1 ? nameOf(list[0]) : `${n} canciones`)
  }

  /**
   * El menú de una canción (o el de todas, si es una de las elegidas).
   *
   * Con `fromSearch` es el de un resultado del desplegable del buscador: ahí
   * la canción no es una fila de la lista que se ve, así que no toca la
   * selección ni la ficha de al lado (ni se ofrece «quitar de esta lista», que
   * habla de otra cosa), y «Reproducir» lo decide quien llama (`play`), que
   * es quien sabe qué cola le toca a lo encontrado.
   * @param {MouseEvent|{clientX: number, clientY: number}} ev
   * @param {Song} song
   * @param {{fromSearch?: boolean, play?: () => any}} [opts]
   */
  function songMenu(ev, song, opts = {}) {
    const fromSearch = !!opts.fromSearch
    // En el modo «Seleccionar» el menú no toca la selección (la que se marcó
    // a mano no se suelta por abrir un menú): sobre una elegida es el de las
    // elegidas, aunque sea una sola; sobre otra, el de esa sin elegirla.
    const mode = !fromSearch && selection.selectMode.value
    if (mode) {
      const chosen = selection.chosenSongs.value
      if (chosen.some((s) => s.id === song.id)) return groupMenu(ev, chosen)
    } else if (!fromSearch) {
      const chosen = selection.selectedSongs.value
      if (chosen.length > 1 && selection.selectedIds.value.includes(song.id)) {
        return groupMenu(ev, chosen)
      }
      selection.only(song.id)
    }
    const current = isPlaying(song)
    /** @type {MenuItem[]} */
    const items = [
      {
        label: current ? (player.state.playing ? 'Pausar' : 'Reanudar') : 'Reproducir',
        icon: current && player.state.playing ? 'pause' : 'play',
        action: () => (opts.play ? opts.play() : ctx.play(song))
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
    if (view.value.kind === 'playlist' && !fromSearch) {
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
    // Con el panel lateral a la vista la ficha ya se ve; si no, se ofrece. Desde
    // un resultado del desplegable el panel enseña OTRA canción (no se elige la
    // fila): ahí «Ver detalles» solo la elige, sin abrir una ventana encima.
    if (!ctx.detailsInView()) {
      items.push({ label: 'Ver detalles', icon: 'eye', action: () => ctx.showDetailsOf(song) })
    } else if (fromSearch && ctx.previewSong) {
      items.push({ label: 'Ver detalles', icon: 'eye', action: () => ctx.previewSong?.(song) })
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
    if (!fromSearch && !mode) selection.select(song.id)
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
        ...(separation.state.ok
          ? [
              {
                label: 'Separar las pistas del repertorio',
                icon: 'mixer',
                action: () => separatePlaylist(pl)
              }
            ]
          : []),
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
    groupItems,
    playlistMenu,
    playlistTargets,
    // lo que hace la barra de la selección (y el menú de grupo)
    addToList,
    favorites,
    unfavorite,
    trash,
    sendTelegram,
    removeFromView
  }
}
