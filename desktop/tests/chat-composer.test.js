import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import { nextTick } from 'vue'
import { readFileSync } from 'node:fs'

// La caja de escribir del asistente, la burbuja de lo que escribes y la cola.
//
// Antes era un <input> de una línea: al pegar la lista del domingo desde
// WhatsApp los saltos desaparecían (llegaba en una tira) y Enter la partía en
// dos mensajes («agrégame esto a la lista:» se contestaba solo). Y mientras
// el asistente respondía no se podía mandar nada.
const { api, store } = vi.hoisted(() => {
  const store = { chats: [], last: null }
  const api = {
    chatTools: vi.fn(async () => ({ model: 'm', available: true, tools: [] })),
    chat: vi.fn(async () => ({ text: 'Hecho.', tools: [] })),
    chatStart: vi.fn(async (messages, context) => {
      store.last = { messages, context }
      return { id: 'j1' }
    }),
    chatPoll: vi.fn(async () => {
      const result = await api.chat(store.last.messages, store.last.context)
      return { text: result.text || '', tools: result.tools || [], done: true, result }
    }),
    chatCancel: vi.fn(async () => ({ ok: true })),
    // los términos, ya aceptados: el asistente los pide (ver useTerms)
    terms: vi.fn(async () => ({
      accepted: (await import('../src/data/terms.js')).TERMS_VERSION,
      at: 1
    })),
    chats: vi.fn(async () => ({
      chats: store.chats.map((c) => ({ id: c.id, title: c.title, n: c.messages.length }))
    })),
    chatCreate: vi.fn(async (title = '') => {
      const c = { id: store.chats.length + 1, title, messages: [] }
      store.chats.unshift(c)
      return { id: c.id, title, n: 0 }
    }),
    chatGet: vi.fn(async (id) => {
      const c = store.chats.find((x) => x.id === id)
      return c ? { id: c.id, title: c.title, messages: c.messages.map((m) => ({ ...m })) } : null
    }),
    chatAppend: vi.fn(async (id, messages) => {
      const c = store.chats.find((x) => x.id === id)
      c.messages.push(...messages.map((m) => ({ ...m })))
      return { n: messages.length }
    }),
    chatRename: vi.fn(async () => ({ ok: true })),
    chatDelete: vi.fn(async () => ({ ok: true })),
    chatSearch: vi.fn(async () => ({ hits: [] })),
    chatConfirm: vi.fn(async () => ({ ok: true, result: {}, text: 'Hecho.' })),
    youtube: vi.fn(async () => ({ available: true, active: false, results: [] })),
    downloadFolder: vi.fn(async () => ({ path: '/m', ready: true, reason: '', suggested: '/m' }))
  }
  return { api, store }
})
vi.mock('../src/api.js', () => ({
  api,
  native: { available: false },
  inTauri: false,
  pickFolder: async () => null,
  errorMessage: (e) => String(e?.message || e)
}))
// el markdown de verdad, contado: para saber qué se vuelve a pasar
vi.mock('../src/utils/markdown.js', async (importOriginal) => {
  const actual = await importOriginal()
  const { vi: v } = await import('vitest')
  return { ...actual, renderMarkdown: v.fn(actual.renderMarkdown) }
})

import ChatPage from '../src/components/ChatPage.vue'
import ChatComposer from '../src/components/ChatComposer.vue'
import { renderMarkdown } from '../src/utils/markdown.js'
import { ask, dialogCancel, useDialog } from '../src/composables/useDialog.js'
import { resetDownloads } from '../src/composables/useDownloads.js'
import { clearNotices, useNotices } from '../src/composables/useNotices.js'
import {
  connectChat,
  resetChat,
  useChat,
  CORE_MESSAGES,
  DRAFT_MAX,
  QUEUE_MAX
} from '../src/composables/useChat.js'

const CSS = readFileSync('src/styles/ui.css', 'utf8')

beforeEach(() => {
  localStorage.clear()
  vi.clearAllMocks()
  vi.useRealTimers()
  dialogCancel()
  resetDownloads()
  resetChat()
  clearNotices()
  connectChat({ reload: vi.fn(), action: vi.fn() })
  store.chats = []
  store.last = null
})

