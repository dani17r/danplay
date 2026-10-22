import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'

// El selector de proveedor de IA: la rejilla de proveedores, el formulario
// con solo lo que cada uno pide, la lista de modelos con lo que se sabe de
// cada uno, y probar/guardar sin que la clave vuelva entera del nucleo.
const held = vi.hoisted(() => ({ state: null, api: null }))

vi.mock('../src/api.js', async (importOriginal) => {
  const actual = await importOriginal()
  const { createState, buildApiDouble } = await import('./support/backend.js')
  held.state = createState()
  held.api = buildApiDouble(actual, held.state)
  const { vi: v } = await import('vitest')
  const app = { ...actual.app, openInBrowser: v.fn(async () => {}) }
  return { ...actual, api: held.api, app, inTauri: true }
})

import AiProviderModal from '../src/components/AiProviderModal.vue'
import ModelPicker from '../src/components/ui/ModelPicker.vue'
import SettingsPage from '../src/components/SettingsPage.vue'
import { dialogOk, dialogCancel, useDialog } from '../src/composables/useDialog.js'

beforeEach(() => {
  vi.clearAllMocks()
  held.state.aiProfiles = {}
  held.state.aiActive = ''
})

const open = async (initial = '') => {
  const w = mount(AiProviderModal, { props: { open: true, initial }, attachTo: document.body })
  await flushPromises()
  await flushPromises()
  return w
}
const card = (w, id) => w.findAll('.ai-card').find((c) => c.text().includes(id))

