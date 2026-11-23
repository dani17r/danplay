import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'

// En una pantalla grande la interfaz se agranda sola (ui_scale.rs, en Rust):
// Ajustes › Apariencia lo enseña, dice cuánto y deja apagarlo. Solo en la app
// de escritorio: en el navegador manda el zoom del propio navegador.
const held = vi.hoisted(() => ({ app: null, inTauri: true }))

vi.mock('../src/api.js', async (importOriginal) => {
  const actual = await importOriginal()
  const { createState, buildApiDouble, createAppDouble } = await import('./support/backend.js')
  held.app = createAppDouble()
  return {
    ...actual,
    get inTauri() {
      return held.inTauri
    },
    api: buildApiDouble(actual, createState()),
    app: held.app
  }
})

import SettingsPage from '../src/components/SettingsPage.vue'

let w = null
beforeEach(() => {
  vi.clearAllMocks()
  held.inTauri = true
  localStorage.clear()
})
afterEach(() => {
  w?.unmount()
  w = null
})
const mountView = async () => {
  w = mount(SettingsPage, { attachTo: document.body })
  await flushPromises()
  await flushPromises()
  return w
}
const toggle = () =>
  w.findAll('label.toggle').find((l) => l.text().includes('Agrandar la interfaz'))

describe('the interface grows on big screens', () => {
  it('shows the switch on, and says how much it grows now', async () => {
    held.app.uiScale.mockResolvedValueOnce([true, 1.25])
    await mountView()
    const t = toggle()
    expect(t).toBeTruthy()
    expect(t.find('[role="switch"]').attributes('aria-checked')).toBe('true')
    expect(t.text()).toContain('al 125 %')
  })

  it('with a window like a laptop, says it is not needed', async () => {
    held.app.uiScale.mockResolvedValueOnce([true, 1])
    await mountView()
    expect(toggle().text()).toContain('no hace falta')
  })

  it('can be turned off, and Rust is told', async () => {
    held.app.uiScale.mockResolvedValueOnce([true, 1.25])
    await mountView()
    await toggle().find('[role="switch"]').trigger('click')
    await flushPromises()
    expect(held.app.setUiScale).toHaveBeenCalledWith(false)
    expect(toggle().find('[role="switch"]').attributes('aria-checked')).toBe('false')
  })

  it('is not offered in the browser, where the browser zooms', async () => {
    held.inTauri = false
    await mountView()
    expect(toggle()).toBeUndefined()
    expect(held.app.uiScale).not.toHaveBeenCalled()
  })
})
