import { describe, it, expect } from 'vitest'
import { mount } from '@vue/test-utils'
import ModelPicker from '../src/components/ui/ModelPicker.vue'

// El selector de modelos: cuando no hay lista, dice por qué; y en el panel del
// chat la lista flota encima (no empuja a los selectores de al lado).
const open = async (w) => {
  await w.find('input').trigger('focus')
  return w.find('.model-menu')
}

describe('the model picker', () => {
  it('says why there is no list when the provider failed', async () => {
    const w = mount(ModelPicker, {
      props: { modelValue: '', models: [], why: 'la clave no es válida' }
    })
    const menu = await open(w)
    expect(menu.text()).toContain('no pude pedir la lista: la clave no es válida')
    w.unmount()
  })

  it('says how many were tried when none answered the tool check', async () => {
    const w = mount(ModelPicker, {
      props: {
        modelValue: '',
        models: [],
        unavailable: [
          { id: 'a', reason: 'saturado' },
          { id: 'b', reason: 'saturado' }
        ]
      }
    })
    const menu = await open(w)
    expect(menu.text()).toContain('ninguno de los 2 modelos contestó a la prueba')
    w.unmount()
  })

  it('keeps the plain hint when nothing is known', async () => {
    const w = mount(ModelPicker, { props: { modelValue: '', models: [] } })
    const menu = await open(w)
    expect(menu.text()).toContain('sin lista: escribe el nombre del modelo')
    w.unmount()
  })

  it('floats its list over what is below when asked to', async () => {
    const models = [{ id: 'gemini-x', name: 'Gemini X', tools: true, released: '2026-01-01' }]
    const floating = mount(ModelPicker, { props: { modelValue: '', models, floating: true } })
    await open(floating)
    expect(floating.find('.model-menu').classes()).toContain('floating')
    floating.unmount()
    const inFlow = mount(ModelPicker, { props: { modelValue: '', models } })
    await open(inFlow)
    expect(inFlow.find('.model-menu').classes()).not.toContain('floating')
    inFlow.unmount()
  })
})
