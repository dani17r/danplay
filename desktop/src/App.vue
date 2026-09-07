<script setup>
import { ref, onMounted, onUnmounted, watch, computed, nextTick } from 'vue'
import { api, native, tray, pickFolder } from './api.js'
import { onClickOutside } from './composables/useClickOutside.js'
import { useHasScroll, useIsOffscreen } from './composables/useHasScroll.js'
import { useViewport } from './composables/useViewport.js'
import Drawer from './components/ui/Drawer.vue'
import CoverArt from './components/ui/CoverArt.vue'
import { useDragSong, onDrop } from './composables/useDragSong.js'
import { DENSITIES, SIZES, KIND_LABEL, allThemes, applyTheme, applyDensity,
         applySize, savedTheme, savedDensity, savedSize } from './themes.js'
import Sidebar from './components/Sidebar.vue'
import SongTable from './components/SongTable.vue'
import SongGrid from './components/SongGrid.vue'
import GroupedSongs from './components/GroupedSongs.vue'
import DetailsPanel from './components/DetailsPanel.vue'
import Player from './components/Player.vue'
import SettingsPage from './components/SettingsPage.vue'
import Icon from './components/Icon.vue'
import DuplicateGroup from './components/DuplicateGroup.vue'
import ChatPage from './components/ChatPage.vue'
import DownloadsPage from './components/DownloadsPage.vue'
import TextField from './components/ui/TextField.vue'
import SelectField from './components/ui/SelectField.vue'
import ToggleField from './components/ui/ToggleField.vue'
import SliderField from './components/ui/SliderField.vue'
import ContextMenu from './components/ui/ContextMenu.vue'
import Loading from './components/ui/Loading.vue'
import EmptyState from './components/ui/EmptyState.vue'
import ModalDialog from './components/ui/ModalDialog.vue'
import Card from './components/ui/Card.vue'

const view = ref({ kind: 'all' })
const query = ref('')
const sort = ref('artist')
const songs = ref([])
const playlists = ref([])
const stats = ref(null)
const status = ref(null)   // /api/status entero: hace falta el flag de IA
const entrada = ref(0)
const selected = ref(null)
const detail = ref(null)
const playing = ref(null)
const queue = ref([])
const queueOrigin = ref(null)
const jumpToSong = ref(null)
// Sube en cada orden de reproducir. Sin esto, volver a pulsar la cancion que ya
// suena no dispara nada: su id no cambia y el reproductor no se entera.
const pulse = ref(0)
const resolving = ref(false)

/** Reproduce una de las copias sin salir de la pagina de duplicados. */
async function playCopy (theme) {
  if (!theme.id) return
  const c = await api.song(theme.id)
  queue.value = [c]
  queueOrigin.value = { ...view.value, label: 'Duplicados' }
  playing.value = c.id; detail.value = c; pulse.value++
}

/** Conserva una copia y borra el resto del grupo. */
async function keepOne (group, elegida) {
  const otras = group.items.filter(t => t.path !== elegida.path).map(t => t.path)
  const ok = await ask({
    kind: 'confirm', title: 'Conservar solo esta', danger: true,
    message: `Se queda:\n  ${elegida.file}\n\nY se borra${otras.length > 1 ? 'n' : ''}:`,
    detail: group.items.filter(t => t.path !== elegida.path)
      .map(t => t.file).join('\n'),
    okLabel: 'Conservar solo esta'
  })
  if (!ok) return
  resolving.value = true
  try {
    const r = await api.resolveDuplicate(elegida.path, otras)
    if (r.ok) {
      notify(r.renamed ? `Guardada como «${r.final_name}»`
                          : `Conservada «${r.final_name}»`, 'ok')
      if (playing.value && otras.some(o => o.endsWith(elegida.file))) playing.value = null
      duplicates.value = await api.duplicates()
      await refreshAll()
    } else {
      notify('No se pudo: ' + (r.reason || (r.failures || []).map(f => f.reason).join(', ')))
    }
  } finally { resolving.value = false }
}
const loading = ref(false)
const duplicates = ref(null)
const importResult = ref(null)
const showDetails = ref(true)
const configured = ref(true)
const initialPath = ref('')
const preparing = ref(false)
const folderNotice = ref(null)
const notices = ref([])
let noticeCounter = 0
function notify (message, kind = 'info', seconds = 6) {
  const id = ++noticeCounter
  notices.value = [...notices.value, { id, message, kind }]
  setTimeout(() => { notices.value = notices.value.filter(a => a.id !== id) }, seconds * 1000)
}

// presentacion
const theme = ref(savedTheme())
const density = ref(savedDensity())
const appSize = ref(savedSize())
// Se comprueba lo que hay guardado en vez de fiarse: la disposicion movible
// que hubo antes escribia en esta misma clave, y quien la usara tiene ahi un
// objeto que no es ningun formato de vista.
const VIEWS = ['list', 'compact', 'grid']
const savedView = localStorage.getItem('danplay.layout')
const layout = ref(VIEWS.includes(savedView) ? savedView : 'list')
const groupBy = ref(localStorage.getItem('danplay.groupBy') || '')
const cardSize = ref(Number(localStorage.getItem('danplay.cardSize') || 164))
const viewMenu = ref(false)
const viewBox = ref(null)

// En estrecho los laterales no caben: pasan a abrirse encima del contenido.
// La decision vive aqui y no repartida en media queries porque cambia el
// COMPORTAMIENTO, no solo el aspecto.
const { isPhone, isCompact } = useViewport()

