// @ts-check
// El asistente: la conversación abierta, la respuesta que está llegando y la
// descarga pedida desde aquí.
//
// Vive fuera del componente a propósito. Estaba dentro de ChatPage y, si
// salías del chat mientras respondía, sus órdenes se perdían: Vue descarta
// los `emit` de un componente desmontado, así que «pon la lista X» y cambiar
// de página dejaba la lista sin sonar (y el aviso de fin de descarga, sin
// contar). Ahora la conversación sigue aunque no se vea, y lo que le toca
// hacer a la app (reproducir, recargar) lo hace quien se conectó con
// `connectChat`, que es la app y no se desmonta nunca.
import { ref, computed, watch } from 'vue'
import { api, errorMessage } from '../api.js'
import { ask, useDialog } from './useDialog.js'
import { notify } from './useNotices.js'
import { useDownloads } from './useDownloads.js'
import { ensureDownloadFolder } from '../utils/downloadFolder.js'

/** @typedef {import('../api.js').ChatContext} ChatContext */

/**
 * Un mensaje tal como se pinta y se guarda.
 * @typedef {Object} ChatMessage
 * @property {'me'|'ai'} role
 * @property {string} text
 * @property {{name: string, summary: string, detail?: string}[]} [tools]
 * @property {boolean} [app]       lo escribió la app, no el modelo
 * @property {string} [event]      un aviso estructural («download_done»)
 * @property {boolean} [hidden]    va al núcleo pero no se pinta
 * @property {boolean} [narrated]  dice haber hecho algo sin herramienta
 * @property {boolean} [error]
 * @property {any} [usage]
 * @property {any} [via]
 * @property {boolean} [canceled]
 */

/** Cada cuánto se pregunta por la respuesta en vivo. */
export const POLL_MS = 250

/** Lo pedido desde aquí, para retomar el seguimiento al volver. */
const FOLLOW_KEY = 'danplay.chat.download'

/** Cuántos mensajes tuyos esperan su turno como mucho mientras el asistente responde. */
export const QUEUE_MAX = 3

/**
 * Cuántos mensajes del historial se mandan al núcleo en cada turno. El núcleo
 * ya recorta a 48 (y por tamaño) y `ChatIn` rechaza más de 200: sin este tope,
 * una conversación larga acababa con cada envío fallando para siempre.
 */
export const CORE_MESSAGES = 60

/** Lo más largo que se deja escribir en la caja (su `maxlength`). */
export const DRAFT_MAX = 50000

/**
 * Lo que la app hace por el chat. Por defecto nada: sin app conectada (las
 * pruebas del componente suelto) el chat funciona igual.
 * @type {{ reload: () => any, action: (a: any) => any, context: () => (ChatContext|null) }}
 */
const hooks = { reload: () => {}, action: () => {}, context: () => null }

const messages = ref(/** @type {ChatMessage[]} */ ([]))
/** Lo escrito en la caja: se conserva al cambiar de página. */
const draft = ref('')
/**
 * Lo que escribiste mientras el asistente respondía: sale solo, en orden, al
 * acabar el turno y sin ningún diálogo abierto. Hasta `QUEUE_MAX` mensajes.
 */
const queue = ref(/** @type {{id: number, text: string}[]} */ ([]))
let queueSeq = 0
const thinking = ref(false)
/** La respuesta que está llegando: el texto según sale y las herramientas según terminan. */
const live = ref(/** @type {{text: string, tools: any[]} | null} */ (null))
/** Qué modelo responde y si la IA está lista (`/api/chat/tools`). */
const info = ref(/** @type {any} */ (null))
const chats = ref(/** @type {any[]} */ ([]))
const chatId = ref(/** @type {number|null} */ (null))
/** La descarga aprobada desde aquí: se sigue y se cuenta al acabar. */
const following = ref(
  /** @type {{items: string[], force: boolean, request?: string} | null} */ (null)
)
const tryingFree = ref(false)
const freeNote = ref('')

