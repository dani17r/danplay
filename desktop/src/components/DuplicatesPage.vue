<script setup>
/**
 * Duplicados: los grupos que el núcleo encuentra, para decidir con qué copia
 * quedarse. Estaba dentro de App.vue.
 *
 * Borrar va siempre a la papelera del sistema y solo dentro de las carpetas
 * de la biblioteca; el núcleo lo comprueba otra vez por su cuenta.
 */
import { ref, computed, onMounted } from 'vue'
import { api, errorMessage } from '../api.js'
import { usePlayback } from '../composables/usePlayback.js'
import { notify } from '../composables/useNotices.js'
import { ask } from '../composables/useDialog.js'
import Card from './ui/Card.vue'
import Loading from './ui/Loading.vue'
import DuplicateGroup from './DuplicateGroup.vue'

const emit = defineEmits(['changed'])

const player = usePlayback()
const groups = ref(null)
const resolving = ref(false)
const playingId = computed(() => player.track.value?.id ?? null)

async function load() {
  groups.value = null
  try {
    groups.value = await api.duplicates()
  } catch (e) {
    notify('No se pudo analizar: ' + errorMessage(e))
    groups.value = { identical: [], similar: [] }
  }
}
onMounted(load)

/** Suena una de las copias sin salir de la página. */
async function playCopy(item) {
  if (!item.id) return
  const song = await api.song(item.id)
  player.setQueue([song], song.id, { kind: 'duplicates', label: 'Duplicados' })
}

/** Se queda con una copia y manda las demás a la papelera. */
async function keepOne(group, chosen) {
  const others = group.items.filter((t) => t.path !== chosen.path)
  const ok = await ask({
    kind: 'confirm',
    title: 'Conservar solo esta',
    danger: true,
    message: `Se queda:\n  ${chosen.file}\n\nY se borra${others.length > 1 ? 'n' : ''}:`,
    detail: others.map((t) => t.file).join('\n'),
    okLabel: 'Conservar solo esta'
  })
  if (!ok) return
  resolving.value = true
  try {
    const r = await api.resolveDuplicate(
      chosen.path,
      others.map((t) => t.path)
    )
    if (!r.ok) {
      notify(
        'No se pudo: ' + (r.reason || (r.failures || []).map((f) => f.reason).join(', ')),
        'info'
      )
      return
    }
    notify(r.renamed ? `Guardada como «${r.final_name}»` : `Conservada «${r.final_name}»`, 'ok')
    if (playingId.value && others.some((o) => o.id === playingId.value)) player.stop()
    await load()
    emit('changed')
  } catch (e) {
    notify('No se pudo: ' + errorMessage(e))
  } finally {
    resolving.value = false
  }
}
</script>

<template>
  <div class="page">
    <h2>Duplicados</h2>
    <div class="desc">
      Escúchalas y quédate con la que prefieras. Al conservar una, las demás van a la papelera y si
      la elegida llevaba el sufijo « - r» se le quita.
    </div>
    <Loading v-if="!groups" text="analizando tu biblioteca…" />
    <template v-else>
      <Card
        v-if="groups.identical.length"
        :title="`Idénticos byte a byte (${groups.identical.length})`"
        note="Son el mismo archivo: puedes quedarte con cualquiera sin escuchar."
      >
        <DuplicateGroup
          v-for="(g, i) in groups.identical"
          :key="'i' + i"
          :group="g"
          :playing="playingId"
          :busy="resolving"
          @play="playCopy"
          @keep-one="keepOne"
        />
      </Card>
      <Card
        v-if="groups.similar.length"
        :title="`Misma canción, archivo distinto (${groups.similar.length})`"
        note="Pueden ser versiones diferentes. Escucha antes de decidir."
      >
        <DuplicateGroup
          v-for="(g, i) in groups.similar"
          :key="'p' + i"
          :group="g"
          :playing="playingId"
          :busy="resolving"
          @play="playCopy"
          @keep-one="keepOne"
        />
      </Card>
      <div v-if="!groups.identical.length && !groups.similar.length" class="card">
        Sin duplicados.
      </div>
    </template>
  </div>
</template>
