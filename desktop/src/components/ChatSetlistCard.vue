<script setup>
/**
 * La tabla de una lista metida o comprobada desde el chat (`fill_playlist`),
 * bajo la respuesta que la trae.
 *
 * Cada canción de la lista pegada junto a lo que se encontró en la biblioteca:
 * en la lista, otra versión, dudosa o falta, con las alternativas que hay.
 * Sus botones arreglan la lista SIN pasar por el modelo («Dejar solo estas»,
 * «Ordenar como mi lista», «Deshacer»); cada uno deja un mensaje que lo
 * cuenta (useChat.fixList). Larga, se pliega.
 */
import { ref, computed, watch, onMounted, onUnmounted, useTemplateRef, useId } from 'vue'
import { useChat, CLICK_GUARD_MS } from '../composables/useChat.js'
import Icon from './Icon.vue'

const props = defineProps({
  /** el mensaje del asistente que trae la tabla (`msg.setlist`) */
  msg: { type: Object, required: true }
})

const chat = useChat()
const s = computed(() => props.msg.setlist)
const name = computed(() => s.value.playlist?.name || 'la lista')

// larga: se ven las primeras y «Ver las N» abre el resto
const FOLD_ROWS = 8
const SHOWN_ROWS = 6
const open = ref(false)
const long = computed(() => s.value.rows.length > FOLD_ROWS)
const rows = computed(() =>
  long.value && !open.value ? s.value.rows.slice(0, SHOWN_ROWS) : s.value.rows
)
// para que «Ver las N» diga qué abre
const tableId = useId()

/** «Artista - Título». */
const songName = (x) => (x?.artist ? `${x.artist} - ${x.title}` : x?.title || '')

/**
 * Cómo está cada fila: icono, palabras y tono.
 * @param {any} r
 */
function mark(r) {
  const listed = r.in_list
  if (r.downloaded && listed) return { icon: 'check', label: 'bajada, en la lista', tone: 'ok' }
  if (r.status === 'found') {
    return listed
      ? { icon: 'check', label: 'en la lista', tone: 'ok' }
      : { icon: 'check', label: 'la tienes, no está en la lista', tone: 'ok' }
  }
  if (r.status === 'other_version') {
    return {
      icon: 'check',
      label: listed ? 'otra versión' : 'otra versión, no está en la lista',
      tone: 'ok'
    }
  }
  if (r.status === 'ambiguous') return { icon: 'warning', label: 'dudosa', tone: 'doubt' }
  if (r.status === 'probable')
    return { icon: 'warning', label: 'dudosa, no la añadí', tone: 'doubt' }
  if (r.status === 'missing') return { icon: 'close', label: 'falta', tone: 'missing' }
  return { icon: 'close', label: 'no es una canción', tone: 'muted' }
}

const counts = computed(() => {
  const out = { ok: 0, doubt: 0, missing: 0 }
  for (const r of s.value.rows) {
    const t = mark(r).tone
    if (t in out) out[t]++
  }
  return out
})

const head = computed(() => {
  const x = s.value
  if (x.mode === 'check') return 'Comprobada, sin cambios'
  if (x.mode === 'set' && x.applied) return 'Dejada solo con estas'
  if (x.playlist?.created) return 'Lista nueva'
  return x.applied ? 'Añadidas' : 'Sin cambios'
})

const actions = computed(() => chat.sheetActions(props.msg))
const busy = computed(() => chat.sheetBusy.has(props.msg))