let jobId = /** @type {string|null} */ (null)
// El aviso de fin de descarga que no se pudo mandar porque el chat estaba
// ocupado: se manda en cuanto termine ese turno, no se pierde. Va SIEMPRE
// delante de lo que haya en `queue` (ver `drain`).
let queued = /** @type {{text: string, event: string} | null} */ (null)
const { dialog } = useDialog()
// el tope de gasto se avisa una vez por sesión, no en cada respuesta
let warnedBudget = false
/** @type {Promise<void> | null} */
let started = null
/** @type {(() => any) | null} */
let stopFollowing = null

const chatTitle = computed(
  () => chats.value.find((c) => c.id === chatId.value)?.title || 'Nueva conversación'
)

/**
 * La app se conecta al chat: qué hacer con sus órdenes y qué tiene la persona
 * delante. `context` se llama al mandar cada mensaje, no antes: así no depende
 * de lo que cambia cuatro veces por segundo mientras suena algo.
 * @param {Partial<typeof hooks>} h
 * @returns {() => void} cómo desconectarse
 */
export function connectChat(h) {
  Object.assign(hooks, h)
  return () => Object.assign(hooks, { reload: () => {}, action: () => {}, context: () => null })
}

/** Lo que se guarda de un mensaje (sin lo transitorio). @param {ChatMessage} m */
function forStore(m) {
  /** @type {Record<string, any>} */
  const out = { role: m.role, text: m.text }
  for (const k of [
    'tools',
    'app',
    'event',
    'hidden',
    'narrated',
    'error',
    'usage',
    'via',
    'canceled'
  ]) {
    const v = /** @type {any} */ (m)[k]
    if (v !== undefined && v !== null && v !== false && v !== '') out[k] = v
  }
  return out
}

/**
 * Lo que se manda al núcleo de cada mensaje. Las herramientas que usó el
 * asistente van también: con eso el núcleo marca en el historial qué hizo de
 * verdad cada mensaje, y el modelo no se cree sus propias frases («ya la
 * creé») cuando no llamó a nada.
 * @param {ChatMessage} m
 */
function forCore(m) {
  /** @type {Record<string, any>} */
  const out = { role: m.role, text: m.text }
  if (m.tools?.length) {
    out.tools = m.tools.map((h) => {
      /** @type {Record<string, any>} */
      const t = { name: h.name, summary: h.summary }
      // lo que devolvió (ids y nombres): la memoria del modelo entre turnos
      if (h.detail) t.detail = h.detail
      return t
    })
  }
  // lo que escribió la app (un cancelado, un fallo, el arranque de una
  // descarga) no es una frase del modelo: el núcleo lo marca como tal
  if (m.app) out.app = true
  // y un aviso estructural (la descarga terminó) se reconoce por su tipo,
  // no por su texto
  if (m.event) out.event = m.event
  return out
}

/**
 * El historial que se manda al núcleo: los últimos `CORE_MESSAGES` y
 * empezando por uno tuyo, como lo recorta el propio núcleo (que descarta las
 * respuestas del asistente que queden al principio). Copia: el historial
 * sigue creciendo mientras se espera la respuesta.
 */
function historyForCore() {
  const recent = messages.value.slice(-CORE_MESSAGES)
  let from = 0
  while (from < recent.length - 1 && recent[from].role !== 'me') from++
  return recent.slice(from).map(forCore)
}

/** Añade un mensaje y lo guarda en la conversación abierta. @param {ChatMessage} m */
async function pushMessage(m) {
  messages.value.push(m)
  await saveShown(m)
}

/** Guarda en la conversación abierta un mensaje que ya está a la vista. @param {ChatMessage} m */
async function saveShown(m) {
  if (!chatId.value) return
  try {
    await api.chatAppend(chatId.value, [forStore(m)])
  } catch (e) {
    notify('No se pudo guardar el mensaje: ' + errorMessage(e))
  }
}

