import { describe, it, expect, beforeEach, vi } from 'vitest'

// doble del backend. vi.hoisted porque vi.mock se iza al principio del fichero
const { api, pickImage } = vi.hoisted(() => ({
  api: {
    edit: vi.fn(), setStars: vi.fn(), toggleFavorite: vi.fn(), setCover: vi.fn(),
    enrich: vi.fn(), details: vi.fn(), autofill: vi.fn(), transpose: vi.fn(),
    coverUrl: (id) => '/c/' + id, coverUrlAlt: () => null
  },
  pickImage: vi.fn(async () => null)
}))
vi.mock('../src/api.js', () => ({ api, pickImage }))
beforeEach(() => vi.clearAllMocks())
import { mount, flushPromises } from '@vue/test-utils'
import DetailsPanel from '../src/components/DetailsPanel.vue'
import { readFileSync, readdirSync } from 'node:fs'
import TextField from '../src/components/ui/TextField.vue'
import StarRating from '../src/components/StarRating.vue'
import SelectField from '../src/components/ui/SelectField.vue'
import ToggleField from '../src/components/ui/ToggleField.vue'
import SliderField from '../src/components/ui/SliderField.vue'
import ColorField from '../src/components/ui/ColorField.vue'
import Card from '../src/components/ui/Card.vue'

describe('Campo', () => {
  it('emite lo que se escribe', async () => {
    const w = mount(TextField, { props: { modelValue: '' } })
    await w.find('input').setValue('barak')
    expect(w.emitted('update:modelValue').at(-1)).toEqual(['barak'])
  })
  it('el boton de limpiar vacia el valor', async () => {
    const w = mount(TextField, { props: { modelValue: 'algo' } })
    await w.find('.field-btn').trigger('click')
    expect(w.emitted('update:modelValue').at(-1)).toEqual([''])
  })
  it('las claves se ocultan y se pueden mostrar', async () => {
    const w = mount(TextField, { props: { modelValue: 'secreto', type: 'password' } })
    expect(w.find('input').attributes('type')).toBe('password')
    await w.find('.field-btn').trigger('click')
    expect(w.find('input').attributes('type')).toBe('text')
  })
  it('marca el foco para poder estilarlo', async () => {
    const w = mount(TextField, { props: { modelValue: '' } })
    await w.find('input').trigger('focus')
    expect(w.classes()).toContain('focused')
    await w.find('input').trigger('blur')
    expect(w.classes()).not.toContain('focused')
  })
  it('muestra el error cuando lo hay', () => {
    const w = mount(TextField, { props: { modelValue: '', error: 'ruta invalida' } })
    expect(w.classes()).toContain('error')
    expect(w.text()).toContain('ruta invalida')
  })
  it('avisa al pulsar Enter', async () => {
    const w = mount(TextField, { props: { modelValue: 'x' } })
    await w.find('input').trigger('keyup.enter')
    expect(w.emitted('enter')).toBeTruthy()
  })
})

describe('Selector', () => {
  const options = [{ v: 'a', n: 'Alfa' }, { v: 'b', n: 'Beta' }, { v: 'c', n: 'Gamma' }]
  const montar = (value = 'a') => mount(SelectField, { props: { modelValue: value, options } })

  it('muestra la opcion elegida', () => {
    expect(montar('b').find('.select-text').text()).toBe('Beta')
  })
  it('abre y cierra el menu', async () => {
    const w = montar()
    expect(w.find('.select-menu').exists()).toBe(false)
    await w.find('.select-box').trigger('click')
    expect(w.find('.select-menu').exists()).toBe(true)
    expect(w.findAll('.select-opt')).toHaveLength(3)
    await w.find('.select-box').trigger('click')
    expect(w.find('.select-menu').exists()).toBe(false)
  })
  it('elegir una opcion la emite y cierra', async () => {
    const w = montar()
    await w.find('.select-box').trigger('click')
    await w.findAll('.select-opt')[2].trigger('click')
    expect(w.emitted('update:modelValue').at(-1)).toEqual(['c'])
    expect(w.find('.select-menu').exists()).toBe(false)
  })
  it('se maneja con el teclado', async () => {
    const w = montar('a')
    await w.find('.select-box').trigger('keydown', { key: 'ArrowDown' })
    expect(w.find('.select-menu').exists()).toBe(true)
    await w.find('.select-box').trigger('keydown', { key: 'ArrowDown' })
    await w.find('.select-box').trigger('keydown', { key: 'Enter' })
    expect(w.emitted('update:modelValue').at(-1)).toEqual(['b'])
  })
  it('Escape cierra sin elegir', async () => {
    const w = montar()
    await w.find('.select-box').trigger('click')
    await w.find('.select-box').trigger('keydown', { key: 'Escape' })
    expect(w.find('.select-menu').exists()).toBe(false)
    expect(w.emitted('update:modelValue')).toBeFalsy()
  })
  it('marca cual esta activa', async () => {
    const w = montar('b')
    await w.find('.select-box').trigger('click')
    expect(w.findAll('.select-opt')[1].classes()).toContain('current')
  })
})

