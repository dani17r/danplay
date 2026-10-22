// Los términos, en un navegador de verdad: salen solos la primera vez, la
// casilla no se puede marcar hasta bajar al final del texto (eso necesita
// alturas de verdad: jsdom no mide nada) y sin aceptarlos «Transformar» no
// se abre.
import { test, expect } from '@playwright/test'
import { rmSync } from 'node:fs'
import { join } from 'node:path'
import { reset, rows } from './helpers.js'
import { SONGS } from './songs.js'

// donde guarda el núcleo que se aceptaron (XDG_DATA_HOME/danplay, ver server.mjs)
const ACCEPTED = join(process.env.DANPLAY_E2E_ROOT || '', 'data', 'danplay', 'terms.json')

test.beforeEach(async ({ request }) => {
  await reset(request) // los deja aceptados: aquí se quitan
  rmSync(ACCEPTED, { force: true })
})

test.afterEach(async ({ request }) => {
  await reset(request)
})

test('the first time they show up on their own and must be read to the end', async ({ page }) => {
  // una ventana bajita: el texto no cabe y hay que bajarlo
  await page.setViewportSize({ width: 1100, height: 700 })
  await page.goto('/')
  await expect(rows(page)).toHaveCount(SONGS.length)
  const modal = page.getByRole('dialog', { name: 'Términos y condiciones de DanPlay' })
  await expect(modal).toBeVisible()
  await expect(modal).toContainText('Daniel Rangel')
  const checkbox = modal.getByRole('checkbox')
  const accept = modal.getByRole('button', { name: 'Aceptar' })
  await expect(checkbox).toBeDisabled()
  await expect(accept).toBeDisabled()

  // al final del texto
  await modal.locator('.terms-reader').evaluate((el) => {
    el.scrollTop = el.scrollHeight
    el.dispatchEvent(new Event('scroll'))
  })
  await expect(checkbox).toBeEnabled()
  await expect(accept).toBeDisabled()
  await checkbox.check()
  await accept.click()
  await expect(modal).toHaveCount(0)

  // aceptados: al volver a abrir ya no salen
  await page.reload()
  await expect(rows(page)).toHaveCount(SONGS.length)
  await expect(page.getByRole('dialog', { name: 'Términos y condiciones de DanPlay' })).toHaveCount(
    0
  )
})

test('without accepting them, «Transformar» asks for them and does not open', async ({ page }) => {
  await page.goto('/')
  await expect(rows(page)).toHaveCount(SONGS.length)
  const modal = page.getByRole('dialog', { name: 'Términos y condiciones de DanPlay' })
  // la primera vez: «Ahora no»
  await modal.getByRole('button', { name: 'Ahora no' }).click()
  await expect(modal).toHaveCount(0)

  await page.getByRole('button', { name: 'Transformar' }).click()
  await expect(modal).toBeVisible()
  await expect(modal).toContainText('Para ordenar y renombrar con «Transformar» necesitas')
  await modal.getByRole('button', { name: 'Ahora no' }).click()
  await expect(modal).toHaveCount(0)
  await expect(page.getByRole('dialog', { name: 'Transformar tu música' })).toHaveCount(0)
})
