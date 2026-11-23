// Las barras de desplazamiento salen solo donde el contenido no cabe. Antes
// la franja de pestañas de «Acerca de» y el hilo vacío del asistente pintaban
// una barra por 1 y 2 píxeles de fracciones de medida.
import { test, expect } from '@playwright/test'
import { reset, openApp } from './helpers.js'

test.beforeEach(async ({ request }) => {
  await reset(request)
})

test('the About tabs do not scroll: they fit', async ({ page }) => {
  await openApp(page)
  await page.locator('.sidebar .nav-link').filter({ hasText: 'Acerca de' }).first().click()
  const tabs = page.locator('.about-tabs')
  await expect(tabs).toBeVisible()
  const { scroll, client } = await tabs.evaluate((el) => ({
    scroll: el.scrollHeight,
    client: el.clientHeight
  }))
  expect(scroll, 'sin barra vertical').toBeLessThanOrEqual(client)
})

test('an empty assistant has no scrollbar', async ({ page }) => {
  await openApp(page)
  await page.locator('.sidebar .nav-link').filter({ hasText: 'Asistente' }).first().click()
  const thread = page.locator('.chat-thread')
  await expect(thread).toBeVisible()
  await expect(thread).toHaveCSS('overflow-y', 'hidden')
})
