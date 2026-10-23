<script setup>
/**
 * Barra lateral.
 *
 * Los repertorios crecen sin techo, asi que ocupan la zona elastica y con su
 * propio scroll: «Nueva lista» va arriba del todo y Gestion queda anclada
 * abajo. Con treinta listas, crear la treintaiuna no obliga a bajar hasta el
 * final ni deja Ajustes fuera de la vista.
 */
import { ref, computed } from 'vue'
import Icon from './Icon.vue'
import TextField from './ui/TextField.vue'
import { useDragSong } from '../composables/useDragSong.js'
import { chatAttention as attention } from '../composables/chatLink.js'
import { vPreload, pages } from '../islands.js'

const props = defineProps({
  /** la vista abierta: {kind, id?, name?} */
  view: { type: Object, required: true },
  playlists: { type: Array, default: () => [] },
  stats: { type: Object, default: null },
  /** cuantos archivos esperan en la Entrada */
  inboxCount: { type: Number, default: 0 },
  /** lo que se esta bajando (el estado de useDownloads) */
  download: { type: Object, default: null },
  /** de donde sale lo que suena: {kind, id, playing}, o null */
  nowPlaying: { type: Object, default: null },
  /** DanPlay se actualizó y las novedades aún no se leyeron */
  newsBadge: Boolean
})
const emit = defineEmits(['go', 'newPlaylist', 'playlistMenu', 'openSequence'])

// Aqui se sueltan las canciones que se arrastran desde la lista. Cada destino
// solo tiene que decir quien es con `data-drop`; de lo que pasa al soltar se
// encarga la app.
const { drag, dragging, isOver } = useDragSong()

// Una propuesta del asistente (bajar lo que falta de una lista) espera tu
// respuesta y estás en otra página: el «Asistente» lo dice, como las descargas.
// Viene de chatLink.js (`attention`): la barra lateral no se trae el asistente.

/** Lo que se dice al pasar con canciones en la mano por encima de una lista. */
function dropHint(/** @type {string} */ name) {
  const n = drag.songs.length
  return n > 1
    ? `Soltar aquí para añadir las ${n} a «${name}»`
    : `Soltar aquí para añadirla a «${name}»`
}

const filter = ref('')

// Lo que se esta bajando, al lado de «Descargas»: «2/3» mientras dura y nada
// cuando no hay nada. Es la señal de que la app esta haciendo algo aunque
// estes en otra pagina (el asistente dice «espera» y aqui se ve que trabaja).
// De donde salio lo que suena: esa entrada lleva un punto. Latiendo si suena,
// quieto si esta en pausa. Es el detalle que dice «esta lista es la que esta
// puesta» sin tener que abrirla.
function sounding(kind, id = null) {
  const n = props.nowPlaying
  if (!n || n.kind !== kind) return false
  return kind === 'playlist' ? n.id === id : true
}
const soundingTitle = computed(() => (props.nowPlaying?.playing ? 'Sonando ahora' : 'En pausa'))

const downloadBadge = computed(() => {
  const d = props.download
  if (!d?.active) return ''
  return d.total > 1 ? `${Math.max(1, d.index || 1)}/${d.total}` : '1'
})
const downloadTitle = computed(() => {
  const d = props.download
  if (!d?.active) return ''
  const phase =
    {
      starting: 'Preparando',
      downloading: 'Bajando',
      converting: 'Convirtiendo a mp3',
      filing: 'Identificando y archivando'
    }[d.phase] || 'Trabajando'
  return `${phase}${d.name ? ' · ' + d.name : ''}${d.percent ? ' · ' + Math.round(d.percent) + '%' : ''}`
})
// el buscador solo aparece cuando de verdad estorba desplazarse
const FILTER_FROM = 8
const shown = computed(() => {
  const q = filter.value.trim().toLowerCase()
  const all = props.playlists || []
  return q ? all.filter((l) => (l.name || '').toLowerCase().includes(q)) : all
})
</script>

