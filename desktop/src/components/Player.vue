<script setup>
import { ref, watch, onMounted, onUnmounted, computed, nextTick } from 'vue'
import { onClickOutside } from '../composables/useClickOutside.js'
import { api, native } from '../api.js'
import Icon from './Icon.vue'
import CoverArt from './ui/CoverArt.vue'
import SliderField from './ui/SliderField.vue'

const props = defineProps(['song','queue','shuffle','repeat','origin','pulse'])
const emit = defineEmits(['previous','next','toggleShuffle','cycleRepeat','jumpTo',
                          'goToOrigin','trackEnded','playSelected','playState'])

// Como se pinta cada modo. El icono dice «lista o cancion» y la marca dice
// «para siempre o una vez».
const REPEAT_LOOK = {
  list:  { icon: 'repeat',    mark: '∞', on: true,  title: 'Repetir la lista (infinito)' },
  one:   { icon: 'repeatOne', mark: '∞', on: true,  title: 'Repetir esta cancion (infinito)' },
  once:  { icon: 'repeatOne', mark: '1', on: true,  title: 'Solo esta cancion: al acabar, se para' },
  queue: { icon: 'repeat',    mark: '1', on: false, title: 'La lista una vez y para' }
}
const repeatLook = computed(() => REPEAT_LOOK[props.repeat] || REPEAT_LOOK.list)

const audio = ref(null)                 // solo se usa fuera de la app (navegador)
const playing = ref(false)
const pos = ref(0)
const total = ref(0)
const volume = ref(Number(localStorage.getItem('danplay.vol') ?? 0.9))
const muted = ref(false)
const speed = ref(Number(localStorage.getItem('danplay.vel') ?? 1))
const showQueue = ref(false)
const fullQueue = ref(false)
const queuePanel = ref(null)
const queueButton = ref(null)
const currentRow = ref(null)
const failure = ref('')
const SPEEDS = [0.5, 0.75, 0.9, 1, 1.1, 1.25, 1.5, 2]
let poll = null
let wasFinished = false


// Solo tres: la anterior, la que suena y la siguiente. Para ver el resto,
// "Ver todo" lleva a la lista desde la que se puso a sonar.
const trio = computed(() => {
  const c = props.queue || []
  const i = c.findIndex(x => x.id === props.song?.id)
  if (i < 0) return []
  return [
    { pos: 'antes', c: c[i - 1] || null },
    { pos: 'ahora', c: c[i] },
    { pos: 'luego', c: c[i + 1] || null }
  ]
})
const LABELS = { antes: 'Sonaba antes', ahora: 'Sonando ahora', luego: 'A continuacion' }

onClickOutside([queuePanel, queueButton], () => { showQueue.value = false })

async function toggleQueue () {
  showQueue.value = !showQueue.value
  if (!showQueue.value) fullQueue.value = false
}
async function expandQueue () {
  fullQueue.value = !fullQueue.value
  if (fullQueue.value) {
    await nextTick()
    const el = currentRow.value
    if (el && typeof el.scrollIntoView === 'function') el.scrollIntoView({ block: 'center' })
  }
}

// ------------------------------------------------------------------ cargar
// Se vigila tambien el pulso: asi volver a pulsar la cancion que ya suena la
// empieza de nuevo, en vez de no hacer nada porque su id no ha cambiado.
watch(() => `${props.song?.id ?? ''}:${props.pulse ?? 0}`, async () => {
  const id = props.song?.id
  failure.value = ''
  if (!id) return
  if (!native.available) {
    audio.value.src = api.audioUrl(id)
    audio.value.volume = muted.value ? 0 : volume.value
    audio.value.playbackRate = speed.value
    Promise.resolve(audio.value.play()).catch(() => {})
    return
  }
  try {
    const { path } = await api.path(id)
    wasFinished = false
    await native.volume(muted.value ? 0 : volume.value)
    await native.speed(speed.value)
    await native.play(path)
  } catch (e) {
    failure.value = String(e).replace(/^Error:\s*/, '')
  }
})

// -------------------------------------------------------------- estado real
async function refresh () {
  if (!native.available) return
  try {
    const e = await native.status()
    playing.value = e.playing
    pos.value = e.position
    total.value = e.duration || props.song?.duration || 0
    if (e.error) failure.value = e.error
    else if (!e.has_output) failure.value = 'Este equipo no tiene salida de audio'
    // fin de pista: se avisa una sola vez. Que hacer luego (repetir, avanzar
    // o pararse) lo decide App, que es quien tiene la cola y el modo.
    if (e.finished && e.path && !wasFinished) {
      wasFinished = true
      emit('trackEnded')
    }
    if (!e.finished) wasFinished = false
  } catch {}
}

// La bandeja necesita saber si esta sonando o en pausa, y quien lo sabe es
// esto: el estado real lo trae el hilo de audio, no la lista.
watch(playing, v => emit('playState', v))