describe('choosing a provider', () => {
  it('shows providers in groups and filters while typing', async () => {
    const w = await open()
    expect(w.text()).toContain('Grandes laboratorios')
    expect(w.text()).toContain('En tu equipo')
    expect(w.findAll('.ai-card').length).toBeGreaterThanOrEqual(4)
    await w.find('input').setValue('olla')
    const names = w.findAll('.ai-card-name').map((c) => c.text())
    expect(names).toEqual(['Ollama'])
    w.unmount()
  })

  it('marks the ones already configured and the one in use', async () => {
    held.state.aiProfiles = {
      openai: {
        id: 'openai',
        provider: 'openai',
        provider_name: 'OpenAI',
        has_key: true,
        key: 'sk-a…',
        model: 'chico',
        chat_model: 'grande',
        fields: {},
        headers: {},
        extra: {}
      }
    }
    held.state.aiActive = 'openai'
    const w = await open()
    const c = card(w, 'OpenAI')
    expect(c.classes()).toContain('configured')
    expect(c.classes()).toContain('active')
    expect(c.text()).toContain('en uso')
    expect(card(w, 'Ollama').classes()).not.toContain('configured')
    w.unmount()
  })

  it('a paid provider asks for the key and links to where to get it', async () => {
    const w = await open()
    await card(w, 'OpenAI').trigger('click')
    await flushPromises()
    expect(w.text()).toContain('Clave de API')
    expect(w.text()).toContain('Consigue tu clave')
    // sin clave no se puede ni probar ni guardar, y se dice por que
    expect(w.text()).toContain('falta la clave')
    const save = w.findAll('button').find((b) => b.text() === 'Guardar y usar')
    expect(save.attributes('disabled')).toBeDefined()
    // el catalogo se pide solo, sin molestar al proveedor (no hay clave)
    expect(held.api.aiModels).toHaveBeenCalledTimes(1)
    expect(held.api.aiModels.mock.calls[0][0].key).toBeUndefined()
    w.unmount()
  })

  it('a local one asks for no key, lets you change the URL and fetches the model list on open', async () => {
    const w = await open()
    await card(w, 'Ollama').trigger('click')
    await flushPromises()
    expect(w.text()).not.toContain('Clave de API')
    expect(w.text()).toContain('URL de la API')
    expect(held.api.aiModels).toHaveBeenCalledTimes(1)
    // los modelos sugeridos por el nucleo rellenan lo vacio
    const boxes = w.findAllComponents(ModelPicker)
    expect(boxes).toHaveLength(2)
    expect(boxes[0].props('modelValue')).toBe('grande')
    expect(boxes[1].props('modelValue')).toBe('chico')
    w.unmount()
  })

  it('the URL placeholders are filled in with what was typed', async () => {
    const w = await open()
    await card(w, 'Amazon Bedrock').trigger('click')
    await flushPromises()
    expect(w.text()).toContain('falta region')
    const region = w.findAll('input').find((i) => i.attributes('placeholder') === 'us-east-1')
    await region.setValue('eu-west-1')
    expect(w.find('.ai-url').text()).toContain('bedrock-runtime.eu-west-1.amazonaws.com')
    w.unmount()
  })

  it('testing sends the form contents without saving, and saving activates it', async () => {
    const w = await open()
    await card(w, 'OpenAI').trigger('click')
    await flushPromises()
    const key = w.findAll('input').find((i) => i.attributes('type') === 'password')
    await key.setValue('sk-nueva-clave')
    await w
      .findAll('button')
      .find((b) => b.text() === 'Probar')
      .trigger('click')
    await flushPromises()
    expect(held.api.aiCheck).toHaveBeenCalledTimes(1)
    const sent = held.api.aiCheck.mock.calls[0][0]
    expect(sent.provider).toBe('openai')
    expect(sent.key).toBe('sk-nueva-clave')
    expect(sent.base_url).toBeUndefined() // la del catalogo no se repite
    expect(held.api.aiSaveProfile).not.toHaveBeenCalled()
    expect(w.find('.key-state.ok').text()).toContain('Funciona')

    await w
      .findAll('button')
      .find((b) => b.text() === 'Guardar y usar')
      .trigger('click')
    await flushPromises()
    expect(held.api.aiSaveProfile).toHaveBeenCalledTimes(1)
    expect(held.api.aiSaveProfile.mock.calls[0][0]).toMatchObject({
      provider: 'openai',
      key: 'sk-nueva-clave',
      activate: true
    })
    expect(w.emitted('saved')).toBeTruthy()
    expect(w.emitted('close')).toBeTruthy()
    expect(held.state.aiActive).toBe('openai')
    w.unmount()
  })

  it('with a saved key the field starts empty and the key is kept unless another is typed', async () => {
    held.state.aiProfiles = {
      openai: {
        id: 'openai',
        provider: 'openai',
        provider_name: 'OpenAI',
        has_key: true,
        key: 'sk-a…',
        model: 'chico',
        chat_model: 'grande',
        fields: {},
        headers: {},
        extra: {}
      }
    }
    held.state.aiActive = 'openai'
    const w = await open('openai')
    expect(w.text()).toContain('OpenAI')
    const key = w.findAll('input').find((i) => i.attributes('type') === 'password')
    expect(key.element.value).toBe('')
    expect(key.attributes('placeholder')).toContain('guardada: sk-a…')
    await w
      .findAll('button')
      .find((b) => b.text() === 'Guardar y usar')
      .trigger('click')
    await flushPromises()
    expect(held.api.aiSaveProfile.mock.calls[0][0].key).toBeUndefined()
    w.unmount()
  })

  it('a custom server asks for a name and an http(s) URL', async () => {
    const w = await open()
    await card(w, 'Añadir otro servidor').trigger('click')
    await flushPromises()
    expect(w.text()).toContain('falta la URL')
    const url = w
      .findAll('input')
      .find((i) => i.attributes('placeholder') === 'https://mi-servidor/v1')
    await url.setValue('http://casa:8080/v1')
    await w
      .findAll('input')
      .find((i) => i.attributes('placeholder') === 'Servidor de la iglesia')
      .setValue('Casa')
    expect(w.text()).not.toContain('falta la URL')
    await w
      .findAll('button')
      .find((b) => b.text() === 'Guardar y usar')
      .trigger('click')
    await flushPromises()
    expect(held.api.aiSaveProfile.mock.calls[0][0]).toMatchObject({
      provider: 'custom',
      name: 'Casa',
      base_url: 'http://casa:8080/v1'
    })
    w.unmount()
  })

  it('extra parameters must be JSON', async () => {
    const w = await open()
    await card(w, 'Ollama').trigger('click')
    await flushPromises()
    await w.find('.ai-advanced-toggle').trigger('click')
    const extra = w.find('textarea[placeholder^="{"]')
    await extra.setValue('esto no es json')
    await w
      .findAll('button')
      .find((b) => b.text() === 'Probar')
      .trigger('click')
    await flushPromises()
    expect(held.api.aiCheck).not.toHaveBeenCalled()
    expect(w.text()).toContain('JSON')
    await extra.setValue('{"reasoning_effort": "low"}')
    await w
      .findAll('button')
      .find((b) => b.text() === 'Probar')
      .trigger('click')
    await flushPromises()
    expect(held.api.aiCheck.mock.calls[0][0].extra).toEqual({ reasoning_effort: 'low' })
    w.unmount()
  })

  it('removing a provider asks first', async () => {
    held.state.aiProfiles = {
      openai: {
        id: 'openai',
        provider: 'openai',
        provider_name: 'OpenAI',
        has_key: true,
        key: 'sk-a…',
        model: 'chico',
        chat_model: 'grande',
        fields: {},
        headers: {},
        extra: {}
      }
    }
    held.state.aiActive = 'openai'
    const w = await open('openai')
    await w
      .findAll('button')
      .find((b) => b.text() === 'Quitar')
      .trigger('click')
    await flushPromises()
    expect(held.api.aiDeleteProfile).not.toHaveBeenCalled()
    dialogOk()
    await flushPromises()
    expect(held.api.aiDeleteProfile).toHaveBeenCalledWith('openai')
    expect(held.state.aiProfiles.openai).toBeUndefined()
    w.unmount()
  })
})

