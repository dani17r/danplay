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

const montados = []
const montar = (props = {}) => {
  const w = mount(SelectionBar, { props: { count: 2, ...props }, attachTo: document.body })
  montados.push(w)
  return w
}
afterEach(() => {
  for (const w of montados.splice(0)) w.unmount()
  document.body.innerHTML = ''
})
beforeEach(() => {
  closeMenu()
  dialogCancel()
  clearNotices()
  vi.clearAllMocks()
})
const boton = (w, act) => w.find(`[data-act="${act}"]`)
const etiquetas = () => menu.value.items.filter((i) => !i.separator).map((i) => i.label)

describe('el contador', () => {
  it('dice cuántas, en singular o plural, y se anuncia al cambiar', async () => {
    const w = montar({ count: 3 })
    const estado = w.find('.sel-status')
    expect(estado.attributes('aria-live')).toBe('polite')
    expect(estado.text()).toBe('3 seleccionadas')
    await w.setProps({ count: 1 })
    expect(estado.text()).toBe('1 seleccionada')
    await w.setProps({ count: 0 })
    expect(estado.text()).toBe('Elige canciones')
  })

  it('es una barra de herramientas con nombre', () => {
    const w = montar()
    expect(w.attributes('role')).toBe('toolbar')
    expect(w.attributes('aria-label')).toBe('Canciones seleccionadas')
  })

  it('si el resto de la lista aún llega, «todas» son las que han llegado y se dice', async () => {
    const w = montar({ count: 400, allSelected: true, loading: true })
    expect(w.find('.sel-note').text()).toContain('el resto sigue cargando')
    await w.setProps({ loading: false })
    expect(w.find('.sel-note').exists()).toBe(false)
  })
})

describe('los botones', () => {
  it('«Seleccionar todas», o «Ninguna» si ya lo están', async () => {
    const w = montar()
    await boton(w, 'all').trigger('click')
    expect(w.emitted('selectAll')).toHaveLength(1)
    expect(boton(w, 'none').exists()).toBe(false)
    await w.setProps({ allSelected: true })
    expect(boton(w, 'all').exists()).toBe(false)
    await boton(w, 'none').trigger('click')
    expect(w.emitted('selectNone')).toHaveLength(1)
  })

  it('Telegram solo si está instalado; quitar de la lista solo en un repertorio', async () => {
    const w = montar()
    expect(boton(w, 'telegram').exists()).toBe(false)
    expect(boton(w, 'remove').exists()).toBe(false)
    await w.setProps({ telegram: true, playlist: 'Domingo' })
    expect(boton(w, 'telegram').exists()).toBe(true)
    expect(boton(w, 'remove').attributes('aria-label')).toBe('Quitar de «Domingo»')
    await boton(w, 'telegram').trigger('click')
    await boton(w, 'remove').trigger('click')
    expect(w.emitted('telegram')).toHaveLength(1)
    expect(w.emitted('removeFromList')).toHaveLength(1)
  })

  it('las de fuera (la lista del reproductor) no ofrecen favoritas ni papelera', () => {
    const w = montar({ library: false })
    expect(boton(w, 'favorites').exists()).toBe(false)
    expect(boton(w, 'trash').exists()).toBe(false)
    expect(boton(w, 'list').exists()).toBe(true)
  })

  it('sin ninguna elegida, lo que actúa sobre ellas no se puede pulsar; todas y Listo sí', () => {
    const w = montar({ count: 0, telegram: true, playlist: 'Domingo' })
    for (const act of ['list', 'favorites', 'telegram', 'remove', 'trash', 'more']) {
      expect(boton(w, act).attributes('disabled'), act).toBeDefined()
    }
    expect(boton(w, 'all').attributes('disabled')).toBeUndefined()
    expect(boton(w, 'done').attributes('disabled')).toBeUndefined()
  })

  it('añadir y «⋯» pasan el clic (el menú se abre bajo el botón); el resto, sin más', async () => {
    const w = montar()
    await boton(w, 'list').trigger('click')
    await boton(w, 'more').trigger('click')
    await boton(w, 'favorites').trigger('click')
    await boton(w, 'trash').trigger('click')
    await boton(w, 'done').trigger('click')
    expect(w.emitted('addToList')[0][0]).toBeInstanceOf(MouseEvent)
    expect(w.emitted('more')[0][0]).toBeInstanceOf(MouseEvent)
    expect(w.emitted('favorites')).toHaveLength(1)
    expect(w.emitted('trash')).toHaveLength(1)
    expect(w.emitted('done')).toHaveLength(1)
    expect(boton(w, 'list').attributes('aria-haspopup')).toBe('menu')
  })

  it('si ya son todas favoritas, el botón lo dice y no hace nada (quitarlas está en «⋯»)', async () => {
    const w = montar({ allFavorite: true })
    const fav = boton(w, 'favorites')
    expect(fav.attributes('aria-disabled')).toBe('true')
    expect(fav.attributes('aria-label')).toBe('Ya son favoritas')
    await fav.trigger('click')
    expect(w.emitted('favorites')).toBeFalsy()
  })

  it('cada botón tiene nombre aunque en estrecho solo se vea el icono', () => {
    const w = montar({ telegram: true, playlist: 'Domingo' })
    for (const b of w.findAll('button')) {
      expect(b.attributes('aria-label'), b.html()).toBeTruthy()
      expect(b.attributes('type')).toBe('button')
    }
  })
})

