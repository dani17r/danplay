// Cuánto tarda la interfaz en lo que más se nota, medido en WebKit contra la
// compilada y el núcleo de verdad (ver playwright.config.js de esta carpeta).
//
// Cada pasada abre un contexto nuevo (sin caché, como la app recién abierta) y:
//   1. arranque: hasta la primera fila de la biblioteca pintada, y hasta que
//      la cuenta de la cabecera dice todas; los bytes de JS y CSS bajados
//      hasta la primera fila y en los 4 s de tiempo libre que siguen; las
//      tareas largas (≥ 50 ms) hasta la primera fila y en ese tiempo libre;
//   2. desplazarse por la lista grande de arriba abajo, 200 px por fotograma:
//      la media, el p95 y el peor fotograma, y cuántos pasan de 33 ms;
//   3. la ficha: de pulsar una fila a ver su título en el panel de detalle;
//   4. buscar al escribir, en la lista y en el desplegable de Favoritos: de
//      la última tecla a ver el resultado (con los 180 ms de espera del
//      buscador dentro);
//   5. cambiar de página: de pulsar «Ajustes» a verla, y de vuelta a la lista;
//   6. el modo estudio: de pulsar su botón (con una canción puesta) a verlo.
// Antes de cada clic el ratón se queda 150 ms encima, como una persona que
// apunta antes de pulsar. De cada medida sale la mediana de las pasadas.
/* eslint-disable no-console -- mide: contarlo por pantalla es su trabajo */
import { test, expect } from '@playwright/test'
import { mkdirSync, writeFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { core, rows } from '../helpers.js'
import { TERMS_VERSION } from '../../src/data/terms.js'
import { probe } from './probe.js'
import { perfSong } from './songs.js'
import { SONGS } from '../songs.js'

const RUNS = Number(process.env.DANPLAY_PERF_RUNS) || 5
const OUT = resolve(process.env.DANPLAY_PERF_OUT || 'test-results/perf.json')
const HOVER_MS = 150

const median = (xs) => {
  const s = xs.filter((x) => Number.isFinite(x)).sort((a, b) => a - b)
  if (!s.length) return NaN
  const m = Math.floor(s.length / 2)
  return s.length % 2 ? s[m] : (s[m - 1] + s[m]) / 2
}

/** Espera a lo que se mira y devuelve lo que tardó desde el clic (o la tecla). */
const result = (page) => page.evaluate(() => window['__perf'].pending)

/** El ratón encima un momento y luego el clic, midiendo hasta que `cond` se cumpla. */
async function timedClick(page, locator, cond, arg) {
  await locator.hover()
  await page.waitForTimeout(HOVER_MS)
  await page.evaluate(cond, arg)
  await locator.click()
  return result(page)
}

/** Una pasada entera, con su contexto en frío. */
async function run(browser, baseURL, total) {
  const context = await browser.newContext({
    baseURL,
    viewport: { width: 1400, height: 900 },
    deviceScaleFactor: 1,
    locale: 'es-ES'
  })
  await context.addInitScript(probe)
  const page = await context.newPage()
  const bodies = new Map()
  const reading = []
  page.on('response', (r) => {
    const type = r.request().resourceType()
    if (type !== 'script' && type !== 'stylesheet') return
    reading.push(
      r.body().then(
        (b) => bodies.set(r.url(), { type, bytes: b.length }),
        () => {}
      )
    )
  })
  const out = {}
  try {
    // ------------------------------------------------------------ arranque
    await page.goto('/')
    await page.waitForFunction(() => window['__perf'].firstRow > 0, null, { timeout: 60_000 })
    await page.waitForFunction(
      (n) => {
        const chip = document.querySelector('.filters .chip')
        if (chip?.textContent.trim() !== String(n)) return false
        window['__perf'].allAt ||= performance.now()
        return true
      },
      total,
      { polling: 'raf', timeout: 60_000 }
    )
    // el tiempo libre de después: lo que se adelanta, se adelanta aquí
    await page.waitForTimeout(4000)
    const start = await page.evaluate(() => ({
      firstRow: window['__perf'].firstRow,
      allAt: window['__perf'].allAt,
      fcp: performance.getEntriesByName('first-contentful-paint')[0]?.startTime ?? NaN,
      gaps: window['__perf'].gaps,
      resources: performance
        .getEntriesByType('resource')
        .map((e) => ({ name: e.name, start: e.startTime })),
      now: performance.now()
    }))
    await Promise.all(reading)
    const bytes = { js: 0, css: 0, files: 0, idleJs: 0, idleCss: 0, idleFiles: 0 }
    for (const r of start.resources) {
      const b = bodies.get(r.name)
      if (!b) continue
      const key = b.type === 'script' ? 'js' : 'css'
      if (r.start <= start.firstRow) {
        bytes[key] += b.bytes
        bytes.files++
      } else {
        bytes['idle' + key[0].toUpperCase() + key.slice(1)] += b.bytes
        bytes.idleFiles++
      }
    }
    const long = (from, to) => {
      const g = start.gaps.filter(([at]) => at >= from && at < to)
      return {
        n: g.length,
        blocked: g.reduce((s, [, ms]) => s + ms - 50, 0),
        max: g.reduce((m, [, ms]) => Math.max(m, ms), 0)
      }
    }
    const boot = long(0, start.firstRow)
    const idle = long(start.firstRow, start.now)
    Object.assign(out, {
      fcp: start.fcp,
      firstRow: start.firstRow,
      allRows: start.allAt,
      jsKB: bytes.js / 1024,
      cssKB: bytes.css / 1024,
      files: bytes.files,
      idleJsKB: bytes.idleJs / 1024,
      idleCssKB: bytes.idleCss / 1024,
      idleFiles: bytes.idleFiles,
      longBoot: boot.n,
      blockedBoot: boot.blocked,
      maxBoot: boot.max,
      longIdle: idle.n,
      blockedIdle: idle.blocked,
      maxIdle: idle.max
    })

    // ------------------------------------------------------- desplazarse
    const scroll = await page.evaluate(async () => {
      const el = document.querySelector('.center .table-wrap')
      el.scrollTop = 0
      await new Promise((r) => requestAnimationFrame(r))
      const deltas = []
      let last = performance.now()
      await new Promise((done) => {
        const step = (t) => {
          deltas.push(t - last)
          last = t
          if (el.scrollTop + el.clientHeight >= el.scrollHeight - 1) return done()
          el.scrollTop += 200
          requestAnimationFrame(step)
        }
        requestAnimationFrame(step)
      })
      el.scrollTop = 0
      deltas.shift()
      const s = [...deltas].sort((a, b) => a - b)
      return {
        frames: s.length,
        mean: s.reduce((a, b) => a + b, 0) / s.length,
        p95: s[Math.floor(s.length * 0.95)],
        max: s[s.length - 1],
        slow: s.filter((d) => d > 33.4).length
      }
    })
    Object.assign(out, {
      scrollMean: scroll.mean,
      scrollP95: scroll.p95,
      scrollMax: scroll.max,
      scrollSlow: scroll.slow,
      scrollFrames: scroll.frames
    })
    await page.waitForTimeout(300)

    // ---------------------------------------------------------- la ficha
    const row = rows(page).nth(2).locator('td.title .cell-text')
    const title = (await row.innerText()).trim()
    out.details = await timedClick(
      page,
      row,
      (t) =>
        window['__perf'].watch(
          () => document.querySelector('.details .details-title')?.textContent.trim() === t
        ),
      title
    )

    // ------------------------------------------------ buscar al escribir
    // En «Todas las canciones» se filtra la propia lista; en las demás vistas
    // (Favoritos) sale el desplegable con lo encontrado en toda la biblioteca.
    const box = page.getByRole('combobox', { name: 'Buscar en la biblioteca' })
    const nav = (name) => page.locator('.sidebar .nav-link', { hasText: name }).first()
    // una sola: con la mitad de lo escrito salen diez o más
    const q = perfSong(Math.min(1234, total - SONGS.length - 1))[1]
    await box.click()
    await page.evaluate(
      (q) =>
        window['__perf'].watch(() => {
          const found = document.querySelectorAll('[data-song-row]')
          return found.length === 1 && found[0].textContent.includes(q)
        }, 'input'),
      q
    )
    await box.pressSequentially(q, { delay: 60 })
    out.searchList = await result(page)
    await box.fill('')
    await expect(page.locator('.filters .chip').first()).toHaveText(String(total))

    await nav('Favoritos').click()
    await expect(page.locator('.filters strong')).toHaveText('Favoritos')
    await box.click()
    await page.evaluate(
      (q) =>
        window['__perf'].watch(() => {
          const found = document.querySelectorAll('.search-results .sr-row')
          return found.length === 1 && found[0].textContent.includes(q)
        }, 'input'),
      q
    )
    await box.pressSequentially(q, { delay: 60 })
    out.searchDropdown = await result(page)
    await box.fill('')
    await page.keyboard.press('Escape')

    // --------------------------------------------------- cambiar de página
    out.toSettings = await timedClick(page, nav('Ajustes'), () =>
      window['__perf'].watch(
        () => document.querySelector('.center .page h2')?.textContent.trim() === 'Ajustes'
      )
    )
    out.backToList = await timedClick(page, nav('Todas las canciones'), () =>
      window['__perf'].watch(
        () =>
          document.querySelector('.filters strong')?.textContent.trim() === 'Todas las canciones' &&
          !!document.querySelector('[data-song-row]')
      )
    )

    // ------------------------------------------------------ el modo estudio
    await rows(page).first().dblclick()
    const playing = page.locator('.player .title')
    await expect(playing).not.toHaveText('')
    const song = (await playing.innerText()).trim()
    out.study = await timedClick(
      page,
      page.locator('.player .pl-study'),
      (t) =>
        window['__perf'].watch(() => {
          const s = document.querySelector('.study')
          return !!s && s.getClientRects().length > 0 && s.textContent.includes(t)
        }),
      song
    )
  } catch (e) {
    // lo que se veía cuando falló, para saber qué medida no se cumplió
    await page.screenshot({ path: resolve('test-results/perf/fallo.png') }).catch(() => {})
    throw e
  } finally {
    await context.close()
  }
  return out
}

test('la interfaz: arranque, bytes, tareas largas y lo que tarda cada parte', async ({
  browser,
  request,
  baseURL
}) => {
  await core(request, 'POST', '/terms/accept', { version: TERMS_VERSION })
  const { stats } = await core(request, 'GET', '/status')
  const runs = []
  for (let i = 0; i < RUNS; i++) {
    const r = await run(browser, baseURL, stats.total)
    console.log(
      `pasada ${i + 1}:`,
      JSON.stringify(r, (k, v) => (typeof v === 'number' ? Math.round(v * 10) / 10 : v))
    )
    runs.push(r)
  }
  const keys = Object.keys(runs[0])
  const med = Object.fromEntries(keys.map((k) => [k, median(runs.map((r) => r[k]))]))
  const rowsOut = keys.map(
    (k) => `${k.padEnd(14)} ${String(Math.round(med[k] * 10) / 10).padStart(9)}`
  )
  console.log(
    `\nmedianas de ${RUNS} pasadas (ms, KB o veces), ${stats.total} canciones:\n` +
      rowsOut.join('\n')
  )
  mkdirSync(dirname(OUT), { recursive: true })
  writeFileSync(
    OUT,
    JSON.stringify(
      {
        label: process.env.DANPLAY_PERF_LABEL || '',
        songs: stats.total,
        runs: RUNS,
        median: med,
        all: runs
      },
      null,
      2
    )
  )
})