describe('Casilla', () => {
  it('alterna al pulsar', async () => {
    const w = mount(ToggleField, { props: { modelValue: false, title: 'Convertir' } })
    await w.find('.toggle-track').trigger('click')
    expect(w.emitted('update:modelValue').at(-1)).toEqual([true])
  })
  it('refleja el estado encendido', () => {
    const w = mount(ToggleField, { props: { modelValue: true } })
    expect(w.find('.toggle-track').classes()).toContain('on')
    expect(w.find('.toggle-track').attributes('aria-checked')).toBe('true')
  })
  it('el texto tambien alterna', async () => {
    const w = mount(ToggleField, { props: { modelValue: false, title: 'X' } })
    await w.find('.toggle-txt').trigger('click')
    expect(w.emitted('update:modelValue')).toBeTruthy()
  })
})

describe('Deslizador', () => {
  it('calcula el porcentaje de relleno', () => {
    const w = mount(SliderField, { props: { modelValue: 5, min: 0, max: 10 } })
    expect(w.find('.slider-track').attributes('style')).toContain('50%')
  })
  it('emite numeros, no cadenas', async () => {
    const w = mount(SliderField, { props: { modelValue: 0, min: 0, max: 10, step: 1 } })
    await w.find('input').setValue('7')
    expect(w.emitted('update:modelValue').at(-1)).toEqual([7])
  })
})

describe('Color', () => {
  it('emite tanto desde la muestra como desde el hex', async () => {
    const w = mount(ColorField, { props: { modelValue: '#ff0000', title: 'Acento' } })
    await w.find('input[type="color"]').setValue('#00ff00')
    expect(w.emitted('update:modelValue').at(-1)).toEqual(['#00ff00'])
    await w.find('.color-hex').setValue('#0000ff')
    expect(w.emitted('update:modelValue').at(-1)).toEqual(['#0000ff'])
  })
})

describe('nada de controles nativos sueltos', () => {
  const vistas = [
    ...readdirSync('src/components').filter(f => f.endsWith('.vue')).map(f => 'src/components/' + f),
    'src/App.vue'
  ]
  it('no hay select, checkbox, range ni color fuera de la libreria', () => {
    const malos = []
    for (const f of vistas) {
      const txt = readFileSync(f, 'utf8')
      for (const pattern of [/<select[\s>]/, /<input[^>]*type="checkbox"/,
                            /<input[^>]*type="range"/, /<input[^>]*type="color"/]) {
        if (pattern.test(txt)) malos.push(`${f}: ${pattern}`)
      }
    }
    expect(malos, 'usa los componentes de ui/: ' + malos.join(', ')).toEqual([])
  })
  it('no hay inputs de texto sin envolver', () => {
    const malos = []
    for (const f of vistas) {
      if (/<input(?![^>]*type="(checkbox|range|color)")/.test(readFileSync(f, 'utf8'))) malos.push(f)
    }
    expect(malos, 'usa TextField.vue: ' + malos.join(', ')).toEqual([])
  })
})

