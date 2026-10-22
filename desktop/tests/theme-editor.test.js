import { describe, it, expect, beforeEach, afterEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import ThemeEditor from '../src/components/ThemeEditor.vue'
import { CATALOG, customThemes, saveCustomTheme, applyTheme } from '../src/themes.js'

// El editor de temas: parte de un tema, enseña los cambios al momento y no
// guarda nada hasta que se pulsa Guardar.
const own = (accent, name) => ({
  name,
  kind: 'dark',
  custom: true,
  v: { ...CATALOG.night.v, accent }
})
const root = () => document.documentElement

let w = null
beforeEach(() => {
  localStorage.clear()
  saveCustomTheme('custom-red', own('#aa0000', 'Rojo'))
  saveCustomTheme('custom-blue', own('#0000aa', 'Azul'))
  applyTheme('night')
})
afterEach(() => {
  w?.unmount()
  w = null
})
const mountView = (props) => {
  w = mount(ThemeEditor, { props: { activeTheme: 'night', ...props }, attachTo: document.body })
  return w
}
const button = (text) => w.findAll('button').find((b) => b.text().includes(text))
const hex = (k) =>
  w
    .findAll('.color-row')
    .find((r) => r.find('.color-name').text() === k)
    .find('.color-hex')

describe('editing a custom theme', () => {
  it('starts from the colors of that theme and saves to it', async () => {
    mountView({ editing: 'custom-red' })
    expect(w.find('h3').text()).toBe('Editar tema')
    expect(hex('Acento').element.value).toBe('#aa0000')
    await hex('Acento').setValue('#bb1111')
    await button('Guardar tema').trigger('click')
    expect(customThemes()['custom-red'].v.accent).toBe('#bb1111')
    expect(w.emitted('saved')[0]).toEqual(['custom-red'])
  })

  it('with the editor open, editing another theme starts it with the colors of that one', async () => {
    // Se quedaban los colores del primero y «Guardar» se los escribia al
    // segundo: el azul acababa rojo.
    mountView({ editing: 'custom-red' })
    await w.setProps({ editing: 'custom-blue' })
    await flushPromises()
    expect(w.find('input').element.value).toBe('Azul')
    expect(hex('Acento').element.value).toBe('#0000aa')
    await button('Guardar tema').trigger('click')
    expect(customThemes()['custom-blue'].v.accent).toBe('#0000aa')
    expect(customThemes()['custom-red'].v.accent).toBe('#aa0000')
  })
})

describe('creating a theme', () => {
  it('starts from the active one and is saved with its name', async () => {
    mountView({ editing: null, activeTheme: 'ocean' })
    expect(w.find('h3').text()).toBe('Crear tema custom')
    expect(w.find('input').element.value).toBe('Oceano (mío)')
    await w.find('input').setValue('Mi Mar')
    await button('Guardar tema').trigger('click')
    const saved = customThemes()['custom-mi-mar']
    expect(saved.name).toBe('Mi Mar')
    expect(saved.v.accent).toBe(CATALOG.ocean.v.accent)
    expect(root().dataset.theme).toBe('custom-mi-mar')
  })

  it('«Partir de» copies the colors of another', async () => {
    mountView({ editing: null })
    await w.find('.select-box').trigger('click')
    await w
      .findAll('.select-opt')
      .find((o) => o.text().includes('Vino'))
      .trigger('click')
    await flushPromises()
    expect(hex('Acento').element.value).toBe(CATALOG.wine.v.accent)
    expect(w.find('input').element.value).toBe('Vino (mío)')
  })

  it('changes show right away, and cancelling restores the previous theme', async () => {
    mountView({ editing: null, activeTheme: 'night' })
    await hex('Acento').setValue('#123456')
    await flushPromises()
    expect(root().dataset.theme).toBe('__preview')
    expect(root().style.getPropertyValue('--accent')).toBe('#123456')
    await button('Cancelar').trigger('click')
    expect(root().dataset.theme).toBe('night')
    expect(w.emitted('close')).toBeTruthy()
    expect(Object.keys(customThemes())).toEqual(['custom-red', 'custom-blue'])
  })

  it('by default it shows six colors and, on request, all twelve', async () => {
    mountView({ editing: null })
    expect(w.findAll('.color-row')).toHaveLength(6)
    await button('Todos los colores').trigger('click')
    expect(w.findAll('.color-row')).toHaveLength(12)
    await button('Menos colores').trigger('click')
    expect(w.findAll('.color-row')).toHaveLength(6)
  })

  it('closing halfway does not leave the preview applied', async () => {
    mountView({ editing: null, activeTheme: 'midnight' })
    await hex('Acento').setValue('#654321')
    await flushPromises()
    w.unmount()
    w = null
    expect(root().dataset.theme).toBe('midnight')
  })
})
