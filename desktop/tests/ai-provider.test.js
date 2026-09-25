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
import { dialogOk } from '../src/composables/useDialog.js'

beforeEach(() => {
  vi.clearAllMocks()
  held.state.aiProfiles = {}
  held.state.aiActive = ''
})

const abrir = async (initial = '') => {
  const w = mount(AiProviderModal, { props: { open: true, initial }, attachTo: document.body })
  await flushPromises(); await flushPromises()
  return w
}
const tarjeta = (w, id) => w.findAll('.ai-card').find((c) => c.text().includes(id))

describe('elegir proveedor', () => {
  it('enseña los proveedores por grupos y filtra al escribir', async () => {
    const w = await abrir()
    expect(w.text()).toContain('Grandes laboratorios')
    expect(w.text()).toContain('En tu equipo')
    expect(w.findAll('.ai-card').length).toBeGreaterThanOrEqual(4)
    await w.find('input').setValue('olla')
    const nombres = w.findAll('.ai-card-name').map((c) => c.text())
    expect(nombres).toEqual(['Ollama'])
    w.unmount()
  })

  it('marca los ya configurados y cual esta en uso', async () => {
    held.state.aiProfiles = {
      openai: { id: 'openai', provider: 'openai', provider_name: 'OpenAI', has_key: true, key: 'sk-a…', model: 'chico', chat_model: 'grande', fields: {}, headers: {}, extra: {} }
    }
    held.state.aiActive = 'openai'
    const w = await abrir()
    const c = tarjeta(w, 'OpenAI')
    expect(c.classes()).toContain('configured')
    expect(c.classes()).toContain('active')
    expect(c.text()).toContain('en uso')
    expect(tarjeta(w, 'Ollama').classes()).not.toContain('configured')
    w.unmount()
  })

  it('un proveedor de pago pide la clave y enlaza a donde conseguirla', async () => {
    const w = await abrir()
    await tarjeta(w, 'OpenAI').trigger('click')
    await flushPromises()
    expect(w.text()).toContain('Clave de API')
    expect(w.text()).toContain('Consigue tu clave')
    // sin clave no se puede ni probar ni guardar, y se dice por que
    expect(w.text()).toContain('falta la clave')
    const guardar = w.findAll('button').find((b) => b.text() === 'Guardar y usar')
    expect(guardar.attributes('disabled')).toBeDefined()
    // el catalogo se pide solo, sin molestar al proveedor (no hay clave)
    expect(held.api.aiModels).toHaveBeenCalledTimes(1)
    expect(held.api.aiModels.mock.calls[0][0].key).toBeUndefined()
    w.unmount()
  })

  it('uno local no pide clave, deja cambiar la URL y pide la lista al entrar', async () => {
    const w = await abrir()
    await tarjeta(w, 'Ollama').trigger('click')
    await flushPromises()
    expect(w.text()).not.toContain('Clave de API')
    expect(w.text()).toContain('URL de la API')
    expect(held.api.aiModels).toHaveBeenCalledTimes(1)
    // los modelos sugeridos por el nucleo rellenan lo vacio
    const cajas = w.findAllComponents(ModelPicker)
    expect(cajas).toHaveLength(2)
    expect(cajas[0].props('modelValue')).toBe('grande')
    expect(cajas[1].props('modelValue')).toBe('chico')
    w.unmount()
  })

  it('los huecos de la URL se rellenan con lo escrito', async () => {
    const w = await abrir()
    await tarjeta(w, 'Amazon Bedrock').trigger('click')
    await flushPromises()
    expect(w.text()).toContain('falta region')
    const region = w.findAll('input').find((i) => i.attributes('placeholder') === 'us-east-1')
    await region.setValue('eu-west-1')
    expect(w.find('.ai-url').text()).toContain('bedrock-runtime.eu-west-1.amazonaws.com')
    w.unmount()
  })

  it('probar manda lo del formulario sin guardar, y guardar lo activa', async () => {
    const w = await abrir()
    await tarjeta(w, 'OpenAI').trigger('click')
    await flushPromises()
    const clave = w.findAll('input').find((i) => i.attributes('type') === 'password')
    await clave.setValue('sk-nueva-clave')
    await w.findAll('button').find((b) => b.text() === 'Probar').trigger('click')
    await flushPromises()
    expect(held.api.aiCheck).toHaveBeenCalledTimes(1)
    const enviado = held.api.aiCheck.mock.calls[0][0]
    expect(enviado.provider).toBe('openai')
    expect(enviado.key).toBe('sk-nueva-clave')
    expect(enviado.base_url).toBeUndefined()   // la del catalogo no se repite
    expect(held.api.aiSaveProfile).not.toHaveBeenCalled()
    expect(w.find('.key-state.ok').text()).toContain('Funciona')

    await w.findAll('button').find((b) => b.text() === 'Guardar y usar').trigger('click')
    await flushPromises()
    expect(held.api.aiSaveProfile).toHaveBeenCalledTimes(1)
    expect(held.api.aiSaveProfile.mock.calls[0][0]).toMatchObject({ provider: 'openai', key: 'sk-nueva-clave', activate: true })
    expect(w.emitted('saved')).toBeTruthy()
    expect(w.emitted('close')).toBeTruthy()
    expect(held.state.aiActive).toBe('openai')
    w.unmount()
  })

  it('con clave guardada la caja llega vacia y se conserva si no se escribe otra', async () => {
    held.state.aiProfiles = {
      openai: { id: 'openai', provider: 'openai', provider_name: 'OpenAI', has_key: true, key: 'sk-a…', model: 'chico', chat_model: 'grande', fields: {}, headers: {}, extra: {} }
    }
    held.state.aiActive = 'openai'
    const w = await abrir('openai')
    expect(w.text()).toContain('OpenAI')
    const clave = w.findAll('input').find((i) => i.attributes('type') === 'password')
    expect(clave.element.value).toBe('')
    expect(clave.attributes('placeholder')).toContain('guardada: sk-a…')
    await w.findAll('button').find((b) => b.text() === 'Guardar y usar').trigger('click')
    await flushPromises()
    expect(held.api.aiSaveProfile.mock.calls[0][0].key).toBeUndefined()
    w.unmount()
  })

  it('un servidor propio pide nombre y URL http(s)', async () => {
    const w = await abrir()
    await tarjeta(w, 'Añadir otro servidor').trigger('click')
    await flushPromises()
    expect(w.text()).toContain('falta la URL')
    const url = w.findAll('input').find((i) => i.attributes('placeholder') === 'https://mi-servidor/v1')
    await url.setValue('http://casa:8080/v1')
    await w.findAll('input').find((i) => i.attributes('placeholder') === 'Servidor de la iglesia').setValue('Casa')
    expect(w.text()).not.toContain('falta la URL')
    await w.findAll('button').find((b) => b.text() === 'Guardar y usar').trigger('click')
    await flushPromises()
    expect(held.api.aiSaveProfile.mock.calls[0][0]).toMatchObject({ provider: 'custom', name: 'Casa', base_url: 'http://casa:8080/v1' })
    w.unmount()
  })

  it('los parametros extra tienen que ser JSON', async () => {
    const w = await abrir()
    await tarjeta(w, 'Ollama').trigger('click')
    await flushPromises()
    await w.find('.ai-advanced-toggle').trigger('click')
    const extra = w.find('textarea[placeholder^="{"]')
    await extra.setValue('esto no es json')
    await w.findAll('button').find((b) => b.text() === 'Probar').trigger('click')
    await flushPromises()
    expect(held.api.aiCheck).not.toHaveBeenCalled()
    expect(w.text()).toContain('JSON')
    await extra.setValue('{"reasoning_effort": "low"}')
    await w.findAll('button').find((b) => b.text() === 'Probar').trigger('click')
    await flushPromises()
    expect(held.api.aiCheck.mock.calls[0][0].extra).toEqual({ reasoning_effort: 'low' })
    w.unmount()
  })

  it('quitar un proveedor pregunta antes', async () => {
    held.state.aiProfiles = {
      openai: { id: 'openai', provider: 'openai', provider_name: 'OpenAI', has_key: true, key: 'sk-a…', model: 'chico', chat_model: 'grande', fields: {}, headers: {}, extra: {} }
    }
    held.state.aiActive = 'openai'
    const w = await abrir('openai')
    await w.findAll('button').find((b) => b.text() === 'Quitar').trigger('click')
    await flushPromises()
    expect(held.api.aiDeleteProfile).not.toHaveBeenCalled()
    dialogOk()
    await flushPromises()
    expect(held.api.aiDeleteProfile).toHaveBeenCalledWith('openai')
    expect(held.state.aiProfiles.openai).toBeUndefined()
    w.unmount()
  })
})

