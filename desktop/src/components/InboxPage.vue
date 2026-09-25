<script setup>
/**
 * La Entrada: el buzón de la biblioteca.
 *
 * Estaba dentro de App.vue. Aquí lleva su propio estado, que no lo necesita
 * nadie más.
 */
import { ref } from 'vue'
import { api, errorMessage } from '../api.js'
import { notify } from '../composables/useNotices.js'
import Icon from './Icon.vue'
import Card from './ui/Card.vue'
import Loading from './ui/Loading.vue'

defineProps({
  /** Cuántos archivos esperan. Lo sabe App, que ya consulta el estado. */
  waiting: { type: Number, default: 0 }
})
const emit = defineEmits(['changed', 'go'])

const working = ref(false)
const result = ref(null)

async function run(dryRun) {
  working.value = true
  try {
    result.value = (await api.runImport({ dry_run: dryRun })).results
    emit('changed')
  } catch (e) {
    notify('No se pudo importar: ' + errorMessage(e))
  } finally {
    working.value = false
  }
}

const counted = (action) => (result.value || []).filter((r) => r.action === action).length
const ACTION_LABEL = { moved: 'archivada', review: 'a revisar', dry_run: 'sería así' }
</script>

<template>
  <div class="page">
    <h2>Entrada</h2>
    <div class="desc">
      Es el buzón de tu biblioteca: una carpeta donde dejas música suelta y DanPlay la ordena por
      ti. Lo que hay aquí todavía <strong>no forma parte de tu biblioteca</strong> ni sale en las
      búsquedas hasta que lo importas.
    </div>

    <Card title="Cómo funciona">
      <ol class="steps">
        <li>
          <strong>Sueltas</strong> los archivos en <code>~/Musica/Entrada</code>, o los bajas desde
          <button class="link" type="button" @click="emit('go', { kind: 'downloads' })">
            Descargas</button
          >.
        </li>
        <li>
          DanPlay <strong>averigua de quién son</strong>: primero por las etiquetas, luego por cómo
          suena, y si hace falta pregunta a la IA.
        </li>
        <li>
          <strong>Limpia el nombre</strong> (fuera «VIDEO OFICIAL», acentos y mayúsculas sostenidas)
          y lo deja en <code>Artistas/&lt;Artista&gt;/</code>.
        </li>
        <li>
          Lo que no logra identificar va a <code>Revisar/</code>.
          <strong>Nunca se inventa un artista.</strong>
        </li>
      </ol>
    </Card>

    <div class="card">
      <div class="path-row">
        <Icon n="inbox" :t="15" />
        <span class="path">~/Musica/Entrada</span>
        <span class="badge" :class="waiting ? '' : 'ok'">
          {{ waiting ? waiting + ' esperando' : 'vacía' }}</span
        >
      </div>

      <div v-if="!waiting" class="hint">
        Nada pendiente. Cuando dejes archivos ahí aparecerán aquí.
      </div>

      <div class="btn-row" style="margin-top: 12px">
        <button class="btn primary" :disabled="working || !waiting" @click="run(false)">
          <Icon n="check" :t="15" />
          Importar {{ waiting }} {{ waiting === 1 ? 'archivo' : 'archivos' }}
        </button>
        <button class="btn" :disabled="working || !waiting" @click="run(true)">
          Probar sin tocar nada
        </button>
        <Loading v-if="working" text="trabajando…" />
      </div>
      <div v-if="waiting" class="hint">
        «Probar sin tocar nada» te enseña dónde acabaría cada archivo, sin moverlo ni renombrarlo.
      </div>
    </div>

    <Card v-if="result" title="Resultado">
      <div class="note">
        {{ counted('moved') }} archivadas · {{ counted('review') }} a revisar ·
        {{ counted('dry_run') }} en prueba
      </div>
      <div v-for="(r, i) in result" :key="i" class="import-row">
        <div style="display: flex; gap: 8px; align-items: center; flex-wrap: wrap">
          <span class="badge" :class="r.action === 'review' ? 'bad' : 'ok'">
            {{ ACTION_LABEL[r.action] || r.action }}</span
          >
          <span
            v-if="r.source"
            class="mono"
            style="font-size: 11px; color: var(--muted2)"
            title="Cómo se identificó"
          >
            {{ r.source }} {{ r.confidence?.toFixed(2) }}
          </span>
          <strong>{{ r.artist ? `${r.artist} — ${r.title}` : r.target || r.source_path }}</strong>
        </div>
        <div v-if="r.target" class="dl-path">{{ r.target }}</div>
        <div v-for="a in r.warnings" :key="a" class="hint">! {{ a }}</div>
      </div>
    </Card>
  </div>
</template>
