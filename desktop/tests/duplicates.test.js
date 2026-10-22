import { describe, it, expect } from 'vitest'
import { mount } from '@vue/test-utils'
import DuplicateGroup from '../src/components/DuplicateGroup.vue'

const group = {
  suggested: '/m/Barak - Mi Gozo - r.mp3',
  items: [
    {
      id: 1,
      path: '/m/Barak - Mi Gozo.mp3',
      relative: 'Barak/Mi Gozo.mp3',
      file: 'Barak - Mi Gozo.mp3',
      artist: 'Barak',
      title: 'Mi Gozo',
      duration: 200,
      bitrate: 128000,
      size: 3200000,
      stars: 0,
      favorite: 0,
      has_suffix: false
    },
    {
      id: 2,
      path: '/m/Barak - Mi Gozo - r.mp3',
      relative: 'Barak/Mi Gozo - r.mp3',
      file: 'Barak - Mi Gozo - r.mp3',
      artist: 'Barak',
      title: 'Mi Gozo',
      duration: 201,
      bitrate: 320000,
      size: 8100000,
      stars: 4,
      favorite: 1,
      has_suffix: true
    }
  ]
}

describe('duplicates group', () => {
  it('lists every copy with its details', () => {
    const w = mount(DuplicateGroup, { props: { group, playing: null, busy: false } })
    expect(w.findAll('.dup-row')).toHaveLength(2)
    expect(w.text()).toContain('128 kbps')
    expect(w.text()).toContain('320 kbps')
    expect(w.text()).toContain('3:20')
    expect(w.text()).toContain('7.7 MB')
  })

  it('marks which one has the best quality and which has the suffix', () => {
    const w = mount(DuplicateGroup, { props: { group, playing: null, busy: false } })
    const withSuffix = w.findAll('.dup-row')[1]
    expect(withSuffix.text()).toContain('mejor calidad')
    expect(withSuffix.text()).toContain('lleva « - r»')
    expect(withSuffix.classes()).toContain('suggested')
  })

  it('each copy can be played', async () => {
    const w = mount(DuplicateGroup, { props: { group, playing: null, busy: false } })
    await w.findAll('.dup-play')[1].trigger('click')
    expect(w.emitted('play')[0][0].id).toBe(2)
  })

  it('marks the one that is playing', () => {
    const w = mount(DuplicateGroup, { props: { group, playing: 2, busy: false } })
    expect(w.findAll('.dup-row')[1].classes()).toContain('playing')
  })

  it('on the playing one the button is pause, and while paused, resume', () => {
    // Pintaba la pausa y al pulsarla volvia a empezar la copia.
    const playing = mount(DuplicateGroup, {
      props: { group, playing: 2, paused: false, busy: false }
    })
    const [other, current] = playing.findAll('.dup-play')
    expect(current.attributes('title')).toBe('Pausar')
    expect(current.findComponent({ name: 'Icon' }).props('n')).toBe('pause')
    expect(other.attributes('title')).toBe('Escuchar')
    const paused = mount(DuplicateGroup, {
      props: { group, playing: 2, paused: true, busy: false }
    })
    expect(paused.findAll('.dup-play')[1].attributes('title')).toBe('Reanudar')
    expect(paused.findAll('.dup-play')[1].findComponent({ name: 'Icon' }).props('n')).toBe('play')
  })

  it('keep emits the group and the chosen copy', async () => {
    const w = mount(DuplicateGroup, { props: { group, playing: null, busy: false } })
    await w.findAll('.dup-keep')[1].trigger('click')
    const [g, picked] = w.emitted('keepOne')[0]
    expect(picked.path).toBe('/m/Barak - Mi Gozo - r.mp3')
    expect(g.items).toHaveLength(2)
  })

  it('while resolving, the buttons are locked', () => {
    const w = mount(DuplicateGroup, { props: { group, playing: null, busy: true } })
    expect(w.findAll('.dup-keep')[0].attributes('disabled')).toBeDefined()
  })

  it('shows the stars and favorite of each copy', () => {
    const w = mount(DuplicateGroup, { props: { group, playing: null, busy: false } })
    expect(w.findAll('.stars .ico.on')).toHaveLength(4)
  })

  it('an unindexed copy cannot be played', () => {
    const g2 = { ...group, items: [{ ...group.items[0], id: null }] }
    const w = mount(DuplicateGroup, { props: { group: g2, playing: null, busy: false } })
    expect(w.find('.dup-play').attributes('disabled')).toBeDefined()
  })
})
