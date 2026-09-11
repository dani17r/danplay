<script setup>
/**
 * La ventana principal: orquesta y poco más.
 *
 * Lo que antes vivía aquí y ahora vive fuera:
 *   la cola y lo que suena   → composables/usePlayback.js (el estado está en Rust)
 *   los avisos               → composables/useNotices.js
 *   los diálogos             → composables/useDialog.js
 *   el menú contextual       → composables/useContextMenu.js
 *   tema, densidad y vistas  → composables/usePreferences.js
 *   bienvenida, entrada y duplicados → sus propios componentes
 */
import { ref, onMounted, onUnmounted, watch, computed, nextTick } from 'vue'
import { api, app as tauriApp, core, errorMessage } from './api.js'
import { onClickOutside } from './composables/useClickOutside.js'
import { useHasScroll, useIsOffscreen } from './composables/useHasScroll.js'
import { useViewport } from './composables/useViewport.js'
import { useDragSong, onDrop } from './composables/useDragSong.js'
import { usePlayback } from './composables/usePlayback.js'
import { useNotices, notify } from './composables/useNotices.js'
import { useDialog, ask } from './composables/useDialog.js'
import { useContextMenu, openMenu, closeMenu } from './composables/useContextMenu.js'
import {
  usePreferences,
  VIEWS,
  VIEW_NAMES,
  VIEW_ICONS,
  GROUPINGS
} from './composables/usePreferences.js'
import { useHotkeys } from './composables/useHotkeys.js'
import { useSearch } from './composables/useSearch.js'
import { usePlaylistActions } from './composables/usePlaylistActions.js'
import { useDownloads } from './composables/useDownloads.js'
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
import SettingsPage from './components/SettingsPage.vue'
import WelcomePage from './components/WelcomePage.vue'
import InboxPage from './components/InboxPage.vue'
import DuplicatesPage from './components/DuplicatesPage.vue'
import Icon from './components/Icon.vue'
import ChatPage from './components/ChatPage.vue'
import StudyBar from './components/StudyBar.vue'
import DownloadsPage from './components/DownloadsPage.vue'
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
// Los repertorios y sus acciones: crear, añadir, exportar, borrar. Se declara
// aqui y no arriba porque necesita `view` (para saber de que lista se quita
// una cancion) y `load` (para volver a pedirla despues).
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
const sort = ref('artist')
// Hacia dónde ordena. Pulsar la misma cabecera la invierte; pulsar otra
// empieza por lo que tenga sentido en ese campo (A-Z en un texto, lo más
// largo primero en una duración).
const sortDesc = ref(false)
function sortBy(field, descByDefault = false) {
  if (sort.value === field) sortDesc.value = !sortDesc.value
  else {
    sort.value = field
    sortDesc.value = descByDefault
  }
}
const songs = ref([])
const stats = ref(null)
const status = ref(null) // /api/status entero: hace falta el flag de IA
const waiting = ref(0) // archivos en la Entrada
const selected = ref(null)
// La selección múltiple: Ctrl (o Cmd) añade o quita; Mayús coge el tramo
// desde la última pulsada; Ctrl+Mayús suma el tramo a lo que había. Sobre
// varias, el menú actúa sobre todas (enviar, añadir a una lista, papelera…).
const selectedIds = ref([])
let anchor = null
const detail = ref(null)
const jumpToSong = ref(null)
const loading = ref(false)
const configured = ref(true)
const viewMenu = ref(false)
const viewBox = ref(null)

/** El id de lo que suena, que es lo que las listas necesitan para marcarlo. */
const playingId = computed(() => player.track.value?.id ?? null)

