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
      accepted: (await import('../src/data/terminos.js')).TERMS_VERSION,
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
function tecla(el, opts = {}) {
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
function caja(props = {}) {
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

const montar = async () => {
  const w = mount(ChatPage, { attachTo: document.body })
  await flushPromises()
  return w
}
const escribir = async (w, t) => {
  await w.find('.chat-foot textarea').setValue(t)
  tecla(w.find('.chat-foot textarea').element)
  await flushPromises()
}
/** Lo último que recibió el núcleo en cada turno. */
const enviados = () => api.chat.mock.calls.map((c) => c[0].at(-1))
/** Deja al asistente «pensando» hasta que se llame a lo devuelto. */
function pensando() {
  let release
  api.chat.mockImplementationOnce(
    () =>
      new Promise((r) => {
        release = r
      })
  )
  return (reply = { text: 'Listo.', tools: [] }) => release(reply)
}

describe('la caja: teclas', () => {
  it('Enter envía y no deja un salto de línea', async () => {
    const w = caja({ modelValue: 'hola' })
    const ev = tecla(w.find('textarea').element)
    expect(ev.defaultPrevented).toBe(true)
    expect(w.emitted('send')).toHaveLength(1)
  })

  it('Shift+Enter baja de línea y no envía', async () => {
    const w = caja({ modelValue: 'agrégame esto a la lista:' })
    const ev = tecla(w.find('textarea').element, { shiftKey: true })
    // lo pone el navegador: no se le quita
    expect(ev.defaultPrevented).toBe(false)
    expect(w.emitted('send')).toBeUndefined()
  })

  it('Ctrl+Enter y Cmd+Enter también envían', async () => {
    const w = caja({ modelValue: 'hola' })
    expect(tecla(w.find('textarea').element, { ctrlKey: true }).defaultPrevented).toBe(true)
    tecla(w.find('textarea').element, { metaKey: true })
    tecla(w.find('textarea').element, { ctrlKey: true, shiftKey: true })
    expect(w.emitted('send')).toHaveLength(3)
  })

  it('componiendo con un IME, Enter es del IME y no envía', async () => {
    const w = caja({ modelValue: 'canción' })
    const a = tecla(w.find('textarea').element, { isComposing: true })
    const b = tecla(w.find('textarea').element, { keyCode: 229 })
    expect(a.defaultPrevented || b.defaultPrevented).toBe(false)
    expect(w.emitted('send')).toBeUndefined()
  })

  it('vacía (o solo blancos) no envía, y el botón se apaga', async () => {
    const w = caja({ modelValue: '  \n ' })
    tecla(w.find('textarea').element)
    expect(w.emitted('send')).toBeUndefined()
    expect(w.find('button[title="Enviar"]').attributes('disabled')).toBeDefined()
    await w.setProps({ modelValue: 'hola' })
    expect(w.find('button[title="Enviar"]').attributes('disabled')).toBeUndefined()
    await w.find('button[title="Enviar"]').trigger('click')
    expect(w.emitted('send')).toHaveLength(1)
  })

  it('mantener Enter pulsado no manda una ráfaga', async () => {
    const w = caja({ modelValue: 'hola' })
    const ev = tecla(w.find('textarea').element, { repeat: true })
    expect(ev.defaultPrevented).toBe(true)
    expect(w.emitted('send')).toBeUndefined()
  })

  it('pegar varias líneas las conserva', async () => {
    const lista = 'agrégame esto a la lista:\n*1.-Sublime Gracia* ✅\n2.- Cuán Grande Es Dios'
    const w = caja()
    await w.find('textarea').setValue(lista)
    expect(w.emitted('update:modelValue').at(-1)).toEqual([lista])
    expect(w.find('textarea').element.value).toBe(lista)
  })
})

describe('la caja: forma y ayuda', () => {
  it('crece con lo escrito, se desplaza pasado el tope y vuelve a una línea al vaciarse', async () => {
    const w = caja()
    const ta = w.find('textarea').element
    // jsdom no mide: el alto del contenido lo decide la prueba
    let alto = 21
    const tope = 168 // las 8 líneas del CSS
    Object.defineProperty(ta, 'scrollHeight', { configurable: true, get: () => alto })
    Object.defineProperty(ta, 'clientHeight', {
      configurable: true,
      get: () => Math.min(parseFloat(ta.style.height) || 0, tope)
    })
    alto = 105
    await w.find('textarea').setValue('1\n2\n3\n4\n5')
    expect(ta.style.height).toBe('105px')
    expect(ta.style.overflowY).toBe('')
    alto = 420
    await w.find('textarea').setValue('x\n'.repeat(20))
    expect(ta.style.height).toBe('420px')
    // no cabe: barra por dentro (el tope lo pone max-height)
    expect(ta.style.overflowY).toBe('auto')
    // enviar la vacía desde fuera: vuelve a una línea
    alto = 21
    await w.setProps({ modelValue: '' })
    await nextTick()
    expect(ta.style.height).toBe('21px')
    expect(ta.style.overflowY).toBe('')
    expect(ta.getAttribute('rows')).toBe('1')
  })

  it('el tope es de unas 8 líneas y nunca más del 35 % del alto de la ventana', () => {
    const regla = CSS.match(/\.composer-input \{([^}]*)\}/)
    expect(regla, 'falta el estilo del textarea').toBeTruthy()
    expect(regla[1]).toMatch(/max-height:\s*min\(35vh,\s*12em\)/)
    expect(regla[1]).toMatch(/line-height:\s*1\.5/)
    expect(regla[1]).toMatch(/resize:\s*none/)
    // una línea de 5000 letras sin espacios se parte, no ensancha la caja
    expect(regla[1]).toMatch(/overflow-wrap:\s*anywhere/)
  })

  it('lleva su nombre, sus atajos, su tope de largo y la ayuda', async () => {
    const w = caja({ maxlength: DRAFT_MAX })
    const ta = w.find('textarea')
    expect(DRAFT_MAX).toBe(50000)
    expect(ta.attributes('maxlength')).toBe('50000')
    expect(ta.attributes('aria-label')).toBe('Mensaje para el asistente')
    expect(ta.attributes('aria-keyshortcuts')).toContain('Enter')
    expect(ta.attributes('aria-keyshortcuts')).toContain('Control+Enter')
    const ayuda = document.getElementById(ta.attributes('aria-describedby'))
    expect(ayuda, 'la ayuda tiene que estar enlazada').toBeTruthy()
    expect(ayuda.textContent.replace(/\s+/g, ' ')).toContain(
      'Enter envía · Shift+Enter nueva línea · admite markdown'
    )
  })

  it('la ayuda se oculta en ventanas de menos de 520 px de alto', () => {
    const bloque = CSS.match(/@media \(max-height: 520px\) \{([\s\S]*?)\n\}/)
    expect(bloque, 'falta la regla de ventana baja').toBeTruthy()
    expect(bloque[1]).toMatch(/\.composer-help \{\s*display: none;/)
  })

  it('cerca del tope de largo dice cuánto lleva', async () => {
    const w = caja({ modelValue: 'x'.repeat(46000) })
    expect(w.find('.composer-count').text()).toBe('46.000 / 50.000')
    await w.setProps({ modelValue: 'x' })
    expect(w.find('.composer-count').exists()).toBe(false)
  })

  it('respondiendo: Enter deja en cola, el botón lo dice y está «Parar»', async () => {
    const w = caja({ modelValue: 'otra cosa', thinking: true })
    const enviar = w.find('.composer-send')
    expect(enviar.attributes('title')).toBe('Dejar en cola')
    expect(enviar.attributes('aria-label')).toBe('Dejar en cola')
    expect(w.find('.composer-help').text()).toContain('lo deja en cola')
    tecla(w.find('textarea').element)
    expect(w.emitted('send')).toHaveLength(1)
    await w.find('.chat-cancel').trigger('click')
    expect(w.emitted('stop')).toHaveLength(1)
    expect(w.find('.chat-cancel').attributes('aria-label')).toBe('Parar la respuesta')
  })

  it('un chip por mensaje en cola: 40 letras, las líneas que faltan y su ×', async () => {
    const w = caja({
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

  it('una letra que son dos unidades (un emoji) no se parte en el chip', async () => {
    const w = caja({ thinking: true, queue: [{ id: 1, text: '🙏'.repeat(45) }] })
    expect(w.find('.composer-chip-txt').text()).toBe('🙏'.repeat(40) + '…')
  })
})

describe('enviar desde la página', () => {
  it('Enter de verdad manda UN mensaje con todos sus saltos de línea y vacía la caja', async () => {
    const w = await montar()
    const lista = 'agrégame esto a la lista:\n*1.-Sublime Gracia* ✅\n2.- Cuán Grande Es Dios'
    await escribir(w, lista)
    expect(api.chatStart).toHaveBeenCalledTimes(1)
    expect(enviados()).toEqual([{ role: 'me', text: lista }])
    expect(w.find('.chat-foot textarea').element.value).toBe('')
    expect(useChat().draft.value).toBe('')
  })

  it('Shift+Enter en la página no manda nada', async () => {
    const w = await montar()
    await w.find('.chat-foot textarea').setValue('primera línea')
    tecla(w.find('.chat-foot textarea').element, { shiftKey: true })
    await flushPromises()
    expect(api.chatStart).not.toHaveBeenCalled()
  })

  it('el borrador se conserva al salir del chat y volver', async () => {
    const w = await montar()
    await w.find('.chat-foot textarea').setValue('a medias\nsegunda')
    w.unmount()
    const w2 = await montar()
    expect(w2.find('.chat-foot textarea').element.value).toBe('a medias\nsegunda')
  })
})

describe('la burbuja de lo que escribes', () => {
  const burbuja = async (texto) => {
    const w = await montar()
    await escribir(w, texto)
    return { w, b: w.find('.chat-msg.me .chat-bubble') }
  }

  it('markdown estándar: **negrita**, *cursiva*, títulos y listas', async () => {
    const { b } = await burbuja(
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

  it('cada salto de línea se ve', async () => {
    const { b } = await burbuja('línea uno\nlínea dos\nlínea tres\n\notro párrafo')
    expect(b.findAll('p')).toHaveLength(2)
    expect(b.find('p').findAll('br')).toHaveLength(2)
    expect(b.find('p').html()).toContain('línea uno<br>línea dos<br>línea tres')
  })

  it('un «#» sin espacio o un asterisco suelto no se convierten en nada', async () => {
    const { b } = await burbuja('#domingo y 3 * 4 = 12')
    expect(b.find('h1').exists()).toBe(false)
    expect(b.find('em').exists()).toBe(false)
    expect(b.text()).toBe('#domingo y 3 * 4 = 12')
  })

  it('no se cuela HTML ni un enlace javascript:', async () => {
    const { b } = await burbuja(
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

  it('los errores y los mensajes de la app siguen en texto plano', async () => {
    api.chat.mockResolvedValueOnce({ error: 'fallo con **asteriscos**' })
    const w = await montar()
    await escribir(w, 'hola')
    const err = w.find('.chat-msg.error .chat-bubble')
    expect(err.classes()).not.toContain('chat-md')
    expect(err.text()).toBe('fallo con **asteriscos**')
  })

  it('más de 12 líneas se pliega, con «Ver más / Ver menos» accesible', async () => {
    const largo = Array.from({ length: 13 }, (_, i) => `${i + 1}.- tema ${i + 1}`).join('\n')
    const { w, b } = await burbuja(largo)
    expect(b.classes()).toContain('chat-folded')
    const boton = w.find('.chat-more')
    expect(boton.text()).toBe('Ver más')
    expect(boton.attributes('aria-expanded')).toBe('false')
    expect(boton.attributes('aria-controls')).toBe(b.attributes('id'))
    await boton.trigger('click')
    expect(w.find('.chat-msg.me .chat-bubble').classes()).not.toContain('chat-folded')
    expect(w.find('.chat-more').text()).toBe('Ver menos')
    expect(w.find('.chat-more').attributes('aria-expanded')).toBe('true')
    await w.find('.chat-more').trigger('click')
    expect(w.find('.chat-msg.me .chat-bubble').classes()).toContain('chat-folded')
  })

  it('12 líneas no se pliegan; una sola línea larguísima, sí', async () => {
    const doce = Array.from({ length: 12 }, (_, i) => `tema ${i}`).join('\n')
    const { w } = await burbuja(doce)
    expect(w.find('.chat-more').exists()).toBe(false)
    await escribir(w, 'x'.repeat(5000))
    expect(w.findAll('.chat-msg.me').at(-1).find('.chat-more').exists()).toBe(true)
  })

  it('la caché del markdown no guarda textos de más de 20 KB', async () => {
    const grande = 'línea con *algo*\n'.repeat(1400) // ~24 KB
    const chico = 'chico *uno*'
    store.chats = [
      {
        id: 1,
        title: 'Lista',
        messages: [
          { role: 'me', text: grande },
          { role: 'me', text: chico }
        ]
      }
    ]
    const w = await montar()
    const veces = (t) => renderMarkdown.mock.calls.filter((c) => c[0] === t).length
    const antes = { grande: veces(grande), chico: veces(chico) }
    expect(antes.grande).toBeGreaterThan(0)
    // cualquier repintado del chat (aquí, escribir en la caja)
    await w.find('.chat-foot textarea').setValue('algo')
    await nextTick()
    expect(veces(grande)).toBeGreaterThan(antes.grande)
    expect(veces(chico)).toBe(antes.chico)
  })
})

describe('la cola de mensajes', () => {
  it('respondiendo, Enter los deja en cola (hasta 3), salen en orden y × quita uno', async () => {
    const soltar = pensando()
    const w = await montar()
    await escribir(w, 'primera')
    expect(useChat().thinking.value).toBe(true)
    await escribir(w, 'segunda')
    await escribir(w, 'tercera\ncon dos\nlíneas')
    await escribir(w, 'cuarta')
    expect(QUEUE_MAX).toBe(3)
    expect(w.findAll('.composer-chip').map((c) => c.find('.composer-chip-txt').text())).toEqual([
      'segunda',
      'tercera (+2 líneas)',
      'cuarta'
    ])
    // la caja se vacía al entrar en cola
    expect(w.find('.chat-foot textarea').element.value).toBe('')
    // la cuarta no cabe: se queda en la caja y se dice
    await escribir(w, 'quinta')
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
    soltar()
    await flushPromises()
    await flushPromises()
    expect(enviados().map((m) => m.text)).toEqual(['primera', 'tercera\ncon dos\nlíneas', 'cuarta'])
    expect(w.findAll('.composer-chip')).toHaveLength(0)
    // la quinta sigue en la caja: no se manda sola
    expect(w.find('.chat-foot textarea').element.value).toBe('quinta')
  })

  it('«Parar» corta la respuesta pero no borra la cola', async () => {
    const soltar = pensando()
    const w = await montar()
    await escribir(w, 'cuéntame algo largo')
    await escribir(w, 'y luego esto')
    await w.find('.chat-cancel').trigger('click')
    await flushPromises()
    expect(api.chatCancel).toHaveBeenCalledWith('j1')
    expect(w.findAll('.composer-chip')).toHaveLength(1)
    soltar({ text: '', canceled: true, tools: [] })
    await flushPromises()
    await flushPromises()
    expect(enviados().map((m) => m.text)).toEqual(['cuéntame algo largo', 'y luego esto'])
  })

  it('con un diálogo abierto no sale: espera a que se cierre', async () => {
    const soltar = pensando()
    const w = await montar()
    await escribir(w, 'primera')
    await escribir(w, 'después del diálogo')
    // un diálogo de la app (aquí uno cualquiera) abierto al acabar el turno
    ask({ kind: 'confirm', title: 'Otra cosa', message: '¿Seguro?' })
    soltar()
    await flushPromises()
    await flushPromises()
    expect(useChat().thinking.value).toBe(false)
    expect(api.chat).toHaveBeenCalledTimes(1)
    expect(w.findAll('.composer-chip')).toHaveLength(1)
    // y con el diálogo abierto, lo nuevo también espera
    await escribir(w, 'otra más')
    expect(api.chat).toHaveBeenCalledTimes(1)
    dialogCancel()
    await flushPromises()
    await flushPromises()
    expect(useDialog().dialog.value.open).toBe(false)
    expect(enviados().map((m) => m.text)).toEqual(['primera', 'después del diálogo', 'otra más'])
  })

  it('el aviso de fin de descarga sale antes que lo que esperaba en la cola', async () => {
    vi.useFakeTimers()
    localStorage.setItem('danplay.chat.download', JSON.stringify({ items: ['x'], force: false }))
    api.youtube.mockResolvedValueOnce({
      active: false,
      phase: 'done',
      results: [{ ok: true, id: 301, artist: 'Autor', song: 'Tema', requested: 'x' }]
    })
    const soltar = pensando()
    const w = await montar()
    await escribir(w, '¿de qué año es?')
    await escribir(w, 'y otra pregunta')
    await vi.advanceTimersByTimeAsync(1000) // termina la descarga
    expect(api.chat).toHaveBeenCalledTimes(1)
    soltar({ text: 'De 1959.', tools: [] })
    await vi.advanceTimersByTimeAsync(10)
    await flushPromises()
    await flushPromises()
    const sent = enviados()
    expect(sent).toHaveLength(3)
    expect(sent[1].event).toBe('download_done')
    expect(sent[2].text).toBe('y otra pregunta')
  })
})

describe('el borrador a salvo', () => {
  it('si el núcleo no recibe el mensaje, vuelve a la caja y no cuenta como enviado', async () => {
    api.chatStart.mockRejectedValueOnce(new Error('Failed to fetch'))
    const w = await montar()
    const lista = 'agrégame esto:\n1. Sublime Gracia\n2. Cuán Grande Es Dios'
    await escribir(w, lista)
    expect(w.find('.chat-foot textarea').element.value).toBe(lista)
    expect(w.findAll('.chat-msg.me')).toHaveLength(0)
    expect(w.find('.chat-msg.error').exists()).toBe(false)
    // no se guardó en la conversación
    expect(store.chats.flatMap((c) => c.messages)).toEqual([])
    expect(
      useNotices().notices.value.some((n) => n.message.includes('No se pudo enviar el mensaje'))
    ).toBe(true)
    expect(useChat().thinking.value).toBe(false)
    // reintentar funciona y ahora sí queda
    tecla(w.find('.chat-foot textarea').element)
    await flushPromises()
    expect(enviados().map((m) => m.text)).toEqual([lista])
    expect(store.chats[0].messages.map((m) => m.role)).toEqual(['me', 'ai'])
  })

  it('también si ni siquiera se pudo crear la conversación (núcleo caído)', async () => {
    api.chatCreate.mockRejectedValueOnce(new Error('503: arrancando'))
    const w = await montar()
    await escribir(w, 'hola')
    expect(w.find('.chat-foot textarea').element.value).toBe('hola')
    expect(api.chatStart).not.toHaveBeenCalled()
  })

  it('un mensaje de la cola que no llega vuelve a la caja con el resto, en orden', async () => {
    const soltar = pensando()
    const w = await montar()
    await escribir(w, 'primera')
    await escribir(w, 'segunda')
    await escribir(w, 'tercera')
    await w.find('.chat-foot textarea').setValue('a medio escribir')
    api.chatStart.mockRejectedValueOnce(new Error('Failed to fetch'))
    soltar()
    await flushPromises()
    await flushPromises()
    expect(w.find('.chat-foot textarea').element.value).toBe(
      'segunda\n\ntercera\n\na medio escribir'
    )
    expect(useChat().queue.value).toEqual([])
  })
})

describe('al núcleo, solo lo último', () => {
  const historial = (n) =>
    Array.from({ length: n }, (_, i) => ({
      role: i % 2 ? 'ai' : 'me',
      text: `mensaje ${i}`
    }))

  it(`con 150 en el historial al núcleo llegan ${CORE_MESSAGES}`, async () => {
    expect(CORE_MESSAGES).toBe(60)
    store.chats = [{ id: 1, title: 'Larga', messages: historial(149) }]
    const w = await montar()
    await escribir(w, 'y ahora?')
    const sent = api.chatStart.mock.calls.at(-1)[0]
    expect(sent).toHaveLength(60)
    expect(sent[0]).toEqual({ role: 'me', text: 'mensaje 90' })
    expect(sent.at(-1)).toEqual({ role: 'me', text: 'y ahora?' })
  })

  it('y empezando por uno tuyo, como recorta el núcleo', async () => {
    store.chats = [{ id: 1, title: 'Larga', messages: historial(150) }]
    const w = await montar()
    await escribir(w, 'y ahora?')
    const sent = api.chatStart.mock.calls.at(-1)[0]
    expect(sent).toHaveLength(59)
    expect(sent[0]).toEqual({ role: 'me', text: 'mensaje 92' })
  })
})