function onKey (e) {
  const t = e.target
  if (t && (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA' || t.isContentEditable)) return
  switch (e.key) {
    case ' ': e.preventDefault(); togglePlay(); break
    case 'ArrowRight': nudge(e.shiftKey ? 30 : 10); break
    case 'ArrowLeft':  nudge(e.shiftKey ? -30 : -10); break
    case 'ArrowUp':   e.preventDefault(); bumpVolume(0.05); break
    case 'ArrowDown': e.preventDefault(); bumpVolume(-0.05); break
    case 'm': case 'M': toggleMute(); break
    case 's': case 'S': emit('toggleShuffle'); break
    case 'r': case 'R': emit('cycleRepeat'); break
    case 'n': case 'N': emit('next'); break
    case 'p': case 'P': emit('previous'); break
  }
}

// El estado del audio se pregunta seguido MIENTRAS SUENA, porque de ahi sale
// la barra de progreso y el aviso de fin de pista. Parado no hay nada que
// mirar: preguntarlo cuatro veces por segundo con la app quieta es trabajo
// tirado. Al pulsar play se refresca al momento, asi que no se nota.
const RITMO_SONANDO = 250
const RITMO_PARADO = 1000
function pollLoop () {
  poll = setTimeout(async () => {
    await refresh()
    pollLoop()
  }, playing.value ? RITMO_SONANDO : RITMO_PARADO)
}

onMounted(() => {
  window.addEventListener('keydown', onKey)
  if (native.available) {
    refresh()
    pollLoop()
  } else {
    const a = audio.value
    a.addEventListener('timeupdate', () => (pos.value = a.currentTime))
    a.addEventListener('loadedmetadata', () => (total.value = a.duration))
    a.addEventListener('play', () => { playing.value = true; failure.value = '' })
    a.addEventListener('pause', () => (playing.value = false))
    a.addEventListener('error', () => (failure.value = 'No se pudo reproducir este archivo'))
    a.addEventListener('ended', () => emit('trackEnded'))
  }
})
onUnmounted(() => {
  window.removeEventListener('keydown', onKey)
  if (poll) clearTimeout(poll)
})

// -------------------------------------------------------------- controles
async function togglePlay () {
  // Sin nada cargado, play reproduce lo que este seleccionado en la lista:
  // tener que dar doble clic a la fila para empezar no es evidente.
  if (!props.song) { emit('playSelected'); return }
  if (native.available) { await native.togglePlay(); refresh() }
  else if (playing.value) audio.value.pause()
  else Promise.resolve(audio.value.play()).catch(() => {})
}
async function seekTo (e) {
  const r = e.currentTarget.getBoundingClientRect()
  const seg = ((e.clientX - r.left) / r.width) * (total.value || 0)
  if (native.available) { await native.seek(seg); pos.value = seg }
  else audio.value.currentTime = seg
}
async function nudge (s) {
  const seg = Math.min(total.value || 0, Math.max(0, pos.value + s))
  if (native.available) { await native.seek(seg); pos.value = seg }
  else audio.value.currentTime = seg
}
function bumpVolume (d) {
  volume.value = Math.min(1, Math.max(0, +(volume.value + d).toFixed(2)))
  muted.value = false
  applyVolume()
}
function applyVolume () {
  const v = muted.value ? 0 : volume.value
  if (native.available) native.volume(v)
  else if (audio.value) audio.value.volume = v
  localStorage.setItem('danplay.vol', volume.value)
}
function toggleMute () { muted.value = !muted.value; applyVolume() }
function cycleSpeed () {
  speed.value = SPEEDS[(SPEEDS.indexOf(speed.value) + 1) % SPEEDS.length]
  if (native.available) native.speed(speed.value)
  else if (audio.value) audio.value.playbackRate = speed.value
  localStorage.setItem('danplay.vel', speed.value)
}
const tt = (s) => (!s && s !== 0) ? '0:00'
  : `${Math.floor(s / 60)}:${String(Math.floor(s % 60)).padStart(2, '0')}`
</script>

<template>
  <transition name="dropdown">
  <div v-if="showQueue" class="queue" :class="{full: fullQueue}" ref="queuePanel">
    <h4>Cola
      <span style="margin-left:auto;color:var(--muted2);text-transform:none;letter-spacing:0">
        {{ (queue||[]).length }} en total</span>
      <button @click="showQueue=false"><Icon n="close" :t="14" /></button></h4>

    <div v-if="!trio.length && !(queue||[]).length" class="queue-empty">Nada sonando</div>

    <!-- toda la cola -->
    <template v-else-if="fullQueue">
      <div v-for="(c,i) in queue" :key="c.id" class="queue-row flat"
           :class="{current: song?.id===c.id}"
           :ref="el => { if (el && song?.id===c.id) currentRow = el }"
           @click="emit('jumpTo', c)">
        <span class="queue-n">
          <Icon v-if="song?.id===c.id" n="play" :t="11" /><template v-else>{{ i+1 }}</template>
        </span>
        <span class="queue-title">{{ c.title }}<span class="sub"> · {{ c.artist || '—' }}</span></span>
        <span class="mono sub queue-dur">{{ tt(c.duration) }}</span>
      </div>
    </template>

    <!-- solo anterior, actual y siguiente -->
    <template v-else>
      <div v-for="t in trio" :key="t.pos" class="queue-row" :class="[t.pos, {act: t.pos==='ahora'}]"
           @click="t.c && emit('jumpTo', t.c)">
        <span class="queue-label">{{ LABELS[t.pos] }}</span>
        <template v-if="t.c">
          <span class="queue-n">
            <Icon v-if="t.pos==='ahora'" n="play" :t="11" />
            <Icon v-else-if="t.pos==='antes'" n="previous" :t="11" />
            <Icon v-else n="next" :t="11" />
          </span>
          <span class="queue-title">
            {{ t.c.title }}<span class="sub"> · {{ t.c.artist || '—' }}</span></span>
          <span class="mono sub queue-dur">{{ t.c.duration ? tt(t.c.duration) : '' }}</span>
        </template>
        <span v-else class="queue-none">—</span>
      </div>
    </template>

    <div class="queue-foot">
      <button class="queue-more" @click="expandQueue">
        <Icon :n="fullQueue ? 'viewCompact' : 'queue'" :t="14" />
        {{ fullQueue ? 'Ver solo 3' : 'Ver la cola entera (' + (queue||[]).length + ')' }}
      </button>
      <button class="queue-more" @click="emit('goToOrigin'); showQueue = false">
        <Icon n="right" :t="14" />
        Ir a{{ origin ? ' ' + origin : ' la lista' }}
      </button>
    </div>
  </div>
  </transition>

  <div class="player">
    <audio v-if="!native.available" ref="audio" preload="metadata"></audio>
    <CoverArt :id="song?.id" :blur="!!song?.blur" class="pl-cover" :icon-size="20" :alt="song?.title || ''" />

    <div class="pl-info">
      <div class="title" style="overflow:hidden;text-overflow:ellipsis;white-space:nowrap">
        {{ song?.title || 'Nada sonando' }}</div>
      <div class="sub" style="overflow:hidden;text-overflow:ellipsis;white-space:nowrap">
        <span v-if="failure" style="color:var(--red)">{{ failure }}</span>
        <template v-else>
          {{ song?.artist || '—' }}
          <span v-if="song?.key" class="mono"> · {{ song.key }}</span>
          <span v-if="song?.bpm" class="mono"> · {{ Math.round(song.bpm) }} bpm</span>
        </template>
      </div>
    </div>

    <div class="pl-controls">
      <button class="pl-btn" :class="{on: shuffle}" title="Aleatorio (S)"
              @click="emit('toggleShuffle')"><Icon n="shuffle" :t="16" /></button>
      <button class="pl-btn" title="Anterior (P)" @click="emit('previous')"><Icon n="previous" :t="16" /></button>
      <button class="pl-btn" title="Retroceder 10 s (←)" @click="nudge(-10)"><Icon n="back10" :t="15" /></button>
      <button class="pl-btn pl-play" title="Reproducir / pausar (espacio)"
              @click="togglePlay"><Icon :n="playing ? 'pause' : 'play'" :t="16" /></button>
      <button class="pl-btn" title="Avanzar 10 s (→)" @click="nudge(10)"><Icon n="forward10" :t="15" /></button>
      <button class="pl-btn" title="Siguiente (N)" @click="emit('next')"><Icon n="next" :t="16" /></button>
      <button class="pl-btn repeat-btn" :class="{on: repeatLook.on}"
              :title="repeatLook.title + '  (R)'"
              @click="emit('cycleRepeat')">
        <Icon :n="repeatLook.icon" :t="16" />
        <span class="repeat-mark">{{ repeatLook.mark }}</span>
      </button>
    </div>

    <div class="pl-bar">
      <span class="time">{{ tt(pos) }}</span>
      <div class="track" @click="seekTo">
        <div class="track-fill" :style="{width: total ? (pos/total*100)+'%' : '0%'}"></div>
      </div>
      <span class="time">{{ tt(total) }}</span>
    </div>

    <button class="speed" title="Velocidad de reproduccion"
            @click="cycleSpeed">{{ speed }}×</button>

    <div style="display:flex;align-items:center;gap:6px;width:118px">
      <button class="pl-btn" style="width:24px;height:24px;font-size:12px"
              :title="muted?'Quitar silencio (M)':'Silenciar (M)'"
              @click="toggleMute"><Icon :n="(muted || !volume) ? 'mute' : 'volume'" :t="15" /></button>
      <SliderField :modelValue="volume" :min="0" :max="1" :step="0.01" width="100%"
                  @update:modelValue="v => { volume = v; muted = false; applyVolume() }" />
    </div>

    <button class="pl-btn" :class="{on: showQueue}" title="Cola de reproduccion"
            ref="queueButton" @click="toggleQueue"><Icon n="queue" :t="16" /></button>
  </div>
</template>
