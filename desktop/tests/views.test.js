import { describe, it, expect } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import SongTable from '../src/components/SongTable.vue'
import SongRows from '../src/components/SongRows.vue'
import SongCards from '../src/components/SongCards.vue'
import SongGrid from '../src/components/SongGrid.vue'
import GroupedSongs from '../src/components/GroupedSongs.vue'
import { useNotices } from '../src/composables/useNotices.js'

// Lo que se puede hacer con una fila es lo mismo en las cuatro vistas, y
// tambien agrupadas: al agrupar se perdia por el camino el evento del clic (y
// con el, Ctrl y Mayus) porque cada vista reenviaba a mano lo que le llegaba.
// Aqui se pulsa de verdad en cada una y se mira lo que sale.
const song = (id, extra = {}) => ({
  id,
  title: 'Cancion ' + id,
  artist: id === 3 ? 'New Wine' : 'Barak',
  file: id + '.mp3',
  duration: 125,
  bitrate: 320000,
  stars: 2,
  favorite: 0,
  folder: 'x',
  key: 'G',
  ...extra
})
const three = () => [song(1), song(2), song(3, { favorite: 1 })]

const VIEWS = [
  ['the table', SongTable, 'table'],
  ['the compact list', SongRows, 'rows'],
  ['the cards', SongCards, 'cards'],
  ['the grid', SongGrid, 'grid']
]
// la cuadricula no enseña ni estrellas ni corazon: se valora y se marca desde
// el menu de la cancion
const WITH_STARS = ['table', 'rows', 'cards']

/** Monta la vista suelta o dentro de la agrupada, que reenvia lo que le llega. */
function mountView(View, layout, grouped, props = {}) {
  return grouped
    ? mount(GroupedSongs, { props: { songs: three(), by: 'artist', layout, ...props } })
    : mount(View, { props: { songs: three(), ...props } })
}
/** Lo que emitio quien sea que este fuera (la vista o la agrupada). */
const emitted = (w, event) => w.emitted(event)

for (const grouped of [false, true]) {
  describe(grouped ? 'grouped' : 'ungrouped', () => {
    for (const [name, View, layout] of VIEWS) {
      describe(name, () => {
        it('the click selects the song and carries its keys (Ctrl, Shift)', async () => {
          const w = mountView(View, layout, grouped)
          const row = w.findAll('[data-song-row]').find((f) => f.text().includes('Cancion 2'))
          await row.trigger('click', { ctrlKey: true })
          const [id, ev] = emitted(w, 'select')[0]
          expect(id).toBe(2)
          expect(ev.ctrlKey, 'la tecla se perdio por el camino').toBe(true)
          await row.trigger('click', { shiftKey: true })
          expect(emitted(w, 'select')[1][1].shiftKey).toBe(true)
        })

        it('the button and a double click play it; right-click opens its menu', async () => {
          const w = mountView(View, layout, grouped)
          const row = w.findAll('[data-song-row]').find((f) => f.text().includes('Cancion 2'))
          await row.find('button[title="Reproducir"]').trigger('click')
          await row.trigger('dblclick')
          expect(emitted(w, 'play').map(([c]) => c.id)).toEqual([2, 2])
          expect(emitted(w, 'select'), 'el boton no es un clic en la fila').toBeFalsy()
          await row.trigger('contextmenu', { clientX: 40, clientY: 50 })
          const [ev, c] = emitted(w, 'context')[0]
          expect(c.id).toBe(2)
          expect(ev.clientX).toBe(40)
        })

        if (WITH_STARS.includes(layout)) {
          it('the heart and the stars change without selecting the row', async () => {
            const w = mountView(View, layout, grouped)
            const row = w.findAll('[data-song-row]').find((f) => f.text().includes('Cancion 3'))
            const heart = row.find('.heart')
            expect(heart.attributes('aria-pressed')).toBe('true')
            await heart.trigger('click')
            expect(emitted(w, 'toggleFavorite')[0][0].id).toBe(3)
            const four = row.findAll('.stars .ico').find((i) => i.attributes('title') === '4 de 5')
            await four.trigger('click')
            expect(emitted(w, 'setStars')[0].map((x) => x.id ?? x)).toEqual([3, 4])
            expect(emitted(w, 'select'), 'marcar no deberia elegir la fila').toBeFalsy()
          })
        }

        it('copies the title and the full name without selecting or playing the row', async () => {
          const written = []
          Object.defineProperty(navigator, 'clipboard', {
            configurable: true,
            value: { writeText: async (t) => written.push(t) }
          })
          const { notices, clearNotices } = useNotices()
          clearNotices()
          const songs = [song(1), song(2, { file: 'Barak - Cancion 2 (En Vivo).mp3' }), song(3)]
          const w = mountView(View, layout, grouped, { songs })
          const row = w.findAll('[data-song-row]').find((f) => f.text().includes('Cancion 2'))
          const title = row.find('button[aria-label="Copiar el título"]')
          const full = row.find('button[aria-label="Copiar el nombre completo"]')
          expect(full.text(), 'el de al final dice que es el nombre').toContain('nombre')
          await title.trigger('click')
          await full.trigger('click')
          await full.trigger('dblclick')
          await flushPromises()
          expect(written).toEqual(['Cancion 2', 'Barak - Cancion 2 (En Vivo)'])
          expect(notices.value.map((n) => n.message)).toContain(
            'Copiado: Barak - Cancion 2 (En Vivo)'
          )
          expect(emitted(w, 'select'), 'copiar no elige la fila').toBeFalsy()
          expect(emitted(w, 'play'), 'ni el doble clic en el boton la pone a sonar').toBeFalsy()
          clearNotices()
        })

        it('the duration reads in minutes, the same in all of them', () => {
          const w = mountView(View, layout, grouped)
          if (layout === 'grid') return // la cuadricula no la enseña
          expect(w.text()).toContain('2:05')
        })
      })
    }
  })
}