// La cancion que se lleva en la mano, para pintar el fantasma que sigue al
// puntero. Quien la recibe se declara con `data-drop`; ver useDragSong.
const { drag, cancelDrag } = useDragSong()
const navOpen = ref(false)
const detailsOpen = ref(false)
// Paginas que no tienen ninguna cancion que seleccionar: ahi el panel de
// detalle solo ocupa 340px para decir «selecciona una cancion». En Duplicados
// SI se queda, porque desde ahi se pueden escuchar las copias y la ficha se
// llena.
const SIN_DETALLE = ['settings', 'chat', 'downloads', 'inbox']
const detailsVisible = computed(() =>
  showDetails.value && !SIN_DETALLE.includes(view.value.kind))
// al navegar se cierra el panel: en movil tapa toda la pantalla
watch(view, () => { navOpen.value = false; detailsOpen.value = false })
watch(isCompact, (v) => { if (!v) { navOpen.value = false; detailsOpen.value = false } })
// tocar una cancion en movil abre su ficha: es lo que se espera
watch(detail, (c) => { if (c && isPhone.value && showDetails.value) detailsOpen.value = true })
// Atajo para volver a lo que suena. Solo aparece si la lista es larga: con
// pocas canciones se ve todo y el boton solo estorbaria.
const centerEl = ref(null)
const scrollBox = () => centerEl.value?.querySelector('.table-wrap, .grid')
const { hasScroll, recheck: recheckScroll } = useHasScroll(scrollBox)
const { offscreen, recheck: recheckVisible } = useIsOffscreen(
  scrollBox, () => centerEl.value?.querySelector('.playing'))
const canJumpToPlaying = computed(() =>
  !!playing.value && hasScroll.value && offscreen.value &&
  !['settings', 'chat', 'downloads', 'inbox', 'duplicates'].includes(view.value.kind))
const viewPanel = ref(null)

// reproduccion
const shuffle = ref(false)
// Cuatro modos, en el orden en que los cicla el boton. Por defecto se repite
// la lista sin fin, que es lo que espera casi todo el mundo al poner musica.
const REPEAT_MODES = ['list', 'one', 'once', 'queue']
const REPEAT_NAMES = {
  list: 'repetir la lista', one: 'repetir esta cancion',
  once: 'solo esta cancion', queue: 'la lista una vez'
}
const repeat = ref(REPEAT_MODES.includes(localStorage.getItem('danplay.repeat'))
  ? localStorage.getItem('danplay.repeat') : 'list')
watch(repeat, v => localStorage.setItem('danplay.repeat', v))

const GROUPINGS = [
  { v: '', n: 'Sin agrupar' }, { v: 'folder', n: 'Por carpeta' },
  { v: 'artist', n: 'Por artista' }, { v: 'album', n: 'Por album' },
  { v: 'initial', n: 'Por inicial' }, { v: 'genre', n: 'Por genero' },
  { v: 'key', n: 'Por tono' }
]

// La agrupacion que se aplica de verdad: "Artistas" siempre agrupa by artist,
// pero eso no cambia la preferencia del usuario para las demas vistas.
onClickOutside(viewBox, () => { viewMenu.value = false })

const effectiveGroupBy = computed(() =>
  view.value.kind === 'artists' ? (groupBy.value || 'artist') : groupBy.value)

const title = computed(() => ({
  all: 'Todas las canciones', favorites: 'Favoritos', artists: 'Artistas',
  inbox: 'Entrada', settings: 'Ajustes', duplicates: 'Duplicados', chat: 'Asistente',
  downloads: 'Descargas',
  playlist: view.value.name
}[view.value.kind] || ''))

const columns = computed(() => layout.value === 'compact'
  ? { stars: false, album: false, key: true, bpm: true, kbps: false }
  : null)

watch(layout, v => localStorage.setItem('danplay.layout', v))
watch(groupBy, v => localStorage.setItem('danplay.groupBy', v))
watch(cardSize, v => localStorage.setItem('danplay.cardSize', v))
watch(density, v => applyDensity(v))
watch(appSize, v => applySize(v))
watch(theme, v => applyTheme(v))

async function load () {
  loading.value = true
  try {
    // el estado se relee siempre: si no, `configured` se quedaba congelado
    // en false y toda la vista central seguia mostrando la bienvenida
    const e = await api.status()
    stats.value = e.stats
    configured.value = e.configured

    if (view.value.kind === 'playlist') {
      songs.value = (await api.playlistSongs(view.value.id)).songs
    } else if (view.value.kind === 'duplicates') {
      duplicates.value = await api.duplicates()
    } else if (['settings', 'inbox', 'chat', 'downloads'].includes(view.value.kind)) {
      // paginas propias
    } else {
      const p = { q: query.value, sort: sort.value, limit: 1000 }
      if (view.value.kind === 'favorites') p.only_favorites = true
      songs.value = (await api.search(p)).songs
    }
  } finally { loading.value = false }
}

async function loadPlaylists () { playlists.value = (await api.playlists()).playlists }
async function loadStatus () {
  const e = await api.status()
  status.value = e
  stats.value = e.stats
  configured.value = e.configured
  entrada.value = (await api.inbox()).total
}

/** Recarga estado y lista. Lo usan Ajustes y la bienvenida. */
async function refreshAll () {
  await Promise.all([loadStatus(), load(), loadPlaylists()])
}

