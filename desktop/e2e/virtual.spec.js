// La lista agrupada larga, en un navegador de verdad: solo se pinta lo que se
// ve, se llega hasta el final sin huecos y la cabecera del grupo que se está
// recorriendo se queda arriba. Las cuentas (alto de fila, de cabecera,
// columnas de la cuadrícula) se miden de lo pintado, y eso jsdom no lo hace.
import { test, expect } from '@playwright/test'
import { SONGS, BIG_COUNT } from './songs.js'
import { BIG, core, reset, rows, runJob, go } from './helpers.js'

const TOTAL = SONGS.length + BIG_COUNT

test.beforeEach(async ({ request }) => {
  await reset(request)
  await core(request, 'POST', '/folders', { path: BIG, label: '', force: false })
  await runJob(request, '/scan', 'escaneo')
})

test.afterEach(async ({ request }) => {
  await core(request, 'DELETE', `/folders?path=${encodeURIComponent(BIG)}`)
})

/** El panel que se desplaza y dónde queda, en la pantalla. */
async function panel(page, selector) {
  const el = page.locator(selector).first()
  await expect(el).toBeVisible()
  return el
}

/** Baja del todo y comprueba que la última fila acaba donde acaba la lista. */
async function toTheEnd(page, scroller, last) {
  await scroller.evaluate((el) => el.scrollTo(0, el.scrollHeight))
  await expect(scroller.getByText(last, { exact: true })).toBeInViewport()
  const gap = await scroller.evaluate((el) => {
    const filas = [...el.querySelectorAll('[data-song-row]')]
    const fin = Math.max(...filas.map((f) => f.getBoundingClientRect().bottom))
    return el.getBoundingClientRect().bottom - fin
  })
  // lo que quede es el relleno del panel, no un hueco de filas que faltan
  expect(gap).toBeLessThan(40)
  expect(gap).toBeGreaterThan(-40)
}

test('«Artistas» con cientos de grupos pinta una ventana y llega al final', async ({ page }) => {
  await page.goto('/')
  await go(page, 'Artistas')
  await expect(page.locator('.filters .chip').first()).toHaveText(String(TOTAL))
  await expect(page.locator('.group-head').first()).toBeVisible()
  const pintadas = await rows(page).count()
  expect(pintadas).toBeGreaterThan(10)
  expect(pintadas, 'pinta la biblioteca entera').toBeLessThan(TOTAL / 2)

  const scroller = await panel(page, '.center .table-wrap')
  // a mitad de la lista, la cabecera de arriba es la del grupo que se ve
  await scroller.evaluate((el) => el.scrollTo(0, el.scrollHeight / 2))
  await expect
    .poll(() =>
      scroller.evaluate((el) => {
        const top = el.getBoundingClientRect().top
        const head = document.elementFromPoint(el.getBoundingClientRect().left + 60, top + 4)
        const name = head?.closest('.group-row')?.querySelector('.group-name')?.textContent
        const fila = [...el.querySelectorAll('[data-song-row]')].find(
          (f) => f.getBoundingClientRect().bottom > top + 60
        )
        const titulo = fila?.querySelector('td.title')?.textContent.trim() || ''
        const n = Number(titulo.replace('Tema ', ''))
        const artista = `Coro ${String(Math.floor(n / 2)).padStart(3, '0')}`
        return name === artista ? 'ok' : `${name} ≠ ${artista}`
      })
    )
    .toBe('ok')

  await toTheEnd(page, scroller, 'Shekinah')
  expect(await rows(page).count()).toBeLessThan(TOTAL / 2)

  // y con el teclado: Inicio lleva a la primera, aunque no estuviera pintada
  await rows(page).last().focus()
  await page.keyboard.press('Home')
  await expect(rows(page).first()).toBeFocused()
  await expect(scroller.getByText('Mi Gozo', { exact: true })).toBeInViewport()
})

test('la cuadrícula agrupada también se recorta y llega al final', async ({ page }) => {
  await page.goto('/')
  await go(page, 'Artistas')
  await page.getByRole('button', { name: 'Cuadrícula' }).click()
  const scroller = await panel(page, '.center .grid')
  await expect(page.locator('.group-head').first()).toBeVisible()
  expect(await rows(page).count()).toBeLessThan(TOTAL)
  await toTheEnd(page, scroller, 'Shekinah')
})
