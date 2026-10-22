import { describe, it, expect, beforeEach, vi } from 'vitest'

// doble del backend. vi.hoisted porque vi.mock se iza al principio del fichero
const { api, app, pickImage } = vi.hoisted(() => ({
  app: { openInBrowser: vi.fn(async () => {}) },
  api: {
    edit: vi.fn(),
    setStars: vi.fn(),
    toggleFavorite: vi.fn(),
    setCover: vi.fn(),
    enrich: vi.fn(),
    details: vi.fn(),
    autofill: vi.fn(),
    transpose: vi.fn(),
    chords: vi.fn(),
    chordsTransposed: vi.fn(),
    coverUrl: (id) => '/c/' + id,
    coverUrlAlt: () => null
  },
  pickImage: vi.fn(async () => null)
}))
vi.mock('../src/api.js', () => ({
  api,
  app,
  pickImage,
  errorMessage: (e) => String(e?.message || e)
}))
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

describe('TextField', () => {
  it('emits what is typed', async () => {
    const w = mount(TextField, { props: { modelValue: '' } })
    await w.find('input').setValue('barak')
    expect(w.emitted('update:modelValue').at(-1)).toEqual(['barak'])
  })
  it('the clear button empties the value', async () => {
    const w = mount(TextField, { props: { modelValue: 'algo' } })
    await w.find('.field-btn').trigger('click')
    expect(w.emitted('update:modelValue').at(-1)).toEqual([''])
  })
  it('passwords are hidden and can be shown', async () => {
    const w = mount(TextField, { props: { modelValue: 'secreto', type: 'password' } })
    expect(w.find('input').attributes('type')).toBe('password')
    await w.find('.field-btn').trigger('click')
    expect(w.find('input').attributes('type')).toBe('text')
  })
  it('marks focus so it can be styled', async () => {
    const w = mount(TextField, { props: { modelValue: '' } })
    await w.find('input').trigger('focus')
    expect(w.classes()).toContain('focused')
    await w.find('input').trigger('blur')
    expect(w.classes()).not.toContain('focused')
  })
  it('shows the error when there is one', () => {
    const w = mount(TextField, { props: { modelValue: '', error: 'ruta invalida' } })
    expect(w.classes()).toContain('error')
    expect(w.text()).toContain('ruta invalida')
  })
  it('notifies on Enter', async () => {
    const w = mount(TextField, { props: { modelValue: 'x' } })
    await w.find('input').trigger('keydown.enter')
    await w.find('input').trigger('keyup.enter')
    expect(w.emitted('enter')).toHaveLength(1)
  })
  it('an Enter pressed outside and released inside does not count', async () => {
    // El Enter que activa un boton o una opcion de menu se SUELTA ya dentro del
    // campo de un dialogo que se abre con texto puesto (Renombrar…): lo
    // aceptaba al instante, sin que nadie lo hubiera pulsado ahi.
    const w = mount(TextField, { props: { modelValue: 'x' } })
    await w.find('input').trigger('keyup.enter')
    expect(w.emitted('enter')).toBeFalsy()
    // y perder el foco entre la pulsacion y la suelta tampoco lo deja a medias
    await w.find('input').trigger('keydown.enter')
    await w.find('input').trigger('blur')
    await w.find('input').trigger('keyup.enter')
    expect(w.emitted('enter')).toBeFalsy()
  })
})