async function browse () {
  const r = await pickFolder()
  if (r) { initialPath.value = r; await setFirstFolder() }
}

async function setFirstFolder (force = false) {
  const path = initialPath.value.trim()
  if (!path) return
  preparing.value = true
  folderNotice.value = null
  try {
    const r = await api.addFolder(path, '', force)
    if (r.action === 'confirm') {
      folderNotice.value = { ...r.notice, confirmable: true }
      return
    }
    if (r.action === 'already_there') notify(r.notice.message + ' — no hace falta añadirla otra vez')
    if (r.action === 'replaced') notify(r.notice.message + ' — se sustituyo por esta')
    if (r.action === 'added') notify('Carpeta añadida. Analizando…', 'ok', 4)
    await api.scan()
    await refreshAll()
    initialPath.value = ''
  } catch (e) {
    folderNotice.value = { message: 'no existe esa carpeta: ' + path, confirmable: false }
  } finally { preparing.value = false }
}

async function select (id) { selected.value = id; detail.value = await api.song(id) }
function play (c, nuevaCola = true) {
  if (nuevaCola) {
    queue.value = songs.value.slice()
    // se recuerda desde donde se puso a sonar, para poder volver
    queueOrigin.value = { ...view.value, label: title.value }
  }
  playing.value = c.id; selected.value = c.id
  pulse.value++
  api.song(c.id).then(d => (detail.value = d))
}

/** Pone a sonar una lista entera desde el principio. */
function playList (list, origin) {
  if (!list.length) return
  queue.value = list.slice()
  queueOrigin.value = origin
  play(list[0], false)          // false: la cola ya la acabamos de poner
}

/**
 * Ordenes que llegan del asistente. El audio lo maneja Rust y la cola vive
 * aqui, asi que el nucleo no puede reproducir por su cuenta: manda la orden
 * y se ejecuta aqui.
 */
async function runAction (a) {
  if (!a?.kind) return
  if (a.kind === 'play_song') {
    const c = songs.value.find(x => x.id === a.song_id) || await api.song(a.song_id)
    if (c) play(c)
    return
  }
  if (a.kind === 'play_playlist') {
    const pl = playlists.value.find(l => l.id === a.playlist_id)
    const list = (await api.playlistSongs(a.playlist_id)).songs || []
    if (!list.length) return notify('Esa lista esta vacia')
    playList(list, { kind: 'playlist', id: a.playlist_id,
                     name: pl?.name, label: pl?.name || 'la lista' })
    return
  }
  if (a.kind === 'player') {
    if (a.command === 'next') return step(1)
    if (a.command === 'previous') return step(-1)
    if (!native.available) return
    if (a.command === 'stop') { await native.stop(); playing.value = null; return }
    if (a.command === 'toggle') return void native.togglePlay()
    // pause y resume son explicitas: hay que mirar como esta para no invertirlo
    const st = await native.status()
    if (a.command === 'pause' && st.playing) await native.togglePlay()
    if (a.command === 'resume' && !st.playing) await native.togglePlay()
  }
}

/** Vuelve a la lista desde la que salio lo que suena y salta a la cancion. */
async function goToOrigin () {
  if (!queueOrigin.value) return
  const { label, ...target } = queueOrigin.value
  if (JSON.stringify(target) !== JSON.stringify(view.value)) {
    view.value = target
    await nextTick()
  }
  selected.value = playing.value
  jumpToSong.value = playing.value
  setTimeout(() => (jumpToSong.value = null), 1200)
}
function step (delta) {
  const list = queue.value.length ? queue.value : songs.value
  if (!list.length) return
  if (shuffle.value && delta > 0) {
    const others = list.filter(c => c.id !== playing.value)
    if (others.length) return play(others[Math.floor(Math.random() * others.length)], false)
  }
  const i = list.findIndex(c => c.id === playing.value)
  let j = i + delta
  // a mano siempre se mueve: dar la vuelta es mas util que no responder
  if (j >= list.length) j = 0
  if (j < 0) j = list.length - 1
  play(list[j], false)
}
function cycleRepeat () {
  repeat.value = REPEAT_MODES[(REPEAT_MODES.indexOf(repeat.value) + 1) % REPEAT_MODES.length]
}

/**
 * Que hacer cuando una cancion se acaba sola.
 *
 * Se decide aqui y no en el reproductor porque aqui estan la cola y el modo.
 * Pulsar «siguiente» a mano siempre avanza: esto solo gobierna el automatico.
 */
function onTrackEnded () {
  const current = nowPlaying.value
  if (!current) return
  if (repeat.value === 'once') return           // se para y no avanza
  if (repeat.value === 'one') return play(current, false)   // otra vez la misma
  const list = queue.value.length ? queue.value : songs.value
  if (!list.length) return
  if (shuffle.value) return step(1)
  const i = list.findIndex(c => c.id === current.id)
  const isLast = i < 0 || i >= list.length - 1
  if (isLast && repeat.value === 'queue') return             // fin de la cola
  play(list[isLast ? 0 : i + 1], false)
}

/** Play sin nada cargado: suena lo que este seleccionado en la lista. */
function playSelected () {
  const c = songs.value.find(x => x.id === selected.value) || songs.value[0]
  if (c) play(c)
}
const nowPlaying = computed(() =>
  (queue.value.concat(songs.value)).find(c => c.id === playing.value) || null)

