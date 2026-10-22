// La barra de la selección múltiple (SelectionBar.vue) y lo que hace con las
// elegidas (las acciones de useSongMenus que comparte con el menú de grupo):
// el contador, qué botones salen y cuándo, el menú de las listas, Telegram
// solo si está, quitar de la lista solo en un repertorio, y los tamaños.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import { ref } from 'vue'

const held = vi.hoisted(() => ({ state: null, api: null, app: null }))
vi.mock('../src/api.js', async (importOriginal) => {
  const actual = await importOriginal()
  const { createState, buildApiDouble, createAppDouble } = await import('./support/backend.js')
  held.state = createState()
  held.api = buildApiDouble(actual, held.state)
  held.app = createAppDouble()
  return { ...actual, api: held.api, app: held.app }
})

import SelectionBar from '../src/components/SelectionBar.vue'
import { useSongMenus } from '../src/composables/useSongMenus.js'
import { useSelection } from '../src/composables/useSelection.js'
import { useContextMenu, closeMenu } from '../src/composables/useContextMenu.js'
import { useDialog, dialogCancel } from '../src/composables/useDialog.js'
import { useNotices, clearNotices } from '../src/composables/useNotices.js'
import { song } from './support/backend.js'
import { allCss } from './support/css.js'

const { menu } = useContextMenu()
const { dialog } = useDialog()
const { notices } = useNotices()

const mounted = []
const mountView = (props = {}) => {
  const w = mount(SelectionBar, { props: { count: 2, ...props }, attachTo: document.body })
  mounted.push(w)
  return w
}
afterEach(() => {
  for (const w of mounted.splice(0)) w.unmount()
  document.body.innerHTML = ''
})
beforeEach(() => {
  closeMenu()
  dialogCancel()
  clearNotices()
  vi.clearAllMocks()
})
const button = (w, act) => w.find(`[data-act="${act}"]`)
const labels = () => menu.value.items.filter((i) => !i.separator).map((i) => i.label)

describe('the counter', () => {
  it('says how many, in singular or plural, and is announced on change', async () => {
    const w = mountView({ count: 3 })
    const status = w.find('.sel-status')
    expect(status.attributes('aria-live')).toBe('polite')
    expect(status.text()).toBe('3 seleccionadas')
    await w.setProps({ count: 1 })
    expect(status.text()).toBe('1 seleccionada')
    await w.setProps({ count: 0 })
    expect(status.text()).toBe('Elige canciones')
  })

  it('is a named toolbar', () => {
    const w = mountView()
    expect(w.attributes('role')).toBe('toolbar')
    expect(w.attributes('aria-label')).toBe('Canciones seleccionadas')
  })

  it('if the rest of the list is still arriving, «todas» means the ones that have arrived and it says so', async () => {
    const w = mountView({ count: 400, allSelected: true, loading: true })
    expect(w.find('.sel-note').text()).toContain('el resto sigue cargando')
    await w.setProps({ loading: false })
    expect(w.find('.sel-note').exists()).toBe(false)
  })
})

