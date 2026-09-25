// El modo estudio: las notas se quedan en su canción aunque la canción cambie
// antes de que se guarden. El guardado espera un momento, y antes leía la
// canción al hacerlo: si ya había cambiado, las notas de la primera se
// guardaban en la segunda (en el índice y en la etiqueta del archivo).
import { test, expect } from '@playwright/test'
import { core, reset, rows, titleTexts, openApp, songsByTitle } from './helpers.js'

test.beforeEach(async ({ request }) => {
  await reset(request)
})

const notesOf = async (request, id) => {
  const { study } = await core(request, 'GET', `/song/${id}`)
  return study ? JSON.parse(study).notes || '' : ''
}

test('las notas se guardan en su canción aunque se pase a la siguiente', async ({
  page,
  request
}) => {
  await openApp(page)
  const [first, second] = await titleTexts(page)
  const byTitle = await songsByTitle(request)
  const a = byTitle[first].id
  const b = byTitle[second].id
  try {
    await rows(page).first().dblclick()
    await expect(page.locator('.player .title')).toHaveText(first)
    await page.locator('.player .pl-study').click()
    const study = page.getByRole('region', { name: 'Modo estudio' })
    await expect(study).toContainText(first)
    await study.locator('textarea').fill('cejilla en 2, entrar tras el redoble')
    // sin esperar a que se guarde: la siguiente
    await page
      .locator('.player')
      .getByRole('button', { name: /^Siguiente/ })
      .click()
    await expect(study).toContainText(second)
    await expect.poll(() => notesOf(request, a)).toBe('cejilla en 2, entrar tras el redoble')
    expect(await notesOf(request, b), 'las notas acabaron en la otra canción').toBe('')
    // y la nueva no hereda lo escrito
    await expect(study.locator('textarea')).toHaveValue('')
    await study.getByRole('button', { name: 'Cerrar', exact: true }).click()
    await expect(study).toHaveCount(0)
  } finally {
    await core(request, 'PUT', `/song/${a}/study`, {})
    await core(request, 'PUT', `/song/${b}/study`, {})
  }
})
