// @ts-check
// Qué canciones están elegidas y la ficha de la principal. Estaba dentro de
// App.vue.
//
// La selección múltiple: Ctrl (o Cmd) añade o quita; Mayús coge el tramo
// desde la última pulsada; Ctrl+Mayús suma el tramo a lo que había. Sobre
// varias, el menú actúa sobre todas (enviar, añadir a una lista, papelera…).
// El tramo va en el orden en que se VE la lista (`shown`): agrupada no es el
// de la lista que llega del núcleo.
import { ref, computed } from 'vue'
import { api } from '../api.js'

/** @typedef {import('../api.js').Song} Song */

/**
 * @param {{ shown: import('vue').Ref<Song[]> }} context
 *   shown: la lista en el orden en que se ve
 */
export function useSelection(context) {
  const { shown } = context
  /** La principal: la de la ficha. */
  const selected = ref(/** @type {number|null} */ (null))
  const selectedIds = ref(/** @type {number[]} */ ([]))
  /** La ficha entera de la principal. */
  const detail = ref(/** @type {any} */ (null))
  /** @type {number|null} de dónde sale el tramo de Mayús */
  let anchor = null
  // La ficha que llega tarde no pisa la de una canción elegida después: cada
  // petición lleva su número y solo vale la última.
  let request = 0

  /**
   * Elige una canción: sola, o con Ctrl y Mayús como en cualquier explorador.
   * @param {number} id
   * @param {{ ctrlKey?: boolean, metaKey?: boolean, shiftKey?: boolean }|null} [ev]
   */
  async function select(id, ev = null) {
    const mine = ++request
    const order = shown.value.map((s) => s.id)
    const ctrl = !!(ev && (ev.ctrlKey || ev.metaKey))
    const shift = !!(ev && ev.shiftKey)
    if (shift && anchor != null && order.includes(anchor) && order.includes(id)) {
      const [a, b] = [order.indexOf(anchor), order.indexOf(id)].sort((x, y) => x - y)
      const range = order.slice(a, b + 1)
      const keep = ctrl ? new Set([...selectedIds.value, ...range]) : new Set(range)
      selectedIds.value = order.filter((x) => keep.has(x))
    } else if (ctrl) {
      const keep = new Set(
        selectedIds.value.length
          ? selectedIds.value
          : selected.value != null
            ? [selected.value]
            : []
      )
      if (keep.has(id)) keep.delete(id)
      else keep.add(id)
      selectedIds.value = order.filter((x) => keep.has(x))
      anchor = id
      if (!keep.has(id)) {
        // se ha quitado: la ficha pasa a la última que quede seleccionada
        selected.value = selectedIds.value.at(-1) ?? null
        if (selected.value == null) detail.value = null
        else await showDetail(selected.value, mine)
        return
      }
    } else {
      selectedIds.value = [id]
      anchor = id
    }
    selected.value = id
    await showDetail(id, mine)
  }

  /**
   * La ficha de esa canción, si para cuando llega sigue siendo la última que
   * se pidió: dos clics seguidos no pueden acabar enseñando la del primero.
   * @param {number} id
   * @param {number} [mine]  el número de la petición (sin él, una nueva)
   */
  async function showDetail(id, mine = ++request) {
    try {
      const song = await api.song(id)
      if (mine === request) detail.value = song
    } catch {
      /* ya no está: la lista recargada lo dirá */
    }
  }

  /** Solo esta (la del menú contextual, o la que se pone a sonar). @param {number} id */
  function only(id) {
    selectedIds.value = [id]
    anchor = id
  }

  /** Nada elegido en grupo: al cambiar de vista. */
  function clear() {
    selectedIds.value = []
    anchor = null
  }

  /** Las canciones de la selección múltiple, en el orden en que se ven. */
  const selectedSongs = computed(() => {
    const keep = new Set(selectedIds.value)
    return keep.size > 1 ? shown.value.filter((s) => keep.has(s.id)) : []
  })

  /**
   * Una canción cambió: si es la de la ficha, la ficha también. Solo la suya:
   * las respuestas llegan cuando llegan, y «Buscar letra» de A con B ya
   * elegida devolvía la ficha a A.
   * @param {Song} song
   */
  function patchDetail(song) {
    if (detail.value?.id === song.id) detail.value = song
  }

  /** Vuelve a pedir la ficha abierta; devuelve la canción, o null. */
  async function refreshDetail() {
    const id = detail.value?.id
    if (id == null || id < 0) return null
    try {
      const song = await api.song(id)
      if (detail.value?.id === id) detail.value = song
      return song
    } catch {
      return null // la canción ya no está: la lista recargada lo dirá
    }
  }

  return {
    selected,
    selectedIds,
    selectedSongs,
    detail,
    select,
    showDetail,
    only,
    clear,
    patchDetail,
    refreshDetail
  }
}
