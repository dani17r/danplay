import { describe, it, expect, vi } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import { parseLrc, stripLrc, currentLine } from '../src/utils/lrc.js'
import SyncedLyrics from '../src/components/ui/SyncedLyrics.vue'

const LRC =
  '[ar:Barak]\n[00:12.50]Mi gozo\n[00:15.00]es el Señor\n[00:20.25][01:02.00]coro repetido\n\n[00:30.00]'

describe('timed lyrics (LRC)', () => {
  it('parses into ordered lines, with repeated timestamps split out', () => {
    const lines = parseLrc(LRC)
    expect(lines.map((l) => [l.t, l.text])).toEqual([
      [12.5, 'Mi gozo'],
      [15, 'es el Señor'],
      [20.25, 'coro repetido'],
      [30, ''],
      [62, 'coro repetido']
    ])
  })
  it('plain lyrics are not LRC', () => {
    expect(parseLrc('Mi gozo\nes el Señor')).toBeNull()
    expect(parseLrc('')).toBeNull()
    expect(parseLrc('[00:01.00]una sola marca')).toBeNull()
  })
  it('can be read without the timestamps', () => {
    expect(stripLrc(LRC)).toBe('Mi gozo\nes el Señor\ncoro repetido')
    expect(stripLrc('sin marcas')).toBe('sin marcas')
  })
  it('knows which line is playing', () => {
    const lines = parseLrc(LRC)
    expect(currentLine(lines, 0)).toBe(-1)
    expect(currentLine(lines, 12.5)).toBe(0)
    expect(currentLine(lines, 16)).toBe(1)
    expect(currentLine(lines, 61.9)).toBe(3)
    expect(currentLine(lines, 100)).toBe(4)
  })
})

describe('lyrics that follow the song', () => {
  const lines = parseLrc(LRC)
  it('highlights the playing line and clicking another jumps there', async () => {
    const w = mount(SyncedLyrics, { props: { lines, position: 16, active: true } })
    const rows = w.findAll('.lrc-line')
    expect(rows[1].classes()).toContain('current')
    expect(rows[0].classes()).toContain('past')
    expect(rows[2].classes()).not.toContain('current')
    await rows[2].trigger('click')
    expect(w.emitted('seek').at(-1)).toEqual([20.25])
  })
  it('follows the line by scrolling only the lyrics box, never the whole details panel', async () => {
    // scrollIntoView movia tambien lo que contiene la letra: en WebKitGTK
    // subia la ficha entera y la letra se quedaba pegada en medio
    const view = vi.fn()
    Element.prototype.scrollIntoView = view
    const many = Array.from({ length: 40 }, (_, i) => ({ t: i * 2, text: 'linea ' + i }))
    const w = mount(SyncedLyrics, { props: { lines: many, position: 0, active: true } })
    const box = w.find('.lrc').element
    Object.defineProperty(box, 'clientHeight', { value: 200, configurable: true })
    Object.defineProperty(box, 'scrollHeight', { value: 800, configurable: true })
    box.getBoundingClientRect = () => ({ top: 100, height: 200 })
    ;[...box.children].forEach((row, i) => {
      row.getBoundingClientRect = () => ({ top: 100 + i * 20, height: 20 })
      Object.defineProperty(row, 'offsetHeight', { value: 20, configurable: true })
    })
    box.scrollTo = vi.fn()
    await w.setProps({ position: 30 }) // la linea 15
    await flushPromises()
    // de 300 a 320 dentro de la caja: su centro, al 40 % de los 200 de alto
    expect(box.scrollTo).toHaveBeenLastCalledWith({ top: 230, behavior: 'smooth' })
    expect(view).not.toHaveBeenCalled()
    // quien mueve la letra a mano manda un rato: no se le quita de donde la puso
    await w.find('.lrc').trigger('wheel')
    box.scrollTo.mockClear()
    await w.setProps({ position: 40 })
    await flushPromises()
    expect(box.scrollTo).not.toHaveBeenCalled()
    delete Element.prototype.scrollIntoView
  })
  it('if it is not the one playing, it neither highlights nor jumps', async () => {
    const w = mount(SyncedLyrics, { props: { lines, position: 16, active: false } })
    expect(w.findAll('.lrc-line.current')).toHaveLength(0)
    await w.findAll('.lrc-line')[2].trigger('click')
    expect(w.emitted('seek')).toBeUndefined()
  })
})