async function ensureChat() {
  if (chatId.value) return
  const c = await api.chatCreate()
  chats.value.unshift({ ...c, n: 0 })
  chatId.value = c.id
}

async function loadChats() {
  try {
    chats.value = (await api.chats()).chats || []
  } catch {
    chats.value = []
  }
}

async function refreshInfo() {
  try {
    info.value = await api.chatTools()
  } catch {
    /* sin núcleo de IA: la cabecera lo dice */
  }
}

/** @param {number} id */
async function openChat(id) {
  if (thinking.value) return
  const c = await api.chatGet(id)
  if (!c) return
  queue.value = []
  chatId.value = c.id
  messages.value = c.messages || []
}

function newChat() {
  if (thinking.value) return
  queue.value = []
  chatId.value = null
  messages.value = []
  draft.value = ''
}

async function renameChat() {
  if (!chatId.value) return
  const title = await ask({
    kind: 'prompt',
    title: 'Nombre de la conversación',
    value: chatTitle.value,
    okLabel: 'Guardar'
  })
  if (!title) return
  await api.chatRename(chatId.value, title)
  const c = chats.value.find((x) => x.id === chatId.value)
  if (c) c.title = title
}

/** @param {{id: number, title?: string}} c */
async function deleteChat(c) {
  const ok = await ask({
    kind: 'confirm',
    title: 'Borrar la conversación',
    danger: true,
    message: `Se borra «${c.title || 'sin título'}». Tu biblioteca no se toca.`,
    okLabel: 'Borrar'
  })
  if (!ok) return
  await api.chatDelete(c.id)
  chats.value = chats.value.filter((x) => x.id !== c.id)
  if (chatId.value === c.id) {
    queue.value = []
    chatId.value = null
    messages.value = []
  }
}

async function clearChat() {
  if (!messages.value.length) return
  const ok = await ask({
    kind: 'confirm',
    title: 'Borrar la conversación',
    message: 'Se borra el historial de esta conversación. Tu biblioteca no se toca.',
    okLabel: 'Borrar'
  })
  if (!ok) return
  if (chatId.value) {
    try {
      await api.chatDelete(chatId.value)
    } catch (e) {
      notify(errorMessage(e))
      return
    }
    chats.value = chats.value.filter((x) => x.id !== chatId.value)
  }
  queue.value = []
  chatId.value = null
  messages.value = []
}

/** La conversación abierta, al portapapeles como texto. */
async function exportChat() {
  if (!chatId.value) return
  try {
    const { markdown } = await api.chatExport(chatId.value)
    await navigator.clipboard.writeText(markdown)
    notify('Conversación copiada al portapapeles', 'ok')
  } catch (e) {
    notify('No se pudo exportar: ' + errorMessage(e))
  }
}

/**
 * Sin IA configurada, el hueco del chat ofrece probar gratis sin clave: el
 * núcleo activa el primer servicio gratuito que responda.
 */
async function tryFree() {
  tryingFree.value = true
  freeNote.value = ''
  try {
    const r = await api.aiFree()
    freeNote.value = r.free?.ok
      ? `Listo: ${r.free.name} con ${r.free.chat_model}, gratis y con límites.`
      : r.free?.reason || 'ninguno responde ahora mismo'
    info.value = await api.chatTools()
  } catch (e) {
    freeNote.value = errorMessage(e)
  } finally {
    tryingFree.value = false
  }
}

// Las herramientas que cambian la biblioteca. OJO: son los nombres de hoy;
// estaban los viejos en castellano y por eso la lista nunca se refrescaba.
// `download_music` no está: en la conversación solo se PIDE; la biblioteca
// cambia cuando termina la descarga, y eso lo avisa `reportDownload`.
const CHANGES_LIBRARY = [
  'create_playlist',
  'add_to_playlist',
  'set_playlist_songs',
  'rename_playlist',
  'edit_song',
  'set_stars',
  'set_favorite',
  'delete_song',
  'delete_playlist',
  'remove_from_playlist',
  'find_lyrics_and_cover',
  'setlist_sheet'
]

