<script setup>
/**
 * La app del celular: el armazón de la de escritorio (App.vue) con solo lo
 * que tiene sentido aquí. Las pistas que se trajeron de la PC son la
 * biblioteca; se estudian con el mismo reproductor, el mismo estudio y la
 * misma ficha (letra y acordes) que en la PC. No hay nada de organizar,
 * descargar, separar, renombrar ni del asistente: eso es de la PC.
 *
 * Las clases y la forma son las de App.vue a propósito: así los estilos de la
 * PC (`styles/`) y sus tramos para pantallas estrechas valen tal cual.
 */
import { ref, computed, watch, onMounted, onUnmounted, useTemplateRef } from 'vue'
import { api, errorMessage } from '../api.js'
import { usePlayback } from '../composables/usePlayback.js'
import { useClick } from '../composables/useClick.js'
import { useNotices, notify } from '../composables/useNotices.js'
import { useDialog, ask } from '../composables/useDialog.js'
import { useContextMenu, closeMenu, openMenu } from '../composables/useContextMenu.js'
import { usePreferences, VIEWS, VIEW_NAMES, VIEW_ICONS } from '../composables/usePreferences.js'
import { useSelection } from '../composables/useSelection.js'
import { useViewport } from '../composables/useViewport.js'
import { onClickOutside } from '../composables/useClickOutside.js'
import { DENSITIES, SIZES, KIND_LABEL, allThemes } from '../themes.js'
import { importBundles } from './importar.js'
import Drawer from '../components/ui/Drawer.vue'
import SongRows from '../components/SongRows.vue'
import SongCards from '../components/SongCards.vue'
import SongGrid from '../components/SongGrid.vue'
import SongTable from '../components/SongTable.vue'
import DetailsPanel from '../components/DetailsPanel.vue'
import Player from '../components/Player.vue'
import CompactPlayer from '../components/CompactPlayer.vue'
import StudyBar from '../components/StudyBar.vue'
import Icon from '../components/Icon.vue'
import TextField from '../components/ui/TextField.vue'
import SelectField from '../components/ui/SelectField.vue'
import SliderField from '../components/ui/SliderField.vue'
import ToggleField from '../components/ui/ToggleField.vue'
import ContextMenu from '../components/ui/ContextMenu.vue'
import ModalDialog from '../components/ui/ModalDialog.vue'
import EmptyState from '../components/ui/EmptyState.vue'
import Loading from '../components/ui/Loading.vue'

const player = usePlayback()
// el clic sigue a cada canción aunque el estudio esté cerrado (como en la PC)
useClick()
const { notices, dismiss } = useNotices()
const { dialog, dialogOk, dialogCancel } = useDialog()
const { menu } = useContextMenu()
const { theme, density, appSize, layout, cardSize, showDetails } = usePreferences()
const { isPhone, isCompact, pocket, width } = useViewport()

const songs = ref(/** @type {any[]} */ ([]))
const query = ref('')
const loading = ref(false)
const studyOpen = ref(false)
const detailsOpen = ref(false)
const viewMenu = ref(false)
const viewBox = useTemplateRef('viewBox')
onClickOutside(viewBox, () => (viewMenu.value = false))

const selection = useSelection({ shown: songs })
const { selected, selectedIds, detail } = selection
const playingId = computed(() => player.track.value?.id ?? null)

/** Las pistas del celular, con lo que se busca. */
async function load() {
  loading.value = true
  try {
    const r = await api.search({ q: query.value, limit: '20000' })
    songs.value = r.songs || []
  } catch (e) {
    notify('No se pudieron leer las pistas: ' + errorMessage(e))
  } finally {
    loading.value = false
  }
}
let searchTimer = null
watch(query, () => {
  clearTimeout(searchTimer)
  searchTimer = setTimeout(load, 200)
})

async function importar() {
  if (await importBundles()) await load()
}

function play(song) {
  if (player.track.value?.id === song.id) return player.toggle()
  player.setQueue(songs.value, song.id, { kind: 'all', label: 'Mis pistas' })
  selection.showDetail(song.id)
}

/** Abre la canción en el estudio (sonando o no). */
function study(song) {
  if (player.track.value?.id !== song.id) {
    player.setQueue(songs.value, song.id, { kind: 'all', label: 'Mis pistas' })
  }
  studyOpen.value = true
}

function select(id, ev) {
  selection.select(id, ev)
}

// tocar una canción en el teléfono abre su ficha, como en la PC estrecha
watch(detail, (c) => {
  if (c && isPhone.value && showDetails.value && !pocket.value) detailsOpen.value = true
})

/** Una canción cambió (estrellas, favorito): en la lista y en la ficha. */
function onUpdated(song) {
  if (!song?.id) return
  songs.value = songs.value.map((s) => (s.id === song.id ? { ...s, ...song } : s))
  if (detail.value?.id === song.id) selection.patchDetail(song)
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
    if (player.track.value?.id === song.id) await player.stop()
    await api.deleteSong(song.id)
    notify('Pista quitada del celular', 'ok')
    if (detail.value?.id === song.id) detailsOpen.value = false
    await load()
  } catch (e) {
    notify('No se pudo quitar: ' + errorMessage(e))
  }
}

