import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'

// El chat sin IA configurada: lo dice y ofrece probar un servicio gratuito
// sin clave, en vez de dejar que escribas y te conteste un error.
const held = vi.hoisted(() => ({ available: false, free: null }))

vi.mock('../src/api.js', async (importOriginal) => {
  const actual = await importOriginal()
  const { vi: v } = await import('vitest')
  held.free = v.fn(async () => {
    held.available = true
    return { free: { ok: true, chosen: 'llm7', name: 'LLM7', chat_model: 'minimax-m2.7' } }
  })
  const api = {
    ...actual.api,
    chatTools: v.fn(async () => ({ model: held.available ? 'minimax-m2.7' : '', provider: held.available ? 'LLM7' : '',
                                   available: held.available, reason: held.available ? '' : 'no hay ningun proveedor de IA elegido', tools: [] })),
    aiFree: held.free
  }
  return { ...actual, api, inTauri: true }
})

import ChatPage from '../src/components/ChatPage.vue'
import { resetDownloads } from '../src/composables/useDownloads.js'

beforeEach(() => { held.available = false; vi.clearAllMocks(); resetDownloads(); localStorage.clear() })

describe('el chat sin IA', () => {
  it('avisa, ofrece probar gratis y al conseguirlo enseña el proveedor', async () => {
    const w = mount(ChatPage, { attachTo: document.body })
    await flushPromises(); await flushPromises()
    expect(w.find('.chat-noai').exists()).toBe(true)
    expect(w.find('.chat-noai').text()).toContain('no hay ningun proveedor')
    await w.find('.chat-noai button').trigger('click')
    await flushPromises(); await flushPromises()
    expect(held.free).toHaveBeenCalledTimes(1)
    expect(w.find('.chat-noai').exists()).toBe(false)
    expect(w.find('.chat-model').text()).toContain('LLM7')
    w.unmount()
  })

  it('con IA lista no molesta', async () => {
    held.available = true
    const w = mount(ChatPage, { attachTo: document.body })
    await flushPromises(); await flushPromises()
    expect(w.find('.chat-noai').exists()).toBe(false)
    w.unmount()
  })
})
