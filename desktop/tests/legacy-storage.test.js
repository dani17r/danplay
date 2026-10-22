// Lo que se guardaba en el navegador con nombres en español
// (utils/legacyStorage.js): pasa a su nombre de ahora sin perderse.
import { describe, it, expect, beforeEach } from 'vitest'
import { LEGACY_KEYS, migrateLegacyStorage } from '../src/utils/legacyStorage.js'

beforeEach(() => localStorage.clear())

describe('lo guardado con los nombres de antes', () => {
  it('pasa al nombre de ahora y el de antes se borra', () => {
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

  it('si ya hay algo con el nombre de ahora, manda eso', () => {
    localStorage.setItem('danplay.vel', '0.8')
    localStorage.setItem('danplay.speed', '1.2')
    migrateLegacyStorage()
    expect(localStorage.getItem('danplay.speed')).toBe('1.2')
    expect(localStorage.getItem('danplay.vel')).toBeNull()
  })

  it('el estilo de la onda cambia también de valor', () => {
    localStorage.setItem('danplay.ondaEstilo', 'rayitas')
    migrateLegacyStorage()
    expect(localStorage.getItem('danplay.waveStyle')).toBe('lines')
  })

  it('las salidas de los carriles de guías se conservan', () => {
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

  it('cada nombre de antes va a uno distinto, y todos en inglés', () => {
    const now = LEGACY_KEYS.map(([, key]) => key)
    expect(new Set(now).size).toBe(now.length)
    for (const [old, key] of LEGACY_KEYS) {
      expect(old).not.toBe(key)
      expect(key).toMatch(/^danplay\.[A-Za-z.]+$/)
    }
  })
})