/** Una pulsación de verdad, para poder mirar si se evitó lo que hace el navegador. */
function press(el, opts = {}) {
  const { keyCode, ...init } = opts
  const ev = new KeyboardEvent('keydown', {
    key: 'Enter',
    bubbles: true,
    cancelable: true,
    ...init
  })
  if (keyCode) Object.defineProperty(ev, 'keyCode', { value: keyCode })
  el.dispatchEvent(ev)
  return ev
}

/** La caja suelta, con su v-model funcionando. */
function box(props = {}) {
  const w = mount(ChatComposer, {
    attachTo: document.body,
    props: {
      modelValue: '',
      ...props,
      'onUpdate:modelValue': (v) => w.setProps({ modelValue: v })
    }
  })
  return w
}

const mountView = async () => {
  const w = mount(ChatPage, { attachTo: document.body })
  await flushPromises()
  return w
}
const typeText = async (w, t) => {
  await w.find('.chat-foot textarea').setValue(t)
  press(w.find('.chat-foot textarea').element)
  await flushPromises()
}
/** Lo último que recibió el núcleo en cada turno. */
const sentTexts = () => api.chat.mock.calls.map((c) => c[0].at(-1))
/** Deja al asistente «pensando» hasta que se llame a lo devuelto. */
function thinking() {
  let release
  api.chat.mockImplementationOnce(
    () =>
      new Promise((r) => {
        release = r
      })
  )
  return (reply = { text: 'Listo.', tools: [] }) => release(reply)
}

describe('the box: keys', () => {
  it('Enter sends and leaves no line break', async () => {
    const w = box({ modelValue: 'hola' })
    const ev = press(w.find('textarea').element)
    expect(ev.defaultPrevented).toBe(true)
    expect(w.emitted('send')).toHaveLength(1)
  })

  it('Shift+Enter adds a new line and does not send', async () => {
    const w = box({ modelValue: 'agrégame esto a la lista:' })
    const ev = press(w.find('textarea').element, { shiftKey: true })
    // lo pone el navegador: no se le quita
    expect(ev.defaultPrevented).toBe(false)
    expect(w.emitted('send')).toBeUndefined()
  })

  it('Ctrl+Enter and Cmd+Enter also send', async () => {
    const w = box({ modelValue: 'hola' })
    expect(press(w.find('textarea').element, { ctrlKey: true }).defaultPrevented).toBe(true)
    press(w.find('textarea').element, { metaKey: true })
    press(w.find('textarea').element, { ctrlKey: true, shiftKey: true })
    expect(w.emitted('send')).toHaveLength(3)
  })

  it('while composing with an IME, Enter belongs to the IME and does not send', async () => {
    const w = box({ modelValue: 'canción' })
    const a = press(w.find('textarea').element, { isComposing: true })
    const b = press(w.find('textarea').element, { keyCode: 229 })
    expect(a.defaultPrevented || b.defaultPrevented).toBe(false)
    expect(w.emitted('send')).toBeUndefined()
  })

  it('empty (or whitespace only) does not send, and the button is disabled', async () => {
    const w = box({ modelValue: '  \n ' })
    press(w.find('textarea').element)
    expect(w.emitted('send')).toBeUndefined()
    expect(w.find('button[title="Enviar"]').attributes('disabled')).toBeDefined()
    await w.setProps({ modelValue: 'hola' })
    expect(w.find('button[title="Enviar"]').attributes('disabled')).toBeUndefined()
    await w.find('button[title="Enviar"]').trigger('click')
    expect(w.emitted('send')).toHaveLength(1)
  })

  it('holding Enter down does not send a burst', async () => {
    const w = box({ modelValue: 'hola' })
    const ev = press(w.find('textarea').element, { repeat: true })
    expect(ev.defaultPrevented).toBe(true)
    expect(w.emitted('send')).toBeUndefined()
  })

  it('pasting several lines keeps them', async () => {
    const list = 'agrégame esto a la lista:\n*1.-Sublime Gracia* ✅\n2.- Cuán Grande Es Dios'
    const w = box()
    await w.find('textarea').setValue(list)
    expect(w.emitted('update:modelValue').at(-1)).toEqual([list])
    expect(w.find('textarea').element.value).toBe(list)
  })
})

