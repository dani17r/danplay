<script setup>
/**
 * Dialogo propio, para no usar los `prompt()` y `confirm()` del navegador:
 * esos ignoran el tema, se ven de otra epoca y no se pueden estilar.
 *
 * Sirve de dos maneras: preguntar un texto (`kind="prompt"`) o pedir
 * confirmacion (`kind="confirm"`).
 */
import { ref, watch, nextTick } from 'vue'
import Icon from '../Icon.vue'
import TextField from './TextField.vue'

const props = defineProps({
  open: Boolean,
  kind: { type: String, default: 'confirm' },   // prompt | confirm
  title: String,
  message: String,
  detail: String,
  value: { type: String, default: '' },
  placeholder: String,
  okLabel: { type: String, default: 'Aceptar' },
  cancelLabel: { type: String, default: 'Cancelar' },
  danger: Boolean
})
const emit = defineEmits(['ok', 'cancel'])

const text = ref('')
const box = ref(null)

watch(() => props.open, async (v) => {
  if (!v) return
  text.value = props.value || ''
  await nextTick()
  // el foco al campo, para poder escribir y pulsar Enter sin tocar el raton
  box.value?.querySelector('input')?.focus()
  box.value?.querySelector('input')?.select()
})

const puedeAceptar = () => props.kind !== 'prompt' || !!text.value.trim()

function aceptar () {
  if (!puedeAceptar()) return
  emit('ok', props.kind === 'prompt' ? text.value.trim() : true)
}
function teclas (e) {
  if (e.key === 'Escape') { e.preventDefault(); emit('cancel') }
  if (e.key === 'Enter' && props.kind !== 'prompt') { e.preventDefault(); aceptar() }
}
</script>

<template>
  <transition name="fade">
    <div v-if="open" class="modal-back" @mousedown.self="emit('cancel')" @keydown="teclas">
      <div class="modal" ref="box" role="dialog" aria-modal="true">
        <h3 class="modal-title">
          <Icon v-if="danger" n="warning" :t="16" style="color:var(--amber)" />
          {{ title }}
        </h3>
        <div v-if="message" class="modal-msg">{{ message }}</div>

        <TextField v-if="kind === 'prompt'" v-model="text" width="100%"
                   :placeholder="placeholder" :limpiable="false" @enter="aceptar" />

        <div v-if="detail" class="modal-detail">{{ detail }}</div>

        <div class="btn-row modal-actions">
          <button class="btn" @click="emit('cancel')">{{ cancelLabel }}</button>
          <button class="btn" :class="danger ? 'danger' : 'primary'"
                  :disabled="!puedeAceptar()" @click="aceptar">{{ okLabel }}</button>
        </div>
      </div>
    </div>
  </transition>
</template>
