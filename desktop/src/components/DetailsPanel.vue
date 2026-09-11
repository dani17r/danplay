<script setup>
import { ref, watch, computed, onMounted, onUnmounted } from 'vue'
import { notify } from '../composables/useNotices.js'
import { api, pickImage } from '../api.js'
import StarRating from './StarRating.vue'
import Icon from './Icon.vue'
import CoverArt from './ui/CoverArt.vue'
import CopyButton from './ui/CopyButton.vue'
import EmptyState from './ui/EmptyState.vue'
import Loading from './ui/Loading.vue'
import SelectField from './ui/SelectField.vue'
import TextField from './ui/TextField.vue'
import SyncedLyrics from './ui/SyncedLyrics.vue'
import { parseLrc, stripLrc } from '../utils/lrc.js'
import { usePlayback } from '../composables/usePlayback.js'

const props = defineProps(['song', 'aiReady'])
// «blur» a secas no: es el nombre de un evento nativo del DOM y se presta
// a confusion con el foco.
const emit = defineEmits(['updated', 'goSettings', 'toggleBlur'])

const details = ref(null)
const loading = ref('')
const failure = ref('')
// La portada se sirve siempre en la misma URL, asi que tras buscarla el
// navegador reusaba la cacheada (o la que dio 404). Subir esto la recarga.
const coverVersion = ref(0)

// Un boton que ya no hace falta estorba: si la cancion tiene letra, no se
// ofrece buscarla. Lo mismo con la portada y con la ficha.
const FIELD_LABELS = { album: 'album', year: 'año', genre: 'genero', key: 'tono' }
const needsLyrics = computed(() => !!props.song && !props.song.lyrics)

// La letra con tiempos: la de LRCLIB (`lyrics_synced`) o, si el mp3 traia
// un LRC en su USLT, la propia `lyrics`. Con la cancion sonando se sigue
// linea a linea; la letra plana se enseña sin las marcas.
const player = usePlayback()
const lrcLines = computed(() => parseLrc(props.song?.lyrics_synced) || parseLrc(props.song?.lyrics))
const plainLyrics = computed(() => (props.song?.lyrics_synced ? props.song.lyrics : stripLrc(props.song?.lyrics)) || '')
const isPlayingThis = computed(() => !!props.song && player.track.value?.id === props.song.id)
const follow = ref(true)
const needsCover = computed(() => !!props.song && !props.song.cover)
const missingInfo = computed(() => props.song
  ? Object.keys(FIELD_LABELS).filter(k => !String(props.song[k] ?? '').trim())
  : [])
// Cuando la IA no puede completar, el boton no desaparece: se queda en ambar
// y deshabilitado, con el motivo debajo.
const blocked = ref(null)
const missingText = computed(() =>
  missingInfo.value.map(k => FIELD_LABELS[k]).join(', '))

watch(() => props.song?.id, () => { blocked.value = null; cancelEdit() })

// ---------------------------------------------------------------- edicion
// Todo lo editable se guarda DENTRO del mp3 (etiquetas ID3), no en la base:
// la base es solo un indice reconstruible.
const EDITABLE = ['title', 'artist', 'album', 'year', 'genre', 'feat',
                  'key', 'bpm', 'lyrics']
const editing = ref(false)
const draft = ref({})
const saving = ref(false)

function snapshot () {
  const s = props.song || {}
  return Object.fromEntries(EDITABLE.map(k => [k, s[k] == null ? '' : String(s[k])]))
}

/** Solo lo que de verdad cambio: no se reescriben etiquetas intactas. */
const changes = computed(() => {
  if (!editing.value) return {}
  const base = snapshot()
  return Object.fromEntries(
    EDITABLE.filter(k => (draft.value[k] ?? '') !== base[k])
            .map(k => [k, draft.value[k] ?? '']))
})
const dirty = computed(() => Object.keys(changes.value).length > 0)

function startEdit () {
  draft.value = snapshot()
  editing.value = true
  failure.value = ''
}
function cancelEdit () {
  editing.value = false
  draft.value = {}
}