describe('the buttons', () => {
  it('«Seleccionar todas», or «Ninguna» if all already are', async () => {
    const w = mountView()
    await button(w, 'all').trigger('click')
    expect(w.emitted('selectAll')).toHaveLength(1)
    expect(button(w, 'none').exists()).toBe(false)
    await w.setProps({ allSelected: true })
    expect(button(w, 'all').exists()).toBe(false)
    await button(w, 'none').trigger('click')
    expect(w.emitted('selectNone')).toHaveLength(1)
  })

  it('Telegram only if installed; remove from the playlist only in a playlist', async () => {
    const w = mountView()
    expect(button(w, 'telegram').exists()).toBe(false)
    expect(button(w, 'remove').exists()).toBe(false)
    await w.setProps({ telegram: true, playlist: 'Domingo' })
    expect(button(w, 'telegram').exists()).toBe(true)
    expect(button(w, 'remove').attributes('aria-label')).toBe('Quitar de «Domingo»')
    await button(w, 'telegram').trigger('click')
    await button(w, 'remove').trigger('click')
    expect(w.emitted('telegram')).toHaveLength(1)
    expect(w.emitted('removeFromList')).toHaveLength(1)
  })

  it('outside ones (the player list) offer neither favorites nor trash', () => {
    const w = mountView({ library: false })
    expect(button(w, 'favorites').exists()).toBe(false)
    expect(button(w, 'trash').exists()).toBe(false)
    expect(button(w, 'list').exists()).toBe(true)
  })

  it('with none selected, the actions on them cannot be clicked; all and Listo can', () => {
    const w = mountView({ count: 0, telegram: true, playlist: 'Domingo' })
    for (const act of ['list', 'favorites', 'telegram', 'remove', 'trash', 'more']) {
      expect(button(w, act).attributes('disabled'), act).toBeDefined()
    }
    expect(button(w, 'all').attributes('disabled')).toBeUndefined()
    expect(button(w, 'done').attributes('disabled')).toBeUndefined()
  })

  it('add and «⋯» pass the click on (the menu opens under the button); the rest, nothing else', async () => {
    const w = mountView()
    await button(w, 'list').trigger('click')
    await button(w, 'more').trigger('click')
    await button(w, 'favorites').trigger('click')
    await button(w, 'trash').trigger('click')
    await button(w, 'done').trigger('click')
    expect(w.emitted('addToList')[0][0]).toBeInstanceOf(MouseEvent)
    expect(w.emitted('more')[0][0]).toBeInstanceOf(MouseEvent)
    expect(w.emitted('favorites')).toHaveLength(1)
    expect(w.emitted('trash')).toHaveLength(1)
    expect(w.emitted('done')).toHaveLength(1)
    expect(button(w, 'list').attributes('aria-haspopup')).toBe('menu')
  })

  it('if all are already favorites, the button says so and does nothing (removing them is in «⋯»)', async () => {
    const w = mountView({ allFavorite: true })
    const fav = button(w, 'favorites')
    expect(fav.attributes('aria-disabled')).toBe('true')
    expect(fav.attributes('aria-label')).toBe('Ya son favoritas')
    await fav.trigger('click')
    expect(w.emitted('favorites')).toBeFalsy()
  })

  it('each button has a name even when narrow only the icon shows', () => {
    const w = mountView({ telegram: true, playlist: 'Domingo' })
    for (const b of w.findAll('button')) {
      expect(b.attributes('aria-label'), b.html()).toBeTruthy()
      expect(b.attributes('type')).toBe('button')
    }
  })
})

describe('what fits', () => {
  // El sitio no depende solo de la ventana (el panel de la ficha estrecha la
  // lista; Telegram y «Quitar» añaden botones): la barra se mide y aprieta lo
  // justo. jsdom no maqueta, así que las medidas se fingen por nivel.
  it('switches to short labels, icons only or just the count depending on space', async () => {
    const w = mountView({ count: 2 })
    const el = w.element
    const level = () =>
      ['fit-short', 'fit-icons', 'fit-tight'].filter((c) => el.classList.contains(c)).length
    let width = 500
    const needed = [900, 700, 420, 300]
    Object.defineProperty(el, 'clientWidth', { configurable: true, get: () => width })
    Object.defineProperty(el, 'scrollWidth', { configurable: true, get: () => needed[level()] })
    await w.setProps({ count: 3 })
    await flushPromises()
    expect([...el.classList]).toEqual(['sel-bar', 'fit-short', 'fit-icons'])
    width = 350
    await w.setProps({ count: 4 })
    await flushPromises()
    expect(level(), 'solo el número').toBe(3)
    width = 1000 // se abre sitio: vuelven las etiquetas enteras
    await w.setProps({ telegram: true })
    await flushPromises()
    expect(level()).toBe(0)
  })
})

describe('with the keyboard', () => {
  const press = (el, key, extra = {}) =>
    el.element.dispatchEvent(
      new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true, ...extra })
    )

  it('arrow keys move between buttons (wrapping around) and Escape finishes', async () => {
    const w = mountView()
    const enabled = w.findAll('button').filter((b) => !b.attributes('disabled'))
    enabled[0].element.focus()
    press(enabled[0], 'ArrowRight')
    expect(document.activeElement).toBe(enabled[1].element)
    press(enabled[1], 'End')
    expect(document.activeElement).toBe(enabled.at(-1).element)
    press(enabled.at(-1), 'ArrowRight')
    expect(document.activeElement).toBe(enabled[0].element)
    press(enabled[0], 'ArrowLeft')
    expect(document.activeElement).toBe(enabled.at(-1).element)
    press(enabled.at(-1), 'Escape')
    expect(w.emitted('done')).toHaveLength(1)
  })
})

