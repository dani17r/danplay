import { describe, it, expect } from 'vitest'
import {
  BODY_MAX,
  BODY_MIN,
  LANE_MAX,
  LANE_MIN,
  TIGHT_BELOW,
  WAVE_MIN,
  bodyHeight,
  isTight,
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

  it('the body gets what its cards ask for, when there is room', () => {
    // a 1536x864 las tarjetas del tono y del clic (266 px) quedaban cortadas
    // con el cuerpo a 200
    expect(bodyHeight(588, 266)).toBe(266)
    expect(bodyHeight(588, 900), 'con su tope').toBe(BODY_MAX)
    const small = { pad: 22, others: 61, gap: 8, rows: 4, rest: 112, wanted: 266 }
    expect(waveRoom({ ...small, inner: 588 })).toBe(588 - 22 - 61 - 24 - 112 - 266)
    // sin sitio para las dos cosas, la onda conserva su mínimo y el cuerpo
    // cede lo que pedía de más (nunca por debajo de lo suyo)
    expect(waveRoom({ ...small, inner: 536 })).toBe(WAVE_MIN)
    expect(waveRoom({ ...small, inner: 480 })).toBe(480 - 22 - 61 - 24 - 112 - BODY_MIN)
  })

  it('a panel too small leaves no room, never a negative one', () => {
    expect(waveRoom({ inner: 300, others: 96, rest: 110 })).toBe(0)
    expect(laneHeight(3, 0)).toBe(LANE_MIN)
  })
})

// A 1366×768 (un portátil sin escalar) el panel de estudio mide 536 px, el
// cuerpo se quedaba en 238 y las tarjetas del tono y del clic piden 266: les
// faltaban 28 px y salían cortadas. Medido en Chrome, desde 800 px de ventana
// las tarjetas caben con los espacios de siempre; por debajo, el panel aprieta
// solo sus espacios (`.study.tight`): el texto es el mismo.
describe('a low window tightens the spacing, not the text', () => {
  it('from 800 px up the usual spacing fits; below it, tight', () => {
    expect(TIGHT_BELOW).toBe(800)
    expect(isTight(768)).toBe(true) // 1366×768
    expect(isTight(700)).toBe(true)
    expect(isTight(799)).toBe(true)
    expect(isTight(800)).toBe(false)
    expect(isTight(864)).toBe(false) // 1536×864, la ventana de verdad: no cambia
    expect(isTight(1080)).toBe(false)
    expect(isTight(0), 'sin medir, como siempre').toBe(false)
  })

  it('at 1366×768 the cards fall short with the usual spacing and fit with the tight one', () => {
    // lo medido: panel de 536 px (535 de alto útil), su cabecera y su barra
    // (60), cuatro filas, y lo demás de la línea de tiempo (111)
    const usual = { inner: 535, pad: 22, others: 60, gap: 8, rows: 4, rest: 111, wanted: 266 }
    const bodyOf = (m) => m.inner - m.pad - m.others - m.gap * (m.rows - 1) - m.rest - waveRoom(m)
    // la onda conserva su mínimo y el cuerpo cede lo que pedía de más: 238 < 266
    expect(waveRoom(usual)).toBe(WAVE_MIN)
    expect(bodyOf(usual)).toBe(238)
    expect(bodyOf(usual)).toBeLessThan(usual.wanted)
    // apretado: 6+8 de relleno en vez de 10+12, 5 de separación en vez de 8, y
    // las columnas piden 238 en vez de 266 (separaciones de fila de 3 y 5, y
    // el relleno de la tarjeta, de 5)
    const tight = { ...usual, pad: 14, gap: 5, wanted: 238 }
    expect(bodyOf(tight)).toBe(238)
    expect(bodyOf(tight)).toBeGreaterThanOrEqual(tight.wanted)
    // y a la onda le sobra: 97 px en vez de 80
    expect(waveRoom(tight)).toBe(97)
    // más baja aún (a 700 el panel no pasa de 520): también caben
    const low = { ...tight, inner: 519 }
    expect(bodyOf(low)).toBe(238)
    expect(waveRoom(low)).toBe(WAVE_MIN + 1)
  })
})
