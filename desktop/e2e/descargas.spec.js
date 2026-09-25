// Descargas enseña qué yt-dlp se usa (YouTube cambia a menudo y uno viejo deja
// de bajar), permite traer el nuevo y avisa si falta el motor de JavaScript.
import { test, expect } from '@playwright/test'
import { core, openApp, go } from './helpers.js'

test('Descargas enseña la versión de yt-dlp y su motor de JavaScript', async ({
  page,
  request
}) => {
  const yt = await core(request, 'GET', '/youtube')
  await openApp(page)
  await go(page, 'Descargas')
  const card = page.locator('.card', { hasText: 'Versión en uso' })
  await expect(card).toBeVisible()
  await expect(card.locator('.yt-version')).toHaveText(yt.version || '—')
  if (yt.version) expect(yt.version).toMatch(/^\d{4}\.\d{1,2}\.\d{1,2}/)
  await expect(card.getByRole('button', { name: /Actualizar yt-dlp/ })).toBeEnabled()
  if (yt.js_runtime) await expect(card).toContainText(`Motor de JavaScript: ${yt.js_runtime}`)
  else await expect(card.locator('.yt-js')).toBeVisible()
})
