import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'

// El ojito de la clave de IA: el listado enseña la clave enmascarada; con el
// ojito se trae entera del núcleo (y se ve, para comprobar lo que se pegó).
const held = vi.hoisted(() => ({ state: null, api: null }))

vi.mock('../src/api.js', async (importOriginal) => {
  const actual = await importOriginal()
  const { createState, buildApiDouble } = await import('./support/backend.js')
  held.state = createState()
  held.api = buildApiDouble(actual, held.state)
  const { vi: v } = await import('vitest')
  held.api.aiKey = v.fn(async () => ({ key: 'sk-clave-entera-1234567890' }))
  const app = { ...actual.app, openInBrowser: v.fn(async () => {}) }
  return { ...actual, api: held.api, app, inTauri: true }
})

import AiProviderModal from '../src/components/AiProviderModal.vue'

beforeEach(() => {
  vi.clearAllMocks()
  held.state.aiProfiles = {
    openai: {
      id: 'openai',
      provider: 'openai',
      provider_name: 'OpenAI',
      has_key: true,
      key: 'sk-c…7890',
      model: 'chico',
      chat_model: 'grande',
      fields: {},
      headers: {},
      extra: {}
    }
  }
  held.state.aiActive = 'openai'
})

describe('the key can be seen and copied', () => {
  it('the eye brings the saved key whole, and shows it', async () => {
    const w = mount(AiProviderModal, {
      props: { open: true, initial: 'openai' },
      attachTo: document.body
    })
    await flushPromises()
    await flushPromises()
    const eye = w.find('button[aria-label="Ver la clave"]')
    expect(eye.exists(), 'el ojito').toBe(true)
    // por defecto, la clave va oculta
    expect(w.find('.ai-key input').attributes('type')).toBe('password')
    await eye.trigger('click')
    await flushPromises()
    expect(held.api.aiKey).toHaveBeenCalledWith('openai')
    expect(w.find('.ai-key input').attributes('type')).toBe('text')
    expect(w.find('.ai-key input').element.value).toBe('sk-clave-entera-1234567890')
    expect(w.find('button[aria-label="Ocultar la clave"]').exists()).toBe(true)
    w.unmount()
  })
})
