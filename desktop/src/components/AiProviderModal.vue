<script setup>
/**
 * Elegir y configurar el proveedor de IA.
 *
 * Dos pasos: la rejilla de proveedores por grupos (grandes laboratorios,
 * plataformas tipo DeepInfra, nubes corporativas, Asia, en tu equipo, otro)
 * y el formulario con SOLO lo que ese proveedor necesita: clave (con el
 * enlace a donde se consigue), URL si es editable, región o recurso si la
 * URL los lleva, y los dos modelos —el rápido para identificar y rellenar
 * fichas, y el de conversación, que necesita herramientas—, con la lista
 * viva del proveedor cruzada con el catálogo de models.dev (precio,
 * herramientas, obsoleto, recomendado). «Probar» comprueba lo escrito sin
 * guardarlo; «Guardar y usar» lo guarda y lo deja activo.
 *
 * Las claves nunca vuelven enteras del núcleo: la caja llega vacía y, si no
 * se escribe nada, se conserva la guardada.
 */
import { ref, computed, watch, nextTick, onUnmounted } from 'vue'
import { api, app, errorMessage } from '../api.js'
import { ask } from '../composables/useDialog.js'
import Icon from './Icon.vue'
import TextField from './ui/TextField.vue'
import Loading from './ui/Loading.vue'
import ModelPicker from './ui/ModelPicker.vue'

const props = defineProps({
  open: Boolean,
  /** id de perfil o proveedor con el que abrir directamente el formulario */
  initial: { type: String, default: '' }
})
const emit = defineEmits(['close', 'saved'])

const GROUP_ICON = { free: 'bolt', lab: 'sparkles', platform: 'cloud', cloud: 'building', asia: 'globe', local: 'cpu', custom: 'server' }

const data = ref(null)            // lo que devuelve /api/ai/providers
const step = ref('pick')          // pick | form
const filter = ref('')
const chosen = ref(null)          // la entrada del catálogo
const profileId = ref('')         // el perfil guardado que se edita ('' si es nuevo)
const form = ref(blank())
const advanced = ref(false)
const models = ref({ ok: false, models: [], catalog: [], suggest: {}, reason: '' })
const loadingModels = ref(false)
const checking = ref(false)
const checkResult = ref(null)
const saving = ref(false)
const error = ref('')
const box = ref(null)

function blank () {
  return { key: '', base_url: '', fields: {}, model: '', chat_model: '', headersText: '', extraText: '', timeout: 60, name: '' }
}

// ------------------------------------------------------------- abrir/cerrar
watch(() => props.open, async (v) => {
  if (!v) { document.removeEventListener('keydown', keys); return }
  document.addEventListener('keydown', keys)
  error.value = ''; filter.value = ''; checkResult.value = null
  step.value = 'pick'
  await load()
  if (props.initial) {
    const prof = data.value?.profiles?.[props.initial]
    const entry = catalogById(prof ? prof.provider : props.initial)
    if (entry) pick(entry, prof ? props.initial : '')
  }
  await nextTick()
  box.value?.querySelector('input')?.focus()
}, { immediate: true })
onUnmounted(() => document.removeEventListener('keydown', keys))

function keys (e) {
  if (e.key === 'Escape') {
    e.preventDefault()
    if (step.value === 'form') back(); else emit('close')
  }
}

async function load (refresh = false) {
  try { data.value = await api.aiProviders(refresh) } catch (e) { error.value = errorMessage(e) }
}
const catalogById = (id) => data.value?.catalog?.find((p) => p.id === id) || null

// ------------------------------------------------------------------- paso 1
const groups = computed(() => {
  if (!data.value) return []
  const q = filter.value.trim().toLowerCase()
  return data.value.groups.map((g) => ({
    ...g,
    items: data.value.catalog.filter((p) => p.group === g.id &&
      (!q || (p.name + ' ' + p.id + ' ' + (p.note || '')).toLowerCase().includes(q)))
  })).filter((g) => g.items.length)
})
/** Los perfiles guardados de un proveedor del catálogo (varios si es «otro»). */
function savedFor (providerId) {
  return Object.values(data.value?.profiles || {}).filter((p) => p.provider === providerId)
}

