<script setup>
/**
 * El reproductor en pequeño, para la ventanita de la bandeja.
 *
 * Vive en su propia ventana, asi que no comparte nada con la app: lo que
 * suena se lo cuenta Rust, que guarda lo ultimo que le dijo la ventana
 * principal, y el avance lo pregunta al hilo de audio, que es quien lo sabe
 * de verdad.
 *
 * Pausar y mover la barra son ordenes directas al audio: funcionan aunque la
 * ventana principal este ocupada. Cambiar de cancion no, porque la cola y el
 * modo de repeticion viven alli; se le manda un aviso y ella decide. Por eso
 * «anterior» y «siguiente» salen en gris cuando no hay a donde ir.
 */
import { ref, computed, onMounted, onUnmounted } from 'vue'
import { native, tray } from './api.js'
import Icon from './components/Icon.vue'
import CoverArt from './components/ui/CoverArt.vue'
import { applyTheme, applyDensity, savedTheme, savedDensity } from './themes.js'

const VACIO = { id: null, title: '', artist: '', playing: false, blur: false,
                has_previous: false, has_next: false }

const now = ref({ ...VACIO })
const pos = ref(0)
const total = ref(0)
const playing = ref(false)

const loaded = computed(() => now.value.id != null)
const pct = computed(() => total.value ? Math.min(100, (pos.value / total.value) * 100) : 0)

function fmt (s) {
  if (!s || s < 0) return '0:00'
  const m = Math.floor(s / 60), r = Math.floor(s % 60)
  return `${m}:${String(r).padStart(2, '0')}`
}

let poll = null
let stop = null

// Igual que en el reproductor grande: seguido mientras suena, tranquilo
// cuando esta parado. Es una ventanita que se queda abierta encima de todo,
// asi que no tiene sentido que este preguntando sin parar sin nada que sonar.
function pollLoop () {
  poll = setTimeout(async () => {
    await refresh()
    pollLoop()
  }, playing.value ? 400 : 1200)
}

async function refresh () {
  try {
    const e = await native.status()
    playing.value = !!e.playing
    pos.value = e.position || 0
    total.value = e.duration || 0
  } catch { /* la app se esta cerrando */ }
}

// Donde la dejaste la ultima vez. Rust la pone en una esquina al crearla,
// pero en Linux nadie sabe donde esta el icono de la bandeja, asi que lo
// razonable es que la coloques tu una vez y no tener que repetirlo.
const POS = 'danplay.miniPos'
async function rememberPlace () {
  if (!tray.available) return
  try {
    const { getCurrentWindow } = await import('@tauri-apps/api/window')
    const { PhysicalPosition } = await import('@tauri-apps/api/dpi')
    const win = getCurrentWindow()
    const saved = JSON.parse(localStorage.getItem(POS) || 'null')
    if (saved) await win.setPosition(new PhysicalPosition(saved.x, saved.y))
    await win.onMoved(({ payload }) => {
      try { localStorage.setItem(POS, JSON.stringify({ x: payload.x, y: payload.y })) } catch {}
    })
  } catch { /* fuera de la app no hay ventana que colocar */ }
}

// Los dos atajos que se esperan en una ventanita asi: espacio para
// pausar y Escape para quitarla de en medio.
function onKey (e) {
  if (e.key === 'Escape') tray.closeMini()
  else if (e.key === ' ') { e.preventDefault(); toggle() }
}

onMounted(async () => {
  window.addEventListener('keydown', onKey)
  // el mismo tema y la misma densidad que la app: comparten el navegador
  applyTheme(savedTheme()); applyDensity(savedDensity())
  const first = await tray.nowPlaying()
  if (first) now.value = { ...VACIO, ...first }
  stop = await tray.onChanged(d => { now.value = { ...VACIO, ...d } })
  await refresh()
  pollLoop()
  rememberPlace()
})
onUnmounted(() => {
  window.removeEventListener('keydown', onKey)
  if (poll) clearTimeout(poll)
  if (typeof stop === 'function') stop()
})

const toggle = () => { if (loaded.value) native.togglePlay().then(refresh) }
const previous = () => tray.send('previous')
const next = () => tray.send('next')
function seek (e) {
  if (!loaded.value || !total.value) return
  const r = e.currentTarget.getBoundingClientRect()
  const s = ((e.clientX - r.left) / r.width) * total.value
  pos.value = s
  native.seek(s)
}
</script>

<template>
  <div class="miniplayer">
    <!-- la barra de arriba hace de asa: la ventana no tiene marco -->
    <header class="mini-head" data-tauri-drag-region>
      <span class="mini-brand" data-tauri-drag-region>
        <span class="brand-dot"></span> DANPLAY</span>
      <button class="icon-btn" title="Abrir DanPlay" @click="tray.showApp()">
        <Icon n="viewGrid" :t="13" /></button>
      <button class="icon-btn" title="Cerrar la ventanita" @click="tray.closeMini()">
        <Icon n="close" :t="13" /></button>
    </header>

    <div class="mini-song">
      <CoverArt :id="now.id" :blur="!!now.blur" class="mini-art" :icon-size="18" :alt="now.title" />
      <div class="mini-text">
        <div class="mini-title">{{ loaded ? (now.title || 'Sin titulo') : 'Nada sonando' }}</div>
        <div class="mini-artist">
          {{ loaded ? (now.artist || 'Sin artista') : 'Elige algo en DanPlay' }}</div>
      </div>
    </div>

    <div class="mini-seek" :class="{off: !loaded}" @click="seek"
         :title="loaded ? 'Ir a un punto' : ''">
      <div class="mini-seek-fill" :style="{width: pct + '%'}"></div>
    </div>
    <div class="mini-times">
      <span>{{ fmt(pos) }}</span>
      <span>{{ loaded ? fmt(total) : '' }}</span>
    </div>

    <div class="mini-controls">
      <button class="mini-btn" :disabled="!loaded || !now.has_previous"
              title="Anterior" @click="previous"><Icon n="previous" :t="15" /></button>
      <button class="mini-btn big" :disabled="!loaded"
              :title="playing ? 'Pausar' : 'Reproducir'" @click="toggle">
        <Icon :n="playing ? 'pause' : 'play'" :t="17" /></button>
      <button class="mini-btn" :disabled="!loaded || !now.has_next"
              title="Siguiente" @click="next"><Icon n="next" :t="15" /></button>
    </div>
  </div>
</template>
