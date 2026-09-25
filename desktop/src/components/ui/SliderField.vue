<script setup>
/**
 * Un deslizador con la parte recorrida pintada.
 *
 * Mientras se arrastra, lo que se ve es lo que hay bajo el dedo, no lo que
 * diga el modelo. El volumen va a Rust y vuelve como estado un momento
 * despues; si ese eco (con un valor ya viejo) se volvia a escribir en el
 * <input> a mitad de arrastre, la bola saltaba hacia atras y no habia forma
 * de llevarla con precision. Al soltar se sigue ignorando el modelo un
 * momento, hasta que confirme lo ultimo que se mando.
 */
import { computed, ref, watch } from 'vue'
const model = defineModel({ type: Number, default: 0 })
const props = defineProps({
  min: { type: Number, default: 0 },
  max: { type: Number, default: 1 },
  step: { type: Number, default: 0.01 },
  label: { type: String, default: '' },
  width: { type: String, default: '' },
  valueText: { type: String, default: '' },
  /** el nombre que se lee cuando no hay etiqueta a la vista (el volumen) */
  ariaLabel: { type: String, default: '' },
  /**
   * Una muesca en ese valor, y lo que pase de ahí en otro color: el 100 %
   * de un volumen que puede subir más de como viene.
   */
  mark: { type: Number, default: null }
})

// Cuanto se espera, tras soltar, a que el modelo confirme lo ultimo enviado.
const GRACE_MS = 800

const shown = ref(model.value) // lo que pinta el <input>
const dragging = ref(false)
let sent = null // lo ultimo que se emitio, y cuando
watch(model, (v) => {
  if (dragging.value) return
  if (sent && Date.now() < sent.until && Math.abs(v - sent.value) > props.step / 2) return
  sent = null
  shown.value = v
})

function onInput(e) {
  shown.value = Number(e.target.value)
  sent = { value: shown.value, until: Date.now() + GRACE_MS }
  model.value = shown.value
}
function grab() {
  dragging.value = true
}
function drop() {
  dragging.value = false
  // si el modelo ya iba por delante (nadie mando nada), se vuelve a el
  if (!sent) shown.value = model.value
}

const toPct = (v) => Math.max(0, Math.min(100, ((v - props.min) / (props.max - props.min)) * 100))
const pct = computed(() => toPct(shown.value))
const markPct = computed(() => (props.mark == null ? null : toPct(props.mark)))
/** Lo que pasa de la muesca: se pinta aparte. */
const over = computed(() =>
  markPct.value != null && pct.value > markPct.value ? pct.value - markPct.value : 0
)
</script>

<template>
  <label class="slider" :style="width ? { width: width } : null">
    <span v-if="label" class="field-label">
      {{ label }}<em v-if="valueText">{{ valueText }}</em></span
    >
    <span
      class="slider-track"
      :style="{ '--pct': (markPct == null ? pct : Math.min(pct, markPct)) + '%' }"
    >
      <span
        v-if="markPct != null"
        class="slider-mark"
        :style="{ left: markPct + '%' }"
        aria-hidden="true"
      ></span>
      <span
        v-if="over"
        class="slider-over"
        :style="{ left: markPct + '%', width: over + '%' }"
        aria-hidden="true"
      ></span>
      <input
        type="range"
        :min="min"
        :max="max"
        :step="step"
        :value="shown"
        :aria-label="label ? undefined : ariaLabel || undefined"
        :aria-valuetext="valueText || undefined"
        @input="onInput"
        @pointerdown="grab"
        @pointerup="drop"
        @pointercancel="drop"
        @keydown="grab"
        @keyup="drop"
        @blur="drop"
      />
    </span>
  </label>
</template>