// Lo que hacen los botones: las acciones de useSongMenus (las mismas del menú
// de clic derecho sobre varias), con todo lo demás falso.
describe('what it does with the selected ones', () => {
  function prepare({ view = { kind: 'all' } } = {}) {
    const songs = ref([song(1), song(2, { title: 'Otra' }), song(3, { title: 'Tercera' })])
    const selection = useSelection({ shown: songs })
    const ctx = {
      view: ref(view),
      player: { track: ref(null), state: { playing: false }, stop: vi.fn() },
      playlistActions: {
        playlists: ref([
          { id: 7, name: 'Domingo', n: 3 },
          { id: 8, name: 'Una lista con un nombre larguísimo que no cabe en ningún sitio', n: 1 }
        ]),
        addTo: vi.fn(),
        addManyTo: vi.fn(),
        create: vi.fn(),
        removeSong: vi.fn(),
        load: vi.fn(async () => {})
      },
      selection,
      play: vi.fn(),
      onUpdated: vi.fn(),
      refreshAll: vi.fn(async () => {}),
      reload: vi.fn(async () => {}),
      detailsInView: () => true,
      showDetailsOf: vi.fn()
    }
    return { ctx, songs, selection, menus: useSongMenus(ctx) }
  }

  it('«Añadir a una lista» opens the playlists (and «Nueva lista…») under the button', async () => {
    const { menus, songs, ctx } = prepare()
    const w = mountView()
    const btn = button(w, 'list').element
    btn.getBoundingClientRect = () => ({ left: 40, bottom: 120, top: 100, right: 160 })
    btn.dispatchEvent(
      new MouseEvent('click', { bubbles: true, clientX: 99, clientY: 99, detail: 1 })
    )
    const [ev] = w.emitted('addToList')[0]
    menus.addToList(ev, songs.value.slice(0, 2))
    expect(menu.value.open).toBe(true)
    expect([menu.value.x, menu.value.y]).toEqual([40, 124])
    expect(menu.value.keyboard, 'con el ratón el menú no se queda el foco').toBe(false)
    expect(menu.value.title).toBe('Añadir 2 a una lista')
    expect(labels()).toEqual([
      'Domingo',
      'Una lista con un nombre larguísimo que no cabe en ningún sitio',
      'Nueva lista…'
    ])
    await menu.value.items[0].action()
    expect(ctx.playlistActions.addManyTo).toHaveBeenCalledWith(songs.value.slice(0, 2), {
      id: 7,
      name: 'Domingo',
      n: 3
    })
  })

  it('with a single one, the playlist gets that song (and its notice mentions it)', async () => {
    const { menus, songs, ctx } = prepare()
    menus.addToList({ clientX: 1, clientY: 1 }, [songs.value[1]])
    expect(menu.value.title).toBe('Añadir a una lista')
    await menu.value.items[0].action()
    expect(ctx.playlistActions.addTo).toHaveBeenCalledWith(songs.value[1], expect.anything())
  })

  it('favorites marks those that were not, with a notice', async () => {
    const { menus, ctx } = prepare()
    await menus.favorites([song(1), song(2, { favorite: 1 }), song(3)])
    expect(held.api.toggleFavorite.mock.calls).toEqual([
      [1, true],
      [3, true]
    ])
    expect(ctx.onUpdated).toHaveBeenCalledTimes(2)
    expect(notices.value.at(-1).message).toBe('2 marcadas como favoritas')
    await menus.favorites([song(2, { favorite: 1 })])
    expect(notices.value.at(-1).message).toBe('Ya era favorita')
  })

  it('trash: with one, the single confirmation (with its file); with several, the one that says how many', async () => {
    const { menus } = prepare()
    menus.trash([song(1)])
    await flushPromises()
    expect(dialog.value.open).toBe(true)
    expect(dialog.value.title).toBe('Mandar a la papelera')
    dialogCancel()
    await flushPromises()
    menus.trash([song(1), song(2)])
    await flushPromises()
    expect(dialog.value.title).toBe('Mandar 2 canciones a la papelera')
    dialogCancel()
    await flushPromises()
    expect(held.api.deleteSong).not.toHaveBeenCalled()
  })

  it('remove from the playlist: only in a playlist, without asking, and says which one', async () => {
    const outside = prepare()
    await outside.menus.removeFromView([song(1)])
    expect(held.api.removeFromPlaylist).not.toHaveBeenCalled()

    const { menus, ctx, selection } = prepare({
      view: { kind: 'playlist', id: 7, name: 'Domingo' }
    })
    selection.setMode(true)
    await selection.select(1, {})
    await selection.select(3, {})
    await menus.removeFromView(selection.chosenSongs.value)
    expect(held.api.removeFromPlaylist.mock.calls).toEqual([
      [7, 1],
      [7, 3]
    ])
    expect(dialog.value.open).toBe(false)
    expect(notices.value.at(-1).message).toBe('2 quitadas de «Domingo»')
    expect(selection.count.value, 'las quitadas ya no están elegidas').toBe(0)
    expect(ctx.reload).toHaveBeenCalledWith(true)
    await menus.removeFromView([song(2, { title: 'Otra' })])
    expect(notices.value.at(-1).message).toBe('«Otra» quitada de «Domingo»')
  })

  it('the «⋯» is the group menu, also with a single one (in singular)', async () => {
    const { menus } = prepare({ view: { kind: 'playlist', id: 7, name: 'Domingo' } })
    await menus.loadShareTargets()
    menus.groupMenu({ clientX: 5, clientY: 5 }, [song(2, { title: 'Otra' })])
    expect(menu.value.title).toBe('Otra')
    expect(labels()).toEqual(
      expect.arrayContaining([
        'Reproducir',
        'Añadir a una lista',
        'Marcar como favorita',
        'Quitar de esta lista',
        'Enviar por Telegram',
        'Mandar a la papelera…'
      ])
    )
    menus.groupMenu({ clientX: 5, clientY: 5 }, [
      song(1, { favorite: 1 }),
      song(2, { favorite: 1 })
    ])
    expect(menu.value.title).toBe('2 canciones')
    expect(labels()).toContain('Quitar 2 de favoritas')
    expect(labels()).not.toContain('Marcar 2 como favoritas')
  })

  it('group options from the search box do not mention «esta lista» and play with their own queue', async () => {
    const { menus, ctx } = prepare({ view: { kind: 'playlist', id: 7, name: 'Domingo' } })
    const play = vi.fn()
    const items = menus.groupItems([song(1), song(2)], { fromSearch: true, play })
    expect(items.map((i) => i.label)).not.toContain('Quitar 2 de esta lista')
    await items[0].action()
    expect(play).toHaveBeenCalledWith([song(1), song(2)])
    expect(ctx.play).not.toHaveBeenCalled()
  })

  it('in the mode, right-clicking a selected one opens the selection menu and deselects nothing', async () => {
    const { menus, selection } = prepare()
    selection.setMode(true)
    await selection.select(1, {})
    menus.songMenu({ clientX: 1, clientY: 1 }, song(1))
    expect(menu.value.title).toBe('Cancion 1')
    expect(labels()).toContain('Marcar como favorita') // el de grupo, en singular
    // sobre una no elegida, el suyo, sin elegirla
    menus.songMenu({ clientX: 1, clientY: 1 }, song(3, { title: 'Tercera' }))
    expect(labels()).toContain('Renombrar…')
    await flushPromises()
    expect(selection.chosenSongs.value.map((s) => s.id)).toEqual([1])
  })
})

