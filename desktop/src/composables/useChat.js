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
import { ref, reactive, computed, watch } from 'vue'
import { api, errorMessage } from '../api.js'
import { ask, useDialog } from './useDialog.js'
import { notify } from './useNotices.js'
import { requireTerms, useTerms } from './useTerms.js'
import { useDownloads } from './useDownloads.js'
import { ensureDownloadFolder } from '../utils/downloadFolder.js'

/** @typedef {import('../api.js').ChatContext} ChatContext */
/** @typedef {import('../api.js').ChatMessage} ChatMessage  un mensaje tal como se pinta y se guarda */
/** @typedef {import('../api.js').ChatConfirm} ChatConfirm */
/** @typedef {import('../api.js').ChatPlan} ChatPlan */
/** @typedef {import('../api.js').ChatSetlist} ChatSetlist */

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

/** Cada cuánto se pregunta por las filas de una tarjeta de descarga que aún busca. */
export const PLAN_POLL_MS = 700

/**
 * Los botones de una tarjeta no hacen caso en este rato tras moverse o
 * cambiar: que una fila que llega o un mensaje nuevo no pongan «Descargar»
 * debajo del dedo justo cuando ibas a pulsar otra cosa.
 */
export const CLICK_GUARD_MS = 500

/** Lo que manda «Volver a pedirlo» en una tarjeta caducada (lo resuelve el núcleo). */
export const ASK_AGAIN = 'vuelve a proponerme la descarga de las que faltan'

/** Cuántas filas del plan se guardan con la conversación. */
const STORE_PLAN_ROWS = 25
/** Cuántas filas de la tabla de una lista se guardan. */
const STORE_SETLIST_ROWS = 60

/**
 * Lo que la app hace por el chat. Por defecto nada: sin app conectada (las
 * pruebas del componente suelto) el chat funciona igual. `go` abre otra
 * página de la app (la de Ajustes, desde la franja del proveedor).
 * @type {{ reload: () => any, action: (a: any) => any, context: () => (ChatContext|null), go: (view: {kind: string}) => any }}
 */
const hooks = { reload: () => {}, action: () => {}, context: () => null, go: () => {} }
const NO_HOOKS = () => ({
  reload: () => {},
  action: () => {},
  context: () => null,
  go: () => {}
})

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
/**
 * La descarga aprobada desde aquí: se sigue y se cuenta al acabar, en LA
 * conversación desde la que se pidió (`chatId`), aunque para entonces haya
 * otra abierta. `id` es el de su tarjeta y `job`, el turno de descarga del
 * núcleo (con él se sabe si la que acaba es la nuestra).
 * @type {import('vue').Ref<{items: string[], force: boolean, request?: string, chatId?: number|null, id?: string, job?: number, playlist?: {id: number, name: string}|null} | null>}
 */
const following = ref(null)
const tryingFree = ref(false)
const freeNote = ref('')
/** Si la página del asistente está a la vista (la insignia y el aviso, si no). */
const visible = ref(false)

/**
 * Lo que se sabe de cada tarjeta de permiso, por su id. No se guarda: el
 * desenlace sale de los mensajes de la app que la citan (`ref`), y si sigue en
 * pie lo dice el servidor (`chatConfirmStatus`). `status` es el del servidor
 * ('' mientras no se sabe); `picks`, las casillas que la persona cambió (las
 * demás siguen lo que diga el servidor); `busy`, que hay un clic en curso.
 * @type {Record<string, {status: string, busy: boolean, picks: Record<number, boolean>, noPlan: boolean}>}
 */
const cards = reactive({})
/** Las tarjetas por las que ya se avisó estando en otra página. */
const warned = new Set()

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
  return () => Object.assign(hooks, NO_HOOKS())
}

// ------------------------------------------------------- texto como dato
// Lo que viene de fuera (títulos de YouTube, canales, nombres) se guarda y se
// pinta como dato: sin saltos de línea, sin controles ni caracteres invisibles
// o que cambian la dirección del texto, y recortado. Es lo mismo que hace
// `safe.datum` en el núcleo; aquí, por si llega algo que no pasó por él.
const INVISIBLE =
  // eslint-disable-next-line no-control-regex -- quitar los caracteres de control es lo que se busca
  /[\x00-\x1f\x7f-\x9f\u{200B}-\u{200F}\u{202A}-\u{202E}\u{2066}-\u{2069}\u{FEFF}]+/gu

/**
 * @param {unknown} text
 * @param {number} [n]  cuántos caracteres como mucho
 */
/**
 * El título de una conversación a partir de su primer mensaje, como lo pone
 * el núcleo (`chats.title_from`): la primera línea con texto, sin las marcas
 * de markdown y de WhatsApp, en 60 letras. Con la lista del domingo pegada
 * salía «agrégame esto…: *Domingo*✅ *1.- Mi…».
 * @param {string} text
 */