describe('SelectField', () => {
  const options = [
    { v: 'a', n: 'Alfa' },
    { v: 'b', n: 'Beta' },
    { v: 'c', n: 'Gamma' }
  ]
  const mountView = (value = 'a') => mount(SelectField, { props: { modelValue: value, options } })

  it('shows the chosen option', () => {
    expect(mountView('b').find('.select-text').text()).toBe('Beta')
  })
  it('opens and closes the menu', async () => {
    const w = mountView()
    expect(w.find('.select-menu').exists()).toBe(false)
    await w.find('.select-box').trigger('click')
    expect(w.find('.select-menu').exists()).toBe(true)
    expect(w.findAll('.select-opt')).toHaveLength(3)
    await w.find('.select-box').trigger('click')
    expect(w.find('.select-menu').exists()).toBe(false)
  })
  it('choosing an option emits it and closes', async () => {
    const w = mountView()
    await w.find('.select-box').trigger('click')
    await w.findAll('.select-opt')[2].trigger('click')
    expect(w.emitted('update:modelValue').at(-1)).toEqual(['c'])
    expect(w.find('.select-menu').exists()).toBe(false)
  })
  it('is keyboard operable', async () => {
    const w = mountView('a')
    await w.find('.select-box').trigger('keydown', { key: 'ArrowDown' })
    expect(w.find('.select-menu').exists()).toBe(true)
    await w.find('.select-box').trigger('keydown', { key: 'ArrowDown' })
    await w.find('.select-box').trigger('keydown', { key: 'Enter' })
    expect(w.emitted('update:modelValue').at(-1)).toEqual(['b'])
  })
  it('Escape closes without choosing', async () => {
    const w = mountView()
    await w.find('.select-box').trigger('click')
    await w.find('.select-box').trigger('keydown', { key: 'Escape' })
    expect(w.find('.select-menu').exists()).toBe(false)
    expect(w.emitted('update:modelValue')).toBeFalsy()
  })
  it('marks which one is active', async () => {
    const w = mountView('b')
    await w.find('.select-box').trigger('click')
    expect(w.findAll('.select-opt')[1].classes()).toContain('current')
  })
})

describe('ToggleField', () => {
  it('toggles on click', async () => {
    const w = mount(ToggleField, { props: { modelValue: false, title: 'Convertir' } })
    await w.find('.toggle-track').trigger('click')
    expect(w.emitted('update:modelValue').at(-1)).toEqual([true])
  })
  it('reflects the on state', () => {
    const w = mount(ToggleField, { props: { modelValue: true } })
    expect(w.find('.toggle-track').classes()).toContain('on')
    expect(w.find('.toggle-track').attributes('aria-checked')).toBe('true')
  })
  it('the label toggles too', async () => {
    const w = mount(ToggleField, { props: { modelValue: false, title: 'X' } })
    await w.find('.toggle-txt').trigger('click')
    expect(w.emitted('update:modelValue')).toBeTruthy()
  })
})

describe('SliderField', () => {
  it('computes the fill percentage', () => {
    const w = mount(SliderField, { props: { modelValue: 5, min: 0, max: 10 } })
    expect(w.find('.slider-track').attributes('style')).toContain('50%')
  })
  it('emits numbers, not strings', async () => {
    const w = mount(SliderField, { props: { modelValue: 0, min: 0, max: 10, step: 1 } })
    await w.find('input').setValue('7')
    expect(w.emitted('update:modelValue').at(-1)).toEqual([7])
  })

  // El volumen va a Rust y vuelve un momento despues. Si ese eco, ya viejo,
  // se escribia en el <input> a mitad de arrastre, la bola saltaba atras.
  it('while dragging, a stale echo of the model does not move the thumb', async () => {
    const w = mount(SliderField, { props: { modelValue: 0.5, min: 0, max: 1, step: 0.01 } })
    const input = w.find('input')
    await input.trigger('pointerdown')
    await input.setValue('0.8')
    expect(w.emitted('update:modelValue').at(-1)).toEqual([0.8])
    await w.setProps({ modelValue: 0.6 }) // lo que Rust confirma de un valor anterior
    expect(input.element.value).toBe('0.8')
    expect(w.find('.slider-track').attributes('style')).toContain('80%')
    await input.trigger('pointerup')
    // hasta que confirme lo ultimo enviado, sigue donde se dejo
    await w.setProps({ modelValue: 0.7 })
    expect(input.element.value).toBe('0.8')
    await w.setProps({ modelValue: 0.8 })
    expect(input.element.value).toBe('0.8')
    // y a partir de ahi vuelve a obedecer al modelo (otra ventana, un atajo…)
    await w.setProps({ modelValue: 0.3 })
    expect(input.element.value).toBe('0.3')
  })

  it('untouched, it follows the model', async () => {
    const w = mount(SliderField, { props: { modelValue: 0.5, min: 0, max: 1, step: 0.01 } })
    await w.setProps({ modelValue: 0.2 })
    expect(w.find('input').element.value).toBe('0.2')
    expect(w.find('.slider-track').attributes('style')).toContain('20%')
  })
})

describe('ColorField', () => {
  it('emits from both the swatch and the hex', async () => {
    const w = mount(ColorField, { props: { modelValue: '#ff0000', title: 'Acento' } })
    await w.find('input[type="color"]').setValue('#00ff00')
    expect(w.emitted('update:modelValue').at(-1)).toEqual(['#00ff00'])
    await w.find('.color-hex').setValue('#0000ff')
    expect(w.emitted('update:modelValue').at(-1)).toEqual(['#0000ff'])
  })
})

