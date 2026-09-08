<script setup>
/**
 * El mini reproductor: lo que sale al pulsar el icono de la bandeja.
 *
 * Es un popup, no una ventana suelta. Aparece pegado al icono (donde el
 * sistema deja: ver docs/PLAN.md §2.8), se va al perder el foco o con Escape,
 * y no se arrastra ni recuerda posición, porque la coloca Rust cada vez.
 *
 * Comparte estado con la ventana grande a través de Rust: `usePlayback`
 * escucha el mismo evento, así que lo que se pulsa aquí se ve allí y al revés.
 */
import { computed, onMounted, onUnmounted } from 'vue'
import { app, mini } from './api.js'
import { usePlayback } from './composables/usePlayback.js'
import { formatTime } from './utils/format.js'
import Icon from './components/Icon.vue'
import CoverArt from './components/ui/CoverArt.vue'
import { applyTheme, applyDensity, savedTheme, savedDensity } from './themes.js'

const player = usePlayback()
const { track, playing, position, duration, hasPrevious, hasNext } = player

const loaded = computed(() => !!track.value)
const percent = computed(() =>
  duration.value ? Math.min(100, (position.value / duration.value) * 100) : 0
)

// Los dos atajos que se esperan en una ventanita así: espacio para pausar y
// Escape para quitarla de en medio.
function onKey(e) {
  if (e.key === 'Escape') mini.hide()
  else if (e.key === ' ') {
    e.preventDefault()
    if (loaded.value) player.toggle()
  }
}

// El tema se guarda en el navegador, que es el mismo para las dos ventanas.
// Sin esto, cambiarlo en la app dejaba el mini con el tema viejo hasta
// reiniciar.
function onStorage(e) {
  if (!e || e.key === null || e.key === 'danplay.theme') applyTheme(savedTheme())
  if (!e || e.key === null || e.key === 'danplay.density') applyDensity(savedDensity())
}

onMounted(() => {
  window.addEventListener('keydown', onKey)
  window.addEventListener('storage', onStorage)
  applyTheme(savedTheme())
  applyDensity(savedDensity())
})
onUnmounted(() => {
  window.removeEventListener('keydown', onKey)
  window.removeEventListener('storage', onStorage)
})

function seek(e) {
  if (!loaded.value || !duration.value) return
  const r = e.currentTarget.getBoundingClientRect()
  player.seek(((e.clientX - r.left) / r.width) * duration.value)
}
</script>

<template>
  <div class="miniplayer">
    <header class="mini-head">
      <span class="mini-brand"> <span class="brand-dot"></span> DANPLAY</span>
      <button class="icon-btn" title="Abrir DanPlay" @click="app.showWindow()">
        <Icon n="viewGrid" :t="13" />
      </button>
      <button class="icon-btn" title="Cerrar la ventanita" @click="mini.hide()">
        <Icon n="close" :t="13" />
      </button>
    </header>

    <div class="mini-song">
      <CoverArt
        :id="track?.id"
        :blur="!!track?.blur"
        class="mini-art"
        :icon-size="18"
        :size="96"
        :alt="track?.title || ''"
      />
      <div class="mini-text">
        <div class="mini-title">
          {{ loaded ? track.title || 'Sin título' : 'Nada sonando' }}
        </div>
        <div class="mini-artist">
          {{ loaded ? track.artist || 'Sin artista' : 'Elige algo en DanPlay' }}
        </div>
      </div>
    </div>

    <div
      class="mini-seek"
      :class="{ off: !loaded }"
      :title="loaded ? 'Ir a un punto' : ''"
      @click="seek"
    >
      <div class="mini-seek-fill" :style="{ width: percent + '%' }"></div>
    </div>
    <div class="mini-times">
      <span>{{ formatTime(position) }}</span>
      <span>{{ loaded ? formatTime(duration) : '' }}</span>
    </div>

    <div class="mini-controls">
      <button
        class="mini-btn"
        :disabled="!loaded || !hasPrevious"
        title="Anterior"
        @click="player.previous()"
      >
        <Icon n="previous" :t="15" />
      </button>
      <button
        class="mini-btn big"
        :disabled="!loaded"
        :title="playing ? 'Pausar' : 'Reproducir'"
        @click="player.toggle()"
      >
        <Icon :n="playing ? 'pause' : 'play'" :t="17" />
      </button>
      <button
        class="mini-btn"
        :disabled="!loaded || !hasNext"
        title="Siguiente"
        @click="player.next()"
      >
        <Icon n="next" :t="15" />
      </button>
    </div>
  </div>
</template>
