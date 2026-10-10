import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'

// Lo que el asistente enseña además del texto: la tarjeta de permiso para
// descargar (en línea, sin diálogo que pare el chat), la tabla de una lista
// metida o comprobada, la franja del proveedor cuando contesta un respaldo y
// los flujos que antes congelaban la conversación (un diálogo DENTRO del
// turno) o dependían de otra vuelta del modelo (completar la lista al acabar
// la descarga).
const held = vi.hoisted(() => ({
  state: null,
  api: null,
  app: null,
  replies: [],
  tools: null,
  yt: []
}))

vi.mock('../src/api.js', async (importOriginal) => {
  const actual = await importOriginal()
  const { createState, buildApiDouble } = await import('./support/backend.js')
  const { vi: v } = await import('vitest')
  held.state = createState()
  held.api = buildApiDouble(actual, held.state)
  held.api.chatTools = v.fn(async () => ({ ...held.tools }))
  // cada turno contesta lo siguiente que la prueba haya puesto en `replies`
  // (una función para dejarlo pendiente hasta que la prueba lo suelte)
  held.api.chatStart = v.fn(async () => ({ id: 'turno' }))
  held.api.chatPoll = v.fn(async () => {
    const next = held.replies.shift() || { text: 'vale', tools: [] }
    const result = typeof next === 'function' ? await next() : next
    return { text: result.text || '', tools: result.tools || [], done: true, result }
  })
  held.api.youtube = v.fn(async () => (held.yt.length > 1 ? held.yt.shift() : held.yt[0]))
  held.app = { ...actual.app, openInBrowser: v.fn(async () => {}) }
  return { ...actual, api: held.api, app: held.app, inTauri: true }
})

import ChatPage from '../src/components/ChatPage.vue'
import { dialogOk, dialogCancel, ask, useDialog } from '../src/composables/useDialog.js'
import { resetDownloads } from '../src/composables/useDownloads.js'
import { clearNotices, useNotices } from '../src/composables/useNotices.js'
import {
  connectChat,
  resetChat,
  useChat,
  shortAnswer,
  datum,
  ASK_AGAIN,
  CLICK_GUARD_MS
} from '../src/composables/useChat.js'
import { song } from './support/backend.js'

const hooks = { reload: vi.fn(), action: vi.fn(), go: vi.fn() }
const URL1 = 'https://www.youtube.com/watch?v=AAAAAAAAAAA'
const URL2 = 'https://www.youtube.com/watch?v=BBBBBBBBBBB'

beforeEach(() => {
  vi.useFakeTimers()
  localStorage.clear()
  vi.clearAllMocks()
  dialogCancel()
  resetDownloads()
  resetChat()
  clearNotices()
  connectChat(hooks)
  held.replies = []
  held.yt = [{ available: true, active: false, results: [], after: null, job: 0 }]
  held.tools = { model: 'm', provider: 'P', available: true, reason: '', tools: [] }
  Object.assign(held.state, {
    chats: [],
    chatConfirms: {},
    chatPlans: {},
    songs: [],
    playlistSongs: []
  })
})
afterEach(() => {
  vi.useRealTimers()
})

const montar = async () => {
  const w = mount(ChatPage, { attachTo: document.body })
  await flushPromises()
  await flushPromises()
  return w
}
const escribir = async (w, t) => {
  await w.find('.chat-foot textarea').setValue(t)
  await w.find('.chat-foot .btn').trigger('click')
  await flushPromises()
  await flushPromises()
}
/** Deja pasar el rato en que la tarjeta no hace caso de los clics. */
const quieta = () => vi.advanceTimersByTimeAsync(CLICK_GUARD_MS + 50)
/** Lo que se le mandó al núcleo en el último turno. */
const enviado = () => held.api.chatStart.mock.calls.at(-1)?.[0] || []

/** Una fila del plan tal como la manda el servidor. */
function fila(i, extra = {}) {
  return {
    i,
    n: i + 2,
    query: `Tema ${i}`,
    kind: 'texto',
    url: '',
    title: '',
    channel: '',
    duration: 0,
    state: 'buscando',
    checked: true,
    badge: '',
    note: '',
    ...extra
  }
}

/** Una propuesta de descarga registrada (en el doble) con su plan. */
function propuesta(id = 'k1', plan = {}) {
  held.state.chatConfirms[id] = 'live'
  held.state.chatPlans[id] = {
    state: 'buscando',
    items: [fila(0), fila(1)],
    already: [
      { n: 1, title: 'Digno De Adorar', song: { id: 142, artist: 'Autor', title: 'Digno' } }
    ],
    unresolved: [],
    effect: 'Al terminar: añado 2 a «domingo»',
    trimmed: 0,
    reason: '',
    ...plan
  }
  return {
    id,
    tool: 'download_music',
    summary: 'Descargar 2 temas de YouTube',
    effect: 'Al terminar: añado 2 a «domingo»',
    plan_state: 'buscando',
    args: { items: ['tema 0', 'tema 1'] }
  }
}

/** Lo que pasa en el núcleo cuando YouTube contesta: las filas resueltas. */
function resuelto(id = 'k1') {
  Object.assign(held.state.chatPlans[id], {
    state: 'listo',
    items: [
      fila(0, {
        state: 'encontrada',
        url: URL1,
        title: 'Tema 0 (En Vivo)',
        channel: 'Canal Uno',
        duration: 245
      }),
      fila(1, {
        state: 'dudosa',
        checked: false,
        url: URL2,
        title: 'Otra cosa',
        channel: 'Canal Dos',
        duration: 30,
        badge: 'no se parece',
        note: 'dura muy poco'
      })
    ]
  })
}

