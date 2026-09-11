<script setup>
import { ref, computed, onMounted } from 'vue'
import { api, app, pickFolder, errorMessage } from '../api.js'
import { addFolder } from '../utils/folders.js'
import { usePreferences } from '../composables/usePreferences.js'
import { notify } from '../composables/useNotices.js'
import { ask, tell } from '../composables/useDialog.js'
import { formatGigabytes } from '../utils/format.js'
import { DENSITIES, KIND_LABEL, allThemes, deleteCustomTheme } from '../themes.js'
import ThemeEditor from './ThemeEditor.vue'
import AiProviderModal from './AiProviderModal.vue'
import Icon from './Icon.vue'
import Loading from './ui/Loading.vue'
import TextField from './ui/TextField.vue'
import SelectField from './ui/SelectField.vue'
import ToggleField from './ui/ToggleField.vue'
import Card from './ui/Card.vue'

const status = ref(null)
const settings = ref({})
const folders = ref({ folders: [], exclusions: [], always_excluded: [] })
const convertibles = ref(null)
const newPath = ref('')
const newPattern = ref('')
const busy = ref('')
const fingerprintKey = ref('')
// «tengo la clave puesta» y «la clave funciona» no son lo mismo: esto hace la
// llamada mas barata posible contra el proveedor para saberlo de verdad, y
// de paso comprueba que el modelo de conversacion sabe usar herramientas.
const checkingKey = ref(false)
const keyState = ref(null)
// El proveedor de IA se elige en un modal aparte: son sesenta y pico
// servicios con sus campos, y aqui solo se enseña cual esta en uso y los
// que ya estan configurados, para saltar entre ellos con un clic.
const aiInfo = ref(null)
const aiModal = ref(false)
const aiInitial = ref('')
const aiProfiles = computed(() => Object.values(aiInfo.value?.profiles || {}))
const activeAi = computed(() => aiInfo.value?.active_profile || null)
// Lo que gasta la IA (tokens y coste segun el catalogo): hoy y este mes.
const usage = ref(null)
async function loadUsage () {
  try { usage.value = await api.aiUsage() } catch { usage.value = null }
}
function money (v) {
  if (v == null) return '—'
  if (v === 0) return '$0'
  return '$' + (v < 0.01 ? v.toFixed(4) : v.toFixed(2)).replace('.', ',')
}
function tokens (u) {
  const n = (u?.prompt || 0) + (u?.completion || 0)
  return n >= 1_000_000 ? (n / 1e6).toFixed(1).replace('.', ',') + ' M' : n >= 1000 ? (n / 1000).toFixed(1).replace('.', ',') + ' k' : String(n)
}
async function setFallback (v) {
  try { aiInfo.value = await api.aiFallback(v) } catch (e) { notify(errorMessage(e)) }
}

function openAi (initial = '') { aiInitial.value = initial; aiModal.value = true }
// «Probar gratis, sin clave»: el nucleo prueba los servicios gratuitos por
// orden y activa el primero que responda. Tarda lo que tarden en contestar.
const tryingFree = ref(false)
const freeResult = ref(null)
async function tryFree () {
  tryingFree.value = true; freeResult.value = null; keyState.value = null
  try {
    const r = await api.aiFree()
    freeResult.value = r.free
    await afterAiSaved(r)
  } catch (e) { freeResult.value = { ok: false, reason: errorMessage(e) } } finally { tryingFree.value = false }
}
async function afterAiSaved (r) {
  aiInfo.value = r
  keyState.value = null
  settings.value = await api.settings()
  status.value = await api.status()
}
async function activateAi (id) {
  if (id === aiInfo.value?.active) return
  try { await afterAiSaved(await api.aiActivate(id)) } catch (e) { notify(errorMessage(e)) }
}
async function removeAi (p) {
  const ok = await ask({ kind: 'confirm', title: 'Quitar este proveedor', danger: true,
                         message: `Se borra la clave y los ajustes de ${p.provider_name}.`, okLabel: 'Quitar' })
  if (!ok) return
  try { await afterAiSaved(await api.aiDeleteProfile(p.id)) } catch (e) { notify(errorMessage(e)) }
}
function ago (ts) {
  if (!ts) return 'nunca'
  const m = Math.round((Date.now() / 1000 - ts) / 60)
  if (m < 1) return 'ahora mismo'
  if (m < 60) return `hace ${m} min`
  const h = Math.round(m / 60)
  return h < 48 ? `hace ${h} h` : `hace ${Math.round(h / 24)} días`
}
// Si el sistema abre las canciones con DanPlay. Se pregunta al entrar y
// despues de pedirlo: es un ajuste del SISTEMA, no nuestro, y puede haberlo
// cambiado otro programa desde fuera.
const player = ref(null)
const claiming = ref(false)

