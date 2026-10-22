<script setup>
/**
 * Dialogo propio, para no usar los `prompt()` y `confirm()` del navegador:
 * esos ignoran el tema, se ven de otra epoca y no se pueden estilar.
 *
 * Sirve de dos maneras: preguntar un texto (`kind="prompt"`) o pedir
 * confirmacion (`kind="confirm"`).
 */
import { ref, watch, onUnmounted, useId, useTemplateRef } from 'vue'
import { useFocusTrap } from '../../composables/useFocusTrap.js'
import Icon from '../Icon.vue'
import TextField from './TextField.vue'
import CheckField from './CheckField.vue'

const props = defineProps({
  open: Boolean,
  kind: { type: String, default: 'confirm' }, // prompt | confirm
  title: { type: String, default: '' },
  message: { type: String, default: '' },
  detail: { type: String, default: '' },
  value: { type: String, default: '' },
  placeholder: { type: String, default: '' },
  okLabel: { type: String, default: 'Aceptar' },
  cancelLabel: { type: String, default: 'Cancelar' },
  danger: Boolean,
  // Una casilla debajo del mensaje ({label, value}): al aceptar se devuelve
  // `{checked}` en vez de `true`.
  check: { type: Object, default: null },
  // Para un prompt que pide una carpeta: «Elegir…» abre el selector del
  // sistema (esta funcion) y lo elegido va al campo, que se puede retocar.
  browse: { type: Function, default: null }
})
const emit = defineEmits(['ok', 'cancel'])

const text = ref('')
const checked = ref(false)
const box = useTemplateRef('box')
const titleId = useId()
const messageId = useId()

// Mientras esta abierto, el foco no se escapa a lo de detras (Tab da la
// vuelta dentro) y al cerrarse vuelve a donde estaba. Al abrir va al campo,
// para poder escribir y pulsar Enter sin tocar el raton; sin campo, al boton
// principal, para aceptar o cancelar a teclas.
// (la casilla, si la hay, no: se marca a propósito, no por llegar)
useFocusTrap(box, {
  active: () => props.open,
  initial: 'input:not([type="checkbox"]), .modal-actions .btn:last-child'
})

// Las teclas se escuchan en el documento MIENTRAS esta abierto. Estaban
// puestas en el div del fondo, que no recibe el foco: en un dialogo de
// confirmacion no hay ningun campo donde escribir, asi que el foco se
// quedaba fuera y ni Escape cerraba ni Enter aceptaba. Solo funcionaba en
// los que preguntan un texto, porque ahi el foco cae en el campo.
watch(
  () => props.open,
  (v) => {
    if (!v) {
      document.removeEventListener('keydown', onKeydown)
      return
    }
    document.addEventListener('keydown', onKeydown)
    text.value = props.value || ''
    checked.value = !!props.check?.value
  },
  { immediate: true }
) // por si nace ya abierto

onUnmounted(() => document.removeEventListener('keydown', onKeydown))

const canAccept = () => props.kind !== 'prompt' || !!text.value.trim()

async function onBrowse() {
  const path = await props.browse?.()
  if (path) text.value = path
}

function accept() {
  if (!canAccept()) return
  if (props.kind === 'prompt') emit('ok', text.value.trim())
  else emit('ok', props.check ? { checked: checked.value } : true)
}
function onKeydown(e) {
  if (e.key === 'Escape') {
    e.preventDefault()
    emit('cancel')
    return
  }
  if (e.key !== 'Enter' || props.kind === 'prompt') return
  // Enter con el foco en un boton es pulsar ESE boton, y eso ya lo hace el
  // navegador. Aceptar aqui ademas hacia que Enter sobre «Cancelar» mandara
  // la cancion a la papelera o borrara la lista.
  if (e.target instanceof Element && e.target.closest('button')) return
  e.preventDefault()
  accept()
}
</script>

<template>
  <transition name="fade">
    <div v-if="open" class="modal-back" role="presentation" @mousedown.self="emit('cancel')">
      <div
        ref="box"
        class="modal"
        role="dialog"
        aria-modal="true"
        :aria-labelledby="title ? titleId : undefined"
        :aria-describedby="message ? messageId : undefined"
      >
        <h3 :id="titleId" class="modal-title">
          <Icon v-if="danger" n="warning" :t="16" style="color: var(--amber)" />
          {{ title }}
        </h3>
        <div v-if="message" :id="messageId" class="modal-msg">{{ message }}</div>

        <div v-if="kind === 'prompt'" class="modal-field">
          <TextField
            v-model="text"
            width="100%"
            :placeholder="placeholder"
            :aria-label="title || placeholder"
            @enter="accept"
          />
          <button v-if="browse" type="button" class="btn" @click="onBrowse">
            <Icon n="folderOpen" :t="14" /> Elegir…
          </button>
        </div>

        <div v-if="detail" class="modal-detail">{{ detail }}</div>

        <CheckField v-if="check" v-model="checked" class="modal-check">
          {{ check.label }}
        </CheckField>

        <div class="btn-row modal-actions">
          <button type="button" class="btn" @click="emit('cancel')">{{ cancelLabel }}</button>
          <button
            type="button"
            class="btn"
            :class="danger ? 'danger' : 'primary'"
            :disabled="!canAccept()"
            @click="accept"
          >
            {{ okLabel }}
          </button>
        </div>
      </div>
    </div>
  </transition>
</template>
