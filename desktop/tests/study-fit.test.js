import { describe, it, expect } from 'vitest'
import {
  BODY_MAX,
  BODY_MIN,
  LANE_MAX,
  LANE_MIN,
  bodyHeight,
  laneHeight,
  waveRoom
} from '../src/utils/studyFit.js'

// El reparto del alto del modo estudio. Con una secuencia de 22 pistas la
// onda se comía el panel entero y de las columnas de abajo (velocidad, clic,
// marcadores, notas) solo quedaba el título.
describe('the height of the study, shared out', () => {
  // un panel a todo el alto de una pantalla de 900: 824 px, con su cabecera
  // y su barra (96), tres filas con 8 de separación y 22 de relleno
  const panel = { inner: 824, pad: 22, others: 96, gap: 8, rows: 4, rest: 110 }

  it('the body always keeps a usable height', () => {
    expect(bodyHeight(400)).toBe(BODY_MIN)
    expect(bodyHeight(824)).toBeCloseTo(280.16)
    expect(bodyHeight(2000)).toBe(BODY_MAX)
  })

  it('22 tracks share what is left, never below what their buttons need', () => {
    const room = waveRoom(panel)
    expect(room).toBe(824 - 22 - 96 - 24 - 280 - 110)
    // no caben a 20 px (440 > 292): se quedan en el mínimo y la línea de
    // tiempo se desplaza, pero el cuerpo no se toca
    expect(laneHeight(22, room)).toBe(LANE_MIN)
    // con más alto sí caben, y crecen con él
    const tall = waveRoom({ ...panel, inner: 1300 })
    expect(tall).toBe(1300 - 22 - 96 - 24 - 300 - 110)
    expect(laneHeight(22, tall)).toBe(34)
  })

  it('a few tracks grow to fill the panel, up to a limit', () => {
    const room = waveRoom({ ...panel, inner: 1004 })
    expect(laneHeight(6, room)).toBe(LANE_MAX)
    expect(laneHeight(6, 200)).toBe(33)
  })

  it('a panel too small leaves no room, never a negative one', () => {
    expect(waveRoom({ inner: 300, others: 96, rest: 110 })).toBe(0)
    expect(laneHeight(3, 0)).toBe(LANE_MIN)
  })
})