<template>
  <aside class="sidebar" :class="{ 'drop-ready': dragging }">
    <nav class="nav-group nav-fixed">
      <div class="nav-title">Biblioteca</div>
      <button
        type="button"
        class="nav-link"
        :class="{ active: view.kind === 'all' }"
        :aria-current="view.kind === 'all' ? 'page' : undefined"
        @click="emit('go', { kind: 'all' })"
      >
        <span class="nav-icon"><Icon n="note" /></span> Todas las canciones
        <span
          v-if="sounding('all')"
          class="now-dot"
          :class="{ paused: !nowPlaying.playing }"
          :title="soundingTitle"
        ></span>
        <span class="count">{{ stats?.total ?? '·' }}</span>
      </button>
      <button
        type="button"
        class="nav-link"
        data-drop="favorites"
        :class="{ active: view.kind === 'favorites', 'drop-over': isOver('favorites') }"
        :aria-current="view.kind === 'favorites' ? 'page' : undefined"
        @click="emit('go', { kind: 'favorites' })"
      >
        <span class="nav-icon"><Icon n="heart" /></span> Favoritos
        <span
          v-if="sounding('favorites')"
          class="now-dot"
          :class="{ paused: !nowPlaying.playing }"
          :title="soundingTitle"
        ></span>
      </button>
      <button
        type="button"
        class="nav-link"
        :class="{ active: view.kind === 'artists' }"
        :aria-current="view.kind === 'artists' ? 'page' : undefined"
        @click="emit('go', { kind: 'artists' })"
      >
        <span class="nav-icon"><Icon n="artists" /></span> Artistas
        <span
          v-if="sounding('artists')"
          class="now-dot"
          :class="{ paused: !nowPlaying.playing }"
          :title="soundingTitle"
        ></span>
      </button>
      <button
        type="button"
        class="nav-link"
        :class="{ active: view.kind === 'player' }"
        :aria-current="view.kind === 'player' ? 'page' : undefined"
        @click="emit('go', { kind: 'player' })"
      >
        <span class="nav-icon"><Icon n="music" /></span> Reproductor
        <span
          v-if="sounding('player')"
          class="now-dot"
          :class="{ paused: !nowPlaying.playing }"
          :title="soundingTitle"
        ></span>
      </button>
      <button
        v-preload="pages.duplicates"
        type="button"
        class="nav-link"
        :class="{ active: view.kind === 'duplicates' }"
        :aria-current="view.kind === 'duplicates' ? 'page' : undefined"
        @click="emit('go', { kind: 'duplicates' })"
      >
        <span class="nav-icon"><Icon n="duplicates" /></span> Duplicados
        <span
          v-if="sounding('duplicates')"
          class="now-dot"
          :class="{ paused: !nowPlaying.playing }"
          :title="soundingTitle"
        ></span>
      </button>
      <button
        v-preload="pages.chat"
        type="button"
        class="nav-link"
        :class="{ active: view.kind === 'chat' }"
        :aria-current="view.kind === 'chat' ? 'page' : undefined"
        :title="attention ? 'El asistente espera tu respuesta' : undefined"
        @click="emit('go', { kind: 'chat' })"
      >
        <span class="nav-icon"><Icon n="ai" /></span> Asistente
        <span v-if="attention" class="count live">1</span>
      </button>
    </nav>

    <section class="nav-group nav-grow">
      <div class="nav-title">
        Repertorios
        <span class="nav-count">{{ (playlists || []).length }}</span>
      </div>

      <button
        type="button"
        class="nav-link nav-new"
        data-drop="new-playlist"
        :class="{ 'drop-over': isOver('new-playlist') }"
        @click="emit('newPlaylist')"
      >
        <span class="nav-icon"><Icon n="plus" /></span> Nueva lista
      </button>

      <TextField
        v-if="(playlists || []).length >= FILTER_FROM"
        v-model="filter"
        icon="search"
        placeholder="Filtrar listas…"
        compact
        width="100%"
        class="nav-filter"
      />

      <div class="nav-scroll">
        <!-- abrir la lista y su menu son dos botones hermanos: un boton
             dentro de otro no se puede pulsar con el teclado -->
        <div v-for="l in shown" :key="l.id" class="nav-playlist-row">
          <button
            type="button"
            class="nav-link nav-playlist"
            :data-drop="'playlist:' + l.id"
            :class="{
              active: view.kind === 'playlist' && view.id === l.id,
              'drop-over': isOver('playlist:' + l.id)
            }"
            :aria-current="view.kind === 'playlist' && view.id === l.id ? 'page' : undefined"
            :title="dragging ? dropHint(l.name) : l.name + '  ·  clic derecho para más opciones'"
            @click="emit('go', { kind: 'playlist', id: l.id, name: l.name })"
            @contextmenu.prevent="emit('playlistMenu', $event, l)"
          >
            <span class="nav-icon" :style="l.color ? { color: l.color } : null"
              ><Icon n="list"
            /></span>
            <span class="nav-name">{{ l.name }}</span>
            <span
              v-if="sounding('playlist', l.id)"
              class="now-dot"
              :class="{ paused: !nowPlaying.playing }"
              :title="soundingTitle"
            ></span>
            <span
              v-if="l.missing"
              class="missing-dot"
              :title="
                l.missing === 1
                  ? 'Una canción de esta lista ya no está'
                  : `${l.missing} canciones de esta lista ya no están`
              "
              >{{ l.missing }}</span
            >
            <span class="count">{{ l.n }}</span>
          </button>
          <button
            type="button"
            class="more"
            title="Más opciones"
            :aria-label="'Opciones de «' + l.name + '»'"
            @click="emit('playlistMenu', $event, l)"
          >
            <Icon n="viewOptions" :t="13" />
          </button>
        </div>

        <div v-if="!shown.length" class="nav-empty">
          {{ filter ? 'Ninguna lista con ese nombre' : 'Todavía no tienes repertorios' }}
        </div>
      </div>
    </section>

    <nav class="nav-group nav-fixed nav-bottom">
      <div class="nav-title">Gestión</div>
      <button
        v-preload="pages.inbox"
        type="button"
        class="nav-link"
        :class="{ active: view.kind === 'inbox' }"
        :aria-current="view.kind === 'inbox' ? 'page' : undefined"
        @click="emit('go', { kind: 'inbox' })"
      >
        <span class="nav-icon"><Icon n="inbox" /></span> Entrada
        <span v-if="inboxCount" class="count">{{ inboxCount }}</span>
      </button>
      <button
        v-preload="pages.downloads"
        type="button"
        class="nav-link"
        :class="{ active: view.kind === 'downloads' }"
        :aria-current="view.kind === 'downloads' ? 'page' : undefined"
        :title="downloadTitle || undefined"
        @click="emit('go', { kind: 'downloads' })"
      >
        <span class="nav-icon"><Icon n="download" /></span> Descargas
        <span v-if="downloadBadge" class="count live">{{ downloadBadge }}</span>
      </button>
      <button
        type="button"
        class="nav-link"
        title="Abrir una secuencia descargada (la carpeta de pistas con su clic, su guía y su proyecto de Ableton)"
        @click="emit('openSequence')"
      >
        <span class="nav-icon"><Icon n="mixer" /></span> Abrir secuencia
      </button>
      <button
        v-preload="pages.settings"
        type="button"
        class="nav-link"
        :class="{ active: view.kind === 'settings' }"
        :aria-current="view.kind === 'settings' ? 'page' : undefined"
        @click="emit('go', { kind: 'settings' })"
      >
        <span class="nav-icon"><Icon n="settings" /></span> Ajustes
      </button>
      <button
        v-preload="pages.about"
        type="button"
        class="nav-link"
        :class="{ active: view.kind === 'about' }"
        :aria-current="view.kind === 'about' ? 'page' : undefined"
        @click="emit('go', { kind: 'about', tab: newsBadge ? 'news' : 'about' })"
      >
        <span class="nav-icon"><Icon n="info" /></span> Acerca de DanPlay
        <span v-if="newsBadge" class="count live" title="DanPlay se actualizó: mira lo nuevo"
          >Nuevo</span
        >
      </button>
      <!-- quién hizo DanPlay, siempre a la vista -->
      <button
        v-preload="pages.about"
        type="button"
        class="nav-credit"
        title="DanPlay es obra de Daniel Rangel"
        @click="emit('go', { kind: 'about', tab: 'about' })"
      >
        DanPlay · por <strong>Daniel Rangel</strong>
      </button>
    </nav>
  </aside>
</template>