export function titleFrom(text) {
  const first =
    String(text || '')
      .split('\n')
      .find((l) => l.trim()) || ''
  const t = first
    .replace(/[*_~`#>]+/g, '')
    .split(/\s+/)
    .filter(Boolean)
    .join(' ')
  return t.length > 60 ? t.slice(0, 59) + '…' : t || 'Conversación'
}

export function datum(text, n = 160) {
  const t = String(text ?? '')
    .replace(INVISIBLE, ' ')
    .replace(/\s+/g, ' ')
    .trim()
  const chars = [...t]
  return chars.length > n ? chars.slice(0, n - 1).join('') + '…' : t
}

/** Solo un número de verdad, o el que se diga. @param {unknown} v @param {any} [or] */
const num = (v, or = 0) => (typeof v === 'number' && Number.isFinite(v) ? v : or)

/** «ver» abre SOLO esto: la dirección canónica de un vídeo de YouTube. */
const CANONICAL_VIDEO = /^https:\/\/www\.youtube\.com\/watch\?v=[A-Za-z0-9_-]{11}$/

/** @param {unknown} url */
export const canonicalVideo = (url) =>
  typeof url === 'string' && CANONICAL_VIDEO.test(url) ? url : ''

/** @param {any} s */
const slimSong = (s) =>
  s && typeof s === 'object'
    ? { id: num(s.id), artist: datum(s.artist, 120), title: datum(s.title, 160) }
    : null

/**
 * El plan de una tarjeta tal como se pinta y se guarda: ≤ 25 filas, sin
 * miniaturas ni nada que no se pinte, cada texto como dato.
 * @param {any} p
 * @returns {ChatPlan}
 */
function slimPlan(p) {
  const items = Array.isArray(p?.items) ? p.items : []
  return {
    state: ['buscando', 'listo', 'parcial', 'error'].includes(p?.state) ? p.state : 'parcial',
    items: items.slice(0, STORE_PLAN_ROWS).map((/** @type {any} */ r, k) => ({
      i: num(r?.i, k),
      n: typeof r?.n === 'number' ? r.n : null,
      query: datum(r?.query, 120),
      kind: datum(r?.kind, 20),
      url: canonicalVideo(r?.url),
      title: datum(r?.title, 120),
      channel: datum(r?.channel, 80),
      duration: num(r?.duration),
      state: datum(r?.state, 20),
      checked: !!r?.checked,
      badge: datum(r?.badge, 40),
      note: datum(r?.note, 200)
    })),
    already: (Array.isArray(p?.already) ? p.already : [])
      .slice(0, STORE_PLAN_ROWS)
      .map((/** @type {any} */ a) => ({
        n: typeof a?.n === 'number' ? a.n : null,
        title: datum(a?.title, 120),
        song: slimSong(a?.song)
      })),
    effect: datum(p?.effect, 200),
    trimmed: num(p?.trimmed),
    reason: datum(p?.reason, 200)
  }
}

/**
 * La tarjeta como se guarda: sin `args` (la interfaz nueva no los usa y no
 * hace falta guardarlos) y con el plan recortado.
 * @param {ChatConfirm} c
 */
function storedConfirm(c) {
  /** @type {Record<string, any>} */
  const out = {
    id: c.id,
    tool: c.tool,
    summary: datum(c.summary, 300),
    effect: datum(c.effect, 300)
  }
  if (c.plan_state) out.plan_state = c.plan_state
  if (c.plan) out.plan = slimPlan(c.plan)
  return out
}

/**
 * La tabla de una lista como se guarda: hasta 60 filas, cada texto como dato.
 * @param {any} s
 * @returns {ChatSetlist}
 */
function slimSetlist(s) {
  const songs = (/** @type {any} */ v, n) =>
    (Array.isArray(v) ? v : []).slice(0, n).map(slimSong).filter(Boolean)
  const ids = (/** @type {any} */ v, n = 300) =>
    (Array.isArray(v) ? v : []).filter((x) => typeof x === 'number').slice(0, n)
  return {
    playlist: {
      id: num(s?.playlist?.id),
      name: datum(s?.playlist?.name, 120),
      created: !!s?.playlist?.created
    },
    mode: ['add', 'set', 'check'].includes(s?.mode) ? s.mode : 'add',
    applied: !!s?.applied,
    rows: (Array.isArray(s?.rows) ? s.rows : [])
      .slice(0, STORE_SETLIST_ROWS)
      .map((/** @type {any} */ r) => ({
        n: typeof r?.n === 'number' ? r.n : null,
        title: datum(r?.title, 160),
        status: datum(r?.status, 20),
        song: slimSong(r?.song),
        alts: songs(r?.alts, 3),
        note: datum(r?.note, 200),
        link: canonicalVideo(r?.link?.url) ? { url: r.link.url } : null,
        in_list: !!r?.in_list,
        kind: datum(r?.kind, 20)
      })),
    added: ids(s?.added),
    already_in: ids(s?.already_in),
    removed: songs(s?.removed, 30),
    extras_in_list: songs(s?.extras_in_list, 60),
    order_differs: !!s?.order_differs,
    previous: Array.isArray(s?.previous) ? ids(s.previous, 500) : null,
    ignored: (Array.isArray(s?.ignored) ? s.ignored : []).slice(0, 10).map((x) => datum(x, 80)),
    truncated: num(s?.truncated),
    mode_note: datum(s?.mode_note, 200),
    ask_note: datum(s?.ask_note, 40)
  }
}

/** Lo que se guarda de un mensaje (sin lo transitorio). @param {ChatMessage} m */
function forStore(m) {
  /** @type {Record<string, any>} */
  const out = { role: m.role, text: m.text }
  for (const k of [
    'tools',
    'app',
    'event',
    'ref',
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
  // la tarjeta y la tabla, para verlas igual al reabrir la conversación (una
  // propuesta sin id es de un núcleo antiguo: no hay tarjeta que guardar)
  if (m.confirm?.id) out.confirm = storedConfirm(m.confirm)
  if (m.setlist) out.setlist = slimSetlist(m.setlist)
  const filled = slimFilled(m.filled)
  if (filled.length) out.filled = filled
  return out
}

/** Las filas que completó una descarga, como se guardan. @param {any} v */
const slimFilled = (v) =>
  (Array.isArray(v) ? v : [])
    .slice(0, STORE_SETLIST_ROWS)
    .map((/** @type {any} */ f) => ({ n: num(f?.n, -1), song: slimSong(f?.song) }))
    .filter((f) => f.n >= 0 && !!f.song?.id)

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

// --------------------------------------------------- la franja del proveedor
// Cuando el proveedor elegido no contesta (sin saldo, clave rechazada,
// saturado, caído) o la última respuesta vino de un respaldo, se dice bajo la
// cabecera, con quién contesta. El texto lo compone la interfaz por el TIPO de
// fallo: nunca se enseña lo que diga el proveedor. Ningún ajuste cambia sin
// un clic de la persona.

/** La última respuesta del modelo que diga quién contestó (si el núcleo no lo dice). */
function lastVia() {
  for (let k = messages.value.length - 1; k >= 0; k--) {
    const m = messages.value[k]
    if (m.role === 'ai' && !m.app && m.via) return m.via
  }
  return null
}

/**
 * Lo que enseña la franja, o null si no hay nada que decir.
 * `primary`: el elegido; `problem`: por qué no contesta ({kind, code, at});
 * `answering`: el respaldo que contesta (o contestó la última vez).
 */
const provider = computed(() => {
  const i = info.value
  if (!i || i.available === false) return null
  const problem = i.primary_problem || null
  // `answering` del núcleo manda (tras «Reintentar» vuelve a null); un
  // núcleo que no lo dice: lo que diga la última respuesta
  const said = 'answering' in i ? i.answering : lastVia()
  const answering = said?.fallback ? said : null
  if (!problem && !answering) return null
  return {
    primary: { name: i.provider || '', model: i.model || '' },
    problem,
    answering
  }
})

/** «Ajustes de IA»: a la página de Ajustes, donde se elige y se arregla. */
function openAiSettings() {
  hooks.go({ kind: 'settings' })
}

/** «Reintentar con el principal»: el siguiente mensaje se prueba con él. */
async function recheckPrimary() {
  try {
    const r = await api.aiRecheck()
    info.value = {
      ...(info.value || {}),
      primary_problem: r?.primary?.problem ?? null,
      answering: r?.answering ?? null
    }
    const name = info.value.provider || 'el principal'
    notify(`El siguiente mensaje irá a ${name}.`, 'ok')
  } catch (e) {
    notify('No se pudo: ' + errorMessage(e))
  }
}

/**
 * «Usar el respaldo como principal»: cambia la IA elegida, y solo si la
 * persona lo confirma en el diálogo, que dice a cuál y con qué modelo.
 */
async function promoteFallback() {
  const p = provider.value
  const a = p?.answering
  if (!a?.id) return
  const before = p.primary.name || 'el que tienes ahora'
  const ok = await inDialogSlot(() =>
    ask({
      kind: 'confirm',
      title: 'Cambiar la IA principal',
      message:
        `A partir de ahora contestará ${a.name} con el modelo ${a.model}. ` +
        `${before} deja de ser la principal: no se borra, y la puedes volver a ` +
        'elegir en Ajustes → Inteligencia artificial.',
      okLabel: `Usar ${a.name}`
    })
  )
  if (!ok) return
  try {
    await api.aiActivate(a.id)
    await refreshInfo()
    notify(`Ahora contesta ${a.name}.`, 'ok')
  } catch (e) {
    notify('No se pudo cambiar: ' + errorMessage(e))
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
  fillSheets(messages.value)
  // las tarjetas que quedaron sin desenlace: ¿siguen en pie? (tras reiniciar
  // la app o pasados 30 minutos, no: caducadas)
  checkCards()
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
  'setlist_sheet',
  'fill_playlist'
]

// ------------------------------------------------------ la cola de mensajes
// Mientras el asistente responde se puede seguir escribiendo: lo que mandas
// queda «en cola» (hasta QUEUE_MAX, en orden) y sale solo al acabar el turno.
// No sale con un diálogo abierto —uno de la app podría traer otro y pisarlo—
// ni delante del aviso de fin de descarga (`queued`), que va siempre primero.

/**
 * ¿Ahora no se puede mandar nada? Respondiendo, con un diálogo abierto o con
 * una pregunta de la app esperando su turno (`inDialogSlot`).
 */
const busy = () => thinking.value || !!dialog.value.open || asking || asks.length > 0

// ------------------------------------------- las preguntas, de una en una
// Solo hay un diálogo, y `ask()` cierra como «cancelado» el que hubiera
// abierto. Lo que pregunta el chat (borrar algo, dónde guardar lo que se baje,
// cambiar la IA) pasa por esta cola: en orden, y nunca encima de otro diálogo,
// que se cerraría solo sin que la persona lo contestara.

/** @typedef {{run: () => any, resolve: (v: any) => void}} AskSlot  una pregunta esperando */
/** @type {AskSlot[]} */
const asks = []
let asking = false

/**
 * Hace `run` (que abre sus diálogos) cuando no haya ningún otro abierto ni
 * esperando delante. Devuelve lo que devuelva `run` (null si falla).
 * @template T
 * @param {() => T | Promise<T>} run
 * @returns {Promise<T|null>}
 */
function inDialogSlot(run) {
  return new Promise((resolve) => {
    asks.push({ run, resolve })
    pumpAsks()
  })
}

function pumpAsks() {
  if (asking || dialog.value.open || !asks.length) return
  const next = /** @type {AskSlot} */ (asks.shift())
  asking = true
  // `run` empieza YA, sin esperar a otra vuelta: el diálogo queda abierto
  // antes de que nadie mire si el chat está libre
  let p
  try {
    p = Promise.resolve(next.run())
  } catch (e) {
    p = Promise.reject(e)
  }
  p.then(
    (v) => settleAsk(next, v),
    () => settleAsk(next, null)
  )
}

/** @param {AskSlot} done @param {any} value */
function settleAsk(done, value) {
  asking = false
  done.resolve(value)
  pumpAsks()
  // sin más preguntas, sale lo que esperaba en la cola
  if (!asking) drain()
}

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

// al cerrarse un diálogo puede salir lo que esperaba por él: primero la
// siguiente pregunta de la app, luego lo que hubiera en la cola
watch(
  () => dialog.value.open,
  (open) => {
    if (open) return
    pumpAsks()
    drain()
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
  // el asistente pide los términos aceptados; los avisos de la app (ocultos)
  // no abren la ventana: sin términos, simplemente no van
  if (hidden ? !useTerms().accepted.value : !(await requireTerms('chat'))) return
  // «sí» o «no» a secas con una tarjeta de descarga esperando: lo atiende la
  // app, por el mismo camino que el clic y sin preguntarle al modelo (que
  // antes volvía a preguntar «¿confirmas?»). Antes que la cola: es la
  // respuesta a lo que tienes delante, no un mensaje más.
  const answer = hidden ? null : cardAnswer(t)
  if (answer) {
    if (fromBox) draft.value = ''
    await answerCard(answer.card, answer.yes, t)
    return
  }
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
  // La respuesta que trae algo que atender (una propuesta): se atiende con
  // el turno ya terminado, para que nada de eso deje el chat parado.
  /** @type {ChatMessage|null} */
  let pending = null
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
    // la tarjeta de permiso y la tabla de la lista van con su respuesta, también
    // si se cortó («Parar») o falló después: lo hecho y lo propuesto se ven
    const cardsOf = await adoptCards(r)
    if (r.error) {
      const m = { role: 'ai', text: r.error, error: true, app: true, ...cardsOf }
      pushMessage(/** @type {ChatMessage} */ (m))
      if (m.confirm) pending = /** @type {ChatMessage} */ (m)
    } else if (r.canceled) {
      const said = live.value?.text || ''
      /** @type {ChatMessage} */
      const m = {
        role: 'ai',
        text: said + (said ? '\n\n' : '') + '_(Respuesta cortada.)_',
        tools: r.tools || [],
        app: !said,
        canceled: true,
        ...cardsOf
      }
      pushMessage(m)
      if (m.confirm) pending = m
      if (m.setlist?.applied) hooks.reload()
    } else {
      /** @type {ChatMessage} */
      const m = {
        role: 'ai',
        text: r.text,
        tools: r.tools || [],
        narrated: !!r.narrated,
        usage: r.usage || null,
        via: r.via || null,
        ...cardsOf
      }
      pushMessage(m)
      if (m.confirm) pending = m
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
    }
    // quién contestó y si el elegido falla: la franja bajo la cabecera
    refreshInfo()
    const c = chats.value.find((x) => x.id === chatId.value)
    if (c && !c.title && !hidden) c.title = titleFrom(t)
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
    // con el turno ya terminado: la propuesta (una tarjeta en línea, o el
    // diálogo para borrar, que queda abierto antes de mirar la cola)
    if (pending) afterTurn(pending)
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

// ------------------------------------------------------- las propuestas
// Lo que no tiene vuelta atrás no lo hace el modelo por su cuenta. El núcleo
// devuelve `confirm` con lo que iba a hacer en vez de hacerlo, registrado en
// el servidor con un id (docs/INTERNAL-CONTRACT.md §3): aceptar es decir que
// sí a ESE id, y lo que se ejecuta lo pone el servidor. Un texto copiado de
// una web o de un título de YouTube no puede borrarte ni bajarte nada.
//
// - Descargar: una tarjeta EN LÍNEA bajo la respuesta (ChatConfirmCard.vue)
//   con las filas que se van rellenando, sus casillas, «Descargar» y «No».
//   No para nada: se puede seguir escribiendo, y un «sí» o un «no» a secas
//   también la contestan.
// - Borrar una canción o una lista: el diálogo de la app, con el turno ya
//   terminado y por la cola de preguntas (nunca encima de otro diálogo).
// - Una propuesta sin id es de un núcleo antiguo: el diálogo de siempre y
//   `(tool, args)`.
//
// Cómo acabó cada tarjeta lo dicen los mensajes de la app que la citan
// (`ref` + `event`); si no hay ninguno, si sigue en pie lo dice el servidor.
// Así se ve igual al reabrir la conversación o tras reiniciar la app.

/** @param {string} id */
function cardInfo(id) {
  if (!cards[id]) cards[id] = { status: '', busy: false, picks: {}, noPlan: false }
  return cards[id]
}

/**
 * Espera a `p` como mucho `ms`; si no llega, null (sin rechazos sueltos).
 * @template T
 * @param {Promise<T>} p @param {number} ms
 * @returns {Promise<T|null>}
 */
function withTimeout(p, ms) {
  /** @type {any} */
  let timer
  return Promise.race([
    p.finally(() => clearTimeout(timer)),
    new Promise((ok) => {
      timer = setTimeout(() => ok(null), ms)
    })
  ])
}

/**
 * Llama al núcleo sin que un fallo (o un núcleo que no tiene esa ruta) pare
 * lo demás.
 * @param {() => any} fn
 */
async function quietly(fn) {
  try {
    return await fn()
  } catch {
    return null
  }
}

/**
 * Lo que trae una respuesta para pintar: la tarjeta y la tabla. Una tarjeta
 * nueva está viva (la acaba de registrar el servidor) y deja sin valor las
 * anteriores de la misma herramienta, como hace él. La de descarga trae ya
 * sus primeras filas: así se guardan con la conversación.
 * @param {import('../api.js').ChatReply} r
 * @returns {Promise<{confirm?: ChatConfirm, setlist?: ChatSetlist}>}
 */
async function adoptCards(r) {
  /** @type {{confirm?: ChatConfirm, setlist?: ChatSetlist}} */
  const out = {}
  if (r.setlist) out.setlist = slimSetlist(r.setlist)
  const c = r.confirm
  if (!c) return out
  if (!c.id) {
    // un núcleo antiguo: el diálogo de siempre, con sus argumentos
    out.confirm = { ...c }
    return out
  }
  /** @type {ChatConfirm} */
  const own = {
    id: c.id,
    tool: c.tool,
    summary: datum(c.summary, 300),
    effect: datum(c.effect, 300)
  }
  if (c.plan_state) own.plan_state = c.plan_state
  for (const m of messages.value) {
    const other = m.confirm
    if (other?.id && other.tool === c.tool && cards[other.id]?.status === 'live') {
      cards[other.id].status = 'superseded'
    }
  }
  cardInfo(c.id).status = 'live'
  if (c.tool === 'download_music') {
    const plan = await withTimeout(Promise.resolve(quietly(() => api.chatPlan(c.id))), 1500)
    if (plan) {
      own.plan = slimPlan(plan)
      own.plan_state = own.plan.state
    }
  }
  out.confirm = own
  return out
}

/**
 * Lo que se hace con una propuesta cuando su turno ya ha terminado.
 * @param {ChatMessage} m
 */
function afterTurn(m) {
  const c = m.confirm
  if (!c) return
  if (!c.id) legacyConfirm(c)
  else if (c.tool !== 'download_music') confirmDanger(m)
  // la de descarga es la tarjeta en línea: se contesta cuando quieras
}

/**
 * Deja un mensaje de la app en la conversación `where`: en la abierta se ve y
 * se guarda; en otra, solo se guarda. Devuelve si fue en la abierta.
 * @param {ChatMessage} m
 * @param {number|null|undefined} [where]
 */
async function postApp(m, where = chatId.value) {
  if (where == null || where === chatId.value) {
    await pushMessage(m)
    return true
  }
  try {
    await api.chatAppend(where, [forStore(m)])
    const c = chats.value.find((x) => x.id === where)
    if (c) c.n = (c.n || 0) + 1
  } catch (e) {
    notify('No se pudo guardar el mensaje: ' + errorMessage(e))
  }
  return false
}

/**
 * Borrar una canción o una lista: el diálogo de la app (fuera del turno y por
 * la cola de preguntas). Se acepta por el id; lo que se borra lo dice el
 * servidor.
 * @param {ChatMessage} m
 */
async function confirmDanger(m) {
  const c = /** @type {ChatConfirm} */ (m.confirm)
  const id = /** @type {string} */ (c.id)
  const where = chatId.value
  const ok = await inDialogSlot(() =>
    ask({
      kind: 'confirm',
      title: 'El asistente quiere hacer esto',
      danger: true,
      message: c.summary,
      okLabel: 'Adelante'
    })
  )
  if (!ok) {
    await postApp(
      {
        role: 'ai',
        text: 'Cancelado, no he tocado nada.',
        app: true,
        ref: id,
        event: 'confirm_declined',
        tools: [{ name: c.tool, summary: 'no lo quiso: no se tocó nada' }]
      },
      where
    )
    return
  }
  try {
    const r = await api.chatConfirm({ id })
    cardInfo(id).status = 'used'
    await postApp(
      {
        role: 'ai',
        text: r.text || 'Hecho.',
        app: true,
        ref: id,
        event: 'confirm_accepted',
        tools: [{ name: c.tool, summary: 'hecho' }]
      },
      where
    )
    hooks.reload()
  } catch (e) {
    await postApp(
      {
        role: 'ai',
        text: 'No se pudo: ' + errorMessage(e),
        error: true,
        app: true,
        ref: id,
        event: 'confirm_failed'
      },
      where
    )
  }
}

/**
 * Lo que pediste antes de esta propuesta (no el «sí» de después): al acabar
 * la descarga, el aviso se lo cita al modelo para que remate ESO.
 * @param {ChatMessage|null} m
 */
function requestFor(m) {
  const list = messages.value
  const at = m ? list.indexOf(m) : -1
  for (let k = (at < 0 ? list.length : at) - 1; k >= 0; k--) {
    const x = list[k]
    if (x.role === 'me' && !x.hidden && !x.event) return x.text
  }
  return ''
}

/**
 * Una propuesta de un núcleo antiguo (sin id): el diálogo de siempre, ahora
 * fuera del turno, y se confirma con `(tool, args)`.
 * @param {ChatConfirm} c
 */
async function legacyConfirm(c) {
  const where = chatId.value
  const answer = await inDialogSlot(async () => {
    const ok = await ask({
      kind: 'confirm',
      title: 'El asistente quiere hacer esto',
      danger: true,
      message: c.summary,
      okLabel: 'Adelante'
    })
    if (!ok) return 'no'
    // la primera vez que se baja algo, dónde guardarlo (utils/downloadFolder.js)
    if (c.tool === 'download_music' && !(await ensureDownloadFolder())) return 'sin carpeta'
    return 'sí'
  })
  if (answer !== 'sí') {
    await postApp(
      {
        role: 'ai',
        text:
          answer === 'sin carpeta'
            ? 'Cancelado: sin carpeta de descargas no he bajado nada.'
            : 'Cancelado, no he tocado nada.',
        app: true
      },
      where
    )
    return
  }
  try {
    const r = await api.chatConfirm(c.tool, c.args || {})
    // con la herramienta apuntada: así el núcleo le cuenta al modelo que la
    // descarga (o el borrado) se pidió y se aceptó de verdad
    await postApp(
      {
        role: 'ai',
        text: r.text || 'Hecho.',
        app: true,
        tools: [
          {
            name: c.tool,
            summary: c.tool === 'download_music' ? 'aceptada, en marcha' : 'hecho'
          }
        ]
      },
      where
    )
    if (c.tool === 'download_music' && r.result?.active) {
      // arrancó en segundo plano: se sigue desde aquí y se cuenta al acabar.
      // Con tu petición original: al terminar, el aviso se la cita al modelo
      // para que remate lo que pediste y no lo que le parezca.
      following.value = {
        items: r.result.items || [],
        force: !!r.result.force,
        request: requestFor(null),
        chatId: where
      }
      rememberFollowing()
      useDownloads().wake()
    } else {
      hooks.reload()
    }
  } catch (e) {
    await postApp(
      { role: 'ai', text: 'No se pudo: ' + errorMessage(e), error: true, app: true },
      where
    )
  }
}

// ----------------------------------------------- la tarjeta de descarga

/**
 * En qué está la tarjeta de descarga de un mensaje: '' si no tiene; si no,
 * live (con botones), checking (se pregunta al servidor), accepted, done,
 * declined, superseded o expired.
 * @param {ChatMessage} m
 */
function cardState(m) {
  const c = m.confirm
  if (!c?.id) return ''
  const list = messages.value
  const at = list.indexOf(m)
  for (let k = list.length - 1; k > at; k--) {
    const x = list[k]
    if (x.ref !== c.id || !x.event) continue
    if (x.event === 'download_done') return 'done'
    if (x.event === 'confirm_accepted') return 'accepted'
    if (x.event === 'confirm_declined') return 'declined'
    break // falló (otra descarga, sin carpeta…): si sigue en pie lo dice el servidor
  }
  const s = cards[c.id]?.status
  if (!s) return 'checking'
  if (s === 'live') return 'live'
  if (s === 'used') return 'accepted'
  if (s === 'superseded') return 'superseded'
  return 'expired'
}

/** La última tarjeta de descarga de la conversación abierta (o null). */
function lastDownloadCard() {
  const list = messages.value
  for (let k = list.length - 1; k >= 0; k--) {
    const c = list[k].confirm
    if (c?.id && c.tool === 'download_music') return list[k]
  }
  return null
}

/**
 * Las tarjetas sin desenlace de la conversación abierta: ¿siguen en pie? Al
 * reabrir una conversación o tras reiniciar la app (el registro del servidor
 * vive en memoria: tras un reinicio, «unknown» = caducada). Con `again`,
 * también las que se creían vivas (al volver a la página).
 */
async function checkCards(again = false) {
  const open = messages.value.filter((m) => {
    if (!m.confirm?.id || m.confirm.tool !== 'download_music') return false
    const state = cardState(m)
    return state === 'checking' || (again && state === 'live')
  })
  await Promise.all(
    open.map(async (m) => {
      const id = /** @type {string} */ (m.confirm?.id)
      const s = await quietly(() => api.chatConfirmStatus(id))
      cardInfo(id).status = s?.state || 'unknown'
    })
  )
}

/**
 * Las filas de la tarjeta, como van ahora (las pide la tarjeta cada poco
 * mientras busca). Devuelve el plan o null.
 * @param {ChatMessage} m
 */
async function refreshPlan(m) {
  const c = m.confirm
  if (!c?.id) return null
  try {
    const p = slimPlan(await api.chatPlan(c.id))
    c.plan = p
    c.plan_state = p.state
    return p
  } catch (e) {
    if (/** @type {any} */ (e)?.status === 404) cardInfo(c.id).noPlan = true
    return null
  }
}

/**
 * Si una fila está marcada: lo que la persona haya tocado o, si no, lo que
 * diga el servidor. Una rechazada nunca.
 * @param {ChatMessage} m @param {import('../api.js').ChatPlanRow} row
 */
function isPicked(m, row) {
  if (row.state === 'rechazada') return false
  const picks = m.confirm?.id ? cards[m.confirm.id]?.picks : null
  return picks && row.i in picks ? !!picks[row.i] : !!row.checked
}

/** @param {ChatMessage} m @param {import('../api.js').ChatPlanRow} row */
function togglePick(m, row) {
  if (!m.confirm?.id || row.state === 'rechazada') return
  cardInfo(m.confirm.id).picks[row.i] = !isPicked(m, row)
}

/**
 * Los índices marcados, o null si la tarjeta no tiene filas (entonces el
 * servidor baja las suyas por defecto).
 * @param {ChatMessage} m
 */
function selectionOf(m) {
  const rows = m.confirm?.plan?.items || []
  if (!rows.length) return null
  return rows.filter((r) => isPicked(m, r)).map((r) => r.i)
}

/**
 * Por qué ya no se puede aceptar (tras un 410): lo que diga el servidor.
 * @param {string} id
 */
async function goneState(id) {
  const s = await quietly(() => api.chatConfirmStatus(id))
  const state = s?.state
  return state && state !== 'live' ? state : 'expired'
}

/**
 * «Descargar» (o un «sí» escrito): se acepta ESA propuesta con las casillas
 * marcadas. La descarga sigue en segundo plano y se cuenta al acabar, en esta
 * conversación.
 * @param {ChatMessage} m
 */
async function acceptCard(m) {
  const c = m.confirm
  if (!c?.id || cardState(m) !== 'live') return
  const id = c.id
  const own = cardInfo(id)
  if (own.busy) return
  own.busy = true
  const where = chatId.value
  try {
    const selection = selectionOf(m)
    if (selection && !selection.length) {
      await postApp(
        {
          role: 'ai',
          text: 'No hay ninguna marcada: marca las que quieras y pulsa «Descargar».',
          app: true
        },
        where
      )
      return
    }
    // la primera vez que se baja algo, dónde guardarlo: por la cola de
    // preguntas, nunca encima de otro diálogo
    const ready = await inDialogSlot(() => ensureDownloadFolder())
    if (!ready) {
      await postApp(
        {
          role: 'ai',
          text: 'Sin carpeta de descargas no he bajado nada. Cuando la elijas, pulsa «Descargar» otra vez.',
          app: true,
          error: true,
          ref: id,
          event: 'confirm_failed'
        },
        where
      )
      return
    }
    const r = await api.chatConfirm(selection ? { id, selection } : { id })
    own.status = 'used'
    const items = Array.isArray(r.result?.items) ? r.result.items : []
    await postApp(
      {
        role: 'ai',
        text: r.text || 'Descargando.',
        app: true,
        ref: id,
        event: 'confirm_accepted',
        // así el modelo sabe, al turno siguiente, que se aceptó y qué se baja
        tools: [
          {
            name: 'download_music',
            summary: 'aceptada, en marcha',
            detail: items.map((x) => datum(x, 80)).join(' | ')
          }
        ]
      },
      where
    )
    if (r.result?.active) {
      following.value = {
        items,
        force: !!r.result.force,
        request: requestFor(m),
        chatId: where,
        id,
        job: num(r.result.job) || undefined,
        playlist: r.result.playlist || null
      }
      rememberFollowing()
      useDownloads().wake()
    } else {
      hooks.reload()
    }
  } catch (e) {
    const status = /** @type {any} */ (e)?.status
    // 410: ya no vale (caducó, hay otra más nueva, ya se atendió): la tarjeta
    // lo dice y ofrece volver a pedirla. Lo demás (otra descarga en marcha,
    // sin yt-dlp) no la gasta: sigue con sus botones.
    if (status === 410) own.status = await goneState(id)
    await postApp(
      {
        role: 'ai',
        text: status === 410 ? errorMessage(e) : 'No se pudo: ' + errorMessage(e),
        error: true,
        app: true,
        ref: id,
        event: 'confirm_failed'
      },
      where
    )
  } finally {
    own.busy = false
  }
}

/**
 * «No» (o un «no» escrito): no se baja nada y no se le pregunta al modelo.
 * @param {ChatMessage} m
 */
async function declineCard(m) {
  const c = m.confirm
  if (!c?.id || cardState(m) !== 'live') return
  const own = cardInfo(c.id)
  if (own.busy) return
  own.busy = true
  try {
    // lo que siguiera buscando en YouTube ya no hace falta
    const id = c.id
    if ((c.plan?.state || c.plan_state) === 'buscando') quietly(() => api.chatPlanCancel(id))
    await postApp({
      role: 'ai',
      text: 'De acuerdo: no bajo nada.',
      app: true,
      ref: c.id,
      event: 'confirm_declined',
      tools: [{ name: 'download_music', summary: 'no la quiso: no se bajó nada' }]
    })
  } finally {
    own.busy = false
  }
}

/**
 * «Dejar de buscar»: lo que falte por buscar en YouTube se queda así; la
 * propuesta sigue en pie con lo resuelto.
 * @param {ChatMessage} m
 */
async function stopSearch(m) {
  const id = m.confirm?.id
  if (!id) return
  await quietly(() => api.chatPlanCancel(id))
  await refreshPlan(m)
}

/** «Volver a pedirlo» en una tarjeta caducada: un mensaje normal al asistente. */
function askAgain() {
  return send(ASK_AGAIN)
}

// --- el «sí» y el «no» escritos (F7)
// Solo un sí o un no A SECAS: «sí, pero quita la 3» es otra cosa y va al modelo.

/** Una palabra en su forma de comparar: sin tildes, en minúsculas y sin letras repetidas. */
const plainWord = (/** @type {string} */ w) =>
  w
    .normalize('NFD')
    .replace(/\p{M}/gu, '')
    .toLowerCase()
    .replace(/([a-z])\1+/g, '$1')

const words = (/** @type {string} */ s) => new Set(s.split(' ').map(plainWord))
const YES_WORDS = words(
  'si ok okay okey okis yes dale vale bueno listo adelante hazlo procede comienza empieza ' +
    'claro venga perfecto confirmo acuerdo va sale correcto exacto bajalas bajala bajalos ' +
    'bajalo descargalas descargala descargalos descargalo'
)
const NO_WORDS = words(
  'no nop nope nel tampoco cancela cancelala cancelalo cancelalas cancelar dejalo dejala ' +
    'dejalas olvidalo olvidala olvidalas'
)
// lo que acompaña a un sí o a un no sin cambiarlo («sí, por favor», «mejor no»)
const FILLER = words(
  'por favor porfa porfavor pues ya todas todos las los la lo eso asi entonces nomas gracias ' +
    'mejor ahora de mas bien muy que y quiero necesito nada bajes descargues hace falta'
)

/**
 * true si es un sí corto, false si es un no corto, null si es otra cosa.
 * @param {string} text
 */
export function shortAnswer(text) {
  const toks = plainWord(String(text || ''))
    .replace(/[^a-z]+/g, ' ')
    .trim()
    .split(' ')
    .filter(Boolean)
  if (!toks.length || toks.length > 6) return null
  let yes = false
  let no = false
  for (const t of toks) {
    if (YES_WORDS.has(t)) yes = true
    else if (NO_WORDS.has(t)) no = true
    else if (!FILLER.has(t)) return null
  }
  if (yes === no) return null
  return yes
}

/**
 * Si lo escrito contesta a la tarjeta de descarga que espera: {card, yes}.
 * @param {string} text
 */
function cardAnswer(text) {
  const yes = shortAnswer(text)
  if (yes === null) return null
  const card = lastDownloadCard()
  if (!card?.confirm?.id || cardState(card) !== 'live' || cards[card.confirm.id]?.busy) {
    return null
  }
  return { card, yes }
}

/**
 * Lo escrito se ve como tuyo (y se guarda) y la tarjeta se contesta por el
 * mismo camino que el clic. El modelo no interviene.
 * @param {ChatMessage} card @param {boolean} yes @param {string} text
 */
async function answerCard(card, yes, text) {
  await pushMessage({ role: 'me', text })
  if (yes) await acceptCard(card)
  else await declineCard(card)
}

/** La tarjeta de descarga que espera respuesta en la conversación abierta, o null. */
const waitingCard = computed(() => {
  const m = lastDownloadCard()
  return m && cardState(m) === 'live' ? m : null
})
/** Hay una tarjeta esperando y la página del asistente no se ve. */
const attention = computed(() => !visible.value && !!waitingCard.value)
watch(attention, (on) => {
  const id = waitingCard.value?.confirm?.id
  if (!on || !id || warned.has(id)) return
  warned.add(id)
  notify('El asistente espera tu visto bueno para descargar: está en «Asistente».', 'info', 10)
})

/** La página del asistente se ve (o deja de verse). @param {boolean} on */
function setVisible(on) {
  visible.value = on
}

// ---------------------------------------------------- la tabla de la lista
// Lo que se hizo con una lista pegada se ve en una tabla bajo la respuesta
// (ChatSetlistCard.vue). Sus botones arreglan la lista sin pasar por el
// modelo y cada uno deja un mensaje de la app que lo cuenta.

/** Lo que entra en una lista: la tenemos (una versión, otra o varias). */
const ADDABLE = ['found', 'other_version', 'ambiguous']

/** Las canciones de la tabla que se tienen, en su orden y sin repetir. @param {ChatSetlist} s */
export function tableIds(s) {
  /** @type {number[]} */
  const out = []
  for (const r of s?.rows || []) {
    const id = r.song?.id
    if (ADDABLE.includes(r.status) && id && !out.includes(id)) out.push(id)
  }
  return out
}

/** Lo que había antes de un arreglo hecho desde la tabla, para «Deshacer». */
const sheetUndo = reactive(new WeakMap())
/** Las tablas con un arreglo en marcha. */
const sheetBusy = reactive(new WeakSet())

/** «Artista - Título». @param {{artist?: string, title?: string}} s */
const songName = (s) => (s.artist ? `${s.artist} - ${s.title || ''}` : s.title || '')

/**
 * «A», «A y B», «A, B y C», «A, B, C, D, E y 3 más».
 * @param {string[]} names @param {number} [more]  cuántas más sin nombre
 */
function joinNames(names, more = 0) {
  const shown = names.filter(Boolean).slice(0, 5)
  const rest = names.filter(Boolean).length - shown.length + more
  if (rest > 0) return `${shown.join(', ')} y ${rest} más`
  if (shown.length < 2) return shown[0] || ''
  return `${shown.slice(0, -1).join(', ')} y ${shown.at(-1)}`
}

/**
 * Pone la tabla al día con cómo quedó la lista: qué está, qué sobra y si el
 * orden es el de la lista pegada.
 * @param {ChatSetlist} s @param {{id: number, artist?: string, title?: string}[]} songs
 */
function settleSheet(s, songs) {
  const order = songs.map((x) => x.id)
  const named = new Set(s.rows.map((r) => r.song?.id).filter(Boolean))
  for (const r of s.rows) r.in_list = !!(r.song && order.includes(r.song.id))
  s.extras_in_list = songs
    .filter((x) => !named.has(x.id))
    .map((x) => slimSong(x))
    .filter((x) => x !== null)
  const seq = tableIds(s)
    .map((id) => order.indexOf(id))
    .filter((p) => p >= 0)
  s.order_differs = seq.some((p, k) => k > 0 && p < seq[k - 1])
}

/**
 * Lo bajado desde la tarjeta de una tabla, en su fila: deja de decir «falta»
 * y pasa a «bajada, en la lista» con su canción. Sale de los avisos de fin de
 * descarga (`filled`, con el `ref` de la tarjeta), así que vale igual en vivo
 * que al reabrir la conversación; repetirlo no cambia nada.
 * @param {ChatMessage[]} list
 */
function fillSheets(list) {
  for (const x of list) {
    if (x.event !== 'download_done' || !x.ref) continue
    const filled = slimFilled(x.filled)
    if (!filled.length) continue
    const s = list.find((y) => y.confirm?.id === x.ref)?.setlist
    if (!s) continue
    for (const f of filled) {
      const r = s.rows.find((y) => y.n === f.n && y.status !== 'ignored')
      if (!r || r.downloaded) continue
      Object.assign(r, {
        status: 'found',
        song: f.song?.title ? f.song : { ...f.song, title: r.title },
        in_list: true,
        downloaded: true
      })
    }
  }
}

/**
 * Lo que se puede hacer con la tabla ahora: {keep, order, undo}.
 * @param {ChatMessage} m
 */
function sheetActions(m) {
  const s = m.setlist
  if (!s?.playlist?.id) return { keep: false, order: false, undo: false }
  const ids = tableIds(s)
  const inSync =
    !s.extras_in_list.length &&
    !s.order_differs &&
    s.rows.every((r) => !ADDABLE.includes(r.status) || r.in_list)
  return {
    keep: ids.length > 0 && (s.extras_in_list.length > 0 || (s.mode === 'check' && !inSync)),
    order: !!s.order_differs,
    undo: !!(sheetUndo.get(m)?.length || s.previous?.length)
  }
}

/**
 * Un botón de la tabla: «Dejar solo estas» (keep), «Ordenar como mi lista»
 * (order) o «Deshacer» (undo). Sin modelo; deja un mensaje que lo cuenta y
 * recarga la lista si está abierta.
 * @param {ChatMessage} m @param {'keep'|'order'|'undo'} what
 */
async function fixList(m, what) {
  const s = m.setlist
  const pid = s?.playlist?.id
  if (!s || !pid || sheetBusy.has(m)) return
  sheetBusy.add(m)
  const where = chatId.value
  const pl = s.playlist.name
  try {
    /** @type {{id: number, artist?: string, title?: string}[]} */
    const before = (await api.playlistSongs(pid))?.songs || []
    const beforeIds = before.map((x) => x.id)
    /** @type {any} */
    let done
    let text
    if (what === 'keep') {
      const ids = tableIds(s)
      done = await api.setPlaylistSongs(pid, ids)
      const gone = before.filter((x) => !ids.includes(x.id)).map(songName)
      text =
        `Dejé «${pl}» solo con ${ids.length === 1 ? 'esta' : `estas ${ids.length}`}, en el orden de tu lista.` +
        (gone.length ? ` Quité ${joinNames(gone)}.` : '')
      sheetUndo.set(m, beforeIds)
    } else if (what === 'order') {
      const ids = tableIds(s).filter((id) => beforeIds.includes(id))
      done = await api.reorderPlaylist(pid, ids)
      text = `Ordené «${pl}» como tu lista.`
      sheetUndo.set(m, beforeIds)
    } else {
      const mine = sheetUndo.get(m)
      const ids = mine?.length ? mine : s.previous || []
      if (!ids.length) return
      done = await api.setPlaylistSongs(pid, ids)
      if (mine?.length) sheetUndo.delete(m)
      else s.previous = null
      text = `Deshice el cambio: «${pl}» vuelve a estar como antes (${ids.length} ${ids.length === 1 ? 'canción' : 'canciones'}).`
    }
    const after = Array.isArray(done?.songs) ? done.songs : null
    if (after) settleSheet(s, after)
    const order = after ? after.map((/** @type {any} */ x) => x.id) : []
    await postApp(
      {
        role: 'ai',
        text,
        app: true,
        tools: [
          {
            name: 'set_playlist_songs',
            summary: `«${pl}»: ${order.length || '?'} canciones`,
            detail: `«${pl}» (id ${pid}) queda así: ids ${order.join(', ')}`
          }
        ]
      },
      where
    )
    hooks.reload()
  } catch (e) {
    await postApp(
      {
        role: 'ai',
        text: `No se pudo cambiar «${pl}»: ${errorMessage(e)}`,
        error: true,
        app: true
      },
      where
    )
  } finally {
    sheetBusy.delete(m)
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

/** El título de una conversación guardada, para decir dónde se contó algo. @param {any} id */
const titleOf = (id) => chats.value.find((c) => c.id === id)?.title || 'otra conversación'

/**
 * Lo que se cuenta al acabar la descarga aprobada, cuando no hubo lista que
 * completar o no se pudo (`after` no llegó o falló). Va en markdown: son
 * listas de temas y se leen mejor con sus negritas. Si entró algo, se le pasa
 * el turno al asistente con un aviso oculto para que remate lo que quedara.
 * @param {any} e       el estado final de la descarga
 * @param {{request?: string, chatId?: number|null, id?: string, playlist?: {id: number, name: string}|null}} [asked]
 */
async function reportDownload(e, asked = {}) {
  const request = asked.request || ''
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
  // la lista que había que completar al terminar no se pudo tocar: se dice
  if (asked.playlist && e.after?.state === 'error') {
    const why = e.after.error ? ` (${datum(e.after.error, 120)})` : ''
    lines.push('', `No pude meterlas en «${asked.playlist.name}»${why}.`)
  }
  const parts = [`${ok.length} descargada${ok.length === 1 ? '' : 's'}`]
  if (already.length) parts.push(`${already.length} ya la${already.length > 1 ? 's' : ''} tenías`)
  if (failed.length) parts.push(`${failed.length} con fallo`)
  /** @type {ChatMessage} */
  const report = {
    role: 'ai',
    text: lines.join('\n').trim(),
    app: true,
    tools: [{ name: 'download_music', summary: parts.join(', ') }]
  }
  // la tarjeta de la que salió queda como «descargada»
  if (asked.id) Object.assign(report, { ref: asked.id, event: 'download_done' })
  const here = await postApp(report, asked.chatId)
  if (ok.length) hooks.reload()
  if (!here) {
    notify(
      `La descarga terminó: te lo cuento en «${titleOf(asked.chatId)}».` +
        (ok.length ? ' Ábrela y pídeme lo que faltara por hacer.' : ''),
      'info',
      10
    )
    return
  }
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

/**
 * Lo que se cuenta cuando la descarga ya completó la lista ella sola
 * (`after.state === 'done'`): qué entró en la lista, qué ya tenías y qué no
 * se pudo, con su motivo. Sin aviso al modelo: no queda nada que rematar.
 * @param {any} e  el estado final de la descarga
 * @param {any} after  lo que hizo el núcleo al terminar
 * @param {{chatId?: number|null, id?: string, playlist?: {id: number, name: string}|null}} asked
 */
async function reportIntoList(e, after, asked) {
  const results = e.results || []
  /** @type {Map<number, string>} */
  const names = new Map()
  /** @type {Map<number, any>} */
  const songs = new Map()
  for (const r of results) {
    if (r.ok && r.id) {
      names.set(r.id, describe(r))
      songs.set(r.id, { id: r.id, artist: r.artist || '', title: r.song || r.title || '' })
    }
    const m = r.already_there ? r.matches?.[0] : null
    if (m?.id && !names.has(m.id)) {
      names.set(m.id, songName(m))
      songs.set(m.id, m)
    }
  }
  // qué fila de la tabla quedó con qué canción (la tabla deja de decir «falta»)
  const filled = slimFilled(
    (Array.isArray(after.placed) ? after.placed : []).map((/** @type {any} */ p) => ({
      n: p?.n,
      song: songs.get(p?.id) || { id: p?.id }
    }))
  )
  const pl = datum(after.playlist?.name || asked.playlist?.name || 'la lista', 120)
  const added = Array.isArray(after.added) ? after.added : []
  const known = added.map((/** @type {number} */ id) => names.get(id)).filter(Boolean)
  const lines = []
  if (e.phase === 'canceled') lines.push('Descarga cancelada.')
  if (added.length) {
    const what = joinNames(/** @type {string[]} */ (known), added.length - known.length)
    lines.push(`${added.length > 1 ? 'Entraron' : 'Entró'} en «${pl}»: ${what}.`)
  } else {
    lines.push(`No entró nada nuevo en «${pl}».`)
  }
  const already = results.filter((/** @type {any} */ r) => r.already_there)
  if (already.length) {
    const them = already.length > 1 ? 'las' : 'la'
    const list = joinNames(already.map((/** @type {any} */ r) => datum(r.title || r.source, 80)))
    lines.push(`${list}: ya ${them} tenías, no ${them} bajé otra vez.`)
  }
  const review = results.filter((/** @type {any} */ r) => r.ok && r.action === 'review')
  if (review.length) {
    lines.push(`${joinNames(review.map(describe))}: en Revisar/, sin artista claro.`)
  }
  const failed = Array.isArray(after.failed) ? after.failed : []
  if (failed.length) {
    const list = failed.map(
      (/** @type {any} */ f) =>
        `${datum(f.label || f.item, 80)} (${datum(f.reason || 'sin motivo conocido', 120)})`
    )
    lines.push(`No se pudo: ${joinNames(list)}.`)
  }
  const skipped = Array.isArray(after.skipped) ? after.skipped : []
  if (skipped.length) {
    const list = skipped.map((/** @type {any} */ s) => datum(s.label || s.item, 80))
    lines.push(`Sin marcar, no las bajé: ${joinNames(list)}.`)
  }
  const ok = results.filter((/** @type {any} */ r) => r.ok)
  const parts = [`${added.length} en «${pl}»`]
  if (ok.length) parts.push(`${ok.length} descargada${ok.length === 1 ? '' : 's'}`)
  if (failed.length) parts.push(`${failed.length} con fallo`)
  // para el modelo, al turno siguiente: los ids exactos de lo que entró
  const detail =
    `entraron en «${pl}» (id ${after.playlist?.id ?? '?'}): ` +
    (added
      .map((/** @type {number} */ id) => `id ${id}${names.has(id) ? ` «${names.get(id)}»` : ''}`)
      .join(', ') || 'ninguna') +
    (failed.length
      ? `; no se pudo: ${failed.map((/** @type {any} */ f) => datum(f.label || f.item, 60)).join(', ')}`
      : '')
  /** @type {ChatMessage} */
  const report = {
    role: 'ai',
    text: lines.join(' '),
    app: true,
    tools: [{ name: 'download_music', summary: parts.join(', '), detail }]
  }
  if (asked.id) Object.assign(report, { ref: asked.id, event: 'download_done' })
  if (asked.id && filled.length) report.filled = filled
  const here = await postApp(report, asked.chatId)
  if (here) fillSheets(messages.value)
  if (added.length || ok.length) hooks.reload()
  if (!here) notify(`La descarga terminó: te lo cuento en «${titleOf(asked.chatId)}».`, 'info', 10)
}

/** Terminó una descarga (la que sea): si es la pedida desde aquí, se cuenta. @param {any} s */
function onDownloadFinished(s) {
  if (!following.value) return
  const asked = following.value
  following.value = null
  rememberFollowing()
  // Solo la descarga que se pidió desde aquí: si mientras tanto se lanzó
  // otra desde la página de Descargas, esa no es nuestra y no se cuenta (con
  // su turno se sabe seguro; sin él, por lo que se pidió).
  if (asked.job && s.job && s.job !== asked.job) return
  const mine = (s.results || []).filter(
    (/** @type {any} */ r) => !r.requested || asked.items.includes(r.requested)
  )
  if (!mine.length && (s.results || []).length) return
  const e = { ...s, results: mine }
  const after = s.after
  const ours = after && (!asked.job || !after.job || after.job === asked.job)
  // la lista ya la completó el núcleo: se cuenta y no hace falta el modelo;
  // si no llegó o falló, el aviso de siempre para que la remate él
  if (ours && after.state === 'done') reportIntoList(e, after, asked)
  else reportDownload(e, asked)
}

/** La burbuja de avance, solo para lo que se pidió desde la conversación abierta. */
const downloading = computed(() => {
  const d = useDownloads().state
  const f = following.value
  if (!f || !d.active) return null
  return f.chatId == null || f.chatId === chatId.value ? d : null
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
  // las tarjetas que esperaban pudieron caducar mientras no se veían
  await Promise.all([refreshInfo(), loadChats(), checkCards(true)])
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
  visible.value = false
  for (const id of Object.keys(cards)) delete cards[id]
  warned.clear()
  asks.length = 0
  asking = false
  Object.assign(hooks, NO_HOOKS())
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
    loadChats,
    // la franja del proveedor
    provider,
    openAiSettings,
    recheckPrimary,
    promoteFallback,
    // las tarjetas de permiso (ChatConfirmCard.vue)
    cards,
    cardState,
    refreshPlan,
    isPicked,
    togglePick,
    selectionOf,
    acceptCard,
    declineCard,
    stopSearch,
    askAgain,
    attention,
    setVisible,
    // la tabla de una lista (ChatSetlistCard.vue)
    sheetActions,
    sheetBusy,
    fixList
  }
}
