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
const mounted = []
const mountView = (c, opts) => {
  const w = mount(c, { attachTo: document.body, ...opts })
  mounted.push(w)
  return w
}
afterEach(() => {
  for (const w of mounted.splice(0)) w.unmount()
  document.body.innerHTML = ''
})
const press = (el, key, extra = {}) => {
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
const three = () => [song(1), song(2), song(3)]

describe('what each key means in a list', () => {
  it('Enter plays; Space pauses; Ctrl+Space selects; Shift+F10 and the menu key open the menu', () => {
    expect(songKey({ key: 'Enter' })).toEqual({ kind: 'play' })
    expect(songKey({ key: ' ' })).toEqual({ kind: 'toggle' })
    expect(songKey({ key: ' ', ctrlKey: true })).toEqual({ kind: 'select' })
    expect(songKey({ key: 'F10', shiftKey: true })).toEqual({ kind: 'menu' })
    expect(songKey({ key: 'F10' })).toBe(null)
    expect(songKey({ key: 'ContextMenu' })).toEqual({ kind: 'menu' })
    expect(songKey({ key: 'a' })).toBe(null)
    expect(songKey({ key: 'Enter', altKey: true })).toBe(null)
  })

  it('in a list, up and down; in a grid, also sideways and a whole line', () => {
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

  it('a move does not leave the list', () => {
    expect(targetIndex({ by: 5 }, 8, 10)).toBe(9)
    expect(targetIndex({ by: -5 }, 2, 10)).toBe(0)
    expect(targetIndex({ to: 'last' }, 2, 10)).toBe(9)
    expect(targetIndex({ by: 1 }, 0, 0)).toBe(-1)
  })
})

describe('the library is navigable with the keyboard', () => {
  const rows = (w) => w.findAll('[data-song-row]')

  it('the whole list is a single tab stop: the selected one, or the first', async () => {
    const w = mountView(SongTable, { props: { songs: three() } })
    expect(rows(w).map((f) => f.attributes('tabindex'))).toEqual(['0', '-1', '-1'])
    await w.setProps({ selected: 2 })
    expect(rows(w).map((f) => f.attributes('tabindex'))).toEqual(['-1', '0', '-1'])
    // los botones de dentro no son paradas: se llega con el menu de la fila
    expect(rows(w)[0].find('.row-play').attributes('tabindex')).toBe('-1')
    expect(rows(w)[0].find('.heart').attributes('tabindex')).toBe('-1')
  })

  it('arrow keys move to the neighbour, focus it and select it', async () => {
    const w = mountView(SongTable, { props: { songs: three(), selected: 1 } })
    rows(w)[0].element.focus()
    await press(rows(w)[0], 'ArrowDown')
    expect(document.activeElement).toBe(rows(w)[1].element)
    expect(w.emitted('select').at(-1)).toEqual([2, { shiftKey: false }])
    // con Mayus se amplia la seleccion (la app lo decide con el ancla)
    await press(rows(w)[1], 'ArrowDown', { shiftKey: true })
    expect(w.emitted('select').at(-1)).toEqual([3, { shiftKey: true }])
    // con Ctrl solo se mueve el foco
    const before = w.emitted('select').length
    await press(rows(w)[2], 'ArrowUp', { ctrlKey: true })
    expect(document.activeElement).toBe(rows(w)[1].element)
    expect(w.emitted('select')).toHaveLength(before)
    // y la que tiene el foco pasa a ser la parada del tabulador
    expect(rows(w)[1].attributes('tabindex')).toBe('0')
  })

  it('Enter plays it and Shift+F10 opens its menu', async () => {
    const w = mountView(SongTable, { props: { songs: three() } })
    await press(rows(w)[1], 'Enter')
    expect(w.emitted('play')[0][0].id).toBe(2)
    await press(rows(w)[1], 'F10', { shiftKey: true })
    const [where, which] = w.emitted('context')[0]
    expect(which.id).toBe(2)
    expect(where).toHaveProperty('clientX')
  })

  it('a key on an inner button belongs to that button, not to the row', async () => {
    const w = mountView(SongTable, { props: { songs: three() } })
    await press(rows(w)[0].find('.heart'), 'Enter')
    expect(w.emitted('play')).toBeFalsy()
  })

  it('Space, with nothing playing, plays the focused one', async () => {
    const w = mountView(SongRows, { props: { songs: three() } })
    await press(rows(w)[2], ' ')
    expect(w.emitted('play')[0][0].id).toBe(3)
  })

  it('in the grid, sideways also moves', async () => {
    const w = mountView(SongGrid, { props: { songs: three() } })
    rows(w)[0].element.focus()
    await press(rows(w)[0], 'ArrowRight')
    expect(document.activeElement).toBe(rows(w)[1].element)
    await press(rows(w)[1], 'End')
    expect(document.activeElement).toBe(rows(w)[2].element)
  })

  it('is announced as a song list, with which one is selected and its position', async () => {
    const table = mountView(SongTable, {
      props: { songs: three(), selected: 2, label: 'Favoritos' }
    })
    const t = table.find('table')
    expect(t.attributes('role')).toBe('grid')
    expect(t.attributes('aria-label')).toBe('Favoritos')
    expect(t.attributes('aria-multiselectable')).toBe('true')
    expect(rows(table).map((f) => f.attributes('aria-selected'))).toEqual([
      'false',
      'true',
      'false'
    ])
    expect(rows(table)[0].attributes('aria-rowindex')).toBe('2') // la 1 es la cabecera
    const list = mountView(SongRows, { props: { songs: three() } })
    expect(list.find('.rows').attributes('role')).toBe('listbox')
    expect(rows(list)[1].attributes('role')).toBe('option')
    expect(rows(list)[1].attributes('aria-posinset')).toBe('2')
    expect(rows(list)[1].attributes('aria-setsize')).toBe('3')
  })

  it('sortable headers are buttons', async () => {
    const w = mountView(SongTable, { props: { songs: three(), sort: 'title' } })
    const title = w.find('th[data-col="title"]')
    expect(title.attributes('aria-sort')).toBe('ascending')
    await title.find('button').trigger('click')
    expect(w.emitted('sortBy')[0]).toEqual(['title', false])
    // la columna sin texto tiene nombre para quien no la ve
    expect(w.find('th[data-col="fav"] .sr-only').text()).toBe('Favorita')
  })
})

describe('the stars with the keyboard', () => {
  it('are a slider from 0 to 5', async () => {
    const w = mountView(StarRating, { props: { value: 2 } })
    const s = w.find('.stars')
    expect(s.attributes('role')).toBe('slider')
    expect(s.attributes('aria-valuenow')).toBe('2')
    expect(s.attributes('aria-valuetext')).toBe('2 de 5')
    expect(s.attributes('tabindex')).toBe('0')
    await press(s, 'ArrowRight')
    await press(s, 'Home')
    await press(s, '4')
    expect(w.emitted('change').map((c) => c[0])).toEqual([3, 0, 4])
  })

  it('in a row they are not a tab stop, and in read-only mode they do not change', async () => {
    expect(
      mountView(StarRating, { props: { value: 1, focusable: false } })
        .find('.stars')
        .attributes('tabindex')
    ).toBe('-1')
    const ro = mountView(StarRating, { props: { value: 3, editable: false } })
    expect(ro.find('.stars').attributes('role')).toBe('img')
    expect(ro.find('.stars').attributes('aria-label')).toContain('3 de 5')
    await press(ro.find('.stars'), 'ArrowRight')
    expect(ro.emitted('change')).toBeFalsy()
  })
})

describe('the dropdown', () => {
  const options = [
    { v: 'a', n: 'Alfa' },
    { v: 'b', n: 'Beta' },
    { v: 'c', n: 'Gamma' }
  ]

  it('says it opens a listbox, whether it is open and what it is called', async () => {
    const w = mountView(SelectField, { props: { modelValue: 'b', options, label: 'Letra' } })
    const box = w.find('.select-box')
    expect(box.attributes('aria-haspopup')).toBe('listbox')
    expect(box.attributes('aria-expanded')).toBe('false')
    // su nombre es la etiqueta (y lo elegido)
    const name = box
      .attributes('aria-labelledby')
      .split(' ')
      .map((id) => document.getElementById(id).textContent.trim())
    expect(name).toEqual(['Letra', 'Beta'])
    await box.trigger('click')
    expect(box.attributes('aria-expanded')).toBe('true')
    const list = w.find('[role="listbox"]')
    expect(list.attributes('id')).toBe(box.attributes('aria-controls'))
    expect(w.findAll('[role="option"]').map((o) => o.attributes('aria-selected'))).toEqual([
      'false',
      'true',
      'false'
    ])
  })

  it('the option highlighted with the arrow keys is announced', async () => {
    const w = mountView(SelectField, { props: { modelValue: 'a', options, label: 'Letra' } })
    const box = w.find('.select-box')
    await press(box, 'ArrowDown') // abre
    await press(box, 'ArrowDown') // pasa a Beta
    const active = box.attributes('aria-activedescendant')
    expect(document.getElementById(active).textContent).toContain('Beta')
    await press(box, 'End')
    expect(document.getElementById(box.attributes('aria-activedescendant')).textContent).toContain(
      'Gamma'
    )
    await press(box, 'Enter')
    expect(w.emitted('update:modelValue').at(-1)).toEqual(['c'])
  })
})

describe('labelled fields', () => {
  it('the slider without a visible label (volume) carries its name on the control', () => {
    const w = mountView(SliderField, {
      props: { modelValue: 0.5 },
      attrs: { 'aria-label': 'Volumen' }
    })
    expect(w.find('input').attributes('aria-label')).toBe('Volumen')
    const withLabel = mountView(SliderField, {
      props: { modelValue: 0.5, label: 'Tamaño', valueText: '50 %' }
    })
    expect(withLabel.find('input').attributes('aria-label')).toBeUndefined()
    expect(withLabel.find('input').attributes('aria-valuetext')).toBe('50 %')
  })

  it('what is passed to a text field goes to the input; the class, to the wrapper', () => {
    const w = mountView(TextField, {
      props: { modelValue: '' },
      attrs: { 'aria-label': 'Buscar', class: 'nav-filter', role: 'combobox' }
    })
    expect(w.find('input').attributes('aria-label')).toBe('Buscar')
    expect(w.find('input').attributes('role')).toBe('combobox')
    expect(w.classes()).toContain('nav-filter')
    expect(w.find('input').classes()).not.toContain('nav-filter')
  })

  it('the switch toggles only once when its text is clicked', async () => {
    const w = mountView(ToggleField, { props: { modelValue: false, title: 'Convertir' } })
    await w.find('.toggle-txt strong').trigger('click')
    expect(w.emitted('update:modelValue')).toEqual([[true]])
  })
})

describe('the menu with the keyboard', () => {
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

  it('enters on the first option, arrow keys move through and Escape closes returning focus', async () => {
    const row = document.createElement('button')
    document.body.appendChild(row)
    row.focus()
    const w = mountView(ContextMenu, {
      props: { open: false, items: items(), title: 'Mi Gozo', keyboard: true }
    })
    await w.setProps({ open: true })
    await flushPromises()
    expect(w.find('[role="menu"]').exists()).toBe(true)
    expect(document.activeElement.textContent).toContain('Reproducir')
    await press(window, 'ArrowDown')
    expect(document.activeElement.textContent).toContain('Valorar')
    await press(window, 'ArrowRight') // abre el submenu
    expect(document.activeElement.textContent).toContain('5 estrellas')
    await press(window, 'ArrowLeft') // y vuelve
    expect(document.activeElement.textContent).toContain('Reproducir')
    await press(window, 'Escape')
    expect(w.emitted('close')).toBeTruthy()
    await w.setProps({ open: false })
    await flushPromises()
    expect(document.activeElement).toBe(row)
  })

  it('opened with the mouse, on close focus does not stay where it was clicked', async () => {
    const row = document.createElement('button')
    document.body.appendChild(row)
    row.focus()
    const w = mountView(ContextMenu, { props: { open: false, items: items(), keyboard: false } })
    await w.setProps({ open: true })
    await flushPromises()
    await w.setProps({ open: false })
    await flushPromises()
    expect(document.activeElement).not.toBe(row)
  })
})

describe('the side panel', () => {
  it('traps focus and Escape from inside closes it', async () => {
    const w = mountView(Drawer, {
      props: { open: false, title: 'Menu' },
      slots: { default: '<button class="dentro">x</button>' }
    })
    await w.setProps({ open: true })
    await flushPromises()
    await nextTick()
    const panel = document.querySelector('.drawer')
    expect(panel.contains(document.activeElement)).toBe(true)
    await press(document.activeElement, 'Escape')
    expect(w.emitted('close')).toBeTruthy()
  })
})

// La selección múltiple con el teclado: Ctrl+A elige todas, Supr quita (o
// manda a la papelera) las elegidas y Escape las suelta. Qué se hace lo
// decide la app; la lista solo lo dice.
describe('selection with the keyboard', () => {
  it('Ctrl+A selects all; with Shift or Alt, it does not', () => {
    expect(songKey({ key: 'a', ctrlKey: true })).toEqual({ kind: 'selectAll' })
    expect(songKey({ key: 'A', ctrlKey: true })).toEqual({ kind: 'selectAll' }) // con Bloq Mayús
    expect(songKey({ key: 'a', metaKey: true })).toEqual({ kind: 'selectAll' })
    expect(songKey({ key: 'a', ctrlKey: true, shiftKey: true })).toBe(null)
    expect(songKey({ key: 'a', ctrlKey: true, altKey: true })).toBe(null)
  })

  it('Delete alone removes; with Ctrl or Shift it does not', () => {
    expect(songKey({ key: 'Delete' })).toEqual({ kind: 'remove' })
    expect(songKey({ key: 'Delete', ctrlKey: true })).toBe(null)
    expect(songKey({ key: 'Delete', shiftKey: true })).toBe(null)
  })

  it('Escape clears only if there is something to clear; otherwise it is let through', () => {
    expect(songKey({ key: 'Escape' })).toBe(null)
    expect(songKey({ key: 'Escape' }, { selecting: true })).toEqual({ kind: 'clear' })
  })

  const rows = (w) => w.findAll('[data-song-row]')

  it('the focused row tells the app: selectAll, remove and clearSelection', async () => {
    for (const view of [SongTable, SongRows, SongGrid]) {
      const w = mountView(view, { props: { songs: three(), selected: 1, selectMode: true } })
      await press(rows(w)[0], 'a', { ctrlKey: true })
      await press(rows(w)[0], 'Delete')
      await press(rows(w)[0], 'Escape')
      expect(w.emitted('selectAll'), view.__name).toEqual([[{ toggle: false }]])
      expect(w.emitted('remove'), view.__name).toHaveLength(1)
      expect(w.emitted('clearSelection'), view.__name).toHaveLength(1)
    }
  })

  it('without the mode and with a single selected one, Escape does not belong to the list (it carries on)', async () => {
    const w = mountView(SongRows, { props: { songs: three(), selected: 1, selectedIds: [1] } })
    const outside = []
    const listener = (e) => outside.push(e.key)
    window.addEventListener('keydown', listener)
    await press(rows(w)[0], 'Escape')
    window.removeEventListener('keydown', listener)
    expect(w.emitted('clearSelection')).toBeFalsy()
    expect(outside, 'Escape debe llegar a quien cierre lo que haya abierto').toEqual(['Escape'])
    // con varias elegidas, sí
    await w.setProps({ selectedIds: [1, 2] })
    await press(rows(w)[0], 'Escape')
    expect(w.emitted('clearSelection')).toHaveLength(1)
  })

  it('in the mode, arrow keys only move focus (they do not tick on the way); Shift adds the range', async () => {
    const w = mountView(SongTable, { props: { songs: three(), selected: 1, selectMode: true } })
    rows(w)[0].element.focus()
    await press(rows(w)[0], 'ArrowDown')
    expect(document.activeElement).toBe(rows(w)[1].element)
    expect(w.emitted('select')).toBeFalsy()
    await press(rows(w)[1], 'ArrowDown', { shiftKey: true })
    expect(w.emitted('select')).toEqual([[3, { shiftKey: true }]])
    // y Ctrl+espacio marca la que tiene el foco
    await press(rows(w)[2], ' ', { ctrlKey: true })
    expect(w.emitted('select').at(-1)).toEqual([3, { ctrlKey: true }])
  })
})