describe('el selector de modelos', () => {
  const modelos = [
    { id: 'grande', name: 'Grande', tools: true, cost_in: 1, cost_out: 5, context: 128000, released: '2026-09-01', deprecated: false, known: true },
    { id: 'chico', name: 'Chico', tools: true, cost_in: 0.1, cost_out: 0.4, context: 32000, released: '2026-08-01', deprecated: false, known: true },
    { id: 'mudo', name: 'Mudo', tools: false, cost_in: 0.1, cost_out: 0.1, context: 8000, released: '2026-07-01', deprecated: false, known: true },
    { id: 'viejo', name: 'Viejo', tools: true, cost_in: 1, cost_out: 1, context: 8000, released: '2024-01-01', deprecated: true, known: true }
  ]
  const montar = (props = {}) => mount(ModelPicker, { props: { modelValue: '', models: modelos, ...props }, attachTo: document.body })

  it('enseña precio, contexto, herramientas y el recomendado', async () => {
    const w = montar({ suggest: 'chico' })
    await w.find('input').trigger('focus')
    const filas = w.findAll('.model-row')
    expect(filas[0].text()).toContain('Chico')
    expect(filas[0].text()).toContain('recomendado')
    expect(filas[0].text()).toContain('$0.1 → $0.4')
    expect(filas[0].text()).toContain('32k ctx')
    expect(filas.find((f) => f.text().includes('Mudo')).text()).toContain('sin herramientas')
    // los obsoletos van escondidos de entrada
    expect(filas.some((f) => f.text().includes('Viejo'))).toBe(false)
    w.unmount()
  })

  it('para conversar filtra de entrada los que no usan herramientas', async () => {
    const w = montar({ needTools: true })
    await w.find('input').trigger('focus')
    const nombres = w.findAll('.model-name').map((f) => f.text())
    expect(nombres).toContain('Grande')
    expect(nombres).not.toContain('Mudo')
    w.unmount()
  })

  it('lo escrito filtra y vale aunque no este en la lista', async () => {
    const w = montar()
    await w.find('input').setValue('gra')
    expect(w.findAll('.model-name').map((f) => f.text())).toEqual(['Grande'])
    expect(w.emitted('update:modelValue').at(-1)).toEqual(['gra'])
    await w.find('input').setValue('mi-modelo-raro')
    expect(w.text()).toContain('nada coincide')
    expect(w.emitted('update:modelValue').at(-1)).toEqual(['mi-modelo-raro'])
    w.unmount()
  })

  it('elegir una fila emite su id y avisa si no usa herramientas', async () => {
    const w = montar()
    await w.find('input').trigger('focus')
    await w.findAll('.model-row').find((f) => f.text().includes('Mudo')).trigger('click')
    expect(w.emitted('update:modelValue').at(-1)).toEqual(['mudo'])
    await w.setProps({ modelValue: 'mudo' })
    expect(w.find('.model-warn').text()).toBe('sin herramientas')
    w.unmount()
  })
})

