import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'

const { api } = vi.hoisted(() => ({
  api: {
    chatTools: vi.fn(async () => ({ model: 'Qwen/Qwen3-Next-80B-A3B-Instruct',
                                           available: true, tools: [] })),
    chat: vi.fn(async () => ({ text: 'Tienes 260 canciones.', tools: [] })),
    chatConfirm: vi.fn(async () => ({ ok: true, result: {}, text: 'Hecho.' })),
    youtube: vi.fn(async () => ({ available: true, active: false, results: [] }))
  }
}))
vi.mock('../src/api.js', () => ({
  api, native: { available: false },
  errorMessage: (e) => String(e?.message || e)
}))
import ChatPage from '../src/components/ChatPage.vue'
import { dialogOk, dialogCancel, useDialog } from '../src/composables/useDialog.js'
import { resetDownloads } from '../src/composables/useDownloads.js'

beforeEach(() => { localStorage.clear(); vi.clearAllMocks(); vi.useRealTimers(); resetDownloads() })

const montar = async () => {
  const w = mount(ChatPage, { attachTo: document.body })
  await flushPromises()
  return w
}
const escribir = async (w, t) => {
  await w.find('.chat-foot input').setValue(t)
  await w.find('.chat-foot .btn').trigger('click')
  await flushPromises()
}

