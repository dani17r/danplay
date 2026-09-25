import { describe, it, expect, beforeEach, afterEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import ThemeEditor from '../src/components/ThemeEditor.vue'
import { CATALOG, customThemes, saveCustomTheme, applyTheme } from '../src/themes.js'

// El editor de temas: parte de un tema, enseña los cambios al momento y no
// guarda nada hasta que se pulsa Guardar.
const propio = (accent, name) => ({
  name,
  kind: 'dark',
  custom: true,
  v: { ...CATALOG.night.v, accent }
})
const raiz = () => document.documentElement

let w = null
beforeEach(() => {
  localStorage.clear()
  saveCustomTheme('propio-rojo', propio('#aa0000', 'Rojo'))
  saveCustomTheme('propio-azul', propio('#0000aa', 'Azul'))
  applyTheme('night')
})
afterEach(() => {
  w?.unmount()
  w = null
})
const montar = (props) => {
  w = mount(ThemeEditor, { props: { activeTheme: 'night', ...props }, attachTo: document.body })
  return w
}
const boton = (text) => w.findAll('button').find((b) => b.text().includes(text))
const hex = (k) =>
  w
    .findAll('.color-row')
    .find((r) => r.find('.color-name').text() === k)
    .find('.color-hex')

describe('editar un tema propio', () => {
  it('parte de los colores de ese tema y guarda en el', async () => {
    montar({ editing: 'propio-rojo' })
    expect(w.find('h3').text()).toBe('Editar tema')
    expect(hex('Acento').element.value).toBe('#aa0000')
    await hex('Acento').setValue('#bb1111')
    await boton('Guardar tema').trigger('click')
    expect(customThemes()['propio-rojo'].v.accent).toBe('#bb1111')
    expect(w.emitted('saved')[0]).toEqual(['propio-rojo'])
  })

  it('con el editor abierto, editar otro tema lo empieza con los colores de ese', async () => {
    // Se quedaban los colores del primero y «Guardar» se los escribia al
    // segundo: el azul acababa rojo.
    montar({ editing: 'propio-rojo' })
    await w.setProps({ editing: 'propio-azul' })
    await flushPromises()
    expect(w.find('input').element.value).toBe('Azul')
    expect(hex('Acento').element.value).toBe('#0000aa')
    await boton('Guardar tema').trigger('click')
    expect(customThemes()['propio-azul'].v.accent).toBe('#0000aa')
    expect(customThemes()['propio-rojo'].v.accent).toBe('#aa0000')
  })
})

describe('crear un tema', () => {
  it('parte del que esta puesto y se guarda con su nombre', async () => {
    montar({ editing: null, activeTheme: 'ocean' })
    expect(w.find('h3').text()).toBe('Crear tema custom')
    expect(w.find('input').element.value).toBe('Oceano (mío)')
    await w.find('input').setValue('Mi Mar')
    await boton('Guardar tema').trigger('click')
    const guardado = customThemes()['propio-mi-mar']
    expect(guardado.name).toBe('Mi Mar')
    expect(guardado.v.accent).toBe(CATALOG.ocean.v.accent)
    expect(raiz().dataset.theme).toBe('propio-mi-mar')
  })

  it('«Partir de» copia los colores de otro', async () => {
    montar({ editing: null })
    await w.find('.select-box').trigger('click')
    await w
      .findAll('.select-opt')
      .find((o) => o.text().includes('Vino'))
      .trigger('click')
    await flushPromises()
    expect(hex('Acento').element.value).toBe(CATALOG.wine.v.accent)
    expect(w.find('input').element.value).toBe('Vino (mío)')
  })

  it('los cambios se ven al momento, y cancelar deja el tema de antes', async () => {
    montar({ editing: null, activeTheme: 'night' })
    await hex('Acento').setValue('#123456')
    await flushPromises()
    expect(raiz().dataset.theme).toBe('__preview')
    expect(raiz().style.getPropertyValue('--accent')).toBe('#123456')
    await boton('Cancelar').trigger('click')
    expect(raiz().dataset.theme).toBe('night')
    expect(w.emitted('close')).toBeTruthy()
    expect(Object.keys(customThemes())).toEqual(['propio-rojo', 'propio-azul'])
  })

  it('por defecto enseña seis colores y, si se pide, los doce', async () => {
    montar({ editing: null })
    expect(w.findAll('.color-row')).toHaveLength(6)
    await boton('Todos los colores').trigger('click')
    expect(w.findAll('.color-row')).toHaveLength(12)
    await boton('Menos colores').trigger('click')
    expect(w.findAll('.color-row')).toHaveLength(6)
  })

  it('al cerrarse a medias no deja puesta la vista previa', async () => {
    montar({ editing: null, activeTheme: 'midnight' })
    await hex('Acento').setValue('#654321')
    await flushPromises()
    w.unmount()
    w = null
    expect(raiz().dataset.theme).toBe('midnight')
  })
})
