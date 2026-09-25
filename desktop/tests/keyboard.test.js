import { describe, it, expect, afterEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import { nextTick } from 'vue'
import { songKey, targetIndex } from '../src/utils/keys.js'
import SongTable from '../src/components/SongTable.vue'
import SongGrid from '../src/components/SongGrid.vue'
import SongRows from '../src/components/SongRows.vue'
import StarRating from '../src/components/StarRating.vue'
import SelectField from '../src/components/ui/SelectField.vue'
import SliderField from '../src/components/ui/SliderField.vue'
import TextField from '../src/components/ui/TextField.vue'
import ToggleField from '../src/components/ui/ToggleField.vue'
import ContextMenu from '../src/components/ui/ContextMenu.vue'
import Drawer from '../src/components/ui/Drawer.vue'

// Todo se puede hacer sin raton: recorrer la biblioteca, poner una cancion,
// abrir su menu, valorar, elegir en un desplegable. Y quien no ve la pantalla
// oye que es cada cosa y en que estado esta.
const montados = []
const montar = (c, opts) => {
  const w = mount(c, { attachTo: document.body, ...opts })
  montados.push(w)
  return w
}
afterEach(() => {
  for (const w of montados.splice(0)) w.unmount()
  document.body.innerHTML = ''
})
const tecla = (el, key, extra = {}) => {
  const target = el.element || el
  target.dispatchEvent(
    new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true, ...extra })
  )
  return flushPromises()
}
const song = (id) => ({
  id,
  title: 'Cancion ' + id,
  artist: 'Barak',
  duration: 100,
  stars: 0,
  favorite: 0
})
const tres = () => [song(1), song(2), song(3)]

describe('que quiere decir cada tecla en una lista', () => {
  it('Enter pone; espacio pausa; Ctrl+espacio elige; Mayus+F10 y la tecla de menu abren el menu', () => {
    expect(songKey({ key: 'Enter' })).toEqual({ kind: 'play' })
    expect(songKey({ key: ' ' })).toEqual({ kind: 'toggle' })
    expect(songKey({ key: ' ', ctrlKey: true })).toEqual({ kind: 'select' })
    expect(songKey({ key: 'F10', shiftKey: true })).toEqual({ kind: 'menu' })
    expect(songKey({ key: 'F10' })).toBe(null)
    expect(songKey({ key: 'ContextMenu' })).toEqual({ kind: 'menu' })
    expect(songKey({ key: 'a' })).toBe(null)
    expect(songKey({ key: 'Enter', altKey: true })).toBe(null)
  })

  it('en una lista, arriba y abajo; en cuadricula, tambien los lados y una linea entera', () => {
    expect(songKey({ key: 'ArrowDown' })).toEqual({ kind: 'move', by: 1 })
    expect(songKey({ key: 'ArrowLeft' })).toBe(null)
    expect(songKey({ key: 'ArrowDown' }, { grid: true, columns: 4 })).toEqual({
      kind: 'move',
      by: 4
    })
    expect(songKey({ key: 'ArrowLeft' }, { grid: true, columns: 4 })).toEqual({
      kind: 'move',
      by: -1
    })
    expect(songKey({ key: 'PageDown' })).toEqual({ kind: 'move', by: 10 })
    expect(songKey({ key: 'Home' })).toEqual({ kind: 'move', to: 'first' })
  })

  it('un movimiento no se sale de la lista', () => {
    expect(targetIndex({ by: 5 }, 8, 10)).toBe(9)
    expect(targetIndex({ by: -5 }, 2, 10)).toBe(0)
    expect(targetIndex({ to: 'last' }, 2, 10)).toBe(9)
    expect(targetIndex({ by: 1 }, 0, 0)).toBe(-1)
  })
})