describe('no bare native controls', () => {
  const views = [
    ...readdirSync('src/components')
      .filter((f) => f.endsWith('.vue'))
      .map((f) => 'src/components/' + f),
    'src/App.vue'
  ]
  it('there is no select, checkbox, range or color outside the library', () => {
    const bad = []
    for (const f of views) {
      const txt = readFileSync(f, 'utf8')
      for (const pattern of [
        /<select[\s>]/,
        /<input[^>]*type="checkbox"/,
        /<input[^>]*type="range"/,
        /<input[^>]*type="color"/
      ]) {
        if (pattern.test(txt)) bad.push(`${f}: ${pattern}`)
      }
    }
    expect(bad, 'usa los componentes de ui/: ' + bad.join(', ')).toEqual([])
  })
  it('there are no unwrapped text inputs', () => {
    const bad = []
    for (const f of views) {
      if (/<input(?![^>]*type="(checkbox|range|color)")/.test(readFileSync(f, 'utf8'))) bad.push(f)
    }
    expect(bad, 'usa TextField.vue: ' + bad.join(', ')).toEqual([])
  })
})

// Lo que se usa en varios sitios vive en components/ui/ y se reutiliza. Estas
// pruebas existen porque la caratula con su hueco estaba escrita tres veces
// (una de ellas manipulando el DOM a mano, con una clase que ya no existia) y
// la rejilla se descuadraba cuando una cancion no tenia portada.
describe('repeated markup lives in a single component', () => {
  const views = [
    ...readdirSync('src/components')
      .filter((f) => f.endsWith('.vue'))
      .map((f) => 'src/components/' + f),
    'src/App.vue'
  ]
  const read = (f) => readFileSync(f, 'utf8')

  it('nobody builds the cover by hand: CoverArt is used', () => {
    const bad = views.filter((f) => /<img[^>]*coverUrl/.test(read(f)))
    expect(bad, 'usa ui/CoverArt.vue: ' + bad.join(', ')).toEqual([])
  })

  it('nobody replaces DOM nodes by hand', () => {
    const bad = views.filter((f) => /replaceWith\(|createElement\(/.test(read(f)))
    expect(bad, 'deja que Vue pinte: ' + bad.join(', ')).toEqual([])
  })

  it('the spinner is rendered with Loading, not standalone', () => {
    const bad = views.filter(
      (f) => /class="spinner"/.test(read(f)) && !/chat-downloading/.test(read(f))
    )
    expect(bad, 'usa ui/Loading.vue: ' + bad.join(', ')).toEqual([])
  })

  it('empty placeholders are rendered with EmptyState', () => {
    const bad = views.filter((f) => /<div[^>]*class="empty"/.test(read(f)))
    expect(bad, 'usa ui/EmptyState.vue: ' + bad.join(', ')).toEqual([])
  })

  it('titled cards are rendered with Card', () => {
    const bad = views.filter((f) => /<div class="card"[^>]*>\s*\n\s*<h3/.test(read(f)))
    expect(bad, 'usa ui/Card.vue: ' + bad.join(', ')).toEqual([])
  })

  it('CoverArt is the only one that knows about failing covers', () => {
    const outside = views
      .filter((f) => !f.endsWith('CoverArt.vue'))
      .filter((f) => /coverUrlAlt/.test(read(f)))
    expect(outside, 'el respaldo de URL vive en CoverArt: ' + outside.join(', ')).toEqual([])
  })
})

// Estaba al reves: al pasar el raton por la 3ª se pintaban la 3, 4 y 5. La
// causa es que CSS solo sabe seleccionar hermanos POSTERIORES (`~`), asi que
// el marcado va 5..1 y el contenedor lo endereza con row-reverse.
describe('star rating', () => {
  const mountStars = (props = {}) => mount(StarRating, { props: { value: 0, ...props } })

  it('the markup goes from 5 to 1', () => {
    const w = mountStars({ value: 0 })
    const titles = w.findAll('.ico').map((i) => i.attributes('title'))
    expect(titles).toEqual(['5 de 5', '4 de 5', '3 de 5', '2 de 5', '1 de 5'])
  })

  it('with 3 stars, 1, 2 and 3 are lit', () => {
    const w = mountStars({ value: 3 })
    const icons = w.findAll('.ico')
    // el marcado va 5,4,3,2,1: las llenas son las tres ultimas
    const filled = icons.map((i) => i.classes().includes('on'))
    expect(filled).toEqual([false, false, true, true, true])
    // la de la nota actual ofrece quitarla; las de debajo, ponerse
    expect(icons[2].attributes('title')).toBe('Quitar valoración')
    expect(icons[3].attributes('title')).toBe('2 de 5')
  })

  it('clicking a star sends its value, not the index', async () => {
    const w = mountStars({ value: 0 })
    const fourth = w.findAll('.ico').find((i) => i.attributes('title') === '4 de 5')
    await fourth.trigger('click')
    expect(w.emitted('change')[0]).toEqual([4])
  })

  it('clicking the current rating clears it', async () => {
    const w = mountStars({ value: 3 })
    const third = w.findAll('.ico').find((i) => i.attributes('title') === 'Quitar valoración')
    await third.trigger('click')
    expect(w.emitted('change')[0]).toEqual([0])
  })

  it('in read-only mode it emits nothing', async () => {
    const w = mountStars({ value: 2, editable: false })
    await w.findAll('.ico')[0].trigger('click')
    expect(w.emitted('change')).toBeFalsy()
  })
})

// Editar la ficha escribe en las ETIQUETAS del mp3, no en la base: la base es
// solo un indice reconstruible. Estas pruebas fijan las reglas de la edicion.
describe('editing the song details', () => {
  const song = {
    id: 7,
    title: 'Mi Gozo',
    artist: 'Barak',
    album: '',
    year: '',
    genre: '',
    feat: '',
    key: '',
    bpm: 0,
    lyrics: '',
    stars: 0,
    favorite: 0,
    duration: 200,
    bitrate: 128000,
    folder: 'Artistas/Barak',
    file: 'x.mp3'
  }
  const mountView = () => mount(DetailsPanel, { props: { song: { ...song }, aiReady: true } })
  const openEditor = async (w) => {
    await w.find('.icon-btn').trigger('click')
    return w
  }

  it('the pencil opens the form', async () => {
    const w = mountView()
    expect(w.find('.edit-form').exists()).toBe(false)
    await openEditor(w)
    expect(w.find('.edit-form').exists()).toBe(true)
  })

  it('without changes only cancel is offered', async () => {
    const w = await openEditor(mountView())
    const texts = w.findAll('.edit-actions .btn').map((b) => b.text())
    expect(texts).toEqual(['Cancelar'])
    expect(w.find('.edit-state').text()).toBe('Sin cambios')
  })

  it('touching something shows save and says how many fields', async () => {
    const w = await openEditor(mountView())
    await w.findAll('.edit-form input')[3].setValue('Generacion Radical') // album
    const texts = w.findAll('.edit-actions .btn').map((b) => b.text())
    expect(texts).toContain('Guardar')
    expect(texts).toContain('Descartar')
    expect(w.find('.edit-state').text()).toBe('1 sin guardar')
  })

  it('going back to the original value leaves it unchanged again', async () => {
    const w = await openEditor(mountView())
    const album = w.findAll('.edit-form input')[3]
    await album.setValue('Algo')
    expect(w.find('.edit-state').text()).toBe('1 sin guardar')
    await album.setValue('')
    expect(w.find('.edit-state').text()).toBe('Sin cambios')
    expect(w.findAll('.edit-actions .btn').map((b) => b.text())).toEqual(['Cancelar'])
  })

  it('Escape closes without saving', async () => {
    const w = await openEditor(mountView())
    await w.findAll('.edit-form input')[3].setValue('Algo')
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }))
    await w.vm.$nextTick()
    expect(w.find('.edit-form').exists()).toBe(false)
    expect(api.edit).not.toHaveBeenCalled()
  })

  it('saving sends ONLY what changed', async () => {
    api.edit.mockResolvedValueOnce({ ...song, album: 'Generacion Radical' })
    const w = await openEditor(mountView())
    await w.findAll('.edit-form input')[3].setValue('Generacion Radical')
    // jsdom no propaga el envio del formulario al pulsar el boton submit
    await w.find('.edit-form').trigger('submit')
    await flushPromises()
    expect(api.edit).toHaveBeenCalledWith(7, { album: 'Generacion Radical' })
  })

  it('bpm is sent as a number, not as text', async () => {
    api.edit.mockResolvedValueOnce({ ...song, bpm: 128 })
    const w = await openEditor(mountView())
    const fields = w.findAll('.edit-form input')
    await fields[fields.length - 1].setValue('128') // bpm es el ultimo input
    await w.find('.edit-form').trigger('submit')
    await flushPromises()
    expect(api.edit).toHaveBeenCalledWith(7, { bpm: 128 })
  })

  it('changing song closes editing', async () => {
    const w = await openEditor(mountView())
    await w.setProps({ song: { ...song, id: 8, title: 'Otra' } })
    expect(w.find('.edit-form').exists()).toBe(false)
  })
})

