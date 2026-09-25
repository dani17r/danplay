<script setup>
/**
 * La primera pantalla, y la de «no hay nada que enseñar».
 *
 * Tres casos, con la misma forma de salir (elegir una carpeta):
 * - todavía no hay ninguna carpeta de música;
 * - las carpetas de siempre ya no están donde estaban (`missing`): se
 *   movieron, o es un disco sin montar. Elegir la carpeta en su sitio nuevo
 *   la vuelve a enlazar (el núcleo la reconoce) y todo vuelve como estaba;
 * - las carpetas están, pero no queda ninguna canción (`empty`).
 *
 * Estaba metida dentro de App.vue como 60 líneas de plantilla sueltas.
 */
import { computed, ref } from 'vue'
import { pickFolder } from '../api.js'
import { addFolder } from '../utils/folders.js'
import { notify } from '../composables/useNotices.js'
import Icon from './Icon.vue'
import Card from './ui/Card.vue'
import TextField from './ui/TextField.vue'
import JobProgress from './ui/JobProgress.vue'

const props = defineProps({
  /** Carpetas gestionadas que ya no están donde estaban. */
  missing: { type: Array, default: () => [] },
  /** Hay carpetas, pero ninguna canción en ellas. */
  empty: { type: Boolean, default: false }
})
const emit = defineEmits(['ready'])

const path = ref('')
const preparing = ref(false)
const notice = ref(null)
/** Cómo va el análisis de la carpeta, según el núcleo. */
const job = ref(null)

const moved = computed(() => props.missing.length > 0)
const quoted = computed(() => props.missing.map((p) => `«${p}»`).join(', '))

async function browse() {
  const chosen = await pickFolder(moved.value ? '¿Dónde está ahora tu música?' : undefined)
  if (chosen) {
    path.value = chosen
    await add()
  }
}

async function add(force = false) {
  if (!path.value.trim()) return
  preparing.value = true
  notice.value = null
  job.value = null
  try {
    const r = await addFolder(path.value, { force, onProgress: (j) => (job.value = j) })
    if (r.action === 'confirm' || r.action === 'error') {
      notice.value = r
      return
    }
    // añadida pero sin analizar: que se vea aqui, no solo en un aviso que se va
    if (r.scanError) notice.value = r
    notify(r.message, r.kind)
    path.value = ''
    emit('ready')
  } finally {
    preparing.value = false
  }
}
</script>

<template>
  <div class="page" style="display: flex; align-items: center; justify-content: center">
    <div style="max-width: 560px; text-align: center">
      <div
        style="display: flex; justify-content: center; margin-bottom: 14px; color: var(--accent)"
      >
        <Icon :n="moved ? 'folderOpen' : 'music'" :t="46" />
      </div>
      <template v-if="moved">
        <h2 style="font-size: 22px">No encuentro tu música</h2>
        <div class="desc" style="margin-bottom: 22px">
          {{ missing.length > 1 ? 'Las carpetas' : 'La carpeta' }}
          <b class="missing-folders" style="overflow-wrap: anywhere">{{ quoted }}</b>
          {{ missing.length > 1 ? 'ya no están donde estaban.' : 'ya no está donde estaba.' }}
          Si la moviste, dime dónde está ahora: vuelve todo como estaba, con tus listas, estrellas y
          notas. Si es un disco, en cuanto lo conectes aparecerá sola.
        </div>
      </template>
      <template v-else-if="empty">
        <h2 style="font-size: 22px">Tu biblioteca está vacía</h2>
        <div class="desc" style="margin-bottom: 22px">
          No queda ninguna canción en tus carpetas. Si moviste tu música a otro sitio, elígelo aquí;
          si la estás copiando, irá apareciendo sola.
        </div>
      </template>
      <template v-else>
        <h2 style="font-size: 22px">Bienvenido a DanPlay</h2>
        <div class="desc" style="margin-bottom: 22px">
          Todavía no hay ninguna carpeta de música. Elige una o varias y DanPlay las analizará:
          identifica los temas, limpia los nombres y los organiza por artista.
        </div>
      </template>
      <Card
        style="text-align: left"
        :title="moved ? '¿Dónde está ahora?' : 'Elegir carpeta de música'"
        :note="
          moved
            ? 'O elige otra carpeta cualquiera.'
            : 'Puedes añadir más carpetas después, en Ajustes.'
        "
      >
        <div style="display: flex; gap: 8px">
          <button class="btn primary" :disabled="preparing" style="gap: 7px" @click="browse">
            <Icon n="folderOpen" :t="15" />
            {{ preparing ? 'Analizando…' : 'Examinar…' }}
          </button>
          <TextField
            v-model="path"
            width="100%"
            icon="folder"
            placeholder="o escribe la ruta:  /home/usuario/Musica"
            aria-label="Ruta de la carpeta de música"
            @enter="add()"
          />
          <button class="btn" :disabled="preparing || !path.trim()" @click="add()">Analizar</button>
        </div>
        <div
          v-if="notice"
          class="hint"
          :style="{ color: notice.confirmable ? 'var(--amber)' : 'var(--muted)' }"
        >
          {{ notice.message }}
          <div v-if="notice.confirmable" class="btn-row" style="margin-top: 8px">
            <button class="btn mini" @click="add(true)">Añadir igualmente</button>
            <button class="btn mini" @click="notice = null">Cancelar</button>
          </div>
        </div>
        <template v-if="preparing">
          <div class="hint">
            Leyendo etiquetas y construyendo el índice. Puede tardar un poco la primera vez.
          </div>
          <JobProgress :job="job" label="analizando tu música…" />
        </template>
      </Card>
      <div style="font-size: 12px; color: var(--muted2); margin-top: 16px">
        Nada se mueve ni se renombra sin que tú lo pidas.
      </div>
    </div>
  </div>
</template>