describe('la biblioteca se recorre con el teclado', () => {
  const filas = (w) => w.findAll('[data-song-row]')

  it('toda la lista es una sola parada del tabulador: la elegida, o la primera', async () => {
    const w = montar(SongTable, { props: { songs: tres() } })
    expect(filas(w).map((f) => f.attributes('tabindex'))).toEqual(['0', '-1', '-1'])
    await w.setProps({ selected: 2 })
    expect(filas(w).map((f) => f.attributes('tabindex'))).toEqual(['-1', '0', '-1'])
    // los botones de dentro no son paradas: se llega con el menu de la fila
    expect(filas(w)[0].find('.row-play').attributes('tabindex')).toBe('-1')
    expect(filas(w)[0].find('.heart').attributes('tabindex')).toBe('-1')
  })

  it('las flechas pasan a la vecina, le dan el foco y la eligen', async () => {
    const w = montar(SongTable, { props: { songs: tres(), selected: 1 } })
    filas(w)[0].element.focus()
    await tecla(filas(w)[0], 'ArrowDown')
    expect(document.activeElement).toBe(filas(w)[1].element)
    expect(w.emitted('select').at(-1)).toEqual([2, { shiftKey: false }])
    // con Mayus se amplia la seleccion (la app lo decide con el ancla)
    await tecla(filas(w)[1], 'ArrowDown', { shiftKey: true })
    expect(w.emitted('select').at(-1)).toEqual([3, { shiftKey: true }])
    // con Ctrl solo se mueve el foco
    const antes = w.emitted('select').length
    await tecla(filas(w)[2], 'ArrowUp', { ctrlKey: true })
    expect(document.activeElement).toBe(filas(w)[1].element)
    expect(w.emitted('select')).toHaveLength(antes)
    // y la que tiene el foco pasa a ser la parada del tabulador
    expect(filas(w)[1].attributes('tabindex')).toBe('0')
  })

  it('Enter la pone a sonar y Mayus+F10 abre su menu', async () => {
    const w = montar(SongTable, { props: { songs: tres() } })
    await tecla(filas(w)[1], 'Enter')
    expect(w.emitted('play')[0][0].id).toBe(2)
    await tecla(filas(w)[1], 'F10', { shiftKey: true })
    const [donde, cual] = w.emitted('context')[0]
    expect(cual.id).toBe(2)
    expect(donde).toHaveProperty('clientX')
  })

  it('una tecla sobre un boton de dentro es de ese boton, no de la fila', async () => {
    const w = montar(SongTable, { props: { songs: tres() } })
    await tecla(filas(w)[0].find('.heart'), 'Enter')
    expect(w.emitted('play')).toBeFalsy()
  })

  it('espacio, sin nada sonando, pone la que tiene el foco', async () => {
    const w = montar(SongRows, { props: { songs: tres() } })
    await tecla(filas(w)[2], ' ')
    expect(w.emitted('play')[0][0].id).toBe(3)
  })

  it('en la cuadricula los lados tambien mueven', async () => {
    const w = montar(SongGrid, { props: { songs: tres() } })
    filas(w)[0].element.focus()
    await tecla(filas(w)[0], 'ArrowRight')
    expect(document.activeElement).toBe(filas(w)[1].element)
    await tecla(filas(w)[1], 'End')
    expect(document.activeElement).toBe(filas(w)[2].element)
  })

  it('se anuncia como lista de canciones, con cual esta elegida y en que puesto va', async () => {
    const tabla = montar(SongTable, { props: { songs: tres(), selected: 2, label: 'Favoritos' } })
    const t = tabla.find('table')
    expect(t.attributes('role')).toBe('grid')
    expect(t.attributes('aria-label')).toBe('Favoritos')
    expect(t.attributes('aria-multiselectable')).toBe('true')
    expect(filas(tabla).map((f) => f.attributes('aria-selected'))).toEqual([
      'false',
      'true',
      'false'
    ])
    expect(filas(tabla)[0].attributes('aria-rowindex')).toBe('2') // la 1 es la cabecera
    const lista = montar(SongRows, { props: { songs: tres() } })
    expect(lista.find('.rows').attributes('role')).toBe('listbox')
    expect(filas(lista)[1].attributes('role')).toBe('option')
    expect(filas(lista)[1].attributes('aria-posinset')).toBe('2')
    expect(filas(lista)[1].attributes('aria-setsize')).toBe('3')
  })

  it('las cabeceras que ordenan son botones', async () => {
    const w = montar(SongTable, { props: { songs: tres(), sort: 'title' } })
    const titulo = w.find('th[data-col="title"]')
    expect(titulo.attributes('aria-sort')).toBe('ascending')
    await titulo.find('button').trigger('click')
    expect(w.emitted('sortBy')[0]).toEqual(['title', false])
    // la columna sin texto tiene nombre para quien no la ve
    expect(w.find('th[data-col="fav"] .sr-only').text()).toBe('Favorita')
  })
})

