<script setup>
/**
 * «Transformar»: cada canción en su sitio y con su nombre, una a una.
 *
 * El núcleo propone (`GET /api/tidy`): dónde está y dónde iría, por qué, y
 * si es segura o para revisar. Aquí se ven de una en una: se aplica, se
 * corrige el artista o el título antes, se salta o se deshace la última.
 * «Aplicar las seguras» hace las seguras seguidas, una tras otra, con su
 * avance y se puede parar. La música sigue sonando: si se mueve la que
 * suena, la tiene en memoria y Rust se entera de su sitio nuevo.
 */
import { ref, computed, watch, onUnmounted, useTemplateRef } from 'vue'
import { api, errorMessage } from '../api.js'
import { notify } from '../composables/useNotices.js'
import { usePlayback } from '../composables/usePlayback.js'
import { useFocusTrap } from '../composables/useFocusTrap.js'
import Icon from './Icon.vue'
import Loading from './ui/Loading.vue'
import TextField from './ui/TextField.vue'

const props = defineProps({ open: Boolean })
// `changed`: algo se movió (la biblioteca se recarga)
const emit = defineEmits(['close', 'changed'])

const player = usePlayback()
const box = useTemplateRef('box')
useFocusTrap(box, { active: () => props.open })

/** @type {import('vue').Ref<any[]>} */
const items = ref([])
const looked = ref(0)
const loading = ref(false)
const at = ref(0)
const help = ref(false)
const busy = ref(false)
const error = ref('')
/** Lo último que se aplicó, para «Deshacer»: {id, from (en la biblioteca), name}. */
const last = ref(/** @type {any} */ (null))
/** «Aplicar las seguras» en marcha: {done, total} (y `stop` lo para). */
const batch = ref(/** @type {{done: number, total: number, stop: boolean}|null} */ (null))
// lo que se escribe en la tarjeta (artista y título), si se corrige
const artist = ref('')
const title = ref('')

const current = computed(() => items.value[at.value] || null)
const sure = computed(() => items.value.filter((p) => p.sure))
/** Si se corrigió el artista o el título: entonces va a las etiquetas también. */
const edited = computed(
  () =>
    !!current.value &&
    (artist.value.trim() !== current.value.artist || title.value.trim() !== current.value.title)
)
/** Por dónde va: a su carpeta de tipo (Pistas…) o a Artistas/. */
const toCategory = computed(() => !!current.value && !current.value.to.startsWith('Artistas'))
/** Dónde quedaría, con lo escrito. */
const target = computed(() => {
  const p = current.value
  if (!p) return ''
  if (!edited.value || toCategory.value) return p.to
  const ext = (p.from.match(/\.[^.\\/]+$/) || [''])[0].toLowerCase()
  const a = artist.value.trim()
  return `Artistas/${a}/${a} - ${title.value.trim()}${ext}`
})
/** La que suena ahora, si es esta. */
const playing = computed(() => !!current.value && player.track.value?.id === current.value.id)
const fileName = (rel) =>
  String(rel || '')
    .split(/[\\/]/)
    .pop()

watch(current, (p) => {
  artist.value = p?.artist || ''
  title.value = p?.title || ''
  error.value = ''
})

async function load() {
  loading.value = true
  error.value = ''
  try {
    const r = await api.tidyList()
    items.value = Array.isArray(r?.items) ? r.items : []
    looked.value = Number(r?.looked) || 0
    at.value = 0
  } catch (e) {
    error.value = errorMessage(e)
  } finally {
    loading.value = false
  }
}
watch(
  () => props.open,
  (open) => {
    if (open) {
      last.value = null
      load()
      document.addEventListener('keydown', keys)
    } else {
      document.removeEventListener('keydown', keys)
    }
  },
  { immediate: true }
)
onUnmounted(() => document.removeEventListener('keydown', keys))

/**
 * Aplica una propuesta. Las que van a su carpeta de tipo, por su ruta; las
 * demás, por artista y título (y a las etiquetas si se corrigieron).
 * @param {any} p
 * @param {{artist?: string, title?: string, tags?: boolean}} [over]
 */