function pick (entry, existingId = '') {
  chosen.value = entry
  profileId.value = existingId || (entry.id !== 'custom' ? entry.id : '')
  const saved = existingId ? data.value.profiles[existingId] : data.value.profiles[entry.id]
  const f = blank()
  f.base_url = (saved?.base_url) || entry.base_url
  f.fields = Object.fromEntries(entry.fields.map((x) => [x.name, saved?.fields?.[x.name] || '']))
  f.model = saved?.model || entry.suggest?.fast || ''
  f.chat_model = saved?.chat_model || entry.suggest?.chat || ''
  f.headersText = Object.entries(saved?.headers || {}).map(([k, v]) => `${k}: ${v}`).join('\n')
  f.extraText = Object.keys(saved?.extra || {}).length ? JSON.stringify(saved.extra, null, 1) : ''
  f.timeout = saved?.timeout || 60
  f.name = entry.id === 'custom' ? (saved?.name || saved?.provider_name || '') : ''
  form.value = f
  advanced.value = !!(f.headersText || f.extraText)
  checkResult.value = null; error.value = ''
  models.value = { ok: false, models: [], catalog: [], suggest: {}, reason: '' }
  step.value = 'form'
  // Con clave guardada, o sin necesitar clave, se pide la lista ya: es lo
  // primero que uno quiere ver, y en un servidor local dice de paso si está
  // arrancado.
  if (saved?.has_key || entry.key !== 'required') loadModels()
  else loadModels(true)     // solo el catálogo, sin molestar al proveedor
  nextTick(() => box.value?.querySelector('.ai-form input')?.focus())
}
function back () { step.value = 'pick'; chosen.value = null; checkResult.value = null; error.value = '' }

// Al pegar una clave se pide la lista sola, un momento despues de dejar de
// escribir: es lo que uno haria a mano, y de paso confirma que la clave vale.
let keyTimer = null
watch(() => form.value.key, (v) => {
  clearTimeout(keyTimer)
  if (step.value === 'form' && v.trim().length > 8) keyTimer = setTimeout(() => loadModels(), 700)
})
onUnmounted(() => clearTimeout(keyTimer))

// ------------------------------------------------------------------- paso 2
const savedProfile = computed(() => (profileId.value ? data.value?.profiles?.[profileId.value] : null))
const needsKey = computed(() => chosen.value?.key === 'required')
const urlEditable = computed(() => !!chosen.value && (chosen.value.group === 'local' || chosen.value.id === 'custom' || !chosen.value.fields.length))
/** La URL con los huecos ({region}, {resource}…) rellenos con lo escrito. */
const shownUrl = computed(() => (form.value.base_url || '').replace(/\{(\w+)\}/g, (_, k) => form.value.fields[k]?.trim() || `{${k}}`))

/** Lo que se manda al núcleo: solo lo que difiere del catálogo. */
function draft () {
  const f = form.value
  const d = { id: profileId.value || undefined, provider: chosen.value.id,
              model: f.model.trim(), chat_model: f.chat_model.trim(),
              fields: { ...f.fields }, timeout: Number(f.timeout) || 60 }
  if (f.key.trim()) d.key = f.key.trim()
  if (f.base_url.trim() && f.base_url.trim() !== chosen.value.base_url) d.base_url = f.base_url.trim()
  if (chosen.value.id === 'custom') { d.base_url = f.base_url.trim(); d.name = f.name.trim() || f.fields.name || '' }
  d.headers = parseHeaders(f.headersText)
  const extra = parseExtra(f.extraText)
  if (extra === null) throw new Error('Los parámetros extra tienen que ser un objeto JSON válido.')
  d.extra = extra
  return d
}
function parseHeaders (text) {
  const out = {}
  for (const line of (text || '').split('\n')) {
    const i = line.indexOf(':')
    if (i > 0) out[line.slice(0, i).trim()] = line.slice(i + 1).trim()
  }
  return out
}
function parseExtra (text) {
  if (!text.trim()) return {}
  try {
    const v = JSON.parse(text)
    return v && typeof v === 'object' && !Array.isArray(v) ? v : null
  } catch { return null }
}