describe('the box: shape and hint', () => {
  it('grows with the text, scrolls past the cap and returns to one line when emptied', async () => {
    const w = box()
    const ta = w.find('textarea').element
    // jsdom no mide: el alto del contenido lo decide la prueba
    let height = 21
    const cap = 168 // las 8 líneas del CSS
    Object.defineProperty(ta, 'scrollHeight', { configurable: true, get: () => height })
    Object.defineProperty(ta, 'clientHeight', {
      configurable: true,
      get: () => Math.min(parseFloat(ta.style.height) || 0, cap)
    })
    height = 105
    await w.find('textarea').setValue('1\n2\n3\n4\n5')
    expect(ta.style.height).toBe('105px')
    expect(ta.style.overflowY).toBe('')
    height = 420
    await w.find('textarea').setValue('x\n'.repeat(20))
    expect(ta.style.height).toBe('420px')
    // no cabe: barra por dentro (el tope lo pone max-height)
    expect(ta.style.overflowY).toBe('auto')
    // enviar la vacía desde fuera: vuelve a una línea
    height = 21
    await w.setProps({ modelValue: '' })
    await nextTick()
    expect(ta.style.height).toBe('21px')
    expect(ta.style.overflowY).toBe('')
    expect(ta.getAttribute('rows')).toBe('1')
  })

  it('the cap is about 8 lines and never more than 35 % of the window height', () => {
    const rule = CSS.match(/\.composer-input \{([^}]*)\}/)
    expect(rule, 'falta el estilo del textarea').toBeTruthy()
    expect(rule[1]).toMatch(/max-height:\s*min\(35vh,\s*12em\)/)
    expect(rule[1]).toMatch(/line-height:\s*1\.5/)
    expect(rule[1]).toMatch(/resize:\s*none/)
    // una línea de 5000 letras sin espacios se parte, no ensancha la caja
    expect(rule[1]).toMatch(/overflow-wrap:\s*anywhere/)
  })

  it('has its name, its shortcuts, its length cap and the hint', async () => {
    const w = box({ maxlength: DRAFT_MAX })
    const ta = w.find('textarea')
    expect(DRAFT_MAX).toBe(50000)
    expect(ta.attributes('maxlength')).toBe('50000')
    expect(ta.attributes('aria-label')).toBe('Mensaje para el asistente')
    expect(ta.attributes('aria-keyshortcuts')).toContain('Enter')
    expect(ta.attributes('aria-keyshortcuts')).toContain('Control+Enter')
    const hint = document.getElementById(ta.attributes('aria-describedby'))
    expect(hint, 'la ayuda tiene que estar enlazada').toBeTruthy()
    expect(hint.textContent.replace(/\s+/g, ' ')).toContain(
      'Enter envía · Shift+Enter nueva línea · admite markdown'
    )
  })

  it('the hint is hidden in windows under 520 px tall', () => {
    const block = CSS.match(/@media \(max-height: 520px\) \{([\s\S]*?)\n\}/)
    expect(block, 'falta la regla de ventana baja').toBeTruthy()
    expect(block[1]).toMatch(/\.composer-help \{\s*display: none;/)
  })

  it('near the length cap it shows how much has been typed', async () => {
    const w = box({ modelValue: 'x'.repeat(46000) })
    expect(w.find('.composer-count').text()).toBe('46.000 / 50.000')
    await w.setProps({ modelValue: 'x' })
    expect(w.find('.composer-count').exists()).toBe(false)
  })

  it('while answering: Enter queues, the button says so and «Parar» is there', async () => {
    const w = box({ modelValue: 'otra cosa', thinking: true })
    const send = w.find('.composer-send')
    expect(send.attributes('title')).toBe('Dejar en cola')
    expect(send.attributes('aria-label')).toBe('Dejar en cola')
    expect(w.find('.composer-help').text()).toContain('lo deja en cola')
    press(w.find('textarea').element)
    expect(w.emitted('send')).toHaveLength(1)
    await w.find('.chat-cancel').trigger('click')
    expect(w.emitted('stop')).toHaveLength(1)
    expect(w.find('.chat-cancel').attributes('aria-label')).toBe('Parar la respuesta')
  })

  it('one chip per queued message: 40 characters, the remaining lines and its ×', async () => {
    const w = box({
      thinking: true,
      queue: [
        { id: 1, text: 'agrégame esto a la lista:\n\n1. Sublime Gracia\n2. Cuán Grande Es Dios' },
        { id: 2, text: 'b'.repeat(100) },
        { id: 3, text: 'una sola\nmás' }
      ]
    })
    const chips = w.findAll('.composer-chip')
    expect(chips.map((c) => c.find('.composer-chip-txt').text())).toEqual([
      'agrégame esto a la lista: (+2 líneas)',
      'b'.repeat(40) + '…',
      'una sola (+1 línea)'
    ])
    expect(w.find('.composer-chips').attributes('aria-labelledby')).toBeTruthy()
    const x = chips[0].find('button')
    expect(x.attributes('aria-label')).toBe(
      'Quitar de la cola: agrégame esto a la lista: (+2 líneas)'
    )
    await x.trigger('click')
    expect(w.emitted('unqueue')).toEqual([[1]])
    // llena: el botón de enviar se apaga y dice por qué
    await w.setProps({ modelValue: 'cuarta' })
    expect(w.find('.composer-send').attributes('disabled')).toBeDefined()
    expect(w.find('.composer-send').attributes('title')).toBe('Ya hay 3 mensajes en cola')
    expect(w.find('[role="status"]').text()).toContain('3 mensajes en cola')
  })

  it('a character made of two units (an emoji) is not split in the chip', async () => {
    const w = box({ thinking: true, queue: [{ id: 1, text: '🙏'.repeat(45) }] })
    expect(w.find('.composer-chip-txt').text()).toBe('🙏'.repeat(40) + '…')
  })
})

