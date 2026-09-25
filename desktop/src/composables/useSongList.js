// @ts-check
// Lo que comparten las cuatro vistas de una lista de canciones (tabla, lista
// fina, fichas y cuadrícula): qué fila está elegida o suena, el botón de
// reproducir o pausar, el arrastre para ordenar, pintar solo lo que se ve,
// «ir a lo que suena», el teclado y los grupos (la lista agrupada es una sola
// lista, con la cabecera de cada grupo entre sus filas).
//
// Estaba copiado en los cuatro componentes, unas cuarenta líneas cada uno, y
// las copias ya habían empezado a separarse.
import { ref, computed, watch, nextTick, useId } from 'vue'
import { useDragSong } from './useDragSong.js'
import { useVirtualRows } from './useVirtualRows.js'
import { usePlayback } from './usePlayback.js'
import { songKey, targetIndex } from '../utils/keys.js'

/** @typedef {import('../api.js').Song} Song */

/**
 * Un grupo de la lista agrupada (lo arma GroupedSongs).
 * @typedef {Object} SongGroup
 * @property {string} key      el nombre del grupo (artista, álbum...)
 * @property {number} start    dónde empiezan sus canciones en `songs`
 * @property {number} size     cuántas se pintan (0 si está plegado)
 * @property {number} count    cuántas tiene
 * @property {number} seconds  cuánto duran
 * @property {boolean} open
 */

/**
 * Un trozo de lo que se pinta: las canciones de un grupo que caen en la
 * ventana, con su cabecera. Sin agrupar hay uno solo, sin cabecera.
 * @typedef {Object} SongSection
 * @property {string} key
 * @property {SongGroup|null} group
 * @property {number} index    su posición entre los grupos
 * @property {number} from     la posición en `songs` de su primera canción pintada
 * @property {Song[]} songs
 */

/**
 * @typedef {Object} SongListProps
 * @property {Song[]} songs
 * @property {number|null} [selected]     la de la ficha
 * @property {number[]} [selectedIds]     la selección múltiple
 * @property {number|null} [playing]      la que suena (o la cargada)
 * @property {number|null} [jumpTo]       a la que hay que ir
 * @property {boolean} [sortable]         se ordena arrastrando (un repertorio)
 * @property {SongGroup[]|null} [groups]  los grupos, si la lista va agrupada
 */

/**
 * @param {SongListProps} props
 * @param {(event: any, ...args: any[]) => void} emit
 * @param {{ anchor: () => (HTMLElement|null|undefined), grid?: boolean }} options
 *   anchor: el elemento que contiene las filas (lo que mide el virtualizado);
 *   grid: van en cuadrícula, varias por línea
 */