function songMenu(ev, song) {
  selection.only(song.id)
  const sounding = player.track.value?.id === song.id
  openMenu(
    ev,
    [
      {
        label: sounding && player.state.playing ? 'Pausar' : 'Reproducir',
        icon: sounding && player.state.playing ? 'pause' : 'play',
        action: () => play(song)
      },
      { label: 'Estudiar', icon: 'academic', action: () => study(song) },
      {
        label: 'Letra y acordes',
        icon: 'lyrics',
        action: async () => {
          await selection.select(song.id)
          detailsOpen.value = true
        }
      },
      { separator: true },
      { label: 'Quitar del celular…', icon: 'trash', action: () => remove(song) }
    ],
    song.title || song.file
  )
}

onMounted(load)
// lo que se trae de otra forma (abrir un .danpista desde WhatsApp) avisa
function onImported() {
  load()
}
window.addEventListener('danplay-imported', onImported)
onUnmounted(() => window.removeEventListener('danplay-imported', onImported))
</script>

<template>
  <div class="app movil" :class="{ pocketed: !!pocket }">
    <header class="topbar">
      <div class="brand"><span class="brand-dot"></span> <span>DANPLAY</span></div>
      <div class="search-box">
        <TextField
          v-model="query"
          icon="search"
          width="100%"
          placeholder="Buscar en tus pistas…"
          aria-label="Buscar en tus pistas"
        />
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
      <button
        type="button"
        class="btn mini"
        style="gap: 6px"
        title="Traer pistas (.danpista) hechas en DanPlay de la PC"
        @click="importar"
      >
        <Icon n="download" :t="14" /> <span class="btn-label">Importar</span>
      </button>
      <div ref="viewBox" style="position: relative">
        <button
          type="button"
          class="btn mini"
          style="gap: 6px"
          :aria-expanded="viewMenu"
          title="Vista"
          @click="viewMenu = !viewMenu"
        >
          <Icon n="viewOptions" :t="14" /> <span class="btn-label">Vista</span>
        </button>
        <transition name="dropdown">
          <div v-if="viewMenu" class="card view-menu">
            <div class="menu-block">
              <div class="menu-head">Esta lista</div>
              <SelectField
                v-model="layout"
                label="Cómo se ve"
                :options="VIEWS.map((v) => ({ v, n: VIEW_NAMES[v] }))"
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
                title="Ficha al tocar"
                hint="La letra y los acordes de la canción que tocas"
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
            </div>
          </div>
        </transition>
      </div>
    </header>
    <div class="main">
      <main class="center">
        <div class="filters">
          <strong style="font-size: 13px">Mis pistas</strong>
          <span class="chip">{{ songs.length }}</span>
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
          <Loading v-if="loading" text="" inline style="margin-left: auto" />
        </div>
        <EmptyState
          v-if="!loading && !songs.length && !query"
          icon="mixer"
          title="Todavía no hay pistas en el celular"
          hint="En DanPlay de la PC: el menú de las pistas de una canción › «Llevar al celular». Pasa el archivo .danpista al teléfono (WhatsApp, Drive, cable) y tócalo aquí con «Importar»."
        />
        <template v-else>
          <SongRows
            v-if="layout === 'rows'"
            :songs="songs"
            :selected="selected"
            :selected-ids="selectedIds"
            :playing="playingId"
            label="Mis pistas"
            @select="select"
            @play="play"
            @context="songMenu"
          />
          <SongCards
            v-else-if="layout === 'cards'"
            :songs="songs"
            :selected="selected"
            :selected-ids="selectedIds"
            :playing="playingId"
            label="Mis pistas"
            @select="select"
            @play="play"
            @context="songMenu"
          />
          <SongGrid
            v-else-if="layout === 'grid'"
            :songs="songs"
            :selected="selected"
            :selected-ids="selectedIds"
            :playing="playingId"
            :size="cardSize"
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
            @select="select"
            @play="play"
            @context="songMenu"
          />
        </template>
      </main>
      <DetailsPanel v-if="showDetails && !isCompact" :song="detail" @updated="onUpdated" />
      <Drawer
        v-if="isCompact"
        :open="detailsOpen && showDetails"
        side="right"
        title="Ficha"
        width="min(400px, 92vw)"
        @close="detailsOpen = false"
      >
        <DetailsPanel :song="detail" @updated="onUpdated" />
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
    <transition name="study">
      <StudyBar v-if="studyOpen" @close="studyOpen = false" />
    </transition>
    <Player :study="studyOpen" @toggle-study="studyOpen = !studyOpen" />
    <CompactPlayer v-if="pocket" :variant="pocket" :roomy="width >= 560" />
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