async function applyOne(p, over = {}) {
  const body = p.to.startsWith('Artistas')
    ? {
        artist: over.artist ?? p.artist,
        title: over.title ?? p.title,
        tags: !!over.tags
      }
    : { to: p.to }
  const r = await api.tidy(p.id, body)
  if (r?.song) player.patchItem(r.song)
  items.value = items.value.filter((x) => x.id !== p.id)
  last.value = { id: p.id, from: p.from, name: fileName(r?.to || p.to) }
  return r
}

async function applyCurrent() {
  const p = current.value
  if (!p || busy.value) return
  busy.value = true
  error.value = ''
  try {
    await applyOne(p, {
      artist: artist.value.trim(),
      title: title.value.trim(),
      tags: edited.value
    })
    if (at.value >= items.value.length) at.value = Math.max(0, items.value.length - 1)
    emit('changed')
  } catch (e) {
    error.value = errorMessage(e)
  } finally {
    busy.value = false
  }
}

/** Las seguras, una tras otra. Se puede parar entre una y otra. */
async function applySure() {
  if (busy.value || !sure.value.length) return
  const list = sure.value.slice()
  busy.value = true
  error.value = ''
  batch.value = { done: 0, total: list.length, stop: false }
  let failed = 0
  for (const p of list) {
    if (batch.value.stop) break
    try {
      await applyOne(p)
    } catch {
      failed++
    }
    batch.value.done++
  }
  const done = batch.value.done - failed
  batch.value = null
  busy.value = false
  at.value = 0
  emit('changed')
  notify(
    `Transformadas ${done} de ${list.length}` + (failed ? ` · ${failed} no se pudieron` : ''),
    failed ? 'info' : 'ok'
  )
}

async function undo() {
  const l = last.value
  if (!l || busy.value) return
  busy.value = true
  try {
    const r = await api.tidy(l.id, { to: l.from })
    if (r?.song) player.patchItem(r.song)
    last.value = null
    emit('changed')
    await load()
  } catch (e) {
    error.value = errorMessage(e)
  } finally {
    busy.value = false
  }
}

const skip = () => (at.value = (at.value + 1) % Math.max(1, items.value.length))
const back = () =>
  (at.value = (at.value - 1 + items.value.length) % Math.max(1, items.value.length))

/** Escape cierra (si no está aplicando: que no se quede a medias sin verse). */
function keys(e) {
  if (e.key !== 'Escape' || e.defaultPrevented || busy.value) return
  e.preventDefault()
  emit('close')
}
</script>

