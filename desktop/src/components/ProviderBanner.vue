<script setup>
/**
 * La franja bajo la cabecera del chat cuando la IA elegida no contesta o la
 * última respuesta la dio un respaldo.
 *
 * Antes solo lo decía una línea gris diminuta bajo cada respuesta, y durante
 * días contestó un modelo de respaldo sin que se notara (la cuenta del
 * elegido no tenía saldo). La frase la compone la interfaz por el TIPO de
 * fallo (saldo, clave, saturado, caído), nunca con el texto del proveedor. Y
 * ningún ajuste cambia sin un clic: «Usar … como principal» además pregunta
 * antes (useChat.promoteFallback).
 *
 * En una ventana baja se queda en una línea y sus botones se abren aparte.
 */
import { ref, computed } from 'vue'
import Icon from './Icon.vue'

const props = defineProps({
  /** {primary: {name, model}, problem: {kind, code, at}|null, answering: {id, name, model, reason}|null} */
  state: { type: Object, required: true }
})
const emit = defineEmits(['settings', 'recheck', 'promote'])

const open = ref(false)

/** Por qué no contesta, en palabras (por el tipo de fallo). */
const KIND = {
  billing: 'no tiene saldo',
  auth: 'no acepta la clave',
  rate: 'pide esperar (demasiadas peticiones)',
  down: 'no responde',
  tools: 'tiene un modelo que no sabe usar herramientas'
}

/** «Qwen/Qwen3-Next-80B-A3B-Instruct» → «Qwen3-Next-80B-A3B-Instruct». */
const shortModel = (m) =>
  String(m || '')
    .split('/')
    .pop()

const backup = computed(() => {
  const a = props.state.answering
  return a ? `${a.name}${a.model ? ' · ' + shortModel(a.model) : ''}` : ''
})

const text = computed(() => {
  const { primary, problem, answering } = props.state
  const who = primary?.name || 'La IA elegida'
  const why = KIND[problem?.kind || answering?.reason] || 'no contesta'
  if (problem && answering) return `${who} ${why}; contesta ${backup.value} (respaldo).`
  if (problem) return `${who} ${why}.`
  return `${who} ${why}; contestó ${backup.value} (respaldo).`
})
</script>

<template>
  <section class="chat-banner" :class="{ open }" aria-label="Estado de la IA">
    <Icon n="warning" :t="14" />
    <p class="chat-banner-text" role="status" :title="text">{{ text }}</p>
    <button
      type="button"
      class="chat-banner-fold field-btn"
      :aria-expanded="open ? 'true' : 'false'"
      aria-controls="chat-banner-actions"
      :title="open ? 'Ocultar las opciones' : 'Ver las opciones'"
      @click="open = !open"
    >
      <Icon n="down" :t="12" />
      <span class="sr-only">Opciones</span>
    </button>
    <div id="chat-banner-actions" class="chat-banner-actions">
      <button type="button" class="btn mini" @click="emit('settings')">
        <Icon n="settings" :t="12" /> Ajustes de IA
      </button>
      <button type="button" class="btn mini" @click="emit('recheck')">
        <Icon n="refresh" :t="12" /> Reintentar con el principal
      </button>
      <button v-if="state.answering?.id" type="button" class="btn mini" @click="emit('promote')">
        Usar {{ backup }} como principal
      </button>
    </div>
  </section>
</template>
