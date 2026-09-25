import { describe, it, expect, afterEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import ModalDialog from '../src/components/ui/ModalDialog.vue'

// El dialogo propio: se maneja entero con el teclado, no deja que el foco se
// vaya a lo de detras y, al cerrarse, lo devuelve a donde estaba.
const montados = []
const abrir = async (props = {}) => {
  const w = mount(ModalDialog, {
    props: { open: true, kind: 'confirm', title: 'Borrar', message: '¿Seguro?', ...props },
    attachTo: document.body
  })
  montados.push(w)
  await flushPromises()
  return w
}
afterEach(() => {
  for (const w of montados.splice(0)) w.unmount()
  document.body.innerHTML = ''
})
const tecla = (target, key, extra = {}) => {
  target.dispatchEvent(
    new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true, ...extra })
  )
  return flushPromises()
}
const boton = (text) =>
  [...document.querySelectorAll('.modal button')].find((b) => b.textContent.trim() === text)

describe('el dialogo se maneja con el teclado', () => {
  it('Escape cierra tambien una confirmacion, no solo los que piden texto', async () => {
    const w = await abrir()
    // la tecla llega al documento, que es donde cae el foco cuando no hay campo
    await tecla(document, 'Escape')
    expect(w.emitted('cancel'), 'Escape no cerro la confirmacion').toBeTruthy()
  })

  it('Enter acepta una confirmacion', async () => {
    const w = await abrir()
    await tecla(document, 'Enter')
    expect(w.emitted('ok')).toBeTruthy()
  })

  it('Enter con el foco en «Cancelar» no acepta', async () => {
    // Aceptaba: «Mandar a la papelera», «Borrar la lista», «Conservar solo
    // esta»... con el foco puesto en Cancelar.
    const w = await abrir({ okLabel: 'A la papelera', danger: true })
    const cancelar = boton('Cancelar')
    cancelar.focus()
    await tecla(cancelar, 'Enter')
    expect(w.emitted('ok'), 'acepto con el foco en Cancelar').toBeFalsy()
  })

  it('deja de escuchar el teclado al cerrarse', async () => {
    const w = await abrir()
    await w.setProps({ open: false })
    await flushPromises()
    await tecla(document, 'Escape')
    expect(w.emitted('cancel'), 'sigue reaccionando con el dialogo cerrado').toBeFalsy()
  })
})

describe('el foco no se escapa', () => {
  it('al abrir va al boton principal; si pide texto, al campo', async () => {
    await abrir({ okLabel: 'Adelante' })
    expect(document.activeElement.textContent.trim()).toBe('Adelante')
    const w = await abrir({ kind: 'prompt', value: 'Domingo', title: 'Nueva lista' })
    expect(document.activeElement.tagName).toBe('INPUT')
    expect(document.activeElement.value).toBe('Domingo')
    w.unmount()
  })

  it('Tab da la vuelta dentro del dialogo', async () => {
    await abrir({ okLabel: 'Adelante' })
    const [cancelar, adelante] = [boton('Cancelar'), boton('Adelante')]
    adelante.focus()
    await tecla(document.activeElement, 'Tab')
    expect(document.activeElement).toBe(cancelar)
    await tecla(document.activeElement, 'Tab', { shiftKey: true })
    expect(document.activeElement).toBe(adelante)
  })

  it('al cerrar vuelve a donde estaba', async () => {
    const antes = document.createElement('button')
    antes.textContent = 'el de detras'
    document.body.appendChild(antes)
    antes.focus()
    const w = await abrir()
    expect(document.activeElement).not.toBe(antes)
    await w.setProps({ open: false })
    await flushPromises()
    expect(document.activeElement).toBe(antes)
  })

  it('se anuncia como dialogo con su titulo y su mensaje', async () => {
    await abrir({ title: 'Borrar la lista', message: 'Las canciones no se borran' })
    const d = document.querySelector('[role="dialog"]')
    expect(d.getAttribute('aria-modal')).toBe('true')
    expect(document.getElementById(d.getAttribute('aria-labelledby')).textContent).toContain(
      'Borrar la lista'
    )
    expect(document.getElementById(d.getAttribute('aria-describedby')).textContent).toContain(
      'no se borran'
    )
  })
})
