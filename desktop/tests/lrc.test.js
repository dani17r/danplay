import { describe, it, expect } from 'vitest'
import { mount } from '@vue/test-utils'
import { parseLrc, stripLrc, currentLine } from '../src/utils/lrc.js'
import SyncedLyrics from '../src/components/ui/SyncedLyrics.vue'

const LRC = '[ar:Barak]\n[00:12.50]Mi gozo\n[00:15.00]es el Señor\n[00:20.25][01:02.00]coro repetido\n\n[00:30.00]'

describe('letra con tiempos (LRC)', () => {
  it('se parsea en lineas ordenadas, con las marcas repetidas desdobladas', () => {
    const lines = parseLrc(LRC)
    expect(lines.map((l) => [l.t, l.text])).toEqual([
      [12.5, 'Mi gozo'], [15, 'es el Señor'], [20.25, 'coro repetido'], [30, ''], [62, 'coro repetido']
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
  it('si no es la que suena, no resalta ni salta', async () => {
    const w = mount(SyncedLyrics, { props: { lines, position: 16, active: false } })
    expect(w.findAll('.lrc-line.current')).toHaveLength(0)
    await w.findAll('.lrc-line')[2].trigger('click')
    expect(w.emitted('seek')).toBeUndefined()
  })
})
