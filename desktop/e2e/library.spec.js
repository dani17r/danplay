// La biblioteca: la bienvenida, buscar, «Artistas» (que siempre agrupa) y
// recorrerla con el teclado.
import { test, expect } from '@playwright/test'
import { SONGS } from './songs.js'
import { core, MUSIC, reset, rows, titles, titleTexts, openApp, go } from './helpers.js'

test.beforeEach(async ({ request }) => {
  await reset(request)
})

test('bienvenida: se elige la carpeta y aparece la lista', async ({ page, request }) => {
  // sin ninguna carpeta: la primera vez que se abre la app
  await core(request, 'DELETE', `/folders?path=${encodeURIComponent(MUSIC)}`)
  await page.goto('/')
  await expect(page.getByRole('heading', { name: 'Bienvenido a DanPlay' })).toBeVisible()
  // en el navegador no hay selector de carpetas del sistema: se escribe
  await page.getByLabel('Ruta de la carpeta de música').fill(MUSIC)
  await page.getByRole('button', { name: 'Analizar', exact: true }).click()
  // el análisis es una tarea larga del núcleo: se sigue hasta que acaba
  await expect(rows(page)).toHaveCount(SONGS.length)
  await expect(page.getByRole('heading', { name: 'Bienvenido a DanPlay' })).toHaveCount(0)
  expect((await titleTexts(page)).sort()).toEqual(SONGS.map((s) => s[1]).sort())
})

test('buscar filtra la lista, y quitar la búsqueda la devuelve entera', async ({ page }) => {
  await openApp(page)
  await page.getByRole('combobox', { name: 'Buscar en la biblioteca' }).fill('shekinah')
  await expect(titles(page)).toHaveText(['Shekinah'])
  await page.getByRole('button', { name: 'Quitar la búsqueda «shekinah»' }).click()
  await expect(rows(page)).toHaveCount(SONGS.length)
})

test('en «Artistas», que siempre agrupa, Ctrl y Mayús eligen lo que se ve', async ({ page }) => {
  await openApp(page)
  // Por título, la lista no va en el orden de los grupos: el tramo de Mayús
  // se sacaba del orden de la lista y elegía canciones que no estaban entre
  // las dos pulsadas
  await page.locator('th[data-col="title"] .th-sort').click()
  await expect(titles(page).first()).toHaveText('A Una Voz')
  await go(page, 'Artistas')
  await expect(page.locator('.group-head')).toHaveCount(3)
  const shown = await titleTexts(page)
  const selected = page.locator('[data-song-row].selected td.title')

  // al agrupar se perdía el evento del clic por el camino, y con él Ctrl y Mayús
  await rows(page).nth(0).click()
  await rows(page)
    .nth(2)
    .click({ modifiers: ['Control'] })
  await expect(selected).toHaveText([shown[0], shown[2]])
  await rows(page)
    .nth(4)
    .click({ modifiers: ['Shift'] })
  await expect(selected).toHaveText(shown.slice(2, 5))

  // el menú de una de las elegidas es el de todas
  await rows(page).nth(3).click({ button: 'right' })
  await expect(page.getByRole('menuitem', { name: /Añadir 3 a una lista/ })).toBeVisible()
  await page.keyboard.press('Escape')
  await expect(page.getByRole('menu')).toHaveCount(0)
})

test('en «Artistas» la lista es una sola: las flechas pasan de un grupo a otro', async ({
  page
}) => {
  await openApp(page)
  await go(page, 'Artistas')
  await expect(page.locator('.group-head')).toHaveCount(3)
  // una sola lista (antes, una por grupo) con una sola parada del tabulador
  await expect(page.getByRole('grid')).toHaveCount(1)
  await expect(page.locator('[data-song-row][tabindex="0"]')).toHaveCount(1)
  const shown = await titleTexts(page)
  // con el teclado (un clic de ratón suelta el foco para que las flechas
  // sigan siendo del reproductor, como el resto de botones)
  await rows(page).nth(1).focus() // la última del primer grupo
  await page.keyboard.press('ArrowDown')
  await expect(rows(page).nth(2)).toBeFocused()
  await expect(page.locator('[data-song-row].selected td.title')).toHaveText([shown[2]])
  // plegar un grupo: sus filas se van, su cabecera se queda
  await page.locator('.group-head', { hasText: 'Barak' }).click()
  await expect(rows(page)).toHaveCount(3)
  await expect(page.locator('.group-head')).toHaveCount(3)
})

test('la lista se recorre con el teclado y Enter pone la canción', async ({ page }) => {
  await openApp(page)
  // una sola parada del tabulador: la primera fila
  await expect(rows(page).first()).toHaveAttribute('tabindex', '0')
  await expect(rows(page).nth(1)).toHaveAttribute('tabindex', '-1')
  await rows(page).first().focus()
  await page.keyboard.press('ArrowDown')
  await expect(rows(page).nth(1)).toBeFocused()
  await expect(rows(page).nth(1)).toHaveClass(/selected/)
  const title = (await titleTexts(page))[1]
  await page.keyboard.press('Enter')
  await expect(page.locator('.player .title')).toHaveText(title)
  // Mayús+F10 abre su menú, que se recorre con las flechas; Escape lo
  // cierra y el foco vuelve a la fila
  await page.keyboard.press('Shift+F10')
  await expect(page.getByRole('menu')).toBeVisible()
  await expect(page.getByRole('menuitem').first()).toBeFocused()
  await page.keyboard.press('ArrowDown')
  await expect(page.getByRole('menuitem').nth(1)).toBeFocused()
  await page.keyboard.press('Escape')
  await expect(page.getByRole('menu')).toHaveCount(0)
  await expect(rows(page).nth(1)).toBeFocused()
})
