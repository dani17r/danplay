<script setup>
/**
 * La caja de escribir del asistente.
 *
 * Un textarea y no un campo de una línea: la lista del domingo se pega desde
 * WhatsApp con sus saltos de línea, y el campo de antes los borraba (la
 * lista llegaba en una sola tira) y además Enter la partía en dos mensajes,
 * el primero de los cuales se contestaba solo.
 *
 * - Enter envía; Shift+Enter baja de línea; Ctrl/Cmd+Enter también envía.
 * - Mientras se compone con un método de entrada (IME), Enter es del IME.
 * - Crece con lo escrito hasta unas 8 líneas (nunca más del 35 % del alto de
 *   la ventana) y de ahí en adelante se desplaza por dentro. Al enviar,
 *   vuelve a una línea.
 * - Si el asistente está respondiendo, lo que mandas queda en cola: un chip
 *   por mensaje, con su × para quitarlo antes de que salga.
 *
 * No sabe nada de la conversación: recibe el texto (v-model), si está
 * respondiendo y la cola, y avisa con eventos. La conversación vive en
 * `useChat`.
 */
import {
  computed,
  nextTick,
  onActivated,
  onMounted,
  onUnmounted,
  useId,
  useTemplateRef,
  watch
} from 'vue'
import Icon from './Icon.vue'

const model = defineModel({ type: String, default: '' })
const props = defineProps({
  /** El asistente está respondiendo: Enter deja el mensaje en cola. */
  thinking: Boolean,
  /** Los mensajes que esperan su turno: `{id, text}`. */
  queue: { type: Array, default: () => [] },
  /** Cuántos caben en la cola. */
  queueMax: { type: Number, default: 3 },
  maxlength: { type: Number, default: 50000 }
})
const emit = defineEmits(['send', 'stop', 'unqueue', 'resize'])

const uid = useId()
const helpId = `${uid}-ayuda`
const queueId = `${uid}-cola`
const input = useTemplateRef('input')
const root = useTemplateRef('root')

const empty = computed(() => !model.value.trim())
const full = computed(() => props.thinking && props.queue.length >= props.queueMax)

/** Lo que dice el botón de enviar: con el asistente respondiendo, no envía aún. */
const sendLabel = computed(() =>
  full.value
    ? `Ya hay ${props.queueMax} mensajes en cola`
    : props.thinking
      ? 'Dejar en cola'
      : 'Enviar'
)

/**
 * Un mensaje en cola, en corto: sus primeras 40 letras y cuántas líneas
 * más trae. Por letras y no por unidades de UTF-16, para no partir un emoji.
 * @param {string} text
 */
function preview(text) {
  const rows = String(text)
    .split('\n')
    .map((l) => l.trim())
    .filter(Boolean)
  const first = Array.from(rows[0] || '')
  const head = first.length > 40 ? first.slice(0, 40).join('').trimEnd() + '…' : first.join('')
  const more = rows.length - 1
  return more > 0 ? `${head} (+${more} ${more === 1 ? 'línea' : 'líneas'})` : head
}

/** @param {KeyboardEvent} e */
function onKeydown(e) {
  if (e.key !== 'Enter') return
  // componiendo con un IME (japonés, chino, acentos muertos en algunos
  // sistemas): ese Enter confirma la composición, no envía
  if (e.isComposing || e.keyCode === 229) return
  const mod = e.ctrlKey || e.metaKey
  // Shift+Enter (y Alt+Enter): una línea nueva, la pone el navegador
  if (!mod && (e.shiftKey || e.altKey)) return
  e.preventDefault()
  // mantener Enter pulsado no manda una ráfaga
  if (e.repeat) return
  submit()
}

function submit() {
  if (empty.value) return
  emit('send')
}

function sendClick() {
  submit()
  // el foco vuelve a la caja: lo normal después de enviar es seguir escribiendo
  input.value?.focus()
}

/**
 * La altura justa para lo escrito. El tope (8 líneas o el 35 % del alto) lo
 * pone el CSS con `max-height`; pasado el tope, se desplaza por dentro.
 */
