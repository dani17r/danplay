<script setup>
/**
 * La proyección: la letra en grande para el proyector de la iglesia (o para
 * el atril). Es una ventana aparte, que se arrastra a la otra pantalla y se
 * pone a pantalla completa con F o con doble clic.
 *
 * Sigue lo que suena: con letra con tiempos (LRCLIB, o un LRC en el mp3) la
 * línea actual va en grande y las de alrededor en gris, sin que nadie
 * toque nada. Sin tiempos, la letra se pasa a mano por bloques (↑ ↓, PgUp,
 * PgDn, espacio pausa). Comparte estado con la app a través de Rust:
 * `usePlayback` escucha el mismo evento que la ventana grande.
 */
import { ref, computed, watch, onMounted, onUnmounted } from 'vue'
import { api, core, projection } from './api.js'
import { usePlayback } from './composables/usePlayback.js'
import { parseLrc, stripLrc, currentLine } from './utils/lrc.js'

const player = usePlayback()
const { track, playing, position } = player

const song = ref(null) // la ficha completa de lo que suena (con la letra)
const loadedFor = ref(null)
const fontSize = ref(load('fontSize', 56))
const contextLines = ref(load('contextLines', 2)) // lineas de alrededor
const block = ref(0) // bloque actual, para la letra sin tiempos
const fullscreen = ref(false)
const showBar = ref(true)
let barTimer = null
let stopChanges = null
let gone = false

function load(key, fallback) {
  try {
    const v = localStorage.getItem('danplay.projection.' + key)
    return v == null ? fallback : JSON.parse(v)
  } catch {
    return fallback
  }
}
function save(key, value) {
  try {
    localStorage.setItem('danplay.projection.' + key, JSON.stringify(value))
  } catch {
    /* sin almacenamiento */
  }
}

// Cuando cambia lo que suena, se pide su ficha: la letra no viaja en el
// estado de reproduccion (seria mucho para cada tick).
async function loadSong(id) {
  if (!id) {
    song.value = null
    loadedFor.value = null
    return
  }
  loadedFor.value = id
  try {
    const s = await api.song(id)
    if (loadedFor.value === id) song.value = s
  } catch {
    if (loadedFor.value === id) song.value = null
  }
}
watch(
  () => track.value?.id ?? null,
  (id) => {
    block.value = 0
    loadSong(id)
  },
  { immediate: true }
)

const lines = computed(() => parseLrc(song.value?.lyrics_synced) || parseLrc(song.value?.lyrics))
const plain = computed(
  () => (song.value?.lyrics_synced ? song.value.lyrics : stripLrc(song.value?.lyrics)) || ''
)
/** La letra sin tiempos, en bloques (estrofas separadas por lineas en blanco). */
const blocks = computed(() =>
  plain.value
    .split(/\n\s*\n/)
    .map((b) => b.trim())
    .filter(Boolean)
)
const current = computed(() => (lines.value ? currentLine(lines.value, position.value) : -1))
/** Las lineas que se ven: la actual y las de alrededor. */
const window_ = computed(() => {
  if (!lines.value) return []
  const n = contextLines.value
  const i = Math.max(0, current.value)
  const out = []
  for (let k = i - n; k <= i + n; k++) {
    const l = lines.value[k]
    out.push({
      key: k,
      text: l ? l.text : '',
      state: !l ? 'empty' : k === current.value ? 'current' : k < current.value ? 'past' : 'next'
    })
  }
  return out
})