/** La respuesta del asistente con la tarjeta (y la tabla de la lista). */
const conTarjeta = (confirm, extra = {}) => ({
  text: 'Añadí a «domingo»: Digno De Adorar. Faltan 2: abajo te pido permiso para bajarlas.',
  tools: [{ name: 'fill_playlist', summary: '«domingo»: 1 de 3', detail: 'n=1 id142' }],
  confirm,
  ...extra
})

describe('la tarjeta de permiso para descargar', () => {
  it('sale en línea, se rellena sola, descarga lo marcado y al acabar cuenta lo que entró en la lista', async () => {
    held.replies.push(conTarjeta(propuesta()))
    const w = await montar()
    await escribir(w, 'mete esta lista en domingo')

    // el turno ya terminó: se puede seguir escribiendo, no hay diálogo
    expect(useChat().thinking.value).toBe(false)
    expect(useDialog().dialog.value.open).toBe(false)
    const card = () => w.find('.chat-msg.ai .cc')
    expect(card().exists()).toBe(true)
    expect(card().text()).toContain('Descargar 2 temas de YouTube')
    expect(card().text()).toContain('buscando en YouTube')
    expect(card().findAll('.cc-row')).toHaveLength(2)
    expect(card().text()).toContain('Ya estaba en tu biblioteca: Autor - Digno')
    expect(card().text()).toContain('Al terminar: añado 2 a «domingo»')
    // sin foco automático: lo que escribías sigue donde estaba
    expect(card().find('[autofocus]').exists()).toBe(false)

    // YouTube contesta: las filas se rellenan solas
    resuelto()
    await vi.advanceTimersByTimeAsync(800)
    const rows = card().findAll('.cc-row')
    expect(rows[0].text()).toContain('Tema 0 (En Vivo)')
    expect(rows[0].text()).toContain('Canal Uno')
    expect(rows[0].text()).toContain('4:05')
    expect(rows[1].text()).toContain('no se parece')
    expect(rows[0].find('[role="checkbox"]').attributes('aria-checked')).toBe('true')
    expect(rows[1].find('[role="checkbox"]').attributes('aria-checked')).toBe('false')
    expect(card().find('.btn.primary').text()).toContain('Descargar (1)')
    // y se deja de preguntar
    const asked = held.api.chatPlan.mock.calls.length
    await vi.advanceTimersByTimeAsync(3000)
    expect(held.api.chatPlan.mock.calls.length).toBe(asked)

    // «ver» abre solo la dirección canónica del vídeo
    await rows[0].find('.cc-see').trigger('click')
    expect(held.app.openInBrowser).toHaveBeenCalledWith(URL1)

    // Descargar: por el id, con las casillas marcadas
    held.api.chatConfirm.mockResolvedValueOnce({
      ok: true,
      text: 'Descargando. Te cuento cuando termine. Al terminar la añado a «domingo».',
      result: {
        active: true,
        id: 'k1',
        job: 5,
        items: [URL1],
        force: false,
        trimmed: false,
        playlist: { id: 7, name: 'domingo' },
        skipped: 1
      }
    })
    held.yt = [
      { available: true, active: true, phase: 'listing', name: '', percent: 100, job: 5 },
      {
        available: true,
        active: false,
        phase: 'done',
        job: 5,
        results: [
          { ok: true, id: 267, artist: 'Autor', song: 'Tema 0', title: 'Tema 0', requested: URL1 }
        ],
        after: {
          job: 5,
          state: 'done',
          playlist: { id: 7, name: 'domingo' },
          added: [267],
          failed: [{ n: 3, item: 'tema 1', label: 'Cena Del Señor', reason: 'no la encontré' }],
          skipped: [],
          order: [142, 267]
        }
      }
    ]
    await quieta()
    await card().find('.btn.primary').trigger('click')
    await flushPromises()
    expect(held.api.chatConfirm).toHaveBeenCalledWith({ id: 'k1', selection: [0] })
    const accepted = useChat().messages.value.find((m) => m.event === 'confirm_accepted')
    expect(accepted).toMatchObject({ app: true, ref: 'k1' })
    expect(accepted.tools[0]).toMatchObject({
      name: 'download_music',
      summary: 'aceptada, en marcha'
    })
    expect(card().find('.cc-actions').exists()).toBe(false)
    expect(card().text()).toContain('Aceptada')

    // mientras mete las canciones en la lista, se dice
    await vi.advanceTimersByTimeAsync(50)
    expect(w.find('.chat-downloading').text()).toContain('Metiendo en la lista')

    // al acabar: el informe de la app, sin pasarle el turno al modelo
    await vi.advanceTimersByTimeAsync(1000)
    await flushPromises()
    const done = useChat().messages.value.find((m) => m.event === 'download_done')
    expect(done).toMatchObject({ app: true, ref: 'k1' })
    expect(done.text).toBe(
      'Entró en «domingo»: Autor - Tema 0. No se pudo: Cena Del Señor (no la encontré).'
    )
    expect(done.tools[0].detail).toContain('id 267')
    expect(held.api.chatStart).toHaveBeenCalledTimes(1)
    expect(card().text()).toContain('Aceptada y descargada')
    expect(hooks.reload).toHaveBeenCalled()
  })

  it('«No» no baja nada y no le pregunta al modelo', async () => {
    held.replies.push(conTarjeta(propuesta()))
    const w = await montar()
    await escribir(w, 'mete esta lista')
    await quieta()
    const no = w.findAll('.cc-actions .btn').find((b) => b.text() === 'No')
    await no.trigger('click')
    await flushPromises()
    expect(held.api.chatConfirm).not.toHaveBeenCalled()
    // lo que seguía buscando en YouTube se para
    expect(held.api.chatPlanCancel).toHaveBeenCalledWith('k1')
    const declined = useChat().messages.value.at(-1)
    expect(declined).toMatchObject({ app: true, ref: 'k1', event: 'confirm_declined' })
    expect(held.api.chatStart).toHaveBeenCalledTimes(1)
    expect(w.find('.cc').text()).toContain('No la quisiste')
    expect(w.find('.cc-actions').exists()).toBe(false)
  })

  it('los botones no hacen caso en el medio segundo que sigue a un cambio', async () => {
    held.replies.push(conTarjeta(propuesta()))
    const w = await montar()
    await escribir(w, 'mete esta lista')
    const descargar = () => w.find('.cc .btn.primary')
    // recién salida: el clic se ignora
    await descargar().trigger('click')
    await flushPromises()
    expect(held.api.chatConfirm).not.toHaveBeenCalled()
    // llegan filas (cambia el contenido): otra vez quieta un rato
    await vi.advanceTimersByTimeAsync(400)
    resuelto()
    await vi.advanceTimersByTimeAsync(350)
    await descargar().trigger('click')
    await flushPromises()
    expect(held.api.chatConfirm).not.toHaveBeenCalled()
    // tocar tus casillas no la «mueve»
    await quieta()
    await w.findAll('.cc-row')[1].find('[role="checkbox"]').trigger('click')
    await descargar().trigger('click')
    await flushPromises()
    expect(held.api.chatConfirm).toHaveBeenCalledWith({ id: 'k1', selection: [0, 1] })
  })

  it('mientras sigue buscando, preguntar por las filas (sin cambios) no bloquea los botones', async () => {
    held.replies.push(conTarjeta(propuesta()))
    const w = await montar()
    await escribir(w, 'mete esta lista')
    // varias consultas del plan, todas iguales («buscando»)
    await vi.advanceTimersByTimeAsync(2000)
    expect(held.api.chatPlan.mock.calls.length).toBeGreaterThan(2)
    await w.find('.cc .btn.primary').trigger('click')
    await flushPromises()
    expect(held.api.chatConfirm).toHaveBeenCalledWith({ id: 'k1', selection: [0, 1] })
  })

  it('caducada (410): lo dice y «Volver a pedirlo» se lo pide al asistente', async () => {
    held.replies.push(conTarjeta(propuesta()))
    const w = await montar()
    await escribir(w, 'mete esta lista')
    // en el servidor ya no vale (pasaron 30 minutos)
    held.state.chatConfirms.k1 = 'expired'
    await quieta()
    await w.find('.cc .btn.primary').trigger('click')
    await flushPromises()
    const failed = useChat().messages.value.at(-1)
    expect(failed).toMatchObject({ ref: 'k1', event: 'confirm_failed', error: true })
    expect(failed.text).toContain('caducó')
    const card = w.find('.cc')
    expect(card.text()).toContain('Caducada')
    expect(card.find('.cc-actions').exists()).toBe(false)
    held.replies.push({ text: 'Te la propongo otra vez.', tools: [] })
    await quieta()
    const again = card.findAll('.cc-link').find((b) => b.text() === 'Volver a pedirlo')
    await again.trigger('click')
    await flushPromises()
    await flushPromises()
    expect(held.api.chatStart).toHaveBeenCalledTimes(2)
    expect(enviado().at(-1)).toEqual({ role: 'me', text: ASK_AGAIN })
  })

  it('otra descarga en marcha (409) no la gasta: sigue con sus botones', async () => {
    held.replies.push(conTarjeta(propuesta()))
    const w = await montar()
    await escribir(w, 'mete esta lista')
    held.api.chatConfirm.mockRejectedValueOnce(
      Object.assign(new Error('ya hay una descarga en marcha'), { status: 409 })
    )
    await quieta()
    await w.find('.cc .btn.primary').trigger('click')
    await flushPromises()
    expect(w.text()).toContain('No se pudo: ya hay una descarga en marcha')
    expect(w.find('.cc-actions').exists()).toBe(true)
  })

  it('una propuesta nueva deja la anterior sin valor', async () => {
    held.replies.push(conTarjeta(propuesta('k1')), conTarjeta(propuesta('k2')))
    const w = await montar()
    await escribir(w, 'mete esta lista')
    await escribir(w, 'y esta otra')
    const cards = w.findAll('.cc')
    expect(cards).toHaveLength(2)
    expect(cards[0].text()).toContain('Hay una propuesta más nueva')
    expect(cards[1].find('.cc-actions').exists()).toBe(true)
  })

  it('«Parar»: lo que ya se hizo y la tarjeta se pintan igual', async () => {
    held.replies.push({
      text: '',
      canceled: true,
      tools: [{ name: 'fill_playlist', summary: '«domingo»: 1 de 3' }],
      confirm: propuesta(),
      setlist: lista()
    })
    const w = await montar()
    await escribir(w, 'mete esta lista')
    const m = useChat().messages.value.at(-1)
    expect(m.canceled).toBe(true)
    expect(w.find('.cc .cc-actions').exists()).toBe(true)
    expect(w.find('.cs').exists()).toBe(true)
  })
})

