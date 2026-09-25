// @ts-check
// La lista que se ve y lo que la rodea: sus canciones (y las que faltan por
// llegar), el orden, el estado del núcleo (carpetas, cifras, lo que espera en
// la Entrada) y cargarlo todo. Estaba dentro de App.vue.
import { ref, shallowRef, watch } from 'vue'
import { api, errorMessage } from '../api.js'
import { notify } from './useNotices.js'

/** @typedef {import('../api.js').Song} Song */
/** @typedef {{ kind: string, id?: number, name?: string }} View */

/** Las páginas propias, sin lista de canciones. */
export const PAGES = ['settings', 'inbox', 'chat', 'downloads', 'duplicates']

// La lista entera, sin cortar. Antes se pedían 1000 y ya: «Todas» y
// «Artistas» se quedaban en las mil primeras sin decir nada. Ahora llega una
// primera página enseguida (lo que se ve nada más entrar) y el resto por
// detrás, a tandas, hasta las `count` que dice el núcleo.
const FIRST_PAGE = 400
const PAGE = 5000

/**
 * @param {{ view: import('vue').Ref<View>, params: () => Record<string, any> }} context
 *   view: la vista abierta; params: lo que se busca (el texto y los filtros)
 */
export function useLibrary(context) {
  const { view } = context
  // Superficial: la lista se sustituye entera (otra búsqueda, otra página,
  // una canción que cambia) y nunca se toca por dentro. Con un `ref` normal
  // Vue envolvía en un proxy cada una de las miles de canciones al recorrerlas.
  const songs = shallowRef(/** @type {Song[]} */ ([]))
  /** Cuántas canciones tiene la vista entera (lo que dice el núcleo, aunque aún no hayan llegado todas). */
  const listCount = ref(0)
  /** Están llegando las páginas de detrás. */
  const loadingMore = ref(false)
  const loading = ref(false)
  const stats = ref(/** @type {any} */ (null))
  /** /api/status entero: hace falta el flag de IA. */
  const status = ref(/** @type {any} */ (null))
  const configured = ref(true)
  /** Carpetas gestionadas que ya no están donde estaban (se movieron, un disco sin montar). */
  const missingFolders = ref(/** @type {string[]} */ ([]))
  /** Archivos en la Entrada. */
  const waiting = ref(0)

  const sort = ref('artist')
  // Hacia dónde ordena. Pulsar la misma cabecera la invierte; pulsar otra
  // empieza por lo que tenga sentido en ese campo (A-Z en un texto, lo más
  // largo primero en una duración).
  const sortDesc = ref(false)

  /**
   * @param {string} field
   * @param {boolean} [descByDefault]
   */
  function sortBy(field, descByDefault = false) {
    // En un repertorio el orden lo pone uno arrastrando las canciones; las
    // cabeceras no lo tocan. Antes cambiaban `sort` y no pasaba nada, sin
    // decir por qué.
    if (view.value.kind === 'playlist') {
      return notify('En una lista el orden lo pones tú: arrastra las canciones', 'info')
    }
    if (sort.value === field) sortDesc.value = !sortDesc.value
    else {
      sort.value = field
      sortDesc.value = descByDefault
    }
  }
  /**
   * @param {string} field
   * @param {boolean} desc
   */
  function applySort(field, desc) {
    sort.value = field
    sortDesc.value = desc
  }

  // Las respuestas se numeran: con el retardo del buscador todavía podía
  // llegar la de «bar» después de la de «barak» y pisar la lista con lo que
  // ya no se estaba buscando. (La ficha lleva su propio contador: compartían
  // uno, y pulsar una fila mientras la lista cargaba la dejaba a medias y el
  // indicador de carga girando para siempre.)
  let loadRequest = 0

  /**
   * `quiet`: sin el indicador de carga. Es para los refrescos de fondo, que
   * pasan cada vez que algo cambia en el núcleo; el indicador es para cuando
   * la persona acaba de pedir algo y espera.
   */
  async function load(quiet = false) {
    const mine = ++loadRequest
    if (!quiet) loading.value = true
    try {
      // el estado se relee siempre: si no, `configured` se quedaba congelado
      // en false y toda la vista central seguía mostrando la bienvenida
      const e = await api.status()
      if (mine !== loadRequest) return
      stats.value = e.stats
      configured.value = e.configured
      missingFolders.value = e.missing_folders || []

      const v = view.value
      if (v.kind === 'playlist') {
        const r = await api.playlistSongs(/** @type {number} */ (v.id))
        if (mine === loadRequest) setSongs(r.songs)
      } else if (v.kind === 'player') {
        // La lista del reproductor no se busca en el índice: es lo que has ido
        // abriendo desde fuera, en el orden en que lo abriste.
        const r = await api.externalList()
        if (mine === loadRequest) setSongs(r.songs)
      } else if (!PAGES.includes(v.kind)) {
        /** @type {Record<string, any>} */
        const p = { ...context.params(), sort: sort.value, desc: sortDesc.value }
        if (v.kind === 'favorites') p.only_favorites = true
        await loadSearch(p, mine)
      }
    } catch (e) {
      if (mine === loadRequest) notify('No se pudo cargar: ' + errorMessage(e))
    } finally {
      if (mine === loadRequest) loading.value = false
    }
  }

  /**
   * Una lista que llega entera de una vez.
   * @param {Song[]} [list]
   */
  function setSongs(list) {
    songs.value = list || []
    listCount.value = songs.value.length
    loadingMore.value = false
  }

  /**
   * @param {Record<string, any>} p  la consulta, sin límite ni desplazamiento
   * @param {number} mine            el número de esta carga: si llega otra, se deja
   */
  async function loadSearch(p, mine) {
    const first = await api.search({ ...p, limit: FIRST_PAGE, from_key: 0 })
    if (mine !== loadRequest) return
    const known = Number.isFinite(first.count)
    songs.value = first.songs
    listCount.value = known ? first.count : first.songs.length
    const more = known ? first.songs.length < first.count : first.songs.length === FIRST_PAGE
    loadingMore.value = more
    if (more) loadRest(p, mine, known ? first.count : Infinity)
  }

  /**
   * Las páginas de detrás, hasta completar la lista (o hasta que llegue otra carga).
   * @param {Record<string, any>} p
   * @param {number} mine
   * @param {number} count
   */
  async function loadRest(p, mine, count) {
    let all = songs.value
    const seen = new Set(all.map((s) => s.id))
    try {
      while (all.length < count) {
        const r = await api.search({ ...p, limit: PAGE, from_key: all.length })
        if (mine !== loadRequest) return
        // Un núcleo que no respetara el desplazamiento devolvería otra vez lo
        // mismo: se para ahí en vez de repetir canciones.
        const fresh = r.songs.filter((s) => !seen.has(s.id))
        for (const s of fresh) seen.add(s.id)
        all = all.concat(fresh)
        songs.value = all
        if (!fresh.length || r.songs.length < PAGE) break
      }
      if (count !== Infinity && all.length < count) {
        console.warn(`[danplay] la lista se quedó en ${all.length} de ${count}`)
      }
      listCount.value = count === Infinity ? all.length : Math.max(all.length, listCount.value)
    } catch (e) {
      if (mine === loadRequest) notify('No se pudo cargar el resto de la lista: ' + errorMessage(e))
    } finally {
      if (mine === loadRequest) loadingMore.value = false
    }
  }

  /** El estado del núcleo: carpetas, cifras y lo que espera en la Entrada. */
  async function loadStatus() {
    const e = await api.status()
    status.value = e
    stats.value = e.stats
    configured.value = e.configured
    missingFolders.value = e.missing_folders || []
    waiting.value = (await api.inbox()).total
  }

  /**
   * Una canción cambió: se cambia en la lista (sin tocar las demás).
   * @param {Song} song
   */
  function patchSong(song) {
    const i = songs.value.findIndex((x) => x.id === song.id)
    if (i < 0) return
    const next = songs.value.slice()
    next[i] = { ...next[i], ...song }
    songs.value = next
  }

  /**
   * Mueve una canción dentro del repertorio abierto: la deja justo antes de
   * la que tiene `targetId`, o justo después si se soltó en su mitad de
   * abajo. La lista se recoloca al momento y el núcleo confirma el orden; si
   * no puede, vuelve como estaba.
   * @param {Song} song
   * @param {number} targetId
   * @param {boolean} after
   */
  async function moveInPlaylist(song, targetId, after) {
    if (view.value.kind !== 'playlist' || song.id === targetId) return
    const before = songs.value
    const ids = before.map((s) => s.id)
    if (!ids.includes(song.id) || !ids.includes(targetId)) return
    const order = ids.filter((id) => id !== song.id)
    order.splice(order.indexOf(targetId) + (after ? 1 : 0), 0, song.id)
    if (order.every((id, i) => id === ids[i])) return // ya estaba ahí
    const by = new Map(before.map((s) => [s.id, s]))
    songs.value = order.map((id) => /** @type {Song} */ (by.get(id)))
    const listId = /** @type {number} */ (view.value.id)
    const stillHere = () => view.value.kind === 'playlist' && view.value.id === listId
    try {
      const r = await api.reorderPlaylist(listId, order)
      // si mientras tanto se cambió de vista, lo que llega ya no es esta lista
      if (stillHere()) songs.value = r.songs
    } catch (e) {
      if (stillHere()) songs.value = before
      notify('No se pudo cambiar el orden: ' + errorMessage(e))
    }
  }

  watch([sort, sortDesc, view], () => load(), { deep: true })

  return {
    songs,
    listCount,
    loadingMore,
    loading,
    stats,
    status,
    configured,
    missingFolders,
    waiting,
    sort,
    sortDesc,
    sortBy,
    applySort,
    load,
    setSongs,
    loadStatus,
    patchSong,
    moveInPlaylist
  }
}
