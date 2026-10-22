// @ts-check
// Cómo se pinta la onda del modo estudio: el estilo lo elige quien toca
// (en el menú del estudio o con el clic derecho sobre la onda) y vale para la
// onda de la canción y para los carriles de las pistas separadas.
//
// Todos pintan lo mismo: por cada columna, el pico (la silueta, apagada) y el
// RMS (el cuerpo, en el color del tema), a la misma escala.

/** @typedef {'bars'|'blocks'|'lines'|'smooth'|'outline'|'mirror'} WaveStyle */

/** @type {Array<{v: WaveStyle, n: string, note: string}>} */
export const WAVE_STYLES = [
  { v: 'bars', n: 'Barras', note: 'finas y juntas' },
  { v: 'blocks', n: 'Bloques', note: 'cuadrados, más gruesos' },
  { v: 'lines', n: 'Rayitas', note: 'líneas finas y separadas' },
  { v: 'smooth', n: 'Suave', note: 'una silueta rellena y redondeada' },
  { v: 'outline', n: 'Contorno', note: 'solo el borde' },
  { v: 'mirror', n: 'Con reflejo', note: 'barras desde abajo y su reflejo' }
]

/** @param {string} v @returns {WaveStyle} */
export const waveStyleOf = (v) =>
  /** @type {WaveStyle} */ (WAVE_STYLES.some((s) => s.v === v) ? v : 'bars')

/** Píxeles por columna de cada estilo. */
const STEP = { bars: 2, blocks: 6, lines: 3, smooth: 2, outline: 2, mirror: 4 }

/**
 * Cuántas columnas caben en `W` píxeles con ese estilo.
 * @param {WaveStyle} style @param {number} W
 */
export function columnsFor(style, W) {
  return Math.max(1, Math.floor(W / (STEP[style] || 2)))
}

const clamp01 = (/** @type {number} */ v) => Math.max(0, Math.min(1, v))
/** En un mp3 de hoy los picos van casi todos a tope: una curva suave. */
const curve = (/** @type {number} */ v) => Math.pow(clamp01(v), 0.7)

/**
 * Pinta las columnas `data` ([pico, rms] de cada una) en la franja de
 * `top` a `top + h`, de `W` píxeles de ancho.
 * @param {CanvasRenderingContext2D} ctx
 * @param {Array<[number, number]>} data
 * @param {{W: number, top: number, h: number, rmsTop: number, text: string,
 *   accent: string, dim?: number, style?: WaveStyle, pad?: number, min?: number}} o
 */
export function paintWave(ctx, data, o) {
  const n = data.length
  if (!n) return
  const { W, top, h, text, accent } = o
  const dim = o.dim ?? 1
  const style = o.style || 'bars'
  const pad = o.pad ?? 2
  const min = o.min ?? 1
  const rmsTop = Math.max(1e-6, o.rmsTop)
  const cw = W / n
  const peak = (/** @type {number} */ i) => curve(data[i][0])
  const body = (/** @type {number} */ i) => Math.min(peak(i), curve(data[i][1] / rmsTop))

  if (style === 'smooth' || style === 'outline') {
    const mid = top + h / 2
    const room = h / 2 - pad
    /** La envolvente (arriba y, de espejo, abajo), redondeada entre columnas. */
    const envelope = (/** @type {(i: number) => number} */ f) => {
      ctx.beginPath()
      ctx.moveTo(0, mid)
      let px = 0
      let py = mid - Math.max(min * 0.5, f(0) * room)
      ctx.lineTo(px, py)
      for (let i = 1; i < n; i++) {
        const x = i * cw + cw / 2
        const y = mid - Math.max(min * 0.5, f(i) * room)
        ctx.quadraticCurveTo(px, py, (px + x) / 2, (py + y) / 2)
        px = x
        py = y
      }
      ctx.lineTo(W, py)
      ctx.lineTo(W, mid)
      // y de vuelta por abajo, en espejo
      px = W
      py = mid + (py - mid) * -1
      ctx.lineTo(px, py)
      for (let i = n - 1; i >= 0; i--) {
        const x = i * cw + cw / 2
        const y = mid + Math.max(min * 0.5, f(i) * room)
        ctx.quadraticCurveTo(px, py, (px + x) / 2, (py + y) / 2)
        px = x
        py = y
      }
      ctx.lineTo(0, py)
      ctx.closePath()
    }
    if (style === 'smooth') {
      envelope(peak)
      ctx.globalAlpha = 0.24 * dim
      ctx.fillStyle = text
      ctx.fill()
      envelope(body)
      ctx.globalAlpha = 0.9 * dim
      ctx.fillStyle = accent
      ctx.fill()
    } else {
      envelope(peak)
      ctx.globalAlpha = 0.95 * dim
      ctx.strokeStyle = accent
      ctx.lineWidth = 1.4
      ctx.lineJoin = 'round'
      ctx.stroke()
      // la línea del centro, para no perderse en los silencios
      ctx.globalAlpha = 0.18 * dim
      ctx.fillStyle = text
      ctx.fillRect(0, mid - 0.5, W, 1)
    }
    ctx.globalAlpha = 1
    return
  }

  if (style === 'mirror') {
    // las barras crecen desde una base, con su reflejo debajo más apagado
    const base = top + pad + (h - 2 * pad) * 0.68
    const up = base - top - pad
    const down = top + h - pad - base
    const bar = Math.max(1, cw - 1)
    for (let i = 0; i < n; i++) {
      const x = i * cw
      const hp = Math.max(min, peak(i) * up)
      const hr = Math.max(min, body(i) * up)
      ctx.globalAlpha = 0.24 * dim
      ctx.fillStyle = text
      ctx.fillRect(x, base - hp, bar, hp)
      ctx.globalAlpha = 0.95 * dim
      ctx.fillStyle = accent
      ctx.fillRect(x, base - hr, bar, hr)
      ctx.globalAlpha = 0.3 * dim
      ctx.fillRect(x, base + 1, bar, Math.min(down, hr * 0.45))
    }
    ctx.globalAlpha = 1
    return
  }

  // barras, bloques y rayitas: columnas desde el centro, arriba y abajo
  const mid = top + h / 2
  const room = h / 2 - pad
  const bar =
    style === 'lines'
      ? Math.max(1, Math.min(1.5, cw * 0.4))
      : style === 'blocks'
        ? Math.max(1, cw - 2)
        : cw >= 3
          ? cw - 1
          : cw
  const shift = (cw - bar) / 2
  for (let i = 0; i < n; i++) {
    const x = i * cw + (style === 'bars' ? 0 : shift)
    let hp = Math.max(min, peak(i) * room)
    let hr = Math.max(min, body(i) * room)
    if (style === 'blocks') {
      // a escalones, como bloques
      const q = Math.max(2, room / 6)
      hp = Math.max(min, Math.ceil(hp / q) * q)
      hr = Math.max(min, Math.ceil(hr / q) * q)
    }
    ctx.globalAlpha = (style === 'lines' ? 0.32 : 0.24) * dim
    ctx.fillStyle = text
    ctx.fillRect(x, mid - hp, bar, hp * 2)
    ctx.globalAlpha = 0.95 * dim
    ctx.fillStyle = accent
    ctx.fillRect(x, mid - hr, bar, hr * 2)
  }
  ctx.globalAlpha = 1
}