// ------------------------------------------------------ la cola de mensajes
// Mientras el asistente responde se puede seguir escribiendo: lo que mandas
// queda «en cola» (hasta QUEUE_MAX, en orden) y sale solo al acabar el turno.
// No sale con un diálogo abierto —uno de la app podría traer otro y pisarlo—
// ni delante del aviso de fin de descarga (`queued`), que va siempre primero.

/** ¿Ahora no se puede mandar nada? Respondiendo, o con un diálogo abierto. */
const busy = () => thinking.value || !!dialog.value.open

/**
 * Deja un mensaje esperando su turno.
 * @param {string} text
 * @returns {boolean} si cupo; con la cola llena no se pierde nada: se queda en la caja
 */
function enqueue(text) {
  if (queue.value.length >= QUEUE_MAX) {
    notify(`Ya hay ${QUEUE_MAX} mensajes en cola: espera a que acabe la respuesta.`)
    return false
  }
  queue.value.push({ id: ++queueSeq, text })
  return true
}

/** Quita un mensaje de la cola antes de que salga. @param {number} id */
function unqueue(id) {
  queue.value = queue.value.filter((q) => q.id !== id)
}

/**
 * Manda lo siguiente que toque, si se puede: primero el aviso de la app
 * (`queued`) y luego la cola, en el orden en que se escribió. Se llama al
 * acabar cada turno y cuando se cierra un diálogo.
 */
function drain() {
  if (busy()) return
  if (queued) {
    const q = queued
    queued = null
    deliver(q.text, true, q.event)
    return
  }
  const next = queue.value.shift()
  if (next) deliver(next.text, false, null)
}

// al cerrarse un diálogo puede salir lo que esperaba por él
watch(
  () => dialog.value.open,
  (open) => {
    if (!open) drain()
  }
)

/**
 * El mensaje no llegó al núcleo (apagado, reiniciándose, un 4xx): no cuenta
 * como enviado. Lo escrito vuelve a la caja —con lo que esperaba en la cola, en
 * su orden, y sin pisar lo que se haya escrito mientras tanto—: una lista de
 * cuarenta líneas pegada no puede perderse por un fallo de la conexión.
 * @param {ChatMessage} m
 * @param {unknown} e
 */
function unsend(m, e) {
  const i = messages.value.indexOf(m)
  if (i >= 0) messages.value.splice(i, 1)
  const back = [m.text, ...queue.value.map((q) => q.text)]
  queue.value = []
  if (draft.value.trim()) back.push(draft.value)
  draft.value = back.join('\n\n')
  notify(
    'No se pudo enviar el mensaje: ' +
      errorMessage(e) +
      '. Sigue en la caja para que lo reintentes.'
  )
}

/**
 * Manda un mensaje: la puerta de entrada de todo lo que se dice al asistente.
 *
 * Si está respondiendo (o hay un diálogo abierto, o ya hay otros esperando),
 * el mensaje se pone en la cola y sale solo, en su turno. Si no, empieza el
 * turno (`deliver`). La caja se vacía en cuanto se acepta, antes de hablar con
 * el núcleo: si no llega, `unsend` lo devuelve.
 *
 * `hidden`: un mensaje que manda la propia app en nombre del usuario (al
 * terminar una descarga, para que el asistente remate lo que quedaba). Va al
 * núcleo como cualquier otro, pero no se pinta como si lo hubieras escrito.
 * `event` lo etiqueta para el núcleo («download_done»). Ocupado, ese aviso no
 * entra en la cola: espera en su hueco (`queued`) y sale el primero.
 * @param {string|null} [text]  sin texto, lo que haya escrito en la caja
 * @param {boolean} [hidden]
 * @param {string|null} [event]
 */
