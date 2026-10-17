// Los avisos de los iconos (v-tips): al dejar el ratón encima un momento sale
// un globo con lo que hace el botón (su `title`), sin el aviso del sistema a
// la vez; se va al salir, al pulsar o con Escape, y el `title` vuelve.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { mount } from '@vue/test-utils'
import { h, withDirectives } from 'vue'
import { vTips, TIP_DELAY } from '../src/composables/useTooltips.js'

const Caja = {
  render: () =>
    withDirectives(
      h('div', { class: 'caja' }, [
        h('button', { class: 'uno', title: 'Grande: todo el alto' }, [h('span', { class: 'ico' })]),
        h('button', { class: 'dos' }, 'sin aviso')
      ]),
      [[vTips]]
    )
}

let w
const globo = () => document.getElementById('danplay-tip')
const pasa = (el, type, extra = {}) =>
  el.dispatchEvent(new PointerEvent(type, { bubbles: true, ...extra }))

beforeEach(() => {
  vi.useFakeTimers()
  w = mount(Caja, { attachTo: document.body })
})
afterEach(() => {
  w.unmount()
  vi.useRealTimers()
  document.body.innerHTML = ''
})

describe('los avisos de los iconos', () => {
  it('salen al dejar el ratón un momento, y se van al salir', async () => {
    const uno = w.find('.uno').element
    // sobre el icono de dentro: cuenta el botón
    pasa(w.find('.uno .ico').element, 'pointerover')
    expect(uno.hasAttribute('title'), 'sin el aviso del sistema a la vez').toBe(false)
    expect(globo()?.classList.contains('on') ?? false, 'aún no').toBe(false)
    vi.advanceTimersByTime(TIP_DELAY + 5)
    await vi.runAllTimersAsync()
    expect(globo().textContent).toBe('Grande: todo el alto')
    expect(uno.getAttribute('aria-describedby')).toBe('danplay-tip')
    pasa(uno, 'pointerout', { relatedTarget: document.body })
    expect(globo().classList.contains('on')).toBe(false)
    expect(uno.getAttribute('title'), 'el title vuelve').toBe('Grande: todo el alto')
  })

  it('pulsar o Escape lo quitan; un botón sin aviso no saca nada', async () => {
    const uno = w.find('.uno').element
    pasa(uno, 'pointerover')
    vi.advanceTimersByTime(TIP_DELAY + 5)
    await vi.runAllTimersAsync()
    expect(globo().classList.contains('on')).toBe(true)
    uno.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }))
    expect(globo().classList.contains('on')).toBe(false)

    pasa(uno, 'pointerover')
    pasa(uno, 'pointerdown')
    vi.advanceTimersByTime(TIP_DELAY + 5)
    await vi.runAllTimersAsync()
    expect(globo().classList.contains('on'), 'al pulsar, antes de salir, no sale').toBe(false)

    // y lo pulsado no lo vuelve a sacar hasta salir de él y volver
    pasa(uno, 'pointerover')
    vi.advanceTimersByTime(TIP_DELAY + 5)
    await vi.runAllTimersAsync()
    expect(globo().classList.contains('on')).toBe(false)
    pasa(uno, 'pointerout', { relatedTarget: document.body })
    pasa(uno, 'pointerover')
    vi.advanceTimersByTime(TIP_DELAY + 5)
    await vi.runAllTimersAsync()
    expect(globo().classList.contains('on')).toBe(true)
    pasa(uno, 'pointerout', { relatedTarget: document.body })

    const before = globo().textContent
    pasa(w.find('.dos').element, 'pointerover')
    vi.advanceTimersByTime(TIP_DELAY + 5)
    await vi.runAllTimersAsync()
    expect(globo().classList.contains('on')).toBe(false)
    expect(globo().textContent).toBe(before)
  })

  it('al desmontar se quitan sus escuchas', async () => {
    const uno = w.find('.uno').element
    w.unmount()
    pasa(uno, 'pointerover')
    expect(uno.getAttribute('title')).toBe('Grande: todo el alto')
    w = mount(Caja, { attachTo: document.body })
  })
})