describe('las estrellas con el teclado', () => {
  it('son un deslizador de 0 a 5', async () => {
    const w = montar(StarRating, { props: { value: 2 } })
    const s = w.find('.stars')
    expect(s.attributes('role')).toBe('slider')
    expect(s.attributes('aria-valuenow')).toBe('2')
    expect(s.attributes('aria-valuetext')).toBe('2 de 5')
    expect(s.attributes('tabindex')).toBe('0')
    await tecla(s, 'ArrowRight')
    await tecla(s, 'Home')
    await tecla(s, '4')
    expect(w.emitted('change').map((c) => c[0])).toEqual([3, 0, 4])
  })

  it('en una fila no son parada del tabulador, y en solo lectura no cambian', async () => {
    expect(
      montar(StarRating, { props: { value: 1, focusable: false } })
        .find('.stars')
        .attributes('tabindex')
    ).toBe('-1')
    const ro = montar(StarRating, { props: { value: 3, editable: false } })
    expect(ro.find('.stars').attributes('role')).toBe('img')
    expect(ro.find('.stars').attributes('aria-label')).toContain('3 de 5')
    await tecla(ro.find('.stars'), 'ArrowRight')
    expect(ro.emitted('change')).toBeFalsy()
  })
})

describe('el desplegable', () => {
  const options = [
    { v: 'a', n: 'Alfa' },
    { v: 'b', n: 'Beta' },
    { v: 'c', n: 'Gamma' }
  ]

  it('dice que abre una lista, si esta abierta y como se llama', async () => {
    const w = montar(SelectField, { props: { modelValue: 'b', options, label: 'Letra' } })
    const caja = w.find('.select-box')
    expect(caja.attributes('aria-haspopup')).toBe('listbox')
    expect(caja.attributes('aria-expanded')).toBe('false')
    // su nombre es la etiqueta (y lo elegido)
    const nombre = caja
      .attributes('aria-labelledby')
      .split(' ')
      .map((id) => document.getElementById(id).textContent.trim())
    expect(nombre).toEqual(['Letra', 'Beta'])
    await caja.trigger('click')
    expect(caja.attributes('aria-expanded')).toBe('true')
    const lista = w.find('[role="listbox"]')
    expect(lista.attributes('id')).toBe(caja.attributes('aria-controls'))
    expect(w.findAll('[role="option"]').map((o) => o.attributes('aria-selected'))).toEqual([
      'false',
      'true',
      'false'
    ])
  })

  it('la opcion resaltada con las flechas se anuncia', async () => {
    const w = montar(SelectField, { props: { modelValue: 'a', options, label: 'Letra' } })
    const caja = w.find('.select-box')
    await tecla(caja, 'ArrowDown') // abre
    await tecla(caja, 'ArrowDown') // pasa a Beta
    const activa = caja.attributes('aria-activedescendant')
    expect(document.getElementById(activa).textContent).toContain('Beta')
    await tecla(caja, 'End')
    expect(document.getElementById(caja.attributes('aria-activedescendant')).textContent).toContain(
      'Gamma'
    )
    await tecla(caja, 'Enter')
    expect(w.emitted('update:modelValue').at(-1)).toEqual(['c'])
  })
})