describe('the model picker', () => {
  const models = [
    {
      id: 'grande',
      name: 'Grande',
      tools: true,
      cost_in: 1,
      cost_out: 5,
      context: 128000,
      released: '2026-09-01',
      deprecated: false,
      known: true
    },
    {
      id: 'chico',
      name: 'Chico',
      tools: true,
      cost_in: 0.1,
      cost_out: 0.4,
      context: 32000,
      released: '2026-08-01',
      deprecated: false,
      known: true
    },
    {
      id: 'mudo',
      name: 'Mudo',
      tools: false,
      cost_in: 0.1,
      cost_out: 0.1,
      context: 8000,
      released: '2026-07-01',
      deprecated: false,
      known: true
    },
    {
      id: 'viejo',
      name: 'Viejo',
      tools: true,
      cost_in: 1,
      cost_out: 1,
      context: 8000,
      released: '2024-01-01',
      deprecated: true,
      known: true
    }
  ]
  const mountView = (props = {}) =>
    mount(ModelPicker, {
      props: { modelValue: '', models: models, ...props },
      attachTo: document.body
    })

  it('shows price, context, tools and the recommended one', async () => {
    const w = mountView({ suggest: 'chico' })
    await w.find('input').trigger('focus')
    const rows = w.findAll('.model-row')
    expect(rows[0].text()).toContain('Chico')
    expect(rows[0].text()).toContain('recomendado')
    expect(rows[0].text()).toContain('$0.1 → $0.4')
    expect(rows[0].text()).toContain('32k ctx')
    expect(rows.find((f) => f.text().includes('Mudo')).text()).toContain('sin herramientas')
    // los obsoletos van escondidos de entrada
    expect(rows.some((f) => f.text().includes('Viejo'))).toBe(false)
    w.unmount()
  })

  it('for chatting, it filters out from the start the models without tools', async () => {
    const w = mountView({ needTools: true })
    await w.find('input').trigger('focus')
    const names = w.findAll('.model-name').map((f) => f.text())
    expect(names).toContain('Grande')
    expect(names).not.toContain('Mudo')
    w.unmount()
  })

  it('typed text filters and is accepted even if it is not in the list', async () => {
    const w = mountView()
    await w.find('input').setValue('gra')
    expect(w.findAll('.model-name').map((f) => f.text())).toEqual(['Grande'])
    expect(w.emitted('update:modelValue').at(-1)).toEqual(['gra'])
    await w.find('input').setValue('mi-modelo-raro')
    expect(w.text()).toContain('nada coincide')
    expect(w.emitted('update:modelValue').at(-1)).toEqual(['mi-modelo-raro'])
    w.unmount()
  })

  it('picking a row emits its id and warns if it does not use tools', async () => {
    const w = mountView()
    await w.find('input').trigger('focus')
    await w
      .findAll('.model-row')
      .find((f) => f.text().includes('Mudo'))
      .trigger('click')
    expect(w.emitted('update:modelValue').at(-1)).toEqual(['mudo'])
    await w.setProps({ modelValue: 'mudo' })
    expect(w.find('.model-warn').text()).toBe('sin herramientas')
    w.unmount()
  })
})

describe('the AI card in Settings', () => {
  it('with no provider it says so and offers to pick one', async () => {
    const w = mount(SettingsPage, { attachTo: document.body })
    await flushPromises()
    await flushPromises()
    expect(w.text()).toContain('Sin proveedor elegido')
    const button = w.findAll('button').find((b) => b.text() === 'Elegir proveedor…')
    expect(button).toBeTruthy()
    await button.trigger('click')
    await flushPromises()
    expect(w.find('.ai-modal').exists()).toBe(true)
    expect(w.text()).toContain('¿Qué IA quieres usar?')
    w.unmount()
  })

  it('with several configured, one click switches between them', async () => {
    held.state.aiProfiles = {
      openai: {
        id: 'openai',
        provider: 'openai',
        provider_name: 'OpenAI',
        has_key: true,
        key: 'sk-a…',
        model: 'chico',
        chat_model: 'grande',
        fields: {},
        headers: {},
        extra: {}
      },
      ollama: {
        id: 'ollama',
        provider: 'ollama',
        provider_name: 'Ollama',
        has_key: false,
        key: '',
        model: 'qwen3:8b',
        chat_model: 'qwen3:8b',
        fields: {},
        headers: {},
        extra: {}
      }
    }
    held.state.aiActive = 'openai'
    const w = mount(SettingsPage, { attachTo: document.body })
    await flushPromises()
    await flushPromises()
    expect(w.find('.ai-current').text()).toContain('OpenAI')
    expect(w.find('.ai-current').text()).toContain('grande')
    const chips = w.findAll('.ai-profile')
    expect(chips).toHaveLength(2)
    await chips
      .find((c) => c.text().includes('Ollama'))
      .find('.ai-profile-pick')
      .trigger('click')
    await flushPromises()
    expect(held.api.aiActivate).toHaveBeenCalledWith('ollama')
    expect(w.find('.ai-current').text()).toContain('Ollama')
    w.unmount()
  })
})