describe('sending from the page', () => {
  it('a real Enter sends ONE message with all its line breaks and empties the box', async () => {
    const w = await mountView()
    const list = 'agrégame esto a la lista:\n*1.-Sublime Gracia* ✅\n2.- Cuán Grande Es Dios'
    await typeText(w, list)
    expect(api.chatStart).toHaveBeenCalledTimes(1)
    expect(sentTexts()).toEqual([{ role: 'me', text: list }])
    expect(w.find('.chat-foot textarea').element.value).toBe('')
    expect(useChat().draft.value).toBe('')
  })

  it('Shift+Enter on the page sends nothing', async () => {
    const w = await mountView()
    await w.find('.chat-foot textarea').setValue('primera línea')
    press(w.find('.chat-foot textarea').element, { shiftKey: true })
    await flushPromises()
    expect(api.chatStart).not.toHaveBeenCalled()
  })

  it('the draft is kept when leaving the chat and coming back', async () => {
    const w = await mountView()
    await w.find('.chat-foot textarea').setValue('a medias\nsegunda')
    w.unmount()
    const w2 = await mountView()
    expect(w2.find('.chat-foot textarea').element.value).toBe('a medias\nsegunda')
  })
})

describe('the bubble of what you type', () => {
  const bubble = async (text) => {
    const w = await mountView()
    await typeText(w, text)
    return { w, b: w.find('.chat-msg.me .chat-bubble') }
  }

  it('standard markdown: **bold**, *italic*, headings and lists', async () => {
    const { b } = await bubble(
      '# Domingo\n**Sublime Gracia** y *Cuán Grande*\n\n- una\n- dos\n\n1. tres\n2. cuatro'
    )
    expect(b.classes()).toContain('chat-md')
    expect(b.find('h1').text()).toBe('Domingo')
    expect(b.find('strong').text()).toBe('Sublime Gracia')
    expect(b.find('em').text()).toBe('Cuán Grande')
    expect(b.findAll('ul li').map((l) => l.text())).toEqual(['una', 'dos'])
    expect(b.findAll('ol li').map((l) => l.text())).toEqual(['tres', 'cuatro'])
    expect(b.text()).not.toContain('**')
  })

  it('every line break shows', async () => {
    const { b } = await bubble('línea uno\nlínea dos\nlínea tres\n\notro párrafo')
    expect(b.findAll('p')).toHaveLength(2)
    expect(b.find('p').findAll('br')).toHaveLength(2)
    expect(b.find('p').html()).toContain('línea uno<br>línea dos<br>línea tres')
  })

  it('a «#» without a space or a lone asterisk turns into nothing', async () => {
    const { b } = await bubble('#domingo y 3 * 4 = 12')
    expect(b.find('h1').exists()).toBe(false)
    expect(b.find('em').exists()).toBe(false)
    expect(b.text()).toBe('#domingo y 3 * 4 = 12')
  })

  it('no HTML or javascript: link slips through', async () => {
    const { b } = await bubble(
      '<img src=x onerror=alert(1)>\n<script>alert(2)</script>\n[pulsa](javascript:alert(3)) **ok**'
    )
    expect(b.find('img').exists()).toBe(false)
    expect(b.find('script').exists()).toBe(false)
    expect(b.find('a').exists()).toBe(false)
    expect(b.find('[onerror]').exists()).toBe(false)
    expect(b.text()).toContain('<img src=x onerror=alert(1)>')
    expect(b.text()).toContain('<script>alert(2)</script>')
    expect(b.text()).toContain('[pulsa](javascript:alert(3))')
    expect(b.find('strong').text()).toBe('ok')
  })

  it('errors and app messages stay as plain text', async () => {
    api.chat.mockResolvedValueOnce({ error: 'fallo con **asteriscos**' })
    const w = await mountView()
    await typeText(w, 'hola')
    const err = w.find('.chat-msg.error .chat-bubble')
    expect(err.classes()).not.toContain('chat-md')
    expect(err.text()).toBe('fallo con **asteriscos**')
  })

  it('more than 12 lines collapses, with an accessible «Ver más / Ver menos»', async () => {
    const longText = Array.from({ length: 13 }, (_, i) => `${i + 1}.- tema ${i + 1}`).join('\n')
    const { w, b } = await bubble(longText)
    expect(b.classes()).toContain('chat-folded')
    const button = w.find('.chat-more')
    expect(button.text()).toBe('Ver más')
    expect(button.attributes('aria-expanded')).toBe('false')
    expect(button.attributes('aria-controls')).toBe(b.attributes('id'))
    await button.trigger('click')
    expect(w.find('.chat-msg.me .chat-bubble').classes()).not.toContain('chat-folded')
    expect(w.find('.chat-more').text()).toBe('Ver menos')
    expect(w.find('.chat-more').attributes('aria-expanded')).toBe('true')
    await w.find('.chat-more').trigger('click')
    expect(w.find('.chat-msg.me .chat-bubble').classes()).toContain('chat-folded')
  })

  it('12 lines do not collapse; a single very long line does', async () => {
    const twelve = Array.from({ length: 12 }, (_, i) => `tema ${i}`).join('\n')
    const { w } = await bubble(twelve)
    expect(w.find('.chat-more').exists()).toBe(false)
    await typeText(w, 'x'.repeat(5000))
    expect(w.findAll('.chat-msg.me').at(-1).find('.chat-more').exists()).toBe(true)
  })

  it('the markdown cache does not store texts over 20 KB', async () => {
    const big = 'línea con *algo*\n'.repeat(1400) // ~24 KB
    const small = 'chico *uno*'
    store.chats = [
      {
        id: 1,
        title: 'Lista',
        messages: [
          { role: 'me', text: big },
          { role: 'me', text: small }
        ]
      }
    ]
    const w = await mountView()
    const times = (t) => renderMarkdown.mock.calls.filter((c) => c[0] === t).length
    const before = { big: times(big), small: times(small) }
    expect(before.big).toBeGreaterThan(0)
    // cualquier repintado del chat (aquí, escribir en la caja)
    await w.find('.chat-foot textarea').setValue('algo')
    await nextTick()
    expect(times(big)).toBeGreaterThan(before.big)
    expect(times(small)).toBe(before.small)
  })
})