describe('chat', () => {
  it('al abrir muestra sugerencias y dice de que habla', async () => {
    const w = await montar()
    expect(w.findAll('.chat-suggest .chip').length).toBeGreaterThan(2)
    // ya si descarga; lo que hay que avisar ahora es que solo lleva musica
    expect(w.text()).toContain('Solo hablo de musica')
    expect(w.text()).not.toContain('No puedo descargar')
  })

  it('muestra el modelo que usa', async () => {
    const w = await montar()
    expect(w.find('.chat-model').text()).toContain('Qwen')
  })

  it('enviar pinta tu mensaje y la respuesta', async () => {
    const w = await montar()
    await escribir(w, '¿cuantas canciones tengo?')
    expect(w.findAll('.chat-msg.me')).toHaveLength(1)
    expect(w.findAll('.chat-msg.ai')).toHaveLength(1)
    expect(w.text()).toContain('Tienes 260 canciones')
  })

  it('manda todo el historial, no solo el ultimo', async () => {
    const w = await montar()
    await escribir(w, 'hola')
    await escribir(w, 'y ahora?')
    const ultima = api.chat.mock.calls.at(-1)[0]
    expect(ultima).toHaveLength(3)          // yo, ia, yo
    expect(ultima[0]).toEqual({ role: 'me', text: 'hola' })
  })

  it('el historial lleva que herramientas uso cada respuesta y que devolvieron', async () => {
    // Con eso el nucleo marca lo que se hizo de verdad en cada mensaje, el
    // modelo no toma sus propias frases («ya la cree») por hechos, y conserva
    // los ids y nombres que enseño («la segunda», «esa»).
    api.chat.mockResolvedValueOnce({
      text: 'Lista creada', tools: [{ name: 'create_playlist', summary: '4 temas', args: { x: 1 },
                                      detail: '«X» (id 3): id 1 «A - B»' }] })
    const w = await montar()
    await escribir(w, 'crea una lista')
    await escribir(w, 'gracias')
    const ultima = api.chat.mock.calls.at(-1)[0]
    expect(ultima[1]).toEqual({ role: 'ai', text: 'Lista creada',
                                tools: [{ name: 'create_playlist', summary: '4 temas', detail: '«X» (id 3): id 1 «A - B»' }] })
    expect(ultima[0]).toEqual({ role: 'me', text: 'crea una lista' })
  })

  it('enseña que herramientas uso', async () => {
    api.chat.mockResolvedValueOnce({
      text: 'Lista creada', tools: [{ name: 'create_playlist', summary: '4 temas' }] })
    const w = await montar()
    await escribir(w, 'armame una lista')
    expect(w.find('.chat-tools').text()).toContain('creo una lista')
    expect(w.find('.chat-tools').text()).toContain('4 temas')
  })

  it('avisa a la app cuando la IA crea una lista', async () => {
    api.chat.mockResolvedValueOnce({
      text: 'ok', tools: [{ name: 'create_playlist', summary: 'x' }] })
    const w = await montar()
    await escribir(w, 'crea una lista')
    expect(w.emitted('reload')).toBeTruthy()
  })

  it('un error del backend se muestra sin romper el chat', async () => {
    api.chat.mockResolvedValueOnce({ error: 'la IA no esta configurada' })
    const w = await montar()
    await escribir(w, 'hola')
    expect(w.find('.chat-msg.error').text()).toContain('no esta configurada')
  })

  it('no envia mensajes vacios', async () => {
    const w = await montar()
    await w.find('.chat-foot input').setValue('   ')
    await w.find('.chat-foot .btn').trigger('click')
    await flushPromises()
    expect(api.chat).not.toHaveBeenCalled()
  })

  it('recuerda la conversacion al reabrir', async () => {
    const w = await montar()
    await escribir(w, 'hola')
    const w2 = await montar()
    expect(w2.text()).toContain('hola')
  })

  it('pulsar una sugerencia la envia', async () => {
    const w = await montar()
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
describe('ordenes del asistente hacia la app', () => {
  it('reenvia las acciones que llegan del nucleo', async () => {
    api.chat.mockResolvedValueOnce({
      text: 'Ya suena', tools: [{ name: 'play_song', summary: 'sonando: Barak - Mi Gozo' }],
      actions: [{ kind: 'play_song', song_id: 7 }]
    })
    const w = await montar()
    await escribir(w, 'pon Mi Gozo')
    expect(w.emitted('action')).toBeTruthy()
    expect(w.emitted('action')[0][0]).toEqual({ kind: 'play_song', song_id: 7 })
  })

  it('sin acciones no emite nada', async () => {
    api.chat.mockResolvedValueOnce({ text: 'Tienes 260 canciones.', tools: [] })
    const w = await montar()
    await escribir(w, '¿cuantas tengo?')
    expect(w.emitted('action')).toBeFalsy()
  })

  it('recarga la biblioteca cuando el asistente la cambia', async () => {
    api.chat.mockResolvedValueOnce({
      text: 'Puntuada', tools: [{ name: 'set_stars', summary: '4 estrellas' }] })
    const w = await montar()
    await escribir(w, 'ponle 4 estrellas')
    expect(w.emitted('reload')).toBeTruthy()
  })
})


// El asistente contesta en markdown. Antes se pintaba tal cual, con los
// asteriscos y las almohadillas a la vista.
describe('el chat pinta el markdown del asistente', () => {
  it('negritas y listas salen como HTML, no como asteriscos', async () => {
    api.chat.mockResolvedValueOnce({
      text: 'Tienes dos:\n\n1. **Mi Gozo** – Barak\n2. **Shekinah** – New Wine', tools: [] })
    const w = await montar()
    await escribir(w, 'que tengo de barak?')
    const bubble = w.find('.chat-msg.ai .chat-bubble')
    expect(bubble.classes()).toContain('chat-md')
    expect(bubble.findAll('li')).toHaveLength(2)
    expect(bubble.find('strong').text()).toBe('Mi Gozo')
    expect(bubble.text()).not.toContain('**')
  })

  it('lo que escribes tu va tal cual, sin interpretar', async () => {
    const w = await montar()
    await escribir(w, 'busca **esto** literal')
    const mine = w.find('.chat-msg.me .chat-bubble')
    expect(mine.classes()).not.toContain('chat-md')
    expect(mine.text()).toContain('**esto**')
  })

  it('un titulo de YouTube con HTML dentro no se cuela', async () => {
    api.chat.mockResolvedValueOnce({
      text: 'Encontre <img src=x onerror=alert(1)> y **esto**', tools: [] })
    const w = await montar()
    await escribir(w, 'busca')
    const bubble = w.find('.chat-msg.ai .chat-bubble')
    expect(bubble.find('img').exists()).toBe(false)
    expect(bubble.text()).toContain('<img src=x onerror=alert(1)>')
    expect(bubble.find('strong').exists()).toBe(true)
  })

  it('los errores del nucleo se ven como texto plano', async () => {
    api.chat.mockResolvedValueOnce({ error: 'la IA **no** esta configurada' })
    const w = await montar()
    await escribir(w, 'hola')
    expect(w.find('.chat-msg.error .chat-bubble').text()).toContain('**no**')
  })
})

// Descargar no se hace dentro de la conversacion: el nucleo devuelve
// `confirm`, la app pregunta, y si aceptas arranca en segundo plano. El chat
// la sigue y, al terminar, cuenta que entro y que no. Antes se decia «te lo
// cuento en Descargas» y aqui no volvia a saberse nada; y el «bajala igual»
// (force) se perdia por el camino.
describe('descargas pedidas al asistente', () => {
  const pendiente = {
    text: 'Te pido permiso.', tools: [{ name: 'download_music', summary: 'espera tu visto bueno' }],
    confirm: { tool: 'download_music', summary: 'Descargar de YouTube: «Ruja o Leão».',
               args: { items: ['Ruja o Leao Carol Braga'], force: true } }
  }

  it('al aceptar manda al nucleo los argumentos tal cual, force incluido', async () => {
    api.chat.mockResolvedValueOnce(pendiente)
    api.chatConfirm.mockResolvedValueOnce({
      ok: true, text: 'Descargando. Te cuento cuando termine.',
      result: { active: true, items: ['Ruja o Leao Carol Braga'], force: true } })
    const w = await montar()
    await escribir(w, 'bajala igual')
    expect(useDialog().dialog.value.open).toBe(true)
    expect(useDialog().dialog.value.message).toContain('Ruja o Leão')
    dialogOk()
    await flushPromises()
    expect(api.chatConfirm).toHaveBeenCalledWith('download_music',
      { items: ['Ruja o Leao Carol Braga'], force: true })
    expect(w.text()).toContain('Descargando. Te cuento cuando termine.')
    // no hace falta recargar la biblioteca todavia: no ha entrado nada
    expect(w.emitted('reload')).toBeFalsy()
  })

  it('si dices que no, no se llama a nada', async () => {
    api.chat.mockResolvedValueOnce(pendiente)
    const w = await montar()
    await escribir(w, 'baja eso')
    dialogCancel()
    await flushPromises()
    expect(api.chatConfirm).not.toHaveBeenCalled()
    expect(w.text()).toContain('Cancelado, no he tocado nada.')
  })

  it('sigue la descarga y al acabar cuenta que entro y que ya tenias', async () => {
    vi.useFakeTimers()
    api.chat.mockResolvedValueOnce(pendiente)
    api.chatConfirm.mockResolvedValueOnce({
      ok: true, text: 'Descargando 2 temas.',
      result: { active: true, items: ['a', 'b'], force: false } })
    api.youtube
      .mockResolvedValueOnce({ active: true, phase: 'downloading', name: 'I Want Jesus', percent: 40, index: 1, total: 2 })
      .mockResolvedValueOnce({ active: false, phase: 'done', results: [
        { ok: true, id: 267, artist: 'Bethel Music', song: 'I Want Jesus (Live)', title: 'I Want Jesus' },
        { ok: false, already_there: true, title: 'Ruja o Leão - Carol Braga',
          matches: [{ id: 3, artist: 'Carol Braga', title: 'Ruja O Leao' }] }
      ] })
    const w = await montar()
    await escribir(w, 'baja estas dos')
    dialogOk()
    await flushPromises()

    // la primera mirada es inmediata; las siguientes, cada 700 ms
    await vi.advanceTimersByTimeAsync(300)
    expect(w.find('.chat-downloading').exists()).toBe(true)
    expect(w.find('.chat-downloading').text()).toContain('Bajando')
    expect(w.find('.chat-downloading').text()).toContain('1/2')

    api.chat.mockResolvedValueOnce({
      text: 'Listo: lista «Herlin» creada con la que entro.',
      tools: [{ name: 'create_playlist', summary: 'lista «Herlin» con 1 temas' }] })
    await vi.advanceTimersByTimeAsync(1000)
    expect(w.find('.chat-downloading').exists()).toBe(false)
    const report = w.findAll('.chat-msg.ai').find(m => m.text().includes('Descargada'))
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
    expect(w.emitted('reload')).toBeTruthy()
    // y ya no sigue preguntando
    const llamadas = api.youtube.mock.calls.length
    await vi.advanceTimersByTimeAsync(3000)
    expect(api.youtube.mock.calls.length).toBe(llamadas)

    // Al entrar algo, se le pasa el turno al asistente para que remate lo que
    // quedara («y armame una lista»). El aviso va al nucleo como un mensaje
    // del usuario, pero no se pinta como si lo hubieras escrito tu.
    const sent = api.chat.mock.calls.at(-1)[0]
    expect(sent.at(-1).role).toBe('me')
    expect(sent.at(-1).text).toContain('La descarga ha terminado')
    // con los ids exactos de lo que entro y la correspondencia con lo pedido:
    // sin ellos el modelo se inventaba ids y creia que habia entrado otra cancion
    expect(sent.at(-1).text).toContain('pediste «I Want Jesus» → entro como «Bethel Music - I Want Jesus (Live)» (id 267)')
    expect(sent.at(-1).text).toContain('NO la vuelvas a descargar')
    expect(sent.at(-1).text).toContain('añadir a una lista no necesita confirmacion')
    // y cita lo que pediste, para que remate ESO y no lo que le parezca
    expect(sent.at(-1).text).toContain('Lo que te pedi fue: «baja estas dos»')
    // y el historial que recibe el nucleo lleva las herramientas de cada mensaje
    const withTools = sent.find(m => m.tools?.some(t => t.name === 'download_music'))
    expect(withTools).toBeTruthy()
    const bubbles = w.findAll('.chat-msg.me')
    expect(bubbles.filter(b => b.isVisible()).some(b => b.text().includes('La descarga ha terminado'))).toBe(false)
    expect(w.text()).toContain('Listo: lista «Herlin» creada')
  })

  it('si cambias de pagina y vuelves, retoma el seguimiento', async () => {
    vi.useFakeTimers()
    localStorage.setItem('danplay.chat.download', JSON.stringify({ items: ['x'], force: false }))
    api.youtube.mockResolvedValueOnce({ active: false, phase: 'done', results: [
      { ok: true, artist: 'Barak', song: 'Mi Gozo' }] })
    const w = await montar()
    await vi.advanceTimersByTimeAsync(1000)
    expect(w.findAll('.chat-msg.ai').some(m => m.text().includes('Barak - Mi Gozo'))).toBe(true)
    expect(localStorage.getItem('danplay.chat.download')).toBeNull()
  })

  it('el aviso al asistente va etiquetado y con los ids; si el chat estaba ocupado, espera su turno', async () => {
    vi.useFakeTimers()
    localStorage.setItem('danplay.chat.download', JSON.stringify({ items: ['x'], force: false }))
    api.youtube.mockResolvedValueOnce({ active: false, phase: 'done', results: [
      { ok: true, id: 301, artist: 'Barak', song: 'Mi Gozo', requested: 'x' }] })
    // el chat esta ocupado con otra pregunta cuando termina la descarga
    let release
    api.chat.mockImplementationOnce(() => new Promise(res => { release = res }))
    const w = await montar()
    await escribir(w, '¿de que año es Kind of Blue?')      // se queda pensando
    await vi.advanceTimersByTimeAsync(1000)                 // termina la descarga
    expect(w.text()).toContain('Barak - Mi Gozo')
    expect(api.chat).toHaveBeenCalledTimes(1)                // el aviso NO se perdio ni se colo
    api.chat.mockResolvedValueOnce({ text: 'Terminado.', tools: [] })
    release({ text: 'De 1959.', tools: [] })
    await flushPromises(); await flushPromises(); await flushPromises()
    expect(api.chat).toHaveBeenCalledTimes(2)
    const sent = api.chat.mock.calls.at(-1)[0]
    expect(sent.at(-1).event).toBe('download_done')
    expect(sent.at(-1).text).toContain('«Barak - Mi Gozo» (id 301)')
    expect(sent.at(-1).text).toContain('responde solo: Terminado')
  })

  it('una descarga que no pidio el chat no se cuenta como suya', async () => {
    vi.useFakeTimers()
    localStorage.setItem('danplay.chat.download', JSON.stringify({ items: ['mi gozo barak'], force: false }))
    api.youtube.mockResolvedValueOnce({ active: false, phase: 'done', results: [
      { ok: true, id: 9, artist: 'Otro', song: 'Otra', requested: 'https://youtu.be/otra' }] })
    const w = await montar()
    await vi.advanceTimersByTimeAsync(1000)
    expect(w.text()).not.toContain('Otro - Otra')
    expect(api.chat).not.toHaveBeenCalled()
    expect(localStorage.getItem('danplay.chat.download')).toBeNull()
  })

  it('lo que escribe la app va marcado y la descarga aceptada lleva su herramienta', async () => {
    api.chat.mockResolvedValueOnce(pendiente)
    api.chatConfirm.mockResolvedValueOnce({ ok: true, text: 'Descargando.',
      result: { active: true, items: ['a'], force: false } })
    const w = await montar()
    await escribir(w, 'baja')
    dialogOk()
    await flushPromises()
    await escribir(w, 'gracias')
    const sent = api.chat.mock.calls.at(-1)[0]
    const accepted = sent.find(m => m.text === 'Descargando.')
    expect(accepted.app).toBe(true)
    expect(accepted.tools).toEqual([{ name: 'download_music', summary: 'aceptada, en marcha' }])
  })

  it('una respuesta narrada se pinta señalada', async () => {
    api.chat.mockResolvedValueOnce({ text: 'Ya la creé.\n\n_(Nota de la app: …)_', tools: [], narrated: true })
    const w = await montar()
    await escribir(w, 'crea la lista')
    const msg = w.findAll('.chat-msg.ai').at(-1)
    expect(msg.classes()).toContain('narrated')
    expect(msg.attributes('title')).toContain('ninguna herramienta')
  })

  it('si no entro nada, no se molesta al asistente', async () => {
    vi.useFakeTimers()
    localStorage.setItem('danplay.chat.download', JSON.stringify({ items: ['x'], force: false }))
    api.youtube.mockResolvedValueOnce({ active: false, phase: 'done', results: [
      { ok: false, already_there: true, title: 'Mi Gozo', matches: [] }] })
    const w = await montar()
    await vi.advanceTimersByTimeAsync(1000)
    expect(w.text()).toContain('Ya la tenías')
    expect(api.chat).not.toHaveBeenCalled()
  })

  it('un fallo al confirmar se enseña sin romper el chat', async () => {
    api.chat.mockResolvedValueOnce(pendiente)
    api.chatConfirm.mockRejectedValueOnce(new Error('409: ya hay una descarga en marcha'))
    const w = await montar()
    await escribir(w, 'baja')
    dialogOk()
    await flushPromises()
    expect(w.find('.chat-msg.error').text()).toContain('ya hay una descarga en marcha')
  })
})
