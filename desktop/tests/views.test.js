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
const tres = () => [song(1), song(2), song(3, { favorite: 1 })]

const VISTAS = [
  ['la tabla', SongTable, 'table'],
  ['la lista fina', SongRows, 'rows'],
  ['las fichas', SongCards, 'cards'],
  ['la cuadricula', SongGrid, 'grid']
]
// la cuadricula no enseña ni estrellas ni corazon: se valora y se marca desde
// el menu de la cancion
const CON_ESTRELLAS = ['table', 'rows', 'cards']

/** Monta la vista suelta o dentro de la agrupada, que reenvia lo que le llega. */
function montar(Vista, layout, agrupada, props = {}) {
  return agrupada
    ? mount(GroupedSongs, { props: { songs: tres(), by: 'artist', layout, ...props } })
    : mount(Vista, { props: { songs: tres(), ...props } })
}
/** Lo que emitio quien sea que este fuera (la vista o la agrupada). */
const emitido = (w, evento) => w.emitted(evento)

for (const agrupada of [false, true]) {
  describe(agrupada ? 'agrupadas' : 'sueltas', () => {
    for (const [nombre, Vista, layout] of VISTAS) {
      describe(nombre, () => {
        it('el clic elige la cancion y lleva sus teclas (Ctrl, Mayus)', async () => {
          const w = montar(Vista, layout, agrupada)
          const fila = w.findAll('[data-song-row]').find((f) => f.text().includes('Cancion 2'))
          await fila.trigger('click', { ctrlKey: true })
          const [id, ev] = emitido(w, 'select')[0]
          expect(id).toBe(2)
          expect(ev.ctrlKey, 'la tecla se perdio por el camino').toBe(true)
          await fila.trigger('click', { shiftKey: true })
          expect(emitido(w, 'select')[1][1].shiftKey).toBe(true)
        })

        it('el boton y el doble clic la ponen a sonar; el clic derecho abre su menu', async () => {
          const w = montar(Vista, layout, agrupada)
          const fila = w.findAll('[data-song-row]').find((f) => f.text().includes('Cancion 2'))
          await fila.find('button[title="Reproducir"]').trigger('click')
          await fila.trigger('dblclick')
          expect(emitido(w, 'play').map(([c]) => c.id)).toEqual([2, 2])
          expect(emitido(w, 'select'), 'el boton no es un clic en la fila').toBeFalsy()
          await fila.trigger('contextmenu', { clientX: 40, clientY: 50 })
          const [ev, c] = emitido(w, 'context')[0]
          expect(c.id).toBe(2)
          expect(ev.clientX).toBe(40)
        })

        if (CON_ESTRELLAS.includes(layout)) {
          it('el corazon y las estrellas se cambian sin elegir la fila', async () => {
            const w = montar(Vista, layout, agrupada)
            const fila = w.findAll('[data-song-row]').find((f) => f.text().includes('Cancion 3'))
            const corazon = fila.find('.heart')
            expect(corazon.attributes('aria-pressed')).toBe('true')
            await corazon.trigger('click')
            expect(emitido(w, 'toggleFavorite')[0][0].id).toBe(3)
            const cuatro = fila
              .findAll('.stars .ico')
              .find((i) => i.attributes('title') === '4 de 5')
            await cuatro.trigger('click')
            expect(emitido(w, 'setStars')[0].map((x) => x.id ?? x)).toEqual([3, 4])
            expect(emitido(w, 'select'), 'marcar no deberia elegir la fila').toBeFalsy()
          })
        }

        it('copia el titulo y el nombre completo sin elegir ni poner a sonar la fila', async () => {
          const escrito = []
          Object.defineProperty(navigator, 'clipboard', {
            configurable: true,
            value: { writeText: async (t) => escrito.push(t) }
          })
          const { notices, clearNotices } = useNotices()
          clearNotices()
          const songs = [song(1), song(2, { file: 'Barak - Cancion 2 (En Vivo).mp3' }), song(3)]
          const w = montar(Vista, layout, agrupada, { songs })
          const fila = w.findAll('[data-song-row]').find((f) => f.text().includes('Cancion 2'))
          const titulo = fila.find('button[aria-label="Copiar el título"]')
          const completo = fila.find('button[aria-label="Copiar el nombre completo"]')
          expect(completo.text(), 'el de al final dice que es el nombre').toContain('nombre')
          await titulo.trigger('click')
          await completo.trigger('click')
          await completo.trigger('dblclick')
          await flushPromises()
          expect(escrito).toEqual(['Cancion 2', 'Barak - Cancion 2 (En Vivo)'])
          expect(notices.value.map((n) => n.message)).toContain(
            'Copiado: Barak - Cancion 2 (En Vivo)'
          )
          expect(emitido(w, 'select'), 'copiar no elige la fila').toBeFalsy()
          expect(emitido(w, 'play'), 'ni el doble clic en el boton la pone a sonar').toBeFalsy()
          clearNotices()
        })

        it('la duracion se lee en minutos, igual en todas', () => {
          const w = montar(Vista, layout, agrupada)
          if (layout === 'grid') return // la cuadricula no la enseña
          expect(w.text()).toContain('2:05')
        })
      })
    }
  })
}

describe('la vista agrupada', () => {
  it('cada grupo dice cuantas lleva y cuanto duran, y se pliega', async () => {
    const w = mount(GroupedSongs, { props: { songs: tres(), by: 'artist', layout: 'rows' } })
    const [barak] = w.findAll('.group-head')
    expect(barak.text()).toContain('2 temas · 4 min')
    expect(barak.attributes('aria-expanded')).toBe('true')
    await barak.trigger('click')
    expect(barak.attributes('aria-expanded')).toBe('false')
    expect(w.findAll('[data-song-row]')).toHaveLength(1)
    await barak.trigger('click')
    expect(w.findAll('[data-song-row]')).toHaveLength(3)
  })

  it('agrupa por lo que se le pida, con un nombre para lo que no lo tiene', () => {
    const sinDatos = [song(1, { artist: '', album: '', genre: '', key: '' })]
    for (const [by, grupo] of [
      ['artist', 'Sin artista'],
      ['album', 'Sin álbum'],
      ['genre', 'Sin género'],
      ['key', 'Sin tono'],
      ['initial', 'C'],
      ['folder', 'x']
    ]) {
      const w = mount(GroupedSongs, { props: { songs: sinDatos, by, layout: 'rows' } })
      expect(w.find('.group-head span').text(), by).toBe(grupo)
    }
  })

  it('sin canciones lo dice', () => {
    const w = mount(GroupedSongs, { props: { songs: [], by: 'artist', layout: 'table' } })
    expect(w.text()).toContain('Nada por aquí')
  })

  it('la tabla agrupada, sin cabecera, lleva el ancho de cada columna', () => {
    // su primera fila es la del grupo, una celda que abarca todas: sin los
    // `col`, `table-layout: fixed` las dejaba todas iguales y el titulo se
    // quedaba en «Mi …»
    const w = mount(GroupedSongs, { props: { songs: tres(), by: 'artist', layout: 'table' } })
    expect(w.find('thead').exists()).toBe(false)
    const cols = w.findAll('colgroup col').map((c) => c.classes()[0])
    expect(cols).toContain('col-title')
    expect(cols).toHaveLength(w.find('[data-song-row]').findAll('td').length)
  })
})
