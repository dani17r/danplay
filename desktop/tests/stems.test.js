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

describe('qué suena de cada pista', () => {
  it('sin tocar nada, todas como vienen', () => {
    const plan = stemPlan(tracks, {})
    expect(on(plan)).toEqual(['drums', 'vocals', 'bass', 'other'])
    expect(plan.every((t) => t.gain === 1 && t.pan === 0)).toBe(true)
    expect(plan[0]).toMatchObject({ key: 'drums', name: 'Batería', path: '/s/Bateria.flac' })
  })

  it('callada no suena; con solos, solo las que lo estén', () => {
    expect(on(stemPlan(tracks, { drums: { mute: true } }))).toEqual(['vocals', 'bass', 'other'])
    const solos = { vocals: { solo: true }, bass: { solo: true } }
    expect(on(stemPlan(tracks, solos))).toEqual(['vocals', 'bass'])
    // callar gana a dejar sola
    expect(on(stemPlan(tracks, { ...solos, bass: { solo: true, mute: true } }))).toEqual(['vocals'])
  })

  it('el volumen a cero es callarla, y lo raro se recorta', () => {
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

describe('el nombre de la mezcla', () => {
  const plan = (settings) => stemPlan(tracks, settings)

  it('dice lo que le falta, sin tildes', () => {
    expect(mixFileName('Barak - Mi Gozo.mp3', plan({ drums: { mute: true } }))).toBe(
      'Barak - Mi Gozo (sin bateria)'
    )
    expect(
      mixFileName('Barak - Mi Gozo.mp3', plan({ drums: { mute: true }, bass: { mute: true } }))
    ).toBe('Barak - Mi Gozo (sin bateria ni bajo)')
  })

  it('o lo único que lleva, o que es una mezcla', () => {
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

  it('la ñ se queda y lo que no vale en un nombre de archivo, no', () => {
    expect(mixFileName('Año: Nuevo?.mp3', plan({ drums: { mute: true } }))).toBe(
      'Año_ Nuevo_ (sin bateria)'
    )
    expect(mixFileName('', plan({}))).toBe('Cancion (mezcla)')
  })
})