async function checkKey () {
  checkingKey.value = true; keyState.value = null
  try { keyState.value = await api.checkAi() }
  catch (e) { keyState.value = { ok: false, reason: errorMessage(e) } }
  finally { checkingKey.value = false }
}
const folderNotice = ref(null)
const editor = ref(null)      // null | '' (isNew) | key (editar)
const catalog = ref(allThemes())
// El tema y la densidad viven en las preferencias, no en props que suben y
// bajan por eventos hasta App.
const { theme, density } = usePreferences()
function afterSave (key) { catalog.value = allThemes(); editor.value = null; theme.value = key }
async function removeTheme (key) {
  const ok = await ask({ kind: 'confirm', title: 'Borrar el tema', danger: true,
                         message: 'Se borrará este tema tuyo. Los del catálogo no se tocan.',
                         okLabel: 'Borrar' })
  if (!ok) return
  deleteCustomTheme(key); catalog.value = allThemes()
  if (theme.value === key) theme.value = 'night'
}
const emit = defineEmits(['reindexed','changed'])

async function load () {
  status.value = await api.status()
  settings.value = await api.settings()
  folders.value = await api.folders()
  convertibles.value = await api.convertible()
  player.value = await app.defaultPlayer()
  // Consulta el catalogo de modelos en segundo plano si hace rato: asi el
  // apartado de IA abre ya con la lista de hoy.
  try { aiInfo.value = await api.aiProviders() } catch { /* sin nucleo de IA: la tarjeta lo dice */ }
  loadUsage()
}
onMounted(load)

/** Pide que el sistema abra las canciones con DanPlay. */
async function claimDefault () {
  claiming.value = true
  try {
    player.value = await app.makeDefaultPlayer()
    if (player.value?.is_default) notify('Ya se abren con DanPlay')
    else if (player.value?.note) notify(player.value.note)
  } catch (e) {
    notify(errorMessage(e))
    // Puede haber cambiado a medias: se vuelve a preguntar al sistema en vez
    // de dejar en pantalla lo que creíamos antes de intentarlo.
    player.value = await app.defaultPlayer()
  } finally {
    claiming.value = false
  }
}

async function save (key, value) {
  settings.value[key] = value
  settings.value = await api.saveSettings({ [key]: value })
  status.value = await api.status()
}
async function browseFolder () {
  const r = await pickFolder('Añadir carpeta de música')
  if (r) { newPath.value = r; await addNewFolder() }
}

// Los nombres de `action` los traduce `utils/folders.js`, que es el mismo
// codigo que usa la bienvenida. Aqui se comparaban con los nombres en
// castellano que la API dejo de usar, asi que no salia ningun aviso y el
// caso «parece una copia» vaciaba el campo sin añadir nada.
async function addNewFolder (force = false) {
  if (!newPath.value.trim()) return
  busy.value = 'folder'
  folderNotice.value = null
  try {
    const r = await addFolder(newPath.value, { force, scan: false })
    if (r.action === 'confirm' || r.action === 'error') {
      folderNotice.value = r
      return
    }
    folderNotice.value = r.action === 'added' ? null : r
    folders.value = await api.folders()
    newPath.value = ''
    notify(r.message, r.kind)
    emit('changed')
  } finally { busy.value = '' }
}

async function scan () {
  busy.value = 'scan'
  try { const r = await api.scan(); status.value = await api.status(); emit('reindexed', r) }
  finally { busy.value = '' }
}
async function convert (dry_run) {
  busy.value = 'convert'
  try {
    // OJO: la API lee `keep`, no `keepOne`. Con el nombre mal, «conservar el
    // original» se ignoraba siempre y los archivos de partida se borraban.
    const r = await api.convert({ dry_run, quality: settings.value.quality,
                                  keep: settings.value.keep_original })
    tell(dry_run ? `Se convertirían ${r.converted} archivos`
                 : `Convertidos ${r.converted}, fallos ${r.failures}`,
         { title: 'Conversión a mp3' })
    convertibles.value = await api.convertible()
  } finally { busy.value = '' }
}
const gb = formatGigabytes
</script>