describe('la tarjeta de IA en Ajustes', () => {
  it('sin proveedor lo dice y ofrece elegir uno', async () => {
    const w = mount(SettingsPage, { attachTo: document.body })
    await flushPromises(); await flushPromises()
    expect(w.text()).toContain('Sin proveedor elegido')
    const boton = w.findAll('button').find((b) => b.text() === 'Elegir proveedor…')
    expect(boton).toBeTruthy()
    await boton.trigger('click')
    await flushPromises()
    expect(w.find('.ai-modal').exists()).toBe(true)
    expect(w.text()).toContain('¿Qué IA quieres usar?')
    w.unmount()
  })

  it('con varios configurados se cambia de uno a otro con un clic', async () => {
    held.state.aiProfiles = {
      openai: { id: 'openai', provider: 'openai', provider_name: 'OpenAI', has_key: true, key: 'sk-a…', model: 'chico', chat_model: 'grande', fields: {}, headers: {}, extra: {} },
      ollama: { id: 'ollama', provider: 'ollama', provider_name: 'Ollama', has_key: false, key: '', model: 'qwen3:8b', chat_model: 'qwen3:8b', fields: {}, headers: {}, extra: {} }
    }
    held.state.aiActive = 'openai'
    const w = mount(SettingsPage, { attachTo: document.body })
    await flushPromises(); await flushPromises()
    expect(w.find('.ai-current').text()).toContain('OpenAI')
    expect(w.find('.ai-current').text()).toContain('grande')
    const chips = w.findAll('.ai-profile')
    expect(chips).toHaveLength(2)
    await chips.find((c) => c.text().includes('Ollama')).trigger('click')
    await flushPromises()
    expect(held.api.aiActivate).toHaveBeenCalledWith('ollama')
    expect(w.find('.ai-current').text()).toContain('Ollama')
    w.unmount()
  })
})

