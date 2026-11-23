import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'

// Las opciones de la IA dentro del asistente: ver quién contesta y con qué
// modelo, cambiarlo, saltar entre proveedores guardados y pegar la clave sin
// salir a Ajustes. Lo que se manda al núcleo es lo mismo que manda Ajustes.
const held = vi.hoisted(() => ({ state: null, api: null }))

vi.mock('../src/api.js', async (importOriginal) => {
  const actual = await importOriginal()
  const { createState, buildApiDouble } = await import('./support/backend.js')
  const { vi: v } = await import('vitest')
  held.state = createState()
  held.api = buildApiDouble(actual, held.state)
  // quién contesta, según lo guardado (como el núcleo)
  held.api.chatTools = v.fn(async () => {
    const p = held.state.aiProfiles[held.state.aiActive]
    return p
      ? { provider: p.provider_name, model: p.chat_model, available: true, tools: [] }
      : {
          provider: '',
          model: '',
          available: false,
          reason: 'no hay ningun proveedor de IA elegido',
          tools: []
        }
  })
  const app = { ...actual.app, openInBrowser: v.fn(async () => {}) }
  return { ...actual, api: held.api, app, inTauri: true }
})

import ChatPage from '../src/components/ChatPage.vue'
import { resetDownloads } from '../src/composables/useDownloads.js'
import { resetChat } from '../src/composables/useChat.js'
import { clearNotices, useNotices } from '../src/composables/useNotices.js'

const OPENAI = {
  id: 'openai',
  provider: 'openai',
  provider_name: 'OpenAI',
  has_key: true,
  key: 'sk-a…f3a1',
  model: 'chico',
  chat_model: 'grande',
  base_url: '',
  fields: {},
  headers: {},
  extra: {},
  timeout: 60
}
const OLLAMA = {
  id: 'ollama',
  provider: 'ollama',
  provider_name: 'Ollama',
  has_key: false,
  key: '',
  model: 'qwen3:8b',
  chat_model: 'qwen3:8b',
  base_url: 'http://localhost:11500/v1',
  fields: {},
  headers: {},
  extra: {},
  timeout: 60
}

beforeEach(() => {
  vi.clearAllMocks()
  resetDownloads()
  resetChat()
  clearNotices()
  localStorage.clear()
  held.state.aiProfiles = { openai: { ...OPENAI } }
  held.state.aiActive = 'openai'
})

const mountView = async () => {
  const w = mount(ChatPage, { attachTo: document.body })
  await flushPromises()
  await flushPromises()
  return w
}
const optionsButton = (w) => w.find('.chat-head button[aria-label="Opciones de la IA"]')
const openPanel = async (w) => {
  await optionsButton(w).trigger('click')
  await flushPromises()
  await flushPromises()
  return w.find('.chat-ai')
}
const button = (w, text) => w.findAll('button').find((b) => b.text().includes(text))
/** El selector de modelo de un papel: 0 = conversación, 1 = identificar. */
const picker = (w, n) => w.findAll('.chat-ai .model-picker')[n]