// ---------------------------------------------- clics con la tabla quieta
const root = useTemplateRef('root')
const still = ref(Date.now())
const moved = () => (still.value = Date.now())
function steady(fn) {
  if (Date.now() - still.value < CLICK_GUARD_MS) return
  fn()
}
// cada valor por separado: un array nuevo en cada vuelta «cambiaría» siempre
watch(
  [
    () => actions.value.keep,
    () => actions.value.order,
    () => actions.value.undo,
    open,
    () => chat.messages.value.length,
    () => chat.thinking.value,
    () => chat.live.value?.text?.length || 0
  ],
  moved
)
let observer = null
let thread = null
onMounted(() => {
  const el = root.value
  if (!el) return
  if (typeof ResizeObserver !== 'undefined') {
    observer = new ResizeObserver(moved)
    observer.observe(el)
  }
  thread = el.closest('.chat-thread')
  thread?.addEventListener('scroll', moved, { passive: true })
  window.addEventListener('resize', moved)
})
onUnmounted(() => {
  observer?.disconnect()
  thread?.removeEventListener('scroll', moved)
  window.removeEventListener('resize', moved)
})
</script>

<template>
  <section ref="root" class="chat-card cs" :aria-label="'La lista «' + name + '»'">
    <header class="cs-head">
      <Icon n="list" :t="14" />
      <strong class="cs-name">«{{ name }}»</strong>
      <span class="cs-mode">{{ head }}</span>
      <span class="cs-counts">
        <span v-if="counts.ok" class="cs-count ok">{{ counts.ok }} ✓</span>
        <span v-if="counts.doubt" class="cs-count doubt"
          >{{ counts.doubt }} dudosa{{ counts.doubt > 1 ? 's' : '' }}</span
        >
        <span v-if="counts.missing" class="cs-count missing"
          >falta{{ counts.missing > 1 ? 'n' : '' }} {{ counts.missing }}</span
        >
      </span>
    </header>

    <ol :id="tableId" class="cs-rows">
      <li v-for="(r, k) in rows" :key="k" class="cs-row" :class="'is-' + mark(r).tone">
        <span class="cs-n mono">{{ r.n ?? '' }}</span>
        <span class="cs-mark"
          ><Icon :n="mark(r).icon" :t="12" /><span class="cs-label">{{ mark(r).label }}</span></span
        >
        <span class="cs-main">
          <span class="cs-asked">{{ r.title }}</span>
          <span v-if="r.song" class="cs-found">→ {{ songName(r.song) }}</span>
          <span v-if="r.alts?.length" class="cs-alts"
            >también: {{ r.alts.map(songName).join(' · ') }}</span
          >
          <span v-if="r.note" class="cs-note">{{ r.note }}</span>
        </span>
      </li>
    </ol>
    <button
      v-if="long"
      type="button"
      class="chat-more"
      :aria-expanded="open ? 'true' : 'false'"
      :aria-controls="tableId"
      @click="open = !open"
    >
      {{ open ? 'Ver menos' : `Ver las ${s.rows.length}` }}
    </button>

    <p v-if="s.extras_in_list?.length" class="cs-extra">
      En «{{ name }}» hay {{ s.extras_in_list.length }} de otras veces:
      {{ s.extras_in_list.map(songName).join(' · ') }}
    </p>
    <p v-if="s.removed?.length" class="cs-extra">
      Quité: {{ s.removed.map(songName).join(' · ') }}
    </p>
    <p v-if="s.ignored?.length" class="cs-extra">
      No tomé por canción: {{ s.ignored.join(' · ') }}
    </p>
    <p v-if="s.truncated" class="cs-extra">Leí las primeras {{ s.rows.length }}.</p>

    <div v-if="actions.keep || actions.order || actions.undo" class="cs-actions">
      <button
        v-if="actions.keep"
        type="button"
        class="btn mini"
        :disabled="busy"
        @click="steady(() => chat.fixList(msg, 'keep'))"
      >
        Dejar solo estas
      </button>
      <button
        v-if="actions.order"
        type="button"
        class="btn mini"
        :disabled="busy"
        @click="steady(() => chat.fixList(msg, 'order'))"
      >
        Ordenar como mi lista
      </button>
      <button
        v-if="actions.undo"
        type="button"
        class="btn mini"
        :disabled="busy"
        @click="steady(() => chat.fixList(msg, 'undo'))"
      >
        Deshacer
      </button>
    </div>
  </section>
</template>
