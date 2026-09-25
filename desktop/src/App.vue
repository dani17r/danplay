<script setup>
/**
 * La ventana principal: orquesta y poco más.
 *
 * Lo que antes vivía aquí y ahora vive fuera:
 *   la cola y lo que suena   → composables/usePlayback.js (el estado está en Rust)
 *   la lista, su orden y el estado del núcleo → composables/useLibrary.js
 *   la selección y la ficha  → composables/useSelection.js
 *   los menús de clic derecho y lo que hacen → composables/useSongMenus.js
 *   los avisos               → composables/useNotices.js
 *   los diálogos             → composables/useDialog.js
 *   el menú contextual       → composables/useContextMenu.js
 *   tema, densidad y vistas  → composables/usePreferences.js
 *   bienvenida, entrada y duplicados → sus propios componentes
 */
import { ref, onMounted, onUnmounted, watch, computed, nextTick, useTemplateRef } from 'vue'
import { api, app as tauriApp, core, errorMessage } from './api.js'
import { useLibrary, PAGES } from './composables/useLibrary.js'
import { useSelection } from './composables/useSelection.js'
import { useSongMenus } from './composables/useSongMenus.js'
import { onClickOutside } from './composables/useClickOutside.js'
import { useHasScroll, useIsOffscreen } from './composables/useHasScroll.js'
import { useViewport } from './composables/useViewport.js'
import { useDragSong, onDrop } from './composables/useDragSong.js'
import { usePlayback } from './composables/usePlayback.js'
import { useNotices, notify } from './composables/useNotices.js'
import { useDialog, ask } from './composables/useDialog.js'
import { useContextMenu, closeMenu } from './composables/useContextMenu.js'
import {
  usePreferences,
  VIEWS,
  VIEW_NAMES,
  VIEW_ICONS,
  GROUPINGS
} from './composables/usePreferences.js'
import { useHotkeys, releaseFocusAfterPointer } from './composables/useHotkeys.js'
import { useFocusTrap } from './composables/useFocusTrap.js'
import { useSearch } from './composables/useSearch.js'
import { usePlaylistActions } from './composables/usePlaylistActions.js'
import { useDownloads } from './composables/useDownloads.js'
import { connectChat } from './composables/useChat.js'
import { groupedOrder } from './utils/groups.js'
import { useRouter } from 'vue-router'
import { routeFor } from './router.js'
import { DENSITIES, SIZES, KIND_LABEL, allThemes } from './themes.js'
import Drawer from './components/ui/Drawer.vue'
import CoverArt from './components/ui/CoverArt.vue'
import Sidebar from './components/Sidebar.vue'
import SongTable from './components/SongTable.vue'
import SongGrid from './components/SongGrid.vue'
import SongRows from './components/SongRows.vue'
import SongCards from './components/SongCards.vue'
import SearchPanel from './components/SearchPanel.vue'
import SearchResults from './components/SearchResults.vue'
import GroupedSongs from './components/GroupedSongs.vue'
import DetailsPanel from './components/DetailsPanel.vue'
import Player from './components/Player.vue'
import WelcomePage from './components/WelcomePage.vue'
import Icon from './components/Icon.vue'
import StudyBar from './components/StudyBar.vue'
import TextField from './components/ui/TextField.vue'
import SelectField from './components/ui/SelectField.vue'
import ToggleField from './components/ui/ToggleField.vue'
import SliderField from './components/ui/SliderField.vue'
import ContextMenu from './components/ui/ContextMenu.vue'
import Loading from './components/ui/Loading.vue'
import ModalDialog from './components/ui/ModalDialog.vue'

const player = usePlayback()
// para la plantilla: dentro de un objeto los refs no se desenvuelven solos
const { shuffle, repeat } = player
const { notices, dismiss } = useNotices()
const { dialog, dialogOk, dialogCancel } = useDialog()
const { menu } = useContextMenu()
const prefs = usePreferences()
const { theme, density, appSize, layout, groupBy, cardSize, showDetails } = prefs

const view = ref({ kind: 'all' })
// El modo estudio (bucle A-B, velocidad sin cambiar el tono, marcadores,
// notas): una barra encima del reproductor. Al cerrarla, todo vuelve a lo
// normal.
const studyOpen = ref(false)
// La lista de la vista, su orden y el estado del núcleo. Lo que se busca lo
// pone el buscador, que se declara más abajo (y a su vez recarga esta lista).
const library = useLibrary({
  view,
  params: () => ({ q: query.value, ...search.filters() })
})
const { songs, listCount, loadingMore, loading, stats, status, waiting } = library
const { configured, missingFolders, sort, sortDesc } = library
const { sortBy, applySort, load, setSongs, loadStatus } = library
// Los repertorios y sus acciones: crear, añadir, exportar, borrar. Necesita
// `view` (para saber de que lista se quita una cancion) y `load` (para
// volver a pedirla despues).
const playlistActions = usePlaylistActions({ view, reload: () => load() })
const { playlists } = playlistActions
// Lo que se está bajando, para el número de «Descargas» en la barra lateral.
const downloads = useDownloads()
// De dónde salió lo que suena, para marcarlo en la barra lateral: la lista,
// «Todas las canciones», Favoritos… Con un punto quieto si está en pausa.
const nowPlaying = computed(() => {
  const o = player.origin.value
  if (!o?.kind || !player.track.value) return null
  return { kind: o.kind, id: o.id ?? null, playing: player.state.playing }
})