describe('«sí» y «no» escritos', () => {
  it('reconoce un sí o un no a secas, con o sin tildes ni signos', () => {
    for (const t of [
      'sí',
      'Si',
      'SIII!!',
      'dale',
      'ok',
      'vale',
      'bueno',
      'listo',
      'adelante',
      'hazlo',
      'bájalas',
      'descárgalas',
      'procede',
      'comienza',
      'empieza',
      'sí, por favor',
      'dale pues 👍',
      '¡Sí, bájalas todas!'
    ]) {
      expect(shortAnswer(t), t).toBe(true)
    }
    for (const t of ['no', 'No.', 'nop', 'mejor no', 'no, gracias', 'no las bajes', 'cancela']) {
      expect(shortAnswer(t), t).toBe(false)
    }
    for (const t of [
      'sí, pero quita la 3',
      'no sé',
      'sí no',
      'dale a la segunda',
      '¿qué es esto?',
      '👍',
      'si tienes la de Barak bájala'
    ]) {
      expect(shortAnswer(t), t).toBe(null)
    }
  })

  it('un «sí» acepta la tarjeta como el clic, con tu burbuja y sin llamar al modelo', async () => {
    held.replies.push(conTarjeta(propuesta()))
    resuelto()
    const w = await montar()
    await escribir(w, 'mete esta lista')
    await escribir(w, 'Sí, dale')
    expect(held.api.chatStart).toHaveBeenCalledTimes(1)
    expect(held.api.chatConfirm).toHaveBeenCalledWith({ id: 'k1', selection: [0] })
    expect(w.findAll('.chat-msg.me').at(-1).text()).toBe('Sí, dale')
    expect(w.find('.chat-foot textarea').element.value).toBe('')
    expect(useChat().messages.value.at(-1).event).toBe('confirm_accepted')
  })

  it('un «no» la rechaza; sin tarjeta esperando, un «sí» va al modelo', async () => {
    held.replies.push(conTarjeta(propuesta()))
    const w = await montar()
    await escribir(w, 'mete esta lista')
    await escribir(w, 'no')
    expect(held.api.chatStart).toHaveBeenCalledTimes(1)
    expect(useChat().messages.value.at(-1).event).toBe('confirm_declined')
    // ya contestada: el siguiente «sí» es un mensaje normal
    await escribir(w, 'sí')
    expect(held.api.chatStart).toHaveBeenCalledTimes(2)
    expect(enviado().at(-1)).toEqual({ role: 'me', text: 'sí' })
  })

  it('«sí, pero…» no la acepta: va al modelo', async () => {
    held.replies.push(conTarjeta(propuesta()))
    const w = await montar()
    await escribir(w, 'mete esta lista')
    await escribir(w, 'sí, pero quita la 3')
    expect(held.api.chatConfirm).not.toHaveBeenCalled()
    expect(held.api.chatStart).toHaveBeenCalledTimes(2)
  })
})

