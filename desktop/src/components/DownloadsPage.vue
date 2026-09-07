<script setup>
import { ref, computed, onMounted, onUnmounted } from 'vue'
import { api } from '../api.js'
import Icon from './Icon.vue'
import TextField from './ui/TextField.vue'
import SelectField from './ui/SelectField.vue'
import ToggleField from './ui/ToggleField.vue'
import Card from './ui/Card.vue'

const emit = defineEmits(['reload', 'notice'])

const query = ref('')
const quality = ref(localStorage.getItem('danplay.ytQuality') || 'high')
const fileIt = ref(localStorage.getItem('danplay.ytFileIt') !== '0')
const results = ref(Number(localStorage.getItem('danplay.ytResults') || 5))

const status = ref(null)           // lo que devuelve /api/youtube
const preview = ref(null)          // lo que se bajaria
const looking = ref(false)
const error = ref('')
const askAgain = ref(null)         // la que ya tienes y preguntamos si bajar igual
const history = ref([])
const historyTotal = ref(0)
const showHistory = ref(false)

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

async function loadHistory () {
  try {
    const r = await api.downloadHistory(60)
    history.value = r.items || []
    historyTotal.value = r.total || 0
  } catch { /* el nucleo aun no responde */ }
}

async function clearHistory () {
  try {
    await api.clearDownloadHistory()
    await loadHistory()
    emit('notice', 'Historial vaciado', 'ok')
  } catch (e) { emit('notice', 'No se pudo vaciar: ' + e) }
}
let poll = null

const QUALITIES = [
  { v: 'high', n: 'Alta', note: '320 kbps' },
  { v: 'medium', n: 'Media', note: '192 kbps' },
  { v: 'variable', n: 'Variable', note: 'VBR, la mejor relacion tamaño/calidad' }
]
const HOW_MANY = [1, 3, 5, 10, 20].map(n => ({ v: n, n: String(n) }))

// El nucleo publica `active`. Antes esto miraba `running`, que no existe, asi
// que la barra nunca salia y el boton nunca se bloqueaba.
const running = computed(() => !!status.value?.active)
const ready = computed(() => !!status.value?.available)
// mientras baja no se toca nada: ni el enlace, ni la calidad, ni el archivado
const locked = computed(() => running.value || !ready.value)

const isSearch = computed(() => {
  const t = query.value.trim().toLowerCase()
  return !!t && !t.startsWith('http://') && !t.startsWith('https://')
})

const done = computed(() => status.value?.results || [])
const summary = computed(() => ({
  ok: done.value.filter(r => r.ok).length,
  already: done.value.filter(r => r.already_there).length,
  failed: done.value.filter(r => !r.ok && !r.already_there).length
}))

const PHASES = {
  starting: 'Preparando', downloading: 'Bajando', converting: 'Convirtiendo a mp3',
  filing: 'Identificando y archivando', done: 'Terminado', canceled: 'Cancelado'
}
const tt = (s) => !s ? '—'
  : `${Math.floor(s / 60)}:${String(Math.floor(s % 60)).padStart(2, '0')}`

async function refresh () {
  try { status.value = await api.youtube() } catch { /* el nucleo aun no responde */ }
}

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
    await refresh()
    schedule(300)          // que la barra aparezca al momento
  } catch (e) {
    error.value = String(e).replace(/^Error:\s*/, '')
  }
}

async function cancelDownload () {
  try { await api.youtubeCancel() } catch { /* da igual: ya habra terminado */ }
}

// Rapido mientras baja algo (hay barra de progreso que mover) y tranquilo
// cuando no. Antes preguntaba cada 0,6 s pasara lo que pasara: cien viajes
// por minuto al nucleo por estar mirando una pagina quieta.
let wasRunning = false
function schedule (ms = running.value ? 600 : 3000) {
  clearTimeout(poll)
  poll = setTimeout(bucle, ms)
}
async function bucle () {
  await refresh()
  if (wasRunning && !running.value) {
    // al terminar: la biblioteca ha cambiado y hay que contar como fue
    const { ok, already, failed } = summary.value
    if (ok) emit('reload')
    const partes = []
    if (ok) partes.push(`${ok} descargada${ok > 1 ? 's' : ''}`)
    if (already) partes.push(`${already} ya la ten${already > 1 ? 'ias' : 'ias'}`)
    if (failed) partes.push(`${failed} sin suerte`)
    if (partes.length) emit('notice', partes.join(' · '), ok ? 'ok' : 'info')
    // si solo hubo repetidas, se ofrece bajarlas igualmente
    const rep = done.value.filter(r => r.already_there && r.url)
    askAgain.value = rep.length ? rep : null
    loadHistory()          // el asistente tambien escribe aqui
  }
  wasRunning = running.value
  schedule()
}