// La agrupación que se aplica de verdad: «Artistas» siempre agrupa por
// artista, pero eso no cambia la preferencia del usuario para las demás.
const effectiveGroupBy = computed(() =>
  view.value.kind === 'artists' ? groupBy.value || 'artist' : groupBy.value
)
// La lista en el orden en que se ve. Agrupada, los grupos van por orden
// alfabético y dentro de cada uno el de la lista: Mayús elige el tramo que se
// ve entre las dos pulsadas y la cola sigue ese orden. Con el de la lista sin
// agrupar, en «Artistas» ordenada por título Mayús se llevaba canciones de
// otros grupos y «siguiente» saltaba de un artista a otro.
const shown = computed(() =>
  effectiveGroupBy.value ? groupedOrder(songs.value, effectiveGroupBy.value) : songs.value
)
// Solo en los repertorios que uno crea se cambia el orden arrastrando: en
// «Todas», Favoritos o una búsqueda el orden lo dan las columnas, y en la
// lista del reproductor, el momento en que se abrió cada archivo. Agrupada
// tampoco: dentro de un grupo no hay un orden de lista que mover.
const sortable = computed(() => view.value.kind === 'playlist' && !effectiveGroupBy.value)

// Qué está elegido y la ficha de la principal (ver useSelection).
const selection = useSelection({ shown })
const { selected, selectedIds, selectedSongs, detail, select } = selection
const jumpToSong = ref(null)
const viewMenu = ref(false)
const viewBox = useTemplateRef('viewBox')

/** El id de lo que suena, que es lo que las listas necesitan para marcarlo. */
const playingId = computed(() => player.track.value?.id ?? null)

// La ultima vista CON canciones: el chat es una pagina propia, asi que
// cuando se abre, «lo que el usuario esta viendo» es la lista de antes (que
// sigue en `songs`). Con eso el asistente entiende «la segunda», «esta» o
// «las seleccionadas» sin que se lo expliquen.
const listView = ref({ kind: 'all' })
watch(
  view,
  (v) => {
    if (!PAGES.includes(v.kind)) listView.value = { ...v }
  },
  { immediate: true }
)
const brief = (s) => ({ id: s.id, artist: s.artist || '', title: s.title || '' })
/**
 * Lo que la persona tiene delante, para el asistente. Es una función que el
 * chat llama al mandar cada mensaje, no un `computed` que se le pasa: el
 * computed dependía de lo que suena, que Rust cuenta cuatro veces por
 * segundo, y el chat se repintaba entero (con todo su markdown) en cada tick.
 * @returns {import('./api.js').ChatContext}
 */
function chatContext() {
  const t = player.track.value
  const marked = selectedSongs.value.length
    ? selectedSongs.value
    : shown.value.filter((s) => s.id === selected.value)
  const names = {
    all: 'Todas las canciones',
    favorites: 'Favoritos',
    artists: 'Artistas',
    player: 'Reproductor'
  }
  const name =
    listView.value.kind === 'playlist'
      ? listView.value.name
      : query.value.trim()
        ? `Búsqueda «${query.value.trim()}»`
        : names[listView.value.kind] || 'la biblioteca'
  return {
    view: { kind: listView.value.kind, name, id: listView.value.id ?? undefined },
    songs: shown.value.slice(0, 20).map(brief),
    total: songs.value.length,
    selected: marked.slice(0, 20).map(brief),
    playing: t ? { ...brief(t), paused: !player.state.playing } : null
  }
}

// La canción que se lleva en la mano, para pintar el fantasma que sigue al
// puntero. Quien la recibe se declara con `data-drop`; ver useDragSong.
const { drag, cancelDrag } = useDragSong()
// En estrecho los laterales no caben: pasan a abrirse encima del contenido.
// La decisión vive aquí y no repartida en media queries porque cambia el
// COMPORTAMIENTO, no solo el aspecto.
const { isPhone, isCompact } = useViewport()
const navOpen = ref(false)
const detailsOpen = ref(false)

// Páginas que no tienen ninguna canción que seleccionar: ahí el panel de
// detalle solo diría «selecciona una canción». En Duplicados SÍ se queda,
// porque desde ahí se pueden escuchar las copias y la ficha se llena.
const WITHOUT_DETAILS = ['settings', 'chat', 'downloads', 'inbox']
const detailsVisible = computed(
  () => showDetails.value && !WITHOUT_DETAILS.includes(view.value.kind)
)
// Las páginas (Ajustes, el asistente, Descargas…) van por el enrutador, que
// las carga al abrirlas y deja el asistente vivo al salir (ver router.js).
// Quien manda es `view`: el enrutador lo sigue.
const router = useRouter()
const isPage = computed(() => PAGES.includes(view.value.kind))
watch(view, (v) => {
  router.push(routeFor(v)).catch((e) => notify('No se pudo abrir la página: ' + errorMessage(e)))
})
/** Lo que recibe cada página, además de lo suyo. */
const pageBindings = computed(
  () =>
    ({
      downloads: { onReload: refreshAll },
      settings: { onReindexed: refreshAll, onChanged: refreshAll },
      inbox: { waiting: waiting.value, onChanged: refreshAll, onGo: (v) => (view.value = v) },
      duplicates: { onChanged: refreshAll }
    })[view.value.kind] || {}
)