async function send(text = null, hidden = false, event = null) {
  const fromBox = text === null
  const t = (text ?? draft.value).trim()
  if (!t) return
  if (busy() || (!hidden && queue.value.length)) {
    if (hidden) {
      queued = { text: t, event: event || '' }
      return
    }
    if (!enqueue(t)) return
    if (fromBox) draft.value = ''
    drain()
    return
  }
  if (fromBox) draft.value = ''
  await deliver(t, hidden, event)
}

/**
 * Un turno entero: el mensaje al núcleo y la respuesta en vivo hasta que
 * termina. Solo lo llaman `send` (la puerta) y `drain` (lo que esperaba).
 * @param {string} t  ya recortado y no vacío
 * @param {boolean} hidden
 * @param {string|null} event
 */
async function deliver(t, hidden, event) {
  /** @type {ChatMessage} */
  const mine = { role: 'me', text: t }
  if (hidden) mine.hidden = true
  if (event) mine.event = event
  thinking.value = true
  live.value = { text: '', tools: [] }
  // ¿Lo tiene ya el núcleo? Hasta entonces, todo se puede deshacer.
  let reached = false
  try {
    await ensureChat()
    // Lo que escribes se ve al instante y se guarda cuando el núcleo lo acepta;
    // el aviso de la app, como siempre, se guarda antes.
    if (hidden) await pushMessage(mine)
    else messages.value.push(mine)
    const { id } = await api.chatStart(historyForCore(), hooks.context())
    reached = true
    jobId = id
    if (!hidden) await saveShown(mine)
    let r = null
    for (;;) {
      const d = await api.chatPoll(id)
      live.value = { text: d.text || '', tools: d.tools || [] }
      if (d.done) {
        r = d.result || { error: 'sin respuesta' }
        break
      }
      await new Promise((resolve) => setTimeout(resolve, POLL_MS))
    }
    if (r.error) {
      pushMessage({ role: 'ai', text: r.error, error: true, app: true })
    } else if (r.canceled) {
      const said = live.value?.text || ''
      pushMessage({
        role: 'ai',
        text: said + (said ? '\n\n' : '') + '_(Respuesta cortada.)_',
        tools: r.tools || [],
        app: !said,
        canceled: true
      })
    } else {
      pushMessage({
        role: 'ai',
        text: r.text,
        tools: r.tools || [],
        narrated: !!r.narrated,
        usage: r.usage || null,
        via: r.via || null
      })
      if (r.budget?.over && !warnedBudget) {
        warnedBudget = true
        notify(
          `La IA lleva $${r.budget.month} este mes, por encima del tope de $${r.budget.limit} (Ajustes)`,
          'info'
        )
      }
      if ((r.tools || []).some((/** @type {any} */ h) => CHANGES_LIBRARY.includes(h.name))) {
        hooks.reload()
      }
      // reproducir no se puede hacer desde Python: el núcleo devuelve la orden
      // y la ejecuta la app (aunque el chat ya no esté a la vista)
      for (const a of r.actions || []) hooks.action(a)
      if (r.confirm) await confirmPending(r.confirm)
    }
    const c = chats.value.find((x) => x.id === chatId.value)
    if (c && !c.title && !hidden) c.title = t.slice(0, 60)
  } catch (e) {
    if (!reached && !hidden) {
      unsend(mine, e)
    } else {
      pushMessage({
        role: 'ai',
        text: 'No pude responder: ' + errorMessage(e),
        error: true,
        app: true
      })
    }
  } finally {
    thinking.value = false
    live.value = null
    jobId = null
    // lo que esperaba su turno: primero el aviso de la app, luego la cola
    drain()
  }
}

/** Corta la respuesta que está llegando. Lo escrito hasta ahí se queda. */
async function cancel() {
  if (!jobId) return
  try {
    await api.chatCancel(jobId)
  } catch {
    /* ya había terminado */
  }
}

