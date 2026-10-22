import { describe, it, expect } from 'vitest'
import { parseKey, transposeKey, semitoneLabel, toneLabel, toneUnit } from '../src/utils/theory.js'

describe('transposing the key name', () => {
  it('understands major, minor, sharps and flats', () => {
    expect(parseKey('G')).toEqual([7, false, ''])
    expect(parseKey('F#m')).toEqual([6, true, ''])
    expect(parseKey('Bb')).toEqual([10, false, ''])
    expect(parseKey('Ebm')).toEqual([3, true, ''])
    expect(parseKey('cosa')).toBeNull()
    expect(parseKey('')).toBeNull()
  })
  it('goes up and down, and wraps around the octave', () => {
    expect(transposeKey('G', 2)).toBe('A')
    expect(transposeKey('G', -2)).toBe('F')
    expect(transposeKey('B', 1)).toBe('C')
    expect(transposeKey('C', -1)).toBe('B')
    expect(transposeKey('Am', 3)).toBe('Cm')
    expect(transposeKey('G', 12)).toBe('G')
  })
  it('spells accidentals as they are read: flats for Bb/Eb/Ab, sharps for F#/C#m', () => {
    expect(transposeKey('C', 1)).toBe('Db')
    expect(transposeKey('C', 6)).toBe('F#')
    expect(transposeKey('Am', 4)).toBe('C#m')
    // y respeta el estilo del original
    expect(transposeKey('Bb', 1)).toBe('B')
    expect(transposeKey('Bb', 3)).toBe('Db')
    expect(transposeKey('F#', 1)).toBe('G')
    expect(transposeKey('F#', 2)).toBe('G#')
  })
  it('labels the shift', () => {
    expect(semitoneLabel(0)).toBe('0')
    expect(semitoneLabel(2)).toBe('+2')
    expect(semitoneLabel(-3)).toBe('−3')
  })
  it('counts the shift in tones: a half tone is a semitone', () => {
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
  it('with a quarter tone it says the starting key and how much it shifts', () => {
    expect(transposeKey('G', 0.5)).toBe('G +¼')
    expect(transposeKey('G', 1.5)).toBe('Ab +¼')
    expect(transposeKey('G', -0.5)).toBe('G −¼')
    expect(transposeKey('Am', -1.5)).toBe('G#m −¼')
    expect(transposeKey('G', 2)).toBe('A')
  })
})

describe('the pitch shift in words', () => {
  it('says it like a musician: half a tone, 1 tone, 1 and a half tones, a quarter', async () => {
    const { toneWords, semitoneWords } = await import('../src/utils/theory.js')
    expect(toneWords(0)).toBe('original')
    expect(toneWords(1)).toBe('medio tono arriba')
    expect(toneWords(-2)).toBe('1 tono abajo')
    expect(toneWords(3)).toBe('1 tono y medio arriba')
    expect(toneWords(0.5)).toBe('un cuarto de tono arriba')
    expect(toneWords(-1.5)).toBe('tres cuartos de tono abajo')
    expect(toneWords(4)).toBe('2 tonos arriba')
    expect(toneWords(2.5)).toBe('1 tono y cuarto arriba')
    expect(toneWords(5, { direction: false })).toBe('2 tonos y medio')
    expect(semitoneWords(1)).toBe('1 semitono')
    expect(semitoneWords(-3)).toBe('3 semitonos')
    expect(semitoneWords(0.5)).toBe('½ semitono')
  })
})