// al navegar se cierra el panel: en móvil tapa toda la pantalla
watch(view, () => {
  navOpen.value = false
  detailsOpen.value = false
  selection.clear()
})
watch(isCompact, (v) => {
  if (!v) {
    navOpen.value = false
    detailsOpen.value = false
  }
})
// tocar una canción en móvil abre su ficha: es lo que se espera
watch(detail, (c) => {
  if (c && isPhone.value && showDetails.value) detailsOpen.value = true
})

// Atajo para volver a lo que suena. Solo aparece si la lista es larga: con
// pocas canciones se ve todo y el botón solo estorbaría.
const centerEl = useTemplateRef('centerEl')
// el panel que se desplaza, en cualquiera de las cuatro vistas (la lista fina
// y las fichas no estaban: ahí el atajo no salía nunca)
const scrollBox = () => centerEl.value?.querySelector('.table-wrap, .grid, .rows, .cards')
const { hasScroll, recheck: recheckScroll } = useHasScroll(scrollBox)
const { offscreen, recheck: recheckVisible } = useIsOffscreen(scrollBox, () =>
  centerEl.value?.querySelector('.playing')
)
const canJumpToPlaying = computed(
  () => !!playingId.value && hasScroll.value && offscreen.value && !isPage.value
)

const REPEAT_NAMES = {
  list: 'repetir la lista',
  one: 'repetir esta canción',
  once: 'solo esta canción',
  queue: 'la lista una vez'
}

onClickOutside(viewBox, () => {
  viewMenu.value = false
})

// La bienvenida sale mientras no haya nada que enseñar: sin carpetas, o con
// la biblioteca vacía porque la música se movió o se borró por fuera (ver
// WelcomePage). Nunca en Ajustes, que es donde se arregla; y con carpetas
// pero sin canciones, solo en las vistas de la biblioteca: el chat, las
// descargas o una lista con archivos de fuera siguen teniendo sentido.
const LIBRARY_VIEWS = ['all', 'favorites', 'artists']
const showWelcome = computed(() => {
  if (view.value.kind === 'settings') return false
  if (!configured.value) return !songs.value.length
  return stats.value?.total === 0 && LIBRARY_VIEWS.includes(view.value.kind)
})

// Para no recargar en cada aviso: solo cuando el núcleo PASA a estar listo.
let coreWasReady = false
let stopCoreWatch = null
let stopExternalWatch = null
let stopChangeWatch = null

const title = computed(
  () =>
    ({
      all: 'Todas las canciones',
      favorites: 'Favoritos',
      artists: 'Artistas',
      inbox: 'Entrada',
      settings: 'Ajustes',
      duplicates: 'Duplicados',
      chat: 'Asistente',
      downloads: 'Descargas',
      player: 'Reproductor',
      playlist: view.value.name
    })[view.value.kind] || ''
)

// ------------------------------------------------------------------ cargar
/**
 * Recarga TODO lo que la app tiene en memoria: estado y contadores, la lista
 * de la vista, los repertorios, la ficha abierta y los datos de la cola.
 *
 * Es lo que se llama cuando algo cambia en el núcleo, venga de donde venga
 * (el asistente, una descarga que termina, Ajustes, la línea de órdenes).
 * Antes cada página refrescaba «lo suyo» y lo demás se quedaba congelado
 * hasta salir y volver a entrar: la lista decía «6 temas» con tres.
 */
async function refreshAll() {
  await Promise.all([loadStatus(), load(true), playlistActions.load(), refreshDetail()])
}

/** La ficha abierta y la copia de la cola, con lo que diga el núcleo ahora. */
async function refreshDetail() {
  const song = await selection.refreshDetail()
  if (song) player.patchItem(song)
}

// Los avisos de cambio llegan cada dos segundos como mucho, y una descarga
// de tres canciones dispara varios seguidos: se agrupan y se refresca una vez.
let changeTimer = null
function onCoreChanged() {
  clearTimeout(changeTimer)
  changeTimer = setTimeout(() => refreshAll(), 250)
}

/**
 * Pone a sonar. La cola pasa a ser la lista que se está viendo, salvo que se
 * diga otra cosa: es lo que se espera al pulsar una canción de una lista.
 */
function play(song, list = null, origin = null) {
  selected.value = song.id
  // Sobre la que ya está puesta, el botón de la fila es pausa/reanudar: antes
  // volvía a empezar la canción, y no había forma de pararla desde la lista.
  if (!list && player.track.value?.id === song.id) return player.toggle()
  player.setQueue(list || shown.value, song.id, origin || { ...view.value, label: title.value })
  selection.showDetail(song.id)
}

// La ficha como ventana emergente: cuando el panel lateral está oculto (o
// en pantallas estrechas, donde es un cajón), «Ver detalles» la abre aquí.
const detailModal = ref(false)
const detailModalEl = useTemplateRef('detailModalEl')
// como un dialogo: el foco se queda dentro y al cerrar vuelve a donde estaba
useFocusTrap(detailModalEl, { active: detailModal })
const sidePanelShown = computed(() => detailsVisible.value && !isCompact.value)
async function showDetailsOf(song) {
  await select(song.id)
  if (isCompact.value && detailsVisible.value) detailsOpen.value = true
  else detailModal.value = true
}

