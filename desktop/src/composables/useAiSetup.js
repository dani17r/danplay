// @ts-check
// Elegir y ajustar la IA: lo que comparten la tarjeta «Inteligencia
// artificial» de Ajustes y el panel de opciones del asistente.
//
// Estaba solo en Ajustes, y para cambiar de modelo o pegar una clave había
// que salir del chat. Ahora los dos usan esto: los mismos perfiles, el mismo
// modal (AiProviderModal) y las mismas llamadas al núcleo. Cada página tiene
// su copia del estado (no es global): Ajustes vuelve a pedirlo al entrar y el
// chat al abrir su panel.
//
// Las claves no pasan por aquí: el núcleo las devuelve enmascaradas y, al
// guardar sin escribir otra, conserva la guardada (y solo la manda a la URL
// con la que se guardó). Cambiar de modelo manda solo el modelo.
import { ref, computed } from 'vue'
import { api, errorMessage } from '../api.js'
import { notify } from './useNotices.js'

/**
 * Lo que se le manda a `/api/ai/models` para el perfil activo: lo guardado,
 * como lo mandaría el formulario del modal sin tocar nada (sin clave: el
 * núcleo pone la guardada si la URL no cambió). `null` si el activo no tiene
 * perfil guardado (viene del entorno, `DANPLAY_AI_*`).
 * @param {any} info  lo que devuelve `api.aiProviders()`
 * @returns {import('../api.js').AiProfile | null}
 */
export function activeDraft(info) {
  const p = info?.profiles?.[info?.active]
  if (!p) return null
  const entry = info.catalog?.find((/** @type {any} */ c) => c.id === p.provider)
  /** @type {import('../api.js').AiProfile} */
  const d = {
    id: p.id,
    provider: p.provider,
    model: p.model || '',
    chat_model: p.chat_model || '',
    fields: { ...(p.fields || {}) },
    headers: { ...(p.headers || {}) },
    extra: { ...(p.extra || {}) },
    timeout: Number(p.timeout) || 60
  }
  // como el modal: la URL solo si no es la del catálogo (o es un servidor
  // propio, que no tiene). Sin ella, uno con otro puerto se quedaría sin su
  // clave: el núcleo no la manda a una dirección distinta de la guardada.
  if (p.base_url && (p.provider === 'custom' || p.base_url !== entry?.base_url))
    d.base_url = p.base_url
  return d
}

/**
 * @param {{ onSaved?: (r: any) => (void | Promise<void>) }} [opts]
 *   `onSaved`: lo que la página hace además cuando cambia algo (Ajustes
 *   vuelve a leer sus ajustes; el chat, quién le contesta).
 */
export function useAiSetup({ onSaved } = {}) {
  /** Lo que devuelve `/api/ai/providers`: catálogo, perfiles y el activo. */
  const aiInfo = ref(/** @type {any} */ (null))
  const aiModal = ref(false)
  const aiInitial = ref('')
  const aiProfiles = computed(() => Object.values(aiInfo.value?.profiles || {}))
  const activeAi = computed(() => aiInfo.value?.active_profile || null)
  /** El perfil guardado del activo (con la clave enmascarada), o null. */
  const activeSaved = computed(() => aiInfo.value?.profiles?.[aiInfo.value?.active] || null)
  /** De qué perfil es la lista de modelos que hay (ver `loadModels`). */
  let modelsOf = ''

  async function loadAi() {
    try {
      aiInfo.value = await api.aiProviders()
    } catch {
      /* sin núcleo de IA: quien lo enseña lo dice */
    }
  }

  /** El modal, en la rejilla o directamente en el formulario de `initial`. */
  function openAi(initial = '') {
    aiInitial.value = initial
    aiModal.value = true
  }

  /**
   * Lo que devuelve el núcleo tras guardar, activar o probar gratis. La
   * lista de modelos se vuelve a pedir (otra clave u otro proveedor), salvo
   * `keepModels`: cambiar de modelo no cambia qué modelos hay.
   */
  async function afterAiSaved(/** @type {any} */ r, keepModels = false) {
    aiInfo.value = r
    if (!keepModels) modelsOf = ''
    await onSaved?.(r)
  }

  async function activateAi(/** @type {string} */ id) {
    if (id === aiInfo.value?.active) return
    try {
      await afterAiSaved(await api.aiActivate(id))
    } catch (e) {
      notify(errorMessage(e))
    }
  }

  // «Probar gratis, sin clave»: el núcleo prueba los servicios gratuitos por
  // orden y activa el primero que responda. Tarda lo que tarden en contestar.
  const tryingFree = ref(false)
  const freeResult = ref(/** @type {any} */ (null))
  async function tryFree() {
    tryingFree.value = true
    freeResult.value = null
    try {
      const r = await api.aiFree()
      freeResult.value = r.free
      await afterAiSaved(r)
    } catch (e) {
      freeResult.value = { ok: false, reason: errorMessage(e) }
    } finally {
      tryingFree.value = false
    }
  }

  // ---- los modelos del activo, para elegirlos sin abrir el modal
  /** Lo que devuelve `/api/ai/models` para el activo, y de qué perfil es. */
  const models = ref(
    /** @type {{ok: boolean, models: any[], catalog: any[], suggest: any, hidden?: any[], reason?: string}} */ ({
      ok: false,
      models: [],
      catalog: [],
      suggest: {}
    })
  )
  const loadingModels = ref(false)
  /** La lista que ven los selectores: la del proveedor si respondió; si no, el catálogo. */
  const modelList = computed(
    () => (models.value.ok ? models.value.models : models.value.catalog) || []
  )

  /**
   * Pide la lista del activo. Una vez por perfil: el núcleo prueba cada
   * modelo con tu clave (lo recuerda el día), y abrir y cerrar el panel no
   * tiene por qué repetirlo. `again` la vuelve a pedir.
   */
  async function loadModels(again = false) {
    const d = activeDraft(aiInfo.value)
    if (!d || (!again && modelsOf === d.id)) return
    modelsOf = d.id
    loadingModels.value = true
    try {
      models.value = await api.aiModels(d)
    } catch (e) {
      models.value = { ok: false, models: [], catalog: [], suggest: {}, reason: errorMessage(e) }
    } finally {
      loadingModels.value = false
    }
  }

  /**
   * Cambia un modelo del activo: `chat_model` (el de la conversación) o
   * `model` (el rápido, para identificar). Solo viaja eso: el núcleo conserva
   * la clave, la URL y lo demás (`providers::save_profile`).
   * @param {'chat_model' | 'model'} role
   * @param {string} id
   */
  const savingModel = ref(false)
  async function setModel(role, id) {
    const p = activeSaved.value
    const name = (id || '').trim()
    if (!p || !name || name === (activeAi.value?.[role] || '')) return false
    savingModel.value = true
    try {
      await afterAiSaved(
        await api.aiSaveProfile({ id: p.id, provider: p.provider, [role]: name, activate: true }),
        true
      )
      return true
    } catch (e) {
      notify('No se pudo cambiar el modelo: ' + errorMessage(e))
      return false
    } finally {
      savingModel.value = false
    }
  }

  return {
    aiInfo,
    aiModal,
    aiInitial,
    aiProfiles,
    activeAi,
    activeSaved,
    loadAi,
    openAi,
    afterAiSaved,
    activateAi,
    tryingFree,
    freeResult,
    tryFree,
    models,
    modelList,
    loadingModels,
    loadModels,
    savingModel,
    setModel
  }
}
