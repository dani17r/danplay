<script setup>
import { ref, onMounted } from 'vue'
import { api, pickFolder, errorMessage } from '../api.js'
import { addFolder } from '../utils/folders.js'
import { usePreferences } from '../composables/usePreferences.js'
import { notify } from '../composables/useNotices.js'
import { ask, tell } from '../composables/useDialog.js'
import { formatGigabytes } from '../utils/format.js'
import { DENSITIES, KIND_LABEL, allThemes, deleteCustomTheme } from '../themes.js'
import ThemeEditor from './ThemeEditor.vue'
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
const aiKey = ref('')
const fingerprintKey = ref('')
// «tengo la clave puesta» y «la clave funciona» no son lo mismo: esto hace la
// llamada mas barata posible contra DeepInfra para saberlo de verdad.
const checkingKey = ref(false)
const keyState = ref(null)

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
}
onMounted(load)

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

    <Card title="Inteligencia artificial" note="Identifica lo que la huella acustica no logra, y busca letra, acordes y datos.">
      <div style="display:flex;gap:8px;align-items:flex-end;margin-bottom:10px">
        <TextField v-model="aiKey" type="password" width="100%" label="Clave de DeepInfra"
               :placeholder="settings.has_ai_key ? 'Guardada: ' + settings.ai_key : 'pega aqui tu clave'"
               @enter="save('ai_key', aiKey); aiKey=''" />
        <button class="btn" :disabled="!aiKey" @click="save('ai_key', aiKey); aiKey=''">
          Guardar</button>
        <button class="btn" :disabled="checkingKey" @click="checkKey">
          {{ checkingKey ? 'Probando…' : 'Probar' }}</button>
      </div>
      <div v-if="keyState" class="key-state" :class="keyState.ok ? 'ok' : 'bad'">
        <Icon :n="keyState.ok ? 'check' : 'warning'" :t="14" />
        <span>{{ keyState.ok
          ? `La clave funciona · modelo ${keyState.model}`
          : keyState.reason }}</span>
      </div>
      <div style="display:flex;gap:8px;align-items:flex-end;margin-bottom:10px">
        <TextField v-model="fingerprintKey" type="password" width="100%" label="Clave de AcoustID"
               :placeholder="settings.fingerprint_key ? 'Guardada' : 'gratis en acoustid.org, opcional'"
               @enter="save('fingerprint_key', fingerprintKey); fingerprintKey=''" />
        <button class="btn" :disabled="!fingerprintKey"
                @click="save('fingerprint_key', fingerprintKey); fingerprintKey=''">Guardar</button>
      </div>
      <ToggleField :modelValue="settings.ai_enabled" title="Usar IA"
               :hint="'Modelo: ' + status.model"
               @update:modelValue="v => save('ai_enabled', v)" />
      <ToggleField :modelValue="settings.write_tags" title="Escribir etiquetas dentro del archivo"
               hint="Estrellas, letra, portada y tono viajan con el mp3"
               @update:modelValue="v => save('write_tags', v)" />
    </Card>

    <Card title="Sistema">
      <div style="display:flex;gap:8px;flex-wrap:wrap;margin-top:6px">
        <span class="badge" :class="status.ai?'ok':'bad'">IA {{ status.ai?'lista':'sin clave' }}</span>
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
