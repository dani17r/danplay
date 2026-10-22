<script setup>
/**
 * La tarjeta de permiso para descargar, EN LÍNEA bajo la respuesta que la
 * trae (antes era un diálogo que paraba el chat hasta contestarlo).
 *
 * Lo pedido junto a lo encontrado en YouTube, fila a fila, según llega: el
 * núcleo busca en segundo plano y aquí se pregunta cada poco mientras busca.
 * Casillas e insignias tal como las da el servidor (lo dudoso sale sin
 * marcar); «ver» abre SOLO la dirección canónica del vídeo. «Descargar» acepta
 * ESTA propuesta por su id con las casillas marcadas; «No», que no. Mientras
 * tanto se puede seguir escribiendo, y un «sí» o un «no» a secas la contestan
 * igual (useChat).
 *
 * Los botones no hacen caso en el medio segundo que sigue a un cambio de sitio
 * o de contenido (una fila que llega, un mensaje nuevo que la empuja): que no
 * se acepte nada por un clic que iba a otro sitio. Sin foco automático ni
 * atajo de teclado, por lo mismo.
 */
import {
  ref,
  computed,
  watch,
  onMounted,
  onActivated,
  onDeactivated,
  onUnmounted,
  useTemplateRef
} from 'vue'
import { app } from '../api.js'
import { useChat, PLAN_POLL_MS, CLICK_GUARD_MS, canonicalVideo } from '../composables/useChat.js'
import Icon from './Icon.vue'
import Loading from './ui/Loading.vue'

const props = defineProps({
  /** el mensaje del asistente que trae la tarjeta (`msg.confirm`) */
  msg: { type: Object, required: true }
})

const chat = useChat()
const c = computed(() => props.msg.confirm)
const state = computed(() => chat.cardState(props.msg))
const live = computed(() => state.value === 'live')
const busy = computed(() => !!chat.cards[c.value.id]?.busy)
const plan = computed(() => c.value.plan || null)
const rows = computed(() => plan.value?.items || [])
const searching = computed(() => (plan.value?.state || c.value.plan_state) === 'buscando')
const picked = computed(() => rows.value.filter((r) => chat.isPicked(props.msg, r)).length)
const canDownload = computed(
  () => live.value && !busy.value && (!rows.value.length || picked.value > 0)
)
const effect = computed(() => plan.value?.effect || c.value.effect || '')

// lo que ya estaba en la biblioteca: no se ofrece, pero se dice
const SHOWN_ALREADY = 6
const already = computed(() =>
  (plan.value?.already || []).map((a) =>
    a.song ? (a.song.artist ? `${a.song.artist} - ${a.song.title}` : a.song.title) : a.title
  )
)
const alreadyText = computed(() => {
  const names = already.value
  if (names.length <= SHOWN_ALREADY) return names.join(' · ')
  return `${names.slice(0, SHOWN_ALREADY).join(' · ')} y ${names.length - SHOWN_ALREADY} más`
})

/** Cómo acabó, en palabras (con botones solo si sigue en pie). */
const STATUS = {
  checking: 'Comprobando si sigue en pie…',
  accepted: 'Aceptada.',
  done: 'Aceptada y descargada.',
  declined: 'No la quisiste: no se bajó nada.',
  superseded: 'Hay una propuesta más nueva: usa esa.',
  expired: 'Caducada.'
}

/** 245 → «4:05». @param {number} s */
function mmss(s) {
  const n = Math.max(0, Math.round(s || 0))
  return `${Math.floor(n / 60)}:${String(n % 60).padStart(2, '0')}`
}

/** «ver»: solo la dirección canónica de un vídeo, en el navegador del sistema. */
function see(url) {
  const safe = canonicalVideo(url)
  if (safe) app.openInBrowser(safe)
}

