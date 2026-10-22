import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'

// Las conversaciones se guardan en el nucleo (aqui, en memoria): asi
// «recuerda la conversacion al reabrir» sigue valiendo sin localStorage.
// La respuesta en vivo (chatStart/chatPoll) delega en `api.chat`: lo que
// las pruebas programan ahi es lo que llega al final del sondeo.
const { api, store } = vi.hoisted(() => {
  const store = { chats: [], last: null }
  const api = {
    chatTools: vi.fn(async () => ({
      model: 'Qwen/Qwen3-Next-80B-A3B-Instruct',
      available: true,
      tools: []
    })),
    chat: vi.fn(async () => ({ text: 'Tienes 260 canciones.', tools: [] })),
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
    chatDelete: vi.fn(async (id) => {
      store.chats = store.chats.filter((x) => x.id !== id)
      return { ok: true }
    }),
    chatSearch: vi.fn(async () => ({ hits: [] })),
    chatConfirm: vi.fn(async () => ({ ok: true, result: {}, text: 'Hecho.' })),
    youtube: vi.fn(async () => ({ available: true, active: false, results: [] })),
    downloadFolder: vi.fn(async () => ({
      path: '/musica',
      ready: true,
      reason: '',
      suggested: '/musica'
    }))
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
import ChatPage from '../src/components/ChatPage.vue'
import { dialogOk, dialogCancel, useDialog } from '../src/composables/useDialog.js'
import { resetDownloads } from '../src/composables/useDownloads.js'
import { connectChat, resetChat, titleFrom } from '../src/composables/useChat.js'

// Lo que la app hace por el chat (reproducir, recargar). La conversacion ya
// no vive en el componente: las ordenes van a quien se conecto, que en la
// app de verdad es App.vue y no se desmonta nunca.
const app = { reload: vi.fn(), action: vi.fn() }

beforeEach(() => {
  localStorage.clear()
  vi.clearAllMocks()
  vi.useRealTimers()
  resetDownloads()
  resetChat()
  connectChat(app)
  store.chats = []
  store.last = null
})

const mountView = async () => {
  const w = mount(ChatPage, { attachTo: document.body })
  await flushPromises()
  return w
}
const typeText = async (w, t) => {
  await w.find('.chat-foot textarea').setValue(t)
  await w.find('.chat-foot .btn').trigger('click')
  await flushPromises()
}

describe('chat', () => {
  it('on open it shows suggestions and says what it talks about', async () => {
    const w = await mountView()
    expect(w.findAll('.chat-suggest .chip').length).toBeGreaterThan(2)
    // ya si descarga; lo que hay que avisar ahora es que solo lleva musica
    expect(w.text()).toContain('Solo hablo de musica')
    expect(w.text()).not.toContain('No puedo descargar')
  })

  it('shows the model it uses', async () => {
    const w = await mountView()
    expect(w.find('.chat-model').text()).toContain('Qwen')
  })

  it('sending renders your message and the reply', async () => {
    const w = await mountView()
    await typeText(w, '¿cuantas canciones tengo?')
    expect(w.findAll('.chat-msg.me')).toHaveLength(1)
    expect(w.findAll('.chat-msg.ai')).toHaveLength(1)
    expect(w.text()).toContain('Tienes 260 canciones')
  })

  it('sends the whole history, not just the last message', async () => {
    const w = await mountView()
    await typeText(w, 'hola')
    await typeText(w, 'y ahora?')
    const last = api.chat.mock.calls.at(-1)[0]
    expect(last).toHaveLength(3) // yo, ia, yo
    expect(last[0]).toEqual({ role: 'me', text: 'hola' })
  })

  it('the history carries which tools each reply used and what they returned', async () => {
    // Con eso el nucleo marca lo que se hizo de verdad en cada mensaje, el
    // modelo no toma sus propias frases («ya la cree») por hechos, y conserva
    // los ids y nombres que enseño («la segunda», «esa»).
    api.chat.mockResolvedValueOnce({
      text: 'Lista creada',
      tools: [
        {
          name: 'create_playlist',
          summary: '4 temas',
          args: { x: 1 },
          detail: '«X» (id 3): id 1 «A - B»'
        }
      ]
    })
    const w = await mountView()
    await typeText(w, 'crea una lista')
    await typeText(w, 'gracias')
    const last = api.chat.mock.calls.at(-1)[0]
    expect(last[1]).toEqual({
      role: 'ai',
      text: 'Lista creada',
      tools: [{ name: 'create_playlist', summary: '4 temas', detail: '«X» (id 3): id 1 «A - B»' }]
    })
    expect(last[0]).toEqual({ role: 'me', text: 'crea una lista' })
  })

  it('shows which tools it used', async () => {
    api.chat.mockResolvedValueOnce({
      text: 'Lista creada',
      tools: [{ name: 'create_playlist', summary: '4 temas' }]
    })
    const w = await mountView()
    await typeText(w, 'armame una lista')
    expect(w.find('.chat-tools').text()).toContain('creo una lista')
    expect(w.find('.chat-tools').text()).toContain('4 temas')
  })

  it('notifies the app when the AI creates a playlist', async () => {
    api.chat.mockResolvedValueOnce({
      text: 'ok',
      tools: [{ name: 'create_playlist', summary: 'x' }]
    })
    const w = await mountView()
    await typeText(w, 'crea una lista')
    expect(app.reload).toHaveBeenCalled()
  })

  it('a backend error is shown without breaking the chat', async () => {
    api.chat.mockResolvedValueOnce({ error: 'la IA no esta configurada' })
    const w = await mountView()
    await typeText(w, 'hola')
    expect(w.find('.chat-msg.error').text()).toContain('no esta configurada')
  })

  it('does not send empty messages', async () => {
    const w = await mountView()
    await w.find('.chat-foot textarea').setValue('   ')
    await w.find('.chat-foot .btn').trigger('click')
    await flushPromises()
    expect(api.chat).not.toHaveBeenCalled()
  })

  it('remembers the conversation on reopening', async () => {
    const w = await mountView()
    await typeText(w, 'hola')
    w.unmount()
    // como si se abriera la app de nuevo: lo que hay se lee del nucleo
    resetChat()
    const w2 = await mountView()
    expect(w2.text()).toContain('hola')
  })

  it('the send button has the send icon, not the «next song» one', async () => {
    const w = await mountView()
    const send = w.find('.chat-foot button[title="Enviar"]')
    expect(send.exists(), 'no encuentro el boton de enviar').toBe(true)
    const icon = w.findAllComponents({ name: 'Icon' }).find((i) => send.element.contains(i.element))
    expect(icon.props('n')).toBe('send')
    await w.find('.chat-foot textarea').setValue('hola')
    await send.trigger('click')
    await flushPromises()
    expect(api.chatStart).toHaveBeenCalled()
  })

  it('clicking a suggestion sends it', async () => {
    const w = await mountView()
    await w.findAll('.chat-suggest .chip')[0].trigger('click')
    await flushPromises()
    expect(api.chat).toHaveBeenCalled()
    expect(w.findAll('.chat-msg.me')).toHaveLength(1)
  })
})

// El nucleo no puede reproducir: el audio lo maneja Rust y la cola vive en la
// interfaz. Las herramientas de reproduccion devuelven una `action` y la app
// la ejecuta. Si esto se rompe, el asistente dice que puso la cancion y no
// suena nada.
describe('assistant commands to the app', () => {
  it('forwards the actions that come from the core', async () => {
    api.chat.mockResolvedValueOnce({
      text: 'Ya suena',
      tools: [{ name: 'play_song', summary: 'sonando: Barak - Mi Gozo' }],
      actions: [{ kind: 'play_song', song_id: 7 }]
    })
    const w = await mountView()
    await typeText(w, 'pon Mi Gozo')
    expect(app.action).toHaveBeenCalledTimes(1)
    expect(app.action.mock.calls[0][0]).toEqual({ kind: 'play_song', song_id: 7 })
  })

  it('without actions it emits nothing', async () => {
    api.chat.mockResolvedValueOnce({ text: 'Tienes 260 canciones.', tools: [] })
    const w = await mountView()
    await typeText(w, '¿cuantas tengo?')
    expect(app.action).not.toHaveBeenCalled()
  })

  it('reloads the library when the assistant changes it', async () => {
    api.chat.mockResolvedValueOnce({
      text: 'Puntuada',
      tools: [{ name: 'set_stars', summary: '4 estrellas' }]
    })
    const w = await mountView()
    await typeText(w, 'ponle 4 estrellas')
    expect(app.reload).toHaveBeenCalled()
  })
})

// El asistente contesta en markdown. Antes se pintaba tal cual, con los
// asteriscos y las almohadillas a la vista.
describe('the chat renders the assistant markdown', () => {
  it('bold and lists come out as HTML, not asterisks', async () => {
    api.chat.mockResolvedValueOnce({
      text: 'Tienes dos:\n\n1. **Mi Gozo** – Barak\n2. **Shekinah** – New Wine',
      tools: []
    })
    const w = await mountView()
    await typeText(w, 'que tengo de barak?')
    const bubble = w.find('.chat-msg.ai .chat-bubble')
    expect(bubble.classes()).toContain('chat-md')
    expect(bubble.findAll('li')).toHaveLength(2)
    expect(bubble.find('strong').text()).toBe('Mi Gozo')
    expect(bubble.text()).not.toContain('**')
  })

  it('what you type is also rendered as markdown, and goes to the core as is', async () => {
    // lo pidio el usuario: asteriscos, almohadillas y saltos de linea a la
    // vista; mas casos en chat-composer.test.js
    const w = await mountView()
    await typeText(w, 'busca **esto** literal')
    const mine = w.find('.chat-msg.me .chat-bubble')
    expect(mine.classes()).toContain('chat-md')
    expect(mine.find('strong').text()).toBe('esto')
    expect(api.chat.mock.calls.at(-1)[0].at(-1).text).toBe('busca **esto** literal')
  })

  it('a YouTube title with HTML inside does not slip through', async () => {
    api.chat.mockResolvedValueOnce({
      text: 'Encontre <img src=x onerror=alert(1)> y **esto**',
      tools: []
    })
    const w = await mountView()
    await typeText(w, 'busca')
    const bubble = w.find('.chat-msg.ai .chat-bubble')
    expect(bubble.find('img').exists()).toBe(false)
    expect(bubble.text()).toContain('<img src=x onerror=alert(1)>')
    expect(bubble.find('strong').exists()).toBe(true)
  })

  it('core errors show as plain text', async () => {
    api.chat.mockResolvedValueOnce({ error: 'la IA **no** esta configurada' })
    const w = await mountView()
    await typeText(w, 'hola')
    expect(w.find('.chat-msg.error .chat-bubble').text()).toContain('**no**')
  })
})

// Descargar no se hace dentro de la conversacion: el nucleo devuelve
// `confirm`, la app pregunta, y si aceptas arranca en segundo plano. El chat
// la sigue y, al terminar, cuenta que entro y que no. Antes se decia «te lo
// cuento en Descargas» y aqui no volvia a saberse nada; y el «bajala igual»
// (force) se perdia por el camino.
describe('downloads requested from the assistant', () => {
  const pending = {
    text: 'Te pido permiso.',
    tools: [{ name: 'download_music', summary: 'espera tu visto bueno' }],
    confirm: {
      tool: 'download_music',
      summary: 'Descargar de YouTube: «Ruja o Leão».',
      args: { items: ['Ruja o Leao Carol Braga'], force: true }
    }
  }

  it('on accept it sends the arguments to the core as is, force included', async () => {
    api.chat.mockResolvedValueOnce(pending)
    api.chatConfirm.mockResolvedValueOnce({
      ok: true,
      text: 'Descargando. Te cuento cuando termine.',
      result: { active: true, items: ['Ruja o Leao Carol Braga'], force: true }
    })
    const w = await mountView()
    await typeText(w, 'bajala igual')
    expect(useDialog().dialog.value.open).toBe(true)
    expect(useDialog().dialog.value.message).toContain('Ruja o Leão')
    dialogOk()
    await flushPromises()
    expect(api.chatConfirm).toHaveBeenCalledWith('download_music', {
      items: ['Ruja o Leao Carol Braga'],
      force: true
    })
    expect(w.text()).toContain('Descargando. Te cuento cuando termine.')
    // no hace falta recargar la biblioteca todavia: no ha entrado nada
    expect(app.reload).not.toHaveBeenCalled()
  })

  it('the first time it asks where to save it; without a folder nothing is downloaded', async () => {
    api.downloadFolder.mockResolvedValueOnce({
      path: '/home/x/Música',
      ready: false,
      reason: 'unset',
      suggested: '/musica'
    })
    api.chat.mockResolvedValueOnce(pending)
    const w = await mountView()
    await typeText(w, 'bajala igual')
    dialogOk() // el visto bueno a la descarga
    await flushPromises()
    expect(useDialog().dialog.value.title).toBe('¿Dónde guardo lo que descargues?')
    dialogCancel()
    await flushPromises()
    expect(api.chatConfirm).not.toHaveBeenCalled()
    expect(w.text()).toContain('sin carpeta de descargas no he bajado nada')
  })

  it('if you say no, nothing is called', async () => {
    api.chat.mockResolvedValueOnce(pending)
    const w = await mountView()
    await typeText(w, 'baja eso')
    dialogCancel()
    await flushPromises()
    expect(api.chatConfirm).not.toHaveBeenCalled()
    expect(w.text()).toContain('Cancelado, no he tocado nada.')
  })

  it('follows the download and, when done, reports what came in and what you already had', async () => {
    vi.useFakeTimers()
    api.chat.mockResolvedValueOnce(pending)
    api.chatConfirm.mockResolvedValueOnce({
      ok: true,
      text: 'Descargando 2 temas.',
      result: { active: true, items: ['a', 'b'], force: false }
    })
    api.youtube
      .mockResolvedValueOnce({
        active: true,
        phase: 'downloading',
        name: 'I Want Jesus',
        percent: 40,
        index: 1,
        total: 2
      })
      .mockResolvedValueOnce({
        active: false,
        phase: 'done',
        results: [
          {
            ok: true,
            id: 267,
            artist: 'Bethel Music',
            song: 'I Want Jesus (Live)',
            title: 'I Want Jesus'
          },
          {
            ok: false,
            already_there: true,
            title: 'Ruja o Leão - Carol Braga',
            matches: [{ id: 3, artist: 'Carol Braga', title: 'Ruja O Leao' }]
          }
        ]
      })
    const w = await mountView()
    await typeText(w, 'baja estas dos')
    dialogOk()
    await flushPromises()

    // la primera mirada es inmediata; las siguientes, cada 700 ms
    await vi.advanceTimersByTimeAsync(300)
    expect(w.find('.chat-downloading').exists()).toBe(true)
    expect(w.find('.chat-downloading').text()).toContain('Bajando')
    expect(w.find('.chat-downloading').text()).toContain('1/2')

    api.chat.mockResolvedValueOnce({
      text: 'Listo: lista «Herlin» creada con la que entro.',
      tools: [{ name: 'create_playlist', summary: 'lista «Herlin» con 1 temas' }]
    })
    await vi.advanceTimersByTimeAsync(1000)
    expect(w.find('.chat-downloading').exists()).toBe(false)
    const report = w.findAll('.chat-msg.ai').find((m) => m.text().includes('Descargada'))
    expect(report).toBeTruthy()
    expect(report.text()).toContain('Bethel Music - I Want Jesus (Live)')
    // el nombre de YouTube no era el de archivo: se dice, para que nadie crea
    // que entro otra cancion
    expect(report.text()).toContain('pediste «I Want Jesus»')
    expect(report.text()).toContain('Ya la tenías')
    expect(report.text()).toContain('Carol Braga - Ruja O Leao')
    expect(report.text()).toContain('otra versión')
    expect(report.find('.chat-tools').text()).toContain('1 descargada, 1 ya la tenías')
    // entro una: la biblioteca tiene que refrescarse
    expect(app.reload).toHaveBeenCalled()
    // y ya no sigue preguntando
    const calls = api.youtube.mock.calls.length
    await vi.advanceTimersByTimeAsync(3000)
    expect(api.youtube.mock.calls.length).toBe(calls)

    // Al entrar algo, se le pasa el turno al asistente para que remate lo que
    // quedara («y armame una lista»). El aviso va al nucleo como un mensaje
    // del usuario, pero no se pinta como si lo hubieras escrito tu.
    const sent = api.chat.mock.calls.at(-1)[0]
    expect(sent.at(-1).role).toBe('me')
    expect(sent.at(-1).text).toContain('La descarga ha terminado')
    // con los ids exactos de lo que entro y la correspondencia con lo pedido:
    // sin ellos el modelo se inventaba ids y creia que habia entrado otra cancion
    expect(sent.at(-1).text).toContain(
      'pediste «I Want Jesus» → entro como «Bethel Music - I Want Jesus (Live)» (id 267)'
    )
    expect(sent.at(-1).text).toContain('NO la vuelvas a descargar')
    expect(sent.at(-1).text).toContain('añadir a una lista no necesita confirmacion')
    // y cita lo que pediste, para que remate ESO y no lo que le parezca
    expect(sent.at(-1).text).toContain('Lo que te pedi fue: «baja estas dos»')
    // y el historial que recibe el nucleo lleva las herramientas de cada mensaje
    const withTools = sent.find((m) => m.tools?.some((t) => t.name === 'download_music'))
    expect(withTools).toBeTruthy()
    const bubbles = w.findAll('.chat-msg.me')
    expect(
      bubbles
        .filter((b) => b.isVisible())
        .some((b) => b.text().includes('La descarga ha terminado'))
    ).toBe(false)
    expect(w.text()).toContain('Listo: lista «Herlin» creada')
  })

  it('if you change page and come back, it resumes tracking', async () => {
    vi.useFakeTimers()
    localStorage.setItem('danplay.chat.download', JSON.stringify({ items: ['x'], force: false }))
    api.youtube.mockResolvedValueOnce({
      active: false,
      phase: 'done',
      results: [{ ok: true, artist: 'Barak', song: 'Mi Gozo' }]
    })
    const w = await mountView()
    await vi.advanceTimersByTimeAsync(1000)
    expect(w.findAll('.chat-msg.ai').some((m) => m.text().includes('Barak - Mi Gozo'))).toBe(true)
    expect(localStorage.getItem('danplay.chat.download')).toBeNull()
  })

  it('the notice to the assistant is tagged and has the ids; if the chat was busy, it waits its turn', async () => {
    vi.useFakeTimers()
    localStorage.setItem('danplay.chat.download', JSON.stringify({ items: ['x'], force: false }))
    api.youtube.mockResolvedValueOnce({
      active: false,
      phase: 'done',
      results: [{ ok: true, id: 301, artist: 'Barak', song: 'Mi Gozo', requested: 'x' }]
    })
    // el chat esta ocupado con otra pregunta cuando termina la descarga
    let release
    api.chat.mockImplementationOnce(
      () =>
        new Promise((res) => {
          release = res
        })
    )
    const w = await mountView()
    await typeText(w, '¿de que año es Kind of Blue?') // se queda pensando
    await vi.advanceTimersByTimeAsync(1000) // termina la descarga
    expect(w.text()).toContain('Barak - Mi Gozo')
    expect(api.chat).toHaveBeenCalledTimes(1) // el aviso NO se perdio ni se colo
    api.chat.mockResolvedValueOnce({ text: 'Terminado.', tools: [] })
    release({ text: 'De 1959.', tools: [] })
    await flushPromises()
    await flushPromises()
    await flushPromises()
    expect(api.chat).toHaveBeenCalledTimes(2)
    const sent = api.chat.mock.calls.at(-1)[0]
    expect(sent.at(-1).event).toBe('download_done')
    expect(sent.at(-1).text).toContain('«Barak - Mi Gozo» (id 301)')
    expect(sent.at(-1).text).toContain('responde solo: Terminado')
  })

  it('a download the chat did not request is not counted as its own', async () => {
    vi.useFakeTimers()
    localStorage.setItem(
      'danplay.chat.download',
      JSON.stringify({ items: ['mi gozo barak'], force: false })
    )
    api.youtube.mockResolvedValueOnce({
      active: false,
      phase: 'done',
      results: [
        { ok: true, id: 9, artist: 'Otro', song: 'Otra', requested: 'https://youtu.be/otra' }
      ]
    })
    const w = await mountView()
    await vi.advanceTimersByTimeAsync(1000)
    expect(w.text()).not.toContain('Otro - Otra')
    expect(api.chat).not.toHaveBeenCalled()
    expect(localStorage.getItem('danplay.chat.download')).toBeNull()
  })

  it('what the app writes is marked and the accepted download carries its tool', async () => {
    api.chat.mockResolvedValueOnce(pending)
    api.chatConfirm.mockResolvedValueOnce({
      ok: true,
      text: 'Descargando.',
      result: { active: true, items: ['a'], force: false }
    })
    const w = await mountView()
    await typeText(w, 'baja')
    dialogOk()
    await flushPromises()
    await typeText(w, 'gracias')
    const sent = api.chat.mock.calls.at(-1)[0]
    const accepted = sent.find((m) => m.text === 'Descargando.')
    expect(accepted.app).toBe(true)
    expect(accepted.tools).toEqual([{ name: 'download_music', summary: 'aceptada, en marcha' }])
  })

  it('a narrated reply renders marked', async () => {
    api.chat.mockResolvedValueOnce({
      text: 'Ya la creé.\n\n_(Nota de la app: …)_',
      tools: [],
      narrated: true
    })
    const w = await mountView()
    await typeText(w, 'crea la lista')
    const msg = w.findAll('.chat-msg.ai').at(-1)
    expect(msg.classes()).toContain('narrated')
    expect(msg.attributes('title')).toContain('ninguna herramienta')
  })

  it('if nothing came in, the assistant is not bothered', async () => {
    vi.useFakeTimers()
    localStorage.setItem('danplay.chat.download', JSON.stringify({ items: ['x'], force: false }))
    api.youtube.mockResolvedValueOnce({
      active: false,
      phase: 'done',
      results: [{ ok: false, already_there: true, title: 'Mi Gozo', matches: [] }]
    })
    const w = await mountView()
    await vi.advanceTimersByTimeAsync(1000)
    expect(w.text()).toContain('Ya la tenías')
    expect(api.chat).not.toHaveBeenCalled()
  })

  it('a failure on confirm is shown without breaking the chat', async () => {
    api.chat.mockResolvedValueOnce(pending)
    api.chatConfirm.mockRejectedValueOnce(new Error('409: ya hay una descarga en marcha'))
    const w = await mountView()
    await typeText(w, 'baja')
    dialogOk()
    await flushPromises()
    expect(w.find('.chat-msg.error').text()).toContain('ya hay una descarga en marcha')
  })
})

describe('the title of a conversation', () => {
  it('comes from the first line with text, without the WhatsApp markup', () => {
    const pasted = '\n  \nagrégame esto a la lista:\n*Domingo*✅\n*1.- Mi Gozo*'
    expect(titleFrom(pasted)).toBe('agrégame esto a la lista:')
    expect(titleFrom('*Domingo* _ensayo_ ~viejo~')).toBe('Domingo ensayo viejo')
    expect(titleFrom('x'.repeat(100))).toHaveLength(60)
    expect(titleFrom('   ')).toBe('Conversación')
  })
})