onMounted(async () => {
  await refresh()
  await loadHistory()
  schedule()
})
onUnmounted(() => clearTimeout(poll))
</script>

<template>
  <div class="page">
    <h2>Descargas</h2>
    <div class="desc">
      Pega el enlace de un video o de una lista de YouTube, o escribe lo que buscas.
      DanPlay baja el audio, lo pasa a mp3 con su caratula y lo archiva por artista
      igual que todo lo demas.
    </div>

    <div v-if="status && !ready" class="card">
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

    <!-- historial: aqui cae todo, del boton y del asistente -->
    <Card v-if="historyTotal">
      <template #title>
        <h3 style="display:flex;align-items:center;gap:8px">
          Historial
          <span class="chip">{{ historyTotal }}</span>
          <button class="btn mini" style="margin-left:auto"
                  @click="showHistory = !showHistory">
            {{ showHistory ? 'Ocultar' : 'Ver' }}</button>
          <button v-if="showHistory" class="btn mini" @click="clearHistory">Vaciar</button>
        </h3>
      </template>
      <div class="note">Queda registrado todo lo que se baja, tanto desde aqui
        como cuando se lo pides al asistente.</div>

      <transition name="dropdown">
        <div v-if="showHistory" class="hist">
          <div v-for="h in history" :key="h.id" class="hist-row">
            <span class="badge" :class="h.ok ? 'ok' : (h.already ? '' : 'bad')">
              {{ h.ok ? 'bajada' : (h.already ? 'ya la tenias' : 'fallo') }}</span>
            <span class="hist-title" :title="h.title">
              {{ h.artist ? `${h.artist} — ${h.song}` : (h.title || h.query) }}</span>
            <span class="hist-src" :title="'Pedida ' + (SOURCE_LABEL[h.source] || h.source)">
              {{ SOURCE_LABEL[h.source] || h.source }}</span>
            <span class="mono hist-when">{{ when(h.at) }}</span>
          </div>
        </div>
      </transition>
    </Card>

    <!-- resultados -->
    <Card v-if="!running && done.length" title="Resultado">
      <div class="note">
        {{ summary.ok }} descargada(s)<template v-if="summary.already">, {{ summary.already }} ya la tenias</template><template v-if="summary.failed">, {{ summary.failed }} con fallo</template>
      </div>
      <div v-for="(r,i) in done" :key="i" class="dl-result">
        <div style="display:flex;gap:8px;align-items:center;flex-wrap:wrap">
          <span class="badge" :class="r.already_there ? '' : (r.ok ? (r.action === 'review' ? '' : 'ok') : 'bad')">
            {{ r.already_there ? 'ya la tenias' : (r.ok ? (r.action || 'bajada') : 'fallo') }}</span>
          <span v-if="r.forced" class="badge">repetida</span>
          <span v-if="r.identified_by" class="mono" style="font-size:11px;color:var(--muted2)">
            {{ r.identified_by }} {{ r.confidence?.toFixed(2) }}</span>
          <strong>{{ r.artist ? `${r.artist} — ${r.song}` : (r.title || r.source) }}</strong>
        </div>
        <div class="dl-path" v-if="r.target">{{ r.target }}</div>
        <div class="hint" v-for="m in r.matches" :key="m.id">
          ya la tienes como: {{ m.artist ? m.artist + ' — ' : '' }}{{ m.title }}
        </div>
        <div class="hint" v-if="!r.ok && !r.already_there">{{ r.reason }}</div>
        <div class="hint" v-for="w in r.warnings" :key="w">! {{ w }}</div>
      </div>
    </Card>
  </div>
</template>
