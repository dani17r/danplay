<script setup>
/**
 * La primera pantalla: todavía no hay ninguna carpeta de música.
 *
 * Estaba metida dentro de App.vue como 60 líneas de plantilla sueltas.
 */
import { ref } from 'vue'
import { pickFolder } from '../api.js'
import { addFolder } from '../utils/folders.js'
import { notify } from '../composables/useNotices.js'
import Icon from './Icon.vue'
import Card from './ui/Card.vue'
import TextField from './ui/TextField.vue'

const emit = defineEmits(['ready'])

const path = ref('')
const preparing = ref(false)
const notice = ref(null)

async function browse() {
  const chosen = await pickFolder()
  if (chosen) {
    path.value = chosen
    await add()
  }
}

async function add(force = false) {
  if (!path.value.trim()) return
  preparing.value = true
  notice.value = null
  try {
    const r = await addFolder(path.value, { force })
    if (r.action === 'confirm' || r.action === 'error') {
      notice.value = r
      return
    }
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
      <div style="display: flex; justify-content: center; margin-bottom: 14px; color: var(--accent)">
        <Icon n="music" :t="46" />
      </div>
      <h2 style="font-size: 22px">Bienvenido a DanPlay</h2>
      <div class="desc" style="margin-bottom: 22px">
        Todavía no hay ninguna carpeta de música. Elige una o varias y DanPlay las analizará:
        identifica los temas, limpia los nombres y los organiza por artista.
      </div>
      <Card
        style="text-align: left"
        title="Elegir carpeta de música"
        note="Puedes añadir más carpetas después, en Ajustes."
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
        <div v-if="preparing" class="hint">
          Leyendo etiquetas y construyendo el índice. Puede tardar un poco la primera vez.
        </div>
      </Card>
      <div style="font-size: 12px; color: var(--muted2); margin-top: 16px">
        Nada se mueve ni se renombra sin que tú lo pidas.
      </div>
    </div>
  </div>
</template>
