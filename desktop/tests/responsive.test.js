import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import { readFileSync } from 'node:fs'
import Drawer from '../src/components/ui/Drawer.vue'
import { PHONE_MAX, TABLET_MAX } from '../src/composables/useViewport.js'

// La app de escritorio y la de movil comparten esta base. Lo que cambia de
// COMPORTAMIENTO (los laterales dejan de ocupar sitio y se abren encima) vive
// en useViewport; el css solo se ocupa del aspecto.

const css = readFileSync('src/style.css', 'utf8')

describe('tramos de pantalla', () => {
  it('el movil es mas estrecho que la tablet', () => {
    expect(PHONE_MAX).toBeLessThan(TABLET_MAX)
  })

  it('hay reglas para los dos tramos', () => {
    expect(css).toContain(`@media (max-width:${TABLET_MAX}px)`)
    expect(css).toContain(`@media (max-width:${PHONE_MAX}px)`)
  })

  it('en movil lo pulsable crece para el dedo', () => {
    const movil = css.slice(css.indexOf(`@media (max-width:${PHONE_MAX}px)`))
    // los objetivos tactiles comodos no bajan de ~38px
    expect(movil).toMatch(/\.pl-play\{width:46px/)
    expect(movil).toMatch(/\.pl-btn\{width:38px/)
  })

  it('el fondo no se desplaza con un panel abierto encima', () => {
    expect(css).toMatch(/body\.drawer-open\{overflow:hidden\}/)
  })

  it('se respeta quien pide menos movimiento', () => {
    expect(css).toMatch(/prefers-reduced-motion[\s\S]*\.drawer/)
  })
})

describe('panel que se abre por un lado', () => {
  const montar = (props = {}) =>
    mount(Drawer, {
      props: { open: true, side: 'left', title: 'Menu', ...props },
      slots: { default: '<p class="dentro">contenido</p>' },
      attachTo: document.body
    })

  beforeEach(() => { document.body.className = ''; document.body.innerHTML = '' })

  it('cerrado no pinta nada', () => {
    montar({ open: false })
    expect(document.querySelector('.drawer')).toBeNull()
  })

  it('abierto muestra su contenido', () => {
    montar()
    expect(document.querySelector('.dentro')).toBeTruthy()
    expect(document.querySelector('.drawer-left')).toBeTruthy()
  })

  it('se abre por el lado que se le pida', () => {
    montar({ side: 'right' })
    expect(document.querySelector('.drawer-right')).toBeTruthy()
  })

  it('pulsar fuera lo cierra', async () => {
    const w = montar()
    await document.querySelector('.drawer-backdrop').dispatchEvent(
      Object.assign(new MouseEvent('click', { bubbles: false }), {}))
    await flushPromises()
    expect(w.emitted('close')).toBeTruthy()
  })

  it('el boton de cerrar lo cierra', async () => {
    const w = montar()
    document.querySelector('.drawer-head .icon-btn').click()
    await flushPromises()
    expect(w.emitted('close')).toBeTruthy()
  })

  it('mientras esta abierto marca el body, y lo suelta al cerrarse', async () => {
    const w = montar()
    await flushPromises()
    expect(document.body.classList.contains('drawer-open')).toBe(true)
    await w.setProps({ open: false })
    await flushPromises()
    expect(document.body.classList.contains('drawer-open')).toBe(false)
  })
})
