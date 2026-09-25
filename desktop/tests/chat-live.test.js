import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'

// El chat en vivo (texto segun sale, herramientas segun terminan, parar),
// las conversaciones guardadas y el coste de cada respuesta.
const held = vi.hoisted(() => ({ state: null, api: null, polls: [] }))

vi.mock('../src/api.js', async (importOriginal) => {
  const actual = await importOriginal()
  const { createState, buildApiDouble } = await import('./support/backend.js')
  const { vi: v } = await import('vitest')
  held.state = createState()
  held.api = buildApiDouble(actual, held.state)
  held.api.chatTools = v.fn(async () => ({ model: 'm', provider: 'P', available: true, reason: '', tools: [] }))
  // cada sondeo devuelve el siguiente estado programado en held.polls
  held.api.chatPoll = v.fn(async () => held.polls.length > 1 ? held.polls.shift() : held.polls[0])
  return { ...actual, api: held.api, inTauri: true }
})

import ChatPage from '../src/components/ChatPage.vue'
import { resetDownloads } from '../src/composables/useDownloads.js'
import { dialogOk } from '../src/composables/useDialog.js'

beforeEach(() => {
  vi.clearAllMocks()
  resetDownloads()
  localStorage.clear()
  held.state.chats = []
  held.polls = [{ text: 'hola', tools: [], done: true, result: { text: 'hola', tools: [], actions: [], confirm: null } }]
})

const montar = async () => {
  const w = mount(ChatPage, { attachTo: document.body })
  await flushPromises(); await flushPromises()
  return w
}
const escribir = async (w, t) => {
  await w.find('.chat-foot input').setValue(t)
  await w.find('.chat-foot .btn').trigger('click')
  await flushPromises(); await flushPromises()
}

describe('la respuesta en vivo', () => {
  it('enseña el texto segun llega y las herramientas segun terminan, y al final queda el mensaje', async () => {
    vi.useFakeTimers()
    held.polls = [
      { text: '', tools: [{ name: 'search_songs', summary: '3 resultados' }], done: false, result: null },
      { text: 'Tienes tres', tools: [{ name: 'search_songs', summary: '3 resultados' }], done: false, result: null },
      { text: 'Tienes tres canciones.', tools: [{ name: 'search_songs', summary: '3 resultados' }], done: true,
        result: { text: 'Tienes tres canciones.', tools: [{ name: 'search_songs', summary: '3 resultados' }], actions: [], confirm: null,
                  usage: { calls: 2, prompt: 3100, completion: 120, cost: 0.0021 }, via: { id: 'p', name: 'P', model: 'm', fallback: false } } }
    ]
    const w = await montar()
    await w.find('.chat-foot input').setValue('¿que tengo?')
    await w.find('.chat-foot .btn').trigger('click')
    await flushPromises(); await flushPromises()
    // primer sondeo: la herramienta ya se ve, sin texto todavia
    expect(w.find('.chat-live').exists()).toBe(true)
    expect(w.find('.chat-live .chat-tools').text()).toContain('busco en tu biblioteca')
    expect(w.find('.chat-cancel').exists()).toBe(true)
    await vi.advanceTimersByTimeAsync(300); await flushPromises()
    expect(w.find('.chat-live .chat-bubble').text()).toContain('Tienes tres')
    await vi.advanceTimersByTimeAsync(300); await flushPromises(); await flushPromises()
    expect(w.find('.chat-live').exists()).toBe(false)
    const ai = w.findAll('.chat-msg.ai')
    expect(ai).toHaveLength(1)
    expect(ai[0].text()).toContain('Tienes tres canciones.')
    expect(ai[0].find('.chat-cost').text()).toBe('3,2k tokens · $0,0021')
    expect(w.find('.chat-cancel').exists()).toBe(false)
    vi.useRealTimers()
  })

  it('parar corta la respuesta y lo que habia llegado se queda señalado', async () => {
    vi.useFakeTimers()
    held.polls = [
      { text: 'Voy a', tools: [], done: false, result: null },
      { text: 'Voy a contarte', tools: [], done: true, result: { text: '', canceled: true, tools: [], actions: [], confirm: null } }
    ]
    const w = await montar()
    await w.find('.chat-foot input').setValue('cuentame')
    await w.find('.chat-foot .btn').trigger('click')
    await flushPromises(); await flushPromises()
    await w.find('.chat-cancel').trigger('click')
    await flushPromises()
    expect(held.api.chatCancel).toHaveBeenCalledWith('job1')
    await vi.advanceTimersByTimeAsync(300); await flushPromises(); await flushPromises()
    const ai = w.findAll('.chat-msg.ai')
    expect(ai).toHaveLength(1)
    expect(ai[0].text()).toContain('Voy a contarte')
    expect(ai[0].text()).toContain('Respuesta cortada')
    vi.useRealTimers()
  })

  it('cuando respondio el respaldo, lo dice', async () => {
    held.polls = [{ text: 'ok', tools: [], done: true,
      result: { text: 'ok', tools: [], actions: [], confirm: null,
                usage: { calls: 1, prompt: 10, completion: 5, cost: 0 }, via: { id: 'llm7', name: 'LLM7', model: 'x', fallback: true } } }]
    const w = await montar()
    await escribir(w, 'hola')
    expect(w.find('.chat-cost').text()).toBe('15 tokens · gratis · respondió LLM7 (respaldo)')
  })
})