describe('the message queue', () => {
  it('while answering, Enter queues them (up to 3), they go out in order and × removes one', async () => {
    const finish = thinking()
    const w = await mountView()
    await typeText(w, 'primera')
    expect(useChat().thinking.value).toBe(true)
    await typeText(w, 'segunda')
    await typeText(w, 'tercera\ncon dos\nlíneas')
    await typeText(w, 'cuarta')
    expect(QUEUE_MAX).toBe(3)
    expect(w.findAll('.composer-chip').map((c) => c.find('.composer-chip-txt').text())).toEqual([
      'segunda',
      'tercera (+2 líneas)',
      'cuarta'
    ])
    // la caja se vacía al entrar en cola
    expect(w.find('.chat-foot textarea').element.value).toBe('')
    // la cuarta no cabe: se queda en la caja y se dice
    await typeText(w, 'quinta')
    expect(w.findAll('.composer-chip')).toHaveLength(3)
    expect(w.find('.chat-foot textarea').element.value).toBe('quinta')
    expect(useNotices().notices.value.some((n) => n.message.includes('3 mensajes en cola'))).toBe(
      true
    )
    // nada de eso se ha mandado todavía
    expect(api.chat).toHaveBeenCalledTimes(1)
    // ni se ve como mensaje tuyo
    expect(w.findAll('.chat-msg.me')).toHaveLength(1)
    // la × quita la segunda
    await w.findAll('.composer-chip')[0].find('button').trigger('click')
    expect(w.findAll('.composer-chip')).toHaveLength(2)
    finish()
    await flushPromises()
    await flushPromises()
    expect(sentTexts().map((m) => m.text)).toEqual([
      'primera',
      'tercera\ncon dos\nlíneas',
      'cuarta'
    ])
    expect(w.findAll('.composer-chip')).toHaveLength(0)
    // la quinta sigue en la caja: no se manda sola
    expect(w.find('.chat-foot textarea').element.value).toBe('quinta')
  })

  it('«Parar» cuts off the reply but does not clear the queue', async () => {
    const finish = thinking()
    const w = await mountView()
    await typeText(w, 'cuéntame algo largo')
    await typeText(w, 'y luego esto')
    await w.find('.chat-cancel').trigger('click')
    await flushPromises()
    expect(api.chatCancel).toHaveBeenCalledWith('j1')
    expect(w.findAll('.composer-chip')).toHaveLength(1)
    finish({ text: '', canceled: true, tools: [] })
    await flushPromises()
    await flushPromises()
    expect(sentTexts().map((m) => m.text)).toEqual(['cuéntame algo largo', 'y luego esto'])
  })

  it('with a dialog open it does not go out: it waits for it to close', async () => {
    const finish = thinking()
    const w = await mountView()
    await typeText(w, 'primera')
    await typeText(w, 'después del diálogo')
    // un diálogo de la app (aquí uno cualquiera) abierto al acabar el turno
    ask({ kind: 'confirm', title: 'Otra cosa', message: '¿Seguro?' })
    finish()
    await flushPromises()
    await flushPromises()
    expect(useChat().thinking.value).toBe(false)
    expect(api.chat).toHaveBeenCalledTimes(1)
    expect(w.findAll('.composer-chip')).toHaveLength(1)
    // y con el diálogo abierto, lo nuevo también espera
    await typeText(w, 'otra más')
    expect(api.chat).toHaveBeenCalledTimes(1)
    dialogCancel()
    await flushPromises()
    await flushPromises()
    expect(useDialog().dialog.value.open).toBe(false)
    expect(sentTexts().map((m) => m.text)).toEqual(['primera', 'después del diálogo', 'otra más'])
  })

  it('the download-finished notice goes out before what was waiting in the queue', async () => {
    vi.useFakeTimers()
    localStorage.setItem('danplay.chat.download', JSON.stringify({ items: ['x'], force: false }))
    api.youtube.mockResolvedValueOnce({
      active: false,
      phase: 'done',
      results: [{ ok: true, id: 301, artist: 'Autor', song: 'Tema', requested: 'x' }]
    })
    const finish = thinking()
    const w = await mountView()
    await typeText(w, '¿de qué año es?')
    await typeText(w, 'y otra pregunta')
    await vi.advanceTimersByTimeAsync(1000) // termina la descarga
    expect(api.chat).toHaveBeenCalledTimes(1)
    finish({ text: 'De 1959.', tools: [] })
    await vi.advanceTimersByTimeAsync(10)
    await flushPromises()
    await flushPromises()
    const sent = sentTexts()
    expect(sent).toHaveLength(3)
    expect(sent[1].event).toBe('download_done')
    expect(sent[2].text).toBe('y otra pregunta')
  })
})