function onKey(e) {
  if (e.key === 'Escape') {
    if (fullscreen.value) toggleFullscreen()
    return
  }
  if (e.key === 'f' || e.key === 'F' || e.key === 'F11') {
    e.preventDefault()
    toggleFullscreen()
    return
  }
  if (e.key === ' ') {
    e.preventDefault()
    player.toggle()
    return
  }
  if (e.key === '+' || e.key === '=') {
    e.preventDefault()
    bigger(1)
    return
  }
  if (e.key === '-') {
    e.preventDefault()
    bigger(-1)
    return
  }
  if (!lines.value && blocks.value.length) {
    if (e.key === 'ArrowDown' || e.key === 'PageDown' || e.key === 'ArrowRight') {
      e.preventDefault()
      block.value = Math.min(blocks.value.length - 1, block.value + 1)
    }
    if (e.key === 'ArrowUp' || e.key === 'PageUp' || e.key === 'ArrowLeft') {
      e.preventDefault()
      block.value = Math.max(0, block.value - 1)
    }
  }
}
function bigger(dir) {
  fontSize.value = Math.max(24, Math.min(140, fontSize.value + dir * 6))
  save('fontSize', fontSize.value)
}
function setContext(n) {
  contextLines.value = n
  save('contextLines', n)
}
async function toggleFullscreen() {
  fullscreen.value = !fullscreen.value
  try {
    await projection.fullscreen(fullscreen.value)
  } catch {
    /* fuera de la app: nada que hacer */
  }
}
function wake() {
  showBar.value = true
  clearTimeout(barTimer)
  barTimer = setTimeout(() => {
    showBar.value = false
  }, 2500)
}
onMounted(async () => {
  document.addEventListener('keydown', onKey)
  document.addEventListener('mousemove', wake)
  wake()
  // La letra que se encuentra con la cancion ya sonando (el boton de la
  // ficha, el asistente) cambia el nucleo y no lo que suena: sin esto, al
  // proyector no llegaba hasta cambiar de cancion.
  const stop = await core.onChanged(() => loadSong(track.value?.id ?? null))
  if (gone)
    stop() // se cerro mientras se apuntaba
  else stopChanges = stop
  try {
    fullscreen.value = await projection.isFullscreen()
  } catch {
    /* fuera de la app */
  }
})
onUnmounted(() => {
  document.removeEventListener('keydown', onKey)
  document.removeEventListener('mousemove', wake)
  clearTimeout(barTimer)
  gone = true
  stopChanges?.()
})
</script>

<template>
  <!-- doble clic: pantalla completa (con el teclado, F) -->
  <div
    class="proj"
    role="presentation"
    :style="{ '--proj-size': fontSize + 'px' }"
    @dblclick="toggleFullscreen"
  >
    <!-- sin nada sonando -->
    <div v-if="!track" class="proj-idle">
      <div class="proj-brand">DanPlay</div>
      <div class="proj-hint">Pon una canción con letra y aparecerá aquí</div>
    </div>

    <!-- letra con tiempos: sigue sola -->
    <div v-else-if="lines" class="proj-synced" :class="{ waiting: current < 0 }">
      <div v-for="l in window_" :key="l.key" class="proj-line" :class="l.state">
        {{ l.text || (l.state === 'current' ? '♪' : '') }}
      </div>
    </div>

    <!-- letra sin tiempos: por bloques, a mano -->
    <div v-else-if="blocks.length" class="proj-block">
      <div class="proj-text">{{ blocks[block] }}</div>
      <div class="proj-pager">{{ block + 1 }} / {{ blocks.length }} · ↑ ↓ para pasar</div>
    </div>

    <div v-else class="proj-idle">
      <div class="proj-title">{{ track.artist }} — {{ track.title }}</div>
      <div class="proj-hint">Esta canción no tiene letra guardada</div>
    </div>

    <!-- la barra de abajo: que suena y los mandos; se esconde sola -->
    <div class="proj-bar" :class="{ hidden: !showBar && fullscreen }">
      <span v-if="track" class="proj-now"
        >{{ track.artist }} — {{ track.title }}<span v-if="!playing"> · en pausa</span></span
      >
      <span v-else class="proj-now">Nada sonando</span>
      <span style="flex: 1"></span>
      <button class="btn mini" type="button" title="Más pequeño (−)" @click="bigger(-1)">A−</button>
      <button class="btn mini" type="button" title="Más grande (+)" @click="bigger(1)">A+</button>
      <button
        class="btn mini"
        type="button"
        :class="{ on: contextLines === 0 }"
        title="Solo la línea que suena"
        @click="setContext(0)"
      >
        1 línea
      </button>
      <button
        class="btn mini"
        type="button"
        :class="{ on: contextLines === 2 }"
        title="La línea y las de alrededor"
        @click="setContext(2)"
      >
        contexto
      </button>
      <button
        class="btn mini"
        type="button"
        :title="fullscreen ? 'Salir de pantalla completa (Esc)' : 'Pantalla completa (F)'"
        @click="toggleFullscreen"
      >
        {{ fullscreen ? 'Salir' : 'Pantalla completa' }}
      </button>
    </div>
  </div>
</template>