describe('borrar: el diálogo, fuera del turno y de uno en uno', () => {
  const borrar = (id = 'd1') => {
    held.state.chatConfirms[id] = 'live'
    return {
      text: 'Te pido permiso.',
      tools: [{ name: 'delete_song', summary: 'espera tu visto bueno' }],
      confirm: { id, tool: 'delete_song', summary: 'Mandar a la papelera «Mi Gozo»', args: {} }
    }
  }

  it('se pregunta con el turno ya terminado y se acepta por el id', async () => {
    held.replies.push(borrar())
    const w = await montar()
    await escribir(w, 'borra mi gozo')
    expect(useChat().thinking.value).toBe(false)
    expect(useDialog().dialog.value.open).toBe(true)
    expect(useDialog().dialog.value.message).toContain('Mi Gozo')
    // no hay tarjeta en línea para esto
    expect(w.find('.cc').exists()).toBe(false)
    dialogOk()
    await flushPromises()
    expect(held.api.chatConfirm).toHaveBeenCalledWith({ id: 'd1' })
    expect(useChat().messages.value.at(-1)).toMatchObject({
      app: true,
      ref: 'd1',
      event: 'confirm_accepted'
    })
    expect(hooks.reload).toHaveBeenCalled()
  })

  it('nunca encima de otro diálogo: espera su turno, y lo de la cola también', async () => {
    let soltar
    held.replies.push(() => new Promise((r) => (soltar = r)))
    held.tools = {
      ...held.tools,
      primary_problem: { kind: 'billing', code: 400, at: 1 },
      answering: { id: 'respaldo', name: 'Respaldo', model: 'r/modelo', fallback: true }
    }
    const w = await montar()
    await escribir(w, 'borra mi gozo')
    await escribir(w, 'y luego esto')
    // mientras piensa, otro diálogo de la app (uno cualquiera)
    ask({ kind: 'confirm', title: 'Otra cosa', message: '¿Seguro?' })
    soltar(borrar())
    await flushPromises()
    await flushPromises()
    expect(useChat().thinking.value).toBe(false)
    // el de borrar NO lo sustituye
    expect(useDialog().dialog.value.title).toBe('Otra cosa')
    // y otra pregunta del chat (cambiar la IA) se pone detrás
    const promote = useChat().promoteFallback()
    dialogCancel()
    await flushPromises()
    expect(useDialog().dialog.value.title).toBe('El asistente quiere hacer esto')
    dialogCancel()
    await flushPromises()
    expect(useDialog().dialog.value.title).toBe('Cambiar la IA principal')
    expect(useChat().messages.value.some((m) => m.event === 'confirm_declined')).toBe(true)
    // lo que esperaba en la cola no sale hasta que no queda ninguna pregunta
    expect(held.api.chatStart).toHaveBeenCalledTimes(1)
    dialogCancel()
    await promote
    await flushPromises()
    await flushPromises()
    expect(held.api.aiActivate).not.toHaveBeenCalled()
    expect(held.api.chatStart).toHaveBeenCalledTimes(2)
    expect(enviado().at(-1).text).toBe('y luego esto')
    expect(w.findAll('.composer-chip')).toHaveLength(0)
  })
})

