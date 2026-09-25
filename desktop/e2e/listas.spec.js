// Los repertorios: crear uno, llevarle canciones arrastrando y ordenarlo
// arrastrando dentro.
import { test, expect } from '@playwright/test'
import { core, reset, rows, titles, titleTexts, openApp } from './helpers.js'

test.beforeEach(async ({ request }) => {
  await reset(request)
})

/**
 * Arrastra con el ratón de verdad: el arrastre es de eventos de puntero (no
 * el nativo del navegador) y empieza al pasar de unos píxeles.
 */
async function drag(page, from, to, { lowerHalf = false } = {}) {
  const a = await from.boundingBox()
  const b = await to.boundingBox()
  await page.mouse.move(a.x + 20, a.y + a.height / 2)
  await page.mouse.down()
  await page.mouse.move(a.x + 40, a.y + a.height / 2 + 10, { steps: 4 })
  const y = lowerHalf ? b.y + b.height * 0.75 : b.y + b.height / 2
  await page.mouse.move(b.x + 30, y, { steps: 10 })
  await page.mouse.up()
}

test('crear un repertorio, llevarle canciones y ordenarlo arrastrando', async ({
  page,
  request
}) => {
  await openApp(page)
  await page.getByRole('button', { name: 'Nueva lista' }).click()
  const dialog = page.getByRole('dialog', { name: 'Nueva lista' })
  await expect(dialog).toBeVisible()
  // el foco ya está en el campo: se escribe y Enter
  await expect(dialog.getByRole('textbox')).toBeFocused()
  await page.keyboard.type('Domingo e2e')
  await page.keyboard.press('Enter')
  await expect(dialog).toHaveCount(0)
  const playlist = page.locator('.nav-playlist', { hasText: 'Domingo e2e' })
  await expect(playlist).toBeVisible()
  const { playlists } = await core(request, 'GET', '/playlists')
  const id = playlists.find((l) => l.name === 'Domingo e2e').id

  // tres canciones, soltadas una a una sobre el repertorio del lateral
  const names = await titleTexts(page)
  for (const i of [0, 1, 2]) {
    await drag(page, titles(page).nth(i), playlist)
    await expect(playlist.locator('.count')).toHaveText(String(i + 1))
  }

  // dentro del repertorio, la primera se lleva debajo de la segunda
  await playlist.click()
  await expect(titles(page)).toHaveText(names.slice(0, 3))
  await expect(page.getByText('arrastra una canción para cambiar el orden')).toBeVisible()
  await drag(page, titles(page).nth(0), rows(page).nth(1), { lowerHalf: true })
  const expected = [names[1], names[0], names[2]]
  await expect(titles(page)).toHaveText(expected)
  // y el orden lo guarda el núcleo, no solo la pantalla
  await expect
    .poll(async () =>
      (await core(request, 'GET', `/playlists/${id}/songs`)).songs.map((s) => s.title)
    )
    .toEqual(expected)
})
