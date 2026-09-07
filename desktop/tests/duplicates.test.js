import { describe, it, expect } from 'vitest'
import { mount } from '@vue/test-utils'
import DuplicateGroup from '../src/components/DuplicateGroup.vue'

const group = {
  suggested: '/m/Barak - Mi Gozo - r.mp3',
  items: [
    { id: 1, path: '/m/Barak - Mi Gozo.mp3', relativa: 'Barak/Mi Gozo.mp3',
      file: 'Barak - Mi Gozo.mp3', artist: 'Barak', title: 'Mi Gozo',
      duration: 200, bitrate: 128000, size: 3200000, stars: 0, favorite: 0,
      tiene_sufijo: false },
    { id: 2, path: '/m/Barak - Mi Gozo - r.mp3', relativa: 'Barak/Mi Gozo - r.mp3',
      file: 'Barak - Mi Gozo - r.mp3', artist: 'Barak', title: 'Mi Gozo',
      duration: 201, bitrate: 320000, size: 8100000, stars: 4, favorite: 1,
      tiene_sufijo: true }
  ]
}

describe('grupo de duplicados', () => {
  it('lista todas las copias con sus datos', () => {
    const w = mount(DuplicateGroup, { props: { group, playing: null, busy: false } })
    expect(w.findAll('.dup-row')).toHaveLength(2)
    expect(w.text()).toContain('128 kbps')
    expect(w.text()).toContain('320 kbps')
    expect(w.text()).toContain('3:20')
    expect(w.text()).toContain('7.7 MB')
  })

  it('marca cual tiene mejor calidad y cual lleva el sufijo', () => {
    const w = mount(DuplicateGroup, { props: { group, playing: null, busy: false } })
    const conSufijo = w.findAll('.dup-row')[1]
    expect(conSufijo.text()).toContain('mejor calidad')
    expect(conSufijo.text()).toContain('lleva « - r»')
    expect(conSufijo.classes()).toContain('suggested')
  })

  it('cada copia se puede escuchar', async () => {
    const w = mount(DuplicateGroup, { props: { group, playing: null, busy: false } })
    await w.findAll('.dup-play')[1].trigger('click')
    expect(w.emitted('play')[0][0].id).toBe(2)
  })

  it('marca la que esta sonando', () => {
    const w = mount(DuplicateGroup, { props: { group, playing: 2, busy: false } })
    expect(w.findAll('.dup-row')[1].classes()).toContain('playing')
  })

  it('mantener emite el grupo y la copia elegida', async () => {
    const w = mount(DuplicateGroup, { props: { group, playing: null, busy: false } })
    await w.findAll('.dup-keep')[1].trigger('click')
    const [g, elegida] = w.emitted('keepOne')[0]
    expect(elegida.path).toBe('/m/Barak - Mi Gozo - r.mp3')
    expect(g.items).toHaveLength(2)
  })

  it('mientras se resuelve, los botones se bloquean', () => {
    const w = mount(DuplicateGroup, { props: { group, playing: null, busy: true } })
    expect(w.findAll('.dup-keep')[0].attributes('disabled')).toBeDefined()
  })

  it('muestra estrellas y favorito de cada copia', () => {
    const w = mount(DuplicateGroup, { props: { group, playing: null, busy: false } })
    expect(w.findAll('.stars .ico.on')).toHaveLength(4)
  })

  it('una copia sin indexar no se puede reproducir', () => {
    const g2 = { ...group, items: [{ ...group.items[0], id: null }] }
    const w = mount(DuplicateGroup, { props: { group: g2, playing: null, busy: false } })
    expect(w.find('.dup-play').attributes('disabled')).toBeDefined()
  })
})
