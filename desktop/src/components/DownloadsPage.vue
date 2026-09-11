<script setup>
import { ref, computed, onMounted, onUnmounted } from 'vue'
import { notify } from '../composables/useNotices.js'
import { useDownloads } from '../composables/useDownloads.js'
import { usePlayback } from '../composables/usePlayback.js'
import { ask } from '../composables/useDialog.js'
import { api, errorMessage } from '../api.js'
import Icon from './Icon.vue'
import TextField from './ui/TextField.vue'
import SelectField from './ui/SelectField.vue'
import ToggleField from './ui/ToggleField.vue'
import Card from './ui/Card.vue'

const emit = defineEmits(['reload'])

const query = ref('')
const quality = ref(localStorage.getItem('danplay.ytQuality') || 'high')
const fileIt = ref(localStorage.getItem('danplay.ytFileIt') !== '0')
const results = ref(Number(localStorage.getItem('danplay.ytResults') || 5))

// El estado de la descarga es de toda la app (lo comparte con el chat y con
// la barra lateral): aqui solo se lee y se arranca.
const downloads = useDownloads()
const status = downloads.state     // lo que devuelve /api/youtube
const preview = ref(null)          // lo que se bajaria
const looking = ref(false)
const error = ref('')
const askAgain = ref(null)         // la que ya tienes y preguntamos si bajar igual
const history = ref([])
const historyTotal = ref(0)
// El historial se trae por tandas: la lista tiene scroll a partir de una
// docena, y «Cargar mas» trae las siguientes. Paginar era mas botones para lo
// mismo.
const PAGE = 30

// Lo descargado se puede poner a sonar desde aqui mismo, y pararlo.
const player = usePlayback()
const playingId = computed(() => player.track.value?.id ?? null)
const historyIcon = (id) => (playingId.value === id && player.playing.value ? 'pause' : 'play')
const historyTitle = (id) =>
  playingId.value !== id ? 'Reproducir' : player.playing.value ? 'Pausar' : 'Reanudar'
async function playFromHistory (id) {
  if (!id) return
  if (playingId.value === id) return player.toggle()
  try {
    const song = await api.song(id)
    player.setQueue([song], song.id, { kind: 'downloads', label: 'Descargas' })
  } catch (e) {
    notify('No se pudo poner: ' + errorMessage(e))
  }
}

const SOURCE_LABEL = { manual: 'a mano', assistant: 'asistente' }
const when = (at) => {
  if (!at) return ''
  const d = new Date(at * 1000)
  const hoy = new Date()
  const mismoDia = d.toDateString() === hoy.toDateString()
  return mismoDia
    ? d.toLocaleTimeString('es', { hour: '2-digit', minute: '2-digit' })
    : d.toLocaleDateString('es', { day: '2-digit', month: 'short' }) + ' ' +
      d.toLocaleTimeString('es', { hour: '2-digit', minute: '2-digit' })
}

async function loadHistory (more = false) {
  try {
    const offset = more ? history.value.length : 0
    const r = await api.downloadHistory(PAGE, offset)
    history.value = more ? history.value.concat(r.items || []) : (r.items || [])
    historyTotal.value = r.total || 0
  } catch { /* el nucleo aun no responde */ }
}

// Como en un navegador: se puede vaciar, pero no mientras baja algo, que lo
// que esta entrando se apunta ahi al terminar.
async function clearHistory () {
  if (running.value) return
  const ok = await ask({
    kind: 'confirm', title: 'Vaciar el historial de descargas',
    message: 'Se borra la lista de lo descargado. Las canciones no se tocan.',
    okLabel: 'Vaciar'
  })
  if (!ok) return
  try {
    await api.clearDownloadHistory()
    await loadHistory()
    notify('Historial vaciado', 'ok')
  } catch (e) { notify('No se pudo vaciar: ' + errorMessage(e)) }
}
const QUALITIES = [
  { v: 'high', n: 'Alta', note: '320 kbps' },
  { v: 'medium', n: 'Media', note: '192 kbps' },
  { v: 'variable', n: 'Variable', note: 'VBR, la mejor relacion tamaño/calidad' }
]
const HOW_MANY = [1, 3, 5, 10, 20].map(n => ({ v: n, n: String(n) }))

// El nucleo publica `active`. Antes esto miraba `running`, que no existe, asi
// que la barra nunca salia y el boton nunca se bloqueaba.
const running = computed(() => !!status.active)
const ready = computed(() => !!status.available)
// mientras baja no se toca nada: ni el enlace, ni la calidad, ni el archivado
const locked = computed(() => running.value || !ready.value)

const isSearch = computed(() => {
  const t = query.value.trim().toLowerCase()
  return !!t && !t.startsWith('http://') && !t.startsWith('https://')
})

const done = computed(() => status.results || [])