// La ultima vista CON canciones: el chat es una pagina propia, asi que
// cuando se abre, «lo que el usuario esta viendo» es la lista de antes (que
// sigue en `songs`). Con eso el asistente entiende «la segunda», «esta» o
// «las seleccionadas» sin que se lo expliquen.
const PAGES = ['settings', 'inbox', 'chat', 'downloads', 'duplicates']
const listView = ref({ kind: 'all' })
watch(view, (v) => { if (!PAGES.includes(v.kind)) listView.value = { ...v } }, { immediate: true })
const brief = (s) => ({ id: s.id, artist: s.artist || '', title: s.title || '' })
/** @type {import('vue').ComputedRef<import('./api.js').ChatContext>} */
const chatContext = computed(() => {
  const t = player.track.value
  const marked = selectedSongs.value.length
    ? selectedSongs.value
    : songs.value.filter((s) => s.id === selected.value)
  const names = { all: 'Todas las canciones', favorites: 'Favoritos', artists: 'Artistas', player: 'Reproductor' }
  const name = listView.value.kind === 'playlist' ? listView.value.name
    : (query.value.trim() ? `Búsqueda «${query.value.trim()}»` : names[listView.value.kind] || 'la biblioteca')
  return {
    view: { kind: listView.value.kind, name, id: listView.value.id ?? undefined },
    songs: songs.value.slice(0, 20).map(brief),
    total: songs.value.length,
    selected: marked.slice(0, 20).map(brief),
    playing: t ? { ...brief(t), paused: !player.state.playing } : null
  }
})

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
// al navegar se cierra el panel: en móvil tapa toda la pantalla
watch(view, () => {
  navOpen.value = false
  detailsOpen.value = false
  selectedIds.value = []
  anchor = null
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
const centerEl = ref(null)
const scrollBox = () => centerEl.value?.querySelector('.table-wrap, .grid')
const { hasScroll, recheck: recheckScroll } = useHasScroll(scrollBox)
const { offscreen, recheck: recheckVisible } = useIsOffscreen(scrollBox, () =>
  centerEl.value?.querySelector('.playing')
)
const canJumpToPlaying = computed(
  () =>
    !!playingId.value &&
    hasScroll.value &&
    offscreen.value &&
    !['settings', 'chat', 'downloads', 'inbox', 'duplicates'].includes(view.value.kind)
)

const REPEAT_NAMES = {
  list: 'repetir la lista',
  one: 'repetir esta canción',
  once: 'solo esta canción',
  queue: 'la lista una vez'
}

// La agrupación que se aplica de verdad: «Artistas» siempre agrupa por
// artista, pero eso no cambia la preferencia del usuario para las demás.
onClickOutside(viewBox, () => {
  viewMenu.value = false
})
const effectiveGroupBy = computed(() =>
  view.value.kind === 'artists' ? groupBy.value || 'artist' : groupBy.value
)

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
// Las respuestas se numeran: con el retardo del buscador todavía podía llegar
// la de «bar» después de la de «barak» y pisar la lista con lo que ya no se
// estaba buscando.
let request = 0
/**
 * `quiet`: sin el indicador de carga. Es para los refrescos de fondo, que
 * pasan cada vez que algo cambia en el núcleo; el indicador es para cuando
 * la persona acaba de pedir algo y espera.
 */
async function load(quiet = false) {
  const mine = ++request
  if (!quiet) loading.value = true
  try {
    // el estado se relee siempre: si no, `configured` se quedaba congelado
    // en false y toda la vista central seguía mostrando la bienvenida
    const e = await api.status()
    if (mine !== request) return
    stats.value = e.stats
    configured.value = e.configured

    if (view.value.kind === 'playlist') {
      const r = await api.playlistSongs(view.value.id)
      if (mine === request) songs.value = r.songs
    } else if (view.value.kind === 'player') {
      // La lista del reproductor no se busca en el índice: es lo que has ido
      // abriendo desde fuera, en el orden en que lo abriste.
      const r = await api.externalList()
      if (mine === request) songs.value = r.songs
    } else if (['settings', 'inbox', 'chat', 'downloads', 'duplicates'].includes(view.value.kind)) {
      // páginas propias
    } else {
      const p = { q: query.value, sort: sort.value, desc: sortDesc.value, limit: 1000,
                  ...search.filters() }
      if (view.value.kind === 'favorites') p.only_favorites = true
      const r = await api.search(p)
      if (mine === request) songs.value = r.songs
    }
  } catch (e) {
    if (mine === request) notify('No se pudo cargar: ' + errorMessage(e))
  } finally {
    if (mine === request) loading.value = false
  }
}

async function loadStatus() {
  const e = await api.status()
  status.value = e
  stats.value = e.stats
  configured.value = e.configured
  waiting.value = (await api.inbox()).total
}

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
  const id = detail.value?.id
  if (id == null || id < 0) return
  try {
    const song = await api.song(id)
    if (detail.value?.id === id) detail.value = song
    player.patchItem(song)
  } catch {
    /* la canción ya no está: la lista recargada lo dirá */
  }
}

// Los avisos de cambio llegan cada dos segundos como mucho, y una descarga
// de tres canciones dispara varios seguidos: se agrupan y se refresca una vez.
let changeTimer = null
function onCoreChanged() {
  clearTimeout(changeTimer)
  changeTimer = setTimeout(() => refreshAll(), 250)
}

async function select(id, ev = null) {
  const mine = ++request
  const order = songs.value.map((s) => s.id)
  const ctrl = !!(ev && (ev.ctrlKey || ev.metaKey))
  const shift = !!(ev && ev.shiftKey)
  if (shift && anchor != null && order.includes(anchor) && order.includes(id)) {
    const [a, b] = [order.indexOf(anchor), order.indexOf(id)].sort((x, y) => x - y)
    const range = order.slice(a, b + 1)
    const keep = ctrl ? new Set([...selectedIds.value, ...range]) : new Set(range)
    selectedIds.value = order.filter((x) => keep.has(x))
  } else if (ctrl) {
    const keep = new Set(selectedIds.value.length ? selectedIds.value : selected.value != null ? [selected.value] : [])
    if (keep.has(id)) keep.delete(id)
    else keep.add(id)
    selectedIds.value = order.filter((x) => keep.has(x))
    anchor = id
    if (!keep.has(id)) {
      // se ha quitado: la ficha pasa a la última que quede seleccionada
      selected.value = selectedIds.value.at(-1) ?? null
      if (selected.value == null) detail.value = null
      else detail.value = await api.song(selected.value)
      return
    }
  } else {
    selectedIds.value = [id]
    anchor = id
  }
  selected.value = id
  const song = await api.song(id)
  if (mine === request || selected.value === id) detail.value = song
}

/** Las canciones de la selección múltiple, en el orden de la lista. */
const selectedSongs = computed(() => {
  const keep = new Set(selectedIds.value)
  return keep.size > 1 ? songs.value.filter((s) => keep.has(s.id)) : []
})

/**
 * Pone a sonar. La cola pasa a ser la lista que se está viendo, salvo que se
 * diga otra cosa: es lo que se espera al pulsar una canción de una lista.
 */
function play(song, list = null, origin = null) {
  selected.value = song.id
  // Sobre la que ya está puesta, el botón de la fila es pausa/reanudar: antes
  // volvía a empezar la canción, y no había forma de pararla desde la lista.
  if (!list && player.track.value?.id === song.id) return player.toggle()
  player.setQueue(list || songs.value, song.id, origin || { ...view.value, label: title.value })
  api.song(song.id).then((d) => (detail.value = d))
}

// A dónde se puede enviar una canción desde este equipo (Telegram, si está
// instalado). Se mira una vez al arrancar; sin destino, la opción no aparece.
const shareTargets = ref({ telegram: false })

/** Abre Telegram con los archivos de esas canciones listos para enviar. */
async function sendToTelegram(list) {
  const items = [].concat(list)
  const paths = []
  for (const song of items) {
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
const sendSongToTelegram = (song) => sendToTelegram([song])

/** Un repertorio entero a Telegram: todas sus canciones. */
async function sendPlaylistToTelegram(pl) {
  const list = (await api.playlistSongs(pl.id)).songs || []
  if (!list.length) return notify('Esa lista está vacía')
  await sendToTelegram(list)
}

// La ficha como ventana emergente: cuando el panel lateral está oculto (o
// en pantallas estrechas, donde es un cajón), «Ver detalles» la abre aquí.
const detailModal = ref(false)
const sidePanelShown = computed(() => detailsVisible.value && !isCompact.value)
async function showDetailsOf(song) {
  await select(song.id)
  if (isCompact.value && detailsVisible.value) detailsOpen.value = true
  else detailModal.value = true
}

/** Abre el explorador del sistema señalando el archivo de la canción. */
async function revealSong(song) {
  const path = song?.path || (await api.song(song.id).catch(() => null))?.path
  if (!path) return notify('No sé dónde está ese archivo')
  try {
    await tauriApp.revealInFolder(path)
  } catch (e) {
    notify(errorMessage(e))
  }
}

/** Pone a sonar una lista entera desde el principio. */
function playList(list, origin) {
  if (!list.length) return
  play(list[0], list, origin)
}

/** Play sin nada cargado: suena lo que esté seleccionado en la lista. */
function playSelected() {
  const song = songs.value.find((x) => x.id === selected.value) || songs.value[0]
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
    songs.value = []
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
async function setStars(song, n) {
  const c = await api.setStars(song.id, n)
  onUpdated(c)
}
async function toggleFavorite(song) {
  const c = await api.toggleFavorite(song.id, !song.favorite)
  onUpdated(c)
}

/** Difumina la portada, o le quita el difuminado. La imagen no se toca. */
async function toggleBlur(song) {
  const c = await api.setBlur(song.id, !song.blur)
  onUpdated(c)
  notify(c.blur ? 'Portada difuminada' : 'Portada a la vista', 'ok', 3)
}

/** Las listas a las que se puede mandar la canción, más «crear una nueva». */
function playlistTargets(songOrList) {
  const many = Array.isArray(songOrList) ? songOrList : null
  const song = many ? many[0] : songOrList
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
    action: () => (many ? playlistActions.create(many) : playlistActions.create(song))
  })
  return kids
}

/** El menú sobre varias canciones seleccionadas: actúa sobre todas. */
function groupMenu(ev, list) {
  const n = list.length
  const inPlaylist = view.value.kind === 'playlist'
  const items = [
    {
      label: `Reproducir estas ${n}`,
      icon: 'play',
      action: () => play(list[0], list, { ...view.value, label: 'la selección' })
    },
    { separator: true },
    { label: `Añadir ${n} a una lista`, icon: 'list', children: playlistTargets(list) },
    {
      label: `Marcar ${n} como favoritas`,
      icon: 'heart',
      action: async () => {
        for (const s of list) if (!s.favorite) onUpdated(await api.toggleFavorite(s.id, true))
        notify(`${n} favoritas`, 'ok')
      }
    }
  ]
  if (inPlaylist) {
    items.push({
      label: `Quitar ${n} de esta lista`,
      icon: 'close',
      action: async () => {
        for (const s of list) await api.removeFromPlaylist(view.value.id, s.id)
        notify(`${n} quitadas de la lista`, 'ok')
        await Promise.all([load(true), playlistActions.load()])
      }
    })
  }
  items.push({ separator: true })
  if (shareTargets.value.telegram) {
    items.push({ label: `Enviar ${n} por Telegram`, icon: 'send', action: () => sendToTelegram(list) })
  }
  items.push({
    label: `Mandar ${n} a la papelera…`,
    icon: 'trash',
    danger: true,
    action: () => trashSongs(list)
  })
  openMenu(ev, items, `${n} canciones`)
}

function songMenu(ev, song) {
  // Sobre una de las seleccionadas, el menú es el de todas ellas
  if (selectedSongs.value.length > 1 && selectedIds.value.includes(song.id)) {
    return groupMenu(ev, selectedSongs.value)
  }
  selectedIds.value = [song.id]
  anchor = song.id
  const inPlaylist = view.value.kind === 'playlist'
  const isCurrent = player.track.value?.id === song.id
  const items = [
    {
      label: isCurrent ? (player.state.playing ? 'Pausar' : 'Reanudar') : 'Reproducir',
      icon: isCurrent && player.state.playing ? 'pause' : 'play',
      action: () => play(song)
    },
    {
      label: song.favorite ? 'Quitar de favoritos' : 'Marcar como favorito',
      icon: song.favorite ? 'heartFull' : 'heart',
      action: () => toggleFavorite(song)
    },
    { separator: true },
    { label: 'Añadir a una lista', icon: 'list', children: playlistTargets(song) }
  ]
  if (inPlaylist) {
    items.push({
      label: 'Quitar de esta lista',
      icon: 'close',
      action: () => playlistActions.removeSong(song)
    })
  }
  items.push({ separator: true })
  items.push({
    label: 'Buscar letra y portada',
    icon: 'lyrics',
    action: () => enrichSong(song)
  })
  items.push({
    label: song.blur ? 'Ver la portada' : 'Difuminar la portada',
    icon: song.blur ? 'eye' : 'eyeOff',
    action: () => toggleBlur(song)
  })
  items.push({ label: 'Renombrar…', icon: 'pencil', action: () => renameSong(song) })
  items.push({ separator: true })
  // Con el panel lateral a la vista la ficha ya se ve; si no, se ofrece
  if (!sidePanelShown.value) {
    items.push({ label: 'Ver detalles', icon: 'eye', action: () => showDetailsOf(song) })
  }
  items.push({ label: 'Abrir la carpeta', icon: 'folderOpen', action: () => revealSong(song) })
  if (shareTargets.value.telegram) {
    items.push({ label: 'Enviar por Telegram', icon: 'send', action: () => sendSongToTelegram(song) })
  }
  items.push({
    label: 'Mandar a la papelera…',
    icon: 'trash',
    danger: true,
    action: () => trashSong(song)
  })
  select(song.id)
  openMenu(ev, items, song.title || song.file)
}

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
        ? [{ label: 'Enviar por Telegram', icon: 'send', action: () => sendPlaylistToTelegram(pl) }]
        : []),
      { separator: true },
      { label: 'Borrar la lista…', icon: 'trash', danger: true, action: () => deletePlaylist(pl) }
    ],
    pl.name
  )
}