/** Pone a sonar una lista entera desde el principio. */
function playList(list, origin) {
  if (!list.length) return
  play(list[0], list, origin)
}

/** Play sin nada cargado: suena lo que esté seleccionado en la lista. */
function playSelected() {
  const song = shown.value.find((x) => x.id === selected.value) || shown.value[0]
  if (song) play(song)
}

/**
 * Órdenes que llegan del asistente. El audio lo maneja Rust y la cola vive
 * en `usePlayback`, así que el núcleo no puede reproducir por su cuenta:
 * manda la orden y se ejecuta aquí.
 */
async function runAction(a) {
  if (!a?.kind) return
  if (a.kind === 'play_song') {
    const song = songs.value.find((x) => x.id === a.song_id) || (await api.song(a.song_id))
    if (song) play(song)
    return
  }
  if (a.kind === 'play_playlist') {
    const pl = playlists.value.find((l) => l.id === a.playlist_id)
    const list = (await api.playlistSongs(a.playlist_id)).songs || []
    if (!list.length) return notify('Esa lista está vacía')
    playList(list, {
      kind: 'playlist',
      id: a.playlist_id,
      name: pl?.name,
      label: pl?.name || 'la lista'
    })
    return
  }
  if (a.kind === 'player') {
    const orders = {
      next: () => player.next(),
      previous: () => player.previous(),
      stop: () => player.stop(),
      toggle: () => player.toggle(),
      pause: () => player.state.playing && player.toggle(),
      resume: () => !player.state.playing && player.toggle()
    }
    await orders[a.command]?.()
  }
}

// El asistente sigue su conversacion aunque su pagina no se vea: lo que pida
// (reproducir, recargar) lo hace la app, que no se desmonta nunca.
const disconnectChat = connectChat({ reload: refreshAll, action: runAction, context: chatContext })

/** Guarda la lista del reproductor como una lista de DanPlay. */
async function savePlayerList() {
  const name = await ask({
    kind: 'prompt',
    title: 'Guardar la lista',
    message:
      'Se guarda tal como está, en el mismo orden. Las canciones de fuera no se copian a la biblioteca: la lista las nombra donde estén.',
    value: 'Del reproductor',
    placeholder: 'Nombre de la lista',
    okLabel: 'Guardar'
  })
  if (!name) return
  try {
    const r = await api.externalSave(String(name).trim())
    notify(`Guardada «${r.name}» con ${r.n} ${r.n === 1 ? 'canción' : 'canciones'}`)
    await playlistActions.load()
  } catch (e) {
    notify(errorMessage(e))
  }
}

/** Vacía la lista del reproductor. Ni toca los archivos ni lo ya guardado. */
async function discardPlayerList() {
  const ok = await ask({
    title: '¿Descartar la lista?',
    message:
      'Se vacía la lista del reproductor. Los archivos siguen donde estén, y las listas que hayas guardado no se tocan.',
    okLabel: 'Descartar',
    danger: true
  })
  if (!ok) return
  try {
    await api.externalClear()
    setSongs([])
  } catch (e) {
    notify(errorMessage(e))
  }
}

/** Vuelve a la lista desde la que salió lo que suena y salta a la canción. */
async function goToOrigin() {
  const origin = player.origin.value
  if (!origin) return
  const { label, ...target } = origin
  void label
  if (target.kind && JSON.stringify(target) !== JSON.stringify(view.value)) {
    view.value = target
    await nextTick()
  }
  selected.value = playingId.value
  jumpToSong.value = playingId.value
  setTimeout(() => (jumpToSong.value = null), 1200)
}

// -------------------------------------------------------------- acciones
// Los menús de clic derecho y lo que se hace desde ellos (ver useSongMenus).
const menus = useSongMenus({
  view,
  player,
  playlistActions,
  selection,
  play,
  onUpdated,
  refreshAll,
  reload: (quiet) => load(quiet),
  detailsInView: () => sidePanelShown.value,
  showDetailsOf
})
const { setStars, toggleFavorite, toggleBlur, songMenu, playlistMenu } = menus

// Qué hacer cuando se suelta una canción arrastrada. Los destinos se declaran
// con `data-drop` allí donde estén, así que aquí solo hay que decidir qué
// significa cada uno.
onDrop(async (target, song, { after } = {}) => {
  const name = song.title || song.file
  if (target.startsWith('sort:')) {
    if (sortable.value) library.moveInPlaylist(song, Number(target.slice(5)), !!after)
  } else if (target === 'favorites') {
    if (song.favorite) return notify(`«${name}» ya estaba en favoritos`)
    await toggleFavorite(song)
    notify(`«${name}» a favoritos`, 'ok')
  } else if (target === 'new-playlist') {
    playlistActions.create(song)
  } else if (target.startsWith('playlist:')) {
    const pl = playlists.value.find((l) => String(l.id) === target.slice(9))
    if (pl) playlistActions.addTo(song, pl)
  }
})

// --------------------------------------------------------------- buscador
const searchBox = useTemplateRef('searchBox')
const resultsEl = useTemplateRef('resultsEl')
const search = useSearch({ view, reload: () => load() })
const { query, quick, quickLoading, quickOpen, advanced, facets, onlyFavorites, minStars } = search