// ------------------------------------------------------ bandeja del sistema
// El menu de la bandeja y la ventanita viven fuera de esta ventana y no ven
// nada de lo que pasa aqui, asi que hay que contarselo. Lo que importa es que
// puedan poner en gris lo que no se puede hacer: sin cancion no hay que
// pausar, y con una sola en la lista no hay anterior ni siguiente.
const nativePlaying = ref(false)
const trayState = computed(() => {
  const c = nowPlaying.value
  const list = queue.value.length ? queue.value : songs.value
  const neighbours = list.length > 1
  return {
    id: c ? c.id : null,
    title: c ? (c.title || c.file || '') : '',
    artist: c ? (c.artist || '') : '',
    playing: nativePlaying.value,
    blur: !!(c && c.blur),
    has_previous: !!c && neighbours,
    has_next: !!c && neighbours
  }
})
watch(trayState, v => { tray.setNowPlaying(v) }, { deep: true, immediate: true })

/** Ordenes que llegan de la bandeja o de la ventanita. */
function trayCommand ({ action } = {}) {
  if (action === 'next') step(1)
  else if (action === 'previous') step(-1)
}

async function setStars (c, n) { Object.assign(c, await api.setStars(c.id, n)) }
async function toggleFavorite (c) { Object.assign(c, await api.toggleFavorite(c.id, !c.favorite)) }

/** Difumina la portada, o le quita el difuminado. La imagen no se toca. */
async function toggleBlur (song) {
  const c = await api.setBlur(song.id, !song.blur)
  Object.assign(song, c)
  onUpdated(c)
  notify(c.blur ? 'Portada difuminada' : 'Portada a la vista', 'ok', 3)
}

// ---------------------------------------------------------------- dialogos
// Los `prompt()` y `confirm()` del navegador ignoran el tema y se ven de otra
// epoca. `ask()` devuelve una promesa con lo que respondio el usuario.
const dialog = ref({ open: false })
let resolveDialog = null
function ask (opts) {
  return new Promise(res => {
    resolveDialog = res
    dialog.value = { ...opts, open: true }
  })
}
function dialogOk (v) { dialog.value = { open: false }; resolveDialog?.(v); resolveDialog = null }
function dialogCancel () { dialog.value = { open: false }; resolveDialog?.(null); resolveDialog = null }

// ------------------------------------------------------------ menu contextual
const menu = ref({ open: false, x: 0, y: 0, items: [], title: '' })
function openMenu (ev, items, title = '') {
  menu.value = { open: true, x: ev.clientX, y: ev.clientY, items, title }
}
const closeMenu = () => { menu.value = { ...menu.value, open: false } }

/** Las listas a las que se puede mandar la cancion, mas «crear una nueva». */
function playlistTargets (song) {
  const kids = playlists.value.map(l => ({
    label: l.name, icon: 'list', note: String(l.n ?? ''),
    action: () => addToPlaylist(song, l)
  }))
  if (kids.length) kids.push({ separator: true })
  kids.push({ label: 'Nueva lista…', icon: 'plus', action: () => newPlaylist(song) })
  return kids
}

function songMenu (ev, song) {
  const inPlaylist = view.value.kind === 'playlist'
  const items = [
    { label: 'Reproducir', icon: 'play', action: () => play(song) },
    { label: song.favorite ? 'Quitar de favoritos' : 'Marcar como favorito',
      icon: song.favorite ? 'heartFull' : 'heart', action: () => toggleFavorite(song) },
    { separator: true },
    { label: 'Añadir a una lista', icon: 'list', children: playlistTargets(song) },
  ]
  if (inPlaylist) {
    items.push({ label: 'Quitar de esta lista', icon: 'close',
                 action: () => removeFromPlaylist(song) })
  }
  items.push({ separator: true })
  items.push({ label: 'Buscar letra y portada', icon: 'lyrics',
               action: () => enrichSong(song) })
  items.push({ label: song.blur ? 'Ver la portada' : 'Difuminar la portada',
               icon: song.blur ? 'eye' : 'eyeOff',
               action: () => toggleBlur(song) })
  items.push({ label: 'Renombrar…', icon: 'pencil', action: () => renameSong(song) })
  items.push({ separator: true })
  items.push({ label: 'Mandar a la papelera…', icon: 'trash', danger: true,
               action: () => trashSong(song) })
  select(song.id)
  openMenu(ev, items, song.title || song.file)
}

function playlistMenu (ev, pl) {
  openMenu(ev, [
    { label: 'Abrir', icon: 'list',
      action: () => { view.value = { kind: 'playlist', id: pl.id, name: pl.name } } },
    { label: 'Exportar a .m3u', icon: 'download', action: () => exportPlaylist(pl) },
    { separator: true },
    { label: 'Borrar la lista…', icon: 'trash', danger: true,
      action: () => deletePlaylist(pl) }
  ], pl.name)
}

// ------------------------------------------------------------------ acciones
async function addToPlaylist (song, pl) {
  const r = await api.addToPlaylist(pl.id, [song.id])
  notify(r.added ? `Añadida a «${pl.name}»` : `Ya estaba en «${pl.name}»`,
         r.added ? 'ok' : 'info')
  loadPlaylists()
}

