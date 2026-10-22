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
const ACCEPTED = join(process.env.DANPLAY_E2E_ROOT || '', 'data', 'danplay', 'terminos.json')

test.beforeEach(async ({ request }) => {
  await reset(request) // los deja aceptados: aquí se quitan
  rmSync(ACCEPTED, { force: true })
})

test.afterEach(async ({ request }) => {
  await reset(request)
})

test('la primera vez salen solos, y hay que leerlos hasta el final', async ({ page }) => {
  // una ventana bajita: el texto no cabe y hay que bajarlo
  await page.setViewportSize({ width: 1100, height: 700 })
  await page.goto('/')
  await expect(rows(page)).toHaveCount(SONGS.length)
  const ventana = page.getByRole('dialog', { name: 'Términos y condiciones de DanPlay' })
  await expect(ventana).toBeVisible()
  await expect(ventana).toContainText('Daniel Rangel')
  const casilla = ventana.getByRole('checkbox')
  const aceptar = ventana.getByRole('button', { name: 'Aceptar' })
  await expect(casilla).toBeDisabled()
  await expect(aceptar).toBeDisabled()

  // al final del texto
  await ventana.locator('.terms-reader').evaluate((el) => {
    el.scrollTop = el.scrollHeight
    el.dispatchEvent(new Event('scroll'))
  })
  await expect(casilla).toBeEnabled()
  await expect(aceptar).toBeDisabled()
  await casilla.check()
  await aceptar.click()
  await expect(ventana).toHaveCount(0)

  // aceptados: al volver a abrir ya no salen
  await page.reload()
  await expect(rows(page)).toHaveCount(SONGS.length)
  await expect(page.getByRole('dialog', { name: 'Términos y condiciones de DanPlay' })).toHaveCount(
    0
  )
})

test('sin aceptarlos, «Transformar» los pide y no se abre', async ({ page }) => {
  await page.goto('/')
  await expect(rows(page)).toHaveCount(SONGS.length)
  const ventana = page.getByRole('dialog', { name: 'Términos y condiciones de DanPlay' })
  // la primera vez: «Ahora no»
  await ventana.getByRole('button', { name: 'Ahora no' }).click()
  await expect(ventana).toHaveCount(0)

  await page.getByRole('button', { name: 'Transformar' }).click()
  await expect(ventana).toBeVisible()
  await expect(ventana).toContainText('Para ordenar y renombrar con «Transformar» necesitas')
  await ventana.getByRole('button', { name: 'Ahora no' }).click()
  await expect(ventana).toHaveCount(0)
  await expect(page.getByRole('dialog', { name: 'Transformar tu música' })).toHaveCount(0)
})
