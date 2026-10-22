<script setup>
/**
 * Las carpetas del teléfono que tienen música, para elegir cuáles salen en
 * «Música del teléfono» (como las carpetas de la PC). Se buscan solas; las
 * de Música y Descargas vienen elegidas.
 */
import { ref, computed, onMounted } from 'vue'
import { api, errorMessage } from '../api.js'
import { notify } from '../composables/useNotices.js'
import Icon from '../components/Icon.vue'
import Loading from '../components/ui/Loading.vue'
import { syncPhoneMedia } from './phoneMusic.js'

const emit = defineEmits(['done'])

const folders = ref(
  /** @type {Array<{path: string, name: string, songs: number, on: boolean}>} */ ([])
)
const loading = ref(true)
const saving = ref(false)

onMounted(async () => {
  try {
    await syncPhoneMedia()
    folders.value = (await api.phoneFolders())?.folders || []
  } catch (e) {
    notify('No se pudieron leer las carpetas: ' + errorMessage(e))
  } finally {
    loading.value = false
  }
})

const chosen = computed(() => folders.value.filter((f) => f.on))
const songs = computed(() => chosen.value.reduce((a, f) => a + f.songs, 0))

function toggle(f) {
  f.on = !f.on
}
function all(on) {
  for (const f of folders.value) f.on = on
}
async function save() {
  saving.value = true
  try {
    const r = await api.setPhoneFolders(chosen.value.map((f) => f.path))
    emit('done', r?.songs ?? 0)
  } catch (e) {
    notify('No se pudieron guardar las carpetas: ' + errorMessage(e))
  } finally {
    saving.value = false
  }
}
</script>

<template>
  <div class="m-folders">
    <p class="m-folders-lead">
      Las carpetas del teléfono con música. Elige las que quieres ver en «Música del teléfono».
    </p>
    <Loading v-if="loading" text="Buscando música en el teléfono…" />
    <p v-else-if="!folders.length" class="m-folders-lead">
      No hay música en el teléfono (o DanPlay no tiene permiso para verla).
    </p>
    <template v-else>
      <div class="m-folders-bulk">
        <button type="button" class="m-chip-btn" @click="all(true)">Todas</button>
        <button type="button" class="m-chip-btn" @click="all(false)">Ninguna</button>
      </div>
      <div class="m-folders-list">
        <button
          v-for="f in folders"
          :key="f.path"
          type="button"
          role="checkbox"
          class="m-folder"
          :class="{ on: f.on }"
          :aria-checked="f.on"
          @click="toggle(f)"
        >
          <Icon :n="f.on ? 'check' : 'folderOpen'" :t="18" />
          <span class="m-folder-name">{{ f.name || 'La memoria del teléfono' }}</span>
          <em>{{ f.songs }}</em>
        </button>
      </div>
    </template>
    <footer class="m-folders-foot">
      <span class="m-folders-count"
        >{{ songs }} {{ songs === 1 ? 'canción' : 'canciones' }} en {{ chosen.length }}
        {{ chosen.length === 1 ? 'carpeta' : 'carpetas' }}</span
      >
      <button
        type="button"
        class="m-chip-btn m-primary"
        :disabled="saving || loading"
        @click="save"
      >
        <Icon n="check" :t="16" /> {{ saving ? 'Leyendo…' : 'Listo' }}
      </button>
    </footer>
  </div>
</template>