// Lo que se usa en varios sitios vive en components/ui/ y se reutiliza. Estas
// pruebas existen porque la caratula con su hueco estaba escrita tres veces
// (una de ellas manipulando el DOM a mano, con una clase que ya no existia) y
// la rejilla se descuadraba cuando una cancion no tenia portada.
describe('lo repetido vive en un solo componente', () => {
  const vistas = [
    ...readdirSync('src/components').filter(f => f.endsWith('.vue')).map(f => 'src/components/' + f),
    'src/App.vue'
  ]
  const leer = (f) => readFileSync(f, 'utf8')

  it('nadie monta la caratula a mano: se usa CoverArt', () => {
    const malos = vistas.filter(f => /<img[^>]*coverUrl/.test(leer(f)))
    expect(malos, 'usa ui/CoverArt.vue: ' + malos.join(', ')).toEqual([])
  })

  it('nadie sustituye nodos del DOM a mano', () => {
    const malos = vistas.filter(f => /replaceWith\(|createElement\(/.test(leer(f)))
    expect(malos, 'deja que Vue pinte: ' + malos.join(', ')).toEqual([])
  })

  it('el girito se pinta con Loading, no suelto', () => {
    const malos = vistas.filter(f =>
      /class="spinner"/.test(leer(f)) && !/chat-downloading/.test(leer(f)))
    expect(malos, 'usa ui/Loading.vue: ' + malos.join(', ')).toEqual([])
  })

  it('los huecos vacios se pintan con EmptyState', () => {
    const malos = vistas.filter(f => /<div[^>]*class="empty"/.test(leer(f)))
    expect(malos, 'usa ui/EmptyState.vue: ' + malos.join(', ')).toEqual([])
  })

  it('las tarjetas con titulo se pintan con Card', () => {
    const malos = vistas.filter(f => /<div class="card"[^>]*>\s*\n\s*<h3/.test(leer(f)))
    expect(malos, 'usa ui/Card.vue: ' + malos.join(', ')).toEqual([])
  })

  it('CoverArt es el unico que sabe de portadas que fallan', () => {
    const fuera = vistas.filter(f => !f.endsWith('CoverArt.vue'))
      .filter(f => /coverUrlAlt/.test(leer(f)))
    expect(fuera, 'el respaldo de URL vive en CoverArt: ' + fuera.join(', ')).toEqual([])
  })
})

// Estaba al reves: al pasar el raton por la 3ª se pintaban la 3, 4 y 5. La
// causa es que CSS solo sabe seleccionar hermanos POSTERIORES (`~`), asi que
// el marcado va 5..1 y el contenedor lo endereza con row-reverse.
describe('valoracion con estrellas', () => {
  const montarEstrellas = (props = {}) =>
    mount(StarRating, { props: { value: 0, ...props } })

  it('el marcado va del 5 al 1', () => {
    const w = montarEstrellas({ value: 0 })
    const titulos = w.findAll('.ico').map(i => i.attributes('title'))
    expect(titulos).toEqual(['5 de 5', '4 de 5', '3 de 5', '2 de 5', '1 de 5'])
  })

  it('con 3 estrellas se marcan la 1, la 2 y la 3', () => {
    const w = montarEstrellas({ value: 3 })
    const iconos = w.findAll('.ico')
    // el marcado va 5,4,3,2,1: las llenas son las tres ultimas
    const llenas = iconos.map(i => i.classes().includes('on'))
    expect(llenas).toEqual([false, false, true, true, true])
    // la de la nota actual ofrece quitarla; las de debajo, ponerse
    expect(iconos[2].attributes('title')).toBe('Quitar valoracion')
    expect(iconos[3].attributes('title')).toBe('2 de 5')
  })

  it('pulsar una estrella manda su valor, no el del indice', async () => {
    const w = montarEstrellas({ value: 0 })
    const cuarta = w.findAll('.ico').find(i => i.attributes('title') === '4 de 5')
    await cuarta.trigger('click')
    expect(w.emitted('change')[0]).toEqual([4])
  })

  it('pulsar la nota actual la quita', async () => {
    const w = montarEstrellas({ value: 3 })
    const tercera = w.findAll('.ico').find(i => i.attributes('title') === 'Quitar valoracion')
    await tercera.trigger('click')
    expect(w.emitted('change')[0]).toEqual([0])
  })

  it('en solo lectura no emite nada', async () => {
    const w = montarEstrellas({ value: 2, editable: false })
    await w.findAll('.ico')[0].trigger('click')
    expect(w.emitted('change')).toBeFalsy()
  })
})

// Editar la ficha escribe en las ETIQUETAS del mp3, no en la base: la base es
// solo un indice reconstruible. Estas pruebas fijan las reglas de la edicion.
describe('editar la ficha de una cancion', () => {
  const cancion = {
    id: 7, title: 'Mi Gozo', artist: 'Barak', album: '', year: '', genre: '',
    feat: '', key: '', bpm: 0, lyrics: '', stars: 0, favorite: 0,
    duration: 200, bitrate: 128000, folder: 'Artistas/Barak', file: 'x.mp3'
  }
  const montar = () => mount(DetailsPanel, { props: { song: { ...cancion }, aiReady: true } })
  const abrirEdicion = async (w) => {
    await w.find('.icon-btn').trigger('click')
    return w
  }

  it('el lapiz abre el formulario', async () => {
    const w = montar()
    expect(w.find('.edit-form').exists()).toBe(false)
    await abrirEdicion(w)
    expect(w.find('.edit-form').exists()).toBe(true)
  })

  it('sin cambios solo se ofrece cancelar', async () => {
    const w = await abrirEdicion(montar())
    const textos = w.findAll('.edit-actions .btn').map(b => b.text())
    expect(textos).toEqual(['Cancelar'])
    expect(w.find('.edit-state').text()).toBe('Sin cambios')
  })

  it('al tocar algo aparece guardar y dice cuantos campos', async () => {
    const w = await abrirEdicion(montar())
    await w.findAll('.edit-form input')[3].setValue('Generacion Radical')  // album
    const textos = w.findAll('.edit-actions .btn').map(b => b.text())
    expect(textos).toContain('Guardar')
    expect(textos).toContain('Descartar')
    expect(w.find('.edit-state').text()).toBe('1 sin guardar')
  })

  it('volver al valor original vuelve a dejarlo sin cambios', async () => {
    const w = await abrirEdicion(montar())
    const album = w.findAll('.edit-form input')[3]
    await album.setValue('Algo')
    expect(w.find('.edit-state').text()).toBe('1 sin guardar')
    await album.setValue('')
    expect(w.find('.edit-state').text()).toBe('Sin cambios')
    expect(w.findAll('.edit-actions .btn').map(b => b.text())).toEqual(['Cancelar'])
  })

  it('Escape cierra sin guardar', async () => {
    const w = await abrirEdicion(montar())
    await w.findAll('.edit-form input')[3].setValue('Algo')
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }))
    await w.vm.$nextTick()
    expect(w.find('.edit-form').exists()).toBe(false)
    expect(api.edit).not.toHaveBeenCalled()
  })

  it('guardar manda SOLO lo que cambio', async () => {
    api.edit.mockResolvedValueOnce({ ...cancion, album: 'Generacion Radical' })
    const w = await abrirEdicion(montar())
    await w.findAll('.edit-form input')[3].setValue('Generacion Radical')
    // jsdom no propaga el envio del formulario al pulsar el boton submit
    await w.find('.edit-form').trigger('submit')
    await flushPromises()
    expect(api.edit).toHaveBeenCalledWith(7, { album: 'Generacion Radical' })
  })

  it('el bpm se manda como numero, no como texto', async () => {
    api.edit.mockResolvedValueOnce({ ...cancion, bpm: 128 })
    const w = await abrirEdicion(montar())
    const campos = w.findAll('.edit-form input')
    await campos[campos.length - 1].setValue('128')      // bpm es el ultimo input
    await w.find('.edit-form').trigger('submit')
    await flushPromises()
    expect(api.edit).toHaveBeenCalledWith(7, { bpm: 128 })
  })

  it('cambiar de cancion cierra la edicion', async () => {
    const w = await abrirEdicion(montar())
    await w.setProps({ song: { ...cancion, id: 8, title: 'Otra' } })
    expect(w.find('.edit-form').exists()).toBe(false)
  })
})