// Que hacer cuando se suelta una cancion arrastrada. Los destinos se declaran
// con `data-drop` alli donde esten, asi que aqui solo hay que decidir que
// significa cada uno.
onDrop(async (target, song) => {
  const name = song.title || song.file
  if (target === 'favorites') {
    if (song.favorite) return notify(`«${name}» ya estaba en favoritos`)
    await toggleFavorite(song)
    notify(`«${name}» a favoritos`, 'ok')
  } else if (target === 'new-playlist') {
    newPlaylist(song)
  } else if (target.startsWith('playlist:')) {
    const pl = playlists.value.find(l => String(l.id) === target.slice(9))
    if (pl) addToPlaylist(song, pl)
  }
})

async function newPlaylist (song = null) {
  const name = await ask({
    kind: 'prompt', title: 'Nueva lista',
    message: song ? `Se creara la lista y se añadira «${song.title || song.file}».`
                  : 'Como se va a llamar el repertorio.',
    placeholder: 'Domingo por la mañana', okLabel: 'Crear'
  })
  if (!name) return
  const r = await api.createPlaylist(name)
  await loadPlaylists()
  if (song && r?.id) await api.addToPlaylist(r.id, [song.id])
  notify(song ? `Lista «${name}» creada con esa cancion` : `Lista «${name}» creada`, 'ok')
  if (song) loadPlaylists()
}

async function removeFromPlaylist (song) {
  await api.removeFromPlaylist(view.value.id, song.id)
  notify('Quitada de la lista', 'ok')
  await Promise.all([load(), loadPlaylists()])
}

async function exportPlaylist (pl) {
  try {
    const r = await api.exportPlaylist(pl.id)
    notify(`Exportada a ${r.path || r.file || 'la biblioteca'}`, 'ok')
  } catch (e) { notify('No se pudo exportar: ' + e) }
}

async function deletePlaylist (pl) {
  const id = typeof pl === 'object' ? pl.id : pl
  const name = typeof pl === 'object' ? pl.name : ''
  const ok = await ask({
    kind: 'confirm', title: 'Borrar la lista', danger: true,
    message: `Se borrara la lista${name ? ` «${name}»` : ''}.\n` +
             'Las canciones NO se borran: siguen en tu biblioteca.',
    okLabel: 'Borrar la lista'
  })
  if (!ok) return
  await api.deletePlaylist(id); loadPlaylists()
  if (view.value.id === id) view.value = { kind: 'all' }
}

async function renameSong (song) {
  const title = await ask({
    kind: 'prompt', title: 'Renombrar', message: 'Titulo de la cancion.',
    value: song.title || '', placeholder: 'Titulo', okLabel: 'Guardar'
  })
  if (!title || title === song.title) return
  const c = await api.edit(song.id, { title })
  onUpdated(c); Object.assign(song, c)
  notify('Renombrada', 'ok')
}

async function enrichSong (song) {
  notify('Buscando letra y portada…', 'info', 3)
  try {
    onUpdated((await api.enrich(song.id, { lyrics: true, cover: true, details: false })).song)
    notify('Listo', 'ok')
  } catch (e) { notify('No se pudo: ' + e) }
}

async function trashSong (song) {
  const ok = await ask({
    kind: 'confirm', title: 'Mandar a la papelera', danger: true,
    message: 'El archivo va a la papelera del sistema, asi que puedes recuperarlo ' +
             'desde ahi. Tambien sale de la biblioteca.',
    detail: song.path || song.file, okLabel: 'A la papelera'
  })
  if (!ok) return
  try {
    const r = await api.deleteSong(song.id)
    if (playing.value === song.id) playing.value = null
    notify(`«${r.name}» esta en la papelera`, 'ok')
    await refreshAll()
  } catch (e) { notify('No se pudo borrar: ' + String(e).replace(/^Error:\s*/, '')) }
}
async function runImport (dry_run) {
  loading.value = true
  try {
    importResult.value = (await api.runImport({ dry_run })).results
    await Promise.all([loadStatus(), load()])
  } finally { loading.value = false }
}

// Escribir en el buscador NO dispara una busqueda por tecla. Cada una
// suponia una consulta al nucleo y repintar la lista entera: escribir «barak»
// eran cinco. Se espera a que pares un momento. El orden y el cambio de vista
// si son inmediatos: ahi no hay nada que esperar.
const SEARCH_DELAY = 180
let searchTimer = null
watch(query, () => {
  clearTimeout(searchTimer)
  searchTimer = setTimeout(load, SEARCH_DELAY)
})
watch([sort, view], load, { deep: true })
// Sin `deep`: solo interesa cuando cambia la LISTA (otra busqueda, otra vista,
// otra agrupacion), que es lo que altera el alto del scroll. Vigilarla en
// profundidad obligaba a recorrer las mil canciones y todos sus campos cada
// vez que se tocaba una sola —poner una estrella, por ejemplo— para acabar
// midiendo un scroll que no habia cambiado.
watch([songs, layout, groupBy, view], () => { recheckScroll(); recheckVisible() })
watch(playing, recheckVisible)

onMounted(async () => {
  // El WebView trae su propio menu de clic derecho (recargar, inspeccionar).
  // En una app de escritorio no pinta nada: se bloquea y cada sitio pone el
  // suyo con las opciones que tengan sentido ahi.
  window.addEventListener('contextmenu', e => e.preventDefault())
  window.addEventListener('keydown', (e) => {
    if (e.key === 'Escape' && drag.song) cancelDrag()
  })
  stopTray = await tray.onCommand(trayCommand)
  applyTheme(theme.value); applyDensity(density.value); applySize(appSize.value)
  await loadStatus()
  await Promise.all([configured.value ? load() : Promise.resolve(), loadPlaylists()])
})