// Las respuestas llegan cuando llegan. Lo que se pidio para una cancion no
// se pinta en la ficha de otra que se eligio mientras tanto.
describe('the details panel and late responses', () => {
  const song = {
    id: 7,
    title: 'Mi Gozo',
    artist: 'Barak',
    album: 'x',
    year: '2019',
    genre: 'x',
    key: 'G',
    bpm: 0,
    lyrics: '',
    stars: 0,
    favorite: 0,
    duration: 200,
    bitrate: 128000,
    folder: 'x',
    file: 'x.mp3'
  }
  const pending = () => {
    let release
    const promise = new Promise((r) => {
      release = r
    })
    return { promise, release: (v) => release(v) }
  }
  const button = (w, text) => w.findAll('button').find((b) => b.text().includes(text))
  /** un cifrado como los que guarda el nucleo */
  const chordChart = (key, chords, extra = {}) => ({
    source: 'Ultimate Guitar',
    url: 'https://tabs.example/1',
    artist: 'Barak',
    title: 'Mi Gozo',
    key,
    capo: 0,
    votes: 12,
    sections: [
      {
        name: 'Coro',
        chords,
        lines: [
          { t: chords.join('   '), c: true },
          { t: 'Mi gozo esta en ti', c: false }
        ]
      }
    ],
    ...extra
  })

  it('chords are looked up on the web and their source is shown', async () => {
    api.chords.mockResolvedValueOnce({
      sheet: chordChart('D', ['D', 'A']),
      tried: ['Ultimate Guitar', 'LaCuerda'],
      failed: [],
      song: { ...song, chords: JSON.stringify({ sheet: chordChart('D', ['D', 'A']) }) }
    })
    const w = mount(DetailsPanel, { props: { song: { ...song }, aiReady: false } })
    await button(w, 'Buscar acordes').trigger('click')
    expect(api.chords).toHaveBeenCalledWith(7, false)
    await flushPromises()
    const updated = w.emitted('updated').at(-1)[0]
    await w.setProps({ song: updated })
    expect(w.text()).toContain('Tono D')
    expect(w.text()).toContain('12 votos')
    expect(button(w, 'Buscar acordes')).toBeUndefined()
    await button(w, 'Ultimate Guitar').trigger('click')
    expect(app.openInBrowser).toHaveBeenCalledWith('https://tabs.example/1')
  })

  it('if there is no chord chart it says so, and it can search again', async () => {
    const chords = JSON.stringify({ sheet: null, sheet_tried: ['Ultimate Guitar', 'LaCuerda'] })
    api.chords.mockResolvedValueOnce({ sheet: null, tried: [], failed: [], song: null })
    const w = mount(DetailsPanel, { props: { song: { ...song, chords }, aiReady: true } })
    expect(w.text()).toContain('No hay cifrado de esta canción en Ultimate Guitar ni LaCuerda')
    expect(w.find('.chords').exists()).toBe(false)
    await button(w, 'Buscar otra vez').trigger('click')
    expect(api.chords).toHaveBeenCalledWith(7, true)
  })

  it('without a key in the chart none is set: it moves up or down by semitones', async () => {
    const chords = JSON.stringify({ sheet: chordChart('', ['C', 'G']) })
    api.chordsTransposed.mockResolvedValueOnce({ sheet: chordChart('', ['D', 'A']) })
    const w = mount(DetailsPanel, { props: { song: { ...song, chords }, aiReady: true } })
    expect(w.text()).toContain('El cifrado no dice el tono')
    const field = w.findAll('.field').find((f) => f.text().includes('como está'))
    await field.find('.select-box').trigger('click')
    await w
      .findAll('.select-opt')
      .find((o) => o.text() === '+2 semitonos')
      .trigger('click')
    await flushPromises()
    expect(api.chordsTransposed).toHaveBeenCalledWith(7, { semitones: 2 })
    expect(w.find('.chords').text()).toBe('D   A')
  })

  it('the full chart puts the chords over the lyrics', async () => {
    const chords = JSON.stringify({ sheet: chordChart('G', ['G', 'C']) })
    const w = mount(DetailsPanel, { props: { song: { ...song, chords }, aiReady: true } })
    expect(w.text()).not.toContain('Mi gozo esta en ti')
    await button(w, 'Cifrado entero').trigger('click')
    expect(w.find('.sheet-full .lyric-line').text()).toBe('Mi gozo esta en ti')
    expect(w.find('.sheet-full .chord-line').text()).toBe('G   C')
  })

  it('the AI details of A are not rendered in the details of B', async () => {
    const p = pending()
    api.details.mockReturnValueOnce(p.promise)
    const w = mount(DetailsPanel, { props: { song: { ...song }, aiReady: true } })
    await button(w, 'Ver detalles IA').trigger('click')
    await w.setProps({ song: { ...song, id: 8, title: 'Otra' } })
    // la otra no se queda bloqueada esperando lo de la primera
    expect(button(w, 'Ver detalles IA').attributes('disabled')).toBeUndefined()
    p.release({ details: { about_the_song: 'Habla del gozo', confidence: 0.9 } })
    await flushPromises()
    expect(w.text()).not.toContain('Habla del gozo')
  })

  it('the transposition of A is not rendered in the details of B', async () => {
    const chords = JSON.stringify({ sheet: chordChart('G', ['G', 'D', 'Em', 'C']) })
    const p = pending()
    api.chordsTransposed.mockReturnValueOnce(p.promise)
    const w = mount(DetailsPanel, { props: { song: { ...song, chords }, aiReady: true } })
    const target = w.findAll('.field').find((f) => f.text().includes('—'))
    await target.find('.select-box').trigger('click')
    await w
      .findAll('.select-opt')
      .find((o) => o.text() === 'A')
      .trigger('click')
    await w.setProps({
      song: {
        ...song,
        id: 8,
        chords: JSON.stringify({ sheet: chordChart('E', ['E', 'B', 'C#m', 'A']) })
      }
    })
    p.release({ sheet: chordChart('A', ['A', 'E', 'F#m', 'D']) })
    await flushPromises()
    expect(w.find('.chords').text()).toBe('E   B   C#m   A')
  })

  it('a failure of A does not appear in the details of B', async () => {
    const p = pending()
    api.enrich.mockReturnValueOnce(
      p.promise.then(() => {
        throw new Error('sin red')
      })
    )
    const w = mount(DetailsPanel, { props: { song: { ...song }, aiReady: true } })
    await button(w, 'Buscar letra').trigger('click')
    await w.setProps({ song: { ...song, id: 8, title: 'Otra' } })
    p.release()
    await flushPromises()
    expect(w.text()).not.toContain('sin red')
  })
})