describe('the draft kept safe', () => {
  it('if the core does not receive the message, it goes back to the box and does not count as sent', async () => {
    api.chatStart.mockRejectedValueOnce(new Error('Failed to fetch'))
    const w = await mountView()
    const list = 'agrégame esto:\n1. Sublime Gracia\n2. Cuán Grande Es Dios'
    await typeText(w, list)
    expect(w.find('.chat-foot textarea').element.value).toBe(list)
    expect(w.findAll('.chat-msg.me')).toHaveLength(0)
    expect(w.find('.chat-msg.error').exists()).toBe(false)
    // no se guardó en la conversación
    expect(store.chats.flatMap((c) => c.messages)).toEqual([])
    expect(
      useNotices().notices.value.some((n) => n.message.includes('No se pudo enviar el mensaje'))
    ).toBe(true)
    expect(useChat().thinking.value).toBe(false)
    // reintentar funciona y ahora sí queda
    press(w.find('.chat-foot textarea').element)
    await flushPromises()
    expect(sentTexts().map((m) => m.text)).toEqual([list])
    expect(store.chats[0].messages.map((m) => m.role)).toEqual(['me', 'ai'])
  })

  it('also if the conversation could not even be created (core down)', async () => {
    api.chatCreate.mockRejectedValueOnce(new Error('503: arrancando'))
    const w = await mountView()
    await typeText(w, 'hola')
    expect(w.find('.chat-foot textarea').element.value).toBe('hola')
    expect(api.chatStart).not.toHaveBeenCalled()
  })

  it('a queued message that does not arrive goes back to the box with the rest, in order', async () => {
    const finish = thinking()
    const w = await mountView()
    await typeText(w, 'primera')
    await typeText(w, 'segunda')
    await typeText(w, 'tercera')
    await w.find('.chat-foot textarea').setValue('a medio escribir')
    api.chatStart.mockRejectedValueOnce(new Error('Failed to fetch'))
    finish()
    await flushPromises()
    await flushPromises()
    expect(w.find('.chat-foot textarea').element.value).toBe(
      'segunda\n\ntercera\n\na medio escribir'
    )
    expect(useChat().queue.value).toEqual([])
  })
})

describe('only the latest goes to the core', () => {
  const history = (n) =>
    Array.from({ length: n }, (_, i) => ({
      role: i % 2 ? 'ai' : 'me',
      text: `mensaje ${i}`
    }))

  it(`with 150 in the history the core gets ${CORE_MESSAGES}`, async () => {
    expect(CORE_MESSAGES).toBe(60)
    store.chats = [{ id: 1, title: 'Larga', messages: history(149) }]
    const w = await mountView()
    await typeText(w, 'y ahora?')
    const sent = api.chatStart.mock.calls.at(-1)[0]
    expect(sent).toHaveLength(60)
    expect(sent[0]).toEqual({ role: 'me', text: 'mensaje 90' })
    expect(sent.at(-1)).toEqual({ role: 'me', text: 'y ahora?' })
  })

  it('and starting with one of yours, as the core trims it', async () => {
    store.chats = [{ id: 1, title: 'Larga', messages: history(150) }]
    const w = await mountView()
    await typeText(w, 'y ahora?')
    const sent = api.chatStart.mock.calls.at(-1)[0]
    expect(sent).toHaveLength(59)
    expect(sent[0]).toEqual({ role: 'me', text: 'mensaje 92' })
  })
})