/** La tabla de una lista metida desde el chat. */
function lista(extra = {}) {
  return {
    playlist: { id: 7, name: 'domingo', created: false },
    mode: 'add',
    applied: true,
    rows: [
      {
        n: 1,
        title: 'Digno De Adorar',
        status: 'found',
        song: { id: 1, artist: 'Autor', title: 'Digno De Adorar' },
        alts: [],
        note: '',
        link: null,
        in_list: true,
        kind: ''
      },
      {
        n: 2,
        title: 'Te Doy Gloria',
        status: 'other_version',
        song: { id: 2, artist: 'Otro', title: 'Te Doy Gloria' },
        alts: [],
        note: '',
        link: { url: URL1 },
        in_list: true,
        kind: 'video'
      },
      {
        n: 3,
        title: 'Rey Infinito',
        status: 'ambiguous',
        song: { id: 3, artist: 'Grupo', title: 'Rey Infinito' },
        alts: [{ id: 30, artist: 'Grupo B', title: 'Rey Infinito (En Vivo)' }],
        note: 'tienes 1 más',
        link: null,
        in_list: true,
        kind: ''
      },
      {
        n: 4,
        title: 'Cena Del Señor',
        status: 'missing',
        song: null,
        alts: [],
        note: '',
        link: null,
        in_list: false,
        kind: ''
      }
    ],
    added: [2, 3],
    already_in: [1],
    removed: [],
    extras_in_list: [{ id: 9, artist: 'Viejo', title: 'De Otra Semana' }],
    order_differs: true,
    previous: null,
    ignored: ['Banda: guitarra'],
    truncated: 0,
    mode_note: '',
    ask_note: '',
    ...extra
  }
}

describe('la tabla de una lista', () => {
  beforeEach(() => {
    held.state.songs = [
      song(1, { title: 'Digno De Adorar', artist: 'Autor' }),
      song(2, { title: 'Te Doy Gloria', artist: 'Otro' }),
      song(3, { title: 'Rey Infinito', artist: 'Grupo' }),
      song(9, { title: 'De Otra Semana', artist: 'Viejo' })
    ]
    // como quedó: la de otra semana delante y el orden cambiado
    held.state.playlistSongs = [9, 3, 1, 2].map((i) => held.state.songs.find((s) => s.id === i))
  })

  it('cada fila: en la lista, otra versión, dudosa o falta, con lo pedido junto a lo hallado', async () => {
    held.replies.push({ text: 'Añadí…', tools: [], setlist: lista() })
    const w = await montar()
    await escribir(w, 'mete esto')
    const rows = w.findAll('.cs-row')
    expect(rows).toHaveLength(4)
    expect(rows[0].text()).toContain('en la lista')
    expect(rows[0].text()).toContain('Digno De Adorar')
    expect(rows[1].text()).toContain('otra versión')
    expect(rows[1].text()).toContain('→ Otro - Te Doy Gloria')
    expect(rows[2].text()).toContain('dudosa')
    expect(rows[2].text()).toContain('también: Grupo B - Rey Infinito (En Vivo)')
    expect(rows[3].text()).toContain('falta')
    expect(rows.map((r) => r.classes().find((c) => c.startsWith('is-')))).toEqual([
      'is-ok',
      'is-ok',
      'is-doubt',
      'is-missing'
    ])
    const card = w.find('.cs')
    expect(card.text()).toContain('En «domingo» hay 1 de otras veces: Viejo - De Otra Semana')
    expect(card.text()).toContain('No tomé por canción: Banda: guitarra')
    // la respuesta con tabla ocupa más ancho
    expect(w.findAll('.chat-msg.ai').at(-1).classes()).toContain('has-card')
  })

  it('«Dejar solo estas» y «Deshacer», sin modelo y contándolo', async () => {
    held.replies.push({ text: 'Añadí…', tools: [], setlist: lista() })
    const w = await montar()
    await escribir(w, 'mete esto')
    const chip = (t) => w.findAll('.cs-actions .btn').find((b) => b.text() === t)
    expect(chip('Deshacer')).toBeUndefined()
    await quieta()
    await chip('Dejar solo estas').trigger('click')
    await flushPromises()
    expect(held.api.setPlaylistSongs).toHaveBeenCalledWith(7, [1, 2, 3])
    const said = useChat().messages.value.at(-1)
    expect(said.app).toBe(true)
    expect(said.text).toBe(
      'Dejé «domingo» solo con estas 3, en el orden de tu lista. Quité Viejo - De Otra Semana.'
    )
    expect(said.tools[0].name).toBe('set_playlist_songs')
    expect(hooks.reload).toHaveBeenCalled()
    expect(held.api.chatStart).toHaveBeenCalledTimes(1)
    // ya no sobra nada y el orden es el de la lista pegada
    expect(chip('Dejar solo estas')).toBeUndefined()
    expect(chip('Ordenar como mi lista')).toBeUndefined()
    await quieta()
    await chip('Deshacer').trigger('click')
    await flushPromises()
    expect(held.api.setPlaylistSongs).toHaveBeenLastCalledWith(7, [9, 3, 1, 2])
    expect(useChat().messages.value.at(-1).text).toContain(
      'vuelve a estar como antes (4 canciones)'
    )
    expect(chip('Dejar solo estas')).toBeTruthy()
  })

  it('«Ordenar como mi lista» y el «Deshacer» de lo que cambió el asistente', async () => {
    held.replies.push({
      text: 'Dejé…',
      tools: [],
      setlist: lista({ mode: 'set', extras_in_list: [], previous: [9, 1] })
    })
    const w = await montar()
    await escribir(w, 'déjala exactamente así')
    const chip = (t) => w.findAll('.cs-actions .btn').find((b) => b.text() === t)
    expect(chip('Dejar solo estas')).toBeUndefined()
    await quieta()
    await chip('Ordenar como mi lista').trigger('click')
    await flushPromises()
    expect(held.api.reorderPlaylist).toHaveBeenCalledWith(7, [1, 2, 3])
    expect(useChat().messages.value.at(-1).text).toBe('Ordené «domingo» como tu lista.')
    expect(chip('Ordenar como mi lista')).toBeUndefined()
    // deshacer el orden y, después, lo que hizo el asistente (`previous`)
    await quieta()
    await chip('Deshacer').trigger('click')
    await flushPromises()
    expect(held.api.setPlaylistSongs).toHaveBeenLastCalledWith(7, [9, 3, 1, 2])
    await quieta()
    await chip('Deshacer').trigger('click')
    await flushPromises()
    expect(held.api.setPlaylistSongs).toHaveBeenLastCalledWith(7, [9, 1])
    expect(chip('Deshacer')).toBeUndefined()
  })

  it('comprobar: «Dejar solo estas» aunque no sobre nada, si la lista no está como la pegada', async () => {
    const rows = lista().rows.map((r) => (r.n === 3 ? { ...r, in_list: false } : r))
    held.replies.push({
      text: 'Comprobé…',
      tools: [],
      setlist: lista({
        mode: 'check',
        applied: false,
        extras_in_list: [],
        rows,
        order_differs: false
      })
    })
    const w = await montar()
    await escribir(w, 'verifica la lista')
    expect(w.find('.cs').text()).toContain('Comprobada, sin cambios')
    expect(w.findAll('.cs-actions .btn').map((b) => b.text())).toEqual(['Dejar solo estas'])
  })

  it('larga, se pliega; sin desbordes con títulos larguísimos', async () => {
    const largo = 'Una Canción Con Un Título Larguísimo '.repeat(12)
    const rows = Array.from({ length: 14 }, (_, k) => ({
      ...lista().rows[0],
      n: k + 1,
      title: largo,
      song: { id: k + 1, artist: 'Autor', title: largo }
    }))
    held.replies.push({ text: 'Añadí…', tools: [], setlist: lista({ rows }) })
    const w = await montar()
    await escribir(w, 'mete esto')
    expect(w.findAll('.cs-row')).toHaveLength(6)
    const more = w.find('.cs .chat-more')
    expect(more.text()).toBe('Ver las 14')
    expect(more.attributes('aria-expanded')).toBe('false')
    await more.trigger('click')
    expect(w.findAll('.cs-row')).toHaveLength(14)
    // los textos de fuera llegan como dato: sin saltos ni recortes a mano
    expect(useChat().messages.value.at(-1).setlist.rows[0].title.length).toBeLessThanOrEqual(160)
  })
})