describe('lo que cabe', () => {
  // El sitio no depende solo de la ventana (el panel de la ficha estrecha la
  // lista; Telegram y «Quitar» añaden botones): la barra se mide y aprieta lo
  // justo. jsdom no maqueta, así que las medidas se fingen por nivel.
  it('pasa a etiquetas cortas, a solo iconos o a solo el número según quepa', async () => {
    const w = montar({ count: 2 })
    const el = w.element
    const nivel = () =>
      ['fit-short', 'fit-icons', 'fit-tight'].filter((c) => el.classList.contains(c)).length
    let ancho = 500
    const necesita = [900, 700, 420, 300]
    Object.defineProperty(el, 'clientWidth', { configurable: true, get: () => ancho })
    Object.defineProperty(el, 'scrollWidth', { configurable: true, get: () => necesita[nivel()] })
    await w.setProps({ count: 3 })
    await flushPromises()
    expect([...el.classList]).toEqual(['sel-bar', 'fit-short', 'fit-icons'])
    ancho = 350
    await w.setProps({ count: 4 })
    await flushPromises()
    expect(nivel(), 'solo el número').toBe(3)
    ancho = 1000 // se abre sitio: vuelven las etiquetas enteras
    await w.setProps({ telegram: true })
    await flushPromises()
    expect(nivel()).toBe(0)
  })
})

describe('con el teclado', () => {
  const tecla = (el, key, extra = {}) =>
    el.element.dispatchEvent(
      new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true, ...extra })
    )

  it('las flechas pasan de un botón a otro (en redondo) y Escape termina', async () => {
    const w = montar()
    const activos = w.findAll('button').filter((b) => !b.attributes('disabled'))
    activos[0].element.focus()
    tecla(activos[0], 'ArrowRight')
    expect(document.activeElement).toBe(activos[1].element)
    tecla(activos[1], 'End')
    expect(document.activeElement).toBe(activos.at(-1).element)
    tecla(activos.at(-1), 'ArrowRight')
    expect(document.activeElement).toBe(activos[0].element)
    tecla(activos[0], 'ArrowLeft')
    expect(document.activeElement).toBe(activos.at(-1).element)
    tecla(activos.at(-1), 'Escape')
    expect(w.emitted('done')).toHaveLength(1)
  })
})

