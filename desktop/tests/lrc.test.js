import { describe, it, expect, vi } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import { parseLrc, stripLrc, currentLine } from '../src/utils/lrc.js'
import SyncedLyrics from '../src/components/ui/SyncedLyrics.vue'

const LRC =
  '[ar:Barak]\n[00:12.50]Mi gozo\n[00:15.00]es el Señor\n[00:20.25][01:02.00]coro repetido\n\n[00:30.00]'

describe('letra con tiempos (LRC)', () => {
  it('se parsea en lineas ordenadas, con las marcas repetidas desdobladas', () => {
    const lines = parseLrc(LRC)
    expect(lines.map((l) => [l.t, l.text])).toEqual([
      [12.5, 'Mi gozo'],
      [15, 'es el Señor'],
      [20.25, 'coro repetido'],
      [30, ''],
      [62, 'coro repetido']
    ])
  })
  it('una letra normal no es LRC', () => {
    expect(parseLrc('Mi gozo\nes el Señor')).toBeNull()
    expect(parseLrc('')).toBeNull()
    expect(parseLrc('[00:01.00]una sola marca')).toBeNull()
  })
  it('se puede leer sin las marcas', () => {
    expect(stripLrc(LRC)).toBe('Mi gozo\nes el Señor\ncoro repetido')
    expect(stripLrc('sin marcas')).toBe('sin marcas')
  })
  it('sabe que linea suena', () => {
    const lines = parseLrc(LRC)
    expect(currentLine(lines, 0)).toBe(-1)
    expect(currentLine(lines, 12.5)).toBe(0)
    expect(currentLine(lines, 16)).toBe(1)
    expect(currentLine(lines, 61.9)).toBe(3)
    expect(currentLine(lines, 100)).toBe(4)
  })
})

describe('la letra que sigue a la cancion', () => {
  const lines = parseLrc(LRC)
  it('resalta la linea que suena y pulsar otra salta ahi', async () => {
    const w = mount(SyncedLyrics, { props: { lines, position: 16, active: true } })
    const rows = w.findAll('.lrc-line')
    expect(rows[1].classes()).toContain('current')
    expect(rows[0].classes()).toContain('past')
    expect(rows[2].classes()).not.toContain('current')
    await rows[2].trigger('click')
    expect(w.emitted('seek').at(-1)).toEqual([20.25])
  })
  it('sigue la linea moviendo solo la caja de la letra, nunca la ficha entera', async () => {
    // scrollIntoView movia tambien lo que contiene la letra: en WebKitGTK
    // subia la ficha entera y la letra se quedaba pegada en medio
    const vista = vi.fn()
    Element.prototype.scrollIntoView = vista
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
    expect(vista).not.toHaveBeenCalled()
    // quien mueve la letra a mano manda un rato: no se le quita de donde la puso
    await w.find('.lrc').trigger('wheel')
    box.scrollTo.mockClear()
    await w.setProps({ position: 40 })
    await flushPromises()
    expect(box.scrollTo).not.toHaveBeenCalled()
    delete Element.prototype.scrollIntoView
  })
  it('si no es la que suena, no resalta ni salta', async () => {
    const w = mount(SyncedLyrics, { props: { lines, position: 16, active: false } })
    expect(w.findAll('.lrc-line.current')).toHaveLength(0)
    await w.findAll('.lrc-line')[2].trigger('click')
    expect(w.emitted('seek')).toBeUndefined()
  })
})