describe('trying it for free, without a key', () => {
  it('from Settings it activates the first free one that responds and reports it', async () => {
    const w = mount(SettingsPage, { attachTo: document.body })
    await flushPromises()
    await flushPromises()
    const button = w.findAll('button').find((b) => b.text() === 'Probar gratis, sin clave')
    expect(button).toBeTruthy()
    await button.trigger('click')
    await flushPromises()
    await flushPromises()
    expect(held.api.aiFree).toHaveBeenCalledTimes(1)
    expect(held.state.aiActive).toBe('llm7')
    expect(w.find('.key-state.ok').text()).toContain('LLM7')
    expect(w.find('.key-state.ok').text()).toContain('terceros')
    expect(w.find('.ai-current').text()).toContain('LLM7')
    // ya hay proveedor: el boton de gratis deja paso a «Probar»
    expect(w.findAll('button').some((b) => b.text() === 'Probar gratis, sin clave')).toBe(false)
    w.unmount()
  })

  it('the modal offers it in the free group and closes once it succeeds', async () => {
    const w = await open()
    expect(w.text()).toContain('Gratis, sin clave')
    await w.find('.ai-free-btn').trigger('click')
    await flushPromises()
    expect(held.api.aiFree).toHaveBeenCalledTimes(1)
    expect(w.emitted('saved')).toBeTruthy()
    expect(w.emitted('close')).toBeTruthy()
    w.unmount()
  })
})

describe('fallback and spending in Settings', () => {
  it('shows the amount spent and lets you turn off the fallback', async () => {
    held.state.aiProfiles = {
      openai: {
        id: 'openai',
        provider: 'openai',
        provider_name: 'OpenAI',
        has_key: true,
        key: 'sk-a…',
        model: 'chico',
        chat_model: 'grande',
        fields: {},
        headers: {},
        extra: {}
      },
      llm7: {
        id: 'llm7',
        provider: 'llm7',
        provider_name: 'LLM7',
        has_key: false,
        key: '',
        model: 'x',
        chat_model: 'x',
        fields: {},
        headers: {},
        extra: {}
      }
    }
    held.state.aiActive = 'openai'
    const w = mount(SettingsPage, { attachTo: document.body })
    await flushPromises()
    await flushPromises()
    expect(w.find('.ai-usage').text()).toContain('Hoy 2 llamadas · 3,4 k tokens · $0,0021')
    expect(w.find('.ai-usage').text()).toContain('Este mes 20 llamadas')
    const toggle = w.findAll('.toggle').find((t) => t.text().includes('Si el proveedor falla'))
    expect(toggle.text()).toContain('LLM7')
    await toggle.find('.toggle-track').trigger('click')
    await flushPromises()
    expect(held.api.aiFallback).toHaveBeenCalledWith(false)
    w.unmount()
  })
})

describe('keyboard in the AI dialog', () => {
  it('Escape with a confirmation on top does not also close the dialog', async () => {
    // Las dos escuchan el teclado: Escape cerraba la confirmacion Y la
    // ventana de debajo, y se perdia lo que se estaba configurando.
    held.state.aiProfiles = {
      openai: {
        id: 'openai',
        provider: 'openai',
        provider_name: 'OpenAI',
        has_key: true,
        key: 'sk-a…',
        model: 'chico',
        chat_model: 'grande',
        fields: {},
        headers: {},
        extra: {}
      }
    }
    held.state.aiActive = 'openai'
    const w = await open('openai')
    expect(w.find('.ai-form').exists()).toBe(true)
    await w
      .findAll('button')
      .find((b) => b.text() === 'Quitar')
      .trigger('click')
    await flushPromises()
    expect(useDialog().dialog.value.open).toBe(true)
    document.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true })
    )
    await flushPromises()
    expect(w.emitted('close')).toBeFalsy()
    expect(w.find('.ai-form').exists(), 'volvio a la lista por debajo de la confirmacion').toBe(
      true
    )
    dialogCancel()
    await flushPromises()
    // sin nada encima, Escape vuelve a ser suyo: del formulario a la lista
    document.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true })
    )
    await flushPromises()
    expect(w.find('.ai-form').exists()).toBe(false)
    w.unmount()
  })
})
