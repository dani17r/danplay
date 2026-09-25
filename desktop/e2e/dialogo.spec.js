// El diálogo de confirmación con el teclado: Enter sobre «Cancelar» cancela.
// Antes ACEPTABA: con el foco en Cancelar, Enter mandaba la canción a la
// papelera.
import { test, expect } from '@playwright/test'
import { existsSync } from 'node:fs'
import { SONGS } from './songs.js'
import { core, reset, rows, titleTexts, openApp, songsByTitle } from './helpers.js'

test.beforeEach(async ({ request }) => {
  await reset(request)
})

test('Enter con el foco en «Cancelar» no manda nada a la papelera', async ({ page, request }) => {
  await openApp(page)
  const [title] = await titleTexts(page)
  const song = (await songsByTitle(request))[title]
  const { path } = await core(request, 'GET', `/song/${song.id}`)
  expect(existsSync(path)).toBe(true)

  await rows(page).first().click({ button: 'right' })
  await page.getByRole('menuitem', { name: 'Mandar a la papelera…' }).click()
  const dialog = page.getByRole('dialog', { name: 'Mandar a la papelera' })
  await expect(dialog).toBeVisible()
  // el foco empieza en el botón principal; Mayús+Tab lo lleva a Cancelar
  await expect(dialog.getByRole('button', { name: 'A la papelera' })).toBeFocused()
  await page.keyboard.press('Shift+Tab')
  await expect(dialog.getByRole('button', { name: 'Cancelar' })).toBeFocused()
  await page.keyboard.press('Enter')

  await expect(dialog).toHaveCount(0)
  await expect(rows(page)).toHaveCount(SONGS.length)
  expect(existsSync(path), 'el archivo se fue a la papelera').toBe(true)
  expect((await core(request, 'GET', `/song/${song.id}`)).id).toBe(song.id)
})
