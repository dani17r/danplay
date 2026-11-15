// Ajustar el clic en bloque, sobre la onda de verdad: elegir unos pulsos con
// la franja de arriba y llevarlos juntos. Lo que se comprueba es lo que
// llega a la canción: los pulsos elegidos corren lo mismo (la separación entre
// ellos no cambia) y los demás se quedan donde estaban. La ventana es la de
// quien lo usa: 1536 x 864 a 125 %.
import { test, expect } from '@playwright/test'
import { core, reset, rows, titleTexts, openApp, songsByTitle } from './helpers.js'

test.use({ viewport: { width: 1536, height: 864 }, deviceScaleFactor: 1.25 })

test.beforeEach(async ({ request }) => {
  await reset(request)
})

/** Los pulsos del clic manual que tiene guardados la canción. */
const savedBeats = async (request, id) => {
  const { study } = await core(request, 'GET', `/song/${id}`)
  return study ? JSON.parse(study).metronome?.own?.beats || [] : []
}

test('box-selecting a few beats and dragging one moves the whole group, keeping its spacing', async ({
  page,
  request
}) => {
  // el clic manual de la canción (20 s): un pulso cada 0,5 s, desde el 0,5
  const beats = Array.from({ length: 38 }, (_, i) => 0.5 + 0.5 * i)
  const bars = Array.from({ length: 10 }, (_, k) => k * 4)
  await page.addInitScript(() => {
    // en «Manual», que es donde se ajustan los pulsos
    localStorage.setItem('danplay.click.mode', '"own"')
    // el recorrido del modo estudio, ya visto: si no, sale encima de la onda
    localStorage.setItem('danplay.study.tourSeen', 'true')
  })
  await openApp(page)
  const [first] = await titleTexts(page)
  const id = (await songsByTitle(request))[first].id
  try {
    await core(request, 'PUT', `/song/${id}/study`, {
      metronome: { own: { beats, bars, meter: 4 } }
    })
    await rows(page).first().dblclick()
    await expect(page.locator('.player .title')).toHaveText(first)
    await page.locator('.player .pl-study').click()
    const study = page.getByRole('region', { name: 'Modo estudio' })
    const edit = study.locator('.study-edit-beats')
    // La rejilla del clic solo la carga la app de escritorio: el reproductor
    // del navegador no dice la ruta del archivo (`state.path`) y sin ella el
    // botón se queda apagado. En cuanto el navegador la cargue, esto corre.
    const ready = await expect(edit)
      .toBeEnabled({ timeout: 8000 })
      .then(
        () => true,
        () => false
      )
    test.skip(!ready, 'sin rejilla del clic en el navegador (ver el comentario de arriba)')
    await edit.click()
    const beat = (i) => study.locator(`.tl-beat[data-i="${i}"]`)
    await expect(beat(9)).toBeVisible()
    const centre = async (i) => {
      const b = await beat(i).boundingBox()
      return b.x + b.width / 2
    }

    // arrastrar por la franja de arriba, del pulso 4 al 9: eligen seis
    const bar = await study.locator('.tl-beatbar').boundingBox()
    const y = bar.y + bar.height / 2
    await page.mouse.move((await centre(4)) - 4, y)
    await page.mouse.down()
    await page.mouse.move((await centre(9)) + 4, y, { steps: 10 })
    await page.mouse.up()
    await expect(study.locator('.tl-beat.sel')).toHaveCount(6)
    await expect(study.locator('.tl-selband')).toBeVisible()

    // llevar uno de ellos (el 6) a la derecha: se van los seis
    const box = await study.locator('.tl-box').boundingBox()
    const grab = box.y + bar.height + 14
    const x = await centre(6)
    await page.mouse.move(x, grab)
    await page.mouse.down()
    await page.mouse.move(x + 12, grab, { steps: 6 })
    await expect(study.locator('.tl-readout')).toContainText('ms')
    await page.mouse.up()

    await expect
      .poll(async () => (await savedBeats(request, id))[6])
      .toBeGreaterThan(beats[6] + 0.01)
    const saved = await savedBeats(request, id)
    expect(saved).toHaveLength(beats.length)
    const shift = saved[4] - beats[4]
    expect(shift).toBeGreaterThan(0.01)
    for (let i = 4; i <= 9; i++) expect(saved[i] - beats[i]).toBeCloseTo(shift, 2)
    // la separación entre ellos, la de antes; los demás, donde estaban
    for (let i = 5; i <= 9; i++) expect(saved[i] - saved[i - 1]).toBeCloseTo(0.5, 2)
    beats.forEach((t, i) => {
      if (i < 4 || i > 9) expect(saved[i]).toBeCloseTo(t, 3)
    })
  } finally {
    await core(request, 'PUT', `/song/${id}/study`, {})
  }
})