describe('the AI options in the chat', () => {
  it('the header has an options button that opens and closes the panel', async () => {
    const w = await mountView()
    const b = optionsButton(w)
    expect(b.exists()).toBe(true)
    expect(b.attributes('title')).toContain('modelo y clave')
    expect(b.attributes('aria-expanded')).toBe('false')
    expect(w.find('.chat-ai').exists()).toBe(false)
    await openPanel(w)
    expect(w.find('.chat-ai').exists()).toBe(true)
    expect(optionsButton(w).attributes('aria-expanded')).toBe('true')
    await optionsButton(w).trigger('click')
    expect(w.find('.chat-ai').exists()).toBe(false)
    w.unmount()
  })

  it('shows the provider and both models in use, and the key only masked', async () => {
    const w = await mountView()
    const panel = await openPanel(w)
    expect(panel.text()).toContain('OpenAI')
    expect(panel.text()).toContain('lista')
    expect(panel.text()).toContain('clave guardada: sk-a…f3a1')
    expect(picker(w, 0).find('input').element.value).toBe('grande')
    expect(picker(w, 1).find('input').element.value).toBe('chico')
    // la lista de modelos se pide con lo guardado y SIN clave: la pone el núcleo
    expect(held.api.aiModels).toHaveBeenCalledTimes(1)
    const sent = held.api.aiModels.mock.calls[0][0]
    expect(sent).toMatchObject({ id: 'openai', provider: 'openai' })
    expect(sent.key).toBeUndefined()
    w.unmount()
  })

  it('a provider on another URL asks for its models with that URL', async () => {
    held.state.aiProfiles = { ollama: { ...OLLAMA } }
    held.state.aiActive = 'ollama'
    const w = await mountView()
    await openPanel(w)
    expect(held.api.aiModels.mock.calls[0][0].base_url).toBe('http://localhost:11500/v1')
    w.unmount()
  })

  it('picking another conversation model saves only that, like Settings, and the header follows', async () => {
    const w = await mountView()
    expect(w.find('.chat-model').text()).toContain('grande')
    await openPanel(w)
    await picker(w, 0).find('input').trigger('focus')
    const row = picker(w, 0)
      .findAll('.model-row')
      .find((r) => r.text().includes('Chico'))
    await row.trigger('click')
    await flushPromises()
    await flushPromises()
    expect(held.api.aiSaveProfile).toHaveBeenCalledTimes(1)
    expect(held.api.aiSaveProfile.mock.calls[0][0]).toEqual({
      id: 'openai',
      provider: 'openai',
      chat_model: 'chico',
      activate: true
    })
    // la clave sigue guardada y el rápido no cambió
    expect(held.state.aiProfiles.openai.has_key).toBe(true)
    expect(held.state.aiProfiles.openai.model).toBe('chico')
    expect(w.find('.chat-model').text()).toContain('chico')
    expect(
      useNotices()
        .notices.value.map((n) => n.message)
        .join(' ')
    ).toContain('conversa ahora con chico')
    w.unmount()
  })

  it('a typed model name is saved only with «Usar», not letter by letter', async () => {
    const w = await mountView()
    await openPanel(w)
    await picker(w, 1).find('input').setValue('mi-modelo')
    await flushPromises()
    expect(held.api.aiSaveProfile).not.toHaveBeenCalled()
    await w.findAll('.chat-ai-model')[1].find('.btn').trigger('click')
    await flushPromises()
    expect(held.api.aiSaveProfile.mock.calls[0][0]).toEqual({
      id: 'openai',
      provider: 'openai',
      model: 'mi-modelo',
      activate: true
    })
    w.unmount()
  })

  it('with several providers saved, one click switches and the header says so', async () => {
    held.state.aiProfiles = { openai: { ...OPENAI }, ollama: { ...OLLAMA } }
    const w = await mountView()
    await openPanel(w)
    const chip = w.findAll('.chat-ai .ai-profile').find((c) => c.text().includes('Ollama'))
    await chip.find('.ai-profile-pick').trigger('click')
    await flushPromises()
    await flushPromises()
    expect(held.api.aiActivate).toHaveBeenCalledWith('ollama')
    expect(w.find('.chat-model').text()).toContain('Ollama')
    expect(w.find('.chat-ai').text()).toContain('Ollama')
    w.unmount()
  })

  it('opens the provider dialog right there, to paste the key, and the header follows', async () => {
    const w = await mountView()
    await openPanel(w)
    await button(w, 'Clave y ajustes').trigger('click')
    await flushPromises()
    await flushPromises()
    // el panel se recoge y el modal abre ya en el formulario de ese proveedor
    expect(w.find('.chat-ai').exists()).toBe(false)
    expect(w.find('.ai-modal').exists()).toBe(true)
    expect(w.find('.ai-modal').text()).toContain('Clave de API')
    const key = w.find('.ai-modal input[type="password"]')
    expect(key.attributes('placeholder')).toContain('guardada: sk-a…f3a1')
    expect(key.element.value).toBe('')
    await key.setValue('sk-nueva-clave-larga')
    await button(w, 'Guardar y usar').trigger('click')
    await flushPromises()
    await flushPromises()
    expect(held.api.aiSaveProfile).toHaveBeenCalledTimes(1)
    expect(held.api.aiSaveProfile.mock.calls[0][0]).toMatchObject({
      id: 'openai',
      key: 'sk-nueva-clave-larga'
    })
    expect(w.find('.ai-modal').exists()).toBe(false)
    // quién contesta se vuelve a preguntar
    expect(held.api.chatTools.mock.calls.length).toBeGreaterThan(1)
    w.unmount()
  })

  it('Escape and a click outside close the panel', async () => {
    const w = await mountView()
    await openPanel(w)
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }))
    await flushPromises()
    expect(w.find('.chat-ai').exists()).toBe(false)
    await openPanel(w)
    // dentro no lo cierra
    w.find('.chat-ai').element.dispatchEvent(new MouseEvent('mousedown', { bubbles: true }))
    await flushPromises()
    expect(w.find('.chat-ai').exists()).toBe(true)
    w.find('.chat-thread').element.dispatchEvent(new MouseEvent('mousedown', { bubbles: true }))
    await flushPromises()
    expect(w.find('.chat-ai').exists()).toBe(false)
    w.unmount()
  })
})