// ---------------------------------------------- clics con la tarjeta quieta
const root = useTemplateRef('root')
const still = ref(Date.now())
const moved = () => (still.value = Date.now())
/** Hace `fn` solo si la tarjeta lleva medio segundo sin moverse ni cambiar. */
function steady(fn) {
  if (Date.now() - still.value < CLICK_GUARD_MS) return
  fn()
}
// lo que la mueve o la cambia: sus filas (lo que manda el servidor, no tus
// casillas), su estado y lo que entra en el hilo debajo o encima
// (cada valor por separado: un getter que devuelve un array nuevo «cambia»
// siempre, y cada consulta del plan volvía a bloquear los botones)
watch(
  [
    () => rows.value.map((r) => `${r.state}|${r.title}|${r.checked}|${r.badge}`).join('/'),
    state,
    () => already.value.length,
    effect,
    () => chat.messages.value.length,
    () => chat.thinking.value,
    () => chat.live.value?.text?.length || 0,
    () => !!chat.downloading.value
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

// ----------------------------------------------- las filas, según llegan
// Se pregunta cada poco mientras busca, y se deja al acabar, al salir de la
// página o si la tarjeta ya no está en pie.
let timer = null
let polling = false
let misses = 0
const wantsRows = () =>
  live.value && !chat.cards[c.value.id]?.noPlan && (!plan.value || searching.value)

function schedule(ms) {
  clearTimeout(timer)
  timer = setTimeout(tick, ms)
}
async function tick() {
  timer = null
  if (!polling || !wantsRows()) return
  const p = await chat.refreshPlan(props.msg)
  if (!polling) return
  misses = p ? 0 : misses + 1
  if ((p ? p.state === 'buscando' : misses < 5) && wantsRows()) schedule(PLAN_POLL_MS)
}
function startPolling() {
  polling = true
  if (wantsRows() && !timer) schedule(plan.value ? PLAN_POLL_MS : 0)
}
function stopPolling() {
  polling = false
  clearTimeout(timer)
  timer = null
}
onMounted(startPolling)
onActivated(startPolling)
onDeactivated(stopPolling)
onUnmounted(stopPolling)
// pasa a estar en pie (se acaba de comprobar) o deja de estarlo
watch(live, (on) => (on ? startPolling() : stopPolling()))
</script>

<template>
  <section
    ref="root"
    class="chat-card cc"
    :class="'is-' + state"
    :aria-label="'Permiso para descargar: ' + c.summary"
    :aria-busy="searching && live ? 'true' : 'false'"
  >
    <header class="cc-head">
      <Icon n="download" :t="14" />
      <strong class="cc-title">{{ c.summary }}</strong>
      <Loading v-if="searching && live" class="cc-searching" text="buscando en YouTube…" />
    </header>

    <ul v-if="rows.length" class="cc-rows">
      <li v-for="row in rows" :key="row.i" class="cc-row" :class="'is-' + row.state">
        <!-- la casilla con lo pedido y lo encontrado: todo es el botón (como las
             casillas de las listas: `pick-box`, sin el control nativo) -->
        <button
          type="button"
          class="cc-pick"
          role="checkbox"
          :aria-checked="chat.isPicked(msg, row) ? 'true' : 'false'"
          :disabled="!live || busy || row.state === 'rechazada'"
          @click="chat.togglePick(msg, row)"
        >
          <span class="pick-box" :class="{ on: chat.isPicked(msg, row) }" aria-hidden="true">
            <Icon v-if="chat.isPicked(msg, row)" n="check" :t="11" :weight="2.4" />
          </span>
          <span class="cc-what">
            <span class="cc-asked"
              ><span v-if="row.n" class="cc-n">{{ row.n }}.</span> {{ row.query }}</span
            >
            <span v-if="row.state === 'buscando' && live" class="cc-found cc-wait">buscando…</span>
            <span v-else-if="row.title" class="cc-found"
              >→ {{ row.title }}<span v-if="row.channel" class="cc-meta"> · {{ row.channel }}</span
              ><span v-if="row.duration" class="cc-meta mono">
                · {{ mmss(row.duration) }}</span
              ></span
            >
            <span v-if="row.note" class="cc-note">{{ row.note }}</span>
          </span>
        </button>
        <span v-if="row.badge" class="cc-badge">{{ row.badge }}</span>
        <button
          v-if="canonicalVideo(row.url)"
          type="button"
          class="cc-see"
          :aria-label="'Ver en YouTube: ' + (row.title || row.query)"
          @click="see(row.url)"
        >
          ver <Icon n="external" :t="11" />
        </button>
      </li>
    </ul>

    <p v-if="already.length" class="cc-already">
      <Icon n="check" :t="11" />
      <span
        >Ya {{ already.length === 1 ? 'estaba' : 'estaban' }} en tu biblioteca:
        {{ alreadyText }}</span
      >
    </p>
    <p v-if="plan?.trimmed" class="cc-note">
      Te propongo {{ rows.length }} de {{ rows.length + plan.trimmed }}; después te propongo el
      resto.
    </p>
    <p v-if="plan?.reason && !searching" class="cc-note">{{ plan.reason }}</p>
    <!-- lo que pasará al terminar: solo mientras puede pasar -->
    <p v-if="effect && ['live', 'checking', 'accepted'].includes(state)" class="cc-effect">
      {{ effect }}
    </p>

    <div v-if="live" class="cc-actions">
      <button
        type="button"
        class="btn primary mini"
        :disabled="!canDownload"
        @click="steady(() => chat.acceptCard(msg))"
      >
        <Icon n="download" :t="12" />
        {{ rows.length ? `Descargar (${picked})` : 'Descargar' }}
      </button>
      <button
        type="button"
        class="btn mini"
        :disabled="busy"
        @click="steady(() => chat.declineCard(msg))"
      >
        No
      </button>
      <button v-if="searching" type="button" class="cc-link" @click="chat.stopSearch(msg)">
        Dejar de buscar
      </button>
    </div>
    <p v-else class="cc-status" role="status">
      {{ STATUS[state] || '' }}
      <button
        v-if="state === 'expired'"
        type="button"
        class="cc-link"
        @click="steady(() => chat.askAgain())"
      >
        Volver a pedirlo
      </button>
    </p>
  </section>
</template>
