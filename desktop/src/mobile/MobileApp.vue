<script setup>
/**
 * La app del celular.
 *
 * Tiene su propio armazón, hecho para una pantalla estrecha y para los
 * dedos: una cabecera con el menú de lado (☰), la lista de canciones táctil
 * (MovilSongs), el reproductor del celular (MovilPlayer) y los ajustes. Lo
 * demás es lo mismo que en la PC: el modo estudio (StudyBar), la ficha con
 * la letra y los acordes (DetailsPanel), los menús y los diálogos; sus
 * retoques para el teléfono están en `styles/movil.css`, que solo carga esta
 * app.
 *
 * Las pistas que se trajeron de la PC son la biblioteca. No hay nada de
 * organizar, descargar, separar, renombrar ni del asistente: eso es de la PC.
 * Sí hay listas (el repertorio del domingo) y favoritas.
 */
import { ref, computed, watch, nextTick, onMounted, onUnmounted } from 'vue'
import { api, errorMessage } from '../api.js'
import { usePlayback } from '../composables/usePlayback.js'
import { useClick } from '../composables/useClick.js'
import { useNotices, notify } from '../composables/useNotices.js'
import { useDialog, ask } from '../composables/useDialog.js'
import { useContextMenu, closeMenu, openMenu } from '../composables/useContextMenu.js'
import { usePreferences } from '../composables/usePreferences.js'
import { formatMinutes } from '../utils/format.js'
import { importBundles, importUri } from './importar.js'
import { tapPlays, compactList, keepAwake } from './prefs.js'
import { useMediaSession } from './mediaSession.js'
import Drawer from '../components/ui/Drawer.vue'
import DetailsPanel from '../components/DetailsPanel.vue'
import StudyBar from '../components/StudyBar.vue'
import Icon from '../components/Icon.vue'
import TextField from '../components/ui/TextField.vue'
import ContextMenu from '../components/ui/ContextMenu.vue'
import ModalDialog from '../components/ui/ModalDialog.vue'
import EmptyState from '../components/ui/EmptyState.vue'
import Loading from '../components/ui/Loading.vue'
import MovilSongs from './MovilSongs.vue'
import MovilPlayer from './MovilPlayer.vue'
import MovilMenu from './MovilMenu.vue'
import MovilFolders from './MovilFolders.vue'
import { hasMusicPermission, askMusicPermission, scanPhone } from './musica.js'
import MovilSettings from './MovilSettings.vue'
import MovilAbout from './MovilAbout.vue'

const player = usePlayback()
// el clic sigue a cada canción aunque el estudio esté cerrado (como en la PC)
useClick()
// los mandos de la bandeja de Android y de la pantalla de bloqueo
useMediaSession()
const { notices, dismiss } = useNotices()
const { dialog, dialogOk, dialogCancel } = useDialog()
const { menu } = useContextMenu()
const prefs = usePreferences()

/** Lo que se ve: las pistas, las favoritas, una lista o la música del teléfono. */
const view = ref(
  /** @type {{kind: 'all'|'favorites'|'list'|'phone', id?: number, name?: string}} */ ({
    kind: 'all'
  })
)
const songs = ref(/** @type {any[]} */ ([]))
const lists = ref(/** @type {any[]} */ ([]))
const total = ref(0)
const favorites = ref(0)
/** La música del teléfono: cuántas hay, si falta el permiso, y las carpetas. */
const phoneTotal = ref(0)
const needPermission = ref(false)
const foldersOpen = ref(false)
const query = ref('')
const searchOpen = ref(false)
const loading = ref(false)
const selectedId = ref(/** @type {number|null} */ (null))
const editing = ref(false)

const menuOpen = ref(false)
const settingsOpen = ref(false)
const aboutOpen = ref(false)
// el estudio puesto (montado) y si se ve: ocultarlo no toca lo que suena
const studyActive = ref(false)
const studyOpen = ref(false)
function toggleStudy() {
  if (!studyActive.value) {
    studyActive.value = true
    studyOpen.value = true
  } else studyOpen.value = !studyOpen.value
}
const fullOpen = ref(false)
const detailsOpen = ref(false)
const detail = ref(/** @type {any} */ (null))