// Qué hacer cuando se suelta una canción arrastrada. Los destinos se declaran
// con `data-drop` allí donde estén, así que aquí solo hay que decidir qué
// significa cada uno.
onDrop(async (target, song) => {
  const name = song.title || song.file
  if (target === 'favorites') {
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

/** Borrar una lista puede dejarte mirando una vista que ya no existe. */
async function deletePlaylist(pl) {
  const id = await playlistActions.remove(pl)
  if (id && view.value.id === id) view.value = { kind: 'all' }
}

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
  onUpdated(await api.edit(song.id, { title }))
  notify('Renombrada', 'ok')
}

async function enrichSong(song) {
  notify('Buscando letra y portada…', 'info', 3)
  try {
    onUpdated((await api.enrich(song.id, { lyrics: true, cover: true, details: false })).song)
    notify('Listo', 'ok')
  } catch (e) {
    notify('No se pudo: ' + errorMessage(e))
  }
}

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
    if (playingId.value === song.id) player.stop()
    notify(`«${r.name}» está en la papelera`, 'ok')
    await refreshAll()
  } catch (e) {
    notify('No se pudo borrar: ' + errorMessage(e))
  }
}

/** Varias a la papelera, con una sola confirmación que dice cuántas. */
async function trashSongs(list) {
  const n = list.length
  const ok = await ask({
    kind: 'confirm',
    title: `Mandar ${n} canciones a la papelera`,
    danger: true,
    message:
      'Los archivos van a la papelera del sistema, así que puedes recuperarlos desde ahí. ' +
      'También salen de la biblioteca.',
    detail: list
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
      if (playingId.value === s.id) player.stop()
      done++
    } catch (e) {
      notify(`No se pudo borrar «${s.title || s.file}»: ${errorMessage(e)}`)
    }
  }
  selectedIds.value = []
  if (done) notify(`${done} en la papelera`, 'ok')
  await refreshAll()
}

