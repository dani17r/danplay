import { describe, it, expect, beforeEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import { allCss } from './support/css.js'
import Drawer from '../src/components/ui/Drawer.vue'
import { PHONE_MAX, TABLET_MAX } from '../src/composables/useViewport.js'

// La app de escritorio y la de movil comparten esta base. Lo que cambia de
// COMPORTAMIENTO (los laterales dejan de ocupar sitio y se abren encima) vive
// en useViewport; el css solo se ocupa del aspecto.

const css = allCss()

describe('screen breakpoints', () => {
  it('mobile is narrower than tablet', () => {
    expect(PHONE_MAX).toBeLessThan(TABLET_MAX)
  })

  it('there are rules for both breakpoints', () => {
    expect(css).toContain(`@media (max-width:${TABLET_MAX}px)`)
    expect(css).toContain(`@media (max-width:${PHONE_MAX}px)`)
  })

  it('on mobile, tap targets grow for fingers', () => {
    const mobile = css.slice(css.indexOf(`@media (max-width:${PHONE_MAX}px)`))
    // los objetivos tactiles comodos no bajan de ~38px
    expect(mobile).toMatch(/\.pl-play\{width:46px/)
    expect(mobile).toMatch(/\.pl-btn\{width:38px/)
  })

  it('the background does not scroll with a panel open on top', () => {
    expect(css).toMatch(/body\.drawer-open\{overflow:hidden\}/)
  })

  it('reduced motion preferences are respected', () => {
    expect(css).toMatch(/prefers-reduced-motion[\s\S]*\.drawer/)
  })
})

describe('a panel that opens from one side', () => {
  const mountView = (props = {}) =>
    mount(Drawer, {
      props: { open: true, side: 'left', title: 'Menu', ...props },
      slots: { default: '<p class="dentro">contenido</p>' },
      attachTo: document.body
    })

  beforeEach(() => {
    document.body.className = ''
    document.body.innerHTML = ''
  })

  it('closed renders nothing', () => {
    mountView({ open: false })
    expect(document.querySelector('.drawer')).toBeNull()
  })

  it('open shows its content', () => {
    mountView()
    expect(document.querySelector('.dentro')).toBeTruthy()
    expect(document.querySelector('.drawer-left')).toBeTruthy()
  })

  it('opens from the requested side', () => {
    mountView({ side: 'right' })
    expect(document.querySelector('.drawer-right')).toBeTruthy()
  })

  it('clicking outside closes it', async () => {
    const w = mountView()
    await document
      .querySelector('.drawer-backdrop')
      .dispatchEvent(Object.assign(new MouseEvent('click', { bubbles: false }), {}))
    await flushPromises()
    expect(w.emitted('close')).toBeTruthy()
  })

  it('the close button closes it', async () => {
    const w = mountView()
    document.querySelector('.drawer-head .icon-btn').click()
    await flushPromises()
    expect(w.emitted('close')).toBeTruthy()
  })

  it('while open it marks the body, and releases it on close', async () => {
    const w = mountView()
    await flushPromises()
    expect(document.body.classList.contains('drawer-open')).toBe(true)
    await w.setProps({ open: false })
    await flushPromises()
    expect(document.body.classList.contains('drawer-open')).toBe(false)
  })
})