const playingId = computed(() => player.track.value?.id ?? null)
const playingSong = computed(
  () => songs.value.find((s) => s.id === playingId.value) || player.track.value
)
const currentList = computed(() =>
  view.value.kind === 'list' ? lists.value.find((l) => l.id === view.value.id) : null
)
const title = computed(() =>
  view.value.kind === 'favorites'
    ? 'Favoritas'
    : view.value.kind === 'list'
      ? currentList.value?.name || view.value.name || 'Lista'
      : view.value.kind === 'phone'
        ? 'Música del teléfono'
        : 'Mis pistas'
)
const subtitle = computed(() => {
  const n = songs.value.length
  const what =
    view.value.kind === 'phone'
      ? n === 1
        ? '1 canción'
        : n + ' canciones'
      : n === 1
        ? '1 pista'
        : n + ' pistas'
  if (view.value.kind !== 'list' || !n) return what
  const secs = songs.value.reduce((a, s) => a + (Number(s.duration) || 0), 0)
  return what + ' · ' + formatMinutes(secs)
})
const origin = computed(() => ({
  kind: view.value.kind,
  id: view.value.id ?? null,
  label: title.value
}))

// ------------------------------------------------------------- cargar
const plain = (s) =>
  String(s || '')
    .toLowerCase()
    .normalize('NFD')
    .replace(/\p{Mn}/gu, '')

async function load() {
  loading.value = true
  try {
    if (view.value.kind === 'phone') {
      needPermission.value = !hasMusicPermission()
      if (needPermission.value) {
        songs.value = []
        return
      }
      let r = await api.phoneSongs(query.value ? { q: query.value } : {})
      // la primera vez: a buscarla
      if (!r.total && !query.value) {
        await scanPhone()
        r = await api.phoneSongs({})
      }
      phoneTotal.value = r.total || 0
      songs.value = r.songs || []
      return
    }
    if (view.value.kind === 'list') {
      const r = await api.playlistSongs(view.value.id)
      const words = plain(query.value).split(/\s+/).filter(Boolean)
      songs.value = (r.songs || []).filter((s) =>
        words.every((w) => plain(`${s.title} ${s.artist} ${s.key}`).includes(w))
      )
    } else {
      const r = await api.search({ q: query.value, limit: '20000' })
      const all = r.songs || []
      if (!query.value) total.value = all.length
      songs.value = view.value.kind === 'favorites' ? all.filter((s) => s.favorite) : all
    }
  } catch (e) {
    notify('No se pudieron leer las pistas: ' + errorMessage(e))
    if (view.value.kind === 'list') go({ kind: 'all' })
  } finally {
    loading.value = false
  }
}
async function loadLists() {
  try {
    const r = await api.playlists()
    lists.value = r.playlists || []
    favorites.value = r.favorites || 0
  } catch {
    /* sin listas: el menú enseña solo las pistas */
  }
}
async function refresh() {
  await Promise.all([load(), loadLists()])
}
let searchTimer = null
watch(query, () => {
  clearTimeout(searchTimer)
  searchTimer = setTimeout(load, 200)
})

// el menú relee las listas y las favoritas al abrirse: que cuente lo de ahora
watch(menuOpen, (open) => {
  if (open) loadLists()
})

function go(v) {
  menuOpen.value = false
  editing.value = false
  selectedId.value = null
  view.value = v
  load()
}

function toggleSearch() {
  if (searchOpen.value) {
    searchOpen.value = false
    query.value = ''
  } else {
    searchOpen.value = true
    nextTick(() => document.querySelector('.m-search input')?.focus())
  }
}

async function importar() {
  menuOpen.value = false
  if (await importBundles()) await refresh()
}

// ----------------------------------------------------------- canciones
/**
 * Lo que va a la cola con esa canción: la lista que se ve; en la música del
 * teléfono, las de su carpeta (como al abrir un archivo en la PC: siguen las
 * de al lado).
 */
async function queueFor(song) {
  if (view.value.kind !== 'phone' && !song.phone) return songs.value
  try {
    const r = await api.phoneFolder(song.id)
    if (r?.songs?.length) return r.songs
  } catch {
    /* la lista que se ve */
  }
  return songs.value
}
async function play(song) {
  if (playingId.value === song.id) return player.toggle()
  player.setQueue(await queueFor(song), song.id, origin.value)
}