// --------------------------------------------------------------- buscador
const searchBox = ref(null)
const resultsEl = ref(null)
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

watch([sort, sortDesc, view], () => load(), { deep: true })
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
function applySort(field, desc) {
  sort.value = field
  sortDesc.value = desc
}
function goToSettings() {
  view.value = { kind: 'settings' }
  detailsOpen.value = false
}

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
  prefs.applyAll()
  await loadStatus()
  await Promise.all([configured.value ? load() : Promise.resolve(), playlistActions.load()])
  offerToBeDefault()

  // El núcleo puede tardar un segundo en levantarse, y arrancar la app
  // abriendo una canción desde el explorador es justo cuando más tarda:
  // todo pasa a la vez. Si se carga antes de que conteste, la biblioteca
  // sale vacía —«Nada por aquí»— y ahí se quedaba, porque nadie escuchaba
  // este aviso. Rust ya lo mandaba desde el principio.
  stopCoreWatch = await core.onStatus((e) => {
    if (e?.ready && !coreWasReady) refreshAll()
    coreWasReady = !!e?.ready
  })

  // Abrir una canción desde el explorador la mete en la lista del
  // reproductor. Si esa lista está delante, tiene que aparecer sola: se
  // cargaba al entrar y se quedaba quieta mientras iban llegando canciones.
  stopExternalWatch = await api.onExternal(() => {
    if (view.value.kind === 'player') load(true)
  })

  // Cualquier cambio en el núcleo —lo haga quien lo haga— se refleja aquí
  // sin salir y volver a entrar. Rust avisa; esta ventana escucha.
  stopChangeWatch = await core.onChanged(onCoreChanged)
  tauriApp.shareTargets().then((t) => (shareTargets.value = t || { telegram: false })).catch(() => {})
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
  stopCoreWatch?.()
  stopExternalWatch?.()
  stopChangeWatch?.()
  clearTimeout(changeTimer)
  cancelDrag()
  window.removeEventListener('contextmenu', blockContextMenu)
  window.removeEventListener('keydown', onEscape)
})