/**
 * Lo que no tiene vuelta atrás no lo hace el modelo por su cuenta.
 *
 * El núcleo devuelve `confirm` con lo que iba a hacer en vez de hacerlo
 * (docs/CONTRATO-INTERNO.md §3): borrar una canción, borrar un repertorio o
 * descargar. Aquí se pregunta, y solo si dices que sí se ejecuta. Un texto
 * copiado de una página web o de un título de YouTube no puede borrarte nada.
 * @param {import('../api.js').ChatConfirm} pending
 */
async function confirmPending(pending) {
  const ok = await ask({
    kind: 'confirm',
    title: 'El asistente quiere hacer esto',
    danger: true,
    message: pending.summary,
    okLabel: 'Adelante'
  })
  if (!ok) {
    pushMessage({ role: 'ai', text: 'Cancelado, no he tocado nada.', app: true })
    return
  }
  // la primera vez que se baja algo, dónde guardarlo (utils/downloadFolder.js)
  if (pending.tool === 'download_music' && !(await ensureDownloadFolder())) {
    pushMessage({
      role: 'ai',
      text: 'Cancelado: sin carpeta de descargas no he bajado nada.',
      app: true
    })
    return
  }
  try {
    const r = await api.chatConfirm(pending.tool, pending.args)
    // con la herramienta apuntada: así el núcleo le cuenta al modelo que la
    // descarga (o el borrado) se pidió y se aceptó de verdad
    pushMessage({
      role: 'ai',
      text: r.text || 'Hecho.',
      app: true,
      tools: [
        {
          name: pending.tool,
          summary: pending.tool === 'download_music' ? 'aceptada, en marcha' : 'hecho'
        }
      ]
    })
    if (pending.tool === 'download_music' && r.result?.active) {
      // arrancó en segundo plano: se sigue desde aquí y se cuenta al acabar.
      // Con tu petición original: al terminar, el aviso se la cita al modelo
      // para que remate lo que pediste y no lo que le parezca.
      const request =
        [...messages.value].reverse().find((m) => m.role === 'me' && !m.hidden)?.text || ''
      following.value = { items: r.result.items || [], force: !!r.result.force, request }
      rememberFollowing()
      useDownloads().wake()
    } else {
      hooks.reload()
    }
  } catch (e) {
    pushMessage({ role: 'ai', text: 'No se pudo: ' + errorMessage(e), error: true, app: true })
  }
}

// -------------------------------------------------- la descarga pedida aquí
// Corre en el núcleo, en segundo plano. El chat la sigue —el mismo estado que
// enseñan Descargas y la barra lateral, vía useDownloads— y, al terminar,
// cuenta qué entró, qué ya tenías y qué falló. Lo pedido se guarda en
// localStorage: una descarga tarda minutos, y si la ventana se recarga
// mientras tanto, al volver se retoma el seguimiento.

function rememberFollowing() {
  try {
    if (following.value) localStorage.setItem(FOLLOW_KEY, JSON.stringify(following.value))
    else localStorage.removeItem(FOLLOW_KEY)
  } catch {
    /* sin almacenamiento (modo privado) */
  }
}

/** Una fila del resultado, legible: con lo que quedó archivada. @param {any} r */
function describe(r) {
  const name = r.song || r.title || r.source || r.requested || 'un tema'
  const who = r.artist ? `${r.artist} - ` : ''
  return `${who}${name}`
}

// Cómo se identificó, en palabras. El nombre con el que entra una descarga lo
// decide esto, no el título de YouTube; si no se dice, la persona (y el
// modelo) creen que se bajó otra canción.
/** @type {Record<string, string>} */
const IDENTIFIED = {
  tags: 'por sus etiquetas',
  fingerprint: 'por huella acústica',
  heuristic: 'por el nombre',
  ai: 'por la IA',
  none: 'sin identificar'
}
/** @param {any} r */
function requestedAs(r) {
  const asked = (r.source || r.title || '').trim()
  const filed = describe(r)
  if (!asked || asked.toLowerCase() === filed.toLowerCase()) return ''
  const how = IDENTIFIED[r.identified_by] ? ` (${IDENTIFIED[r.identified_by]})` : ''
  return ` — pediste «${asked}»; se archivó con ese nombre${how}`
}