describe('the chat with no AI configured', () => {
  beforeEach(() => {
    held.state.aiProfiles = {}
    held.state.aiActive = ''
  })

  it('offers to choose a provider and paste the key right there', async () => {
    const w = await mountView()
    expect(w.find('.chat-noai').exists()).toBe(true)
    expect(w.find('.chat-noai').text()).toContain('aquí mismo')
    await w.find('.chat-noai .chat-noai-setup').trigger('click')
    await flushPromises()
    await flushPromises()
    expect(w.find('.ai-modal').exists()).toBe(true)
    expect(w.find('.ai-modal').text()).toContain('¿Qué IA quieres usar?')
    // se elige uno y se pega la clave: el chat ya tiene con quién hablar
    await w
      .findAll('.ai-modal .ai-card')
      .find((c) => c.text().includes('OpenAI'))
      .trigger('click')
    await flushPromises()
    await w.find('.ai-modal input[type="password"]').setValue('sk-mi-clave-de-prueba')
    await button(w, 'Guardar y usar').trigger('click')
    await flushPromises()
    await flushPromises()
    expect(held.state.aiActive).toBe('openai')
    expect(w.find('.chat-noai').exists()).toBe(false)
    w.unmount()
  })

  it('the options panel says so and offers the free trial', async () => {
    const w = await mountView()
    const panel = await openPanel(w)
    expect(panel.text()).toContain('Sin proveedor elegido')
    expect(panel.find('.model-picker').exists()).toBe(false)
    await button(w, 'Probar gratis, sin clave').trigger('click')
    // el del hueco del chat y el del panel: aquí, el del panel
    await flushPromises()
    await flushPromises()
    expect(held.api.aiFree).toHaveBeenCalledTimes(1)
    expect(w.find('.chat-ai').text()).toContain('Listo: LLM7')
    expect(w.find('.chat-model').text()).toContain('LLM7')
    w.unmount()
  })

  it('with the AI turned off in Settings, it can be turned on from the chat', async () => {
    held.state.aiProfiles = { openai: { ...OPENAI } }
    held.state.aiActive = 'openai'
    const overview = held.api.aiProviders.getMockImplementation()
    let on = false
    held.api.aiProviders.mockImplementation(async () => ({
      ...(await overview()),
      ai_enabled: on,
      ai_ready: on
    }))
    const saveSettings = held.api.saveSettings.getMockImplementation()
    held.api.saveSettings.mockImplementation(async (d) => {
      on = d.ai_enabled
      return d
    })
    const chatTools = held.api.chatTools.getMockImplementation()
    held.api.chatTools.mockImplementation(async () =>
      on
        ? chatTools()
        : { model: '', available: false, reason: 'la IA esta apagada en Ajustes', tools: [] }
    )
    try {
      const w = await mountView()
      await w.find('.chat-noai .chat-noai-setup').trigger('click')
      await flushPromises()
      await flushPromises()
      // apagada, elegir proveedor no la arregla: se abre el panel, que la enciende
      expect(w.find('.ai-modal').exists()).toBe(false)
      await button(w, 'Encender la IA').trigger('click')
      await flushPromises()
      await flushPromises()
      expect(held.api.saveSettings).toHaveBeenCalledWith({ ai_enabled: true })
      expect(w.find('.chat-noai').exists()).toBe(false)
      w.unmount()
    } finally {
      held.api.aiProviders.mockImplementation(overview)
      held.api.chatTools.mockImplementation(chatTools)
      held.api.saveSettings.mockImplementation(saveSettings)
    }
  })
})