<template>
  <transition name="fade">
    <div
      v-if="open"
      class="modal-back"
      role="presentation"
      @mousedown.self="!busy && emit('close')"
    >
      <div
        ref="box"
        class="modal tidy-modal"
        role="dialog"
        aria-modal="true"
        aria-label="Transformar tu música"
      >
        <h3 class="modal-title">
          <Icon n="sparkles" :t="16" /> Transformar
          <button
            class="btn mini tidy-help-btn"
            type="button"
            :aria-expanded="help ? 'true' : 'false'"
            @click="help = !help"
          >
            ¿Qué hace?
          </button>
          <button
            class="btn mini tidy-close"
            type="button"
            title="Cerrar"
            :disabled="busy"
            @click="emit('close')"
          >
            <Icon n="close" :t="13" />
          </button>
        </h3>

        <transition name="dropdown">
          <div v-if="help" class="tidy-help">
            <p>
              Pone cada canción en su sitio y con su nombre, como quedan las que descargas:
              <span class="mono">Artistas/Artista/Artista - Título</span>. Las Drum Cam, los
              tutoriales y las secuencias, a su carpeta.
            </p>
            <p>
              <b>Una a una, y tú decides.</b> Ves dónde está y dónde iría, puedes corregir el
              artista o el título antes, saltarla o deshacer la última. Nada se mueve sin que lo
              pidas.
            </p>
            <p>
              <b>Sigue siendo la misma canción:</b> sus estrellas, listas, estudio, pistas separadas
              y el clic van con ella. Si está sonando, sigue sonando.
            </p>
            <p>
              Solo mira <span class="mono">Artistas/</span> y lo suelto en tu carpeta de música; tus
              carpetas (Pistas, Tutoriales…) no se tocan. Las «seguras» salen del propio nombre del
              archivo o de su carpeta; las que solo dicen sus etiquetas, revísalas.
            </p>
          </div>
        </transition>

        <Loading v-if="loading" style="margin: 18px 0" />
        <div v-else-if="!items.length" class="tidy-empty">
          <Icon n="check" :t="18" />
          <div>
            <b>Todo en su sitio.</b>
            <span v-if="looked"> Se miraron {{ looked }} canciones.</span>
          </div>
        </div>
        <template v-else>
          <div class="tidy-count">
            <span
              >Canción <b>{{ at + 1 }}</b> de {{ items.length }}</span
            >
            <span class="tidy-dot">·</span>
            <span>{{ sure.length }} {{ sure.length === 1 ? 'segura' : 'seguras' }}</span>
          </div>

          <transition name="tidy-swap" mode="out-in">
            <article v-if="current" :key="current.id" class="tidy-card">
              <div class="tidy-badges">
                <span v-if="current.sure" class="tidy-badge ok">segura</span>
                <span v-else class="tidy-badge doubt">revísala</span>
                <span v-if="playing" class="tidy-badge playing">suena ahora · seguirá sonando</span>
              </div>
              <div class="tidy-row">
                <span class="tidy-k">Ahora</span>
                <span class="tidy-path mono" :title="current.from">{{ current.from }}</span>
              </div>
              <div class="tidy-row">
                <span class="tidy-k">Quedaría</span>
                <span class="tidy-path mono to" :title="target">{{ target }}</span>
              </div>
              <ul class="tidy-why">
                <li v-for="w in current.why" :key="w">{{ w }}</li>
              </ul>
              <div v-if="!toCategory" class="tidy-fields">
                <TextField v-model="artist" label="Artista" width="100%" compact />
                <TextField v-model="title" label="Título" width="100%" compact />
              </div>
              <div v-if="edited" class="tidy-note">
                Lo corregido se guarda también en sus etiquetas.
              </div>
              <div v-if="error" class="tidy-error" role="alert">{{ error }}</div>
            </article>
          </transition>
        </template>

        <!-- abajo del todo y siempre a la vista: con la ayuda abierta, los
             botones quedaban por debajo y habia que bajar para aplicar -->
        <div v-if="items.length || last" class="tidy-foot">
          <div v-if="items.length" class="tidy-actions">
            <button
              class="btn mini"
              type="button"
              :disabled="busy || items.length < 2"
              title="La anterior"
              @click="back"
            >
              ←
            </button>
            <button
              class="btn mini"
              type="button"
              :disabled="busy || items.length < 2"
              title="Dejarla como está y ver la siguiente"
              @click="skip"
            >
              Saltar
            </button>
            <button
              class="btn primary tidy-apply"
              type="button"
              :disabled="busy || !current || (!toCategory && (!artist.trim() || !title.trim()))"
              @click="applyCurrent"
            >
              <Icon n="check" :t="13" /> Aplicar
            </button>
            <span class="tidy-spacer"></span>
            <button
              v-if="!batch"
              class="btn mini"
              type="button"
              :disabled="busy || !sure.length"
              title="Las seguras, una tras otra (se puede parar)"
              @click="applySure"
            >
              Aplicar las seguras ({{ sure.length }})
            </button>
            <template v-else>
              <span class="tidy-batch mono">{{ batch.done }} / {{ batch.total }}</span>
              <button class="btn mini" type="button" @click="batch.stop = true">Parar</button>
            </template>
          </div>
          <transition name="fade">
            <div v-if="last && !batch" class="tidy-last">
              <span
                >Hecho: <b>{{ last.name }}</b></span
              >
              <button class="btn mini" type="button" :disabled="busy" @click="undo">
                Deshacer
              </button>
            </div>
          </transition>
        </div>
      </div>
    </div>
  </transition>
</template>
