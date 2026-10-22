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

describe('the checkbox', () => {
  it('with a checkbox, accepting reports whether it was ticked', async () => {
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

  it('without a checkbox, accepting is still true', async () => {
    const w = await open()
    expect(document.querySelector('.modal-check')).toBeNull()
    button('Aceptar').click()
    await flushPromises()
    expect(w.emitted('ok')[0]).toEqual([true])
  })
})

describe('the dialog is keyboard operable', () => {
  it('Escape also closes a confirmation, not just the prompts asking for text', async () => {
    const w = await open()
    // la tecla llega al documento, que es donde cae el foco cuando no hay campo
    await press(document, 'Escape')
    expect(w.emitted('cancel'), 'Escape no cerro la confirmacion').toBeTruthy()
  })

  it('Enter accepts a confirmation', async () => {
    const w = await open()
    await press(document, 'Enter')
    expect(w.emitted('ok')).toBeTruthy()
  })

  it('Enter with focus on «Cancelar» does not accept', async () => {
    // Aceptaba: «Mandar a la papelera», «Borrar la lista», «Conservar solo
    // esta»... con el foco puesto en Cancelar.
    const w = await open({ okLabel: 'A la papelera', danger: true })
    const cancel = button('Cancelar')
    cancel.focus()
    await press(cancel, 'Enter')
    expect(w.emitted('ok'), 'acepto con el foco en Cancelar').toBeFalsy()
  })

  it('stops listening to the keyboard once closed', async () => {
    const w = await open()
    await w.setProps({ open: false })
    await flushPromises()
    await press(document, 'Escape')
    expect(w.emitted('cancel'), 'sigue reaccionando con el dialogo cerrado').toBeFalsy()
  })
})

describe('focus does not escape', () => {
  it('on open it goes to the main button; if it asks for text, to the field', async () => {
    await open({ okLabel: 'Adelante' })
    expect(document.activeElement.textContent.trim()).toBe('Adelante')
    const w = await open({ kind: 'prompt', value: 'Domingo', title: 'Nueva lista' })
    expect(document.activeElement.tagName).toBe('INPUT')
    expect(document.activeElement.value).toBe('Domingo')
    w.unmount()
  })

  it('Tab wraps around inside the dialog', async () => {
    await open({ okLabel: 'Adelante' })
    const [cancel, proceed] = [button('Cancelar'), button('Adelante')]
    proceed.focus()
    await press(document.activeElement, 'Tab')
    expect(document.activeElement).toBe(cancel)
    await press(document.activeElement, 'Tab', { shiftKey: true })
    expect(document.activeElement).toBe(proceed)
  })

  it('on close it returns to where it was', async () => {
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

  it('is announced as a dialog with its title and message', async () => {
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

describe('asking for a folder', () => {
  it('«Elegir…» opens the picker and puts the choice in the field, which can be accepted', async () => {
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

  it('if you cancel the picker, the field stays as it was; without `browse` there is no button', async () => {
    await open({ kind: 'prompt', value: '/musica', browse: async () => null })
    button('Elegir…').click()
    await flushPromises()
    expect(document.querySelector('.modal input').value).toBe('/musica')
    for (const w of mounted.splice(0)) w.unmount()
    await open({ kind: 'prompt', value: '/musica' })
    expect(button('Elegir…')).toBeUndefined()
  })
})