// La tarjeta estaba escrita a mano veinte veces. Al unificarla, el hueco del
// titulo trae su propio <h3> (hay titulos con icono o con una cuenta al lado),
// asi que lo que hay que vigilar es que no acaben anidados.
describe('tarjeta', () => {
  it('pinta el titulo y la nota', () => {
    const w = mount(Card, { props: { title: 'Apariencia', note: 'Se aplica al instante' } })
    expect(w.find('h3').text()).toBe('Apariencia')
    expect(w.find('.note').text()).toBe('Se aplica al instante')
  })

  it('sin titulo ni nota no deja huecos vacios', () => {
    const w = mount(Card, { slots: { default: '<p>algo</p>' } })
    expect(w.find('h3').exists()).toBe(false)
    expect(w.find('.note').exists()).toBe(false)
    expect(w.text()).toBe('algo')
  })

  it('un titulo con marcado propio no se anida dentro de otro', () => {
    const w = mount(Card, { slots: { title: '<h3>Historial <span class="chip">4</span></h3>' } })
    expect(w.findAll('h3'), 'el <h3> quedo dentro de otro <h3>').toHaveLength(1)
    expect(w.find('h3 .chip').text()).toBe('4')
  })

  it('conserva las clases y estilos que le pasen', () => {
    const w = mount(Card, { attrs: { class: 'dl-dup' }, props: { title: 'x' } })
    expect(w.classes()).toContain('card')
    expect(w.classes()).toContain('dl-dup')
  })
})