<template>
  <div class="page" v-if="status">
    <h2>Ajustes</h2>
    <div class="desc">DanPlay · {{ status.stats.total }} canciones · {{ gb(status.stats.bytes) }} GB
      · {{ Math.round(status.stats.seconds/3600) }} horas</div>

    <Card title="Apariencia" note="El tema se aplica al instante y se recuerda.">
      <div style="display:grid;grid-template-columns:repeat(auto-fill,minmax(132px,1fr));gap:9px">
        <div v-for="(t,k) in catalog" :key="k" @click="theme = k"
             :style="{background:t.v.panel, border:'2px solid '+(theme===k?t.v.accent:t.v.border),
                      borderRadius:'8px', padding:'10px', cursor:'pointer', position:'relative'}">
          <div style="display:flex;gap:4px;margin-bottom:7px">
            <span v-for="c in [t.v.accent,t.v.text,t.v.muted,t.v.panel2]" :key="c"
                  :style="{background:c,width:'15px',height:'15px',borderRadius:'3px',
                           border:'1px solid '+t.v.border2}"></span>
          </div>
          <div :style="{color:t.v.text,fontSize:'12px',fontWeight:theme===k?600:400}">
            {{ t.name }}</div>
          <div v-if="t.custom" style="display:flex;gap:6px;margin-top:6px">
            <button class="btn mini" style="padding:2px 7px"
                    @click.stop="editor=k">Editar</button>
            <button class="btn mini" style="padding:2px 7px"
                    @click.stop="removeTheme(k)">Borrar</button>
          </div>
          <div v-else :style="{fontSize:'10px',color:t.v.muted2,marginTop:'6px'}">
            {{ KIND_LABEL[t.kind] || t.kind }}</div>
        </div>
        <div @click="editor=''" :style="{border:'2px dashed var(--border2)',borderRadius:'8px',
             padding:'10px',cursor:'pointer',display:'flex',flexDirection:'column',
             alignItems:'center',justifyContent:'center',minHeight:'86px',color:'var(--muted2)'}">
          <div style="font-size:20px">＋</div>
          <div style="font-size:11.5px">Crear el mio</div>
        </div>
      </div>

      <div style="display:flex;align-items:center;gap:9px;margin-top:13px">
        <SelectField :modelValue="density" width="220px" label="Densidad de las listas"
                  :options="Object.entries(DENSITIES).map(([k,d]) => ({v: k, n: d.name}))"
                  @update:modelValue="v => density = v" />
      </div>
    </Card>

    <ThemeEditor v-if="editor !== null" :activeTheme="theme" :editing="editor || null"
                @close="editor=null" @saved="afterSave" />

    <Card title="Atajos de teclado">
      <div style="display:grid;grid-template-columns:auto 1fr;gap:5px 14px;font-size:12px">
        <span class="mono" style="color:var(--accent)">espacio</span><span>reproducir / pausar</span>
        <span class="mono" style="color:var(--accent)">← →</span><span>retroceder / avanzar 10 s (con Shift, 30 s)</span>
        <span class="mono" style="color:var(--accent)">↑ ↓</span><span>volumen</span>
        <span class="mono" style="color:var(--accent)">N / P</span><span>siguiente / anterior</span>
        <span class="mono" style="color:var(--accent)">M</span><span>silenciar</span>
        <span class="mono" style="color:var(--accent)">S</span><span>aleatorio</span>
        <span class="mono" style="color:var(--accent)">R</span><span>repetir</span>
      </div>
    </Card>

    <Card title="Carpetas gestionadas" note="DanPlay analiza e indexa todo lo que haya dentro de estas carpetas.">
      <div v-if="folders.folders.length > 1" class="hint"
           style="margin-bottom:9px;color:var(--amber)">
        Si dos carpetas contienen la misma musica, cada cancion aparecera dos veces.
      </div>
      <div v-for="c in folders.folders" :key="c.path" class="path-row">
        <span class="path" :title="c.path">{{ c.path }}</span>
        <span class="badge">{{ c.n }} temas</span>
        <button class="btn mini" @click="api.removeFolder(c.path).then(r=>{folders=r; emit('changed')})">Quitar</button>
      </div>
      <div style="display:flex;gap:8px;margin-top:10px">
        <button class="btn" :disabled="busy==='folder'" @click="browseFolder" style="gap:7px">
          <Icon n="folderOpen" :t="15" /> Examinar…</button>
        <TextField v-model="newPath" width="100%" icon="folder"
               placeholder="o escribe la ruta" @enter="addNewFolder()" />
        <button class="btn" :disabled="busy==='folder' || !newPath.trim()"
                @click="addNewFolder()">Agregar</button>
      </div>
      <div v-if="folderNotice" class="hint"
           :style="{color: folderNotice.confirmable ? 'var(--amber)' : 'var(--muted)'}">
        {{ folderNotice.message }}
        <div v-if="folderNotice.confirmable" class="btn-row" style="margin-top:8px">
          <button class="btn mini" @click="addNewFolder(true)">Añadir igualmente</button>
          <button class="btn mini" @click="folderNotice=null">Cancelar</button>
        </div>
      </div>
      <div style="margin-top:12px">
        <button class="btn primary" :disabled="busy==='scan'" @click="scan">
          {{ busy==='scan' ? 'Analizando…' : 'Analizar e indexar todo' }}</button>
      </div>
    </Card>

    <Card title="Carpetas omitidas" note="No se indexan. Util para material de trabajo o copias.">
      <div style="display:flex;gap:6px;flex-wrap:wrap;margin-bottom:10px">
        <span v-for="e in folders.exclusions" :key="e.pattern" class="chip x"
              :title="e.note" @click="api.removeExclusion(e.pattern).then(r=>folders=r)">
          {{ e.pattern }} ×</span>
        <span v-if="!folders.exclusions.length" style="color:var(--muted2);font-size:12px">
          ninguna</span>
      </div>
      <div style="display:flex;gap:8px;align-items:flex-end">
        <TextField v-model="newPattern" width="100%" placeholder="Secuencias  o  */Copias/*"
               @enter="api.addExclusion(newPattern).then(r=>{folders=r;newPattern=''})" />
        <button class="btn" :disabled="!newPattern.trim()"
                @click="api.addExclusion(newPattern).then(r=>{folders=r;newPattern=''})">
          Omitir</button>
      </div>
      <div class="hint">Siempre omitidas: {{ (folders.always_excluded || []).join(', ') }}</div>
    </Card>

    <Card title="Formato de archivo" note="Unifica la biblioteca en mp3 cuando llega algo en otro formato.">
      <ToggleField :modelValue="settings.convert_mp3" title="Convertir a mp3 al importar"
               hint="Si llega un .m4a, .flac, .ogg o .wma, se pasa a mp3 automaticamente"
               @update:modelValue="v => save('convert_mp3', v)" />
      <ToggleField :modelValue="settings.keep_original" title="Conservar el archivo original"
               hint="Ocupa el doble, pero no pierdes la fuente"
               @update:modelValue="v => save('keep_original', v)" />
      <div style="margin-top:11px">
        <SelectField :modelValue="settings.quality" width="240px" label="Calidad"
                  :options="[{v:'high',n:'Alta',note:'320 kbps'},
                              {v:'medium',n:'Media',note:'192 kbps'},
                              {v:'variable',n:'Variable',note:'V0, peso ajustado'}]"
                  @update:modelValue="v => save('quality', v)" />
      </div>
      <div class="hint">
        Nunca se convierten: {{ status.never_convert.join(', ') }} — ahi los .wav son material
        de produccion y comprimirlos seria perder calidad.
      </div>
      <div style="margin-top:12px" v-if="convertibles">
        <div style="font-size:12px;color:var(--muted);margin-bottom:8px">
          {{ convertibles.total }} archivos en la biblioteca no son mp3</div>
        <div class="btn-row" v-if="convertibles.total">
          <button class="btn mini" :disabled="busy" @click="convert(true)">Simular</button>
          <button class="btn mini" :disabled="busy" @click="convert(false)">Convertir ahora</button>
        </div>
      </div>
    </Card>

    <Card title="Biblioteca" note="Donde vive tu musica. DanPlay organiza dentro de esta carpeta.">
      <div style="display:flex;gap:8px;align-items:flex-end">
        <TextField v-model="settings.library" width="100%" icon="folder" placeholder="~/Musica"
               @enter="save('library', settings.library)" />
        <button class="btn" @click="save('library', settings.library)">Guardar</button>
      </div>
    </Card>

    <Card title="Inteligencia artificial" note="Identifica lo que la huella acustica no logra, busca letra, acordes y datos, y es el asistente. Vale cualquier proveedor: OpenAI, Anthropic, Google Gemini, DeepInfra, OpenRouter, Ollama en tu equipo…">
      <div class="ai-current">
        <span class="ai-current-ico"><Icon n="ai" :t="22" /></span>
        <div class="ai-current-txt">
          <strong v-if="activeAi">{{ activeAi.name }}</strong>
          <strong v-else>Sin proveedor elegido</strong>
          <span v-if="activeAi">conversación: <span class="mono">{{ activeAi.chat_model || '—' }}</span>
            · identificar: <span class="mono">{{ activeAi.model || '—' }}</span></span>
          <span v-else>Elige uno y pega su clave; o usa un modelo en tu propio equipo, sin clave.</span>
        </div>
        <span class="badge" :class="settings.ai_ready ? 'ok' : 'bad'">
          {{ settings.ai_ready ? 'lista' : (settings.ai_reason || 'sin configurar') }}</span>
        <div class="btn-row">
          <button v-if="activeAi" class="btn" @click="openAi(activeAi.id)">Ajustar…</button>
          <button class="btn primary" @click="openAi()">{{ activeAi ? 'Cambiar…' : 'Elegir proveedor…' }}</button>
          <button v-if="!activeAi" class="btn" :disabled="tryingFree" @click="tryFree">
            {{ tryingFree ? 'Buscando uno que responda…' : 'Probar gratis, sin clave' }}</button>
          <button v-else class="btn" :disabled="checkingKey" @click="checkKey">
            {{ checkingKey ? 'Probando…' : 'Probar' }}</button>
        </div>
      </div>
      <div v-if="freeResult" class="key-state" :class="freeResult.ok ? 'ok' : 'bad'">
        <Icon :n="freeResult.ok ? 'check' : 'warning'" :t="14" />
        <span v-if="freeResult.ok">Listo: {{ freeResult.name }} con <span class="mono">{{ freeResult.chat_model }}</span>,
          gratis y sin cuenta. Tiene límites y tus preguntas pasan por un servicio de terceros: para uso
          serio, pon tu clave o usa un modelo en tu equipo.</span>
        <span v-else>{{ freeResult.reason }}</span>
      </div>
      <div v-if="keyState" class="key-state" :class="keyState.ok ? 'ok' : 'bad'">
        <Icon :n="keyState.ok ? 'check' : 'warning'" :t="14" />
        <span v-if="keyState.ok">Funciona · {{ keyState.model }} responde en {{ keyState.latency_ms }} ms
          <span v-if="keyState.tools_ok"> · el asistente puede usar {{ keyState.chat_model }}</span>
          <span v-else style="color:var(--amber)"> · {{ keyState.chat_model }}: {{ keyState.tools_reason }}</span>
        </span>
        <span v-else>{{ keyState.reason }}</span>
      </div>
      <div v-if="aiProfiles.length > 1" class="ai-profiles">
        <span v-for="p in aiProfiles" :key="p.id" class="ai-profile" :class="{active: p.id === aiInfo.active}"
              :title="p.id === aiInfo.active ? 'en uso' : 'usar este'" @click="activateAi(p.id)">
          <Icon v-if="p.id === aiInfo.active" n="check" :t="11" />
          {{ p.provider_name }}
          <button class="field-btn" type="button" title="Ajustar" @click.stop="openAi(p.id)"><Icon n="pencil" :t="11" /></button>
          <button class="field-btn" type="button" title="Quitar" @click.stop="removeAi(p)"><Icon n="close" :t="11" /></button>
        </span>
      </div>
      <ToggleField :modelValue="settings.ai_enabled" title="Usar IA"
               hint="Apagada, la app identifica solo por etiquetas y huella, y el asistente se calla"
               @update:modelValue="v => save('ai_enabled', v)" />
      <ToggleField v-if="aiInfo" :modelValue="aiInfo.fallback" title="Si el proveedor falla, usar los demás"
               :hint="aiInfo.fallbacks?.length
                 ? 'Caído, sin crédito o saturado: se responde con ' + aiInfo.fallbacks.map(f => f.name).join(', ') + ' y se avisa'
                 : 'No hay otro configurado: añade uno (o el gratuito) y hará de respaldo'"
               @update:modelValue="setFallback" />
      <div v-if="usage" class="ai-usage">
        <span><strong>Hoy</strong> {{ usage.today.calls }} llamadas · {{ tokens(usage.today) }} tokens · {{ money(usage.today.cost) }}</span>
        <span><strong>Este mes</strong> {{ usage.month.calls }} llamadas · {{ tokens(usage.month) }} tokens · {{ money(usage.month.cost) }}</span>
        <span v-if="usage.month.unpriced" class="hint" style="margin:0">{{ usage.month.unpriced }} llamadas sin precio conocido (no cuentan en el coste)</span>
      </div>
      <div style="display:flex;gap:8px;align-items:flex-end;margin:6px 0 10px">
        <TextField v-model="fingerprintKey" type="password" width="100%" label="Clave de AcoustID"
               :placeholder="settings.fingerprint_key ? 'Guardada' : 'gratis en acoustid.org, opcional'"
               @enter="save('fingerprint_key', fingerprintKey); fingerprintKey=''" />
        <button class="btn" :disabled="!fingerprintKey"
                @click="save('fingerprint_key', fingerprintKey); fingerprintKey=''">Guardar</button>
      </div>
      <ToggleField :modelValue="settings.write_tags" title="Escribir etiquetas dentro del archivo"
               hint="Estrellas, letra, portada y tono viajan con el mp3"
               @update:modelValue="v => save('write_tags', v)" />
      <div v-if="aiInfo" class="hint">
        Catálogo de modelos (models.dev): {{ aiInfo.catalog_status.models }} modelos de
        {{ aiInfo.catalog_status.providers }} proveedores, comprobado {{ ago(aiInfo.catalog_status.checked_at) }}.
        Se vuelve a consultar cada vez que abres el selector.
      </div>
    </Card>

    <AiProviderModal :open="aiModal" :initial="aiInitial" @close="aiModal = false" @saved="afterAiSaved" />

    <Card v-if="player?.supported" title="Abrir canciones con DanPlay"
          note="Para que al abrir una canción desde el explorador de archivos suene aquí.">
      <div class="setting-row">
        <div>
          <div>{{ player.is_default ? 'Las canciones se abren con DanPlay.'
                                    : 'Ahora mismo las abre otro programa.' }}</div>
          <div class="hint" v-if="!player.direct">
            Windows no deja que un programa se ponga solo: DanPlay queda en «Abrir con»
            y el último clic lo das tú en Ajustes.
          </div>
          <div class="hint" v-else-if="player.is_default">
            Puedes volver a cambiarlo desde los ajustes de tu escritorio.
          </div>
        </div>
        <button class="btn" :disabled="claiming || (player.is_default && player.direct)"
                @click="claimDefault">
          {{ claiming ? 'un momento…'
             : player.is_default ? (player.direct ? 'Ya está puesto' : 'Volver a Ajustes')
             : 'Abrirlas con DanPlay' }}
        </button>
      </div>
      <div class="hint" v-if="player.note">{{ player.note }}</div>
    </Card>

    <Card title="Sistema">
      <div style="display:flex;gap:8px;flex-wrap:wrap;margin-top:6px">
        <span class="badge" :class="status.ai?'ok':'bad'">IA {{ status.ai ? (status.provider || 'lista') : 'sin configurar' }}</span>
        <span class="badge" :class="status.rust?'ok':'bad'">Rust {{ status.rust?'activo':'no compilado' }}</span>
        <span class="badge" :class="status.ffmpeg?'ok':'bad'">ffmpeg</span>
        <span class="badge" :class="status.fingerprint?'ok':'bad'">Huella acustica</span>
      </div>
      <div class="hint" v-if="!status.fingerprint">{{ status.fingerprint_reason }}</div>
      <div class="hint">Ajustes en {{ settings.settings_file }}</div>
    </Card>
  </div>
  <div v-else class="page"><Loading text="cargando ajustes…" /></div>
</template>