/** Al elegir un resultado: se abre su ficha sin moverte de sitio. */
async function pickResult(song) {
  quickOpen.value = false
  await select(song.id)
}
/**
 * Reproducir un resultado del desplegable.
 *
 * La cola pasa a ser LO ENCONTRADO, no la lista de la página: si no,
 * «siguiente» no recorrería los resultados, que es lo que se espera después
 * de buscar.
 */
function playResult(song) {
  quickOpen.value = false
  play(song, quick.value, { kind: 'all', label: `Búsqueda «${query.value.trim()}»` })
}
/** «Verlos todos»: se va a la biblioteca con la misma búsqueda puesta. */
function seeAllResults() {
  quickOpen.value = false
  view.value = { kind: 'all' }
}
function onSearchKey(e) {
  if (quickOpen.value && resultsEl.value?.onKey(e)) return
  if (e.key === 'Escape') search.close()
}
onClickOutside(searchBox, search.close)

// Sin `deep`: solo interesa cuando cambia la LISTA (otra búsqueda, otra vista,
// otra agrupación), que es lo que altera el alto del scroll. Vigilarla en
// profundidad obligaba a recorrer las mil canciones y todos sus campos cada
// vez que se tocaba una sola —poner una estrella, por ejemplo— para acabar
// midiendo un scroll que no había cambiado.
watch([songs, layout, groupBy, view], () => {
  recheckScroll()
  recheckVisible()
})
watch(playingId, recheckVisible)

/** Salir de DanPlay del todo, que con la bandeja ya no es cerrar la ventana. */
async function quit() {
  viewMenu.value = false
  await tauriApp.quit()
}
useHotkeys({ 'ctrl+q': quit }, { global: true })

function toggleAdvanced() {
  advanced.value = !advanced.value
  quickOpen.value = false
}
function goTo(v) {
  view.value = v
  viewMenu.value = false
}
function goToSettings() {
  view.value = { kind: 'settings' }
  detailsOpen.value = false
}

let stopFocusRelease = null
function blockContextMenu(e) {
  e.preventDefault()
}
function onEscape(e) {
  if (e.key !== 'Escape') return
  if (drag.song) cancelDrag()
  if (detailModal.value) detailModal.value = false
}

onMounted(async () => {
  // El WebView trae su propio menú de clic derecho (recargar, inspeccionar).
  // En una app de escritorio no pinta nada: se bloquea y cada sitio pone el
  // suyo con las opciones que tengan sentido ahí.
  window.addEventListener('contextmenu', blockContextMenu)
  window.addEventListener('keydown', onEscape)
  // que tras pulsar un botón con el ratón el espacio siga pausando la música
  stopFocusRelease = releaseFocusAfterPointer(window)
  prefs.applyAll()

  // El núcleo puede tardar un segundo en levantarse, y arrancar la app
  // abriendo una canción desde el explorador es justo cuando más tarda:
  // todo pasa a la vez. Si se carga antes de que conteste, la biblioteca
  // sale vacía —«Nada por aquí»— y ahí se quedaba, porque nadie escuchaba
  // este aviso. Rust ya lo mandaba desde el principio.
  //
  // Se escucha ANTES de la primera carga: si el núcleo aún no contesta, esa
  // carga falla, y antes el fallo cortaba el arranque sin llegar a escuchar
  // nada. La app se quedaba vacía para siempre.
  stopCoreWatch = await core.onStatus((e) => {
    if (e?.ready && !coreWasReady) refreshAll()
    coreWasReady = !!e?.ready
  })
  try {
    await loadStatus()
    await Promise.all([configured.value ? load() : Promise.resolve(), playlistActions.load()])
  } catch {
    /* el núcleo aún no contesta: el aviso de arriba lo carga todo al llegar */
  }
  offerToBeDefault()

  // Abrir una canción desde el explorador la mete en la lista del
  // reproductor. Si esa lista está delante, tiene que aparecer sola: se
  // cargaba al entrar y se quedaba quieta mientras iban llegando canciones.
  stopExternalWatch = await api.onExternal(() => {
    if (view.value.kind === 'player') load(true)
  })

  // Cualquier cambio en el núcleo —lo haga quien lo haga— se refleja aquí
  // sin salir y volver a entrar. Rust avisa; esta ventana escucha.
  stopChangeWatch = await core.onChanged(onCoreChanged)
  menus.loadShareTargets()
  // y si había una descarga en marcha (un reinicio de la ventana), que se vea
  downloads.refresh()
})

/** La clave de «ya lo pregunté». Una vez en la vida, no en cada arranque. */
const ASKED_DEFAULT = 'danplay.default-player-asked'

/**
 * La primera vez, ofrece abrir las canciones con DanPlay.
 *
 * Aquí y no en el instalador porque en Linux el reproductor predeterminado es
 * un ajuste TUYO (vive en `~/.config/mimeapps.list`): un paquete que se
 * instala como root no puede ponerlo, y si lo pusiera estaría decidiendo por
 * ti. Se pregunta una sola vez; quien diga que no lo tiene en Ajustes.
 */