export function useSongList(props, emit, options) {
  const { startDrag, isDragged, isBefore, isAfter } = useDragSong()
  // Sobre la que está puesta, el botón de la fila es pausa (o reanudar si
  // está en pausa); en las demás, reproducir. Lo de «sonando» lo sabe el
  // reproductor.
  const player = usePlayback()
  const sounding = player.playing

  // Elegida: la principal (la ficha) o cualquiera de la selección múltiple
  // (Ctrl y Mayús al pulsar, decididas por la app).
  const chosen = computed(() => new Set(props.selectedIds || []))
  /** @param {number} id */
  const picked = (id) => props.selected === id || chosen.value.has(id)

  // En un repertorio cada fila es destino de arrastre (`sort:id`), para poder
  // cambiar el orden. La raya se pinta en la mitad por la que va el puntero,
  // menos sobre la propia fila que se lleva: ahí no hay nada que marcar.
  /** @param {Song} c */
  const dropKey = (c) => 'sort:' + c.id
  /** @param {Song} c */
  const before = (c) => !!props.sortable && !isDragged(c.id) && isBefore(dropKey(c))
  /** @param {Song} c */
  const after = (c) => !!props.sortable && !isDragged(c.id) && isAfter(dropKey(c))

  /** @param {Song} c */
  const rowIcon = (c) => (props.playing === c.id && sounding.value ? 'pause' : 'play')
  /** @param {Song} c */
  const rowTitle = (c) =>
    props.playing !== c.id ? 'Reproducir' : sounding.value ? 'Pausar' : 'Reanudar'

  // Los elementos de cada fila, para desplazarse hasta ellos y darles el
  // foco. Un Map normal, fuera de la reactividad: nadie los pinta. Cada fila
  // se quita al desmontarse; antes se quedaban los nodos de todo lo que se
  // había ido pintando hasta que cambiaba la lista.
  /** @type {Map<number, HTMLElement>} */
  const elements = new Map()
  /** @param {number} id */
  const refFor = (id) => (/** @type {any} */ el) => {
    if (el) elements.set(id, el)
    else elements.delete(id)
  }

  // ------------------------------------------------------------ virtualizado
  const virtual = useVirtualRows(options.anchor, () => props.songs.length, {
    groups: () => props.groups
  })

  /** Lo que se pinta, grupo a grupo. @type {import('vue').ComputedRef<SongSection[]>} */
  const sections = computed(() => {
    const w = virtual.span.value
    const count = props.songs.length
    const out = []
    for (let g = w.gFirst; g <= w.gLast; g++) {
      const item = w.list[g]
      const end = item.start + item.size
      const a = Math.min(end, item.start + (g === w.gFirst ? w.lineFirst * w.cols : 0))
      const b = Math.min(end, count, g === w.gLast ? item.start + w.lineLast * w.cols : end)
      const group = w.heads ? /** @type {SongGroup} */ (item) : null
      out.push({
        key: group ? 'g:' + group.key : '',
        group,
        index: g,
        from: a,
        songs: props.songs.slice(a, Math.max(a, b))
      })
    }
    return out
  })

  /** El número que se enseña en la fila: dentro de su grupo, si va agrupada. */
  const numberOf = (/** @type {SongSection} */ sec, /** @type {number} */ i) =>
    sec.from + i + 1 - (sec.group ? sec.group.start : 0)

  // agrupada y con todo plegado no hay canciones, pero sí las cabeceras
  const hasRows = computed(() => props.songs.length > 0 || !!props.groups?.length)

  // La cabecera de cada grupo nombra su trozo de lista (`aria-labelledby`).
  const uid = useId()
  const sectionId = (/** @type {SongSection} */ sec) => `${uid}-g${sec.index}`

  /**
   * Trae a la ventana la fila de esa posición (pintándola si no lo estaba)
   * y devuelve su elemento.
   * @param {number} index
   * @param {ScrollLogicalPosition} [block]
   */
  async function revealRow(index, block = 'nearest') {
    const song = props.songs[index]
    if (!song) return null
    const { viewport, rowHeight } = (await virtual.reveal(index)) || {}
    await nextTick()
    const el = elements.get(song.id)
    if (el && typeof el.scrollIntoView === 'function') {
      el.scrollIntoView({ block, behavior: block === 'center' ? 'smooth' : 'auto' })
    } else if (viewport && rowHeight > 0) {
      viewport.scrollTop = Math.max(0, index * rowHeight - viewport.clientHeight / 2)
    }
    return el || null
  }

  // «ir a lo que suena»
  watch(
    () => props.jumpTo,
    (id) => {
      if (!id) return
      const i = props.songs.findIndex((c) => c.id === id)
      if (i >= 0) revealRow(i, 'center')
    }
  )

  // -------------------------------------------------------------- teclado
  // La lista es una sola parada del tabulador: la fila con el foco (o, si
  // ninguna lo ha tenido, la elegida, o la primera) lleva tabindex 0 y las
  // demás -1. Las flechas pasan a la vecina y la eligen.
  const focusId = ref(/** @type {number|null} */ (null))
  const tabStop = computed(() => {
    const has = (/** @type {any} */ id) => id != null && props.songs.some((c) => c.id === id)
    if (has(focusId.value)) return focusId.value
    if (has(props.selected)) return props.selected
    return props.songs[0]?.id ?? null
  })
  /** @param {Song} c */
  const tabindex = (c) => (c.id === tabStop.value ? 0 : -1)

  /**
   * @param {KeyboardEvent} e
   * @param {Song} c
   * @param {number} index  su posición en la lista entera
   */
  async function onKey(e, c, index) {
    // la tecla era de un botón de dentro de la fila (el corazón, reproducir)
    if (e.target !== e.currentTarget) return
    const key = songKey(e, { grid: !!options.grid, columns: virtual.columns.value })
    if (!key) return
    e.preventDefault()
    e.stopPropagation()
    if (key.kind === 'play') return emit('play', c)
    if (key.kind === 'toggle') {
      // espacio pausa o sigue, como en el resto de la app; sin nada cargado,
      // pone esta
      if (player.track.value) return player.toggle()
      return emit('play', c)
    }
    if (key.kind === 'select') return emit('select', c.id, { ctrlKey: true })
    if (key.kind === 'menu') {
      const r = /** @type {HTMLElement} */ (e.currentTarget).getBoundingClientRect()
      return emit('context', { clientX: r.left + 24, clientY: r.top + r.height / 2 }, c)
    }
    const to = targetIndex(key, index, props.songs.length)
    if (to < 0 || to === index) return
    const next = props.songs[to]
    focusId.value = next.id
    const el = await revealRow(to)
    el?.focus()
    // con Ctrl solo se mueve el foco; con Mayús se amplía la selección
    if (!(e.ctrlKey || e.metaKey)) emit('select', next.id, { shiftKey: e.shiftKey })
  }

  /** @param {Song} c */
  const onFocus = (c) => (focusId.value = c.id)

  return {
    picked,
    before,
    after,
    dropKey,
    rowIcon,
    rowTitle,
    sounding,
    isDragged,
    startDrag,
    refFor,
    sections,
    numberOf,
    hasRows,
    sectionId,
    padTop: virtual.padTop,
    padBottom: virtual.padBottom,
    tabindex,
    onKey,
    onFocus
  }
}