describe('the grouped view', () => {
  it('each group says how many it has and how long they last, and collapses', async () => {
    const w = mount(GroupedSongs, { props: { songs: three(), by: 'artist', layout: 'rows' } })
    const [barakGroup] = w.findAll('.group-head')
    expect(barakGroup.text()).toContain('2 temas · 4 min')
    expect(barakGroup.attributes('aria-expanded')).toBe('true')
    await barakGroup.trigger('click')
    expect(barakGroup.attributes('aria-expanded')).toBe('false')
    expect(w.findAll('[data-song-row]')).toHaveLength(1)
    await barakGroup.trigger('click')
    expect(w.findAll('[data-song-row]')).toHaveLength(3)
  })

  it('groups by whatever is requested, with a name for what has none', () => {
    const noData = [song(1, { artist: '', album: '', genre: '', key: '' })]
    for (const [by, group] of [
      ['artist', 'Sin artista'],
      ['album', 'Sin álbum'],
      ['genre', 'Sin género'],
      ['key', 'Sin tono'],
      ['initial', 'C'],
      ['folder', 'x']
    ]) {
      const w = mount(GroupedSongs, { props: { songs: noData, by, layout: 'rows' } })
      expect(w.find('.group-head span').text(), by).toBe(group)
    }
  })

  it('with no songs it says so', () => {
    const w = mount(GroupedSongs, { props: { songs: [], by: 'artist', layout: 'table' } })
    expect(w.text()).toContain('Nada por aquí')
  })

  it('the grouped table, without a header, carries the width of each column', () => {
    // su primera fila es la del grupo, una celda que abarca todas: sin los
    // `col`, `table-layout: fixed` las dejaba todas iguales y el titulo se
    // quedaba en «Mi …»
    const w = mount(GroupedSongs, { props: { songs: three(), by: 'artist', layout: 'table' } })
    expect(w.find('thead').exists()).toBe(false)
    const cols = w.findAll('colgroup col').map((c) => c.classes()[0])
    expect(cols).toContain('col-title')
    expect(cols).toHaveLength(w.find('[data-song-row]').findAll('td').length)
  })
})