describe('la franja del proveedor', () => {
  const caido = (kind, answering = true) => ({
    model: 'claude-x',
    provider: 'Claude (Anthropic)',
    available: true,
    reason: '',
    tools: [],
    primary_problem: { kind, code: 400, at: 1 },
    answering: answering
      ? {
          id: 'deepinfra',
          name: 'DeepInfra',
          model: 'Qwen/Qwen3-Next-80B-A3B-Instruct',
          fallback: true,
          reason: kind
        }
      : null
  })

  it.each([
    [
      'billing',
      'Claude (Anthropic) no tiene saldo; contesta DeepInfra · Qwen3-Next-80B-A3B-Instruct (respaldo).'
    ],
    ['auth', 'Claude (Anthropic) no acepta la clave; contesta DeepInfra'],
    ['rate', 'Claude (Anthropic) pide esperar (demasiadas peticiones); contesta DeepInfra'],
    ['down', 'Claude (Anthropic) no responde; contesta DeepInfra']
  ])('%s: lo dice con sus palabras, nunca con las del proveedor', async (kind, text) => {
    held.tools = caido(kind)
    const w = await montar()
    const banner = w.find('.chat-banner')
    expect(banner.exists()).toBe(true)
    expect(banner.attributes('aria-label')).toBe('Estado de la IA')
    expect(banner.find('[role="status"]').text()).toContain(text)
  })

  it('sin respuesta del respaldo todavía, dice solo lo del principal; sin problema, nada', async () => {
    held.tools = caido('billing', false)
    const w = await montar()
    expect(w.find('.chat-banner-text').text()).toBe('Claude (Anthropic) no tiene saldo.')
    expect(w.findAll('.chat-banner-actions .btn')).toHaveLength(2)
    w.unmount()
    resetChat()
    connectChat(hooks)
    held.tools = {
      model: 'm',
      provider: 'P',
      available: true,
      tools: [],
      primary_problem: null,
      answering: null
    }
    const w2 = await montar()
    expect(w2.find('.chat-banner').exists()).toBe(false)
  })

  it('sale también tras una respuesta del respaldo (núcleo que no dice `answering`)', async () => {
    held.replies.push({
      text: 'hola',
      tools: [],
      via: { id: 'llm7', name: 'LLM7', model: 'x', fallback: true, reason: 'down' }
    })
    const w = await montar()
    expect(w.find('.chat-banner').exists()).toBe(false)
    await escribir(w, 'hola')
    expect(w.find('.chat-banner-text').text()).toBe('P no responde; contestó LLM7 · x (respaldo).')
  })

  it('sus botones: Ajustes, reintentar y usar el respaldo (solo tras confirmarlo)', async () => {
    held.tools = caido('billing')
    const w = await montar()
    const boton = (t) => w.findAll('.chat-banner-actions .btn').find((b) => b.text().includes(t))
    await boton('Ajustes de IA').trigger('click')
    expect(hooks.go).toHaveBeenCalledWith({ kind: 'settings' })

    // usar el respaldo: nombra proveedor y modelo, y sin «sí» no cambia nada
    expect(boton('como principal').text()).toBe(
      'Usar DeepInfra · Qwen3-Next-80B-A3B-Instruct como principal'
    )
    await boton('como principal').trigger('click')
    await flushPromises()
    expect(useDialog().dialog.value.message).toContain('DeepInfra')
    expect(useDialog().dialog.value.message).toContain('Qwen/Qwen3-Next-80B-A3B-Instruct')
    expect(held.api.aiActivate).not.toHaveBeenCalled()
    dialogCancel()
    await flushPromises()
    expect(held.api.aiActivate).not.toHaveBeenCalled()
    await boton('como principal').trigger('click')
    await flushPromises()
    dialogOk()
    await flushPromises()
    expect(held.api.aiActivate).toHaveBeenCalledWith('deepinfra')

    // reintentar: se olvida el fallo y la franja se va
    await boton('Reintentar').trigger('click')
    await flushPromises()
    expect(held.api.aiRecheck).toHaveBeenCalled()
    expect(w.find('.chat-banner').exists()).toBe(false)
  })
})