function fit() {
  const el = input.value
  // guardada por <KeepAlive> o escondida: ahí no se puede medir nada, y
  // medir daría 0 y la dejaría sin alto al volver
  if (!el || !el.isConnected) return
  // Mientras se mide, la caja de fuera conserva su alto: si no, el hilo de
  // arriba crecía un instante, perdía su desplazamiento y, al volver, lo
  // último de la conversación quedaba tapado.
  const box = el.parentElement
  if (box) box.style.minHeight = `${box.offsetHeight}px`
  el.style.overflowY = 'hidden'
  el.style.height = 'auto'
  const needed = el.scrollHeight
  if (needed > 0) {
    el.style.height = `${needed}px`
    // solo con barra cuando de verdad no cabe
    el.style.overflowY = el.clientHeight < needed ? 'auto' : ''
  } else {
    el.style.height = ''
    el.style.overflowY = ''
  }
  if (box) box.style.minHeight = ''
}

// lo que cambia el texto sin teclear (enviar lo vacía, un fallo lo devuelve)
watch(model, () => nextTick(fit))

// Cuando la caja crece (más líneas, chips de la cola), el hilo encoge por
// abajo; quien la usa decide si sigue mostrando lo último.
let lastHeight = 0
/** @type {ResizeObserver | null} */
let observer = null

onMounted(() => {
  fit()
  window.addEventListener('resize', fit, { passive: true })
  if (typeof ResizeObserver !== 'undefined' && root.value) {
    lastHeight = root.value.offsetHeight
    observer = new ResizeObserver(() => {
      const h = root.value?.offsetHeight || 0
      if (h && lastHeight && h !== lastHeight) emit('resize', h - lastHeight)
      lastHeight = h
    })
    observer.observe(root.value)
  }
})
onActivated(() => nextTick(fit))
onUnmounted(() => {
  window.removeEventListener('resize', fit)
  observer?.disconnect()
})

defineExpose({ focus: () => input.value?.focus() })
</script>

<template>
  <div ref="root" class="composer">
    <!-- Lo que espera su turno mientras el asistente responde -->
    <div v-if="queue.length" class="composer-queue">
      <span :id="queueId" class="composer-queue-label">En cola</span>
      <ul class="composer-chips" :aria-labelledby="queueId">
        <li v-for="q in queue" :key="q.id" class="chip composer-chip" :title="q.text.slice(0, 400)">
          <span class="composer-chip-txt">{{ preview(q.text) }}</span>
          <button
            type="button"
            class="field-btn composer-chip-x"
            title="Quitar de la cola"
            :aria-label="'Quitar de la cola: ' + preview(q.text)"
            @click="emit('unqueue', q.id)"
          >
            <Icon n="close" :t="11" />
          </button>
        </li>
      </ul>
    </div>
    <span class="sr-only" role="status">{{
      queue.length
        ? queue.length === 1
          ? 'Un mensaje en cola: sale al acabar la respuesta'
          : `${queue.length} mensajes en cola: salen al acabar la respuesta`
        : ''
    }}</span>

    <div class="composer-row">
      <div class="field composer-field" :class="{ 'composer-full': full }">
        <div class="field-box composer-box">
          <textarea
            ref="input"
            v-model="model"
            class="composer-input"
            rows="1"
            :maxlength="maxlength"
            placeholder="Escribe lo que necesites…"
            aria-label="Mensaje para el asistente"
            aria-keyshortcuts="Enter Control+Enter Meta+Enter"
            :aria-describedby="helpId"
            spellcheck="true"
            @keydown="onKeydown"
            @input="fit"
          ></textarea>
        </div>
      </div>
      <!-- `next` es el icono de «canción siguiente» del reproductor; aquí lo
           que se hace es enviar, no saltar de pista. -->
      <button
        class="btn primary composer-send"
        type="button"
        :title="sendLabel"
        :aria-label="sendLabel"
        :disabled="empty || full"
        @click="sendClick"
      >
        <Icon n="send" :t="15" />
      </button>
      <button
        v-if="thinking"
        class="btn chat-cancel"
        type="button"
        title="Parar la respuesta"
        aria-label="Parar la respuesta"
        @click="emit('stop')"
      >
        <Icon n="close" :t="14" /> <span class="composer-stop-txt">Parar</span>
      </button>
    </div>

    <p :id="helpId" class="composer-help">
      <span class="composer-keys"
        ><kbd>Enter</kbd> {{ thinking ? 'lo deja en cola' : 'envía' }} ·
        <kbd>Shift+Enter</kbd> nueva línea · admite markdown</span
      >
      <span v-if="model.length > maxlength * 0.9" class="composer-count mono"
        >{{ model.length.toLocaleString('es') }} / {{ maxlength.toLocaleString('es') }}</span
      >
    </p>
  </div>
</template>