/** Una canción cambió: se refresca en la lista, en la ficha y en la cola. */
function onUpdated(song) {
  if (!song) return
  detail.value = song
  const i = songs.value.findIndex((x) => x.id === song.id)
  if (i >= 0) songs.value[i] = { ...songs.value[i], ...song }
  player.patchItem(song)
}
</script>

<template>
  <div class="app">
    <header class="topbar">
      <button
        v-if="isCompact"
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

      <div class="view-switch">
        <button
          v-for="v in VIEWS"
          :key="v"
          :class="{ on: layout === v }"
          :title="VIEW_NAMES[v]"
          @click="layout = v"
        >
          <Icon :n="VIEW_ICONS[v]" :t="15" />
        </button>
      </div>

      <div ref="viewBox" style="position: relative">
        <button class="btn mini" style="gap: 6px" @click="viewMenu = !viewMenu">
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
                :options="Object.entries(SIZES).map(([k, s]) => ({ v: k, n: s.name, note: s.note }))"
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
          v-if="!configured && !songs.length && view.kind !== 'settings'"
          @ready="refreshAll"
        />

        <ChatPage v-else-if="view.kind === 'chat'" :context="chatContext" @reload="refreshAll" @action="runAction" />

        <DownloadsPage v-else-if="view.kind === 'downloads'" @reload="refreshAll" />

        <SettingsPage
          v-else-if="view.kind === 'settings'"
          @reindexed="refreshAll"
          @changed="refreshAll"
        />

        <InboxPage
          v-else-if="view.kind === 'inbox'"
          :waiting="waiting"
          @changed="refreshAll"
          @go="(v) => (view = v)"
        />

        <DuplicatesPage v-else-if="view.kind === 'duplicates'" @changed="refreshAll" />

        <template v-else>
          <div class="filters">
            <strong style="font-size: 13px">{{ title }}</strong>
            <span class="chip">{{ songs.length }}</span>
            <span
              v-if="groupBy && view.kind !== 'artists'"
              class="chip x"
              @click="groupBy = ''"
            >
              {{ GROUPINGS.find((a) => a.v === groupBy)?.n }} ×</span
            >
            <span v-if="query" class="chip x" @click="query = ''"> «{{ query }}» ×</span>
            <span v-if="shuffle" class="chip on">aleatorio</span>
            <span v-if="repeat !== 'list'" class="chip on">{{ REPEAT_NAMES[repeat] }}</span>
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
            :selected="selected"
            :selected-ids="selectedIds"
            :playing="playingId"
            :sort="sort"
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
          <div v-if="detailModal" class="modal-back" @mousedown.self="detailModal = false">
            <div class="modal details-modal" role="dialog" aria-modal="true" aria-label="Ficha de la canción">
              <button class="btn mini details-modal-close" title="Cerrar" @click="detailModal = false">
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
      <div
        v-if="drag.song"
        class="drag-ghost"
        :style="{ left: drag.x + 'px', top: drag.y + 'px' }"
      >
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

    <StudyBar v-if="studyOpen" @close="studyOpen = false" />
    <Player :study="studyOpen" @go-to-origin="goToOrigin" @play-selected="playSelected"
            @toggle-study="studyOpen = !studyOpen" />

    <ContextMenu
      :open="menu.open"
      :x="menu.x"
      :y="menu.y"
      :items="menu.items"
      :title="menu.title"
      @close="closeMenu"
    />

    <ModalDialog v-bind="dialog" @ok="dialogOk" @cancel="dialogCancel" />
  </div>
</template>