const missing = computed(() => {
  if (!chosen.value) return ''
  const f = form.value
  // en el orden del formulario: primero los huecos de la URL, luego la clave
  for (const x of chosen.value.fields) if (x.name !== 'name' && !f.fields[x.name]?.trim()) return `falta ${x.label.toLowerCase()}`
  if (needsKey.value && !f.key.trim() && !savedProfile.value?.has_key) return 'falta la clave'
  if (chosen.value.id === 'custom' && !/^https?:\/\//.test(f.base_url.trim())) return 'falta la URL (http o https)'
  if (!f.model.trim() && !f.chat_model.trim()) return 'falta el modelo'
  return ''
})

/** Lista que ven los selectores: la del proveedor si respondió; si no, el catálogo. */
const modelList = computed(() => (models.value.ok ? models.value.models : models.value.catalog) || [])
const listNote = computed(() => {
  const m = models.value
  if (loadingModels.value) return 'pidiendo la lista al proveedor…'
  if (m.ok) return `${m.models.length} modelos disponibles con tu clave en ${m.provider || chosen.value?.name}`
  if (m.catalog?.length) return `${m.catalog.length} modelos según el catálogo` + (m.reason ? ` · el proveedor no respondió: ${m.reason}` : ' · con la clave, se pide la lista real')
  return m.reason ? `sin lista: ${m.reason}` : 'sin lista: escribe el nombre del modelo'
})

async function loadModels (catalogOnly = false) {
  loadingModels.value = true
  try {
    let d
    try { d = draft() } catch (e) { error.value = e.message; return }
    if (catalogOnly) delete d.key
    models.value = await api.aiModels(d)
    // sin modelo escrito, el recomendado; y si el proveedor lista lo que hay
    // (Ollama: lo descargado) y lo escrito no está, se avisa dejándolo
    const s = models.value.suggest || {}
    if (!form.value.chat_model && s.chat) form.value.chat_model = s.chat
    if (!form.value.model) form.value.model = s.fast || form.value.chat_model
  } catch (e) {
    models.value = { ok: false, models: [], catalog: [], suggest: {}, reason: errorMessage(e) }
  } finally { loadingModels.value = false }
}

async function check () {
  checking.value = true; checkResult.value = null; error.value = ''
  try { checkResult.value = await api.aiCheck(draft()) } catch (e) { checkResult.value = { ok: false, reason: errorMessage(e) } } finally { checking.value = false }
}

async function save () {
  saving.value = true; error.value = ''
  try {
    const r = await api.aiSaveProfile({ ...draft(), activate: true })
    emit('saved', r)
    emit('close')
  } catch (e) { error.value = errorMessage(e) } finally { saving.value = false }
}

async function remove () {
  const ok = await ask({ kind: 'confirm', title: 'Quitar este proveedor', danger: true,
                         message: `Se borra la clave y los ajustes de ${chosen.value.name}. Puedes volver a configurarlo cuando quieras.`,
                         okLabel: 'Quitar' })
  if (!ok) return
  try {
    const r = await api.aiDeleteProfile(profileId.value)
    data.value = r; emit('saved', r); back()
  } catch (e) { error.value = errorMessage(e) }
}

const openLink = (url) => app.openInBrowser(url).catch(() => {})

// «Probar gratis, sin clave» desde la rejilla: el nucleo elige el primero
// de los gratuitos que responda, lo activa, y aqui se cierra con lo hecho.
const tryingFree = ref(false)
async function tryFree () {
  tryingFree.value = true; error.value = ''
  try {
    const r = await api.aiFree()
    if (r.free?.ok) { emit('saved', r); emit('close') } else {
      data.value = r
      error.value = r.free?.reason || 'ninguno responde ahora mismo'
    }
  } catch (e) { error.value = errorMessage(e) } finally { tryingFree.value = false }
}

function ago (ts) {
  if (!ts) return 'nunca'
  const m = Math.round((Date.now() / 1000 - ts) / 60)
  if (m < 1) return 'ahora mismo'
  if (m < 60) return `hace ${m} min`
  const h = Math.round(m / 60)
  if (h < 48) return `hace ${h} h`
  return `hace ${Math.round(h / 24)} días`
}
</script>

<template>
  <transition name="fade">
    <div v-if="open" class="modal-back" @mousedown.self="emit('close')">
      <div class="modal ai-modal" ref="box" role="dialog" aria-modal="true">

        <!-- ============ paso 1: elegir ============ -->
        <template v-if="step === 'pick'">
          <h3 class="modal-title">
            <Icon n="ai" :t="16" /> ¿Qué IA quieres usar?
            <button class="btn mini ai-close" type="button" title="Cerrar" @click="emit('close')">
              <Icon n="close" :t="13" /></button>
          </h3>
          <div class="modal-msg">Todos hablan el mismo protocolo, así que vale cualquiera: un laboratorio,
            una plataforma como DeepInfra u OpenRouter, la nube de tu empresa, un modelo corriendo en este
            equipo o un servidor propio. Los ya configurados llevan una marca.</div>
          <TextField v-model="filter" width="100%" icon="search" placeholder="buscar proveedor…" compact />
          <Loading v-if="!data" style="margin-top:14px" />
          <div v-else class="ai-groups">
            <section v-for="g in groups" :key="g.id" class="ai-group">
              <h4><Icon :n="GROUP_ICON[g.id] || 'cloud'" :t="14" /> {{ g.name }}
                <span class="ai-group-note">{{ g.note }}</span>
                <button v-if="g.id === 'free'" class="btn mini primary ai-free-btn" type="button"
                        :disabled="tryingFree" @click="tryFree">
                  {{ tryingFree ? 'buscando uno que responda…' : 'Probar gratis ahora' }}</button>
              </h4>
              <div class="ai-grid">
                <template v-for="p in g.items" :key="p.id">
                  <!-- «otro» puede tener varios servidores guardados: uno por tarjeta -->
                  <button v-for="s in (p.id === 'custom' ? savedFor('custom') : [])" :key="s.id"
                          type="button" class="ai-card configured" :class="{active: data.active === s.id}"
                          @click="pick(p, s.id)">
                    <span class="ai-card-name">{{ s.provider_name || s.name }}</span>
                    <span class="ai-card-note mono">{{ s.base_url }}</span>
                    <span class="ai-card-mark"><Icon n="check" :t="12" /> {{ data.active === s.id ? 'en uso' : 'configurado' }}</span>
                  </button>
                  <button type="button" class="ai-card"
                          :class="{configured: p.id !== 'custom' && data.profiles[p.id], active: data.active === p.id}"
                          @click="pick(p)">
                    <span class="ai-card-name">{{ p.id === 'custom' ? 'Añadir otro servidor…' : p.name }}</span>
                    <span class="ai-card-note">{{ p.key === 'none' ? 'sin clave' : p.key === 'optional' ? 'clave opcional' : (p.note || 'con clave') }}</span>
                    <span v-if="p.id !== 'custom' && data.profiles[p.id]" class="ai-card-mark">
                      <Icon n="check" :t="12" /> {{ data.active === p.id ? 'en uso' : 'configurado' }}</span>
                  </button>
                </template>
              </div>
            </section>
            <div v-if="!groups.length" class="model-empty">ningún proveedor se llama así. Elige «Añadir otro
              servidor» si tienes una URL compatible con OpenAI.</div>
          </div>
          <div v-if="error && step === 'pick'" class="key-state bad"><Icon n="warning" :t="14" /> {{ error }}</div>
          <div v-if="data" class="ai-catalog-line">
            <span>Catálogo de modelos (models.dev): {{ data.catalog_status.models }} modelos de
              {{ data.catalog_status.providers }} proveedores · comprobado {{ ago(data.catalog_status.checked_at) }}
              <span v-if="data.catalog_status.error" style="color:var(--amber)"> · {{ data.catalog_status.error }}</span></span>
            <button class="btn mini" type="button" :disabled="data.catalog_status.refreshing" @click="load(true)">
              <Icon n="refresh" :t="12" /> Actualizar</button>
          </div>
        </template>

        <!-- ============ paso 2: configurar ============ -->
        <template v-else-if="chosen">
          <h3 class="modal-title">
            <button class="btn mini" type="button" title="Volver a la lista" @click="back"><Icon n="back" :t="13" /></button>
            <Icon :n="GROUP_ICON[chosen.group] || 'cloud'" :t="16" />
            {{ chosen.id === 'custom' ? (form.name || 'Servidor propio') : chosen.name }}
            <button v-if="chosen.docs" class="btn mini" type="button" title="Documentación" @click="openLink(chosen.docs)">
              <Icon n="external" :t="12" /> docs</button>
            <button class="btn mini ai-close" type="button" title="Cerrar" @click="emit('close')"><Icon n="close" :t="13" /></button>
          </h3>
          <div v-if="chosen.note" class="modal-msg">{{ chosen.note }}</div>

          <div class="ai-form">
            <TextField v-if="chosen.id === 'custom'" v-model="form.name" width="100%" label="Nombre"
                       placeholder="Servidor de la iglesia" />
            <template v-for="x in chosen.fields.filter(x => x.name !== 'name')" :key="x.name">
              <TextField v-model="form.fields[x.name]" width="100%" :label="x.label" :placeholder="x.placeholder" />
            </template>

            <div v-if="chosen.key !== 'none'" class="ai-key">
              <TextField v-model="form.key" type="password" width="100%"
                         :label="needsKey ? 'Clave de API' : 'Clave de API (si tu servidor la pide)'"
                         :placeholder="savedProfile?.has_key ? 'guardada: ' + savedProfile.key + ' (escribe otra para cambiarla)' : 'pega aquí la clave'" />
              <button v-if="chosen.key_url" class="btn mini ai-key-link" type="button" @click="openLink(chosen.key_url)">
                <Icon n="key" :t="12" /> Consigue tu clave <Icon n="external" :t="11" /></button>
            </div>

            <TextField v-if="urlEditable" v-model="form.base_url" width="100%" label="URL de la API"
                       :placeholder="chosen.base_url || 'https://mi-servidor/v1'"
                       :hint="chosen.group === 'local' ? 'cambia el puerto si arrancaste el servidor en otro' : ''" />
            <div v-else class="ai-url mono"><Icon n="globe" :t="12" /> {{ shownUrl }}</div>

            <div class="ai-models">
              <div class="ai-models-head">
                <span class="field-label">Modelos</span>
                <span class="ai-list-note">{{ listNote }}</span>
                <button class="btn mini" type="button" :disabled="loadingModels" @click="loadModels()">
                  <Icon n="refresh" :t="12" /> {{ loadingModels ? 'cargando…' : 'Cargar la lista' }}</button>
              </div>
              <ModelPicker v-model="form.chat_model" label="Para conversar (el asistente)" :models="modelList"
                           :suggest="models.suggest?.chat" needTools :loading="loadingModels"
                           hint="necesita un modelo que sepa usar herramientas: es como consulta tu biblioteca" />
              <ModelPicker v-model="form.model" label="Para identificar y rellenar fichas" :models="modelList"
                           :suggest="models.suggest?.fast" :loading="loadingModels"
                           hint="muchas llamadas cortas: aquí compensa el barato y rápido" />
            </div>

            <button type="button" class="ai-advanced-toggle" @click="advanced = !advanced">
              <Icon n="right" :t="12" class="group-chevron" :class="{open: advanced}" /> Avanzado</button>
            <div v-if="advanced" class="ai-advanced">
              <TextField v-model="form.headersText" multiline :rows="2" width="100%" label="Cabeceras extra"
                         placeholder="X-Mi-Cabecera: valor  (una por línea)" />
              <TextField v-model="form.extraText" multiline :rows="3" width="100%" label="Parámetros extra (JSON)"
                         placeholder='{"reasoning_effort": "low"}'
                         hint="se añaden a cada petición tal cual: reasoning_effort, thinking, top_p, lo que admita tu proveedor" />
              <TextField v-model="form.timeout" type="number" width="180px" label="Tiempo máximo (segundos)" />
            </div>

            <div v-if="checkResult" class="key-state" :class="checkResult.ok ? 'ok' : 'bad'">
              <Icon :n="checkResult.ok ? 'check' : 'warning'" :t="14" />
              <span v-if="checkResult.ok">
                Funciona · {{ checkResult.model }} responde en {{ checkResult.latency_ms }} ms
                <span v-if="checkResult.tools_ok"> · el asistente puede usar {{ checkResult.chat_model }}</span>
                <span v-else style="color:var(--amber)"> · {{ checkResult.chat_model }}: {{ checkResult.tools_reason }}</span>
              </span>
              <span v-else>{{ checkResult.reason }}</span>
            </div>
            <div v-if="error" class="key-state bad"><Icon n="warning" :t="14" /> {{ error }}</div>
          </div>

          <div class="btn-row modal-actions ai-actions">
            <button v-if="savedProfile" class="btn" type="button" @click="remove">Quitar</button>
            <span style="flex:1"></span>
            <button class="btn" type="button" :disabled="checking || !!missing" :title="missing" @click="check">
              {{ checking ? 'Probando…' : 'Probar' }}</button>
            <button class="btn primary" type="button" :disabled="saving || !!missing" :title="missing" @click="save">
              {{ saving ? 'Guardando…' : 'Guardar y usar' }}</button>
          </div>
          <div v-if="missing" class="hint" style="text-align:right">{{ missing }}</div>
        </template>
      </div>
    </div>
  </transition>
</template>
