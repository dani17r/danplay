// Los estilos de la onda (utils/wavePaint.js): cada uno pinta lo mismo (el
// pico y el cuerpo de cada columna) a su manera, sin salirse de su franja.
import { describe, it, expect } from 'vitest'
import { WAVE_STYLES, columnsFor, paintWave, waveStyleOf } from '../src/utils/wavePaint.js'

/** Un lienzo de mentira que apunta dónde se pinta. */
function lienzo() {
  const ys = []
  const xs = []
  const ctx = {
    globalAlpha: 1,
    fillStyle: '',
    strokeStyle: '',
    lineWidth: 1,
    lineJoin: '',
    fills: 0,
    strokes: 0,
    fillRect(x, y, w, h) {
      xs.push(x, x + w)
      ys.push(y, y + h)
      this.fills++
    },
    beginPath() {},
    moveTo(x, y) {
      xs.push(x)
      ys.push(y)
    },
    lineTo(x, y) {
      xs.push(x)
      ys.push(y)
    },
    quadraticCurveTo(cx, cy, x, y) {
      xs.push(x)
      ys.push(y)
    },
    closePath() {},
    fill() {
      this.fills++
    },
    stroke() {
      this.strokes++
    }
  }
  return { ctx, xs, ys }
}

// una canción de mentira: sube y baja
const data = Array.from({ length: 60 }, (_, i) => {
  const v = Math.abs(Math.sin(i / 6))
  return [v, v * 0.4]
})

describe('los estilos de la onda', () => {
  it('son seis, y lo desconocido vuelve a las barras', () => {
    expect(WAVE_STYLES.map((s) => s.v)).toEqual([
      'barras',
      'bloques',
      'rayitas',
      'suave',
      'contorno',
      'reflejo'
    ])
    expect(waveStyleOf('suave')).toBe('suave')
    expect(waveStyleOf('lo que sea')).toBe('barras')
  })

  it('cada estilo pinta dentro de su franja (un carril de 40 a 80 px)', () => {
    for (const { v } of WAVE_STYLES) {
      const { ctx, xs, ys } = lienzo()
      paintWave(ctx, data, {
        W: 300,
        top: 40,
        h: 40,
        rmsTop: 0.4,
        text: '#fff',
        accent: '#4ade80',
        style: v
      })
      expect(ctx.fills + ctx.strokes, v).toBeGreaterThan(0)
      expect(Math.min(...ys), v).toBeGreaterThanOrEqual(40)
      expect(Math.max(...ys), v).toBeLessThanOrEqual(80)
      expect(Math.min(...xs), v).toBeGreaterThanOrEqual(0)
      expect(Math.max(...xs), v).toBeLessThanOrEqual(300.5)
      expect(ctx.globalAlpha, 'deja la opacidad como estaba').toBe(1)
    }
  })

  it('los bloques son más gruesos que las barras; las rayitas, más separadas', () => {
    expect(columnsFor('barras', 600)).toBe(300)
    expect(columnsFor('bloques', 600)).toBe(100)
    expect(columnsFor('rayitas', 600)).toBe(200)
  })

  it('sin columnas no pinta nada', () => {
    const { ctx } = lienzo()
    paintWave(ctx, [], { W: 100, top: 0, h: 40, rmsTop: 1, text: '#fff', accent: '#0f0' })
    expect(ctx.fills + ctx.strokes).toBe(0)
  })
})