describe('the sizes (CSS)', () => {
  const css = allCss()
  const breakpoint = (px) => {
    const i = css.indexOf(`@media (max-width:${px}px){.sel-`)
    expect(i, `no hay tramo de ${px}px para la barra`).toBeGreaterThan(-1)
    return css.slice(
      i,
      css.indexOf('\n@media', i + 10) > 0 ? css.indexOf('\n@media', i + 10) : undefined
    )
  }

  it('medium: short labels; narrow: icons only; column: only the count visible', () => {
    expect(breakpoint(1100)).toMatch(/\.sel-long\{display:none\}/)
    expect(breakpoint(1100)).toMatch(/\.sel-short\{display:inline\}/)
    expect(breakpoint(700)).toMatch(/\.sel-label,[^{]*\{display:none\}/)
    expect(breakpoint(700)).toMatch(/\.select-toggle \.btn-label\{display:none\}/)
    // la palabra no desaparece del todo: la lee el lector de pantalla
    expect(breakpoint(460)).toMatch(/\.sel-word,\.sel-note\{position:absolute;width:1px;height:1px/)
  })

  it('never overflows: if it does not fit, it scrolls horizontally; and the list does not shrink', () => {
    expect(css).toMatch(/\.sel-bar\{[^}]*overflow:auto hidden/)
    expect(css).toMatch(/\.sel-bar\{[^}]*flex:none/)
    expect(css).toMatch(/\.sel-status\{[^}]*text-overflow:ellipsis/)
  })

  it('theme colors (light or dark), none fixed', async () => {
    const { readFileSync } = await import('node:fs')
    const { join, dirname } = await import('node:path')
    const { fileURLToPath } = await import('node:url')
    const file = join(
      dirname(fileURLToPath(import.meta.url)),
      '..',
      'src',
      'styles',
      'selection-bar.css'
    )
    const own = readFileSync(file, 'utf8').replace(/\/\*[\s\S]*?\*\//g, '')
    expect(own).not.toMatch(/#[0-9a-f]{3,8}\b|rgba?\(/i)
    expect(own).toMatch(/var\(--accent\)/)
  })
})