describe('probar gratis, sin clave', () => {
  it('desde Ajustes activa el primer gratuito que responde y lo cuenta', async () => {
    const w = mount(SettingsPage, { attachTo: document.body })
    await flushPromises(); await flushPromises()
    const boton = w.findAll('button').find((b) => b.text() === 'Probar gratis, sin clave')
    expect(boton).toBeTruthy()
    await boton.trigger('click')
    await flushPromises(); await flushPromises()
    expect(held.api.aiFree).toHaveBeenCalledTimes(1)
    expect(held.state.aiActive).toBe('llm7')
    expect(w.find('.key-state.ok').text()).toContain('LLM7')
    expect(w.find('.key-state.ok').text()).toContain('terceros')
    expect(w.find('.ai-current').text()).toContain('LLM7')
    // ya hay proveedor: el boton de gratis deja paso a «Probar»
    expect(w.findAll('button').some((b) => b.text() === 'Probar gratis, sin clave')).toBe(false)
    w.unmount()
  })

  it('el modal lo ofrece en el grupo gratuito y se cierra al conseguirlo', async () => {
    const w = await abrir()
    expect(w.text()).toContain('Gratis, sin clave')
    await w.find('.ai-free-btn').trigger('click')
    await flushPromises()
    expect(held.api.aiFree).toHaveBeenCalledTimes(1)
    expect(w.emitted('saved')).toBeTruthy()
    expect(w.emitted('close')).toBeTruthy()
    w.unmount()
  })
})

describe('respaldo y gasto en Ajustes', () => {
  it('enseña lo gastado y deja apagar el respaldo', async () => {
    held.state.aiProfiles = {
      openai: { id: 'openai', provider: 'openai', provider_name: 'OpenAI', has_key: true, key: 'sk-a…', model: 'chico', chat_model: 'grande', fields: {}, headers: {}, extra: {} },
      llm7: { id: 'llm7', provider: 'llm7', provider_name: 'LLM7', has_key: false, key: '', model: 'x', chat_model: 'x', fields: {}, headers: {}, extra: {} }
    }
    held.state.aiActive = 'openai'
    const w = mount(SettingsPage, { attachTo: document.body })
    await flushPromises(); await flushPromises()
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