describe('las conversaciones guardadas', () => {
  it('el primer mensaje crea la conversacion y todo se guarda en el nucleo', async () => {
    const w = await montar()
    expect(held.state.chats).toHaveLength(0)
    await escribir(w, 'armame una lista')
    expect(held.api.chatCreate).toHaveBeenCalledTimes(1)
    expect(held.state.chats).toHaveLength(1)
    const guardados = held.state.chats[0].messages
    expect(guardados.map((m) => m.role)).toEqual(['me', 'ai'])
    expect(guardados[0].text).toBe('armame una lista')
    expect(held.state.chats[0].title).toBe('armame una lista')
    expect(w.find('.chat-title').text()).toBe('armame una lista')
  })

  it('al volver se abre la ultima, y desde la lista se cambia, se crea y se borra', async () => {
    held.state.chats = [
      { id: 2, title: 'Set del domingo', updated: 2, messages: [{ role: 'me', text: 'domingo' }, { role: 'ai', text: 'Aqui va el set.' }] },
      { id: 1, title: 'Tonos', updated: 1, messages: [{ role: 'me', text: '¿en que tono?' }, { role: 'ai', text: 'En Sol.' }] }
    ]
    const w = await montar()
    expect(w.text()).toContain('Aqui va el set.')
    await w.findAll('.chat-head .btn').at(0).trigger('click')   // conversaciones
    await flushPromises()
    const items = w.findAll('.chat-item')
    expect(items.map((i) => i.find('.chat-item-title').text())).toEqual(['Set del domingo', 'Tonos'])
    await items[1].trigger('click')
    await flushPromises()
    expect(w.text()).toContain('En Sol.')
    expect(w.text()).not.toContain('Aqui va el set.')
    // nueva: vacia y sin conversacion hasta el primer mensaje
    await w.findAll('.chat-head .btn').at(1).trigger('click')
    await flushPromises()
    expect(w.find('.chat-empty').exists()).toBe(true)
    expect(held.api.chatCreate).not.toHaveBeenCalled()
    // borrar desde la lista pregunta antes
    await w.findAll('.chat-head .btn').at(0).trigger('click')
    await flushPromises()
    await w.findAll('.chat-item-x').at(0).trigger('click')
    await flushPromises()
    expect(held.api.chatDelete).not.toHaveBeenCalled()
    dialogOk()
    await flushPromises()
    expect(held.api.chatDelete).toHaveBeenCalledWith(2)
    expect(held.state.chats.map((c) => c.id)).toEqual([1])
  })

  it('buscar en todas enseña los mensajes que coinciden y abre su conversacion', async () => {
    vi.useFakeTimers()
    held.state.chats = [
      { id: 1, title: 'Tonos', updated: 1, messages: [{ role: 'me', text: '¿en que tono esta Mi Gozo?' }, { role: 'ai', text: 'En Sol.' }] },
      { id: 2, title: 'Otra', updated: 2, messages: [{ role: 'me', text: 'hola' }] }
    ]
    const w = await montar()
    await w.findAll('.chat-head .btn').at(0).trigger('click')
    await flushPromises()
    await w.find('.chat-list input').setValue('gozo')
    await vi.advanceTimersByTimeAsync(400); await flushPromises()
    expect(held.api.chatSearch).toHaveBeenCalledWith('gozo')
    const hits = w.findAll('.chat-item')
    expect(hits).toHaveLength(1)
    expect(hits[0].text()).toContain('Mi Gozo')
    await hits[0].trigger('click')
    await flushPromises()
    expect(w.text()).toContain('En Sol.')
    vi.useRealTimers()
  })

  it('lo que habia en el localStorage de antes pasa a la base una sola vez', async () => {
    localStorage.setItem('danplay.chat', JSON.stringify([{ role: 'me', text: 'vieja' }, { role: 'ai', text: 'respuesta vieja', tools: [] }]))
    const w = await montar()
    expect(held.state.chats).toHaveLength(1)
    expect(held.state.chats[0].title).toBe('Conversación anterior')
    expect(held.state.chats[0].messages[1].text).toBe('respuesta vieja')
    expect(localStorage.getItem('danplay.chat')).toBeNull()
    expect(w.text()).toContain('respuesta vieja')
  })
})

describe('exportar y presupuesto', () => {
  it('copia la conversacion como texto al portapapeles', async () => {
    held.state.chats = [{ id: 2, title: 'Set', updated: 2, messages: [{ role: 'me', text: 'hola' }, { role: 'ai', text: 'que tal' }] }]
    const written = []
    Object.defineProperty(navigator, 'clipboard', { value: { writeText: async (t) => { written.push(t) } }, configurable: true })
    const w = await montar()
    const copiar = w.findAll('.chat-head .btn').find((b) => b.attributes('title')?.includes('Copiar'))
    await copiar.trigger('click')
    await flushPromises()
    expect(held.api.chatExport).toHaveBeenCalledWith(2)
    expect(written[0]).toContain('# conversacion 2')
    w.unmount()
  })

  it('al pasar el tope mensual lo dice una vez, sin cortar la respuesta', async () => {
    held.polls = [{ text: 'ok', tools: [], done: true,
      result: { text: 'ok', tools: [], actions: [], confirm: null, budget: { limit: 1, month: 1.5, over: true } } }]
    const w = await montar()
    await escribir(w, 'hola')
    expect(w.findAll('.chat-msg.ai')).toHaveLength(1)
    const { notices } = await import('../src/composables/useNotices.js').then((m) => m.useNotices())
    expect(notices.value.some((n) => n.message.includes('tope'))).toBe(true)
    w.unmount()
  })
})