describe('conversaciones guardadas y compatibilidad', () => {
  it('se guarda la tarjeta (sin args, plan recortado y como dato), la tabla y la referencia; al modelo no van', async () => {
    const muchas = Array.from({ length: 30 }, (_, k) =>
      fila(k, { title: `Título\ncon salto ${k}`, url: k ? URL1 : 'javascript:alert(1)' })
    )
    held.replies.push(conTarjeta(propuesta('k1', { items: muchas }), { setlist: lista() }))
    const w = await montar()
    await escribir(w, 'mete esta lista')
    const saved = held.state.chats[0].messages.find((m) => m.confirm)
    expect(saved.confirm.args).toBeUndefined()
    expect(saved.confirm.plan.items).toHaveLength(25)
    expect(saved.confirm.plan.items[0].title).toBe('Título con salto 0')
    expect(saved.confirm.plan.items[0].url).toBe('')
    expect(saved.setlist.rows).toHaveLength(4)
    // el modelo no recibe la tarjeta ni la tabla: le basta lo que hizo cada herramienta
    await quieta()
    const no = w.findAll('.cc-actions .btn').find((b) => b.text() === 'No')
    await no.trigger('click')
    await flushPromises()
    expect(held.state.chats[0].messages.at(-1)).toMatchObject({
      ref: 'k1',
      event: 'confirm_declined'
    })
    await escribir(w, 'gracias')
    for (const m of enviado()) {
      expect(m.confirm).toBeUndefined()
      expect(m.setlist).toBeUndefined()
    }
    expect(enviado().find((m) => m.tools?.[0]?.name === 'fill_playlist').tools[0].detail).toBe(
      'n=1 id142'
    )
  })

  it('al reabrir: una tarjeta sin desenlace se pregunta al servidor; tras reiniciar, caducada', async () => {
    held.state.chats = [
      {
        id: 1,
        title: 'domingo',
        messages: [
          { role: 'me', text: 'mete la lista' },
          {
            role: 'ai',
            text: 'Faltan 2.',
            confirm: { id: 'viejo', tool: 'download_music', summary: 'Descargar 2 temas' }
          },
          {
            role: 'ai',
            text: 'Otra.',
            confirm: { id: 'hecho', tool: 'download_music', summary: 'Descargar 1 tema' }
          },
          { role: 'ai', text: 'Descargando.', app: true, ref: 'hecho', event: 'confirm_accepted' }
        ]
      }
    ]
    const w = await montar()
    await flushPromises()
    const cards = w.findAll('.cc')
    expect(cards).toHaveLength(2)
    // el registro del servidor vive en memoria: tras reiniciar, «unknown»
    expect(held.api.chatConfirmStatus).toHaveBeenCalledWith('viejo')
    expect(held.api.chatConfirmStatus).not.toHaveBeenCalledWith('hecho')
    expect(cards[0].text()).toContain('Caducada')
    expect(cards[0].text()).toContain('Volver a pedirlo')
    expect(cards[1].text()).toContain('Aceptada')
  })

  it('un chat guardado antes (sin tarjetas ni tablas) se lee igual', async () => {
    held.state.chats = [
      {
        id: 1,
        title: 'antes',
        messages: [
          { role: 'me', text: 'hola' },
          {
            role: 'ai',
            text: 'Hola, ¿qué buscas?',
            tools: [{ name: 'search_songs', summary: '3' }]
          }
        ]
      }
    ]
    const w = await montar()
    expect(w.findAll('.chat-msg')).toHaveLength(2)
    expect(w.find('.cc').exists()).toBe(false)
    expect(w.find('.cs').exists()).toBe(false)
    expect(held.api.chatConfirmStatus).not.toHaveBeenCalled()
  })

  it('un núcleo antiguo (propuesta sin id): el diálogo, ya fuera del turno, con (tool, args)', async () => {
    held.replies.push({
      text: 'Te pido permiso.',
      tools: [],
      confirm: { tool: 'download_music', summary: 'Descargar «X»', args: { items: ['x'] } }
    })
    const w = await montar()
    await escribir(w, 'baja x')
    expect(useChat().thinking.value).toBe(false)
    expect(useDialog().dialog.value.open).toBe(true)
    expect(w.find('.cc').exists()).toBe(false)
    dialogOk()
    await flushPromises()
    await flushPromises()
    expect(held.api.chatConfirm).toHaveBeenCalledWith('download_music', { items: ['x'] })
  })
})

