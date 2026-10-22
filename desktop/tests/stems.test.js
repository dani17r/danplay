// Las pistas separadas en el mezclador: qué suena según callar y dejar sola,
// y cómo se llama la mezcla que se guarda.
import { describe, it, expect } from 'vitest'
import { stemPlan, mixFileName } from '../src/utils/stems.js'

const tracks = [
  { source: 'drums', name: 'Batería', path: '/s/Bateria.flac' },
  { source: 'vocals', name: 'Voces', path: '/s/Voces.flac' },
  { source: 'bass', name: 'Bajo', path: '/s/Bajo.flac' },
  { source: 'other', name: 'Otros', path: '/s/Otros.flac' }
]
const on = (plan) => plan.filter((t) => t.on).map((t) => t.key)

describe('what plays from each stem', () => {
  it('untouched, all of them as they come', () => {
    const plan = stemPlan(tracks, {})
    expect(on(plan)).toEqual(['drums', 'vocals', 'bass', 'other'])
    expect(plan.every((t) => t.gain === 1 && t.pan === 0)).toBe(true)
    expect(plan[0]).toMatchObject({ key: 'drums', name: 'Batería', path: '/s/Bateria.flac' })
  })

  it('muted does not play; with solos, only the soloed ones', () => {
    expect(on(stemPlan(tracks, { drums: { mute: true } }))).toEqual(['vocals', 'bass', 'other'])
    const solos = { vocals: { solo: true }, bass: { solo: true } }
    expect(on(stemPlan(tracks, solos))).toEqual(['vocals', 'bass'])
    // callar gana a dejar sola
    expect(on(stemPlan(tracks, { ...solos, bass: { solo: true, mute: true } }))).toEqual(['vocals'])
  })

  it('zero volume means muting it, and odd values are clamped', () => {
    const plan = stemPlan(tracks, {
      drums: { gain: 0 },
      vocals: { gain: 9, pan: -3 },
      bass: { gain: Number.NaN, pan: Number.NaN }
    })
    expect(plan[0].on).toBe(false)
    expect(plan[1]).toMatchObject({ gain: 2, pan: -1, on: true })
    expect(plan[2]).toMatchObject({ gain: 1, pan: 0, on: true })
  })
})

describe('the mix name', () => {
  const plan = (settings) => stemPlan(tracks, settings)

  it('with cues it says so, and the song alone only says what it contains', () => {
    expect(mixFileName('Mi Gozo.mp3', plan({ drums: { mute: true } }), { guides: true })).toBe(
      'Mi Gozo (sin bateria, con guias)'
    )
    expect(mixFileName('Barak - Mi Gozo.mp3', [], { guides: true, click: true })).toBe(
      'Barak - Mi Gozo (con clic, con guias)'
    )
    expect(mixFileName('Barak - Mi Gozo.mp3', [])).toBe('Barak - Mi Gozo')
  })

  it('says what it is missing, without accents', () => {
    expect(mixFileName('Barak - Mi Gozo.mp3', plan({ drums: { mute: true } }))).toBe(
      'Barak - Mi Gozo (sin bateria)'
    )
    expect(
      mixFileName('Barak - Mi Gozo.mp3', plan({ drums: { mute: true }, bass: { mute: true } }))
    ).toBe('Barak - Mi Gozo (sin bateria ni bajo)')
  })

  it('or the only thing it has, or that it is a mix', () => {
    expect(mixFileName('Mi Gozo.flac', plan({ vocals: { solo: true } }))).toBe(
      'Mi Gozo (solo voces)'
    )
    expect(mixFileName('Mi Gozo.flac', plan({}))).toBe('Mi Gozo (mezcla)')
    expect(
      mixFileName(
        'Mi Gozo',
        plan({ drums: { mute: true }, bass: { mute: true }, other: { mute: true } })
      )
    ).toBe('Mi Gozo (solo voces)')
  })

  it('the ñ stays and what is invalid in a file name does not', () => {
    expect(mixFileName('Año: Nuevo?.mp3', plan({ drums: { mute: true } }))).toBe(
      'Año_ Nuevo_ (sin bateria)'
    )
    expect(mixFileName('', plan({}))).toBe('Cancion (mezcla)')
  })

  it('with the click, it says so', () => {
    expect(mixFileName('Mi Gozo.mp3', plan({ drums: { mute: true } }), { click: true })).toBe(
      'Mi Gozo (sin bateria, con clic)'
    )
  })
})