// ------------------------------------------------- la música del teléfono
async function givePermission() {
  if (await askMusicPermission()) {
    needPermission.value = false
    await scanPhone().catch(() => null)
    await load()
  } else {
    notify('Sin el permiso, DanPlay no puede ver la música del teléfono', 'info', 5)
  }
}
async function rescan() {
  loading.value = true
  try {
    const r = await scanPhone()
    notify(`${r?.songs ?? 0} canciones en el teléfono`, 'ok', 3)
  } catch (e) {
    notify('No se pudo mirar la música: ' + errorMessage(e))
  }
  await load()
}
async function foldersDone(n) {
  foldersOpen.value = false
  notify(`${n} ${n === 1 ? 'canción' : 'canciones'} en las carpetas elegidas`, 'ok', 3)
  await load()
}

function select(song) {
  selectedId.value = selectedId.value === song.id ? null : song.id
}

// ------------------------------------------------------------ el estudio
const studyBar = ref(null)
/** Lo oculta como su botón «Ocultar»: lo que suena sigue igual. */
function closeStudy() {
  if (studyBar.value?.hide) studyBar.value.hide()
  else studyOpen.value = false
}
// deslizar la cabecera del estudio hacia abajo lo cierra
let pull = null
function studyDown(e) {
  if (!e.target.closest?.('.study-head') || e.target.closest('button')) return
  pull = { y: e.clientY, t: Date.now(), el: e.currentTarget, dy: 0 }
  pull.el.style.transition = 'none'
  pull.el.setPointerCapture?.(e.pointerId)
}
function studyMove(e) {
  if (!pull) return
  pull.dy = Math.max(0, e.clientY - pull.y)
  pull.el.style.transform = pull.dy ? `translateY(${pull.dy}px)` : ''
}
function studyUp() {
  if (!pull) return
  const { el, dy, t } = pull
  pull = null
  const fast = dy / Math.max(1, Date.now() - t) > 0.6
  el.style.transition = 'transform 0.26s var(--ease-out)'
  if (dy > 110 || (fast && dy > 30)) {
    el.style.transform = 'translateY(100%)'
    closeStudy()
    // oculto se queda montado: al volver a verlo, en su sitio
    setTimeout(() => {
      el.style.transform = ''
      el.style.transition = ''
    }, 300)
  } else {
    el.style.transform = ''
    setTimeout(() => (el.style.transition = ''), 300)
  }
}

/** Abre la canción en el estudio (sonando o no). */
async function study(song) {
  if (playingId.value !== song.id) player.setQueue(await queueFor(song), song.id, origin.value)
  fullOpen.value = false
  studyActive.value = true
  studyOpen.value = true
}

async function lyrics(song) {
  if (!song?.id) return
  try {
    detail.value = await api.song(song.id)
    detailsOpen.value = true
  } catch (e) {
    notify('No se pudo abrir la letra: ' + errorMessage(e))
  }
}

/** Una canción cambió (estrellas, favorito): en la lista y en la ficha. */
function onUpdated(song) {
  if (!song?.id) return
  songs.value = songs.value.map((s) => (s.id === song.id ? { ...s, ...song } : s))
  if (detail.value?.id === song.id) detail.value = { ...detail.value, ...song }
  if (player.track.value?.id === song.id) player.patchItem?.(song)
}

async function toggleFavorite(song) {
  const on = !(songs.value.find((s) => s.id === song.id)?.favorite ?? song.favorite)
  try {
    const r = await api.toggleFavorite(song.id, on)
    onUpdated(r && r.id ? r : { ...song, favorite: on })
    notify(on ? 'En favoritas' : 'Fuera de favoritas', 'ok', 2)
    if (view.value.kind === 'favorites' && !on) load()
    loadLists()
  } catch (e) {
    notify('No se pudo marcar: ' + errorMessage(e))
  }
}

async function remove(song) {
  const ok = await ask({
    kind: 'confirm',
    title: 'Quitar del celular',
    danger: true,
    message: 'La pista se borra de este teléfono. En la PC no se toca: se puede volver a traer.',
    detail: song.title || song.file,
    okLabel: 'Quitar'
  })
  if (!ok) return
  try {
    if (playingId.value === song.id) await player.stop()
    await api.deleteSong(song.id)
    notify('Pista quitada del celular', 'ok')
    if (detail.value?.id === song.id) detailsOpen.value = false
    await refresh()
  } catch (e) {
    notify('No se pudo quitar: ' + errorMessage(e))
  }
}

