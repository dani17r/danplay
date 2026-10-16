<script setup>
/**
 * La ventana de los términos y condiciones.
 *
 * Sale sola la primera vez que se abre DanPlay, y cada vez que se quiere
 * separar en pistas, usar el asistente o «Transformar» sin haberlos aceptado
 * (ver useTerms.js). Para aceptarlos hay que bajar hasta el final del texto:
 * hasta entonces la casilla no se puede marcar, y sin la casilla «Aceptar»
 * no hace nada.
 */
import { ref, watch, nextTick, onUnmounted, useId, useTemplateRef } from 'vue'
import Icon from './Icon.vue'
import CheckField from './ui/CheckField.vue'
import { useTerms } from '../composables/useTerms.js'
import { useFocusTrap } from '../composables/useFocusTrap.js'
import { notify } from '../composables/useNotices.js'
import { errorMessage } from '../api.js'
import { AUTHOR, TERMS, TERMS_VERSION } from '../data/terminos.js'

const terms = useTerms()
const { state } = terms

const box = useTemplateRef('box')
const reader = useTemplateRef('reader')
const titleId = useId()
/** se llegó al final del texto */
const read = ref(false)
const agreed = ref(false)
const saving = ref(false)

useFocusTrap(box, { active: () => state.open, initial: '.terms-reader' })

/** Al final, o sin nada que bajar (pantallas altas): ya se puede marcar. */
function check() {
  const el = reader.value
  if (!el) return
  if (el.scrollTop + el.clientHeight >= el.scrollHeight - 8) read.value = true
}

function keys(e) {
  if (e.key === 'Escape') {
    e.preventDefault()
    terms.close()
  }
}

watch(
  () => state.open,
  async (open) => {
    document.removeEventListener('keydown', keys)
    if (!open) return
    read.value = false
    agreed.value = false
    document.addEventListener('keydown', keys)
    await nextTick()
    if (reader.value) reader.value.scrollTop = 0
    check()
  },
  { immediate: true }
)
onUnmounted(() => document.removeEventListener('keydown', keys))

async function accept() {
  if (!read.value || !agreed.value || saving.value) return
  saving.value = true
  try {
    await terms.accept()
    notify('Términos aceptados. ¡Gracias!', 'ok', 4)
  } catch (e) {
    notify('No se pudieron guardar: ' + errorMessage(e))
  } finally {
    saving.value = false
  }
}
</script>

<template>
  <transition name="fade">
    <div v-if="state.open" class="modal-back terms-back" role="presentation">
      <div
        ref="box"
        class="modal terms-modal"
        role="dialog"
        aria-modal="true"
        :aria-labelledby="titleId"
      >
        <header class="terms-head">
          <h3 :id="titleId" class="modal-title">Términos y condiciones de DanPlay</h3>
          <p class="terms-by">
            DanPlay es obra de <strong>{{ AUTHOR.name }}</strong
            >. Versión de los términos: {{ TERMS_VERSION }}.
          </p>
          <p v-if="state.reason" class="terms-why">
            <Icon n="lockClosed" :t="14" /> Para {{ state.reason }} necesitas aceptar los términos.
          </p>
        </header>

        <!-- se baja con la rueda, el dedo o las flechas: por eso puede tener el foco -->
        <!-- eslint-disable-next-line vuejs-accessibility/no-noninteractive-tabindex -->
        <div
          ref="reader"
          class="terms-reader"
          tabindex="0"
          role="document"
          aria-label="El texto de los términos"
          @scroll="check"
        >
          <section v-for="sec in TERMS" :key="sec.title">
            <h4>{{ sec.title }}</h4>
            <p v-for="(p, i) in sec.body" :key="i">{{ p }}</p>
          </section>
          <p class="terms-end">— Fin de los términos —</p>
        </div>

        <CheckField v-model="agreed" class="terms-agree" :disabled="!read">
          He leído los términos y condiciones hasta el final y los acepto.
        </CheckField>
        <p v-if="!read" class="terms-hint">Baja hasta el final del texto para poder aceptarlos.</p>

        <div class="btn-row modal-actions">
          <button type="button" class="btn" @click="terms.close()">Ahora no</button>
          <button
            type="button"
            class="btn primary"
            :disabled="!read || !agreed || saving"
            @click="accept"
          >
            Aceptar
          </button>
        </div>
      </div>
    </div>
  </transition>
</template>