async function offerToBeDefault() {
  try {
    if (localStorage.getItem(ASKED_DEFAULT)) return
  } catch {
    return // sin almacenamiento no hay forma de recordar el «no», así que ni se pregunta
  }
  const state = await tauriApp.defaultPlayer()
  if (!state?.supported || state.is_default) return
  try {
    localStorage.setItem(ASKED_DEFAULT, '1')
  } catch {
    return
  }
  const yes = await ask({
    title: '¿Abrir las canciones con DanPlay?',
    message: state.direct
      ? 'Al abrir una canción desde el explorador de archivos sonará aquí. Puedes cambiarlo cuando quieras en Ajustes.'
      : 'DanPlay quedará en «Abrir con». Windows pide que el último clic lo des tú, así que se abrirá su ventana de Ajustes.',
    okLabel: 'Sí, que las abra'
  })
  if (!yes) return
  try {
    const now = await tauriApp.makeDefaultPlayer()
    if (now?.is_default) notify('Ya se abren con DanPlay')
    else if (now?.note) notify(now.note)
  } catch (e) {
    notify(errorMessage(e))
  }
}

// si la app se va con algo en la mano, que no queden escuchas sueltas
onUnmounted(() => {
  disconnectChat()
  stopCoreWatch?.()
  stopExternalWatch?.()
  stopChangeWatch?.()
  clearTimeout(changeTimer)
  cancelDrag()
  window.removeEventListener('contextmenu', blockContextMenu)
  window.removeEventListener('keydown', onEscape)
  stopFocusRelease?.()
})

/**
 * Una canción cambió: se refresca en la lista, en la ficha y en la cola.
 *
 * La ficha solo si es la suya. Las respuestas llegan cuando llegan: «Buscar
 * letra» de A con B ya elegida devolvía la ficha a A, y poner estrellas en
 * una fila cambiaba la ficha a esa fila sin haberla elegido.
 */
function onUpdated(song) {
  if (!song) return
  selection.patchDetail(song)
  library.patchSong(song)
  player.patchItem(song)
}
</script>