// --------------------------------------------------------------- listas
async function newList(songToAdd = null) {
  menuOpen.value = false
  const name = await ask({
    kind: 'prompt',
    title: 'Nueva lista',
    message: songToAdd
      ? `Se crea con «${songToAdd.title}» dentro.`
      : 'Por ejemplo, la del domingo o la del ensayo.',
    placeholder: 'Domingo 19 de octubre',
    okLabel: 'Crear'
  })
  if (!name) return
  try {
    const made = await api.createPlaylist(name)
    const l = { id: made.id, name: made.name || name }
    if (songToAdd) await api.addToPlaylist(l.id, [songToAdd.id])
    await loadLists()
    notify(`Lista «${l.name}» creada`, 'ok', 3)
    if (!songToAdd) go({ kind: 'list', id: l.id, name: l.name })
  } catch (e) {
    notify('No se pudo crear la lista: ' + errorMessage(e))
  }
}

async function addTo(list, song) {
  try {
    const r = await api.addToPlaylist(list.id, [song.id])
    notify(r?.added ? `Añadida a «${list.name}»` : `Ya estaba en «${list.name}»`, 'ok', 3)
    await loadLists()
  } catch (e) {
    notify('No se pudo añadir: ' + errorMessage(e))
  }
}

async function takeOut(song) {
  const l = currentList.value
  if (!l) return
  try {
    await api.removeFromPlaylist(l.id, song.id)
    songs.value = songs.value.filter((s) => s.id !== song.id)
    await loadLists()
  } catch (e) {
    notify('No se pudo quitar de la lista: ' + errorMessage(e))
  }
}

async function moveSong(from, to) {
  const l = currentList.value
  if (!l || to < 0 || to >= songs.value.length) return
  const next = songs.value.slice()
  const [s] = next.splice(from, 1)
  next.splice(to, 0, s)
  songs.value = next
  try {
    await api.reorderPlaylist(
      l.id,
      next.map((x) => x.id)
    )
  } catch (e) {
    notify('No se pudo ordenar: ' + errorMessage(e))
    load()
  }
}

function playAll(shuffled = false) {
  if (!songs.value.length) return
  const list = songs.value
  const first = shuffled ? list[Math.floor(Math.random() * list.length)] : list[0]
  player.setShuffle(!!shuffled)
  player.setQueue(list, first.id, origin.value)
  fullOpen.value = true
}

async function renameList() {
  const l = currentList.value
  if (!l) return
  const name = await ask({
    kind: 'prompt',
    title: 'Cambiar el nombre',
    value: l.name,
    okLabel: 'Guardar'
  })
  if (!name || name === l.name) return
  try {
    await api.editPlaylist(l.id, { name })
    await loadLists()
  } catch (e) {
    notify('No se pudo cambiar el nombre: ' + errorMessage(e))
  }
}

async function deleteList() {
  const l = currentList.value
  if (!l) return
  const ok = await ask({
    kind: 'confirm',
    title: 'Borrar la lista',
    danger: true,
    message: 'Se borra la lista; las pistas siguen en el celular.',
    detail: l.name,
    okLabel: 'Borrar'
  })
  if (!ok) return
  try {
    await api.deletePlaylist(l.id)
    await loadLists()
    go({ kind: 'all' })
  } catch (e) {
    notify('No se pudo borrar: ' + errorMessage(e))
  }
}

function listMenu(ev) {
  openMenu(
    ev,
    [
      { label: 'Ordenar', icon: 'list', action: () => (editing.value = true) },
      { label: 'Cambiar el nombre…', icon: 'pencil', action: renameList },
      { separator: true },
      { label: 'Borrar la lista…', icon: 'trash', danger: true, action: deleteList }
    ],
    currentList.value?.name || 'Lista'
  )
}