/**
 * Lo que se cuenta al acabar la descarga aprobada. Va en markdown: son listas
 * de temas y se leen mejor con sus negritas.
 * @param {any} e       el estado final de la descarga
 * @param {string} [request]  lo que pediste, para citárselo al modelo
 */
function reportDownload(e, request = '') {
  const results = e.results || []
  const ok = results.filter((/** @type {any} */ r) => r.ok)
  const already = results.filter((/** @type {any} */ r) => r.already_there)
  const failed = results.filter((/** @type {any} */ r) => !r.ok && !r.already_there)
  const lines = []
  if (e.phase === 'canceled') lines.push('Descarga cancelada.')
  if (ok.length) {
    lines.push(ok.length > 1 ? `**Descargadas (${ok.length}):**` : '**Descargada:**')
    for (const r of ok) {
      let where = ''
      if (r.action === 'review') where = ' → en *Revisar/*, sin artista claro'
      else if (r.forced) where = ' (otra versión)'
      lines.push(`- **${describe(r)}**${where}${requestedAs(r)}`)
    }
  }
  if (already.length) {
    lines.push(
      '',
      already.length > 1
        ? '**Ya las tenías, no las he bajado:**'
        : '**Ya la tenías, no la he bajado:**'
    )
    for (const r of already) {
      const m = r.matches?.[0]
      const as = m ? ` — en tu biblioteca como «${m.artist ? m.artist + ' - ' : ''}${m.title}»` : ''
      lines.push(`- ${r.title || r.source || 'un tema'}${as}`)
    }
    lines.push(
      '',
      already.length > 1
        ? 'Si las quieres igualmente como otra versión, dímelo y las bajo.'
        : 'Si la quieres igualmente como otra versión, dímelo y la bajo.'
    )
  }
  if (failed.length) {
    lines.push('', '**No se pudo:**')
    for (const r of failed) lines.push(`- ${describe(r)}: ${r.reason || 'sin motivo conocido'}`)
  }
  if (!results.length && e.phase !== 'canceled') {
    lines.push('No se ha bajado nada.' + (e.error ? ` ${e.error}` : ''))
  }
  const parts = [`${ok.length} descargada${ok.length === 1 ? '' : 's'}`]
  if (already.length) parts.push(`${already.length} ya la${already.length > 1 ? 's' : ''} tenías`)
  if (failed.length) parts.push(`${failed.length} con fallo`)
  pushMessage({
    role: 'ai',
    text: lines.join('\n').trim(),
    app: true,
    tools: [{ name: 'download_music', summary: parts.join(', ') }]
  })
  if (ok.length) hooks.reload()
  // Si entró algo, se le pasa el turno al asistente: «descárgame estas y
  // ármame una lista» se quedaba a medias, porque él no se entera solo de que
  // la descarga acabó y la persona tenía que volver a pedírselo.
  //
  // Con los ids EXACTOS de lo que entró. Sin ellos, el modelo se los
  // inventaba y la lista salía con otras canciones.
  if (ok.length) {
    const ids = ok
      .filter((/** @type {any} */ r) => r.id)
      .map((/** @type {any} */ r) => {
        const asked = (r.source || r.title || '').trim()
        return `pediste «${asked || r.requested || '?'}» → entro como «${describe(r)}» (id ${r.id})`
      })
      .join('; ')
    const notice =
      '[aviso de la app] La descarga ha terminado y ES la que pediste; el nombre con ' +
      'el que entra lo decide la identificacion, no YouTube. ' +
      (ids || 'No entro ninguna con id.') +
      '. Usa EXACTAMENTE esos ids y NO la vuelvas a descargar. ' +
      (request ? `Lo que te pedi fue: «${request.slice(0, 300)}». ` : '') +
      'Si de eso quedaba algo por hacer con esas canciones (añadirla a una lista, ' +
      'ponerla a sonar…), hazlo AHORA con las herramientas —añadir a una lista no necesita ' +
      'confirmacion— y cuentamelo en una linea; no toques nada mas. ' +
      'Si no quedaba nada, responde solo: Terminado.'
    if (thinking.value) queued = { text: notice, event: 'download_done' }
    else send(notice, true, 'download_done')
  }
}

