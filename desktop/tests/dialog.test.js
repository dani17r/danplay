import { describe, it, expect, afterEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import ModalDialog from '../src/components/ui/ModalDialog.vue'

// El dialogo propio: se maneja entero con el teclado, no deja que el foco se
// vaya a lo de detras y, al cerrarse, lo devuelve a donde estaba.
const mounted = []
const open = async (props = {}) => {
  const w = mount(ModalDialog, {
    props: { open: true, kind: 'confirm', title: 'Borrar', message: '¿Seguro?', ...props },
    attachTo: document.body
  })
  mounted.push(w)
  await flushPromises()
  return w
}
afterEach(() => {
  for (const w of mounted.splice(0)) w.unmount()
  document.body.innerHTML = ''
})
const press = (target, key, extra = {}) => {
  target.dispatchEvent(
    new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true, ...extra })
  )
  return flushPromises()
}
const button = (text) =>
  [...document.querySelectorAll('.modal button')].find((b) => b.textContent.trim() === text)

describe('la casilla', () => {
  it('con casilla, aceptar dice si quedó marcada', async () => {
    const w = await open({ check: { label: 'Quitarla también de sus listas', value: true } })
    const checkbox = document.querySelector('.modal-check input')
    expect(checkbox.checked).toBe(true)
    expect(document.querySelector('.modal-check').textContent).toContain('de sus listas')
    checkbox.click()
    await flushPromises()
    button('Aceptar').click()
    await flushPromises()
    expect(w.emitted('ok')[0]).toEqual([{ checked: false }])
  })

  it('sin casilla, aceptar sigue siendo true', async () => {
    const w = await open()
    expect(document.querySelector('.modal-check')).toBeNull()
    button('Aceptar').click()
    await flushPromises()
    expect(w.emitted('ok')[0]).toEqual([true])
  })
})

describe('el dialogo se maneja con el teclado', () => {
  it('Escape cierra tambien una confirmacion, no solo los que piden texto', async () => {
    const w = await open()
    // la tecla llega al documento, que es donde cae el foco cuando no hay campo
    await press(document, 'Escape')
    expect(w.emitted('cancel'), 'Escape no cerro la confirmacion').toBeTruthy()
  })

  it('Enter acepta una confirmacion', async () => {
    const w = await open()
    await press(document, 'Enter')
    expect(w.emitted('ok')).toBeTruthy()
  })

  it('Enter con el foco en «Cancelar» no acepta', async () => {
    // Aceptaba: «Mandar a la papelera», «Borrar la lista», «Conservar solo
    // esta»... con el foco puesto en Cancelar.
    const w = await open({ okLabel: 'A la papelera', danger: true })
    const cancel = button('Cancelar')
    cancel.focus()
    await press(cancel, 'Enter')
    expect(w.emitted('ok'), 'acepto con el foco en Cancelar').toBeFalsy()
  })

  it('deja de escuchar el teclado al cerrarse', async () => {
    const w = await open()
    await w.setProps({ open: false })
    await flushPromises()
    await press(document, 'Escape')
    expect(w.emitted('cancel'), 'sigue reaccionando con el dialogo cerrado').toBeFalsy()
  })
})

describe('el foco no se escapa', () => {
  it('al abrir va al boton principal; si pide texto, al campo', async () => {
    await open({ okLabel: 'Adelante' })
    expect(document.activeElement.textContent.trim()).toBe('Adelante')
    const w = await open({ kind: 'prompt', value: 'Domingo', title: 'Nueva lista' })
    expect(document.activeElement.tagName).toBe('INPUT')
    expect(document.activeElement.value).toBe('Domingo')
    w.unmount()
  })

  it('Tab da la vuelta dentro del dialogo', async () => {
    await open({ okLabel: 'Adelante' })
    const [cancel, proceed] = [button('Cancelar'), button('Adelante')]
    proceed.focus()
    await press(document.activeElement, 'Tab')
    expect(document.activeElement).toBe(cancel)
    await press(document.activeElement, 'Tab', { shiftKey: true })
    expect(document.activeElement).toBe(proceed)
  })

  it('al cerrar vuelve a donde estaba', async () => {
    const before = document.createElement('button')
    before.textContent = 'el de detras'
    document.body.appendChild(before)
    before.focus()
    const w = await open()
    expect(document.activeElement).not.toBe(before)
    await w.setProps({ open: false })
    await flushPromises()
    expect(document.activeElement).toBe(before)
  })

  it('se anuncia como dialogo con su titulo y su mensaje', async () => {
    await open({ title: 'Borrar la lista', message: 'Las canciones no se borran' })
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

describe('pedir una carpeta', () => {
  it('«Elegir…» abre el selector y pone lo elegido en el campo, que se puede aceptar', async () => {
    const w = await open({
      kind: 'prompt',
      title: '¿Dónde guardo lo que descargues?',
      value: '/musica',
      browse: async () => '/mnt/disco/Musica'
    })
    button('Elegir…').click()
    await flushPromises()
    expect(document.querySelector('.modal input').value).toBe('/mnt/disco/Musica')
    button('Aceptar').click()
    expect(w.emitted('ok')[0]).toEqual(['/mnt/disco/Musica'])
  })

  it('si cancelas el selector, el campo se queda como estaba; sin `browse` no hay boton', async () => {
    await open({ kind: 'prompt', value: '/musica', browse: async () => null })
    button('Elegir…').click()
    await flushPromises()
    expect(document.querySelector('.modal input').value).toBe('/musica')
    for (const w of mounted.splice(0)) w.unmount()
    await open({ kind: 'prompt', value: '/musica' })
    expect(button('Elegir…')).toBeUndefined()
  })
})