function songMenu(ev, song) {
  const sounding = playingId.value === song.id
  const fav = songs.value.find((s) => s.id === song.id)?.favorite ?? song.favorite
  if (song.phone || view.value.kind === 'phone') {
    // una canción del teléfono: sonar y estudiar (las listas y las favoritas
    // son de las pistas traídas)
    return openMenu(
      ev,
      [
        {
          label: sounding && player.state.playing ? 'Pausar' : 'Reproducir',
          icon: sounding && player.state.playing ? 'pause' : 'play',
          action: () => play(song)
        },
        { label: 'Estudiar', icon: 'academic', action: () => study(song) }
      ],
      song.title || song.file
    )
  }
  openMenu(
    ev,
    [
      {
        label: sounding && player.state.playing ? 'Pausar' : 'Reproducir',
        icon: sounding && player.state.playing ? 'pause' : 'play',
        action: () => play(song)
      },
      { label: 'Estudiar', icon: 'academic', action: () => study(song) },
      { label: 'Letra y acordes', icon: 'lyrics', action: () => lyrics(song) },
      {
        label: 'Añadir a una lista',
        icon: 'plus',
        children: [
          ...lists.value.map((l) => ({
            label: l.name,
            icon: 'list',
            note: String(l.n ?? ''),
            action: () => addTo(l, song)
          })),
          { label: 'Nueva lista…', icon: 'plus', action: () => newList(song) }
        ]
      },
      ...(view.value.kind === 'list'
        ? [{ label: 'Quitar de esta lista', icon: 'close', action: () => takeOut(song) }]
        : []),
      {
        label: fav ? 'Quitar de favoritas' : 'Marcar como favorita',
        icon: fav ? 'heart' : 'heartFull',
        action: () => toggleFavorite(song)
      },
      { separator: true },
      { label: 'Quitar del celular…', icon: 'trash', danger: true, action: () => remove(song) }
    ],
    song.title || song.file
  )
}

/** «Ir a…» desde el reproductor: la lista desde la que se puso a sonar. */
function goToOrigin() {
  const o = player.origin.value
  if (o?.kind === 'list' && o.id != null) go({ kind: 'list', id: o.id, name: o.label })
  else if (o?.kind === 'favorites') go({ kind: 'favorites' })
  else if (o?.kind === 'phone') go({ kind: 'phone' })
  else go({ kind: 'all' })
  const id = playingId.value
  if (id != null) selectedId.value = id
}

function openSettings() {
  menuOpen.value = false
  settingsOpen.value = true
}

function openAbout() {
  menuOpen.value = false
  aboutOpen.value = true
}

function help() {
  menuOpen.value = false
  ask({
    kind: 'alert',
    title: 'Cómo traer pistas de la PC',
    message:
      'En DanPlay de la PC, abre el modo estudio de una canción separada y, en el menú ⋯ de las pistas, elige «Convertir a DanPista (para el celular)». Sale un archivo .danpista en la carpeta «Celular» de tu biblioteca.',
    detail:
      'Pásalo al teléfono como quieras (WhatsApp, Drive, cable) y tócalo: «Abrir con DanPlay». O aquí, en el menú, «Importar pistas».',
    okLabel: 'Entendido'
  })
}

onMounted(() => {
  // el tema y el tamaño guardados (sin esto el celular arrancaba siempre
  // con los de fábrica)
  prefs.applyAll()
  refresh()
})

// El botón «atrás» de Android: cierra lo que haya abierto, de lo de encima a
// lo de debajo. Lo pregunta la app (MainActivity.kt); si aquí no queda nada
// que cerrar, la app pasa a segundo plano y la música sigue, como en
// cualquier reproductor.
window.__danplayAtras = () => {
  if (menu.value.open) closeMenu()
  else if (dialog.value.open) dialogCancel()
  else if (fullOpen.value) fullOpen.value = false
  else if (detailsOpen.value) detailsOpen.value = false
  else if (settingsOpen.value) settingsOpen.value = false
  else if (aboutOpen.value) aboutOpen.value = false
  else if (menuOpen.value) menuOpen.value = false
  else if (studyOpen.value) closeStudy()
  else if (editing.value) editing.value = false
  else if (searchOpen.value) toggleSearch()
  else if (selectedId.value != null) selectedId.value = null
  else if (view.value.kind !== 'all') go({ kind: 'all' })
  else return false
  return true
}
onUnmounted(() => delete window.__danplayAtras)