describe('el informe al acabar la descarga', () => {
  /** Acepta la tarjeta y deja la descarga en marcha (job 5). */
  async function aceptada(w, result = {}) {
    held.api.chatConfirm.mockResolvedValueOnce({
      ok: true,
      text: 'Descargando.',
      result: {
        active: true,
        id: 'k1',
        job: 5,
        items: [URL1],
        force: false,
        playlist: { id: 7, name: 'domingo' },
        ...result
      }
    })
    await quieta()
    await w.find('.cc .btn.primary').trigger('click')
    await flushPromises()
  }

  it('sin `after` (o con error): el informe de siempre y el aviso oculto para que remate el modelo', async () => {
    held.replies.push(conTarjeta(propuesta()))
    const w = await montar()
    await escribir(w, 'mete esta lista')
    held.yt = [
      {
        available: true,
        active: false,
        phase: 'done',
        job: 5,
        results: [{ ok: true, id: 267, artist: 'Autor', song: 'Tema 0', requested: URL1 }],
        after: { job: 5, state: 'error', error: 'la lista cambió de nombre' }
      }
    ]
    held.replies.push({ text: 'Añadida a «domingo».', tools: [] })
    await aceptada(w)
    await vi.advanceTimersByTimeAsync(1000)
    await flushPromises()
    const report = useChat().messages.value.find((m) => m.event === 'download_done')
    expect(report.ref).toBe('k1')
    expect(report.text).toContain('No pude meterlas en «domingo» (la lista cambió de nombre)')
    expect(held.api.chatStart).toHaveBeenCalledTimes(2)
    const notice = enviado().at(-1)
    expect(notice).toMatchObject({ role: 'me', event: 'download_done' })
    expect(notice.text).toContain('(id 267)')
    expect(notice.text).toContain('Lo que te pedi fue: «mete esta lista»')
  })

  it('va a la conversación desde la que se pidió aunque se haya abierto otra', async () => {
    held.state.chats = [{ id: 9, title: 'otra', messages: [{ role: 'me', text: 'hola' }] }]
    held.replies.push(conTarjeta(propuesta()))
    const w = await montar()
    useChat().newChat()
    await escribir(w, 'mete esta lista')
    const from = useChat().chatId.value
    expect(from).not.toBe(9)
    held.yt = [
      {
        available: true,
        active: false,
        phase: 'done',
        job: 5,
        results: [{ ok: true, id: 267, artist: 'Autor', song: 'Tema 0', requested: URL1 }],
        after: {
          job: 5,
          state: 'done',
          playlist: { id: 7, name: 'domingo' },
          added: [267],
          failed: [],
          skipped: [],
          order: [267]
        }
      }
    ]
    await aceptada(w)
    expect(useChat().following.value.chatId).toBe(from)
    // abre otra conversación mientras baja
    await useChat().openChat(9)
    await vi.advanceTimersByTimeAsync(1000)
    await flushPromises()
    expect(useChat().messages.value.some((m) => m.event === 'download_done')).toBe(false)
    const there = held.state.chats.find((c) => c.id === from).messages.at(-1)
    expect(there).toMatchObject({ ref: 'k1', event: 'download_done' })
    expect(there.text).toBe('Entró en «domingo»: Autor - Tema 0.')
    expect(useNotices().notices.value.some((n) => n.message.includes('te lo cuento en'))).toBe(true)
  })

  it('una descarga de otro turno no se cuenta como la nuestra', async () => {
    held.replies.push(conTarjeta(propuesta()))
    const w = await montar()
    await escribir(w, 'mete esta lista')
    held.yt = [
      {
        available: true,
        active: false,
        phase: 'done',
        job: 6,
        results: [{ ok: true, id: 1, artist: 'X', song: 'Y', requested: URL1 }],
        after: null
      }
    ]
    await aceptada(w)
    await vi.advanceTimersByTimeAsync(1000)
    await flushPromises()
    expect(useChat().messages.value.some((m) => m.event === 'download_done')).toBe(false)
  })
})

describe('lo que cambia la biblioteca y lo que se ve fuera del chat', () => {
  it('meter una lista recarga la biblioteca', async () => {
    held.replies.push({ text: 'Añadí…', tools: [{ name: 'fill_playlist', summary: 'x' }] })
    const w = await montar()
    await escribir(w, 'mete esto')
    expect(hooks.reload).toHaveBeenCalled()
  })

  it('con una tarjeta esperando y el chat fuera de la vista, se avisa una vez', async () => {
    held.replies.push(conTarjeta(propuesta()))
    const w = await montar()
    await escribir(w, 'mete esta lista')
    expect(useChat().attention.value).toBe(false)
    w.unmount()
    await flushPromises()
    expect(useChat().attention.value).toBe(true)
    const avisos = () =>
      useNotices().notices.value.filter((n) => n.message.includes('espera tu visto bueno'))
    expect(avisos()).toHaveLength(1)
    useChat().setVisible(true)
    useChat().setVisible(false)
    await flushPromises()
    expect(avisos()).toHaveLength(1)
  })

  it('el texto de fuera se guarda como dato', () => {
    const raro = 'a\nb' + String.fromCharCode(0x202e) + 'c' + String.fromCharCode(0x200b) + 'd'
    expect(datum(raro)).toBe('a b c d')
    expect(datum('x'.repeat(200), 10)).toBe('xxxxxxxxx…')
  })
})