async function saveEdit () {
  if (!dirty.value) return cancelEdit()
  saving.value = true; failure.value = ''
  try {
    const payload = { ...changes.value }
    if ('bpm' in payload) payload.bpm = Number(payload.bpm) || 0
    emit('updated', await api.edit(props.song.id, payload))
    const n = Object.keys(payload).length
    notify(`${n} ${n === 1 ? 'campo guardado' : 'campos guardados'} en el archivo`, 'ok')
    cancelEdit()
  } catch (e) {
    failure.value = String(e).replace(/^Error:\s*/, '')
  } finally { saving.value = false }
}

/** Escape cancela; Ctrl+Enter guarda. */
function onEditKeys (e) {
  if (!editing.value) return
  if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); cancelEdit() }
  else if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) { e.preventDefault(); saveEdit() }
}
onMounted(() => window.addEventListener('keydown', onEditKeys, true))
onUnmounted(() => window.removeEventListener('keydown', onEditKeys, true))

async function changeCover () {
  const path = await pickImage()
  if (!path) return
  loading.value = 'cover'; failure.value = ''
  try {
    const r = await api.setCover(props.song.id, path)
    emit('updated', r.song)
    coverVersion.value++
    notify(`Caratula cambiada (${r.kb} KB)`, 'ok')
  } catch (e) {
    failure.value = String(e).replace(/^Error:\s*/, '')
  } finally { loading.value = '' }
}

async function autofill () {
  if (!props.aiReady) {
    blocked.value = { reason: 'Hace falta configurar la IA en Ajustes para completar la ficha.' }
    return
  }
  loading.value = 'autofill'; failure.value = ''; blocked.value = null
  try {
    const r = await api.autofill(props.song.id)
    if (r.song) emit('updated', r.song)
    if (r.missing?.length) {
      blocked.value = { reason: r.reason || 'La IA no pudo completar toda la ficha.' }
    }
    const n = Object.keys(r.filled || {}).length
    notify(n ? `Ficha completada: ${Object.keys(r.filled)
      .map(k => FIELD_LABELS[k] || k).join(', ')}`
      : (r.reason || 'No se pudo completar nada'), n ? 'ok' : 'info')
  } catch (e) {
    failure.value = String(e).replace(/^Error:\s*/, '')
  } finally { loading.value = '' }
}
const tonoDestino = ref('')
const transpuesto = ref(null)
const TONOS = ['C','C#','D','D#','E','F','F#','G','G#','A','A#','B']

watch(() => props.song?.id, () => {
  details.value = null; transpuesto.value = null; tonoDestino.value = ''
  failure.value = ''; coverVersion.value = 0
})

const acordesJson = computed(() => {
  if (details.value) return details.value
  if (!props.song?.chords) return null
  try { return JSON.parse(props.song.chords) } catch { return null }
})

const progression = computed(() =>
  transpuesto.value?.progression ?? acordesJson.value?.progression ?? '')
/** Los acordes en texto plano, con sus secciones, listos para pegar. */
const acordesParaCopiar = computed(() => {
  const trozos = []
  if (progression.value) trozos.push(progression.value)
  for (const [k, v] of Object.entries(secciones.value || {})) trozos.push(`${k}: ${v}`)
  return trozos.join('\n')
})

const secciones = computed(() =>
  transpuesto.value?.section_chords ?? acordesJson.value?.section_chords ?? null)

async function loadDetails () {
  loading.value = 'details'; failure.value = ''
  try {
    const r = await api.details(props.song.id)
    details.value = r.details
    if (r.details?.error) failure.value = r.details.error
  } catch (e) {
    failure.value = String(e).replace(/^Error:\s*/, '')
  } finally { loading.value = '' }
}