describe('campos con nombre', () => {
  it('el deslizador sin etiqueta a la vista (el volumen) lleva su nombre en el control', () => {
    const w = montar(SliderField, {
      props: { modelValue: 0.5 },
      attrs: { 'aria-label': 'Volumen' }
    })
    expect(w.find('input').attributes('aria-label')).toBe('Volumen')
    const conEtiqueta = montar(SliderField, {
      props: { modelValue: 0.5, label: 'Tamaño', valueText: '50 %' }
    })
    expect(conEtiqueta.find('input').attributes('aria-label')).toBeUndefined()
    expect(conEtiqueta.find('input').attributes('aria-valuetext')).toBe('50 %')
  })

  it('lo que se le pasa a un campo de texto va al campo; la clase, a la caja', () => {
    const w = montar(TextField, {
      props: { modelValue: '' },
      attrs: { 'aria-label': 'Buscar', class: 'nav-filter', role: 'combobox' }
    })
    expect(w.find('input').attributes('aria-label')).toBe('Buscar')
    expect(w.find('input').attributes('role')).toBe('combobox')
    expect(w.classes()).toContain('nav-filter')
    expect(w.find('input').classes()).not.toContain('nav-filter')
  })

  it('el interruptor cambia una sola vez al pulsar su texto', async () => {
    const w = montar(ToggleField, { props: { modelValue: false, title: 'Convertir' } })
    await w.find('.toggle-txt strong').trigger('click')
    expect(w.emitted('update:modelValue')).toEqual([[true]])
  })
})

describe('el menu con el teclado', () => {
  const items = () => [
    { label: 'Reproducir', action: () => {} },
    {
      label: 'Valorar',
      children: [
        { label: '5 estrellas', action: () => {} },
        { label: 'Sin valorar', action: () => {} }
      ]
    },
    { separator: true },
    { label: 'Borrar', disabled: true }
  ]

  it('entra en la primera opcion, las flechas recorren y Escape cierra devolviendo el foco', async () => {
    const fila = document.createElement('button')
    document.body.appendChild(fila)
    fila.focus()
    const w = montar(ContextMenu, {
      props: { open: false, items: items(), title: 'Mi Gozo', keyboard: true }
    })
    await w.setProps({ open: true })
    await flushPromises()
    expect(w.find('[role="menu"]').exists()).toBe(true)
    expect(document.activeElement.textContent).toContain('Reproducir')
    await tecla(window, 'ArrowDown')
    expect(document.activeElement.textContent).toContain('Valorar')
    await tecla(window, 'ArrowRight') // abre el submenu
    expect(document.activeElement.textContent).toContain('5 estrellas')
    await tecla(window, 'ArrowLeft') // y vuelve
    expect(document.activeElement.textContent).toContain('Reproducir')
    await tecla(window, 'Escape')
    expect(w.emitted('close')).toBeTruthy()
    await w.setProps({ open: false })
    await flushPromises()
    expect(document.activeElement).toBe(fila)
  })

  it('abierto con el raton, al cerrar no se queda el foco donde se pulso', async () => {
    const fila = document.createElement('button')
    document.body.appendChild(fila)
    fila.focus()
    const w = montar(ContextMenu, { props: { open: false, items: items(), keyboard: false } })
    await w.setProps({ open: true })
    await flushPromises()
    await w.setProps({ open: false })
    await flushPromises()
    expect(document.activeElement).not.toBe(fila)
  })
})

describe('el panel lateral', () => {
  it('atrapa el foco y Escape desde dentro lo cierra', async () => {
    const w = montar(Drawer, {
      props: { open: false, title: 'Menu' },
      slots: { default: '<button class="dentro">x</button>' }
    })
    await w.setProps({ open: true })
    await flushPromises()
    await nextTick()
    const panel = document.querySelector('.drawer')
    expect(panel.contains(document.activeElement)).toBe(true)
    await tecla(document.activeElement, 'Escape')
    expect(w.emitted('close')).toBeTruthy()
  })
})