const PHASES = {
  starting: 'Preparando', downloading: 'Bajando', converting: 'Convirtiendo a mp3',
  filing: 'Identificando y archivando', done: 'Terminado', canceled: 'Cancelado'
}
const tt = (s) => !s ? '—'
  : `${Math.floor(s / 60)}:${String(Math.floor(s % 60)).padStart(2, '0')}`

async function showPreview () {
  const q = query.value.trim()
  if (!q) return
  looking.value = true; error.value = ''; preview.value = null
  try {
    const r = await api.youtubeInfo(q, results.value)
    if (r.ok) preview.value = r
    else error.value = r.reason || 'no se encontro nada'
  } catch (e) {
    error.value = String(e).replace(/^Error:\s*/, '')
  } finally { looking.value = false }
}

/** `what` y `force` permiten relanzar una concreta como repetida. */
async function startDownload (what = null, force = false) {
  const q = (what ?? query.value).trim()
  if (!q || running.value) return
  error.value = ''; askAgain.value = null
  localStorage.setItem('danplay.ytQuality', quality.value)
  localStorage.setItem('danplay.ytFileIt', fileIt.value ? '1' : '0')
  localStorage.setItem('danplay.ytResults', String(results.value))
  try {
    await api.youtubeDownload({
      query: q, quality: quality.value, file_it: fileIt.value,
      results: force ? 1 : results.value, force
    })
    preview.value = null
    downloads.wake()       // que la barra aparezca al momento
  } catch (e) {
    error.value = String(e).replace(/^Error:\s*/, '')
  }
}

async function cancelDownload () {
  try { await api.youtubeCancel() } catch { /* da igual: ya habra terminado */ }
}

// Al terminar una descarga (esta o la del asistente): la biblioteca ha
// cambiado y hay que contar como fue.
const stopFollowing = downloads.onFinished((s) => {
  const results = s.results || []
  const ok = results.filter(r => r.ok).length
  const already = results.filter(r => r.already_there).length
  const failed = results.filter(r => !r.ok && !r.already_there).length
  if (ok) emit('reload')
  const partes = []
  if (ok) partes.push(`${ok} descargada${ok > 1 ? 's' : ''}`)
  if (already) partes.push(`${already} ya la tenias`)
  if (failed) partes.push(`${failed} sin suerte`)
  if (partes.length) notify(partes.join(' · '), ok ? 'ok' : 'info')
  // si hubo repetidas, se ofrece bajarlas igualmente
  const rep = results.filter(r => r.already_there && r.url)
  askAgain.value = rep.length ? rep : null
  loadHistory()          // el asistente tambien escribe aqui
})

onMounted(async () => {
  await downloads.refresh()
  await loadHistory()
})
onUnmounted(stopFollowing)
</script>