// Mientras suena, la pantalla no se apaga (si así se quiere en Ajustes): se
// toca encima con el teléfono en el atril.
let wake = null
async function holdScreen(on) {
  try {
    if (on && !wake && navigator.wakeLock) {
      wake = await navigator.wakeLock.request('screen')
      wake.addEventListener?.('release', () => (wake = null))
    } else if (!on && wake) {
      await wake.release()
      wake = null
    }
  } catch {
    /* sin permiso o sin la API: la pantalla hace lo de siempre */
  }
}
watch(
  () => !!player.state.playing && keepAwake.value,
  (on) => holdScreen(on)
)
function onVisible() {
  if (document.visibilityState === 'visible' && player.state.playing && keepAwake.value) {
    holdScreen(true)
  }
}
document.addEventListener('visibilitychange', onVisible)
onUnmounted(() => document.removeEventListener('visibilitychange', onVisible))

// «Abrir con DanPlay» desde WhatsApp o Descargas: Android le pasa el
// archivo a la página por aquí (MainActivity.kt) y se importa como con el botón
window.__danplayAbrir = async (uri) => {
  if (await importUri(uri)) await refresh()
}
onUnmounted(() => delete window.__danplayAbrir)
</script>

<template>
  <div
    class="m-app"
    :class="{ 'm-studying': studyOpen, 'm-compact': compactList, 'm-playing': !!playingId }"
  >
    <header class="m-top">
      <button type="button" class="m-icon" aria-label="Abrir el menú" @click="menuOpen = true">
        <Icon n="menu" :t="24" />
      </button>
      <div class="m-top-title">
        <strong>{{ title }}</strong>
        <span>{{ subtitle }}</span>
      </div>
      <button
        type="button"
        class="m-icon"
        :class="{ on: searchOpen }"
        :aria-label="searchOpen ? 'Cerrar la búsqueda' : 'Buscar'"
        :aria-expanded="searchOpen"
        @click="toggleSearch"
      >
        <Icon :n="searchOpen ? 'close' : 'search'" :t="22" />
      </button>
      <button
        type="button"
        class="m-icon"
        aria-label="Importar pistas (.danpista) hechas en DanPlay de la PC"
        @click="importar"
      >
        <Icon n="download" :t="22" />
      </button>
    </header>

    <transition name="m-expand">
      <div v-if="searchOpen" class="m-search">
        <TextField
          v-model="query"
          icon="search"
          width="100%"
          :placeholder="view.kind === 'list' ? 'Buscar en esta lista…' : 'Título, artista o tono…'"
          aria-label="Buscar en tus pistas"
        />
      </div>
    </transition>

    <main class="m-main">
      <div v-if="view.kind === 'list' && (songs.length || editing)" class="m-list-head">
        <template v-if="editing">
          <span class="m-list-note">Sube, baja o quita canciones</span>
          <button type="button" class="m-chip-btn m-primary" @click="editing = false">
            <Icon n="check" :t="16" /> Listo
          </button>
        </template>
        <template v-else>
          <button type="button" class="m-chip-btn m-primary" @click="playAll(false)">
            <Icon n="play" :t="16" /> Tocar en orden
          </button>
          <button type="button" class="m-chip-btn" @click="playAll(true)">
            <Icon n="shuffle" :t="16" /> Aleatorio
          </button>
          <button
            type="button"
            class="m-icon m-list-more"
            aria-label="Opciones de la lista"
            @click="listMenu"
          >
            <Icon n="moreVertical" :t="20" />
          </button>
        </template>
      </div>

      <div v-if="view.kind === 'phone' && !needPermission" class="m-list-head">
        <button type="button" class="m-chip-btn" @click="foldersOpen = true">
          <Icon n="folderOpen" :t="16" /> Carpetas
        </button>
        <button type="button" class="m-chip-btn" @click="rescan">
          <Icon n="refresh" :t="16" /> Buscar música nueva
        </button>
      </div>

      <div v-if="view.kind === 'phone' && needPermission" class="m-empty">
        <EmptyState
          icon="music"
          title="La música de tu teléfono"
          hint="Para tocar aquí tus canciones (MP3, M4A, FLAC…), Android pide permiso de «Música y audio». DanPlay solo las lee: no sube nada a ningún sitio."
        />
        <button type="button" class="m-chip-btn m-primary" @click="givePermission">
          <Icon n="check" :t="16" /> Dar permiso
        </button>
      </div>
      <Loading v-else-if="loading && !songs.length" text="Leyendo tus pistas…" />
      <EmptyState
        v-else-if="!songs.length && query"
        icon="search"
        title="Nada con eso"
        :hint="'No hay pistas con «' + query + '»'"
      />
      <div v-else-if="!songs.length && view.kind === 'phone'" class="m-empty">
        <EmptyState
          icon="music"
          title="No hay música en las carpetas elegidas"
          hint="Elige las carpetas del teléfono donde guardas tu música."
        />
        <button type="button" class="m-chip-btn m-primary" @click="foldersOpen = true">
          <Icon n="folderOpen" :t="16" /> Elegir carpetas
        </button>
      </div>
      <div v-else-if="!songs.length && view.kind === 'all'" class="m-empty">
        <EmptyState
          icon="mixer"
          title="Todavía no hay pistas en el celular"
          hint="En DanPlay de la PC: el menú de las pistas de una canción › «Convertir a DanPista». Pasa el archivo .danpista al teléfono (WhatsApp, Drive, cable) y tócalo, o tráelo desde aquí."
        />
        <button type="button" class="m-chip-btn m-primary" @click="importar">
          <Icon n="download" :t="16" /> Importar pistas
        </button>
      </div>
      <EmptyState
        v-else-if="!songs.length && view.kind === 'favorites'"
        icon="heart"
        title="Sin favoritas"
        hint="Mantén pulsada una canción (o su ⋮) › «Marcar como favorita»."
      />
      <div v-else-if="!songs.length" class="m-empty">
        <EmptyState
          icon="list"
          title="La lista está vacía"
          hint="Ve a «Mis pistas», mantén pulsada una canción (o toca su ⋮) › «Añadir a una lista»."
        />
        <button type="button" class="m-chip-btn" @click="go({ kind: 'all' })">
          <Icon n="music" :t="16" /> Ir a Mis pistas
        </button>
      </div>
      <MovilSongs
        v-else
        :songs="songs"
        :selected-id="selectedId"
        :playing-id="playingId"
        :playing="!!player.state.playing"
        :tap-plays="tapPlays"
        :editing="editing"
        :label="title"
        @select="select"
        @play="play"
        @study="study"
        @lyrics="lyrics"
        @menu="songMenu"
        @move="moveSong"
        @remove="takeOut"
      />
    </main>

    <Drawer
      :open="menuOpen"
      side="left"
      title=""
      width="min(320px, 86vw)"
      @close="menuOpen = false"
    >
      <MovilMenu
        :view="view"
        :lists="lists"
        :total="total"
        :favorites="favorites"
        :phone="phoneTotal"
        @go="go"
        @new-list="newList()"
        @import="importar"
        @settings="openSettings"
        @help="help"
        @about="openAbout"
      />
    </Drawer>
    <Drawer
      :open="foldersOpen"
      side="right"
      title="Carpetas con música"
      width="min(420px, 94vw)"
      @close="foldersOpen = false"
    >
      <MovilFolders v-if="foldersOpen" @done="foldersDone" />
    </Drawer>
    <Drawer
      :open="settingsOpen"
      side="left"
      title="Ajustes"
      width="min(420px, 100vw)"
      @close="settingsOpen = false"
    >
      <MovilSettings :total="total" />
    </Drawer>
    <Drawer
      :open="aboutOpen"
      side="left"
      title="Acerca de DanPlay"
      width="min(460px, 100vw)"
      @close="aboutOpen = false"
    >
      <MovilAbout v-if="aboutOpen" />
    </Drawer>
    <Drawer
      :open="detailsOpen"
      side="right"
      title="Letra y acordes"
      width="100vw"
      @close="detailsOpen = false"
    >
      <DetailsPanel :song="detail" @updated="onUpdated" />
    </Drawer>

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

    <transition name="study">
      <StudyBar
        v-if="studyActive"
        v-show="studyOpen"
        ref="studyBar"
        :visible="studyOpen"
        @hide="studyOpen = false"
        @exit="
          () => {
            studyOpen = false
            studyActive = false
          }
        "
        @pointerdown="studyDown"
        @pointermove="studyMove"
        @pointerup="studyUp"
        @pointercancel="studyUp"
      />
    </transition>
    <MovilPlayer
      v-model:full="fullOpen"
      :study="studyOpen"
      :song="playingSong"
      :study-active="studyActive"
      @toggle-study="toggleStudy"
      @lyrics="lyrics(playingSong)"
      @favorite="toggleFavorite"
      @go-to-origin="goToOrigin"
    />
    <transition name="m-fade">
      <div v-if="menu.open" class="m-scrim" aria-hidden="true"></div>
    </transition>
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
