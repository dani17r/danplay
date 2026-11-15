// Ajustar el clic manual de muchos pulsos a la vez, con la interfaz de verdad:
// elegir un tramo, escribir un tempo en «Tempo y onda», ver la prueba en la
// onda, aplicarla y deshacerla y rehacerla con el teclado.
//
// La canción de prueba es un tono (sin golpes que pegar): el ajuste a la onda
// lo cubren las pruebas de la interfaz con golpes inventados. Aquí vale que el
// camino entero funcione en Chrome: la selección, el panel, el guardado con la
// canción y las teclas.
import { test, expect } from '@playwright/test'
import { core, reset, rows, openApp, songsByTitle, withFilePath } from './helpers.js'

test.use({ viewport: { width: 1536, height: 864 } })

test.beforeEach(async ({ request }) => {
  await reset(request)
})

/** Su clic manual: 120 bpm (cada medio segundo) desde el segundo 1, un «1» cada cuatro. */
const OWN = {
  beats: Array.from({ length: 36 }, (_, i) => Math.round((1 + i * 0.5) * 1000) / 1000),
  bars: Array.from({ length: 9 }, (_, i) => i * 4),
  meter: 4,
  touched: []
}

const ownOf = async (request, id) => {
  const { study } = await core(request, 'GET', `/song/${id}`)
  return study ? JSON.parse(study).metronome?.own : undefined
}

test('select a segment, type a tempo, see the preview, apply, undo and redo', async ({
  page,
  request
}, testInfo) => {
  const byTitle = await songsByTitle(request)
  const id = byTitle['Mi Gozo'].id
  // el clic manual ya hecho: así hay pulsos que ajustar sin el detector del núcleo
  await core(request, 'PUT', `/song/${id}/study`, { metronome: { own: OWN } })
  await withFilePath(page)
  await page.addInitScript(() => {
    localStorage.setItem('danplay.click.mode', JSON.stringify('own'))
    localStorage.setItem('danplay.study.tourSeen', 'true')
  })
  try {
    await openApp(page)
    await rows(page).filter({ hasText: 'Mi Gozo' }).first().dblclick()
    await expect(page.locator('.player .title')).toHaveText('Mi Gozo')
    await page.locator('.player .pl-study').click()
    const study = page.getByRole('region', { name: 'Modo estudio' })
    await study.getByRole('button', { name: /Ajustar pulsos/ }).click()
    const tools = study.locator('.beat-tools')
    await expect(tools).toBeVisible()
    await expect(tools).toContainText('Ningún pulso elegido')

    // un tramo: los pulsos de 5 s a 15 s, con «Hasta el final» y los botones de la barra
    await tools.getByRole('button', { name: 'Todos', exact: true }).click()
    await expect(tools.locator('.bt-summary')).toContainText('36 pulsos')
    await tools.getByRole('button', { name: 'Ninguno', exact: true }).click()
    await expect(tools.locator('.bt-summary')).toContainText('Ningún pulso elegido')
    await page.keyboard.press('Control+a')
    await expect(tools.locator('.bt-summary')).toContainText('36 pulsos')

    // «Tempo y onda»: escribir 76 bpm para todo
    await tools.getByRole('button', { name: /Tempo y onda/ }).click()
    const panel = study.locator('.beat-align')
    await expect(panel).toBeVisible()
    await expect(panel.locator('.ba-span')).toContainText('36 pulsos')
    await panel.getByRole('radio', { name: 'Poner un tempo' }).click()
    await panel.getByLabel('Tempo del tramo, en pulsos por minuto').fill('76')
    // lo que se prueba se ve en la onda y NO se guarda
    await expect(panel.locator('.ba-text')).toContainText('tempo exacto')
    expect((await ownOf(request, id)).beats).toEqual(OWN.beats)
    await page.screenshot({ path: testInfo.outputPath('tempo-y-onda-1536x864.png') })

    await panel.getByRole('button', { name: 'Aplicar', exact: true }).click()
    await expect(panel).toBeHidden()
    await expect.poll(async () => (await ownOf(request, id)).beats.length).not.toBe(36)
    const applied = (await ownOf(request, id)).beats
    expect(applied[1] - applied[0]).toBeCloseTo(60 / 76, 2)
    await expect(tools.getByRole('button', { name: 'Deshacer' })).toHaveAttribute(
      'title',
      /Deshacer: tempo 76 bpm/
    )

    // Ctrl+Z lo deja como estaba y Ctrl+Mayús+Z lo vuelve a poner
    await page.keyboard.press('Control+z')
    await expect.poll(async () => (await ownOf(request, id)).beats).toEqual(OWN.beats)
    await page.keyboard.press('Control+Shift+z')
    await expect.poll(async () => (await ownOf(request, id)).beats).toEqual(applied)
    await page.screenshot({ path: testInfo.outputPath('herramientas-1536x864.png') })
  } finally {
    await core(request, 'PUT', `/song/${id}/study`, {})
  }
})