<template>
  <div class="app">
    <header class="topbar">
      <button
        v-if="isCompact"
        type="button"
        class="icon-btn nav-toggle"
        title="Menú"
        @click="navOpen = true"
      >
        <Icon n="viewList" :t="17" />
      </button>
      <div class="brand"><span class="brand-dot"></span> DANPLAY</div>
      <div ref="searchBox" class="search-box">
        <TextField
          v-model="query"
          icon="search"
          width="100%"
          placeholder="Buscar…  artista:barak  tono:Bb  bpm>100  duracion>300"
          aria-label="Buscar en la biblioteca"
          role="combobox"
          aria-autocomplete="list"
          :aria-expanded="quickOpen && !advanced"
          :aria-controls="quickOpen && !advanced ? resultsEl?.listId : undefined"
          :aria-activedescendant="quickOpen && !advanced ? resultsEl?.activeId : undefined"
          @keydown="onSearchKey"
        >
          <template #actions>
            <button
              class="field-btn search-more"
              :class="{ on: advanced }"
              type="button"
              tabindex="-1"
              title="Búsqueda avanzada: filtros y orden"
              @click.prevent="toggleAdvanced"
            >
              <Icon n="viewOptions" :t="15" />
            </button>
          </template>
        </TextField>

        <transition name="dropdown">
          <SearchPanel
            v-if="advanced"
            v-model:query="query"
            v-model:only-favorites="onlyFavorites"
            v-model:min-stars="minStars"
            :facets="facets"
            :sort="sort"
            :desc="sortDesc"
            @sort="applySort"
            @close="advanced = false"
          />
        </transition>

        <transition name="dropdown">
          <SearchResults
            v-if="quickOpen && !advanced"
            ref="resultsEl"
            :songs="quick"
            :loading="quickLoading"
            :query="query"
            @pick="pickResult"
            @play="playResult"
            @see-all="seeAllResults"
            @close="quickOpen = false"
          />
        </transition>
      </div>

      <div class="view-switch" role="group" aria-label="Cómo se ve la lista">
        <button
          v-for="v in VIEWS"
          :key="v"
          type="button"
          :class="{ on: layout === v }"
          :title="VIEW_NAMES[v]"
          :aria-pressed="layout === v"
          @click="layout = v"
        >
          <Icon :n="VIEW_ICONS[v]" :t="15" />
        </button>
      </div>

      <div ref="viewBox" style="position: relative">
        <button
          type="button"
          class="btn mini"
          style="gap: 6px"
          :aria-expanded="viewMenu"
          @click="viewMenu = !viewMenu"
        >
          <Icon n="viewOptions" :t="14" /> Vista
        </button>
        <transition name="dropdown">
          <div v-if="viewMenu" class="card view-menu">
            <!-- separado en dos: lo que cambia la LISTA que estás viendo y lo
                 que cambia la APP entera. Estaba todo revuelto y había que
                 leérselo entero para dar con lo que buscabas. -->
            <div class="menu-block">
              <div class="menu-head">Esta lista</div>
              <!-- En estrecho el conmutador de la barra no cabe y se esconde;
                   el css ya lo decía, pero aquí no había forma de elegir. -->
              <SelectField
                v-model="layout"
                label="Cómo se ve"
                :options="VIEWS.map((v) => ({ v, n: VIEW_NAMES[v] }))"
              />
              <SelectField
                v-model="groupBy"
                label="Agrupar"
                :options="GROUPINGS.map((a) => ({ v: a.v, n: a.n }))"
              />
              <SelectField
                v-model="density"
                label="Densidad de las filas"
                :options="Object.entries(DENSITIES).map(([k, d]) => ({ v: k, n: d.name }))"
              />
              <SliderField
                v-if="layout === 'grid'"
                v-model="cardSize"
                :min="110"
                :max="280"
                :step="10"
                label="Tamaño de ficha"
                :value-text="cardSize + ' px'"
              />
              <ToggleField
                v-model="showDetails"
                title="Panel de detalle"
                hint="La columna con la letra y los acordes"
              />
            </div>

            <div class="menu-block">
              <div class="menu-head">La app</div>
              <SelectField
                v-model="theme"
                label="Tema"
                :options="
                  Object.entries(allThemes()).map(([k, t]) => ({
                    v: k,
                    n: t.name,
                    note: KIND_LABEL[t.kind] || t.kind,
                    color: t.v.accent
                  }))
                "
              />
              <SelectField
                v-model="appSize"
                label="Tamaño de la app"
                :options="
                  Object.entries(SIZES).map(([k, s]) => ({ v: k, n: s.name, note: s.note }))
                "
              />
              <button class="btn mini mini-open" title="Salir de DanPlay (Ctrl+Q)" @click="quit">
                <Icon n="close" :t="14" /> Salir de DanPlay
              </button>
            </div>
          </div>
        </transition>
      </div>
      <span v-if="stats" class="syntax-hint">{{ stats.total }} temas</span>
    </header>

    <div class="main">
      <Sidebar
        v-if="!isCompact"
        :view="view"
        :playlists="playlists"
        :stats="stats"
        :entrada="waiting"
        :download="downloads.state"
        :now-playing="nowPlaying"
        @go="goTo"
        @new-playlist="playlistActions.create()"
        @playlist-menu="playlistMenu"
      />

      <Drawer v-else :open="navOpen" side="left" title="DanPlay" @close="navOpen = false">
        <Sidebar
          :view="view"
          :playlists="playlists"
          :stats="stats"
          :entrada="waiting"
          :download="downloads.state"
          :now-playing="nowPlaying"
          @go="goTo"
          @new-playlist="playlistActions.create()"
          @playlist-menu="playlistMenu"
        />
      </Drawer>

      <main ref="centerEl" class="center">
        <WelcomePage
          v-if="showWelcome"
          :missing="missingFolders"
          :empty="configured"
          @ready="refreshAll"
        />

        <!-- Las páginas: se cargan al abrirlas, y el asistente se queda vivo
             al salir (lo que tenías escrito, por dónde ibas del hilo). El
             RouterView no se quita nunca: con él se iría lo que guarda
             <KeepAlive> -->
        <RouterView v-slot="{ Component, route }">
          <KeepAlive include="ChatPage">
            <component
              :is="Component"
              v-if="!showWelcome && isPage && Component && route.name === view.kind"
              v-bind="pageBindings"
            />
          </KeepAlive>
        </RouterView>

        <template v-if="!showWelcome && !isPage">
          <div class="filters">
            <strong style="font-size: 13px">{{ title }}</strong>
            <span class="chip" :title="loadingMore ? `llevan ${songs.length}` : undefined">{{
              listCount || songs.length
            }}</span>
            <span v-if="loadingMore" class="sort-hint">cargando el resto…</span>
            <button
              v-if="groupBy && view.kind !== 'artists'"
              type="button"
              class="chip x"
              title="Dejar de agrupar"
              :aria-label="
                'Dejar de agrupar (' + GROUPINGS.find((a) => a.v === groupBy)?.n.toLowerCase() + ')'
              "
              @click="groupBy = ''"
            >
              {{ GROUPINGS.find((a) => a.v === groupBy)?.n }} ×
            </button>
            <button
              v-if="query"
              type="button"
              class="chip x"
              title="Quitar la búsqueda"
              :aria-label="'Quitar la búsqueda «' + query + '»'"
              @click="query = ''"
            >
              «{{ query }}» ×
            </button>
            <span v-if="shuffle" class="chip on">aleatorio</span>
            <span v-if="repeat !== 'list'" class="chip on">{{ REPEAT_NAMES[repeat] }}</span>
            <!-- que se sepa que en un repertorio el orden se cambia a mano -->
            <span v-if="sortable && songs.length > 1" class="sort-hint">
              arrastra una canción para cambiar el orden
            </span>
            <template v-if="view.kind === 'player' && songs.length">
              <button class="btn mini" style="margin-left: auto" @click="savePlayerList">
                <Icon n="save" :t="13" /> Guardar
              </button>
              <button class="btn mini" @click="discardPlayerList">
                <Icon n="trash" :t="13" /> Descartar
              </button>
            </template>
            <Loading v-if="loading" text="" inline style="margin-left: auto" />
          </div>

          <GroupedSongs
            v-if="effectiveGroupBy"
            :songs="songs"
            :by="effectiveGroupBy"
            :layout="layout"
            :sort="sort"
            :desc="sortDesc"
            :selected="selected"
            :selected-ids="selectedIds"
            :playing="playingId"
            :size="cardSize"
            :jump-to="jumpToSong"
            @sort-by="sortBy"
            @select="select"
            @play="play"
            @context="songMenu"
            @set-stars="setStars"
            @toggle-favorite="toggleFavorite"
          />
          <SongRows
            v-else-if="layout === 'rows'"
            :songs="songs"
            :sortable="sortable"
            :selected="selected"
            :selected-ids="selectedIds"
            :playing="playingId"
            :jump-to="jumpToSong"
            @select="select"
            @play="play"
            @set-stars="setStars"
            @toggle-favorite="toggleFavorite"
            @context="songMenu"
          />

          <SongCards
            v-else-if="layout === 'cards'"
            :songs="songs"
            :sortable="sortable"
            :selected="selected"
            :selected-ids="selectedIds"
            :playing="playingId"
            :jump-to="jumpToSong"
            @select="select"
            @play="play"
            @set-stars="setStars"
            @toggle-favorite="toggleFavorite"
            @context="songMenu"
          />

          <SongGrid
            v-else-if="layout === 'grid'"
            :songs="songs"
            :sortable="sortable"
            :selected="selected"
            :selected-ids="selectedIds"
            :playing="playingId"
            :size="cardSize"
            :jump-to="jumpToSong"
            @select="select"
            @play="play"
            @context="songMenu"
          />
          <SongTable
            v-else
            :songs="songs"
            :sortable="sortable"
            :selected="selected"
            :selected-ids="selectedIds"
            :playing="playingId"
            :sort="sortable ? '' : sort"
            :desc="sortDesc"
            :jump-to="jumpToSong"
            @select="select"
            @play="play"
            @context="songMenu"
            @set-stars="setStars"
            @toggle-favorite="toggleFavorite"
            @sort-by="sortBy"
          />
        </template>

        <transition name="pop">
          <button
            v-if="canJumpToPlaying"
            class="jump-fab"
            title="Ir a la canción que suena"
            @click="goToOrigin"
          >
            <Icon n="note" :t="17" />
          </button>
        </transition>
      </main>

      <DetailsPanel
        v-if="detailsVisible && !isCompact"
        :song="detail"
        :ai-ready="!!status?.ai"
        @updated="onUpdated"
        @toggle-blur="toggleBlur"
        @go-settings="view = { kind: 'settings' }"
      />

      <Teleport to="body">
        <transition name="fade">
          <div
            v-if="detailModal"
            class="modal-back"
            role="presentation"
            @mousedown.self="detailModal = false"
          >
            <div
              ref="detailModalEl"
              class="modal details-modal"
              role="dialog"
              aria-modal="true"
              aria-label="Ficha de la canción"
            >
              <button
                type="button"
                class="btn mini details-modal-close"
                title="Cerrar"
                @click="detailModal = false"
              >
                <Icon n="close" :t="14" />
              </button>
              <DetailsPanel
                :song="detail"
                :ai-ready="!!status?.ai"
                @updated="onUpdated"
                @toggle-blur="toggleBlur"
                @go-settings="((detailModal = false), goToSettings())"
              />
            </div>
          </div>
        </transition>
      </Teleport>

      <Drawer
        v-if="isCompact"
        :open="detailsOpen && detailsVisible"
        side="right"
        title="Ficha"
        width="min(400px, 92vw)"
        @close="detailsOpen = false"
      >
        <DetailsPanel
          :song="detail"
          :ai-ready="!!status?.ai"
          @updated="onUpdated"
          @toggle-blur="toggleBlur"
          @go-settings="goToSettings"
        />
      </Drawer>
    </div>

    <transition-group name="toast" tag="div" class="toasts" role="status" aria-live="polite">
      <div
        v-for="a in notices"
        :key="a.id"
        class="toast"
        :class="a.kind === 'ok' ? 'hint-ok' : 'hint-amber'"
      >
        <Icon :n="a.kind === 'ok' ? 'check' : 'warning'" :t="15" />
        <span>{{ a.message }}</span>
        <button class="close" title="Cerrar el aviso" @click="dismiss(a.id)">
          <Icon n="close" :t="13" />
        </button>
      </div>
    </transition-group>

    <teleport to="body">
      <div v-if="drag.song" class="drag-ghost" :style="{ left: drag.x + 'px', top: drag.y + 'px' }">
        <CoverArt
          :id="drag.song.id"
          :blur="!!drag.song.blur"
          class="ghost-art"
          :icon-size="13"
          :size="96"
          :alt="drag.song.title || drag.song.file"
        />
        <span class="ghost-name">{{ drag.song.title || drag.song.file }}</span>
      </div>
    </teleport>

    <!-- el estudio sube por encima de la lista, sin recolocar nada detrás -->
    <transition name="study">
      <StudyBar v-if="studyOpen" @close="studyOpen = false" />
    </transition>
    <Player
      :study="studyOpen"
      @go-to-origin="goToOrigin"
      @play-selected="playSelected"
      @toggle-study="studyOpen = !studyOpen"
    />

    <ContextMenu
      :open="menu.open"
      :x="menu.x"
      :y="menu.y"
      :items="menu.items"
      :title="menu.title"
      :keyboard="menu.keyboard"
      @close="closeMenu"
    />

    <ModalDialog v-bind="dialog" @ok="dialogOk" @cancel="dialogCancel" />
  </div>
</template>
