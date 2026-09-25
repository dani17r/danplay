import { describe, it, expect } from 'vitest'
import { parseKey, transposeKey, semitoneLabel, toneLabel, toneUnit } from '../src/utils/theory.js'

describe('transponer el nombre del tono', () => {
  it('entiende mayores, menores, sostenidos y bemoles', () => {
    expect(parseKey('G')).toEqual([7, false, ''])
    expect(parseKey('F#m')).toEqual([6, true, ''])
    expect(parseKey('Bb')).toEqual([10, false, ''])
    expect(parseKey('Ebm')).toEqual([3, true, ''])
    expect(parseKey('cosa')).toBeNull()
    expect(parseKey('')).toBeNull()
  })
  it('sube y baja, y da la vuelta a la octava', () => {
    expect(transposeKey('G', 2)).toBe('A')
    expect(transposeKey('G', -2)).toBe('F')
    expect(transposeKey('B', 1)).toBe('C')
    expect(transposeKey('C', -1)).toBe('B')
    expect(transposeKey('Am', 3)).toBe('Cm')
    expect(transposeKey('G', 12)).toBe('G')
  })
  it('escribe los alterados como se leen: bemoles para Bb/Eb/Ab, sostenidos para F#/C#m', () => {
    expect(transposeKey('C', 1)).toBe('Db')
    expect(transposeKey('C', 6)).toBe('F#')
    expect(transposeKey('Am', 4)).toBe('C#m')
    // y respeta el estilo del original
    expect(transposeKey('Bb', 1)).toBe('B')
    expect(transposeKey('Bb', 3)).toBe('Db')
    expect(transposeKey('F#', 1)).toBe('G')
    expect(transposeKey('F#', 2)).toBe('G#')
  })
  it('etiqueta el corrimiento', () => {
    expect(semitoneLabel(0)).toBe('0')
    expect(semitoneLabel(2)).toBe('+2')
    expect(semitoneLabel(-3)).toBe('−3')
  })
  it('cuenta el corrimiento en tonos: medio tono es un semitono', () => {
    expect(toneLabel(0)).toBe('0')
    expect(toneLabel(1)).toBe('+½')
    expect(toneLabel(2)).toBe('+1')
    expect(toneLabel(3)).toBe('+1½')
    expect(toneLabel(-2)).toBe('−1')
    // un cuarto de tono es medio semitono
    expect(toneLabel(0.5)).toBe('+¼')
    expect(toneLabel(1.5)).toBe('+¾')
    expect(toneLabel(-2.5)).toBe('−1¼')
    expect(toneLabel(12)).toBe('+6')
    // lo que no es un cuarto se dice con decimales
    expect(toneLabel(0.3)).toBe('+0.15')
    expect(toneUnit(1)).toBe('tono')
    expect(toneUnit(-2)).toBe('tono')
    expect(toneUnit(3)).toBe('tonos')
  })
  it('con un cuarto de tono dice de que tono parte y cuanto se corre', () => {
    expect(transposeKey('G', 0.5)).toBe('G +¼')
    expect(transposeKey('G', 1.5)).toBe('Ab +¼')
    expect(transposeKey('G', -0.5)).toBe('G −¼')
    expect(transposeKey('Am', -1.5)).toBe('G#m −¼')
    expect(transposeKey('G', 2)).toBe('A')
  })
})