/** Antes esto se tragaba los fallos: se quedaba igual y no decia por que. */
async function enrich (opts) {
  loading.value = 'enrich'; failure.value = ''
  const antes = { lyrics: !!props.song.lyrics, cover: !!props.song.cover }
  try {
    const r = await api.enrich(props.song.id, opts)
    emit('updated', r.song)
    if (opts.cover) coverVersion.value++      // obliga a pedirla otra vez
    const hecho = []
    if (opts.lyrics) hecho.push(r.song?.lyrics && !antes.lyrics ? 'letra encontrada'
                                : r.song?.lyrics ? 'ya tenia letra' : 'no se encontro letra')
    if (opts.cover) hecho.push(r.result?.cover ? 'portada encontrada' : 'no se encontro portada')
    if (hecho.length) notify(hecho.join(' · '),
                           hecho.some(h => h.includes('encontrada')) ? 'ok' : 'info')
  } catch (e) {
    failure.value = String(e).replace(/^Error:\s*/, '')
  } finally { loading.value = '' }
}
async function transponer () {
  const base = acordesJson.value
  if (!base || !tonoDestino.value) return
  const r = await api.transpose({
    text: base.progression || '',
    from_key: base.likely_key || props.song.key,
    to_key: tonoDestino.value
  })
  const sec = {}
  for (const [k, v] of Object.entries(base.section_chords || {})) {
    sec[k] = (await api.transpose({ text: v, from_key: base.likely_key,
                                    to_key: tonoDestino.value })).text
  }
  transpuesto.value = { progression: r.text, section_chords: sec, capo: r.capo }
}
const fmtDuration = (s) => s ? `${Math.floor(s/60)}:${String(Math.floor(s%60)).padStart(2,'0')}` : '—'
</script>

