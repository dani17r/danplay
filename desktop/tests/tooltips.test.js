// Los avisos de los iconos (v-tips): al dejar el ratón encima un momento sale
// un globo con lo que hace el botón (su `title`), sin el aviso del sistema a
// la vez; se va al salir, al pulsar o con Escape, y el `title` vuelve.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { mount } from '@vue/test-utils'
import { h, withDirectives } from 'vue'
import { vTips, TIP_DELAY } from '../src/composables/useTooltips.js'

const Box = {
  render: () =>
    withDirectives(
      h('div', { class: 'box' }, [
        h('button', { class: 'one', title: 'Grande: todo el alto' }, [h('span', { class: 'ico' })]),
        h('button', { class: 'two' }, 'sin aviso')
      ]),
      [[vTips]]
    )
}

let w
const tip = () => document.getElementById('danplay-tip')
const fire = (el, type, extra = {}) =>
  el.dispatchEvent(new PointerEvent(type, { bubbles: true, ...extra }))

beforeEach(() => {
  vi.useFakeTimers()
  w = mount(Box, { attachTo: document.body })
})
afterEach(() => {
  w.unmount()
  vi.useRealTimers()
  document.body.innerHTML = ''
})

describe('los avisos de los iconos', () => {
  it('salen al dejar el ratón un momento, y se van al salir', async () => {
    const one = w.find('.one').element
    // sobre el icono de dentro: cuenta el botón
    fire(w.find('.one .ico').element, 'pointerover')
    expect(one.hasAttribute('title'), 'sin el aviso del sistema a la vez').toBe(false)
    expect(tip()?.classList.contains('on') ?? false, 'aún no').toBe(false)
    vi.advanceTimersByTime(TIP_DELAY + 5)
    await vi.runAllTimersAsync()
    expect(tip().textContent).toBe('Grande: todo el alto')
    expect(one.getAttribute('aria-describedby')).toBe('danplay-tip')
    fire(one, 'pointerout', { relatedTarget: document.body })
    expect(tip().classList.contains('on')).toBe(false)
    expect(one.getAttribute('title'), 'el title vuelve').toBe('Grande: todo el alto')
  })

  it('pulsar o Escape lo quitan; un botón sin aviso no saca nada', async () => {
    const one = w.find('.one').element
    fire(one, 'pointerover')
    vi.advanceTimersByTime(TIP_DELAY + 5)
    await vi.runAllTimersAsync()
    expect(tip().classList.contains('on')).toBe(true)
    one.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }))
    expect(tip().classList.contains('on')).toBe(false)

    fire(one, 'pointerover')
    fire(one, 'pointerdown')
    vi.advanceTimersByTime(TIP_DELAY + 5)
    await vi.runAllTimersAsync()
    expect(tip().classList.contains('on'), 'al pulsar, antes de salir, no sale').toBe(false)

    // y lo pulsado no lo vuelve a sacar hasta salir de él y volver
    fire(one, 'pointerover')
    vi.advanceTimersByTime(TIP_DELAY + 5)
    await vi.runAllTimersAsync()
    expect(tip().classList.contains('on')).toBe(false)
    fire(one, 'pointerout', { relatedTarget: document.body })
    fire(one, 'pointerover')
    vi.advanceTimersByTime(TIP_DELAY + 5)
    await vi.runAllTimersAsync()
    expect(tip().classList.contains('on')).toBe(true)
    fire(one, 'pointerout', { relatedTarget: document.body })

    const before = tip().textContent
    fire(w.find('.two').element, 'pointerover')
    vi.advanceTimersByTime(TIP_DELAY + 5)
    await vi.runAllTimersAsync()
    expect(tip().classList.contains('on')).toBe(false)
    expect(tip().textContent).toBe(before)
  })

  it('al desmontar se quitan sus escuchas', async () => {
    const one = w.find('.one').element
    w.unmount()
    fire(one, 'pointerover')
    expect(one.getAttribute('title')).toBe('Grande: todo el alto')
    w = mount(Box, { attachTo: document.body })
  })
})