// La tarjeta estaba escrita a mano veinte veces. Al unificarla, el hueco del
// titulo trae su propio <h3> (hay titulos con icono o con una cuenta al lado),
// asi que lo que hay que vigilar es que no acaben anidados.
describe('Card', () => {
  it('renders the title and the note', () => {
    const w = mount(Card, { props: { title: 'Apariencia', note: 'Se aplica al instante' } })
    expect(w.find('h3').text()).toBe('Apariencia')
    expect(w.find('.note').text()).toBe('Se aplica al instante')
  })

  it('without title or note it leaves no empty gaps', () => {
    const w = mount(Card, { slots: { default: '<p>algo</p>' } })
    expect(w.find('h3').exists()).toBe(false)
    expect(w.find('.note').exists()).toBe(false)
    expect(w.text()).toBe('algo')
  })

  it('a title with its own markup is not nested inside another', () => {
    const w = mount(Card, { slots: { title: '<h3>Historial <span class="chip">4</span></h3>' } })
    expect(w.findAll('h3'), 'el <h3> quedo dentro de otro <h3>').toHaveLength(1)
    expect(w.find('h3 .chip').text()).toBe('4')
  })

  it('keeps the classes and styles passed to it', () => {
    const w = mount(Card, { attrs: { class: 'dl-dup' }, props: { title: 'x' } })
    expect(w.classes()).toContain('card')
    expect(w.classes()).toContain('dl-dup')
  })
})