// Lo que hacen los botones: las acciones de useSongMenus (las mismas del menú
// de clic derecho sobre varias), con todo lo demás falso.
describe('lo que hace con las elegidas', () => {
  function preparar({ view = { kind: 'all' } } = {}) {
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

  it('«Añadir a una lista» abre las listas (y «Nueva lista…») debajo del botón', async () => {
    const { menus, songs, ctx } = preparar()
    const w = montar()
    const btn = boton(w, 'list').element
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
    expect(etiquetas()).toEqual([
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

  it('con una sola, la lista recibe esa canción (y su aviso habla de ella)', async () => {
    const { menus, songs, ctx } = preparar()
    menus.addToList({ clientX: 1, clientY: 1 }, [songs.value[1]])
    expect(menu.value.title).toBe('Añadir a una lista')
    await menu.value.items[0].action()
    expect(ctx.playlistActions.addTo).toHaveBeenCalledWith(songs.value[1], expect.anything())
  })

  it('favoritas marca las que no lo eran, con un aviso', async () => {
    const { menus, ctx } = preparar()
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

  it('la papelera: con una, la confirmación de una (con su archivo); con varias, la que dice cuántas', async () => {
    const { menus } = preparar()
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

  it('quitar de la lista: solo en un repertorio, sin preguntar, y dice de cuál', async () => {
    const fuera = preparar()
    await fuera.menus.removeFromView([song(1)])
    expect(held.api.removeFromPlaylist).not.toHaveBeenCalled()

    const { menus, ctx, selection } = preparar({
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

  it('el «⋯» es el menú de grupo, también con una sola (en singular)', async () => {
    const { menus } = preparar({ view: { kind: 'playlist', id: 7, name: 'Domingo' } })
    await menus.loadShareTargets()
    menus.groupMenu({ clientX: 5, clientY: 5 }, [song(2, { title: 'Otra' })])
    expect(menu.value.title).toBe('Otra')
    expect(etiquetas()).toEqual(
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
    expect(etiquetas()).toContain('Quitar 2 de favoritas')
    expect(etiquetas()).not.toContain('Marcar 2 como favoritas')
  })

  it('las opciones del grupo desde el buscador no hablan de «esta lista» y reproducen con su cola', async () => {
    const { menus, ctx } = preparar({ view: { kind: 'playlist', id: 7, name: 'Domingo' } })
    const play = vi.fn()
    const items = menus.groupItems([song(1), song(2)], { fromSearch: true, play })
    expect(items.map((i) => i.label)).not.toContain('Quitar 2 de esta lista')
    await items[0].action()
    expect(play).toHaveBeenCalledWith([song(1), song(2)])
    expect(ctx.play).not.toHaveBeenCalled()
  })

  it('en el modo, el clic derecho sobre una elegida es el de las elegidas y no suelta nada', async () => {
    const { menus, selection } = preparar()
    selection.setMode(true)
    await selection.select(1, {})
    menus.songMenu({ clientX: 1, clientY: 1 }, song(1))
    expect(menu.value.title).toBe('Cancion 1')
    expect(etiquetas()).toContain('Marcar como favorita') // el de grupo, en singular
    // sobre una no elegida, el suyo, sin elegirla
    menus.songMenu({ clientX: 1, clientY: 1 }, song(3, { title: 'Tercera' }))
    expect(etiquetas()).toContain('Renombrar…')
    await flushPromises()
    expect(selection.chosenSongs.value.map((s) => s.id)).toEqual([1])
  })
})

describe('los tamaños (css)', () => {
  const css = allCss()
  const tramo = (px) => {
    const i = css.indexOf(`@media (max-width:${px}px){.sel-`)
    expect(i, `no hay tramo de ${px}px para la barra`).toBeGreaterThan(-1)
    return css.slice(
      i,
      css.indexOf('\n@media', i + 10) > 0 ? css.indexOf('\n@media', i + 10) : undefined
    )
  }

  it('medio: etiquetas cortas; estrecho: solo iconos; columna: solo el número a la vista', () => {
    expect(tramo(1100)).toMatch(/\.sel-long\{display:none\}/)
    expect(tramo(1100)).toMatch(/\.sel-short\{display:inline\}/)
    expect(tramo(700)).toMatch(/\.sel-label,[^{]*\{display:none\}/)
    expect(tramo(700)).toMatch(/\.select-toggle \.btn-label\{display:none\}/)
    // la palabra no desaparece del todo: la lee el lector de pantalla
    expect(tramo(460)).toMatch(/\.sel-word,\.sel-note\{position:absolute;width:1px;height:1px/)
  })

  it('nunca desborda: si no cabe, se desplaza en horizontal; y no se encoge la lista', () => {
    expect(css).toMatch(/\.sel-bar\{[^}]*overflow:auto hidden/)
    expect(css).toMatch(/\.sel-bar\{[^}]*flex:none/)
    expect(css).toMatch(/\.sel-status\{[^}]*text-overflow:ellipsis/)
  })

  it('colores del tema (claro u oscuro), ninguno fijo', async () => {
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
