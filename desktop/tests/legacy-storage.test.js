// Lo que se guardaba en el navegador con nombres en español
// (utils/legacyStorage.js): pasa a su nombre de ahora sin perderse.
import { describe, it, expect, beforeEach } from 'vitest'
import { LEGACY_KEYS, migrateLegacyStorage } from '../src/utils/legacyStorage.js'

beforeEach(() => localStorage.clear())

describe('what was saved under the old names', () => {
  it('moves to the current name and the old one is deleted', () => {
    localStorage.setItem('danplay.vel', '0.8')
    localStorage.setItem('danplay.clic.sonido', '"madera"')
    localStorage.setItem('danplay.movil.toque', '1')
    migrateLegacyStorage()
    expect(localStorage.getItem('danplay.speed')).toBe('0.8')
    expect(localStorage.getItem('danplay.click.sound')).toBe('"madera"')
    expect(localStorage.getItem('danplay.mobile.tapPlays')).toBe('1')
    expect(localStorage.getItem('danplay.vel')).toBeNull()
    expect(localStorage.getItem('danplay.clic.sonido')).toBeNull()
    expect(localStorage.getItem('danplay.movil.toque')).toBeNull()
  })

  it('if something already exists under the current name, that wins', () => {
    localStorage.setItem('danplay.vel', '0.8')
    localStorage.setItem('danplay.speed', '1.2')
    migrateLegacyStorage()
    expect(localStorage.getItem('danplay.speed')).toBe('1.2')
    expect(localStorage.getItem('danplay.vel')).toBeNull()
  })

  it('the waveform style also changes value', () => {
    localStorage.setItem('danplay.ondaEstilo', 'rayitas')
    migrateLegacyStorage()
    expect(localStorage.getItem('danplay.waveStyle')).toBe('lines')
  })

  it('the cue lane outputs are kept', () => {
    localStorage.setItem(
      'danplay.salidas',
      JSON.stringify({
        preset: 'custom',
        custom: { song: 'main', stems: { guias_g1: 'aux', drums: 'main' } },
        mono: { stems: { guias_g1: true } }
      })
    )
    migrateLegacyStorage()
    const prefs = JSON.parse(localStorage.getItem('danplay.outputs'))
    expect(prefs.custom.stems).toEqual({ cues_g1: 'aux', drums: 'main' })
    expect(prefs.mono.stems).toEqual({ cues_g1: true })
  })

  it('each old name maps to a different one, all of them in English', () => {
    const now = LEGACY_KEYS.map(([, key]) => key)
    expect(new Set(now).size).toBe(now.length)
    for (const [old, key] of LEGACY_KEYS) {
      expect(old).not.toBe(key)
      expect(key).toMatch(/^danplay\.[A-Za-z.]+$/)
    }
  })
})