<template>
  <div class="page">
    <h2>Descargas</h2>
    <div class="desc">
      Pega el enlace de un video o de una lista de YouTube, o escribe lo que buscas.
      DanPlay baja el audio, lo pasa a mp3 con su caratula y lo archiva por artista
      igual que todo lo demas.
    </div>

    <div v-if="status.known && !ready" class="card">
      <h3>Falta una pieza</h3>
      <div class="note">{{ status.reason }}</div>
      <div class="hint">Mientras tanto el resto de DanPlay funciona igual.</div>
    </div>

    <Card title="Que quieres bajar">
      <div class="note">Un enlace trae exactamente ese video o esa lista.
        Si escribes texto suelto, se busca en YouTube.</div>

      <TextField v-model="query" width="100%" icon="search" :disabled="locked"
                 placeholder="https://youtu.be/…  ·  o:  barak sera llena la tierra"
                 @enter="startDownload()" />

      <div class="dl-options">
        <SelectField v-model="quality" label="Calidad" :options="QUALITIES"
                     :disabled="locked" width="220px" />
        <SelectField v-if="isSearch" v-model="results" label="Cuantos traer"
                     :options="HOW_MANY" :disabled="locked" width="120px" />
      </div>

      <ToggleField v-model="fileIt" :disabled="locked"
                   title="Archivar al terminar"
                   hint="Identifica la cancion, limpia el nombre y la deja en Artistas/.
                         Si lo apagas, se queda en Entrada/ para que la revises tu." />

      <div class="btn-row" style="margin-top:12px">
        <button class="btn primary" :disabled="locked || !query.trim()"
                @click="startDownload()" style="gap:7px">
          <Icon n="download" :t="15" />
          {{ running ? 'Bajando…' : 'Descargar' }}</button>
        <button class="btn" :disabled="locked || !query.trim() || looking"
                @click="showPreview">
          {{ looking ? 'Mirando…' : 'Ver que se bajaria' }}</button>
        <button v-if="running" class="btn" @click="cancelDownload">Cancelar</button>
      </div>

      <div v-if="running" class="hint">
        Mientras baja no se puede cambiar el enlace ni las opciones.
      </div>
      <div v-if="error" class="hint" style="color:var(--red);font-style:normal">{{ error }}</div>
    </Card>

    <!-- en marcha -->
    <Card v-if="running">
      <template #title>
        <h3>{{ PHASES[status.phase] || 'Trabajando' }}
          <span v-if="status.total > 1" class="chip" style="margin-left:8px">
            {{ status.index }} de {{ status.total }}</span></h3>
      </template>
      <div class="dl-current">{{ status.name || '…' }}</div>
      <div class="track" style="cursor:default">
        <div class="track-fill" :style="{width: (status.percent || 0) + '%'}"></div>
      </div>
      <div class="dl-pct">
        <span class="mono">{{ (status.percent || 0).toFixed(0) }}%</span>
        <span class="sub">{{ PHASES[status.phase] || '' }}</span>
      </div>
    </Card>

    <!-- ya la tienes: preguntar si bajarla igualmente -->
    <div class="card dl-dup" v-if="askAgain && !running">
      <h3><Icon n="warning" :t="15" /> Esa ya la tienes</h3>
      <div v-for="(r,i) in askAgain" :key="i" class="dl-result">
        <strong>{{ r.title }}</strong>
        <div class="hint" v-for="m in r.matches" :key="m.id">
          en tu biblioteca: {{ m.artist ? m.artist + ' — ' : '' }}{{ m.title }}
        </div>
        <div class="btn-row" style="margin-top:9px">
          <button class="btn" :disabled="locked" @click="startDownload(r.url, true)">
            Descargar igualmente como repetida</button>
          <button class="btn mini" @click="askAgain = null">No, dejalo</button>
        </div>
      </div>
      <div class="hint">La copia se guarda con el sufijo « - r» para que puedas
        compararlas y borrar la que no quieras desde Duplicados.</div>
    </div>

    <!-- lo que se bajaria -->
    <Card v-if="preview && !running" note="Todavia no se ha bajado nada.">
      <template #title>
        <h3>{{ preview.name || 'Resultados' }}
          <span class="chip" style="margin-left:8px">{{ preview.items.length }}</span></h3>
      </template>
      <div v-for="t in preview.items" :key="t.id" class="dl-row">
        <span class="dl-title">{{ t.title }}</span>
        <span class="sub">{{ t.channel }}</span>
        <span class="mono sub dl-dur">{{ tt(t.duration) }}</span>
      </div>
    </Card>

    <!-- historial: aqui cae todo, del boton y del asistente. Siempre a la
         vista (antes habia una tarjeta de «Resultado» aparte que decia lo
         mismo). Cada fila se puede poner a sonar y parar desde aqui. -->
    <Card v-if="historyTotal || done.length">
      <template #title>
        <h3 style="display:flex;align-items:center;gap:8px">
          Historial
          <span class="chip">{{ historyTotal }}</span>
          <button class="btn mini" style="margin-left:auto" :disabled="running"
                  :title="running ? 'Cuando termine lo que esta bajando' : 'Borrar la lista de lo descargado'"
                  @click="clearHistory">Vaciar</button>
        </h3>
      </template>
      <div class="note">Queda registrado todo lo que se baja, tanto desde aqui
        como cuando se lo pides al asistente.</div>

      <div class="hist">
        <div v-for="h in history" :key="h.id" class="hist-row"
             :class="{sounding: playingId === h.song_id}">
          <button v-if="h.song_id && h.ok" class="row-go hist-play" :title="historyTitle(h.song_id)"
                  @click="playFromHistory(h.song_id)">
            <Icon :n="historyIcon(h.song_id)" :t="12" /></button>
          <span v-else class="hist-play-gap"></span>
          <span class="badge" :class="h.ok ? 'ok' : (h.already ? '' : 'bad')">
            {{ h.ok ? 'bajada' : (h.already ? 'ya la tenias' : 'fallo') }}</span>
          <span class="hist-title" :title="h.target || h.reason || h.title">
            {{ h.artist ? `${h.artist} — ${h.song}` : (h.title || h.query) }}</span>
          <span class="hist-src" :title="'Pedida ' + (SOURCE_LABEL[h.source] || h.source)">
            {{ SOURCE_LABEL[h.source] || h.source }}</span>
          <span class="mono hist-when">{{ when(h.at) }}</span>
        </div>
        <div v-if="!history.length" class="hint">Todavia no se ha bajado nada.</div>
      </div>
      <div v-if="history.length < historyTotal" class="btn-row" style="margin-top:8px">
        <button class="btn mini" @click="loadHistory(true)">
          Cargar mas ({{ historyTotal - history.length }} antes)</button>
      </div>
    </Card>
  </div>
</template>
