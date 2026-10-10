// @ts-check
// Qué canciones están elegidas y la ficha de la principal. Estaba dentro de
// App.vue.
//
// La selección múltiple: Ctrl (o Cmd) añade o quita; Mayús coge el tramo
// desde la última pulsada; Ctrl+Mayús suma el tramo a lo que había. Sobre
// varias, el menú actúa sobre todas (enviar, añadir a una lista, papelera…).
// El tramo va en el orden en que se VE la lista (`shown`): agrupada no es el
// de la lista que llega del núcleo.
//
// El modo «Seleccionar» (el botón con el check de la lista): mientras dura,
// cada fila lleva su casilla y un clic normal pone o quita esa canción, como
// Ctrl+clic, sin soltar las demás. No se guarda: al cambiar de vista se sale.
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
  /** El modo «Seleccionar»: casillas en las filas y el clic que alterna. */
  const selectMode = ref(false)

  /**
   * Elige una canción: sola, o con Ctrl y Mayús como en cualquier explorador.
   *
   * En el modo «Seleccionar» un clic es siempre «poner o quitar esta» (como
   * Ctrl+clic, que ahí no cambia nada) y Mayús suma el tramo a lo que había:
   * con casillas nada se suelta sin quererlo. Sin evento (no es un clic: la
   * ficha de un resultado del buscador, «Ver detalles»…), en el modo se suma
   * a las elegidas en vez de dejarla sola, que tiraría lo marcado a mano.
   * @param {number} id
   * @param {{ ctrlKey?: boolean, metaKey?: boolean, shiftKey?: boolean }|null} [ev]
   */
  async function select(id, ev = null) {
    const mine = ++request
    const order = shown.value.map((s) => s.id)
    const mode = selectMode.value
    const ctrl = !!(ev && (ev.ctrlKey || ev.metaKey)) || (mode && !!ev)
    const shift = !!(ev && ev.shiftKey)
    if (mode && !ev) {
      const keep = new Set(chosenIds())
      keep.add(id)
      selectedIds.value = order.filter((x) => keep.has(x))
    } else if (shift && anchor != null && order.includes(anchor) && order.includes(id)) {
      const [a, b] = [order.indexOf(anchor), order.indexOf(id)].sort((x, y) => x - y)
      const range = order.slice(a, b + 1)
      const keep = ctrl ? new Set([...chosenIds(), ...range]) : new Set(range)
      selectedIds.value = order.filter((x) => keep.has(x))
    } else if (ctrl) {
      const keep = new Set(chosenIds())
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

  /**
   * La que se pone a sonar pasa a ser la principal, sin desordenar lo que se
   * eligió a mano: en el modo «Seleccionar» no se toca nada (poner una a
   * sonar para escucharla no es marcarla), y con varias elegidas solo se
   * mueve la principal si es una de ellas. Si no, queda ella sola: antes la
   * principal cambiaba y la elegida de antes seguía marcada, dos filas
   * pintadas como elegidas sin que nadie las hubiera elegido juntas.
   * @param {number} id
   */
  function follow(id) {
    if (selectMode.value) return
    if (selectedIds.value.length > 1) {
      if (selectedIds.value.includes(id)) selected.value = id
      return
    }
    selected.value = id
    only(id)
  }

  /**
   * Nada elegido en grupo: al cambiar de vista. La principal se queda (es la
   * de la ficha, y el asistente la entiende como «esta» al abrir su página).
   */
  function clear() {
    selectedIds.value = []
    anchor = null
  }

  // ------------------------------------------------- el modo «Seleccionar»
  /** Pone o quita el modo. Quitarlo no suelta nada (eso es `finish`). @param {boolean} v */
  function setMode(v) {
    selectMode.value = !!v
  }

  /** El botón «Seleccionar»: lo pone, o termina como «Listo». */
  function toggleMode() {
    if (selectMode.value) finish()
    else setMode(true)
  }

  /** «Listo» (y Escape): fuera del modo y nada elegido. */
  function finish() {
    setMode(false)
    selectNone()
  }

  /**
   * Todas las que se ven, que son las CARGADAS (la lista llega por páginas: lo
   * que aún no ha llegado no se puede elegir). La principal se queda si está
   * entre ellas; si no, pasa a la primera SIN pedir su ficha: elegir todas no
   * es abrir ninguna.
   */
  function selectAll() {
    const order = shown.value.map((s) => s.id)
    selectedIds.value = order
    if (selected.value == null || !order.includes(selected.value)) {
      selected.value = order[0] ?? null
    }
    if (anchor == null || !order.includes(anchor)) anchor = selected.value
  }

  /**
   * Ninguna: ni el grupo ni la principal, que si no seguiría marcada (la fila
   * pinta la principal también). La ficha abierta no se toca.
   */
  function selectNone() {
    selectedIds.value = []
    selected.value = null
    anchor = null
  }

  /** Esas ya no están (a la papelera, fuera de la lista): se sueltan. @param {number[]} ids */
  function forget(ids) {
    const gone = new Set(ids)
    if (selectedIds.value.some((x) => gone.has(x))) {
      selectedIds.value = selectedIds.value.filter((x) => !gone.has(x))
    }
    if (selected.value != null && gone.has(selected.value)) {
      selected.value = selectedIds.value.at(-1) ?? null
    }
    if (anchor != null && gone.has(anchor)) anchor = null
  }

  /**
   * Las elegidas tal como se pintan: el grupo más la principal (la fila marca
   * las dos; tras poner a sonar una con su botón, la principal es esa y el
   * grupo sigue). Lo que se ve marcado es lo que se toca.
   */
  function chosenIds() {
    const ids = new Set(selectedIds.value)
    if (selected.value != null) ids.add(selected.value)
    return ids
  }

  /** Las canciones de la selección múltiple, en el orden en que se ven. */
  const selectedSongs = computed(() => {
    const keep = new Set(selectedIds.value)
    return keep.size > 1 ? shown.value.filter((s) => keep.has(s.id)) : []
  })

  /**
   * Las elegidas, aunque sea una sola, en el orden en que se ven: sobre las
   * que actúan la barra de la selección y las teclas (Supr). Las que ya no
   * están en la lista no cuentan.
   */
  const chosenSongs = computed(() => {
    const keep = chosenIds()
    return keep.size ? shown.value.filter((s) => keep.has(s.id)) : []
  })
  const count = computed(() => chosenSongs.value.length)
  /** Están todas las que se ven (y hay alguna). */
  const allSelected = computed(() => count.value > 0 && count.value === shown.value.length)
  /** Algunas sí y otras no: la casilla de «todas» a medias. */
  const partial = computed(() => count.value > 0 && !allSelected.value)

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
    follow,
    clear,
    patchDetail,
    refreshDetail,
    selectMode,
    setMode,
    toggleMode,
    finish,
    selectAll,
    selectNone,
    forget,
    chosenSongs,
    count,
    allSelected,
    partial
  }
}