/** Terminó una descarga (la que sea): si es la pedida desde aquí, se cuenta. @param {any} s */
function onDownloadFinished(s) {
  if (!following.value) return
  const asked = following.value
  following.value = null
  rememberFollowing()
  // Solo la descarga que se pidió desde aquí: si mientras tanto se lanzó
  // otra desde la página de Descargas, esa no es nuestra y no se cuenta.
  const mine = (s.results || []).filter(
    (/** @type {any} */ r) => !r.requested || asked.items.includes(r.requested)
  )
  if (!mine.length && (s.results || []).length) return
  reportDownload({ ...s, results: mine }, asked.request || '')
}

/** La burbuja de avance, solo para lo que se pidió desde aquí. */
const downloading = computed(() => {
  const d = useDownloads().state
  return following.value && d.active ? d : null
})

/**
 * La primera vez que se abre el chat: la IA que hay, las conversaciones, lo
 * que hubiera en el localStorage de versiones anteriores y la descarga que
 * se estuviera siguiendo. Una sola vez; lo que pasa después ya lo sabe.
 */
function start() {
  if (started) return started
  started = (async () => {
    stopFollowing = useDownloads().onFinished(onDownloadFinished)
    await refreshInfo()
    await loadChats()
    // La conversación que vivía en el localStorage (versiones anteriores)
    // pasa a la base una sola vez, para no perderla.
    let old
    try {
      old = JSON.parse(localStorage.getItem('danplay.chat') || 'null')
    } catch {
      old = null
    }
    if (Array.isArray(old) && old.length) {
      try {
        const c = await api.chatCreate('Conversación anterior')
        await api.chatAppend(c.id, old.map(forStore))
        localStorage.removeItem('danplay.chat')
        await loadChats()
      } catch {
        /* se intenta la próxima vez */
      }
    }
    if (chats.value.length && !chatId.value && !messages.value.length) {
      await openChat(chats.value[0].id)
    }
    // una descarga pedida desde aquí que seguía en marcha al recargar
    try {
      const pending = localStorage.getItem(FOLLOW_KEY)
      if (pending) {
        following.value = JSON.parse(pending)
        useDownloads().wake()
      }
    } catch {
      /* sin almacenamiento (modo privado) */
    }
  })()
  return started
}

/**
 * Cada vez que el chat se pone a la vista: lo que haya cambiado fuera (la IA
 * elegida en Ajustes, conversaciones nuevas). La conversación abierta sigue.
 */
async function show() {
  if (!started) return start()
  await Promise.all([refreshInfo(), loadChats()])
}

/** Vuelve al estado inicial. Solo para pruebas. */
export function resetChat() {
  stopFollowing?.()
  stopFollowing = null
  started = null
  jobId = null
  queued = null
  queue.value = []
  queueSeq = 0
  warnedBudget = false
  messages.value = []
  draft.value = ''
  thinking.value = false
  live.value = null
  info.value = null
  chats.value = []
  chatId.value = null
  following.value = null
  tryingFree.value = false
  freeNote.value = ''
  Object.assign(hooks, { reload: () => {}, action: () => {}, context: () => null })
}

export function useChat() {
  return {
    messages,
    draft,
    queue,
    thinking,
    live,
    info,
    chats,
    chatId,
    chatTitle,
    following,
    downloading,
    tryingFree,
    freeNote,
    start,
    show,
    send,
    unqueue,
    cancel,
    openChat,
    newChat,
    renameChat,
    deleteChat,
    clearChat,
    exportChat,
    tryFree,
    loadChats
  }
}