<template>
  <aside class="details" v-if="song">
    <div class="details-head">
      <div class="cover-wrap">
        <CoverArt :id="song.id" :version="coverVersion" :blur="!!song.blur"
                  class="cover" :icon-size="40" :alt="song.title || song.file" />
        <!-- Difuminar no toca el archivo: la imagen sigue entera y se puede
             volver a ver cuando se quiera. Por eso esta siempre a mano y no
             escondido tras el modo de edicion. -->
        <button class="cover-blur" :title="song.blur
                  ? 'Ver la portada como es' : 'Difuminar esta portada'"
                @click="emit('toggleBlur', song)">
          <Icon :n="song.blur ? 'eye' : 'eyeOff'" :t="14" />
        </button>
        <button v-if="editing" class="cover-change" :disabled="loading === 'cover'"
                title="Elegir una imagen del disco" @click="changeCover">
          <Icon n="image" :t="15" />
          <span>{{ loading === 'cover' ? 'Poniendo…' : 'Cambiar' }}</span>
        </button>
      </div>

      <div class="details-titlebar">
        <div style="flex:1;min-width:0">
          <div class="copiable">
            <div class="details-title">{{ song.title || song.file }}</div>
            <CopyButton :text="song.title || song.file" what="el titulo"
                        @copied="ok => notify(ok ? 'Titulo copiado'
                          : 'No se pudo copiar', ok ? 'ok' : 'info')" />
          </div>
          <div class="copiable">
            <div class="details-artist">{{ song.artist || 'Artista sin identificar' }}</div>
            <CopyButton v-if="song.artist" :text="song.artist" what="el artista"
                        @copied="ok => notify(ok ? 'Artista copiado'
                          : 'No se pudo copiar', ok ? 'ok' : 'info')" />
          </div>
          <div v-if="song.feat && !editing" class="details-artist" style="font-size:12px">
            feat. {{ song.feat }}</div>
        </div>
        <button v-if="!editing" class="icon-btn" title="Editar la ficha (se guarda en el mp3)"
                @click="startEdit"><Icon n="pencil" :t="14" /></button>
      </div>
      <div style="margin-top:10px;display:flex;align-items:center;gap:12px">
        <StarRating :value="song.stars||0" :t="19"
                   @change="n=>api.setStars(song.id,n).then(c=>emit('updated',c))" />
        <span class="heart" :class="{on:song.favorite}"
              :title="song.favorite ? 'Quitar de favoritos' : 'Marcar como favorito'"
              @click="api.toggleFavorite(song.id,!song.favorite).then(c=>emit('updated',c))">
          <Icon :n="song.favorite ? 'heartFull' : 'heart'" :t="19" /></span>
      </div>
    </div>

    <dl v-if="!editing" class="facts">
      <dt>Album</dt><dd>{{ song.album || '—' }}</dd>
      <dt>Año</dt><dd>{{ song.year || '—' }}</dd>
      <dt>Genero</dt><dd>{{ song.genre || '—' }}</dd>
      <dt>Duracion</dt><dd class="mono">{{ fmtDuration(song.duration) }}</dd>
      <dt>Calidad</dt><dd class="mono">{{ song.bitrate ? Math.round(song.bitrate/1000)+' kbps' : '—' }}</dd>
      <dt>Tono</dt><dd class="mono">{{ song.key || '—' }}</dd>
      <dt>BPM</dt><dd class="mono">{{ song.bpm ? Math.round(song.bpm) : '—' }}</dd>
      <dt>Carpeta</dt><dd :title="song.folder">{{ song.folder }}</dd>
      <dt v-if="song.playlists?.length">Listas</dt>
      <dd v-if="song.playlists?.length">{{ song.playlists.join(', ') }}</dd>
    </dl>

    <!-- modo edicion: todo esto se escribe en las etiquetas del propio mp3 -->
    <form v-else class="edit-form" @submit.prevent="saveEdit">
      <TextField v-model="draft.title" label="Titulo" width="100%" />
      <TextField v-model="draft.artist" label="Artista" width="100%" />
      <TextField v-model="draft.feat" label="Colaboran" width="100%"
                 placeholder="separa con comas" />
      <TextField v-model="draft.album" label="Album" width="100%" />
      <div class="edit-row">
        <TextField v-model="draft.year" label="Año" width="100%" placeholder="2019" />
        <TextField v-model="draft.genre" label="Genero" width="100%" />
      </div>
      <div class="edit-row">
        <TextField v-model="draft.key" label="Tono" width="100%" placeholder="Bb" />
        <TextField v-model="draft.bpm" label="BPM" width="100%" type="number" />
      </div>
      <TextField v-model="draft.lyrics" label="Letra" width="100%" multiline :rows="10"
                 placeholder="Se guarda dentro del mp3" />

      <div class="edit-actions">
        <span class="edit-state">
          {{ dirty ? (Object.keys(changes).length + ' sin guardar') : 'Sin cambios' }}
        </span>
        <button type="button" class="btn mini" @click="cancelEdit">
          {{ dirty ? 'Descartar' : 'Cancelar' }}</button>
        <button v-if="dirty" type="submit" class="btn mini primary" :disabled="saving">
          {{ saving ? 'Guardando…' : 'Guardar' }}</button>
      </div>
      <div class="hint">Escape cancela · Ctrl+Enter guarda</div>
    </form>

    <div class="section">
      <div class="btn-row">
        <button v-if="needsLyrics" class="btn mini" :disabled="!!loading"
                title="Se busca en LRCLIB; si no esta, la completa la IA"
                @click="enrich({lyrics:true,cover:false,details:false})">
          Buscar letra</button>
        <button v-if="needsCover" class="btn mini" :disabled="!!loading"
                title="Se busca en iTunes y en Cover Art Archive"
                @click="enrich({lyrics:false,cover:true,details:false})">
          Buscar portada</button>
        <button v-if="missingInfo.length" class="btn mini"
                :class="{warn: !!blocked || !aiReady}"
                :disabled="!!loading || !!blocked"
                :title="blocked ? blocked.reason : 'Completa: ' + missingText"
                @click="autofill">
          Rellenar informacion con IA</button>
        <button class="btn mini" :disabled="!!loading || !aiReady"
                :title="aiReady ? 'Tono, acordes y contexto, con IA'
                                : 'Hace falta configurar la IA en Ajustes'"
                @click="loadDetails">
          Ver detalles IA</button>
      </div>
      <div v-if="blocked" class="hint warn-text">
        <Icon n="warning" :t="13" /> {{ blocked.reason }}
      </div>
      <div v-else-if="missingInfo.length" class="hint">
        Sin rellenar: {{ missingText }}
      </div>
      <Loading v-if="loading" text="consultando…" style="margin-top:9px" />
      <div v-else-if="!aiReady" class="hint" style="margin-top:8px">
        Sin IA configurada solo se busca en LRCLIB y en las caratulas publicas.
        <a class="link" @click="emit('goSettings')">Elegir la IA en Ajustes</a>
      </div>
      <div v-if="failure" class="hint" style="color:var(--red);font-style:normal">
        {{ failure }}</div>
    </div>

    <div class="section copiable-section" v-if="acordesJson">
      <h4>Acordes
        <span class="badge" v-if="acordesJson.confidence">
          confianza {{ Math.round(acordesJson.confidence*100) }}%</span>
        <CopyButton :text="acordesParaCopiar" what="los acordes" :size="14"
                    @copied="ok => notify(ok ? 'Acordes copiados'
                      : 'No se pudo copiar', ok ? 'ok' : 'info')" />
      </h4>
      <div v-if="progression" class="chords">{{ progression }}</div>
      <div v-if="secciones" style="margin-top:9px">
        <div v-for="(v,k) in secciones" :key="k" style="margin-bottom:7px">
          <div style="font-size:10px;text-transform:uppercase;color:var(--muted2);letter-spacing:1px">{{ k }}</div>
          <div class="chords">{{ v }}</div>
        </div>
      </div>
      <div style="margin-top:11px;display:flex;gap:7px;align-items:center">
        <span style="font-size:11px;color:var(--muted2)">Transponer a</span>
        <SelectField :modelValue="tonoDestino" width="112px"
                  :options="[{v:'',n:'—'}, ...TONOS.map(t => ({v:t, n:t}))]"
                  @update:modelValue="v => { tonoDestino = v; transponer() }" />
      </div>
      <div v-if="transpuesto?.capo?.length" class="hint">
        Cejilla: {{ transpuesto.capo.map(c=>`traste ${c[0]} con formas de ${c[1]}`).join(' · ') }}
      </div>
      <div class="hint" v-if="acordesJson.confidence < 0.7">
        Acordes aproximados, generados por IA. Verificalos antes de tocar.
      </div>
    </div>

    <div class="section" v-if="acordesJson?.involved_artists?.length">
      <h4>Artistas implicados</h4>
      <div style="display:flex;gap:6px;flex-wrap:wrap">
        <span class="chip" v-for="a in acordesJson.involved_artists" :key="a">{{ a }}</span>
      </div>
    </div>

    <div class="section" v-if="acordesJson?.about_the_song">
      <h4>Sobre la cancion</h4>
      <div style="font-size:12.5px;color:var(--muted);line-height:1.6">
        {{ acordesJson.about_the_song }}</div>
    </div>

    <div class="section copiable-section" v-if="song.lyrics || song.lyrics_synced">
      <h4>Letra
        <CopyButton :text="plainLyrics" what="la letra" :size="14"
                    @copied="ok => notify(ok ? 'Letra copiada'
                      : 'No se pudo copiar', ok ? 'ok' : 'info')" />
        <button v-if="lrcLines" class="btn mini lrc-toggle" type="button" :class="{on: follow}"
                :title="isPlayingThis ? 'La linea que suena, resaltada; pulsa una para ir ahi' : 'Con tiempos: al ponerla a sonar, sigue la letra'"
                @click="follow = !follow">
          <Icon n="play" :t="11" /> {{ follow ? 'Siguiendo' : 'Seguir la cancion' }}</button>
      </h4>
      <SyncedLyrics v-if="lrcLines && follow" :lines="lrcLines" :position="player.position.value"
                    :active="isPlayingThis" @seek="t => player.seek(t)" />
      <div v-else class="lyrics">{{ plainLyrics }}</div>
    </div>
  </aside>

  <aside class="details" v-else>
    <EmptyState icon="note" title="Selecciona una cancion"
                hint="Aqui veras su ficha, la letra y los acordes"
                style="padding-top:70px" />
  </aside>
</template>
