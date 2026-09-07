import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'

const { api } = vi.hoisted(() => ({
  api: {
    chatTools: vi.fn(async () => ({ model: 'Qwen/Qwen3-Next-80B-A3B-Instruct',
                                           available: true, tools: [] })),
    chat: vi.fn(async () => ({ text: 'Tienes 260 canciones.', tools: [] }))
  }
}))
vi.mock('../src/api.js', () => ({ api, native: { available: false } }))
import ChatPage from '../src/components/ChatPage.vue'

beforeEach(() => { localStorage.clear(); vi.clearAllMocks() })

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