// si la app se va con algo en la mano, que no queden escuchas sueltas
let stopTray = null
onUnmounted(() => {
  cancelDrag()
  clearTimeout(searchTimer)
  if (typeof stopTray === 'function') stopTray()
})

function onUpdated (c) {
  detail.value = c
  const i = songs.value.findIndex(x => x.id === c.id)
  if (i >= 0) songs.value[i] = { ...songs.value[i], ...c }
}
</script>

<template>
<div class="app">
  <header class="topbar">
    <button v-if="isCompact" class="icon-btn nav-toggle" title="Menu"
            @click="navOpen = true"><Icon n="viewList" :t="17" /></button>
    <div class="brand"><span class="brand-dot"></span> DANPLAY</div>
    <TextField v-model="query" icon="search" width="min(620px, 42vw)"
           placeholder="Buscar…  artista:barak  tono:Bb  bpm>100  duracion>300" />

    <div class="view-switch">
      <button :class="{on: layout==='list'}" title="Vista de lista" @click="layout='list'"><Icon n="viewList" :t="15" /></button>
      <button :class="{on: layout==='compact'}" title="Vista compacta" @click="layout='compact'"><Icon n="viewCompact" :t="15" /></button>
      <button :class="{on: layout==='grid'}" title="Vista de cuadricula" @click="layout='grid'"><Icon n="viewGrid" :t="15" /></button>
    </div>

    <div style="position:relative" ref="viewBox">
      <button class="btn mini" @click="viewMenu=!viewMenu" style="gap:6px">
        <Icon n="viewOptions" :t="14" /> Vista</button>
      <transition name="dropdown">
      <div v-if="viewMenu" class="card view-menu" ref="viewPanel">
        <!-- separado en dos: lo que cambia la LISTA que estas viendo y lo que
             cambia la APP entera. Estaba todo revuelto y habia que leerselo
             entero para dar con lo que buscabas. -->
        <div class="menu-block">
          <div class="menu-head">Esta lista</div>
          <SelectField v-model="groupBy" label="Agrupar"
                    :options="GROUPINGS.map(a => ({v: a.v, n: a.n}))" />
          <SelectField v-model="density" label="Densidad de las filas"
                    :options="Object.entries(DENSITIES).map(([k,d]) => ({v: k, n: d.name}))" />
          <SliderField v-if="layout==='grid'" v-model="cardSize"
                      :min="110" :max="280" :step="10"
                      label="Tamaño de ficha" :valueText="cardSize + ' px'" />
          <ToggleField v-model="showDetails" title="Panel de detalle"
                   hint="La columna con la letra y los acordes" />
        </div>

        <div class="menu-block">
          <div class="menu-head">La app</div>
          <SelectField v-model="theme" label="Tema"
                    :options="Object.entries(allThemes()).map(([k,t]) =>
                               ({v: k, n: t.name, note: KIND_LABEL[t.kind] || t.kind,
                                 color: t.v.accent}))" />
          <SelectField v-model="appSize" label="Tamaño de la app"
                    :options="Object.entries(SIZES).map(([k,s]) => ({v: k, n: s.name, note: s.note}))" />
          <button v-if="native.available" class="btn mini mini-open"
                  @click="tray.openMini(); viewMenu = false">
            <Icon n="note" :t="14" /> Mini reproductor</button>
        </div>
      </div>
      </transition>
    </div>
    <span class="syntax-hint" v-if="stats">{{ stats.total }} temas</span>
  </header>

  <div class="main">
    <Sidebar v-if="!isCompact" :view="view" :playlists="playlists" :stats="stats"
             :entrada="entrada" @go="v=>{view=v; viewMenu=false}"
             @newPlaylist="newPlaylist()" @playlistMenu="playlistMenu" />

    <Drawer v-else :open="navOpen" side="left" title="DanPlay" @close="navOpen=false">
      <Sidebar :view="view" :playlists="playlists" :stats="stats" :entrada="entrada"
               @go="v=>{view=v; viewMenu=false}"
               @newPlaylist="newPlaylist()" @playlistMenu="playlistMenu" />
    </Drawer>

    <main class="center" ref="centerEl">
      <template v-if="!configured && !songs.length && view.kind !== 'settings'">
        <div class="page" style="display:flex;align-items:center;justify-content:center">
          <div style="max-width:560px;text-align:center">
            <div style="display:flex;justify-content:center;margin-bottom:14px;color:var(--accent)">
              <Icon n="music" :t="46" /></div>
            <h2 style="font-size:22px">Bienvenido a DanPlay</h2>
            <div class="desc" style="margin-bottom:22px">
              Todavia no hay ninguna carpeta de musica. Elige una o varias y
              DanPlay las analizara: identifica los temas, limpia los nombres y
              los organiza por artista.
            </div>
            <Card style="text-align:left" title="Elegir carpeta de musica" note="Puedes añadir mas carpetas despues, en Ajustes.">
              <div style="display:flex;gap:8px">
                <button class="btn primary" :disabled="preparing" @click="browse" style="gap:7px">
                  <Icon n="folderOpen" :t="15" />
                  {{ preparing ? 'Analizando…' : 'Examinar…' }}</button>
                <TextField v-model="initialPath" width="100%" icon="folder"
                       placeholder="o escribe la ruta:  /home/usuario/Musica"
                       @enter="setFirstFolder()" />
                <button class="btn" :disabled="preparing || !initialPath.trim()"
                        @click="setFirstFolder()">Analizar</button>
              </div>
              <div v-if="folderNotice" class="hint"
                   :style="{color: folderNotice.confirmable ? 'var(--amber)' : 'var(--muted)'}">
                {{ folderNotice.message }}
                <div v-if="folderNotice.confirmable" class="btn-row" style="margin-top:8px">
                  <button class="btn mini" @click="setFirstFolder(true)">Añadir igualmente</button>
                  <button class="btn mini" @click="folderNotice=null">Cancelar</button>
                </div>
              </div>
              <div class="hint" v-if="preparing">
                Leyendo etiquetas y construyendo el indice. Puede tardar un poco
                la primera vez.
              </div>
            </Card>
            <div style="font-size:12px;color:var(--muted2);margin-top:16px">
              Nada se mueve ni se renombra sin que tu lo pidas.
            </div>
          </div>
        </div>
      </template>

      <template v-else-if="view.kind==='chat'">
        <ChatPage @reload="refreshAll" @action="runAction" />
      </template>

      <template v-else-if="view.kind === 'downloads'">
        <DownloadsPage @reload="refreshAll" @notice="(m,t)=>notify(m,t)" />
      </template>

      <template v-else-if="view.kind === 'settings'">
        <SettingsPage :theme="theme" :density="density"
                 @theme="t=>theme=t" @density="d=>density=d"
                 @reindexed="refreshAll" @changed="refreshAll" />
      </template>

      <template v-else-if="view.kind === 'inbox'">
        <div class="page">
          <h2>Entrada</h2>
          <div class="desc">
            Es el buzon de tu biblioteca: una carpeta donde dejas musica suelta
            y DanPlay la ordena por ti. Lo que hay aqui todavia
            <strong>no forma parte de tu biblioteca</strong> ni sale en las
            busquedas hasta que lo importas.
          </div>

          <Card title="Como funciona">
            <ol class="steps">
              <li><strong>Sueltas</strong> los archivos en <code>~/Musica/Entrada</code>,
                o los bajas desde <a class="link" @click="view={kind:'downloads'}">Descargas</a>.</li>
              <li>DanPlay <strong>averigua de quien son</strong>: primero por las
                etiquetas, luego por como suena, y si hace falta pregunta a la IA.</li>
              <li><strong>Limpia el nombre</strong> (fuera «VIDEO OFICIAL», acentos
                y mayusculas sostenidas) y lo deja en <code>Artistas/&lt;Artista&gt;/</code>.</li>
              <li>Lo que no logra identificar va a <code>Revisar/</code>.
                <strong>Nunca se inventa un artista.</strong></li>
            </ol>
          </Card>

          <div class="card">
            <div class="path-row">
              <Icon n="inbox" :t="15" />
              <span class="path">~/Musica/Entrada</span>
              <span class="badge" :class="entrada ? '' : 'ok'">
                {{ entrada ? entrada + ' esperando' : 'vacia' }}</span>
            </div>

            <div v-if="!entrada" class="hint">
              Nada pendiente. Cuando dejes archivos ahi apareceran aqui.
            </div>

            <div class="btn-row" style="margin-top:12px">
              <button class="btn primary" :disabled="loading||!entrada" @click="runImport(false)">
                <Icon n="check" :t="15" />
                Importar {{ entrada }} {{ entrada === 1 ? 'archivo' : 'archivos' }}</button>
              <button class="btn" :disabled="loading||!entrada" @click="runImport(true)">
                Probar sin tocar nada</button>
              <Loading v-if="loading" text="trabajando…" />
            </div>
            <div v-if="entrada" class="hint">
              «Probar sin tocar nada» te enseña donde acabaria cada archivo,
              sin moverlo ni renombrarlo.
            </div>
          </div>

          <Card v-if="importResult" title="Resultado">
            <div class="note">
              {{ importResult.filter(r => r.action === 'moved').length }} archivadas ·
              {{ importResult.filter(r => r.action === 'review').length }} a revisar ·
              {{ importResult.filter(r => r.action === 'dry_run').length }} en prueba
            </div>
            <div v-for="(r,i) in importResult" :key="i" class="import-row">
              <div style="display:flex;gap:8px;align-items:center;flex-wrap:wrap">
                <span class="badge" :class="r.action==='review' ? 'bad' : 'ok'">
                  {{ r.action === 'moved' ? 'archivada'
                   : r.action === 'review' ? 'a revisar'
                   : r.action === 'dry_run' ? 'seria asi' : r.action }}</span>
                <span v-if="r.source" class="mono" style="font-size:11px;color:var(--muted2)"
                      :title="'Como se identifico'">
                  {{ r.source }} {{ r.confidence?.toFixed(2) }}</span>
                <strong>{{ r.artist ? `${r.artist} — ${r.title}` : (r.target || r.source_path) }}</strong>
              </div>
              <div class="dl-path" v-if="r.target">{{ r.target }}</div>
              <div class="hint" v-for="a in r.warnings" :key="a">! {{ a }}</div>
            </div>
          </Card>
        </div>
      </template>

      <template v-else-if="view.kind==='duplicates'">
        <div class="page">
          <h2>Duplicados</h2>
          <div class="desc">Escuchalas y quedate con la que prefieras.
            Al conservar una, las demas se borran y si la elegida llevaba el sufijo
            « - r» se le quita.</div>
          <Loading v-if="!duplicates" text="analizando tu biblioteca…" />
          <template v-else>
            <Card v-if="duplicates.identical.length" :title="`Identicos byte a byte (${duplicates.identical.length})`" note="Son el mismo archivo: puedes quedarte con cualquiera sin escuchar.">
              <DuplicateGroup v-for="(g,i) in duplicates.identical" :key="'i'+i" :group="g"
                              :playing="playing" :busy="resolving"
                              @play="playCopy" @keepOne="keepOne" />
            </Card>
            <Card v-if="duplicates.similar.length" :title="`Misma cancion, archivo distinto (${duplicates.similar.length})`" note="Pueden ser versiones diferentes. Escucha antes de decidir.">
              <DuplicateGroup v-for="(g,i) in duplicates.similar" :key="'p'+i" :group="g"
                              :playing="playing" :busy="resolving"
                              @play="playCopy" @keepOne="keepOne" />
            </Card>
            <div v-if="!duplicates.identical.length && !duplicates.similar.length"
                 class="card">Sin duplicados.</div>
          </template>
        </div>
      </template>

      <template v-else>
        <div class="filters">
          <strong style="font-size:13px">{{ title }}</strong>
          <span class="chip">{{ songs.length }}</span>
          <span v-if="groupBy && view.kind!=='artists'" class="chip x" @click="groupBy=''">
            {{ GROUPINGS.find(a=>a.v===groupBy)?.n }} ×</span>
          <span v-if="query" class="chip x" @click="query=''">
            «{{ query }}» ×</span>
          <span v-if="shuffle" class="chip on">aleatorio</span>
          <span v-if="repeat!=='list'" class="chip on">{{ REPEAT_NAMES[repeat] }}</span>
          <Loading v-if="loading" :text="''" inline style="margin-left:auto" />
        </div>

        <GroupedSongs v-if="effectiveGroupBy" :songs="songs" :by="effectiveGroupBy" :layout="layout"
                  :selected="selected" :playing="playing" :size="cardSize" :jumpTo="jumpToSong"
                  @select="select" @play="play" @context="songMenu"
                  @setStars="setStars" @toggleFavorite="toggleFavorite" />
        <SongGrid v-else-if="layout==='grid'" :songs="songs"
                    :selected="selected" :playing="playing" :size="cardSize"
                    :jumpTo="jumpToSong"
                    @select="select" @play="play" @context="songMenu" />
        <SongTable v-else :songs="songs" :selected="selected" :playing="playing"
               :sort="sort" :columns="columns" :jumpTo="jumpToSong"
               @select="select" @play="play" @context="songMenu"
               @setStars="setStars" @toggleFavorite="toggleFavorite" @sortBy="c=>sort=c" />
      </template>

      <transition name="pop">
        <button v-if="canJumpToPlaying" class="jump-fab" @click="goToOrigin"
                title="Ir a la cancion que suena">
          <Icon n="note" :t="17" />
        </button>
      </transition>
    </main>

    <DetailsPanel v-if="detailsVisible && !isCompact"
                  :song="detail" :aiReady="!!status?.ai"
                  @updated="onUpdated" @notice="(m,k)=>notify(m,k)"
                  @toggle-blur="toggleBlur"
                  @goSettings="view = { kind: 'settings' }" />

    <Drawer v-if="isCompact" :open="detailsOpen && detailsVisible" side="right"
            title="Ficha" width="min(400px, 92vw)" @close="detailsOpen=false">
      <DetailsPanel :song="detail" :aiReady="!!status?.ai"
                    @updated="onUpdated" @notice="(m,k)=>notify(m,k)"
                    @toggle-blur="toggleBlur"
                    @goSettings="view = { kind: 'settings' }; detailsOpen = false" />
    </Drawer>
  </div>

  <transition-group name="toast" tag="div" class="toasts">
    <div v-for="a in notices" :key="a.id" class="toast"
           :class="a.kind === 'ok' ? 'hint-ok' : 'hint-amber'">
      <Icon :n="a.kind === 'ok' ? 'check' : 'warning'" :t="15" />
      <span>{{ a.message }}</span>
      <button class="close" @click="notices = notices.filter(x => x.id !== a.id)">
        <Icon n="close" :t="13" /></button>
    </div>
  </transition-group>

  <teleport to="body">
    <div v-if="drag.song" class="drag-ghost"
         :style="{left: drag.x + 'px', top: drag.y + 'px'}">
      <CoverArt :id="drag.song.id" :blur="!!drag.song.blur" class="ghost-art" :icon-size="13"
                :alt="drag.song.title || drag.song.file" />
      <span class="ghost-name">{{ drag.song.title || drag.song.file }}</span>
    </div>
  </teleport>

  <Player :song="nowPlaying" :queue="queue" :origin="queueOrigin?.label"
               :pulse="pulse"
               :shuffle="shuffle" :repeat="repeat"
               @goToOrigin="goToOrigin"
               @previous="step(-1)" @next="step(1)"
               @toggleShuffle="shuffle=!shuffle"
               @cycleRepeat="cycleRepeat"
               @trackEnded="onTrackEnded" @playSelected="playSelected"
               @playState="v => nativePlaying = v"
               @jumpTo="c => play(c, false)" />

  <ContextMenu :open="menu.open" :x="menu.x" :y="menu.y"
               :items="menu.items" :title="menu.title" @close="closeMenu" />

  <ModalDialog v-bind="dialog" @ok="dialogOk" @cancel="dialogCancel" />
</div>
</template>
